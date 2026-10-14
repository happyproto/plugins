## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-db` gets fixes.

## What this plugin can reach

`happyview-db` declares one capability, `records:read`, and imports exactly four host functions: `host_records_query`, `host_records_count`, `host_records_get` and `host_records_search`. HappyView's loader refuses a module whose imports need more than its manifest declares, so this plugin can't write records, touch the operator's own tables, make network requests or read secrets. Like every plugin, it runs sandboxed in WASM with no filesystem access.

The plugin never writes SQL. It folds a chain into a query spec, checking step names, operators, sort directions and limits along the way, and the host validates field paths and generates the query. So if something goes wrong here, the worst case should be a script reading indexed records it was always allowed to read.

## What's worth reporting

- A field path, operator or value that ends up in the generated SQL unescaped, on either SQLite or Postgres.
- A query that reads anything other than indexed records, like internal `happyview_*` tables.
- A way past the 100-row cap on `limit` or `search`.
- Anything that lets this module call a host import outside the four above.
