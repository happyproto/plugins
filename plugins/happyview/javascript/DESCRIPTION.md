The JavaScript interpreter for HappyView v3. HappyView doesn't ship a script runtime anymore, so this plugin is what runs your JavaScript scripts: QuickJS-ng compiled to WASM, behind the `execute` and `validate` exports the host calls. The script you save is exactly the module QuickJS runs.

A script is an ES module whose default export is the handler, and everything it reaches comes in through `import`:

```js
import db from "happyview.db";
import http from "happyview.http";

export default async function handle(input, ctx) {
  const [posts, profile] = await Promise.all([
    db.records("app.example.post").limit(input.limit ?? 10).run(),
    http.get(`https://example.com/${input.did}`),
  ]);
  return { posts, profile };
}
```

## Modules, sandbox and limits

- A library function, or the last method in a chain like `.run()`, returns a promise and starts right away, so `Promise.all` over three calls takes as long as the slowest one. The host runs up to eight of a script's calls at a time and queues the rest.
- Four built-in modules: `internal.json`, `internal.time`, `internal.tids` and `internal.logging` (which `console` writes through).
- `import db from "happyview.db"` and `import { records } from "happyview.db"` both work for any installed library. The bridge sends the same documents Lua's does, so a library can't tell which language called it.
- The sandbox has no `eval`, no `Function` constructor, no timers, and no file or process access.
- Scripts get an instruction budget `catch` can't swallow and a memory ceiling, and runaway recursion throws a `RangeError` you can catch.

## Permissions

- `library:call`: call your other installed library plugins, which run with their own permissions.
- `script:host`: log, report job progress, check for a stop request and wait, all within its own run.
- `wasi:clock`, `wasi:random`, `wasi:stdio`: read the clock, read secure random numbers (for TIDs), and write to this plugin's log.
