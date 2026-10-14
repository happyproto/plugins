## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-backlinks-v1.0.0) (2026-10-07)

- **First release on HappyView v3's plugin API.** Ships the `to(uri)` builder with `collection`, `did`, `limit`, `cursor` and `run`. Each result is a record envelope, `{uri, did, collection, rkey, cid, indexed_at, record}`, where v2 returned the record body with `uri` added to it. Built for v3 only; v2 instances can't install it.
