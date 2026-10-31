use std::{collections::BTreeMap, fmt::Write, path::{Component, Path, PathBuf}};
use photonyx_models::ir::*;


const KEYWORDS: &[&str] = &[
	"addrspace", "align", "allowzero", "and", "anyframe", "anytype", "asm", "break", "callconv", "catch",
	"comptime", "const", "continue", "defer", "else", "enum", "errdefer", "error", "export", "extern", "fn",
	"for", "if", "inline", "linksection", "noalias", "noinline", "nosuspend", "opaque", "or", "orelse",
	"packed", "pub", "resume", "return", "struct", "suspend", "switch", "test", "threadlocal", "try",
	"union", "unreachable", "var", "volatile", "while",
	"true", "false", "null", "undefined", "type", "bool", "void", "noreturn", "anyerror", "anyopaque",
	"usize", "isize", "comptime_int", "comptime_float"
];

fn ident (name: &str) -> String {
	let primitive_int = name.len() > 1
		&& (name.starts_with('u') || name.starts_with('i') || name.starts_with('f'))
		&& name[1..].chars().all(|ch| ch.is_ascii_digit());

	if KEYWORDS.contains(&name) || primitive_int {
		return format!("@\"{name}\"");
	}

	return name.to_owned();
}

pub(super) fn string_literal (value: &str) -> String {
	let mut result = String::from("\"");

	for ch in value.chars() {
		match ch {
			'"' => result.push_str("\\\""),
			'\\' => result.push_str("\\\\"),
			'\n' => result.push_str("\\n"),
			'\r' => result.push_str("\\r"),
			'\t' => result.push_str("\\t"),
			_ => result.push(ch)
		}
	}

	result.push('"');
	return result;
}

fn source_path (file: &Path, root: &Path) -> String {
	let relative = file.strip_prefix(root).unwrap_or(file);
	let normalized: PathBuf = relative.components().filter(|part| !matches!(part, Component::CurDir)).collect();
	return normalized.to_string_lossy().into_owned();
}

fn value_type (field: &Field, enum_alias: Option<&str>) -> String {
	if let Some(alias) = enum_alias {
		return alias.to_owned();
	}

	return match field.field_type {
		FieldType::U8 => "u8",
		FieldType::U16 => "u16",
		FieldType::U32 => "u32",
		FieldType::U64 => "u64",
		FieldType::I8 => "i8",
		FieldType::I16 => "i16",
		FieldType::I32 => "i32",
		FieldType::I64 => "i64",
		FieldType::F32 => "f32",
		FieldType::F64 => "f64",
		FieldType::Bool => "bool",
		FieldType::String(_) | FieldType::Char(_) | FieldType::Text | FieldType::Json => "[]const u8",
		FieldType::DateTime => "*m.DateTime",
		// `type: enum` always has `enum:`, checked by photonyx-models
		FieldType::Enum => "u32"
	}.to_owned();
}

fn default_literal (field: &Field) -> Option<String> {
	let default = match &field.default {
		Some(default) => default,
		None => return if field.nullable { Some("null".to_owned()) } else { None }
	};

	if field.enum_ref.is_some() {
		if let Default::Str(name) = default {
			return Some(format!(".{}", ident(name)));
		}
	}

	return match default {
		Default::Int(value) => Some(value.to_string()),
		Default::Float(value) => Some(format!("{value:?}")),
		Default::Bool(value) => Some(value.to_string()),
		Default::Now => Some("m.CURRENT_DATE".to_owned()),
		// DateTime literals can not be a Zig default value
		Default::Str(_) if field.field_type == FieldType::DateTime => None,
		Default::Str(value) => Some(string_literal(value))
	};
}

/// Name under which an enum is visible in a table file and the import path for it
fn enum_import (current_group: &str, target: &EntityRef) -> (String, String) {
	return match &target.group {
		Some(group) if group != current_group => (format!("{group}_{}", target.name), format!("../{group}/{}.zig", target.name)),
		_ => (target.name.clone(), format!("{}.zig", target.name))
	};
}

fn render_enum (out: &mut String, name: &str, value: &Enum) {
	let _ = writeln!(out, "pub const {name} = enum {{");
	for item in &value.values {
		if let Some(doc) = &item.doc {
			for line in doc.lines() {
				let _ = writeln!(out, "    /// {line}");
			}
		}

		let _ = writeln!(out, "    {},", ident(&item.name));
	}
	out.push_str("};\n");
}

fn render_table (out: &mut String, set: &ModelSet, group: &Group, name: &str, table: &Table) {
	out.push_str("const m = @import(\"photonyx\").models;\n");

	let mut aliases: BTreeMap<String, String> = BTreeMap::new();
	for field in &table.fields {
		if let Some(target) = &field.enum_ref {
			let (alias, path) = enum_import(&group.name, target);
			aliases.insert(alias, path);
		}
	}

	for (alias, path) in &aliases {
		let target_name = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
		let _ = writeln!(out, "const {alias} = @import(\"{path}\").{target_name};");
	}

	let enum_alias = |field: &Field| field.enum_ref.as_ref().map(|target| enum_import(&group.name, target).0);

	let _ = writeln!(out, "\npub const {name} = struct {{");
	for field in &table.fields {
		let mut field_type = value_type(field, enum_alias(field).as_deref());
		if field.nullable {
			field_type.insert(0, '?');
		}

		match default_literal(field) {
			Some(default) => { let _ = writeln!(out, "    {}: {field_type} = {default},", ident(&field.name)); }
			None => { let _ = writeln!(out, "    {}: {field_type},", ident(&field.name)); }
		}
	}

	let field_names: Vec<String> = table.fields.iter().map(|field| ident(&field.name)).collect();
	let _ = writeln!(out, "\n    /// For select(): hints instead of strings");
	let _ = writeln!(out, "    pub const Field = enum {{ {} }};", field_names.join(", "));

	out.push_str("\n    /// For where(): ZLS completes fields in `.{ . }`\n");
	out.push_str("    pub const Where = struct {\n");
	for field in &table.fields {
		let mut field_type = value_type(field, enum_alias(field).as_deref());
		if field.nullable {
			field_type.insert(0, '?');
		}

		let _ = writeln!(out, "        {}: ?m.Cond({field_type}) = null,", ident(&field.name));
	}
	out.push_str("    };\n\n");

	out.push_str("    pub const Table = m.Table(");
	let _ = write!(out, "{name}, Where, Field, .{{ .db = {}, .model = {}, .columns = &.{{\n", string_literal(&group.name), string_literal(name));

	for field in &table.fields {
		let _ = write!(out, "        .{{ .name = {}", string_literal(field.column_name()));

		if let Some(target) = &field.enum_ref {
			if let Some(Entity { kind: EntityKind::Enum(value), .. }) = set.find_entity(&group.name, target) {
				if field.field_type.is_integer() {
					let ints: Vec<String> = value.values.iter().map(|item| item.int.unwrap_or(0).to_string()).collect();
					let _ = write!(out, ", .enum_int = &.{{ {} }}", ints.join(", "));
				} else {
					let names: Vec<String> = value.values.iter().map(|item| string_literal(item.db_name())).collect();
					let _ = write!(out, ", .enum_db = &.{{ {} }}", names.join(", "));
				}
			}
		}

		out.push_str(" },\n");
	}

	out.push_str("    } });\n\n");

	out.push_str("    // The entity is the table itself: methods are aliases of the thin generic.\n");
	out.push_str("    pub const where = Table.where;\n");
	out.push_str("    pub const select = Table.select;\n");
	out.push_str("    pub const count = Table.count;\n");
	out.push_str("    pub const exists = Table.exists;\n");
	out.push_str("    pub const on = Table.on;\n\n");

	let _ = writeln!(out, "    /// Frees an entity from `first()` (entities of `all()` are freed by the list)");
	let _ = writeln!(out, "    pub fn deinit(self: *{name}) void {{");
	out.push_str("        Table.deinitEntity(self);\n");
	out.push_str("    }\n");

	out.push_str("};\n");
}

/// Relative output path --> File content
pub fn render (set: &ModelSet, root: &Path) -> BTreeMap<PathBuf, String> {
	let mut files = BTreeMap::new();

	let mut db = String::from("// GENERATED by `onyx models generate` - DO NOT EDIT\n\n");
	let mut attach = String::new();

	for group in &set.groups {
		let _ = writeln!(db, "pub const {} = struct {{", group.name);

		// Enums first to avoid import struggle
		for pass_tables in [false, true] {
			for entity in &group.entities {
				let mut out = format!("// GENERATED from {} - DO NOT EDIT\n", source_path(&entity.file, root));

				match &entity.kind {
					EntityKind::Enum(value) if !pass_tables => render_enum(&mut out, &entity.name, value),
					EntityKind::Table(table) if pass_tables && !table.deleted => {
						render_table(&mut out, set, group, &entity.name, table);
						let _ = writeln!(attach, "    {}.{}.Table.attach();", group.name, entity.name);
					}
					_ => continue
				}

				let _ = writeln!(db, "    pub const {0} = @import(\"{1}/{0}.zig\").{0};", entity.name, group.name);
				files.insert(PathBuf::from(&group.name).join(format!("{}.zig", entity.name)), out);
			}
		}

		db.push_str("};\n\n");
	}

	db.push_str("/// Called by photonyx: attaches every table to the models schema below\n");
	db.push_str("pub export fn provide_models() void {\n");
	db.push_str(&attach);
	db.push_str("}\n\n");
	db.push_str("pub const schema = @embedFile(\"schema.json\");\n");
	db.push_str("pub export const models_schema: [*:0]const u8 = schema;\n");

	files.insert(PathBuf::from("db.zig"), db);
	return files;
}
