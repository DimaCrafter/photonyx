use std::{collections::BTreeMap, fmt};
use json::JsonValue;
use sha2::{Digest, Sha256};


/// Major version of the runtime document format
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
	U8, U16, U32, U64,
	I8, I16, I32, I64,
	F32, F64,
	Bool,
	String(u32),
	Char(u32),
	Text,
	DateTime,
	Json,
	/// Native `ENUM(...)` of the dialect
	Enum
}

impl FieldType {
	pub fn parse (raw: &str) -> Option<FieldType> {
		let sized = |prefix: &str| -> Option<u32> {
			let rest = raw.strip_prefix(prefix)?.strip_prefix('(')?.strip_suffix(')')?;

			let size: u32 = rest.trim().parse().ok()?;
			if size == 0 {
				return None;
			}

			return Some(size);
		};

		return Some(match raw {
			"u8" => FieldType::U8,
			"u16" => FieldType::U16,
			"u32" => FieldType::U32,
			"u64" => FieldType::U64,
			"i8" => FieldType::I8,
			"i16" => FieldType::I16,
			"i32" => FieldType::I32,
			"i64" => FieldType::I64,
			"f32" => FieldType::F32,
			"f64" => FieldType::F64,
			"bool" => FieldType::Bool,
			"text" => FieldType::Text,
			"datetime" => FieldType::DateTime,
			"json" => FieldType::Json,
			"enum" => FieldType::Enum,
			_ => {
				if let Some(size) = sized("string") {
					FieldType::String(size)
				} else if let Some(size) = sized("char") {
					FieldType::Char(size)
				} else {
					return None;
				}
			}
		});
	}

	pub fn is_integer (&self) -> bool {
		return self.int_range().is_some();
	}

	pub fn int_range (&self) -> Option<(i128, i128)> {
		return Some(match self {
			FieldType::U8 => (0, u8::MAX as i128),
			FieldType::U16 => (0, u16::MAX as i128),
			FieldType::U32 => (0, u32::MAX as i128),
			FieldType::U64 => (0, u64::MAX as i128),
			FieldType::I8 => (i8::MIN as i128, i8::MAX as i128),
			FieldType::I16 => (i16::MIN as i128, i16::MAX as i128),
			FieldType::I32 => (i32::MIN as i128, i32::MAX as i128),
			FieldType::I64 => (i64::MIN as i128, i64::MAX as i128),
			_ => return None
		});
	}

	pub fn is_float (&self) -> bool {
		return matches!(self, FieldType::F32 | FieldType::F64);
	}

	pub fn is_string (&self) -> bool {
		return matches!(self, FieldType::String(_) | FieldType::Char(_) | FieldType::Text);
	}

	pub fn max_length (&self) -> Option<u32> {
		return match self {
			FieldType::String(size) | FieldType::Char(size) => Some(*size),
			_ => None
		};
	}
}

impl fmt::Display for FieldType {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		return match self {
			FieldType::U8 => f.write_str("u8"),
			FieldType::U16 => f.write_str("u16"),
			FieldType::U32 => f.write_str("u32"),
			FieldType::U64 => f.write_str("u64"),
			FieldType::I8 => f.write_str("i8"),
			FieldType::I16 => f.write_str("i16"),
			FieldType::I32 => f.write_str("i32"),
			FieldType::I64 => f.write_str("i64"),
			FieldType::F32 => f.write_str("f32"),
			FieldType::F64 => f.write_str("f64"),
			FieldType::Bool => f.write_str("bool"),
			FieldType::String(size) => write!(f, "string({size})"),
			FieldType::Char(size) => write!(f, "char({size})"),
			FieldType::Text => f.write_str("text"),
			FieldType::DateTime => f.write_str("datetime"),
			FieldType::Json => f.write_str("json"),
			FieldType::Enum => f.write_str("enum")
		};
	}
}


#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeModels {
	pub version: u32,
	pub dbs: BTreeMap<String, RuntimeDb>
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuntimeDb {
	pub tables: BTreeMap<String, RuntimeModel>,
	pub enums: BTreeMap<String, RuntimeEnum>
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeModel {
	pub table: String,
	/// Field names of the primary key
	pub primary: Vec<String>,
	/// In the order of fields of generated structures and setters
	pub columns: Vec<RuntimeField>
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeField {
	pub name: String,
	/// Column name in the database when it differs from `name`
	pub column: Option<String>,
	pub field_type: FieldType,
	pub nullable: bool,
	/// Enum entity: `Name` in the same db or `group.Name`
	pub enum_ref: Option<String>
}

impl RuntimeField {
	pub fn column_name (&self) -> &str {
		return self.column.as_deref().unwrap_or(&self.name);
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEnum {
	pub values: Vec<RuntimeEnumValue>
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEnumValue {
	pub name: String,
	pub int: Option<i64>,
	/// String representation in the database when it differs from `name`
	pub db: Option<String>
}

impl RuntimeEnumValue {
	pub fn db_name (&self) -> &str {
		return self.db.as_deref().unwrap_or(&self.name);
	}
}


impl RuntimeModels {
	pub fn new () -> Self {
		RuntimeModels { version: VERSION, dbs: BTreeMap::new() }
	}

	pub fn parse (raw: &str) -> Result<Self, String> {
		let root = json::parse(raw).map_err(|error| format!("invalid models schema JSON: {error}"))?;
		return RuntimeModels::from_json(&root);
	}

	pub fn from_json (root: &JsonValue) -> Result<Self, String> {
		let version = root["version"].as_u32().ok_or("models schema: `version` is missing")?;
		if version != VERSION {
			return Err(format!("models schema: unsupported version {version}, expected {VERSION}"));
		}

		let mut dbs = BTreeMap::new();
		for (db_name, db_raw) in root["dbs"].entries() {
			let mut db = RuntimeDb::default();

			for (name, raw) in db_raw["tables"].entries() {
				let table = RuntimeModel::from_json(raw)
					.map_err(|error| format!("models schema: {db_name}.{name}: {error}"))?;
				db.tables.insert(name.to_owned(), table);
			}

			for (name, raw) in db_raw["enums"].entries() {
				let value = RuntimeEnum::from_json(raw)
					.map_err(|error| format!("models schema: {db_name}.{name}: {error}"))?;

				db.enums.insert(name.to_owned(), value);
			}

			dbs.insert(db_name.to_owned(), db);
		}

		return Ok(RuntimeModels { version, dbs });
	}

	pub fn to_json (&self) -> JsonValue {
		let mut dbs = JsonValue::new_object();
		for (db_name, db) in &self.dbs {
			let mut tables = JsonValue::new_object();
			for (name, table) in &db.tables {
				tables[name.as_str()] = table.to_json();
			}

			let mut enums = JsonValue::new_object();
			for (name, value) in &db.enums {
				enums[name.as_str()] = value.to_json();
			}

			let mut db_json = JsonValue::new_object();
			db_json["tables"] = tables;
			db_json["enums"] = enums;
			dbs[db_name.as_str()] = db_json;
		}

		let mut root = JsonValue::new_object();
		root["version"] = self.version.into();
		root["dbs"] = dbs;
		return root;
	}

	pub fn to_canonical (&self) -> String {
		return canonical(&self.to_json());
	}

	pub fn find_table (&self, db: &str, model: &str) -> Option<&RuntimeModel> {
		return self.dbs.get(db)?.tables.get(model);
	}

	pub fn find_enum (&self, db: &str, name: &str) -> Option<&RuntimeEnum> {
		if let Some((group, name)) = name.split_once('.') {
			return self.dbs.get(group)?.enums.get(name);
		}

		return self.dbs.get(db)?.enums.get(name);
	}

	pub fn hash_model (&self, db: &str, model: &str) -> Option<String> {
		let table = self.find_table(db, model)?;

		let mut enums = JsonValue::new_object();
		for column in &table.columns {
			if let Some(enum_ref) = &column.enum_ref {
				if let Some(value) = self.find_enum(db, enum_ref) {
					enums[enum_ref.as_str()] = value.to_json();
				}
			}
		}

		let mut root = JsonValue::new_object();
		root["table"] = table.to_json();
		root["enums"] = enums;

		let digest = Sha256::digest(canonical(&root).as_bytes());
		return Some(digest.iter().map(|byte| format!("{byte:02x}")).collect());
	}
}

impl RuntimeModel {
	fn from_json (raw: &JsonValue) -> Result<Self, String> {
		let table = raw["table"].as_str().ok_or("`table` is missing")?.to_owned();
		let primary = raw["primary"]
			.members()
			.map(|item| item.as_str().map(str::to_owned).ok_or("`primary` must contain strings"))
			.collect::<Result<Vec<_>, _>>()?;

		let mut columns = Vec::new();
		for item in raw["columns"].members() {
			let name = item["name"].as_str().ok_or("column `name` is missing")?.to_owned();
			let raw_type = item["type"].as_str().ok_or_else(|| format!("column {name}: `type` is missing"))?;
			let field_type = FieldType::parse(raw_type).ok_or_else(|| format!("column {name}: unknown type `{raw_type}`"))?;

			columns.push(RuntimeField {
				column: item["column"].as_str().map(str::to_owned),
				field_type,
				nullable: item["nullable"].as_bool().unwrap_or(false),
				enum_ref: item["enum"].as_str().map(str::to_owned),
				name
			});
		}

		return Ok(RuntimeModel { table, primary, columns });
	}

	pub fn to_json (&self) -> JsonValue {
		let mut columns = JsonValue::new_array();
		for column in &self.columns {
			let mut item = JsonValue::new_object();
			item["name"] = column.name.as_str().into();
			if let Some(column_name) = &column.column {
				item["column"] = column_name.as_str().into();
			}

			item["type"] = column.field_type.to_string().into();
			item["nullable"] = column.nullable.into();
			if let Some(enum_ref) = &column.enum_ref {
				item["enum"] = enum_ref.as_str().into();
			}

			let _ = columns.push(item);
		}

		let mut primary = JsonValue::new_array();
		for name in &self.primary {
			let _ = primary.push(name.as_str());
		}

		let mut result = JsonValue::new_object();
		result["table"] = self.table.as_str().into();
		result["primary"] = primary;
		result["columns"] = columns;
		return result;
	}
}

impl RuntimeEnum {
	fn from_json (raw: &JsonValue) -> Result<Self, String> {
		let mut values = Vec::new();
		for item in raw["values"].members() {
			values.push(RuntimeEnumValue {
				name: item["name"].as_str().ok_or("enum value `name` is missing")?.to_owned(),
				int: item["int"].as_i64(),
				db: item["db"].as_str().map(str::to_owned)
			});
		}

		return Ok(RuntimeEnum { values });
	}

	pub fn to_json (&self) -> JsonValue {
		let mut values = JsonValue::new_array();
		for value in &self.values {
			let mut item = JsonValue::new_object();
			item["name"] = value.name.as_str().into();
			if let Some(int) = value.int {
				item["int"] = int.into();
			}

			if let Some(db) = &value.db {
				item["db"] = db.as_str().into();
			}

			let _ = values.push(item);
		}

		let mut result = JsonValue::new_object();
		result["values"] = values;
		return result;
	}
}

pub fn canonical (value: &JsonValue) -> String {
	let mut result = String::new();
	write_canonical(value, &mut result);
	return result;
}

fn write_canonical (value: &JsonValue, out: &mut String) {
	match value {
		JsonValue::Object(object) => {
			let mut entries: Vec<(&str, &JsonValue)> = object.iter().collect();
			entries.sort_by(|a, b| a.0.cmp(b.0));

			out.push('{');
			for (i, (key, item)) in entries.into_iter().enumerate() {
				if i != 0 {
					out.push(',');
				}

				out.push_str(&JsonValue::from(key).dump());
				out.push(':');
				write_canonical(item, out);
			}
			out.push('}');
		}
		JsonValue::Array(items) => {
			out.push('[');
			for (i, item) in items.iter().enumerate() {
				if i != 0 {
					out.push(',');
				}

				write_canonical(item, out);
			}
			out.push(']');
		}
		_ => out.push_str(&value.dump())
	}
}
