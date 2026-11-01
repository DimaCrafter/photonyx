use bindings::c::Slice;

use crate::{c::{c_deinit, c_init, c_slice, c_str, c_string}, http::{codes::HttpCode, entity::{HttpHeaders, Request, Response, ResponseType}}};


#[no_mangle]
pub extern "C" fn request_headers (req: &Request) -> *const HttpHeaders {
	return &req.headers;
}

#[no_mangle]
pub extern "C" fn response_new () -> *mut Response {
	c_init(Response::empty)
}

#[no_mangle]
pub extern "C" fn response_set_code (res: &mut Response, code: HttpCode) {
	res.code = code;
}

#[no_mangle]
pub extern "C" fn response_headers (res: &mut Response) -> *mut HttpHeaders {
	return &mut res.headers;
}

#[no_mangle]
pub extern "C" fn response_set_drop (res: &mut Response) {
	res.payload = ResponseType::Drop;
}

#[no_mangle]
pub unsafe extern "C" fn response_set_payload (res: &mut Response, r_ptr: *mut u8, size: usize) {
	res.payload = ResponseType::Payload(Vec::from_raw_parts(r_ptr, size, size));
}

/// Unlike `response_set_payload`, leaves `ptr` with the caller
#[no_mangle]
pub unsafe extern "C" fn response_set_payload_copy (res: &mut Response, ptr: *const u8, size: usize) {
	res.payload = ResponseType::Payload(c_slice(ptr, size).to_vec());
}

#[no_mangle]
pub unsafe extern "C" fn response_drop (res: *mut Response) {
	c_deinit(res)
}
// todo: replace c_str with Slice<u8> to prevent useless slice -> c_str -> slice converting

#[no_mangle]
pub unsafe extern "C" fn http_headers_get (headers: &HttpHeaders, name: Slice<u8>) -> Slice<u8> {
	return match headers.get(name.as_str_unchecked()) {
		Some(value) => value.into(),
		None => Slice::null()
	};
}

#[no_mangle]
pub unsafe extern "C" fn http_headers_set (headers: &mut HttpHeaders, name: c_str, value: c_str) {
	headers.set(c_string(name), c_string(value));
}

#[no_mangle]
pub unsafe extern "C" fn http_headers_set_default (headers: &mut HttpHeaders, name: c_str, value: c_str) {
	headers.set_default(c_string(name), c_string(value));
}

#[no_mangle]
pub unsafe extern "C" fn http_headers_set_normal (headers: &mut HttpHeaders, name: c_str, value: c_str) {
	headers.set_normal(c_string(name), c_string(value));
}
