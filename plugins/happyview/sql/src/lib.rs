//! `happyview.sql`: raw SQL plus a chainable table builder over the
//! operator's own tables. A chain is folded into a query spec and sent to
//! one host import; the host generates the SQL.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod chain;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    library_plugin, ApiExport, ApiSurface, CallContext, ObjectCall, PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-sql", "SQL", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.sql")
        .describe("Raw SQL and a table builder over the operator's own tables")
        .export(
            ApiExport::constructor("from")
                .describe("Start a query over one table")
                .param("table", "string", "Table name")
                .lazy("where")
                .lazy("sort")
                .lazy("limit")
                .immediate("run")
                .immediate("count"),
        )
        .export(
            ApiExport::function("raw")
                .describe("Run a statement with backend-native placeholders")
                .param(
                    "sql",
                    "string",
                    "Statement with backend-native placeholders",
                )
                .param("params", "array?", "Bind values"),
        )
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "from" => from(&ObjectCall::from_args(args)?),
        "raw" => raw(args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn from(call: &ObjectCall) -> Result<Value, PluginError> {
    let chain = chain::fold(call)?;
    match call.call.name.as_str() {
        "run" => host::table_query(&chain::to_table_query(&chain, false)),
        "count" => host::table_query(&chain::to_table_query(&chain, true)),
        other => Err(PluginError::new(
            "BAD_CHAIN",
            alloc::format!("{other}: unknown method"),
        )),
    }
}

fn raw(args: &[Value]) -> Result<Value, PluginError> {
    let sql = args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::bad_input("sql is required"))?;
    let params = match args.get(1) {
        None | Some(Value::Null) => alloc::vec::Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        _ => return Err(PluginError::bad_input("params must be an array")),
    };
    let rows = host::db_query(sql, &params)?;
    Ok(Value::Array(rows.into_iter().map(Value::Object).collect()))
}
