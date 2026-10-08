//! `happyview.jobs`: enqueue background jobs for the job worker. A thin
//! translator over one SDK host wrapper — this crate neither runs jobs nor
//! resolves their type itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, JobCreate, JobGet, JobListAny,
    JobView, PluginError, PluginInfo, Value,
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
        .export(
            ApiExport::function("get")
                .describe("Read one of the caller's own jobs; nil if it is not theirs or absent")
                .param("id", "string", "The job's id")
                .returns(json!({"type": "object?", "description": "The job, or nil"})),
        )
        .export(
            ApiExport::function("get_any")
                .describe("Read any user's job; nil if absent")
                .param("id", "string", "The job's id")
                .returns(json!({"type": "object?", "description": "The job, or nil"})),
        )
        .export(
            ApiExport::function("list_any")
                .describe("List jobs across every user, newest first")
                .param_json(list_opts_param())
                .returns(json!({"type": "array", "description": "The matching jobs"})),
        )
}

fn list_opts_param() -> Value {
    json!({"name": "opts", "type": "object?", "description": "List filters", "properties": [
        {"name": "status", "type": "array?", "description": "Statuses to include; default every status"},
        {"name": "job_type", "type": "string?", "description": "Only jobs of this type"},
        {"name": "limit", "type": "number?", "description": "Maximum jobs; the host defaults to 50 and caps at 200"}
    ]})
}

fn opts_param() -> Value {
    json!({"name": "opts", "type": "object?", "description": "Job options", "properties": [
        {"name": "auth", "type": "boolean?", "description": "Inherit the caller's PDS auth into the job; default false"}
    ]})
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "create" => create(args),
        "get" => get(args, host::jobs_get),
        "get_any" => get(args, host::jobs_get_any),
        "list_any" => list_any(args),
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

fn get(
    args: &[Value],
    read: fn(&JobGet) -> Result<Option<JobView>, PluginError>,
) -> Result<Value, PluginError> {
    let id = str_arg(args, 0, "id")?;
    match read(&JobGet { id })? {
        Some(view) => to_value(&view),
        None => Ok(Value::Null),
    }
}

fn list_any(args: &[Value]) -> Result<Value, PluginError> {
    let jobs = host::jobs_list_any(&list_opts(args)?)?;
    to_value(&jobs)
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, PluginError> {
    serde_json::to_value(value).map_err(|err| PluginError::host(err.to_string()))
}

/// `opts` is optional, but a non-table is rejected rather than read as "no
/// filters", which would list every job.
fn list_opts(args: &[Value]) -> Result<JobListAny, PluginError> {
    match args.first() {
        None | Some(Value::Null) => Ok(JobListAny::default()),
        Some(value @ Value::Object(_)) => serde_json::from_value(value.clone())
            .map_err(|err| PluginError::bad_input(format!("invalid opts: {err}"))),
        _ => Err(PluginError::bad_input("opts must be a table")),
    }
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

    #[test]
    fn list_opts_reads_status_type_and_limit() {
        let args =
            vec![json!({"status": ["running"], "job_type": "instance.operation", "limit": 10})];
        let spec = list_opts(&args).unwrap();
        assert_eq!(spec.status, vec!["running".to_string()]);
        assert_eq!(spec.job_type.as_deref(), Some("instance.operation"));
        assert_eq!(spec.limit, Some(10));
    }

    #[test]
    fn list_opts_allows_nil() {
        assert_eq!(list_opts(&[]).unwrap(), JobListAny::default());
        assert_eq!(list_opts(&[Value::Null]).unwrap(), JobListAny::default());
    }

    #[test]
    fn list_opts_rejects_a_non_table() {
        assert_eq!(list_opts(&[json!("x")]).unwrap_err().code, "BAD_INPUT");
    }
}
