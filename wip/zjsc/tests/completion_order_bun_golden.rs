//! Golden de completion, ordem de avaliação e escopo contra o JavaScriptCore do bun: `tests/golden/completion_order_bun.tsv`
//! sai de `scripts/gen-completion-order-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre valores de completion de eval/Function (if,
//! switch, try/finally, loops, labels), ordem de operandos e de atribuição composta com getters, optional chaining com
//! delete e chamadas, `new a.b()`, exponenciação, template tags com cache de strings e raw, TDZ, function em bloco do
//! Annex B, acessores em `with`, labels com continue em finally, generators com finally aninhado e arrows async.
mod common;

const GOLDEN: &str = include_str!("golden/completion_order_bun.tsv");
const PRELUDES: &str = include_str!("golden/completion_order.preludes.json");

#[test]
fn completion_order_and_scope_match_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição. O gerador roda o programa por `(0, eval)(fonte)`, então as `var` e
    // `function` do prelúdio nascem configuráveis (`delete l` dá `true`); `IndirectEvalDrained` faz o mesmo e
    // esvazia as microtarefas antes de ler `R`, como o `setTimeout(0)` do filho.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1000, |source| {
        common::EvalMode::IndirectEvalDrained.evaluate(source, "completion_order_case.js", "R")
    });
}
