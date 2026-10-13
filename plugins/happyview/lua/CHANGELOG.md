## [1.1.0](https://github.com/happyproto/plugins/releases/tag/happyview-lua-v1.1.0) (2026-10-10)

- **Library calls can run together.** The new `internal.async` module has one function, `all(f1, f2, ...)`. Library calls made inside the functions you hand it are started instead of waited on, so a database read and an HTTP request cost the slower of the two instead of both. Results come back in argument order, and a call that fails raises inside its own function, where `pcall` can catch it.
- **Tracebacks name the library bridge.** Every library call passes through the same shim, so its frame showed up as `internal.async` in library errors even for scripts that never touched it. It's labelled `[library bridge]` now.

## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-lua-v1.0.0) (2026-10-07)

- **First release on HappyView v3's plugin API.** Lua 5.4.8 moved out of the HappyView binary and into this plugin: the `handle(input, ctx)` contract, `require` for installed libraries, the `internal.json`, `internal.time`, `internal.tids` and `internal.logging` modules, the sandbox, and the instruction and memory budgets.
