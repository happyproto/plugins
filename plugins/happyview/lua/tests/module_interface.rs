//! The built module's interface, which is the only place two properties are
//! observable: that `interpreter_plugin!` really emits the six names the host
//! resolves, and that vendoring PUC Lua has not linked a preview-1 import the
//! host refuses. Both sets are asserted against a fixed list rather than a
//! count, so a dependency that quietly adds an import fails here naming it.
//!
//! A test cannot drive the wasm build, so it skips when the module is absent.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wasmparser::{ExternalKind, Parser, Payload};

/// `memory` comes from the cdylib, the rest from `interpreter_plugin!`, and
/// this module exports nothing else at all.
///
/// The `Global` arm below exports nothing today. It is there because wasm-ld
/// publishes `__data_end` and `__heap_base` as globals on some cdylibs —
/// `crates/happyview-plugin-sdk/tests/exports.rs` accommodates exactly that —
/// so a link that started emitting them should not fail this on a layout
/// constant. Anything that is neither a function, a memory nor a global still
/// fails, naming it.
const EXPECTED_EXPORTS: &[&str] = &[
    "alloc",
    "dealloc",
    "execute",
    "memory",
    "plugin_info",
    "validate",
];

/// The four bridge imports — a call, a call started and a wait on started
/// ones for `internal.async`, and the surface — plus the four `script:host`
/// ones. An interpreter needs no other host function: everything a script can
/// reach arrives through `require`.
const ALLOWED_ENV_IMPORTS: &[&str] = &[
    "host_call_library",
    "host_call_library_start",
    "host_call_library_wait_any",
    "host_get_api_surface",
    "host_job_progress",
    "host_job_should_stop",
    "host_job_wait",
    "host_script_log",
];

/// What `wasi:clock`, `wasi:random` and `wasi:stdio` grant, plus the
/// lifecycle imports a wasi-libc build links that answer from an empty
/// context. Every other preview-1 import is refused at load, so `path_open`
/// absent is a consequence of this list rather than a separate assertion.
const ALLOWED_WASI_IMPORTS: &[&str] = &[
    "args_get",
    "args_sizes_get",
    "clock_res_get",
    "clock_time_get",
    "environ_get",
    "environ_sizes_get",
    "fd_fdstat_get",
    "fd_prestat_dir_name",
    "fd_prestat_get",
    "fd_write",
    "proc_exit",
    "random_get",
    "sched_yield",
];

fn module_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("HAPPYVIEW_LUA_WASM") {
        return PathBuf::from(explicit);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
        PathBuf::from,
    );
    target.join("wasm32-wasip1/release/happyview_lua.wasm")
}

#[test]
fn the_built_module_exports_the_interpreter_abi_and_imports_only_what_the_host_allows() {
    let path = module_path();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "skipping: {} not built. Run `cargo build --release -p happyview-lua \
             --target wasm32-wasip1` with the wasi-sdk environment set, or point \
             HAPPYVIEW_LUA_WASM at the module.",
            path.display()
        );
        return;
    };

    let mut exports = BTreeSet::new();
    let mut env_imports = BTreeSet::new();
    let mut wasi_imports = BTreeSet::new();
    let mut other_imports = BTreeSet::new();

    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.expect("the module should be valid wasm") {
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("valid export");
                    match export.kind {
                        ExternalKind::Func | ExternalKind::Memory => {
                            exports.insert(export.name.to_string());
                        }
                        ExternalKind::Global => {}
                        kind => panic!("unexpected export kind {kind:?} for {}", export.name),
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.expect("valid import");
                    match import.module {
                        "env" => env_imports.insert(import.name.to_string()),
                        "wasi_snapshot_preview1" => wasi_imports.insert(import.name.to_string()),
                        module => other_imports.insert(format!("{module}::{}", import.name)),
                    };
                }
            }
            _ => {}
        }
    }

    // Every violation is collected rather than asserted in turn, so one run
    // names all of them and each check is visibly load-bearing.
    let mut problems: Vec<String> = Vec::new();

    let expected: BTreeSet<String> = EXPECTED_EXPORTS.iter().map(|s| s.to_string()).collect();
    if exports != expected {
        problems.push(format!("exports are {exports:?}, expected {expected:?}"));
    }

    let allowed_env: BTreeSet<&str> = ALLOWED_ENV_IMPORTS.iter().copied().collect();
    for name in env_imports
        .iter()
        .filter(|n| !allowed_env.contains(n.as_str()))
    {
        problems.push(format!("env import the host does not define: {name}"));
    }

    let allowed_wasi: BTreeSet<&str> = ALLOWED_WASI_IMPORTS.iter().copied().collect();
    for name in wasi_imports
        .iter()
        .filter(|n| !allowed_wasi.contains(n.as_str()))
    {
        problems.push(format!("preview-1 import the host refuses: {name}"));
    }

    for name in &other_imports {
        problems.push(format!(
            "import from a module the host does not link: {name}"
        ));
    }

    assert!(
        problems.is_empty(),
        "{}:\n{}",
        path.display(),
        problems.join("\n")
    );
}
