## Reporting a vulnerability

Report it privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Please don't open a public issue for a vulnerability.

Only the latest release of `happyview-record` gets fixes, so check you're on it first.

## What this plugin can reach

It declares `caller:write`, `records:read` and `records:write`, and the repo README puts it in the High tier, because two of those write data:

- **`caller:write`** writes to the calling user's own repo on their PDS (`create`, `put`, `delete`, `upload_blob`). The host owns the session; the plugin never sees credentials.
- **`records:write`** writes directly to this instance's index, skipping the PDS (`save_local`, `delete_local`). `save_local` takes an optional `did` and passes it to the host as given, so it isn't limited to the caller's repo.

`records:read` covers `load` and `lexicon`. The plugin imports exactly eight host functions, all listed in its README. Like every HappyView plugin, it runs sandboxed in WASM with no filesystem access, and the loader refuses a module whose imports need more than its manifest declares.

## What's worth reporting

- A caller-acting write landing in a repo that isn't the caller's, or running with no caller at all instead of failing with `NO_SESSION`.
- `put` with `swap_cid` replacing a record whose CID doesn't match.
- Required-field validation being skipped when `validate` wasn't set to `false`.
- Anything that lets a script write into the index outside what `save_local` and `delete_local` describe.
