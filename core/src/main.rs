#![feature(try_trait_v2)]

use std::process;

use app::App;

use crate::{app::modules::load_modules, utils::log::{log_error, log_info}};

pub mod app;
pub mod http;
pub mod http1;
pub mod websocket;
pub mod context;
pub mod utils;
pub(crate) mod c;

pub extern crate photonyx_macro;

pub fn main () {
    let app = Box::leak(Box::new(App::new()));

    if let Err(error) = load_modules(&mut app.modules, "modules") {
        log_error(&format!("Failed to load modules: {error}"));
        process::exit(-1);
    }

    log_info(&format!("Loaded modules: {}", app.modules.len()));

    for module in &app.modules {
        module.with_on_attach(|on_attach| {
            if let Some(call) = on_attach {
                log_info(&format!("{}: calling on_attach", module.get_name()));
                app.router.with_module(module.get_name(), |router| call(router));
            }
        });
    }

    app::server::start_server(app);
}
