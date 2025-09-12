use bindings::c::Slice;
use crate::{c::c_unwrap, context::{http::HttpContext, session::Session}, http::entity::{Request, Response}};


#[no_mangle]
pub extern "C" fn http_context_get_body_ref (ctx: &HttpContext) -> Slice<u8> {
	return Slice::for_vec(&ctx.req.body);
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