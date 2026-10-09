// Gera tests/golden/dynamic_fn_bun.tsv: construção dinâmica de função (Function, new Function, os construtores de
// GeneratorFunction, AsyncFunction e AsyncGeneratorFunction), Function.prototype.toString de cada forma de função
// (nomes, computed, getters, métodos, classes, async arrow, native de built-ins e bound), eval direto x indireto (this,
// vazamento de var, propagação de strict, new.target, super em método, arguments), valores de conclusão do eval e as
// mensagens exatas de SyntaxError de Function() e eval, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança). Caminho da máquina no resultado descarta
// o programa. Cada execução tem timeout. Não repete function_source, eval_scope nem completion_value.
// Uso: bun scripts/gen-dynamic-fn-golden.js > tests/golden/dynamic_fn_bun.tsv
const fs = require("fs");
const { emitRow, sampleByHash, stepSampler } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Os candidatos entram em `pool`; os que só entram em parte levam a densidade (`thin(passo, programa)`, 1 em `passo`) e a
// escolha é por hash do texto do programa (`sampleByHash` dentro do `stepSampler`), nunca pela posição na lista.
const pool = stepSampler();
const add = body => pool.push(1, body);
const thin = (step, body) => pool.push(step, body);
const tryR = body => `try { ${body} } catch (e) { R = e.name + ': ' + e.message }`;
const T = body => add(tryR(body));
const q = JSON.stringify;
// Resumo de uma função: length, name e texto.
const SUM = "JSON.stringify([f.length, f.name, f.toString()])";
// Lista de argumentos literais de Function.
const args = list => list.map(q).join(", ");

// ---- Function(...) com parâmetros em vários formatos.
const GEN = "Object.getPrototypeOf(function* () {}).constructor";
const ASY = "Object.getPrototypeOf(async function () {}).constructor";
const AGEN = "Object.getPrototypeOf(async function* () {}).constructor";
const ctors = [["Function", "Function"], ["new Function", "Function"], ["GeneratorFunction", GEN], ["AsyncFunction", ASY], ["AsyncGeneratorFunction", AGEN]];
const paramForms = [
  [[], "return 1"],
  [["a"], "return a"],
  [["a, b"], "return a + b"],
  [["a", "b"], "return a + b"],
  [["a", "b", "c"], "return a"],
  [["a,b", "c"], "return c"],
  [["a", "b,c"], "return c"],
  [["...rest"], "return rest.length"],
  [["a", "...rest"], "return rest.length"],
  [["a, ...rest"], "return rest.length"],
  [["a = 1"], "return a"],
  [["a = 1, b = a + 1"], "return b"],
  [["a", "b = a"], "return b"],
  [["a = 1", "b"], "return b"],
  [["{ a }"], "return a"],
  [["{ a, b = 2 }"], "return b"],
  [["[a, b]"], "return b"],
  [["[a, , b]"], "return b"],
  [["{ a: [b] }"], "return b"],
  [["{ a }", "[b]"], "return a"],
  [["{ a } = {}"], "return a"],
  [["[a] = [1]", "...r"], "return a"],
  [["a, b,"], "return a"],
  [["a,"], "return a"],
  [["a", "b,"], "return a"],
  [["a /* c */"], "return a"],
  [["/* c */ a"], "return a"],
  [["a // c\n"], "return a"],
  [["// c\na"], "return a"],
  [["a, /* x */ b"], "return b"],
  [["a /* ) */"], "return a"],
  [["a = /* ) */ 1"], "return a"],
  [["a", "/* x */"], "return a"],
  [["\ta\n,\n b\n"], "return b"],
  [["a "], "return a"],
  [["é"], "return é"],
  [["\\u0061"], "return a"],
  [["a = function () { return 1 }"], "return a()"],
  [["a = (b, c) => b"], "return a"],
  [["a = `x`"], "return a"],
  [["a = ')'"], "return a"],
  [["async"], "return async"],
  [["yield"], "return yield"],
  [["await"], "return await"],
  [["let"], "return let"],
  [["eval"], "return eval"],
  [["arguments"], "return arguments"],
  [["a, a"], "return a"],
  [["a", "a"], "return a"],
  [["[a], a"], "return a"],
  [["a", "...a"], "return a"],
  [["...a", "b"], "return a"],
  [["...a,"], "return a"],
  [["...a = []"], "return a"],
  [["a = 1"], "'use strict'; return a"],
  [["a, a"], "'use strict'; return a"],
  [["eval"], "'use strict'; return eval"],
  [["arguments"], "'use strict'; return 1"],
  [["static"], "'use strict'; return 1"],
  [["implements"], "return 1"],
  [["a"], "'use strict'; return this"],
  [["a = 1"], "'use strict'"],
  [["{ a }"], "'use strict'"],
  [["a"], "return typeof new.target"],
  [["a"], "return typeof this"],
  [["a"], ""],
  [["a"], "\n"],
  [["a"], "// c"],
  [["a"], "/* c */"],
  [["a"], "return a // c"],
  [["a"], "return a /* c */"],
  [["a"], "return `\n`"],
  [["a"], "if (a) { return 1 } else { return 2 }"],
  [["a"], "var a; return a"],
  [["a"], "let a; return a"],
  [["a"], "const a = 1; return a"],
  [["a = 1"], "var a; return a"],
  [["a = 1"], "let a"],
  [["{ a }"], "var a"],
  [["{ a }"], "let a"],
  [["a"], "function a() {} return a"],
  [["a"], "return arguments.length"],
  [["a = 1"], "return arguments.length"],
  [["a"], "super.x"],
  [["a"], "super()"],
  [["a"], "return await 1"],
  [["a"], "yield 1"],
  [["a"], "return yield"],
  [["a"], "break"],
  [["a"], "continue"],
  [["a"], "x: { break x }"],
  [["a"], "return"],
  [["a"], "return;;"],
  [["a"], "import('x')"],
  [["a"], "import.meta"],
  [["a"], "export default 1"],
  [["a"], "import x from 'y'"],
  [["a"], "#x in a"],
  [["a"], "class C { #x; m() { return #x in this } }"],
  [["a"], "<!-- c\nreturn 1"],
  [["a"], "return 1\n--> c"],
  [["a"], "#!shebang\nreturn 1"],
  [["a"], "'use strict'; with (a) {}"],
  [["a"], "with (a) { return 1 }"],
  [["a"], "delete a"],
  [["a"], "'use strict'; delete a"],
  [["a"], "return 08"],
  [["a"], "'use strict'; return 08"],
  [["a"], "return 0o8"],
  [["a"], "'use strict'; return '\\07'"],
  [["a"], "return '\\07'"],
  [["a"], "function (){}"],
  [["a"], "return function () {}"],
];
for (const [ctor, expr] of ctors.slice(0, 2)) {
  for (const [params, body] of paramForms) {
    T(`var f = ${ctor}(${args([...params, body])}); R = ${SUM}`);
  }
}
// Construtores especiais só com um subconjunto (o texto muda com o tipo), pela expressão que chega ao construtor.
for (const [, expr] of ctors.slice(2)) {
  for (const [params, body] of paramForms) {
    thin(3, tryR(`var C = ${expr}; var f = C(${args([...params, body])}); R = ${SUM}`));
  }
  for (const [params, body] of sampleByHash(paramForms, 12, ([ps, b]) => `${expr}|${JSON.stringify(ps)}|${b}`)) {
    T(`var C = ${expr}; var f = new C(${args([...params, body])}); R = ${SUM}`);
  }
}

// ---- Function: propriedades do resultado e injeção.
const injections = [
  ["a){", "}"],
  ["a){ return 1 }, function(b", "return 2"],
  ["a) { return 1 } //", "return 2"],
  ["a", "} function g() {"],
  ["a", "}); (function () {"],
  ["a", "return 1 }, { x: 1"],
  ["/*", "*/ return 1"],
  ["a /*", "*/ b"],
  ["a", "/*"],
  ["a //", "return 1"],
  ["a\n//", "return 1"],
  ["a = 1,", "return a"],
  ["a) => (", "1"],
  ["a", "}\n)\n(function(){"],
  ["", "}) + (function () {"],
  ["a=function(){", "}"],
  ["a = 1) { return a } function g(", "return 1"],
  ["...a, b", "return 1"],
  ["a)", "{ return 1 }"],
  ["(a)", "return a"],
  ["a b", "return 1"],
  ["a;b", "return 1"],
  ["1", "return 1"],
  ["a.b", "return a"],
  ["'a'", "return 1"],
  ["a, 1", "return 1"],
  ["[", "return 1"],
  ["a=", "return 1"],
  ["...", "return 1"],
  ["`", "return 1"],
  ["a", "`"],
  ["a", "'"],
  ["a", "\\"],
];
for (const [p, b] of injections) {
  T(`var f = Function(${args([p, b])}); R = ${SUM}`);
  T(`var f = new Function(${args([p, b])}); R = typeof f`);
}
T("var f = Function('a', 'b', 'return a + b'); R = JSON.stringify([f(1, 2), f.length, f.name, Object.getOwnPropertyNames(f).sort(), f.hasOwnProperty('prototype')])");
T("var f = Function('return this'); R = String(f() === globalThis)");
T("var f = Function(\"'use strict'; return this\"); R = String(f())");
T("var f = Function('return typeof x'); var x = 1; R = f()");
T("(function () { var local = 1; var f = Function('return typeof local'); R = f() })()");
T("(function () { 'use strict'; var f = Function('return this'); R = String(f() === globalThis) })()");
T("var f = Function('return arguments.length'); R = f(1, 2, 3)");
T("var f = Function('a', 'return a'); R = JSON.stringify([f.name, f.toString === Function.prototype.toString, Object.getPrototypeOf(f) === Function.prototype])");
T("var f = Function(); R = JSON.stringify([f(), f.length, f.name, f.toString()])");
T("var f = Function(undefined); R = f.toString()");
T("var f = Function(null); R = f.toString()");
T("var f = Function(1, 2); R = f.toString()");
T("var f = Function({ toString() { return 'a' } }, { toString() { return 'return a' } }); R = f.toString()");
T("var f = Function('a', { toString() { throw new RangeError('boom') } }); R = f.toString()");
T("var f = Function(Symbol()); R = f.toString()");
T("var f = Function('a', Symbol()); R = f.toString()");
T("var f = Function('a', 1n); R = f.toString()");
T("var f = Function(['a', 'b'], 'return 1'); R = f.toString()");
T("var f = Function('a', 'b', 'return a'); R = f.bind(null, 1).toString()");
T("var f = Function.call(null, 'a', 'return a'); R = f.toString()");
T("var f = Function.apply(null, ['a', 'return a']); R = f.toString()");
T("var f = Reflect.construct(Function, ['a', 'return a']); R = f.toString()");
T("class F extends Function {} var f = new F('a', 'return a'); R = JSON.stringify([f.toString(), f instanceof F, Object.getPrototypeOf(f) === F.prototype])");
T("function NT() {} var f = Reflect.construct(Function, ['return 1'], NT); R = JSON.stringify([Object.getPrototypeOf(f) === NT.prototype, f.toString()])");
T("var f = Reflect.construct(Function, ['return 1'], Object.assign(function () {}.bind(), {})); R = typeof f");
T("var P = new Proxy(Function, {}); var f = new P('a', 'return a'); R = f.toString()");
T("var f = Function('return new.target'); R = String(f())");
T("var f = Function('return new.target'); R = typeof new f()");
T("var f = Function('a', 'b', 'return a'); R = JSON.stringify(Object.getOwnPropertyDescriptor(f, 'name'))");
T("var f = Function('a', 'b', 'return a'); R = JSON.stringify(Object.getOwnPropertyDescriptor(f, 'length'))");
T("var f = Function('a', 'b', 'return a'); R = JSON.stringify(Object.getOwnPropertyDescriptor(f, 'prototype'))");
T("var f = Function('a', 'return a'); R = String(Function.prototype.toString.call(f) === f.toString())");
T("var g = GeneratorFunctionProto(); function GeneratorFunctionProto() { return (function* () {}).constructor } var f = g('yield 1'); R = JSON.stringify([f().next(), f.toString(), f.name])");
T("var f = (function* () {}).constructor('a', 'yield a; yield a + 1'); R = JSON.stringify([...f(5)])");
T("var f = (function* () {}).constructor('yield* [1, 2]'); R = JSON.stringify([...f()])");
T("var f = (function* () {}).constructor('yield = 1'); R = f.toString()");
T("var f = (function* () {}).constructor('a = yield', 'return a'); R = f.toString()");
T("var f = (function* () {}).constructor('yield', 'return 1'); R = f.toString()");
T("var f = (function* () {}).constructor('a', 'var yield'); R = f.toString()");
T("var f = (function* () {}).constructor('return yield'); R = f.toString()");
T("var f = (function* () {}).constructor('a = yield 1', ''); R = f.toString()");
T("var f = (async function () {}).constructor('await 1'); R = f.toString()");
T("var f = (async function () {}).constructor('a', 'return await a'); var p = f(2); R = String(p instanceof Promise) + ' ' + f.constructor.name");
T("var f = (async function () {}).constructor('await', 'return 1'); R = f.toString()");
T("var f = (async function () {}).constructor('a = await 1', ''); R = f.toString()");
T("var f = (async function () {}).constructor('var await'); R = f.toString()");
T("var f = (async function () {}).constructor('for await (var x of []) ;'); R = f.toString()");
T("var f = (async function* () {}).constructor('yield await 1'); R = f.toString()");
T("var f = (async function* () {}).constructor('a = yield', ''); R = f.toString()");
T("var f = (async function* () {}).constructor('for await (var x of a) yield x', ''); R = f.toString()");
T("var f = (async function* () {}).constructor('await', ''); R = f.toString()");
T("var f = (async function () {}).constructor('a', 'b', 'return 1'); R = JSON.stringify([f.name, f.length, Object.getPrototypeOf(f) === (async function () {}).constructor.prototype, f.hasOwnProperty('prototype')])");
T("var f = (function* () {}).constructor('a', 'b', 'return 1'); R = JSON.stringify([f.name, f.length, f.hasOwnProperty('prototype'), Object.getPrototypeOf(f.prototype) === (function* () {}).constructor.prototype.prototype])");
T("var f = (async function* () {}).constructor('a', 'return 1'); R = JSON.stringify([f.hasOwnProperty('prototype'), Object.prototype.toString.call(f), Object.prototype.toString.call(f())])");
for (const [name, expr] of ctors.slice(2)) {
  T(`var C = ${expr}; R = JSON.stringify([C.name, C.length, Object.getPrototypeOf(C) === Function, C.prototype[Symbol.toStringTag], Object.getOwnPropertyNames(C).sort()])`);
  T(`var C = ${expr}; R = String(C === Function) + ' ' + String(typeof C.prototype.constructor)`);
  T(`var C = ${expr}; R = C.prototype.toString === Function.prototype.toString`);
  T(`var C = ${expr}; var f = C(); R = JSON.stringify([f.toString(), f.length])`);
  T(`var C = ${expr}; var f = C('/*', '*/'); R = f.toString()`);
  T(`var C = ${expr}; class D extends C {} var f = new D('a', 'return a'); R = JSON.stringify([f.toString(), Object.getPrototypeOf(f) === D.prototype])`);
}

// ---- Function.prototype.toString em cada forma.
const forms = [
  "function f() {}", "function  f ( a ,b )  {  }", "function /* c */ f /* d */ () /* e */ { /* f */ }",
  "function f() { return 1 } // fim", "function* g() {}", "function * g () { yield 1 }", "async function h() {}",
  "async  function  h ( ) { await 1 }", "async function* ag() {}", "async function * ag () { yield 1 }",
  "var f = function () {}", "var f = function named() {}", "var f = function* () {}", "var f = async function () {}",
  "var f = async function* () {}", "var f = () => {}", "var f = (a, b) => a + b", "var f = a => a", "var f = async () => {}",
  "var f = async a => a", "var f = async (a, b) => { await a }", "var f = async  ( a ) =>  a", "var f = ( a ) => /* c */ a",
  "var f = (\n a\n) =>\n a", "var f = (a) => ({})", "var f = (a = 1, { b } = {}, ...c) => 0",
  "var o = { m() {} }; var f = o.m", "var o = { m ( ) { return 1 } }; var f = o.m", "var o = { 'q'() {} }; var f = o.q",
  "var o = { 1() {} }; var f = o[1]", "var o = { 1.5() {} }; var f = o['1.5']", "var o = { [`a${1}`]() {} }; var f = o.a1",
  "var k = 'x'; var o = { [k]() {} }; var f = o.x", "var o = { [Symbol.iterator]() {} }; var f = o[Symbol.iterator]",
  "var o = { *g() {} }; var f = o.g", "var o = { async m() {} }; var f = o.m", "var o = { async *m() {} }; var f = o.m",
  "var o = { get a() { return 1 } }; var f = Object.getOwnPropertyDescriptor(o, 'a').get",
  "var o = { set a(v) {} }; var f = Object.getOwnPropertyDescriptor(o, 'a').set",
  "var o = { get [1 + 1]() { return 1 } }; var f = Object.getOwnPropertyDescriptor(o, '2').get",
  "var o = { get 'a b'() { return 1 } }; var f = Object.getOwnPropertyDescriptor(o, 'a b').get",
  "var o = { f: function () {} }; var f = o.f", "var o = { f: () => 1 }; var f = o.f", "var o = { f: class {} }; var f = o.f",
  "class A {} var f = A", "class  A  {  }  var f = A", "class A extends Object {} var f = A", "class A { constructor() {} } var f = A",
  "class A { constructor ( a ) { } m() {} } var f = A", "var f = class {}", "var f = class Named {}", "var f = class extends Array {}",
  "class A { m() {} } var f = A.prototype.m", "class A { static m() {} } var f = A.m", "class A { static async *m() {} } var f = A.m",
  "class A { get a() { return 1 } } var f = Object.getOwnPropertyDescriptor(A.prototype, 'a').get",
  "class A { static set a(v) {} } var f = Object.getOwnPropertyDescriptor(A, 'a').set",
  "class A { ['c' + 1]() {} } var f = A.prototype.c1", "class A { #p() {} static g(o) { return o.#p } } var f = A.g(new A)",
  "class A { x = () => 1 } var f = new A().x", "class A { static x = function () {} } var f = A.x",
  "class A { static { } } var f = A", "class A { m() { return class B {} } } var f = new A().m()",
  "var f = function () {}.bind()", "var f = function a() {}.bind(null)", "var f = (() => 1).bind(null)",
  "var f = class A { m() {} }.prototype.m.bind(null)", "var f = Function.prototype.bind.call(Math.max, null)",
  "var f = Math.max", "var f = Object", "var f = Array.prototype.push", "var f = Object.getOwnPropertyDescriptor(Map.prototype, 'size').get",
  "var f = Symbol", "var f = Symbol.prototype[Symbol.toPrimitive]", "var f = Promise.resolve", "var f = Function.prototype",
  "var f = Function", "var f = Function.prototype.toString", "var f = parseInt", "var f = eval", "var f = Reflect.ownKeys",
  "var f = JSON.stringify", "var f = Array.prototype[Symbol.iterator]", "var f = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get",
  "var f = RegExp.prototype[Symbol.replace]", "var f = Date.prototype[Symbol.toPrimitive]", "var f = Intl.DateTimeFormat",
  "var f = Proxy", "var f = new Proxy(function () {}, {})", "var f = new Proxy(class A {}, {})",
  "var f = Object.getPrototypeOf(function* () {}).constructor", "var f = Object.getPrototypeOf(async function () {}).constructor",
  "var f = (function* () {}).constructor.prototype.constructor", "var f = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
  "var f = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set", "var f = Array.from", "var f = Array[Symbol.species] , g = Object.getOwnPropertyDescriptor(Array, Symbol.species).get",
  "var f = (function () { return arguments.callee })", "var f = function () { 'use strict' }", "var f = () => { 'use strict' }",
  "var f = async function () { /* é */ }", "var f = function () { return '\u{1F600}' }", "var f = function () {}",
  "var f = function\n() {}", "var f = function () {\r\n}", "var f = function () {}\n\n", "var f = function   () {}",
  "var f = (function () {})", "var f = ((a) => a)", "var f = [function () {}][0]", "var f = (0, function () {})",
  "var f = async function () {}.constructor", "var f = Object.getOwnPropertyDescriptor(class { static get a() {} }, 'a').get",
  "label: var f = function () {}", "var f = function* () { yield function () {} }", "var { f } = { f() {} }", "var [f] = [function () {}]",
  "var f; ({ f = function () {} } = {})", "var f = true ? function () {} : 0", "var f = (class { static m() {} }).m",
];
for (const form of forms) {
  T(`${form}\nR = Function.prototype.toString.call(f)`);
  if (/function\b|=>|class/.test(form.slice(0, 20))) T(`${form}\nR = JSON.stringify([String(f), f.name, f.length])`);
}
// Declarações cuja forma é referida por nome.
T("function d() {} R = d.toString()");
T("function* d() {} R = d.toString()");
T("async function d() {} R = d.toString()");
T("class D { } R = D.toString()");
T("class D { static x = 1 } R = D.toString()");
T("R = (function () {}).toString.call(function () {})");
// toString em não-função.
for (const v of ["{}", "[]", "null", "undefined", "1", "'s'", "Symbol()", "new Proxy({}, {})", "new Proxy(function () {}, {})", "Math", "class {}.prototype", "Function.prototype.bind.call({})"]) {
  T(`R = Function.prototype.toString.call(${v})`);
}
T("R = Function.prototype.toString.call(new Proxy(class { }, {}))");
T("R = Function.prototype.toString.call(new Proxy({}, { apply() {} }))");
T("var f = function () {}; f.toString = () => 'x'; R = Function.prototype.toString.call(f) + String(f)");
T("var f = function () {}; Object.defineProperty(f, 'name', { value: 'zzz' }); R = f.toString()");
T("R = Function.prototype.toString.length + ' ' + Function.prototype.toString.name");
T("R = JSON.stringify(Object.getOwnPropertyNames(Function.prototype).sort())");
T("R = Function.prototype.toString.call(Function.prototype.toString)");
T("R = (function () {}).bind().name + '|' + (function a() {}).bind().name + '|' + (function a() {}).bind().bind().name");
T("var b = (function (a, b, c) {}).bind(null, 1); R = JSON.stringify([b.length, b.name, b.toString(), b.hasOwnProperty('prototype')])");
T("var s = Symbol('d'); var o = { [s]() {} }; R = JSON.stringify([o[s].name, o[s].toString()])");
T("var s = Symbol(); var o = { [s]() {} }; R = JSON.stringify([o[s].name, o[s].toString()])");

// ---- eval direto x indireto.
const evalTests = [
  // this
  "R = eval('this') === globalThis",
  "R = (0, eval)('this') === globalThis",
  "R = (function () { return eval('this') }).call(7) + ''",
  "R = (function () { return (0, eval)('this') === globalThis })()",
  "R = (function () { 'use strict'; return typeof eval('this') }).call(7)",
  "R = (function () { 'use strict'; return typeof (0, eval)('this') })()",
  "R = (() => eval('this') === globalThis)()",
  "var o = { m() { return eval('this') === o } }; R = o.m()",
  "var o = { m() { return (0, eval)('this') === o } }; R = o.m()",
  "var o = { m() { return [eval][0]('this') === globalThis } }; R = o.m()",
  "var e = eval; var o = { m() { return e('this') === globalThis } }; R = o.m()",
  "R = (function () { return window_eval('this') === globalThis; function window_eval(s) { return (1, eval)(s) } })()",
  "R = globalThis.eval('this') === globalThis",
  "R = (eval)('var qa1 = 1; typeof qa1')",
  "var o = { eval }; R = o.eval('this') === globalThis",
  "R = (eval, eval)('this') === globalThis",
  "R = eval?.('this') === globalThis",
  "R = (function () { return eval?.('this') }).call(5) + ''",
  "R = (function () { var x = 1; return eval?.('typeof x') })()",
  "R = (function () { var x = 1; return eval('typeof x') })()",
  "R = (function () { var x = 1; return (0, eval)('typeof x') })()",
  "R = (function () { var x = 1; return (eval)('typeof x') })()",
  "R = (function () { var x = 1; return eval.call(null, 'typeof x') })()",
  "R = (function () { var x = 1; return eval.apply(null, ['typeof x']) })()",
  "R = (function () { var x = 1; return Reflect.apply(eval, null, ['typeof x']) })()",
  "R = (function () { var x = 1; var e = eval; return e('typeof x') })()",
  "R = (function () { var x = 1; return new Function('return typeof x')() })()",
  "R = (function () { var x = 1; return eval(...['typeof x']) })()",
  "R = (function () { var x = 1; return eval('typeof x', 2) })()",
  "R = (function () { var x = 1; return eval() })() + ''",
  "R = (function () { var x = 1; return eval(5) })() + ''",
  "R = (function () { var x = 1; return eval({}) })() + ''",
  "R = typeof eval(new String('1 + 1'))",
  "R = eval(Object('1 + 1')) instanceof String",
  "R = (function () { var x = 1; return eval(`x`) })()",
  "R = (function () { var x = 1; return eval('x', 'y') })()",
  "R = (function () { var eval = function () { return 'mine' }; return eval('1') })()",
  "R = (function (eval) { return eval('1 + 1') })(function (s) { return 'p:' + s })",
  "R = (function () { with ({ eval: function (s) { return 'w:' + s } }) { return eval('1') } })()",
  // var leak
  "R = (function () { eval('var v1 = 1'); return typeof v1 })()",
  "R = (function () { (0, eval)('var v2 = 1'); return typeof v2 })() + ' ' + typeof v2",
  "R = (function () { 'use strict'; eval('var v3 = 1'); return typeof v3 })()",
  "R = (function () { eval('\"use strict\"; var v4 = 1'); return typeof v4 })()",
  "R = (function () { eval('var v5 = 1'); var v5; return v5 })()",
  "R = (function () { var v6 = 0; eval('var v6 = 1'); return v6 })()",
  "R = (function () { eval('function ff1() { return 1 }'); return typeof ff1 })()",
  "R = (function () { 'use strict'; eval('function ff2() { return 1 }'); return typeof ff2 })()",
  "R = (function () { eval('let l1 = 1'); return typeof l1 })()",
  "R = (function () { eval('const c1 = 1'); return typeof c1 })()",
  "R = (function () { eval('class K1 {}'); return typeof K1 })()",
  "R = (function () { let q = 1; try { eval('var q = 2') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { let q = 1; { try { eval('var q = 2') } catch (e) { return e.name + ': ' + e.message } } })()",
  "R = (function () { { let q = 1; try { eval('var q = 2') } catch (e) { return e.name + ': ' + e.message } } })()",
  "R = (function (p) { eval('var p = 2'); return p })(1)",
  "R = (function (p = 0) { eval('var p = 2'); return p })(1)",
  "R = (function (p = () => eval('1')) { var p2 = 3; return p2 })()",
  "R = (function (a, b = eval('var z1 = 1')) { return typeof z1 })()",
  "R = (function (a, b = eval('var a = 5')) { return a })(1)",
  "R = (function (a = eval('var a')) { return a })()",
  "R = (function (a = eval('var zz = 1; zz')) { var zz = 2; return a + ':' + zz })()",
  "R = (function (a = eval('var arguments')) { return a })()",
  "R = (function () { eval('var arguments = 1'); return typeof arguments })()",
  "R = (function () { 'use strict'; eval('var arguments = 1') })()",
  "R = (() => { eval('var av = 1'); return typeof av })()",
  "R = (() => { try { eval('var arguments') } catch (e) { return e.name } return 'ok' })()",
  "eval('var g1 = 1'); R = typeof g1 + ' ' + JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'g1'))",
  "(0, eval)('var g2 = 1'); R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'g2'))",
  "(0, eval)('let g3 = 1'); R = typeof g3 + ' ' + typeof globalThis.g3",
  "(0, eval)('const g4 = 1'); R = typeof g4",
  "(0, eval)('function g5() {}'); R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'g5'))",
  "(0, eval)('class G6 {}'); R = typeof G6 + ' ' + typeof globalThis.G6",
  "let g7 = 1; try { (0, eval)('var g7') } catch (e) { R = e.name + ': ' + e.message }",
  "var g8 = 1; try { (0, eval)('let g8') } catch (e) { R = e.name + ': ' + e.message }; R = R || 'ok'",
  "try { (0, eval)('let g9; let g9') } catch (e) { R = e.name + ': ' + e.message }",
  "(0, eval)('let g10 = 1'); try { (0, eval)('let g10 = 2') } catch (e) { R = e.name + ': ' + e.message }",
  "(0, eval)('\"use strict\"; var g11 = 1'); R = typeof g11",
  "(0, eval)('\"use strict\"; function g12() {}'); R = typeof g12",
  "(function () { 'use strict'; (0, eval)('var g13 = 1') })(); R = typeof g13",
  "Object.defineProperty(globalThis, 'g14', { value: 1, configurable: false }); try { (0, eval)('function g14() {}') } catch (e) { R = e.name + ': ' + e.message }",
  "Object.defineProperty(globalThis, 'g15', { value: 1, configurable: false, writable: true, enumerable: true }); (0, eval)('function g15() {}'); R = typeof g15",
  "var d1 = Object.getOwnPropertyDescriptor(globalThis, 'g16'); eval('var g16 = 1'); R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'g16'))",
  "eval('var g17 = 1'); R = String(delete globalThis.g17)",
  "var g18 = 1; R = String(delete globalThis.g18)",
  "eval('g19 = 1'); R = String(delete globalThis.g19)",
  "R = (function () { eval('var dv = 1'); return String(delete dv) })()",
  "R = (function () { var nv = 1; return String(delete nv) })()",
  "R = (function () { eval('var dv2 = 1'); delete dv2; return typeof dv2 })()",
  "R = (function () { eval('var dv3 = 1'); return eval('delete dv3') + ':' + typeof dv3 })()",
  // strict propagation
  "R = (function () { 'use strict'; return eval('(function () { return this })()') + '' })()",
  "R = (function () { return eval('(function () { return this })()') === globalThis })()",
  "R = (function () { 'use strict'; return eval('typeof undeclared1; undeclared1 = 1') })()",
  "R = (function () { 'use strict'; try { eval('undeclared2 = 1') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { try { eval('\"use strict\"; undeclared3 = 1') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('with ({}) {}') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('var public') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('010') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('delete x') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('arguments = 1') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('eval = 1') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('function f(a, a) {}') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; try { eval('({ a: 1, a: 2 })'); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; return eval('arguments.length') })(1, 2)",
  "R = (function () { try { return (0, eval)('\"use strict\"; 010') } catch (e) { return e.name + ': ' + e.message } })()",
  "R = (function () { 'use strict'; return (0, eval)('010') })()",
  "R = (function () { 'use strict'; return (0, eval)('(function () { return this })()') === globalThis })()",
  "R = (function () { 'use strict'; return (0, eval)('with ({ a: 1 }) a') })()",
  "R = (function () { 'use strict'; return eval('eval(\"with ({ a: 1 }) a\")') })()",
  "R = (function () { return eval('\"use strict\"; (function () { return this })()') + '' })()",
  "R = (function () { return eval('(function () { \"use strict\"; return this })()') + '' })()",
  "R = (function () { eval('\"use strict\"'); return (function () { return this })() === globalThis })()",
  "R = (function () { eval('\"use strict\"; var sv = 1'); return typeof sv })()",
  "R = (function () { eval('\"use strict\"; eval(\"var sv2 = 1\"); return typeof sv2 }'); return typeof sv2 })()",
  "class A { m() { return eval('(function () { return this })()') + '' } } R = new A().m()",
  "class A { m() { try { return eval('with ({}) {}') } catch (e) { return e.name + ': ' + e.message } } } R = new A().m()",
  "class A { m() { try { return eval('var yield') } catch (e) { return e.name + ': ' + e.message } } } R = new A().m()",
  "class A { static x = eval('1 + 1') } R = A.x",
  "class A { static x = (function () { return eval('this') === A })() } R = A.x",
  "class A { x = eval('this') } var a = new A(); R = String(a.x === a)",
  "class A { x = eval('new.target') } R = String(new A().x)",
  "class A { x = eval('arguments') } try { new A() } catch (e) { R = e.name + ': ' + e.message }",
  "class A { static { R = typeof eval('this') } }",
  "class A { static { var s = eval('var sv = 1; sv'); R = s + ':' + typeof sv } }",
  "class A { static { try { eval('await') ; R = 'ok' } catch (e) { R = e.name + ': ' + e.message } } }",
  "class A { static { try { eval('arguments'); R = 'ok' } catch (e) { R = e.name + ': ' + e.message } } }",
  // new.target
  "R = (function () { return eval('new.target') })() + ''",
  "R = String(typeof new (function () { this.t = eval('new.target') })().t)",
  "function F() { this.t = eval('new.target === F') } R = String(new F().t)",
  "function F() { return eval('new.target') } R = String(F())",
  "function F() { return (0, eval)('new.target') } try { F() } catch (e) { R = e.name + ': ' + e.message }",
  "try { eval('new.target') } catch (e) { R = e.name + ': ' + e.message }",
  "try { (0, eval)('new.target') } catch (e) { R = e.name + ': ' + e.message }",
  "R = (() => { try { return eval('new.target') } catch (e) { return e.name + ': ' + e.message } })()",
  "function F() { return (() => eval('new.target'))() } R = String(typeof new F())",
  "function F() { return eval('(() => new.target)()') } R = String(F())",
  "function F() { return eval('function g() { return new.target } g()') } R = String(F())",
  "function F() { return eval('function g() { return new.target } new g()') === undefined } R = String(F())",
  "class A { constructor() { this.t = eval('new.target') === B } } class B extends A {} R = String(new B().t)",
  "function F(a = eval('new.target')) { return a } R = String(F()) + String(typeof new F())",
  "R = (function () { return eval('eval(\"new.target\")') })() + ''",
  "R = (function () { try { return Function('return new.target')() } catch (e) { return e.name } })() + ''",
  // super em eval dentro de método
  "var o = { __proto__: { x: 1 }, m() { return eval('super.x') } }; R = o.m()",
  "var o = { __proto__: { x: 1 }, m() { return (0, eval)('super.x') } }; try { R = o.m() } catch (e) { R = e.name + ': ' + e.message }",
  "var o = { __proto__: { x: 1 }, m() { return eval('(() => super.x)()') } }; R = o.m()",
  "var o = { __proto__: { x: 1 }, m() { return eval('function g() { return super.x }') } }; try { R = o.m() } catch (e) { R = e.name + ': ' + e.message }",
  "var o = { __proto__: { m() { return 'pm' } }, m() { return eval('super.m()') } }; R = o.m()",
  "var o = { __proto__: { m() { return this.v } }, v: 7, m() { return eval('super.m()') } }; R = o.m()",
  "var o = { __proto__: { x: 1 }, m() { eval('super.x = 5'); return this.x + ':' + Object.getPrototypeOf(this).x } }; R = o.m()",
  "var o = { __proto__: { x: 1 }, f: function () { return eval('super.x') } }; try { R = o.f() } catch (e) { R = e.name + ': ' + e.message }",
  "var o = { __proto__: { x: 1 }, get g() { return eval('super.x') } }; R = o.g",
  "var o = { __proto__: { x: 1 }, m: () => 0, async am() { return eval('super.x') } }; o.am().then(v => globalThis.R = v)",
  "try { eval('super.x') } catch (e) { R = e.name + ': ' + e.message }",
  "try { eval('super()') } catch (e) { R = e.name + ': ' + e.message }",
  "function f() { try { return eval('super.x') } catch (e) { return e.name + ': ' + e.message } } R = f()",
  "class A { x() { return 'ax' } } class B extends A { x() { return eval('super.x()') } } R = new B().x()",
  "class A { static s() { return 'as' } } class B extends A { static s() { return eval('super.s()') } } R = B.s()",
  "class A { constructor() { this.a = 1 } } class B extends A { constructor() { eval('super()'); this.b = 2 } } R = JSON.stringify(new B())",
  "class A {} class B extends A { constructor() { (0, eval)('super()') } } try { new B() } catch (e) { R = e.name + ': ' + e.message }",
  "class A {} class B extends A { constructor() { eval('super()'); eval('super()') } } try { new B() } catch (e) { R = e.name + ': ' + e.message }",
  "class A {} class B extends A { constructor() { var f = () => eval('super()'); f(); this.k = 1 } } R = JSON.stringify(new B())",
  "class A {} class B extends A { constructor() { eval('this') } } try { new B() } catch (e) { R = e.name + ': ' + e.message }",
  "class A {} class B extends A { constructor() { eval('super()'); R = String(eval('this') === this) } } new B()",
  "class A {} class B { constructor() { try { eval('super()') } catch (e) { R = e.name + ': ' + e.message } } } new B()",
  "class A { m() { try { eval('super()') } catch (e) { return e.name + ': ' + e.message } } } R = new A().m()",
  "class A { x = 1 } class B extends A { y = eval('super.x') } R = String(new B().y)",
  "class A { static x = 1 } class B extends A { static y = eval('super.x') } R = String(B.y)",
  "class A { get p() { return 'pp' } } class B extends A { get p() { return eval('super.p') } } R = new B().p",
  "class A {} class B extends A { m() { return eval('super.m') } } R = String(new B().m())",
  "class A { m() { return 1 } } class B extends A { m() { return eval('(() => super.m())()') + 1 } } R = new B().m()",
  "class A { m() { return 1 } } class B extends A { m() { return eval('eval(\"super.m()\")') + 1 } } R = new B().m()",
  "class A { m() { return 1 } } class B extends A { m() { return new Function('return super.m()') } } try { R = new B().m()() } catch (e) { R = e.name + ': ' + e.message }",
  // arguments
  "R = (function () { return eval('arguments.length') })(1, 2, 3)",
  "R = (function () { return (0, eval)('typeof arguments') })(1)",
  "R = (function (a) { eval('arguments[0] = 9'); return a })(1)",
  "R = (function (a) { 'use strict'; eval('arguments[0] = 9'); return a })(1)",
  "R = (function () { return eval('arguments') === arguments })()",
  "R = (function () { return eval('(function () { return arguments.length })(1, 2)') })()",
  "R = (function () { return eval('(() => arguments.length)()') })(1, 2, 3)",
  "R = (() => { try { return eval('arguments.length') } catch (e) { return e.name + ': ' + e.message } })()",
  "function f() { return (() => eval('arguments.length'))() } R = f(1, 2)",
  "function f() { return eval('var arguments = 5; arguments') } R = f(1, 2)",
  "function f() { eval('var arguments = 5'); return typeof arguments } R = f(1, 2)",
  "function f() { eval('arguments = 5'); return arguments } R = f(1, 2)",
  "function f() { var arguments = 3; return eval('arguments') } R = f(1, 2)",
  "function f(arguments) { return eval('arguments') } R = f(8)",
  "function f() { return eval('eval(\"arguments.length\")') } R = f(1)",
  "function f(a = eval('arguments.length')) { return a } R = f(undefined, 2)",
  "function f(a = eval('arguments.length'), b = arguments.length) { return a + ':' + b } R = f(undefined, 2, 3)",
  "R = Object.prototype.toString.call((function () { return eval('arguments') })())",
  "R = (function () { return Object.prototype.toString.call(eval('arguments')) })()",
  "var o = { m() { return eval('arguments.length') } }; R = o.m(1, 2)",
  "class A { m() { return eval('arguments.length') } } R = new A().m(1, 2, 3)",
  "async function f() { return eval('arguments.length') } f(1, 2).then(v => globalThis.R = v)",
  "function* g() { yield eval('arguments.length') } R = g(1, 2, 3).next().value",
];
for (const t of evalTests) T(t);

// ---- eval: valores de conclusão e escopo léxico.
const completions = [
  "1", "1;", "1; 2", "var a = 1", "var a = 1; a", "let a = 1", "let a = 1; a", "const a = 1", "function f() {}", "function f() {} 3",
  "class A {}", "class A {} 4", "if (true) 1", "if (false) 1", "if (true) 1; else 2", "if (false) 1; else 2", "if (true) {}", "1; if (true) {}",
  "1; if (false) 2", "1; if (true) { }", "1; if (true) { 2 }", "1; {}", "1; { 2 }", "1; { 2; }", "{ 1 } 2", "{} 5", "5; {}",
  "1; var x = 2", "1; var x", "1; let x", "1; function f() {}", "1; class A {}", "1; ;", ";", ";;", "1;;;", "1; do { 2; break } while (0)",
  "do { 1 } while (false)", "do { } while (false)", "1; do { } while (false)", "1; do { 2; continue } while (false)", "1; do { break } while (false)",
  "1; while (false) 2", "1; while (true) { 2; break }", "1; while (true) { break }", "2; for (var i = 0; i < 2; i++) i", "2; for (var i = 0; i < 0; i++) i",
  "2; for (var i = 0; i < 2; i++) { }", "2; for (var i = 0; i < 3; i++) { if (i == 1) continue; i }", "2; for (var i = 0; i < 3; i++) { i; if (i == 1) break }",
  "3; for (var k in { a: 1, b: 2 }) k", "3; for (var k in {}) k", "3; for (var v of [7, 8]) v", "3; for (var v of []) v", "3; for (var v of [7]) { }",
  "1; switch (1) { case 1: 2 }", "1; switch (1) { case 1: }", "1; switch (1) { case 2: 2 }", "1; switch (1) { default: 3; break; case 1: }", "switch (1) { case 1: 2; break; case 2: 3 }",
  "1; try { 2 } catch (e) { 3 }", "1; try { throw 0 } catch (e) { 3 }", "1; try { } finally { 4 }", "1; try { 2 } finally { 4 }", "1; try { throw 0 } catch (e) { } finally { 5 }",
  "1; try { throw 0 } catch (e) { 6 } finally { 5 }", "1; try { } catch (e) { 3 }", "try { 2 } catch (e) { }", "try { throw 1 } catch (e) { }",
  "1; l: { 2; break l }", "1; l: { break l }", "1; l: 2", "l: { 3; l2: { 4; break l } 5 }", "1; with ({}) 2", "1; with ({}) { }", "with ({ a: 5 }) a",
  "1; x: for (;;) { 2; break x }", "1; x: for (;;) { break x }", "1; for (;;) { 9; break }", "var i = 0; do { i++; if (i < 3) continue; 'x' } while (i < 3)",
  "1 + 1", "'s'", "typeof 1", "void 0", "null", "undefined", "[]", "({})", "({}).x", "(function () {})", "(() => 1)", "(class {})",
  "`t`", "/r/", "this === globalThis", "1, 2", "x = 5", "x++", "delete globalThis.nope", "new Object", "1n", "-0", "0 / 0",
  "debugger", "1; debugger", "1; debugger; 2", "'a'; 'use strict'", "'use strict'; 1", "'use strict'", "'use strict'; var q", "yield", "await",
  "async function af() {} 1", "function* gf() {}", "1; async function af() {}", "var f = function () {}", "var f = function () {}; f",
  "1; 2; 3; 4; 5", "({ a: 1 }); 2", "{ a: 1 }", "{ a: 1, b: 2 }", "{ a: 1; }", "{ a }", "{ ;a }", "{ var a = 1 }", "{ let a = 1 }", "{ let a = 1; a }",
  "1; { let a = 2 }", "1; { var a = 2 }", "1; { function f() {} }", "1; { class A {} }", "1; if (true) function f() {}", "1; if (true) { function f() {} }",
  "if (true) { 1 } else { 2 }", "if (false) { 1 } else { 2 }", "if (true) { var z = 1 } else { 2 }", "9; if (true) { var z = 1 }", "9; if (true) var z = 1",
  "9; if (true) ;", "9; if (true) { ; }", "9; if (true) { 1; ; }", "9; if (true) 1; else ;",
  "1; do 2; while (false)", "1; do ; while (false)", "1; for (;;) break", "1; for (var i = 0; i < 1; i++) ;", "1; for (var i = 0; i < 1; i++) i; ",
  "1; for (var i of [1]) { break }", "1; for (var i of [1]) { 2; break }", "1; for (var i in { a: 1 }) { 2; continue }",
  "1; for (let i = 0; i < 2; i++) { i }", "1; for (const i of [4]) { i }", "1; for (const i in { a: 1 }) { i }",
  "1; while (false) ;", "1; while (true) { break }", "var n = 3; while (n--) { n }", "var n = 3; while (n--) { n; continue }",
  "var n = 3; while (n--) { if (n == 1) break; n }", "var n = 3; while (n--) { n; if (n == 1) break }",
  "1; try { 2; throw 0 } catch (e) { }", "1; try { 2 } finally { }", "1; try { 2; } finally { 3; }", "1; try { throw 0 } catch (e) { 7 } finally { }",
  "l: try { 1; break l } finally { 2 }", "1; l: try { 2; break l } finally { 3 }", "do { try { 1; break } finally { 2 } } while (false)",
  "do { try { 1; continue } finally { 2 } } while (false)", "do { 1; try { 2 } finally { break } } while (false)", "do { 1; try { throw 0 } finally { break } } while (false)",
  "1; for (var i = 0; i < 2; i++) { try { 2; continue } finally { 3 } }", "1; for (var i = 0; i < 2; i++) { try { 2; break } finally { } }",
  "1; switch (0) { case 0: try { 2 } finally { 3 } }", "1; switch (0) { case 0: l: { 2; break l } }", "1; switch (0) { case 0: 2; case 1: }",
  "1; switch (0) { case 0: 2; case 1: break; }", "1; switch (0) { case 0: 2; break; case 1: 3 }", "1; switch (3) { }", "1; switch (3) { default: }",
  "1; switch (3) { default: 4 }", "1; switch (0) { case 0: { } }", "1; switch (0) { case 0: { 5 } }",
  "1; label: for (;;) { 2; break label; }", "1; a: b: { 2; break a }", "1; a: b: 3", "a: { break a }", "1; a: { 2; b: { break a } }",
  "var o = { get x() { return 1 } }; o.x", "var o = { get x() { return 1 } }", "function f(a) { return a } f(3)", "function f(a) { return a }", "(function f(a) { return a })(4)",
  "1; try { eval('2') } finally { }", "eval('3')", "eval('3;;')", "eval('var q = 3')", "1; eval('')", "1; eval('4')", "1; eval(';')", "1; eval('{}')",
  "eval('1; eval(\"2\")')", "eval('1; (0, eval)(\"2; var\")')", "(0, eval)('3')", "(0, eval)('var q = 3; q')", "Function('return 5')()", "new Function('return 6')()",
  "1; new Function('2')()", "1; Function('')()",
  "1; for (var i = 0; i < 2; i++) { }", "for (var i = 0; i < 2; i++) { i; break }", "for (;;) { 1; break }", "for (;;) { break }",
  "var a = [1, 2]; for (var x of a) { x; if (x == 1) continue }", "var a = [1, 2]; for (var x of a) { if (x == 1) continue; x }",
  "var a = [1, 2]; for (var x of a) { if (x == 2) break; x }", "var a = [1, 2]; for (var x of a) { x; if (x == 2) break }",
  "1; yield_ = 3", "1; let y; y = 2", "let y = 3; y", "const y = 3; y", "let y; y", "let y", "var y; y", "var y",
  "if (1) 5; else 6;", "if (0) 5; else 6;", "if (1) { 5; } else { 6; }", "if (0) { 5; } else { }", "7; if (0) { 5; } else { }", "7; if (0) 5; else ;",
  "4; (function () { return 5 })()", "4; (() => { })()", "4; new (class { })", "4; void 0", "4; null", "4; undefined",
  "var s = ''; s += 'a'", "var s = 'x'; s += 'a'; s", "var o = {}; o.a = 1", "var o = {}; o.a = 1; o", "var a = []; a[1] = 2", "var a = []; a.push(1)",
  "1; ({}) ", "1; ({}); 2;", "1\n2", "1\n;\n2", "1 // c", "1 /* c */", "// c\n1", "/* c */", "/* c */ 1; /* d */", "'use strict'; 1; 2",
  "1; 'use strict'", "'a'; 'b'", "'a'\n'b'", "\"use strict\"\n2", "'use strict'; var sloppy = 1; sloppy",
];
for (const c of completions) {
  add(`try { globalThis.R = String((0, eval)(${q(c)})) } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
// Os mesmos valores de conclusão por eval direto dentro de função (escopo local).
for (const c of completions) {
  thin(5, `try { globalThis.R = String((function () { return eval(${q(c)}) })()) } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
// Valor de conclusão em modo estrito.
for (const c of completions) {
  thin(7, `try { globalThis.R = String((function () { 'use strict'; return eval(${q(c)}) })()) } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}

// ---- SyntaxError exatos de Function() e eval.
const evalErrors = [
  "", " ", "(", ")", "{", "}", "[", "]", "var", "var 1", "var a b", "let", "let let", "const", "const a", "const a;", "if", "if (", "if (1", "if (1) else",
  "for (", "for (;", "for (;;", "for (var", "for (var of", "while", "do", "do 1", "function", "function (", "function f(", "function f(a", "function f(a,", "function f(a) ", "function f(a) {",
  "function () {}", "function* () {}", "async function () {}", "class", "class A", "class A {", "class {}", "class A extends", "class A { constructor() {} constructor() {} }",
  "class A { get constructor() {} }", "class A { static prototype() {} }", "class A { #a; #a }", "class A { m() { #x } }", "class A { m() { this.#x } }", "class A { constructor() { super() } }",
  "1 +", "+", "1 + * 2", "a ? b", "a ? b :", "a =>", "=> a", "(a, b) =>", "(a, b) => {", "(a b) => 1", "async (a b) => 1", "async a b", "(...a, b) => 1", "(a, ...b,) => 1",
  "({ a: 1, a: 2 }, { __proto__: 1, __proto__: 2 })", "({ __proto__: 1, __proto__: 2 })", "({ get a(x) {} })", "({ set a() {} })", "({ set a(x, y) {} })", "({ async get a() {} })",
  "({ a b })", "({ a = 1 })", "({ a = 1 } = 1)", "[a = 1]", "[...a, b] = 1", "({ ...a, b } = 1)", "({ ...{ a } } = 1)", "[...[a]] = 1", "1 = 2", "a++ = 1", "++a++", "a + b = c",
  "new.target", "import.meta", "import", "import(", "import()", "import('a', 'b', 'c')", "export", "export default 1", "await 1", "yield 1", "async function f() { await }",
  "function* g() { yield\n* 1 }", "function* g() { var yield }", "async function f() { var await }", "function f() { 'use strict'; var yield }", "'use strict'; var yield", "'use strict'; var let",
  "'use strict'; var static", "'use strict'; var implements", "'use strict'; eval = 1", "'use strict'; arguments++", "'use strict'; function eval() {}", "'use strict'; (eval) => 1",
  "'use strict'; with (a) {}", "'use strict'; delete a", "'use strict'; delete (a)", "'use strict'; delete ((a))", "'use strict'; 010", "'use strict'; '\\01'", "'use strict'; 08", "'use strict'; 09.5",
  "'use strict'; function f(a, a) {}", "'use strict'; (a, a) => 1", "'use strict'; ({ m(a, a) {} })", "function f(a, a) { 'use strict' }", "function f(a = 1) { 'use strict' }", "function f({ a }) { 'use strict' }",
  "function f(...a) { 'use strict' }", "(a = 1) => { 'use strict' }", "function f(a, a = 1) {}", "(a, a) => 1", "function f(a, [a]) {}", "({ m(a, a) {} })", "async function f(a, a) {}", "function* g(a, a) {}",
  "let a; let a", "let a; var a", "var a; let a", "const a = 1; var a", "let a; { var a }", "{ let a; var a }", "{ var a; let a }", "function f() { let a; var a }", "function f(a) { let a }", "function f() { let a; function a() {} }",
  "{ function a() {} function a() {} }", "'use strict'; { function a() {} function a() {} }", "{ function a() {} var a }", "{ function* a() {} function a() {} }", "{ async function a() {} function a() {} }",
  "switch (1) { case 1: let a; case 2: let a }", "try {} catch (a) { let a }", "try {} catch (a) { var a }", "try {} catch ([a]) { var a }", "try {} catch (a) { for (var a of []) ; }", "try {} catch (a) { for (var a in {}) ; }",
  "try {} catch (a, b) {}", "try {}", "try {} catch {} finally", "catch (a) {}", "finally {}", "try { } catch (a) { } catch (b) { }",
  "for (let a of b, c) ;", "for (let of x) ;", "for (let a, b of c) ;", "for (var a = 1 of b) ;", "for (var a = 1 in b) ;", "'use strict'; for (var a = 1 in b) ;", "for (let a = 1 in b) ;", "for (a of b, c) ;",
  "for (async of []) ;", "for (let in {}) ;", "for (let.x of []) ;", "for (const a;;) ;", "for (const a in b, c) ;", "for (let [a] ;;) ;", "for (let a of []) { var a }", "for (let a;;) { var a }", "for (let a, a;;) ;",
  "break", "continue", "break x", "continue x", "x: x: ;", "x: { continue x }", "x: while (1) { function f() { break x } }", "return", "return 1", "{ return }", "if (1) return",
  "a: function* g() {}", "a: async function f() {}", "a: class A {}", "a: let b", "a: const b = 1", "if (1) class A {}", "if (1) let a", "if (1) const a = 1", "while (1) function f() {}", "if (1) function* g() {}",
  "if (1) async function f() {}", "'use strict'; if (1) function f() {}", "a: if (1) function f() {}", "a: while (1) a: ;", "label: function f() {}", "'use strict'; label: function f() {}", "do function f() {} while (0)",
  "1..a", "1.a", "1a", "1_", "1__0", "1_.5", "0_1", "0x", "0b", "0o", "0b2", "0o8", "0xg", "1e", "1e+", ".e1", "1n.5", "1.5n", "01n", "0x1.5", "08n", "1_000n", "1__0n",
  "'\\u{110000}'", "'\\u{}'", "'\\u12'", "'\\x1'", "'\\xg0'", "'\\u{1'", "`\\u{110000}`", "`\\unicode`", "`\\xg`", "`\\01`", "`${`", "`${1`", "`${}`", "`${1}", "`", "'", "\"", "'a\nb'", "\"a\nb\"", "/a/gg", "/a/x", "/(/", "/[/", "/a/ig", "/(?<a>)(?<a>)/", "/\\1(a)/u", "/\\p{x}/u", "/{1}/u", "/a{2,1}/",
  "/*", "/* /", "<!--", "-->", "1 --> 2", "\n--> c", "/**/ --> c", "#", "#!", " #!x", "@", "\\", "\\u0061", "\\u00", "\\u{61}", "a\\u{}", "var \\u{1F600}", "var \u{1F600}", "var a‌", "var ‌", "\u0000", "' '", "' '", "var ·", "var a·",
  "a?.b = 1", "a?.b++", "a?.[0] = 1", "new a?.b", "a?.`x`", "a?.b`x`", "a ?? b || c", "a || b ?? c", "a && b ?? c", "(a ?? b) || c", "-a ** b", "(-a) ** b", "typeof a ** b", "!a ** b", "await a ** b", "a ** -b",
  "async () => await", "async function* f() { yield await }", "async (a = await 1) => 1", "async function f(a = await 1) {}", "function* g(a = yield) {}", "function* g() { (a = yield) => 1 }", "async function f() { (a = await 1) => 1 }",
  "function f() { new.target = 1 }", "function f() { new.target++ }", "function f() { for (new.target of []) ; }", "new.target = 1", "function f() { ({ new.target } = 1) }", "function f() { new.target() }",
  "super", "super.a", "super()", "({ m() { super() } })", "({ m: function () { super.a } })", "({ m() { function f() { super.a } } })", "class A { m() { super() } }", "class A extends B { m() { super() } }", "class A extends B { constructor() { function f() { super() } } }", "class A extends B { constructor() { (() => super())() } }",
  "x = { a, b: }", "x = [1, 2", "x = (1, 2", "x = f(1,", "f(...)", "f(a b)", "f(,)", "f(a,,)", "[,] = 1", "[a,,] = 1", "[...a,] = 1", "[...a = 1] = 1", "({ ...a, } = 1)", "({ ...a = 1 } = 1)", "(a, b) = 1", "([a]) = 1", "({ a }) = 1", "({ a: (b) } = 1)", "({ a: (b = 1) } = 1)", "[(a)] = 1", "[(a = 1)] = 1", "[(a.b)] = 1", "[(a, b)] = 1",
  "let [a, a] = 1", "const { a, a } = 1", "var [a, a] = 1", "let { a: b, b } = 1", "let a, b, a", "let [a]", "const [a]", "let {a}", "var [a]", "var {a}", "let [a] = 1, b", "for (let [a, a] of []) ;",
  "class A { static { await } }", "class A { static { return } }", "class A { static { arguments } }", "class A { static { super() } }", "class A { static { break } }", "class A { static { var await } }", "class A { static { yield } }", "class A { static async *m() { await yield } }",
  "class A { x = arguments }", "class A { x = super() }", "class A { 'constructor' = 1 }", "class A { constructor = 1 }", "class A { static constructor = 1 }", "class A { static prototype = 1 }", "class A { static 'prototype' = 1 }", "class A { #constructor }", "class A { static #p; static #p }", "class A { get #p() {} set #p(v) {} get #p() {} }", "class A { get #p() {} static set #p(v) {} }", "class A { m() { delete this.#p } #p }", "class A { m() { delete (this.#p) } #p }", "class A { m() { this?.#p } #p }", "class A { m() { #p in #p in this } #p }", "class A { m() { 1 + #p in this } #p }", "class A { m() { #p } #p }", "class A { m() { return #p in this } }", "class A extends B, C {}", "class A extends (B, C) {}", "class A extends B = 1 {}", "class A { m() {} ; ; n() {} }", "class A { m() {} , n() {} }", "class A { , }", "class A { async\n m() {} }", "class A { get\n m() {} }", "class A { static\n m() {} }", "class A { *\n m() {} }", "class A { async *\n m() {} }", "class A { static async\n m() {} }", "class A { x\n y }", "class A { x y }", "class A { x = 1 y = 2 }", "class A { x = 1; y = 2 }", "class A { 'a' 'b' }", "class A { async x }", "class A { get x }", "class A { static x y }", "class A { static static }", "class A { static static() {} }", "class A { static get }", "class A { static get() {} }", "class A { async() {} }", "class A { async }", "class A { get() {} set() {} }", "class A { static async }",
  "var a = 1; var a = 2; a b", "var a = 1, ", "var a, ;", "var a = ;", "var = 1", "var [", "var {", "var {a", "var {a:", "var {a: }", "var {a: 1}", "var {1}", "var {'a'}", "var {a.b}", "var [a.b]", "var [1]", "var ...a", "var a = (", "var a = [", "var a = {", "var a = function", "var a = class", "var a = async", "var a = =>",
];
for (const src of evalErrors) {
  add(`try { (0, eval)(${q(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
// Erros por eval direto dentro de função (escopo de função, new.target e super conforme o contexto).
for (const src of evalErrors) {
  thin(4, `try { (function () { eval(${q(src)}); })(); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
// Os mesmos como corpo de Function e de AsyncFunction, e como lista de parâmetros.
for (const src of evalErrors) {
  thin(3, `try { Function(${q(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
for (const src of evalErrors) {
  thin(6, `try { (async function () {}).constructor(${q(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
for (const src of evalErrors) {
  thin(6, `try { (function* () {}).constructor(${q(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
for (const src of evalErrors) {
  thin(8, `try { (async function* () {}).constructor(${q(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}
for (const src of evalErrors) {
  thin(5, `try { Function(${q(src)}, 'return 1'); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`);
}

// ---- Execução.
const programs = pool.resolve();
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "dynfn-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (o bun transpila arquivos e muda a semântica de script),
// então o programa vai por ele; o SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "dynfn_source.js");
const file = path.join(dir, "dynfn_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
