//! Golden de `Symbol.species` em grade contra o JavaScriptCore do bun: `tests/golden/species_grid_bun.tsv` sai de
//! `scripts/gen-species-grid-golden.js`, rodado no bun 1.4.2. Cada linha é o sufixo de um programa (o prelúdio está em
//! `species_grid.preludes.json`) e o texto da variável global `R` que ele grava. A grade cruza as operações que consultam
//! species (Array map, filter, slice, splice, concat, flat e flatMap; TypedArray map, filter, slice e subarray;
//! ArrayBuffer e SharedArrayBuffer slice; Promise then, catch e finally; RegExp split e matchAll) com valores de species
//! (undefined, null, não construtor, construtor que devolve objeto menor, de outro tipo, primitivo ou congelado, que
//! lança, classe, bound, proxy), `constructor` que não é objeto ou cujo getter lança, receptores (array, array-like,
//! proxy, subclasse com species estático), a contagem de chamadas e argumentos, as operações de Map e Set que ignoram
//! species e os getters `Symbol.species` dos construtores embutidos.
mod common;

const GOLDEN: &str = include_str!("golden/species_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/species_grid.preludes.json");

#[test]
fn species_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "species_grid_case.js", "R"));
}
