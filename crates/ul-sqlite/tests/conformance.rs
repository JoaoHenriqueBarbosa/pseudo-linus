//! Conformidade do `sqlite3` contra o golden do sqlite3 3.46.1 do Debian 13
//! (`testbench/corpus/cases/sqlite`), no kernel de teste e no kernel real.
//!
//! `cargo test -p ul-sqlite --test conformance -- --nocapture` mostra o placar e as falhas.

use pl_testing::{KernelCandidate, Report, TestkitCandidate, score_tool};

/// Casos `script` (encadeiam vários comandos pelo bash): dependem do `bash` do crate `shell`.
const SCRIPT_CASES: &[&str] = &[
    "sqlite-multi-persist-between-commands",
    "sqlite-multi-schema-file-redirect",
    "sqlite-multi-pipe-into-cli",
    "sqlite-multi-output-redirect",
    "sqlite-multi-exit-status",
    "sqlite-multi-transaction-across-statements",
    "sqlite-x-readonly-write-attempt",
    "sqlite-x-hot-journal-abrupt-exit",
];

/// Falhas conhecidas, com o motivo no STATUS.md.
const KNOWN: &[&str] = &[];

fn check(report: &Report) {
    report.print();
    let unexpected: Vec<String> = report
        .failing_ids()
        .into_iter()
        .filter(|id| !SCRIPT_CASES.contains(&id.as_str()) && !KNOWN.contains(&id.as_str()))
        .collect();
    assert!(unexpected.is_empty(), "falhas inesperadas: {unexpected:?}\n{}", report.summary());
}

#[test]
fn sqlite_corpus_testkit() {
    let cand = TestkitCandidate::new("sqlite3 (testkit)", ul_sqlite::programs());
    check(&score_tool("sqlite", &cand));
}

#[test]
fn sqlite_corpus_kernel() {
    let cand = KernelCandidate::new("sqlite3 (kernel)", ul_sqlite::programs());
    check(&score_tool("sqlite", &cand));
}
