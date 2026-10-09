// Gera tests/golden/error_stack_bun.tsv: formato das linhas de Error.stack, erro dentro de callback nativo,
// captureStackTrace, stackTraceLimit, prepareStackTrace com CallSite, cause, AggregateError, eval e geradores,
// medidos no bun 1.4.2. Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// O arquivo se chama `error_stack_case.js` dos dois lados; o diretório temporário sai do texto da pilha e
// qualquer resultado que ainda contenha caminho da máquina é descartado.
// Uso: bun scripts/gen-error-stack-golden.js > tests/golden/error_stack_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];

// ---- 1. Forma da linha de frame: cada definição lança ou captura dentro de uma construção diferente.
const shapes = {
  anonymous_fn: "R = (function () { return new Error('x').stack })()",
  named_fn: "function named() { return new Error('x').stack }; R = named()",
  arrow: "var f = () => new Error('x').stack; R = f()",
  assigned_arrow: "var o = {}; o.a = () => new Error('x').stack; R = o.a()",
  method: "var o = { m() { return new Error('x').stack } }; R = o.m()",
  class_method: "class K { m() { return new Error('x').stack } }; R = new K().m()",
  static_method: "class K { static m() { return new Error('x').stack } }; R = K.m()",
  constructor: "class K { constructor() { this.s = new Error('x').stack } }; R = new K().s",
  derived_constructor: "class A {}; class K extends A { constructor() { super(); this.s = new Error('x').stack } }; R = new K().s",
  function_constructor: "function K() { this.s = new Error('x').stack }; R = new K().s",
  getter: "var o = { get g() { return new Error('x').stack } }; R = o.g",
  setter: "var o = { set s(v) { R = new Error('x').stack } }; o.s = 1",
  static_getter: "class K { static get g() { return new Error('x').stack } }; R = K.g",
  private_method: "class K { #p() { return new Error('x').stack } m() { return this.#p() } }; R = new K().m()",
  static_block: "class K { static { R = new Error('x').stack } }",
  field_init: "class K { f = new Error('x').stack }; R = new K().f",
  static_field: "class K { static f = new Error('x').stack }; R = K.f",
  computed_method: "var k = 'dyn'; var o = { [k]() { return new Error('x').stack } }; R = o.dyn()",
  symbol_method: "var s = Symbol('s'); var o = { [s]() { return new Error('x').stack } }; R = o[s]()",
  async_before_await: "async function a() { return new Error('x').stack }; a().then(v => { R = v })",
  async_after_await: "async function a() { await 1; return new Error('x').stack }; a().then(v => { R = v })",
  async_nested: "async function b() { await 1; return new Error('x').stack }; async function a() { return await b() }; a().then(v => { R = v })",
  async_method: "var o = { async m() { await 0; return new Error('x').stack } }; o.m().then(v => { R = v })",
  async_arrow: "var f = async () => { await 0; return new Error('x').stack }; f().then(v => { R = v })",
  generator: "function* g() { yield new Error('x').stack }; R = g().next().value",
  generator_resumed: "function* g() { yield 1; yield new Error('x').stack }; var i = g(); i.next(); R = i.next().value",
  async_generator: "async function* g() { await 0; yield new Error('x').stack }; g().next().then(v => { R = v.value })",
  eval_direct: "R = eval('new Error(\"x\").stack')",
  eval_in_fn: "function f() { return eval('new Error(\"x\").stack') }; R = f()",
  eval_indirect: "R = (0, eval)('new Error(\"x\").stack')",
  eval_fn_in_eval: "R = eval('(function inner() { return new Error(\"x\").stack })()')",
  new_function: "R = new Function('return new Error(\"x\").stack')()",
  new_function_args: "R = new Function('a', 'b', 'return new Error(\"x\").stack')(1, 2)",
  bound: "function f() { return new Error('x').stack }; R = f.bind(null)()",
  call: "function f() { return new Error('x').stack }; R = f.call({})",
  apply: "function f() { return new Error('x').stack }; R = f.apply(null, [])",
  reflect_apply: "function f() { return new Error('x').stack }; R = Reflect.apply(f, null, [])",
  reflect_construct: "function F() { this.s = new Error('x').stack }; R = Reflect.construct(F, []).s",
  iife_named: "R = (function iife() { return new Error('x').stack })()",
  new_iife: "R = new (function Foo() { this.s = new Error('x').stack })().s",
  nested_depth: "function a() { return b() }; function b() { return c() }; function c() { return new Error('x').stack }; R = a()",
  proto_method: "function F() {}; F.prototype.m = function () { return new Error('x').stack }; R = new F().m()",
  property_name_fn: "var o = { f: function () { return new Error('x').stack } }; R = o.f()",
  nested_object: "var o = { a: { b: { c() { return new Error('x').stack } } } }; R = o.a.b.c()",
  call_on_number: "Number.prototype.nm = function () { return new Error('x').stack }; R = (1).nm()",
  call_on_string: "String.prototype.sm = function () { return new Error('x').stack }; R = 'a'.sm()",
  call_on_array: "Array.prototype.am = function () { return new Error('x').stack }; R = [].am()",
  call_on_map: "Map.prototype.mm = function () { return new Error('x').stack }; R = new Map().mm()",
  call_on_function: "Function.prototype.fm = function () { return new Error('x').stack }; R = (function () {}).fm()",
  top_level: "R = new Error('x').stack",
  top_level_multiline: "\n\nR =\n  new Error(\n    'x'\n  ).stack",
  thrown_caught: "function f() { throw new Error('x') }; try { f() } catch (e) { R = e.stack }",
  thrown_finally: "function f() { try { throw new Error('x') } finally { } }; try { f() } catch (e) { R = e.stack }",
  rethrown: "function f() { throw new Error('x') }; try { try { f() } catch (e) { throw e } } catch (e2) { R = e2.stack }",
  engine_type_error: "function f() { null.p }; try { f() } catch (e) { R = e.stack }",
  engine_reference_error: "function f() { undefinedVariable }; try { f() } catch (e) { R = e.stack }",
  engine_range_error: "function f() { new Array(-1) }; try { f() } catch (e) { R = e.stack }",
  engine_not_function: "function f() { var x = 1; x() }; try { f() } catch (e) { R = e.stack }",
  engine_const_assign: "function f() { const c = 1; c = 2 }; try { f() } catch (e) { R = e.stack }",
  engine_in_arrow: "var f = () => null.p; try { f() } catch (e) { R = e.stack }",
  engine_in_method: "var o = { m() { undefined.p } }; try { o.m() } catch (e) { R = e.stack }",
  engine_in_class: "class K { constructor() { null.p } }; try { new K } catch (e) { R = e.stack }",
  engine_in_getter: "var o = { get g() { null.p } }; try { o.g } catch (e) { R = e.stack }",
  engine_in_static_block: "try { class K { static { null.p } } } catch (e) { R = e.stack }",
  engine_super_before: "class A {}; class K extends A { constructor() { this.x = 1 } }; try { new K } catch (e) { R = e.stack }",
  engine_class_call: "class K {}; try { K() } catch (e) { R = e.stack }",
  engine_new_arrow: "var f = () => {}; try { new f } catch (e) { R = e.stack }",
  engine_stack_overflow: "function r() { r() }; try { r() } catch (e) { R = e.constructor.name + ':' + e.message }",
  engine_json_parse: "function f() { JSON.parse('{') }; try { f() } catch (e) { R = e.stack }",
  engine_bigint: "function f() { return 1n + 1 }; try { f() } catch (e) { R = e.stack }",
  engine_symbol_string: "function f() { return '' + Symbol() }; try { f() } catch (e) { R = e.stack }",
  engine_spread: "function f() { return [...1] }; try { f() } catch (e) { R = e.stack }",
  engine_destructure: "function f() { var { a } = null }; try { f() } catch (e) { R = e.stack }",
  engine_instanceof: "function f() { return 1 instanceof 1 }; try { f() } catch (e) { R = e.stack }",
  engine_in_op: "function f() { return 'a' in 1 }; try { f() } catch (e) { R = e.stack }",
  engine_delete_strict: "function f() { delete Object.prototype }; try { f() } catch (e) { R = e.stack }",
  engine_frozen_write: "function f() { Object.freeze({ a: 1 }).a = 2 }; try { f() } catch (e) { R = e.stack }",
};
for (const body of Object.values(shapes)) programs.push(body);

// ---- 2. Erro dentro de callback nativo: o frame nativo aparece como `at <nome> (native)` ou some.
const nativeCallbacks = {
  array_map: "[1].map(cb)",
  array_for_each: "[1].forEach(cb)",
  array_filter: "[1].filter(cb)",
  array_reduce: "[1].reduce(cb, 0)",
  array_reduce_no_init: "[1, 2].reduce(cb)",
  array_find: "[1].find(cb)",
  array_some: "[1].some(cb)",
  array_every: "[1].every(cb)",
  array_sort: "[2, 1].sort(cb)",
  array_flat_map: "[1].flatMap(cb)",
  array_from: "Array.from([1], cb)",
  typed_map: "new Uint8Array(1).map(cb)",
  typed_for_each: "new Uint8Array(1).forEach(cb)",
  string_replace: "'a'.replace(/a/, cb)",
  string_replace_str: "'a'.replace('a', cb)",
  string_replace_all: "'a'.replaceAll('a', cb)",
  map_for_each: "new Map([[1, 1]]).forEach(cb)",
  set_for_each: "new Set([1]).forEach(cb)",
  json_parse_reviver: "JSON.parse('{\"a\":1}', cb)",
  json_stringify_replacer: "JSON.stringify({ a: 1 }, cb)",
  json_to_json: "JSON.stringify({ toJSON: cb })",
  function_call: "cb.call(null)",
  function_apply: "cb.apply(null, [])",
  function_bind: "cb.bind(null)()",
  reflect_apply: "Reflect.apply(cb, null, [])",
  new_promise_executor: "new Promise(cb)",
  to_primitive: "+{ valueOf: cb }",
  to_string: "'' + { toString: cb }",
  symbol_to_primitive: "+{ [Symbol.toPrimitive]: cb }",
  getter_define: "Object.defineProperty({}, 'p', { get: cb }).p",
  setter_define: "Object.defineProperty({}, 'p', { set: cb }).p = 1",
  array_join_to_string: "[{ toString: cb }].join()",
  object_from_entries: "Object.fromEntries({ [Symbol.iterator]: cb })",
  iterator_next: "for (var x of { [Symbol.iterator]() { return { next: cb } } }) ;",
  spread_iterator: "[...{ [Symbol.iterator]: cb }]",
  tagged_template: "((s) => cb())`x`",
  proxy_get: "new Proxy({}, { get: cb }).p",
  proxy_set: "new Proxy({}, { set: cb }).p = 1",
  proxy_has: "'p' in new Proxy({}, { has: cb })",
  proxy_delete: "delete new Proxy({}, { deleteProperty: cb }).p",
  proxy_own_keys: "Object.keys(new Proxy({}, { ownKeys: cb }))",
  proxy_get_own_prop: "Object.getOwnPropertyDescriptor(new Proxy({}, { getOwnPropertyDescriptor: cb }), 'p')",
  proxy_define: "Object.defineProperty(new Proxy({}, { defineProperty: cb }), 'p', {})",
  proxy_get_proto: "Object.getPrototypeOf(new Proxy({}, { getPrototypeOf: cb }))",
  proxy_apply: "new Proxy(function () {}, { apply: cb })()",
  proxy_construct: "new (new Proxy(function () {}, { construct: cb }))",
  reflect_get_getter: "Reflect.get({ get p() { return cb() } }, 'p')",
  promise_then:"Promise.resolve(1).then(cb)",
  promise_catch: "Promise.reject(1).catch(cb)",
  promise_finally: "Promise.resolve(1).finally(cb)",
  promise_all: "Promise.all([1]).then(cb)",
  async_iterate: "(async () => { for await (var x of [1]) cb() })()",
};
const callbackKinds = {
  throwing: "function cb() { throw new Error('x') }",
  capture: "function cb() { R = new Error('x').stack }",
  arrow_capture: "var cb = () => { R = new Error('x').stack }",
  engine_error: "function cb() { null.p }",
};
for (const [kind, definition] of Object.entries(callbackKinds)) {
  for (const call of Object.values(nativeCallbacks)) {
    if (kind === "throwing" || kind === "engine_error") {
      programs.push(`${definition}; try { ${call} } catch (e) { R = e.stack }`);
    } else {
      programs.push(`${definition}; ${call}`);
    }
  }
}

// ---- 3. Error.captureStackTrace(obj, fn).
const captureCases = [
  "var o = {}; Error.captureStackTrace(o); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o) }; f(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; function g() { f() }; g(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, g) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, h) }; function g() { f() }; function h() { g() }; h(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, function () {}) }; f(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, 1) }; f(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, null) }; f(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, {}) }; f(); R = o.stack",
  "var o = {}; function f() { Error.captureStackTrace(o, undefined) }; f(); R = o.stack",
  "var o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "var o = { toString() { return 'custom' } }; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "var o = Object.create(null); Error.captureStackTrace(o); R = typeof o.stack",
  "var o = function () {}; Error.captureStackTrace(o); R = typeof o.stack",
  "var o = []; Error.captureStackTrace(o); R = typeof o.stack",
  "var o = new Error('m'); Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "var o = new Error('m'); o.message = 'changed'; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]",
  "try { Error.captureStackTrace(1) } catch (e) { R = e.constructor.name + ':' + e.message }",
  "try { Error.captureStackTrace() } catch (e) { R = e.constructor.name + ':' + e.message }",
  "try { Error.captureStackTrace(null) } catch (e) { R = e.constructor.name + ':' + e.message }",
  "try { Error.captureStackTrace('s') } catch (e) { R = e.constructor.name + ':' + e.message }",
  "var o = Object.freeze({}); try { Error.captureStackTrace(o); R = 'ok' } catch (e) { R = e.constructor.name + ':' + e.message }",
  "var o = Object.preventExtensions({}); try { Error.captureStackTrace(o); R = 'ok' } catch (e) { R = e.constructor.name + ':' + e.message }",
  "var o = {}; Object.defineProperty(o, 'stack', { value: 1, configurable: false }); try { Error.captureStackTrace(o); R = String(o.stack) } catch (e) { R = e.constructor.name + ':' + e.message }",
  "var o = {}; Error.captureStackTrace(o); var d = Object.getOwnPropertyDescriptor(o, 'stack'); R = typeof d.value + ',' + d.writable + ',' + d.enumerable + ',' + d.configurable",
  "var o = {}; Error.captureStackTrace(o); R = Object.getOwnPropertyNames(o).join()",
  "var o = {}; R = Error.captureStackTrace(o) === undefined",
  "R = Error.captureStackTrace.length + ',' + Error.captureStackTrace.name",
  "function F() { Error.captureStackTrace(this, F) }; R = new F().stack.split('\\n')[0]",
  "function F() { Error.captureStackTrace(this, F) }; function mk() { return new F() }; R = mk().stack",
  "class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, E) } }; function mk() { return new E('m') }; R = mk().stack",
  "class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, new.target) } }; class F extends E {}; function mk() { return new F('m') }; R = mk().stack",
  "class E extends Error {}; function mk() { return new E('m') }; R = mk().stack",
  "class E extends Error { constructor(m) { super(m) } }; function mk() { return new E('m') }; R = mk().stack",
  "var o = {}; Error.captureStackTrace(o); Error.captureStackTrace(o); R = o.stack.split('\\n').length",
  "var o = {}; var s = Symbol(); o[s] = 1; Error.captureStackTrace(o); R = o.stack.split('\\n')[0] + '|' + o[s]",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; f(); R = JSON.stringify(o.stack)",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; function g() { return f() }; g(); R = o.stack.split('\\n').length",
  "var o = {}; var f = () => Error.captureStackTrace(o, f); f(); R = JSON.stringify(o.stack)",
  "var o = {}; async function f() { await 0; Error.captureStackTrace(o, f) }; f().then(() => { R = JSON.stringify(o.stack) })",
  "var o = {}; function* f() { Error.captureStackTrace(o, f); yield 1 }; f().next(); R = JSON.stringify(o.stack)",
  "var o = {}; var b = function f() { Error.captureStackTrace(o, f) }.bind(null); b(); R = JSON.stringify(o.stack)",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; [1].forEach(f); R = JSON.stringify(o.stack)",
  "var o = {}; function f() { Error.captureStackTrace(o, f) }; [1].forEach(function g() { f() }); R = JSON.stringify(o.stack)",
];
for (const body of captureCases) programs.push(body);

// ---- 4. stackTraceLimit.
const limits = ["0", "1", "2", "3", "5", "10", "11", "1.5", "0.5", "-1", "Infinity", "-Infinity", "NaN", "'3'", "'abc'", "null", "undefined", "true", "{}", "[]", "[2]", "2n === 2n ? 2 : 0", "1e3", "4294967296", "-0"];
const probes = {
  new_error: "function rec(n) { if (n === 0) return new Error('x'); var r = rec(n - 1); return r }; Error.stackTraceLimit = LIMIT; var e = rec(14); R = 'stack' in e ? String(e.stack).split('\\n').length : 'nostack'",
  capture: "function rec(n) { var o = {}; if (n === 0) { Error.captureStackTrace(o); return o }; var r = rec(n - 1); return r }; Error.stackTraceLimit = LIMIT; R = String(rec(14).stack).split('\\n').length",
  thrown: "function rec(n) { if (n === 0) null.p; var r = rec(n - 1); return r }; Error.stackTraceLimit = LIMIT; try { rec(14) } catch (e) { R = 'stack' in e ? String(e.stack).split('\\n').length : 'nostack' }",
  header: "Error.stackTraceLimit = LIMIT; var e = new Error('m'); R = JSON.stringify(e.stack)",
  readback: "Error.stackTraceLimit = LIMIT; R = typeof Error.stackTraceLimit + ':' + String(Error.stackTraceLimit)",
};
for (const limit of limits) {
  for (const probe of Object.values(probes)) programs.push(probe.replace(/LIMIT/g, limit));
}
programs.push(
  "R = Error.stackTraceLimit",
  "R = JSON.stringify(Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit'))",
  "delete Error.stackTraceLimit; R = 'stackTraceLimit' in Error",
  "delete Error.stackTraceLimit; R = 'stack' in new Error('x')",
  "Error.stackTraceLimit = 1; R = new TypeError('x').stack.split('\\n').length",
  "Error.stackTraceLimit = 1; R = new RangeError('x').stack.split('\\n').length",
  "Error.stackTraceLimit = 1; R = new AggregateError([], 'x').stack.split('\\n').length",
  "Error.stackTraceLimit = 0; R = 'stack' in new AggregateError([], 'x')",
  "TypeError.stackTraceLimit = 0; R = 'stack' in new TypeError('x')",
  "TypeError.stackTraceLimit = 1; R = typeof Error.stackTraceLimit",
  "var d = Object.defineProperty(Error, 'stackTraceLimit', { get() { return 1 }, configurable: true }); R = new Error('x').stack.split('\\n').length",
  "Object.defineProperty(Error, 'stackTraceLimit', { get() { throw new Error('boom') }, configurable: true }); try { R = new Error('x').stack } catch (e) { R = 'threw:' + e.message }",
  "Error.stackTraceLimit = { valueOf() { return 1 } }; R = new Error('x').stack.split('\\n').length",
  "Object.freeze(Error); try { Error.stackTraceLimit = 1 } catch (e) { R = 'threw' }; R = R || String(Error.stackTraceLimit)",
);

// ---- 5. Error.prepareStackTrace e CallSite.
const callSiteMethods = [
  "getThis", "getTypeName", "getFunction", "getFunctionName", "getMethodName", "getFileName", "getLineNumber",
  "getColumnNumber", "getEvalOrigin", "getScriptNameOrSourceURL", "isToplevel", "isEval", "isNative", "isConstructor",
  "isAsync", "isPromiseAll", "getPromiseIndex", "getEnclosingLineNumber", "getEnclosingColumnNumber", "toString",
  "getScriptHash", "getPosition",
];
const callSiteContexts = {
  anonymous_fn: "(function () { return CAPTURE })()",
  named_fn: "(function named() { return CAPTURE })()",
  arrow: "(() => CAPTURE)()",
  method: "({ m() { return CAPTURE } }).m()",
  class_method: "new (class K { m() { return CAPTURE } })().m()",
  static_method: "(class K { static m() { return CAPTURE } }).m()",
  constructor: "new (class K { constructor() { this.s = CAPTURE } })().s",
  function_constructor: "(function K() { this.s = CAPTURE; return this }).call({}).s",
  eval: "eval('CAPTURE')",
  new_function: "new Function('return CAPTURE')()",
  getter: "({ get g() { return CAPTURE } }).g",
  native_callback: "[1].map(() => CAPTURE)[0]",
  bound: "(function b() { return CAPTURE }).bind({ q: 1 })()",
  top_level: "CAPTURE",
};
const capture = "(function () { var h = Error.prepareStackTrace; var out; Error.prepareStackTrace = function (e, cs) { out = cs; return 'p' }; var e = new Error('x'); e.stack; Error.prepareStackTrace = h; var c = out[0]; return String(c.METHOD()) })()";
for (const [name, context] of Object.entries(callSiteContexts)) {
  for (const method of callSiteMethods) {
    const expr = context.replace("CAPTURE", capture.replace("METHOD", method));
    programs.push(`try { R = ${expr} } catch (e) { R = 'threw:' + e.constructor.name + ':' + e.message }`);
  }
}
programs.push(
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(Reflect.ownKeys(Object.getPrototypeOf(cs[0])).map(String)) }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { var p = Object.getPrototypeOf(cs[0]); return Reflect.ownKeys(p).map(k => String(k) + ':' + Object.getOwnPropertyDescriptor(p, k).enumerable).join() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { var p = Object.getPrototypeOf(cs[0]); return [p.getScriptId.length, p.getScriptId.name, p.toJSON.length, p.toJSON.name].join() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return typeof cs[0].getScriptId() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return cs[0].getScriptId() > 0 }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(Object.keys(cs[0].toJSON())) }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return typeof cs[0].toJSON() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { var j = cs[0].toJSON(); return j.lineNumber + ':' + j.columnNumber + ':' + j.functionName + ':' + /error_stack_case\\.js$/.test(j.sourceURL) }; function foo() { return new Error('x').stack }; R = foo()",
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(cs.map(c => c.toJSON())).replace(/\"sourceURL\":\"[^\"]*error_stack_case\\.js\"/g, 'U') }; function foo() { return new Error('x').stack }; R = foo()",
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(cs.map(c => c.toJSON())).replace(/\"sourceURL\":\"[^\"]*error_stack_case\\.js\"/g, 'U') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(cs.map(c => c.toJSON())).replace(/\"sourceURL\":\"[^\"]*error_stack_case\\.js\"/g, 'U') }; R = [1].map(function () { return new Error('x').stack })[0]",
  "Error.prepareStackTrace = function (e, cs) { return JSON.stringify(cs) === JSON.stringify(cs.map(c => c.toJSON())) }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getScriptId()).join() }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { try { cs[0].getScriptId.call({}) } catch (x) { return x.constructor.name + ':' + x.message } }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { try { cs[0].toJSON.call({}) } catch (x) { return x.constructor.name + ':' + x.message } }; R = new Error('x').stack",
);
// Frame a frame: os 11 métodos que o relatório de CallSite lista, medidos em cada tipo de frame (índice da
// frame de interesse em `idx`); getTypeName/getThis/getFunction com typeof porque `String()` esconde o tipo.
const frameProbe = "Error.prepareStackTrace = function (e, cs) { var c = cs[IDX]; if (!c) return 'sem frame'; var o = []; "
  + "['getFileName', 'getLineNumber', 'getColumnNumber', 'getScriptNameOrSourceURL', 'getFunctionName', 'getMethodName', 'isNative', 'isEval', 'isToplevel', 'toString'].forEach(function (m) { o.push(m + '=' + typeof c[m]() + ':' + c[m]()) }); "
  + "o.push('getTypeName=' + typeof c.getTypeName() + ':' + c.getTypeName()); o.push('getThis=' + typeof c.getThis()); o.push('getFunction=' + typeof c.getFunction()); return o.join(' ') }; ";
const frameKinds = [
  ["function JS", 1, "function mk() { return new Error('x').stack } function foo() { return mk() } R = foo()"],
  ["método", 1, "function mk() { return new Error('x').stack } var o = { m() { return mk() } }; R = o.m()"],
  ["construtor", 1, "function mk() { return new Error('x').stack } class K { constructor() { this.s = mk() } }; R = new K().s"],
  ["callback de Array.map", 1, "function mk() { return new Error('x').stack } R = [1].map(function cb() { return mk() })[0]"],
  ["native Array.map (frame do chamador)", 2, "function mk() { return new Error('x').stack } R = [1].map(function cb() { return mk() })[0]"],
  ["native Array.reduce no topo", 0, "try { [].reduce((a, b) => a) } catch (e) { R = e.stack }"],
  ["builtin finally", 1, "Promise.resolve(1).finally(function () { R = new Error('x').stack })"],
  ["eval (frame do código)", 1, "function mk() { return new Error('x').stack } R = eval('mk()')"],
  ["eval (frame nativa eval)", 2, "function mk() { return new Error('x').stack } R = eval('mk()')"],
  ["new Function", 1, "function mk() { return new Error('x').stack } R = new Function('mk', 'return mk()')(mk)"],
  ["async", 1, "function mk() { return new Error('x').stack } async function f() { await 1; R = mk() } f()"],
  ["gerador", 1, "function mk() { return new Error('x').stack } function* g() { yield mk() } R = g().next().value"],
  ["bound", 1, "function mk() { return new Error('x').stack } R = (function b() { return mk() }).bind({})()"],
  ["top-level", 1, "function mk() { return new Error('x').stack } R = mk()"],
  ["captureStackTrace", 0, "var o = {}; Error.captureStackTrace(o); R = o.stack"],
];
for (const [, idx, body] of frameKinds) {
  programs.push(frameProbe.replace("IDX", String(idx)) + body);
}
programs.push(
  "Error.prepareStackTrace = function (e, cs) { return cs[0].getLineNumber() + ':' + cs[0].getColumnNumber() + ':' + cs[0].getFileName() + ':' + cs[0].isNative() }; try { [].reduce((a, b) => a) } catch (e) { R = e.stack }",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber() + ':' + c.isNative()).join('|') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = eval('new Error(\"x\").stack')",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; R = [1].map(function () { return new Error('x').stack })[0]",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFileName()).join('|') }; R = [1].map(function () { return new Error('x').stack })[0]",
);
programs.push(
  "Error.prepareStackTrace = function (e, cs) { return cs.length }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return typeof cs + ':' + Array.isArray(cs) }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return this === Error }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return arguments.length }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return e.message }; R = new Error('msg').stack",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.toString()).join('|') }; function f() { return new Error('x').stack }; R = f()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; function f() { return new Error('x').stack }; function g() { return f() }; R = g()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getLineNumber() + ':' + c.getColumnNumber()).join('|') }; function f() { return new Error('x').stack }; R = f()",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFileName()).join('|') }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return { n: cs.length } }; R = JSON.stringify(new Error('x').stack)",
  "Error.prepareStackTrace = function (e, cs) { return 42 }; R = typeof new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return undefined }; R = typeof new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return null }; R = String(new Error('x').stack)",
  "Error.prepareStackTrace = function (e, cs) { throw new Error('prep') }; try { R = new Error('x').stack } catch (e) { R = 'threw:' + e.message }",
  "Error.prepareStackTrace = 1; R = new Error('x').stack.split('\\n')[0]",
  "Error.prepareStackTrace = null; R = new Error('x').stack.split('\\n')[0]",
  "Error.prepareStackTrace = function (e, cs) { return 'once' }; var e = new Error('x'); Error.prepareStackTrace = undefined; R = e.stack",
  "Error.prepareStackTrace = function (e, cs) { return 'lazy' }; var e = new Error('x'); R = e.stack + e.stack",
  "var n = 0; Error.prepareStackTrace = function (e, cs) { n++; return 'c' + n }; var e = new Error('x'); e.stack; e.stack; R = n",
  "Error.prepareStackTrace = function (e, cs) { return 'cap' }; var o = {}; Error.captureStackTrace(o); R = o.stack",
  "Error.prepareStackTrace = function (e, cs) { return e === o }; var o = {}; Error.captureStackTrace(o); R = String(o.stack)",
  "Error.prepareStackTrace = function (e, cs) { return Object.prototype.toString.call(cs[0]) }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return typeof cs[0].getThis() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return String(cs[0].getFunction()) }; function f() { return new Error('x').stack }; R = f()",
  "'use strict'; Error.prepareStackTrace = function (e, cs) { return String(cs[0].getThis()) }; function f() { return new Error('x').stack }; R = f()",
  "Error.prepareStackTrace = function (e, cs) { return cs[0].getTypeName() }; var o = { m() { return new Error('x').stack } }; R = o.m()",
  "Error.prepareStackTrace = function (e, cs) { return cs[0].getMethodName() }; var o = { m() { return new Error('x').stack } }; R = o.m()",
  "Error.prepareStackTrace = function (e, cs) { return [cs[0].isConstructor(), cs[0].isToplevel(), cs[0].isNative(), cs[0].isEval()].join() }; function F() { this.s = new Error('x').stack }; R = new F().s",
  "Error.prepareStackTrace = function (e, cs) { return Object.getOwnPropertyNames(Object.getPrototypeOf(cs[0])).sort().join() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return Object.keys(cs[0]).join() }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return cs[0].constructor.name }; R = new Error('x').stack",
  "Error.prepareStackTrace = function (e, cs) { return cs.length }; Error.stackTraceLimit = 2; function f() { return new Error('x').stack }; function g() { return f() }; function h() { return g() }; R = h()",
  "Error.prepareStackTrace = function (e, cs) { return cs.length }; Error.stackTraceLimit = 0; R = String(new Error('x').stack)",
  "Error.prepareStackTrace = function (e, cs) { return cs.length }; var e = new TypeError('x'); R = e.stack",
  "Error.prepareStackTrace = function (e, cs) { return e.constructor.name }; try { null.p } catch (e) { R = e.stack }",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.getFunctionName()).join('|') }; try { [1].map(function cb() { null.p }) } catch (e) { R = e.stack }",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => c.isNative()).join('|') }; try { [1].map(function cb() { null.p }) } catch (e) { R = e.stack }",
  "Error.prepareStackTrace = function (e, cs) { return cs.map(c => String(c)).join('|') }; try { [1].map(function cb() { null.p }) } catch (e) { R = e.stack }",
);

// ---- 6. cause, AggregateError, erro de sintaxe em eval, geradores.
programs.push(
  "var e = new Error('outer', { cause: new Error('inner') }); R = e.stack.split('\\n')[0] + '|' + e.cause.message",
  "var e = new Error('outer', { cause: 1 }); R = e.cause + ':' + Object.getOwnPropertyNames(e).join()",
  "var e = new Error('outer', { cause: undefined }); R = 'cause' in e",
  "var e = new Error('outer', {}); R = 'cause' in e",
  "var e = new Error('outer', { get cause() { return 'g' } }); R = e.cause",
  "var e = new Error('outer', { cause: new Error('inner') }); R = typeof e.cause.stack",
  "function f() { return new Error('i') }; function g() { return new Error('o', { cause: f() }) }; var e = g(); R = e.stack + '||' + e.cause.stack",
  "var d = Object.getOwnPropertyDescriptor(new Error('o', { cause: 1 }), 'cause'); R = [d.writable, d.enumerable, d.configurable].join()",
  "var e = new Error('a', { cause: new Error('b', { cause: new Error('c') }) }); R = e.cause.cause.message",
  "var a = new Error('a'); a.cause = a; R = a.stack.split('\\n')[0]",
  "var e = new AggregateError([new Error('a'), new TypeError('b')], 'agg'); R = e.stack.split('\\n')[0] + '|' + e.errors.length + '|' + e.errors[1].name",
  "var e = new AggregateError([], 'agg'); R = e.stack.split('\\n').length",
  "function f() { return new AggregateError([new Error('a')], 'agg') }; R = f().stack",
  "var e = new AggregateError(new Set([1, 2]), 'agg', { cause: 'c' }); R = e.errors.join() + e.cause",
  "R = Object.getOwnPropertyNames(new AggregateError([], 'm')).join()",
  "try { new AggregateError() } catch (e) { R = e.constructor.name + ':' + e.message }",
  "try { new AggregateError(1) } catch (e) { R = e.constructor.name + ':' + e.message }",
  "Promise.any([Promise.reject(new Error('a'))]).catch(e => { R = e.constructor.name + ':' + e.message + ':' + e.errors.length + ':' + e.stack.split('\\n')[0] })",
  "Promise.any([]).catch(e => { R = e.constructor.name + ':' + e.message + ':' + e.errors.length })",
  "try { eval('var') } catch (e) { R = e.constructor.name + ':' + e.message + '|' + e.stack }",
  "try { eval('1 +') } catch (e) { R = e.stack }",
  "function f() { eval('}') }; try { f() } catch (e) { R = e.stack }",
  "try { eval('let a; let a') } catch (e) { R = e.stack }",
  "try { new Function('a b', '') } catch (e) { R = e.stack }",
  "try { new Function('}') } catch (e) { R = e.stack }",
  "try { (0, eval)('@') } catch (e) { R = e.stack }",
  "try { eval('\\n\\n  var 1') } catch (e) { R = e.stack }",
  "try { eval('throw new Error(\"in eval\")') } catch (e) { R = e.stack }",
  "try { eval('null.p') } catch (e) { R = e.stack }",
  "function f() { eval('throw new Error(\"in eval\")') }; try { f() } catch (e) { R = e.stack }",
  "try { eval('eval(\"throw new Error(1)\")') } catch (e) { R = e.stack }",
  "try { new Function('throw new Error(1)')() } catch (e) { R = e.stack }",
  "try { new Function('a', 'b', 'null.p')() } catch (e) { R = e.stack }",
  "try { eval('(function () { throw new Error(1) })()') } catch (e) { R = e.stack }",
  "try { eval('//# sourceURL=virtual.js\\nthrow new Error(1)') } catch (e) { R = e.stack }",
  "try { eval('throw new Error(1)\\n//# sourceURL=named_eval.js') } catch (e) { R = e.stack }",
  "try { new Function('//# sourceURL=fn.js\\nthrow new Error(1)')() } catch (e) { R = e.stack }",
  "function* g() { throw new Error('x') }; try { g().next() } catch (e) { R = e.stack }",
  "function* g() { yield 1; throw new Error('x') }; var i = g(); i.next(); try { i.next() } catch (e) { R = e.stack }",
  "function* g() { try { yield 1 } finally { throw new Error('x') } }; var i = g(); i.next(); try { i.return() } catch (e) { R = e.stack }",
  "function* g() { yield 1 }; var i = g(); i.next(); i.next(); try { i.throw(new Error('x')) } catch (e) { R = e.stack }",
  "function* g() { var x = yield; null.p }; var i = g(); i.next(); try { i.next(1) } catch (e) { R = e.stack }",
  "function* inner() { throw new Error('x') }; function* outer() { yield* inner() }; try { outer().next() } catch (e) { R = e.stack }",
  "function* g() { yield 1 }; var i = g(); try { i.next.call({}) } catch (e) { R = e.stack }",
  "function* g() { i.next() }; var i = g(); try { i.next() } catch (e) { R = e.constructor.name + ':' + e.message }",
  "async function* g() { throw new Error('x') }; g().next().catch(e => { R = e.stack })",
  "async function* g() { await 0; throw new Error('x') }; g().next().catch(e => { R = e.stack })",
  "async function f() { throw new Error('x') }; f().catch(e => { R = e.stack })",
  "async function f() { await 0; throw new Error('x') }; f().catch(e => { R = e.stack })",
  "async function g() { await 0; throw new Error('x') }; async function f() { await g() }; f().catch(e => { R = e.stack })",
  "async function g() { await 0; throw new Error('x') }; async function f() { try { await g() } catch (e) { throw e } }; f().catch(e => { R = e.stack })",
  "async function g() { await 0; return new Error('x').stack }; async function f() { return await g() }; async function h() { return await f() }; h().then(v => { R = v })",
  "async function g() { await 0; return new Error('x').stack }; Promise.all([g()]).then(v => { R = v[0] })",
  "async function g() { await 0; return new Error('x').stack }; Promise.all([1, g()]).then(v => { R = v[1] })",
  "async function g() { await 0; return new Error('x').stack }; Promise.any([g()]).then(v => { R = v })",
  "async function g() { await 0; return new Error('x').stack }; Promise.allSettled([g()]).then(v => { R = v[0].value })",
  "async function g() { await 0; return new Error('x').stack }; Promise.race([g()]).then(v => { R = v })",
  "Promise.resolve().then(function t() { R = new Error('x').stack })",
  "Promise.resolve().then(() => Promise.resolve()).then(function t() { R = new Error('x').stack })",
  "new Promise(function ex() { R = new Error('x').stack })",
  "new Promise((_, rej) => rej(new Error('x'))).catch(e => { R = e.stack })",
  "Promise.reject(new Error('x')).catch(e => { R = e.stack })",
  "Promise.resolve().then(function timer() { R = new Error('x').stack })",
  "Promise.resolve().then(function micro() { R = new Error('x').stack })",
  "var e = new Error('x'); R = e.stack === e.stack",
  "var e = new Error('x'); e.stack = 'custom'; R = e.stack",
  "var e = new Error('x'); delete e.stack; R = 'stack' in e",
  "var e = new Error('x'); R = JSON.stringify(Object.getOwnPropertyDescriptor(e, 'stack') && Object.keys(Object.getOwnPropertyDescriptor(e, 'stack')))",
  "var e = new Error('x'); var d = Object.getOwnPropertyDescriptor(e, 'stack'); R = [typeof d.value, d.writable, d.enumerable, d.configurable].join()",
  "R = 'stack' in Error.prototype",
  "R = Object.getOwnPropertyNames(Error.prototype).sort().join()",
  "R = String(Error.prototype.stack)",
  "var o = Object.create(Error.prototype); R = typeof o.stack",
  "var e = new Error('x'); var c = Object.create(e); R = c.stack === e.stack",
  "class E extends Error { constructor() { super('x'); this.name = 'E' } }; R = new E().stack.split('\\n')[0]",
  "class E extends Error { get name() { return 'G' } }; R = new E('m').stack.split('\\n')[0]",
  "class E extends Error {}; E.prototype.name = 'P'; R = new E('m').stack.split('\\n')[0]",
  "function E(m) { var e = Error.call(this, m); e.name = 'E'; return e }; R = new E('m').stack.split('\\n')[0]",
  "var e = new Error('x'); e.name = 'Changed'; R = e.stack.split('\\n')[0]",
  "var e = new Error('line1\\nline2'); R = JSON.stringify(e.stack.split('\\n').slice(0, 3))",
  "var e = new Error(''); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new Error(); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new Error(undefined); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new Error(null); R = JSON.stringify(e.stack.split('\\n')[0])",
  "var e = new Error({ toString() { return 'obj' } }); R = e.stack.split('\\n')[0]",
  "var e = new Error(Symbol.iterator.description); R = e.stack.split('\\n')[0]",
  "var e = new TypeError('t'); R = e.stack.split('\\n')[0]",
  "var e = new RangeError('r'); R = e.stack.split('\\n')[0]",
  "var e = new SyntaxError('s'); R = e.stack.split('\\n')[0]",
  "var e = new ReferenceError('r'); R = e.stack.split('\\n')[0]",
  "var e = new EvalError('e'); R = e.stack.split('\\n')[0]",
  "var e = new URIError('u'); R = e.stack.split('\\n')[0]",
  "var e = new SuppressedError(new Error('a'), new Error('b'), 'm'); R = e.stack.split('\\n')[0]",
  "var e = Error('x'); R = e.stack.split('\\n').length > 1",
  "var e = TypeError('x'); R = e.stack.split('\\n')[0]",
  "var e = new Error('x'); R = e.stack.split('\\n').slice(1).every(l => /^    at /.test(l))",
  "function f() { return new Error('x') }; R = f().stack.split('\\n')[1]",
  "function f() { return new Error('x') }; R = /\\(error_stack_case\\.js:\\d+:\\d+\\)$/.test(f().stack.split('\\n')[1])",
  "R = new Error('x').stack.split('\\n')[1].replace(/\\d+/g, 'N')",
  "function f() { return new Error('x').stack.split('\\n')[1] }; R = f().replace(/\\d+/g, 'N')",
  "R = (() => new Error('x').stack.split('\\n')[1])().replace(/\\d+/g, 'N')",
  "R = typeof Error.prototype.toString.call({ name: 'N', message: 'M' })",
  "R = Error.prototype.toString.call({ name: 'N', message: 'M' })",
  "R = Error.prototype.toString.call({})",
  "R = Error.prototype.toString.call({ name: '', message: 'M' })",
  "R = Error.prototype.toString.call({ name: 'N', message: '' })",
  "try { Error.prototype.toString.call(1) } catch (e) { R = e.constructor.name + ':' + e.message }",
  "R = String(new Error('x', { cause: 'c' }))",
);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "error-stack-golden-"));
const file = path.join(dir, "error_stack_case.js");
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
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 15000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\/|\/var\/folders\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("error_stack", lines));
fs.rmSync(dir, { recursive: true, force: true });
