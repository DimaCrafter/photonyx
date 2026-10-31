use photonyx_models::ir::{self, DeletePolicy, DialectBlock, EntityKind, ModelSet};
use crate::{dialect::SqlDialect, schema::*};


pub fn to_db (set: &ModelSet, group: &str, dialect: &dyn SqlDialect, defaults: &[DialectBlock]) -> Result<DbSchema, String> {
	let current = set.find_group(group).ok_or_else(|| format!("group `{group}` not found"))?;
	let mut schema = DbSchema::default();
	let mut errors = Vec::new();

	for entity in &current.entities {
		let table = match &entity.kind {
			EntityKind::Table(table) => table,
			EntityKind::Enum(_) => continue
		};

		match table_to_db(set, group, &entity.name, table, dialect, defaults) {
			Ok((result, enum_types)) => {
				if schema.tables.contains_key(&result.name) {
					errors.push(format!("{}: table `{}` is declared by two models", entity.name, result.name));
				}

				if !result.deleted {
					for (name, values) in enum_types {
						match schema.enum_types.get(&name) {
							Some(existing) if *existing != values => {
								errors.push(format!("{}: enum type `{name}` is used with other values by another model", entity.name));
							}
							_ => {
								schema.enum_types.insert(name, values);
							}
						}
					}
				}

				schema.tables.insert(result.name.clone(), result);
			}
			Err(error) => {
				errors.push(format!("{}: {error}", entity.name));
			}
		}
	}

	for table in schema.tables.values() {
		if table.deleted {
			continue;
		}

		for fk in &table.foreign_keys {
			if schema.tables.get(&fk.ref_table).is_some_and(|target| target.deleted) {
				errors.push(format!("{}: index `{}` links to `{}`, which is marked `deleted: true`; remove the link first", table.model, fk.index, fk.ref_table));
			}
		}
	}

	if !errors.is_empty() {
		return Err(errors.join("\n"));
	}

	return Ok(schema);
}

fn table_to_db (
	set: &ModelSet,
	group: &str,
	model: &str,
	table: &ir::Table,
	dialect: &dyn SqlDialect,
	defaults: &[DialectBlock]
) -> Result<(TableSchema, Vec<(String, Vec<String>)>), String> {
	let mut result = TableSchema::new(&dialect.table_name(table, defaults)?);
	let mut enum_types = Vec::new();
	result.model = model.to_owned();
	result.deleted = table.deleted;
	result.options = dialect.table_options(table, defaults)?;

	let column_of = |field_name: &str| -> Result<String, String> {
		return table
			.find_field(field_name)
			.map(|field| field.column_name().to_owned())
			.ok_or_else(|| format!("field `{field_name}` not found"));
	};

	for field in &table.fields {
		let enum_entity = match &field.enum_ref {
			Some(target) => match set.find_entity(group, target) {
				Some(ir::Entity { kind: EntityKind::Enum(value), .. }) => Some(value),
				_ => return Err(format!("field `{}`: enum `{target}` not found", field.name))
			},
			None => None
		};

		let options = dialect
			.column_options(field)
			.map_err(|error| format!("field `{}`: {error}", field.name))?;

		let default = match (&field.default, options.default) {
			(Some(_), Some(_)) => {
				return Err(format!("field `{}`: `default` and `{}.default` together", field.name, dialect.id()));
			}
			(Some(value), None) => Some(default_to_db(field, value, enum_entity)?),
			(None, Some(expr)) => Some(DefaultValue::Expr(expr)),
			(None, None) => None
		};

		if options.generated.is_some() && (default.is_some() || field.auto || field.update_now) {
			return Err(format!("field `{}`: a generated column can not have a default, `auto` or `update`", field.name));
		}

		let column_type = dialect
			.map_type(field, enum_entity)
			.map_err(|error| format!("field `{}`: {error}", field.name))?;

		if let (ColumnType::EnumType(name), Some(entity)) = (&column_type, enum_entity) {
			enum_types.push((name.clone(), entity.values.iter().map(|value| value.db_name().to_owned()).collect()));
		}

		result.columns.push(ColumnSchema {
			name: field.column_name().to_owned(),
			db_type: column_type,
			nullable: field.nullable,
			default,
			on_update_now: field.update_now,
			generated: options.generated,
			auto_increment: field.auto,
			collation: options.collation
		});
	}

	for index in &table.indexes {
		let columns = index.columns
			.iter()
			.map(|name| column_of(name))
			.collect::<Result<Vec<_>, _>>()?;

		if index.primary {
			result.primary = columns;
			continue;
		}

		let name = dialect
			.index_name(table, index)
			.map_err(|error| format!("index `{}`: {error}", index.name))?;

		let using = dialect
				.index_using(index)
				.map_err(|error| format!("index `{}`: {error}", index.name))?;

		result.indexes.push(IndexSchema {
			name: name.clone(),
			columns: columns.clone(),
			unique: index.unique,
			using
		});

		if let Some(link) = &index.link {
			let target = match set.find_entity(group, &link.target) {
				Some(ir::Entity { kind: EntityKind::Table(target), .. }) => target,
				_ => return Err(format!("index `{}`: link target `{}` not found", index.name, link.target))
			};

			let target_index = match &link.index {
				Some(name) => target.find_index(name),
				None => target.primary()
			}.ok_or_else(|| format!("index `{}`: link target index of `{}` not found", index.name, link.target))?;

			let ref_columns = target_index.columns
				.iter()
				.map(|name|
					target
						.find_field(name)
						.map(|field| field.column_name().to_owned())
						.ok_or_else(|| format!("field `{}.{name}` not found", link.target))
					)
				.collect::<Result<Vec<_>, _>>()?;

			result.foreign_keys.push(ForeignKey {
				name: index.constraint.clone().unwrap_or_else(|| index.name.clone()),
				index: name,
				columns,
				ref_table: dialect.table_name(target, defaults)?,
				ref_columns,
				ref_index: match &link.index {
					Some(_) => Some(dialect.index_name(target, target_index)?),
					None => None
				},
				on_delete: match link.delete {
					DeletePolicy::Restrict => OnDelete::Restrict,
					DeletePolicy::Cascade => OnDelete::Cascade,
					DeletePolicy::SetNull => OnDelete::SetNull,
					DeletePolicy::None => OnDelete::NoAction
				}
			});
		}
	}

	return Ok((result, enum_types));
}

fn default_to_db (field: &ir::Field, value: &ir::Default, enum_entity: Option<&ir::Enum>) -> Result<DefaultValue, String> {
	if let (Some(entity), ir::Default::Str(name)) = (enum_entity, value) {
		let item = entity.values
			.iter()
			.find(|item| &item.name == name)
			.ok_or_else(|| format!("field `{}`: default `{name}` is not a value of the enum", field.name))?;

		if field.field_type.is_integer() {
			return item.int.map(DefaultValue::Int).ok_or_else(|| format!("field `{}`: enum value `{name}` has no `int`", field.name));
		}

		return Ok(DefaultValue::Str(item.db_name().to_owned()));
	}

	return Ok(match value {
		ir::Default::Int(value) => DefaultValue::Int(*value),
		ir::Default::Float(value) => DefaultValue::Float(*value),
		ir::Default::Bool(value) => DefaultValue::Bool(*value),
		ir::Default::Str(value) => DefaultValue::Str(value.clone()),
		ir::Default::Now => DefaultValue::Now
	});
}
