use crate::{app::router::Router, c::{c_str, c_string, c_unwrap}, context::http::HttpContext, http::entity::{Response, ResponseRet}};


// #[no_mangle]
// pub extern "C" fn router_new () -> *mut Router {
// 	c_init(Router::empty)
// }


#[no_mangle]
pub unsafe extern "C" fn router_register (router: &mut Router, pattern: c_str, action: extern "C" fn (*mut HttpContext) -> *mut Response) {
	router.register(c_string(pattern), move |ctx| {
		let res = (action)(ctx);
		if res.is_null() {
			return ResponseRet::Return;
		} else {
			return ResponseRet::Replace(c_unwrap(res));
		}
	});
}

// #[no_mangle]
// pub unsafe extern "C" fn router_drop (router: *mut Router) {
// 	c_deinit(router)
// }
