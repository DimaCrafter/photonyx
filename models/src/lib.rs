pub mod runtime;

#[cfg(feature = "ir")]
pub mod check;
#[cfg(feature = "ir")]
pub mod emit;
#[cfg(feature = "ir")]
pub mod error;
#[cfg(feature = "ir")]
pub mod ir;
#[cfg(feature = "parse")]
pub mod parse;
#[cfg(feature = "ir")]
pub mod snapshot;

#[cfg(feature = "ir")]
pub use convert::*;
#[cfg(feature = "parse")]
pub use loaded::*;

#[cfg(feature = "parse")]
mod loaded {
	use std::path::PathBuf;
	use crate::{check::check, error::ModelError, ir::*, parse::load_group};

	pub struct Loaded {
		pub set: ModelSet,
		pub files: Vec<PathBuf>,
		pub errors: Vec<ModelError>
	}

	pub fn load_groups (groups: &[(String, PathBuf)]) -> Loaded {
		let mut errors = Vec::new();
		let mut files = Vec::new();
		let mut set = ModelSet { groups: Vec::new() };

		for (name, dir) in groups {
			if let Some(group) = load_group(name, dir, &mut files, &mut errors) {
				set.groups.push(group);
			}
		}

		set.groups.sort_by(|a, b| a.name.cmp(&b.name));
		if errors.is_empty() {
			check(&set, &mut errors);
		}

		return Loaded { set, files, errors };
	}
}

#[cfg(feature = "ir")]
mod convert {
	use crate::{ir::*, runtime};

	pub fn to_runtime (set: &ModelSet) -> runtime::RuntimeModels {
		let mut document = runtime::RuntimeModels::new();

		for group in &set.groups {
			let mut db = runtime::RuntimeDb::default();

			for entity in &group.entities {
				match &entity.kind {
					EntityKind::Table(table) => {
						if table.deleted {
							continue;
						}

						let columns = table.fields
							.iter()
							.map(|field| runtime::RuntimeField {
								name: field.name.clone(),
								column: field.column.clone(),
								field_type: field.field_type,
								nullable: field.nullable,
								enum_ref: field.enum_ref
									.as_ref()
									.map(|target| match &target.group {
										Some(target_group) if target_group != &group.name => target.to_string(),
										_ => target.name.clone()
									})
							})
							.collect();

						db.tables.insert(entity.name.clone(), runtime::RuntimeModel {
							table: table.table.clone(),
							primary: table.primary().map(|index| index.columns.clone()).unwrap_or_default(),
							columns
						});
					}
					EntityKind::Enum(value) => {
						db.enums.insert(entity.name.clone(), runtime::RuntimeEnum {
							values: value.values
								.iter()
								.map(|item| runtime::RuntimeEnumValue {
									name: item.name.clone(),
									int: item.int,
									db: item.db.clone()
								})
								.collect()
						});
					}
				}
			}

			document.dbs.insert(group.name.clone(), db);
		}

		return document;
	}
}
