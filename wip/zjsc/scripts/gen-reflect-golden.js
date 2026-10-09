// Gera tests/golden/reflect_bun.tsv: invariantes de cada trap do Proxy (alvo não configurável, não gravável, não
// extensível), Proxy.revocable e uso após revoke, Proxy como protótipo, Proxy em for-in/keys/JSON/spread/assign,
// ordem de traps e Reflect.* (receiver, argumentos inválidos), medidos no bun 1.4.2. Complementa
// proxy_class_bun.tsv: programa que já está lá (mesma fonte) é pulado.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa grava em `R` o
// resultado formatado ou "NomeDoErro: mensagem". Resultado com caminho da máquina é descartado.
// Uso: bun scripts/gen-reflect-golden.js > tests/golden/reflect_bun.tsv
const fs = require("fs");
const { emitFactoredLines, knownPrograms, prepareProgram } = require("./golden-prelude.js");
const lines = [];
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const prelude = `function fmt(v) {
  if (typeof v === "symbol") return v.toString();
  if (typeof v === "function") return "[function]";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "bigint") return v + "n";
  if (v === undefined) return "undefined";
  if (typeof v === "object" && v !== null) { try { return JSON.stringify(v, (k, x) => typeof x === "symbol" ? x.toString() : x === undefined ? "<u>" : x); } catch (e) { return "[object]"; } }
  return String(v);
}
function run(f) { try { return fmt(f()); } catch (e) { return e.name + ": " + e.message; } }
`;

const bodies = [];
const add = (body) => bodies.push(body);
const t = (code) => add(`globalThis.R = run(function () { ${code} });`);

// Estados do alvo, cada um com uma propriedade `a` (ou nenhuma).
const targets = {
  ncnw: "const t = {}; Object.defineProperty(t, 'a', { value: 1 });",
  ncw: "const t = {}; Object.defineProperty(t, 'a', { value: 1, writable: true });",
  nwc: "const t = {}; Object.defineProperty(t, 'a', { value: 1, configurable: true });",
  accNoGet: "const t = {}; Object.defineProperty(t, 'a', { set(v) {} });",
  accNoSet: "const t = {}; Object.defineProperty(t, 'a', { get() { return 1; } });",
  accBoth: "const t = {}; Object.defineProperty(t, 'a', { get() { return 1; }, set(v) {} });",
  plain: "const t = { a: 1 };",
  empty: "const t = {};",
  frozen: "const t = Object.freeze({ a: 1 });",
  sealed: "const t = Object.seal({ a: 1 });",
  nonext: "const t = Object.preventExtensions({ a: 1 });",
  nonextEmpty: "const t = Object.preventExtensions({});",
  ncSym: "const t = {}; Object.defineProperty(t, Symbol.iterator, { value: 1 }); const k = Symbol.iterator;",
};
const returns = ["undefined", "1", "2", "'x'", "null", "NaN", "0", "true", "false", "{}", "[]", "'a'", "-0", "Object(1)"];

// ---- get
for (const [n, setup] of Object.entries(targets)) {
  for (const r of returns) {
    t(`${setup} const p = new Proxy(t, { get() { return ${r}; } }); return p.a;`);
    t(`${setup} const p = new Proxy(t, { get() { return ${r}; } }); return Reflect.get(p, 'a');`);
  }
}
// ---- set
for (const [n, setup] of Object.entries(targets)) {
  for (const r of returns) {
    t(`${setup} const p = new Proxy(t, { set() { return ${r}; } }); p.a = 5; return 'ok';`);
    t(`'use strict'; ${setup} const p = new Proxy(t, { set() { return ${r}; } }); p.a = 5; return 'ok';`);
    t(`${setup} const p = new Proxy(t, { set() { return ${r}; } }); return Reflect.set(p, 'a', 1);`);
  }
  for (const v of ["1", "2"]) t(`'use strict'; ${setup} const p = new Proxy(t, { set() { return true; } }); p.a = ${v}; return 'ok';`);
}
// ---- has
for (const [n, setup] of Object.entries(targets)) {
  for (const r of returns) {
    t(`${setup} const p = new Proxy(t, { has() { return ${r}; } }); return 'a' in p;`);
    t(`${setup} const p = new Proxy(t, { has() { return ${r}; } }); return Reflect.has(p, 'a');`);
    t(`${setup} const p = new Proxy(t, { has() { return ${r}; } }); with (p) { return typeof a; }`.replace("'use strict'; ", ""));
  }
}
// ---- deleteProperty
for (const [n, setup] of Object.entries(targets)) {
  for (const r of returns) {
    t(`${setup} const p = new Proxy(t, { deleteProperty() { return ${r}; } }); return delete p.a;`);
    t(`'use strict'; ${setup} const p = new Proxy(t, { deleteProperty() { return ${r}; } }); return delete p.a;`);
    t(`${setup} const p = new Proxy(t, { deleteProperty() { return ${r}; } }); return Reflect.deleteProperty(p, 'a');`);
  }
}
// ---- defineProperty
const descs = ["{ value: 1 }", "{ value: 2 }", "{ value: 1, writable: true }", "{ value: 1, configurable: true }", "{ value: 1, configurable: false }", "{ get() {} }", "{ writable: false }", "{ configurable: false, writable: false, value: 1 }"];
for (const [n, setup] of Object.entries(targets)) {
  for (const r of ["true", "false", "undefined", "1"]) {
    for (const d of descs) {
      t(`${setup} const p = new Proxy(t, { defineProperty() { return ${r}; } }); return Reflect.defineProperty(p, 'a', ${d});`);
    }
    t(`${setup} const p = new Proxy(t, { defineProperty() { return ${r}; } }); Object.defineProperty(p, 'a', { value: 1 }); return 'ok';`);
  }
}
// ---- getOwnPropertyDescriptor
const gopdReturns = ["undefined", "{ value: 1 }", "{ value: 1, configurable: true }", "{ value: 1, configurable: false }", "{ value: 1, writable: true, configurable: false }", "{ value: 2, configurable: false, writable: false }", "{ get() {}, configurable: true }", "{ get() {}, configurable: false }", "{ value: 1, configurable: true, writable: true }", "1", "null", "'x'", "{ get() {}, value: 1 }", "{ configurable: true }", "{ configurable: false, writable: false, value: 1 }"];
for (const [n, setup] of Object.entries(targets)) {
  for (const r of gopdReturns) {
    t(`${setup} const p = new Proxy(t, { getOwnPropertyDescriptor() { return ${r}; } }); return Object.getOwnPropertyDescriptor(p, 'a');`);
  }
}
// ---- ownKeys
const keysReturns = ["[]", "['a']", "['a', 'a']", "['a', 'b']", "['b']", "[1]", "[Symbol.iterator]", "[{}]", "undefined", "null", "1", "'a'", "{ length: 1, 0: 'a' }", "new Set(['a'])", "[undefined]", "['a', null]"];
for (const [n, setup] of Object.entries(targets)) {
  for (const r of keysReturns) {
    t(`${setup} const p = new Proxy(t, { ownKeys() { return ${r}; } }); return Reflect.ownKeys(p);`);
    t(`${setup} const p = new Proxy(t, { ownKeys() { return ${r}; } }); return Object.keys(p);`);
  }
}
// ---- getPrototypeOf / setPrototypeOf / isExtensible / preventExtensions
const protoSetups = { ext: "const t = {};", nonext: "const t = Object.preventExtensions({});", nonextProto: "const t = Object.preventExtensions(Object.create(Array.prototype));", extProto: "const t = Object.create(Array.prototype);" };
for (const [n, setup] of Object.entries(protoSetups)) {
  for (const r of ["null", "undefined", "{}", "Array.prototype", "Object.prototype", "1", "'x'", "function () {}"]) {
    t(`${setup} const p = new Proxy(t, { getPrototypeOf() { return ${r}; } }); return Object.getPrototypeOf(p) === Array.prototype;`);
    t(`${setup} const p = new Proxy(t, { getPrototypeOf() { return ${r}; } }); return Reflect.getPrototypeOf(p) === null;`);
    t(`${setup} const p = new Proxy(t, { getPrototypeOf() { return ${r}; } }); return p instanceof Array;`);
    t(`${setup} const p = new Proxy(t, { getPrototypeOf() { return ${r}; } }); return Array.prototype.isPrototypeOf(p);`);
  }
  for (const r of ["true", "false", "undefined", "1", "0", "''"]) {
    for (const target of ["null", "Object.prototype", "Array.prototype", "{}"]) {
      t(`${setup} const p = new Proxy(t, { setPrototypeOf() { return ${r}; } }); return Reflect.setPrototypeOf(p, ${target});`);
      t(`${setup} const p = new Proxy(t, { setPrototypeOf() { return ${r}; } }); Object.setPrototypeOf(p, ${target}); return 'ok';`);
    }
    t(`${setup} const p = new Proxy(t, { isExtensible() { return ${r}; } }); return Object.isExtensible(p);`);
    t(`${setup} const p = new Proxy(t, { isExtensible() { return ${r}; } }); return Reflect.isExtensible(p);`);
    t(`${setup} const p = new Proxy(t, { preventExtensions() { return ${r}; } }); return Reflect.preventExtensions(p);`);
    t(`${setup} const p = new Proxy(t, { preventExtensions() { return ${r}; } }); Object.preventExtensions(p); return 'ok';`);
    t(`${setup} const p = new Proxy(t, { preventExtensions() { return ${r}; } }); Object.freeze(p); return 'ok';`);
    t(`${setup} const p = new Proxy(t, { preventExtensions() { return ${r}; } }); Object.seal(p); return 'ok';`);
  }
}
// ---- apply / construct
const callables = { fn: "function (a, b) { return a + b; }", arrow: "(a, b) => a + b", cls: "class A { constructor(a) { this.a = a; } }", bound: "function () {}.bind(null)", gen: "function* () {}", async: "async function () {}", method: "({ m() {} }).m" };
for (const [n, f] of Object.entries(callables)) {
  for (const r of ["undefined", "1", "{}", "null", "'x'", "function () {}", "[]", "Symbol()"]) {
    t(`const p = new Proxy(${f}, { apply() { return ${r}; } }); return typeof p(1, 2);`);
    t(`const p = new Proxy(${f}, { construct() { return ${r}; } }); return typeof new p(1, 2);`);
    t(`const p = new Proxy(${f}, { construct() { return ${r}; } }); return typeof Reflect.construct(p, [1]);`);
  }
  t(`const p = new Proxy(${f}, {}); return p(1, 2);`);
  t(`const p = new Proxy(${f}, {}); return new p(1, 2) !== undefined;`);
  t(`const p = new Proxy(${f}, { apply: 1 }); return p();`);
  t(`const p = new Proxy(${f}, { construct: 1 }); return new p();`);
  t(`const p = new Proxy(${f}, { apply: null }); return typeof p;`);
  t(`const p = new Proxy(${f}, { construct: null }); return typeof new p;`);
  t(`const p = new Proxy(${f}, { apply(t, th, args) { return [typeof t, th, args.length]; } }); return p.call(5, 1, 2, 3);`);
  t(`const p = new Proxy(${f}, { apply(t, th, args) { return Array.isArray(args) + ':' + args.join(); } }); return Reflect.apply(p, null, [1, 2]);`);
  t(`const p = new Proxy(${f}, { construct(t, args, nt) { return { nt: nt === p, n: args.length }; } }); return new p(1, 2);`);
}
// Alvo não chamável ou não construtor.
for (const target of ["{}", "[]", "1 && {}", "Math.max", "(() => 1)", "({ m() {} }).m", "async () => 1", "function* () {}", "async function* () {}", "Symbol", "BigInt", "Math"]) {
  t(`const p = new Proxy(${target}, { apply() { return 1; }, construct() { return {}; } }); return typeof p + ':' + (() => { try { return p(); } catch (e) { return e.name + e.message; } })();`);
  t(`const p = new Proxy(${target}, { apply() { return 1; }, construct() { return {}; } }); try { return typeof new p; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`const p = new Proxy(${target}, {}); try { return Reflect.construct(function () {}, [], p) !== undefined; } catch (e) { return e.name + ': ' + e.message; }`);
}
// ---- Traps que não são função
for (const trap of ["get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf", "setPrototypeOf", "isExtensible", "preventExtensions"]) {
  for (const v of ["1", "'s'", "{}", "[]", "true", "Symbol()", "null", "undefined"]) {
    t(`const p = new Proxy({ a: 1 }, { ${trap}: ${v} }); const out = []; for (const op of [() => p.a, () => { p.a = 2; return 1; }, () => 'a' in p, () => delete p.a, () => Object.defineProperty(p, 'b', { value: 1 }), () => Object.getOwnPropertyDescriptor(p, 'a'), () => Object.keys(p), () => Object.getPrototypeOf(p), () => Object.setPrototypeOf(p, null), () => Object.isExtensible(p), () => Object.preventExtensions(p)]) { try { op(); out.push('.'); } catch (e) { out.push(e.message); } } return out.join('|');`);
  }
}

// ---- Revoke: cada operação, mensagem própria.
const revokedOps = [
  "p.a", "p.a = 1", "'a' in p", "delete p.a", "Object.defineProperty(p, 'a', { value: 1 })", "Object.getOwnPropertyDescriptor(p, 'a')", "Object.keys(p)", "Reflect.ownKeys(p)",
  "Object.getPrototypeOf(p)", "Object.setPrototypeOf(p, null)", "Object.isExtensible(p)", "Object.preventExtensions(p)", "Object.freeze(p)", "Object.seal(p)", "Object.isFrozen(p)", "Object.isSealed(p)",
  "p()", "new p()", "Array.isArray(p)", "typeof p", "String(p)", "`${p}`", "p + ''", "JSON.stringify(p)", "p instanceof Object", "({}) instanceof p", "[...p]", "Object.assign({}, p)", "Object.entries(p)", "Object.values(p)",
  "Object.prototype.toString.call(p)", "Object.prototype.hasOwnProperty.call(p, 'a')", "Object.prototype.propertyIsEnumerable.call(p, 'a')", "Object.prototype.isPrototypeOf.call(p, {})", "for (const k in p) {}", "Reflect.get(p, 'a')", "Reflect.set(p, 'a', 1)", "Reflect.has(p, 'a')",
  "Reflect.apply(p, null, [])", "Reflect.construct(p, [])", "Object.create(p)", "Object.getOwnPropertyNames(p)", "Object.getOwnPropertySymbols(p)", "Object.getOwnPropertyDescriptors(p)", "Object.fromEntries(p)", "Array.from(p)", "[].concat(p)", "Function.prototype.toString.call(p)", "Object.prototype.toString.call([p])", "p.a++", "p.a ??= 1", "p?.a", "({ ...p })", "new Map(p)",
  "Object.is(p, p)", "p === p", "Object.hasOwn(p, 'a')", "structuredClone(p)", "Array.prototype.push.call(p, 1)", "Array.prototype.map.call(p, x => x)", "Object.groupBy(p, x => x)", "Object.entries({ a: p }).length", "Symbol.keyFor(p)", "WeakRef(p)", "new WeakRef(p).deref() === p", "new WeakMap().set(p, 1).has(p)", "new Set([p]).has(p)", "Reflect.getPrototypeOf(p)", "Reflect.setPrototypeOf(p, null)", "Reflect.isExtensible(p)", "Reflect.preventExtensions(p)", "Reflect.defineProperty(p, 'a', {})", "Reflect.getOwnPropertyDescriptor(p, 'a')", "Reflect.deleteProperty(p, 'a')",
];
for (const [kind, target] of [["obj", "{}"], ["fn", "function () {}"], ["arr", "[]"]]) {
  for (const op of revokedOps) {
    t(`const { proxy: p, revoke } = Proxy.revocable(${target}, {}); revoke(); return ${op};`.replace("return for", "for").replace(/return (for \(const k in p\) \{\})/, "$1"));
  }
}
t("const r = Proxy.revocable({}, {}); return Object.keys(r).join() + typeof r.revoke + r.revoke.length + r.revoke.name + JSON.stringify(Object.getOwnPropertyNames(r.revoke));");
t("const r = Proxy.revocable({}, {}); return r.revoke() + ':' + r.revoke();");
t("const r = Proxy.revocable({}, {}); r.revoke(); return new Proxy(r.proxy, {}) !== undefined;");
t("const r = Proxy.revocable({}, {}); r.revoke(); return Proxy.revocable(r.proxy, {}).proxy !== undefined;");
t("const r = Proxy.revocable({}, {}); const q = new Proxy(r.proxy, {}); r.revoke(); return q.a;");
t("const r = Proxy.revocable(function () {}, {}); r.revoke(); return typeof r.proxy;");
t("const r = Proxy.revocable({}, {}); r.revoke(); return typeof r.proxy;");
t("const r = Proxy.revocable([], {}); r.revoke(); return Array.isArray(r.proxy);");
t("const r = Proxy.revocable({}, { get() { return 1; } }); const f = r.revoke; f.call(null); return r.proxy.a;");
t("const r = Proxy.revocable({}, {}); return Object.isExtensible(r.revoke) + ':' + (r.revoke.prototype === undefined) + ':' + ('prototype' in r.revoke);");
t("const r = Proxy.revocable({}, {}); try { new r.revoke(); } catch (e) { return e.name + ': ' + e.message; }");
t("return Proxy.revocable.call(null, {}, {}) !== undefined;");
t("try { new Proxy.revocable({}, {}); } catch (e) { return e.name + ': ' + e.message; }");
for (const [a, b] of [["1", "{}"], ["{}", "1"], ["null", "{}"], ["{}", "null"], ["", ""], ["{}", ""]]) {
  t(`try { return Proxy.revocable(${a}${b ? ", " + b : ""}) !== undefined; } catch (e) { return e.name + ': ' + e.message; }`);
}
// Revoke dentro da trap.
for (const trap of ["get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf", "isExtensible", "preventExtensions"]) {
  const ops = { get: "p.a", set: "p.a = 1", has: "'a' in p", deleteProperty: "delete p.a", defineProperty: "Object.defineProperty(p, 'a', { value: 1, configurable: true })", getOwnPropertyDescriptor: "Object.getOwnPropertyDescriptor(p, 'a')", ownKeys: "Object.keys(p)", getPrototypeOf: "Object.getPrototypeOf(p)", isExtensible: "Object.isExtensible(p)", preventExtensions: "Object.preventExtensions(p)" };
  t(`const { proxy: p, revoke } = Proxy.revocable({ a: 1 }, { ${trap}(t, ...r) { revoke(); return Reflect.${trap}(t, ...r.slice(0, ${trap === "get" || trap === "set" ? 0 : r_len(trap)})); } }); return ${ops[trap]};`);
  t(`const { proxy: p, revoke } = Proxy.revocable({ a: 1 }, { ${trap}(t, ...r) { revoke(); return undefined; } }); try { return ${ops[trap]}; } catch (e) { return e.name + ': ' + e.message; }`);
}
function r_len() { return 1; }

// ---- Proxy como protótipo.
for (const [kind, h] of [["get", "get(t, k, r) { log.push('get:' + String(k) + ':' + (r === o)); return 7; }"], ["set", "set(t, k, v, r) { log.push('set:' + String(k) + ':' + v + ':' + (r === o)); return true; }"], ["has", "has(t, k) { log.push('has:' + String(k)); return true; }"], ["gopd", "getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return undefined; }"], ["def", "defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); }"], ["del", "deleteProperty(t, k) { log.push('del:' + String(k)); return true; }"], ["keys", "ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t); }"], ["gpo", "getPrototypeOf(t) { log.push('gpo'); return null; }"]]) {
  const setup = `const log = []; const proto = new Proxy({ x: 1 }, { ${h} }); const o = Object.create(proto);`;
  t(`${setup} return [o.a, log.join()];`);
  t(`${setup} o.a = 1; return [Object.keys(o).join(), log.join()];`);
  t(`${setup} return ['a' in o, log.join()];`);
  t(`${setup} return [delete o.a, log.join()];`);
  t(`${setup} for (const k in o) {} return log.join();`);
  t(`${setup} return [Object.keys(o).length, log.join()];`);
  t(`${setup} Object.defineProperty(o, 'a', { value: 1 }); return log.join();`);
  t(`${setup} return [o instanceof Object, log.join()];`);
  t(`${setup} return [Object.prototype.isPrototypeOf.call(proto, o), log.join()];`);
  t(`${setup} return [o.hasOwnProperty('a'), log.join()];`);
  t(`${setup} return [Reflect.get(o, 'a', 5), log.join()];`);
  t(`${setup} return [Reflect.set(o, 'a', 1, 5), log.join()];`);
  t(`${setup} const c = Object.create(o); return [c.a, log.join()];`);
  t(`${setup} const c = Object.create(o); c.a = 3; return [Object.keys(c).join(), log.join()];`);
  t(`${setup} class C {}; Object.setPrototypeOf(C.prototype, proto); const c = new C(); return [c.zz, log.join()];`);
  t(`${setup} with (o) { try { return [typeof zz, log.join()]; } catch (e) { return e.name; } }`);
}
t("const o = Object.create(new Proxy({}, { get(t, k, r) { return r; } })); return o.a === o;");
t("const o = Object.create(new Proxy({}, { set(t, k, v, r) { r.seen = v; return true; } })); o.a = 4; return Object.keys(o).join() + o.seen;");
t("const o = Object.create(new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r); } })); o.a = 4; return Object.getOwnPropertyDescriptor(o, 'a').value;");
t("'use strict'; const o = Object.create(new Proxy({}, { set() { return false; } })); try { o.a = 1; } catch (e) { return e.name + ': ' + e.message; }");
t("const arr = Object.create(new Proxy([1, 2, 3], {})); return arr.length + ':' + arr[1];");
t("const o = Object.create(new Proxy({}, { get(t, k) { return typeof k; } })); return [o.a, o[Symbol.iterator], o[1]].join();");
t("const o = Object.create(new Proxy({}, { has(t, k) { return k === 'x'; } })); with (o) { return typeof x + typeof y; }");

// ---- Proxy em operações de alto nível.
const pa = "new Proxy({ a: 1, b: 2, [Symbol('s')]: 3 }, {})";
for (const [n, e] of Object.entries({
  forIn: "const out = []; for (const k in P) out.push(k); return out.join();",
  keys: "return Object.keys(P).join();", values: "return Object.values(P).join();", entries: "return Object.entries(P).join('|');",
  names: "return Object.getOwnPropertyNames(P).join();", syms: "return Object.getOwnPropertySymbols(P).length;",
  json: "return JSON.stringify(P);", jsonArr: "return JSON.stringify([P, { x: P }]);", jsonInd: "return JSON.stringify(P, null, 1);",
  isArray: "return Array.isArray(P);", spread: "return JSON.stringify({ ...P });", assign: "return JSON.stringify(Object.assign({}, P));",
  assignTo: "const o = Object.assign(P, { z: 1 }); return o === P;", inst: "return P instanceof Object;", instFn: "return ({}) instanceof new Proxy(Object, {});",
  typeofP: "return typeof P;", tos: "return Object.prototype.toString.call(P);", str: "return String(P);", concat: "return '' + P;",
  freeze: "Object.freeze(P); return Object.isFrozen(P);", seal: "Object.seal(P); return Object.isSealed(P);",
  fromEntries: "return JSON.stringify(Object.fromEntries(Object.entries(P)));", hasOwn: "return Object.hasOwn(P, 'a');",
  gopds: "return JSON.stringify(Object.getOwnPropertyDescriptors(P));", defs: "return JSON.stringify(Object.defineProperties({}, P));",
  create: "return Object.create(P).a;", withP: "with (P) { return a + b; }", destr: "const { a, ...rest } = P; return a + JSON.stringify(rest);",
  structured: "try { structuredClone(P); } catch (e) { return e.name; }", arrFrom: "return Array.from(P).length;",
  objectIs: "return Object.is(P, P);", weak: "return new WeakSet([P]).has(P);", mapKey: "return new Map([[P, 1]]).get(P);",
})) {
  t(`const P = ${pa}; ${e}`);
  t(`const log = []; const P = new Proxy({ a: 1, b: 2 }, new Proxy({}, { get(t, k) { log.push(k); return undefined; } })); try { (function () { ${e} })(); } catch (e) {} return log.join();`);
}
for (const [n, e] of Object.entries({
  length: "return P.length;", push: "P.push(4); return P.length + ':' + P[3];", pop: "return P.pop() + ':' + P.length;", shift: "return P.shift() + ':' + P.length;", unshift: "P.unshift(0); return P.length;",
  concat: "return [].concat(P).length;", concat2: "return P.concat([4]).length;", concatNoSpread: "P[Symbol.isConcatSpreadable] = false; return [].concat(P).length;", map: "return P.map(x => x * 2).join();", slice: "return P.slice(1).join();",
  species: "return Array.isArray(P.map(x => x)) + ':' + (P.constructor === Array);", filter: "return P.filter(x => x > 1).join();", splice: "return P.splice(1, 1).join() + ':' + P.length;", join: "return P.join('-');",
  indexOf: "return P.indexOf(2);", includes: "return P.includes(3);", reverse: "return P.reverse().join();", sort: "return P.sort((a, b) => b - a).join();", fill: "return P.fill(0).join();", flat: "return P.flat().join();",
  spread: "return [...P].join();", jsonP: "return JSON.stringify(P);", ctor: "return P.constructor === Array;", proto: "return Object.getPrototypeOf(P) === Array.prototype;", tos: "return Object.prototype.toString.call(P);", ownk: "return Reflect.ownKeys(P).join();", setLen: "P.length = 1; return P.length + ':' + JSON.stringify(P);",
  isArr: "return Array.isArray(P) + ':' + Array.isArray(new Proxy(P, {}));", from: "return Array.from(P).join();", of: "return Array.prototype.slice.call(P).length;", forEach: "let n = 0; P.forEach(() => n++); return n;", at: "return P.at(-1);", copyWithin: "return P.copyWithin(0, 1).join();", entries: "return JSON.stringify([...P.entries()]);", keysI: "return [...P.keys()].join();", lastIndexOf: "return P.lastIndexOf(1);", toSorted: "return P.toSorted((a, b) => b - a).join();", toReversed: "return P.toReversed().join();", with: "return P.with(0, 9).join();", reduce: "return P.reduce((a, b) => a + b);", findLast: "return P.findLast(x => x < 3);", delIdx: "delete P[1]; return P.length + ':' + (1 in P);", defLen: "return Reflect.defineProperty(P, 'length', { value: 1 }) + ':' + P.length;",
})) {
  t(`const P = new Proxy([1, 2, 3], {}); ${e}`);
  t(`const log = []; const P = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); }, getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); } }); (function () { ${e} })(); return log.join();`);
}
t("const P = new Proxy([], { get(t, k, r) { return k === 'constructor' ? { [Symbol.species]: function (n) { return { length: n, tag: 'sp' }; } } : Reflect.get(t, k, r); } }); return JSON.stringify(P.map(x => x));");
t("const P = new Proxy([1], { get(t, k, r) { return k === 'constructor' ? { [Symbol.species]: function (n) { return { length: n, tag: 'sp' }; } } : Reflect.get(t, k, r); } }); return JSON.stringify(P.map(x => x));");
t("const P = new Proxy([1, 2], { get(t, k, r) { return k === 'constructor' ? undefined : Reflect.get(t, k, r); } }); return Array.isArray(P.map(x => x));");
t("const P = new Proxy([1, 2], { get(t, k, r) { return k === 'constructor' ? null : Reflect.get(t, k, r); } }); try { return Array.isArray(P.map(x => x)); } catch (e) { return e.name + ': ' + e.message; }");
t("const P = new Proxy([1, 2], { get(t, k, r) { return k === 'constructor' ? 1 : Reflect.get(t, k, r); } }); try { return Array.isArray(P.map(x => x)); } catch (e) { return e.name + ': ' + e.message; }");
t("const P = new Proxy([1, 2], { get(t, k, r) { return k === Symbol.isConcatSpreadable ? true : Reflect.get(t, k, r); } }); return [].concat(P, [3]).join();");
t("const P = new Proxy({ length: 2, 0: 'a', 1: 'b' }, { get(t, k, r) { return k === Symbol.isConcatSpreadable ? true : Reflect.get(t, k, r); } }); return [].concat(P).join();");
t("const P = new Proxy([1, 2, 3], {}); return Array.prototype.concat.call(P, 4).length;");
t("const P = new Proxy([1, 2, 3], {}); return P.concat(4) instanceof Array;");
t("const P = new Proxy([3, 1, 2], {}); return P.sort() === P;");

// ---- Proxy de Proxy.
for (const depth of [1, 2, 3, 5]) {
  let expr = "{ a: 1 }";
  for (let i = 0; i < depth; i++) expr = `new Proxy(${expr}, { get(t, k, r) { log.push('g' + ${i}); return Reflect.get(t, k, r === undefined ? t : r); } })`;
  t(`const log = []; const P = ${expr}; return [P.a, log.join()];`);
  t(`const log = []; const P = ${expr}; return ['a' in P, Object.keys(P).join(), log.join()];`);
}
let nested = "{}";
for (let i = 0; i < 3; i++) nested = `new Proxy(${nested}, {})`;
t(`const P = ${nested}; P.x = 1; return JSON.stringify(P) + Object.getPrototypeOf(P)?.constructor?.name;`);
t("const inner = new Proxy({}, { get() { return 'inner'; } }); const outer = new Proxy(inner, {}); return outer.zz;");
t("const inner = new Proxy({}, { get() { return 'inner'; } }); const outer = new Proxy(inner, { get(t, k, r) { return Reflect.get(t, k, r) + '+outer'; } }); return outer.zz;");
t("const inner = new Proxy({}, { set(t, k, v, r) { return false; } }); const outer = new Proxy(inner, {}); return Reflect.set(outer, 'a', 1);");
t("const inner = new Proxy({}, { has() { return true; } }); const outer = new Proxy(inner, {}); return 'q' in outer;");
t("const inner = new Proxy({}, { ownKeys() { return ['a']; }, getOwnPropertyDescriptor() { return { value: 1, enumerable: true, configurable: true }; } }); const outer = new Proxy(inner, {}); return Object.keys(outer).join();");
t("const inner = new Proxy({}, { getPrototypeOf() { return Array.prototype; } }); const outer = new Proxy(inner, {}); return outer instanceof Array;");
t("const inner = new Proxy(function () { return 1; }, {}); const outer = new Proxy(inner, { apply(t, th, a) { return t() + 1; } }); return outer();");
t("const inner = new Proxy(class A {}, {}); const outer = new Proxy(inner, {}); return new outer() instanceof inner;");
t("const o = {}; const p1 = new Proxy(o, {}); const p2 = new Proxy(p1, {}); Object.preventExtensions(p2); return Object.isExtensible(o) + ':' + Object.isExtensible(p1);");
t("const o = { a: 1 }; const p1 = new Proxy(o, {}); const p2 = new Proxy(p1, {}); Object.freeze(p2); return Object.isFrozen(o);");
t("const p1 = Proxy.revocable({}, {}); const p2 = new Proxy(p1.proxy, {}); p1.revoke(); try { Object.keys(p2); } catch (e) { return e.name + ': ' + e.message; }");
t("const p1 = Proxy.revocable({}, {}); const p2 = new Proxy(p1.proxy, {}); p1.revoke(); return typeof p2;");
t("const p1 = Proxy.revocable([], {}); const p2 = new Proxy(p1.proxy, {}); p1.revoke(); try { return Array.isArray(p2); } catch (e) { return e.name + ': ' + e.message; }");
t("const p1 = Proxy.revocable({}, {}); const p2 = new Proxy(p1.proxy, {}); p1.revoke(); try { return JSON.stringify(p2); } catch (e) { return e.name + ': ' + e.message; }");

// ---- Proxy de função com new.target.
t("function F() { return new.target; } const P = new Proxy(F, {}); return new P() === F;");
t("function F() { this.nt = new.target === P; } const P = new Proxy(F, {}); return new P().nt;");
t("function F() { this.nt = new.target === F; } const P = new Proxy(F, {}); return new P().nt;");
t("class A { constructor() { this.nt = new.target; } } const P = new Proxy(A, {}); return new P().nt === P;");
t("class A { constructor() { this.nt = new.target; } } const P = new Proxy(A, {}); return new P().nt === A;");
t("class A { constructor() { this.nt = new.target; } } const P = new Proxy(A, { construct(t, a, nt) { return Reflect.construct(t, a, nt); } }); return new P().nt === P;");
t("class A { constructor() { this.nt = new.target; } } const P = new Proxy(A, { construct(t, a, nt) { return Reflect.construct(t, a); } }); return new P().nt === A;");
t("class A { constructor() { this.nt = new.target; } } class B {} const P = new Proxy(A, { construct(t, a, nt) { return Reflect.construct(t, a, B); } }); const o = new P(); return (o.nt === B) + ':' + (Object.getPrototypeOf(o) === B.prototype);");
t("function F() {} const P = new Proxy(F, { construct(t, a, nt) { return Reflect.construct(t, a, nt); } }); return Object.getPrototypeOf(new P()) === F.prototype;");
t("function F() {} const P = new Proxy(F, {}); P.prototype = { x: 1 }; return new P().x;");
t("function F() {} const P = new Proxy(F, { get(t, k, r) { return k === 'prototype' ? { y: 2 } : Reflect.get(t, k, r); } }); return new P().y;");
t("class A { constructor() { return new.target.name; } } const P = new Proxy(A, {}); return new P() instanceof A;");
t("class B extends new Proxy(class A { constructor() { this.nt = new.target.name; } }, {}) {} return new B().nt;");
t("class A { constructor() { this.nt = new.target; } } class B extends new Proxy(A, {}) {} return new B().nt === B;");
t("class A { static s = 1; } const P = new Proxy(A, {}); return P.s + ':' + P.name + ':' + P.length;");
t("class A {} const P = new Proxy(A, {}); try { return P(); } catch (e) { return e.name + ': ' + e.message; }");
t("const P = new Proxy(function () {}, {}); return P.name + ':' + P.length + ':' + (P.prototype === undefined);");
t("const P = new Proxy(function f(a, b) {}, {}); return Function.prototype.toString.call(P);");
t("const P = new Proxy(class A {}, {}); return Function.prototype.toString.call(P);");
t("const P = new Proxy(() => {}, {}); return Function.prototype.toString.call(P);");
t("const P = new Proxy({}, {}); try { return Function.prototype.toString.call(P); } catch (e) { return e.name + ': ' + e.message; }");
t("const P = new Proxy(function () {}, {}); return P.bind(null) !== undefined && P.call(1) === undefined && P.apply(null, []) === undefined;");
t("const P = new Proxy(function () { return this; }, {}); return P.call(5) === 5 || typeof P.call(5);");
t("const P = new Proxy(function () { 'use strict'; return this; }, {}); return P.call(5);");
t("const P = new Proxy(function () { return arguments.length; }, {}); return P(1, 2, 3) + P.apply(null, [1]);");
t("const P = new Proxy(function () {}, {}); return P instanceof Function && Object.getPrototypeOf(P) === Function.prototype;");
t("const P = new Proxy(function () {}, {}); return Object.prototype.toString.call(P);");
t("const P = new Proxy(Array, {}); return new P(3).length + ':' + P.of(1, 2).length + ':' + Array.isArray(P(2));");
t("const P = new Proxy(Map, {}); return new P([[1, 2]]).get(1);");
t("const P = new Proxy(Promise, {}); return P.resolve(1) instanceof Promise;");
t("const P = new Proxy(Date, {}); return new P(0).getTime();");
t("const P = new Proxy(Error, {}); return new P('m').message + ':' + (new P('m') instanceof Error);");
t("const P = new Proxy(Object, {}); return new P(1) instanceof Number;");
t("const P = new Proxy(Symbol, {}); try { return new P(); } catch (e) { return e.name + ': ' + e.message; }");
t("const P = new Proxy(Boolean, {}); return P(0) + ':' + typeof new P(0);");
t("const P = new Proxy(Function, {}); return new P('return 1')();");
t("const P = new Proxy(RegExp, {}); return new P('a', 'g').flags;");
t("const P = new Proxy(Uint8Array, {}); return new P(2).length;");
t("class A extends new Proxy(Array, {}) {} const a = new A(3); return a.length + ':' + (a instanceof Array) + ':' + Array.isArray(a);");
t("class A extends new Proxy(Map, {}) {} return new A([[1, 2]]).get(1);");
t("class A extends new Proxy(Error, {}) {} return new A('x').message + ':' + new A('x').stack.split('\\n')[0];");
t("class A extends new Proxy(Object, {}) {} return new A() instanceof A;");
t("class A extends new Proxy(function () {}, { get(t, k, r) { return k === 'prototype' ? null : Reflect.get(t, k, r); } }) {} try { return typeof new A(); } catch (e) { return e.name + ': ' + e.message; }");
t("try { class A extends new Proxy({}, {}) {} } catch (e) { return e.name + ': ' + e.message; }");
t("try { class A extends new Proxy(() => {}, {}) {} } catch (e) { return e.name + ': ' + e.message; }");

// ---- Ordem de traps.
const logger = "const log = []; const h = new Proxy({}, { get(t, name) { return (...a) => { log.push(name + (typeof a[1] === 'string' || typeof a[1] === 'symbol' ? ':' + String(a[1]) : '')); return Reflect[name](...a); }; } });";
for (const [n, e] of Object.entries({
  assign: "Object.assign({}, P);", assignTo: "Object.assign(P, { x: 1, y: 2 });", entries: "Object.entries(P);", values: "Object.values(P);", keys: "Object.keys(P);", freeze: "Object.freeze(P);", seal: "Object.seal(P);", isFrozen: "Object.isFrozen(P);", isSealed: "Object.isSealed(P);",
  fromEntries: "Object.fromEntries(Object.entries(P));", spread: "({ ...P });", forIn: "for (const k in P) {}", withS: "with (P) { a; }", withSet: "with (P) { a = 2; }", withCall: "with (P) { typeof zz; }", ownKeys: "Reflect.ownKeys(P);", getOwnPropertyNames: "Object.getOwnPropertyNames(P);", gopds: "Object.getOwnPropertyDescriptors(P);",
  json: "JSON.stringify(P);", jsonRev: "JSON.parse('{\"a\":1}', function (k, v) { return v; }); JSON.stringify({ a: P });", destr: "const { a, b } = P;", destrRest: "const { a, ...r } = P;", inOp: "'a' in P;", del: "delete P.a;", setOp: "P.a = 5;", setNew: "P.q = 5;", incr: "P.a++;", compound: "P.a += 1;", logical: "P.a ||= 1; P.zz ??= 2;", hasOwn: "Object.hasOwn(P, 'a');", hop: "P.hasOwnProperty('a');", propEnum: "Object.prototype.propertyIsEnumerable.call(P, 'a');", isProto: "Object.prototype.isPrototypeOf.call(Object.prototype, P);", inst: "P instanceof Object;", tos: "Object.prototype.toString.call(P);", str: "String(P);", toPrim: "P + 1;", defProp: "Object.defineProperty(P, 'z', { value: 1, configurable: true });", defProps: "Object.defineProperties(P, { z: { value: 1 } });", gopd: "Object.getOwnPropertyDescriptor(P, 'a');", setProto: "Object.setPrototypeOf(P, null);", create: "Object.create(P).a;", isExt: "Object.isExtensible(P);", prevExt: "Object.preventExtensions(P);", groupBy: "Object.groupBy([P], x => 'k');", arrayFrom: "Array.from(P);", mapCtor: "new Map(Object.entries(P));", structuredClone: "try { structuredClone(P); } catch (e) {}", objectCall: "Object(P);", rest: "(({ a, ...r }) => 0)(P);", spreadArr: "try { [...P]; } catch (e) {}", getter: "P.a;", sym: "P[Symbol.iterator];", symTag: "String(P);", numKey: "P[0];", concatArr: "[].concat(P);", isArr: "Array.isArray(P);", flatten: "[[P]].flat();", defaultOpts: "Object.entries(Object.assign({}, P));",
})) {
  t(`${logger} const P = new Proxy({ a: 1, b: 2, [Symbol.for('s')]: 3 }, h); ${e} return log.join();`);
  t(`${logger} const P = new Proxy([1, 2], h); ${e} return log.join();`.replace("zz", "zz"));
}

// ---- Reflect.
const rf = ["apply", "construct", "defineProperty", "deleteProperty", "get", "getOwnPropertyDescriptor", "getPrototypeOf", "has", "isExtensible", "ownKeys", "preventExtensions", "set", "setPrototypeOf"];
for (const m of rf) {
  t(`return typeof Reflect.${m} + ':' + Reflect.${m}.length + ':' + Reflect.${m}.name;`);
  for (const bad of ["1", "'s'", "undefined", "null", "true", "Symbol()", "1n", "function () {}"]) {
    const call = { apply: `Reflect.apply(${bad}, null, [])`, construct: `Reflect.construct(${bad}, [])`, defineProperty: `Reflect.defineProperty(${bad}, 'a', {})`, deleteProperty: `Reflect.deleteProperty(${bad}, 'a')`, get: `Reflect.get(${bad}, 'a')`, getOwnPropertyDescriptor: `Reflect.getOwnPropertyDescriptor(${bad}, 'a')`, getPrototypeOf: `Reflect.getPrototypeOf(${bad})`, has: `Reflect.has(${bad}, 'a')`, isExtensible: `Reflect.isExtensible(${bad})`, ownKeys: `Reflect.ownKeys(${bad})`, preventExtensions: `Reflect.preventExtensions(${bad})`, set: `Reflect.set(${bad}, 'a', 1)`, setPrototypeOf: `Reflect.setPrototypeOf(${bad}, null)` }[m];
    t(`return ${call};`);
  }
  t(`return Reflect.${m}();`);
  t(`return Reflect.${m}.call(undefined);`);
  t(`try { return new Reflect.${m}(); } catch (e) { return e.name + ': ' + e.message; }`);
}
for (const bad of ["1", "'s'", "undefined", "null", "{}", "true", "Symbol()", "function () {}"]) {
  t(`return Reflect.apply(function () { return arguments.length; }, null, ${bad});`);
  t(`return Reflect.construct(function () { this.n = arguments.length; }, ${bad}).n;`);
  t(`return Reflect.construct(function () {}, [], ${bad}) !== undefined;`);
  t(`return Reflect.setPrototypeOf({}, ${bad});`);
  t(`return Reflect.defineProperty({}, 'a', ${bad});`);
  t(`return Reflect.get({ get a() { return this; } }, 'a', ${bad}) === ${bad};`);
  t(`return Reflect.get({ get a() { 'use strict'; return typeof this; } }, 'a', ${bad});`);
  t(`return Reflect.set({}, 'a', 1, ${bad});`);
  t(`return Reflect.set({ set a(v) { 'use strict'; this.x = v; } }, 'a', 1, ${bad});`);
  t(`return Reflect.has({ a: 1 }, ${bad});`);
  t(`return Reflect.getOwnPropertyDescriptor({ a: 1 }, ${bad});`);
  t(`return Reflect.deleteProperty({ a: 1 }, ${bad});`);
  t(`return Reflect.ownKeys(Object(${bad}));`);
}
// apply com array-like.
for (const al of ["{ length: 2, 0: 'a', 1: 'b' }", "{ length: 0 }", "{}", "{ length: '2', 0: 1, 1: 2 }", "{ length: -1 }", "{ length: 1.9, 0: 'x' }", "{ length: 2 ** 32 }", "'ab'", "new String('ab')", "[1, , 3]", "Object.assign(() => {}, { 0: 1, length: 1 })", "new Uint8Array([1, 2])", "(function () { return arguments; })(1, 2)", "new Proxy([1, 2], {})", "{ get length() { throw new Error('len'); } }", "{ length: 1, get 0() { throw new Error('idx'); } }", "{ length: NaN }", "{ length: Infinity }", "{ length: {valueOf() { return 2; }} }", "{ length: 1, 0: undefined }", "new Set([1])", "new Map([[1, 2]])", "{ length: 2 ** 53 }"]) {
  t(`return Reflect.apply(function (...a) { return a.length + ':' + a.join(); }, null, ${al});`);
  t(`return Reflect.construct(function (...a) { this.r = a.length + ':' + a.join(); }, ${al}).r;`);
  t(`return Reflect.apply(Math.max, null, ${al});`);
  t(`return Math.max.apply(null, ${al});`);
  t(`return (function (...a) { return a.length; }).apply(null, ${al});`);
}
for (const thisV of ["undefined", "null", "1", "'s'", "{}", "true", "Symbol.iterator"]) {
  t(`return Reflect.apply(function () { return typeof this; }, ${thisV}, []);`);
  t(`return Reflect.apply(function () { 'use strict'; return this; }, ${thisV}, []);`);
  t(`return Reflect.apply(() => typeof this, ${thisV}, []);`);
}
t("return Reflect.apply(Math.max, null, [1, 2, 3]);");
t("return Reflect.apply(String.prototype.slice, 'abcdef', [1, 3]);");
t("return Reflect.apply(Array.prototype.concat, [1], [[2], 3]).join();");
t("return Reflect.apply(class A {}, null, []);");
t("return Reflect.apply(Symbol, null, ['x']).toString();");
t("return Reflect.apply(BigInt, null, [1]);");
t("return Reflect.apply(Object.prototype.toString, 1, []);");
t("return Reflect.apply(function f() { return f.length; }, null, [1, 2, 3, 4]);");
t("return Reflect.apply(Function.prototype.call, function () { return this; }, [5]);");
t("return Reflect.apply(Function.prototype.apply, function (a) { return a; }, [null, [7]]);");
// construct com newTarget.
for (const nt of ["undefined", "null", "1", "{}", "function () {}", "() => {}", "class B {}", "async function () {}", "function* () {}", "Math.max", "(function () {}).bind(null)", "Symbol", "new Proxy(function () {}, {})", "new Proxy({}, {})", "Object", "Array", "Date", "Promise", "Boolean", "(class extends Array {})", "({ m() {} }).m", "Function.prototype", "Proxy"]) {
  t(`try { const o = Reflect.construct(function () { this.a = 1; }, [], ${nt}); return Object.getPrototypeOf(o) === Object.prototype ? 'objproto' : 'other'; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(class A { constructor() { this.nt = new.target === (${nt}); } }, [], ${nt}); return o.nt; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(Array, [3], ${nt}); return Array.isArray(o) + ':' + o.length; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(Date, [0], ${nt}); return typeof o; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(Error, ['m'], ${nt}); return o.message + ':' + (o instanceof Error); } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(Map, [], ${nt}); return o instanceof Map; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { const o = Reflect.construct(Promise, [() => {}], ${nt}); return o instanceof Promise; } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { return Reflect.construct(${nt}, []) !== undefined; } catch (e) { return e.name + ': ' + e.message; }`);
}
for (const tgt of ["function () {}", "() => {}", "Math.max", "({ m() {} }).m", "async function () {}", "function* () {}", "class A {}", "Symbol", "BigInt", "(function () {}).bind(null)", "parseInt", "Math.abs", "Function.prototype", "new Proxy(function () {}, {})", "new Proxy(() => {}, {})", "async () => {}", "Reflect.get", "Array.prototype.map", "Promise.resolve", "Object.prototype.toString"]) {
  t(`try { return typeof Reflect.construct(${tgt}, []); } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { return typeof Reflect.construct(function () {}, [], ${tgt}); } catch (e) { return e.name + ': ' + e.message; }`);
  t(`try { return typeof new (${tgt})(); } catch (e) { return e.name + ': ' + e.message; }`);
}
// receiver diferente: get/set/defineProperty.
t("const o = { get a() { return this.v; }, v: 1 }; return Reflect.get(o, 'a', { v: 2 });");
t("const o = { get a() { return this.v; }, v: 1 }; return Reflect.get(o, 'a');");
t("const o = { a: 1 }; return Reflect.get(o, 'a', { a: 2 });");
t("const o = { set a(v) { this.v = v; } }; const r = {}; Reflect.set(o, 'a', 5, r); return JSON.stringify([o, r]);");
t("const o = { a: 1 }; const r = {}; const res = Reflect.set(o, 'a', 5, r); return JSON.stringify([res, o, r]);");
t("const o = {}; const r = {}; const res = Reflect.set(o, 'a', 5, r); return JSON.stringify([res, o, r]);");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1, writable: false }); const r = {}; return Reflect.set(o, 'a', 5, r) + ':' + JSON.stringify(r);");
t("const o = {}; const r = {}; Object.defineProperty(r, 'a', { value: 1, writable: false }); return Reflect.set(o, 'a', 5, r) + ':' + r.a;");
t("const o = {}; const r = {}; Object.defineProperty(r, 'a', { get() { return 1; }, configurable: true }); return Reflect.set(o, 'a', 5, r);");
t("const o = {}; const r = {}; Object.defineProperty(r, 'a', { value: 1, writable: true, configurable: false }); return Reflect.set(o, 'a', 5, r) + ':' + r.a;");
t("const o = {}; const r = Object.freeze({}); return Reflect.set(o, 'a', 5, r);");
t("const o = {}; const r = Object.preventExtensions({}); return Reflect.set(o, 'a', 5, r);");
t("const o = { set a(v) { } }; return Reflect.set(o, 'a', 5, Object.freeze({}));");
t("const o = { get a() { return 1; } }; return Reflect.set(o, 'a', 5);");
t("const o = { get a() { return 1; } }; return Reflect.set(o, 'a', 5, {});");
t("const o = {}; const r = new Proxy({}, { defineProperty(t, k, d) { return Reflect.defineProperty(t, k, Object.assign({ x: 1 }, d)); }, getOwnPropertyDescriptor(t, k) { return Reflect.getOwnPropertyDescriptor(t, k); } }); Reflect.set(o, 'a', 5, r); return JSON.stringify(Reflect.getOwnPropertyDescriptor(r, 'a'));");
t("const log = []; const o = {}; const r = new Proxy({}, { defineProperty(t, k, d) { log.push('def:' + JSON.stringify(d)); return Reflect.defineProperty(t, k, d); }, getOwnPropertyDescriptor(t, k) { log.push('gopd'); return Reflect.getOwnPropertyDescriptor(t, k); } }); Reflect.set(o, 'a', 5, r); Reflect.set(o, 'a', 6, r); return log.join();");
t("const r = []; const o = Object.create({ set a(v) { r.push(this); } }); Reflect.set(o, 'a', 1, 'prim'); return typeof r[0];");
t("'use strict'; const r = []; const o = { set a(v) { 'use strict'; r.push(this); } }; Reflect.set(o, 'a', 1, 5); return r[0];");
t("return Reflect.set({}, 'a', 1, 5);");
t("return Reflect.set({}, 'a', 1, 'str');");
t("return Reflect.set({}, 'a', 1, Symbol());");
t("return Reflect.set({}, 'a', 1, 1n);");
t("return Reflect.set({}, 'a', 1, true);");
t("return Reflect.set({}, 'a', 1, null);");
t("return Reflect.set({}, 'a', 1, undefined);");
t("return Reflect.set([], 'length', 1, {});");
t("const a = [1, 2, 3]; const r = {}; return Reflect.set(a, 'length', 1, r) + ':' + a.length + ':' + JSON.stringify(r);");
t("const a = [1, 2, 3]; const r = [9, 9, 9]; return Reflect.set(a, 'length', 1, r) + ':' + a.length + ':' + r.length;");
t("const a = [1, 2, 3]; const r = [9]; return Reflect.set(a, 1, 'x', r) + ':' + JSON.stringify(a) + JSON.stringify(r);");
t("const a = []; Reflect.set(a, 0, 'x', a); return a.length;");
t("return Reflect.set(Object.freeze({ a: 1 }), 'a', 2);");
t("return Reflect.set(Object.freeze([1]), 0, 2) + ':' + Reflect.set(Object.freeze([1]), 'length', 0);");
t("return Reflect.set(Object.preventExtensions({}), 'a', 1);");
t("return Reflect.set('str'.constructor.prototype, 'x', 1) + ':' + String.prototype.x;");
t("return Reflect.set(Object.create(Object.freeze({ a: 1 })), 'a', 2);");
t("return Reflect.set(Object.create({ get a() { return 1; } }), 'a', 2);");
t("const o = Object.create({ set a(v) { this._a = v; } }); return Reflect.set(o, 'a', 2) + ':' + o._a;");
t("const o = {}; return Reflect.set(o, Symbol.iterator, 1) + ':' + (o[Symbol.iterator] === 1);");
t("const o = {}; return Reflect.set(o, 1.5, 1) + ':' + Object.keys(o);");
t("const o = {}; return Reflect.set(o, { toString() { return 'k'; } }, 1) + ':' + Object.keys(o);");
t("const o = {}; return Reflect.set(o, { toString() { throw new Error('ts'); } }, 1);");
t("return Reflect.get({ a: 1 }, { toString() { throw new Error('ts'); } });");
t("return Reflect.has({ a: 1 }, { toString() { return 'a'; } });");
t("return Reflect.get('abc', 0);");
t("return Reflect.get(Object('abc'), 1);");
t("return Reflect.get([1, 2], 'length');");
t("return Reflect.get({}, Symbol.toPrimitive);");
t("return Reflect.get(function () {}, 'name');");
t("return Reflect.get(Reflect, Symbol.toStringTag);");
t("return Object.prototype.toString.call(Reflect);");
t("return Reflect[Symbol.toStringTag] + ':' + Object.getOwnPropertyDescriptor(Reflect, Symbol.toStringTag).configurable;");
t("return Object.getOwnPropertyNames(Reflect).sort().join();");
t("return JSON.stringify(Object.getOwnPropertyDescriptor(Reflect, 'get'));");
t("return typeof Reflect + ':' + Object.getPrototypeOf(Reflect) === Object.prototype;");
t("try { Reflect(); } catch (e) { return e.name + ': ' + e.message; }");
t("try { new Reflect(); } catch (e) { return e.name + ': ' + e.message; }");
// defineProperty retornando false.
t("const o = Object.freeze({ a: 1 }); return Reflect.defineProperty(o, 'a', { value: 2 });");
t("const o = Object.freeze({ a: 1 }); return Reflect.defineProperty(o, 'a', { value: 1 });");
t("const o = Object.freeze({ a: 1 }); return Reflect.defineProperty(o, 'b', { value: 1 });");
t("const o = Object.preventExtensions({}); return Reflect.defineProperty(o, 'b', { value: 1 });");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1 }); return Reflect.defineProperty(o, 'a', { get() {} });");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1 }); return Reflect.defineProperty(o, 'a', { enumerable: true });");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1 }); return Reflect.defineProperty(o, 'a', { configurable: true });");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1 }); return Reflect.defineProperty(o, 'a', { writable: true });");
t("const o = {}; Object.defineProperty(o, 'a', { value: 1, writable: true }); return Reflect.defineProperty(o, 'a', { writable: false }) + ':' + Reflect.defineProperty(o, 'a', { writable: true });");
t("const o = {}; Object.defineProperty(o, 'a', { get() {} }); return Reflect.defineProperty(o, 'a', { get() {} });");
t("const g = function () {}; const o = {}; Object.defineProperty(o, 'a', { get: g }); return Reflect.defineProperty(o, 'a', { get: g });");
t("const o = []; Object.defineProperty(o, 'length', { writable: false }); return Reflect.defineProperty(o, 0, { value: 1 }) + ':' + o.length;");
t("const o = [1, 2]; Object.defineProperty(o, 1, { configurable: false }); return Reflect.defineProperty(o, 'length', { value: 0 }) + ':' + o.length;");
t("const o = []; return Reflect.defineProperty(o, 'length', { value: -1 });");
t("return Reflect.defineProperty([], 'length', { get() {} });");
t("return Reflect.defineProperty(function () {}, 'prototype', { value: 1 });");
t("return Reflect.defineProperty(function () {}, 'name', { value: 'x' });");
t("return Reflect.defineProperty(function () {}, 'length', { value: 1, writable: true });");
t("return Reflect.defineProperty(class {}, 'prototype', { value: {} });");
t("return Reflect.defineProperty(Math, 'PI', { value: 3 });");
t("return Reflect.defineProperty(Math, 'PI', { value: Math.PI });");
t("return Reflect.defineProperty(globalThis, 'NaN', { value: NaN }) + ':' + Reflect.defineProperty(globalThis, 'undefined', { value: 1 });");
t("return Reflect.defineProperty(new String('ab'), 0, { value: 'x' }) + ':' + Reflect.defineProperty(new String('ab'), 2, { value: 'x' });");
t("return Reflect.defineProperty(new String('ab'), 'length', { value: 5 });");
t("return Reflect.defineProperty(new Uint8Array(2), 0, { value: 5 }) + ':' + Reflect.defineProperty(new Uint8Array(2), 5, { value: 5 }) + ':' + Reflect.defineProperty(new Uint8Array(2), 0, { get() {} });");
t("return Reflect.defineProperty(new Uint8Array(2), 0, { value: 5, configurable: false }) + ':' + Reflect.defineProperty(new Uint8Array(2), 0, { value: 5, enumerable: false });");
t("return Reflect.defineProperty((function () { return arguments; })(1), 0, { value: 2 });");
t("const o = {}; return Reflect.defineProperty(o, 'a', { get() {}, value: 1 });");
t("const o = {}; return Reflect.defineProperty(o, 'a', { get: 1 });");
t("const o = {}; return Reflect.defineProperty(o, 'a', { set: 1 });");
t("const o = {}; return Reflect.defineProperty(o, 'a', { writable: 1, enumerable: '', configurable: 'x', value: 1 }) + JSON.stringify(Object.getOwnPropertyDescriptor(o, 'a'));");
t("const o = {}; return Reflect.defineProperty(o, 'a', Object.create({ value: 7 })) + ':' + o.a;");
t("const o = {}; return Reflect.defineProperty(o, 'a', new Proxy({}, { get(t, k) { return k === 'value' ? 9 : undefined; }, has(t, k) { return k === 'value'; } })) + ':' + o.a;");
t("const log = []; const o = {}; Reflect.defineProperty(o, 'a', new Proxy({ value: 1 }, { has(t, k) { log.push('has:' + k); return k in t; }, get(t, k) { log.push('get:' + k); return t[k]; } })); return log.join();");
t("const log = []; const o = {}; Object.defineProperty(o, 'a', new Proxy({ get() {}, enumerable: true }, { has(t, k) { log.push('has:' + k); return k in t; }, get(t, k) { log.push('get:' + k); return t[k]; } })); return log.join();");
t("return Reflect.defineProperty({}, Symbol(), { value: 1 });");
t("return Reflect.defineProperty({}, 1, { value: 1 });");
t("return Reflect.defineProperty({}, 'a', 1);");
t("return Reflect.defineProperty({}, 'a');");
t("return Reflect.defineProperty({});");
t("return Reflect.defineProperty({}, 'a', { value: 1 }, 5);");
// outros Reflect.
t("return Reflect.getOwnPropertyDescriptor({ get a() { return 1; } }, 'a').get.name;");
t("return JSON.stringify(Reflect.getOwnPropertyDescriptor([1], 'length'));");
t("return JSON.stringify(Reflect.getOwnPropertyDescriptor('ab', 0));");
t("return JSON.stringify(Reflect.getOwnPropertyDescriptor(Object('ab'), 0));");
t("return Reflect.getPrototypeOf(Object.create(null));");
t("return Reflect.getPrototypeOf(Object('s')) === String.prototype;");
t("return Reflect.setPrototypeOf(Object.preventExtensions({}), null) + ':' + Reflect.setPrototypeOf(Object.preventExtensions({}), Object.prototype);");
t("const a = {}; const b = Object.create(a); return Reflect.setPrototypeOf(a, b);");
t("const a = {}; return Reflect.setPrototypeOf(a, a);");
t("return Reflect.setPrototypeOf(Object.prototype, {}) + ':' + Reflect.setPrototypeOf(Object.prototype, null);");
t("return Reflect.setPrototypeOf(Object.prototype, Object.create(Object.prototype));");
t("return Reflect.setPrototypeOf(globalThis, null) + ':' + (Object.getPrototypeOf(globalThis) === null);");
t("return Reflect.setPrototypeOf({}, undefined);");
t("return Reflect.setPrototypeOf({}, 1);");
t("return Reflect.setPrototypeOf({}, function () {});");
t("return Reflect.setPrototypeOf(1, null);");
t("return Reflect.isExtensible(Object.freeze({})) + ':' + Reflect.isExtensible({});");
t("return Reflect.preventExtensions({}) + ':' + Reflect.preventExtensions(Object.freeze({}));");
t("return Reflect.ownKeys({ b: 1, a: 2, 1: 1, 0: 0, [Symbol.iterator]: 1, '-1': 1, '01': 1, 4294967295: 1, 4294967294: 1 }).map(String).join();");
t("return Reflect.ownKeys([1, 2]).join();");
t("return Reflect.ownKeys(function f(a) {}).join();");
t("return Reflect.ownKeys(class A { static x = 1; static m() {} }).join();");
t("return Reflect.ownKeys('ab'.constructor('xy') && Object('xy')).join();");
t("return Reflect.ownKeys(new Uint8Array(3)).join();");
t("return Reflect.ownKeys(new Error('x')).join();");
t("return Reflect.ownKeys(/a/g).join();");
t("return Reflect.ownKeys((function () { return arguments; })(1, 2)).map(String).join();");
t("return Reflect.deleteProperty(Object.freeze({ a: 1 }), 'a') + ':' + Reflect.deleteProperty(Object.freeze({}), 'a');");
t("return Reflect.deleteProperty([1, 2], 'length') + ':' + Reflect.deleteProperty([1, 2], 0);");
t("return Reflect.deleteProperty(function () {}, 'prototype') + ':' + Reflect.deleteProperty(function () {}, 'name');");
t("return Reflect.deleteProperty(globalThis, 'NaN') + ':' + Reflect.deleteProperty(new String('ab'), 0);");
t("return Reflect.has('abc'.constructor.prototype, 'slice');");
t("return Reflect.has([], 'length') + ':' + Reflect.has([], 0) + ':' + Reflect.has([, 1], 0);");
t("return Reflect.has(Object.create({ a: 1 }), 'a') + ':' + Reflect.has(Object.create(null), 'toString');");
t("return Reflect.get(Object.create({ get a() { return this; } }), 'a') !== undefined;");
t("return Reflect.apply(Reflect.get, null, [{ a: 1 }, 'a']);");
t("return Reflect.apply(Reflect.apply, null, [function () { return 1; }, null, []]);");
t("return Reflect.construct(Reflect.construct, [function () {}, []]) !== undefined;");
t("return Reflect.apply.call(null, function () { return 2; }, null, []);");
t("return Reflect.construct.call(null, function () { this.x = 3; }, []).x;");
t("return Reflect.ownKeys(Reflect).length;");

// ---- Execução: um bun por programa, resultado de `R` na saída marcada.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "reflect-golden-"));
const file = path.join(dir, "reflect_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const existing = new Set();
for (const program of knownPrograms("reflect_bun.tsv", ["proxy_class_bun.tsv"])) existing.add(JSON.stringify(program));
const seen = new Set();
let kept = 0;
let dropped = 0;
let covered = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
bodies.splice(0, bodies.length, ...bodies.filter((p) => !usesHostApi(p)));
for (const body of bodies) {
  if (seen.has(body)) continue;
  seen.add(body);
  // As combinações geram mais de cinco mil programas; fica uma amostra estável (hash do texto) de cerca de 2 em 7.
  let hash = 0;
  for (let i = 0; i < body.length; i++) hash = (hash * 131 + body.charCodeAt(i)) >>> 0;
  if (hash % 7 >= 2) continue;
  const original = prelude + body;
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  if (existing.has(JSON.stringify(original)) || existing.has(JSON.stringify(source))) {
    covered++;
    continue;
  }
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find((line) => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  // Mensagens com o texto do ponto de chamada ("evaluating '...'", "In 'p()'") são do renderizador de expressão, não do Proxy.
  if (/\(evaluating '|\(In '/.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, já cobertos ${covered}\n`);
process.stdout.write(emitFactoredLines("reflect", lines));
fs.rmSync(dir, { recursive: true, force: true });
