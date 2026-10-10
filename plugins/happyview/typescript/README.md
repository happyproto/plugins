# happyview-typescript

The TypeScript interpreter, as a plugin: SWC's TypeScript transform in front
of the [`happyview-quickjs`](../quickjs/README.md) engine, compiled to
`wasm32-wasip1`, behind the two exports the host addresses an interpreter by.
`execute` runs a script, `validate` says whether one is runnable.

```ts
import db from "happyview.db";
import http from "happyview.http";
import log from "internal.logging";

interface Input {
  did: string;
  limit?: number;
}

export default async function handle(input: Input, ctx: { caller_did?: string }) {
  log.info("looking up", { did: input.did, who: ctx.caller_did });
  const [posts, profile] = await Promise.all([
    db.records("app.example.post").limit(input.limit ?? 10).run(),
    http.get(`https://example.com/${input.did}`),
  ]);
  return { posts, profile };
}
```

The script is compiled to JavaScript and then run by the engine, so
everything it can rely on once it runs — the contract, `ctx`, the built-in
modules, `import` and the library bridge, conversion, the sandbox, the
budgets, the error contract and the five permissions — is the engine's, and
is written down once, in [its README](../quickjs/README.md). This page is
only what TypeScript adds.

Types are erased, not checked: nothing here runs the TypeScript type
checker, so a type error is the editor's to show and never stops a run.
HappyView declares no types for `ctx` or for a library's module, so a script
declares the shapes it reads, as above, or uses `any`.

## What the transform runs

A full transform, not type stripping alone, so the TypeScript that has
meaning at run time works:

- **`enum`**, numeric with its reverse mapping (`Color[0] === "Red"`) and
  string.
- **`namespace`** with values, nested or not.
- **Parameter properties** — `constructor(public x: number)`.
- Everything that is only a type, erased: annotations, `interface`, `type`,
  generics, `as`, `<T>` assertions, `satisfies`, `!`, `declare`, `abstract`,
  overloads.

An import used only as a type is removed with the types, `import type` or
not, so a library a script names only for its types need not be installed.
Class fields keep their own semantics, as TypeScript's
`useDefineForClassFields` gives them on any target that has fields.

## What is refused

Each is a `syntax` error on its own line, from `validate` and from a run
alike, and `validate` reports every one in the script rather than the first:

- **Decorators** and **`accessor` fields.** SWC would pass them through as
  standard decorators, and QuickJS-ng, which runs the script, does not
  implement them yet.
- **`import x = require("…")`** and **`export =`**, which are CommonJS, and a
  script is a module. `import type T = require("…")` is a type and is erased;
  `import X = A.B`, an alias, is plain TypeScript and runs.
- **JSX.** A script is TypeScript, not TSX; JSX is recognised and refused by
  name rather than reported as the stray `<` TypeScript reads it as.

## Positions

The transform reprints the script, so the JavaScript QuickJS runs puts things
on different lines — an `enum` alone becomes several. It also emits a source
map, and every position the engine reports is read back through it: an
error's `line`, every `script:L:C` frame in its `raw` stack, a compile error,
and a validate error. A failure is therefore placed on the line the author
wrote. A position the map does not cover keeps the one QuickJS reported
rather than inventing one; the transform's own errors are in the author's
source from the start.

## Budgets

The transform runs inside the module, before the engine builds a runtime.
Its allocations come from Rust's allocator rather than QuickJS's, so the
engine's memory ceiling does not count them, and the instruction budget
cannot reach inside it. Both are bounded instead by what the host already
imposes on any guest — its ceiling on the module's memory and its wall
clock — and both grow with the size of the script.

## Size

About 4.6 MB, against the JavaScript plugin's 1.2 MB: SWC's parser,
transform and code generator are the difference. The module's imports are
the JavaScript plugin's exactly. SWC's default file loader would link the
WASI filesystem imports through `std::fs`; this plugin gives it a loader that
reads no files, so none is linked.

## Building

Like the JavaScript plugin, this is not pure Rust on
`wasm32-unknown-unknown`: the engine vendors QuickJS-ng, which
`rquickjs-sys` compiles with clang from **wasi-sdk 34.0**. The environment is
the JavaScript plugin's; see [its README](../javascript/README.md#building).

```sh
cargo build --release -p happyview-typescript --target wasm32-wasip1
TZ=UTC cargo test -p happyview-typescript
```

## Releases

A change to this directory or to the engine's releases this plugin.
`.releaserc.json` lists both paths for `release/package-paths.mjs`, which
counts a commit only when it touches one of them.

## Tests

`cargo test -p happyview-typescript` runs each transform, each refusal and
the position of each kind of error, and the 16-script conformance corpus —
the JavaScript corpus's scripts with their types written in — through both
`validate` and `execute`, against the same table of answers the JavaScript
plugin's corpus meets. It also reads the built module's import and export
sets, when one has been built, and fails if either differs from the
JavaScript plugin's.
