`happyview.account` lets a script read the email address of the user who called it, as their own PDS reports it. Your script never sees a token, and it gets the address only when the user's sign-in granted it.

## Usage

```lua
local account = require("happyview.account")

function handle(input, ctx)
  local address = account.email()
  return {email = address.email, confirmed = address.email_confirmed}
end
```

## API

- `email()` returns `{email?, email_confirmed?}`. Both are absent unless the account has an address and the user's sign-in granted it: the reference PDS releases it under `transition:email` or `account:email`. Absent is an answer, not an error.

With no caller (a query, label or record-event script, or a job without `auth`), or a caller authenticated by a service-auth token, `email()` raises `NO_SESSION`.

## Capabilities

- `caller:email`, to read the calling user's email address.

An email address is personal data. Install this only if your scripts need it, and remember that a job that returns it, or reports it as progress, stores it in the job's row.
