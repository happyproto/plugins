## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-backlinks` gets fixes.

## What this plugin can reach

`happyview-backlinks` declares one capability, `records:read`, and imports exactly one host function, `host_backlinks_query`. HappyView's loader refuses a module whose imports need more than its manifest declares, so this plugin can't write records, touch the operator's own tables, make network requests or read secrets. Like every plugin, it runs sandboxed in WASM with no filesystem access.

The plugin never writes SQL. It checks the chain (a URI, a required collection, known steps, a numeric limit) and sends it to the host as a query spec. The host owns the join against `happyview_record_refs`. So the worst a bug here should do is let a script read indexed records it could already read.

## What's worth reporting

- A URI, collection, DID or cursor that ends up in the generated SQL unescaped, on SQLite or Postgres.
- A query that returns anything other than indexed records, like rows from other internal `happyview_*` tables.
- A crafted `cursor` that skips the collection scope or the `did` filter.
- Anything that lets this module call a host import other than `host_backlinks_query`.
