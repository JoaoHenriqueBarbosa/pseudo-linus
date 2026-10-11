// Gera tests/golden/destructuring_bun.tsv: destructuring e parâmetros (array/object patterns, defaults, rest,
// aninhados, iteradores que lançam e fecham, ordem de avaliação, parâmetros com defaults e escopo, arguments mapeado
// e não mapeado, TDZ de parâmetros, for-of/for-in com patterns, catch com pattern) medidos no bun.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança); SyntaxError sai por eval indireto.
// Os programas não usam API de host (setTimeout, process, console, require, Bun, URL, Buffer); o bun mede por
// `require('node:vm').runInThisContext`, nunca como arquivo, porque o transpilador do bun muda a semântica de script.
// Uso: bun scripts/gen-destructuring-golden.js > tests/golden/destructuring_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const J = "JSON.stringify";
// Programa com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Fonte compilada por eval indireto (escopo global), para os SyntaxError.
const S = src => add(`try { (0, eval)(${JSON.stringify(src)}); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }`);
// Iterador instrumentado: `mk(valores, opções)` registra iter/next/ret em `log`.
const PRELUDE =
  "var log = []; function mk(vals, o) { o = o || {}; var i = 0; return { [Symbol.iterator]() { log.push('iter'); return this }, " +
  "next() { log.push('next' + i); if (o.throwAt === i) throw new Error('boom' + i); return i < vals.length ? { value: vals[i++], done: false } : { value: undefined, done: true } }, " +
  "return(v) { log.push('ret'); if (o.retThrow) throw new Error('rt'); return o.retVal === undefined ? {} : o.retVal } } } ";
// Programa com o iterador instrumentado; o log vai junto da mensagem quando lança.
const L = body =>
  add(`${PRELUDE}try { ${body} } catch (e) { R = e.name + ': ' + e.message + '|' + log.join() }`);

// ---- Array patterns: formas básicas e valores de origem.
const arrayPatterns = [
  "[a]", "[a, b]", "[, a]", "[a, , b]", "[a = 9]", "[a, b = a]", "[...a]", "[a, ...b]", "[, ...a]", "[[a]]", "[[a], b]",
  "[{ a }]", "[a = 1, [b = 2]]", "[...[a, b]]", "[...{ length: a }]", "[a, [b, [c]]]", "[]", "[,]", "[a, ,]",
];
const arraySources = ["[1, 2, 3]", "[]", "[[7], 8]", "'xyz'", "new Set([4, 5])", "[undefined, null]", "[0]", "[[], {}]", "undefined", "null", "1", "{}", "{ length: 1, 0: 'a' }", "(function* () { yield 1; yield 2 })()"];
for (const p of arrayPatterns) {
  for (const src of arraySources.slice(0, 6)) {
    T(`var a, b, c; var ${p.replace(/\ba\b/g, "a").replace(/^\[/, "[")} = ${src}; R = ${J}([typeof a, a, b, c])`);
  }
}
for (const src of arraySources.slice(6)) {
  T(`var [a, b = 5] = ${src}; R = ${J}([a, b])`);
  T(`let [x, ...y] = ${src}; R = ${J}([x, y])`);
}
T("var [a, b] = 'ab'; R = a + b");
T("var [a, b] = '\\ud83d\\ude00x'; R = a.length + ':' + b");
T("var [...r] = 'a\\ud83d\\ude00'; R = r.length");
T("var [a] = new Map([[1, 2]]); R = " + J + "(a)");
T("var [a, b] = new Map([[1, 2], [3, 4]]).keys(); R = a + b");
T("var [a, b] = [1, 2].entries(); R = " + J + "([a, b])");
T("var [a = 1, b = a] = [undefined, undefined]; R = a + b");
T("var [a = 1] = [null]; R = a");
T("var [a = 1] = [0]; R = a");
T("var [a = 1] = [undefined]; R = a");
T("var [a = 1] = [,]; R = a");
T("var [a = (() => 5)()] = []; R = a");
T("var [f = () => 1, g = function () {}, h = class {}] = []; R = [f.name, g.name, h.name].join()");
T("var [f = function () {}] = []; R = f.name");
T("var [f = (function () {})] = []; R = f.name");
T("var [f = (0, function () {})] = []; R = f.name === ''");
T("var o = {}; [o.f = function () {}] = []; R = o.f.name === ''");
T("var f; [f = function () {}] = []; R = f.name");
T("var f; [f = () => {}] = []; R = f.name");
T("var f; ({ f = class {} } = {}); R = f.name");
T("var a = [1, 2, 3]; var [x, ...y] = a; y.push(9); R = a.length + ':' + y.length");
T("var [...[a, ...b]] = [1, 2, 3]; R = " + J + "([a, b])");
T("var [...[a, ...b]] = 'abc'; R = " + J + "([a, b])");
T("var [[a, b] = [1, 2], { c } = { c: 3 }] = []; R = [a, b, c].join()");
T("var [[a, b] = [1, 2]] = [[5]]; R = [a, b].join()");
T("var [{ length }] = ['abc']; R = length");
T("var [[length]] = ['abc']; R = length");
T("var [a] = [[1, 2]]; R = " + J + "(a)");
T("var [a, [b, [c, [d]]]] = [1, [2, [3, [4]]]]; R = a + b + c + d");
T("var [a, [b]] = [1]; R = a");
T("var [a, [b]] = [1, null]; R = a");
T("var [a, { b }] = [1, undefined]; R = a");
T("var [a, ...[]] = [1, 2]; R = a");
T("var [, , , a] = [1, 2, 3, 4]; R = a");
T("var [a, a] = [1, 2]; R = a");
T("let [a] = [1], [b] = [2]; R = a + b");
T("const [a, b] = [1, 2]; try { a = 3 } catch (e) { R = e.name + ': ' + e.message }");
T("const [...r] = [1]; r.push(2); R = r.length");
T("Array.prototype[Symbol.iterator] = function* () { yield 'p' }; var [a] = [1]; R = a");
T("var saved = Array.prototype[Symbol.iterator]; Array.prototype[Symbol.iterator] = function () { return { next() { return { done: true } } } }; var [a] = [1]; Array.prototype[Symbol.iterator] = saved; R = typeof a");
T("var [a] = { [Symbol.iterator]() { return [10][Symbol.iterator]() } }; R = a");
T("var [a] = { [Symbol.iterator]: 1 }; R = a");
T("var [a] = { [Symbol.iterator]() { return 1 } }; R = a");
T("var [a] = { [Symbol.iterator]() { return {} } }; R = a");
T("var [a] = { [Symbol.iterator]() { return { next: 1 } } }; R = a");
T("var [a] = { [Symbol.iterator]() { return { next() { return 1 } } } }; R = a");
T("var [a] = { [Symbol.iterator]() { return { next() { return undefined } } } }; R = a");
T("var [a] = { [Symbol.iterator]: null }; R = a");
T("var [a] = { [Symbol.iterator]: undefined }; R = a");
T("var [a] = Symbol(); R = a");
T("var [a] = 1n; R = a");
T("var [a] = true; R = a");
T("var [a] = NaN; R = a");
T("var [a] = () => 1; R = a");
T("var [a] = class {}; R = a");
T("var [a] = new Date(0); R = a");
T("var [a] = /x/; R = a");
T("var [a] = new Proxy([5], {}); R = a");
T("var [a] = new Proxy({}, {}); R = a");
T("var [a, b] = (function () { return arguments })(1, 2); R = a + b");
T("var [a, b] = new Uint8Array([3, 4]); R = a + b");
T("var [a] = new ArrayBuffer(1); R = a");
T("var [a, b] = Object.assign([1, 2], { 5: 3 }); R = a + b");
T("var a = [1, 2]; var [b, c] = a.concat([3]); R = b + c");
T("var [a] = [1, 2].values(); R = a");
T("var [a] = Object.create([1, 2]); R = a");
T("var [a, b] = Object.setPrototypeOf({ 0: 'x', 1: 'y', length: 2 }, Array.prototype); R = a");
T("var [a = b, b] = [undefined, 1]; R = a");
T("var [a, b = a] = [1]; R = b");
T("var [a = 1, b = a + 1, c = b + 1] = []; R = a + b + c");
T("var [x = y, y = 1] = []; R = x");
T("let [x = y, y = 1] = []; R = x");
T("let [x = x] = []; R = x");
T("let [x = x] = [3]; R = x");
T("var [x = typeof x] = []; R = x");
T("let [x = typeof x] = []; R = x");
T("const [x = 1, y = x] = []; R = x + y");

// ---- Object patterns.
const objectPatterns = [
  "{ a }", "{ a, b }", "{ a: b }", "{ a = 1 }", "{ a: b = 2 }", "{ a, ...r }", "{ ...r }", "{ a: { b } }", "{ a: [b] }",
  "{ 'a': b }", "{ 1: b }", "{ ['a']: b }", "{ [k]: b }", "{ a: { b = 3 } = {} }", "{ a: [b, c = 4] = [] }", "{}", "{ a: {} }",
];
const objectSources = ["{ a: 1, b: 2 }", "{ a: undefined }", "{ a: null }", "{ a: { b: 5 } }", "{ a: [7, 8] }", "{ 1: 'one', a: 'x' }", "'str'", "[1, 2]", "Object(1n)"];
for (const p of objectPatterns) {
  for (const src of objectSources) {
    T(`var k = 'a', a, b, c, r; var ${p} = ${src}; R = ${J}([a, b, c, r])`);
  }
}
for (const src of ["undefined", "null", "1", "true", "Symbol('s')", "1n", "''", "NaN", "0"]) {
  T(`var { a } = ${src}; R = typeof a`);
  T(`var { } = ${src}; R = 'ok'`);
  T(`var { a = 1 } = ${src}; R = a`);
}
T("var { toFixed } = 1; R = typeof toFixed");
T("var { length } = 'abc'; R = length");
T("var { 0: a, length } = 'abc'; R = a + length");
T("var { a } = Symbol.iterator; R = typeof a");
T("var { description } = Symbol('d'); R = description");
T("var { a: { b } } = { a: null }; R = b");
T("var { a: { b } } = { a: undefined }; R = b");
T("var { a: { b } } = {}; R = b");
T("var { a: [b] } = { a: null }; R = b");
T("var { a: [b] } = {}; R = b");
T("var { a: { b } = null } = {}; R = b");
T("var { a: [b] = null } = {}; R = b");
T("var { a: { b } = {} } = {}; R = typeof b");
T("var { a = 1, a: b } = { a: 5 }; R = a + ':' + b");
T("var { a: b, a: c } = { a: 5 }; R = b + c");
T("var { x: { y: { z } } } = { x: { y: { z: 'deep' } } }; R = z");
T("var { [`k${1}`]: v } = { k1: 'tpl' }; R = v");
T("var { [1 + 1]: v } = { 2: 'num' }; R = v");
T("var s = Symbol('s'); var { [s]: v } = { [s]: 'sym' }; R = v");
T("var { [{ toString() { return 'a' } }]: v } = { a: 'obj' }; R = v");
T("var { [Symbol.iterator]: it } = []; R = typeof it");
T("var { __proto__: p } = {}; R = p === Object.prototype");
T("var { __proto__ } = {}; R = __proto__ === Object.prototype");
T("var { constructor } = {}; R = constructor === Object");
T("var { a, ...r } = { a: 1, b: 2, c: 3 }; R = " + J + "(r)");
T("var { ...r } = { a: 1, get b() { return 2 } }; R = " + J + "(Object.getOwnPropertyDescriptor(r, 'b'))");
T("var { ...r } = [1, 2]; R = " + J + "(r)");
T("var { ...r } = 'ab'; R = " + J + "(r)");
T("var { ...r } = 1; R = " + J + "(r)");
T("var { ...r } = null; R = r");
T("var { ...r } = Object.create({ inherited: 1 }); R = " + J + "(r)");
T("var o = Object.defineProperty({ a: 1 }, 'h', { value: 2, enumerable: false }); var { ...r } = o; R = " + J + "(Object.getOwnPropertyNames(r))");
T("var s = Symbol('s'); var { ...r } = { [s]: 1, a: 2 }; R = Object.getOwnPropertySymbols(r).length + ':' + r.a");
T("var s = Symbol('s'); var { [s]: x, ...r } = { [s]: 1, a: 2 }; R = Object.getOwnPropertySymbols(r).length + ':' + r.a");
T("var { a, ...{ length } } = { a: 1, b: 2 }; R = length");
T("var { a, ...r } = Object.create(null, { a: { value: 1, enumerable: true } }); R = Object.getPrototypeOf(r) === Object.prototype");
T("var { ...r } = { __proto__: null, a: 1 }; R = Object.getPrototypeOf(r) === Object.prototype");
T("var { ...r } = { ['__proto__']: 1 }; R = Object.getOwnPropertyNames(r).join()");
T("var { 'b-c': bc, 'd e': de } = { 'b-c': 1, 'd e': 2 }; R = bc + de");
T("var { 0: a, 1: b, 2: c = 'd' } = ['x', 'y']; R = a + b + c");
T("var { 0.5: a, 1e1: b, 0x10: c } = { 0.5: 'h', 10: 't', 16: 's' }; R = a + b + c");
T("var { a = 1, b = a } = {}; R = a + b");
T("var { a = b, b = 1 } = {}; R = a");
T("let { a = b, b = 1 } = {}; R = a");
T("let { a = a } = {}; R = a");
T("var { a = (() => 'f')() } = {}; R = a");
T("var { a = 1 } = { a: null }; R = a");
T("var { a = 1 } = { a: undefined }; R = a");
T("var { a = 1 } = { get a() { return undefined } }; R = a");
T("var { f = function () {}, g = () => {}, h = class {} } = {}; R = [f.name, g.name, h.name].join()");
T("var { f: g = function () {} } = {}; R = g.name");
T("var { f: g = (function () {}) } = {}; R = g.name");
T("var o = {}; ({ f: o.g = function () {} } = {}); R = o.g.name === ''");
T("var { a: b } = { a: function () {} }; R = b.name");
T("var { a: { b } } = { a: { b: 1 } }; R = typeof a");
T("let { a: { b } } = { a: { b: 1 } }; R = typeof a");
T("const { a } = { a: 1 }; try { a = 2 } catch (e) { R = e.name + ': ' + e.message }");
T("var { a, a } = { a: 1 }; R = a");
T("var { a: x, b: x } = { a: 1, b: 2 }; R = x");
T("var { a: [x, y] = [1, 2], b: { z } = { z: 3 } } = {}; R = x + y + z");
T("var { length: n, 0: first } = [5, 6, 7]; R = n + first");
T("var { a: { length } } = { a: 'abcd' }; R = length");
T("var { a: [{ b: [c] }] } = { a: [{ b: [9] }] }; R = c");
T("var { a: [{ b: [c] }] } = { a: [{ b: [] }] }; R = c");
T("var { a: [{ b: [c] }] } = { a: [] }; R = c");
T("var { a: { b: { c } } } = { a: { b: null } }; R = c");

// ---- Getters, proxies e ordem de acesso em object patterns.
L("var o = { get a() { log.push('ga'); return 1 }, get b() { log.push('gb'); return 2 } }; var { b, a } = o; R = log.join()");
L("var o = { get a() { log.push('ga'); return 1 } }; var { a, a: c } = o; R = log.join()");
L("var o = { get a() { log.push('ga'); return 1 }, get b() { log.push('gb'); return 2 } }; var { a, ...r } = o; R = log.join() + ':' + " + J + "(r)");
L("var o = { get a() { log.push('ga'); return 1 }, get b() { log.push('gb'); return 2 } }; var { ...r } = o; R = log.join()");
L("var p = new Proxy({ a: 1, b: 2 }, { get(t, k) { log.push('get:' + String(k)); return t[k] }, ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k) } }); var { a, ...r } = p; R = log.join()");
L("var p = new Proxy({ a: 1 }, { get(t, k) { log.push('get:' + String(k)); return t[k] } }); var { a, b } = p; R = log.join()");
L("var p = new Proxy({ a: 1 }, { get(t, k) { log.push('get:' + String(k)); return t[k] }, has(t, k) { log.push('has'); return k in t } }); var { a = 5 } = p; R = log.join()");
L("var o = { get a() { throw new Error('ga') } }; var { a } = o");
L("var o = { get a() { log.push('ga'); throw new Error('ga') }, get b() { log.push('gb'); return 1 } }; var { a, b } = o");
L("var { a = (log.push('d1'), 1), b = (log.push('d2'), 2) } = { a: 0 }; R = log.join()");
L("var { [(log.push('k1'), 'a')]: x = (log.push('d1'), 1), [(log.push('k2'), 'b')]: y = (log.push('d2'), 2) } = {}; R = log.join()");
L("var { [(log.push('k1'), 'a')]: x, ...r } = { a: 1, b: 2 }; R = log.join() + ':' + " + J + "(r)");
L("var { [{ toString() { log.push('ts'); return 'a' } }]: x, ...r } = { a: 1, b: 2 }; R = log.join() + ':' + " + J + "(r)");
L("var { [(log.push('k'), Symbol.iterator)]: x } = []; R = log.join()");
L("var [{ a = (log.push('d'), 1) }, b = (log.push('e'), 2)] = [{}]; R = log.join()");
L("var { a: [x = (log.push('d1'), 1)] = (log.push('d2'), []) } = {}; R = log.join()");
L("var { a = (log.push('d1'), { b: (log.push('d3'), 1) }) } = {}; var { b: c = (log.push('d2'), 2) } = a; R = log.join()");

// ---- Iteradores que lançam, fecham e retornam.
L("var [a, b] = mk([1, 2, 3]); R = log.join()");
L("var [a, b, c] = mk([1, 2, 3]); R = log.join()");
L("var [a, b, c, d] = mk([1, 2, 3]); R = log.join()");
L("var [a] = mk([]); R = log.join()");
L("var [] = mk([1]); R = log.join()");
L("var [,] = mk([1]); R = log.join()");
L("var [, ,] = mk([1, 2, 3]); R = log.join()");
L("var [...a] = mk([1, 2, 3]); R = log.join() + ':' + a.length");
L("var [a, ...b] = mk([1, 2, 3]); R = log.join() + ':' + b.length");
L("var [a, ...[b]] = mk([1, 2, 3]); R = log.join()");
L("var [a = 1] = mk([undefined]); R = log.join()");
L("var [a = 1, b = 2] = mk([]); R = log.join()");
L("var [[a]] = mk([[1]]); R = log.join()");
L("var [[a]] = mk([mk([1])]); R = log.join()");
L("var [[a, b]] = mk([mk([1, 2, 3])]); R = log.join()");
L("var [[a, b, c]] = mk([mk([1])]); R = log.join()");
L("var [a, b] = mk([1, 2], { retVal: 5 }); R = log.join()");
L("var [a] = mk([1], { retVal: 5 }); R = log.join()");
L("var [a] = mk([1], { retVal: null }); R = log.join()");
L("var [a] = mk([1], { retThrow: true }); R = log.join()");
L("var [a] = mk([1], { retVal: undefined }); R = log.join()");
L("var [a, b = (() => { throw new Error('dflt') })()] = mk([1, undefined], { retThrow: true }); R = log.join()");
L("var [a = (() => { throw new Error('dflt') })()] = mk([undefined]); R = log.join()");
L("var [a, b = (() => { throw new Error('dflt') })()] = mk([1, undefined]); R = log.join()");
L("var [a] = mk([1], { throwAt: 0 }); R = log.join()");
L("var [a, b] = mk([1, 2], { throwAt: 1 }); R = log.join()");
L("var [a, b] = mk([1, 2], { throwAt: 1, retThrow: true }); R = log.join()");
L("var [...a] = mk([1, 2], { throwAt: 1 }); R = log.join()");
L("var [a, ...b] = mk([1, 2, 3], { throwAt: 2 }); R = log.join()");
L("var [a] = mk([1], { retVal: 1 }); R = log.join()");
L("var [a] = mk([1], { retVal: 'x' }); R = log.join()");
L("var [{ a }] = mk([null]); R = log.join()");
L("var [{ a }] = mk([null], { retThrow: true }); R = log.join()");
L("var [[a]] = mk([null]); R = log.join()");
L("var [a, [b]] = mk([1, undefined]); R = log.join()");
L("var [a = 1, { b }] = mk([undefined, undefined]); R = log.join()");
L("var a; [a] = mk([1]); R = log.join()");
L("var a, b; [a, b] = mk([1]); R = log.join()");
L("var o = {}; [o.x, o.y] = mk([1, 2]); R = log.join()");
L("var o = { set x(v) { log.push('setx'); throw new Error('sx') } }; [o.x] = mk([1]); R = log.join()");
L("var o = { set x(v) { log.push('setx'); throw new Error('sx') } }; [o.x] = mk([1], { retThrow: true }); R = log.join()");
L("var o = { set x(v) { log.push('setx') } }; [o.x, ...o.x] = mk([1, 2]); R = log.join()");
L("var a; [a = (log.push('d'), 9)] = mk([]); R = log.join()");
L("var a; [a = (() => { throw new Error('dflt') })()] = mk([]); R = log.join()");
L("var a; [(log.push('t'), { set x(v) { log.push('set') } }).x] = mk([1]); R = log.join()");
L("var a; [(log.push('t'), { set x(v) { log.push('set') } }).x] = mk([]); R = log.join()");
L("var a; [(log.push('t'), { set x(v) { log.push('set') } }).x = (log.push('d'), 1)] = mk([]); R = log.join()");
L("var a = [1]; var [x] = a; R = log.join() + typeof x");
L("function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } var [a] = g(); R = log.join() + a");
L("function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } var [a, b] = g(); R = log.join() + a + b");
L("function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } var [a, b, c] = g(); R = log.join() + typeof c");
L("function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } var [...a] = g(); R = log.join() + a.length");
L("function* g() { try { yield 1 } finally { throw new Error('fin') } } var [a] = g(); R = log.join()");
L("function* g() { try { yield 1 } finally { return 5 } } var [a] = g(); R = a");
L("function* g() { try { yield 1; yield 2 } finally { yield 'f' } } var [a] = g(); R = a");
L("function* g() { var x = yield 1; log.push('got' + x); yield 2 } var [a, b] = g(); R = log.join() + a + b");
L("function* g() { yield 1; throw new Error('g') } var [a] = g(); R = a");
L("function* g() { yield 1; throw new Error('g') } var [a, b] = g(); R = a");
L("function* g() { yield 1; throw new Error('g') } var [...a] = g(); R = a");
L("var it = mk([1, 2, 3]); var [a] = it; var [b] = it; R = log.join()");
L("var it = [1, 2, 3][Symbol.iterator](); var [a] = it; var [b] = it; R = a + ':' + b");
L("var it = [1, 2, 3][Symbol.iterator](); var [a, ...r] = it; var [b] = it; R = a + ':' + r.length + ':' + typeof b");
L("var a = { [Symbol.iterator]() { log.push('i1'); return { next() { return { done: true } } } } }; var [x] = a, [y] = a; R = log.join()");
L("var [a] = { [Symbol.iterator]() { log.push('i'); return { next() { log.push('n'); return { done: false, get value() { log.push('v'); return 1 } } } , return() { log.push('r'); return {} } } } }; R = log.join()");
L("var [a, b] = { [Symbol.iterator]() { return { next() { log.push('n'); return { done: true, get value() { log.push('v'); return 1 } } } } } }; R = log.join()");
L("var [a] = { [Symbol.iterator]() { return { next() { log.push('n'); return { get done() { log.push('d'); return false }, get value() { log.push('v'); return 1 } } } , return() { log.push('r'); return {} } } } }; R = log.join()");
L("var [a, b] = { [Symbol.iterator]() { return { next() { log.push('n'); return { get done() { log.push('d'); return true }, get value() { log.push('v'); return 1 } } } } } }; R = log.join()");
L("var [a, ...r] = { [Symbol.iterator]() { var n = 0; return { next() { log.push('n'); return { get done() { log.push('d'); return n++ > 1 }, get value() { log.push('v'); return n } } } } } }; R = log.join()");
L("var next = 0; var [a, b] = { [Symbol.iterator]() { return { get next() { log.push('getnext'); return () => ({ done: next++ > 3, value: next }) } } } }; R = log.join()");
L("var [a] = { get [Symbol.iterator]() { log.push('geti'); return function () { return [1][Symbol.iterator]() } } }; R = log.join()");
L("var [a, b] = { get [Symbol.iterator]() { log.push('geti'); return function () { return [1, 2][Symbol.iterator]() } } }; R = log.join()");
L("var o = { return() { log.push('r'); return {} }, next() { log.push('n'); return { done: false, value: 1 } }, [Symbol.iterator]() { return this } }; var [a, b, c] = o; R = log.join()");
L("var o = { return() { log.push('r'); return {} }, next() { log.push('n'); return { done: false, value: 1 } }, [Symbol.iterator]() { return this } }; var [a, ...b] = o; R = 'ñ'");

// ---- Ordem de avaliação em assignment patterns.
L("var o = {}; function t(n) { log.push(n); return o } [t('a').x, t('b').y] = [1, 2]; R = log.join()");
L("var o = {}; function t(n) { log.push(n); return o } ({ a: t('a').x, b: t('b').y } = { a: 1, b: 2 }); R = log.join()");
L("var o = {}; function t(n) { log.push(n); return o } function k(n) { log.push(n); return n } ({ [k('ka')]: t('ta').x, [k('kb')]: t('tb').y } = { ka: 1, kb: 2 }); R = log.join()");
L("var o = { set x(v) { log.push('setx' + v) }, set y(v) { log.push('sety' + v) } }; [o.x, o.y] = [1, 2]; R = log.join()");
L("var o = { set x(v) { log.push('setx' + v) } }; [o.x = (log.push('d'), 5)] = []; R = log.join()");
L("var o = { set x(v) { log.push('setx' + v) } }; [o.x = (log.push('d'), 5)] = [7]; R = log.join()");
L("var o = {}; var i = 0; [o[i++], o[i++]] = ['a', 'b']; R = " + J + "(o) + i");
L("var o = {}; var i = 0; ({ a: o[i++], b: o[i++] } = { a: 'a', b: 'b' }); R = " + J + "(o) + i");
L("var a = [0, 0]; var i = 0; [a[i++], a[i++]] = [i, i]; R = " + J + "(a)");
L("var o = {}; function t() { log.push('t'); return null } try { [t().x] = [1] } catch (e) { R = e.name + ':' + log.join() }");
L("var o = {}; function t() { log.push('t'); return null } try { [t().x] = mk([1]) } catch (e) { R = e.name + ':' + log.join() }");
L("var x; try { ({ a: x } = null) } catch (e) { R = e.name + ': ' + e.message }");
L("var x; try { ({ a: x } = undefined) } catch (e) { R = e.name + ': ' + e.message }");
L("var x; try { [x] = null } catch (e) { R = e.name + ': ' + e.message }");
L("var x; try { [x] = undefined } catch (e) { R = e.name + ': ' + e.message }");
L("var x; try { [x] = {} } catch (e) { R = e.name + ': ' + e.message }");
L("var x; try { [x] = 5 } catch (e) { R = e.name + ': ' + e.message }");
L("var x; var r = ([x] = [1, 2]); R = r.length");
L("var x; var r = ({ x } = { x: 1, y: 2 }); R = " + J + "(r)");
L("var x, y; var r = ([x, y] = [y, x] = [1, 2]); R = x + ':' + y + ':' + r.length");
L("var a = 1, b = 2; [a, b] = [b, a]; R = a + ':' + b");
L("var a = [1, 2]; [a[0], a[1]] = [a[1], a[0]]; R = a.join()");
L("var a = 1, b = 2, c = 3; [a, b, c] = [c, a, b]; R = a + ':' + b + ':' + c");
L("var o = { a: 1, b: 2 }; ({ a: o.b, b: o.a } = o); R = o.a + ':' + o.b");
L("var x, y; ({ x, y = x } = { x: 1 }); R = x + ':' + y");
L("var x, y; [x = 1, y = x] = []; R = x + ':' + y");
L("var a; ({ a } = { a: 1 }); R = a");
L("var a; ({ a } = {}); R = typeof a");
L("var a = 5; ({ a } = {}); R = typeof a");
L("var a = 5; ({ a = 6 } = {}); R = a");
L("var a, b; ({ a, ...b } = { a: 1, c: 3 }); R = " + J + "([a, b])");
L("var a, b; [a, ...b] = [1, 2, 3]; R = " + J + "([a, b])");
L("var o = {}; [...o.r] = [1, 2]; R = " + J + "(o.r)");
L("var o = {}; [...o['r']] = [1, 2]; R = " + J + "(o.r)");
L("var o = {}; ({ ...o.r } = { a: 1 }); R = " + J + "(o.r)");
L("var o = {}; [...[o.a, o.b]] = [1, 2]; R = " + J + "(o)");
L("var o = {}; ({ ...{ length: o.n } } = { a: 1, b: 2 }); R = o.n");
L("var a, b; [{ a }, [b]] = [{ a: 1 }, [2]]; R = a + b");
L("var a, b; ({ x: [a], y: { b } } = { x: [1], y: { b: 2 } }); R = a + b");
L("var a; [a] = [1]; [a] = [a + 1]; R = a");
L("var a, b; [a, b] = 'xy'; R = a + b");
L("var a; ({ length: a } = 'abc'); R = a");
L("var a; ({ 0: a } = 'abc'); R = a");
L("var a; [(a)] = [1]; R = a");
L("var a; ({ a: (a) } = { a: 2 }); R = a");
L("var o = {}; [(o.a)] = [1]; R = o.a");
L("var o = {}; [((o.a))] = [1]; R = o.a");
L("let a; [a] = [1]; R = a");
L("let a; try { [b] = [1]; let b } catch (e) { R = e.name }");
L("const c = 1; try { [c] = [2] } catch (e) { R = e.name + ': ' + e.message }");
L("const c = 1; try { ({ c } = { c: 2 }) } catch (e) { R = e.name + ': ' + e.message }");
L("const c = 1; try { [...c] = [2] } catch (e) { R = e.name + ': ' + e.message }");
L("'use strict'; try { [undeclaredVar1] = [1] } catch (e) { R = e.name + ': ' + e.message }");
L("'use strict'; try { ({ a: undeclaredVar2 } = { a: 1 }) } catch (e) { R = e.name + ': ' + e.message }");
L("[undeclaredVar3] = [1]; R = typeof undeclaredVar3 + delete globalThis.undeclaredVar3");
L("({ a: undeclaredVar4 } = { a: 1 }); R = undeclaredVar4 + delete globalThis.undeclaredVar4");
L("var o = Object.freeze({ x: 1 }); [o.x] = [2]; R = o.x");
L("'use strict'; var o = Object.freeze({ x: 1 }); try { [o.x] = [2] } catch (e) { R = e.name + ': ' + e.message }");
L("'use strict'; var o = Object.freeze({ x: 1 }); try { ({ a: o.x } = { a: 2 }) } catch (e) { R = e.name + ': ' + e.message }");
L("var o = { get x() { return 1 } }; [o.x] = [2]; R = o.x");
L("'use strict'; var o = { get x() { return 1 } }; try { [o.x] = [2] } catch (e) { R = e.name + ': ' + e.message }");

// ---- Parâmetros: defaults, rest, patterns e escopo.
T("function f(a, b = 2, c) { return [a, b, c] } R = " + J + "(f(1, undefined, 3))");
T("function f(a, b = 2) { return b } R = f(1, null)");
T("function f(a = 1, b) { return f.length } R = f()");
T("function f(a, b = 1, c) { return f.length } R = f()");
T("function f(a, ...r) { return f.length } R = f()");
T("function f({ a }, [b]) { return f.length } R = f({}, [])");
T("function f({ a } = {}, [b] = []) { return f.length } R = f()");
T("function f(a = 1) { return f.length } R = f()");
T("var f = (a, b = 1) => a; R = f.length");
T("var f = async (a, ...r) => a; R = f.length");
T("function f({ a, b }) { return a + b } R = f({ a: 1, b: 2 })");
T("function f({ a, b } = { a: 1, b: 2 }) { return a + b } R = f()");
T("function f({ a, b } = { a: 1, b: 2 }) { return a + b } R = f({ a: 5, b: 5 })");
T("function f({ a = 1, b = 2 } = {}) { return a + b } R = f()");
T("function f({ a = 1, b = 2 } = {}) { return a + b } R = f({ a: 10 })");
T("function f({ a = 1, b = 2 }) { return a + b } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f({ a = 1, b = 2 }) { return a + b } try { f(null) } catch (e) { R = e.name + ': ' + e.message }");
T("function f([a, b]) { return a + b } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f([a, b]) { return a + b } try { f(1) } catch (e) { R = e.name + ': ' + e.message }");
T("function f([a, b] = [1, 2]) { return a + b } R = f()");
T("function f([a, [b]] = [1, [2]]) { return a + b } R = f()");
T("function f([a, ...r]) { return r.length } R = f('abcd')");
T("function f(...[a, b]) { return a + b } R = f(1, 2)");
T("function f(...{ length }) { return length } R = f(1, 2, 3)");
T("function f(...r) { return r } R = " + J + "(f())");
T("function f(a, ...r) { return r } R = " + J + "(f(1, 2, 3))");
T("function f(...r) { return Array.isArray(r) } R = f()");
T("var f = (...r) => r.length; R = f(1, 2, 3)");
T("var f = ([a, b]) => a + b; R = f([1, 2])");
T("var f = ({ a }) => a; R = f({ a: 1 })");
T("var f = ({ a } = { a: 2 }) => a; R = f()");
T("var f = (a, { b }, [c]) => a + b + c; R = f(1, { b: 2 }, [3])");
T("var f = ({ a }, ...[b]) => a + b; R = f({ a: 1 }, 2)");
T("var f = async ({ a }) => a; f({ a: 4 }).then(v => { R = v })");
T("var f = async ({ a }) => a; f(null).catch(e => { R = e.name + ': ' + e.message })");
T("function* g({ a }) { yield a } R = g({ a: 3 }).next().value");
T("function* g({ a }) { yield a } try { g(null); R = 'lazy' } catch (e) { R = e.name + ': ' + e.message }");
T("async function f({ a }) { return a } var p = f(null); R = p instanceof Promise; p.catch(() => {})");
T("class K { m({ a }, [b] = [2]) { return a + b } } R = new K().m({ a: 1 })");
T("class K { constructor({ a }) { this.a = a } } R = new K({ a: 7 }).a");
T("class K { static m([a]) { return a } } R = K.m([8])");
T("class K { set s({ a }) { R = a } } new K().s = { a: 9 }");
T("class K { set s([a]) { R = a } } new K().s = [3]");
T("var o = { m({ a }) { return a }, set s({ a }) { R = a } }; o.s = { a: 6 }; R = R + o.m({ a: 1 })");
T("var f = function ({ a }, b = a) { return b }; R = f({ a: 4 })");
T("function f(a = 1, b = a + 1, c = b + 1) { return [a, b, c] } R = " + J + "(f())");
T("function f(a = 1, b = a + 1, c = b + 1) { return [a, b, c] } R = " + J + "(f(5))");
T("function f(a = 1, b = a + 1, c = b + 1) { return [a, b, c] } R = " + J + "(f(5, undefined, 0))");
T("function f({ a }, b = a) { return b } R = f({ a: 'x' })");
T("function f({ a, b = a }) { return b } R = f({ a: 'x' })");
T("function f([a, b = a]) { return b } R = f(['x'])");
T("function f(a, { b = a }) { return b } R = f('x', {})");
T("function f({ a = b }, b) { return a } try { f({}, 1) } catch (e) { R = e.name + ': ' + e.message }");
T("function f({ a = b }, b) { return a } R = f({ a: 1 }, 2)");
T("function f(a = (log = 1, 2)) { return a } var log; f(); R = log");
T("var n = 0; function f(a = n++) { return a } f(); f(1); f(); R = n");
T("var n = 0; function f(a = ++n, b = ++n) { return [a, b] } R = " + J + "(f()) + n");
T("var n = 0; function f(a = ++n) { return a } R = f() + f() + f()");
T("var x = 1; function f(a = x) { var x = 2; return a } R = f()");
T("var x = 1; function f(a = x) { x = 2; return a } R = f() + ':' + x");
T("var x = 1; function f(a = () => x) { var x = 2; return a() } R = f()");
T("var x = 1; function f(a = () => x) { x = 3; return a() } R = f()");
T("var x = 1; function f(a = () => x, x = 5) { return a() } R = f()");
T("function f(a = () => { a = 9 }) { a(); return a } R = f()");
T("function f(a = () => { a = 9 }) { var a; a(); return a } R = f()");
T("function f(a = () => { a = 9 }) { var a = 1; a(); return a } R = f()");
T("function f(a, b = () => { a = 9 }) { var a; b(); return a } R = f(1)");
T("function f(a, b = () => { a = 9 }) { b(); var a; return a } R = f(1)");
T("function f(a, b = () => a) { var a; a = 5; return b() } R = f(1)");
T("function f(a, b = () => a) { { var a = 5 } return [a, b()] } R = " + J + "(f(1))");
T("function f(a, b = () => a) { let c = 1; return b() } R = f(1)");
T("function f(a, b = () => c) { var c = 1; try { return b() } catch (e) { return e.name } } R = f(1)");
T("var c = 'g'; function f(a, b = () => c) { var c = 1; return b() } R = f(1)");
T("function f(a = this.v) { return a } R = f.call({ v: 'tv' })");
T("function f(a = new.target) { return typeof a } R = f() + typeof new f()");
T("function f(a = () => this.v) { return a() } R = f.call({ v: 'tv' })");
T("function f(a = arguments[1]) { return a } R = f(undefined, 'second')");
T("function f(a = arguments.length) { return a } R = f()");
T("function f(a = arguments.length) { return a } R = f(undefined)");
T("function f(a, b = arguments.length) { return b } R = f(1, undefined, 3)");
T("var f = (a = arguments) => a; R = typeof f()");
T("function f(a = eval('1 + 1')) { return a } R = f()");
T("function f(a = eval('var q = 5; q'), b = typeof q) { return [a, b] } R = " + J + "(f())");
T("function f(a = eval('b'), b = 1) { return a } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = f) { return a === f } R = f()");
T("var f = function g(a = g) { return a === g } R = f()");
T("var f = (a = f) => a; try { R = f() === f } catch (e) { R = e.name }");
T("let f = (a = f) => a; R = f() === f");
T("function f(a = typeof f2) { return a } var f2 = 1; R = f()");
T("function f(a = 1, a2 = a) { return a2 } R = f(undefined)");
T("function f(a = function () { return 1 }) { return a.name } R = f()");
T("function f(a = () => {}) { return a.name } R = f()");
T("function f(a = class {}) { return a.name } R = f()");
T("function f(a = class { static x = 1 }) { return a.x } R = f()");
T("function f({ a = function () {} }) { return a.name } R = f({})");
T("function f([a = () => {}]) { return a.name } R = f([])");
T("function f(a, b) { return a } R = f.length + ':' + f.name");
T("function f(a = 1, b) {} R = f.length");
T("function f(a, b = 1, c) {} R = f.length");
T("function f(a, { b }, [c], ...d) {} R = f.length");
T("function f(...a) {} R = f.length");
T("R = (function (a, b = 2, ...c) {}).length");
T("R = ((a, b) => {}).length + ':' + ((...a) => {}).length + ':' + ((a = 1) => {}).length");
T("R = new Function('a = 1', 'b', 'return [a, b]')(undefined, 2).join()");
T("R = new Function('{ a }', '[b]', 'return a + b')({ a: 1 }, [2])");
T("R = new Function('...r', 'return r.length')(1, 2, 3)");
T("R = new Function('a, b = a', 'return b')(3)");
T("R = Function('a = 1', 'return a').length");
T("try { new Function('a, a = 1', '') } catch (e) { R = e.name + ': ' + e.message }");
T("try { new Function('a = 1', '\"use strict\"') } catch (e) { R = e.name + ': ' + e.message }");
T("try { new Function('{ a }', '\"use strict\"') } catch (e) { R = e.name + ': ' + e.message }");
T("try { new Function('...a', '\"use strict\"') } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = 1) { 'use strict' }");
T("function f(a = 1) { 'use strict' } R = typeof f");
T("function f(a = 1) { 'use strict'; return this } R = typeof f.call(5)");
T("(function () { 'use strict'; function f(a = 1) { return this } R = typeof f.call(5) })()");

// ---- arguments mapeado vs não mapeado.
T("function f(a) { arguments[0] = 2; return a } R = f(1)");
T("function f(a) { a = 2; return arguments[0] } R = f(1)");
T("function f(a, b) { arguments[1] = 2; return b } R = f(1)");
T("function f(a, b) { b = 2; return arguments[1] } R = f(1)");
T("function f(a, b) { b = 2; return arguments.length } R = f(1)");
T("function f(a, b) { arguments[1] = 2; return arguments.length } R = f(1)");
T("function f(a, b) { arguments[2] = 2; return arguments.length } R = f(1)");
T("function f(a) { arguments.length = 0; a = 3; return arguments[0] } R = f(1)");
T("function f(a) { delete arguments[0]; a = 3; return arguments[0] } R = f(1)");
T("function f(a) { delete arguments[0]; arguments[0] = 4; return a } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { get() { return 'g' } }); return a } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { get() { return 'g' } }); a = 5; return arguments[0] } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { enumerable: false }); a = 5; return arguments[0] } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { writable: false }); arguments[0] = 7; return a } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { writable: false }); a = 7; return arguments[0] } R = f(1)");
T("function f(a) { Object.freeze(arguments); a = 7; return arguments[0] } R = f(1)");
T("function f(a) { Object.freeze(arguments); arguments[0] = 7; return a } R = f(1)");
T("function f(a) { Object.seal(arguments); a = 7; return arguments[0] } R = f(1)");
T("function f(a) { return Object.getOwnPropertyDescriptor(arguments, '0').writable } R = f(1)");
T("function f(a) { return " + J + "(Object.getOwnPropertyDescriptor(arguments, 'length')) } R = f(1, 2)");
T("function f(a) { return " + J + "(Object.getOwnPropertyDescriptor(arguments, 'callee') !== undefined) } R = f(1)");
T("function f(a) { 'use strict'; try { arguments.callee } catch (e) { return e.name + ': ' + e.message } } R = f(1)");
T("function f(a) { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set } R = f(1)");
T("function f(a = 1) { return Object.getOwnPropertyDescriptor(arguments, 'callee').get !== undefined } R = f()");
T("function f(a = 1) { try { return arguments.callee } catch (e) { return e.name } } R = f()");
T("function f({ a }) { try { return arguments.callee } catch (e) { return e.name } } R = f({})");
T("function f(...r) { try { return arguments.callee } catch (e) { return e.name } } R = f()");
T("function f(a) { 'use strict'; arguments[0] = 2; return a } R = f(1)");
T("function f(a) { 'use strict'; a = 2; return arguments[0] } R = f(1)");
T("function f(a = 0) { arguments[0] = 2; return a } R = f(1)");
T("function f(a = 0) { a = 2; return arguments[0] } R = f(1)");
T("function f({ a }) { arguments[0] = { a: 2 }; return a } R = f({ a: 1 })");
T("function f([a]) { a = 2; return arguments[0][0] } R = f([1])");
T("function f(a, ...r) { arguments[0] = 2; return a } R = f(1)");
T("function f(a, ...r) { a = 2; return arguments[0] } R = f(1)");
T("var f = function (a) { arguments[0] = 2; return a }; R = f(1)");
T("var o = { m(a) { arguments[0] = 2; return a } }; R = o.m(1)");
T("class K { m(a) { arguments[0] = 2; return a } } R = new K().m(1)");
T("class K { constructor(a) { arguments[0] = 2; this.a = a } } R = new K(1).a");
T("function f(a) { (() => { arguments[0] = 2 })(); return a } R = f(1)");
T("function f(a) { (function () { arguments[0] = 2 })(); return a } R = f(1)");
T("function f(a) { eval('arguments[0] = 2'); return a } R = f(1)");
T("function f(a) { eval('a = 2'); return arguments[0] } R = f(1)");
T("function f(a) { var g = () => { a = 2 }; g(); return arguments[0] } R = f(1)");
T("function f(a, a2) { arguments[0] = 'x'; arguments[1] = 'y'; return a + a2 } R = f(1, 2)");
T("function f(a, a) { arguments[0] = 'x'; return a } R = f(1, 2)");
T("function f(a, a) { arguments[1] = 'y'; return a } R = f(1, 2)");
T("function f(a, a) { a = 'z'; return arguments[0] + arguments[1] } R = f(1, 2)");
T("function f(a) { arguments[0] = 2; return arguments[0] } R = f()");
T("function f(a) { a = 2; return arguments.length } R = f()");
T("function f(a, b, c) { arguments[2] = 'c'; return c } R = f(1, 2)");
T("function f(a, b, c) { arguments[2] = 'c'; return c } R = f(1, 2, 3)");
T("function f(a) { return Array.prototype.slice.call(arguments).join() } R = f(1, 2, 3)");
T("function f(a) { a = 9; return Array.prototype.slice.call(arguments).join() } R = f(1, 2, 3)");
T("function f(a) { return [...arguments].join() } R = f(1, 2)");
T("function f(a) { var [x, y] = arguments; return x + y } R = f(1, 2)");
T("function f(a) { var { length } = arguments; return length } R = f(1, 2)");
T("function f(a) { var { 0: x } = arguments; a = 7; return x } R = f(1)");
T("function f(a) { a = 7; var { 0: x } = arguments; return x } R = f(1)");
T("function f(a) { return Object.keys(arguments).join() } R = f(1, 2)");
T("function f(a) { return Object.getOwnPropertyNames(arguments).join() } R = f(1, 2)");
T("function f(a) { return Object.getOwnPropertyNames(arguments).join() } R = f()");
T("function f(a) { return Reflect.ownKeys(arguments).map(String).join() } R = f(1)");
T("function f(a) { return arguments[Symbol.iterator] === Array.prototype.values } R = f()");
T("function f(a) { return typeof arguments[Symbol.toStringTag] + Object.prototype.toString.call(arguments) } R = f()");
T("function f() { return arguments.constructor === Object } R = f()");
T("function f() { return Object.getPrototypeOf(arguments) === Object.prototype } R = f()");
T("function f() { return arguments instanceof Array } R = f()");
T("function f() { return JSON.stringify(arguments) } R = f(1, 'a')");
T("function f() { arguments.length = 5; return Array.prototype.slice.call(arguments).length } R = f(1)");
T("function f(a) { arguments.length = 0; return [...arguments].length } R = f(1)");
T("function f(a) { var arguments; return typeof arguments } R = f(1)");
T("function f(a) { var arguments = 1; return typeof a } R = f(2)");
T("function f(a) { var arguments = 1; a = 5; return arguments } R = f(2)");
T("function f(a) { function arguments() {} a = 5; return typeof arguments } R = f(2)");
T("function f(a, arguments) { return typeof arguments } R = f(1, 2)");
T("function f(arguments, a) { arguments = 5; return a } R = f(1, 2)");
T("function f(a = arguments) { var arguments = 3; return a === arguments } R = f()");
T("function f(a = arguments[0]) { var arguments = 3; return a } R = f(undefined)");
T("function f(a, b = arguments[0]) { a = 'changed'; return b } R = f('orig')");
T("function f(a, b = () => arguments[0]) { a = 'changed'; return b() } R = f('orig')");
T("function f(a = 1, b = () => arguments[0]) { arguments[0] = 'x'; return a } R = f()");
T("function f(a = 1, b = () => arguments[0]) { arguments[0] = 'x'; return b() } R = f('y')");
T("var f = (a) => { return typeof arguments }; R = f(1)");
T("function outer() { var f = (a) => { a = 2; return arguments[0] }; return f(1) } R = outer(9)");
T("function outer(a) { var f = () => { a = 2; return arguments[0] }; return f() } R = outer(9)");
T("function outer() { var f = () => arguments.length; return f(1, 2, 3) } R = outer(1)");
T("function outer() { return (() => (() => arguments[0])())() } R = outer('deep')");
T("function f() { return typeof arguments === 'object' && arguments.length } R = f(...[1, 2, 3])");
T("function f() { return arguments.length } R = f.apply(null, { length: 3 })");
T("function f() { return arguments.length } R = f.call(null, 1, 2)");
T("function f() { return arguments.length } R = Reflect.apply(f, null, [1, 2, 3, 4])");
T("function f() { return arguments.length } R = new f() instanceof f");
T("function f() { return arguments.length } R = f.bind(null, 1)(2, 3)");
T("function f(a) { return arguments.length } R = f.bind(null, 1)()");
T("function f(a, b) { arguments[0] = 'x'; return b } R = f.bind(null, 1)(2)");

// ---- TDZ de parâmetros.
T("function f(a = b, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b, b = 1) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b, b) { return a } R = f(1)");
T("function f(a = b, b) { return a } R = f(undefined, 2) === undefined");
T("function f(a = a) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = a) { return a } R = f(7)");
T("function f({ a = b }, b) {} try { f({}, 1) } catch (e) { R = e.name + ': ' + e.message }");
T("function f([a = b], b) {} try { f([]) } catch (e) { R = e.name + ': ' + e.message }");
T("function f({ a = a }) {} try { f({}) } catch (e) { R = e.name + ': ' + e.message }");
T("function f({ a = b, b }) {} try { f({}) } catch (e) { R = e.name + ': ' + e.message }");
T("function f({ a = b, b }) { return a } R = f({ a: 1 })");
T("function f([a = b, b]) {} try { f([]) } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = () => b, b) { return a() } R = f()");
T("function f(a = () => b, b = 3) { return a() } R = f()");
T("function f(a = (() => b)(), b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = typeof b, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = typeof c, b) { return a } R = f()");
T("function f(a = eval('b'), b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b = 1, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b++, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = [b], b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = { b }, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = `${b}`, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b.x, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b(), b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("var f = (a = b, b) => {}; try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("var f = ({ a = b }, b) => {}; try { f({}) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { m(a = b, b) {} } try { new K().m() } catch (e) { R = e.name + ': ' + e.message }");
T("class K { constructor(a = b, b) {} } try { new K() } catch (e) { R = e.name + ': ' + e.message }");
T("class K { static m(a = b, b) {} } try { K.m() } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { m(a = b, b) {} }; try { o.m() } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { set s(a = b) {} }; R = 'x'");
T("function* g(a = b, b) {} try { g(); R = 'lazy' } catch (e) { R = e.name + ': ' + e.message }");
T("async function f(a = b, b) {} var p = f(); p.catch(e => { R = e.name + ': ' + e.message })");
T("async function* g(a = b, b) {} try { g(); R = 'lazy' } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a, b = a, c = b) { return [a, b, c] } R = " + J + "(f(1))");
T("function f(a, b = c, c = 1) { return b } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a, b = c, c = 1) { return b } R = f(0, 5)");
T("function f(a = b, b = a) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = b, b = a) { return [a, b] } R = " + J + "(f(1))");
T("function f(a = b, b = a) { return [a, b] } R = " + J + "(f(undefined, 2))");
T("function f(a = 1, b = a) { return b } R = f()");
T("function f(a = () => a) { return a() === a } R = f()");
T("function f(a = () => b, b = a()) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("let tdzOuter = 1; function f(a = tdzOuter) { let tdzOuter = 2; return a } R = f()");
T("function f(a = tdzLater) { return a } try { f() } catch (e) { R = e.name + ': ' + e.message } let tdzLater = 1");
T("function f(a = tdzLater2) { return a } let tdzLater2 = 4; R = f()");

// ---- for-of e for-in com patterns.
T("var r = []; for (var [a, b] of [[1, 2], [3, 4]]) r.push(a + b); R = r.join()");
T("var r = []; for (let [a, b] of [[1, 2], [3, 4]]) r.push(a + b); R = r.join()");
T("var r = []; for (const [a, b] of [[1, 2], [3, 4]]) r.push(a * b); R = r.join()");
T("var r = []; for (var { a, b } of [{ a: 1, b: 2 }, { a: 3, b: 4 }]) r.push(a + b); R = r.join()");
T("var r = []; for (let { a, b = 5 } of [{ a: 1 }, { a: 3, b: 4 }]) r.push(a + b); R = r.join()");
T("var r = []; for (const { a: { b } } of [{ a: { b: 1 } }, { a: { b: 2 } }]) r.push(b); R = r.join()");
T("var r = []; for (const [k, v] of Object.entries({ x: 1, y: 2 })) r.push(k + v); R = r.join()");
T("var r = []; for (const [k, v] of new Map([['a', 1], ['b', 2]])) r.push(k + v); R = r.join()");
T("var r = []; for (const [i, v] of ['x', 'y'].entries()) r.push(i + v); R = r.join()");
T("var r = []; for (const { length } of ['a', 'bb']) r.push(length); R = r.join()");
T("var r = []; for (const [c] of 'ab') r.push(c); R = r.join()");
T("var r = []; for (const [a, ...rest] of [[1, 2, 3]]) r.push(rest.length); R = r.join()");
T("var r = []; for (const { a, ...rest } of [{ a: 1, b: 2, c: 3 }]) r.push(" + J + "(rest)); R = r.join()");
T("var a, b, r = []; for ([a, b] of [[1, 2], [3, 4]]) r.push(a + b); R = r.join() + a + b");
T("var a, b, r = []; for ({ a, b } of [{ a: 1, b: 2 }]) r.push(a + b); R = r.join() + a + b");
T("var o = {}, r = []; for ([o.x, o.y] of [[1, 2], [3, 4]]) r.push(o.x + o.y); R = r.join()");
T("var o = {}, r = []; for ({ a: o.x } of [{ a: 1 }, { a: 2 }]) r.push(o.x); R = r.join()");
T("var a, r = []; for ([a = 9] of [[], [1]]) r.push(a); R = r.join()");
T("var a, r = []; for ({ a = 9 } of [{}, { a: 1 }]) r.push(a); R = r.join()");
T("var r = []; for (var [a = 9] of [[], [1]]) r.push(a); R = r.join()");
T("var r = []; for (let [a = 9] of [[], [1]]) r.push(a); R = r.join()");
T("for (let [a] of [null]) {}");
T("for (let { a } of [null]) {}");
T("for (let [a] of [undefined]) {}");
T("for (let { a } of [undefined]) {}");
T("for (let [a] of [1]) {}");
T("for (let [a] of [{}]) {}");
T("for (let { a } of [1]) R = typeof a");
T("for (var { a } of [null]) {}");
T("var a; for ([a] of [null]) {}");
T("var a; for ({ a } of [null]) {}");
T("var fs = []; for (let [a] of [[1], [2]]) fs.push(() => a); R = fs.map(f => f()).join()");
T("var fs = []; for (let { a } of [{ a: 1 }, { a: 2 }]) fs.push(() => a); R = fs.map(f => f()).join()");
T("var fs = []; for (var [a] of [[1], [2]]) fs.push(() => a); R = fs.map(f => f()).join()");
T("var fs = []; for (let [a, b = () => a] of [[1], [2]]) fs.push(b); R = fs.map(f => f()).join()");
T("var fs = []; for (let [a, b = () => a] of [[1], [2]]) { a = 'm'; fs.push(b) } R = fs.map(f => f()).join()");
T("var r = []; for (let [a, b = a] of [[1], [2, 3]]) r.push(b); R = r.join()");
T("for (let [a = b, b] of [[]]) {}");
T("for (let { a = b, b } of [{}]) {}");
T("for (let [a = a] of [[]]) {}");
T("var r; for (let [a = (r = typeof a)] of [[]]) {}");
T("for (const [a] of [[1]]) { a = 2 }");
T("for (const { a } of [{ a: 1 }]) { a = 2 }");
T("for (let [a, a2] of [[1, 2]]) { var a3 = a + a2; R = a3 }");
T("var r = []; for (let [a, b] of [[1, 2]]) { let c = a; r.push(c, b) } R = r.join()");
T("var r = []; for (let { a } of [{ a: 1 }]) { let a2 = a; r.push(a2) } R = r.join()");
T("for (let [a] of [[1]]) { let a }");
T("for (let [a] of [[1]]) { var a }");
L("for (var [a] of [mk([1, 2])]) { log.push('body') } R = log.join()");
L("for (var [a, b] of [mk([1])]) { log.push('body') } R = log.join()");
L("for (var [a] of [mk([1, 2])]) { break } R = log.join()");
L("for (var [a] of mk([[1], [2]])) { log.push('b' + a) } R = log.join()");
L("for (var [a] of mk([[1], [2]])) { break } R = log.join()");
L("for (var [a] of mk([[1], [2]])) { throw new Error('body') } ");
L("for (var [a] of mk([[1], [2]])) { continue } R = log.join()");
L("outer: for (var x of mk([1, 2])) { for (var [a] of mk([[1]])) { continue outer } } R = log.join()");
L("for (var [a] of mk([null])) {} ");
L("for (var [a] of mk([[1]], { retThrow: true })) { break } ");
L("for (var [a] of mk([[1]], { retThrow: true })) { throw new Error('body') } ");
L("for (var { a } of mk([null])) {} ");
L("for ([a] of mk([null])) {} ");
L("for (var [a] of mk([[1]], { throwAt: 0 })) {} ");
L("var f = function* () { for (var [a] of mk([[1], [2]])) yield a }; var it = f(); it.next(); it.return(); R = log.join()");
L("var f = function* () { for (var [a] of mk([[1], [2]])) yield a }; var it = f(); it.next(); try { it.throw(new Error('t')) } catch (e) { R = e.message + log.join() }");
T("var r = []; for (var [a, b] in { xy: 1, zw: 2 }) r.push(a + b); R = r.join()");
T("var r = []; for (let [a, b] in { xy: 1, zw: 2 }) r.push(a + b); R = r.join()");
T("var r = []; for (const [a] in { xy: 1, zw: 2 }) r.push(a); R = r.join()");
T("var r = []; for (const { length } in { x: 1, yyy: 2 }) r.push(length); R = r.join()");
T("var r = []; for (const [a, ...b] in { xyz: 1 }) r.push(a + b.join('')); R = r.join()");
T("var r = []; for (let { 0: a } in { x: 1 }) r.push(a); R = r.join()");
T("var a, b, r = []; for ([a, b] in { xy: 1 }) r.push(a + b); R = r.join()");
T("var a, r = []; for ({ length: a } in { xyz: 1 }) r.push(a); R = r.join()");
T("var a, r = []; for ([a = 'd'] in { '': 1 }) r.push(a); R = r.join()");
T("var o = {}, r = []; for ([o.a, o.b] in { xy: 1 }) r.push(o.a + o.b); R = r.join()");
T("var fs = []; for (let [a] in { x: 1, y: 2 }) fs.push(() => a); R = fs.map(f => f()).join()");
T("for (let [a] in { 1: 1 }) { R = a }");
T("for (let { a } in { x: 1 }) { R = typeof a }");
T("for (var { a = 5 } in { x: 1 }) { R = a }");
T("for (let [a = b, b] in { x: 1 }) {}");
T("var s = Symbol('k'); var r = []; for (var [a] in { [s]: 1, ab: 2 }) r.push(a); R = r.join()");
T("var r = []; for (var [a, b] in [1, 2]) r.push(a + ':' + b); R = r.join()");
T("var r = []; for (var [a, b] in 'ab') r.push(a); R = r.join()");
T("for (var [a] in null) {} R = 'ok'");
T("for (var { a } in undefined) {} R = 'ok'");
T("var r = []; for (var x in { a: 1 }) { var [y] = x; r.push(y) } R = r.join()");

// ---- catch com pattern.
T("try { throw [1, 2] } catch ([a, b]) { R = a + b }");
T("try { throw { a: 1, b: 2 } } catch ({ a, b }) { R = a + b }");
T("try { throw { a: { b: 3 } } } catch ({ a: { b } }) { R = b }");
T("try { throw [[1]] } catch ([[a]]) { R = a }");
T("try { throw [] } catch ([a = 5]) { R = a }");
T("try { throw {} } catch ({ a = 6 }) { R = a }");
T("try { throw [1, 2, 3] } catch ([a, ...r]) { R = r.length }");
T("try { throw { a: 1, b: 2 } } catch ({ a, ...r }) { R = " + J + "(r) }");
T("try { throw 'str' } catch ([a, b]) { R = a + b }");
T("try { throw 'str' } catch ({ length }) { R = length }");
T("try { throw 1 } catch ({ toFixed }) { R = typeof toFixed }");
T("try { throw null } catch ({ a }) { R = 'x' }");
T("try { throw undefined } catch ({ a }) { R = 'x' }");
T("try { throw null } catch ([a]) { R = 'x' }");
T("try { throw 1 } catch ([a]) { R = 'x' }");
T("try { throw {} } catch ([a]) { R = 'x' }");
T("try { throw new Error('m') } catch ({ message, name }) { R = name + message }");
T("try { null.x } catch ({ message }) { R = message }");
T("try { throw new TypeError('t') } catch ({ constructor: { name } }) { R = name }");
T("try { throw { a: 1 } } catch ({ a }) { try { throw { a: a + 1 } } catch ({ a }) { R = a } }");
T("try { throw [1] } catch ([a]) { try { throw [a + 1] } catch ([b]) { R = a + b } }");
T("var a = 'outer'; try { throw [1] } catch ([a]) { } R = a");
T("var a = 'outer'; try { throw [1] } catch ([a]) { a = 2 } R = a");
T("try { throw [1] } catch ([a]) { var f = () => a } R = f()");
T("var fs = []; for (var i = 0; i < 2; i++) { try { throw [i] } catch ([a]) { fs.push(() => a) } } R = fs.map(f => f()).join()");
T("try { throw [1] } catch ([a]) { { let a = 2; R = a } }");
T("try { throw [1] } catch ([a]) { { var b = a } } R = b");
T("try { throw [1] } catch ([a]) { eval('var a = 9'); R = a }");
T("try { throw [1] } catch ([a]) { eval('var c = a') } R = c");
T("try { throw {} } catch ({ a = b, b }) { }");
T("try { throw {} } catch ({ a = 1, b = a }) { R = b }");
T("try { throw {} } catch ({ a = a }) { }");
T("try { throw [] } catch ([a = () => a]) { R = a() === a }");
T("try { throw { a: 1 } } catch ({ a: [b] }) { R = b }");
T("try { throw 1 } catch ({ a: [b] }) { R = b }");
L("try { throw mk([1, 2]) } catch ([a, b]) { R = log.join() + a + b }");
L("try { throw mk([1, 2]) } catch ([a]) { R = log.join() + a }");
L("try { throw mk([1], { throwAt: 0 }) } catch ([a]) { R = 'x' }");
L("try { throw mk([1]) } catch ([...r]) { R = log.join() + r.length }");
T("function f() { try { throw [1] } catch ([a]) { return a } finally { R = 'fin' } } R = f() + R");
T("function f() { try { throw {} } catch ({ a = 3 }) { return a } } R = f()");
T("var r = (() => { try { throw [4] } catch ([a]) { return () => a } })(); R = r()");
T("(function () { try { throw [1] } catch ([a]) { throw [a + 1] } })()");
T("try { try { throw [1] } catch ([a]) { throw [a + 1] } } catch ([b]) { R = b }");
T("try { throw [1] } catch ([a]) { } finally { R = typeof a }");
T("class K { m() { try { throw { v: 1 } } catch ({ v }) { return v + this.n } } n = 2 } R = new K().m()");

// ---- Misturas dentro de funções, closures e classes.
T("function f() { var [a, b] = [1, 2]; var { c, d } = { c: 3, d: 4 }; return a + b + c + d } R = f()");
T("function f() { let [a, b] = [1, 2]; { let [a, b] = [3, 4]; return a + b } } R = f()");
T("function f() { var [a] = [1]; function g() { return a } a = 2; return g() } R = f()");
T("function f() { const [a, b] = [1, 2]; return () => a + b } R = f()()");
T("function f() { let { a, b } = { a: 1, b: 2 }; return () => { a++; return a + b } } R = f()()");
T("function f() { return typeof a; let [a] = [1] } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f() { var [a, b] = [b, 1]; return a } R = f()");
T("function f() { let [a, b] = [b, 1]; return a } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f() { let { a, b } = { a: b, b: 1 }; return a } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f() { let [a = b, b = 1] = []; return a } try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f() { var x = 1; var [x] = [2]; return x } R = f()");
T("function f(x) { var [x] = [2]; return x } R = f(1)");
T("function f(x) { var [x] = []; return x } R = f(1)");
T("function f(x) { var { x } = {}; return x } R = f(1)");
T("function f(x) { var { y: x } = { y: 3 }; return arguments[0] } R = f(1)");
T("function f(x) { var [x] = [3]; return arguments[0] } R = f(1)");
T("function f(x = 0) { var [x] = [3]; return arguments[0] } R = f(1)");
T("function f(x = 0) { var [x] = []; return x } R = f(1)");
T("function f(x = 0) { var x; return x } R = f(1)");
T("function f(x = 0) { var [x] = [undefined]; return x } R = f()");
T("function f({ x }) { var { x } = { x: 'inner' }; return x } R = f({ x: 'outer' })");
T("function f({ x }) { var { x } = {}; return x } R = f({ x: 'outer' })");
T("function f([x], g = () => x) { var [x] = ['body']; return g() } R = f(['param'])");
T("function f([x], g = () => x) { var [x] = []; return x + g() } R = f(['param'])");
T("class K { #p = 1; m({ a = this.#p }) { return a } } R = new K().m({})");
T("class K { constructor({ a, ...r }) { Object.assign(this, r); this.a = a } } R = " + J + "(new K({ a: 1, b: 2 }))");
T("class K { static [(() => { var { a } = { a: 'k' }; return a })()] = 1 } R = Object.keys(K).join()");
T("class K { x = (() => { var [a] = [5]; return a })() } R = new K().x");
T("class K { static { var [a, b] = [1, 2]; K.s = a + b } } R = K.s");
T("class K { static { let { a } = { a: 3 }; K.s = () => a } } R = K.s()");
T("var o = { m() { var [a] = [1]; return a }, get g() { var { b } = { b: 2 }; return b } }; R = o.m() + o.g");
T("function* g() { var [a, b] = yield; yield a + b } var it = g(); it.next(); R = it.next([1, 2]).value");
T("function* g() { var { a } = yield 1; yield a } var it = g(); it.next(); R = it.next({ a: 'g' }).value");
T("function* g(...[a, b]) { yield a + b } R = g(1, 2).next().value");
T("function* g() { var [a = yield 'dflt'] = []; return a } var it = g(); var first = it.next().value; R = first + it.next('v').value");
T("function* g() { var { [yield 'key']: v } = { k: 'val' }; return v } var it = g(); it.next(); R = it.next('k').value");
T("function* g() { var [a, b] = [yield 1, yield 2]; return a + b } var it = g(); it.next(); it.next(10); R = it.next(20).value");
T("async function f() { var [a, b] = await Promise.all([1, 2]); return a + b } f().then(v => { R = v })");
T("async function f() { var { a = await Promise.resolve(7) } = {}; return a } f().then(v => { R = v })");
T("async function f() { var [a = await 1, b = await 2] = []; return a + b } f().then(v => { R = v })");
T("async function f() { var { [await 'k']: v } = { k: 'ak' }; return v } f().then(v => { R = v })");
T("async function f() { try { var [a] = await null } catch (e) { return e.name } } f().then(v => { R = v })");
T("async function f([a, b] = [1, 2]) { await null; return a + b } f().then(v => { R = v })");
T("async function f() { for await (var [a, b] of [[1, 2], Promise.resolve([3, 4])]) R = (R || 0) + a + b } f()");
T("async function f() { var r = []; for await (const { a } of [{ a: 1 }, { a: 2 }]) r.push(a); return r.join() } f().then(v => { R = v })");
T("async function* g() { var [a] = yield 1; yield a } var it = g(); it.next().then(() => it.next([5])).then(r => { R = r.value })");
T("var f = async ([a]) => a; f(null).catch(e => { R = e.name })");
T("var it = [[1, 2]][Symbol.iterator](); var [[a, b]] = it; R = a + b");
T("var arr = [1, 2, 3]; var [a, ...rest] = arr; arr.length = 0; R = a + ':' + rest.length");
T("var [a, b] = [1, 2]; var [c, d] = [b, a]; R = c + d");
T("var obj = { arr: [1, { deep: [2, 3] }] }; var { arr: [one, { deep: [two, three] }] } = obj; R = one + two + three");
T("var { a: { b: { c: { d: { e } } } } } = { a: { b: { c: { d: { e: 'e' } } } } }; R = e");
T("var [[[[[x]]]]] = [[[[['x']]]]]; R = x");
T("var { a: { a: { a } } } = { a: { a: { a: 1 } } }; R = a");
T("var [{ a: [{ b }] }] = [{ a: [{ b: 'ok' }] }]; R = b");
T("var { x = 1, y = 2, z = 3 } = { y: null }; R = " + J + "([x, y, z])");
T("var { [Symbol.toPrimitive]: tp } = Symbol('s'); R = typeof tp");
T("var { a, b, c, d, e, f, g, h, i, j } = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10 }; R = a + b + c + d + e + f + g + h + i + j");
T("var [a, b, c, d, e, f, g, h, i, j] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]; R = a + b + c + d + e + f + g + h + i + j");
T("var [a, b, c, d, e, f, g, h, i, j, ...r] = Array.from({ length: 20 }, (_, i) => i); R = r.length");
T("var { a, ...r } = Object.fromEntries(Array.from({ length: 50 }, (_, i) => ['k' + i, i])); R = Object.keys(r).length");

// ---- SyntaxError em patterns e parâmetros.
const syntaxErrors = [
  "var [a] ", "var { a }", "let [a]", "const { a }", "var [a, ...b,] = []", "var [...a, b] = []", "var [...a = []] = []",
  "var { ...a, b } = {}", "var { ...{ a } } = {}", "var { ...[a] } = {}", "var { ...a, } = {}", "var { a: 1 } = {}", "var [1] = []",
  "var [a.b] = []", "var { a: b.c } = {}", "var [(a)] = []", "var { a: (b) } = {}", "var [...a.b] = []", "var { 'a' } = {}",
  "var { 1 } = {}", "var { [a] } = {}", "var { a b } = {}", "var { , a } = {}", "var [a b] = []", "var [,,,", "var { a:",
  "[a] = ", "[a.b = 1] = []", "[a, ...b, c] = []", "[...a,] = []", "({ a: 1 } = {})", "({ a() {} } = {})", "({ get a() {} } = {})",
  "({ a: b() } = {})", "[a()] = []", "[a + 1] = []", "[...a()] = []", "({ ...a() } = {})", "({ ...(a) } = {})", "({ ...{ b } } = {})",
  "({ ...[b] } = {})", "[(a = 1)] = []", "[([a])] = []", "[({ a })] = []", "({ a: ([b]) } = {})", "({ a: (b = 1) } = {})", "(a, b) = 1",
  "([a]) = 1", "({ a }) = 1", "[a] += 1", "[a]++", "({ a })++", "for ([a] = 1 of []);", "for (var [a] = 1 of []);", "for (let [a] = 1 of []);",
  "for (var [a] = 1 in {});", "for (let { a } = 1 in {});", "for (var [a], b of []);", "for (let [a], [b] of []);", "for ([a] = [] of []);",
  "for (const [a];;);", "for (const { a };;);", "for (let [a];;);", "for (let [a, a] of []);", "for (let { a, b: a } of []);",
  "let [a, a] = []", "const { a, b: a } = {}", "let [a, ...a] = []", "let { a, ...a } = {}", "let [a] = [], a", "let { a } = {}; let [a] = []",
  "function f([a], a) {}", "function f({ a }, a) {}", "function f({ a, b: a }) {}", "function f([a, a]) {}", "function f(a, ...a) {}",
  "function f(...a,) {}", "function f(...a, b) {}", "function f(...a = []) {}", "function f(...[a] = []) {}", "function f(a,, b) {}",
  "function f(a = 1, ...a) {}", "function f(a = 1) { 'use strict' }", "function f({ a }) { 'use strict' }", "function f([a]) { 'use strict' }",
  "function f(...a) { 'use strict' }", "(a = 1) => { 'use strict' }", "({ a }) => { 'use strict' }", "(...a) => { 'use strict' }",
  "({ m(a = 1) { 'use strict' } })", "class A { m({ a }) { 'use strict' } }", "(a, ...b,) => 1", "(...a, b) => 1", "(a, a) => 1",
  "([a], a) => 1", "({ a }, a) => 1", "(a, [a]) => 1", "(a = 1, a) => 1", "({ a }, { a }) => 1", "([a, a]) => 1", "({ a, b: a }) => 1",
  "(a, { b: a }) => 1", "async (a, a) => 1", "async ([a], a) => 1", "async ({ a }, { a }) => 1", "({ a: 1 }) => 1", "([1]) => 1",
  "({ a: b.c }) => 1", "([a.b]) => 1", "([(a)]) => 1", "(({ a })) => 1", "(([a])) => 1", "(a, (b)) => 1", "((a), b) => 1", "(...(a)) => 1",
  "([...a,]) => 1", "([...a, b]) => 1", "({ ...a, b }) => 1", "({ ...{ a } }) => 1", "(a = 1,) => 1", "(a,,) => 1", "(,) => 1",
  "function f(a) { let a }", "function f([a]) { let a }", "function f({ a }) { const a = 1 }", "function f(a = 1) { class a {} }",
  "function f(...a) { let a }", "(a) => { let a }", "([a]) => { const a = 1 }", "({ a }) => { let a }", "(a = 1) => { let a }",
  "function f([a]) { var a; let a }", "function f(a) { { let a } var b; let b }", "function f(a, b = a) { let b }",
  "'use strict'; function f([eval]) {}", "'use strict'; function f({ arguments }) {}", "'use strict'; var [eval] = []", "'use strict'; ({ eval } = {})",
  "'use strict'; [arguments] = []", "'use strict'; ({ a: eval } = {})", "'use strict'; ({ a: arguments } = {})", "'use strict'; [...eval] = []",
  "'use strict'; var { a: yield } = {}", "'use strict'; var [let] = []", "'use strict'; (eval) => 1", "'use strict'; ([eval]) => 1",
  "'use strict'; ({ arguments }) => 1", "'use strict'; (...eval) => 1", "'use strict'; try { } catch ([eval]) { }",
  "function* g() { var [yield] = [] }", "function* g([yield]) {}", "function* g(a = yield) {}", "function* g({ a = yield }) {}",
  "function* g() { var { a = yield } = {} }", "function* g() { (a = yield) => 1 }", "function* g() { ([yield]) => 1 }",
  "async function f() { var [await] = [] }", "async function f([await]) {}", "async function f(a = await 1) {}", "async function f({ a = await 1 }) {}",
  "async function f() { (a = await 1) => 1 }", "async ([await]) => 1", "async ({ await }) => 1", "async (a = await 1) => 1",
  "try { } catch ([a, a]) { }", "try { } catch ({ a, b: a }) { }", "try { } catch ([a]) { let a }", "try { } catch ({ a }) { var a }",
  "try { } catch ([a]) { for (var a of []); }", "try { } catch ([a]) { function a() {} }", "try { } catch ([...a,]) { }", "try { } catch ([a] = []) { }",
  "try { } catch ({ a } = {}) { }", "try { } catch (...a) { }", "try { } catch ([a], b) { }", "try { } catch ([1]) { }", "try { } catch ({ a: 1 }) { }",
  "try { } catch ([a.b]) { }", "try { } catch ((a)) { }", "try { } catch ([(a)]) { }",
  "({ a = 1 })", "({ a = 1 }, 1)", "[{ a = 1 }]", "x = { a = 1 }", "f({ a = 1 })", "({ a = 1 }).x", "({ a = 1 }) + 1", "({ a: { b = 1 } })",
  "({ a = 1, b } = {})", "({ a = 1 } = {})", "[{ a = 1 }] = [{}]", "({ a: { b = 1 } } = { a: {} })", "for ({ a = 1 } of []);", "for ({ a = 1 } in {});",
  "for ({ a = 1 };;);", "({ a = 1 }) => 1", "async ({ a = 1 }) => 1", "async ({ a = 1 })", "async ({ a = 1 }).x",
  "({ __proto__: a, __proto__: b } = {})", "({ __proto__: a, __proto__: b })", "({ __proto__: a, __proto__: b }) => 1", "[{ __proto__: a, __proto__: b }] = []",
  "function f({ __proto__: a, __proto__: b }) {}", "var { __proto__: a, __proto__: b } = {}",
  "[...a] = 1, [...b] = 2", "var [...[a, ...b]] = []", "var [...[...a]] = []", "var [...[...a, b]] = []", "var [...{ ...a }] = []",
  "let let = 1", "let [let] = []", "let { let } = {}", "const [let] = []", "let { a: let } = {}", "for (let [let] of []);", "for (let { let } in {});",
  "let [a] = 1, [a] = 2", "var [a] = 1, [a] = 2; let a", "let { a } = 1; var { a } = 2", "const [a] = 1; var a", "var a; const { a } = 1",
  "async function f() { for await (var [a] = 1 of []); }", "async function f() { for await ([a] = 1 of []); }", "async function f() { for await (let [a, a] of []); }",
  "async function f() { for await (const [a]; ;); }", "async function f() { for await (var [a] in {}); }",
  "class A { m([a], a) {} }", "class A { constructor({ a }, a) {} }", "class A { static m(...a, b) {} }", "class A { set s([a]) { 'use strict' } }",
  "class A { set s(...a) {} }", "class A { set s(a, b) {} }", "class A { set s({ a }, b) {} }", "class A { set s() {} }", "class A { get g(a) {} }",
  "class A { get g([a]) {} }", "({ set s(...a) {} })", "({ set s(a, b) {} })", "({ set s() {} })", "({ get g(a) {} })", "({ set s([a], a) {} })",
  "({ set s(a = 1) { 'use strict' } })", "({ set s([a]) { 'use strict' } })",
];
for (const src of syntaxErrors) S(src);

// ---- Validações finais com o mesmo resultado esperado em construções válidas (controle de falso positivo do parser).
const validSources = [
  "var [a, b = 1, ...c] = []", "var { a, b: [c], d: { e }, ...f } = {}", "[a, [b], { c }, ...d] = [1, [2], { c: 3 }]", "({ a, b: [c], ...d } = { b: [] })",
  "({ a = 1, b: c = 2 } = {})", "[a = 1, [b = 2]] = [undefined, []]", "for (var [a, b] of []);", "for (let { a } in {});", "for ([a, b] of []);",
  "for ({ a, b } in {});", "(function ([a], { b }, ...c) {})", "(([a], { b }, ...c) => 1)", "(async ([a], { b }, ...c) => 1)",
  "({ m([a], { b } = {}) {}, get g() { return 1 }, set s([a]) {} })", "class A { m([a]) {} static s({ a }) {} set x({ a }) {} }",
  "try { throw 1 } catch ([a]) { }", "try { throw [1] } catch ({ length }) { }", "var [a = function () {}] = []", "var { a = class {} } = {}",
  "var [yield] = []", "var { await } = {}", "var [async] = []", "var { let } = {}", "var [of] = []", "var { get, set, static } = {}",
  "({ a: [b] = [] } = {})", "[{ a } = {}] = []", "[...{ length }] = []", "[...[a]] = []", "({ ...a.b } = {})", "[...a.b] = []", "[...a['b']] = []",
  "[a.b, a['c'], a().d] = []", "({ x: a.b, y: a['c'], z: a().d } = {})", "[a = 1, b.c = 2] = []", "({ a: b.c = 1 } = {})", "[(a.b)] = []", "({ x: (a.b) } = {})",
  "[(a)] = []", "({ x: (a) } = {})", "[((a))] = []", "({ ...(a.b) } = {})", "[...(a.b)] = []",
  "function f(a = 1, { b }, [c] = [], ...d) {}", "function* g(a = 1, { b }) {}", "async function h(a = 1, { b }) {}", "(function (a = function (b = 1) {}) {})",
  "(a, b = 1, [c], { d }, ...e) => 1", "async (a, b = 1, [c], { d }, ...e) => 1", "((a = 1) => 1)", "(({ a } = {}) => 1)", "(([a] = []) => 1)",
  "var f = ({ a, b }, c = a + b) => c", "var f = ([a, b], c = a + b) => c", "var o = { f({ a }, [b] = [a]) { return b } }",
];
for (const src of validSources) S(src);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "destructuring-golden-"));
// O bun passa arquivos pelo transpilador próprio (muda a semântica de script do JSC). `vm.runInThisContext` roda como
// ProgramExecutable do JSC puro, então o programa vai por ele; o SyntaxError de compilação é engolido e `R` fica
// indefinido ("<undefined>"). O resultado sai no `exit`, depois de esvaziadas as microtarefas.
const source_file = path.join(dir, "destructuring_source.js");
const file = path.join(dir, "destructuring_case.js");
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
