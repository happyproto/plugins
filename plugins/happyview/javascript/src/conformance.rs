//! The conformance corpus in JavaScript: the scripts under `tests/corpus/`,
//! with a provenance note saying what each was translated from, run through
//! the engine's harness.

use happyview_quickjs::conformance::Corpus;
use happyview_quickjs::JavaScript;

fn corpus() -> Corpus<'static> {
    Corpus {
        frontend: &JavaScript,
        dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus"),
        extension: "js",
    }
}

#[test]
fn every_corpus_script_validates_except_the_one_with_no_handle() {
    corpus().every_script_validates_except_the_one_with_no_handle();
}

#[test]
fn every_corpus_script_runs_against_a_host_and_answers_what_it_should() {
    corpus().every_script_runs_against_a_host_and_answers_what_it_should();
}

#[test]
fn a_chained_call_in_the_corpus_sends_the_lua_bridges_document() {
    corpus().a_chained_call_sends_the_lua_bridges_document();
}
