#![feature(try_trait_v2)]

use json::object;

use dc_api_core::{app::{config::CONFIG, router::Router, App}, http::codes::HttpCode, validator_json, websocket::WebSocketEndpoints};


validator_json! {
	TestValidator,
	range_test: i32 as range(0, 100),
	sub: TestSubValidator as nested()
}

validator_json! {
	TestSubValidator,
	a: bool,
	enum_value: String as str_enum("Test", "MyEnumValue")
}



fn main () {
    let mut router = Router::empty();
    let mut ws = WebSocketEndpoints::empty();

    router.register("/test-endpoint/ctx".to_string(), |ctx| {
        let msg = format!("{:#?}", ctx);
        return ctx.text(&msg);
    });

    router.register("/test-endpoint/plus".to_string(), |ctx| {
        let payload = ctx.validate_json::<TestValidator>()?;
        println!("payload = {:?}", payload);

        return ctx.json(object! {
            result: 2 + 3,
            config_hello: CONFIG["hello"].clone()
        });
    });

    router.register("/test-endpoint/ip".to_string(), |ctx| {
        let msg = format!("{:?}", ctx.address);
        return ctx.text(&msg);
    });

    router.register("/test-endpoint/headers".to_string(), |ctx| {
        let hostname = ctx.get_header_default("host", "none".to_string());
        ctx.set_header("x-echo-host", hostname);
        return ctx.text("Check headers!");
    });

    router.register("/test-endpoint/{sup}-{sub}".to_string(), |ctx| {
        let msg = format!("{:?}", ctx.params);
        return ctx.text(&msg);
    });

    router.register("/test-endpoint/404".to_string(), |ctx| {
        return ctx.text_status("Nothing there!", HttpCode::NotFound);
    });

    router.register("/test-endpoint/redirect".to_string(), |ctx| {
        return ctx.redirect("./redirected");
    });

    ws.register("/socket", "test-event", |ctx| {
        ctx.text("reply", "Event handled!");
    });

    unsafe {
        let lib = libloading::Library::new("../zig-test/zig-out/lib/libzig_test.dylib").expect("no dylib");
        let func: libloading::Symbol<unsafe extern "C" fn (router: *mut Router)> = lib.get(b"on_attach").expect("no on_attach");
        func(&mut router);
    }

    dc_api_core::spawn_server(Box::leak(Box::new(App { router, ws_endpoints: ws })));
}
