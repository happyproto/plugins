`happyview.blobs` gives your scripts somewhere to keep byte content, filed under the CID of the content itself. You hand it bytes and a media type and get a CID back. You never pick the key.

Storing the same content twice returns the same CID and keeps one copy. Nothing can be filed under a CID it doesn't hash to, so a CID from `put` is safe to publish as a checksum.

## Usage

```lua
local blobs = require("happyview.blobs")

function handle(input, ctx)
  local cid = blobs.put(input.body, "application/wasm")
  local info = blobs.stat(cid)   -- {cid, mime_type, size}, no transfer
  return {cid = cid, size = info.size}
end
```

You don't need `get` to serve the bytes from an XRPC endpoint. Declare the method's output encoding in its lexicon and return a blob ref, and the host sends the content without it passing through your script:

```lua
return {["$type"] = "blob", ref = {["$link"] = info.cid},
        mimeType = info.mime_type, size = info.size}
```

## API

- `put(bytes, mime_type)` stores content and returns its CID.
- `get(cid)` returns `{bytes, mime_type, size}`, or nil.
- `stat(cid)` returns `{cid, mime_type, size}`, or nil, without transferring anything.
- `exists(cid)` returns a boolean.

A JSON string can't hold arbitrary bytes, so bytes that aren't UTF-8 travel as an array of byte values, in both directions. `put` accepts a string or an array, and `get` returns a string only when the content is valid UTF-8. `stat` doesn't return bytes, so prefer it when you only need the size or type.

## Capabilities

- `blobs:write`, for `put`.
- `blobs:read`, for `get`, `stat` and `exists`.

They're separate capabilities because serving content and adding it are separate decisions. This plugin declares both.
