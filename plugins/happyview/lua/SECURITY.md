## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's [advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for one.

Only the latest release of this plugin gets fixes.

## What this plugin can reach

It runs sandboxed in WASM with no filesystem access, and it declares five capabilities: `library:call`, `script:host`, `wasi:clock`, `wasi:random` and `wasi:stdio`. HappyView checks the module's imports against that list when it loads the plugin, and refuses it if they ask for more. That leaves it able to call installed libraries, act on its own run (log, report progress, check for a stop, wait), read the clock and secure randomness, and write to its own plugin log.

It has no network or database access of its own. Anything like that goes through a library plugin, and each library runs with its own permissions, not this one's. So a script can only do what the libraries you've installed let it do.

Inside the guest, the VM is built without `io`, `package` and `debug`, so their C isn't even in the module. `load`, `dofile` and `loadfile` are gone, and `os` is an allow-list of `time`, `date` and `difftime`. The host enforces its own wall-clock limit and a second memory ceiling above the plugin's.

## What's worth reporting

- A script escaping the sandbox: running code from a string at run time, or reaching `io`, `os.execute` or anything else outside the allow-list.
- A script getting around a budget, like swallowing a spent instruction budget with `pcall`, `xpcall` or a coroutine and carrying on.
- A library plugin managing to shadow an `internal.` module.
- One script run seeing another run's `ctx`, job or data.
