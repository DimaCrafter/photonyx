use std::sync::OnceLock;
use crate::db::DatabaseConnection;


static FN_LINKS: OnceLock<&'static RustFnLinks> = OnceLock::new();

pub struct RustFnLinks {
	pub find_db_connection: fn (key: &str) -> Option<&'static Box<dyn DatabaseConnection>>
}

#[unsafe(no_mangle)]
extern "C" fn rs_link (links: &'static RustFnLinks) {
	FN_LINKS.get_or_init(|| links);
}

#[inline]
pub fn rs_fn () -> &'static RustFnLinks {
	return FN_LINKS.get().unwrap();
}
