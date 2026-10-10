//! Golden do formato de pilha e das mensagens de erro do V8, medido no Chromium 154 do oráculo
//! (`scripts/gen-v8-stack-golden.sh`): o `probe.js` roda como script clássico de página, com os modos do V8 que o
//! navegador liga, e cada caso deixado em `__out` tem de sair igual ao do oráculo.

mod common;

use zjsc::api::eval::evaluate_browser_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const PROBE: &str = include_str!("golden/v8_stack/probe.js");
const EXPECTED: &str = include_str!("golden/v8_stack/expected.json");

#[test]
fn v8_stack_matches_chromium() {
    let expected = common::json_string_array(EXPECTED.trim()).expect("golden em JSON");
    let value = evaluate_browser_script_result(PROBE, "file:///p/probe.js", "__out").expect("a sonda não lança");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    let actual = common::json_string_array(&text).expect("__out em JSON");
    assert_eq!(actual.len(), expected.len(), "quantidade de casos");
    let mut divergent = Vec::new();
    for (index, (want, got)) in expected.iter().zip(&actual).enumerate() {
        if want != got {
            divergent.push(format!(
                "caso {index}:\n  esperado: {:?}\n  obtido:   {:?}",
                String::from_utf16_lossy(want),
                String::from_utf16_lossy(got)
            ));
        }
    }
    assert!(divergent.is_empty(), "{} de {} divergem do Chromium:\n{}", divergent.len(), expected.len(), divergent.join("\n"));
}
