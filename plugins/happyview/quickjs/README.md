# happyview-quickjs

The engine HappyView's QuickJS interpreter plugins share: QuickJS-ng through
[`rquickjs`](https://crates.io/crates/rquickjs), the event loop that settles
library calls, the bridge, the built-in modules, the budgets, `ctx`, the
sandbox and the error contract. It is a library crate, not a plugin: it has no
manifest and is never released on its own. Each language is a plugin of its
own that names a front end — the step from what an author wrote to the
JavaScript QuickJS compiles — and wires it into the two exports the host
addresses an interpreter by: `execute` runs a script, `validate` says whether
one is runnable.

| plugin | front end |
| --- | --- |
| [`happyview-javascript`](../javascript/README.md) | none: the script is the module |

Everything below is therefore what a script sees in every QuickJS language.
Nothing here is privileged: everything a script can reach arrives through
`import`, which resolves the library plugins the instance already has.

## The contract

A script is an ES module whose default export is the handler. Anything at
file scope runs once, while the module loads, before either argument exists.

```js
import db from "happyview.db";
import http from "happyview.http";
import log from "internal.logging";

export default async function handle(input, ctx) {
  log.info("looking up", { q: input.q, who: ctx.caller_did });
  const [posts, profile] = await Promise.all([
    db.records("app.example.post").limit(input.limit ?? 10).run(),
    http.get(`https://example.com/${input.did}`),
  ]);
  return { posts, profile };
}
```

`input` is the script's first argument and differs by trigger: query
parameters for a query, the procedure input for a procedure,
`{action, uri, did, collection, rkey, record}` for a record event,
`{src, uri, val, neg, cts, exp}` for a label, and the job's own input for a
job.

The handler may be `async`; a returned promise is awaited, and what it
settles to is what was returned. File scope may use top-level `await`, which
settles before `handle` is called.

What is **returned** decides what the host does. An object or an array is a
value; `undefined` or `null` is "nothing"; anything else is neither. A record
event reads those three as skip, replace and proceed, a label as skip, merge
and continue, and a query, procedure or job serialises the value as its
answer.

### `ctx`, per trigger

The same fields the Lua plugin builds, under the same names. Common to all
five: `trigger`, `caller_did`, `has_pds_auth`, `env`. A field that does not
apply to the trigger is **absent** — `"method" in ctx` is false for a record
event — rather than present and `undefined`.

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
functions act on the job row while the script is still running. They keep the
Lua names, so the docs and the host stay one vocabulary:

- `ctx.job.id`
- `ctx.job.progress(data)` — persist progress, visible in the dashboard
- `ctx.job.should_stop()` — `true` once the job is cancelling or pausing.
  Cooperative: the script has to check it and return
- `ctx.job.wait(seconds)` — sleep, clamped to 0 – 3600. It returns a promise,
  so `await ctx.job.wait(5)` reads as what it does

The host holds the job for the whole run, so none of the three takes an id and
a script cannot name another job.

## The four built-in modules

Under `internal.`, which a published plugin can never shadow.

```js
import json from "internal.json";
import time from "internal.time";
import { create } from "internal.tids";
import log from "internal.logging";

export default function handle(input, ctx) {
  log.warn("careful", { uri: input.uri });
  return {
    rkey: create(),
    at: time.to_iso8601(time.now()),
    body: json.encode([]),
  };
}
```

- **`internal.json`** — `encode(value)` and `decode(text)`, with the
  conversion rules below. There is no `to_array`: that exists in Lua only
  because a Lua table cannot say whether it is empty as an array or as a map.
- **`internal.time`** — `now()` in milliseconds since the epoch,
  `to_iso8601(ms)`, `from_iso8601(text)` (`null` when it will not parse).
- **`internal.tids`** — `create()`, `to_tid(ms)`, `from_tid(tid)`.
- **`internal.logging`** — `debug`, `info`, `warn`, `error`, each
  `(message[, fields])`. A line carries the trigger, the caller and the job it
  came from, which only the host knows, so this is a host call rather than a
  local one. `debug` reaches the process log and no table. A line that cannot
  be written never fails the run; `fields` that cannot be encoded do.

`console.log`, `info`, `debug`, `warn` and `error` are `internal.logging` at
the matching level (`log` as `info`), their arguments joined into one line.

Any other `internal.` name is refused, naming the four.

## `import`, and installed libraries

`import` resolves a built-in first, then a library plugin by the namespace it
declares. An unknown name fails the run and names the plugin that would serve
it. A path or a URL — `./helper.js`, `https://…` — is refused: everything a
script imports is a name the instance already knows. Dynamic `import()` goes
through the same resolution.

Every module has the same shape: a default export holding every function, and
each function again under its own name. So `import db from "happyview.db"` and
`import { records } from "happyview.db"` both work, and
`import { delete as remove } from "happyview.http"` reaches a function whose
name is a reserved word.

A library's exports arrive under their own names. A plain export is a
function. A *constructor* export returns an object whose methods chain — each
one accumulating a step and returning the object — until a method that does
not, which sends the object and the call as one request:

```js
import db from "happyview.db";
const rows = await db.records("app.example.post").where("author", ctx.caller_did).limit(5).run();
```

The documents on the wire are the Lua bridge's, byte for byte, so a library
cannot tell which language called it. An argument list ends at the first
`undefined`, which is where Lua's ends at `nil` — `.limit(input.limit)` with no
`limit` sends a step with no argument in either language.

### Async, and running calls at once

**Every call that sends returns a promise**: a library function, and a chain's
final method. The chain's own steps send nothing and stay synchronous, which is
what keeps them chainable.

A call is started the moment it is made, and the host runs it beside the
script. So `Promise.all` over three calls costs the slowest of them rather than
their sum, and a call made without `await` is already under way while the
script does something else. The host runs at most eight of one script's calls
at once; a ninth is started at once and queued until one of the eight
finishes. A call still running when `handle` returns is let finish and its
result discarded, so a fire-and-forget write still lands; a run the host cuts
short — a trap, or its wall clock — takes its calls with it.

A failed call rejects with an `Error` whose `message` is exactly what the Lua
bridge raises for the same failure, so a script matching on text inside it
keeps working, and with `code` and `retryable` to read without parsing. An
`AUTH_ERROR:` in a library's message survives into the run's failure, which is
what lets the host answer 401.

A promise that can never settle — one awaiting nothing that is still in flight
— ends the run with a clear error rather than hanging it. There are no timers:
`setTimeout` does not exist.

### Conversion

A value crossing to a library, to `json.encode`, to `ctx.job.progress` or out
as the handler's answer follows `JSON.stringify`'s rules where those are
sound: an object is an object and an array an array, `undefined` leaves an
object and is `null` in an array, `toJSON` (a `Date`'s, say) answers for its
object, and a number with no fraction is an integer. Where `stringify` would
silently drop a value — a function, a symbol — or fail without saying where —
a `BigInt`, a cycle — the conversion is refused with an error naming the value
and the key it was under.

## The sandbox

Present: the ECMAScript standard library QuickJS-ng implements — `JSON`,
`Promise`, `Map`, `Set`, `RegExp`, `Date`, typed arrays, `Proxy`, `WeakRef`
and the rest — and `console`.

Absent, and each for its own reason:

- **`std`, `os`, `print`, timers, file and process access** — QuickJS-ng's
  `quickjs-libc` half, which is never compiled in. It is not removed after the
  fact; the module does not contain it.
- **`eval`, and the `Function` constructor in all four kinds** (`Function`,
  `AsyncFunction`, `GeneratorFunction`, `AsyncGeneratorFunction`) — turning a
  string into code at run time escapes every check the save path made, which
  is the line the Lua plugin draws at `load`. The constructors are replaced
  rather than deleted, so `instanceof Function` and `.constructor` still
  behave; calling one throws an `EvalError`.

`removed_globals`, the host's list of HappyView v2's Lua globals, is not read:
no script in a QuickJS language predates v3, so there is nothing for the migration
sentence to send anyone to, and reading an undeclared name in a module is
already a `ReferenceError` naming it.

**There is no timezone in the guest.** The host gives it no environment, so
`Date` reads and writes UTC. The engine's test suite, and each plugin's,
therefore runs under `TZ=UTC`, where a native run matches the module.

## The three budgets, and the two the host owns

**Instructions — the engine's.** QuickJS checks for an interrupt at every
loop back-edge and call, and asks the engine once every 10,000 checks;
`limits.instructions` is counted in checks. Once spent it stays spent, and the interrupt is **uncatchable**: `catch`
and `finally` are skipped, an `async` function's promise never settles, and a
run that somehow reaches a normal return afterwards fails as a timeout anyway.
A job run is exempt, which is what `should_stop` is for.

**Memory — the engine's.** `limits.memory_bytes` is a ceiling on QuickJS's
own allocations, counted by the engine's allocator, so exhausting it fails
the run as a memory error rather than a trap.

**Stack — the engine's.** 384 KiB of QuickJS stack, so runaway recursion is a
`RangeError: Maximum call stack size exceeded` a script can catch, with its
line. Upstream QuickJS-ng compiles this check out under WASI, so the
workspace builds it from a fork that keeps it. The check measures the guest's
shadow stack, but what runs out is the host's native stack, which costs far
more per level; the ceiling is sized so it fires first on a host that gives a
guest 16 MiB of native stack, as HappyView does. A host on wasmtime's
512 KiB default runs out first, so there runaway recursion still traps.

**Wall clock — the host's**, as an interruption no script can catch. It counts
only time the guest is running, so a script waiting on a library call is not
charged for it.

**Memory again — the host's**, a second ceiling above this one, so the clean
error is the one a script meets and the trap is only ever reached by something
the engine's ceiling cannot see.

## Errors

A failure's `message` is an error's own message, prefixed by its name unless
that is plain `Error` — `throw new Error("boom")` reads as `boom`, a property
read of `undefined` as `TypeError: cannot read property 'x' of undefined`. Its
`line` is the innermost frame in the script itself, so an error a library call
raised is placed at the line that made the call. `raw` keeps the name, the
message and the whole stack for the event log.

A compile failure is `syntax` only when it is a `SyntaxError`; an import that
nothing serves is found while compiling, and is `runtime`, as a `require`
failure is in Lua.

`validate` compiles the module and evaluates file scope with every import a
stub — a value whose every property is a function returning another stub — so
a chain of any shape at file scope evaluates without a host, and a named
import of any name is satisfied. It then requires a default export that is a
function.

## Permissions

Five, declared by every plugin built on the engine, and what each one grants,
in the words the install page shows. They are
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

The clock is what `Date` and `internal.time` read; randomness is what a TID's
clock id needs; and the log is where the engine's own diagnostics would go. The module's import section is checked against these
five at load, so it can reach nothing else. Running calls at once needs no
sixth: it reaches nothing a script could not reach one call at a time.

## Building

The engine is built as part of each plugin, for `wasm32-wasip1`, never on its
own. Like `happyview-lua`, it is not pure Rust on `wasm32-unknown-unknown`: it
vendors QuickJS-ng, which `rquickjs-sys` compiles with clang from **wasi-sdk
34.0** against the target's own sysroot. Each plugin's README has the
environment and the commands.

`src/quickjs_stubs.c` is compiled by this crate's build script and linked
whole, ahead of both QuickJS and libc, into every plugin built on it; it is
what keeps a module's WASI imports to exactly the three the host grants. Its
own header says why each definition is there; the short version is that
Rust's panic machinery asks for the environment and every stdio stream
carries a seek and a close, and each of those links an import the host
refuses.

## Tests

`TZ=UTC cargo test -p happyview-quickjs` runs everything that does not need a
host: the sandbox, the JSON rules, the bridge's chained documents, `ctx`, the
built-ins, the budgets, the error contract, and the event loop against a fake
host that settles calls in an order each test picks.

The `testing` feature exposes that fake host and the conformance corpus's
harness to the plugins' own tests, so each language runs its translation of
the 16-script corpus against one table of what every script must answer.
