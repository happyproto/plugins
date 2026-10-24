//! `happyview-http` standard library plugin: outbound HTTP requests through the host.
//! Exports `get`, `post`, `put`, `patch`, `delete` and `head`.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::host::{self, HttpRequest};
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, Bytes, CallContext, Map, PluginError, PluginInfo,
    Value,
};

const METHODS: [&str; 6] = ["get", "post", "put", "patch", "delete", "head"];

library_plugin! {
    info: PluginInfo::new("happyview-http", "HTTP Client", "1.0.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    let opts = json!({"name": "opts", "type": "object?", "description": "Request options", "properties": [
        {"name": "headers", "type": "object?", "description": "Header name to value"},
        {"name": "body", "type": "string?", "description": "Request body (ignored for get/head)"}
    ]});
    let returns = json!({"type": "object", "properties": [
        {"name": "status", "type": "integer"},
        {"name": "body", "type": "string|object", "description": "The body as text, or {\"$bytes\": \"<base64>\"} when it is not valid UTF-8"},
        {"name": "headers", "type": "object", "description": "Lower-cased header name to value"}
    ]});
    ApiSurface::new("happyview.http")
        .describe("Outbound HTTP requests")
        .exports(METHODS.iter().map(|method| {
            ApiExport::function(*method)
                .describe(format!("Send a {} request", method.to_uppercase()))
                .param("url", "string", "Target URL")
                .param_json(opts.clone())
                .returns(returns.clone())
        }))
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    if !METHODS.contains(&function) {
        return Err(PluginError::unknown_function(function));
    }
    request(function, args)
}

fn request(method: &str, args: &[Value]) -> Result<Value, PluginError> {
    let url = args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::bad_input("url is required"))?;
    let opts = args.get(1).unwrap_or(&Value::Null);

    // A body is meaningless on get/head, so it is dropped rather than sent.
    let body = match (method, &opts["body"]) {
        ("get" | "head", _) | (_, Value::Null) => None,
        (_, Value::String(text)) => Some(text.clone().into_bytes()),
        (_, value) => Some(value.to_string().into_bytes()),
    };
    let response = host::http_request(&HttpRequest {
        method: method.to_uppercase(),
        url: url.to_string(),
        headers: request_headers(opts),
        body,
    })?;

    // Header names arrive however the server wrote them; scripts index this map
    // by name, so lower-case it once here.
    let mut headers = Map::new();
    for (name, value) in &response.headers {
        headers.insert(name.to_lowercase(), Value::String(value.clone()));
    }
    let body = response_body(method, response.body);
    Ok(json!({"status": response.status, "body": body, "headers": headers}))
}

/// The body as a script gets it: a string when it is text, and
/// `{"$bytes": "<base64>"}` when it is not, so binary content — an image, a
/// module — arrives whole rather than with its invalid bytes replaced. A HEAD
/// response has no body to report even if the server sent one.
fn response_body(method: &str, body: Vec<u8>) -> Value {
    if method == "head" {
        return Value::from("");
    }
    happyview_plugin_sdk::serde_json::to_value(Bytes(body)).unwrap_or(Value::Null)
}

fn request_headers(opts: &Value) -> Vec<(String, String)> {
    opts["headers"]
        .as_object()
        .map(|headers| {
            headers
                .iter()
                .map(|(name, value)| {
                    let value = match value {
                        Value::String(text) => text.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    (name.clone(), value)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_body_is_a_string() {
        assert_eq!(response_body("get", b"hi".to_vec()), json!("hi"));
    }

    #[test]
    fn a_binary_body_is_dollar_bytes_rather_than_mangled_text() {
        assert_eq!(
            response_body("get", vec![0x00, 0xff]),
            json!({"$bytes": "AP8"})
        );
    }

    #[test]
    fn a_head_response_has_no_body() {
        assert_eq!(response_body("head", b"ignored".to_vec()), json!(""));
    }
}

#[cfg(test)]
mod surface_tests {
    #[test]
    fn the_surface_parses() {
        happyview_plugin_sdk::declarations::assert_surface_parses(&super::surface());
    }
}
