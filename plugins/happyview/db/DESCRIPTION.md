`happyview.db` is how your HappyView scripts read the records your instance has indexed. You build a query by chaining steps onto `records(collection)`, and nothing is sent until you call `run()`, `count()` or `first()`. The plugin folds the chain into a query spec and the host writes the SQL, so the same script works on SQLite and Postgres.

```lua
local db = require("happyview.db")

function handle(input, ctx)
  local page = db.records("app.bsky.feed.post")
    :where("author", "=", ctx.caller_did)
    :sort("createdAt", "desc")
    :limit(input.limit)
    :cursor(input.cursor)
    :run()

  return { posts = page.records, cursor = page.cursor }
end
```

In JavaScript and TypeScript it's the same chain behind `import db from "happyview.db"`, and the final call returns a promise you `await`.

## Functions

- `records(collection)` with lazy `where`, `sort`, `limit`, `cursor` and `did` steps, then `run()` (`{records, cursor}`), `count()` or `first()`.
- `get(uri)` for one record by AT URI, or nil.
- `search(collection, field, query, limit?)` for a substring search on one field, ranked by where the match lands. Defaults to 10 results, max 100.
- `backend()` tells you `"sqlite"` or `"postgres"`.

A few things I'd want to know up front:

- **Records come back as envelopes.** Each one is `{uri, did, collection, rkey, cid, indexed_at, record}`, with the stored body untouched under `record`.
- **A `nil` step is skipped.** That's why `:limit(input.limit)` above is fine when the caller didn't send a limit.
- **`where` takes dotted JSON paths.** Operators are `=`, `!=`, `<`, `>`, `<=`, `>=`, `like`, `not like` and `ilike`, and repeated `where`s combine with `AND`. `limit` defaults to 20 and caps at 100.

## Capabilities

It only declares `records:read`, so it can read indexed records but can't write them, make network requests or read secrets.
