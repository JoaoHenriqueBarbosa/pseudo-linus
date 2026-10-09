//! Golden de `Function.prototype` contra o JavaScriptCore do bun: `tests/golden/function_proto_bun.tsv` sai de
//! `scripts/gen-function-proto-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre `toString` de todas as
//! categorias, `bind` (name, length, `new`, `instanceof`), `call`/`apply` com array-like, `Symbol.hasInstance`,
//! `name`/`length`, `caller`/`arguments`, `Reflect.construct` com `newTarget` diferente, `instanceof` e a recursão
//! profunda (`RangeError`). O arquivo se chama `function_proto_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/function_proto_bun.tsv");

const PRELUDES: &str = include_str!("golden/function_proto.preludes.json");

#[test]
fn function_prototype_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1200, "function_proto_case.js");
}
