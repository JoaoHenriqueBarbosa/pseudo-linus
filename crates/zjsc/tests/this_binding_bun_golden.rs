//! Golden de `this` binding contra o JavaScriptCore do bun: `tests/golden/this_binding_bun.tsv` sai de
//! `scripts/gen-this-binding-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava (`typeof this` e a identidade). Cruza funções sloppy, strict, arrow, método, classe, bound,
//! generator, async, Proxy e nativas com `f()`, `obj.f()`, `(obj.f)()`, `(0,obj.f)()`, `obj?.f()`, `call`/`apply`/`bind`
//! com thisArg undefined, null e primitivos, `Reflect.apply`, `new` em bound, tagged template com membro, `with` e eval;
//! callbacks de Array, Map, Set, TypedArray, String e JSON com thisArg; getters e setters com receiver de
//! `Reflect.get`/`Reflect.set`; e o `this` global em eval, Function e inicializadores de classe.
mod common;

const GOLDEN: &str = include_str!("golden/this_binding_bun.tsv");
const PRELUDES: &str = include_str!("golden/this_binding.preludes.json");

#[test]
fn this_binding_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "this_binding_case.js", "R"));
}
