use json::JsonValue;
use crate::context::session::Session;


#[no_mangle]
pub extern "C" fn session_get_data (session: &mut Session) -> *mut JsonValue {
	return session.get_data();
}

#[no_mangle]
pub extern "C" fn session_is_none (session: &Session) -> bool {
	return session.is_none();
}
