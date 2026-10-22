use bindings::c::Slice;
use email_syntax_verify_opt::validate_email;
use json::JsonValue;
use crate::{c::{c_deinit, c_init, c_str, c_string, c_unwrap}, utils::validator::ValidationError};


#[no_mangle]
pub unsafe extern "C" fn validation_error_new (message: c_str) -> *mut ValidationError {
	c_init(|| ValidationError {
		message: c_string(message),
		path: Vec::new()
	})
}

#[no_mangle]
pub unsafe extern "C" fn validation_error_prepend_path (error: &mut ValidationError, parent_key: c_str) {
	error.path.insert(0, c_string(parent_key));
}

#[no_mangle]
pub unsafe extern "C" fn validation_error_into_json (error: *mut ValidationError) -> *mut JsonValue {
	c_init(|| c_unwrap(error).into_json())
}

#[no_mangle]
pub unsafe extern "C" fn validation_error_drop (value: *mut ValidationError) {
	c_deinit(value);
}

#[no_mangle]
pub unsafe extern "C" fn validation_check_email (input: Slice<u8>) -> bool {
	return validate_email(input.as_str_unchecked());
}
