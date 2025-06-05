use crate::{c::{c_deinit, c_init, c_str, c_string}, http::{codes::HttpCode, entity::{HttpHeaders, Response, ResponseType}}};

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

#[no_mangle]
pub unsafe extern "C" fn response_drop (res: *mut Response) {
	c_deinit(res)
}
