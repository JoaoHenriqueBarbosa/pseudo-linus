//! Golden de declarações globais com o global não extensível, contra o JavaScriptCore do bun:
//! `tests/golden/global_declaration_bun.tsv` sai de `scripts/gen-global-declaration-golden.js`, rodado no bun 1.4.2.
//! Cada programa cria `R`, aplica `Object.preventExtensions` ou `Object.seal` em `globalThis` e roda uma declaração por
//! eval indireto: `var` e `function` lançam `TypeError` (`Can't declare global variable/function '<nome>': ...`),
//! `let`/`const`/`class` passam, a atribuição implícita cria a global em sloppy e lança `ReferenceError` em strict.
mod common;

const GOLDEN: &str = include_str!("golden/global_declaration_bun.tsv");

#[test]
fn global_declarations_on_non_extensible_global_match_bun() {
    common::run_golden_big_stack(GOLDEN, 40, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
