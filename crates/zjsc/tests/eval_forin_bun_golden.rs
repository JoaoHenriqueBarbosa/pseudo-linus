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

/// Dois quirks de ordem do JavaScriptCore que a especificação não fixa (medidos no bun 1.4.2):
/// - 2545, 2556 e 2578: a invariante de `ownKeys` de um Proxy sobre `new String('ab')` cita no bun outra chave não configurável
///   omitida (`length`, `1`) que o motor (`0`, a primeira do alvo); a ordem de checagem da invariante é do JSC e a causa exata
///   no código do motor não foi investigada;
/// - 2827 e 2828: a ordem em que o for-in do global lista os nomes que o script declarou (`log,T,S,FI,R`) difere; causa não investigada.
const CAUSE: &str = "ordem que o JavaScriptCore produz e o motor ainda não reproduz (invariante de ownKeys, ordem das chaves do global)";
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "2545", cause: CAUSE },
    common::KnownCase { label: "2556", cause: CAUSE },
    common::KnownCase { label: "2578", cause: CAUSE },
    common::KnownCase { label: "2827", cause: CAUSE },
    common::KnownCase { label: "2828", cause: CAUSE },
];

#[test]
fn eval_forin_matches_bun() {
    common::run_factored_big_stack_known_lines(GOLDEN, PRELUDES, 3000, KNOWN, |source| evaluate_named_script_result(source, "eval_forin_case.js", "R"));
}
