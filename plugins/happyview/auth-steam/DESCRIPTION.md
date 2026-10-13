Lets people on your HappyView instance link their Steam account. Steam uses OpenID 2.0 instead of OAuth, so this one works differently from the other auth plugins. There's no token to pass around, only a verified SteamID and your Web API key.

Scripts don't call this plugin. HappyView calls it through its `/external-auth` routes when a user links or refreshes an account.

## How it works

- **Sign-in goes through Steam's OpenID page.** `get_authorize_url` sends the user to `steamcommunity.com/openid/login`, with HappyView's callback as `openid.return_to` and the `state` in its query string.
- **Every callback gets checked with Steam.** `handle_callback` POSTs the `openid.*` parameters back to Steam with `openid.mode=check_authentication` and refuses the link unless Steam answers `is_valid:true`.
- **The SteamID64 stands in for a token.** It's taken from `openid.claimed_id` and stored as the access token, with type `SteamID`. SteamIDs don't expire, so `refresh_tokens` returns the same one.
- **Profiles come from the Steam Web API.** `get_profile` calls `ISteamUser/GetPlayerSummaries/v2` with your API key and reports the SteamID, persona name, profile URL and full-size avatar.

## Capabilities

- `network:request:unrestricted`, to reach Steam's OpenID endpoint and `api.steampowered.com`.
- `secrets:read`, to read the Steam Web API key you configure.
