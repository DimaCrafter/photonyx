use std::ffi::c_void;
use bindings::db::{EntityDeinitFn, EntityInitFn, EntityList, EntitySetAnyField, ModelField, ModelFieldMeta, ModelMetaImpl};
use crate::c::{c_str, c_string};


pub struct ModelMeta {
	pub origin_module: Option<String>,
	pub name: String,
	pub init: EntityInitFn,
	pub deinit: EntityDeinitFn,
	pub fields: Vec<ModelField>
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

	fn add_field (&mut self, name: String, meta: ModelFieldMeta, setter: EntitySetAnyField) {
		// if let ModelFieldType::Enum = meta.type_id {
		// 	println!("add_field {name} = enum, {}", meta.length);
		// 	for i in 0..meta.enum_size {
		// 		let ptr = unsafe { meta.enum_values.offset(i as isize) };
		// 		let name = unsafe { c_string(*ptr) };
		// 		println!("- {name}");
		// 	}
		// }
		self.fields.push(ModelField { name, meta, setter });
	}

	fn get_fields (&self) -> &Vec<ModelField> {
		return &self.fields;
	}
}

#[no_mangle]
pub unsafe extern "C" fn model_meta_add_field (
	model: &mut Box<dyn ModelMetaImpl>,
	name: c_str, meta: ModelFieldMeta,
	setter: EntitySetAnyField
) {
	model.add_field(c_string(name), meta, setter);
}
