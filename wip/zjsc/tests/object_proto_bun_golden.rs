//! Golden de `Object.prototype` e `Object` contra o JavaScriptCore do bun: `tests/golden/object_proto_bun.tsv` sai de
//! `scripts/gen-object-proto-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre o getter e o setter de `__proto__` (primitivos, protótipo nulo, ciclos,
//! Proxy, objetos não extensíveis), `__defineGetter__`/`__defineSetter__`/`__lookupGetter__`/`__lookupSetter__`,
//! `Object.prototype.toString` com `@@toStringTag`, `toLocaleString`, `valueOf` em primitivos, `isPrototypeOf` e
//! `propertyIsEnumerable` com Proxy, `Object.assign` e spread com getters e símbolos, `Object.entries`/`values`/`keys`
//! em strings e arrays esparsos, `Object.setPrototypeOf`/`Reflect.setPrototypeOf` com ciclos e protótipo imutável,
//! `Object.hasOwn` e `Object.groupBy`.
mod common;

const GOLDEN: &str = include_str!("golden/object_proto_bun.tsv");
const PRELUDES: &str = include_str!("golden/object_proto.preludes.json");

#[test]
fn object_proto_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "object_proto_case.js", "R"));
}
