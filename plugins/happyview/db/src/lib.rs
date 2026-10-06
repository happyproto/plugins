//! `happyview.db`: the records builder. A chain is folded into a query spec
//! and sent to one host import; the host generates the SQL.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod chain;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    library_plugin, ApiExport, ApiSurface, CallContext, ObjectCall, PluginError, PluginInfo,
    RecordsSearch, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-db", "Records", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.db")
        .describe("Query indexed records")
        .export(
            ApiExport::constructor("records")
                .describe("Start a query over one collection")
                .param("collection", "string", "Lexicon collection NSID")
                .lazy("where")
                .lazy("sort")
                .lazy("limit")
                .lazy("cursor")
                .lazy("did")
                .immediate("run")
                .immediate("count")
                .immediate("first"),
        )
        .export(
            ApiExport::function("get")
                .describe("One record by AT URI, or null")
                .param("uri", "string", "AT URI"),
        )
        .export(
            ApiExport::function("search")
                .describe("Substring search on one field, ranked by match position")
                .param("collection", "string", "Lexicon collection NSID")
                .param("field", "string", "Record field path")
                .param("query", "string", "Text to find")
                .param("limit", "integer?", "Max results, default 10, max 100"),
        )
        .export(ApiExport::function("backend").describe("\"sqlite\" or \"postgres\""))
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
