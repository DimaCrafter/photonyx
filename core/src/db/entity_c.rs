use bindings::{c::Slice, db::{EntityHandle, EntityList}};
use crate::c::c_deinit;


#[no_mangle]
pub extern "C" fn entity_list_get_items (list: &EntityList) -> Slice<EntityHandle> {
	return list.get_items();
}

#[no_mangle]
pub unsafe extern "C" fn entity_list_deinit (list: *mut EntityList) {
	c_deinit(list);
}