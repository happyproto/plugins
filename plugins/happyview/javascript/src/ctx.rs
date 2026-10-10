//! The second argument of `handle`. Everything in it is data and arrives in
//! the execute input — except `ctx.job`'s three functions, which act on the
//! job row while the script is still running and so are host imports. The
//! host holds the job id for the whole run, so a script cannot name another
//! job.

use std::rc::Rc;

use happyview_plugin_sdk::{ExecuteContext, JobProgressRequest};
use rquickjs::{Ctx, Function, Object, Promise, Result, Value};

use crate::backend::Backend;
use crate::builtins::throw_error;
use crate::convert;

/// A field that does not apply to this trigger is absent rather than
/// `undefined`-valued: `"method" in ctx` is false for a record event, which
/// is the distinction the input draws by leaving the key out.
pub fn build<'js>(
    ctx: &Ctx<'js>,
    context: &ExecuteContext,
    backend: &Rc<dyn Backend>,
) -> Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("trigger", context.trigger.as_str())?;
    object.set("has_pds_auth", context.has_pds_auth)?;
    object.set("env", convert::to_js(ctx, &serde_json::json!(context.env))?)?;

    if let Some(caller_did) = &context.caller_did {
        object.set("caller_did", caller_did.as_str())?;
    }
    if let Some(method) = &context.method {
        object.set("method", method.as_str())?;
    }
    if let Some(collection) = &context.collection {
        object.set("collection", collection.as_str())?;
    }
    if let Some(params) = &context.params {
        object.set(
            "params",
            convert::to_js(ctx, &serde_json::Value::Object(params.clone()))?,
        )?;
    }
    if let Some(delegate_did) = &context.delegate_did {
        object.set("delegate_did", delegate_did.as_str())?;
    }
    if let Some(space) = &context.space {
        let table = Object::new(ctx.clone())?;
        table.set("uri", space.uri.as_str())?;
        table.set("id", space.id.as_str())?;
        table.set("did", space.did.as_str())?;
        table.set("authority_did", space.authority_did.as_str())?;
        table.set("spaceType", space.type_nsid.as_str())?;
        table.set("skey", space.skey.as_str())?;
        object.set("space", table)?;
    }
    if let Some(job) = &context.job {
        object.set("job", job_object(ctx, job.id.as_str(), backend)?)?;
    }

    Ok(object)
}

/// The Lua names, so the docs and the host keep one vocabulary. `progress`
/// and `should_stop` answer at once; `wait` hands back a promise, settled
/// once the host's wait returns, so `await ctx.job.wait(5)` reads as what it
/// does. The guest is blocked for the wait either way — there is nothing
/// else for it to run, since a job's library calls settle only when waited
/// on.
fn job_object<'js>(ctx: &Ctx<'js>, id: &str, backend: &Rc<dyn Backend>) -> Result<Object<'js>> {
    let job = Object::new(ctx.clone())?;
    job.set("id", id)?;

    let progress = backend.clone();
    job.set(
        "progress",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, data: Value<'js>| -> Result<()> {
                let data = convert::to_json(&ctx, data)?;
                progress
                    .job_progress(&JobProgressRequest { data })
                    .map_err(|e| throw_error(&ctx, &format!("job.progress failed: {}", e.message)))
            },
        )?
        .with_name("progress")?,
    )?;

    let should_stop = backend.clone();
    job.set(
        "should_stop",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| -> Result<bool> {
            should_stop
                .job_should_stop()
                .map_err(|e| throw_error(&ctx, &format!("job.should_stop failed: {}", e.message)))
        })?
        .with_name("should_stop")?,
    )?;

    let wait = backend.clone();
    job.set(
        "wait",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, seconds: f64| -> Result<Promise<'js>> {
                let (promise, resolve, reject) = ctx.promise()?;
                match wait.job_wait(seconds) {
                    Ok(()) => resolve.call::<_, ()>(())?,
                    Err(e) => {
                        let _ = throw_error(&ctx, &format!("job.wait failed: {}", e.message));
                        reject.call::<_, ()>((ctx.catch(),))?
                    }
                }
                Ok(promise)
            },
        )?
        .with_name("wait")?,
    )?;

    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::Fake;
    use crate::sandbox;
    use rquickjs::{FromJs, Runtime};
    use serde_json::json;

    fn with_ctx<R>(context: serde_json::Value, f: impl for<'js> FnOnce(Ctx<'js>, &Fake) -> R) -> R {
        let context: ExecuteContext =
            serde_json::from_value(context).expect("the context should deserialize");
        let fake = Rc::new(Fake::new());
        let backend: Rc<dyn Backend> = fake.clone();
        let runtime = Runtime::new().unwrap();
        let js = sandbox::create(&runtime, backend.clone()).unwrap();
        js.with(|ctx| {
            let object = build(&ctx, &context, &backend).unwrap();
            ctx.globals().set("ctx", object).unwrap();
            f(ctx, &fake)
        })
    }

    fn eval<T: for<'js> FromJs<'js>>(ctx: &Ctx<'_>, source: &str) -> T {
        ctx.eval(source)
            .unwrap_or_else(|e| panic!("{source}: {e}: {:?}", ctx.catch()))
    }

    #[test]
    fn an_anonymous_query_carries_the_common_fields_and_nothing_else() {
        with_ctx(
            json!({ "trigger": "xrpc.query:app.test.q", "method": "app.test.q" }),
            |ctx, _| {
                assert_eq!(eval::<String>(&ctx, "ctx.trigger"), "xrpc.query:app.test.q");
                assert_eq!(eval::<String>(&ctx, "ctx.method"), "app.test.q");
                assert!(!eval::<bool>(&ctx, "ctx.has_pds_auth"));
                assert!(eval::<bool>(&ctx, "typeof ctx.env === 'object'"));
                assert_eq!(
                    eval::<Vec<String>>(&ctx, "Object.keys(ctx).sort()"),
                    vec!["env", "has_pds_auth", "method", "trigger"]
                );
                for absent in [
                    "caller_did",
                    "collection",
                    "params",
                    "delegate_did",
                    "space",
                    "job",
                ] {
                    assert!(
                        !eval::<bool>(&ctx, &format!("'{absent}' in ctx")),
                        "{absent}"
                    );
                }
            },
        );
    }

    #[test]
    fn a_space_scoped_procedure_carries_every_field_it_has() {
        with_ctx(
            json!({
                "trigger": "xrpc.procedure:app.test.p",
                "caller_did": "did:plc:me",
                "has_pds_auth": true,
                "env": { "API": "k" },
                "method": "app.test.p",
                "collection": "app.test.rec",
                "params": { "q": "x" },
                "delegate_did": "did:plc:delegate",
                "space": {
                    "uri": "at://s", "id": "sid", "did": "did:plc:a",
                    "authority_did": "did:plc:auth", "spaceType": "t", "skey": "k",
                },
            }),
            |ctx, _| {
                assert_eq!(
                    eval::<String>(
                        &ctx,
                        "[ctx.caller_did, ctx.has_pds_auth, ctx.env.API, ctx.method, \
                          ctx.collection, ctx.params.q, ctx.delegate_did, \
                          ctx.space.authority_did, ctx.space.spaceType].join('|')"
                    ),
                    "did:plc:me|true|k|app.test.p|app.test.rec|x|did:plc:delegate|did:plc:auth|t"
                );
            },
        );
    }

    #[test]
    fn a_record_event_and_a_label_carry_only_what_applies() {
        with_ctx(
            json!({ "trigger": "record.created:app.test.rec", "collection": "app.test.rec" }),
            |ctx, _| {
                assert_eq!(eval::<String>(&ctx, "ctx.collection"), "app.test.rec");
                assert!(!eval::<bool>(&ctx, "'method' in ctx"));
            },
        );
        with_ctx(json!({ "trigger": "label.created" }), |ctx, _| {
            for absent in ["method", "collection", "params", "job"] {
                assert!(
                    !eval::<bool>(&ctx, &format!("'{absent}' in ctx")),
                    "{absent}"
                );
            }
        });
    }

    #[test]
    fn a_job_run_carries_its_id_and_exactly_the_four_names() {
        with_ctx(
            json!({ "trigger": "job.run:reindex", "job": { "id": "1a2b" } }),
            |ctx, _| {
                assert_eq!(eval::<String>(&ctx, "ctx.job.id"), "1a2b");
                assert_eq!(
                    eval::<Vec<String>>(&ctx, "Object.keys(ctx.job).sort()"),
                    vec!["id", "progress", "should_stop", "wait"]
                );
            },
        );
    }

    /// The job id is the host's for the whole run, so nothing a script passes
    /// can name another job — the three functions take no id at all.
    #[test]
    fn the_job_controls_reach_the_host_and_name_no_job() {
        with_ctx(
            json!({ "trigger": "job.run:reindex", "job": { "id": "1a2b" } }),
            |ctx, fake| {
                fake.should_stop.set(true);
                eval::<()>(&ctx, "ctx.job.progress({ done: 1 })");
                assert!(eval::<bool>(&ctx, "ctx.job.should_stop()"));
                let waited: Value = ctx.eval("ctx.job.wait(2.5)").unwrap();
                assert!(waited.is_promise());
                assert_eq!(*fake.progress.borrow(), vec![json!({ "done": 1 })]);
                assert_eq!(*fake.waits.borrow(), vec![2.5]);
            },
        );
    }

    /// Off wasm the real host answers `NotWasm`, which is what a failing
    /// control looks like from the script.
    #[test]
    fn a_failing_job_control_names_itself() {
        let context: ExecuteContext =
            serde_json::from_value(json!({ "trigger": "job.run:x", "job": { "id": "1" } }))
                .unwrap();
        let backend: Rc<dyn Backend> = Rc::new(crate::backend::Host);
        let runtime = Runtime::new().unwrap();
        let js = sandbox::create(&runtime, backend.clone()).unwrap();
        js.with(|ctx| {
            ctx.globals()
                .set("ctx", build(&ctx, &context, &backend).unwrap())
                .unwrap();
            for (source, label) in [
                ("ctx.job.progress({ done: 1 })", "job.progress failed:"),
                ("ctx.job.should_stop()", "job.should_stop failed:"),
            ] {
                ctx.eval::<Value, _>(source).expect_err(source);
                let thrown = ctx.catch();
                let message: String = thrown.as_object().unwrap().get("message").unwrap();
                assert!(message.starts_with(label), "{source}: {message}");
            }
            let promise: Promise = ctx.eval("ctx.job.wait(0)").unwrap();
            assert_eq!(promise.state(), rquickjs::promise::PromiseState::Rejected);
            // A rejection nobody handles is still a value the test can read.
            let _ = promise.result::<Value>();
            let reason = ctx.catch();
            let message: String = reason.as_object().unwrap().get("message").unwrap();
            assert!(message.starts_with("job.wait failed:"), "{message}");
        });
    }
}
