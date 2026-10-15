//! `happyview-lua`: Lua 5.4 as a HappyView interpreter plugin. PUC Lua 5.4.8
//! through `mlua`, compiled to `wasm32-wasip1`, behind the SDK's `execute`
//! and `validate` exports.

mod artifact;
mod budget;
mod builtins;
mod chunk;
mod concurrent;
mod conformance;
mod convert;
mod ctx;
mod imports;
mod modules;
mod require;
mod sandbox;
mod test;

use happyview_plugin_sdk::{
    interpreter_plugin, ChunkRole, ExecuteInput, ExecuteLimits, ExecuteOutput, PluginError,
    PluginInfo, ScriptErrorKind, ScriptKind, ScriptValueKind, ValidateError, ValidateInput,
    ValidateOutput,
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
    execute_with(input, modules::host())
}

/// [`execute_script`] with relative `require`s resolved through `resolver`,
/// which a test chooses.
pub(crate) fn execute_with(
    input: &ExecuteInput,
    resolver: Box<dyn modules::Resolver>,
) -> Result<ExecuteOutput, PluginError> {
    execute_in(input, resolver, std::rc::Rc::new(test::Host))
}

/// [`execute_with`] with the host a test file reports to chosen as well, so a
/// native test can record what a test file reported.
pub(crate) fn execute_in(
    input: &ExecuteInput,
    resolver: Box<dyn modules::Resolver>,
    tests: std::rc::Rc<dyn test::TestHost>,
) -> Result<ExecuteOutput, PluginError> {
    let lua = sandbox::create(&input.removed_globals).map_err(interpreter_error)?;
    // The budget goes first: `internal.async` resumes through the catch guard
    // it installs, and takes it when `require` is installed.
    let budget = budget::install(&lua, &input.limits).map_err(interpreter_error)?;
    require::install(&lua, &input.libraries).map_err(interpreter_error)?;
    modules::install(&lua, &input.chunk_name, resolver).map_err(interpreter_error)?;
    let failed =
        |default, e: &mlua::Error| script_failure(default, e, &budget, &modules::known(&lua));

    // A test file's globals exist before its chunk runs, since `describe` is
    // called at file scope.
    let suite = match input.kind {
        ScriptKind::Test => {
            Some(test::install(&lua, tests, &input.chunk_name).map_err(interpreter_error)?)
        }
        _ => None,
    };

    let main = chunk::load(
        &lua,
        &input.chunk_name,
        &input.source,
        input.artifact.as_ref(),
    )
    .map_err(|why| PluginError::new("INTERPRETER_ERROR", why))?;
    if let Err(e) = main.and_then(|main| main.call::<()>(())) {
        return Ok(failed(load_failure_kind(&e), &e));
    }

    // A test file has no `handle`: running its collected tests is the run.
    if let Some(suite) = suite {
        return Ok(match suite.run(&lua) {
            Ok(counts) => ExecuteOutput::Returned {
                value: counts,
                value_kind: ScriptValueKind::Object,
            },
            Err(e) => failed(ScriptErrorKind::Runtime, &e),
        });
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
        Err(e) => return Ok(failed(ScriptErrorKind::Runtime, &e)),
    };

    let returned: mlua::Value = match handle.call(arguments) {
        Ok(value) => value,
        Err(e) => return Ok(failed(ScriptErrorKind::Runtime, &e)),
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
        Err(e) => Ok(failed(ScriptErrorKind::Runtime, &e)),
    }
}

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    // Loading the chunk under the same guard a run uses is the point: without
    // it the editor would accept a top-level read of a name every run refuses.
    let lua = sandbox::create(&input.removed_globals).map_err(interpreter_error)?;
    require::install_stub(&lua).map_err(interpreter_error)?;
    let budget = budget::install(&lua, &VALIDATE_LIMITS).map_err(interpreter_error)?;
    let name = input.chunk_name.as_deref().unwrap_or(chunk::DEFAULT);
    if input.role == ChunkRole::Test {
        test::install(&lua, std::rc::Rc::new(test::Validating), name).map_err(interpreter_error)?;
    }
    let refused = |e: mlua::Error| {
        let kind = if budget.is_spent() {
            ScriptErrorKind::Timeout
        } else {
            load_failure_kind(&e)
        };
        let located = chunk::locate(&e.to_string(), &[name.to_string()]);
        ValidateOutput::invalid(vec![ValidateError {
            kind,
            line: located.line,
            message: located.message,
        }])
    };

    let main = match chunk::load_source(&lua, name, &input.source) {
        Ok(main) => main,
        Err(e) => return Ok(refused(e)),
    };
    // Dumped before file scope runs, so the artifact is the compiled chunk
    // and holds nothing file scope did with the stub `require`.
    let artifact = input.prepare.then(|| artifact::prepare(&main));
    if let Err(e) = main.call::<()>(()) {
        return Ok(refused(e));
    }
    // A module is required and a test is run, neither called, so only a
    // handler has a `handle` to define.
    if input.role == ChunkRole::Handler && lua.globals().get::<mlua::Function>("handle").is_err() {
        return Ok(invalid(ScriptErrorKind::MissingHandle, MISSING_HANDLE));
    }
    Ok(ValidateOutput {
        valid: true,
        artifact,
        imports: imports::relative_requires(&input.source),
        ..ValidateOutput::default()
    })
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
        chunk: None,
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
    known: &[String],
) -> ExecuteOutput {
    let raw = error.to_string();
    let located = chunk::locate(&raw, known);
    let kind = if budget.is_spent() {
        ScriptErrorKind::Timeout
    } else if budget::is_memory_error(error) {
        ScriptErrorKind::Memory
    } else {
        default
    };
    ExecuteOutput::Error {
        kind,
        message: located.message,
        line: located.line,
        raw,
        chunk: located.chunk,
    }
}

fn invalid(kind: ScriptErrorKind, message: impl Into<String>) -> ValidateOutput {
    ValidateOutput {
        valid: false,
        errors: vec![ValidateError {
            kind,
            line: None,
            message: message.into(),
        }],
        ..ValidateOutput::default()
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

    #[test]
    fn validate_reports_the_literal_relative_requires() {
        let validated = validate_source(&validate_input(
            "local format = require('./lib/format')\nlocal json = require('internal.json')\nfunction handle() end",
            &[],
        ))
        .unwrap();
        assert!(validated.valid);
        assert_eq!(
            validated.imports,
            vec![happyview_plugin_sdk::ImportRef {
                specifier: "./lib/format".into(),
                line: 1
            }]
        );
    }

    fn prepared(source: &str, chunk_name: &str) -> happyview_plugin_sdk::PreparedArtifact {
        let mut input = validate_input(source, &[]);
        input.chunk_name = Some(chunk_name.to_string());
        input.prepare = true;
        let validated = validate_source(&input).unwrap();
        assert!(validated.valid, "{:?}", validated.errors);
        validated
            .artifact
            .expect("a prepared, valid script carries its artifact")
    }

    #[test]
    fn validate_prepares_only_when_asked_and_only_what_is_valid() {
        assert!(
            validate_source(&validate_input("function handle() end", &[]))
                .unwrap()
                .artifact
                .is_none()
        );
        let mut refused = validate_input("function handle(", &[]);
        refused.prepare = true;
        let refused = validate_source(&refused).unwrap();
        assert!(!refused.valid);
        assert!(refused.artifact.is_none());
        assert_eq!(
            prepared("function handle() end", "script").engine,
            artifact::ENGINE
        );
    }

    #[test]
    fn a_run_from_the_artifact_needs_no_source_and_keeps_its_lines() {
        let artifact = prepared(
            "function handle(input)\n  if input.fail then\n    error('boom')\n  end\n  return { ok = true }\nend",
            "src/jobs/x.lua",
        );
        let mut input = execute_input("");
        input.chunk_name = "src/jobs/x.lua".to_string();
        input.artifact = Some(artifact.clone());
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!({ "ok": true }),
                value_kind: ScriptValueKind::Object,
            }
        );

        input.input = json!({ "fail": true });
        match execute_script(&input).unwrap() {
            ExecuteOutput::Error {
                chunk,
                line,
                message,
                ..
            } => {
                assert_eq!(chunk.as_deref(), Some("src/jobs/x.lua"));
                assert_eq!(line, Some(3));
                assert_eq!(message, "boom");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_module_validates_without_a_handle() {
        let mut input = validate_input("local M = {}\nfunction M.f() return 1 end\nreturn M", &[]);
        assert_eq!(
            validate_source(&input).unwrap().errors[0].kind,
            ScriptErrorKind::MissingHandle
        );
        input.role = ChunkRole::Module;
        input.prepare = true;
        input.chunk_name = Some("src/lib/m.lua".to_string());
        let validated = validate_source(&input).unwrap();
        assert!(validated.valid, "{:?}", validated.errors);
        assert!(validated.artifact.is_some());
    }

    #[test]
    fn a_local_modules_values_survive_file_scope_at_validation() {
        for (role, source) in [
            (
                ChunkRole::Module,
                "local LIMIT = require('./config').limit * 2 .. ''\nreturn { limit = LIMIT }",
            ),
            (
                ChunkRole::Handler,
                "local LIMIT = require('./config').limit * 2 .. ''\nfunction handle() return LIMIT end",
            ),
        ] {
            let mut input = validate_input(source, &[]);
            input.role = role;
            input.chunk_name = Some("src/lib/m.lua".to_string());
            let validated = validate_source(&input).unwrap();
            assert!(validated.valid, "{role:?}: {:?}", validated.errors);
        }
    }

    #[test]
    fn an_artifact_this_build_cannot_load_falls_back_to_the_source_or_fails_by_name() {
        let mut artifact = prepared("function handle() return 'from bytecode' end", "script");
        artifact.engine = "puc-lua-5.1.5".to_string();

        let mut input = execute_input("function handle() return 'from source' end");
        input.artifact = Some(artifact.clone());
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!("from source"),
                value_kind: ScriptValueKind::Other,
            }
        );

        input.source = String::new();
        let refused = execute_script(&input).unwrap_err();
        assert_eq!(refused.code, "INTERPRETER_ERROR");
        assert!(
            refused.message.contains("puc-lua-5.1.5"),
            "{}",
            refused.message
        );
    }

    /// A dump keeps the name it was compiled under whatever it is loaded as,
    /// so an artifact for another file would report its errors, and its
    /// `require` referrer, as that file.
    #[test]
    fn an_artifact_prepared_for_another_chunk_is_not_run_as_this_one() {
        let artifact = prepared("function handle() return 'from a' end", "src/a.lua");

        let mut input = execute_input("function handle() return 'from b' end");
        input.chunk_name = "src/b.lua".to_string();
        input.artifact = Some(artifact.clone());
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!("from b"),
                value_kind: ScriptValueKind::Other,
            }
        );

        input.source = String::new();
        let refused = execute_script(&input).unwrap_err();
        assert_eq!(refused.code, "INTERPRETER_ERROR");
        assert!(
            refused.message.contains("src/a.lua") && refused.message.contains("src/b.lua"),
            "{}",
            refused.message
        );

        // The same artifact under its own name runs from the bytecode.
        input.chunk_name = "src/a.lua".to_string();
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!("from a"),
                value_kind: ScriptValueKind::Other,
            }
        );
    }

    /// Bytes carrying this build's stamp that are not a dump are the host's
    /// artifact being damaged, and are refused before they can run.
    #[test]
    fn a_damaged_artifact_falls_back_to_the_source_or_fails() {
        let mut artifact = prepared("function handle() return 'from bytecode' end", "script");
        artifact.bytes.truncate(artifact.bytes.len() / 2);

        let mut input = execute_input("function handle() return 'from source' end");
        input.artifact = Some(artifact);
        assert_eq!(
            execute_script(&input).unwrap(),
            ExecuteOutput::Returned {
                value: json!("from source"),
                value_kind: ScriptValueKind::Other,
            }
        );

        input.source = String::new();
        let refused = execute_script(&input).unwrap_err();
        assert_eq!(refused.code, "INTERPRETER_ERROR");
        assert!(
            refused.message.contains("did not load"),
            "{}",
            refused.message
        );
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
                ..
            } => (kind, message, line, raw),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// A project chunk's errors name its path, which is what a terminal
    /// makes clickable and what the host prints.
    #[test]
    fn a_project_chunk_reports_its_path_and_line() {
        let mut input = input_with_limits(
            "function handle()\n  local t = nil\n  return t.x\nend",
            json!(1_000_000),
            67_108_864,
        );
        input.chunk_name = "src/lexicons/app/test/q.lua".to_string();
        match run_bounded(input) {
            ExecuteOutput::Error {
                chunk, line, raw, ..
            } => {
                assert_eq!(
                    chunk.as_deref(),
                    Some("src/lexicons/app/test/q.lua"),
                    "{raw}"
                );
                assert_eq!(line, Some(3), "{raw}");
                assert!(raw.contains("src/lexicons/app/test/q.lua:3:"), "{raw}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }

        let mut validate = validate_input("function handle(", &[]);
        validate.chunk_name = Some("src/jobs/x.lua".to_string());
        let refused = validate_source(&validate).unwrap();
        assert_eq!(refused.errors[0].line, Some(1), "{:?}", refused.errors);
    }

    /// Bytecode handed in as source text is refused rather than run: only
    /// the artifact field carries bytecode.
    #[test]
    fn bytecode_in_the_source_field_is_a_syntax_error() {
        let lua = mlua::Lua::new();
        let dumped = lua
            .load("function handle() return 1 end")
            .into_function()
            .unwrap()
            .dump(false);
        let source = String::from_utf8_lossy(&dumped).into_owned();
        let (kind, message, ..) = failure_of(execute_script(&execute_input(&source)).unwrap());
        assert_eq!(kind, ScriptErrorKind::Syntax);
        assert!(message.contains("binary chunk"), "{message}");
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

    /// `async.all` resumes its functions itself, and catches what they raise
    /// until every one has finished, so it has to be one more layer the spent
    /// budget walks out through.
    #[test]
    fn an_instruction_budget_ends_a_run_inside_async_all() {
        for body in [
            "async.all(function() while true do end end)",
            "async.all(function() return 1 end, function() while true do end end)",
            "while true do pcall(async.all, function() while true do end end) end",
            "return { async.all(function() pcall(function() while true do end end) return 1 end, \
                 function() return 2 end) }",
        ] {
            let source =
                format!("local async = require('internal.async') function handle() {body} end");
            let output = run_bounded(input_with_limits(&source, json!(10_000), 67_108_864));
            let (kind, message, _, _) = failure_of(output);
            assert_eq!(kind, ScriptErrorKind::Timeout, "{body}");
            assert!(message.contains("execution limit"), "{body}: {message}");
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

    const TEST_FILE: &str = "src/lib/a.test.lua";

    fn test_file_input(source: &str) -> ExecuteInput {
        serde_json::from_value(json!({
            "source": source,
            "kind": "test",
            "input": null,
            "context": {"trigger": format!("test:{TEST_FILE}")},
            "limits": {"instructions": null, "memory_bytes": 67_108_864},
            "chunk_name": TEST_FILE,
        }))
        .unwrap()
    }

    /// A test file run against `files`, an in-memory project, reporting to a
    /// recording host.
    fn run_test_file_in(
        files: &[(&str, &str)],
        host: std::rc::Rc<test::Recording>,
        source: &str,
    ) -> ExecuteOutput {
        execute_in(
            &test_file_input(source),
            Box::new(modules::tests::Files::new(files)),
            host,
        )
        .unwrap()
    }

    fn run_test_file(source: &str) -> (ExecuteOutput, std::rc::Rc<test::Recording>) {
        let host = std::rc::Rc::new(test::Recording::default());
        let output = run_test_file_in(&[], host.clone(), source);
        (output, host)
    }

    #[test]
    fn a_test_file_reports_every_test_by_its_full_name() {
        let (output, host) = run_test_file(
            r#"describe("format", function()
      it("joins", function() end)
      describe("nested", function()
        it("still runs", function() end)
      end)
    end)
    pending("later")"#,
        );
        assert_eq!(
            output,
            ExecuteOutput::Returned {
                value: json!({"passed": 2, "failed": 0, "skipped": 1}),
                value_kind: ScriptValueKind::Object,
            }
        );
        let reports = host.reports.borrow();
        let names: Vec<&str> = reports.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            ["format > joins", "format > nested > still runs", "later"]
        );
        assert_eq!(reports[2].status, happyview_plugin_sdk::TestStatus::Skip);
        assert!(reports.iter().all(|r| r.path == TEST_FILE));
    }

    /// A module is unit-tested directly: the test file requires it by relative
    /// path, as a handler would.
    #[test]
    fn a_test_file_requires_a_module_and_tests_it() {
        let host = std::rc::Rc::new(test::Recording::default());
        run_test_file_in(
            &[(
                "src/lib/format.lua",
                "return { join = function(a, b) return a .. '-' .. b end }",
            )],
            host.clone(),
            r#"local format = require("./format")
    it("joins", function()
      if format.join("a", "b") ~= "a-b" then error("joined wrong") end
    end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(
            reports[0].status,
            happyview_plugin_sdk::TestStatus::Pass,
            "{:?}",
            reports[0].message
        );
    }

    #[test]
    fn a_runtime_error_fails_its_test_at_its_line_and_the_next_test_runs() {
        let (_, host) = run_test_file(
            r#"it("breaks", function()
      local t = nil
      return t.x
    end)
    it("runs anyway", function() end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(reports[0].status, happyview_plugin_sdk::TestStatus::Fail);
        assert_eq!(reports[0].line, Some(3));
        assert_eq!(reports[0].file.as_deref(), Some(TEST_FILE));
        assert!(reports[0]
            .message
            .as_deref()
            .unwrap()
            .contains("attempt to index a nil value"));
        assert_eq!(reports[1].status, happyview_plugin_sdk::TestStatus::Pass);
    }

    #[test]
    fn hooks_run_in_order_and_after_each_runs_after_a_failure() {
        let (_, host) = run_test_file(
            r#"local seen = {}
    describe("hooks", function()
      before_each(function() seen[#seen + 1] = "before" end)
      after_each(function() seen[#seen + 1] = "after" end)
      it("first", function() seen[#seen + 1] = "first" error("boom") end)
      it("second", function()
        if table.concat(seen, ",") ~= "before,first,after,before" then error(table.concat(seen, ",")) end
      end)
    end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(reports[0].status, happyview_plugin_sdk::TestStatus::Fail);
        assert_eq!(reports[0].line, Some(5));
        assert_eq!(
            reports[1].status,
            happyview_plugin_sdk::TestStatus::Pass,
            "{:?}",
            reports[1].message
        );
    }

    #[test]
    fn a_throwing_after_each_fails_a_passing_test_but_keeps_an_earlier_failure() {
        let (_, host) = run_test_file(
            r#"describe("hooks", function()
      after_each(function()
        error("cleanup")
      end)
      it("failed", function()
        error("first")
      end)
      it("passed", function() end)
    end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(reports[0].status, happyview_plugin_sdk::TestStatus::Fail);
        assert_eq!(reports[0].message.as_deref(), Some("first"));
        assert_eq!(reports[0].line, Some(6));
        assert_eq!(reports[1].status, happyview_plugin_sdk::TestStatus::Fail);
        assert_eq!(reports[1].message.as_deref(), Some("cleanup"));
        assert_eq!(reports[1].line, Some(3));
    }

    #[test]
    fn an_assertion_outside_a_test_keeps_its_file_and_line() {
        for (source, line) in [
            ("local x = 1\nassert.are.equal(1, 2)", 2),
            (
                "describe(\"d\", function()\n  local x = 1\n  assert.are.equal(1, 2)\nend)",
                3,
            ),
        ] {
            let (output, _) = run_test_file(source);
            match output {
                ExecuteOutput::Error {
                    chunk,
                    line: got,
                    message,
                    ..
                } => {
                    assert_eq!(chunk.as_deref(), Some(TEST_FILE), "{source}");
                    assert_eq!(got, Some(line), "{source}");
                    assert_eq!(message, "expected 1, got 2", "{source}");
                }
                other => panic!("expected an error, got {other:?}"),
            }
        }
    }

    #[test]
    fn hooks_nest_outer_first_and_a_failing_before_each_skips_the_body() {
        let (_, host) = run_test_file(
            r#"local seen = {}
    describe("outer", function()
      before_each(function() seen[#seen + 1] = "o-before" end)
      after_each(function() seen[#seen + 1] = "o-after" end)
      describe("inner", function()
        before_each(function() seen[#seen + 1] = "i-before" end)
        after_each(function() seen[#seen + 1] = "i-after" end)
        it("a", function() seen[#seen + 1] = "a" end)
        before_each(function() if #seen > 5 then error("stop") end end)
        it("b", function() seen[#seen + 1] = "body-b" end)
      end)
      it("c", function()
        if table.concat(seen, ",") ~= "o-before,i-before,a,i-after,o-after,o-before,i-before,i-after,o-after,o-before" then
          error(table.concat(seen, ","))
        end
      end)
    end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(reports[1].message.as_deref(), Some("stop"));
        assert_eq!(
            reports[2].status,
            happyview_plugin_sdk::TestStatus::Pass,
            "{:?}",
            reports[2].message
        );
    }

    #[test]
    fn a_hook_cannot_be_registered_inside_a_running_test() {
        let (_, host) = run_test_file(r#"it("outer", function() before_each(function() end) end)"#);
        let reports = host.reports.borrow();
        assert!(reports[0]
            .message
            .as_deref()
            .unwrap()
            .contains("inside a running test"));
    }

    #[test]
    fn a_test_cannot_declare_another_test() {
        let (_, host) = run_test_file(r#"it("outer", function() it("inner", function() end) end)"#);
        let reports = host.reports.borrow();
        assert_eq!(reports.len(), 1);
        assert!(reports[0]
            .message
            .as_deref()
            .unwrap()
            .contains("inside a running test"));
    }

    #[test]
    fn a_syntax_error_fails_the_file_rather_than_a_test() {
        let (output, host) = run_test_file("describe(\"x\", function(");
        assert!(matches!(
            output,
            ExecuteOutput::Error {
                kind: ScriptErrorKind::Syntax,
                ..
            }
        ));
        assert!(host.reports.borrow().is_empty());
    }

    #[test]
    fn internal_test_is_refused_outside_a_test_file() {
        let output = execute_script(&execute_input(
            r#"local t = require("internal.test") function handle() end"#,
        ))
        .unwrap();
        let message = message_of(output);
        assert!(message.contains("only in test files"), "{message}");
    }

    #[test]
    fn a_test_file_validates_without_a_handle() {
        let mut input = validate_input(
            r#"describe("x", function() it("y", function() end) end)"#,
            &[],
        );
        input.role = ChunkRole::Test;
        input.chunk_name = Some(TEST_FILE.to_string());
        assert!(
            validate_source(&input).unwrap().valid,
            "the globals exist while it validates"
        );
        input.role = ChunkRole::Handler;
        assert!(
            !validate_source(&input).unwrap().valid,
            "a handler still needs handle"
        );
    }

    fn failures(source: &str) -> Vec<(String, Option<u32>)> {
        let (_, host) = run_test_file(source);
        let reports = host.reports.borrow();
        reports
            .iter()
            .filter(|r| r.status == happyview_plugin_sdk::TestStatus::Fail)
            .map(|r| (r.message.clone().unwrap_or_default(), r.line))
            .collect()
    }

    #[test]
    fn each_assertion_passes_and_fails_with_its_message_at_its_line() {
        let got = failures(
            r#"it("equal", function() assert.are.equal(200, 404) end)
    it("same", function() assert.are.same({ a = { 1, 2 } }, { a = { 1, 3 } }) end)
    it("true", function() assert.is_true(1) end)
    it("false", function() assert.is_false(nil) end)
    it("nil", function() assert.is_nil("x") end)
    it("plain", function() assert(false, "nope") end)
    it("passes", function()
      assert.are.equal("a", "a")
      assert.are.same({ a = { 1, 2 }, b = "x" }, { b = "x", a = { 1, 2 } })
      assert.is_true(true)
      assert.is_false(false)
      assert.is_nil(nil)
      assert(1 == 1)
    end)"#,
        );
        assert_eq!(
            got,
            vec![
                ("expected 200, got 404".to_string(), Some(1)),
                (
                    "expected { a = { 1, 2 } }, got { a = { 1, 3 } }".to_string(),
                    Some(2)
                ),
                ("expected true, got 1".to_string(), Some(3)),
                ("expected false, got nil".to_string(), Some(4)),
                ("expected nil, got \"x\"".to_string(), Some(5)),
                ("nope".to_string(), Some(6)),
            ]
        );
    }

    #[test]
    fn has_error_checks_that_a_call_raises_and_optionally_what() {
        let got = failures(
            r#"it("raises", function() assert.has_error(function() error("boom") end) end)
    it("matches", function() assert.has_error(function() error("boom") end, "boom") end)
    it("none", function() assert.has_error(function() end) end)
    it("other", function() assert.has_error(function() error("bang") end, "boom") end)"#,
        );
        assert_eq!(
            got,
            vec![
                ("expected an error, got none".to_string(), Some(3)),
                (
                    "expected the error \"boom\", got \"bang\"".to_string(),
                    Some(4)
                ),
            ]
        );
    }

    #[test]
    fn a_failure_in_a_helper_is_placed_at_the_helpers_line() {
        let got = failures(
            r#"local function check(status)
      assert.are.equal(200, status)
    end
    it("uses a helper", function()
      check(500)
    end)"#,
        );
        assert_eq!(got, vec![("expected 200, got 500".to_string(), Some(2))]);
    }

    #[test]
    fn trigger_calls_reach_the_host_with_the_wire_shape() {
        use happyview_plugin_sdk::TestTriggerKind;
        let (_, host) = run_test_file(
            r#"local test = require("internal.test")
    it("query", function()
      local res = test.query("com.example.getThing", { id = "1" }, { as = "did:plc:alice" })
      assert.are.equal(200, res.status)
      assert.are.equal(1, res.body.thing)
    end)
    it("record", function()
      test.record_create("com.example.thing", { did = "did:plc:alice", rkey = "1", record = { n = 1 } })
      test.record_delete("com.example.thing", { rkey = "1" })
    end)
    it("job and label", function()
      test.job("export", { n = 1 })
      test.label("at://did:plc:alice/com.example.thing/1", { val = "spam" })
    end)
    it("mocks", function()
      test.mock("http", { get = { { status = 200 }, { status = 500 } } })
      assert.are.equal("get", test.calls("http")[1].method)
      assert.are.same({}, test.pds_writes())
    end)"#,
        );
        let failed: Vec<_> = host
            .reports
            .borrow()
            .iter()
            .filter(|r| r.status != happyview_plugin_sdk::TestStatus::Pass)
            .map(|r| (r.name.clone(), r.message.clone()))
            .collect();
        assert!(failed.is_empty(), "{failed:?}");

        let triggers = host.triggers.borrow();
        assert_eq!(triggers[0].kind, TestTriggerKind::Query);
        assert_eq!(triggers[0].input, json!({"id": "1"}));
        assert_eq!(triggers[0].as_did.as_deref(), Some("did:plc:alice"));
        assert_eq!(triggers[1].kind, TestTriggerKind::RecordCreate);
        assert_eq!(triggers[1].did.as_deref(), Some("did:plc:alice"));
        assert_eq!(triggers[1].rkey.as_deref(), Some("1"));
        assert_eq!(triggers[1].input, json!({"n": 1}));
        assert_eq!(triggers[2].kind, TestTriggerKind::RecordDelete);
        assert_eq!(triggers[2].input, json!(null));
        assert_eq!(triggers[3].kind, TestTriggerKind::Job);
        assert_eq!(triggers[4].kind, TestTriggerKind::Label);
        assert_eq!(triggers[4].input, json!({"val": "spam"}));
        let mocks = host.mocks.borrow();
        assert_eq!(
            mocks[0].responses,
            json!({"get": [{"status": 200}, {"status": 500}]})
        );
    }

    #[test]
    fn a_host_refusal_fails_the_test_at_the_call() {
        let host = std::rc::Rc::new(test::Recording::default());
        host.refuse.set(true);
        run_test_file_in(
            &[],
            host.clone(),
            r#"local test = require("internal.test")
    it("untracked", function()
      test.record_create("com.example.unknown", { record = {} })
    end)"#,
        );
        let reports = host.reports.borrow();
        assert_eq!(reports[0].status, happyview_plugin_sdk::TestStatus::Fail);
        assert_eq!(reports[0].line, Some(3));
        assert_eq!(
            reports[0].message.as_deref(),
            Some("test.record_create: com.example.unknown has no record lexicon in this project")
        );
    }
}
