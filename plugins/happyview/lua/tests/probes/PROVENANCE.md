# Provenance

Copies, byte for byte and unmodified, from HappyView's
`.superpowers/spikes/lua-wasm/`:

- `_prelude.lua` and the fifteen probe files from `probes/`, 257 cases over
  Lua 5.4's observable behaviour — numbers, strings, patterns, tables,
  metatables, coroutines, varargs, utf8, bitwise operators, `goto`,
  attributes, error shapes and the `os` subset.
- `reference.txt` from `results/ref_probes_lua54.txt`: what native Lua 5.4
  prints for every one of them, recorded through the spike's own harness.

They carry no added header, because several cases report the line an error was
raised on.

`tests/probes.rs` runs them.
