//! Identidade do `globalThis`: o valor visível ao JS é o `JSGlobalProxy` (`JSGlobalObject::globalThis`), nunca
//! a célula de escopo do `JSGlobalObject`. `to_this_strict` devolve `undefined` para a célula de escopo, então
//! qualquer caminho que vazasse a célula crua faria `Object.prototype.hasOwnProperty.call(globalThis, ...)` falhar.
//! Medido no bun 1.4.2: `Object.prototype.toString.call(globalThis)` é `[object Object]` e as quatro formas de
//! obter o global (`globalThis`, `this` sloppy, `Function('return this')()`, `(0, eval)('this')`) são idênticas.
use zjsc::api::eval::evaluate_script;

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

#[test]
fn every_path_yields_the_same_global() {
    assert!(is_true(
        "(function () { return this; })() === globalThis && Function('return this')() === globalThis \
         && (0, eval)('this') === globalThis && globalThis.globalThis === globalThis"
    ));
    assert!(is_true("(function () { 'use strict'; return this; })() === undefined"));
}

#[test]
fn natives_accept_the_global_as_this() {
    assert!(is_true("Object.prototype.toString.call(globalThis) === '[object Object]'"));
    assert!(is_true("Object.prototype.hasOwnProperty.call(globalThis, 'x') === false"));
    assert!(is_true("var gx = 1; Object.prototype.hasOwnProperty.call(globalThis, 'gx') === true"));
    assert!(is_true("Object.prototype.toString.call(Function('return this')()) === '[object Object]'"));
}
