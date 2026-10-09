// Gera tests/golden/function_proto_bun.tsv: Function.prototype (toString de todas as categorias, bind, call/apply,
// Symbol.hasInstance, name/length, caller/arguments, new.target e Reflect.construct, instanceof, recursão profunda)
// medidos no bun 1.4.2. Complementa tests/golden/globals_bun.tsv e function_error_bun.tsv.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-function-proto-golden.js > tests/golden/function_proto_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRE =
  "var S = function (v) { if (Object.is(v, -0)) return '-0'; if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; if (typeof v === 'function') return 'function'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var T = function (f) { try { return S(f()) } catch (e) { return e.name + ': ' + e.message } };\n";
const programs = [];
const add = body => programs.push(PRE + body);
const show = expr => add("R = T(() => " + expr + ")");
let sampleCounter = 0;
// Famílias geradas em volume: entra uma a cada três, para o golden ficar em cerca de 1500 programas.
const sampled = emit => { if (sampleCounter++ % 3 === 0) emit(); };

// ---- 1. toString de todas as categorias.
const sources = [
  "function f() {}", "function f(a, b) { return a + b }", "function  g ( ) { }", "function* g() { yield 1 }",
  "async function a() {}", "async function* ag() {}", "(function () {})", "(function () { /* c */ })",
  "(() => 1)", "(a => a)", "(async () => {})", "(async a => a)", "(class A {})", "(class { })",
  "(class A extends Object { constructor() { super() } })", "({ m() {} }).m", "({ get g() { return 1 } })",
  "({ *gen() {} }).gen", "({ async am() {} }).am", "({ async *agm() {} }).agm", "({ 'quoted key'() {} })['quoted key']",
  "({ 1() {} })[1]", "({ [`c${1}`]() {} }).c1", "({ [Symbol.iterator]() {} })[Symbol.iterator]",
  "({ [Symbol('desc')]() {} })[Object.getOwnPropertySymbols({ [Symbol('desc')]() {} })[0]]",
  "({ f: function () {} }).f", "({ f: () => {} }).f", "({ f: class {} }).f", "(class { static m() {} }).m",
  "(class { static get s() { return 1 } })", "(class { #p() {} static t(o) { return o.#p } })",
  "(class { static { } })", "(class { constructor(a, b) {} })", "new Function('a', 'b', 'return a')",
  "new Function('a, b', 'c', 'return a')", "new Function()", "new Function('')", "new Function('/*x*/')",
  "new Function('a', '//c')", "Function('a', 'b', 'return 1')", "Object.getPrototypeOf(async function () {})",
  "Object.getPrototypeOf(function* () {})", "Object.getPrototypeOf(async function* () {})",
  "(function () {}).bind()", "(function f() {}).bind(null, 1)", "(() => {}).bind()", "(class A {}).bind()",
  "Function.prototype", "Function.prototype.bind.call(Function.prototype)", "Function", "Function.prototype.toString",
  "Function.prototype.call", "Function.prototype.apply", "Function.prototype.bind", "Object", "Array.prototype.push",
  "Math.max", "JSON.parse", "Symbol", "Symbol.for", "Promise", "Promise.resolve", "Proxy", "Reflect.apply",
  "parseInt", "eval", "Date", "RegExp", "Error", "Map", "BigInt", "String.prototype.at",
  "new Proxy(function () {}, {})", "new Proxy(class {}, {})", "new Proxy(() => {}, {})", "new Proxy(Math.max, {})",
  "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get", "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
  "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set", "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get",
  "Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get", "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').set", "Object.getOwnPropertyDescriptor(Map, Symbol.species).get",
  "Array.prototype[Symbol.iterator]", "String.prototype[Symbol.iterator]", "Date.prototype[Symbol.toPrimitive]",
  "RegExp.prototype[Symbol.replace]", "Function.prototype[Symbol.hasInstance]", "Symbol.prototype[Symbol.toPrimitive]",
  "Object.getPrototypeOf(Int8Array)", "Object.getPrototypeOf(Int8Array).from", "Int8Array", "ArrayBuffer", "DataView",
  "(function () { return function () {} })()", "(function () { return () => 1 })()",
  "Object.getOwnPropertyDescriptor({ get [Symbol('a')]() { return 1 } }, Object.getOwnPropertySymbols({ get [Symbol('a')]() { return 1 } })[0])",
  "Reflect.ownKeys(function () {}).join()", "Object.getOwnPropertyDescriptor({ set x(v) {} }, 'x').set",
  "Object.getOwnPropertyDescriptor({ get x() { return 1 } }, 'x').get",
  "Object.getOwnPropertyDescriptor(class { static get x() { return 1 } }, 'x').get",
  "Object.getOwnPropertyDescriptor(class { get x() { return 1 } }.prototype, 'x').get",
  "(function () {}).constructor", "(async function () {}).constructor", "(function* () {}).constructor",
  "(async function* () {}).constructor", "(class {}).constructor",
];
for (const s of sources) {
  show("(" + s + ").toString()");
  show("Function.prototype.toString.call(" + s + ")");
  show("String(" + s + ")");
  show("typeof (" + s + ")");
}
// Enumera as builtins do próprio bun: nome, length e toString de cada função própria.
const roots = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "Math", "JSON", "Reflect", "Map", "Set",
  "WeakMap", "WeakSet", "WeakRef", "Promise", "RegExp", "Date", "Error", "BigInt", "ArrayBuffer", "DataView",
  "Int8Array", "Float64Array", "Proxy", "Atomics", "Intl", "globalThis",
];
const skipGlobal = new Set(["Bun", "process", "require", "console", "fetch", "performance", "setTimeout", "setInterval", "setImmediate",
  "queueMicrotask", "structuredClone", "reportError", "alert", "confirm", "prompt", "atob", "btoa", "addEventListener",
  "removeEventListener", "dispatchEvent", "clearTimeout", "clearInterval", "clearImmediate", "postMessage", "navigator", "self"]);
for (const root of roots) {
  let target;
  try { target = eval(root); } catch { continue; }
  const targets = [[root, target]];
  if (target && target.prototype && typeof target.prototype === "object") targets.push([root + ".prototype", target.prototype]);
  for (const [label, obj] of targets) {
    for (const key of Reflect.ownKeys(obj)) {
      if (label === "globalThis" && (typeof key !== "string" || skipGlobal.has(key) || !/^[A-Za-z]+$/.test(key))) continue;
      if (typeof key === "symbol") continue;
      if (!/^[A-Za-z_$][\w$]*$/.test(key)) continue;
      const d = Object.getOwnPropertyDescriptor(obj, key);
      const access = label + "." + key;
      if (typeof d.value === "function") {
        const f = label === "globalThis" ? key : access;
        sampled(() => show("[" + f + ".toString(), " + f + ".name, " + f + ".length].join('|')"));
      } else if (d.get || d.set) {
        const d2 = "Object.getOwnPropertyDescriptor(" + label + ", " + JSON.stringify(key) + ")";
        show("[" + d2 + ".get && " + d2 + ".get.toString(), " + d2 + ".set && " + d2 + ".set.toString(), " + d2 + ".get && " + d2 + ".get.name, " + d2 + ".get && " + d2 + ".get.length].join('|')");
      }
    }
  }
}

// ---- 2. bind.
const bindTargets = {
  "function x(a, b, c) {}": "x", "function () {}": "anon", "(a, b) => 0": "arrow", "class K { constructor(a) {} }": "K",
  "async function y(a) {}": "y", "function* z() {}": "z", "Math.max": "max", "Array": "Array", "Object.create(null)": "nul",
};
const bindArgs = ["", "null", "null, 1", "null, 1, 2", "null, 1, 2, 3, 4", "undefined, 1", "{}", "0", "'s'"];
for (const [t, label] of Object.entries(bindTargets)) {
  for (const a of bindArgs) {
    show("(" + t + ").bind(" + a + ").name");
    show("(" + t + ").bind(" + a + ").length");
    show("(" + t + ").bind(" + a + ").toString()");
    show("Object.getOwnPropertyNames((" + t + ").bind(" + a + ")).join()");
  }
  show("(" + t + ").bind().bind().bind().name");
  show("(" + t + ").bind(null, 1).bind(null, 2).length");
  show("(" + t + ").bind().hasOwnProperty('prototype')");
  show("Object.getPrototypeOf((" + t + ").bind()) === Object.getPrototypeOf(" + t + ")");
  show("typeof (" + t + ").bind()");
  show("Object.getOwnPropertyDescriptor((" + t + ").bind(), 'name')");
  show("Object.getOwnPropertyDescriptor((" + t + ").bind(), 'length')");
}
const bindMisc = [
  "(function () { return this }).bind(5)() === 5",
  "(function () { return typeof this }).bind(5)()",
  "(function () { return this }).bind(null)() === globalThis",
  "(function () { 'use strict'; return this }).bind(null)() === null",
  "(function () { 'use strict'; return this }).bind(undefined)() === undefined",
  "(function () { return this === globalThis }).bind(undefined)()",
  "(function () { return [].slice.call(arguments).join() }).bind(null, 1, 2)(3, 4)",
  "(function () { return arguments.length }).bind(null, ...new Array(1000).fill(0))(...new Array(1000).fill(0))",
  "(function () { return new.target === undefined }).bind()()",
  "new ((function F(a) { this.a = a }).bind(null, 5))().a",
  "new ((function F(a) { this.a = a }).bind(null, 5))() instanceof (function F(a) { this.a = a })",
  "(function () { function F() {}; var B = F.bind(); return new B instanceof F })()",
  "(function () { function F() {}; var B = F.bind(); return new B instanceof B })()",
  "(function () { function F() {}; var B = F.bind(); return Object.getPrototypeOf(new B) === F.prototype })()",
  "(function () { function F() {}; var B = F.bind(); return {} instanceof B })()",
  "(function () { function F() {}; var B = F.bind().bind(); return new F instanceof B })()",
  "(function () { function F() {}; var B = F.bind(); B.prototype = 1; return new F instanceof B })()",
  "(function () { function F() {}; var B = F.bind(); Object.defineProperty(B, 'prototype', { value: {} }); return new B instanceof B })()",
  "(function () { function F() {}; var B = F.bind(); return Function.prototype[Symbol.hasInstance].call(B, new F) })()",
  "(function () { function F() { this.nt = new.target === F } ; var B = F.bind(); return new B().nt })()",
  "(function () { function F() { this.nt = new.target } ; var B = F.bind(); return new B().nt === F })()",
  "(function () { class A { constructor() { this.nt = new.target } }; var B = A.bind(); return Reflect.construct(B, [], Object).nt === Object })()",
  "(function () { class A { constructor() { this.nt = new.target } }; var B = A.bind(); return Reflect.construct(B, [], B).nt === A })()",
  "(function () { class A {}; var B = A.bind(); return B() })()",
  "(function () { class A {}; var B = A.bind(); return typeof new B })()",
  "(() => 1).bind().call.name",
  "(function () { return 1 }).bind().call()",
  "(function () {}).bind.call(1)",
  "(function () {}).bind.call({})",
  "(function () {}).bind.call(null)",
  "(function () {}).bind.call(undefined)",
  "Function.prototype.bind.call(Symbol())",
  "Function.prototype.bind.call(/x/)",
  "Function.prototype.bind.call(new Proxy(function () {}, {}), null).name",
  "Function.prototype.bind.call(new Proxy(class {}, {}), null)() ",
  "new (Function.prototype.bind.call(new Proxy(class { constructor() { this.x = 1 } }, {}), null))().x",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: 1 }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: Symbol('s') }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: 'n' }); return f.bind().name })()",
  "(function () { var f = function () {}; delete f.name; return JSON.stringify(f.bind().name) })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { get() { return 'g' } }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { get() { throw new RangeError('boom') } }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: {} }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: null }); return f.bind().name })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 7 }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 7 }); return f.bind(null, 1, 2).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 7 }); return f.bind(null, ...new Array(10).fill(0)).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: Infinity }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: Infinity }); return f.bind(null, 1, 2, 3).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: -Infinity }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: -5 }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 2.7 }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: -2.7 }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: NaN }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: '3' }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 2 ** 53 }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 2 ** 31 }); return f.bind(null, 1).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 1e300 }); return f.bind(null, 1).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: 2n }); return f.bind().length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { value: {} }); return f.bind().length })()",
  "(function () { var f = function () {}; delete f.length; return f.bind(null, 1).length })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'length', { get() { throw new SyntaxError('len') } }); return f.bind().length })()",
  "(function () { var f = function (a, b) {}; Object.setPrototypeOf(f, null); return f.bind().length })()",
  "(function () { var f = function (a, b) {}; Object.setPrototypeOf(f, {}); return f.bind() })()",
  "(function () { var f = function (a, b) {}; var p = {}; Object.setPrototypeOf(f, p); return Object.getPrototypeOf(f.bind()) === p })()",
  "(function () { var f = function (a, b) {}; Object.setPrototypeOf(f, null); return Object.getPrototypeOf(f.bind()) })()",
  "(function () { var p = new Proxy(function () {}, { getPrototypeOf() { throw new EvalError('gp') } }); return Function.prototype.bind.call(p) })()",
  "(function () { var log = []; var p = new Proxy(function () {}, { getOwnPropertyDescriptor(t, k) { log.push('gopd ' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { log.push('get ' + String(k)); return t[k] }, getPrototypeOf(t) { log.push('gpo'); return Object.getPrototypeOf(t) }, has(t, k) { log.push('has ' + String(k)); return k in t } }); Function.prototype.bind.call(p); return log.join() })()",
  "(function () { var o = { get length() { return 3 }, get name() { return 'nm' } }; return Function.prototype.bind.call(Object.setPrototypeOf(function () {}, o)).length })()",
  "Function.prototype.bind.call(function () {}, null, ...[1, 2, 3]).length",
  "Function.prototype.bind.length",
  "Function.prototype.bind.name",
  "Object.getOwnPropertyDescriptor(Function.prototype.bind, 'length').configurable",
  "Object.getOwnPropertyDescriptor(Function.prototype.bind, 'name').writable",
  "(function () { var b = (function () {}).bind(); b.name = 'x'; return b.name })()",
  "(function () { var b = (function () {}).bind(); b.length = 9; return b.length })()",
  "(function () { var b = (function () {}).bind(); delete b.name; return JSON.stringify(b.name) })()",
  "(function () { var b = (function () {}).bind(); Object.defineProperty(b, 'name', { value: 'q' }); return b.name })()",
  "(function () { var b = (function () {}).bind(); b.extra = 1; return Object.keys(b).join() })()",
  "(function () { var b = (function () {}).bind(); b.prototype = {}; return Object.keys(b).join() })()",
  "(function () { var b = (function () {}).bind(); return b.hasOwnProperty('caller') + ',' + b.hasOwnProperty('arguments') })()",
  "(function () { var b = (function () {}).bind(); return b.caller })()",
  "(function () { var b = (function () {}).bind(); return b.arguments })()",
  "(function () { var b = (function () {}).bind(); b.caller = 1 })()",
  "(function () { var b = (function () {}).bind(); b.arguments = 1 })()",
  "(function () { 'use strict'; var b = (function () {}).bind(); return b.caller })()",
  "Object.isExtensible((function () {}).bind())",
  "Object.isFrozen((function () {}).bind())",
  "Object.isFrozen(Object.freeze((function () {}).bind()))",
  "(function () { var b = Object.freeze((function () {}).bind()); b.name = 'x'; return b.name })()",
  "(function () { var s = Symbol('d'); var f = function () {}; Object.defineProperty(f, 'name', { value: s }); return f.bind().name })()",
  "((function () {}).bind().name === 'bound ')",
  "((function () {}).bind().bind().name === 'bound bound ')",
  "(function () { const o = { m() {} }; return o.m.bind().name })()",
  "(function () { const o = { get m() { return 1 } }; return Object.getOwnPropertyDescriptor(o, 'm').get.bind().name })()",
  "(function () { const o = { set m(v) {} }; return Object.getOwnPropertyDescriptor(o, 'm').set.bind().name })()",
  "(function () { const o = { [Symbol('sd')]() {} }; return o[Object.getOwnPropertySymbols(o)[0]].bind().name })()",
  "(function () { const o = { [Symbol()]() {} }; return JSON.stringify(o[Object.getOwnPropertySymbols(o)[0]].bind().name) })()",
  "(function () { var BF = (function () {}).bind(); return Object.getPrototypeOf(BF) === Function.prototype })()",
  "(function () { var BF = (async function () {}).bind(); return Object.getPrototypeOf(BF) === Object.getPrototypeOf(async function () {}) })()",
  "(function () { var BF = (function* () {}).bind(); return typeof BF().next })()",
  "(async function () {}).bind()() instanceof Promise",
  "(function () { var B = (class { static s = 1 }).bind(); return B.s })()",
  "(function () { var B = Date.bind(null, 2020, 0, 1); return new B().getFullYear() })()",
  "(function () { var B = Date.bind(null, 2020, 0, 1); return typeof B() })()",
  "(function () { var B = Array.bind(null, 3); return new B().length })()",
  "(function () { var B = Array.bind(null, 3); return Array.isArray(B()) })()",
  "(function () { var B = Number.bind(null, '12'); return B() + new B().valueOf() })()",
  "(function () { var B = Promise.bind(null, function (r) { r(1) }); return new B() instanceof Promise })()",
  "(function () { var B = Promise.bind(); return B() })()",
  "(function () { var B = Map.bind(); return B() })()",
  "(function () { var B = Symbol.bind(); return new B })()",
  "(function () { var B = BigInt.bind(null, 1); return new B })()",
  "(function () { var B = Error.bind(null, 'm'); return B().message + new B().message })()",
  "(function () { var B = Function.bind(null, 'return 7'); return B()() })()",
  "(function () { var B = Proxy.bind(null, {}); return B() })()",
  "(function () { var B = Proxy.bind(null, {}, {}); return typeof new B })()",
  "Reflect.construct(function () {}.bind(), [], function () {}.bind())",
  "Reflect.construct(function () {}, [], function () {}.bind()) instanceof Object",
  "(() => {}).bind().prototype",
  "new ((() => {}).bind())",
  "new ((async function () {}).bind())",
  "new ((function* () {}).bind())",
  "new (({ m() {} }).m.bind())",
  "new (Math.max.bind())",
  "new (Math.max.bind())()",
  "new (Function.prototype.bind.call(Symbol))",
  "new (Function.prototype.call.bind(function () {}))",
];
for (const e of bindMisc) show(e);
for (let n = 0; n <= 12; n++) {
  const params = Array.from({ length: n }, (_, i) => "p" + i).join(",");
  for (let k = 0; k <= 14; k += 2) {
    show("(function (" + params + ") {}).bind(null" + ", 0".repeat(k) + ").length");
  }
}

// ---- 3. call, apply e Reflect.apply.
const callMisc = [
  "(function () { return this }).call(1) instanceof Number",
  "(function () { 'use strict'; return this }).call(1)",
  "(function () { 'use strict'; return this }).call()",
  "(function () { 'use strict'; return this }).call(null)",
  "(function () { return this === globalThis }).call(null)",
  "(function () { return typeof this }).call('s')",
  "(function () { return typeof this }).call(Symbol())",
  "(function () { return typeof this }).call(1n)",
  "(function () { return arguments.length }).apply(null)",
  "(function () { return arguments.length }).apply(null, undefined)",
  "(function () { return arguments.length }).apply(null, null)",
  "(function () { return arguments.length }).apply(null, [])",
  "(function () { return arguments.length }).apply(null, [1, 2, 3])",
  "(function () { return arguments.length }).apply(null, { length: 3 })",
  "(function () { return [].slice.call(arguments).join() }).apply(null, { length: 3, 0: 'a', 2: 'c' })",
  "(function () { return arguments.length }).apply(null, { length: -1 })",
  "(function () { return arguments.length }).apply(null, { length: 2.9 })",
  "(function () { return arguments.length }).apply(null, { length: '3' })",
  "(function () { return arguments.length }).apply(null, { length: NaN })",
  "(function () { return arguments.length }).apply(null, { length: Infinity })",
  "(function () { return arguments.length }).apply(null, { length: 2 ** 32 })",
  "(function () { return arguments.length }).apply(null, { length: 2 ** 20 })",
  "(function () { return arguments.length }).apply(null, { length: 2 ** 24 })",
  "(function () { return arguments.length }).apply(null, { length: 2 ** 31 })",
  "(function () { return arguments.length }).apply(null, { length: 2 ** 53 })",
  "(function () { return arguments.length }).apply(null, { length: 1e6 })",
  "(function () { return arguments.length }).apply(null, new Array(1e5).fill(0))",
  "(function () { return arguments.length }).apply(null, new Array(1e6).fill(0))",
  "(function () { return arguments.length }).apply(null, new Array(5e6).fill(0))",
  "Math.max.apply(null, new Array(1e5).fill(1))",
  "Math.max.apply(null, new Array(2e5).fill(1))",
  "String.fromCharCode.apply(null, new Array(1e5).fill(65)).length",
  "Array.prototype.push.apply([], new Array(1e5).fill(0))",
  "Math.max(...new Array(1e5).fill(1))",
  "Math.max(...new Array(3e5).fill(1))",
  "Math.max.apply(null, { length: 2 ** 32 })",
  "(function () {}).apply(null, 1)",
  "(function () {}).apply(null, 'ab')",
  "(function () {}).apply(null, true)",
  "(function () {}).apply(null, Symbol())",
  "(function () {}).apply(null, 1n)",
  "(function () {}).apply(null, function () {})",
  "(function () { return arguments.length }).apply(null, function (a, b) {})",
  "(function () { return arguments.length }).apply(null, new Set([1, 2]))",
  "(function () { return arguments.length }).apply(null, 'ab')",
  "(function () { return arguments.length }).apply(null, (function () { return arguments })(1, 2, 3))",
  "(function () { return arguments.length }).apply(null, new Uint8Array(4))",
  "(function () { return arguments.length }).apply(null, { get length() { throw new RangeError('len') } })",
  "(function () { return arguments.length }).apply(null, { length: 1, get 0() { throw new RangeError('idx') } })",
  "(function () { return arguments.length }).apply(null, new Proxy([1, 2], {}))",
  "(function () { var log = []; (function () {}).apply(null, new Proxy([1, 2], { get(t, k) { log.push(String(k)); return t[k] } })); return log.join() })()",
  "(function () { var log = []; (function () {}).apply(null, new Proxy({ length: 2 }, { get(t, k) { log.push(String(k)); return t[k] } })); return log.join() })()",
  "(function () { var log = []; (function () {}).apply(null, new Proxy({ length: 2 }, { has(t, k) { log.push('has ' + String(k)); return k in t }, get(t, k) { return t[k] } })); return log.join() })()",
  "Function.prototype.apply.call(1)",
  "Function.prototype.apply.call({})",
  "Function.prototype.apply.call(null)",
  "Function.prototype.apply.call(undefined)",
  "Function.prototype.apply.call(Symbol())",
  "Function.prototype.apply.call(/re/)",
  "Function.prototype.apply.call(class {})",
  "Function.prototype.apply.call(class A {})",
  "Function.prototype.apply.call(new Proxy({}, {}))",
  "Function.prototype.apply.call(new Proxy(function () { return 5 }, {}))",
  "Function.prototype.call.call(1)",
  "Function.prototype.call.call({})",
  "Function.prototype.call.call(null)",
  "Function.prototype.call.call(undefined)",
  "Function.prototype.call.call(class {})",
  "Function.prototype.call.call(class A {})",
  "Function.prototype.call.call(function () { return 1 })",
  "Function.prototype.call.call(Function.prototype.call.bind(function () { return this }), 5) instanceof Number",
  "Function.prototype.call.call(Function.prototype.call, function () { return 2 })",
  "Function.prototype.call.apply(function () { return this }, [7]) instanceof Number",
  "Function.prototype.apply.apply(function () { return arguments.length }, [null, [1, 2]])",
  "Function.prototype.apply.call(function () { return arguments.length }, null, [1, 2, 3])",
  "Function.prototype.call.bind(function () { return this })(5) instanceof Number",
  "Function.prototype.call.length",
  "Function.prototype.call.name",
  "Function.prototype.apply.length",
  "Function.prototype.apply.name",
  "Function.prototype.toString.length",
  "Function.prototype.toString.call(1)",
  "Function.prototype.toString.call({})",
  "Function.prototype.toString.call(null)",
  "Function.prototype.toString.call(undefined)",
  "Function.prototype.toString.call('s')",
  "Function.prototype.toString.call(Symbol())",
  "Function.prototype.toString.call(new Proxy({}, {}))",
  "Function.prototype.toString.call(new Proxy(function () {}, {}))",
  "Function.prototype.toString.call(new Proxy(new Proxy(function () {}, {}), {}))",
  "Function.prototype.toString.call(class {})",
  "Function.prototype.toString.call(/x/)",
  "Function.prototype.toString.apply()",
  "Function.prototype.toString.apply(function z() {})",
  "Reflect.apply(function () { return this }, 1, []) instanceof Number",
  "Reflect.apply(function () { return arguments.length }, null, { length: 4 })",
  "Reflect.apply(function () {}, null)",
  "Reflect.apply(function () {}, null, null)",
  "Reflect.apply(function () {}, null, undefined)",
  "Reflect.apply(function () {}, null, 1)",
  "Reflect.apply(1, null, [])",
  "Reflect.apply({}, null, [])",
  "Reflect.apply(null, null, [])",
  "Reflect.apply(class {}, null, [])",
  "Reflect.apply()",
  "Reflect.apply(function () {})",
  "Reflect.apply(Math.max, null, [1, 5, 2])",
  "Reflect.construct(function () {}, [])",
  "Reflect.construct(function () {}, 1)",
  "Reflect.construct(function () {}, null)",
  "Reflect.construct(function () {}, undefined)",
  "Reflect.construct(1, [])",
  "Reflect.construct(() => {}, [])",
  "Reflect.construct(function () {}, [], 1)",
  "Reflect.construct(function () {}, [], () => {})",
  "Reflect.construct(function () {}, [], {})",
  "Reflect.construct(function () {}, [], Math.max)",
  "Reflect.construct(function () {}, [], null)",
  "Reflect.construct(function () {}, [], undefined)",
  "Reflect.construct(function () {})",
  "Reflect.construct()",
  "Reflect.construct(Math.max, [])",
  "Reflect.construct(async function () {}, [])",
  "Reflect.construct(function* () {}, [])",
  "Reflect.construct(class {}, [], function () {}) instanceof Object",
  "Reflect.construct(function () {}, { length: 2 })",
  "Reflect.construct(function () { this.n = arguments.length }, { length: 2 }).n",
  "Reflect.construct(function () { this.n = arguments.length }, new Array(1e5).fill(0)).n",
  "(function () { return arguments.length }).call()",
  "(function () { return arguments.length }).call(null, 1, 2, 3)",
  "(function () { return arguments.length }).call(null, ...new Array(1000).fill(0))",
];
for (const e of callMisc) show(e);
const arrayLikes = ["[]", "[1]", "[1,2]", "[,]", "[1,,3]", "{length:0}", "{length:1}", "{length:2,0:'x',1:'y'}", "'abc'", "new String('ab')", "(function(){return arguments})(1,2)", "new Int8Array(2)", "{length:-3}", "{length:'2',0:1,1:2}", "{length:4.5}", "{length:true}", "{length:null}", "{length:{}}", "{length:{valueOf(){return 2}}}"];
for (const a of arrayLikes) {
  show("(function () { return arguments.length + ':' + Array.prototype.join.call(arguments, '|') }).apply(null, " + a + ")");
  show("Reflect.apply(function () { return arguments.length }, null, " + a + ")");
  show("Math.max.apply(null, " + a + ")");
}

// ---- 4. Function.prototype[Symbol.hasInstance] e instanceof.
const instValues = ["{}", "[]", "1", "null", "undefined", "function () {}", "Object.create(null)", "new (function F() {})", "'s'", "Symbol()", "1n", "new Proxy({}, {})"];
const instTargets = ["Object", "Array", "Function", "function () {}", "(function () {}).bind()", "class {}", "() => {}", "Math.max", "(function () {}).bind().bind()", "new Proxy(function () {}, {})", "async function () {}", "Symbol", "Number"];
for (const v of instValues) {
  for (const t of instTargets) {
    show("(" + v + ") instanceof (" + t + ")");
  }
  show("Function.prototype[Symbol.hasInstance].call(Object, " + v + ")");
}
const instMisc = [
  "({}) instanceof {}",
  "({}) instanceof 1",
  "({}) instanceof null",
  "({}) instanceof undefined",
  "({}) instanceof 'str'",
  "({}) instanceof Symbol()",
  "({}) instanceof []",
  "({}) instanceof Math",
  "({}) instanceof new Proxy({}, {})",
  "({}) instanceof (() => {})",
  "({}) instanceof (async () => {})",
  "({}) instanceof Math.max",
  "(function () { var F = function () {}; F.prototype = 1; return ({}) instanceof F })()",
  "(function () { var F = function () {}; F.prototype = null; return ({}) instanceof F })()",
  "(function () { var F = function () {}; F.prototype = 'x'; return 1 instanceof F })()",
  "(function () { var F = function () {}; F.prototype = undefined; return 1 instanceof F })()",
  "(function () { var F = function () {}; F.prototype = undefined; return ({}) instanceof F })()",
  "(function () { var F = function () {}; delete F.prototype; return ({}) instanceof F })()",
  "(function () { var F = function () {}; F.prototype = Symbol(); return ({}) instanceof F })()",
  "(function () { var F = function () {}; F.prototype = function () {}; return ({}) instanceof F })()",
  "(function () { var F = function () {}; F.prototype = function () {}; return F.prototype instanceof F })()",
  "(function () { var F = function () {}; F.prototype = {}; var o = Object.create(F.prototype); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new F; F.prototype = {}; return o instanceof F })()",
  "(function () { var F = function () {}; var o = new F; Object.setPrototypeOf(o, null); return o instanceof F })()",
  "(function () { var F = function () {}; return Object.create(Object.create(F.prototype)) instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { return F.prototype } }); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { throw new URIError('gp') } }); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { return 1 } }); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { return null } }); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { return undefined } }); return o instanceof F })()",
  "(function () { var F = function () {}; var o = new Proxy({}, { getPrototypeOf() { return F.prototype } }); return Function.prototype[Symbol.hasInstance].call(F, o) })()",
  "(function () { var o = { [Symbol.hasInstance](v) { return v === 1 } }; return 1 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance](v) { return 'yes' } }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance](v) { return 0 } }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]: 1 }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]: {} }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]: null }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]: undefined }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]: undefined }; return 2 instanceof o })()",
  "(function () { var o = { get [Symbol.hasInstance]() { throw new EvalError('get') } }; return 2 instanceof o })()",
  "(function () { var o = { [Symbol.hasInstance]() { throw new EvalError('call') } }; return 2 instanceof o })()",
  "(function () { var F = function () {}; Object.defineProperty(F, Symbol.hasInstance, { value: () => true }); return 1 instanceof F })()",
  "(function () { var F = function () {}; F[Symbol.hasInstance] = () => true; return 1 instanceof F })()",
  "(function () { class A { static [Symbol.hasInstance](v) { return v === 3 } }; return [3 instanceof A, 4 instanceof A].join() })()",
  "(function () { class A { static [Symbol.hasInstance](v) { return this === A } }; class B extends A {}; return [1 instanceof A, 1 instanceof B].join() })()",
  "(function () { class A { static [Symbol.hasInstance](v) { return super[Symbol.hasInstance](v) } }; return [new A instanceof A, ({}) instanceof A].join() })()",
  "(function () { var F = function () {}; var B = F.bind(); F[Symbol.hasInstance] = () => 'f'; return (new F) instanceof B })()",
  "(function () { var F = function () {}; var B = F.bind(); B[Symbol.hasInstance] = () => 'b'; return (new F) instanceof B })()",
  "(function () { var F = function () {}; var B = F.bind(); Object.defineProperty(F, 'prototype', { get() { throw new EvalError('proto') } }); return ({}) instanceof B })()",
  "(function () { var F = function () {}; Object.defineProperty(F, 'prototype', { get() { throw new EvalError('proto') } }); return ({}) instanceof F })()",
  "(function () { var F = function () {}; Object.defineProperty(F, 'prototype', { get() { throw new EvalError('proto') } }); return 1 instanceof F })()",
  "(function () { var F = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); return [] instanceof F })()",
  "(function () { var F = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? 1 : t[k] } }); return [] instanceof F })()",
  "(function () { var F = new Proxy(function () {}, { get(t, k) { return k === Symbol.hasInstance ? () => 'p' : t[k] } }); return [] instanceof F })()",
  "Function.prototype[Symbol.hasInstance].call(1, {})",
  "Function.prototype[Symbol.hasInstance].call({}, {})",
  "Function.prototype[Symbol.hasInstance].call(null, {})",
  "Function.prototype[Symbol.hasInstance].call(function () {}, 1)",
  "Function.prototype[Symbol.hasInstance].call()",
  "Function.prototype[Symbol.hasInstance].call(Object)",
  "Function.prototype[Symbol.hasInstance].call(function () {}.bind(), {})",
  "Function.prototype[Symbol.hasInstance].call(class {}, {})",
  "Function.prototype[Symbol.hasInstance].call(new Proxy(function () {}, {}), {})",
  "Function.prototype[Symbol.hasInstance].call(Array, [])",
  "Function.prototype[Symbol.hasInstance].call(Array, new Proxy([], {}))",
  "Function.prototype[Symbol.hasInstance].call(function () {}, new Proxy({}, { getPrototypeOf() { throw new EvalError('gp') } }))",
  "Function.prototype[Symbol.hasInstance].name",
  "Function.prototype[Symbol.hasInstance].length",
  "Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance)",
  "Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance).writable",
  "Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance).configurable",
  "Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance).enumerable",
  "Function.prototype[Symbol.hasInstance].toString()",
  "Function.prototype[Symbol.hasInstance] === Function.prototype[Symbol.hasInstance]",
  "Object.getOwnPropertyDescriptor(Function, Symbol.hasInstance)",
  "Object.getOwnPropertyDescriptor(Object, Symbol.hasInstance)",
  "(function () { 'use strict'; Function.prototype[Symbol.hasInstance] = 1 })()",
  "(function () { delete Function.prototype[Symbol.hasInstance]; return [] instanceof Array })()",
  "(function () { var F = function () {}; var g = Object.getPrototypeOf(F); var saved = g[Symbol.hasInstance]; var r = (new F) instanceof F; return r })()",
  "(function () { var F = function () {}; Object.setPrototypeOf(F, null); return (new F) instanceof F })()",
  "(function () { var F = function () {}; Object.setPrototypeOf(F, Object.prototype); return (new F) instanceof F })()",
  "(function () { var o = Object.create(null); o[Symbol.hasInstance] = Function.prototype[Symbol.hasInstance]; return 1 instanceof o })()",
  "(function () { var o = Object.create(null); return 1 instanceof o })()",
  "(function () { var o = Object.create(Function.prototype); return {} instanceof o })()",
  "(function () { return ({}) instanceof Object.create(Function.prototype) })()",
  "(function () { return Symbol() instanceof Symbol })()",
  "(function () { return Object(Symbol()) instanceof Symbol })()",
  "(function () { return Object(1n) instanceof BigInt })()",
  "(function () { return new Number(1) instanceof Number })()",
  "(function () { return 1 instanceof Number })()",
  "(function () { return (function () {}) instanceof Function })()",
  "(function () { return Function instanceof Function })()",
  "(function () { return Object instanceof Function })()",
  "(function () { return Function instanceof Object })()",
  "(function () { return Function.prototype instanceof Function })()",
  "(function () { return Function.prototype instanceof Object })()",
  "(function () { return Object.prototype instanceof Object })()",
  "(function () { return (async function () {}) instanceof Function })()",
  "(function () { return (class {}) instanceof Function })()",
  "(function () { return Math.max instanceof Function })()",
  "(function () { return new Proxy(function () {}, {}) instanceof Function })()",
  "(function () { return (function () {}).bind() instanceof Function })()",
  "(function () { return Object.create(Function.prototype) instanceof Function })()",
];
for (const e of instMisc) show(e);

// ---- 5. name e length: valores, descritores e redefinição.
const nameLenSubjects = ["function f(a, b) {}", "function (a) {}", "(a, b, c) => 0", "class K { constructor(a) {} }", "class K {}", "async function af(a, b = 1) {}",
  "function g(a, b = 1, c) {}", "function h(...r) {}", "function i(a, ...r) {}", "function j({ a }, [b]) {}", "function* k(a) {}", "({ m(a) {} }).m",
  "({ get p() { return 1 } }, Object.getOwnPropertyDescriptor({ get p() { return 1 } }, 'p').get)", "Math.max", "Function.prototype", "Object", "Symbol", "Date.prototype.getTime"];
const redefs = [
  "f.name = 'x'", "Object.defineProperty(f, 'name', { value: 'x' })", "Object.defineProperty(f, 'name', { value: 'x', writable: true })",
  "Object.defineProperty(f, 'name', { value: 'x', enumerable: true })", "Object.defineProperty(f, 'name', { value: 'x', configurable: false })",
  "Object.defineProperty(f, 'name', { get() { return 'g' } })", "delete f.name", "Object.defineProperty(f, 'length', { value: 9 })",
  "Object.defineProperty(f, 'length', { value: -1 })", "Object.defineProperty(f, 'length', { writable: true })", "f.length = 5",
  "delete f.length", "Object.defineProperty(f, 'length', { value: 9, configurable: false })", "Object.defineProperty(f, 'length', { get() { return 3 } })",
  "Object.freeze(f)", "Object.seal(f)", "Object.preventExtensions(f)", "Object.defineProperty(f, 'length', { enumerable: true })",
  "Object.defineProperty(f, 'name', { value: undefined })", "Object.defineProperty(f, 'name', { value: Symbol('q') })",
];
for (const s of nameLenSubjects) {
  const isDesc = s.includes("Object.getOwnPropertyDescriptor(");
  const subject = isDesc ? s.split(", ").slice(1).join(", ").replace(/\)$/, "") : s;
  const base = "var f = " + (isDesc ? "Object.getOwnPropertyDescriptor({ get p() { return 1 } }, 'p').get" : subject) + ";";
  show("[" + (isDesc ? "Object.getOwnPropertyDescriptor({ get p() { return 1 } }, 'p').get" : subject) + "].map(f => f.name + '/' + f.length)[0]");
  add(base + " R = T(() => JSON.stringify([Object.getOwnPropertyDescriptor(f, 'name'), Object.getOwnPropertyDescriptor(f, 'length'), Reflect.ownKeys(f).map(String)]))");
  for (const r of redefs) {
    sampled(() => add(base + " R = T(() => { " + r + "; return JSON.stringify([Object.getOwnPropertyDescriptor(f, 'name'), Object.getOwnPropertyDescriptor(f, 'length'), f.toString().length > 0]) })"));
    sampled(() => add(base + " R = T(() => { " + r + "; return String(f.bind().name) + '/' + f.bind().length })"));
  }
}
const nameInference = [
  "var a = function () {}; a.name", "var a = () => {}; a.name", "var a = class {}; a.name", "var a = class { static name = 'x' }; a.name",
  "var a = class { static name() {} }; typeof a.name", "var a = class B {}; a.name", "var o = { a: function () {} }; o.a.name",
  "var o = { a: () => {} }; o.a.name", "var o = { ['a' + 'b']: function () {} }; o.ab.name", "var s = Symbol('d'); var o = { [s]: function () {} }; o[s].name",
  "var s = Symbol(); var o = { [s]: function () {} }; JSON.stringify(o[s].name)", "var o = { get a() { return 1 } }; Object.getOwnPropertyDescriptor(o, 'a').get.name",
  "var o = { set a(v) {} }; Object.getOwnPropertyDescriptor(o, 'a').set.name", "var s = Symbol('sd'); var o = { get [s]() { return 1 } }; Object.getOwnPropertyDescriptor(o, s).get.name",
  "var o = { 1: function () {} }; o[1].name", "var o = { 1.5: function () {} }; o[1.5].name", "var o = { 1n: function () {} }; o[1].name", "var o = { 'a b': function () {} }; o['a b'].name",
  "var o = {}; o.x = function () {}; JSON.stringify(o.x.name)", "var o = {}; o['x'] = () => {}; JSON.stringify(o.x.name)", "let [a = function () {}] = []; a.name",
  "let { a = () => {} } = {}; a.name", "let a; a = function () {}; a.name", "let a; a ||= function () {}; a.name", "let a; a ??= () => {}; a.name",
  "let a = 1; a &&= function () {}; a.name", "function f(a = function () {}) { return a.name }; f()", "(function (a = class {}) { return a.name })()",
  "var f = (0, function () {}); JSON.stringify(f.name)", "var f = (function () {}); f.name", "var f = ((function () {})); f.name", "var f = (0, () => {}); JSON.stringify(f.name)",
  "class A { static m = function () {} }; A.m.name", "class A { m = () => {} }; new A().m.name", "class A { #p = function () {}; g() { return this.#p.name } }; new A().g()",
  "class A { static #p = () => {}; static g() { return A.#p.name } }; A.g()", "class A { static [Symbol('x')]() {} }; A[Object.getOwnPropertySymbols(A)[0]].name",
  "class A { get a() { return 1 } }; Object.getOwnPropertyDescriptor(A.prototype, 'a').get.name", "class A { static async *ag() {} }; A.ag.name",
  "new Function().name", "new Function('a', 'return a').name", "Function().name", "(new Function).name", "(async function () {}).name === ''",
  "var f = function* () {}; f.name", "var f = async () => {}; f.name", "export_ = 1; (function () {}).name === ''", "(class {}).name === ''",
  "Object.getOwnPropertyNames(class {}).join()", "Object.getOwnPropertyNames(class { static x = 1 }).join()", "Object.getOwnPropertyNames(function () {}).join()",
  "Object.getOwnPropertyNames(() => {}).join()", "Object.getOwnPropertyNames(async function () {}).join()", "Object.getOwnPropertyNames(function* () {}).join()",
  "Object.getOwnPropertyNames(({ m() {} }).m).join()", "Object.getOwnPropertyNames(Math.max).join()", "Object.getOwnPropertyNames(Function.prototype).join()",
  "Reflect.ownKeys(Function.prototype).map(String).join()", "Object.getOwnPropertyNames(class { static m() {} static x = 1 }).join()",
  "Object.getOwnPropertyNames(function () { 'use strict' }).join()", "Object.getOwnPropertyNames(Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').get).join()",
];
for (const e of nameInference) add("R = T(() => { " + e.replace(/^(.*;)?\s*([^;]+)$/, (m, a, b) => (a || "") + " return S(" + b.trim() + ")") + " })");

// ---- 6. caller e arguments (strict, sloppy, bound, arrow, class, generator, async).
const sloppy = (body) => "var N = new Function('return ' + " + JSON.stringify(body) + "); ";
const callerSubjects = {
  strict: "function () { 'use strict' }", sloppy: "new Function('')", arrow: "() => 1", asyncArrow: "async () => 1", cls: "class {}", method: "({ m() {} }).m",
  gen: "function* () {}", asyncFn: "async function () {}", bound: "(function () {}).bind()", boundStrict: "(function () { 'use strict' }).bind()",
  boundSloppy: "new Function('').bind()", native: "Math.max", proxy: "new Proxy(function () {}, {})", fproto: "Function.prototype", getter: "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').get",
};
for (const [label, expr] of Object.entries(callerSubjects)) {
  for (const prop of ["caller", "arguments"]) {
    show("(" + expr + ")." + prop);
    show("(function () { var f = " + expr + "; f." + prop + " = 1; return f." + prop + " })()");
    show("(function () { var f = " + expr + "; return Object.getOwnPropertyDescriptor(f, '" + prop + "') })()");
    show("(function () { var f = " + expr + "; return f.hasOwnProperty('" + prop + "') })()");
    show("(function () { var f = " + expr + "; return '" + prop + "' in f })()");
    show("(function () { var f = " + expr + "; return delete f." + prop + " })()");
    show("(function () { var f = " + expr + "; return Reflect.defineProperty(f, '" + prop + "', { value: 1 }) })()");
    show("(function () { var f = " + expr + "; return Object.getOwnPropertyNames(f).filter(k => k === '" + prop + "').length })()");
  }
}
const callerMisc = [
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller')",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments')",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get === Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').get",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get === Object.getOwnPropertyDescriptor(Function.prototype, 'caller').set",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.name",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.length",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').configurable",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').enumerable",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.call(function () {})",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.call(1)",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.call()",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').set.call(function () {}, 1)",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').get.call(function () { 'use strict' })",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').get.call(new Function(''))",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'arguments').get.call({})",
  "Function.prototype.caller", "Function.prototype.arguments",
  "(function () { Function.prototype.caller = 1 })()",
  "(function () { 'use strict'; return (function () { return arguments.callee })() })()",
  "(function () { return (function () { return arguments.callee })() })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee') })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get.name })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get.toString() })()",
  "(function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').configurable })()",
  "(function () { 'use strict'; arguments.callee = 1 })()",
  "(function () { return typeof arguments.callee })()",
  "(function (a) { return Object.getOwnPropertyDescriptor(arguments, 'callee').value === arguments.callee })()",
  "(function (a) { return Object.getOwnPropertyDescriptor(arguments, 'callee').enumerable })()",
  "(function (a) { return Object.getOwnPropertyNames(arguments).join() })()",
  "(function (a) { 'use strict'; return Object.getOwnPropertyNames(arguments).join() })()",
  "(function (a = 1) { return Object.getOwnPropertyNames(arguments).join() })()",
  "(function (a) { return Reflect.ownKeys(arguments).map(String).join() })(1, 2)",
  "(function (a) { 'use strict'; return Reflect.ownKeys(arguments).map(String).join() })(1, 2)",
  "(() => { try { return arguments.callee } catch (e) { return e.name } })()",
  "(function () { return (() => arguments.length)() })(1, 2)",
  "(function () { return new Function('return typeof arguments')() })()",
  "var N = new Function('return N.caller'); N()",
  "var N = new Function('return N.caller'); (function outer() { return N() === outer })()",
  "var N = new Function('return N.caller'); (function outer() { 'use strict'; return N() })()",
  "var N = new Function('return N.caller'); (() => N())()",
  "var N = new Function('return N.caller'); (class { static m() { return N() } }).m()",
  "var N = new Function('return N.caller'); N.call()",
  "var N = new Function('return N.arguments.length'); N(1, 2, 3)",
  "var N = new Function('return N.arguments'); N() === null",
  "var N = new Function('return N.arguments === arguments'); N()",
  "var N = new Function('return N.arguments[0]'); N(9)",
  "var N = new Function('a', 'a = 5; return N.arguments[0]'); N(9)",
  "var N = new Function('return typeof N.caller'); (function () { 'use strict'; return N() })()",
  "var N = new Function('return typeof N.caller'); new N",
  "var N = new Function('return typeof N.caller'); N.apply()",
  "var N = new Function('return typeof N.caller'); Reflect.apply(N, null, [])",
  "var N = new Function('return typeof N.caller'); [1].map(N)[0]",
  "var N = new Function('return N.caller'); (function outer() { return [1].map(N)[0] })()",
  "var N = new Function('return N.caller === null'); N()",
  "var N = new Function('return N.caller === null'); N.bind()()",
  "var N = new Function('return typeof N.caller'); (function outer() { return N.bind()() })()",
  "var N = new Function('return typeof N.caller'); (function outer() { return Reflect.construct(N, []) })() instanceof Object",
  "var S2 = new Function('\"use strict\"; return typeof S2.caller'); S2()",
  "var N = new Function('return N.caller'); (async function outer() { return N() })().then(v => { R = typeof v }); R = 'sync'",
  "var N = new Function('return typeof N.caller'); (function* outer() { yield N() })().next().value",
  "var N = new Function('return N.caller'); var o = { m() { return N() } }; typeof o.m()",
  "var N = new Function('return N.caller'); var o = { get m() { return N() } }; typeof o.m",
  "var N = new Function('return N.caller'); eval('N()')",
  "var N = new Function('return N.caller'); (0, eval)('N()')",
  "var N = new Function('return N.caller'); new Function('return N()')()",
  "var N = new Function('return N.caller'); (function outer() { return new Function('N', 'return N()')(N) === outer })()",
  "(function () { var f = function () {}; Object.defineProperty(f, 'caller', { value: 3 }); return f.caller })()",
  "(function () { var f = new Function(''); Object.defineProperty(f, 'caller', { value: 3 }); return f.caller })()",
  "(function () { var f = new Function(''); Object.defineProperty(f, 'arguments', { value: 3 }); return f.arguments })()",
  "(function () { var f = new Function(''); f.caller = 3; return f.caller })()",
  "(function () { var f = new Function(''); return Reflect.set(f, 'caller', 3) })()",
  "(function () { var f = new Function(''); return Reflect.set(f, 'arguments', 3) })()",
  "(function () { var f = function () { 'use strict' }; return Reflect.set(f, 'caller', 3) })()",
  "(function () { var f = function () { 'use strict' }; return Reflect.get(f, 'caller') })()",
  "(function () { var f = function () { 'use strict' }; return Reflect.get(f, 'caller', {}) })()",
  "(function () { var f = function () { 'use strict' }; return Reflect.get(Function.prototype, 'caller', f) })()",
  "(function () { var f = function () { 'use strict' }; return Reflect.set(Function.prototype, 'caller', 1, f) })()",
  "(function () { var o = Object.create(function () {}); return o.caller })()",
  "(function () { var o = Object.create(function () {}); o.caller = 1; return Object.keys(o).join() })()",
  "(function () { var o = Object.create(Function.prototype); return o.caller })()",
  "(function () { var o = Object.create(Function.prototype); o.caller = 1 })()",
  "(function () { var o = Object.create(Function.prototype); return o.arguments })()",
  "Object.getOwnPropertyNames(Function.prototype).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(function () {}).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(new Function('')).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(function () { 'use strict' }).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(class {}).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(() => {}).filter(k => k === 'caller' || k === 'arguments').join()",
  "Object.getOwnPropertyNames(Math.max).filter(k => k === 'caller' || k === 'arguments').join()",
  "JSON.stringify(Object.getOwnPropertyDescriptor(new Function(''), 'caller'))",
  "JSON.stringify(Object.getOwnPropertyDescriptor(new Function(''), 'arguments'))",
  "'caller' in Function.prototype",
  "Function.prototype.hasOwnProperty('arguments')",
];
for (const e of callerMisc) {
  if (/^var |R = /.test(e)) add("R = T(() => { " + e.replace(/^(.*;)?\s*([^;]+)$/, (m, a, b) => (a || "") + " return S(" + b.trim() + ")") + " })");
  else show(e);
}

// ---- 7. Function.prototype como função chamável e propriedades de Function.
const fp = [
  "Function.prototype()", "Function.prototype(1, 2)", "typeof Function.prototype", "Function.prototype.name === ''", "Function.prototype.length",
  "new Function.prototype", "new (Function.prototype.bind.call(Function.prototype))", "Object.getPrototypeOf(Function.prototype) === Object.prototype",
  "Function.prototype.prototype", "Function.prototype.hasOwnProperty('prototype')", "Function.prototype.toString()", "Function.prototype.call()",
  "Function.prototype.apply()", "Function.prototype.bind()", "Function.prototype.bind().name", "Function.prototype.bind().length",
  "Function.prototype.constructor === Function", "Function.prototype.call(1)", "Function.prototype.apply(1, 2)", "Reflect.construct(Function.prototype, [])",
  "Reflect.construct(Function, [], Function.prototype)", "Function.prototype instanceof Function", "Function.prototype.isPrototypeOf(Function)",
  "Function.prototype.isPrototypeOf(Function.prototype)", "Object.prototype.toString.call(Function.prototype)", "Object.prototype.toString.call(function () {})",
  "Object.prototype.toString.call(class {})", "Object.prototype.toString.call(async function () {})", "Object.prototype.toString.call(function* () {})",
  "Object.prototype.toString.call(async function* () {})", "Object.prototype.toString.call(Math.max.bind())", "Object.prototype.toString.call(new Proxy(function () {}, {}))",
  "Object.getOwnPropertyDescriptor(Function.prototype, 'constructor')", "Object.getOwnPropertyDescriptor(Function, 'prototype')", "Object.getOwnPropertyDescriptor(Function, 'length')",
  "Object.getOwnPropertyDescriptor(Function, 'name')", "Function.length", "Function.name", "Reflect.ownKeys(Function).map(String).join()",
  "Reflect.ownKeys(Function.prototype).map(String).join()", "Object.isExtensible(Function.prototype)", "Object.isFrozen(Function.prototype)",
  "Object.isSealed(Function.prototype)", "Function.prototype.isPrototypeOf(async function () {})", "Object.getPrototypeOf(async function () {}).constructor.name",
  "Object.getPrototypeOf(function* () {}).constructor.name", "Object.getPrototypeOf(async function* () {}).constructor.name", "Function.prototype.call.call === Function.prototype.call",
  "Function.prototype.call.apply === Function.prototype.call", "Function.prototype.call.bind === Function.prototype.bind", "Function.prototype.call.call.call === Function.prototype.call",
  "(function () { Function.prototype.name = 'x'; return Function.prototype.name })()", "(function () { 'use strict'; Function.prototype.name = 'x' })()",
  "(function () { 'use strict'; Function.prototype.length = 3 })()", "(function () { 'use strict'; delete Function.prototype.name; return Function.prototype.name })()",
  "(function () { var f = function () {}; f.name = 'x'; return f.name })()", "(function () { 'use strict'; var f = function () {}; f.name = 'x' })()",
  "(function () { 'use strict'; var f = function () {}; f.length = 3 })()", "(function () { 'use strict'; var f = function () {}; f.prototype = 3; return f.prototype })()",
  "(function () { 'use strict'; var f = class {}; f.prototype = 3 })()", "(function () { 'use strict'; var f = class {}; Object.defineProperty(f, 'prototype', { value: 1 }) })()",
  "(function () { var f = function () {}; return JSON.stringify(Object.getOwnPropertyDescriptor(f, 'prototype')) })()",
  "(function () { var f = class {}; return JSON.stringify(Object.getOwnPropertyDescriptor(f, 'prototype')) })()",
  "(function () { var f = function* () {}; return JSON.stringify(Object.getOwnPropertyDescriptor(f, 'prototype')) })()",
  "(function () { var f = async function () {}; return Object.getOwnPropertyDescriptor(f, 'prototype') })()",
  "(function () { var f = function () {}; return JSON.stringify(Object.getOwnPropertyDescriptor(f.prototype, 'constructor')) })()",
  "(function () { var f = function* () {}; return Object.getPrototypeOf(f.prototype) === Object.getPrototypeOf(function* () {}).prototype })()",
  "(function () { var f = function* () {}; return f.prototype.hasOwnProperty('constructor') })()",
  "(function () { var f = function () {}; f.prototype = null; return Object.getPrototypeOf(new f) === Object.prototype })()",
  "(function () { var f = function () {}; f.prototype = 1; return Object.getPrototypeOf(new f) === Object.prototype })()",
  "(function () { var f = function () {}; f.prototype = Math; return new f instanceof f })()",
  "(function () { var f = function () {}; f.prototype = Array.prototype; return Array.isArray(new f) })()",
  "Function('return this')() === globalThis",
  "Function('return typeof this')()",
  "Function.call(null, 'return 1')()",
  "Function.apply(null, ['a', 'return a'])(5)",
  "new Function('a', 'b', 'return a + b')(1, 2)",
  "new Function('a,b', 'return a + b')(1, 2)",
  "new Function('a = 1', 'b = 2', 'return a + b')()",
  "new Function('...a', 'return a.length')(1, 2, 3)",
  "new Function('{a}', 'return a')({ a: 4 })",
  "new Function('a', 'a', 'return a')(1, 2)",
  "new Function('a', 'a', '\"use strict\"; return a')",
  "new Function('a', 'let a')",
  "new Function('a = 1', '\"use strict\"')",
  "new Function('}', '')",
  "new Function('/*', '*/){')",
  "new Function('a', '}) + (function () {')",
  "new Function('', '}')",
  "new Function('return 1;', '')",
  "new Function('a b', '')",
  "new Function('a-', '')",
  "new Function('yield', '')",
  "new Function('await', '')",
  "new Function('let', '')",
  "new Function('let', '\"use strict\"')",
  "new Function('super()')",
  "new Function('new.target')()",
  "new Function('return new.target')()",
  "new (new Function('this.x = new.target === undefined'))().x",
  "new Function('return arguments.length')(1, 2)",
  "new Function('\"use strict\"; return this')()",
  "new Function('return this')() === globalThis",
  "new Function('a', 'return a').toString()",
  "new Function('a', 'b', 'return a').toString()",
  "new Function('a,b', 'return a').toString()",
  "new Function('a /* c */', '/* d */ return a').toString()",
  "new Function('a', '// c').toString()",
  "new Function('\\n').toString()",
  "new Function('a\\n', 'b').toString()",
  "new Function('a', 'return 1').name",
  "new Function('a', 'return 1').length",
  "new Function('a, b = 1', 'return 1').length",
  "new Function('...a', 'return 1').length",
  "new Function('a', 'b', 'c', 'return 1').length",
  "Object.getPrototypeOf(new Function) === Function.prototype",
  "new Function instanceof Function",
  "new Function.prototype.constructor('return 1')()",
  "Reflect.construct(Function, ['return 7'])()",
  "Reflect.construct(Function, ['return 7'], Object)() ",
  "Object.getPrototypeOf(Reflect.construct(Function, ['return 7'], Object)) === Object.prototype",
  "Object.getPrototypeOf(Reflect.construct(Function, ['return 7'], class extends Function {})) !== Function.prototype",
  "(function () { class F extends Function {}; var f = new F('return 3'); return [f(), f instanceof F, Object.getPrototypeOf(f) === F.prototype].join() })()",
  "(function () { class F extends Function { constructor() { super('return this.v'); this.v = 9 } }; return new F().call(new F()) })()",
  "(function () { class F extends Function {}; return new F('a', 'return a').toString() })()",
  "(function () { class F extends Function {}; return new F().name })()",
  "(function () { class F extends Function {}; return typeof new F() })()",
  "(function () { var P = new Proxy(Function, {}); return P('return 2')() })()",
  "(function () { var P = new Proxy(Function, { construct(t, a, nt) { return Reflect.construct(t, a, nt) } }); return new P('return 2')() })()",
  "String(Function.prototype)",
  "String(Function)",
  "`${Math.max}`",
  "Math.max + ''",
  "(function () {}) + ''",
  "(() => 1) + 1",
  "'' + class A { m() { } }",
  "[function () {}] + ''",
  "String([() => 1, class {}])",
  "JSON.stringify(function () {})",
  "JSON.stringify({ f: function () {} })",
  "JSON.stringify([function () {}])",
  "JSON.stringify(class {})",
  "Object.keys(function () {}).length",
  "Object.keys(Function.prototype).length",
  "Object.entries(class { static a = 1 }).join()",
  "(function () { var f = function () {}; f.a = 1; return JSON.stringify(Object.assign({}, f)) })()",
  "(function () { var f = function () {}; return Object.getOwnPropertyNames(f).join() })()",
  "(function () { var f = function () {}; return Reflect.ownKeys(Object.assign(f, { [Symbol('z')]: 1, a: 1 })).map(String).join() })()",
  "(function () { var f = class { static a = 1; static b() {} }; return Reflect.ownKeys(f).map(String).join() })()",
  "(function () { var f = class { static a = 1; static name = 'n' }; return Reflect.ownKeys(f).map(String).join() + f.name })()",
  "(function () { var f = class { static length = 5 }; return f.length })()",
  "(function () { var f = class { static get length() { return 5 } }; return f.length })()",
  "(function () { var f = class { static length() {} }; return typeof f.length })()",
  "(function () { var f = class { static name() {} }; return typeof f.name })()",
  "(function () { var f = class { static get name() { return 'gn' } }; return f.name })()",
  "(function () { var f = class { static prototype() {} } })()",
  "(function () { var f = class { static 'prototype'() {} } })()",
  "(function () { var f = class { static ['prototype']() {} } })()",
  "(function () { var f = class { static prototype = 1 } })()",
  "(function () { var f = class { static ['prototype'] = 1 } })()",
  "(function () { var f = class { static async prototype() {} } })()",
  "(function () { var f = class { static get prototype() {} } })()",
  "(function () { var f = class { prototype() {} }; return typeof new f().prototype })()",
  "(function () { var f = class { constructor() {} constructor() {} } })()",
  "(function () { var f = class { get constructor() {} } })()",
  "(function () { var f = class { async constructor() {} } })()",
  "(function () { var f = class { *constructor() {} } })()",
  "(function () { var f = class { static constructor() {} }; return typeof f.constructor })()",
  "(function () { var f = class { constructor = 1 } })()",
  "(function () { var f = class { static constructor = 1 } })()",
  "(function () { var f = class { 'constructor' = 1 } })()",
  "(function () { var f = class { ['constructor'] = 1 }; return new f().constructor })()",
];
for (const e of fp) {
  if (/^var |R = /.test(e)) add("R = T(() => { " + e + " })");
  else show(e);
}

// ---- 8. new.target e Reflect.construct com newTarget diferente.
const nt = [
  "(function () { function F() { this.nt = new.target } ; function G() {}; return Reflect.construct(F, [], G).nt === G })()",
  "(function () { function F() { this.nt = new.target } ; return new F().nt === F })()",
  "(function () { function F() { return new.target }; return F() })()",
  "(function () { function F() { return new.target }; return typeof new F })()",
  "(function () { function F() { return new.target === undefined }; return F.call({}) })()",
  "(function () { function F() { return new.target === undefined }; return F.apply({}) })()",
  "(function () { function F() { return new.target === undefined }; return Reflect.apply(F, {}, []) })()",
  "(function () { function F() { return new.target === undefined }; return F.bind()() })()",
  "(function () { var f = () => new.target; return f() })",
  "(function () { function F() { return (() => new.target)() }; return typeof new F })()",
  "(function () { function F() { return (() => new.target)() }; return F() })()",
  "(function () { function F() { this.t = (() => new.target)() }; return new F().t === F })()",
  "(function () { function F() { this.t = eval('new.target') }; return new F().t === F })()",
  "(function () { function F() { this.t = new Function('return new.target')() }; return new F().t })()",
  "(function () { function F() { this.t = (function () { return new.target })() }; return new F().t })()",
  "(function () { class A { constructor() { this.t = new.target } }; class B extends A {}; return new B().t === B })()",
  "(function () { class A { constructor() { this.t = new.target } }; class B extends A { constructor() { super() } }; return new B().t === B })()",
  "(function () { class A { constructor() { this.t = new.target } }; class B extends A {}; return Reflect.construct(A, [], B).t === B })()",
  "(function () { class A { constructor() { this.t = new.target } }; class B extends A {}; return Reflect.construct(B, [], A).t === A })()",
  "(function () { class A { constructor() { this.t = new.target } }; return Reflect.construct(A, [], Object).t === Object })()",
  "(function () { class A { constructor() { this.t = new.target } }; return Reflect.construct(A, [], Function).t === Function })()",
  "(function () { class A { constructor() { this.t = new.target } }; return Reflect.construct(A, [], function () {}).t.name })()",
  "(function () { class A {}; function G() {}; G.prototype = Array.prototype; return Array.isArray(Reflect.construct(A, [], G)) })()",
  "(function () { class A {}; function G() {}; G.prototype = Array.prototype; return Reflect.construct(A, [], G) instanceof Array })()",
  "(function () { class A {}; function G() {}; G.prototype = Array.prototype; return Object.getPrototypeOf(Reflect.construct(A, [], G)) === Array.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = null; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = 1; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = 'x'; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = undefined; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = Symbol(); return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; G.prototype = function () {}; return typeof Object.getPrototypeOf(Reflect.construct(F, [], G)) })()",
  "(function () { function F() {}; function G() {}; delete G.prototype; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; function G() {}; Object.defineProperty(G, 'prototype', { get() { throw new EvalError('p') } }); return Reflect.construct(F, [], G) })()",
  "(function () { function F() {}; var G = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Array.prototype })()",
  "(function () { function F() {}; var log = []; var G = new Proxy(function () {}, { get(t, k) { log.push(String(k)); return t[k] } }); Reflect.construct(F, [], G); return log.join() })()",
  "(function () { function F() {}; var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype })()",
  "(function () { function F() {}; var G = (function () {}).bind(); G.prototype = Array.prototype; return Object.getPrototypeOf(Reflect.construct(F, [], G)) === Array.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Array, [], G)) === Array.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Map, [], G)) === Map.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Error, [], G)) === Error.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(TypeError, [], G)) === TypeError.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Promise, [function () {}], G)) === Promise.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Date, [], G)) === Date.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(RegExp, [], G)) === RegExp.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Function, [], G)) === Function.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Object, [], G)) === Object.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Number, [1], G)) === Number.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(String, ['a'], G)) === String.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Boolean, [true], G)) === Boolean.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(ArrayBuffer, [1], G)) === ArrayBuffer.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Uint8Array, [1], G)) === Uint8Array.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(Set, [], G)) === Set.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(WeakMap, [], G)) === WeakMap.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(DataView, [new ArrayBuffer(1)], G)) === DataView.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(AggregateError, [[]], G)) === AggregateError.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(class A {}, [], G)) === Object.prototype })()",
  "(function () { var G = (function () {}).bind(); return Object.getPrototypeOf(Reflect.construct(class A extends Array {}, [], G)) === Array.prototype })()",
  "(function () { function G() {}; G.prototype = Date.prototype; return Object.getPrototypeOf(Reflect.construct(Array, [], G)) === Date.prototype })()",
  "(function () { function G() {}; G.prototype = Date.prototype; return Array.isArray(Reflect.construct(Array, [], G)) })()",
  "(function () { function G() {}; G.prototype = null; return Object.getPrototypeOf(Reflect.construct(Array, [], G)) === Array.prototype })()",
  "(function () { function G() {}; G.prototype = null; return Object.getPrototypeOf(Reflect.construct(Map, [], G)) === Map.prototype })()",
  "(function () { function G() {}; G.prototype = 1; return Object.getPrototypeOf(Reflect.construct(Error, [], G)) === Error.prototype })()",
  "(function () { function G() {}; G.prototype = 1; return Object.getPrototypeOf(Reflect.construct(Promise, [function () {}], G)) === Promise.prototype })()",
  "(function () { function G() {}; Object.defineProperty(G, 'prototype', { get() { throw new EvalError('p') } }); return Reflect.construct(Array, [], G) })()",
  "(function () { function G() {}; Object.defineProperty(G, 'prototype', { get() { throw new EvalError('p') } }); return Reflect.construct(Map, [], G) })()",
  "(function () { function G() {}; Object.defineProperty(G, 'prototype', { get() { throw new EvalError('p') } }); return Reflect.construct(class {}, [], G) })()",
  "(function () { class A extends Array {}; return Object.getPrototypeOf(new A) === A.prototype })()",
  "(function () { class A extends Array { constructor() { super(3) } }; return new A().length })()",
  "(function () { class A extends Array {}; return A.from([1, 2]) instanceof A })()",
  "(function () { class A extends Array {}; return new A(1, 2).map(x => x) instanceof A })()",
  "(function () { class A extends Map {}; return new A().set(1, 2).get(1) })()",
  "(function () { class A extends Promise {}; return A.resolve(1) instanceof A })()",
  "(function () { class A extends Error {}; return new A('m') instanceof A })()",
  "(function () { class A extends Function {}; return new A() instanceof A })()",
  "(function () { class A extends Object { constructor() { super(1) } }; return typeof new A })()",
  "(function () { class A extends Object { constructor() { return super(1) } }; return new A instanceof A })()",
  "(function () { class A extends null {}; return new A })()",
  "(function () { class A extends null { constructor() { return Object.create(A.prototype) } }; return new A instanceof A })()",
  "(function () { class A extends null { constructor() { super() } }; return new A })()",
  "(function () { class A extends 1 {} })()",
  "(function () { class A extends (() => {}) {} })()",
  "(function () { class A extends Math.max {} })()",
  "(function () { class A extends (function () {}).bind() {}; return typeof new A })()",
  "(function () { class A extends (async function () {}) {} })()",
  "(function () { class A extends (function* () {}) {} })()",
  "(function () { function F() {}; F.prototype = 1; class A extends F {} })()",
  "(function () { function F() {}; F.prototype = null; class A extends F {}; return Object.getPrototypeOf(A.prototype) === null })()",
  "(function () { function F() {}; F.prototype = undefined; class A extends F {} })()",
  "(function () { function F() {}; F.prototype = {}; class A extends F {}; return new A instanceof F })()",
  "(function () { var calls = 0; class A extends (calls++, Object) {}; return calls })()",
  "(function () { class A { constructor() { new.target.x = 1 } }; class B extends A {}; new B; return B.x })()",
  "(function () { class A { constructor() { return Object.create(new.target.prototype) } }; class B extends A {}; return new B instanceof B })()",
  "(function () { class A { constructor() { this.n = new.target.name } }; return Reflect.construct(A, [], class Named {}).n })()",
  "(function () { class A { constructor() { this.n = new.target.name } }; return Reflect.construct(A, [], function Fn() {}).n })()",
  "(function () { class A {}; return A() })()",
  "(function () { class A {}; return A.call({}) })()",
  "(function () { class A {}; return Reflect.apply(A, {}, []) })()",
  "(function () { class A { constructor() { this.x = 1 } }; return Reflect.construct(A, [], Array).x })()",
  "(function () { class A { constructor() { this.x = 1 } }; return Array.isArray(Reflect.construct(A, [], Array)) })()",
  "(function () { class A { constructor() { this.x = 1 } }; return Object.getPrototypeOf(Reflect.construct(A, [], Array)) === Array.prototype })()",
  "(function () { var o = Reflect.construct(function () { this.a = 1 }, [], Array); return [Array.isArray(o), o.a, Object.getPrototypeOf(o) === Array.prototype].join() })()",
  "(function () { var o = Reflect.construct(Array, [3], Object); return [Array.isArray(o), o.length, Object.getPrototypeOf(o) === Object.prototype].join() })()",
  "(function () { var o = Reflect.construct(Array, [3], Array); return [Array.isArray(o), o.length].join() })()",
  "(function () { var o = Reflect.construct(Date, [0], Object); return [Object.getPrototypeOf(o) === Object.prototype, typeof o.getTime].join() })()",
  "(function () { var o = Reflect.construct(Error, ['m'], Object); return [Object.getPrototypeOf(o) === Object.prototype, o.message, typeof o.stack].join() })()",
  "(function () { var o = Reflect.construct(Error, ['m'], function () {}); return [Object.prototype.toString.call(o), o.message].join() })()",
  "(function () { var o = Reflect.construct(Map, [], function () {}); return Object.prototype.toString.call(o) })()",
  "(function () { var o = Reflect.construct(Promise, [function () {}], function () {}); return Object.prototype.toString.call(o) })()",
  "(function () { var o = Reflect.construct(Array, [], function () {}); return Object.prototype.toString.call(o) })()",
  "(function () { var o = Reflect.construct(Function, [], function () {}); return typeof o })()",
  "(function () { var o = Reflect.construct(Function, ['return 1'], function () {}); return Object.getPrototypeOf(o) === Function.prototype })()",
  "(function () { var F = function () {}; F.prototype = Function.prototype; var o = Reflect.construct(Function, ['return 1'], F); return o() })()",
  "(function () { var F = function () {}; F.prototype = Object.getPrototypeOf(async function () {}); var o = Reflect.construct(Function, ['return 1'], F); return Object.getPrototypeOf(o) === F.prototype })()",
  "(function () { var AF = Object.getPrototypeOf(async function () {}).constructor; return Object.getPrototypeOf(new AF('return 1')) === AF.prototype })()",
  "(function () { var AF = Object.getPrototypeOf(async function () {}).constructor; return new AF('return 1')() instanceof Promise })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return new GF('yield 1')().next().value })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return GF.length + GF.name })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return Object.getPrototypeOf(GF) === Function })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return new GF('a', 'yield a').toString() })()",
  "(function () { var AGF = Object.getPrototypeOf(async function* () {}).constructor; return new AGF('a', 'yield a').toString() })()",
  "(function () { var AF = Object.getPrototypeOf(async function () {}).constructor; return new AF('a', 'await a').toString() })()",
  "(function () { var AF = Object.getPrototypeOf(async function () {}).constructor; return new AF('await', '') })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return new GF('yield', '') })()",
  "(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; return new GF('a = yield', '') })()",
  "(function () { var AF = Object.getPrototypeOf(async function () {}).constructor; return new AF('a = await 1', '') })()",
];
for (const e of nt) show(e);

// ---- 9. Recursão profunda, RangeError e tail position não otimizada.
const rec = [
  "(function () { function f(n) { return f(n + 1) + 1 }; return f(0) })()",
  "(function () { 'use strict'; function f(n) { return n === 0 ? 0 : f(n - 1) }; return f(1e6) })()",
  "(function () { 'use strict'; function f(n) { if (n === 0) return 0; return f(n - 1) }; return f(1e7) })()",
  "(function () { function f(n) { if (n === 0) return 0; return f(n - 1) }; return f(1e7) })()",
  "(function () { 'use strict'; function f(n, a) { return n === 0 ? a : f(n - 1, a + 1) }; return f(1e6, 0) })()",
  "(function () { 'use strict'; function even(n) { return n === 0 ? true : odd(n - 1) }; function odd(n) { return n === 0 ? false : even(n - 1) }; return even(1e7) })()",
  "(function () { var f = n => n === 0 ? 0 : f(n - 1); return f(1e7) })()",
  "(function () { var o = { get a() { return this.a } }; return o.a })()",
  "(function () { var o = { set a(v) { this.a = v } }; o.a = 1 })()",
  "(function () { var o = { toString() { return String(this) } }; return String(o) })()",
  "(function () { var o = { valueOf() { return +this } }; return +o })()",
  "(function () { var o = { toJSON() { return JSON.stringify(this) } }; return JSON.stringify(o) })()",
  "(function () { var p = new Proxy({}, { get(t, k, r) { return r[k] } }); return p.x })()",
  "(function () { var p = new Proxy(function () {}, { apply(t, th, a) { return p() } }); return p() })()",
  "(function () { var p = new Proxy(function () {}, { construct(t, a) { return new p } }); return new p })()",
  "(function () { var f = function () {}; f = f.bind(f); for (var i = 0; i < 1e5; i++) f = f.bind(); return typeof f })()",
  "(function () { var f = function () { return 1 }; for (var i = 0; i < 5e4; i++) f = f.bind(); return f() })()",
  "(function () { var o = {}; var p = o; for (var i = 0; i < 1e5; i++) p = Object.create(p); return p instanceof Object })()",
  "(function () { var s = 'x'; var f = function () { return 1 }; for (var i = 0; i < 2000; i++) f = Function.prototype.call.bind(f); return typeof f })()",
  "(function () { function f() { return f.call(this) }; return f() })()",
  "(function () { function f() { return f.apply(this, arguments) }; return f() })()",
  "(function () { function f() { return Reflect.apply(f, this, arguments) }; return f() })()",
  "(function () { function f() { return f.bind(this)() }; return f() })()",
  "(function () { function f() { return new f }; return f() })()",
  "(function () { function f() { return [1].map(f) }; return f() })()",
  "(function () { function f() { return [1].map(() => f()) }; return f() })()",
  "(function () { function f() { return Array.from({ length: 1 }, f) }; return f() })()",
  "(function () { function f() { return eval('f()') }; return f() })()",
  "(function () { function f() { return new Function('f', 'return f()')(f) }; return f() })()",
  "(function () { function f() { try { return f() } catch (e) { return f() } }; return f() })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return d > 1000 } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return e instanceof RangeError } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return e.constructor === RangeError } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return e.message } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return e.name } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return typeof e.stack } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return Object.getOwnPropertyNames(e).join() } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return Object.prototype.toString.call(e) } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return String(e) } })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { return Object.getPrototypeOf(e) === RangeError.prototype } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return e.stack.split('\\n')[0] } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return e.stack.split('\\n').length > 2 } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return e.stack.split('\\n').length <= 11 } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return e.stack.split('\\n')[1].trim().startsWith('at f (') } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { try { f() } catch (e2) { return e !== e2 } } })()",
  "(function () { function f() { f() }; for (var i = 0; i < 5; i++) { try { f() } catch (e) { } } return 'ok' })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return (function g(n) { return n === 0 ? 'ok' : g(n - 1) })(1000) } })()",
  "(function () { function f() { f() }; try { f() } catch (e) { return (function g(n) { return n === 0 ? 'ok' : g(n - 1) })(5000) } })()",
  "(function () { function f() { try { f() } finally { } }; try { f() } catch (e) { return e.name } })()",
  "(function () { var order = []; function f(n) { try { f(n + 1) } finally { if (n === 0) order.push('fin0') } }; try { f(0) } catch (e) { order.push(e.name) } return order.join() })()",
  "(function () { var n = 0; function f() { n++; try { f() } catch (e) { n += 0 } }; f(); return n > 100 })()",
  "(function () { function f() { f() }; return Promise.resolve().then(f).catch(e => e.name) instanceof Promise })()",
  "(function () { function f() { f() }; var r; Promise.resolve().then(f).catch(e => { globalThis.R = e.name + ': ' + e.message }); return 'pending' })()",
  "(function () { var d = 0; function f() { d++; f() }; try { f() } catch (e) { var d1 = d; d = 0; try { f() } catch (e2) { return Math.abs(d1 - d) < d1 / 2 } } })()",
  "(function () { function f() { return f.call(null) }; try { return f() } catch (e) { return e.message } })()",
  "(function () { function f() { return [].concat.apply([], [f()]) }; try { return f() } catch (e) { return e.message } })()",
  "(function () { function f(a) { return f(a, a) }; try { return f(1) } catch (e) { return e.message } })()",
  "(function () { var o = { f() { return this.f() } }; try { return o.f() } catch (e) { return e.message } })()",
  "(function () { var o = { get x() { return this.x } }; try { return o.x } catch (e) { return e.message } })()",
  "(function () { var a = []; a[0] = a; try { return String(a) } catch (e) { return e.name } })()",
  "(function () { var a = []; a[0] = a; return String(a) === '' })()",
  "(function () { var a = []; for (var i = 0; i < 1e5; i++) a = [a]; try { return JSON.stringify(a).length } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var a = []; for (var i = 0; i < 1e5; i++) a = [a]; try { return String(a).length } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var o = {}; for (var i = 0; i < 1e5; i++) o = { o }; try { return JSON.stringify(o).length } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var s = '('.repeat(1e5) + ')'.repeat(1e5); try { return eval(s) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var s = '['.repeat(1e5) + ']'.repeat(1e5); try { return JSON.parse(s).length } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var s = '['.repeat(1e6); try { return JSON.parse(s) } catch (e) { return e.name } })()",
  "(function () { var s = 'a' + '+a'.repeat(1e5); try { return typeof new Function('a', 'return ' + s) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { try { return new RegExp('('.repeat(1e5) + ')'.repeat(1e5)).source.length } catch (e) { return e.name } })()",
  "(function () { var f = function () { return arguments.length }; try { return f.apply(null, new Array(1e7).fill(0)) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { try { return Math.max(...new Array(1e7).fill(0)) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { try { return new Array(1e7).fill(0).concat([1]).length } catch (e) { return e.name } })()",
  "(function () { try { return String.fromCharCode.apply(null, new Array(1e7).fill(65)).length } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return Array.prototype.push.apply([], args) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return (function () { return arguments.length }).apply(null, args) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return Reflect.construct(function () { this.n = arguments.length }, args).n } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return (function () { return arguments.length }).bind(null, ...args)() } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return (function () { return arguments.length }).call(null, ...args) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(2e6).fill(0); try { return new (function () { this.n = arguments.length })(...args).n } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var args = new Array(5e5).fill(0); return (function () { return arguments.length }).bind(null, ...args)(...args) })()",
  "(function () { var args = new Array(5e5).fill(0); return (function () { return arguments.length }).bind(null, ...args).length })()",
  "(function () { var args = new Array(5e5).fill(0); return (function (a, b) {}).bind(null, ...args).length })()",
  "(function () { var f = function () { return arguments.length }; return f.apply(null, { length: 2 ** 32 - 1 }) })()",
  "(function () { var f = function () { return arguments.length }; return f.apply(null, { length: 2 ** 31 - 1 }) })()",
  "(function () { var f = function () { return arguments.length }; return f.apply(null, { length: 2 ** 32 + 1 }) })()",
];
for (const e of rec) {
  if (e.includes("globalThis.R")) add("R = 'unset'; T(() => " + e + ")");
  else show(e);
}
// Recursão com e sem try, profundidade configurável e tail position.
for (const mode of ["plain", "strict", "arrow", "method", "getter", "bound", "apply", "call", "new", "async", "gen", "proxy", "toString", "closure", "default param", "spread", "template"]) {
  const bodies = {
    plain: "function f(n) { return n === 0 ? 0 : f(n - 1) }",
    strict: "function f(n) { 'use strict'; return n === 0 ? 0 : f(n - 1) }",
    arrow: "var f = n => n === 0 ? 0 : f(n - 1)",
    method: "var f = (o => n => n === 0 ? 0 : o.m(n - 1))(({ m(n) { return n === 0 ? 0 : f(n) } }) ); var o = { m: n => f(n) }",
    getter: "var o = { get g() { return o.g } }; function f(n) { return o.g }",
    bound: "var g = function (n) { return n === 0 ? 0 : g2(n - 1) }; var g2 = g.bind(null); function f(n) { return g2(n) }",
    apply: "function f(n) { return n === 0 ? 0 : f.apply(null, [n - 1]) }",
    call: "function f(n) { return n === 0 ? 0 : f.call(null, n - 1) }",
    new: "function f(n) { return n === 0 ? 0 : new f(n - 1) }",
    async: "async function f(n) { return n === 0 ? 0 : await f(n - 1) }",
    gen: "function* f(n) { if (n > 0) yield* f(n - 1) }",
    proxy: "var p = new Proxy(function () {}, { apply(t, th, a) { return a[0] === 0 ? 0 : p(a[0] - 1) } }); function f(n) { return p(n) }",
    toString: "var o = { toString() { return this.n === 0 ? '0' : String({ n: this.n - 1, toString: o.toString }) } }; function f(n) { return String({ n, toString: o.toString }) }",
    closure: "function f(n) { return (() => n === 0 ? 0 : f(n - 1))() }",
    "default param": "function f(n, a = n === 0 ? 0 : f(n - 1)) { return a }",
    spread: "function f(n, ...r) { return n === 0 ? 0 : f(n - 1, ...r) }",
    template: "function f(n) { return n === 0 ? 0 : `${f(n - 1)}` }",
  };
  for (const depth of ["10", "1000", "5000", "1e5", "1e7"]) {
    for (const wrapper of ["T(() => f(" + depth + "))", "T(() => { try { return f(" + depth + ") } catch (e) { return 'caught ' + e.name } })", "T(() => { try { return f(" + depth + ") } finally { } })"]) {
      if (mode === "async") add(bodies[mode] + "; R = 'unset'; f(" + depth + ").then(v => { globalThis.R = S(v) }, e => { globalThis.R = e.name + ': ' + e.message })");
      else if (mode === "gen") add(bodies[mode] + "; R = T(() => { var it = f(" + depth + "); return S(it.next().done) })");
      else add(bodies[mode] + "; R = " + wrapper);
    }
  }
}
// Pilha esgotada dentro de builtins que chamam funções do usuário.
for (const host of ["[1].map", "[1].forEach", "[1].reduce", "[3, 2].sort", "[1].flatMap", "[1].find", "Array.from", "new Map([[1, 1]]).forEach", "new Set([1]).forEach", "'a'.replace", "JSON.parse", "Reflect.apply", "Object.defineProperty"]) {
  const arg = {
    "[1].map": "cb", "[1].forEach": "cb", "[1].reduce": "(a, b) => cb()", "[3, 2].sort": "cb", "[1].flatMap": "cb", "[1].find": "cb", "Array.from": "[1], cb",
    "new Map([[1, 1]]).forEach": "cb", "new Set([1]).forEach": "cb", "'a'.replace": "/a/, cb", "JSON.parse": "'[1]', cb", "Reflect.apply": "cb, null, []", "Object.defineProperty": "{}, 'x', { get: cb }",
  }[host];
  const call = host === "Object.defineProperty" ? "Object.defineProperty({}, 'x', { get: cb }).x" : host + "(" + arg + ")";
  add("var depth = 0; function cb() { depth++; return " + call.replace("cb", "cb") + " }; R = T(() => { cb(); return 1 })");
  add("function cb() { return " + call + " }; R = T(() => { try { cb() } catch (e) { return e.name + ': ' + e.message } })");
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "function-proto-golden-"));
const file = path.join(dir, "function_proto_case.js");
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
  const original = '"use strict";\n' + body.replace(/(^|[^.\w])R = /g, "$1globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 180000 });
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
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("function_proto", lines));
fs.rmSync(dir, { recursive: true, force: true });
