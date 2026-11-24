use super::entity::{HttpMethod, Request, Response};
use crate::app::config::{CorsConfig, CONFIG};

pub struct Cors {
    origin: Option<String>,
}

impl Cors {
    pub fn new(req: &Request) -> Self {
        Cors {
            origin: req.headers.get("origin").map(str::to_owned),
        }
    }

    fn apply_origin_check(&self, res: &mut Response) {
        let Some(origin) = self.origin.as_deref() else {
            return;
        };

        res.headers.set("Vary".to_string(), "Origin".to_string());

        if CONFIG.http.cors.is_trusted(origin) {
            res.headers
                .set("Access-Control-Allow-Origin".to_string(), origin.to_owned());
        }
    }

    pub fn apply_normal(&self, res: &mut Response) {
        res.headers.set(
            "Access-Control-Expose-Headers".to_string(),
            CONFIG.http.cors.headers.clone(),
        );
        self.apply_origin_check(res);
    }

    pub fn apply_preflight(&self, res: &mut Response) {
        res.headers.set(
            "Access-Control-Allow-Methods".to_string(),
            CONFIG.http.cors.methods.clone(),
        );
        res.headers.set(
            "Access-Control-Allow-Headers".to_string(),
            CONFIG.http.cors.headers.clone(),
        );
        res.headers.set(
            "Access-Control-Max-Age".to_string(),
            CONFIG.http.cors.ttl.clone(),
        );
        self.apply_origin_check(res);
    }
}

impl CorsConfig {
    pub fn is_trusted(&self, origin: &str) -> bool {
        return self.any_origin || self.origins.iter().any(|trusted| trusted == origin);
    }
}

pub fn is_request_allowed(req: &Request, cors: &CorsConfig) -> bool {
    if matches!(req.method, HttpMethod::GET | HttpMethod::OPTIONS) {
        return true;
    }

    return is_unsafe_request_allowed(
        req.headers.get("sec-fetch-site"),
        req.headers.get("origin"),
        req.headers.get("host"),
        cors,
    );
}

fn is_unsafe_request_allowed(
    fetch_site: Option<&str>,
    origin: Option<&str>,
    host: Option<&str>,
    cors: &CorsConfig,
) -> bool {
    let is_trusted = || cors.any_origin || origin.is_some_and(|value| cors.is_trusted(value));

    match fetch_site {
        Some("same-origin" | "none") => return true,
        Some(_) => return is_trusted(),
        None => {}
    }

    let Some(origin) = origin else {
        // Either same-origin or not a browser
        return true;
    };

    let same_host = match (origin.split_once("://"), host) {
        (Some((_, origin_host)), Some(host)) => origin_host.eq_ignore_ascii_case(host),
        _ => false,
    };

    return same_host || is_trusted();
}

pub fn normalize_origin(text: &str) -> Option<String> {
    let (scheme, host) = text.split_once("://")?;

    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let host_ok = !host.is_empty() && !host.contains(['/', '?', '#', '@', ' ']);

    if !scheme_ok || !host_ok {
        return None;
    }

    return Some(text.to_ascii_lowercase());
}

#[cfg(test)]
mod tests {
    use super::{is_unsafe_request_allowed, normalize_origin};
    use crate::app::config::CorsConfig;

    fn config(origins: &[&str]) -> CorsConfig {
        let mut config = CorsConfig::default();
        config.any_origin = origins.contains(&"*");
        config.origins = origins
            .iter()
            .filter(|origin| **origin != "*")
            .map(|origin| origin.to_string())
            .collect();
        return config;
    }

    fn allowed(fetch_site: Option<&str>, origin: Option<&str>, origins: &[&str]) -> bool {
        return is_unsafe_request_allowed(
            fetch_site,
            origin,
            Some("lg.rifty.org"),
            &config(origins),
        );
    }

    #[test]
    fn fetch_site() {
        assert!(allowed(
            Some("same-origin"),
            Some("https://lg.rifty.org"),
            &[]
        ));
        assert!(allowed(Some("none"), None, &[]));
        assert!(!allowed(Some("same-site"), Some("https://rifty.org"), &[]));
        assert!(!allowed(
            Some("cross-site"),
            Some("https://evil.example"),
            &[]
        ));
        assert!(!allowed(Some("cross-site"), None, &[]));
        // The header wins over a matching origin
        assert!(!allowed(
            Some("cross-site"),
            Some("https://lg.rifty.org"),
            &[]
        ));
    }

    #[test]
    fn trusted_origins() {
        assert!(allowed(
            Some("cross-site"),
            Some("https://app.example"),
            &["https://app.example"]
        ));
        assert!(!allowed(
            Some("cross-site"),
            Some("https://evil.example"),
            &["https://app.example"]
        ));
        assert!(!allowed(
            Some("cross-site"),
            Some("http://app.example"),
            &["https://app.example"]
        ));
        assert!(allowed(
            Some("cross-site"),
            Some("https://evil.example"),
            &["*"]
        ));
        assert!(allowed(Some("cross-site"), None, &["*"]));
        assert!(allowed(None, Some("https://evil.example"), &["*"]));
        assert!(allowed(
            None,
            Some("https://app.example"),
            &["https://app.example"]
        ));
    }

    #[test]
    fn older_browsers() {
        assert!(allowed(None, None, &[]));
        assert!(allowed(None, Some("https://lg.rifty.org"), &[]));
        assert!(allowed(None, Some("http://LG.rifty.org"), &[]));
        assert!(!allowed(None, Some("https://evil.example"), &[]));
        assert!(!allowed(
            None,
            Some("https://lg.rifty.org.evil.example"),
            &[]
        ));
        assert!(!allowed(None, Some("https://lg.rifty.org:8443"), &[]));
        assert!(!allowed(None, Some("null"), &[]));
        assert!(!is_unsafe_request_allowed(
            None,
            Some("https://lg.rifty.org"),
            None,
            &config(&[])
        ));
    }

    #[test]
    fn origins_in_config() {
        assert_eq!(
            normalize_origin("https://Example.com:8080").as_deref(),
            Some("https://example.com:8080")
        );
        assert_eq!(
            normalize_origin("http://[::1]:3000").as_deref(),
            Some("http://[::1]:3000")
        );
        assert!(normalize_origin("example.com").is_none());
        assert!(normalize_origin("https://").is_none());
        assert!(normalize_origin("https://example.com/").is_none());
        assert!(normalize_origin("https://example.com?a").is_none());
        assert!(normalize_origin("https://user@example.com").is_none());
        assert!(normalize_origin("://example.com").is_none());
        assert!(normalize_origin("null").is_none());
    }
}
