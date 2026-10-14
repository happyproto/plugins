## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-sql-v1.0.0) (2026-10-07)

- **First release on HappyView v3's plugin API.** Ships the `from(table)` builder with `where`, `sort`, `limit`, `run` and `count`, plus `raw(sql, params?)` for any statement. Both are blocked from HappyView's protected `happyview_*` tables, the same guard Lua's `db.raw` uses. Built for v3 only; v2 instances can't install it.
