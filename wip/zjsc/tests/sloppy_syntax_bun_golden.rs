//! Golden de semântica sloppy e sintaxe de borda contra o JavaScriptCore do bun: `tests/golden/sloppy_syntax_bun.tsv`
//! sai de `scripts/gen-sloppy-syntax-golden.js`, rodado no bun 1.4.2. Cada linha é um programa sloppy (JSON) e o texto
//! da variável global `R` que ele grava (valor ou `Nome: mensagem` do erro, inclusive SyntaxError). Cobre funções em
//! blocos, `with` e unscopables, octais e escapes, comentários HTML, `arguments`, `this` sloppy, eval, `new.target`,
//! `__defineGetter__` e `__proto__`, `Function(...)`, labels, ASI, templates, optional chaining, atribuição lógica,
//! exponente, separadores numéricos, BigInt, campos e métodos privados, blocos estáticos, `accessor` e `super`.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/sloppy_syntax_bun.tsv");

#[test]
fn sloppy_syntax_matches_bun() {
    common::run_golden(GOLDEN, 3000, |source| evaluate_named_script_result(source, "sloppy_syntax_case.js", "R"));
}
