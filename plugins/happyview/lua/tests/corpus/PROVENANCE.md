# Provenance

The 84 Lua files beside this one are copies, byte for byte and unmodified:

- 79 `*.expected.lua` from HappyView's `tests/codemod/cases/`, the outputs the
  v2-to-v3 script codemod is pinned to produce.
- 5 from HappyView's `web/src/lib/lua-templates/`, the bodies the script editor
  prefills.

Together they are every v3 script this project knows the exact text of, which
is what makes them a conformance corpus rather than a sample. They carry no
added header, because a line a script reports would move if they did.

Three are expected to be refused, and are refused by the native sandbox too:
the two `marker_*_at_file_scope` cases read a removed global while loading,
which is what the codemod's marker comment tells a human to fix, and
`query.marker_no_handle.expected.lua` defines no `handle`.

`tests/corpus.rs` runs them.
