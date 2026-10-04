//! Conformidade do `envsubst` contra o golden do oráculo (gettext-base 0.23.1), no testkit e no
//! kernel real.

use pl_testing::{KernelCandidate, TestkitCandidate, score_tool};

#[test]
fn envsubst_conformance() {
    let cand = TestkitCandidate::new("envsubst (testkit)", ul_misc::programs());
    let report = score_tool("envsubst", &cand);
    report.print();
    assert!(report.conformance.strict_pass >= 22, "regressão no envsubst: {}", report.summary());
}

#[test]
fn envsubst_conformance_kernel() {
    let cand = KernelCandidate::new("envsubst (kernel)", ul_misc::programs());
    let report = score_tool("envsubst", &cand);
    report.print();
    assert!(report.conformance.strict_pass >= 22, "regressão no envsubst (kernel): {}", report.summary());
}
