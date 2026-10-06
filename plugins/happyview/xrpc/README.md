# happyview-xrpc

Library plugin exposing `happyview.xrpc`: XRPC query and procedure calls as
the calling script's user. Both exports are a thin translator over one SDK
host wrapper each; the host builds the request and owns the credentials.

## Lua

```lua
local time = require("internal.time")
local xrpc = require("happyview.xrpc")

function handle(input, ctx)
  local profile = xrpc.query("app.bsky.actor.getProfile", {actor = "alice.test"})
  return xrpc.procedure("com.atproto.repo.createRecord", {
    repo = ctx.caller_did,
    collection = "app.bsky.feed.like",
    record = {subject = input.subject, createdAt = time.to_iso8601(time.now())},
  })
end
```

## Surface

- `query(method[, params])` — an XRPC query (GET), as the calling user.
- `procedure(method[, input[, params]])` — an XRPC procedure (POST), as the
  calling user.

Neither builds the request itself; both hand the method, input and query
parameters straight to the host, which resolves where the call goes and
signs it as the caller.

## Errors

- `BAD_INPUT` — a missing `method`, or a `params`/`input` that isn't an
  object. Raised by this crate before any host call.
- `NO_SESSION` — this script context has no caller to act as (a label or
  record-event script calling `procedure`; `query` tolerates this and falls
  back to an anonymous request where the destination allows one).
- `AUTH_REQUIRED` / `PDS_ERROR` / `XRPC_ERROR` — the call reached a PDS or an
  NSID's authority and failed there: no valid session, or the far side
  rejected the request. Passed through unchanged.

## Capabilities

`caller:read`, `caller:call`. This library is **Critical tier**:
`caller:call` lets a script invoke *any* XRPC procedure as the user,
including ones that change their account. A procedure this instance doesn't
serve is forwarded to the NSID's authority without the user's credentials.
Grant it only to scripts an operator trusts with the user's whole account.

Imports exactly `host_caller_xrpc_query`, `host_caller_xrpc_procedure`.

## Build

```bash
cargo test -p happyview-xrpc
cargo build --release --target wasm32-unknown-unknown -p happyview-xrpc
```
