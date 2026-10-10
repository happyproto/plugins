//! The built module's interface, which is the only place two properties are
//! observable: that `interpreter_plugin!` really emits the six names the host
//! resolves, and that vendoring QuickJS has not linked an import beyond the
//! ones this plugin means to make. All three sets are asserted equal to a
//! fixed list rather than within one, so an import that appears *or*
//! disappears fails here naming it — a dropped bridge import would mean a
//! script's calls are not reaching the host at all.
//!
//! A test cannot drive the wasm build, so it skips when the module is absent.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wasmparser::{ExternalKind, Parser, Payload};

/// `memory` comes from the cdylib, the rest from `interpreter_plugin!`, and
/// this module exports nothing else at all.
///
/// Globals are not counted: wasm-ld publishes `__data_end` and `__heap_base`
/// as globals on some cdylibs — `crates/happyview-plugin-sdk/tests/exports.rs`
/// accommodates exactly that — so a link that started emitting them should
/// not fail this on a layout constant. Anything that is neither a function, a
/// memory nor a global still fails, naming it.
const EXPECTED_EXPORTS: &[&str] = &[
    "alloc",
    "dealloc",
    "execute",
    "memory",
    "plugin_info",
    "validate",
];

/// The three bridge imports — a surface read, a call started, a wait for any
/// of them — and the four `script:host` ones. An interpreter needs no other
/// host function: everything a script can reach arrives through `import`.
const EXPECTED_ENV_IMPORTS: &[&str] = &[
    "host_call_library_start",
    "host_call_library_wait_any",
    "host_get_api_surface",
    "host_job_progress",
    "host_job_should_stop",
    "host_job_wait",
    "host_script_log",
];

/// Exactly what `wasi:clock`, `wasi:random` and `wasi:stdio` grant, and none
/// of the lifecycle imports a wasi-libc build would otherwise link:
/// `src/quickjs_stubs.c` says what displaces each.
const EXPECTED_WASI_IMPORTS: &[&str] = &["clock_time_get", "fd_write", "random_get"];

fn module_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("HAPPYVIEW_JAVASCRIPT_WASM") {
        return PathBuf::from(explicit);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
        PathBuf::from,
    );
    target.join("wasm32-wasip1/release/happyview_javascript.wasm")
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_built_module_exports_the_interpreter_abi_and_imports_exactly_what_it_means_to() {
    let path = module_path();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "skipping: {} not built. Run `cargo build --release -p happyview-javascript \
             --target wasm32-wasip1` with the wasi-sdk environment set, or point \
             HAPPYVIEW_JAVASCRIPT_WASM at the module.",
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
    for (what, got, expected) in [
        ("exports", &exports, set(EXPECTED_EXPORTS)),
        ("env imports", &env_imports, set(EXPECTED_ENV_IMPORTS)),
        (
            "preview-1 imports",
            &wasi_imports,
            set(EXPECTED_WASI_IMPORTS),
        ),
    ] {
        for name in got.difference(&expected) {
            problems.push(format!("unexpected {what}: {name}"));
        }
        for name in expected.difference(got) {
            problems.push(format!("missing {what}: {name}"));
        }
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
