This needs HappyView v3 (plugin `api_version` `"2"`). It doesn't need any other interpreter. HappyView v3 won't save or run a script until at least one interpreter is installed, so on a fresh v3 instance, install this (or `happyview-lua`, or `happyview-javascript`) first.

Library plugins like `happyview-db` and `happyview-http` are separate installs. The interpreter connects your scripts to whichever ones you've installed.

## Install

Installing takes one manifest URL. HappyView fetches the `.wasm` from the same place.

```
https://github.com/happyproto/plugins/releases/download/happyview-typescript-v1.0.0/manifest.json
```

Newer versions show up on the [releases page](https://github.com/happyproto/plugins/releases). Pick one of these three routes:

1. **Dashboard.** Go to **Settings > Plugins > Add Plugin**, paste the URL, review the permissions and click **Install Plugin**.
2. **Environment.** Set `PLUGIN_URLS` and restart:
   ```
   PLUGIN_URLS=happyview-typescript|https://github.com/happyproto/plugins/releases/download/happyview-typescript-v1.0.0/manifest.json
   ```
   The format is `id|url` or `id|url|sha256:<hash>`, comma-separated for more than one plugin.
3. **File system.** Put `manifest.json` and `happyview-typescript.wasm` in a directory under `./plugins/`.

There are no secrets or settings to configure.

## Check it worked

- It's listed under **Settings > Plugins**, or by `GET /admin/plugins?type=interpreter`.
- Boot stops logging `no interpreter plugin is installed`.
- TypeScript scripts under **Settings > Scripts** are no longer flagged as not runnable (`runnable` is `true` in the scripts list).
