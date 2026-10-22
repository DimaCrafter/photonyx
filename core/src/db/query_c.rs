use std::ptr::null_mut;
use bindings::{c::Slice, db::{DatabaseConnection, EntityHandle, EntityList, ModelMetaImpl, QueryBuilder}};
use json::JsonValue;
use crate::c::{c_init, c_unwrap};


#[no_mangle]
pub extern "C" fn query_builder_select (query: &mut Box<dyn QueryBuilder>, projection: &Slice<Slice<u8>>) {
	query.select(projection);
}

#[no_mangle]
pub extern "C" fn query_builder_where (query: &mut Box<dyn QueryBuilder>, conditions: &JsonValue) {
	query.query(conditions);
}

#[no_mangle]
pub extern "C" fn query_builder_debug (query: &mut Box<dyn QueryBuilder>) {
	query.debug();
}

#[no_mangle]
pub unsafe extern "C" fn db_connection_exec_first (
	conn: &mut Box<dyn DatabaseConnection>,
	model: &Box<dyn ModelMetaImpl>,
	query_ptr: *mut Box<dyn QueryBuilder>
) -> EntityHandle {
	let query_any = c_unwrap(query_ptr).build();
	match conn.exec_first(model, query_any) {
		Ok(value_opt) => match value_opt {
			Some(value) => return value,
			None => return null_mut()
		}
		Err(error) => {
			println!("Query failed: {error}");
			return null_mut();
		}
	};
}

#[no_mangle]
pub unsafe extern "C" fn db_connection_exec_all (
	conn: &mut Box<dyn DatabaseConnection>,
	model: &Box<dyn ModelMetaImpl>,
	query_ptr: *mut Box<dyn QueryBuilder>
) -> *mut EntityList {
	let query_any = c_unwrap(query_ptr).build();
	match conn.exec_all(model, query_any) {
		Ok(result) => {
			return c_init(|| result);
		}
		Err(error) => {
			println!("Query failed: {error}");
			return c_init(|| model.init_entity_list());
		}
	};
}

#[no_mangle]
pub unsafe extern "C" fn db_connection_count(
	conn: &mut Box<dyn DatabaseConnection>,
	model: &Box<dyn ModelMetaImpl>,
	conditions: &JsonValue
) -> u32 {
	let mut query = conn.new_query(model.get_name());
	query.count(conditions);

	let query_any = query.build();
	let res = match conn.exec_value(query_any) {
		Ok(res) => res,
		Err(error) => {
			// todo: propagate errors
			println!("Query failed: {error}");
			return 0;
		}
	};

	let value_any = match res {
		Some(value_any) => value_any,
		None => return 0
	};

	if let Some(value) = value_any.downcast_ref::<i64>() {
		return *value as u32;
	}

	if let Some(value) = value_any.downcast_ref::<u64>() {
		return *value as u32;
	}

	return 0;
}

#[no_mangle]
pub unsafe extern "C" fn db_connection_exists(
	conn: &mut Box<dyn DatabaseConnection>,
	model: &Box<dyn ModelMetaImpl>,
	conditions: &JsonValue
) -> bool {
	let mut query = conn.new_query(model.get_name());
	query.select_raw("1");
	query.query(conditions);

	let query_any = query.build();
	match conn.exec_value(query_any) {
		Ok(res) => {
			return res.is_some();
		},
		Err(error) => {
			// todo: propagate errors
			println!("Query failed: {error}");
			return false;
		}
	}
}
