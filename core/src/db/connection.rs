use std::{collections::HashMap, sync::OnceLock};
use bindings::db::DatabaseConnection;


pub(crate) static DB_CONNECTIONS: OnceLock<DatabaseConnections> = OnceLock::new();

pub struct DatabaseConnections {
	map: HashMap<String, Box<dyn DatabaseConnection>>
}

impl DatabaseConnections {
	pub fn new () -> Self {
		DatabaseConnections { map: HashMap::new() }
	}

	pub fn register (&mut self, key: String, value: Box<dyn DatabaseConnection>) {
		self.map.insert(key, value);
	}

	pub fn find (&self, key: &str) -> Option<&Box<dyn DatabaseConnection>> {
		return self.map.get(key);
	}
}

pub fn init_database_connections_store (map: DatabaseConnections) {
	DB_CONNECTIONS.get_or_init(|| map);
}
