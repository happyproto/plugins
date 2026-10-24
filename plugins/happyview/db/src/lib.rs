//! `happyview.db`: the records builder. A chain is folded into a query spec
//! and sent to one host import; the host generates the SQL.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod chain;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiMethod, ApiSurface, CallContext, ObjectCall, PluginError,
    PluginInfo, RecordsSearch, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-db", "Records", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.db")
        .describe("Query indexed records")
        .named_type(
            "RecordEnvelope",
            "An indexed record beside where it came from",
            json!({"type": "object", "properties": [
                {"name": "uri", "type": "string", "description": "The record's AT URI"},
                {"name": "did", "type": "string", "description": "The repo it is in"},
                {"name": "collection", "type": "string", "description": "Its collection NSID"},
                {"name": "rkey", "type": "string", "description": "Its record key"},
                {"name": "cid", "type": "string?", "description": "Its CID; null for a record written locally and not yet seen on the network"},
                {"name": "indexed_at", "type": "string?", "description": "When the network's copy was indexed; null until it has been"},
                {"name": "record", "type": "object", "description": "The record body"}
            ]}),
        )
        .named_type(
            "RecordsPage",
            "One page of a query",
            json!({"type": "object", "properties": [
                {"name": "records", "type": "RecordEnvelope[]"},
                {"name": "cursor", "type": "string?", "description": "Pass to cursor() for the next page; absent on the last"}
            ]}),
        )
        .named_type(
            "Operator",
            "A comparison where() accepts",
            json!("'=' | '!=' | '<' | '>' | '<=' | '>=' | 'like' | 'not like' | 'ilike'"),
        )
        .export(
            ApiExport::constructor("records")
                .describe("Start a query over one collection")
                .param("collection", "string", "Lexicon collection NSID")
                .method(
                    ApiMethod::lazy("where")
                        .describe("Keep records whose field compares true; every where() must hold")
                        .param("field", "string?", "Record field path; nil skips the step")
                        .param("op", "Operator", "How to compare")
                        .param("value", "string | number | boolean", "What to compare with"),
                )
                .method(
                    ApiMethod::lazy("sort")
                        .describe("Order by a field")
                        .param("field", "string?", "Record field path; nil skips the step")
                        .param("direction", "('asc' | 'desc')?", "Default desc"),
                )
                .method(
                    ApiMethod::lazy("limit")
                        .describe("At most this many records")
                        .param("n", "integer?", "nil skips the step"),
                )
                .method(
                    ApiMethod::lazy("cursor")
                        .describe("Continue from a page's cursor")
                        .param("cursor", "string?", "nil skips the step"),
                )
                .method(
                    ApiMethod::lazy("did")
                        .describe("Only records in one repo")
                        .param("did", "string?", "nil skips the step"),
                )
                .method(
                    ApiMethod::immediate("run")
                        .describe("Run the query")
                        .returns(json!("RecordsPage")),
                )
                .method(
                    ApiMethod::immediate("count")
                        .describe("Count what the query matches")
                        .returns(json!("integer")),
                )
                .method(
                    ApiMethod::immediate("first")
                        .describe("The first record the query matches, or null")
                        .returns(json!("RecordEnvelope?")),
                ),
        )
        .export(
            ApiExport::function("get")
                .describe("One record by AT URI, or null")
                .param("uri", "string", "AT URI")
                .returns(json!("RecordEnvelope?")),
        )
        .export(
            ApiExport::function("search")
                .describe("Substring search on one field, ranked by match position")
                .param("collection", "string", "Lexicon collection NSID")
                .param("field", "string", "Record field path")
                .param("query", "string", "Text to find")
                .param("limit", "integer?", "Max results, default 10, max 100")
                .returns(json!("RecordEnvelope[]")),
        )
        .export(
            ApiExport::function("backend")
                .describe("Which database the instance runs on")
                .returns(json!("('sqlite' | 'postgres')?")),
        )
}

fn dispatch(function: &str, args: &[Value], ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "records" => records(&ObjectCall::from_args(args)?),
        "get" => {
            let uri = args
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| PluginError::bad_input("uri is required"))?;
            Ok(host::records_get(uri)?.unwrap_or(Value::Null))
        }
        "search" => {
            let spec = RecordsSearch {
                collection: str_arg(args, 0, "collection")?,
                field: str_arg(args, 1, "field")?,
                query: str_arg(args, 2, "query")?,
                limit: args.get(3).and_then(Value::as_u64).map(|n| n as u32),
            };
            Ok(Value::Array(host::records_search(&spec)?))
        }
        "backend" => Ok(ctx
            .db_backend
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null)),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn str_arg(args: &[Value], i: usize, name: &str) -> Result<alloc::string::String, PluginError> {
    args.get(i)
        .and_then(Value::as_str)
        .map(alloc::string::ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(alloc::format!("{name} is required")))
}

fn records(call: &ObjectCall) -> Result<Value, PluginError> {
    let chain = chain::fold(call)?;
    match call.call.name.as_str() {
        "run" => serde_json::to_value(host::records_query(&chain::to_query(&chain))?)
            .map_err(PluginError::from),
        "count" => Ok(Value::from(host::records_count(&chain::to_count(&chain))?)),
        "first" => {
            let mut query = chain::to_query(&chain);
            query.limit = Some(1);
            let page = host::records_query(&query)?;
            Ok(page.records.into_iter().next().unwrap_or(Value::Null))
        }
        other => Err(PluginError::new(
            "BAD_CHAIN",
            alloc::format!("{other}: unknown method"),
        )),
    }
}

#[cfg(test)]
mod surface_tests {
    #[test]
    fn the_surface_parses() {
        happyview_plugin_sdk::declarations::assert_surface_parses(&super::surface());
    }
}
