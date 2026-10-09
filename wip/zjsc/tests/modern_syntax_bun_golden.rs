//! Golden de sintaxe moderna de borda contra o JavaScriptCore do bun: `tests/golden/modern_syntax_bun.tsv` sai de
//! `scripts/gen-modern-syntax-golden.js`, rodado no bun 1.4.2 com `vm.runInThisContext` (JSC puro). Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava (`<undefined>` quando o script inteiro não compila).
//! Cobre optional chaining em todas as posições, `??` misturado com `||`/`&&`, atribuição lógica com getters, setters,
//! const e Proxy, `**`, separadores numéricos, BigInt literais, templates com escapes inválidos em tagged, static blocks,
//! `using`, `import.meta` fora de módulo, regex vs divisão, ASI de borda, Unicode em identificadores, comentários HTML,
//! hashbang, getters com nomes numéricos/string/computados, spread de objeto e label + function.
mod common;


const GOLDEN: &str = include_str!("golden/modern_syntax_bun.tsv");

#[test]
fn modern_syntax_matches_bun() {
    common::run_golden(GOLDEN, 500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
