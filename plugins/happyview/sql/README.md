# happyview-sql

Library plugin exposing `happyview.sql`: raw SQL, and a chainable table
builder over the operator's own tables. The builder folds its chain into a
query spec and sends it to one host import; the host generates the SQL and
owns the schema.

## Lua

```lua
local sql = require("happyview.sql")

local page = sql.from("leaderboard")
  :where("score", ">", 100)
  :sort("score", "desc")
  :limit(20)
  :run()

local total = sql.from("leaderboard"):where("score", ">", 100):count()

local rows = sql.raw("SELECT id, score FROM leaderboard WHERE score > ?", {100})
```

## Surface

- `from(table)` — constructor. Lazy `where(field, op, value)`,
  `sort(field, direction)`, `limit(n)`. Immediate `run()` → array of rows,
  `count()` → integer.
- `raw(sql, params?)` — a statement with backend-native placeholders (`?` on
  SQLite, `$1` on Postgres). `raw` runs every statement, query or write,
  through the same host import: a query's rows come back as an array of
  objects, a write comes back as an empty array.

`where` operators: `=`, `!=`, `<`, `>`, `<=`, `>=`, `like`, `not like`,
`ilike` (case-insensitive on input; the host lowers it to `LIKE`/`ILIKE` per
backend). A number or boolean value binds typed against the column; a string
binds as text. Repeated `where` steps combine with `AND`. `sort` direction
defaults to `desc`; `limit` defaults to 20 and caps at 100.

A lazy step given `nil` is skipped, so `:limit(input.limit)` reads as no
limit when the input has none.

`table` in `from` and every table name in `raw` must be a bare identifier:
letters, digits and underscores, starting with a letter or underscore. This
is the same rule Lua's `db.raw` enforces, and it exists to make table names
tokenizable rather than to sanitize input — the guard below is what actually
blocks access.

## Protected tables

Both `from` and `raw` are blocked from touching internal `happyview_*`
tables that aren't explicitly allowlisted (secrets, tokens, auth state,
cryptographic material), the same guard shared with Lua's `db.raw`. A table
merely containing `happyview_` mid-name, or one entirely outside that
prefix, is unaffected — operators can do anything they want with their own
tables.

## Errors

- `BAD_CHAIN` — the object document itself is malformed (unknown step,
  unknown operator, bad sort direction, non-numeric limit). Raised by this
  crate before any host call.
- `BAD_INPUT` / `FORBIDDEN` / `DB_ERROR` — the host rejected an otherwise
  well-formed call (bad table name, protected table, or a database error).
  Passed through unchanged.

## Capabilities

`database:read`, `database:write`. This library is **Critical tier**: it
declares `database:write`, which lets `raw` run any statement — including
schema changes and deletes — against every table it isn't explicitly
blocked from. Grant it only to scripts an operator trusts with the whole
database.

Imports exactly `host_table_query`, `host_db_query`.

## Build

```bash
cargo test -p happyview-sql
cargo build --release --target wasm32-unknown-unknown -p happyview-sql
```
