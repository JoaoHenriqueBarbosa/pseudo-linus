// Gera tests/golden/limits_range_bun.tsv: limites e RangeError/TypeError de tamanho e de pilha, medidos no bun 1.4.2,
// capturando só `e.name` e `e.message` (nunca stack, nunca tempo). Cobre recursão profunda por todas as formas de
// chamada (função, método, getter, construtor, trap de Proxy, toString, JSON), JSON aninhado, spread e apply com
// arrays enormes, repeat/padStart/padEnd, tamanhos de Array, ArrayBuffer, TypedArray, DataView, toFixed/toPrecision/
// toExponential/toString(radix), limites de BigInt, regex grande e catastrófico curto, e tail call.
// Complementa limits_bun.tsv (gerado por gen-limits-golden.js): aqui o foco é o texto da mensagem de cada limite.
// Cada programa grava `R`; descartados: mais de 2 s, resultado diferente entre duas execuções, caminho da máquina.
// Uso: bun scripts/gen-limits-range-golden.js > tests/golden/limits_range_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Corpo que atribui R; exceção vira `Nome: mensagem`.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Instrução que roda sem atribuir R; `ok` quando não lança.
const X = stmt => T(`${stmt}; R = 'ok'`);
// Expressão cujo String() vira R.
const V = expr => T(`R = String(${expr})`);
// Recursão sem fim: o resultado esperado é o RangeError de pilha; o motor segue vivo depois.
const S = body => T(`${body}; R = 'ok'`);

// ---- Recursão infinita por forma de chamada.
const recursions = {
  "função": "function f() { return f() + 1 } f()",
  "função sem retorno": "function f() { f() } f()",
  "mútua": "function a() { return b() + 1 } function b() { return a() + 1 } a()",
  "arrow": "var f = () => f() + 1; f()",
  "expressão nomeada": "(function g() { return g() + 1 })()",
  "método de objeto": "var o = { m() { return this.m() + 1 } }; o.m()",
  "método de classe": "class C { m() { return this.m() + 1 } } new C().m()",
  "método estático": "class C { static m() { return C.m() + 1 } } C.m()",
  "getter": "var o = { get g() { return this.g + 1 } }; o.g",
  "setter": "var o = { set s(v) { this.s = v } }; o.s = 1",
  "getter de classe": "class C { get g() { return this.g } } new C().g",
  "construtor": "function C() { new C() } new C()",
  "construtor de classe": "class C { constructor() { new C() } } new C()",
  "construtor derivado": "class A { constructor() { new B() } } class B extends A {} new B()",
  "super method": "class A { m() { return this.m() } } class B extends A { m() { return super.m() } } new B().m()",
  "campo de classe": "class C { x = new C() } new C()",
  "static block": "class C { static { new C() } }",
  "call": "function f() { return f.call(null) } f()",
  "apply": "function f() { return f.apply(null, []) } f()",
  "bind": "function f() { return g() } var g = f.bind(null); f()",
  "Reflect.apply": "function f() { return Reflect.apply(f, null, []) } f()",
  "Reflect.construct": "function F() { return Reflect.construct(F, []) } new F()",
  "toString": "var o = { toString() { return '' + this } }; '' + o",
  "valueOf": "var o = { valueOf() { return +this } }; +o",
  "Symbol.toPrimitive": "var o = { [Symbol.toPrimitive]() { return `${this}` } }; `${o}`",
  "toJSON": "var o = { toJSON() { return JSON.stringify(this) } }; JSON.stringify(o)",
  "replacer de JSON": "function r(k, v) { return JSON.stringify({ a: 1 }, r) } JSON.stringify({ a: 1 }, r)",
  "reviver de JSON": "function r(k, v) { return JSON.parse('[1]', r) } JSON.parse('[1]', r)",
  "Symbol.hasInstance": "var o = { [Symbol.hasInstance](v) { return v instanceof o } }; 1 instanceof o",
  "Symbol.iterator": "var o = { [Symbol.iterator]() { return [...o][Symbol.iterator]() } }; [...o]",
  "map callback": "function f() { return [1].map(f) } f()",
  "forEach callback": "function f() { [1].forEach(f) } f()",
  "sort comparator": "function f() { return [2, 1].sort(f) } f()",
  "replace callback": "function f() { return 'a'.replace(/a/, f) } f()",
  "Array.from": "function f() { return Array.from([1], f) } f()",
  "Promise executor": "function f() { new Promise(f) } f()",
  "generator yield*": "function* g() { yield* g() } [...g()]",
  "generator next": "function* g() { g().next(); yield 1 } g().next()",
  "async síncrono": "async function f() { await f() } f(); R2 = 1",
  "eval": "function f() { eval('f()') } f()",
  "Function": "var f = new Function('return f() + 1'); globalThis.f = f; f()",
  "template tag": "function t() { return t`x` } t`x`",
  "destructuring default": "function f({ a = f() } = {}) { return a } f()",
  "parâmetro default": "function f(a = f()) { return a } f()",
  "getter no protótipo": "var p = { get x() { return o.x } }; var o = Object.create(p); o.x",
  "valueOf em comparação": "var o = { valueOf() { return o < 1 } }; o < 1",
  "toString em chave": "var o = { toString() { return ({})[this] } }; ({})[o]",
  "instanceof de bound": "function F() {} var B = F.bind(); B.prototype; 1 instanceof B; function f() { return f() } f()",
  "exceção recursiva": "function f() { try { f() } finally { } } f()",
  "with": "var o = { f() { with (o) { f() } } }; o.f()",
};
for (const body of Object.values(recursions)) S(body);
// O motor continua utilizável depois do estouro, e a mensagem é a mesma capturada em catch interno.
T("function f() { f() } try { f() } catch (e) { R = e.name + '|' + (e instanceof RangeError) + '|' + e.message }");
T("function f() { f() } try { f() } catch (e) { try { f() } catch (e2) { R = e2.message === e.message } }");
T("function f() { f() } try { f() } catch (e) { R = Object.prototype.toString.call(e) + typeof e.stack }");
T("function f() { f() } try { f() } catch (e) { R = [e.constructor === RangeError, e.hasOwnProperty('message'), 'cause' in e].join() }");
T("function f() { f() } try { f() } catch (e) { R = String(e) }");
T("function f() { f() } try { f() } catch (e) { R = Object.keys(e).join() + '|' + Object.getOwnPropertyNames(e).includes('stack') }");
T("function f() { try { f() } catch (e) { return e.message } } R = f()");
T("var n = 0; function f() { n++; f() } try { f() } catch (e) { R = n > 1000 ? 'fundo' : 'raso' }");
T("var n = 0; function f() { n++; f() } try { f() } catch (e) { var m = n; n = 0; try { f() } catch (e2) { R = Math.abs(n - m) < m / 2 } }");
T("function f() { f() } for (var i = 0; i < 5; i++) { try { f() } catch (e) { R = i + e.message } }");
T("function f() { f() } try { f() } catch (e) { R = (function () { return [1, 2, 3].map(x => x * 2).join() })() }");
T("function f() { f() } try { f() } catch (e) { R = JSON.stringify({ a: [1, 2] }) }");
T("function f() { f() } try { f() } finally { R = 'finally' }");
T("function f() { try { f() } finally { R = 'fin' } } try { f() } catch (e) { R += e.name }");
T("var r = []; function f(n) { r.push(n); f(n + 1) } try { f(0) } catch (e) { R = r.length > 100 && r[0] === 0 && r[1] === 1 }");
// Profundidades que cabem.
for (const depth of [10, 100, 1000, 3000, 5000]) {
  T(`function f(n) { return n === 0 ? 0 : 1 + f(n - 1) } R = f(${depth})`);
}
for (const depth of [10, 100, 1000]) {
  T(`class C { m(n) { return n === 0 ? 0 : 1 + this.m(n - 1) } } R = new C().m(${depth})`);
  T(`var o = { get g() { return this.d-- === 0 ? 0 : 1 + this.g }, d: ${depth} }; R = o.g`);
  T(`function C(n) { this.c = n === 0 ? null : new C(n - 1) } var d = 0, c = new C(${depth}); while (c.c) { c = c.c; d++ } R = d`);
  T(`var p = new Proxy({}, { get(t, k, r) { return t.d-- === 0 ? 0 : 1 + r[k] } }); p.d = ${depth}; R = 1`);
}
// Proxy: cada trap recursivo.
const traps = {
  get: "var p = new Proxy({}, { get(t, k, r) { return r.x } }); p.x",
  set: "var p = new Proxy({}, { set(t, k, v, r) { r.x = v; return true } }); p.x = 1",
  has: "var p = new Proxy({}, { has(t, k) { return k in p } }); 'x' in p",
  deleteProperty: "var p = new Proxy({}, { deleteProperty(t, k) { return delete p[k] } }); delete p.x",
  ownKeys: "var p = new Proxy({}, { ownKeys(t) { return Reflect.ownKeys(p) } }); Object.keys(p)",
  getOwnPropertyDescriptor: "var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return Object.getOwnPropertyDescriptor(p, k) } }); Object.getOwnPropertyDescriptor(p, 'x')",
  defineProperty: "var p = new Proxy({}, { defineProperty(t, k, d) { return Reflect.defineProperty(p, k, d) } }); Object.defineProperty(p, 'x', { value: 1 })",
  getPrototypeOf: "var p = new Proxy({}, { getPrototypeOf(t) { return Object.getPrototypeOf(p) } }); Object.getPrototypeOf(p)",
  setPrototypeOf: "var p = new Proxy({}, { setPrototypeOf(t, v) { return Reflect.setPrototypeOf(p, v) } }); Object.setPrototypeOf(p, null)",
  isExtensible: "var p = new Proxy({}, { isExtensible(t) { return Object.isExtensible(p) } }); Object.isExtensible(p)",
  preventExtensions: "var p = new Proxy({}, { preventExtensions(t) { return Reflect.preventExtensions(p) } }); Object.preventExtensions(p)",
  apply: "var p = new Proxy(function () {}, { apply(t, th, a) { return p() } }); p()",
  construct: "var p = new Proxy(function () {}, { construct(t, a) { return new p() } }); new p()",
};
for (const body of Object.values(traps)) S(body);
// Cadeia de Proxy (proxy de proxy) e de protótipos.
T("var p = {}; for (var i = 0; i < 100000; i++) p = new Proxy(p, {}); try { p.x; R = 'ok' } catch (e) { R = e.name + ': ' + e.message }");
T("var p = {}; for (var i = 0; i < 100000; i++) p = new Proxy(p, {}); try { R = typeof p } catch (e) { R = e.name + ': ' + e.message }");
T("var p = function () {}; for (var i = 0; i < 100000; i++) p = new Proxy(p, {}); p(); R = 'ok'");
T("var p = {}; for (var i = 0; i < 50000; i++) p = new Proxy(p, {}); R = Object.keys(p).length");
T("var o = {}; for (var i = 0; i < 100000; i++) o = Object.create(o); R = o.x === undefined");
T("var o = {}; for (var i = 0; i < 100000; i++) o = Object.create(o); R = 'x' in o");
T("var o = {}; for (var i = 0; i < 100000; i++) o = Object.create(o); R = o instanceof Object");
T("var a = []; a[0] = a; R = String(a)");
T("var a = []; a[0] = a; R = a.join()");
T("var a = [1]; a.push(a); R = a.toString()");
T("var a = []; a[0] = a; R = a.toLocaleString()");
T("var o = {}; o.o = o; JSON.stringify(o); R = 'x'");
T("var a = []; a[0] = a; JSON.stringify(a); R = 'x'");
T("var o = { a: {} }; o.a.b = o; JSON.stringify(o); R = 'x'");
T("var m = new Map(); m.set(m, m); JSON.stringify([...m]); R = 'x'");
T("var o = {}; Object.defineProperty(o, 'x', { get() { return JSON.stringify(o) }, enumerable: true }); JSON.stringify(o); R = 'x'");

// ---- JSON aninhado.
for (const depth of [100, 1000, 5000, 10000, 100000, 1000000]) {
  T(`var s = '['.repeat(${depth}) + ']'.repeat(${depth}); var v = JSON.parse(s); R = Array.isArray(v)`);
  T(`var s = '{"a":'.repeat(${depth}) + '1' + '}'.repeat(${depth}); var v = JSON.parse(s); R = typeof v`);
  T(`var o = []; for (var i = 0; i < ${depth}; i++) o = [o]; R = JSON.stringify(o).length`);
  T(`var o = {}; for (var i = 0; i < ${depth}; i++) o = { a: o }; R = JSON.stringify(o).length`);
}
T("var s = '['.repeat(100000); JSON.parse(s)");
T("var s = '['.repeat(100000) + ']'.repeat(99999); JSON.parse(s)");
T("var s = '['.repeat(100001) + ']'.repeat(100000); R = JSON.parse(s).length");
T("var s = '['.repeat(100000) + ']'.repeat(100000); R = JSON.parse(s, function (k, v) { return v }).length");
T("var s = '['.repeat(100000) + ']'.repeat(100000); R = JSON.stringify(JSON.parse(s)).length");
T("var o = {}; for (var i = 0; i < 100000; i++) o = { a: o }; R = JSON.stringify(o, null, 2).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = JSON.stringify(o, null, 1).length");
T("var o = {}; for (var i = 0; i < 100000; i++) o = { a: o }; R = JSON.stringify(o, ['a']).length");
T("var o = {}; for (var i = 0; i < 100000; i++) o = { a: o }; R = JSON.stringify(o, (k, v) => v).length");
T("var o = {}; for (var i = 0; i < 10000; i++) o = { a: o }; R = JSON.stringify(o, (k, v) => v).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = String(o).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = o.flat(Infinity).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = o.flat(1e6).length");
T("var o = []; for (var i = 0; i < 1000; i++) o = [o]; R = o.flat(Infinity).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = o.toString().length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = o.join('-').length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = o.toLocaleString().length");
T("var o = {}; for (var i = 0; i < 100000; i++) o = { a: o }; R = Object.keys(o).length");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = typeof structuredCloneNotUsed");
T("var s = 'a'; for (var i = 0; i < 100000; i++) s = '(' + s + ')'; R = s.length");
T("var o = { a: 1 }; for (var i = 0; i < 100000; i++) o = { __proto__: o }; R = JSON.stringify(o)");
T("var o = []; for (var i = 0; i < 100000; i++) o = [o]; R = Array.isArray(o)");

// ---- Spread, apply e argumentos enormes.
for (const n of [1000, 10000, 65535, 65536, 100000, 200000, 500000, 1000000, 2000000, 5000000]) {
  T(`function f() { return arguments.length } R = f(...new Array(${n}))`);
  T(`function f() { return arguments.length } R = f.apply(null, new Array(${n}))`);
  T(`R = Math.max(...new Array(${n}).fill(1))`);
  T(`R = Math.max.apply(null, new Array(${n}).fill(1))`);
  T(`R = String.fromCharCode.apply(null, new Array(${n}).fill(65)).length`);
}
for (const n of [100000, 1000000]) {
  T(`var a = []; a.push(...new Array(${n}).fill(1)); R = a.length`);
  T(`R = [...new Array(${n}).fill(1)].length`);
  T(`R = new Array(${n}).fill(1).concat(new Array(${n}).fill(2)).length`);
  T(`function C() { this.n = arguments.length } R = new C(...new Array(${n})).n`);
  T(`R = Reflect.apply(function () { return arguments.length }, null, new Array(${n}))`);
  T(`R = Reflect.construct(function () { this.n = arguments.length }, new Array(${n})).n`);
  T(`R = Array.of(...new Array(${n})).length`);
  T(`R = ((...r) => r.length)(...new Array(${n}))`);
  T(`R = new Array(...new Array(${n})).length`);
  T(`R = Function.prototype.call.apply(function () { return arguments.length }, new Array(${n}))`);
  T(`var a = [1]; a.unshift(...new Array(${n})); R = a.length`);
  T(`var a = [1]; a.splice(0, 0, ...new Array(${n})); R = a.length`);
}
for (const len of ["2**32", "2**32 - 1", "2**31", "2**53", "-1", "1e9", "NaN", "Infinity", "'3'", "1.9", "2**31 - 1", "4294967296"]) {
  T(`function f() { return arguments.length } R = f.apply(null, { length: ${len} })`);
  T(`R = Reflect.apply(function () { return arguments.length }, null, { length: ${len} })`);
}
T("R = Function.prototype.apply.call(1)");
T("R = Function.prototype.apply.call(function () {}, null, 1)");
T("R = Function.prototype.apply.call(function () {}, null, 'abc')");
T("R = (function () { return arguments.length }).apply(null, null)");
T("R = (function () { return arguments.length }).apply(null, undefined)");
T("R = Reflect.apply(function () {}, null)");
T("R = Reflect.apply(function () {}, null, null)");
T("R = Reflect.construct(function () {}, [], 1)");
T("R = Reflect.construct(() => {}, [])");
T("R = Math.max(...{ length: 3 })");
T("R = Math.max(...1)");
T("R = Math.max(...null)");
T("R = Math.max(...undefined)");
T("R = [...{}]");
T("R = [...1]");
T("R = new Array(...[2 ** 32])");
T("R = Array.apply(null, { length: 2 ** 32 })");
T("R = Array.apply(null, { length: -1 })");

// ---- String.prototype.repeat/padStart/padEnd e comprimento máximo de string.
for (const count of ["-1", "-Infinity", "Infinity", "NaN", "'a'", "2**31", "2**32", "2**53", "2**31 - 1 + 1", "1e10", "-0", "0.9", "-0.5", "null", "undefined", "{}", "[]", "[2]", "1n"]) {
  T(`R = 'ab'.repeat(${count}).length`);
}
T("R = ''.repeat(2 ** 31).length");
T("R = ''.repeat(Infinity)");
T("R = ''.repeat(-1)");
T("R = ''.repeat(2 ** 53).length");
T("R = 'a'.repeat(2 ** 30 + 1).length > 0");
T("R = 'ab'.repeat(2 ** 30).length");
T("R = 'abcd'.repeat(2 ** 29).length");
T("R = 'a'.repeat(2 ** 31 - 1 + 1)");
T("R = String.prototype.repeat.call(null, 1)");
T("R = String.prototype.repeat.call(undefined, 1)");
T("R = String.prototype.repeat.call(Symbol(), 1)");
T("R = Symbol().toString().repeat(Infinity)");
T("R = ''.repeat({ valueOf() { return Infinity } })");
T("R = 'x'.repeat({ valueOf() { return 3 } })");
for (const n of ["2**31", "2**32", "2**53", "2**31 + 5", "1e10", "Infinity", "-1", "NaN", "'3'", "1.9", "-Infinity", "2**30 * 2"]) {
  T(`R = 'a'.padStart(${n}).length`);
  T(`R = 'a'.padEnd(${n}, 'xy').length`);
}
T("R = 'a'.padStart(2 ** 31, '').length");
T("R = 'a'.padEnd(2 ** 31, '').length");
T("R = 'a'.padStart(2 ** 31, 'x'.repeat(10)).length");
T("R = 'a'.padEnd(2 ** 53 - 1, 'x').length");
T("R = ''.padStart(2 ** 31 - 1 + 1, 'x').length");
T("R = 'abc'.padStart(10, undefined)");
T("R = 'abc'.padEnd(10, null)");
T("R = 'abc'.padStart(Infinity, '')");
T("R = 'abc'.padEnd(Infinity, '')");
T("R = 'abc'.padStart(5, { toString() { return '12' } })");
T("R = String.prototype.padStart.call(null, 5)");
T("R = String.prototype.padEnd.call(undefined, 5)");
// Crescimento por concatenação, join, replace, split e outros construtores de string.
T("var s = 'a'.repeat(2 ** 20); for (var i = 0; i < 20; i++) s += s; R = s.length");
T("var s = 'a'.repeat(2 ** 29); s += s; R = s.length > 0");
T("var s = 'a'.repeat(2 ** 29); s = s + s + s + s; R = s.length");
T("var s = 'ab'.repeat(2 ** 29); s = s + s; R = s.length");
T("var s = 'a'.repeat(2 ** 30); R = s.length");
T("var s = 'a'.repeat(2 ** 30); R = (s + 'a').length");
T("var s = 'a'.repeat(2 ** 30); R = (s + s).length");
T("var a = new Array(2 ** 20).fill('a'.repeat(2 ** 11)); R = a.join('').length");
T("var a = new Array(2 ** 20 + 1).fill('a'.repeat(2 ** 11)); R = a.join('').length");
T("var a = new Array(2 ** 16).fill('a'.repeat(2 ** 15)); R = a.join('b').length");
T("R = new Array(2 ** 31).join('ab').length");
T("R = new Array(2 ** 30).join('abc').length");
T("R = new Array(2 ** 30 + 2).join('ab').length");
T("R = new Array(2 ** 32 - 1).join('abcdef').length");
T("R = new Array(2 ** 31 + 2).join('').length");
T("R = Array(2 ** 30).join('a').length > 0");
T("R = 'a'.replace(/a/, 'b'.repeat(2 ** 29)).length");
T("var s = 'a'.repeat(2 ** 20); R = s.replace(/a/g, 'b'.repeat(2 ** 12)).length");
T("var s = 'a'.repeat(2 ** 21); R = s.replaceAll('a', 'b'.repeat(2 ** 10)).length");
T("var s = 'a'.repeat(2 ** 21); R = s.replaceAll('a', 'b'.repeat(2 ** 11)).length");
T("R = 'a'.repeat(2 ** 16).split('').length");
T("R = String.fromCharCode.apply(null, new Array(2 ** 16).fill(97)).length");
T("R = 'x'.concat(...new Array(2 ** 16).fill('a'.repeat(2 ** 15))).length");
T("R = 'x'.concat(...new Array(2 ** 17).fill('a'.repeat(2 ** 15))).length");
T("R = 'a'.localeCompare('b'.repeat(2 ** 20))");
T("var s = 'a'.repeat(2 ** 30); R = s.toUpperCase().length");
T("var s = 'ß'.repeat(2 ** 30); R = s.toUpperCase().length");
T("var s = 'ß'.repeat(2 ** 29); R = s.toUpperCase().length");
T("var s = '\\u0130'.repeat(2 ** 30); R = s.toLowerCase().length");
T("R = ('a'.repeat(2 ** 20) + 'b'.repeat(2 ** 20)).normalize('NFD').length");
T("var s = 'a'.repeat(2 ** 30); R = s.padEnd(2 ** 31 - 1).length");
T("var s = 'a'.repeat(2 ** 30); R = s.padEnd(2 ** 30 + 5).length");
T("R = `${'a'.repeat(2 ** 30)}${'b'.repeat(2 ** 30)}`.length");
T("R = ['a'.repeat(2 ** 30), 'b'.repeat(2 ** 30)].join('').length");
T("R = 'a'.repeat(2 ** 30).concat('a'.repeat(2 ** 30)).length");

// ---- Tamanhos de Array.
for (const len of ["2**32", "2**32 + 1", "2**53", "-1", "-2", "1.5", "NaN", "Infinity", "-Infinity", "'x'", "'-1'", "2**32 - 1", "2**32 - 2", "4294967295", "4294967296", "1e10", "-0", "1e21", "0.1", "1n"]) {
  T(`R = new Array(${len}).length`);
  T(`R = Array(${len}).length`);
  T(`var a = []; a.length = ${len}; R = a.length`);
}
for (const len of ["2**32", "-1", "1.5", "NaN", "'x'", "null", "undefined", "{}", "[]", "[5]", "true", "2**32 - 1", "Infinity", "'4294967296'", "'4294967295'", "{ valueOf() { return -1 } }", "{ valueOf() { return 2 ** 32 } }", "1n"]) {
  T(`var a = [1, 2, 3]; a.length = ${len}; R = a.length`);
}
T("var a = []; a.length = 2 ** 32 - 1; a.push(1); R = a.length");
T("var a = []; a.length = 2 ** 32 - 1; R = a.push()");
T("var a = []; a.length = 2 ** 32 - 1; a.push(1, 2); R = a.length");
T("var a = []; a.length = 2 ** 32 - 1; R = a.unshift(1)");
T("var a = []; a.length = 2 ** 32 - 1; R = a.concat([1]).length");
T("var a = []; a.length = 2 ** 32 - 1; R = a.pop()");
T("var a = []; a.length = 2 ** 32 - 1; R = a.indexOf(1)");
T("var a = []; a.length = 2 ** 32 - 1; R = a.lastIndexOf(1)");
T("var a = []; a.length = 2 ** 32 - 1; a[2 ** 32 - 1] = 1; R = a.length + ',' + Object.keys(a)");
T("var a = []; a[2 ** 32 - 2] = 1; R = a.length");
T("var a = []; a[2 ** 32 - 1] = 1; R = a.length + ',' + Object.keys(a)");
T("var a = []; a[2 ** 32] = 1; R = a.length + ',' + Object.keys(a)");
T("var a = []; a['4294967295'] = 1; R = a.length");
T("var a = []; a['4294967294'] = 1; R = a.length");
T("var a = []; a[-1] = 1; R = a.length + ',' + Object.keys(a)");
T("var a = []; a[1.5] = 1; R = a.length + ',' + Object.keys(a)");
T("var a = []; Object.defineProperty(a, 'length', { value: 2 ** 32 }); R = a.length");
T("var a = []; Object.defineProperty(a, 'length', { value: -1 }); R = a.length");
T("var a = []; Object.defineProperty(a, 'length', { value: 1.5 }); R = a.length");
T("var a = []; Object.defineProperty(a, 'length', { value: 'x' }); R = a.length");
T("var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a.push(4); R = a.length");
T("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a.length = 1; R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a.push(4); R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a.pop(); R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a.length = 0; R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a[3] = 1; R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a.shift(); R = a.length");
T("'use strict'; var a = Object.freeze([1, 2, 3]); a.sort(); R = a.length");
T("var a = Object.seal([1, 2, 3]); a.pop(); R = a.length");
T("var a = Object.preventExtensions([1, 2, 3]); a.push(4); R = a.length");
T("R = Array.from({ length: -1 }).length");
T("R = Array.from({ length: 2 ** 32 + 1 }, function () { throw new Error('loop') })");
T("R = Array.from({ length: 2 ** 53 }, function () { throw new Error('loop') })");
T("R = Array.prototype.slice.call({ length: 2 ** 53 }, 2 ** 53 - 2).length");
T("R = Array.prototype.push.call({ length: 2 ** 53 - 1 }, 1)");
T("R = Array.prototype.push.call({ length: 2 ** 53 - 1 })");
T("R = Array.prototype.push.call({ length: 2 ** 53 }, 1)");
T("R = Array.prototype.unshift.call({ length: 2 ** 53 - 1 }, 1)");
T("R = Array.prototype.splice.call({ length: 2 ** 53 - 1 }, 0, 0, 1)");
T("R = Array.prototype.concat.call({ length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true }, [1])");
T("R = Array.prototype.pop.call({ length: 2 ** 53 + 10 })");
T("R = Array.prototype.at.call({ length: 2 ** 53 + 10 }, -1)");
T("R = Array.prototype.fill.call({ length: 3 }, 1).length");
T("R = Array.prototype.lastIndexOf.call({ length: 2 ** 53 + 10 }, 1)");
T("R = Array.prototype.includes.call({ length: 2 ** 53 + 10, 0: 5 }, 5)");
T("R = Array.prototype.indexOf.call({ length: 2 ** 53 + 10, 0: 5 }, 5)");
T("R = Array.prototype.reverse.call({ length: 0 }).length");
T("R = new Array(2 ** 32 - 1).toString().length");
T("R = new Array(2 ** 20).toString().length");
T("R = new Array(2 ** 20).fill().length");
T("R = [].concat(new Array(2 ** 32 - 1), new Array(1)).length");
T("R = [].concat(new Array(2 ** 32 - 1), [1]).length");
T("R = new Array(2 ** 32 - 1).concat(new Array(2 ** 32 - 1)).length");
T("R = new Array(2 ** 31).concat(new Array(2 ** 31)).length");
T("R = new Array(2 ** 31).concat(new Array(2 ** 31 - 1)).length");
T("var a = new Array(2 ** 32 - 1); R = a.slice(2 ** 32 - 3).length");
T("var a = new Array(2 ** 32 - 1); R = a.splice(2 ** 32 - 3, 5).length");
T("var a = new Array(2 ** 32 - 1); a.splice(0, 0, 1); R = a.length");
T("var a = new Array(2 ** 32 - 1); R = a.flat().length");
T("var a = new Array(2 ** 32 - 1); R = a.at(-1)");
T("var a = new Array(2 ** 32 - 1); R = a.with(0, 1).length");
T("var a = new Array(2 ** 32 - 1); R = a.toSpliced(0, 0, 1).length");
T("var a = new Array(2 ** 32 - 1); R = a.toReversed().length");
T("var a = new Array(2 ** 32 - 1); R = a.toSorted().length");
T("R = [].with(0, 1)");
T("R = [1].with(1, 1)");
T("R = [1].with(-2, 1)");
T("R = [1].with(2 ** 32, 1)");
T("R = [1, 2].toSpliced(0, 0, ...new Array(2 ** 20)).length");
T("R = Array.prototype.toSpliced.call({ length: 2 ** 32 }, 0, 0)");
T("R = Array.prototype.toSorted.call({ length: 2 ** 32 })");
T("R = Array.prototype.toReversed.call({ length: 2 ** 32 })");
T("R = Array.prototype.with.call({ length: 2 ** 32 }, 0, 1)");
T("R = Array.prototype.toSpliced.call({ length: 2 ** 53 - 1 }, 0, 0, 1)");
T("R = Array.prototype.flat.call({ length: 2 ** 32 })");
T("R = Array.prototype.flatMap.call({ length: 2 ** 32 }, x => x)");
T("R = Array.prototype.copyWithin.call({ length: 2 ** 53 + 5 }, 0, 1, 1).length");
T("R = Array.prototype.map.call({ length: 2 ** 32 }, x => x)");
T("R = Array.prototype.filter.call({ length: 2 ** 32 }, x => x)");
T("R = Array.prototype.slice.call({ length: 2 ** 32 }, 0, 2 ** 32)");
T("R = Array.prototype.splice.call({ length: 2 ** 32 }, 0, 2 ** 32)");

// ---- ArrayBuffer, SharedArrayBuffer, DataView e TypedArray.
for (const size of ["2**53", "2**53 - 1", "2**53 + 2", "2**64", "-1", "-0", "1.5", "NaN", "Infinity", "-Infinity", "'x'", "'10'", "undefined", "null", "true", "{}", "[]", "[3]", "2**32 + 1", "1e21", "-1.5", "0.5", "1n", "2**40"]) {
  T(`R = new ArrayBuffer(${size}).byteLength`);
}
for (const size of ["2**53", "-1", "1.5", "NaN", "Infinity", "'x'", "2**64", "1e21", "-0.5"]) {
  T(`R = new SharedArrayBuffer(${size}).byteLength`);
}
T("R = ArrayBuffer(8).byteLength");
T("R = ArrayBuffer.call(null, 8)");
T("R = new ArrayBuffer(8, { maxByteLength: 4 }).byteLength");
T("R = new ArrayBuffer(8, { maxByteLength: 2 ** 53 }).byteLength");
T("R = new ArrayBuffer(8, { maxByteLength: -1 }).byteLength");
T("R = new ArrayBuffer(8, { maxByteLength: 16 }).maxByteLength");
T("R = new ArrayBuffer(8, { maxByteLength: undefined }).resizable");
T("R = new ArrayBuffer(8, { maxByteLength: 'x' }).resizable");
T("R = new ArrayBuffer(8, { maxByteLength: Infinity }).resizable");
T("R = new ArrayBuffer(8, { maxByteLength: 2 ** 32 + 1 }).resizable");
T("R = new SharedArrayBuffer(8, { maxByteLength: 4 }).byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); b.resize(17); R = b.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); b.resize(-1); R = b.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); b.resize(2 ** 53); R = b.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); b.resize(NaN); R = b.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); b.resize(16); R = b.byteLength");
T("var b = new ArrayBuffer(8); b.resize(4); R = b.byteLength");
T("var b = new ArrayBuffer(8); R = b.maxByteLength + ',' + b.resizable");
T("var b = new SharedArrayBuffer(8, { maxByteLength: 16 }); b.grow(17); R = b.byteLength");
T("var b = new SharedArrayBuffer(8, { maxByteLength: 16 }); b.grow(4); R = b.byteLength");
T("var b = new SharedArrayBuffer(8); b.grow(16); R = b.byteLength");
T("var b = new ArrayBuffer(8); R = b.slice(2 ** 53, 2 ** 54).byteLength");
T("var b = new ArrayBuffer(8); R = b.slice(-100, 100).byteLength");
T("var b = new ArrayBuffer(8); R = b.slice(NaN, NaN).byteLength");
T("var b = new ArrayBuffer(8); R = b.transfer(2 ** 53).byteLength");
T("var b = new ArrayBuffer(8); R = b.transfer(-1).byteLength");
T("var b = new ArrayBuffer(8); R = b.transfer(16).byteLength");
T("var b = new ArrayBuffer(8); b.transfer(); R = b.byteLength + ',' + b.detached");
T("var b = new ArrayBuffer(8); b.transfer(); R = b.slice(0).byteLength");
T("var b = new ArrayBuffer(8); b.transfer(); b.transfer(); R = 'x'");
T("var b = new ArrayBuffer(8); b.transfer(); b.resize(1); R = 'x'");
T("var b = new ArrayBuffer(8); b.transfer(); R = new Uint8Array(b).length");
T("var b = new ArrayBuffer(8); b.transfer(); R = new DataView(b).byteLength");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = u.length + ',' + u.byteLength + ',' + u.byteOffset");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = u[0]");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); u.fill(1); R = 'x'");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = u.at(0)");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = [...u].length");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = u.subarray(0).length");
T("var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); R = u.slice(0).length");
T("var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); R = d.getInt8(0)");
T("var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); R = d.byteLength");
T("var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); R = d.byteOffset");
T("var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); R = d.buffer.byteLength");
T("R = ArrayBuffer.isView(1)");
T("R = ArrayBuffer.prototype.slice.call({}, 0)");
T("R = ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(1), 0)");
T("R = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call({})");
T("R = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call(new SharedArrayBuffer(1))");
T("R = Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'byteLength').get.call(new ArrayBuffer(1))");
const TA = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
for (const name of TA) {
  for (const size of ["-1", "2**53", "2**64", "1.5", "NaN", "Infinity", "'x'", "2**33"]) {
    T(`R = new ${name}(${size}).length`);
  }
}
for (const name of ["Int8Array", "Int16Array", "Int32Array", "Float64Array", "BigInt64Array"]) {
  T(`R = new ${name}(new ArrayBuffer(9)).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 1).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 17).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 0, 100).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 0, -1).length`);
  T(`R = new ${name}(new ArrayBuffer(16), -1).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 2 ** 53).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 0, 2 ** 53).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 8, 2).length`);
  T(`R = new ${name}(new ArrayBuffer(16), 16).length`);
  T(`R = new ${name}(new ArrayBuffer(16), NaN, NaN).length`);
  T(`R = ${name}(4).length`);
  T(`var t = new ${name}(4); t.set([1, 2, 3, 4, 5]); R = t.length`);
  T(`var t = new ${name}(4); t.set([1], 4); R = t.length`);
  T(`var t = new ${name}(4); t.set([1], -1); R = t.length`);
  T(`var t = new ${name}(4); t.set([1], 2 ** 53); R = t.length`);
  T(`var t = new ${name}(4); t.set({ length: 2 ** 32 }); R = t.length`);
  T(`var t = new ${name}(4); t.set(new ${name}(5)); R = t.length`);
  T(`var t = new ${name}(4); R = t.subarray(2 ** 53).length`);
  T(`var t = new ${name}(4); R = t.slice(-100, 100).length`);
  T(`var t = new ${name}(4); R = t.fill(0, 2 ** 53).length`);
  T(`var t = new ${name}(4); R = t.at(2 ** 53)`);
  T(`var t = new ${name}(4); R = t.with(4, 0).length`);
  T(`var t = new ${name}(4); R = t.with(-5, 0).length`);
}
T("R = new Int8Array(2 ** 32).length");
T("R = new Int8Array(2 ** 31).length > 0");
T("R = new Float64Array(2 ** 30).length > 0");
T("R = Int8Array.from({ length: 2 ** 32 }).length");
T("R = Int8Array.from({ length: -1 }).length");
T("R = Int8Array.of(...new Array(100000).fill(1)).length");
T("R = new Int8Array({ length: 2 ** 53 }).length");
T("R = new Int8Array({ length: -1 }).length");
T("R = new Int8Array({ length: 1.5, 0: 1 }).length");
T("R = new Int8Array(new Array(2 ** 20).fill(1)).length");
T("R = new Int8Array(Symbol())");
T("R = new Int8Array(1n)");
T("R = new BigInt64Array([1])");
T("R = new Int8Array([1n])");
T("R = new Int8Array(new BigInt64Array(1))");
T("R = new BigInt64Array(new Int8Array(1))");
T("R = new Uint8Array(new SharedArrayBuffer(8), 1, 2).length");
T("R = new Uint8Array(new SharedArrayBuffer(8), 9).length");
T("R = Uint8Array.prototype.length");
T("R = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), 'length').get.call({})");
T("R = Object.getPrototypeOf(Uint8Array)()");
T("R = new (Object.getPrototypeOf(Uint8Array))()");
T("R = Uint8Array.prototype.fill.call([], 1)");
T("R = Uint8Array.prototype.set.call({}, [])");
T("R = Uint8Array.BYTES_PER_ELEMENT + ',' + Float64Array.BYTES_PER_ELEMENT");
T("R = Uint8Array.from([1], 1)");
T("R = Uint8Array.from.call(Array, [1])");
T("R = Uint8Array.of.call(Array, 1)");
T("R = Uint8Array.from.call(function () { return new Uint8Array(0) }, [1, 2])");
T("R = Uint8Array.of.call(function () { return new Uint8Array(0) }, 1, 2)");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var u = new Uint8Array(b, 4, 4); b.resize(6); R = u.length");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var u = new Uint8Array(b, 4); b.resize(2); R = u.length + ',' + u.byteOffset");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var u = new Uint8Array(b, 4); b.resize(2); u.fill(1); R = 'x'");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var u = new Uint8Array(b); b.resize(16); R = u.length");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var u = new Uint8Array(b, 0, 8); b.resize(4); R = u.length + ',' + u.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var d = new DataView(b, 4, 4); b.resize(6); R = d.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var d = new DataView(b, 4); b.resize(2); R = d.byteLength");
T("var b = new ArrayBuffer(8, { maxByteLength: 16 }); var d = new DataView(b); b.resize(16); R = d.byteLength");
// DataView.
for (const args of ["", "1", "{}", "new ArrayBuffer(8), 9", "new ArrayBuffer(8), -1", "new ArrayBuffer(8), 2 ** 53", "new ArrayBuffer(8), 0, 9", "new ArrayBuffer(8), 4, 5", "new ArrayBuffer(8), 8, 0", "new ArrayBuffer(8), 8, 1", "new ArrayBuffer(8), NaN, NaN", "new ArrayBuffer(8), 0, -1", "new ArrayBuffer(8), 0, 2 ** 53", "new ArrayBuffer(8), 'x'", "new ArrayBuffer(8), 1.9, 2.9", "new SharedArrayBuffer(8), 9", "new Uint8Array(8)", "new ArrayBuffer(8), undefined, 9"]) {
  T(`R = new DataView(${args}).byteLength`);
}
T("R = DataView(new ArrayBuffer(8)).byteLength");
for (const op of ["getInt8(8)", "getInt8(-1)", "getInt8(2 ** 53)", "getInt8(NaN)", "getInt8(Infinity)", "getInt16(7)", "getInt32(5)", "getFloat64(1)", "getFloat64(0)", "getBigInt64(1)", "getBigUint64(8)", "getInt8()", "getUint16(7, true)", "getFloat32(5, true)", "getFloat32(4.9)", "getFloat32('4')", "getInt8(-0.5)", "getInt8(2 ** 32)"]) {
  T(`R = new DataView(new ArrayBuffer(8)).${op}`);
}
for (const op of ["setInt8(8, 1)", "setInt8(-1, 1)", "setInt16(7, 1)", "setFloat64(1, 1)", "setBigInt64(1, 1n)", "setBigInt64(0, 1)", "setInt8(0, 1n)", "setInt8(2 ** 53, 1)", "setUint32(5, 1)", "setInt8(0)", "setFloat32(0, {})", "setBigUint64(0, -1n)", "setBigUint64(0, 2n ** 64n)", "setBigInt64(0, 2n ** 63n)", "setBigInt64(0, 'x')", "setBigInt64(0, '12')", "setInt8(0, Symbol())"]) {
  T(`var d = new DataView(new ArrayBuffer(8)); d.${op}; R = 'ok'`);
}
T("R = DataView.prototype.getInt8.call({}, 0)");
T("R = DataView.prototype.getInt8.call(new Uint8Array(1), 0)");
T("R = Object.getOwnPropertyDescriptor(DataView.prototype, 'byteLength').get.call({})");
T("R = Object.getOwnPropertyDescriptor(DataView.prototype, 'buffer').get.call(new Uint8Array(1))");

// ---- Number.prototype.toFixed/toPrecision/toExponential/toString(radix).
for (const digits of ["-1", "0", "100", "101", "1000", "Infinity", "-Infinity", "NaN", "'50'", "'101'", "1e21", "100.9", "101.1", "-0.9", "-1.1", "undefined", "null", "{}", "[]", "'x'", "2**32", "1n"]) {
  T(`R = (1.5).toFixed(${digits})`);
  T(`R = (1.5).toPrecision(${digits})`);
  T(`R = (1.5).toExponential(${digits})`);
}
T("R = (1e21).toFixed(101)");
T("R = (NaN).toFixed(101)");
T("R = (Infinity).toFixed(-1)");
T("R = (NaN).toPrecision(101)");
T("R = (Infinity).toPrecision(0)");
T("R = (NaN).toExponential(101)");
T("R = (Infinity).toExponential(-1)");
T("R = (-Infinity).toExponential(1000)");
T("R = (0).toFixed(100).length");
T("R = (1e20).toFixed(100).length");
T("R = (1e-10).toFixed(100).length");
T("R = (123.456).toPrecision(100).length");
T("R = (123.456).toExponential(100).length");
T("R = (5e-324).toFixed(100).length");
T("R = (5e-324).toPrecision(100).slice(0, 12)");
T("R = (1.7976931348623157e308).toFixed(100)");
T("R = (1.7976931348623157e308).toPrecision(100).slice(0, 12)");
T("R = (1.7976931348623157e308).toExponential(100).slice(0, 12)");
T("R = Number.prototype.toFixed.call('1', 1)");
T("R = Number.prototype.toFixed.call({}, 1)");
T("R = Number.prototype.toPrecision.call(null, 1)");
T("R = Number.prototype.toExponential.call(Symbol(), 1)");
T("R = Number.prototype.toString.call(1n)");
T("R = Number.prototype.valueOf.call('1')");
T("R = Number.prototype.toLocaleString.call('1')");
for (const radix of ["0", "1", "37", "-1", "36", "2", "Infinity", "NaN", "'16'", "'37'", "1.9", "36.9", "37.1", "null", "undefined", "{}", "[]", "[10]", "2**32 + 2", "1e21", "-0", "1n", "Symbol()"]) {
  T(`R = (255).toString(${radix})`);
}
for (const radix of ["0", "1", "37", "-1", "36", "2", "'x'", "NaN", "Infinity"]) {
  T(`R = (255n).toString(${radix})`);
  T(`R = (0.5).toString(${radix})`);
  T(`R = (NaN).toString(${radix})`);
  T(`R = (Infinity).toString(${radix})`);
}
T("R = (-255.5).toString(2)");
T("R = (1e21).toString(36)");
T("R = (2 ** 53).toString(2).length");
T("R = (Number.MAX_VALUE).toString(2).length");
T("R = (Number.MAX_VALUE).toString(36).length");
T("R = (Number.MIN_VALUE).toString(2).length");
T("R = (Number.MIN_VALUE).toString(36).length > 100");
T("R = (0.1).toString(3).length > 10");
T("R = Number.prototype.toString.call(1, 37)");
T("R = Number.prototype.toString.call('1', 2)");
T("R = Number.prototype.toString.call(new Number(5), 2)");
T("R = Number.prototype.toString.call(Object(1), 1)");
T("R = parseInt('10', 37)");
T("R = parseInt('10', 1)");
T("R = parseInt('10', 36)");
T("R = parseInt('10', -1)");
T("R = parseInt('10', 2 ** 32 + 2)");
T("R = parseInt('10', 2 ** 32)");
T("R = parseInt('10', Infinity)");
T("R = parseInt('z', 36)");
T("R = parseInt('zz'.repeat(100), 36) > 0");
T("R = parseFloat('1e1000')");
T("R = Number('1e1000')");
T("R = Number('-1e1000')");
T("R = Number('1e-1000')");
T("R = Number('9'.repeat(400))");
T("R = Number('0x' + 'f'.repeat(300))");
T("R = Number('0b' + '1'.repeat(1100))");
T("R = 1e1000");
T("R = 0.1e-1000");
T("R = Number.MAX_SAFE_INTEGER + 2");
T("R = Number.MAX_VALUE * 2");
T("R = (2 ** 1024) + ',' + (2 ** -1075) + ',' + (2 ** 1023)");
T("R = Math.pow(2, 1e10) + ',' + Math.pow(2, -1e10)");
T("R = 2 ** 53 + 1");
T("R = (2 ** 53).toFixed(0) + ',' + (2 ** 70).toFixed(0)");
T("R = (1e21).toFixed(2) + ',' + (1e20).toFixed(2)");
T("R = Number.prototype.toFixed.length + ',' + Number.prototype.toPrecision.length + ',' + Number.prototype.toExponential.length");

// ---- BigInt: expoente negativo, gigante, deslocamentos e conversões.
for (const exp of ["-1n", "-2n ** 64n", "-(2n ** 100n)"]) {
  T(`R = 2n ** (${exp})`);
  T(`R = 0n ** (${exp})`);
  T(`R = 1n ** (${exp})`);
  T(`R = (-1n) ** (${exp})`);
}
for (const exp of ["1000000000n", "2n ** 31n", "2n ** 32n", "2n ** 53n", "2n ** 64n", "2n ** 100n", "10n ** 12n", "1000000n", "100000000n"]) {
  T(`R = 2n ** (${exp})`);
  T(`R = 3n ** (${exp})`);
  T(`R = (-2n) ** (${exp})`);
}
for (const base of ["0n", "1n", "-1n"]) {
  for (const exp of ["2n ** 64n", "2n ** 100n", "2n ** 64n + 1n", "10n ** 30n", "10n ** 30n + 1n"]) {
    T(`R = (${base}) ** (${exp})`);
  }
}
T("R = (2n ** 64n) ** (2n ** 40n)");
T("R = (2n ** 64n) ** 100000000n");
T("R = (2n ** 1000n) ** 1000000n");
T("R = (2n ** 1000n) ** 10000000n");
T("R = (10n ** 100n) ** (10n ** 10n)");
T("R = 10n ** 1000000n > 0n");
T("R = (10n ** 100000n).toString().length");
T("R = (10n ** 300000n).toString().length");
T("R = (2n ** 100000n).toString(16).length");
T("R = (2n ** 100000n).toString(2).length");
T("R = (2n ** 1000000n).toString(32).length");
T("R = (2n ** 1000000n) * (2n ** 1000000n) > 0n");
T("R = BigInt.asUintN(2 ** 53, 1n)");
T("R = BigInt.asUintN(2 ** 53 - 1, 1n)");
T("R = BigInt.asUintN(-1, 1n)");
T("R = BigInt.asIntN(-1, 1n)");
T("R = BigInt.asIntN(2 ** 53, 1n)");
T("R = BigInt.asIntN(2 ** 53 - 1, -1n)");
T("R = BigInt.asIntN(NaN, 5n)");
T("R = BigInt.asIntN(1.9, 5n)");
T("R = BigInt.asUintN(0, 5n)");
T("R = BigInt.asUintN(1, 5)");
T("R = BigInt.asUintN(1, '5')");
T("R = BigInt.asUintN(1, 'x')");
T("R = BigInt.asUintN(1, 1.5)");
T("R = BigInt.asUintN(2 ** 32, 2n ** 100n) === 2n ** 100n");
T("R = BigInt.asUintN(2 ** 40, 2n ** 100n) === 2n ** 100n");
T("R = BigInt.asUintN(2 ** 40, -1n).toString().length");
T("R = BigInt.asUintN(2 ** 31, -1n).toString().length");
T("R = BigInt.asUintN(2 ** 30, -1n).toString(2).length");
T("R = BigInt.asIntN(2 ** 40, -1n)");
T("R = 1n << 2n ** 40n");
T("R = 1n << 2n ** 31n");
T("R = 1n << 2n ** 32n");
T("R = 1n << 1000000000n");
T("R = 1n << 100000000n > 0n");
T("R = 1n << (2n ** 64n)");
T("R = 0n << (2n ** 64n)");
T("R = 0n << (2n ** 100n)");
T("R = 1n << -(2n ** 40n)");
T("R = -1n << -(2n ** 40n)");
T("R = 1n >> (2n ** 40n)");
T("R = -1n >> (2n ** 40n)");
T("R = -5n >> (2n ** 100n)");
T("R = 1n >> -(2n ** 40n)");
T("R = 1n >> -(2n ** 64n)");
T("R = 1n >> -1000000000n");
T("R = 0n >> -(2n ** 100n)");
T("R = 1n >>> 1n");
T("R = 1n >>> 0n");
T("R = 1n / 0n");
T("R = 1n % 0n");
T("R = -1n % 0n");
T("R = 0n / 0n");
T("R = (2n ** 100n) / 0n");
T("R = 1n + 1");
T("R = 1n * 1.5");
T("R = 1n ** 1");
T("R = +1n");
T("R = Math.max(1n)");
T("R = Math.abs(1n)");
T("R = 1n < 1.5");
T("R = 2n ** 64n > Infinity");
T("R = BigInt(Infinity)");
T("R = BigInt(-Infinity)");
T("R = BigInt(NaN)");
T("R = BigInt(1.5)");
T("R = BigInt(-1.5)");
T("R = BigInt(1e300) > 0n");
T("R = BigInt(Number.MAX_VALUE).toString().length");
T("R = BigInt(2 ** 53 + 2)");
T("R = BigInt(undefined)");
T("R = BigInt(null)");
T("R = BigInt(Symbol())");
T("R = BigInt('x')");
T("R = BigInt('1.5')");
T("R = BigInt('1e3')");
T("R = BigInt('0x')");
T("R = BigInt('0b2')");
T("R = BigInt('-0x1')");
T("R = BigInt(' 12 ')");
T("R = BigInt('')");
T("R = BigInt('1'.repeat(100000)) > 0n");
T("R = BigInt('9'.repeat(10000)).toString().length");
T("R = BigInt('0x' + 'f'.repeat(100000)).toString(16).length");
T("R = BigInt({})");
T("R = BigInt([])");
T("R = BigInt([5])");
T("R = BigInt(true)");
T("R = new BigInt(1)");
T("R = BigInt.prototype.toString.call(1)");
T("R = BigInt.prototype.valueOf.call('1')");
T("R = BigInt.prototype.toLocaleString.call({})");
T("R = JSON.stringify({ a: 1n })");
T("R = JSON.stringify(1n)");
T("R = JSON.stringify([1n])");
T("R = Number(2n ** 1024n)");
T("R = Number(-(2n ** 1024n))");
T("R = Number(2n ** 100000n)");
T("R = parseInt(String(2n ** 1024n))");
T("R = new Array(1n)");
T("R = [1, 2, 3].at(1n)");
T("R = 'abc'.charAt(1n)");
T("R = new Int8Array(2n)");
T("var a = []; a[2n ** 64n] = 1; R = Object.keys(a)");
T("R = Math.pow(2n, 2n)");
T("R = isNaN(1n)");
T("R = Number.isInteger(1n)");
T("R = Number.parseFloat('1n')");
T("R = BigInt(Number.MAX_SAFE_INTEGER) + 2n");
T("R = (2n ** 64n).toString(37)");
T("R = (2n ** 64n).toString(1)");
T("R = (2n ** 64n).toString(0)");
T("R = (-(2n ** 64n)).toString(36)");

// ---- Regex: tamanho de padrão, backtracking curto e erros de sintaxe.
T("R = /(a+)+b/.test('a'.repeat(22))");
T("R = /(a*)*b/.test('a'.repeat(22))");
T("R = /(a|aa)+b/.test('a'.repeat(24))");
T("R = /^(a+)+$/.test('a'.repeat(22) + 'b')");
T("R = /(x+x+)+y/.test('x'.repeat(20))");
T("R = /(.*)*c/.test('a'.repeat(22))");
T("R = /(?:a?){20}a{20}/.test('a'.repeat(20))");
T("R = /(a+)+b/.exec('a'.repeat(15) + 'b')[0].length");
T("R = /^(([a-z])+.)+[A-Z]([a-z])+$/.test('aaaaaaaaaaaaaaaaaaaaaa!')");
T("R = 'a'.repeat(100000).replace(/a*?b|a/g, 'x').length");
T("R = /a*?b/.test('a'.repeat(100000))");
T("R = /(a)*/.exec('a'.repeat(100000))[0].length");
T("R = /(?:a|b)*c/.test('ab'.repeat(50000))");
T("R = /(?:a|b)*/.exec('ab'.repeat(500000))[0].length");
T("R = /(a|b)*/.exec('ab'.repeat(100000))[0].length");
T("R = /(a|b)*c/.test('ab'.repeat(100000))");
T("R = /(?:a|b)*c/.test('ab'.repeat(500000))");
T("R = new RegExp('(?:' + 'a|'.repeat(10000) + 'b)').test('b')");
T("R = new RegExp('a'.repeat(100000)).test('a'.repeat(100000))");
T("R = new RegExp('a'.repeat(1000000)).test('a')");
T("R = new RegExp('(' .repeat(1000) + ')'.repeat(1000)).test('')");
T("R = new RegExp('('.repeat(10000) + ')'.repeat(10000)).test('')");
T("R = new RegExp('('.repeat(100000) + ')'.repeat(100000)).test('')");
T("R = new RegExp('(?:'.repeat(100000) + ')'.repeat(100000)).test('')");
T("R = new RegExp('(?:'.repeat(1000) + 'a' + ')'.repeat(1000)).test('a')");
T("R = new RegExp('('.repeat(70000) + ')'.repeat(70000)).test('')");
T("R = new RegExp('[' .repeat(10) + ']'.repeat(10))");
T("R = new RegExp('(' .repeat(100))");
T("R = new RegExp(')')");
T("R = new RegExp('[')");
T("R = new RegExp('*')");
T("R = new RegExp('a**')");
T("R = new RegExp('a{2,1}')");
T("R = new RegExp('a{99999999999}').test('a')");
T("R = new RegExp('a{1,99999999999}').test('a')");
T("R = new RegExp('a{4294967296}').test('a')");
T("R = new RegExp('a{4294967295}').test('a')");
T("R = new RegExp('a{2147483648}').test('a')");
T("R = new RegExp('a{2147483647}').test('a')");
T("R = new RegExp('a{1000000}').test('a')");
T("R = new RegExp('(?:a{1000}){1000}').test('a')");
T("R = new RegExp('(?:a{10000}){10000}').test('a')");
T("R = new RegExp('(?:a{100000}){100000}').test('a')");
T("R = new RegExp('(a{1000}){1000}').test('a')");
T("R = new RegExp('(?:a{65536}){2}').test('a')");
T("R = new RegExp('(?:(?:a{100}){100}){100}').test('a')");
T("R = new RegExp('(?:(?:a{1000}){1000}){1000}').test('a')");
T("R = /a{0,1000000}/.test('b')");
T("R = /(?:a{0,1000}){0,1000}/.test('b')");
T("R = /(?:a{1000}){1000}/.test('a')");
T("R = new RegExp('\\\\' )");
T("R = new RegExp('(?<n>a)(?<n>b)')");
T("R = new RegExp('\\\\k<x>', 'u')");
T("R = new RegExp('\\\\1', 'u')");
T("R = new RegExp('a', 'gg')");
T("R = new RegExp('a', 'z')");
T("R = new RegExp('a', 'uv')");
T("R = new RegExp('(?<=a)+', 'u')");
T("R = new RegExp('a{,5}', 'u')");
T("R = new RegExp('[b-a]')");
T("R = new RegExp('\\\\u{110000}', 'u')");
T("R = new RegExp('\\\\p{Foo}', 'u')");
T("R = new RegExp('(?<a', 'u')");
T("R = new RegExp('x{1', 'u')");
T("R = new RegExp('(?i:a)')");
T("R = RegExp.prototype.exec.call({}, 'a')");
T("R = RegExp.prototype.test.call(1, 'a')");
T("R = Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get.call({})");
T("R = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call(1)");
T("R = Object.getOwnPropertyDescriptor(RegExp.prototype, 'source').get.call({})");
T("R = 'a'.matchAll(/a/)");
T("R = 'a'.replaceAll(/a/, 'b')");
T("R = 'a'.repeat(1000000).match(/a/g).length");
T("R = 'a'.repeat(1000000).split(/a/).length");
T("R = 'ab'.repeat(500000).replace(/(a)(b)/g, '$2$1').length");
T("R = /\\b(\\w+)\\s+\\1\\b/.test('word word'.repeat(1000))");
T("R = /(?=(a+))a*b\\1/.test('baaabac')");
T("R = /(.*?)*x/.test('a'.repeat(22))");
T("R = /^(?:a+)*$/.test('a'.repeat(22) + '!')");
T("R = /(\\d+)+x/.test('1'.repeat(22))");
T("R = /^(\\w+\\s?)*$/.test('word '.repeat(5) + '!')");
T("R = /([a-z]+)*\\d/.test('abcdefghijklmnopqrstu')");
T("R = /(a?){25}a{25}/.test('a'.repeat(25))");
T("R = /(?:(?:a+)+)+b/.test('a'.repeat(18))");
T("R = /^(?:(a+)|b)*c/.test('aaaaaaaaaaaaaaaaaaab')");

// ---- Tail calls (strict) e erros em chamada de cauda.
for (const n of [1000, 10000, 100000, 1000000]) {
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : f(n - 1) } R = f(${n})`);
  T(`'use strict'; var f = n => n === 0 ? 'done' : f(n - 1); R = f(${n})`);
  T(`'use strict'; function f(n) { if (n === 0) return 'done'; return f(n - 1) } R = f(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : f.call(null, n - 1) } R = f(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : f.apply(null, [n - 1]) } R = f(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : n && f(n - 1) } R = f(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : (0, f)(n - 1) } R = f(${n})`);
  T(`'use strict'; function a(n) { return n === 0 ? 'done' : b(n - 1) } function b(n) { return a(n) } R = a(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : 1 + f(n - 1) } R = f(${n})`);
  T(`'use strict'; function f(n) { try { return n === 0 ? 'done' : f(n - 1) } finally { } } R = f(${n})`);
  T(`'use strict'; function f(n) { return n === 0 ? 'done' : [f(n - 1)][0] } R = f(${n})`);
  T(`'use strict'; var o = { m(n) { return n === 0 ? 'done' : this.m(n - 1) } }; R = o.m(${n})`);
  T(`'use strict'; class C { static m(n) { return n === 0 ? 'done' : C.m(n - 1) } } R = C.m(${n})`);
  T(`function f(n) { return n === 0 ? 'done' : f(n - 1) } R = f(${n})`);
}
T("'use strict'; function f() { return f() } f(); R = 'ok'");
T("'use strict'; function f(n) { return n === 0 ? new Error('x').message : f(n - 1) } R = f(1000000)");
T("'use strict'; function f(n) { return n === 0 ? (function () { return typeof arguments.callee })() : f(n - 1) } R = f(10)");
T("'use strict'; function f(n) { return n === 0 ? f.caller : f(n - 1) } R = f(10)");
T("'use strict'; function f(n) { return n === 0 ? arguments.callee : f(n - 1) } R = f(10)");
T("'use strict'; function f(n) { return n === 0 ? null.x : f(n - 1) } R = f(1000000)");
T("'use strict'; function f(n) { return n === 0 ? undefinedVar : f(n - 1) } R = f(1000000)");
T("'use strict'; function f(n) { return n === 0 ? (() => { throw new RangeError('fim') })() : f(n - 1) } R = f(100000)");
T("'use strict'; function f(n) { if (n === 0) throw new TypeError('base'); return f(n - 1) } R = f(100000)");
T("'use strict'; function f(n) { return n === 0 ? 1n + 1 : f(n - 1) } R = f(100000)");
T("'use strict'; function f(n) { return n === 0 ? [].reduce((a, b) => a) : f(n - 1) } R = f(100000)");
T("'use strict'; function f(n) { return n === 0 ? new (class A { constructor() { new.target.x.y } })() : f(n - 1) } R = f(100)");
T("'use strict'; var f = 1; function g(n) { return n === 0 ? f() : g(n - 1) } R = g(100000)");
T("'use strict'; var o = {}; function g(n) { return n === 0 ? o.m() : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new 1 : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new (() => {}) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? Symbol() + '' : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? 'a'.repeat(-1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? (1).toFixed(101) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? decodeURIComponent('%') : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? JSON.parse('{') : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new Array(-1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? Object.defineProperty(1, 'x', {}) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? Object.freeze([1]).push(2) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? null[Symbol.iterator] : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? ({}).x.y.z : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? ({}).f() : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? ({}).a.b() : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? [][0]() : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? (void 0)() : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new Proxy({}, null) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? Reflect.ownKeys(1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new WeakMap().set(1, 1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? new Set(1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? [...1] : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? Promise.resolve.call(1) : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? class A extends 1 {} : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? 1 in 1 : g(n - 1) } R = g(100000)");
T("'use strict'; function g(n) { return n === 0 ? 1 instanceof 1 : g(n - 1) } R = g(100000)");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "limits-range-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (sem o transpilador do bun sobre arquivos).
const source_file = path.join(dir, "limits_range_source.js");
const file = path.join(dir, "limits_range_case.js");
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
const measure = source => {
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 2000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) return null;
  try {
    return JSON.parse(marked.slice(1));
  } catch (error) {
    return null;
  }
};
// Programas que alocariam gigabytes (strings de 2^29 ou mais, arrays esparsos de 2^32 percorridos, joins de 2^31)
// ficam de fora: o golden não pode consumir memória nem tempo do bun.
const HEAVY = new RegExp(
  [
    "2 \\*\\* (29|30)\\b",
    "new Array\\(2 \\*\\* 3[12]",
    "var a = new Array\\(2 \\*\\* 32 - 1\\)",
    "new Array\\(2 \\*\\* 32 - 1\\)\\.(toString|concat)",
    "\\[\\]\\.concat\\(new Array\\(2 \\*\\* 32",
    "Int8Array\\(2 \\*\\* 3[12]\\)",
    "2 \\*\\* 17\\)",
    "2 \\*\\* 16\\)\\.fill\\('a'",
    "2 \\*\\* 20\\)\\.fill\\('a'",
    "s \\+= s",
    "\\.repeat\\(2 \\*\\* 2[01]\\)\\; R = s\\.replace",
    "'a'\\.repeat\\(2 \\*\\* 21\\)",
    "Int8Array\\.from\\(\\{ length: 2 \\*\\* 32",
    "Array\\.from\\(\\{ length: 2 \\*\\* (32|53)",
  ].join("|"),
);
for (const body of programs) {
  if (HOST.test(body) || HEAVY.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  const first = measure(source);
  if (first === null) {
    dropped++;
    process.stderr.write("sem resultado (ou mais de 2 s): " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const second = measure(source);
  if (second !== first) {
    dropped++;
    process.stderr.write("instável: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = first.split("file://" + prefix).join("file:///").split(prefix).join("");
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
