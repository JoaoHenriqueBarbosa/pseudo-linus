// Gera tests/golden/proxy_reflect_bun.tsv: Proxy e Reflect em cenários avançados que os outros golden de proxy
// (proxy_bun, proxy_class_bun, proxy_trace_bun, reflect_bun) não cobrem: invariantes de cada trap contra alvos
// não configuráveis, não graváveis, não extensíveis, selados e congelados; Proxy.revocable e o uso depois da
// revogação; Proxy como protótipo (get, set, has, receiver, with, class extends); Proxy em for-in, Object.keys,
// JSON.stringify, Array.isArray, spread, instanceof, Object.assign e Array.from; Reflect.construct com newTarget;
// ordem de Reflect.ownKeys; e as mensagens de TypeError exatas. Tudo medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança). Sem APIs de host.
// Uso: bun scripts/gen-proxy-reflect-golden.js > tests/golden/proxy_reflect_bun.tsv
const { emitFactored } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);

// Formatador estável de valores e log de traps. `LH(t)` devolve um Proxy de log como handler: cada trap
// registra `nome:chave` e repassa para Reflect.
const PRELUDE =
  "var L = [];\n" +
  "function F(v) { try {\n" +
  "  if (typeof v === 'string') return JSON.stringify(v);\n" +
  "  if (typeof v === 'symbol') return v.toString();\n" +
  "  if (typeof v === 'function') return 'fn';\n" +
  "  if (typeof v === 'bigint') return v + 'n';\n" +
  "  if (Object.is(v, -0)) return '-0';\n" +
  "  if (v === null || typeof v !== 'object') return String(v);\n" +
  "  if (Array.isArray(v)) return '[' + v.map(F).join(',') + ']';\n" +
  "  return '{' + Reflect.ownKeys(v).map(function (k) { return String(k) + ':' + F(v[k]) }).join(',') + '}';\n" +
  "} catch (e) { return '!' + e.name } }\n" +
  "function LH(extra) { return new Proxy(extra || {}, { get: function (h, name) {\n" +
  "  if (extra && name in extra) return function () { L.push(name + ':' + (typeof arguments[1] === 'symbol' ? 'sym' : typeof arguments[1] === 'string' ? arguments[1] : '')); return extra[name].apply(this, arguments) };\n" +
  "  return function () { L.push(name + ':' + (typeof arguments[1] === 'symbol' ? 'sym' : typeof arguments[1] === 'string' ? arguments[1] : '')); return Reflect[name].apply(null, arguments) } } }) }\n" +
  "function Q(f) { try { return F(f()) } catch (e) { return e.name + ': ' + e.message } }\n";
// Programa com captura; o corpo atribui R.
const T = body => add(`${PRELUDE}try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Programa que mede o resultado de uma expressão (função) e o log de traps.
const O = (setup, expr) => T(`${setup}; var r = Q(function () { 'use strict'; return ${expr} }); R = r + ' | ' + L.join(',')`);
const OS = (setup, expr) => T(`${setup}; var r = Q(function () { return ${expr} }); R = r + ' | ' + L.join(',')`);

// ---- 1. Invariantes por trap contra alvos de formas diferentes.
const targets = {
  ncnw: "var t = {}; Object.defineProperty(t, 'a', { value: 1 })",
  accessorNoGet: "var t = {}; Object.defineProperty(t, 'a', { get: undefined, set: undefined })",
  accessorGet: "var t = {}; Object.defineProperty(t, 'a', { get: function () { return 1 }, set: undefined })",
  ncw: "var t = {}; Object.defineProperty(t, 'a', { value: 1, writable: true })",
  nonExt: "var t = { a: 1 }; Object.preventExtensions(t)",
  frozen: "var t = { a: 1 }; Object.freeze(t)",
  plain: "var t = { a: 1 }",
  emptyNonExt: "var t = {}; Object.preventExtensions(t)",
  sealed: "var t = { a: 1, b: 2 }; Object.seal(t)",
};
const handlerOps = [
  // [trap, retorno, operação]
  ["get", "2", "p.a"], ["get", "undefined", "p.a"], ["get", "1", "p.a"], ["get", "undefined", "Reflect.get(p, 'a')"],
  ["set", "true", "(p.a = 2, 'ok')"], ["set", "false", "(p.a = 2, 'ok')"], ["set", "true", "Reflect.set(p, 'a', 1)"],
  ["set", "true", "Reflect.set(p, 'a', 2)"], ["set", "false", "Reflect.set(p, 'a', 2)"],
  ["has", "false", "'a' in p"], ["has", "true", "'a' in p"], ["has", "false", "'z' in p"], ["has", "true", "Reflect.has(p, 'z')"],
  ["deleteProperty", "true", "delete p.a"], ["deleteProperty", "false", "delete p.a"], ["deleteProperty", "true", "Reflect.deleteProperty(p, 'a')"],
  ["defineProperty", "true", "Object.defineProperty(p, 'a', { value: 3 })"],
  ["defineProperty", "false", "Object.defineProperty(p, 'a', { value: 3 })"],
  ["defineProperty", "true", "Reflect.defineProperty(p, 'a', { value: 1 })"],
  ["defineProperty", "true", "Reflect.defineProperty(p, 'a', { value: 3, configurable: false })"],
  ["defineProperty", "true", "Reflect.defineProperty(p, 'z', { value: 3, configurable: false })"],
  ["defineProperty", "true", "Reflect.defineProperty(p, 'z', { value: 3, configurable: true })"],
  ["defineProperty", "true", "Reflect.defineProperty(p, 'a', { configurable: false, writable: false })"],
  ["getOwnPropertyDescriptor", "undefined", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "{ value: 1, configurable: true }", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "{ value: 1, configurable: false }", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "{ value: 1, configurable: false, writable: true }", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "{ value: 1, configurable: false, writable: false }", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "{ value: 9, configurable: true }", "F(Object.getOwnPropertyDescriptor(p, 'z'))"],
  ["getOwnPropertyDescriptor", "{ get: undefined, configurable: false }", "F(Object.getOwnPropertyDescriptor(p, 'a'))"],
  ["getOwnPropertyDescriptor", "5", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["ownKeys", "[]", "F(Reflect.ownKeys(p))"], ["ownKeys", "['a']", "F(Reflect.ownKeys(p))"], ["ownKeys", "['a', 'a']", "F(Reflect.ownKeys(p))"],
  ["ownKeys", "['a', 1]", "F(Reflect.ownKeys(p))"], ["ownKeys", "5", "F(Reflect.ownKeys(p))"], ["ownKeys", "['b']", "F(Reflect.ownKeys(p))"],
  ["ownKeys", "['a', 'z']", "F(Reflect.ownKeys(p))"], ["ownKeys", "['b', 'a']", "F(Object.getOwnPropertyNames(p))"],
  ["ownKeys", "[]", "F(Object.keys(p))"], ["ownKeys", "['a']", "F(Object.keys(p))"],
  ["getPrototypeOf", "null", "F(Object.getPrototypeOf(p))"], ["getPrototypeOf", "Array.prototype", "F(Object.getPrototypeOf(p) === Array.prototype)"],
  ["getPrototypeOf", "5", "Object.getPrototypeOf(p)"], ["getPrototypeOf", "Object.prototype", "F(Reflect.getPrototypeOf(p) === Object.prototype)"],
  ["setPrototypeOf", "true", "F(Object.setPrototypeOf(p, null) === p)"], ["setPrototypeOf", "false", "Object.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "true", "Reflect.setPrototypeOf(p, Array.prototype)"], ["setPrototypeOf", "true", "Reflect.setPrototypeOf(p, Object.prototype)"],
  ["setPrototypeOf", "false", "Reflect.setPrototypeOf(p, null)"],
  ["isExtensible", "true", "Object.isExtensible(p)"], ["isExtensible", "false", "Object.isExtensible(p)"], ["isExtensible", "1", "Reflect.isExtensible(p)"],
  ["preventExtensions", "true", "F(Object.preventExtensions(p) === p)"], ["preventExtensions", "false", "Object.preventExtensions(p)"],
  ["preventExtensions", "true", "Reflect.preventExtensions(p)"], ["preventExtensions", "false", "Reflect.preventExtensions(p)"],
  ["preventExtensions", "true", "Object.freeze(p) === p"], ["preventExtensions", "true", "Object.seal(p) === p"],
];
for (const [name, setup] of ["ncnw", "accessorNoGet", "nonExt", "frozen"].map(key => [key, targets[key]])) {
  for (const [trap, ret, expr] of handlerOps) {
    O(`${setup}; var p = new Proxy(t, LH({ ${trap}: function () { return ${ret} } }))`, expr);
  }
}

// ---- 2. Traps sem retorno, com this e com argumentos observados.
for (const trap of ["get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf",
  "setPrototypeOf", "isExtensible", "preventExtensions", "apply", "construct"]) {
  T(`var h = { ${trap}: function () { return [this === h, arguments.length, typeof arguments[0]].join() } };
var p = new Proxy(function () {}, h); var r;
try { r = ({ get: () => p.x, set: () => (p.x = 1, 'ok'), has: () => 'x' in p, deleteProperty: () => delete p.x,
  defineProperty: () => Reflect.defineProperty(p, 'x', {}), getOwnPropertyDescriptor: () => Reflect.getOwnPropertyDescriptor(p, 'x'),
  ownKeys: () => Reflect.ownKeys(p), getPrototypeOf: () => Reflect.getPrototypeOf(p), setPrototypeOf: () => Reflect.setPrototypeOf(p, null),
  isExtensible: () => Reflect.isExtensible(p), preventExtensions: () => Reflect.preventExtensions(p), apply: () => p(1, 2),
  construct: () => new p(1, 2, 3) })['${trap}']() } catch (e) { r = e.name + ': ' + e.message }
R = F(r)`);
}
// handler com trap não função
for (const bad of ["1", "'s'", "{}", "null", "undefined", "true", "Symbol()", "[]"]) {
  for (const op of ["p.x", "'x' in p", "Object.keys(p)", "delete p.x", "Object.getPrototypeOf(p)", "p()", "Object.isExtensible(p)"]) {
    T(`var p = new Proxy(function () {}, { get: ${bad}, has: ${bad}, ownKeys: ${bad}, deleteProperty: ${bad}, getPrototypeOf: ${bad}, apply: ${bad}, isExtensible: ${bad} });
R = Q(function () { return F(${op}) })`);
  }
}
// handler com getter que lança ou muda
T("var p = new Proxy({}, { get get() { throw new RangeError('lookup') } }); R = Q(function () { return p.x })");
T("var n = 0; var p = new Proxy({ x: 1 }, { get get() { n++; return undefined } }); p.x; p.x; R = n");
T("var h = {}; var p = new Proxy({ x: 1 }, h); var a = p.x; h.get = function () { return 'late' }; R = a + p.x");
T("var h = { get: function () { return 'a' } }; var p = new Proxy({}, h); var a = p.x; delete h.get; R = a + String(p.x)");
T("var h = new Proxy({}, { get: function (t, k) { L.push(String(k)); return undefined } }); var p = new Proxy({ a: 1 }, h); p.a; 'a' in p; R = L.join()");
T("var p = new Proxy({}, new Proxy({}, {})); p.x = 1; R = F(Object.keys(p)) + F(p.x)");
T("var h = new Proxy({}, { get: function (t, k) { return k === 'get' ? function (t, key, r) { return key + ':' + (r === p) } : undefined } }); var p = new Proxy({}, h); R = p.zz");

// ---- 3. Proxy.revocable e uso depois da revogação.
const revOps = ["p.x", "p.x = 1", "'x' in p", "delete p.x", "Object.keys(p)", "Object.getPrototypeOf(p)", "Object.setPrototypeOf(p, null)",
  "Object.isExtensible(p)", "Object.preventExtensions(p)", "Object.getOwnPropertyDescriptor(p, 'x')", "Object.defineProperty(p, 'x', {})",
  "p()", "new p()", "Reflect.ownKeys(p)", "Reflect.get(p, 'x')", "Reflect.has(p, 'x')", "typeof p", "Array.isArray(p)", "JSON.stringify(p)",
  "String(p)", "p + ''", "Object.prototype.toString.call(p)", "[...p]", "({ ...p })", "for (var k in p); 'done'", "p instanceof Object",
  "({}) instanceof p", "Object.assign({}, p)", "Object.entries(p)", "Object.freeze(p)", "Object.isFrozen(p)", "Object.hasOwn(p, 'x')",
  "p.hasOwnProperty", "Reflect.getPrototypeOf(p)", "Reflect.apply(p, null, [])", "Reflect.construct(p, [])", "Object.create(p).x",
  "Promise.resolve(p) instanceof Promise", "Function.prototype.call.call(p)", "Array.from(p)", "typeof Object.prototype.valueOf.call(p)"];
for (const kind of ["{}", "function () {}"]) {
  for (const op of revOps) T(`var rv = Proxy.revocable(${kind}, {}); var p = rv.proxy; rv.revoke(); R = Q(function () { return ${op} })`);
}
T("var rv = Proxy.revocable({}, {}); R = Object.keys(rv).join() + ':' + typeof rv.proxy + ':' + typeof rv.revoke + ':' + rv.revoke.length + ':' + rv.revoke.name + ':' + Object.getPrototypeOf(rv) === Object.prototype");
T("var rv = Proxy.revocable({}, {}); R = rv.revoke.name === '' ? 'empty' : rv.revoke.name; R += ':' + F(Object.getOwnPropertyNames(rv.revoke)) + ':' + Object.hasOwn(rv.revoke, 'prototype')");
T("var rv = Proxy.revocable({}, {}); R = F(rv.revoke()) + F(rv.revoke())");
T("var rv = Proxy.revocable({}, {}); rv.revoke(); R = F(Q(function () { return new Proxy(rv.proxy, {}) }))");
T("var rv = Proxy.revocable({}, {}); rv.revoke(); R = F(Q(function () { return Proxy.revocable(rv.proxy, {}) }))");
T("var rv = Proxy.revocable({}, {}); rv.revoke(); R = F(Q(function () { return new Proxy({}, rv.proxy) }))");
T("var rv = Proxy.revocable({}, { get: function () { rv.revoke(); return 1 } }); R = Q(function () { return rv.proxy.a }) + ':' + Q(function () { return rv.proxy.a })");
T("var rv = Proxy.revocable({ a: 1 }, { ownKeys: function (t) { rv.revoke(); return Reflect.ownKeys(t) } }); R = Q(function () { return Object.keys(rv.proxy) })");
T("var rv = Proxy.revocable({ a: 1 }, { has: function (t, k) { rv.revoke(); return true } }); R = Q(function () { return 'a' in rv.proxy }) + Q(function () { return 'a' in rv.proxy })");
T("var rv = Proxy.revocable({}, {}); var r = rv.revoke; R = Q(function () { return r.call(null) }) + Q(function () { return new r() })");
T("var rv = Proxy.revocable(function () {}, {}); rv.revoke(); R = typeof rv.proxy");
T("var rv = Proxy.revocable({}, {}); rv.revoke(); R = typeof rv.proxy");
T("var rv = Proxy.revocable([], {}); rv.revoke(); R = Q(function () { return Array.isArray(rv.proxy) })");
T("R = Q(function () { return Proxy.revocable() })");
T("R = Q(function () { return Proxy.revocable({}) })");
T("R = Q(function () { return new Proxy.revocable({}, {}) })");
T("R = Q(function () { return Proxy.revocable(1, {}) })");
T("R = Q(function () { return Proxy.revocable({}, 1) })");
T("R = Q(function () { return Proxy.revocable.length + ':' + Proxy.revocable.name })");
T("R = Q(function () { return Proxy({}, {}) })");
T("R = Q(function () { return new Proxy() })");
T("R = Q(function () { return new Proxy({}) })");
T("R = Q(function () { return new Proxy(1, {}) })");
T("R = Q(function () { return new Proxy({}, null) })");
T("R = Q(function () { return new Proxy(null, {}) })");
T("R = Q(function () { return new Proxy('s', {}) })");
T("R = Q(function () { return new Proxy(Symbol(), {}) })");
T("R = Q(function () { return Proxy.length + ':' + Proxy.name + ':' + typeof Proxy.prototype + ':' + F(Object.getOwnPropertyNames(Proxy)) })");
T("R = Q(function () { return Reflect.construct(Proxy, [{}, {}]) instanceof Object })");
T("R = Q(function () { return Reflect.construct(Proxy, [{}, {}], function () {}) instanceof Object })");
T("R = Q(function () { return Reflect.apply(Proxy, null, [{}, {}]) })");
T("R = Q(function () { return Object.prototype.toString.call(new Proxy([], {})) + Object.prototype.toString.call(new Proxy(function () {}, {})) + Object.prototype.toString.call(new Proxy(new Date(0), {})) })");
T("R = Q(function () { return Date.prototype.getTime.call(new Proxy(new Date(0), {})) })");
T("R = Q(function () { return Map.prototype.get.call(new Proxy(new Map(), {}), 1) })");
T("R = Q(function () { return Object.getOwnPropertyDescriptor(new Proxy(class { static a = 1 }, {}), 'a').value })");

// ---- 4. Proxy como protótipo.
const protoHandlers = {
  get: "get: function (t, k, r) { L.push('get:' + String(k) + ':' + (r === o)); return 'P' + String(k) }",
  has: "has: function (t, k) { L.push('has:' + String(k)); return k === 'yes' }",
  set: "set: function (t, k, v, r) { L.push('set:' + String(k) + ':' + (r === o)); return true }",
  setFalse: "set: function (t, k, v, r) { L.push('set:' + String(k)); return false }",
  gopd: "getOwnPropertyDescriptor: function (t, k) { L.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k) }",
  def: "defineProperty: function (t, k, d) { L.push('def:' + String(k)); return Reflect.defineProperty(t, k, d) }",
  all: "get: function (t, k, r) { L.push('get:' + String(k)); return Reflect.get(t, k, r) }, has: function (t, k) { L.push('has:' + String(k)); return Reflect.has(t, k) }, set: function (t, k, v, r) { L.push('set:' + String(k)); return Reflect.set(t, k, v, r) }",
};
const protoOps = ["o.x", "o.own", "'x' in o", "'yes' in o", "o.x = 1", "(o.own = 2, F(Object.keys(o)))", "Reflect.get(o, 'x')", "Reflect.set(o, 'x', 1)",
  "Reflect.has(o, 'x')", "o.hasOwnProperty('x')", "Object.hasOwn(o, 'x')", "o.toString === Object.prototype.toString", "o[Symbol.iterator]",
  "o[Symbol.toPrimitive]", "String(o)", "o + ''", "o.constructor", "o[0]", "o['0'] = 1", "delete o.x", "Object.getOwnPropertyNames(o).join()",
  "Object.entries(o).length", "F(Object.assign({}, o))", "(function () { var n = 0; for (var k in o) n++; return n })()", "JSON.stringify(o)",
  "Object.getPrototypeOf(o) === pr", "pr.isPrototypeOf(o)", "o instanceof Object", "(0, function () { with (o) { return typeof zzz } })()",
  "Object.defineProperty(o, 'x', { value: 1 }).x"];
for (const [hname, h] of Object.entries(protoHandlers)) {
  for (const op of protoOps) {
    OS(`var pr = new Proxy({}, { ${h} }); var o = Object.create(pr); o.own = 0; delete o.own; o.own = 1`.replace("o.own = 0; delete o.own; ", ""), op);
    if (hname === "get" || hname === "all") OS(`var pr = new Proxy({ y: 1 }, { ${h} }); var o = Object.create(pr)`, op);
  }
}
T("var pr = new Proxy({}, { get: function (t, k, r) { return r === o } }); var o = Object.create(pr); R = o.anything");
T("var pr = new Proxy({}, { set: function (t, k, v, r) { Object.defineProperty(r, k, { value: 'via' + v, configurable: true }); return true } }); var o = Object.create(pr); o.q = 1; R = F(Object.getOwnPropertyDescriptor(o, 'q'))");
T("class B { constructor() { this.b = 1 } } var P = new Proxy(B, {}); class D extends P { constructor() { super(); this.d = 2 } } R = F(new D()) + (new D() instanceof B) + (new D() instanceof P)");
T("class B {} var P = new Proxy(B, { get: function (t, k, r) { L.push(String(k)); return Reflect.get(t, k, r) } }); class D extends P {} R = L.join() + ':' + (Object.getPrototypeOf(D) === P)");
T("class B {} var P = new Proxy(B, { construct: function (t, a, nt) { L.push('c:' + (nt === D)); return Reflect.construct(t, a, nt) } }); class D extends P {} var d = new D(); R = L.join() + ':' + (Object.getPrototypeOf(d) === D.prototype)");
T("function B() {} var P = new Proxy(B, { get: function (t, k, r) { return k === 'prototype' ? { tag: 1 } : Reflect.get(t, k, r) } }); class D extends P {} R = F(Object.getPrototypeOf(D.prototype))");
T("function B() {} var P = new Proxy(B, { get: function (t, k, r) { return k === 'prototype' ? 5 : Reflect.get(t, k, r) } }); R = Q(function () { class D extends P {}; return 1 })");
T("var P = new Proxy(function () {}, { get: function (t, k, r) { return k === 'prototype' ? null : Reflect.get(t, k, r) } }); class D extends P {} R = Object.getPrototypeOf(D.prototype)");
T("var P = new Proxy({}, {}); R = Q(function () { class D extends P {} })");
T("var P = new Proxy(function () {}, {}); class D extends P {} R = F(Object.getPrototypeOf(D.prototype) === P.prototype) + (Object.getPrototypeOf(D) === P)");
T("var pr = new Proxy({}, { getPrototypeOf: function () { L.push('gpo'); return Array.prototype } }); var o = Object.create(pr); R = (o instanceof Array) + ':' + Array.isArray(o) + ':' + L.join()");
T("var pr = new Proxy({}, { getPrototypeOf: function () { L.push('gpo'); return null } }); var o = Object.create(pr); R = (o instanceof Object) + ':' + L.join()");
T("var pr = new Proxy({}, {}); var o = Object.create(pr); Object.setPrototypeOf(o, null); R = Object.getPrototypeOf(o)");
T("var pr = new Proxy({}, { setPrototypeOf: function () { L.push('spo'); return true } }); var o = Object.create(pr); R = Object.setPrototypeOf(pr, {}) === pr ? L.join() : 'x'");
T("var pr = new Proxy({}, {}); R = Q(function () { return Object.setPrototypeOf(pr, pr) })");
T("var a = {}; var p = new Proxy(a, {}); R = Q(function () { return Object.setPrototypeOf(a, p) })");
T("var a = {}; var p = new Proxy(a, {}); R = Q(function () { a.__proto__ = p; return 'ok' })");
T("var a = {}; var p = new Proxy(a, { getPrototypeOf: function () { return p } }); R = Q(function () { return Object.getPrototypeOf(p) === p }) + Q(function () { return p instanceof Object }) ");
T("var a = {}; var p = new Proxy(a, { getPrototypeOf: function () { return p } }); R = Q(function () { return p.isPrototypeOf({}) }) + Q(function () { return Object.prototype.isPrototypeOf.call(p, p) })");
T("var pr = new Proxy({}, { has: function (t, k) { L.push('has:' + String(k)); return false } }); var o = Object.create(pr); R = (function () { with (o) { return typeof zzzzz } })() + ':' + L.join()");
T("var pr = new Proxy({}, { has: function (t, k) { L.push('has:' + String(k)); return k === 'wx' }, get: function (t, k) { L.push('get:' + String(k)); return 7 } }); R = (function () { with (pr) { return wx } })() + ':' + L.join()");
T("var pr = new Proxy({ wx: 1 }, { has: function (t, k) { L.push('has:' + String(k)); return true }, get: function (t, k) { L.push('get:' + String(k)); return k === Symbol.unscopables ? { wx: true } : 1 } }); R = (function () { var wx = 'outer'; with (pr) { return wx } })() + ':' + L.join()");
T("var pr = new Proxy({ wx: 1 }, { has: function (t, k) { L.push('has:' + String(k)); return true }, set: function (t, k, v) { L.push('set:' + String(k)); return true } }); (function () { with (pr) { wx = 5 } })(); R = L.join()");

// ---- 5. Proxy nas operações da linguagem (traps observados).
const subjects = {
  obj: "{ a: 1, b: 2, [Symbol('s')]: 3 }",
  arr: "[1, 2, 3]",
  fn: "function () { return 1 }",
  nested: "{ a: { b: 1 }, c: [1, { d: 2 }] }",
  withProto: "Object.assign(Object.create({ inherited: 1 }), { own: 2 })",
  nonEnum: "Object.defineProperty({ vis: 1 }, 'hid', { value: 2, enumerable: false })",
  withToJSON: "{ toJSON: function () { return 'custom' }, a: 1 }",
};
const ops = [
  "F(Object.keys(p))", "F(Object.values(p))", "F(Object.entries(p))", "F(Object.getOwnPropertyNames(p))", "F(Reflect.ownKeys(p))",
  "F(Object.getOwnPropertySymbols(p))", "F(Object.getOwnPropertyDescriptors(p))", "JSON.stringify(p)", "JSON.stringify({ w: p })",
  "JSON.stringify([p])", "JSON.stringify(p, null, 1)", "JSON.stringify(p, ['a'])", "Array.isArray(p)", "Array.isArray({ p: 1 }) + ':' + Array.isArray(p)",
  "F({ ...p })", "F(Object.assign({}, p))", "F([...(function* () { for (var k in p) yield k })()])", "(function () { var r = []; for (var k in p) r.push(k); return r.join() })()",
  "(function () { var r = []; for (var k in p) r.push(k); return r.join() })() + ':' + F(Object.keys(p))",
  "p instanceof Object", "p instanceof Array", "p instanceof Function", "Object.prototype.isPrototypeOf.call(Object.prototype, p)",
  "Object.isFrozen(p)", "Object.isSealed(p)", "Object.isExtensible(p)", "F(Object.freeze(p) === p)", "F(Object.seal(p) === p)",
  "F(Object.preventExtensions(p) === p)", "Object.prototype.toString.call(p)", "String(Object.prototype.hasOwnProperty.call(p, 'a'))",
  "'a' in p", "p.propertyIsEnumerable('a')", "Object.prototype.propertyIsEnumerable.call(p, 'a')", "F(Object.fromEntries(Object.entries(p)))",
  "F(structuredCloneLike(p))", "F(Array.from(p))", "F(Array.prototype.concat.call([], p))", "F([].concat(p))",
  "F(Array.prototype.slice.call(p))", "F(Array.prototype.map.call(p, function (x) { return x }))", "Array.prototype.join.call(p)",
  "F(Array.prototype.indexOf.call(p, 2))", "F(Array.prototype.includes.call(p, 2))", "F(Array.prototype.reverse.call(p))",
  "F(Array.prototype.push.call(p, 9))", "F(Array.prototype.pop.call(p))", "F(Array.prototype.sort.call(p))",
  "F(Array.prototype.splice.call(p, 0, 1))", "F(Array.prototype.fill.call(p, 0))", "F(Array.prototype.flat.call(p))",
  "p.length", "F(Object.groupBy(p, function (x) { return 'k' }))", "typeof p", "p == p", "p === p", "p + 1", "`${p}`", "Number(p)", "Symbol.keyFor(Symbol.for('k'))",
  "Reflect.ownKeys(p).length", "p.hasOwnProperty('a')", "Object.hasOwn(p, 'a')", "p.valueOf() === p", "p.toString()", "p.constructor === Object",
  "F(Object.entries(Object.getOwnPropertyDescriptors(p)).length)", "delete p.a", "(p.a = 5, F(p))", "Object.defineProperty(p, 'n', { value: 1 }) === p",
  "F(Reflect.getOwnPropertyDescriptor(p, 'a'))", "F(new Set(p))", "F(new Map([[1, p]]).get(1) === p)", "F(Object.keys(Object.create(p)))",
  "F(Object.getOwnPropertyNames(Object.create(p)))", "(function () { var r = []; for (var k in Object.create(p)) r.push(k); return r.join() })()",
];
for (const [name, subject] of ["obj", "arr", "fn"].map(key => [key, subjects[key]])) {
  for (const op of ops.filter((_, index) => index % 2 === (name === "arr" ? 1 : 0))) {
    // O corpo dos programas usa uma função auxiliar (structuredCloneLike) que não existe: troca por JSON.
    const expr = op.replace("structuredCloneLike", "JSON.parse(JSON.stringify.bind(JSON))&&(function(x){return x})");
    OS(`var p = new Proxy(${subject}, LH())`, expr);
  }
}

// ---- 6. Reflect.construct com newTarget.
const ctorCases = [
  ["function A() { this.k = new.target === B }", "function B() {}", "Reflect.construct(A, [], B)"],
  ["function A() { this.k = new.target === A }", "function B() {}", "Reflect.construct(A, [])"],
  ["function A() {}", "function B() {}", "Object.getPrototypeOf(Reflect.construct(A, [], B)) === B.prototype"],
  ["function A() {}", "function B() {}; B.prototype = null", "Object.getPrototypeOf(Reflect.construct(A, [], B)) === Object.prototype"],
  ["function A() {}", "function B() {}; B.prototype = 5", "Object.getPrototypeOf(Reflect.construct(A, [], B)) === Object.prototype"],
  ["function A() {}", "var B = function () {}; B.prototype = { tag: 1 }", "Reflect.construct(A, [], B).tag"],
  ["class A { constructor() { this.n = new.target.name } }", "class B {}", "Reflect.construct(A, [], B).n"],
  ["class A { constructor() { this.n = new.target.name } }", "function Bf() {}", "Reflect.construct(A, [], Bf).n"],
  ["class A { x = 1 }", "class B { y = 2 }", "F(Reflect.construct(A, [], B))"],
  ["class A { x = 1 }", "class B { y = 2 }", "Reflect.construct(A, [], B) instanceof B"],
  ["class A { x = 1 }", "class B extends A {}", "Reflect.construct(A, [], B) instanceof B"],
  ["class A { constructor(a, b) { this.s = a + b } }", "class B {}", "Reflect.construct(A, [1, 2], B).s"],
  ["class A { constructor(a, b) { this.s = [a, b] } }", "class B {}", "F(Reflect.construct(A, { length: 2, 0: 'x', 1: 'y' }, B).s)"],
  ["class A {}", "class B {}", "Reflect.construct(A, 1, B)"],
  ["class A {}", "class B {}", "Reflect.construct(A, null, B)"],
  ["class A {}", "class B {}", "Reflect.construct(A, undefined, B)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], 1)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], null)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], undefined)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], {})"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], () => {})"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], function* g() {})"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], async function f() {})"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], Math.max)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], Symbol)"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], BigInt)"],
  ["class A {}", "class B {}", "Reflect.construct(() => {}, [])"],
  ["class A {}", "class B {}", "Reflect.construct(Math.max, [])"],
  ["class A {}", "class B {}", "Reflect.construct({ m() {} }.m, [])"],
  ["class A {}", "class B {}", "Reflect.construct(function* g() {}, [])"],
  ["class A {}", "class B {}", "Reflect.construct(async function f() {}, [])"],
  ["class A {}", "class B {}", "Reflect.construct()"],
  ["class A {}", "class B {}", "Reflect.construct(A)"],
  ["class A {}", "class B {}", "Reflect.construct(1, [])"],
  ["class A {}", "class B {}", "Reflect.construct({}, [])"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], new Proxy(B, {}))"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], new Proxy(() => {}, {}))"],
  ["class A {}", "class B {}", "Reflect.construct(A, [], Proxy.revocable(B, {}).proxy) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Array, [3], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Array, [3], B).length"],
  ["class A {}", "class B {}", "Array.isArray(Reflect.construct(Array, [3], B))"],
  ["class A {}", "class B {}", "Reflect.construct(Error, ['m'], B) instanceof Error"],
  ["class A {}", "class B {}", "Reflect.construct(Error, ['m'], B).message"],
  ["class A {}", "class B {}", "Object.getPrototypeOf(Reflect.construct(Error, ['m'], B)) === B.prototype"],
  ["class A {}", "class B {}", "Object.prototype.toString.call(Reflect.construct(Error, ['m'], B))"],
  ["class A {}", "class B {}", "Object.prototype.toString.call(Reflect.construct(Date, [0], B))"],
  ["class A {}", "class B {}", "Date.prototype.getTime.call(Reflect.construct(Date, [5], B))"],
  ["class A {}", "class B {}", "Object.prototype.toString.call(Reflect.construct(Map, [], B))"],
  ["class A {}", "class B {}", "Map.prototype.size === undefined"],
  ["class A {}", "class B {}", "Reflect.construct(Map, [[[1, 2]]], B).get(1)"],
  ["class A {}", "class B {}", "Reflect.construct(Set, [[1, 2]], B).size"],
  ["class A {}", "class B {}", "Reflect.construct(WeakMap, [], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Promise, [function (r) { r(1) }], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Promise, [function () {}], B) instanceof Promise"],
  ["class A {}", "class B {}", "Reflect.construct(RegExp, ['a', 'g'], B).flags"],
  ["class A {}", "class B {}", "Reflect.construct(RegExp, ['a', 'g'], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Function, ['return 7'], B)()"],
  ["class A {}", "class B {}", "Reflect.construct(Function, ['return 7'], B) instanceof B"],
  ["class A {}", "class B {}", "typeof Reflect.construct(Object, [], B)"],
  ["class A {}", "class B {}", "Reflect.construct(Object, [], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Object, [1], B) instanceof B"],
  ["class A {}", "class B {}", "typeof Reflect.construct(Number, [1], B)"],
  ["class A {}", "class B {}", "Reflect.construct(Number, [1], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(String, ['ab'], B).length"],
  ["class A {}", "class B {}", "Reflect.construct(Boolean, [0], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Boolean, [0], B).valueOf()"],
  ["class A {}", "class B {}", "Reflect.construct(Uint8Array, [2], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Uint8Array, [2], B).length"],
  ["class A {}", "class B {}", "Reflect.construct(ArrayBuffer, [4], B).byteLength"],
  ["class A {}", "class B {}", "Reflect.construct(DataView, [new ArrayBuffer(4)], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Symbol, [], B)"],
  ["class A {}", "class B {}", "Reflect.construct(BigInt, [1], B)"],
  ["class A {}", "class B {}", "Reflect.construct(Proxy, [{}, {}], B) instanceof B"],
  ["class A {}", "class B {}", "Reflect.construct(Reflect.construct, [A, []])"],
  ["class A {}", "class B {}", "Reflect.construct(Reflect.construct, [A, [], B]) instanceof B"],
  ["class A { constructor() { return { custom: 1 } } }", "class B {}", "F(Reflect.construct(A, [], B))"],
  ["class A { constructor() { return 5 } }", "class B {}", "F(Reflect.construct(A, [], B))"],
  ["class A extends Object { constructor() { super(); this.z = 1 } }", "class B {}", "Reflect.construct(A, [], B) instanceof B"],
  ["class A extends Array {}", "class B {}", "Reflect.construct(A, [2], B) instanceof B"],
  ["class A extends Array {}", "class B {}", "Array.isArray(Reflect.construct(A, [2], B))"],
  ["class A extends Array {}", "class B extends Array {}", "Reflect.construct(A, [2, 3], B).length"],
  ["class A extends Error {}", "class B {}", "Reflect.construct(A, ['q'], B).message"],
  ["class A { static #p = 1; static has(o) { return #p in o } }", "class B {}", "A.has(Reflect.construct(A, [], B))"],
  ["class A { #p = 1; static has(o) { return #p in o } }", "class B {}", "A.has(Reflect.construct(A, [], B))"],
  ["class A { #p = 1; static has(o) { return #p in o } }", "class B {}", "A.has(Reflect.construct(B, [], A))"],
  ["class A { #p = 1; static has(o) { return #p in o } }", "class B {}", "A.has(Reflect.construct(Object, [], A))"],
  ["function A() { return new.target }", "function B() {}", "Reflect.construct(A, [], B) === B"],
  ["function A() { return new.target }", "function B() {}", "typeof Reflect.construct(A, [])"],
  ["function A() { return new.target }", "function B() {}", "Reflect.construct(A, [], Object) === Object"],
  ["function A() { return this }", "function B() {}", "Reflect.construct(A, [], B) instanceof B"],
  ["function A() { 'use strict'; return this }", "function B() {}", "Reflect.construct(A, [], B) instanceof B"],
  ["function A() { return arguments.length }", "function B() {}", "typeof Reflect.construct(A, [1, 2, 3], B)"],
  ["function A() { this.a = [].slice.call(arguments) }", "function B() {}", "F(Reflect.construct(A, [1, 2, 3], B).a)"],
  ["function A() { this.a = arguments.length }", "function B() {}", "Reflect.construct(A, new Array(5), B).a"],
  ["function A() { this.a = arguments.length }", "function B() {}", "Reflect.construct(A, { length: 3 }, B).a"],
  ["function A() { this.a = arguments.length }", "function B() {}", "Reflect.construct(A, 'ab', B).a"],
  ["function A() { this.a = arguments.length }", "function B() {}", "Reflect.construct(A, new Proxy([1, 2], {}), B).a"],
  ["function A() { this.a = arguments.length }", "function B() {}", "Reflect.construct(A, { get length() { throw new EvalError('len') } }, B).a"],
  ["function A() { this.a = arguments[0] }", "function B() {}", "Reflect.construct(A, { length: 1, get 0() { return 'g' } }, B).a"],
  ["function A() {}", "function B() {}", "Reflect.construct(A, [], Object.defineProperty(function () {}, 'prototype', { get() { throw new EvalError('pg') } }))"],
  ["function A() {}", "function B() {}", "Reflect.construct(A, [], new Proxy(function () {}, { get(t, k) { throw new EvalError('pg:' + String(k)) } }))"],
  ["function A() {}", "function B() {}", "Reflect.construct(A, [], new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? { viaProxy: 1 } : undefined } })).viaProxy"],
  ["var log = []; function A() {}", "function B() {}", "(Reflect.construct(A, [], new Proxy(B, LH())), L.join())"],
  ["var log = []; function A() {}", "function B() {}", "(Reflect.construct(new Proxy(A, LH()), [1], B), L.join())"],
  ["var log = []; function A() {}", "function B() {}", "(Reflect.construct(new Proxy(A, LH()), [1]), L.join())"],
  ["var log = []; function A() {}", "function B() {}", "(new (new Proxy(A, LH()))(), L.join())"],
  ["var log = []; function A() {}", "function B() {}", "(Reflect.construct(new Proxy(A, LH({ construct(t, a, nt) { return { forged: nt === B } } })), [], B), L.join())"],
  ["var log = []; function A() {}", "function B() {}", "F(Reflect.construct(new Proxy(A, LH({ construct(t, a, nt) { return { forged: nt === B } } })), [], B))"],
  ["var log = []; function A() {}", "function B() {}", "F(Reflect.construct(new Proxy(A, LH({ construct(t, a, nt) { return 1 } })), [], B))"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(A, { construct() { return undefined } }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(A, { construct() { return null } }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(A, { construct() { return function () {} } }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(A, { construct(t, args) { return { n: args.length, isArr: Array.isArray(args) } } }))(1, 2))"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(() => {}, { construct() { return {} } }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(function () {}, { construct: null }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(function () {}, { construct: 5 }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy({}, { construct() { return {} } }))())"],
  ["var log = []; function A() {}", "function B() {}", "F(Reflect.construct(new Proxy({}, {}), []))"],
  ["var log = []; function A() {}", "function B() {}", "F(Reflect.construct(A, [], new Proxy({}, {})))"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(Math.max, {}))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(class { constructor() { this.v = 1 } }, {}))())"],
  ["var log = []; function A() {}", "function B() {}", "F(new (new Proxy(class { constructor() { this.v = new.target.name } }, {}))())"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(class K { constructor() { this.v = new.target === P } }, {}); return new P().v })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(class K { constructor() { this.v = new.target === P } }, {}); return Reflect.construct(P, []).v })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { this.v = new.target === P }, {}); return new P().v })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { return new.target === P }, {}); return typeof new P() })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { return new.target }, {}); return new P() === P })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { return new.target }, {}); return typeof new P() })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { return this }, {}); return typeof P() })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { 'use strict'; var P = new Proxy(function () { return this }, {}); return typeof P() })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var o = { P: new Proxy(function () { return this }, {}) }; return o.P() === o })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var o = { P: new Proxy(function () { return this }, { apply(t, th, a) { return th } }) }; return o.P() === o })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () { return 1 }, { apply(t, th, a) { return [typeof th, a.length, Array.isArray(a)].join() } }); return P(1, 2, 3) })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () {}, { apply: function () { return 'ap' } }); return Reflect.apply(P, 1, []) + P.call(1) + P.apply(1, [1]) + P.bind(1)() })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () {}, { apply: function () { return 'ap' } }); return P.name + ':' + P.length + ':' + typeof P.prototype })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function () {}, {}); return F(Function.prototype.toString.call(P).length > 0) })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(function foo() {}, {}); return Function.prototype.toString.call(P) })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy({}, {}); return Function.prototype.toString.call(P) })()"],
  ["var log = []; function A() {}", "function B() {}", "(function () { var P = new Proxy(class Z {}, {}); return Function.prototype.toString.call(P) })()"],
];
for (const [decl, decl2, expr] of ctorCases) T(`${decl}; ${decl2}; R = Q(function () { return ${expr} })`);

// ---- 7. Reflect.ownKeys: ordem das chaves.
const ownKeysCases = [
  "{ b: 1, a: 2, 2: 'x', 1: 'y', [Symbol.for('s')]: 3, 10: 'z', '-1': 'n', '01': 'o', '4294967294': 'big', '4294967295': 'over' }",
  "{ z: 1, [Symbol('a')]: 2, y: 3, [Symbol('b')]: 4, 0: 5 }",
  "[5, 6, 7]",
  "Object.assign([1, 2], { x: 1, [Symbol.iterator]: 0 })",
  "function f(a, b) {}",
  "(function () { 'use strict' })",
  "class K { static a = 1; static m() {} static get g() { return 1 } static #p = 1; static [Symbol.for('k')] = 2 }",
  "class K2 { m() {} }.prototype",
  "new Error('m')",
  "'str'",
  "new String('ab')",
  "new Number(1)",
  "Object.assign(new String('ab'), { x: 1, 5: 'f' })",
  "new Uint8Array(2)",
  "Object.assign(new Uint8Array(2), { x: 1 })",
  "/a/g",
  "Object.assign(/a/g, { x: 1 })",
  "new Date(0)",
  "new Map([[1, 2]])",
  "function () {}.bind(null)",
  "(function* g() {})",
  "(async function f() {})",
  "(() => 1)",
  "Math",
  "JSON",
  "Reflect",
  "Symbol.prototype",
  "globalThis.Object.getPrototypeOf(function () {})",
  "(function () { return arguments })(1, 2)",
  "(function () { 'use strict'; return arguments })(1, 2)",
  "Object.defineProperties({}, { b: { value: 1, enumerable: false }, a: { value: 2, enumerable: true }, 3: { value: 3 } })",
  "Object.create({ inherited: 1 }, { own: { value: 1 } })",
  "{ __proto__: { p: 1 }, own: 1 }",
  "{ ['__proto__']: 1, a: 2 }",
  "Object.freeze({ b: 1, a: 2 })",
  "{ get a() { return 1 }, set a(v) {}, b: 2 }",
  "{ 1.5: 1, 1e3: 2, 0x10: 3, '1e3': 4 }",
  "{ [-0]: 1, [0]: 2 }",
  "{ 'a b': 1, 'é': 2, '': 3, ' ': 4 }",
  "new Proxy({ b: 1, a: 2, 1: 0 }, {})",
  "new Proxy({ b: 1 }, { ownKeys: () => ['z', 'b', 'y'] })",
  "new Proxy({}, { ownKeys: () => [Symbol.for('s'), '1', 'a'] })",
  "new Proxy([1, 2], {})",
  "new Proxy(function f() {}, {})",
];
for (const subject of ownKeysCases) {
  OS(`var o = ${subject}`, "F(Reflect.ownKeys(o))");
  OS(`var o = ${subject}`, "F(Object.getOwnPropertyNames(o))");
  OS(`var o = ${subject}`, "F(Object.keys(o))");
  OS(`var o = ${subject}`, "F(Object.getOwnPropertySymbols(o).map(String))");
  OS(`var o = ${subject}`, "(function () { var r = []; for (var k in o) r.push(k); return r.join() })()");
}
T("var o = { b: 1, a: 2 }; delete o.b; o.b = 3; o[1] = 0; R = F(Reflect.ownKeys(o))");
T("var o = { a: 1 }; Object.defineProperty(o, 'a', { value: 2 }); o.b = 1; R = F(Reflect.ownKeys(o))");
T("var o = {}; o[Symbol.for('x')] = 1; o.a = 1; o[Symbol.for('y')] = 1; o[3] = 1; o[2] = 1; R = F(Reflect.ownKeys(o).map(String))");
T("var o = { a: 1 }; Object.preventExtensions(o); R = F(Reflect.ownKeys(o))");
T("R = Q(function () { return Reflect.ownKeys(1) })");
T("R = Q(function () { return Reflect.ownKeys('s') })");
T("R = Q(function () { return Reflect.ownKeys(null) })");
T("R = Q(function () { return Reflect.ownKeys() })");
T("R = Q(function () { return Reflect.ownKeys(Symbol()) })");
T("R = Q(function () { return Reflect.ownKeys(function () {}) })");
T("R = Q(function () { return Reflect.ownKeys([]) })");
T("var big = {}; for (var i = 100; i >= 0; i--) big['k' + i] = i; for (var i = 0; i < 50; i++) big[i * 2] = i; R = F(Reflect.ownKeys(big).slice(0, 8)) + Reflect.ownKeys(big).length");
T("var o = {}; o[4294967294] = 1; o[4294967295] = 1; o[4294967296] = 1; o[-1] = 1; o[1] = 1; R = F(Reflect.ownKeys(o))");
T("var o = Object.create(null); o.a = 1; o[0] = 1; R = F(Reflect.ownKeys(o))");
T("var a = [1, 2, 3]; a.x = 1; a[5] = 6; R = F(Reflect.ownKeys(a))");
T("var a = [1, 2, 3]; a.length = 1; R = F(Reflect.ownKeys(a))");
T("var a = []; a[2] = 1; a[0] = 1; R = F(Reflect.ownKeys(a))");
T("var f = function () {}; f.a = 1; R = F(Reflect.ownKeys(f))");
T("var f = function () {}; f.a = 1; Object.defineProperty(f, 'name', { value: 'n' }); R = F(Reflect.ownKeys(f))");
T("var f = function () {}; delete f.name; f.name2 = 1; R = F(Reflect.ownKeys(f))");
T("class K { static b = 1; static a() {} } R = F(Reflect.ownKeys(K))");
T("var obj = { toString: 1 }; R = F(Reflect.ownKeys(Object.assign(obj, { valueOf: 2 })))");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) } }); Object.keys(p); Object.getOwnPropertyNames(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get: function (t, k, r) { calls.push('get:' + k); return Reflect.get(t, k, r) } }); Object.entries(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get: function (t, k, r) { calls.push('get:' + k); return Reflect.get(t, k, r) } }); Object.assign({}, p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get: function (t, k, r) { calls.push('get:' + k); return Reflect.get(t, k, r) } }); ({ ...p }); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get: function (t, k, r) { calls.push('get:' + k); return Reflect.get(t, k, r) } }); var { a, ...rest } = p; R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get: function (t, k, r) { calls.push('get:' + k); return Reflect.get(t, k, r) } }); Object.freeze(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, defineProperty: function (t, k, d) { calls.push('def:' + k + ':' + Object.keys(d)); return Reflect.defineProperty(t, k, d) }, preventExtensions: function (t) { calls.push('pe'); return Reflect.preventExtensions(t) } }); Object.freeze(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, defineProperty: function (t, k, d) { calls.push('def:' + k + ':' + Object.keys(d)); return Reflect.defineProperty(t, k, d) }, preventExtensions: function (t) { calls.push('pe'); return Reflect.preventExtensions(t) } }); Object.seal(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, isExtensible: function (t) { calls.push('ie'); return Reflect.isExtensible(t) } }); Object.isFrozen(p); R = calls.join()");
T("var calls = []; var p = new Proxy({ a: 1, b: 2 }, { ownKeys: function (t) { calls.push('ok'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor: function (t, k) { calls.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, isExtensible: function (t) { calls.push('ie'); return Reflect.isExtensible(t) } }); Object.isSealed(p); R = calls.join()");

// ---- 8. Mensagens de TypeError exatas dos Reflect e do Proxy.
const reflectNames = ["apply", "construct", "defineProperty", "deleteProperty", "get", "getOwnPropertyDescriptor", "getPrototypeOf", "has",
  "isExtensible", "ownKeys", "preventExtensions", "set", "setPrototypeOf"];
const badTargets = ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n", "function () {}", "{}", "[]"];
for (const name of reflectNames) {
  for (const target of badTargets) T(`R = Q(function () { return F(Reflect.${name}(${target}, 'k', {}, {})) })`);
  T(`R = Q(function () { return F(Reflect.${name}()) })`);
  T(`R = Q(function () { return Reflect.${name}.length + ':' + Reflect.${name}.name + ':' + typeof Reflect.${name}.prototype })`);
  T(`R = Q(function () { return new Reflect.${name}({}) })`);
  T(`R = Q(function () { return F(Object.getOwnPropertyDescriptor(Reflect, '${name}')) })`);
}
for (const bad of ["1", "'s'", "undefined", "null", "{}", "Symbol()", "true"]) {
  T(`R = Q(function () { return Reflect.apply(${bad}, null, []) })`);
  T(`R = Q(function () { return Reflect.apply(function () {}, null, ${bad}) })`);
  T(`R = Q(function () { return F(Reflect.defineProperty({}, 'k', ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.setPrototypeOf({}, ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.getOwnPropertyDescriptor({ k: 1 }, ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.get({}, ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.has({}, ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.deleteProperty({}, ${bad})) })`);
  T(`R = Q(function () { return F(Reflect.set({}, ${bad}, 1)) })`);
}
T("R = Q(function () { return Reflect.get({}, { toString() { throw new EvalError('ts') } }) })");
T("R = Q(function () { return Reflect.has({}, { toString() { return 'k' } }) })");
T("R = Q(function () { return Reflect.get({ k: 7 }, { toString() { return 'k' } }) })");
T("R = Q(function () { return Reflect.get({ k: 7 }, { [Symbol.toPrimitive]() { return 'k' } }) })");
T("R = Q(function () { return Reflect.get({}, Symbol('q')) })");
T("R = Q(function () { return Reflect.ownKeys(Symbol()) })");
T("R = Q(function () { return Reflect.get({ get g() { return this } }, 'g', 5) })");
T("R = Q(function () { return typeof Reflect.get({ get g() { return this } }, 'g', 5) })");
T("R = Q(function () { return Reflect.get({ get g() { 'use strict'; return typeof this } }, 'g', 5) })");
T("R = Q(function () { return Reflect.get({ get g() { return typeof this } }, 'g', 5) })");
T("R = Q(function () { return Reflect.set({ set s(v) { this.got = v } }, 's', 1, 'str') })");
T("var r = {}; Reflect.set({ set s(v) { this.got = v } }, 's', 1, r); R = F(r)");
T("var r = {}; R = Reflect.set({}, 'k', 1, r) + F(r)");
T("var r = Object.freeze({}); R = Reflect.set({}, 'k', 1, r) + F(r)");
T("var r = Object.defineProperty({}, 'k', { value: 0, writable: false }); R = Reflect.set({}, 'k', 1, r)");
T("var r = Object.defineProperty({}, 'k', { value: 0, writable: true, configurable: false }); R = Reflect.set({}, 'k', 1, r) + F(r)");
T("var r = Object.defineProperty({}, 'k', { get() { return 0 }, configurable: true }); R = Reflect.set({}, 'k', 1, r)");
T("var r = { k: 0 }; R = Reflect.set({ k: 5 }, 'k', 1, r) + F(r)");
T("var r = { k: 0 }; R = Reflect.set(Object.freeze({ k: 5 }), 'k', 1, r)");
T("var r = { k: 0 }; R = Reflect.set({ get k() { return 1 } }, 'k', 1, r)");
T("R = Reflect.set({}, 'k', 1, 1) + ':' + Reflect.set({}, 'k', 1, null) + ':' + Reflect.set({}, 'k', 1, undefined)");
T("var a = [1, 2, 3]; R = Reflect.set(a, 'length', 1) + F(a)");
T("var a = [1, 2, 3]; Object.defineProperty(a, 1, { configurable: false }); R = Reflect.set(a, 'length', 0) + F(a)");
T("var a = Object.freeze([1]); R = Reflect.set(a, 0, 2) + Reflect.set(a, 1, 2) + Reflect.set(a, 'length', 0)");
T("var a = [1]; R = Reflect.defineProperty(a, 'length', { value: -1 })");
T("R = Q(function () { return Reflect.defineProperty([], 'length', { value: -1 }) })");
T("R = Q(function () { return Reflect.defineProperty([], 'length', { value: 1.5 }) })");
T("R = Q(function () { return Reflect.defineProperty({}, 'k', { get: 1 }) })");
T("R = Q(function () { return Reflect.defineProperty({}, 'k', { get() {}, value: 1 }) })");
T("R = Q(function () { return Reflect.defineProperty({}, 'k', { set: 1 }) })");
T("R = Q(function () { return Reflect.defineProperty({}, 'k', { get() {}, writable: true }) })");
T("R = Q(function () { return Reflect.defineProperty({}, 'k', { get: undefined, value: 1 }) })");
T("R = Q(function () { return Object.defineProperty({}, 'k', { get: 1 }) })");
T("R = Q(function () { return Object.defineProperty({}, 'k', { get() {}, value: 1 }) })");
T("R = Q(function () { return Object.defineProperty({}, 'k', 1) })");
T("R = Q(function () { return Object.defineProperty(1, 'k', {}) })");
T("R = Q(function () { return Object.defineProperties({}, { k: 1 }) })");
T("R = Q(function () { return Object.defineProperty(Object.freeze({}), 'k', { value: 1 }) })");
T("R = Q(function () { return Object.defineProperty(Object.freeze({ k: 1 }), 'k', { value: 2 }) })");
T("R = Q(function () { return Object.defineProperty(Object.freeze({ k: 1 }), 'k', { value: 1 }).k })");
T("R = Q(function () { return Object.defineProperty(Object.defineProperty({}, 'k', { value: 1 }), 'k', { get() {} }) })");
T("R = Q(function () { return Object.defineProperty(Object.defineProperty({}, 'k', { value: 1 }), 'k', { enumerable: true }) })");
T("R = Q(function () { return Object.defineProperty(Object.defineProperty({}, 'k', { value: 1 }), 'k', { configurable: true }) })");
T("R = Q(function () { return Object.defineProperty(Object.defineProperty({}, 'k', { value: 1 }), Symbol.iterator, {}) && Object.defineProperty(Object.defineProperty({}, Symbol('s'), { value: 1 }), 'z', {}).z })");
T("R = Q(function () { return Object.defineProperty(Object.defineProperty({}, Symbol('desc'), { value: 1 }), Object.getOwnPropertySymbols(Object.defineProperty({}, Symbol('desc'), { value: 1 }))[0], { value: 2 }) })");
T("R = Q(function () { 'use strict'; Reflect.getPrototypeOf; return Object.getPrototypeOf(1) === Number.prototype })");
T("R = Q(function () { return Reflect.getPrototypeOf(1) })");
T("R = Q(function () { return Object.setPrototypeOf(1, null) })");
T("R = Q(function () { return Object.setPrototypeOf(undefined, null) })");
T("R = Q(function () { return Object.setPrototypeOf({}, 1) })");
T("R = Q(function () { return Object.setPrototypeOf({}) })");
T("R = Q(function () { return Reflect.setPrototypeOf({}) })");
T("R = Q(function () { return Reflect.setPrototypeOf({}, undefined) })");
T("R = Q(function () { var a = {}, b = Object.create(a); return Reflect.setPrototypeOf(a, b) })");
T("R = Q(function () { var a = {}, b = Object.create(a); return Object.setPrototypeOf(a, b) })");
T("R = Q(function () { return Object.setPrototypeOf(Object.preventExtensions({}), {}) })");
T("R = Q(function () { return Reflect.setPrototypeOf(Object.preventExtensions({}), {}) })");
T("R = Q(function () { var o = Object.preventExtensions({}); return Reflect.setPrototypeOf(o, Object.prototype) })");
T("R = Q(function () { return Reflect.setPrototypeOf(Object.prototype, {}) })");
T("R = Q(function () { return Object.setPrototypeOf(Object.prototype, {}) })");
T("R = Q(function () { return Reflect.setPrototypeOf(Object.prototype, null) })");
T("R = Q(function () { return Object.prototype.__proto__ = {} })");
T("R = Q(function () { 'use strict'; return Object.prototype.__proto__ = {} })");
T("R = Q(function () { 'use strict'; var o = Object.preventExtensions({}); o.__proto__ = {}; return 1 })");
T("R = Q(function () { 'use strict'; var o = Object.preventExtensions({}); o.x = 1; return 1 })");
T("R = Q(function () { 'use strict'; var o = Object.freeze({ x: 1 }); o.x = 2; return 1 })");
T("R = Q(function () { 'use strict'; var o = Object.freeze({ x: 1 }); delete o.x; return 1 })");
T("R = Q(function () { 'use strict'; var o = { get x() { return 1 } }; o.x = 2; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { set() { return false } }); p.x = 1; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { set() { return false } }); p[Symbol('s')] = 1; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { set() { return false } }); p[0] = 1; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { deleteProperty() { return false } }); delete p.x; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { deleteProperty() { return false } }); delete p[Symbol('s')]; return 1 })");
T("R = Q(function () { var p = new Proxy({}, { set() { return false } }); p.x = 1; return 'sloppy ok' })");
T("R = Q(function () { var p = new Proxy({}, { deleteProperty() { return false } }); return delete p.x })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { defineProperty() { return false } }); return Object.defineProperty(p, 'x', {}) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { defineProperty() { return false } }); return Reflect.defineProperty(p, 'x', {}) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { defineProperty() { return false } }); return Object.defineProperty(p, Symbol('s'), {}) })");
T("R = Q(function () { var p = new Proxy({}, { defineProperty() { return false } }); p.x = 1; return 'ok' })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { defineProperty() { return false } }); p.x = 1; return 'ok' })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { defineProperty() { return false } }); return [1].concat(Object.defineProperties(p, { a: { value: 1 } })) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { preventExtensions() { return false } }); return Object.preventExtensions(p) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { preventExtensions() { return false } }); return Object.freeze(p) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { preventExtensions() { return false } }); return Object.seal(p) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { setPrototypeOf() { return false } }); return Object.setPrototypeOf(p, {}) })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { setPrototypeOf() { return false } }); p.__proto__ = {}; return 1 })");
T("R = Q(function () { 'use strict'; var p = new Proxy({}, { setPrototypeOf() { return false } }); return Object.getPrototypeOf(Object.create(null)) === null && Reflect.setPrototypeOf(p, {}) })");
T("R = Q(function () { var o = Object.create(new Proxy({}, { set() { return false } })); o.x = 1; return 'sloppy ok:' + F(Object.keys(o)) })");
T("R = Q(function () { 'use strict'; var o = Object.create(new Proxy({}, { set() { return false } })); o.x = 1; return 1 })");
T("R = Q(function () { 'use strict'; var o = Object.create(new Proxy({}, { set() { return true } })); o.x = 1; return F(Object.keys(o)) })");
T("R = Q(function () { 'use strict'; var o = Object.create(new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r) } })); o.x = 1; return F(Object.keys(o)) })");
T("R = Q(function () { 'use strict'; var o = Object.freeze(Object.create(new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r) } }))); o.x = 1; return F(Object.keys(o)) })");

// ---- 9. Reflect geral e interações adicionais.
T("R = F(Object.getOwnPropertyNames(Reflect).sort()) + ':' + Reflect[Symbol.toStringTag] + ':' + Object.prototype.toString.call(Reflect) + ':' + typeof Reflect");
T("R = F(Object.getOwnPropertyDescriptor(Reflect, Symbol.toStringTag))");
T("R = Q(function () { return Reflect() })");
T("R = Q(function () { return new Reflect() })");
T("R = Q(function () { return Object.getPrototypeOf(Reflect) === Object.prototype })");
T("R = F([Reflect.apply(Math.max, null, [1, 3, 2]), Reflect.apply(String.prototype.slice, 'abc', [1]), Reflect.apply(function () { return this }, 5, [])])");
T("R = Q(function () { return typeof Reflect.apply(function () { return this }, 5, []) })");
T("R = Q(function () { return typeof Reflect.apply(function () { 'use strict'; return this }, 5, []) })");
T("R = Q(function () { return Reflect.apply(function () { return arguments.length }, null, { length: 4 }) })");
T("R = Q(function () { return Reflect.apply(function () { return arguments.length }, null, 'abc') })");
T("R = Q(function () { return Reflect.apply(function () { return arguments.length }, null, new Proxy([1, 2], {})) })");
T("R = Q(function () { return Reflect.apply(class {}, null, []) })");
T("R = Q(function () { return Reflect.apply(Symbol, null, ['d']).toString() })");
T("R = Q(function () { return Reflect.apply(Date, null, []).length > 0 })");
T("R = Q(function () { return Reflect.apply(Number, null, ['5']) })");
T("R = Q(function () { return Reflect.apply(BigInt, null, [5]) })");
T("R = Q(function () { return Reflect.apply(Array, null, [3]).length })");
T("R = Q(function () { return Reflect.apply(Map, null, []) })");
T("R = Q(function () { return Reflect.apply(Promise, null, []) })");
T("R = Q(function () { return Reflect.apply(Proxy, null, [{}, {}]) })");
T("R = Q(function () { return Reflect.apply(Function.prototype.call, function () { return this }, [7]) })");
T("R = Q(function () { return Reflect.apply(Reflect.apply, null, [Math.max, null, [1, 2]]) })");
T("R = Q(function () { return Reflect.apply(Function.prototype.apply, Math.max, [null, [4, 5]]) })");
T("R = Q(function () { return Reflect.apply(Function.prototype.bind, Math.max, [null, 9])() })");
T("R = Q(function () { return Reflect.apply(new Proxy(Math.max, {}), null, [1, 2]) })");
T("R = Q(function () { return F(Reflect.apply(new Proxy(function () { return this }, {}), 'x', [])) })");
T("R = Q(function () { return Reflect.get(new Proxy({}, { get: function (t, k, r) { return r } }), 'k', 'recv') })");
T("R = Q(function () { return Reflect.get(new Proxy({}, { get: function (t, k, r) { return r } }), 'k') === undefined })");
T("var o = { x: 1 }; R = Q(function () { return Reflect.get(new Proxy({}, { get: function (t, k, r) { return r } }), 'k') === undefined })");
T("var p = new Proxy({ get v() { return this } }, {}); R = Q(function () { return Reflect.get(p, 'v') === p }) + Q(function () { return p.v === p }) + Q(function () { return Reflect.get(p, 'v', 1) })");
T("var tgt = { get v() { return this } }; var p = new Proxy(tgt, { get(t, k, r) { return Reflect.get(t, k) } }); R = Q(function () { return p.v === tgt })");
T("var tgt = { get v() { return this } }; var p = new Proxy(tgt, { get(t, k, r) { return t[k] } }); R = Q(function () { return p.v === tgt })");
T("var tgt = { set v(x) { this.got = x } }; var p = new Proxy(tgt, {}); p.v = 1; R = F(Object.keys(p)) + F(Object.keys(tgt)) + F(tgt.got)");
T("var tgt = {}; var p = new Proxy(tgt, {}); p.v = 1; R = F(Object.keys(tgt)) + Reflect.set(p, 'w', 2, tgt) + F(Object.keys(tgt))");
T("var tgt = {}; var p = new Proxy(tgt, { defineProperty(t, k, d) { L.push(Object.keys(d).join('+')); return Reflect.defineProperty(t, k, d) }, getOwnPropertyDescriptor(t, k) { L.push('gopd'); return Reflect.getOwnPropertyDescriptor(t, k) } }); p.v = 1; p.v = 2; R = L.join()");
T("var tgt = { v: 1 }; var p = new Proxy(tgt, { defineProperty(t, k, d) { L.push(Object.keys(d).join('+')); return Reflect.defineProperty(t, k, d) }, getOwnPropertyDescriptor(t, k) { L.push('gopd'); return Reflect.getOwnPropertyDescriptor(t, k) } }); p.v = 2; R = L.join()");
T("var tgt = {}; var p = new Proxy(tgt, { defineProperty(t, k, d) { L.push(Object.keys(d).join('+')); return Reflect.defineProperty(t, k, d) } }); Object.defineProperty(p, 'v', { value: 1, writable: true }); Object.defineProperty(p, 'w', { get() {} }); R = L.join()");
T("var tgt = {}; var p = new Proxy(tgt, { defineProperty(t, k, d) { L.push(Object.keys(d).join('+') + '|' + F(d)); return Reflect.defineProperty(t, k, d) } }); Object.defineProperty(p, 'v', Object.create({ value: 1 }, { enumerable: { value: true } })); R = L.join()");
T("var tgt = {}; var p = new Proxy(tgt, { defineProperty(t, k, d) { L.push(F(d)); return Reflect.defineProperty(t, k, d) } }); Object.defineProperties(p, { a: { value: 1 }, b: { value: 2, enumerable: true } }); R = L.join()");
T("var tgt = {}; var p = new Proxy(tgt, { getOwnPropertyDescriptor(t, k) { L.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, ownKeys(t) { L.push('ownKeys'); return Reflect.ownKeys(t) } }); Object.defineProperties(tgt, { a: { value: 1, enumerable: true } }); Object.getOwnPropertyDescriptors(p); R = L.join()");
T("var p = new Proxy({ a: 1 }, { getOwnPropertyDescriptor(t, k) { return { value: 'v', enumerable: true, configurable: true } } }); R = F(Object.getOwnPropertyDescriptor(p, 'a')) + F(Object.getOwnPropertyDescriptor(p, 'zz'))");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { value: 'v', enumerable: true, configurable: true } } }); R = F(Object.getOwnPropertyDescriptor(p, 'q')) + p.hasOwnProperty('q') + p.propertyIsEnumerable('q') + Object.hasOwn(p, 'q')");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { get: undefined, configurable: true } } }); R = F(Object.getOwnPropertyDescriptor(p, 'q'))");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { get() {}, value: 1, configurable: true } } }); R = Q(function () { return Object.getOwnPropertyDescriptor(p, 'q') })");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { get: 1, configurable: true } } }); R = Q(function () { return Object.getOwnPropertyDescriptor(p, 'q') })");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return Object.create({ value: 4, configurable: true }) } }); R = Q(function () { return F(Object.getOwnPropertyDescriptor(p, 'q')) })");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { value: 4, configurable: 'yes', enumerable: 1, writable: 0 } } }); R = Q(function () { return F(Object.getOwnPropertyDescriptor(p, 'q')) })");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return function () {} } }); R = Q(function () { return F(Object.getOwnPropertyDescriptor(p, 'q')) })");
T("var p = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return null } }); R = Q(function () { return F(Object.getOwnPropertyDescriptor(p, 'q')) })");
T("var p = new Proxy({}, { ownKeys() { return { length: 2, 0: 'a', 1: 'b' } } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return new Set(['a']) } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return 'ab' } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return [Symbol.for('x'), 'a'] } }); R = Q(function () { return F(Reflect.ownKeys(p).map(String)) + F(Object.keys(p)) + F(Object.getOwnPropertySymbols(p).length) })");
T("var p = new Proxy({}, { ownKeys() { return ['a', {}] } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return [{ toString() { return 'a' } }] } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return [1] } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return [null] } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return new Proxy(['a', 'b'], {}) } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { var a = ['a']; a.length = 3; return a } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return Array(2) } }); R = Q(function () { return F(Reflect.ownKeys(p)) })");
T("var p = new Proxy({}, { ownKeys() { return ['a', 'b'] } }); R = Q(function () { return F(Object.keys(p)) }) + Q(function () { return F(Object.getOwnPropertyNames(p)) })");
T("var p = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a'] } }); R = Q(function () { return F(Object.keys(p)) + F(Object.entries(p)) + JSON.stringify(p) + F(Object.assign({}, p)) })");
T("var p = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a'] } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a', 'c'] } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a', 'c'] }, getOwnPropertyDescriptor(t, k) { return { value: 1, enumerable: true, configurable: true } } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a', 'c'] }, getOwnPropertyDescriptor(t, k) { return { value: 1, enumerable: true, configurable: true } } }); R = Q(function () { return F(Object.keys(p)) + JSON.stringify(p) })");
T("var p = new Proxy({ a: 1 }, { ownKeys() { return ['a', 'a'] } }); R = Q(function () { return F(Object.keys(p)) })");
T("var p = new Proxy({ a: 1 }, { ownKeys() { return ['a', 'a'] } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1 }, { ownKeys() { return ['a', 'a'] } }); R = Q(function () { return F(Object.getOwnPropertyNames(p)) })");
T("var p = new Proxy({ a: 1 }, { getPrototypeOf() { return { inh: 1 } }, ownKeys() { return ['a'] } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1 }, { getPrototypeOf() { return new Proxy({ inh: 1 }, { ownKeys() { return ['inh', 'a'] } }) } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1 }, { getPrototypeOf() { return null } }); R = Q(function () { var r = []; for (var k in p) r.push(k); return r.join() })");
T("var p = new Proxy({ a: 1, b: 2 }, { get(t, k) { L.push('get:' + String(k)); return t[k] } }); var r = []; for (var k in p) { r.push(k); delete p.b } R = r.join() + ':' + L.join()");
T("var tgt = { a: 1, b: 2, c: 3 }; var p = new Proxy(tgt, {}); var r = []; for (var k in p) { r.push(k); delete tgt.b } R = r.join()");
T("var tgt = { a: 1, b: 2 }; var p = new Proxy(tgt, {}); var r = []; for (var k in p) { r.push(k); tgt.z = 1 } R = r.join()");
T("var tgt = { a: 1, b: 2 }; var p = new Proxy(tgt, {}); var r = []; for (var k in p) { r.push(k); Object.defineProperty(tgt, 'b', { enumerable: false }) } R = r.join()");
T("var p = new Proxy([1, 2, 3], {}); var r = []; for (var i of p) r.push(i); R = r.join() + ':' + Array.isArray(p) + ':' + F([...p]) + ':' + F(Array.from(p))");
T("var p = new Proxy([1, 2, 3], LH()); var r = [...p]; R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); var [a, b] = p; R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); Math.max(...p); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.map(function (x) { return x }); R = L.join()");
T("var p = new Proxy([3, 1, 2], LH()); p.sort(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.push(4); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.length = 1; R = L.join() + F(Reflect.ownKeys(p))");
T("var p = new Proxy([1, 2, 3], LH()); JSON.stringify(p); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); Array.isArray(p); R = L.join() + 'x'");
T("var p = new Proxy([1, 2, 3], LH()); [].concat(p); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); Array.prototype.includes.call(p, 5); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.indexOf(3); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.reverse(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.shift(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.unshift(0); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.splice(1, 1); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.slice(1); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.join('-'); R = L.join()");
T("var p = new Proxy([1, [2], 3], LH()); p.flat(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.at(-1); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.fill(0); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.copyWithin(0, 1); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.toSorted(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.with(0, 9); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.forEach(function () {}); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); Object.keys(p); Object.values(p); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.pop(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.lastIndexOf(1); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.reduce(function (a, b) { return a + b }); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.findLast(function (x) { return x == 1 }); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p.keys().next(); p.entries().next(); R = L.join()");
T("var p = new Proxy([1, 2, 3], LH()); p[Symbol.iterator]().next(); R = L.join()");
T("var C = function () {}; C[Symbol.species] = Array; var a = [1]; a.constructor = new Proxy(Array, LH()); a.map(function (x) { return x }); R = L.join()");
T("var a = new Proxy([1, 2], {}); R = F(Array.prototype.map.call(a, function (x) { return x * 2 })) + (Array.prototype.map.call(a, function (x) { return x }) instanceof Array)");
T("var a = new Proxy([1, 2], {}); R = Array.isArray(Array.prototype.concat.call(a)) + ':' + F([].concat(a, a)) + ':' + F(Array.prototype.concat.call(a, 3))");
T("var a = new Proxy([1, 2], {}); R = F(a.slice()) + (a.slice() instanceof Array) + F(a.filter(Boolean)) + F(a.flatMap(function (x) { return [x, x] }))");
T("var a = new Proxy({ length: 2, 0: 'a', 1: 'b' }, {}); R = F(Array.from(a)) + F(Array.prototype.slice.call(a)) + Array.prototype.join.call(a)");
T("var a = new Proxy([[1, 2], [3]], {}); R = F(a.flat()) + F(Array.prototype.concat.apply([], a))");
T("var o = { [Symbol.isConcatSpreadable]: true, length: 2, 0: 'x', 1: 'y' }; var p = new Proxy(o, {}); R = F([].concat(p))");
T("var p = new Proxy([1, 2], { get(t, k, r) { return k === Symbol.isConcatSpreadable ? false : Reflect.get(t, k, r) } }); R = F([].concat(p).length)");
T("var p = new Proxy({}, { get(t, k, r) { return k === Symbol.isConcatSpreadable ? true : k === 'length' ? 2 : k } }); R = F([].concat(p))");
T("var p = new Proxy({}, { get(t, k, r) { return k === Symbol.toPrimitive ? function (h) { return 'prim:' + h } : undefined } }); R = `${p}` + (p + '') + (p * 1) + String(p) + [p].join() + F(Object.keys({ [p]: 1 }))");
T("var p = new Proxy({}, { get(t, k, r) { L.push(String(k)); return undefined } }); String(p); R = L.join()");
T("var p = new Proxy({}, { get(t, k, r) { L.push(String(k)); return undefined } }); +p; R = L.join()");
T("var p = new Proxy({}, { get(t, k, r) { L.push(String(k)); return k === 'valueOf' ? function () { return 3 } : undefined } }); R = (p * 2) + ':' + L.join()");
T("var p = new Proxy({}, { get(t, k, r) { L.push(String(k)); return k === 'toString' ? function () { return 'ts' } : undefined } }); R = (p + '') + ':' + L.join()");
T("var p = new Proxy({}, { get(t, k, r) { L.push(String(k)); return undefined } }); p instanceof Object; R = L.join()");
T("var p = new Proxy(function () {}, { get(t, k, r) { L.push(String(k)); return Reflect.get(t, k, r) } }); ({}) instanceof p; R = L.join()");
T("var p = new Proxy(function () {}, { get(t, k, r) { L.push(String(k)); return k === Symbol.hasInstance ? function (v) { return 'hi' } : Reflect.get(t, k, r) } }); R = (({}) instanceof p) + ':' + L.join()");
T("var p = new Proxy(class {}, {}); R = (new p() instanceof p) + ':' + ({} instanceof p)");
T("var p = new Proxy({}, {}); R = Q(function () { return ({}) instanceof p })");
T("var p = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? 5 : undefined } }); R = Q(function () { return ({}) instanceof p })");
T("var p = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : undefined } }); R = Q(function () { return [] instanceof p })");
T("var p = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : undefined } }); R = Q(function () { return 1 instanceof p })");
T("var f = function () {}.bind(); var p = new Proxy(f, {}); R = Q(function () { return ({}) instanceof p })");
T("var p = new Proxy({}, { getPrototypeOf() { return Array.prototype } }); R = [Array.isArray(p), p instanceof Array, Object.prototype.toString.call(p), Array.prototype.isPrototypeOf(p)].join()");
T("var p = new Proxy([], { getPrototypeOf() { return Object.prototype } }); R = [Array.isArray(p), p instanceof Array, Object.prototype.toString.call(p), Array.prototype.isPrototypeOf(p)].join()");
T("var p = new Proxy(new Proxy([], {}), {}); R = [Array.isArray(p), JSON.stringify(p), Object.prototype.toString.call(p)].join()");
T("var p = new Proxy(new Proxy({}, {}), {}); R = [Array.isArray(p), JSON.stringify(p)].join()");
T("var rv = Proxy.revocable([], {}); rv.revoke(); R = Q(function () { return Array.isArray(new Proxy(rv.proxy, {})) })");
T("var rv = Proxy.revocable({}, {}); rv.revoke(); R = Q(function () { return Array.isArray(rv.proxy) })");
T("var p = new Proxy([], { get(t, k) { L.push(String(k)); return Reflect.get(t, k) } }); R = Array.isArray(p) + ':' + L.length");
T("var p = new Proxy({ toJSON() { return 'tj' } }, {}); R = JSON.stringify(p) + JSON.stringify({ p: p })");
T("var p = new Proxy({ a: 1, b: { c: 2 } }, LH()); JSON.stringify(p); R = L.join()");
T("var p = new Proxy({ a: 1, b: { c: 2 } }, LH()); JSON.stringify(p, function (k, v) { return v }); R = L.join()");
T("var p = new Proxy({ a: 1, b: 2 }, LH()); JSON.stringify(p, ['b']); R = L.join()");
T("var p = new Proxy({ a: 1, b: 2 }, LH()); JSON.stringify({ x: p }, null, 2); R = L.join()");
T("var p = new Proxy([1, 2], LH()); JSON.stringify({ x: p }, null, 2); R = L.join()");
T("var p = new Proxy({ a: 1 }, { get(t, k) { return k === 'toJSON' ? function (key) { return 'key=' + key } : t[k] } }); R = JSON.stringify({ q: p }) + JSON.stringify([p]) + JSON.stringify(p)");
T("var p = new Proxy({ a: undefined, b: function () {}, c: Symbol('s'), d: 1 }, {}); R = JSON.stringify(p)");
T("var p = new Proxy({}, {}); p.self = p; R = Q(function () { return JSON.stringify(p) })");
T("var p = new Proxy({ n: 1n }, {}); R = Q(function () { return JSON.stringify(p) })");
T("var p = new Proxy({ a: 1 }, { ownKeys() { return ['a', 'b'] }, get(t, k) { return k === 'b' ? 2 : t[k] }, getOwnPropertyDescriptor(t, k) { return { value: 1, enumerable: true, configurable: true } } }); R = JSON.stringify(p)");
T("var p = new Proxy({ a: 1 }, { ownKeys() { return ['a', Symbol.for('s')] } }); R = JSON.stringify(p) + F(Object.keys(p))");
T("var p = new Proxy({}, { get(t, k) { return k === 'length' ? 2 : k === '0' ? 'a' : k === '1' ? 'b' : undefined } }); R = Array.prototype.join.call(p) + Array.from(p).join('')");
T("var p = new Proxy({ a: 1, b: 2 }, { get(t, k, r) { return k === 'a' ? 100 : Reflect.get(t, k, r) } }); R = F({ ...p }) + F(Object.assign({}, p)) + F(Object.entries(p)) + JSON.stringify(p)");
T("var o = Object.assign({}, new Proxy({ a: 1 }, { get(t, k) { L.push('get:' + String(k)); return t[k] }, ownKeys(t) { L.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { L.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k) } })); R = L.join()");
T("var tgt = {}; var p = new Proxy({ a: 1 }, { ownKeys(t) { return ['a', 'a'] } }); R = Q(function () { return F(Object.assign(tgt, p)) })");
T("var tgt = new Proxy({}, LH()); Object.assign(tgt, { a: 1, b: 2 }); R = L.join()");
T("var tgt = new Proxy({}, LH()); ({ ...{ a: 1 } }); var o = { ...tgt, z: 1 }; R = L.join()");
T("var tgt = new Proxy({}, LH()); Object.defineProperty(tgt, 'a', { value: 1 }); Object.setPrototypeOf(tgt, null); Object.isExtensible(tgt); Object.preventExtensions(tgt); R = L.join()");
T("var tgt = new Proxy({}, LH()); tgt.a = 1; tgt.a; delete tgt.a; 'a' in tgt; R = L.join()");
T("var tgt = new Proxy({}, LH()); tgt.a = 1; tgt.a += 1; tgt.a++; R = L.join()");
T("var tgt = new Proxy({}, LH()); tgt.a ??= 1; tgt.a ||= 2; tgt.a &&= 3; R = L.join()");
T("var tgt = new Proxy({}, LH()); tgt.a?.b; tgt?.a; delete tgt?.a; R = L.join()");
T("var tgt = new Proxy({ f() { return 1 } }, LH()); tgt.f(); R = L.join()");
T("var tgt = new Proxy({ f() { return this === tgt } }, LH()); R = tgt.f() + ':' + L.join()");
T("var tgt = new Proxy({}, LH()); var { a, b = 2 } = tgt; R = L.join()");
T("var tgt = new Proxy({}, LH()); var { ...rest } = tgt; R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); var { ...rest } = tgt; R = L.join()");
T("var tgt = new Proxy({ a: 1, b: 2 }, LH()); var { a, ...rest } = tgt; R = L.join()");
T("var tgt = new Proxy({}, LH()); tgt['k' + 1] = 1; tgt[Symbol.iterator]; tgt[1]; R = L.join()");
T("var tgt = new Proxy({}, LH()); Object.freeze(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.freeze(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.seal(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.isFrozen(tgt); Object.isSealed(tgt); R = L.join()");
T("var tgt = new Proxy(Object.freeze({ a: 1 }), LH()); Object.isFrozen(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.getOwnPropertyDescriptors(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.fromEntries(Object.entries(tgt)); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); structuredCloneLike = 0; Object.getOwnPropertyNames(tgt); Object.getOwnPropertySymbols(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); tgt.hasOwnProperty('a'); tgt.propertyIsEnumerable('a'); Object.hasOwn(tgt, 'a'); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); tgt.toString(); tgt.valueOf(); tgt.toLocaleString(); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.prototype.toString.call(tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.prototype.isPrototypeOf.call(Object.prototype, tgt); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.prototype.__lookupGetter__.call(tgt, 'a'); Object.prototype.__defineGetter__.call(tgt, 'b', function () {}); R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); tgt.__proto__; tgt.__proto__ = null; R = L.join()");
T("var tgt = new Proxy({ a: 1 }, LH()); Object.entries(tgt).map(String); Object.values(tgt); R = L.join()");
T("var tgt = new Proxy(new Map(), LH()); R = Q(function () { return tgt.size }) + ':' + L.join()");
T("var tgt = new Proxy(new Map(), LH()); R = Q(function () { return Map.prototype.get.call(tgt, 1) })");
T("var tgt = new Proxy(new Set(), LH()); R = Q(function () { return tgt.add(1) })");
T("var tgt = new Proxy(new Date(0), LH()); R = Q(function () { return tgt.getTime() })");
T("var tgt = new Proxy(new Date(0), LH()); R = Q(function () { return tgt.toJSON() }) + L.join()");
T("var tgt = new Proxy(new Date(0), LH()); R = Q(function () { return JSON.stringify(tgt) }) + L.join()");
T("var tgt = new Proxy(/a/g, LH()); R = Q(function () { return tgt.test('a') })");
T("var tgt = new Proxy(/a/g, LH()); R = Q(function () { return RegExp.prototype.exec.call(tgt, 'a') })");
T("var tgt = new Proxy(/a/g, LH()); R = Q(function () { return 'aa'.replace(tgt, 'b') }) + ':' + L.join()");
T("var tgt = new Proxy(/a/g, LH()); R = Q(function () { return 'aa'.match(tgt).length }) ");
T("var tgt = new Proxy(new Error('m'), LH()); R = Q(function () { return Error.prototype.toString.call(tgt) }) + ':' + L.join()");
T("var tgt = new Proxy(new Error('m'), LH()); R = Q(function () { return tgt.stack === undefined || typeof tgt.stack }) ");
T("var tgt = new Proxy(Promise.resolve(1), LH()); R = Q(function () { return tgt.then(function () {}) })");
T("var tgt = new Proxy(new Uint8Array(2), LH()); R = Q(function () { return tgt.length }) + Q(function () { return tgt[0] })");
T("var tgt = new Proxy(new Uint8Array(2), LH()); R = Q(function () { return Uint8Array.prototype.fill.call(tgt, 1) })");
T("var tgt = new Proxy(new Uint8Array(2), LH()); R = Q(function () { return Object.keys(tgt) }) + Q(function () { return JSON.stringify(tgt) })");
T("var tgt = new Proxy(new String('ab'), LH()); R = Q(function () { return tgt.length }) + Q(function () { return tgt.toString() }) + Q(function () { return String.prototype.valueOf.call(tgt) })");
T("var tgt = new Proxy(new Number(5), LH()); R = Q(function () { return tgt + 1 }) + Q(function () { return Number.prototype.valueOf.call(tgt) })");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return tgt.name + tgt.length }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return tgt.bind(null).name }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return tgt.call(null) }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return tgt.apply(null, [1]) }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return Reflect.apply(tgt, null, []) }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return new tgt() instanceof tgt }) + L.join()");
T("var tgt = new Proxy(function () {}, LH()); R = Q(function () { return typeof new tgt() }) + L.join()");
T("var tgt = new Proxy(class { constructor() { this.a = 1 } }, LH()); R = Q(function () { return F(new tgt()) }) + L.join()");
T("var tgt = new Proxy(class { static s = 1 }, LH()); R = Q(function () { return tgt.s }) + L.join()");
T("var tgt = new Proxy(class { static s = 1 }, LH()); R = Q(function () { return tgt() }) + L.join()");
T("var tgt = new Proxy(class {}, LH()); R = Q(function () { return Reflect.construct(tgt, [], Object) }) + L.join()");
T("var tgt = new Proxy(async function () {}, LH()); R = Q(function () { return tgt() instanceof Promise }) + L.join()");
T("var tgt = new Proxy(function* () {}, LH()); R = Q(function () { return tgt().next().done }) + L.join()");
T("var tgt = new Proxy(() => 1, LH()); R = Q(function () { return new tgt() }) + L.join()");
T("var tgt = new Proxy(() => 1, LH()); R = Q(function () { return tgt() }) + L.join()");
T("var tgt = new Proxy({}, LH()); R = Q(function () { return tgt() }) + L.join()");
T("var tgt = new Proxy({}, LH()); R = Q(function () { return new tgt() }) + L.join()");
T("var tgt = new Proxy({}, LH()); R = Q(function () { return typeof tgt }) + L.join()");
T("var tgt = new Proxy(Math.max, LH()); R = Q(function () { return tgt(1, 5) }) + L.join()");
T("var tgt = new Proxy(Math.max, LH()); R = Q(function () { return new tgt() }) + L.join()");
T("var tgt = new Proxy(Math.max, LH()); R = typeof tgt + ':' + Q(function () { return tgt.name })");
T("var tgt = new Proxy(Symbol, LH()); R = Q(function () { return tgt('d').toString() }) + Q(function () { return new tgt() })");
T("var tgt = new Proxy(Object, LH()); R = Q(function () { return F(new tgt({ a: 1 })) + F(tgt.keys({ b: 1 })) })");
T("var tgt = new Proxy(Array, LH()); R = Q(function () { return F(new tgt(2).length) + F(tgt.of(1, 2)) + Array.isArray(new tgt()) })");
T("var tgt = new Proxy(Array, {}); class Sub extends tgt {} R = Q(function () { var s = new Sub(); s.push(1); return s.length + ':' + Array.isArray(s) + ':' + (s.map(function (x) { return x }) instanceof Sub) })");
T("var tgt = new Proxy(Error, {}); class E2 extends tgt {} R = Q(function () { var e = new E2('m'); return e.message + ':' + (e instanceof Error) + ':' + e.name })");
T("var tgt = new Proxy(Promise, {}); class P2 extends tgt {} R = Q(function () { return (P2.resolve(1) instanceof P2) + ':' + (new P2(function () {}) instanceof Promise) })");
T("var tgt = new Proxy(Map, {}); class M2 extends tgt {} R = Q(function () { var m = new M2([[1, 2]]); return m.get(1) + ':' + m.size })");
T("var tgt = new Proxy(Function, {}); R = Q(function () { return new tgt('return 3')() + tgt('return 4')() })");
T("var tgt = new Proxy(Date, {}); R = Q(function () { return new tgt(0).getTime() + ':' + typeof tgt() })");
T("var tgt = new Proxy(RegExp, {}); R = Q(function () { return new tgt('a', 'g').flags + ':' + tgt('b').source })");
T("var tgt = new Proxy(Boolean, {}); R = Q(function () { return new tgt(0) instanceof Boolean }) + Q(function () { return tgt(1) })");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "proxy-reflect-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (o bun passa arquivos pelo transpilador próprio).
const source_file = path.join(dir, "proxy_source.js");
const file = path.join(dir, "proxy_case.js");
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
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("proxy_reflect", rows));
fs.rmSync(dir, { recursive: true, force: true });
