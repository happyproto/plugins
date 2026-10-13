The TypeScript interpreter for HappyView v3. It runs SWC's TypeScript transform inside the module and passes the result to the same QuickJS-ng engine `happyview-javascript` uses, so once a script is running it behaves exactly like JavaScript.

```ts
import db from "happyview.db";

interface Input {
  limit?: number;
}

export default async function handle(input: Input, ctx: { caller_did?: string }) {
  return db.records("app.example.post").limit(input.limit ?? 10).run();
}
```

## What TypeScript adds

- **Types are erased without being checked.** Nothing runs the type checker, so a type error is your editor's problem and never stops a run. HappyView declares no types for `ctx` or libraries, so declare the shapes you read or use `any`.
- **It's a full transform.** `enum` (with reverse mapping), `namespace` with values, and parameter properties all work. An import used only as a type is removed, so you don't need that library installed.
- **Errors point at your lines.** Every reported position goes back through a source map to the line you wrote.
- **Some things are refused up front.** Decorators and `accessor` fields (QuickJS-ng doesn't run them yet), `import x = require()` and `export =` (CommonJS), and JSX. `validate` reports every one, each on its own line.

Concurrent library calls, the four `internal.` modules, the sandbox and the budgets all come from the shared engine and work the same as in `happyview-javascript`.

## Permissions

The same five as the JavaScript plugin, with exactly the same imports:

- `library:call`: call your other installed library plugins, which run with their own permissions.
- `script:host`: log, report job progress, check for a stop request and wait, all within its own run.
- `wasi:clock`, `wasi:random`, `wasi:stdio`: read the clock, read secure random numbers, and write to this plugin's log.
