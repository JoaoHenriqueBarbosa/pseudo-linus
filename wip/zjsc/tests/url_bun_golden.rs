//! Golden de `URL` contra o JavaScriptCore do bun: `tests/golden/url_bun.tsv` sai de `scripts/gen-url-golden.js`,
//! rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Só os casos de
//! [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas inclusivas) rodam: por ora a FORMA (descritores, nomes,
//! `length`, ordem das chaves, chamada sem `new`, argumentos ausentes, `this` alheio). O comportamento da análise
//! (URLs inválidas, base relativa, getters sobre URLs reais, `searchParams`, setters) entra na próxima fatia.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/url_bun.tsv");

const IN_SCOPE: &[(usize, usize)] = &[
    // Forma do global, do construtor e do protótipo; descritores dos acessores e dos estáticos; chamada sem `new`;
    // `new URL()` sem argumento.
    (0, 32),
    // `canParse()` e `parse()` sem argumento, `createObjectURL()` sem argumento, `revokeObjectURL`.
    (84, 87),
    // `this` alheio nos getters e em `toString`/`toJSON`.
    (91, 116),
    // Setters: `search`, `href` e os demais acessores de `URLDecomposition`.
    (305, 305),
    (307, 650),
    (653, 658),
    // Host `xn--` inválido (construtor, base, href, setters) e `createObjectURL`/`revokeObjectURL`.
    (725, 1011),
];

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line))
}

#[test]
fn url_matches_bun() {
    let scoped: String = GOLDEN.lines().enumerate().filter(|&(number, _)| in_scope(number)).map(|(_, line)| format!("{line}\n")).collect();
    let expected: usize = IN_SCOPE.iter().map(|&(first, last)| last - first + 1).sum();
    assert_eq!(scoped.lines().count(), expected, "faixa de IN_SCOPE fora do golden");
    common::check(&scoped, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "url_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
