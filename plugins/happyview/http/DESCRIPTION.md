`happyview.http` lets your HappyView scripts call other services over HTTP. It's six functions, one per method: `get`, `post`, `put`, `patch`, `delete` and `head`. Each takes a URL and an optional `opts` table and returns `{status, body, headers}`.

```lua
local http = require("happyview.http")
local json = require("internal.json")

function handle(input, ctx)
  local res = http.post("https://api.example.com/items", {
    headers = { ["content-type"] = "application/json" },
    body = json.encode({ title = input.title }),
  })
  if res.status ~= 201 then
    return { error = "UpstreamError", message = "items answered " .. res.status }
  end
  return json.decode(res.body)
end
```

In JavaScript or TypeScript, `delete` is a reserved word, so import it under another name: `import { delete as remove } from "happyview.http"`.

## How it behaves

- **4xx and 5xx responses don't raise.** Check `status` yourself. Redirects are followed.
- **Binary bodies arrive whole.** A response body that isn't valid UTF-8 comes back as `{"$bytes": "<base64>"}` rather than as text.
- **Header names come back lower-cased.** `res.headers["content-type"]` works however the server spelled it.
- **Tables get sent as JSON.** A table `body` or header value is sent as its JSON text, but no `content-type` is set for you, so add one. `body` is ignored on `get` and `head`.
- **The host enforces limits.** 100 requests per script run, 100 MB per response, and 500 MB transferred per run. Going over raises `HTTP_ERROR`, same as a DNS, TLS or timeout failure.

## Capabilities

It asks for `network:request:unrestricted`, which means it can reach any host your HappyView server can, internal services included. It has to, because the URL is the script's to choose. That makes it High tier: a script that calls it is as trusted as the operator who saved it.
