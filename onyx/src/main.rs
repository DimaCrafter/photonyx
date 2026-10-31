use std::{env, process};
use crate::log::Log;

mod config;
mod db;
mod generate;
mod log;
mod plan;


const USAGE: &str = "Usage:
  onyx models generate --lang zig [--project <path>] [--protocol <n>] [--zls-delay <ms>] [--watch-dir=<dir>...] <out-dir> <depfile>
  onyx models plan [--db <db>] [--name <name>] [--check] [--yes] [--project <path>] [--modules <dir>]
  onyx models snapshot --db <db> [--project <path>]
  onyx models import --db <db> [--force] [--enum-hints] [--models <dir>] [--migrations <dir>] [--project <path>] [--config <path>] [--modules <dir>]
  onyx models drift [--db <db>] [--project <path>] [--config <path>] [--modules <dir>]
  onyx migrate status [--db <db>] [--project <path>] [--config <path>] [--modules <dir>]
  onyx migrate apply [--db <db>] [--project <path>] [--config <path>] [--modules <dir>]
  onyx migrate baseline --db <db> [--force] [--project <path>] [--config <path>] [--modules <dir>]

  --project <path>  project file, default photonyx.json: databases with their driver, models
                    (db.<db>.models, default src/models/<db>), migrations (db.<db>.migrations,
                    default migrations/<db>) and dialect defaults (db.<db>.mysql/postgres);
                    relative paths are resolved from the directory of the file
  --config <path>   photonyx config, default config.json: connections (db.<db>), only for
                    commands that connect; databases are matched by name, driver must be the same
  --modules <dir>   drivers directory (their SQL dialects are used), default modules
  --zls-delay <ms>  pause between deleting and re-creating changed files (ZLS 0.17
                    ignores `changed` events), default 500; 0 disables the workaround.
                    Also ONYX_ZLS_DELAY=<ms>.
  --check           only compare the snapshot with the models, write nothing;
                    exit code 1 if a migration or a snapshot update is needed
  --yes             write migrations with dangerous changes without asking
  --models <dir>    with --db: models directory instead of db.<db>.models (relative to the
                    current directory)
  --migrations <dir>  with --db: migrations directory instead of db.<db>.migrations
  --force           import: overwrite existing model files and the snapshot;
                    baseline: mark applied even if the database differs from the snapshot
  --enum-hints      import: print values of short string columns (candidates for enums)

  generate and plan/snapshot read only the project file. Commands that connect to the database
  (import, drift, migrate) take the connection from db.<db> of the config and the driver's SQL
  dialect from --modules.";

pub fn usage_by_prefix (command: &str) -> String {
	let prefix = format!("onyx {command}");
	let line = USAGE
    .lines()
    .map(str::trim)
    .find(|line| *line == prefix || line.starts_with(&format!("{prefix} ")))
    .unwrap_or(&prefix);

	return format!("Usage: {line}\nAll commands and options: onyx help");
}

fn commands () -> String {
	let lines: Vec<&str> = USAGE.lines()
    .map(str::trim)
    .filter(|line| line.starts_with("onyx "))
    .collect();

	return format!("Available commands:\n  {}", lines.join("\n  "));
}

fn main () {
	let args: Vec<String> = env::args().skip(1).collect();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();

	let code = match args.as_slice() {
		["models", "generate", rest @ ..] => generate::run(rest),
		["models", "plan", rest @ ..] => plan::run_plan(rest),
		["models", "snapshot", rest @ ..] => plan::run_snapshot(rest),
		["models", "import", rest @ ..] => db::run_import(rest),
		["models", "drift", rest @ ..] => db::run_drift(rest),
		["migrate", "status", rest @ ..] => db::run_status(rest),
		["migrate", "apply", rest @ ..] => db::run_apply(rest),
		["migrate", "baseline", rest @ ..] => db::run_baseline(rest),
		["help" | "--help" | "-h"] => {
			println!("{USAGE}");
			0
		}
		_ => {
			Log::error("Unknown command");
      println!("\n{}\n\nDetailed usage: onyx help", commands());
			2
		}
	};

	process::exit(code);
}
