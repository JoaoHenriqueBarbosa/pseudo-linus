//! Conformidade contra o golden da bancada (`testbench/corpus/cases/{diff,patch}`), sobre o testkit.
//! Rode com `--nocapture` pra ver o placar e as falhas.

use pl_testing::{TestkitCandidate, score_tool};

fn candidate() -> TestkitCandidate {
    TestkitCandidate::new("ul-diff (testkit)", ul_diff::programs())
}

#[test]
fn diff_corpus() {
    let report = score_tool("diff", &candidate());
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}

#[test]
fn patch_corpus() {
    let report = score_tool("patch", &candidate());
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}
