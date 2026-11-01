use std::{collections::HashMap, ffi::c_void, fmt, net::IpAddr, ptr::null_mut};
use json::{object, JsonValue};
use crate::{app::config::CONFIG, context::session::Session, http::{codes::HttpCode, cors::Cors, entity::{HttpHeaders, Request, Response, ResponseExit, ResponseRet, ResponseSink, ResponseType}}, utils::log::log_warning};
use crate::utils::validator::*;


pub struct ResponseTarget<'a> {
	pub sink: &'a mut dyn ResponseSink,
	pub cors: &'a Cors
}

impl fmt::Debug for ResponseTarget<'_> {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		return f.write_str("ResponseTarget");
	}
}

#[derive(Debug)]
pub struct HttpContext<'a> {
	pub(crate) _meta_ptr: *mut c_void,

	pub req: Request,
	pub res: Response,
	pub params: HashMap<String, String>,
	pub address: IpAddr,
	address_text: Option<String>,
	session: Option<Session>,
	target: Option<ResponseTarget<'a>>,
	streaming: bool
}

impl<'a> HttpContext<'a> {
	pub fn from (address: IpAddr, req: Request, params: HashMap<String, String>, target: Option<ResponseTarget<'a>>) -> Self {
		HttpContext {
			req,
			res: Response {
				code: HttpCode::NotSent,
				headers: HttpHeaders::empty(),
				payload: ResponseType::NoContent
			},
			params,
			address,
			address_text: None,
			session: None,
			target,
			streaming: false,
			_meta_ptr: null_mut()
		}
	}

	pub fn get_address_text (&mut self) -> &str {
		return self.address_text.get_or_insert_with(|| self.address.to_string());
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
				self.session = Some(Session::decode(&session_header));
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
	pub fn get_header_default<'s> (&'s self, name: &str, default: &'s str) -> &'s str {
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
			Ok(payload) => Ok(payload),
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

		return Err(ResponseExit::Return);
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

		return Err(ResponseExit::Return);
	}

	pub fn redirect (&mut self, target: &str) -> ResponseRet {
		self.res.code = HttpCode::TemporaryRedirect;
		self.res.headers.set("location".to_string(), target.to_string());
		self.res.payload = ResponseType::NoContent;

		return Err(ResponseExit::Return);
	}

	#[inline]
	pub fn drop (self) -> ResponseRet {
		return Err(ResponseExit::Replace(Response::drop()));
	}

	#[inline]
	pub fn is_streaming (&self) -> bool {
		return self.streaming;
	}

	/// Returns `false` when the client is gone or the context has no connection
	pub fn stream_begin (&mut self) -> bool {
		if self.streaming {
			return true;
		}

		if self.target.is_none() {
			return false;
		}

		self.commit_session();

		if matches!(self.res.code, HttpCode::NotSent) {
			self.res.code = HttpCode::OK;
		}

		self.streaming = true;

		let target = self.target.as_mut().unwrap();
		target.cors.apply_normal(&mut self.res);
		return target.sink.begin_stream(&self.res).is_ok();
	}

	/// Returns `false` when the client is gone
	pub fn stream_write (&mut self, chunk: &[u8]) -> bool {
		if !self.stream_begin() {
			return false;
		}

		return self.target.as_mut().unwrap().sink.write_stream(chunk).is_ok();
	}

	fn commit_session (&mut self) {
		let Some(session) = self.session.as_mut() else {
			return;
		};

		session.commit();

		if session.need_header_refresh {
			session.need_header_refresh = false;
			let token = session.crypt();
			self.set_header("session", token);
		}
	}

	pub fn finalize (mut self) -> Response {
		self.commit_session();

		if self.streaming {
			if let Some(target) = self.target.as_mut() {
				let _ = target.sink.finish_stream();
			}

			self.res.payload = ResponseType::Streamed;
		}

		return self.res;
	}
}
