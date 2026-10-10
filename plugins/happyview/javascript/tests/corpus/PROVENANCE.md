# Provenance

The 16 JavaScript files beside this one are hand translations of scripts in
`plugins/happyview/lua/tests/corpus/`, one each, under the same name with
`.js` for `.expected.lua`:

- 5 of the editor templates (`query`, `procedure`, `job`, `trigger`,
  `record-event`), which between them cover every trigger.
- 11 of the codemod's expected outputs, chosen for reaching a library of each
  kind — a plain function, a constructor with an immediate method, a
  built-in by named import — and for one each of the label and job
  contracts. `label.event_to_input` is `label.event_and_bare_globals_to_input`
  with the codemod's half of its name dropped, since nothing here was
  migrated.

A translation keeps the original's shape and changes only what the language
has to: `require` becomes `import`, `function handle` becomes the default
export, a sending call is awaited, `nil` becomes `undefined`. Where the
original carries a codemod polyfill for a v2 shape, the translation is the
script the polyfill stands in for, since no JavaScript script predates v3.
`query.atproto_functions` also awaits its three independent lookups together,
which is what this plugin has that the Lua one does not.

`query.marker_no_handle` is refused, as its Lua counterpart is: it exports
`handle` by name rather than as the default.

`src/conformance.rs` runs them, through `validate` and then through `execute`
against a host that answers each library call.
