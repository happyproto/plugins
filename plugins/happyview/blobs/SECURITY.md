## Reporting a vulnerability

Report it privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Please don't open a public issue for a vulnerability.

Only the latest release of `happyview-blobs` gets fixes, so check you're on it first.

## What this plugin can reach

It declares `blobs:read` and `blobs:write`, and nothing else: no secrets, no network, no records. Like every HappyView plugin, it runs sandboxed in WASM with no filesystem access, and the loader refuses a module whose imports need more than its manifest declares.

The plugin itself chooses no keys and stores nothing. It hands bytes and a media type to the host, and the host computes the CID and stores the row. Content can't be filed under a CID it doesn't hash to, so a stored CID is safe to publish as a checksum.

Because it declares both capabilities, any script that can call it can both add content and read back anything stored under a CID it knows.

## What's worth reporting

- A way to store content under a CID that doesn't match its bytes, or to overwrite what's already stored under a CID.
- `get` or `stat` returning content, a size or a media type that doesn't belong to the CID asked for.
- Bytes getting corrupted on the way through, for example in the string versus byte-array conversion.
