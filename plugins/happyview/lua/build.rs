use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=src/lua_stubs.c");

    // The vendored Lua's release, which an artifact's engine stamp names.
    // Read from the header rather than written down here, so a `lua-src`
    // bump cannot leave artifacts stamped with the release it replaced.
    let include = PathBuf::from(
        std::env::var_os("DEP_LUA_INCLUDE")
            .expect("mlua-sys publishes the vendored Lua's include directory"),
    );
    let header = std::fs::read_to_string(include.join("lua.h")).expect("the vendored lua.h");
    let field = |name: &str| -> String {
        header
            .lines()
            .find_map(|line| {
                line.strip_prefix("#define ")?
                    .strip_prefix(name)?
                    .trim()
                    .strip_prefix('"')?
                    .strip_suffix('"')
                    .map(str::to_string)
            })
            .unwrap_or_else(|| panic!("lua.h defines {name}"))
    };
    println!(
        "cargo::rustc-env=HV_LUA_RELEASE={}.{}.{}",
        field("LUA_VERSION_MAJOR"),
        field("LUA_VERSION_MINOR"),
        field("LUA_VERSION_RELEASE"),
    );

    // The stubs exist to keep translation units out of the *module*; a native
    // build links the whole vendored Lua and opens no more of it, so it needs
    // none of this.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        return;
    }

    cc::Build::new()
        .file("src/lua_stubs.c")
        .include(&include)
        .warnings(false)
        .cargo_metadata(false)
        .compile("hv_lua_stubs");

    // Whole-archive, and emitted from a build script so cargo places it ahead
    // of both Lua and libc: the stubs displace libc's definitions only by
    // being linked before the archive members that would answer for them.
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    println!("cargo::rustc-link-search=native={out_dir}");
    println!("cargo::rustc-link-lib=static:+whole-archive=hv_lua_stubs");
}
