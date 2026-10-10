//! Proper tail calls ponta a ponta: `op_tail_call` e `op_tail_call_varargs` trocam o frame do chamador pelo do
//! callee (`prepareForTailCall`), então a recursão em posição de cauda em modo estrito não cresce a pilha, nem a
//! de registradores nem a nativa (`Interpreter::MAX_NATIVE_DEPTH`). Fora do modo estrito o gerador não emite
//! `op_tail_call` (`BytecodeGenerator::m_allowTailCallOptimization`) e a mesma recursão estoura como no JSC:
//! `RangeError: Maximum call stack size exceeded.`
use zjsc::api::eval::evaluate_script;

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source).unwrap_or_else(|_| panic!("lançou exceção: {source}"));
    value.is_true()
}

#[test]
fn strict_self_recursion_one_million_iterations() {
    assert!(is_true(
        "function loop(n, acc) { 'use strict'; if (n === 0) return acc; return loop(n - 1, acc + 1); } \
         loop(1000000, 0) === 1000000"
    ));
}

#[test]
fn strict_mutual_recursion_one_million_iterations() {
    assert!(is_true(
        "function isEven(n) { 'use strict'; if (n === 0) return true; return isOdd(n - 1); } \
         function isOdd(n) { 'use strict'; if (n === 0) return false; return isEven(n - 1); } \
         isEven(1000000) === true && isOdd(1000000) === false && isOdd(999999) === true"
    ));
}

#[test]
fn sloppy_recursion_is_not_a_tail_call() {
    assert!(is_true(
        "function loop(n, acc) { if (n === 0) return acc; return loop(n - 1, acc + 1); } \
         var t; try { loop(1000000, 0); } catch (e) { t = e instanceof RangeError && e.message === 'Maximum call stack size exceeded.'; } \
         t === true"
    ));
    assert!(is_true(
        "function isEven(n) { if (n === 0) return true; return isOdd(n - 1); } \
         function isOdd(n) { if (n === 0) return false; return isEven(n - 1); } \
         var t; try { isEven(1000000); } catch (e) { t = e instanceof RangeError; } t === true"
    ));
}

#[test]
fn strict_tail_call_changes_the_argument_count() {
    // O callee com mais argumentos que o chamador começa abaixo do frame dele; com menos, sobe; com menos
    // que `numParameters`, passa pelo arity fixup. A área de argumentos não pode erodir em 300 mil trocas.
    assert!(is_true(
        "function f(n) { 'use strict'; if (n === 0) return 0; return g(n - 1, 1, 2, 3, 4, 5); } \
         function g(n, a, b, c, d, e) { 'use strict'; return h(n, a + b + c + d + e); } \
         function h(n, a, b, c) { 'use strict'; return f(n); } \
         f(300000) === 0"
    ));
}

#[test]
fn strict_tail_call_keeps_this_and_arguments() {
    assert!(is_true(
        "var o = { m(n, s) { 'use strict'; return n === 0 ? [this === o, s] : this.m(n - 1, s + arguments.length); } }; \
         var r = o.m(100000, 0); r[0] === true && r[1] === 200000"
    ));
}

#[test]
fn strict_tail_call_varargs() {
    // A diretiva fica no topo do programa: `'use strict'` dentro de uma função com parâmetro `...rest` é SyntaxError
    // (diretiva em lista de parâmetros não simples), no bun e no porte.
    assert!(is_true(
        "'use strict'; function v(n, ...rest) { if (n === 0) return rest.length; return v(...[n - 1, 1, 2]); } \
         v(200000) === 2"
    ));
    assert!(is_true(
        "function w(n) { 'use strict'; if (n === 0) return arguments.length; return w.apply(null, [n - 1]); } \
         w(100000) === 1"
    ));
}

#[test]
fn tail_call_to_native_and_exceptions_through_the_replaced_frame() {
    assert!(is_true("function f() { 'use strict'; return Math.max(1, 2); } f() === 2"));
    assert!(is_true(
        "function thrower() { throw 7; } function f() { 'use strict'; return thrower(); } \
         var t; try { f(); } catch (e) { t = e; } t === 7"
    ));
    assert!(is_true(
        "function f(n) { 'use strict'; if (n === 0) throw 9; return f(n - 1); } \
         var t; try { f(500000); } catch (e) { t = e; } t === 9"
    ));
}

#[test]
fn tail_caller_frame_is_gone_from_the_stack_trace() {
    assert!(is_true(
        "function tailCallee() { 'use strict'; return new Error('x').stack; } \
         function tailCaller() { 'use strict'; return tailCallee(); } \
         function plainCaller() { var s = tailCallee(); return s; } \
         var tail = tailCaller(); var plain = plainCaller(); \
         tail.includes('tailCallee') && !tail.includes('tailCaller') && plain.includes('plainCaller')"
    ));
}

// Medido no bun 1.4.2 (tests/golden/tailcall_bun.tsv): `return f()` dentro de `try` com `finally` e `new F()`
// nunca são cauda no JSC (`ReturnNode` com `hasFinallyScopes()`, `emitConstruct`), então a recursão de 1e6 estoura
// e o `catch` de cima recebe o `RangeError` (não a célula `Exception` embrulhada).
#[test]
fn try_finally_and_construct_are_not_tail_calls_and_throw_range_error() {
    assert!(is_true(
        "'use strict'; function f(n) { try { return n ? f(n - 1) : 0; } finally {} } \
         var t; try { f(1000000); } catch (e) { t = e instanceof RangeError && e.name === 'RangeError'; } t === true"
    ));
    assert!(is_true(
        "'use strict'; function F(n) { this.v = n ? new F(n - 1) : 0; } \
         var t; try { new F(1000000); } catch (e) { t = e instanceof RangeError && e.name === 'RangeError'; } t === true"
    ));
}

// Medido no bun 1.4.2: a bound function em cauda só vira o alvo direto no sítio que viu uma bound function só (o
// `CallLinkInfo` monomórfico, mesmo alternando duas fixas); o sítio que recebe uma bound function nova a cada chamada
// cresce a pilha e estoura (antes o porte desembrulhava sempre e a recursão `return f.bind(this)()` comia a memória).
#[test]
fn bound_tail_call_unwraps_only_at_a_monomorphic_site() {
    assert!(is_true(
        "'use strict'; var a = function (n) { return n ? b(n - 1) : 0; }.bind(null); \
         var b = function (n) { return n ? a(n - 1) : 0; }.bind(null); a(1000000) === 0"
    ));
    assert!(is_true(
        "'use strict'; var d = 0; function f(n) { d++; if (d > 300000) return 'fundo'; return n ? f.bind(null)(n - 1) : 0; } \
         var t; try { t = f(1000000); } catch (e) { t = e instanceof RangeError; } t === true"
    ));
}

// Medido no bun 1.4.2: em modo estrito o `return chamada()` de um async generator é `op_tail_call`, e o `await` que o
// `return` emite depois nunca roda, nem quando o callee é nativo (`Promise.resolve`): o valor do resultado é a própria
// promessa. Dentro de `try` com `finally` não há cauda e o valor é esperado. Antes a cauda para nativo seguia o
// bytecode depois do `op_tail_call` e esperava a promessa.
#[test]
fn tail_call_to_a_native_returns_from_the_caller_frame() {
    let source = "'use strict'; const out = []; \
        const show = (tag, g) => g.next().then(() => g.next()).then((r) => out.push(tag + '=' + (r.value instanceof Promise ? 'promise' : r.value))); \
        const o = { m() { return Promise.resolve('V'); } }; \
        const cases = [['method', async function* () { yield 0; return Promise.resolve('V'); }], \
          ['js', async function* () { yield 0; return o.m(); }], \
          ['fin', async function* () { try { yield 0; } finally { return Promise.resolve('V'); } }], \
          ['try_fin', async function* () { try { yield 0; return Promise.resolve('V'); } finally {} }]]; \
        (async () => { for (const [tag, f] of cases) await show(tag, f()); globalThis.R = out.join(); })();";
    let value = zjsc::api::eval::evaluate_named_script_result(source, "main.js", "R").expect("sem exceção");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "method=promise,js=promise,fin=promise,try_fin=V");
}
