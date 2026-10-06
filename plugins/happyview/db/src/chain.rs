//! Folds the object document a `records(...)` chain produces into the
//! host's query spec. Nothing here touches SQL: the host owns that.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::{
    Condition, Filter, ObjectCall, PluginError, RecordsCount, RecordsQuery, Sort, Value,
};

const OPS: [&str; 9] = ["=", "!=", "<", ">", "<=", ">=", "like", "not like", "ilike"];

#[derive(Debug)]
pub struct RecordsChain {
    pub collection: String,
    pub did: Option<String>,
    pub conditions: Vec<Condition>,
    pub sort: Option<Sort>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
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

/// A filter's `value` argument: a string, number or boolean, passed through
/// as the JSON value the script gave.
fn value_arg(step: &str, args: &[Value], i: usize) -> Result<Value, PluginError> {
    match args.get(i) {
        Some(v @ Value::String(_)) | Some(v @ Value::Number(_)) | Some(v @ Value::Bool(_)) => {
            Ok(v.clone())
        }
        _ => Err(bad_chain(step, "value must be a string, number or boolean")),
    }
}

pub fn fold(call: &ObjectCall) -> Result<RecordsChain, PluginError> {
    let collection = call
        .args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::bad_input("records(collection) needs a collection"))?
        .to_string();
    let mut chain = RecordsChain {
        collection,
        did: None,
        conditions: Vec::new(),
        sort: None,
        limit: None,
        cursor: None,
    };
    for step in &call.steps {
        let name = step.name.as_str();
        let args = &step.args;
        // a step whose first argument is absent or nil reads as "not called", so
        // `:limit(input.limit)` is no limit when the input carries none
        if matches!(name, "where" | "sort" | "limit" | "cursor" | "did")
            && matches!(args.first(), None | Some(Value::Null))
        {
            continue;
        }
        match name {
            "where" => {
                let field = string_arg(name, args, 0, "field")?;
                let op = string_arg(name, args, 1, "op")?;
                if !OPS.contains(&op.to_lowercase().as_str()) {
                    return Err(bad_chain(name, &alloc::format!("unknown operator '{op}'")));
                }
                let value = value_arg(name, args, 2)?;
                chain.conditions.push(Condition { field, op, value });
            }
            "sort" => {
                let field = string_arg(name, args, 0, "field")?;
                let direction = match args.get(1) {
                    None => "desc".to_string(),
                    Some(v) => {
                        let d = v.as_str().unwrap_or("").to_lowercase();
                        if d != "asc" && d != "desc" {
                            return Err(bad_chain(
                                name,
                                &alloc::format!(
                                    "direction '{}' must be asc or desc",
                                    v.as_str().unwrap_or("")
                                ),
                            ));
                        }
                        d
                    }
                };
                chain.sort = Some(Sort { field, direction });
            }
            "limit" => {
                chain.limit = Some(
                    args.first()
                        .and_then(Value::as_u64)
                        .map(|n| n as u32)
                        .ok_or_else(|| bad_chain(name, "limit must be a non-negative integer"))?,
                );
            }
            "cursor" => chain.cursor = Some(string_arg(name, args, 0, "cursor")?),
            "did" => chain.did = Some(string_arg(name, args, 0, "did")?),
            other => return Err(bad_chain(other, "unknown step")),
        }
    }
    Ok(chain)
}

fn filter(chain: &RecordsChain) -> Option<Filter> {
    match chain.conditions.len() {
        0 => None,
        1 => Some(Filter::Condition(chain.conditions[0].clone())),
        _ => Some(Filter::Group {
            combine: "and".to_string(),
            conditions: chain
                .conditions
                .iter()
                .cloned()
                .map(Filter::Condition)
                .collect(),
        }),
    }
}

pub fn to_query(chain: &RecordsChain) -> RecordsQuery {
    RecordsQuery {
        collection: chain.collection.clone(),
        did: chain.did.clone(),
        filter: filter(chain),
        sort: chain.sort.clone(),
        limit: chain.limit,
        cursor: chain.cursor.clone(),
    }
}

pub fn to_count(chain: &RecordsChain) -> RecordsCount {
    RecordsCount {
        collection: chain.collection.clone(),
        did: chain.did.clone(),
        filter: filter(chain),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::{MethodCall, ObjectCall, Step};
    use serde_json::json;

    fn doc(steps: Vec<Step>, call: &str) -> ObjectCall {
        ObjectCall {
            args: vec![json!("app.bsky.feed.post")],
            steps,
            call: MethodCall {
                name: call.into(),
                args: vec![],
            },
        }
    }

    fn step(name: &str, args: Vec<serde_json::Value>) -> Step {
        Step {
            name: name.into(),
            args,
        }
    }

    #[test]
    fn folds_every_step_into_a_query() {
        let call = doc(
            vec![
                step(
                    "where",
                    vec![json!("author"), json!("="), json!("did:plc:a")],
                ),
                step("where", vec![json!("likes"), json!(">"), json!(10)]),
                step("sort", vec![json!("createdAt"), json!("desc")]),
                step("limit", vec![json!(5)]),
                step("cursor", vec![json!("abc")]),
                step("did", vec![json!("did:plc:a")]),
            ],
            "run",
        );
        let chain = fold(&call).unwrap();
        let q = to_query(&chain);
        assert_eq!(q.collection, "app.bsky.feed.post");
        assert_eq!(q.did.as_deref(), Some("did:plc:a"));
        assert_eq!(q.limit, Some(5));
        assert_eq!(q.cursor.as_deref(), Some("abc"));
        assert_eq!(q.sort.as_ref().unwrap().field, "createdAt");
        match q.filter.unwrap() {
            Filter::Group {
                combine,
                conditions,
            } => {
                assert_eq!(combine, "and");
                assert_eq!(conditions.len(), 2);
                match &conditions[1] {
                    Filter::Condition(c) => assert_eq!(c.value, json!(10)),
                    _ => panic!("condition"),
                }
            }
            _ => panic!("group"),
        }
    }

    #[test]
    fn no_where_means_no_filter_and_sort_defaults_desc() {
        let chain = fold(&doc(vec![step("sort", vec![json!("name")])], "run")).unwrap();
        let q = to_query(&chain);
        assert!(q.filter.is_none());
        assert_eq!(q.sort.unwrap().direction, "desc");
    }

    #[test]
    fn booleans_and_numbers_pass_through_as_their_own_json_type() {
        let chain = fold(&doc(
            vec![step(
                "where",
                vec![json!("active"), json!("="), json!(true)],
            )],
            "run",
        ))
        .unwrap();
        assert_eq!(chain.conditions[0].value, json!(true));
    }

    #[test]
    fn bad_steps_are_bad_chain() {
        for (steps, needle) in [
            (vec![step("where", vec![json!("a")])], "where"),
            (
                vec![step("where", vec![json!("a"), json!("~"), json!(1)])],
                "~",
            ),
            (vec![step("sort", vec![json!("a"), json!("up")])], "up"),
            (vec![step("limit", vec![json!("ten")])], "limit"),
            (vec![step("nope", vec![])], "nope"),
        ] {
            let err = fold(&doc(steps, "run")).unwrap_err();
            assert_eq!(err.code, "BAD_CHAIN");
            assert!(err.message.contains(needle), "{}", err.message);
        }
    }

    #[test]
    fn nil_limit_is_dropped_from_the_chain() {
        for args in [vec![], vec![json!(null)]] {
            let chain = fold(&doc(vec![step("limit", args)], "run")).unwrap();
            assert_eq!(chain.limit, None);
        }
    }

    #[test]
    fn present_limit_still_validates() {
        let err = fold(&doc(vec![step("limit", vec![json!("ten")])], "run")).unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("limit"));
    }

    #[test]
    fn two_nil_steps_in_a_row_are_both_dropped() {
        let chain = fold(&doc(
            vec![step("where", vec![json!(null)]), step("limit", vec![])],
            "run",
        ))
        .unwrap();
        assert!(chain.conditions.is_empty());
        assert_eq!(chain.limit, None);
    }

    #[test]
    fn nil_sort_cursor_and_did_are_dropped() {
        let chain = fold(&doc(
            vec![
                step("sort", vec![json!(null)]),
                step("cursor", vec![]),
                step("did", vec![json!(null)]),
            ],
            "run",
        ))
        .unwrap();
        assert!(chain.sort.is_none());
        assert!(chain.cursor.is_none());
        assert!(chain.did.is_none());
    }

    #[test]
    fn missing_collection_is_bad_input() {
        let call = ObjectCall {
            args: vec![],
            steps: vec![],
            call: MethodCall {
                name: "run".into(),
                args: vec![],
            },
        };
        assert_eq!(fold(&call).unwrap_err().code, "BAD_INPUT");
    }
}
