//! Semantic checks of a loaded model set (concept §2, §2.1)

use std::collections::{HashMap, HashSet};
use crate::{error::ModelError, ir::*};


const RESERVED: &[&str] = &[
	// Zig
	"addrspace", "align", "allowzero", "and", "anyframe", "anytype", "asm", "break", "callconv", "catch",
	"comptime", "const", "continue", "defer", "else", "enum", "errdefer", "error", "export", "extern", "fn",
	"for", "if", "inline", "linksection", "noalias", "noinline", "nosuspend", "opaque", "or", "orelse",
	"packed", "pub", "resume", "return", "struct", "suspend", "switch", "test", "threadlocal", "try",
	"union", "unreachable", "var", "volatile", "while", "type", "anyerror", "anyopaque", "bool", "void",
	"noreturn", "true", "false", "null", "undefined", "usize", "isize",
	// Rust
	"as", "crate", "dyn", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "ref", "self", "Self",
	"static", "super", "trait", "unsafe", "use", "where", "async", "await", "abstract", "become", "box",
	"do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
	// C
	"auto", "case", "char", "default", "double", "float", "goto", "int", "long", "register", "short",
	"signed", "sizeof", "typedef", "unsigned"
];

const ENTITY_DECLS: &[&str] = &["Field", "Where", "Table", "where", "select", "count", "exists", "on", "deinit"];

pub fn is_reserved_field (name: &str) -> bool {
	return ENTITY_DECLS.contains(&name);
}

pub fn is_identifier (name: &str) -> bool {
	let mut chars = name.chars();
	return match chars.next() {
		Some(first) => (first.is_ascii_alphabetic() || first == '_') && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
		None => false
	};
}

fn is_reserved (name: &str) -> bool {
	return RESERVED.contains(&name) || is_int_type_name(name);
}

/// `u8`, `i32`, ...
fn is_int_type_name (name: &str) -> bool {
	let rest = match name.strip_prefix('u').or_else(|| name.strip_prefix('i')).or_else(|| name.strip_prefix('f')) {
		Some(rest) => rest,
		None => return false
	};

	return !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit());
}


pub fn check (set: &ModelSet, errors: &mut Vec<ModelError>) {
	for group in &set.groups {
		if !is_identifier(&group.name) || is_reserved(&group.name) {
			errors.push(ModelError::file(&group.dir, format!("group name `{}` must be an identifier and not a reserved word", group.name)));
		}

		check_entity_names(group, errors);

		for entity in &group.entities {
			match &entity.kind {
				EntityKind::Table(table) => check_table(set, group, entity, table, errors),
				EntityKind::Enum(value) => check_enum(entity, value, errors)
			}
		}
	}

	check_enum_usages(set, errors);
}

fn check_entity_names (group: &Group, errors: &mut Vec<ModelError>) {
	let mut seen: HashMap<String, &Entity> = HashMap::new();
	for entity in &group.entities {
		if !is_identifier(&entity.name) || is_reserved(&entity.name) {
			let err = ModelError::file(
				&entity.file,
				format!(
					"entity name `{}` (file name) must be an identifier in every target language and not a reserved word",
					entity.name
				)
			);

			errors.push(err);
		}

		let lower = entity.name.to_lowercase();
		if let Some(other) = seen.get(&lower) {
			let err = ModelError::file(
				&entity.file,
				format!(
					"entity `{}` differs from `{}` only by case (file names collide on case-insensitive file systems)",
					entity.name, other.name
				)
			);

			errors.push(err);
		} else {
			seen.insert(lower, entity);
		}
	}
}

fn check_table (set: &ModelSet, group: &Group, entity: &Entity, table: &Table, errors: &mut Vec<ModelError>) {
	let file = &entity.file;
	let mut error = |pos: Pos, message: String| errors.push(ModelError::at(file, pos, message));

	if table.table.is_empty() {
		error(Pos { line: 1, col: 1 }, "`table` must not be empty".to_owned());
	}

	if table.fields.is_empty() {
		error(Pos { line: 1, col: 1 }, "table must have at least one field".to_owned());
	}

	let mut columns = HashSet::new();
	for field in &table.fields {
		if !is_identifier(&field.name) {
			error(field.pos, format!("field name `{}` must be an identifier", field.name));
		} else if is_reserved_field(&field.name) {
			error(field.pos, format!("field name `{}` is reserved by generated bindings (use `column:` to keep the column name)", field.name));
		}

		if !columns.insert(field.column_name()) {
			error(field.pos, format!("field `{}`: column `{}` is used twice", field.name, field.column_name()));
		}

		let enum_entity = match &field.enum_ref {
			Some(target) => match set.find_entity(&group.name, target) {
				Some(Entity { kind: EntityKind::Enum(value), .. }) => Some(value),
				Some(_) => {
					error(field.pos, format!("field `{}`: `{target}` is not an enum", field.name));
					None
				}
				None => {
					error(field.pos, format!("field `{}`: enum `{target}` not found", field.name));
					None
				}
			},
			None => None
		};

		if field.field_type == FieldType::Enum && field.enum_ref.is_none() {
			error(field.pos, format!("field `{}`: type `enum` requires `enum: <Entity>`", field.name));
		}

		if field.enum_ref.is_some() && !(field.field_type.is_string() || field.field_type.is_integer() || field.field_type == FieldType::Enum) {
			error(field.pos, format!("field `{}`: enum can be stored only as string(N), char(N), text, an integer or enum, not {}", field.name, field.field_type));
		}

		if field.update_now && field.field_type != FieldType::DateTime {
			error(field.pos, format!("field `{}`: `update: now` requires datetime, not {}", field.name, field.field_type));
		}

		if field.auto {
			if !field.field_type.is_integer() {
				error(field.pos, format!("field `{}`: `auto` requires an integer type", field.name));
			}

			if !table.indexes.iter().any(|index| index.columns.contains(&field.name)) {
				error(field.pos, format!("field `{}`: `auto` field must be a part of some index (primary or other)", field.name));
			}
		}

		if let Some(default) = &field.default {
			if let Some(message) = check_default(field, default, enum_entity) {
				error(field.pos, format!("field `{}`: {message}", field.name));
			}
		}
	}

	for index in &table.indexes {
		let mut seen = HashSet::new();
		for column in &index.columns {
			if table.find_field(column).is_none() {
				error(index.pos, format!("index `{}`: field `{column}` not found", index.name));
			}

			if !seen.insert(column) {
				error(index.pos, format!("index `{}`: field `{column}` is listed twice", index.name));
			}
		}

		if index.primary && index.link.is_some() {
			error(index.pos, "primary key can not have `link`".to_owned());
		}

		if let Some(link) = &index.link {
			if let Some(message) = check_link(set, group, table, index, link) {
				error(index.pos, format!("index `{}`: {message}", index.name));
			}
		}
	}
}

fn check_default (field: &Field, default: &Default, enum_entity: Option<&Enum>) -> Option<String> {
	if field.enum_ref.is_some() {
		return match (default, enum_entity) {
			(Default::Str(name), Some(value)) => {
				if value.values.iter().any(|item| &item.name == name) {
					None
				} else {
					Some(format!("default `{name}` is not a value of the enum"))
				}
			}
			(Default::Str(_), None) => None,
			_ => Some("default of an enum field must be a value name".to_owned())
		};
	}

	let field_type = field.field_type;
	if let Some((min, max)) = field_type.int_range() {
		return match default {
			Default::Int(value) if (*value as i128) >= min && (*value as i128) <= max => None,
			Default::Int(value) => Some(format!("default {value} does not fit into {field_type}")),
			_ => Some(format!("default must be an integer for {field_type}"))
		};
	}

	return match field_type {
		FieldType::F32 | FieldType::F64 => match default {
			Default::Int(_) | Default::Float(_) => None,
			_ => Some(format!("default must be a number for {field_type}"))
		},
		FieldType::Bool => match default {
			Default::Bool(_) => None,
			_ => Some("default must be true or false for bool".to_owned())
		},
		FieldType::String(size) | FieldType::Char(size) => match default {
			Default::Str(value) if value.chars().count() <= size as usize => None,
			Default::Str(value) => Some(format!("default \"{value}\" is longer than {size}")),
			_ => Some(format!("default must be a string for {field_type}"))
		},
		FieldType::Text | FieldType::Json => match default {
			Default::Str(_) => None,
			_ => Some(format!("default must be a string for {field_type}"))
		},
		FieldType::DateTime => match default {
			Default::Now | Default::Str(_) => None,
			_ => Some("default must be `now` or a datetime string".to_owned())
		},
		_ => Some(format!("default is not supported for {field_type}"))
	};
}

fn check_link (set: &ModelSet, group: &Group, table: &Table, index: &Index, link: &Link) -> Option<String> {
	let target_entity = match set.find_entity(&group.name, &link.target) {
		Some(entity) => entity,
		None => return Some(format!(
			"link target `{}` not found (models referenced by `link` must be available to the generator)",
			link.target
		))
	};

	let target = match &target_entity.kind {
		EntityKind::Table(target) => target,
		EntityKind::Enum(_) => return Some(format!("link target `{}` is an enum, not a table", link.target))
	};

	let target_index = match &link.index {
		Some(name) => match target.find_index(name) {
			Some(target_index) => {
				if !target_index.primary && !target_index.unique {
					return Some(format!("link target index `{}.{name}` must be unique", link.target));
				}

				target_index
			}
			None => return Some(format!("link target index `{}.{name}` not found", link.target))
		},
		None => match target.primary() {
			Some(primary) => primary,
			None => return Some(format!("link target `{}` has no primary key, use `link: {}.<unique index>`", link.target, link.target))
		}
	};

	if index.columns.len() != target_index.columns.len() {
		return Some(format!(
			"has {} columns, but the link target has {}",
			index.columns.len(), target_index.columns.len()
		));
	}

	let target_group = link.target.group_or(&group.name);
	for (source_name, target_name) in index.columns.iter().zip(&target_index.columns) {
		let (source, target_field) = match (table.find_field(source_name), target.find_field(target_name)) {
			(Some(source), Some(target_field)) => (source, target_field),
			// Missing fields are reported by the index checks
			_ => continue
		};

		let source_enum = source.enum_ref
			.as_ref()
			.map(|item| (item.group_or(&group.name).to_owned(), item.name.clone()));

		let target_enum = target_field.enum_ref
			.as_ref()
			.map(|item| (item.group_or(target_group).to_owned(), item.name.clone()));

		if source.field_type != target_field.field_type || source_enum != target_enum {
			return Some(format!(
				"type of `{source_name}` ({}) must match `{}.{target_name}` ({}) exactly",
				source.field_type, link.target, target_field.field_type
			));
		}

		if link.delete == DeletePolicy::SetNull && !source.nullable {
			return Some(format!("`delete: set_null` requires nullable `{source_name}`"));
		}
	}

	return None;
}

fn check_enum (entity: &Entity, value: &Enum, errors: &mut Vec<ModelError>) {
	if value.values.is_empty() {
		errors.push(ModelError::file(&entity.file, "enum must have at least one value".to_owned()));
	}

	let mut names = HashSet::new();
	let mut db_names = HashSet::new();
	let mut ints = HashSet::new();
	for item in &value.values {
		if !is_identifier(&item.name) {
			errors.push(ModelError::at(&entity.file, item.pos, format!("enum value `{}` must be an identifier", item.name)));
		}

		if !names.insert(&item.name) {
			errors.push(ModelError::at(&entity.file, item.pos, format!("enum value `{}` is declared twice", item.name)));
		}

		if !db_names.insert(item.db_name()) {
			errors.push(ModelError::at(&entity.file, item.pos, format!("enum value `{}`: database representation `{}` is used twice", item.name, item.db_name())));
		}

		if let Some(int) = item.int {
			if !ints.insert(int) {
				errors.push(ModelError::at(&entity.file, item.pos, format!("enum value `{}`: int {int} is used twice", item.name)));
			}
		}
	}
}

fn check_enum_usages (set: &ModelSet, errors: &mut Vec<ModelError>) {
	for group in &set.groups {
		for entity in &group.entities {
			let table = match &entity.kind {
				EntityKind::Table(table) => table,
				_ => continue
			};

			for field in &table.fields {
				let target = match &field.enum_ref {
					Some(target) => target,
					None => continue
				};

				let (enum_entity, value) = match set.find_entity(&group.name, target) {
					Some(enum_entity @ Entity { kind: EntityKind::Enum(value), .. }) => (enum_entity, value),
					_ => continue
				};

				if let Some((min, max)) = field.field_type.int_range() {
					for item in &value.values {
						match item.int {
							None => errors.push(ModelError::at(&enum_entity.file, item.pos, format!(
								"enum value `{}` needs `int:` because `{}.{}` stores {} as {}",
								item.name, entity.name, field.name, target, field.field_type
							))),
							Some(int) if (int as i128) < min || (int as i128) > max => errors.push(ModelError::at(&entity.file, field.pos, format!(
								"field `{}`: {} value `{}` (int {int}) does not fit into {}",
								field.name, target, item.name, field.field_type
							))),
							_ => {}
						}
					}
				}

				if let Some(size) = field.field_type.max_length() {
					for item in &value.values {
						if item.db_name().chars().count() > size as usize {
							errors.push(ModelError::at(&entity.file, field.pos, format!(
								"field `{}`: {} value `{}` (\"{}\") does not fit into {}",
								field.name, target, item.name, item.db_name(), field.field_type
							)));
						}
					}
				}
			}
		}
	}
}
