//! Golden de Array contra o JavaScriptCore do bun: `tests/golden/array_bun.tsv` sai de `scripts/gen-array-golden.js`,
//! rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas, com o prelúdio `S`/`T`/`D` que serializa buracos,
//! `-0`, bigint e captura exceção) e o texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas.
//! Cobre os métodos de `Array.prototype` (inclusive os de cópia `toSorted`/`toSpliced`/`toReversed`/`with`), `Array.from`,
//! `Array.of`, `Array.fromAsync`, iteradores, arrays esparsos, array-likes de `length` gigante, species, proxies, getters
//! que mutam durante a iteração, TypedArray contra Array e as mensagens exatas de erro.
mod common;

const GOLDEN: &str = include_str!("golden/array_bun.tsv");

const PRELUDES: &str = include_str!("golden/array.preludes.json");

#[test]
fn array_matches_bun() {
    // Pilha grande, como o bun (que aguenta 4313 níveis de `join` aninhado): a thread de teste padrão tem 2 MiB, e o
    // orçamento nativo padrão (1 MiB) estouraria bem antes do limite lógico do `join`.
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 2000, "array_case.js");
}
