use crate::ir::*;


pub fn entity_to_yml (entity: &Entity, header: &[String]) -> String {
	let mut out = String::new();
	for line in header {
		out.push_str(if line.is_empty() { "#" } else { "# " });
		out.push_str(line);
		out.push('\n');
	}

	match &entity.kind {
		EntityKind::Table(table) => table_to_yml(table, &mut out),
		EntityKind::Enum(value) => enum_to_yml(value, &mut out)
	}

	return out;
}

fn table_to_yml (table: &Table, out: &mut String) {
	out.push_str("kind: table\n");
	out.push_str(&format!("table: {}\n", scalar(&table.table)));
	if table.deleted {
		out.push_str("deleted: true\n");
	}

	out.push_str("fields:\n");
	let width = table.fields.iter().map(|field| key(&field.name).chars().count() + 1).max().unwrap_or(0);
	for field in &table.fields {
		out.push_str(&format!("  {:<width$} {}\n", format!("{}:", key(&field.name)), field_value(field)));
	}

	if !table.indexes.is_empty() {
		out.push_str("indexes:\n");
		let width = table.indexes.iter().map(|index| key(&index.name).chars().count() + 1).max().unwrap_or(0);
		for index in &table.indexes {
			out.push_str(&format!("  {:<width$} {}\n", format!("{}:", key(&index.name)), index_value(index)));
		}
	}

	for block in &table.dialects {
		out.push_str(&format!("{}: {}\n", block.dialect, raw(&block.value)));
	}
}

fn field_value (field: &Field) -> String {
	let ty = format!("{}{}", if field.nullable { "?" } else { "" }, field.field_type);
	let simple = field.column.is_none() && field.enum_ref.is_none() && field.default.is_none() && !field.update_now && !field.auto && field.dialects.is_empty();
	if simple {
		return ty;
	}

	let mut parts = vec![format!("type: {ty}")];
	if let Some(column) = &field.column {
		parts.push(format!("column: {}", scalar(column)));
	}

	if let Some(target) = &field.enum_ref {
		parts.push(format!("enum: {target}"));
	}

	if let Some(value) = &field.default {
		parts.push(format!("default: {}", default_value(value)));
	}

	if field.update_now {
		parts.push("update: now".to_owned());
	}

	if field.auto {
		parts.push("auto: true".to_owned());
	}

	for block in &field.dialects {
		parts.push(format!("{}: {}", block.dialect, raw(&block.value)));
	}

	return format!("{{ {} }}", parts.join(", "));
}

fn default_value (value: &Default) -> String {
	return match value {
		Default::Int(value) => value.to_string(),
		Default::Float(value) => float(*value),
		Default::Bool(value) => value.to_string(),
		Default::Str(value) => scalar(value),
		Default::Now => "now".to_owned()
	};
}

fn index_value (index: &Index) -> String {
	let columns = format!("[{}]", index.columns.iter().map(|name| scalar(name)).collect::<Vec<_>>().join(", "));
	let simple = !index.unique && index.link.is_none() && index.constraint.is_none() && index.dialects.is_empty();
	if simple {
		return columns;
	}

	let mut parts = vec![format!("columns: {columns}")];
	if index.unique {
		parts.push("unique: true".to_owned());
	}

	if let Some(link) = &index.link {
		let target = match &link.index {
			Some(name) => format!("{}.{name}", link.target),
			None => link.target.to_string()
		};

		parts.push(format!("link: {target}"));
		parts.push(format!("delete: {}", match link.delete {
			DeletePolicy::Restrict => "restrict",
			DeletePolicy::Cascade => "cascade",
			DeletePolicy::SetNull => "set_null",
			DeletePolicy::None => "none"
		}));
	}

	if let Some(constraint) = &index.constraint {
		parts.push(format!("constraint: {}", scalar(constraint)));
	}

	for block in &index.dialects {
		parts.push(format!("{}: {}", block.dialect, raw(&block.value)));
	}

	return format!("{{ {} }}", parts.join(", "));
}

fn enum_to_yml (value: &Enum, out: &mut String) {
	out.push_str("kind: enum\n");

	let plain = value.values.iter().all(|item| item.int.is_none() && item.db.is_none() && item.doc.is_none());
	if plain {
		let names: Vec<String> = value.values.iter().map(|item| scalar(&item.name)).collect();
		out.push_str(&format!("values: [{}]\n", names.join(", ")));
		return;
	}

	out.push_str("values:\n");
	let width = value.values.iter().map(|item| key(&item.name).chars().count() + 1).max().unwrap_or(0);
	for item in &value.values {
		let mut parts = Vec::new();
		if let Some(int) = item.int {
			parts.push(format!("int: {int}"));
		}

		if let Some(db) = &item.db {
			parts.push(format!("db: {}", scalar(db)));
		}

		if let Some(doc) = &item.doc {
			parts.push(format!("doc: {}", scalar(doc)));
		}

		if parts.is_empty() {
			out.push_str(&format!("  {}:\n", key(&item.name)));
		} else {
			out.push_str(&format!("  {:<width$} {{ {} }}\n", format!("{}:", key(&item.name)), parts.join(", ")));
		}
	}
}

fn raw (value: &Raw) -> String {
	return match value {
		Raw::Null => "null".to_owned(),
		Raw::Bool(value) => value.to_string(),
		Raw::Int(value) => value.to_string(),
		Raw::Float(value) => float(*value),
		Raw::Str(value) => scalar(value),
		Raw::Seq(items) => format!("[{}]", items.iter().map(raw).collect::<Vec<_>>().join(", ")),
		Raw::Map(entries) => format!(
			"{{ {} }}",
			entries
				.iter()
				.map(|(name, item)| format!("{}: {}", key(name), raw(item)))
				.collect::<Vec<_>>()
				.join(", ")
		)
	};
}

fn float (value: f64) -> String {
	let text = value.to_string();
	if text.contains(['.', 'e', 'E']) || !value.is_finite() {
		return text;
	} else {
		return format!("{text}.0");
	}
}

fn is_special (value: &str) -> bool {
	return matches!(value.to_ascii_lowercase().as_str(), "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "y" | "n" | "~");
}

pub fn scalar (value: &str) -> String {
	let mut chars = value.chars();
	let plain = match chars.next() {
		Some(first) => (first.is_ascii_alphabetic() || first == '_') && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
		None => false
	};

	if plain && !is_special(value) {
		return value.to_owned();
	}

	return json::stringify(value);
}

fn key (value: &str) -> String {
	return scalar(value);
}
