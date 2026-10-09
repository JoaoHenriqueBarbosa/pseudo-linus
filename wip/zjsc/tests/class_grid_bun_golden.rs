//! Golden da grade de classes contra o JavaScriptCore do bun: `tests/golden/class_grid_bun.tsv` sai de
//! `scripts/gen-class-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre a ordem de inicialização de campos públicos, privados e estáticos e de blocos
//! `static`, computed keys com efeitos, `extends` de null/função/Proxy/bound/arrow/generator, `super()` repetido, ausente,
//! em arrow e em eval, `this` antes do `super`, retorno de objeto ou primitivo do construtor derivado, `new.target` em
//! cadeias, `Symbol.species`, métodos privados, `#x in obj`, brand checks, accessors estáticos, herança de builtins e as
//! mensagens exatas de TypeError, ReferenceError e SyntaxError.
mod common;

const GOLDEN: &str = include_str!("golden/class_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/class_grid.preludes.json");

#[test]
fn class_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "class_grid_case.js", "R"));
}
