# happyview-spaces

Library plugin exposing `happyview.spaces`: read and write permissioned
spaces — records, membership and invites. Every export is a thin
translator over one SDK host wrapper each; the host owns policy
enforcement, LtHash state and commit signing.

## Lua

```lua
local spaces = require("happyview.spaces")

local space = spaces.create({spaceType = "app.example.thing", skey = "s1"})
local info = spaces.info(space.uri)             -- table, or nil if no such space
local page = spaces.query({uri = space.uri, limit = 50})

local h = spaces.get(space.uri)
h:write_record({collection = "app.example.thing", record = {text = "hi"}})
h:add_member({did = "did:plc:abc", access = "write"})
h:update({display_name = "New name"})
h:is_member("did:plc:abc")                      -- boolean
```

## Surface

- `create{spaceType, skey, display_name?, description?, read_policy?, write_policy?, app_access?, config?}` → space
- `accept_invite{token}` → space
- `info(uri)` → space, or nil
- `query{uri, collection?, limit?, cursor?}` → `{records, cursor}`
- `get(uri)` — an object bound to one space, with thirteen immediate methods:
  `write_record{collection, record}`, `put_record{collection, rkey, record, swap_cid?}`,
  `delete_record{collection, rkey, swap_cid?}` → `true`, `add_member{did, access?, is_delegation?}`,
  `set_member{did, access?, is_delegation?}`, `remove_member{did}` → `true`,
  `update{display_name?, description?, read_policy?, write_policy?, app_access?, config?}` → space,
  `delete()` → `true`, `create_invite{access?, max_uses?, expires_at?}`,
  `members()`, `is_member(did)` → boolean, `access(did)` → string or nil,
  `records{collection?, limit?, cursor?}`

`get` itself never fails — it just remembers the URI. A missing or
unusable URI is `NOT_FOUND` on the first method that needs the space to
exist, except `access`/`is_member`, which return nil/`false` instead; a
malformed URI is `BAD_INPUT`. `info` covers the nil-able lookup `get`
doesn't: check before committing to a space at all.

`update`'s `display_name`/`description` are patches — omit to leave alone,
`false`/`nil` to clear, a string to set. The policy fields (`read_policy`,
`write_policy`, `app_access`, `config`) are plain replacements: a script
always sends a complete document rather than editing one in place.

Every read (`info`, `query`, `members`, `access`, `records`, `is_member`)
needs no caller, and none check a space's read policy — `spaces:read` sees
every space's members and records regardless of who runs the script.
Every write acts strictly as the calling user, with that user's own
membership and access level, same as the HTTP handlers enforce. A write on
a space migrated to the user's own PDS needs that user's OAuth session;
without one it's `NOT_AUTHORIZED`, not a silent local write that forks the
two copies.

## Errors

- `BAD_INPUT` — a missing/malformed argument or field, naming it; raised
  before any host call.
- `NOT_FOUND` — no space exists at this URI, or no record/invite matched.
- `NOT_AUTHORIZED` — not a member, or lacks the access level needed.
- `CONFLICT` — `add_member` on an existing member, or a stale `swap_cid`.
- `PDS_ERROR` — a migrated space's PDS rejected the request.
- `FORBIDDEN` — the plugin lacks the capability the call needs.
- `SPACES_DISABLED` — the spaces feature isn't enabled on this instance.
- `HOST_ERROR` — a database or transport failure inside the host.

## Capabilities

Both `spaces:read` and `spaces:write` are **High tier**. `spaces:read`
reaches `info`, `query`, `members`, `access`, `is_member`, `records`,
unfiltered by any space's own read policy. `spaces:write` reaches
everything else, including `create`, `accept_invite` and every mutating
`get(uri)` method.

Imports exactly the fifteen `host_spaces_*` functions: `info`, `query`,
`members`, `access`, `create`, `accept_invite`, `write_record`,
`put_record`, `delete_record`, `add_member`, `set_member`,
`remove_member`, `update`, `delete`, `create_invite`.

## Build

```bash
cargo test -p happyview-spaces
cargo build --release --target wasm32-unknown-unknown -p happyview-spaces
```
