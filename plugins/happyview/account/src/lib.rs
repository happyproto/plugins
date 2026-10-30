//! `happyview.account`: the calling script's user's email address, as their
//! PDS reports it. A thin translator over one SDK host wrapper. It is its own
//! library, rather than a function on `happyview.xrpc`, so that only an
//! instance that installs it consents to `caller:email`.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, PluginError, PluginInfo, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-account", "Account", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.account")
        .describe("The calling user's email address, as their PDS reports it")
        .export(
            ApiExport::function("email")
                .describe("The calling user's email address and whether it is confirmed")
                .returns(json!({"type": "object", "properties": [
                    {"name": "email", "type": "string?", "description": "Absent unless the user's sign-in granted it, under transition:email or account:email"},
                    {"name": "email_confirmed", "type": "boolean?"}
                ]})),
        )
}

fn dispatch(function: &str, _args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "email" => serde_json::to_value(host::caller_get_email()?).map_err(PluginError::from),
        other => Err(PluginError::unknown_function(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_function_is_refused() {
        let err = dispatch("session", &[], &CallContext::default()).unwrap_err();
        assert_eq!(err.code, "UNKNOWN_FUNCTION");
    }

    #[test]
    fn the_surface_parses() {
        happyview_plugin_sdk::declarations::assert_surface_parses(&surface());
    }
}
