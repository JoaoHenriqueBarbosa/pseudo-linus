//! Golden de construtores de TypedArray contra o JavaScriptCore do bun: `tests/golden/typedarray_ctor_bun.tsv` sai de
//! `scripts/gen-typedarray-ctor-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cobre os 11 tipos mais `Float16Array` contra argumentos (tamanho, array-like,
//! iterável, buffer com offset e length, outro typed array, buffer redimensionável e destacado), conversões de elemento
//! (`ToNumber`/`ToBigInt` com a ordem das chamadas, clamp, arredondamento), `Atomics` básico, `from`/`of` com `mapfn` e
//! `this`, métodos com índices extremos, ordem de efeitos em subclasses com `species` e mensagens exatas de erro.
//! O prelúdio comum das linhas fica em `tests/golden/typedarray_ctor.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/typedarray_ctor_bun.tsv");
const PRELUDES: &str = include_str!("golden/typedarray_ctor.preludes.json");

#[test]
fn typedarray_ctor_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "typedarray_ctor_case.js", "R")));
}
