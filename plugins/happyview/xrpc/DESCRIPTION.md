`happyview.xrpc` lets a script make XRPC calls as the user who called it. You name the method and pass the input. HappyView works out where the call goes, signs it as the caller and sends it. Your script never sees a token.

## Usage

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

## API

- `query(method[, params])` makes an XRPC query (GET).
- `procedure(method[, input[, params]])` makes an XRPC procedure call (POST).
- `session()` returns the calling user's account as their PDS reports it, `{did, handle?}`. Their email address is `happyview-account`'s `email()`, which needs its own capability.

Neither builds the request. Both pass the method, input and params to the host.

In a label or record-event script there's no caller, so `procedure` raises `NO_SESSION`. `query` falls back to an anonymous request where the destination allows one.

Failures from the far side (`AUTH_REQUIRED`, `PDS_ERROR`, `XRPC_ERROR`) come through unchanged, so you see what the PDS or the NSID's authority said.

## Capabilities

- `caller:read` and `caller:call`, to make calls as the calling user.

I'd treat this one carefully. `caller:call` lets a script invoke *any* XRPC procedure as the user, including ones that change their account. Only install it if you trust every script that will call it with the user's whole account. If all you need is to write records, `happyview-record` is one tier lower (High instead of Critical).
