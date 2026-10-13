You need HappyView v3 (this plugin's `api_version` is `"2"`) and an interpreter plugin (`happyview-lua`, `happyview-javascript` or `happyview-typescript`). Without one, no scripts run and nothing can call this library.

Installing takes one manifest URL. HappyView fetches the `.wasm` from the same place.

```
https://github.com/happyproto/plugins/releases/download/happyview-jobs-v1.1.0/manifest.json
```

That's 1.1.0, the latest release as I write this. Newer ones are on the [releases page](https://github.com/happyproto/plugins/releases).

## Install it

Pick one:

1. **Dashboard.** Go to **Settings > Plugins > Add Plugin**, paste the manifest URL, review it and click **Install Plugin**.
2. **Environment.** Set `PLUGIN_URLS` to `id|url`, optionally with `|sha256:<hash>` on the end. Separate multiple plugins with commas:
   ```
   PLUGIN_URLS=happyview-jobs|https://github.com/happyproto/plugins/releases/download/happyview-jobs-v1.1.0/manifest.json
   ```
3. **Filesystem.** Put `manifest.json` and `happyview-jobs.wasm` in a directory under `./plugins/`.

There are no secrets to configure. A job only does something if a script handles its type. Create that script in **Settings > Scripts > New** with **Job** as the trigger source, so its trigger is `job.run:<type>`. A job with no matching script fails as soon as the worker picks it up.

## Check it worked

It shows up on **Settings > Plugins**, and `require("happyview.jobs")` in a Lua script (or `import` from `"happyview.jobs"` in JavaScript or TypeScript) loads without an error.
