// Gera tests/golden/call_edge_bun.tsv: chamadas e argumentos de borda medidos no bun 1.4.2. Cobre `arguments`
// (mapeado vs não mapeado, length, callee, Symbol.iterator, reatribuição de parâmetro, arrow, herdado), rest params,
// parâmetros default (TDZ entre parâmetros, escopo próprio), `apply` com array-like gigante, `call` com `this`
// primitivo em sloppy vs strict, `new` com retorno primitivo/objeto, spread de iteráveis customizados (iterador que
// lança, `return()`), tail call estrito vs sloppy, recursão profunda, getter/setter com `super`,
// `Function.prototype.toString` de métodos computados e `bind` com `new`, `length` e `name`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança). Caminho da máquina no resultado
// descarta o programa.
// Uso: bun scripts/gen-call-edge-golden.js > tests/golden/call_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Programa com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Expressão avaliada e convertida em texto (JSON quando possível).
const prelude = "function S(v){try{return typeof v==='symbol'?v.toString():typeof v==='undefined'?'undefined':Object.is(v,-0)?'-0':typeof v==='function'?'function':typeof v==='bigint'?v+'n':JSON.stringify(v)}catch(e){return String(v)}}";
const E = expr => T(`${prelude} R = S(${expr})`);
// Corpo com declarações, devolvendo valor por `R = S(...)`.
const B = body => T(`${prelude} ${body}`);

// ---- 1. `arguments`: mapeado vs não mapeado.
const paramForms = [
  ["function f(a, b)", "sloppy simples"],
  ["function f(a, b = 5)", "default"],
  ["function f(a, ...b)", "rest"],
  ["function f({a}, b)", "destructuring"],
  ["function f(a, b) { 'use strict';", "strict"],
];
const bodies = [
  "a = 9; return [arguments[0], arguments.length]",
  "arguments[0] = 8; return [a, arguments[0]]",
  "arguments[1] = 8; return [b, arguments[1], arguments.length]",
  "delete arguments[0]; arguments[0] = 7; return [a, arguments[0]]",
  "Object.defineProperty(arguments, '0', {value: 3}); return [a, arguments[0]]",
  "Object.defineProperty(arguments, '0', {writable: false}); a = 4; return [a, arguments[0]]",
  "Object.defineProperty(arguments, '0', {get() { return 6 }}); a = 4; return [a, arguments[0]]",
  "Object.defineProperty(arguments, '0', {value: 3, writable: false}); a = 4; return [a, arguments[0]]",
  "a = 2; b = 3; return [].slice.call(arguments)",
  "return Object.prototype.toString.call(arguments)",
  "return [typeof arguments.callee, arguments.length]",
  "return Object.getOwnPropertyNames(arguments).join()",
  "return Object.getOwnPropertyDescriptor(arguments, 'length')",
  "return Object.getOwnPropertyDescriptor(arguments, 'callee') && Object.keys(Object.getOwnPropertyDescriptor(arguments, 'callee')).join()",
  "return Object.getOwnPropertyDescriptor(arguments, Symbol.iterator).value === Array.prototype.values",
  "return Object.keys(arguments).join()",
  "arguments.length = 1; return [].slice.call(arguments)",
  "arguments[5] = 1; return [arguments.length, Object.keys(arguments).join()]",
  "return [...arguments]",
  "return Array.from(arguments)",
  "var g = () => arguments; return g() === arguments",
  "var g = () => arguments[0]; a = 11; return g()",
  "return JSON.stringify(arguments)",
  "return Object.isExtensible(arguments) + ',' + Object.isFrozen(arguments)",
];
for (const [head] of paramForms) {
  for (const body of bodies) {
    const strictMarker = head.endsWith("{'use strict';") || head.endsWith("'use strict';");
    const fn = strictMarker ? `${head} ${body} }` : `${head} { ${body} }`;
    B(`${fn} R = S(f(1, 2, 3))`);
    B(`${fn} R = S(f(1))`);
  }
}
// arguments.callee em strict, reatribuição e arrow/herdado.
B("function f() { 'use strict'; return arguments.callee } R = S(f())");
B("function f() { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set } R = S(f())");
B("function f() { return arguments.callee === f } R = S(f())");
B("function f(a = 1) { return arguments.callee === f } R = S(f())");
B("function f(...a) { return arguments.callee } R = S(f())");
B("function f() { arguments = 5; return arguments } R = S(f(1))");
B("function f() { 'use strict'; arguments = 5; return arguments } R = S(f(1))");
B("function f() { var arguments = 7; return arguments } R = S(f(1))");
B("function f() { var arguments; return arguments.length } R = S(f(1, 2))");
B("function f(arguments) { return arguments } R = S(f(1))");
B("function f(arguments = 4) { return arguments } R = S(f())");
B("function f() { function arguments() {} return typeof arguments } R = S(f(1))");
B("function f() { let arguments = 1; return arguments } R = S(f())");
B("function f() { return (() => (() => arguments.length)())() } R = S(f(1, 2, 3))");
B("function f() { return (() => arguments)() } R = S(f(1)[0])");
B("var g = () => typeof arguments; R = S(g())");
B("function f() { var g = () => { arguments = [9]; }; g(); return arguments } R = S(f(1))");
B("function f(a) { var g = () => { a = 3; }; g(); return arguments[0] } R = S(f(1))");
B("class C { m() { return (() => arguments.length)() } } R = S(new C().m(1, 2))");
B("class C { constructor() { this.n = arguments.length } } R = S(new C(1, 2, 3).n)");
B("class C { static x = (() => typeof arguments)() } R = S(C.x)");
B("function f() { return eval('arguments.length') } R = S(f(1, 2))");
B("function f(a) { eval('a = 5'); return arguments[0] } R = S(f(1))");
B("function f(a) { eval('var arguments = 5'); return arguments } R = S(f(1))");
B("function f() { 'use strict'; eval('var arguments = 5'); return arguments } R = S(f(1))");
B("function f(a) { return a ? f.apply(null, arguments) : arguments.length } R = S(f(0, 1, 2))");
B("function f() { return Array.prototype.map.call(arguments, x => x * 2) } R = S(f(1, 2, 3))");
B("function f() { return arguments.length } R = S(f(...[1, 2], ...'ab', ...new Set([5])))");
B("function f() { return arguments.length } R = S(f(...Array(1000)))");
B("function f() { return arguments.length } R = S(f.apply(null, {length: 3}))");
B("function f() { return arguments.length } R = S(f.apply(null, {length: 2, 0: 1}))");
B("function f() { return [].join.call(arguments) } R = S(f.apply(null, {length: 3, 0: 'a', 2: 'c'}))");
B("function f() { 'use strict'; Object.defineProperty(arguments, 'length', {value: 10}); return arguments.length } R = S(f(1))");
B("function f() { Object.freeze(arguments); arguments[0] = 2; return arguments[0] } R = S(f(1))");
B("function f(a) { Object.freeze(arguments); a = 2; return arguments[0] } R = S(f(1))");
B("function f(a) { Object.seal(arguments); a = 2; return arguments[0] } R = S(f(1))");
B("function f(a) { Object.preventExtensions(arguments); a = 2; return arguments[0] } R = S(f(1))");
B("function f(a, a) { return [a, arguments[0], arguments[1]] } R = S(f(1, 2))");
B("function f(a, a) { arguments[0] = 9; return a } R = S(f(1, 2))");
B("function f(a, a) { arguments[1] = 9; return a } R = S(f(1, 2))");
B("function f(a) { arguments.length = 0; return [a, arguments[0]] } R = S(f(1))");
B("function f(a) { arguments[Symbol.iterator] = undefined; return [...arguments] } R = S(f(1))");
B("function f() { arguments[Symbol.iterator] = function* () { yield 'x' }; return [...arguments] } R = S(f(1, 2))");
B("function f() { delete arguments[Symbol.iterator]; return [...arguments] } R = S(f(1))");
B("function f() { return Object.getPrototypeOf(arguments) === Object.prototype } R = S(f())");
B("function f() { return Object.getPrototypeOf(arguments) === Object.prototype } R = S(f.apply(null, []))");
B("function f() { Object.setPrototypeOf(arguments, Array.prototype); return arguments.map(x => x + 1) } R = S(f(1, 2))");
B("function f() { return typeof arguments[Symbol.toStringTag] } R = S(f())");
B("function f() { return arguments.hasOwnProperty('callee') } R = S(f())");
B("function f() { 'use strict'; return arguments.hasOwnProperty('callee') } R = S(f())");
B("function f() { return Reflect.ownKeys(arguments).map(String).join() } R = S(f(1, 2))");

// ---- 2. Rest params.
B("function f(...r) { return r } R = S(f())");
B("function f(a, ...r) { return r } R = S(f(1, 2, 3))");
B("function f(...r) { return f.length } R = S(f(1, 2))");
B("function f(a, b, ...r) { return f.length } R = S(f(1))");
B("function f(a = 1, ...r) { return f.length } R = S(f())");
B("function f(...[a, b]) { return [a, b] } R = S(f(1, 2, 3))");
B("function f(...{length}) { return length } R = S(f(1, 2, 3))");
B("function f(...r) { r.push(1); return arguments.length } R = S(f(1, 2))");
B("function f(...r) { r[0] = 9; return arguments[0] } R = S(f(1))");
B("function f(...r) { return Array.isArray(r) && Object.getPrototypeOf(r) === Array.prototype } R = S(f())");
B("function f(...r) { 'use strict'; return r }");
E("(function (...r) { return r.length })(...Array(5))");
E("((...r) => r.length)(...'abc')");
E("(function (a, ...r) { return typeof arguments })()");
E("(function (...r) { return Object.getOwnPropertyDescriptor(arguments, 'callee') })()");
B("var o = { m(...r) { return r.length } }; R = S(o.m(1, 2, 3))");
B("class C { constructor(...r) { this.n = r.length } } R = S(new C(1, 2).n)");
B("class C extends Array { constructor(...r) { super(...r) } } R = S(new C(3).length)");
B("function f(...r) { return r.length } R = S(f.apply(null, new Array(65536)))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: 100000}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: 1e7}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: 2 ** 32}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: -1}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: 2 ** 53}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: '3'}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: NaN}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: Infinity}))");
B("function f(...r) { return r.length } R = S(f.apply(null, {length: 2.9}))");
B("function f() { return arguments.length } R = S(f.apply(null, 1))");
B("function f() { return arguments.length } R = S(f.apply(null, 'ab'))");
B("function f() { return arguments.length } R = S(f.apply(null, null))");
B("function f() { return arguments.length } R = S(f.apply(null, undefined))");
B("function f() { return arguments.length } R = S(f.apply(null, Symbol()))");
B("function f() { return arguments.length } R = S(f.apply(null, new Proxy([1, 2], {})))");
B("function f() { return arguments.length } R = S(f.apply(null, {get length() { throw new Error('len') }}))");
B("function f() { return arguments[0] } R = S(f.apply(null, {length: 1, get 0() { throw new Error('idx') }}))");
B("function f() { return arguments.length } R = S(f.apply(null, (function () { return arguments })(1, 2, 3)))");
B("function f() { return arguments.length } R = S(Reflect.apply(f, null, {length: 2}))");
B("function f() { return arguments.length } R = S(Reflect.apply(f, null, 1))");
B("function f() { return arguments.length } R = S(Reflect.apply(f, null))");
B("function f() { return arguments.length } R = S(Reflect.construct(f, {length: 2}))");
B("R = S(Math.max.apply(null, Array(10000).fill(1)))");
B("R = S(Math.max.apply(null, Array(200000).fill(1)))");
B("R = S(Math.max.apply(null, Array(1000000).fill(1)))");
B("R = S(String.fromCharCode.apply(null, Array(70000).fill(65)).length)");
B("R = S(Math.max(...Array(200000).fill(1)))");
B("R = S(Math.max(...Array(1000000).fill(1)))");
B("R = S([].concat.apply([], Array(50000).fill([1])).length)");
B("R = S(Array.prototype.push.apply([], Array(100000).fill(0)))");
B("R = S(new Function('return arguments.length').apply(null, Array(120000).fill(0)))");
B("R = S(Function.prototype.call.call(function () { return arguments.length }, null, 1, 2))");
B("R = S(Function.prototype.apply.call(function () { return arguments.length }, null, [1, 2, 3]))");
B("R = S(Function.prototype.call.apply(function () { return this }, [5]) instanceof Number)");

// ---- 3. Parâmetros default: TDZ e escopo próprio.
B("function f(a = b, b) { return [a, b] } R = S(f(1, 2))");
B("function f(a = b, b) { return [a, b] } R = S(f())");
B("function f(a = b, b = 1) { return [a, b] } R = S(f(undefined, 2))");
B("function f(a = () => b, b) { return a() } R = S(f(undefined, 5))");
B("function f(a = () => b, b = 3) { var b = 9; return [a(), b] } R = S(f())");
B("function f(a = () => b, b = 3) { b = 9; return a() } R = S(f())");
B("function f(a, b = a) { return b } R = S(f(4))");
B("function f(a, b = () => a) { a = 5; return b() } R = S(f(4))");
B("function f(a, b = () => a) { var a = 6; return [a, b()] } R = S(f(4))");
B("function f(a, b = () => a) { var a; return [a, b()] } R = S(f(4))");
B("function f(a, b = () => a) { var a = 6; a = 7; return [a, b()] } R = S(f(4))");
B("function f(a = 1, b = () => a) { { var a = 2 } return [a, b()] } R = S(f())");
B("function f(a = x, x = 1) { return a } R = S(f())");
B("function f(a = a) { return a } R = S(f())");
B("function f(a = a) { return a } R = S(f(3))");
B("function f(a = typeof a) { return a } R = S(f())");
B("function f(a = eval('b'), b = 2) { return a } R = S(f())");
B("function f(a = eval('var z = 1'), b = z) { return [b, typeof z] } R = S(f())");
B("function f(a = eval('var z = 1')) { var z = 2; return z } R = S(f())");
B("var x = 'outer'; function f(a = x) { var x = 'inner'; return a } R = S(f())");
B("var x = 'outer'; function f(a = x) { { let x = 'inner'; } return a } R = S(f())");
B("var x = 'outer'; function f(a = () => x) { var x = 'inner'; return a() } R = S(f())");
B("var x = 'outer'; function f(a = () => x) { let x = 'inner'; return a() } R = S(f())");
B("function f(a = 1) { function a() {} return typeof a } R = S(f())");
B("function f(a = 1) { var a; return a } R = S(f())");
B("function f(a = 1) { var a = 2; return a } R = S(f())");
B("function f(a, b = 1) { return arguments.length } R = S(f(1, undefined, undefined))");
B("function f(a = 1) { return arguments[0] } R = S(f())");
B("function f(a = 1) { a = 5; return arguments[0] } R = S(f(2))");
B("function f(a = 1, b = arguments[0]) { return b } R = S(f(7))");
B("function f(a = arguments) { return a.length } R = S(f(undefined, 2))");
B("function f(a = () => arguments) { return a()[0] } R = S(f(undefined, 2))");
B("function f(a = this) { return a === globalThis } R = S(f())");
B("function f(a = this) { 'use strict'; return a } R = S(f())");
B("function f(a = new.target) { return a === undefined } R = S(f())");
B("function f(a = new.target) { return a === f } R = S(new f() instanceof f)");
B("function f(a = f) { return a === f } R = S(f())");
B("var f = function g(a = g) { return a === g }; R = S(f())");
B("function f(a = (b = 2) => b, b) { return a() } R = S(f())");
B("function f(a = 1, [b] = [a + 1], {c} = {c: b + 1}) { return [a, b, c] } R = S(f())");
B("function f({a} = {a: 1}, [b] = [2]) { return [a, b] } R = S(f())");
B("function f([a] = []) { return a } R = S(f(null))");
B("function f({a}) { return a } R = S(f())");
B("function f({a}) { return a } R = S(f(null))");
B("function f({a} = null) { return a } R = S(f())");
B("function f(a = (() => { throw new Error('boom') })()) { return a } R = S(f(1))");
B("function f(a = (() => { throw new Error('boom') })()) { return a } R = S(f())");
B("var c = 0; function f(a = ++c) { return a } f(); f(1); f(); R = S(c)");
B("function f(a = 1, b) { return f.length } R = S(f())");
B("function f(a, b = 1, c) { return f.length } R = S(f())");
B("function f(a = 1, b = a + 1) { return [a, b] } R = S(f(undefined, undefined))");
B("function f(a = 1, b = a + 1) { return [a, b] } R = S(f(null, null))");
B("function f(a = yield1) { return a } var yield1 = 3; R = S(f())");
B("function* g(a = 1) { yield a } R = S([...g()])");
B("function* g(a = (() => { throw new Error('early') })()) { yield 1 } try { g() } catch (e) { R = 'threw ' + e.message }");
B("async function g(a = (() => { throw new Error('early') })()) {} var p = g(); R = S(p instanceof Promise)");
B("var o = { m(a = this.v) { return a }, v: 5 }; R = S(o.m())");
B("class C { m(a = super.x) { return a } } C.prototype.__proto__ = {x: 8}; R = S(new C().m())");
B("class C { static m(a = C) { return a === C } } R = S(C.m())");
B("class C { constructor(a = this) { } } R = S(0)");
B("class C { constructor(a = this) { } } class D extends C { constructor(a = this) { super(); } } R = S(new D())");
B("class D extends Object { constructor(a = super()) { } } R = S(new D() instanceof D)");
B("var f = (a = 1, b = a) => [a, b]; R = S(f())");
B("var f = (a = b, b) => [a, b]; R = S(f())");
B("var f = (a, b = () => a) => { var a = 3; return b() }; R = S(f(1))");
B("var f = (a = () => b, b = 2) => { let c = 1; return a() }; R = S(f())");
E("(function (a, b = 2) {}).length");
E("(function (a = 1, b) {}).length");
E("(function (...a) {}).length");
E("(function ({a}, [b]) {}).length");
E("((a, b = 1, c) => 0).length");
E("(async (a, b = 1) => 0).length");

// ---- 4. `call`/`apply` com `this` primitivo: sloppy vs strict (boxing).
const primitives = ["1", "'s'", "true", "Symbol.iterator", "10n", "null", "undefined", "NaN", "0", "''", "false", "-0"];
for (const p of primitives) {
  B(`function f() { return typeof this } R = S(f.call(${p}))`);
  B(`function f() { 'use strict'; return typeof this } R = S(f.call(${p}))`);
  B(`function f() { return this === globalThis } R = S(f.call(${p}))`);
  B(`function f() { return Object.prototype.toString.call(this) } R = S(f.call(${p}))`);
  B(`function f() { return this } R = S(typeof f.apply(${p}, []) + ',' + (f.apply(${p}, []) === ${p}))`);
  B(`var f = (() => typeof this).bind(${p}); R = S(f())`);
  B(`function f() { return typeof this } R = S(f.bind(${p})())`);
  B(`function f() { 'use strict'; return typeof this } R = S(f.bind(${p})())`);
  B(`function f() { return typeof this } R = S(Reflect.apply(f, ${p}, []))`);
}
B("function f() { return this } R = S(f.call(1) === f.call(1))");
B("function f() { return this } R = S(f.call(1).valueOf())");
B("function f() { this.x = 1; return this } R = S(typeof f.call('a').x)");
B("function f() { 'use strict'; this.x = 1 } R = S(f.call('a'))");
B("function f() { 'use strict'; this.x = 1 } R = S(f.call(undefined))");
B("function f() { this.x = 1; return globalThis.x } R = S(f())");
B("function f() { 'use strict'; return this } R = S(f())");
B("function f() { return this === globalThis } R = S(f())");
B("function f() { return this === globalThis } R = S([1].map(f)[0])");
B("function f() { 'use strict'; return this === undefined } R = S([1].map(f)[0])");
B("function f() { return typeof this } R = S([1].map(f, 'x')[0])");
B("function f() { 'use strict'; return typeof this } R = S([1].map(f, 'x')[0])");
B("var o = {f() { return typeof this }}; R = S((0, o.f)())");
B("var o = {f() { 'use strict'; return typeof this }}; R = S((0, o.f)())");
B("var o = {f() { return this === o }}; R = S([o.f][0]())");
B("var o = {f() { return this === o }}; R = S((o.f)())");
B("var o = {f() { return this === o }}; R = S((o.f = o.f)())");
B("var o = {f() { return this === o }}; R = S((true && o.f)())");
B("var o = {f() { return this === o }}; R = S(o?.f())");
B("var o = {f() { return this === o }}; R = S((o?.f)())");
B("var o = {f() { return this === o }}; R = S(o['f']())");
B("var o = {f() { return this === o }}; R = S(o.f``)");
B("function f() { return typeof this } R = S(f.call(new Boolean(false)) + typeof f.call(Object(1n)))");
B("Number.prototype.me = function () { return typeof this }; R = S((5).me())");
B("Number.prototype.me = function () { 'use strict'; return typeof this }; R = S((5).me())");
B("String.prototype.me = function () { return this === 'a' }; R = S('a'.me())");
B("String.prototype.me = function () { 'use strict'; return this === 'a' }; R = S('a'.me())");
B("Symbol.prototype.me = function () { return typeof this }; R = S(Symbol().me())");
B("BigInt.prototype.me = function () { 'use strict'; return typeof this }; R = S((1n).me())");
B("Object.defineProperty(Number.prototype, 'me', {get() { return typeof this }}); R = S((5).me)");
B("Object.defineProperty(Number.prototype, 'me', {get() { 'use strict'; return typeof this }}); R = S((5).me)");
B("Object.defineProperty(Number.prototype, 'me', {set(v) { R = typeof this }}); (5).me = 1");
B("Object.defineProperty(Number.prototype, 'me', {set(v) { 'use strict'; R = typeof this }}); (5).me = 1");
B("function f() { return typeof this } R = S(new Proxy(f, {}).call(1))");
B("function f() { return typeof this } R = S(new Proxy(f, {apply(t, th, a) { return typeof th }}).call(1))");
B("function f() { return typeof this } R = S(Function.prototype.call.call(f, 1))");
B("R = S(typeof (function () { return this }).call(1))");
B("R = S(Object.prototype.toString.call(function () { return this }.call('x')))");
B("R = S((function () { return this }).call(null) === globalThis)");
B("R = S((function () { 'use strict'; return this }).call(null))");
B("R = S((function () { return this }).call(Object.create(null)) !== globalThis)");
B("function f() { return typeof this } R = S(f.call(f.call(1)))");
B("function f() { return arguments.length } R = S(f.call())");
B("function f() { return arguments.length } R = S(f.call(null))");
B("function f() { return this } R = S(f.call() === globalThis)");
B("function f() { 'use strict'; return this } R = S(f.call())");
B("function f() { return eval('typeof this') } R = S(f.call(1))");
B("function f() { return (() => typeof this)() } R = S(f.call(1))");
B("function f() { return (() => typeof this)() } R = S(f.call(null))");
B("function f() { 'use strict'; return (() => typeof this)() } R = S(f.call(1))");
B("function f() { return new.target } R = S(f.call(1))");
B("function f() { return typeof this } R = S(new f() === undefined)");
B("function f() { return typeof new.target } R = S(Reflect.construct(f, [], Object))");
B("var g = function () { return typeof this }; R = S(g.call(true))");
B("var g = { m: function () { return typeof this } }.m; R = S(g.call(true))");
B("var g = { async m() { return typeof this } }.m; g.call(1).then(v => { globalThis.R = v })");
B("var g = { *m() { yield typeof this } }.m; R = S(g.call(1).next().value)");
B("var g = { *m() { 'use strict'; yield typeof this } }.m; R = S(g.call(1).next().value)");

// ---- 5. `new` com retorno primitivo/objeto.
const returns = ["1", "'s'", "true", "null", "undefined", "Symbol()", "10n", "{a: 1}", "[1]", "function () {}", "new Number(1)", "new String('x')", "Object(Symbol())", "Object(1n)", "NaN", "this"];
for (const r of returns) {
  B(`function F() { this.k = 1; return ${r} } var v = new F(); R = S([typeof v, v instanceof F, v && typeof v.k])`);
  B(`class C { constructor() { return ${r} } } try { var v = new C(); R = S([typeof v, v instanceof C]) } catch (e) { R = e.name + ': ' + e.message }`);
  B(`class B {} class D extends B { constructor() { super(); return ${r} } } try { var v = new D(); R = S([typeof v, v instanceof D]) } catch (e) { R = e.name + ': ' + e.message }`);
  B(`class B {} class D extends B { constructor() { return ${r} } } try { var v = new D(); R = S([typeof v, v instanceof D]) } catch (e) { R = e.name + ': ' + e.message }`);
}
B("function F() { return Object(1) } R = S(new F() instanceof Number)");
B("function F() {} F.prototype = 5; R = S(Object.getPrototypeOf(new F()) === Object.prototype)");
B("function F() {} F.prototype = null; R = S(Object.getPrototypeOf(new F()) === Object.prototype)");
B("function F() {} F.prototype = Symbol(); R = S(Object.getPrototypeOf(new F()) === Object.prototype)");
B("function F() {} F.prototype = function () {}; R = S(new F() instanceof F)");
B("function F() {} Object.defineProperty(F, 'prototype', {get() { throw new Error('proto') }}); R = S(new F())");
B("var F = () => {}; R = S(new F())");
B("var F = async function () {}; R = S(new F())");
B("var F = function* () {}; R = S(new F())");
B("var o = {m() {}}; R = S(new o.m())");
B("var o = {get x() { return 1 }}; R = S(new (Object.getOwnPropertyDescriptor(o, 'x').get)())");
B("R = S(new Math.max())");
B("R = S(typeof new Date().getTime())");
B("R = S(new (class { static x = 1 })().x)");
B("R = S(new new Function('this.a = 1'))");
B("R = S(new (function () { this.a = 1 }))");
B("R = S(new (function () { return () => 1 })())");
B("function F() { return new.target } R = S(new F() === F)");
B("function F() { return new.target } R = S(Reflect.construct(F, [], Array) === Array)");
B("function F() { this.n = new.target.name } R = S(Reflect.construct(F, [], class Z {}).n)");
B("function F() {} R = S(Reflect.construct(F, [], Math.max))");
B("function F() {} R = S(Reflect.construct(F, [], () => {}))");
B("function F() {} R = S(Reflect.construct(F, [], function () {}.bind()))");
B("R = S(Reflect.construct(Date, [], Object) instanceof Date)");
B("R = S(Object.prototype.toString.call(Reflect.construct(Date, [0], Object)))");
B("R = S(Object.getPrototypeOf(Reflect.construct(Array, [], Object)) === Object.prototype)");
B("class A { constructor() { this.t = new.target } } class B extends A {} R = S(new B().t === B)");
B("class A {} R = S(A())");
B("class A {} R = S(A.call({}))");
B("class A extends null {} R = S(new A())");
B("class A extends null { constructor() { return Object.create(A.prototype) } } R = S(new A() instanceof A)");
B("class A { constructor() { return 1 } } R = S(typeof new A())");
B("class A { constructor() { this.x = 1 } } class B extends A { constructor() { super(); super() } } R = S(new B())");
B("class A {} class B extends A { constructor() { this.x = 1; super() } } R = S(new B())");
B("class A {} class B extends A { constructor() { } } R = S(new B())");
B("class A {} class B extends A { constructor() { return undefined } } R = S(new B())");
B("class A {} class B extends A { constructor() { var f = () => super(); f(); f() } } R = S(new B())");
B("class A {} class B extends A { constructor() { var f = () => super(); f(); return this } } R = S(new B() instanceof B)");
B("class A { constructor() { return {z: 1} } } class B extends A { constructor() { super(); this.y = 2 } } R = S(new B())");
B("function A() { return {z: 1} } class B extends A { constructor() { super(); this.y = 2 } } R = S([new B(), new B() instanceof B])");
B("function A() {} A.prototype.p = 1; class B extends A {} R = S(new B().p)");
B("class A { #p = 1; static has(o) { return #p in o } } class B extends A { constructor() { return Object.create(null) } } R = S(A.has(new B()))");
B("class A { constructor(o) { return o } } class B extends A { #p = 1; static has(o) { return #p in o } } var o = {}; new B(o); try { new B(o) } catch (e) { R = e.name + ': ' + e.message }");
B("class A { constructor(o) { return o } } class B extends A { x = 1 } var o = {}; new B(o); R = S(o)");
B("class A { constructor(o) { return o } } class B extends A { x = 1 } var o = Object.freeze({}); try { new B(o) } catch (e) { R = e.name + ': ' + e.message }");

// ---- 6. Spread de iteráveis customizados.
const log = "var log = [];";
const iterable = (nextBody, withReturn) =>
  `{ [Symbol.iterator]() { var i = 0; return { next() { ${nextBody} }${withReturn ? ", return() { log.push('return'); return {} }" : ""} } } }`;
B(`${log} var it = ${iterable("i++; if (i > 3) return {done: true}; return {value: i, done: false}", true)}; R = S([[...it], log])`);
B(`${log} var it = ${iterable("i++; if (i == 2) throw new Error('next'); return {value: i, done: false}", true)}; try { [...it] } catch (e) { R = S([e.message, log]) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; var [a, b] = it; R = S([a, b, log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; var [a, ...b] = [1, 2]; R = S([a, b, log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; var [] = it; R = S(log)`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; var [,] = it; R = S(log)`);
B(`${log} var it = ${iterable("i++; return {value: i, done: i > 1}", true)}; var [a, b, c] = it; R = S([a, b, c, log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; for (var x of it) { if (x == 2) break } R = S([x, log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; for (var x of it) { if (x == 2) continue; if (x == 3) break } R = S([x, log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; try { for (var x of it) { throw new Error('body') } } catch (e) { R = S([e.message, log]) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; (function () { for (var x of it) { return 1 } })(); R = S(log)`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; outer: for (var j of [1]) { for (var x of it) { continue outer } } R = S(log)`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, return() { throw new Error('ret') } } } }; try { for (var x of it) { throw new Error('body') } } catch (e) { R = S(e.message) }`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, return() { throw new Error('ret') } } } }; try { for (var x of it) { break } } catch (e) { R = S(e.message) }`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, return() { return 1 } } } }; try { for (var x of it) { break } } catch (e) { R = e.name + ': ' + e.message }`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, return: 1 } } }; try { for (var x of it) { break } } catch (e) { R = e.name + ': ' + e.message }`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, return: null } } }; for (var x of it) { break } R = 'ok'`);
B(`${log} var it = { [Symbol.iterator]() { return { next() { return {done: false} }, get return() { log.push('get'); return undefined } } } }; for (var x of it) { break } R = S(log)`);
B(`var it = { [Symbol.iterator]() { return { next() { return 1 } } } }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = { [Symbol.iterator]() { return { next() { return null } } } }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = { [Symbol.iterator]() { return 1 } }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = { [Symbol.iterator]() { return {} } }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = { [Symbol.iterator]: 1 }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = { [Symbol.iterator]: null }; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`var it = {}; try { [...it] } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { [...1] } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { [...undefined] } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { [...null] } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { Math.max(...undefined) } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { Math.max(...{}) } catch (e) { R = e.name + ': ' + e.message }`);
B(`try { new Date(...5) } catch (e) { R = e.name + ': ' + e.message }`);
B(`var o = {}; try { (function () {})(...o.x) } catch (e) { R = e.name + ': ' + e.message }`);
B(`R = S([...'a\\u{1F600}b'].length)`);
B(`R = S([...new Map([[1, 2]])])`);
B(`R = S([...new Set([1, 1, 2]).values()])`);
B(`R = S(Math.max(...new Set([1, 5, 2])))`);
B(`R = S([...{ *[Symbol.iterator]() { yield 1; yield 2 } }])`);
B(`${log} function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } var [a] = g(); R = S([a, log])`);
B(`${log} function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } [...g()]; R = S(log)`);
B(`${log} function* g() { try { yield 1; yield 2 } finally { log.push('fin'); yield 'extra' } } for (var x of g()) { log.push(x); break } R = S(log)`);
B(`${log} function* g() { try { yield 1 } finally { throw new Error('fin') } } try { for (var x of g()) { break } } catch (e) { R = S(e.message) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: i > 2}", true)}; function f(...r) { return r } R = S([f(...it), log])`);
B(`${log} var it = ${iterable("i++; if (i == 2) throw new Error('n'); return {value: i, done: false}", true)}; function f(...r) { return r } try { f(...it) } catch (e) { R = S([e.message, log]) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: i > 2}", true)}; R = S([Array.from(it), log])`);
B(`${log} var it = ${iterable("i++; return {value: i, done: i > 2}", true)}; R = S([new Set(it).size, log])`);
B(`${log} var it = ${iterable("i++; return {value: [i, i], done: i > 2}", true)}; R = S([new Map(it).size, log])`);
B(`${log} var it = ${iterable("i++; return {value: 1, done: false}", true)}; try { new Map(it) } catch (e) { R = S([e.name, log]) }`);
B(`${log} var it = ${iterable("i++; return {value: 1, done: false}", true)}; try { new Map(it) } catch (e) { R = e.message }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; try { Object.fromEntries(it) } catch (e) { R = S([e.name, log]) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: false}", true)}; try { Promise.all(it); R = 'sync' } catch (e) { R = S(e.name) }`);
B(`${log} var it = ${iterable("i++; return {value: i, done: i > 2}", true)}; var {0: a} = [...it]; R = S([a, log])`);
B(`var it = { [Symbol.iterator]() { return { next() { return {done: true} }, [Symbol.iterator]() { return this } } } }; R = S([...it])`);
B(`var calls = 0; var a = [1, 2]; a[Symbol.iterator] = function () { calls++; return [][Symbol.iterator].call(this) }; [...a]; R = S(calls)`);
B(`var a = [1, 2, 3]; var out = []; for (var x of a) { if (x == 1) a.push(4); out.push(x) } R = S(out)`);
B(`var a = [1, 2, 3]; var out = []; for (var x of a) { if (x == 1) a.length = 1; out.push(x) } R = S(out)`);
B(`var saved = Array.prototype[Symbol.iterator]; Array.prototype[Symbol.iterator] = function* () { yield 'patched' }; try { R = S([...[1, 2]]) } finally { Array.prototype[Symbol.iterator] = saved }`);
B(`var saved = Array.prototype[Symbol.iterator]; Array.prototype[Symbol.iterator] = function* () { yield 'patched' }; try { R = S(Math.max(...[1, 2])) } finally { Array.prototype[Symbol.iterator] = saved }`);
B(`var proto = Object.getPrototypeOf([][Symbol.iterator]()); var saved = proto.next; proto.next = function () { return {done: true} }; try { R = S([...[1, 2]]) } finally { proto.next = saved }`);
B(`var proto = Object.getPrototypeOf([][Symbol.iterator]()); var saved = proto.next; proto.next = function () { return {done: true} }; try { R = S(Math.max(...[1, 2])) } finally { proto.next = saved }`);
B(`var proto = Object.getPrototypeOf([][Symbol.iterator]()); var saved = proto.next; proto.next = function () { return {done: true} }; try { var [a] = [1]; R = S(a) } finally { proto.next = saved }`);
B(`function f() { return arguments.length } R = S(f(...[1, 2], ...[], ...[3]))`);
B(`function f(a, b, c) { return [a, b, c] } R = S(f(1, ...[2, 3]))`);
B(`function f(a, b, c) { return [a, b, c] } R = S(f(...[1], 2, ...[3]))`);
B(`function f() { return this } var o = {f}; R = S(o.f(...[]) === o)`);
B(`class A { constructor(...a) { this.a = a } } R = S(new A(...[1, 2], 3).a)`);
B(`R = S(new Array(...[3]).length)`);
B(`R = S(new Date(...[2020, 0, 1]).getFullYear())`);
B(`R = S([..."ab", ..."cd"])`);
B(`R = S([...[1, , 3]].hasOwnProperty(1))`);
B(`R = S({...[1, 2]})`);
B(`R = S({...'ab', ...null, ...undefined, ...1})`);
B(`var log = []; var o = { get a() { log.push('a'); return 1 }, get b() { log.push('b'); return 2 } }; var c = {...o}; R = S([c, log])`);
B(`var o = {...{get a() { return 1 }}}; R = S(Object.getOwnPropertyDescriptor(o, 'a'))`);
B(`var o = {...Object.defineProperty({}, 'a', {value: 1, enumerable: false})}; R = S(Object.keys(o))`);
B(`var s = Symbol('s'); var o = {...{[s]: 1}}; R = S(Object.getOwnPropertySymbols(o).length)`);
B(`var o = {...new Proxy({a: 1}, {ownKeys() { return ['a', 'b'] }, getOwnPropertyDescriptor(t, k) { return {value: 1, enumerable: true, configurable: true} }, get(t, k) { return k }})}; R = S(o)`);

// ---- 7. Tail calls profundos, estrito vs sloppy, e recursão.
for (const depth of [100, 1000, 10000, 20000, 100000, 1000000]) {
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : f(n - 1) } R = S(f(${depth}))`);
  B(`function f(n) { return n == 0 ? 'done' : f(n - 1) } R = S(f(${depth}))`);
  B(`function f(n) { 'use strict'; if (n == 0) return 'done'; return f(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; var f = n => n == 0 ? 'done' : f(n - 1); R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : g(n - 1) } function g(n) { return f(n) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : 1 + f(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { try { return n == 0 ? 'done' : f(n - 1) } catch (e) { return 'caught ' + e.name } } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { if (n == 0) return 'done'; return f.call(null, n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { if (n == 0) return 'done'; return f.apply(null, [n - 1]) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { if (n == 0) return 'done'; return f.bind(null)(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; class C { m(n) { return n == 0 ? 'done' : this.m(n - 1) } } R = S(new C().m(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : f(n - 1, 1, 2, 3, 4, 5, 6, 7, 8) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n, a, b, c, d, e) { return n == 0 ? 'done' : f(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : f(n - 1, ...[]) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : (0, f)(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : eval('f')(n - 1) } R = S(f(${depth}))`);
  B(`'use strict'; function f(n) { return n == 0 ? 'done' : f(n - 1) } function g() { return f(${depth}) } R = S(g() + h()); function h() { return 1 }`);
  B(`function f(n) { return n == 0 ? 0 : 1 + f(n - 1) } R = S(f(${depth}))`);
  B(`var f = function (n) { return n == 0 ? 0 : 1 + f(n - 1) }; R = S(f(${depth}))`);
  B(`function f(n) { return n == 0 ? [] : [f(n - 1)] } var r = f(${Math.min(depth, 10000)}); R = S(typeof r)`);
  B(`function f(n) { return n == 0 ? 0 : [1].map(() => f(n - 1))[0] } R = S(f(${Math.min(depth, 3000)}))`);
  B(`function* g(n) { if (n > 0) yield* g(n - 1); yield n } R = S([...g(${Math.min(depth, 2000)})].length)`);
  B(`function f(n) { return n == 0 ? 0 : f(n - 1) + 1 } try { f(${depth}); R = 'ok' } catch (e) { R = e instanceof RangeError }`);
  B(`class A { constructor(n) { this.c = n == 0 ? null : new A(n - 1) } } try { new A(${Math.min(depth, 10000)}); R = 'ok' } catch (e) { R = e.name }`);
}
B("function f() { f() } try { f() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { f() } try { f() } catch (e) { R = e instanceof RangeError }");
B("var o = {get x() { return this.x }}; try { o.x } catch (e) { R = e.name + ': ' + e.message }");
B("var o = {set x(v) { this.x = v }}; try { o.x = 1 } catch (e) { R = e.name + ': ' + e.message }");
B("var p = new Proxy({}, {get(t, k, r) { return r[k] }}); try { p.x } catch (e) { R = e.name + ': ' + e.message }");
B("var o = {toString() { return String(this) }}; try { String(o) } catch (e) { R = e.name + ': ' + e.message }");
B("class A { constructor() { new A() } } try { new A() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { new f() } try { new f() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { return f.bind()() } try { f() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { return [1].map(f) } try { f() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { return JSON.stringify({toJSON: f}) } try { f() } catch (e) { R = e.name + ': ' + e.message }");
B("function f() { try { f() } finally { } } try { f() } catch (e) { R = e.name }");
B("var depth = 0; function f() { depth++; try { f() } catch (e) { depth--; throw e } } try { f() } catch (e) { R = S(depth) }");
B("var d = 0; function f() { d++; f() } try { f() } catch (e) { var first = d; d = 0; try { f() } catch (e2) { R = S(Math.abs(first - d) < first * 0.2) } }");
B("function f() { try { f() } catch (e) { return typeof e } } R = S(f())");
B("function f(n) { if (n == 0) throw new Error('bottom'); try { f(n - 1) } finally { } } try { f(5000) } catch (e) { R = e.message }");
B("function f(n) { return n == 0 ? new Error('x').stack.split('\\n').length : f(n - 1) } R = S(f(100) > 1)");
B("function f(n) { return n == 0 ? Error.captureStackTrace : f(n - 1) } R = S(typeof f(10))");
B("Error.stackTraceLimit = 3; function f(n) { return n == 0 ? new Error('x').stack.split('\\n').length : f(n - 1) } try { R = S(f(50)) } finally { Error.stackTraceLimit = 100 }");

// ---- 8. `super` em getter/setter e métodos.
B("class A { get x() { return this.v } } class B extends A { get x() { return super.x + 1 } } var b = new B(); b.v = 1; R = S(b.x)");
B("class A { set x(v) { this._x = v } } class B extends A { set x(v) { super.x = v * 2 } } var b = new B(); b.x = 2; R = S(b._x)");
B("class A { get x() { return 1 } } class B extends A { set x(v) { } } R = S(new B().x)");
B("class A { set x(v) { this._x = v } } class B extends A { get x() { return 1 } } var b = new B(); b.x = 2; R = S([b.x, b._x])");
B("class A { get x() { return this } } class B extends A { m() { return super.x === this } } R = S(new B().m())");
B("class A { x = 1 } class B extends A { m() { return super.x } } R = S(new B().m())");
B("class A {} A.prototype.x = 5; class B extends A { m() { return super.x } } R = S(new B().m())");
B("class A {} class B extends A { m() { super.x = 1; return [Object.keys(this), Object.keys(A.prototype)] } } R = S(new B().m())");
B("class A {} class B extends A { m() { super.x = 1; return this.hasOwnProperty('x') } } R = S(new B().m())");
B("class A { set x(v) { R = S(this instanceof B) } } class B extends A { m() { super.x = 1 } } new B().m()");
B("class A {} Object.defineProperty(A.prototype, 'x', {value: 1, writable: false}); class B extends A { m() { super.x = 2 } } try { new B().m(); R = 'silent' } catch (e) { R = e.name + ': ' + e.message }");
B("class A {} Object.defineProperty(A.prototype, 'x', {get() { return 1 }}); class B extends A { m() { super.x = 2 } } try { new B().m(); R = 'silent' } catch (e) { R = e.name + ': ' + e.message }");
B("var o = { __proto__: { get x() { return 'p' } }, get x() { return super.x + 'c' } }; R = S(o.x)");
B("var o = { __proto__: { set x(v) { R = 'p' + v } }, set x(v) { super.x = v + 'c' } }; o.x = 1");
B("var o = { __proto__: { m() { return 'p' } }, m() { return super.m() + 'c' } }; var q = Object.create(o); R = S(q.m())");
B("var o = { m() { return super.toString === Object.prototype.toString } }; R = S(o.m())");
B("var o = { m() { return super.x } }; Object.setPrototypeOf(o, {x: 'late'}); R = S(o.m())");
B("var o = { m() { return super.x } }; var f = o.m; R = S(f.call({}))");
B("var o = { m() { return super['x'] } }; Object.setPrototypeOf(o, {x: 'k'}); R = S(o.m())");
B("var k = 'x'; var o = { m() { return super[k = 'y'] }, }; Object.setPrototypeOf(o, {y: 'ky'}); R = S(o.m())");
B("class A { static s() { return 'A' } } class B extends A { static s() { return super.s() + 'B' } } R = S(B.s())");
B("class A { static get g() { return this.name } } class B extends A { static get g() { return super.g } } R = S(B.g)");
B("class A { m() { return 1 } } class B extends A { m() { return (() => super.m())() } } R = S(new B().m())");
B("class A { m() { return 1 } } class B extends A { m() { return eval('super.m()') } } R = S(new B().m())");
B("class A { m() { return 1 } } class B extends A { m() { return (function () { return eval('super.m()') })() } } try { R = S(new B().m()) } catch (e) { R = e.name }");
B("class A { m() { return 1 } } class B extends A { m() { delete super.m } } try { new B().m() } catch (e) { R = e.name + ': ' + e.message }");
B("class A { m() { return 1 } } class B extends A { m() { return super.m?.() } } R = S(new B().m())");
B("class A { m() { return 1 } } class B extends A { m() { return super.n?.() } } R = S(new B().m())");
B("class A { m() { return 1 } } class B extends A { m() { return super.m`x` } } R = S(new B().m())");
B("class A { get x() { return 1 } } class B extends A { m() { super.x++; return this.x } } try { R = S(new B().m()) } catch (e) { R = e.name + ': ' + e.message }");
B("class A { get x() { return 1 } set x(v) { this._x = v } } class B extends A { m() { super.x++; return this._x } } R = S(new B().m())");
B("class A { get x() { return 1 } set x(v) { this._x = v } } class B extends A { m() { super.x += 5; return this._x } } R = S(new B().m())");
B("class A { get x() { return undefined } set x(v) { this._x = v } } class B extends A { m() { super.x ??= 5; return this._x } } R = S(new B().m())");
B("class A { get x() { return 1 } set x(v) { this._x = v } } class B extends A { m() { super.x ||= 5; return this._x } } R = S(new B().m())");
B("class A {} class B extends A { m() { return super.constructor === A } } R = S(new B().m())");
B("class A {} class B extends A { static m() { return super.constructor === Function } } R = S(B.m())");
B("class B extends null { m() { return super.x } } try { new B().m() } catch (e) { R = e.name + ': ' + e.message }");
B("class B extends null { static m() { return super.name } } R = S(B.m())");
B("var o = { m() { return super.x }, __proto__: null }; try { o.m() } catch (e) { R = e.name + ': ' + e.message }");
B("var o = { get [Symbol.toStringTag]() { return super.constructor.name } }; R = S(String(o))");
B("var s = Symbol('s'); class A { get [s]() { return 'A' } } class B extends A { get [s]() { return super[s] + 'B' } } R = S(new B()[s])");

// ---- 9. `Function.prototype.toString` de métodos computados.
B("var k = 'm'; var o = { [k]() { return 1 } }; R = S(o.m.toString())");
B("var o = { ['a' + 'b'](x) { return x } }; R = S(o.ab.toString())");
B("var o = { [1 + 1](x) { return x } }; R = S(o[2].toString())");
B("var o = { get ['g']() { return 1 } }; R = S(Object.getOwnPropertyDescriptor(o, 'g').get.toString())");
B("var o = { set ['s'](v) { } }; R = S(Object.getOwnPropertyDescriptor(o, 's').set.toString())");
B("var o = { *['g']() { yield 1 } }; R = S(o.g.toString())");
B("var o = { async ['a']() { } }; R = S(o.a.toString())");
B("var o = { async *['a']() { } }; R = S(o.a.toString())");
B("var o = { 'str'() { } }; R = S(o.str.toString())");
B("var o = { 12() { } }; R = S(o[12].toString())");
B("var o = { 1.5() { } }; R = S(o[1.5].toString())");
B("var o = { 0x10() { } }; R = S(o[16].toString())");
B("var o = { 1n() { } }; R = S(o[1].toString())");
B("var s = Symbol('d'); var o = { [s]() { } }; R = S(o[s].toString())");
B("var s = Symbol('d'); var o = { [s]() { } }; R = S(o[s].name)");
B("var s = Symbol(); var o = { [s]() { } }; R = S(o[s].name)");
B("var o = { get a() { return 1 } }; R = S(Object.getOwnPropertyDescriptor(o, 'a').get.name)");
B("var s = Symbol('d'); var o = { get [s]() { return 1 } }; R = S(Object.getOwnPropertyDescriptor(o, s).get.name)");
B("var o = { set a(v) { } }; R = S(Object.getOwnPropertyDescriptor(o, 'a').set.name)");
B("class C { ['m' + 1]() { } } R = S(C.prototype.m1.toString())");
B("class C { static ['m']() { } } R = S(C.m.toString())");
B("class C { static async *['m']() { } } R = S(C.m.toString())");
B("class C { get ['g']() { return 1 } } R = S(Object.getOwnPropertyDescriptor(C.prototype, 'g').get.toString())");
B("class C { static get ['g']() { return 1 } } R = S(Object.getOwnPropertyDescriptor(C, 'g').get.toString())");
B("class C { #p() { } static t(o) { return o.#p.toString() } } R = S(C.t(new C()))");
B("class C { #p() { } static t(o) { return o.#p.name } } R = S(C.t(new C()))");
B("class C { static #p() { } static t() { return C.#p.name } } R = S(C.t())");
B("class C { get #p() { return 1 } static t(o) { return o.#p } } R = S(C.t(new C()))");
B("class C { ['x'] = () => 1 } R = S(new C().x.name)");
B("class C { static ['x'] = function () { } } R = S(C.x.name)");
B("class C { x = class { } } R = S(new C().x.name)");
B("class C { 'a b'() { } } R = S(C.prototype['a b'].name)");
B("class C { constructor() { } } R = S(C.toString())");
B("class C { m() { } } R = S(C.toString())");
B("class /* c */ C /* d */ { /* e */ m /* f */ ( ) /* g */ { } } R = S(C.toString() + '|' + C.prototype.m.toString())");
B("var o = { /* a */ m /* b */ ( /* c */ ) /* d */ { } }; R = S(o.m.toString())");
B("var o = { m() {} , n() {} }; R = S(o.n.toString())");
B("var f = function /* a */ ( /* b */ ) { }; R = S(f.toString())");
B("var f = ( /* a */ ) => /* b */ 1; R = S(f.toString())");
B("var f = async /* a */ ( ) => 1; R = S(f.toString())");
B("var f = async function /* a */ * /* b */ g() { }; R = S(f.toString())");
B("function /* a */ g /* b */ ( ) { } R = S(g.toString())");
B("function* g() { } R = S(g.toString())");
B("async function g() { } R = S(g.toString())");
B("function\ng\n(\n)\n{\n} R = S(g.toString())");
B("var f = new Function('a', 'b', 'return a'); R = S(f.toString())");
B("var f = new Function('a, b', 'return a'); R = S(f.toString())");
B("var f = new Function(); R = S(f.toString())");
B("var f = new Function('/*x*/', 'return 1'); R = S(f.toString())");
B("var f = new (Object.getPrototypeOf(async function () { }).constructor)('a', 'return a'); R = S(f.toString())");
B("var f = new (Object.getPrototypeOf(function* () { }).constructor)('a', 'yield a'); R = S(f.toString())");
B("R = S(function () { }.bind().toString())");
B("R = S((function f() { }).bind().toString())");
B("R = S(Math.max.toString())");
B("R = S(Math.max.bind().toString())");
B("R = S(class { }.bind?.name)");
B("R = S(Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.toString())");
B("R = S(Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.name)");
B("R = S(Symbol.prototype[Symbol.toPrimitive].toString())");
B("R = S(Symbol.prototype[Symbol.toPrimitive].name)");
B("R = S(Array.prototype[Symbol.iterator].name)");
B("R = S(Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.name)");
B("R = S(new Proxy(function () { }, {}).toString())");
B("try { R = S(Function.prototype.toString.call(new Proxy({}, {}))) } catch (e) { R = e.name + ': ' + e.message }");
B("try { R = S(Function.prototype.toString.call({})) } catch (e) { R = e.name + ': ' + e.message }");
B("R = S(Function.prototype.toString.call(Function.prototype))");
B("R = S(Function.prototype.toString.call(Proxy))");
B("var o = { m() { } }; R = S(Function.prototype.toString.call(o.m) === o.m.toString())");
B("var o = { m() { } }; o.m.toString = () => 'x'; R = S(String(o.m))");
B("R = S((function () { return 1 }).toString === Function.prototype.toString)");
B("R = S((() => { }).toString())");
B("R = S((async () => { }).toString())");
B("R = S((x => x * 2).toString())");
B("R = S(((a, b) => a + b).toString())");
B("var o = { async *[Symbol.asyncIterator]() { } }; R = S(o[Symbol.asyncIterator].toString() + '|' + o[Symbol.asyncIterator].name)");
B("var o = { [Symbol.iterator]() { } }; R = S(o[Symbol.iterator].name)");
B("var o = { get [Symbol.toStringTag]() { return 'x' } }; R = S(Object.getOwnPropertyDescriptor(o, Symbol.toStringTag).get.name)");
B("R = S(Function.prototype.toString.call(class { static { } }))");

// ---- 10. `bind` com `new`, `length` e `name`.
B("function f(a, b, c) { } R = S([f.bind().length, f.bind(null, 1).length, f.bind(null, 1, 2, 3, 4).length])");
B("function f(a, b, c) { } R = S(f.bind(null, 1).name + '|' + f.bind().bind().name)");
B("var f = function () { }; R = S(f.bind().name)");
B("var f = () => { }; R = S(f.bind().name)");
B("function f() { } Object.defineProperty(f, 'name', {value: 5}); R = S(f.bind().name)");
B("function f() { } Object.defineProperty(f, 'name', {value: Symbol()}); R = S(f.bind().name)");
B("function f() { } Object.defineProperty(f, 'name', {value: undefined}); R = S(f.bind().name)");
B("function f() { } delete f.name; R = S(f.bind().name)");
B("function f() { } Object.defineProperty(f, 'length', {value: 5}); R = S(f.bind(null, 1).length)");
B("function f() { } Object.defineProperty(f, 'length', {value: -5}); R = S(f.bind().length)");
B("function f() { } Object.defineProperty(f, 'length', {value: 2.7}); R = S(f.bind().length)");
B("function f() { } Object.defineProperty(f, 'length', {value: Infinity}); R = S(f.bind(null, 1).length)");
B("function f() { } Object.defineProperty(f, 'length', {value: -Infinity}); R = S(f.bind().length)");
B("function f() { } Object.defineProperty(f, 'length', {value: '3'}); R = S(f.bind().length)");
B("function f() { } Object.defineProperty(f, 'length', {value: NaN}); R = S(f.bind().length)");
B("function f() { } delete f.length; R = S(f.bind().length)");
B("function f() { } Object.defineProperty(f, 'length', {value: 2 ** 40}); R = S(f.bind(null, 1).length)");
B("function f() { } Object.defineProperty(f, 'length', {get() { throw new Error('len') }, configurable: true}); try { f.bind() } catch (e) { R = e.message }");
B("function f() { } Object.defineProperty(f, 'name', {get() { throw new Error('nm') }, configurable: true}); try { f.bind() } catch (e) { R = e.message }");
B("function f() { } R = S(Object.getOwnPropertyDescriptor(f.bind(), 'name'))");
B("function f() { } R = S(Object.getOwnPropertyDescriptor(f.bind(), 'length'))");
B("function f() { } R = S(f.bind().hasOwnProperty('prototype'))");
B("function f() { } R = S(Object.getOwnPropertyNames(f.bind()).join())");
B("function f() { } R = S(Object.getPrototypeOf(f.bind()) === Function.prototype)");
B("function f() { } Object.setPrototypeOf(f, Array.prototype); R = S(Object.getPrototypeOf(f.bind()) === Array.prototype)");
B("function f() { } Object.setPrototypeOf(f, null); R = S(Object.getPrototypeOf(Function.prototype.bind.call(f)))");
B("function f() { } f.extra = 1; R = S(f.bind().extra)");
B("function F(a, b) { this.a = a; this.b = b } var B = F.bind({ignored: 1}, 1); var o = new B(2); R = S([o.a, o.b, o instanceof F, o instanceof B])");
B("function F() { this.t = this } var B = F.bind({x: 1}); var o = new B(); R = S([o.t === o, o.x])");
B("function F() { return new.target } var B = F.bind(); R = S([new B() === F, Reflect.construct(B, [], Array) === Array])");
B("function F() { return new.target } var B = F.bind(); R = S(new B() === F)");
B("function F() { } var B = F.bind(); R = S(new B().constructor === F)");
B("function F() { } F.prototype.p = 1; var B = F.bind(); R = S(new B().p)");
B("function F() { } var B = F.bind(); B.prototype = {p: 2}; R = S(new B().p)");
B("function F() { } var B = F.bind(); R = S(B.prototype)");
B("var B = (() => { }).bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = ({ m() { } }).m.bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = (async function () { }).bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = (function* () { }).bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = Math.max.bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = Date.bind(null, 2020); R = S([new B().getFullYear(), typeof B(), new B() instanceof Date])");
B("var B = Array.bind(null, 3); R = S(new B().length)");
B("var B = Array.bind(null, 1, 2); R = S(new B())");
B("var B = Object.bind(null, 1); R = S(typeof new B())");
B("var B = Number.bind(null, '5'); R = S([new B() + 1, typeof new B(), B()])");
B("var B = Map.bind(null, [[1, 2]]); R = S(new B().get(1))");
B("var B = Promise.bind(null, r => r(1)); R = S(new B() instanceof Promise)");
B("var B = Proxy.bind(null, {}); R = S(typeof new B({}))");
B("var B = Symbol.bind(); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("var B = BigInt.bind(null, 5); try { new B() } catch (e) { R = e.name + ': ' + e.message }");
B("class A { constructor(x) { this.x = x } } var B = A.bind(null, 7); R = S([new B().x, new B() instanceof A])");
B("class A { constructor() { this.t = new.target } } var B = A.bind(); R = S(new B().t === A)");
B("class A { } var B = A.bind(); try { B() } catch (e) { R = e.name + ': ' + e.message }");
B("class A extends Array { } var B = A.bind(); R = S(new B() instanceof A)");
B("class A { static s = 1 } var B = A.bind(); R = S(B.s)");
B("class A { } R = S(A.bind().name)");
B("class A { } R = S(Object.getOwnPropertyNames(A.bind()).join())");
B("function f(a, b) { return [this, a, b] } var g = f.bind('t', 1); R = S([typeof g.call('x', 2)[0], g(2)[1], g.apply(null, [5])[2]])");
B("function f() { 'use strict'; return this } var g = f.bind('t'); R = S([g(), g.call('x'), g.bind('y')()])");
B("function f() { return this } var g = f.bind(1); R = S(typeof g())");
B("function f() { return this } var g = f.bind(null); R = S(g() === globalThis)");
B("function f() { return arguments.length } var g = f.bind(null, 1, 2).bind(null, 3); R = S(g(4))");
B("function f() { return [].slice.call(arguments) } var g = f.bind(null, ...[1, 2]); R = S(g(...[3]))");
B("function f() { return arguments.length } var g = f.bind(null, ...Array(70000)); R = S(g(1))");
B("function f() { return arguments.length } R = S(f.bind.apply(f, Array(100000)))");
B("function f() { return arguments.length } var g = f.bind(null, ...Array(65000)); R = S(g(...Array(65000)))");
B("var g = (function () { }).bind(); R = S(g instanceof Function && Object.prototype.toString.call(g))");
B("function F() { } var B = F.bind(); R = S([new F() instanceof B, {} instanceof B, F.prototype.isPrototypeOf(new B())])");
B("function F() { } var B = F.bind(); B.prototype = 1; R = S(typeof new B())");
B("var B = (function F() { }).bind(); R = S(B.name + '|' + B.bind().name + '|' + B.bind().bind().name)");
B("var o = {m() { return this }}; var g = o.m.bind(o); R = S(g.call({}) === o)");
B("var f = function () { return this }; var g = f.bind(1).bind(2); R = S(g() instanceof Number && g().valueOf())");
B("R = S(Function.prototype.bind.call(1))");
B("try { Function.prototype.bind.call({}) } catch (e) { R = e.name + ': ' + e.message }");
B("try { Function.prototype.bind.call(undefined) } catch (e) { R = e.name + ': ' + e.message }");
B("try { Function.prototype.call.call({}) } catch (e) { R = e.name + ': ' + e.message }");
B("try { Function.prototype.apply.call({}) } catch (e) { R = e.name + ': ' + e.message }");
B("try { Function.prototype.apply.call(function () { }, null, 1) } catch (e) { R = e.name + ': ' + e.message }");
B("R = S(Function.prototype.length + ',' + Function.prototype.name + ',' + Function.prototype())");
B("R = S([Function.prototype.call.length, Function.prototype.apply.length, Function.prototype.bind.length])");
B("R = S([Function.prototype.call.name, Function.prototype.apply.name, Function.prototype.bind.name])");
B("R = S(new Proxy(function f(a) { }, {}).bind().length)");
B("R = S(new Proxy(function f(a, b) { }, {get(t, k) { return k == 'length' ? 9 : t[k] }}).bind().length)");
B("R = S(new Proxy(class { }, {}).bind().name)");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "call-edge-golden-"));
// O bun passa arquivos pelo transpilador próprio; `vm.runInThisContext` roda como ProgramExecutable do JSC puro, então
// o programa vai por ele. SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>"). Programas que
// gravam `R` depois de microtarefas (async) esperam o esvaziamento antes de ler `R`.
const source_file = path.join(dir, "call_source.js");
const file = path.join(dir, "call_case.js");
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
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000 });
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
