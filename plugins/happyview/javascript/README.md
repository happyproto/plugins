# happyview-javascript

The JavaScript interpreter, as a plugin: the
[`happyview-quickjs`](../quickjs/README.md) engine — QuickJS-ng compiled to
`wasm32-wasip1` — behind the two exports the host addresses an interpreter by.
`execute` runs a script, `validate` says whether one is runnable.

A script's language is therefore an install rather than a release. JavaScript
is what the engine already runs, so this plugin's front end prepares nothing:
the script an author saved is the module QuickJS compiles, and every position
an error reports is already the author's own.

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

What a script can rely on — the contract, `ctx`, the built-in modules,
`import` and the library bridge, conversion, the sandbox, the budgets, the
error contract and the five permissions — is the engine's, and is written
down once, in [its README](../quickjs/README.md).

## Building

Like `happyview-lua`, this is not pure Rust on `wasm32-unknown-unknown`. The
engine vendors QuickJS-ng, which `rquickjs-sys` compiles with clang from
**wasi-sdk 34.0** against the target's own sysroot.

`.cargo/config.toml` at the repo root already names everything, assuming
wasi-sdk unpacked at `/opt/wasi-sdk`. With it there:

```sh
cargo build --release -p happyview-javascript --target wasm32-wasip1
TZ=UTC cargo test -p happyview-quickjs -p happyview-javascript
```

Installed elsewhere, override the same values the Lua plugin's README lists,
plus `WASI_SDK` itself — `rquickjs-sys` reads it to find the toolchain, and
without it downloads a wasi-sdk of its own choosing:

```sh
export WASI_SDK=/path/to/wasi-sdk-34.0
export CC_wasm32_wasip1="$WASI_SDK/bin/wasm32-wasip1-clang"
export AR_wasm32_wasip1="$WASI_SDK/bin/llvm-ar"
export CFLAGS_wasm32_wasip1="--sysroot=$WASI_SDK/share/wasi-sysroot"
export CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="-L $WASI_SDK/share/wasi-sysroot/lib/wasm32-wasip1"
```

## Releases

A change to this directory or to the engine's releases this plugin: a fix in
the engine is a fix in every module built on it. `.releaserc.json` lists both
paths for `release/package-paths.mjs`, which counts a commit only when it
touches one of them.

## Tests

`cargo test -p happyview-javascript` runs a 16-script conformance corpus,
translated from the Lua plugin's, through both `validate` and `execute` on the
engine's harness. It also reads the built module's import and export sets,
when one has been built, and fails if either differs from the list it pins.
The engine's own tests are `cargo test -p happyview-quickjs`.
