//! One installed library's `ApiSurface` as the object its module exports.
//! The shapes are the Lua bridge's, so a library sees the same documents
//! from either language: a function's arguments as a JSON array, and a
//! constructor's chain as `{args, steps, call}` sent as one argument.
//!
//! What differs is that every call that *sends* — a function, or a chain's
//! immediate method — returns a promise rather than blocking, so a script
//! can have several in flight. A chain's lazy steps send nothing and stay
//! synchronous, which is what keeps `db.records(c).where(...).limit(5)`
//! chainable.

use std::cell::RefCell;
use std::rc::Rc;

use happyview_plugin_sdk::{ApiMethod, ApiSurface, Value as Json};
use rquickjs::function::{Rest, This};
use rquickjs::{Ctx, Function, Object, Promise, Result, Value};
use serde_json::{json, Map};

use crate::calls::Calls;
use crate::convert;

/// The library's exports as one object. Keys are the surface's canonical
/// names, which is what every document on the wire carries. An export in a
/// kind this interpreter does not serve — a constant, say — is left out
/// rather than guessed at.
pub fn module<'js>(
    ctx: &Ctx<'js>,
    library: &str,
    surface: &ApiSurface,
    calls: &Rc<Calls>,
) -> Result<Object<'js>> {
    let module = Object::new(ctx.clone())?;
    for export in &surface.exports {
        if export.is_function() {
            module.set(
                export.name.as_str(),
                library_function(ctx, calls, library, &export.name)?,
            )?;
        } else if export.is_constructor() {
            module.set(
                export.name.as_str(),
                constructor(ctx, calls, library, &export.name, &export.methods)?,
            )?;
        }
    }
    Ok(module)
}

fn library_function<'js>(
    ctx: &Ctx<'js>,
    calls: &Rc<Calls>,
    library: &str,
    name: &str,
) -> Result<Function<'js>> {
    let calls = Rc::clone(calls);
    let (library, function) = (library.to_string(), name.to_string());
    let label = format!("{library}.{function}");
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> Result<Promise<'js>> {
            let args = convert::arguments(&ctx, args.0);
            calls.send(&ctx, &library, &function, &label, args)
        },
    )?
    .with_name(name)
}

/// What a chain has accumulated: the constructor's own arguments, and each
/// step in order. Held in Rust rather than on the object, so a script can
/// neither read nor rewrite a document between building and sending it.
#[derive(Default)]
struct Chain {
    args: Vec<Json>,
    steps: Vec<(String, Vec<Json>)>,
}

impl Chain {
    /// The object and the call as one document: the constructor's own
    /// arguments, the accumulated steps in order, and the call that ends the
    /// chain.
    fn document(&self, name: &str, call_args: Vec<Json>) -> Json {
        let steps: Vec<Json> = self
            .steps
            .iter()
            .map(|(step, args)| {
                let mut one = Map::new();
                one.insert(step.clone(), Json::Array(args.clone()));
                Json::Object(one)
            })
            .collect();
        json!({
            "args": self.args,
            "steps": steps,
            "call": { "name": name, "args": call_args },
        })
    }
}

/// A constructor hands out a fresh object per call, its methods closed over
/// that object's chain. `lazy` appends a step and returns the object;
/// `immediate` sends the chain and the call as one document.
fn constructor<'js>(
    ctx: &Ctx<'js>,
    calls: &Rc<Calls>,
    library: &str,
    constructor_name: &str,
    methods: &[ApiMethod],
) -> Result<Function<'js>> {
    // Keyed by each method's own canonical name, never by position: `mode` is
    // an unvalidated string from the plugin, and a method in a mode this
    // interpreter does not recognise has to be skipped rather than shift a
    // later one onto the wrong key.
    let methods: Vec<(String, bool)> = methods
        .iter()
        .filter(|method| method.is_lazy() || method.is_immediate())
        .map(|method| (method.name.clone(), method.is_lazy()))
        .collect();
    let calls = Rc::clone(calls);
    let (library, constructor_name) = (library.to_string(), constructor_name.to_string());
    let name = constructor_name.clone();

    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> Result<Object<'js>> {
            let chain = Rc::new(RefCell::new(Chain {
                args: convert::arguments(&ctx, args.0)?,
                steps: Vec::new(),
            }));
            let object = Object::new(ctx.clone())?;
            for (method, lazy) in &methods {
                let function = if *lazy {
                    lazy_method(&ctx, &chain, method)?
                } else {
                    immediate_method(&ctx, &calls, &chain, &library, &constructor_name, method)?
                };
                object.set(method.as_str(), function)?;
            }
            Ok(object)
        },
    )?
    .with_name(name.as_str())
}

/// Appends `{name: args}` and returns the object, so a chain reads left to
/// right and the document reads the same way. Synchronous, so an argument
/// that will not convert throws here, at the step that carried it.
fn lazy_method<'js>(
    ctx: &Ctx<'js>,
    chain: &Rc<RefCell<Chain>>,
    name: &str,
) -> Result<Function<'js>> {
    let chain = Rc::clone(chain);
    let step = name.to_string();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              this: This<Value<'js>>,
              args: Rest<Value<'js>>|
              -> Result<Value<'js>> {
            let args = convert::arguments(&ctx, args.0)?;
            chain.borrow_mut().steps.push((step.clone(), args));
            Ok(this.0)
        },
    )?
    .with_name(name)
}

fn immediate_method<'js>(
    ctx: &Ctx<'js>,
    calls: &Rc<Calls>,
    chain: &Rc<RefCell<Chain>>,
    library: &str,
    constructor_name: &str,
    name: &str,
) -> Result<Function<'js>> {
    let (calls, chain) = (Rc::clone(calls), Rc::clone(chain));
    let (library, constructor_name, method) = (
        library.to_string(),
        constructor_name.to_string(),
        name.to_string(),
    );
    // The Lua bridge's label, `:` and all, so a failure reads the same from
    // either language.
    let label = format!("{library}.{constructor_name}:{method}");
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> Result<Promise<'js>> {
            let document = convert::arguments(&ctx, args.0)
                .map(|call_args| vec![chain.borrow().document(&method, call_args)]);
            calls.send(&ctx, &library, &constructor_name, &label, document)
        },
    )?
    .with_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::Fake;
    use crate::backend::Backend;
    use crate::budget;
    use crate::sandbox;
    use happyview_plugin_sdk::{ApiExport, ExecuteLimits, PluginError};
    use rquickjs::FromJs;

    /// A surface in the shape the standard `db` library publishes: one plain
    /// function, one constructor with two lazy steps and one immediate call,
    /// and a constant this interpreter does not serve.
    pub(crate) fn surface() -> ApiSurface {
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

    /// The module bound as the global `db`, on `fake`, with the event loop's
    /// `settle` bound as the global `settle(promise)` → its value or its
    /// rejection's message.
    fn with_db<R>(fake: Fake, f: impl for<'js> FnOnce(Ctx<'js>, &Fake) -> R) -> R {
        let fake = Rc::new(fake);
        let backend: Rc<dyn Backend> = fake.clone();
        let (runtime, budget) = budget::runtime().unwrap();
        let limits = ExecuteLimits {
            instructions: None,
            memory_bytes: 0,
        };
        budget::install(&runtime, &budget, &limits);
        let calls = Rc::new(Calls::new(backend.clone()));
        let context = sandbox::create(&runtime, backend).unwrap();
        context.with(|ctx| {
            let _clear = crate::calls::ClearOnDrop(&calls);
            let db = module(&ctx, "happyview-db", &surface(), &calls).unwrap();
            ctx.globals().set("db", db).unwrap();
            install_settle(&ctx, calls.clone(), budget);
            f(ctx, &fake)
        })
    }

    fn install_settle<'js>(ctx: &Ctx<'js>, calls: Rc<Calls>, budget: budget::Budget) {
        let settle = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, promise: Promise<'js>| -> Result<Value<'js>> {
                match calls.run(&ctx, &promise, &budget) {
                    Ok(value) => Ok(value),
                    Err(crate::calls::Unsettled::Thrown(_)) => {
                        let thrown = ctx.catch();
                        let object = thrown.as_object().unwrap();
                        let summary = Object::new(ctx.clone())?;
                        for key in ["message", "code", "retryable"] {
                            summary.set(key, object.get::<_, Value>(key)?)?;
                        }
                        Ok(summary.into_value())
                    }
                    Err(_) => Ok(rquickjs::String::from_str(ctx, "unsettled")?.into_value()),
                }
            },
        )
        .unwrap();
        ctx.globals().set("settle", settle).unwrap();
    }

    fn eval<T: for<'js> FromJs<'js>>(ctx: &Ctx<'_>, source: &str) -> T {
        ctx.eval(source)
            .unwrap_or_else(|e| panic!("{source}: {e}: {:?}", ctx.catch()))
    }

    fn json_of(ctx: &Ctx<'_>, source: &str) -> Json {
        let value: Value = ctx
            .eval(source)
            .unwrap_or_else(|e| panic!("{source}: {e}: {:?}", ctx.catch()));
        convert::to_json(ctx, value).unwrap()
    }

    #[test]
    fn a_surface_becomes_functions_and_constructors_and_nothing_else() {
        with_db(Fake::new(), |ctx, _| {
            assert_eq!(
                eval::<Vec<String>>(&ctx, "Object.keys(db).sort()"),
                vec!["count", "records"]
            );
            assert!(eval::<bool>(
                &ctx,
                "typeof db.count === 'function' && db.count.name === 'count'"
            ));
        });
    }

    #[test]
    fn a_method_in_an_unrecognised_mode_is_skipped_by_name() {
        with_db(Fake::new(), |ctx, _| {
            assert_eq!(
                eval::<Vec<String>>(&ctx, "Object.keys(db.records('c'))"),
                vec!["where", "limit", "all"]
            );
        });
    }

    #[test]
    fn a_function_sends_its_arguments_and_resolves_with_the_result() {
        let fake =
            Fake::new().respond(|_, function, args| Ok(json!({ "fn": function, "args": args })));
        with_db(fake, |ctx, fake| {
            assert_eq!(
                json_of(&ctx, "settle(db.count('app.test.rec', { a: 1 }))"),
                json!({ "fn": "count", "args": ["app.test.rec", { "a": 1 }] })
            );
            assert_eq!(fake.calls.borrow()[0].0, "happyview-db");
        });
    }

    #[test]
    fn a_chain_builds_the_document_the_call_carries() {
        let fake = Fake::new().respond(|_, _, args| Ok(args[0].clone()));
        with_db(fake, |ctx, fake| {
            assert_eq!(
                json_of(&ctx, "settle(db.records('c').where('a', 1).limit(5).all())"),
                json!({
                    "args": ["c"],
                    "steps": [{ "where": ["a", 1] }, { "limit": [5] }],
                    "call": { "name": "all", "args": [] },
                })
            );
            // The function the host is asked for is the constructor's.
            assert_eq!(fake.calls.borrow()[0].1, "records");
        });
    }

    #[test]
    fn an_undefined_argument_cuts_the_step_it_is_in_and_nothing_after_it() {
        let fake = Fake::new().respond(|_, _, args| Ok(args[0].clone()));
        with_db(fake, |ctx, _| {
            let document = json_of(
                &ctx,
                "settle(db.records('c').where('a', undefined, 3).limit(5).all(undefined))",
            );
            assert_eq!(
                document["steps"],
                json!([{ "where": ["a"] }, { "limit": [5] }])
            );
            assert_eq!(document["call"]["args"], json!([]));
        });
    }

    #[test]
    fn an_empty_chain_still_carries_its_three_keys() {
        let fake = Fake::new().respond(|_, _, args| Ok(args[0].clone()));
        with_db(fake, |ctx, _| {
            assert_eq!(
                json_of(&ctx, "settle(db.records().all())"),
                json!({ "args": [], "steps": [], "call": { "name": "all", "args": [] } })
            );
        });
    }

    #[test]
    fn two_chains_from_one_constructor_do_not_share_steps() {
        let fake = Fake::new().respond(|_, _, args| Ok(args[0].clone()));
        with_db(fake, |ctx, _| {
            let value = json_of(
                &ctx,
                "const a = db.records('a').limit(1); const b = db.records('b'); \
                 [settle(a.all()), settle(b.all())]",
            );
            assert_eq!(value[0]["steps"], json!([{ "limit": [1] }]));
            assert_eq!(value[1]["steps"], json!([]));
        });
    }

    #[test]
    fn sending_returns_a_promise_and_a_lazy_step_returns_the_chain() {
        with_db(Fake::new(), |ctx, _| {
            assert!(eval::<bool>(
                &ctx,
                "const q = db.records('c'); \
                 db.count() instanceof Promise && q.all() instanceof Promise && q.where('x') === q"
            ));
        });
    }

    #[test]
    fn a_library_error_rejects_with_the_lua_bridges_wording() {
        let fake = Fake::new().respond(|_, function, _| match function {
            "count" => {
                Err(PluginError::new("NO_SESSION", "AUTH_ERROR: no PDS session").retryable())
            }
            _ => Err(PluginError::new(
                "LIBRARY_ERROR",
                "Library call depth limit (8) exceeded",
            )),
        });
        with_db(fake, |ctx, _| {
            assert_eq!(
                json_of(&ctx, "settle(db.count())"),
                json!({
                    "message": "happyview-db.count: Plugin returned error: NO_SESSION - AUTH_ERROR: no PDS session",
                    "code": "NO_SESSION",
                    "retryable": true,
                })
            );
            assert_eq!(
                json_of(&ctx, "settle(db.records('c').all())"),
                json!({
                    "message": "happyview-db.records:all: Library call depth limit (8) exceeded",
                    "code": "LIBRARY_ERROR",
                    "retryable": false,
                })
            );
        });
    }

    #[test]
    fn a_call_that_cannot_start_rejects_at_once() {
        with_db(Fake::new().refusing_start(), |ctx, fake| {
            assert!(eval::<bool>(
                &ctx,
                "settle(db.count()).code === 'HOST_ERROR'"
            ));
            assert!(fake.calls.borrow().is_empty());
        });
    }

    #[test]
    fn an_argument_that_will_not_convert_rejects_a_send_and_throws_from_a_step() {
        with_db(Fake::new(), |ctx, fake| {
            assert_eq!(
                eval::<String>(&ctx, "settle(db.count(() => 1)).message"),
                "cannot convert a function to JSON"
            );
            ctx.eval::<Value, _>("db.records('c').where(10n)")
                .expect_err("a step converts as it is taken");
            ctx.catch();
            assert!(fake.calls.borrow().is_empty());
        });
    }
}
