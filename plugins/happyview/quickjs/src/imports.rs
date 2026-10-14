//! `import`: the script itself, which a run's entry imports by its chunk
//! name; a module of the script's own project, which the host answers for a
//! relative path; a built-in; or an installed library plugin reached through
//! the bridge. Namespaces are paired with plugin ids in the execute input,
//! because a script imports the first and the host imports take the second,
//! and resolving one from the other inside the guest would move namespace
//! resolution out of the registry that already does it.
//!
//! Every module a script imports is the same shape, whatever serves it: a
//! default export holding every function, and each function again under its
//! own name, so `import db from "happyview.db"` and
//! `import { records } from "happyview.db"` both work. QuickJS loads each
//! name once per run, so a module handed out twice is the same object.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use happyview_plugin_sdk::{LibraryRef, ModuleResolve, PreparedArtifact, ResolvedModule};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Module, Result};

use crate::backend::Backend;
use crate::bridge;
use crate::builtins::{self, throw_error};
use crate::calls::{library_error, Calls};
use crate::{Chunks, Frontend, Prepared, SourceMap};

/// Whether `name` names a file beside the importer rather than a built-in
/// or a library.
pub fn is_relative(name: &str) -> bool {
    name.starts_with("./") || name.starts_with("../")
}

fn refusal(name: &str) -> String {
    format!(
        "cannot import '{name}': a script imports built-in modules and installed libraries \
         by name, and its project's own modules by a path starting with ./ or ../, never \
         an absolute path or a URL"
    )
}

/// How a specifier becomes the name QuickJS loads and caches a module by.
pub struct Specifiers {
    /// `None` while validating.
    project: Option<Rc<Project>>,
}

impl Specifiers {
    pub fn validating() -> Self {
        Self { project: None }
    }

    pub fn running(project: Rc<Project>) -> Self {
        Self {
            project: Some(project),
        }
    }
}

impl Resolver for Specifiers {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        let relative = is_relative(name);
        if (name.starts_with('.') && !relative) || name.starts_with('/') || name.contains(':') {
            return Err(throw_error(ctx, &refusal(name)));
        }
        match &self.project {
            // Validation stubs every import, a relative one included: whether
            // the file exists is the host's question, asked of the imports
            // `validate` reports.
            None => Ok(name.to_string()),
            Some(project) if base == ENTRY && name == project.main() => Ok(name.to_string()),
            Some(project) if relative => project.resolve(ctx, base, name),
            Some(_) => Ok(name.to_string()),
        }
    }
}

/// The module a run declares and evaluates. It imports the script by its
/// chunk name, which sends the script through the loader like any other
/// module: a module read back from bytecode is linked to its own imports
/// only when QuickJS reaches it as an import.
pub const ENTRY: &str = "<entry>";

pub fn entry_source(main: &str) -> String {
    let quoted = serde_json::to_string(main).expect("a string always encodes");
    format!("import * as main from {quoted};\nexport default main;\n")
}

/// Project code as the loader declares it.
pub enum Body {
    Bytecode(Bytecode),
    Source(Prepared),
}

/// Bytecode `validate` wrote, with the map its front end kept. Its fields
/// are private so that [`Body::from_artifact`] is the only way to build
/// one: QuickJS does not verify bytecode, and that is where an artifact's
/// stamp is checked.
pub struct Bytecode {
    bytes: Vec<u8>,
    map: Option<SourceMap>,
}

impl Body {
    /// A chunk's bytecode, when the host sent an artifact this build can
    /// load and its map reads back. `Ok(None)` says to prepare `source`
    /// instead: there is no artifact, or there is source to fall back to.
    /// The `Err` is an artifact that cannot be used with no source behind
    /// it, which the handler reports as the host's fault and a module throws
    /// at its importer.
    pub fn from_artifact(
        frontend: &dyn Frontend,
        artifact: Option<&PreparedArtifact>,
        source: &str,
    ) -> std::result::Result<Option<Body>, String> {
        let Some(artifact) = artifact else {
            return Ok(None);
        };
        let map = crate::artifact::usable(artifact).and_then(|()| {
            artifact
                .source_map
                .as_deref()
                .map(|json| frontend.read_map(json))
                .transpose()
        });
        match map {
            Ok(map) => Ok(Some(Body::Bytecode(Bytecode {
                bytes: artifact.bytes.clone(),
                map,
            }))),
            Err(_) if !source.is_empty() => Ok(None),
            Err(why) => Err(why),
        }
    }
}

/// Host error codes whose message is a sentence written for the script's
/// author: a miss, a run with no project, and a path leaving `src/`.
const HOST_CODES: [&str; 3] = ["NOT_FOUND", "UNSUPPORTED", "BAD_INPUT"];

/// A run's project code: the script, under its chunk name, and every local
/// module the host has answered for it, under its id.
///
/// QuickJS keeps one module per name for the life of a context and checks
/// that list before it asks the loader, so naming a local module by the
/// host's id is what de-duplicates it: two specifiers for one file, from one
/// importer or two, load it once.
pub struct Project {
    frontend: &'static dyn Frontend,
    backend: Rc<dyn Backend>,
    main: String,
    body: RefCell<Option<Body>>,
    /// Modules the host answered and QuickJS has not loaded yet.
    answered: RefCell<BTreeMap<String, ResolvedModule>>,
    /// `(importer, specifier)` to id, so a pair is asked of the host once:
    /// QuickJS resolves a dynamic `import()` again every time it runs.
    resolved: RefCell<BTreeMap<(String, String), String>>,
    pub chunks: RefCell<Chunks>,
}

impl Project {
    pub fn new(
        frontend: &'static dyn Frontend,
        backend: Rc<dyn Backend>,
        main: &str,
        body: Body,
    ) -> Self {
        Self {
            frontend,
            backend,
            main: main.to_string(),
            body: RefCell::new(Some(body)),
            answered: RefCell::new(BTreeMap::new()),
            resolved: RefCell::new(BTreeMap::new()),
            chunks: RefCell::new(Chunks::default()),
        }
    }

    pub fn main(&self) -> &str {
        &self.main
    }

    /// A relative specifier's module id, from the host. `base` is the
    /// importing module's name, which is its id, so the host resolves the
    /// specifier against the importer's own directory. A miss carries the
    /// host's own sentence, which already names the specifier, the importer
    /// and any same-named file in another language.
    pub fn resolve<'js>(&self, ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String> {
        let key = (base.to_string(), name.to_string());
        if let Some(id) = self.resolved.borrow().get(&key) {
            return Ok(id.clone());
        }
        let request = ModuleResolve {
            specifier: name.to_string(),
            referrer: base.to_string(),
        };
        let module = match self.backend.module_resolve(&request) {
            Ok(module) => module,
            Err(error) if HOST_CODES.contains(&error.code.as_str()) => {
                return Err(throw_error(ctx, &error.message))
            }
            Err(error) => {
                return Err(throw_error(
                    ctx,
                    &format!(
                        "cannot resolve '{name}': {} - {}",
                        error.code, error.message
                    ),
                ))
            }
        };
        let id = module.id.clone();
        if !self.chunks.borrow().contains(&id) && id != self.main {
            self.answered.borrow_mut().insert(id.clone(), module);
        }
        self.resolved.borrow_mut().insert(key, id.clone());
        Ok(id)
    }

    /// `name`'s module when it is project code; `None` sends the loader on
    /// to the built-ins and the libraries.
    pub fn load<'js>(&self, ctx: &Ctx<'js>, name: &str) -> Option<Result<Module<'js, Declared>>> {
        if name == self.main {
            let body = self.body.borrow_mut().take()?;
            return Some(self.declare(ctx, name, body));
        }
        let module = self.answered.borrow_mut().remove(name)?;
        let declared = self
            .body_of(ctx, &module)
            .and_then(|body| self.declare(ctx, name, body));
        // A module that failed to compile stays answered, so a script that
        // catches the failure and imports it again hears the same failure.
        if declared.is_err() {
            self.answered.borrow_mut().insert(name.to_string(), module);
        }
        Some(declared)
    }

    /// A local module as the loader declares it: its bytecode when this
    /// build wrote it, its source through the front end otherwise.
    fn body_of<'js>(&self, ctx: &Ctx<'js>, module: &ResolvedModule) -> Result<Body> {
        match Body::from_artifact(self.frontend, module.artifact.as_ref(), &module.source) {
            Ok(Some(body)) => return Ok(body),
            Ok(None) => {}
            Err(why) => return Err(throw_error(ctx, &format!("module '{}': {why}", module.id))),
        }
        match self.frontend.prepare(&module.source) {
            Ok(prepared) => Ok(Body::Source(prepared)),
            Err(errors) => {
                let first = errors.into_iter().next();
                let place = first
                    .as_ref()
                    .and_then(|error| error.line)
                    .map_or_else(String::new, |line| format!(":{line}"));
                let message = first.map_or_else(
                    || "could not be prepared".to_string(),
                    |error| error.message,
                );
                Err(rquickjs::Exception::throw_syntax(
                    ctx,
                    &format!("{}{place}: {message}", module.id),
                ))
            }
        }
    }

    fn declare<'js>(
        &self,
        ctx: &Ctx<'js>,
        name: &str,
        body: Body,
    ) -> Result<Module<'js, Declared>> {
        match body {
            Body::Bytecode(Bytecode { bytes, map }) => {
                self.chunks.borrow_mut().insert(name, map);
                // SAFETY: QuickJS does not verify bytecode, and
                // `Module::load` checks only in a debug assertion that the
                // bytes decode to a module before treating the value as
                // one, so they must be a module this engine wrote. They
                // reach here only through `Body::from_artifact`, which has
                // checked with `artifact::usable` that they are this
                // engine's format. That they are a module this interpreter's
                // own `validate` wrote rests on the host: it hands back an
                // artifact only to the interpreter whose id it was stamped
                // with, which matters because JavaScript and TypeScript share
                // one engine stamp.
                let module = unsafe { Module::load(ctx.clone(), &bytes) }?;
                let compiled_as: String = module.name()?;
                if compiled_as != name {
                    return Err(throw_error(
                        ctx,
                        &format!(
                            "the prepared artifact for '{name}' was compiled as '{compiled_as}'"
                        ),
                    ));
                }
                Ok(module)
            }
            Body::Source(Prepared { code, map, .. }) => {
                self.chunks.borrow_mut().insert(name, map);
                Module::declare(ctx.clone(), name, code)
            }
        }
    }
}

/// Project code first, then what [`Libraries`] serves.
pub struct Load {
    pub project: Rc<Project>,
    pub libraries: Libraries,
}

impl Loader for Load {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js, Declared>> {
        if let Some(module) = self.project.load(ctx, name) {
            return module;
        }
        self.libraries.load(ctx, name, attributes)
    }
}

fn missing_module(ctx: &Ctx<'_>, name: &str) -> rquickjs::Error {
    if name.starts_with(builtins::PREFIX) {
        return throw_error(
            ctx,
            &format!(
                "module '{name}' not found -- built-in modules are: {}",
                builtins::MODULES.join(", ")
            ),
        );
    }
    throw_error(
        ctx,
        &format!("module '{name}' not found -- is the '{name}' library plugin installed?"),
    )
}

/// What a run imports from.
pub struct Libraries {
    backend: Rc<dyn Backend>,
    libraries: Vec<LibraryRef>,
    calls: Rc<Calls>,
}

impl Libraries {
    pub fn new(backend: Rc<dyn Backend>, libraries: &[LibraryRef], calls: Rc<Calls>) -> Self {
        Self {
            backend,
            libraries: libraries.to_vec(),
            calls,
        }
    }
}

impl Loader for Libraries {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js, Declared>> {
        let exports = match builtins::module(ctx, name, &self.backend)? {
            Some(exports) => exports,
            // The prefix is decided here rather than left to the lookup, so a
            // library registered under an `internal.` namespace cannot be
            // served by one path and refused by the other.
            None if name.starts_with(builtins::PREFIX) => return Err(missing_module(ctx, name)),
            None => {
                let library = self
                    .libraries
                    .iter()
                    .find(|library| library.namespace == name)
                    .ok_or_else(|| missing_module(ctx, name))?;
                let surface = match self.backend.library_surface(&library.id) {
                    Ok(surface) => surface,
                    Err(error) => return Err(ctx.throw(library_error(ctx, name, &error)?)),
                };
                bridge::module(ctx, &library.id, &surface, &self.calls)?
            }
        };
        let names: Vec<String> = exports.keys::<String>().collect::<Result<_>>()?;
        let module = Module::declare(ctx.clone(), name, exporting(&names, "import.meta.exports"))?;
        module.meta()?.set("exports", exports)?;
        Ok(module)
    }
}

/// The source of a module that exports `object` as its default and each of
/// `names` from it. The object reaches the module through its own
/// `import.meta`, which no other module can read, rather than through
/// anything global a script could reach first.
///
/// Each name is exported as a string literal, which ES2022 allows, so a
/// library function called `delete` or `get-thing` is importable by name
/// where an identifier could not carry it. A `default` among them would
/// collide with the default export, and stays reachable through it.
fn exporting(names: &[String], object: &str) -> String {
    let mut source = format!("const exports = {object};\nexport default exports;\n");
    for (i, name) in names.iter().filter(|name| *name != "default").enumerate() {
        let quoted = serde_json::to_string(name).expect("a string always encodes");
        source.push_str(&format!(
            "const e{i} = exports[{quoted}];\nexport {{ e{i} as {quoted} }};\n"
        ));
    }
    source
}

/// For `validate`: every module, whatever its name, is a stub whose every
/// property is a function returning another stub, so file scope of any
/// shape evaluates without a host or an instance of anything. A stub is not
/// thenable, so awaiting one at the top level settles at once.
///
/// A named import needs the stub to export that name, and nothing tells the
/// loader which names the importer wants before linking fails. So `wanted`
/// is filled from each failure — QuickJS names the missing export and the
/// module — and validation runs again; see `lib.rs`.
pub struct Stubs {
    pub wanted: Rc<RefCell<BTreeMap<String, BTreeSet<String>>>>,
}

const STUB: &str = r#"
const stub = () => new Proxy({}, {
  get: (_, key) => key === "then" || typeof key === "symbol" ? undefined : function () { return stub(); },
});
"#;

impl Loader for Stubs {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js, Declared>> {
        let names: Vec<String> = self
            .wanted
            .borrow()
            .get(name)
            .map(|names| names.iter().cloned().collect())
            .unwrap_or_default();
        // The stub's properties are minted on read, so each named export is
        // read off it like any other: a function returning a stub.
        let source = format!(
            "{STUB}const object = stub();\n{}",
            exporting(&names, "object")
        );
        Module::declare(ctx.clone(), name, source)
    }
}

/// Read a link failure's missing export, as QuickJS words it:
/// `Could not find export 'records' in module 'happyview.db'`.
pub fn missing_export(message: &str) -> Option<(String, String)> {
    let rest = message.strip_prefix("Could not find export '")?;
    let (export, rest) = rest.split_once("' in module '")?;
    let module = rest.strip_suffix('\'')?;
    Some((module.to_string(), export.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_failure_names_the_module_and_the_export() {
        assert_eq!(
            missing_export("Could not find export 'records' in module 'happyview.db'"),
            Some(("happyview.db".to_string(), "records".to_string()))
        );
        assert_eq!(missing_export("something else"), None);
    }

    #[test]
    fn every_name_is_exported_as_a_string_literal_and_default_is_not() {
        let source = exporting(
            &[
                "records".to_string(),
                "delete".to_string(),
                "default".to_string(),
            ],
            "x",
        );
        assert!(
            source.contains(r#"export { e0 as "records" };"#),
            "{source}"
        );
        assert!(source.contains(r#"export { e1 as "delete" };"#), "{source}");
        assert!(!source.contains(r#""default""#), "{source}");
    }
}
