## [1.0.1](https://github.com/happyproto/plugins/releases/tag/happyview-javascript-v1.0.1) (2026-10-10)

- **Runaway recursion is a `RangeError` you can catch.** In 1.0.0 a function that recursed forever ran the host out of stack and trapped the whole run. Now the engine gives scripts 384 KiB of QuickJS stack and throws `RangeError: Maximum call stack size exceeded`, with its line, before that happens. Upstream QuickJS-ng compiles this check out under WASI, so the plugin is built from a fork that keeps it. On a host that gives a guest 16 MiB of native stack, as HappyView does, the check fires before the native stack runs out.

## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-javascript-v1.0.0) (2026-10-10)

- **First release on HappyView v3's plugin API.** JavaScript scripts run on QuickJS-ng, as ES modules with a default-export `handle(input, ctx)`, `import` for installed libraries, the `internal.json`, `internal.time`, `internal.tids` and `internal.logging` modules, `console`, and instruction and memory budgets.
- **Library calls run at once.** A library function, or the last method in a chain, returns a promise and starts the moment it's made, so `Promise.all` over several calls costs the slowest of them. The host runs up to eight at a time per script. A failed call rejects with an `Error` carrying the same `message` Lua would raise, plus `code` and `retryable`.
