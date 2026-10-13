You need HappyView v3 (this plugin's `api_version` is `"2"`) and an interpreter plugin (`happyview-lua`, `happyview-javascript` or `happyview-typescript`). Without one, no scripts run and nothing can call this library.

Installing takes one manifest URL. HappyView fetches the `.wasm` from the same place.

```
https://github.com/happyproto/plugins/releases/download/happyview-linked-repos-v1.0.0/manifest.json
```

That's 1.0.0, the latest release as I write this. Newer ones are on the [releases page](https://github.com/happyproto/plugins/releases).

## Install it

Pick one:

1. **Dashboard.** Go to **Settings > Plugins > Add Plugin**, paste the manifest URL, review it and click **Install Plugin**.
2. **Environment.** Set `PLUGIN_URLS` to `id|url`, optionally with `|sha256:<hash>` on the end. Separate multiple plugins with commas:
   ```
   PLUGIN_URLS=happyview-linked-repos|https://github.com/happyproto/plugins/releases/download/happyview-linked-repos-v1.0.0/manifest.json
   ```
3. **Filesystem.** Put `manifest.json` and `happyview-linked-repos.wasm` in a directory under `./plugins/`.

There are no secrets to configure, but you do need at least one linked repo. In the dashboard, go to **Access > Linked Repos**, choose **Link a repo**, pick the scopes, and authorize it. You can authorize inline or send the invite link to whoever owns the account. Until then, `list()` comes back empty.

## Check it worked

It shows up on **Settings > Plugins**, and `require("happyview.linked_repos")` in a Lua script (or `import` from `"happyview.linked_repos"` in JavaScript or TypeScript) loads without an error.
