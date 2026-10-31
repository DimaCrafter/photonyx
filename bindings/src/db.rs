use std::{any::Any, ffi::c_void};
use json::JsonValue;
use photonyx_models::runtime::{RuntimeField, RuntimeModels, FieldType};
use crate::{c::Slice, rs::rs_fn};


pub type EntityHandle = *mut c_void;
pub type EntityInitFn = extern "C" fn () -> *mut c_void;
pub type EntityDeinitFn = extern "C" fn (*mut c_void);

pub struct ModelMeta {
	pub db: String,
	pub name: String,
	pub table: String,
	pub primary_key_cols: Vec<String>,
	pub fields: Vec<ModelField>,
	pub init: EntityInitFn,
	pub deinit: EntityDeinitFn
}

pub struct ModelField {
	/// Field name in code
	pub name: String,
	/// Column name in the database
	pub column: String,
	pub type_id: ModelFieldType,
	/// Abstract storage type of the column (`u32`, `string(32)`, `enum`, ...)
	pub storage: FieldType,
	pub nullable: bool,
	pub enum_values: Vec<ModelEnumValue>,
	pub setter: EntitySetAnyField
}

pub struct ModelEnumValue {
	pub name: String,
	/// String representation in the database when natively supported
	pub db: String,
	/// Numeric representation in the database
	pub int: Option<i64>
}

impl ModelField {
	pub fn enum_ordinal_by_str (&self, value: &str) -> Option<u32> {
		return self.enum_values
			.iter()
			.position(|item| item.db == value)
			.map(|index| index as u32);
	}

	pub fn enum_ordinal_by_int (&self, value: i64) -> Option<u32> {
		return self.enum_values
			.iter()
			.position(|item| item.int == Some(value))
			.map(|index| index as u32);
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModelFieldType {
    U8,
    I8,
    U16,
    I16,
    F32,
    U32,
    I32,
    F64,
    U64,
    I64,
    Bool,
    String,
    Json,
    DateTime,
    Enum
}

impl ModelFieldType {
	pub fn from_column (column: &RuntimeField) -> Self {
		if column.enum_ref.is_some() {
			return ModelFieldType::Enum;
		}

		return match column.field_type {
			FieldType::U8 => ModelFieldType::U8,
			FieldType::U16 => ModelFieldType::U16,
			FieldType::U32 => ModelFieldType::U32,
			FieldType::U64 => ModelFieldType::U64,
			FieldType::I8 => ModelFieldType::I8,
			FieldType::I16 => ModelFieldType::I16,
			FieldType::I32 => ModelFieldType::I32,
			FieldType::I64 => ModelFieldType::I64,
			FieldType::F32 => ModelFieldType::F32,
			FieldType::F64 => ModelFieldType::F64,
			FieldType::Bool => ModelFieldType::Bool,
			FieldType::String(_) | FieldType::Char(_) | FieldType::Text => ModelFieldType::String,
			FieldType::DateTime => ModelFieldType::DateTime,
			FieldType::Json => ModelFieldType::Json,
			FieldType::Enum => ModelFieldType::Enum
		};
	}
}

impl ModelMeta {
	pub fn from_runtime (
		document: &RuntimeModels,
		db: &str,
		model: &str,
		init: EntityInitFn,
		deinit: EntityDeinitFn,
		setters: &[EntitySetAnyField]
	) -> Result<Self, String> {
		let table = document
			.find_table(db, model)
			.ok_or_else(|| format!("model {db}.{model} is not described in the module's models schema"))?;

		if setters.len() != table.columns.len() {
			return Err(format!("model {db}.{model}: {} setters given, but the schema has {} columns", setters.len(), table.columns.len()));
		}

		let mut fields = Vec::new();
		for (column, setter) in table.columns.iter().zip(setters) {
			let mut enum_values = Vec::new();
			if let Some(enum_ref) = &column.enum_ref {
				let value = document
					.find_enum(db, enum_ref)
					.ok_or_else(|| format!("model {db}.{model}: enum {enum_ref} is not described in the models schema"))?;

				enum_values = value.values
					.iter()
					.map(|item| ModelEnumValue {
						name: item.name.clone(),
						db: item.db_name().to_owned(),
						int: item.int
					})
					.collect();
			}

			fields.push(ModelField {
				name: column.name.clone(),
				column: column.column_name().to_owned(),
				type_id: ModelFieldType::from_column(column),
				storage: column.field_type,
				nullable: column.nullable,
				enum_values,
				setter: *setter
			});
		}

		let mut primary_key_cols = Vec::new();
		for name in &table.primary {
			let field = fields
				.iter()
				.find(|field| &field.name == name)
				.ok_or_else(|| format!("model {db}.{model}: primary key field {name} not found"))?;

			primary_key_cols.push(field.column.clone());
		}

		return Ok(ModelMeta {
			db: db.to_owned(),
			name: model.to_owned(),
			table: table.table.clone(),
			primary_key_cols,
			fields,
			init,
			deinit
		});
	}

	/// A new entity with default values
	#[inline]
	pub fn init_entity (&self) -> Result<EntityHandle, String> {
		let entity = (self.init)();
		if entity.is_null() {
			return Err(format!("model {}.{}: failed to allocate an entity (out of memory)", &self.db, &self.name));
		}

		return Ok(entity);
	}

	#[inline]
	pub fn init_entity_list (&self) -> EntityList {
		return EntityList::new(self.deinit);
	}

	#[inline]
	pub fn deinit_entity (&self, ptr: EntityHandle) {
		(self.deinit)(ptr);
	}
}

pub type EntitySetAnyField = extern "C" fn (entity: *mut c_void, value: *const c_void);


#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelsCheck {
	/// Model names whose hash differs from the applied one
	pub mismatched: Vec<String>,
	/// Model names that are not in `photonyx_models`
	pub unknown: Vec<String>,
	/// `true` when `photonyx_models` does not exist
	pub no_table: bool
}

impl ModelsCheck {
	pub fn is_ok (&self) -> bool {
		return self.mismatched.is_empty() && self.unknown.is_empty() && !self.no_table;
	}
}

pub trait DatabaseConnection: Sync + Send {
    fn as_any (&self) -> &dyn Any;

	fn new_query (&self, table: &str) -> Box<dyn QueryBuilder>;
	fn exec_value (&mut self, query_any: Box<dyn Any>) -> Result<Option<Box<dyn Any>>, String>;
	fn exec_first (&mut self, model: &ModelMeta, query_any: Box<dyn Any>) -> Result<Option<EntityHandle>, String>;
    fn exec_all (&mut self, model: &ModelMeta, query_any: Box<dyn Any>) -> Result<EntityList, String>;

	/// Compares `(model, hash)` pairs of modules with the models applied to the database.
	/// Returns `Ok(None)` if the driver does not support this check.
	fn check_models (&self, _models: &[(String, String)]) -> Result<Option<ModelsCheck>, String> {
		return Ok(None);
	}
}

pub trait DatabaseImpl {
    fn get_id (&self) -> String;
    fn connect (&self, raw_config: &JsonValue) -> Result<Box<dyn DatabaseConnection>, String>;
}

pub trait QueryBuilder {
	fn count (&mut self, query: &JsonValue);
	/// Returns query for `exec_*` functions
	fn build (&self) -> Box<dyn Any>;
    fn select (&mut self, projection: &Slice<Slice<u8>>);
    fn select_raw (&mut self, stmt: &str);
    fn query (&mut self, conditions: &JsonValue);
    fn limit (&mut self, offset: u32, length: u32);
    fn debug (&self);
}

pub struct EntityList {
    deinit: EntityDeinitFn,
    inner: Vec<EntityHandle>
}

impl EntityList {
    pub fn new (deinit: EntityDeinitFn) -> Self {
        EntityList { deinit, inner: Vec::new() }
    }

    pub fn push (&mut self, entity: EntityHandle) {
        self.inner.push(entity);
    }

    pub fn get_items (&self) -> Slice<EntityHandle> {
        return Slice::for_vec(&self.inner);
    }
}

impl Drop for EntityList {
    fn drop (&mut self) {
        for entity in &self.inner {
            (self.deinit)(*entity);
        }
    }
}

#[inline]
pub fn find_db_connection (key: &str) -> Option<&'static Box<dyn DatabaseConnection>> {
    return (rs_fn().find_db_connection)(key);
}

// TODO: DRY
#[allow(non_camel_case_types)]
pub type c_str = *const std::ffi::c_char;

pub unsafe fn c_string (ptr: c_str) -> String {
	use std::ptr::slice_from_raw_parts;
	let raw = &*slice_from_raw_parts(ptr as *const u8, c_strlen(ptr));
	return String::from_utf8_lossy(raw).into_owned();
}

pub fn c_strlen (raw: c_str) -> usize {
	if raw.is_null() {
        return 0;
    }

    let mut len = 0;
    unsafe {
        let mut ptr = raw;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
    }

    return len;
}
