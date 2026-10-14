You'll need HappyView v3 (plugin `api_version` `"2"`). No interpreter needed; HappyView calls auth plugins directly.

The current release's manifest is:

```
https://github.com/happyproto/plugins/releases/download/happyview-auth-microsoft-v1.0.0/manifest.json
```

HappyView fetches `happyview-auth-microsoft.wasm` from beside it. Newer versions are on the [releases page](https://github.com/happyproto/plugins/releases).

## Install it

Pick one of these:

1. **Dashboard.** Go to **Settings > Plugins**, click **Add Plugin**, paste the manifest URL and click **Install Plugin**.
2. **Environment variable.** Set `PLUGIN_URLS` before starting HappyView:

   ```
   PLUGIN_URLS=happyview-auth-microsoft|https://github.com/happyproto/plugins/releases/download/happyview-auth-microsoft-v1.0.0/manifest.json
   ```

   Add `|sha256:<hash>` to pin it; separate several plugins with commas.
3. **File system.** Put the release's `manifest.json` and `happyview-auth-microsoft.wasm` together in a directory under `./plugins/`.

## Set up the Azure app

Register an Azure AD (Entra) app with a client secret. The plugin signs in through the `consumers` endpoint with the `User.Read` and `offline_access` scopes, so the app has to accept personal Microsoft accounts. Add this as a redirect URI:

```
<your public URL>/external-auth/happyview-auth-microsoft/callback
```

One app can serve `happyview-auth-xbox` too; register both callbacks.

Enter the client ID and secret from the plugin's settings button on **Settings > Plugins**. That needs `TOKEN_ENCRYPTION_KEY` (`openssl rand -base64 32` makes one). Or set them in the environment:

```bash
PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_ID=<client id>
PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_SECRET=<client secret>
```

Dashboard secrets win over environment variables.

## Check it worked

It should show up under **Settings > Plugins**, and linking an account should land you back on HappyView with `auth=success` in the URL.
