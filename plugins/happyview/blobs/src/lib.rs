//! `happyview.blobs`: store and read byte content addressed by its own CID.
//! A thin translator over three SDK host wrappers — this crate chooses no
//! keys and stores nothing itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-blobs", "Blobs", "0.1.0"),
    surface: surface,
    call: dispatch,
}

/// How bytes travel in both directions, stated once because both the `put`
/// parameter and the `get` result follow it: JSON cannot hold arbitrary bytes
/// in a string, so content that is not valid UTF-8 is an array of byte
/// values instead.
const BYTES_SHAPE: &str = "Content: a string, or an array of byte values";

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.blobs")
        .describe("Store and read byte content addressed by the CID of its contents")
        .export(
            ApiExport::function("put")
                .describe("Store content and return the CID it is filed under")
                .param("bytes", "string", BYTES_SHAPE)
                .param("mime_type", "string", "The content's media type")
                .returns(json!({
                    "type": "string",
                    "description": "The content's CID. Storing the same content twice returns the same CID and stores one copy"
                })),
        )
        .export(
            ApiExport::function("get")
                .describe("Read stored content; nil if nothing is stored under the CID")
                .param("cid", "string", "The content's CID")
                .returns(json!({
                    "type": "object?",
                    "description": "{bytes, mime_type, size}, or nil. Transfers the content — prefer stat when only its size or type is wanted"
                })),
        )
        .export(
            ApiExport::function("stat")
                .describe("A blob's media type and size, without transferring it")
                .param("cid", "string", "The content's CID")
                .returns(json!({
                    "type": "object?",
                    "description": "{cid, mime_type, size}, or nil"
                })),
        )
        .export(
            ApiExport::function("exists")
                .describe("Whether content is stored under the CID")
                .param("cid", "string", "The content's CID")
                .returns(json!({"type": "boolean", "description": "Whether it is held"})),
        )
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "put" => {
            let bytes = bytes_arg(args, 0)?;
            let mime_type = str_arg(args, 1, "mime_type")?;
            Ok(Value::from(host::blob_put(&bytes, &mime_type)?))
        }
        "get" => match host::blob_get(&str_arg(args, 0, "cid")?)? {
            Some(blob) => serde_json::to_value(blob).map_err(PluginError::from),
            None => Ok(Value::Null),
        },
        "stat" => match host::blob_stat(&str_arg(args, 0, "cid")?)? {
            Some(info) => serde_json::to_value(info).map_err(PluginError::from),
            None => Ok(Value::Null),
        },
        "exists" => Ok(Value::from(host::blob_exists(&str_arg(args, 0, "cid")?)?)),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(alloc::format!("{name} is required")))
}

/// A Lua string is a byte string, but a JSON string is not: content that is
/// not valid UTF-8 cannot cross as one. Both shapes are therefore accepted,
/// as `happyview.record`'s `upload_blob` accepts them.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bytes_arg_reads_a_utf8_string() {
        assert_eq!(bytes_arg(&[json!("hello")], 0).unwrap(), b"hello".to_vec());
    }

    #[test]
    fn bytes_arg_reads_a_byte_array() {
        // The case a string cannot carry: 0xff is not valid UTF-8.
        assert_eq!(
            bytes_arg(&[json!([0, 97, 255])], 0).unwrap(),
            vec![0u8, 97, 255]
        );
    }

    #[test]
    fn bytes_arg_rejects_an_out_of_range_byte_value() {
        assert!(bytes_arg(&[json!([256])], 0).is_err());
    }

    #[test]
    fn bytes_arg_rejects_a_non_string_non_array() {
        assert!(bytes_arg(&[json!(7)], 0).is_err());
        assert!(bytes_arg(&[], 0).is_err());
    }

    #[test]
    fn the_surface_names_every_dispatched_function() {
        let surface = serde_json::to_value(surface()).unwrap();
        let declared: Vec<&str> = surface["exports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(declared, ["put", "get", "stat", "exists"]);

        // A declared export that dispatch does not know would be a surface
        // promising what a call cannot answer.
        for name in declared {
            let err = dispatch(name, &[], &CallContext::default()).unwrap_err();
            assert_ne!(
                err.code, "UNKNOWN_FUNCTION",
                "{name} is declared but not dispatched"
            );
        }
    }
}
