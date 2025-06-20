use std::{any::Any, ffi::{c_char, c_void}};
use json::JsonValue;

use crate::c::Slice;

pub type EntityHandle = *mut c_void;
pub type EntityInitFn = extern "C" fn () -> *mut c_void;
pub type EntityDeinitFn = extern "C" fn (*mut c_void);

#[repr(C)]
pub struct ModelFieldMeta {
    pub type_id: ModelFieldType,
    pub length: usize,
    pub optional: bool,
    pub enum_values: *const *const c_char,
    pub enum_size: usize
}

pub struct ModelField {
	pub name: String,
	pub meta: ModelFieldMeta,
	pub setter: EntitySetAnyField
}

#[repr(u8)]
pub enum ModelFieldType {
	Unknown,
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
    Array,
    Enum,
}

pub type EntitySetAnyField = extern "C" fn (entity: *mut c_void, value: *const c_void);

pub trait DatabaseConnection: Sync + Send {
	fn prepare_model (&mut self, model: &mut Box<dyn ModelMetaImpl>);

	fn new_query (&self, collection: &str) -> Box<dyn QueryBuilder>;
	fn exec_first (&mut self, model: &Box<dyn ModelMetaImpl>, query_any: Box<dyn Any>) -> Result<Option<EntityHandle>, String>;
    fn exec_all (&mut self, model: &Box<dyn ModelMetaImpl>, query_any: Box<dyn Any>) -> Result<EntityList, String>;
}

pub trait DatabaseImpl {
    fn connect (&self, raw_config: &JsonValue) -> Result<Box<dyn DatabaseConnection>, String>;
}

pub trait ModelMetaImpl {
    fn init_entity (&self) -> EntityHandle;
	fn init_entity_list (&self) -> EntityList;
	fn deinit_entity (&self, ptr: EntityHandle);
	fn get_name (&self) -> &str;

	fn add_field (&mut self, name: String, meta: ModelFieldMeta, setter: EntitySetAnyField);
    fn get_fields (&self) -> &Vec<ModelField>;
}

pub trait QueryBuilder {
	fn count (&mut self, query: &JsonValue);
	fn build (self: Box<Self>) -> Box<dyn Any>;
    fn select (&mut self, projection: &Slice<Slice<u8>>);
    fn query (&mut self, conditions: &JsonValue);
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
