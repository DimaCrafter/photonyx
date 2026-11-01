use crate::app::config::CONFIG;
use super::entity::{Request, Response};


pub struct Cors {
	origin: Option<String>
}

impl Cors {
	pub fn new (req: &Request) -> Self {
		Cors {
			origin: req.headers.get("origin").map(str::to_owned)
		}
	}

	fn apply_origin_check (&self, res: &mut Response) {
		let (allow_origin, reflected) = resolve_origin(&CONFIG.cors.origin, self.origin.as_deref());
		res.headers.set("Access-Control-Allow-Origin".to_string(), allow_origin);

		if reflected {
			res.headers.set("Vary".to_string(), "Origin".to_string());
		}
	}

	pub fn apply_normal (&self, res: &mut Response) {
		res.headers.set("Access-Control-Expose-Headers".to_string(), CONFIG.cors.headers.clone());
		self.apply_origin_check(res);
	}

	pub fn apply_preflight (&self, res: &mut Response) {
		res.headers.set("Access-Control-Allow-Methods".to_string(), CONFIG.cors.methods.clone());
		res.headers.set("Access-Control-Allow-Headers".to_string(), CONFIG.cors.headers.clone());
		res.headers.set("Access-Control-Max-Age".to_string(), CONFIG.cors.ttl.clone());
		self.apply_origin_check(res);
	}
}

/// Returns the allowed origin and whether it depends on the request.
/// `cors.origin` from the config has priority, without it any origin is allowed.
fn resolve_origin (configured: &str, requested: Option<&str>) -> (String, bool) {
	if !configured.is_empty() {
		return (configured.to_owned(), false);
	}

	return match requested {
		Some(origin) => (origin.to_owned(), true),
		None => ("*".to_owned(), false)
	};
}

#[cfg(test)]
mod tests {
	use super::resolve_origin;

	#[test]
	fn configured_origin_has_priority () {
		assert_eq!(resolve_origin("https://a.test", Some("https://b.test")), ("https://a.test".to_owned(), false));
		assert_eq!(resolve_origin("https://a.test", None), ("https://a.test".to_owned(), false));
	}

	#[test]
	fn request_origin_is_reflected_by_default () {
		assert_eq!(resolve_origin("", Some("https://b.test")), ("https://b.test".to_owned(), true));
		assert_eq!(resolve_origin("", None), ("*".to_owned(), false));
	}
}
