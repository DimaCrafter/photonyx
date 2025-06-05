use crate::utils::{bake_fatal, json_read_array, log::log_error_lines, sync::{AppStatic, LazyInit}};
use json::{object, JsonValue};
use std::{env, fs, ops::{Index, IndexMut}, path::Path, process, str::FromStr};


pub static CONFIG: AppStatic<Config> = AppStatic::new();

pub struct Config {
    obj: JsonValue,

    pub host: String,
    pub port: u16,
    pub cors: CorsConfig
}

impl Config {
    pub fn default () -> Self {
        Config {
            obj: object! {},
            host: "127.0.0.1".to_owned(),
            port: 8081,
            cors: CorsConfig::default()
        }
    }

    pub fn load<P: AsRef<Path>> (&mut self, path: P) {
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

        if let Some(port) = Config::parse_env("PORT") {
            self.port = port;
        } else if let Some(port) = self.obj["port"].as_u16() {
            self.port = port;
        }

        if let Some(host) = self.obj["host"].as_str() {
            self.host = host.to_owned();
        }

        self.cors.load(&self.obj);
    }

    #[inline]
    pub fn get_env (key: &str) -> Option<String> {
        return env::var(key).ok();
    }

    pub fn parse_env<T: FromStr> (key: &str) -> Option<T> {
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
    fn init () -> Self {
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

pub struct CorsConfig {
	pub origin: String,
	pub methods: String,
	pub headers: String,
	pub ttl: String
}

impl CorsConfig {
    pub const fn default () -> Self {
        CorsConfig {
            origin: String::new(),
            methods: String::new(),
            headers: String::new(),
            ttl: String::new()
        }
    }

	fn load (&mut self, config: &JsonValue) {
        if let Some(origin) = config["cors"]["origin"].as_str() {
            self.origin = origin.to_owned();
        }

		let methods = json_read_array(
			&config["cors"]["methods"],
			JsonValue::as_str,
			bake_fatal("Config parsing error: cors.methods[...] must be a string")
		);

        if let Some(list) = methods {
            self.methods = list.join(",");
        } else {
            self.methods = "GET,POST".to_owned();
        }

		let headers = json_read_array(
			&config["cors"]["headers"],
			JsonValue::as_str,
			bake_fatal("Config parsing error: cors.headers[...] must be a string")
		);

        if let Some(list) = headers {
            self.headers = list.join(",");
        } else {
            self.headers = "content-type,session".to_owned();
        }

        if let Some(ttl) = config["cors"]["ttl"].as_u32() {
            self.ttl = ttl.to_string();
        } else {
            self.ttl = "86400".to_owned();
        }
	}
}
