//! The two exports end to end: the contract, the event loop against a host
//! that settles calls in the order a test picks, the budgets, imports, and
//! the error contract the host reads.

use super::*;
use crate::backend::fake::{Fake, Order};
use happyview_plugin_sdk::{ApiExport, ApiMethod, ApiSurface};
use serde_json::json;

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
    let output = execute_with(&input(source, fields), fake.clone())
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
    }
    let input = input(
        "import db from 'happyview.db';\nexport default () => db.get('x');",
        libraries(),
    );
    let error = execute_with(&input, Rc::new(Refusing(fake()))).unwrap_err();
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
             libraries by name, never a path or a URL"
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
            "import x from './helper.js';\nexport default () => 1;".to_string(),
            path_refusal("./helper.js"),
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
            "import x from './y.js';\nexport default () => 1;",
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
