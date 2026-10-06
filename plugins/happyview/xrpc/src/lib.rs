//! `happyview.xrpc`: XRPC query and procedure calls as the calling script's
//! user. A thin translator over two SDK host wrappers — this crate neither
//! builds requests nor talks to the network itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    library_plugin, ApiExport, ApiSurface, CallContext, CallerXrpcProcedure, CallerXrpcQuery, Map,
    PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-xrpc", "XRPC", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.xrpc")
        .describe("XRPC query and procedure calls as the calling user")
        .export(
            ApiExport::function("query")
                .describe("Send an XRPC query as the calling user")
                .param("method", "string", "XRPC method NSID")
                .param("params", "object?", "Query parameters"),
        )
        .export(
            ApiExport::function("procedure")
                .describe("Send an XRPC procedure as the calling user")
                .param("method", "string", "XRPC method NSID")
                .param("input", "object?", "Procedure input body")
                .param("params", "object?", "Query parameters"),
        )
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "query" => query(args),
        "procedure" => procedure(args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn query(args: &[Value]) -> Result<Value, PluginError> {
    let method = str_arg(args, 0, "method")?;
    let params = params_arg(args, 1)?;
    host::caller_xrpc_query(&CallerXrpcQuery { method, params })
}

fn procedure(args: &[Value]) -> Result<Value, PluginError> {
    let method = str_arg(args, 0, "method")?;
    let input = input_arg(args, 1)?;
    let params = params_arg(args, 2)?;
    host::caller_xrpc_procedure(&CallerXrpcProcedure {
        method,
        input,
        params,
    })
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{name} is required")))
}

fn params_arg(args: &[Value], index: usize) -> Result<Map<String, Value>, PluginError> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        _ => Err(PluginError::bad_input("params must be an object")),
    }
}

fn input_arg(args: &[Value], index: usize) -> Result<Value, PluginError> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        _ => Err(PluginError::bad_input("input must be an object")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::json;

    #[test]
    fn str_arg_reads_the_indexed_string() {
        let args = [json!("com.atproto.repo.getRecord")];
        assert_eq!(
            str_arg(&args, 0, "method").unwrap(),
            "com.atproto.repo.getRecord"
        );
    }

    #[test]
    fn str_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_arg(&args, 0, "method").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn params_arg_defaults_to_empty_when_absent() {
        let args: [Value; 0] = [];
        assert!(params_arg(&args, 0).unwrap().is_empty());
    }

    #[test]
    fn params_arg_defaults_to_empty_for_null() {
        let args = [Value::Null];
        assert!(params_arg(&args, 0).unwrap().is_empty());
    }

    #[test]
    fn params_arg_passes_through_an_object() {
        let args = [json!({"limit": 10})];
        let params = params_arg(&args, 0).unwrap();
        assert_eq!(params.get("limit"), Some(&json!(10)));
    }

    #[test]
    fn params_arg_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = params_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn input_arg_defaults_to_null_when_absent() {
        let args: [Value; 0] = [];
        assert_eq!(input_arg(&args, 0).unwrap(), Value::Null);
    }

    #[test]
    fn input_arg_defaults_to_null_for_null() {
        let args = [Value::Null];
        assert_eq!(input_arg(&args, 0).unwrap(), Value::Null);
    }

    #[test]
    fn input_arg_passes_through_an_object() {
        let args = [json!({"repo": "did:plc:abc"})];
        assert_eq!(input_arg(&args, 0).unwrap(), json!({"repo": "did:plc:abc"}));
    }

    #[test]
    fn input_arg_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = input_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }
}
