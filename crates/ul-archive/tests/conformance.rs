//! Conformidade contra o golden da bancada (`testbench/corpus/cases/archive`), sobre o testkit.
//! Rode com `--nocapture` pra ver o placar e as falhas. Casos `script` precisam do `bash` (crate
//! `shell`) e dos coreutils na tabela; enquanto eles não existem, esses casos contam como falha.

use pl_testing::{TestkitCandidate, score_tool};

fn candidate() -> TestkitCandidate {
    TestkitCandidate::new("ul-archive (testkit)", ul_archive::programs())
}

#[test]
fn archive_corpus() {
    let report = score_tool("archive", &candidate());
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}
