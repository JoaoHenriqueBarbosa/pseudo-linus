//! Regressão: `String.prototype.replace` com callback sobre dezenas de milhares de matches. Os resultados do `exec` ficavam
//! só num `Vec` do código nativo, sem raiz, e uma coleta durante o callback varria os que ainda não tinham sido lidos
//! (pânico `asObject em objeto que o registro ainda não expõe`, achado pelo baseline de desempenho em `bench/js/regex.js`).
use zjsc::api::eval::evaluate_named_script_reporting_uncaught;
use zjsc::wtf::text::conversion_mode::ConversionMode;

#[test]
fn replace_with_callback_survives_collections_over_many_matches() {
    let source = "var text = ''; for (var i = 0; i < 60000; i++) text += 'foo_bar-' + i + '\\n'; \
                  var R = String(text.replace(/foo_(\\w+)-(\\d+)/g, function (_, a, b) { return a.toUpperCase() + b; }).length)";
    let read = evaluate_named_script_reporting_uncaught(source, "regexp_replace_gc.js", "R").expect("sem exceção solta");
    let value = read.unwrap_or_else(|_| panic!("leitura de R"));
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    // 60000 linhas "BAR<n>\n": 3 + dígitos de n + 1, somadas.
    let expected: usize = (0..60000usize).map(|n| 3 + n.to_string().len() + 1).sum();
    assert_eq!(text, expected.to_string());
}
