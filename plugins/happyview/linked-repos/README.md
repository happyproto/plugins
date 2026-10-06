# happyview-linked-repos

Library plugin exposing `happyview.linked_repos`: record writes, blob
uploads and XRPC calls through repos an admin has linked to this instance.
Every export is a thin translator over one SDK host wrapper each; the host
looks up the grant, enforces its scopes, and owns the PDS credentials.

## Lua

```lua
local linked_repos = require("happyview.linked_repos")

local grants = linked_repos.list()   -- {id, did, handle, reason, status, scopes}[]

local repo = linked_repos.get("did:plc:abc")
local ref = repo:create_record({collection = "app.bsky.feed.post", rkey = "3kabc", record = {text = "hi"}})
repo:put_record({collection = "app.bsky.feed.post", rkey = "3kabc", record = {text = "hi again"}})
repo:delete_record({collection = "app.bsky.feed.post", rkey = "3kabc"})
local blob = repo:upload_blob(bytes, "image/png")
local result = repo:call("com.atproto.repo.listRecords", {params = {collection = "app.bsky.feed.post"}})
```

## Surface

- `list()` — every grant this plugin may act through.
- `get(did)` — an object bound to one DID, with five immediate methods:
  - `create_record{collection, record, rkey?}` → `{uri, cid}`
  - `put_record{collection, rkey, record, swap_cid?}` → `{uri, cid}`
  - `delete_record{collection, rkey}` → `true`
  - `upload_blob(bytes, mime_type)` → the PDS's blob ref, as-is; `bytes` is a
    string or byte array
  - `call(nsid[, {params, input}])` → the response

`get` itself never fails — it just remembers the DID. Every method
re-resolves the grant fresh, so a script that wants a grant's `handle`,
`reason` or `status` reads them from `list()`; an unlinked or revoked DID
only ever surfaces as `NOT_LINKED` on the first method call against it.

## Errors

- `BAD_INPUT` — a missing or malformed argument, or a table missing a field
  a method requires, naming it. Raised by this crate before any host call.
- `NOT_LINKED` — no grant exists for this DID, or it's not in a usable state.
- `SCOPE` — the grant exists but doesn't cover this collection or method.
- `NEEDS_REAUTH` — the grant's session has expired and needs the admin to
  relink it.
- `FORBIDDEN` — the plugin lacks `linked_repos:use`.
- `PDS_ERROR` — the linked repo's PDS rejected the request.
- `HOST_ERROR` — a database or transport failure inside the host.

`call` has no scope pre-check of its own: it is the escape hatch to any XRPC
method, and only the linked repo's own PDS can enforce a scope against an
arbitrary NSID.

## Capabilities

`linked_repos:use`. This library is **High tier**: it can write records and
upload blobs through any repo an admin has linked, within that grant's
scopes, and call any XRPC method through it — constrained only by what the
repo's own PDS allows.

Imports exactly `host_linked_repos_list`, `host_linked_repo_create_record`,
`host_linked_repo_put_record`, `host_linked_repo_delete_record`,
`host_linked_repo_upload_blob`, `host_linked_repo_call`.

## Build

```bash
cargo test -p happyview-linked-repos
cargo build --release --target wasm32-unknown-unknown -p happyview-linked-repos
```
