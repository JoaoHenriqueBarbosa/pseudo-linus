//! Conformidade do `strings` contra o golden da bancada (GNU binutils 2.44 do Debian 13), no kernel
//! de teste e no kernel real.

#[test]
fn conformance_strings() {
    let cand = pl_testing::TestkitCandidate::new("strings (testkit)", ul_misc::programs());
    let report = pl_testing::score_tool("strings", &cand);
    report.print();
    assert_eq!(
        report.missing_golden, 0,
        "casos sem golden: rode `oracle gen --tool strings`"
    );
    assert!(report.strict_rate() >= 1.0, "placar estrito caiu");
}

#[test]
fn conformance_strings_kernel() {
    let cand = pl_testing::KernelCandidate::new("strings (kernel)", ul_misc::programs());
    let report = pl_testing::score_tool("strings", &cand);
    report.print();
    assert!(
        report.strict_rate() >= 1.0,
        "placar estrito no kernel real caiu"
    );
}
