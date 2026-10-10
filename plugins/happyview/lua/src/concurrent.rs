//! `internal.async`: library calls that run together. Plain Lua stays
//! synchronous, and only a function handed to `async.all` has its library
//! calls started and waited on rather than made in turn.
//!
//! mlua offers a Rust function no way to yield without its `async` feature,
//! and a library call is a Rust function. So every library function and
//! immediate method the bridge builds is a Lua shim over two Rust entries — a
//! blocking call and a start that answers a handle — and it is the shim that
//! yields, with the scheduler below resuming it once the host says the call
//! has settled.

use std::cell::RefCell;
use std::collections::HashMap;

use happyview_plugin_sdk::{host, PluginError};
use mlua::{AppDataRef, Function, Lua, Result as LuaResult, Table, Value};
use serde_json::Value as Json;

use crate::budget;
use crate::convert;
use crate::require::plugin_error;

const SHIM_KEY: &str = "happyview.async.shim";

/// What the shim's frame is called in a traceback. Every library call passes
/// through it, `internal.async` or not, so it is named for what it is to a
/// script that never asked for concurrency. Lua prints it verbatim, with no
/// `[string "..."]` around it.
const CHUNK_NAME: &str = "[library bridge]";
const ALL_KEY: &str = "happyview.async.all";

/// The label a failure of the scheduler itself raises under, as opposed to a
/// failure of a call it was waiting on.
const LABEL: &str = "internal.async";

/// The shim and the scheduler, as one chunk so they share the set of threads
/// the scheduler made. Every global it uses is read once, when the run is set
/// up, so a script that reassigns `coroutine.yield` or `pcall` cannot change
/// how its library calls are made.
///
/// A library call yields only from a thread `all` created, and only where
/// that thread can yield at all — not from inside a `table.sort` comparator,
/// say. Everywhere else, a script's own coroutines included, it blocks as it
/// always has. Ownership is by identity rather than by "is this a coroutine",
/// because a script's own coroutine yielding a handle would hand it to the
/// script rather than to the scheduler.
///
/// Every function runs to its end before `all` raises, so nothing is left in
/// flight behind an error, and the error raised is the lowest-positioned one
/// whichever settled first. It is raised at level 0, so a string arrives
/// without a second position and a table arrives as the same table.
const SCHEDULER: &str = r##"
local resume, finish, wait = ...
local create, running, status = coroutine.create, coroutine.running, coroutine.status
local yield, isyieldable = coroutine.yield, coroutine.isyieldable
local pack, unpack, format = table.pack, table.unpack, string.format
local error, next, select, setmetatable, type = error, next, select, setmetatable, type

local owned = setmetatable({}, { __mode = "k" })
local PENDING = {}

local function shim(call, start)
    return function(...)
        if not owned[running()] or not isyieldable() then
            return call(...)
        end
        local handle = start(...)
        yield(PENDING, handle)
        return finish(handle)
    end
end

local function all(...)
    local n = select("#", ...)
    local threads = {}
    for i = 1, n do
        local f = (select(i, ...))
        if type(f) ~= "function" then
            error(format("bad argument #%d to 'all' (function expected, got %s)", i, type(f)), 2)
        end
        threads[i] = create(f)
        owned[threads[i]] = true
    end

    local results, failures, waiting = {}, {}, {}
    local function step(i)
        local thread = threads[i]
        local outcome = pack(resume(thread))
        if not outcome[1] then
            failures[i] = { outcome[2] }
        elseif status(thread) == "dead" then
            results[i] = outcome[2]
        elseif outcome[2] == PENDING then
            waiting[outcome[3]] = i
        else
            failures[i] = { "internal.async: a function passed to async.all may yield only through a library call" }
        end
    end

    for i = 1, n do
        step(i)
    end
    while next(waiting) ~= nil do
        local handle = wait(waiting)
        local i = waiting[handle]
        waiting[handle] = nil
        step(i)
    end

    for i = 1, n do
        if failures[i] then
            error(failures[i][1], 0)
        end
    end
    return unpack(results, 1, n)
end

return shim, all
"##;

/// What the bridge reaches the host through. A trait so the scheduler can be
/// driven natively: off wasm every host wrapper answers `NotWasm`, and the
/// order calls settle in is the one thing a test most needs to choose.
pub trait Backend {
    fn call(&self, library: &str, function: &str, args: &[Json]) -> Result<Json, PluginError>;
    fn start(&self, library: &str, function: &str, args: &[Json]) -> Result<u32, PluginError>;
    fn wait_any(&self, handles: &[u32]) -> Result<(u32, Result<Json, PluginError>), PluginError>;
}

struct Host;

impl Backend for Host {
    fn call(&self, library: &str, function: &str, args: &[Json]) -> Result<Json, PluginError> {
        host::call_library(library, function, args)
    }

    fn start(&self, library: &str, function: &str, args: &[Json]) -> Result<u32, PluginError> {
        host::call_library_start(library, function, args)
    }

    fn wait_any(&self, handles: &[u32]) -> Result<(u32, Result<Json, PluginError>), PluginError> {
        host::call_library_wait_any(handles)
    }
}

/// A started call: the label its failure raises under, and its outcome once
/// the host has settled it and until the shim collects it.
struct Started {
    label: String,
    outcome: Option<Result<Json, PluginError>>,
}

pub struct Bridge {
    backend: Box<dyn Backend>,
    started: RefCell<HashMap<u32, Started>>,
}

/// Install the bridge for a run: the host as its backend, and the shim and
/// scheduler built over the catch guard `budget` left for `coroutine.resume`.
/// Resuming through that guard is what keeps a spent budget inside a
/// scheduled function from being recorded as one more failure while its
/// siblings run on. A VM with no guard has no budget to protect.
pub fn install(lua: &Lua) -> LuaResult<()> {
    use_backend(lua, Box::new(Host));
    let resume = match lua.named_registry_value::<Option<Function>>(budget::RESUME_KEY)? {
        Some(guarded) => guarded,
        None => lua.globals().get::<Table>("coroutine")?.get("resume")?,
    };
    let finish = lua.create_function(finish)?;
    let wait = lua.create_function(wait)?;
    let (shim, all): (Function, Function) = lua
        .load(SCHEDULER)
        .set_name(format!("={CHUNK_NAME}"))
        .call((resume, finish, wait))?;
    lua.set_named_registry_value(SHIM_KEY, shim)?;
    lua.set_named_registry_value(ALL_KEY, all)
}

pub fn use_backend(lua: &Lua, backend: Box<dyn Backend>) {
    lua.set_app_data(Bridge {
        backend,
        started: RefCell::new(HashMap::new()),
    });
}

fn bridge(lua: &Lua) -> LuaResult<AppDataRef<'_, Bridge>> {
    lua.app_data_ref::<Bridge>()
        .ok_or_else(|| mlua::Error::runtime("the library bridge is not installed"))
}

/// The function a script sees for one library entry point: `call` when the
/// running thread is not the scheduler's, `start` and a yield when it is.
pub fn shim(lua: &Lua, call: Function, start: Function) -> LuaResult<Function> {
    lua.named_registry_value::<Function>(SHIM_KEY)?
        .call((call, start))
}

/// The `internal.async` module.
pub fn module(lua: &Lua) -> LuaResult<Table> {
    let module = lua.create_table()?;
    module.set("all", lua.named_registry_value::<Function>(ALL_KEY)?)?;
    Ok(module)
}

/// A blocking call, which is every call made outside a scheduled function.
pub fn call(
    lua: &Lua,
    library: &str,
    function: &str,
    label: &str,
    args: &[Json],
) -> LuaResult<Value> {
    let outcome = bridge(lua)?.backend.call(library, function, args);
    settle(lua, label, outcome)
}

/// Start a call and answer its handle. A refusal to start is raised here, at
/// the call site, exactly as a blocking call's failure would be.
pub fn start(
    lua: &Lua,
    library: &str,
    function: &str,
    label: &str,
    args: &[Json],
) -> LuaResult<Value> {
    let bridge = bridge(lua)?;
    let handle = bridge
        .backend
        .start(library, function, args)
        .map_err(|e| plugin_error(label, e))?;
    bridge.started.borrow_mut().insert(
        handle,
        Started {
            label: label.to_string(),
            outcome: None,
        },
    );
    Ok(Value::Integer(handle.into()))
}

/// Block until one of the handles `all` is waiting on settles, keep its
/// outcome for the shim to collect, and answer which it was. The handles go
/// to the host in ascending order so a run is reproducible from its inputs.
fn wait(lua: &Lua, waiting: Table) -> LuaResult<u32> {
    let mut handles = waiting
        .pairs::<u32, Value>()
        .map(|pair| pair.map(|(handle, _)| handle))
        .collect::<LuaResult<Vec<u32>>>()?;
    handles.sort_unstable();
    let bridge = bridge(lua)?;
    let (handle, outcome) = bridge
        .backend
        .wait_any(&handles)
        .map_err(|e| plugin_error(LABEL, e))?;
    match bridge.started.borrow_mut().get_mut(&handle) {
        Some(started) if handles.contains(&handle) => started.outcome = Some(outcome),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "{LABEL}: the host settled call {handle}, which was not being waited on"
            )))
        }
    }
    Ok(handle)
}

/// The settled call's value, or its failure raised as a blocking call raises
/// it. Runs on the thread that made the call, so that is where it raises.
fn finish(lua: &Lua, handle: u32) -> LuaResult<Value> {
    let started = {
        let bridge = bridge(lua)?;
        let mut started = bridge.started.borrow_mut();
        match started.get(&handle) {
            Some(Started {
                outcome: Some(_), ..
            }) => started.remove(&handle),
            // Reachable only by a script resuming one of the scheduler's
            // threads itself, through `coroutine.running`.
            _ => None,
        }
    };
    match started {
        Some(Started {
            label,
            outcome: Some(outcome),
        }) => settle(lua, &label, outcome),
        _ => Err(mlua::Error::runtime(format!(
            "{LABEL}: a library call was resumed before its result arrived"
        ))),
    }
}

fn settle(lua: &Lua, label: &str, outcome: Result<Json, PluginError>) -> LuaResult<Value> {
    match outcome {
        Ok(value) => convert::library_result(lua, &value),
        Err(e) => Err(plugin_error(label, e)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use happyview_plugin_sdk::{ApiExport, ApiMethod, ApiSurface, ExecuteLimits};
    use mlua::MultiValue;

    use super::*;
    use crate::require;
    use crate::sandbox::tests::sandbox;

    type Log = Rc<RefCell<Vec<String>>>;

    /// A host that records what it was asked and settles the newest
    /// outstanding call first, which is the order that most disagrees with
    /// the order calls were made in.
    struct Fake {
        log: Log,
        next: Cell<u32>,
        started: RefCell<HashMap<u32, (String, Vec<Json>)>>,
    }

    /// `echo` answers its first argument and `fail` fails with it; the
    /// `records` constructor answers its own first argument.
    fn answer(function: &str, args: &[Json]) -> Result<Json, PluginError> {
        match function {
            "fail" => Err(PluginError::new(
                "BOOM",
                args[0].as_str().unwrap_or_default(),
            )),
            "records" => Ok(args[0]["args"][0].clone()),
            _ => Ok(args.first().cloned().unwrap_or(Json::Null)),
        }
    }

    impl Backend for Fake {
        fn call(&self, _: &str, function: &str, args: &[Json]) -> Result<Json, PluginError> {
            self.log.borrow_mut().push(format!("call {function}"));
            answer(function, args)
        }

        fn start(&self, _: &str, function: &str, args: &[Json]) -> Result<u32, PluginError> {
            self.log.borrow_mut().push(format!("start {function}"));
            let handle = self.next.get() + 1;
            self.next.set(handle);
            self.started
                .borrow_mut()
                .insert(handle, (function.to_string(), args.to_vec()));
            Ok(handle)
        }

        fn wait_any(
            &self,
            handles: &[u32],
        ) -> Result<(u32, Result<Json, PluginError>), PluginError> {
            let handle = *handles.iter().max().expect("a wait names a handle");
            self.log.borrow_mut().push(format!("settle {handle}"));
            let (function, args) = self.started.borrow_mut().remove(&handle).unwrap();
            Ok((handle, answer(&function, &args)))
        }
    }

    fn surface() -> ApiSurface {
        let mut records = ApiExport::constructor("records");
        records.methods = vec![ApiMethod::lazy("limit"), ApiMethod::immediate("all")];
        ApiSurface::new("test.lib")
            .export(ApiExport::function("echo"))
            .export(ApiExport::function("fail"))
            .export(records)
    }

    /// A run's VM in the order `execute` builds it, with the fake behind the
    /// bridge and `lib` and `async` as globals.
    fn vm() -> (Lua, Log) {
        vm_with(None)
    }

    fn vm_with(instructions: Option<u32>) -> (Lua, Log) {
        let lua = sandbox();
        budget::install(
            &lua,
            &ExecuteLimits {
                instructions,
                memory_bytes: 0,
            },
        )
        .unwrap();
        require::install(&lua, &[]).unwrap();
        let log = Log::default();
        use_backend(
            &lua,
            Box::new(Fake {
                log: Rc::clone(&log),
                next: Cell::new(0),
                started: RefCell::new(HashMap::new()),
            }),
        );
        let lib = require::build_module(&lua, "test-lib", &surface()).unwrap();
        lua.globals().set("lib", lib).unwrap();
        let async_module: Table = lua.load("return require('internal.async')").eval().unwrap();
        lua.globals().set("async", async_module).unwrap();
        (lua, log)
    }

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, source: &str) -> T {
        lua.load(source)
            .eval()
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }

    fn error_of(lua: &Lua, source: &str) -> String {
        lua.load(source)
            .eval::<MultiValue>()
            .expect_err(source)
            .to_string()
    }

    fn log_of(log: &Log) -> Vec<String> {
        log.borrow().clone()
    }

    #[test]
    fn results_come_back_in_argument_order_whatever_order_calls_settle_in() {
        let (lua, log) = vm();
        let joined: String = eval(
            &lua,
            "local a, b, c = async.all( \
                 function() return lib.echo('a') end, \
                 function() return lib.echo('b') end, \
                 function() return lib.records('c'):limit(1):all() end) \
             return a .. b .. c",
        );
        assert_eq!(joined, "abc");
        assert_eq!(
            log_of(&log),
            [
                "start echo",
                "start echo",
                "start records",
                "settle 3",
                "settle 2",
                "settle 1"
            ]
        );
    }

    #[test]
    fn a_function_making_calls_in_turn_waits_for_each() {
        let (lua, log) = vm();
        let joined: String = eval(
            &lua,
            "local a, b = async.all( \
                 function() local x = lib.echo('x') return x .. lib.echo('y') end, \
                 function() return lib.echo('b') end) \
             return a .. b",
        );
        assert_eq!(joined, "xyb");
        assert_eq!(
            log_of(&log),
            [
                "start echo",
                "start echo",
                "settle 2",
                "settle 1",
                "start echo",
                "settle 3"
            ]
        );
    }

    #[test]
    fn functions_without_library_calls_and_no_functions_at_all_run_straight_through() {
        let (lua, log) = vm();
        let (none, two, a, b, c): (usize, usize, i64, Value, i64) = eval(
            &lua,
            "local a, b, c = async.all( \
                 function() return 1 end, \
                 function() end, \
                 function() return 3, 4 end) \
             return select('#', async.all()), \
                 select('#', async.all(function() end, function() end)), a, b, c",
        );
        assert_eq!((none, two, a, c), (0, 2, 1, 3));
        assert!(b.is_nil());
        assert!(log_of(&log).is_empty());
    }

    #[test]
    fn an_argument_that_is_not_a_function_is_named_by_its_position() {
        let (lua, _) = vm();
        let error = error_of(&lua, "async.all(function() end, 5)");
        assert!(
            error.contains("bad argument #2 to 'all' (function expected, got number)"),
            "{error}"
        );
    }

    /// The failure a scheduled call raises is the one a blocking call raises,
    /// and it raises where the call was made, so `pcall` catches it there.
    #[test]
    fn a_library_error_raises_at_the_call_site_and_pcall_catches_it() {
        let (lua, _) = vm();
        let (sync, scheduled, wrapped): (String, String, String) = eval(
            &lua,
            "local _, sync = pcall(lib.fail, 'caught') \
             local scheduled, wrapped = async.all( \
                 function() local ok, e = pcall(lib.fail, 'caught') \
                     assert(not ok) return tostring(e) end, \
                 function() local ok, e = pcall(function() return lib.fail('caught') end) \
                     assert(not ok) return tostring(e) end) \
             return tostring(sync), scheduled, wrapped",
        );
        let message = "test-lib.fail: Plugin returned error: BOOM - caught";
        for seen in [&sync, &scheduled, &wrapped] {
            assert!(seen.contains(message), "{seen}");
        }
    }

    #[test]
    fn an_uncaught_failure_is_raised_by_all_and_the_lowest_position_wins() {
        let (lua, log) = vm();
        let error = error_of(
            &lua,
            "async.all( \
                 function() return lib.echo('fine') end, \
                 function() lib.fail('second') end, \
                 function() lib.fail('third') end)",
        );
        assert!(
            error.contains("test-lib.fail: Plugin returned error: BOOM - second"),
            "{error}"
        );
        assert!(!error.contains("third"), "{error}");
        // The third settled first, and nothing was abandoned on its failure.
        assert_eq!(log_of(&log)[3..], ["settle 3", "settle 2", "settle 1"]);
    }

    #[test]
    fn a_raised_value_leaves_all_unchanged() {
        let (lua, _) = vm();
        let (code, text): (i64, String) = eval(
            &lua,
            "local _, t = pcall(async.all, function() error({ code = 7 }) end) \
             local _, s = pcall(async.all, function() end, function() error('plain', 0) end) \
             return t.code, s",
        );
        assert_eq!((code, text.as_str()), (7, "plain"));
    }

    #[test]
    fn a_call_outside_all_blocks() {
        let (lua, log) = vm();
        let value: String = eval(&lua, "return lib.echo('x')");
        assert_eq!(value, "x");
        assert_eq!(log_of(&log), ["call echo"]);
    }

    /// Every library call passes through the shim, so its frame is in every
    /// library error's traceback; a script that never asked for
    /// `internal.async` should not find that name there.
    #[test]
    fn a_library_error_names_the_bridge_rather_than_internal_async() {
        let (lua, _) = vm();
        let error = error_of(&lua, "lib.fail('x')");
        assert!(
            error.contains("test-lib.fail: Plugin returned error: BOOM - x"),
            "{error}"
        );
        assert!(error.contains(&format!("{CHUNK_NAME}:")), "{error}");
        assert!(!error.contains("internal.async"), "{error}");
    }

    /// Only the scheduler's own threads yield a call, and only where they can
    /// yield at all.
    #[test]
    fn a_script_coroutine_or_a_c_boundary_inside_all_still_blocks() {
        let (lua, log) = vm();
        let joined: String = eval(
            &lua,
            "local inner, sorted = async.all( \
                 function() return coroutine.wrap(function() return lib.echo('inner') end)() end, \
                 function() local t = { 2, 1 } \
                     table.sort(t, function(a, b) lib.echo('c') return a < b end) \
                     return t[1] end) \
             return inner .. sorted",
        );
        assert_eq!(joined, "inner1");
        assert!(
            log_of(&log).iter().all(|line| line == "call echo"),
            "{log:?}"
        );
    }

    /// The inner scheduler runs to completion before the outer one moves on,
    /// so the second outer function starts only after both inner calls settle.
    #[test]
    fn a_nested_all_runs_to_completion_inside_its_function() {
        let (lua, log) = vm();
        let joined: String = eval(
            &lua,
            "local a, b = async.all( \
                 function() \
                     local x, y = async.all( \
                         function() return lib.echo('x') end, \
                         function() return lib.echo('y') end) \
                     return x .. y \
                 end, \
                 function() return lib.echo('b') end) \
             return a .. b",
        );
        assert_eq!(joined, "xyb");
        assert_eq!(
            log_of(&log),
            [
                "start echo",
                "start echo",
                "settle 2",
                "settle 1",
                "start echo",
                "settle 3"
            ]
        );
    }

    /// A function that spends the budget stops the scheduler there, rather
    /// than being recorded as a failure while its siblings go on to start
    /// calls the run will never see.
    #[test]
    fn a_spent_budget_starts_no_further_calls() {
        let (lua, log) = vm_with(Some(1_000));
        let error = error_of(
            &lua,
            "async.all(function() while true do end end, function() return lib.echo('late') end)",
        );
        assert!(error.contains(budget::SPENT), "{error}");
        assert!(log_of(&log).is_empty(), "{log:?}");
    }

    #[test]
    fn a_function_that_yields_by_itself_fails_by_name() {
        let (lua, _) = vm();
        let error = error_of(&lua, "async.all(function() coroutine.yield(1) end)");
        assert!(
            error.contains("may yield only through a library call"),
            "{error}"
        );
    }

    #[test]
    fn require_hands_out_one_async_module() {
        let (lua, _) = vm();
        assert!(eval::<bool>(
            &lua,
            "return rawequal(require('internal.async'), require('internal.async')) \
                 and type(async.all) == 'function'"
        ));
    }
}
