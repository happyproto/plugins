//! `internal.test` and the test-file globals, for a run whose kind is `test`.

mod host;

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use happyview_plugin_sdk::{TestReport, TestStatus};
use mlua::{Function, Lua, Result as LuaResult, Table, Value};

use crate::convert;

#[cfg(test)]
pub use host::Recording;
pub use host::{Host, TestHost, Validating};

/// The innermost frame in a script or module: the line in the test file, or
/// in a module it imported, that called the assertion that failed. The
/// dashboard's chunk names no file, so it is not reported as one, and a stack
/// with no script frame on it places nothing.
pub(crate) fn location(lua: &Lua) -> (Option<String>, Option<u32>) {
    match crate::modules::caller(lua) {
        Ok(caller) => (
            Some(caller.name).filter(|name| name != crate::chunk::DEFAULT),
            caller.line.and_then(|line| u32::try_from(line).ok()),
        ),
        Err(_) => (None, None),
    }
}

/// A raised message's place and its text, read the way a run's own failure
/// is, against the chunks this run loaded.
pub(crate) fn split(lua: &Lua, text: &str) -> (Option<String>, Option<u32>, String) {
    let located = crate::chunk::locate(text, &crate::modules::known(lua));
    (located.chunk, located.line, located.message)
}

pub const MODULE: &str = "internal.test";

const PRELUDE: &str = include_str!("prelude.lua");

/// `=`-named, like the plugin's other chunks, so `chunk::name_of_source`
/// reads it as nobody's file and `location` steps over it.
const PRELUDE_NAME: &str = "=internal.test";

pub struct Suite {
    run: Function,
}

/// Install the globals and `internal.test` before the file's chunk runs,
/// since `describe` is called at file scope.
pub fn install(lua: &Lua, host: Rc<dyn TestHost>, chunk_name: &str) -> LuaResult<Suite> {
    let bindings = bindings(lua, host, chunk_name)?;
    let (run, module): (Function, Table) =
        lua.load(PRELUDE).set_name(PRELUDE_NAME).call(bindings)?;
    crate::require::preload(lua, MODULE, module)?;
    Ok(Suite { run })
}

impl Suite {
    /// Run every collected test and answer the counts.
    pub fn run(&self, lua: &Lua) -> LuaResult<serde_json::Value> {
        let counts: Value = self.run.call(())?;
        convert::to_json(lua, counts)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

fn status(name: &str) -> LuaResult<TestStatus> {
    match name {
        "pass" => Ok(TestStatus::Pass),
        "fail" => Ok(TestStatus::Fail),
        "skip" => Ok(TestStatus::Skip),
        other => Err(mlua::Error::runtime(format!("unknown test status {other}"))),
    }
}

fn host_error(label: &str, error: happyview_plugin_sdk::PluginError) -> mlua::Error {
    mlua::Error::runtime(format!("{label}: {}", error.message))
}

fn bindings(lua: &Lua, host: Rc<dyn TestHost>, chunk_name: &str) -> LuaResult<Table> {
    let t = lua.create_table()?;
    t.set("now", lua.create_function(|_, ()| Ok(now_millis()))?)?;
    t.set(
        "location",
        lua.create_function(|lua, ()| Ok(location(lua)))?,
    )?;
    t.set(
        "split",
        lua.create_function(|lua, text: String| Ok(split(lua, &text)))?,
    )?;

    let reporter = host.clone();
    let path = chunk_name.to_string();
    t.set(
        "report",
        lua.create_function(move |_, report: Table| {
            let report = TestReport {
                path: path.clone(),
                name: report.get("name")?,
                status: status(&report.get::<String>("status")?)?,
                message: report.get("message")?,
                file: report.get("file")?,
                line: report.get("line")?,
                duration_ms: report.get::<Option<u64>>("duration_ms")?.unwrap_or(0),
            };
            reporter
                .report(&report)
                .map_err(|e| host_error("reporting a test", e))
        })?,
    )?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "src/lib/a.test.lua";

    fn vm() -> Lua {
        let lua = crate::sandbox::tests::sandbox();
        crate::modules::install(&lua, FILE, Box::new(crate::modules::tests::Files::new(&[])))
            .unwrap();
        lua
    }

    #[test]
    fn location_is_the_innermost_project_frame() {
        let lua = vm();
        lua.globals()
            .set(
                "where_am_i",
                lua.create_function(|lua, ()| Ok(location(lua))).unwrap(),
            )
            .unwrap();
        let (file, line): (Option<String>, Option<u32>) =
            crate::chunk::load_source(&lua, FILE, "local x = 1\nreturn where_am_i()")
                .unwrap()
                .call(())
                .unwrap();
        assert_eq!((file.as_deref(), line), (Some(FILE), Some(2)));
    }

    #[test]
    fn an_error_splits_into_its_place_and_message() {
        let lua = vm();
        let err = crate::chunk::load_source(&lua, FILE, "local x = 1\nerror('boom')")
            .unwrap()
            .call::<()>(())
            .unwrap_err();
        assert_eq!(
            split(&lua, &err.to_string()),
            (Some(FILE.to_string()), Some(2), "boom".to_string())
        );
        for text in ["boom", "AUTH_ERROR: no session", "status:404: x"] {
            assert_eq!(split(&lua, text), (None, None, text.to_string()), "{text}");
        }
    }
}
