//! `delete globalThis.x` sobre binding da `SymbolTable` do global (`JSSymbolTableObject::deleteProperty`).
//! Medido no bun 1.4.2 com `vm.runInThisContext`: `var g = 1; delete globalThis.g` dá `false` (inclusive com o `var`
//! dentro de um `try`), a propriedade segue como `{writable, enumerable, configurable: false}`; o `var` criado por
//! `eval` sloppy e a atribuição implícita continuam apagáveis (`true`).
use zjsc::api::eval::{describe_exception, evaluate_script};

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", describe_exception(&thrown)));
    value.is_true()
}

#[test]
fn script_var_is_not_deletable_through_global_this() {
    assert!(is_true("var g1 = 1; delete globalThis.g1 === false && g1 === 1"));
    assert!(is_true("try { var g2 = 1; } catch (e) {} delete globalThis.g2 === false"));
    assert!(is_true("function g3() {} delete globalThis.g3 === false"));
    assert!(is_true("let g4 = 1; delete globalThis.g4 === true"));
}

#[test]
fn every_delete_path_hits_the_symbol_table() {
    // `Reflect.deleteProperty` (builtin JS que chega ao `[[Delete]]` do global).
    assert!(is_true("var g7 = 1; Reflect.deleteProperty(globalThis, 'g7') === false && g7 === 1"));
    // `delete x` sloppy resolve o escopo global e consulta a `SymbolTable`.
    assert!(is_true("var g8 = 1; delete g8 === false && g8 === 1"));
    assert!(is_true("function g9() {} delete g9 === false"));
    // `this` no topo do script é o `JSGlobalProxy`, que encaminha ao alvo.
    assert!(is_true("var g10 = 1; delete this.g10 === false && g10 === 1"));
    // Leitura posterior por `(0, eval)` ainda enxerga o binding.
    assert!(is_true("var g11 = 1; delete globalThis.g11; (0, eval)('g11') === 1"));
    assert!(is_true("var g12 = 1; delete globalThis.g12; Object.getOwnPropertyDescriptor(globalThis, 'g12').configurable === false"));
    // `delete` por chave computada.
    assert!(is_true("var g13 = 1; delete globalThis['g13'] === false"));
}

#[test]
fn strict_delete_of_symbol_table_binding_throws() {
    assert!(is_true(
        "var g14 = 1; (function () { 'use strict'; try { delete globalThis.g14; return false; } \
         catch (e) { return e instanceof TypeError && e.message === 'Unable to delete property.'; } })()"
    ));
    assert!(is_true("var g15 = 1; Reflect.deleteProperty(globalThis, 'g15') === false"));
}

#[test]
fn eval_var_and_implicit_global_stay_deletable() {
    assert!(is_true("eval('var g5 = 1'); delete globalThis.g5 === true"));
    assert!(is_true("g6 = 1; delete globalThis.g6 === true"));
}
