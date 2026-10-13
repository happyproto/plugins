Lets people on your HappyView instance link their Xbox account, so you get their gamertag and XUID instead of a generic Microsoft identity. If you only need the Microsoft account itself, `happyview-auth-microsoft` is the simpler choice.

Scripts don't call this plugin. HappyView calls it through its `/external-auth` routes when a user links or refreshes an account.

## How it works

- **Sign-in goes through Microsoft.** `get_authorize_url` sends the user to Microsoft's `consumers` OAuth 2.0 endpoint asking for `XboxLive.signin XboxLive.offline_access`.
- **Only the Microsoft token is stored.** `handle_callback` trades the code for a Microsoft access token, refresh token and expiry, and `refresh_tokens` renews them with the refresh token.
- **Xbox Live tokens are fetched when they're needed.** Xbox Live wants two more exchanges before it'll tell you who someone is, so `get_profile` does them on every call. The Microsoft token becomes a user token at `user.auth.xboxlive.com`, and that becomes an XSTS token (retail sandbox) carrying the XUID and gamertag.
- **Profiles come from Xbox Live.** The account ID is the XUID, the display name is the gamertag, the avatar is `GameDisplayPicRaw`, and the profile URL points at `xbox.com`. If the profile settings call fails, it falls back to the XSTS gamertag with no avatar rather than failing the whole read.

## Capabilities

- `network:request:unrestricted`, to reach Microsoft's login endpoints and the Xbox Live auth and profile services.
- `secrets:read`, to read the Azure client ID and secret you configure.
