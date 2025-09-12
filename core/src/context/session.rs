use std::{ops::{Index, IndexMut}, sync::OnceLock, time::{SystemTime, UNIX_EPOCH}};
use bindings::session::SessionStoreImpl;
use hmac::{Hmac, Mac};
use json::{object, JsonValue};
use sha2::Sha256;
use crate::{app::config::CONFIG, utils::system_as_unix_num};

type Hs256 = Hmac<Sha256>;


pub(crate) static SESSION_STORE: OnceLock<Box<dyn SessionStoreImpl>> = OnceLock::new();

pub fn init_session_store (store: Box<dyn SessionStoreImpl>) {
	SESSION_STORE.get_or_init(|| store);
}


#[derive(Debug)]
pub struct Session {
	inner: JsonValue,
	pub need_header_refresh: bool
}

static mut NULL_SESSION: Session = Session { inner: JsonValue::Null, need_header_refresh: false };

impl Session {
	pub fn new () -> Self {
		Session { inner: object! {}, need_header_refresh: false }
	}

	pub fn empty_ref () -> &'static mut Self {
		// SAFETY: any mutable calls on `JsonValue::Null` is ignored
		#[allow(static_mut_refs)]
		return unsafe { &mut NULL_SESSION };
	}

	/// Internally used. Please, use indexing instead of this method.
	#[inline]
	pub fn get_data (&mut self) -> &mut JsonValue {
		return &mut self.inner["data"];
	}

	#[inline]
	pub fn is_none (&self) -> bool {
		return self.inner.len() == 0;
	}


	pub fn crypt (&self) -> String {
		let session_cfg = CONFIG.session.as_ref().expect("Sessions was not configured!");
		let expires_at_unix = SystemTime::now().checked_add(session_cfg.ttl).unwrap().duration_since(UNIX_EPOCH).unwrap();

		let mut result = object! {
			id: self.inner["id"].clone(),
			expires: expires_at_unix.as_millis() as f64
		};

		result["sign"] = JsonValue::String(sign_session_token(&result));
		return result.dump();
	}

	pub fn decode (token_str: &str) -> Option<Self> {
		let mut token = json::parse(token_str).ok()?;

		let sign = token.remove("sign");
		let sign_raw = base64::decode(sign.as_str()?.as_bytes()).ok()?;

		if !verify_session_token(&token, &sign_raw) {
			return None;
		}

		if let Some(expiry) = token["expires"].as_f64() {
			if expiry <= system_as_unix_num(&SystemTime::now()) {
				return None;
			}
		}

		return Some(Session {
			inner: object! { id: token.remove("id"), expires: token.remove("expires"), data: null },
			need_header_refresh: false
		});
	}

	pub fn save (&mut self) {
		let store = SESSION_STORE.get().unwrap();
		if self["id"].is_null() {
			let expires = self["expires"].as_f64().unwrap();
			store.create(&mut self.inner, expires);
			self.need_header_refresh = true;
		} else {
			store.update(&mut self.inner);
		}
	}

	pub fn destroy (&mut self) {
		let store = SESSION_STORE.get().unwrap();
		store.destroy(self.get_data());

		self.need_header_refresh = true;
	}
}

impl Index<&str> for Session {
	type Output = JsonValue;

	#[inline]
	fn index (&self, index: &str) -> &Self::Output {
		return &self.inner["data"][index];
	}
}

impl IndexMut<&str> for Session {
	#[inline]
	fn index_mut (&mut self, index: &str) -> &mut Self::Output {
		return &mut self.inner["data"][index];
	}
}


fn new_session_hasher () -> Hs256 {
	let session_cfg = CONFIG.session.as_ref().expect("Sessions was not configured!");
	return Hs256::new_from_slice(session_cfg.secret.as_bytes()).expect("Invalid session.secret");
}

pub fn sign_session_token (token: &JsonValue) -> String {
	let mut hash = new_session_hasher();
	hash.update(token.dump().as_bytes());

	let sign_raw = hash.finalize().into_bytes();
	return base64::encode(sign_raw);
}

pub fn verify_session_token (token_with_no_sign: &JsonValue, sign: &[u8]) -> bool {
	let mut hash = new_session_hasher();
	hash.update(token_with_no_sign.dump().as_bytes());

	return hash.verify_slice(sign).is_ok();
}
