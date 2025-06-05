#[macro_export]
macro_rules! validator_struct {
	{ $name:ident, $( $field:ident : $type:ty ),* } => {
		#[derive(Default, Debug)]
		struct $name {
			$($field: $type,)*
		}
	};
}

#[macro_export]
macro_rules! validator_impl {
	(range($name:ident, $value:expr, $min:literal, $max:literal)) => {
		if ($value < $min) || ($value > $max) {
			return Err(ValidationError {
				message: format!("value out of range [{}; {}]", $min, $max),
				path: vec![$name.to_owned()]
			});
		}
	};
	(nested($name:ident, $value:expr,)) => {
		if let Err(mut error) = $value.validate() {
			error.path.insert(0, $name.to_owned());
			return Err(error);
		}
	};
	(str_enum($name:ident, $value:expr, $($valid_value:literal),* )) => {
		match $value.as_str() {
			$( $valid_value )|* => {}
			_ => return Err(ValidationError {
				message: format!("value must be one of: {}", vec![$($valid_value),*].join(", ")),
				path: vec![$name.to_owned()]
			})
		}
	};
	{ $name:ident, $( $field:ident : $type:ty $(as $method:ident( $($opts:tt),* ))? ),* } => {
		impl Validate for $name {
			fn validate (&self) -> Result<(), ValidationError> {
				$($(
					let name = stringify!($field);
					$crate::validator_impl!($method(name, self.$field, $($opts),*));
				)?)*

				return Ok(());
			}
		}
	};
}

#[macro_export]
macro_rules! validator {
	{ $name:ident, $( $field:ident : $type:ty $(as $method:ident($($opts:tt),*))? ),* } => {
		use $crate::utils::validator::*;
		$crate::validator_struct! { $name, $( $field: $type ),* }
		$crate::validator_impl! { $name, $( $field: $type $(as $method($($opts),*))? ),* }
	};
}

#[macro_export]
macro_rules! json_parse_impl {
	($name:ident, $value:expr, $input:expr, bool, $method:ident) => {
		if let Some(value) = $input.as_bool() {
			$value = value;
		} else {
			return Err(ValidationError {
				message: "expected bool".to_owned(),
				path: vec![$name.to_owned()]
			});
		}
	};
	($name:ident, $value:expr, $input:expr, String, $method:ident) => {
		if let Some(value) = $input.as_str() {
			$value = value.to_owned();
		} else {
			return Err(ValidationError {
				message: "expected string".to_owned(),
				path: vec![$name.to_owned()]
			});
		}
	};
	($name:ident, $value:expr, $input:expr, i32, $method:ident) => {
		if let Some(value) = $input.as_i32() {
			$value = value;
		} else {
			return Err(ValidationError {
				message: "expected i32".to_owned(),
				path: vec![$name.to_owned()]
			});
		}
	};
	($name:ident, $value:expr, $input:expr, $type:ty, nested) => {
		if let Err(mut error) = $value.assign_json(&$input) {
			error.path.insert(0, $name.to_owned());
			return Err(error);
		}
	};
	{ $name:ident, $( $field:ident : $type:ident $($method:ident)? ),* } => {
		impl ValidateJson for $name {
			fn parse_json (&mut self, raw: &str) -> Result<(), ValidationError> {
				return match json::parse(raw) {
					Err(error) => Err(ValidationError { message: error.to_string(), path: Vec::new() }),
					Ok(parsed) => return self.assign_json(&parsed)
				}
			}

			fn assign_json (&mut self, parsed: &json::JsonValue) -> Result<(), ValidationError> {
				$($(
					let name = stringify!($field);
					$crate::json_parse_impl!(name, self.$field, parsed[stringify!($field)], $type, $method);
				)?)*

				return Ok(());
			}
		}
	};
}

#[macro_export]
macro_rules! validator_json {
	{ $name:ident, $( $field:ident : $type:tt $(as $method:ident($($opts:tt),*))? ),* } => {
		$crate::validator! { $name, $( $field: $type $(as $method($($opts),*))? ),* }
		$crate::json_parse_impl! { $name, $( $field: $type $($method)? ),* }
	};
}

pub struct ValidationError {
	pub message: String,
	pub path: Vec<String>
}

pub trait ValidateJson {
	fn parse_json (&mut self, raw: &str) -> Result<(), ValidationError>;
	fn assign_json (&mut self, parsed: &json::JsonValue) -> Result<(), ValidationError>;
}

pub trait Validate {
	fn validate (&self) -> Result<(), ValidationError>;
}

pub fn validate_json<T: Validate + ValidateJson + Default> (raw: &str) -> Result<T, ValidationError> {
	let mut payload: T = Default::default();
	payload.parse_json(raw)?;
	payload.validate()?;

	return Ok(payload);
}
