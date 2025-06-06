# Photonyx (WIP)

Simple API core for your projects. Photonyx is a [dc-api-core](https://github.com/DimaCrafter/dc-api-core) successor.

Written on Rust it can on any C compatible language. You can develop extensible commercial back-ends with all pros
of system languages.

## Concept

Main process (core) is the Photonyx itself. Core responsible for accepting HTTP requests, parsing, routing an so on.
Request handlers is being registered by modules, which is just dynamic library file. All modules are loaded on startup,
and they can hook `on_attach` event to register routes they want, initialize their state or to do anything else.

Here comes the extensibility I talk about. You can write basic service and other programmer can add to it new functionality
without touching your code by implementing new module on a language this person prefer. At the same time your code still
available only as a compiled library, and the only way to get it's code - decompile it.

<!-- TODO: quick start (onyx + manual) -->

## Bindings

| Language | Repo                                          |
|----------|-----------------------------------------------|
| Rust     | -                                             |
| C/C++    | -                                             |
| Zig      | <https://github.com/DimaCrafter/photonyx.zig> |

## External interface

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
