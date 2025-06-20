pub mod c;
pub mod db;
pub mod validator;


pub fn camel_to_kebab (value: &str) -> String {
    let mut is_last_upper = false;
    let mut result = String::new();

    for ch in value.chars() {
        if ch.to_ascii_uppercase() == ch {
            if is_last_upper {
                result.push(ch.to_ascii_lowercase());
            } else {
                result.push('-');
                result.push(ch.to_ascii_lowercase());
            }

            is_last_upper = true;
        } else {
            result.push(ch);
            is_last_upper = false;
        }
    }

    if result.starts_with('-') {
		return String::from(&result[1..]);
	} else {
		return result;
	}
}
