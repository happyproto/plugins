//! `happyview.backlinks`: pages through records whose strong refs point at
//! a given AT URI. A chain is folded into a query spec and sent to one host
//! import; the host owns the join against `happyview_record_refs`.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::string::{String, ToString};

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    library_plugin, ApiExport, ApiSurface, BacklinksQuery, CallContext, ObjectCall, PluginError,
    PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-backlinks", "Backlinks", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.backlinks")
        .describe("Query records that reference a given AT URI")
        .export(
            ApiExport::constructor("to")
                .describe("Start a query for records referencing one AT URI")
                .param("uri", "string", "AT URI")
                .lazy("collection")
                .lazy("did")
                .lazy("limit")
                .lazy("cursor")
                .immediate("run"),
        )
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "to" => to(&ObjectCall::from_args(args)?),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn to(call: &ObjectCall) -> Result<Value, PluginError> {
    let query = fold(call)?;
    match call.call.name.as_str() {
        "run" => serde_json::to_value(host::backlinks_query(&query)?).map_err(PluginError::from),
        other => Err(bad_chain(other, "unknown method")),
    }
}

fn bad_chain(step: &str, msg: &str) -> PluginError {
    PluginError::new("BAD_CHAIN", alloc::format!("{step}: {msg}"))
}

fn string_arg(step: &str, args: &[Value], i: usize, what: &str) -> Result<String, PluginError> {
    args.get(i)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad_chain(step, &alloc::format!("{what} must be a string")))
}

fn fold(call: &ObjectCall) -> Result<BacklinksQuery, PluginError> {
    let uri = call
        .args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::bad_input("to(uri) needs an AT URI"))?
        .to_string();
    let mut collection: Option<String> = None;
    let mut did: Option<String> = None;
    let mut limit: Option<u32> = None;
    let mut cursor: Option<String> = None;
    for step in &call.steps {
        let name = step.name.as_str();
        let args = &step.args;
        // a step whose first argument is absent or nil reads as "not called", so
        // `:limit(input.limit)` is no limit when the input carries none
        if matches!(name, "collection" | "did" | "limit" | "cursor")
            && matches!(args.first(), None | Some(Value::Null))
        {
            continue;
        }
        match name {
            "collection" => collection = Some(string_arg(name, args, 0, "collection")?),
            "did" => did = Some(string_arg(name, args, 0, "did")?),
            "limit" => {
                limit = Some(
                    args.first()
                        .and_then(Value::as_u64)
                        .map(|n| n as u32)
                        .ok_or_else(|| bad_chain(name, "limit must be a non-negative integer"))?,
                )
            }
            "cursor" => cursor = Some(string_arg(name, args, 0, "cursor")?),
            other => return Err(bad_chain(other, "unknown step")),
        }
    }
    let collection = collection.ok_or_else(|| bad_chain("collection", "collection is required"))?;
    Ok(BacklinksQuery {
        uri,
        collection,
        did,
        limit,
        cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::{MethodCall, ObjectCall, Step};
    use serde_json::json;

    #[test]
    fn folds_into_a_backlinks_query() {
        let call = ObjectCall {
            args: vec![json!("at://a/c/1")],
            steps: vec![
                Step {
                    name: "collection".into(),
                    args: vec![json!("app.bsky.feed.like")],
                },
                Step {
                    name: "did".into(),
                    args: vec![json!("did:plc:a")],
                },
                Step {
                    name: "limit".into(),
                    args: vec![json!(7)],
                },
                Step {
                    name: "cursor".into(),
                    args: vec![json!("c")],
                },
            ],
            call: MethodCall {
                name: "run".into(),
                args: vec![],
            },
        };
        let q = fold(&call).unwrap();
        assert_eq!(q.uri, "at://a/c/1");
        assert_eq!(q.collection, "app.bsky.feed.like");
        assert_eq!(q.did.as_deref(), Some("did:plc:a"));
        assert_eq!(q.limit, Some(7));
        assert_eq!(q.cursor.as_deref(), Some("c"));
    }

    #[test]
    fn collection_is_required() {
        let call = ObjectCall {
            args: vec![json!("at://a/c/1")],
            steps: vec![],
            call: MethodCall {
                name: "run".into(),
                args: vec![],
            },
        };
        let err = fold(&call).unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("collection"));
    }

    fn doc(steps: Vec<Step>) -> ObjectCall {
        ObjectCall {
            args: vec![json!("at://a/c/1")],
            steps,
            call: MethodCall {
                name: "run".into(),
                args: vec![],
            },
        }
    }

    #[test]
    fn nil_limit_is_dropped_from_the_chain() {
        for args in [vec![], vec![json!(null)]] {
            let call = doc(vec![
                Step {
                    name: "collection".into(),
                    args: vec![json!("app.bsky.feed.like")],
                },
                Step {
                    name: "limit".into(),
                    args,
                },
            ]);
            let q = fold(&call).unwrap();
            assert_eq!(q.limit, None);
        }
    }

    #[test]
    fn present_limit_still_validates() {
        let call = doc(vec![
            Step {
                name: "collection".into(),
                args: vec![json!("app.bsky.feed.like")],
            },
            Step {
                name: "limit".into(),
                args: vec![json!("ten")],
            },
        ]);
        let err = fold(&call).unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("limit"));
    }

    #[test]
    fn two_nil_steps_in_a_row_are_both_dropped() {
        let call = doc(vec![
            Step {
                name: "collection".into(),
                args: vec![json!("app.bsky.feed.like")],
            },
            Step {
                name: "did".into(),
                args: vec![json!(null)],
            },
            Step {
                name: "cursor".into(),
                args: vec![],
            },
        ]);
        let q = fold(&call).unwrap();
        assert!(q.did.is_none());
        assert!(q.cursor.is_none());
    }

    #[test]
    fn nil_collection_leaves_it_missing() {
        let call = doc(vec![Step {
            name: "collection".into(),
            args: vec![json!(null)],
        }]);
        let err = fold(&call).unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("collection"));
    }
}
