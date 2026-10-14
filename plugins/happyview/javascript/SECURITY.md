## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for one.

Only the latest release of this plugin gets fixes.

## What this plugin can reach

It runs sandboxed in WASM with no filesystem access, and it declares five capabilities: `library:call`, `script:host`, `wasi:clock`, `wasi:random` and `wasi:stdio`. HappyView checks the module's imports against that list when it loads the plugin, and refuses it if they ask for more. That leaves it able to call installed libraries, act on its own run (log, report progress, check for a stop, wait), read the clock and secure randomness, and write to its own plugin log. Running calls concurrently doesn't need a sixth, because it reaches nothing a script couldn't reach one call at a time.

It has no network or database access of its own. Anything like that goes through a library plugin, and each library runs with its own permissions, not this one's.

Inside the guest, QuickJS-ng's `std` and `os` modules aren't compiled in. `eval` is gone and all four `Function` constructors throw an `EvalError`. `import` only resolves names the instance knows, never a path or a URL. The host enforces its own wall-clock limit and a second memory ceiling above the engine's.

## What's worth reporting

- A script escaping the sandbox: turning a string into code at run time, importing from a path or URL, or reaching file or process access.
- A script getting around a budget, like catching a spent instruction budget and carrying on.
- A library plugin managing to shadow an `internal.` module.
- One script run seeing another run's `ctx`, job, data or in-flight calls.

This plugin is mostly the shared `happyview-quickjs` engine, so an engine bug usually affects `happyview-typescript` too. One report covers both.
