//! Conformidade do ul-procps contra o golden do oráculo (`testbench/corpus/cases/procps`). Só entra
//! no corpus o que é determinístico no oráculo (ajuda, versões, erros, listas de sinais, buscas sem
//! casamento); os formatos com dados de processo são conferidos pelos testes com `/proc` falso.

#[test]
fn conformance_procps() {
    let cand = pl_testing::TestkitCandidate::new("procps (testkit)", ul_procps::programs());
    let report = pl_testing::score_tool("procps", &cand);
    report.print();
    let c = &report.conformance;
    assert!(c.total > 0, "sem casos de conformidade");
    // Piso do placar atual, pra pegar regressão.
    assert!(report.strict_rate() >= 1.0, "placar estrito caiu: {}", report.summary());
}
