//! O ramo JSONP de `Interpreter::executeProgram`: um programa que é só `var x = JSON`, `a.b = JSON`, `a[n] = JSON`
//! ou `f(JSON)` (separados por `;`) não passa pelo parser de JS. Os casos de `let` e de duplicata foram medidos no
//! bun 1.4.2 com `vm.runInThisContext` em sequência no mesmo global.
use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// Roda os scripts em sequência no mesmo global e devolve os erros (`índice:texto`) e o texto de `globalThis.R`.
fn run(scripts: &[&str]) -> (Vec<String>, String) {
    let (errors, value) = evaluate_script_sequence_result(scripts, "jsonp_case.js", "globalThis.R");
    let text = String::from_utf8_lossy(&value.expect("R").to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    (errors, text)
}

#[test]
fn var_declaration_over_global_lexical_does_not_throw() {
    // `let x` fica no ambiente léxico global; o JSONP `var x = 2` só cria a variável no objeto global.
    let (errors, text) = run(&["let x=1;", "var x = 2", "globalThis.R = String(globalThis.x) + ',' + String(x)"]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "2,1");
}

#[test]
fn var_without_initializer_is_real_js_and_throws_duplicate() {
    let (errors, _) = run(&["let x=1;", "var x;", "globalThis.R = 'ok'"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("1:") && errors[0].contains("Can't create duplicate variable: 'x'"), "{errors:?}");
}

#[test]
fn var_with_trailing_expression_is_real_js_and_throws_duplicate() {
    let (errors, _) = run(&["let x=1;", "var x = 2; 1", "globalThis.R = 'ok'"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("1:") && errors[0].contains("Can't create duplicate variable: 'x'"), "{errors:?}");
}

#[test]
fn call_dot_and_lookup_entries_run_against_the_global() {
    let (errors, text) = run(&[
        "var log = []; var o = {}; function f(v) { log.push(v); }",
        "f({\"a\":1}); o.k = [1,2]; o.list = [0]; o.list[1] = 7;",
        "globalThis.R = JSON.stringify([log, o])",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "[[{\"a\":1}],{\"k\":[1,2],\"list\":[0,7]}]");
}

#[test]
fn unknown_head_variable_on_a_later_entry_throws_reference_error() {
    let (errors, _) = run(&["var a = {};", "a.q = 1; zz.c = 1;", "globalThis.R = String(a.q)"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("zz is not defined"), "{errors:?}");
}

#[test]
fn assigning_to_a_let_variable_through_jsonp_checks_tdz_and_stores() {
    let (errors, text) = run(&["let y = 1;", "y = 5;", "globalThis.R = String(y)"]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "5");
}

// Casos medidos no bun 1.4.2 com `vm.runInThisContext` em sequência no mesmo global. O bun acrescenta ao `TypeError`
// de base `undefined`/`null` o trecho do fonte do frame JS que chamou (`evaluating 'vm.runInThisContext(s)'`); aqui
// não há frame JS acima do script, então só o início da mensagem é comparado. A entrada `f(JSON)` (`Call`) nunca é
// JSONP (`needsFullSourceInfo`), por isso `f(1)` com `f` não função sai com o texto do bytecode, não é medido aqui.

#[test]
fn undefined_or_null_base_throws_not_an_object() {
    for (setup, put, expected) in [
        ("var u1;", "u1.a = 1", "undefined is not an object"),
        ("var u2;", "u2.a.b = 1", "undefined is not an object"),
        ("var u3;", "u3.a.b.c = 1", "undefined is not an object"),
        ("var n1 = null;", "n1.a = 1", "null is not an object"),
        ("var n2 = null;", "n2.a.b = 1", "null is not an object"),
        ("var o1 = {};", "o1.x.y = 1", "undefined is not an object"),
        ("var o2 = {x:null};", "o2.x.y = 1", "null is not an object"),
        ("var o3 = {x:{y:null}};", "o3.x.y.z = 1", "null is not an object"),
        ("var o4 = {a:[]};", "o4.a[0][1] = 2", "undefined is not an object"),
        ("var o5 = {a:[]};", "o5.a[0].x = 2", "undefined is not an object"),
        ("var u4;", "u4[1] = 2", "undefined is not an object"),
        ("var n3 = null;", "n3[1] = 2", "null is not an object"),
        ("var o6 = Object.freeze({});", "o6.a.b = 1", "undefined is not an object"),
        ("let l1 = null;", "l1.a = 1", "null is not an object"),
        ("let l2;", "l2.a = 1", "undefined is not an object"),
        ("let l3 = {};", "l3.a.b = 1", "undefined is not an object"),
        ("var a1 = {};", "a1.zz.b = 1", "undefined is not an object"),
    ] {
        let (errors, _) = run(&[setup, put, "globalThis.R = 1"]);
        assert_eq!(errors.len(), 1, "{put}: {errors:?}");
        assert!(errors[0].starts_with("1:") && errors[0].contains(expected), "{put}: {errors:?}");
    }
}

#[test]
fn unknown_head_variable_is_a_reference_error() {
    // Na primeira entrada é o `goto failedJSONP` (o JS de verdade dá o mesmo texto); nas seguintes é o
    // `createUndefinedVariableError` do próprio ramo JSONP.
    for script in ["zz.a = 1", "zz[1] = 2", "var a2 = {}; a2.q = 1; zz.a = 1", "var a3 = {}; a3.q = 1; zz[1] = 2"] {
        let (errors, _) = run(&[script, "globalThis.R = 1"]);
        assert_eq!(errors.len(), 1, "{script}: {errors:?}");
        assert!(errors[0].contains("zz is not defined"), "{script}: {errors:?}");
    }
}

#[test]
fn tdz_head_is_missing_on_later_entries_and_tdz_error_on_the_first_or_single_name() {
    // `let` que lançou antes de inicializar: o slot vazio do ambiente léxico conta como ausente no percurso.
    for (put, expected) in [
        ("var q1 = {}; q1.z = {}; t1.a = {}", "t1 is not defined"),
        ("var q2 = {}; t2[0] = {}", "t2 is not defined"),
        ("var q3 = {}; t3.x.y = {}", "t3 is not defined"),
        ("t4.a = {}", "Cannot access 't4' before initialization."),
        ("t5.x.y = {}", "Cannot access 't5' before initialization."),
        ("t6[0] = {}", "Cannot access 't6' before initialization."),
        ("t7 = {}", "Cannot access 't7' before initialization."),
        ("var q4 = {}; t8 = {}", "Cannot access 't8' before initialization."),
    ] {
        let name = put.split(['.', '[', ' ']).find(|part| part.starts_with('t') && part.len() == 2).expect("nome");
        let (errors, _) = run(&[&format!("throw 1; let {name};"), put, "globalThis.R = 1"]);
        assert_eq!(errors.len(), 2, "{put}: {errors:?}");
        assert!(errors[1].starts_with("1:") && errors[1].contains(expected), "{put}: {errors:?}");
    }
}

#[test]
fn put_on_a_primitive_is_silent_without_a_setter() {
    let (errors, text) = run(&[
        "var s1 = 'abc'; var n1 = 5; var b1 = true; var y1 = Symbol(); var g1 = 10n;",
        "s1.c = 1; n1.c = 1; b1.c = 1; y1.c = 1; g1.c = 1; s1.length = 1; s1[0] = 'z'; s1[5] = 'z'; n1[2] = 1",
        "globalThis.R = [s1, s1.c, n1.c, b1.c, y1.c, g1.c, s1.length].join(',')",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "abc,,,,,,3");
}

#[test]
fn put_on_a_primitive_calls_the_prototype_setter_with_the_primitive_as_this() {
    let (errors, text) = run(&[
        "Object.defineProperty(String.prototype,'sa',{set(v){'use strict'; globalThis.RA = typeof this + ':' + this + ':' + JSON.stringify(v)},configurable:true}); var sA='hi';",
        "Object.defineProperty(Number.prototype,'sb',{set(v){'use strict'; globalThis.RB = typeof this + ':' + this},configurable:true}); var nB=5;",
        "Object.defineProperty(Object.prototype,'sd',{set(v){'use strict'; globalThis.RD = typeof this},configurable:true}); var nD=5;",
        "Object.defineProperty(Symbol.prototype,'se',{set(v){'use strict'; globalThis.RE = typeof this},configurable:true}); var sE=Symbol();",
        "Object.defineProperty(BigInt.prototype,'sf',{set(v){'use strict'; globalThis.RF = typeof this},configurable:true}); var bF=10n;",
        "sA.sa = {\"k\":1}; nB.sb = 1; nD.sd = 1; sE.se = 1; bF.sf = 1",
        "globalThis.R = [RA, RB, RD, RE, RF].join('|')",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "string:hi:{\"k\":1}|number:5|number|symbol|bigint");
}

#[test]
fn put_on_a_primitive_by_index_is_intercepted_by_a_prototype_setter() {
    // Mesmo com o índice dentro do comprimento da string: só `length` é somente leitura em `putToPrimitive`.
    let (errors, text) = run(&[
        "Object.defineProperty(String.prototype,'7',{set(v){'use strict'; globalThis.RG = typeof this + v},configurable:true}); var sG='hi';",
        "Object.defineProperty(Number.prototype,'3',{set(v){'use strict'; globalThis.RH = typeof this + v},configurable:true}); var nH=1;",
        "Object.defineProperty(String.prototype,'1',{set(v){'use strict'; globalThis.RI = 'called'},configurable:true}); var sI='hi';",
        "sG[7] = 3; nH[3] = 4; sI[1] = 4",
        "globalThis.R = [RG, RH, RI].join('|')",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "string3|number4|called");
}

#[test]
fn put_on_a_primitive_propagates_a_throwing_setter_and_ignores_readonly_or_getter_only() {
    let (errors, _) = run(&[
        "Object.defineProperty(Boolean.prototype,'sc',{set(v){throw new RangeError('bs')},configurable:true}); var bC=true;",
        "bC.sc = 1",
        "globalThis.R = 1",
    ]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("1:") && errors[0].contains("bs"), "{errors:?}");

    let (errors, text) = run(&[
        "Object.defineProperty(Number.prototype,'ro',{value:1,writable:false,configurable:true}); var nK=1;",
        "Object.defineProperty(String.prototype,'e',{get(){return 1},configurable:true}); var sE2='x';",
        "Number.prototype.dd = 1; var nL=1;",
        "nK.ro = 4; sE2.e = 7; nL.dd = 4",
        "globalThis.R = [nK.ro, sE2.e, nL.dd].join(',')",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "1,1,1");
}

// Medido no bun 1.4.2 (`(0,eval)(src)`): em modo estrito toda escrita em primitivo que nenhum setter da cadeia
// intercepta lança "Attempted to assign to readonly property." (índice dentro ou fora da string, `length`, nome
// qualquer); com setter em `String.prototype['1']` o setter roda com o primitivo e nada lança. No sloppy é silêncio.
#[test]
fn strict_put_on_a_primitive_throws_readonly_unless_a_setter_intercepts_the_index() {
    let message = "Attempted to assign to readonly property.";
    for body in ["s[1] = 4", "s[9] = 4", "s.length = 4", "s.foo = 4", "n[1] = 4", "n.x = 4", "b.x = 4"] {
        let (errors, _) = run(&[
            "var s = 'hi', n = 5, b = true;",
            &format!("(function () {{ 'use strict'; {body} }})()"),
            "globalThis.R = 1",
        ]);
        assert_eq!(errors.len(), 1, "{body}: {errors:?}");
        assert!(errors[0].starts_with("1:") && errors[0].contains(message), "{body}: {errors:?}");
    }

    let (errors, text) = run(&[
        "var s = 'hi';",
        "Object.defineProperty(String.prototype,'1',{set(v){'use strict'; globalThis.RS = typeof this + v},configurable:true});",
        "(function () { 'use strict'; s[1] = 4 })()",
        "globalThis.R = RS",
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "string4");

    let (errors, text) = run(&["var s = 'hi'; s[1] = 4; s[9] = 4; s.length = 4;", "globalThis.R = s"]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(text, "hi");
}
