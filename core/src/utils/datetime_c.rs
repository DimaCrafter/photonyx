use bindings::c::disown;
use chrono::{DateTime, Utc};


#[no_mangle]
pub extern "C" fn datetime_from_micros (timestamp: i64) -> *mut DateTime<Utc> {
	return disown(DateTime::from_timestamp_micros(timestamp).unwrap());
}
