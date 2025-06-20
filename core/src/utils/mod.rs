use std::process;
use json::JsonValue;
use crate::utils::log::log_error;

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
