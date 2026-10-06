//! The VM a script runs in: the libraries it keeps, the globals it does not,
//! and the guard that turns a v2 global into a migration instruction.

use mlua::{Lua, LuaOptions, Result as LuaResult, StdLib};

/// The name every script chunk is loaded under. Lua renders an error in a
/// named chunk as `[string "script"]:12: message`, which is what the `line`
/// field is read out of; an unnamed chunk yields no line at all.
pub const CHUNK_NAME: &str = "script";

/// The libraries the sandbox keeps. `io`, `os`, `package` and `debug` are
/// never opened rather than opened and deleted, which is also what keeps
/// their C translation units — and the filesystem and process imports the
/// host refuses — out of the linked module. `os` is opened and cut down
/// instead, for the reason [`OS_SUBSET`] gives.
fn libraries() -> StdLib {
    StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH | StdLib::OS
}

/// The whole of `os` a script may have. An allow-list rather than a list of
/// deletions, so a function a later Lua adds to the library is absent until
/// someone names it here.
///
/// They are Lua's own. `os.time`'s normalisation, `os.date`'s specifiers and
/// both of their error messages are then PUC's rather than an imitation of
/// PUC's — which matters because an imitation cannot raise what PUC raises:
/// mlua wraps every error a Rust callback returns in an object of its own,
/// where Lua's C raises a plain string.
const OS_SUBSET: [&str; 3] = ["time", "date", "difftime"];

/// Base-library entry points a script may not reach. Loading a chunk at run
/// time escapes every check the save path made, and collecting by hand is a
/// way to make timings depend on when the collector ran.
const REMOVED_BASE_FUNCTIONS: [&str; 4] = ["load", "dofile", "loadfile", "collectgarbage"];

/// The error a read of a removed global raises. The wording is the host's:
/// a stored script that predates the codemod would otherwise fail as `attempt
/// to index a nil value` somewhere inside its own body, naming neither the
/// global nor the fix.
pub fn removed_global_message(name: &str) -> String {
    format!(
        "the '{name}' global was removed in v3; run the script codemod \
         (Settings → Scripts → Migrate, or happyview-codemod) -- see the \
         Migrating scripts guide"
    )
}

/// Build the VM. `removed_globals` arrives as data in the execute input, so
/// this guard and the host's codemod stay one list.
pub fn create(removed_globals: &[String]) -> LuaResult<Lua> {
    let lua = Lua::new_with(libraries(), LuaOptions::default())?;
    let globals = lua.globals();

    for name in REMOVED_BASE_FUNCTIONS {
        globals.raw_set(name, mlua::Value::Nil)?;
    }
    // `os.clock` is not in the subset: WASI preview 1 offers no process CPU
    // clock, and a function that always fails teaches a script author nothing.
    // `internal.time.now()` is what measures elapsed time.
    let stock: mlua::Table = globals.get("os")?;
    let os = lua.create_table()?;
    for name in OS_SUBSET {
        os.set(name, stock.get::<mlua::Value>(name)?)?;
    }
    globals.set("os", os)?;

    let removed: Vec<String> = removed_globals.to_vec();
    let guard = lua.create_table()?;
    guard.set(
        "__index",
        lua.create_function(
            move |_, (_globals, key): (mlua::Table, mlua::Value)| -> LuaResult<mlua::Value> {
                if let mlua::Value::String(name) = &key {
                    let name = name.to_str()?;
                    if removed.iter().any(|removed| removed == &*name) {
                        return Err(mlua::Error::runtime(removed_global_message(&name)));
                    }
                }
                // Every other unknown name is `nil`, as in plain Lua.
                Ok(mlua::Value::Nil)
            },
        )?,
    )?;
    // Only reads are guarded, so a script can still define helpers at file
    // scope and can shadow a removed name by assigning it.
    globals.set_metatable(Some(guard))?;

    Ok(lua)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The names the host sends today. Held here only so the tests have
    /// something to pass; the plugin itself never carries a list.
    const REMOVED: [&str; 32] = [
        "now",
        "log",
        "TID",
        "toarray",
        "json",
        "method",
        "input",
        "params",
        "caller_did",
        "collection",
        "delegate_did",
        "env",
        "space",
        "action",
        "uri",
        "did",
        "rkey",
        "record",
        "event",
        "job",
        "src",
        "val",
        "neg",
        "cts",
        "exp",
        "db",
        "http",
        "xrpc",
        "atproto",
        "linked_repos",
        "jobs",
        "Record",
    ];

    fn removed() -> Vec<String> {
        REMOVED.iter().map(|s| s.to_string()).collect()
    }

    pub(crate) fn sandbox() -> Lua {
        create(&removed()).expect("the sandbox should build")
    }

    #[test]
    fn the_libraries_a_script_may_not_reach_are_absent() {
        let lua = sandbox();
        for expression in [
            "io",
            "package",
            "debug",
            "require",
            "load",
            "dofile",
            "loadfile",
            "collectgarbage",
            "os.execute",
            "os.remove",
            "os.rename",
            "os.exit",
            "os.getenv",
            "os.tmpname",
            "os.setlocale",
            "os.clock",
        ] {
            let value: mlua::Value = lua
                .load(format!("return {expression}"))
                .eval()
                .unwrap_or_else(|e| panic!("{expression}: {e}"));
            assert!(value.is_nil(), "{expression} is still reachable");
        }
    }

    #[test]
    fn the_libraries_a_script_does_get_are_present() {
        let lua = sandbox();
        let ok: bool = lua
            .load(
                "return type(print) == 'function' \
                 and type(string.upper) == 'function' \
                 and type(table.concat) == 'function' \
                 and type(math.floor) == 'function' \
                 and type(utf8.char) == 'function' \
                 and type(coroutine.create) == 'function' \
                 and type(os.time) == 'function' \
                 and type(os.date) == 'function' \
                 and type(os.difftime) == 'function'",
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn every_removed_global_raises_the_migration_sentence() {
        let lua = sandbox();
        for name in REMOVED {
            let error = lua
                .load(format!("return {name}"))
                .eval::<mlua::Value>()
                .expect_err(name)
                .to_string();
            assert!(
                error.contains(&removed_global_message(name)),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn the_guard_carries_no_list_of_its_own() {
        let lua = create(&["only_this".to_string()]).unwrap();
        let error = lua
            .load("return only_this")
            .eval::<mlua::Value>()
            .expect_err("a name the host sent should raise")
            .to_string();
        assert!(
            error.contains(&removed_global_message("only_this")),
            "{error}"
        );
        // A name the host did not send is an ordinary unknown global, even
        // though the host happens to send it today.
        let value: mlua::Value = lua.load("return db").eval().unwrap();
        assert!(value.is_nil());
    }

    #[test]
    fn an_unrelated_unknown_global_is_nil() {
        let lua = sandbox();
        let value: mlua::Value = lua.load("return no_such_thing").eval().unwrap();
        assert!(value.is_nil());
        let value: mlua::Value = lua.load("return _G[42]").eval().unwrap();
        assert!(value.is_nil());
    }

    #[test]
    fn assigning_a_global_works_and_shadows_a_removed_name() {
        let lua = sandbox();
        let n: i64 = lua
            .load("helper_count = 3; function helper() return helper_count end; return helper()")
            .eval()
            .unwrap();
        assert_eq!(n, 3);
        let s: String = lua
            .load(r#"log = function(m) return "mine:" .. m end; return log("x")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "mine:x");
    }
}
