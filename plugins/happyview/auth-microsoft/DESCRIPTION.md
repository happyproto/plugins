Lets people on your HappyView instance link their personal Microsoft account. It's plain OAuth 2.0 against Microsoft's `consumers` endpoint, with the profile read from Microsoft Graph. If you want gamertags and XUIDs, that's `happyview-auth-xbox`; the two can share one Azure app.

Scripts don't call this plugin. HappyView calls it through its `/external-auth` routes when a user links or refreshes an account.

## How it works

- **Sign-in goes through Microsoft.** `get_authorize_url` asks for `User.Read offline_access`, which is enough to read the user's own profile and get a refresh token.
- **Tokens refresh normally.** `handle_callback` trades the code for an access token, refresh token and expiry, and `refresh_tokens` renews them with the refresh token.
- **Profiles come from Graph.** `get_profile` calls `graph.microsoft.com/v1.0/me` and reports the Graph user ID and display name, with the profile URL pointing at `account.microsoft.com/profile`.
- **No avatar.** Graph serves the profile photo from a separate call, which this plugin doesn't make, so `avatar_url` is always empty.

## Capabilities

- `network:request:unrestricted`, to reach Microsoft's login endpoints and Microsoft Graph.
- `secrets:read`, to read the Azure client ID and secret you configure.
