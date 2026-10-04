//! Conformidade do `tree` contra o golden do oráculo (tree 2.2.1 do Debian 13), no testkit e no
//! kernel real.

use pl_testing::{KernelCandidate, TestkitCandidate, score_tool};

#[test]
fn tree_conformance() {
    let cand = TestkitCandidate::new("tree (testkit)", ul_misc::programs());
    let report = score_tool("tree", &cand);
    report.print();
    assert!(report.conformance.strict_pass >= 80, "regressão no tree: {}", report.summary());
}

#[test]
fn tree_conformance_kernel() {
    let cand = KernelCandidate::new("tree (kernel)", ul_misc::programs());
    let report = score_tool("tree", &cand);
    report.print();
    assert!(report.conformance.strict_pass >= 1, "regressão no tree (kernel): {}", report.summary());
}
