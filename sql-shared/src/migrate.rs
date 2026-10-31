use std::{collections::BTreeMap, fs, io::ErrorKind, path::{Path, PathBuf}};
use bindings::db::ModelsCheck;
use photonyx_models::{ir::{DialectBlock, ModelSet, Table}, to_runtime};
use sha2::{Digest, Sha256};
use crate::{compare::{drift, Drift}, dialect::{SqlDialect, SqlSession, SqlValue}, diff::{diff, Op}, schema::*, to_db::to_db};


pub const SERVICE_TABLE_PREFIX: &str = "photonyx_";
pub const MIGRATIONS_TABLE: &str = "photonyx_migrations";
pub const MODELS_TABLE: &str = "photonyx_models";
pub const BASELINE_FILE: &str = "00000000-000-baseline.sql";


#[derive(Debug, Clone)]
pub struct MigrationFile {
	/// Full file name
	pub name: String,
	pub path: PathBuf,
	pub text: String,
	pub checksum: String
}

pub fn is_migration_name (name: &str) -> bool {
	let bytes = name.as_bytes();
	return name.ends_with(".sql")
		&& bytes.len() >= 16
		&& bytes[..8].iter().all(u8::is_ascii_digit)
		&& bytes[8] == b'-'
		&& bytes[9..12].iter().all(u8::is_ascii_digit)
		&& (bytes[12] == b'-' || &name[12..] == ".sql");
}

pub fn order_key (name: &str) -> (&str, &str, &str) {
	return (&name[..8], &name[9..12], name);
}

/// Calculate SHA-256 (hex) of the migration file
pub fn checksum (text: &str) -> String {
	let digest = Sha256::digest(text.as_bytes());
	return digest.iter().map(|byte| format!("{byte:02x}")).collect();
}

pub fn list_files (dir: &Path) -> Result<(Vec<MigrationFile>, Vec<String>), String> {
	let entries = match fs::read_dir(dir) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
		Err(error) => return Err(format!("{}: {error}", dir.display()))
	};

	let mut files = Vec::new();
	let mut ignored = Vec::new();
	for entry in entries {
		let path = entry.map_err(|error| format!("{}: {error}", dir.display()))?.path();
		let name = match path.file_name().and_then(|name| name.to_str()) {
			Some(name) if name.ends_with(".sql") && path.is_file() => name.to_owned(),
			_ => continue
		};

		if !is_migration_name(&name) {
			ignored.push(name);
			continue;
		}

		let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
		files.push(MigrationFile {
			checksum: checksum(&text),
			name,
			path,
			text
		});
	}

	files.sort_by(|a, b| order_key(&a.name).cmp(&order_key(&b.name)));
	ignored.sort();
	return Ok((files, ignored));
}


#[derive(Debug, Clone, PartialEq)]
pub struct FileStatement {
	pub sql: String,
	/// 1-based
	pub line: usize
}

/// Splits a migration file by `;` outside of the comments and quotes of the dialect
pub fn split_statements (text: &str, dialect: &dyn SqlDialect) -> Vec<FileStatement> {
	let chars: Vec<char> = text.chars().collect();
	let mut result = Vec::new();
	let mut current = String::new();
	let mut start_line = 0;
	let mut line = 1;
	let mut i = 0;

	while i < chars.len() {
		let ch = chars[i];

		if let Some(end) = dialect.comment_end(&chars, i) {
			if !current.trim().is_empty() {
				current.extend(&chars[i..end]);
			}

			line += chars[i..end].iter().filter(|ch| **ch == '\n').count();
			i = end;
			continue;
		}

		if let Some(end) = dialect.quoted_end(&chars, i, &current) {
			if current.trim().is_empty() {
				start_line = line;
			}

			current.extend(&chars[i..end]);
			line += chars[i..end].iter().filter(|ch| **ch == '\n').count();
			i = end;
			continue;
		}

		if ch == ';' {
			finish(&mut current, start_line, &mut result);
			i += 1;
			continue;
		}

		if !ch.is_whitespace() && current.trim().is_empty() {
			start_line = line;
		}

		current.push(ch);
		if ch == '\n' {
			line += 1;
		}

		i += 1;
	}

	finish(&mut current, start_line, &mut result);
	return result;
}

/// End (exclusive) of a line comment at `chars[start]`
pub fn line_end (chars: &[char], start: usize) -> usize {
	return chars[start..]
		.iter()
		.position(|ch| *ch == '\n')
		.map(|offset| start + offset)
		.unwrap_or(chars.len());
}

/// End (exclusive) of a `/* ... */` comment at `chars[start]`
pub fn block_end (chars: &[char], start: usize) -> usize {
	return chars[start + 2..]
		.windows(2)
		.position(|pair| pair == ['*', '/'])
		.map(|offset| start + 2 + offset + 2)
		.unwrap_or(chars.len());
}

/// End (exclusive) of the text quoted by `chars[start]`
pub fn quote_end (chars: &[char], start: usize, escapes: bool) -> usize {
	let quote = chars[start];
	let mut end = start + 1;
	while end < chars.len() {
		if chars[end] == '\\' && escapes {
			end += 2;
			continue;
		}

		if chars[end] == quote {
			if chars.get(end + 1) == Some(&quote) {
				end += 2;
				continue;
			}
			break;
		}

		end += 1;
	}

	return (end + 1).min(chars.len());
}

fn finish (current: &mut String, start_line: usize, result: &mut Vec<FileStatement>) {
	let sql = current.trim().to_owned();
	if !sql.is_empty() {
		result.push(FileStatement { sql, line: start_line });
	}

	current.clear();
}


pub fn build_service_table (dialect: &dyn SqlDialect, defaults: &[DialectBlock], name: String, columns: Vec<ColumnSchema>) -> Result<TableSchema, String> {
	let mut schema = TableSchema::new(name.as_str());
	schema.primary = vec![columns[0].name.clone()];
	schema.columns = columns;

	let table = Table {
		table: name,
		fields: Vec::new(),
		indexes: Vec::new(),
		deleted: false,
		dialects: Vec::new()
	};

	schema.options = dialect.table_options(&table, defaults)?;

	return Ok(schema);
}

pub fn ensure_service_tables (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, defaults: &[DialectBlock]) -> Result<(), String> {
	let service_tables = vec![
		build_service_table(dialect, defaults, MIGRATIONS_TABLE.to_owned(), vec![
			ColumnSchema::new("name", ColumnType::Varchar(255), false),
			ColumnSchema::new("checksum", ColumnType::Char(64), false),
			ColumnSchema::new("applied_at", ColumnType::DateTime, false)
		])?,
		build_service_table(dialect, defaults, MODELS_TABLE.to_owned(), vec![
			ColumnSchema::new("model", ColumnType::Varchar(255), false),
			ColumnSchema::new("hash", ColumnType::Char(64), false),
			ColumnSchema::new("applied_at", ColumnType::DateTime, false)
		])?
	];

	for table in service_tables {
		if dialect.table_exists(session, &table.name)? {
			continue;
		}

		for statement in dialect.render(&[Op::CreateTable(table)]) {
			session.exec(&statement.sql)?;
		}
	}

	return Ok(());
}


#[derive(Debug, Clone, PartialEq)]
pub struct MigrationRow {
	pub name: String,
	pub checksum: String,
	pub applied_at: String
}

/// `None` if there is no table yet
pub fn read_applied (session: &mut dyn SqlSession, dialect: &dyn SqlDialect) -> Result<Option<Vec<MigrationRow>>, String> {
	if !dialect.table_exists(session, MIGRATIONS_TABLE)? {
		return Ok(None);
	}

	let rows = session.query(&format!("SELECT name, checksum, applied_at FROM {MIGRATIONS_TABLE}"), &[])?;
	let mut result: Vec<MigrationRow> = rows
		.iter()
		.map(|row| MigrationRow {
			name: row.first().and_then(SqlValue::text).unwrap_or_default(),
			checksum: row.get(1).and_then(SqlValue::text).unwrap_or_default(),
			applied_at: row.get(2).and_then(SqlValue::text).unwrap_or_default()
		})
		.collect();

	result.sort_by(|a, b| {
		return match (is_migration_name(&a.name), is_migration_name(&b.name)) {
			(true, true) => order_key(&a.name).cmp(&order_key(&b.name)),
			_ => a.name.cmp(&b.name)
		};
	});

	return Ok(Some(result));
}


#[derive(Debug, Clone, Default)]
pub struct Status {
	/// Migrations table exists
	pub initialized: bool,
	pub applied: Vec<MigrationRow>,
	pub files: Vec<MigrationFile>,
	pub pending: Vec<MigrationFile>,
	pub missed: Vec<String>,
	pub changed_files: Vec<String>,
	pub missing_files: Vec<String>,
	/// `.sql` files with a name that is not `YYYYMMDD-NNN-<name>.sql`
	pub ignored: Vec<String>
}

pub fn status (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, dir: &Path) -> Result<Status, String> {
	let (files, ignored) = list_files(dir)?;
	let applied = read_applied(session, dialect)?;

	let mut result = Status {
		initialized: applied.is_some(),
		applied: applied.unwrap_or_default(),
		files,
		ignored,
		..Status::default()
	};

	let by_name: BTreeMap<&str, &MigrationRow> = result.applied.iter().map(|item| (item.name.as_str(), item)).collect();
	let last = result.files
		.iter()
		.filter(|file| by_name.contains_key(file.name.as_str()))
		.map(|file| order_key(&file.name))
		.max();

	for file in &result.files {
		match by_name.get(file.name.as_str()) {
			Some(applied) if applied.checksum != file.checksum => {
				result.changed_files.push(file.name.clone());
			}
			Some(_) => {}
			None => {
				if last.is_some_and(|last| order_key(&file.name) < last) {
					result.missed.push(file.name.clone());
				}

				result.pending.push(file.clone());
			}
		}
	}

	for item in &result.applied {
		if !result.files.iter().any(|file| file.name == item.name) {
			result.missing_files.push(item.name.clone());
		}
	}

	return Ok(result);
}


#[derive(Debug, Clone)]
pub struct ApplyFailure {
	pub file: String,
	/// 1-based number of the statement in the file
	pub statement: usize,
	pub line: usize,
	pub sql: String,
	pub error: String,
	/// Statements of the file executed before the failed one
	pub executed: usize,
	pub rolled_back: bool
}

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
	/// Applied files and the number of their statements
	pub applied: Vec<(String, usize)>,
	pub missed: Vec<String>,
	pub failure: Option<ApplyFailure>,
	pub models_written: bool
}

pub fn apply (
	session: &mut dyn SqlSession,
	dialect: &dyn SqlDialect,
	dir: &Path,
	snapshot: Option<(&ModelSet, &str)>,
	defaults: &[DialectBlock]
) -> Result<ApplyReport, String> {
	dialect.lock(session)?;
	ensure_service_tables(session, dialect, defaults)?;

	let state = status(session, dialect, dir)?;
	if !state.changed_files.is_empty() {
		return Err(format!(
			"applied migrations were changed: {}\nfix forward: restore the files and write a new migration",
			state.changed_files.join(", ")
		));
	}

	let mut report = ApplyReport {
		missed: state.missed.clone(),
		..ApplyReport::default()
	};

	for file in &state.pending {
		let statements = split_statements(&file.text, dialect);
		if let Some(failure) = run_file(session, dialect, file, &statements)? {
			report.failure = Some(failure);
			return Ok(report);
		}

		report.applied.push((file.name.clone(), statements.len()));
	}

	if let Some((set, group)) = snapshot {
		write_models(session, &model_hashes(set, group))?;
		report.models_written = true;
	}

	return Ok(report);
}

fn run_file (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, file: &MigrationFile, statements: &[FileStatement]) -> Result<Option<ApplyFailure>, String> {
	let transactional = dialect.transactional_ddl();
	if transactional {
		session.exec("BEGIN")?;
	}

	for (index, statement) in statements.iter().enumerate() {
		if let Err(error) = session.exec(&statement.sql) {
			if transactional {
				session.exec("ROLLBACK")?;
			}

			return Ok(Some(ApplyFailure {
				file: file.name.clone(),
				statement: index + 1,
				line: statement.line,
				sql: statement.sql.clone(),
				error,
				executed: index,
				rolled_back: transactional
			}));
		}
	}

	record(session, &file.name, &file.checksum)?;
	if transactional {
		session.exec("COMMIT")?;
	}

	return Ok(None);
}

fn record (session: &mut dyn SqlSession, name: &str, checksum: &str) -> Result<(), String> {
	session.query(
		&format!("INSERT INTO {MIGRATIONS_TABLE} (name, checksum, applied_at) VALUES (?, ?, CURRENT_TIMESTAMP)"),
		&[SqlValue::Str(name.to_owned()), SqlValue::Str(checksum.to_owned())]
	)?;

	return Ok(());
}


pub fn model_hashes (set: &ModelSet, group: &str) -> Vec<(String, String)> {
	let document = to_runtime(set);
	let names: Vec<&String> = document.dbs
		.get(group)
		.map(|db| db.tables.keys().collect())
		.unwrap_or_default();

	return names
		.into_iter()
		.filter_map(|name| document.hash_model(group, name).map(|hash| (name.clone(), hash)))
		.collect();
}

pub fn write_models (session: &mut dyn SqlSession, hashes: &[(String, String)]) -> Result<(), String> {
	let rows = session.query(&format!("SELECT model, hash FROM {MODELS_TABLE}"), &[])?;
	let existing: BTreeMap<String, String> = rows
		.iter()
		.filter_map(|row| Some((row.first()?.text()?, row.get(1)?.text()?)))
		.collect();

	for (model, hash) in hashes {
		let params = [SqlValue::Str(hash.clone()), SqlValue::Str(model.clone())];

		match existing.get(model) {
			Some(current) if current == hash => {}
			Some(_) => {
				session.query(&format!("UPDATE {MODELS_TABLE} SET hash = ?, applied_at = CURRENT_TIMESTAMP WHERE model = ?"), &params)?;
			}
			None => {
				session.query(&format!("INSERT INTO {MODELS_TABLE} (hash, model, applied_at) VALUES (?, ?, CURRENT_TIMESTAMP)"), &params)?;
			}
		}
	}

	for model in existing.keys() {
		if !hashes.iter().any(|(name, _)| name == model) {
			session.query(
				&format!("DELETE FROM {MODELS_TABLE} WHERE model = ?"),
				&[SqlValue::Str(model.clone())]
			)?;
		}
	}

	return Ok(());
}

pub fn check_models (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, models: &[(String, String)]) -> Result<ModelsCheck, String> {
	if !dialect.table_exists(session, MODELS_TABLE)? {
		return Ok(ModelsCheck {
			unknown: models.iter().map(|(model, _)| model.clone()).collect(),
			no_table: true,
			..ModelsCheck::default()
		});
	}

	let rows = session.query(&format!("SELECT model, hash FROM {MODELS_TABLE}"), &[])?;
	let applied: BTreeMap<String, String> = rows
		.iter()
		.filter_map(|row| Some((row.first()?.text()?, row.get(1)?.text()?)))
		.collect();

	let mut result = ModelsCheck::default();
	for (model, hash) in models {
		match applied.get(model) {
			Some(current) if current == hash => {}
			Some(_) => result.mismatched.push(model.clone()),
			None => result.unknown.push(model.clone())
		}
	}

	return Ok(result);
}


pub fn drift_db (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, snapshot: &ModelSet, group: &str, defaults: &[DialectBlock]) -> Result<Drift, String> {
	let expected = to_db(snapshot, group, dialect, defaults).map_err(|error| format!("snapshot: {error}"))?;
	let actual = dialect.introspect_for(session, &expected)?;
	return Ok(drift(&actual, &expected));
}

pub fn baseline_ops (snapshot: &ModelSet, group: &str, dialect: &dyn SqlDialect, defaults: &[DialectBlock]) -> Result<Vec<Op>, String> {
	let to = to_db(snapshot, group, dialect, defaults).map_err(|error| format!("snapshot: {error}"))?;
	return Ok(diff(&DbSchema::default(), &to).ops);
}

pub fn mark_baseline (
	session: &mut dyn SqlSession,
	dialect: &dyn SqlDialect,
	file: &MigrationFile,
	snapshot: &ModelSet,
	group: &str,
	defaults: &[DialectBlock]
) -> Result<(), String> {
	dialect.lock(session)?;
	ensure_service_tables(session, dialect, defaults)?;

	let applied = read_applied(session, dialect)?.unwrap_or_default();
	if !applied.is_empty() {
		return Err(format!("{} migrations are already applied (the first is {}): baseline is done once, on a database without migrations", applied.len(), applied[0].name));
	}

	record(session, &file.name, &file.checksum)?;
	write_models(session, &model_hashes(snapshot, group))?;
	return Ok(());
}
