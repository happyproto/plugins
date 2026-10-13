## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of this plugin gets fixes.

## What this plugin can do

It declares one capability, `linked_repos:use`, which is **High tier**. With it, a script can write records and upload blobs through any repo an admin has linked, within that grant's scopes, and call any XRPC method through it, limited only by what the repo's own PDS allows.

It imports exactly six host functions: `host_linked_repos_list`, `host_linked_repo_create_record`, `host_linked_repo_put_record`, `host_linked_repo_delete_record`, `host_linked_repo_upload_blob` and `host_linked_repo_call`. The loader refuses a module whose imports need more than it declares.

The plugin runs sandboxed in WASM with no filesystem access, and it only talks to HappyView through the host functions it imports. It never sees the linked repos' credentials. HappyView looks up the grant, enforces its scopes and holds the PDS session.

## What's worth reporting

- A write or upload that lands outside a grant's scopes, or goes through a grant that's revoked or not linked.
- Anything that exposes a linked repo's session or tokens to a script.
- A way to act as a DID that no admin linked.
