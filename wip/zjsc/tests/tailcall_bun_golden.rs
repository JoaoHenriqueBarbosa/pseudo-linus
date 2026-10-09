//! Golden de proper tail calls contra o JavaScriptCore do bun: `tests/golden/tailcall_bun.tsv` sai de
//! `scripts/gen-tailcall-golden.js`, rodado no bun 1.4.2. Cada linha é `caso`, o programa (JSON), o texto da
//! variável global `R` (JSON): o resultado de uma recursão de 1e6 níveis, ou o nome do erro (`RangeError`), e a
//! coluna opcional de modo e mapa de posições (`common::ProgramMeta`).
//!
//! Os casos `esm_*` (antes `sloppy_*`) são programas sem diretiva que o bun roda como ESM, portanto estritos; a
//! cauda vem do `op_tail_call` do bytecode, não do JIT. Os `sloppy_*` são `.cjs` sem diretiva (modo 1 na meta): o
//! bun dá `RangeError`, pois sloppy não emite `op_tail_call`. Nenhum caso é pulado. Ver `wip/notes/tailcall-sloppy.md`.
mod common;

use common::{evaluate_golden_program, guarded, json_string, ProgramMeta};

const GOLDEN: &str = include_str!("golden/tailcall_bun.tsv");

#[test]
fn tail_calls_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(4, '\t');
        let name = columns.next().expect("nome do caso");
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        let meta = columns.next().map_or_else(ProgramMeta::default, ProgramMeta::parse);
        total += 1;
        match guarded(|| evaluate_golden_program(&source, &meta, "tailcall_case.js", "R")) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{name}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{name}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 10, "golden com só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
