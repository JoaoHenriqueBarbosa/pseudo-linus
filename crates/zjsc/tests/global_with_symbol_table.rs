//! `with (globalThis) { var x = 1 }` escreve pelo `JSGlobalObject::put`, que consulta a `SymbolTable` do global antes
//! do `Base::put`. Medido no bun 1.4.2: o script seguinte vê `typeof wa === 'number'`.
use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn result_of(scripts: &[&str]) -> String {
    let (errors, value) = evaluate_script_sequence_result(scripts, "eval_case.js", "globalThis.R");
    assert!(errors.is_empty(), "erros: {errors:?}");
    let value = value.unwrap_or_else(|_| panic!("exceção ao ler R"));
    String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
}

#[test]
fn var_written_inside_with_global_reaches_symbol_table() {
    let text = result_of(&["with (globalThis) { var wa = 1 }", "globalThis.R = typeof wa"]);
    assert!(text.contains("number"), "{text}");
}

#[test]
fn assignment_inside_with_global_updates_existing_var() {
    let text = result_of(&["var wb = 1;", "with (globalThis) { wb = 2 }", "globalThis.R = String(wb)"]);
    assert!(text.contains('2'), "{text}");
}
