use bindings::c::disown;
use chrono::{DateTime, Utc};
use crate::c::{c_deinit, c_init_str, c_str};


pub const CURRENT_DATE_C: *mut chrono::DateTime<Utc> = std::ptr::without_provenance_mut(usize::MAX);

#[no_mangle]
pub extern "C" fn datetime_from_micros (timestamp: i64) -> *mut DateTime<Utc> {
	return disown(DateTime::from_timestamp_micros(timestamp).unwrap());
}

#[no_mangle]
pub unsafe extern "C" fn datetime_deinit (value: *mut DateTime<Utc>) {
	if value.is_null() || value == CURRENT_DATE_C {
		return;
	}

	c_deinit(value);
}

/// `YYYY-MM-DD HH:MM:SS[.ffffff]` in UTC
#[no_mangle]
pub unsafe extern "C" fn datetime_to_string (value: *const DateTime<Utc>) -> c_str {
	let datetime = if value == CURRENT_DATE_C { Utc::now() } else { *value };
	return c_init_str(datetime.format("%Y-%m-%d %H:%M:%S%.f").to_string());
}
