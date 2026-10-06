# happyview-atproto

Library plugin exposing `happyview.atproto`: AT Protocol service resolution,
blob download, label lookup, and attestation signing. Every export is a thin
translator over one SDK host wrapper; the host does the DID resolution,
network fetch, and signing.

## Lua

```lua
local atproto = require("happyview.atproto")

local pds = atproto.resolve_service_endpoint("did:plc:abc")     -- string or nil
local blob = atproto.blob_download("did:plc:abc", "bafy...")    -- {bytes, mime_type, size}
local labels = atproto.get_labels("at://did:plc:abc/app.bsky.feed.post/xyz")
local by_uri = atproto.get_labels_batch({"at://did:plc:a/x/1", "at://did:plc:b/x/2"})

local sig = atproto.sign(record)
local ok = atproto.verify_signature(record, sig, "did:plc:abc")
```

## Surface

- `resolve_service_endpoint(did)` — the AT Protocol service a DID's document
  advertises (its PDS, typically), or nil.
- `blob_download(did, cid)` — `{bytes, mime_type, size}`.
- `get_labels(uri)` — labels on one URI, as an array.
- `get_labels_batch(uris)` — labels on a set of URIs, as a table keyed by
  URI; every requested URI is present, possibly with an empty array.
- `sign(record)` — the inline signature object to attach to `record`.
- `verify_signature(record, signature, repo_did)` — boolean.

## Byte convention

`blob_download`'s `bytes` arrives as a Lua string when the blob is valid
UTF-8, and as an array of byte values otherwise — the same rule an HTTP
response body follows.

## Errors

- `BAD_INPUT` — a missing or malformed argument, or a `get_labels_batch`
  call over 100 URIs; chunk larger batches.
- `RESOLVE_ERROR` — `blob_download` could not resolve the DID, or the PDS it
  resolved to is not a public https host. `resolve_service_endpoint` never
  raises for an unresolvable DID; it returns `nil`.
- `BLOB_ERROR` — the PDS refused the blob fetch.
- `NO_SIGNER` — this instance has no attestation signer configured, from
  `sign` and `verify_signature` alike.
- `HOST_ERROR` — a database or transport failure inside the host.
- `UNVERIFIABLE` — `verify_signature` could not check the signature at all
  (malformed bytes, a missing field, a record that will not encode) and
  raises rather than returning `false`, because only `false` is a statement
  about the record — it means the signature was checked and does not match,
  while an error means no check happened.

## Capabilities

`atproto:read`, `attest:sign`. This library is **High tier**: `attest:sign`
mints a signature asserting this instance vouches for content, and
`atproto:read` reaches any DID's service endpoints, any repo's blobs, and any
URI's labels — not just the calling user's own.

Imports exactly `host_atproto_resolve_service`, `host_atproto_blob_download`,
`host_labels_get`, `host_attest_sign`, `host_attest_verify`.

## Build

```bash
cargo test -p happyview-atproto
cargo build --release --target wasm32-unknown-unknown -p happyview-atproto
```
