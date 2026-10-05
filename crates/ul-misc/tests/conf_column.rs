//! Conformidade do `column` contra o golden do oráculo (util-linux 2.41.5), mais os casos de
//! `column` do corpus de CSV, no testkit e no kernel real.

use pl_testing::{KernelCandidate, TestkitCandidate, score_ids, score_tool};

const CSV_IDS: [&str; 5] = [
    "csv-column-basic",
    "csv-column-empty-and-ragged",
    "csv-column-unicode-width",
    "csv-column-stdin",
    "csv-column-crlf",
];

#[test]
fn column_conformance() {
    let cand = TestkitCandidate::new("column (testkit)", ul_misc::programs());
    let report = score_tool("column", &cand);
    report.print();
    assert!(
        report.conformance.strict_pass >= 129,
        "regressão no column: {}",
        report.summary()
    );
}

#[test]
fn csv_column_cases() {
    let cand = TestkitCandidate::new("column (testkit)", ul_misc::programs());
    let report = score_ids("csv", &cand, &CSV_IDS);
    report.print();
    assert_eq!(
        report.conformance.strict_pass,
        CSV_IDS.len(),
        "{}",
        report.summary()
    );
}

#[test]
fn column_conformance_kernel() {
    let cand = KernelCandidate::new("column (kernel)", ul_misc::programs());
    let report = score_tool("column", &cand);
    report.print();
    let csv = score_ids("csv", &cand, &CSV_IDS);
    csv.print();
    assert!(
        report.conformance.strict_pass >= 1,
        "regressão no column (kernel): {}",
        report.summary()
    );
}
