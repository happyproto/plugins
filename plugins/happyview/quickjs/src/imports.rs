//! `import`: a built-in, or an installed library plugin reached through the
//! bridge. Namespaces are paired with plugin ids in the execute input,
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

use happyview_plugin_sdk::LibraryRef;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Module, Result};

use crate::backend::Backend;
use crate::bridge;
use crate::builtins::{self, throw_error};
use crate::calls::{library_error, Calls};

/// A path or a URL names a file this plugin has no way to read, and a bare
/// name that happens to look like one would resolve differently on every
/// host. Everything a script imports is a name the instance already knows.
pub struct Names;

impl Resolver for Names {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        _base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if name.starts_with('.') || name.starts_with('/') || name.contains(':') {
            return Err(throw_error(
                ctx,
                &format!(
                    "cannot import '{name}': a script imports built-in modules and installed \
                     libraries by name, never a path or a URL"
                ),
            ));
        }
        Ok(name.to_string())
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
