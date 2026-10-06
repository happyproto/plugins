# happyview-db

Library plugin exposing `happyview.db`: a chainable builder over indexed
records. It folds the chain into a query spec and sends it to one host
import; the host generates the SQL and owns the schema.

## Lua

```lua
local db = require("happyview.db")

function handle(input, ctx)
  local page = db.records("app.bsky.feed.post")
    :where("author", "=", ctx.caller_did)
    :where("text", "like", "%happyview%")
    :sort("createdAt", "desc")
    :limit(20)
    :run()

  for _, row in ipairs(page.records) do
    print(row.uri, row.record.text)
  end

  local total = db.records("app.bsky.feed.post"):where("author", "=", ctx.caller_did):count()
  local newest = db.records("app.bsky.feed.post"):sort("createdAt", "desc"):first()
  return {total = total, newest = newest}
end

local one = db.get("at://did:plc:abc/app.bsky.feed.post/xyz") -- envelope or nil
local hits = db.search("app.bsky.feed.post", "text", "happyview", 10)
local backend = db.backend() -- "sqlite" or "postgres"
```

## Surface

- `records(collection)` — constructor. Lazy `where(field, op, value)`,
  `sort(field, direction)`, `limit(n)`, `cursor(c)`, `did(d)`. Immediate
  `run()` → `{records, cursor}`, `count()` → integer, `first()` → one
  envelope or nil.
- `get(uri)` — one record by AT URI, or nil.
- `search(collection, field, query, limit?)` — substring search on one
  field, ranked by match position. Default limit 10, max 100.
- `backend()` — `"sqlite"` or `"postgres"`, from the call context the host
  fills on every call.

`run`, `first`, `get` and `search` return each record as an envelope:

```lua
{uri, did, collection, rkey, cid, indexed_at, record}
```

`record` is the stored body verbatim, so a body with its own `uri` field
keeps it. `cid` is nil while the row holds no CID (a `save_local` write) and
`indexed_at` is nil until the network has echoed the record. A record written
through `happyview.record` is in the index at once, with the PDS's `cid` and
no `indexed_at`.

`where` fields are dotted JSON paths with optional array indices, validated
by the host. Operators: `=`, `!=`, `<`, `>`, `<=`, `>=`, `like`,
`not like`, `ilike` (case-insensitive on input; the host lowers it to
`LIKE`/`ILIKE` per backend). Filters send the value as given; the host
compares record fields as text regardless of its JSON type. Repeated `where`
steps combine with `AND`. `limit` defaults to 20 and caps at 100.

Pagination depends on `sort`: the default sort paginates by a `(created_at,
uri)` keyset cursor; a custom `sort` paginates by offset cursor instead.
Both move opaquely through `cursor()`.

A lazy step given `nil` is skipped, so `:limit(input.limit)` reads as no
limit when the input has none.

## Errors

- `BAD_CHAIN` — the object document itself is malformed (unknown step,
  unknown operator, bad sort direction, non-numeric limit). Raised by this
  crate before any host call.
- `INVALID_SPEC` — the host rejected an otherwise well-formed spec (bad
  field path). Passed through unchanged.

## Capabilities

`records:read`. Imports exactly `host_records_query`, `host_records_count`,
`host_records_get`, `host_records_search`.

## Build

```bash
cargo test -p happyview-db
cargo build --release --target wasm32-unknown-unknown -p happyview-db
```
