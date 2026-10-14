`happyview.sql` is for the tables you own. If your scripts keep their own data next to HappyView's index (a leaderboard, a cache, a queue), it gives them a chainable table builder for the common reads and `raw` for everything else.

```lua
local sql = require("happyview.sql")

function handle(input, ctx)
  local top = sql.from("leaderboard")
    :where("score", ">", 100)
    :sort("score", "desc")
    :limit(input.limit)
    :run()

  local total = sql.from("leaderboard"):where("score", ">", 100):count()
  return { top = top, total = total }
end
```

In JavaScript and TypeScript it's the same API behind `import sql from "happyview.sql"`, and `run()`, `count()` and `raw()` return promises.

## API

- `from(table)` takes lazy `where`, `sort` and `limit` steps, then `run()` (an array of rows) or `count()`.
- `raw(sql, params?)` runs any statement, query or write. Placeholders are the backend's own: `?` on SQLite, `$1` on Postgres. A query returns its rows; a write returns an empty array.
- A number or boolean binds against the column's type, and a string binds as text.
- A `nil` step is skipped, so `:limit(input.limit)` is fine when there's no limit. `sort` defaults to `desc`, and `limit` defaults to 20 and caps at 100.
- Table names are bare identifiers: letters, digits and underscores, starting with a letter or underscore.
- Internal `happyview_*` tables holding secrets, tokens, auth state and keys are blocked unless they're allowlisted. Your own tables are yours to do what you like with.

## Capabilities

It asks for `database:read` and `database:write`. That makes it Critical tier, because `raw` can run any statement, schema changes and deletes included, against every table that isn't blocked. Only hand it to scripts you'd trust with the whole database.
