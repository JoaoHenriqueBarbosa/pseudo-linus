//! Conformidade do `jq` (e, quando existir, do `yq`) contra o golden da bancada
//! (`testbench/golden/<tool>`), sobre o testkit e sobre o kernel real.
//!
//! Rode com `cargo test -p ul-jq --test conformance -- --nocapture` para ver o placar.

use pl_testing::{score_tool, KernelCandidate, TestkitCandidate};

fn programs() -> Vec<sysabi::Program> {
    ul_jq::programs()
}

#[test]
fn jq_cases_on_testkit() {
    let cand = TestkitCandidate::new("jq (testkit)", programs());
    let report = score_tool("jq", &cand);
    report.print();
    assert!(report.conformance.total > 0);
}

#[test]
fn jq_cases_on_kernel() {
    let cand = KernelCandidate::new("jq (kernel)", programs());
    let report = score_tool("jq", &cand);
    report.print();
    assert!(report.conformance.total > 0);
}
