use std::collections::{BTreeMap, BTreeSet};
use crate::schema::*;


#[derive(Debug, Clone, PartialEq)]
pub enum Op {
	/// FKs of the table that close a cycle follow as `AddForeignKey`
	CreateTable(TableSchema),
	DropTable { table: String },
	AddColumn { table: String, column: ColumnSchema, after: Option<String> },
	DropColumn { table: String, column: String },
	ModifyColumn { table: String, from: ColumnSchema, to: ColumnSchema },
	AddPrimaryKey { table: String, columns: Vec<String> },
	DropPrimaryKey { table: String },
	AddIndex { table: String, index: IndexSchema },
	DropIndex { table: String, index: String },
	AddForeignKey { table: String, fk: ForeignKey },
	DropForeignKey { table: String, name: String },
	AlterTableOptions { table: String, from: BTreeMap<String, String>, to: BTreeMap<String, String> },
	CreateEnumType { name: String, values: Vec<String> },
	AddEnumValues { name: String, from: Vec<String>, to: Vec<String> },
	RecreateEnumType { name: String, from: Vec<String>, to: Vec<String>, columns: Vec<(String, ColumnSchema)> },
	DropEnumType { name: String },

	// TODO: generic defaults
	SetUpdateNow { table: String, columns: Vec<String> },
	CreateUpdateNowFunction,
	DropUpdateNowFunction
}

impl Op {
	pub fn get_table (&self) -> &str {
		return match self {
			Op::CreateTable(table) => &table.name,

			Op::DropTable { table }
			| Op::AddColumn { table, .. }
			| Op::DropColumn { table, .. }
			| Op::ModifyColumn { table, .. }
			| Op::AddPrimaryKey { table, .. }
			| Op::DropPrimaryKey { table }
			| Op::AddIndex { table, .. }
			| Op::DropIndex { table, .. }
			| Op::AddForeignKey { table, .. }
			| Op::DropForeignKey { table, .. }
			| Op::AlterTableOptions { table, .. }
			| Op::SetUpdateNow { table, .. } => table,

			Op::CreateEnumType { name, .. }
			| Op::AddEnumValues { name, .. }
			| Op::RecreateEnumType { name, .. }
			| Op::DropEnumType { name } => name,

			Op::CreateUpdateNowFunction | Op::DropUpdateNowFunction => ""
		};
	}

	pub fn is_enum_type (&self) -> bool {
		return matches!(self, Op::CreateEnumType { .. } | Op::AddEnumValues { .. } | Op::RecreateEnumType { .. } | Op::DropEnumType { .. });
	}
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diff {
	pub ops: Vec<Op>,
	/// Tables of `from` that are gone from `to` without a tombstone
	pub untracked: Vec<String>
}

#[derive(Default)]
struct Phases {
	drop_fks: Vec<Op>,
	auto_off: Vec<Op>,
	drop_indexes: Vec<Op>,
	options: Vec<Op>,
	types: Vec<Op>,
	columns: Vec<Op>,
	retypes: Vec<Op>,
	add_indexes: Vec<Op>,
	auto_on: Vec<Op>,
	create: Vec<Op>,
	add_fks: Vec<Op>,
	drop_tables: Vec<Op>,
	drop_types: Vec<Op>
}

pub fn diff (from: &DbSchema, to: &DbSchema) -> Diff {
	let mut phases = Phases::default();
	let mut new_tables = Vec::new();
	let mut dropped = Vec::new();
	let mut altered = Vec::new();

	for (name, target) in &to.tables {
		let source = from.tables.get(name).filter(|source| !source.deleted);
		match (source, target.deleted) {
			(Some(source), true) => dropped.push(source),
			(None, true) => {}
			(None, false) => new_tables.push(target),
			(Some(source), false) => altered.push((source, target))
		}
	}

	let untracked = from.tables
		.values()
		.filter(|source| !source.deleted && !to.tables.contains_key(&source.name))
		.map(|source| source.name.clone())
		.collect();

	let mut retyped: BTreeSet<(String, String)> = BTreeSet::new();
	for (source, target) in &altered {
		for column in &target.columns {
			if source.find_column(&column.name).is_some_and(|old| old.db_type != column.db_type) {
				retyped.insert((target.name.clone(), column.name.clone()));
			}
		}
	}

	for (source, target) in &altered {
		alter_table(source, target, &retyped, &mut phases);
	}

	create_tables(&new_tables, &mut phases);
	drop_tables(&dropped, &mut phases);
	enum_types(from, to, &mut phases);

	let update_now = |schema: &DbSchema|
		schema.tables
			.values()
			.any(|table| !table.deleted && table.columns.iter().any(|column| column.on_update_now));

	match (update_now(from), update_now(to)) {
		(false, true) => phases.types.push(Op::CreateUpdateNowFunction),
		(true, false) => phases.drop_types.push(Op::DropUpdateNowFunction),
		_ => {}
	}

	let ops = [
		phases.drop_fks,
		phases.auto_off,
		phases.drop_indexes,
		phases.options,
		phases.types,
		phases.columns,
		phases.retypes,
		phases.add_indexes,
		phases.auto_on,
		phases.create,
		phases.add_fks,
		phases.drop_tables,
		phases.drop_types
	].concat();

	return Diff { ops, untracked };
}

fn enum_types (from: &DbSchema, to: &DbSchema, phases: &mut Phases) {
	for (name, values) in &to.enum_types {
		let old = match from.enum_types.get(name) {
			Some(old) if old == values => continue,
			Some(old) => old,
			None => {
				phases.types.push(Op::CreateEnumType {
					name: name.clone(),
					values: values.clone()
				});
				continue;
			}
		};

		let mut kept = values.iter().filter(|value| old.contains(value));
		if old.iter().all(|value| kept.next() == Some(value)) {
			phases.types.push(Op::AddEnumValues {
				name: name.clone(),
				from: old.clone(),
				to: values.clone()
			});
			continue;
		}

		let mut columns = Vec::new();
		for table in to.tables.values().filter(|table| from.tables.contains_key(&table.name)) {
			for column in &table.columns {
				if column.db_type == ColumnType::EnumType(name.clone()) {
					columns.push((table.name.clone(), column.clone()));
				}
			}
		}

		phases.retypes.push(Op::RecreateEnumType {
			name: name.clone(),
			from: old.clone(),
			to: values.clone(), columns
		});
	}

	for name in from.enum_types.keys() {
		if !to.enum_types.contains_key(name) {
			phases.drop_types.push(Op::DropEnumType { name: name.clone() });
		}
	}
}

fn alter_table (source: &TableSchema, target: &TableSchema, retyped: &BTreeSet<(String, String)>, phases: &mut Phases) {
	let table = target.name.clone();

	if source.options != target.options {
		phases.options.push(Op::AlterTableOptions {
			table: table.clone(),
			from: source.options.clone(),
			to: target.options.clone()
		});
	}

	if source.primary != target.primary {
		if !source.primary.is_empty() {
			phases.drop_indexes.push(Op::DropPrimaryKey { table: table.clone() });
		}

		if !target.primary.is_empty() {
			phases.add_indexes.push(Op::AddPrimaryKey {
				table: table.clone(),
				columns: target.primary.clone()
			});
		}
	}

	let mut dropped_indexes = BTreeSet::new();
	for index in &source.indexes {
		if target.find_index(&index.name) != Some(index) {
			dropped_indexes.insert(index.name.clone());

			phases.drop_indexes.push(Op::DropIndex {
				table: table.clone(),
				index: index.name.clone()
			});
		}
	}

	for index in &target.indexes {
		if source.find_index(&index.name) != Some(index) {
			phases.add_indexes.push(Op::AddIndex {
				table: table.clone(),
				index: index.clone()
			});
		}
	}

	let touches = |fk: &ForeignKey| -> bool {
		return dropped_indexes.contains(&fk.index)
			|| fk.columns.iter().any(|column| retyped.contains(&(table.clone(), column.clone())))
			|| fk.ref_columns.iter().any(|column| retyped.contains(&(fk.ref_table.clone(), column.clone())));
	};

	for fk in &source.foreign_keys {
		let kept = target.find_foreign_key(&fk.name).is_some_and(|new| new.same(fk) && !touches(new));
		if !kept {
			phases.drop_fks.push(Op::DropForeignKey {
				table: table.clone(),
				name: fk.name.clone()
			});
		}
	}

	for fk in &target.foreign_keys {
		let kept = source.find_foreign_key(&fk.name).is_some_and(|old| old.same(fk) && !touches(fk));
		if !kept {
			phases.add_fks.push(Op::AddForeignKey {
				table: table.clone(),
				fk: fk.clone()
			});
		}
	}

	for column in &source.columns {
		if target.find_column(&column.name).is_none() {
			if column.auto_increment {
				phases.auto_off.push(Op::ModifyColumn {
					table: table.clone(),
					from: column.clone(),
					to: ColumnSchema { auto_increment: false, ..column.clone() }
				});
			}

			phases.columns.push(Op::DropColumn {
				table: table.clone(),
				column: column.name.clone()
			});
		}
	}

	for column in &target.columns {
		if let Some(old) = source.find_column(&column.name) {
			if old != column {
				let op = Op::ModifyColumn {
					table: table.clone(),
					from: old.clone(),
					to: column.clone()
				};

				match (old.auto_increment, column.auto_increment) {
					(true, false) => phases.auto_off.push(op),
					(false, true) => phases.auto_on.push(op),
					_ => phases.columns.push(op)
				}
			}
		}
	}

	let update_now = |table: &TableSchema| -> BTreeSet<String> {
		return table.columns
			.iter()
			.filter(|column| column.on_update_now)
			.map(|column| column.name.clone())
			.collect();
	};

	if update_now(source) != update_now(target) {
		phases.retypes.push(Op::SetUpdateNow {
			table: table.clone(),
			columns: target.columns
				.iter()
				.filter(|column| column.on_update_now)
				.map(|column| column.name.clone())
				.collect()
		});
	}

	let mut previous: Option<String> = None;
	for column in &target.columns {
		if source.find_column(&column.name).is_none() {
			let added = ColumnSchema { auto_increment: false, ..column.clone() };
			if column.auto_increment {
				phases.auto_on.push(Op::ModifyColumn {
					table: table.clone(),
					from: added.clone(),
					to: column.clone()
				});
			}

			phases.columns.push(Op::AddColumn {
				table: table.clone(),
				column: added,
				after: previous.clone()
			});
		}

		previous = Some(column.name.clone());
	}
}

fn create_tables (tables: &[&TableSchema], phases: &mut Phases) {
	// On a cycle the first remaining table (by name) is created without FKs to the tables that are not created yet
	let mut remaining: BTreeMap<&str, &TableSchema> = tables
		.iter()
		.map(|table| (table.name.as_str(), *table))
		.collect();

	while !remaining.is_empty() {
		let ready = remaining
			.values()
			.find(|table| {
				table.foreign_keys
					.iter()
					.all(|fk| fk.ref_table == table.name || !remaining.contains_key(fk.ref_table.as_str()))
			})
			.copied();

		let (table, deferred) = match ready {
			Some(table) => (table.clone(), Vec::new()),
			None => {
				let first = *remaining.values().next().unwrap();
				let mut table = first.clone();

				let (deferred, inline): (Vec<ForeignKey>, Vec<ForeignKey>) = table.foreign_keys
					.drain(..)
					.partition(|fk| fk.ref_table != first.name && remaining.contains_key(fk.ref_table.as_str()));

				table.foreign_keys = inline;
				(table, deferred)
			}
		};

		remaining.remove(table.name.as_str());
		for fk in deferred {
			phases.add_fks.push(Op::AddForeignKey { table: table.name.clone(), fk });
		}

		phases.create.push(Op::CreateTable(table));
	}
}

fn drop_tables (tables: &[&TableSchema], phases: &mut Phases) {
	// On a cycle FKs that reference the first remaining table are dropped beforehand
	let mut remaining: BTreeMap<&str, &TableSchema> = tables
		.iter()
		.map(|table| (table.name.as_str(), *table))
		.collect();

	while !remaining.is_empty() {
		let referenced = |name: &str|
			remaining
				.values()
				.any(|other| other.name != name && other.foreign_keys.iter().any(|fk| fk.ref_table == name));

		let ready = remaining.keys().find(|name| !referenced(name)).copied();

		let name = match ready {
			Some(name) => name,
			None => {
				let first = *remaining.keys().next().unwrap();
				for other in remaining.values() {
					for fk in &other.foreign_keys {
						if other.name != first && fk.ref_table == first {
							phases.drop_fks.push(Op::DropForeignKey {
								table: other.name.clone(),
								name: fk.name.clone()
							});
						}
					}
				}

				first
			}
		};

		remaining.remove(name);
		phases.drop_tables.push(Op::DropTable { table: name.to_owned() });
	}
}
