# happyview-account

Library plugin exposing `happyview.account`: the calling script's user's email
address, as their own PDS reports it. A thin translator over one SDK host
wrapper; the host makes the call and owns the credentials.

It's a library of its own, not a function on `happyview.xrpc`, so that only
an instance that installs it is asked to consent to `caller:email`. The
caller's DID and handle are `require("happyview.xrpc").session()`.

## Lua

```lua
local account = require("happyview.account")

function handle(input, ctx)
  local address = account.email()
  if address.email and address.email_confirmed then
    -- …
  end
end
```

## Surface

- `email()` — `{email?, email_confirmed?}` from the caller's own
  `com.atproto.server.getSession`. Both are there only when the account has
  an address and the user's sign-in granted it, which the reference PDS does
  under `transition:email` or `account:email`. Their absence is not an error.

## Errors

- `NO_SESSION` — this script context has no caller (a query, label or
  record-event script, or a job without `auth`), or the caller is a
  service-auth token, which runs on the user's own sign-in to the instance
  rather than one the calling service holds.
- `AUTH_REQUIRED` / `PDS_ERROR` — the call reached the PDS and failed there.
  A `getSession` that names a different account is also `PDS_ERROR`.

## Capabilities

`caller:email` (High). Imports exactly `host_caller_get_email`.

## Build

```bash
cargo test -p happyview-account
cargo build --release --target wasm32-unknown-unknown -p happyview-account
```
