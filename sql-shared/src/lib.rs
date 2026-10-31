use photonyx_models::ir::{DialectBlock, ModelSet};

pub mod compare;
pub mod danger;
pub mod dialect;
pub mod diff;
pub mod import;
pub mod migrate;
pub mod migration;
pub mod schema;
pub mod to_db;

pub use dialect::{SqlDialect, Statement};
pub use diff::{diff, Op};
pub use schema::*;
pub use to_db::to_db;


#[derive(Debug, Clone)]
pub struct Plan {
	pub ops: Vec<Op>,
	/// Tables of the snapshot that are gone from the models without a tombstone
	pub untracked: Vec<String>,
	pub dangers: Vec<danger::Danger>,
	pub changes: Vec<migration::TableChange>
}

pub fn plan (snapshot: &ModelSet, models: &ModelSet, group: &str, dialect: &dyn SqlDialect, defaults: &[DialectBlock]) -> Result<Plan, String> {
	let from = to_db(snapshot, group, dialect, defaults).map_err(|error| format!("snapshot: {error}"))?;
	let to = to_db(models, group, dialect, defaults)?;

	let diff::Diff { ops, untracked } = diff(&from, &to);
	let dangers = danger::analyze(&ops);
	let changes = migration::summarize(&ops);
	return Ok(Plan { ops, untracked, dangers, changes });
}
