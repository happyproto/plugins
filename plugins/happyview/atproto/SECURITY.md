## Reporting a vulnerability

Report it privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Please don't open a public issue for a vulnerability.

Only the latest release of `happyview-atproto` gets fixes, so check you're on it first.

## What this plugin can reach

It declares `atproto:read` and `attest:sign`, and the repo README puts it in the High tier:

- **`attest:sign`** mints a signature saying this instance vouches for content. Any script that can call `sign` can produce one.
- **`atproto:read`** reaches any DID's service endpoints, any repo's blobs and any URI's labels, not only the calling user's.

The plugin resolves nothing and signs nothing itself; the host does the DID resolution, fetching and signing, and the key never enters the plugin. `blob_download` refuses a PDS that isn't a public https host. Like every HappyView plugin, it runs sandboxed in WASM with no filesystem access, and the loader refuses a module whose imports need more than its manifest declares.

## What's worth reporting

- Anything that leaks the attestation private key, or gets a signature over content the script didn't pass to `sign`.
- `verify_signature` returning `true` for a forged, altered or replayed signature.
- `resolve_identity` returning a handle that wasn't verified in both directions.
- `blob_download` reaching a private or non-https host.
