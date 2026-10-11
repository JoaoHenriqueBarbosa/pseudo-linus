//! Golden de mensagens de erro de runtime do dia a dia contra o JavaScriptCore do bun: `tests/golden/error_message_bun.tsv`
//! sai de `scripts/gen-error-message-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` (`name: message` do erro capturado, ou `ok <typeof>`). Cobre chamar valor não chamável
//! (com o `(evaluating '...')` de vários formatos de callee), leitura e escrita em undefined/null, `in` e `instanceof`
//! inválidos, spread e destructuring, JSON circular, BigInt e Symbol misturados, const, classes e super, estouro de
//! pilha, RangeError de números, URI malformado e RegExp inválido.
mod common;

const GOLDEN: &str = include_str!("golden/error_message_bun.tsv");

#[test]
fn error_message_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "error_message_case.js", "R"));
}
