//! The conformance corpus's harness: hand translations of the Lua plugin's
//! corpus scripts, one or more per trigger, run through both entry points.
//!
//! Each language keeps its own translation of the corpus under its
//! `tests/corpus/` and runs it through this, from a unit test in its own
//! crate: an integration test would have to link the plugin, and the `rlib`
//! that needs would cost the shipped module what LTO can no longer
//! internalise. The cases are named by stem, so one table pins what every
//! language's translation of a script must answer.
//!
//! Where the Lua corpus can only be validated — answering a library call
//! needs a host — this one is also *executed*, against a host that answers
//! each call the way the standard library would in shape, so every script is
//! covered past its first call and the calls it makes are pinned.

use std::path::PathBuf;
use std::rc::Rc;

use happyview_plugin_sdk::{
    ApiExport, ApiMethod, ApiSurface, ExecuteInput, ExecuteOutput, PluginError, ScriptErrorKind,
    ScriptValueKind, ValidateInput, Value as Json,
};
use serde_json::json;

use crate::backend::fake::Fake;
use crate::Frontend;

/// One language's translation of the corpus.
pub struct Corpus<'a> {
    pub frontend: &'a dyn Frontend,
    /// The directory holding the scripts, `tests/corpus` in the language's
    /// crate.
    pub dir: PathBuf,
    /// The language's file extension, without the dot.
    pub extension: &'static str,
}

impl Corpus<'_> {
    fn read(&self, stem: &str) -> String {
        let path = self.dir.join(format!("{stem}.{}", self.extension));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Every script's stem, sorted.
    fn stems(&self) -> Vec<String> {
        let suffix = format!(".{}", self.extension);
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .unwrap_or_else(|e| panic!("{}: {e}", self.dir.display()))
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter_map(|name| name.strip_suffix(&suffix).map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn execute(
        &self,
        stem: &str,
        kind: &str,
        input: Json,
        context: Json,
    ) -> (ExecuteOutput, Rc<Fake>) {
        let input: ExecuteInput = serde_json::from_value(json!({
            "source": self.read(stem),
            "kind": kind,
            "input": input,
            "context": context,
            "libraries": libraries(),
            "limits": { "instructions": 1_000_000, "memory_bytes": 67_108_864 },
        }))
        .unwrap();
        let fake = Rc::new(host());
        let output = crate::execute_with(self.frontend, &input, fake.clone())
            .unwrap_or_else(|e| panic!("{stem}: {e:?}"));
        (output, fake)
    }

    pub fn every_script_validates_except_the_one_with_no_handle(&self) {
        let stems = self.stems();
        assert_eq!(stems.len(), 16, "the corpus is 16 files: {stems:?}");
        let mut refused = Vec::new();
        for stem in &stems {
            let validated = crate::validate(
                self.frontend,
                &ValidateInput {
                    source: self.read(stem),
                    removed_globals: Vec::new(),
                },
            )
            .expect("validate should answer an output rather than an envelope error");
            if !validated.valid {
                let error = &validated.errors[0];
                refused.push((stem.clone(), error.kind, error.message.clone()));
            }
        }
        assert_eq!(
            refused,
            vec![(
                "query.marker_no_handle".to_string(),
                ScriptErrorKind::MissingHandle,
                crate::MISSING_HANDLE.to_string()
            )]
        );
    }

    pub fn every_script_runs_against_a_host_and_answers_what_it_should(&self) {
        let cases = cases();
        // Every file is run at least once, so a script added to the corpus
        // without a case fails here.
        for stem in self.stems() {
            assert!(
                cases.iter().any(|case| case.file == stem),
                "{stem} has no case"
            );
        }

        for case in cases {
            let (output, fake) = self.execute(case.file, case.kind, case.input, case.context);
            match (case.expect, output) {
                (Expect::Value(value, kind), output) => assert_eq!(
                    output,
                    ExecuteOutput::Returned {
                        value,
                        value_kind: kind
                    },
                    "{}",
                    case.file
                ),
                (Expect::Rkey, ExecuteOutput::Returned { value, .. }) => {
                    let rkey = value["rkey"].as_str().unwrap_or_default();
                    assert_eq!(rkey.len(), 13, "{}: {value}", case.file);
                }
                (Expect::Refused(kind), ExecuteOutput::Error { kind: got, .. }) => {
                    assert_eq!(got, kind, "{}", case.file)
                }
                (_, output) => panic!("{}: {output:?}", case.file),
            }

            let calls: Vec<Json> = fake
                .calls
                .borrow()
                .iter()
                .map(|(library, function, _)| json!([library, function]))
                .collect();
            assert_eq!(Json::Array(calls), case.calls, "{}", case.file);
        }
    }

    /// The document a chain's immediate method sends, which no answer above
    /// shows.
    pub fn a_chained_call_sends_the_lua_bridges_document(&self) {
        let (_, fake) = self.execute(
            "procedure.spaces_create_and_write",
            "xrpc_procedure",
            json!({ "skey": "k", "text": "hi" }),
            json!({ "trigger": "xrpc.procedure:app.example.chat" }),
        );
        assert_eq!(
            fake.calls.borrow()[1].2,
            vec![json!({
                "args": ["ats://did:plc:space/chat/k"],
                "steps": [],
                "call": {
                    "name": "write_record",
                    "args": [{ "collection": "app.example.message", "record": { "text": "hi" } }],
                },
            })]
        );
    }
}

fn constructor(name: &str, lazy: &[&str], immediate: &[&str]) -> ApiExport {
    let mut export = ApiExport::constructor(name);
    export.methods = lazy
        .iter()
        .map(|method| ApiMethod::lazy(*method))
        .chain(immediate.iter().map(|method| ApiMethod::immediate(*method)))
        .collect();
    export
}

fn functions(namespace: &str, names: &[&str]) -> ApiSurface {
    names
        .iter()
        .fold(ApiSurface::new(namespace), |surface, name| {
            surface.export(ApiExport::function(*name))
        })
}

fn libraries() -> Json {
    json!([
        { "namespace": "happyview.db", "id": "happyview-db" },
        { "namespace": "happyview.record", "id": "happyview-record" },
        { "namespace": "happyview.http", "id": "happyview-http" },
        { "namespace": "happyview.jobs", "id": "happyview-jobs" },
        { "namespace": "happyview.atproto", "id": "happyview-atproto" },
        { "namespace": "happyview.spaces", "id": "happyview-spaces" },
        { "namespace": "happyview.linked_repos", "id": "happyview-linked-repos" },
    ])
}

/// The standard libraries' surfaces, as much of each as the corpus
/// reaches.
fn host() -> Fake {
    Fake::new()
        .surface(
            "happyview-db",
            functions("happyview.db", &["get", "search"]).export(constructor(
                "records",
                &["did", "limit", "cursor", "where"],
                &["run", "count"],
            )),
        )
        .surface(
            "happyview-record",
            functions("happyview.record", &["create"]),
        )
        .surface(
            "happyview-http",
            functions("happyview.http", &["get", "post"]),
        )
        .surface("happyview-jobs", functions("happyview.jobs", &["create"]))
        .surface(
            "happyview-atproto",
            functions(
                "happyview.atproto",
                &[
                    "resolve_service_endpoint",
                    "get_labels",
                    "sign",
                    "verify_signature",
                ],
            ),
        )
        .surface(
            "happyview-spaces",
            functions("happyview.spaces", &["create"]).export(constructor(
                "get",
                &[],
                &["write_record"],
            )),
        )
        .surface(
            "happyview-linked-repos",
            functions("happyview.linked_repos", &["list"]).export(constructor(
                "get",
                &[],
                &["create_record"],
            )),
        )
        .respond(answer)
}

/// What each call answers: the standard library's shape where a script
/// reads a field of it, and the call itself echoed everywhere else, so
/// the expected output pins what was sent.
fn answer(library: &str, function: &str, args: &[Json]) -> Result<Json, PluginError> {
    Ok(match (library, function) {
        ("happyview-db", "get") if args[0] == json!("at://missing") => Json::Null,
        ("happyview-db", "get") => json!({
            "uri": args[0], "cid": "bafyrecord", "record": { "text": "hello" },
        }),
        ("happyview-record", "create") => {
            json!({ "uri": "at://did:plc:me/c/1", "cid": "bafyc" })
        }
        ("happyview-http", "get") => json!({ "status": 200, "body": "data" }),
        ("happyview-spaces", "create") => json!({ "uri": "ats://did:plc:space/chat/k" }),
        ("happyview-jobs", "create") => json!("job-1"),
        // True only for the signature `sign` echoed, so the check is
        // seen to receive what the lookup before it settled to.
        ("happyview-atproto", "verify_signature") => {
            json!(args[1] == json!({ "via": "sign", "args": [{ "ok": true }] }))
        }
        _ => json!({ "via": function, "args": args }),
    })
}

enum Expect {
    Value(Json, ScriptValueKind),
    /// A fresh TID under `rkey`, which no fixed value can match.
    Rkey,
    Refused(ScriptErrorKind),
}

struct Case {
    file: &'static str,
    kind: &'static str,
    input: Json,
    context: Json,
    expect: Expect,
    /// Every call the run made, as `[library, function]`, in order.
    calls: Json,
}

fn query(trigger: &str) -> Json {
    json!({ "trigger": trigger, "caller_did": "did:plc:me", "collection": "app.example.post" })
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            file: "query",
            kind: "xrpc_query",
            input: json!({ "did": "did:plc:a", "limit": "5" }),
            context: query("xrpc.query:app.example.list"),
            expect: Expect::Value(
                json!({ "via": "records", "args": [{
                    "args": ["app.example.post"],
                    "steps": [{ "did": ["did:plc:a"] }, { "limit": [5] }, { "cursor": [] }],
                    "call": { "name": "run", "args": [] },
                }] }),
                ScriptValueKind::Object,
            ),
            calls: json!([["happyview-db", "records"]]),
        },
        Case {
            file: "query",
            kind: "xrpc_query",
            input: json!({ "uri": "at://missing" }),
            context: query("xrpc.query:app.example.get"),
            expect: Expect::Value(
                json!({ "error": "NotFound", "message": "no record at at://missing" }),
                ScriptValueKind::Object,
            ),
            calls: json!([["happyview-db", "get"]]),
        },
        Case {
            file: "procedure",
            kind: "xrpc_procedure",
            input: json!({ "text": "hi" }),
            context: json!({
                "trigger": "xrpc.procedure:app.example.create",
                "caller_did": "did:plc:me",
            }),
            expect: Expect::Value(
                json!({ "uri": "at://did:plc:me/c/1", "cid": "bafyc" }),
                ScriptValueKind::Object,
            ),
            calls: json!([["happyview-record", "create"]]),
        },
        Case {
            file: "job",
            kind: "job",
            input: json!({}),
            context: json!({ "trigger": "job.run:reindex", "job": { "id": "j1" } }),
            expect: Expect::Value(json!({ "done": true }), ScriptValueKind::Object),
            calls: json!([]),
        },
        Case {
            file: "trigger",
            kind: "record_event",
            input: json!({ "action": "create", "uri": "at://x" }),
            context: json!({ "trigger": "record.created:app.example.post" }),
            expect: Expect::Value(
                json!({ "action": "create", "uri": "at://x" }),
                ScriptValueKind::Object,
            ),
            calls: json!([]),
        },
        Case {
            file: "record-event",
            kind: "record_event",
            input: json!({ "action": "delete", "uri": "at://x", "did": "did:plc:a" }),
            context: json!({ "trigger": "record.deleted:app.example.post" }),
            expect: Expect::Value(json!(true), ScriptValueKind::Other),
            calls: json!([]),
        },
        Case {
            file: "record.template_index_hook",
            kind: "record_event",
            input: json!({ "action": "update", "uri": "at://x" }),
            context: json!({ "trigger": "record.updated:app.example.post" }),
            expect: Expect::Value(Json::Null, ScriptValueKind::None),
            calls: json!([]),
        },
        Case {
            file: "label.event_to_input",
            kind: "label",
            input: json!({ "src": "did:plc:l", "uri": "at://x", "val": "spam", "neg": true }),
            context: json!({ "trigger": "label.created" }),
            expect: Expect::Value(Json::Null, ScriptValueKind::None),
            calls: json!([]),
        },
        Case {
            file: "label.event_to_input",
            kind: "label",
            input: json!({ "src": "did:plc:l", "uri": "at://x", "val": "spam", "neg": false }),
            context: json!({ "trigger": "label.created" }),
            expect: Expect::Value(
                json!({
                    "src": "did:plc:l", "uri": "at://x", "val": "spam",
                    "raw": { "src": "did:plc:l", "uri": "at://x", "val": "spam", "neg": false },
                }),
                ScriptValueKind::Object,
            ),
            calls: json!([]),
        },
        Case {
            file: "query.http_calls",
            kind: "xrpc_query",
            input: json!({}),
            context: query("xrpc.query:app.example.fetch"),
            expect: Expect::Value(json!({ "status": 200 }), ScriptValueKind::Object),
            calls: json!([["happyview-http", "get"], ["happyview-http", "post"]]),
        },
        Case {
            file: "query.json_module",
            kind: "xrpc_query",
            input: json!({}),
            context: query("xrpc.query:app.example.json"),
            expect: Expect::Value(json!({ "ok": true }), ScriptValueKind::Object),
            calls: json!([]),
        },
        Case {
            file: "procedure.tid_create",
            kind: "xrpc_procedure",
            input: json!({}),
            context: json!({ "trigger": "xrpc.procedure:app.example.mint" }),
            expect: Expect::Rkey,
            calls: json!([]),
        },
        Case {
            file: "procedure.jobs_create",
            kind: "xrpc_procedure",
            input: json!({}),
            context: json!({
                "trigger": "xrpc.procedure:app.example.reindex",
                "collection": "app.example.post",
            }),
            expect: Expect::Value(json!({ "job_id": "job-1" }), ScriptValueKind::Object),
            calls: json!([["happyview-jobs", "create"]]),
        },
        Case {
            file: "query.atproto_functions",
            kind: "xrpc_query",
            input: json!({ "uri": "at://x" }),
            context: query("xrpc.query:app.example.labels"),
            expect: Expect::Value(
                json!({
                    "pds": { "via": "resolve_service_endpoint", "args": ["did:plc:me"] },
                    "labels": { "via": "get_labels", "args": ["at://x"] },
                    "ok": true,
                }),
                ScriptValueKind::Object,
            ),
            calls: json!([
                ["happyview-atproto", "resolve_service_endpoint"],
                ["happyview-atproto", "get_labels"],
                ["happyview-atproto", "sign"],
                ["happyview-atproto", "verify_signature"],
            ]),
        },
        Case {
            file: "procedure.spaces_create_and_write",
            kind: "xrpc_procedure",
            input: json!({ "skey": "k", "text": "hi" }),
            context: json!({ "trigger": "xrpc.procedure:app.example.chat" }),
            expect: Expect::Value(
                json!({ "uri": "ats://did:plc:space/chat/k" }),
                ScriptValueKind::Object,
            ),
            calls: json!([["happyview-spaces", "create"], ["happyview-spaces", "get"]]),
        },
        Case {
            file: "query.linked_repos_calls",
            kind: "xrpc_query",
            input: json!({ "did": "did:plc:other" }),
            context: query("xrpc.query:app.example.repos"),
            expect: Expect::Value(
                json!({ "grants": { "via": "list", "args": [] } }),
                ScriptValueKind::Object,
            ),
            calls: json!([
                ["happyview-linked-repos", "list"],
                ["happyview-linked-repos", "get"],
            ]),
        },
        Case {
            file: "job.job_input_and_controls",
            kind: "job",
            input: json!({ "collection": "app.example.post" }),
            context: json!({ "trigger": "job.run:reindex", "job": { "id": "j2" } }),
            expect: Expect::Value(
                json!({ "id": "j2", "collection": "app.example.post" }),
                ScriptValueKind::Object,
            ),
            calls: json!([]),
        },
        Case {
            file: "query.marker_no_handle",
            kind: "xrpc_query",
            input: json!({}),
            context: query("xrpc.query:app.example.none"),
            expect: Expect::Refused(ScriptErrorKind::MissingHandle),
            calls: json!([]),
        },
    ]
}
