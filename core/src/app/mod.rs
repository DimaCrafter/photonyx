use crate::{app::modules::Module, websocket::WebSocketEndpoints};
use self::router::Router;

pub mod config;
pub mod modules;
pub mod server;
pub mod router;
pub mod router_c;


pub struct App {
	pub ws_endpoints: WebSocketEndpoints,
	pub router: Router,
	pub modules: Vec<Module>
}

impl App {
	pub fn new () -> Self {
		App {
			ws_endpoints: WebSocketEndpoints::empty(),
			router: Router::empty(),
			modules: Vec::new()
		}
	}
}
