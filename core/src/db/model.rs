use std::sync::{Arc, Mutex};
use photonyx_models::runtime::RuntimeModels;


pub struct AttachContext {
	pub module: String,
	pub models: Option<Arc<RuntimeModels>>
}

static ATTACH_CONTEXT: Mutex<Option<AttachContext>> = Mutex::new(None);

pub fn with_attach_context<R, F: FnOnce () -> R> (context: AttachContext, call: F) -> R {
	*ATTACH_CONTEXT.lock().unwrap() = Some(context);
	let result = call();
	*ATTACH_CONTEXT.lock().unwrap() = None;

	return result;
}

pub(crate) fn current_attach_context<R, F: FnOnce (Option<&AttachContext>) -> R> (call: F) -> R {
	let guard = ATTACH_CONTEXT.lock().unwrap();
	return call(guard.as_ref());
}
