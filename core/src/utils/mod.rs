use std::{process, time::{Duration, SystemTime, UNIX_EPOCH}};
use json::JsonValue;
use crate::utils::log::log_error;

pub mod datetime_c;
pub mod json_c;
pub mod log;
pub mod macros;
pub mod stream;
pub mod sync;
pub mod validator;
pub mod validator_c;


pub fn json_access<'a> (obj: &'a mut JsonValue, path: &'a str) -> &'a mut JsonValue {
    let mut result = obj;
	for part in path.split('.') {
		result = &mut result[part];
	}

	return result;
}


pub fn json_read_array<'a, V, G, E>(obj: &'a JsonValue, getter: G, empty: E) -> Option<Vec<V>>
where
    V: 'a,
    G: Fn(&'a JsonValue) -> Option<V>,
    E: Fn() -> V,
{
    match obj {
        JsonValue::Array(array) => Some({
            array.iter()
                .map(|raw| {
                    if let Some(value) = (getter)(raw) {
                        value
                    } else {
                        (empty)()
                    }
                })
                .collect::<Vec<V>>()
        }),
        _ => None,
    }
}

pub fn bake_fatal<'a, R: 'a> (msg: &'a str) -> impl (Fn() -> R) + 'a {
    return move || {
        log_error(msg);
        process::exit(-1);
    };
}

pub fn parse_duration (input: &str) -> Option<Duration> {
    if input.ends_with("s") {
        return Some(Duration::from_secs(input[..input.len() - 1].parse().ok()?));
    } else if input.ends_with("m") {
        return Some(Duration::from_mins(input[..input.len() - 1].parse().ok()?));
    } else if input.ends_with("h") {
        return Some(Duration::from_hours(input[..input.len() - 1].parse().ok()?));
    } else if input.ends_with("d") {
        return Some(Duration::from_days(input[..input.len() - 1].parse().ok()?));
    } else if input.ends_with("w") {
        return Some(Duration::from_weeks(input[..input.len() - 1].parse().ok()?));
    } else {
        return None;
    }
}

pub fn system_as_unix_num (timestamp: &SystemTime) -> f64 {
    return timestamp.duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
}
