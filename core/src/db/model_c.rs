use bindings::{c::Slice, db::{ModelMetaImpl, ModelFieldMeta, ModelFieldConstraint, LinkPolicy, EntitySetAnyField}, camel_to_kebab};
use crate::c::{c_str, c_string};


#[no_mangle]
pub unsafe extern "C" fn model_meta_add_field (
	model: &mut Box<dyn ModelMetaImpl>,
	name: c_str,
	setter: EntitySetAnyField
) -> *mut ModelFieldMeta {
	return &mut model.add_field(c_string(name), setter).meta;
}

#[no_mangle]
pub extern "C" fn model_meta_add_unique_key (model: &mut Box<dyn ModelMetaImpl>, compound: &Slice<Slice<u8>>) {
	model.add_unique_key(compound);
}

#[no_mangle]
pub extern "C" fn model_field_meta_add_length (field_meta: &mut ModelFieldMeta, value: u32) {
	field_meta.constraints.push(ModelFieldConstraint::Length { value });
}

#[no_mangle]
pub extern "C" fn model_field_meta_add_primary_key (field_meta: &mut ModelFieldMeta) {
	field_meta.constraints.push(ModelFieldConstraint::PrimaryKey);
}

#[no_mangle]
pub unsafe extern "C" fn model_field_meta_add_link (
	field_meta: &mut ModelFieldMeta,
	dst_collection: c_str,
	dst_column: c_str,
	delete: LinkPolicy,
	update: LinkPolicy
) {
	field_meta.constraints.push(ModelFieldConstraint::Link {
		dst_table: get_table_name(&c_string(dst_collection)),
		dst_column: c_string(dst_column),
		delete,
		update
	});
}

// TODO: DRY
pub fn get_table_name (collection: &str) -> String {
	let mut table_name = camel_to_kebab(collection);
	if table_name.ends_with('s') {
		table_name.push_str("es");
	} else {
		table_name.push_str("s");
	}

	return table_name;
}
