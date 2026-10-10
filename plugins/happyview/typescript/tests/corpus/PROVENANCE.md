# Provenance

The 16 TypeScript files beside this one are the JavaScript corpus's scripts
(`plugins/happyview/javascript/tests/corpus/`), one each, under the same name
with `.ts` for `.js`. That corpus is itself a hand translation of scripts in
`plugins/happyview/lua/tests/corpus/`; its `PROVENANCE.md` says which.

A port keeps the JavaScript script's shape and logic and adds what an author
writing TypeScript would: an interface for the `input` and the part of `ctx`
the script reads, and the types of values it builds and returns. Nothing in HappyView declares `ctx`'s type, so
each script declares the fields it uses. Library modules are untyped, so a
library's results are typed where the script reads them.

`query.marker_no_handle` is refused, as its counterparts are: it exports
`handle` by name rather than as the default.

`src/conformance.rs` runs them through the engine's harness, which holds one
table of what every script must answer, whichever language it is written in.
