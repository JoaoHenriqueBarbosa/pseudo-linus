//! Espalhamento em chamadas e `new` ponta a ponta: `op_spread`, `op_new_array_with_spread` e as quatro
//! famílias `op_call_varargs`, `op_tail_call_varargs`, `op_construct_varargs` e `op_super_construct_varargs`
//! (`emitCallVarargs`, `emitConstructImpl`, `emitNewArrayWithSpread`, `Function.prototype.call/apply` com
//! espalhamento em `NodesCodegen.cpp`, e `sizeOfVarargs`/`sizeFrameForVarargs`/`loadVarargs` de
//! `Interpreter.cpp`). Os valores esperados são os da especificação e os do C++ (`Interpreter::maxArguments`
//! é `0x100000`; ao estourar, `RangeError: Maximum call stack size exceeded.`).
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como booleano.
fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

const JOIN: &str = "function f() { return Array.prototype.join.call(arguments, '-'); } var a = [1, 2, 3];";

fn with_join(body: &str) -> String {
    format!("{JOIN} {body}")
}

#[test]
fn spread_in_call_arguments() {
    assert!(is_true(&with_join(
        "f(...a) === '1-2-3' && f(...a, 4) === '1-2-3-4' && f(0, ...a) === '0-1-2-3' \
         && f(...a, ...a) === '1-2-3-1-2-3' && f(...[]) === '' && f(...[], ...[]) === '' && f(9, ...[], 8) === '9-8'"
    )));
}

#[test]
fn spread_of_iterables() {
    assert!(is_true(&with_join(
        "function* g() { yield 1; yield 2; } \
         f(...'abc') === 'a-b-c' && f(...new Set([1, 2])) === '1-2' && f(...new Map([[1, 2]])) === '1,2' \
         && f(...g()) === '1-2' && f(...a.keys()) === '0-1-2' && f(...'') === ''"
    )));
}

#[test]
fn spread_of_holes_reads_undefined() {
    assert!(is_true(&with_join("f(...[1, , 3]) === '1--3'")));
    assert!(is_true("function f() { return arguments.length; } f(...[, ,]) === 2"));
}

#[test]
fn spread_with_custom_iterator_and_errors() {
    assert!(is_true(&with_join(
        "var it = { [Symbol.iterator]() { var i = 0; return { next() { return i < 2 ? { value: i++, done: false } : { done: true }; } }; } }; \
         f(...it) === '0-1'"
    )));
    assert!(is_true(&with_join(
        "var t; try { f(...{ [Symbol.iterator]() { throw 11; } }) } catch (e) { t = e } t === 11"
    )));
    assert!(is_true(&with_join(
        "var t; try { f(...undefined) } catch (e) { t = e instanceof TypeError } t === true"
    )));
    assert!(is_true(&with_join("var t; try { f(...5) } catch (e) { t = e instanceof TypeError } t === true")));
    assert!(is_true(&with_join("var t; try { f(...{}) } catch (e) { t = e instanceof TypeError } t === true")));
}

#[test]
fn spread_keeps_receiver_and_evaluation_order() {
    assert!(is_true(
        "var o = { v: 7, m() { return this.v + arguments.length; } }; var a = [1, 2]; o.m(...a) === 9 && o['m'](...a) === 9"
    ));
    assert!(is_true(
        "var log = []; function f() {} var a = [1]; \
         (log.push('callee'), f)((log.push('arg'), 0), ...(log.push('spread'), a)); log.join() === 'callee,arg,spread'"
    ));
    assert!(is_true("var o = { m() { return this === o; } }; var a = []; o?.m(...a) === true && o.m?.(...a) === true"));
}

#[test]
fn spread_in_new_and_super() {
    assert!(is_true(
        "function P(x, y) { this.s = x + y; } var a = [3, 4]; new P(...a).s === 7 && new P(1, ...[2]).s === 3"
    ));
    assert!(is_true("var d = new Date(...[2020, 0, 1]); d.getFullYear() === 2020"));
    assert!(is_true(
        "class A { constructor(...r) { this.r = r; } } class B extends A { constructor(...r) { super(...r, 9); } } \
         var b = new B(1, 2); b.r.join() === '1,2,9'"
    ));
    assert!(is_true("class A { constructor(...r) { this.r = r; } } class B extends A {} new B(...[1, 2, 3]).r.join() === '1,2,3'"));
    assert!(is_true(
        "class A { constructor(x) { this.x = x; } } class B extends A { constructor(a) { var f = () => super(...a); f(); } } \
         new B([5]).x === 5"
    ));
    assert!(is_true("function P() { return new.target; } var a = []; new P(...a) === P"));
}

#[test]
fn function_prototype_apply() {
    assert!(is_true(&with_join(
        "f.apply(null, a) === '1-2-3' && f.apply(null) === '' && f.apply(null, null) === '' \
         && f.apply(null, undefined) === '' && f.apply(null, []) === ''"
    )));
    assert!(is_true(&with_join(
        "f.apply(null, { length: 2, 0: 'a', 1: 'b' }) === 'a-b' && f.apply(null, { length: 3 }) === '--' \
         && f.apply(null, { length: '2', 0: 1, 1: 2 }) === '1-2' && f.apply(null, { length: -1 }) === '' \
         && f.apply(null, { length: 2.9, 0: 1, 1: 2 }) === '1-2' && f.apply(null, { length: NaN }) === '' \
         && f.apply(null, {}) === ''"
    )));
    assert!(is_true(&with_join(
        "function g() { return f.apply(null, arguments); } g(1, 2) === '1-2' && g() === '' \
         && (function (x) { x = 5; return f.apply(null, arguments); })(1, 2) === '5-2'"
    )));
    assert!(is_true(&with_join(
        "f.apply(null, new Uint8Array([1, 2])) === '1-2' && f.apply(null, (function () { return arguments; })(7, 8)) === '7-8'"
    )));
    assert!(is_true("function f() { return this; } f.apply(5, []) == 5 && f.apply(undefined, []) === globalThis"));
    assert!(is_true("function f() { 'use strict'; return this; } f.apply(5, []) === 5 && f.apply(undefined, []) === undefined"));
}

#[test]
fn function_prototype_apply_length_conversion() {
    assert!(is_true(&with_join(
        "f.apply(null, { length: { valueOf() { return 2; } }, 0: 1, 1: 2 }) === '1-2'"
    )));
    assert!(is_true(&with_join(
        "var t; try { f.apply(null, { length: { valueOf() { throw 4; } } }) } catch (e) { t = e } t === 4"
    )));
    assert!(is_true(&with_join(
        "var t; try { f.apply(null, { get length() { throw 6; } }) } catch (e) { t = e } t === 6"
    )));
    assert!(is_true(&with_join(
        "var t; try { f.apply(null, { length: Symbol() }) } catch (e) { t = e instanceof TypeError } t === true"
    )));
    assert!(is_true(&with_join(
        "var t; try { f.apply(null, { length: 1n }) } catch (e) { t = e instanceof TypeError } t === true"
    )));
    assert!(is_true(&with_join(
        "var t; try { f.apply(null, { length: 1, get 0() { throw 8; } }) } catch (e) { t = e } t === 8"
    )));
}

#[test]
fn function_prototype_apply_rejects_non_array_like() {
    let message = "'second argument to Function.prototype.apply must be an Array-like object'";
    for argument in ["1", "'str'", "true", "Symbol()", "1n"] {
        assert!(
            is_true(&with_join(&format!(
                "var t; try {{ f.apply(null, {argument}) }} catch (e) {{ t = e instanceof TypeError && e.message.startsWith({message}) }} t === true"
            ))),
            "{argument}"
        );
    }
}

#[test]
fn function_prototype_call_with_spread() {
    assert!(is_true(&with_join(
        "f.call(null, ...a) === '1-2-3' && f.call(...[null, 1, 2]) === '1-2' && f.call(null) === '' \
         && f.call(null, 5, ...a) === '5-1-2-3'"
    )));
    assert!(is_true("function f() { return this; } f.call(...[7]) == 7 && f.call(...[]) === globalThis"));
}

#[test]
fn call_and_apply_overridden_take_the_real_call() {
    assert!(is_true("function f() { return 1; } f.apply = function () { return 'x'; }; f.apply(null, [1]) === 'x'"));
    assert!(is_true("function f() { return 1; } f.call = function () { return 'y'; }; f.call(null, ...[1]) === 'y'"));
}

#[test]
fn tail_position_varargs() {
    assert!(is_true(&with_join(
        "function g(x) { return f(...x); } function h() { return f.apply(this, arguments); } \
         g(a) === '1-2-3' && h(4, 5) === '4-5'"
    )));
    assert!(is_true(
        "function sum(n, acc) { 'use strict'; return n === 0 ? acc : sum(...[n - 1, acc + n]); } sum(200, 0) === 20100"
    ));
}

#[test]
fn many_arguments() {
    assert!(is_true(
        "var big = []; for (var i = 0; i < 10000; i++) big.push(i); \
         Math.max(...big) === 9999 && Math.max.apply(null, big) === 9999 \
         && (function () { return arguments.length; })(...big) === 10000 \
         && (function () { return arguments.length; }).apply(null, { length: 5000 }) === 5000"
    ));
    assert!(is_true(
        "var big = new Array(1000).fill(1); String.fromCharCode(...big).length === 1000 && Math.min(0, ...big, 5) === 0"
    ));
}

#[test]
fn too_many_arguments_is_a_range_error() {
    let check = "e instanceof RangeError && e.message === 'Maximum call stack size exceeded.'";
    for source in [
        "f.apply(null, { length: 0x100001 })",
        "f.apply(null, { length: 2000000 })",
        "f.apply(null, { length: Infinity })",
        "f.apply(null, { length: 4294967296 })",
        "f.apply(null, { length: 1e300 })",
    ] {
        assert!(
            is_true(&with_join(&format!("var t; try {{ {source} }} catch (e) {{ t = {check} }} t === true"))),
            "{source}"
        );
    }
}

#[test]
fn deep_recursion_through_varargs_is_a_range_error() {
    assert!(is_true(
        "function r(...x) { return r(...x, 1) + 1; } var t; try { r() } catch (e) { t = e instanceof RangeError } t === true"
    ));
    assert!(is_true(
        "function r() { return r.apply(null, arguments) + 1; } var t; try { r() } catch (e) { t = e instanceof RangeError } t === true"
    ));
}

#[test]
fn array_literal_spread() {
    assert!(is_true(
        "var a = [1, 2]; var r = [...a, 3]; r.length === 3 && r[2] === 3 && [0, ...a, ...a].join() === '0,1,2,1,2' \
         && [...'ab'].join() === 'a,b' && [...[]].length === 0 && [...a].join() === '1,2' && [...a] !== a"
    ));
    assert!(is_true(
        "var r = [...[1, 2], , 3]; r.length === 4 && !(2 in r) && r[3] === 3 && [...[1], ,].length === 2"
    ));
    assert!(is_true(
        "var r = [...[1, , 3]]; r.length === 3 && 1 in r && r[1] === undefined && r[2] === 3"
    ));
    assert!(is_true(
        "var t; try { [...undefined] } catch (e) { t = e instanceof TypeError } t === true"
    ));
    assert!(is_true(
        "function* g() { yield 1; yield 2; } [0, ...g(), ...new Set([3])].join() === '0,1,2,3'"
    ));
}

#[test]
fn direct_eval_with_spread() {
    assert!(is_true("eval(...['1 + 2']) === 3"));
    assert!(is_true("var x = 10; (function () { var x = 20; return eval(...['x']); })() === 20"));
    assert!(is_true("eval(...[]) === undefined"));
}

/// Medido no bun 1.4.2: `eval(...args)` é eval direto (vê `this` e os locais da função).
#[test]
fn direct_eval_with_spread_sees_this() {
    assert!(is_true("var o = {}; function f() { return eval(...['this === o']); } f.call(o) === true"));
    assert!(is_true("var o = {}; function g() { return eval('this === o'); } g.call(o) === true"));
}

/// Medido no bun 1.4.2: o nome de function expression capturado só pela função filha (ou por
/// `eval`) continua visível nas variantes generator e async.
#[test]
fn named_function_expression_captured_only_by_child() {
    assert!(is_true("var b = function nf2() { return function () { return typeof nf2; }; }; b()() === 'function'"));
    assert!(is_true("var c = function nf3() { return eval('typeof nf3'); }; c() === 'function'"));
    assert!(is_true("var a = function* nf() { return typeof nf; }; a().next().value === 'function'"));
    assert!(is_true("var e = function* nf5() { yield (function () { return typeof nf5; })(); }; e().next().value === 'function'"));
    assert!(is_true("var h = function* nf7() { return eval('typeof nf7'); }; h().next().value === 'function'"));
}

/// Medido no bun 1.4.2 (cada programa por eval indireto, sloppy): nome de function expression lido, atribuído
/// (sloppy ignora, strict lança `TypeError: Attempted to assign to readonly property.`), sombreado e visto de
/// fora, quando só a função filha (arrow, `with`, `eval`, generator) o usa.
#[test]
fn named_function_expression_name_used_by_child_only() {
    let cases: &[(&str, &str)] = &[
        ("var f=function g(){return ()=>g;};typeof f()()", "function"),
        ("var f=function g(){return ()=>{g=1;return typeof g};};f()()", "function"),
        ("var f=function g(){return ()=>{g=1};};f()();typeof g", "undefined"),
        ("var f=function g(){return ()=>g;};typeof g", "undefined"),
        ("var f=function g(){return ()=>eval('typeof g');};f()()", "function"),
        ("var f=function g(){return ()=>eval('g=1;typeof g');};f()()", "function"),
        ("var f=function g(){return (g)=>g;};f()(5)", "5"),
        ("var f=function g(){return ()=>{var g=2;return g};};f()()", "2"),
        ("var f=function g(){return ()=>{let g=2;return g};};f()()", "2"),
        ("var f=function* g(){yield ()=>g;};typeof f().next().value()", "function"),
        ("var f=function* g(){yield ()=>eval('g');};typeof f().next().value()", "function"),
        ("var f=function* g(){yield ()=>{g=1;return typeof g};};f().next().value()", "function"),
        ("var f=function* g(){yield ()=>eval('g=1;typeof g')};f().next().value()", "function"),
        ("var f=function* g(){var g=3;yield ()=>g};f().next().value()", "3"),
        ("var f=function g(){return ()=>{with({}){return typeof g}}};f()()", "function"),
        ("var f=function g(){return ()=>{with({}){g=1;return typeof g}}};f()()", "function"),
        ("var f=function g(a=()=>g){return a};typeof f()()", "function"),
        ("var f=function* g(a=()=>g){yield a};typeof f().next().value()", "function"),
        ("var f=function g(){return ()=>g.name};f()()", "g"),
        ("var f=class g{static m(){return ()=>typeof g}};f.m()()", "function"),
    ];
    for (source, expected) in cases {
        // `String(...)`: os esperados numéricos ("5", "2", "3") são o texto do valor, não uma string JS.
        let literal = format!("String((0, eval)({source:?})) === {expected:?}");
        assert!(is_true(&literal), "esperado {expected} em: {source}");
    }
}

/// Parâmetro, `let`, `var`, `catch`, valor padrão e `eval` com o mesmo nome do function expression sombreiam o
/// nome (o escopo do nome da função fica por fora de todos eles). Valores medidos no bun 1.4.2.
#[test]
fn named_function_expression_name_shadowed_by_inner_bindings() {
    let cases: &[(&str, &str)] = &[
        ("var f=function g(){return (g)=>g;};f()(5)", "5"),
        ("var f=function g(){return function(g){return g};};f()(5)", "5"),
        ("var f=function g(){return (x)=>{let g=7;return g};};f()(5)", "7"),
        ("var f=function g(){let g=7;return g;};f()", "7"),
        ("var f=function g(){return (g=9)=>g;};f()()", "9"),
        ("var f=function g(){return (g=9)=>g;};f()(5)", "5"),
        ("var f=function g(){return ((g)=>(h)=>g)(5)};f()(1)", "5"),
        ("var f=function g(){try{throw 4}catch(g){return ()=>g}};f()()", "4"),
        ("var f=function g(){try{throw 4}catch(g){return g}};f()", "4"),
        ("var f=function g(){return (g)=>eval('g');};f()(5)", "5"),
        ("var f=function g(){return (g)=>()=>g;};f()(5)()", "5"),
        ("var f=function g(){return ()=>eval('var g=8;g');};f()()", "8"),
        ("var f=function g(){var g=3;return ()=>g};f()()", "3"),
        ("var f=function g(){return function h(g){return ()=>g}};f()(6)()", "6"),
        ("var f=function g(){return ([g])=>g};f()([5])", "5"),
        ("var f=function g(){return (...g)=>g.length};f()(1,2,3)", "3"),
        ("var f=function g(){return (g)=>{var g;return g}};f()(5)", "5"),
    ];
    for (source, expected) in cases {
        let literal = format!("String((0, eval)({source:?})) === {expected:?}");
        assert!(is_true(&literal), "esperado {expected} em: {source}");
    }
}

/// Medido no bun 1.4.2: no strict (ou via eval estrito) a atribuição ao nome pela filha lança.
#[test]
fn named_function_expression_strict_assignment_by_child_throws() {
    let cases = [
        "var f=function g(){'use strict';return ()=>{g=1;return typeof g};};f()()",
        "var f=function g(){'use strict';return ()=>eval('g=1');};f()()",
        "var f=function* g(){'use strict';yield ()=>{g=1};};f().next().value()",
        "var f=class g{static m(){return ()=>{g=1}}};f.m()()",
    ];
    for source in cases {
        let wrapped = format!(
            "try {{ (0, eval)({source:?}); false }} catch (e) {{ e instanceof TypeError && e.message === 'Attempted to assign to readonly property.' }}"
        );
        assert!(is_true(&wrapped), "esperado TypeError em: {source}");
    }
}
