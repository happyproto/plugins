//! `happyview.atproto`: AT Protocol service resolution, blob download, label
//! lookup, and attestation signing. A thin translator over five SDK host
//! wrappers — this crate neither resolves DIDs nor signs anything itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, AtprotoBlobDownload, AttestSign, AttestVerify,
    CallContext, LabelsGet, Map, PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-atproto", "Atproto", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.atproto")
        .describe(
            "AT Protocol service resolution, blob download, label lookup, and attestation signing",
        )
        .export(
            ApiExport::function("resolve_service_endpoint")
                .describe("Resolve the AT Protocol service a DID's document advertises")
                .param("did", "string", "Subject DID"),
        )
        .export(
            ApiExport::function("blob_download")
                .describe("Download a blob from a repo")
                .param("did", "string", "Repo DID")
                .param("cid", "string", "Blob CID")
                .returns(blob_shape()),
        )
        .export(
            ApiExport::function("get_labels")
                .describe("Labels applied to one URI")
                .param("uri", "string", "AT URI"),
        )
        .export(
            ApiExport::function("get_labels_batch")
                .describe("Labels applied to a set of URIs, keyed by URI")
                .param("uris", "array", "AT URIs"),
        )
        .export(
            ApiExport::function("sign")
                .describe("Sign a record with this instance's attestation key")
                .param("record", "object", "Record body"),
        )
        .export(
            ApiExport::function("verify_signature")
                .describe("Verify a record's attestation signature")
                .param("record", "object", "Record body")
                .param("signature", "object", "Inline signature object")
                .param(
                    "repo_did",
                    "string",
                    "Repo DID the record claims to belong to",
                ),
        )
}

fn blob_shape() -> Value {
    json!({"type": "object", "properties": [
        {"name": "bytes", "type": "string", "description": "UTF-8 text, or an array of byte values when not UTF-8"},
        {"name": "mime_type", "type": "string"},
        {"name": "size", "type": "integer"}
    ]})
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "resolve_service_endpoint" => resolve_service_endpoint(args),
        "blob_download" => blob_download(args),
        "get_labels" => get_labels(args),
        "get_labels_batch" => get_labels_batch(args),
        "sign" => sign(args),
        "verify_signature" => verify_signature(args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn resolve_service_endpoint(args: &[Value]) -> Result<Value, PluginError> {
    let did = str_arg(args, 0, "did")?;
    Ok(host::atproto_resolve_service(&did)?
        .map(Value::from)
        .unwrap_or(Value::Null))
}

fn blob_download(args: &[Value]) -> Result<Value, PluginError> {
    let did = str_arg(args, 0, "did")?;
    let cid = str_arg(args, 1, "cid")?;
    let blob = host::atproto_blob_download(&AtprotoBlobDownload { did, cid })?;
    serde_json::to_value(blob).map_err(PluginError::from)
}

fn get_labels(args: &[Value]) -> Result<Value, PluginError> {
    let uri = str_arg(args, 0, "uri")?;
    let mut labels = host::labels_get(&LabelsGet {
        uris: vec![uri.clone()],
    })?;
    let list = labels.remove(&uri).unwrap_or_default();
    serde_json::to_value(list).map_err(PluginError::from)
}

fn get_labels_batch(args: &[Value]) -> Result<Value, PluginError> {
    let uris = str_array_arg(args, 0, "uris")?;
    let labels = host::labels_get(&LabelsGet { uris })?;
    let mut out = Map::new();
    for (uri, list) in labels {
        out.insert(uri, serde_json::to_value(list).map_err(PluginError::from)?);
    }
    Ok(Value::Object(out))
}

fn sign(args: &[Value]) -> Result<Value, PluginError> {
    let record = object_arg(args, 0, "record")?;
    host::attest_sign(&AttestSign { record })
}

fn verify_signature(args: &[Value]) -> Result<Value, PluginError> {
    let record = object_arg(args, 0, "record")?;
    let signature = object_arg(args, 1, "signature")?;
    let repo_did = str_arg(args, 2, "repo_did")?;
    let valid = host::attest_verify(&AttestVerify {
        record,
        signature,
        repo_did,
    })?;
    Ok(Value::Bool(valid))
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{name} is required")))
}

fn str_array_arg(args: &[Value], index: usize, name: &str) -> Result<Vec<String>, PluginError> {
    match args.get(index) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(ToString::to_string).ok_or_else(|| {
                    PluginError::bad_input(format!("{name} must be an array of strings"))
                })
            })
            .collect(),
        _ => Err(PluginError::bad_input(format!(
            "{name} must be an array of strings"
        ))),
    }
}

fn object_arg(args: &[Value], index: usize, name: &str) -> Result<Value, PluginError> {
    match args.get(index) {
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        _ => Err(PluginError::bad_input(format!(
            "{name} is required and must be an object"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_arg_reads_the_indexed_string() {
        let args = [json!("did:plc:abc")];
        assert_eq!(str_arg(&args, 0, "did").unwrap(), "did:plc:abc");
    }

    #[test]
    fn str_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_arg(&args, 0, "did").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn str_array_arg_reads_an_array_of_strings() {
        let args = [json!(["at://did:plc:a/x/1", "at://did:plc:b/x/2"])];
        let uris = str_array_arg(&args, 0, "uris").unwrap();
        assert_eq!(uris, vec!["at://did:plc:a/x/1", "at://did:plc:b/x/2"]);
    }

    #[test]
    fn str_array_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_array_arg(&args, 0, "uris").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn str_array_arg_rejects_a_non_array() {
        let args = [json!("not an array")];
        let err = str_array_arg(&args, 0, "uris").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn str_array_arg_rejects_an_array_with_a_non_string_element() {
        let args = [json!(["ok", 5])];
        let err = str_array_arg(&args, 0, "uris").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn object_arg_passes_through_an_object() {
        let args = [json!({"$type": "app.test.thing"})];
        assert_eq!(
            object_arg(&args, 0, "record").unwrap(),
            json!({"$type": "app.test.thing"})
        );
    }

    #[test]
    fn object_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = object_arg(&args, 0, "record").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn object_arg_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = object_arg(&args, 0, "signature").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }
}
