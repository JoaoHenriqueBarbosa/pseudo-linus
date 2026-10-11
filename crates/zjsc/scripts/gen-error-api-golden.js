// Gera tests/golden/error_api_bun.tsv: API avançada de Error medida no bun 1.4.2 (JavaScriptCore): captureStackTrace
// (objeto qualquer, constructorOpt, objeto congelado), stackTraceLimit (0, 1, não número, Infinity), prepareStackTrace
// com todos os métodos de CallSite, descritor e escrita da propriedade `stack`, formato de frames por contexto, `cause`
// (opções, ordem de leitura, undefined x ausente), AggregateError (errors, ordem, iteráveis), SuppressedError,
// Error.prototype.toString com name/message exóticos, coerção da mensagem, Symbol em message, subclasses e name.
// O programa roda por `vm.runInThisContext(fonte, { filename: "x.js" })` (ProgramExecutable do JSC puro, sem o
// transpilador do bun). Todo texto de stack passa por `N`, que descarta os frames do hospedeiro depois do último frame
// de `x.js` e troca `x.js:linha:coluna` por `x.js:L:C`, então nada depende de caminho ou de posição.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-error-api-golden.js > tests/golden/error_api_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Auxiliares injetados no começo de cada programa: N normaliza stack, F filtra callsites do programa, C descreve um callsite.
const PRE =
  "var N = function (s) { var l = String(s).split('\\n'), last = -1; for (var i = 0; i < l.length; i++) if (l[i].indexOf('x.js') >= 0) last = i; if (last >= 0) l = l.slice(0, last + 1); return l.join('\\n').replace(/x\\.js:\\d+:\\d+/g, 'x.js:L:C') }; " +
  "var C = function (c, m) { try { var v = c[m](); return typeof v === 'string' ? v.replace(/x\\.js:\\d+:\\d+/g, 'x.js:L:C') : String(v) } catch (e) { return 'throws ' + e.name } }; " +
  "var P = function (f) { var o = Error.prepareStackTrace; Error.prepareStackTrace = f; try { return f.out() } finally { Error.prepareStackTrace = o } }; ";

const programs = [];
const add = body => programs.push(PRE + body);
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
const J = "JSON.stringify";

// ---- 1. captureStackTrace.
const targets = {
  plain: "{}",
  array: "[]",
  fn: "function () {}",
  error: "new Error('m')",
  withStack: "{ stack: 'old' }",
  nullProto: "Object.create(null)",
  classInst: "new (class K {})",
  frozen: "Object.freeze({})",
  sealed: "Object.seal({})",
  nonExt: "Object.preventExtensions({})",
  frozenStack: "Object.freeze({ stack: 1 })",
  getterStack: "{ get stack() { return 'g' } }",
  proxy: "new Proxy({}, {})",
};
for (const [name, expr] of Object.entries(targets)) {
  T(`var o = ${expr}; Error.captureStackTrace(o); R = typeof o.stack + ':' + N(o.stack).split('\\n').slice(0, 2).join('|')`);
  T(`var o = ${expr}; Error.captureStackTrace(o); var d = Object.getOwnPropertyDescriptor(o, 'stack'); R = d ? ${J}({ w: d.writable, e: d.enumerable, c: d.configurable, g: typeof d.get, s: typeof d.set, v: typeof d.value }) : 'none'`);
}
const opts = ["undefined", "null", "1", "'f'", "{}", "function () {}", "Math.max", "class {}", "outer"];
for (const opt of opts) {
  T(`function outer() { return inner() } function inner() { var o = {}; Error.captureStackTrace(o, ${opt}); return o.stack } R = N(outer())`);
}
T("function a() { return b() } function b() { return c() } function c() { var o = {}; Error.captureStackTrace(o, b); return o.stack } R = N(a())");
T("function a() { return b() } function b() { return c() } function c() { var o = {}; Error.captureStackTrace(o, a); return o.stack } R = N(a())");
T("function a() { return b() } function b() { return c() } function c() { var o = {}; Error.captureStackTrace(o, c); return o.stack } R = N(a())");
T("function a() { var o = {}; Error.captureStackTrace(o, function () {}); return o.stack } R = N(a())");
T("var o = {}; Error.captureStackTrace(o, Error.captureStackTrace); R = typeof o.stack");
T("class K { constructor() { Error.captureStackTrace(this, K) } } R = N(new K().stack)");
T("class K { constructor() { Error.captureStackTrace(this, this.constructor) } } class L extends K {} R = N(new L().stack)");
T("function E(m) { this.message = m; Error.captureStackTrace(this, E) } E.prototype = Object.create(Error.prototype); E.prototype.name = 'E'; R = N(new E('boom').stack)");
T("function E(m) { this.message = m; Error.captureStackTrace(this) } E.prototype = Object.create(Error.prototype); E.prototype.name = 'E'; R = N(new E('boom').stack)");
T("var o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); R = N(o.stack).split('\\n')[0]");
T("var o = { toString() { return 'custom' } }; Error.captureStackTrace(o); R = N(o.stack).split('\\n')[0]");
T("var o = Object.create(Error.prototype); Error.captureStackTrace(o); R = N(o.stack).split('\\n')[0]");
T("var o = Object.create(Error.prototype, { message: { value: 'mm' } }); Error.captureStackTrace(o); R = N(o.stack).split('\\n')[0]");
T("var o = new TypeError('tt'); Error.captureStackTrace(o); R = N(o.stack).split('\\n')[0]");
T("var o = new TypeError('tt'); var s = o.stack; Error.captureStackTrace(o); R = s === o.stack");
T("var o = {}; Error.captureStackTrace(o); var s1 = o.stack; Error.captureStackTrace(o); R = s1 === o.stack");
T("R = typeof Error.captureStackTrace + Error.captureStackTrace.length + Error.captureStackTrace.name");
T("Error.captureStackTrace()");
T("Error.captureStackTrace(1)");
T("Error.captureStackTrace('s')");
T("Error.captureStackTrace(null)");
T("Error.captureStackTrace(undefined)");
T("Error.captureStackTrace(Symbol())");
T("R = Error.captureStackTrace({}) === undefined");
T("Error.captureStackTrace(Object.freeze({}))");
T("'use strict'; Error.captureStackTrace(Object.freeze({}))");
T("var o = Object.freeze({ stack: 1 }); Error.captureStackTrace(o)");
T("var o = Object.defineProperty({}, 'stack', { value: 1, writable: false, configurable: false }); Error.captureStackTrace(o)");
T("var o = Object.defineProperty({}, 'stack', { value: 1, writable: false, configurable: true }); Error.captureStackTrace(o); R = typeof o.stack");
T("var o = new Proxy({}, { defineProperty() { R = 'trap'; return true } }); Error.captureStackTrace(o); R = R || 'no trap'");
T("var log = []; var o = new Proxy({}, { defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d) }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r) } }); Error.captureStackTrace(o); R = log.join()");
T("var o = {}; Error.captureStackTrace(o); R = Object.getOwnPropertyNames(o).join()");
T("var o = { a: 1 }; Error.captureStackTrace(o); R = Object.keys(o).join() + '|' + Object.getOwnPropertyNames(o).join()");
T("var o = {}; Error.captureStackTrace(o); R = JSON.stringify(o)");
T("var o = {}; Error.captureStackTrace(o); R = delete o.stack");
T("var o = {}; Error.captureStackTrace(o); delete o.stack; R = typeof o.stack");
T("var o = {}; Error.captureStackTrace(o); o.stack = 'w'; R = o.stack");
T("var o = {}; Error.captureStackTrace(o); Object.defineProperty(o, 'stack', { value: 'd' }); R = o.stack");
T("function f() { var o = {}; Error.captureStackTrace(o); return o.stack } R = N(f()).split('\\n').length");
T("Error.stackTraceLimit = 2; function f() { var o = {}; Error.captureStackTrace(o); return o.stack } function g() { return f() } function h() { return g() } R = N(h()); delete Error.stackTraceLimit");
T("Error.stackTraceLimit = 0; var o = {}; Error.captureStackTrace(o); R = N(o.stack); Error.stackTraceLimit = 100");
T("var o = {}; Error.captureStackTrace(o, undefined); R = N(o.stack).split('\\n')[0]");
T("var C2 = function () {}; var o = new C2; Error.captureStackTrace(o, C2); R = typeof o.stack");
T("var o = {}; var f = function f() { Error.captureStackTrace(o, f) }; f(); R = N(o.stack).split('\\n').length");
T("var o = {}; var bound = function g() { Error.captureStackTrace(o, bound) }.bind(null); bound(); R = typeof o.stack");
T("var o = {}; (async () => { Error.captureStackTrace(o) })(); R = N(o.stack).split('\\n')[0]");
T("var o = {}; (function* () { Error.captureStackTrace(o) })().next(); R = N(o.stack)");

// ---- 2. Error.stackTraceLimit.
const limits = ["0", "1", "2", "3", "-1", "-Infinity", "Infinity", "NaN", "1.5", "2.9", "'2'", "'x'", "null", "undefined", "true", "false", "{}", "[]", "[2]", "Symbol()", "2 ** 32", "0.5", "-0", "'0'", "1n"];
const chain = "function a() { return b() } function b() { return c() } function c() { return d() } function d() { return new Error('x').stack }";
for (const lim of limits) {
  T(`${chain} Error.stackTraceLimit = ${lim}; try { R = N(a()) } finally { Error.stackTraceLimit = 100 }`);
  T(`${chain} Error.stackTraceLimit = ${lim}; try { R = String(N(a()).split('\\n').length) + ':' + typeof Error.stackTraceLimit } finally { Error.stackTraceLimit = 100 }`);
}
for (const lim of ["0", "1", "Infinity", "'x'", "undefined"]) {
  T(`Error.stackTraceLimit = ${lim}; try { var o = {}; Error.captureStackTrace(o); R = typeof o.stack + ':' + N(o.stack).split('\\n').length } finally { Error.stackTraceLimit = 100 }`);
  T(`Error.stackTraceLimit = ${lim}; try { R = N(new TypeError('q').stack) } finally { Error.stackTraceLimit = 100 }`);
  T(`Error.stackTraceLimit = ${lim}; try { R = N(new AggregateError([], 'q').stack) } finally { Error.stackTraceLimit = 100 }`);
  T(`Error.stackTraceLimit = ${lim}; try { null.x } catch (e) { R = N(e.stack) } finally { Error.stackTraceLimit = 100 }`);
}
T("var d = Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit'); R = " + J + "({ v: d.value, w: d.writable, e: d.enumerable, c: d.configurable })");
T("delete Error.stackTraceLimit; R = N(new Error('x').stack); Error.stackTraceLimit = 100");
T("delete Error.stackTraceLimit; R = String(Error.stackTraceLimit); Error.stackTraceLimit = 100");
T("Error.stackTraceLimit = 1; var e = new Error('x'); Error.stackTraceLimit = 100; R = N(e.stack)");
T("Error.stackTraceLimit = { valueOf() { R = 'called'; return 1 } }; new Error('x'); Error.stackTraceLimit = 100");
T("Object.defineProperty(Error, 'stackTraceLimit', { get() { R = 'getter'; return 1 }, configurable: true }); new Error('x'); delete Error.stackTraceLimit; Error.stackTraceLimit = 100");
T("Object.freeze(Error); R = N(new Error('x').stack).split('\\n').length");
T("var C2 = class extends Error {}; C2.stackTraceLimit = 1; R = N(new C2('x').stack).split('\\n').length");
T("var C2 = class extends Error {}; R = String(C2.stackTraceLimit)");
T("Error.stackTraceLimit = 1; try { R = N(new RangeError('x').stack) } finally { Error.stackTraceLimit = 100 }");
T("Error.stackTraceLimit = 1; try { function f() { return new Error('x').stack } R = N(f()) } finally { Error.stackTraceLimit = 100 }");
T("R = typeof Error.stackTraceLimit + Error.stackTraceLimit");
T("TypeError.stackTraceLimit = 0; R = N(new TypeError('x').stack).split('\\n').length; delete TypeError.stackTraceLimit");

// ---- 3. Error.prepareStackTrace: métodos de CallSite por contexto.
const methods = ["getFunctionName", "getFileName", "getLineNumber", "getColumnNumber", "isNative", "isEval", "isConstructor", "isToplevel", "getTypeName", "getMethodName", "toString", "getThis", "getFunction", "getEvalOrigin", "isAsync", "isPromiseAll", "getPromiseIndex", "getScriptNameOrSourceURL", "getEnclosingLineNumber", "getEnclosingColumnNumber", "getPosition", "getScriptHash", "isStrict"];
const contexts = {
  plainFn: "function foo() { return new Error('x').stack } R = ",
  method: "var o = { bar() { return new Error('x').stack } }; R = ",
  ctor: "function Ctor() { this.s = new Error('x').stack } R = ",
  classMethod: "class K { m() { return new Error('x').stack } } R = ",
  arrow: "var arrow = () => new Error('x').stack; R = ",
  top: "R = ",
};
const calls = { plainFn: "foo()", method: "o.bar()", ctor: "new Ctor().s", classMethod: "new K().m()", arrow: "arrow()", top: "new Error('x').stack" };
for (const [ctx, setup] of Object.entries(contexts)) {
  for (const m of methods) {
    T(`${setup}P(Object.assign(function (e, cs) { return C(cs[0], '${m}') }, { out() { return ${calls[ctx]} } }))`);
  }
}
const prep = body => `Error.prepareStackTrace = function (e, cs) { ${body} };`;
T(`${prep("return cs.length > 0")} R = new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return 42")} R = typeof new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return undefined")} R = typeof new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return null")} R = String(new Error('x').stack); Error.prepareStackTrace = undefined`);
T(`${prep("return { cs: Array.isArray(cs) }")} R = JSON.stringify(new Error('x').stack); Error.prepareStackTrace = undefined`);
T(`${prep("throw new RangeError('from prepare')")} try { new Error('x').stack } catch (e) { R = e.name + ':' + e.message } Error.prepareStackTrace = undefined`);
T(`${prep("throw new RangeError('from prepare')")} try { R = typeof new Error('x').stack } catch (e) { R = 'thrown' } Error.prepareStackTrace = undefined`);
T(`${prep("return e.message + ':' + e.name")} R = new TypeError('tm').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return this === Error ? 'err' : typeof this")} R = new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return arguments.length")} R = new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return Object.prototype.toString.call(cs)")} R = new Error('x').stack; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(c => c.constructor.name + typeof c).join()")} R = N(new Error('x').stack); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} function f() { return new Error('x').stack } R = N(f()); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} function f() { return new Error('x').stack } function g() { return f() } R = N(g()); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} class K { constructor() { this.s = new Error('x').stack } } R = N(new K().s); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} R = N(eval('new Error(\"x\").stack')); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} R = N(new Function('return new Error(\"x\").stack')()); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} R = N([1].map(function () { return new Error('x').stack })[0]); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(String).join('\\n')")} R = N((() => { try { null.x } catch (e) { return e.stack } })()); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(c => c.getFunctionName()).join()")} function f() { return new Error('x').stack } R = f(); Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(c => c.isNative()).join()")} R = [1].map(function () { return new Error('x').stack })[0]; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(c => c.getTypeName()).join()")} R = [1].map(function () { return new Error('x').stack })[0]; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.map(c => c.getFunctionName()).join()")} R = [1].map(function foo() { return new Error('x').stack })[0]; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.length")} Error.stackTraceLimit = 1; R = new Error('x').stack; Error.stackTraceLimit = 100; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.length")} Error.stackTraceLimit = 0; R = new Error('x').stack; Error.stackTraceLimit = 100; Error.prepareStackTrace = undefined`);
T(`${prep("return cs.length")} var o = {}; Error.captureStackTrace(o); R = o.stack; Error.prepareStackTrace = undefined`);
T(`${prep("return e === o")} var o = {}; Error.captureStackTrace(o); R = o.stack; Error.prepareStackTrace = undefined`);
T(`${prep("return String(e === o)")} var o = new Error('x'); R = o.stack; Error.prepareStackTrace = undefined`);
T(`${prep("return 'once'")} var e = new Error('x'); R = e.stack + e.stack; Error.prepareStackTrace = undefined`);
T(`var n = 0; ${prep("return 'n' + (++n)")} var e = new Error('x'); R = e.stack + e.stack; Error.prepareStackTrace = undefined`);
T(`var n = 0; ${prep("return 'n' + (++n)")} var e = new Error('x'); n = 5; R = e.stack; Error.prepareStackTrace = undefined`);
T(`${prep("return 'late'")} var e = new Error('x'); Error.prepareStackTrace = undefined; R = N(e.stack).split('\\n')[0]`);
T(`var e = new Error('x'); ${prep("return 'late'")} R = N(e.stack).split('\\n')[0]; Error.prepareStackTrace = undefined`);
T(`${prep("return 'x'")} var e = new Error('x'); R = Object.getOwnPropertyDescriptor(e, 'stack') ? 'own' : 'none'; Error.prepareStackTrace = undefined`);
T(`${prep("return 'x'")} R = typeof Error.prepareStackTrace; Error.prepareStackTrace = undefined`);
T("Error.prepareStackTrace = 5; R = N(new Error('x').stack).split('\\n')[0]; Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = {}; R = N(new Error('x').stack).split('\\n')[0]; Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = null; R = N(new Error('x').stack).split('\\n')[0]; Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = class {}; try { R = typeof new Error('x').stack } catch (e) { R = e.name } Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = Math.max; R = String(new Error('x').stack); Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = function () { return new Error('inner').stack }; try { R = typeof new Error('x').stack } catch (e) { R = e.name } Error.prepareStackTrace = undefined");
T("TypeError.prepareStackTrace = function () { return 'sub' }; R = new TypeError('x').stack; delete TypeError.prepareStackTrace");
T("var C2 = class extends Error {}; C2.prepareStackTrace = function () { return 'sub' }; R = N(new C2('x').stack).split('\\n')[0]");
T("Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFileName() === 'x.js') .join() }; function f() { return new Error('x').stack } R = f().split(',').slice(0, 2).join(); Error.prepareStackTrace = undefined");
T("Error.prepareStackTrace = function (e, cs) { return JSON.stringify(cs[0].getThis() === undefined) }; R = new Error('x').stack; Error.prepareStackTrace = undefined");

// ---- 4. Propriedade `stack`.
T("var e = new Error('x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = " + J + "({ own: !!d, w: d && d.writable, e: d && d.enumerable, c: d && d.configurable, v: d && typeof d.value, g: d && typeof d.get, s: d && typeof d.set })");
T("R = Object.getOwnPropertyNames(new Error('x')).join()");
T("R = Object.getOwnPropertyNames(new Error('x', { cause: 1 })).join()");
T("R = Object.getOwnPropertyNames(new Error()).join()");
T("R = Object.getOwnPropertyNames(new AggregateError([], 'm')).join()");
T("R = Reflect.ownKeys(new Error('x')).join()");
T("R = Object.keys(new Error('x')).length");
T("R = JSON.stringify(new Error('x'))");
T("R = 'stack' in Error.prototype");
T("R = Object.getOwnPropertyNames(Error.prototype).sort().join()");
T("R = Object.getOwnPropertyNames(Error).sort().join()");
T("R = Object.getOwnPropertyNames(TypeError.prototype).sort().join()");
T("var e = new Error('x'); e.stack = 'custom'; R = e.stack");
T("var e = new Error('x'); e.stack = 5; R = typeof e.stack");
T("var e = new Error('x'); e.stack = undefined; R = typeof e.stack + 'stack' in e");
T("var e = new Error('x'); delete e.stack; R = typeof e.stack + ('stack' in e)");
T("var e = new Error('x'); Object.freeze(e); R = N(e.stack).split('\\n')[0]");
T("var e = new Error('x'); Object.freeze(e); e.stack = 'no'; R = N(e.stack).split('\\n')[0]");
T("'use strict'; var e = new Error('x'); Object.freeze(e); e.stack = 'no'");
T("var e = Object.freeze(new Error('x')); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = " + J + "({ w: d.writable, c: d.configurable })");
T("var e = Object.seal(new Error('x')); e.stack = 'sealed'; R = e.stack");
T("var e = new Error('x'); Object.defineProperty(e, 'stack', { value: 'dv' }); R = e.stack + ':' + Object.getOwnPropertyDescriptor(e, 'stack').writable");
T("var e = new Error('x'); Object.defineProperty(e, 'stack', { get() { return 'gg' } }); R = e.stack");
T("var e = new Error('x'); var s1 = e.stack; R = s1 === e.stack");
T("var e = new Error('x'); e.message = 'changed'; R = N(e.stack).split('\\n')[0]");
T("var e = new Error('x'); e.name = 'Changed'; R = N(e.stack).split('\\n')[0]");
T("var e = new Error('x'); e.stack; e.message = 'late'; R = N(e.stack).split('\\n')[0]");
T("var e = new Error('x'); var s = e.stack; R = typeof s");
T("var e = new Error('x'); R = Object.getOwnPropertyDescriptor(e, 'stack').value === e.stack");
T("var e = Object.create(new Error('p')); R = typeof e.stack + ':' + Object.hasOwn(e, 'stack')");
T("var e = Object.create(new Error('p')); e.stack = 's'; R = Object.hasOwn(e, 'stack') + e.stack");
T("var p = new Error('p'); Object.freeze(p); var e = Object.create(p); e.stack = 's'; R = Object.hasOwn(e, 'stack')");
T("var e = new Error('x'); R = e.hasOwnProperty('stack') + ':' + e.propertyIsEnumerable('stack')");
T("var e = new Error('x'); var c = structuredCloneLike(e); function structuredCloneLike(x) { return Object.assign(Object.create(Object.getPrototypeOf(x)), x) } R = typeof c.stack");
T("var e = new Error('x'); R = Object.entries(Object.getOwnPropertyDescriptors(e)).map(([k, d]) => k + ':' + d.enumerable).join()");
T("var o = Object.create(Error.prototype); R = typeof o.stack");
T("class K extends Error {} R = Object.getOwnPropertyNames(new K('x')).join()");
T("class K extends Error { constructor() { super('x'); this.extra = 1 } } R = Object.getOwnPropertyNames(new K).join()");
T("class K extends Error { constructor() { super('x'); Object.defineProperty(this, 'stack', { value: 'own' }) } } R = new K().stack");
T("class K extends Error { get stack() { return 'proto-getter' } } R = new K('x').stack");
T("class K extends Error { constructor() { super('x'); R = Object.hasOwn(this, 'stack') } } new K");
T("function E() { return Reflect.construct(Error, ['x'], E) } E.prototype = Object.create(Error.prototype); R = typeof new E().stack");
T("var e = Reflect.construct(Error, ['x'], Object); R = Object.getPrototypeOf(e) === Object.prototype");
T("var e = Reflect.construct(Error, ['x'], function () {}.bind()); R = typeof e.stack");
T("var e = Error('x'); R = N(e.stack).split('\\n')[0]");
T("var e = Error.call({}, 'x'); R = e instanceof Error");

// ---- 5. Formato de frames por contexto (cada um já normalizado por N).
const S = "new Error('x').stack";
const frames = [
  `R = N(eval("${S}"))`,
  `R = N((0, eval)("${S}"))`,
  `R = N(eval("function ev() { return ${S} } ev()"))`,
  `R = N(eval("eval(\\"${S}\\")"))`,
  `R = N(new Function("return ${S}")())`,
  `R = N(new Function("a", "b", "return ${S}")(1, 2))`,
  `R = N(Function("return ${S}")())`,
  `R = N(new (Function.constructor)("return ${S}")())`,
  `var f = new Function("return ${S}"); R = N((function outer() { return f() })())`,
  `R = N(eval("(function () { return ${S} })")())`,
  `R = N(eval("(() => ${S})")())`,
  `R = N(eval("var x = 1; function g() { return ${S} } g()"))`,
  `var AF = (async function () {}).constructor; R = 'ok'`,
  `function C1() { this.s = ${S} } R = N(new C1().s)`,
  `function C1() { this.s = ${S} } R = N(Reflect.construct(C1, []).s)`,
  `class A { constructor() { this.s = ${S} } } R = N(new A().s)`,
  `class A { constructor() { this.s = ${S} } } class B extends A { constructor() { super() } } R = N(new B().s)`,
  `class A { static { R = N(${S}) } }`,
  `class A { x = ${S} } R = N(new A().x)`,
  `class A { static x = ${S} } R = N(A.x)`,
  `class A { #p = ${S}; get() { return this.#p } } R = N(new A().get())`,
  `class A { ['comp' + 1]() { return ${S} } } R = N(new A().comp1())`,
  `class A { get g() { return ${S} } } R = N(new A().g)`,
  `class A { static get g() { return ${S} } } R = N(A.g)`,
  `var o = { get g() { return ${S} } }; R = N(o.g)`,
  `var o = { set g(v) { R = N(${S}) } }; o.g = 1`,
  `var o = { m() { return ${S} } }; R = N(o.m())`,
  `var o = { f: function () { return ${S} } }; R = N(o.f())`,
  `var o = { f: () => ${S} }; R = N(o.f())`,
  `var o = { 'a b'() { return ${S} } }; R = N(o['a b']())`,
  `var o = { [Symbol('s')]() { return ${S} } }; R = N(o[Object.getOwnPropertySymbols(o)[0]]())`,
  `var o = { 1() { return ${S} } }; R = N(o[1]())`,
  `var f = function () { return ${S} }; R = N(f())`,
  `var f = function named() { return ${S} }; R = N(f())`,
  `var f = () => ${S}; R = N(f())`,
  `R = N((function () { return ${S} })())`,
  `R = N((() => ${S})())`,
  `R = N((function* () { yield ${S} })().next().value)`,
  `R = N((async function () { return ${S} })() && 'p')`,
  `var s; (async function af() { s = ${S} })(); R = N(s)`,
  `var s; (async function af() { await 0; s = ${S} })(); R = 'ok'`,
  `var s; (async () => { s = ${S} })(); R = N(s)`,
  `async function inner() { return ${S} } async function outer() { return await inner() } var s; outer().then(v => s = v); R = 'sync'`,
  `function a() { return b() } function b() { return ${S} } R = N(a())`,
  `function a() { return [1].map(function cb() { return ${S} })[0] } R = N(a())`,
  `function a() { return [1].forEach(function cb() { R = N(${S}) }) } a()`,
  `R = N([3, 1, 2].sort(function cmp(x, y) { R = N(${S}); return x - y }) && R)`,
  `R = N(JSON.parse('[1]', function rev() { R = N(${S}); return 1 }) && R)`,
  `R = N('a'.replace(/a/, function rep() { return ${S} }))`,
  `R = N(Reflect.apply(function ap() { return ${S} }, null, []))`,
  `R = N(Function.prototype.call.call(function cl() { return ${S} }))`,
  `R = N(new Proxy(function pf() { return ${S} }, {})())`,
  `R = N(new Proxy(function pf() { return ${S} }, { apply(t, th, args) { return t() } })())`,
  `var o = { toString() { return ${S} } }; R = N(String(o))`,
  `var o = { valueOf() { R = N(${S}); return 1 } }; +o`,
  `var o = { get [Symbol.toPrimitive]() { R = N(${S}); return undefined } }; try { +o } catch (e) {}`,
  `R = N(function () { try { throw new Error('x') } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { null.x } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { undefinedVar } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { f.call.call(1) } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { new (function () {}.bind()).x } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { [].reduce(function () {}) } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { 'a'.repeat(-1) } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { new Array(-1) } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { JSON.parse('{') } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { Symbol() + '' } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { BigInt(1.5) } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { (function r() { r() })() } catch (e) { return e.name + e.stack.length > 0 } }())`,
  `R = N(function f() { try { eval('1 +') } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { new Function('1 +') } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { class A extends null { constructor() { super() } } new A } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { x = y; let y } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { const c = 1; c = 2 } catch (e) { return e.stack } }())`,
  `R = N(function f() { try { Object.defineProperty(Object.freeze({}), 'a', { value: 1 }) } catch (e) { return e.stack } }())`,
  `R = N(function f() { 'use strict'; try { undefined.x = 1 } catch (e) { return e.stack } }())`,
  `R = N(new Error('x', { cause: new Error('c') }).stack)`,
  `R = N(new AggregateError([new Error('i')], 'agg').stack)`,
  `R = N(new TypeError('t').stack)`,
  `R = N(new RangeError().stack)`,
  `R = N(new Error().stack)`,
  `R = N(new Error('').stack)`,
  `R = N(new Error('multi\\nline').stack)`,
  `R = N(new Error('a', { cause: 1 }).stack)`,
  `var e = new Error('x'); e.name = 'Custom'; R = N(e.stack)`,
  `class MyErr extends Error {} R = N(new MyErr('x').stack)`,
  `class MyErr extends Error { constructor(m) { super(m); this.name = 'MyErr' } } R = N(new MyErr('x').stack)`,
  `class MyErr extends Error { get name() { return 'Getter' } } R = N(new MyErr('x').stack)`,
  `class MyErr extends Error {} MyErr.prototype.name = 'Proto'; R = N(new MyErr('x').stack)`,
  `function Old(m) { var e = Error.call(this, m); this.stack = e.stack } Old.prototype = Object.create(Error.prototype); R = N(new Old('x').stack)`,
  `function f() { return ${S} } R = N(f.call(null))`,
  `function f() { return ${S} } R = N(f.bind(null)())`,
  `function f() { return ${S} } R = N(f.apply(null, []))`,
  `function f() { return ${S} } var b = f.bind(null); R = N(b())`,
  `function f() { return ${S} } R = N(Reflect.construct(f, []) && 'obj')`,
  `function f() { 'use strict'; return ${S} } R = N(f())`,
  `var o = { f() { return (() => ${S})() } }; R = N(o.f())`,
  `var o = { f() { return [1].map(() => ${S})[0] } }; R = N(o.f())`,
  `with ({ w: 1 }) { R = N(${S}) }`,
  `with ({ wf() { return ${S} } }) { R = N(wf()) }`,
  `label: { R = N(${S}) }`,
  `switch (1) { case 1: R = N(${S}) }`,
  `for (var i = 0; i < 1; i++) { R = N(${S}) }`,
  `try { R = N(${S}) } finally {}`,
  `var f = function () { return ${S} }; Object.defineProperty(f, 'name', { value: 'renamed' }); R = N(f())`,
  `var f = function () { return ${S} }; Object.defineProperty(f, 'name', { value: 5 }); R = N(f())`,
  `var f = function () { return ${S} }; Object.defineProperty(f, 'name', { get() { return 'got' } }); R = N(f())`,
  `var f = function orig() { return ${S} }; Object.defineProperty(f, 'name', { value: '' }); R = N(f())`,
  `var f = function orig() { return ${S} }; delete f.name; R = N(f())`,
  `class A { static m() { return ${S} } } Object.defineProperty(A, 'name', { value: 'Z' }); R = N(A.m())`,
  `class A { constructor() { this.s = ${S} } } Object.defineProperty(A, 'name', { value: 'Z' }); R = N(new A().s)`,
  `var o = { __proto__: { m() { return ${S} } } }; R = N(o.m())`,
  `var Cls = class { m() { return ${S} } }; R = N(new Cls().m())`,
  `R = N((class { static m() { return ${S} } }).m())`,
  `R = N(new (class { constructor() { this.s = ${S} } })().s)`,
];
for (const f of frames) T(f);

// ---- 6. cause.
T("var e = new Error('m', { cause: 'c' }); R = e.cause");
T("var e = new Error('m', { cause: undefined }); R = Object.hasOwn(e, 'cause') + ':' + String(e.cause)");
T("var e = new Error('m', {}); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m'); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', { cause: null }); R = Object.hasOwn(e, 'cause') + ':' + e.cause");
T("var e = new Error('m', { cause: 0 }); R = Object.hasOwn(e, 'cause') + ':' + e.cause");
T("var e = new Error('m', { cause: undefined }); var d = Object.getOwnPropertyDescriptor(e, 'cause'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable })");
T("var e = new Error('m', { cause: 1 }); var d = Object.getOwnPropertyDescriptor(e, 'cause'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable, v: d.value })");
T("var e = new Error('m', 5); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', 'cause'); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', null); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', undefined); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', []); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', function () {}); R = Object.hasOwn(e, 'cause')");
T("var f = function () {}; f.cause = 7; var e = new Error('m', f); R = e.cause");
T("var e = new Error('m', Object.create({ cause: 'inherited' })); R = Object.hasOwn(e, 'cause') + ':' + e.cause");
T("var e = new Error('m', Object.create({ cause: undefined })); R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', new Proxy({}, { has(t, k) { R = 'has:' + String(k); return false } })); R = R + ':' + Object.hasOwn(e, 'cause')");
T("var log = []; var o = new Proxy({ cause: 1 }, { has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k) }, get(t, k) { log.push('get:' + String(k)); return Reflect.get(t, k) } }); new Error('m', o); R = log.join()");
T("var log = []; var o = { get cause() { log.push('cause'); return 1 } }; var m = { toString() { log.push('msg'); return 'x' } }; new Error(m, o); R = log.join()");
T("var log = []; var o = { get cause() { log.push('cause'); return 1 } }; var m = { toString() { log.push('msg'); return 'x' } }; new AggregateError([], m, o); R = log.join()");
T("var log = []; var errs = { [Symbol.iterator]() { log.push('iter'); return [][Symbol.iterator]() } }; var o = { get cause() { log.push('cause'); return 1 } }; var m = { toString() { log.push('msg'); return 'x' } }; new AggregateError(errs, m, o); R = log.join()");
T("var log = []; var o = { get cause() { log.push('cause'); return 1 } }; var m = { toString() { log.push('msg'); return 'x' } }; var nt = new Proxy(Error, { get(t, k, r) { log.push('proto:' + String(k)); return Reflect.get(t, k, r) } }); Reflect.construct(Error, [m, o], nt); R = log.join()");
T("var e = new Error('m', { cause: new Error('inner') }); R = e.cause instanceof Error && e.cause.message");
T("var e = new Error('m', { cause: e2 }); var e2 = 1");
T("var o = {}; o.cause = o; var e = new Error('m', o); R = e.cause === o");
T("var e = new Error('m', { cause: 'a' }); e.cause = 'b'; R = e.cause");
T("var e = new Error('m', { cause: 'a' }); delete e.cause; R = Object.hasOwn(e, 'cause')");
T("var e = new Error('m', { cause: 'a' }); R = Object.keys(e).length + ':' + JSON.stringify(e)");
T("R = Object.hasOwn(Error.prototype, 'cause')");
T("R = Error.length + ':' + TypeError.length + ':' + AggregateError.length");
T("var e = new TypeError('m', { cause: 1 }); R = e.cause");
T("var e = new RangeError('m', { cause: 1 }); R = e.cause");
T("var e = new EvalError('m', { cause: 1 }); R = e.cause");
T("var e = new URIError('m', { cause: 1 }); R = e.cause");
T("var e = new ReferenceError('m', { cause: 1 }); R = e.cause");
T("var e = new SyntaxError('m', { cause: 1 }); R = e.cause");
T("var e = new AggregateError([], 'm', { cause: 1 }); R = e.cause");
T("var e = AggregateError([], 'm', { cause: 1 }); R = e.cause");
T("var e = Error('m', { cause: 1 }); R = e.cause");
T("class K extends Error {} R = new K('m', { cause: 1 }).cause");
T("class K extends Error { constructor(m) { super(m, { cause: 'fixed' }) } } R = new K('m', { cause: 1 }).cause");
T("class K extends Error { constructor(m, o) { super(m); this.cause = o } } R = new K('m', 'later').cause");
T("class K extends Error { constructor(m, o) { super(m, o) } } R = Object.hasOwn(new K('m'), 'cause') + ':' + Object.hasOwn(new K('m', { cause: undefined }), 'cause')");
T("var e = new Error('m', { cause: 1, extra: 2 }); R = Object.getOwnPropertyNames(e).join()");
T("var e = new Error(undefined, { cause: 1 }); R = Object.getOwnPropertyNames(e).join()");
T("var e = new Error('x', { cause: 1 }); R = Object.getOwnPropertyNames(e).join()");

// ---- 7. AggregateError.
T("var e = new AggregateError([1, 2], 'm'); R = " + J + "(e.errors) + e.message + e.name");
T("var e = new AggregateError([]); R = e.errors.length + ':' + String(e.message) + ':' + Object.hasOwn(e, 'message')");
T("var e = new AggregateError([], undefined); R = Object.hasOwn(e, 'message')");
T("var e = new AggregateError([], ''); R = Object.hasOwn(e, 'message')");
T("var e = new AggregateError(new Set([1, 2, 2]), 'm'); R = " + J + "(e.errors)");
T("var e = new AggregateError(new Map([[1, 2]]), 'm'); R = " + J + "(e.errors)");
T("var e = new AggregateError('abc', 'm'); R = " + J + "(e.errors)");
T("var e = new AggregateError((function* () { yield 1; yield 2 })(), 'm'); R = " + J + "(e.errors)");
T("var e = new AggregateError({ length: 2, 0: 'a', 1: 'b' }, 'm'); R = " + J + "(e.errors)");
T("new AggregateError({ length: 2 }, 'm')");
T("new AggregateError(1, 'm')");
T("new AggregateError(null, 'm')");
T("new AggregateError(undefined, 'm')");
T("new AggregateError()");
T("new AggregateError(Symbol(), 'm')");
T("new AggregateError(true)");
T("new AggregateError({})");
T("new AggregateError({ [Symbol.iterator]: 1 })");
T("new AggregateError({ [Symbol.iterator]() { return 1 } })");
T("new AggregateError({ [Symbol.iterator]() { return {} } })");
T("new AggregateError({ [Symbol.iterator]() { return { next() { return 1 } } } })");
T("new AggregateError({ [Symbol.iterator]() { return { next() { throw new RangeError('n') } } } })");
T("var closed = false; try { new AggregateError({ [Symbol.iterator]() { return { next() { throw new RangeError('n') }, return() { closed = true; return {} } } } }) } catch (e) {} R = closed");
T("var log = []; var it = { [Symbol.iterator]() { log.push('iter'); var i = 0; return { next() { log.push('next'); return i++ < 2 ? { value: i, done: false } : { done: true } } } } }; var e = new AggregateError(it, { toString() { log.push('msg'); return 'm' } }); R = log.join()");
T("var e = new AggregateError([1], 'm'); var d = Object.getOwnPropertyDescriptor(e, 'errors'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable, a: Array.isArray(d.value) })");
T("var e = new AggregateError([1], 'm'); R = Object.getOwnPropertyNames(e).join()");
T("var a = [1]; var e = new AggregateError(a, 'm'); R = e.errors === a");
T("var a = [1]; var e = new AggregateError(a, 'm'); a.push(2); R = e.errors.length");
T("var e = new AggregateError([1], 'm'); e.errors.push(2); R = e.errors.length");
T("var e = new AggregateError([1], 'm'); e.errors = 'x'; R = e.errors");
T("var e = new AggregateError([new Error('a'), new TypeError('b')], 'm'); R = e.errors.map(x => x.name + x.message).join()");
T("var e = new AggregateError([1], 'm'); R = e instanceof Error && Object.getPrototypeOf(AggregateError) === Error");
T("R = AggregateError.prototype.name + ':' + Object.hasOwn(AggregateError.prototype, 'message') + ':' + AggregateError.prototype.message");
T("R = Object.getOwnPropertyNames(AggregateError.prototype).sort().join()");
T("R = Object.getOwnPropertyNames(AggregateError).sort().join()");
T("R = AggregateError.name + AggregateError.length");
T("R = String(new AggregateError([], 'm'))");
T("R = N(new AggregateError([], 'm').stack).split('\\n')[0]");
T("R = Object.prototype.toString.call(new AggregateError([]))");
T("class K extends AggregateError {} var e = new K([1], 'm'); R = e.name + e.errors.length + (e instanceof K)");
T("class K extends AggregateError { constructor() { super([1, 2], 'sub') } } R = new K().errors.length");
T("R = new AggregateError([], 'm', { cause: 'c' }).cause");
T("var e = new AggregateError([], 'm', { cause: 'c' }); R = Object.getOwnPropertyNames(e).join()");
T("var p = Promise.any([Promise.reject(1), Promise.reject(2)]); var out; p.catch(e => { out = e }); R = 'queued'");
T("var out; Promise.any([]).catch(e => { out = e.name + e.errors.length + e.message }); R = 'queued'");
T("var e = new AggregateError([1, 2, 3], 'm'); R = Array.isArray(e.errors) + ':' + e.errors.length");
T("var e = new AggregateError(new Array(3), 'm'); R = e.errors.length + ':' + (0 in e.errors)");
T("var e = new AggregateError([, 1], 'm'); R = e.errors.length + ':' + (0 in e.errors)");
T("var e = new AggregateError([1], 'm', null); R = Object.hasOwn(e, 'cause')");
T("var e = new AggregateError([1], { toString() { return 'ts' } }); R = e.message");
T("var e = new AggregateError([1], 1); R = typeof e.message + e.message");
T("var e = new AggregateError([1], Symbol()); R = 'ok'");

// ---- 8. SuppressedError.
T("R = typeof SuppressedError");
T("var e = new SuppressedError('err', 'sup', 'msg'); R = e.name + ':' + e.message + ':' + e.error + ':' + e.suppressed");
T("var e = new SuppressedError(); R = Object.getOwnPropertyNames(e).join()");
T("var e = new SuppressedError(1, 2); R = Object.getOwnPropertyNames(e).join() + ':' + Object.hasOwn(e, 'message')");
T("var e = new SuppressedError(1, 2, 3); R = Object.getOwnPropertyNames(e).join()");
T("var e = new SuppressedError(undefined, undefined, undefined); R = Object.getOwnPropertyNames(e).join()");
T("var e = new SuppressedError(1, 2, 'm', { cause: 'c' }); R = Object.getOwnPropertyNames(e).join()");
T("var e = SuppressedError(1, 2, 'm'); R = e instanceof SuppressedError");
T("var e = new SuppressedError(1, 2, 'm'); var d = Object.getOwnPropertyDescriptor(e, 'error'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable })");
T("var e = new SuppressedError(1, 2, 'm'); var d = Object.getOwnPropertyDescriptor(e, 'suppressed'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable })");
T("R = Object.getPrototypeOf(SuppressedError) === Error");
T("R = Object.getPrototypeOf(SuppressedError.prototype) === Error.prototype");
T("R = SuppressedError.length + SuppressedError.name");
T("R = SuppressedError.prototype.name + ':' + Object.getOwnPropertyNames(SuppressedError.prototype).sort().join()");
T("R = String(new SuppressedError(1, 2, 'boom'))");
T("R = N(new SuppressedError(1, 2, 'boom').stack).split('\\n')[0]");
T("var log = []; new SuppressedError({ get x() { log.push('e') } }, 2, { toString() { log.push('msg'); return 'm' } }, { get cause() { log.push('cause'); return 1 } }); R = log.join()");
T("class K extends SuppressedError {} var e = new K(1, 2, 'm'); R = e.name + (e instanceof K) + e.error");
T("R = Object.prototype.toString.call(new SuppressedError(1, 2))");
T("var a = {}; var e = new SuppressedError(a, a, 'm'); R = e.error === a && e.suppressed === a");
T("R = typeof DisposableStack + typeof AsyncDisposableStack + typeof Symbol.dispose + typeof Symbol.asyncDispose");
T("var e = new SuppressedError(new Error('a'), new Error('b'), 'm'); R = e.error.message + e.suppressed.message");

// ---- 9. Error.prototype.toString exótico.
const ts = Error.prototype.toString;
const exotic = [
  "{}", "{ name: 'N' }", "{ message: 'M' }", "{ name: 'N', message: 'M' }", "{ name: '', message: 'M' }", "{ name: 'N', message: '' }",
  "{ name: '', message: '' }", "{ name: undefined, message: 'M' }", "{ name: 'N', message: undefined }", "{ name: null, message: null }",
  "{ name: 1, message: 2 }", "{ name: true, message: false }", "{ name: 1n, message: 2n }", "{ name: {}, message: [] }", "{ name: [1, 2], message: [3] }",
  "{ name: { toString() { return 'ts' } }, message: 'M' }", "{ name: 'N', message: { toString() { return 'ms' } } }",
  "{ name: { valueOf() { return 'vo' }, toString: null }, message: 'M' }", "{ name: { toString() { throw new RangeError('boom') } } }",
  "{ message: { toString() { throw new RangeError('boom') } } }", "{ name: Symbol('s') }", "{ message: Symbol('s') }",
  "{ name: 'a\\nb', message: 'c\\nd' }", "{ name: ' ', message: ' ' }", "{ name: '\\u0000', message: '\\u0000' }", "{ name: 'é', message: 'ü' }",
  "{ get name() { return 'G' }, get message() { return 'H' } }", "{ get name() { throw new TypeError('gn') } }", "{ get message() { throw new TypeError('gm') } }",
  "Object.create({ name: 'P', message: 'Q' })", "Object.create(null, { name: { value: 'X' } })", "new Proxy({}, { get(t, k) { return String(k) } })",
  "function () {}", "[]", "[1]", "new Error('real')", "new TypeError('real')", "Object.assign(new Error('real'), { name: 'Over' })",
  "Object.assign(new Error('real'), { message: 'Over' })", "Object.assign(new Error('real'), { name: '' })", "Object.assign(new Error('real'), { name: undefined })",
  "new Proxy(new Error('p'), {})", "Object.create(Error.prototype)", "Object.create(TypeError.prototype)", "new (class Q extends Error {})('z')",
];
for (const ex of exotic) {
  T(`R = Error.prototype.toString.call(${ex})`);
}
for (const bad of ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n"]) T(`R = Error.prototype.toString.call(${bad})`);
T("R = Error.prototype.toString.length + Error.prototype.toString.name");
T("R = String(new Error('m'))");
T("R = '' + new Error('m')");
T("R = `${new TypeError('m')}`");
T("R = String(new Error())");
T("R = String(new Error(''))");
T("var e = new Error('m'); e.name = undefined; R = String(e)");
T("var e = new Error('m'); e.name = null; R = String(e)");
T("var e = new Error('m'); e.message = undefined; R = String(e)");
T("var e = new Error('m'); e.message = null; R = String(e)");
T("var e = new Error('m'); delete e.message; R = String(e)");
T("var e = new Error('m'); Object.setPrototypeOf(e, null); R = typeof Error.prototype.toString.call(e)");
T("R = Object.prototype.toString.call(new Error('m'))");
T("var e = new Error('m'); e[Symbol.toStringTag] = 'Tag'; R = Object.prototype.toString.call(e)");
T("R = Object.prototype.toString.call(Error.prototype)");
T("R = Object.prototype.toString.call(Object.create(Error.prototype))");
T("R = Error.prototype.name + ':' + Error.prototype.message + ':' + Error.prototype.toString()");
T("R = TypeError.prototype.toString === Error.prototype.toString");
T("R = RangeError.prototype.name + EvalError.prototype.name + URIError.prototype.name + ReferenceError.prototype.name + SyntaxError.prototype.name");
T("R = [TypeError, RangeError, EvalError, URIError, ReferenceError, SyntaxError].map(c => Object.hasOwn(c.prototype, 'message') + ':' + c.prototype.message.length).join()");
T("Error.prototype.name = 'Changed'; try { R = String(new Error('m')) } finally { Error.prototype.name = 'Error' }");
T("Error.prototype.message = 'dm'; try { R = String(new Error()) + '|' + String(new Error('own')) + '|' + Object.hasOwn(new Error(), 'message') } finally { Error.prototype.message = '' }");
T("TypeError.prototype.name = 'TT'; try { R = String(new TypeError('m')) } finally { TypeError.prototype.name = 'TypeError' }");
T("R = Object.getOwnPropertyDescriptor(Error.prototype, 'name').writable + ':' + Object.getOwnPropertyDescriptor(Error.prototype, 'name').enumerable");

// ---- 10. Coerção da mensagem.
const messages = [
  "undefined", "null", "1", "-0", "1.5", "NaN", "Infinity", "true", "false", "''", "' '", "'a'", "'a\\nb'", "1n", "[]", "[1, 2]", "[[]]", "[null]", "[undefined]", "{}",
  "{ toString() { return 'ts' } }", "{ valueOf() { return 'vo' } }", "{ toString: null, valueOf() { return 'vo2' } }", "{ toString() { return {} }, valueOf() { return 7 } }",
  "{ toString() { return {} }, valueOf() { return {} } }", "{ toString() { throw new RangeError('ts') } }", "{ [Symbol.toPrimitive](h) { return 'hint:' + h } }",
  "{ [Symbol.toPrimitive]() { return {} } }", "{ [Symbol.toPrimitive]: 1 }", "function f() {}", "class K {}", "new Date(0)", "/re/g", "new Error('inner')", "Symbol('s')", "Symbol.iterator",
  "Object(Symbol('w'))", "new String('boxed')", "new Number(3)", "new Boolean(false)", "Object(1n)", "new Proxy({}, {})", "new Proxy([], {})", "Object.create(null)", "Math", "JSON", "globalThis === 1",
];
for (const m of messages) {
  T(`var e = new Error(${m}); R = Object.hasOwn(e, 'message') + ':' + typeof e.message + ':' + e.message`);
  T(`var e = new TypeError(${m}); R = String(e)`);
}
for (const m of ["undefined", "'x'", "Symbol()", "1"]) {
  T(`var e = new AggregateError([], ${m}); R = Object.hasOwn(e, 'message') + ':' + String(e.message)`);
}
T("var e = new Error('m'); var d = Object.getOwnPropertyDescriptor(e, 'message'); R = " + J + "({ w: d.writable, e: d.enumerable, c: d.configurable, v: d.value })");
T("var e = new Error(); R = Object.hasOwn(e, 'message') + String(e.message === '')");
T("var e = new Error(undefined); R = Object.hasOwn(e, 'message')");
T("var e = new Error(null); R = Object.hasOwn(e, 'message') + e.message");
T("var e = new Error('a', 'b', 'c'); R = e.message");
T("var e = new Error('x'.repeat(100000)); R = e.message.length");
T("var e = new Error('x'.repeat(100)); R = N(e.stack).split('\\n')[0].length");
T("var n = 0; var m = { toString() { n++; return 'm' } }; var e = new Error(m); e.message; e.message; R = n");
T("var n = 0; var m = { toString() { n++; return 'm' } }; var e = new Error(m); e.stack; R = n");
T("var m = { toString() { return 'later' } }; var e = new Error(m); m.toString = () => 'x'; R = e.message");
T("var e = Error(Symbol('s'))");
T("var e = new Error(Symbol.iterator)");
T("var e = new Error(`${1}`); R = e.message");
T("var e = new Error('a' + 'b'); R = e.message");
T("var e = new Error(String.raw`\\n`); R = e.message");

// ---- 11. Subclasses e name.
T("class K extends Error {} var e = new K('m'); R = e.name + ':' + e.constructor.name + ':' + String(e) + ':' + (e instanceof K) + (e instanceof Error)");
T("class K extends Error { constructor(m) { super(m); this.name = 'K' } } R = String(new K('m'))");
T("class K extends Error { constructor(m) { super(m); this.name = this.constructor.name } } class L extends K {} R = String(new L('m'))");
T("class K extends Error { get name() { return 'GK' } } R = String(new K('m'))");
T("class K extends Error { static get [Symbol.species]() { return 1 } } R = String(new K('m'))");
T("class K extends Error { toString() { return 'custom toString' } } R = String(new K('m')) + ':' + N(new K('m').stack).split('\\n')[0]");
T("class K extends Error { get message() { return 'gm' } } R = String(new K('m')) + ':' + Object.hasOwn(new K('m'), 'message')");
T("class K extends Error { constructor() { super() } } R = Object.hasOwn(new K, 'message') + String(new K)");
T("class K extends Error { constructor() { super(); this.message = 'set' } } R = String(new K)");
T("class K extends Error { constructor() { super('a'); Object.setPrototypeOf(this, Error.prototype) } } R = (new K instanceof K) + ':' + new K().constructor.name");
T("class K extends Error {} R = Object.getPrototypeOf(K) === Error && Object.getPrototypeOf(K.prototype) === Error.prototype");
T("class K extends TypeError {} R = new K('m').name + ':' + String(new K('m')) + (new K instanceof TypeError)");
T("class K extends RangeError {} class L extends K {} R = String(new L('m')) + (new L instanceof RangeError)");
T("class K extends Error { constructor(m, o) { super(m, o); this.code = 1 } } R = " + J + "(Object.getOwnPropertyNames(new K('m')))");
T("class K extends Error { static create(m) { return new this(m) } } class L extends K {} R = L.create('x').constructor.name");
T("class K extends Error { constructor(...a) { super(...a) } } R = new K('m', { cause: 1 }).cause");
T("class K extends Error { constructor() { return Object.create(K.prototype) } } R = Object.hasOwn(new K, 'stack') + ':' + (new K instanceof Error)");
T("class K extends Error { constructor() { return {} } } R = new K() instanceof Error");
T("class K extends Error { constructor() { } } new K");
T("class K extends Error { constructor() { super(); super() } } new K");
T("function F() { Error.call(this, 'm') } F.prototype = Object.create(Error.prototype); R = Object.hasOwn(new F, 'message') + ':' + (new F instanceof Error) + ':' + String(new F)");
T("function F(m) { this.message = m } F.prototype = Object.create(Error.prototype); F.prototype.name = 'F'; R = String(new F('x'))");
T("function F(m) { this.message = m } F.prototype = new Error; R = String(new F('x')) + ':' + typeof new F('x').stack");
T("var o = Object.create(Error.prototype); o.name = 'O'; o.message = 'm'; R = String(o) + ':' + (o instanceof Error) + typeof o.stack");
T("R = Error.prototype.constructor === Error && TypeError.prototype.constructor === TypeError");
T("R = Object.getPrototypeOf(TypeError) === Error && Object.getPrototypeOf(RangeError) === Error && Object.getPrototypeOf(AggregateError) === Error");
T("R = [Error, TypeError, RangeError, EvalError, URIError, ReferenceError, SyntaxError, AggregateError].map(c => c.name + c.length).join()");
T("R = [Error, TypeError, RangeError, EvalError, URIError, ReferenceError, SyntaxError, AggregateError].map(c => typeof c.captureStackTrace + typeof c.stackTraceLimit).join()");
T("R = Object.getOwnPropertyDescriptor(Error, 'prototype').writable + ':' + Object.getOwnPropertyDescriptor(Error, 'prototype').configurable");
T("var e = new Error('m'); e.name = 'Set'; R = e.name + ':' + Object.hasOwn(e, 'name') + ':' + Object.keys(e).join()");
T("var e = new Error('m'); Object.defineProperty(e, 'name', { value: 'Def' }); R = String(e) + ':' + Object.keys(e).join()");
T("var e = new Error('m'); e.name = 5; R = String(e)");
T("var e = new Error('m'); e.name = { toString() { return 'obj' } }; R = String(e)");
T("var e = new Error('m'); e.name = ''; e.message = ''; R = '[' + String(e) + ']'");
T("var e = new Error('m'); e.name = 'Err'; R = N(e.stack).split('\\n')[0]");
T("var e = new TypeError('m'); e.name = 'Err'; R = String(e) + ':' + N(e.stack).split('\\n')[0]");
T("var e = new Error('m'); Error.prototype.name = 'Late'; var s = N(e.stack).split('\\n')[0]; Error.prototype.name = 'Error'; R = s");
T("var e = new Error('m'); e.name = 'a'; R = e instanceof Error && Object.prototype.toString.call(e)");
T("R = Error.prototype.hasOwnProperty('toString') + ':' + Object.getOwnPropertyDescriptor(Error.prototype, 'toString').enumerable");
T("var e = new Error('m'); R = typeof Error.prototype.toString.call(e) + ':' + (e.toString === Error.prototype.toString)");
T("R = Object.getOwnPropertyDescriptor(Error.prototype, 'message').writable + ':' + JSON.stringify(Error.prototype.message)");
T("R = Error.prototype.isPrototypeOf(new TypeError) + ':' + (Error.prototype instanceof Error)");
T("R = Object.prototype.toString.call(new class extends Error {})");
T("var e = new (class extends Error {})('m'); R = Object.getPrototypeOf(e).constructor.name === ''");
T("Error.prototype.toString = function () { return 'patched' }; try { R = String(new TypeError('m')) } finally { delete Error.prototype.toString; }");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "error-api-golden-"));
const source_file = path.join(dir, "case_source.js");
const file = path.join(dir, "case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8"), { filename: "x.js" }) } catch (e) {}\n`,
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
for (const body of programs) {
  const test = body.slice(PRE.length);
  if (HOST.test(test)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(test.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(test.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
