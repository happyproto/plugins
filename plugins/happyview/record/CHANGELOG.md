## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-record-v1.0.0) (2026-10-07)

- **First release, on HappyView v3's plugin API.** Scripts get `happyview.record`: `create`, `put`, `delete` and `upload_blob` as the calling user, `load` for indexed records, `save_local` and `delete_local` for writing the local index directly, and `validate` and `lexicon` for checking records against the lexicons your instance holds.
- **Your own writes show up right away.** `create`, `put` and `delete` mirror into the local index as soon as the PDS accepts them, so the next `load` sees them without waiting for the network to echo the record back.
