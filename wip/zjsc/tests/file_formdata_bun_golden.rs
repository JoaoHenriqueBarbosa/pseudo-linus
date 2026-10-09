//! Golden de `FormData` (e `File`) contra o JavaScriptCore do bun: `tests/golden/file_formdata_bun.tsv` sai de
//! `scripts/gen-file-formdata-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Só os casos de [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas inclusivas) rodam.
//! Ficam de fora, por LACUNA: o corpo multipart de `Response`/`Request` (216, 217).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/file_formdata_bun.tsv");

const IN_SCOPE: &[(usize, usize)] = &[
    // Forma dos globais `File` e `FormData`: descritor, `length`, `name`, protótipo, chamada sem `new`.
    (0, 17),
    // `File`: relação com `Blob`, construtor, `lastModified`, `slice`, acessores.
    (18, 68),
    // Forma do protótipo, `this` alheio no getter e nos métodos (inclusive o de `Blob`), `FormData.from`.
    (69, 75),
    // Construtor e subclasse, `append`, `get`, `getAll`, `has`, `length`, `set`, `delete`, o ramo `Blob`/`File`
    // (cópia a cada `get`, nome, `lastModified`, terceiro argumento), iteração viva, `forEach`, `toJSON`.
    (76, 183),
    // `FormData.from`: erros de argumento (input vazio, tipo inválido, boundary inválido), `Blob` como input,
    // boundary vazio, e os erros de corpo malformado com o sufixo `while parsing FormData`.
    (184, 215),
];

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line))
}

#[test]
fn file_formdata_matches_bun() {
    let scoped: String = GOLDEN.lines().enumerate().filter(|&(number, _)| in_scope(number)).map(|(_, line)| format!("{line}\n")).collect();
    let expected: usize = IN_SCOPE.iter().map(|&(first, last)| last - first + 1).sum();
    assert_eq!(scoped.lines().count(), expected, "faixa de IN_SCOPE fora do golden");
    common::check(&scoped, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "file_formdata_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
