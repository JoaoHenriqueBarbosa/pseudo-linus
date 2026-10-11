//! A gravação de função nativa sem transição de `Structure` (`putDirectNativeFunctionWithoutTransition`) só vale para
//! objeto com estrutura própria, como no C++. Num objeto comum, cuja estrutura é a compartilhada do global, ela punha a
//! função em todo objeto vazio: o `require` do corredor dos goldens (`builtin`, `readFileSync`, `runInThisContext`)
//! aparecia em `{}`, em `Object.assign({}, 'ab')` e no `for-in` de qualquer objeto.
use zjsc::api::eval::{describe_exception, evaluate_run_in_this_context_with_caller, evaluate_script};

fn text(value: zjsc::runtime::js_value::JSValue) -> String {
    String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
}

const PROBE: &str = "(function () { var seen = []; for (var k in {}) seen.push(k); \
                     return JSON.stringify([Object.getOwnPropertyNames({}), seen, Object.keys(Object.assign({}, 'ab')), \
                     Object.keys(Object.getOwnPropertyDescriptors('a'))]) })()";

#[test]
fn empty_objects_stay_empty_in_the_cjs_runner() {
    let source = format!("globalThis.R = {PROBE};");
    let caller = "try { require(\"node:vm\").runInThisContext(require(\"node:fs\").readFileSync(\"case_source.js\", \"utf8\"){options}) } \
                  catch (e) { globalThis.R = 'lançou: ' + e }\n";
    let value = evaluate_run_in_this_context_with_caller(&source, None, "R", caller)
        .unwrap_or_else(|thrown| panic!("lançou: {}", describe_exception(&thrown)));
    assert_eq!(text(value), r#"[[],[],["0","1"],["0","length"]]"#);
}

#[test]
fn empty_objects_stay_empty_in_a_plain_script() {
    let value = evaluate_script(PROBE).unwrap_or_else(|thrown| panic!("lançou: {}", describe_exception(&thrown)));
    assert_eq!(text(value), r#"[[],[],["0","1"],["0","length"]]"#);
}
