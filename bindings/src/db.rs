use std::{any::Any, ffi::{c_char, c_void}, ptr::null};
use json::JsonValue;
use crate::{c::Slice, rs::rs_fn};


pub type EntityHandle = *mut c_void;
pub type EntityInitFn = extern "C" fn () -> *mut c_void;
pub type EntityDeinitFn = extern "C" fn (*mut c_void);

#[repr(C)]
pub struct ModelFieldMeta {
    pub type_id: ModelFieldType,
    pub length: usize,
    pub optional: bool,
    pub enum_values: *const *const c_char,
    pub enum_size: usize,
    pub constraints: Vec<ModelFieldConstraint>
}

impl ModelFieldMeta {
    pub fn new () -> Self {
        ModelFieldMeta {
            type_id: ModelFieldType::Unknown,
            length: 0,
            optional: false,
            enum_values: null(),
            enum_size: 0,
            constraints: Vec::new()
        }
    }

    pub fn iter_enum (&self) -> ModelFieldEnumIter {
        ModelFieldEnumIter {
            values: self.enum_values,
            size: self.enum_size,
            step: self.length,
            offset: 0
        }
    }
}

pub struct ModelFieldEnumIter {
    values: *const *const c_char,
    size: usize,
    step: usize,
    offset: usize
}

impl Iterator for ModelFieldEnumIter {
    type Item = String;

    fn next (&mut self) -> Option<Self::Item> {
        if self.offset == self.size {
            return None;
        }

        let ptr = unsafe { self.values.offset(self.offset as isize) };
        self.offset += self.step;

        return Some(unsafe { c_string(*ptr) });
    }
}

pub enum ModelFieldConstraint {
    Length { value: u32 },
    PrimaryKey,
    Link {
        dst_table: String,
        dst_column: String,
        delete: LinkPolicy,
        update: LinkPolicy
    }
}

#[derive(Debug)]
#[repr(u8)]
pub enum LinkPolicy {
    /// Link will be used just as key
    None,
    /// Service.user_id --> User: operation on User will be restricted if Service exist
    Restrict,
    /// Service.user_id --> User: operation on User will cause some operation on Service
    Cascade,
    /// Service.user_id --> User: operation on User will set Service.user_id to null
    SetNull
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
    Enum,
    DateTime
}

pub type EntitySetAnyField = extern "C" fn (entity: *mut c_void, value: *const c_void);

pub trait DatabaseConnection: Sync + Send {
	fn auto_migrate (&mut self, model: &mut Box<dyn ModelMetaImpl>);
    fn as_any (&self) -> &dyn Any;

	fn new_query (&self, collection: &str) -> Box<dyn QueryBuilder>;
	fn exec_value (&mut self, query_any: Box<dyn Any>) -> Result<Option<Box<dyn Any>>, String>;
	fn exec_first (&mut self, model: &Box<dyn ModelMetaImpl>, query_any: Box<dyn Any>) -> Result<Option<EntityHandle>, String>;
    fn exec_all (&mut self, model: &Box<dyn ModelMetaImpl>, query_any: Box<dyn Any>) -> Result<EntityList, String>;
}

pub trait DatabaseImpl {
    fn get_id (&self) -> String;
    fn connect (&self, raw_config: &JsonValue) -> Result<Box<dyn DatabaseConnection>, String>;
}

pub trait ModelMetaImpl {
    fn init_entity (&self) -> EntityHandle;
	fn init_entity_list (&self) -> EntityList;
	fn deinit_entity (&self, ptr: EntityHandle);
	fn get_name (&self) -> &str;

	fn add_field (&mut self, name: String, setter: EntitySetAnyField) -> &mut ModelField;
    fn get_fields (&self) -> &Vec<ModelField>;

    fn add_unique_key (&mut self, compound: &Slice<Slice<u8>>);
    fn get_unique_keys (&self) -> &Vec<Vec<String>>;
}

pub trait QueryBuilder {
	fn count (&mut self, query: &JsonValue);
	fn build (self: Box<Self>) -> Box<dyn Any>;
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
