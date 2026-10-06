//! `happyview-lua`: Lua 5.4 as a HappyView interpreter plugin. PUC Lua 5.4.8
//! through `mlua`, compiled to `wasm32-wasip1`, behind the SDK's `execute`
//! and `validate` exports.

mod budget;
mod builtins;
mod conformance;
mod convert;
mod ctx;
mod require;
mod sandbox;

use happyview_plugin_sdk::{
    interpreter_plugin, ExecuteInput, ExecuteLimits, ExecuteOutput, PluginError, PluginInfo,
    ScriptErrorKind, ScriptValueKind, ValidateError, ValidateInput, ValidateOutput,
};

const MISSING_HANDLE: &str = "script must define a handle() function";

/// Validation carries no operator limits, so it works from a fixed pair:
/// large enough for any file-scope work a script does while loading, small
/// enough that a loop at that scope ends rather than hanging the editor.
const VALIDATE_LIMITS: ExecuteLimits = ExecuteLimits {
    instructions: Some(1_000_000),
    memory_bytes: 64 * 1024 * 1024,
};

interpreter_plugin! {
    info: PluginInfo::new("happyview-lua", "Lua", "0.1.0"),
    execute: execute_script,
    validate: validate_source,
}

fn execute_script(input: &ExecuteInput) -> Result<ExecuteOutput, PluginError> {
    let lua = sandbox::create(&input.removed_globals).map_err(interpreter_error)?;
    require::install(&lua, &input.libraries).map_err(interpreter_error)?;
    let budget = budget::install(&lua, &input.limits).map_err(interpreter_error)?;

    if let Err(e) = lua
        .load(input.source.as_str())
        .set_name(sandbox::CHUNK_NAME)
        .exec()
    {
        return Ok(script_failure(load_failure_kind(&e), &e, &budget));
    }
    let Ok(handle) = lua.globals().get::<mlua::Function>("handle") else {
        return Ok(failure(ScriptErrorKind::MissingHandle, MISSING_HANDLE));
    };

    let arguments = convert::to_lua(&lua, &input.input)
        .and_then(|argument| Ok((argument, ctx::build(&lua, &input.context)?)));
    let arguments = match arguments {
        Ok(arguments) => arguments,
        // A ceiling small enough to refuse the arguments is the script's
        // limit being reached, not the interpreter failing.
        Err(e) => return Ok(script_failure(ScriptErrorKind::Runtime, &e, &budget)),
    };

    let returned: mlua::Value = match handle.call(arguments) {
        Ok(value) => value,
        Err(e) => return Ok(script_failure(ScriptErrorKind::Runtime, &e, &budget)),
    };

    // A script that reached a normal return with its budget spent has escaped
    // the catch guards, and the reference turns that into the limit error
    // regardless of what it returned. Defence in depth: `lib.rs`'s
    // catch-and-return case is pinned, so nothing known reaches this.
    if budget.is_spent() {
        return Ok(failure(ScriptErrorKind::Timeout, budget::SPENT));
    }

    let value_kind = value_kind(&returned);
    match convert::to_json(&lua, returned) {
        Ok(value) => Ok(ExecuteOutput::Returned { value, value_kind }),
        Err(e) => Ok(script_failure(ScriptErrorKind::Runtime, &e, &budget)),
    }
}

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    // Loading the chunk under the same guard a run uses is the point: without
    // it the editor would accept a top-level read of a name every run refuses.
    let lua = sandbox::create(&input.removed_globals).map_err(interpreter_error)?;
    require::install_stub(&lua).map_err(interpreter_error)?;
    let budget = budget::install(&lua, &VALIDATE_LIMITS).map_err(interpreter_error)?;
    if let Err(e) = lua
        .load(input.source.as_str())
        .set_name(sandbox::CHUNK_NAME)
        .exec()
    {
        let kind = if budget.is_spent() {
            ScriptErrorKind::Timeout
        } else {
            load_failure_kind(&e)
        };
        let raw = e.to_string();
        let (line, message) = split_position(&raw);
        return Ok(ValidateOutput {
            valid: false,
            errors: vec![ValidateError {
                kind,
                line,
                message,
            }],
        });
    }
    match lua.globals().get::<mlua::Function>("handle") {
        Ok(_) => Ok(ValidateOutput {
            valid: true,
            errors: Vec::new(),
        }),
        Err(_) => Ok(invalid(ScriptErrorKind::MissingHandle, MISSING_HANDLE)),
    }
}

/// The host branches three ways on what a script returned, and a JSON `null`
/// cannot tell a returned nothing from a returned null.
fn value_kind(value: &mlua::Value) -> ScriptValueKind {
    match value {
        mlua::Value::Nil => ScriptValueKind::None,
        mlua::Value::Table(_) => ScriptValueKind::Object,
        _ => ScriptValueKind::Other,
    }
}

fn failure(kind: ScriptErrorKind, message: impl Into<String>) -> ExecuteOutput {
    let message = message.into();
    ExecuteOutput::Error {
        kind,
        raw: message.clone(),
        message,
        line: None,
    }
}

/// Why loading a chunk failed. A chunk that will not parse is `syntax`;
/// anything else raised while it loads is a `runtime` failure, and the one
/// that matters is a removed global read at file scope — calling that a
/// syntax error sends an author looking for a missing `end`, and leaves a
/// client unable to tell a parse failure from an unmigrated script.
fn load_failure_kind(error: &mlua::Error) -> ScriptErrorKind {
    match error {
        mlua::Error::SyntaxError { .. } => ScriptErrorKind::Syntax,
        _ => ScriptErrorKind::Runtime,
    }
}

/// A failure the script earned. `kind` is read from the run rather than from
/// the text: a spent budget may finally surface as whatever error the script
/// was in the middle of, and an allocator refusal is a variant rather than a
/// sentence.
fn script_failure(
    default: ScriptErrorKind,
    error: &mlua::Error,
    budget: &budget::Budget,
) -> ExecuteOutput {
    let raw = error.to_string();
    let (line, message) = split_position(&raw);
    let kind = if budget.is_spent() {
        ScriptErrorKind::Timeout
    } else if budget::is_memory_error(error) {
        ScriptErrorKind::Memory
    } else {
        default
    };
    ExecuteOutput::Error {
        kind,
        message,
        line,
        raw,
    }
}

/// `[string "script"]:12: message` into its two halves, so the host is handed
/// a line rather than left to parse one out.
///
/// The traceback goes first: it names only chunk lines and library frames,
/// which is noise to a caller, and a position inside it would otherwise be
/// read as the error's own. An error with no position keeps its whole text,
/// which is why `error("x", 0)` reads as it was written.
fn split_position(raw: &str) -> (Option<u32>, String) {
    let raw = raw.split("\nstack traceback:").next().unwrap_or(raw);
    if let Some(bracket) = raw.find("]:") {
        let after = &raw[bracket + 2..];
        if let Some(colon) = after.find(": ") {
            if let Ok(line) = after[..colon].parse::<u32>() {
                return (Some(line), after[colon + 2..].to_string());
            }
        }
    }
    (None, raw.to_string())
}

fn invalid(kind: ScriptErrorKind, message: impl Into<String>) -> ValidateOutput {
    ValidateOutput {
        valid: false,
        errors: vec![ValidateError {
            kind,
            line: None,
            message: message.into(),
        }],
    }
}

/// A VM that cannot be built or a value that cannot cross the boundary is the
/// interpreter failing, not the script, so it leaves as an envelope error
/// rather than as a script result.
fn interpreter_error(e: mlua::Error) -> PluginError {
    PluginError::new("INTERPRETER_ERROR", e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn execute_input(source: &str) -> ExecuteInput {
        input_with(source, &[])
    }

    fn input_with(source: &str, removed_globals: &[&str]) -> ExecuteInput {
        serde_json::from_value(json!({
            "source": source,
            "kind": "xrpc_query",
            "input": {},
            "context": {"trigger": "xrpc.query:app.test.q"},
            "limits": {"instructions": 1_000_000, "memory_bytes": 67_108_864},
            "removed_globals": removed_globals,
        }))
        .expect("the execute input should deserialize")
    }

    fn validate_input(source: &str, removed_globals: &[&str]) -> ValidateInput {
        serde_json::from_value(json!({
            "source": source,
            "removed_globals": removed_globals,
        }))
        .expect("the validate input should deserialize")
    }

    fn message_of(output: ExecuteOutput) -> String {
        match output {
            ExecuteOutput::Error { message, .. } => message,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    fn input_with_limits(
        source: &str,
        instructions: serde_json::Value,
        memory: u64,
    ) -> ExecuteInput {
        serde_json::from_value(json!({
            "source": source,
            "kind": "xrpc_query",
            "input": {},
            "context": {"trigger": "xrpc.query:app.test.q"},
            "limits": {"instructions": instructions, "memory_bytes": memory},
        }))
        .expect("the execute input should deserialize")
    }

    /// Runs on a thread of its own, so a budget that fails to end a run fails
    /// the test rather than hanging the process.
    fn run_bounded(input: ExecuteInput) -> ExecuteOutput {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(execute_script(&input));
        });
        rx.recv_timeout(std::time::Duration::from_secs(20))
            .expect("the run must end on its own")
            .expect("execute should answer an output rather than an envelope error")
    }

    fn failure_of(output: ExecuteOutput) -> (ScriptErrorKind, String, Option<u32>, String) {
        match output {
            ExecuteOutput::Error {
                kind,
                message,
                line,
                raw,
            } => (kind, message, line, raw),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// The host branches three ways on this, and a JSON `null` cannot tell a
    /// returned nothing from a returned null.
    #[test]
    fn what_a_script_returned_carries_its_kind() {
        for (source, value, kind) in [
            ("function handle() end", json!(null), ScriptValueKind::None),
            (
                "function handle() return { a = 1 } end",
                json!({ "a": 1 }),
                ScriptValueKind::Object,
            ),
            (
                "function handle() return 7 end",
                json!(7),
                ScriptValueKind::Other,
            ),
            (
                r#"function handle() return "x" end"#,
                json!("x"),
                ScriptValueKind::Other,
            ),
            // The null sentinel a decode hands back is a value, not an
            // absence, so it is `other` rather than `none`.
            (
                r#"local json = require("internal.json")
                   function handle() return json.decode("null") end"#,
                json!(null),
                ScriptValueKind::Other,
            ),
        ] {
            let output = execute_script(&execute_input(source)).unwrap();
            assert_eq!(
                output,
                ExecuteOutput::Returned {
                    value,
                    value_kind: kind
                },
                "{source}"
            );
        }
    }

    /// `ctx` has to reach `handle`, which only the call site can show.
    #[test]
    fn handle_receives_the_input_and_the_context() {
        let input: ExecuteInput = serde_json::from_value(json!({
            "source": "function handle(input, ctx) \
                       return { q = input.q, trigger = ctx.trigger, who = ctx.caller_did } end",
            "kind": "xrpc_query",
            "input": { "q": "x" },
            "context": { "trigger": "xrpc.query:app.test.q", "caller_did": "did:plc:me" },
            "limits": { "instructions": 1_000_000, "memory_bytes": 67_108_864 },
        }))
        .unwrap();
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!({
                    "q": "x",
                    "trigger": "xrpc.query:app.test.q",
                    "who": "did:plc:me",
                }),
                value_kind: ScriptValueKind::Object,
            }
        );
    }

    #[test]
    fn an_instruction_budget_ends_a_run_through_every_layer_of_catching() {
        for source in [
            "function handle() while true do end end",
            "function handle() while true do pcall(function() while true do end end) end end",
            "function handle() pcall(function() while true do end end) return { ok = true } end",
            "function handle() while true do \
                 coroutine.resume(coroutine.create(function() while true do end end)) end end",
            "function handle() while true do \
                 pcall(coroutine.wrap(function() while true do end end)) end end",
            "function handle() while true do pcall(function() \
                 string.gsub('x', 'x', function() while true do end end) end) end end",
        ] {
            let output = run_bounded(input_with_limits(source, json!(10_000), 67_108_864));
            let (kind, message, _, _) = failure_of(output);
            assert_eq!(kind, ScriptErrorKind::Timeout, "{source}");
            assert!(message.contains("execution limit"), "{source}: {message}");
        }
    }

    #[test]
    fn no_instruction_budget_runs_a_long_loop_to_completion() {
        let output = run_bounded(input_with_limits(
            "function handle() local n = 0 for i = 1, 3000000 do n = n + 1 end return n end",
            json!(null),
            67_108_864,
        ));
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!(3_000_000),
                value_kind: ScriptValueKind::Other,
            }
        );
    }

    #[test]
    fn a_run_under_its_budget_keeps_ordinary_pcall_and_xpcall() {
        let output = run_bounded(input_with_limits(
            r#"function handle()
                 local ok, err = pcall(error, "boom")
                 local ok2, seen = xpcall(function() error("bang") end,
                                          function(m) return "handled" end)
                 local fine, value = pcall(function() return 7 end)
                 return { ok = ok, ok2 = ok2, seen = seen, fine = fine, value = value,
                          ended = tostring(err):sub(-4) }
               end"#,
            json!(1_000_000),
            67_108_864,
        ));
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!({
                    "ok": false, "ok2": false, "seen": "handled",
                    "fine": true, "value": 7, "ended": "boom",
                }),
                value_kind: ScriptValueKind::Object,
            }
        );
    }

    #[test]
    fn a_memory_ceiling_is_a_memory_failure_rather_than_a_trap() {
        let output = run_bounded(input_with_limits(
            "function handle() local t = {} local i = 1 \
             while true do t[i] = { i, i, i, i } i = i + 1 end end",
            json!(null),
            8 * 1024 * 1024,
        ));
        let (kind, message, _, _) = failure_of(output);
        assert_eq!(kind, ScriptErrorKind::Memory);
        assert!(message.contains("not enough memory"), "{message}");
    }

    #[test]
    fn every_error_kind_is_produced_by_a_script_that_earns_it() {
        let cases: [(&str, serde_json::Value, ScriptErrorKind); 5] = [
            (
                "function handle(",
                json!(1_000_000),
                ScriptErrorKind::Syntax,
            ),
            (
                "local x = 1",
                json!(1_000_000),
                ScriptErrorKind::MissingHandle,
            ),
            (
                r#"function handle() error("boom") end"#,
                json!(1_000_000),
                ScriptErrorKind::Runtime,
            ),
            (
                "function handle() while true do end end",
                json!(1_000),
                ScriptErrorKind::Timeout,
            ),
            (
                "function handle() local t = {} local i = 1 \
                 while true do t[i] = { i, i } i = i + 1 end end",
                json!(null),
                ScriptErrorKind::Memory,
            ),
        ];
        for (source, instructions, expected) in cases {
            let output = run_bounded(input_with_limits(source, instructions, 8 * 1024 * 1024));
            let (kind, _, _, _) = failure_of(output);
            assert_eq!(kind, expected, "{source}");
        }
    }

    #[test]
    fn a_runtime_error_carries_its_line_and_a_message_with_no_position() {
        let output = run_bounded(input_with_limits(
            "function handle()\n  local t = nil\n  return t.x\nend",
            json!(1_000_000),
            67_108_864,
        ));
        let (kind, message, line, raw) = failure_of(output);
        assert_eq!(kind, ScriptErrorKind::Runtime);
        assert_eq!(line, Some(3), "{raw}");
        assert_eq!(message, "attempt to index a nil value (local 't')");
        // The traceback the host logs is kept whole.
        assert!(raw.contains("stack traceback:"), "{raw}");
    }

    #[test]
    fn an_error_with_no_position_carries_no_line() {
        for source in [
            r#"function handle() error("x", 0) end"#,
            "function handle() return db.x end",
        ] {
            let mut input = input_with_limits(source, json!(1_000_000), 67_108_864);
            input.removed_globals = vec!["db".to_string()];
            let (_, message, line, _) = failure_of(run_bounded(input));
            assert_eq!(line, None, "{source}: {message}");
        }
    }

    #[test]
    fn an_auth_error_message_survives_untouched() {
        let output = run_bounded(input_with_limits(
            r#"function handle() error("AUTH_ERROR: no session") end"#,
            json!(1_000_000),
            67_108_864,
        ));
        let (kind, message, line, _) = failure_of(output);
        assert_eq!(kind, ScriptErrorKind::Runtime);
        assert_eq!(message, "AUTH_ERROR: no session");
        assert_eq!(line, Some(1));
    }

    #[test]
    fn validate_refuses_what_the_native_check_refuses() {
        for (source, expected) in [
            ("function handle(", ScriptErrorKind::Syntax),
            (
                "function other() return {} end",
                ScriptErrorKind::MissingHandle,
            ),
            (
                "while true do end\nfunction handle() end",
                ScriptErrorKind::Timeout,
            ),
        ] {
            let validated = validate_source(&validate_input(source, &[])).unwrap();
            assert!(!validated.valid, "{source}");
            assert_eq!(validated.errors[0].kind, expected, "{source}");
        }
        assert!(
            validate_source(&validate_input("function handle() return {} end", &[]))
                .unwrap()
                .valid
        );
    }

    #[test]
    fn execute_calls_handle_and_answers_what_it_returned() {
        let output = execute_script(&execute_input("function handle() return {ok = true} end"))
            .expect("execute should answer an output rather than an envelope error");
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!({"ok": true}),
                value_kind: ScriptValueKind::Object,
            }
        );
    }

    /// The list travels in the input and the guard is built from it, so this
    /// is the one place the two meet.
    #[test]
    fn a_removed_global_the_input_named_raises_the_migration_sentence() {
        let output =
            execute_script(&input_with("function handle() return db.x end", &["db"])).unwrap();
        let message = message_of(output);
        assert!(
            message.contains(&sandbox::removed_global_message("db")),
            "{message}"
        );
    }

    /// The editor's check and a run read the same list, so a name a run
    /// refuses cannot be saved as valid.
    #[test]
    fn validate_refuses_a_top_level_read_of_a_removed_global() {
        // A bare read, so the unguarded case is a valid script rather than
        // an index of nil.
        let source = "local base = env\nfunction handle() return base end";
        let validated = validate_source(&validate_input(source, &["env"])).unwrap();
        assert!(!validated.valid);
        assert!(
            validated.errors[0]
                .message
                .contains(&sandbox::removed_global_message("env")),
            "{:?}",
            validated.errors[0]
        );
        // The same source with nothing removed is an ordinary script.
        assert!(validate_source(&validate_input(source, &[])).unwrap().valid);
    }

    #[test]
    fn a_script_reaches_the_os_subset_the_sandbox_keeps() {
        let output = execute_script(&execute_input(
            r#"function handle()
                 return { stamp = os.date("!%Y-%m-%d", 0), clock = os.clock }
               end"#,
        ))
        .unwrap();
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!({"stamp": "1970-01-01"}),
                value_kind: ScriptValueKind::Object,
            }
        );
    }

    /// `require` has to be installed for a run, and a built-in has to resolve
    /// through it without a host.
    #[test]
    fn a_script_reaches_a_builtin_through_require() {
        let output = execute_script(&execute_input(
            r#"local json = require("internal.json")
               function handle() return { encoded = json.encode({ a = 1 }) } end"#,
        ))
        .unwrap();
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!({ "encoded": r#"{"a":1}"# }),
                value_kind: ScriptValueKind::Object,
            }
        );
    }

    /// Validation compiles a top-level chain against a stub, so a script that
    /// builds a query at file scope is not refused for want of an instance.
    #[test]
    fn validate_accepts_a_v3_script_that_requires_a_library() {
        let validated = validate_source(&validate_input(
            r#"local db = require("happyview.db")
               local log = require("internal.logging")
               function handle(input, ctx)
                 log.info("hi", { who = ctx.caller_did })
                 return db.records("app.test.rec"):limit(input.limit):all()
               end"#,
            &[],
        ))
        .unwrap();
        assert!(validated.valid, "{:?}", validated.errors);
    }

    #[test]
    fn a_script_with_no_handle_is_refused_by_both_exports() {
        let output = execute_script(&execute_input("local x = 1")).unwrap();
        assert!(matches!(
            output,
            ExecuteOutput::Error {
                kind: ScriptErrorKind::MissingHandle,
                ..
            }
        ));
        let validated = validate_source(&validate_input("local x = 1", &[])).unwrap();
        assert!(!validated.valid);
        assert_eq!(validated.errors[0].kind, ScriptErrorKind::MissingHandle);
    }
}
