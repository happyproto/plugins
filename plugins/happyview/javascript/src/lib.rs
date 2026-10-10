//! `happyview-javascript`: JavaScript as a HappyView interpreter plugin.
//! QuickJS-ng through `happyview-quickjs`, compiled to `wasm32-wasip1`, behind
//! the SDK's `execute` and `validate` exports.
//!
//! JavaScript is what the engine already runs, so its front end prepares
//! nothing: the script an author saved is the module QuickJS compiles.

use happyview_plugin_sdk::{
    interpreter_plugin, ExecuteInput, ExecuteOutput, PluginError, PluginInfo, ValidateInput,
    ValidateOutput,
};
use happyview_quickjs::JavaScript;

interpreter_plugin! {
    info: PluginInfo::new("happyview-javascript", "JavaScript", "0.1.0"),
    execute: execute_script,
    validate: validate_source,
}

fn execute_script(input: &ExecuteInput) -> Result<ExecuteOutput, PluginError> {
    happyview_quickjs::execute(&JavaScript, input)
}

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    happyview_quickjs::validate(&JavaScript, input)
}

#[cfg(test)]
mod conformance;
