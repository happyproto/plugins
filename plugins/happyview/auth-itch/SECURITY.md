## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-auth-itch` gets fixes.

## What this plugin can reach

`happyview-auth-itch` declares two capabilities, `network:request:unrestricted` and `secrets:read`. HappyView's loader refuses a module whose imports need more than its manifest declares, so this plugin can't touch records, the database or KV storage. Like every plugin, it runs sandboxed in WASM with no filesystem access.

HappyView loads secrets per plugin, so `secrets:read` gives this plugin its itch.io client ID and secret and nothing belonging to any other plugin. The network capability is the broad one. It can send a request to any host the HappyView server can reach, though the code only ever talks to `itch.io`.

HappyView itself generates the OAuth `state`, ties it to the user, and consumes it once on the callback before this plugin sees anything. HappyView stores the tokens the plugin returns, encrypted at rest.

## What's worth reporting

- The client secret or an itch.io token showing up anywhere it shouldn't, like logs or error messages. itch.io tokens don't expire, so a leaked one stays useful until the user revokes it.
- A way to link an itch.io account you don't own, or one user's profile ending up on another user's account.
- The plugin sending a token to any host other than itch.io.
