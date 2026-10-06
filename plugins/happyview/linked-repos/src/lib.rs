//! `happyview.linked_repos`: record writes, blob uploads and XRPC calls
//! through repos an admin has linked to this instance. A thin translator
//! over six SDK host wrappers — this crate neither looks up grants nor
//! talks to a PDS itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, LinkedRepoBlobUpload, LinkedRepoCall,
    LinkedRepoRecordCreate, LinkedRepoRecordDelete, LinkedRepoRecordPut, ObjectCall, PluginError,
    PluginInfo, RecordRef, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-linked-repos", "Linked Repos", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.linked_repos")
        .describe("Act through repos an admin has linked to this instance")
        .export(
            ApiExport::function("list")
                .describe("Every linked-repo grant this plugin may act through")
                .returns(json!({"type": "array", "items": grant_shape()})),
        )
        .export(
            ApiExport::constructor("get")
                .describe("Act through one linked-repo grant, by DID")
                .param("did", "string", "The linked repo's DID")
                .immediate("create_record")
                .immediate("put_record")
                .immediate("delete_record")
                .immediate("upload_blob")
                .immediate("call"),
        )
}

fn grant_shape() -> Value {
    json!({"type": "object", "properties": [
        {"name": "id", "type": "string"},
        {"name": "did", "type": "string?"},
        {"name": "handle", "type": "string?"},
        {"name": "reason", "type": "string?"},
        {"name": "status", "type": "string"},
        {"name": "scopes", "type": "string"}
    ]})
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "list" => list(),
        "get" => get(&ObjectCall::from_args(args)?),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn list() -> Result<Value, PluginError> {
    let grants = host::linked_repos_list()?;
    let values: Vec<Value> = grants
        .into_iter()
        .map(|grant| {
            json!({
                "id": grant.id,
                "did": grant.did,
                "handle": grant.handle,
                "reason": grant.reason,
                "status": grant.status,
                "scopes": grant.scopes,
            })
        })
        .collect();
    Ok(Value::Array(values))
}

/// `get(did)` returns no object of its own — every method re-resolves the
/// grant, so there is nothing here to cache between calls, and the object
/// document is the whole state a WASM guest has anyway.
fn get(call: &ObjectCall) -> Result<Value, PluginError> {
    if !call.steps.is_empty() {
        return Err(PluginError::bad_input(
            "get() takes no lazy steps; call a method directly",
        ));
    }
    let did = call
        .args
        .first()
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input("did is required"))?;

    let method_args = call.call.args.as_slice();
    match call.call.name.as_str() {
        "create_record" => create_record(&did, method_args),
        "put_record" => put_record(&did, method_args),
        "delete_record" => delete_record(&did, method_args),
        "upload_blob" => upload_blob(&did, method_args),
        "call" => call_method(&did, method_args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn create_record(did: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let record = required_object_field(table, "record")?;
    let rkey = optional_str_field(table, "rkey");

    let result = host::linked_repo_create_record(&LinkedRepoRecordCreate {
        did: did.to_string(),
        collection,
        rkey,
        record,
    })?;
    Ok(record_ref_value(result))
}

fn put_record(did: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let rkey = required_str_field(table, "rkey")?;
    let record = required_object_field(table, "record")?;
    let swap_cid = optional_str_field(table, "swap_cid");

    let result = host::linked_repo_put_record(&LinkedRepoRecordPut {
        did: did.to_string(),
        collection,
        rkey,
        record,
        swap_cid,
    })?;
    Ok(record_ref_value(result))
}

fn delete_record(did: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let rkey = required_str_field(table, "rkey")?;

    host::linked_repo_delete_record(&LinkedRepoRecordDelete {
        did: did.to_string(),
        collection,
        rkey,
    })?;
    Ok(Value::Bool(true))
}

fn upload_blob(did: &str, args: &[Value]) -> Result<Value, PluginError> {
    let bytes = bytes_arg(args, 0)?;
    let mime_type = str_arg(args, 1, "mime_type")?;
    host::linked_repo_upload_blob(&LinkedRepoBlobUpload {
        did: did.to_string(),
        bytes,
        mime_type,
    })
}

fn call_method(did: &str, args: &[Value]) -> Result<Value, PluginError> {
    let method = str_arg(args, 0, "nsid")?;
    let opts = opts_arg(args, 1)?;
    let params = optional_value_field(opts, "params");
    let input = optional_value_field(opts, "input");

    host::linked_repo_call(&LinkedRepoCall {
        did: did.to_string(),
        method,
        params,
        input,
    })
}

fn record_ref_value(record_ref: RecordRef) -> Value {
    json!({"uri": record_ref.uri, "cid": record_ref.cid})
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{name} is required")))
}

/// The wire type accepts `bytes` as a JSON string (UTF-8 content) or an array
/// of byte values; this mirrors that here, ahead of the host call, so a
/// malformed blob body fails fast with a script-actionable message rather
/// than a `serde` error surfaced through `HOST_ERROR`.
fn bytes_arg(args: &[Value], index: usize) -> Result<Vec<u8>, PluginError> {
    match args.get(index) {
        Some(Value::String(text)) => Ok(text.clone().into_bytes()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_u64()
                    .filter(|n| *n <= u8::MAX as u64)
                    .map(|n| n as u8)
                    .ok_or_else(|| {
                        PluginError::bad_input("bytes must be an array of byte values (0-255)")
                    })
            })
            .collect(),
        _ => Err(PluginError::bad_input(
            "bytes is required and must be a string or an array of bytes",
        )),
    }
}

/// A method's one table argument (`create_record`, `put_record`,
/// `delete_record`). Not optional — every one of these methods needs at
/// least the fields it requires from it.
fn table_arg(args: &[Value], index: usize) -> Result<&Value, PluginError> {
    match args.get(index) {
        Some(value @ Value::Object(_)) => Ok(value),
        _ => Err(PluginError::bad_input(
            "expected a table argument with the method's fields",
        )),
    }
}

/// `call`'s second argument is optional, but when a script does pass
/// something it must be a table — a malformed one (a string, say) silently
/// reading as "no options" would hide the mistake rather than reject it.
fn opts_arg(args: &[Value], index: usize) -> Result<&Value, PluginError> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(&Value::Null),
        Some(value @ Value::Object(_)) => Ok(value),
        _ => Err(PluginError::bad_input("opts must be an object")),
    }
}

fn required_str_field(table: &Value, key: &str) -> Result<String, PluginError> {
    table
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{key} is required")))
}

fn optional_str_field(table: &Value, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn required_object_field(table: &Value, key: &str) -> Result<Value, PluginError> {
    match table.get(key) {
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        _ => Err(PluginError::bad_input(format!(
            "{key} is required and must be an object"
        ))),
    }
}

/// A field that is either absent or `null` reads the same as not having been
/// passed at all — the same rule every optional field in this crate follows.
fn optional_value_field(table: &Value, key: &str) -> Option<Value> {
    match table.get(key) {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn str_arg_reads_the_indexed_string() {
        let args = [json!("did:plc:abc")];
        assert_eq!(str_arg(&args, 0, "nsid").unwrap(), "did:plc:abc");
    }

    #[test]
    fn str_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_arg(&args, 0, "nsid").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn bytes_arg_reads_a_utf8_string() {
        let args = [json!("hello")];
        assert_eq!(bytes_arg(&args, 0).unwrap(), b"hello".to_vec());
    }

    #[test]
    fn bytes_arg_reads_a_byte_array() {
        let args = [json!([0, 159, 146, 150])];
        assert_eq!(bytes_arg(&args, 0).unwrap(), vec![0, 159, 146, 150]);
    }

    #[test]
    fn bytes_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = bytes_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn bytes_arg_rejects_a_non_string_non_array() {
        let args = [json!(42)];
        let err = bytes_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn bytes_arg_rejects_an_out_of_range_byte_value() {
        let args = [json!([1, 2, 300])];
        let err = bytes_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = table_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_rejects_a_non_object() {
        let args = [json!("not a table")];
        let err = table_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_passes_through_an_object() {
        let args = [json!({"collection": "app.test.thing"})];
        assert_eq!(
            table_arg(&args, 0).unwrap(),
            &json!({"collection": "app.test.thing"})
        );
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
    fn opts_arg_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = opts_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn required_str_field_reads_a_present_key() {
        let table = json!({"collection": "app.test.thing"});
        assert_eq!(
            required_str_field(&table, "collection").unwrap(),
            "app.test.thing"
        );
    }

    #[test]
    fn required_str_field_names_a_missing_key() {
        let table = json!({});
        let err = required_str_field(&table, "collection").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
        assert!(err.message.contains("collection"));
    }

    #[test]
    fn optional_str_field_reads_a_present_key() {
        let table = json!({"rkey": "xyz"});
        assert_eq!(optional_str_field(&table, "rkey"), Some("xyz".to_string()));
    }

    #[test]
    fn optional_str_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_str_field(&table, "rkey"), None);
    }

    #[test]
    fn required_object_field_reads_a_present_object() {
        let table = json!({"record": {"text": "hi"}});
        assert_eq!(
            required_object_field(&table, "record").unwrap(),
            json!({"text": "hi"})
        );
    }

    #[test]
    fn required_object_field_names_a_missing_key() {
        let table = json!({});
        let err = required_object_field(&table, "record").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
        assert!(err.message.contains("record"));
    }

    #[test]
    fn required_object_field_rejects_a_non_object() {
        let table = json!({"record": "not an object"});
        let err = required_object_field(&table, "record").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn optional_value_field_reads_a_present_key() {
        let table = json!({"params": {"limit": 10}});
        assert_eq!(
            optional_value_field(&table, "params"),
            Some(json!({"limit": 10}))
        );
    }

    #[test]
    fn optional_value_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_value_field(&table, "params"), None);
    }

    #[test]
    fn optional_value_field_is_none_for_null() {
        let table = json!({"params": null});
        assert_eq!(optional_value_field(&table, "params"), None);
    }
}
