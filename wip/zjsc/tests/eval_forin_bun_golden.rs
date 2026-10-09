//! Golden de eval e for-in contra o JavaScriptCore do bun: `tests/golden/eval_forin_bun.tsv` sai de
//! `scripts/gen-eval-forin-golden.js`, rodado no bun 1.4.2. Cada linha é um programa sloppy (JSON) e o texto da variável
//! global `R` que ele grava. Cobre eval direto e indireto com var/function/let/const/class em global, função e `with`,
//! global não extensível (`Object.preventExtensions(globalThis)`), setters e getters no global, for-in sobre Proxy com
//! log de traps (ownKeys, getOwnPropertyDescriptor, getPrototypeOf), chaves que não são string, mutação durante o laço,
//! protótipos com chaves sombreadas, strings, arrays, typed arrays, arguments e todos os alvos de for-in.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/eval_forin_bun.tsv");
const PRELUDES: &str = include_str!("golden/eval_forin.preludes.json");

#[test]
fn eval_forin_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| evaluate_named_script_result(source, "eval_forin_case.js", "R"));
}
