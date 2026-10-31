use json::JsonValue;

pub trait SessionStoreMeta: Sync + Send {
	fn get_id (&self) -> String;
	fn new_impl (&self, arg: &str) -> Option<Box<dyn SessionStoreImpl>>;
}

pub trait SessionStoreImpl: Sync + Send {
	/// Stores a new session, returns its id
	fn create (&self, data: &JsonValue, expires: u64) -> Result<u64, String>;
	/// Returns data of a session that exists and has not expired
	fn load (&self, id: u64) -> Result<Option<JsonValue>, String>;
	fn update (&self, id: u64, data: &JsonValue, expires: u64) -> Result<(), String>;
	fn destroy (&self, id: u64) -> Result<(), String>;
}
