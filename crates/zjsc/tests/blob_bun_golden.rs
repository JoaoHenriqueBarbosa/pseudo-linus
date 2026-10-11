//! Golden de `Blob` contra o JavaScriptCore do bun: `tests/golden/blob_bun.tsv` sai de `scripts/gen-blob-golden.js`,
//! rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava, lida depois do
//! esvaziamento das promessas. Divergência conhecida: `delete`, `exists`, `formData`, `image`, `stat`, `stream`, `unlink`,
//! `write` e `writer` só têm a forma (ver `src/runtime/blob.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/blob_bun.tsv");

#[test]
fn blob_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 120, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "blob_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}

/// A constante canônica do tipo (medido no bun 1.4.2): `text/plain` ganha `;charset=utf-8` em `Blob`, `File` e `slice`;
/// a busca é no texto como veio, então `Text/plain` só vira `text/plain`; `text/csv` não muda.
#[test]
fn blob_type_takes_the_canonical_mime_constant() {
    let source = "var R = [new Blob(['x'], {type: 'text/plain'}).type, new File(['x'], 'a', {type: 'application/javascript'}).type, \
                  new Blob(['x']).slice(0, 1, 'text/jsx').type, new Blob(['x'], {type: 'Text/plain'}).type, new Blob(['x'], {type: 'text/csv'}).type].join('|')";
    let value = zjsc::api::eval::evaluate_named_script_result(source, "blob_case.js", "R").expect("R");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "text/plain;charset=utf-8|text/javascript;charset=utf-8|text/javascript;charset=utf-8|text/plain|text/csv");
}
