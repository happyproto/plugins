## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for one.

Only the latest release of this plugin gets fixes.

## What this plugin can reach

It runs sandboxed in WASM with no filesystem access, and it declares five capabilities: `library:call`, `script:host`, `wasi:clock`, `wasi:random` and `wasi:stdio`. HappyView checks the module's imports against that list when it loads the plugin, and refuses it if they ask for more. That leaves it able to call installed libraries, act on its own run (log, report progress, check for a stop, wait), read the clock and secure randomness, and write to its own plugin log. Its imports are exactly the JavaScript plugin's; SWC's file loader is replaced with one that reads no files.

It has no network or database access of its own. Anything like that goes through a library plugin, which runs with its own permissions, not this one's.

After the transform, a script runs in the same QuickJS-ng sandbox as `happyview-javascript`: no `std` or `os` modules, no `eval`, `Function` constructors that throw, and `import` only for names the instance knows. The transform itself runs before the engine starts, so the engine's instruction and memory budgets don't cover it. The host's wall-clock limit and memory ceiling do.

## What's worth reporting

- A script escaping the sandbox: turning a string into code at run time, importing from a path or URL, or reaching file or process access.
- A script getting around a budget, including one built to make the transform run past the host's limits.
- A library plugin managing to shadow an `internal.` module.
- One script run seeing another run's `ctx`, job, data or in-flight calls.

An engine bug usually affects `happyview-javascript` too. One report covers both.
