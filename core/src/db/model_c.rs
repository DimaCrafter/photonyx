use std::ptr::null_mut;
use bindings::db::{EntityDeinitFn, EntityInitFn, EntitySetAnyField, ModelMeta};
use crate::{app::config::CONFIG, c::{c_slice, c_str, c_string}, db::{connection::DB_CONNECTIONS, model::current_attach_context}, utils::log::{log_error, log_info}};


#[no_mangle]
pub unsafe extern "C" fn model_attach (
	db: c_str,
	model: c_str,
	init: EntityInitFn,
	deinit: EntityDeinitFn,
	setters: *const EntitySetAnyField,
	setters_len: usize
) -> *mut ModelMeta {
	let db = c_string(db);
	let model = c_string(model);
	let setters = c_slice(setters, setters_len);

	let result = current_attach_context(|context| {
		let context = context.ok_or_else(|| format!("model {db}.{model}: model_attach must be called from provide_models"))?;
		let module = &context.module;

		let models = context.models.as_ref().ok_or_else(|| format!("{module}: model {db}.{model} attached, but the module does not export models_schema"))?;
		let meta = ModelMeta::from_runtime(models, &db, &model, init, deinit, setters).map_err(|error| format!("{module}: {error}"))?;

		let has_connection = DB_CONNECTIONS.get().is_some_and(|store| store.find(&db).is_some());
		if !has_connection {
			let reason = if CONFIG["db"][db.as_str()].is_object() {
				"failed to connect to it at start (see above)"
			} else {
				"it is not configured in `db` of the config"
			};

			return Err(format!("{module}: model {db}.{model} is bound to database '{db}', but {reason}"));
		}

		log_info(&format!("{module}: attached model {db}.{model} (table {}, {} columns)", &meta.table, meta.fields.len()));
		return Ok(meta);
	});

	match result {
		Ok(meta) => return Box::into_raw(Box::new(meta)),
		Err(error) => {
			log_error(&error);
			return null_mut();
		}
	}
}
