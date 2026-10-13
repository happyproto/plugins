Lets people on your HappyView instance link their itch.io account. It's the simplest of the auth plugins, with one OAuth 2.0 scope and one profile call.

Scripts don't call this plugin. HappyView calls it through its `/external-auth` routes when a user links or refreshes an account.

## How it works

- **Sign-in goes through itch.io.** `get_authorize_url` sends the user to `itch.io/user/oauth` asking for `profile:me`, so it can see who they are and nothing else.
- **The code becomes a token.** `handle_callback` exchanges it at `itch.io/api/1/oauth/token`.
- **Tokens don't expire.** No expiry is stored, and `refresh_tokens` returns the same token.
- **Profiles come from `/me`.** `get_profile` calls `itch.io/api/1/me` and reports the itch.io user ID, the display name (falling back to the username), the profile URL and the cover image as the avatar.

## Capabilities

- `network:request:unrestricted`, to reach itch.io's OAuth and API endpoints.
- `secrets:read`, to read the OAuth client ID and secret you configure.
