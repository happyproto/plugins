You need HappyView v3 (plugin `api_version` `"2"`). This is a library plugin, so it does nothing on its own: scripts call it, and a script only runs when an interpreter plugin is installed (`happyview-lua`, `happyview-javascript` or `happyview-typescript`).

## Install

The whole install is one manifest URL. HappyView fetches the `.wasm` from beside it.

```
https://github.com/happyproto/plugins/releases/download/happyview-atproto-v1.1.0/manifest.json
```

That's the current release. Newer ones show up on the [releases page](https://github.com/happyproto/plugins/releases), tagged `happyview-atproto-v<version>`.

Pick one route:

1. **Dashboard.** Go to **Settings > Plugins**, click **Add Plugin**, paste the URL, review the details and click **Install Plugin**.
2. **Environment.** Set `PLUGIN_URLS` before HappyView starts:

   ```
   PLUGIN_URLS=happyview-atproto|https://github.com/happyproto/plugins/releases/download/happyview-atproto-v1.1.0/manifest.json
   ```

   The format is `id|url`, or `id|url|sha256:<hash>` to pin the download. Separate several plugins with commas.
3. **File system.** Put the release's `manifest.json` and `happyview-atproto.wasm` in a directory under `./plugins/`, such as `./plugins/atproto/`.

## Configuration

None required. `sign` and `verify_signature` use the instance's attestation key, which HappyView generates on first startup. To bring your own, set `ATTESTATION_PRIVATE_KEY` (hex, 32-byte secp256k1). If the key fails to load, both raise `NO_SIGNER`.

## Check it worked

The plugin is listed on **Settings > Plugins**, and `require("happyview.atproto")` succeeds in a Lua script. If it's missing after a `PLUGIN_URLS` or file system install, check the boot log for `Failed to install plugin at boot`.
