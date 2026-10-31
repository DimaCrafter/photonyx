use std::{fs, io::ErrorKind, path::{Path, PathBuf}};
use json::JsonValue;
use photonyx_models::{ir::ModelSet, snapshot};
use crate::{dialect::Statement, diff::Op};


pub const SNAPSHOT_FILE: &str = "models.snapshot.json";

pub fn read_snapshot (migrations_dir: &Path) -> Result<Option<(ModelSet, JsonValue)>, String> {
	let path = migrations_dir.join(SNAPSHOT_FILE);
	let raw = match fs::read_to_string(&path) {
		Ok(raw) => raw,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(format!("{}: {error}", path.display()))
	};

	let hint = "if it has git conflict markers, resolve them with `onyx models snapshot --db <db>`";
	let root = json::parse(&raw).map_err(|error| format!("{}: invalid JSON ({error}); {hint}", path.display()))?;
	let (set, _) = snapshot::from_json(&root).map_err(|error| format!("{}: {error}", path.display()))?;
	return Ok(Some((set, root)));
}


/// `YYYYMMDD-NNN-<name>.sql` with the next free `NNN` for `date` in the directory
pub fn next_file_name (migrations_dir: &Path, date: &str, name: &str) -> Result<String, String> {
	let mut last = 0;
	match fs::read_dir(migrations_dir) {
		Ok(entries) => {
			for entry in entries {
				let file_name = entry
					.map_err(|error| format!("{}: {error}", migrations_dir.display()))?
					.file_name();

				let file_name = file_name.to_string_lossy();
				if !file_name.ends_with(".sql") {
					continue;
				}

				let file_num = file_name
					.strip_prefix(date)
					.and_then(|rest| rest.strip_prefix('-'))
					.and_then(|rest| rest.get(..3));

				if let Some(number) = file_num {
					if let Ok(number) = number.parse::<u32>() {
						last = last.max(number);
					}
				}
			}
		}
		Err(error) if error.kind() == ErrorKind::NotFound => {}
		Err(error) => return Err(format!("{}: {error}", migrations_dir.display()))
	}

	if last >= 999 {
		return Err(format!("{}: no free migration number for {date}", migrations_dir.display()));
	}

	return Ok(format!("{date}-{:03}-{name}.sql", last + 1));
}

/// Lowercase `a-z0-9` words joined by `-`, at most 60 characters
pub fn slug (raw: &str) -> String {
	let mut result = String::new();
	for ch in raw.chars() {
		if ch.is_ascii_alphanumeric() {
			result.push(ch.to_ascii_lowercase());
		} else if !result.is_empty() && !result.ends_with('-') {
			result.push('-');
		}
	}

	if result.len() > 60 {
		let end = result[..=60].rfind('-').unwrap_or(60);
		result.truncate(end);
	}

	return result.trim_end_matches('-').to_owned();
}

pub fn derive_name (changes: &[TableChange]) -> String {
	let name = match changes {
		[] => "empty".to_owned(),
		[change] => match change.action {
			TableAction::Create => format!("create-{}", change.table),
			TableAction::Drop => format!("drop-{}", change.table),
			TableAction::CreateType => format!("create-type-{}", change.table),
			TableAction::DropType => format!("drop-type-{}", change.table),
			TableAction::Alter | TableAction::AlterType => {
				let mut parts = vec![change.table.clone()];
				let words = |prefix: char|
					change.items
						.iter()
						.filter_map(move |item| item.strip_prefix(prefix))
						.filter(|item| !item.contains('('))
						.map(str::to_owned)
						.collect::<Vec<_>>();

				let added = words('+');
				let dropped = words('-');
				let changed = words('~');

				for (verb, columns) in [("add", added), ("drop", dropped), ("change", changed)] {
					if !columns.is_empty() {
						parts.push(verb.to_owned());
						parts.extend(columns.into_iter().take(2));
					}
				}

				if parts.len() == 1 {
					parts.push("indexes".to_owned());
				}

				parts.join("-")
			}
		},
		_ => {
			let is_table_create = changes.iter().all(|change| change.action == TableAction::Create);
			let prefix = if is_table_create { "create-" } else { "" };

			if changes.len() <= 3 {
				let tables: Vec<&str> = changes.iter().map(|change| change.table.as_str()).collect();
				format!("{prefix}{}", tables.join("-"))
			} else {
				format!("{prefix}{}-and-{}-more", changes[0].table, changes.len() - 1)
			}
		}
	};

	let result = slug(&name);
	if result.is_empty() {
		return "migration".to_owned();
	} else {
		return result;
	};
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAction {
	Create,
	Alter,
	Drop,
	CreateType,
	AlterType,
	DropType
}

impl TableAction {
	pub fn sql (&self) -> &'static str {
		return match self {
			TableAction::Create => "CREATE TABLE",
			TableAction::Alter => "ALTER TABLE",
			TableAction::Drop => "DROP TABLE",
			TableAction::CreateType => "CREATE TYPE",
			TableAction::AlterType => "ALTER TYPE",
			TableAction::DropType => "DROP TYPE"
		};
	}

	fn is_type (&self) -> bool {
		return matches!(self, TableAction::CreateType | TableAction::AlterType | TableAction::DropType);
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableChange {
	pub table: String,
	pub action: TableAction,
	pub items: Vec<String>,
	pub op_idx: Vec<usize>
}

impl TableChange {
	pub fn describe (&self) -> String {
		if self.items.is_empty() {
			return self.action.sql().to_owned();
		}

		return format!("{}  {}", self.action.sql(), self.items.join(" "));
	}
}

pub fn summarize (ops: &[Op]) -> Vec<TableChange> {
	let mut result: Vec<TableChange> = Vec::new();

	for (index, op) in ops.iter().enumerate() {
		let action = match op {
			Op::CreateTable(_) => TableAction::Create,
			Op::DropTable { .. } => TableAction::Drop,
			Op::CreateEnumType { .. } => TableAction::CreateType,
			Op::DropEnumType { .. } => TableAction::DropType,
			Op::AddEnumValues { .. } | Op::RecreateEnumType { .. } => TableAction::AlterType,
			// The function follows `update: now` columns, which are in the summary already
			Op::CreateUpdateNowFunction | Op::DropUpdateNowFunction => continue,
			_ => TableAction::Alter
		};

		let position_opt = result
			.iter()
			.position(|change| change.table == op.get_table() && change.action.is_type() == op.is_enum_type());

		let position = match position_opt {
			Some(position) => position,
			None => {
				result.push(TableChange { table: op.get_table().to_owned(), action, items: Vec::new(), op_idx: Vec::new() });
				result.len() - 1
			}
		};

		let change = &mut result[position];
		change.op_idx.push(index);

		// FKs closing a cycle of new tables and FKs dropped before DROP TABLE belong to CREATE/DROP of the same table
		if action != TableAction::Alter && action != TableAction::AlterType {
			change.action = action;
		}

		let item = match op {
			Op::CreateTable(_) | Op::DropTable { .. } | Op::CreateEnumType { .. } | Op::DropEnumType { .. } => {
				continue
			}
			Op::SetUpdateNow { .. } => {
				continue
			}
			Op::AddEnumValues { from, to, .. } => {
				to
					.iter()
					.filter(|value| !from.contains(value))
					.map(|value| format!("+{value}"))
					.collect::<Vec<_>>()
					.join(" ")
			}
			Op::RecreateEnumType { from, to, .. } => {
				let mut items: Vec<String> = to
					.iter()
					.filter(|value| !from.contains(value))
					.map(|value| format!("+{value}"))
					.collect();

				items.extend(from.iter().filter(|value| !to.contains(value)).map(|value| format!("-{value}")));
				if items.is_empty() {
					items.push("order".to_owned());
				}

				items.join(" ")
			}
			Op::CreateUpdateNowFunction | Op::DropUpdateNowFunction => {
				continue
			}
			Op::AddColumn { column, .. } => {
				format!("+{}", column.name)
			}
			Op::DropColumn { column, .. } => {
				format!("-{column}")
			}
			// `AUTO_INCREMENT` of an added column is turned on by a separate MODIFY
			Op::ModifyColumn { to, .. } if change.items.contains(&format!("+{}", to.name)) => {
				continue
			}
			Op::ModifyColumn { to, .. } => {
				format!("~{}", to.name)
			}
			Op::AddPrimaryKey { columns, .. } => {
				format!("+pk({})", columns.join(","))
			}
			Op::DropPrimaryKey { .. } => {
				"-pk".to_owned()
			}
			Op::AddIndex { index, .. } => {
				format!("+index({})", index.name)
			}
			Op::DropIndex { index, .. } => {
				format!("-index({index})")
			}
			Op::AddForeignKey { fk, .. } => {
				format!("+fk({})", fk.name)
			}
			Op::DropForeignKey { name, .. } => {
				format!("-fk({name})")
			}
			Op::AlterTableOptions { .. } => {
				"options".to_owned()
			}
		};

		if change.action == TableAction::Alter || change.action == TableAction::AlterType {
			change.items.push(item);
		}
	}

	result.sort_by(|a, b| a.table.cmp(&b.table));
	return result;
}


pub struct MigrationText {
	pub text: String,
	/// Operation index --> 1-based line
	pub op_lines: Vec<Option<usize>>
}

pub fn compose (header: &[String], statements: &[Statement], op_count: usize) -> MigrationText {
	let mut text = String::new();
	for line in header {
		if line.is_empty() {
			text.push_str("-- \n");
		} else {
			text.push_str("-- ");
			text.push_str(line);
			text.push('\n');
		}
	}

	text.push('\n');

	let mut op_lines = vec![None; op_count];
	let mut line = header.len() + 2;
	let mut previous_op = None;

	for statement in statements {
		let multiline = statement.sql.contains('\n');
		if previous_op.is_some() && (multiline || previous_op.is_some_and(|(_, was_multiline)| was_multiline)) {
			text.push('\n');
			line += 1;
		}

		if let Some(slot) = op_lines.get_mut(statement.op_idx) {
			slot.get_or_insert(line);
		}

		text.push_str(&statement.sql);
		text.push_str(";\n");
		line += statement.sql.matches('\n').count() + 1;
		previous_op = Some((statement.op_idx, multiline));
	}

	return MigrationText { text, op_lines };
}

/// Writes files through temporary ones and renames them in the given order
pub fn write_files (files: &[(PathBuf, String)]) -> Result<(), String> {
	let mut temporary = Vec::new();
	for (path, content) in files {
		let dir = path.parent().unwrap_or(Path::new("."));
		fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;

		let file_name = path
			.file_name()
			.map(|name| name.to_string_lossy().into_owned())
			.unwrap_or_default();

		let temp = dir.join(format!(".{file_name}.tmp"));
		if let Err(error) = fs::write(&temp, content) {
			for (temp, _) in &temporary {
				let _ = fs::remove_file(temp);
			}

			return Err(format!("{}: {error}", temp.display()));
		}

		temporary.push((temp, path));
	}

	for (temp, path) in temporary {
		fs::rename(&temp, path).map_err(|error| format!("{}: {error}", path.display()))?;
	}

	return Ok(());
}

pub fn snapshot_text (set: &ModelSet, group: &str) -> Option<(String, JsonValue)> {
	let root = snapshot::to_json(set, group)?;
	return Some((snapshot::pretty(&root), root));
}
