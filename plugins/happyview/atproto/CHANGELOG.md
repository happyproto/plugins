## [1.1.0](https://github.com/happyproto/plugins/releases/tag/happyview-atproto-v1.1.0) (2026-10-10)

- **Resolve a handle or a DID with `resolve_identity`.** Pass either one and get back `{did, handle}`. `handle` is nil unless the handle and the DID document point at each other, so you never get an unverified handle to put on screen.

## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-atproto-v1.0.0) (2026-10-07)

- **First release, on HappyView v3's plugin API.** Scripts get `happyview.atproto` with `resolve_service_endpoint`, `blob_download`, `get_labels`, `get_labels_batch`, `sign` and `verify_signature`, covering service lookup for any DID, blobs from any repo, labels on any URI, and attestation signatures from your instance's key.
