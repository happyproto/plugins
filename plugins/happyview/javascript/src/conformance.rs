//! The conformance corpus: hand translations of the Lua plugin's corpus
//! scripts, one or more per trigger, run through both exports.
//!
//! They live in the crate rather than in `tests/` because an integration test
//! would have to link it, and the `rlib` that needs would cost the shipped
//! module what LTO can no longer internalise. The scripts themselves are
//! under `tests/corpus/`, with a provenance note saying what each was
//! translated from.
//!
//! Where the Lua corpus can only be validated — answering a library call
//! needs a host — this one is also *executed*, against a host that answers
//! each call the way the standard library would in shape, so every script is
//! covered past its first call and the calls it makes are pinned.

#[cfg(test)]
mod corpus {
    use std::rc::Rc;

    use happyview_plugin_sdk::{
        ApiExport, ApiMethod, ApiSurface, ExecuteInput, ExecuteOutput, PluginError,
        ScriptErrorKind, ScriptValueKind, ValidateInput, Value as Json,
    };
    use serde_json::json;

    use crate::backend::fake::Fake;

    fn read(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn corpus() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".js"))
            .collect();
        names.sort();
        names
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
                file: "query.js",
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
                file: "query.js",
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
                file: "procedure.js",
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
                file: "job.js",
                kind: "job",
                input: json!({}),
                context: json!({ "trigger": "job.run:reindex", "job": { "id": "j1" } }),
                expect: Expect::Value(json!({ "done": true }), ScriptValueKind::Object),
                calls: json!([]),
            },
            Case {
                file: "trigger.js",
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
                file: "record-event.js",
                kind: "record_event",
                input: json!({ "action": "delete", "uri": "at://x", "did": "did:plc:a" }),
                context: json!({ "trigger": "record.deleted:app.example.post" }),
                expect: Expect::Value(json!(true), ScriptValueKind::Other),
                calls: json!([]),
            },
            Case {
                file: "record.template_index_hook.js",
                kind: "record_event",
                input: json!({ "action": "update", "uri": "at://x" }),
                context: json!({ "trigger": "record.updated:app.example.post" }),
                expect: Expect::Value(Json::Null, ScriptValueKind::None),
                calls: json!([]),
            },
            Case {
                file: "label.event_to_input.js",
                kind: "label",
                input: json!({ "src": "did:plc:l", "uri": "at://x", "val": "spam", "neg": true }),
                context: json!({ "trigger": "label.created" }),
                expect: Expect::Value(Json::Null, ScriptValueKind::None),
                calls: json!([]),
            },
            Case {
                file: "label.event_to_input.js",
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
                file: "query.http_calls.js",
                kind: "xrpc_query",
                input: json!({}),
                context: query("xrpc.query:app.example.fetch"),
                expect: Expect::Value(json!({ "status": 200 }), ScriptValueKind::Object),
                calls: json!([["happyview-http", "get"], ["happyview-http", "post"]]),
            },
            Case {
                file: "query.json_module.js",
                kind: "xrpc_query",
                input: json!({}),
                context: query("xrpc.query:app.example.json"),
                expect: Expect::Value(json!({ "ok": true }), ScriptValueKind::Object),
                calls: json!([]),
            },
            Case {
                file: "procedure.tid_create.js",
                kind: "xrpc_procedure",
                input: json!({}),
                context: json!({ "trigger": "xrpc.procedure:app.example.mint" }),
                expect: Expect::Rkey,
                calls: json!([]),
            },
            Case {
                file: "procedure.jobs_create.js",
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
                file: "query.atproto_functions.js",
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
                file: "procedure.spaces_create_and_write.js",
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
                file: "query.linked_repos_calls.js",
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
                file: "job.job_input_and_controls.js",
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
                file: "query.marker_no_handle.js",
                kind: "xrpc_query",
                input: json!({}),
                context: query("xrpc.query:app.example.none"),
                expect: Expect::Refused(ScriptErrorKind::MissingHandle),
                calls: json!([]),
            },
        ]
    }

    fn execute(
        case_file: &str,
        kind: &str,
        input: Json,
        context: Json,
    ) -> (ExecuteOutput, Rc<Fake>) {
        let input: ExecuteInput = serde_json::from_value(json!({
            "source": read(case_file),
            "kind": kind,
            "input": input,
            "context": context,
            "libraries": libraries(),
            "limits": { "instructions": 1_000_000, "memory_bytes": 67_108_864 },
        }))
        .unwrap();
        let fake = Rc::new(host());
        let output = crate::execute_with(&input, fake.clone())
            .unwrap_or_else(|e| panic!("{case_file}: {e:?}"));
        (output, fake)
    }

    #[test]
    fn every_corpus_script_validates_except_the_one_with_no_handle() {
        let corpus = corpus();
        assert_eq!(corpus.len(), 16, "the corpus is 16 files: {corpus:?}");
        let mut refused = Vec::new();
        for name in &corpus {
            let validated = crate::validate_source(&ValidateInput {
                source: read(name),
                removed_globals: Vec::new(),
            })
            .expect("validate should answer an output rather than an envelope error");
            if !validated.valid {
                let error = &validated.errors[0];
                refused.push((name.clone(), error.kind, error.message.clone()));
            }
        }
        assert_eq!(
            refused,
            vec![(
                "query.marker_no_handle.js".to_string(),
                ScriptErrorKind::MissingHandle,
                crate::MISSING_HANDLE.to_string()
            )]
        );
    }

    #[test]
    fn every_corpus_script_runs_against_a_host_and_answers_what_it_should() {
        let cases = cases();
        // Every file is run at least once, so a script added to the corpus
        // without a case fails here.
        for name in corpus() {
            assert!(
                cases.iter().any(|case| case.file == name),
                "{name} has no case"
            );
        }

        for case in cases {
            let (output, fake) = execute(case.file, case.kind, case.input, case.context);
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
    #[test]
    fn a_chained_call_in_the_corpus_sends_the_lua_bridges_document() {
        let (_, fake) = execute(
            "procedure.spaces_create_and_write.js",
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
