use std::ffi::c_void;
use bindings::{c::Slice, db::{EntityDeinitFn, EntityInitFn, EntityList, EntitySetAnyField, ModelField, ModelFieldMeta, ModelMetaImpl}};
use crate::c::{c_str, c_string};


pub struct ModelMeta {
	pub origin_module: Option<String>,
	pub name: String,
	pub init: EntityInitFn,
	pub deinit: EntityDeinitFn,
	pub fields: Vec<ModelField>,
	pub unique_keys: Vec<Vec<String>>
}

impl ModelMetaImpl for ModelMeta {
	fn init_entity (&self) -> *mut c_void {
		return (self.init)();
	}

	fn init_entity_list (&self) -> EntityList {
		return EntityList::new(self.deinit);
	}

	fn deinit_entity (&self, ptr: *mut c_void) {
		(self.deinit)(ptr);
	}

	fn get_name (&self) -> &str {
		return &self.name;
	}

	fn add_field (&mut self, name: String, setter: EntitySetAnyField) -> &mut ModelField {
		self.fields.push(ModelField { name, meta: ModelFieldMeta::new(), setter });
		return self.fields.last_mut().unwrap();
	}

	fn get_fields (&self) -> &Vec<ModelField> {
		return &self.fields;
	}

	fn add_unique_key (&mut self, compound: &Slice<Slice<u8>>) {
		self.unique_keys.push(
			compound
				.into_iter()
				.map(|field| unsafe { field.as_str_unchecked() }.to_owned())
				.collect()
		);
	}

	fn get_unique_keys (&self) -> &Vec<Vec<String>> {
		return &self.unique_keys;
	}
}
