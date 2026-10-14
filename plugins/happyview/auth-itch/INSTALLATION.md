You'll need HappyView v3 (plugin `api_version` `"2"`). No interpreter needed; HappyView calls auth plugins directly.

The current release's manifest is:

```
https://github.com/happyproto/plugins/releases/download/happyview-auth-itch-v1.0.0/manifest.json
```

HappyView fetches `happyview-auth-itch.wasm` from beside it. Newer versions are on the [releases page](https://github.com/happyproto/plugins/releases).

## Install it

Pick one of these:

1. **Dashboard.** Go to **Settings > Plugins**, click **Add Plugin**, paste the manifest URL and click **Install Plugin**.
2. **Environment variable.** Set `PLUGIN_URLS` before starting HappyView:

   ```
   PLUGIN_URLS=happyview-auth-itch|https://github.com/happyproto/plugins/releases/download/happyview-auth-itch-v1.0.0/manifest.json
   ```

   Add `|sha256:<hash>` to pin it; separate several plugins with commas.
3. **File system.** Put the release's `manifest.json` and `happyview-auth-itch.wasm` together in a directory under `./plugins/`.

## Set up the itch.io app

Create an OAuth application in your itch.io account settings and note its client ID and secret. The plugin asks for the `profile:me` scope. Set its redirect URL to:

```
<your public URL>/external-auth/happyview-auth-itch/callback
```

Enter the client ID and secret from the plugin's settings button on **Settings > Plugins**. That needs `TOKEN_ENCRYPTION_KEY` (`openssl rand -base64 32` makes one). Or set them in the environment:

```bash
PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_ID=<client id>
PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_SECRET=<client secret>
```

Dashboard secrets win over environment variables.

## Check it worked

It should show up under **Settings > Plugins**, and linking an account should land you back on HappyView with `auth=success` in the URL.
