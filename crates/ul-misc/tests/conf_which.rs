//! Conformidade do `which` contra o golden do oráculo (debianutils 5.23), no testkit e no kernel real.

use pl_testing::{KernelCandidate, TestkitCandidate, score_tool};

#[test]
fn which_conformance() {
    let cand = TestkitCandidate::new("which (testkit)", ul_misc::programs());
    let report = score_tool("which", &cand);
    report.print();
    assert!(
        report.conformance.strict_pass >= 26,
        "regressão no which: {}",
        report.summary()
    );
}

#[test]
fn which_conformance_kernel() {
    let cand = KernelCandidate::new("which (kernel)", ul_misc::programs());
    let report = score_tool("which", &cand);
    report.print();
    assert!(
        report.conformance.strict_pass >= 1,
        "regressão no which (kernel): {}",
        report.summary()
    );
}
