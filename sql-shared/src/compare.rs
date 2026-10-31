use std::collections::BTreeMap;
use crate::{diff::{diff, Op}, schema::*};


#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drift {
	/// What would bring the database to the snapshot
	pub ops: Vec<Op>,
	/// Tables of the database that are not in the models
	pub extra: Vec<String>
}

impl Drift {
	pub fn is_empty (&self) -> bool {
		return self.ops.is_empty();
	}
}

/// Compute diff between `actual` introspected database and `expected` snapshot
pub fn drift (actual: &DbSchema, expected: &DbSchema) -> Drift {
	let mut actual = actual.clone();
	let mut expected = expected.clone();

	for (name, table) in actual.tables.iter_mut() {
		if let Some(target) = expected.tables.get(name) {
			// Only the options the models set
			table.options.retain(|key, _| target.options.contains_key(key));

			for (key, value) in table.options.iter_mut() {
				if target.options.get(key).is_some_and(|wanted| wanted.eq_ignore_ascii_case(value)) {
					*value = target.options[key].clone();
				}
			}
		}

		align_table(table);
	}

	for table in expected.tables.values_mut() {
		align_table(table);
	}

	let result = diff(&actual, &expected);
	return Drift {
		ops: result.ops,
		extra: result.untracked
	};
}

fn align_table (table: &mut TableSchema) {
	let collation = table.options.get("collation").cloned();

	for column in table.columns.iter_mut() {
		if column.collation.is_some() && column.collation == collation {
			column.collation = None;
		}

		column.default = match (&column.db_type, column.default.take()) {
			(ColumnType::Float | ColumnType::Double, Some(DefaultValue::Int(value))) => {
				Some(DefaultValue::Float(value as f64))
			},
			(ColumnType::Int { .. }, Some(DefaultValue::Float(value))) if value.fract() == 0.0 => {
				Some(DefaultValue::Int(value as i64))
			},
			(ColumnType::Bool, Some(DefaultValue::Int(value))) if value == 0 || value == 1 => {
				Some(DefaultValue::Bool(value == 1))
			},
			(ColumnType::Int { .. }, Some(DefaultValue::Bool(value))) => {
				Some(DefaultValue::Int(value as i64))
			},
			(_, Some(DefaultValue::Expr(expr))) => {
				Some(DefaultValue::Expr(normalize_expr(&expr)))
			},
			(_, other) => other
		};

		if let Some(generated) = column.generated.as_mut() {
			generated.expr = normalize_expr(&generated.expr);
		}
	}

	for index in table.indexes.iter_mut() {
		if index.using.as_deref().is_some_and(|using| using.eq_ignore_ascii_case("btree")) {
			index.using = None;
		}
	}

	for fk in table.foreign_keys.iter_mut() {
		if fk.on_delete == OnDelete::NoAction {
			fk.on_delete = OnDelete::Restrict;
		}
	}
}


/// Convert SQL expression to a comparable form
pub fn normalize_expr (expr: &str) -> String {
	let unescaped = expr.replace("\\'", "'");

	let mut result = String::new();
	let mut chars = unescaped.chars().peekable();
	while let Some(ch) = chars.next() {
		match ch {
			'\'' => {
				result.push(ch);
				while let Some(inner) = chars.next() {
					result.push(inner);
					if inner == '\'' {
						if chars.peek() == Some(&'\'') {
							result.push(chars.next().unwrap());
							continue;
						}
						break;
					}
				}
			}
			'`' => {}
			_ if ch.is_whitespace() => {}
			_ => result.push(ch.to_ascii_lowercase())
		}
	}

	// Charset introducer before a string literal.
	// MySQL Example: `_utf8mb4'a'` converts to `'a'`.
	let mut cleaned = String::new();
	let mut rest = result.as_str();
	while let Some(position) = rest.find('\'') {
		let (head, tail) = rest.split_at(position);
		let start = head.rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).map(|index| index + 1).unwrap_or(0);

		let word = &head[start..];
		if word.starts_with('_') && word.len() > 1 {
			cleaned.push_str(&head[..start]);
		} else {
			cleaned.push_str(head);
		}

		let mut end = 1;
		let bytes = tail.as_bytes();
		while end < bytes.len() {
			if bytes[end] == b'\'' {
				if end + 1 < bytes.len() && bytes[end + 1] == b'\'' {
					end += 2;
					continue;
				}

				end += 1;
				break;
			}

			end += 1;
		}

		cleaned.push_str(&tail[..end]);
		rest = &tail[end..];
	}

	cleaned.push_str(rest);

	return without_grouping(&without_casts(&cleaned));
}

/// Drops cast postfixes (like `::type` for PostgreSQL) outside of string literals
fn without_casts (expr: &str) -> String {
	let chars: Vec<char> = expr.chars().collect();

	let mut result = String::new();
	let mut i = 0;
	while i < chars.len() {
		let ch = chars[i];
		if ch == '\'' {
			let mut end = i + 1;
			while end < chars.len() {
				if chars[end] == '\'' {
					if chars.get(end + 1) == Some(&'\'') {
						end += 2;
						continue;
					}
					break;
				}
				end += 1;
			}

			let end = (end + 1).min(chars.len());
			result.extend(&chars[i..end]);
			i = end;
			continue;
		}

		if ch == ':' && chars.get(i + 1) == Some(&':') {
			let mut end = i + 2;
			if chars.get(end) == Some(&'"') {
				end = chars[end + 1..].iter().position(|ch| *ch == '"').map(|offset| end + 1 + offset + 1).unwrap_or(chars.len());
			} else {
				while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_' || chars[end] == '.') {
					end += 1;
				}
			}

			// Type modifiers and arrays
			if chars.get(end) == Some(&'(') {
				end = chars[end..].iter().position(|ch| *ch == ')').map(|offset| end + offset + 1).unwrap_or(chars.len());
			}

			while chars.get(end) == Some(&'[') && chars.get(end + 1) == Some(&']') {
				end += 2;
			}

			i = end;
			continue;
		}

		result.push(ch);
		i += 1;
	}

	return result;
}

const SYNONYMS: &[(&str, &str)] = &[
	("ucase", "upper"),
	("lcase", "lower"),
	("substr", "substring"),
	("mid", "substring"),
	("character_length", "char_length"),
	("now", "current_timestamp")
];

fn without_grouping (expr: &str) -> String {
	let mut result = String::new();
	let mut kept = Vec::new();
	let mut quoted = false;

	for ch in expr.chars() {
		if quoted {
			result.push(ch);
			quoted = ch != '\'';
			continue;
		}

		match ch {
			'\'' => {
				quoted = true;
				result.push(ch);
			}
			'(' => {
				let name_start = result
					.rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
					.map(|index| index + 1)
					.unwrap_or(0);

				let call = name_start < result.len();
				if call {
					let name = result[name_start..].to_owned();
					if let Some((_, canonical)) = SYNONYMS.iter().find(|(synonym, _)| *synonym == name) {
						result.truncate(name_start);
						result.push_str(canonical);
					}

					result.push(ch);
				}

				kept.push(call);
			}
			')' => {
				if kept.pop().unwrap_or(true) {
					result.push(ch);
				}
			}
			_ => result.push(ch)
		}
	}

	return result;
}

pub fn describe_column (column: &ColumnSchema) -> String {
	let mut result = format!("{} {}", column.db_type, if column.nullable { "NULL" } else { "NOT NULL" });
	if let Some(generated) = &column.generated {
		result.push_str(&format!(
			" GENERATED AS ({}) {}",
			generated.expr,
			if generated.stored { "STORED" } else { "VIRTUAL" }
		));
	}

	if let Some(value) = &column.default {
		result.push_str(" DEFAULT ");
		result.push_str(&match value {
			DefaultValue::Int(value) => value.to_string(),
			DefaultValue::Float(value) => value.to_string(),
			DefaultValue::Bool(value) => value.to_string(),
			DefaultValue::Str(value) => format!("'{value}'"),
			DefaultValue::Now => "CURRENT_TIMESTAMP".to_owned(),
			DefaultValue::Expr(expr) => format!("({expr})")
		});
	}

	if column.on_update_now {
		result.push_str(" ON UPDATE CURRENT_TIMESTAMP");
	}

	if column.auto_increment {
		result.push_str(" AUTO_INCREMENT");
	}

	if let Some(collation) = &column.collation {
		result.push_str(" COLLATE ");
		result.push_str(collation);
	}

	return result;
}

fn describe_index (index: &IndexSchema) -> String {
	let mut result = format!("{}({})", if index.unique { "unique " } else { "" }, index.columns.join(", "));
	if let Some(using) = &index.using {
		result.push_str(" using ");
		result.push_str(using);
	}

	return result;
}

fn describe_options (options: &BTreeMap<String, String>) -> String {
	if options.is_empty() {
		return "none".to_owned();
	}

	return options.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(", ");
}

pub fn describe_op (op: &Op) -> String {
	return match op {
		Op::CreateTable(table) => {
			format!("table `{}` is missing", table.name)
		},
		Op::DropTable { table } => {
			format!("table `{table}` is marked `deleted: true`, but exists")
		},
		Op::AddColumn { column, .. } => {
			format!("column `{}` is missing: {}", column.name, describe_column(column))
		}
		Op::DropColumn { column, .. } => {
			format!("column `{column}` is not in the models")
		}
		Op::ModifyColumn { from, to, .. } => {
			format!("column `{}`: {} in the database, {} in the models", to.name, describe_column(from), describe_column(to))
		}
		Op::AddPrimaryKey { columns, .. } => {
			format!("primary key ({}) is missing or differs", columns.join(", "))
		}
		Op::DropPrimaryKey { .. } => {
			"primary key of the database is not in the models or differs".to_owned()
		}
		Op::AddIndex { index, .. } => {
			format!("index `{}` {} is missing or differs", index.name, describe_index(index))
		}
		Op::DropIndex { index, .. } => {
			format!("index `{index}` of the database is not in the models or differs")
		}
		Op::AddForeignKey { fk, .. } => {
			format!("foreign key `{}` ({}) --> {}({}) is missing or differs", fk.name, fk.columns.join(", "), fk.ref_table, fk.ref_columns.join(", "))
		}
		Op::DropForeignKey { name, .. } => {
			format!("foreign key `{name}` of the database is not in the models or differs")
		}
		Op::AlterTableOptions { from, to, .. } => {
			format!("options: {} in the database, {} in the models", describe_options(from), describe_options(to))
		}
		Op::CreateEnumType { name, values } => {
			format!("enum type `{name}` ({}) is missing", values.join(", "))
		}
		Op::AddEnumValues { name, from, to } | Op::RecreateEnumType { name, from, to, .. } => {
			format!("enum type `{name}`: ({}) in the database, ({}) in the models", from.join(", "), to.join(", "))
		}
		Op::DropEnumType { name } => {
			format!("enum type `{name}` of the database is not in the models")
		}
		Op::SetUpdateNow { columns, .. } => {
			format!("`update: now` columns are ({}) in the models", columns.join(", "))
		}
		Op::CreateUpdateNowFunction => {
			"the `update: now` function is missing".to_owned()
		}
		Op::DropUpdateNowFunction => {
			"the `update: now` function is not needed by the models".to_owned()
		}
	};
}

pub fn describe (drift: &Drift) -> Vec<(String, Vec<String>)> {
	let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
	for op in &drift.ops {
		if matches!(op, Op::SetUpdateNow { .. } | Op::CreateUpdateNowFunction | Op::DropUpdateNowFunction) {
			continue;
		}

		let added = |name: &str|
			drift.ops
			.iter()
			.any(|other| matches!(other, Op::AddColumn { table, column, .. } if table == op.get_table() && column.name == name));

		if let Op::ModifyColumn { to, .. } = op {
			if added(&to.name) {
				continue;
			}
		}

		result
			.entry(op.get_table().to_owned())
			.or_default()
			.push(describe_op(op));
	}

	return result.into_iter().collect();
}
