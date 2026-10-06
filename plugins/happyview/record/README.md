# happyview-record

Library plugin exposing `happyview.record`: record writes as the calling
script's user, direct local-index writes, and lexicon validation. Every
export is a thin translator over one SDK host wrapper; the host does the work.

## Lua

```lua
local record = require("happyview.record")

local ref = record.create("app.bsky.feed.post", {text = "hi"})
record.put(ref.uri, {text = "hi again"}, {validate = false})
record.delete(ref.uri)
local blob = record.upload_blob(bytes, "image/png")    -- the PDS's blob ref
local loaded = record.load(ref.uri)                    -- envelope or nil
local local_ref = record.save_local("app.bsky.feed.post", "xyz", {text = "hi"})
record.delete_local(local_ref.uri)

local normalized = record.validate("app.bsky.feed.post", {text = "hi"})
local lexicon = record.lexicon("app.bsky.feed.post")   -- table or nil
```

## Surface

- `create(collection, tbl[, opts])` — `opts.rkey`, `opts.repo` (defaults to
  the caller's own, the only repo a caller-acting write can target),
  `opts.validate` (default `true`). Returns `{uri, cid}`.
- `put(uri, tbl[, opts])` — `opts.swap_cid` (a no-create guarantee: refuses
  unless it matches the record's current CID), `opts.validate` (default
  `true`, as for `create`). Returns `{uri, cid}`.
- `delete(uri)` — deletes from the user's own repo.
- `upload_blob(bytes, mime_type)` — `bytes` is a string or byte array;
  returns the PDS's blob ref, as-is.
- `load(uri)` — one indexed record, or nil.
- `save_local(collection, rkey, tbl[, did])` — writes straight into the local
  index, bypassing the PDS. `did` defaults to the calling user; required when
  called with no caller (a label or record-event script).
- `delete_local(uri)` — removes from the local index. Returns whether a row
  was actually there.
- `validate(collection, tbl)` — normalizes and checks a record without
  writing anywhere.
- `lexicon(collection)` — the lexicon document this instance holds for the
  collection, exactly as uploaded, or nil when none is registered. A record
  schema sits at `defs.main.record` (its `key`, `required` and `properties`
  are what `validate` reads).

There is no `generate_rkey`: use `require("internal.tids").create()`.

## Reads and the local index

`load` returns an envelope:

```lua
{uri, did, collection, rkey, cid, indexed_at, record}
```

`record` is the stored body verbatim, so a body with its own `uri` field
keeps it.

`create`, `put` and `delete` mirror into the local index as soon as the PDS
accepts them, so a script sees its own write on its next `load` (or any
`happyview.db` read): `create` and `put` upsert the row with the PDS's
`cid`, and `delete` removes it. `indexed_at` stays nil until the network
echoes the record. A `save_local` row has neither `cid` nor `indexed_at`
until then. A mirror failure is logged and does not fail the write.

## `$type` and defaults

`create`, `put`, `save_local` and `validate` all inject `$type: collection`
when the record doesn't already carry one, then fill in any top-level
property missing from the lexicon's declared defaults. `create`/`put` skip
the required-field check when `opts.validate` is `false`; the other two
(`save_local`, `validate`) always run it.

## Errors

- `BAD_INPUT` — a malformed argument (collection, uri, record, bytes, or a
  `save_local` with no `did` and no caller), raised before any host call.
- `INVALID_RECORD` — a required lexicon field is missing, naming which.
- `NO_SESSION` — no caller to act as (a label or record-event script calling
  `create`/`put`/`delete`/`upload_blob`).
- `AUTH_REQUIRED` / `WRITABLE_REPO` / `PDS_ERROR` — the PDS write failed: no
  valid session, the target repo isn't the caller's, or the PDS rejected it.

## Capabilities

`caller:write`, `records:read`, `records:write`. This library is **High
tier**: `caller:write` and `records:write` both let it write data — the
first to the user's own repo, the second straight into this instance's
index, bypassing the network.

Imports exactly `host_caller_create_record`, `host_caller_put_record`,
`host_caller_delete_record`, `host_caller_upload_blob`, `host_records_get`,
`host_records_index_put`, `host_records_index_delete`, `host_lexicon_get`.

## Build

```bash
cargo test -p happyview-record
cargo build --release --target wasm32-unknown-unknown -p happyview-record
```
