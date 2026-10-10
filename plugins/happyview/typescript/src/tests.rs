//! TypeScript through both exports: each transform runs, each refusal is a
//! validate error on its line, and every position an error reports is in
//! the source the author wrote. What a script does once it runs is the
//! engine's, tested there.

use std::rc::Rc;

use happyview_plugin_sdk::{
    ApiExport, ApiSurface, ExecuteInput, ExecuteOutput, ScriptErrorKind, ScriptValueKind,
    ValidateError, ValidateInput, ValidateOutput,
};
use happyview_quickjs::fake::Fake;
use serde_json::json;

use super::*;

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

fn run(source: &str) -> ExecuteOutput {
    execute_script(&input(source, json!({}))).expect("an output, not an envelope error")
}

fn validated(source: &str) -> ValidateOutput {
    validate_source(&ValidateInput {
        source: source.to_string(),
        removed_globals: Vec::new(),
    })
    .expect("an output, not an envelope error")
}

fn returned(output: ExecuteOutput) -> serde_json::Value {
    match output {
        ExecuteOutput::Returned { value, .. } => value,
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

/// The one error a refused script is refused with, which both exports agree
/// on.
fn refused(source: &str) -> ValidateError {
    let output = validated(source);
    assert!(!output.valid, "{source}");
    assert_eq!(output.errors.len(), 1, "{source}: {:?}", output.errors);
    let error = output.errors[0].clone();

    let (kind, message, line, _) = failure_of(run(source));
    assert_eq!(
        (kind, message.as_str(), line),
        (error.kind, error.message.as_str(), error.line),
        "{source}"
    );
    error
}

// --- what the transform runs -------------------------------------------------

#[test]
fn an_enum_runs_with_its_reverse_mapping() {
    assert_eq!(
        returned(run(
            "enum Color { Red, Green = 5, Blue }
             export default (): unknown[] => [Color.Red, Color.Blue, Color[5], Color[Color.Blue]];"
        )),
        json!([0, 6, "Green", "Blue"])
    );
}

#[test]
fn a_string_enum_runs() {
    assert_eq!(
        returned(run(
            "enum Kind { Post = 'app.example.post', Like = 'app.example.like' }
             export default () => ({ post: Kind.Post, keys: Object.keys(Kind) });"
        )),
        json!({ "post": "app.example.post", "keys": ["Post", "Like"] })
    );
}

#[test]
fn a_namespace_with_values_runs() {
    assert_eq!(
        returned(run("namespace Util {
               export const twice = (n: number): number => n * 2;
               export namespace Inner { export const name = 'inner'; }
             }
             export default () => [Util.twice(21), Util.Inner.name];")),
        json!([42, "inner"])
    );
}

#[test]
fn parameter_properties_become_fields() {
    assert_eq!(
        returned(run(
            "class Point {
               constructor(public x: number, private readonly y: number = 2) {}
               sum(): number { return this.x + this.y; }
             }
             export default () => { const p = new Point(1); return [p.x, p.sum()]; };"
        )),
        json!([1, 3])
    );
}

#[test]
fn types_and_type_only_syntax_leave_nothing_behind() {
    assert_eq!(
        returned(run(
            "import type { Thing } from 'happyview.nowhere';
             interface Row { id: string; n?: number }
             type Pair<T> = [T, T];
             function pair<T>(value: T): Pair<T> { return [value, value]; }
             const config = { limit: 5, kinds: ['a', 'b'] } as const;
             const row = { id: 'x' } satisfies Row;
             declare const missing: Thing;
             abstract class Base { abstract name(): string; }
             function size(value: string): number;
             function size(value: unknown[]): number;
             function size(value: string | unknown[]): number { return value.length; }
             class Named extends Base { name(): string { return 'n'; } }
             export default function handle(input: Record<string, unknown>): unknown {
               const n = <number>(input.n ?? 1);
               return { pair: pair<number>(n), limit: config.limit, id: row.id!, name: new Named().name(), size: size('abc') };
             }"
        )),
        json!({ "pair": [1, 1], "limit": 5, "id": "x", "name": "n", "size": 3 })
    );
}

/// An import used only as a type is elided, so a library a script names only
/// for its types need not be installed.
#[test]
fn a_type_only_import_of_a_library_does_not_require_it() {
    for source in [
        "import type db from 'happyview.db';
         export default (): typeof db | null => null;",
        "import db from 'happyview.db';
         type Db = typeof db;
         export default (): Db | null => null;",
        "import { type records } from 'happyview.db';
         export default (): typeof records | null => null;",
    ] {
        assert_eq!(returned(run(source)), json!(null), "{source}");
    }
}

#[test]
fn library_imports_reach_the_bridge_through_annotated_code() {
    let fake = Rc::new(
        Fake::new()
            .surface(
                "happyview-db",
                ApiSurface::new("happyview.db").export(ApiExport::function("get")),
            )
            .respond(|library, function, args| {
                Ok(json!({ "via": format!("{library}.{function}"), "args": args }))
            }),
    );
    let input = input(
        "import db from 'happyview.db';
         import { get } from 'happyview.db';
         interface Answer { via: string; args: unknown[] }
         export default async function handle(input: { a: string }, ctx: any): Promise<Answer[]> {
           const results: Answer[] = await Promise.all([db.get(input.a), get('b')]);
           return results;
         }",
        json!({
            "input": { "a": "x" },
            "libraries": [{ "namespace": "happyview.db", "id": "happyview-db" }],
        }),
    );
    let output = happyview_quickjs::execute_with(&TypeScript, &input, fake.clone()).unwrap();
    assert_eq!(
        returned(output),
        json!([
            { "via": "happyview-db.get", "args": ["x"] },
            { "via": "happyview-db.get", "args": ["b"] },
        ])
    );
    assert_eq!(fake.calls.borrow().len(), 2);
}

#[test]
fn what_a_script_returned_carries_its_kind() {
    assert_eq!(
        run("export default async (): Promise<object> => ({ ok: true });"),
        ExecuteOutput::Returned {
            value: json!({ "ok": true }),
            value_kind: ScriptValueKind::Object,
        }
    );
}

// --- positions ---------------------------------------------------------------

/// An enum and a namespace each become several lines of JavaScript, so
/// everything after them is on a different line in the module QuickJS ran.
const AFTER_AN_ENUM: &str = "enum Color {
  Red,
  Green,
}
namespace Util {
  export const one = 1;
}
export default function handle(): number {
  const t = undefined as any;
  return t.x + Util.one + Color.Red;
}
";

#[test]
fn a_runtime_error_after_an_enum_is_placed_on_its_typescript_line() {
    let (kind, message, line, raw) = failure_of(run(AFTER_AN_ENUM));
    assert_eq!(kind, ScriptErrorKind::Runtime);
    assert_eq!(message, "TypeError: cannot read property 'x' of undefined");
    assert_eq!(line, Some(10), "{raw}");
    assert!(raw.contains("at handle (script:10:"), "{raw}");
}

#[test]
fn a_thrown_error_in_a_nested_call_is_placed_frame_by_frame() {
    let source = "enum E { A }
function inner(n: number): never {
  throw new Error(`boom ${n}`);
}
function outer(): never {
  return inner(E.A);
}
export default () => outer();
";
    let (_, message, line, raw) = failure_of(run(source));
    assert_eq!(message, "boom 0");
    assert_eq!(line, Some(3), "{raw}");
    assert!(raw.contains("at inner (script:3:"), "{raw}");
    assert!(raw.contains("at outer (script:6:"), "{raw}");
}

#[test]
fn a_file_scope_error_is_placed_on_its_line_by_validate() {
    let output =
        validated("enum E { A, B }\nconst x: any = undefined;\nx.y;\nexport default () => E.B;");
    assert!(!output.valid);
    assert_eq!(output.errors[0].kind, ScriptErrorKind::Runtime);
    assert_eq!(output.errors[0].line, Some(3), "{:?}", output.errors);
}

#[test]
fn a_typescript_syntax_error_carries_its_line() {
    let error = refused("const a: number = 1;\nconst b: = 2;\nexport default () => a;\n");
    assert_eq!(error.kind, ScriptErrorKind::Syntax);
    assert_eq!(error.line, Some(2), "{error:?}");
}

// --- refusals ----------------------------------------------------------------

#[test]
fn commonjs_forms_are_refused_on_their_lines() {
    let error = refused("import fs = require('fs');\nexport default () => 1;");
    assert_eq!(error.line, Some(1));
    assert!(error.message.contains("import x = require"), "{error:?}");

    let error = refused("const handle = () => 1;\n\nexport = handle;");
    assert_eq!(error.line, Some(3));
    assert!(error.message.contains("export ="), "{error:?}");

    // A type-only import assignment is a type, and an alias of a namespace
    // is plain TypeScript; neither is refused.
    assert_eq!(
        returned(run("import type T = require('x');
             namespace A { export namespace B { export const c = 3; } }
             import C = A.B;
             export default (): T | number => C.c;")),
        json!(3)
    );
}

#[test]
fn decorators_and_accessors_are_refused_on_their_lines() {
    let error = refused(
        "function d(value: any) { return value; }\n@d\nclass A {}\nexport default () => 1;",
    );
    assert_eq!(error.line, Some(2));
    assert!(
        error.message.starts_with("decorators are not supported"),
        "{error:?}"
    );

    let error = refused("class A {\n  accessor x = 1;\n}\nexport default () => new A().x;");
    assert_eq!(error.line, Some(2));
    assert!(error.message.starts_with("`accessor` fields"), "{error:?}");
}

/// TSX reads as TypeScript gone wrong — a stray `<`, an unterminated
/// regular expression — so it is recognised and refused by name.
#[test]
fn jsx_is_refused_by_name_on_its_line() {
    let error = refused("const x = 1;\nconst view = <div>{x}</div>;\nexport default () => view;");
    assert_eq!(error.kind, ScriptErrorKind::Syntax);
    assert_eq!(error.line, Some(2), "{error:?}");
    assert!(
        error.message.starts_with("JSX is not supported"),
        "{error:?}"
    );

    let error = refused("export default () => (\n  <>\n    <p />\n  </>\n);");
    assert_eq!(error.line, Some(2), "{error:?}");

    // A generic arrow function and a type assertion are not JSX.
    assert_eq!(
        returned(run("const id = <T,>(value: T): T => value;
             export default () => id(<number>(1 as unknown));")),
        json!(1)
    );
}

/// Every refusal is reported, in source order, not only the first.
#[test]
fn validate_reports_every_refusal() {
    let output = validated(
        "import a = require('a');\nfunction d(v: any) { return v; }\n@d class A {}\nexport = A;",
    );
    let lines: Vec<Option<u32>> = output.errors.iter().map(|error| error.line).collect();
    assert_eq!(
        lines,
        vec![Some(1), Some(3), Some(4)],
        "{:?}",
        output.errors
    );
}

// --- validate ----------------------------------------------------------------

#[test]
fn validate_accepts_a_script_that_imports_libraries_any_way() {
    let output = validated(
        "import db from 'happyview.db';
         import { records, get as fetch } from 'happyview.db';
         import log, { info } from 'internal.logging';
         import type { Anything } from 'happyview.http';
         enum Mode { Fast, Slow }
         const warm = await db.records('app.test.rec').where('a', 1).limit(5).run();
         info('loaded', { warm, mode: Mode.Fast });
         export default async function handle(input: { limit?: number }, ctx: { collection: string }) {
           return records(ctx.collection).limit(input.limit).run();
         }",
    );
    assert!(output.valid, "{:?}", output.errors);
}

#[test]
fn validate_refuses_a_script_with_no_default_function() {
    let output = validated("export const handle = (): number => 1;");
    assert_eq!(output.errors[0].kind, ScriptErrorKind::MissingHandle);
}
