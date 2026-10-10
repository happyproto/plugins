//! The conformance corpus in TypeScript: the JavaScript corpus's scripts
//! with their types written in, under `tests/corpus/`, run through the
//! engine's harness against the same table of answers.

use happyview_quickjs::conformance::Corpus;

use crate::frontend::TypeScript;

fn corpus() -> Corpus<'static> {
    Corpus {
        frontend: &TypeScript,
        dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus"),
        extension: "ts",
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
