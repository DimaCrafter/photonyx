use crate::http::{cors::normalize_origin, proxy::IpNet};
use crate::utils::{
    bake_fatal, json_read_array,
    log::{log_error, log_error_lines},
    parse_duration,
    sync::{AppStatic, LazyInit},
};
use json::{object, JsonValue};
use std::{
    env, fs,
    ops::{Index, IndexMut},
    path::Path,
    process,
    str::FromStr,
    time::Duration,
};

pub static CONFIG: AppStatic<Config> = AppStatic::new();

pub struct Config {
    obj: JsonValue,

    pub http: HttpConfig,
    pub session: Option<SessionConfig>,
}

impl Config {
    pub fn default() -> Self {
        Config {
            obj: object! {},
            http: HttpConfig::default(),
            session: None,
        }
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) {
        let raw = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                log_error_lines("Config reading error", err.to_string());
                process::exit(-1);
            }
        };

        match json::parse(&raw) {
            Ok(value) => {
                self.obj = value;
            }
            Err(err) => {
                log_error_lines("Config parsing error", err.to_string());
                process::exit(-1);
            }
        };

        self.http.load(&self.obj["http"]);

        if let Some(port) = Config::parse_env("PORT") {
            self.http.port = port;
        }

        if self.obj["session"].is_object() {
            let mut session = SessionConfig::default();
            session.load(&self.obj);
            self.session = Some(session);
        }
    }

    pub fn get_path(&self, path: Vec<&str>) -> &JsonValue {
        let mut anchor = &self.obj;
        for part in path {
            if anchor.is_null() {
                break;
            }

            anchor = &anchor[part];
        }

        return anchor;
    }

    #[inline]
    pub fn get_env(key: &str) -> Option<String> {
        return env::var(key).ok();
    }

    pub fn parse_env<T: FromStr>(key: &str) -> Option<T> {
        let mut result = None;
        if let Ok(value_str) = env::var(key) {
            if let Ok(parsed) = value_str.parse::<T>() {
                result = Some(parsed);
            }
        }

        return result;
    }
}

impl LazyInit for Config {
    fn init() -> Self {
        let mut config = Config::default();
        config.load("config.json");
        return config;
    }
}

impl Index<&str> for Config {
    type Output = JsonValue;

    fn index(&self, index: &str) -> &Self::Output {
        return &self.obj[index];
    }
}

impl IndexMut<&str> for Config {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        return &mut self.obj[index];
    }
}

pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    /// Limit for the request line and headers, bytes
    pub max_header_size: usize,
    /// Limit for the request body, bytes
    pub max_body_size: usize,
    /// Time to receive the whole request
    pub read_timeout: Duration,
    /// Time for a single write to the client
    pub write_timeout: Duration,
    /// Trusted CIDRs to parse `X-Forwarded-For`
    pub trusted_proxies: Vec<IpNet>,
    pub cors: CorsConfig,
}

impl HttpConfig {
    pub fn default() -> Self {
        HttpConfig {
            host: "127.0.0.1".to_owned(),
            port: 8081,
            max_header_size: 16 * 1024,
            max_body_size: 1024 * 1024,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            trusted_proxies: Vec::new(),
            cors: CorsConfig::default(),
        }
    }

    fn load(&mut self, http: &JsonValue) {
        if let Some(host) = http["host"].as_str() {
            self.host = host.to_owned();
        }

        if let Some(port) = http["port"].as_u16() {
            self.port = port;
        }

        if let Some(size) = http["maxHeaderSize"].as_usize() {
            self.max_header_size = size;
        }

        if let Some(size) = http["maxBodySize"].as_usize() {
            self.max_body_size = size;
        }

        if let Some(timeout) = read_timeout_option(http, "readTimeout") {
            self.read_timeout = timeout;
        }

        if let Some(timeout) = read_timeout_option(http, "writeTimeout") {
            self.write_timeout = timeout;
        }

        self.trusted_proxies = read_trusted_proxies(&http["trustedProxies"]);
        self.cors.load(&http["cors"]);
    }
}

pub struct CorsConfig {
    pub any_origin: bool,
    pub origins: Vec<String>,
    pub methods: String,
    pub headers: String,
    pub ttl: String,
}

impl CorsConfig {
    pub const fn default() -> Self {
        CorsConfig {
            any_origin: false,
            origins: Vec::new(),
            methods: String::new(),
            headers: String::new(),
            ttl: String::new(),
        }
    }

    fn load(&mut self, cors: &JsonValue) {
        self.read_origins(&cors["origins"]);

        let methods = json_read_array(
            &cors["methods"],
            JsonValue::as_str,
            bake_fatal("Config parsing error: http.cors.methods[...] must be a string"),
        );

        if let Some(list) = methods {
            self.methods = list.join(",");
        } else {
            self.methods = "GET,POST".to_owned();
        }

        let headers = json_read_array(
            &cors["headers"],
            JsonValue::as_str,
            bake_fatal("Config parsing error: http.cors.headers[...] must be a string"),
        );

        if let Some(list) = headers {
            self.headers = list.join(",");
        } else {
            self.headers = "content-type,session".to_owned();
        }

        if let Some(ttl) = cors["ttl"].as_u32() {
            self.ttl = ttl.to_string();
        } else {
            self.ttl = "86400".to_owned();
        }
    }
}

impl CorsConfig {
    fn read_origins(&mut self, value: &JsonValue) {
        let fail = |got: String| -> ! {
            log_error(&format!("Config parsing error: `http.cors.origins` must be an array of `\"*\"` or origins like `https://example.com`, got `{got}`"));
            process::exit(-1);
        };

        let list = match value {
            JsonValue::Null => return,
            JsonValue::Array(list) => list,
            other => fail(other.dump()),
        };

        for item in list {
            match item.as_str() {
                Some("*") => self.any_origin = true,
                Some(text) => self
                    .origins
                    .push(normalize_origin(text).unwrap_or_else(|| fail(item.dump()))),
                None => fail(item.dump()),
            }
        }
    }
}

fn read_trusted_proxies(value: &JsonValue) -> Vec<IpNet> {
    let fail = |got: String| -> ! {
        log_error(&format!("Config parsing error: `http.trustedProxies` must be an array of addresses or subnets like `10.0.0.0/8`, got `{got}`"));
        process::exit(-1);
    };

    let list = match value {
        JsonValue::Null => return Vec::new(),
        JsonValue::Array(list) => list,
        other => fail(other.dump()),
    };

    return list
        .iter()
        .map(|item| {
            item.as_str()
                .and_then(IpNet::parse)
                .unwrap_or_else(|| fail(item.dump()))
        })
        .collect();
}

fn read_timeout_option(http: &JsonValue, key: &str) -> Option<Duration> {
    let value = http[key].as_str()?;

    match parse_duration(value) {
        Some(duration) if !duration.is_zero() => Some(duration),
        _ => {
            log_error(&format!(
                "Config parsing error: `http.{key}` must be a positive duration like `30s`"
            ));
            process::exit(-1);
        }
    }
}

pub struct SessionConfig {
    pub ttl: Duration,
    pub secret: String,
    pub store: String,
    pub store_arg: String,
}

impl SessionConfig {
    pub const fn default() -> Self {
        SessionConfig {
            ttl: Duration::ZERO,
            secret: String::new(),
            store: String::new(),
            store_arg: String::new(),
        }
    }

    fn load(&mut self, config: &JsonValue) {
        let ttl_json = &config["session"]["ttl"];
        if let Some(ttl_str) = ttl_json.as_str() {
            if let Some(ttl) = parse_duration(ttl_str) {
                self.ttl = ttl;
            }
        }

        if let Some(secret) = config["session"]["secret"].as_str() {
            self.secret = secret.to_owned();
        } else {
            log_error("Config parsing error: `session.secret` is required when `session` present");
            process::exit(-1);
        }

        if let Some(store) = config["session"]["store"].as_str() {
            let (name, arg) = match store.split_once(':') {
                Some(pair) => pair,
                None => (store, ""),
            };

            self.store = name.to_owned();
            self.store_arg = arg.to_owned();
        } else {
            log_error("Config parsing error: `session.store` is required when `session` present");
            process::exit(-1);
        }
    }
}
