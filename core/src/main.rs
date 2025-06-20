#![feature(try_trait_v2)]

use std::process;
use app::App;
use crate::{app::{config::CONFIG, modules::load_modules}, db::connection::{init_database_connections_store, DatabaseConnections}, utils::log::{log_error, log_info}};

pub mod app;
pub mod http;
pub mod http1;
pub mod websocket;
pub mod context;
pub mod db;
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

    // stage 1 - loading database providers
    let mut db_connections = DatabaseConnections::new();

    for module in &app.modules {
        if let Some(database) = module.provide_database() {
            let cfg = &CONFIG["db"]["primary"];
            match database.connect(cfg) {
                Err(error) => {
                    println!("failed to create connection: {}", error);
                }
                Ok(conn) => {
                    // todo! un-hardcode     VVVVVVV
                    db_connections.register("primary".to_owned(), conn);
                }
            }
        }
    }

    init_database_connections_store(db_connections);

    // stage 2 - loading controllers
    for module in &app.modules {
        module.provide_models();
        module.provide_routes(&mut app.router);
    }

    app::server::start_server(app);
}
