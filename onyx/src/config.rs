use std::{env, fs, path::{Path, PathBuf}};
use json::JsonValue;
use libloading::Library;
use photonyx_models::ir::{DialectBlock, Raw};
use photonyx_sql_shared::{dialect::ProvideSqlDialectFn, SqlDialect};


const DIALECT_BLOCKS: [&str; 2] = ["mysql", "postgres"];

// TODO: project config instead of just DB config
pub struct DbConfig {
	pub name: String,
	pub driver: String,
	/// `db.<db>.models` of the project, default `src/models/<db>`
	pub models: PathBuf,
	/// `models` is set explicitly in the project or by `--models`
	pub models_explicit: bool,
	/// `db.<db>.migrations` of the project, default `migrations/<db>`
	pub migrations: PathBuf,
	/// `db.<db>.mysql`/`postgres` of the project
	pub dialect_defaults: Vec<DialectBlock>,
	/// The `db.<db>` branch of `config.json`
	pub raw: JsonValue
}

fn read_json (path: &Path) -> Result<JsonValue, String> {
	let raw = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
	return json::parse(&raw).map_err(|error| format!("{}: invalid JSON: {error}", path.display()));
}

pub fn load_project (path: &Path) -> Result<Vec<DbConfig>, String> {
	let root = read_json(path)?;
	let base = path.parent().unwrap_or(Path::new(""));

	let mut result = Vec::new();
	for (name, item) in root["db"].entries() {
		let path_of = |key: &str, default: String| -> Result<(PathBuf, bool), String> {
			match &item[key] {
				JsonValue::Null => {
					return Ok((base.join(default), false));
				}
				value => {
					return value
						.as_str()
						.map(|value| (base.join(value), true))
						.ok_or_else(|| format!("{}: db.{name}.{key} must be a string", path.display()));
				}
			};
		};

		let (models, models_explicit) = path_of("models", format!("src/models/{name}"))?;
		result.push(DbConfig {
			name: name.to_owned(),
			driver: item["driver"]
				.as_str()
				.ok_or_else(|| format!("{}: db.{name}.driver is missing", path.display()))?
				.to_owned(),
			models,
			models_explicit,
			migrations: path_of("migrations", format!("migrations/{name}"))?.0,
			dialect_defaults: DIALECT_BLOCKS
				.iter()
				.filter(|dialect| !item[**dialect].is_null())
				.map(|dialect| DialectBlock { dialect: (*dialect).to_owned(), value: raw_of(&item[*dialect]) })
				.collect(),
			raw: JsonValue::Null
		});
	}

	return Ok(result);
}

pub fn attach_connections (dbs: &mut [DbConfig], path: &Path) -> Result<(), String> {
	let root = read_json(path)?;
	for db in dbs {
		let item = &root["db"][db.name.as_str()];
		if item.is_null() {
			continue;
		}

		let driver = item["driver"]
			.as_str()
			.ok_or_else(|| format!("{}: db.{}.driver is missing", path.display(), db.name))?;

		if driver != db.driver {
			return Err(format!(
				"db.{}: driver \"{driver}\" in {} differs from \"{}\" in the project file",
				db.name,
				path.display(),
				db.driver
			));
		}

		let blocks = db.dialect_defaults
			.iter()
			.filter(|block| !item[block.dialect.as_str()].is_null());

		for block in blocks {
			let Raw::Map(defaults) = &block.value else { continue };
			for (key, value) in defaults {
				let connection = &item[block.dialect.as_str()][key.as_str()];
				if !connection.is_null() && raw_of(connection) != *value {
					return Err(format!("db.{}.{}.{key} in {} differs from the project file", db.name, block.dialect, path.display()));
				}
			}
		}

		db.raw = item.clone();
	}

	return Ok(());
}

/// Returns (Group, Models directory)[]
pub fn model_groups (dbs: &[DbConfig]) -> Result<Vec<(String, PathBuf)>, String> {
	let mut result = Vec::new();
	for db in dbs {
		if db.models.is_dir() {
			result.push((db.name.clone(), db.models.clone()));
		} else if db.models_explicit {
			return Err(format!("db.{}: models directory {} not found", db.name, db.models.display()));
		}
	}

	return Ok(result);
}

fn raw_of (value: &JsonValue) -> Raw {
	return match value {
		JsonValue::Null => Raw::Null,
		JsonValue::Boolean(value) => Raw::Bool(*value),
		JsonValue::Short(_) | JsonValue::String(_) => Raw::Str(value.as_str().unwrap_or_default().to_owned()),
		JsonValue::Number(_) => match value.as_i64() {
			Some(int) if value.as_f64() == Some(int as f64) => Raw::Int(int),
			_ => Raw::Float(value.as_f64().unwrap_or_default())
		},
		JsonValue::Array(items) => Raw::Seq(items.iter().map(raw_of).collect()),
		JsonValue::Object(object) => Raw::Map(object.iter().map(|(key, item)| (key.to_owned(), raw_of(item))).collect())
	};
}


pub struct Dialects {
	items: Vec<Box<dyn SqlDialect>>,
	pub errors: Vec<String>
}

impl Dialects {
	pub fn load (dir: &Path) -> Dialects {
		let mut result = Dialects { items: Vec::new(), errors: Vec::new() };

		let entries = match fs::read_dir(dir) {
			Ok(entries) => entries,
			Err(error) => {
				result.errors.push(format!("{}: {error}", dir.display()));
				return result;
			}
		};

		let mut paths: Vec<PathBuf> = entries
			.filter_map(|entry| entry.ok().map(|entry| entry.path()))
			.filter(|path| path.extension().is_some_and(|ext| ext == env::consts::DLL_EXTENSION))
			.collect();

		paths.sort();

		for path in paths {
			let lib = match unsafe { Library::new(&path) } {
				Ok(lib) => lib,
				Err(error) => {
					result.errors.push(format!("{}: {error}", path.display()));
					continue;
				}
			};

			let provide = unsafe { lib.get::<ProvideSqlDialectFn>(b"provide_sql_dialect") }.ok().map(|symbol| *symbol);
			if let Some(provide) = provide {
				result.items.push(provide());
			}

			// The dialect's code lives in the library and will be loaded as long as CLI task run
			std::mem::forget(lib);
			// TODO? Bound `lib` to the dialect or a DB module to avoid forgetting
		}

		return result;
	}

	pub fn find (&self, driver: &str) -> Option<&dyn SqlDialect> {
		return self.items
			.iter()
			.find(|dialect| dialect.id() == driver)
			.map(|dialect| dialect.as_ref());
	}
}
