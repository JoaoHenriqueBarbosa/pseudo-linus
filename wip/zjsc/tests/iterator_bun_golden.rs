//! Golden de iteradores contra o JavaScriptCore do bun: `tests/golden/iterator_bun.tsv` sai de
//! `scripts/gen-iterator-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava. Cobre os helpers de `Iterator.prototype`, `Iterator.from`/`concat`, o fechamento do iterador
//! subjacente, geradores (`yield*`, `return`/`throw` em todos os estados), destructuring, spread, `Array.from`,
//! `Symbol.iterator` dos embutidos e os métodos de Set com set-likes inválidos, com o log de chamadas em `L`.
mod common;

const GOLDEN: &str = include_str!("golden/iterator_bun.tsv");

const PRELUDES: &str = include_str!("golden/iterator.preludes.json");

#[test]
fn iterators_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1500, "iterator_case.js");
}
