use json::JsonValue;
use crate::c::{c_deinit, c_init, c_init_str, c_str, c_string, c_unwrap};

#[no_mangle]
pub extern "C" fn json_new_object () -> *mut JsonValue {
	c_init(JsonValue::new_object)
}

#[no_mangle]
pub unsafe extern "C" fn json_set (obj: &mut JsonValue, key: c_str, value: *mut JsonValue) -> bool {
	return obj.insert(&c_string(key), c_unwrap(value)).is_ok();
}

#[no_mangle]
pub extern "C" fn json_dump (value: &JsonValue) -> c_str {
	c_init_str(value.dump())
}

#[no_mangle]
pub extern "C" fn json_dump_pretty (value: &JsonValue, ident: u16) -> c_str {
	c_init_str(value.pretty(ident))
}

#[no_mangle]
pub extern "C" fn json_new_array () -> *mut JsonValue {
	c_init(JsonValue::new_array)
}

#[no_mangle]
pub unsafe extern "C" fn json_push (list: &mut JsonValue, value: *mut JsonValue) -> bool {
	return list.push(c_unwrap(value)).is_ok();
}

#[no_mangle]
pub extern "C" fn json_new_null () -> *mut JsonValue {
	c_init(|| JsonValue::Null)
}

#[no_mangle]
pub extern "C" fn json_new_number (value: f64) -> *mut JsonValue {
	c_init(|| JsonValue::from(value))
}

#[no_mangle]
pub extern "C" fn json_new_bool (value: bool) -> *mut JsonValue {
	c_init(|| JsonValue::from(value))
}

#[no_mangle]
pub unsafe extern "C" fn json_new_str (value: c_str) -> *mut JsonValue {
	c_init(|| JsonValue::from(c_string(value)))
}

#[no_mangle]
pub unsafe extern "C" fn json_drop (value: *mut JsonValue) {
	c_deinit(value);
}
