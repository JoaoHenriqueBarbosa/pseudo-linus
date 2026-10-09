//! Golden de Annex B e recursos legados contra o JavaScriptCore do bun: `tests/golden/annexb_bun.tsv` sai de
//! `scripts/gen-annexb-golden.js`, rodado no bun 1.4.2 (TZ=UTC). Cada linha é um programa sloppy (JSON) e o texto da
//! variável global `R` que ele grava (valor ou `Nome: mensagem` do erro). Cobre `__proto__`, `__defineGetter__` e
//! irmãos, métodos HTML de String, `substr`, `trimLeft`, `escape`, Date legado, RegExp legado, comentários HTML,
//! octais, funções em blocos, `with`, `arguments.callee`, `caller`, `toStringTag`, `split` e `sort`.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/annexb_bun.tsv");

#[test]
fn annexb_matches_bun() {
    common::run_golden(GOLDEN, 700, |source| {
        set_time_zone_spec_override(Some("UTC"));
        // Como o filho do gerador: qualquer exceção não capturada do programa (SyntaxError de `R = 1 --> 0`, de
        // `(var n = 0; ...)`, ou ReferenceError de `--x`) faz o `catch` do preload zerar `R`, e o golden registra
        // `<undefined>`, mesmo que o programa já a tivesse atribuído.
        let (errors, result) = evaluate_script_sequence_result(&[source], "annexb_case.js", "typeof R === 'undefined' ? undefined : R");
        if errors.is_empty() { result } else { Ok(zjsc::runtime::js_value::JSValue::undefined()) }
    });
}
