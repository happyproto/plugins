//! The context a script runs in: QuickJS's standard intrinsics, `console`,
//! and nothing that turns a string into code at run time.
//!
//! What is absent by construction needs no code here. QuickJS-ng's
//! `quickjs-libc` — the `std` and `os` modules, file and process access,
//! timers — is a separate translation unit `rquickjs` compiles only when
//! asked, and nothing here asks. Everything else a script reaches arrives
//! through `import`.

use std::rc::Rc;

use rquickjs::{Context, Ctx, Result, Runtime};

use crate::backend::Backend;
use crate::builtins;

/// The name the script's module is compiled under. QuickJS writes a frame in
/// it as `script:12:5`, which is what an error's `line` is read out of.
pub const MODULE_NAME: &str = "script";

/// Closes the four routes from a string to code: `eval`, and the
/// constructors of the four function kinds, which are reachable from any
/// function of that kind as `.constructor` and not only as globals. Each
/// constructor is replaced rather than deleted, keeping its prototype, so
/// `instanceof Function` and `fn.constructor === Function` still hold.
///
/// Loading code at run time escapes every check the save path made, which is
/// why the Lua plugin removes `load`; this is the same line drawn in the same
/// place. It runs before the script's module, in the script's own realm, so
/// nothing the script does can come first.
const CLOSE_CODE_GENERATION: &str = r#"
(() => {
  "use strict";
  const message = "code generation from strings is not available to a script";
  const kinds = [function () {}, async function () {}, function* () {}, async function* () {}];
  for (const prototype of kinds.map((kind) => Object.getPrototypeOf(kind))) {
    const closed = function () { throw new EvalError(message); };
    Object.defineProperty(closed, "name", { value: prototype.constructor.name });
    Object.defineProperty(closed, "prototype", { value: prototype });
    Object.defineProperty(prototype, "constructor", {
      value: closed, writable: true, configurable: true, enumerable: false,
    });
  }
  Object.defineProperty(globalThis, "Function", {
    value: Function.prototype.constructor, writable: true, configurable: true, enumerable: false,
  });
  delete globalThis.eval;
})();
"#;

/// A context on `runtime` with the script's globals in place. The engine's
/// own `eval` is still what compiles the script's module — it is the global
/// binding a script could call that is gone, not the compiler.
pub fn create(runtime: &Runtime, backend: Rc<dyn Backend>) -> Result<Context> {
    let context = Context::full(runtime)?;
    context.with(|ctx| prepare(&ctx, backend))?;
    Ok(context)
}

fn prepare(ctx: &Ctx<'_>, backend: Rc<dyn Backend>) -> Result<()> {
    ctx.eval::<(), _>(CLOSE_CODE_GENERATION)?;
    ctx.globals()
        .set("console", builtins::console(ctx, backend)?)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::backend::fake::Fake;
    use rquickjs::Value;

    /// A sandboxed context on a runtime of its own, for a test that needs
    /// only a `Ctx`.
    pub(crate) fn with_context<R>(f: impl for<'js> FnOnce(Ctx<'js>) -> R) -> R {
        let runtime = Runtime::new().expect("a runtime");
        let context = create(&runtime, Rc::new(Fake::new())).expect("the sandbox should build");
        context.with(f)
    }

    fn eval_bool(source: &str) -> bool {
        with_context(|ctx| {
            ctx.eval::<bool, _>(source)
                .unwrap_or_else(|e| panic!("{source}: {e}"))
        })
    }

    fn thrown(source: &str) -> String {
        with_context(|ctx| {
            ctx.eval::<Value, _>(source).expect_err(source);
            let error = ctx.catch();
            let error = error.as_object().expect("an error object");
            format!(
                "{}: {}",
                error.get::<_, String>("name").unwrap(),
                error.get::<_, String>("message").unwrap()
            )
        })
    }

    #[test]
    fn no_string_becomes_code() {
        let refused = "EvalError: code generation from strings is not available to a script";
        for source in [
            "Function('return 1')()",
            "new Function('return 1')",
            "(function () {}).constructor('return 1')",
            "(() => 1).constructor('return 1')",
            "(async function () {}).constructor('return 1')",
            "(function* () {}).constructor('return 1')",
            "(async function* () {}).constructor('return 1')",
            "Reflect.construct(Function, ['return 1'])",
            "(class {}).constructor('return 1')",
        ] {
            assert_eq!(thrown(source), refused, "{source}");
        }
        assert!(eval_bool(
            "typeof eval === 'undefined' && !('eval' in globalThis)"
        ));
    }

    #[test]
    fn closing_code_generation_leaves_functions_ordinary() {
        assert!(eval_bool(
            "(() => 1) instanceof Function \
             && (function () {}).constructor === Function \
             && Function.name === 'Function' \
             && (async () => {}).constructor.name === 'AsyncFunction' \
             && Object.getPrototypeOf(async () => {}).constructor.prototype \
                === Object.getPrototypeOf(async () => {}) \
             && typeof Function.prototype.call === 'function' \
             && !Object.keys(globalThis).includes('Function')"
        ));
    }

    #[test]
    fn the_standard_library_is_present_and_the_host_library_is_not() {
        assert!(eval_bool(
            "typeof JSON.parse === 'function' && typeof Promise.all === 'function' \
             && typeof Map === 'function' && typeof RegExp === 'function' \
             && typeof Date.now === 'function' && typeof Math.random === 'function' \
             && typeof console.log === 'function'"
        ));
        assert!(eval_bool(
            "['std', 'os', 'print', 'scriptArgs', 'setTimeout', 'require', 'process'] \
             .every((name) => typeof globalThis[name] === 'undefined')"
        ));
    }

    /// The guest has no zone to read, so `Date` reads and writes UTC — which
    /// a native test only matches under `TZ=UTC`.
    #[test]
    fn dates_are_utc() {
        assert!(eval_bool(
            "new Date(0).getHours() === 0 && new Date(0).getTimezoneOffset() === 0"
        ));
    }
}
