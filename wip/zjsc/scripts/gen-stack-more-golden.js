// Gera tests/golden/stack_more_bun.tsv: o `Error.stack` de 500 programas medido no bun 1.4.2, ampliando
// `gen-stack-golden.js`. Colunas: a fonte (JSON) e o valor da variável global `R` (JSON). O arquivo se chama
// `file.js` dos dois lados: o diretório temporário sai do texto, o que sobra é a URL que o zjsc imprime.
// Programas que gravam `R` depois de `await`/`then` valem porque o bun lê `R` na saída do processo.
// Também grava tests/golden/stack_more.preludes.json (prelúdios fatorados) e a quinta coluna (modo e mapa de posições).
// Uso: bun scripts/gen-stack-more-golden.js > tests/golden/stack_more_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const lines = (...rows) => rows.join("\n");
const P = "new Error('x').stack";
const TARGET = 500;

// Famílias com cobertura obrigatória entram primeiro; a combinatória grande entra por último e é cortada em 500.
const priority = [];
const bulk = [];

// ---------------------------------------------------------------------------------------------
// Tipos de função: `defs` declara, `call` produz o valor sincronamente (ou uma Promise, se `async`).
// ---------------------------------------------------------------------------------------------
const kinds = [
  { n: "named", defs: `function f() { return ${P} }`, call: "f()" },
  { n: "anon-var", defs: `var f = function () { return ${P} }`, call: "f()" },
  { n: "anon-iife", defs: "", call: `(function () { return ${P} })()` },
  { n: "arrow", defs: `var f = () => ${P}`, call: "f()" },
  { n: "arrow-block", defs: `var f = () => { return ${P} }`, call: "f()" },
  { n: "method", defs: `var o = { m() { return ${P} } }`, call: "o.m()" },
  { n: "method-fn", defs: `var o = { m: function () { return ${P} } }`, call: "o.m()" },
  { n: "method-named-fn", defs: `var o = { m: function inner() { return ${P} } }`, call: "o.m()" },
  { n: "getter", defs: `var o = { get g() { return ${P} } }`, call: "o.g" },
  { n: "setter", defs: `var o = { set s(v) { globalThis.T = ${P} } }`, call: "(o.s = 1, globalThis.T)" },
  { n: "ctor", defs: `class K { constructor() { this.s = ${P} } }`, call: "new K().s" },
  { n: "derived-ctor", defs: `class B { constructor() { this.s = ${P} } }\nclass C extends B { constructor() { super() } }`, call: "new C().s" },
  { n: "derived-ctor-implicit", defs: `class B { constructor() { this.s = ${P} } }\nclass C extends B {}`, call: "new C().s" },
  { n: "class-method", defs: `class K { m() { return ${P} } }`, call: "new K().m()" },
  { n: "static-method", defs: `class K { static s() { return ${P} } }`, call: "K.s()" },
  { n: "static-getter", defs: `class K { static get g() { return ${P} } }`, call: "K.g" },
  { n: "static-block", defs: `class K { static r = null; static { K.r = ${P} } }`, call: "K.r" },
  { n: "field-init", defs: `class K { f = ${P} }`, call: "new K().f" },
  { n: "static-field-init", defs: `class K { static f = ${P} }`, call: "K.f" },
  { n: "private-method", defs: `class K { #p() { return ${P} } q() { return this.#p() } }`, call: "new K().q()" },
  { n: "computed-method", defs: `var k = 'dyn'; var o = { [k]() { return ${P} } }`, call: "o.dyn()" },
  { n: "symbol-method", defs: `var s = Symbol('s'); var o = { [s]() { return ${P} } }`, call: "o[s]()" },
  { n: "generator", defs: `function* g() { yield ${P} }`, call: "g().next().value" },
  { n: "generator-method", defs: `var o = { *g() { yield ${P} } }`, call: "o.g().next().value" },
  { n: "new-target", defs: `function F() { this.s = ${P}; this.nt = new.target === F }`, call: "new F().s" },
  { n: "eval-inside", defs: `function f() { return eval("new Error('x').stack") }`, call: "f()" },
  { n: "eval-top", defs: "", call: `eval("new Error('x').stack")` },
  { n: "eval-calls-fn", defs: `function g() { return ${P} }`, call: `eval("g()")` },
  { n: "indirect-eval", defs: "", call: `(0, eval)("new Error('x').stack")` },
  { n: "new-function", defs: "", call: `new Function("return new Error('x').stack")()` },
  { n: "new-function-args", defs: "", call: `new Function('a', 'b', "return new Error('x').stack")(1, 2)` },
  { n: "proxy-get", defs: `var p = new Proxy({}, { get(t, k) { return ${P} } })`, call: "p.foo" },
  { n: "proxy-set", defs: `var p = new Proxy({}, { set(t, k, v) { globalThis.T = ${P}; return true } })`, call: "(p.x = 1, globalThis.T)" },
  { n: "proxy-has", defs: `var p = new Proxy({}, { has(t, k) { globalThis.T = ${P}; return true } })`, call: "('x' in p, globalThis.T)" },
  { n: "proxy-apply", defs: `var p = new Proxy(function () {}, { apply() { return ${P} } })`, call: "p()" },
  { n: "proxy-construct", defs: `var p = new Proxy(function () {}, { construct() { return { s: ${P} } } })`, call: "new p().s" },
  { n: "tagged", defs: `function tag(s) { return ${P} }`, call: "tag`a${1}b`" },
  { n: "tagged-method", defs: `var o = { tag(s) { return ${P} } }`, call: "o.tag`a`" },
  { n: "define-getter", defs: `var o = {}; Object.defineProperty(o, 'p', { get() { return ${P} } })`, call: "o.p" },
  { n: "define-getter-named", defs: `var o = {}; Object.defineProperty(o, 'p', { get: function gp() { return ${P} } })`, call: "o.p" },
  { n: "define-value-fn", defs: `var o = {}; Object.defineProperty(o, 'p', { value: function vp() { return ${P} } })`, call: "o.p()" },
  { n: "to-primitive", defs: `var o = { [Symbol.toPrimitive]() { globalThis.T = ${P}; return 1 } }`, call: "(+o, globalThis.T)" },
  { n: "to-string-impl", defs: `var o = { toString() { globalThis.T = ${P}; return 'a' } }`, call: "('' + o, globalThis.T)" },
  { n: "value-of-impl", defs: `var o = { valueOf() { globalThis.T = ${P}; return 1 } }`, call: "(o * 2, globalThis.T)" },
  { n: "instanceof-hook", defs: `var C = { [Symbol.hasInstance]() { globalThis.T = ${P}; return true } }`, call: "(1 instanceof C, globalThis.T)" },
  { n: "iterator-next", defs: `var it = { [Symbol.iterator]() { return { next() { globalThis.T = ${P}; return { done: true } } } } }`, call: "([...it], globalThis.T)" },
  { n: "bound-fn", defs: `function f() { return ${P} }\nvar b = f.bind(null)`, call: "b()" },
  { n: "arrow-in-method", defs: `var o = { m() { return (() => ${P})() } }`, call: "o.m()" },
  { n: "nested-named", defs: `function outer() { function inner() { return ${P} } return inner() }`, call: "outer()" },
  { n: "closure-factory", defs: `function mk() { return function made() { return ${P} } }`, call: "mk()()" },
  { n: "accessor-class-pair", defs: `class K { get a() { return this._a } set a(v) { this._a = ${P} } }`, call: "(function () { var k = new K(); k.a = 1; return k.a })()" },
  { n: "async-fn", async: true, defs: `async function a() { return ${P} }`, call: "a()" },
  { n: "async-await-first", async: true, defs: `async function a() { await 1; return ${P} }`, call: "a()" },
  { n: "async-arrow", async: true, defs: `var a = async () => { await 1; return ${P} }`, call: "a()" },
  { n: "async-method", async: true, defs: `var o = { async m() { await 1; return ${P} } }`, call: "o.m()" },
  { n: "async-static", async: true, defs: `class K { static async s() { await null; return ${P} } }`, call: "K.s()" },
  { n: "async-nested", async: true, defs: `async function inner() { await 0; return ${P} }\nasync function outer() { return await inner() }`, call: "outer()" },
  { n: "async-nested-noawait", async: true, defs: `async function inner() { await 0; return ${P} }\nasync function outer() { return inner() }`, call: "outer()" },
  { n: "async-two-awaits", async: true, defs: `async function a() { await 1; await 2; return ${P} }`, call: "a()" },
  { n: "async-gen", async: true, defs: `async function* g() { await 1; yield ${P} }`, call: "g().next().then(r => r.value)" },
  { n: "async-gen-sync", async: true, defs: `async function* g() { yield ${P} }`, call: "g().next().then(r => r.value)" },
  { n: "async-in-all", async: true, defs: `async function a(i) { await 1; return ${P} }`, call: "Promise.all([a(1)]).then(r => r[0])" },
  { n: "then-cb", async: true, defs: "", call: `Promise.resolve().then(function th() { return ${P} })` },
  { n: "then-arrow", async: true, defs: "", call: `Promise.resolve().then(() => ${P})` },
  { n: "catch-cb", async: true, defs: "", call: `Promise.reject(1).catch(function ct() { return ${P} })` },
  { n: "finally-cb", async: true, defs: "", call: `Promise.resolve().finally(function () { globalThis.T = ${P} }).then(() => globalThis.T)` },
  { n: "promise-executor", defs: `var s; new Promise(function exec(res) { s = ${P}; res() })`, call: "s" },
  { n: "queue-microtask", async: true, defs: "", call: `new Promise(res => queueMicrotask(function qm() { res(${P}) }))` },
  { n: "map-cb", defs: "", call: `[1].map(function cb() { return ${P} })[0]` },
  { n: "map-arrow", defs: "", call: `[1].map(() => ${P})[0]` },
  { n: "foreach-cb", defs: "var s;", call: `([1].forEach(function () { s = ${P} }), s)` },
  { n: "filter-cb", defs: "var s;", call: `([1].filter(function flt() { s = ${P}; return true }), s)` },
  { n: "reduce-cb", defs: "", call: `[1, 2].reduce(function red(a, b) { return ${P} })` },
  { n: "find-cb", defs: "var s;", call: `([1].find(function fnd() { s = ${P} }), s)` },
  { n: "some-cb", defs: "var s;", call: `([1].some(function sm() { s = ${P}; return true }), s)` },
  { n: "flatmap-cb", defs: "", call: `[1].flatMap(function fm() { return [${P}] })[0]` },
  { n: "from-mapfn", defs: "", call: `Array.from([1], function mf() { return ${P} })[0]` },
  { n: "sort-cmp", defs: "var s;", call: `([2, 1].sort(function cmp(a, b) { s = ${P}; return a - b }), s)` },
  { n: "reviver", defs: "var s;", call: `(JSON.parse('[1]', function rv(k, v) { s = ${P}; return v }), s)` },
  { n: "replacer", defs: "var s;", call: `(JSON.stringify({ a: 1 }, function rp(k, v) { s = ${P}; return v }), s)` },
  { n: "to-json", defs: "var s;", call: `(JSON.stringify({ toJSON() { s = ${P}; return 1 } }), s)` },
  { n: "string-replace", defs: "var s;", call: `('a'.replace('a', function rep() { s = ${P}; return 'b' }), s)` },
  { n: "regexp-replace", defs: "var s;", call: `('a'.replace(/a/, () => { s = ${P}; return 'b' }), s)` },
  { n: "map-foreach", defs: "var s;", call: `(new Map([[1, 2]]).forEach(function mfe() { s = ${P} }), s)` },
  { n: "set-foreach", defs: "var s;", call: `(new Set([1]).forEach(() => { s = ${P} }), s)` },
  { n: "object-entries-map", defs: "", call: `Object.entries({ a: 1 }).map(([k]) => ${P})[0]` },
  { n: "reflect-construct", defs: `function F() { this.e = ${P} }`, call: "Reflect.construct(F, []).e" },
  { n: "reflect-construct-newtarget", defs: `function F() { this.e = ${P} }\nfunction G() {}`, call: "Reflect.construct(F, [], G).e" },
  { n: "reflect-get-getter", defs: `var o = { get g() { return ${P} } }`, call: "Reflect.get(o, 'g')" },
  { n: "function-ctor-call", defs: "", call: `Function.prototype.call.call(function nm() { return ${P} })` },
  { n: "settimeout-free-microtask", async: true, defs: "", call: `(async () => { await null; await null; return ${P} })()` },
  { n: "symbol-species", defs: `class A extends Array { static get [Symbol.species]() { return function sp() { globalThis.T = ${P}; return [] } } }`, call: "(new A(1).map(x => x), globalThis.T)" },
  { n: "label-block", defs: `function f() { lbl: { return ${P} } }`, call: "f()" },
  { n: "switch", defs: `function f(x) { switch (x) { case 1: return ${P} } }`, call: "f(1)" },
  { n: "for-of", defs: `function f() { for (var i of [1]) { return ${P} } }`, call: "f()" },
  { n: "for-in", defs: `function f() { for (var i in { a: 1 }) { return ${P} } }`, call: "f()" },
  { n: "try-finally", defs: `function f() { try { return ${P} } finally { } }`, call: "f()" },
  { n: "with-default-param", defs: `function f(a = ${P}) { return a }`, call: "f()" },
  { n: "destructure-default", defs: `function f({ a = ${P} } = {}) { return a }`, call: "f()" },
  { n: "arguments-callee-free", defs: `function f() { return (function () { return ${P} }).apply(this, arguments) }`, call: "f(1, 2)" },
  { n: "getter-in-class-chain", defs: `class A { get g() { return ${P} } }\nclass B extends A { get g() { return super.g } }`, call: "new B().g" },
  { n: "super-method", defs: `class A { m() { return ${P} } }\nclass B extends A { m() { return super.m() } }`, call: "new B().m()" },
  { n: "static-super", defs: `class A { static s() { return ${P} } }\nclass B extends A { static s() { return super.s() } }`, call: "B.s()" },
  { n: "object-assign-getter", defs: `var src = { get g() { return ${P} } }`, call: "Object.assign({}, src).g" },
  { n: "spread-getter", defs: `var src = { get g() { return ${P} } }`, call: "({ ...src }).g" },
  { n: "default-ctor-field-order", defs: `class A { x = 1; constructor() { this.s = ${P} } }`, call: "new A().s" },
];

const callerWrappers = [
  { n: "direct", wrap: c => c },
  { n: "depth1", wrap: c => `(function w1() { var r = ${c}; return r })()` },
  { n: "depth2", wrap: c => `(function w2() { var r = (function w1() { var r = ${c}; return r })(); return r })()` },
  { n: "arrow-wrap", wrap: c => `(() => { var r = ${c}; return r })()` },
  { n: "try-wrap", wrap: c => `(function () { try { var r = ${c}; return r } catch (e) { return 'caught' } })()` },
  { n: "method-wrap", wrap: c => `({ run() { var r = ${c}; return r } }).run()` },
];

const limits = ["0", "1", "3", "50", "Infinity"];

const build = (kind, callWrap, preamble) => {
  const assign = kind.async
    ? `Promise.resolve(${callWrap(kind.call)}).then(v => { globalThis.R = v })`
    : `globalThis.R = ${callWrap(kind.call)}`;
  return lines(preamble || "", kind.defs, assign).replace(/^\n+/, "");
};

// Todos os tipos, chamada direta (síncronos e assíncronos).
for (const kind of kinds) priority.push(build(kind, c => c));

// Limites de `Error.stackTraceLimit`, nos 12 primeiros tipos síncronos e dois assíncronos.
const limitKinds = [...kinds.filter(k => !k.async).slice(0, 10), kinds.find(k => k.n === "async-await-first"), kinds.find(k => k.n === "map-cb")];
for (const limit of limits) {
  for (const kind of limitKinds) {
    if (limit === "Infinity" && kind.n !== "named" && kind.n !== "async-await-first") continue;
    priority.push(build(kind, c => `(function w1() { var r = ${c}; return r })()`, `Error.stackTraceLimit = ${limit}`));
  }
}
priority.push("globalThis.R = [Error.stackTraceLimit, Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit').writable].join()");
priority.push(lines("Error.stackTraceLimit = 'x'", "function f() { return new Error('x').stack }", "globalThis.R = f()"));
priority.push(lines("Error.stackTraceLimit = -1", "function f() { return new Error('x').stack }", "globalThis.R = f()"));
priority.push(lines("Error.stackTraceLimit = 2.7", "function f() { return new Error('x').stack }", "function g() { var r = f(); return r }", "globalThis.R = g()"));
priority.push(lines("delete Error.stackTraceLimit", "function f() { return new Error('x').stack }", "globalThis.R = f()"));

// Chamada por call/apply/bind/Reflect.apply e variações, sobre quatro funções.
const callForms = [
  "f()", "f.call(null)", "f.apply(null, [])", "f.bind(null)()", "Reflect.apply(f, null, [])", "[0].map(() => f())[0]", "[0].map(f)[0]",
  "Function.prototype.call.call(f)", "(0, f)()", "new (function () { this.v = f() })().v",
  "Array.from([0], f)[0]",
];
const fnForms = [
  { n: "named", def: `function f() { return ${P} }` },
  { n: "arrow", def: `var f = () => ${P}` },
  { n: "anon", def: `var f = function () { return ${P} }` },
  { n: "method", def: `var f = ({ f() { return ${P} } }).f` },
];
for (const fn of fnForms) for (const form of callForms) priority.push(lines(fn.def, `globalThis.R = ${form}`));

// Error.captureStackTrace(obj, fn).
const capture = [
  lines("function cap() { var o = {}; Error.captureStackTrace(o); return o.stack }", "globalThis.R = cap()"),
  lines("function cap() { var o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); return o.stack }", "globalThis.R = cap()"),
  lines("function inner(o) { Error.captureStackTrace(o, inner) }", "function outer() { var o = {}; inner(o); return o.stack }", "globalThis.R = outer()"),
  lines("function inner(o) { Error.captureStackTrace(o, outer) }", "function outer() { var o = {}; inner(o); return o.stack }", "function top() { return outer() }", "globalThis.R = top()"),
  lines("function inner(o) { Error.captureStackTrace(o, function nope() {}) }", "function outer() { var o = {}; inner(o); return o.stack }", "globalThis.R = outer()"),
  lines("function inner(o) { Error.captureStackTrace(o, 1) }", "function outer() { var o = {}; inner(o); return o.stack }", "globalThis.R = outer()"),
  lines("var o = new Error('orig'); Error.captureStackTrace(o); globalThis.R = o.stack"),
  lines("function f() { var e = new Error('orig'); Error.captureStackTrace(e, f); return e.stack }", "function g() { return f() }", "globalThis.R = g()"),
  lines("class K { constructor() { Error.captureStackTrace(this, K) } }", "function mk() { return new K() }", "globalThis.R = mk().stack"),
  lines("class K { constructor() { Error.captureStackTrace(this, this.constructor) } }", "class L extends K {}", "function mk() { return new L() }", "globalThis.R = mk().stack"),
  lines("class MyErr extends Error { constructor(m) { super(m); Error.captureStackTrace(this, MyErr) } }", "function thrower() { return new MyErr('mine') }", "globalThis.R = thrower().stack"),
  lines("function f() { var o = {}; Error.captureStackTrace(o); return Object.getOwnPropertyDescriptor(o, 'stack') }", "var d = f(); globalThis.R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()"),
  lines("var o = {}; Error.captureStackTrace(o); globalThis.R = Object.keys(o).join() + '|' + Object.getOwnPropertyNames(o).join()"),
  lines("function f() { var o = Object.freeze({}); try { Error.captureStackTrace(o) } catch (e) { return e.name + ': ' + e.message } return 'ok' }", "globalThis.R = f()"),
  lines("try { Error.captureStackTrace() } catch (e) { globalThis.R = e.name + ': ' + e.message }"),
  lines("try { Error.captureStackTrace(1) } catch (e) { globalThis.R = e.name + ': ' + e.message }"),
  lines("Error.stackTraceLimit = 1", "function f() { var o = {}; Error.captureStackTrace(o); return o.stack }", "function g() { return [f()][0] }", "globalThis.R = g()"),
  lines("function f() { var o = {}; Error.captureStackTrace(o); return o.stack }", "globalThis.R = [1].map(function cb() { return f() })[0]"),
  lines("async function f() { await 1; var o = {}; Error.captureStackTrace(o); return o.stack }", "f().then(v => { globalThis.R = v })"),
  lines("function f() { return Error.captureStackTrace({}) }", "globalThis.R = String(f())"),
];
priority.push(...capture);

// prepareStackTrace com CallSite.
const sites = [
  "getFunctionName()", "getFileName()", "getLineNumber()", "getColumnNumber()", "getTypeName()", "getMethodName()", "isNative()",
  "isConstructor()", "isAsync()", "isEval()", "isToplevel()", "getThis()", "toString()", "getEvalOrigin()", "getScriptNameOrSourceURL()",
  "isPromiseAll()", "getPromiseIndex()",
];
const siteCtx = [
  { n: "fn", defs: "function f() { return new Error('x').stack }", call: "f()" },
  { n: "method", defs: "var o = { m() { return new Error('x').stack } }", call: "o.m()" },
  { n: "ctor", defs: "function F() { this.s = new Error('x').stack }", call: "new F().s" },
  { n: "eval", defs: "", call: `eval("new Error('x').stack")` },
  { n: "async", async: true, defs: "async function a() { await 1; return new Error('x').stack }", call: "a()" },
];
for (const site of sites) {
  for (const ctx of siteCtx) {
    const prep = `Error.prepareStackTrace = (e, cs) => cs.map(c => { try { return String(c.${site}) } catch (x) { return 'T:' + x.name } }).join('|')`;
    priority.push(build(ctx, c => c, prep));
  }
}
priority.push(
  lines("Error.prepareStackTrace = (e, cs) => [e.message, cs.length, Array.isArray(cs)].join()", "function f() { return new Error('msg').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => ({ n: cs.length })", "function f() { return new Error('msg').stack }", "globalThis.R = JSON.stringify(f())"),
  lines("Error.prepareStackTrace = () => undefined", "function f() { return new Error('msg').stack }", "globalThis.R = String(f())"),
  lines("Error.prepareStackTrace = () => { throw new RangeError('in prep') }", "function f() { try { return new Error('msg').stack } catch (e) { return e.name + ':' + e.message } }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => cs[0].constructor.name", "function f() { return new Error('msg').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => Object.getOwnPropertyNames(Object.getPrototypeOf(cs[0])).sort().join()", "function f() { return new Error('msg').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => 'once'", "function f() { var e = new Error('m'); return e.stack + e.stack }", "globalThis.R = f()"),
  lines("var n = 0; Error.prepareStackTrace = (e, cs) => 'call' + (++n)", "var e = new Error('m'); var a = e.stack; var b = e.stack; globalThis.R = a + b + n"),
  lines("Error.prepareStackTrace = (e, cs) => 'cap'", "var o = {}; Error.captureStackTrace(o); globalThis.R = o.stack"),
  lines("Error.prepareStackTrace = (e, cs) => typeof e", "var o = {}; Error.captureStackTrace(o); globalThis.R = o.stack"),
  lines("Error.prepareStackTrace = (e, cs) => String(this === undefined)", "globalThis.R = new Error('m').stack"),
  lines("Error.prepareStackTrace = function (e, cs) { return typeof this }", "globalThis.R = new Error('m').stack"),
  lines("Error.prepareStackTrace = 5", "function f() { return new Error('m').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = null", "function f() { return new Error('m').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getLineNumber()).join()", "Error.stackTraceLimit = 1", "function f() { return new Error('m').stack }", "globalThis.R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFunctionName()).join()", "class K { static { globalThis.R = new Error('x').stack } }"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFunctionName()).join()", "var o = { get p() { return new Error('x').stack } }", "globalThis.R = o.p"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFunctionName()).join()", "class K { get p() { return new Error('x').stack } }", "globalThis.R = new K().p"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getTypeName()).join()", "class K { m() { return new Error('x').stack } }", "class L extends K {}", "globalThis.R = new L().m()"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getMethodName()).join()", "var o = { a() { return this.b() }, b() { return new Error('x').stack } }", "globalThis.R = o.a()"),
);

// Erro com cause e AggregateError.
const causes = [
  "var e = new Error('outer', { cause: new Error('inner') }); globalThis.R = e.stack + '|' + e.cause.stack",
  "var e = new Error('outer', { cause: 'str' }); globalThis.R = e.stack + '|' + e.cause",
  "var e = new Error('outer', { cause: undefined }); globalThis.R = e.stack + '|' + ('cause' in e)",
  "var e = new Error('outer', {}); globalThis.R = e.stack + '|' + ('cause' in e)",
  "function mk() { return new Error('c') } function wrap() { try { mk() } catch (x) {} return new Error('w', { cause: mk() }) } globalThis.R = wrap().stack + '|' + wrap().cause.stack",
  "var e = new AggregateError([new Error('a'), new Error('b')], 'agg'); globalThis.R = e.stack + '|' + e.errors.length",
  "var e = new AggregateError([], 'agg', { cause: new Error('c') }); globalThis.R = e.stack + '|' + e.cause.message",
  "function f() { return new AggregateError([1], 'agg').stack } globalThis.R = f()",
  "function f() { return new AggregateError([1]).stack } globalThis.R = f()",
  "var e = new AggregateError([new Error('in')], 'agg'); globalThis.R = e.errors[0].stack",
  "Promise.any([Promise.reject(new Error('r'))]).catch(e => { globalThis.R = e.name + '|' + e.stack })",
  "async function f() { await 1; try { await Promise.any([Promise.reject(1)]) } catch (e) { return e.stack } } f().then(v => { globalThis.R = v })",
  "class MyAgg extends AggregateError {} globalThis.R = new MyAgg([1], 'm').stack",
  "var e = new TypeError('t', { cause: new RangeError('r') }); globalThis.R = e.stack + '|' + e.cause.stack",
  "var e = new Error('m'); e.cause = new Error('late'); globalThis.R = e.stack",
  "try { null.x } catch (e) { var w = new Error('wrapped', { cause: e }); globalThis.R = w.stack + '|' + w.cause.stack }",
  "function a() { throw new Error('a') } function b() { try { a() } catch (e) { throw new Error('b', { cause: e }) } } try { b() } catch (e) { globalThis.R = e.stack + '|' + e.cause.stack }",
  "var e = new Error('m', { cause: 1 }); globalThis.R = Object.getOwnPropertyDescriptor(e, 'cause').enumerable + '|' + Object.keys(e).join()",
  "var e = new AggregateError([1, 2], 'm'); globalThis.R = Object.getOwnPropertyNames(e).sort().join()",
  "var e = new Error('m'); globalThis.R = Object.getOwnPropertyNames(e).sort().join() + '|' + Object.getOwnPropertyDescriptor(e, 'stack').writable",
  "var e = new Error('m'); e.stack = 'custom'; globalThis.R = e.stack",
  "var e = new Error('m'); e.message = 'changed'; globalThis.R = e.stack",
  "var e = new Error('m'); e.name = 'Renamed'; globalThis.R = e.stack + '|' + e.toString()",
  "var e = new Error(); e.message = 'late'; globalThis.R = e.stack",
  "var e = new Error('m'); delete e.stack; globalThis.R = String(e.stack)",
  "var e = new Error('m'); globalThis.R = typeof Object.getOwnPropertyDescriptor(e, 'stack').get + '|' + typeof Object.getOwnPropertyDescriptor(e, 'stack').value",
  "var e = Object.create(Error.prototype); globalThis.R = String(e.stack)",
  "class E1 extends Error { constructor(m) { super(m); this.name = 'E1' } } function f() { return new E1('boom') } globalThis.R = f().stack",
  "class E1 extends Error { get name() { return 'Getter' } } globalThis.R = new E1('boom').stack",
  "class E1 extends Error {} E1.prototype.name = 'Proto'; globalThis.R = new E1('boom').stack",
  "var e = new Error('m'); Object.defineProperty(e, 'message', { value: 'redef' }); globalThis.R = e.stack",
  "var e = new RangeError('r'); globalThis.R = e.stack.split('\\n')[0]",
  "var e = new EvalError('r'); globalThis.R = e.stack.split('\\n')[0]",
  "var e = new URIError('r'); globalThis.R = e.stack.split('\\n')[0]",
  "var e = new ReferenceError('r'); globalThis.R = e.stack.split('\\n')[0]",
];
for (const body of causes) priority.push(body);

// Linhas e colunas de erros lançados em expressões de várias linhas.
const multiline = [
  lines("function f() { return new Error('x').stack }", "globalThis.R = f(1,", "  f(2,", "    3))"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = [", "  1,", "  f(),", "  3", "][2]"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = {", "  a: 1,", "  b: f()", "}.b"),
  lines("function f() { return new Error('x').stack }", "globalThis.R =", "  f()"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = 1 +", "  f()", "  + 2"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = true", "  ? f()", "  : 0"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = `a", "b${", "f()", "}c`"),
  lines("function f() { return new Error('x').stack }", "var o = { m: f }", "globalThis.R = o", "  .m", "  ()"),
  lines("function f() { return new Error('x').stack }", "var o = { m: f }", "globalThis.R = o.m(", ")"),
  lines("function f() { return new Error('x').stack }", "var o = { m: f }", "globalThis.R = o", "  ['m']()"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = new (", "  function () { this.s = f() }", ")().s"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = [1]", "  .map(", "    x => f()", "  )[0]"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = [1]", "  .map(x =>", "    f())[0]"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = (", "  1,", "  f()", ")"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = f(", "  ...[1]", ")"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = f", "  `a`"),
  lines("function f() { return new Error('x').stack }", "globalThis.R = f?.", "  ()"),
  lines("function f() { return new Error('x').stack }", "var o = { f }", "globalThis.R = o?.", "  f()"),
  lines("function f() { return new Error('x').stack }", "var a = 1,", "  b = f(),", "  c = 3", "globalThis.R = b"),
  lines("function f() {", "  return new Error(", "    'x'", "  ).stack", "}", "globalThis.R = f()"),
  lines("function f() {", "  return new", "    Error('x').stack", "}", "globalThis.R = f()"),
  lines("function f() {", "  var e =", "    new Error('x')", "  return e.stack", "}", "globalThis.R = f()"),
  lines("function f() {", "  return Error(", "  'x').stack", "}", "globalThis.R = f()"),
  lines("try {", "  null", "    .x", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  var o = {}", "  o", "    .a", "    .b", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  var o = {}", "  o.a(", "    1)", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  var o = {}", "  o", "    .a()", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  undefinedFn(", "    1,", "    2)", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  new (", "    undefined)()", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  var x = 1;", "  x()", "} catch (e) { globalThis.R = e.stack }"),
  lines("try {", "  throw", "    new Error('multi')", "} catch (e) { globalThis.R = e.stack }"),
  lines("function t() {", "  throw new", "    Error('t')", "}", "try { t() } catch (e) { globalThis.R = e.stack }"),
  lines("function t() {", "  throw (", "    new Error('t'))", "}", "try { t() } catch (e) { globalThis.R = e.stack }"),
  lines("function a() { return b() }", "function b() {", "  return c()", "}", "function c() { return new Error('x').stack }", "globalThis.R = a()"),
  lines("function a() { var r = b(); return r }", "function b() { var r = c(); return r }", "function c() { return new Error('x').stack }", "globalThis.R = a()"),
  lines("function c() { return new Error('x').stack }", "class K {", "  m() {", "    var r =", "      c()", "    return r", "  }", "}", "globalThis.R = new K().m()"),
  lines("function c() { return new Error('x').stack }", "var o = {", "  get g() {", "    var r = c()", "    return r", "  }", "}", "globalThis.R = o.g"),
  lines("function c() { return new Error('x').stack }", "\tvar r = c()", "globalThis.R = r"),
  lines("function c() { return new Error('x').stack }", "var s = '\\u00e9\\u00e9'; globalThis.R = c()"),
  lines("function c() { return new Error('x').stack }", "var s = '日本語'; globalThis.R = c()"),
  lines("function c() { return new Error('x').stack }", "var s = '😀'; globalThis.R = c()"),
  lines("function c() { return new Error('x').stack }", "/* c1 */ /* c2\n */ globalThis.R = c()"),
  lines("function c() { return new Error('x').stack }", "// comentário", "globalThis.R = c() // fim"),
  lines("function c() { return new Error('x').stack }", "var a = [c(), c()]", "globalThis.R = a[1]"),
  lines("function c() { return new Error('x').stack }", "var a = { x: c(), y: c() }", "globalThis.R = a.y"),
  lines("function c() { return new Error('x').stack }", "globalThis.R = c() + c()"),
  lines("function c() { return new Error('x').stack }", "globalThis.R = (c(), c())"),
  lines("function c() { return new Error('x').stack }", "globalThis.R = c()\r\n"),
  lines("function c() { return new Error('x').stack }\r\nglobalThis.R =\r\n  c()"),
];
priority.push(...multiline);

// SyntaxError de eval, new Function e afins.
const badSources = [
  "var = 1", "1 +", "function (", "if (", "}", "{", "let let = 1", "var a; let a", "return 1", "break", "continue", "await 1", "yield 1",
  "class { }", "new.target", "super()", "a => { ", "x = {a:1,,}", "'unterminated", "`unterminated", "/unterminated", "1..a.", "0b2", "1_", "@",
  "for (;;", "a ?? b || c", "async () => await", "import x from 'y'", "export default 1", "\\u{110000}", "var \\u0061wait", "'use strict'; with (a) {}",
  "'use strict'; var eval", "'use strict'; 010", "function f(a, a) { 'use strict' }", "({ a: 1, a: 2, __proto__: 1, __proto__: 2 })", "x\n++\n",
  "1\n2 3", "a b", "var a = ;", "do ;", "label: label: ;", "throw\n1", "[...a, b] = c", "({a}) = 1", "for (let of x);", "delete x", "typeof",
];
for (const bad of badSources) {
  const q = JSON.stringify(bad);
  priority.push(`try { eval(${q}) } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }`);
}
for (const bad of badSources.slice(0, 12)) {
  const q = JSON.stringify(bad);
  priority.push(`try { new Function(${q}) } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }`);
  priority.push(`function f() { try { (0, eval)(${q}) } catch (e) { return e.stack } }\nglobalThis.R = f()`);
}
priority.push(
  lines("function f() { try { eval('var = 1') } catch (e) { return e.stack } }", "function g() { return f() }", "globalThis.R = g()"),
  lines("try { eval('\\n\\n  var = 1') } catch (e) { globalThis.R = e.stack }"),
  lines("try { new Function('a', 'b', 'return +') } catch (e) { globalThis.R = e.stack }"),
  lines("try { JSON.parse('{bad') } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }"),
  lines("try { JSON.parse('') } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }"),
  lines("function f() { return JSON.parse('[1,]') }", "try { f() } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }"),
  lines("try { new RegExp('(') } catch (e) { globalThis.R = e.name + ': ' + e.message + '\\n' + e.stack }"),
  lines("try { eval('(function(){ return new Error(\"in\").stack })()') ; globalThis.R = eval('(function inEval(){ return new Error(\"in\").stack })()') } catch (e) { globalThis.R = e.stack }"),
  lines("globalThis.R = eval('function ev() { return new Error(\"in\").stack }; ev()')"),
  lines("globalThis.R = eval('var f = () => new Error(\"in\").stack; f()')"),
  lines("globalThis.R = eval('eval(\"new Error(1).stack\")')"),
  lines("function f() { return eval('eval(\"new Error(1).stack\")') }", "globalThis.R = f()"),
  lines("globalThis.R = eval('//# sourceURL=named.js\\nnew Error(1).stack')"),
  lines("globalThis.R = new Function('//# sourceURL=named2.js\\nreturn new Error(1).stack')()"),
  lines("globalThis.R = eval('\\n\\n\\nnew Error(1).stack')"),
  lines("globalThis.R = eval('1;\\nnew Error(1).stack')"),
);

// Combinatória grande: todo tipo (síncrono) por todos os envoltórios, e cada tipo assíncrono por alguns.
for (const wrapper of callerWrappers.slice(1)) {
  for (const kind of kinds) {
    if (kind.async && wrapper.n !== "depth1" && wrapper.n !== "arrow-wrap") continue;
    bulk.push(build(kind, wrapper.wrap));
  }
}

const seen = new Set();
const programs = [];
for (const body of [...priority, ...bulk]) {
  if (seen.has(body)) continue;
  seen.add(body);
  programs.push(body);
  if (programs.length === TARGET) break;
}
if (programs.length < TARGET) throw new Error("só " + programs.length + " programas distintos");
const droppedFromPriority = Math.max(0, priority.filter((b, i) => priority.indexOf(b) === i).length - TARGET);
if (droppedFromPriority > 0) throw new Error("a lista prioritária passa de " + TARGET + ": " + droppedFromPriority + " cortados");

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stack-more-golden-"));
const file = path.join(dir, "file.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
// O bun transpila o arquivo antes do JSC: o golden grava o texto canônico (`prepareProgram`) e o bun executa `executable`,
// de modo que as posições do stack saem do fonte original.
const rows = [];
for (const body of programs) {
  const original = '"use strict";\n' + body;
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) throw new Error("sem resultado para: " + original + "\n" + run.stderr);
  const stack = JSON.parse(marked.slice(1)).split("file://" + prefix).join("").split(prefix).join("");
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(stack) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stdout.write(emitFactoredLines("stack_more", rows));
fs.rmSync(dir, { recursive: true, force: true });
