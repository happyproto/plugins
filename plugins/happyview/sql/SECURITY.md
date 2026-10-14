## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-sql` gets fixes.

## What this plugin can reach

`happyview-sql` declares `database:read` and `database:write`, and imports exactly two host functions: `host_table_query` and `host_db_query`. HappyView's loader refuses a module whose imports need more than its manifest declares, so it can't make network requests or read secrets or KV storage. Like every plugin, it runs sandboxed in WASM with no filesystem access.

Inside the database, though, it's Critical tier. `raw` can run any statement, including schema changes and deletes, against every table it isn't blocked from. Treat any script that calls it as trusted with the whole database.

The one boundary is the protected-table guard. Both `from` and `raw` refuse internal `happyview_*` tables that aren't explicitly allowlisted (secrets, tokens, auth state, cryptographic material). It's the same guard Lua's `db.raw` uses. Table names have to be bare identifiers so the guard can find them in a statement.

## What's worth reporting

- Any way to read or write a protected `happyview_*` table through `from` or `raw`, on SQLite or Postgres. Quoting, schema-qualified names, comments, CTEs, views or anything else that gets a statement past the guard counts.
- A builder field, operator or value that reaches the generated SQL unescaped.
- Anything that lets this module call a host import other than the two above.

Writing to, or dropping, your own non-`happyview_` tables is what this plugin is for, so that isn't a vulnerability.
