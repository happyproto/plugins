//! `internal.test` and the test-file globals, for a run whose kind is `test`.

mod host;

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use happyview_plugin_sdk::{
    TestCalls, TestMock, TestReport, TestStatus, TestTrigger, TestTriggerKind,
};
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

/// What a raised value says. An error a Rust function raised reaches Lua
/// wrapped, and its `tostring` prefixes the kind (`runtime error: `), which is
/// mlua's rendering rather than anything the test or the host wrote.
fn raised_text(lua: &Lua, raised: Value) -> LuaResult<String> {
    fn message(error: &mlua::Error) -> String {
        match error {
            mlua::Error::RuntimeError(message) => message.clone(),
            mlua::Error::CallbackError { cause, .. } => message(cause),
            other => other.to_string(),
        }
    }
    match raised {
        Value::Error(error) => Ok(message(&error)),
        other => lua.globals().get::<Function>("tostring")?.call(other),
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

fn trigger_label(kind: TestTriggerKind) -> &'static str {
    match kind {
        TestTriggerKind::Query => "test.query",
        TestTriggerKind::Procedure => "test.procedure",
        TestTriggerKind::RecordCreate => "test.record_create",
        TestTriggerKind::RecordUpdate => "test.record_update",
        TestTriggerKind::RecordDelete => "test.record_delete",
        TestTriggerKind::Job => "test.job",
        TestTriggerKind::Label => "test.label",
    }
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
        lua.create_function(|lua, raised: Value| Ok(split(lua, &raised_text(lua, raised)?)))?,
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

    let triggers = host.clone();
    t.set(
        "trigger",
        lua.create_function(move |lua, spec: Table| {
            let spec: TestTrigger =
                serde_json::from_value(convert::to_json(lua, Value::Table(spec))?)
                    .map_err(|e| mlua::Error::runtime(format!("test trigger: {e}")))?;
            let label = trigger_label(spec.kind);
            let result = triggers.trigger(&spec).map_err(|e| host_error(label, e))?;
            convert::library_result(lua, &serde_json::to_value(result).unwrap_or_default())
        })?,
    )?;

    let mocks = host.clone();
    t.set(
        "mock",
        lua.create_function(move |lua, (namespace, responses): (String, Value)| {
            let spec = TestMock {
                namespace,
                responses: convert::to_json(lua, responses)?,
            };
            mocks.mock(&spec).map_err(|e| host_error("test.mock", e))
        })?,
    )?;

    let calls = host.clone();
    t.set(
        "calls",
        lua.create_function(move |lua, namespace: String| {
            let found = calls
                .calls(&TestCalls { namespace })
                .map_err(|e| host_error("test.calls", e))?;
            convert::library_result(lua, &serde_json::to_value(found).unwrap_or_default())
        })?,
    )?;

    let writes = host;
    t.set(
        "pds_writes",
        lua.create_function(move |lua, ()| {
            let found = writes
                .pds_writes()
                .map_err(|e| host_error("test.pds_writes", e))?;
            convert::library_result(lua, &serde_json::to_value(found).unwrap_or_default())
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
