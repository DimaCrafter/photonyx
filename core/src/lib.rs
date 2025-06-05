#![feature(try_trait_v2)]

use app::App;

pub mod app;
pub mod http;
pub mod http1;
pub mod websocket;
pub mod context;
pub mod utils;
pub(crate) mod c;

pub extern crate dc_macro;

pub fn spawn_server (app: &'static App) {
    app::server::start_server(app);
}
