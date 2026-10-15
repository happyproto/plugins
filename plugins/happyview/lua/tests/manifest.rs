//! The published manifest, which the host reads before it ever loads the
//! module: the extensions a project file is matched to this interpreter by,
//! and the capabilities its imports need. `host_module_resolve` is under
//! `script:host`, so local modules add no capability; the test runner's
//! imports are `script:test`.

use serde_json::Value;

fn manifest() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("manifest.json"))
        .expect("valid JSON")
}

#[test]
fn a_lua_file_is_matched_to_this_interpreter_by_its_extension() {
    assert_eq!(manifest()["extensions"], serde_json::json!(["lua"]));
    assert_eq!(manifest()["language_id"], "lua");
}

#[test]
fn the_capabilities_are_exactly_what_the_imports_need() {
    assert_eq!(
        manifest()["capabilities"],
        serde_json::json!([
            "library:call",
            "script:host",
            "script:test",
            "wasi:clock",
            "wasi:random",
            "wasi:stdio"
        ])
    );
}
