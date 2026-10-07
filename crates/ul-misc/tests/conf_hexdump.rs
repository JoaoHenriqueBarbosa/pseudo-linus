//! Conformidade do `hexdump`/`hd` contra o golden da bancada (util-linux 2.41.5 do Debian 13), no
//! kernel de teste e no kernel real.

#[test]
fn conformance_hexdump() {
    let cand = pl_testing::TestkitCandidate::new("hexdump (testkit)", ul_misc::programs());
    let report = pl_testing::score_tool("hexdump", &cand);
    report.print();
    assert_eq!(
        report.missing_golden, 0,
        "casos sem golden: rode `oracle gen --tool hexdump`"
    );
    assert!(report.conformance.strict_rate() >= 1.0, "placar estrito caiu");
}

#[test]
fn conformance_hexdump_kernel() {
    let cand = pl_testing::KernelCandidate::new("hexdump (kernel)", ul_misc::programs());
    let report = pl_testing::score_tool("hexdump", &cand);
    report.print();
    assert!(
        report.conformance.strict_rate() >= 1.0,
        "placar estrito no kernel real caiu"
    );
}
