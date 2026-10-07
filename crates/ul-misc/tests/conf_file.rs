//! Conformidade do `file` contra o golden da bancada (file 5.46 do Debian 13), no kernel de teste e
//! no kernel real. No kernel real os executáveis embutidos são ELFs do Debian, então os casos de ELF
//! exercitam o leitor de ELF (`readelf.c`) sobre `/usr/bin` de verdade.

#[test]
fn conformance_file() {
    let cand = pl_testing::TestkitCandidate::new("file (testkit)", ul_misc::programs());
    let report = pl_testing::score_tool("file", &cand);
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode `oracle gen --tool file`");
    assert!(report.conformance.strict_rate() >= 1.0, "placar estrito caiu");
}

#[test]
fn conformance_file_kernel() {
    let cand = pl_testing::KernelCandidate::new("file (kernel)", ul_misc::programs());
    let report = pl_testing::score_tool("file", &cand);
    report.print();
    assert!(report.conformance.strict_rate() >= 1.0, "placar estrito no kernel real caiu");
}
