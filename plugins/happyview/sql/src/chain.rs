//! Folds the object document a `from(...)` chain produces into the host's
//! table query spec. Nothing here touches SQL: the host owns that.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::{Condition, Filter, ObjectCall, PluginError, Sort, TableQuery, Value};

const OPS: [&str; 9] = ["=", "!=", "<", ">", "<=", ">=", "like", "not like", "ilike"];

#[derive(Debug)]
pub struct TableChain {
    pub table: String,
    pub conditions: Vec<Condition>,
    pub sort: Option<Sort>,
    pub limit: Option<u32>,
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

pub fn fold(call: &ObjectCall) -> Result<TableChain, PluginError> {
    let table = call
        .args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::bad_input("from(table) needs a table name"))?
        .to_string();
    let mut chain = TableChain {
        table,
        conditions: Vec::new(),
        sort: None,
        limit: None,
    };
    for step in &call.steps {
        let name = step.name.as_str();
        let args = &step.args;
        // a step whose first argument is absent or nil reads as "not called", so
        // `:limit(input.limit)` is no limit when the input carries none
        if matches!(name, "where" | "sort" | "limit")
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
            other => return Err(bad_chain(other, "unknown step")),
        }
    }
    Ok(chain)
}

fn filter(chain: &TableChain) -> Option<Filter> {
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

pub fn to_table_query(chain: &TableChain, count: bool) -> TableQuery {
    TableQuery {
        table: chain.table.clone(),
        filter: filter(chain),
        sort: chain.sort.clone(),
        limit: chain.limit,
        count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::{Filter, MethodCall, ObjectCall, Step};
    use serde_json::json;

    fn doc(steps: Vec<Step>, call: &str) -> ObjectCall {
        ObjectCall {
            args: vec![json!("leaderboard")],
            steps,
            call: MethodCall {
                name: call.into(),
                args: vec![],
            },
        }
    }

    #[test]
    fn folds_into_a_table_query() {
        let call = doc(
            vec![
                Step {
                    name: "where".into(),
                    args: vec![json!("score"), json!(">"), json!(100)],
                },
                Step {
                    name: "sort".into(),
                    args: vec![json!("score"), json!("asc")],
                },
                Step {
                    name: "limit".into(),
                    args: vec![json!(3)],
                },
            ],
            "run",
        );
        let q = to_table_query(&fold(&call).unwrap(), false);
        assert_eq!(q.table, "leaderboard");
        assert!(matches!(q.filter, Some(Filter::Condition(_))));
        assert_eq!(q.sort.unwrap().direction, "asc");
        assert_eq!(q.limit, Some(3));
        assert!(!q.count);
    }

    #[test]
    fn count_sets_the_flag() {
        let q = to_table_query(&fold(&doc(vec![], "count")).unwrap(), true);
        assert!(q.count);
    }

    #[test]
    fn nil_limit_is_dropped_from_the_chain() {
        for args in [vec![], vec![json!(null)]] {
            let chain = fold(&doc(
                vec![Step {
                    name: "limit".into(),
                    args,
                }],
                "run",
            ))
            .unwrap();
            assert_eq!(chain.limit, None);
        }
    }

    #[test]
    fn present_limit_still_validates() {
        let err = fold(&doc(
            vec![Step {
                name: "limit".into(),
                args: vec![json!("ten")],
            }],
            "run",
        ))
        .unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("limit"));
    }

    #[test]
    fn two_nil_steps_in_a_row_are_both_dropped() {
        let chain = fold(&doc(
            vec![
                Step {
                    name: "where".into(),
                    args: vec![json!(null)],
                },
                Step {
                    name: "limit".into(),
                    args: vec![],
                },
            ],
            "run",
        ))
        .unwrap();
        assert!(chain.conditions.is_empty());
        assert_eq!(chain.limit, None);
    }

    #[test]
    fn nil_sort_is_dropped() {
        let chain = fold(&doc(
            vec![Step {
                name: "sort".into(),
                args: vec![json!(null)],
            }],
            "run",
        ))
        .unwrap();
        assert!(chain.sort.is_none());
    }

    #[test]
    fn unknown_step_is_bad_chain() {
        let err = fold(&doc(
            vec![Step {
                name: "cursor".into(),
                args: vec![json!("x")],
            }],
            "run",
        ))
        .unwrap_err();
        assert_eq!(err.code, "BAD_CHAIN");
        assert!(err.message.contains("cursor"));
    }
}
