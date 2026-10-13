## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of this plugin gets fixes.

## What this plugin can do

It declares `spaces:read` and `spaces:write`, both **High tier**. `spaces:read` reads every space's members and records, without checking the space's own read policy, so treat it as instance-wide read access to spaces. `spaces:write` creates spaces, accepts invites, and makes every mutating call on a space, always as the calling user with that user's own membership and access level.

It imports exactly the fifteen `host_spaces_*` functions. The loader refuses a module whose imports need more than it declares.

The plugin runs sandboxed in WASM with no filesystem access, and it only talks to HappyView through the host functions it imports. Policy enforcement, LtHash state and commit signing all stay in HappyView.

## What's worth reporting

- A write that succeeds without the caller having the membership or access level it needs.
- A write to a PDS-hosted space that lands locally instead of failing without the user's session.
- Any way to act on a space as a different user.
