//! Borda de buffers contra o JavaScriptCore do bun: `tests/golden/buffer_edge_bun.tsv` sai de
//! `scripts/gen-buffer-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre Atomics (add/and/compareExchange/exchange/load/or/store/sub/xor por tipo, notify,
//! wait com timeout 0, waitAsync), SharedArrayBuffer (growable, grow, maxByteLength), ArrayBuffer redimensionável
//! (resize, transfer, transferToFixedLength, detached), typed arrays com length-tracking, DataView sobre eles, erros e
//! mensagens.
mod common;

const GOLDEN: &str = include_str!("golden/buffer_edge_bun.tsv");

#[test]
fn buffer_edge_matches_bun() {
    common::run_golden(GOLDEN, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "buffer_edge_case.js", "R"));
}
