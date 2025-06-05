use crate::app::config::CONFIG;
use super::entity::{Request, Response};


pub struct Cors {
	origin: String
}

impl Cors {
	pub fn new (req: &Request) -> Self {
		Cors {
			origin: req.headers.get("origin").unwrap_or("*".to_string())
		}
	}

	pub fn apply_origin_check (self, res: &mut Response) {
		let allow_origin = if self.origin.is_empty() {
			CONFIG.cors.origin.clone()
		} else {
			self.origin
		};

		res.headers.set("Access-Control-Allow-Origin".to_string(), allow_origin);
	}

	pub fn apply_normal (self, res: &mut Response) {
		res.headers.set("Access-Control-Expose-Headers".to_string(), CONFIG.cors.headers.clone());
		self.apply_origin_check(res);
	}

	pub fn apply_preflight (self, res: &mut Response) {
		res.headers.set("Access-Control-Allow-Methods".to_string(), CONFIG.cors.methods.clone());
		res.headers.set("Access-Control-Allow-Headers".to_string(), CONFIG.cors.headers.clone());
		res.headers.set("Access-Control-Max-Age".to_string(), CONFIG.cors.ttl.clone());
		self.apply_origin_check(res);
	}
}
