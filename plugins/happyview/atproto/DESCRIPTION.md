`happyview.atproto` is the AT Protocol toolbox for your scripts: resolving identities and services, downloading blobs, looking up labels, and signing records with your instance's attestation key. HappyView does the resolving, fetching and signing.

## Usage

```lua
local atproto = require("happyview.atproto")

function handle(input, ctx)
  local who = atproto.resolve_identity(input.actor)  -- {did, handle}
  local pds = atproto.resolve_service_endpoint(who.did)
  local labels = atproto.get_labels(input.uri)
  return {did = who.did, handle = who.handle, pds = pds, labels = labels}
end
```

## API

- `resolve_identity(identifier)` takes a handle or a DID and returns `{did, handle}`. `handle` is nil unless the handle and the DID document point at each other, so you never put an unverified handle on screen.
- `resolve_service_endpoint(did)` returns the service the DID document advertises (usually its PDS), or nil.
- `blob_download(did, cid)` returns `{bytes, mime_type, size}`. Bytes that aren't UTF-8 arrive as an array of byte values.
- `get_labels(uri)` returns an array of labels. `get_labels_batch(uris)` caps at 100 URIs and returns a table keyed by URI, with every requested URI present.
- `sign(record)` returns an inline signature object to attach to the record.
- `verify_signature(record, signature, repo_did)` returns a boolean.

`verify_signature` returns `false` only when it checked the signature and it didn't match. If it can't check (malformed bytes, a missing field, a record that won't encode), it raises `UNVERIFIABLE`.

## Capabilities

- `atproto:read`, for resolution, blobs and labels. It reaches any DID, any repo and any URI, not only the calling user's.
- `attest:sign`, for `sign` and `verify_signature`. A signature from `sign` says your instance vouches for that content.
