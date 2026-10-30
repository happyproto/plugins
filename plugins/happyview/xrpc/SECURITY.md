## Reporting a vulnerability

Report it privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Please don't open a public issue for a vulnerability.

Only the latest release of `happyview-xrpc` gets fixes, so check you're on it first.

## What this plugin can reach

It declares `caller:read` and `caller:call`, and the repo README puts it in the Critical tier. `caller:call` lets a script invoke *any* XRPC procedure as the user, including ones that change their account. Treat any script that calls it as trusted with the user's whole account.

The plugin doesn't build requests or hold credentials. It passes the method, input and params to the host, which resolves where the call goes and signs it as the caller. A procedure this instance doesn't serve is forwarded to the NSID's authority without the user's credentials.

It imports exactly `host_caller_xrpc_query`, `host_caller_get_session` and `host_caller_xrpc_procedure`. `session()` reads only the account fields of the caller's own `getSession`: DID, handle, and the email address when their session was granted `transition:email`. Like every HappyView plugin, it runs sandboxed in WASM with no filesystem access, and the loader refuses a module whose imports need more than its manifest declares.

## What's worth reporting

- A call that goes out as someone other than the calling user.
- The user's credentials reaching a destination they shouldn't, such as a forwarded procedure.
- `procedure` running with no caller instead of failing with `NO_SESSION`.
- A `query` with no session that gets more than an anonymous request should.
