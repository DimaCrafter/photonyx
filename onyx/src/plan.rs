use std::{io::{stdin, stdout, BufRead, IsTerminal, Write}, path::PathBuf};
use photonyx_models::{Loaded, ir::{Group, ModelSet}, load_groups, snapshot};
use photonyx_sql_shared::{danger::Danger, migration::{self, TableChange, SNAPSHOT_FILE}, plan, Plan};
use crate::{config::{self, DbConfig, Dialects}, log::Log};


pub struct Options {
	/// `--project`: models and migrations of the databases
	pub project: PathBuf,
	/// `--config`: connections, only for commands that connect
	pub config: PathBuf,
	pub modules: PathBuf,
	pub db: Option<String>,
	pub name: Option<String>,
	pub check: bool,
	pub yes: bool,
	/// `--models`/`--migrations`: directories instead of the ones from the project file (with `--db`)
	pub models: Option<PathBuf>,
	pub migrations: Option<PathBuf>,
	pub force: bool,
	pub enum_hints: bool
}

pub fn parse_args (args: &[&str], allowed: &[&str]) -> Result<Options, String> {
	let mut options = Options {
		project: PathBuf::from("photonyx.json"),
		config: PathBuf::from("config.json"),
		modules: PathBuf::from("modules"),
		db: None,
		name: None,
		check: false,
		yes: false,
		models: None,
		migrations: None,
		force: false,
		enum_hints: false
	};

	let mut iter = args.iter();
	while let Some(arg) = iter.next() {
		if !allowed.contains(arg) {
			return Err(format!("unknown argument {arg}"));
		}

		let mut value = || iter.next().map(|value| value.to_string()).ok_or(format!("{arg}: value expected"));
		match *arg {
			"--project" => options.project = PathBuf::from(value()?),
			"--config" => options.config = PathBuf::from(value()?),
			"--modules" => options.modules = PathBuf::from(value()?),
			"--db" => options.db = Some(value()?),
			"--name" => options.name = Some(value()?),
			"--check" => options.check = true,
			"--yes" => options.yes = true,
			"--models" => options.models = Some(PathBuf::from(value()?)),
			"--migrations" => options.migrations = Some(PathBuf::from(value()?)),
			"--force" => options.force = true,
			"--enum-hints" => options.enum_hints = true,
			_ => unreachable!()
		}
	}

	if (options.models.is_some() || options.migrations.is_some()) && options.db.is_none() {
		return Err("--models and --migrations need --db".to_owned());
	}

	return Ok(options);
}

/// Databases of the project with `--models`/`--migrations` applied to the one from `--db`
fn project_dbs (options: &Options) -> Result<Vec<DbConfig>, String> {
	let mut dbs = config::load_project(&options.project)?;
	if let Some(name) = &options.db {
		let db = dbs.iter_mut().find(|db| &db.name == name).ok_or_else(|| format!("{}: database `{name}` is not in the project", options.project.display()))?;
		if let Some(models) = &options.models {
			db.models = models.clone();
			db.models_explicit = true;
		}

		if let Some(migrations) = &options.migrations {
			db.migrations = migrations.clone();
		}
	}

	return Ok(dbs);
}

/// Project databases (or the one from `--db`) with their connections from `--config`
pub fn select_dbs (options: &Options) -> Result<Vec<DbConfig>, String> {
	let mut dbs = project_dbs(options)?;
	dbs.retain(|db| options.db.as_ref().is_none_or(|name| &db.name == name));
	config::attach_connections(&mut dbs, &options.config)?;
	return Ok(dbs);
}

/// Project databases (or the one from `--db`) and models of every project database that has
/// a models directory: references to other groups (`shared.Currency`) resolve between them
fn load (options: &Options) -> Result<(Vec<DbConfig>, Loaded), String> {
	let dbs = project_dbs(options)?;
	if let Some(name) = &options.db {
		if let Some(db) = dbs.iter().find(|db| &db.name == name && !db.models.is_dir()) {
			return Err(format!("db.{name}: models directory {} not found", db.models.display()));
		}
	}

	let loaded = load_groups(&config::model_groups(&dbs)?);
	if !loaded.errors.is_empty() {
		let list: Vec<String> = loaded.errors.iter().map(|error| error.to_string()).collect();
		return Err(list.join("\n"));
	}

	let selected = dbs.into_iter().filter(|db| options.db.as_ref().is_none_or(|name| &db.name == name)).collect();
	return Ok((selected, loaded));
}


#[derive(PartialEq)]
enum Outcome {
	Done,
	Outdated,
	Declined
}

pub fn run_plan (args: &[&str]) -> i32 {
	let options = match parse_args(args, &["--project", "--modules", "--db", "--name", "--check", "--yes", "--models", "--migrations"]) {
		Ok(options) => options,
		Err(error) => {
			Log::error_full(&format!("models plan: {error}"), &crate::usage_by_prefix("models plan"));
			return 2;
		}
	};

	let (dbs, loaded) = match load(&options) {
		Ok(result) => result,
		Err(error) => {
			Log::error_full("models plan failed", &error);
			return 1;
		}
	};

	let dialects = Dialects::load(&options.modules);
	let width = dbs.iter().map(|db| db.name.chars().count() + 2).max().unwrap_or(0).max(10);

	let mut code = 0;
	for db in &dbs {
		match plan_db(&options, db, &loaded, &dialects, width) {
			Ok(Outcome::Done) => {}
			Ok(Outcome::Outdated | Outcome::Declined) => code = 1,
			Err(error) => {
				Log::error_full(&format!("models plan: {}", db.name), &error);
				code = 1;
			}
		}
	}

	return code;
}

fn plan_db (options: &Options, db: &DbConfig, loaded: &Loaded, dialects: &Dialects, width: usize) -> Result<Outcome, String> {
	let head = format!("{:<width$}", db.name);
	let indent = " ".repeat(width + 2);

	if !db.models.is_dir() {
		Log::line(&format!("{head}{}", Log::dimmed(&format!("no models at {}, skipped", db.models.display()))));
		return Ok(Outcome::Done);
	}

	let dialect = match dialects.find(&db.driver) {
		Some(dialect) => dialect,
		None => {
			Log::line(&format!("{head}{}", Log::warning(&format!("⚠ driver \"{}\" has no schema dialect, skipped", db.driver))));
			for error in &dialects.errors {
				Log::line(&format!("{indent}{}", Log::dimmed(error)));
			}

			return Ok(Outcome::Done);
		}
	};

	let snapshot_path = db.migrations.join(SNAPSHOT_FILE);
	let previous = migration::read_snapshot(&db.migrations)?;
	let (snapshot_text, snapshot_root) = migration::snapshot_text(&loaded.set, &db.name).ok_or("models group not loaded")?;
	let snapshot_changed = match &previous {
		Some((_, root)) => !snapshot::is_same(root, &snapshot_root),
		None => loaded.set.find_group(&db.name).is_some_and(|group| !group.entities.is_empty())
	};

	let empty = ModelSet { groups: vec![Group::new_detached(db.name.clone())] };
	let from = previous.as_ref().map(|(set, _)| set).unwrap_or(&empty);
	let result = plan(from, &loaded.set, &db.name, dialect, &db.dialect_defaults)?;

	if result.ops.is_empty() {
		if !snapshot_changed {
			Log::line(&format!("{head}up to date"));
			print_untracked(&result, &indent);
			return Ok(Outcome::Done);
		}

		if options.check {
			Log::line(&format!("{head}{}", Log::warning("snapshot is outdated (no schema changes), run `onyx models plan`")));
			print_untracked(&result, &indent);
			return Ok(Outcome::Outdated);
		}

		migration::write_files(&[(snapshot_path.clone(), snapshot_text)])?;
		Log::line(&format!("{head}{} updated (no schema changes)", snapshot_path.display()));
		print_untracked(&result, &indent);
		return Ok(Outcome::Done);
	}

	let statements = dialect.render(&result.ops);
	let name = options.name.as_deref().map(migration::slug).filter(|name| !name.is_empty()).unwrap_or_else(|| migration::derive_name(&result.changes));
	let date = chrono::Local::now().format("%Y%m%d").to_string();
	let file_name = migration::next_file_name(&db.migrations, &date, &name)?;
	let path = db.migrations.join(&file_name);

	let header = header(db, &result);
	let text = migration::compose(&header, &statements, result.ops.len());
	let line_of = |danger: &Danger| text.op_lines.get(danger.op_idx).copied().flatten();

	let suffix = if options.check { Log::warning("  (migration needed, not written)") } else { String::new() };
	Log::line(&format!("{head}{}{suffix}", path.display()));
	print_changes(&result.changes, &result.dangers, &indent, line_of);
	print_untracked(&result, &indent);

	if options.check {
		return Ok(Outcome::Outdated);
	}

	if !result.dangers.is_empty() && !options.yes && !confirm(&indent)? {
		return Ok(Outcome::Declined);
	}

	migration::write_files(&[(path, text.text), (snapshot_path, snapshot_text)])?;
	return Ok(Outcome::Done);
}

fn header (db: &DbConfig, result: &Plan) -> Vec<String> {
	let date = chrono::Local::now().format("%Y-%m-%d");
	let mut lines = vec![
		format!("Migration of `{}`, generated by `onyx models plan` on {date}", db.name),
		format!("from {SNAPSHOT_FILE} and the current models. Review before applying: a renamed column"),
		"is planned as DROP + ADD and loses data unless replaced with RENAME COLUMN.".to_owned(),
		String::new()
	];

	let width = result.changes.iter().map(|change| change.table.chars().count()).max().unwrap_or(0) + 2;
	for change in &result.changes {
		lines.push(format!("{:<width$}{}", change.table, change.describe()));
	}

	for danger in &result.dangers {
		lines.push(format!("⚠ {}", danger.describe()));
	}

	return lines;
}

fn print_changes (changes: &[TableChange], dangers: &[Danger], indent: &str, line_of: impl Fn(&Danger) -> Option<usize>) {
	let width = changes.iter().map(|change| change.table.chars().count()).max().unwrap_or(0) + 2;

	for change in changes {
		let mut line = format!("{indent}{:<width$}{}", change.table, change.describe());
		let warnings: Vec<String> = dangers.iter().filter(|danger| change.op_idx.contains(&danger.op_idx)).map(|danger| {
			let at = line_of(danger).map(|line| format!(" (line {line})")).unwrap_or_default();
			let what = if danger.details.is_empty() { danger.kind.to_string() } else { format!("{} {}", danger.kind, danger.details) };
			Log::warning(&format!("⚠ {what}{at}"))
		}).collect();

		let column = line.chars().count() + 3;
		for (i, warning) in warnings.iter().enumerate() {
			if i == 0 {
				line.push_str("   ");
				line.push_str(warning);
			} else {
				line.push('\n');
				line.push_str(&" ".repeat(column));
				line.push_str(warning);
			}
		}

		Log::line(&line);
	}
}

fn print_untracked (result: &Plan, indent: &str) {
	for table in &result.untracked {
		Log::line(&format!("{indent}{}", Log::dimmed(&format!("{table}  not in models, left as is (a model with `deleted: true` drops it)"))));
	}
}

fn confirm (indent: &str) -> Result<bool, String> {
	if !stdin().is_terminal() || !stdout().is_terminal() {
		Log::line(&format!("{indent}{}", Log::warning("dangerous changes: not written, review the summary and run again with --yes")));
		return Ok(false);
	}

	print!("{indent}dangerous changes, write the migration? [y/N] ");
	let _ = stdout().flush();

	let mut answer = String::new();
	stdin().lock().read_line(&mut answer).map_err(|error| error.to_string())?;
	let accepted = matches!(answer.trim(), "y" | "Y" | "yes");
	if !accepted {
		Log::line(&format!("{indent}not written"));
	}

	return Ok(accepted);
}


pub fn run_snapshot (args: &[&str]) -> i32 {
	let options = match parse_args(args, &["--project", "--db", "--models", "--migrations"]) {
		Ok(options) if options.db.is_some() => options,
		Ok(_) => {
			Log::error_full("models snapshot: --db is required", &crate::usage_by_prefix("models snapshot"));
			return 2;
		}
		Err(error) => {
			Log::error_full(&format!("models snapshot: {error}"), &crate::usage_by_prefix("models snapshot"));
			return 2;
		}
	};

	let result = load(&options).and_then(|(dbs, loaded)| {
		let db = dbs.first().ok_or("no database")?;
		let (text, _) = migration::snapshot_text(&loaded.set, &db.name).ok_or("models group not loaded")?;
		let path = db.migrations.join(SNAPSHOT_FILE);
		migration::write_files(&[(path.clone(), text)])?;
		Log::line(&format!("{}  {} written", db.name, path.display()));
		return Ok(());
	});

	if let Err(error) = result {
		Log::error_full("models snapshot failed", &error);
		return 1;
	}

	return 0;
}
