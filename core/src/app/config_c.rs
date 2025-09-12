use std::ops::Deref;

use json::JsonValue;

use crate::{app::config::{Config, CONFIG}, c::{c_str, c_string}};

#[no_mangle]
pub extern "C" fn get_config () -> *const Config {
	return CONFIG.deref();
}

#[no_mangle]
pub unsafe extern "C" fn config_get_path<'a> (config: &'a Config, path: c_str) -> &'a JsonValue {
	return config.get_path(c_string(path).split('.').collect());
}
