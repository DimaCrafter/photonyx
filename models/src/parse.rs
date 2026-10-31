use std::{fs, path::{Path, PathBuf}};
use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
use crate::{error::ModelError, ir::*};


const DIALECTS: [&str; 2] = ["mysql", "postgres"];

type Node<'a> = MarkedYaml<'a>;

fn pos_of (node: &Node) -> Pos {
	return Pos { line: node.span.start.line(), col: node.span.start.col() + 1 };
}

fn as_str<'a> (node: &'a Node) -> Option<&'a str> {
	if let YamlData::Value(Scalar::String(value)) = &node.data {
		return Some(value);
	}

	return None;
}

fn is_null (node: &Node) -> bool {
	return matches!(&node.data, YamlData::Value(Scalar::Null));
}

fn to_raw (node: &Node) -> Raw {
	return match &node.data {
		YamlData::Value(Scalar::Null) => Raw::Null,
		YamlData::Value(Scalar::Boolean(value)) => Raw::Bool(*value),
		YamlData::Value(Scalar::Integer(value)) => Raw::Int(*value),
		YamlData::Value(Scalar::FloatingPoint(value)) => Raw::Float(value.into_inner()),
		YamlData::Value(Scalar::String(value)) => Raw::Str(value.to_string()),
		YamlData::Sequence(items) => Raw::Seq(items.iter().map(to_raw).collect()),
		YamlData::Mapping(map) => Raw::Map(
			map.iter().map(|(key, value)| (key_string(key), to_raw(value))).collect()
		),
		YamlData::Tagged(_, inner) => to_raw(inner),
		_ => Raw::Null
	};
}

fn key_string (key: &Node) -> String {
	return match &key.data {
		YamlData::Value(Scalar::String(value)) => value.to_string(),
		YamlData::Value(Scalar::Integer(value)) => value.to_string(),
		YamlData::Value(Scalar::Boolean(value)) => value.to_string(),
		YamlData::Value(Scalar::FloatingPoint(value)) => value.to_string(),
		_ => String::from("?")
	};
}

struct FileParser<'f> {
	file: &'f Path,
	errors: &'f mut Vec<ModelError>
}

impl<'f> FileParser<'f> {
	fn error (&mut self, node: &Node, message: String) {
		self.errors.push(ModelError::at(self.file, pos_of(node), message));
	}

	fn entries<'a> (&mut self, node: &'a Node<'a>, what: &str) -> Vec<(&'a str, &'a Node<'a>, &'a Node<'a>)> {
		let map = match &node.data {
			YamlData::Mapping(map) => map,
			_ => {
				self.error(node, format!("{what}: expected a mapping"));
				return Vec::new();
			}
		};

		let mut result = Vec::new();
		for (key, value) in map.iter() {
			match as_str(key) {
				Some(name) => result.push((name, key, value)),
				None => self.error(key, format!("{what}: keys must be strings"))
			}
		}

		return result;
	}

	fn string_list (&mut self, node: &Node, what: &str) -> Vec<String> {
		let items = match &node.data {
			YamlData::Sequence(items) => items,
			_ => {
				self.error(node, format!("{what}: expected a list of names"));
				return Vec::new();
			}
		};

		let mut result = Vec::new();
		for item in items {
			match as_str(item) {
				Some(value) => result.push(value.to_owned()),
				None => self.error(item, format!("{what}: expected a name"))
			}
		}

		return result;
	}

	fn bool_value (&mut self, node: &Node, what: &str) -> bool {
		if let YamlData::Value(Scalar::Boolean(value)) = &node.data {
			return *value;
		}

		self.error(node, format!("{what}: expected true or false"));
		return false;
	}

	fn string_value (&mut self, node: &Node, what: &str) -> Option<String> {
		match as_str(node) {
			Some(value) => return Some(value.to_owned()),
			None => {
				self.error(node, format!("{what}: expected a string"));
				return None;
			}
		}
	}

	fn entity (&mut self, root: &Node) -> Option<EntityKind> {
		let mut kind = None;
		for (key, _, value) in self.entries(root, "model") {
			if key == "kind" {
				kind = Some((self.string_value(value, "kind")?, value));
			}
		}

		let (kind, kind_node) = match kind {
			Some(kind) => kind,
			None => {
				self.error(root, "`kind` is required (table or enum)".to_owned());
				return None;
			}
		};

		return match kind.as_str() {
			"table" => Some(EntityKind::Table(self.table(root))),
			"enum" => Some(EntityKind::Enum(self.enum_entity(root))),
			_ => {
				self.error(kind_node, format!("unknown kind `{kind}`, expected table or enum"));
				None
			}
		};
	}

	fn table (&mut self, root: &Node) -> Table {
		let mut table = Table { table: String::new(), fields: Vec::new(), indexes: Vec::new(), deleted: false, dialects: Vec::new() };
		let mut has_table = false;

		for (key, key_node, value) in self.entries(root, "table") {
			match key {
				"kind" => {}
				"table" => {
					if let Some(name) = self.string_value(value, "table") {
						table.table = name;
						has_table = true;
					}
				}
				"fields" => {
					for (name, name_node, spec) in self.entries(value, "fields") {
						if let Some(field) = self.field(name, name_node, spec) {
							table.fields.push(field);
						}
					}
				}
				"indexes" => {
					for (name, name_node, spec) in self.entries(value, "indexes") {
						if let Some(index) = self.index(name, name_node, spec) {
							table.indexes.push(index);
						}
					}
				}
				"deleted" => table.deleted = self.bool_value(value, "deleted"),
				_ if DIALECTS.contains(&key) => table.dialects.push(DialectBlock { dialect: key.to_owned(), value: to_raw(value) }),
				_ => self.error(key_node, format!("unknown key `{key}` in table model"))
			}
		}

		if !has_table {
			self.error(root, "`table` is required for kind: table".to_owned());
		}

		return table;
	}

	fn field (&mut self, name: &str, name_node: &Node, spec: &Node) -> Option<Field> {
		let mut field = Field {
			name: name.to_owned(),
			column: None,
			field_type: FieldType::Text,
			nullable: false,
			enum_ref: None,
			default: None,
			update_now: false,
			auto: false,
			dialects: Vec::new(),
			pos: pos_of(name_node)
		};

		let what = format!("field `{name}`");
		if let Some(raw_type) = as_str(spec) {
			self.field_type(&mut field, raw_type, spec)?;
			return Some(field);
		}

		if !matches!(&spec.data, YamlData::Mapping(_)) {
			self.error(spec, format!("{what}: expected a type (`?string(64)`) or an object"));
			return None;
		}

		let mut type_set = false;
		let mut default_node = None;
		for (key, key_node, value) in self.entries(spec, &what) {
			match key {
				"type" => {
					let raw_type = self.string_value(value, &format!("{what}.type"))?;
					self.field_type(&mut field, &raw_type, value)?;
					type_set = true;
				}
				"enum" => {
					if let Some(target) = self.string_value(value, &format!("{what}.enum")) {
						field.enum_ref = Some(EntityRef::parse(&target));
					}
				}
				"default" => {
					default_node = Some(value);
				}
				"update" => match as_str(value) {
					Some("now") => field.update_now = true,
					_ => self.error(value, format!("{what}.update: the only value is `now` (set the current time on every update)"))
				},
				"auto" => {
					field.auto = self.bool_value(value, &format!("{what}.auto"));
				}
				"column" => {
					field.column = self.string_value(value, &format!("{what}.column"));
				}
				_ if DIALECTS.contains(&key) => {
					field.dialects.push(DialectBlock { dialect: key.to_owned(), value: to_raw(value) });
				}
				_ => {
					self.error(key_node, format!("{what}: unknown key `{key}`"));
				}
			}
		}

		if !type_set {
			self.error(spec, format!("{what}: `type` is required"));
			return None;
		}

		if let Some(value) = default_node {
			field.default = match &value.data {
				YamlData::Value(Scalar::Boolean(value)) => {
					Some(Default::Bool(*value))
				}
				YamlData::Value(Scalar::Integer(value)) => {
					Some(Default::Int(*value))
				}
				YamlData::Value(Scalar::FloatingPoint(value)) => {
					Some(Default::Float(value.into_inner()))
				}
				YamlData::Value(Scalar::String(value)) => {
					if value == "now" && field.field_type == FieldType::DateTime {
						Some(Default::Now)
					} else {
						Some(Default::Str(value.to_string()))
					}
				}
				_ => {
					self.error(value, format!("{what}: default must be a number, string, bool or `now` (nullable fields default to null)"));
					None
				}
			};
		}

		return Some(field);
	}

	fn field_type (&mut self, field: &mut Field, raw: &str, node: &Node) -> Option<()> {
		let (nullable, raw_type) = match raw.strip_prefix('?') {
			Some(rest) => (true, rest),
			None => (false, raw)
		};

		match FieldType::parse(raw_type.trim()) {
			Some(field_type) => {
				field.field_type = field_type;
				field.nullable = nullable;
				return Some(());
			}
			None => {
				self.error(node, format!(
					"field `{}`: unknown type `{raw_type}` (expected u8..u64, i8..i64, f32, f64, bool, string(N), char(N), text, datetime, json, enum)",
					field.name
				));
				return None;
			}
		}
	}

	fn index (&mut self, name: &str, name_node: &Node, spec: &Node) -> Option<Index> {
		let mut index = Index {
			name: name.to_owned(),
			columns: Vec::new(),
			primary: name == "primary",
			unique: false,
			link: None,
			constraint: None,
			dialects: Vec::new(),
			pos: pos_of(name_node)
		};

		let what = format!("index `{name}`");
		if let YamlData::Sequence(_) = &spec.data {
			index.columns = self.string_list(spec, &what);
			return Some(index);
		}

		if !matches!(&spec.data, YamlData::Mapping(_)) {
			self.error(spec, format!("{what}: expected a list of fields or an object"));
			return None;
		}

		if index.primary {
			self.error(spec, "primary: expected a list of fields (`primary: [id]`)".to_owned());
			return None;
		}

		let mut link_target = None;
		let mut delete = None;
		for (key, key_node, value) in self.entries(spec, &what) {
			match key {
				"columns" => index.columns = self.string_list(value, &format!("{what}.columns")),
				"unique" => index.unique = self.bool_value(value, &format!("{what}.unique")),
				"link" => link_target = self.string_value(value, &format!("{what}.link")),
				"delete" => {
					let raw = self.string_value(value, &format!("{what}.delete"))?;
					delete = Some(match raw.as_str() {
						"restrict" => DeletePolicy::Restrict,
						"cascade" => DeletePolicy::Cascade,
						"set_null" => DeletePolicy::SetNull,
						"none" => DeletePolicy::None,
						_ => {
							self.error(value, format!("{what}: unknown delete policy `{raw}` (restrict, cascade, set_null, none)"));
							return None;
						}
					});
				}
				"constraint" => index.constraint = self.string_value(value, &format!("{what}.constraint")),
				_ if DIALECTS.contains(&key) => index.dialects.push(DialectBlock { dialect: key.to_owned(), value: to_raw(value) }),
				_ => self.error(key_node, format!("{what}: unknown key `{key}`"))
			}
		}

		if index.columns.is_empty() {
			self.error(spec, format!("{what}: `columns` is required"));
		}

		match link_target {
			Some(raw) => {
				let (target, target_index) = parse_link(&raw);
				index.link = Some(Link { target, index: target_index, delete: delete.unwrap_or(DeletePolicy::Restrict) });
			}
			None => {
				if delete.is_some() {
					self.error(spec, format!("{what}: `delete` requires `link`"));
				}

				if index.constraint.is_some() {
					self.error(spec, format!("{what}: `constraint` requires `link`"));
				}
			}
		}

		return Some(index);
	}

	fn enum_entity (&mut self, root: &Node) -> Enum {
		let mut result = Enum { values: Vec::new() };
		let mut has_values = false;

		for (key, key_node, value) in self.entries(root, "enum") {
			match key {
				"kind" => {}
				"values" => {
					has_values = true;
					self.enum_values(value, &mut result);
				}
				_ => self.error(key_node, format!("unknown key `{key}` in enum model"))
			}
		}

		if !has_values {
			self.error(root, "`values` is required for kind: enum".to_owned());
		}

		return result;
	}

	fn enum_values (&mut self, node: &Node, result: &mut Enum) {
		match &node.data {
			// values: [a, b]
			YamlData::Sequence(items) => {
				for item in items {
					match as_str(item) {
						Some(name) => result.values.push(EnumValue { name: name.to_owned(), int: None, db: None, doc: None, pos: pos_of(item) }),
						None => self.error(item, "enum value: expected a name".to_owned())
					}
				}
			}
			// values: { a: doc } | { a: { int, db, doc } }
			YamlData::Mapping(_) => {
				for (name, name_node, spec) in self.entries(node, "values") {
					let mut value = EnumValue { name: name.to_owned(), int: None, db: None, doc: None, pos: pos_of(name_node) };
					let what = format!("enum value `{name}`");

					if let Some(doc) = as_str(spec) {
						value.doc = Some(doc.to_owned());
					} else if let YamlData::Mapping(_) = &spec.data {
						for (key, key_node, item) in self.entries(spec, &what) {
							match key {
								"int" => match &item.data {
									YamlData::Value(Scalar::Integer(int)) => value.int = Some(*int),
									_ => self.error(item, format!("{what}: `int` must be an integer"))
								},
								"db" => value.db = self.string_value(item, &format!("{what}.db")),
								"doc" => value.doc = self.string_value(item, &format!("{what}.doc")),
								_ => self.error(key_node, format!("{what}: unknown key `{key}`"))
							}
						}
					} else if !is_null(spec) {
						self.error(spec, format!("{what}: expected a doc string or an object"));
					}

					result.values.push(value);
				}
			}
			_ => self.error(node, "values: expected a list or a mapping".to_owned())
		}
	}
}

/// Parse `Model`, `Model.index`, `group.Model` or `group.Model.index`
fn parse_link (raw: &str) -> (EntityRef, Option<String>) {
	let parts: Vec<&str> = raw.split('.').collect();
	return match parts.as_slice() {
		[model] => (EntityRef { group: None, name: (*model).to_owned() }, None),
		[first, second] => {
			if first.starts_with(|ch: char| ch.is_uppercase()) {
				(EntityRef { group: None, name: (*first).to_owned() }, Some((*second).to_owned()))
			} else {
				(EntityRef { group: Some((*first).to_owned()), name: (*second).to_owned() }, None)
			}
		}
		[group, model, index, ..] => (EntityRef { group: Some((*group).to_owned()), name: (*model).to_owned() }, Some((*index).to_owned())),
		[] => (EntityRef { group: None, name: String::new() }, None)
	};
}


pub fn parse_entity (file: &Path, source: &str, errors: &mut Vec<ModelError>) -> Option<Entity> {
	let name = file
		.file_stem()
		.and_then(|stem| stem.to_str())
		.unwrap_or_default()
		.to_owned();

	let docs = match MarkedYaml::load_from_str(source) {
		Ok(docs) => docs,
		Err(error) => {
			let marker = error.marker();
			errors.push(ModelError::at(file, Pos { line: marker.line(), col: marker.col() + 1 }, format!("invalid yml: {}", error.info())));
			return None;
		}
	};

	let root = match docs.first() {
		Some(root) => root,
		None => {
			errors.push(ModelError::at(file, Pos { line: 1, col: 1 }, "empty model file".to_owned()));
			return None;
		}
	};

	let mut parser = FileParser { file, errors };
	let kind = parser.entity(root)?;
	return Some(Entity { name, file: file.to_owned(), kind });
}

pub fn load_group (name: &str, group_dir: &Path, files: &mut Vec<PathBuf>, errors: &mut Vec<ModelError>) -> Option<Group> {
	let mut entity_files = match list_dir(group_dir) {
		Ok(entries) => entries,
		Err(error) => {
			errors.push(ModelError::file(group_dir, format!("failed to read group directory: {error}")));
			return None;
		}
	};

	entity_files.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "yml"));

	let mut entities = Vec::new();
	for file in entity_files {
		let source = match fs::read_to_string(&file) {
			Ok(source) => source,
			Err(error) => {
				errors.push(ModelError::file(&file, format!("failed to read: {error}")));
				continue;
			}
		};

		files.push(file.clone());
		if let Some(entity) = parse_entity(&file, &source, errors) {
			entities.push(entity);
		}
	}

	entities.sort_by(|a, b| a.name.cmp(&b.name));
	return Some(Group { name: name.to_owned(), dir: group_dir.to_owned(), entities });
}

fn list_dir (dir: &Path) -> std::io::Result<Vec<PathBuf>> {
	let mut result = Vec::new();
	for entry in fs::read_dir(dir)? {
		let path = entry?.path();
		if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| !name.starts_with('.')) {
			result.push(path);
		}
	}

	result.sort();
	return Ok(result);
}
