const c = @import("./dcore.zig");
const std = @import("std");

pub export fn on_attach(router: *c.Router) void {
    router.register("/zip-zop", handleZipZop);
}

fn handleZipZop(_: *c.HttpContext) !*c.Response {
    const res_body = try c.ra.dupe(u8, "Рецепт вкусного чая...");

    var res = c.Response.init();
    res.set_code(c.HttpCode.ImTeapot);
    res.headers().set("content-type", "plain/tea");
    res.set_payload(res_body);

    return res;
}
