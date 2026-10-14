You need HappyView v3 (plugin `api_version` `"2"`). This is a library plugin, so it does nothing on its own: scripts call it, and a script only runs when an interpreter plugin is installed (`happyview-lua`, `happyview-javascript` or `happyview-typescript`).

## Install

The whole install is one manifest URL. HappyView fetches the `.wasm` from beside it.

```
https://github.com/happyproto/plugins/releases/download/happyview-xrpc-v1.0.0/manifest.json
```

That's the current release. Newer ones show up on the [releases page](https://github.com/happyproto/plugins/releases), tagged `happyview-xrpc-v<version>`.

Pick one route:

1. **Dashboard.** Go to **Settings > Plugins**, click **Add Plugin**, paste the URL, review the details and click **Install Plugin**.
2. **Environment.** Set `PLUGIN_URLS` before HappyView starts:

   ```
   PLUGIN_URLS=happyview-xrpc|https://github.com/happyproto/plugins/releases/download/happyview-xrpc-v1.0.0/manifest.json
   ```

   The format is `id|url`, or `id|url|sha256:<hash>` to pin the download. Separate several plugins with commas.
3. **File system.** Put the release's `manifest.json` and `happyview-xrpc.wasm` in a directory under `./plugins/`, such as `./plugins/xrpc/`.

## Configuration

None. It needs no secrets of its own. Calls go out as the calling user with the session HappyView already holds for them, so there's nothing for you to hand it.

## Check it worked

The plugin is listed on **Settings > Plugins**, and `require("happyview.xrpc")` succeeds in a Lua script. If you installed through `PLUGIN_URLS` or the file system and it's missing, look in the boot log for `Failed to install plugin at boot`. HappyView logs a refused plugin there and starts without it.
