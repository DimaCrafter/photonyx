use crate::{app::router::{MiddlewareResult, Router}, c::{c_str, c_string, c_unwrap}, context::http::HttpContext, http::entity::{Response, ResponseExit}};


#[no_mangle]
pub unsafe extern "C" fn router_register (router: &mut Router, pattern: c_str, action: extern "C" fn (*mut HttpContext) -> *mut Response) {
	router.register(c_string(pattern), move |ctx| {
		let res = (action)(ctx);
		if res.is_null() {
			return Err(ResponseExit::Return);
		} else {
			return Err(ResponseExit::Replace(c_unwrap(res)));
		}
	});
}

#[no_mangle]
pub unsafe extern "C" fn router_use (router: &mut Router, pattern: c_str, middleware: extern "C" fn (*mut HttpContext) -> MiddlewareResult) {
	router.use_middleware(c_string(pattern), move |ctx| middleware(ctx));
}
