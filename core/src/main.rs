#![feature(try_trait_v2)]
#![feature(duration_constructors)]

use std::{collections::HashMap, process};
use app::App;
use crate::{app::{config::CONFIG, modules::load_modules}, context::session::init_session_store, db::connection::{init_database_connections_store, DatabaseConnections}, utils::log::{log_error, log_info}};

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

    // stage 1 - loading database and session store providers
    let mut db_connections = DatabaseConnections::new();
    let mut session_stores = HashMap::new();

    for module in &app.modules {
        if let Some(database) = module.provide_database() {
            let db_id = database.get_id();
            for (db_conf_name, db_conf) in CONFIG["db"].entries() {
                if !matches!(db_conf["driver"].as_str(), Some(db_conf_driver) if db_conf_driver == db_id) {
                    continue;
                }

                match database.connect(db_conf) {
                    Err(error) => {
                        println!("failed to create connection: {}", error);
                    }
                    Ok(conn) => {
                        db_connections.register(db_conf_name.to_owned(), conn);
                    }
                }
            }
        }

        if let Some(session_store) = module.provide_session_store() {
            session_stores.insert(session_store.get_id(), session_store);
        }
    }

    init_database_connections_store(db_connections);

    if let Some(ref session_cfg) = CONFIG.session {
        if let Some(store) = session_stores.remove(&session_cfg.store) {
            if let Some(store_impl) = store.new_impl(&session_cfg.store_arg) {
                init_session_store(store_impl);
            } else {
                log_error("Failed to initialize session store");
                process::exit(-1);
            }
        } else {
            log_error(&format!("Session store with identifier `{}` was not registered!", &session_cfg.store));
            process::exit(-1);
        }
    }

    // stage 2 - loading controllers
    for module in &app.modules {
        module.provide_models();
        module.provide_routes(&mut app.router);
    }

    // stage 3
    for module in &app.modules {
        module.startup();
    }

    app::server::start_server(app);
}
