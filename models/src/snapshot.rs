use std::{collections::BTreeMap, path::PathBuf};
use json::JsonValue;
use crate::{ir::*, runtime::canonical};


/// Version of the snapshot format
pub const VERSION: u32 = 1;

pub fn to_json (set: &ModelSet, group: &str) -> Option<JsonValue> {
	let current = set.find_group(group)?;

	let mut entities = JsonValue::new_object();
	for entity in &current.entities {
		entities[entity.name.as_str()] = entity_to_json(entity);
	}

	// DB --> Entity name --> Entity
	let mut external: BTreeMap<String, BTreeMap<String, JsonValue>> = BTreeMap::new();
	for entity in &current.entities {
		let table = match &entity.kind {
			EntityKind::Table(table) => table,
			EntityKind::Enum(_) => continue
		};

		let refs = table.fields.iter().filter_map(|field| field.enum_ref.as_ref())
			.chain(table.indexes.iter().filter_map(|index| index.link.as_ref().map(|link| &link.target)));

		for target in refs {
			let target_group = target.group_or(group);
			if target_group == group {
				continue;
			}

			if let Some(target_entity) = set.find_entity(group, target) {
				external.entry(target_group.to_owned()).or_default().insert(target_entity.name.clone(), entity_to_json(target_entity));
			}
		}
	}

	let mut root = JsonValue::new_object();
	root["version"] = VERSION.into();
	root["db"] = group.into();
	root["entities"] = entities;

	if !external.is_empty() {
		let mut groups = JsonValue::new_object();
		for (name, items) in external {
			let mut group_json = JsonValue::new_object();
			for (entity_name, value) in items {
				group_json[entity_name.as_str()] = value;
			}

			groups[name.as_str()] = group_json;
		}

		root["external"] = groups;
	}

	return Some(root);
}

/// Returns the set and the name of the snapshot group.
pub fn from_json (root: &JsonValue) -> Result<(ModelSet, String), String> {
	let version = root["version"].as_u32().ok_or("snapshot: `version` is missing")?;
	if version != VERSION {
		return Err(format!("snapshot: unsupported version {version}, expected {VERSION}"));
	}

	let db = root["db"].as_str().ok_or("snapshot: `db` is missing")?.to_owned();
	let mut groups = vec![group_from_json(&db, &root["entities"])?];
	for (name, entities) in root["external"].entries() {
		groups.push(group_from_json(name, entities)?);
	}

	groups.sort_by(|a, b| a.name.cmp(&b.name));
	return Ok((ModelSet { groups }, db));
}

fn group_from_json (name: &str, entities: &JsonValue) -> Result<Group, String> {
	let mut result = Vec::new();
	for (entity_name, raw) in entities.entries() {
		let kind = entity_from_json(raw).map_err(|error| format!("snapshot: {name}.{entity_name}: {error}"))?;
		result.push(Entity {
			name: entity_name.to_owned(),
			file: PathBuf::from(format!("{name}/{entity_name}.yml")),
			kind
		});
	}

	result.sort_by(|a, b| a.name.cmp(&b.name));

	return Ok(Group {
		name: name.to_owned(),
		dir: PathBuf::from(name),
		entities: result
	});
}


fn entity_to_json (entity: &Entity) -> JsonValue {
	let mut result = JsonValue::new_object();
	match &entity.kind {
		EntityKind::Table(table) => {
			result["kind"] = "table".into();
			result["table"] = table.table.as_str().into();

			let mut fields = JsonValue::new_array();
			for field in &table.fields {
				let _ = fields.push(field_to_json(field));
			}

			result["fields"] = fields;

			let mut indexes = JsonValue::new_array();
			for index in &table.indexes {
				let _ = indexes.push(index_to_json(index));
			}

			result["indexes"] = indexes;

			if table.deleted {
				result["deleted"] = true.into();
			}

			put_dialects(&mut result, &table.dialects);
		}

		EntityKind::Enum(value) => {
			result["kind"] = "enum".into();

			let mut values = JsonValue::new_array();
			for item in &value.values {
				let mut item_json = JsonValue::new_object();
				item_json["name"] = item.name.as_str().into();
				if let Some(int) = item.int {
					item_json["int"] = int.into();
				}

				if let Some(db) = &item.db {
					item_json["db"] = db.as_str().into();
				}

				let _ = values.push(item_json);
			}
			result["values"] = values;
		}
	}

	return result;
}

fn field_to_json (field: &Field) -> JsonValue {
	let mut result = JsonValue::new_object();
	result["name"] = field.name.as_str().into();
	if let Some(column) = &field.column {
		result["column"] = column.as_str().into();
	}

	result["type"] = field.field_type.to_string().into();
	if field.nullable {
		result["nullable"] = true.into();
	}

	if let Some(target) = &field.enum_ref {
		result["enum"] = target.to_string().into();
	}

	if let Some(default) = &field.default {
		result["default"] = match default {
			Default::Int(value) => json::object! { "int": *value },
			Default::Float(value) => json::object! { "float": *value },
			Default::Bool(value) => json::object! { "bool": *value },
			Default::Str(value) => json::object! { "str": value.as_str() },
			Default::Now => "now".into()
		};
	}

	if field.update_now {
		result["update"] = "now".into();
	}

	if field.auto {
		result["auto"] = true.into();
	}

	put_dialects(&mut result, &field.dialects);
	return result;
}

fn index_to_json (index: &Index) -> JsonValue {
	let mut result = JsonValue::new_object();
	result["name"] = index.name.as_str().into();

	let mut columns = JsonValue::new_array();
	for column in &index.columns {
		let _ = columns.push(column.as_str());
	}

	result["columns"] = columns;

	if index.primary {
		result["primary"] = true.into();
	}

	if index.unique {
		result["unique"] = true.into();
	}

	if let Some(link) = &index.link {
		let mut link_json = JsonValue::new_object();
		link_json["target"] = link.target.to_string().into();

		if let Some(target_index) = &link.index {
			link_json["index"] = target_index.as_str().into();
		}

		link_json["delete"] = delete_name(link.delete).into();
		result["link"] = link_json;
	}

	if let Some(constraint) = &index.constraint {
		result["constraint"] = constraint.as_str().into();
	}

	put_dialects(&mut result, &index.dialects);
	return result;
}

fn delete_name (policy: DeletePolicy) -> &'static str {
	return match policy {
		DeletePolicy::Restrict => "restrict",
		DeletePolicy::Cascade => "cascade",
		DeletePolicy::SetNull => "set_null",
		DeletePolicy::None => "none"
	};
}

fn put_dialects (target: &mut JsonValue, dialects: &[DialectBlock]) {
	for block in dialects {
		target[block.dialect.as_str()] = raw_to_json(&block.value);
	}
}

fn raw_to_json (value: &Raw) -> JsonValue {
	return match value {
		Raw::Null => JsonValue::Null,
		Raw::Bool(value) => (*value).into(),
		Raw::Int(value) => (*value).into(),
		Raw::Float(value) => (*value).into(),
		Raw::Str(value) => value.as_str().into(),
		Raw::Seq(items) => JsonValue::Array(items.iter().map(raw_to_json).collect()),
		Raw::Map(items) => {
			let mut result = JsonValue::new_object();
			for (key, item) in items {
				result[key.as_str()] = raw_to_json(item);
			}

			result
		}
	};
}


const DIALECTS: [&str; 2] = ["mysql", "postgres"];

fn entity_from_json (raw: &JsonValue) -> Result<EntityKind, String> {
	return match raw["kind"].as_str() {
		Some("table") => {
			let mut fields = Vec::new();
			for item in raw["fields"].members() {
				fields.push(field_from_json(item)?);
			}

			let mut indexes = Vec::new();
			for item in raw["indexes"].members() {
				indexes.push(index_from_json(item)?);
			}

			Ok(EntityKind::Table(Table {
				table: raw["table"].as_str().ok_or("`table` is missing")?.to_owned(),
				fields,
				indexes,
				deleted: raw["deleted"].as_bool().unwrap_or(false),
				dialects: dialects_from_json(raw)
			}))
		}
		Some("enum") => {
			let mut values = Vec::new();
			for item in raw["values"].members() {
				values.push(EnumValue {
					name: item["name"].as_str().ok_or("enum value `name` is missing")?.to_owned(),
					int: item["int"].as_i64(),
					db: item["db"].as_str().map(str::to_owned),
					doc: None,
					pos: Pos::default()
				});
			}

			Ok(EntityKind::Enum(Enum { values }))
		}
		_ => Err("`kind` must be table or enum".to_owned())
	};
}

fn field_from_json (raw: &JsonValue) -> Result<Field, String> {
	let name = raw["name"].as_str().ok_or("field `name` is missing")?.to_owned();
	let raw_type = raw["type"].as_str().ok_or_else(|| format!("field {name}: `type` is missing"))?;
	let field_type = FieldType::parse(raw_type).ok_or_else(|| format!("field {name}: unknown type `{raw_type}`"))?;

	let default = match &raw["default"] {
		JsonValue::Null => None,
		value if value.as_str() == Some("now") => Some(Default::Now),
		value => Some(
			if let Some(int) = value["int"].as_i64() {
				Default::Int(int)
			} else if let Some(float) = value["float"].as_f64() {
				Default::Float(float)
			} else if let Some(flag) = value["bool"].as_bool() {
				Default::Bool(flag)
			} else if let Some(text) = value["str"].as_str() {
				Default::Str(text.to_owned())
			} else {
				return Err(format!("field {name}: invalid `default`"));
			}
		)
	};

	return Ok(Field {
		column: raw["column"].as_str().map(str::to_owned),
		field_type,
		nullable: raw["nullable"].as_bool().unwrap_or(false),
		enum_ref: raw["enum"].as_str().map(EntityRef::parse),
		default,
		update_now: raw["update"].as_str() == Some("now"),
		auto: raw["auto"].as_bool().unwrap_or(false),
		dialects: dialects_from_json(raw),
		pos: Pos::default(),
		name
	});
}

fn index_from_json (raw: &JsonValue) -> Result<Index, String> {
	let name = raw["name"].as_str().ok_or("index `name` is missing")?.to_owned();
	let columns = raw["columns"]
		.members()
		.map(|item| item.as_str().map(str::to_owned).ok_or_else(|| format!("index {name}: `columns` must contain strings")))
		.collect::<Result<Vec<_>, _>>()?;

	let link = match &raw["link"] {
		JsonValue::Null => None,
		value => Some(Link {
			target: EntityRef::parse(value["target"].as_str().ok_or_else(|| format!("index {name}: link `target` is missing"))?),
			index: value["index"].as_str().map(str::to_owned),
			delete: match value["delete"].as_str() {
				Some("restrict") | None => DeletePolicy::Restrict,
				Some("cascade") => DeletePolicy::Cascade,
				Some("set_null") => DeletePolicy::SetNull,
				Some("none") => DeletePolicy::None,
				Some(other) => return Err(format!("index {name}: unknown delete policy `{other}`"))
			}
		})
	};

	return Ok(Index {
		columns,
		primary: raw["primary"].as_bool().unwrap_or(false),
		unique: raw["unique"].as_bool().unwrap_or(false),
		link,
		constraint: raw["constraint"].as_str().map(str::to_owned),
		dialects: dialects_from_json(raw),
		pos: Pos::default(),
		name
	});
}

fn dialects_from_json (raw: &JsonValue) -> Vec<DialectBlock> {
	let mut result = Vec::new();
	for dialect in DIALECTS {
		if !raw[dialect].is_null() {
			result.push(DialectBlock { dialect: dialect.to_owned(), value: raw_from_json(&raw[dialect]) });
		}
	}

	return result;
}

fn raw_from_json (value: &JsonValue) -> Raw {
	return match value {
		JsonValue::Null => Raw::Null,
		JsonValue::Boolean(value) => Raw::Bool(*value),
		JsonValue::Short(_) | JsonValue::String(_) => Raw::Str(value.as_str().unwrap_or_default().to_owned()),
		JsonValue::Number(_) => match value.as_i64() {
			Some(int) if value.as_f64() == Some(int as f64) => Raw::Int(int),
			_ => Raw::Float(value.as_f64().unwrap_or_default())
		},
		JsonValue::Array(items) => Raw::Seq(items.iter().map(raw_from_json).collect()),
		JsonValue::Object(object) => Raw::Map(object.iter().map(|(key, item)| (key.to_owned(), raw_from_json(item))).collect())
	};
}


pub fn is_same (a: &JsonValue, b: &JsonValue) -> bool {
	return canonical(a) == canonical(b);
}

pub fn pretty (value: &JsonValue) -> String {
	let mut result = String::new();
	write_pretty(value, 0, &mut result);
	result.push('\n');

	return result;
}

const LINE_LIMIT: usize = 160;

fn is_flat (value: &JsonValue) -> bool {
	return match value {
		JsonValue::Object(object) => object.iter().all(|(_, item)| is_flat(item)),
		JsonValue::Array(items) => items.iter().all(|item| !item.is_object() && !item.is_array()),
		_ => true
	};
}

fn write_pretty (value: &JsonValue, depth: usize, out: &mut String) {
	if is_flat(value) {
		let inline = inline(value);
		if depth * 2 + inline.len() <= LINE_LIMIT {
			out.push_str(&inline);
			return;
		}
	}

	let indent = "  ".repeat(depth + 1);
	match value {
		JsonValue::Object(object) => {
			let mut entries: Vec<(&str, &JsonValue)> = object.iter().collect();
			entries.sort_by(|a, b| a.0.cmp(b.0));

			out.push_str("{\n");

			for (i, (key, item)) in entries.iter().enumerate() {
				out.push_str(&indent);
				out.push_str(&JsonValue::from(*key).dump());
				out.push_str(": ");
				write_pretty(item, depth + 1, out);
				out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
			}

			out.push_str(&"  ".repeat(depth));
			out.push('}');
		}
		JsonValue::Array(items) => {
			out.push_str("[\n");

			for (i, item) in items.iter().enumerate() {
				out.push_str(&indent);
				write_pretty(item, depth + 1, out);
				out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
			}

			out.push_str(&"  ".repeat(depth));
			out.push(']');
		}
		_ => out.push_str(&value.dump())
	}
}

fn inline (value: &JsonValue) -> String {
	return match value {
		JsonValue::Object(object) => {
			if object.is_empty() {
				return "{}".to_owned();
			}

			let mut entries: Vec<(&str, &JsonValue)> = object.iter().collect();
			entries.sort_by(|a, b| a.0.cmp(b.0));

			let items: Vec<String> = entries
				.iter()
				.map(|(key, item)| format!("{}: {}", JsonValue::from(*key).dump(), inline(item)))
				.collect();

			format!("{{ {} }}", items.join(", "))
		}
		JsonValue::Array(items) => {
			let items: Vec<String> = items.iter().map(inline).collect();
			format!("[{}]", items.join(", "))
		}
		_ => value.dump()
	};
}
