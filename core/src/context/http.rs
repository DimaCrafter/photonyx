use std::{collections::HashMap, ffi::c_void, net::IpAddr, ptr::null_mut};
use json::{object, JsonValue};
use crate::{app::config::CONFIG, context::session::Session, http::{codes::HttpCode, entity::{HttpConnection, HttpHeaders, Request, Response, ResponseRet, ResponseType}}, utils::log::log_warning};
use crate::utils::validator::*;


#[derive(Debug)]
pub struct HttpContext {
	pub(crate) _meta_ptr: *mut c_void,

	pub req: Request,
	pub res: Response,
	pub params: HashMap<String, String>,
	pub address: IpAddr,
	session: Option<Session>
}

impl HttpContext {
	pub fn from<Connection: HttpConnection> (connection: &Connection, req: Request, params: HashMap<String, String>) -> Self {
		HttpContext {
			req,
			res: Response {
				code: HttpCode::NotSent,
				headers: HttpHeaders::empty(),
				payload: ResponseType::NoContent
			},
			params,
			address: connection.get_address(),
			session: None,
			_meta_ptr: null_mut()
		}
	}

	pub fn get_session (&mut self) -> &mut Session {
		if self.session.is_none() {
			if CONFIG.session.is_none() {
				log_warning("Using `HttpContext::get_session` when sessions are disabled!");
				return Session::empty_ref();
			}

			if let Some(session_header) = self.get_header("session") {
				// In case that session token is invalid or expired it's better to serve
				// the client with a new session rather than just throwing an error.
				self.session = Session::decode(&session_header).or_else(|| Some(Session::new()));
			} else {
				self.session = Some(Session::new());
			}
		}

		return self.session.as_mut().unwrap();
	}

	#[inline]
	pub fn get_header (&self, name: &str) -> Option<&str> {
		return self.req.headers.get(name);
	}

	#[inline]
	pub fn get_header_default<'a> (&'a self, name: &str, default: &'a str) -> &'a str {
		return self.get_header(name).unwrap_or(default);
	}

	#[inline]
	pub fn set_header (&mut self, name: &str, value: String) {
		self.res.headers.set(name.to_string(), value);
	}

	pub fn validate_json<T: Validate + ValidateJson + Default> (&mut self) -> ResponseRet<T> {
		let body_str = match str::from_utf8(self.req.body.as_slice()) {
			Ok(value) => value,
			Err(error) => return self.json_status(
				object! { "type": "ValidationError", "message": error.to_string() },
				HttpCode::BadRequest
			)
		};

		match validate_json::<T>(body_str) {
			Ok(payload) => ResponseRet::Result(payload),
			Err(error) => self.json_status(
				object! { "type": "ValidationError", "message": error.message, "path": error.path },
				HttpCode::BadRequest
			)
		}
	}

	#[inline]
	pub fn json (&mut self, data: JsonValue) -> ResponseRet {
		return self.json_status(data, HttpCode::OK);
	}

	#[inline]
	pub fn json_status<T> (&mut self, data: JsonValue, code: HttpCode) -> ResponseRet<T> {
		self.res.code = code;
		self.res.headers.set_default("content-type".to_owned(), "application/json".to_owned());
		self.res.payload = ResponseType::Payload(data.dump().into());

		return ResponseRet::Return;
	}

	#[inline]
	pub fn text (&mut self, message: &str) -> ResponseRet {
		return self.text_status(message, HttpCode::OK);
	}

	#[inline]
	pub fn text_status (&mut self, message: &str, code: HttpCode) -> ResponseRet {
		self.res.code = code;
		self.res.headers.set_default("content-type".to_owned(), "text/plain".to_owned());
		self.res.payload = ResponseType::Payload(message.into());

		return ResponseRet::Return;
	}

	pub fn redirect (&mut self, target: &str) -> ResponseRet {
		self.res.code = HttpCode::TemporaryRedirect;
		self.res.headers.set("location".to_string(), target.to_string());
		self.res.payload = ResponseType::NoContent;

		return ResponseRet::Return;
	}

	#[inline]
	pub fn drop (self) -> ResponseRet {
		return ResponseRet::Replace(Response::drop());
	}

	pub fn finalize (mut self) -> Response {
		if let Some(session) = &self.session {
			if session.need_header_refresh {
				self.set_header("session", session.crypt());
			}
		}

		return self.res;
	}
}
