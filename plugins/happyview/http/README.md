# happyview-http

Library plugin exposing `happyview.http`: outbound HTTP requests. All six
exports are one thin translator over one SDK host wrapper; the host sends
the request and enforces the limits.

## Lua

```lua
local http = require("happyview.http")
local json = require("internal.json")

function handle(input, ctx)
  local page = http.get("https://api.example.com/items?limit=10")
  if page.status ~= 200 then
    return { error = "UpstreamError", message = "items answered " .. page.status }
  end

  local created = http.post("https://api.example.com/items", {
    headers = {
      ["content-type"] = "application/json",
      authorization = "Bearer " .. ctx.env.EXAMPLE_TOKEN,
    },
    body = json.encode({ title = input.title }),
  })
  return { items = json.decode(page.body), created = created.status == 201 }
end
```

## Surface

- `get(url[, opts])`, `post(url[, opts])`, `put(url[, opts])`,
  `patch(url[, opts])`, `delete(url[, opts])`, `head(url[, opts])` — send a
  request with that method and return `{status, body, headers}`.
- `opts.headers` — a table of header name to value. A non-string value is
  sent as its JSON text; a nil one as the empty string.
- `opts.body` — the request body, a string. A table is sent as its JSON
  text, but no `content-type` is set for it, so say so in `opts.headers`.
  Ignored on `get` and `head`.

In the result, `status` is the integer status code, `body` is the response
text (always `""` for `head`), and `headers` is keyed by **lower-cased**
header name, so `res.headers["content-type"]` works whatever the server
wrote. A response that isn't valid UTF-8 has its invalid bytes replaced.

A 4xx or 5xx response is a result, not an error: check `status`. Redirects
are followed.

## Errors

- `BAD_INPUT` — a missing or non-string `url`. Raised by this crate before
  any host call.
- `HTTP_ERROR` — the request never produced a response (DNS, connection,
  TLS, timeout), or it ran into a host limit: 100 requests per script run,
  100 MB per response, 500 MB transferred per script run.
- `FORBIDDEN` — the plugin lacks its network capability.
- `UNKNOWN_FUNCTION` — a method other than the six above.

## Capabilities

`network:request:unrestricted`. This library is **High tier**: it reaches
any host, including internal services the HappyView server can reach and
the public internet cannot. It has to be unrestricted because the URL is
the script's to choose, and a script that calls it is as trusted as the
operator who saved it. A plugin that only ever talks to known hosts should
declare `network:request` with `allowed_hosts` and call the host itself
rather than depend on this library.

Imports exactly `host_http_request`.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown -p happyview-http
```
