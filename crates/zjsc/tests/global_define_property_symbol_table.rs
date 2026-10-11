//! `Object.defineProperty(globalThis, 'gx', {value: 9})` sobre `var`/função do script global: o binding vive na
//! `SymbolTable` do global e `JSGlobalObject::defineOwnProperty` grava o valor ali. Resultados medidos no bun 1.4.2
//! (`#9` em todos), conferidos também no golden `global_semantics_bun.tsv`.
use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const READ: &str = "try { globalThis.R = String((Object.defineProperty(globalThis, 'gx', { value: 9 }), gx)) } catch (e) { globalThis.R = e.name + ': ' + e.message }";

fn run(declaration: &str) -> String {
    let scripts = [declaration, READ];
    let (_, value) = evaluate_script_sequence_result(&scripts, "eval_case.js", "globalThis.R");
    String::from_utf8_lossy(&value.expect("R").to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
}

#[test]
fn define_property_value_updates_global_var_binding() {
    for declaration in [
        "var gx = 1;",
        "var gx;",
        "function gx() {}",
        "var gx = 1; var gx = 2;",
        "function gx() {} var gx;",
        "var gx; function gx() {}",
    ] {
        assert_eq!(run(declaration), "9", "{declaration}");
    }
}

#[test]
fn define_property_writable_false_makes_binding_read_only() {
    let scripts = [
        "var gx = 1;",
        "Object.defineProperty(globalThis, 'gx', { writable: false }); gx = 5; globalThis.R = String(gx)",
    ];
    let (_, value) = evaluate_script_sequence_result(&scripts, "eval_case.js", "globalThis.R");
    let text = String::from_utf8_lossy(&value.expect("R").to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "1");
}
