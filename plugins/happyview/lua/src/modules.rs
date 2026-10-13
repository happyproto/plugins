//! `require("./…")` and `require("../…")`: a module from the project the run
//! was started from, resolved by the host against the run's own snapshot.
//!
//! The host answers a module's canonical id, so two specifiers that name one
//! file load it once. The importer is read off the call stack rather than
//! passed in: `require` is one global shared by every chunk, and Lua tells it
//! nothing about its caller.

use happyview_plugin_sdk::{host, ModuleResolve, PluginError, ResolvedModule};
use mlua::{Lua, Result as LuaResult, Table, Value};

use crate::chunk;

const LOADED_KEY: &str = "happyview.modules.loaded";
const LOADING_KEY: &str = "happyview.modules.loading";
/// Every id the host answered in this run, loaded or not: a module whose
/// file scope failed is in neither table above, and its error still has to
/// be placed in it.
const SEEN_KEY: &str = "happyview.modules.seen";
/// The id each `(referrer, specifier)` pair resolved to, keyed by referrer
/// and then by specifier. The snapshot a run resolves against is pinned, so
/// a pair answers the same id for the whole run, and a `require` inside a
/// function called on every request crosses to the host once.
const RESOLVED_KEY: &str = "happyview.modules.resolved";

/// What a relative `require` is resolved through. A trait so the resolution
/// can be driven natively: off wasm the host wrapper answers `NotWasm`.
pub trait Resolver {
    fn resolve(&self, request: &ModuleResolve) -> Result<ResolvedModule, PluginError>;
}

struct Host;

impl Resolver for Host {
    fn resolve(&self, request: &ModuleResolve) -> Result<ResolvedModule, PluginError> {
        host::module_resolve(request)
    }
}

pub fn host() -> Box<dyn Resolver> {
    Box::new(Host)
}

struct Modules {
    resolver: Box<dyn Resolver>,
    chunk_name: String,
}

pub fn is_relative(name: &str) -> bool {
    name.starts_with("./") || name.starts_with("../")
}

pub fn install(lua: &Lua, chunk_name: &str, resolver: Box<dyn Resolver>) -> LuaResult<()> {
    for key in [LOADED_KEY, LOADING_KEY, SEEN_KEY, RESOLVED_KEY] {
        lua.set_named_registry_value(key, lua.create_table()?)?;
    }
    lua.set_app_data(Modules {
        resolver,
        chunk_name: chunk_name.to_string(),
    });
    Ok(())
}

pub fn require(lua: &Lua, specifier: &str) -> LuaResult<Value> {
    let caller = caller(lua);
    let referrer = caller.name.clone();
    let loaded: Table = lua.named_registry_value(LOADED_KEY)?;
    let loading: Table = lua.named_registry_value(LOADING_KEY)?;

    if let Some(id) = resolved_before(lua, &referrer, specifier)? {
        if let Some(value) = settled(&caller, &loaded, &loading, &id)? {
            return Ok(value);
        }
        // A module whose file scope failed is neither loaded nor loading, and
        // a second `require` of it runs it again from what the host answers.
    }

    let resolved = {
        let modules = lua
            .app_data_ref::<Modules>()
            .ok_or_else(|| mlua::Error::runtime("local modules are not installed in this VM"))?;
        modules.resolver.resolve(&ModuleResolve {
            specifier: specifier.to_string(),
            referrer: referrer.clone(),
        })
    }
    .map_err(|error| unresolved(&caller, specifier, error))?;
    let id = resolved.id.as_str();
    lua.named_registry_value::<Table>(SEEN_KEY)?
        .raw_set(id, true)?;
    remember(lua, &referrer, specifier, id)?;

    if let Some(value) = settled(&caller, &loaded, &loading, id)? {
        return Ok(value);
    }

    let main = chunk::load(lua, id, &resolved.source, resolved.artifact.as_ref())
        .map_err(|why| caller.raise(format!("module '{id}': {why}")))??;
    loading.raw_set(id, true)?;
    let returned = main.call::<Value>(());
    loading.raw_set(id, Value::Nil)?;
    // A module that returns nothing is still loaded, as Lua's own `require`
    // records it, so its file scope does not run a second time.
    let value = match returned? {
        Value::Nil => Value::Boolean(true),
        value => value,
    };
    loaded.raw_set(id, &value)?;
    Ok(value)
}

/// Module `id`'s value when it has already loaded, and the cycle error when
/// it is still loading; `None` when it has to be run.
fn settled(caller: &Caller, loaded: &Table, loading: &Table, id: &str) -> LuaResult<Option<Value>> {
    let cached: Value = loaded.raw_get(id)?;
    if !cached.is_nil() {
        return Ok(Some(cached));
    }
    if loading.raw_get::<bool>(id)? {
        return Err(caller.raise(format!(
            "module '{id}' was required again by '{}' while it was still loading: \
             local modules cannot require each other in a cycle",
            caller.name
        )));
    }
    Ok(None)
}

fn resolved_before(lua: &Lua, referrer: &str, specifier: &str) -> LuaResult<Option<String>> {
    let resolved: Table = lua.named_registry_value(RESOLVED_KEY)?;
    match resolved.raw_get::<Option<Table>>(referrer)? {
        Some(from) => from.raw_get(specifier),
        None => Ok(None),
    }
}

fn remember(lua: &Lua, referrer: &str, specifier: &str, id: &str) -> LuaResult<()> {
    let resolved: Table = lua.named_registry_value(RESOLVED_KEY)?;
    let from = match resolved.raw_get::<Option<Table>>(referrer)? {
        Some(from) => from,
        None => {
            let from = lua.create_table()?;
            resolved.raw_set(referrer, &from)?;
            from
        }
    };
    from.raw_set(specifier, id)
}

/// The codes whose message is a sentence the host wrote for the script's
/// author: a miss (naming the specifier, the importer and any same-named file
/// in another language), a run with no project, and a path leaving `src/`.
const HOST_SENTENCES: [&str; 3] = ["NOT_FOUND", "UNSUPPORTED", "BAD_INPUT"];

fn unresolved(caller: &Caller, specifier: &str, error: PluginError) -> mlua::Error {
    if HOST_SENTENCES.contains(&error.code.as_str()) {
        return caller.raise(error.message);
    }
    caller.raise(format!(
        "cannot resolve '{specifier}': {} - {}",
        error.code, error.message
    ))
}

/// Where a `require` call is written: the chunk, and the line it is on.
struct Caller {
    name: String,
    line: Option<usize>,
}

impl Caller {
    /// An error placed at the `require` call, as Lua's own `require` places
    /// one. An error a Rust function returns carries no position otherwise,
    /// and a failed `require` with no line sends an author searching.
    fn raise(&self, message: impl std::fmt::Display) -> mlua::Error {
        match self.line {
            Some(line) => {
                mlua::Error::runtime(format!("{}:{line}: {message}", chunk::shown(&self.name)))
            }
            None => mlua::Error::runtime(message.to_string()),
        }
    }
}

/// The nearest frame whose source is a script's or a module's. C frames
/// (`pcall`, a wrapped coroutine) and the plugin's own `=`-named chunks are
/// stepped over, so the importer is the file the call is written in however
/// the call is reached.
fn caller(lua: &Lua) -> Caller {
    let mut level = 0;
    while let Some((source, line)) = lua.inspect_stack(level, |debug| {
        (
            debug.source().source.map(|source| source.into_owned()),
            debug.current_line(),
        )
    }) {
        if let Some(name) = source.as_deref().and_then(chunk::name_of_source) {
            return Caller {
                name: name.to_string(),
                line,
            };
        }
        level += 1;
    }
    Caller {
        name: lua
            .app_data_ref::<Modules>()
            .map(|modules| modules.chunk_name.clone())
            .unwrap_or_else(|| chunk::DEFAULT.to_string()),
        line: None,
    }
}

/// Every chunk name an error in this run can carry, longest first so a path
/// is matched before a shorter name it ends with.
pub fn known(lua: &Lua) -> Vec<String> {
    let mut names: Vec<String> = lua
        .app_data_ref::<Modules>()
        .map(|modules| vec![modules.chunk_name.clone()])
        .unwrap_or_default();
    if let Ok(seen) = lua.named_registry_value::<Table>(SEEN_KEY) {
        names.extend(
            seen.pairs::<String, Value>()
                .filter_map(Result::ok)
                .map(|(id, _)| id),
        );
    }
    names.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use happyview_plugin_sdk::{
        ExecuteInput, ExecuteOutput, ModuleResolve, PluginError, PreparedArtifact, ResolvedModule,
        ScriptErrorKind, ScriptValueKind,
    };
    use serde_json::json;

    use super::Resolver;

    /// A project in memory, resolved the way the host resolves one: against
    /// the importer's directory, with `.lua` added when the specifier has no
    /// extension. Every request is recorded.
    #[derive(Clone, Default)]
    struct Files {
        files: Rc<BTreeMap<String, (String, Option<PreparedArtifact>)>>,
        asked: Rc<RefCell<Vec<(String, String)>>>,
    }

    impl Files {
        fn new(files: &[(&str, &str)]) -> Self {
            Self {
                files: Rc::new(
                    files
                        .iter()
                        .map(|(id, source)| (id.to_string(), (source.to_string(), None)))
                        .collect(),
                ),
                asked: Rc::default(),
            }
        }

        fn with_artifact(mut self, id: &str, source: &str, artifact: PreparedArtifact) -> Self {
            Rc::make_mut(&mut self.files).insert(id.into(), (source.into(), Some(artifact)));
            self
        }
    }

    fn join(referrer: &str, specifier: &str) -> String {
        let mut parts: Vec<&str> = referrer.split('/').collect();
        parts.pop();
        for part in specifier.split('/') {
            match part {
                "." => {}
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        let id = parts.join("/");
        if id.ends_with(".lua") {
            id
        } else {
            format!("{id}.lua")
        }
    }

    impl Resolver for Files {
        fn resolve(&self, request: &ModuleResolve) -> Result<ResolvedModule, PluginError> {
            self.asked
                .borrow_mut()
                .push((request.specifier.clone(), request.referrer.clone()));
            let id = join(&request.referrer, &request.specifier);
            match self.files.get(&id) {
                Some((source, artifact)) => Ok(ResolvedModule {
                    id,
                    source: source.clone(),
                    artifact: artifact.clone(),
                }),
                None => Err(PluginError::new(
                    "NOT_FOUND",
                    format!(
                        "no module '{}' from '{}': looked for {id}",
                        request.specifier, request.referrer
                    ),
                )),
            }
        }
    }

    const HANDLER: &str = "src/lexicons/app/test/q.lua";

    fn run(files: &Files, source: &str) -> ExecuteOutput {
        let input: ExecuteInput = serde_json::from_value(json!({
            "source": source,
            "chunk_name": HANDLER,
            "kind": "xrpc_query",
            "input": {},
            "context": { "trigger": "xrpc.query:app.test.q" },
            "limits": { "instructions": 1_000_000, "memory_bytes": 67_108_864 },
        }))
        .unwrap();
        crate::execute_with(&input, Box::new(files.clone())).expect("an output")
    }

    fn value(output: ExecuteOutput) -> serde_json::Value {
        match output {
            ExecuteOutput::Returned { value, .. } => value,
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn a_handler_requires_a_module_by_a_relative_path() {
        let files = Files::new(&[(
            "src/lexicons/app/test/lib/format.lua",
            "local M = {}\nfunction M.shout(s) return s:upper() end\nreturn M",
        )]);
        let output = run(
            &files,
            "local format = require('./lib/format')\nfunction handle() return format.shout('hi') end",
        );
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!("HI"),
                value_kind: ScriptValueKind::Other
            }
        );
        assert_eq!(
            files.asked.borrow().as_slice(),
            &[("./lib/format".to_string(), HANDLER.to_string())]
        );
    }

    #[test]
    fn a_module_is_evaluated_once_however_many_chunks_require_it() {
        let files = Files::new(&[
            (
                "src/lexicons/app/test/shared.lua",
                "loads = (loads or 0) + 1\nreturn {}",
            ),
            (
                "src/lexicons/app/test/lib/a.lua",
                "return require('../shared')",
            ),
            (
                "src/lexicons/app/test/lib/b.lua",
                "return require('../shared')",
            ),
        ]);
        let output = run(
            &files,
            "local a = require('./lib/a')\nlocal b = require('./lib/b')\nlocal c = require('./shared')\n\
             function handle() return { loads = loads, same = rawequal(a, b) and rawequal(b, c) } end",
        );
        assert_eq!(value(output), json!({ "loads": 1, "same": true }));
        // Each nested require names the module it is written in.
        assert!(files.asked.borrow().contains(&(
            "../shared".to_string(),
            "src/lexicons/app/test/lib/a.lua".to_string()
        )));
    }

    #[test]
    fn the_importer_is_the_file_the_call_is_written_in_however_it_is_reached() {
        let files = Files::new(&[
            ("src/lexicons/app/test/lib/f.lua", "return 'f'"),
            (
                "src/lexicons/app/test/lib/late.lua",
                "return { get = function() return require('./f') end }",
            ),
        ]);
        let output = run(
            &files,
            "local late = require('./lib/late')\n\
             function handle()\n\
               local ok, viapcall = pcall(require, './lib/f')\n\
               local viawrap = coroutine.wrap(function() return require('./lib/f') end)()\n\
               return { viapcall, viawrap, late.get() }\n\
             end",
        );
        assert_eq!(value(output), json!(["f", "f", "f"]));
        let asked = files.asked.borrow();
        assert!(asked.contains(&(
            "./f".to_string(),
            "src/lexicons/app/test/lib/late.lua".to_string()
        )));
        // Two calls from the handler, one crossing to the host: the pair's
        // id is remembered for the run.
        assert_eq!(
            asked
                .iter()
                .filter(|(s, r)| s == "./lib/f" && r == HANDLER)
                .count(),
            1
        );
    }

    #[test]
    fn a_module_that_returns_nothing_is_loaded_once_and_required_as_true() {
        let files = Files::new(&[("src/lexicons/app/test/side.lua", "count = (count or 0) + 1")]);
        let output = run(
            &files,
            "local a = require('./side')\nlocal b = require('./side')\nfunction handle() return { a = a, b = b, count = count } end",
        );
        assert_eq!(value(output), json!({ "a": true, "b": true, "count": 1 }));
    }

    #[test]
    fn a_miss_raises_the_hosts_own_sentence_at_the_requiring_line() {
        let files = Files::new(&[]);
        match run(
            &files,
            "\nlocal m = require('./nope')\nfunction handle() end",
        ) {
            ExecuteOutput::Error {
                kind,
                message,
                line,
                chunk,
                ..
            } => {
                assert_eq!(kind, ScriptErrorKind::Runtime);
                assert!(
                    message.contains("no module './nope' from 'src/lexicons/app/test/q.lua'"),
                    "{message}"
                );
                assert_eq!(chunk.as_deref(), Some(HANDLER));
                assert_eq!(line, Some(2));
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn modules_that_require_each_other_fail_naming_the_cycle() {
        let files = Files::new(&[
            ("src/lexicons/app/test/a.lua", "return require('./b')"),
            ("src/lexicons/app/test/b.lua", "return require('./a')"),
        ]);
        match run(&files, "local a = require('./a')\nfunction handle() end") {
            ExecuteOutput::Error {
                message,
                chunk,
                line,
                ..
            } => {
                assert!(
                    message.starts_with("module 'src/lexicons/app/test/a.lua' was required again by 'src/lexicons/app/test/b.lua' while it was still loading"),
                    "{message}"
                );
                assert_eq!(chunk.as_deref(), Some("src/lexicons/app/test/b.lua"));
                assert_eq!(line, Some(1));
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn an_error_inside_a_module_names_the_module_and_its_line() {
        let files = Files::new(&[(
            "src/lexicons/app/test/lib/format.lua",
            "local M = {}\nfunction M.f(t)\n  return t.x\nend\nreturn M",
        )]);
        match run(
            &files,
            "local format = require('./lib/format')\nfunction handle() return format.f(nil) end",
        ) {
            ExecuteOutput::Error { chunk, line, .. } => {
                assert_eq!(
                    chunk.as_deref(),
                    Some("src/lexicons/app/test/lib/format.lua")
                );
                assert_eq!(line, Some(3));
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_module_runs_from_its_artifact_and_falls_back_to_source_when_it_cannot() {
        let id = "src/lexicons/app/test/m.lua";
        let lua = mlua::Lua::new();
        let compiled = crate::chunk::load_source(&lua, id, "return 'bytecode'").unwrap();
        let artifact = crate::artifact::prepare(&compiled);

        let files = Files::new(&[]).with_artifact(id, "", artifact.clone());
        assert_eq!(
            value(run(
                &files,
                "local m = require('./m')\nfunction handle() return m end"
            )),
            json!("bytecode")
        );

        let mut foreign = artifact;
        foreign.engine = "puc-lua-5.1.5".into();
        let files = Files::new(&[]).with_artifact(id, "return 'source'", foreign.clone());
        assert_eq!(
            value(run(
                &files,
                "local m = require('./m')\nfunction handle() return m end"
            )),
            json!("source")
        );

        let files = Files::new(&[]).with_artifact(id, "", foreign);
        match run(&files, "local m = require('./m')\nfunction handle() end") {
            ExecuteOutput::Error { message, .. } => {
                assert!(message.contains("puc-lua-5.1.5"), "{message}")
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_modules_source_is_never_loaded_as_bytecode() {
        let lua = mlua::Lua::new();
        let dumped = lua.load("return 1").into_function().unwrap().dump(false);
        let files = Files::new(&[(
            "src/lexicons/app/test/m.lua",
            &String::from_utf8_lossy(&dumped),
        )]);
        match run(&files, "local m = require('./m')\nfunction handle() end") {
            ExecuteOutput::Error { message, .. } => {
                assert!(message.contains("binary chunk"), "{message}")
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_name_still_means_a_builtin_or_a_library() {
        let files = Files::new(&[]);
        let output = run(
            &files,
            "local json = require('internal.json')\nfunction handle() return json.encode({ a = 1 }) end",
        );
        assert_eq!(value(output), json!(r#"{"a":1}"#));
        assert!(files.asked.borrow().is_empty());
    }
}
