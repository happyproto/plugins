## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-auth-steam` gets fixes.

## What this plugin can reach

`happyview-auth-steam` declares two capabilities, `network:request:unrestricted` and `secrets:read`. HappyView's loader refuses a module whose imports need more than its manifest declares, so this plugin can't touch records, the database or KV storage. Like every plugin, it runs sandboxed in WASM with no filesystem access.

HappyView loads secrets per plugin, so `secrets:read` gives this plugin the Steam API key and nothing belonging to any other plugin. The network capability is the broad one. It can send a request to any host the HappyView server can reach, though the code only ever talks to `steamcommunity.com` and `api.steampowered.com`.

HappyView itself generates the `state`, ties it to the user, and consumes it once on the callback before this plugin sees anything. The plugin's own job is proving the callback came from Steam, which it does with OpenID's `check_authentication` round trip.

## What's worth reporting

- A callback that gets accepted without Steam confirming it, or a way to link a SteamID you don't own (say, by tampering with `openid.claimed_id`).
- The Steam Web API key showing up anywhere it shouldn't, like logs or error messages. It travels in the `GetPlayerSummaries` query string, so URL logging counts.
- One user's SteamID or profile ending up on another user's account.
