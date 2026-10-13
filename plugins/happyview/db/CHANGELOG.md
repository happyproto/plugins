## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-db-v1.0.0) (2026-10-07)

- **First release on HappyView v3's plugin API.** Ships the `records(collection)` query builder plus `get`, `search` and `backend`. Record reads now return an envelope, `{uri, did, collection, rkey, cid, indexed_at, record}`, with the stored body under `record`. v2 returned the body with `uri` written onto it. Built for v3 only; v2 instances can't install it.
