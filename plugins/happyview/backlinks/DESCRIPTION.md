`happyview.backlinks` answers "what points at this?" Give it an AT URI and a collection, and it finds the indexed records whose strong refs point at that URI: the likes on a post, the replies to a thread, whatever your lexicons link together.

```lua
local backlinks = require("happyview.backlinks")

function handle(input, ctx)
  local page = backlinks
    .to(input.uri)
    :collection("app.bsky.feed.like")
    :limit(input.limit)
    :cursor(input.cursor)
    :run()

  -- each row.record is the liking record, not the post it points at
  return { likes = page.records, cursor = page.cursor }
end
```

In JavaScript and TypeScript it's the same chain behind `import backlinks from "happyview.backlinks"`, and `run()` returns a promise.

## Building a query

`to(uri)` starts the chain. Then:

- `collection(nsid)` is required. The host needs a collection to scope the search, so leaving it out fails before anything is sent.
- `did(did)` narrows to backlinks authored by one repo.
- `limit(n)` and `cursor(cursor)` page through results. The host caps the limit, and a page without a `cursor` is the last one.
- `run()` sends it and returns `{records, cursor}`.

Each record is an envelope, `{uri, did, collection, rkey, cid, indexed_at, record}`, where `record` is the referencing record's body exactly as stored. A `nil` step is skipped, so passing `input.limit` straight through is fine when the caller didn't send one.

## Capabilities

`records:read` only. It reads the index of record references and the records themselves. It can't write, make network requests or read secrets.
