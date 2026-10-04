//! Conformidade contra o golden da bancada sobre o kernel real (`--features kernel`). Mesmos casos do
//! `conformance.rs`, mas com processos de verdade: pipes concorrentes (`gzip -c | tar -t`), sinais e
//! o VFS do kernel. Rode com
//! `cargo test -p ul-archive --features kernel --test kernel_conformance -- --nocapture`.

#![cfg(feature = "kernel")]

use pl_testing::{KernelCandidate, score_tool};

#[test]
fn archive_corpus_on_kernel() {
    let cand = KernelCandidate::new("ul-archive (kernel)", ul_archive::programs());
    let report = score_tool("archive", &cand);
    report.print();
    assert_eq!(report.missing_golden, 0, "casos sem golden: rode o oráculo");
}
