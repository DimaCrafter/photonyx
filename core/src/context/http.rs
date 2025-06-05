use std::{collections::HashMap, net::IpAddr};
use json::{object, JsonValue};
use crate::http::{codes::HttpCode, entity::{HttpConnection, HttpHeaders, Request, Response, ResponseRet, ResponseType}};
use crate::utils::validator::*;


#[derive(Debug)]
pub struct HttpContext {
	pub req: Request,
	pub res: Response,
	pub params: HashMap<String, String>,
	pub address: IpAddr
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
			address: connection.get_address()
		}
	}

	#[inline]
	pub fn get_header (&self, name: &str) -> Option<String> {
		return self.req.headers.get(name);
	}

	#[inline]
	pub fn get_header_default (&self, name: &str, default: String) -> String {
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
}
