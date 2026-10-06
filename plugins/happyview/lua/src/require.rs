//! `require(name)`: a built-in, or an installed library plugin reached
//! through the two bridge imports. Namespaces are paired with plugin ids in
//! the execute input, because `require` takes the first and the imports take
//! the second, and resolving one from the other inside the guest would move
//! namespace resolution out of the registry that already does it.

use happyview_plugin_sdk::{host, ApiMethod, ApiSurface, LibraryRef, PluginError};
use mlua::{Function, Lua, MultiValue, Result as LuaResult, Table, Value};
use serde_json::{json, Map, Value as Json};

use crate::builtins;
use crate::convert;

const LOADED_KEY: &str = "happyview.require.loaded";

/// A library's failure as the host renders it, so a script matching on text
/// inside the message keeps working whichever side raised it.
///
/// Two shapes, because the host sends two. A library that returned an error
/// of its own arrives as a code and a message, and
/// `ExecutionError::PluginError` displays those as `Plugin returned error:
/// {code} - {message}`. Everything else — a depth limit, a library that is
/// not installed, one that trapped — arrives under `LIBRARY_ERROR` carrying
/// the host's own rendering, which the native path shows unprefixed.
fn plugin_error(label: &str, error: PluginError) -> mlua::Error {
    if error.code == HOST_RENDERED {
        return mlua::Error::runtime(format!("{label}: {}", error.message));
    }
    mlua::Error::runtime(format!(
        "{label}: Plugin returned error: {} - {}",
        error.code, error.message
    ))
}

/// The code the host uses when the message is already its own.
///
/// A discriminator rather than a guarantee: nothing stops a library from
/// returning this code as its own error, and the envelope carries no field
/// saying which side wrote the message, so such a library gets the unprefixed
/// rendering where the native path would prefix it. Reading the branch as
/// exact would be wrong.
const HOST_RENDERED: &str = "LIBRARY_ERROR";

fn missing_module(name: &str) -> mlua::Error {
    if name.starts_with(builtins::PREFIX) {
        return mlua::Error::runtime(format!(
            "module '{name}' not found -- built-in modules are: {}",
            builtins::MODULES.join(", ")
        ));
    }
    mlua::Error::runtime(format!(
        "module '{name}' not found -- is the '{name}' library plugin installed?"
    ))
}

/// Install `require` for a run. Resolution is synchronous because every host
/// import is, and a script calls `require` at file scope anyway.
pub fn install(lua: &Lua, libraries: &[LibraryRef]) -> LuaResult<()> {
    lua.set_named_registry_value(LOADED_KEY, lua.create_table()?)?;
    let libraries: Vec<(String, String)> = libraries
        .iter()
        .map(|entry| (entry.namespace.clone(), entry.id.clone()))
        .collect();

    let require = lua.create_function(move |lua, name: String| {
        let loaded: Table = lua.named_registry_value(LOADED_KEY)?;
        // One table per name per VM: a module is stateful once a constructor
        // has handed out objects, and two copies would not share that state.
        if let Value::Table(cached) = loaded.raw_get::<Value>(name.as_str())? {
            return Ok(cached);
        }

        let module = match builtins::module(lua, &name)? {
            Some(module) => module,
            // The prefix is decided here rather than left to the lookup, so a
            // library registered under an `internal.` namespace cannot be
            // served by one path and refused by the other.
            None if name.starts_with(builtins::PREFIX) => return Err(missing_module(&name)),
            None => {
                let (_, id) = libraries
                    .iter()
                    .find(|(namespace, _)| *namespace == name)
                    .ok_or_else(|| missing_module(&name))?;
                let surface = host::library_surface(id).map_err(|e| plugin_error(&name, e))?;
                build_module(lua, id, &surface)?
            }
        };
        loaded.raw_set(name.as_str(), &module)?;
        Ok(module)
    })?;
    lua.globals().set("require", require)
}

/// One library's surface as a table. Keys are the surface's canonical names,
/// which is what every document on the wire carries.
fn build_module(lua: &Lua, library: &str, surface: &ApiSurface) -> LuaResult<Table> {
    let module = lua.create_table()?;
    for export in &surface.exports {
        if export.is_function() {
            module.set(
                export.name.as_str(),
                library_function(lua, library, &export.name)?,
            )?;
        } else if export.is_constructor() {
            module.set(
                export.name.as_str(),
                constructor(lua, library, &export.name, &export.methods)?,
            )?;
        }
        // An export in a kind this interpreter does not serve — a constant,
        // say — is left out rather than guessed at.
    }
    Ok(module)
}

fn library_function(lua: &Lua, library: &str, name: &str) -> LuaResult<Function> {
    let (library, name) = (library.to_string(), name.to_string());
    lua.create_function(move |lua, args: MultiValue| {
        let args = arguments(lua, args)?;
        call(lua, &library, &name, &format!("{library}.{name}"), &args)
    })
}

/// A constructor hands out objects sharing one method table, since a method
/// acts on the object it is given. `lazy` appends a step and returns the
/// object; `immediate` sends the object and the call as one document.
fn constructor(
    lua: &Lua,
    library: &str,
    constructor_name: &str,
    methods: &[ApiMethod],
) -> LuaResult<Function> {
    let index = lua.create_table()?;
    // Keyed by each method's own canonical name, never by position: `mode` is
    // an unvalidated string from the plugin, and a method in a mode this
    // interpreter does not recognise has to be skipped rather than shift a
    // later one onto the wrong key.
    for method in methods {
        if method.is_lazy() {
            index.set(method.name.as_str(), lazy_method(lua, &method.name)?)?;
        } else if method.is_immediate() {
            index.set(
                method.name.as_str(),
                immediate_method(lua, library, constructor_name, &method.name)?,
            )?;
        }
    }
    let meta = lua.create_table()?;
    meta.set("__index", index)?;

    lua.create_function(move |lua, args: MultiValue| {
        let object = lua.create_table()?;
        object.set("__args", lua.create_sequence_from(args)?)?;
        object.set("__steps", lua.create_table()?)?;
        object.set_metatable(Some(meta.clone()))?;
        Ok(object)
    })
}

/// Appends `{name, args}` and returns the object, so a chain reads left to
/// right and the document reads the same way.
fn lazy_method(lua: &Lua, name: &str) -> LuaResult<Function> {
    let name = name.to_string();
    lua.create_function(
        move |lua, (object, args): (Table, MultiValue)| -> LuaResult<Table> {
            let steps: Table = object.get("__steps")?;
            let step = lua.create_table()?;
            step.set("name", name.as_str())?;
            step.set("args", lua.create_sequence_from(args)?)?;
            steps.push(step)?;
            Ok(object)
        },
    )
}

fn immediate_method(
    lua: &Lua,
    library: &str,
    constructor_name: &str,
    name: &str,
) -> LuaResult<Function> {
    let (library, constructor_name, name) = (
        library.to_string(),
        constructor_name.to_string(),
        name.to_string(),
    );
    lua.create_function(move |lua, (object, args): (Table, MultiValue)| {
        let document = call_document(lua, &object, &name, args)?;
        call(
            lua,
            &library,
            &constructor_name,
            &format!("{library}.{constructor_name}:{name}"),
            &[document],
        )
    })
}

/// The object and the call as one document: the constructor's own arguments,
/// the accumulated steps in order, and the call that ends the chain. Built
/// here rather than through `to_value` so an empty argument list stays `[]`.
fn call_document(lua: &Lua, object: &Table, name: &str, args: MultiValue) -> LuaResult<Json> {
    let constructor_args = convert::sequence_to_json(lua, &object.get("__args")?)?;
    let mut steps = Vec::new();
    for step in object.get::<Table>("__steps")?.sequence_values::<Table>() {
        let step = step?;
        let mut one = Map::new();
        one.insert(
            step.get::<String>("name")?,
            Json::Array(convert::sequence_to_json(lua, &step.get("args")?)?),
        );
        steps.push(Json::Object(one));
    }
    Ok(json!({
        "args": constructor_args,
        "steps": steps,
        "call": { "name": name, "args": arguments(lua, args)? },
    }))
}

fn arguments(lua: &Lua, args: MultiValue) -> LuaResult<Vec<Json>> {
    args.into_iter()
        .map(|value| convert::to_json(lua, value))
        .collect()
}

fn call(lua: &Lua, library: &str, function: &str, label: &str, args: &[Json]) -> LuaResult<Value> {
    match host::call_library(library, function, args) {
        Ok(value) => convert::library_result(lua, &value),
        Err(e) => Err(plugin_error(label, e)),
    }
}

/// For `validate`: every module is a table whose every field is a function
/// returning another such table, so a top-level chain of any shape compiles
/// without an instance of anything.
pub fn install_stub(lua: &Lua) -> LuaResult<()> {
    let require = lua.create_function(|lua, _name: String| stub(lua))?;
    lua.globals().set("require", require)
}

fn stub(lua: &Lua) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_table, _key): (Value, Value)| {
            lua.create_function(|lua, _: MultiValue| stub(lua))
        })?,
    )?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::sandbox;
    use happyview_plugin_sdk::ApiExport;

    /// A surface in the shape the standard `db` library publishes: one plain
    /// function, one constructor with two lazy steps and one immediate call,
    /// and a constant this interpreter does not serve.
    fn surface() -> ApiSurface {
        let mut records = ApiExport::constructor("records");
        records.methods = vec![
            ApiMethod::lazy("where"),
            ApiMethod {
                name: "sideways".into(),
                mode: "sideways".into(),
            },
            ApiMethod::lazy("limit"),
            ApiMethod::immediate("all"),
        ];
        let mut constant = ApiExport::function("VERSION");
        constant.kind = "constant".into();
        ApiSurface::new("happyview.db")
            .export(ApiExport::function("count"))
            .export(records)
            .export(constant)
    }

    fn module_vm() -> Lua {
        let lua = sandbox();
        let module = build_module(&lua, "happyview-db", &surface()).unwrap();
        lua.globals().set("db", module).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, source: &str) -> T {
        lua.load(source)
            .eval()
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }

    fn error_of(lua: &Lua, source: &str) -> String {
        lua.load(source)
            .eval::<Value>()
            .expect_err(source)
            .to_string()
    }

    #[test]
    fn a_surface_becomes_functions_and_constructors_and_nothing_else() {
        let lua = module_vm();
        assert!(eval::<bool>(
            &lua,
            "return type(db.count) == 'function' and type(db.records) == 'function'"
        ));
        // A constant is an export kind this interpreter does not serve.
        assert!(eval::<Value>(&lua, "return db.VERSION").is_nil());
    }

    #[test]
    fn a_method_in_an_unrecognised_mode_is_skipped_by_name() {
        let lua = module_vm();
        assert!(eval::<bool>(
            &lua,
            "local q = db.records('c') \
             return type(q.where) == 'function' and q.sideways == nil \
                 and type(q.limit) == 'function' and type(q.all) == 'function'"
        ));
    }

    #[test]
    fn a_chain_builds_the_document_the_call_carries() {
        let lua = module_vm();
        let object: Table = eval(&lua, "return db.records('c'):where('a', 1):limit(5)");
        let document = call_document(&lua, &object, "all", MultiValue::new()).unwrap();
        assert_eq!(
            document,
            json!({
                "args": ["c"],
                "steps": [{ "where": ["a", 1] }, { "limit": [5] }],
                "call": { "name": "all", "args": [] },
            })
        );
    }

    #[test]
    fn a_nil_argument_cuts_the_step_it_is_in_and_nothing_after_it() {
        let lua = module_vm();
        let object: Table = eval(&lua, "return db.records('c'):where('a', nil, 3):limit(5)");
        let document = call_document(&lua, &object, "all", MultiValue::new()).unwrap();
        assert_eq!(
            document["steps"],
            json!([{ "where": ["a"] }, { "limit": [5] }])
        );
    }

    #[test]
    fn an_empty_chain_still_carries_its_three_keys() {
        let lua = module_vm();
        let object: Table = eval(&lua, "return db.records()");
        let document = call_document(&lua, &object, "all", MultiValue::new()).unwrap();
        assert_eq!(
            document,
            json!({ "args": [], "steps": [], "call": { "name": "all", "args": [] } })
        );
    }

    #[test]
    fn require_resolves_a_builtin_and_caches_it() {
        let lua = sandbox();
        install(&lua, &[]).unwrap();
        assert!(eval::<bool>(
            &lua,
            "local a = require('internal.json') local b = require('internal.json') \
             return rawequal(a, b) and type(a.encode) == 'function'"
        ));
    }

    #[test]
    fn an_unknown_namespace_names_the_plugin_that_would_serve_it() {
        let lua = sandbox();
        install(&lua, &[]).unwrap();
        let error = error_of(&lua, "return require('happyview.db')");
        assert!(
            error.contains(
                "module 'happyview.db' not found -- is the 'happyview.db' library plugin installed?"
            ),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_internal_module_names_the_four() {
        let lua = sandbox();
        install(&lua, &[]).unwrap();
        let error = error_of(&lua, "return require('internal.nope')");
        assert!(
            error.contains(
                "module 'internal.nope' not found -- built-in modules are: \
                 internal.logging, internal.time, internal.tids, internal.json"
            ),
            "{error}"
        );
    }

    /// A library may not take an `internal.` name, whichever order the two
    /// lookups happen in: the plugin would otherwise serve one the native
    /// path refuses.
    #[test]
    fn an_internal_name_is_refused_even_when_a_library_claims_it() {
        let lua = sandbox();
        install(
            &lua,
            &[LibraryRef {
                namespace: "internal.json".into(),
                id: "impostor".into(),
            }],
        )
        .unwrap();
        // The built-in still wins for a name that is one.
        assert!(eval::<bool>(
            &lua,
            "return type(require('internal.json').encode) == 'function'"
        ));

        install(
            &lua,
            &[LibraryRef {
                namespace: "internal.sneaky".into(),
                id: "impostor".into(),
            }],
        )
        .unwrap();
        let error = error_of(&lua, "return require('internal.sneaky')");
        assert!(error.contains("built-in modules are:"), "{error}");
    }

    /// The host sends two error shapes and the bridge has to render each as
    /// the host does: a library's own error with the code, and the host's own
    /// rendering unprefixed.
    #[test]
    fn a_host_rendered_failure_keeps_the_hosts_own_wording() {
        let of_its_own = plugin_error(
            "happyview.db.search",
            PluginError::new("NO_SESSION", "no PDS session for this caller"),
        )
        .to_string();
        assert!(
            of_its_own.contains(
                "happyview.db.search: Plugin returned error: NO_SESSION - no PDS session"
            ),
            "{of_its_own}"
        );

        let the_hosts = plugin_error(
            "happyview.db.search",
            PluginError::new(HOST_RENDERED, "Library call depth limit (8) exceeded"),
        )
        .to_string();
        assert!(
            the_hosts.contains("happyview.db.search: Library call depth limit (8) exceeded"),
            "{the_hosts}"
        );
        assert!(!the_hosts.contains("Plugin returned error"), "{the_hosts}");
    }

    /// A namespace the input paired with a plugin reaches the bridge rather
    /// than the not-found message; off wasm the surface fetch is what fails,
    /// and it fails as a plugin error naming the namespace.
    #[test]
    fn a_paired_namespace_reaches_the_bridge() {
        let lua = sandbox();
        install(
            &lua,
            &[LibraryRef {
                namespace: "happyview.db".into(),
                id: "happyview-db".into(),
            }],
        )
        .unwrap();
        let error = error_of(&lua, "return require('happyview.db')");
        assert!(
            error.contains("happyview.db: Plugin returned error:"),
            "{error}"
        );
        assert!(!error.contains("not found"), "{error}");
    }

    #[test]
    fn the_stub_require_compiles_a_chain_of_any_shape() {
        let lua = sandbox();
        install_stub(&lua).unwrap();
        lua.load(
            "local db = require('happyview.db') \
             local q = db.records('c'):where('a', 1):limit(5):all() \
             function handle() return q end",
        )
        .exec()
        .expect("a stub chain of any shape should compile");
    }
}
