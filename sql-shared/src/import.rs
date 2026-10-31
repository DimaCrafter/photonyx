use std::{collections::{BTreeMap, BTreeSet}, path::Path};
use photonyx_models::ir::{self, DeletePolicy, DialectBlock, Entity, EntityKind, EntityRef, Group, Raw};
use crate::{dialect::SqlDialect, schema::*};


pub struct Draft {
	pub group: Group,
	pub notes: Vec<String>
}

pub fn import (schema: &DbSchema, group: &str, dir: &Path, dialect: &dyn SqlDialect, defaults: &[DialectBlock]) -> Result<Draft, String> {
	let mut notes = Vec::new();
	let mut entities = Vec::new();

	// Table name --> Model name
	let mut models: BTreeMap<&str, String> = BTreeMap::new();
	let mut taken = BTreeSet::new();
	for name in schema.tables.keys() {
		let (mut model, irregular) = model_name(name);
		if !photonyx_models::check::is_identifier(&model) {
			model = format!("T{model}");
		}

		let base = model.clone();
		let mut counter = 2;
		while taken.contains(&model.to_lowercase()) {
			model = format!("{base}{counter}");
			counter += 1;
		}

		if irregular || model != base {
			notes.push(format!("{name}: model name `{model}` is a guess, rename the file (and `link:`s to it) if needed"));
		}

		taken.insert(model.to_lowercase());
		models.insert(name.as_str(), model);
	}

	let mut type_entities: BTreeMap<&str, String> = BTreeMap::new();
	for (type_name, values) in &schema.enum_types {
		let mut entity_name = pascal(type_name.rsplit('.').next().unwrap_or(type_name));
		if !photonyx_models::check::is_identifier(&entity_name) {
			entity_name = format!("T{entity_name}");
		}

		let base = entity_name.clone();
		let mut counter = 2;
		while taken.contains(&entity_name.to_lowercase()) {
			entity_name = format!("{base}{counter}");
			counter += 1;
		}

		taken.insert(entity_name.to_lowercase());
		entities.push(Entity {
			file: dir.join(format!("{entity_name}.yml")),
			kind: EntityKind::Enum(ir::Enum { values: enum_items(values) }),
			name: entity_name.clone()
		});

		type_entities.insert(type_name.as_str(), entity_name);
	}

	let mut bools = Vec::new();
	for (name, table) in &schema.tables {
		let model = models[name.as_str()].clone();
		let mut result = ir::Table { table: name.clone(), fields: Vec::new(), indexes: Vec::new(), deleted: false, dialects: Vec::new() };

		// Column --> Field name
		let mut fields: BTreeMap<&str, String> = BTreeMap::new();
		let mut field_names = BTreeSet::new();
		for column in &table.columns {
			let mut field = identifier(&column.name);
			if photonyx_models::check::is_reserved_field(&field) {
				field.push('_');
			}

			let base = field.clone();
			let mut counter = 2;
			while field_names.contains(&field) {
				field = format!("{base}{counter}");
				counter += 1;
			}

			field_names.insert(field.clone());
			fields.insert(column.name.as_str(), field);
		}

		for column in &table.columns {
			let field_name = fields[column.name.as_str()].clone();
			let at = format!("{name}.{}", column.name);

			let mut enum_entity = None;
			let mut shared_enum = None;
			let field_type = match &column.db_type {
				ColumnType::Int { size, unsigned } => match (size, unsigned) {
					(IntSize::Tiny, true) => ir::FieldType::U8,
					(IntSize::Tiny, false) => ir::FieldType::I8,
					(IntSize::Small, true) => ir::FieldType::U16,
					(IntSize::Small, false) => ir::FieldType::I16,
					(IntSize::Medium, unsigned) => {
						notes.push(format!(
							"{at}: mediumint has no model type, imported as {}; drift will show the difference until a migration widens it",
							if *unsigned { "u32" } else { "i32" }
						));

						if *unsigned {
							ir::FieldType::U32
						} else {
							ir::FieldType::I32
						}
					}
					(IntSize::Normal, true) => ir::FieldType::U32,
					(IntSize::Normal, false) => ir::FieldType::I32,
					(IntSize::Big, true) => ir::FieldType::U64,
					(IntSize::Big, false) => ir::FieldType::I64
				},
				ColumnType::Float => ir::FieldType::F32,
				ColumnType::Double => ir::FieldType::F64,
				ColumnType::Bool => {
					bools.push(at.clone());
					ir::FieldType::Bool
				}
				ColumnType::Varchar(size) => ir::FieldType::String(*size),
				ColumnType::Char(size) => ir::FieldType::Char(*size),
				ColumnType::Text => ir::FieldType::Text,
				ColumnType::DateTime => ir::FieldType::DateTime,
				ColumnType::Json => ir::FieldType::Json,
				ColumnType::Enum(values) => {
					let entity_name = format!("{model}{}", pascal(&field_name));
					enum_entity = Some((entity_name, values.clone()));
					ir::FieldType::Enum
				}
				ColumnType::EnumType(type_name) => match type_entities.get(type_name.as_str()) {
					Some(entity_name) => {
						shared_enum = Some(entity_name.clone());
						ir::FieldType::Enum
					}
					None => {
						notes.push(format!("{at}: enum type `{type_name}` was not read, the column is not imported"));
						continue;
					}
				},
				ColumnType::Other(raw) => {
					notes.push(format!("{at}: type `{raw}` has no model type, the column is not imported (drift will show it)"));
					continue;
				}
			};

			let mut enum_ref = None;
			let mut enum_values = Vec::new();
			if let Some((entity_name, values)) = enum_entity {
				enum_values = enum_items(&values);
				notes.push(format!("{at}: native enum --> `{entity_name}.yml`, rename it or merge with an existing enum if needed"));

				entities.push(Entity {
					file: dir.join(format!("{entity_name}.yml")),
					kind: EntityKind::Enum(ir::Enum { values: enum_values.clone() }),
					name: entity_name.clone()
				});

				enum_ref = Some(EntityRef { group: None, name: entity_name });
			}

			if let Some(entity_name) = shared_enum {
				let type_name = match &column.db_type {
					ColumnType::EnumType(type_name) => type_name,
					_ => unreachable!()
				};

				enum_values = enum_items(&schema.enum_types[type_name]);
				enum_ref = Some(EntityRef { group: None, name: entity_name });
			}

			let mut block_entries: Vec<(&str, Raw)> = Vec::new();
			if let Some(collation) = &column.collation {
				block_entries.push(("collation", Raw::Str(collation.clone())));
			}

			if let Some(generated) = &column.generated {
				block_entries.push(("generated", Raw::Str(generated.expr.clone())));
				block_entries.push(("stored", Raw::Bool(generated.stored)));
			}

			let default = match &column.default {
				None => None,
				Some(DefaultValue::Expr(expr)) => {
					block_entries.push(("default", Raw::Str(expr.clone())));
					None
				}
				Some(value) => match default_of(value, field_type, &enum_values) {
					Ok(value) => Some(value),
					Err(error) => {
						notes.push(format!("{at}: {error}, default is not imported"));
						None
					}
				}
			};

			let mut dialects = Vec::new();
			if !block_entries.is_empty() {
				dialects.push(DialectBlock {
					dialect: dialect.id().to_owned(),
					value: Raw::Map(
						block_entries
							.into_iter()
							.map(|(key, value)| (key.to_owned(), value))
							.collect()
					)
				});
			}

			result.fields.push(ir::Field {
				column: if field_name != column.name { Some(column.name.clone()) } else { None },
				name: field_name,
				field_type,
				nullable: column.nullable,
				enum_ref,
				default,
				update_now: column.on_update_now,
				auto: column.auto_increment,
				dialects,
				pos: ir::Pos::default()
			});
		}

		let imported = |column: &str|
			result.fields
				.iter()
				.find(|field| field.column_name() == column)
				.map(|field| field.name.clone());

		let field_list = |columns: &[String]| -> Option<Vec<String>> {
			return columns
				.iter()
				.map(|column| imported(column))
				.collect();
		};

		if !table.primary.is_empty() {
			match field_list(&table.primary) {
				Some(columns) => result.indexes.push(index("primary", columns, true, false)),
				None => notes.push(format!("{name}: primary key uses a column that was not imported, skipped"))
			}
		}

		// Map index names of the database to names in models
		let index_name = |table: &str, index: &str|
			dialect.import_index_name(table.rsplit('.').next().unwrap_or(table), index);

		for item in &table.indexes {
			let columns = match field_list(&item.columns) {
				Some(columns) => columns,
				None => {
					notes.push(format!("{name}: index `{}` uses a column that was not imported, skipped", item.name));
					continue;
				}
			};

			let (model_name, mut entries) = index_name(name, &item.name);
			let mut value = index(&model_name, columns, false, item.unique);
			if let Some(using) = &item.using {
				entries.insert("using".to_owned(), using.clone());
			}

			if !entries.is_empty() {
				value.dialects.push(block(dialect.id(), entries.into_iter().collect()));
			}

			result.indexes.push(value);
		}

		for fk in &table.foreign_keys {
			let at = format!("{name}: foreign key `{}`", fk.name);
			let target = match models.get(fk.ref_table.as_str()) {
				Some(target) => target.clone(),
				None => {
					notes.push(format!("{at} references `{}`, which is not imported; the link is skipped", fk.ref_table));
					continue;
				}
			};

			let fk_index = index_name(name, &fk.index).0;
			let position = result.indexes
				.iter()
				.position(|item| !item.primary && item.name == fk_index);

			let position = match position {
				Some(position) if result.indexes[position].link.is_none() => {
					position
				}
				_ => {
					notes.push(format!("{at}: no separate index `{fk_index}` under it, the link is skipped (declare an index with `link:` by hand)"));
					continue;
				}
			};

			if field_list(&fk.columns).as_ref() != Some(&result.indexes[position].columns) {
				notes.push(format!("{at}: index `{fk_index}` has other columns than the foreign key, the link is skipped"));
				continue;
			}

			let target_table = &schema.tables[&fk.ref_table];
			let expected = match &fk.ref_index {
				None => {
					target_table.primary.clone()
				}
				Some(index_name) => {
					target_table
						.find_index(index_name)
						.map(|item| item.columns.clone())
						.unwrap_or_default()
				}
			};

			if expected != fk.ref_columns {
				notes.push(format!(
					"{at}: referenced columns ({}) are not the whole {} of `{}`, the link is skipped",
					fk.ref_columns.join(", "),
					fk.ref_index.as_deref().unwrap_or("primary key"),
					fk.ref_table
				));
				continue;
			}

			let item = &mut result.indexes[position];
			item.link = Some(ir::Link {
				target: EntityRef { group: None, name: target },
				index: fk.ref_index.as_ref().map(|ref_index| index_name(&fk.ref_table, ref_index).0),
				delete: match fk.on_delete {
					OnDelete::Restrict | OnDelete::NoAction => DeletePolicy::Restrict,
					OnDelete::Cascade => DeletePolicy::Cascade,
					OnDelete::SetNull => DeletePolicy::SetNull
				}
			});

			if fk.name != fk_index {
				item.constraint = Some(fk.name.clone());
			}
		}

		let options = dialect
			.import_table_options(&table.options, defaults)
			.map_err(|error| format!("{name}: {error}"))?;

		if !options.is_empty() {
			result.dialects.push(block(dialect.id(), options.into_iter().collect()));
		}

		entities.push(Entity {
			file: dir.join(format!("{model}.yml")),
			name: model,
			kind: EntityKind::Table(result)
		});
	}

	// Only MySQL keeps bool as tinyint(1)
	if !bools.is_empty() && dialect.id() == "mysql" {
		notes.push(format!("tinyint(1) imported as bool: {} (use u8 if it stores numbers)", bools.join(", ")));
	}

	entities.sort_by(|a, b| a.name.cmp(&b.name));
	return Ok(Draft {
		group: Group {
			name: group.to_owned(),
			dir: dir.to_path_buf(),
			entities
		},
		notes
	});
}

fn index (name: &str, columns: Vec<String>, primary: bool, unique: bool) -> ir::Index {
	return ir::Index {
		name: name.to_owned(),
		columns,
		primary,
		unique,
		link: None,
		constraint: None,
		dialects: Vec::new(),
		pos: ir::Pos::default()
	};
}

fn block<K: ToString> (dialect: &str, entries: Vec<(K, String)>) -> DialectBlock {
	return DialectBlock {
		dialect: dialect.to_owned(),
		value: Raw::Map(entries.into_iter().map(|(key, value)| (key.to_string(), Raw::Str(value))).collect())
	};
}

fn default_of (value: &DefaultValue, field_type: ir::FieldType, enum_values: &[ir::EnumValue]) -> Result<ir::Default, String> {
	return Ok(match value {
		DefaultValue::Str(value) if field_type == ir::FieldType::Enum => {
			let item = enum_values
				.iter()
				.find(|item| item.db_name() == value)
				.ok_or_else(|| format!("default `{value}` is not a value of the enum"))?;

			ir::Default::Str(item.name.clone())
		}
		DefaultValue::Float(value) if value.fract() == 0.0 && value.abs() < 1e15 => {
			ir::Default::Int(*value as i64)
		}
		DefaultValue::Float(value) => {
			ir::Default::Float(*value)
		}
		DefaultValue::Int(value) if field_type == ir::FieldType::Bool => {
			ir::Default::Bool(*value != 0)
		}
		DefaultValue::Int(value) => {
			ir::Default::Int(*value)
		}
		DefaultValue::Bool(value) => {
			ir::Default::Bool(*value)
		}
		DefaultValue::Str(value) => {
			ir::Default::Str(value.clone())
		}
		DefaultValue::Now => ir::Default::Now,
		// Expression goes to the dialect block, see the caller
		DefaultValue::Expr(_) => return Err("expression default".to_owned())
	});
}

fn enum_items (values: &[String]) -> Vec<ir::EnumValue> {
	let mut result: Vec<ir::EnumValue> = Vec::new();
	for value in values {
		let mut name = identifier(value);
		let base = name.clone();
		let mut counter = 2;
		while result.iter().any(|item| item.name == name) {
			name = format!("{base}{counter}");
			counter += 1;
		}

		result.push(ir::EnumValue {
			db: if &name != value { Some(value.clone()) } else { None },
			name,
			int: None,
			doc: None,
			pos: ir::Pos::default()
		});
	}

	return result;
}

/// `user-id` --> `user_id`, `1st` --> `_1st`, empty --> `_`
fn identifier (raw: &str) -> String {
	let mut result: String = raw
		.chars()
		.map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '_' })
		.collect();

	if !result.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_') {
		result.insert(0, '_');
	}

	return result;
}

/// `user_id` --> `UserId`, `createdAt` --> `CreatedAt`
fn pascal (raw: &str) -> String {
	return raw
		.split(|ch: char| !ch.is_ascii_alphanumeric())
		.filter(|word| !word.is_empty())
		.map(|word| {
			let mut chars = word.chars();
			let first = chars.next().unwrap();
			format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
		})
		.collect();
}

pub fn model_name (table: &str) -> (String, bool) {
	let mut words: Vec<String> = table
		.split(|ch: char| !ch.is_ascii_alphanumeric())
		.filter(|word| !word.is_empty())
		.map(str::to_owned)
		.collect();

	let last = match words.pop() {
		Some(last) => last,
		None => return ("Table".to_owned(), true)
	};

	let lower = last.to_ascii_lowercase();
	let (singular, irregular) = if lower == "uses" {
		(last[..last.len() - 1].to_owned(), false)
	} else if lower.ends_with("ies") && lower.len() > 3 {
		(format!("{}y", &last[..last.len() - 3]), false)
	} else if ["sses", "shes", "ches", "xes", "uses"].iter().any(|suffix| lower.ends_with(suffix)) {
		(last[..last.len() - 2].to_owned(), false)
	} else if lower.ends_with("ss") || lower.ends_with("us") || lower.ends_with("is") {
		(last.clone(), true)
	} else if let Some(stem) = last.strip_suffix('s') {
		(stem.to_owned(), false)
	} else {
		(last.clone(), true)
	};

	words.push(singular);
	return (pascal(&words.join("_")), irregular);
}
