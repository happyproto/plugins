#!/usr/bin/env bash
# Renders tests/declarations/input.json through both renderers and hands the
# result, with a sample handler each, to the language's own checker: tsc for
# the .d.ts files and lua-language-server for the LuaLS ones. Snapshot tests
# say the output is what we wrote; this says the tools accept it.
#
#   LUA_LANGUAGE_SERVER   the lua-language-server binary (default: on PATH)
#   CARGO_ARGS            extra arguments for cargo, before the subcommand
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

render() {
  # shellcheck disable=SC2086
  DECLARATIONS_OUT="$out/$2" cargo ${CARGO_ARGS:-} test -j 2 -q -p "$1" --lib \
    declarations::tests::render_the_shared_fixture -- --ignored --exact >/dev/null
}
render happyview-quickjs ts
render happyview-lua lua

cp "$here/sample.ts" "$out/ts/sample.ts"
cat > "$out/ts/tsconfig.json" <<'JSON'
{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "target": "es2022",
    "module": "es2022",
    "moduleResolution": "bundler",
    "lib": ["es2022"],
    "types": []
  },
  "include": ["**/*.ts"]
}
JSON
(cd "$root" && npx --no-install tsc -p "$out/ts/tsconfig.json")
echo "tsc: ok"

cp "$here/sample.lua" "$out/lua/sample.lua"
cat > "$out/lua/.luarc.json" <<'JSON'
{
  "runtime.version": "Lua 5.4",
  "diagnostics.globals": ["handle", "job", "on_post"],
  "diagnostics.severity": { "lowercase-global": "Hint!" }
}
JSON
"${LUA_LANGUAGE_SERVER:-lua-language-server}" --check "$out/lua" --checklevel=Information \
  --logpath "$out/lua-log"
echo "lua-language-server: ok"
