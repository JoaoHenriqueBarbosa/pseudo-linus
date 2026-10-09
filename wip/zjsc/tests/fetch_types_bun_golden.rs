//! Golden de `Request` e `Response` contra o JavaScriptCore do bun: `tests/golden/fetch_types_bun.tsv` sai de
//! `scripts/gen-fetch-types-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava. Só os casos de [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas inclusivas, menos os de
//! [`EXCLUDED`]) rodam. `Request` e `Response` existem; a LACUNA (única, dos streams) deixa de fora o que lê `body`.
//! Ficam de fora, por LACUNA (única, dos streams): `Response.body` com corpo (um `ReadableStream`), `textStream`, o corpo de
//! `ReadableStream`, e as linhas que serializam `Response.error().body` ou o `body` de `Response.redirect` (o `{}` do
//! golden é a stream; o `Location` normalizado é conferido por elas e entra quando a stream existir).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/fetch_types_bun.tsv");

const IN_SCOPE: &[(usize, usize)] = &[
    // Descritor do global, `length`, `name`.
    (127, 130),
    // Chaves do protótipo, métodos e acessores (descritor, `this` alheio), construtor sem `new`, subclasse, `toStringTag`.
    (131, 180),
    // Corpo por tipo: `text()` de cada tipo de entrada e os cabeçalhos implícitos (menos os que leem `body`).
    (181, 222),
    // Cabeçalhos do `ResponseInit` e `headers` estável.
    (223, 239),
    // `status`, `statusText`, `ok` e o intervalo de `status` com o `RangeError` exato.
    (371, 443),
    // Estáticos `error`, `json` e `redirect` (menos os que leem `body`).
    (444, 447),
    (449, 473),
    (484, 486),
    (490, 491),
    (499, 508),
    // `clone`, leituras e `bodyUsed`/`Body already used`.
    (516, 524),
    (526, 558),
    (559, 559),
    (561, 567),
    (569, 575),
    (577, 583),
    (585, 598),
    (599, 599),
    (601, 606),
    (617, 620),
    // Corpos que viram texto, `bytes`/`arrayBuffer` de views.
    (725, 745),
    // `Request`: forma, brand check, corpo por tipo, cabeçalhos de `init` (menos os que leem `body`).
    (0, 126),
    // `Request`: construtor (URL, `method`, `body`, opções, `signal`, `Request` de `Request`) e `clone`.
    (240, 370),
    // Brand check cruzado `Request`/`Response` e leituras do corpo de `Request` (menos as que leem `body`).
    (509, 515),
    (621, 654),
    (656, 662),
    (664, 670),
    (672, 678),
    (680, 686),
    (688, 694),
    (696, 701),
    (712, 724),
    // `formData()` de multipart malformado: `TypeError` `FormData parse error ...` (`ERR_FORMDATA_PARSE_ERROR`).
    (751, 768),
    // `new Request(objeto)`: `url` presente, `toString` próprio, `href`, `URL`, getter que lança, `Proxy`.
    (769, 788),
];

/// Linhas dentro das faixas que leem `body` (`ReadableStream`) ou constroem um `ReadableStream`.
const EXCLUDED: &[usize] = &[
    181, 183, 185, 187, 189, 191, 193, 195, 197, 199, 201, 203, 205, 207, 209, 211, 217, 218, 221,
    // `Request`: as que leem `body.constructor.name` e a que constrói um `ReadableStream`.
    68, 70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90, 92, 94, 96, 98, 104, 105, 108,
];

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line)) && !EXCLUDED.contains(&line)
}

#[test]
fn fetch_types_matches_bun() {
    let scoped: String = GOLDEN.lines().enumerate().filter(|&(number, _)| in_scope(number)).map(|(_, line)| format!("{line}\n")).collect();
    let expected: usize = (0..GOLDEN.lines().count()).filter(|&number| in_scope(number)).count();
    assert_eq!(scoped.lines().count(), expected, "faixa de IN_SCOPE fora do golden");
    common::check(&scoped, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "fetch_types_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
