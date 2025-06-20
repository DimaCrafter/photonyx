use std::ptr::null;
use bindings::db::{DatabaseConnection, EntityDeinitFn, EntityInitFn, ModelMetaImpl, QueryBuilder};
use crate::{c::{c_init, c_str, c_string}, db::{connection::DB_CONNECTIONS, model::ModelMeta}, utils::log::log_info};


#[no_mangle]
pub unsafe extern "C" fn find_db_connection (id: c_str) -> *const Box<dyn DatabaseConnection> {
	if let Some(store) = DB_CONNECTIONS.get() {
		if let Some(conn) = store.find(&c_string(id)) {
			return conn;
		} else {
			return null();
		}
	} else {
		return null();
	}
}

#[no_mangle]
pub unsafe extern "C" fn db_connection_prepare_model (
	conn: &mut Box<dyn DatabaseConnection>,
	name: c_str,
	init: EntityInitFn,
	deinit: EntityDeinitFn
) -> *mut Box<dyn ModelMetaImpl> {
	let name = c_string(name);

	// todo: track what module registered the model
	let reg_msg = format!("registered model {} {{ init = {:p}, deinit = {:p} }}", &name, init, deinit);
	log_info(&reg_msg);

	let model = ModelMeta { origin_module: None, name, init, deinit, fields: Vec::new() };
	let mut boxed: Box<dyn ModelMetaImpl> = Box::new(model);
	conn.prepare_model(&mut boxed);

	return Box::into_raw(boxed.into());
}

#[no_mangle]
pub extern "C" fn db_connection_new_query (
	conn: &Box<dyn DatabaseConnection>,
	model: &Box<dyn ModelMetaImpl>
) -> *mut Box<dyn QueryBuilder> {
	c_init(|| conn.new_query(model.get_name()))
}
