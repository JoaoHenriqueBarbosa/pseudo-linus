//! Golden de eval e with contra o JavaScriptCore do bun: `tests/golden/eval_with_bun.tsv` sai de
//! `scripts/gen-eval-with-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre eval direto e indireto numa grade de contextos (função, arrow, método, classe,
//! campo, parâmetro padrão, with, catch, bloco), declarações `var`/`function`/`let` dentro de eval e seus conflitos, `this`,
//! `arguments`, `new.target` e `super` em eval, `with` com `Symbol.unscopables` e `Proxy` com log, `delete` de bindings
//! criados por eval, `Function`, `GeneratorFunction`, `AsyncFunction` e declarações globais via eval indireto.
//! O prelúdio comum das linhas fica em `tests/golden/eval_with.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/eval_with_bun.tsv");
const PRELUDES: &str = include_str!("golden/eval_with.preludes.json");

#[test]
fn eval_with_matches_bun() {
    // O filho do oráculo captura o `globalThis` antes do programa e imprime `String(R)` depois dele (o `RESULT_PRELOAD` dos
    // geradores). Um `delete globalThis` ou um `var globalThis=1` no programa não muda de onde ele lê `R`, e ler pela
    // propriedade `globalThis` depois do programa lançaria `ReferenceError`: o modo drenado lê pelo `this` do topo.
    common::check(GOLDEN, PRELUDES, 3000, |source| {
        common::guarded_units(|| common::EvalMode::IndirectEvalDrained.evaluate(source, "eval_with_case.js", "R"))
    });
}
