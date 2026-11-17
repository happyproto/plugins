//! The second argument of `handle`. Everything in it is data and arrives in
//! the execute input — except `ctx.job`'s three functions, which act on the
//! job row while the script is still running, and `ctx.caller_assertion`,
//! which asks the host about the run's caller. Those are host imports. The
//! host holds the job id and the caller for the whole run, so a script cannot
//! name another job or another caller.

use happyview_plugin_sdk::{host, CallerAssertionRequest, ExecuteContext, JobProgressRequest};
use mlua::{IntoLuaMulti, Lua, Result as LuaResult, Table, Value};

use crate::convert;

/// A field that does not apply to this trigger is absent rather than nil, so
/// an interpreter for a language that tells the two apart renders each
/// correctly. In Lua they read the same, which is why this is a property of
/// the input rather than of the table.
pub fn build(lua: &Lua, context: &ExecuteContext) -> LuaResult<Table> {
    let ctx = lua.create_table()?;
    ctx.set("trigger", context.trigger.as_str())?;
    ctx.set("has_pds_auth", context.has_pds_auth)?;
    ctx.set(
        "env",
        convert::to_lua(lua, &serde_json::json!(context.env))?,
    )?;

    if let Some(caller_did) = &context.caller_did {
        ctx.set("caller_did", caller_did.as_str())?;
        ctx.set("caller_assertion", caller_assertion(lua)?)?;
    }
    if let Some(method) = &context.method {
        ctx.set("method", method.as_str())?;
    }
    if let Some(collection) = &context.collection {
        ctx.set("collection", collection.as_str())?;
    }
    if let Some(params) = &context.params {
        ctx.set(
            "params",
            convert::to_lua(lua, &serde_json::Value::Object(params.clone()))?,
        )?;
    }
    if let Some(delegate_did) = &context.delegate_did {
        ctx.set("delegate_did", delegate_did.as_str())?;
    }
    if let Some(space) = &context.space {
        let table = lua.create_table()?;
        table.set("uri", space.uri.as_str())?;
        table.set("id", space.id.as_str())?;
        table.set("did", space.did.as_str())?;
        table.set("authority_did", space.authority_did.as_str())?;
        table.set("spaceType", space.type_nsid.as_str())?;
        table.set("skey", space.skey.as_str())?;
        ctx.set("space", table)?;
    }
    if let Some(job) = &context.job {
        ctx.set("job", job_table(lua, job.id.as_str())?)?;
    }

    Ok(ctx)
}

/// `ctx.caller_assertion(audience)`: a signed statement, from the instance,
/// of who called this script and with which key, for the service named by
/// `audience`. Returns the token, or `nil` and the reason the instance gave:
/// an audience it doesn't allow, a caller who didn't sign in with DPoP, or
/// assertions turned off.
fn caller_assertion(lua: &Lua) -> LuaResult<mlua::Function> {
    lua.create_function(|lua, audience: String| {
        match host::caller_assertion(&CallerAssertionRequest { audience }) {
            Ok(assertion) => (
                Value::String(lua.create_string(&assertion.token)?),
                Value::Nil,
            )
                .into_lua_multi(lua),
            Err(e) => {
                (Value::Nil, Value::String(lua.create_string(&e.message)?)).into_lua_multi(lua)
            }
        }
    })
}

fn job_table(lua: &Lua, id: &str) -> LuaResult<Table> {
    let job = lua.create_table()?;
    job.set("id", id)?;
    job.set(
        "progress",
        lua.create_function(|lua, data: Value| {
            let data = convert::to_json(lua, data)?;
            host::job_progress(&JobProgressRequest { data })
                .map_err(|e| mlua::Error::runtime(format!("job.progress failed: {}", e.message)))
        })?,
    )?;
    job.set(
        "should_stop",
        lua.create_function(|_, ()| {
            host::job_should_stop()
                .map_err(|e| mlua::Error::runtime(format!("job.should_stop failed: {}", e.message)))
        })?,
    )?;
    job.set(
        "wait",
        lua.create_function(|_, seconds: f64| {
            host::job_wait(seconds)
                .map_err(|e| mlua::Error::runtime(format!("job.wait failed: {}", e.message)))
        })?,
    )?;
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::sandbox;
    use serde_json::json;

    fn ctx_vm(context: serde_json::Value) -> Lua {
        let context: ExecuteContext =
            serde_json::from_value(context).expect("the context should deserialize");
        let lua = sandbox();
        let table = build(&lua, &context).unwrap();
        lua.globals().set("ctx", table).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, source: &str) -> T {
        lua.load(source)
            .eval()
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }

    #[test]
    fn an_anonymous_query_carries_the_common_fields_and_nothing_else() {
        let lua = ctx_vm(json!({ "trigger": "xrpc.query:app.test.q", "method": "app.test.q" }));
        assert_eq!(
            eval::<String>(&lua, "return ctx.trigger"),
            "xrpc.query:app.test.q"
        );
        assert_eq!(eval::<String>(&lua, "return ctx.method"), "app.test.q");
        assert!(!eval::<bool>(&lua, "return ctx.has_pds_auth"));
        assert_eq!(eval::<String>(&lua, "return type(ctx.env)"), "table");
        for absent in [
            "caller_did",
            "caller_assertion",
            "collection",
            "params",
            "delegate_did",
            "space",
            "job",
        ] {
            assert!(
                eval::<Value>(&lua, &format!("return ctx.{absent}")).is_nil(),
                "{absent} should be absent"
            );
        }
    }

    #[test]
    fn a_space_scoped_procedure_carries_every_field_it_has() {
        let lua = ctx_vm(json!({
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
        }));
        assert_eq!(
            eval::<String>(
                &lua,
                r#"return ctx.caller_did .. "|" .. tostring(ctx.has_pds_auth) .. "|" .. ctx.env.API
                   .. "|" .. ctx.method .. "|" .. ctx.collection .. "|" .. ctx.params.q
                   .. "|" .. ctx.delegate_did .. "|" .. ctx.space.authority_did
                   .. "|" .. ctx.space.spaceType"#
            ),
            "did:plc:me|true|k|app.test.p|app.test.rec|x|did:plc:delegate|did:plc:auth|t"
        );
    }

    /// Off wasm there's no host to ask, so the call reports why instead of
    /// raising, the same way a refusal from the host comes back.
    #[test]
    fn a_caller_can_ask_for_an_assertion_and_hears_why_not() {
        let lua = ctx_vm(json!({
            "trigger": "xrpc.procedure:app.test.p",
            "caller_did": "did:plc:me",
            "method": "app.test.p",
        }));
        assert_eq!(
            eval::<String>(&lua, "return type(ctx.caller_assertion)"),
            "function"
        );
        let (token, reason): (Value, String) = lua
            .load(r#"return ctx.caller_assertion("https://service.example")"#)
            .eval()
            .unwrap();
        assert!(token.is_nil());
        assert!(!reason.is_empty());
    }

    #[test]
    fn a_record_event_and_a_label_carry_only_what_applies() {
        let lua = ctx_vm(json!({
            "trigger": "record.created:app.test.rec",
            "collection": "app.test.rec",
        }));
        assert_eq!(
            eval::<String>(&lua, "return ctx.collection"),
            "app.test.rec"
        );
        assert!(eval::<Value>(&lua, "return ctx.method").is_nil());

        let lua = ctx_vm(json!({ "trigger": "label.created" }));
        for absent in ["method", "collection", "params", "job"] {
            assert!(
                eval::<Value>(&lua, &format!("return ctx.{absent}")).is_nil(),
                "{absent} should be absent"
            );
        }
    }

    #[test]
    fn a_job_run_carries_its_id_and_exactly_the_four_names() {
        let lua = ctx_vm(json!({
            "trigger": "job.run:reindex",
            "job": { "id": "1a2b" },
        }));
        assert_eq!(eval::<String>(&lua, "return ctx.job.id"), "1a2b");
        assert!(eval::<bool>(
            &lua,
            "return type(ctx.job.progress) == 'function' \
                and type(ctx.job.should_stop) == 'function' \
                and type(ctx.job.wait) == 'function'"
        ));
        let names: Vec<String> = eval(
            &lua,
            "local names = {} for k in pairs(ctx.job) do names[#names + 1] = k end \
             table.sort(names) return names",
        );
        assert_eq!(names, vec!["id", "progress", "should_stop", "wait"]);
    }

    /// The job id is the host's for the whole run, so nothing a script passes
    /// can name another job — the three functions take no id at all.
    #[test]
    fn the_job_functions_reach_the_host_and_name_no_job() {
        let lua = ctx_vm(json!({ "trigger": "job.run:reindex", "job": { "id": "1a2b" } }));
        for (source, label) in [
            ("ctx.job.progress({ done = 1 })", "job.progress failed:"),
            ("return ctx.job.should_stop()", "job.should_stop failed:"),
            ("ctx.job.wait(0)", "job.wait failed:"),
        ] {
            let error = lua
                .load(source)
                .eval::<Value>()
                .expect_err(source)
                .to_string();
            assert!(error.contains(label), "{source}: {error}");
        }
    }
}
