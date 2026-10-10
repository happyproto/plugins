use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=src/quickjs_stubs.c");

    // The stubs exist to keep libc's translation units out of the *module*; a
    // native build is a test binary that may use its environment and streams
    // as it likes, so it needs none of this.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        return;
    }

    cc::Build::new()
        .file("src/quickjs_stubs.c")
        .warnings(false)
        .cargo_metadata(false)
        .compile("hv_quickjs_stubs");

    // Whole-archive, and emitted from a build script so cargo places it ahead
    // of both QuickJS and libc: the stubs displace libc's definitions only by
    // being linked before the archive members that would answer for them.
    // The archive travels bundled inside this crate's rlib, and rustc links
    // it whole into each plugin's module, so no plugin repeats this.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    println!("cargo::rustc-link-search=native={}", out_dir.display());
    println!("cargo::rustc-link-lib=static:+whole-archive=hv_quickjs_stubs");
}
