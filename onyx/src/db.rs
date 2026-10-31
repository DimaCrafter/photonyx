use photonyx_models::{emit, load_groups, ir::ModelSet};
use photonyx_sql_shared::{compare::{self, Drift}, dialect::{SqlSession, SqlValue}, import, migrate::{self, Status}, migration::{self, SNAPSHOT_FILE}, schema::{ColumnType, DbSchema}, to_db, SqlDialect};
use crate::{config::{DbConfig, Dialects}, log::Log, plan::{parse_args, select_dbs, Options}};


fn each_db (command: &str, args: &[&str], allowed: &[&str], need_db: bool, run: impl Fn(&Options, &DbConfig, &dyn SqlDialect, &str) -> Result<bool, String>) -> i32 {
	let options = match parse_args(args, allowed) {
		Ok(options) if need_db && options.db.is_none() => {
			Log::error_full(&format!("{command}: --db is required"), &crate::usage_by_prefix(command));
			return 2;
		}
		Ok(options) => options,
		Err(error) => {
			Log::error_full(&format!("{command}: {error}"), &crate::usage_by_prefix(command));
			return 2;
		}
	};

	let dbs = match select_dbs(&options) {
		Ok(dbs) => dbs,
		Err(error) => {
			Log::error_full(&format!("{command} failed"), &error);
			return 1;
		}
	};

	let dialects = Dialects::load(&options.modules);
	let width = dbs.iter().map(|db| db.name.chars().count() + 2).max().unwrap_or(0).max(10);

	let mut code = 0;
	for db in &dbs {
		let head = format!("{:<width$}", db.name);
		if db.raw.is_null() {
			Log::line(&format!("{head}{}", Log::warning(&format!("⚠ no db.{} in {}, skipped", db.name, options.config.display()))));
			if options.db.is_some() {
				code = 1;
			}

			continue;
		}

		let dialect = match dialects.find(&db.driver) {
			Some(dialect) => dialect,
			None => {
				Log::line(&format!("{head}{}", Log::warning(&format!("⚠ driver \"{}\" has no schema dialect, skipped", db.driver))));
				for error in &dialects.errors {
					Log::line(&format!("{}{}", " ".repeat(width + 2), Log::dimmed(error)));
				}

				if options.db.is_some() {
					code = 1;
				}

				continue;
			}
		};

		match run(&options, db, dialect, &head) {
			Ok(true) => {}
			Ok(false) => code = 1,
			Err(error) => {
				Log::error_full(&format!("{command}: {}", db.name), &error);
				code = 1;
			}
		}
	}

	return code;
}

fn connect (db: &DbConfig, dialect: &dyn SqlDialect) -> Result<Box<dyn SqlSession>, String> {
	return dialect.connect(&db.raw).map_err(|error| format!("connect to `{}`: {error}", db.name));
}

fn read_snapshot (db: &DbConfig) -> Result<Option<ModelSet>, String> {
	return Ok(migration::read_snapshot(&db.migrations)?.map(|(set, _)| set));
}

fn indent (head: &str) -> String {
	return " ".repeat(head.chars().count() + 2);
}

fn print_drift (drift: &Drift, head: &str) {
	let pad = indent(head);
	let lines = compare::describe(drift);
	let width = lines.iter().map(|(table, _)| table.chars().count()).chain(drift.extra.iter().map(|table| table.chars().count())).max().unwrap_or(0) + 2;

	for (table, items) in &lines {
		for (i, item) in items.iter().enumerate() {
			let name = if i == 0 { table.as_str() } else { "" };
			Log::line(&format!("{pad}{name:<width$}{}", Log::warning(item)));
		}
	}

	for table in &drift.extra {
		Log::line(&format!("{pad}{}", Log::dimmed(&format!("{table:<width$}not in the models, left as is"))));
	}
}


pub fn run_import (args: &[&str]) -> i32 {
	// TODO: beautify commands code
	return each_db("models import", args, &["--project", "--config", "--modules", "--db", "--models", "--migrations", "--force", "--enum-hints"], true, |options, db, dialect, head| {
		let mut session = connect(db, dialect)?;
		let schema = dialect.introspect(session.as_mut())?;
		let draft = import::import(&schema, &db.name, &db.models, dialect, &db.dialect_defaults)?;

		let snapshot_path = db.migrations.join(SNAPSHOT_FILE);
		let date = chrono::Local::now().format("%Y-%m-%d");
		let header = [format!("imported from the database by `onyx models import` on {date}: review names, types and enums")];

		let mut files = Vec::<(std::path::PathBuf, String)>::new();
		for entity in &draft.group.entities {
			files.push((db.models.join(format!("{}.yml", entity.name)), emit::entity_to_yml(entity, &header)));
		}

		let existing: Vec<String> = files
			.iter()
			.map(|(path, _)| path)
			.chain([&snapshot_path])
			.filter(|path| path.exists())
			.map(|path| path.display().to_string())
			.collect();

		if !existing.is_empty() && !options.force {
			return Err(format!("files exist, nothing written (--force overwrites them):\n{}", existing.join("\n")));
		}

		migration::write_files(&files)?;

		let loaded = load_groups(&[(db.name.clone(), db.models.clone())]);
		if !loaded.errors.is_empty() {
			let list: Vec<String> = loaded.errors.iter().map(|error| error.to_string()).collect();
			return Err(format!("written models do not load:\n{}", list.join("\n")));
		}

		let (text, _) = migration::snapshot_text(&loaded.set, &db.name).ok_or("models group not loaded")?;
		migration::write_files(&[(snapshot_path.clone(), text)])?;

		let tables = draft.group.entities.iter().filter(|entity| matches!(entity.kind, photonyx_models::ir::EntityKind::Table(_))).count();
		Log::line(&format!("{head}{} tables --> {}, snapshot {}", tables, db.models.display(), snapshot_path.display()));

		let pad = indent(head);
		for note in &draft.notes {
			Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ {note}"))));
		}

		let expected = to_db(&loaded.set, &db.name, dialect, &db.dialect_defaults)?;
		let drift = compare::drift(&schema, &expected);
		if drift.is_empty() {
			Log::line(&format!("{pad}round trip: models match the database"));
		} else {
			Log::line(&format!("{pad}{}", Log::warning("⚠ round trip: the imported models differ from the database")));
			print_drift(&drift, head);
		}

		if options.enum_hints {
			print_enum_hints(session.as_mut(), dialect, &schema, &pad)?;
		}

		return Ok(drift.is_empty());
	});
}

fn print_enum_hints (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, schema: &DbSchema, pad: &str) -> Result<(), String> {
	const LIMIT: usize = 20;
	for table in schema.tables.values() {
		for column in &table.columns {
			let short = matches!(column.db_type, ColumnType::Varchar(size) | ColumnType::Char(size) if size <= 64);
			let keyed = table.primary.contains(&column.name) || table.indexes.iter().any(|index| index.unique && index.columns.contains(&column.name));
			if !short || keyed {
				continue;
			}

			let ident = |name: &str| name.split('.').map(|part| dialect.quote(part)).collect::<Vec<_>>().join(".");
			let name = dialect.quote(&column.name);
			let rows = session.query(&format!("SELECT DISTINCT {name} FROM {} WHERE {name} IS NOT NULL ORDER BY 1 LIMIT {}", ident(&table.name), LIMIT + 1), &[])?;
			if rows.is_empty() || rows.len() > LIMIT {
				continue;
			}

			let values: Vec<String> = rows.iter()
				.filter_map(|row| row.first().and_then(SqlValue::text))
				.map(|value| format!("'{value}'"))
				.collect();

			Log::line(&format!("{pad}{}", Log::dimmed(&format!("{}.{}: {}", table.name, column.name, values.join(", ")))));
		}
	}

	return Ok(());
}


pub fn run_drift (args: &[&str]) -> i32 {
	return each_db("models drift", args, &["--project", "--config", "--modules", "--db", "--models", "--migrations"], false, |_, db, dialect, head| {
		let snapshot = match read_snapshot(db)? {
			Some(snapshot) => snapshot,
			None => {
				Log::line(&format!("{head}{}", Log::dimmed(&format!("no snapshot at {}, nothing to compare", db.migrations.display()))));
				return Ok(true);
			}
		};

		let mut session = connect(db, dialect)?;
		let state = migrate::status(session.as_mut(), dialect, &db.migrations)?;
		let drift = migrate::drift_db(session.as_mut(), dialect, &snapshot, &db.name, &db.dialect_defaults)?;

		if !state.pending.is_empty() {
			Log::line(&format!("{head}{}", Log::warning(&format!("⚠ {} migrations are not applied, a drift is expected (onyx migrate status)", state.pending.len()))));
		}

		if drift.is_empty() {
			Log::line(&format!("{head}no drift: the database matches {SNAPSHOT_FILE}"));
		} else {
			Log::line(&format!("{head}{}", Log::warning(&format!("⚠ drift in {} tables", compare::describe(&drift).len()))));
		}

		print_drift(&drift, head);
		return Ok(drift.is_empty());
	});
}


fn print_status (state: &Status, models_outdated: usize, head: &str) -> bool {
	let pad = indent(head);
	let last = state.applied.last();

	let summary = match last {
		Some(last) => format!("{} applied, the last is {} ({})", state.applied.len(), last.name, last.applied_at),
		None if state.initialized => "no migrations applied".to_owned(),
		None => format!("no migrations applied ({} does not exist)", migrate::MIGRATIONS_TABLE)
	};

	let clean = state.pending.is_empty() && state.changed_files.is_empty() && models_outdated == 0;
	Log::line(&format!("{head}{summary}{}", if clean { ", up to date" } else { "" }));

	for file in &state.pending {
		let missed = if state.missed.contains(&file.name) { Log::warning("   ⚠ older than the last applied (a branch merged later)") } else { String::new() };
		Log::line(&format!("{pad}pending  {}{missed}", file.name));
	}

	for name in &state.changed_files {
		Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ changed  {name}: the file differs from the applied one (checksum)"))));
	}

	for name in &state.missing_files {
		Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ missing  {name}: applied, but there is no file"))));
	}

	if models_outdated > 0 {
		Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ {models_outdated} models in {} differ from {SNAPSHOT_FILE}, `onyx migrate apply` updates them", migrate::MODELS_TABLE))));
	}

	for name in &state.ignored {
		Log::line(&format!("{pad}{}", Log::dimmed(&format!("ignored  {name}: not a YYYYMMDD-NNN-<name>.sql name"))));
	}

	return clean;
}

fn models_outdated (session: &mut dyn SqlSession, dialect: &dyn SqlDialect, snapshot: Option<&ModelSet>, group: &str) -> Result<usize, String> {
	let snapshot = match snapshot {
		Some(snapshot) => snapshot,
		None => return Ok(0)
	};

	let hashes = migrate::model_hashes(snapshot, group);
	let check = migrate::check_models(session, dialect, &hashes)?;
	let mut count = check.mismatched.len() + check.unknown.len();
	if !check.no_table {
		let rows = session.query(&format!("SELECT model FROM {}", migrate::MODELS_TABLE), &[])?;
		count += rows.iter().filter_map(|row| row.first().and_then(SqlValue::text)).filter(|model| !hashes.iter().any(|(name, _)| name == model)).count();
	}

	return Ok(count);
}

pub fn run_status (args: &[&str]) -> i32 {
	return each_db("migrate status", args, &["--project", "--config", "--modules", "--db", "--models", "--migrations"], false, |_, db, dialect, head| {
		let mut session = connect(db, dialect)?;
		let state = migrate::status(session.as_mut(), dialect, &db.migrations)?;
		let snapshot = read_snapshot(db)?;
		let outdated = if state.initialized { models_outdated(session.as_mut(), dialect, snapshot.as_ref(), &db.name)? } else { 0 };
		return Ok(print_status(&state, outdated, head));
	});
}


pub fn run_apply (args: &[&str]) -> i32 {
	return each_db("migrate apply", args, &["--project", "--config", "--modules", "--db", "--models", "--migrations"], false, |_, db, dialect, head| {
		let pad = indent(head);
		let snapshot = read_snapshot(db)?;
		let mut session = connect(db, dialect)?;

		let report = migrate::apply(session.as_mut(), dialect, &db.migrations, snapshot.as_ref().map(|set| (set, db.name.as_str())), &db.dialect_defaults)?;
		match (report.applied.len(), &report.failure) {
			(0, None) => Log::line(&format!("{head}nothing to apply")),
			(count, None) => Log::line(&format!("{head}applied {count} migrations")),
			(count, Some(failure)) => Log::line(&format!("{head}{}", Log::warning(&format!("applied {count} migrations, {} failed", failure.file))))
		}

		for (name, statements) in &report.applied {
			let missed = if report.missed.contains(name) { Log::warning("   ⚠ older than the last applied (a branch merged later)") } else { String::new() };
			Log::line(&format!("{pad}{name}  {}{missed}", Log::dimmed(&format!("{statements} statements"))));
		}

		if let Some(failure) = &report.failure {
			let state = if failure.rolled_back {
				"the file was rolled back".to_owned()
			} else if failure.executed > 0 {
				format!("{} statements before it are executed and stay (DDL is not transactional), the file is not marked applied: make it start from the failed statement (or undo them by hand) and run apply again", failure.executed)
			} else {
				"nothing of the file is executed, it is not marked applied".to_owned()
			};

			return Err(format!(
				"{}:{} statement {}: {}\n{}\n\n{state}",
				db.migrations.join(&failure.file).display(), failure.line, failure.statement, failure.error, failure.sql
			));
		}

		let snapshot = match snapshot {
			Some(snapshot) => snapshot,
			None => {
				Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ no {SNAPSHOT_FILE}: {} not written, no drift check", migrate::MODELS_TABLE))));
				return Ok(true);
			}
		};

		let drift = migrate::drift_db(session.as_mut(), dialect, &snapshot, &db.name, &db.dialect_defaults)?;
		if drift.is_empty() {
			Log::line(&format!("{pad}drift check: the database matches {SNAPSHOT_FILE}"));
			print_drift(&drift, head);
			return Ok(true);
		}

		Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ drift check: the database does not match {SNAPSHOT_FILE}; fix forward with a new migration"))));
		print_drift(&drift, head);
		return Ok(false);
	});
}


pub fn run_baseline (args: &[&str]) -> i32 {
	return each_db("migrate baseline", args, &["--project", "--config", "--modules", "--db", "--models", "--migrations", "--force"], true, |options, db, dialect, head| {
		let pad = indent(head);
		let snapshot = read_snapshot(db)?.ok_or_else(|| format!("no {SNAPSHOT_FILE} in {}: run `onyx models import` or `onyx models snapshot` first", db.migrations.display()))?;
		let mut session = connect(db, dialect)?;

		let applied = migrate::read_applied(session.as_mut(), dialect)?.unwrap_or_default();
		if let Some(first) = applied.first() {
			return Err(format!("{} migrations are already applied (the first is {}): baseline is done once, on a database without migrations", applied.len(), first.name));
		}

		let (files, _) = migrate::list_files(&db.migrations)?;
		let others: Vec<&str> = files.iter().map(|file| file.name.as_str()).filter(|name| *name != migrate::BASELINE_FILE).collect();
		if !others.is_empty() {
			return Err(format!("{} already has migrations ({}): baseline goes before all of them", db.migrations.display(), others.join(", ")));
		}

		let path = db.migrations.join(migrate::BASELINE_FILE);
		if !path.exists() {
			let ops = migrate::baseline_ops(&snapshot, &db.name, dialect, &db.dialect_defaults)?;
			let statements = dialect.render(&ops);
			let changes = migration::summarize(&ops);
			let date = chrono::Local::now().format("%Y-%m-%d");

			let mut header = vec![
				format!("Baseline of `{}`: the whole schema of {SNAPSHOT_FILE}, `onyx migrate baseline` on {date}.", db.name),
				"A database that already has this schema gets it marked applied without running;".to_owned(),
				"a new environment (an empty database) gets the schema from it by `onyx migrate apply`.".to_owned(),
				String::new()
			];
			let width = changes.iter().map(|change| change.table.chars().count()).max().unwrap_or(0) + 2;
			header.extend(changes.iter().map(|change| format!("{:<width$}{}", change.table, change.describe())));

			let text = migration::compose(&header, &statements, ops.len());
			migration::write_files(&[(path.clone(), text.text)])?;
			Log::line(&format!("{head}{} written", path.display()));
		} else {
			Log::line(&format!("{head}{} exists, using it", path.display()));
		}

		let schema = dialect.introspect(session.as_mut())?;
		if schema.tables.is_empty() {
			Log::line(&format!("{pad}the database is empty: run `onyx migrate apply` to create the schema from the baseline"));
			return Ok(true);
		}

		let drift = migrate::drift_db(session.as_mut(), dialect, &snapshot, &db.name, &db.dialect_defaults)?;
		if !drift.is_empty() {
			Log::line(&format!("{pad}{}", Log::warning(&format!("⚠ the database differs from {SNAPSHOT_FILE}:"))));
			print_drift(&drift, head);
			if !options.force {
				return Err("baseline must describe the database as it is: fix the models (or re-import them) and run `onyx models snapshot`, or pass --force to mark it anyway".to_owned());
			}
		}

		let (files, _) = migrate::list_files(&db.migrations)?;
		let file = files.iter().find(|file| file.name == migrate::BASELINE_FILE).ok_or("baseline file disappeared")?;
		migrate::mark_baseline(session.as_mut(), dialect, file, &snapshot, &db.name, &db.dialect_defaults)?;
		Log::line(&format!("{pad}marked applied without running; {} written", migrate::MODELS_TABLE));
		return Ok(true);
	});
}
