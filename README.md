# Plugins

WASM plugins for [HappyView](https://github.com/gamesgamesgamesgamesgames/happyview):
external-account auth, the libraries its scripts call, and the interpreters that
run those scripts.

## Development happens on `alpha`

HappyView v3 is in alpha, and so are these plugins. The work — every plugin, its
sources and its release configuration — lives on the [`alpha`](../../tree/alpha)
branch, which publishes `x.y.z-alpha.N` prereleases to the plugin registry.

`main` is reserved for the first stable release and carries nothing else until
then. Nothing installs from it.
