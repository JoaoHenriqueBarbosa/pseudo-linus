// Gera tests/golden/annexb_bun.tsv: Annex B e recursos legados (__proto__, __defineGetter__ e irmãos, métodos HTML de
// String, substr, trimLeft/trimRight, escape/unescape, getYear/setYear/toGMTString, RegExp legado, comentários HTML,
// octais, funções em blocos, labels, for-in com inicializador, with, arguments.callee, caller, toStringTag, datas
// legadas, split com limit, sort com undefined e buracos) medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Os programas rodam em modo sloppy (a menos que tragam "use strict"); `ev(code)` avalia por eval indireto e grava
// `Nome: mensagem` do erro, ou o valor; `tc(code)` faz o mesmo sem eval. Nada de caminho da máquina no resultado.
// Uso: bun scripts/gen-annexb-golden.js > tests/golden/annexb_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const q = JSON.stringify;
// Avalia `code` por eval indireto (global, sloppy); grava resultado ou erro.
const ev = code => `try { R = String((0, eval)(${q(code)})) } catch (e) { R = e.name + ": " + e.message }`;
// Avalia `expr` (JS) direto, grava String(resultado) ou erro.
const tc = expr => `try { R = String(${expr}) } catch (e) { R = e.name + ": " + e.message }`;
// Mesmo com JSON.stringify do resultado.
const tj = expr => `try { R = JSON.stringify(${expr}) } catch (e) { R = e.name + ": " + e.message }`;

// ---- __proto__
for (const code of [
  "({ __proto__: null }).__proto__",
  "Object.getPrototypeOf({ __proto__: null })",
  "Object.getPrototypeOf({ __proto__: Array.prototype }) === Array.prototype",
  "Object.getPrototypeOf({ __proto__: 1 }) === Object.prototype",
  "Object.getPrototypeOf({ __proto__: 'x' }) === Object.prototype",
  "Object.getPrototypeOf({ __proto__: undefined }) === Object.prototype",
  "Object.getPrototypeOf({ '__proto__': null })",
  "Object.getPrototypeOf({ \"__proto__\": null })",
  "Object.keys({ __proto__: {} }).length",
  "Object.keys({ ['__proto__']: 1 }).join()",
  "Object.getPrototypeOf({ ['__proto__']: null }) === Object.prototype",
  "var __proto__ = null; Object.getPrototypeOf({ __proto__ }) === Object.prototype",
  "var __proto__ = 5; Object.keys({ __proto__ }).join()",
  "Object.keys({ __proto__() {} }).join()",
  "Object.getPrototypeOf({ __proto__() {} }) === Object.prototype",
  "Object.keys({ get __proto__() { return 1 } }).join()",
  "({ get __proto__() { return 7 } }).__proto__",
  "Object.keys({ set __proto__(v) {} }).join()",
  "({ __proto__: 1, __proto__: 2 })",
  "({ __proto__: null, ['__proto__']: 2 }).__proto__",
  "({ __proto__: null, __proto__() {} })",
  "({ __proto__: null, get __proto__() {} })",
  "({ __proto__: null, '__proto__': 1 })",
  "({ __proto__: null, \"__proto__\": 1 })",
  "({ __proto__: null, __proto__ })",
  "var __proto__; ({ __proto__: null, __proto__ })",
  "({ __proto__: null, ...{ __proto__: 1 } })",
  "({ __proto__: a, __proto__: b } = {})",
  "({ __proto__: a, __proto__: b } = { __proto__: 1 })",
  "var a, b; ({ __proto__: a, __proto__: b } = {}); 'ok'",
  "[{ __proto__: a, __proto__: b }] = [{}]",
  "(function f({ __proto__: a, __proto__: b }) {})",
  "(({ __proto__: a, __proto__: b }) => 1)",
  "(({ __proto__: a, __proto__: b }) => 1) ; ({ __proto__: a, __proto__: b }) => 1",
  "({ __proto__: a, __proto__: b }) => 1",
  "({ __proto__: 1, __proto__: 2 }) => 1",
  "for ({ __proto__: a, __proto__: b } of []) ;",
  "var { __proto__: a, __proto__: b } = {}",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.name",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.name",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.length",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.length",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').enumerable",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').configurable",
  "'value' in Object.getOwnPropertyDescriptor(Object.prototype, '__proto__')",
  "typeof Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
  "typeof Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.call(1) === Number.prototype",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.call('s') === String.prototype",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.call(null)",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.call(undefined)",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call(null, {})",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call(undefined, {})",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call(1, {})",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call({}, 1)",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call({})",
  "var o = {}; o.__proto__ = Array.prototype; o instanceof Array",
  "var o = {}; o.__proto__ = null; Object.getPrototypeOf(o)",
  "var o = {}; o.__proto__ = 1; Object.getPrototypeOf(o) === Object.prototype",
  "var o = {}; o.__proto__ = undefined; Object.getPrototypeOf(o) === Object.prototype",
  "var o = Object.create(null); o.__proto__ = {}; Object.keys(o).join()",
  "var o = Object.create(null); o.__proto__ = {}; Object.getPrototypeOf(o)",
  "var o = Object.create(null); o.__proto__",
  "var o = Object.preventExtensions({}); o.__proto__ = {}",
  "'use strict'; var o = Object.preventExtensions({}); o.__proto__ = {}",
  "var o = Object.preventExtensions({}); o.__proto__ = Object.prototype; 'same'",
  "var a = {}, b = Object.create(a); a.__proto__ = b",
  "var a = {}; a.__proto__ = a",
  "var a = {}, b = Object.create(a), c = Object.create(b); a.__proto__ = c",
  "'__proto__' in {}",
  "({}).hasOwnProperty('__proto__')",
  "Object.prototype.hasOwnProperty('__proto__')",
  "JSON.stringify(Object.getOwnPropertyNames(Object.prototype).includes('__proto__'))",
  "JSON.stringify(JSON.parse('{\"__proto__\": 1}'))",
  "Object.keys(JSON.parse('{\"__proto__\": 1}')).join()",
  "Object.getPrototypeOf(JSON.parse('{\"__proto__\": null}')) === Object.prototype",
  "JSON.parse('{\"__proto__\": null}').__proto__",
  "Object.assign({}, { __proto__: 1 }).hasOwnProperty('__proto__')",
  "Object.assign({}, JSON.parse('{\"__proto__\": 1}')).hasOwnProperty('__proto__')",
  "Object.getPrototypeOf(Object.assign({}, JSON.parse('{\"__proto__\": null}'))) === Object.prototype",
  "Object.getPrototypeOf({ ...JSON.parse('{\"__proto__\": null}') }) === Object.prototype",
  "({ ...JSON.parse('{\"__proto__\": 3}') }).hasOwnProperty('__proto__')",
  "var f = function () {}; Object.getPrototypeOf({ __proto__: f }) === f",
  "class A {}; Object.getPrototypeOf({ __proto__: A.prototype }) === A.prototype",
  "class A { __proto__ = 1 }; Object.keys(new A()).join()",
  "class A { ['__proto__'] = 1 }; Object.getPrototypeOf(new A()) === A.prototype",
  "class A { static __proto__() {} }; typeof A.__proto__",
  "var o = { __proto__: null }; 'toString' in o",
  "var o = { __proto__: null }; typeof o.__proto__",
  "var o = { a: 1, __proto__: { b: 2 } }; o.b",
  "var o = { __proto__: { b: 2 }, b: 3 }; o.b",
  "var o = { __proto__: { get g() { return this.x } }, x: 9 }; o.g",
  "Object.getOwnPropertyDescriptor({ __proto__: 1 }, '__proto__')",
  "typeof Object.getOwnPropertyDescriptor({ ['__proto__']: 1 }, '__proto__')",
  "Object.getOwnPropertyDescriptor({ ['__proto__']: 1 }, '__proto__').value",
  "Object.entries({ __proto__: [], x: 1 }).join()",
  "Reflect.getPrototypeOf({ __proto__: null })",
  "Reflect.setPrototypeOf({}, null)",
  "Reflect.set({}, '__proto__', 5)",
  "var o = {}; Reflect.set(o, '__proto__', Array.prototype); o instanceof Array",
  "delete Object.prototype.__proto__; var o = {}; o.__proto__ = 4; Object.keys(o).join()",
  "var o = { __proto__: null }; o.__proto__ = 4; Object.keys(o).join()",
  "Object.getOwnPropertyNames({ __proto__: null, a: 1 }).join()",
  "new Proxy({}, {}).__proto__ === Object.prototype",
  "var p = new Proxy({}, { getPrototypeOf() { return Array.prototype } }); p.__proto__ === Array.prototype",
  "var p = new Proxy({}, { setPrototypeOf() { return false } }); p.__proto__ = {}",
  "'use strict'; var p = new Proxy({}, { setPrototypeOf() { return false } }); p.__proto__ = {}",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call(Object.freeze({}), {})",
]) add(ev(code));
for (const code of ["({ __proto__: null, __proto__: null })", "({ __proto__: 1, '__proto__': 2 })", "({ '__proto__': 1, \"__proto__\": 2 })", "({ __proto__: 1, __proto__ : 2, a: 3 })", "x = { __proto__: 1, __proto__: 2 }", "f({ __proto__: 1, __proto__: 2 })", "[{ __proto__: 1, __proto__: 2 }]", "({ a: { __proto__: 1, __proto__: 2 } })", "({ __proto__: 1, __proto__ })", "({ __proto__, __proto__: 1 })", "({ __proto__, __proto__ })", "({ __proto__: 1, set __proto__(v) {} })", "({ __proto__: 1, async __proto__() {} })", "({ __proto__: 1, *__proto__() {} })", "({ __proto__: 1, ['__proto__']: 2, __proto__: 3 })"]) add(ev(code));

// ---- __defineGetter__ e irmãos
for (const code of [
  "var o = {}; o.__defineGetter__('a', function () { return 5 }); o.a",
  "var o = {}; o.__defineSetter__('a', function (v) { this.b = v }); o.a = 3; o.b",
  "var o = {}; o.__defineGetter__('a', function () {}); JSON.stringify(Object.getOwnPropertyDescriptor(o, 'a'), (k, v) => typeof v === 'function' ? 'fn' : v)",
  "var o = {}; o.__defineSetter__('a', function () {}); JSON.stringify(Object.getOwnPropertyDescriptor(o, 'a'), (k, v) => typeof v === 'function' ? 'fn' : v)",
  "var o = {}; o.__defineGetter__('a', function () {}); Object.getOwnPropertyDescriptor(o, 'a').enumerable",
  "var o = {}; o.__defineGetter__('a', function () {}); Object.getOwnPropertyDescriptor(o, 'a').configurable",
  "var o = {}; o.__defineGetter__('a', 1)",
  "var o = {}; o.__defineGetter__('a')",
  "var o = {}; o.__defineGetter__('a', {})",
  "var o = {}; o.__defineGetter__('a', undefined)",
  "var o = {}; o.__defineGetter__('a', null)",
  "var o = {}; o.__defineSetter__('a', 1)",
  "var o = {}; o.__defineSetter__('a')",
  "var o = {}; o.__defineSetter__('a', 'x')",
  "Object.prototype.__defineGetter__.call(null, 'a', function () {})",
  "Object.prototype.__defineGetter__.call(undefined, 'a', function () {})",
  "Object.prototype.__defineSetter__.call(null, 'a', function () {})",
  "Object.prototype.__defineGetter__.call(1, 'a', function () {})",
  "Object.prototype.__defineGetter__.call(Object.freeze({}), 'a', function () {})",
  "Object.prototype.__defineGetter__.call(Object.preventExtensions({}), 'a', function () {})",
  "var o = {}; Object.defineProperty(o, 'a', { value: 1 }); o.__defineGetter__('a', function () {})",
  "var o = {}; Object.defineProperty(o, 'a', { value: 1, configurable: true }); o.__defineGetter__('a', function () { return 2 }); o.a",
  "var o = { a: 1 }; o.__defineGetter__('a', function () { return 2 }); o.a",
  "var o = { a: 1 }; o.__defineGetter__('a', function () { return 2 }); Object.keys(o).join()",
  "var o = {}; o.__defineGetter__('a', function () { return 2 }); o.__defineSetter__('a', function () {}); typeof Object.getOwnPropertyDescriptor(o, 'a').get + typeof Object.getOwnPropertyDescriptor(o, 'a').set",
  "var o = {}; var s = Symbol('s'); o.__defineGetter__(s, function () { return 4 }); o[s]",
  "var o = {}; o.__defineGetter__({ toString() { return 'k' } }, function () { return 4 }); o.k",
  "var o = {}; o.__defineGetter__({ toString() { throw new RangeError('boom') } }, function () {})",
  "var o = {}; o.__defineGetter__('a', function () {}); o.__defineGetter__.length",
  "Object.prototype.__defineGetter__.name + Object.prototype.__defineSetter__.name",
  "Object.prototype.__defineGetter__.length + '' + Object.prototype.__defineSetter__.length + Object.prototype.__lookupGetter__.length + Object.prototype.__lookupSetter__.length",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__defineGetter__').enumerable",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__defineGetter__').writable",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__lookupGetter__').configurable",
  "new Object.prototype.__defineGetter__('a', function () {})",
  "var o = {}; o.__defineGetter__('a', function () {}) === undefined",
  "var o = { get a() { return 1 } }; typeof o.__lookupGetter__('a')",
  "var o = { get a() { return 1 } }; o.__lookupGetter__('a')()",
  "var o = { get a() { return 1 } }; o.__lookupSetter__('a')",
  "var o = { set a(v) {} }; typeof o.__lookupSetter__('a')",
  "var o = { set a(v) {} }; o.__lookupGetter__('a')",
  "var o = { a: 1 }; o.__lookupGetter__('a')",
  "var o = { a: 1 }; o.__lookupSetter__('a')",
  "var o = { a: 1 }; o.__lookupGetter__('b')",
  "var p = { get a() { return 1 } }; var o = Object.create(p); typeof o.__lookupGetter__('a')",
  "var p = { get a() { return 1 } }; var o = Object.create(p); o.a = 5; typeof o.__lookupGetter__('a')",
  "var p = { get a() { return 1 } }; var o = Object.create(p); Object.defineProperty(o, 'a', { value: 2 }); o.__lookupGetter__('a')",
  "var p = { get a() { return 1 } }; var o = Object.create(p); Object.defineProperty(o, 'a', { value: 2 }); o.__lookupSetter__('a')",
  "Object.prototype.__lookupGetter__.call(null, 'a')",
  "Object.prototype.__lookupGetter__.call(undefined, 'a')",
  "Object.prototype.__lookupSetter__.call(null, 'a')",
  "Object.prototype.__lookupGetter__.call(1, 'a')",
  "Object.prototype.__lookupGetter__.call('str', 'length')",
  "typeof Object.prototype.__lookupGetter__.call(Object.prototype, '__proto__')",
  "typeof Object.prototype.__lookupSetter__.call({}, '__proto__')",
  "typeof ({}).__lookupGetter__('__proto__')",
  "({}).__lookupGetter__('__proto__').name",
  "({}).__lookupSetter__('__proto__').name",
  "var s = Symbol('t'); var o = { get [s]() { return 1 } }; typeof o.__lookupGetter__(s)",
  "var o = { get [1]() { return 1 } }; typeof o.__lookupGetter__(1)",
  "var o = { get [1]() { return 1 } }; typeof o.__lookupGetter__('1')",
  "var o = { get a() { return 1 } }; o.__lookupGetter__({ toString() { return 'a' } }) === Object.getOwnPropertyDescriptor(o, 'a').get",
  "var o = {}; o.__lookupGetter__({ toString() { throw new TypeError('t') } })",
  "var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { get() { return 1 }, configurable: true } } }); typeof Object.prototype.__lookupGetter__.call(p, 'z')",
  "var log = []; var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { log.push('gopd:' + k); return undefined }, getPrototypeOf(t) { log.push('gpo'); return null } }); Object.prototype.__lookupGetter__.call(p, 'z'); log.join()",
  "var o = {}; o.__defineGetter__('a', function () { return this === o }); o.a",
  "var o = {}; o.__defineSetter__('a', function (v) { R2 = this === o }); o.a = 1; R2",
  "var o = {}; o.__defineGetter__('length', function () { return 9 }); o.length",
  "var a = []; a.__defineGetter__('length', function () { return 9 })",
  "var a = []; a.__defineGetter__('0', function () { return 9 }); a[0]",
  "var f = function () {}; f.__defineGetter__('name', function () { return 'n' }); f.name",
  "var o = Object.create({}, { a: { value: 1, configurable: false } }); o.__defineSetter__('a', function () {})",
  "var o = {}; o.__defineGetter__('a', function () { return 1 }); o.__defineGetter__('a', function () { return 2 }); o.a",
  "var o = {}; o.__defineGetter__('a', function () { return 1 }); o.__defineSetter__('a', function () {}); o.a",
  "var o = {}; o.__defineGetter__('a', function () { return 1 }); delete o.a; o.a",
  "var o = {}; o.__defineGetter__('a', function () { return 1 }); o.a = 5; o.a",
  "'use strict'; var o = {}; o.__defineGetter__('a', function () { return 1 }); o.a = 5",
]) add(ev(code));

// ---- Métodos HTML de String
const htmlNoArg = ["big", "blink", "bold", "fixed", "italics", "small", "strike", "sub", "sup"];
const htmlArg = { anchor: "name", fontcolor: "color", fontsize: "size", link: "href" };
const subjects = ["'abc'", "''", "'a\"b'", "'a\"\"b\"'", "'<&>'", "'\\u0000'", "'\\n'", "123", "true", "null", "undefined", "({ toString() { return 'ts' } })", "Symbol('s')", "[1, 2]", "'\\u{1F600}'"];
for (const m of htmlNoArg) {
  for (const s of subjects.slice(0, 7)) add(tc(`String.prototype.${m}.call(${s})`));
  add(tc(`'x'.${m}('extra')`));
  add(tc(`String.prototype.${m}.call(null)`));
  add(tc(`String.prototype.${m}.call(undefined)`));
  add(tc(`String.prototype.${m}.length + ' ' + String.prototype.${m}.name`));
  add(tc(`new String.prototype.${m}()`));
  add(tc(`Object.getOwnPropertyDescriptor(String.prototype, '${m}').enumerable`));
}
const attrs = ["'v'", "'a\"b'", "'a\"\"b'", "''", "'&quot;'", "'\\\"'", "undefined", "null", "123", "({ toString() { return 'q\"' } })", "Symbol('s')", "'<>'", "'\\n'", "'\\u0022'"];
for (const [m] of Object.entries(htmlArg)) {
  for (const a of attrs) add(tc(`'abc'.${m}(${a})`));
  add(tc(`'abc'.${m}()`));
  add(tc(`'a\"b'.${m}('c\"d')`));
  add(tc(`String.prototype.${m}.call(null, 'x')`));
  add(tc(`String.prototype.${m}.call(undefined, 'x')`));
  add(tc(`String.prototype.${m}.call(12, 'x')`));
  add(tc(`String.prototype.${m}.length + ' ' + String.prototype.${m}.name`));
  add(tc(`'abc'.${m}({ toString() { throw new RangeError('boom') } })`));
}
add(tc("'x'.anchor('a', 'b')"));
add(tc("'abc'.fontsize(7)"));
add(tc("'abc'.fontcolor('#ff0000')"));
add(tc("'abc'.link('http://a/b?c=\"d\"')"));
add(tc("'a'.bold().italics().sub()"));
add(tc("'a'.sup().sup()"));
add(tc("'a'.anchor('x').link('y')"));
add(tc("typeof String.prototype.big"));
add(tc("Object.getOwnPropertyNames(String.prototype).filter(n => ['anchor','big','blink','bold','fixed','fontcolor','fontsize','italics','link','small','strike','sub','sup'].includes(n)).length"));

// ---- substr, trimLeft/trimRight
const substrCases = [
  "'abcdef'.substr(2)", "'abcdef'.substr(2, 3)", "'abcdef'.substr(-2)", "'abcdef'.substr(-2, 1)", "'abcdef'.substr(-10, 2)",
  "'abcdef'.substr(10)", "'abcdef'.substr(0, 0)", "'abcdef'.substr(0, -1)", "'abcdef'.substr(1, Infinity)", "'abcdef'.substr(-Infinity, 2)",
  "'abcdef'.substr(Infinity)", "'abcdef'.substr(NaN, 2)", "'abcdef'.substr(2, NaN)", "'abcdef'.substr(2, undefined)", "'abcdef'.substr(undefined, 2)",
  "'abcdef'.substr('1', '2')", "'abcdef'.substr(1.9, 2.9)", "'abcdef'.substr(-1.5, 1)", "'abcdef'.substr()", "'abcdef'.substr(null, 1)",
  "'abcdef'.substr(2, null)", "'abcdef'.substr(6)", "'abcdef'.substr(5, 5)", "'abcdef'.substr(2 ** 32, 1)", "'abcdef'.substr(0, 2 ** 32)",
  "''.substr(0)", "''.substr(-1, 5)", "'\\u{1F600}x'.substr(1, 1).length", "'\\u{1F600}x'.substr(0, 1).charCodeAt(0)",
  "String.prototype.substr.call(null)", "String.prototype.substr.call(undefined, 1)", "String.prototype.substr.call(12345, 1, 2)",
  "String.prototype.substr.call(true, 1)", "String.prototype.substr.call({ toString() { return 'obj' } }, 1)",
  "String.prototype.substr.length", "String.prototype.substr.name", "'abc'.substr({ valueOf() { return 1 } })",
  "'abc'.substr(Symbol())", "'abc'.substr(1n)", "'abcdef'.substr(-0, 2)", "'abcdef'.substr(1, -0)", "'abcdef'.substr(-6, 6)", "'abcdef'.substr(-7, 6)",
  "'abcdef'.substr(3, 1e21)", "'abcdef'.substr(-1e21)", "'abcdef'.substr(1e21)",
];
for (const c of substrCases) add(tc(c));
for (const n of ["trimLeft", "trimRight"]) {
  const base = n === "trimLeft" ? "trimStart" : "trimEnd";
  for (const s of ["'  a  '", "'\\t\\n a \\u00a0\\u2003\\ufeff'", "''", "'   '", "'\\u180e a \\u180e'", "'\\u200b a \\u200b'", "'a'", "'\\u2028\\u2029a\\u2028'", "'\\u0085a\\u0085'", "'\\v\\fa\\v\\f'"]) {
    add(tj(`'[' + ${s}.${n}() + ']'`));
  }
  add(tc(`String.prototype.${n} === String.prototype.${base}`));
  add(tc(`String.prototype.${n}.name`));
  add(tc(`String.prototype.${n}.length`));
  add(tc(`String.prototype.${n}.call(null)`));
  add(tc(`String.prototype.${n}.call(undefined)`));
  add(tc(`String.prototype.${n}.call(12)`));
  add(tc(`Object.getOwnPropertyDescriptor(String.prototype, '${n}').enumerable`));
  add(tc(`Object.getOwnPropertyDescriptor(String.prototype, '${n}').writable`));
  add(tc(`Object.getOwnPropertyDescriptor(String.prototype, '${n}').configurable`));
  add(tc(`Object.getOwnPropertyNames(String.prototype).includes('${n}')`));
  add(tc(`new String.prototype.${n}()`));
}
add(tc("String.prototype.trimStart.name"));
add(tc("String.prototype.trimEnd.name"));
add(tc("String.prototype.trimLeft.name + String.prototype.trimRight.name"));

// ---- escape/unescape
const escCases = [
  "'abc'", "'a b'", "'a+b'", "'a/b'", "'a@b*c_d-e.f'", "'äöü'", "'\\u0100'", "'\\uffff'", "'\\u{1F600}'", "'\\ud800'", "'\\udc00'", "''",
  "'%'", "'%%'", "'~!#$&()=?'", "'\\x00\\x01\\x7f\\x80\\xff'", "'a\\nb\\tc'", "'1234567890'", "'ABCxyz'", "'[]{}<>'", "'\\''", "'\"'", "'`'", "'^|\\\\'", "'\\u00e9\\u0301'", "'\\u20ac'", "'日本語'", "'a\\u0000b'", "'\\u00ff\\u0100'", "'+ -'",
];
for (const c of escCases) {
  add(tc(`escape(${c})`));
  add(tc(`unescape(escape(${c})) === ${c}`));
}
const unescCases = [
  "'%41'", "'%u0041'", "'%u00e9'", "'%U0041'", "'%4'", "'%'", "'%zz'", "'%u004'", "'%u00zz'", "'%u'", "'%%41'", "'%4%41'", "'%E9'", "'%e9'", "'%u20AC'", "'%u20ac'", "'%uD83D%uDE00'", "'%uD83D'", "'%00'", "'a%20b'", "'a%2'", "'%2g'", "'%u0'", "'%u00411'", "'%411'", "'%u+041'", "'%+41'", "'%-4'", "'%0x'", "'+'", "'%7e'", "'%7E'", "'%u007E'", "'%25'", "'%2525'", "'%u0025'", "'%ug'", "'%u 041'", "'% 41'",
];
for (const c of unescCases) add(tj(`unescape(${c})`));
add(tc("escape.length + ' ' + escape.name + ' ' + unescape.length + ' ' + unescape.name"));
add(tc("escape()"));
add(tc("unescape()"));
add(tc("escape(null)"));
add(tc("escape({ toString() { return 'a b' } })"));
add(tc("escape(Symbol())"));
add(tc("unescape(Symbol())"));
add(tc("escape(1n)"));
add(tc("new escape('a')"));
add(tc("typeof globalThis.escape + typeof globalThis.unescape"));
add(tc("Object.getOwnPropertyDescriptor(globalThis, 'escape').enumerable"));
add(tc("Object.getOwnPropertyDescriptor(globalThis, 'unescape').writable"));
add(tc("Object.getOwnPropertyDescriptor(globalThis, 'escape').configurable"));

// ---- Date legado
const dateCases = [
  "new Date(2020, 0, 1).getYear()", "new Date(1999, 0, 1).getYear()", "new Date(1900, 0, 1).getYear()", "new Date(1899, 0, 1).getYear()", "new Date(2100, 5, 1).getYear()",
  "new Date(NaN).getYear()", "Date.prototype.getYear.call({})", "Date.prototype.getYear.call(null)", "Date.prototype.getYear.length + ' ' + Date.prototype.getYear.name",
  "var d = new Date(2000, 0, 1); d.setYear(99); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(100); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(0); d.getFullYear()",
  "var d = new Date(2000, 0, 1); d.setYear(1999); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(-1); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(NaN); d.getTime()",
  "var d = new Date(2000, 0, 1); d.setYear(); d.getTime()", "var d = new Date(2000, 0, 1); d.setYear('50'); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(50.9); d.getFullYear()",
  "var d = new Date(NaN); d.setYear(2001); d.getFullYear()", "var d = new Date(NaN); d.setYear(99); d.getFullYear()", "var d = new Date(NaN); d.setYear(99); d.getMonth()",
  "var d = new Date(2000, 5, 15); d.setYear(2010); d.getMonth() + '-' + d.getDate()", "var d = new Date(2000, 1, 29); d.setYear(2001); d.getMonth() + '-' + d.getDate()",
  "var d = new Date(2000, 0, 1); d.setYear(99)", "Date.prototype.setYear.call({}, 1)", "Date.prototype.setYear.length + ' ' + Date.prototype.setYear.name",
  "Date.prototype.toGMTString === Date.prototype.toUTCString", "Date.prototype.toGMTString.name", "Date.prototype.toGMTString.length",
  "new Date(0).toGMTString()", "new Date(NaN).toGMTString()", "new Date(8.64e15).toGMTString()", "new Date(-1).toGMTString()", "new Date(-62198755200000).toGMTString()",
  "Date.prototype.toGMTString.call({})", "Date.prototype.toGMTString.call(1)", "Object.getOwnPropertyDescriptor(Date.prototype, 'toGMTString').enumerable",
  "Object.getOwnPropertyDescriptor(Date.prototype, 'getYear').writable", "Object.getOwnPropertyNames(Date.prototype).includes('setYear')",
  "new Date('2024-1-1').getTime() === new Date(2024, 0, 1).getTime()", "new Date('2024-1-1').getDate()", "new Date('2024-01-1').getDate()", "new Date('2024-1-01').getDate()",
  "new Date('2024-13-1').getTime()", "new Date('2024/1/1').getDate()", "new Date('1/2/2024').getMonth()", "new Date('1/2/2024').getDate()", "new Date('01/02/2024').getFullYear()",
  "new Date('2024-01-01').getTime()", "new Date('2024-01-01T00:00:00').getTime() === new Date(2024, 0, 1).getTime()", "new Date('2024-01-01T00:00:00Z').getTime()",
  "new Date('2024-1-1 10:20').getHours()", "new Date('2024-1-1 10:20:30').getSeconds()", "new Date('2024-1-1T10:20').getTime()", "new Date('Jan 1 2024').getFullYear()",
  "new Date('January 1, 2024').getMonth()", "new Date('1 Jan 2024').getDate()", "new Date('Mon, 01 Jan 2024 00:00:00 GMT').getTime()", "new Date('Mon Jan 01 2024').getDate()",
  "new Date('2024').getTime()", "new Date('2024-02').getTime()", "new Date('2024-02-30').getTime()", "new Date('2024-02-29').getTime()", "new Date('2023-02-29').getTime()",
  "new Date('99').getTime()", "new Date('1').getMonth()", "new Date('12').getMonth()", "new Date('13').getTime()", "new Date('0').getFullYear()", "new Date('49').getFullYear()", "new Date('50').getFullYear()",
  "new Date('1/1/49').getFullYear()", "new Date('1/1/50').getFullYear()", "new Date('1/1/99').getFullYear()", "new Date('1/1/00').getFullYear()", "new Date('1/1/100').getFullYear()",
  "new Date('Jan 1 49').getFullYear()", "new Date('Jan 1 50').getFullYear()", "new Date('Jan 1 1').getFullYear()", "new Date('abc').getTime()", "new Date('').getTime()", "new Date(' ').getTime()",
  "new Date('2024-1-1 GMT').getTime()", "new Date('2024-1-1 UTC').getTime()", "new Date('2024-1-1 +0100').getTime()", "new Date('2024-1-1 10:00 PM').getHours()", "new Date('2024-1-1 12:00 AM').getHours()",
  "new Date('Jan 1 2024 10:00 EST').getTime()", "new Date('Jan 1 2024 10:00 PST').getTime()", "new Date('Jan 1 2024 10:00 GMT+0200').getTime()", "new Date('Jan 1 2024 (comment) 10:00 GMT').getTime()",
  "new Date('Tue Jan 01 2024 00:00:00 GMT+0000 (UTC)').getTime()", "new Date('Sat, 01 Jan 2000 00:00:00 GMT').toISOString()", "new Date('2000-01-01T00:00:00.5Z').getMilliseconds()",
  "new Date('2000-01-01T24:00:00Z').getTime()", "new Date('2000-01-01T24:00:01Z').getTime()", "new Date('+002000-01-01T00:00:00Z').getTime()", "new Date('-000000-01-01T00:00:00Z').getTime()",
  "new Date('2000-01-01T00:00Z').getTime()", "new Date('2000-01-01T00Z').getTime()", "new Date('2000-01-01T00:00:00+01:00').getTime()", "new Date('2000-01-01T00:00:00+0100').getTime()",
  "Date.parse('2024-1-1') === Date.parse('1/1/2024')", "Date.parse('Thu, 01 Jan 1970 00:00:00 GMT')", "Date.parse('1970-01-01T00:00:00.000Z')", "Date.parse('Jan 1, 1970 UTC')",
  "Date.parse('1970 Jan 1 UTC')", "Date.parse('1970-1-1 UTC')", "Date.parse('00:00 Jan 1 1970 UTC')", "Date.parse('Jan 1 1970 00:00:00 +0000')", "Date.parse('1.1.1970 UTC')",
];
for (const c of dateCases) add(ev(c).replace(/R = String\(/, "R = String(").replace("(0, eval)", "(0, eval)"));
// Fuso do processo é TZ=UTC no gerador e no teste: fixa via variável de ambiente (ver execução).

// ---- RegExp legado
const reLegacy = [
  "/(a)(b)(c)(d)(e)(f)(g)(h)(i)/.exec('abcdefghi'); [RegExp.$1, RegExp.$2, RegExp.$3, RegExp.$4, RegExp.$5, RegExp.$6, RegExp.$7, RegExp.$8, RegExp.$9].join()",
  "/(a)(b)/.exec('xaby'); RegExp.$1 + RegExp.$2 + '|' + RegExp.$3",
  "/(a)/.exec('a'); /b/.exec('b'); RegExp.$1 === ''",
  "/(a)/.exec('a'); /b/.exec('c'); RegExp.$1",
  "/(a)|(b)/.exec('b'); '[' + RegExp.$1 + '][' + RegExp.$2 + ']'",
  "/(?:a)(b)/.exec('ab'); RegExp.$1",
  "/(?<n>a)/.exec('a'); RegExp.$1",
  "/a/.exec('xay'); RegExp.lastMatch + RegExp['$&']",
  "/a/.exec('xay'); RegExp.leftContext + '|' + RegExp.rightContext + '|' + RegExp['$`'] + '|' + RegExp[\"$'\"]",
  "/(a)(b)/.exec('xaby'); RegExp.lastParen + RegExp['$+']",
  "/a/.exec('xay'); RegExp.input + RegExp.$_",
  "/a/.exec('xay'); RegExp.lastParen === ''",
  "RegExp.input = 'set'; RegExp.input + RegExp.$_",
  "RegExp.$_ = 'set2'; RegExp.input",
  "RegExp.input = 5; typeof RegExp.input",
  "RegExp.input = { toString() { return 'obj' } }; RegExp.input",
  "RegExp.multiline = true; RegExp.multiline + '' + RegExp['$*']",
  "RegExp.multiline = false; RegExp.multiline",
  "RegExp.multiline = 1; typeof RegExp.multiline",
  "RegExp['$*'] = 1; RegExp.multiline",
  "RegExp.multiline = true; /^b/.test('a\\nb')",
  "RegExp.multiline = false; /^b/.test('a\\nb')",
  "RegExp.multiline = true; RegExp.multiline = false; /^b/.test('a\\nb')",
  "'aXbXc'.replace(/X/, '-'); RegExp.leftContext + '|' + RegExp.rightContext",
  "'abc'.match(/b/); RegExp.lastMatch",
  "'abc'.match(/(b)/g); RegExp.lastMatch + RegExp.$1",
  "'abcabc'.match(/(b)/g); RegExp.leftContext",
  "/x/.test('abc'); RegExp.lastMatch",
  "/b/.test('abc'); RegExp.lastMatch + RegExp.leftContext",
  "'abc'.search(/c/); RegExp.lastMatch",
  "'abc'.split(/b/); RegExp.lastMatch",
  "'abc'.replace(/(b)/, '[$1]'); RegExp.$1",
  "'abc'.replaceAll(/(b)/g, '[$1]'); RegExp.$1",
  "[...'abab'.matchAll(/(a)/g)]; RegExp.$1",
  "var re = /(a)/; re.test('a'); re.test('b'); RegExp.$1",
  "var re = /(a)/y; re.lastIndex = 1; re.test('ba'); RegExp.$1",
  "class R2 extends RegExp {}; new R2('(a)').exec('a'); RegExp.$1",
  "class R2 extends RegExp {}; new R2('(a)').exec('a'); R2.$1",
  "class R2 extends RegExp {}; R2.$1 = 'x'",
  "RegExp.$1 = 'x'; RegExp.$1",
  "'use strict'; RegExp.$1 = 'x'",
  "'use strict'; RegExp.lastMatch = 'x'",
  "'use strict'; delete RegExp.$1",
  "delete RegExp.$1",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').enumerable",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').configurable",
  "typeof Object.getOwnPropertyDescriptor(RegExp, '$1').get + typeof Object.getOwnPropertyDescriptor(RegExp, '$1').set",
  "Object.getOwnPropertyDescriptor(RegExp, 'input').enumerable",
  "typeof Object.getOwnPropertyDescriptor(RegExp, 'input').set",
  "typeof Object.getOwnPropertyDescriptor(RegExp, 'lastMatch').set",
  "typeof Object.getOwnPropertyDescriptor(RegExp, '$&').set",
  "typeof Object.getOwnPropertyDescriptor(RegExp, 'multiline').set",
  "Object.getOwnPropertyDescriptor(RegExp, 'input').get.name",
  "Object.getOwnPropertyDescriptor(RegExp, 'lastMatch').get.name",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').get.name",
  "Object.getOwnPropertyDescriptor(RegExp, '$_').get.name",
  "Object.getOwnPropertyDescriptor(RegExp, 'rightContext').get.length",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').get.call({})",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').get.call(null)",
  "Object.getOwnPropertyDescriptor(RegExp, '$1').get.call(RegExp)",
  "class R2 extends RegExp {}; Object.getOwnPropertyDescriptor(RegExp, '$1').get.call(R2)",
  "Object.getOwnPropertyNames(RegExp).filter(n => /^\\$|input|last|Context|multiline/.test(n)).sort().join()",
  "Object.keys(RegExp).length",
  "var re = /a/; re.compile('b'); re.source",
  "var re = /a/g; re.compile('b'); re.global",
  "var re = /a/g; re.compile('b', 'i'); re.flags",
  "var re = /a/g; re.compile(); re.source + re.flags",
  "var re = /a/g; re.compile(undefined, undefined); re.source + re.flags",
  "var re = /a/g; re.lastIndex = 3; re.compile('b'); re.lastIndex",
  "var re = /a/g; re.compile(/x/i); re.source + re.flags",
  "var re = /a/g; re.compile(/x/i, 'g')",
  "var re = /a/g; re.compile(/x/i, undefined); re.flags",
  "var re = /a/; re.compile('(')",
  "var re = /a/; re.compile('a', 'zz')",
  "var re = /a/; re.compile('a', 'gg')",
  "var re = /a/; re === re.compile('b')",
  "var re = /a/; re.compile('\\\\')",
  "RegExp.prototype.compile.call({}, 'a')",
  "RegExp.prototype.compile.call(null, 'a')",
  "RegExp.prototype.compile.length + ' ' + RegExp.prototype.compile.name",
  "class R2 extends RegExp {}; new R2('a').compile('b')",
  "class R2 extends RegExp {}; new R2('a').compile('b').source",
  "var re = /a/; Object.defineProperty(re, 'lastIndex', { writable: false }); re.compile('b')",
  "var re = /a/; Object.freeze(re); re.compile('b')",
  "var re = /a/; re.compile('b'); re.exec('b') !== null",
  "var re = /a/y; re.compile('b'); re.sticky",
  "var re = /(?<n>a)/; re.compile('b'); re.exec('b').groups",
  "var re = /a/; re.compile('a', 'u'); re.unicode",
  "var re = /a/; re.compile('\\\\u{61}', 'u'); re.test('a')",
  "var re = /a/; re.compile('\\\\u{61}'); re.test('a')",
  "var re = /a/; re.compile('a', 'v'); re.unicodeSets",
  "var re = /a/; re.compile('a', 'd'); re.hasIndices",
  "var re = /a/; re.compile('a', 's'); re.dotAll",
  "RegExp.prototype.hasOwnProperty('compile')",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'compile').enumerable",
  "RegExp.prototype.compile.call(RegExp.prototype, 'a')",
];
for (const c of reLegacy) add(ev(c));

// RegExp: sintaxe do Annex B (sem u) e com u
const reSyntax = [
  "/\\8/.test('8')", "/\\9/.test('9')", "/\\8\\9/.exec('89')[0]", "/[\\8]/.test('8')", "/[\\9]/.test('9')", "/\\1/.test('\\u0001')", "/\\1/.test('1')", "/(a)\\1/.test('aa')", "/\\2(a)/.test('a')", "/\\2(a)(b)/.test('b')",
  "/\\0/.test('\\0')", "/\\00/.test('\\0')", "/\\01/.test('\\u0001')", "/\\07/.test('\\u0007')", "/\\08/.test('\\u00008')", "/\\377/.test('\\u00ff')", "/\\400/.test('\\u00200')", "/\\777/.test('\\u003f7')", "/[\\1]/.test('\\u0001')", "/[\\12]/.test('\\n')", "/[\\0]/.test('\\0')",
  "/\\c/.test('\\\\c')", "/\\c/.exec('\\\\c')[0]", "/\\cA/.test('\\u0001')", "/\\ca/.test('\\u0001')", "/\\c1/.test('\\\\c1')", "/\\c_/.test('\\\\c_')", "/[\\c]/.test('\\\\')", "/[\\c]/.test('c')", "/[\\c1]/.test('\\u0011')", "/[\\c_]/.test('\\u001f')", "/[\\c*]/.test('*')", "/[\\c*]/.test('\\\\')", "/\\c*/.test('\\\\c*')", "/\\c(/.test('\\\\c(')",
  "/{/.test('{')", "/}/.test('}')", "/a{/.test('a{')", "/a{1/.test('a{1')", "/a{1,/.test('a{1,')", "/a{,2}/.test('a{,2}')", "/a{1,2/.test('a{1,2')", "/x{1}{/.test('x{')", "/{1}/.test('{1}')", "/{/u", "/a{/u", "/}/u", "/]/u", "/]/.test(']')", "/a]/.test('a]')", "/[a]]/.test('a]')", "/[]]/.test('x')", "/[]/.test('a')", "/[^]/.test('a')", "/[^]]/.test('ab')",
  "/\\a/.test('a')", "/\\_/.test('_')", "/\\-/.test('-')", "/\\-/u", "/\\a/u", "/[\\-]/u.test('-')", "/[\\d-x]/.test('-')", "/[\\d-x]/u", "/[a-\\d]/.test('-')", "/[a-\\d]/u", "/[\\s-\\d]/.test('-')", "/[--a]/.test('.')",
  "/\\p{L}/.test('p{L}')", "/\\p{L}/.test('a')", "/\\p/.test('p')", "/\\p/u", "/\\k/.test('k')", "/\\k<a>/.test('k<a>')", "/(?<a>x)\\k<a>/.test('xx')", "/(?<a>x)\\k/.test('xk')", "/\\k<a>(?<a>x)/.test('x')", "/\\k<b>(?<a>x)/",
  "/(?=a)*/.test('')", "/(?=a)+/.test('b')", "/(?=a)?/.test('b')", "/(?=a){2}/.test('a')", "/(?!a)*/.test('a')", "/(?!a){1,2}/.test('b')", "/(?=a)*/u", "/(?!a)+/u", "/(?<=a)*/", "/(?<!a)+/", "/(?<=a)?/.test('')", "/(?<=a){2}/.test('')",
  "/^*/", "/$*/.test('')", "/^+/", "/\\b*/", "/\\B+/", "/a**/", "/a+*/", "/a{1}{2}/", "/*/", "/+/", "/?/", "/a|*/", "/(*)/", "/(?:*)/", "/a(?:)*/.test('a')",
  "/a{2,1}/", "/a{1,2}?/.test('a')", "/a{99999999999}/.test('a')", "/a{0}/.test('')", "/(?:a{0})*/.test('')",
  "/\\u0041/.test('A')", "/\\u41/.test('u41')", "/\\u{41}/.test('u'.repeat(41))", "/\\u{41}/u.test('A')", "/\\u{110000}/u", "/\\x4/.test('x4')", "/\\x41/.test('A')", "/\\x4g/.test('x4g')", "/\\xg/.test('xg')", "/\\x/.test('x')", "/\\x/u",
  "/[\\b]/.test('\\b')", "/\\b/u.test('')", "/[\\B]/.test('B')", "/[\\B]/u", "/[\\B]/.test('b')", "/\\-/.test('-')", "/[\\u{61}]/.test('u')", "/[\\u{61}]/u.test('a')",
  "/(?<a>x)(?<a>y)/", "/(?<a>x)|(?<a>y)/.test('y')", "/(?<a>x)(?<b>y)/.exec('xy').groups.b", "/(?<$a>x)/.test('x')", "/(?<_>x)/.test('x')", "/(?<1>x)/", "/(?<a/", "/(?<>x)/", "/(?<\\u0061>x)/.exec('x').groups.a", "/(?<a>x)\\k<a>\\k<a>/.test('xxx')",
  "/(?i)a/", "/(?-i:a)/.test('A')", "/(?i:a)/.test('A')", "/(?:a)/.test('a')", "/(?a)/", "/(?/", "/(?<=a)b/.test('ab')", "/(?<!a)b/.test('cb')",
  "/\\1(a)/.exec('a')[0]", "/(a)\\2/.test('a\\u0002')", "/(a)\\10/.test('a\\b')", "/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10/.test('abcdefghijj')", "/\\10/.test('\\b')", "/\\18/.test('\\u00018')", "/\\1a/.test('\\u0001a')",
  "/a\\/b/.test('a/b')", "/[/]/.test('/')", "/\\//.source", "new RegExp('/').source", "new RegExp('\\\\/').source", "new RegExp('\\n').source", "new RegExp('[/]').source", "new RegExp('').source", "RegExp.prototype.source", "RegExp.prototype.toString()", "new RegExp('\\u2028').source",
  "/./.test('\\u2028')", "/./s.test('\\n')", "/\\s/.test('\\ufeff')", "/\\S/.test('\\u180e')", "/\\w/iu.test('\\u017f')", "/\\w/i.test('\\u017f')", "/[\\w-a]/.test('-')", "/[\\w-a]/u",
  "/\\u{1F600}/.test('\\u{1F600}')", "/^.$/.test('\\u{1F600}')", "/^.$/u.test('\\u{1F600}')", "/\\ud83d/.test('\\u{1F600}')", "/\\ud83d/u.test('\\u{1F600}')",
  "/[\\u{1F600}]/.test('\\ud83d')", "/[\\u{1F600}]/u.test('\\ud83d')",
];
for (const c of reSyntax) add(ev(c));
for (const [src, flags] of [["a", "gg"], ["a", "x"], ["a", "uv"], ["a", "gimsuyd"], ["a", "gimsvyd"], ["a", ""], ["{", ""], ["{", "u"], ["}", ""], ["]", ""], ["]", "u"], ["\\8", ""], ["\\8", "u"], ["\\c", ""], ["\\c", "u"], ["a{1", ""], ["a{1", "u"], ["(?=a)*", ""], ["(?=a)*", "u"], ["(?<=a)*", ""], ["\\1", ""], ["\\1", "u"], ["\\k<a>", ""], ["\\k<a>", "u"], ["(?<a>.)\\k<b>", ""], ["[b-a]", ""], ["[\\d-a]", ""], ["[\\d-a]", "u"], ["\\-", "u"], ["\\p{L}", ""], ["\\p{L}", "u"], ["\\p{Nope}", "u"], ["\\p{L}", "v"], ["[\\p{L}--a]", "v"], ["[a&&b]", "v"], ["[a&&b]", ""], ["(", ""], [")", ""], ["[", ""], ["\\", ""], ["a**", ""], ["", "gg"]]) {
  add(ev(`new RegExp(${q(src)}, ${q(flags)}).source + '/' + new RegExp(${q(src)}, ${q(flags)}).flags`));
}

// ---- HTML-like comments
const htmlComments = [
  "<!-- comentário\nR = 'a'", "R = 'a' <!-- resto\n", "R = 1\n--> comentário\n", "R = 1\n  --> comentário\n", "R = 1\n/* x */ --> comentário\n", "R = 1 --> 0", "var x = 2, y = 1; R = x --> y", "var x = 1; R = x-->0",
  "R = 'a'\n/*\n*/ --> comentário", "R = 'a'\n/* */ /* */ --> c", "R = 'a' /* */ --> c", "<!--\nR = 5", "<!-- a <!-- b\nR = 6", "R = 7; <!-- tail", "R = 8;\n-->\nR = 9",
  "R = typeof <!--x\n1", "R = 1 <!--2\n", "var a = 1; R = a<!--1\n", "R = '<!--'", "R = '-->'", "R = /<!--/.source", "R = `<!--${1}-->`", "R = [1, 2] // <!--\n", "R = 1\n/**/-->x", "R = 1\n/*\n*/-->x", "R = 1\n--> x\nR = 2\n--> y",
  "-->\nR = 1", "  -->\nR = 2", "/* */ --> c\nR = 3", "/*\n*/ --> c\nR = 4", "// x\n--> c\nR = 5", "R = 1; --> c", "R = 1; /* */ --> c", "R = 1\n\t--> c", "R = 1\n--> c\n--> d", "R = 1\n-- > c",
  "R = 1\n--\n> c", "var a = 5; R = a\n-->\n3", "var a = 5; R = a\n--> 3", "R = 0\n--x\n", "var x = 1; R = x\n--x\nR2 = x", "R = 1 /*\n*/ --> c", "R = 2 /* c */ <!-- d", "R = 'x' + <!-- y\n'z'",
  "<!-- x\n--> y\nR = 'ok'", "R = eval('1 <!-- c')", "R = eval('1\\n--> c')", "R = new Function('return 1 <!-- c')()", "R = new Function('--> c\\nreturn 2')()", "R = new Function('<!-- c\\nreturn 3')()", "R = (0, eval)('--> c\\n4')", "R = (0, eval)('<!-- c\\n5')",
];
for (const c of htmlComments) add(c.includes("R = ") ? `try { ${""}} catch (e) {}\n` + c : c);
// modo strict e módulo-ish: comentário HTML continua valendo em script strict
for (const c of ["'use strict'; R = 1 <!-- c\n", "'use strict'; R = 1\n--> c\n", "'use strict';\n<!-- c\nR = 3", "'use strict';\n--> c\nR = 4"]) add(c);
// em módulo: SyntaxError via import dinâmico não disponível sem arquivo; usa o texto do erro por Function
for (const c of ["a <!-- b", "a\n--> b", "<!-- b", "--> b"]) add(`try { new Function(${q(c)}); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }`);

// ---- Literais octais
const octalCases = [
  "010", "08", "09", "07", "00", "01", "0777", "0778", "019", "089", "0.5", "00.5", "08.5", "010.5", "07.5", "09.5", "010e1", "08e1", "010n", "08n", "0o10", "0O17", "0o8", "0o", "0o7_7", "0b11", "0B1", "0b2", "0x1f", "0X1F", "0xg",
  "010 + 1", "-010", "+010", "010.toString()", "010..toString()", "010 .toString()", "08 .toString()", "0_1", "01_0", "0_8", "08_1", "0.0_1", "1_0", "1__0", "1_", "0x_1", "0o_1", "0b_1", "00_1",
  "typeof 010", "010 === 8", "08 === 8", "09.9", "'use strict'; 010", "'use strict'; 08", "'use strict'; 09", "'use strict'; 0o10", "'use strict'; 00", "'use strict'; 0", "'use strict'; 0.5", "'use strict'; 0e1", "'use strict'; 00.5",
  "function f() { 'use strict'; return 010 }", "function f() { 'use strict'; return 08 }", "function f() { return 010; 'use strict' }", "function f(a = 010) { 'use strict' }", "function f() { 010; 'use strict'; }", "(function () { 'use strict'; return 0 })()", "class A { m() { return 010 } }", "class A { m() { return 08 } }", "class A { x = 010 }", "var o = { 010: 1 }; Object.keys(o).join()", "var o = { 08: 1 }; Object.keys(o).join()", "var o = { 010() {} }; Object.keys(o).join()", "({ 010: 1 })[8]", "({ 0o10: 1 })[8]", "({ 08.5: 1 })['8.5']", "({ 1.50: 1 })['1.5']",
  "'use strict'; ({ 010: 1 })", "'use strict'; ({ 08: 1 })", "`${010}`", "'use strict'; `${010}`", "010 .valueOf()", "Number('010')", "Number('08')", "Number('0o10')", "parseInt('010')", "parseInt('08')", "parseInt('0x10')", "parseInt('010', 10)", "+'0b11'", "+'0o17'", "+'0x1F'", "+'-0x1F'", "+'010'", "Number('0b2')", "Number('0o8')", "Number('1_0')", "Number('08.5')",
  "new Function('return 010')()", "new Function('\"use strict\"; return 010')()", "new Function('a = 010', '\"use strict\"')", "new Function('a', '\"use strict\"; return 08')", "eval('010')", "eval('\"use strict\"; 010')", "eval('08')", "(0, eval)('\"use strict\"; 08')",
  "012 + 019", "0.1 + 0.2 === 0.3", "00000000000000000000000000000000000001", "0777777777777777777777", "0888888888888888888888", "0.", "0.e1", "08.", "08.e1", "010.e1",
  "var 010 = 1", "1.toString()", "1 .toString()", "1..toString()", "1.e1", "0x1.toString()", "0b1.toString()", "01.toString()", "08.toString()", "08..toString()",
  "0 .toString()", "0..toString()", "0.0.toString()", "5e-324", "0e0", "0E5", "0.0e5", "00e5", "08e5", "07e5", "010e-1",
];
for (const c of octalCases) add(ev(c));
// Escapes octais em strings
const octalStr = [
  "'\\07'", "'\\0'", "'\\00'", "'\\01'", "'\\1'", "'\\7'", "'\\8'", "'\\9'", "'\\08'", "'\\09'", "'\\18'", "'\\377'", "'\\400'", "'\\777'", "'\\0000'", "'\\1234'", "'\\3777'", "'\\x41\\101'", "'\\a'", "'\\c'", "'\\z'", "'\\u'", "'\\x'", "'\\x1'", "'\\u00'",
  "'\\07'.length", "'\\07'.charCodeAt(0)", "'\\08'.length", "'\\08'.charCodeAt(1)", "'\\8'.charCodeAt(0)", "'\\9'.charCodeAt(0)", "'\\377'.charCodeAt(0)", "'\\400'.length", "'\\400'.charCodeAt(1)", "'\\1'.charCodeAt(0)", "'\\00'.charCodeAt(0)", "'\\0'.charCodeAt(0)",
  "\"\\07\"", "'\\\n'", "'\\\r\n'", "'\\\u2028'", "'a\\\nb'", "'\\u{41}'", "'\\u{0}'", "'\\u{110000}'", "'\\u{}'", "'\\u{zz}'",
  "'use strict'; '\\07'", "'use strict'; '\\0'", "'use strict'; '\\00'", "'use strict'; '\\8'", "'use strict'; '\\9'", "'use strict'; '\\08'", "'use strict'; '\\1'", "'use strict'; '\\01'", "'use strict'; '\\x41'",
  "function f() { 'use strict'; return '\\07' }", "function f() { '\\07'; 'use strict' }", "function f() { '\\07'; 'use strict'; return 1 }", "function f() { 'use strict'; '\\07' }", "function f(a = '\\07') { 'use strict' }", "'\\07'; 'use strict'; 1", "'\\8'; 'use strict'; 1", "'\\00'; 'use strict'; 1", "'\\0'; 'use strict'; 1",
  "`\\07`", "`\\0`", "`\\00`", "`\\8`", "`\\1`", "(x => x.raw[0])`\\07`", "(x => x[0])`\\07`", "(x => x[0] === undefined)`\\07`", "(x => x[0] === undefined)`\\8`", "(x => x.raw[0])`\\8`", "(x => x[0])`\\0`", "(x => x[0] === undefined)`\\00`", "(x => x[0] === undefined)`\\01`", "(x => x[0] === undefined)`\\u`", "(x => x.raw[0])`\\u`", "(x => x[0] === undefined)`\\xz`", "(x => x[0] === undefined)`\\u{110000}`",
  "'\\u{1F600}'.length", "'\\ud83d\\ude00'.length", "'\\v'.charCodeAt(0)", "'\\b'.charCodeAt(0)", "'\\f'.charCodeAt(0)", "'\\'.length", "'\\",
];
for (const c of octalStr) add(ev(c.includes("(x") || c.includes("`") ? c : (c.startsWith("'use strict'") || c.startsWith("function") || c.startsWith("'\\07'; ") || c.startsWith("'\\8'; ") || c.startsWith("'\\00'; ") || c.startsWith("'\\0'; ") ? c : "(" + c + ")")));

// ---- Funções em blocos (Annex B.3.3), labels, if (x) function
const blockFn = [
  "{ function f() { return 1 } } typeof f", "typeof f; { function f() {} } typeof f", "{ function f() { return 1 } } f()", "typeof f; { function f() {} }",
  "var r = []; r.push(typeof f); { r.push(typeof f); function f() {} } r.push(typeof f); r.join()",
  "{ function f() { return 1 } function f() { return 2 } } f()", "{ function f() { return 1 } var f; } ", "{ let f; function f() {} }", "{ function f() {} let f; }", "let f; { function f() {} } typeof f", "let f = 1; { function f() {} } f",
  "{ { function f() { return 3 } } } f()", "if (true) { function f() { return 4 } } f()", "if (false) { function f() { return 4 } } typeof f", "if (true) function f() { return 5 } f()", "if (false) function f() { return 5 } typeof f", "if (true) function f() {} else function g() {} typeof f + typeof g",
  "if (false) function f() {} else function g() {} typeof f + typeof g", "if (true) function f() {} else ; typeof f", "if (1) function* g() {}", "if (1) async function g() {}", "if (1) class C {}", "if (1) let x", "if (1) let\nx", "if (1) const x = 1", "if (true) function f() {} \n else function g() {}",
  "while (0) function f() {}", "for (;;) function f() {}", "do function f() {} while (0)", "with ({}) function f() {}", "label: function f() {} typeof f", "a: b: function f() {} typeof f", "label: function* g() {}", "label: async function g() {}", "label: class C {}", "label: let x", "label: { function f() {} } typeof f", "if (1) label: function f() {}",
  "while (0) label: function f() {}", "for (;;) label: function f() {}", "label: if (0) function f() {} typeof f", "'use strict'; label: function f() {}", "'use strict'; if (1) function f() {}", "'use strict'; if (1) { function f() {} } typeof f", "'use strict'; { function f() { return 1 } } typeof f", "'use strict'; { function f() {} f() } typeof f",
  "'use strict'; { function f() {} function f() {} }", "'use strict'; { function f() {} var f }", "'use strict'; function g() { { function f() {} } return typeof f } g()", "function g() { { function f() {} } return typeof f } g()", "function g() { var r = typeof f; { function f() {} } return r + typeof f } g()", "function g() { { function f() { return 1 } } return f() } g()",
  "function g(f) { { function f() {} } return typeof f } g(1)", "function g(f) { { function f() {} } return f } g(1)", "function g() { let f = 1; { function f() {} } return f } g()", "function g() { { function f() {} } let f; } g()", "function g() { var f = 1; { function f() {} } return typeof f } g()", "function g() { { function f() { return 1 } f = 2 } return typeof f } g()", "function g() { { function f() { return 1 } f = 2; return typeof f } } g()", "function g() { { f = 2; function f() { return 1 } } return typeof f } g()",
  "function g() { { function f() { return 1 } } { function f() { return 2 } } return f() } g()", "function g() { { function f() { return 1 } } { let f = 5 } return f() } g()", "function g() { { let f = 5; { function f() {} } } return typeof f } g()", "function g() { { let f = 5; { function f() {} } } } g()", "function g() { try { } catch (f) { { function f() {} } } return typeof f } g()", "function g() { try { throw 1 } catch (f) { { function f() {} } } return typeof f } g()",
  "function g() { try { throw 1 } catch (f) { function f() {} } } g()", "function g() { try { throw 1 } catch ({ f }) { { function f() {} } } } g()", "function g() { for (let f of [1]) { { function f() {} } } return typeof f } g()", "function g() { for (let f;;) { { function f() {} } break } } g()", "function g() { for (var f of [1]) { { function f() {} } } return typeof f } g()",
  "function g() { switch (1) { case 1: function f() { return 6 } } return f() } g()", "function g() { switch (1) { case 0: function f() {} } return typeof f } g()", "function g() { switch (1) { case 1: function f() {} case 2: function f() {} } return typeof f } g()", "function g() { switch (1) { case 1: function f() {} case 2: let f } } g()", "function g() { switch (1) { case 1: function f() {} case 2: var f } } g()",
  "function g() { { function arguments() {} } return typeof arguments } g()", "function g() { { function g() {} } return typeof g } g()", "function g() { { function eval() {} } } g()", "function g() { 'use strict'; { function eval() {} } } g()", "function g() { { function let() {} } return typeof let } g()", "function g() { { function yield() {} } return typeof yield } g()", "function* g() { { function yield() {} } } g().next()", "async function g() { { function await() {} } return typeof await } g().then(v => { R = v })",
  "(function () { { function f() {} } return typeof f })()", "(function f2() { { function f2() {} } return typeof f2 })()", "(function f2() { return typeof f2 })()", "(() => { { function f() {} } return typeof f })()", "var o = { m() { { function f() {} } return typeof f } }; o.m()", "class A { m() { { function f() {} } return typeof f } }; new A().m()", "class A { static { { function f() {} } } }; typeof f", "class A { static { { function f() {} } return 1 } }",
  "eval('{ function f() {} } typeof f')", "(0, eval)('{ function f() {} } typeof f')", "(0, eval)('{ function f1() {} } typeof f1') + typeof f1", "(function () { eval('{ function f() {} }'); return typeof f })()", "(function () { 'use strict'; eval('{ function f() {} }'); return typeof f })()", "(function () { eval('var x = 1'); return typeof x })()", "(function () { 'use strict'; eval('var x = 1'); return typeof x })()", "(function () { eval('function f() {}'); return typeof f })()",
  "(function () { let f; eval('{ function f() {} }'); return typeof f })()", "(function () { let f = 1; eval('{ function f() {} }'); return typeof f })()", "(function () { { let f; eval('{ function f() {} }'); } return typeof f })()", "(function () { eval('var f; { function f() {} }') ; return typeof f })()", "(function (f) { eval('{ function f() {} }'); return typeof f })(1)",
  "{ function f() {} } Object.getOwnPropertyDescriptor(globalThis, 'f').configurable", "{ function f() {} } Object.getOwnPropertyDescriptor(globalThis, 'f').enumerable", "{ function f() {} } Object.getOwnPropertyDescriptor(globalThis, 'f').writable", "{ function f() {} } delete globalThis.f", "typeof globalThis.f; { function f() {} } globalThis.f === f", "Object.prototype.hasOwnProperty.call(globalThis, 'f'); { function f() {} }",
  "{ function f() { return 1 } f = 2; } typeof f", "{ function f() { return 1 } f = 2; } f()", "{ f = 2; function f() { return 1 } } typeof f", "{ function f() { return 1 } { f = 3 } } typeof f", "{ function f() {} f.x = 1 } f.x", "{ function f() {} } f.name", "{ function f() {} } f.length", "{ function f(a, b) {} } f.length", "{ let g = function f() {}; } typeof f",
  "var f = 1; { function f() {} } typeof f", "var f = 1; { function f() {} f = 2 } f", "var f = 1; { function f() { return 1 } } f()", "{ function f() {} } { function f() { return 7 } } f()", "{ function f() {} var g } typeof g", "function f() { return 0 } { function f() { return 1 } } f()", "function f() { return 0 } { function f() { return 1 } f = 5 } f()",
  "function f() { return 0 } { function f() { return 1 } f = 5; } typeof f", "function f() { return 0 } { f = 5; function f() { return 1 } } typeof f", "{ function f() {} } function f() { return 8 } typeof f", "typeof f; function f() {} { function f() { return 9 } } f()", "var r = typeof f; { function f() {} } r", "let r = typeof f; { function f() {} } r",
  "{ async function f() {} } typeof f", "{ function* f() {} } typeof f", "{ async function* f() {} } typeof f", "{ class f {} } typeof f", "{ async function f() {} async function f() {} }", "{ function* f() {} function* f() {} }", "{ function f() {} function* f() {} }", "{ function f() {} async function f() {} }", "{ async function f() {} function f() {} }", "{ function f() {} class f {} }",
  "switch (1) { case 1: function f() {} } typeof f", "switch (0) { case 1: function f() {} } typeof f", "switch (1) { case 1: function f() {} default: function f() {} } typeof f", "switch (1) { case 1: function f() {} default: function* f() {} }", "switch (1) { case 1: let f; default: function f() {} }",
  "try { function f() {} } catch (e) {} typeof f", "try { throw 1 } catch (e) { function f() {} } typeof f", "try { throw 1 } catch (e) { function e() {} }", "try { throw 1 } catch (e) { { function e() {} } } typeof e", "try { throw 1 } catch (e) { var e = 2 } typeof e", "try { throw 1 } catch ([e]) { var e }", "try { throw 1 } catch (e) { for (var e of []); }", "try { throw 1 } catch (e) { for (var e in {}); }", "try { throw 1 } catch (e) { for (var e;;) break }", "try { throw 1 } catch (e) { let e }", "try { } finally { function f() {} } typeof f",
  "with ({}) { function f() {} } typeof f", "with ({ f: 1 }) { function f() {} } typeof f", "with ({ f: 1 }) { function f() {} f = 3 } typeof f", "var o = { f: 1 }; with (o) { function f() {} } o.f", "var o = { f: 1 }; with (o) { function f() {} } typeof f", "var o = { f: 1 }; with (o) { var f = 5 } o.f",
];
for (const c of blockFn) add(ev(c));

// ---- for (var i = 0 in {}), with, arguments.callee, caller
const misc1 = [
  "for (var i = 0 in {}) ; i", "for (var i = 0 in { a: 1 }) ; i", "for (var i = 5 in {}) ; i", "for (var i = (1, 2) in {}) ; i", "for (var i = 0 in { a: 1, b: 2 }) ; i", "var r = []; for (var i = (r.push('init'), 0) in { a: 1 }) r.push(i); r.join()", "var r = []; for (var i = (r.push('init'), 0) in {}) r.push(i); r.join()",
  "'use strict'; for (var i = 0 in {}) ;", "for (let i = 0 in {}) ;", "for (const i = 0 in {}) ;", "for (var [i] = 0 in {}) ;", "for (var {i} = 0 in {}) ;", "for (var i = 0 of []) ;", "for (var i = 0 of [1]) ;", "for (i = 0 in {}) ;", "for (var i, j in {}) ;", "for (var i = 0, j = 1 in {}) ;", "for (var i = 0 in {}, {}) ;",
  "async function f() { for await (var i = 0 of []) ; } ", "for (var i = 0 in {}) function f() {}", "for (var i = 0 in {}) let\nx", "for (var i = 0 in { a: 1 }) ; typeof i", "for (var i = 'x' in { a: 1 }) ; i", "for (var i = 1 in 2) ; i", "for (var i = 1 in null) ; i", "for (var i = 1 in undefined) ; i", "for (var i = 1 in 'ab') ; i", "for (var i = 1 in [5, 6]) ; i", "(function () { for (var i = 3 in {}); return i })()", "(function () { 'use strict'; for (var i = 3 in {}); return i })()",
  "var o = { a: 1 }; with (o) { a }", "var o = { a: 1 }; with (o) { a = 2 } o.a", "var o = { a: 1 }; with (o) { var a = 3 } o.a", "var o = { a: 1 }; with (o) { var b = 3 } o.b + typeof b", "with ({}) { var x = 1 } x", "with (null) {}", "with (undefined) {}", "with (1) {}", "with ('s') { length }", "with (Symbol()) { description }", "with ([1, 2]) { length }",
  "'use strict'; with ({}) {}", "function f() { 'use strict'; with ({}) {} }", "with ({}) function f() {}", "with ({}) label: function f() {}", "with ({}) class C {}", "with ({}) let\nx", "with ({ a: 1 }) { (function () { return a })() }", "with ({ a: 1 }) { eval('a') }", "with ({ a: 1 }) { eval('var a = 2') } a", "var o = { a: 1 }; with (o) { eval('var a = 2') } o.a",
  "var o = { x: 1, [Symbol.unscopables]: { x: true } }; var x = 'outer'; with (o) { x }", "var o = { x: 1, [Symbol.unscopables]: { x: false } }; with (o) { x }", "var o = { x: 1, [Symbol.unscopables]: 1 }; with (o) { x }", "var o = { x: 1, [Symbol.unscopables]: null }; with (o) { x }", "var o = { x: 1, get [Symbol.unscopables]() { throw new Error('u') } }; with (o) { x }", "with ([]) { typeof values + typeof keys + typeof entries + typeof flat + typeof at + typeof includes }",
  "with ([]) { typeof fill }", "with ([]) { typeof find }", "with ([]) { typeof copyWithin }", "with ([]) { typeof flatMap }", "with ([]) { typeof findLast }", "with ([]) { typeof toSorted }", "with ([]) { typeof push }", "Object.keys(Array.prototype[Symbol.unscopables]).sort().join()", "Object.getPrototypeOf(Array.prototype[Symbol.unscopables])", "Array.prototype[Symbol.unscopables].toString",
  "var x = 'o'; with ({ x: 'w' }) { var f = function () { return x } } f()", "var x = 'o'; with ({ x: 'w' }) { var g = () => x } g()", "var o = { x: 1 }; with (o) { delete x } 'x' in o", "var o = { x: 1 }; with (o) { typeof x }", "var o = { x: 1 }; with (o) { x++ } o.x", "var o = { f() { return this } }; with (o) { f() === o }", "var o = { f() { return this === undefined } }; with (o) { (0, f)() }", "with ({ a: 1 }) { var a } a", "with ({}) { y = 5 } y",
  "(function () { return arguments.callee })() === undefined", "(function f() { return arguments.callee === f })()", "(function () { return typeof arguments.callee })()", "(function () { 'use strict'; return arguments.callee })()", "(function () { 'use strict'; arguments.callee = 1 })()", "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').enumerable })()", "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').configurable })()", "(function () { 'use strict'; return typeof Object.getOwnPropertyDescriptor(arguments, 'callee').get })()", "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get.name })()", "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get.length })()",
  "(function () { return Object.getOwnPropertyDescriptor(arguments, 'callee').enumerable })()", "(function () { return Object.getOwnPropertyDescriptor(arguments, 'callee').writable })()", "(function () { return Object.getOwnPropertyDescriptor(arguments, 'callee').configurable })()", "(function () { delete arguments.callee; return typeof arguments.callee })()", "(function () { arguments.callee = 1; return arguments.callee })()", "(function () { return Object.getOwnPropertyNames(arguments).join() })(1, 2)", "(function () { 'use strict'; return Object.getOwnPropertyNames(arguments).join() })(1, 2)",
  "(function (a) { return Object.getOwnPropertyNames(arguments).join() })(1)", "(function () { return Object.prototype.toString.call(arguments) })()", "(function () { return arguments[Symbol.iterator] === Array.prototype.values })()", "(function () { return arguments.length })(1, 2, 3)", "(() => { try { return arguments.callee } catch (e) { return e.name } })()", "(function () { return (() => arguments.callee)() })() === undefined",
  "(function f() { return (() => arguments.callee)() === f })()", "(function () { return (function () { return arguments.callee })() })().name", "(function f() { return f.caller })()", "(function f() { return f.caller })() === null", "function outer() { return inner() } function inner() { return inner.caller } outer() === outer", "function outer() { return inner() } function inner() { return inner.caller } outer.name + typeof outer()",
  "function inner() { return inner.caller } inner()", "function inner() { return inner.caller } (() => inner())() === null", "function inner() { return inner.caller } (function w() { return inner() })() .name", "function inner() { 'use strict'; return inner.caller }", "function inner() { 'use strict'; return typeof inner } inner()", "function s() { 'use strict' } s.caller", "function s() { 'use strict' } s.arguments", "function s() { 'use strict' } s.caller = 1", "function s() { 'use strict' } s.arguments = 1",
  "(function () { 'use strict'; return (function () { return arguments.callee })() })()", "(function s() { 'use strict'; return s.caller })()", "(function s() { 'use strict'; return s.arguments })()", "(function s() { 'use strict'; return Object.getOwnPropertyNames(s).join() })()", "(function s() { return Object.getOwnPropertyNames(s).join() })()", "(function () { return Object.getOwnPropertyNames(function () {}).join() })()", "Object.getOwnPropertyNames(function () { 'use strict' }).join()", "Object.getOwnPropertyNames(() => 1).join()", "Object.getOwnPropertyNames(class {}).join()", "Object.getOwnPropertyNames(async function () {}).join()", "Object.getOwnPropertyNames(function* () {}).join()", "Object.getOwnPropertyNames(({ m() {} }).m).join()", "Object.getOwnPropertyNames(Function.prototype).sort().join()",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get === Object.getOwnPropertyDescriptor(Function.prototype, 'caller').set", "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').get === Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get", "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').enumerable", "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').configurable", "typeof Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get",
  "Function.prototype.caller", "Function.prototype.arguments", "Function.prototype.caller = 1", "'use strict'; Function.prototype.caller = 1", "(() => 1).caller", "(() => 1).arguments", "(class {}).caller", "(class {}).arguments", "(async function () {}).caller", "(function* () {}).caller", "({ m() {} }).m.caller", "({ m() {} }).m.arguments", "(function () {}).bind().caller", "(function () {}).bind().arguments", "Math.max.caller", "Math.max.arguments", "parseInt.caller", "Object.getOwnPropertyNames(Math.max).join()", "(function () {}).hasOwnProperty('caller')", "(function () {}).hasOwnProperty('arguments')", "'caller' in function () {}", "'caller' in (() => 1)", "(function () {}).caller", "(function () {}).arguments", "(function (a) { return a }).arguments", "function f(a) { return f.arguments } f(1)", "function f(a) { return f.arguments.length } f(1, 2)", "function f(a) { return f.arguments[0] } f(7)", "function f(a) { a = 9; return f.arguments[0] } f(7)", "function f() { return f.arguments === f.arguments } f()", "function f() { return f.arguments === arguments } f()",
  "function f() { return Object.prototype.toString.call(f.arguments) } f()", "function f() { 'use strict'; return f.arguments } f()", "function f() { return g() } function g() { return g.caller.arguments[0] } f(11)", "function f() { return g() } function g() { return g.caller.caller } f()", "function f() { return g() } function g() { return g.caller.caller === null } f()", "Object.defineProperty(function () {}, 'caller', { value: 1 }).caller", "var f = function () {}; f.caller = 5; f.caller", "var f = function () {}; Object.defineProperty(f, 'caller', { value: 5 }); f.caller", "var f = function () {}; delete f.caller; f.caller", "var f = function () { 'use strict' }; Object.defineProperty(f, 'caller', { value: 5 }); f.caller", "Object.defineProperty(function () { 'use strict' }, 'arguments', { value: 1 }).arguments",
];
for (const c of misc1) add(ev(c));

// ---- Object.prototype.toString e builtinTag
const toStr = [
  "Object.prototype.toString.call(undefined)", "Object.prototype.toString.call(null)", "Object.prototype.toString.call(1)", "Object.prototype.toString.call('')", "Object.prototype.toString.call(true)", "Object.prototype.toString.call(Symbol())", "Object.prototype.toString.call(1n)", "Object.prototype.toString.call([])", "Object.prototype.toString.call(function () {})", "Object.prototype.toString.call(() => 1)", "Object.prototype.toString.call(async function () {})", "Object.prototype.toString.call(function* () {})", "Object.prototype.toString.call(class {})",
  "Object.prototype.toString.call(new Error())", "Object.prototype.toString.call(new TypeError())", "Object.prototype.toString.call(new AggregateError([]))", "Object.prototype.toString.call(Object.create(Error.prototype))", "Object.prototype.toString.call(new Boolean(1))", "Object.prototype.toString.call(new Number(1))", "Object.prototype.toString.call(new String(''))", "Object.prototype.toString.call(Object(Symbol()))", "Object.prototype.toString.call(Object(1n))",
  "Object.prototype.toString.call(new Date())", "Object.prototype.toString.call(Object.create(Date.prototype))", "Object.prototype.toString.call(/a/)", "Object.prototype.toString.call(Object.create(RegExp.prototype))", "Object.prototype.toString.call((function () { return arguments })())", "Object.prototype.toString.call((function () { 'use strict'; return arguments })())", "Object.prototype.toString.call(JSON)", "Object.prototype.toString.call(Math)", "Object.prototype.toString.call(Reflect)", "Object.prototype.toString.call(globalThis)", "Object.prototype.toString.call(Atomics)", "Object.prototype.toString.call(Intl)",
  "Object.prototype.toString.call(new Map())", "Object.prototype.toString.call(new Set())", "Object.prototype.toString.call(new WeakMap())", "Object.prototype.toString.call(new WeakSet())", "Object.prototype.toString.call(new WeakRef({}))", "Object.prototype.toString.call(Promise.resolve())", "Object.prototype.toString.call(new ArrayBuffer(1))", "Object.prototype.toString.call(new SharedArrayBuffer(1))", "Object.prototype.toString.call(new DataView(new ArrayBuffer(1)))", "Object.prototype.toString.call(new Uint8Array(1))", "Object.prototype.toString.call(new Proxy([], {}))", "Object.prototype.toString.call(new Proxy({}, {}))", "Object.prototype.toString.call(new Proxy(function () {}, {}))",
  "Object.prototype.toString.call([].values())", "Object.prototype.toString.call(new Map().entries())", "Object.prototype.toString.call(''[Symbol.iterator]())", "Object.prototype.toString.call((function* () {})())", "Object.prototype.toString.call((async function* () {})())", "Object.prototype.toString.call(/a/[Symbol.matchAll]('a'))", "Object.prototype.toString.call(Symbol.prototype)", "Object.prototype.toString.call(Array.prototype)", "Object.prototype.toString.call(Function.prototype)", "Object.prototype.toString.call(Error.prototype)", "Object.prototype.toString.call(Boolean.prototype)", "Object.prototype.toString.call(Number.prototype)", "Object.prototype.toString.call(String.prototype)", "Object.prototype.toString.call(Date.prototype)", "Object.prototype.toString.call(RegExp.prototype)", "Object.prototype.toString.call(Object.prototype)", "Object.prototype.toString.call(Promise.prototype)", "Object.prototype.toString.call(Map.prototype)", "Object.prototype.toString.call(BigInt.prototype)",
  "var a = []; a[Symbol.toStringTag] = 'X'; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = 'X'; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = 1; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = undefined; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = null; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = Symbol(); Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = ''; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = 'a b'; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = {}; Object.prototype.toString.call(a)", "var a = {}; a[Symbol.toStringTag] = new String('S'); Object.prototype.toString.call(a)",
  "var a = function () {}; a[Symbol.toStringTag] = 'F'; Object.prototype.toString.call(a)", "var a = new Error(); a[Symbol.toStringTag] = 'E'; Object.prototype.toString.call(a)", "var a = new Boolean(1); a[Symbol.toStringTag] = 'B'; Object.prototype.toString.call(a)", "var a = new Date(); a[Symbol.toStringTag] = 'D'; Object.prototype.toString.call(a)", "var a = /a/; a[Symbol.toStringTag] = 'RE'; Object.prototype.toString.call(a)", "(function () { arguments[Symbol.toStringTag] = 'A'; return Object.prototype.toString.call(arguments) })()", "var a = new Number(1); a[Symbol.toStringTag] = 'N'; Object.prototype.toString.call(a)", "var a = new String(''); a[Symbol.toStringTag] = 'S'; Object.prototype.toString.call(a)",
  "Object.prototype.toString.call({ get [Symbol.toStringTag]() { throw new RangeError('g') } })", "Object.prototype.toString.call({ get [Symbol.toStringTag]() { return 'G' } })", "Object.prototype.toString.call(new Proxy({}, { get(t, k) { return k === Symbol.toStringTag ? 'P' : undefined } }))", "Object.prototype.toString.call(new Proxy([], { get(t, k) { return k === Symbol.toStringTag ? 'P' : t[k] } }))", "Object.prototype.toString.call(new Proxy([], { get() { throw new Error('t') } }))", "Object.prototype.toString.call(new Proxy([], {}))", "Object.prototype.toString.call(Proxy.revocable([], {}).proxy)", "var r = Proxy.revocable([], {}); r.revoke(); Object.prototype.toString.call(r.proxy)",
  "Object.prototype.toString.call(Object.create(Array.prototype))", "Object.prototype.toString.call(Object.create(Function.prototype))", "Object.prototype.toString.call(Object.create(Boolean.prototype))", "Object.prototype.toString.call(Object.create(Number.prototype))", "Object.prototype.toString.call(Object.create(String.prototype))", "Object.prototype.toString.call(Object.setPrototypeOf(function () {}, null))", "Object.prototype.toString.call(Object.setPrototypeOf([], null))", "Object.prototype.toString.call(Object.setPrototypeOf(new Error(), null))", "Object.prototype.toString.call(Object.setPrototypeOf(new Date(), null))", "Object.prototype.toString.call(Object.setPrototypeOf(/a/, null))", "Object.prototype.toString.call(Object.setPrototypeOf(new Boolean(1), null))", "Object.prototype.toString.call(Object.setPrototypeOf(new Map(), null))",
  "String(Symbol.toStringTag)", "Symbol.prototype[Symbol.toStringTag]", "Map.prototype[Symbol.toStringTag]", "Set.prototype[Symbol.toStringTag]", "Promise.prototype[Symbol.toStringTag]", "ArrayBuffer.prototype[Symbol.toStringTag]", "DataView.prototype[Symbol.toStringTag]", "BigInt.prototype[Symbol.toStringTag]", "JSON[Symbol.toStringTag]", "Math[Symbol.toStringTag]", "Reflect[Symbol.toStringTag]", "Atomics[Symbol.toStringTag]", "Intl[Symbol.toStringTag]", "globalThis[Symbol.toStringTag]", "WeakMap.prototype[Symbol.toStringTag]", "WeakRef.prototype[Symbol.toStringTag]", "Object.getPrototypeOf(Uint8Array.prototype)[Symbol.toStringTag]", "Uint8Array.prototype[Symbol.toStringTag]", "(function* () {}).constructor.prototype[Symbol.toStringTag]", "Object.getPrototypeOf(function* () {}).prototype[Symbol.toStringTag]", "Object.getPrototypeOf([][Symbol.iterator]())[Symbol.toStringTag]", "Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag).writable", "Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag).configurable", "Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag).enumerable",
  "Object.prototype.toString.call(1) === '[object Number]'", "({}).toString()", "({}).toString.call()", "Object.prototype.toString.length + Object.prototype.toString.name", "Object.prototype.toLocaleString.call(1)", "Object.prototype.toLocaleString.call(null)", "Object.prototype.toLocaleString.call({ toString() { return 'ts' } })", "Object.prototype.toLocaleString.length", "Object.prototype.toLocaleString.call({ toString: 1 })", "Object.prototype.valueOf.call(null)", "Object.prototype.valueOf.call(1) instanceof Number", "String(Object.prototype.toString)", "String(Object.prototype.toString.call(Object.create(null)))", "Object.prototype.toString.call(new (class extends Array {})())", "Object.prototype.toString.call(new (class extends Error {})())", "Object.prototype.toString.call(new (class extends Boolean {})())", "Object.prototype.toString.call(new (class extends Date {})())", "Object.prototype.toString.call(new (class extends RegExp {})('a'))", "Object.prototype.toString.call(new (class extends Function {})())", "Object.prototype.toString.call(new (class extends Map {})())", "Object.prototype.toString.call(new (class { get [Symbol.toStringTag]() { return 'Cls' } })())", "Object.prototype.toString.call(new (class { static get [Symbol.toStringTag]() { return 'Cls' } })())", "Object.prototype.toString.call(class { static get [Symbol.toStringTag]() { return 'Cls' } })",
  "Object.prototype.toString.call(Symbol.iterator)", "Object.prototype.toString.call(Object(Symbol.iterator))", "'' + { [Symbol.toStringTag]: 'X' }", "`${{ [Symbol.toStringTag]: 'X' }}`", "String(new Error('x'))", "String(new Error('x', { cause: 1 }))", "String(Object.create(Error.prototype))", "String([1, [2, 3]])", "String(function f() {})", "String({ toString: null, valueOf() { return 'v' } })", "String(Object.create(null, { toString: { value() { return 'n' } } }))",
];
for (const c of toStr) add(ev(c));

// ---- split com limit, sort sem comparador, undefined/holes
const splitCases = [
  "'a,b,c'.split(',', 2)", "'a,b,c'.split(',', 0)", "'a,b,c'.split(',', 1)", "'a,b,c'.split(',', 10)", "'a,b,c'.split(',', -1)", "'a,b,c'.split(',', -1).length", "'a,b,c'.split(',', 2 ** 32)", "'a,b,c'.split(',', 2 ** 32 + 1)", "'a,b,c'.split(',', 2 ** 32 - 1)", "'a,b,c'.split(',', 4294967296.5)", "'a,b,c'.split(',', NaN)", "'a,b,c'.split(',', undefined)", "'a,b,c'.split(',', null)", "'a,b,c'.split(',', '2')", "'a,b,c'.split(',', 1.9)", "'a,b,c'.split(',', Infinity)", "'a,b,c'.split(',', -Infinity)", "'a,b,c'.split(',', -0)", "'a,b,c'.split(',', true)", "'a,b,c'.split(',', {})", "'a,b,c'.split(',', { valueOf() { return 2 } })", "'a,b,c'.split(',', Symbol())", "'a,b,c'.split(',', 1n)",
  "''.split(',')", "''.split('')", "''.split(',', 0)", "''.split(/(?:)/)", "''.split(/a/)", "''.split(/a/, 0)", "'abc'.split('')", "'abc'.split('', 2)", "'abc'.split(undefined)", "'abc'.split(undefined, 0)", "'abc'.split()", "'abc'.split(null)", "'a null b'.split(null)", "'abc'.split(/(?:)/)", "'abc'.split(/(?:)/, 2)", "'abc'.split(/b/)", "'abc'.split(/(b)/)", "'abc'.split(/(b)/, 2)", "'abc'.split(/(b)/, 1)", "'abc'.split(/(x)?b/)", "'abc'.split(/(x)?b/, 2)", "'abc'.split(/(?:x)|b/)", "'abcabc'.split(/b/, 2)", "'abcabc'.split(/b/y)", "'abcabc'.split(/B/i)", "'a1b2c'.split(/\\d/)", "'a1b2c'.split(/(\\d)/, 3)", "'a\\nb'.split(/$/m)", "'ab'.split(/(?=b)/)", "'ab'.split(/\\b/)", "'ab cd'.split(/\\s*/)", "'abc'.split(/a*?/)", "'abc'.split(/a*/)", "'abc'.split(/a*/, 2)", "'test'.split(/(?:)/u)", "'\\u{1F600}a'.split('')", "'\\u{1F600}a'.split(/(?:)/u)", "'\\u{1F600}a'.split(/(?:)/)", "'\\u{1F600}a'.split(/(?:)/u, 1)",
  "'a,b'.split({ [Symbol.split](s, l) { return [s, l] } }, 5)", "'a,b'.split({ [Symbol.split]: null }, 5)", "'a,b'.split({ [Symbol.split]: undefined, toString() { return ',' } })", "'a,b'.split({ toString() { return ',' } })", "'a,b'.split({ [Symbol.split]: 1 })", "'ab'.split(/(?:)/, 2 ** 32 + 1)", "'a,b,c'.split(',', 4294967297)", "RegExp.prototype[Symbol.split].call(/,/, 'a,b,c', 2)", "RegExp.prototype[Symbol.split].call({}, 'a')", "RegExp.prototype[Symbol.split].length", "String.prototype.split.length", "String.prototype.split.call(null, ',')", "String.prototype.split.call(undefined)", "String.prototype.split.call(12345, 3)", "String.prototype.split.call(true, 'r')", "String.prototype.split.call({ toString() { return 'a-b' } }, '-', 1)", "'abc'.split('b', { valueOf() { throw new RangeError('l') } })", "'abc'.split({ toString() { throw new RangeError('s') } }, { valueOf() { throw new RangeError('l') } })", "'abc'.split(/b/, { valueOf() { throw new RangeError('l') } })", "class X extends RegExp { static get [Symbol.species]() { return RegExp } } 'abc'.split(new X('b'))", "var re = /b/; re.constructor = { [Symbol.species]: function (p, f) { return new RegExp(p, f) } }; 'abc'.split(re)", "var re = /b/g; re.lastIndex = 5; 'abc'.split(re); re.lastIndex", "var re = /b/y; 'abc'.split(re)", "var re = /(b)/; re.exec = function () { return null }; 'abc'.split(re)",
];
for (const c of splitCases) add(tj(`(${c})`));
const sortCases = [
  "[3, undefined, 1, undefined, 2].sort()", "[3, undefined, 1, undefined, 2].sort((a, b) => b - a)", "[3, undefined, 1, undefined, 2].sort(() => 0)", "[undefined, undefined].sort()", "[undefined].sort()", "[].sort()", "[3, , 1, , 2].sort()", "[3, , 1, , 2].sort().length", "'0' in [3, , 1].sort()", "2 in [3, , 1].sort()", "Object.keys([3, , 1, , 2].sort()).join()", "Object.keys([3, , 1, undefined, 2].sort()).join()", "Object.keys([, , 1].sort()).join()",
  "[3, , 1, undefined, , 2].sort()", "[3, , 1, undefined, , 2].sort((a, b) => b - a)", "[, 'b', , 'a'].sort()", "[, 'b', , 'a'].sort().length", "[10, 9, 1, 2].sort()", "[10, 9, 1, 2].sort((a, b) => a - b)", "['b', 'a', 'B', 'A'].sort()", "[true, false, null, undefined, NaN, 0, -0, '', 'a'].sort().map(String)", "[-1, -2, 0, 1, 2].sort()", "[1, 2, 3].sort(undefined)", "[3, 2, 1].sort(undefined)", "[3, 2, 1].sort(null)", "[3, 2, 1].sort({})", "[3, 2, 1].sort(1)", "[3, 2, 1].sort('a')", "[3, 2, 1].sort(true)", "[3, 2, 1].sort(Symbol())", "[].sort(1)", "[1].sort(1)", "[3, 2, 1].sort(class {})",
  "[3, 2, 1].sort(function () { return NaN })", "[3, 2, 1].sort(function () { return undefined })", "[3, 2, 1].sort(function () { return '1' })", "[3, 2, 1].sort(function () { return { valueOf() { return -1 } } })", "[3, 2, 1].sort(function () { throw new RangeError('c') })", "[3, 2, 1].sort(function () { return Symbol() })", "[3, 2, 1].sort(function () { return 1n })", "var n = 0; [3, 2, 1].sort(function () { n++; return 0 }); n > 0",
  "var o = { length: 3, 0: 'c', 1: 'a', 2: 'b' }; Array.prototype.sort.call(o); o[0] + o[1] + o[2]", "var o = { length: 3, 0: 'c', 2: 'b' }; Array.prototype.sort.call(o); Object.keys(o).join()", "var o = { length: 3, 0: 'c', 1: undefined, 2: 'b' }; Array.prototype.sort.call(o); Object.keys(o).join() + String(o[2])", "Array.prototype.sort.call('abc')", "Array.prototype.sort.call(null)", "Array.prototype.sort.call(undefined)", "Array.prototype.sort.call(1)", "Array.prototype.sort.call(Object.freeze([2, 1]))", "Array.prototype.sort.call(Object.freeze([1]))", "Array.prototype.sort.call(Object.freeze([]))", "var a = [2, 1]; Object.defineProperty(a, 0, { writable: false }); a.sort()", "Array.prototype.sort.call({ length: 2 ** 32 })", "Array.prototype.sort.length", "Array.prototype.sort.name",
  "var a = [3, 2, 1]; a.sort() === a", "var a = [1, 2, 3]; a.sort(function (x, y) { a.length = 0; return y - x }); a.length", "var a = [3, 2, 1]; a.sort(function (x, y) { a.push(9); return x - y }); a.length", "var a = [3, 2, 1]; a.sort(function (x, y) { delete a[0]; return x - y })", "var a = [3, 2, 1]; Object.defineProperty(a, 1, { get() { return 5 }, set(v) {}, configurable: true }); a.sort(); a.join()", "[{ toString() { return 'b' } }, { toString() { return 'a' } }].sort().map(String)", "[{ toString() { throw new RangeError('s') } }, 1].sort()", "[1, { toString() { throw new RangeError('s') } }].sort()", "[undefined, { toString() { throw new RangeError('s') } }].sort()", "[Symbol(), 1].sort()", "[1, Symbol()].sort()", "[1n, 2, 1].sort()", "[20n, 3, 100].sort()", "['\\u{1F600}', '\\uffff'].sort().map(s => s.length)", "['é', 'e', 'z'].sort()", "['a10', 'a9', 'a1'].sort()", "[0, -0, 0].sort().map(x => 1 / x)", "[NaN, 1, NaN, 0].sort()", "[Infinity, -Infinity, 0].sort()", "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].sort()", "Array.from({ length: 20 }, (_, i) => 20 - i).sort().join()", "Array.from({ length: 20 }, (_, i) => (i * 7) % 20).sort((a, b) => a - b).join()",
  "[3, 1, 2].toSorted()", "[3, undefined, 1].toSorted()", "[3, , 1].toSorted()", "Object.keys([3, , 1].toSorted()).join()", "[3, 1, 2].toSorted(1)", "[3, 1, 2].toSorted(undefined)", "Array.prototype.toSorted.length", "new Uint8Array([3, 1, 2]).sort()", "new Uint8Array([3, 1, 2]).sort(undefined)", "new Uint8Array([3, 1, 2]).sort(null)", "new Float64Array([3, NaN, -0, 0, -Infinity]).sort().join()", "new Float64Array([0, -0]).sort().map(x => 1 / x).join()", "new Uint8Array([3, 1, 2]).sort(1)", "new Uint8Array([3, 1, 2]).sort(function (a, b) { return b - a }).join()",
];
for (const c of sortCases) add(tj(`(${c})`));

// ---- Date: métodos legados extras e parsing, strings com aspas e Function/RegExp em modo estrito vs sloppy
const extra = [
  "(function () { return this === globalThis })()", "(function () { 'use strict'; return this })()", "(function () { return typeof this })()", "(function () { return typeof this }).call(1)", "(function () { 'use strict'; return typeof this }).call(1)", "(function () { return this }).call('s') instanceof String", "(function () { return this === globalThis }).call(null)", "(function () { return this === globalThis }).call(undefined)", "(function () { 'use strict'; return this }).call(null)",
  "(function (a, a) { return a })(1, 2)", "(function (a, a) { 'use strict' })", "(function (a, a) { return arguments[0] })(1, 2)", "function f(a) { arguments[0] = 2; return a } f(1)", "function f(a) { a = 2; return arguments[0] } f(1)", "function f(a) { 'use strict'; arguments[0] = 2; return a } f(1)", "function f(a = 0) { arguments[0] = 2; return a } f(1)", "function f(a, ...r) { arguments[0] = 2; return a } f(1)", "function f(a) { delete arguments[0]; arguments[0] = 5; return a } f(1)", "function f(a) { Object.defineProperty(arguments, '0', { writable: false }); a = 7; return arguments[0] } f(1)",
  "var yield = 1; yield", "var let = 1; let", "var static = 1; static", "var implements = 1; implements", "'use strict'; var yield", "'use strict'; var let", "'use strict'; var static", "'use strict'; var implements", "'use strict'; var interface", "'use strict'; var package", "'use strict'; var private", "'use strict'; var protected", "'use strict'; var public", "var await = 1; await", "var async = 1; async", "var of = 1; of", "var get = 1, set = 2; get + set",
  "'use strict'; eval = 1", "'use strict'; arguments = 1", "'use strict'; var eval", "'use strict'; function eval() {}", "'use strict'; (function arguments() {})", "'use strict'; (eval) => 1", "'use strict'; ({ eval } = {})", "'use strict'; delete x", "'use strict'; delete (x)", "'use strict'; delete ((x))", "'use strict'; delete x.y", "delete x", "'use strict'; undeclared = 1", "undeclared2 = 1; typeof undeclared2", "'use strict'; NaN = 1", "NaN = 1; NaN", "'use strict'; undefined = 1", "'use strict'; Infinity++",
  "'use strict'; Object.freeze({ a: 1 }).a = 2", "Object.freeze({ a: 1 }).a = 2", "'use strict'; delete Object.freeze({ a: 1 }).a", "delete Object.freeze({ a: 1 }).a", "'use strict'; 'abc'.length = 1", "'abc'.length = 1", "'use strict'; 'abc'[0] = 'x'", "'abc'[0] = 'x'", "'use strict'; (1).x = 1", "(1).x = 1", "'use strict'; Symbol().x = 1", "'use strict'; delete 'abc'[0]", "delete 'abc'[0]", "'use strict'; delete [].length", "delete [].length",
  "'use strict'; ({ get a() { return 1 } }).a = 2", "({ get a() { return 1 } }).a = 2", "'use strict'; Object.preventExtensions({}).a = 1", "Object.preventExtensions({}).a = 1", "'use strict'; Object.defineProperty({}, 'a', { value: 1 }).a = 2", "'use strict'; undefined.x", "'use strict'; null.x = 1", "'use strict'; var f = function () { return this }; f()", "var f = function () { return this === globalThis }; f()", "'use strict'; var o = { f() { return typeof this } }; (0, o.f)()",
  "0 || { if: 1 }.if", "({ if: 1, class: 2, function: 3 }).class", "({ get if() { return 1 } }).if", "var o = { true: 1, null: 2, undefined: 3 }; o.true + o.null + o.undefined", "var a = 1; a\n++\na", "var a = 1, b = 2; a\n++b; a + ',' + b", "return 1", "new.target", "function f() { return new.target } f()", "function f() { return typeof new.target } new f() instanceof Object", "(() => new.target)", "super.x", "yield: 1", "await: 1", "async\nfunction f() {}", "var async; async\nfunction f() {}", "x = { async\nm() {} }", "let\nlet = 1", "var let; let\n[0] = 1", "if (1) let\n{}", "do ; while (0) 1", "do ; while (0)\n1", "do ; while (0) x = 1", "if (1) ; else ; 1", "a: a: ;", "a: { a: ; }", "a: { b: ; } a: ;", "a: function f() {} a: ;", "break", "continue", "a: { break a }", "a: { continue a }", "a: while (0) { function f() { break a } }", "a: while (0) { (() => { continue a }) }", "while (0) { break }", "switch (1) { case 1: break; continue }", "a: while (0) { continue a }", "a: b: while (0) { continue a }", "a: { b: while (0) { continue a } }", "a: if (1) { continue a }", "a: for (;;) { break a }", "a: do { continue a } while (0)", "var a; a: ; a",
  "/* \n */ --> x\n1", "1 /* \n */ --> x\n", "x = 1 /* */ <!-- y\n; x", "x = 1; x /*\n*/ --> y", "var x = 5; x\n--> 0", "var x = 5; x --> 0", "var x = 5; x\n-->\nx", "var x = 5; (x--) > 0", "var x = 5; x-- > 0",
];
for (const c of extra) add(ev(c));

// ---- Duplicatas de nomes, modo estrito e getters/setters legados em classes e funções
const extra2 = [
  "String.prototype.substr.call('abc', -1, 1)", "'abc'.substring(2, 0)", "'abc'.substring(-1, 2)", "'abc'.substring(NaN)", "'abc'.substring(1, undefined)", "'abc'.slice(2, 0)", "'abc'.slice(-2)", "'abc'.substr(-2, 1)", "'abc'.at(-1)", "'abc'.concat(1, null)", "'a-b-c'.replace('-', '$&$&')", "'a-b-c'.replace('-', '$`')", "'a-b-c'.replace('-', \"$'\")", "'abc'.replace('b', '$0')", "'abc'.replace('b', '$1')", "'abc'.replace(/(b)/, '$01')", "'abc'.replace(/(b)/, '$10')", "'abc'.replace(/(b)/, '$2')", "'abc'.replace(/(b)/, '$00')", "'abc'.replace(/(b)/, '$<x>')", "'abc'.replace(/(?<x>b)/, '$<x>')", "'abc'.replace(/(?<x>b)/, '$<y>')", "'abc'.replace(/(?<x>b)/, '$<x')", "'abc'.replace(/(b)/, '$$')", "'abc'.replace(/(b)/, '$')", "'abc'.replace(/(b)/, '$a')",
  "'abc'.indexOf('')", "'abc'.lastIndexOf('')", "'abc'.lastIndexOf('c', -5)", "'abc'.localeCompare('abd')", "'abc'.normalize('NFD')", "'abc'.normalize('X')", "'abc'.repeat(-1)", "'abc'.repeat(0)", "'abc'.padStart(5, '')", "'abc'.padEnd(6, '12')", "'abc'.startsWith(/a/)", "'abc'.includes(/a/)", "'abc'.endsWith({ [Symbol.match]: false, toString() { return 'c' } })",
  "'abc'.match(/(?<x>b)/).groups.x", "'abc'.match(/x/g)", "'abc'.match(/x/)", "'abcabc'.match(/b/g).length", "'abc'.match()", "'abc'.match(null)", "'abc'.match('b').index", "'abc'.search('b')", "'abc'.search()", "'abc'.search(/(?:)/)",
  "Number('  12  ')", "Number('1,2')", "Number('0x')", "Number('.5')", "Number('5.')", "Number('+.5')", "Number('1e')", "Number('Infinity')", "Number('-Infinity')", "Number('infinity')", "Number('\\u00a0 7 \\ufeff')", "parseFloat('1.5e3x')", "parseFloat('.e3')", "parseFloat('-.5')", "parseFloat('0x10')", "parseInt('  -0x1f')", "parseInt('1e3')", "parseInt('12', 37)", "parseInt('12', 1)", "parseInt('12', 0)", "parseInt('')", "parseInt('9007199254740993')",
  "(25).toString(36)", "(0.5).toString(2)", "(255).toString(16)", "(-255).toString(16)", "(1e21).toString(7).length", "(1.1).toFixed(20)", "(1e21).toFixed(2)", "(0.000001).toString()", "(1e-7).toString()", "(123.456).toPrecision(4)", "(0).toExponential(2)", "(-1.5).toFixed(0)", "(2.5).toFixed(0)", "(1.005).toFixed(2)", "(1234.5678).toFixed(-1)",
  "[1, 2, 3].join(undefined)", "[1, 2, 3].join(null)", "[null, undefined, 1].join()", "[[], [[]], 1].toString()", "[1, [2, [3, [4]]]].flat(Infinity).join()", "Array(3).join('-')", "Array.apply(null, Array(2)).join('x')", "[,].length", "[, ,].length", "[1, , 2].length", "0 in [, 1]", "1 in [, 1]", "Array(3).fill().length", "Array.from('abc').join()", "Array.of(7).length", "Array(2 ** 32)", "Array(-1)", "Array(1.5)", "new Array('3').length", "[].concat([1], 2, [[3]]).length", "[1, 2, 3].indexOf(2, -1)", "[NaN].indexOf(NaN)", "[NaN].includes(NaN)", "[1, 2, 3].reverse().join()", "[1, 2, 3].lastIndexOf(3, -5)", "[3, 2, 1].findLast(x => x > 1)",
  "Object.getOwnPropertyNames(String.prototype).filter(n => /^(substr|trimLeft|trimRight|anchor|big|blink|bold|fixed|fontcolor|fontsize|italics|link|small|strike|sub|sup)$/.test(n)).sort().join()", "Object.getOwnPropertyNames(Date.prototype).filter(n => /^(getYear|setYear|toGMTString)$/.test(n)).sort().join()", "Object.getOwnPropertyNames(Object.prototype).sort().join()", "Object.getOwnPropertyNames(RegExp.prototype).sort().join()", "Object.getOwnPropertyNames(globalThis).filter(n => /^(escape|unescape)$/.test(n)).sort().join()",
  "Object.getOwnPropertyNames(String.prototype).includes('trimStart')", "typeof String.prototype.at", "typeof ''.isWellFormed", "typeof String.prototype.toWellFormed", "typeof String.prototype.replaceAll", "typeof String.prototype.matchAll", "typeof Array.prototype.group", "typeof Array.prototype.groupBy", "typeof Object.groupBy", "typeof Map.groupBy", "typeof Array.prototype.findLast", "typeof Array.fromAsync", "typeof Object.hasOwn", "typeof structuredClone", "typeof Symbol.dispose", "typeof Iterator", "typeof Promise.withResolvers", "typeof Array.prototype.with", "typeof Error.captureStackTrace",
];
for (const c of extra2) add(ev(c));

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "annexb-golden-"));
const file = path.join(dir, "annexb_case.js");
const preload = path.join(dir, "preload.js");
// O bun roda arquivos como módulo (estrito); o programa precisa de semântica de script sloppy, então o preload o
// executa com vm.runInThisContext e o arquivo principal fica vazio.
fs.writeFileSync(
  preload,
  "const vm = require('vm'); const src = require('fs').readFileSync(" + JSON.stringify(file) + ", 'utf8');\n" +
    "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') });\n" +
    "try { vm.runInThisContext(src, { filename: 'annexb_case.js' }) } catch (e) { globalThis.R = undefined }\n",
);
const runner = path.join(dir, "runner.js");
fs.writeFileSync(runner, "");
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  // `R = String(...)` precisa gravar na global mesmo em modo sloppy: já grava (atribuição a não declarada).
  const source = body;
  fs.writeFileSync(file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, runner], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "UTC" } });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
