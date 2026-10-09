// Gera tests/golden/shadow_realm_bun.tsv: ShadowRealm (construtor, evaluate, importValue, funções remotas,
// isolamento de globais, erros) medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// O arquivo se chama `shadow_realm_case.js` dos dois lados; o programa roda em modo estrito e o processo só grava
// `R` na saída, depois de esvaziadas as microtarefas.
// Uso: bun scripts/gen-shadow-realm-golden.js > tests/golden/shadow_realm_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const PRE = "var r = new ShadowRealm();\nvar show = e => (e && e.constructor ? e.constructor.name : typeof e) + ': ' + (e && e.message);\n";

// Programa síncrono: grava o resultado de `expr` ou o erro lançado.
const SHOW = "var show = e => (e && e.constructor ? e.constructor.name : typeof e) + ': ' + (e && e.message);\n";
const tryExpr = (expr, pre = PRE) =>
  (pre === "" ? SHOW : "") + pre + `try { globalThis.R = String(${expr}) } catch (e) { globalThis.R = show(e) }`;
// Mesmo, mas descreve o valor com typeof e o resultado de String.
const describe = (expr, pre = PRE) =>
  (pre === "" ? SHOW : "") + pre + `try { var v = ${expr}; globalThis.R = typeof v + ':' + String(v) } catch (e) { globalThis.R = show(e) }`;

// ---- Construtor.
for (const c of [
  "ShadowRealm()",
  "ShadowRealm.call({})",
  "Reflect.apply(ShadowRealm, undefined, [])",
  "new ShadowRealm(1)",
  "new ShadowRealm({})",
  "new ShadowRealm(undefined, 2)",
  "Reflect.construct(ShadowRealm, [], Object)",
  "Reflect.construct(ShadowRealm, [], Function)",
  "Reflect.construct(ShadowRealm, [], function () {})",
  "Reflect.construct(ShadowRealm, [], class A {})",
  "Reflect.construct(ShadowRealm, [], Array)",
  "Reflect.construct(ShadowRealm, [], ShadowRealm)",
  "Reflect.construct(ShadowRealm, [], () => {})",
  "class S extends ShadowRealm {}; new S()",
  "class S extends ShadowRealm { constructor() { super(); this.x = 1 } }; new S().x",
  "class S extends ShadowRealm {}; new S() instanceof ShadowRealm",
  "class S extends ShadowRealm {}; Object.getPrototypeOf(new S()) === S.prototype",
  "class S extends ShadowRealm {}; new S().evaluate('1+2')",
  "Object.getPrototypeOf(new ShadowRealm()) === ShadowRealm.prototype",
  "new ShadowRealm() instanceof ShadowRealm",
  "new ShadowRealm() instanceof Object",
  "typeof new ShadowRealm()",
  "typeof ShadowRealm",
  "ShadowRealm.length",
  "ShadowRealm.name",
  "ShadowRealm.prototype.constructor === ShadowRealm",
  "Object.getPrototypeOf(ShadowRealm) === Function.prototype",
  "Object.getOwnPropertyNames(ShadowRealm).sort().join()",
  "Object.getOwnPropertyNames(ShadowRealm.prototype).sort().join()",
  "Object.getOwnPropertySymbols(ShadowRealm.prototype).map(String).join()",
  "JSON.stringify(Object.getOwnPropertyDescriptor(ShadowRealm, 'prototype'))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(ShadowRealm.prototype, 'evaluate'))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(ShadowRealm.prototype, 'importValue'))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(ShadowRealm.prototype, 'constructor'))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(ShadowRealm.prototype, Symbol.toStringTag))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'ShadowRealm'))",
  "ShadowRealm.prototype.evaluate.length",
  "ShadowRealm.prototype.evaluate.name",
  "ShadowRealm.prototype.importValue.length",
  "ShadowRealm.prototype.importValue.name",
  "Object.prototype.toString.call(new ShadowRealm())",
  "Object.prototype.toString.call(ShadowRealm.prototype)",
  "String(new ShadowRealm())",
  "ShadowRealm.prototype[Symbol.toStringTag]",
  "new ShadowRealm()[Symbol.toStringTag]",
  "Object.keys(new ShadowRealm()).length",
  "Object.isExtensible(new ShadowRealm())",
  "Reflect.ownKeys(new ShadowRealm()).length",
  "ShadowRealm.prototype.toString === Object.prototype.toString",
  "Function.prototype.toString.call(ShadowRealm).includes('native code')",
  "Function.prototype.toString.call(ShadowRealm.prototype.evaluate).includes('native code')",
]) {
  programs.push(tryExpr(`(() => { ${/;/.test(c) ? c.replace(/; ([^;]+)$/, "; return $1") : "return " + c} })()`, ""));
}

// ---- Receptor inválido nos métodos.
for (const method of ["evaluate", "importValue"]) {
  for (const recv of ["undefined", "null", "1", "'s'", "{}", "[]", "Object.create(ShadowRealm.prototype)", "ShadowRealm.prototype", "function () {}", "Symbol()", "new Proxy(new ShadowRealm(), {})"]) {
    const args = method === "evaluate" ? "'1'" : "'./x.js', 'a'";
    programs.push(tryExpr(`ShadowRealm.prototype.${method}.call(${recv}, ${args})`, ""));
  }
}

// ---- evaluate: tipos do argumento.
for (const arg of ["", "undefined", "null", "1", "true", "Symbol()", "{}", "[]", "function () {}", "1n", "new String('1')", "{ toString() { return '1' } }", "`1`", "'1'", "NaN", "-0"]) {
  programs.push(tryExpr(`r.evaluate(${arg})`));
}

// ---- evaluate: retornos primitivos.
for (const src of [
  "1+1", "1", "-0", "0", "NaN", "Infinity", "-Infinity", "1.5", "1e21", "0.1+0.2", "2**53", "'a'", "'é'", "''", "'a' + 'b'",
  "true", "false", "null", "undefined", "void 0", "1n", "2n**64n", "-5n", "Symbol.iterator", "Symbol('x')", "Symbol.for('k')",
  "typeof 1", "typeof null", "typeof undefined", "typeof Symbol()", "typeof (() => 1)", "typeof {}", "typeof 1n",
  "`t${1+1}`", "[1,2].length", "[1,2].join('-')", "({a: 1}).a", "'abc'.toUpperCase()", "parseInt('12px')",
  "", " ", "\n", "// só comentário", "/* c */", "1;", "1;2;3", "var x = 1", "var x = 1; x", "let y = 2; y", "const z = 3; z", "x = 5", "if (true) { 7 } else { 8 }",
  "for (var i = 0; i < 3; i++) { i }", "do { 9 } while (false)", "'use strict'; 4", "this === undefined", "typeof this", "this === globalThis",
  "typeof globalThis", "typeof window", "typeof self", "typeof process", "typeof require", "typeof console", "typeof Bun", "typeof setTimeout",
  "typeof queueMicrotask", "typeof ShadowRealm", "typeof WebAssembly", "typeof Intl", "typeof JSON", "typeof Math", "typeof Reflect", "typeof Proxy",
  "typeof Atomics", "typeof SharedArrayBuffer", "typeof globalThis.eval", "typeof fetch", "typeof structuredClone", "typeof TextEncoder", "typeof URL",
  "typeof Temporal", "typeof escape", "typeof WeakRef", "typeof FinalizationRegistry", "typeof AggregateError", "typeof Iterator",
  "new Date(0).getTime()", "Math.max(1, 2)", "JSON.stringify({a:[1]})", "String(Object.prototype)", "Object.prototype.toString.call(null)",
  "Object.getPrototypeOf(globalThis) === Object.prototype", "Object.getOwnPropertyNames(globalThis).includes('ShadowRealm')",
  "Object.getOwnPropertyNames(globalThis).includes('Bun')", "Object.getOwnPropertyNames(globalThis).includes('console')",
  "Object.getOwnPropertyNames(globalThis).includes('eval')", "Object.getOwnPropertyNames(globalThis).includes('Array')",
  "Object.getOwnPropertyNames(globalThis).includes('process')",
  "String(globalThis)", "globalThis.toString()", "Function('return typeof this')()", "(0, eval)('1+1')", "eval('2+3')", "new Function('return 6')()",
  "(function () { return typeof this })()", "(function () { 'use strict'; return typeof this })()", "(() => typeof this)()",
  "async function f() {}; typeof f", "Symbol.toStringTag in globalThis", "Object.prototype.toString.call(globalThis)",
  "Number.MAX_SAFE_INTEGER", "'\\u{1F600}'.length", "'x'.repeat(3)", "[..."+"'ab'"+"].length", "new Intl.NumberFormat('en').format(1234.5)",
]) {
  programs.push(tryExpr(`r.evaluate(${JSON.stringify(src)})`));
  if (src.length < 8) programs.push(describe(`r.evaluate(${JSON.stringify(src)})`));
}

// ---- evaluate: valores que não passam (objetos), funções e símbolos.
for (const src of [
  "({})", "[]", "[1,2]", "new Date(0)", "/x/", "new Map", "new Set", "new Error('e')", "Promise.resolve(1)", "new Proxy({}, {})", "Object.create(null)",
  "new Uint8Array(1)", "new ArrayBuffer(1)", "new String('s')", "new Number(1)", "new Boolean(true)", "Object(1n)", "Object(Symbol())", "globalThis", "Math", "JSON",
  "Reflect", "Array", "Object", "Object.prototype", "Function.prototype", "(class A {})", "(class A { static x = 1 })", "async function f() {}; f",
  "(function* g() {})", "(async function* ag() {})", "(async () => 1)", "new WeakMap", "new WeakRef({})", "Symbol.prototype", "arguments", "(function () { return arguments })()",
  "new Proxy(function () {}, {})", "new Proxy(function () {}, { apply() { return 1 } })", "Math.max", "parseInt", "Array.prototype.map", "Object.keys", "Function.prototype",
  "Array.prototype", "Date", "Symbol", "BigInt", "Proxy", "Reflect.apply", "console", "ShadowRealm", "eval", "new Function", "(function () {}).bind()",
  "Array.prototype[Symbol.iterator]", "function* g() {}; g()", "[][Symbol.iterator]()", "new Intl.Locale('en')", "Intl", "Atomics", "WebAssembly",
]) {
  programs.push(tryExpr(`r.evaluate(${JSON.stringify(src)})`));
}

// ---- evaluate: funções viram wrapped functions (inspeção).
const fnSources = [
  "(function () {})", "(function named(a, b) {})", "(() => 1)", "(a, b, c) => a", "(function (a, b = 1, c) {})", "(function (...args) {})", "(function ({ a }, [b]) {})",
  "(class A { constructor(a) {} })", "(class {})", "(class A { static s() {} })", "(async function af(a) {})", "(function* gf(a, b) {})", "(async () => {})", "(async function* () {})",
  "Math.max", "parseInt", "Array.prototype.map", "Object.keys", "(function () {}).bind(null)", "(function f(a, b, c) {}).bind(null, 1)", "Symbol",
  "({ m() {} }).m", "({ get x() { return 1 } }).__lookupGetter__('x')", "(function () {}).bind(null).bind(null)", "Function.prototype", "Array", "Object", "Date", "Proxy",
  "new Proxy(function f(a) {}, {})", "(() => {})", "function () {}; (function z() {})",
  "(function () { 'use strict' })", "(function () {}).call", "String", "Reflect.get", "eval", "(0, eval)",
];
const fnProbes = [
  ["typeof", "typeof f"],
  ["name", "f.name"],
  ["length", "f.length"],
  ["toString", "Function.prototype.toString.call(f)"],
  ["hasPrototype", "'prototype' in f"],
  ["ownProps", "Reflect.ownKeys(f).map(String).join()"],
  ["nameDesc", "JSON.stringify(Object.getOwnPropertyDescriptor(f, 'name'))"],
  ["lengthDesc", "JSON.stringify(Object.getOwnPropertyDescriptor(f, 'length'))"],
  ["proto", "Object.getPrototypeOf(f) === Function.prototype"],
  ["tag", "Object.prototype.toString.call(f)"],
  ["isExt", "Object.isExtensible(f)"],
];
for (const src of fnSources) {
  for (const [, probe] of fnProbes.slice(0, 4)) {
    programs.push(tryExpr(probe, PRE + `var f = r.evaluate(${JSON.stringify(src)});\n`));
  }
}

// ---- Wrapped functions: chamada, this, argumentos, retornos.
const callTargets = [
  ["id", "(function (x) { return x })"],
  ["arrow", "(x => x)"],
  ["nothing", "(function () {})"],
  ["this", "(function () { return typeof this })"],
  ["thisStrict", "(function () { 'use strict'; return typeof this })"],
  ["thisIsGlobal", "(function () { return this === globalThis })"],
  ["argc", "(function () { return arguments.length })"],
  ["sum", "(function (a, b) { return a + b })"],
  ["retObj", "(function () { return {} })"],
  ["retFn", "(function () { return function inner(a, b) { return a } })"],
  ["retArrow", "(function () { return () => 42 })"],
  ["retFn2", "(function () { return function () { return function () { return 'deep' } } })"],
  ["retSym", "(function () { return Symbol('s') })"],
  ["retBig", "(function () { return 10n ** 20n })"],
  ["throwStr", "(function () { throw 'boom' })"],
  ["throwNum", "(function () { throw 42 })"],
  ["throwErr", "(function () { throw new Error('inside') })"],
  ["throwTypeErr", "(function () { throw new TypeError('te') })"],
  ["throwRangeErr", "(function () { throw new RangeError('re') })"],
  ["throwObj", "(function () { throw { a: 1 } })"],
  ["throwFn", "(function () { throw function () {} })"],
  ["throwUndef", "(function () { throw undefined })"],
  ["throwNull", "(function () { throw null })"],
  ["throwSym", "(function () { throw Symbol('q') })"],
  ["throwCustom", "(function () { class MyErr extends Error {} throw new MyErr('custom') })"],
  ["throwNoMsg", "(function () { throw new Error() })"],
  ["refErr", "(function () { return nope })"],
  ["typeErr", "(function () { return undefined.x })"],
  ["counter", "(function () { globalThis.n = (globalThis.n || 0) + 1; return globalThis.n })"],
  ["newTarget", "(function () { return new.target === undefined })"],
  ["ctor", "(class A { constructor() { this.x = 1 } })"],
  ["max", "Math.max"],
  ["toUpper", "(s => s.toUpperCase())"],
  ["async", "(async function () { return 1 })"],
  ["gen", "(function* () { yield 1 })"],
  ["thisProp", "(function () { return this.x })"],
];
const argLists = ["", "1", "1, 2", "{}"];
for (const [, target] of callTargets) {
  for (const args of argLists) {
    programs.push(describe(`f(${args})`, PRE + `var f = r.evaluate(${JSON.stringify(target)});\n`));
  }
}
// this passado pela chamada
for (const thisv of ["undefined", "null", "1", "'s'", "{}", "[]", "function () {}", "globalThis", "Symbol()", "{ x: 7 }"]) {
  programs.push(describe(`f.call(${thisv})`, PRE + `var f = r.evaluate("(function () { return typeof this })");\n`));
  programs.push(describe(`f.call(${thisv})`, PRE + `var f = r.evaluate("(function () { return this })");\n`));
  programs.push(describe(`f.call(${thisv}, 1)`, PRE + `var f = r.evaluate("(function () { return this === undefined })");\n`));
}
for (const how of ["f.apply(null, [1, 2])", "f.apply(null, [{}])", "f.bind(null, 3)(4)", "f.bind(null, {})(4)", "Reflect.apply(f, undefined, [1, 1])", "new f(1, 2)", "new f()", "new (f.bind(null))()", "Reflect.construct(f, [1])", "[1, 2].map(f)", "[3].reduce(f, 1)", "f(...[1, 2])"]) {
  programs.push(describe(how, PRE + `var f = r.evaluate("(function (a, b) { return a + b })");\n`));
  programs.push(describe(how, PRE + `var f = r.evaluate("(class A { constructor(a, b) { this.a = a } })");\n`));
  programs.push(describe(how, PRE + `var f = r.evaluate("(x => x)");\n`));
}

// ---- Wrapped functions: identidade, tipo, protótipo, instanceof, toStringTag, propriedades.
const wf = PRE + `var f = r.evaluate("(function foo(a, b) { return a })");\n`;
for (const e of [
  "f === r.evaluate('(function foo(a, b) { return a })')", "f instanceof Function", "f instanceof Object", "f instanceof ShadowRealm", "Function.prototype.isPrototypeOf(f)",
  "typeof f", "typeof f.call", "f.call === Function.prototype.call", "f.bind === Function.prototype.bind", "f.hasOwnProperty('prototype')", "f.prototype", "'prototype' in f",
  "Object.getOwnPropertyNames(f).join()", "Object.getOwnPropertyNames(f).sort().join()", "Reflect.ownKeys(f).length",
  "f.name", "f.length", "f.toString()", "String(f)", "f + ''", "Object.prototype.toString.call(f)", "f[Symbol.toStringTag]", "Symbol.toStringTag in f",
  "Object.getOwnPropertyDescriptor(f, 'name').configurable", "Object.getOwnPropertyDescriptor(f, 'name').writable", "Object.getOwnPropertyDescriptor(f, 'name').enumerable",
  "Object.getOwnPropertyDescriptor(f, 'length').configurable", "Object.getOwnPropertyDescriptor(f, 'length').writable",
  "Object.getPrototypeOf(f) === Function.prototype", "Object.getPrototypeOf(f) === r.evaluate('Function.prototype')", "f.constructor === Function", "f.constructor.name",
  "Object.isFrozen(f)", "Object.isSealed(f)", "Object.isExtensible(f)", "Object.keys(f).length", "JSON.stringify(f)", "JSON.stringify({ f })", "f.caller", "f.arguments",
  "f.x = 1; f.x", "f.x = 1; Object.keys(f).join()", "delete f.name", "delete f.length", "(f.name = 'z', f.name)", "(f.length = 9, f.length)",
  "Object.defineProperty(f, 'k', { value: 1 }).k", "Object.setPrototypeOf(f, null) === f", "Object.preventExtensions(f) === f", "f.hasOwnProperty('name')", "f.hasOwnProperty('length')",
  "Reflect.getPrototypeOf(f) === Function.prototype", "Reflect.has(f, 'call')", "Reflect.ownKeys(f).map(String).join()", "Function.prototype.toString.call(f).includes('native code')",
  "Function.prototype.toString.call(f)", "new f() instanceof f", "Reflect.construct(f, [], Object) instanceof Object", "Object.getPrototypeOf(new f())", "f.bind(null).name", "f.bind(null).length",
  "f.bind(null, 1).length", "f.call.length", "(() => { class A extends f {} })()", "(() => { class A extends f {}; return new A() instanceof f })()",
  "Object.getOwnPropertyNames(Object.getPrototypeOf(f)).includes('call')", "f.hasOwnProperty('call')", "Object.entries(f).length", "Array.isArray(f)", "f == f", "f === f", "Object.is(f, f)",
  "new Proxy(f, {})()", "new Proxy(f, {}).name", "typeof new Proxy(f, {})", "String(Symbol.hasInstance in f)", "1 instanceof f", "({}) instanceof f", "Function.prototype[Symbol.hasInstance].call(f, 1)",
]) {
  programs.push(tryExpr(e, wf));
}
// Funções com nome e comprimento variados.
for (const [src, probe] of [
  ["(function () {})", "f.name === '' ? '<empty>' : f.name"],
  ["(function foo(a, b, c) {})", "f.name + f.length"],
  ["(function (a, b = 1, c) {})", "f.length"],
  ["(function (...a) {})", "f.length"],
  ["(class Foo { })", "f.name"],
  ["Object.defineProperty(function () {}, 'name', { value: 'custom' })", "f.name"],
  ["Object.defineProperty(function () {}, 'length', { value: 7 })", "f.length"],
  ["Object.defineProperty(function () {}, 'name', { value: 5 })", "typeof f.name + String(f.name)"],
  ["Object.defineProperty(function () {}, 'length', { value: -1 })", "f.length"],
  ["Object.defineProperty(function () {}, 'length', { value: Infinity })", "f.length"],
  ["Object.defineProperty(function () {}, 'length', { value: 2.7 })", "f.length"],
  ["Object.defineProperty(function () {}, 'length', { value: '3' })", "f.length"],
  ["Object.defineProperty(function () {}, 'length', { value: 2 ** 40 })", "f.length"],
  ["Object.defineProperty(function () {}, 'name', { get() { throw new Error('getter') } })", "f.name"],
  ["Object.defineProperty(function () {}, 'length', { get() { throw new Error('getter') } })", "f.length"],
  ["(function () { var g = function () {}; delete g.name; return g })()", "JSON.stringify(f.name)"],
  ["(function () { var g = function () {}; delete g.length; return g })()", "f.length"],
  ["(function () { var g = function () {}; delete g.name; delete g.length; return g })()", "f.length + ':' + f.name"],
  ["Object.defineProperty(function a() {}, 'name', { value: Symbol('s') })", "typeof f.name"],
  ["(function () { var g = function () {}; Object.defineProperty(g, 'name', { value: 'x' }); Object.setPrototypeOf(g, null); return g })()", "f.name"],
  ["new Proxy(function foo(a) {}, {})", "f.name + f.length"],
  ["new Proxy(function foo(a) {}, { get() { return 'p' } })", "f.name"],
  ["new Proxy(function foo(a) {}, { get() { throw new Error('trap') } })", "f.name"],
  ["new Proxy(function foo(a) {}, { getOwnPropertyDescriptor() { throw new Error('trapd') } })", "f.name"],
  ["(function () { var g = function foo() {}; return g.bind(null) })()", "f.name"],
  ["(function () { var g = function () {}; g.bind = 1; return g })()", "typeof f.bind"],
  ["(function () { var g = Object.create(Function.prototype); return g })()", "typeof g"],
  ["Function.prototype", "f.name === '' && f.length === 0"],
]) {
  programs.push(tryExpr(probe, PRE + `var f = r.evaluate(${JSON.stringify(src)});\n`));
}

// ---- Funções do chamador passadas ao realm (chamada de volta, argumentos primitivos vs objetos).
for (const [rsrc, args] of [
  ["(function (cb) { return typeof cb })", "() => 1"],
  ["(function (cb) { return cb(1, 2) })", "(a, b) => a + b"],
  ["(function (cb) { return cb() })", "() => ({})"],
  ["(function (cb) { return cb() })", "() => { throw new Error('caller') }"],
  ["(function (cb) { return cb() })", "() => { throw 'str' }"],
  ["(function (cb) { return cb({}) })", "o => o"],
  ["(function (cb) { return cb(function () {}) })", "g => typeof g"],
  ["(function (cb) { return cb.name })", "function outer() {}"],
  ["(function (cb) { return cb.length })", "function outer(a, b) {}"],
  ["(function (cb) { return cb instanceof Function })", "function outer() {}"],
  ["(function (cb) { return Object.getPrototypeOf(cb) === Function.prototype })", "function outer() {}"],
  ["(function (cb) { return Object.getPrototypeOf(cb) === Function.prototype })", "() => 1"],
  ["(function (cb) { return cb === cb })", "() => 1"],
  ["(function (cb) { return 'prototype' in cb })", "function outer() {}"],
  ["(function (cb) { return cb.toString() })", "function outer() {}"],
  ["(function (cb) { return Object.prototype.toString.call(cb) })", "() => 1"],
  ["(function (cb) { return new cb() })", "function Outer() {}"],
  ["(function (cb) { return cb.call(undefined, 1) })", "x => x"],
  ["(function (cb) { return cb.bind(null, 1)() })", "x => x"],
  ["(function (cb) { return [1, 2].map(cb).join() })", "x => x * 2"],
  ["(function (cb) { return cb(undefined) })", "x => typeof x"],
  ["(function (cb) { return cb(null) })", "x => typeof x"],
  ["(function (cb) { return cb(1n) })", "x => typeof x"],
  ["(function (cb) { return cb(Symbol('a')) })", "x => typeof x"],
  ["(function (cb) { return cb(this) })", "x => typeof x"],
  ["(function (cb) { return cb([]) })", "x => typeof x"],
  ["(function (cb, v) { return cb(v) })", "x => x, 5"],
  ["(function (cb) { return cb(() => 1)() })", "f => f"],
  ["(function (cb) { return cb(() => 1)() })", "f => typeof f"],
  ["(function (cb) { try { cb() } catch (e) { return e.constructor === TypeError } })", "() => { throw new RangeError('x') }"],
  ["(function (cb) { try { cb() } catch (e) { return e.message } })", "() => { throw new RangeError('x') }"],
  ["(function (cb) { try { cb() } catch (e) { return e.message } })", "() => { throw {} }"],
  ["(function (cb) { try { cb() } catch (e) { return e.message } })", "() => { throw 'prim' }"],
  ["(function (cb) { try { cb() } catch (e) { return String(e) } })", "() => { throw 7 }"],
  ["(function (cb) { try { cb({}) } catch (e) { return e.message } })", "o => o"],
  ["(function (cb) { try { return cb() } catch (e) { return e.message } })", "() => ({})"],
]) {
  programs.push(describe(`f(${args})`, PRE + `var f = r.evaluate(${JSON.stringify(rsrc)});\n`));
}

// ---- Erros lançados dentro do realm viram TypeError no chamador.
for (const body of [
  "throw 1", "throw 'x'", "throw null", "throw undefined", "throw true", "throw 1n", "throw Symbol('s')", "throw {}", "throw []", "throw { message: 'm' }", "throw function () {}",
  "throw new Error('e1')", "throw new TypeError('e2')", "throw new RangeError('e3')", "throw new SyntaxError('e4')", "throw new ReferenceError('e5')", "throw new EvalError('e6')", "throw new URIError('e7')",
  "throw new AggregateError([], 'e8')", "throw new Error('')", "throw new Error()", "throw Object.assign(new Error('x'), { name: 'Custom' })", "throw Object.create(Error.prototype)",
  "throw { toString() { return 'ts' } }", "throw new (class MyErr extends Error {})('mine')", "undefined()", "null.x", "x.y.z", "nope", "let a = 1; let a = 2", "1 +", "}", "new (void 0)",
  "Symbol() + ''", "BigInt(1.5)", "new Array(-1)", "'a'.repeat(-1)", "decodeURIComponent('%')", "JSON.parse('{')", "JSON.parse('')", "new Proxy({}, null)", "Object.defineProperty(1, 'x', {})",
  "(function f() { f() })()", "Reflect.ownKeys(1)", "new Intl.NumberFormat('xx-invalid-')", "structuredClone(() => {})", "class A extends null { constructor() { super() } }; new A", "new Map([1])",
  "[].reduce((a, b) => a)", "new Uint8Array(-1)", "null[Symbol.iterator]", "for (const x of 1) {}", "const [a] = null", "const { b } = undefined", "await 1", "yield 1", "return 1", "break", "continue", "new.target",
  "super.x", "import.meta", "import('./nonexistent-module.js')", "throw new Error('multi\\nline')", "throw new Error('é ünïcode')", "throw Object.create(null)",
  "throw new Proxy({}, {})", "throw new Proxy(function () {}, {})", "throw new String('boxed')", "throw new Number(5)", "throw new Date(0)", "throw /re/", "throw new Map",
  "Promise.reject(1)", "(async () => { throw 1 })()",
]) {
  programs.push(tryExpr(`r.evaluate(${JSON.stringify(body)})`));
  programs.push(describe(`(() => { try { r.evaluate(${JSON.stringify(body)}); return 'no error' } catch (e) { return [e instanceof TypeError, e.constructor === TypeError, e.name, Object.getPrototypeOf(e) === TypeError.prototype, e.message].join('|') } })()`));
}
// O TypeError do evaluate pertence ao realm chamador (mesma identidade que o TypeError de cima).
programs.push(tryExpr("(() => { try { r.evaluate('throw new Error(1)') } catch (e) { return e.constructor === TypeError && Object.getPrototypeOf(e) === TypeError.prototype } })()"));
programs.push(tryExpr("(() => { try { r.evaluate('throw 1') } catch (e) { return Object.getOwnPropertyNames(e).sort().join() } })()"));
programs.push(tryExpr("(() => { try { r.evaluate('throw 1') } catch (e) { return typeof e.stack } })()"));
programs.push(tryExpr("(() => { try { r.evaluate('throw 1') } catch (e) { return e.cause === undefined } })()"));
programs.push(tryExpr("(() => { try { r.evaluate('1 +') } catch (e) { return e.constructor === SyntaxError } })()"));
programs.push(tryExpr("(() => { try { r.evaluate('1 +') } catch (e) { return e instanceof SyntaxError } })()"));

// ---- SyntaxError no evaluate: cria o SyntaxError do realm chamador.
for (const body of [
  "1 +", "}", "{", "(", "var", "let let = 1", "1 = 2", "a b", "'unterminated", "`unterminated", "/unterminated", "/*", "function () {}", "class {", "if", "if (", "for (;;", "return", "return 1",
  "yield", "await", "break", "continue", "new.target", "super()", "super.x", "import x from 'y'", "export default 1", "export {}", "import.meta", "#x in {}", "@dec class A {}", "0b2", "08n", "1__0",
  "'\\u{110000}'", "'\\xZZ'", "/(/", "/[/", "/a/zz", "let x; let x", "const y", "var a; let a", "'use strict'; with (a) {}", "'use strict'; 010", "'use strict'; delete x", "'use strict'; var eval", "'use strict'; arguments = 1",
  "function f(a, a) { 'use strict' }", "({ a: 1, __proto__: 1, __proto__: 2 })", "async () => await", "label: label: 1", "a?.b = 1", "a ?? b || c", "x => { 'use strict'; with (a) {} }", "for (let of of []) {}", "while", "do", "try {}", "catch (e) {}", "else", "case 1:", "default:", "<!--", "-->",
  "1;\n+", "\n\n}", "var await", "enum", "class A { constructor() {} constructor() {} }", "class A extends B { constructor() { super.x; super() } }; }", "({ get a(x) {} })", "({ set a() {} })", "new import('x')", "1++", "++1", "typeof", "void", "delete", "in", "instanceof",
]) {
  programs.push(describe(`(() => { try { r.evaluate(${JSON.stringify(body)}); return 'ok' } catch (e) { return [e.constructor === SyntaxError, e.name, e.constructor.name, e.message].join('|') } })()`));
}

// ---- Modo estrito e eval indireto dentro do realm.
for (const src of [
  "'use strict'; typeof this", "'use strict'; this === undefined", "(function () { return this === undefined })()", "(function () { return this === globalThis })()",
  "var sloppyVar = 1; typeof sloppyVar", "sloppyUndeclared = 5; typeof sloppyUndeclared", "undeclaredVar2 = 6; globalThis.undeclaredVar2", "'use strict'; undeclaredStrict = 1",
  "'use strict'; var s = 1; delete globalThis.s", "(function () { 'use strict'; return this })()", "(function () { return arguments.callee === undefined })", "(function () { 'use strict'; try { return arguments.callee } catch (e) { return e.constructor.name } })()",
  "(0, eval)('var ie = 1'); typeof ie", "(0, eval)('var ie2 = 2'); globalThis.ie2", "(0, eval)('1 + 1')", "(0, eval)('this === globalThis')", "(0, eval)('typeof this')", "(0, eval)('(function () { return this === globalThis })()')",
  "(0, eval)('\\'use strict\\'; var ie3 = 1'); typeof ie3", "eval('var de = 1'); typeof de", "'use strict'; eval('var de2 = 1'); typeof de2", "var geval = eval; geval('var ge = 3'); ge", "var geval2 = eval; geval2('let le = 4'); typeof le",
  "(0, eval)('let leb = 1'); typeof leb", "(0, eval)('const ceb = 1; ceb')", "(0, eval)('1 +')", "(0, eval)('throw 1')", "(0, eval)('throw new Error(\\'ie\\')')", "(0, eval)('({})')", "(0, eval)('(() => 1)')",
  "typeof (0, eval)('(() => 1)')", "(0, eval)('globalThis') === globalThis", "(0, eval)('Array') === Array", "Function('return this')() === globalThis", "Function('return typeof ShadowRealm')()",
  "Function('a', 'b', 'return a + b')(1, 2)", "new Function('return Array')() === Array", "Function('1 +')", "Function('return Object.getPrototypeOf(this) === Object.prototype')()",
  "var f = new Function('return 1'); typeof f", "var f = Function('x', 'return x * 2'); f(4)", "(function () { return eval('this') === globalThis })()", "(function () { return (0, eval)('this') === globalThis })()",
  "(function () { var local = 1; return eval('local') })()", "(function () { var local = 1; return (0, eval)('typeof local') })()", "(function () { var local = 1; return (0, eval)('local') })()",
  "(function () { return typeof eval })()", "eval.length", "eval.name", "typeof eval", "eval === (0, eval)", "eval(1)", "eval()", "eval('')", "eval(undefined)", "eval({})", "eval(null)",
  "let tdz1 = 1; (0, eval)('typeof tdz1')", "var order = []; order.push(1); (0, eval)('order = [9]'); order.length",
]) {
  programs.push(tryExpr(`r.evaluate(${JSON.stringify(src)})`));
}

// ---- Isolamento de globais.
const isolation = [
  // var global do realm não vaza
  [`r.evaluate("var leak = 1"); typeof leak`, ""],
  [`r.evaluate("var leak = 1"); typeof globalThis.leak`, ""],
  [`r.evaluate("leak2 = 1"); typeof leak2`, ""],
  [`r.evaluate("globalThis.leak3 = 1"); typeof leak3`, ""],
  [`r.evaluate("function leakFn() {}"); typeof leakFn`, ""],
  [`r.evaluate("let leakLet = 1"); typeof leakLet`, ""],
  [`r.evaluate("class LeakClass {}"); typeof LeakClass`, ""],
  [`r.evaluate("Array.leaked = 1"); Array.leaked`, ""],
  [`r.evaluate("Array.prototype.leaked = 1"); [].leaked`, ""],
  [`r.evaluate("Object.prototype.leaked = 1"); ({}).leaked`, ""],
  [`r.evaluate("Object.prototype.leaked = 1"); r.evaluate("({}).leaked") + '|' + ({}).leaked`, ""],
  [`r.evaluate("Function.prototype.leaked = 1"); (() => {}).leaked`, ""],
  [`r.evaluate("String.prototype.leaked = 1"); 'a'.leaked`, ""],
  [`r.evaluate("Number.prototype.leaked = 1"); (1).leaked`, ""],
  [`r.evaluate("Math.leaked = 1"); Math.leaked`, ""],
  [`r.evaluate("delete globalThis.Array"); typeof Array`, ""],
  [`r.evaluate("globalThis.Array = 1"); typeof Array`, ""],
  [`r.evaluate("globalThis.Array = 1"); r.evaluate("typeof Array")`, ""],
  [`r.evaluate("delete globalThis.Array"); r.evaluate("typeof Array")`, ""],
  [`globalThis.callerVar = 1; r.evaluate("typeof callerVar")`, ""],
  [`var callerVar2 = 1; r.evaluate("typeof callerVar2")`, ""],
  [`globalThis.callerVar3 = 1; r.evaluate("typeof globalThis.callerVar3")`, ""],
  [`Array.prototype.callerLeak = 1; r.evaluate("[].callerLeak")`, ""],
  [`Object.prototype.callerLeak = 1; r.evaluate("({}).callerLeak")`, ""],
  [`r.evaluate("var a = 1; a"); r.evaluate("typeof a")`, ""],
  [`r.evaluate("var a = 1"); r.evaluate("a")`, ""],
  [`r.evaluate("let b = 1"); r.evaluate("b")`, ""],
  [`r.evaluate("let b = 1"); r.evaluate("let b = 2")`, ""],
  [`r.evaluate("const c = 1"); r.evaluate("c + 1")`, ""],
  [`r.evaluate("var d = 1"); r.evaluate("var d = 2; d")`, ""],
  [`r.evaluate("var e1 = 1"); new ShadowRealm().evaluate("typeof e1")`, ""],
  [`var r2 = new ShadowRealm(); r.evaluate("var sh = 1"); r2.evaluate("typeof sh")`, ""],
  [`var r2 = new ShadowRealm(); r.evaluate("Array.prototype.x = 1"); r2.evaluate("[].x")`, ""],
  [`var r2 = new ShadowRealm(); r.evaluate("Array") === r2.evaluate("Array")`, ""],
  [`var r2 = new ShadowRealm(); r.evaluate("Array") === r2.evaluate("Array")`, ""],
  [`r.evaluate("this") === globalThis`, ""],
  // Protótipos distintos
  [`var A = r.evaluate("Array"); A === Array`, ""],
  [`var A = r.evaluate("Array"); A === Array`, ""],
  [`var f = r.evaluate("(function () { return [] })"); typeof f()`, ""],
  [`var f = r.evaluate("(function (a) { return a instanceof Array })"); f([])`, ""],
  [`var f = r.evaluate("(function (a) { return Array.isArray(a) })"); f([])`, ""],
  [`var f = r.evaluate("(function (a) { return a instanceof Array })"); f({})`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a })"); f([])`, ""],
  [`var f = r.evaluate("(function () { return Array.prototype })"); f()`, ""],
  [`r.evaluate("Array.prototype === Array.prototype")`, ""],
  [`r.evaluate("[]")`, ""],
  [`r.evaluate("[] instanceof Array")`, ""],
  [`r.evaluate("(function () {}) instanceof Function")`, ""],
  [`r.evaluate("Object.getPrototypeOf(function () {}) === Function.prototype")`, ""],
  [`r.evaluate("(() => {}) instanceof Object")`, ""],
  [`var f = r.evaluate("(function () {})"); f instanceof r.evaluate("Function")`, ""],
  [`var F = r.evaluate("Function"); F`, ""],
  [`var f = r.evaluate("(function () {})"); f instanceof Function`, ""],
  [`var f = r.evaluate("(function () {})"); Object.getPrototypeOf(f) === Function.prototype`, ""],
  [`var f = r.evaluate("(function () { return Object.getPrototypeOf(function () {}) === Function.prototype })"); f()`, ""],
  [`var f = r.evaluate("(function () { return Function.prototype })"); f()`, ""],
  [`var f = r.evaluate("(function () { return Function })"); f()`, ""],
  [`var f = r.evaluate("(function (g) { return g instanceof Function })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (g) { return Object.getPrototypeOf(g) === Function.prototype })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (g) { return g.constructor === Function })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (g) { return g.constructor === Function })"); f(function () {})`, ""],
  [`var f = r.evaluate("(function () { return Error })"); f()`, ""],
  [`var f = r.evaluate("(function () { return new Error('x') })"); f()`, ""],
  [`var f = r.evaluate("(function () { try { null.x } catch (e) { return e } })"); f()`, ""],
  [`var f = r.evaluate("(function () { try { null.x } catch (e) { return e instanceof TypeError } })"); f()`, ""],
  [`var f = r.evaluate("(function () { try { null.x } catch (e) { return e.constructor === TypeError } })"); f()`, ""],
  [`var f = r.evaluate("(function () { try { null.x } catch (e) { return e.constructor.name } })"); f()`, ""],
  [`var f = r.evaluate("(function (E) { return E })"); f(TypeError)`, ""],
  [`var f = r.evaluate("(function (E) { return E })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return new E('x') })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return typeof new E('x') })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return new E('x') instanceof Error })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { try { new E('x') } catch (e) { return e.message } })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { try { new E('x') } catch (e) { return e.constructor === TypeError } })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return Reflect.construct(E, ['x']) })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return E.name })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return E.prototype })"); f(Error)`, ""],
  [`var f = r.evaluate("(function (E) { return Object.getPrototypeOf(E) === Function.prototype })"); f(Error)`, ""],
  [`var f = r.evaluate("(function () { return class A {} })"); typeof f()`, ""],
  [`var f = r.evaluate("(function () { return class A {} })"); f().name`, ""],
  [`var f = r.evaluate("(function () { return class A { constructor() { this.x = 1 } } })"); typeof new (f())()`, ""],
  [`var f = r.evaluate("(function () { return class A { constructor() { this.x = 1 } } })"); new (f())() instanceof (f())`, ""],
  [`var f = r.evaluate("(function () { return Symbol.iterator })"); f() === Symbol.iterator`, ""],
  [`var f = r.evaluate("(function () { return Symbol.for('k') })"); f() === Symbol.for('k')`, ""],
  [`var f = r.evaluate("(function (s) { return s === Symbol.iterator })"); f(Symbol.iterator)`, ""],
  [`var f = r.evaluate("(function (s) { return s === Symbol.for('k') })"); f(Symbol.for('k'))`, ""],
  [`var f = r.evaluate("(function (s) { return Symbol.keyFor(s) })"); f(Symbol.for('key'))`, ""],
  [`var f = r.evaluate("(function () { return Symbol('local') })"); f().toString()`, ""],
  [`var f = r.evaluate("(function () { return Symbol('local') })"); f().description`, ""],
  [`var f = r.evaluate("(function () { return Symbol('local') })"); f() === f()`, ""],
  [`var f = r.evaluate("(function (a, b) { return a === b })"); f(1n, 1n)`, ""],
  [`var f = r.evaluate("(function (a, b) { return a === b })"); f('x', 'x')`, ""],
  [`var f = r.evaluate("(function (a, b) { return a === b })"); var s = Symbol(); f(s, s)`, ""],
  [`var f = r.evaluate("(function (a, b) { return a === b })"); f(NaN, NaN)`, ""],
  [`var f = r.evaluate("(function (a, b) { return Object.is(a, b) })"); f(-0, 0)`, ""],
  [`var f = r.evaluate("(function (a) { return Object.is(a, -0) })"); f(-0)`, ""],
  [`var f = r.evaluate("(function (a) { return 1 / a })"); f(-0)`, ""],
  [`var f = r.evaluate("(function () { return -0 })"); 1 / f()`, ""],
  [`var f = r.evaluate("(function () { return 0.1 + 0.2 })"); f()`, ""],
  [`var f = r.evaluate("(function (a) { return a })"); f('\\ud800')`, ""],
  [`var f = r.evaluate("(function (a) { return a.length })"); f('\\u{1F600}')`, ""],
  [`var f = r.evaluate("(function (a) { return a })"); f(2n ** 100n)`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a })"); f(2n ** 100n)`, ""],
  [`var f = r.evaluate("(function (a) { return a })"); f(Infinity)`, ""],
  [`var f = r.evaluate("(function (a) { return a })"); f(undefined) === undefined`, ""],
  [`var f = r.evaluate("(function (a) { return a })"); f(null) === null`, ""],
  [`var f = r.evaluate("(function () { return undefined })"); f() === undefined`, ""],
  [`var f = r.evaluate("(function () { return null })"); f() === null`, ""],
  [`var f = r.evaluate("(function () { return [] })"); f()`, ""],
  [`var f = r.evaluate("(function () { return new Promise(() => {}) })"); f()`, ""],
  [`var f = r.evaluate("(function () { return Promise.resolve(1) })"); f()`, ""],
  [`var f = r.evaluate("(function () { return globalThis })"); f()`, ""],
  [`var f = r.evaluate("(function () { return this })"); f()`, ""],
  [`var f = r.evaluate("(function () { return this })"); f.call({})`, ""],
  [`var f = r.evaluate("(function () { return this })"); f.call(1)`, ""],
  [`var f = r.evaluate("(function () { return arguments })"); f()`, ""],
  [`var f = r.evaluate("(function (...a) { return a })"); f(1)`, ""],
  [`var f = r.evaluate("(function (a) { a.x = 1 })"); f({})`, ""],
  [`var f = r.evaluate("(function (a) { a.x = 1 })"); var o = {}; try { f(o) } catch (e) {} Object.keys(o).length`, ""],
  [`var f = r.evaluate("(function (a) { return a() })"); f(function () { return 1 })`, ""],
  [`var f = r.evaluate("(function (a) { return a() })"); f(function () { return {} })`, ""],
  [`var f = r.evaluate("(function (a) { return a() })"); f(function () { return function () { return 7 } })`, ""],
  [`var f = r.evaluate("(function (a) { return a()() })"); f(function () { return function () { return 7 } })`, ""],
  [`var f = r.evaluate("(function (a) { return a()() })"); f(function () { return function () { return {} } })`, ""],
  [`var f = r.evaluate("(function (a) { return a.call })"); typeof f(function () {})`, ""],
  [`var f = r.evaluate("(function (a) { return a.call === Function.prototype.call })"); f(function () {})`, ""],
  [`var f = r.evaluate("(function (a) { return a.call === Function.prototype.call })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (a) { return a.bind === Function.prototype.bind })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (a) { return Reflect.ownKeys(a).join() })"); f(function foo(x) {})`, ""],
  [`var f = r.evaluate("(function (a) { return Reflect.ownKeys(a).join() })"); f(() => 1)`, ""],
  [`var f = r.evaluate("(function (a) { return a.name })"); f(function foo(x) {})`, ""],
  [`var f = r.evaluate("(function (a) { return a.length })"); f(function foo(x, y) {})`, ""],
  [`var f = r.evaluate("(function (a) { return a.prototype })"); f(function foo(x, y) {})`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a.prototype })"); f(function foo(x, y) {})`, ""],
  [`var f = r.evaluate("(function (a) { return a.caller })"); f(function foo(x, y) {})`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a })"); f(new Proxy(function () {}, {}))`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a })"); f(new Proxy({}, {}))`, ""],
  [`var f = r.evaluate("(function (a) { return a() })"); f(new Proxy(function () { return 5 }, {}))`, ""],
  [`var f = r.evaluate("(function (a) { return typeof a })"); f(class A {})`, ""],
  [`var f = r.evaluate("(function (A) { return new A().x })"); f(class A { constructor() { this.x = 1 } })`, ""],
  [`var f = r.evaluate("(function (A) { return typeof new A() })"); f(class A { constructor() { this.x = 1 } })`, ""],
  [`var f = r.evaluate("(function (A) { return A() })"); f(class A {})`, ""],
  [`var f = r.evaluate("(function (A) { return A() })"); f(async function () {})`, ""],
  [`var f = r.evaluate("(function (A) { return typeof A() })"); f(async function () {})`, ""],
  [`var f = r.evaluate("(function (A) { return typeof A().next })"); f(function* () {})`, ""],
  [`var f = r.evaluate("(function (A) { return typeof A() })"); f(function* () {})`, ""],
  [`var f = r.evaluate("(function (A) { return typeof A })"); f(async () => 1)`, ""],
];
for (const [body, pre] of isolation) {
  programs.push(describe(`(() => { ${body.includes(";") ? body.replace(/;\s*([^;]+)$/, "; return ($1)") : "return (" + body + ")"} })()`, PRE + pre));
}
// Globais herdados: o global do realm é distinto do chamador (várias propriedades).
for (const name of ["Array", "Object", "Function", "Error", "TypeError", "Promise", "Map", "Set", "Symbol", "Date", "RegExp", "JSON", "Math", "Reflect", "Proxy", "String", "Number", "Boolean", "BigInt", "Intl", "WeakMap", "Uint8Array", "ArrayBuffer", "Iterator", "AggregateError", "ShadowRealm", "eval", "parseInt", "isNaN", "globalThis"]) {
  programs.push(tryExpr(`typeof r.evaluate(${JSON.stringify("typeof " + name)})`));
  programs.push(tryExpr(`r.evaluate(${JSON.stringify("typeof " + name)})`));
  programs.push(tryExpr(`(() => { var g = r.evaluate(${JSON.stringify("(function () { return " + name + " })")}); var v = g(); return v === ${name} })()`));
  programs.push(tryExpr(`(() => { var g = r.evaluate(${JSON.stringify("(function () { return typeof " + name + "; })")}); return g() })()`));
}
// Realm aninhado.
for (const e of [
  "r.evaluate('new ShadowRealm().evaluate(\"1+2\")')",
  "r.evaluate('typeof new ShadowRealm()')",
  "r.evaluate('new ShadowRealm()')",
  "r.evaluate('new ShadowRealm().evaluate(\"({})\")')",
  "r.evaluate('new ShadowRealm().evaluate(\"throw 1\")')",
  "r.evaluate('(function () { try { new ShadowRealm().evaluate(\"throw 1\") } catch (e) { return e.constructor === TypeError } })()')",
  "r.evaluate('new ShadowRealm().evaluate(\"(function () { return 5 })\")()')",
  "r.evaluate('ShadowRealm') === ShadowRealm",
  "r.evaluate('Object.getPrototypeOf(new ShadowRealm()) === ShadowRealm.prototype')",
  "r.evaluate('ShadowRealm.prototype[Symbol.toStringTag]')",
  "r.evaluate('Object.prototype.toString.call(new ShadowRealm())')",
  "r.evaluate('ShadowRealm.prototype.evaluate.call(new ShadowRealm(), \"2*3\")')",
]) {
  programs.push(tryExpr(e));
}
// Mesmo realm, evaluate com objeto de outro realm no receptor.
programs.push(tryExpr("(() => { var other = r.evaluate('new ShadowRealm()'); return typeof other })()"));
programs.push(tryExpr("(() => { var other = r.evaluate('ShadowRealm'); return other === ShadowRealm })()"));
programs.push(tryExpr("(() => { var f = r.evaluate('(function () { return new ShadowRealm() })'); return f() })()"));
programs.push(tryExpr("(() => { var f = r.evaluate('(function (R) { return new R() })'); return f(ShadowRealm) })()"));
programs.push(tryExpr("(() => { var f = r.evaluate('(function (R) { return new R().evaluate(\"1+1\") })'); return f(ShadowRealm) })()"));

// ---- Error de outro realm.
for (const e of [
  "var E = r.evaluate('(function () { return new Error(1) })'); E()",
  "var E = r.evaluate('(function () { try { throw new Error(1) } catch (e) { return e } })'); E()",
  "var E = r.evaluate('(function () { return [new Error(\"m\") instanceof Error, Object.getPrototypeOf(new Error(\"m\")) === Error.prototype].join() })'); E()",
  "var E = r.evaluate('(function (e) { return e instanceof Error })'); E(new Error('x'))",
  "var E = r.evaluate('(function (e) { return e })'); E(new Error('x'))",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e instanceof TypeError } })'); E(() => { null.x })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { null.x })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.constructor === TypeError } })'); E(() => { throw new Error('x') })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.constructor === TypeError } })'); E(() => { throw 1 })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw new Error('orig') })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw 'str' })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw Symbol('x') })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw { a: 1 } })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw undefined })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw null })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw 5n })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw function () {} })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw new Error('') })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.message } })'); E(() => { throw new Error() })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return Object.getOwnPropertyNames(e).sort().join() } })'); E(() => { throw 1 })",
  "var E = r.evaluate('(function (f) { try { f() } catch (e) { return e.cause } })'); E(() => { throw 1 })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { throw 1 })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { throw new Error('x') })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { throw {} })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { undefinedVariable })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { new (void 0) })",
  "var E = r.evaluate('(function (f) { return f() })'); E(() => { throw new RangeError('range') })",
]) {
  programs.push(tryExpr(`(() => { ${e} })()`.replace(/\}\)\(\)$/, "})()").replace(/; ([A-Za-z]+\()/, "; return $1")));
}
// Chamada que lança dentro da função remota, sem passar pelo evaluate.
for (const body of ["throw 1", "throw 'x'", "throw new Error('e')", "throw {}", "throw undefined", "throw null", "throw Symbol()", "throw new RangeError('r')", "null.x", "nope", "new (void 0)", "throw new TypeError('t')", "throw function () {}", "throw 1n", "throw new Error('')"]) {
  programs.push(describe(`(() => { try { f(); return 'no error' } catch (e) { return [e.constructor === TypeError, e instanceof TypeError, e.name, e.message].join('|') } })()`, PRE + `var f = r.evaluate(${JSON.stringify("(function () { " + body + " })")});\n`));
  programs.push(describe(`(() => { try { new f(); return 'no error' } catch (e) { return [e.constructor === TypeError, e.name, e.message].join('|') } })()`, PRE + `var f = r.evaluate(${JSON.stringify("(function () { " + body + " })")});\n`));
}

// ---- importValue.
const importCases = [
  // argumentos inválidos (síncrono ou rejeição)
  "r.importValue()", "r.importValue('./x.js')", "r.importValue('./x.js', undefined)", "r.importValue(1, 'a')", "r.importValue('./x.js', 1)", "r.importValue('./x.js', Symbol())", "r.importValue(Symbol(), 'a')",
  "r.importValue({}, 'a')", "r.importValue('./x.js', {})", "r.importValue(undefined, undefined)", "r.importValue(null, 'a')", "r.importValue('./x.js', null)", "r.importValue(1n, 'a')",
  "r.importValue({ toString() { throw new Error('ts') } }, 'a')", "r.importValue('./x.js', { toString() { throw new Error('ts2') } })",
  "r.importValue({ toString() { return './nonexistent-module-xyz.js' } }, 'a')", "r.importValue('./nonexistent-module-xyz.js', { toString() { return 'a' } })",
  // módulo inexistente
  "r.importValue('./nonexistent-module-xyz.js', 'a')", "r.importValue('nonexistent-package-xyz', 'a')", "r.importValue('', 'a')", "r.importValue('./', 'a')", "r.importValue('./x.js', '')",
  "r.importValue('data:text/javascript,export const a = 1', 'a')", "r.importValue('data:text/javascript,export const a = 1', 'b')", "r.importValue('data:text/javascript,export default 1', 'default')",
  "r.importValue('data:text/javascript,export function f() { return 1 }', 'f')", "r.importValue('data:text/javascript,export const o = {}', 'o')", "r.importValue('data:text/javascript,export const s = \"str\"', 's')",
  "r.importValue('data:text/javascript,export const n = 42', 'n')", "r.importValue('data:text/javascript,export const u = undefined', 'u')", "r.importValue('data:text/javascript,throw new Error(1)', 'a')",
  "r.importValue('data:text/javascript,syntax error here', 'a')", "r.importValue('data:text/javascript,export const a = 1', 'toString')", "r.importValue('data:text/javascript,export const a = 1', '__proto__')",
  "r.importValue('data:text/javascript,export const a = 1', 'then')", "r.importValue('data:text/javascript,export const a = () => 3', 'a')",
];
for (const e of importCases) {
  programs.push(
    PRE +
      `var out = [];\ntry { var p = ${e}; out.push(p instanceof Promise, Object.getPrototypeOf(p) === Promise.prototype); p.then(v => { out.push('ok:' + typeof v + ':' + (typeof v === 'function' ? v() : String(v))); globalThis.R = out.join('|') }, e => { out.push('rej:' + show(e)); globalThis.R = out.join('|') }) } catch (e) { globalThis.R = 'sync:' + show(e) }`,
  );
}
// Receptor da importValue, tipo da promessa, comprimento etc.
for (const e of ["r.importValue('./x.js', 'a') instanceof Promise", "typeof r.importValue", "r.importValue.length", "ShadowRealm.prototype.importValue.call({}, 'a', 'b')"]) {
  programs.push(tryExpr(e));
}
// importValue de módulo que falha: erro vira TypeError do chamador.
for (const modsrc of ["throw 1", "throw new Error('mod')", "export const a = 1; throw 'x'", "syntax error here", "import './nonexistent-nested.js'", "import { x } from 'data:text/javascript,export const y = 1'", "await 1; export const a = 2", "export const a = 1", "export default function () { return 9 }"]) {
  programs.push(
    PRE +
      `var url = 'data:text/javascript,' + encodeURIComponent(${JSON.stringify(modsrc)});\nr.importValue(url, 'a').then(v => { globalThis.R = 'ok:' + typeof v }, e => { globalThis.R = 'rej:' + [e.constructor === TypeError, e.name, e.message].join('|') })`,
  );
}
// importValue devolvendo funções: wrapped.
programs.push(PRE + `r.importValue('data:text/javascript,export function f(a) { return a + 1 }', 'f').then(f => { globalThis.R = [typeof f, f.name, f.length, f(1), 'prototype' in f].join('|') }, e => { globalThis.R = show(e) })`);
programs.push(PRE + `r.importValue('data:text/javascript,export const o = { a: 1 }', 'o').then(v => { globalThis.R = 'ok' }, e => { globalThis.R = show(e) })`);
programs.push(PRE + `r.importValue('data:text/javascript,export const a = 1', 'a').then(v => { globalThis.R = 'ok:' + v }, e => { globalThis.R = show(e) })`);
programs.push(PRE + `r.importValue('data:text/javascript,export class C {}', 'C').then(v => { globalThis.R = [typeof v, v.name].join('|') }, e => { globalThis.R = show(e) })`);
programs.push(PRE + `r.importValue('data:text/javascript,export const sym = Symbol.iterator', 'sym').then(v => { globalThis.R = String(v === Symbol.iterator) }, e => { globalThis.R = show(e) })`);

// ---- toStringTag / protótipo em contexto.
for (const e of [
  "Object.prototype.toString.call(r)", "String(r)", "r + ''", "`${r}`", "Object.getOwnPropertyNames(r).length", "r.constructor === ShadowRealm", "r.hasOwnProperty('evaluate')", "'evaluate' in r",
  "r.evaluate === ShadowRealm.prototype.evaluate", "r.importValue === ShadowRealm.prototype.importValue", "JSON.stringify(r)", "Object.keys(r).join()", "typeof r.evaluate", "typeof r.importValue", "typeof r.eval",
  "Symbol.toStringTag in r", "ShadowRealm.prototype.hasOwnProperty(Symbol.toStringTag)", "Object.getPrototypeOf(ShadowRealm.prototype) === Object.prototype", "Object.isFrozen(ShadowRealm.prototype)",
  "Object.isFrozen(ShadowRealm)", "(() => { 'use strict'; r.x = 1; return r.x })()", "(() => { 'use strict'; r.evaluate = 1; return typeof r.evaluate })()", "delete ShadowRealm.prototype.evaluate", "ShadowRealm.prototype.evaluate = 1",
  "(() => { var o = Object.create(ShadowRealm.prototype); return Object.prototype.toString.call(o) })()", "ShadowRealm.prototype.evaluate.call(Object.create(ShadowRealm.prototype), '1')",
  "ShadowRealm.prototype.evaluate.call(r, '1+1')", "ShadowRealm.prototype.evaluate.apply(r, ['2+2'])", "Reflect.apply(ShadowRealm.prototype.evaluate, r, ['3+3'])", "ShadowRealm.prototype.evaluate.bind(r)('4+4')",
  "(0, r.evaluate)('1')", "[r].map(x => x.evaluate('5+5'))[0]", "new (r.evaluate)('1')", "new r.evaluate('1')", "new (ShadowRealm.prototype.evaluate)()", "new ShadowRealm.prototype.importValue('a', 'b')",
  "r.evaluate.call(r, '1+8')", "Object.getPrototypeOf(r.evaluate) === Function.prototype", "r.evaluate.hasOwnProperty('prototype')",
]) {
  programs.push(tryExpr(e));
}

// ---- Várias instâncias e ordem de efeitos.
for (const e of [
  "var a = new ShadowRealm(), b = new ShadowRealm(); a.evaluate('var v = 1'); a.evaluate('v') + '|' + b.evaluate('typeof v')",
  "var a = new ShadowRealm(), b = new ShadowRealm(); a.evaluate('Object.prototype.z = 1'); [a.evaluate('({}).z'), b.evaluate('({}).z')].join()",
  "var a = new ShadowRealm(); var f = a.evaluate('(function () { return globalThis.c = (globalThis.c || 0) + 1 })'); [f(), f(), f()].join()",
  "var a = new ShadowRealm(); var f = a.evaluate('(function () { return globalThis.c = (globalThis.c || 0) + 1 })'); f(); a.evaluate('c')",
  "var a = new ShadowRealm(); var f = a.evaluate('(function () { return globalThis.c = (globalThis.c || 0) + 1 })'); f(); typeof c",
  "var a = new ShadowRealm(), b = new ShadowRealm(); var f = a.evaluate('(function (g) { return g() })'); var g = b.evaluate('(function () { return 11 })'); f(g)",
  "var a = new ShadowRealm(), b = new ShadowRealm(); var f = a.evaluate('(function (g) { return g() })'); var g = b.evaluate('(function () { return {} })'); try { f(g) } catch (e) { e.constructor.name + ': ' + e.message }",
  "var a = new ShadowRealm(), b = new ShadowRealm(); var f = a.evaluate('(function (g) { return typeof g })'); f(b.evaluate('(function () {})'))",
  "var a = new ShadowRealm(); a.evaluate('var o = {}'); a.evaluate('typeof o')",
  "var a = new ShadowRealm(); a.evaluate('var o = {}'); a.evaluate('o')",
  "var a = new ShadowRealm(); a.evaluate('function g() { return 1 }'); a.evaluate('g()')",
  "var a = new ShadowRealm(); a.evaluate('function g() { return 1 }'); a.evaluate('g')",
  "var a = new ShadowRealm(); a.evaluate('function g() { return 1 }'); typeof a.evaluate('g')",
  "var a = new ShadowRealm(); a.evaluate('class K {}'); a.evaluate('typeof K')",
  "var a = new ShadowRealm(); a.evaluate('class K {}'); a.evaluate('class K {}')",
  "var a = new ShadowRealm(); a.evaluate('let q = 1'); a.evaluate('var q')",
  "var a = new ShadowRealm(); a.evaluate('var q = 1'); a.evaluate('let q')",
  "var a = new ShadowRealm(); a.evaluate('Object.freeze(Object.prototype)'); a.evaluate('Object.isFrozen(Object.prototype)') + '|' + Object.isFrozen(Object.prototype)",
  "var a = new ShadowRealm(); a.evaluate('Object.freeze(globalThis)'); a.evaluate('Object.isFrozen(globalThis)') + '|' + Object.isFrozen(globalThis)",
  "var a = new ShadowRealm(); a.evaluate('globalThis.Symbol = 1'); typeof Symbol",
  "var a = new ShadowRealm(); a.evaluate('Symbol.iterator') === Symbol.iterator",
  "var a = new ShadowRealm(); a.evaluate('Symbol.for(\"x\")') === Symbol.for('x')",
  "var a = new ShadowRealm(); a.evaluate('Symbol.for(\"x\") === Symbol.for(\"x\")')",
  "var a = new ShadowRealm(); a.evaluate('Symbol(\"x\")') === a.evaluate('Symbol(\"x\")')",
  "var a = new ShadowRealm(); a.evaluate('Symbol.toStringTag') === Symbol.toStringTag",
  "var a = new ShadowRealm(); var order = []; try { a.evaluate('throw 1') } catch (e) { order.push('caught') } order.join()",
  "var a = new ShadowRealm(); var n = 0; for (var i = 0; i < 20; i++) n += a.evaluate(String(i)); n",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (n) { return n < 2 ? n : 0 })'); f(1) + f(0) + f(5)",
  "var a = new ShadowRealm(); var f = a.evaluate('(function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2) })'); f(15)",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (s) { return s.split(\"\").reverse().join(\"\") })'); f('abc')",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (s) { return s.length })'); f('é\\u{1F600}')",
  "var a = new ShadowRealm(); var f = a.evaluate('(function () { return \"\\\\u00e9\" })'); f().length",
  "var a = new ShadowRealm(); var f = a.evaluate('(function () { return 1n << 70n })'); f()",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a, b) { return a * b })'); f(1n << 40n, 3n)",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a, b) { return a * b })'); f(2, 3n)",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a) { return a + 1 })'); f('1')",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a) { return a + 1 })'); f(Symbol())",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a) { return `${a}` })'); f(Symbol())",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a) { return String(a) })'); f(Symbol('d'))",
  "var a = new ShadowRealm(); var f = a.evaluate('(function (a) { return a.description })'); f(Symbol('d'))",
]) {
  programs.push(tryExpr(`(() => { ${e.replace(/; ([^;]+)$/, "; return ($1)")} })()`, ""));
}

// ---- Funções remotas encadeadas e propriedades dinâmicas.
for (const [src, probe] of [
  ["(function () { return function () { return function () { return 1 } } })", "f()()()"],
  ["(function () { return function () { return function () { return 1 } } })", "typeof f()()"],
  ["(function () { return function () { return function () { return {} } } })", "f()()()"],
  ["(function () { return (a) => (b) => a + b })", "f()(1)"],
  ["(function () { return (a) => (b) => a + b })", "f()(1)(2)"],
  ["(function () { return function named(a, b) {} })", "f().name + f().length"],
  ["(function () { return function named(a, b) {} })", "f() === f()"],
  ["(function () { var g = function () {}; return g })", "f() === f()"],
  ["(function () { var g = function () {}; return () => g })", "f()() === f()()"],
  ["(function () { var g = function () {}; return () => g })", "f()() === f()"],
  ["(function () { return function () { return this } })", "f().call({})"],
  ["(function () { return function () { return typeof this } })", "f().call(1)"],
  ["(function () { return function () { return typeof this } })", "f()()"],
  ["(function () { return class A {} })", "f().name"],
  ["(function () { return class A { static s() { return 5 } } })", "f().s()"],
  ["(function () { return class A { static s() { return 5 } } })", "typeof f().s"],
  ["(function () { return class A { static s() { return 5 } } })", "f().hasOwnProperty('s')"],
  ["(function () { return Math.max })", "f()(1, 2, 3)"],
  ["(function () { return Math.max })", "f().name"],
  ["(function () { return parseInt })", "f()('12')"],
  ["(function () { return JSON.stringify })", "f()(1)"],
  ["(function () { return JSON.stringify })", "f()({})"],
  ["(function () { return JSON.parse })", "f()('1')"],
  ["(function () { return JSON.parse })", "f()('{')"],
  ["(function () { return Array.isArray })", "f()([])"],
  ["(function () { return Array.from })", "f()([1])"],
  ["(function () { return Object.keys })", "f()({ a: 1 })"],
  ["(function () { return Object.assign })", "f()({}, {})"],
  ["(function () { return Object.create })", "f()(null)"],
  ["(function () { return String })", "f()(1)"],
  ["(function () { return String })", "f()({})"],
  ["(function () { return String })", "f()(Symbol('x'))"],
  ["(function () { return Number })", "f()('7')"],
  ["(function () { return Boolean })", "f()(0)"],
  ["(function () { return Symbol })", "f()('x')"],
  ["(function () { return Symbol })", "typeof f()('x')"],
  ["(function () { return BigInt })", "f()(5)"],
  ["(function () { return Date.now })", "typeof f()()"],
  ["(function () { return Promise.resolve })", "f()(1)"],
  ["(function () { return Promise.resolve })", "typeof f()"],
  ["(function () { return eval })", "f()('1+1')"],
  ["(function () { return eval })", "f()('({})')"],
  ["(function () { return Function })", "f()('return 1')()"],
  ["(function () { return Function })", "typeof f()('return 1')"],
  ["(function () { return Function })", "f().name"],
  ["(function () { return setTimeout })", "typeof f()"],
  ["(function () { return queueMicrotask })", "typeof f()"],
  ["(function () { return globalThis.eval })", "typeof f()"],
  ["(function () { return Object.prototype.toString })", "f().call([])"],
  ["(function () { return Object.prototype.toString })", "f().call({})"],
  ["(function () { return Object.prototype.hasOwnProperty })", "f().call({ a: 1 }, 'a')"],
  ["(function () { return Array.prototype.push })", "f().call([], 1)"],
  ["(function () { return Array.prototype.map })", "f().call([1, 2], x => x * 2)"],
  ["(function () { return Array.prototype.map })", "f().call([1, 2], function (x) { return {} })"],
  ["(function () { return Array.prototype.map })", "f().call([1, 2], function (x) { return x })"],
  ["(function () { return function (cb) { return cb(1) } })", "f()(x => x + 1)"],
  ["(function () { return function (cb) { return cb(1) } })", "f()(x => ({}))"],
  ["(function () { return function (cb) { return cb } })", "f()(x => 1) === f()"],
  ["(function () { return function (cb) { return cb } })", "typeof f()(x => 1)"],
  ["(function () { return function (cb) { return cb } })", "f()(x => 1)(2)"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; f()(cb) === cb"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; f()(cb)(5)"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; f()(cb).name"],
  ["(function () { return function (cb) { return cb } })", "var cb = function named(x) {}; f()(cb).name + f()(cb).length"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; f()(f()(cb))(5)"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; typeof f()(f()(cb))"],
  ["(function () { return function (cb) { return cb } })", "var cb = x => x + 1; f()(f()(cb)) === cb"],
]) {
  programs.push(describe(probe.startsWith("var ") ? `(() => { ${probe.replace(/; ([^;]+)$/, "; return ($1)")} })()` : probe, PRE + `var f = r.evaluate(${JSON.stringify(src)});\n`));
}

// ---- Promessas e assincronia dentro do realm.
for (const e of [
  "r.evaluate('(async function () { return 1 })')()", "r.evaluate('(function () { return Promise.resolve(1) })')()", "r.evaluate('Promise.resolve(1)')", "r.evaluate('(async () => 1)()')",
  "r.evaluate('queueMicrotask')", "r.evaluate('typeof queueMicrotask')", "r.evaluate('typeof setTimeout')", "r.evaluate('typeof Promise')",
  "r.evaluate('(function () { var out = []; Promise.resolve().then(() => out.push(1)); out.push(0); return out.join() })')()",
  "r.evaluate('(function () { return typeof (async function () {}).constructor })')()",
  "r.evaluate('Object.getPrototypeOf(async function () {}).constructor.name')", "r.evaluate('Object.getPrototypeOf(function* () {}).constructor.name')",
  "r.evaluate('(function () { return typeof Symbol.asyncIterator })')()", "r.evaluate('typeof Symbol.asyncIterator')",
]) {
  programs.push(
    PRE +
      `try { var v = ${e}; if (v && typeof v.then === 'function') v.then(x => { globalThis.R = 'then:' + typeof x + ':' + String(x) }, e => { globalThis.R = 'rej:' + show(e) }); else globalThis.R = typeof v + ':' + String(v) } catch (e) { globalThis.R = show(e) }`,
  );
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "shadow-realm-golden-"));
const file = path.join(dir, "shadow_realm_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
const lines = [];
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body;
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result === "<undefined>" || result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("descartado (indefinido ou caminho da máquina): " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("shadow_realm", lines));
fs.rmSync(dir, { recursive: true, force: true });
