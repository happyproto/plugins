//! `happyview.jobs`: enqueue background jobs for the job worker. A thin
//! translator over one SDK host wrapper — this crate neither runs jobs nor
//! resolves their type itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, JobCreate, PluginError, PluginInfo,
    Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-jobs", "Jobs", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.jobs")
        .describe("Enqueue background jobs for the job worker")
        .export(
            ApiExport::function("create")
                .describe("Enqueue a job; returns its id")
                .param("job_type", "string", "Free-form job type name")
                .param("input", "object?", "Job input; nil sends {}")
                .param_json(opts_param())
                .returns(json!({"type": "string", "description": "The job's id"})),
        )
}

fn opts_param() -> Value {
    json!({"name": "opts", "type": "object?", "description": "Job options", "properties": [
        {"name": "auth", "type": "boolean?", "description": "Inherit the caller's PDS auth into the job; default false"}
    ]})
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "create" => create(args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn create(args: &[Value]) -> Result<Value, PluginError> {
    let job_type = str_arg(args, 0, "job_type")?;
    let input = input_arg(args, 1);
    let opts = opts_arg(args, 2)?;
    let auth = opts["auth"].as_bool().unwrap_or(false);

    let id = host::jobs_create(&JobCreate {
        job_type,
        input,
        auth,
    })?;
    Ok(Value::String(id))
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{name} is required")))
}

/// A nil `input` sends `{}` rather than `null`, so a job script always finds
/// `job.input` a table.
fn input_arg(args: &[Value], index: usize) -> Value {
    match args.get(index) {
        None | Some(Value::Null) => json!({}),
        Some(value) => value.clone(),
    }
}

/// `opts` is optional, but when a script does pass something it must be a
/// table — a malformed `opts` (a string, say) silently reading as "no
/// options" would hide the mistake rather than reject it.
fn opts_arg(args: &[Value], index: usize) -> Result<&Value, PluginError> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(&Value::Null),
        Some(value @ Value::Object(_)) => Ok(value),
        _ => Err(PluginError::bad_input("opts must be an object")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_arg_reads_the_indexed_string() {
        let args = [json!("app.test.import")];
        assert_eq!(str_arg(&args, 0, "job_type").unwrap(), "app.test.import");
    }

    #[test]
    fn str_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_arg(&args, 0, "job_type").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn input_arg_defaults_to_an_empty_object_when_absent() {
        let args: [Value; 0] = [];
        assert_eq!(input_arg(&args, 1), json!({}));
    }

    #[test]
    fn input_arg_defaults_to_an_empty_object_for_null() {
        let args = [Value::Null, Value::Null];
        assert_eq!(input_arg(&args, 1), json!({}));
    }

    #[test]
    fn input_arg_passes_through_a_present_value() {
        let args = [Value::Null, json!({"count": 3})];
        assert_eq!(input_arg(&args, 1), json!({"count": 3}));
    }

    #[test]
    fn opts_arg_defaults_to_null_when_absent() {
        let args: [Value; 0] = [];
        assert_eq!(opts_arg(&args, 0).unwrap(), &Value::Null);
    }

    #[test]
    fn opts_arg_defaults_to_null_for_null() {
        let args = [Value::Null];
        assert_eq!(opts_arg(&args, 0).unwrap(), &Value::Null);
    }

    #[test]
    fn opts_arg_passes_through_an_object() {
        let args = [json!({"auth": true})];
        assert_eq!(opts_arg(&args, 0).unwrap(), &json!({"auth": true}));
    }

    #[test]
    fn opts_arg_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = opts_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }
}
