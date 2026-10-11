// Gera tests/golden/global_edge_bun.tsv: objeto global e funções globais de borda (parseInt/parseFloat/isNaN/isFinite,
// encodeURI/decodeURI/escape/unescape, descritores de globalThis, delete de globais, `this`, call/apply/bind, Symbol.hasInstance,
// caller/arguments, `arguments.callee`, descritores de Function, toString de nativas) medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa roda por `vm.runInThisContext` (JSC puro) num processo novo e grava `globalThis.R`.
// Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-global-edge-golden.js > tests/golden/global_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  "function S(v){try{return typeof v==='string'?JSON.stringify(v):Object.is(v,-0)?'-0':typeof v==='bigint'?v+'n':" +
  "typeof v==='symbol'?v.toString():typeof v==='undefined'?'undefined':typeof v==='function'?'function':String(v)}catch(e){return '?'}}\n" +
  "function T(f){try{return S(f())}catch(e){return 'throw '+(e&&e.name)+': '+(e&&e.message)}}\n";

const programs = [];
const add = body => programs.push(body);
const q = s => JSON.stringify(s);
// Expressão avaliada com captura de exceção.
const E = expr => add(`${PRELUDE}globalThis.R = T(() => (${expr}));`);
// Corpo de statements que atribui globalThis.R.
const B = body => add(`${PRELUDE}try { ${body} } catch (e) { globalThis.R = e.name + ': ' + e.message }`);

// ---- 1. parseInt / parseFloat / isNaN / isFinite.
const numInputs = [
  "", " ", "  12  ", "\\t\\n12", "\\u00a012", "\\ufeff12", "\\u200b12", "\\u180e12", "12abc", "abc", "-", "+", "-0", "+0", "0x", "0x1f", "-0x1f", "0X1F", "0b11", "0o17",
  "1e3", "1e", ".5", "5.", "-.5", "+.5", "..5", "1_000", "1,000", "Infinity", "-Infinity", "+Infinity", "infinity", "Infinityx", "NaN", "0.0000001", "1e21", "1e-7", "123456789012345678901234567890",
  "9007199254740993", "0.1e2", "00012", "-00012", "0x10p2", "1e+3", "1e-3", "1E3", "٣", "１２", "12\\u2028", "0.", "-0.0", "1.7976931348623159e308", "5e-324", "2e-324", "0x7fffffffffffffff",
  "  -  1", "1 2", "1\\u00002", "0e0", "-0e0", "1e1000", "-1e1000", "0.0e-400", ".e1", "0x.1", "0x1.8",
];
for (const s of numInputs) {
  const lit = `"${s}"`;
  for (const f of ["parseInt", "parseFloat", "Number", "isNaN", "isFinite", "Number.isNaN", "Number.isFinite", "Number.parseFloat"]) E(`${f}(${lit})`);
}
for (const s of ["12", "ff", "FF", "z", "Z", "10", "7", "8", "9", "0x1f", "-0x1f", "101", "zz", "1e3", "  42  ", "9007199254740993", "0.5", "12abc"]) {
  for (const radix of [0, 1, 2, 3, 8, 10, 16, 36, 37, -1, 4294967298, 4294967312, "'16'", "'0x10'", "NaN", "Infinity", "undefined", "null", "true", "{}", "[16]", "16.9", "-0", "1e1"])
    E(`parseInt(${q(s)}, ${radix})`);
}
for (const v of [
  "undefined", "null", "true", "false", "0", "-0", "NaN", "Infinity", "-Infinity", "''", "' '", "'0'", "'1e1000'", "[]", "[1]", "[1,2]", "{}", "[[]]", "[[1]]", "new Date(NaN)", "new Date(0)", "Symbol()", "1n", "function(){}",
  "{valueOf(){return 3}}", "{valueOf(){return NaN}}", "{toString(){return '5'}}", "{valueOf(){return {}}, toString(){return '7'}}", "{valueOf(){return {}}, toString(){return {}}}", "new Number(3)", "new String('4')", "new Boolean(true)",
  "Object(1n)", "Object(Symbol())", "{[Symbol.toPrimitive](){return 9}}", "{[Symbol.toPrimitive](){return {}}}", "{[Symbol.toPrimitive]:1}",
]) {
  for (const f of ["isNaN", "isFinite", "Number.isNaN", "Number.isFinite", "parseInt", "parseFloat", "Number.isInteger", "Number.isSafeInteger"]) E(`${f}(${v})`);
}
E("[parseInt, parseFloat, isNaN, isFinite].map(f => f.length + ':' + f.name).join()");
E("parseInt === Number.parseInt");
E("parseFloat === Number.parseFloat");
E("isNaN === Number.isNaN");
E("Object.getOwnPropertyNames(parseInt).join()");
E("parseInt.hasOwnProperty('prototype')");
E("(() => { try { new parseInt('1') } catch (e) { return e.name + ': ' + e.message } })()");
E("(() => { try { new isNaN('1') } catch (e) { return e.name + ': ' + e.message } })()");
E("parseInt.call(null, '12')");
E("parseInt.apply(null, ['11', 2])");
E("['1', '2', '3'].map(parseInt).join()");
E("['10', '10', '10'].map(parseInt).join()");
E("parseInt(0.0000005)");
E("parseInt(1e21)");
E("parseInt(-1e-7)");
E("parseInt(null, 36)");
E("parseInt('Infinity')");
E("parseFloat('1e1000')");
E("parseFloat('-1e1000')");
E("parseFloat('0x10')");
E("parseFloat('1.5.5')");
E("parseFloat('1e5e5')");
E("1 / parseFloat('-0')");
E("1 / parseInt('-0')");
E("parseInt('9'.repeat(400))");
E("parseFloat('9'.repeat(400))");
E("parseFloat('0.' + '0'.repeat(400) + '1')");
E("parseInt('1'.repeat(70), 2)");
E("parseInt('z'.repeat(20), 36)");

// ---- 2. encodeURI / decodeURI / encodeURIComponent / decodeURIComponent / escape / unescape.
const uriInputs = [
  "", "a", "abc", "a b", "a+b", "a%b", "%", "%%", "%2", "%2G", "%G2", "%41", "%4", "%e", "%E2", "%E2%82", "%E2%82%AC", "%e2%82%ac", "%C0%80", "%C1%BF", "%C2", "%C2%80", "%ED%A0%80", "%ED%BF%BF", "%F4%90%80%80",
  "%F0%80%80%80", "%F0%90%80%80", "%F4%8F%BF%BF", "%80", "%BF", "%F8%88%80%80%80", "%FF", "%FE", "%F5%80%80%80", "%E0%80%80", "%E0%A0%80", "%EF%BF%BE", "%00", "%0", "%u0041", "%U0041", "%25", "%2525",
  "%3B", "%2F", "%3F", "%23", "%24", "%26", "%2B", "%2C", "%3A", "%3D", "%40", "%20", "%7E", "%21", "%27", "%28", "%29", "%2A", "%5B", "%5D", "%7B", "%7D", "%7C", "%5C", "%5E", "%60",
  ";/?:@&=+$,#", "-_.!~*'()", "[]{}|\\^`<>\"", "é", "€", "\u0000", "\u007f", "\u0080", "߿", "ࠀ", "￿", "￾", "😀", "\ud83d", "\ude00", "\ud83d\ud83d", "\ude00\ud83d", "a\ud800", "􏿿",
  "\ud800a", "\udfff", "\ud83dx\ude00", " ", "﻿", "ı", "K", "http://a b/c?d=e f#g h", "%E4%BD%A0%E5%A5%BD", "你好", "%F0%9F%98%80", "%f0%9f%98%80", "%F0%9F%98", "%F0%9F", "%F0",
  "a%20b%2", "%2F%3F", "%E2%82%AC%", "%E2%82%AC%2", "%41%42%43", "%4a%4A", "%7e", "%1", "% 1", "%١١", "%+1", "%-1", "%0x", "%x0",
];
const jsLit = s => q(s);
for (const s of uriInputs) {
  for (const f of ["encodeURI", "encodeURIComponent", "decodeURI", "decodeURIComponent", "escape", "unescape"]) E(`${f}(${jsLit(s)})`);
}
for (const s of ["%u0041", "%u00e9", "%u20AC", "%uD83D%uDE00", "%u", "%u1", "%u12", "%u123", "%u123G", "%U0041", "%uFFFF", "%u0000", "%uzzzz", "%41", "%4", "%", "%%41", "%u%u", "%u0041%", "%E9", "%e9", "%00", "%FF", "%100", "%u00411",
  "a%20b", "%20", "%2", "%2G", "% ", "%é"]) {
  E(`unescape(${jsLit(s)}).split('').map(c => c.charCodeAt(0).toString(16)).join()`);
}
for (const s of ["@*_+-./", "abc ABC 123", "éè", "€", "😀", "\ud83d", "\u0000\u001f\u007f", "!#$%&'()", "=?[]{}~", "ÿĀ", "￿"]) E(`escape(${jsLit(s)})`);
E("escape.length + ':' + escape.name + ':' + unescape.length + ':' + unescape.name");
E("[encodeURI, decodeURI, encodeURIComponent, decodeURIComponent].map(f => f.length + ':' + f.name).join()");
E("escape(undefined)");
E("escape(null)");
E("escape()");
E("escape(12.5)");
E("escape({toString(){return 'a b'}})");
E("escape(Symbol())");
E("escape(1n)");
E("encodeURI()");
E("decodeURI()");
E("encodeURIComponent(undefined)");
E("encodeURIComponent(null)");
E("encodeURIComponent([1, 2])");
E("encodeURIComponent(Symbol())");
E("encodeURIComponent({toString(){throw new RangeError('x')}})");
E("encodeURI('\\ud800')");
E("(() => { try { encodeURI('\\ud800') } catch (e) { return e instanceof URIError } })()");
E("(() => { try { decodeURI('%') } catch (e) { return [e.name, e.message, Object.getPrototypeOf(e) === URIError.prototype].join() } })()");
E("(() => { try { decodeURIComponent('%E2%82') } catch (e) { return e.constructor === URIError } })()");
E("decodeURI('%3B%2F%3F%3A%40%26%3D%2B%24%2C%23')");
E("decodeURIComponent('%3B%2F%3F%3A%40%26%3D%2B%24%2C%23')");
E("decodeURI('%3b%2f%3F')");
E("decodeURI('%41%3B%42')");
E("decodeURI('%25')");
E("decodeURI('%2525')");
E("decodeURI('%23%24%26%2B%2C%2F%3A%3B%3D%3F%40')");
E("encodeURI(decodeURI('%2F'))");
E("Array.from({length: 128}, (_, i) => encodeURIComponent(String.fromCharCode(i))).join('')");
E("Array.from({length: 128}, (_, i) => encodeURI(String.fromCharCode(i))).join('')");
E("Array.from({length: 128}, (_, i) => escape(String.fromCharCode(i))).join('')");
E("encodeURIComponent('\\ud83d\\ude00'.repeat(3))");
E("decodeURIComponent('%F0%9F%98%80%F0%9F%98%80').length");
E("decodeURIComponent('%EF%BB%BFa').length");
E("encodeURI('a'.repeat(100000)).length");
E("decodeURI('%61'.repeat(50000)).length");
E("new.target === undefined && (() => { try { new encodeURI('a') } catch (e) { return e.message } })()");
E("encodeURIComponent.call(null)");
E("decodeURIComponent.call('x', '%41')");

// ---- 3. globalThis: descritores de var / function / let / class / const / atribuição solta (script).
const declForms = [
  ["var v = 1", "v"], ["function v() {}", "v"], ["let v = 1", "v"], ["const v = 1", "v"], ["class v {}", "v"], ["v = 1", "v"], ["var v", "v"], ["var v; var v = 2", "v"],
  ["function v() {} var v = 3", "v"], ["async function v() {}", "v"], ["function* v() {}", "v"], ["async function* v() {}", "v"],
];
for (const [decl, name] of declForms) {
  const d = `globalThis.R = T(() => JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, '${name}'), (k, x) => typeof x === 'function' ? 'fn' : x === undefined ? 'U' : x));`;
  add(`${PRELUDE}${decl};\n${d}`);
  add(`${PRELUDE}'use strict';\n${decl};\n${d}`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => ['${name}' in globalThis, globalThis.hasOwnProperty('${name}'), typeof globalThis.${name}, typeof ${name}, Object.keys(globalThis).includes('${name}')].join());`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => [delete globalThis.${name}, '${name}' in globalThis, typeof globalThis.${name}].join());`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => { globalThis.${name} = 9; return S(globalThis.${name}) });`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => (0, eval)('delete ${name}'));`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => [Reflect.deleteProperty(globalThis, '${name}'), Reflect.has(globalThis, '${name}')].join());`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => (0, eval)('var ${name}; typeof ${name}'));`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => (0, eval)('let ${name} = 5; ${name}'));`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => (0, eval)('function ${name}() {}; typeof ${name}'));`);
  add(`${PRELUDE}${decl};\nglobalThis.R = T(() => (0, eval)('(function(){ return typeof ${name} })()'));`);
}
// Redeclaração entre dois scripts não é possível aqui (um só script por processo); usa eval indireto para a matriz.
const firsts = ["var a = 1", "let a = 1", "const a = 1", "function a() {}", "class a {}", "a = 1"];
const seconds = ["var a = 2", "let a = 2", "const a = 2", "function a() {}", "class a {}", "a = 2", "var a", "function* a() {}", "async function a() {}"];
for (const f of firsts) for (const s of seconds) {
  add(`${PRELUDE}${f};\nglobalThis.R = T(() => { (0, eval)(${q(s)}); return typeof a + ':' + S(Object.getOwnPropertyDescriptor(globalThis, 'a') && Object.getOwnPropertyDescriptor(globalThis, 'a').configurable) });`);
}
for (const f of ["undefined", "NaN", "Infinity", "globalThis", "Object", "Math", "JSON", "Reflect", "Intl", "parseInt", "eval", "Symbol", "WebAssembly", "escape", "Atomics", "SharedArrayBuffer", "Proxy", "Date", "Array"]) {
  add(`${PRELUDE}globalThis.R = T(() => JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, '${f}'), (k, x) => typeof x === 'function' ? 'fn' : x === undefined ? 'U' : x));`);
}
for (const f of ["undefined", "NaN", "Infinity"]) {
  add(`${PRELUDE}globalThis.R = T(() => { ${f} = 1; return typeof ${f} + (Object.is(${f}, ${f}) ? '' : 'nan') });`);
  add(`${PRELUDE}'use strict'; globalThis.R = T(() => { ${f} = 1; return typeof ${f} });`);
  add(`${PRELUDE}globalThis.R = T(() => (0, eval)('var ${f}; typeof ${f}'));`);
  add(`${PRELUDE}globalThis.R = T(() => (0, eval)('let ${f} = 1; typeof ${f}'));`);
  add(`${PRELUDE}globalThis.R = T(() => (0, eval)('function ${f}() {}; typeof ${f}'));`);
  add(`${PRELUDE}globalThis.R = T(() => [delete globalThis.${f}, Reflect.deleteProperty(globalThis, '${f}')].join());`);
  add(`${PRELUDE}globalThis.R = T(() => (function(${f}) { return typeof ${f} })(7));`);
  add(`${PRELUDE}globalThis.R = T(() => { var ${f} = 3; return typeof ${f} })`);
}
E("Object.prototype.toString.call(globalThis)");
E("globalThis === this");
E("globalThis === self");
E("typeof window + typeof self + typeof global + typeof globalThis");
E("globalThis.globalThis === globalThis");
E("Object.getPrototypeOf(globalThis) === Object.prototype");
E("Object.getPrototypeOf(Object.getPrototypeOf(globalThis)) === Object.prototype");
E("Object.isExtensible(globalThis)");
E("Object.isFrozen(globalThis)");
E("globalThis[Symbol.toStringTag]");
E("Object.getOwnPropertySymbols(globalThis).length");
E("(() => { var d = Object.getOwnPropertyDescriptor(globalThis, 'globalThis'); return [d.writable, d.enumerable, d.configurable].join() })()");
E("Object.keys(globalThis).includes('Object')");
E("Object.getOwnPropertyNames(globalThis).includes('Object')");
E("Object.getOwnPropertyNames(globalThis).includes('R')");
E("typeof globalThis.constructor + ':' + globalThis.constructor.name");
E("'toString' in globalThis && globalThis.hasOwnProperty('toString')");
E("(function(){ return this === globalThis })()");
E("(function(){ 'use strict'; return this })()");
E("(() => this === globalThis)()");
E("(function(){ return typeof this })()");
E("(function(){ 'use strict'; return typeof this })()");
E("(function(){ return this }).call(1) instanceof Number");
E("(function(){ 'use strict'; return this }).call(1)");
E("(function(){ return this }).call('s').length");
E("(function(){ return typeof this }).call(Symbol())");
E("(function(){ return typeof this }).call(1n)");
E("(function(){ return this === globalThis }).call(null)");
E("(function(){ return this === globalThis }).call(undefined)");
E("(function(){ 'use strict'; return this }).call(null)");
E("(function(){ 'use strict'; return this }).call(undefined)");
E("(function(){ return this === globalThis }).apply(null)");
E("(function(){ return this === globalThis }).bind(null)()");
E("(function(){ return this === globalThis }).bind(undefined)()");
E("(function(){ return this === globalThis })()");
E("(function(){ return (function(){ return this === globalThis })() }).call({})");
E("({ f() { return this === globalThis } }).f.call(undefined)");
E("({ f() { return (() => this)() } }).f.call(7) instanceof Number");
E("({ f() { 'use strict'; return (() => this)() } }).f.call(7)");
E("(class { static m() { return this } }).m.call(undefined)");
E("(class { m() { return this } }).prototype.m.call(undefined)");
E("(async function(){ return this === globalThis })().constructor === Promise");
E("(function*(){ yield this })().next().value === globalThis");
E("(function*(){ 'use strict'; yield this })().next().value");
E("[1].map(function(){ return this === globalThis })[0]");
E("[1].map(function(){ 'use strict'; return this })[0]");
E("[1].map(function(){ return typeof this }, 5)[0]");
E("[1].map(function(){ 'use strict'; return typeof this }, 5)[0]");
E("[1].map(function(){ return this === globalThis }, null)[0]");
E("[1].forEach.call([1], function(){ R2 = this === globalThis }); R2");
E("typeof globalThis.R2");
E("new (function(){ this.x = this === globalThis })().x");
E("(function(){ return new.target })()");
E("Reflect.apply(function(){ return this }, 5, []) instanceof Number");
E("Reflect.apply(function(){ 'use strict'; return this }, 5, [])");
E("eval('this') === globalThis");
E("(0, eval)('this') === globalThis");
E("(function(){ return eval('this') }).call(3) instanceof Number");
E("(function(){ 'use strict'; return eval('this') }).call(3)");
E("(function(){ return (0, eval)('this') === globalThis }).call(3)");
E("new Function('return this')() === globalThis");
E("new Function('\"use strict\"; return this')()");
E("Function('return this').call(2) instanceof Number");
E("(function(){ return this }).call(true) instanceof Boolean");
E("(function(){ return Object.prototype.toString.call(this) }).call(1n)");
E("(function(){ return Object.prototype.toString.call(this) }).call(Symbol.iterator)");
E("(function(){ return Object.prototype.toString.call(this) }).call('x')");
E("(function(){ 'use strict'; return Object.prototype.toString.call(this) }).call(undefined)");
E("(function(){ 'use strict'; return Object.prototype.toString.call(this) }).call(null)");
E("(function(){ return this }).call(globalThis) === globalThis");
E("((a = this) => a === globalThis)()");
E("(function(a = this){ return a === globalThis })()");
E("(function(){ var self = this; return (function(){ return self === this })() })()");
B("var f = function(){ return this }; globalThis.R = String(f() === globalThis) + String(({f}).f() !== globalThis) + String(typeof (0, ({f}).f)())");
B("var o = { f() { return this } }; globalThis.R = String((o.f)() === o) + String((0, o.f)() === globalThis) + String((o.f = o.f)() === globalThis) + String((o['f'])() === o)");
B("var o = { f() { 'use strict'; return this } }; globalThis.R = String((0, o.f)()) + String((o.f || 1)())");
B("var o = { f() { return this } }; var g = o.f; globalThis.R = String(g() === globalThis) + String(g.call(o) === o)");
B("with ({ f() { return this } }) { globalThis.R = typeof f() }");
B("with ({ f() { 'use strict'; return this } }) { globalThis.R = typeof f() }");
B("var o = { f() { return this } }; with (o) { globalThis.R = String(f() === o) }");
B("var o = { f() { return this } }; var h = { o }; globalThis.R = String(h.o.f() === h.o) + String((h.o.f)() === h.o)");
B("this.zz = 1; globalThis.R = String(globalThis.zz) + String(delete globalThis.zz) + String(typeof zz)");
B("this.R = 'via this'");
B("var self2 = this; globalThis.R = String(self2 === globalThis) + typeof this");
B("globalThis.R = String(Object.getOwnPropertyDescriptor(this, 'R') === undefined)");
B("function f() { return typeof this.R } globalThis.R = f()");
B("function f() { 'use strict'; return this } globalThis.R = String(f())");
B("function f() { return this } globalThis.R = String(f() === this)");
B("function f() { this.q = 1 } f(); globalThis.R = String(globalThis.q) + String(typeof q); delete globalThis.q");
B("function f() { 'use strict'; this.q = 1 } f()");
B("function f() { 'use strict'; this.q = 1 } try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { get g() { return this }, set s(v) { this.v = v } }; globalThis.R = String(o.g === o) + String(Object.getOwnPropertyDescriptor(o, 'g').get.call(undefined) === globalThis)");
B("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); globalThis.R = String(d.get.call(1) === Number.prototype) + String(typeof d.set)");
B("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.get.call(undefined) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call(undefined, {}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(this.__proto__ === Object.prototype) + String(globalThis.__proto__ === this.__proto__)");
B("globalThis.R = String(this.hasOwnProperty('globalThis')) + String(this.propertyIsEnumerable('globalThis'))");
B("var undefinedDesc = Object.getOwnPropertyDescriptor(globalThis, 'undefined'); globalThis.R = JSON.stringify([undefinedDesc.writable, undefinedDesc.enumerable, undefinedDesc.configurable])");
B("globalThis.R = Object.keys(this).filter(k => !['R'].includes(k) && typeof globalThis[k] !== 'function').sort().join().length > -1 ? 'ok' : 'no'");
B("var x1 = 1, x2 = 2; globalThis.R = Object.keys(globalThis).filter(k => /^x\\d$/.test(k)).join()");
B("let x1 = 1; globalThis.R = String('x1' in globalThis) + String(typeof x1) + String(Object.keys(globalThis).includes('x1'))");
B("let x1 = 1; globalThis.x1 = 2; globalThis.R = x1 + ':' + globalThis.x1");
B("var x1 = 1; delete globalThis.x1; globalThis.R = String(typeof x1) + String(Object.getOwnPropertyDescriptor(globalThis, 'x1').configurable)");
B("x1 = 1; delete globalThis.x1; globalThis.R = String(typeof x1)");
B("x1 = 1; globalThis.R = String(delete x1) + String(typeof x1)");
B("var x1 = 1; globalThis.R = String(delete x1) + String(typeof x1)");
B("function x1() {} globalThis.R = String(delete x1) + String(typeof x1)");
B("globalThis.x1 = 1; globalThis.R = String(delete x1) + String(typeof x1)");
B("Object.defineProperty(globalThis, 'x1', { value: 1, configurable: false }); globalThis.R = String(delete globalThis.x1) + String(typeof x1) + String(Reflect.deleteProperty(globalThis, 'x1'))");
B("Object.defineProperty(globalThis, 'x1', { value: 1, configurable: true }); globalThis.R = String(delete x1) + String(typeof x1)");
B("Object.defineProperty(globalThis, 'x1', { get() { return 7 }, configurable: true }); globalThis.R = String(x1) + String(typeof x1) + String(delete x1)");
B("Object.defineProperty(globalThis, 'x1', { get() { throw new TypeError('boom') }, configurable: true }); globalThis.R = String(typeof x1)");
B("Object.defineProperty(globalThis, 'x1', { value: 1, writable: false, configurable: true }); x1 = 2; globalThis.R = String(x1)");
B("Object.defineProperty(globalThis, 'x1', { value: 1, writable: false, configurable: true }); (function(){ 'use strict'; x1 = 2 })()");
B("Object.defineProperty(globalThis, 'x1', { value: 1, writable: false, configurable: true }); try { (function(){ 'use strict'; x1 = 2 })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (function(){ 'use strict'; undeclaredVariable = 2 })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { undeclaredVariable } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = typeof undeclaredVariable");
B("try { undeclaredVariable++ } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { undeclaredVariable += 1 } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { undeclaredVariable = 1; globalThis.R = String(globalThis.undeclaredVariable) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (function(){ 'use strict'; delete undeclaredVariable })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(delete undeclaredVariable)");
B("globalThis.R = String(delete globalThis.undeclaredVariable) + String(delete this.nope)");
B("globalThis.R = String(delete Math.PI) + String(delete globalThis.Math) + String(typeof Math)");
B("globalThis.R = String(delete globalThis.Object) + String(typeof Object)");
B("globalThis.R = String(delete globalThis.NaN) + String(delete globalThis.undefined) + String(delete globalThis.Infinity)");
B("try { (function(){ 'use strict'; delete globalThis.NaN })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (function(){ 'use strict'; delete Object.prototype })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; delete x') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; delete (x)') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; delete ((x))') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('delete (x)') ; globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(typeof eval) + String(eval.length) + eval.name + String(eval === globalThis.eval)");
B("var eval2 = eval; var x1 = 'g'; function f() { var x1 = 'l'; return [eval('x1'), eval2('x1'), (0, eval)('x1'), globalThis.eval('x1'), (eval)('x1'), eval?.('x1'), (eval, eval)('x1')].join() } globalThis.R = f()");
B("function f() { var x1 = 'l'; var o = { eval }; return o.eval('typeof x1') } globalThis.R = f()");
B("function f() { var x1 = 'l'; return new Function('return typeof x1')() } globalThis.R = f()");
B("function f(eval) { return eval } globalThis.R = f(5)");
B("try { eval('function f(eval) { \"use strict\"; }') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; var eval') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; eval = 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; arguments = 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; function eval() {}') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; (eval) => 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; eval++') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; try {} catch (eval) {}') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; ({ eval } = {})') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { eval('\"use strict\"; [eval] = []') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var eval = 5; globalThis.R = String(typeof eval)");
B("var eval = 5; globalThis.R = String(eval) + String(typeof globalThis.eval)");
B("let eval = 5; globalThis.R = String(eval)");
B("const eval = 5; globalThis.R = String(eval)");
B("globalThis.R = String(({ eval: 7 }).eval) + String(({ eval() { return 8 } }).eval())");
B("var o = { eval }; globalThis.R = String(o.eval('1+1')) + String(o.eval === eval)");
B("globalThis.R = String(eval(1)) + String(eval()) + String(eval('')) + String(eval({a:1}).a) + String(eval(null)) + String(eval(undefined))");
B("globalThis.R = String(eval('1;;;;')) + String(eval('1;var a;')) + String(eval('1;function f(){}')) + String(eval('var a = 1')) + String(eval('{}')) + String(eval('1;{}')) + String(eval('1;if(0);'))");
B("globalThis.R = String(eval('({})').constructor.name) + String(eval('{a:1}')) + String(eval('{a:1,b:2}'))");
B("globalThis.R = String(eval('1;do { 2; break } while(0)')) + String(eval('3;switch(0){case 0: 4}')) + String(eval('5;try{6}finally{7}')) + String(eval('8;for(;0;);'))");
B("globalThis.R = String(eval(new String('1+1'))) + typeof eval(new String('1+1'))");
B("var x1 = 1; function f() { eval('var x1 = 2'); return x1 } globalThis.R = f() + ':' + x1");
B("function f() { 'use strict'; eval('var x2 = 2'); return typeof x2 } globalThis.R = f()");
B("function f() { eval('var x2 = 2'); return typeof x2 } globalThis.R = f() + typeof x2");
B("function f() { (0, eval)('var x2 = 2'); return typeof x2 } globalThis.R = f() + typeof x2 + String(delete globalThis.x2)");
B("function f() { eval('function g() {}'); return typeof g } globalThis.R = f() + typeof g");
B("function f() { eval('let q = 1'); return typeof q } globalThis.R = f()");
B("function f() { return eval('arguments.length') } globalThis.R = f(1, 2, 3)");
B("function f() { return (0, eval)('typeof arguments') } globalThis.R = f(1, 2, 3)");
B("function f() { return eval('typeof new.target') } globalThis.R = f()");
B("function f() { return (0, eval)('new.target') } try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('super.x') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('return 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('break') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('await 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('yield 1') ; globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('let let = 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('var undefined = 1; var NaN; var Infinity;'); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('let undefined = 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('const NaN = 1; globalThis.R = String(NaN)') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('class Infinity {}') ; globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (0, eval)('function undefined() {}'); globalThis.R = typeof undefined } catch (e) { globalThis.R = e.name + ': ' + e.message }");

// ---- 4. arguments.callee, caller, Function.prototype.caller/arguments.
B("function f() { return arguments.callee === f } globalThis.R = f()");
B("function f() { 'use strict'; return arguments.callee } try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee') } var d = f(); globalThis.R = [typeof d.get, d.get === d.set, d.enumerable, d.configurable, 'value' in d].join()");
B("function f() { return Object.getOwnPropertyDescriptor(arguments, 'callee') } var d = f(); globalThis.R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()");
B("function f(a) { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor((function(){ 'use strict'; return arguments })(), 'callee').get } globalThis.R = f()");
B("var t = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); globalThis.R = JSON.stringify([typeof t.get, typeof t.set, t.get === t.set, t.enumerable, t.configurable])");
B("var t = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); globalThis.R = JSON.stringify([typeof t.get, typeof t.set, t.get === t.set, t.enumerable, t.configurable])");
B("var t = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); globalThis.R = t.get.name + ':' + t.get.length + ':' + t.set.name + ':' + t.set.length");
B("try { Function.prototype.caller } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.arguments } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(Function.prototype.caller)");
B("globalThis.R = String(Function.prototype.arguments)");
B("function f() { 'use strict'; return f.caller } try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { 'use strict'; return f.arguments } try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return f.caller } function g() { return f() } globalThis.R = String(g() === g)");
B("function f() { return f.caller } globalThis.R = String(f())");
B("function f() { return f.caller } function g() { 'use strict'; return f() } globalThis.R = String(g())");
B("function f() { return f.caller } globalThis.R = String((function() { return f() })() === null)");
B("function f() { return f.arguments } globalThis.R = String(f(1, 2).length)");
B("function f() { return f.arguments } globalThis.R = String(f.arguments)");
B("function f() { return f.arguments === arguments } globalThis.R = String(f())");
B("function f() { return f.caller } var o = { m() { return f() } }; globalThis.R = String(o.m() === o.m)");
B("function f() { return f.caller } globalThis.R = String((() => f())())");
B("function f() { return f.caller } globalThis.R = String((async function(){ return f() })())");
B("function f() { return f.caller } globalThis.R = String(f.call(null))");
B("var f = (function() { 'use strict'; return function g() { return typeof g.caller } })(); try { globalThis.R = f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = () => 1; globalThis.R = String(Object.getOwnPropertyNames(f).sort().join()) + String(f.hasOwnProperty('caller')) + String(f.hasOwnProperty('arguments'))");
B("function f() {} globalThis.R = Object.getOwnPropertyNames(f).join() + '|' + Object.getOwnPropertyNames(f).length");
B("function f() { 'use strict' } globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = class {}; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = class { static x = 1; static m() {} }; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = function*() {}; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = async function() {}; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = async () => 1; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = { m() {} }.m; globalThis.R = Object.getOwnPropertyNames(f).join()");
B("var f = { get g() { return 1 } }; globalThis.R = Object.getOwnPropertyNames(Object.getOwnPropertyDescriptor(f, 'g').get).join()");
B("var f = function() {}.bind(); globalThis.R = Object.getOwnPropertyNames(f).join()");
B("globalThis.R = Object.getOwnPropertyNames(Math.max).join()");
B("globalThis.R = Object.getOwnPropertyNames(Function.prototype).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Function).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Object.getPrototypeOf(function*(){})).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Object.getPrototypeOf(async function(){})).sort().join()");
B("globalThis.R = Reflect.ownKeys(Function.prototype).map(String).join()");

// ---- 5. Propriedades de Function: name, length, prototype.
const fnForms = {
  decl: "function f(a, b) {}", expr: "(function (a, b) {})", named: "(function g(a, b) {})", arrow: "((a, b) => 1)", arrowParens: "(a => 1)", method: "({ m(a, b) {} }).m", getter: "Object.getOwnPropertyDescriptor({ get g() { return 1 } }, 'g').get",
  setter: "Object.getOwnPropertyDescriptor({ set s(v) {} }, 's').set", asyncFn: "(async function (a, b) {})", asyncArrow: "(async (a, b) => 1)", asyncMethod: "({ async m(a, b) {} }).m", gen: "(function* (a, b) {})",
  genMethod: "({ *m(a, b) {} }).m", asyncGen: "(async function* (a, b) {})", asyncGenMethod: "({ async *m(a, b) {} }).m", cls: "(class { })", clsNamed: "(class K { constructor(a, b) {} })", clsMethod: "(class { m(a, b) {} }).prototype.m",
  clsStatic: "(class { static m(a, b) {} }).m", clsPrivate: "(class { static f() { return class { static #p() {} static g() { return this.#p } } } }).f().g()", clsGetter: "Object.getOwnPropertyDescriptor((class { get g() { return 1 } }).prototype, 'g').get",
  clsExtends: "(class extends Object { })", defaults: "(function (a, b = 1, c) {})", rest: "(function (a, ...r) {})", destr: "(function ({ a }, [b]) {})", defaultFirst: "(function (a = 1, b) {})", nativeFn: "Math.max",
  nativeCtor: "Map", bound: "(function (a, b) {}).bind(null, 1)", boundBound: "(function (a, b, c) {}).bind(null, 1).bind(null, 2)", computed: "({ ['x' + 1]() {} }).x1", symbolKey: "({ [Symbol('d')]() {} })[Object.getOwnPropertySymbols({ [Symbol('d')]() {} })[0]]",
  symbolMethod: "(() => { var s = Symbol('desc'); return ({ [s]() {} })[s] })()", symbolEmpty: "(() => { var s = Symbol(); return ({ [s]() {} })[s] })()", fnCtor: "new Function('a', 'b', 'return 1')", fnCtorAnon: "Function()", genCtor: "new (Object.getPrototypeOf(function*(){}).constructor)('a', 'yield a')",
  proxyFn: "new Proxy(function (a, b) {}, {})", objectName: "({ f: function () {} }).f", objectArrow: "({ f: () => 1 }).f", objectClass: "({ f: class {} }).f", varName: "(() => { var v = function () {}; return v })()", letArrow: "(() => { let v = () => 1; return v })()",
  defaultName: "(({ a = function () {} } = {}) => a)()", assignName: "(() => { var v; v = function () {}; return v })()", assignClass: "(() => { var v; v = class {}; return v })()", assignAnd: "(() => { var v = null; v ??= function () {}; return v })()",
  parenName: "(() => { var v = (function () {}); return v })()", commaName: "(() => { var v = (0, function () {}); return v })()", staticField: "(class { static f = function () {} }).f", staticFieldArrow: "(class { static f = () => 1 }).f",
  privateField: "(class { static #f = function () {}; static g() { return this.#f } }).g()", exportDefault: "(function () {})", propertyAssign: "(() => { var o = {}; o.p = function () {}; return o.p })()",
  emptyName: "({ '': function () {} })['']", numKey: "({ 1: function () {} })[1]", bigKey: "({ 1n: function () {} })[1]", getterSymbol: "(() => { var s = Symbol('q'); return Object.getOwnPropertyDescriptor({ get [s]() {} }, s).get })()",
};
const fnProps = [
  "typeof f", "f.name", "f.length", "JSON.stringify(Object.getOwnPropertyDescriptor(f, 'name'))", "JSON.stringify(Object.getOwnPropertyDescriptor(f, 'length'))",
  "(() => { var d = Object.getOwnPropertyDescriptor(f, 'prototype'); return d ? JSON.stringify([d.writable, d.enumerable, d.configurable, typeof d.value]) : 'none' })()",
  "f.hasOwnProperty('prototype')", "Object.getOwnPropertyNames(f).join()", "Object.getPrototypeOf(f) === Function.prototype", "Object.prototype.toString.call(f)",
  "(() => { var p = f.prototype; return p && typeof p === 'object' ? Object.getOwnPropertyNames(p).join() + ':' + JSON.stringify(Object.getOwnPropertyDescriptor(p, 'constructor') && [Object.getOwnPropertyDescriptor(p, 'constructor').enumerable, Object.getOwnPropertyDescriptor(p, 'constructor').writable, Object.getOwnPropertyDescriptor(p, 'constructor').configurable]) : String(p) })()",
  "(() => { try { return typeof new f() } catch (e) { return e.name + ': ' + e.message } })()", "(() => { try { return typeof f() } catch (e) { return e.name + ': ' + e.message } })()", "Object.isExtensible(f)", "Reflect.ownKeys(f).map(String).join()",
];
for (const [label, expr] of Object.entries(fnForms)) {
  for (const p of fnProps) B(`var f = ${expr}; globalThis.R = T(() => (${p}))`);
}
B("function f(a, b) {} f.name = 'x'; globalThis.R = f.name");
B("function f(a, b) { 'use strict' } try { (function(){ 'use strict'; f.name = 'x' })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f(a, b) {} globalThis.R = String(delete f.name) + String(f.name) + String(f.hasOwnProperty('name'))");
B("function f(a, b) {} globalThis.R = String(delete f.length) + String(f.length) + String(Function.prototype.length)");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: 7 }); globalThis.R = f.length + ':' + f.bind().length");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: -1 }); globalThis.R = f.length + ':' + f.bind().length");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: 2.7 }); globalThis.R = f.length + ':' + f.bind().length");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: Infinity }); globalThis.R = f.length + ':' + f.bind(null, 1).length");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: -Infinity }); globalThis.R = f.length + ':' + f.bind(null, 1).length");
B("function f(a, b) {} Object.defineProperty(f, 'length', { value: '3' }); globalThis.R = f.length + ':' + f.bind().length");
B("function f(a, b) {} delete f.length; globalThis.R = f.bind().length + ':' + f.bind(null, 1, 2, 3).length");
B("function f(a, b) {} Object.defineProperty(f, 'name', { value: 7 }); globalThis.R = f.bind().name");
B("function f(a, b) {} Object.defineProperty(f, 'name', { value: 'n' }); globalThis.R = f.bind().name + ':' + f.bind().bind().name");
B("function f(a, b) {} delete f.name; globalThis.R = JSON.stringify(f.bind().name)");
B("function f(a, b) {} Object.defineProperty(f, 'name', { value: Symbol('s') }); globalThis.R = JSON.stringify(f.bind().name)");
B("function f(a, b) {} Object.defineProperty(f, 'name', { get() { return 'gn' }, configurable: true }); globalThis.R = f.bind().name");
B("function f(a, b) {} Object.defineProperty(f, 'length', { get() { throw new EvalError('len') }, configurable: true }); try { f.bind() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f(a, b) {} f.hasOwnProperty = () => { throw 1 }; globalThis.R = f.bind().length");
B("var f = new Proxy(function (a, b) {}, { getOwnPropertyDescriptor() { return undefined }, get(t, k) { return k === 'length' ? 5 : t[k] } }); globalThis.R = Function.prototype.bind.call(f).length");
B("var f = function (a, b) {}; Object.setPrototypeOf(f, null); try { globalThis.R = Function.prototype.bind.call(f).length } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = function () {}; Object.setPrototypeOf(f, Object.prototype); try { globalThis.R = typeof f.call + typeof f.bind } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() {} f.prototype = null; globalThis.R = String(Object.getPrototypeOf(new f()) === Object.prototype)");
B("function f() {} f.prototype = 1; globalThis.R = String(Object.getPrototypeOf(new f()) === Object.prototype)");
B("function f() {} f.prototype = function() {}; globalThis.R = String(typeof new f())");
B("function f() {} Object.defineProperty(f, 'prototype', { writable: false }); try { (function(){ 'use strict'; f.prototype = {} })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() {} globalThis.R = String(delete f.prototype)");
B("class C {} try { (function(){ 'use strict'; delete C.prototype })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class C {} try { C.prototype = {}; globalThis.R = String(C.prototype === Object.getPrototypeOf(new C())) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class C {} try { (function(){ 'use strict'; C.prototype = {} })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class C {} try { Object.defineProperty(C, 'prototype', { value: {} }) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class C {} try { Object.defineProperty(C, 'prototype', { value: C.prototype }); globalThis.R = 'same ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var a = () => 1; try { new a() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var a = { m() {} }; try { new a.m() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var a = async function() {}; try { new a() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var a = function*() {}; try { new a() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K {} try { K() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K {} try { K.call({}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K {} try { Reflect.apply(K, {}, []) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K {} try { K.apply(null) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K {} try { K.bind(null)() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("class K { constructor() { this.t = new.target === K } } globalThis.R = String(new (K.bind(null))().t)");
B("class K { constructor() { this.t = new.target === K } } var B2 = K.bind(null); globalThis.R = String(Reflect.construct(B2, [], K).t) + String(Reflect.construct(B2, [], B2).t)");
B("var f = function() {}; Object.setPrototypeOf(f, Function.prototype); globalThis.R = Function.prototype.toString.call(f)");
B("globalThis.R = String(Function.prototype.length) + String(Function.prototype.name === '') + typeof Function.prototype() + String(Function.prototype.hasOwnProperty('prototype'))");
B("try { new Function.prototype() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = Function.length + ':' + Function.name + ':' + Function.prototype.constructor.name + ':' + String(Function.prototype.constructor === Function)");
B("globalThis.R = String(Function.prototype.apply.length) + Function.prototype.call.length + Function.prototype.bind.length + Function.prototype.toString.length + Function.prototype[Symbol.hasInstance].length");
B("globalThis.R = Function.prototype.apply.name + ',' + Function.prototype.call.name + ',' + Function.prototype.bind.name + ',' + Function.prototype.toString.name + ',' + Function.prototype[Symbol.hasInstance].name");
B("var d = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance); globalThis.R = [d.writable, d.enumerable, d.configurable].join()");

// ---- 6. call / apply / bind.
B("function f(a, b) { return [this === globalThis, a, b].join() } globalThis.R = [f.call(), f.call(null, 1), f.call(undefined, 1, 2, 3), f.call({}, 1)].join('|')");
B("function f() { return arguments.length } globalThis.R = [f.apply(), f.apply(null), f.apply(null, undefined), f.apply(null, null), f.apply(null, []), f.apply(null, [1, 2]), f.apply(null, { length: 3 }), f.apply(null, { length: 2, 0: 'a' })].join()");
B("function f() { return arguments.length } try { f.apply(null, 1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, 'ab') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, true) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, Symbol()) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } globalThis.R = f.apply(null, function(a, b) {})");
B("function f() { return Array.prototype.join.call(arguments) } globalThis.R = f.apply(null, { length: 3 })");
B("function f() { return arguments.length } globalThis.R = f.apply(null, { length: -1 }) + ':' + f.apply(null, { length: '2' }) + ':' + f.apply(null, { length: 2.9 }) + ':' + f.apply(null, { length: NaN })");
B("function f() { return arguments.length } try { f.apply(null, { length: 2 ** 32 }) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, { length: 2 ** 53 }) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, { length: 1e6 }); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } try { f.apply(null, new Array(200000)); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function f() { return arguments.length } globalThis.R = f.apply(null, new Proxy([1, 2, 3], {}))");
B("var log = []; function f() { return arguments.length } var a = new Proxy({ length: 2 }, { get(t, k) { log.push(String(k)); return t[k] } }); f.apply(null, a); globalThis.R = log.join()");
B("function f() { return arguments.length } globalThis.R = f.apply(null, (function() { return arguments })(1, 2, 3))");
B("function f() { return arguments.length } globalThis.R = f.apply(null, new Uint8Array(3))");
B("function f() { return arguments.length } globalThis.R = f.apply(null, new Set([1, 2]))");
B("function f() { return arguments[0] } globalThis.R = String(f.apply(null, [, 1]))");
B("function f() { return 0 in arguments } globalThis.R = String(f.apply(null, [, 1]))");
B("try { Function.prototype.apply.call(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.call.call(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.bind.call(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.call.call({}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call({}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call(undefined) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call(null) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call(class {}) ; globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Function.prototype.toString.call(new Proxy({}, {})) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis.R = Function.prototype.toString.call(new Proxy(function() {}, {})) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis.R = Function.prototype.toString.call(new Proxy(class {}, {})) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis.R = Function.prototype.toString.call(new Proxy(() => 1, {})) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis.R = Function.prototype.toString.call(Proxy.revocable(function() {}, {}).proxy) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { globalThis.R = Function.prototype.toString.call(r.proxy) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { globalThis.R = typeof r.proxy } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { r.proxy() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { Function.prototype.call.call(r.proxy) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { Function.prototype.bind.call(r.proxy) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var r = Proxy.revocable(function() {}, {}); r.revoke(); try { ({}) instanceof r.proxy } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = function(a, b, c) { return [this, a, b, c].map(String).join() }; globalThis.R = [f.bind(1)(), f.bind(1, 2)(3), f.bind(1, 2, 3)(4, 5), f.bind()(), f.bind(undefined, undefined)()].join('|')");
B("var f = function(a, b, c) {}; globalThis.R = [f.bind().length, f.bind(null, 1).length, f.bind(null, 1, 2, 3, 4).length, f.bind().bind(null, 1).length, f.bind(null, 1).bind(null, 2).bind(null, 3).length].join()");
B("var f = function foo() {}; globalThis.R = [f.bind().name, f.bind().bind().name, f.bind().bind().bind().name, (() => {}).bind().name, (class A {}).bind().name, Math.max.bind().name, f.bind.bind(f)().name].join('|')");
B("var f = function() {}; globalThis.R = JSON.stringify([f.bind().name, (function*() {}).bind().name, ({ m() {} }).m.bind().name, ({ get g() { return 1 } }, 1), Object.getOwnPropertyDescriptor({ get g() { return 1 } }, 'g').get.bind().name])");
B("var f = function() {}; var b = f.bind(); globalThis.R = [b.hasOwnProperty('prototype'), typeof b.prototype, Object.getOwnPropertyNames(b).join(), Object.getPrototypeOf(b) === Function.prototype, Object.isExtensible(b)].join()");
B("var f = function() {}; Object.setPrototypeOf(f, Array.prototype); globalThis.R = String(Object.getPrototypeOf(Function.prototype.bind.call(f)) === Array.prototype)");
B("var P = { x: 1 }; var f = function() {}; Object.setPrototypeOf(f, P); globalThis.R = String(Object.getPrototypeOf(Function.prototype.bind.call(f)) === P)");
B("var f = function() {}; f.extra = 1; globalThis.R = String(f.bind().extra)");
B("function F(a, b) { this.a = a; this.b = b; this.t = new.target === F } var BF = F.bind({ ignored: 1 }, 1); var o = new BF(2); globalThis.R = JSON.stringify(o) + String(o instanceof F) + String(o instanceof BF) + String(Object.getPrototypeOf(o) === F.prototype)");
B("function F() { this.nt = new.target } var BF = F.bind(null); var o = new BF(); globalThis.R = String(o.nt === F) + String(o.nt === BF)");
B("function F() { return { r: 1 } } var BF = F.bind(null); globalThis.R = JSON.stringify(new BF())");
B("function F() { return 5 } var BF = F.bind(null); globalThis.R = String(new BF() instanceof F)");
B("function F() { this.x = 1 } F.prototype = { y: 2 }; var BF = F.bind(); globalThis.R = String(new BF().y)");
B("function F() {} var BF = F.bind(); F.prototype = null; globalThis.R = String(Object.getPrototypeOf(new BF()) === Object.prototype)");
B("function F() {} var BF = F.bind(); globalThis.R = String(BF.prototype) + String(new BF() instanceof BF) + String(Object.getPrototypeOf(new BF()) === F.prototype)");
B("function F() {} var BF = F.bind(); try { globalThis.R = String({} instanceof BF) + String(new F() instanceof BF) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function F() {} var BF = F.bind().bind().bind(); globalThis.R = String(new F() instanceof BF) + String(new BF() instanceof F) + String(new BF().constructor === F)");
B("var BF = (() => 1).bind(); try { new BF() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = ({ m() {} }).m.bind(); try { new BF() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = Math.max.bind(null, 1); globalThis.R = String(BF(5, 3)) + String(BF.length) + BF.name");
B("try { new (Math.max.bind())() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = Date.bind(null, 2020, 0); globalThis.R = String(new BF(5).getDate()) + typeof BF()");
B("var BF = Array.bind(null, 3); globalThis.R = String(new BF().length) + String(BF().length) + String(new BF(4).length)");
B("var BF = Map.bind(null, [[1, 2]]); globalThis.R = String(new BF().get(1))");
B("var BF = Map.bind(null); try { BF() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = Symbol.bind(null, 'x'); globalThis.R = BF().toString() + String(typeof BF.prototype)");
B("var BF = Symbol.bind(null, 'x'); try { new BF() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = Proxy.bind(null, {}); try { BF({}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var BF = Promise.bind(null, function(r) { r(1) }); try { globalThis.R = String(new BF() instanceof Promise) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = function() { return this }; var b = f.bind(5); globalThis.R = typeof b() + typeof b.call(7) + typeof b.apply(8) + typeof b.bind(9)()");
B("var f = function() { 'use strict'; return this }; var b = f.bind(5); globalThis.R = typeof b() + typeof b.call(7) + typeof b.apply(8) + typeof b.bind(9)()");
B("var f = function() { return this }; var b = f.bind(null); globalThis.R = String(b() === globalThis) + String(b.call({}) === globalThis)");
B("var f = function() { 'use strict'; return this }; var b = f.bind(null); globalThis.R = String(b()) + String(f.bind(undefined)()) + String(f.bind()())");
B("var f = function() { return arguments.length }; var many = new Array(70000).fill(1); try { globalThis.R = String(f.bind(null, ...many.slice(0, 5)).apply(null, many.slice(0, 10))) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = function() { return arguments.length }; var many = Array.from({ length: 300 }, (_, i) => i); globalThis.R = String(f.bind.apply(f, [null].concat(many))())");
B("function f() { return this } globalThis.R = String(typeof f.call.call(f, 1)) + String(f.call.call(f) === globalThis) + String(Function.prototype.call.call(function() { return arguments.length }, 1, 2, 3))");
B("globalThis.R = String(Function.prototype.call.call.call(function() { return 7 })) + String(Function.prototype.call.apply(function() { return 8 }))");
B("globalThis.R = String(Function.prototype.apply.call(function() { return arguments.length }, null, [1, 2, 3])) + String(Function.prototype.apply.apply(function() { return arguments.length }, [null, [1, 2]]))");
B("var f = Function.prototype.call; try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = Function.prototype.apply; try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = Function.prototype.bind; try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { f: Function.prototype.call }; try { o.f() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { m: 1 }; try { o.m.call() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = {}; try { o.m() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = {}; try { o.m.call() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { undefinedFunction() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (void 0)() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { null() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { (1)() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { 'abc'() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({})() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { [].x.y } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new (1)() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new ({})() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new Math.max() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new Symbol() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new BigInt(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new (async () => {})() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new globalThis() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new Reflect.apply() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { new JSON() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Math() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Reflect() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Atomics() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Intl() } catch (e) { globalThis.R = e.name + ': ' + e.message }");

// ---- 7. Symbol.hasInstance / instanceof.
B("globalThis.R = String(Function.prototype[Symbol.hasInstance].call(Array, [])) + String(Function.prototype[Symbol.hasInstance].call(Array, {})) + String(Function.prototype[Symbol.hasInstance].call({}, []))");
B("globalThis.R = String(Function.prototype[Symbol.hasInstance].call(1, 1)) + String(Function.prototype[Symbol.hasInstance].call(undefined, {}))");
B("globalThis.R = String(Function.prototype[Symbol.hasInstance].call(Object, 1)) + String(Function.prototype[Symbol.hasInstance].call(Number, new Number(1)))");
B("globalThis.R = String(Function.prototype[Symbol.hasInstance].call(function() {}.bind(), {}))");
B("function F() {} globalThis.R = String(F[Symbol.hasInstance](new F())) + String(F[Symbol.hasInstance]({})) + String(F[Symbol.hasInstance](1))");
B("function F() {} F.prototype = 1; try { F[Symbol.hasInstance]({}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function F() {} F.prototype = 1; globalThis.R = String(F[Symbol.hasInstance](1))");
B("function F() {} F.prototype = null; try { ({}) instanceof F } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("function F() {} F.prototype = undefined; try { ({}) instanceof F } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof 1 } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof {} } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof null } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof undefined } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { 1 in 1 } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { 'a' in 'abc' } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { 1 instanceof (() => 1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof (() => 1) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof ({ m() {} }).m } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof Math.max } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof (async function() {}) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { ({}) instanceof Symbol } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { globalThis.R = String(Symbol() instanceof Symbol) + String(Object(Symbol()) instanceof Symbol) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { [Symbol.hasInstance](v) { return v === 1 } }; globalThis.R = String(1 instanceof o) + String(2 instanceof o)");
B("var o = { [Symbol.hasInstance]: 1 }; try { 1 instanceof o } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { [Symbol.hasInstance]: null }; try { globalThis.R = String(1 instanceof o) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { [Symbol.hasInstance]: undefined }; try { globalThis.R = String(1 instanceof o) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { [Symbol.hasInstance]() { return 'yes' } }; globalThis.R = typeof (1 instanceof o) + String(1 instanceof o)");
B("var o = { [Symbol.hasInstance]() { return 0 } }; globalThis.R = String(1 instanceof o)");
B("var o = { [Symbol.hasInstance]() { return {} } }; globalThis.R = String(1 instanceof o)");
B("var o = { [Symbol.hasInstance]() { throw new RangeError('hi') } }; try { 1 instanceof o } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { get [Symbol.hasInstance]() { throw new SyntaxError('get') } }; try { 1 instanceof o } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = { [Symbol.hasInstance](v) { return this === o } }; globalThis.R = String(1 instanceof o)");
B("var o = { [Symbol.hasInstance](v) { return arguments.length + ':' + v } }; globalThis.R = String(7 instanceof o)");
B("var o = { [Symbol.hasInstance](v) { 'use strict'; return typeof this } }; globalThis.R = String(7 instanceof o)");
B("class C { static [Symbol.hasInstance](v) { return v === 'x' } } globalThis.R = String('x' instanceof C) + String(new C() instanceof C)");
B("class C { static [Symbol.hasInstance](v) { return false } } class D extends C {} globalThis.R = String(new D() instanceof C) + String(new D() instanceof D)");
B("class C { static get [Symbol.hasInstance]() { return () => true } } globalThis.R = String(1 instanceof C)");
B("function F() {} Object.defineProperty(F, Symbol.hasInstance, { value: () => true }); globalThis.R = String(1 instanceof F)");
B("function F() {} F[Symbol.hasInstance] = () => true; globalThis.R = String(1 instanceof F) + String(F.hasOwnProperty(Symbol.hasInstance))");
B("function F() {} Object.defineProperty(F, Symbol.hasInstance, { value: undefined }); globalThis.R = String(({}) instanceof F)");
B("function F() {} Object.defineProperty(F, Symbol.hasInstance, { value: null }); globalThis.R = String(new F() instanceof F)");
B("Object.defineProperty(Function.prototype, Symbol.hasInstance, { value: () => 'hijack', configurable: true }); try { globalThis.R = String({} instanceof Object) } finally { }");
B("var f = Function.prototype[Symbol.hasInstance]; try { Function.prototype[Symbol.hasInstance] = () => true; globalThis.R = String(({}) instanceof Array) + String(Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance).value === f) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var f = Function.prototype[Symbol.hasInstance]; try { (function(){ 'use strict'; Function.prototype[Symbol.hasInstance] = () => true })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var P = new Proxy(function() {}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); globalThis.R = String([] instanceof P)");
B("var P = new Proxy(function() {}, { getPrototypeOf() { return Array.prototype } }); globalThis.R = String(P instanceof Array)");
B("var p = new Proxy({}, { getPrototypeOf() { return Array.prototype } }); globalThis.R = String(p instanceof Array) + String(Array.isArray(p)) + String(Object.getPrototypeOf(p) === Array.prototype)");
B("var p = new Proxy({}, { getPrototypeOf() { throw new URIError('gp') } }); try { p instanceof Array } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var p = new Proxy({}, { getPrototypeOf() { return 1 } }); try { p instanceof Array } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var o = Object.create(null); globalThis.R = String(o instanceof Object) + String(Object.prototype.isPrototypeOf(o))");
B("var o = Object.create(Array.prototype); globalThis.R = String(o instanceof Array) + String(Array.isArray(o))");
B("function F() {} var o = new F(); Object.setPrototypeOf(o, null); globalThis.R = String(o instanceof F)");
B("function F() {} var G = function() {}; G.prototype = F.prototype; globalThis.R = String(new F() instanceof G)");
B("function F() {} var o = new F(); F.prototype = {}; globalThis.R = String(o instanceof F)");
B("globalThis.R = String(1 instanceof Number) + String(new Number(1) instanceof Number) + String('' instanceof String) + String(Object('') instanceof String) + String(null instanceof Object) + String(undefined instanceof Object)");
B("globalThis.R = String(function() {} instanceof Function) + String(function() {} instanceof Object) + String(Function instanceof Function) + String(Object instanceof Function) + String(Function instanceof Object)");
B("globalThis.R = String(Function.prototype instanceof Function) + String(Function.prototype instanceof Object) + String(Object.prototype instanceof Object) + String(typeof Function.prototype)");
B("globalThis.R = String(globalThis instanceof Object) + String(globalThis instanceof Function)");
B("globalThis.R = String(class {} instanceof Function) + String((() => 1) instanceof Function) + String(async function() {} instanceof Function) + String(function*() {} instanceof Function) + String(Math.max instanceof Function)");
B("var AF = Object.getPrototypeOf(async function() {}).constructor; globalThis.R = String((async function() {}) instanceof AF) + String(AF instanceof Function) + AF.name + String(AF.prototype.constructor === AF)");
B("var GF = Object.getPrototypeOf(function*() {}).constructor; globalThis.R = String((function*() {}) instanceof GF) + GF.name + GF.length + String(Object.getPrototypeOf(GF) === Function)");
B("var AGF = Object.getPrototypeOf(async function*() {}).constructor; globalThis.R = AGF.name + AGF.length + String(Object.getPrototypeOf(AGF) === Function) + String(typeof AGF('return 1'))");
B("var GF = Object.getPrototypeOf(function*() {}).constructor; globalThis.R = JSON.stringify([typeof GF.prototype, Object.prototype.toString.call(GF.prototype), GF.prototype[Symbol.toStringTag], Object.getOwnPropertyNames(GF.prototype).sort().join()])");
B("var GF = Object.getPrototypeOf(function*() {}).constructor; globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(GF.prototype, 'prototype') && Object.getOwnPropertyDescriptor(GF.prototype, 'prototype').writable) + String(Object.getOwnPropertyDescriptor(GF.prototype, 'prototype').configurable)");

// ---- 8. toString de funções nativas e de fonte.
const natives = [
  "Math.max", "Math.abs", "Object", "Object.keys", "Array", "Array.prototype.push", "Function", "Function.prototype", "Function.prototype.call", "Function.prototype.toString", "Symbol", "Symbol.prototype[Symbol.toPrimitive]",
  "Map", "Map.prototype.get", "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get", "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get", "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get", "Function.prototype[Symbol.hasInstance]", "Array.prototype[Symbol.iterator]", "RegExp.prototype[Symbol.match]", "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get",
  "Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get", "parseInt", "eval", "isNaN", "escape", "encodeURI", "JSON.parse", "Reflect.apply", "Promise", "Promise.resolve", "Proxy", "Date", "Date.now", "Error", "TypeError", "AggregateError",
  "BigInt", "Number", "String", "Boolean", "RegExp", "WeakMap", "Set", "ArrayBuffer", "Uint8Array", "DataView", "Atomics.add", "Intl.NumberFormat", "WebAssembly.Module",
  "Object.getPrototypeOf(Uint8Array)", "Object.getPrototypeOf(function*() {}).constructor", "Object.getPrototypeOf(async function() {}).constructor", "Object.getPrototypeOf(async function*() {}).constructor",
  "Object.getPrototypeOf([][Symbol.iterator]()).next", "Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))[Symbol.iterator]", "Proxy.revocable", "Function.prototype.bind.call(function() {})", "(function() {}).bind()",
  "Math.max.bind()", "(class {}).bind()", "Symbol.for", "String.prototype.at", "Array.prototype.toSorted", "Object.groupBy", "Array.fromAsync", "Promise.withResolvers", "globalThis.Iterator", "Math.f16round", "Float16Array", "Error.captureStackTrace",
];
for (const n of natives) {
  B(`var f = ${n}; globalThis.R = T(() => Function.prototype.toString.call(f))`);
  B(`var f = ${n}; globalThis.R = T(() => String(f))`);
  B(`var f = ${n}; globalThis.R = T(() => f.toString === Function.prototype.toString && f.toString().replace(/\\s+/g, ' ').length > 0)`);
  B(`var f = ${n}; globalThis.R = T(() => (f + '').indexOf('[native code]') >= 0)`);
}
const srcs = [
  "function f() {}", "function  f ( a ,b ) { return a }", "function /*c*/ f /*d*/ () /*e*/ {}", "function* g() {}", "async function h() {}", "async function* i() {}", "(a, b) => a + b", "a => a", "async a => a", "async (a) => { }",
  "({ m() {} }).m", "({ get g() { return 1 } })", "({ async *m() {} }).m", "({ ['a' + 1]() {} })['a1']", "({ 'str'() {} }).str", "({ 1() {} })[1]", "(class A { })", "(class A extends Object { constructor() { super() } })", "(class { static m() {} }).m",
  "(class { static #p() {}; static g() { return this.#p } }).g()", "class Q { x = 1 }; Q", "(class { get a() { return 1 } }).prototype", "function f(a = 1, { b }, [c], ...d) {}", "function f() { 'use strict'; }", "function\nf\n(\n)\n{\n}", "function f() { /* é */ }",
  "function f() { return `a${1}b` }", "function \\u0066() {}", "function fé() {}", "function \u{1d7d8}() {}".replace("\u{1d7d8}", "x"), "({ async m() {} }).m", "({ *[Symbol.iterator]() {} })[Symbol.iterator]", "(function () {})", "(function* () {})", "(async () => {})",
  "new Function('a', 'b', 'return a+b')", "new Function('a,b', 'return a')", "new Function('/*x*/a', '//c\\nreturn a')", "new Function('')", "new Function()", "new (Object.getPrototypeOf(function*(){}).constructor)('yield 1')",
  "new (Object.getPrototypeOf(async function(){}).constructor)('await 1')", "new (Object.getPrototypeOf(async function*(){}).constructor)('a', 'yield a')", "Function('a', 'b', 'c', 'return 1')", "new Function('a = 1', '{ b }', 'return a')",
];
for (const s of srcs) {
  B(`var f = (0, eval)(${q("(" + s + ")")}); globalThis.R = T(() => Function.prototype.toString.call(f))`);
}
for (const s of srcs.filter(x => !/^class Q/.test(x))) {
  B(`var f = (${s}); globalThis.R = T(() => String(f))`);
  B(`var f = (${s}); globalThis.R = T(() => typeof f === 'function' ? f.toString().length : typeof f)`);
}
B("var f = function() {}; f.toString = () => 'custom'; globalThis.R = String(f) + Function.prototype.toString.call(f)");
B("var o = { toString: Function.prototype.toString }; try { o.toString() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(Function.prototype.toString.call(Function.prototype))");
B("globalThis.R = String(Function.prototype.toString.call(Function.prototype.toString)) + '|' + Function.prototype.toString.toString()");
B("globalThis.R = Function.prototype + ''");
B("globalThis.R = String(Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get))");
B("globalThis.R = String(Function.prototype.toString.call(class { static x = 1 }))");
B("globalThis.R = Function.prototype.toString.call(function f() {}).length");
B("globalThis.R = `${function f(){ }}|${() => 1}|${class Z {}}`");
B("globalThis.R = [function(){}, () => {}].map(String).join('|')");
B("globalThis.R = JSON.stringify(function() {}) + JSON.stringify([function() {}]) + JSON.stringify({ f() {} })");
B("globalThis.R = String(Function.prototype.toString.call(Object.getOwnPropertyDescriptor({ get [Symbol('a')]() {} }, Object.getOwnPropertySymbols({ get [Symbol('a')]() {} })[0] || 'x') ))");
B("var s = Symbol('sym'); var o = { [s]() {} }; globalThis.R = String(o[s]) + ':' + o[s].name");
B("var s = Symbol('sym'); var o = { get [s]() { return 1 } }; globalThis.R = Object.getOwnPropertyDescriptor(o, s).get.name");
B("var s = Symbol('sym'); var o = { set [s](v) {} }; globalThis.R = Object.getOwnPropertyDescriptor(o, s).set.name");
B("var o = { get a() { return 1 }, set a(v) {} }; var d = Object.getOwnPropertyDescriptor(o, 'a'); globalThis.R = d.get.name + ',' + d.set.name + ',' + d.get.length + ',' + d.set.length + String(d.get.hasOwnProperty('prototype'))");
B("var o = { get a() { return 1 } }; var d = Object.getOwnPropertyDescriptor(o, 'a'); try { new d.get() } catch (e) { globalThis.R = e.name + ': ' + e.message }");

// ---- 9. Construtor Function / GeneratorFunction com argumentos de borda e globais acessados.
for (const args of [
  "", "'return 1'", "'a', 'return a'", "'a,b', 'return b'", "'a', 'b', 'return a+b'", "'a /*c*/', 'return a'", "'a //c\\n', 'return a'", "'...a', 'return a.length'", "'a=1', 'return a'", "'{a}', 'return a'", "'a', 'a', 'return a'",
  "'a', '\"use strict\"; return arguments.callee'", "'a, a', '\"use strict\"; return a'", "'a', '}'", "'a', '}{'", "'a', '} function g() {'", "'a) { return 1; } (function(', ''", "'/*', '*/){'", "'', '/*'", "'', '//'", "'a', '-->'", "'', '<!--'",
  "'a', 'return this'", "'return typeof globalThis'", "'return typeof arguments'", "'return new.target'", "'return super.x'", "'return await 1'", "'yield 1'", "'let a = 1; var a'", "'a', 'let a'", "'a', 'var a; return a'", "'\\u0061', 'return \\u0061'",
  "'a', 'return a', 'extra'", "undefined", "null", "'null'", "{ toString() { return 'return 5' } }", "{ toString() { throw new RangeError('ts') } }", "Symbol()", "'a', Symbol()", "1", "[ 'return 3' ]", "'return `a`'", "'#!x'", "'return 1;//'",
  "'return 1', 'return 2'", "'a', 'return a', 'b'", "'\\n', 'return 1'", "'a\\u2028', 'return 1'", "'if', 'return 1'", "'eval', 'return eval'", "'eval', '\"use strict\"; return eval'", "'arguments', 'return arguments'", "'arguments', '\"use strict\"; return 1'",
  "'a', '\"use strict\"; with(a) {}'", "'a', 'with(a) { return x }'", "'', 'return delete x'", "'', '\"use strict\"; return delete x'", "'a = 1, b', 'return b'", "'a', 'b = a', 'return b'",
]) {
  for (const ctor of ["Function", "Object.getPrototypeOf(function*(){}).constructor", "Object.getPrototypeOf(async function(){}).constructor", "Object.getPrototypeOf(async function*(){}).constructor"]) {
    const sel = ctor === "Function" ? "" : "gen";
    B(`globalThis.R = T(() => { var f = ${sel ? "new (" + ctor + ")" : "new Function"}(${args}); return typeof f + ':' + f.length + ':' + f.name + ':' + f.toString().replace(/\\n/g, '\\\\n') })`);
    if (!sel) B(`globalThis.R = T(() => { var f = Function(${args}); return typeof f + ':' + f.length + ':' + f.name + ':' + String(f()) })`);
  }
}

// ---- 10. Misc de globais de borda.
B("globalThis.R = typeof WebAssembly + typeof Intl + typeof Reflect");
B("globalThis.R = String(Object.getOwnPropertyDescriptor(globalThis, 'Object').enumerable) + String(Object.getOwnPropertyDescriptor(globalThis, 'Object').writable) + String(Object.getOwnPropertyDescriptor(globalThis, 'Object').configurable)");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'Math'); globalThis.R = [d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'NaN'); globalThis.R = [d.writable, d.enumerable, d.configurable, Object.is(d.value, NaN)].join()");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'undefined'); globalThis.R = [d.writable, d.enumerable, d.configurable, d.value === undefined].join()");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'Infinity'); globalThis.R = [d.writable, d.enumerable, d.configurable, d.value].join()");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'parseInt'); globalThis.R = [d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(globalThis, 'eval'); globalThis.R = [d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(Math, 'PI'); globalThis.R = [d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(Math, Symbol.toStringTag); globalThis.R = [d.value, d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(JSON, Symbol.toStringTag); globalThis.R = [d.value, d.writable, d.enumerable, d.configurable].join()");
B("var d = Object.getOwnPropertyDescriptor(Reflect, Symbol.toStringTag); globalThis.R = [d && d.value, d && d.configurable].join()");
B("globalThis.R = [Object.prototype.toString.call(Math), Object.prototype.toString.call(JSON), Object.prototype.toString.call(Reflect), Object.prototype.toString.call(Atomics), Object.prototype.toString.call(Intl), Object.prototype.toString.call(WebAssembly)].join()");
B("globalThis.R = [typeof Math, typeof JSON, typeof Reflect, typeof Atomics, typeof Intl].join()");
B("globalThis.R = Object.getOwnPropertyNames(Reflect).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Math).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(JSON).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Atomics).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Object.prototype).sort().join()");
B("globalThis.R = Object.getOwnPropertyNames(Number).sort().join()");
B("globalThis.R = ['parseInt', 'parseFloat', 'isNaN', 'isFinite', 'encodeURI', 'decodeURI', 'encodeURIComponent', 'decodeURIComponent', 'escape', 'unescape', 'eval'].map(k => k + ':' + typeof globalThis[k] + ':' + (globalThis[k] && globalThis[k].length)).join()");
B("var s = new Set(Object.getOwnPropertyNames(globalThis)); globalThis.R = ['Array', 'ArrayBuffer', 'BigInt', 'Boolean', 'DataView', 'Date', 'Error', 'EvalError', 'FinalizationRegistry', 'Float32Array', 'Float64Array', 'Function', 'Int16Array', 'Int32Array', 'Int8Array', 'Iterator', 'JSON', 'Map', 'Math', 'Number', 'Object', 'Promise', 'Proxy', 'RangeError', 'ReferenceError', 'Reflect', 'RegExp', 'Set', 'SharedArrayBuffer', 'String', 'Symbol', 'SyntaxError', 'TypeError', 'URIError', 'Uint16Array', 'Uint32Array', 'Uint8Array', 'Uint8ClampedArray', 'WeakMap', 'WeakRef', 'WeakSet', 'Atomics', 'Intl', 'AggregateError', 'WebAssembly', 'escape', 'unescape', 'globalThis', 'NaN', 'Infinity', 'undefined', 'eval'].filter(k => !s.has(k)).join() || 'all present'");
B("globalThis.R = String('Window' in globalThis) + String('document' in globalThis) + String('module' in globalThis) + String('exports' in globalThis) + String('__dirname' in globalThis)");
B("globalThis.R = String(typeof print) + String(typeof load) + String(typeof readFile) + String(typeof $vm) + String(typeof debug) + String(typeof gc)");
B("globalThis.R = String(globalThis.hasOwnProperty('R')) + String(Object.keys(globalThis).indexOf('R') >= 0)");
B("var a = 1; globalThis.R = String(Object.getOwnPropertyDescriptor(globalThis, 'a').configurable) + String(Object.getOwnPropertyDescriptor(globalThis, 'a').enumerable)");
B("function a() {} globalThis.R = String(Object.getOwnPropertyDescriptor(globalThis, 'a').configurable) + String(Object.getOwnPropertyDescriptor(globalThis, 'a').writable)");
B("globalThis.R = String(Object.getOwnPropertyDescriptor(globalThis, 'R').configurable)");
B("Object.freeze(globalThis); globalThis.R2 = 1; globalThis.R = String(typeof R2) + String(Object.isFrozen(globalThis))");
B("Object.preventExtensions(globalThis); try { newGlobal = 1; globalThis.R = 'created ' + typeof newGlobal } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("Object.preventExtensions(globalThis); try { (function() { 'use strict'; newGlobal = 1 })() } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("Object.preventExtensions(globalThis); try { (0, eval)('var newGlobal = 1') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("Object.preventExtensions(globalThis); try { (0, eval)('function newGlobal() {}') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("Object.preventExtensions(globalThis); try { (0, eval)('let newGlobal = 1; globalThis.R = typeof newGlobal') } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("Object.setPrototypeOf(globalThis, { inherited: 1 }); globalThis.R = String(inherited) + String('inherited' in globalThis) + String(globalThis.hasOwnProperty === undefined)");
B("var old = Object.getPrototypeOf(globalThis); Object.setPrototypeOf(globalThis, new Proxy({}, { has(t, k) { return k === 'ghost' }, get(t, k) { return k === 'ghost' ? 'boo' : undefined } })); globalThis.R = String(typeof ghost) + String(ghost)");
B("var old = Object.getPrototypeOf(globalThis); Object.setPrototypeOf(globalThis, new Proxy({}, { has(t, k) { return k === 'ghost' }, get(t, k) { return k === 'ghost' ? 'boo' : undefined } })); try { globalThis.R = String(typeof otherThing) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Object.setPrototypeOf(globalThis, globalThis) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Object.setPrototypeOf(globalThis, Object.create(globalThis)) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("globalThis.R = String(Object.setPrototypeOf(globalThis, Object.prototype) === globalThis)");
B("try { globalThis.__proto__ = null; globalThis.R = String(Object.getPrototypeOf(globalThis)) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("try { Object.setPrototypeOf(globalThis, null); globalThis.R = String(typeof Object) + String(typeof toString) + String(typeof hasOwnProperty) } catch (e) { globalThis.R = e.name + ': ' + e.message }");
B("var g = globalThis; globalThis.R = String(g.toString()) + Object.prototype.toString.call(this)");
B("globalThis.R = String(globalThis) + String(`${globalThis}`)");
B("globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, Symbol.toStringTag))");
B("globalThis.R = String(Object.getOwnPropertyNames(globalThis).filter(k => k.length === 1).sort().join())");
B("globalThis.R = typeof globalThis.Window + typeof globalThis.Global + typeof globalThis.GlobalObject");
B("globalThis.R = String(this === globalThis) + String(typeof this) + String(this.constructor === Object) + String(this.toString === Object.prototype.toString)");
B("globalThis.R = Object.getOwnPropertyNames(this).length > 40 ? 'many' : 'few'");
B("globalThis.R = String(Object.getOwnPropertyDescriptor(globalThis, 'escape') !== undefined) + String(Object.getOwnPropertyDescriptor(globalThis, 'unescape') !== undefined) + String(Object.getOwnPropertyDescriptor(globalThis, 'escape').enumerable)");
B("globalThis.R = String(escape === globalThis.escape) + String(unescape.call(null, '%41')) + String(escape.call({}, 'a b'))");
B("globalThis.R = [Object.getOwnPropertyNames(String.prototype).filter(k => /^(anchor|big|blink|bold|fixed|fontcolor|fontsize|italics|link|small|strike|sub|sup|substr|trimLeft|trimRight)$/.test(k)).sort().join()].join()");
B("globalThis.R = String(String.prototype.trimLeft === String.prototype.trimStart) + String(String.prototype.trimRight === String.prototype.trimEnd) + String.prototype.trimLeft.name + String.prototype.trimRight.name");
B("globalThis.R = String(Date.prototype.toGMTString === Date.prototype.toUTCString) + Date.prototype.toGMTString.name");
B("globalThis.R = String(Object.prototype.__lookupGetter__.length) + Object.prototype.__defineGetter__.name + String(typeof Object.prototype.__lookupSetter__)");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "global-edge-golden-"));
// O bun passa arquivos pelo transpilador próprio; `vm.runInThisContext` roda como ProgramExecutable do JSC puro. O
// SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "global_edge_source.js");
const file = path.join(dir, "global_edge_case.js");
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
const candidates = [];
const { usesHostApi } = require("./host-api.js");
const { sampleByHash } = require("./golden-prelude.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  candidates.push(body.replace(/\bR = /g, "globalThis.R = ").replace(/globalThis\.globalThis\.R/g, "globalThis.R"));
}
// A matriz completa passa de 500 programas; amostra por hash (sampleByHash; TARGET, padrão 520; TARGET=100000 gera tudo).
const TARGET = Number(process.env.TARGET || 520);
let kept = 0;
let dropped = 0;
for (const source of sampleByHash(candidates, TARGET)) {
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`candidatos ${candidates.length}, mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
