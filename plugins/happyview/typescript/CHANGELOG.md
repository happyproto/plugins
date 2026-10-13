## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-typescript-v1.0.0) (2026-10-10)

- **First release on HappyView v3's plugin API.** TypeScript scripts run through SWC's transform and then the shared QuickJS-ng engine, so they get everything `happyview-javascript` 1.0.1 has: concurrent library calls, the `internal.` modules, the sandbox and the budgets. `enum`, `namespace` and parameter properties work. Decorators, CommonJS forms and JSX are refused with a `syntax` error, each on its own line, and every error position is mapped back to the line you wrote.
