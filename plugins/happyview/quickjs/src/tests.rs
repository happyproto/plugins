//! The two exports end to end: the contract, the event loop against a host
//! that settles calls in the order a test picks, the budgets, imports, and
//! the error contract the host reads.

use super::*;
use crate::backend::fake::{Fake, Order};
use happyview_plugin_sdk::{ApiExport, ApiMethod, ApiSurface, ChunkRole};
use serde_json::json;

fn execute_script(input: &ExecuteInput) -> Result<ExecuteOutput, PluginError> {
    execute(&JavaScript, input)
}

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    validate(&JavaScript, input)
}

fn input(source: &str, extra: serde_json::Value) -> ExecuteInput {
    let mut base = json!({
        "source": source,
        "kind": "xrpc_query",
        "input": {},
        "context": { "trigger": "xrpc.query:app.test.q" },
        "limits": { "instructions": 1_000_000, "memory_bytes": 67_108_864 },
    });
    if let serde_json::Value::Object(extra) = extra {
        for (key, value) in extra {
            base[key] = value;
        }
    }
    serde_json::from_value(base).expect("the execute input should deserialize")
}

fn execute_input(source: &str) -> ExecuteInput {
    input(source, json!({}))
}

fn with_limits(source: &str, instructions: serde_json::Value, memory: u64) -> ExecuteInput {
    input(
        source,
        json!({ "limits": { "instructions": instructions, "memory_bytes": memory } }),
    )
}

fn validate_input(source: &str) -> ValidateInput {
    ValidateInput {
        source: source.to_string(),
        removed_globals: Vec::new(),
        ..Default::default()
    }
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

fn returned(output: ExecuteOutput) -> (serde_json::Value, ScriptValueKind) {
    match output {
        ExecuteOutput::Returned { value, value_kind } => (value, value_kind),
        other => panic!("expected a value, got {other:?}"),
    }
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

/// The standard `db` library's shape, plus an `http` with a function whose
/// name is a reserved word.
fn libraries() -> serde_json::Value {
    json!({
        "libraries": [
            { "namespace": "happyview.db", "id": "happyview-db" },
            { "namespace": "happyview.http", "id": "happyview-http" },
        ]
    })
}

fn fake() -> Fake {
    let mut records = ApiExport::constructor("records");
    records.methods = vec![
        ApiMethod::lazy("where"),
        ApiMethod::lazy("limit"),
        ApiMethod::immediate("run"),
    ];
    Fake::new()
        .surface(
            "happyview-db",
            ApiSurface::new("happyview.db")
                .export(ApiExport::function("get"))
                .export(records),
        )
        .surface(
            "happyview-http",
            ApiSurface::new("happyview.http")
                .export(ApiExport::function("get"))
                .export(ApiExport::function("delete")),
        )
}

/// The responder answers each call with what it was asked, so a result in
/// the wrong slot would show.
fn echo(fake: Fake) -> Fake {
    fake.respond(|library, function, args| {
        Ok(json!({ "via": format!("{library}.{function}"), "args": args }))
    })
}

/// `source` run on `fake`, with the two libraries paired and `extra` laid
/// over the input.
fn run_with(fake: Fake, source: &str, extra: serde_json::Value) -> (ExecuteOutput, Rc<Fake>) {
    let fake = Rc::new(fake);
    let mut fields = libraries();
    if let serde_json::Value::Object(extra) = extra {
        for (key, value) in extra {
            fields[key] = value;
        }
    }
    let output = execute_with(&JavaScript, &input(source, fields), fake.clone())
        .expect("an output, not an envelope error");
    (output, fake)
}

// --- the contract ---------------------------------------------------------

#[test]
fn what_a_script_returned_carries_its_kind() {
    for (source, value, kind) in [
        (
            "export default function handle() {}",
            json!(null),
            ScriptValueKind::None,
        ),
        (
            "export default () => null",
            json!(null),
            ScriptValueKind::None,
        ),
        (
            "export default () => ({ a: 1 })",
            json!({ "a": 1 }),
            ScriptValueKind::Object,
        ),
        (
            "export default () => [1, 2]",
            json!([1, 2]),
            ScriptValueKind::Object,
        ),
        ("export default () => 7", json!(7), ScriptValueKind::Other),
        (
            "export default () => 'x'",
            json!("x"),
            ScriptValueKind::Other,
        ),
        (
            "export default () => false",
            json!(false),
            ScriptValueKind::Other,
        ),
        // A returned promise is awaited first, and what it settles to is what
        // counts.
        (
            "export default async () => ({ a: 1 })",
            json!({ "a": 1 }),
            ScriptValueKind::Object,
        ),
        (
            "export default async () => {}",
            json!(null),
            ScriptValueKind::None,
        ),
        (
            "export default () => Promise.resolve(3)",
            json!(3),
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

#[test]
fn handle_receives_the_input_and_the_context() {
    let input = input(
        "export default function handle(input, ctx) { \
           return { q: input.q, trigger: ctx.trigger, who: ctx.caller_did, \
                    hasJob: 'job' in ctx }; }",
        json!({
            "input": { "q": "x" },
            "context": { "trigger": "xrpc.query:app.test.q", "caller_did": "did:plc:me" },
        }),
    );
    assert_eq!(
        returned(execute_script(&input).unwrap()).0,
        json!({ "q": "x", "trigger": "xrpc.query:app.test.q", "who": "did:plc:me", "hasJob": false })
    );
}

#[test]
fn file_scope_runs_once_before_handle_and_top_level_await_settles_first() {
    let output = execute_script(&execute_input(
        "let order = []; \
         order.push('file'); \
         await Promise.resolve(); \
         order.push('after await'); \
         export default function handle() { order.push('handle'); return order; }",
    ))
    .unwrap();
    assert_eq!(returned(output).0, json!(["file", "after await", "handle"]));
}

#[test]
fn a_script_with_no_default_function_is_refused_by_both_exports() {
    for source in [
        "const x = 1;",
        "export const handle = () => 1;",
        "export default 5;",
    ] {
        let (kind, message, ..) = failure_of(execute_script(&execute_input(source)).unwrap());
        assert_eq!(kind, ScriptErrorKind::MissingHandle, "{source}");
        assert_eq!(message, MISSING_HANDLE);
        let validated = validate_source(&validate_input(source)).unwrap();
        assert!(!validated.valid, "{source}");
        assert_eq!(validated.errors[0].kind, ScriptErrorKind::MissingHandle);
    }
}

// --- the error contract ---------------------------------------------------

#[test]
fn every_error_kind_is_produced_by_a_script_that_earns_it() {
    let cases: [(&str, serde_json::Value, ScriptErrorKind); 5] = [
        (
            "export default function handle(",
            json!(1_000_000),
            ScriptErrorKind::Syntax,
        ),
        (
            "const x = 1;",
            json!(1_000_000),
            ScriptErrorKind::MissingHandle,
        ),
        (
            "export default () => { throw new Error('boom'); }",
            json!(1_000_000),
            ScriptErrorKind::Runtime,
        ),
        (
            "export default () => { while (true) {} }",
            json!(1_000),
            ScriptErrorKind::Timeout,
        ),
        (
            "export default () => { const t = []; while (true) t.push([1, 2, 3, 4]); }",
            json!(null),
            ScriptErrorKind::Memory,
        ),
    ];
    for (source, instructions, expected) in cases {
        let output = run_bounded(with_limits(source, instructions, 8 * 1024 * 1024));
        let (kind, message, ..) = failure_of(output);
        assert_eq!(kind, expected, "{source}: {message}");
    }
}

#[test]
fn a_runtime_error_carries_its_line_and_a_message_with_no_position() {
    let output = run_bounded(execute_input(
        "export default function handle() {\n  const t = undefined;\n  return t.x;\n}",
    ));
    let (kind, message, line, raw) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(line, Some(3), "{raw}");
    assert_eq!(message, "TypeError: cannot read property 'x' of undefined");
    // The stack the host logs is kept whole.
    assert!(raw.contains("at handle (script:3"), "{raw}");
}

#[test]
fn a_thrown_error_reads_as_its_message_and_anything_else_as_its_string() {
    for (source, expected, line) in [
        (
            "export default () => {\n  throw new Error('boom');\n}",
            "boom",
            Some(2),
        ),
        (
            "export default () => { throw new RangeError('far'); }",
            "RangeError: far",
            Some(1),
        ),
        ("export default () => { throw 'plain'; }", "plain", None),
        (
            "export default () => { throw { code: 1 }; }",
            "[object Object]",
            None,
        ),
        (
            "export default async () => { throw new Error('later'); }",
            "later",
            Some(1),
        ),
    ] {
        let (kind, message, at, _) = failure_of(run_bounded(execute_input(source)));
        assert_eq!(kind, ScriptErrorKind::Runtime, "{source}");
        assert_eq!(message, expected, "{source}");
        assert_eq!(at, line, "{source}");
    }
}

#[test]
fn a_syntax_error_carries_its_line() {
    let (kind, message, line, raw) =
        failure_of(run_bounded(execute_input("const a = 1;\nconst = 2;\n")));
    assert_eq!(kind, ScriptErrorKind::Syntax);
    assert_eq!(line, Some(2), "{raw}");
    assert!(message.starts_with("SyntaxError:"), "{message}");

    let validated = validate_source(&validate_input("const a = 1;\nconst = 2;\n")).unwrap();
    assert_eq!(validated.errors[0].kind, ScriptErrorKind::Syntax);
    assert_eq!(validated.errors[0].line, Some(2));
}

#[test]
fn an_auth_error_message_survives_untouched() {
    let (kind, message, line, _) = failure_of(run_bounded(execute_input(
        "export default () => { throw new Error('AUTH_ERROR: no session'); }",
    )));
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(message, "AUTH_ERROR: no session");
    assert_eq!(line, Some(1));
}

// --- the budgets ----------------------------------------------------------

#[test]
fn an_instruction_budget_ends_a_run_through_every_layer_of_catching() {
    for source in [
        "export default () => { while (true) {} }",
        "export default () => { while (true) { try { while (true) {} } catch (e) {} } }",
        "export default () => { try { while (true) {} } catch (e) {} return { ok: true }; }",
        "export default () => { try { while (true) {} } finally { return { ok: true }; } }",
        "export default async () => { try { await (async () => { while (true) {} })(); } \
         catch (e) { return { caught: String(e) }; } }",
        "export default () => new Promise(() => { while (true) {} }).catch(() => ({ ok: true }))",
        "export default async () => { for (;;) await null; }",
        "export default () => { const f = () => f(); try { f(); } catch (e) {} while (true) {} }",
        "while (true) {}\nexport default () => 1;",
    ] {
        let output = run_bounded(with_limits(source, json!(10_000), 67_108_864));
        let (kind, message, ..) = failure_of(output);
        assert_eq!(kind, ScriptErrorKind::Timeout, "{source}");
        assert_eq!(message, budget::SPENT, "{source}");
    }
}

/// An `await` on a library call the interrupted code had already started is
/// one way a script could reach a normal return after the interrupt; the
/// limit wins anyway.
#[test]
fn a_spent_budget_wins_over_a_return_reached_afterwards() {
    let (output, _) = run_with(
        echo(fake()),
        "import db from 'happyview.db';
         export default async () => {
           const pending = db.get('x');
           try { (() => { while (true) {} })(); } catch (e) {}
           return await pending;
         }",
        json!({ "limits": { "instructions": 10_000, "memory_bytes": 67_108_864 } }),
    );
    assert_eq!(failure_of(output).0, ScriptErrorKind::Timeout);
}

#[test]
fn no_instruction_budget_runs_a_long_loop_to_completion() {
    let output = run_bounded(with_limits(
        "export default () => { let n = 0; for (let i = 0; i < 3000000; i++) n++; return n; }",
        json!(null),
        67_108_864,
    ));
    assert_eq!(returned(output).0, json!(3_000_000));
}

#[test]
fn a_run_under_its_budget_keeps_ordinary_try_and_catch() {
    let output = run_bounded(execute_input(
        "export default async () => {
           let caught, rejected, finished;
           try { throw new Error('boom'); } catch (e) { caught = e.message; }
           try { await Promise.reject(new Error('bang')); } catch (e) { rejected = e.message; }
           try { } finally { finished = true; }
           return { caught, rejected, finished };
         }",
    ));
    assert_eq!(
        returned(output).0,
        json!({ "caught": "boom", "rejected": "bang", "finished": true })
    );
}

#[test]
fn a_memory_ceiling_is_a_memory_failure_rather_than_a_trap() {
    let output = run_bounded(with_limits(
        "export default () => { const t = []; let i = 0; while (true) t.push([i, i, i, i++]); }",
        json!(null),
        8 * 1024 * 1024,
    ));
    let (kind, message, ..) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Memory);
    assert!(message.contains("out of memory"), "{message}");
}

#[test]
fn deep_recursion_is_an_error_the_script_can_catch() {
    let output = run_bounded(execute_input(
        "export default () => { const f = (n) => f(n + 1) + 1; \
         try { f(0); } catch (e) { return e.message; } }",
    ));
    assert_eq!(
        returned(output).0,
        json!("Maximum call stack size exceeded")
    );
}

// --- the event loop -------------------------------------------------------

const ALL_THREE: &str = "import db from 'happyview.db';
import http from 'happyview.http';
export default async () => {
  const [a, b, c] = await Promise.all([db.get('a'), http.get('b'), db.records('c').limit(1).run()]);
  return [a, b, c];
}";

#[test]
fn promise_all_settles_whichever_order_the_host_answers_in() {
    for order in [Order::Lowest, Order::Highest] {
        let (output, fake) = run_with(echo(fake().order(order)), ALL_THREE, json!({}));
        assert_eq!(
            returned(output).0,
            json!([
                { "via": "happyview-db.get", "args": ["a"] },
                { "via": "happyview-http.get", "args": ["b"] },
                { "via": "happyview-db.records", "args": [{
                    "args": ["c"], "steps": [{ "limit": [1] }], "call": { "name": "run", "args": [] },
                }] },
            ])
        );
        // All three were started before any was waited on.
        assert_eq!(fake.calls.borrow().len(), 3);
        let settled = fake.settled.borrow().clone();
        match order {
            Order::Lowest => assert_eq!(settled, vec![1, 2, 3]),
            Order::Highest => assert_eq!(settled, vec![3, 2, 1]),
        }
    }
}

#[test]
fn named_and_default_imports_reach_the_same_functions() {
    let (output, _) = run_with(
        echo(fake()),
        "import db, { get, records } from 'happyview.db';
         import { delete as remove } from 'happyview.http';
         import * as all from 'happyview.db';
         export default async () => [
           get === db.get, records === db.records, all.default === db, typeof remove,
           await remove('x'),
         ];",
        json!({}),
    );
    assert_eq!(
        returned(output).0,
        json!([true, true, true, "function", { "via": "happyview-http.delete", "args": ["x"] }])
    );
}

#[test]
fn a_sequence_of_awaits_runs_one_call_at_a_time() {
    let (output, fake) = run_with(
        echo(fake()),
        "import db from 'happyview.db';
         export default async () => {
           const a = await db.get(1);
           return await db.get(a.args[0] + 1);
         }",
        json!({}),
    );
    assert_eq!(returned(output).0["args"], json!([2]));
    assert_eq!(*fake.settled.borrow(), vec![1, 2]);
}

#[test]
fn a_library_rejection_is_an_error_carrying_the_envelope() {
    let fake = fake().respond(|_, _, _| Err(PluginError::new("NOT_FOUND", "no such record")));
    let (output, _) = run_with(
        fake,
        "import db from 'happyview.db';
         export default async () => {
           try { await db.get('x'); } catch (e) {
             return { error: e instanceof Error, message: e.message, code: e.code, retryable: e.retryable };
           }
         }",
        json!({}),
    );
    assert_eq!(
        returned(output).0,
        json!({
            "error": true,
            "message": "happyview-db.get: Plugin returned error: NOT_FOUND - no such record",
            "code": "NOT_FOUND",
            "retryable": false,
        })
    );
}

/// The host answers 401 when it finds `AUTH_ERROR:` anywhere in a failure's
/// text, so a library's credential failure left to propagate has to carry it
/// out of the run.
#[test]
fn an_unhandled_library_auth_error_carries_its_prefix_out_of_the_run() {
    let fake = fake().respond(|_, _, _| {
        Err(PluginError::new(
            "LIBRARY_ERROR",
            "AUTH_ERROR:DPoP session not found",
        ))
    });
    let (output, _) = run_with(
        fake,
        "import db from 'happyview.db';\nexport default async () => {\n  return await db.get('x');\n};",
        json!({}),
    );
    let (kind, message, _, raw) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(
        message,
        "happyview-db.get: AUTH_ERROR:DPoP session not found"
    );
    assert!(raw.contains("AUTH_ERROR:"), "{raw}");
}

#[test]
fn a_promise_nothing_can_settle_is_a_deadlock_rather_than_a_hang() {
    for (source, what) in [
        (
            "export default () => new Promise(() => {});",
            "handle's promise",
        ),
        (
            "await new Promise(() => {});\nexport default () => 1;",
            "the module",
        ),
    ] {
        let (kind, message, ..) = failure_of(run_bounded(execute_input(source)));
        assert_eq!(kind, ScriptErrorKind::Runtime, "{source}");
        assert_eq!(message, never_settled(what), "{source}");
    }
}

/// A call left running when `handle` settles is the host's to drain; the
/// run still answers, and the call was started.
#[test]
fn an_unawaited_call_does_not_hold_the_run() {
    let (output, fake) = run_with(
        echo(fake()),
        "import db from 'happyview.db';
         export default () => { db.get('fire and forget'); return { done: true }; }",
        json!({}),
    );
    assert_eq!(returned(output).0, json!({ "done": true }));
    assert_eq!(fake.calls.borrow().len(), 1);
    assert!(fake.settled.borrow().is_empty());
}

#[test]
fn top_level_await_reaches_a_library_through_the_loop() {
    let (output, _) = run_with(
        echo(fake()),
        "import db from 'happyview.db';
         const first = await db.get('boot');
         export default () => first;",
        json!({}),
    );
    assert_eq!(returned(output).0["args"], json!(["boot"]));
}

/// A wait the host refuses names no call, so nothing in the script can be
/// told about it: it is this plugin failing, and leaves as an envelope error.
#[test]
fn a_host_that_refuses_a_wait_is_the_interpreter_failing() {
    struct Refusing(Fake);
    impl Backend for Refusing {
        fn library_surface(&self, l: &str) -> Result<ApiSurface, PluginError> {
            self.0.library_surface(l)
        }
        fn start(&self, l: &str, f: &str, a: &[serde_json::Value]) -> Result<u32, PluginError> {
            self.0.start(l, f, a)
        }
        fn wait_any(
            &self,
            _: &[u32],
        ) -> Result<(u32, Result<serde_json::Value, PluginError>), PluginError> {
            Err(PluginError::bad_input("handle 9 was never issued"))
        }
        fn script_log(
            &self,
            r: &happyview_plugin_sdk::ScriptLogRequest,
        ) -> Result<(), PluginError> {
            self.0.script_log(r)
        }
        fn job_progress(
            &self,
            r: &happyview_plugin_sdk::JobProgressRequest,
        ) -> Result<(), PluginError> {
            self.0.job_progress(r)
        }
        fn job_should_stop(&self) -> Result<bool, PluginError> {
            self.0.job_should_stop()
        }
        fn job_wait(&self, s: f64) -> Result<(), PluginError> {
            self.0.job_wait(s)
        }
        fn test_trigger(
            &self,
            spec: &happyview_plugin_sdk::TestTrigger,
        ) -> Result<happyview_plugin_sdk::TestTriggerResult, PluginError> {
            self.0.test_trigger(spec)
        }
        fn test_mock(&self, spec: &happyview_plugin_sdk::TestMock) -> Result<(), PluginError> {
            self.0.test_mock(spec)
        }
        fn test_calls(
            &self,
            spec: &happyview_plugin_sdk::TestCalls,
        ) -> Result<Vec<happyview_plugin_sdk::TestCall>, PluginError> {
            self.0.test_calls(spec)
        }
        fn test_pds_writes(&self) -> Result<Vec<happyview_plugin_sdk::TestPdsWrite>, PluginError> {
            self.0.test_pds_writes()
        }
        fn test_report(
            &self,
            report: &happyview_plugin_sdk::TestReport,
        ) -> Result<(), PluginError> {
            self.0.test_report(report)
        }
    }
    let input = input(
        "import db from 'happyview.db';\nexport default () => db.get('x');",
        libraries(),
    );
    let error = execute_with(&JavaScript, &input, Rc::new(Refusing(fake()))).unwrap_err();
    assert_eq!(error.code, "BAD_INPUT");
}

#[test]
fn a_job_waits_through_a_promise() {
    let (output, fake) = run_with(
        fake(),
        "export default async (input, ctx) => {
           ctx.job.progress({ step: 1 });
           await ctx.job.wait(3);
           return { stop: ctx.job.should_stop(), id: ctx.job.id };
         }",
        json!({
            "kind": "job",
            "context": { "trigger": "job.run:reindex", "job": { "id": "j1" } },
            "limits": { "instructions": null, "memory_bytes": 67_108_864 },
        }),
    );
    assert_eq!(returned(output).0, json!({ "stop": false, "id": "j1" }));
    assert_eq!(*fake.waits.borrow(), vec![3.0]);
    assert_eq!(*fake.progress.borrow(), vec![json!({ "step": 1 })]);
}

// --- imports --------------------------------------------------------------

#[test]
fn a_builtin_resolves_by_default_and_by_name() {
    let output = execute_script(&execute_input(
        "import json from 'internal.json';
         import { encode } from 'internal.json';
         import { now } from 'internal.time';
         export default () => ({ same: encode === json.encode, encoded: encode({ a: 1 }), now: now() > 0 });",
    ))
    .unwrap();
    assert_eq!(
        returned(output).0,
        json!({ "same": true, "encoded": r#"{"a":1}"#, "now": true })
    );
}

#[test]
fn an_import_nothing_serves_is_refused_naming_what_would() {
    let path_refusal = |name: &str| {
        format!(
            "cannot import '{name}': a script imports built-in modules and installed \
             libraries by name, and its project's own modules by a path starting with ./ or ../, \
             never an absolute path or a URL"
        )
    };
    for (source, expected) in [
        (
            "import db from 'happyview.db';\nexport default () => 1;".to_string(),
            "module 'happyview.db' not found -- is the 'happyview.db' library plugin installed?"
                .to_string(),
        ),
        (
            "import x from 'internal.nope';\nexport default () => 1;".to_string(),
            "module 'internal.nope' not found -- built-in modules are: \
             internal.logging, internal.time, internal.tids, internal.json"
                .to_string(),
        ),
        (
            "import x from 'https://example.com/x.js';\nexport default () => 1;".to_string(),
            path_refusal("https://example.com/x.js"),
        ),
        (
            "export default async () => { await import('/etc/passwd'); }".to_string(),
            path_refusal("/etc/passwd"),
        ),
    ] {
        let (kind, message, ..) = failure_of(run_bounded(execute_input(&source)));
        assert_eq!(kind, ScriptErrorKind::Runtime, "{source}");
        assert_eq!(message, expected, "{source}");
    }

    // A relative path is the host's to answer, and off wasm the host is not
    // there to answer it.
    let (_, message, ..) = failure_of(run_bounded(execute_input(
        "import x from './helper.js';\nexport default () => 1;",
    )));
    assert!(
        message.starts_with("cannot resolve './helper.js':"),
        "{message}"
    );
}

/// A library may not take an `internal.` name, whichever order the two
/// lookups happen in.
#[test]
fn an_internal_name_is_refused_even_when_a_library_claims_it() {
    let (output, _) = run_with(
        fake(),
        "import x from 'internal.sneaky';\nexport default () => 1;",
        json!({ "libraries": [{ "namespace": "internal.sneaky", "id": "happyview-db" }] }),
    );
    let (_, message, ..) = failure_of(output);
    assert!(message.contains("built-in modules are:"), "{message}");

    let (output, _) = run_with(
        fake(),
        "import json from 'internal.json';\nexport default () => typeof json.encode;",
        json!({ "libraries": [{ "namespace": "internal.json", "id": "happyview-db" }] }),
    );
    assert_eq!(returned(output).0, json!("function"));
}

#[test]
fn a_library_whose_surface_cannot_be_read_fails_naming_the_namespace() {
    let (output, _) = run_with(
        Fake::new(),
        "import db from 'happyview.db';\nexport default () => 1;",
        json!({}),
    );
    let (_, message, ..) = failure_of(output);
    assert_eq!(message, "happyview.db: no library happyview-db");

    // The real host, off wasm, fails the same way: the pairing is in the
    // input, and only the surface fetch needs the host.
    let input = input(
        "import db from 'happyview.db';\nexport default () => 1;",
        libraries(),
    );
    let (_, message, ..) = failure_of(execute_script(&input).unwrap());
    assert!(
        message.starts_with("happyview.db: Plugin returned error:"),
        "{message}"
    );
}

#[test]
fn a_named_import_a_library_does_not_export_is_refused() {
    let (output, _) = run_with(
        fake(),
        "import { nope } from 'happyview.db';\nexport default () => 1;",
        json!({}),
    );
    let (kind, message, ..) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(
        message,
        "SyntaxError: Could not find export 'nope' in module 'happyview.db'"
    );
}

#[test]
fn console_reaches_the_log() {
    let (output, fake) = run_with(
        fake(),
        "console.log('loaded');\nexport default () => { console.warn('w', { n: 1 }); };",
        json!({}),
    );
    returned(output);
    let messages: Vec<String> = fake
        .logs
        .borrow()
        .iter()
        .map(|log| log.message.clone())
        .collect();
    assert_eq!(messages, vec!["loaded", r#"w {"n":1}"#]);
}

// --- validate -------------------------------------------------------------

fn validate_with(source: &str, chunk_name: &str, prepare: bool, module: bool) -> ValidateOutput {
    let mut input = validate_input(source);
    input.chunk_name = Some(chunk_name.to_string());
    input.prepare = prepare;
    input.role = if module {
        ChunkRole::Module
    } else {
        ChunkRole::Handler
    };
    validate_source(&input).unwrap()
}

#[test]
fn validate_prepares_only_when_asked_and_only_what_is_valid() {
    let source = "export default () => 1;";
    assert!(validate_with(source, "script", false, false)
        .artifact
        .is_none());
    assert!(validate_with("export default (", "script", true, false)
        .artifact
        .is_none());
    let artifact = validate_with(source, "script", true, false)
        .artifact
        .expect("a valid, prepared script carries its artifact");
    assert_eq!(artifact.format, "quickjs-bytecode");
    assert_eq!(artifact.source_map, None);
    assert!(!artifact.bytes.is_empty());
}

#[test]
fn a_relative_import_is_a_stub_while_validating() {
    let validated = validate_with(
        "import { format } from './lib/format.js';\nimport helper from '../helper.js';\nexport default () => format(helper);",
        "src/lexicons/app/test/q.js",
        true,
        false,
    );
    assert!(validated.valid, "{:?}", validated.errors);
}

#[test]
fn a_module_validates_without_a_default_export() {
    let source = "export function shout(s) { return s.toUpperCase(); }";
    assert_eq!(
        validate_with(source, "lib/m.js", true, false).errors[0].kind,
        ScriptErrorKind::MissingHandle
    );
    let validated = validate_with(source, "lib/m.js", true, true);
    assert!(validated.valid, "{:?}", validated.errors);
    assert!(validated.artifact.is_some());
}

#[test]
fn the_libc_bytecode_module_is_not_importable() {
    for source in [
        "import * as bjson from 'qjs:bjson';\nexport default () => 1;",
        "export default async () => (await import('qjs:bjson')).read;",
    ] {
        let (kind, message, ..) = failure_of(run_bounded(execute_input(source)));
        assert_eq!(kind, ScriptErrorKind::Runtime, "{source}");
        assert!(
            message.starts_with("cannot import 'qjs:bjson'"),
            "{source}: {message}"
        );
    }
}

#[test]
fn a_path_or_a_url_is_still_refused_while_validating() {
    for name in ["/etc/passwd", "https://example.com/x.js", ".hidden"] {
        let validated = validate_with(
            &format!("import x from '{name}';\nexport default () => x;"),
            "script",
            true,
            false,
        );
        assert!(!validated.valid, "{name}");
        assert!(validated.artifact.is_none(), "{name}");
        assert!(
            validated.errors[0]
                .message
                .contains(&format!("cannot import '{name}'")),
            "{name}: {:?}",
            validated.errors
        );
    }
}

#[test]
fn validate_refuses_what_a_run_refuses() {
    for (source, expected) in [
        ("export default function handle(", ScriptErrorKind::Syntax),
        ("export function other() {}", ScriptErrorKind::MissingHandle),
        (
            "while (true) {}\nexport default () => 1;",
            ScriptErrorKind::Timeout,
        ),
        (
            "throw new Error('at load');\nexport default () => 1;",
            ScriptErrorKind::Runtime,
        ),
        (
            "import x from '/y.js';\nexport default () => 1;",
            ScriptErrorKind::Runtime,
        ),
        (
            "await new Promise(() => {});\nexport default () => 1;",
            ScriptErrorKind::Runtime,
        ),
    ] {
        let validated = validate_source(&validate_input(source)).unwrap();
        assert!(!validated.valid, "{source}");
        assert_eq!(
            validated.errors[0].kind, expected,
            "{source}: {:?}",
            validated.errors
        );
    }
    assert!(
        validate_source(&validate_input("export default () => ({})"))
            .unwrap()
            .valid
    );
}

/// Validation stubs every import, so a script that builds and awaits a chain
/// at file scope, by default or named import, is not refused for want of an
/// instance.
#[test]
fn validate_accepts_a_script_that_imports_libraries_any_way() {
    let validated = validate_source(&validate_input(
        "import db from 'happyview.db';
         import { records, get as fetch, delete as remove } from 'happyview.db';
         import log, { info } from 'internal.logging';
         import * as http from 'happyview.http';
         const warm = await db.records('app.test.rec').where('a', 1).limit(5).run();
         info('loaded', { warm });
         log.info('again');
         export default async function handle(input, ctx) {
           return records(ctx.collection).limit(input.limit).run();
         }",
    ))
    .unwrap();
    assert!(validated.valid, "{:?}", validated.errors);
}

#[test]
fn validate_places_a_file_scope_error_on_its_line() {
    let validated = validate_source(&validate_input(
        "import db from 'happyview.db';\nconst x = undefined;\nx.y;\nexport default () => 1;",
    ))
    .unwrap();
    assert_eq!(validated.errors[0].kind, ScriptErrorKind::Runtime);
    assert_eq!(validated.errors[0].line, Some(3), "{:?}", validated.errors);
}

/// A v2 global's name is an ordinary unknown identifier in JavaScript: the
/// guard list is the Lua plugin's concern.
#[test]
fn removed_globals_are_not_read() {
    let mut input = execute_input("export default () => typeof db;");
    input.removed_globals = vec!["db".to_string()];
    assert_eq!(
        returned(execute_script(&input).unwrap()).0,
        json!("undefined")
    );
}

// --- the front end --------------------------------------------------------

/// A front end that refuses everything, as a language's compiler refuses a
/// script it cannot turn into JavaScript.
struct Refusing;

impl Frontend for Refusing {
    fn prepare(&self, _: &str) -> Result<Prepared, Vec<ValidateError>> {
        Err(vec![
            ValidateError {
                kind: ScriptErrorKind::Syntax,
                line: Some(3),
                message: "first".to_string(),
            },
            ValidateError {
                kind: ScriptErrorKind::Syntax,
                line: None,
                message: "second".to_string(),
            },
        ])
    }
}

/// What a front end refuses never reaches QuickJS: validation answers every
/// refusal, and a run fails as the first one, with all of them in its log.
#[test]
fn a_front_end_refusal_is_the_scripts_failure_in_both_exports() {
    let validated = validate(&Refusing, &validate_input("anything")).unwrap();
    assert!(!validated.valid);
    assert_eq!(validated.errors.len(), 2);
    assert_eq!(validated.errors[0].line, Some(3));

    let output = execute(&Refusing, &execute_input("anything")).unwrap();
    assert_eq!(
        output,
        ExecuteOutput::Error {
            kind: ScriptErrorKind::Syntax,
            message: "first".to_string(),
            line: Some(3),
            raw: "line 3: first\nsecond".to_string(),
            chunk: None,
        }
    );
}

/// A front end that moves every line down two, as a compiler that emits a
/// prelude does, and says so in its map.
struct Shifting;

impl Frontend for Shifting {
    fn prepare(&self, source: &str) -> Result<Prepared, Vec<ValidateError>> {
        let lines = source.lines().count() as u32;
        let mappings = (1..=lines)
            .map(|line| Mapping {
                generated: Position {
                    line: line + 2,
                    column: 1,
                },
                original: Position { line, column: 1 },
            })
            .collect();
        Ok(Prepared {
            code: format!("\n\n{source}"),
            map: Some(SourceMap::new(mappings)),
            source_map: None,
        })
    }
}

#[test]
fn every_position_an_error_reports_is_the_authors() {
    let output = execute(
        &Shifting,
        &execute_input(
            "export default function handle() {\n  const t = undefined;\n  return t.x;\n}",
        ),
    )
    .unwrap();
    // A dashboard script's chunk is the default name, which a failure
    // leaves out rather than naming a file the author never saw.
    assert!(
        matches!(&output, ExecuteOutput::Error { chunk: None, .. }),
        "{output:?}"
    );
    let (kind, _, line, raw) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(line, Some(3), "{raw}");
    assert!(raw.contains("at handle (script:3:1)"), "{raw}");

    let (kind, _, line, raw) =
        failure_of(execute(&Shifting, &execute_input("const a = 1;\nconst = 2;\n")).unwrap());
    assert_eq!(kind, ScriptErrorKind::Syntax);
    assert_eq!(line, Some(2), "{raw}");
    assert!(raw.contains("script:2:"), "{raw}");

    let validated = validate(
        &Shifting,
        &validate_input("const x = undefined;\nx.y;\nexport default () => 1;"),
    )
    .unwrap();
    assert_eq!(validated.errors[0].line, Some(2), "{:?}", validated.errors);
}

#[test]
fn an_error_names_the_file_it_is_in() {
    let mut input = execute_input(
        "export default function handle() {\n  const t = undefined;\n  return t.x;\n}",
    );
    input.chunk_name = "src/lexicons/app/test/q.js".to_string();
    match run_bounded(input) {
        ExecuteOutput::Error {
            chunk, line, raw, ..
        } => {
            assert_eq!(
                chunk.as_deref(),
                Some("src/lexicons/app/test/q.js"),
                "{raw}"
            );
            assert_eq!(line, Some(3), "{raw}");
            assert!(
                raw.contains("at handle (src/lexicons/app/test/q.js:3:"),
                "{raw}"
            );
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn validate_reports_the_literal_relative_imports() {
    let validated = validate_with(
        "import { format } from './lib/format.js';\nimport json from 'internal.json';\nexport default async () => (await import('../late.js')).x;",
        "script",
        false,
        false,
    );
    assert!(validated.valid, "{:?}", validated.errors);
    let found: Vec<(String, u32)> = validated
        .imports
        .into_iter()
        .map(|import| (import.specifier, import.line))
        .collect();
    assert_eq!(
        found,
        vec![("./lib/format.js".into(), 1), ("../late.js".into(), 3)]
    );
}

// --- prepared artifacts ---------------------------------------------------

fn prepared_artifact(source: &str, chunk_name: &str) -> happyview_plugin_sdk::PreparedArtifact {
    let validated = validate_with(source, chunk_name, true, false);
    assert!(validated.valid, "{:?}", validated.errors);
    validated.artifact.expect("an artifact")
}

#[test]
fn a_run_from_the_artifact_needs_no_source_and_keeps_its_positions() {
    let name = "src/jobs/x.js";
    let artifact = prepared_artifact(
        "import json from 'internal.json';\nexport default function handle(input) {\n  if (input.fail) throw new Error('boom');\n  return { encoded: json.encode({ a: 1 }) };\n}\n",
        name,
    );
    let mut sent = input("", json!({ "chunk_name": name }));
    sent.artifact = Some(artifact);
    assert_eq!(
        returned(execute_script(&sent).unwrap()).0,
        json!({ "encoded": r#"{"a":1}"# })
    );

    sent.input = json!({ "fail": true });
    match execute_script(&sent).unwrap() {
        ExecuteOutput::Error {
            chunk,
            line,
            message,
            ..
        } => {
            assert_eq!(chunk.as_deref(), Some(name));
            assert_eq!(line, Some(3));
            assert_eq!(message, "boom");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn an_artifact_this_build_cannot_load_falls_back_to_the_source_or_fails_by_name() {
    let mut artifact = prepared_artifact("export default () => 'from bytecode';", "script");
    artifact.engine = "quickjs-ng-0.10.0".to_string();
    let mut sent = execute_input("export default () => 'from source';");
    sent.artifact = Some(artifact);
    assert_eq!(
        returned(execute_script(&sent).unwrap()).0,
        json!("from source")
    );

    sent.source = String::new();
    let refused = execute_script(&sent).unwrap_err();
    assert_eq!(refused.code, "INTERPRETER_ERROR");
    assert!(
        refused.message.contains("quickjs-ng-0.10.0"),
        "{}",
        refused.message
    );
}

/// JavaScript's front end reads no map, so an artifact carrying one is an
/// artifact whose map cannot be read.
#[test]
fn an_artifact_whose_map_cannot_be_read_falls_back_to_the_source_or_fails() {
    let mut artifact = prepared_artifact("export default () => 'from bytecode';", "script");
    artifact.source_map = Some("{}".to_string());
    let mut sent = execute_input("export default () => 'from source';");
    sent.artifact = Some(artifact.clone());
    assert_eq!(
        returned(execute_script(&sent).unwrap()).0,
        json!("from source")
    );

    sent.source = String::new();
    let refused = execute_script(&sent).unwrap_err();
    assert_eq!(refused.code, "INTERPRETER_ERROR");
    assert!(
        refused.message.contains("source map"),
        "{}",
        refused.message
    );

    let id = "src/lexicons/app/test/m.js";
    let (output, _) = run_project(
        fake().module_artifact(id, "export default 'source';", artifact.clone()),
        "import m from './m.js';\nexport default () => m;",
    );
    assert_eq!(returned(output).0, json!("source"));
    let (output, _) = run_project(
        fake().module_artifact(id, "", artifact),
        "import m from './m.js';\nexport default () => m;",
    );
    let (_, message, ..) = failure_of(output);
    assert!(
        message.contains(id) && message.contains("source map"),
        "{message}"
    );
}

#[test]
fn an_artifact_compiled_under_another_name_is_refused() {
    let artifact = prepared_artifact("export default () => 1;", "src/a.js");
    let mut sent = input("", json!({ "chunk_name": "src/b.js" }));
    sent.artifact = Some(artifact);
    let (kind, message, ..) = failure_of(execute_script(&sent).unwrap());
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(
        message,
        "the prepared artifact for 'src/b.js' was compiled as 'src/a.js'"
    );
}

#[test]
fn a_script_with_top_level_await_runs_from_its_artifact() {
    let artifact = prepared_artifact(
        "import db from 'happyview.db';\nconst first = await db.get('x');\nexport default () => first;",
        "script",
    );
    let (output, _) = run_with(
        echo(fake()),
        "",
        json!({ "artifact": serde_json::to_value(&artifact).unwrap() }),
    );
    assert_eq!(
        returned(output).0,
        json!({ "via": "happyview-db.get", "args": ["x"] })
    );
}

/// QuickJS does not verify bytecode, so bytes reach its reader only from an
/// artifact stamped by this build. Source text whose characters are this
/// build's bytecode is compiled as source, and fails as source does, rather
/// than being read.
#[test]
fn source_text_carrying_bytecode_is_compiled_never_read() {
    let artifact = prepared_artifact("export default () => 'from bytecode';", "script");
    let source: String = artifact.bytes.iter().map(|&byte| byte as char).collect();
    let (kind, message, ..) = failure_of(execute_script(&execute_input(&source)).unwrap());
    assert!(
        matches!(kind, ScriptErrorKind::Syntax | ScriptErrorKind::Runtime),
        "{kind:?}: {message}"
    );
}

/// Bytes that would fault QuickJS's reader are never handed to it when the
/// stamp is not this build's, whichever half of the stamp is wrong.
#[test]
fn an_artifact_whose_stamp_mismatches_is_never_read() {
    let garbage = vec![0xff; 64];
    let stamped = artifact::prepared(garbage.clone(), None);
    let mut other_format = stamped.clone();
    other_format.format = "lua-bytecode".to_string();
    let mut other_engine = stamped;
    other_engine.engine = "quickjs-ng-0.10.0".to_string();

    for (artifact, named) in [
        (other_format, "lua-bytecode"),
        (other_engine, "quickjs-ng-0.10.0"),
    ] {
        let mut sent = execute_input("export default () => 'from source';");
        sent.artifact = Some(artifact);
        assert_eq!(
            returned(execute_script(&sent).unwrap()).0,
            json!("from source")
        );

        sent.source = String::new();
        let refused = execute_script(&sent).unwrap_err();
        assert_eq!(refused.code, "INTERPRETER_ERROR");
        assert!(refused.message.contains(named), "{}", refused.message);
    }
}

// --- local modules ----------------------------------------------------------

const HANDLER: &str = "src/lexicons/app/test/q.js";

fn run_project(fake: Fake, source: &str) -> (ExecuteOutput, Rc<Fake>) {
    run_with(fake, source, json!({ "chunk_name": HANDLER }))
}

#[test]
fn a_script_imports_a_module_by_a_relative_path() {
    let (output, fake) = run_project(
        fake().module(
            "src/lexicons/app/test/lib/format.js",
            "export function shout(s) { return s.toUpperCase(); }\nexport default { shout };",
        ),
        "import { shout } from './lib/format.js';\nimport fmt from './lib/format';\nexport default () => [shout('hi'), fmt.shout === shout];",
    );
    assert_eq!(returned(output).0, json!(["HI", true]));
    assert_eq!(
        fake.resolved.borrow().as_slice(),
        &[
            ("./lib/format.js".to_string(), HANDLER.to_string()),
            ("./lib/format".to_string(), HANDLER.to_string()),
        ]
    );
}

#[test]
fn a_module_is_evaluated_once_and_asked_for_once_per_importer() {
    let (output, fake) = run_project(
        fake()
            .module(
                "src/lexicons/app/test/shared.js",
                "globalThis.loads = (globalThis.loads ?? 0) + 1;\nexport const id = {};",
            )
            .module(
                "src/lexicons/app/test/lib/a.js",
                "export { id } from '../shared.js';",
            )
            .module(
                "src/lexicons/app/test/lib/b.js",
                "export { id } from '../shared.js';",
            ),
        "import { id as a } from './lib/a.js';\nimport { id as b } from './lib/b.js';\nimport { id as c } from './shared.js';\n\
         export default () => ({ loads: globalThis.loads, same: a === b && b === c });",
    );
    assert_eq!(returned(output).0, json!({ "loads": 1, "same": true }));
    let resolved = fake.resolved.borrow();
    assert!(resolved.contains(&(
        "../shared.js".to_string(),
        "src/lexicons/app/test/lib/a.js".to_string()
    )));
    let unique: std::collections::BTreeSet<_> = resolved.iter().collect();
    assert_eq!(
        unique.len(),
        resolved.len(),
        "a pair was asked twice: {resolved:?}"
    );
}

/// QuickJS resolves a static import once per module, so only an `import()`
/// run twice reaches the resolver twice with the same pair.
#[test]
fn a_repeated_dynamic_import_asks_the_host_once() {
    let (output, fake) = run_project(
        fake().module("src/lexicons/app/test/v.js", "export default 7;"),
        "import v from './v.js';\n\
         export default async () => [v, (await import('./v.js')).default, (await import('./v.js')).default];",
    );
    assert_eq!(returned(output).0, json!([7, 7, 7]));
    let pair = ("./v.js".to_string(), HANDLER.to_string());
    let asked = fake
        .resolved
        .borrow()
        .iter()
        .filter(|asked| **asked == pair)
        .count();
    assert_eq!(asked, 1, "{:?}", fake.resolved.borrow());
}

/// The same specifier from two directories is two modules: each importer
/// resolves against its own location.
#[test]
fn one_specifier_from_two_importers_names_two_modules() {
    let (output, fake) = run_project(
        fake()
            .module("src/lexicons/app/test/a/util.js", "export default 'a';")
            .module("src/lexicons/app/test/b/util.js", "export default 'b';")
            .module(
                "src/lexicons/app/test/a/index.js",
                "export { default } from './util.js';",
            )
            .module(
                "src/lexicons/app/test/b/index.js",
                "export { default } from './util.js';",
            ),
        "import a from './a/index.js';\nimport b from './b/index.js';\nexport default () => [a, b];",
    );
    assert_eq!(returned(output).0, json!(["a", "b"]));
    let resolved = fake.resolved.borrow();
    for importer in ["a", "b"] {
        assert!(
            resolved.contains(&(
                "./util.js".to_string(),
                format!("src/lexicons/app/test/{importer}/index.js")
            )),
            "{resolved:?}"
        );
    }
}

#[test]
fn modules_that_import_each_other_work_as_es_modules_do() {
    let (output, _) = run_project(
        fake()
            .module(
                "src/lexicons/app/test/even.js",
                "import { odd } from './odd.js';\nexport const even = (n) => n === 0 || odd(n - 1);",
            )
            .module(
                "src/lexicons/app/test/odd.js",
                "import { even } from './even.js';\nexport const odd = (n) => n !== 0 && even(n - 1);",
            ),
        "import { even } from './even.js';\nexport default () => [even(4), even(3)];",
    );
    assert_eq!(returned(output).0, json!([true, false]));
}

#[test]
fn a_dynamic_import_resolves_against_the_module_it_is_written_in() {
    let (output, fake) = run_project(
        fake()
            .module(
                "src/lexicons/app/test/lib/late.js",
                "export const load = () => import('./value.js');",
            )
            .module("src/lexicons/app/test/lib/value.js", "export default 42;"),
        "import { load } from './lib/late.js';\nexport default async () => (await load()).default;",
    );
    assert_eq!(returned(output).0, json!(42));
    assert!(fake.resolved.borrow().contains(&(
        "./value.js".to_string(),
        "src/lexicons/app/test/lib/late.js".to_string()
    )));
}

#[test]
fn a_miss_fails_with_the_hosts_sentence() {
    let (output, _) = run_project(
        fake(),
        "import x from './nope.js';\nexport default () => x;",
    );
    let (kind, message, ..) = failure_of(output);
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(
        message,
        format!("no module './nope.js' from {HANDLER}: looked for src/lexicons/app/test/nope.js")
    );
}

#[test]
fn an_error_inside_a_module_names_the_module_and_its_line() {
    let (output, _) = run_project(
        fake().module(
            "src/lexicons/app/test/lib/format.js",
            "export function f(t) {\n  return t.x;\n}",
        ),
        "import { f } from './lib/format.js';\nexport default () => f(undefined);",
    );
    match output {
        ExecuteOutput::Error { chunk, line, .. } => {
            assert_eq!(
                chunk.as_deref(),
                Some("src/lexicons/app/test/lib/format.js")
            );
            assert_eq!(line, Some(2));
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_module_runs_from_its_artifact_and_falls_back_to_source_when_it_cannot() {
    let id = "src/lexicons/app/test/m.js";
    let mut artifact = validate_with("export default 'bytecode';", id, true, true)
        .artifact
        .expect("an artifact");
    let (output, _) = run_project(
        fake().module_artifact(id, "", artifact.clone()),
        "import m from './m.js';\nexport default () => m;",
    );
    assert_eq!(returned(output).0, json!("bytecode"));

    artifact.engine = "quickjs-ng-0.10.0".to_string();
    let (output, _) = run_project(
        fake().module_artifact(id, "export default 'source';", artifact.clone()),
        "import m from './m.js';\nexport default () => m;",
    );
    assert_eq!(returned(output).0, json!("source"));

    let (output, _) = run_project(
        fake().module_artifact(id, "", artifact),
        "import m from './m.js';\nexport default () => m;",
    );
    let (_, message, ..) = failure_of(output);
    assert!(message.contains("quickjs-ng-0.10.0"), "{message}");
}

#[test]
fn a_bare_name_still_means_a_builtin_or_a_library() {
    let (output, fake) = run_project(
        echo(fake()),
        "import json from 'internal.json';\nimport db from 'happyview.db';\nexport default async () => [json.encode({ a: 1 }), (await db.get('x')).via];",
    );
    assert_eq!(
        returned(output).0,
        json!([r#"{"a":1}"#, "happyview-db.get"])
    );
    assert!(fake.resolved.borrow().is_empty());
}

#[test]
fn a_module_that_failed_to_compile_fails_the_same_way_when_imported_again() {
    let (output, _) = run_project(
        fake().module("src/lexicons/app/test/broken.js", "export const = 1;"),
        "const attempt = () => import('./broken.js').then(() => 'loaded', (e) => String(e));\n\
         export default async () => [await attempt(), await attempt()];",
    );
    let value = returned(output).0;
    assert_eq!(value[0], value[1], "{value}");
    assert!(
        value[0].as_str().unwrap().starts_with("SyntaxError"),
        "{value}"
    );
}

#[test]
fn the_fake_records_test_requests() {
    let fake = Fake::new().answering(happyview_plugin_sdk::TestTriggerResult {
        status: 201,
        body: json!({"ok": true}),
    });
    let spec: happyview_plugin_sdk::TestTrigger =
        serde_json::from_value(json!({"kind": "query", "target": "a.b.c"})).unwrap();
    assert_eq!(fake.test_trigger(&spec).unwrap().status, 201);
    assert_eq!(fake.triggers.borrow().len(), 1);
    let refusing = Fake::new().refusing_triggers();
    assert_eq!(refusing.test_trigger(&spec).unwrap_err().code, "BAD_INPUT");
}

const TEST_FILE: &str = "src/lib/a.test.js";

fn test_file_input(source: &str) -> ExecuteInput {
    input(
        source,
        json!({
            "kind": "test",
            "input": null,
            "context": { "trigger": format!("test:{TEST_FILE}") },
            "limits": { "instructions": null, "memory_bytes": 67_108_864 },
            "chunk_name": TEST_FILE,
        }),
    )
}

fn run_test_file(fake: Fake, source: &str) -> (ExecuteOutput, Rc<Fake>) {
    let fake = Rc::new(fake);
    let output = execute_with(&JavaScript, &test_file_input(source), fake.clone())
        .expect("an output, not an envelope error");
    (output, fake)
}

fn statuses(
    fake: &Fake,
) -> Vec<(
    String,
    happyview_plugin_sdk::TestStatus,
    Option<String>,
    Option<u32>,
)> {
    fake.reports
        .borrow()
        .iter()
        .map(|r| (r.name.clone(), r.status, r.message.clone(), r.line))
        .collect()
}

#[test]
fn a_test_file_reports_every_test_by_its_full_name() {
    use happyview_plugin_sdk::TestStatus::*;
    let (output, fake) = run_test_file(
        Fake::new(),
        r#"import { describe, it } from "internal.test";
describe("format", () => {
  it("joins", () => {});
  describe("nested", () => {
    it("awaits", async () => { await Promise.resolve(1); });
  });
  it.skip("later");
});"#,
    );
    assert_eq!(
        returned(output),
        (
            json!({"passed": 2, "failed": 0, "skipped": 1}),
            ScriptValueKind::Object
        )
    );
    let got = statuses(&fake);
    assert_eq!(got[0].0, "format > joins");
    assert_eq!(got[1].0, "format > nested > awaits");
    assert_eq!((got[2].0.as_str(), got[2].1), ("format > later", Skip));
    assert!(fake.reports.borrow().iter().all(|r| r.path == TEST_FILE));
    assert_eq!(got[0].1, Pass);
}

#[test]
fn a_thrown_error_fails_its_test_at_its_line() {
    use happyview_plugin_sdk::TestStatus::*;
    let (_, fake) = run_test_file(
        Fake::new(),
        r#"import { it } from "internal.test";
it("breaks", () => {
  null.x;
});
it("runs anyway", () => {});"#,
    );
    let got = statuses(&fake);
    assert_eq!(got[0].1, Fail);
    assert_eq!(got[0].3, Some(3));
    assert!(
        got[0].2.as_deref().unwrap().contains("TypeError"),
        "{:?}",
        got[0].2
    );
    assert_eq!(fake.reports.borrow()[0].file.as_deref(), Some(TEST_FILE));
    assert_eq!(got[1].1, Pass);
}

#[test]
fn hooks_run_in_order_and_after_each_runs_after_a_failure() {
    use happyview_plugin_sdk::TestStatus::*;
    let (_, fake) = run_test_file(
        Fake::new(),
        r#"import { describe, it, beforeEach, afterEach } from "internal.test";
const seen = [];
describe("hooks", () => {
  beforeEach(() => { seen.push("before"); });
  afterEach(async () => { seen.push("after"); });
  it("first", () => { seen.push("first"); throw new Error("boom"); });
  it("second", () => {
    if (seen.join(",") !== "before,first,after,before") throw new Error(seen.join(","));
  });
});"#,
    );
    let got = statuses(&fake);
    assert_eq!((got[0].1, got[0].2.as_deref()), (Fail, Some("boom")));
    assert_eq!(got[1].1, Pass, "{:?}", got[1].2);
}

#[test]
fn internal_test_is_refused_outside_a_test_file() {
    let (kind, message, _, _) = failure_of(
        execute_script(&execute_input(
            r#"import { it } from "internal.test"; export default () => 1;"#,
        ))
        .unwrap(),
    );
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert!(message.contains("only in test files"), "{message}");
}

#[test]
fn a_test_file_validates_without_a_default_export() {
    let mut input = validate_input(r#"import { it } from "internal.test"; it("x", () => {});"#);
    input.role = ChunkRole::Test;
    input.chunk_name = Some(TEST_FILE.to_string());
    assert!(validate_source(&input).unwrap().valid);
    input.role = ChunkRole::Handler;
    assert!(!validate_source(&input).unwrap().valid);
}

#[test]
fn a_file_that_never_imports_internal_test_reports_nothing() {
    let (output, fake) = run_test_file(Fake::new(), "const x = 1;");
    assert_eq!(
        returned(output).0,
        json!({"passed": 0, "failed": 0, "skipped": 0})
    );
    assert!(fake.reports.borrow().is_empty());
}

/// A module is unit-tested directly: the test file imports it by relative
/// path, as a handler would, and a failure inside the module is placed there.
#[test]
fn a_test_file_imports_a_module_and_a_failure_inside_it_is_placed_there() {
    use happyview_plugin_sdk::TestStatus::*;
    let (_, fake) = run_test_file(
        Fake::new().module(
            "src/lib/format.js",
            "export const join = (a, b) => `${a}-${b}`;\nexport const broken = () => {\n  null.x;\n};\n",
        ),
        r#"import { it } from "internal.test";
import { join, broken } from "./format.js";
it("joins", () => { if (join("a", "b") !== "a-b") throw new Error("joined wrong"); });
it("breaks inside the module", () => { broken(); });"#,
    );
    let reports = fake.reports.borrow();
    assert_eq!(reports[0].status, Pass, "{:?}", reports[0].message);
    assert_eq!(reports[1].status, Fail);
    assert_eq!(reports[1].file.as_deref(), Some("src/lib/format.js"));
    assert_eq!(reports[1].line, Some(3));
}
