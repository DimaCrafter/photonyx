use json::JsonValue;

pub trait SessionStoreMeta: Sync + Send {
	fn get_id (&self) -> String;
	fn new_impl (&self, arg: &str) -> Option<Box<dyn SessionStoreImpl>>;
}

pub trait SessionStoreImpl: Sync + Send {
	fn create (&self, inner: &mut json::JsonValue, expires: f64);
	fn update (&self, inner: &mut json::JsonValue);
	fn destroy (&self, data: &mut JsonValue);
}
