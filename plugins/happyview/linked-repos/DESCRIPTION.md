Lets your scripts write to repos that aren't the caller's. An admin links an atproto account to the instance over OAuth (dashboard: **Access > Linked Repos**) and picks its scopes. HappyView keeps the session refreshed in the background. This plugin is how a script acts through that grant.

I built Linked Repos for [Cartridge](https://cartridge.dev), which has to create verifications in its own repo and merge community contributions into a central one. Before this, that meant storing an app password and managing sessions in every script, and any script holding the password had full control of the account.

## Usage

```lua
local linked_repos = require("happyview.linked_repos")

local repo = linked_repos.get("did:plc:abc")
local ref = repo:create_record({
  collection = "app.bsky.feed.post",
  record = {text = "hi"},
})
```

## What it exposes

- `list()` returns every grant the plugin can act through, with `did`, `handle`, `reason`, `status` and `scopes`.
- `get(did)` returns a handle with `create_record`, `put_record`, `delete_record`, `upload_blob` and `call`.
- `get` never fails, because it only records the DID. An unlinked or revoked repo shows up as `NOT_LINKED` on the first method call.
- `call` makes any XRPC call through the linked repo, so the only thing enforcing scopes on it is that repo's PDS.

## Capabilities

`linked_repos:use`, which is **High tier**. Any script that can load this library can write through every repo an admin has linked, within each grant's scopes.
