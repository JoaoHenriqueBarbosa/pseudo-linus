// Gera tests/golden/error_bun.tsv: Error e stack traces de runtime medidos no bun 1.4.2.
// Cobre construtor (message, options.cause, ordem de leitura), AggregateError, SuppressedError,
// Error.captureStackTrace, Error.stackTraceLimit, Error.prepareStackTrace com CallSite, a propriedade `stack`,
// toString, instanceof, TypeError com `evaluating '...'` (variantes de expressão) e os cinco tipos de erro nativos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// O arquivo se chama `error_case.js` dos dois lados, então nome, linha e coluna de cada frame são idênticos.
// Uso: bun scripts/gen-error-golden.js > tests/golden/error_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = (...bodies) => programs.push(...bodies);
// Captura o erro lançado por `code` em R como "Nome: mensagem".
const T = code => `try { ${code} } catch (e) { R = e instanceof Error ? e.name + ': ' + e.message : 'non-error ' + String(e) }`;

// ---- 1. Construtor: message e options.cause.
const messages = ["undefined", "''", "'m'", "'a b'", "1", "null", "true", "{}", "[1,2]", "Symbol.iterator.description", "{ toString() { return 'ts' } }", "'\\n'", "'\\u00e9'", "0", "-0", "1n"];
for (const m of messages) {
  add(`R = new Error(${m}).message`);
  add(`R = String(Object.getOwnPropertyDescriptor(new Error(${m}), 'message') !== undefined)`);
  add(`R = String(new Error(${m}))`);
  add(`R = Error(${m}).message`);
  add(`R = new TypeError(${m}).message + '|' + new RangeError(${m}).message`);
  add(`R = JSON.stringify(Object.getOwnPropertyNames(new Error(${m})))`);
}
add(T("new Error(Symbol())"), "R = new Error().hasOwnProperty('message')", "R = new Error(undefined).hasOwnProperty('message')", "R = new Error('').hasOwnProperty('message')");
const causes = ["undefined", "null", "1", "'c'", "new Error('inner')", "{}", "[]", "Symbol.iterator", "function f() {}", "0", "false"];
for (const c of causes) {
  add(`var e = new Error('m', { cause: ${c} }); R = e.hasOwnProperty('cause') + '|' + typeof e.cause`);
  add(`var e = new TypeError('m', { cause: ${c} }); R = JSON.stringify(Object.getOwnPropertyNames(e))`);
  add(`var e = new Error('m', { cause: ${c} }); var d = Object.getOwnPropertyDescriptor(e, 'cause'); R = d ? [d.writable, d.enumerable, d.configurable].join() : 'none'`);
  add(`var e = new Error('m', { cause: ${c} }); R = String(e)`);
  add(`var e = Error('m', { cause: ${c} }); R = e.hasOwnProperty('cause')`);
}
const optionsForms = ["undefined", "null", "1", "'s'", "{}", "{ cause: undefined }", "{ get cause() { return 7 } }", "Object.create({ cause: 9 })", "new Proxy({}, {})", "[]", "function () {}", "Object.assign(function () {}, { cause: 3 })", "{ 'cause': 1, other: 2 }", "Symbol()", "true"];
for (const o of optionsForms) {
  add(T(`var e = new Error('m', ${o}); R = e.hasOwnProperty('cause') + '|' + String(e.cause)`));
  add(T(`var e = new RangeError('m', ${o}); R = e.hasOwnProperty('cause') + '|' + String(e.cause)`));
}
// Ordem de leitura: message ToString antes de options.cause; HasProperty antes de Get.
add(
  "var log = []; var m = { toString() { log.push('toString'); return 'x' } }; var o = new Proxy({}, { has(t, k) { log.push('has ' + String(k)); return true }, get(t, k) { log.push('get ' + String(k)); return 5 } }); new Error(m, o); R = log.join()",
  "var log = []; var o = new Proxy({}, { has(t, k) { log.push('has ' + String(k)); return false }, get(t, k) { log.push('get ' + String(k)); return 5 } }); new Error('m', o); R = log.join()",
  "var log = []; var m = { toString() { log.push('toString'); return 'x' } }; new Error(m, { get cause() { log.push('cause'); return 1 } }); R = log.join()",
  "var log = []; var o = { get message() { log.push('message'); return 'q' }, get cause() { log.push('cause'); return 1 } }; new Error(o.message, o); R = log.join()",
  "var log = []; var p = new Proxy({}, { getPrototypeOf() { log.push('gpo'); return null }, ownKeys() { log.push('ownKeys'); return [] } }); new Error('m', p); R = log.join()",
  T("var o = new Proxy({}, { has() { throw new RangeError('has') } }); new Error('m', o)"),
  T("var o = new Proxy({}, { has() { return true }, get() { throw new RangeError('get') } }); new Error('m', o)"),
  T("new Error({ toString() { throw new RangeError('ts') } }, { get cause() { throw new TypeError('cause') } })"),
  "var log = []; class E2 extends Error { constructor(m, o) { log.push('before'); super(m, o); log.push('after') } }; new E2('x', { cause: 1 }); R = log.join()",
  "var e = new Error('a', { cause: 'b' }); R = Object.keys(e).length + '|' + JSON.stringify(Object.getOwnPropertyNames(e).sort())",
);

// ---- 2. AggregateError.
const iterables = ["[]", "[1,2,3]", "new Set([1,2])", "'ab'", "new Map([[1,2]])", "(function* () { yield 1; yield 2 })()", "undefined", "null", "1", "{}", "{ length: 1, 0: 'a' }", "{ [Symbol.iterator]() { return [7][Symbol.iterator]() } }", "[new Error('a'), new TypeError('b')]", "{ [Symbol.iterator]: 1 }", "{ [Symbol.iterator]() { return {} } }", "{ [Symbol.iterator]() { return { next() { throw new RangeError('nx') } } } }", "[,]", "[undefined]", "[[]]"];
for (const it of iterables) {
  add(T(`var e = new AggregateError(${it}); R = JSON.stringify([e.errors.length, e.message, e.name, Array.isArray(e.errors)])`));
  add(T(`var e = new AggregateError(${it}, 'msg'); R = e.message + '|' + String(e)`));
  add(T(`var e = new AggregateError(${it}, 'msg', { cause: 2 }); R = e.hasOwnProperty('cause') + '|' + e.cause`));
  add(T(`var e = AggregateError(${it}, 'msg'); R = e.errors.length + '|' + (e instanceof AggregateError)`));
  add(T(`var e = new AggregateError(${it}); var d = Object.getOwnPropertyDescriptor(e, 'errors'); R = [d.writable, d.enumerable, d.configurable].join()`));
  add(T(`var e = new AggregateError(${it}); R = JSON.stringify(Object.getOwnPropertyNames(e))`));
}
add(
  "R = [AggregateError.length, AggregateError.name, Object.getPrototypeOf(AggregateError) === Error, Object.getPrototypeOf(AggregateError.prototype) === Error.prototype, AggregateError.prototype.name, AggregateError.prototype.message === ''].join()",
  "R = Object.getOwnPropertyNames(AggregateError.prototype).sort().join()",
  "var log = []; var o = { [Symbol.iterator]() { log.push('iter'); return [][Symbol.iterator]() } }; new AggregateError(o, { toString() { log.push('msg'); return '' } }, { get cause() { log.push('cause'); return 1 } }); R = log.join()",
  "var a = [1]; var e = new AggregateError(a); a.push(2); R = e.errors.length + '|' + (e.errors === a)",
  T("class A2 extends AggregateError {}; var e = new A2([1], 'x'); R = [e.name, e instanceof A2, e instanceof AggregateError, e.errors.length, String(e)].join()"),
);

// ---- 3. SuppressedError (bun pode não ter; o resultado fica registrado de qualquer modo).
add(
  "R = typeof SuppressedError",
  T("var e = new SuppressedError(new Error('a'), new Error('b'), 'm'); R = [e.name, e.message, e.error.message, e.suppressed.message, String(e)].join('|')"),
  T("var e = new SuppressedError(1, 2); R = [e.hasOwnProperty('message'), e.error, e.suppressed].join()"),
  T("var e = SuppressedError(1, 2, 'x'); R = e.message + '|' + e.error + '|' + e.suppressed"),
  T("var e = new SuppressedError(); R = String(e.error) + '|' + String(e.suppressed) + '|' + e.hasOwnProperty('error')"),
  T("var e = new SuppressedError(1, 2, 'm', { cause: 3 }); R = JSON.stringify(Object.getOwnPropertyNames(e))"),
  T("var e = new SuppressedError(1, 2, 'm'); var d = Object.getOwnPropertyDescriptor(e, 'error'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("var e = new SuppressedError(1, 2, 'm'); var d = Object.getOwnPropertyDescriptor(e, 'suppressed'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("R = [SuppressedError.length, SuppressedError.name, Object.getPrototypeOf(SuppressedError) === Error, SuppressedError.prototype.name].join()"),
  T("R = Object.getOwnPropertyNames(SuppressedError.prototype).sort().join()"),
  T("R = String(new SuppressedError(1, 2, 'm') instanceof Error)"),
  T("class S2 extends SuppressedError {}; var e = new S2(1, 2, 'q'); R = [e.name, e instanceof S2, String(e)].join()"),
  T("var log = []; new SuppressedError({ get x() { log.push('e') } }, 2, { toString() { log.push('m'); return '' } }, { get cause() { log.push('c') } }); R = log.join()"),
  T("{ using a = { [Symbol.dispose]() { throw new Error('d1') } }; using b = { [Symbol.dispose]() { throw new Error('d2') } }; throw new Error('body') }"),
);

// ---- 4. Error.captureStackTrace.
const capCtx = [
  ["R = (function () { var o = {}; Error.captureStackTrace(o); return o.stack })()", "named"],
  ["function a() { var o = {}; Error.captureStackTrace(o, a); return o.stack }; function b() { return a() }; R = b()", "opt"],
  ["function a() { var o = {}; Error.captureStackTrace(o, b); return o.stack }; function b() { return a() }; R = b()", "opt-outer"],
  ["function a() { var o = {}; Error.captureStackTrace(o, c); return o.stack }; function b() { return a() }; function c() {}; R = b()", "opt-absent"],
  ["function a() { var o = {}; Error.captureStackTrace(o, 1); return o.stack }; R = a()", "opt-number"],
  ["function a() { var o = {}; Error.captureStackTrace(o, null); return o.stack }; R = a()", "opt-null"],
  ["function a() { var o = {}; Error.captureStackTrace(o, undefined); return o.stack }; R = a()", "opt-undef"],
  ["function a() { var o = {}; Error.captureStackTrace(o, {}); return o.stack }; R = a()", "opt-object"],
  ["function a() { var o = {}; Error.captureStackTrace(o, () => {}); return o.stack }; R = a()", "opt-arrow"],
  ["function a() { var o = {}; Error.captureStackTrace(o, Math.max); return o.stack }; R = a()", "opt-native"],
  ["function a() { var o = {}; Error.captureStackTrace(o, a.bind(null)); return o.stack }; R = a()", "opt-bound"],
  ["class K { constructor() { Error.captureStackTrace(this, K) } }; function mk() { return new K() }; R = mk().stack", "class-ctor"],
  ["class K { constructor() { Error.captureStackTrace(this, new.target) } }; class L extends K {}; R = new L().stack", "newtarget"],
  ["class K { constructor() { Error.captureStackTrace(this) } }; R = new K().stack", "class-nocopt"],
  ["function MyErr(m) { this.message = m; Error.captureStackTrace(this, MyErr) }; function mk() { return new MyErr('q') }; R = mk().stack", "fn-ctor"],
  ["function MyErr(m) { this.message = m; this.name = 'MyErr'; Error.captureStackTrace(this) }; R = new MyErr('q').stack", "fn-ctor-name"],
  ["var o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); R = o.stack", "named-obj"],
  ["var o = { name: 'N' }; Error.captureStackTrace(o); R = o.stack", "name-only"],
  ["var o = { message: 'M' }; Error.captureStackTrace(o); R = o.stack", "msg-only"],
  ["var o = {}; Error.captureStackTrace(o); R = o.stack", "empty"],
  ["var o = { toString() { return 'custom' } }; Error.captureStackTrace(o); R = o.stack", "tostring"],
  ["var o = { name: 5, message: 6 }; Error.captureStackTrace(o); R = o.stack", "nonstring"],
  ["var o = new Error('orig'); Error.captureStackTrace(o); R = o.stack", "on-error"],
  ["var o = new Error('orig'); o.message = 'changed'; Error.captureStackTrace(o); R = o.stack", "changed"],
  ["var o = function () {}; Error.captureStackTrace(o); R = typeof o.stack", "on-function"],
  ["var o = []; Error.captureStackTrace(o); R = typeof o.stack", "on-array"],
  ["var o = Object.create(null); Error.captureStackTrace(o); R = typeof o.stack", "null-proto"],
  ["var o = {}; Error.captureStackTrace(o); var d = Object.getOwnPropertyDescriptor(o, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d].join()", "descriptor"],
  ["var o = Object.freeze({}); try { Error.captureStackTrace(o); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }", "frozen"],
  ["var o = Object.preventExtensions({}); try { Error.captureStackTrace(o); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }", "nonext"],
  ["var o = Object.seal({ stack: 1 }); try { Error.captureStackTrace(o); R = 'ok ' + o.stack } catch (e) { R = e.name + ': ' + e.message }", "sealed-has"],
  ["var o = {}; Object.defineProperty(o, 'stack', { value: 1, configurable: false }); try { Error.captureStackTrace(o); R = 'ok ' + o.stack } catch (e) { R = e.name + ': ' + e.message }", "nonconf"],
  ["var o = {}; Object.defineProperty(o, 'stack', { value: 1, configurable: true, writable: false }); try { Error.captureStackTrace(o); R = typeof o.stack } catch (e) { R = e.name + ': ' + e.message }", "nonwritable"],
  ["var o = new Proxy({}, {}); try { Error.captureStackTrace(o); R = typeof o.stack } catch (e) { R = e.name + ': ' + e.message }", "proxy"],
  ["try { Error.captureStackTrace(1) } catch (e) { R = e.name + ': ' + e.message }", "prim-number"],
  ["try { Error.captureStackTrace('s') } catch (e) { R = e.name + ': ' + e.message }", "prim-string"],
  ["try { Error.captureStackTrace(undefined) } catch (e) { R = e.name + ': ' + e.message }", "undef"],
  ["try { Error.captureStackTrace(null) } catch (e) { R = e.name + ': ' + e.message }", "null"],
  ["try { Error.captureStackTrace() } catch (e) { R = e.name + ': ' + e.message }", "noargs"],
  ["try { Error.captureStackTrace(Symbol()) } catch (e) { R = e.name + ': ' + e.message }", "symbol"],
  ["R = Error.captureStackTrace({}) === undefined", "retval"],
  ["R = Error.captureStackTrace.length + '|' + Error.captureStackTrace.name", "meta"],
  ["R = Object.getOwnPropertyNames(Error).sort().join()", "ownprops"],
  ["R = typeof TypeError.captureStackTrace + '|' + (TypeError.captureStackTrace === Error.captureStackTrace)", "inherit"],
  ["function a() { var o = {}; Error.captureStackTrace(o, a); return o.stack }; function b() { return a() }; function c() { return b() }; R = c()", "deep"],
  ["Error.stackTraceLimit = 1; function a() { var o = {}; Error.captureStackTrace(o); return o.stack }; function b() { return a() }; R = b()", "limit1"],
  ["Error.stackTraceLimit = 0; var o = {}; Error.captureStackTrace(o); R = JSON.stringify(o.stack)", "limit0"],
  ["Error.stackTraceLimit = 1; function a() { var o = {}; Error.captureStackTrace(o, a); return o.stack }; function b() { return a() }; function c() { return b() }; R = c()", "limit1-opt"],
  ["var o = {}; Error.captureStackTrace(o); var s1 = o.stack; Error.captureStackTrace(o); R = (s1 === o.stack) + '|' + (s1.split('\\n').length === o.stack.split('\\n').length)", "twice"],
  ["var e = new Error('a'); var s = e.stack; var o = {}; Error.captureStackTrace(o); R = typeof s + typeof o.stack", "mixed"],
  ["var o = {}; Error.captureStackTrace(o); o.stack = 'set'; R = o.stack", "assign"],
  ["var o = {}; Error.captureStackTrace(o); delete o.stack; R = String(o.stack)", "delete"],
  ["var o = {}; Error.captureStackTrace(o); R = JSON.stringify(Object.getOwnPropertyNames(o))", "names"],
  ["var o = {}; Error.captureStackTrace(o); R = JSON.stringify(Object.keys(o))", "keys"],
  ["var o = { a: 1 }; Error.captureStackTrace(o); R = JSON.stringify(o)", "json"],
  ["var o = {}; (function inner() { (() => { Error.captureStackTrace(o) })() })(); R = o.stack", "arrow-inner"],
  ["var o = {}; async function af() { await 1; Error.captureStackTrace(o) }; af().then(() => { R = o.stack })", "async"],
  ["var o = {}; var p = { m() { Error.captureStackTrace(o, this.m) } }; p.m(); R = o.stack", "method-opt"],
  ["var o = {}; var p = { get g() { Error.captureStackTrace(o) } }; p.g; R = o.stack", "getter"],
  ["var o = {}; eval('Error.captureStackTrace(o)'); R = o.stack", "eval"],
  ["var o = {}; new Function('o', 'Error.captureStackTrace(o)')(o); R = o.stack", "newfunction"],
  ["var o = {}; [1].forEach(function cb() { Error.captureStackTrace(o, cb) }); R = o.stack", "foreach-opt"],
];
for (const [body] of capCtx) add(body);

// ---- 5. Error.stackTraceLimit.
const limits = ["0", "-1", "-0", "1", "2", "3", "100", "Infinity", "-Infinity", "NaN", "1.5", "2.9", "'3'", "'abc'", "null", "undefined", "true", "false", "{}", "[]", "[2]", "1e10", "2**31", "2**32+1", "Symbol()", "1n", "{ valueOf() { return 2 } }"];
const deep = "function f4() { return new Error('x').stack }; function f3() { return f4() }; function f2() { return f3() }; function f1() { return f2() }; ";
for (const l of limits) {
  add(T(`Error.stackTraceLimit = ${l}; ${deep} R = JSON.stringify(f1())`));
  add(T(`Error.stackTraceLimit = ${l}; ${deep} R = f1().split('\\n').length`));
  add(T(`Error.stackTraceLimit = ${l}; R = typeof Error.stackTraceLimit + ':' + String(Error.stackTraceLimit)`));
  add(T(`Error.stackTraceLimit = ${l}; ${deep} var e; try { null.x } catch (err) { e = err }; R = e.stack.split('\\n').length`));
  add(T(`Error.stackTraceLimit = ${l}; var o = {}; Error.captureStackTrace(o); R = JSON.stringify(o.stack)`));
}
add(
  "R = [Error.stackTraceLimit, Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit') && Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit').writable].join()",
  "var d = Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit'); R = [d.writable, d.enumerable, d.configurable].join()",
  `delete Error.stackTraceLimit; ${deep} R = JSON.stringify(f1())`,
  `delete Error.stackTraceLimit; R = String(Error.stackTraceLimit)`,
  `Error.stackTraceLimit = 2; ${deep} var e = f1(); Error.stackTraceLimit = 10; R = e.split('\\n').length`,
  `${deep} var e = new Error('x'); Error.stackTraceLimit = 0; R = e.stack.split('\\n').length`,
  `Error.stackTraceLimit = 2; ${deep} class E2 extends Error {}; R = new E2('q').stack.split('\\n').length`,
  `Error.stackTraceLimit = 1; ${deep} R = new TypeError('q').stack`,
  `Error.stackTraceLimit = 1; var e; try { undefinedVariable } catch (err) { e = err }; R = e.stack`,
  `Object.defineProperty(Error, 'stackTraceLimit', { get() { return 1 }, configurable: true }); ${deep} R = f1()`,
  `Object.defineProperty(Error, 'stackTraceLimit', { get() { throw new RangeError('lim') }, configurable: true }); try { R = new Error('x').stack } catch (e) { R = 'threw ' + e.message }`,
  `Error.stackTraceLimit = 3; function rec(n) { return n ? rec(n - 1) : new Error('x').stack }; R = rec(10)`,
  `Error.stackTraceLimit = Infinity; function rec(n) { return n ? rec(n - 1) : new Error('x').stack }; R = rec(10).split('\\n').length`,
  `Error.stackTraceLimit = 100; function rec(n) { return n ? rec(n - 1) : new Error('x').stack }; R = rec(150).split('\\n').length`,
);

// ---- 6. Error.prepareStackTrace com CallSite.
const E = "new Error('x').stack";
const siteCtx = [
  `function named() { return ${E} }; R = named()`,
  `var anon = function () { return ${E} }; R = anon()`,
  `R = (function () { return ${E} })()`,
  `R = (() => ${E})()`,
  `var o = { m() { return ${E} } }; R = o.m()`,
  `var o = { get g() { return ${E} } }; R = o.g`,
  `var o = { set s(v) { R = ${E} } }; o.s = 1`,
  `class C { constructor() { this.s = ${E} } }; R = new C().s`,
  `class C { static sm() { return ${E} } }; R = C.sm()`,
  `class C { m() { return ${E} } }; R = new C().m()`,
  `class C { #p() { return ${E} } q() { return this.#p() } }; R = new C().q()`,
  `async function af() { return ${E} }; af().then(v => { R = v })`,
  `async function af() { await 1; return ${E} }; af().then(v => { R = v })`,
  `R = eval("${E}")`,
  `R = eval("(function ev() { return ${E} })()")`,
  `R = new Function("return ${E}")()`,
  `function bf() { return ${E} }; R = bf.bind(null)()`,
  `function* gen() { yield ${E} }; R = gen().next().value`,
  `R = [1].map(function cb() { return ${E} })[0]`,
  `function Ctor() { this.s = ${E} }; R = new Ctor().s`,
  `var o = { m() { return ${E} } }; R = o.m.call(1)`,
  `var o = { m() { return ${E} } }; R = o.m.call('s')`,
  `var o = { m() { return ${E} } }; R = o.m.call(null)`,
  `Number.prototype.nm = function () { return ${E} }; R = (1).nm()`,
  `function Foo() {}; Foo.prototype.meth = function () { return ${E} }; R = new Foo().meth()`,
  `var o = { ['comp' + 1]() { return ${E} } }; R = o.comp1()`,
  `var f = function inner() { return ${E} }; R = f()`,
  `R = [1].map(() => ${E})[0]`,
  `function outer() { return (() => ${E})() }; R = outer()`,
  `var o = { f: function () { return ${E} } }; R = o.f()`,
  `var o = { f: () => ${E} }; R = o.f()`,
  `var o = { a: { b() { return ${E} } } }; R = o.a.b()`,
  `Promise.resolve().then(function thenCb() { R = ${E} })`,
  `Promise.resolve().then(function tm() { R = ${E} })`,
  `${E.replace("new Error('x').stack", "")}R = (function () { return Reflect.apply(function ra() { return ${E} }, null, []) })()`,
];
const sitePrepare = (fnBody) => `Error.prepareStackTrace = function (e, cs) { var m = function (c, n) { try { var v = c[n](); return typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean' || v == null ? String(v) : typeof v } catch (x) { return 'ERR ' + x.name + ': ' + x.message } }; ${fnBody} }; `;
const siteMethods = ["getFileName", "getLineNumber", "getColumnNumber", "getFunctionName", "getTypeName", "getMethodName", "isNative", "isConstructor", "isAsync", "isEval", "isToplevel", "toString", "getEvalOrigin", "getScriptNameOrSourceURL", "isPromiseAll", "getPromiseIndex", "getEnclosingLineNumber", "getEnclosingColumnNumber", "getPosition", "isStrict"];
for (let i = 0; i < siteCtx.length; i++) {
  for (const meth of siteMethods) {
    add(T(sitePrepare(`return cs.map(function (c) { return m(c, '${meth}') }).join('\\n')`) + siteCtx[i]));
  }
  add(T(sitePrepare("return typeof cs + '|' + Array.isArray(cs) + '|' + cs.length + '|' + (e instanceof Error)") + siteCtx[i]));
}
add(
  T("Error.prepareStackTrace = function (e, cs) { return 42 }; R = typeof new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return undefined }; R = String(new Error('x').stack)"),
  T("Error.prepareStackTrace = function (e, cs) { return { a: 1 } }; R = JSON.stringify(new Error('x').stack)"),
  T("Error.prepareStackTrace = function (e, cs) { throw new RangeError('prep') }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { throw new RangeError('prep') }; try { R = new Error('x').stack } catch (e) { R = 'caught ' + e.message }"),
  T("Error.prepareStackTrace = function (e, cs) { return 'a' }; var e = new Error('x'); R = e.stack + e.stack"),
  T("var n = 0; Error.prepareStackTrace = function (e, cs) { n++; return 'a' + n }; var e = new Error('x'); R = e.stack + e.stack + n"),
  T("var n = 0; Error.prepareStackTrace = function (e, cs) { n++; return 'a' }; var e = new Error('x'); R = String(n)"),
  T("Error.prepareStackTrace = function (e, cs) { return e.name + '/' + e.message }; R = new TypeError('tm').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return this === Error ? 'Error-this' : typeof this }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { 'use strict'; return String(this) }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function () { return arguments.length + '' }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = 1; R = new Error('x').stack.split('\\n')[0]"),
  T("Error.prepareStackTrace = null; R = new Error('x').stack.split('\\n')[0]"),
  T("Error.prepareStackTrace = {}; R = new Error('x').stack.split('\\n')[0]"),
  T("Error.prepareStackTrace = undefined; R = new Error('x').stack.split('\\n')[0]"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; Error.stackTraceLimit = 0; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; Error.stackTraceLimit = 1; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; var o = {}; Error.captureStackTrace(o); R = o.stack"),
  T("Error.prepareStackTrace = function (e, cs) { return Object.prototype.toString.call(e) }; var o = {}; Error.captureStackTrace(o); R = o.stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; function a() { var o = {}; Error.captureStackTrace(o, a); return o.stack }; R = a()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0] === undefined ? 'none' : typeof cs[0] }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return Object.getOwnPropertyNames(Object.getPrototypeOf(cs[0])).sort().join() }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return Object.prototype.toString.call(cs[0]) + '|' + cs[0].constructor.name }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return Object.keys(cs[0]).join() }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return String(cs[0].getThis()) + '|' + typeof cs[0].getFunction() }; R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return String(cs[0].getThis()) }; 'use strict'; function f() { return new Error('x').stack }; R = f()"),
  T("var seen; Error.prepareStackTrace = function (e, cs) { seen = e; return 'z' }; var e = new Error('x'); e.stack; R = String(seen === e)"),
  T("Error.prepareStackTrace = function (e, cs) { return 'z' }; var e = new Error('x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(String).join('|') }; function f() { return new Error('x').stack }; R = f()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c + '').join('|') }; var o = { m() { return new Error('x').stack } }; R = o.m()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => `${c}`).join('|') }; class K { m() { return new Error('x').stack } }; R = new K().m()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; class K { static s() { return new Error('x').stack } }; R = K.s()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; class K { constructor() { this.s = new Error('x').stack } }; R = new K().s"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = (function () { return eval('new Error(\"x\").stack') })()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = eval('new Error(\"x\").stack\\n//# sourceURL=fixed_source.js')"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFileName() + ':' + c.getLineNumber()).join('|') }; R = eval('new Error(\"x\").stack\\n//# sourceURL=fixed_source.js')"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFileName() + ':' + c.getLineNumber()).join('|') }; R = new Function('return new Error(\"x\").stack')()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; var o = { __proto__: { inh() { return new Error('x').stack } } }; R = o.inh()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; var f = function () { return new Error('x').stack }; var g = f; R = g()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; var o = {}; o.x = function () { return new Error('x').stack }; R = o.x()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; var s = Symbol('sy'); var o = { [s]() { return new Error('x').stack } }; R = o[s]()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; function f() { return new Error('x').stack }; Object.defineProperty(f, 'name', { value: 'renamed' }); R = f()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getTypeName()).join('|') }; function F() {}; F.prototype.m = function () { return new Error('x').stack }; R = new F().m()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getTypeName()).join('|') }; R = [1].map(function () { return new Error('x').stack })[0]"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getMethodName()).join('|') }; var o = { aa() { return new Error('x').stack } }; R = o.aa()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isNative() + ':' + c.getFunctionName()).join('|') }; R = [1].map(function () { return new Error('x').stack })[0]"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isNative() + ':' + c.getFunctionName() + ':' + c.getFileName()).join('|') }; R = JSON.parse('[1]', function () { return new Error('x').stack })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; R = [3, 1, 2].sort(function srt() { return new Error('x').stack.length })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isConstructor()).join('|') }; function F() { this.s = new Error('x').stack }; R = new F().s"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isConstructor()).join('|') }; function F() { return new Error('x').stack }; R = F()"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isConstructor()).join('|') }; class A { constructor() { this.s = new Error('x').stack } }; class B extends A { constructor() { super() } }; R = new B().s"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isAsync()).join('|') }; async function a1() { await null; return new Error('x').stack }; async function a2() { return await a1() }; a2().then(v => { R = v })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName() + ':' + c.isAsync()).join('|') }; async function a1() { await null; return new Error('x').stack }; async function a2() { return await a1() }; a2().then(v => { R = v })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; async function a1() { await null; return new Error('x').stack }; async function a2() { return await a1() }; a2().then(v => { R = v })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; async function a1() { await null; throw new Error('x') }; async function a2() { try { await a1() } catch (err) { return err.stack } }; a2().then(v => { R = v })"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; Promise.all([1]).then(function () { R = new Error('x').stack })"),
  T("Error.prepareStackTrace = function (e, cs) { var s = cs[0].getLineNumber() + ':' + cs[0].getColumnNumber(); return s }; \n\n  R = new Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; R = new\nError('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; R = Error('x').stack"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; try { null.x } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; try { throw new Error('x') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; try { undefinedVar } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() }; try { [].reduce(function () {}) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName() + ':' + c.getLineNumber()).join('|') }; try { [].reduce(function () {}) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName() + ':' + c.getLineNumber()).join('|') }; try { JSON.parse('{') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName() + ':' + c.getLineNumber()).join('|') }; try { new Array(-1) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName() + ':' + c.getLineNumber()).join('|') }; try { 'a'.repeat(-1) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isNative() + ':' + c.getFunctionName()).join('|') }; try { 'a'.repeat(-1) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { 'a'.repeat(-1) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { Object.defineProperty(1, 'a', {}) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { new (class A { constructor() { null.x } }) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { (function f() { 'use strict'; undefinedVar = 1 })() } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { decodeURIComponent('%') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { (1).toFixed(101) } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { eval('var') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; try { new Function('var') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; try { eval('var') } catch (err) { R = err.stack }"),
  T("Error.prepareStackTrace = function (e, cs) { return cs.length }; function r() { r() }; try { r() } catch (err) { R = typeof err.stack + ':' + err.name }"),
  T("Error.prepareStackTrace = function (e, cs) { return 'c' + cs.length }; function r() { r() }; try { r() } catch (err) { R = err.stack }"),
);

// ---- 7. `stack` como propriedade de dado.
const stackProps = [
  "var e = new Error('x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d, 'set' in d].join()",
  "var e = new TypeError('x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()",
  "var e = new AggregateError([], 'x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()",
  "class K extends Error {}; var d = Object.getOwnPropertyDescriptor(new K('x'), 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()",
  "R = String(Object.getOwnPropertyDescriptor(Error.prototype, 'stack'))",
  "R = String('stack' in Error.prototype)",
  "R = String(Object.getOwnPropertyDescriptor(new Error('x'), 'stack') !== undefined)",
  "R = JSON.stringify(Object.getOwnPropertyNames(new Error('x')))",
  "R = JSON.stringify(Object.getOwnPropertyNames(new Error()))",
  "R = JSON.stringify(Object.keys(new Error('x')))",
  "R = JSON.stringify(Object.getOwnPropertyNames(new Error('x', { cause: 1 })))",
  "R = JSON.stringify(new Error('x'))",
  "R = JSON.stringify(Object.getOwnPropertyNames(new TypeError('x')))",
  "var e = new Error('x'); e.stack = 'custom'; R = e.stack",
  "var e = new Error('x'); e.stack = 5; R = typeof e.stack",
  "var e = new Error('x'); delete e.stack; R = String(e.stack)",
  "var e = new Error('x'); delete e.stack; R = String('stack' in e)",
  "var e = new Error('x'); Object.defineProperty(e, 'stack', { value: 'v' }); R = e.stack",
  "var e = new Error('x'); Object.freeze(e); R = typeof e.stack",
  "var e = new Error('x'); Object.freeze(e); try { e.stack = 1 } catch (x) { R = x.name + ': ' + x.message }",
  "var e = new Error('x'); Object.freeze(e); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [d.writable, d.configurable].join()",
  "var e = new Error('x'); Object.defineProperty(e, 'stack', { get() { return 'g' } }); R = e.stack",
  "var e = new Error('x'); R = typeof e.stack + typeof e.stack",
  "var e = new Error('x'); R = (e.stack === e.stack)",
  "var e = Object.create(Error.prototype); R = String(e.stack)",
  "var e = Object.create(new Error('x')); R = typeof e.stack + '|' + e.hasOwnProperty('stack')",
  "function E2(m) { this.message = m }; E2.prototype = Object.create(Error.prototype); R = String(new E2('x').stack)",
  "function E2(m) { Error.call(this, m) }; E2.prototype = Object.create(Error.prototype); R = String(new E2('x').stack) + '|' + new E2('x').hasOwnProperty('message')",
  "function E2(m) { var e = Error.call(this, m); this.stack = e.stack }; E2.prototype = Object.create(Error.prototype); R = typeof new E2('x').stack",
  "var e = new Error('x'); var c = Object.assign({}, e); R = JSON.stringify(Object.keys(c))",
  "var e = new Error('x'); var c = { ...e }; R = JSON.stringify(Object.keys(c))",
  "var e = new Error('x'); R = String(Object.entries(e).length)",
  "var e = new Error('x'); var s = []; for (var k in e) s.push(k); R = s.join()",
  "var e = new Error('x'); R = typeof structuredClone(e).stack",
  "var e = new Error('x'); var c = structuredClone(e); R = [c.name, c.message, c instanceof Error, c.stack === e.stack].join()",
  "var e = new TypeError('x'); var c = structuredClone(e); R = [c.name, c.message, c instanceof TypeError].join()",
  "var e = new Error('x', { cause: 'c' }); var c = structuredClone(e); R = [c.cause, c.hasOwnProperty('cause')].join()",
  "R = Object.prototype.hasOwnProperty.call(new Error('x'), 'stack') + '|' + Object.prototype.hasOwnProperty.call(new Error('x'), 'message')",
  "var e = new Error('x'); Object.preventExtensions(e); R = typeof e.stack",
  "var e = new Error('x'); var s = Object.getOwnPropertyDescriptors(e); R = Object.keys(s).join()",
  "var e = new Error('x'); e.name = 'Changed'; R = e.stack.split('\\n')[0]",
  "var e = new Error('x'); e.message = 'Changed'; R = e.stack.split('\\n')[0]",
  "var e = new Error('x'); var s = e.stack; e.message = 'Changed'; R = e.stack === s",
  "class K extends Error { constructor(m) { super(m); this.name = 'K' } }; R = new K('x').stack.split('\\n')[0]",
  "class K extends Error { get name() { return 'KK' } }; R = new K('x').stack.split('\\n')[0]",
  "class K extends Error {}; K.prototype.name = 'KP'; R = new K('x').stack.split('\\n')[0]",
  "class K extends Error { constructor(m) { super(m); this.message = 'later' } }; R = new K('x').stack.split('\\n')[0]",
  "class K extends Error {}; R = new K('x').stack.split('\\n')[0]",
  "class K extends TypeError {}; R = new K('x').stack.split('\\n')[0]",
  "class K extends Error { static get [Symbol.species]() { return 1 } }; R = new K('x').stack.split('\\n')[0]",
  "var e = new Error('x'); e.name = undefined; R = String(e.name) + '|' + String(e)",
];
add(...stackProps);

// ---- 8. Formato de `err.stack`.
const stackFmt = [
  "function f() { return new Error('x').stack }; R = f()",
  "R = new Error('x').stack",
  "R = new Error().stack",
  "R = new Error('').stack",
  "R = new Error('multi\\nline').stack",
  "R = new TypeError('x').stack",
  "R = new RangeError('x').stack",
  "R = new EvalError('x').stack",
  "R = new URIError('x').stack",
  "R = new SyntaxError('x').stack",
  "R = new ReferenceError('x').stack",
  "R = new AggregateError([], 'x').stack",
  "var e = new Error('x'); e.name = 'Custom'; R = e.stack",
  "class K extends Error {}; R = new K('x').stack",
  "class K extends Error { constructor(m) { super(m); this.name = 'K' } }; R = new K('x').stack",
  "function f() { return new Error('x').stack }; R = f.call(1)",
  "function f() { 'use strict'; return new Error('x').stack }; R = f.call(1)",
  "function f() { return new Error('x').stack }; function g() { return f() }; R = g()",
  "function f() { return new Error('x').stack }; function g() { return f() }; function h() { return g() }; R = h()",
  "var o = { m() { return new Error('x').stack } }; R = o.m()",
  "var o = { get g() { return new Error('x').stack } }; R = o.g",
  "class K { m() { return new Error('x').stack } }; R = new K().m()",
  "class K { static m() { return new Error('x').stack } }; R = K.m()",
  "class K { constructor() { this.s = new Error('x').stack } }; R = new K().s",
  "class K { static { R = new Error('x').stack } }",
  "class K { static p = new Error('x').stack }; R = K.p",
  "class K { p = new Error('x').stack }; R = new K().p",
  "async function f() { return new Error('x').stack }; f().then(v => { R = v })",
  "async function f() { await 1; return new Error('x').stack }; f().then(v => { R = v })",
  "async function g() { await 1; return new Error('x').stack }; async function f() { return await g() }; f().then(v => { R = v })",
  "async function g() { return new Error('x').stack }; async function f() { return await g() }; f().then(v => { R = v })",
  "function* g() { yield new Error('x').stack }; R = g().next().value",
  "async function* g() { yield new Error('x').stack }; g().next().then(v => { R = v.value })",
  "R = eval('new Error(\"x\").stack')",
  "R = eval('new Error(\"x\").stack\\n//# sourceURL=fixed_eval.js')",
  "R = new Function('return new Error(\"x\").stack')()",
  "R = new Function('a', 'b', 'return new Error(\"x\").stack')()",
  "function f() { return new Error('x').stack }; R = f.bind(null)()",
  "function f() { return new Error('x').stack }; R = f.apply(null)",
  "function f() { return new Error('x').stack }; R = f.call(null)",
  "function f() { return new Error('x').stack }; R = Reflect.apply(f, null, [])",
  "function f() { return new Error('x').stack }; R = [1].map(f)[0]",
  "function f() { return new Error('x').stack }; R = [1].map(function () { return f() })[0]",
  "R = [1].map(() => new Error('x').stack)[0]",
  "R = [1].forEach(function fe() { R = new Error('x').stack })",
  "function f() { return new Error('x').stack }; var o = { toString: f }; R = String(o)",
  "function f() { return new Error('x').stack }; var o = { valueOf: f }; R = o + ''",
  "function f() { return new Error('x').stack }; var o = { get [Symbol.toPrimitive]() { return f } }; R = `${o}`",
  "function f() { return new Error('x').stack }; R = new Proxy(f, {})()",
  "function f() { return new Error('x').stack }; R = new Proxy(f, { apply(t, th, a) { return t() } })()",
  "R = JSON.stringify({ toJSON() { return new Error('x').stack } })",
  "R = JSON.parse('[1]', function () { return new Error('x').stack })",
  "R = 'ab'.replace('a', function () { return new Error('x').stack })",
  "R = [2, 1].sort(function (a, b) { R = new Error('x').stack; return a - b }), R",
  "var e; try { throw new Error('x') } catch (err) { e = err }; R = e.stack",
  "function f() { throw new Error('x') }; try { f() } catch (e) { R = e.stack }",
  "function f() { try { throw new Error('x') } finally { } }; try { f() } catch (e) { R = e.stack }",
  "var e = (function () { return new Error('x') })(); R = (function () { return e.stack })()",
  "function f() { return new Error('x').stack }; R = f.name + '|' + f()",
  "var f = function () { return new Error('x').stack }; R = f()",
  "var o = {}; o.f = function () { return new Error('x').stack }; R = o.f()",
  "var o = { f: function () { return new Error('x').stack } }; R = o.f()",
  "var o = { f: function g() { return new Error('x').stack } }; R = o.f()",
  "var o = { f: () => new Error('x').stack }; R = o.f()",
  "var f = () => new Error('x').stack; R = f()",
  "(function () { R = new Error('x').stack })()",
  "(() => { R = new Error('x').stack })()",
  "(async () => { R = new Error('x').stack })()",
  "new (function () { R = new Error('x').stack })()",
  "new (function Named() { R = new Error('x').stack })()",
  "new (class { constructor() { R = new Error('x').stack } })()",
  "new (class Named { constructor() { R = new Error('x').stack } })()",
  "function Foo() { R = new Error('x').stack }; new Foo",
  "function Foo() { R = new Error('x').stack }; Foo.prototype.bar = function () { R = new Error('x').stack }; new Foo().bar()",
  "function f() { return new Error('x').stack }; R = new f() instanceof Error",
  "var o = { a: { b: { c() { return new Error('x').stack } } } }; R = o.a.b.c()",
  "var o = { ['a' + 'b']() { return new Error('x').stack } }; R = o.ab()",
  "var o = { [Symbol('s')]() {} }; var s = Object.getOwnPropertySymbols(o)[0]; R = typeof s",
  "var o = { async am() { return new Error('x').stack } }; o.am().then(v => { R = v })",
  "var o = { *gm() { yield new Error('x').stack } }; R = o.gm().next().value",
  "var o = { set s(v) { R = new Error('x').stack } }; o.s = 1",
  "function f() { return new Error('x').stack }\nfunction g() { return f() }\nfunction h() {\n  var r =\n    g()\n  return r\n}\nR = h()",
  "\n\n\nR = new Error('x').stack",
  "R = new Error(\n'x'\n).stack",
  "var a = 1, b = new Error('x'); R = b.stack",
  "var e = new Error('x'); R = e.stack.split('\\n').length",
  "var e = new Error('x'); R = e.stack.split('\\n')[0]",
  "var e = new Error('x'); R = e.stack.split('\\n').slice(1).every(l => /^    at /.test(l))",
  "var e = new Error('x'); R = e.stack.indexOf('Error: x\\n    at')",
  "function f() { return new Error('x').stack }; R = f().split('\\n').map(l => l.replace(/:\\d+:\\d+/, ':L:C')).join('\\n')",
  "function f() { return new Error('x') }; var e = f(); R = e.stack.split('\\n')[1].trim().split(' ')[1]",
  "function f() { return new Error('x') }; R = f().stack.split('\\n')[1].replace(/\\d+/g, 'N')",
  "function f() { return new Error('x') }; R = f().stack.split('\\n')[1].match(/\\((.*)\\)/)[1]",
  "R = new Error('x').stack.split('\\n').length > 1",
  "function r(n) { return n ? r(n - 1) : new Error('x').stack }; R = r(5)",
  "function r(n) { return n ? r(n - 1) : new Error('x').stack }; R = r(12).split('\\n').length",
  "function r(n) { return n ? r(n - 1) : new Error('x').stack }; R = r(30)",
  "function a() { return b() }; function b() { return c() }; function c() { return new Error('x').stack }; R = a()",
  "function a() { return new Error('x').stack }; R = (0, a)()",
  "function a() { return new Error('x').stack }; R = (a)()",
  "function a() { return new Error('x').stack }; R = a?.()",
  "function a() { return new Error('x').stack }; var o = { a }; R = o?.a()",
  "function a() { return new Error('x').stack }; var o = { a }; R = o['a']()",
  "function a() { return new Error('x').stack }; var o = { a }; var k = 'a'; R = o[k]()",
  "function a() { return new Error('x').stack }; var t = a``; R = t",
  "function a() { return new Error('x').stack }; R = a(...[])",
  "function tag() { return new Error('x').stack }; R = tag`a${1}b`",
  "var s = Symbol('d'); var o = { [s]() { return new Error('x').stack } }; R = o[s]()",
  "var o = { 'a b'() { return new Error('x').stack } }; R = o['a b']()",
  "var o = { 1() { return new Error('x').stack } }; R = o[1]()",
  "Number.prototype.nm = function () { return new Error('x').stack }; R = (1).nm()",
  "Array.prototype.am = function () { return new Error('x').stack }; R = [].am()",
  "Object.prototype.om = function () { return new Error('x').stack }; R = ({}).om()",
  "class K { m() { return new Error('x').stack } }; class L extends K {}; R = new L().m()",
  "var K = class { m() { return new Error('x').stack } }; R = new K().m()",
  "var K = class Named { m() { return new Error('x').stack } }; R = new K().m()",
  "R = new Error('x').stack.includes('error_case.js')",
  "R = new Error('x', { cause: new Error('inner') }).stack.split('\\n').length",
  "var e = new Error('x'); var c = new Error('y', { cause: e }); R = c.stack.split('\\n')[0]",
  "try { try { null.x } catch (e) { throw new Error('outer', { cause: e }) } } catch (e) { R = e.stack.split('\\n')[0] + '|' + e.cause.stack.split('\\n')[0] }",
  "Promise.reject(new Error('x')).catch(e => { R = e.stack })",
  "new Promise(function () { throw new Error('x') }).catch(e => { R = e.stack })",
  "new Promise(function pexec() { throw new Error('x') }).catch(e => { R = e.stack })",
  "Promise.resolve().then(function th() { throw new Error('x') }).catch(e => { R = e.stack })",
  "Promise.resolve().then(() => { throw new Error('x') }).catch(e => { R = e.stack })",
  "queueMicrotask(function qm() { R = new Error('x').stack })",
  "Promise.resolve().then(function tm() { R = new Error('x').stack })",
];
add(...stackFmt);

// ---- 9. toString de erros e Error.prototype.toString em objetos quaisquer.
const names = ["undefined", "''", "'N'", "null", "1", "{}", "Symbol('s')", "'a b'", "true", "{ toString() { return 'ts' } }"];
const msgs = ["undefined", "''", "'M'", "null", "1", "{}", "'multi\\nline'", "{ toString() { return 'ms' } }", "false"];
for (const n of names) {
  add(T(`var e = new Error('m'); e.name = ${n}; R = String(e)`));
  add(T(`var e = new Error(); e.name = ${n}; R = String(e)`));
  add(T(`R = Error.prototype.toString.call({ name: ${n}, message: 'm' })`));
}
for (const m of msgs) {
  add(T(`var e = new Error('x'); e.message = ${m}; R = String(e)`));
  add(T(`R = Error.prototype.toString.call({ name: 'N', message: ${m} })`));
  add(T(`R = Error.prototype.toString.call({ message: ${m} })`));
  add(T(`R = Error.prototype.toString.call({ name: '', message: ${m} })`));
}
add(
  T("R = Error.prototype.toString.call({})"),
  T("R = Error.prototype.toString.call([])"),
  T("R = Error.prototype.toString.call(function () {})"),
  T("R = Error.prototype.toString.call(new Date(NaN))"),
  T("R = Error.prototype.toString.call(Object.create(null))"),
  T("R = Error.prototype.toString.call(Object.create({ name: 'PN', message: 'PM' }))"),
  T("R = Error.prototype.toString.call(1)"),
  T("R = Error.prototype.toString.call('s')"),
  T("R = Error.prototype.toString.call(true)"),
  T("R = Error.prototype.toString.call(undefined)"),
  T("R = Error.prototype.toString.call(null)"),
  T("R = Error.prototype.toString.call(Symbol())"),
  T("R = Error.prototype.toString.call(1n)"),
  T("R = Error.prototype.toString.call(new Proxy({ name: 'P', message: 'Q' }, {}))"),
  T("R = Error.prototype.toString.call({ get name() { throw new RangeError('gn') } })"),
  T("R = Error.prototype.toString.call({ name: 'a', get message() { throw new RangeError('gm') } })"),
  T("var log = []; Error.prototype.toString.call({ get name() { log.push('name'); return 'n' }, get message() { log.push('message'); return 'm' } }); R = log.join()"),
  T("R = Error.prototype.toString.call({ name: { toString() { throw new RangeError('nts') } }, message: 'm' })"),
  T("R = Error.prototype.toString.length + '|' + Error.prototype.toString.name"),
  T("R = String(new Error('a')) + '|' + String(new TypeError('a')) + '|' + String(new RangeError()) + '|' + String(new AggregateError([]))"),
  T("R = new Error('a') + ''"),
  T("R = `${new Error('a')}`"),
  T("R = [new Error('a'), new TypeError('b')].join()"),
  T("R = '' + [new Error('a')]"),
  T("R = Object.prototype.toString.call(new Error('a'))"),
  T("R = Object.prototype.toString.call(new TypeError('a'))"),
  T("R = Object.prototype.toString.call(Object.create(Error.prototype))"),
  T("class K extends Error {}; R = Object.prototype.toString.call(new K) + '|' + String(new K) + '|' + new K().name"),
  T("class K extends Error { get [Symbol.toStringTag]() { return 'KT' } }; R = Object.prototype.toString.call(new K)"),
  T("R = [Error.prototype.name, Error.prototype.message === '', TypeError.prototype.name, RangeError.prototype.name, SyntaxError.prototype.name, ReferenceError.prototype.name, URIError.prototype.name, EvalError.prototype.name].join()"),
  T("R = Object.getOwnPropertyNames(Error.prototype).sort().join()"),
  T("R = Object.getOwnPropertyNames(TypeError.prototype).sort().join()"),
  T("R = Object.getOwnPropertyNames(TypeError).sort().join()"),
  T("R = [Error.length, TypeError.length, Error.name, TypeError.name, AggregateError.length].join()"),
  T("R = [Object.getPrototypeOf(TypeError) === Error, Object.getPrototypeOf(TypeError.prototype) === Error.prototype, TypeError.prototype.constructor === TypeError].join()"),
  T("var d = Object.getOwnPropertyDescriptor(Error.prototype, 'name'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("var d = Object.getOwnPropertyDescriptor(Error.prototype, 'message'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("var d = Object.getOwnPropertyDescriptor(Error.prototype, 'toString'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("var d = Object.getOwnPropertyDescriptor(globalThis, 'Error'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("var d = Object.getOwnPropertyDescriptor(Error, 'prototype'); R = [d.writable, d.enumerable, d.configurable].join()"),
  T("R = Error.prototype.constructor === Error"),
  T("R = Object.prototype.hasOwnProperty.call(TypeError.prototype, 'message') + '|' + JSON.stringify(TypeError.prototype.message)"),
  T("R = new Error('a') == 'Error: a'"),
  T("R = String(new Error('a', { cause: 'b' }))"),
);

// ---- 10. instanceof entre classes derivadas.
const classes = ["Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError"];
for (const a of classes) for (const b of ["Error", "TypeError", "RangeError", "Object", "Function", "AggregateError"]) {
  add(T(`R = (new ${a}(${a === "AggregateError" ? "[]" : ""}) instanceof ${b})`));
}
add(
  T("class A extends Error {}; class B extends A {}; var b = new B('m'); R = [b instanceof A, b instanceof B, b instanceof Error, b instanceof TypeError, b.name, b.constructor.name, Object.getPrototypeOf(b) === B.prototype].join()"),
  T("class A extends TypeError {}; class B extends A {}; var b = new B('m'); R = [b instanceof A, b instanceof TypeError, b instanceof Error, b instanceof RangeError, b.name, String(b)].join()"),
  T("class A extends Error { constructor(m) { super(m); this.name = 'A' } }; class B extends A { constructor(m) { super(m); this.name = 'B' } }; R = String(new B('q')) + '|' + new B('q').stack.split('\\n')[0]"),
  T("class A extends Error {}; A.prototype.name = 'AP'; class B extends A {}; R = String(new B('q')) + '|' + new B('q').name"),
  T("function A(m) { var e = Reflect.construct(Error, [m], new.target); return e }; A.prototype = Object.create(Error.prototype); R = (new A('x') instanceof A) + '|' + (new A('x') instanceof Error)"),
  T("function A(m) { Error.call(this, m) }; A.prototype = Object.create(Error.prototype); var a = new A('x'); R = [a instanceof Error, a.message === '', Object.prototype.toString.call(a), 'stack' in a].join()"),
  T("function A(m) { this.message = m; Error.captureStackTrace(this, A) }; A.prototype = Object.create(Error.prototype); A.prototype.name = 'A'; var a = new A('x'); R = [a instanceof Error, String(a), a.stack.split('\\n')[0]].join()"),
  T("var e = Reflect.construct(Error, ['m'], TypeError); R = [e instanceof TypeError, e instanceof Error, e.name, Object.getPrototypeOf(e) === TypeError.prototype].join()"),
  T("var e = Reflect.construct(TypeError, ['m'], Error); R = [e instanceof TypeError, e instanceof Error, e.name].join()"),
  T("var e = Reflect.construct(Error, ['m'], Object); R = [e instanceof Object, e instanceof Error, Object.getPrototypeOf(e) === Object.prototype].join()"),
  T("var e = Reflect.construct(Error, ['m'], function () {}.bind()); R = [e instanceof Error, typeof e.stack].join()"),
  T("var nt = function () {}; nt.prototype = null; var e = Reflect.construct(Error, ['m'], nt); R = [Object.getPrototypeOf(e) === Error.prototype, e instanceof Error].join()"),
  T("var nt = function () {}; nt.prototype = 1; var e = Reflect.construct(TypeError, ['m'], nt); R = [Object.getPrototypeOf(e) === TypeError.prototype].join()"),
  T("Object.setPrototypeOf(Error.prototype, null); R = 'x'"),
  T("R = Error[Symbol.hasInstance] === Function.prototype[Symbol.hasInstance]"),
  T("R = ({}) instanceof Error"),
  T("R = Object.create(Error.prototype) instanceof Error"),
  T("R = (new Error) instanceof Object.getPrototypeOf(TypeError)"),
  T("R = Error.isError ? [Error.isError(new Error), Error.isError({}), Error.isError(Object.create(Error.prototype))].join() : 'no isError'"),
  T("R = typeof Error.isError"),
  T("R = Error.prototype.isPrototypeOf(new TypeError) + '|' + TypeError.prototype.isPrototypeOf(new Error)"),
  T("var e = new Error('x'); Object.setPrototypeOf(e, TypeError.prototype); R = [e instanceof TypeError, e.name, String(e)].join()"),
  T("var e = new TypeError('x'); Object.setPrototypeOf(e, null); R = [e instanceof Error, typeof e.stack, typeof e.message].join()"),
  T("try { Error() ; R = 'ok' } catch (e) { R = e.name }"),
  T("try { Error.call({}, 'a'); R = 'ok' } catch (e) { R = e.name }"),
  T("R = Error.call({}, 'a') instanceof Error"),
  T("R = Error.apply(null, ['a']).message"),
  T("class A extends Error { constructor() { try { this } catch (e) { R = e.name + ': ' + e.message } super() } }; new A"),
  T("class A extends Error { constructor() { } }; new A"),
  T("class A extends Error { constructor() { super(); super() } }; new A"),
  T("class A extends Error { constructor() { return 1 } }; new A"),
  T("class A extends Error { constructor() { return {} } }; R = (new A) instanceof Error"),
  T("class A extends null {}; new A"),
  T("class A extends Error {}; A()"),
  T("Error.prototype.constructor = 1; R = new Error('x').stack.split('\\n')[0]"),
  T("var e = new Error('x'); e.constructor = 1; R = e.stack.split('\\n')[0]"),
);

// ---- 11. TypeError nativo lançado por operações, com `evaluating '...'`.
const bases = [
  ["var x;", "x"], ["var x = null;", "x"], ["var x = undefined;", "x"],
  ["var o = {};", "o.p"], ["var o = { a: null };", "o.a"], ["var o = { a: {} };", "o.a.b"],
  ["var a = [];", "a[0]"], ["var a = [null];", "a[0]"], ["var f = function () {};", "f()"],
  ["var f = function () {}; var k = 'q';", "f()[k]"], ["var o = { f() { return null } };", "o.f()"],
  ["var o = { f() {} };", "o.f().g"], ["var a = [[]];", "a[0][1]"], ["var s = 'str';", "s.nope"],
  ["var n = 5;", "n.nope"],
];
const ops = [
  "B.y", "B.y.z", "B[0]", "B['k']", "B[k]", "B.y()", "B.y.z()", "B()", "new B", "new B()", "new B.y", "B.y = 1", "B[0] = 1",
  "B.y++", "++B.y", "B.y += 1", "delete B.y", "B.y?.z.w", "B.y.z = 2", "B[k]()", "B[0]()", "B`t`", "B.y`t`", "[...B]", "(function(){}).apply(null, B)",
];
const typeErrExprs = [];
for (const [setup, b] of bases) for (const op of ops) typeErrExprs.push(`var k = 'kk'; ${setup} ${op.split("B").join(b)}`);
// Reduz para ~150 variantes mantendo mistura (cada 3ª combinação de 375).
const typeErrChosen = typeErrExprs.filter((_, i) => i % 2 === 0);
for (const code of typeErrChosen) add(T(code));
add(
  T("var {a} = null"), T("var {a} = undefined"), T("var [a] = undefined"), T("var [a] = null"), T("var [a] = {}"), T("var [a] = 1"),
  T("var {a: {b}} = {}"), T("var { ...r } = null"), T("(function ({a}) {})()"), T("(function ([a]) {})()"), T("(function ({a}) {})(null)"),
  T("for (var i of undefined) ;"), T("for (var i of null) ;"), T("for (var i of 1) ;"), T("for (var i of {}) ;"), T("for (var i in null) R = 'ok'"),
  T("[...undefined]"), T("[...null]"), T("[...1]"), T("[...{}]"), T("(function () {})(...undefined)"), T("Math.max(...null)"), T("new Map(1)"), T("new Set(1)"),
  T("new Map([1])"), T("new WeakMap([[1, 1]])"), T("new WeakSet([1])"), T("Array.from(null)"), T("Object.keys(null)"), T("Object.entries(undefined)"),
  T("Object.assign(null)"), T("Object.defineProperty(1, 'a', {})"), T("Object.defineProperty({}, 'a', 1)"), T("Object.defineProperty({}, 'a', { get: 1 })"),
  T("Object.defineProperty({}, 'a', { get() {}, value: 1 })"), T("Object.setPrototypeOf(1)"), T("Object.setPrototypeOf({}, 1)"), T("Object.create(1)"), T("Object.freeze(1); Object.defineProperty(Object.freeze({}), 'a', { value: 1 })"),
  T("null instanceof 1"), T("({}) instanceof {}"), T("({}) instanceof (() => {})"), T("1 in 1"), T("'a' in 'b'"), T("'a' in null"), T("Symbol() + 1"), T("`${Symbol()}`"), T("+Symbol()"), T("Symbol() + ''"),
  T("BigInt(1.5)"), T("1n + 1"), T("BigInt('x')"), T("+1n"), T("1n >>> 0n"), T("new Symbol"), T("new BigInt(1)"), T("new Math"), T("new (() => {})"), T("new (async function () {})"), T("new (function* () {})"), T("new ({ m() {} }).m"),
  T("class A extends 1 {}"), T("class A extends (() => {}) {}"), T("class A extends null { constructor() { super() } }; new A"), T("class A { constructor() { this.x } }; A()"), T("class A {}; A()"), T("(class {})()"),
  T("var o = {}; o.f()"), T("var o = {}; o.a.b()"), T("var o = { a: 1 }; o.a()"), T("var o = { a: 1 }; o.a.b()"), T("1()"), T("'s'()"), T("({})()"), T("[]()"), T("null()"), T("undefined()"), T("(void 0)()"), T("(0, undefined)()"),
  T("var a = []; a[0]()"), T("var a = [1]; a[0]()"), T("var a = {}; a['b c']()"), T("var o = { 'a-b': 1 }; o['a-b']()"), T("var o = {}; o[Symbol('s')]()"), T("var o = {}; o[1]()"), T("var o = {}; o[1.5]()"),
  T("var s = Symbol('d'); var o = {}; o[s]()"), T("var o = { f: 1 }; new o.f"), T("var o = { f: 1 }; new o.f()"), T("var o = {}; new o.a.b"), T("var o = {}; new o.f"),
  T("'use strict'; undefinedVar"), T("undefinedVar = 1; 'use strict'"), T("(function () { 'use strict'; undefinedVar = 1 })()"), T("(function () { 'use strict'; NaN = 1 })()"), T("(function () { 'use strict'; undefined = 1 })()"),
  T("(function () { 'use strict'; Object.freeze({ a: 1 }).a = 2 })()"), T("(function () { 'use strict'; var o = Object.freeze({}); o.a = 1 })()"), T("(function () { 'use strict'; var o = { get a() { return 1 } }; o.a = 2 })()"),
  T("(function () { 'use strict'; delete Object.prototype })()"), T("(function () { 'use strict'; delete [].length })()"), T("(function () { 'use strict'; Object.defineProperty({}, 'a', { value: 1 }).a = 2 })()"),
  T("(function () { 'use strict'; 'str'.x = 1 })()"), T("(function () { 'use strict'; (1).x = 1 })()"), T("(function () { 'use strict'; 'str'.length = 1 })()"), T("(function () { 'use strict'; arguments.callee })()"), T("(function () { 'use strict'; (function () {}).caller })()"),
  T("(function () { 'use strict'; Object.preventExtensions({}).a = 1 })()"), T("(function () { 'use strict'; var s = Symbol(); s.x = 1 })()"), T("(function () { 'use strict'; var a = Object.freeze([1]); a.push(2) })()"), T("Object.freeze([1]).push(2)"), T("Object.freeze([1]).pop()"),
  T("let a = 1; { a; let a = 2 }"), T("a; let a = 1"), T("const c = 1; c = 2"), T("const c = 1; c++"), T("{ b; const b = 1 }"), T("class A { constructor() { this.x; } }; class B extends A { constructor() { this.y; super() } }; new B"),
  T("new Array(-1)"), T("new Array(1.5)"), T("[].length = -1"), T("'a'.repeat(-1)"), T("'a'.repeat(Infinity)"), T("(1).toFixed(101)"), T("(1).toString(1)"), T("(1).toPrecision(0)"), T("(1).toExponential(-1)"), T("new ArrayBuffer(-1)"), T("new Uint8Array(-1)"),
  T("decodeURIComponent('%')"), T("decodeURI('%E0%A4%A')"), T("encodeURI('\\ud800')"), T("encodeURIComponent('\\udc00')"), T("new RegExp('(')"), T("new RegExp('[')"), T("new RegExp('a', 'zz')"), T("/a/.test.call(1)"), T("JSON.parse('{')"), T("JSON.parse('')"),
  T("JSON.parse('[1,]')"), T("JSON.parse(undefined)"), T("var a = {}; a.a = a; JSON.stringify(a)"), T("JSON.stringify(1n)"), T("eval('var')"), T("eval('1 +')"), T("eval('}')"), T("new Function('}')"), T("new Function('a', 'return')"), T("eval('let a; let a')"),
  T("eval('break')"), T("eval('return 1')"), T("eval('x = ')"), T("eval('1 = 2')"), T("eval('if')"), T("eval('\"')"), T("eval('/')"), T("eval('`')"), T("eval('(')"), T("eval('[')"), T("eval('{')"), T("eval('a b')"), T("eval('1a')"), T("eval('\\\\')"),
  T("eval('class')"), T("eval('function')"), T("eval('async')"), T("eval('yield 1')"), T("eval('await 1')"), T("eval('new.target')"), T("eval('super()')"), T("eval('import.meta')"), T("eval('a => {')"), T("eval('for (;;')"),
  T("(function f(a, a) { 'use strict' })"), T("eval('(function f(a, a) { \"use strict\" })')"), T("eval('\"use strict\"; with ({}) {}')"), T("eval('\"use strict\"; var eval')"), T("eval('\"use strict\"; 010')"),
  T("(function () { return this.x })()"), T("(function () { 'use strict'; return this.x })()"), T("(() => this.x.y)()"), T("var o = { f() { return this.x.y } }; o.f()"),
  T("Reflect.ownKeys(1)"), T("Reflect.construct(1)"), T("Reflect.apply(1)"), T("Reflect.get(1, 'a')"), T("new Proxy(1, {})"), T("new Proxy({}, 1)"), T("Proxy({}, {})"), T("Proxy.revocable(1, {})"),
  T("var p = Proxy.revocable({}, {}); p.revoke(); p.proxy.a"), T("var p = Proxy.revocable(function () {}, {}); p.revoke(); p.proxy()"),
  T("new Proxy({}, { get: 1 }).a"), T("Object.keys(new Proxy({}, { ownKeys() { return 1 } }))"), T("Object.keys(new Proxy({}, { ownKeys() { return [1] } }))"), T("Object.getPrototypeOf(new Proxy({}, { getPrototypeOf() { return 1 } }))"),
  T("new Promise()"), T("new Promise(1)"), T("Promise()"), T("Promise.resolve.call(1)"), T("Promise.all.call(1)"), T("new Promise(function () {}).then.call(1)"),
  T("[].reduce(function () {})"), T("[].reduceRight(function () {})"), T("[].map(1)"), T("[].forEach()"), T("[].sort(1)"), T("[].find(null)"), T("[].flatMap(1)"), T("Array.prototype.map.call(null)"), T("Array.prototype.push.call(null)"),
  T("'abc'.localeCompare.call(null)"), T("String.prototype.trim.call(null)"), T("String.prototype.toString.call(1)"), T("Number.prototype.toString.call('a')"), T("Boolean.prototype.valueOf.call(1)"), T("Symbol.prototype.toString.call(1)"),
  T("Date.prototype.getTime.call({})"), T("new Date().toISOString.call(1)"), T("new Date(NaN).toISOString()"), T("Map.prototype.get.call({}, 1)"), T("Set.prototype.add.call([], 1)"), T("WeakMap.prototype.set.call(new WeakMap, 1, 1)"), T("new WeakSet().add(1)"), T("new WeakRef(1)"),
  T("Function.prototype.call.call(1)"), T("Function.prototype.apply.call(1)"), T("Function.prototype.bind.call(1)"), T("Function.prototype.toString.call({})"), T("(function () {}).apply(null, 1)"), T("(function () {}).call.apply(1)"),
  T("Symbol.keyFor(1)"), T("Symbol.prototype.description"), T("Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.call(1)"), T("Atomics.add(1, 0, 0)"), T("new SharedArrayBuffer(-1)"), T("new DataView(1)"), T("new DataView(new ArrayBuffer(1), 2)"),
  T("new Intl.NumberFormat('xx-invalid-')"), T("new Intl.DateTimeFormat('en', { timeZone: 'Nope' })"), T("(1).toLocaleString('en', { style: 'currency' })"), T("'a'.normalize('x')"), T("new Intl.Locale()"), T("Intl.getCanonicalLocales('x_y')"),
  T("structuredClone(function () {})"), T("structuredClone(Symbol())"), T("new TextDecoder('nope')"), T("atob('*')"), T("new URL('x')"), T("new URL('http://')"),
  T("function f() { f() }; f()"), T("var a = []; a[0] = a; String(Array(1e9).join('x'))"),
  T("(function () { var x; x.y })()"), T("(function (x) { x.y })()"), T("(function (x) { x.y.z })({})"), T("((x) => x.y)()"), T("((x) => x.y.z)({})"), T("(async function () { var x; x.y })().catch(e => { R = e.name + ': ' + e.message })"),
  T("(function* () { var x; x.y })().next()"), T("var o = { get g() { var x; return x.y } }; o.g"), T("class A { m() { var x; x.y } }; new A().m()"), T("class A { static m() { var x; x.y } }; A.m()"), T("class A { #p; m() { return this.#p.q } }; new A().m()"),
  T("class A { #p = 1; static m(o) { return o.#p } }; A.m({})"), T("class A { #p = 1; static m(o) { o.#p = 1 } }; A.m({})"), T("class A { #m() {} static c(o) { o.#m() } }; A.c({})"), T("class A { static #p = 1; static m(o) { return #p in o } }; A.m(1)"),
  T("var o = {}; o?.a.b"), T("var o = null; o?.a.b; o.a"), T("var o = { a: null }; o?.a.b"), T("var f; f?.()(); f()"), T("var o = {}; o.f?.(); o.f()"), T("var o = { a: undefined }; o.a?.b.c; o.a.b"),
  T("var x; x.y.z"), T("var x = {}; x.y.z"), T("var x = { y: {} }; x.y.z.w"), T("var a = [1]; a[1].x"), T("var a = []; a[0].x = 1"), T("var s; s.length"), T("var s; s[0]"), T("var s; s['a']"), T("var s; s.a.b.c.d"),
  T("document.body"), T("window.x"), T("undefinedVar.nope.x"), T("globalThis.nope.x"), T("nope.x"), T("typeof nope.x"), T("nope()"), T("new nope"), T("nope++"), T("nope += 1"), T("delete nope.x"), T("void nope"), T("-nope"), T("[nope]"), T("({ a: nope })"), T("`${nope}`"),
  T("var x; x.y; "), T("x = {}; x.y.z"), T("var o = { a: 1 }; o.b.c"), T("var o = { a: 1 }; o.a.b.c"), T("var o = { a: [] }; o.a[0].c"), T("var o = { a: [] }; o.a.b.c"), T("var o = {}; o['a']['b']"), T("var o = {}; var k = 'a'; o[k][k]"), T("var o = {}; o[1][2]"), T("var o = {}; o[0.5].x"),
  T("var a = {}, b = {}; a.x.y"), T("var a = { b: {} }; a.b.c.d()"), T("var a = { b: {} }; a.b.c()"), T("var a = { b: {} }; a['b']['c']()"), T("var a = { b: {} }; var k = 'c'; a.b[k]()"), T("var a = { b() {} }; a.b().c()"), T("var a = { b() {} }; a.b().c"),
  T("(function () {})().x"), T("(function () {})()()"), T("(() => {})().x"), T("(async () => {})().x.y"), T("[1].map(function () {}).x.y"), T("[].x.y"), T("({}).x.y"), T("({}).x()"), T("'a'.x()"), T("'a'.x.y"), T("(1).x()"), T("(1).x.y"), T("true.x.y"), T("Symbol().x.y"), T("1n.x.y"),
  T("Math.nope()"), T("Math.nope.x"), T("JSON.nope()"), T("Object.nope()"), T("Array.nope()"), T("undefinedVar.nope()"),T("String.nope()"), T("Number.nope.x"), T("Date.nope()"), T("Promise.nope()"), T("Reflect.nope()"), T("Symbol.nope()"), T("Intl.nope()"), T("BigInt.nope()"),
);

// ---- 12. Tipos de erro nativos com mensagens exatas do JSC.
add(
  T("undefinedVariable"), T("undefinedVariable = 1; R = 'sloppy'"), T("(function () { 'use strict'; undefinedVariable = 1 })()"), T("typeof undefinedVariable; undefinedVariable"), T("let q = q"), T("{ x; let x }"), T("const k = k"),
  T("class A extends A {}"), T("class A { constructor() { this } }; class B extends A { constructor() { this } }; new B"), T("(0, eval)('undefinedVariable2')"), T("eval('undefinedVariable3')"), T("new Function('return undefinedVariable4')()"),
  T("new Array(-1)"), T("new Array(2 ** 32)"), T("[].length = 2 ** 32"), T("'a'.repeat(2 ** 31)"), T("new ArrayBuffer(2 ** 53)"), T("(1).toFixed(-1)"), T("(1).toString(37)"), T("(1).toString(1)"), T("(1).toPrecision(101)"), T("(1).toExponential(101)"),
  T("'a'.padStart(2 ** 31)"), T("'a'.normalize('x')"), T("new Date(NaN).toISOString()"), T("BigInt(1.5)"), T("BigInt(NaN)"), T("BigInt(Infinity)"), T("1n / 0n"), T("1n % 0n"), T("2n ** -1n"), T("BigInt.asIntN(-1, 1n)"), T("BigInt.asUintN(2 ** 53, 1n)"),
  T("new Uint8Array(new ArrayBuffer(8), 9)"), T("new Uint16Array(new ArrayBuffer(8), 1)"), T("new Uint16Array(new ArrayBuffer(7))"), T("new DataView(new ArrayBuffer(1)).getInt32(0)"), T("new DataView(new ArrayBuffer(1), 2)"), T("new Uint8Array(-1)"),
  T("Array.prototype.concat.call(null)"), T("[].splice.call(Object.freeze([1]), 0, 1)"), T("new Intl.NumberFormat('en', { maximumFractionDigits: 101 })"), T("new Intl.NumberFormat('en', { style: 'bad' })"), T("Intl.getCanonicalLocales('en_US')"), T("new Intl.DateTimeFormat('en', { timeZone: 'X/Y' })"),
  T("function f() { f() }; f()"), T("function f() { return f() }; f()"), T("var o = {}; o.o = o; JSON.stringify(o)"), T("var a = []; a.push(a); a.flat(Infinity)"), T("(function f(n) { return f(n + 1) + 1 })(0)"),
  T("decodeURIComponent('%')"), T("decodeURIComponent('%E0')"), T("decodeURI('%')"), T("decodeURI('%zz')"), T("encodeURI('\\ud800')"), T("encodeURIComponent('\\udfff')"), T("encodeURI('\\ud800a')"), T("decodeURIComponent('%C0%80')"), T("decodeURIComponent('%ED%A0%80')"),
  T("eval('1 +')"), T("eval('var')"), T("eval('let let')"), T("eval('}')"), T("eval(')')"), T("eval('1 2')"), T("eval('a b')"), T("eval('if (')"), T("eval('for (')"), T("eval('function')"), T("eval('function (')"), T("eval('(function')"), T("eval('x = {')"), T("eval('[1,')"),
  T("eval('\"abc')"), T("eval(\"'abc\")"), T("eval('`abc')"), T("eval('/abc')"), T("eval('/(/')"), T("eval('0b2')"), T("eval('0x')"), T("eval('1_')"), T("eval('1__0')"), T("eval('08.5')"), T("eval('\"use strict\"; 08')"), T("eval('\\\\u')"), T("eval('a\\\\u0')"),
  T("eval('let a; var a')"), T("eval('const a')"), T("eval('const a = 1; const a = 2')"), T("eval('class A {}; class A {}')"), T("eval('break')"), T("eval('continue')"), T("eval('return')"), T("eval('x: x: 1')"), T("eval('a: { break b }')"),
  T("eval('new.target')"), T("eval('super.x')"), T("eval('super()')"), T("eval('yield')"), T("eval('(yield)')"), T("eval('function* g() { yield = 1 }')"), T("eval('async function f() { await = 1 }')"), T("eval('await 1')"), T("eval('import x from \"y\"')"), T("eval('export var x')"),
  T("eval('1 = 1')"), T("eval('++1')"), T("eval('1++')"), T("eval('a++ = 1')"), T("eval('for (1 of []) ;')"), T("eval('({a:1} = 1)')"), T("eval('({a}) = 1')"), T("eval('[a] += 1')"), T("eval('(a, b) = 1')"), T("eval('async () => await')"), T("eval('delete x', 1); eval('\"use strict\"; delete x')"),
  T("eval('\"use strict\"; with (a) {}')"), T("eval('\"use strict\"; eval = 1')"), T("eval('\"use strict\"; arguments = 1')"), T("eval('\"use strict\"; var yield')"), T("eval('\"use strict\"; function f(a, a) {}')"), T("eval('\"use strict\"; delete a')"), T("eval('\"use strict\"; 010')"), T("eval('\"use strict\"; \"\\\\01\"')"),
  T("eval('function f(a = 1) { \"use strict\" }')"), T("eval('(a, a) => 1')"), T("eval('(...a, b) => 1')"), T("eval('function f(...a, b) {}')"), T("eval('({ get a(x) {} })')"), T("eval('({ set a() {} })')"), T("eval('class A { constructor() {} constructor() {} }')"), T("eval('class A { get constructor() {} }')"), T("eval('class A { static prototype() {} }')"),
  T("eval('class A { #a; #a }')"), T("eval('class A { m() { this.#b } }')"), T("eval('class A extends B { constructor() { super() } }; class C { constructor() { super() } }')"), T("eval('({ a: 1, a: 2, __proto__: 1, __proto__: 2 })')"), T("eval('/a/gg')"), T("eval('/(?<a>.)(?<a>.)/')"), T("eval('/\\\\k<a>/u')"), T("eval('/a{2,1}/')"), T("eval('/[b-a]/')"),
  T("new RegExp('(')"), T("new RegExp(')')"), T("new RegExp('[')"), T("new RegExp('*')"), T("new RegExp('a**')"), T("new RegExp('\\\\')"), T("new RegExp('a', 'gg')"), T("new RegExp('a', 'x')"), T("new RegExp('(?<a>.)(?<a>.)')"), T("new RegExp('\\\\k<a>', 'u')"), T("new RegExp('[b-a]')"), T("new RegExp('a{2,1}')"), T("new RegExp('(?<=a)+', 'u')"), T("new RegExp('\\\\p{Nope}', 'u')"), T("new RegExp('{', 'u')"), T("new RegExp('(?', '')"),
  T("'a'.matchAll(/a/)"), T("'a'.replaceAll(/a/, '')"), T("RegExp.prototype.exec.call({}, '')"), T("RegExp.prototype.test.call(1)"), T("RegExp.prototype.global"), T("Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get.call({})"),
  T("JSON.parse('{')"), T("JSON.parse('{a:1}')"), T("JSON.parse(\"{'a':1}\")"), T("JSON.parse('[1,]')"), T("JSON.parse('')"), T("JSON.parse('undefined')"), T("JSON.parse('01')"), T("JSON.parse('1.')"), T("JSON.parse('\"\\\\x\"')"), T("JSON.parse('\"\\n\"')"), T("JSON.parse('nul')"), T("JSON.parse('[1 2]')"), T("JSON.parse('{\"a\" 1}')"), T("JSON.parse('{\"a\":}')"), T("JSON.parse('1 2')"), T("JSON.parse('\\u0000')"), T("JSON.parse('[')"), T("JSON.parse('\"abc')"),
  T("null.x"), T("undefined.x"), T("null[0]"), T("null()"), T("new null"), T("new undefined"), T("new 1"), T("new ''"), T("new {}"), T("new []"), T("(1)()"), T("(1).x()"), T("'x'()"), T("true()"),
  T("Symbol() + 1"), T("Symbol() * 1"), T("`${Symbol()}`"), T("String(Symbol()) + Symbol()"), T("Symbol().toString.call(1)"), T("new Symbol()"), T("Symbol.keyFor('a')"), T("Object(Symbol()) + ''"), T("[Symbol()].join()"), T("({}) + Symbol()"), T("Symbol() < 1"), T("Symbol() == 1; Symbol() + ''"),
  T("1n + 1"), T("1n * 1"), T("+1n"), T("Math.max(1n)"), T("1n >>> 1n"), T("BigInt('a')"), T("BigInt(undefined)"), T("BigInt(null)"), T("BigInt(Symbol())"), T("BigInt({})"), T("new BigInt(1)"), T("BigInt.prototype.toString.call(1)"), T("1n < 1; 1n + undefined"), T("BigInt(1e400)"), T("1n ** 1000000000000n"),
  T("({}).x.y"), T("({}).x()"), T("[].x()"), T("[].x.y"), T("var o; o.x"), T("var o; o.x()"), T("var o; o[0]"), T("var o; o[0]()"), T("var o; o.x.y"), T("var o; o.x = 1"), T("var o = null; o.x = 1"), T("var o = null; o[0] = 1"), T("var o; o.x++"), T("var o; o.x += 1"), T("var o; delete o.x"),
  T("var f = 1; f()"), T("var f = {}; f()"), T("var f = {}; f.g()"), T("var f = {}; f.g.h()"), T("var f = []; f[0]()"), T("var f = []; f.g()"), T("var f = 'a'; f()"), T("var f = null; f()"), T("var f; f()"), T("var f = Symbol(); f()"), T("var f = 1n; f()"),
  T("var f = 1; new f"), T("var f = {}; new f"), T("var f = () => {}; new f"), T("var f = async function () {}; new f"), T("var f = function* () {}; new f"), T("var f = { m() {} }; new f.m"), T("var f = Math.max; new f"), T("var f = parseInt; new f"), T("new Math.max()"), T("new JSON.parse('1')"),
  T("class A {}; A()"), T("class A { constructor() {} }; A.call({})"), T("class A { static m() {} }; new A.m"), T("class A extends Object {}; A()"), T("class A extends 1 {}"), T("class A extends {} {}"), T("class A extends (function () {}).bind() {}"), T("var f = function () {}; f.prototype = 1; class A extends f {}"), T("class A extends Symbol {}; new A"), T("class A extends null {}; new A"),
  T("Object.defineProperty(1, 'a', {})"), T("Object.defineProperty({}, 1, 1)"), T("Object.defineProperties({}, null)"), T("Object.create()"), T("Object.create(1)"), T("Object.setPrototypeOf({})"), T("Object.setPrototypeOf(null, {})"), T("Object.getPrototypeOf(null)"), T("Object.getPrototypeOf(undefined)"), T("Object.keys(undefined)"), T("Object.fromEntries(1)"), T("Object.fromEntries([1])"), T("Object.freeze(new Uint8Array(1))"),
  T("Object.defineProperty(Object.freeze({}), 'a', { value: 1 })"), T("Object.defineProperty(Object.preventExtensions({}), 'a', { value: 1 })"), T("var o = {}; Object.defineProperty(o, 'a', { value: 1 }); Object.defineProperty(o, 'a', { value: 2 })"), T("var o = {}; Object.defineProperty(o, 'a', { get() {}, value: 1 })"), T("Object.defineProperty({}, 'a', { get: 1 })"), T("Object.defineProperty({}, 'a', { set: 1 })"),
  T("Object.setPrototypeOf(Object.preventExtensions({}), {})"), T("var a = {}, b = Object.create(a); Object.setPrototypeOf(a, b)"), T("Object.prototype.__proto__ = {}"), T("Object.setPrototypeOf(Object.prototype, {})"), T("Reflect.setPrototypeOf(1, {})"), T("Reflect.defineProperty(1, 'a', {})"),
  T("[].reduce((a, b) => a)"), T("[].reduceRight((a, b) => a)"), T("[].map()"), T("[].map(1)"), T("[].forEach(null)"), T("[].filter({})"), T("[].some('a')"), T("[].every(Symbol())"), T("[].find(1)"), T("[].findIndex(1)"), T("[].findLast(1)"), T("[].flatMap(1)"), T("[].sort(1)"), T("[].sort(null)"), T("[].toSorted(1)"), T("[].with(0, 1)"), T("[].at.call(null)"), T("Array.from({}, 1)"), T("Array.from(1, 1)"), T("Array.of.call(1)"), T("new Array(1).fill.call(null)"),
  T("new Map(1)"), T("new Map([1])"), T("new Map({})"), T("new Set(1)"), T("new Set({})"), T("new WeakMap([1])"), T("new WeakSet(1)"), T("new WeakMap().set(1, 1)"), T("new WeakSet().add('a')"), T("new WeakRef(1)"), T("new FinalizationRegistry(1)"), T("Map()"), T("Set()"), T("WeakMap()"), T("Map.prototype.get.call({})"), T("Map.prototype.size"), T("Set.prototype.has.call(new Map, 1)"), T("Map.groupBy(1, 1)"), T("Object.groupBy(null, 1)"),
  T("new Promise()"), T("new Promise(1)"), T("Promise()"), T("Promise.resolve.call(1)"), T("Promise.reject.call({})"), T("Promise.all.call(1)"), T("Promise.prototype.then.call(1)"), T("Promise.prototype.finally.call(1)"), T("new Promise(() => {}).then.call({})"),
  T("new Proxy(1, {})"), T("new Proxy({}, 1)"), T("Proxy()"), T("Proxy.revocable()"), T("var r = Proxy.revocable({}, {}); r.revoke(); r.proxy.a"), T("var r = Proxy.revocable({}, {}); r.revoke(); new Proxy(r.proxy, {})"), T("new Proxy({}, { get: 1 }).a"), T("new Proxy({}, { has: 1 }); 'a' in new Proxy({}, { has: 1 })"),
  T("var p = new Proxy(Object.freeze({ a: 1 }), { get() { return 2 } }); p.a"), T("var p = new Proxy({}, { ownKeys() { return [1] } }); Object.keys(p)"), T("var p = new Proxy({}, { ownKeys() { return ['a', 'a'] } }); Object.keys(p)"), T("var p = new Proxy({}, { getPrototypeOf() { return 1 } }); Object.getPrototypeOf(p)"), T("var p = new Proxy({}, { set() { return false } }); (function () { 'use strict'; p.a = 1 })()"), T("var p = new Proxy({}, { deleteProperty() { return false } }); (function () { 'use strict'; delete p.a })()"), T("var p = new Proxy({}, { defineProperty() { return false } }); Object.defineProperty(p, 'a', {})"), T("var p = new Proxy({}, { setPrototypeOf() { return false } }); Object.setPrototypeOf(p, null)"), T("var p = new Proxy({}, { preventExtensions() { return false } }); Object.preventExtensions(p)"),
  T("Reflect.ownKeys(1)"), T("Reflect.get(1, 'a')"), T("Reflect.has(1, 'a')"), T("Reflect.construct(1, [])"), T("Reflect.construct(function () {}, 1)"), T("Reflect.apply(1, null, [])"), T("Reflect.apply(function () {}, null, 1)"), T("Reflect.getPrototypeOf(1)"), T("Reflect.set(1, 'a', 1)"), T("Reflect.deleteProperty(1, 'a')"),
  T("Function.prototype.call.call(1)"), T("Function.prototype.apply.call({})"), T("Function.prototype.bind.call(1)"), T("Function.prototype.toString.call(1)"), T("Function.prototype.toString.call({})"), T("(function () {}).apply(null, 1)"), T("(function () {}).apply(null, 'a')"), T("Function.prototype[Symbol.hasInstance].call(1, {})"), T("1 instanceof 1"), T("({}) instanceof {}"), T("({}) instanceof (() => {})"), T("({}) instanceof Math"), T("var f = function () {}; f.prototype = 1; ({}) instanceof f"), T("({}) instanceof { [Symbol.hasInstance]: 1 }"),
  T("1 in 1"), T("'a' in 'a'"), T("'a' in 1"), T("'a' in null"), T("'a' in undefined"), T("Symbol() in 1"), T("#a in {}"),
  T("var {a} = null"), T("var {a} = undefined"), T("var {} = null"), T("var [a] = null"), T("var [a] = {}"), T("var [a] = 1"), T("var [] = 1"), T("var [a] = Symbol()"), T("var [a] = { [Symbol.iterator]: 1 }"), T("var [a] = { [Symbol.iterator]() { return 1 } }"), T("var [a] = { [Symbol.iterator]() { return {} } }"), T("var [a] = { [Symbol.iterator]() { return { next() { return 1 } } } }"), T("for (var x of 1) ;"), T("for (var x of {}) ;"), T("for (var x of { [Symbol.iterator]() { return { next() { return 1 } } } }) ;"),
  T("[...1]"), T("[...{}]"), T("[...null]"), T("f(...1); function f() {}"), T("f(...{}); function f() {}"), T("Math.max(...undefined)"), T("new Date(...null)"), T("({ ...null }); [...{ [Symbol.iterator]() { return 1 } }]"), T("function* g() { yield* 1 }; g().next()"), T("function* g() { yield* {} }; g().next()"), T("async function* g() { yield* 1 }; g().next().catch(e => { R = e.name + ': ' + e.message })"), T("Array.from({ [Symbol.iterator]: 1 })"), T("new Map({ [Symbol.iterator]: 1 })"), T("Promise.all(1).catch(e => { R = e.name + ': ' + e.message })"), T("Promise.all().catch(e => { R = e.name + ': ' + e.message })"), T("Promise.allSettled(1).catch(e => { R = e.name + ': ' + e.message })"), T("Promise.race(1).catch(e => { R = e.name + ': ' + e.message })"), T("Promise.any([]).catch(e => { R = e.name + ': ' + e.message + '|' + e.errors.length })"), T("Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => { R = e.name + ': ' + e.message + '|' + e.errors.join() })"),
  T("function* g() { yield 1 }; var it = g(); it.next(); it.next(); it.throw(new RangeError('t'))"), T("function* g() { it.next() }; var it = g(); it.next()"), T("function* g() {}; g.prototype.next.call({})"), T("var it = (function* () { yield 1 })(); it.return.call(1)"), T("[][Symbol.iterator]().next.call({})"), T("new Map()[Symbol.iterator]().next.call([][Symbol.iterator]())"), T("'a'[Symbol.iterator]().next.call(1)"),
  T("let a = 1; let a = 2"), T("let a; { var a }"), T("const a = 1; a = 2"), T("const a = 1; a++"), T("const a = 1; a += 1"), T("const a = 1; ({ a } = { a: 2 })"), T("const a = 1; [a] = [2]"), T("const a = 1; for (a of [1]) ;"), T("const a = 1; for (a in { x: 1 }) ;"), T("class A { m() { A = 1 } }; new A().m()"), T("class A {}; A = 1; R = typeof A"), T("(function f() { 'use strict'; f = 1 })()"), T("(function f() { f = 1; R = typeof f })()"),
  T("{ a; let a }"), T("{ typeof a; let a }"), T("{ a = 1; let a }"), T("{ a++; let a }"), T("{ (() => a)(); let a }"), T("{ class B extends a {}; let a }"), T("function f(a = b, b) {}; f()"), T("function f(a = a) {}; f()"), T("(function (a = this) {})(); class A extends Object { constructor(a = this) { super() } }; new A"), T("class A { x = this.y.z }; new A"), T("class A { static x = null.y }"),
  T("class A { constructor() { this.a } }; class B extends A { constructor() { this.a; super() } }; new B"), T("class A {}; class B extends A { constructor() { } }; new B"), T("class A {}; class B extends A { constructor() { super(); super() } }; new B"), T("class A {}; class B extends A { constructor() { return 1 } }; new B"), T("class A {}; class B extends A { constructor() { return undefined } }; new B"), T("class A { constructor() { return 1 } }; R = typeof new A"), T("class A extends Object { constructor() { var f = () => super(); f(); f() } }; new A"),
  T("class A { #x = 1; static g(o) { return o.#x } }; A.g({})"), T("class A { #x = 1; static s(o) { o.#x = 1 } }; A.s({})"), T("class A { #m() {} static c(o) { o.#m() } }; A.c({})"), T("class A { get #g() { return 1 } static r(o) { return o.#g } }; A.r({})"), T("class A { set #s(v) {} static r(o) { return o.#s } }; A.r(new A)"), T("class A { get #g() { return 1 } static w(o) { o.#g = 1 } }; A.w(new A)"), T("class A { #m() {} static w(o) { o.#m = 1 } }; A.w(new A)"), T("class A { #x; constructor(o) { return o } }; class B extends A { #y; constructor(o) { super(o) } }; var o = {}; new B(o); new B(o)"),
  T("var o = { get a() { throw new RangeError('g') } }; o.a"), T("var o = { toString() { throw new RangeError('ts') } }; String(o)"), T("var o = { valueOf() { return {} }, toString() { return {} } }; o + 1"), T("var o = { [Symbol.toPrimitive]() { return {} } }; o + 1"), T("var o = { [Symbol.toPrimitive]: 1 }; o + 1"), T("var o = Object.create(null); o + 1"), T("`${Object.create(null)}`"), T("String(Object.create(null))"), T("Object.create(null) + ''"), T("[Object.create(null)].join()"), T("Number(Object.create(null))"), T("parseInt(Object.create(null))"), T("Math.abs(Object.create(null))"), T("new Date(Object.create(null))"), T("'a'.concat(Object.create(null))"),
  T("var a = []; a.length = -1"), T("var a = []; a.length = 1.5"), T("var a = []; a.length = 'x'"), T("var a = []; a.length = 2 ** 32"), T("var a = Object.freeze([]); (function () { 'use strict'; a.length = 1 })()"), T("var a = Object.freeze([1]); (function () { 'use strict'; a[0] = 2 })()"), T("var a = Object.freeze([1]); (function () { 'use strict'; a[1] = 2 })()"), T("var a = [1]; Object.defineProperty(a, 'length', { writable: false }); (function () { 'use strict'; a.push(1) })()"), T("var a = [1]; Object.defineProperty(a, 'length', { writable: false }); a.push(1)"), T("var a = [1]; Object.defineProperty(a, 'length', { writable: false }); a.pop()"), T("[].concat.call(null)"), T("Array.prototype.concat.call(undefined)"), T("[].lastIndexOf.call(null)"), T("Array.prototype.join.call(undefined)"),
  T("'a'.at.call(null)"), T("String.prototype.at.call(undefined)"), T("String.prototype.trim.call(null)"), T("String.prototype.toUpperCase.call(undefined)"), T("String.prototype.slice.call(null)"), T("String.prototype.valueOf.call(1)"), T("String.prototype.toString.call({})"), T("String.fromCodePoint(-1)"), T("String.fromCodePoint(1.5)"), T("String.fromCodePoint(0x110000)"), T("String.fromCodePoint(NaN)"), T("String.raw()"), T("String.raw(null)"), T("'a'.localeCompare('b', 'xx-bad')"), T("'a'.toLocaleUpperCase('x_y')"), T("'abc'.isWellFormed.call(null)"),
  T("Number.prototype.toString.call('a')"), T("Number.prototype.valueOf.call({})"), T("Number.prototype.toFixed.call('a')"), T("Boolean.prototype.toString.call(1)"), T("Boolean.prototype.valueOf.call({})"), T("Symbol.prototype.valueOf.call(1)"), T("Symbol.prototype.toString.call({})"), T("BigInt.prototype.valueOf.call(1)"), T("Date.prototype.getTime.call({})"), T("Date.prototype.toString.call(1)"), T("Date.prototype.toISOString.call(new Date(NaN))"), T("new Date(NaN).toJSON(); Date.prototype.toJSON.call({ toISOString: 1 })"), T("Date.prototype[Symbol.toPrimitive].call(1)"), T("new Date()[Symbol.toPrimitive]('x')"), T("new Date()[Symbol.toPrimitive]()"), T("Date.prototype.setTime.call({})"),
  T("ArrayBuffer(1)"), T("new ArrayBuffer(-1)"), T("ArrayBuffer.prototype.slice.call({})"), T("new Uint8Array(-1)"), T("Uint8Array(1)"), T("new Uint8Array({ length: -1 })"), T("new Uint8Array(1).set([1, 2])"), T("new Uint8Array(1).set([1], 2)"), T("new Uint8Array(1).set([1], -1)"), T("Uint8Array.prototype.fill.call([])"), T("Uint8Array.prototype.length"), T("new Uint8Array(new ArrayBuffer(1), 0, 2)"), T("new Float64Array(new ArrayBuffer(7))"), T("new BigInt64Array([1])"), T("new Uint8Array([1n])"), T("new BigInt64Array(1).fill(1)"), T("new Uint8Array(1).fill(1n)"), T("var u = new Uint8Array(1); u.buffer.transfer(); u.length; u.fill(1)"), T("new Uint8Array(1).subarray.call({})"), T("new Uint8Array(1).with(2, 1)"), T("new Uint8Array(1).with(0, 1n)"), T("new Uint8Array(1).toSorted(1)"), T("new Uint8Array(1).map(1)"),
  T("new DataView(1)"), T("new DataView(new ArrayBuffer(1), -1)"), T("new DataView(new ArrayBuffer(1), 0, 2)"), T("DataView(new ArrayBuffer(1))"), T("new DataView(new ArrayBuffer(1)).getInt8(1)"), T("new DataView(new ArrayBuffer(1)).setInt8(-1, 0)"), T("new DataView(new ArrayBuffer(1)).getBigInt64(0)"), T("new DataView(new ArrayBuffer(8)).setBigInt64(0, 1)"), T("DataView.prototype.getInt8.call({})"), T("new SharedArrayBuffer(-1)"), T("Atomics.add(new Uint8Array(1), 1, 1)"), T("Atomics.add([], 0, 1)"), T("Atomics.wait(new Int32Array(1), 0, 0)"), T("Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 'x')"), T("Atomics.notify(new Uint8Array(1), 0)"),
  T("Intl.getCanonicalLocales('x_y')"), T("new Intl.Locale()"), T("new Intl.Locale('x_y')"), T("new Intl.NumberFormat('x_y')"), T("new Intl.NumberFormat('en', { style: 'currency' })"), T("new Intl.NumberFormat('en', { style: 'currency', currency: 'x' })"), T("new Intl.NumberFormat('en', { minimumFractionDigits: 101 })"), T("new Intl.NumberFormat('en', { minimumFractionDigits: 3, maximumFractionDigits: 1 })"), T("new Intl.NumberFormat('en', { style: 'unit' })"), T("new Intl.NumberFormat('en', { style: 'unit', unit: 'nope' })"), T("new Intl.DateTimeFormat('en', { timeZone: 'Nope/Nope' })"), T("new Intl.DateTimeFormat('en', { dateStyle: 'x' })"), T("new Intl.DateTimeFormat('en').format(NaN)"), T("new Intl.DateTimeFormat('en', { dateStyle: 'short', year: 'numeric' })"), T("new Intl.PluralRules('en', { type: 'x' })"), T("new Intl.RelativeTimeFormat('en').format(1, 'x')"), T("new Intl.ListFormat('en', { type: 'x' })"), T("new Intl.Collator('en', { usage: 'x' })"), T("new Intl.Segmenter('en', { granularity: 'x' })"), T("new Intl.DisplayNames('en')"), T("new Intl.DisplayNames('en', { type: 'x' })"), T("Intl.NumberFormat.prototype.format"),
  T("nope(function () {})"), T("nope('x')"), T("new nope()"),
  T("new FinalizationRegistry()"), T("new WeakRef()"), T("new WeakRef({}).deref.call({})"), T("Symbol.for(Symbol())"), T("Symbol.keyFor('x')"), T("Symbol.prototype.description.x"), T("Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.call({})"),
  T("(function () { 'use strict'; return arguments.callee })()"), T("(function () { 'use strict'; return (function () {}).caller })()"), T("(function () { 'use strict'; return (function () {}).arguments })()"), T("(function () { 'use strict'; var f = function () {}; f.caller = 1 })()"), T("(function () {}).caller; (() => {}).caller"), T("Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.call(function () { 'use strict' })"), T("(() => {}).caller"), T("(async function () {}).caller"), T("(function* () {}).arguments"), T("class A {}; A.caller"), T("Function.prototype.caller"), T("Function.prototype.arguments"),
  T("Function('}')"), T("Function('a', '}')"), T("Function('a b', '')"), T("Function('a,', '')"), T("Function('...a', '')"), T("Function('a', 'b', 'c', 'return')"), T("Function('/*', '*/){')"), T("Function('a = 1', '\"use strict\"')"), T("new Function('return super.x')"), T("new Function('await 1')"), T("new Function('yield 1')"), T("new Function('let a; let a')"), T("new Function('break')"), T("new Function('new.target = 1')"),
  T("new (async function* () {})()"), T("(async () => {}).prototype.x"), T("(() => {}).prototype.x"), T("new (async () => {})"), T("new (() => {})"), T("new ({ m() {} }).m"), T("new ({ get g() {} }).g"), T("new (class { static m() {} }).m"), T("var o = { m() {} }; new o.m()"), T("var f = (() => {}).bind(); new f"), T("var f = function () {}.bind(); R = typeof new f"),
);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "error-golden-"));
const file = path.join(dir, "error_case.js");
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
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (`undefined` vira `void 0`, `'a'` vira `"a"`, `new` some das posições do stack):
  // o programa gravado é o texto canônico e o que o bun executa é `executableSource(original)` (ver golden-prelude.js).
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
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("error", lines));
fs.rmSync(dir, { recursive: true, force: true });
