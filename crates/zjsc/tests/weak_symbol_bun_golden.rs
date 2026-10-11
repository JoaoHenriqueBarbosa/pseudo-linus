//! Golden de WeakRef, FinalizationRegistry, WeakMap e WeakSet com chaves symbol contra o JavaScriptCore do bun:
//! `tests/golden/weak_symbol_bun.tsv` sai de `scripts/gen-weak-symbol-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (prelúdio fatorado) e o texto da variável global `R` que ele grava. Só a parte síncrona e determinística:
//! construtores com argumentos inválidos (Symbol.for registrado como chave proibida, símbolo comum e well-known
//! permitidos), deref, register/unregister com token, alvo igual ao held, cleanupSome, Symbol.for/keyFor em grade
//! (strings estranhas, coerção, valueOf que lança) e a descrição de Symbol(). Nada depende de o coletor rodar.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/weak_symbol_bun.tsv");
const PRELUDES: &str = include_str!("golden/weak_symbol.preludes.json");

#[test]
fn weak_symbol_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "weak_symbol_case.js", "R"));
}
