use std::ptr::null;
use bindings::db::{DatabaseConnection, ModelMeta, QueryBuilder};
use crate::{c::{c_init, c_str, c_string}, db::connection::DB_CONNECTIONS};


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
pub extern "C" fn db_connection_new_query (conn: &Box<dyn DatabaseConnection>, model: &ModelMeta) -> *mut Box<dyn QueryBuilder> {
	c_init(|| conn.new_query(&model.table))
}
