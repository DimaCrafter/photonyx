const std = @import("std");
const Allocator = std.mem.Allocator;
const Alignment = std.mem.Alignment;

extern fn r_alloc(size: usize, alignment: usize) callconv(.C) ?[*]u8;
extern fn r_realloc(ptr: [*]u8, old_size: usize, new_size: usize, alignment: usize) callconv(.C) ?[*]u8;
extern fn r_dealloc(ptr: [*]u8, size: usize, alignment: usize) callconv(.C) void;

const RAllocator = struct {
    const vtable: Allocator.VTable = .{
        .alloc = alloc,
        .resize = resize,
        .remap = remap,
        .free = free,
    };

    fn alloc(_: *anyopaque, size: usize, alignment: Alignment, _: usize) ?[*]u8 {
        return r_alloc(size, alignment.toByteUnits());
    }

    fn resize(_: *anyopaque, _: []u8, _: Alignment, _: usize, _: usize) bool {
        return false;
    }

    fn remap(_: *anyopaque, memory: []u8, alignment: Alignment, new_size: usize, _: usize) ?[*]u8 {
        return r_realloc(memory.ptr, memory.len, new_size, alignment.toByteUnits());
    }

    fn free(_: *anyopaque, memory: []u8, alignment: Alignment, _: usize) void {
        r_dealloc(memory.ptr, memory.len, alignment.toByteUnits());
    }
};

pub const ra: Allocator = .{
    .ptr = undefined,
    .vtable = &RAllocator.vtable,
};

pub fn convertStr(zigString: []const u8) ![]u8 {
    const cRaString = try ra.alloc(u8, zigString.len + 1);

    for (0..zigString.len) |i| {
        cRaString[i] = zigString[i];
    }

    cRaString[zigString.len] = 0;

    return cRaString;
}

pub const HttpCode = enum(isize) {
    NotSent,
    Continue = 100,
    SwitchingProtocols = 101,
    Processing = 102,
    EarlyHints = 103,
    OK = 200,
    Created = 201,
    Accepted = 202,
    NonAuthoritativeInformation = 203,
    NoContent = 204,
    ResetContent = 205,
    PartialContent = 206,
    MultiStatus = 207,
    AlreadyReported = 208,
    ImUsed = 226,
    MultipleChoices = 300,
    MovedPermanently = 301,
    Found = 302,
    SeeOther = 303,
    NotModified = 304,
    UseProxy = 305,
    SwitchProxy = 306,
    TemporaryRedirect = 307,
    PermanentRedirect = 308,
    BadRequest = 400,
    Unauthorized = 401,
    PaymentRequired = 402,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    NotAcceptable = 406,
    ProxyAuthenticationRequired = 407,
    RequestTimeout = 408,
    Conflict = 409,
    Gone = 410,
    LengthRequired = 411,
    PreconditionFailed = 412,
    RequestEntityTooLarge = 413,
    URITooLong = 414,
    UnsupportedMediaType = 415,
    RangeNotSatisfiable = 416,
    ExpectationFailed = 417,
    ImTeapot = 418,
    MisdirectedRequest = 421,
    UnprocessableEntity = 422,
    Locked = 423,
    FailedDependency = 424,
    TooEarly = 425,
    UpgradeRequired = 426,
    PreconditionRequired = 428,
    TooManyRequests = 429,
    RequestHeaderFieldsTooLarge = 431,
    UnavailableForLegalReasons = 451,
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
    HTTPVersionNotSupported = 505,
    VariantAlsoNegotiates = 506,
    InsufficientStorage = 507,
    LoopDetected = 508,
    NotExtended = 510,
    NetworkAuthenticationRequired = 511,
};

pub const HttpContext = opaque {};

pub const HttpHeaders = opaque {
    pub inline fn set(self: *HttpHeaders, name: c_str, value: c_str) void {
        http_headers_set(self, name, value);
    }

    pub inline fn set_default(self: *HttpHeaders, name: c_str, value: c_str) void {
        http_headers_set_default(self, name, value);
    }

    pub inline fn set_normal(self: *HttpHeaders, name: c_str, value: c_str) void {
        http_headers_set_normal(self, name, value);
    }
};

extern fn response_new() callconv(.C) *Response;
extern fn response_set_code(res: *Response, code: HttpCode) callconv(.C) void;
extern fn response_headers(res: *Response) callconv(.C) *HttpHeaders;
extern fn response_set_drop(res: *Response) callconv(.C) void;
extern fn response_set_payload(res: *Response, r_ptr: [*]u8, size: usize) callconv(.C) void;
extern fn response_drop(res: *Response) callconv(.C) void;

pub const Response = opaque {
    pub inline fn init() *Response {
        return response_new();
    }

    pub inline fn set_code(self: *Response, code: HttpCode) void {
        return response_set_code(self, code);
    }

    pub inline fn headers(self: *Response) *HttpHeaders {
        return response_headers(self);
    }

    pub inline fn set_drop(self: *Response) void {
        response_set_drop(self);
    }

    pub inline fn set_payload(self: *Response, r_payload: []u8) void {
        response_set_payload(self, r_payload.ptr, r_payload.len);
    }

    pub inline fn deinit(self: *Response) void {
        response_drop(self);
    }
};

pub const ActionHandler = *const fn (*HttpContext) anyerror!?*Response;
pub const Router = opaque {
    // pub inline fn init() *Router {
    //     return router_new();
    // }

    pub inline fn register(self: *Router, pattern: c_str, action: ActionHandler) void {
        const handler = struct {
            fn call(ctx: *HttpContext) callconv(.C) ?*Response {
                const res = action(ctx) catch |err| {
                    std.log.err("Route {s} returned error: {}", .{ pattern, err });
                    // todo? return 500 error
                    return null;
                };

                return res;
            }
        };

        router_register(self, pattern, handler.call);
    }

    // pub inline fn deinit(self: *Router) void {
    //     router_drop(self);
    // }
};

const c_str = [*:0]const u8;

// extern fn router_new() *Router;

const ActionHandlerC = *const fn (*HttpContext) callconv(.C) ?*Response;
extern fn router_register(router: *Router, pattern: c_str, action: ActionHandlerC) callconv(.C) void;

// extern fn router_drop(router: *Router) void;

extern fn http_headers_set(headers: *HttpHeaders, name: c_str, value: c_str) callconv(.C) void;

extern fn http_headers_set_default(headers: *HttpHeaders, name: c_str, value: c_str) callconv(.C) void;

extern fn http_headers_set_normal(headers: *HttpHeaders, name: c_str, value: c_str) callconv(.C) void;
