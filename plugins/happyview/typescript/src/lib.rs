//! `happyview-typescript`: TypeScript as a HappyView interpreter plugin.
//! SWC's TypeScript transform in front of the `happyview-quickjs` engine,
//! compiled to `wasm32-wasip1`, behind the SDK's `execute` and `validate`
//! exports.
//!
//! Everything a script sees once it runs is the engine's, so a TypeScript
//! script and a JavaScript one with the same logic behave the same way; this
//! crate only turns the one into the other and keeps every error's position
//! in the source the author wrote.

mod frontend;

use happyview_plugin_sdk::{
    interpreter_plugin, ExecuteInput, ExecuteOutput, PluginError, PluginInfo, ValidateInput,
    ValidateOutput,
};

use frontend::TypeScript;

interpreter_plugin! {
    info: PluginInfo::new("happyview-typescript", "TypeScript", "0.1.0"),
    execute: execute_script,
    validate: validate_source,
}

fn execute_script(input: &ExecuteInput) -> Result<ExecuteOutput, PluginError> {
    happyview_quickjs::execute(&TypeScript, input)
}

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    happyview_quickjs::validate(&TypeScript, input)
}

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod tests;
