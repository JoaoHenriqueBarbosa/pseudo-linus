// Gera tests/golden/stack_format_bun.tsv: formato de `Error.prototype.stack` e de Error.captureStackTrace em
// cenários que o error_stack_bun.tsv não cobre (métodos de classe, estáticos, accessors, async, geradores,
// eval e new Function aninhados, frames nativos, stackTraceLimit com valores estranhos, prepareStackTrace com
// CallSite por contexto, cause, AggregateError, constructorOpt, async stack traces e nomes inferidos).
// Roda cada programa com `vm.runInThisContext` (filename `error_stack_case.js`), sem source map; resultado com
// caminho da máquina é descartado. Colunas: fonte (JSON) e valor de `R` (JSON).
// Uso: bun scripts/gen-stack-format-golden.js > tests/golden/stack_format_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const STACK = "new Error('x').stack";

// ---- 1. Frames por construção.
const shapes = [
  `class K { m() { return ${STACK} } }; R = new K().m()`,
  `class K { static s() { return ${STACK} } }; R = K.s()`,
  `class K { get g() { return ${STACK} } }; R = new K().g`,
  `class K { set g(v) { R = ${STACK} } }; new K().g = 1`,
  `class K { static get g() { return ${STACK} } }; R = K.g`,
  `class K { static set g(v) { R = ${STACK} } }; K.g = 1`,
  `class K { constructor() { this.s = ${STACK} } }; R = new K().s`,
  `class A { constructor() { this.s = ${STACK} } }; class B extends A {}; R = new B().s`,
  `class A { constructor() { this.s = ${STACK} } }; class B extends A { constructor() { super() } }; R = new B().s`,
  `class A { m() { return ${STACK} } }; class B extends A { m() { return super.m() } }; R = new B().m()`,
  `class K { static m() { return ${STACK} } }; class L extends K {}; R = L.m()`,
  `class K { #p() { return ${STACK} } static c(o) { return o.#p() } }; R = K.c(new K)`,
  `class K { static #p() { return ${STACK} } static c() { return K.#p() } }; R = K.c()`,
  `class K { f = () => ${STACK} }; R = new K().f()`,
  `class K { static f = () => ${STACK} }; R = K.f()`,
  `class K { ['co' + 'mp']() { return ${STACK} } }; R = new K().comp()`,
  `class K { static async m() { await 0; return ${STACK} } }; K.m().then(v => { R = v })`,
  `class K { async m() { return ${STACK} } }; new K().m().then(v => { R = v })`,
  `class K { *g() { yield ${STACK} } }; R = new K().g().next().value`,
  `class K { static *g() { yield ${STACK} } }; R = K.g().next().value`,
  `class K { async *g() { yield ${STACK} } }; new K().g().next().then(v => { R = v.value })`,
  `var K = class { m() { return ${STACK} } }; R = new K().m()`,
  `var K = class Named { m() { return ${STACK} } }; R = new K().m()`,
  `R = new (class { m() { return ${STACK} } })().m()`,
  `function* g() { yield ${STACK} }; var i = g(); R = i.next().value`,
  `function* g() { var r = yield 1; yield ${STACK} }; var i = g(); i.next(); R = i.next(2).value`,
  `function* g() { yield* h() }; function* h() { yield ${STACK} }; R = g().next().value`,
  `function* g() { yield 1 }; var i = g(); R = (function () { return ${STACK} }).call(i)`,
  `var o = { *g() { yield ${STACK} } }; R = o.g().next().value`,
  `var o = { async m() { return ${STACK} } }; o.m().then(v => { R = v })`,
  `var o = { get x() { return ${STACK} } }; R = o.x`,
  `var o = { set x(v) { R = ${STACK} } }; o.x = 1`,
  `var o = { f() { return eval("${STACK}") } }; R = o.f()`,
  `function f() { return eval("${STACK}") }; R = f()`,
  `R = eval("(function g() { return ${STACK} })()")`,
  `R = eval("eval(\\"${STACK}\\")")`,
  `function f() { return eval("(0, eval)(\\"${STACK}\\")") }; R = f()`,
  `function f() { return new Function("return ${STACK}")() }; R = f()`,
  `R = new Function("return eval(\\"${STACK}\\")")()`,
  `R = eval("new Function(\\"return ${STACK}\\")()")`,
  `var F = new Function("a", "return ${STACK}"); R = F.call(null, 1)`,
  `var F = Function("return ${STACK}"); R = new F().constructor === Object ? 'obj' : 'x'`,
  `R = [1].map(function () { return ${STACK} })[0]`,
  `R = [1].map(() => ${STACK})[0]`,
  `R = [[1]].flatMap(function inner(x) { return [${STACK}] })[0]`,
  `R = Array.from([1], function (x) { return ${STACK} })[0]`,
  `R = [3, 1].sort(function (a, b) { R = ${STACK}; return a - b }) && R`,
  `R = 'a'.replace('a', function () { return ${STACK} })`,
  `R = JSON.parse('[1]', function (k, v) { R = ${STACK}; return v }) && R`,
  `R = JSON.parse(JSON.stringify({ toJSON() { return ${STACK} } }))`,
  `new Promise(function (res) { res(${STACK}) }).then(v => { R = v })`,
  `new Promise((res) => res(${STACK})).then(v => { R = v })`,
  `R = new Map([[1, 2]]); R.forEach(function () { R = ${STACK} })`,
  `R = Reflect.construct(function F() { this.s = ${STACK} }, []).s`,
  `R = Reflect.construct(class K { constructor() { this.s = ${STACK} } }, [], Object).s`,
  `function F() { this.s = ${STACK} }; R = Reflect.construct(F, [], class Z {}).s`,
  `R = new (function () { this.s = ${STACK} })().s`,
  `R = new (function Foo() { return { s: ${STACK} } })().s`,
  `var o = { f: function () { return ${STACK} } }; var g = o.f; R = g()`,
  `var o = { f: function () { return ${STACK} } }; R = o.f.call(1)`,
  `var o = { f: function () { return ${STACK} } }; R = o['f']()`,
  `var o = { 'a-b': function () { return ${STACK} } }; R = o['a-b']()`,
  `var o = { 1: function () { return ${STACK} } }; R = o[1]()`,
  `var o = { [Symbol.iterator]() { return ${STACK} } }; R = o[Symbol.iterator]()`,
  `var o = { [Symbol('d')]() { return ${STACK} } }; R = o[Object.getOwnPropertySymbols(o)[0]]()`,
  `var p = new Proxy({ m() { return ${STACK} } }, {}); R = p.m()`,
  `var p = new Proxy(function f() { return ${STACK} }, {}); R = p()`,
  `var p = new Proxy({}, { get() { return ${STACK} } }); R = p.q`,
  `var p = new Proxy({}, { has() { R = ${STACK}; return true } }); 'x' in p`,
  `var o = { toString() { return ${STACK} } }; R = '' + o`,
  `var o = { valueOf() { return ${STACK}.length } }; R = +o > 0 ? 'ok' : 'no'`,
  `var o = { [Symbol.toPrimitive]() { R = ${STACK}; return 1 } }; +o`,
  `var s = Symbol('d'); var o = { get [s]() { return ${STACK} } }; R = o[s]`,
  `function f() { return ${STACK} }; R = f.bind(null).call()`,
  `function f() { return ${STACK} }; var g = f.bind(null).bind(null); R = g()`,
  `function f() { return ${STACK} }; R = new (f.bind(null))`,
  `function f() { return ${STACK} }; R = [f].map(g => g())[0]`,
  `function f() { return ${STACK} }; R = Function.prototype.call.call(f)`,
  `function f() { return ${STACK} }; R = Reflect.apply(Function.prototype.apply, f, [null])`,
  `var f = function () { return ${STACK} }; R = f()`,
  `var f; f = function () { return ${STACK} }; R = f()`,
  `let f = () => ${STACK}; R = f()`,
  `const f = async () => ${STACK}; f().then(v => { R = v })`,
  `label: { R = ${STACK} }`,
  `with ({}) { R = ${STACK} }`,
  `try { throw 1 } catch (e) { R = ${STACK} }`,
  `try { R = ${STACK} } finally { }`,
  `for (var i = 0; i < 1; i++) { R = ${STACK} }`,
  `var t = (s) => s; R = t\`${"${"}${STACK}}\``,
  `R = (function () { 'use strict'; return ${STACK} }).call(undefined)`,
  `R = (function () { return (function () { return (function () { return ${STACK} })() })() })()`,
];
for (const body of shapes) programs.push(body);

// ---- 2. Frames nativos e novos, em cada construção de chamada nativa.
const nativeCalls = [
  "[1].map(cb)", "[1].forEach(cb)", "[1].filter(cb)", "[1].reduce(cb, 0)", "[1].find(cb)", "[1].findLast(cb)",
  "[1].findIndex(cb)", "[1].some(cb)", "[1].every(cb)", "[1].flatMap(cb)", "[2, 1].toSorted(cb)", "Array.from([1], cb)",
  "new Set([1]).forEach(cb)", "new Map([[1, 1]]).forEach(cb)", "new Promise(cb)", "Promise.resolve().then(cb)",
  "'a'.replace(/a/g, cb)", "new Uint8Array(1).map(cb)", "Object.groupBy([1], cb)", "cb.call(null)",
  "Reflect.apply(cb, null, [])",
];
const nativeKinds = [
  "function cb() { throw new Error('x') }; try { CALL } catch (e) { R = e.stack }",
  "function cb() { R = new Error('x').stack }; CALL",
  "var cb = () => { R = new Error('x').stack }; CALL",
];
for (const kind of nativeKinds) {
  for (const call of nativeCalls) programs.push(kind.replace("CALL", call));
}
programs.push(
  "try { new Promise(1) } catch (e) { R = e.stack }",
  "try { Promise.resolve.call(1) } catch (e) { R = e.stack }",
  "try { [].reduce(function () {}) } catch (e) { R = e.stack }",
  "try { new Array(-1) } catch (e) { R = e.stack }",
  "try { 'a'.repeat(-1) } catch (e) { R = e.stack }",
  "try { Object.defineProperty(1, 'a', {}) } catch (e) { R = e.stack }",
  "try { Symbol() + '' } catch (e) { R = e.stack }",
  "try { new Symbol() } catch (e) { R = e.stack }",
  "try { JSON.parse('{') } catch (e) { R = e.stack }",
  "try { new Intl.NumberFormat('xx-invalid-') } catch (e) { R = e.stack.split('\\n')[0] }",
  "try { BigInt(1.5) } catch (e) { R = e.stack }",
  "try { new WeakMap().set(1, 1) } catch (e) { R = e.stack }",
  "try { Reflect.construct(1) } catch (e) { R = e.stack }",
  "try { new Proxy(1, {}) } catch (e) { R = e.stack }",
  "try { Function.prototype.toString.call(1) } catch (e) { R = e.stack }",
  "try { decodeURIComponent('%') } catch (e) { R = e.stack }",
  "try { new (class A { constructor() { null.x } }) } catch (e) { R = e.stack }",
  "try { [].at.call(null) } catch (e) { R = e.stack }",
  "try { Array.prototype.map.call(null, () => {}) } catch (e) { R = e.stack }",
);

// ---- 3. Error.stackTraceLimit com valores estranhos, em quatro sondas.
const limits = ["0", "1", "2", "4", "Infinity", "-1", "NaN", "'2'", "'x'", "null", "undefined", "true", "{}", "[3]", "1.9", "-0", "2 ** 32"];
const probes = [
  "function rec(n) { if (n === 0) return new Error('x'); return rec(n - 1) }; Error.stackTraceLimit = LIMIT; var e = rec(8); R = 'stack' in e ? e.stack.split('\\n').length : 'nostack'",
  "function rec(n) { var o = {}; if (n === 0) { Error.captureStackTrace(o); return o }; return rec(n - 1) }; Error.stackTraceLimit = LIMIT; R = String(rec(8).stack).split('\\n').length",
  "class K { m(n) { if (n === 0) return new Error('x'); return this.m(n - 1) } }; Error.stackTraceLimit = LIMIT; R = String(new K().m(6).stack).split('\\n').slice(1, 3).join('|')",
  "async function rec(n) { if (n === 0) { await 0; return new Error('x') } return rec(n - 1) }; Error.stackTraceLimit = LIMIT; rec(4).then(e => { R = 'stack' in e ? e.stack.split('\\n').length : 'nostack' })",
];
for (const limit of limits) {
  for (const [index, probe] of probes.entries()) {
    // Com limite 4 a recursão de cauda (`return rec(n - 1)` colapsa em modo estrito) deixa só `rec` e o código global
    // do programa; as demais linhas do bun seriam `runInThisContext` e o runner, que não existem fora do harness.
    if (limit === "4" && index < 2) continue;
    programs.push(probe.replace(/LIMIT/g, limit));
  }
}
programs.push(
  "Error.stackTraceLimit = 0; R = JSON.stringify(Object.getOwnPropertyNames(new Error('x')))",
  "Error.stackTraceLimit = 0; R = JSON.stringify(new Error('x').stack)",
  "Error.stackTraceLimit = 1; function f() { return new Error('x').stack }; R = f()",
);

// ---- 4. prepareStackTrace com CallSite, por contexto de definição.
const csContexts = {
  class_method: "new (class K { m() { return CAPTURE } })().m()",
  static_method: "(class K { static s() { return CAPTURE } }).s()",
  class_getter: "new (class K { get g() { return CAPTURE } })().g",
  class_setter: "(function () { var o = new (class K { set g(v) { this.r = CAPTURE } })(); o.g = 1; return o.r })()",
  ctor_new: "new (class K { constructor() { this.r = CAPTURE } })().r",
  derived_ctor: "new (class B extends (class A {}) { constructor() { super(); this.r = CAPTURE } })().r",
  fn_ctor: "new (function Foo() { this.r = CAPTURE })().r",
  eval_in_fn: "(function f() { return eval('CAPTURE') })()",
  new_function: "new Function('return CAPTURE')()",
  arrow_in_method: "({ m() { return (() => CAPTURE)() } }).m()",
  generator: "(function* g() { yield CAPTURE })().next().value",
  map_callback: "[1].map(function cb() { return CAPTURE })[0]",
  computed: "({ ['c' + 'k']() { return CAPTURE } }).ck()",
  toplevel: "CAPTURE",
};
const csMethods = ["getFunctionName", "getTypeName", "getMethodName", "getLineNumber", "getColumnNumber", "isNative", "isEval", "isConstructor", "isToplevel", "toString"];
const capture = "(function () { var h = Error.prepareStackTrace; var out; Error.prepareStackTrace = function (e, cs) { out = cs; return 'p' }; var e = new Error('x'); e.stack; Error.prepareStackTrace = h; var c = out[0]; return String(c.METHOD()) })()";
for (const context of Object.values(csContexts)) {
  for (const method of csMethods) {
    const expr = context.replace("CAPTURE", capture.replace("METHOD", method));
    programs.push(`try { R = ${expr} } catch (e) { R = 'threw:' + e.constructor.name + ':' + e.message }`);
  }
}
programs.push(
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; class K { static s() { return new Error('x').stack } }; R = K.s()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; class K { constructor() { this.s = new Error('x').stack } }; R = new K().s",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; async function f() { await 0; return new Error('x').stack }; f().then(v => { R = v })",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isAsync()).join('|') }; async function g() { await 0; return new Error('x').stack }; async function f() { return await g() }; f().then(v => { R = v })",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; function* g() { yield new Error('x').stack }; R = g().next().value",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = new Function('return new Error(\"x\").stack')()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = new Promise(function (r) { r(new Error('x').stack) }) && 'p'",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = [1].map(() => new Error('x').stack)[0]",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; R = [1].map(function cb() { return new Error('x').stack })[0]",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getTypeName()).join('|') }; class K { m() { return new Error('x').stack } }; R = new K().m()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getMethodName()).join('|') }; class K { m() { return new Error('x').stack } }; R = new K().m()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isConstructor()).join('|') }; class K { constructor() { this.s = new Error('x').stack } }; R = new K().s",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isEval()).join('|') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getEvalOrigin()).join('|') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isNative()).join('|') }; R = [1].map(() => new Error('x').stack)[0]",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber()).join('|') }; class K {\n  m() {\n    return new Error('x').stack\n  }\n}\nR = new K().m()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber()).join('|') }; var o = {\n  f() { return new Error('x').stack }\n}\nR = o\n  .f()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber()).join('|') }; R = eval('\\n\\n  new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber()).join('|') }; R = new Function('\\n return new Error(\"x\").stack')()",
);

// ---- 5. captureStackTrace com constructorOpt.
programs.push(
  "var o = {}; class K { m() { Error.captureStackTrace(o, K.prototype.m) } }; function g() { new K().m() }; g(); R = o.stack",
  "var o = {}; class K { static s() { Error.captureStackTrace(o, K.s) } }; function g() { K.s() }; g(); R = o.stack",
  "var o = {}; class K { constructor() { Error.captureStackTrace(o, K) } }; function g() { new K() }; g(); R = o.stack",
  "var o = {}; class K { constructor() { Error.captureStackTrace(o, new.target) } }; class L extends K {}; function g() { new L() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, g) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, h) }; function g() { f() }; function h() { g() }; function top() { h() }; top(); R = o.stack",
  "var o = {}; var f = function () { Error.captureStackTrace(o, f) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; var f = () => { Error.captureStackTrace(o, f) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; var b = f.bind(null); function g() { b() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, g) }; function g() { [1].forEach(f) }; function h() { g() }; h(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, Array.prototype.forEach) }; function g() { [1].forEach(f) }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, Error) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, Error.captureStackTrace) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, eval) }; function g() { eval('f()') }; g(); R = o.stack",
  "var o = {}; async function f() { await 0; Error.captureStackTrace(o, f) }; async function g() { await f() }; g().then(() => { R = o.stack })",
  "var o = {}; function* f() { Error.captureStackTrace(o, f); yield 1 }; function g() { f().next() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; function g() { try { f() } finally { } }; g(); R = o.stack",
  "class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, E) } }; function g() { throw new E('m') }; try { g() } catch (e) { R = e.stack }",
  "class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, this.constructor) } }; class F extends E {}; function g() { throw new F('m') }; try { g() } catch (e) { R = e.stack }",
  "function MyError(m) { this.message = m; Error.captureStackTrace(this, MyError) }; MyError.prototype = Object.create(Error.prototype); MyError.prototype.name = 'MyError'; function g() { return new MyError('m') }; R = g().stack",
  "function MyError(m) { this.message = m; Error.captureStackTrace(this) }; MyError.prototype = Object.create(Error.prototype); function g() { return new MyError('m') }; R = g().stack",
  "var o = { name: 'Custom', message: 'msg' }; function f() { Error.captureStackTrace(o, f) }; function g() { f() }; g(); R = o.stack",
  "var o = { name: 'Custom' }; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "var o = { message: 'only' }; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "var o = {}; Error.stackTraceLimit = 1; function f() { Error.captureStackTrace(o, f) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; Error.stackTraceLimit = 2; function f() { Error.captureStackTrace(o) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; function f() { Error.captureStackTrace(o, f) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; Error.prepareStackTrace = function (e, cs) { return cs.length }; function f() { Error.captureStackTrace(o) }; f(); R = o.stack",
);

// ---- 6. cause, AggregateError, SuppressedError.
programs.push(
  "function f() { return new Error('in', { cause: new TypeError('c') }) }; var e = f(); R = e.stack + '\\n||' + e.cause.stack",
  "var e = new Error('o', { cause: new Error('c') }); R = String(e.cause.stack.split('\\n')[0]) + '|' + Object.getOwnPropertyNames(e).join()",
  "class E extends Error { constructor(m, o) { super(m, o) } }; function f() { return new E('m', { cause: 'k' }) }; var e = f(); R = e.stack.split('\\n')[0] + '|' + e.cause",
  "var e = new Error('o', { cause: { a: 1 } }); R = JSON.stringify(e.cause) + e.stack.split('\\n').length",
  "var c = new Error('c'); var e = new Error('o', { cause: c }); R = e.stack !== c.stack",
  "function f() { try { null.p } catch (c) { throw new Error('wrapped', { cause: c }) } }; try { f() } catch (e) { R = e.stack + '\\n||' + e.cause.stack }",
  "function f() { try { null.p } catch (c) { var e = new Error('wrapped'); e.cause = c; throw e } }; try { f() } catch (e) { R = e.stack }",
  "var e = new AggregateError([new Error('a')], 'agg', { cause: new Error('c') }); R = e.stack.split('\\n')[0] + '|' + e.cause.message + '|' + Object.getOwnPropertyNames(e).join()",
  "function f() { return new AggregateError([], 'm') }; function g() { return f() }; R = g().stack",
  "class A extends AggregateError { }; function f() { return new A([], 'm') }; R = f().stack",
  "class A extends AggregateError { constructor() { super([], 'm'); this.name = 'A' } }; R = new A().stack.split('\\n')[0]",
  "var e = new AggregateError([1, 2], ''); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new AggregateError([1, 2]); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new AggregateError([new Error('a'), new Error('b')], 'm'); R = e.errors.map(x => x.stack.split('\\n')[0]).join('|')",
  "function f() { return new Error('a') }; var e = new AggregateError([f()], 'm'); R = e.errors[0].stack.split('\\n')[1]",
  "Promise.any([Promise.reject(new Error('a')), Promise.reject(new Error('b'))]).catch(e => { R = e.stack.split('\\n')[0] + '|' + e.errors.map(x => x.message).join() })",
  "Promise.any([Promise.reject(1)]).catch(e => { R = e.stack })",
  "async function f() { await Promise.any([]) }; f().catch(e => { R = e.stack })",
  "var e = new SuppressedError(new Error('a'), new Error('b'), 'm'); R = e.stack.split('\\n')[0] + '|' + e.error.message + '|' + e.suppressed.message",
  "function f() { return new SuppressedError(1, 2, 'm') }; R = f().stack",
  "R = Object.prototype.toString.call(new AggregateError([]))",
  "var e = new Error('m', { cause: 1 }); R = e.stack.split('\\n')[0] + '|' + Object.keys(e).length",
  "var e = new RangeError('m', { cause: new Error('c') }); R = e.stack.split('\\n')[0] + e.cause.message",
);

// ---- 7. Async stack traces (throw depois de await, encadeado, em combinadores).
programs.push(
  "async function c() { await 0; throw new Error('x') }; async function b() { await c() }; async function a() { await b() }; a().catch(e => { R = e.stack })",
  "async function c() { await 0; throw new Error('x') }; async function b() { return c() }; async function a() { return await b() }; a().catch(e => { R = e.stack })",
  "async function c() { await 0; return new Error('x').stack }; async function b() { const v = await c(); return v }; async function a() { const v = await b(); return v }; a().then(v => { R = v })",
  "class K { async c() { await 0; return new Error('x').stack } async b() { return await this.c() } }; new K().b().then(v => { R = v })",
  "class K { static async c() { await 0; return new Error('x').stack } static async b() { return await K.c() } }; K.b().then(v => { R = v })",
  "var o = { async c() { await 0; return new Error('x').stack }, async b() { return await this.c() } }; o.b().then(v => { R = v })",
  "var c = async () => { await 0; return new Error('x').stack }; var b = async () => await c(); b().then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; async function a() { return (await Promise.all([c()]))[0] }; a().then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; async function a() { return (await Promise.allSettled([c()]))[0].value }; a().then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; async function a() { return await Promise.race([c()]) }; a().then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; async function a() { return await Promise.any([c()]) }; a().then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; async function a() { return (await Promise.all([1, c()]))[1] }; a().then(v => { R = v })",
  "async function c() { await 0; throw new Error('x') }; async function a() { await Promise.all([c()]) }; a().catch(e => { R = e.stack })",
  "async function c() { await 0; throw new Error('x') }; async function a() { await Promise.race([c()]) }; a().catch(e => { R = e.stack })",
  "async function c() { await 0; throw new Error('x') }; async function a() { try { await c() } catch (e) { return e.stack } }; a().then(v => { R = v })",
  "async function c() { throw new Error('x') }; async function a() { await c() }; a().catch(e => { R = e.stack })",
  "async function c() { throw new Error('x') }; async function a() { await 0; await c() }; a().catch(e => { R = e.stack })",
  "async function c() { null.p }; async function a() { await c() }; a().catch(e => { R = e.stack })",
  "async function* g() { await 0; yield new Error('x').stack }; async function a() { for await (var v of g()) return v }; a().then(v => { R = v })",
  "async function* g() { yield new Error('x').stack }; async function a() { for await (var v of g()) return v }; a().then(v => { R = v })",
  "async function* g() { await 0; throw new Error('x') }; async function a() { for await (var v of g()) ; }; a().catch(e => { R = e.stack })",
  "async function c() { await 0; return new Error('x').stack }; Promise.resolve().then(() => c()).then(v => { R = v })",
  "async function c() { await 0; return new Error('x').stack }; Promise.resolve().then(async () => { R = await c() })",
  "async function c() { await 0; return new Error('x').stack }; (async () => { R = await c() })()",
  "async function c() { await 0; return new Error('x').stack }; (async function top() { R = await c() })()",
  "var thenable = { then(res) { res(new Error('x').stack) } }; (async function a() { R = await thenable })()",
  "async function c() { await { then(r) { r(1) } }; return new Error('x').stack }; c().then(v => { R = v })",
  "async function c() { await Promise.resolve(); await Promise.resolve(); return new Error('x').stack }; async function b() { return await c() }; b().then(v => { R = v })",
  "async function c() { return new Error('x').stack }; async function a() { return await c() }; a().then(v => { R = v })",
  "async function f() { Error.stackTraceLimit = 1; await 0; return new Error('x').stack }; f().then(v => { R = v })",
  "async function f() { await 0; return await Promise.reject(new Error('x')) }; f().catch(e => { R = e.stack })",
  "function f() { return new Promise((_, rej) => rej(new Error('x'))) }; async function a() { await f() }; a().catch(e => { R = e.stack })",
  "async function a() { await 0; return new Error('x').stack }; var p = a(); p.then(v => { R = v })",
);

// ---- 8. Nomes inferidos de funções anônimas, arrows e computados no texto do frame.
const named = [
  "var f = function () { return STACK }; R = f()",
  "var f = () => STACK; R = f()",
  "let f = function () { return STACK }; R = f()",
  "const f = function () { return STACK }; R = f()",
  "var o = {}; o.f = function () { return STACK }; R = o.f()",
  "var o = { a: {} }; o.a.f = function () { return STACK }; R = o.a.f()",
  "var o = {}; o['f'] = function () { return STACK }; R = o.f()",
  "var o = { f: () => STACK }; R = o.f()",
  "var o = { f: function () { return STACK } }; R = o.f()",
  "var o = { 'q r': function () { return STACK } }; R = o['q r']()",
  "var k = 'dyn'; var o = { [k]: function () { return STACK } }; R = o.dyn()",
  "var k = 'dyn'; var o = { [k]: () => STACK }; R = o.dyn()",
  "var s = Symbol('sy'); var o = { [s]: function () { return STACK } }; R = o[s]()",
  "var s = Symbol(); var o = { [s]: function () { return STACK } }; R = o[s]()",
  "var f = (function () { return function () { return STACK } })(); R = f()",
  "var f = (() => () => STACK)(); R = f()",
  "var [f] = [function () { return STACK }]; R = f()",
  "var { f } = { f: function () { return STACK } }; R = f()",
  "var { f = function () { return STACK } } = {}; R = f()",
  "function g(f = function () { return STACK }) { return f() }; R = g()",
  "function g(f = () => STACK) { return f() }; R = g()",
  "var f; f ||= function () { return STACK }; R = f()",
  "var f; f ??= () => STACK; R = f()",
  "var f = class { static m() { return STACK } }.m; R = f()",
  "var o = { f: class { static m() { return STACK } } }; R = o.f.m()",
  "var f = function g() { return STACK }; R = f()",
  "var f = function () { return STACK }; Object.defineProperty(f, 'name', { value: 'renamed' }); R = f()",
  "var f = function named() { return STACK }; Object.defineProperty(f, 'name', { value: 'renamed' }); R = f()",
  "var f = function () { return STACK }; Object.defineProperty(f, 'name', { value: 1 }); R = f()",
  "var o = { get f() { return STACK } }; R = o.f",
  "var o = { get ['dy' + 'n']() { return STACK } }; R = o.dyn",
  "var o = { async f() { return STACK } }; o.f().then(v => { R = v })",
  "var o = { f: async function () { return STACK } }; o.f().then(v => { R = v })",
  "var o = { f: async () => STACK }; o.f().then(v => { R = v })",
  "var o = { f: function* () { yield STACK } }; R = o.f().next().value",
  "var o = { f: async function* () { yield STACK } }; o.f().next().then(v => { R = v.value })",
  "var o = { __proto__: { f() { return STACK } } }; R = o.f()",
  "function F() {}; F.prototype.f = function () { return STACK }; R = new F().f()",
  "function F() {}; F.s = function () { return STACK }; R = F.s()",
  "var F = function () {}; F.prototype = { f() { return STACK } }; R = new F().f()",
  "var o = Object.create({ f() { return STACK } }); R = o.f()",
  "var o = Object.assign({}, { f() { return STACK } }); R = o.f()",
  "var o = { f() { return STACK } }; var p = Object.create(o); p.g = o.f; R = p.g()",
  "var o = { f() { return STACK } }; var c = { f: o.f }; R = c.f()",
  "var f = function () { return STACK }; var o = { g: f }; R = o.g()",
  "var f = function () { return STACK }; var o = {}; o.g = f; R = o.g()",
  "R = (function () { return STACK }).call({ x: 1 })",
  "R = (function () { return STACK }).apply([], [])",
  "R = (() => STACK).call({})",
];
for (const body of named) programs.push(body.replace(/STACK/g, STACK));

// ---- Execução: `vm.runInThisContext(src, {filename})` em bun filho novo por programa, para a coluna não passar pelo
// source map que `bun arquivo.js` aplica (o divot do JSC fica no `(` de `new Error(`, igual ao porte). O filename é o
// mesmo de tests/stack_format_bun_golden.rs (`error_stack_case.js`). Os frames do próprio runner (`runInThisContext`
// e `zz_runner.js`) são removidos do texto; como eles ocupam vaga no `Error.stackTraceLimit`, cada programa roda duas
// vezes (runner com 0 e com 3 funções de embrulho) e é descartado se o resultado muda, o que denuncia truncamento.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stack-format-golden-"));
const srcFile = path.join(dir, "src.txt");
const runner = path.join(dir, "zz_runner.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
fs.writeFileSync(
  runner,
  `const vm = require('node:vm');
const src = require('fs').readFileSync(process.argv[2], 'utf8');
const depth = Number(process.argv[3]);
function go(n) { return n === 0 ? vm.runInThisContext(src, { filename: 'error_stack_case.js' }) : go(n - 1) }
try { go(depth) } catch (e) { }
`,
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
function measure(source, depth) {
  fs.writeFileSync(srcFile, source);
  const run = spawnSync(process.execPath, ["--preload", preload, runner, srcFile, String(depth)], { encoding: "utf8", cwd: dir, timeout: 15000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) return null;
  const text = JSON.parse(marked.slice(1));
  return text.split("\n").map(stripRunnerLine).filter(line => line !== null).join("\n");
}
// Linha com frame do runner: se for um `prepareStackTrace` que junta os frames num texto só (`join('|')`), a linha
// tem os frames do programa seguidos dos do runner; corta no primeiro segmento do runner em vez de descartar tudo
// (descartar dava `""` e escondia o formato do `CallSite`).
function stripRunnerLine(line) {
  const isRunner = part => part.includes("runInThisContext") || part.includes("zz_runner.js");
  if (!isRunner(line)) return line;
  const parts = line.split("|");
  const cut = parts.findIndex(isRunner);
  return parts.length > 1 && cut > 0 ? parts.slice(0, cut).join("|") : null;
}
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  const first = measure(source, 0);
  if (first === null) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  if (first !== measure(source, 3)) {
    dropped++;
    process.stderr.write("truncado pelos frames do runner: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = first.split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\/|\/var\/folders\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
