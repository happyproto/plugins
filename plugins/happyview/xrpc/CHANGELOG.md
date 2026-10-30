## Unreleased

- **`session()`** returns the calling user's account as their PDS reports it: `{did, handle?}`. The email address is `happyview-account`'s `email()`, behind its own capability. Needs a HappyView with the `host_caller_get_session` import.

## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-xrpc-v1.0.0) (2026-10-07)

- **First release, on HappyView v3's plugin API.** Scripts get `happyview.xrpc` with `query` and `procedure`, which make XRPC calls as the calling user. HappyView resolves where each call goes and signs it, so the script never touches the user's credentials.
