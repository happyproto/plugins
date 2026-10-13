You'll need HappyView v3 (plugin `api_version` `"2"`) and an interpreter plugin (`happyview-lua`, `happyview-javascript` or `happyview-typescript`). Without an interpreter, no script can run, so nothing can call this library.

This one is Critical tier (it can write to your database), so only install it if your scripts need it.

The current release's manifest is:

```
https://github.com/happyproto/plugins/releases/download/happyview-sql-v1.0.0/manifest.json
```

HappyView fetches `happyview-sql.wasm` from beside it. Newer versions show up on the [releases page](https://github.com/happyproto/plugins/releases).

## Install it

Pick one of these:

1. **Dashboard.** Go to **Settings > Plugins**, click **Add Plugin**, paste the manifest URL, review the details and click **Install Plugin**.
2. **Environment variable.** Set `PLUGIN_URLS` before starting HappyView:

   ```
   PLUGIN_URLS=happyview-sql|https://github.com/happyproto/plugins/releases/download/happyview-sql-v1.0.0/manifest.json
   ```

   You can pin it with `|sha256:<hash>` on the end, and list several plugins separated by commas.
3. **File system.** Download `manifest.json` and `happyview-sql.wasm` from the release and put them together in a directory under `./plugins/`, like `./plugins/happyview-sql/`.

There are no secrets to configure.

## Check it worked

The plugin should show up under **Settings > Plugins**. Then save a script that calls it, like `require("happyview.sql")` in Lua or `import sql from "happyview.sql"` in JavaScript.
