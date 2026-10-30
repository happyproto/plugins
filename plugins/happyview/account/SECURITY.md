## Reporting a vulnerability

Report it privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Please don't open a public issue for a vulnerability.

Only the latest release of `happyview-account` gets fixes, so check you're on it first.

## What this plugin can reach

It declares `caller:email` (High) and imports exactly `host_caller_get_email`. That reads the `email` and `emailConfirmed` fields of the calling user's own `com.atproto.server.getSession`, and nothing else. The host refuses a reply naming another account, and refuses a service-auth caller outright.

The plugin doesn't build requests or hold credentials. Like every HappyView plugin, it runs sandboxed in WASM with no filesystem access, and the loader refuses a module whose imports need more than its manifest declares.

## What's worth reporting

- An address returned for anyone but the calling user.
- An address returned with no caller, or to a service-auth caller.
- Any `getSession` field other than the two above reaching a script.
