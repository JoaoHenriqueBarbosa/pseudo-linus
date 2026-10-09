//! Golden da grade de TDZ e escopo léxico contra o JavaScriptCore do bun: `tests/golden/tdz_grid_bun.tsv` sai de
//! `scripts/gen-tdz-grid-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável global `R` que
//! ele grava. Cobre o acesso antes da inicialização a let/const/class em cada posição (mesmo bloco, closure chamada antes,
//! arrow, método, getter, campo, bloco `static`, gerador, laço, eval direto) e forma de acesso, defaults de parâmetro
//! referindo parâmetro posterior, `for (let i in/of)` com expressão referindo `i`, `switch` com `case` compartilhando
//! bloco, `class extends` e computed key referindo o próprio nome, shadowing em `catch (e)` com `var e`, function
//! declarations em bloco com `let` homônimo, `let` global contra `var` homônimo avaliados com `(0,eval)` em sequência e as
//! mensagens exatas de ReferenceError, TypeError e SyntaxError.
mod common;

const GOLDEN: &str = include_str!("golden/tdz_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/tdz_grid.preludes.json");

#[test]
fn tdz_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "tdz_grid_case.js", "R"));
}
