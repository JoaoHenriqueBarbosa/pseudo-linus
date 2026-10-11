//! `Object.defineProperty(globalThis, 'dp', {...})` no mesmo script de um `var dp` / `var dp = 2` / `function dp() {}`:
//! a declaração é içada (`ProgramExecutable::initializeGlobalProperties` cria a entrada na `SymbolTable` do global
//! antes do corpo rodar), então o `defineProperty` cai em `JSGlobalObject::defineOwnProperty` sobre a variável: grava
//! o valor ali e mantém os atributos da variável (gravável, enumerável, não configurável), seja qual for o descritor.
//! Resultados do golden `global_semantics_bun.tsv` (medido no JavaScriptCore do bun 1.4.2).
use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const READ: &str = "try { globalThis.R = String((function () { try { return typeof dp + '/' + String(dp) } catch (e) { return e.name } })() + '|' + String(globalThis.W) + '|' + (function (o) { if (!o) return 'none'; return ('value' in o ? 'v:' + (typeof o.value === 'function' ? 'fn' : String(o.value)) : 'a:' + typeof o.get + '/' + typeof o.set) + ' w' + o.writable + ' e' + o.enumerable + ' c' + o.configurable })(Object.getOwnPropertyDescriptor(globalThis, 'dp'))) } catch (e) { globalThis.R = e.name + ': ' + e.message }";

fn run(descriptor: &str, declaration: &str) -> String {
    let first = format!("Object.defineProperty(globalThis, 'dp', {{ {descriptor} }});\n{declaration}");
    let scripts = [first.as_str(), READ];
    let (_, value) = evaluate_script_sequence_result(&scripts, "eval_case.js", "globalThis.R");
    String::from_utf8_lossy(&value.expect("R").to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
}

#[test]
fn define_property_on_hoisted_global_declaration_keeps_variable_attributes() {
    let descriptors = ["value: 1", "value: 1, writable: true", "value: 1, writable: true, enumerable: true"];
    let cases = [
        ("var dp;", "number/1|undefined|v:1 wtrue etrue cfalse"),
        ("var dp = 2;", "number/2|undefined|v:2 wtrue etrue cfalse"),
        ("function dp() {}", "number/1|undefined|v:1 wtrue etrue cfalse"),
    ];
    for descriptor in descriptors {
        for (declaration, expected) in cases {
            assert_eq!(run(descriptor, declaration), expected, "{descriptor} / {declaration}");
        }
    }
}
