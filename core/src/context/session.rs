use hmac::KeyInit;
use std::{ops::{Index, IndexMut}, sync::OnceLock, time::{SystemTime, UNIX_EPOCH}};
use base64::{prelude::BASE64_STANDARD, Engine};
use bindings::session::SessionStoreImpl;
use hmac::{Hmac, Mac};
use json::{object, JsonValue};
use sha2::Sha256;
use crate::{app::config::CONFIG, utils::{log::log_error, system_as_unix_num}};

type Hs256 = Hmac<Sha256>;


pub(crate) static SESSION_STORE: OnceLock<Box<dyn SessionStoreImpl>> = OnceLock::new();

pub fn init_session_store (store: Box<dyn SessionStoreImpl>) {
	SESSION_STORE.get_or_init(|| store);
}


#[derive(Debug)]
pub struct Session {
	id: Option<u64>,
	data: JsonValue,
	/// Data as it was loaded, to save only changed sessions
	loaded: JsonValue,
	pub need_header_refresh: bool
}

static mut NULL_SESSION: Session = Session {
	id: None,
	data: JsonValue::Null,
	loaded: JsonValue::Null,
	need_header_refresh: false
};

impl Session {
	pub fn new () -> Self {
		Session {
			id: None,
			data: object!{},
			loaded: object!{},
			need_header_refresh: false
		}
	}

	pub fn empty_ref () -> &'static mut Self {
		// SAFETY: any mutable calls on `JsonValue::Null` is ignored
		#[allow(static_mut_refs)]
		return unsafe { &mut NULL_SESSION };
	}

	/// Internally used. Please, use indexing instead of this method.
	#[inline]
	pub fn get_data (&mut self) -> &mut JsonValue {
		return &mut self.data;
	}

	#[inline]
	pub fn is_none (&self) -> bool {
		return self.data.is_empty();
	}

	/// Returns token for the `session` header
	pub fn crypt (&self) -> String {
		let Some(id) = self.id else {
			return String::new();
		};

		let mut result = object! {
			id: id,
			expires: expires_at() as f64
		};

		result["sign"] = JsonValue::String(sign_session_token(&result));
		return result.dump();
	}

	pub fn decode (token_str: &str) -> Self {
		let Some(id) = verify_token(token_str) else {
			return Session::new();
		};

		let store = SESSION_STORE.get().unwrap();
		match store.load(id) {
			Ok(Some(data)) if data.is_object() => {
				return Session { id: Some(id), loaded: data.clone(), data, need_header_refresh: false };
			}
			Ok(_) => return Session::new(),
			Err(error) => {
				log_error(&format!("Session store: {error}"));
				return Session::new();
			}
		}
	}

	pub fn commit (&mut self) {
		if self.data.is_null() || self.data == self.loaded {
			return;
		}

		let store = SESSION_STORE.get().unwrap();
		let result = match self.id {
			None if self.data.is_empty() => Ok(()),
			None => store.create(&self.data, expires_at()).map(|id| self.id = Some(id)),
			Some(id) if self.data.is_empty() => store.destroy(id).map(|_| self.id = None),
			Some(id) => store.update(id, &self.data, expires_at())
		};

		match result {
			Ok(()) => {
				self.loaded = self.data.clone();
				self.need_header_refresh = true;
			}
			Err(error) => log_error(&format!("Session store: {error}"))
		}
	}

	pub fn destroy (&mut self) {
		if !self.data.is_null() {
			self.data = object! {};
		}
	}
}

fn expires_at () -> u64 {
	let session_cfg = CONFIG.session.as_ref().expect("Sessions was not configured!");
	return SystemTime::now()
		.checked_add(session_cfg.ttl)
		.unwrap()
		.duration_since(UNIX_EPOCH)
		.unwrap()
		.as_millis() as u64;
}

fn verify_token (token_str: &str) -> Option<u64> {
	let mut token = json::parse(token_str).ok()?;

	let sign = token.remove("sign");
	let sign_raw = BASE64_STANDARD.decode(sign.as_str()?.as_bytes()).ok()?;

	if !verify_session_token(&token, &sign_raw) {
		return None;
	}

	if let Some(expiry) = token["expires"].as_f64() {
		if expiry <= system_as_unix_num(&SystemTime::now()) {
			return None;
		}
	}

	return token["id"].as_u64();
}

impl Index<&str> for Session {
	type Output = JsonValue;

	#[inline]
	fn index (&self, index: &str) -> &Self::Output {
		return &self.data[index];
	}
}

impl IndexMut<&str> for Session {
	#[inline]
	fn index_mut (&mut self, index: &str) -> &mut Self::Output {
		return &mut self.data[index];
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
	return BASE64_STANDARD.encode(sign_raw);
}

pub fn verify_session_token (token_with_no_sign: &JsonValue, sign: &[u8]) -> bool {
	let mut hash = new_session_hasher();
	hash.update(token_with_no_sign.dump().as_bytes());

	return hash.verify_slice(sign).is_ok();
}
