use std::collections::BTreeMap;
use crate::{app::{config::CONFIG, modules::Module}, db::connection::DB_CONNECTIONS, utils::log::{log_error, log_info, log_warning}};


#[derive(Clone, Copy, PartialEq)]
enum Mode {
	Warn,
	Strict
}

/// Check model hashes from loaded modules against connected database.
/// Returns `false` on mismatch when `migrations_check` is `strict`.
pub fn check_models (modules: &[Module]) -> bool {
	// DB key --> (Model name, Hash) --> Modules
	let mut dbs: BTreeMap<String, BTreeMap<(String, String), Vec<String>>> = BTreeMap::new();
	for module in modules {
		let document = match module.get_models() {
			Some(document) => document,
			None => continue
		};

		for (db, models) in &document.dbs {
			for model in models.tables.keys() {
				if let Some(hash) = document.hash_model(db, model) {
					dbs.entry(db.clone()).or_default().entry((model.clone(), hash)).or_default().push(module.get_name().to_owned());
				}
			}
		}
	}

	let mut ok = true;
	for (db, models) in &dbs {
		let mode = match CONFIG["db"][db.as_str()]["migrations_check"].as_str() {
			None | Some("warn") => Mode::Warn,
			Some("strict") => Mode::Strict,
			Some(other) => {
				log_error(&format!("db.{db}.migrations_check: expected \"warn\" or \"strict\", got \"{other}\""));
				ok = false;
				continue;
			}
		};

		let connection = match DB_CONNECTIONS.get().and_then(|connections| connections.find(db)) {
			Some(connection) => connection,
			None => {
				report(mode, &format!("db {db}: no connection (not configured or failed to connect), models are not checked"));
				ok &= mode == Mode::Warn;
				continue;
			}
		};

		let pairs: Vec<(String, String)> = models.keys().cloned().collect();
		let check = match connection.check_models(&pairs) {
			Ok(Some(check)) => check,
			Ok(None) => {
				log_warning(&format!("db {db}: the driver does not check models against the database, skipped"));
				continue;
			}
			Err(error) => {
				report(mode, &format!("db {db}: models check failed: {error}"));
				ok &= mode == Mode::Warn;
				continue;
			}
		};

		if check.is_ok() {
			log_info(&format!("db {db}: models of modules match the applied ones ({} models)", pairs.len()));
			continue;
		}

		let mut lines = Vec::new();
		if check.no_table {
			lines.push("photonyx_models does not exist: migrations were never applied (onyx migrate apply / onyx migrate baseline)".to_owned());
		}

		for ((model, hash), names) in models {
			let state = if check.mismatched.contains(model) && !check.unknown.contains(model) {
				// A model can be in several modules with different hashes
				if is_applied(connection.as_ref(), model, hash) { continue; }
				"differs from the one applied to the database"
			} else if check.unknown.contains(model) && !check.no_table {
				"is not applied to the database"
			} else {
				continue;
			};

			lines.push(format!("model {db}.{model} of {} {state}", names.join(", ")));
		}

		let message = format!(
			"db {db}: models of modules do not match the applied ones ({}); the modules are built with other models than the migrations applied to the database",
			if mode == Mode::Strict { "migrations_check = strict" } else { "migrations_check = warn" }
		);
		report(mode, &format!("{message}\n  {}", lines.join("\n  ")));
		ok &= mode == Mode::Warn;
	}

	return ok;
}

fn is_applied (connection: &dyn bindings::db::DatabaseConnection, model: &str, hash: &str) -> bool {
	return connection
		.check_models(&[(model.to_owned(), hash.to_owned())])
		.ok()
		.flatten()
		.is_some_and(|check| check.is_ok());
}

fn report (mode: Mode, message: &str) {
	match mode {
		Mode::Warn => log_warning(message),
		Mode::Strict => log_error(message)
	}
}
