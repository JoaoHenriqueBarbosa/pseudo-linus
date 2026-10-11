//! Golden de `SharedArrayBuffer` e dos caminhos de `Atomics` que não bloqueiam contra o JavaScriptCore do bun:
//! `tests/golden/sab_bun.tsv` sai de `scripts/gen-sab-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON,
//! várias linhas) e o texto da variável global `R` que ele grava. Cobre o construtor e `growable`/`maxByteLength`/`grow`,
//! `slice` e species, subclasses e receptores errados, `@@toStringTag`, `Atomics.wait`/`notify`/`waitAsync` (timeout 0,
//! `not-equal`, `timed-out`, `{async:false,value}` e `{async:true,value:Promise}` com `notify` e a ordem das
//! microtarefas), erros de tipo e de faixa do `Atomics`, `DataView` com length-tracking sobre SAB redimensionável e
//! `TypedArray` sobre ele depois de `grow`. Os programas assíncronos esvaziam as microtarefas e só então gravam `R`;
//! `EvalMode::IndirectEvalDrained` esvazia as microtarefas antes de ler `R`, como o `setTimeout(0)` do filho do gerador.
mod common;

const GOLDEN: &str = include_str!("golden/sab_bun.tsv");
const PRELUDES: &str = include_str!("golden/sab.preludes.json");

#[test]
fn sab_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1000, |source| common::EvalMode::IndirectEvalDrained.evaluate(source, "sab_case.js", "R"));
}
