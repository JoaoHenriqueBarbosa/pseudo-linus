//! Golden de propriedades e acessores contra o JavaScriptCore do bun: `tests/golden/accessor_bun.tsv` sai de
//! `scripts/gen-accessor-golden.js`, rodado no bun 1.4.2 via `node:vm`. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre `defineProperty`/`defineProperties`/`getOwnPropertyDescriptors`,
//! getters e setters em literais e classes, `super.prop`, `__proto__` literal, `__defineGetter__`/`__lookupGetter__`,
//! `freeze`/`seal`/`preventExtensions` em arrays e typed arrays, propriedades indexadas contra nomeadas, ordem de chaves
//! inteiras, `Symbol.toPrimitive`, herança de acessores e escrita em readonly no modo estrito contra o sloppy.
mod common;

const GOLDEN: &str = include_str!("golden/accessor_bun.tsv");
const PRELUDES: &str = include_str!("golden/accessor.preludes.json");

#[test]
fn accessor_matches_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 500, "accessor_case.js");
}
