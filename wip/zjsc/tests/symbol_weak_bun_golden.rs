//! Golden de Symbol, WeakRef, FinalizationRegistry e WeakMap/WeakSet contra o JavaScriptCore do bun:
//! `tests/golden/symbol_weak_bun.tsv` sai de `scripts/gen-symbol-weak-golden.js`, rodado no bun 1.4.2. Cada linha é
//! um programa (JSON, várias linhas) e o texto da variável global `R` que ele grava: o resultado, ou `Nome: mensagem`
//! do erro lançado. Cobre `Symbol()`, `Symbol.for`/`keyFor`, conversões e suas mensagens de TypeError, símbolo como
//! chave de propriedade, wrapper `Object(sym)`, `@@toPrimitive`, chaves símbolo em WeakMap/WeakSet, WeakRef,
//! FinalizationRegistry e os símbolos bem conhecidos nos builtins. Nada depende de GC.
mod common;

const GOLDEN: &str = include_str!("golden/symbol_weak_bun.tsv");
const PRELUDES: &str = include_str!("golden/symbol_weak.preludes.json");

#[test]
fn symbol_and_weak_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 600, "symbol_weak_case.js");
}
