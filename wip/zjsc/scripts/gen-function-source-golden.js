// Gera tests/golden/function_source_bun.tsv: `Function.prototype.toString` (texto-fonte exato), `name` e `length`
// de funções declaradas, expressões, arrows, métodos, classes, `new Function`, bound, nativas, Proxy e eval,
// medidos no bun 1.4.2. Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// O arquivo se chama `function_source_case.js` dos dois lados; qualquer resultado com caminho da máquina é descartado.
// Uso: bun scripts/gen-function-source-golden.js > tests/golden/function_source_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram, prepareScript, RESULT_PRELOAD } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const lines = (...rows) => rows.join("\n");
const programs = [];

// Registra o texto, o nome e o comprimento de `f` num JSON (os escapes deixam as quebras de linha visíveis).
const report = "R = JSON.stringify([Function.prototype.toString.call(f), f.name, f.length])";
const addValue = expr => programs.push(lines(`var f = (\n${expr}\n);`, report));
const addValueViaString = expr => programs.push(lines(`var f = (\n${expr}\n);`, "R = String(f) + '|' + f.toString()"));

// ---- Funções declaradas.
const declarations = [
  ["function d1() {}", "d1"],
  ["function d2 ( a ) { return a }", "d2"],
  ["function d3(a, b = 1, c) {}", "d3"],
  ["function* d4() { yield 1 }", "d4"],
  ["function * d5 ( ) { yield 1 }", "d5"],
  ["async function d6() { await 1 }", "d6"],
  ["async   function   d7 ( ) { }", "d7"],
  ["async function* d8() { yield 1 }", "d8"],
  ["async function * d9 ( x ) { yield x }", "d9"],
  ["function /* a */ d10 /* b */ ( /* c */ ) /* d */ { /* e */ }", "d10"],
  ["function d11() { // c\n}", "d11"],
  ["function d12() { /* multi\nline */ }", "d12"],
  ["function\td13\t(\t)\t{\t}", "d13"],
  ["function\nd14\n(\na\n)\n{\nreturn a\n}", "d14"],
  ["function d15(a, // c\n b) {}", "d15"],
  ["function d16() {}\n\n\n", "d16"],
  ["function get() {}", "get"],
  ["function set() {}", "set"],
  ["function static() {}", "static"],
  ["function async() {}", "async"],
  ["function of() {}", "of"],
  ["function await() {}", "await"],
  ["function yield() {}", "yield"],
  ["function let() {}", "let"],
  ["function \\u0061bc() {}", "abc"],
  ["function \\u{61}bc() {}", "abc"],
  ["function caf\u00e9() {}", "caf\u00e9"],
  ["function \u03c0() {}", "\u03c0"],
  ["function $d() {}", "$d"],
  ["function _d() {}", "_d"],
  ["function \u{1d49c}x() {}", "\u{1d49c}x"],
  ["function d17({ a, b }, [c, d], ...rest) {}", "d17"],
  ["function d18() { 'use strict'; return 1 }", "d18"],
  ["function d19() { return `t ${1}` }", "d19"],
  ["function d20() { return /re\\//g }", "d20"],
  ["function d21() { return '\\u0061' }", "d21"],
  ["class D22 {}", "D22"],
  ["class D23 extends Object { m() {} }", "D23"],
  ["class /* a */ D24 /* b */ { /* c */ }", "D24"],
  ["class D25 { constructor(a, b) {} }", "D25"],
  ["class D26 { static x = 1; y = 2 }", "D26"],
  ["class D27 { static { var z = 1 } }", "D27"],
  ["class D28 extends (class {}) {}", "D28"],
  ["class D29 { #p = 1; static has(o) { return #p in o } }", "D29"],
];
for (const [decl, name] of declarations) {
  programs.push(lines(decl, `var f = ${name};`, report));
  programs.push(lines(decl, `R = ${name}.toString() === Function.prototype.toString.call(${name}) && String(${name}) === ${name}.toString() ? ${name}.toString() : 'diff'`));
}

// ---- Expressões de função, arrows e classes.
const expressions = [
  "function () {}",
  "function(){}",
  "function f() {}",
  "function f(a, b) { return a + b }",
  "function   spaced  (  a ,  b  )   {  return a  }",
  "function /* c1 */ f /* c2 */ ( /* c3 */ a /* c4 */ ) /* c5 */ { /* c6 */ }",
  "function f() { // line comment\n  return 1 // trailing\n}",
  "function\nf\n(\na\n)\n{\nreturn a\n}",
  "function\n(\n)\n{\n}",
  "function f(a /* x */, b // y\n, c) {}",
  "function* () {}",
  "function*(){}",
  "function * g ( ) { yield 1 }",
  "async function () {}",
  "async function(){}",
  "async  function  a ( )  { }",
  "async function* () {}",
  "async function*(){}",
  "async function * ag ( x ) { yield x }",
  "function \\u0061b() {}",
  "function caf\u00e9() {}",
  "function f\u0301() {}",
  "function \u4f60\u597d() {}",
  "function \ud83d\ude00() {}".replace("\ud83d\ude00", "x\u{1f600}".slice(0, 1)),
  "function f() { return '\u00e9\u4f60\ud83d\ude00' }",
  "function f() { return '\\u0061 \\x61' }",
  "function f(a = '\u00e9', { b = 2 } = {}) {}",
  "function f(a, b = 1, c) {}",
  "function f(...r) {}",
  "function f(a, ...r) {}",
  "function f(a = 1, b) {}",
  "function f(a, b, c, d, e, f, g, h) {}",
  "function get() {}",
  "function set() {}",
  "function static() {}",
  "function async() {}",
  "function f() { return function g() {} }",
  "function f() { return () => 1 }",
  // arrows
  "() => 1",
  "()=>1",
  "(a) => a",
  "a => a",
  "a=>a",
  "  a  =>  a  ",
  "(a, b) => { return a + b }",
  "(a,b)=>{return a+b}",
  "( a , b ) => a",
  "(\na\n) => a",
  "a\n=> a",
  "(a) /* c */ => /* d */ a",
  "async () => 1",
  "async()=>1",
  "async a => a",
  "async a=>a",
  "async (a, b) => { await a }",
  "async  ( ) => { }",
  "async (\n) => 1",
  "async /* c */ a => a",
  "(a = 1, { b }, [c], ...d) => 0",
  "(a, b = 1) => 0",
  "([a, b]) => a",
  "({ a }) => a",
  "(...a) => a",
  "() => ({ x: 1 })",
  "() => {}",
  "x => y => x + y",
  "x => { return x } // trailing",
  "(x) => x /* c */",
  "async x => await x",
  "() => `t`",
  "() => /re/",
  "() => 'é'",
  // classes
  "class {}",
  "class A {}",
  "class A { }",
  "class   A   {   }",
  "class\nA\n{\n}",
  "class extends Object {}",
  "class A extends Object {}",
  "class A extends Object { constructor() { super() } }",
  "class /* c */ A /* d */ extends /* e */ Object /* f */ { }",
  "class A { m() {} }",
  "class A { static s = 1; #p = 2; get g() { return 1 } }",
  "class A { static { } }",
  "class A { static { this.x = 1 } static { this.y = 2 } }",
  "class { static { } }",
  "class { x = 1 }",
  "class { x }",
  "class { x; y; z }",
  "class { 'q' = 1; 42 = 2; [1 + 1] = 3 }",
  "class { static x }",
  "class { #x; static a(o) { return o.#x } }",
  "class { constructor(a, b, c) {} }",
  "class { constructor(a, b = 1, c) {} }",
  "class { constructor(...a) {} }",
  "class B { constructor(a) { this.a = a } }",
  "class A { /* comment */ m() { /* in */ } // tail\n}",
  "class A { // c\n m() {} }",
  "class A {\n  constructor() {}\n  m() {}\n  static s() {}\n}",
  "class A extends (class {}) {}",
  "class A extends null {}",
  "class A { static name() {} }",
  "class A { static name = 'x' }",
  "class A { static length = 5 }",
  "class A { static async *[Symbol.iterator]() {} }",
  "class A { get [Symbol.toStringTag]() { return 'A' } }",
  "class A { 'constructor'() {} }",
  "class \\u0061b {}",
  "class caf\u00e9 {}",
  "class A { m\u00e9() {} }",
  "class A { static async m() {} static *g() {} static async *ag() {} }",
  "(class {})",
  "(class A {})",
  "(class A extends Object {})",
  // objetos e atribuições com inferência de nome
  "({ f: function () {} }).f",
  "({ f: () => 1 }).f",
  "({ f: async () => 1 }).f",
  "({ f: class {} }).f",
  "({ f: function* () {} }).f",
  "({ f: async function* () {} }).f",
  "({ 'a b': function () {} })['a b']",
  "({ 1: () => 1 })[1]",
  "({ [Symbol('s')]: function () {} })[Object.getOwnPropertySymbols({ [Symbol('s')]: 0 })[0]]".replace(/^.*$/, "(() => { var s = Symbol('desc'); return { [s]: function () {} }[s] })()"),
  "(() => { var s = Symbol(); return { [s]: function () {} }[s] })()",
  "(() => { var s = Symbol('d'); return { [s]() {} }[s] })()",
  "(() => { var s = Symbol('d'); return class { static [s]() {} }[s] })()",
  "(() => { var v = function () {}; return v })()",
  "(() => { var v = () => 1; return v })()",
  "(() => { var v = class {}; return v })()",
  "(() => { let v; v = function () {}; return v })()",
  "(() => { var o = {}; o.p = function () {}; return o.p })()",
  "(() => { var [v = function () {}] = []; return v })()",
  "(() => { var { v = () => 1 } = {}; return v })()",
  "(() => { function w(p = function () {}) { return p } return w() })()",
  "(() => { var v = (function () {}); return v })()",
  "(() => { var v = (0, function () {}); return v })()",
  "(() => { var v = function () {}.bind(); return v })()",
];
for (const expr of expressions) {
  addValue(expr);
  addValueViaString(expr);
}

// ---- Métodos: contexto x tipo x chave. Cada combinação recupera a função e relata texto, nome e comprimento.
const keys = [
  ["m", "'m'"],
  ["'q k'", "'q k'"],
  ["42", "42"],
  ["['c' + 'd']", "'cd'"],
  ["[Symbol.iterator]", "Symbol.iterator"],
  ["\\u0061b", "'ab'"],
  ["get", "'get'"],
  ["set", "'set'"],
  ["static", "'static'"],
  ["async", "'async'"],
  ["caf\u00e9", "'caf\u00e9'"],
];
const kinds = [
  { head: "", params: "a, b = 1", pick: (o, k) => `${o}[${k}]` },
  { head: "*", params: "", pick: (o, k) => `${o}[${k}]` },
  { head: "async ", params: "a", pick: (o, k) => `${o}[${k}]` },
  { head: "async *", params: "a, b", pick: (o, k) => `${o}[${k}]` },
  { head: "get ", params: "", pick: (o, k) => `Object.getOwnPropertyDescriptor(${o}, ${k}).get` },
  { head: "set ", params: "v", pick: (o, k) => `Object.getOwnPropertyDescriptor(${o}, ${k}).set` },
];
for (const [key, lookup] of keys) {
  for (const kind of kinds) {
    const member = `${kind.head}${key}(${kind.params}) { ${kind.head.startsWith("get") ? "return 1" : ""} }`;
    programs.push(lines(`var o = { ${member} };`, `var f = ${kind.pick("o", lookup)};`, report));
    programs.push(lines(`var C = class { ${member} };`, `var f = ${kind.pick("C.prototype", lookup)};`, report));
    programs.push(lines(`var C = class { static ${member} };`, `var f = ${kind.pick("C", lookup)};`, report));
  }
}
// Espaçamento e comentários no cabeçalho do método.
const spacedMethods = [
  "({ m ( a ) { return a } }).m",
  "({ async   m ( ) { } }).m",
  "({ *   m ( ) { } }).m",
  "({ async   *   m ( ) { } }).m",
  "({ get   x ( ) { return 1 } }).__lookupGetter__('x')",
  "({ set   x ( v ) { } }).__lookupSetter__('x')",
  "({ /* a */ m /* b */ ( /* c */ ) /* d */ { } }).m",
  "({ m\n(\n)\n{\n} }).m",
  "({ async\nm() {} }).m".replace("async\nm", "async m"),
  "(class { static   m ( ) { } }).m",
  "(class { static\nm() {} }).m",
  "(class { static /* c */ async /* d */ * /* e */ m() {} }).m",
  "(class { static get   g ( ) { return 1 } }).__lookupGetter__('g')",
  "(class { /* a */ get /* b */ g /* c */ () { return 1 } }).prototype.__lookupGetter__('g')",
  "(class { m() {} }).prototype.m",
  "(class { ['x' + 1]() {} }).prototype.x1",
  "(class { 'str'() {} }).prototype.str",
  "(class { 1.5() {} }).prototype[1.5]",
  "(class { 0x10() {} }).prototype[16]",
  "(class { 1n() {} }).prototype[1]",
  "(class { async *[Symbol.asyncIterator]() {} }).prototype[Symbol.asyncIterator]",
  "(class { static [Symbol.hasInstance](v) { return true } })[Symbol.hasInstance]",
];
for (const expr of spacedMethods) addValue(expr);
// Métodos privados.
for (const head of ["", "*", "async ", "async *"]) {
  programs.push(lines(`class C { #m(a, b) { } static get() { return new C().#m } }`.replace("#m(a, b)", `${head}#m(a, b)`), "var f = C.get();", report));
  programs.push(lines(`class C { static ${head}#m(a) { } static get() { return C.#m } }`, "var f = C.get();", report));
}
programs.push(lines("class C { #p = function () {}; static get() { return new C().#p } }", "var f = C.get();", report));
programs.push(lines("class C { static #p = () => 1; static get() { return C.#p } }", "var f = C.get();", report));
programs.push(lines("class C { #x; static m(o) { return o.#x } }", "R = C.m.toString()"));
programs.push(lines("class C { static #g() { return 1 } static t() { return C.#g.name } }", "R = C.t()"));
programs.push(lines("class C { #g() { return 1 } t() { return this.#g.name } }", "R = new C().t()"));

// ---- Construtor Function e irmãos (texto exato com `anonymous` e quebras de linha).
const constructors = [
  ["Function", ""],
  ["Function", "''"],
  ["Function", "'return 1'"],
  ["Function", "'a', 'b', 'return a + b'"],
  ["Function", "'a, b', 'return a + b'"],
  ["Function", "'a', 'b,c', 'return 1'"],
  ["Function", "'a = 1', 'return a'"],
  ["Function", "'...a', 'return a'"],
  ["Function", "'/* c */ return 1 // x'"],
  ["Function", "'a //', 'return a'"],
  ["Function", "'a /* c */, b', '\\n return a\\n'"],
  ["Function", "'a', 'b', ''"],
  ["Function", "'', ''"],
  ["Function", "'\\n', 'return 1'"],
  ["Function", "'{ a }', 'return a'"],
  ["Function", "'[a]', 'return a'"],
  ["Function", "'return this'"],
  ["Function", "'\\u0061', 'return \\u0061'"],
  ["Function", "'caf\u00e9', 'return caf\u00e9'"],
  ["Function", "'a', 'return `x ${a}`'"],
  ["Function", "'a', '\"use strict\"; return a'"],
  ["Function", "'a', 'b', 'c', 'd', 'return a'"],
  ["Function", "'  a  ,  b  ', '  return a  '"],
  ["Function", "1, 2"],
  ["Function", "undefined"],
  ["Function", "null"],
  ["Function", "{ toString() { return 'return 7' } }"],
  ["Function", "'}'"],
  ["Function", "'a', '} function z() {'"],
  ["GF", ""],
  ["GF", "'yield 1'"],
  ["GF", "'a', 'yield a'"],
  ["GF", "'a', 'b', 'yield a; yield b'"],
  ["GF", "'a, b', ''"],
  ["GF", "'/* c */ yield 1 // x'"],
  ["AF", ""],
  ["AF", "'await 1'"],
  ["AF", "'a', 'b', 'return a + b'"],
  ["AF", "'a = await 1', ''"],
  ["AF", "'return await 1 // c'"],
  ["AGF", ""],
  ["AGF", "'yield 1'"],
  ["AGF", "'a', 'await a; yield a'"],
  ["AGF", "'a', 'b', 'yield* [a, b]'"],
  ["AGF", "'/* c */'"],
];
const ctorExpr = {
  Function: "Function",
  GF: "Object.getPrototypeOf(function* () {}).constructor",
  AF: "Object.getPrototypeOf(async function () {}).constructor",
  AGF: "Object.getPrototypeOf(async function* () {}).constructor",
};
for (const [ctor, args] of constructors) {
  addValue(`${ctorExpr[ctor]}(${args})`);
  addValue(`new ${ctorExpr[ctor]}(${args})`);
}
addValue("Function.call(null, 'a', 'return a')");
addValue("Function.apply(null, ['a', 'b', 'return a'])");
addValue("Reflect.construct(Function, ['a', 'return a'])");
addValue("Reflect.construct(Function, ['a', 'return a'], Object)");
addValue("new (Function.bind(null, 'a'))('return a')");
addValue("Function('a', 'return a').bind(null)");
programs.push(lines("var G = Object.getPrototypeOf(function* () {}).constructor; R = [G.name, G.length, G.toString()].join('|')"));
programs.push(lines("var A = Object.getPrototypeOf(async function () {}).constructor; R = [A.name, A.length, A.toString()].join('|')"));
programs.push(lines("var AG = Object.getPrototypeOf(async function* () {}).constructor; R = [AG.name, AG.length, AG.toString()].join('|')"));
programs.push(lines("R = [Function.name, Function.length, Function.toString()].join('|')"));

// ---- Bound functions.
const bounds = [
  "(function f() {}).bind(null)",
  "(function f(a, b) {}).bind(null, 1)",
  "(function f(a, b, c) {}).bind(null, 1, 2, 3, 4)",
  "(() => 1).bind(null)",
  "(class A {}).bind(null)",
  "(async function a() {}).bind(null)",
  "(function* g() {}).bind(null)",
  "Math.max.bind(null)",
  "Array.prototype.push.bind([])",
  "(function f() {}).bind(null).bind(null)",
  "(function f() {}).bind(null).bind(null).bind(null)",
  "Function.prototype.bind.call(function () {}, null)",
  "(function () {}).bind()",
  "new Proxy(function f() {}, {}).bind(null)",
  "({ m(a) {} }).m.bind(null)",
  "(function f(a) {}).bind(null, 1, 2)",
  "Object.defineProperty(function f() {}, 'name', { value: 'renamed' }).bind(null)",
  "Object.defineProperty(function f() {}, 'name', { value: 42 }).bind(null)",
  "Object.defineProperty(function f(a, b) {}, 'length', { value: 7 }).bind(null)",
  "Object.defineProperty(function f(a, b) {}, 'length', { value: -1 }).bind(null)",
  "Object.defineProperty(function f(a, b) {}, 'length', { value: Infinity }).bind(null, 1)",
  "Object.defineProperty(function f(a, b) {}, 'length', { value: 'x' }).bind(null)",
  "Symbol.prototype[Symbol.toPrimitive].bind(Symbol())",
  "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.bind(new Map)",
];
for (const expr of bounds) addValue(expr);

// ---- Funções nativas: lista fixa mais enumeração de Math, Reflect e JSON.
const natives = [
  "Math.max", "Math.abs", "parseInt", "parseFloat", "isNaN", "isFinite", "eval", "escape", "unescape",
  "encodeURIComponent", "decodeURI", "Object", "Function", "Function.prototype", "Function.prototype.toString",
  "Function.prototype.call", "Function.prototype.apply", "Function.prototype.bind", "Function.prototype[Symbol.hasInstance]",
  "Symbol", "Symbol.for", "Symbol.keyFor", "Map", "Set", "WeakMap", "WeakRef", "Promise", "Promise.resolve", "Promise.all",
  "Promise.prototype.then", "Error", "TypeError", "RangeError", "AggregateError", "Error.captureStackTrace", "JSON.parse",
  "JSON.stringify", "RegExp", "RegExp.prototype.exec", "RegExp.prototype[Symbol.replace]", "RegExp.prototype[Symbol.matchAll]",
  "String", "String.prototype.slice", "String.fromCharCode", "String.prototype[Symbol.iterator]", "Number", "Number.prototype.toFixed",
  "Number.isInteger", "Boolean", "Date", "Date.now", "Date.prototype[Symbol.toPrimitive]", "Reflect.apply", "Proxy", "BigInt",
  "BigInt.asIntN", "ArrayBuffer", "ArrayBuffer.isView", "DataView", "Uint8Array", "Array", "Array.isArray", "Array.from",
  "Array.prototype.push", "Array.prototype.map", "Array.prototype.values", "Array.prototype[Symbol.iterator]",
  "Array.prototype[Symbol.unscopables] && 'u'".replace(/ &&.*/, "").replace("Array.prototype[Symbol.unscopables]", "Object.getPrototypeOf([][Symbol.iterator]())[Symbol.iterator]"),
  "Object.getPrototypeOf([][Symbol.iterator]()).next",
  "Object.getPrototypeOf(new Map()[Symbol.iterator]()).next",
  "Object.getPrototypeOf(function* () {}).constructor",
  "Object.getPrototypeOf(function* () {}).prototype.next",
  "Object.getPrototypeOf(async function () {}).constructor",
  "Object.getPrototypeOf(async function* () {}).constructor",
  "Object.getPrototypeOf(Int8Array)",
  "Object.getPrototypeOf(Int8Array).from",
  "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get",
  "Object.getOwnPropertyDescriptor(Set.prototype, 'size').get",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get",
  "Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
  "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get",
  "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array).prototype, 'length').get",
  "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array), Symbol.species).get",
  "Object.getOwnPropertyDescriptor(Map, Symbol.species).get",
  "Object.getOwnPropertyDescriptor(Array, Symbol.species).get",
  "Object.getOwnPropertyDescriptor(Promise, Symbol.species).get",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').set",
  "Object.getOwnPropertyDescriptor(Error.prototype, 'stack') && 1".replace(/ && 1$/, "").replace(/^.*$/, "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array).prototype, Symbol.toStringTag).get"),
  "Symbol.prototype[Symbol.toPrimitive]",
  "Symbol.prototype[Symbol.toStringTag] || Symbol.prototype.toString",
  "Symbol.prototype.valueOf",
  "Date.prototype[Symbol.toPrimitive]",
  "Map.prototype[Symbol.iterator]",
  "Set.prototype[Symbol.iterator]",
  "Map.prototype.entries",
  "RegExp.prototype[Symbol.split]",
  "RegExp.prototype[Symbol.match]",
  "RegExp.prototype[Symbol.search]",
  "String.prototype.at",
  "Intl.DateTimeFormat",
  "Atomics.add",
  "globalThis.queueMicrotask",
  "globalThis.structuredClone",
  "Object.prototype.hasOwnProperty",
  "Object.prototype.toString",
  "Object.prototype.__defineGetter__",
  "Object.prototype.__lookupGetter__",
  "Object.getPrototypeOf((async function* () {})()).next",
  "Object.getPrototypeOf((async function () {})())" + ".constructor",
];
for (const expr of natives) addValue(expr);
for (const ns of ["Math", "Reflect", "JSON"]) {
  const probe = spawnSync(process.execPath, ["-e", `console.log(JSON.stringify(Object.getOwnPropertyNames(${ns}).filter(n => typeof ${ns}[n] === 'function')))`], { encoding: "utf8" });
  for (const name of JSON.parse(probe.stdout.trim())) addValue(`${ns}[${JSON.stringify(name)}]`);
}

// ---- Proxy de função e objeto que não é função.
const proxies = [
  "new Proxy(function f() {}, {})",
  "new Proxy(class A {}, {})",
  "new Proxy(Math.max, {})",
  "new Proxy(() => 1, {})",
  "new Proxy(async function () {}, {})",
  "new Proxy(function* () {}, {})",
  "new Proxy(new Proxy(function f() {}, {}), {})",
  "new Proxy(function f() {}, { get() { return 1 } })",
  "new Proxy(function f() {}, { apply() { return 1 } })",
  "new Proxy(Function.prototype.toString, {})",
  "new Proxy(function f(a, b) {}.bind(null), {})",
];
for (const expr of proxies) {
  programs.push(lines(`var f = (${expr});`, "R = Function.prototype.toString.call(f)"));
  programs.push(lines(`var f = (${expr});`, "R = [typeof f, f.name, f.length].join('|')"));
}
const nonFunctions = [
  "{}", "null", "undefined", "1", "'s'", "true", "[]", "/x/", "Symbol()", "new Map", "1n", "Object.create(Function.prototype)",
  "new Proxy({}, {})", "new Proxy([], {})", "{ call() {} }", "Math", "JSON", "globalThis", "Symbol.prototype", "Function.prototype.call.call",
  "new Date", "new Error", "Reflect", "Object.assign(() => 1, {}).constructor.prototype.prototype",
];
for (const bad of nonFunctions) {
  programs.push(lines(`try { R = Function.prototype.toString.call(${bad}) } catch (e) { R = e.name + ': ' + e.message }`));
}
programs.push(lines("try { R = Function.prototype.toString() } catch (e) { R = e.name + ': ' + e.message }"));
programs.push(lines("try { Function.prototype.toString.apply(1) } catch (e) { R = e.name + ': ' + e.message }"));
programs.push(lines("try { Function.prototype.toString.bind({})() } catch (e) { R = e.name + ': ' + e.message }"));
programs.push(lines("var o = { toString: Function.prototype.toString }; try { R = o.toString() } catch (e) { R = e.name + ': ' + e.message }"));
programs.push(lines("var f = function () {}; f.toString = 1; R = Function.prototype.toString.call(f)"));
programs.push(lines("var f = function () {}; f.toString = () => 'x'; R = String(f) + '|' + Function.prototype.toString.call(f)"));
programs.push(lines("var f = function a() {}; f.__proto__ = null; R = Function.prototype.toString.call(f)"));
programs.push(lines("var f = function a() {}; Object.setPrototypeOf(f, {}); R = Function.prototype.toString.call(f)"));
programs.push(lines("R = Function.prototype.toString.length + ':' + Function.prototype.toString.name"));
programs.push(lines("R = Function.prototype + '|' + Function.prototype.name + '|' + Function.prototype.length + '|' + typeof Function.prototype"));

// ---- Funções de eval.
const evals = [
  "(0, eval)('(function e1() {})')",
  "eval('(function e2 ( ) { })')",
  "eval('(() => 2)')",
  "eval('(class E {})')",
  "eval('(function () {})')",
  "eval('(async () => 1)')",
  "eval('(function* e3() { yield 1 })')",
  "eval('(async function* () {})')",
  "eval('({ m() {} }).m')",
  "eval('(function e4() { return eval(\"(function inner() {})\") })')()",
  "eval('function e5() {} e5')",
  "eval('var e6 = function () {}; e6')",
  "eval('(function e7(a, b) { /* c */ })')",
  "new Function('return eval(\"(function e8() {})\")')()",
  "eval('(function e9() {})\\n')",
  "eval('/* lead */ (function e10() {}) /* tail */')",
  "eval('(class { static { } })')",
  "eval('(\\u0061 => a)'.replace('a)', '\\u0061)'))".replace(/^.*$/, "eval('(a => a)')"),
  "(0, eval)('(function () {}).bind()')",
];
for (const expr of evals) addValue(expr);

// ---- toString depois de mexer em name (não muda o texto) e length.
const renames = [
  "Object.defineProperty(function f() {}, 'name', { value: 'g' })",
  "Object.defineProperty(function () {}, 'name', { value: 'g' })",
  "Object.defineProperty(function f() {}, 'name', { value: '' })",
  "Object.defineProperty(function f() {}, 'name', { value: 123 })",
  "Object.defineProperty(function f() {}, 'name', { value: undefined })",
  "Object.defineProperty(function f() {}, 'name', { get() { return 'dyn' } })",
  "Object.defineProperty(() => 1, 'name', { value: 'arrow' })",
  "Object.defineProperty(class A {}, 'name', { value: 'B' })",
  "Object.defineProperty(class A { static m() {} }, 'name', { value: 'B' })",
  "Object.defineProperty(async function a() {}, 'name', { value: 'b' })",
  "Object.defineProperty(function* a() {}, 'name', { value: 'b' })",
  "Object.defineProperty(Math.max, 'name', { value: 'maxx' })",
  "Object.defineProperty(function (a, b) {}, 'length', { value: 9 })",
  "Object.defineProperty(function (a, b) {}, 'length', { value: 'x' })",
  "(() => { var f = function a() {}; delete f.name; return f })()",
  "(() => { var f = function a(b) {}; delete f.length; return f })()",
  "(() => { var f = function a() {}; delete f.name; f.name = 'w'; return f })()",
  "(() => { 'use strict'; var f = function a() {}; try { f.name = 'w' } catch (e) {} return f })()",
  "(() => { var f = class A {}; delete f.name; return f })()",
  "(() => { var f = Math.max; var n = Object.getOwnPropertyDescriptor(f, 'name'); return Object.defineProperty(function () {}, 'name', n) })()",
  "Object.setPrototypeOf(function f() {}, null)",
  "Object.assign(function f() {}, { toString: 1 })",
  "Object.freeze(function f() {})",
];
for (const expr of renames) {
  addValue(expr);
  addValueViaString(expr);
}

// ---- Classes anônimas, construtores padrão e funções dentro de classes.
const classes = [
  "(class {})",
  "(class { })",
  "(class extends Object {})",
  "(class { constructor() {} })",
  "(class { constructor(a, b) {} })",
  "(class A { static m() {} })",
  "(() => { var C = class {}; return C })()",
  "(() => { var C = class extends Object {}; return C })()",
  "(() => { var o = { C: class {} }; return o.C })()",
  "(() => { var o = {}; o.C = class {}; return o.C })()",
  "(() => { class A {} class B extends A {} return B })()",
  "(() => { class A { constructor(x, y) {} } class B extends A {} return B })()",
  "(() => { class A { constructor(x, y) {} } class B extends A {} return Object.getPrototypeOf(B) })()",
  "(() => { class A { constructor(x, y) {} } class B extends A { constructor(...a) { super(...a) } } return B })()",
  "(() => { class A { m() {} } return new A().m })()",
  "(() => { class A { m() {} } return A.prototype.constructor })()",
  "(() => { class A { static x = function () {} } return A.x })()",
  "(() => { class A { x = () => 1 } return new A().x })()",
  "(() => { class A { static x = class {} } return A.x })()",
  "(() => { class A { static { A.y = function () {} } } return A.y })()",
  "(() => { class A { static { this.y = () => 1 } } return A.y })()",
  "(() => { class A { 'x y' = function () {} } return new A()['x y'] })()",
  "(() => { class A { [1 + 1] = class {} } return new A()[2] })()",
  "(() => { function F() {} return F.prototype.constructor })()",
  "(() => { function F() {} F.prototype.m = function () {}; return F.prototype.m })()",
  "(() => { function F() { this.m = function () {} } return new F().m })()",
  "(() => { function F() {} return Object.getPrototypeOf(F) })()",
  "(() => { var o = { __proto__: function () {} }; return Object.getPrototypeOf(o) })()",
  "(() => { var o = { __proto__: () => 1 }; return o.__proto__ })()",
];
for (const expr of classes) {
  addValue(expr);
  addValueViaString(expr);
}
programs.push(lines("class A { constructor() {} }; R = A.toString()"));
programs.push(lines("class A extends Object {}; R = A.toString()"));
programs.push(lines("class A { static x = 1 }; R = A + ''"));
programs.push(lines("R = `${class {}}|${class A {}}|${class extends Object {}}`"));
programs.push(lines("R = '' + (class Q { })"));
programs.push(lines("R = (function () {}) + ''"));
programs.push(lines("R = `${function a() {}}`"));
programs.push(lines("R = [function () {}, () => 1, class {}].join()"));
programs.push(lines("R = String([function a() {}, async () => 1])"));
programs.push(lines("R = JSON.stringify(function () {}) + '|' + JSON.stringify({ f() {} })"));
programs.push(lines("R = (function a() {}).constructor === Function && (async () => {}).constructor !== Function ? 'ok' : 'bad'"));
programs.push(lines("var s = Symbol('x'); R = Function.prototype.toString.call({ [s]() {} }[s])"));
programs.push(lines("var f = function () { return arguments.callee }; R = Function.prototype.toString.call(f())"));
programs.push(lines("R = (function () { return Function.prototype.toString.call(arguments.callee) })()"));
programs.push(lines("R = (function a() { return a.toString() })()"));
programs.push(lines("R = (() => 1).toString() === '() => 1' ? 'same' : 'diff'"));
programs.push(lines("var calls = 0; var f = function () { calls++ }; f.toString(); String(f); R = calls"));
programs.push(lines("R = Object.getOwnPropertyNames(Function.prototype).sort().join()"));

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "function-source-golden-"));
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, RESULT_PRELOAD);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const rows = [];
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  // Programa que lança sem `try` (ou que nem parseia) não grava `R`: o resultado dele é o erro (nome e mensagem),
  // capturado por um invólucro que só entra quando o programa puro não rodou. `captured` é o texto que o erro vira.
  const capture = "globalThis.R = e.name + ': ' + e.message";
  const wrappers = [
    text => text,
    text => `try {\n${text}\n} catch (e) { ${capture} }`,
    text => `try { (0, eval)(${JSON.stringify(text)}) } catch (e) { ${capture} }`,
  ];
  let wrapperIndex = 0;
  const original = () => wrappers[wrapperIndex](body.replace(/\bR = /g, "globalThis.R = "));
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  // Programa que só vale fora do modo estrito (`function static() {}`) o bun rejeita como módulo: tenta-se de novo como
  // CJS sloppy (`.cjs`), que é como ele o roda de fato.
  // Se nem isso o transpilador do bun parseia (`await` como identificador), o JSC recebe o texto por `vm`.
  const attempt = sloppy => {
    const text = original();
    const prepared = sloppy === "script" ? prepareScript(text) : prepareProgram(text, sloppy);
    const target = path.join(dir, "function_source_case" + prepared.file_extension);
    fs.writeFileSync(target, prepared.executable);
    const run = spawnSync(process.execPath, ["--preload", preload, target], { encoding: "utf8", cwd: dir });
    const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
    return { prepared, marked, result: marked ? JSON.parse(marked.slice(1)) : null };
  };
  const ran = t => t.marked && t.result !== "<undefined>";
  let tried;
  for (wrapperIndex = 0; wrapperIndex < wrappers.length; wrapperIndex++) {
    tried = attempt(false);
    if (!ran(tried)) tried = attempt(true);
    if (!ran(tried)) tried = attempt("script");
    if (ran(tried)) break;
  }
  const { source, meta } = tried.prepared;
  if (!tried.marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = tried.result.split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result === "<undefined>") {
    // Nem como módulo nem como CJS sloppy o programa rodou (SyntaxError): não mede nada.
    dropped++;
    process.stderr.write("programa não rodou: " + JSON.stringify(body) + "\n");
    continue;
  }
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("function_source", rows));
fs.rmSync(dir, { recursive: true, force: true });
