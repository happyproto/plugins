# happyview-lua

The Lua interpreter, as a plugin. PUC Lua 5.4.8 through
[`mlua`](https://crates.io/crates/mlua), compiled to `wasm32-wasip1`, behind
the two exports the host addresses an interpreter by: `execute` runs a script,
`validate` says whether one is runnable.

A script's language is therefore an install rather than a release. Nothing
here is privileged: everything a script can reach arrives through `require`,
which resolves the library plugins the instance already has.

## The contract

A script defines one function. Anything at file scope runs once, while the
script loads, before either argument exists.

```lua
local db = require("happyview.db")
local log = require("internal.logging")

function handle(input, ctx)
  log.info("looking up", { q = input.q, who = ctx.caller_did })
  return db.records("app.example.post"):limit(input.limit or 10):run()
end
```

`input` is the script's first argument and differs by trigger: query
parameters for a query, the procedure input for a procedure,
`{action, uri, did, collection, rkey, record}` for a record event,
`{src, uri, val, neg, cts, exp}` for a label, and the job's own input for a
job.

What is **returned** decides what the host does. A table is a value; `nil` is
"nothing"; anything else is neither. A record event reads those three as
skip, replace and proceed, a label as skip, merge and continue, and a query,
procedure or job serialises the value as its answer.

### `ctx`, per trigger

Common to all five: `trigger`, `caller_did`, `has_pds_auth`, `env`. A field
that does not apply to the trigger is **absent**, not `nil`-valued — the two
read alike in Lua, but the distinction is real on the wire.

| trigger | `ctx` also carries |
| --- | --- |
| XRPC query | `method`, `collection` |
| XRPC procedure | `method`, `collection`, `params`, `delegate_did` |
| record event | `collection` |
| label | — |
| job | `job` |

A space-scoped query or procedure also carries
`space = {uri, id, did, authority_did, spaceType, skey}`.

`ctx.env` is the instance's script variables, flat. Read-only by convention
rather than by enforcement.

`ctx.job` is the only part of `ctx` that is not data, because its three
functions act on the job row while the script is still running:

- `ctx.job.id`
- `ctx.job.progress(table)` — persist progress, visible in the dashboard
- `ctx.job.should_stop()` — `true` once the job is cancelling or pausing.
  Cooperative: the script has to check it and return
- `ctx.job.wait(seconds)` — sleep, clamped to 0 – 3600

The host holds the job for the whole run, so none of the three takes an id and
a script cannot name another job.

## The four built-in modules

Under `internal.`, which a published plugin can never shadow.

```lua
local json = require("internal.json")
local time = require("internal.time")
local tids = require("internal.tids")
local log  = require("internal.logging")

function handle(input, ctx)
  log.warn("careful", { uri = input.uri })
  return {
    rkey = tids.create(),
    at = time.to_iso8601(time.now()),
    body = json.encode(json.to_array({})),
  }
end
```

- **`internal.json`** — `encode(value)`, `decode(text)`, and `to_array(table)`,
  which marks a table so an empty one encodes as `[]`. Lua has one table type,
  so an empty sequence and an empty map are the same value until something
  says which was meant.
- **`internal.time`** — `now()` in milliseconds since the epoch,
  `to_iso8601(ms)`, `from_iso8601(text)` (`nil` when it will not parse).
- **`internal.tids`** — `create()`, `to_tid(ms)`, `from_tid(tid)`.
- **`internal.logging`** — `debug`, `info`, `warn`, `error`, each
  `(message[, fields])`. A line carries the trigger, the caller and the job it
  came from, which only the host knows, so this is a host call rather than a
  local one. `debug` reaches the process log and no table. A line that cannot
  be written never fails the run; a `fields` table that cannot be encoded
  does.

Any other `internal.` name raises, naming the four.

## `require`, and installed libraries

`require(name)` resolves a built-in first, then a library plugin by the
namespace it declares. An unknown name raises and names the plugin that would
serve it. Each name resolves once per run, so a module handed out twice is the
same table.

A library's exports arrive under their own names. A plain export is a
function. A *constructor* export returns an object whose methods chain —
each one accumulating a step and returning the object — until a method that
does not, which sends the object and the call as one request:

```lua
local db = require("happyview.db")
db.records("app.example.post"):where("author", ctx.caller_did):limit(5):run()
```

A library's error arrives as a Lua error whose text is the one the host
renders, so a script matching on a code inside it keeps working.

`nil` and JSON `null` are kept apart deliberately. A **library result's**
`null` becomes `nil`, so `if row.handle then` and `== nil` both behave.
A `null` anywhere else — in `input`, in `ctx`, in `json.decode` — is a
sentinel value that is truthy and encodes back to `null`, because a field
that was sent as null is not a field that was absent.

## The sandbox

Present: `print`, `string`, `table`, `math`, `utf8`, the coroutine library,
the base library, and a three-function `os`.

Absent, and each for its own reason:

- **`io`, `package`, `debug`** — never opened. They are not deleted from a
  table after the fact; the VM is built without them, which is also what keeps
  their C out of the compiled module and, with it, every filesystem and
  process call the host refuses.
- **`load`, `dofile`, `loadfile`** — loading a chunk at run time escapes every
  check the save path made.
- **`collectgarbage`** — collecting by hand makes a script's timings depend on
  when it ran.
- **`require`'s stock version** — replaced by the one above.
- **`os.clock`** — WASI preview 1 offers no process CPU clock. A function that
  is absent is better than one that always fails; `internal.time.now()` is
  what measures elapsed time.
- **`os.execute`, `os.exit`, `os.getenv`, `os.remove`, `os.rename`,
  `os.tmpname`, `os.setlocale`** — the `os` a script gets is `time`, `date`
  and `difftime`, and nothing else. It is an allow-list, so a function a later
  Lua adds to the library is absent until someone names it.

The three that remain are **Lua's own C functions**, not a reimplementation,
so their output, their normalisation and their error messages are PUC's.

Reading a global that v2 scripts used — `db`, `input`, `env`, `Record` and the
rest — **raises** with the sentence that names the codemod, rather than
yielding `nil` and failing later as `attempt to index a nil value` somewhere
inside the script's own body. Assignment is untouched, so a script may still
define helpers at file scope and may shadow one of those names by assigning
it.

`print` writes to this instance's plugin log, one line per call. It is not a
substitute for `internal.logging`, which is attributable and queryable.

**There is no timezone in the guest.** The host gives it no environment, so
wasi-libc has no `TZ` to read and answers UTC, which makes `os.date`'s `!`
prefix a no-op and makes `os.time{...}` read its table as UTC. That last one
is worth knowing: PUC reads such a table as *local* time, so on a server whose
zone is not UTC this differs from what the same script meant under HappyView
v2's in-process interpreter.

It follows that this plugin's own test suite needs `TZ=UTC`, since it compares
against output recorded under it and a native run takes the machine's zone.
The suite says so rather than quietly dropping the three cases that notice.

## The two budgets, and the two the host owns

**Instructions — this plugin's.** A hook every `limits.instructions` Lua
instructions, sticky once tripped: `pcall`, `xpcall`, `coroutine.resume` and a
wrapped coroutine each re-raise after the call they protect returns, so a
spent budget walks out through every layer of catching and a script cannot
swallow it. A job run is exempt, which is what `should_stop` is for.

**Memory — this plugin's.** `limits.memory_bytes` is mlua's own allocator
ceiling, so exhausting it is a Lua error a script can see rather than a trap.

**Wall clock — the host's**, as an interruption no Lua code can catch. It
counts only time the guest is running, so a script waiting on a PDS write is
not charged for the host's latency.

**Memory again — the host's**, a second ceiling above this one, so the clean
error is the one a script meets and the trap is only ever reached by something
the Lua limit cannot see.

## Permissions

Five, and what each one grants, in the words the install page shows. They are
copied from `PluginCapability::consent_sentence` in HappyView's
`src/plugin/capabilities.rs`, which is the only place they are defined; no
test spans the two repositories, so a change there has to be brought here by
hand.

| capability | grants |
| --- | --- |
| `library:call` | Call other installed library plugins, which run with their own permissions (not this plugin's). |
| `script:host` | Log, report progress, check for a stop request and wait on behalf of the script run it is inside; the host supplies the run's identity and job. |
| `wasi:clock` | Read the wall clock. |
| `wasi:random` | Read secure random numbers. |
| `wasi:stdio` | Write to this instance's plugin log. |

The clock is what `os.time`, `os.date` and `internal.time` read; randomness is
what a TID's clock id needs; and the log is where `print` goes. The module's
import section is checked against these five at load, so it can reach nothing
else.

## Building

This is the one plugin in the repo that is not pure Rust on
`wasm32-unknown-unknown`. It vendors PUC Lua, which `lua-src` compiles with
clang from **wasi-sdk 34.0**, and it needs the target's own sysroot because
rustc's bundled wasi-libc ships neither `libsetjmp.a` nor
`libwasi-emulated-signal.a`.

`.cargo/config.toml` at the repo root already names everything, assuming
wasi-sdk unpacked at `/opt/wasi-sdk`. With it there:

```sh
cargo build --release -p happyview-lua --target wasm32-wasip1
TZ=UTC cargo test -p happyview-lua          # native: the corpus and the probes
```

Installed elsewhere, override the four values — and reach for
`CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS` rather than `RUSTFLAGS`, which is not
target-scoped and whose `-L` into the wasi sysroot breaks every host build in
the same shell, this crate's own tests included:

```sh
export WASI_SDK=/path/to/wasi-sdk-34.0
export CC_wasm32_wasip1="$WASI_SDK/bin/wasm32-wasip1-clang"
export AR_wasm32_wasip1="$WASI_SDK/bin/llvm-ar"
export CFLAGS_wasm32_wasip1="--sysroot=$WASI_SDK/share/wasi-sysroot"
export CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="-L $WASI_SDK/share/wasi-sysroot/lib/wasm32-wasip1"
```

Without `CC_wasm32_wasip1` the platform's own clang runs and reports "no
available targets are compatible with triple wasm32-wasip1", which names
neither the toolchain nor the fix.

`src/lua_stubs.c` is compiled and linked ahead of both Lua and libc, and it is
what keeps the module's WASI imports inside the set the host allows. Its own
header says why each definition is there; the short version is that not
opening a Lua library does not unlink it.

## Tests

`cargo test -p happyview-lua` runs everything that does not need a host: the
sandbox, the `os` subset against Lua 5.4's recorded output, the JSON rules,
the bridge's chained documents, `ctx`, the budgets, the error contract, the
84-file conformance corpus and the 257 Lua 5.4 probes. It also reads the built
module's import and export sets, when one has been built.

What needs a host lives in HappyView: `tests/lua_interpreter_plugin.rs` loads
and runs this module, and `tests/lua_differential.rs` compares it against the
native runner case for case.
