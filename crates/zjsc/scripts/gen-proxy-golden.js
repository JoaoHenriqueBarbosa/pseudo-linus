// Gera tests/golden/proxy_bun.tsv: Proxy e Reflect (cada trap, invariantes, revogação, ordem das traps, with,
// class extends) medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// O arquivo se chama `proxy_case.js` dos dois lados.
// Uso: bun scripts/gen-proxy-golden.js > tests/golden/proxy_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE = `function fmt(v) {
  if (typeof v === "symbol") return v.toString();
  if (typeof v === "function") return "[function]";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "bigint") return v + "n";
  if (v === undefined) return "undefined";
  if (typeof v === "object" && v !== null) { try { return JSON.stringify(v, (k, x) => typeof x === "symbol" ? x.toString() : x === undefined ? "<u>" : typeof x === "function" ? "[function]" : x); } catch (e) { return "[object]"; } }
  return String(v);
}
function run(f) { try { return fmt(f()); } catch (e) { return e.name + ": " + e.message; } }
var L = [];
`;

const programs = [];
const add = body => programs.push(PRELUDE + body);

// ---- Cada trap: operações x variações de handler.
const traps = {
  get: {
    target: "{a: 1, b: 2}",
    normal: "(t, k, r) => 'got:' + String(k)",
    ops: ["p.a", "p[1]", "p[Symbol.iterator]", "Reflect.get(p, 'z', {w: 1})", "p.hasOwnProperty", "p['a b']"],
  },
  set: {
    target: "{a: 1, b: 2}",
    normal: "(t, k, v, r) => { L.push(String(k) + '=' + v); return true; }",
    ops: ["p.a = 5", "p[0] = 7", "Reflect.set(p, 'q', 1, {})", "(p.x = 1, p.y = 2, 3)"],
  },
  has: {
    target: "{a: 1}",
    normal: "(t, k) => { L.push(String(k)); return true; }",
    ops: ["'a' in p", "'zzz' in p", "Reflect.has(p, Symbol.iterator)", "0 in p"],
  },
  deleteProperty: {
    target: "{a: 1, b: 2}",
    normal: "(t, k) => { L.push(String(k)); return true; }",
    ops: ["delete p.a", "delete p.zz", "Reflect.deleteProperty(p, 'b')", "delete p[0]"],
  },
  defineProperty: {
    target: "{a: 1}",
    normal: "(t, k, d) => { L.push(k + ':' + fmt(d)); return true; }",
    ops: [
      "Object.defineProperty(p, 'x', {value: 1})",
      "Reflect.defineProperty(p, 'y', {get() {}, enumerable: true})",
      "Object.defineProperties(p, {m: {value: 1}, n: {value: 2}})",
      "(p.a = 9, 1)",
    ],
  },
  getOwnPropertyDescriptor: {
    target: "{a: 1}",
    normal: "(t, k) => ({value: 5, configurable: true, enumerable: true, writable: true})",
    ops: [
      "Object.getOwnPropertyDescriptor(p, 'a')",
      "Object.getOwnPropertyDescriptor(p, 'zz')",
      "p.hasOwnProperty('a')",
      "Object.prototype.propertyIsEnumerable.call(p, 'a')",
    ],
  },
  ownKeys: {
    target: "{a: 1, b: 2}",
    normal: "() => ['x', 'y']",
    ops: [
      "Object.keys(p)",
      "Object.getOwnPropertyNames(p)",
      "Reflect.ownKeys(p)",
      "Object.getOwnPropertySymbols(p)",
      "JSON.stringify(p)",
      "(() => { var r = []; for (var k in p) r.push(k); return r; })()",
    ],
  },
  getPrototypeOf: {
    target: "{a: 1}",
    normal: "() => Array.prototype",
    ops: [
      "Object.getPrototypeOf(p) === Array.prototype",
      "p instanceof Array",
      "Reflect.getPrototypeOf(p) === Array.prototype",
      "p.__proto__ === Array.prototype",
      "Array.prototype.isPrototypeOf(p)",
    ],
  },
  setPrototypeOf: {
    target: "{a: 1}",
    normal: "(t, pr) => { L.push(fmt(pr === null)); return true; }",
    ops: [
      "Object.setPrototypeOf(p, null) === p",
      "Reflect.setPrototypeOf(p, Array.prototype)",
      "(p.__proto__ = {}, 1)",
    ],
  },
  isExtensible: {
    target: "{a: 1}",
    normal: "t => Reflect.isExtensible(t)",
    ops: ["Object.isExtensible(p)", "Reflect.isExtensible(p)", "Object.isFrozen(p)", "Object.isSealed(p)"],
  },
  preventExtensions: {
    target: "{a: 1}",
    normal: "t => { Object.preventExtensions(t); return true; }",
    ops: ["Object.preventExtensions(p) === p", "Reflect.preventExtensions(p)", "Object.freeze(p) === p", "Object.seal(p) === p"],
  },
  apply: {
    target: "function (a, b) { return 'target'; }",
    normal: "(t, th, args) => 'applied:' + args.length",
    ops: ["p()", "p(1, 2, 3)", "p.call({}, 1)", "p.apply(null, [1, 2])", "Reflect.apply(p, 1, [])"],
  },
  construct: {
    target: "function () { this.t = 1; }",
    normal: "(t, args, nt) => ({n: args.length, same: nt === p})",
    ops: ["new p()", "new p(1, 2)", "Reflect.construct(p, [1], Array)", "Reflect.construct(p, [])"],
  },
};
const variants = {
  normal: null,
  returnsFalse: "() => false",
  returnsUndefined: "() => undefined",
  throwsError: "() => { throw new RangeError('boom'); }",
  throwsPrimitive: "() => { throw 42; }",
  notFunctionNumber: "1",
  notFunctionObject: "({})",
  notFunctionString: "'str'",
  isNull: "null",
  isUndefined: "undefined",
  getterThrows: "GETTER",
};
for (const [name, info] of Object.entries(traps)) {
  for (const op of info.ops) {
    for (const [vname, vexpr] of Object.entries(variants)) {
      let handler;
      if (vname === "getterThrows") {
        handler = `Object.defineProperty({}, '${name}', {get() { throw new SyntaxError('getter'); }})`;
      } else {
        const impl = vname === "normal" ? info.normal : vexpr;
        handler = `{${name}: ${impl}}`;
      }
      add(`var p = new Proxy(${info.target}, ${handler});\nglobalThis.R = run(() => { "use strict"; return ${op}; }) + " | " + fmt(L);\n`);
    }
  }
}

// ---- Violações de invariante.
const inv = [
  // get
  ["get", "Object.defineProperty({}, 'a', {value: 1})", "() => 2", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {value: 1})", "() => 1", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {value: NaN})", "() => NaN", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {value: 0})", "() => -0", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {value: 1, writable: true})", "() => 2", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {value: 1, configurable: true})", "() => 2", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {set(v) {}})", "() => 2", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {set(v) {}})", "() => undefined", "p.a"],
  ["get", "Object.defineProperty({}, 'a', {get() { return 1; }})", "() => 2", "p.a"],
  // set
  ["set", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "(p.a = 2, 1)"],
  ["set", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "(p.a = 1, 1)"],
  ["set", "Object.defineProperty({}, 'a', {value: 1})", "() => false", "(p.a = 2, 1)"],
  ["set", "Object.defineProperty({}, 'a', {get() {}})", "() => true", "(p.a = 2, 1)"],
  ["set", "Object.defineProperty({}, 'a', {get() {}, set(v) {}})", "() => true", "(p.a = 2, 1)"],
  ["set", "Object.defineProperty({}, 'a', {value: 1, writable: true})", "() => true", "(p.a = 2, 1)"],
  ["set", "{a: 1}", "() => false", "(p.a = 2, 1)"],
  ["set", "{a: 1}", "() => 0", "(p.a = 2, 1)"],
  ["set", "{a: 1}", "() => 'yes'", "Reflect.set(p, 'a', 2)"],
  // has
  ["has", "Object.defineProperty({}, 'a', {value: 1})", "() => false", "'a' in p"],
  ["has", "Object.defineProperty({}, 'a', {value: 1, configurable: true})", "() => false", "'a' in p"],
  ["has", "Object.preventExtensions({a: 1})", "() => false", "'a' in p"],
  ["has", "Object.preventExtensions({a: 1})", "() => true", "'a' in p"],
  ["has", "Object.preventExtensions({a: 1})", "() => false", "'b' in p"],
  ["has", "Object.preventExtensions({})", "() => true", "'b' in p"],
  ["has", "{}", "() => 1", "'b' in p"],
  ["has", "{}", "() => ''", "'b' in p"],
  // deleteProperty
  ["deleteProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "delete p.a"],
  ["deleteProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => false", "delete p.a"],
  ["deleteProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => false", "Reflect.deleteProperty(p, 'a')"],
  ["deleteProperty", "Object.defineProperty({}, 'a', {value: 1, configurable: true})", "() => true", "delete p.a"],
  ["deleteProperty", "Object.preventExtensions({a: 1})", "() => true", "delete p.a"],
  ["deleteProperty", "Object.preventExtensions({a: 1})", "() => true", "delete p.b"],
  ["deleteProperty", "{a: 1}", "() => false", "delete p.a"],
  // defineProperty
  ["defineProperty", "Object.preventExtensions({})", "() => true", "Object.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "Object.preventExtensions({})", "() => true", "Reflect.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "{}", "() => true", "Object.defineProperty(p, 'a', {value: 1, configurable: false})"],
  ["defineProperty", "{}", "() => true", "Object.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "{a: 1}", "() => true", "Object.defineProperty(p, 'a', {value: 1, configurable: false})"],
  ["defineProperty", "{a: 1}", "() => true", "Object.defineProperty(p, 'a', {value: 1, configurable: true})"],
  ["defineProperty", "Object.defineProperty({}, 'a', {value: 1, writable: true})", "() => true", "Object.defineProperty(p, 'a', {value: 1, writable: false, configurable: false})"],
  ["defineProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "Object.defineProperty(p, 'a', {value: 2})"],
  ["defineProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "Object.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "Object.defineProperty({}, 'a', {value: 1})", "() => true", "Object.defineProperty(p, 'a', {get() {}})"],
  ["defineProperty", "{a: 1}", "() => false", "Object.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "{a: 1}", "() => false", "Reflect.defineProperty(p, 'a', {value: 1})"],
  ["defineProperty", "{a: 1}", "() => undefined", "Reflect.defineProperty(p, 'a', {value: 1})"],
  // getOwnPropertyDescriptor
  ["getOwnPropertyDescriptor", "{a: 1}", "() => 1", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => 'x'", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => null", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => true", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => Symbol()", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.defineProperty({}, 'a', {value: 1})", "() => undefined", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.preventExtensions({a: 1})", "() => undefined", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.preventExtensions({})", "() => ({value: 1, configurable: true})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{}", "() => ({value: 1, configurable: false})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => ({value: 1, configurable: false})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.defineProperty({}, 'a', {value: 1})", "() => ({value: 2, configurable: false})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.defineProperty({}, 'a', {value: 1, writable: true})", "() => ({value: 1, configurable: false, writable: false})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "Object.defineProperty({}, 'a', {value: 1})", "() => ({value: 1, configurable: false, writable: false})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => ({get: 1})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => ({get() {}, value: 1})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => ({configurable: true})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => ({})", "Object.getOwnPropertyDescriptor(p, 'a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => 1", "p.hasOwnProperty('a')"],
  ["getOwnPropertyDescriptor", "{a: 1}", "() => 1", "Object.getOwnPropertyDescriptors(p)"],
  // ownKeys
  ["ownKeys", "{}", "() => 1", "Object.keys(p)"],
  ["ownKeys", "{}", "() => 'ab'", "Object.keys(p)"],
  ["ownKeys", "{}", "() => null", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => undefined", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => [1]", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => [{}]", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => [null]", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => ['a', 'a']", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => { var s = Symbol(); return [s, s]; }", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => ({length: 1, 0: 'a'})", "Reflect.ownKeys(p)"],
  ["ownKeys", "{}", "() => ({length: 2, 0: 'a', 1: 'b'})", "Object.keys(p)"],
  ["ownKeys", "{}", "() => new Set(['a'])", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.defineProperty({}, 'a', {value: 1})", "() => []", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.defineProperty({}, 'a', {value: 1})", "() => ['b']", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.defineProperty({}, 'a', {value: 1, configurable: true})", "() => []", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.preventExtensions({a: 1})", "() => []", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.preventExtensions({a: 1})", "() => ['a', 'b']", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.preventExtensions({a: 1})", "() => ['a']", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.preventExtensions({a: 1})", "() => ['b']", "Reflect.ownKeys(p)"],
  ["ownKeys", "Object.freeze({a: 1, [Symbol.for('s')]: 2})", "() => ['a']", "Reflect.ownKeys(p)"],
  ["ownKeys", "{a: 1}", "() => ['a', 'b']", "Object.getOwnPropertyNames(p)"],
  ["ownKeys", "{a: 1}", "() => ['b', Symbol.iterator, 'a']", "Reflect.ownKeys(p)"],
  ["ownKeys", "{a: 1}", "() => ['b', Symbol.iterator, 'a']", "Object.getOwnPropertySymbols(p)"],
  ["ownKeys", "{a: 1}", "() => ['b']", "JSON.stringify(p)"],
  // getPrototypeOf
  ["getPrototypeOf", "{}", "() => 1", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "{}", "() => 'x'", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "{}", "() => undefined", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "{}", "() => true", "Reflect.getPrototypeOf(p)"],
  ["getPrototypeOf", "{}", "() => Symbol()", "p.__proto__"],
  ["getPrototypeOf", "{}", "() => 1", "p instanceof Object"],
  ["getPrototypeOf", "{}", "() => null", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "{}", "() => function () {}", "typeof Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "Object.preventExtensions({})", "() => Array.prototype", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "Object.preventExtensions({})", "() => Object.prototype", "Object.getPrototypeOf(p) === Object.prototype"],
  ["getPrototypeOf", "Object.preventExtensions(Object.create(null))", "() => null", "Object.getPrototypeOf(p)"],
  ["getPrototypeOf", "Object.preventExtensions(Object.create(null))", "() => ({})", "Object.getPrototypeOf(p)"],
  // setPrototypeOf
  ["setPrototypeOf", "Object.preventExtensions({})", "() => true", "Object.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "Object.preventExtensions({})", "() => true", "Object.setPrototypeOf(p, Object.prototype) === p"],
  ["setPrototypeOf", "Object.preventExtensions({})", "() => false", "Reflect.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "Object.preventExtensions({})", "() => false", "Object.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "{}", "() => false", "Object.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "{}", "() => false", "Reflect.setPrototypeOf(p, null)"],
  ["setPrototypeOf", "{}", "() => false", "(p.__proto__ = null, 1)"],
  ["setPrototypeOf", "{}", "() => 1", "Reflect.setPrototypeOf(p, {})"],
  ["setPrototypeOf", "{}", "() => true", "Object.setPrototypeOf(p, 1)"],
  ["setPrototypeOf", "{}", "() => true", "Object.setPrototypeOf(p)"],
  ["setPrototypeOf", "{}", "() => true", "Reflect.setPrototypeOf(p, 1)"],
  ["setPrototypeOf", "{}", "() => true", "Reflect.setPrototypeOf(p, undefined)"],
  // isExtensible
  ["isExtensible", "{}", "() => false", "Object.isExtensible(p)"],
  ["isExtensible", "Object.preventExtensions({})", "() => true", "Object.isExtensible(p)"],
  ["isExtensible", "{}", "() => true", "Object.isExtensible(p)"],
  ["isExtensible", "Object.preventExtensions({})", "() => false", "Reflect.isExtensible(p)"],
  ["isExtensible", "{}", "() => 1", "Object.isExtensible(p)"],
  ["isExtensible", "{}", "() => 0", "Reflect.isExtensible(p)"],
  ["isExtensible", "{}", "() => false", "Object.isFrozen(p)"],
  ["isExtensible", "{}", "() => false", "Object.isSealed(p)"],
  // preventExtensions
  ["preventExtensions", "{}", "() => true", "Object.preventExtensions(p)"],
  ["preventExtensions", "{}", "() => true", "Reflect.preventExtensions(p)"],
  ["preventExtensions", "{}", "() => false", "Object.preventExtensions(p)"],
  ["preventExtensions", "{}", "() => false", "Reflect.preventExtensions(p)"],
  ["preventExtensions", "{}", "() => 1", "Reflect.preventExtensions(p)"],
  ["preventExtensions", "{}", "() => true", "Object.freeze(p)"],
  ["preventExtensions", "{}", "() => true", "Object.seal(p)"],
  ["preventExtensions", "Object.preventExtensions({})", "() => true", "Reflect.preventExtensions(p)"],
  ["preventExtensions", "Object.preventExtensions({})", "() => false", "Reflect.preventExtensions(p)"],
  // construct
  ["construct", "function () {}", "() => 1", "new p()"],
  ["construct", "function () {}", "() => undefined", "new p()"],
  ["construct", "function () {}", "() => null", "new p()"],
  ["construct", "function () {}", "() => 'x'", "new p()"],
  ["construct", "function () {}", "() => Symbol()", "new p()"],
  ["construct", "function () {}", "() => ({ok: 1})", "new p()"],
  ["construct", "function () {}", "() => function () {}", "typeof new p()"],
  ["construct", "function () {}", "() => []", "Reflect.construct(p, [])"],
  ["construct", "function () {}", "() => 1", "Reflect.construct(p, [])"],
  ["construct", "function () {}", "(t, a, nt) => nt", "new p() === p"],
  ["construct", "function () {}", "(t, a, nt) => { L.push(nt === Array); return {}; }", "Reflect.construct(p, [], Array)"],
  ["construct", "function () {}", "(t, a) => { L.push(Array.isArray(a)); return {}; }", "new p(1, 2)"],
  ["construct", "function () {}", "(t, a) => { L.push(a.length); return {}; }", "Reflect.construct(p, {length: 2})"],
  ["construct", "() => {}", "() => ({})", "new p()"],
  ["construct", "async function f() {}", "() => ({})", "new p()"],
  ["construct", "function* g() {}", "() => ({})", "new p()"],
  ["construct", "({m() {}}).m", "() => ({})", "new p()"],
  ["construct", "class A {}", "() => ({})", "p()"],
  ["construct", "class A {}", "() => ({})", "new p() instanceof Object"],
  ["construct", "Math.max", "() => ({})", "new p()"],
  ["construct", "Symbol", "() => ({})", "new p()"],
  ["construct", "Date", "() => ({})", "p() === Date()"],
  ["construct", "{}", "() => ({})", "new p()"],
  // apply
  ["apply", "{}", "() => 1", "p()"],
  ["apply", "() => 1", "() => 2", "p()"],
  ["apply", "() => 1", "(t, th, a) => [th, a.length]", "p.call(5, 1, 2)"],
  ["apply", "() => 1", "(t, th, a) => Array.isArray(a)", "p(1)"],
  ["apply", "() => 1", "(t, th, a) => th", "p.call(undefined)"],
  ["apply", "() => 1", "(t, th, a) => typeof th", "p.call('s')"],
  ["apply", "() => 1", "(t, th, a) => typeof th", "(function () { return p(); })()"],
  ["apply", "() => 1", "(t, th, a) => typeof th", "({m: p}).m()"],
  ["apply", "() => 1", "(t, th, a) => a", "Reflect.apply(p, 1, 'x')"],
  ["apply", "() => 1", "(t, th, a) => a", "Reflect.apply(p, 1)"],
  ["apply", "() => 1", "(t, th, a) => a.length", "Reflect.apply(p, 1, {length: 3})"],
  ["apply", "function () {}", "Reflect.apply", "p.call(function () { return 7; })"],
  ["apply", "class A {}", "() => 1", "p()"],
  ["apply", "class A {}", "Reflect.apply", "p()"],
  ["apply", "function () {}", "() => 1", "new p()"],
];
for (const [trap, target, impl, op] of inv) {
  add(`var p = new Proxy(${target}, {${trap}: ${impl}});\nglobalThis.R = run(() => { "use strict"; return ${op}; }) + " | " + fmt(L);\n`);
}

// ---- Construtor, Proxy.revocable e revogação.
const ctor = [
  "typeof Proxy",
  "Proxy.length",
  "Proxy.name",
  "Proxy.prototype",
  "Object.getOwnPropertyNames(Proxy).sort().join()",
  "Object.getOwnPropertyDescriptor(Proxy, 'revocable')",
  "Object.getOwnPropertyDescriptor(Proxy, 'prototype')",
  "Object.getPrototypeOf(Proxy) === Function.prototype",
  "Proxy.revocable.length",
  "Proxy.revocable.name",
  "Proxy()",
  "Proxy({}, {})",
  "new Proxy()",
  "new Proxy({})",
  "new Proxy({}, undefined)",
  "new Proxy({}, null)",
  "new Proxy(null, {})",
  "new Proxy(undefined, {})",
  "new Proxy(1, {})",
  "new Proxy('s', {})",
  "new Proxy(Symbol(), {})",
  "new Proxy(1n, {})",
  "new Proxy({}, 1)",
  "new Proxy({}, 'x')",
  "new Proxy({}, function () {})",
  "typeof new Proxy({}, function () {})",
  "new Proxy(function () {}, [])",
  "new Proxy([], new Proxy({}, {}))",
  "Proxy.revocable()",
  "Proxy.revocable({})",
  "Proxy.revocable(1, {})",
  "Proxy.revocable({}, 1)",
  "new Proxy.revocable({}, {})",
  "Object.keys(Proxy.revocable({}, {})).join()",
  "Object.keys(Proxy.revocable({}, {})).length",
  "typeof Proxy.revocable({}, {}).revoke",
  "Proxy.revocable({}, {}).revoke.length",
  "Proxy.revocable({}, {}).revoke.name",
  "Object.getOwnPropertyNames(Proxy.revocable({}, {}).revoke).sort().join()",
  "Proxy.revocable({}, {}).revoke()",
  "(r => (r.revoke(), r.revoke()))(Proxy.revocable({}, {}))",
  "(r => (r.revoke(), r.revoke === r.revoke))(Proxy.revocable({}, {}))",
  "(r => new r.revoke())(Proxy.revocable({}, {}))",
  "Object.getPrototypeOf(Proxy.revocable({}, {})) === Object.prototype",
  "(r => { r.revoke.call(null); return typeof r.proxy; })(Proxy.revocable({}, {}))",
  "(r => { var f = r.revoke; f(); return Object.keys(r.proxy); })(Proxy.revocable({}, {}))",
  "new Proxy({}, {}) instanceof Object",
  "new Proxy({}, {}) instanceof Proxy",
  "Object.prototype.hasOwnProperty.call(new Proxy({}, {}), 'prototype')",
  "new Proxy(Proxy, {}).name",
  "new (new Proxy(Proxy, {}))({}, {}) instanceof Object",
  "Reflect.construct(Proxy, [{}, {}]) instanceof Object",
  "Reflect.construct(Proxy, [{}, {}], Array) instanceof Array",
  "Reflect.apply(Proxy, null, [{}, {}])",
  "Proxy.call(null, {}, {})",
  "Proxy.apply(null, [{}, {}])",
  "new Proxy({a: 1}, {}).a",
  "String(new Proxy({}, {}))",
  "String(new Proxy([], {}))",
  "String(new Proxy(function f() {}, {}))",
  "Function.prototype.toString.call(new Proxy(function f() {}, {}))",
  "Function.prototype.toString.call(new Proxy(class A {}, {}))",
  "Function.prototype.toString.call(new Proxy({}, {}))",
  "Function.prototype.toString.call(new Proxy(() => 1, {}))",
  "new Proxy(function f(a, b) {}, {}).length",
  "new Proxy(function f(a, b) {}, {}).name",
  "new Proxy(class Foo {}, {}).name",
  "typeof new Proxy(function () {}, {})",
  "typeof new Proxy(class {}, {})",
  "typeof new Proxy({}, {})",
  "typeof new Proxy([], {})",
  "typeof new Proxy(() => 1, {})",
  "typeof new Proxy(new Proxy(function () {}, {}), {})",
  "typeof new Proxy(new Proxy({}, {}), {})",
  "typeof new Proxy(async function () {}, {})",
  "typeof new Proxy(function* () {}, {})",
  "typeof new Proxy(Math.max, {})",
  "typeof new Proxy(Symbol, {})",
  "typeof new Proxy(Object.assign(() => 1, {x: 1}), {})",
];
for (const body of ctor) add(`globalThis.R = run(() => ${body});\n`);

// Operações sobre proxy revogado.
const revokedOps = [
  "p.a", "p.a = 1", "'a' in p", "delete p.a", "Object.keys(p)", "Reflect.ownKeys(p)", "Object.getOwnPropertyNames(p)",
  "Object.getOwnPropertyDescriptor(p, 'a')", "Object.defineProperty(p, 'a', {value: 1})", "Object.getPrototypeOf(p)",
  "Object.setPrototypeOf(p, null)", "Object.isExtensible(p)", "Object.preventExtensions(p)", "Object.freeze(p)",
  "Object.isFrozen(p)", "JSON.stringify(p)", "String(p)", "p + ''", "typeof p", "p instanceof Object", "Object.prototype.toString.call(p)",
  "Array.isArray(p)", "[...p]", "({...p})", "Object.assign({}, p)", "for (var k in p) {}", "p.hasOwnProperty('a')",
  "Reflect.get(p, 'a')", "Reflect.has(p, 'a')", "Reflect.getPrototypeOf(p)", "Object.entries(p)", "Object.values(p)",
  "Object.getOwnPropertyDescriptors(p)", "Object.prototype.isPrototypeOf.call(p, {})", "Object.create(p)", "new Proxy(p, {})",
  "Proxy.revocable(p, {}).proxy === undefined", "Object.is(p, p)", "p === p", "[p].includes(p)", "Function.prototype.toString.call(p)",
  "Symbol.keyFor(p)", "Math.max === undefined", "Object.prototype.propertyIsEnumerable.call(p, 'a')",
];
for (const targetExpr of ["{a: 1}", "[1, 2]", "function () {}"]) {
  for (const op of revokedOps) {
    add(`var r = Proxy.revocable(${targetExpr}, {});\nvar p = r.proxy;\nr.revoke();\nglobalThis.R = run(() => { "use strict"; return ${op}; });\n`);
  }
}
for (const op of ["p()", "new p()", "p.call(1)", "Reflect.apply(p, 1, [])", "Reflect.construct(p, [])", "typeof p", "Function.prototype.call.call(p)", "p.bind", "Array.isArray(p)", "class X extends p {}"]) {
  add(`var r = Proxy.revocable(function () {}, {});\nvar p = r.proxy;\nr.revoke();\nglobalThis.R = run(() => { "use strict"; return ${op}; });\n`);
}
// Revogar dentro de uma trap.
for (const [trap, op] of [
  ["get", "p.a"], ["set", "(p.a = 1, 2)"], ["has", "'a' in p"], ["deleteProperty", "delete p.a"], ["ownKeys", "Object.keys(p)"],
  ["getOwnPropertyDescriptor", "Object.getOwnPropertyDescriptor(p, 'a')"], ["getPrototypeOf", "Object.getPrototypeOf(p)"],
]) {
  add(`var r = Proxy.revocable({a: 1}, {${trap}(t) { r.revoke(); return Reflect[${JSON.stringify(trap === "ownKeys" ? "ownKeys" : trap)}].apply(null, arguments); }});\nvar p = r.proxy;\nglobalThis.R = run(() => ${op}) + " | " + run(() => p.a);\n`);
}
// Handler revogado entre as traps (o handler do proxy não é o proxy revogado).
add(`var r = Proxy.revocable({}, {});\nvar p = new Proxy({a: 1}, r.proxy);\nr.revoke();\nglobalThis.R = run(() => p.a) + " | " + run(() => 'a' in p) + " | " + run(() => Object.keys(p));\n`);
add(`var r = Proxy.revocable({}, {});\nr.revoke();\nglobalThis.R = run(() => new Proxy(r.proxy, {})) + " | " + run(() => new Proxy({}, r.proxy));\n`);
add(`var r = Proxy.revocable({}, {});\nr.revoke();\nglobalThis.R = run(() => Proxy.revocable(r.proxy, {}).proxy === undefined) + " | " + run(() => Proxy.revocable({}, r.proxy).proxy === undefined);\n`);

// ---- Proxy de proxy, de função, de array, de classe.
const nestedOps = [
  "p.a", "p.a = 2", "'a' in p", "delete p.a", "Object.keys(p)", "Reflect.ownKeys(p)", "Object.getOwnPropertyDescriptor(p, 'a')",
  "Object.getPrototypeOf(p) === Object.prototype", "Object.isExtensible(p)", "JSON.stringify(p)", "Array.isArray(p)",
  "Object.prototype.toString.call(p)", "typeof p",
];
for (const op of nestedOps) {
  add(`var p = new Proxy(new Proxy({a: 1}, {get(t, k, r) { L.push('inner.get'); return Reflect.get(t, k, r); }, has(t, k) { L.push('inner.has'); return Reflect.has(t, k); }, deleteProperty(t, k) { L.push('inner.del'); return Reflect.deleteProperty(t, k); }, ownKeys(t) { L.push('inner.keys'); return Reflect.ownKeys(t); }, set(t, k, v, r) { L.push('inner.set'); return Reflect.set(t, k, v, r); }, getOwnPropertyDescriptor(t, k) { L.push('inner.gopd'); return Reflect.getOwnPropertyDescriptor(t, k); }}), {get(t, k, r) { L.push('outer.get'); return Reflect.get(t, k, r); }, has(t, k) { L.push('outer.has'); return Reflect.has(t, k); }, deleteProperty(t, k) { L.push('outer.del'); return Reflect.deleteProperty(t, k); }, ownKeys(t) { L.push('outer.keys'); return Reflect.ownKeys(t); }, set(t, k, v, r) { L.push('outer.set'); return Reflect.set(t, k, v, r); }, getOwnPropertyDescriptor(t, k) { L.push('outer.gopd'); return Reflect.getOwnPropertyDescriptor(t, k); }});\nglobalThis.R = run(() => { "use strict"; return ${op}; }) + " | " + fmt(L);\n`);
}
for (const depth of [2, 3, 5, 10]) {
  for (const op of ["p.a", "p()", "new p()", "typeof p", "Array.isArray(p)", "Object.keys(p)"]) {
    const target = op === "p()" || op === "new p()" ? "function () { return {a: 1}; }" : op === "Array.isArray(p)" ? "[1]" : "{a: 1}";
    add(`var p = ${target};\nfor (var i = 0; i < ${depth}; i++) p = new Proxy(p, {});\nglobalThis.R = run(() => ${op});\n`);
  }
}
// Proxy cíclico via protótipo.
add(`var p = new Proxy({}, {});\nglobalThis.R = run(() => Object.setPrototypeOf(p, p)) + " | " + run(() => { var q = Object.create(p); return Object.setPrototypeOf(p, q); });\n`);
add(`var o = {}; var p = new Proxy(o, {});\nglobalThis.R = run(() => Object.setPrototypeOf(o, p)) + " | " + run(() => o.zz);\n`);
add(`var o = {}; var p = new Proxy(o, {});\nglobalThis.R = run(() => (Object.setPrototypeOf(p, p), 1));\n`);
add(`var o = {}; var p = new Proxy({}, {setPrototypeOf() { return true; }});\nglobalThis.R = run(() => Object.setPrototypeOf(o, p)) + " | " + run(() => Object.setPrototypeOf(p, o));\n`);

// Proxy de array.
const arrOps = [
  "Array.isArray(p)", "p.length", "p.push(4)", "p.pop()", "p.slice(1)", "p.map(x => x * 2)", "p.includes(2)", "p.indexOf(3)",
  "p.join('-')", "p.concat([9])", "[].concat(p)", "[...p]", "Array.from(p)", "p.reverse()", "p.sort((a, b) => b - a)",
  "p.splice(1, 1)", "p.shift()", "p.unshift(0)", "p.fill(7)", "p.length = 1", "p[5] = 1", "JSON.stringify(p)", "Object.keys(p)",
  "p.forEach(x => x)", "p.filter(x => x > 1)", "p.find(x => x > 1)", "p.flat()", "p.at(-1)", "p.toString()", "Object.prototype.toString.call(p)",
  "p instanceof Array", "p.constructor === Array", "Array.prototype.slice.call(p)", "Math.max(...p)", "for (var x of p) {}", "p.entries().next().value",
  "p.findLast(x => true)", "p.reduce((a, b) => a + b)", "Array.of.call(function () { return p; })",
];
for (const op of arrOps) {
  add(`var log = [];\nvar p = new Proxy([1, 2, 3], new Proxy({}, {get(t, k) { return (tt, kk, ...rest) => { log.push(k + ':' + (typeof kk === 'symbol' ? kk.toString() : kk)); return Reflect[k](tt, kk, ...rest); }; }}));\nglobalThis.R = run(() => ${op}) + " | " + log.join(',');\n`);
}
// Proxy de função / classe.
const fnOps = [
  "p(2)", "new p(2)", "p.name", "p.length", "p.prototype === F.prototype", "p.call(null, 3)", "p.bind(null, 1)(2)", "p.apply(null, [4])",
  "typeof p", "p instanceof F", "new p(1) instanceof F", "Object.getPrototypeOf(p) === Function.prototype", "p.toString === Function.prototype.toString",
  "Function.prototype.call.call(p, null, 5)", "Reflect.construct(p, [1], Object)", "Reflect.construct(p, [1], p) instanceof F", "p.hasOwnProperty('prototype')",
  "Object.getOwnPropertyNames(p).sort().join()", "class B extends p {}", "(class B extends p { constructor() { super(); } }, 1)",
  "new (class B extends p { constructor() { super(5); this.z = 1; } })().z", "new (class B extends p {})() instanceof F",
  "Object.getPrototypeOf(class B extends p {}) === p", "(class B extends p {}).__proto__ === p",
  "Math.max.call === Function.prototype.call", "[1, 2].map(p)", "[1, 2].map(p).length", "p.caller", "p.arguments",
];
for (const op of fnOps) {
  add(`function F(x) { this.x = x; return undefined; }\nvar p = new Proxy(F, {});\nglobalThis.R = run(() => ${op});\n`);
  add(`class F { constructor(x) { this.x = x; } static s() { return 's'; } }\nvar p = new Proxy(F, {});\nglobalThis.R = run(() => ${op});\n`);
}
for (const op of ["p.s()", "p.s === F.s", "Object.getOwnPropertyNames(p).sort().join()", "new p(4).x", "p(4)", "new (class B extends p {})(7).x"]) {
  add(`class F { constructor(x) { this.x = x; } static s() { return 's'; } }\nvar p = new Proxy(F, {construct(t, a, nt) { L.push('c'); return Reflect.construct(t, a, nt); }, get(t, k, r) { L.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = run(() => ${op}) + " | " + fmt(L);\n`);
}

// ---- Ordem de chamada das traps (handler que é um proxy registra cada consulta).
const logHandler = `var log = [];
var rec = new Proxy({}, {get(t, k) { log.push('h.' + String(k)); return (tt, ...rest) => { return Reflect[k](tt, ...rest); }; }});
`;
const orderTargets = ["{a: 1, b: 2, [Symbol('s')]: 3}", "[1, 2]", "function f() {}", "class K { static z = 1 }"];
const orderOps = [
  "p.a", "p.a = 5", "'a' in p", "delete p.a", "Object.keys(p)", "Object.values(p)", "Object.entries(p)", "Object.getOwnPropertyNames(p)",
  "Object.getOwnPropertySymbols(p)", "Reflect.ownKeys(p)", "Object.getOwnPropertyDescriptors(p)", "Object.assign({}, p)", "({...p})",
  "JSON.stringify(p)", "(() => { var r = []; for (var k in p) r.push(k); return r; })()", "Object.freeze(p)", "Object.seal(p)",
  "Object.isFrozen(p)", "Object.isSealed(p)", "Object.isExtensible(p)", "Object.preventExtensions(p)", "Object.getPrototypeOf(p)",
  "Object.setPrototypeOf(p, null)", "p.hasOwnProperty('a')", "Object.hasOwn(p, 'a')", "Object.defineProperty(p, 'n', {value: 1})",
  "p instanceof Object", "Object.prototype.toString.call(p)", "String(p)", "p + ''", "`${p}`", "p == 1", "p < 1", "+p", "Array.isArray(p)",
  "Object.fromEntries(Object.entries(p))", "Math.max === 1", "[].concat(p)", "Array.prototype.concat.call([], p).length",
  "Object.create(p).a", "Object.create(p).zz = 1", "Object.groupBy === undefined", "Array.from(p)", "isNaN(p)", "p.valueOf() === p",
  "Object.keys(Object.create(p))", "Object.getOwnPropertyNames(Object.create(p))", "(Object.create(p).a = 1, 2)", "Reflect.set(Object.create(p), 'a', 9)",
  "Reflect.defineProperty(p, 'a', {value: 2})", "Reflect.get(p, 'a', 5)", "Reflect.has(p, 'a')", "p.toString()", "p.toLocaleString",
];
for (const t of orderTargets) {
  for (const op of orderOps) {
    add(`${logHandler}var p = new Proxy(${t}, rec);\nglobalThis.R = run(() => { "use strict"; return ${op}; }) + " | " + log.join(',');\n`);
  }
}
// Handler cujo get de trap registra e devolve a trap só para algumas.
add(`var log = [];\nvar h = {get get() { log.push('get'); return undefined; }, get set() { log.push('set'); return undefined; }};\nvar p = new Proxy({}, h);\np.a; p.a = 1;\nglobalThis.R = log.join(',');\n`);
add(`var log = [];\nvar p = new Proxy({a: 1}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nvar s = \`\${p}\`;\nglobalThis.R = log.join(',') + ' | ' + s;\n`);
add(`var log = [];\nvar p = new Proxy({a: 1}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nvar s = p + 1;\nglobalThis.R = log.join(',') + ' | ' + s;\n`);
add(`var log = [];\nvar p = new Proxy({a: 1}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nvar s = [...[p]].length; var j = JSON.stringify({p});\nglobalThis.R = log.join(',') + ' | ' + j;\n`);
add(`var log = [];\nvar p = new Proxy({a: 1, toJSON() { return 'j'; }}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = JSON.stringify(p) + ' | ' + log.join(',');\n`);
add(`var log = [];\nvar p = new Proxy([1, 2], {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = JSON.stringify(p) + ' | ' + log.join(',');\n`);
add(`var log = [];\nvar p = new Proxy({}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = Object.prototype.toString.call(p) + ' | ' + log.join(',');\n`);
add(`var log = [];\nvar p = new Proxy([], {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = Object.prototype.toString.call(p) + ' | ' + log.join(',');\n`);
add(`var log = [];\nvar p = new Proxy(function () {}, {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }});\nglobalThis.R = Object.prototype.toString.call(p) + ' | ' + log.join(',');\n`);
add(`var p = new Proxy({}, {get(t, k) { return k === Symbol.toStringTag ? 'Custom' : undefined; }});\nglobalThis.R = Object.prototype.toString.call(p);\n`);
add(`var p = new Proxy([], {get(t, k) { return k === Symbol.toStringTag ? 'Custom' : Reflect.get(t, k); }});\nglobalThis.R = Object.prototype.toString.call(p);\n`);
add(`var p = new Proxy(new Date(0), {});\nglobalThis.R = run(() => Object.prototype.toString.call(p)) + ' | ' + run(() => p.getTime()) + ' | ' + run(() => Date.prototype.getTime.call(p));\n`);
add(`var p = new Proxy(new Map(), {});\nglobalThis.R = run(() => p.size) + ' | ' + run(() => p.get(1)) + ' | ' + run(() => Map.prototype.get.call(p, 1)) + ' | ' + run(() => Object.prototype.toString.call(p));\n`);
add(`var p = new Proxy(new Set([1]), {});\nglobalThis.R = run(() => p.size) + ' | ' + run(() => p.has(1)) + ' | ' + run(() => [...p]);\n`);
add(`var p = new Proxy(new Set([1]), {get(t, k) { var v = Reflect.get(t, k, t); return typeof v === 'function' ? v.bind(t) : v; }});\nglobalThis.R = run(() => p.size) + ' | ' + run(() => p.has(1)) + ' | ' + run(() => [...p]);\n`);
add(`var p = new Proxy(/a/g, {});\nglobalThis.R = run(() => p.test('a')) + ' | ' + run(() => p.source) + ' | ' + run(() => RegExp.prototype.exec.call(p, 'a')) + ' | ' + run(() => 'aa'.replace(p, 'b')) + ' | ' + run(() => Object.prototype.toString.call(p));\n`);
add(`var p = new Proxy(new Error('x'), {});\nglobalThis.R = run(() => p.message) + ' | ' + run(() => Object.prototype.toString.call(p)) + ' | ' + run(() => String(p)) + ' | ' + run(() => p instanceof Error);\n`);
add(`var p = new Proxy(Promise.resolve(1), {});\nglobalThis.R = run(() => p.then(() => 1)) + ' | ' + run(() => Promise.prototype.then.call(p, () => 1)) + ' | ' + run(() => Promise.resolve(p) === p);\n`);
add(`var p = new Proxy(new Uint8Array(2), {});\nglobalThis.R = run(() => p.length) + ' | ' + run(() => p[0]) + ' | ' + run(() => Object.keys(p)) + ' | ' + run(() => ArrayBuffer.isView(p)) + ' | ' + run(() => Object.prototype.toString.call(p));\n`);
add(`var p = new Proxy(new String('ab'), {});\nglobalThis.R = run(() => p.length) + ' | ' + run(() => String.prototype.toString.call(p)) + ' | ' + run(() => p.toString()) + ' | ' + run(() => p[0]) + ' | ' + run(() => Object.keys(p));\n`);
add(`var p = new Proxy(new Number(5), {});\nglobalThis.R = run(() => p + 1) + ' | ' + run(() => p.valueOf()) + ' | ' + run(() => Number.prototype.valueOf.call(p));\n`);
add(`var p = new Proxy(Symbol, {});\nglobalThis.R = run(() => p('d').toString()) + ' | ' + run(() => new p()) + ' | ' + run(() => p.iterator === Symbol.iterator);\n`);
add(`var p = new Proxy({}, {});\nglobalThis.R = run(() => Symbol.keyFor(p)) + ' | ' + run(() => Object(Symbol()) instanceof Symbol) + ' | ' + run(() => Math.max === 0);\n`);
add(`var p = new Proxy(Object, {});\nglobalThis.R = run(() => p.keys({a: 1})) + ' | ' + run(() => new p(5) instanceof Number) + ' | ' + run(() => p({}) instanceof Object);\n`);
add(`var p = new Proxy(Array, {});\nglobalThis.R = run(() => new p(3).length) + ' | ' + run(() => p.of(1, 2)) + ' | ' + run(() => Array.isArray(p())) + ' | ' + run(() => class X extends p {}) + ' | ' + run(() => new (class X extends p {})(2).length);\n`);
add(`var p = new Proxy(Map, {});\nglobalThis.R = run(() => new p([[1, 2]]).get(1)) + ' | ' + run(() => p([])) + ' | ' + run(() => new (class X extends p {})([[1, 2]]).get(1));\n`);
add(`var p = new Proxy(Promise, {});\nglobalThis.R = run(() => p.resolve(1) instanceof Promise) + ' | ' + run(() => new p(() => {}) instanceof Promise);\n`);
add(`var p = new Proxy(Error, {});\nglobalThis.R = run(() => new p('m').message) + ' | ' + run(() => p('m') instanceof Error) + ' | ' + run(() => new (class X extends p {})('q').message);\n`);

// ---- Reflect.*.
const reflectOps = [
  "typeof Reflect", "Object.prototype.toString.call(Reflect)", "Reflect[Symbol.toStringTag]", "Object.getOwnPropertyNames(Reflect).sort().join()",
  "Object.getPrototypeOf(Reflect) === Object.prototype", "typeof Reflect.apply", "Reflect.apply.length", "Reflect.construct.length", "Reflect.defineProperty.length",
  "Reflect.deleteProperty.length", "Reflect.get.length", "Reflect.getOwnPropertyDescriptor.length", "Reflect.getPrototypeOf.length", "Reflect.has.length",
  "Reflect.isExtensible.length", "Reflect.ownKeys.length", "Reflect.preventExtensions.length", "Reflect.set.length", "Reflect.setPrototypeOf.length",
  "Reflect.apply.name", "Reflect.getOwnPropertyDescriptor.name", "new Reflect()", "Reflect()", "new Reflect.get({}, 'a')",
  "Object.getOwnPropertyDescriptor(Reflect, 'get')", "Object.getOwnPropertyDescriptor(Reflect, Symbol.toStringTag)",
  "Reflect.apply(Math.max, null, [1, 5, 3])", "Reflect.apply(Math.max)", "Reflect.apply(Math.max, null)", "Reflect.apply(Math.max, null, 1)",
  "Reflect.apply(1, null, [])", "Reflect.apply(Math.max, null, null)", "Reflect.apply(function () { return this; }, 5, [])", "Reflect.apply(function () { 'use strict'; return this; }, 5, [])",
  "Reflect.apply(String.prototype.slice, 'hello', [1, 3])", "Reflect.apply(class A {}, null, [])", "Reflect.apply(function () { return arguments.length; }, null, {length: 3})",
  "Reflect.construct(function () { this.a = 1; }, [])", "Reflect.construct(Date, [0]).getTime()", "Reflect.construct(1, [])", "Reflect.construct(() => 1, [])",
  "Reflect.construct(function () {}, 1)", "Reflect.construct(function () {}, [], 1)", "Reflect.construct(function () {}, [], () => 1)",
  "Reflect.construct(function () {}, [], Array) instanceof Array", "Reflect.construct(Array, [3]).length", "Reflect.construct(Array, [], Object) instanceof Array",
  "Reflect.construct(Array, [], Object) instanceof Object", "Reflect.construct(function () { return new.target; }, [], Map) === Map",
  "Reflect.construct(class A { constructor() { this.nt = new.target.name; } }, [], class B {}).nt", "Object.getPrototypeOf(Reflect.construct(function () {}, [], Array)) === Array.prototype",
  "Reflect.construct(function () {}, [], function () {}.bind()) instanceof Object", "Reflect.construct(Promise, [() => {}], Object) instanceof Promise",
  "Reflect.construct(Error, ['m'], Array).message", "Reflect.construct(Error, ['m'], Array) instanceof Array", "Reflect.construct(String, ['ab'], Array).length",
  "Reflect.defineProperty({}, 'a', {value: 1})", "Reflect.defineProperty(Object.freeze({}), 'a', {value: 1})", "Reflect.defineProperty(1, 'a', {})",
  "Reflect.defineProperty({}, 'a', 1)", "Reflect.defineProperty({}, 'a')", "Reflect.defineProperty({}, 'a', {get: 1})", "Reflect.defineProperty({}, 'a', {get() {}, value: 1})",
  "Reflect.defineProperty([], 'length', {value: 5})", "Reflect.defineProperty([], 'length', {value: -1})", "Reflect.defineProperty(Object.defineProperty({}, 'a', {value: 1}), 'a', {value: 2})",
  "Reflect.defineProperty({}, Symbol.iterator, {value: 1})", "Reflect.defineProperty({}, {toString() { return 'k'; }}, {value: 1})",
  "Reflect.deleteProperty({a: 1}, 'a')", "Reflect.deleteProperty(Object.freeze({a: 1}), 'a')", "Reflect.deleteProperty({}, 'a')", "Reflect.deleteProperty(1, 'a')",
  "Reflect.deleteProperty([1], 'length')", "Reflect.deleteProperty([1], 0)",
  "Reflect.get({a: 1}, 'a')", "Reflect.get({get a() { return this; }}, 'a', 5)", "Reflect.get({get a() { return this; }}, 'a') === undefined", "Reflect.get({a: 1}, 'b')",
  "Reflect.get(1, 'a')", "Reflect.get({}, Symbol.iterator)", "Reflect.get([5], 0)", "Reflect.get('abc', 0)", "Reflect.get({a: 1})", "Reflect.get({undefined: 7})",
  "Reflect.get(Object.create({p: 2}), 'p')", "Reflect.get({get a() { 'use strict'; return typeof this; }}, 'a', 5)",
  "Reflect.getOwnPropertyDescriptor({a: 1}, 'a')", "Reflect.getOwnPropertyDescriptor({a: 1}, 'b')", "Reflect.getOwnPropertyDescriptor(1, 'a')",
  "Reflect.getOwnPropertyDescriptor([], 'length')", "Reflect.getOwnPropertyDescriptor({get a() { return 1; }}, 'a')", "Reflect.getOwnPropertyDescriptor(function f(a) {}, 'length')",
  "Reflect.getPrototypeOf({}) === Object.prototype", "Reflect.getPrototypeOf(Object.create(null))", "Reflect.getPrototypeOf(1)", "Reflect.getPrototypeOf([]) === Array.prototype",
  "Reflect.getPrototypeOf(() => 1) === Function.prototype", "Reflect.getPrototypeOf(class A extends Array {}) === Array",
  "Reflect.has({a: 1}, 'a')", "Reflect.has({}, 'toString')", "Reflect.has(1, 'a')", "Reflect.has([1], 0)", "Reflect.has([1], 'length')", "Reflect.has({}, Symbol.iterator)",
  "Reflect.isExtensible({})", "Reflect.isExtensible(Object.freeze({}))", "Reflect.isExtensible(1)", "Reflect.isExtensible(Object.preventExtensions([]))",
  "Reflect.ownKeys({b: 1, a: 2, 1: 3, [Symbol.iterator]: 4})", "Reflect.ownKeys([1, 2])", "Reflect.ownKeys(1)", "Reflect.ownKeys('ab')", "Reflect.ownKeys(function f(a) {})",
  "Reflect.ownKeys(class A { static m() {} })", "Reflect.ownKeys(Object.defineProperty({}, 'h', {value: 1}))", "Reflect.ownKeys({2: 1, 10: 1, 1: 1, b: 1, a: 1})",
  "Reflect.preventExtensions({})", "Reflect.preventExtensions(1)", "Reflect.preventExtensions(Object.freeze({}))",
  "Reflect.set({}, 'a', 1)", "Reflect.set(Object.freeze({}), 'a', 1)", "Reflect.set(1, 'a', 1)", "Reflect.set({set a(v) { this.z = v; }}, 'a', 1, {})",
  "(o => (Reflect.set({set a(v) { this.z = v; }}, 'a', 5, o), o.z))({})", "(o => (Reflect.set({}, 'a', 5, o), o.a))({})", "(o => Reflect.set({}, 'a', 5, o))(Object.freeze({}))",
  "(o => Reflect.set({}, 'a', 5, o))(1)", "Reflect.set({}, 'a', 5, 1)", "Reflect.set({}, 'a', 5, undefined)", "Reflect.set({}, 'a', 5, null)",
  "(o => (Reflect.set(Object.defineProperty({}, 'a', {value: 1, writable: false}), 'a', 2, o), Object.getOwnPropertyDescriptor(o, 'a')))({a: 0})",
  "(o => (Reflect.set({a: 1}, 'a', 2, o), Object.getOwnPropertyDescriptor(o, 'a')))({})", "(o => Reflect.set({a: 1}, 'a', 2, o))(Object.defineProperty({}, 'a', {get() {}}))",
  "(o => Reflect.set({a: 1}, 'a', 2, o))(Object.defineProperty({}, 'a', {value: 1, writable: false}))", "(o => (Reflect.set({a: 1}, 'a', 2, o), o.a))(Object.defineProperty({}, 'a', {value: 1, writable: true}))",
  "Reflect.set([], 'length', 3)", "(a => (Reflect.set(a, 0, 1), a.length))([])", "(a => (Reflect.set([], 0, 1, a), a.length))([])", "Reflect.set(Object.create({set a(v) {}}), 'a', 1)",
  "Reflect.set(Object.create({get a() { return 1; }}), 'a', 1)", "Reflect.set(Object.create(Object.freeze({a: 1})), 'a', 2)",
  "Reflect.setPrototypeOf({}, null)", "Reflect.setPrototypeOf({}, 1)", "Reflect.setPrototypeOf(1, null)", "Reflect.setPrototypeOf({}, undefined)", "Reflect.setPrototypeOf({})",
  "Reflect.setPrototypeOf(Object.preventExtensions({}), {})", "Reflect.setPrototypeOf(Object.preventExtensions({}), Object.prototype)", "(o => Reflect.setPrototypeOf(o, o))({})",
  "(o => (p => Reflect.setPrototypeOf(o, p))(Object.create(o)))({})", "Reflect.setPrototypeOf(Object.prototype, {})", "Reflect.setPrototypeOf(Object.prototype, null)",
  "Reflect.setPrototypeOf(function () {}, null)", "Reflect.setPrototypeOf([], () => 1)",
];
for (const body of reflectOps) add(`globalThis.R = run(() => ${body});\n`);

// ---- Reflect com receiver e proxies.
const receiverOps = [
  "Reflect.get(p, 'a', 5)", "Reflect.get(p, 'a')", "Reflect.set(p, 'a', 5, 6)", "Reflect.set(p, 'a', 5)", "Reflect.get(Object.create(p), 'a')",
  "(Object.create(p).a = 5, 1)", "(Object.create(p).b = 5, 1)", "Reflect.set(Object.create(p), 'a', 5, {})", "Reflect.has(Object.create(p), 'a')",
];
for (const op of receiverOps) {
  add(`var log = [];\nvar t = {get a() { return this; }, set a(v) { log.push('setter'); }};\nvar p = new Proxy(t, {get(t, k, r) { log.push('get:' + typeof r + ':' + (r === p)); return Reflect.get(t, k, r); }, set(t, k, v, r) { log.push('set:' + typeof r + ':' + (r === p)); return Reflect.set(t, k, v, r); }, getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k) + ':' + fmt(d)); return Reflect.defineProperty(t, k, d); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }});\nglobalThis.R = run(() => ${op}) + " | " + log.join(',');\n`);
}
// Set via proxy no protótipo: define na receiver.
for (const op of ["(o.a = 1, o.hasOwnProperty('a'))", "(o.a = 1, log.join())", "(o[0] = 1, log.join())", "(o.a = 1, Object.keys(o).join())"]) {
  add(`var log = [];\nvar p = new Proxy({}, {set(t, k, v, r) { log.push('set:' + String(k) + ':' + (r === o)); return Reflect.set(t, k, v, r); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); }, getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }});\nvar o = Object.create(p);\nglobalThis.R = run(() => ${op}) + " | " + log.join(',');\n`);
}
// Receiver como proxy: define na receiver chama defineProperty/gOPD do proxy.
for (const op of ["Reflect.set({}, 'a', 1, p)", "Reflect.set({a: 0}, 'a', 1, p)", "Reflect.set({set a(v) {}}, 'a', 1, p)", "Reflect.set(Object.freeze({a: 1}), 'a', 1, p)"]) {
  add(`var log = [];\nvar p = new Proxy({}, {getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k) + ':' + fmt(d)); return Reflect.defineProperty(t, k, d); }, set(t, k, v, r) { log.push('SET'); return true; }});\nglobalThis.R = run(() => ${op}) + " | " + log.join(',');\n`);
}

// ---- Construções de linguagem.
const langs = [
  // Array.isArray / JSON / for-in / spread / Object.keys
  "Array.isArray(new Proxy([], {}))", "Array.isArray(new Proxy({}, {}))", "Array.isArray(new Proxy(new Proxy([], {}), {}))",
  "JSON.stringify(new Proxy({a: 1, b: [1, 2]}, {}))", "JSON.stringify(new Proxy([1, {a: 2}], {}))", "JSON.stringify({x: new Proxy({a: 1}, {})})",
  "JSON.stringify(new Proxy({a: 1, b: 2}, {ownKeys: () => ['b']}))", "JSON.stringify(new Proxy({a: 1}, {get: (t, k) => k === 'toJSON' ? () => 'custom' : t[k]}))",
  "JSON.stringify(new Proxy({a: 1}, {getOwnPropertyDescriptor: () => undefined}))", "JSON.stringify(new Proxy({a: 1}, {get: () => 9}))",
  "JSON.stringify(new Proxy([1, 2], {get: (t, k) => k === 'length' ? 1 : t[k]}))", "JSON.stringify(new Proxy(function () {}, {}))", "JSON.stringify([new Proxy(function () {}, {})])",
  "JSON.stringify(new Proxy({}, {}), null, 2)", "JSON.stringify({a: new Proxy({b: 1}, {})}, null, 1)", "JSON.stringify(new Proxy({a: 1, b: 2}, {}), ['a'])",
  "JSON.parse('{\"a\": 1}', new Proxy(function (k, v) { return v; }, {})).a", "JSON.stringify(Proxy.revocable({}, {}).proxy)",
  "(() => { var r = []; for (var k in new Proxy({a: 1, b: 2}, {})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1, b: 2}, {ownKeys: () => ['b', 'a']})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1, b: 2}, {ownKeys: () => ['x', 'a'], getOwnPropertyDescriptor: (t, k) => k === 'x' ? {value: 1, enumerable: true, configurable: true} : Reflect.getOwnPropertyDescriptor(t, k)})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1}, {getPrototypeOf: () => ({inh: 1})})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in Object.create(new Proxy({a: 1, b: 2}, {}))) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in Object.create(new Proxy({a: 1}, {ownKeys: () => ['q'], getOwnPropertyDescriptor: () => ({value: 1, enumerable: true, configurable: true})}))) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy([5, 6], {})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1}, {ownKeys: () => { throw new Error('ok'); }})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1, b: 2}, {getOwnPropertyDescriptor(t, k) { r.push('d' + k); return Reflect.getOwnPropertyDescriptor(t, k); }})) r.push(k); return r; })()",
  "(() => { var r = []; for (var k in new Proxy({a: 1, b: 2}, {has(t, k) { r.push('h' + k); return true; }})) r.push(k); return r; })()",
  "Object.keys(new Proxy({a: 1, b: 2}, {}))", "Object.keys(new Proxy({a: 1, b: 2}, {getOwnPropertyDescriptor: () => undefined}))",
  "Object.keys(new Proxy({a: 1, b: 2}, {ownKeys: () => ['a', 'z', Symbol.iterator]}))", "Object.keys(new Proxy({}, {ownKeys: () => ['a', 'b'], getOwnPropertyDescriptor: () => ({value: 1, enumerable: true, configurable: true})}))",
  "Object.keys(new Proxy({}, {ownKeys: () => ['a', 'b'], getOwnPropertyDescriptor: (t, k) => k === 'a' ? {value: 1, enumerable: false, configurable: true} : {value: 1, enumerable: true, configurable: true}}))",
  "Object.values(new Proxy({a: 1, b: 2}, {get: (t, k) => t[k] * 10}))", "Object.entries(new Proxy({a: 1}, {get: () => 'v'}))",
  "Object.getOwnPropertyNames(new Proxy({a: 1}, {ownKeys: () => ['q', Symbol.iterator]}))", "Object.getOwnPropertyDescriptors(new Proxy({a: 1}, {}))",
  "Object.getOwnPropertyDescriptors(new Proxy({a: 1}, {getOwnPropertyDescriptor: () => undefined}))", "Object.getOwnPropertyDescriptors(new Proxy({}, {ownKeys: () => ['x'], getOwnPropertyDescriptor: () => ({value: 3, configurable: true})}))",
  "[...new Proxy([1, 2, 3], {})]", "[...new Proxy({}, {get: (t, k) => k === Symbol.iterator ? function* () { yield 1; yield 2; } : undefined})]", "[...new Proxy({}, {})]",
  "({...new Proxy({a: 1, b: 2}, {})})", "({...new Proxy({a: 1}, {ownKeys: () => ['a', 'b'], get: (t, k) => k, getOwnPropertyDescriptor: () => ({value: 1, enumerable: true, configurable: true})})})",
  "({...new Proxy({a: 1}, {get: () => 7})})", "({...new Proxy({a: 1}, {getOwnPropertyDescriptor: () => undefined})})", "Object.assign({}, new Proxy({a: 1, b: 2}, {}))",
  "Object.assign(new Proxy({}, {set: (t, k, v) => { t[k] = v * 2; return true; }}), {a: 1, b: 2})", "Object.assign(new Proxy({}, {set: () => false}), {a: 1})",
  "Object.assign(new Proxy({}, {defineProperty: () => false}), {a: 1})", "Object.assign({}, new Proxy([1, 2], {}))",
  "(({a, ...rest}) => [a, rest])(new Proxy({a: 1, b: 2, c: 3}, {}))", "(({a, b}) => a + b)(new Proxy({a: 1, b: 2}, {get: (t, k) => t[k] * 3}))", "(([x, y]) => x + y)(new Proxy([1, 2], {}))",
  // instanceof, typeof, toString
  "new Proxy({}, {}) instanceof Object", "new Proxy([], {}) instanceof Array", "new Proxy(function () {}, {}) instanceof Function", "({}) instanceof new Proxy(Object, {})",
  "[] instanceof new Proxy(Array, {})", "({}) instanceof new Proxy(function () {}, {})", "({}) instanceof new Proxy(function () {}, {get: (t, k) => k === Symbol.hasInstance ? () => true : t[k]})",
  "({}) instanceof new Proxy(function () {}, {get: (t, k) => k === 'prototype' ? Array.prototype : t[k]})", "[] instanceof new Proxy(function () {}, {get: (t, k) => k === 'prototype' ? Array.prototype : t[k]})",
  "({}) instanceof new Proxy({}, {})", "({}) instanceof new Proxy(function () {}, {get: (t, k) => k === 'prototype' ? 1 : t[k]})",
  "new (new Proxy(function F() {}, {}))() instanceof Function", "(() => { class A {} var P = new Proxy(A, {}); return new A() instanceof P; })()",
  "(() => { class A {} var P = new Proxy(A, {}); return new P() instanceof A; })()", "(() => { class A {} var P = new Proxy(A, {}); return new P() instanceof P; })()",
  "new Proxy({}, {getPrototypeOf: () => Array.prototype}) instanceof Array", "new Proxy({}, {getPrototypeOf: () => null}) instanceof Object",
  "Object.prototype.toString.call(new Proxy({}, {}))", "Object.prototype.toString.call(new Proxy([], {}))", "Object.prototype.toString.call(new Proxy(function () {}, {}))",
  "Object.prototype.toString.call(new Proxy(new Proxy([], {}), {}))", "Object.prototype.toString.call(new Proxy(new Date(), {}))", "Object.prototype.toString.call(new Proxy(new Error(), {}))",
  "Object.prototype.toString.call(new Proxy(new Boolean(true), {}))", "Object.prototype.toString.call(new Proxy(/x/, {}))", "Object.prototype.toString.call(new Proxy(arguments, {}))",
  "Object.prototype.toString.call(new Proxy({}, {get: () => 'T'}))", "Object.prototype.toString.call(new Proxy({}, {get: (t, k) => k === Symbol.toStringTag ? 5 : undefined}))",
  "String(new Proxy({}, {get: (t, k) => k === 'toString' ? () => 'custom' : undefined}))", "`${new Proxy({}, {get: (t, k) => k === Symbol.toPrimitive ? () => 'prim' : undefined})}`",
  "new Proxy({}, {get: (t, k) => k === Symbol.toPrimitive ? h => h : undefined}) + ''", "+new Proxy({}, {get: (t, k) => k === Symbol.toPrimitive ? h => h === 'number' ? 42 : 0 : undefined})",
  "new Proxy({}, {get: (t, k) => k === Symbol.toPrimitive ? 1 : undefined}) + ''", "new Proxy({}, {get: (t, k) => k === Symbol.toPrimitive ? () => ({}) : undefined}) + ''",
  "new Proxy({valueOf() { return 3; }}, {}) * 2", "new Proxy({toString() { return 's'; }}, {}) + 1", "[new Proxy({}, {})].join()", "[new Proxy([1, 2], {})].join()",
  "[1, new Proxy([2, 3], {})].flat()", "[new Proxy([2, 3], {})].concat([[4]])", "[].concat(new Proxy([2, 3], {}))", "[].concat(new Proxy({length: 1, 0: 'x', [Symbol.isConcatSpreadable]: true}, {}))",
  "[].concat(new Proxy([2, 3], {get: (t, k) => k === Symbol.isConcatSpreadable ? false : t[k]})).length", "[[1], [2]].flat().length", "Array.from(new Proxy([1, 2], {}))",
  "Array.from(new Proxy({length: 2, 0: 'a', 1: 'b'}, {}))", "Array.prototype.map.call(new Proxy({length: 2, 0: 1, 1: 2}, {}), x => x + 1)",
  "Array.prototype.push.call(new Proxy({length: 0}, {}), 'a')", "Array.prototype.includes.call(new Proxy({length: 1, 0: 'z'}, {}), 'z')",
  "Math.max.apply(null, new Proxy([1, 9, 3], {}))", "Math.max(...new Proxy([1, 9, 3], {}))", "String.fromCharCode.apply(null, new Proxy([104, 105], {}))",
  "new Proxy(function () { return arguments.length; }, {})(...[1, 2, 3])", "new Proxy(function () { return new.target === undefined; }, {})()",
  "new (new Proxy(function () { return new.target === undefined; }, {}))() instanceof Object", "new (new Proxy(function () { this.nt = typeof new.target; }, {}))().nt",
  "(() => { var P = new Proxy(function () { return new.target; }, {}); return new P() === P; })()", "(() => { var P = new Proxy(function () { return new.target; }, {}); return Reflect.construct(P, [], Object) === Object; })()",
  "(() => { var F = function () {}; var P = new Proxy(F, {}); return Object.getPrototypeOf(new P()) === F.prototype; })()",
  "(() => { var F = function () {}; var P = new Proxy(F, {get: (t, k) => k === 'prototype' ? Array.prototype : t[k]}); return Object.getPrototypeOf(new P()) === Array.prototype; })()",
  "(() => { var F = function () {}; var P = new Proxy(F, {get: (t, k) => k === 'prototype' ? 5 : t[k]}); return Object.getPrototypeOf(new P()) === Object.prototype; })()",
  "(() => { var F = function () {}; var P = new Proxy(F, {get: () => { throw new Error('g'); }}); return new P(); })()",
  "(() => { var F = function () {}; var P = new Proxy(F, {get: () => { throw new Error('g'); }}); return P(); })()",
  // delete e atribuições com sloppy/strict
  "(() => { 'use strict'; var p = new Proxy({a: 1}, {deleteProperty: () => false}); return delete p.a; })()", "(() => { var p = new Proxy({a: 1}, {deleteProperty: () => false}); return delete p.a; })()",
  "(() => { 'use strict'; var p = new Proxy({a: 1}, {set: () => false}); p.a = 2; return 'no throw'; })()", "(() => { var p = new Proxy({a: 1}, {set: () => false}); p.a = 2; return 'no throw'; })()",
  "(() => { 'use strict'; var p = new Proxy({a: 1}, {set: () => false}); p[3] = 2; return 'no throw'; })()", "(() => { 'use strict'; var p = new Proxy({a: 1}, {set: () => false}); p[Symbol.iterator] = 2; return 'no throw'; })()",
  "(() => { 'use strict'; var p = new Proxy({a: 1}, {defineProperty: () => false}); p.a = 2; return 'no throw'; })()", "(() => { 'use strict'; var p = new Proxy({}, {defineProperty: () => false}); p.a = 2; return 'no throw'; })()",
  "(() => { 'use strict'; var p = new Proxy({}, {defineProperty: () => false}); Object.defineProperty(p, 'a', {value: 1}); return 'no throw'; })()",
  "(() => { 'use strict'; var p = new Proxy({a: 1}, {set: () => false}); p.a++; return 'no throw'; })()", "(() => { 'use strict'; var p = new Proxy({a: 1}, {set: () => false}); p.a += 1; return 'no throw'; })()",
  "(() => { var p = new Proxy({a: 1}, {}); p.a++; p.a += 5; return p.a; })()", "(() => { var log = []; var p = new Proxy({a: 1}, {get: (t, k) => { log.push('g'); return t[k]; }, set: (t, k, v) => { log.push('s'); t[k] = v; return true; }}); p.a++; p.a += 1; return log.join(); })()",
  "(() => { var log = []; var p = new Proxy({a: 1}, {has: (t, k) => { log.push('h' + String(k)); return k in t; }, get: (t, k) => { log.push('g' + String(k)); return t[k]; }}); return ('a' in p) + ' ' + log.join(); })()",
  "(() => { 'use strict'; var p = new Proxy({}, {set: () => true}); return (p.a = 5); })()", "(() => { var p = new Proxy({}, {set: () => false}); return (p.a = 5); })()",
  "(() => { 'use strict'; var p = new Proxy(Object.freeze({a: 1}), {}); p.a = 2; })()", "(() => { 'use strict'; var p = new Proxy(Object.freeze({a: 1}), {}); delete p.a; })()",
  "(() => { 'use strict'; var p = new Proxy(Object.freeze({}), {}); p.b = 2; })()", "(() => { 'use strict'; var p = new Proxy({}, {preventExtensions: () => false}); Object.preventExtensions(p); })()",
  "(() => { 'use strict'; var p = new Proxy({}, {setPrototypeOf: () => false}); Object.setPrototypeOf(p, null); })()", "(() => { 'use strict'; var p = new Proxy({}, {setPrototypeOf: () => false}); p.__proto__ = null; })()",
  "(() => { var p = new Proxy({}, {setPrototypeOf: () => false}); p.__proto__ = {}; return 1; })()", "(() => { var p = new Proxy({}, {setPrototypeOf: () => false}); return Object.prototype.__lookupSetter__('__proto__').call(p, {}); })()",
  "(() => { var p = new Proxy({}, {getPrototypeOf: () => Array.prototype}); return p.__proto__ === Array.prototype; })()", "(() => { var p = new Proxy({}, {}); return p.__proto__ === Object.prototype; })()",
  "(() => { var p = new Proxy({}, {get: (t, k) => k === '__proto__' ? 'fake' : undefined}); return p.__proto__; })()",
  "(() => { var p = new Proxy(Object.create(null), {}); return p.__proto__; })()", "(() => { var o = Object.create(new Proxy({}, {get: (t, k, r) => r === o ? 'viaProto' : 'no'})); return o.zz; })()",
  "(() => { var o = Object.create(new Proxy({}, {has: (t, k) => k === 'q'})); return 'q' in o; })()", "(() => { var o = Object.create(new Proxy({}, {has: (t, k) => k === 'q'})); return o.hasOwnProperty('q'); })()",
  "(() => { var o = Object.create(new Proxy({}, {set: (t, k, v, r) => { r.viaSet = v; return true; }})); o.a = 1; return Object.keys(o).join(); })()",
  "(() => { var o = Object.create(new Proxy({}, {set: () => true})); o.a = 1; return Object.keys(o).length; })()",
  "(() => { var o = Object.create(new Proxy({}, {set: () => false})); 'use strict'; o.a = 1; return Object.keys(o).length; })()",
  "(() => { 'use strict'; var o = Object.create(new Proxy({}, {set: () => false})); o.a = 1; })()",
  "(() => { var o = Object.create(new Proxy({}, {deleteProperty: () => { throw new Error('d'); }})); delete o.a; return 'ok'; })()",
  "(() => { var o = Object.create(new Proxy({}, {ownKeys: () => ['k']})); return Object.keys(o).length; })()",
  "(() => { var o = Object.create(new Proxy({}, {getOwnPropertyDescriptor: () => { throw new Error('x'); }})); return o.hasOwnProperty('a'); })()",
  "(() => { var o = Object.create(new Proxy({}, {getOwnPropertyDescriptor: () => { throw new Error('x'); }})); o.a = 1; return Object.keys(o).join(); })()",
  // Identidade, WeakMap, Map, Set, symbols
  "(() => { var t = {}; var p = new Proxy(t, {}); return [p === t, Object.is(p, t), p == t].join(); })()", "(() => { var t = {}; var p = new Proxy(t, {}); var m = new Map([[t, 1]]); return m.has(p); })()",
  "(() => { var t = {}; var p = new Proxy(t, {}); var w = new WeakSet([p]); return w.has(p) + ' ' + w.has(t); })()", "(() => { var p = new Proxy({}, {}); var w = new WeakMap(); w.set(p, 1); return w.get(p); })()",
  "(() => { var p = new Proxy({}, {}); return Object.is(p, p) && p === p; })()", "(() => { var p = new Proxy(function () {}, {}); return new Set([p, p]).size; })()",
  "(() => { var p = new Proxy({}, {}); return Object.isFrozen(Object.freeze(p)); })()", "(() => { var t = {a: 1}; var p = new Proxy(t, {}); Object.freeze(p); return Object.isFrozen(t); })()",
  "(() => { var t = {a: 1}; var p = new Proxy(t, {}); Object.seal(p); return Object.isSealed(t) + ' ' + Object.isFrozen(t); })()", "(() => { var t = [1, 2]; var p = new Proxy(t, {}); Object.freeze(p); return Object.isFrozen(t); })()",
  "(() => { var log = []; var p = new Proxy({a: 1}, {defineProperty(t, k, d) { log.push(k + fmt(d)); return Reflect.defineProperty(t, k, d); }, getOwnPropertyDescriptor(t, k) { log.push('g' + k); return Reflect.getOwnPropertyDescriptor(t, k); }, ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t); }, preventExtensions(t) { log.push('pe'); return Reflect.preventExtensions(t); }}); Object.freeze(p); return log.join(); })()",
  "(() => { var log = []; var p = new Proxy({a: 1}, {defineProperty(t, k, d) { log.push(k + fmt(d)); return Reflect.defineProperty(t, k, d); }, getOwnPropertyDescriptor(t, k) { log.push('g' + k); return Reflect.getOwnPropertyDescriptor(t, k); }, ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t); }, preventExtensions(t) { log.push('pe'); return Reflect.preventExtensions(t); }}); Object.seal(p); return log.join(); })()",
  "(() => { var log = []; var p = new Proxy(Object.freeze({a: 1}), {isExtensible(t) { log.push('ie'); return Reflect.isExtensible(t); }, ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t); }, getOwnPropertyDescriptor(t, k) { log.push('g' + k); return Reflect.getOwnPropertyDescriptor(t, k); }}); Object.isFrozen(p); return log.join(); })()",
  "typeof new Proxy({}, {}) === 'object'", "typeof new Proxy(function () {}, {get: () => 1}) === 'function'", "(() => { var p = new Proxy(function () {}, {getPrototypeOf: () => null}); return typeof p + ' ' + (p instanceof Function); })()",
  "Object.getOwnPropertyDescriptor(new Proxy({a: 1}, {}), 'a').value", "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => ({value: 1, configurable: true})}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => ({get() {}, set: undefined, configurable: true})}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => ({value: 1, extra: 2, configurable: true})}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => ({get value() { return 7; }, configurable: true})}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => Object.create({value: 8, configurable: true})}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => function () {}}), 'a')",
  "Object.getOwnPropertyDescriptor(new Proxy({}, {getOwnPropertyDescriptor: () => new Proxy({value: 3, configurable: true}, {})}), 'a')",
  "Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => (Object.keys(d).join() === 'value' ? true : false)}), 'a', {value: 1})",
  "Reflect.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { return Reflect.ownKeys(d).join(); }}), 'a', {value: 1, enumerable: true})",
  "(() => { var seen; Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { seen = d; return true; }}), 'a', {get: undefined, configurable: true}); return Reflect.ownKeys(seen).join(); })()",
  "(() => { var seen; Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { seen = d; return true; }}), 'a', Object.create({value: 1, enumerable: true})); return Reflect.ownKeys(seen).join(); })()",
  "(() => { var seen; Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { seen = d; return true; }}), 'a', {value: 1, extra: 1}); return Reflect.ownKeys(seen).join(); })()",
  "(() => { var seen; Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { seen = d; return true; }}), 'a', {value: 1}); return Object.getPrototypeOf(seen) === Object.prototype; })()",
  "(() => { var seen; Object.defineProperty(new Proxy({}, {defineProperty: (t, k, d) => { seen = k; return true; }}), 1, {value: 1}); return typeof seen; })()",
  "(() => { var seen; var k = {toString() { return 'ks'; }}; Object.defineProperty(new Proxy({}, {defineProperty: (t, key, d) => { seen = key; return true; }}), k, {value: 1}); return typeof seen + seen; })()",
  "(() => { var seen; 'x' in new Proxy({}, {has: (t, key) => { seen = key; return true; }}); return typeof seen; })()",
  "(() => { var seen; new Proxy({}, {get: (t, key) => { seen = key; }})[5]; return typeof seen; })()",
  "(() => { var seen; new Proxy([], {get: (t, key) => { seen = key; }})[5]; return typeof seen + seen; })()",
  "(() => { var seen; new Proxy({}, {get: (t, key) => { seen = key; }})[-0]; return typeof seen + seen; })()",
  "(() => { var seen; new Proxy({}, {get: (t, key) => { seen = key; }})[1.5]; return typeof seen + seen; })()",
  "(() => { var seen; new Proxy({}, {get: (t, key) => { seen = key; }})[2 ** 32]; return typeof seen + seen; })()",
  "(() => { var seen = []; new Proxy({}, {get: (t, key, r) => { seen.push(typeof r); }}).a; new Proxy({}, {get: function (t, key, r) { seen.push(this === undefined ? 'u' : typeof this); }}).a; return seen.join(); })()",
  "(() => { var h = {get(t, k) { return this === h; }}; return new Proxy({}, h).a; })()", "(() => { var h = {apply(t, th, a) { return this === h; }}; return new Proxy(function () {}, h)(); })()",
  "(() => { var h = {construct(t, a) { return {same: this === h}; }}; return new (new Proxy(function () {}, h))().same; })()",
  "(() => { 'use strict'; var h = {get() { return typeof this; }}; return new Proxy({}, h).a; })()",
  "(() => { var h = Object.create({get: () => 'inherited'}); return new Proxy({}, h).a; })()", "(() => { var h = {}; var p = new Proxy({}, h); h.get = () => 'late'; return p.a; })()",
  "(() => { var h = {get: () => 1}; var p = new Proxy({}, h); delete h.get; return p.a; })()", "(() => { var h = {get: () => 1}; var p = new Proxy({}, h); h.get = null; return p.a; })()",
  "(() => { var h = {get: () => 1}; var p = new Proxy({}, h); h.get = 5; return p.a; })()", "(() => { var p = new Proxy({}, new Proxy({}, {get: (t, k) => () => k})); return p.a; })()",
  "(() => { var p = new Proxy({}, new Proxy({}, {get: (t, k) => (tt, kk) => k + ':' + String(kk)})); return p.a + ' ' + ('b' in p); })()",
  "(() => { var t = {a: 1}; var p = new Proxy(t, {}); p.b = 2; delete p.a; return Object.keys(t).join(); })()", "(() => { var t = []; var p = new Proxy(t, {}); p.push(1, 2); p[5] = 1; return t.length; })()",
  "(() => { var t = {}; var p = new Proxy(t, {}); Object.defineProperty(p, 'x', {get() { return this === p; }}); return p.x; })()",
  "(() => { var t = {}; var p = new Proxy(t, {}); Object.defineProperty(t, 'x', {get() { return this === p; }}); return p.x + ' ' + t.x; })()",
  "(() => { var t = {set x(v) { this.y = v; }}; var p = new Proxy(t, {}); p.x = 3; return Object.keys(t).join() + Object.keys(p).join(); })()",
  "(() => { var t = {set x(v) { this.y = v; }}; var p = new Proxy(t, {set(t, k, v, r) { return Reflect.set(t, k, v, t); }}); p.x = 3; return Object.keys(t).join(); })()",
  "(() => { var t = {}; var p = new Proxy(t, {set(t, k, v, r) { return Reflect.set(t, k, v, r); }}); p.x = 3; return Object.keys(t).join(); })()",
  "(() => { var t = {}; var p = new Proxy(t, {set(t, k, v, r) { return Reflect.set(t, k, v, r); }, defineProperty(t, k, d) { return Reflect.defineProperty(t, k, d); }}); p.x = 3; return Object.keys(t).join(); })()",
];
for (const body of langs) add(`globalThis.R = run(() => ${body});\n`);

// ---- with, Symbol.unscopables, `in`.
const withs = [
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = a; }", "has,get:a"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }}); with (p) { a; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }}); with (p) { a = 2; } globalThis.R = log.join() + ' ' + p.a;"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }}); with (p) { a = 2; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }}); with (p) { typeof zzz; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }}); with (p) { typeof a; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return k in t; }, get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }}); with (p) { try { zzz; } catch (e) { log.push(e.name + ': ' + e.message); } } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({f() { return this === p; }}, {has(t, k) { log.push('has:' + String(k)); return k in t; }}); with (p) { log.push(f()); } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({f() { return typeof this; }}, {}); with (p) { log.push(f()); } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {has(t, k) { log.push('has:' + String(k)); return true; }, get(t, k) { log.push('get:' + String(k)); return k === Symbol.unscopables ? undefined : 'v'; }}); with (p) { globalThis.R = anything; } globalThis.R += '|' + log.join();"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? {a: true} : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? {a: false} : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? {a: 1} : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? 5 : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? null : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? new Proxy({a: true}, {get(tt, kk) { return tt[kk]; }}) : t[k]}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var log = []; var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? new Proxy({}, {get(tt, kk) { log.push('uns:' + String(kk)); return false; }}) : t[k]}); with (p) { a; } globalThis.R = log.join();"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: (t, k) => { if (k === Symbol.unscopables) throw new Error('u'); return t[k]; }}); try { with (p) { a; } globalThis.R = 'no throw'; } catch (e) { globalThis.R = e.message; }"],
  ["var p = new Proxy({a: 1}, {has: () => { throw new Error('h'); }}); try { with (p) { a; } globalThis.R = 'no throw'; } catch (e) { globalThis.R = e.message; }"],
  ["var p = new Proxy({a: 1}, {has: () => false}); var a = 'outer'; with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {has: () => true, get: () => undefined}); with (p) { globalThis.R = typeof a; }"],
  ["var p = new Proxy({}, {has: (t, k) => k === 'dyn', get: (t, k) => k === 'dyn' ? 'dynamic' : undefined}); with (p) { globalThis.R = dyn; }"],
  ["var p = new Proxy({}, {has: (t, k) => k === 'dyn', get: (t, k) => k === 'dyn' ? 'dynamic' : undefined, set(t, k, v) { globalThis.R = 'set:' + String(k) + '=' + v; return true; }}); with (p) { dyn = 5; }"],
  ["var p = new Proxy({}, {has: (t, k) => k === 'dyn', get: (t, k) => k === 'dyn' ? 'dynamic' : undefined, set() { return false; }}); try { with (p) { dyn = 5; } globalThis.R = 'no throw (sloppy)'; } catch (e) { globalThis.R = e.message; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { var a = 7; } globalThis.R = p.a + ' ' + typeof globalThis.a;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { var b = 7; } globalThis.R = p.b + ' ' + b;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { function g() { return a; } } globalThis.R = g();"],
  ["var p = new Proxy({a: 1}, {}); var g; with (p) { g = () => a; } p.a = 9; globalThis.R = g();"],
  ["var p = new Proxy({a: 1}, {}); var g; with (p) { g = () => a; } delete p.a; globalThis.R = (() => { try { return g(); } catch (e) { return e.name + ': ' + e.message; } })();"],
  ["var p = new Proxy({a: 1}, {has(t, k) { return k in t; }}); var g; with (p) { g = () => a; } delete p.a; var a = 'fallback'; globalThis.R = g();"],
  ["var r = Proxy.revocable({a: 1}, {}); with (r.proxy) { globalThis.R = a; r.revoke(); try { a; } catch (e) { globalThis.R += '|' + e.name + ': ' + e.message; } }"],
  ["var r = Proxy.revocable({a: 1}, {}); r.revoke(); try { with (r.proxy) { a; } globalThis.R = 'no throw'; } catch (e) { globalThis.R = e.name + ': ' + e.message; }"],
  ["var p = new Proxy([1, 2, 3], {}); with (p) { globalThis.R = length + ',' + join('-') + ',' + (typeof values); }"],
  ["var p = new Proxy([1, 2, 3], {}); with (p) { globalThis.R = typeof keys + ',' + typeof entries + ',' + typeof find + ',' + typeof flat + ',' + typeof fill + ',' + typeof includes; }"],
  ["var p = new Proxy([1, 2, 3], {}); var keys = 'outer'; var values = 'outer'; with (p) { globalThis.R = typeof keys + ',' + typeof values; }"],
  ["with (new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : (...a) => k})) { globalThis.R = foo(); }"],
  ["with (new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : function () { return this === undefined ? 'u' : typeof this; }})) { globalThis.R = foo(); }"],
  ["with (new Proxy({}, {has: (t, k) => k === 'foo', get: function (t, k) { return k === 'foo' ? function () { return typeof this; } : undefined; }})) { globalThis.R = foo(); }"],
  ["with (new Proxy({}, {has: (t, k) => k === 'foo', get: function (t, k) { return k === 'foo' ? function () { 'use strict'; return this === undefined ? 'undef' : 'obj'; } : undefined; }})) { globalThis.R = foo(); }"],
  ["with (new Proxy({}, {has: (t, k) => k === 'foo', get: function (t, k) { return k === 'foo' ? 5 : undefined; }})) { try { foo(); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }"],
  ["with (new Proxy({}, {has: (t, k) => k === 'foo', get: function (t, k) { return k === 'foo' ? {} : undefined; }})) { try { foo(); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }"],
  ["with (new Proxy({}, {has: (t, k) => k === 'foo', get: function (t, k) { return k === 'foo' ? () => 1 : undefined; }})) { try { new foo(); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }"],
  ["var log = []; with (new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }})) { Math.max(1, 2); } globalThis.R = log.join();"],
  ["var log = []; with (new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }})) { (function () { return log.length; })(); } globalThis.R = log.join();"],
  ["var log = []; with (new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }})) { (function (zz) { return zz + log.length; })(1); } globalThis.R = log.join();"],
  ["var log = []; with (new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }})) { eval('undefinedVar1'); } globalThis.R = log.join();"],
  ["var log = []; with (new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }})) { try { eval('undefinedVar1'); } catch (e) { log.push(e.message); } } globalThis.R = log.join();"],
  ["var p = new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : 'x'}); with (p) { globalThis.R = [typeof undefined, typeof NaN, typeof Infinity].join(); }"],
  ["var p = new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : 'x'}); with (p) { globalThis.R = [undefined, this === globalThis].join(); }"],
  ["var p = new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : 'x'}); with (p) { globalThis.R = (() => typeof globalThis)() + typeof p; }"],
  ["var o = {a: 1}; o[Symbol.unscopables] = {a: true}; var a = 'outer'; with (new Proxy(o, {})) { globalThis.R = a; }"],
  ["var o = {a: 1}; o[Symbol.unscopables] = {a: true}; var a = 'outer'; with (o) { globalThis.R = a; }"],
  ["var log = []; var o = {a: 1}; var p = new Proxy(o, {get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has:' + String(k)); return k in t; }}); with (p) { a; a; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has:' + String(k)); return k in t; }}); with (p) { a++; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has:' + String(k)); return k in t; }, set(t, k, v) { log.push('set:' + String(k)); t[k] = v; return true; }}); with (p) { a++; } globalThis.R = log.join();"],
  ["var log = []; var p = new Proxy({a: 1}, {get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has:' + String(k)); return k in t; }}); with (p) { delete a; } globalThis.R = log.join() + ' ' + ('a' in p);"],
  ["var log = []; var p = new Proxy({a: 1}, {deleteProperty(t, k) { log.push('del:' + String(k)); return delete t[k]; }, has(t, k) { log.push('has:' + String(k)); return k in t; }}); with (p) { delete a; } globalThis.R = log.join();"],
  ["var p = new Proxy({a: {b: 1}}, {}); with (p) with (a) { globalThis.R = b; }"],
  ["var p = new Proxy({a: 1}, {}); var q = new Proxy({b: 2}, {}); with (p) with (q) { globalThis.R = a + b; }"],
  ["var p = new Proxy({a: 1}, {has: (t, k) => k !== 'a' && k in t}); var q = new Proxy({a: 2}, {}); with (q) with (p) { globalThis.R = a; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = (function () { return a; })(); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = (() => { try { return eval('a'); } catch (e) { return e.message; } })(); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = eval('var zz = 5; a + zz'); } globalThis.R += ' ' + typeof zz + ' ' + p.zz;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = new Function('return typeof a')(); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = [1].map(function () { return a; })[0]; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { class C { m() { return a; } } globalThis.R = new C().m(); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { let a2 = a; const c = a; globalThis.R = a2 + c; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { for (var i = 0; i < 2; i++) a += i; } globalThis.R = p.a;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = `${a}${typeof a}`; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = a ? 'y' : 'n'; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = [a, ...[a]].join(); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = JSON.stringify({a}); }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { globalThis.R = (({a}) => a)({a: 5}) + a; }"],
  ["var p = new Proxy({a: 1}, {}); with (p) { ({a} = {a: 6}); } globalThis.R = p.a;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { [a] = [8]; } globalThis.R = p.a;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { a ||= 3; a &&= 4; a ??= 5; } globalThis.R = p.a;"],
  ["var p = new Proxy({a: null}, {}); with (p) { a ??= 5; } globalThis.R = p.a;"],
  ["var p = new Proxy({a: 1}, {}); with (p) { a **= 3; a <<= 2; } globalThis.R = p.a;"],
  ["var p = new Proxy({}, {has: () => true, get: (t, k) => k === Symbol.unscopables ? undefined : undefined, set(t, k, v) { globalThis.R = String(k) + v; return true; }}); with (p) { newVar = 1; }"],
  ["'use strict'; try { eval('with ({}) {}'); } catch (e) { globalThis.R = e.name + ': ' + e.message; }"],
  ["var p = new Proxy({}, {has: () => false}); with (p) { newGlobalVarX = 3; } globalThis.R = typeof newGlobalVarX + ' ' + Object.keys(p).length;"],
  ["var log = []; var p = new Proxy({}, {has: (t, k) => { log.push('has:' + String(k)); return false; }}); with (p) { newGlobalVarY = 3; } globalThis.R = log.join();"],
];
for (const [body] of withs) add(`${body}\n`);

// `in`
const inOps = [
  "'a' in new Proxy({a: 1}, {})", "'b' in new Proxy({a: 1}, {})", "'toString' in new Proxy({}, {})", "0 in new Proxy([5], {})", "'length' in new Proxy([], {})",
  "Symbol.iterator in new Proxy([], {})", "'a' in new Proxy({}, {has: () => true})", "'a' in new Proxy({a: 1}, {has: () => false})", "1 in new Proxy({}, {has: (t, k) => typeof k})",
  "'a' in new Proxy({}, {has: (t, k) => k})", "'a' in new Proxy({}, {has: () => 'str'})", "'a' in new Proxy({}, {has: () => 0})", "'a' in new Proxy({}, {has: () => null})",
  "'a' in new Proxy({}, {has: () => { throw new TypeError('in'); }})", "'a' in new Proxy(function () {}, {})", "'prototype' in new Proxy(function () {}, {})", "'name' in new Proxy(function f() {}, {})",
  "'a' in new Proxy(new Proxy({a: 1}, {}), {})", "'a' in Object.create(new Proxy({a: 1}, {}))", "'a' in Object.create(new Proxy({}, {has: () => true}))", "'a' in 5",
  "'a' in new Proxy(Object.create({a: 1}), {})", "'a' in new Proxy({}, {has: (t, k) => k === 'a'}) && !('b' in new Proxy({}, {has: (t, k) => k === 'a'}))",
  "(({a}) => 1)(new Proxy({}, {has: () => false}))", "Object.hasOwn(new Proxy({a: 1}, {}), 'a')", "Object.hasOwn(new Proxy({}, {has: () => true}), 'a')",
  "new Proxy({a: 1}, {}).hasOwnProperty('a')", "new Proxy({}, {has: () => true}).hasOwnProperty('a')", "Object.prototype.hasOwnProperty.call(new Proxy({a: 1}, {getOwnPropertyDescriptor: () => undefined}), 'a')",
  "'a' in new Proxy({}, {has: () => true, get: () => { throw new Error('get called'); }})", "(() => { var log = []; 'a' in new Proxy({}, new Proxy({}, {get: (t, k) => { log.push(k); }})); return log.join(); })()",
  "(() => { var log = []; 'a' in new Proxy({}, new Proxy({has() { return true; }}, {get: (t, k) => { log.push(k); return t[k]; }})); return log.join(); })()",
];
for (const body of inOps) add(`globalThis.R = run(() => ${body});\n`);

// ---- class extends Proxy e super.
const classes = [
  "class A {}; class B extends new Proxy(A, {}) {}; return new B() instanceof A;",
  "class A {}; class B extends new Proxy(A, {}) {}; return Object.getPrototypeOf(B) !== A;",
  "class A { constructor() { this.n = new.target.name; } }; class B extends new Proxy(A, {}) {}; return new B().n;",
  "class A {}; var P = new Proxy(A, {construct(t, a, nt) { L.push('construct:' + (nt === B)); return Reflect.construct(t, a, nt); }}); class B extends P {}; new B(); return fmt(L);",
  "class A {}; var P = new Proxy(A, {construct(t, a, nt) { return {fake: 1}; }}); class B extends P { constructor() { super(); this.mine = 1; } }; return fmt(new B());",
  "class A {}; var P = new Proxy(A, {construct(t, a, nt) { return {fake: 1}; }}); class B extends P {}; return Object.getPrototypeOf(new B()) === Object.prototype;",
  "class A {}; var P = new Proxy(A, {construct() { return 1; }}); class B extends P {}; return new B();",
  "class A {}; var P = new Proxy(A, {get(t, k, r) { L.push(String(k)); return Reflect.get(t, k, r); }}); class B extends P {}; new B(); return fmt(L);",
  "class A {}; var P = new Proxy(A, {get(t, k, r) { L.push(String(k)); return Reflect.get(t, k, r); }}); class B extends P { static s = 1; m() {} }; return fmt(L);",
  "class A {}; var P = new Proxy(A, {getPrototypeOf() { L.push('gpo'); return Function.prototype; }}); class B extends P {}; return fmt(L);",
  "class A {}; var P = new Proxy(A, {get(t, k) { return k === 'prototype' ? null : t[k]; }}); class B extends P {}; return Object.getPrototypeOf(B.prototype);",
  "class A {}; var P = new Proxy(A, {get(t, k) { return k === 'prototype' ? 1 : t[k]; }}); class B extends P {}; return 1;",
  "class A {}; var P = new Proxy(A, {get(t, k) { return k === 'prototype' ? undefined : t[k]; }}); class B extends P {}; return 1;",
  "class A {}; var P = new Proxy(A, {get(t, k) { return k === 'prototype' ? Array.prototype : t[k]; }}); class B extends P {}; return Object.getPrototypeOf(B.prototype) === Array.prototype;",
  "var P = new Proxy({}, {}); class B extends P {}; return 1;",
  "var P = new Proxy(() => 1, {}); class B extends P {}; return 1;",
  "var P = new Proxy(function () {}, {}); class B extends P {}; return new B() instanceof B;",
  "var P = new Proxy(function* () {}, {}); class B extends P {}; return 1;",
  "var P = new Proxy(async function () {}, {}); class B extends P {}; return 1;",
  "var P = new Proxy(Object.assign(function () {}, {prototype: null}), {}); class B extends P {}; return Object.getPrototypeOf(B.prototype);",
  "var r = Proxy.revocable(function () {}, {}); r.revoke(); class B extends r.proxy {}; return 1;",
  "var r = Proxy.revocable(function () {}, {}); class B extends r.proxy {}; r.revoke(); return new B();",
  "var r = Proxy.revocable(class A {}, {}); class B extends r.proxy {}; r.revoke(); return B.name;",
  "class A { static s() { return 'S'; } m() { return 'M'; } }; class B extends new Proxy(A, {}) {}; return B.s() + new B().m();",
  "class A { static s() { return this.name; } }; class B extends new Proxy(A, {}) {}; return B.s();",
  "class A { m() { return 'A.m'; } }; class B extends new Proxy(A, {}) { m() { return super.m() + '+B'; } }; return new B().m();",
  "class A { static s() { return 'A.s'; } }; class B extends new Proxy(A, {}) { static s() { return super.s() + '+B'; } }; return B.s();",
  "class A {}; class B extends new Proxy(A, {get(t, k, r) { return k === 's' ? 'proxied' : Reflect.get(t, k, r); }}) { static t() { return super.s; } }; return B.t();",
  "class A {}; var P = new Proxy(A, {get(t, k, r) { return k === 's' ? 'proxied:' + (r === B) : Reflect.get(t, k, r); }}); class B extends P { static t() { return super.s; } }; return B.t();",
  "class A {}; var P = new Proxy(A.prototype, {get(t, k, r) { return k === 'q' ? 'protoGet:' + (r === o) : Reflect.get(t, k, r); }}); class B { m() { return super.q; } }; Object.setPrototypeOf(B.prototype, P); var o = new B(); return o.m();",
  "var P = new Proxy({set(k) { return 1; }}, {}); class B { m() { super.zz = 5; return this.zz; } }; Object.setPrototypeOf(B.prototype, P); return new B().m();",
  "var P = new Proxy({}, {set(t, k, v, r) { L.push('set:' + k + ':' + (r === o)); return Reflect.set(t, k, v, r); }}); class B { m() { super.zz = 5; return this.zz; } }; Object.setPrototypeOf(B.prototype, P); var o = new B(); return o.m() + fmt(L);",
  "class A { #p = 1; static has(o) { return #p in o; } }; return A.has(new Proxy(new A(), {})) + ' ' + A.has(new A());",
  "class A { #p = 1; get() { return this.#p; } }; return new A().get.call(new Proxy(new A(), {}));",
  "class A { #p = 1; get() { return this.#p; } }; class B extends A {}; return new Proxy(new B(), {}).get();",
  "class A { constructor() { return new Proxy(this, {get(t, k, r) { return k === 'x' ? 'trap' : Reflect.get(t, k, r); }}); } }; class B extends A { constructor() { super(); this.y = 1; } }; var o = new B(); return o.x + ' ' + o.y + ' ' + (o instanceof B);",
  "class A { constructor() { return new Proxy({}, {}); } }; class B extends A { #priv = 1; static t(o) { return #priv in o; } }; return B.t(new B());",
  "class A { constructor() { return new Proxy({}, {defineProperty(t, k, d) { L.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); }}); } }; class B extends A { f = 1; g = 2; }; new B(); return fmt(L);",
  "class A { constructor() { return new Proxy({}, {defineProperty() { return false; }}); } }; class B extends A { f = 1; }; new B(); return 1;",
  "class A { constructor() { return new Proxy({}, {set(t, k, v) { L.push('set:' + k); return true; }}); } }; class B extends A { f = 1; constructor() { super(); this.g = 2; } }; new B(); return fmt(L);",
  "var p = new Proxy(class A {}, {}); return String(p.name) + ' ' + typeof p + ' ' + p.length;",
  "var p = new Proxy(class A { constructor(a, b) {} }, {}); return p.length + ' ' + p.name;",
  "var p = new Proxy(class A {}, {}); return p();",
  "var p = new Proxy(class A {}, {apply() { return 'applied'; }}); return p();",
  "var p = new Proxy(class A { static x = 1 }, {}); return p.x;",
  "var p = new Proxy(class A { static #x = 1; static g() { return A.#x; } }, {}); return p.g();",
  "var p = new Proxy(class A { static #x = 1; static g() { return this.#x; } }, {}); return p.g();",
  "var p = new Proxy(class A { static { L.push('static'); } }, {}); return fmt(L);",
  "var p = new Proxy(class A extends Array {}, {}); return new p(3).length + ' ' + Array.isArray(new p()) + ' ' + (new p() instanceof Array);",
  "var p = new Proxy(class A extends Array {}, {}); return p.from([1, 2]) instanceof p;",
  "var p = new Proxy(class A extends Array {}, {}); return new p(1, 2).map(x => x) instanceof p;",
  "var p = new Proxy(class A extends Array {}, {get(t, k, r) { return k === Symbol.species ? Array : Reflect.get(t, k, r); }}); return new p(1, 2).map(x => x) instanceof p;",
  "class A extends Array {} ; var o = new A(1, 2, 3); var p = new Proxy(o, {}); return p.map(x => x) instanceof A;",
  "class A extends Array {} ; var o = new A(1, 2, 3); var p = new Proxy(o, {}); return Array.isArray(p) + ' ' + p.length + ' ' + (p instanceof A);",
  "class A extends Error {} ; var p = new Proxy(new A('m'), {}); return p.message + ' ' + (p instanceof A) + ' ' + (p instanceof Error) + ' ' + Object.prototype.toString.call(p);",
  "class A extends Promise {}; var P = new Proxy(A, {}); return P.resolve(1) instanceof A;",
  "class A extends Map {}; var P = new Proxy(A, {}); var m = new P([[1, 2]]); return m.get(1) + ' ' + (m instanceof A);",
  "class A extends Map {}; var m = new Proxy(new A([[1, 2]]), {}); return m.get(1);",
  "class A extends Function {}; var P = new Proxy(A, {}); return new P('return 5')();",
  "class A extends Object { constructor() { return super(); } }; var P = new Proxy(A, {}); return new P() instanceof A;",
  "class A extends null {}; var P = new Proxy(A, {}); return typeof P;",
  "class A extends null {}; var P = new Proxy(A, {}); class B extends P {}; return 1;",
  "class A extends null {}; var P = new Proxy(A, {}); class B extends P {}; return new B();",
  "class A extends null { constructor() { return Object.create(A.prototype); } }; var P = new Proxy(A, {}); class B extends P {}; return new B() instanceof A;",
];
for (const body of classes) add(`globalThis.R = run(() => { ${body} });\n`);

// ---- Mais combinações de trap x tipo de alvo, tratando tipo de chave.
const keyKinds = ["'a'", "1", "Symbol.iterator", "Symbol('desc')", "''", "'__proto__'", "'constructor'", "'length'", "-0", "'01'"];
for (const key of keyKinds) {
  add(`var p = new Proxy({}, {get(t, k) { L.push(typeof k + ':' + String(k)); }, set(t, k) { L.push('set:' + typeof k + ':' + String(k)); return true; }, has(t, k) { L.push('has:' + typeof k + ':' + String(k)); return true; }, deleteProperty(t, k) { L.push('del:' + typeof k + ':' + String(k)); return true; }, getOwnPropertyDescriptor(t, k) { L.push('gopd:' + typeof k + ':' + String(k)); }, defineProperty(t, k) { L.push('def:' + typeof k + ':' + String(k)); return true; }});\nglobalThis.R = run(() => { var k = ${key}; p[k]; p[k] = 1; k in p; delete p[k]; Object.getOwnPropertyDescriptor(p, k); Object.defineProperty(p, k, {}); return fmt(L); });\n`);
}
// Argumentos recebidos por cada trap.
const argProbes = [
  "get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf", "setPrototypeOf", "isExtensible", "preventExtensions", "apply", "construct",
];
const argOp = {
  get: "p.a", set: "p.a = 1", has: "'a' in p", deleteProperty: "delete p.a", defineProperty: "Object.defineProperty(p, 'a', {value: 1})",
  getOwnPropertyDescriptor: "Object.getOwnPropertyDescriptor(p, 'a')", ownKeys: "Object.keys(p)", getPrototypeOf: "Object.getPrototypeOf(p)",
  setPrototypeOf: "Object.setPrototypeOf(p, null)", isExtensible: "Object.isExtensible(p)", preventExtensions: "Object.preventExtensions(p)",
  apply: "p.call(7, 1, 2)", construct: "new p(1, 2)",
};
for (const trap of argProbes) {
  const target = trap === "apply" || trap === "construct" ? "function T() {}" : "{}";
  add(`var tgt = ${target};\nvar h = {${trap}() { L.push(arguments.length + ':' + (arguments[0] === tgt) + ':' + (this === h)); return Reflect.${trap}(...arguments); }};\nvar p = new Proxy(tgt, h);\nglobalThis.R = run(() => ${argOp[trap]}) + ' | ' + fmt(L);\n`);
  add(`var tgt = ${target};\nvar h = {${trap}(...a) { L.push(a.map(x => typeof x).join()); return Reflect.${trap}(...a); }};\nvar p = new Proxy(tgt, h);\nglobalThis.R = run(() => ${argOp[trap]}) + ' | ' + fmt(L);\n`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "proxy-golden-"));
const file = path.join(dir, "proxy_case.js");
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
  const original = body;
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("proxy", lines));
fs.rmSync(dir, { recursive: true, force: true });
