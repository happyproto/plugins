# happyview-blobs

Library plugin exposing `happyview.blobs`: store and read byte content
addressed by the CID of its own contents. Each export is a thin translator
over one SDK host wrapper; the host computes the CID, stores the row and
enforces which capability a call needs.

The key is the hash of the content, so a caller never chooses it. Storing the
same content twice returns the same CID and keeps one copy, and content
cannot be filed under a CID it does not hash to — which is what makes a
stored CID safe to publish as a checksum.

## Lua

```lua
local blobs = require("happyview.blobs")

function handle(input, ctx)
  local cid = blobs.put(input.body, "application/wasm")

  if blobs.exists(cid) then
    local info = blobs.stat(cid)          -- {cid, mime_type, size}, no transfer
    local blob = blobs.get(cid)           -- {bytes, mime_type, size}, or nil
    return {cid = cid, size = info.size, mime_type = blob.mime_type}
  end
end
```

Serving the bytes from an XRPC endpoint needs no `get`: declare the method's
output encoding in its lexicon and return a blob ref, and the host sends the
content without it passing through the script.

```lua
function handle(input, ctx)
  local info = blobs.stat(input.cid)
  if not info then return nil end
  return {
    ["$type"] = "blob",
    ref = { ["$link"] = info.cid },
    mimeType = info.mime_type,
    size = info.size,
  }
end
```

## Bytes and JSON

A Lua string is a byte string; a JSON string is not. Content that is not
valid UTF-8 therefore crosses as an array of byte values in both directions:
`put` accepts either shape, and `get` answers a string when the content is
valid UTF-8 and an array otherwise. `stat` avoids the question, which is one
reason to prefer it.

## Capabilities

| Capability | What it allows |
|---|---|
| `blobs:write` | `put` |
| `blobs:read` | `get`, `stat`, `exists` |

Reading and writing are separate because serving content and adding it are
separate decisions: a plugin that serves need not be able to add, and one
that ingests need not read back what others stored.

`exists` needs no import of its own — the SDK derives it from `stat`, so
there is no second answer to the same question that could disagree.
