//! Golden de métodos legados e Annex B de objeto e string contra o JavaScriptCore do bun:
//! `tests/golden/annexb_methods_bun.tsv` sai de `scripts/gen-annexb-methods-golden.js`, rodado no bun 1.4.2 (TZ=UTC).
//! Cada linha é uma única chamada `T(function () { ... })` sobre o prelúdio comum (S, N, C, T) e o texto da variável global
//! `R` que ela grava (valor serializado ou `Nome: mensagem` do erro). Cobre `__defineGetter__` e irmãos em receptores
//! variados, o getter e o setter de `__proto__` em receptores exóticos, os métodos HTML de String, `escape` e `unescape`,
//! `getYear`, `setYear` e `toGMTString`, as estáticas legadas de RegExp e `compile`, `Function.prototype.caller` e
//! `arguments`, e `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`, `toLocaleString`, `valueOf` e `toString`.
mod common;

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/annexb_methods_bun.tsv");
const PRELUDES: &str = include_str!("golden/annexb_methods.preludes.json");

#[test]
fn annexb_methods_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| {
        set_time_zone_spec_override(Some("UTC"));
        evaluate_named_script_result(source, "annexb_methods_case.js", "R")
    });
}
