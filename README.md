# Plugins

WASM plugins, one directory per platform they plug into. [HappyView](https://github.com/gamesgamesgamesgamesgames/happyview) is the first: external-account auth, the libraries its scripts call, and the interpreters that run those scripts at all.

## Why this repository is separate

A HappyView v2 instance discovers installable plugins by reading GitHub releases from one hardcoded repository, [`gamesgamesgamesgamesgames/happyview-plugins`](https://github.com/gamesgamesgamesgamesgames/happyview-plugins) — not a setting, not an environment variable. The plugins here declare plugin API version `"2"`, which v2 refuses, so releasing them there would offer every v2 instance an install that cannot succeed. Releasing them from a repository v2 does not read makes them invisible to it by construction, with nothing resting on tag shapes or version parsing.

That other repository keeps serving the four auth plugins v2 instances already install from it.

This is also the bootstrap for the plugin registry. The registry is itself a HappyView v3 instance whose XRPC surface is Lua scripts, and a v3 instance runs no script until an interpreter is installed, so the registry cannot serve the interpreter it needs in order to function. Exactly one artefact has to come from outside the registry, through `PLUGIN_URLS`, which takes an identifier and a plain URL.

## Layout

```
plugins/
  happyview/
    atproto/   auth-itch/       auth-microsoft/  auth-steam/
    auth-xbox/ backlinks/       blobs/           db/
    http/      javascript/      jobs/            linked-repos/
    lua/       record/          spaces/          sql/
    xrpc/
```

A directory drops the platform prefix its crate and plugin id carry, because the platform directory already says it. Everything downstream — the host loader, the release tags, the installable artefacts — keys off `manifest.json` and the crate name, both of which keep the full `happyview-` prefix.

## Releases

Each plugin releases independently via semantic-release, configured in its own `.releaserc.json` and driven by `.github/workflows/release.yml`. Releases publish from `main` and from nowhere else: there is nothing here to keep away from a released channel, so there are no prerelease branches.

A release tags `<plugin id>-v<version>`, stamps that version into `manifest.json`, and attaches the built `.wasm` beside the stamped manifest. Those two files are what an instance installs.

## HappyView auth plugins

Auth plugins link an external account to a HappyView user. They authorize, exchange and refresh tokens, and report who the token belongs to; they do not ingest data.

| Plugin                     | Platform  | Auth Type | Capabilities                                   |
| -------------------------- | --------- | --------- | ----------------------------------------------- |
| `happyview-auth-steam`     | Steam     | OpenID    | `network:request:unrestricted`, `secrets:read` |
| `happyview-auth-xbox`      | Xbox      | OAuth2    | `network:request:unrestricted`, `secrets:read` |
| `happyview-auth-microsoft` | Microsoft | OAuth2    | `network:request:unrestricted`, `secrets:read` |
| `happyview-auth-itch`      | itch.io   | OAuth2    | `network:request:unrestricted`, `secrets:read` |

## HappyView library plugins

Library plugins expose functions to HappyView scripts (`require("<namespace>")` in Lua, `import` from `"<namespace>"` in JavaScript). They declare the capabilities they need in `manifest.json`; the HappyView loader refuses a plugin whose WASM imports need more than it declares.

| Plugin           | Namespace         | Capabilities                    | Provides                                                   |
| ---------------- | ----------------- | -------------------------------- | ---------------------------------------------------------- |
| `happyview-http` | `happyview.http`  | `network:request:unrestricted`  | `get`, `post`, `put`, `patch`, `delete`, `head`            |
| `happyview-db`   | `happyview.db`    | `records:read`                  | `records(collection)` builder, `get`, `search`, `backend` |
| `happyview-sql`  | `happyview.sql`   | `database:read`, `database:write` | `from(table)` builder, `raw(sql, params?)`               |
| `happyview-backlinks` | `happyview.backlinks` | `records:read`              | `to(uri)` builder                                          |
| `happyview-blobs` | `happyview.blobs` | `blobs:read`, `blobs:write` | `put(bytes, mime_type)`, `get(cid)`, `stat(cid)`, `exists(cid)` |
| `happyview-record` | `happyview.record` | `caller:write`, `records:read`, `records:write` | `create`, `put`, `delete`, `upload_blob`, `load`, `save_local`, `delete_local`, `validate` |
| `happyview-xrpc` | `happyview.xrpc` | `caller:read`, `caller:call` | `query(method, params?)`, `procedure(method, input?, params?)` |
| `happyview-atproto` | `happyview.atproto` | `atproto:read`, `attest:sign` | `resolve_service_endpoint`, `blob_download`, `get_labels`, `get_labels_batch`, `sign`, `verify_signature` |
| `happyview-linked-repos` | `happyview.linked_repos` | `linked_repos:use` | `list`, `get(did)` builder: `create_record`, `put_record`, `delete_record`, `upload_blob`, `call` |
| `happyview-jobs` | `happyview.jobs` | `jobs:create`, `jobs:read`, `jobs:read_any` | `create(job_type, input, opts?)`, `get(id)`, `get_any(id)`, `list_any(opts?)` |
| `happyview-spaces` | `happyview.spaces` | `spaces:read`, `spaces:write` | `create`, `accept_invite`, `info(uri)`, `query`, `get(uri)` builder: `write_record`, `put_record`, `delete_record`, `add_member`, `set_member`, `remove_member`, `update`, `delete`, `create_invite`, `members`, `is_member`, `access`, `records` |

Requires HappyView v3 (plugin API `api_version` `"2"`).

## HappyView interpreter plugins

An interpreter plugin is what runs a script at all, so a script's language is
an install rather than a release. It declares the language id that
`scripts.script_type` stores, exports `execute` and `validate` instead of
`call`, and bridges whatever libraries are installed rather than depending on
any.

| Plugin | Language | Capabilities | Target |
| --- | --- | --- | --- |
| `happyview-lua` | `lua` (PUC Lua 5.4.8) | `library:call`, `script:host`, `wasi:clock`, `wasi:random`, `wasi:stdio` | `wasm32-wasip1` |
| `happyview-javascript` | `javascript` (QuickJS-ng) | `library:call`, `script:host`, `wasi:clock`, `wasi:random`, `wasi:stdio` | `wasm32-wasip1` |

They are the only members of this workspace that are not pure Rust on
`wasm32-unknown-unknown`: each vendors an engine written in C — PUC Lua and
QuickJS-ng — which is compiled with clang from wasi-sdk 34.0, so they are kept
out of the workspace's default member set and have a CI job of their own. Each
one's README has the contract it gives a script and the environment it needs
to build.

## Writing a plugin with the SDK

`happyview-plugin-sdk` owns everything between a plugin and the host: the guest allocator, the packed-`i64` calling convention, the JSON envelope, and the `env` host imports. A plugin crate needs no `extern "C"` block, no raw pointers, and no `#[global_allocator]` — `library_plugin!` emits all of it, in the plugin crate, where the wasm linker reliably keeps the exports.

The SDK lives in the HappyView repo, not this one, at `crates/happyview-plugin-sdk`. Until it is published to crates.io, this workspace pulls it from the branch HappyView's v3 work lives on:

```toml
# root Cargo.toml
[workspace.dependencies]
happyview-plugin-sdk = { git = "https://github.com/gamesgamesgamesgamesgames/happyview", branch = "alpha" }
```

```toml
# plugins/happyview/<name>/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
happyview-plugin-sdk.workspace = true
```

Describe the plugin, its API surface, and how to dispatch a call. That is the whole contract:

```rust
#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use happyview_plugin_sdk::host::{self, HttpRequest};
use happyview_plugin_sdk::{
    library_plugin, ApiExport, ApiSurface, CallContext, PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-http", "HTTP Client", "1.0.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.http")
        .describe("Outbound HTTP requests")
        .export(
            ApiExport::function("get")
                .describe("Send a GET request")
                .param("url", "string", "Target URL"),
        )
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "get" => {
            let url = args
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| PluginError::bad_input("url is required"))?;
            let response = host::http_request(&HttpRequest::new("GET", url))?;
            Ok(happyview_plugin_sdk::json!({"status": response.status, "body": response.body}))
        }
        other => Err(PluginError::unknown_function(other)),
    }
}
```

The full `plugins/happyview/http` source is about a hundred lines.

The macro emits `alloc`, `dealloc`, `plugin_info`, `get_api_surface` and `call`, plus the bump allocator and the wasm `#[panic_handler]`. Pass `heap = <bytes>` as the first field to size the guest heap; it defaults to 512 KiB. A plugin that exports something other than the library ABI calls `export_abi!()` on its own and writes its exports by hand.

An auth plugin uses `auth_plugin!` instead, which emits `plugin_info` plus the four exports the external-account flow calls — `get_authorize_url`, `handle_callback`, `refresh_tokens` and `get_profile` — each with its own typed input and output (`AuthorizeUrlInput`, `CallbackInput`, `RefreshInput`, `TokenInput`, `TokenSet`, `ExternalProfile`). `CallbackInput::param` reads one callback query parameter, whatever the provider called it, so OAuth 2.0's `code` and OpenID 2.0's `openid.*` keys are read the same way. The four `plugins/happyview/auth-*` crates are working examples.

### Host functions and capabilities

`happyview_plugin_sdk::host` wraps all seventeen host imports. Each wrapper's doc comment names the capability the plugin's `manifest.json` must declare, and the HappyView loader refuses a plugin whose wasm imports need more than it declared. The linker drops an import along with the code that would have called it, so an unused wrapper costs nothing; the SDK's `tests/exports.rs` in the HappyView repo pins that.

| Wrapper | Import | Capability |
| ------- | ------ | ---------- |
| `log` / `debug` / `info` / `warn` / `error` | `host_log` | none |
| `get_secret` | `host_get_secret` | `secrets:read` |
| `http_request` | `host_http_request` | `network:request` or `network:request:unrestricted` |
| `kv_get` | `host_kv_get` | `kv:read` |
| `kv_set` / `kv_delete` | `host_kv_set` / `host_kv_delete` | `kv:write` |
| `lookup_record` | `host_lookup_record` | `records:read` |
| `call_library` / `library_surface` | `host_call_library` / `host_get_api_surface` | `library:call` |
| `records_query` | `host_records_query` | `records:read` |
| `records_count` | `host_records_count` | `records:read` |
| `records_get` | `host_records_get` | `records:read` |
| `records_search` | `host_records_search` | `records:read` |
| `backlinks_query` | `host_backlinks_query` | `records:read` |
| `blob_put` | `host_blob_put` | `blobs:write` |
| `blob_get` / `blob_stat` | `host_blob_get` / `host_blob_stat` | `blobs:read` |
| `table_query` | `host_table_query` | `database:read` |
| `db_query` | `host_db_query` | `database:read` or `database:write` |
| `db_execute` | `host_db_execute` | `database:write` |

Native builds compile the whole SDK, so a plugin's own logic is testable with `cargo test`; off wasm32 every host wrapper returns `HostError::NotWasm` rather than calling anything. The SDK's own test suite runs in the HappyView repo's CI, not here.

```bash
cargo build --release --target wasm32-unknown-unknown -p happyview-http
cargo clippy --target wasm32-unknown-unknown -p happyview-http -- -D warnings
```

## Installation

Download the `.wasm` file and its `manifest.json` from the plugin's [latest release](https://github.com/happyproto/plugins/releases) and place them in your HappyView plugins directory.

## Building from Source

Requirements:

- Rust with `wasm32-unknown-unknown` target

```bash
# Add WASM target if needed
rustup target add wasm32-unknown-unknown

# Build all plugins
cargo build --release --target wasm32-unknown-unknown

# Plugins will be in target/wasm32-unknown-unknown/release/*.wasm
```

`happyview-lua` and `happyview-javascript` are excluded from that build and need wasi-sdk; each one's own README has the environment.

## Configuration

Each plugin requires environment variables in HappyView with the prefix `PLUGIN_{PLUGIN_ID}_`, the plugin id upper-cased with every non-alphanumeric character replaced by `_`:

```bash
# Steam
PLUGIN_HAPPYVIEW_AUTH_STEAM_API_KEY=your_steam_api_key

# Xbox (Azure AD app with Xbox Live scopes)
PLUGIN_HAPPYVIEW_AUTH_XBOX_CLIENT_ID=your_azure_client_id
PLUGIN_HAPPYVIEW_AUTH_XBOX_CLIENT_SECRET=your_azure_client_secret

# Microsoft (can use same Azure app as Xbox)
PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_ID=your_azure_client_id
PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_SECRET=your_azure_client_secret

# itch.io
PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_ID=your_itch_client_id
PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_SECRET=your_itch_client_secret
```
