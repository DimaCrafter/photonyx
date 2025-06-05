use std::io::Error;
use std::net::{SocketAddr, TcpListener, TcpStream};
use threadpool::ThreadPool;

use crate::app::config::CONFIG;
use crate::http::cors::Cors;
use crate::utils::log::*;
use super::App;
use crate::context::http::HttpContext;
use crate::context::ws::SocketContext;
use crate::http::{entity::*, codes::HttpCode};
use crate::http1::{Http1Engine, Http1Connection};
use crate::websocket::{websocket_handshake, HandshakeResult, maintain_websocket};


pub fn start_server (app: &'static App) {
    let bind_address = format!("{}:{}", CONFIG.host, CONFIG.port);

	match TcpListener::bind(&bind_address) {
        Ok(listener) => {
            let pool = ThreadPool::new(32);
			log_success(&format!("Listening on {}", bind_address));

			loop {
				let socket = listener.accept().unwrap();
				pool.execute(move || proceed_connection::<Http1Engine, Http1Connection>(app, socket));
			}
		}
		Err(error) => {
            log_error_lines("Listen error", error.to_string());
		}
	}
}

fn proceed_connection<Http: HttpEngine<Connection> + Send, Connection: HttpConnection>
(app: &App, socket: (TcpStream, SocketAddr)) {
    let mut connection = Http::handle_connection(socket);

    match connection.parse() {
        ParsingResult::Complete(req) => {
            if is_connection_upgrade(&req) {
                if is_websocket_upgrade(&req) {
                    proceed_websocket::<Connection>(app, connection, req);
                } else {
                    let _ = connection.respond(Response::from_status(HttpCode::BadRequest));
                }
            } else {
                let _ = proceed_http::<Connection>(app, connection, req);
            }
        }
        ParsingResult::Partial => {}
        ParsingResult::Error(res_code) => {
            let _ = connection.respond(Response::from_status(res_code));
            let _ = connection.disconnect();
        }
        ParsingResult::Invalid => {
            let _ = connection.disconnect();
        }
    }
}

fn proceed_http<Connection: HttpConnection> (app: &App, mut connection: Connection, req: Request) -> Result<(), Error> {
    let mut res;
    let cors = Cors::new(&req);

    if let HttpMethod::OPTIONS = req.method {
        res = Response::from_status(HttpCode::OK);
        cors.apply_preflight(&mut res);
    } else if let Some((endpoint, params)) = app.router.match_path(&req.path) {
        let mut ctx = HttpContext::from(&connection, req, params);
        match (endpoint.call)(&mut ctx) {
            ResponseRet::Replace(response) => {
                res = response;
            },
            _ => {
                res = ctx.res;
            }
        }
        cors.apply_normal(&mut res);
    } else {
        res = Response::from_code(HttpCode::NotFound, "API endpoint not found");
        cors.apply_normal(&mut res);
    }

    connection.respond(res)?;
    return connection.disconnect();
}

fn proceed_websocket<Connection: HttpConnection> (app: &App, mut connection: Connection, req: Request) {
    match websocket_handshake(app, &req) {
        HandshakeResult::Ok(endpoint_index, res) => {
            // todo: handle all `let _ = ...`
            let _ = connection.respond(res);
            let ctx = SocketContext::from::<Connection>(connection, req);
            let _ = maintain_websocket(app, ctx, endpoint_index);
        }
        HandshakeResult::Err(res) => {
            let _ = connection.respond(res);
            let _ = connection.disconnect();
        }
    }
}

fn is_connection_upgrade (req: &Request) -> bool {
    matches!(req.headers.get("connection"), Some(value) if value == "Upgrade")
}

fn is_websocket_upgrade (req: &Request) -> bool {
    matches!(req.headers.get("upgrade"), Some(value) if value == "websocket")
}
