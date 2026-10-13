## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of `happyview-http` gets fixes.

## What this plugin can reach

`happyview-http` declares one capability, `network:request:unrestricted`, and imports exactly one host function, `host_http_request`. HappyView's loader refuses a module whose imports need more than its manifest declares, so this plugin can't read secrets, records, the database or KV storage. Like every plugin, it runs sandboxed in WASM with no filesystem access.

It can send a request to any host the HappyView server can reach, including internal services the public internet can't. It needs that because the URL is the script's to choose, and it's why I call it High tier. A script that calls it is as trusted as the operator who saved it. If you're writing a plugin that only talks to known hosts, declare `network:request` with `allowed_hosts` and call the host yourself rather than depending on this library.

HappyView enforces the per-run limits itself: 100 requests, 100 MB per response and 500 MB transferred.

## What's worth reporting

- A request going out without the plugin holding its network capability.
- A way past the per-run request or size limits.
- Request headers or bodies from one script run leaking into another's request or response.
- Anything that lets this module call a host import other than `host_http_request`.
