//! Golden de `require` e `require.resolve` do wrapper CJS contra o bun 1.4.2: `tests/golden/cjs_require_bun.tsv` sai de
//! `scripts/gen-cjs-require-golden.js`. Cada linha é `nome<TAB>JSON(programa)<TAB>JSON(resultado)<TAB>[1]`; o programa
//! roda no wrapper CJS (`evaluate_cjs_program`) com `__filename` `/case.cjs` e grava a string `R`.
mod common;

use common::{evaluate_golden_program, guarded, json_string, ProgramMeta};

const GOLDEN: &str = include_str!("golden/cjs_require_bun.tsv");

#[test]
fn cjs_require_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(4, '\t');
        let name = columns.next().expect("nome do caso");
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        let meta = columns.next().map_or_else(ProgramMeta::default, ProgramMeta::parse);
        total += 1;
        match guarded(|| evaluate_golden_program(&source, &meta, "/case.cjs", "R")) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{name}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{name}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 30, "golden com só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
