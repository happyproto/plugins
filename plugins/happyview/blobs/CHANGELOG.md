## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-blobs-v1.0.0) (2026-10-08)

- **First release, on HappyView v3's plugin API.** Scripts get `happyview.blobs` with `put`, `get`, `stat` and `exists`, storing byte content under the CID of its own bytes. Storing the same content twice returns the same CID and keeps one copy, and content that isn't valid UTF-8 travels as an array of byte values.
