The Lua interpreter for HappyView v3. HappyView doesn't ship a script runtime anymore, so without this plugin your Lua scripts don't run. It's PUC Lua 5.4.8 compiled to WASM, behind the `execute` and `validate` exports the host calls.

A script defines one `handle(input, ctx)` function, and everything it reaches comes in through `require`:

```lua
local async = require("internal.async")
local db = require("happyview.db")
local http = require("happyview.http")

function handle(input, ctx)
  local posts, profile = async.all(
    function() return db.records("app.example.post"):limit(10):run() end,
    function() return http.get("https://example.com/" .. input.did) end
  )
  return { posts = posts, profile = profile }
end
```

## Modules, sandbox and limits

- Five built-in modules: `internal.json`, `internal.time`, `internal.tids`, `internal.logging` and `internal.async`. No published plugin can shadow the `internal.` names.
- Library calls made inside the functions you pass to `async.all` run concurrently, and their results come back in argument order.
- `require` resolves `happyview.*` names to whatever library plugins your instance has installed.
- The sandbox has no `io`, `package`, `debug`, `load` or `collectgarbage`, and `os` only has `time`, `date` and `difftime`.
- Scripts get an instruction budget they can't `pcall` their way out of, and a memory ceiling that raises a Lua error when it's hit, so the run doesn't crash.

## Permissions

- `library:call`: call your other installed library plugins, which run with their own permissions.
- `script:host`: log, report job progress, check for a stop request and wait, all within its own run.
- `wasi:clock`, `wasi:random`, `wasi:stdio`: read the clock, read secure random numbers (for TIDs), and write to this plugin's log (where `print` goes).
