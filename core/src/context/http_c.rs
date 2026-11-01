use std::ffi::c_void;

use bindings::c::Slice;
use crate::{c::{c_slice, c_unwrap}, context::{http::HttpContext, session::Session}, http::entity::{HttpMethod, Request, Response}};


#[no_mangle]
pub extern "C" fn http_context_get_body_ref (ctx: &HttpContext) -> Slice<u8> {
	return Slice::for_vec(&ctx.req.body);
}

/// Text form of the peer address, valid until the handler returns
#[no_mangle]
pub extern "C" fn http_context_get_address (ctx: &mut HttpContext) -> Slice<u8> {
	return ctx.get_address_text().into();
}

#[no_mangle]
pub extern "C" fn http_context_get_method (ctx: &HttpContext) -> HttpMethod {
	return ctx.req.method;
}

#[no_mangle]
pub extern "C" fn http_context_stream_begin (ctx: &mut HttpContext) -> bool {
	return ctx.stream_begin();
}

#[no_mangle]
pub unsafe extern "C" fn http_context_stream_write (ctx: &mut HttpContext, ptr: *const u8, size: usize) -> bool {
	return ctx.stream_write(c_slice(ptr, size));
}

#[no_mangle]
pub extern "C" fn http_context_get_meta_ref (ctx: &HttpContext) -> *mut c_void {
	return ctx._meta_ptr;
}

#[no_mangle]
pub extern "C" fn http_context_set_meta_ref (ctx: &mut HttpContext, ptr: *mut c_void) {
	ctx._meta_ptr = ptr;
}

#[no_mangle]
pub extern "C" fn http_context_get_request (ctx: &mut HttpContext) -> *const Request {
	return &ctx.req;
}

#[no_mangle]
pub extern "C" fn http_context_get_response (ctx: &mut HttpContext) -> *mut Response {
	return &mut ctx.res;
}

#[no_mangle]
pub extern "C" fn http_context_get_session (ctx: &mut HttpContext) -> *mut Session {
	return ctx.get_session();
}

#[no_mangle]
pub unsafe extern "C" fn http_context_set_response (ctx: &mut HttpContext, res: *mut Response) {
	ctx.res = c_unwrap(res);
}