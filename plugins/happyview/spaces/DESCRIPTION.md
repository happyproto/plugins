Gives scripts the same access to permissioned spaces that HappyView's HTTP handlers have. A script can create a space, manage its members, read and write its records, and create invites. Policy checks, LtHash state and commit signing still happen in HappyView.

Spaces are experimental in HappyView, behind the `feature.spaces_enabled` setting.

## Usage

```lua
local spaces = require("happyview.spaces")

local space = spaces.create({spaceType = "app.example.thing", skey = "s1"})

local h = spaces.get(space.uri)
h:write_record({collection = "app.example.thing", record = {text = "hi"}})
h:add_member({did = "did:plc:abc", access = "write"})

local page = spaces.query({uri = space.uri, limit = 50})
```

## What it exposes

- `create`, `accept_invite`, `info(uri)` and `query` at the top level.
- `get(uri)` returns a handle for one space: `write_record`, `put_record`, `delete_record`, `add_member`, `set_member` (also `put_member`), `remove_member`, `update`, `delete`, `create_invite`, `members`, `is_member`, `access` and `records`.
- Writes always act as the calling user, with that user's own membership and access level. If the space is hosted on the user's PDS, a write needs their OAuth session. Without it the write fails with `NOT_AUTHORIZED`, since writing only the local copy would leave the two copies out of sync.
- Reads don't check a space's read policy. `spaces:read` sees every space's members and records, whoever runs the script.

## Capabilities

`spaces:read` and `spaces:write`, both **High tier**. Read covers `info`, `query`, `members`, `access`, `is_member` and `records`. Write covers everything else.
