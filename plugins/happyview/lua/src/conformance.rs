//! The conformance suites: every v3 script this project knows the text of,
//! and the 257 Lua 5.4 probes, both run through the exports rather than
//! around them.
//!
//! They live in the crate rather than in `tests/` because an integration test
//! would have to link it, and the `rlib` that needs costs 192 KB in the
//! shipped module — LTO can no longer internalise what an rlib must keep
//! exportable. The material they read is unmodified and stays under
//! `tests/corpus/` and `tests/probes/`, each with its own provenance note.

/// Every v3 script this project knows the exact text of, run through the
/// plugin's own `validate` export.
///
/// The files in `tests/corpus/` are unmodified copies of HappyView's codemod
/// expectations and editor templates; `tests/corpus/PROVENANCE.md` says which
/// is which and why three of them are refused.
///
/// What this covers and what it does not: a corpus script that only loads and
/// defines `handle` is covered whole, because loading is where the sandbox,
/// the removed-global guard and the stub `require` all act. A script that
/// *calls* a library is covered up to its first call, since answering one
/// needs a host — the 34 bridge cases and the differential comparison against
/// the native runner both live in HappyView, where wasmtime does.
#[cfg(test)]
mod corpus {
    use happyview_plugin_sdk::{ScriptErrorKind, ValidateInput};

    /// The names the host sends, and the list the two `marker_*_at_file_scope`
    /// cases are refused by.
    const REMOVED_GLOBALS: [&str; 32] = [
        "now",
        "log",
        "TID",
        "toarray",
        "json",
        "method",
        "input",
        "params",
        "caller_did",
        "collection",
        "delegate_did",
        "env",
        "space",
        "action",
        "uri",
        "did",
        "rkey",
        "record",
        "event",
        "job",
        "src",
        "val",
        "neg",
        "cts",
        "exp",
        "db",
        "http",
        "xrpc",
        "atproto",
        "linked_repos",
        "jobs",
        "Record",
    ];

    /// The three the native sandbox refuses too, each for a reason the codemod's
    /// own marker comment states.
    /// The kinds matter as much as the names: a file-scope read of a removed
    /// global is a *runtime* failure carrying the migration sentence, not a
    /// parse failure, and an author told otherwise goes looking for a missing
    /// `end`.
    const EXPECTED_REFUSALS: [(&str, ScriptErrorKind); 3] = [
        (
            "procedure.marker_input_read_at_file_scope.expected.lua",
            ScriptErrorKind::Runtime,
        ),
        (
            "query.marker_context_read_at_file_scope.expected.lua",
            ScriptErrorKind::Runtime,
        ),
        (
            "query.marker_no_handle.expected.lua",
            ScriptErrorKind::MissingHandle,
        ),
    ];

    fn corpus() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "lua"))
            .collect();
        files.sort();
        files
            .into_iter()
            .map(|path| {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                (name, std::fs::read_to_string(&path).unwrap())
            })
            .collect()
    }

    fn validate(source: &str) -> happyview_plugin_sdk::ValidateOutput {
        crate::validate_source(&ValidateInput {
            source: source.to_string(),
            removed_globals: REMOVED_GLOBALS.iter().map(|s| s.to_string()).collect(),
        })
        .expect("validate should answer an output rather than an envelope error")
    }

    #[test]
    fn every_corpus_script_loads_and_defines_handle_under_the_guard() {
        let corpus = corpus();
        assert_eq!(corpus.len(), 84, "the corpus is 84 files");

        let mut refused: Vec<(String, ScriptErrorKind, String)> = Vec::new();
        for (name, source) in &corpus {
            let validated = validate(source);
            if !validated.valid {
                let error = &validated.errors[0];
                refused.push((name.clone(), error.kind, error.message.clone()));
            }
        }

        let expected: Vec<&str> = EXPECTED_REFUSALS.iter().map(|(name, _)| *name).collect();
        let got: Vec<&str> = refused.iter().map(|(name, ..)| name.as_str()).collect();
        assert_eq!(
            got,
            expected,
            "refused with:\n{}",
            refused
                .iter()
                .map(|(name, kind, message)| format!("  {name}: {kind:?}: {message}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        for ((name, kind, message), (_, expected_kind)) in refused.iter().zip(EXPECTED_REFUSALS) {
            assert_eq!(*kind, expected_kind, "{name}: {message}");
        }
    }

    /// Each marker case is refused for its own reason, not merely refused. A
    /// guard that raised on every name, or a `validate` that refused everything,
    /// would pass the count above.
    #[test]
    fn the_two_file_scope_markers_are_refused_by_the_removed_global_guard() {
        for name in [
            "procedure.marker_input_read_at_file_scope.expected.lua",
            "query.marker_context_read_at_file_scope.expected.lua",
        ] {
            let source = corpus()
                .into_iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, source)| source)
                .unwrap_or_else(|| panic!("{name} is missing from the corpus"));

            let refused = validate(&source);
            assert!(!refused.valid, "{name}");
            assert!(
                refused.errors[0].message.contains("was removed in v3"),
                "{name}: {}",
                refused.errors[0].message
            );

            // With nothing removed, the same file-scope read is an ordinary
            // index of nil — so what refuses it is the guard and not the parser.
            let without_guard = crate::validate_source(&ValidateInput {
                source,
                removed_globals: Vec::new(),
            })
            .unwrap();
            assert!(!without_guard.valid, "{name}");
            assert!(
                without_guard.errors[0]
                    .message
                    .contains("attempt to index a nil value"),
                "{name}: {}",
                without_guard.errors[0].message
            );
        }
    }
}

/// The 257 Lua 5.4 probes, run through the plugin's own `execute` export and
/// compared against what native Lua 5.4 prints for each one.
///
/// `tests/probes/` holds unmodified copies of the spike's probe files and its
/// recorded reference output; `tests/probes/PROVENANCE.md` says where from.
///
/// Two things are normalised on both sides before comparing, exactly as the
/// spike's harness normalised them: a chunk position keeps its line number
/// but loses the chunk's name, since the reference was recorded under a
/// differently spelled chunk; and a table or function address becomes
/// `ADDR`, since no two runs agree on one.
///
/// Each probe file runs as the body of `handle`, opened on the file's own
/// first line so that no case's reported line number moves. `removed_globals`
/// is empty here: these probe Lua's behaviour, and the reference was recorded
/// against a VM with no migration guard.
///
/// What this cannot cover: a case that needs a host to answer. None of the
/// probes calls a library — that is the 34 bridge cases, which live in
/// HappyView with wasmtime, beside the differential comparison against the
/// native runner.
#[cfg(test)]
mod probes {
    use std::collections::BTreeMap;

    use happyview_plugin_sdk::{
        ExecuteContext, ExecuteInput, ExecuteLimits, ExecuteOutput, ScriptKind,
    };
    use regex::Regex;

    /// `os.time{...}` reads its table as *local* time, because that is what
    /// PUC does, so the recorded reference is only comparable under the zone
    /// it was recorded in. In the module there is no zone to read — the host
    /// gives the guest no environment, so wasi-libc answers UTC — but a
    /// native run takes the machine's, and three cases then differ by its
    /// offset.
    ///
    /// Checked through the interpreter rather than through the host's own
    /// clock, because what matters is the zone Lua sees. Loud rather than
    /// skipped: a conformance suite that quietly drops three cases is worse
    /// than one that says what it needs.
    fn require_utc() {
        let same: bool = run(r#"function handle()
                 return tostring(os.date("!%Y-%m-%dT%H:%M:%S", 0)
                                 == os.date("%Y-%m-%dT%H:%M:%S", 0))
               end"#
            .to_string())
        .is_ok_and(|report| report == "true");
        assert!(
            same,
            "the recorded reference was taken under TZ=UTC and `os.time` reads a \
             table as local time, so this suite needs TZ=UTC. Re-run it with \
             `TZ=UTC cargo test -p happyview-lua`."
        );
    }

    /// Every case the reference exercises that this plugin answers differently,
    /// with the reason. The reference was recorded against a VM with the whole
    /// standard library and no sandbox, so most of these are the sandbox doing
    /// its job rather than the interpreter disagreeing with Lua.
    ///
    /// Five are policy. `os.clock` is absent because preview 1 has no process CPU
    /// clock and a function that always fails teaches a script author nothing;
    /// `load`, `loadstring` and `dofile` are absent because loading a chunk at
    /// run time escapes every check the save path made. `wrap_dead` differs only
    /// in the chunk name of a position, which the catch guards add by being a Lua
    /// frame the reference did not have — the native runner installs the same
    /// guards, so this differs from PUC and not from the path being replaced.
    ///
    /// All six are policy. There is no fidelity gap left: `os.time` and
    /// `os.date` are Lua's own C functions, so their output, their
    /// normalisation and their error messages are PUC's rather than an
    /// imitation.
    const KNOWN_DIFFERENCES: [&str; 6] = [
        "clock_type",
        "load_exists",
        "load_chunk",
        "load_env",
        "load_syntax_error",
        "wrap_dead",
    ];

    fn probes_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/probes")
    }

    fn normalize(line: &str) -> String {
        let position = Regex::new(r#"(\[string "[^"]*"\]|\bscript|\binput):(\d+):"#).unwrap();
        let address =
            Regex::new(r"(table|function|userdata|thread|builtin): (0x)?[0-9a-fA-F]+").unwrap();
        let line = position.replace_all(line, "<pos>:$2:");
        address.replace_all(&line, "$1: ADDR").to_string()
    }

    /// A report into `(case name, whole record)`. A record is `name = …` or
    /// `name ! …`; a line starting neither belongs to the record before it, which
    /// is how a value containing a newline stays one case.
    fn records(report: &str) -> Vec<(String, String)> {
        let head = Regex::new(r"^([A-Za-z0-9_]+) [=!] ").unwrap();
        let mut out: Vec<(String, String)> = Vec::new();
        for line in report.lines() {
            match head.captures(line) {
                Some(captured) => out.push((captured[1].to_string(), normalize(line))),
                None => {
                    if let Some(last) = out.last_mut() {
                        last.1.push('\n');
                        last.1.push_str(&normalize(line));
                    }
                }
            }
        }
        out
    }

    /// The recorded Lua 5.4 output, by section. `== name` opens a section.
    fn reference() -> BTreeMap<String, Vec<(String, String)>> {
        let text = std::fs::read_to_string(probes_dir().join("reference.txt"))
            .expect("the recorded reference output should be beside the probes");
        let mut sections = BTreeMap::new();
        let mut current = String::new();
        let mut body = String::new();
        for line in text.lines() {
            match line.strip_prefix("== ") {
                Some(name) => {
                    if !current.is_empty() {
                        sections.insert(current.clone(), records(&body));
                    }
                    current = name.to_string();
                    body.clear();
                }
                None => {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
        if !current.is_empty() {
            sections.insert(current, records(&body));
        }
        sections
    }

    fn run(source: String) -> Result<String, String> {
        let input = ExecuteInput {
            source,
            kind: ScriptKind::XrpcQuery,
            input: serde_json::json!({}),
            context: ExecuteContext {
                trigger: "probe".to_string(),
                ..Default::default()
            },
            libraries: Vec::new(),
            limits: ExecuteLimits {
                // The heaviest probe files loop, and none of them is the subject
                // of the budget's own tests.
                instructions: None,
                memory_bytes: 256 * 1024 * 1024,
            },
            removed_globals: Vec::new(),
        };
        match crate::execute_script(&input).expect("execute should answer an output") {
            ExecuteOutput::Returned { value, .. } => match value {
                serde_json::Value::String(report) => Ok(report),
                other => Err(format!(
                    "a probe file returned {other} rather than a report"
                )),
            },
            ExecuteOutput::Error { message, .. } => Err(message),
        }
    }

    #[test]
    fn every_probe_matches_the_lua_54_reference_but_the_known_differences() {
        let dir = probes_dir();
        let prelude = std::fs::read_to_string(dir.join("_prelude.lua")).expect("the probe prelude");
        require_utc();
        let reference = reference();
        assert_eq!(reference.len(), 15, "fifteen probe sections");

        let mut total = 0usize;
        let mut matched = 0usize;
        // Keyed by case name, not by the rendered text: a filter over the
        // whole line would whitelist a case whose *value* happened to contain
        // another case's name, and a whole-section failure whose error text
        // did. The empty key is a section failure, which is never expected.
        let mut differences: Vec<(String, String)> = Vec::new();

        for (section, expected) in &reference {
            let path = dir.join(format!("{section}.lua"));
            let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{section}: {e}"));
            // `handle` opens on the prelude's own first line and closes after the
            // probe's last, so every reported line number is the one the
            // reference recorded.
            let report = match run(format!("function handle() {prelude}\n{body} end")) {
                Ok(report) => report,
                Err(e) => {
                    total += expected.len();
                    differences.push((
                        String::new(),
                        format!(
                            "{section}: the whole file failed ({} cases): {e}",
                            expected.len()
                        ),
                    ));
                    continue;
                }
            };
            let got: BTreeMap<String, String> = records(&report).into_iter().collect();

            for (case, want) in expected {
                total += 1;
                match got.get(case) {
                    Some(have) if have == want => matched += 1,
                    Some(have) => differences.push((
                        case.clone(),
                        format!("{section}/{case}:\n    reference: {want}\n    got:       {have}"),
                    )),
                    None => differences.push((
                        case.clone(),
                        format!("{section}/{case}: missing from the report"),
                    )),
                }
            }
        }

        assert_eq!(total, 257, "the corpus is 257 cases");

        let unexpected: Vec<&str> = differences
            .iter()
            .filter(|(case, _)| !KNOWN_DIFFERENCES.contains(&case.as_str()))
            .map(|(_, rendered)| rendered.as_str())
            .collect();
        assert!(
            unexpected.is_empty(),
            "{matched}/{total} cases match; {} unexpected:\n{}",
            unexpected.len(),
            unexpected.join("\n")
        );

        // Below this is a regression; above it means a known difference has
        // closed and the list above should lose an entry.
        assert_eq!(
            matched,
            257 - KNOWN_DIFFERENCES.len(),
            "expected every case but {KNOWN_DIFFERENCES:?} to match"
        );
    }
}
