# dc-api-core.rs

Branch: pure-sync

This branch contains thread pool based HTTP/WS server without NAPI.

C API naming convention:

- `get_{name}` - get global instance (`&'static`),
- `{struct name}_get_{field}` - get field value,
- `{struct name}_set_{field}` - set field value,
- `{struct name}_{field}` - get field pointer,
- `{struct name}_{method}` - struct method (first argument - `&mut self`).

## TODOs

- [ ] Hardware dependent pool size
- [x] CORS
  - [x] Policy configuration
- [ ] URL-encoded form support
- [ ] Multipart form support
- [ ] JSON support
- [ ] HttpContext
  - [ ] query
  - [ ] data
  - [ ] session
- [ ] SocketContext
  - [ ] end
  - [ ] subscribe/unsubscribe
  - [ ] broadcast
- [x] Config module
  - [ ] Development mode with config overriding
- [ ] Database plugins support
- [x] Log module
