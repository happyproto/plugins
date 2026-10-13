`happyview.record` is how a script writes records. It writes to the calling user's repo on their PDS, writes directly to your instance's local index, and checks records against the lexicons your instance holds.

## Usage

```lua
local record = require("happyview.record")

function handle(input, ctx)
  local ref = record.create("app.bsky.feed.post", {text = input.text})
  local saved = record.load(ref.uri)   -- already there, see below
  return {uri = ref.uri, cid = saved.cid}
end
```

You see your own writes right away. `create`, `put` and `delete` mirror into the local index as soon as the PDS accepts them, so the next `load` (or any `happyview.db` read) has them. `indexed_at` stays nil until the network echoes the record back.

## API

- `create(collection, tbl[, opts])` and `put(uri, tbl[, opts])` return `{uri, cid}`. `put` takes `opts.swap_cid` and refuses the write unless it matches the current CID.
- `delete(uri)` deletes from the user's repo, and `upload_blob(bytes, mime_type)` returns the PDS's blob ref.
- `load(uri)` returns `{uri, did, collection, rkey, cid, indexed_at, record}`, or nil.
- `save_local(collection, rkey, tbl[, did])` and `delete_local(uri)` skip the PDS and write the index directly. They also work in label and record-event scripts, which have no user to act as (pass `save_local` a `did` there).
- `validate(collection, tbl)` normalizes and checks a record without writing it.
- `lexicon(collection)` returns the stored lexicon document, or nil.

Every write fills in `$type` and the lexicon's declared defaults. Required fields are checked unless you pass `validate = false` to `create` or `put`.

There's no `generate_rkey`. Use `require("internal.tids").create()`.

## Capabilities

- `caller:write`, to write to the calling user's own repo.
- `records:write`, to write into the local index.
- `records:read`, for `load` and `lexicon`.

Two of those write data, so it's High tier.
