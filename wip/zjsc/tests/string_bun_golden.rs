//! Golden de String, Array.prototype.join, String.raw e template literals contra o JavaScriptCore do bun:
//! `tests/golden/string_bun.tsv` sai de `scripts/gen-string-golden.js`, rodado no bun 1.4.2. Cada linha é o programa
//! canônico (prelúdio com o serializador `S` de `tests/golden/string_bun_prelude.js`, fatorado em `string.preludes.json`,
//! mais a expressão sob `try`), o texto que `S` devolve para o valor dela, ou `throw Nome: mensagem`, e a quinta coluna
//! com o modo e o mapa de posições. Cobre at/codePointAt, isWellFormed/toWellFormed, padStart/padEnd, split com regex e
//! limite, replace com padrões `$` e funções, matchAll, normalize, localeCompare (en, pt, sv, de),
//! toLocaleUpperCase('tr'), `Array.prototype.join`, `String.raw` e template literals. O que `builtins_bun_golden.rs` já
//! cobre de String (uma passada por método, com poucos argumentos) não é repetido aqui: este golden varre a combinação
//! de bases, índices, separadores, limites, locales e padrões de substituição.
mod common;

const GOLDEN: &str = include_str!("golden/string_bun.tsv");
const PRELUDES: &str = include_str!("golden/string.preludes.json");

#[test]
fn string_array_join_and_templates_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 8000, "string_case.js");
}
