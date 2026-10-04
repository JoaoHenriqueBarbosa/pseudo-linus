//! Conformidade contra o golden da bancada sobre o kernel real (`--features kernel`). Mesmos casos do
//! `conformance.rs`, mas com processos de verdade: pipes concorrentes, sinais e o VFS do kernel.
//! Rode com `cargo test -p ul-diff --features kernel --test kernel_conformance -- --nocapture`.

#![cfg(feature = "kernel")]

use pl_testing::{KernelCandidate, score_tool};

fn candidate() -> KernelCandidate {
    KernelCandidate::new("ul-diff (kernel)", ul_diff::programs())
}

#[test]
fn diff_corpus_on_kernel() {
    let report = score_tool("diff", &candidate());
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}

#[test]
fn patch_corpus_on_kernel() {
    let report = score_tool("patch", &candidate());
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}
