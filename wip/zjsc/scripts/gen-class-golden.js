// Gera tests/golden/class_bun.tsv: classes e campos medidos no bun 1.4.2 (campos públicos, privados e estáticos,
// ordem de inicialização, blocos static, `#x in obj`, métodos e acessores privados, herança de builtins, new.target,
// super em objetos literais e métodos estáticos, retorno de construtor derivado, `this` antes de super, inferência de
// nome, toString de classe, brand check e mensagens exatas de erro).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Os programas não usam `Object.*` estático (o golden de modelo de objetos cobre isso): só `Reflect` e operadores.
// Uso: bun scripts/gen-class-golden.js > tests/golden/class_bun.tsv
const { emitFactored, prepareProgram } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const prelude =
  '"use strict";\n' +
  "globalThis.F = (v, d = 0) => typeof v === 'symbol' ? v.toString() : typeof v === 'function' ? '[fn ' + v.name + ']' : " +
  "typeof v === 'string' ? JSON.stringify(v) : typeof v === 'bigint' ? v + 'n' : " +
  "Array.isArray(v) ? '[' + v.map(x => F(x, d + 1)).join(',') + ']' : " +
  "v && typeof v === 'object' ? (d > 3 ? '{...}' : '{' + Reflect.ownKeys(v).map(k => String(k) + ':' + F(v[k], d + 1)).join(',') + '}') : String(v);\n" +
  "globalThis.run = f => { try { return F(f()); } catch (e) { return '!' + (e && e.name) + ': ' + (e && e.message); } };\n";

const programs = [];
// Cada caso é o corpo de uma função; o resultado (ou o erro lançado) vira `R`.
const add = body => programs.push(prelude + "R = run(() => (0, eval)(" + JSON.stringify('"use strict";\n' + body) + "));");

// ---- 1. Declarações de campo × observações.
const fieldDecls = [
  "x = 1", "x", "'x' = 2", "1 = 3", "[k] = 4", "#p = 5", "static s = 6", "static #sp = 7", "x = this", "x = () => this",
  "x = function () {}", "x = class {}", "x = () => {}", "[Symbol.iterator] = 1", "[sym] = function () {}", "[sym] = () => {}",
  "#f = function () {}", "static f = function () {}", "static f = () => {}", "static f = class {}", "static x = this",
  "'quoted key' = 1", "0.5 = 1", "x = 1; y = this.x + 1", "x = y; y = 2", "[k2()] = 1", "static [k] = 1",
  "async = 1", "get = 1", "set = 2", "static = 3", "x = new.target", "x = arguments0()", "constructor2 = 1",
  "x = typeof A", "static x = typeof A", "x = super0", "static self = A", "x = (() => { return this; })()",
  "static s = this.name",
];
const fieldObs = [
  "Reflect.ownKeys(new A)", "Reflect.ownKeys(A)", "F(new A)", "F(Reflect.getOwnPropertyDescriptor(new A, 'x'))",
  "F(Reflect.getOwnPropertyDescriptor(A, 's'))", "typeof A.f === 'function' ? A.f.name : '-'",
  "(() => { const a = new A; const n = Reflect.ownKeys(a)[0]; return typeof a[n] === 'function' ? a[n].name : typeof a[n]; })()",
  "new A().x === new A().x",
];
for (const decl of fieldDecls) {
  for (const obs of fieldObs) {
    add(
      "const k = 'kk', sym = Symbol('desc');\nconst k2 = () => 'k2', arguments0 = () => 0, super0 = 0;\n" +
        "class A { " + decl + " }\n" + obs,
    );
  }
}

// ---- 2. Ordem de inicialização: super, campos, construtor, blocos static.
const orderParts = {
  baseField: "bf = log('baseField');",
  baseCtor: "constructor() { log('baseCtor'); }",
  derivedField: "df = log('derivedField');",
  derivedCtor: "constructor() { log('before super'); super(); log('after super'); }",
  derivedCtorNoLog: "constructor() { super(); }",
  baseStatic: "static bs = log('baseStatic');",
  derivedStatic: "static ds = log('derivedStatic');",
  baseBlock: "static { log('baseBlock'); }",
  derivedBlock: "static { log('derivedBlock'); }",
  derivedPrivate: "#dp = log('derivedPrivate');",
  basePrivate: "#bp = log('basePrivate');",
  computedBase: "[log('computedBase')] = 1;",
  computedDerived: "[log('computedDerived')] = 1;",
  baseMethod: "m() { log('m'); }",
};
const orderCombos = [
  ["baseField", "baseCtor", "derivedField", "derivedCtor"],
  ["baseField", "baseCtor", "derivedField", "derivedCtorNoLog"],
  ["baseField", "baseCtor", "derivedField", "derivedPrivate", "derivedCtor"],
  ["baseField", "basePrivate", "baseCtor", "derivedField", "derivedPrivate", "derivedCtor"],
  ["baseStatic", "baseBlock", "baseCtor", "derivedStatic", "derivedBlock", "derivedField", "derivedCtor"],
  ["baseStatic", "baseBlock", "baseCtor", "derivedStatic", "derivedBlock", "derivedField", "derivedCtorNoLog"],
  ["computedBase", "baseField", "baseCtor", "computedDerived", "derivedField", "derivedCtor"],
  ["baseBlock", "baseStatic", "computedBase", "baseCtor", "derivedBlock", "derivedStatic", "computedDerived", "derivedCtorNoLog"],
  ["baseField", "derivedField"],
  ["baseCtor", "derivedField"],
  ["baseField", "baseCtor", "derivedCtor"],
  ["baseMethod", "baseField", "baseCtor", "derivedField", "derivedCtorNoLog"],
];
const baseKeys = new Set(["baseField", "baseCtor", "baseStatic", "baseBlock", "basePrivate", "computedBase", "baseMethod"]);
for (const combo of orderCombos) {
  const base = combo.filter(p => baseKeys.has(p)).map(p => orderParts[p]).join("\n  ");
  const derived = combo.filter(p => !baseKeys.has(p)).map(p => orderParts[p]).join("\n  ");
  for (const act of ["new D", "new B", "new D, new D", "Reflect.construct(D, [], B)", "Reflect.construct(B, [], D)"]) {
    for (const tail of ["F(trace)", "trace.length"]) {
      add(
        "const trace = [];\nconst log = s => (trace.push(s), s);\n" +
          "class B {\n  " + base + "\n}\nclass D extends B {\n  " + derived + "\n}\n" + act + ";\n" + tail,
      );
    }
  }
  // Sem herança: só o lado derivado, e só o lado base.
  add("const trace = [];\nconst log = s => (trace.push(s), s);\nclass B {\n  " + base + "\n}\nnew B; trace");
  add("const trace = [];\nconst log = s => (trace.push(s), s);\nclass B {\n  " + base + "\n}\ntrace");
}

// ---- 3. Blocos static.
const staticBlocks = [
  "static { this.a = 1; }", "static { A.a = 1; }", "static { var v = 1; this.v = v; }", "static { let v = 1; this.v = v; }",
  "static { this.t = typeof this; }", "static { this.n = this.name; }", "static x = 1; static { this.y = this.x + 1; }",
  "static { this.a = 1; } static { this.b = this.a + 1; }", "static { try { throw 1; } catch (e) { this.e = e; } }",
  "static { this.f = () => this; }", "static { this.g = function () { return this; }; }",
  "static { this.h = new.target; }", "static { this.i = typeof arguments0; }", "static #p = 1; static { this.q = this.#p; }",
  "static { this.#p2 = 5; } static #p2;", "static { this.r = super.constructor === Function; }",
  "static { this.s = eval('this') === this; }", "static { label: { this.l = 1; break label; } }",
  "static { for (var i = 0; i < 3; i++) this['k' + i] = i; }", "static { const o = { m() { return super.toString === Object.prototype.toString; } }; this.so = o.m(); }",
  "static { this.async = 1; }", "static { return; }", "static { await0 = 1; }", "static { yield0 = 1; }",
  "static { var await; }", "static { class In { static { In.deep = 1; } } this.In = In; }",
  "static { function fn() { return 1; } this.fn = fn(); }", "static { this.v1 = typeof v1; var v1 = 1; }",
  "static { this.d = Reflect.ownKeys(this).join(); }", "static {}", "static { this.sym = Symbol.iterator in this; }",
  "static { arguments; }", "static { await 1; }", "static { this.x = () => arguments; }",
];
const blockObs = ["Reflect.ownKeys(A)", "F(A)", "typeof A", "A.a", "A.f && A.f.name"];
for (const block of staticBlocks) {
  for (const obs of blockObs) add("let arguments0, await0, yield0;\nclass A { " + block + " }\n" + obs);
}

// ---- 4. `#x in obj`, brand check, métodos e acessores privados.
const privDecls = {
  field: "#x = 1;",
  method: "#x() { return 1; }",
  getter: "get #x() { return 1; }",
  setter: "set #x(v) {}",
  accessor: "get #x() { return 1; } set #x(v) { this.set = v; }",
  staticField: "static #x = 1;",
  staticMethod: "static #x() { return 1; }",
  staticGetter: "static get #x() { return 1; }",
  staticSetter: "static set #x(v) {}",
};
const receivers = {
  undefined: "undefined", null: "null", number: "1", string: "'s'", plain: "{}", instance: "new A", sub: "new (class S extends A {})",
  other: "new (class O { #x = 1; })", proto: "Reflect.construct(Object, [], A) && Object0.create(new A)", cls: "A",
  fn: "function () {}", proxy: "new Proxy(new A, {})", symbol: "Symbol()", bigint: "1n", arr: "[]",
};
const privOps = {
  in: "#x in o", read: "o.#x", write: "o.#x = 2", call: "o.#x()", inc: "o.#x++", compound: "o.#x += 1", logical: "o.#x ??= 1",
  destructure: "({ a: o.#x } = { a: 1 })", optional: "o?.#x", delete: "(() => { try { eval('delete o.#x'); } catch (e) { return e.name; } })()",
  tagged: "o.#x`t`", template: "`${o.#x}`", forof: "(() => { for (o.#x of [1]); })()", typeof: "typeof o.#x", arrowread: "(() => o.#x)()",
};
const object0 = "const Object0 = { create: p => Reflect.construct(function () {}, [], class extends Function { })  };\n";
for (const [kind, decl] of Object.entries(privDecls)) {
  for (const [recvName, recv] of Object.entries(receivers)) {
    if (recvName === "proto") continue;
    for (const [opName, op] of Object.entries(privOps)) {
      if (opName === "delete" || opName === "forof" || opName === "tagged" || opName === "template" || opName === "typeof" || opName === "optional") {
        if (!["field", "method", "accessor"].includes(kind) || !["undefined", "plain", "instance", "other"].includes(recvName)) continue;
      }
      if (["logical", "compound", "inc", "write", "destructure", "call"].includes(opName) && !["field", "method", "getter", "setter", "accessor", "staticField", "staticMethod", "staticGetter", "staticSetter"].includes(kind)) continue;
      if (["logical", "compound", "inc"].includes(opName) && !["undefined", "plain", "instance", "cls", "other"].includes(recvName)) continue;
      add(
        object0 + "class A { " + decl + " static test(o) { return " + op + "; } }\n" +
          "const o = " + recv + ";\nA.test(o)",
      );
    }
  }
}

// ---- 5. Mensagens exatas de brand check e acesso a membro privado, cenários nomeados.
const named = [
  "class A { #x = 1; static g(o) { return o.#x; } } A.g({})",
  "class A { #x = 1; static s(o) { o.#x = 1; } } A.s({})",
  "class A { #m() {} static c(o) { o.#m(); } } A.c({})",
  "class A { #m() {} static s(o) { o.#m = 1; } } A.s(new A)",
  "class A { get #g() { return 1; } static s(o) { o.#g = 1; } } A.s(new A)",
  "class A { set #g(v) {} static r(o) { return o.#g; } } A.r(new A)",
  "class A { static #m() {} static s() { A.#m = 1; } } A.s()",
  "class A { #x; constructor(o) { return o; } } class B extends A { #y; constructor(o) { super(o); } static has(o) { return #y in o; } } const o = {}; new B(o); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { #y = 1; } const o = {}; new B(o); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { #y() {} } const o = {}; new B(o); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { get #y() { return 1; } } const o = {}; new B(o); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { #y = 1; static get(o) { return o.#y; } } const o = {}; new B(o); B.get(o)",
  "class A { #x = 1; static g(o) { return o.#x; } } A.g(new Proxy(new A, {}))",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g(new Proxy(new A, {}))",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g(1)",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g(undefined)",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g('str')",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g(Symbol())",
  "class A { #x = 1; static g(o) { return #x in o; } } A.g(null)",
  "class A { #x = 1; has(o) { return #x in o; } } new A().has(new A) + ',' + new A().has({})",
  "class A { #x = 1; static g(o) { return #x in o; } } const h = new Proxy(new A, { has() { throw 1; }, getPrototypeOf() { throw 2; } }); A.g(h)",
  "class A { #x; static g(o) { return o.#x; } } A.g(Object.create(new A))",
  "class A { #x = 1; static g(o) { return o.#x; } } class B extends A {} A.g(new B)",
  "class A { static #x = 1; static g(o) { return o.#x; } } class B extends A {} B.g(B)",
  "class A { static #x = 1; static g() { return this.#x; } } class B extends A {} B.g()",
  "class A { static #m() { return 1; } static g() { return this.#m(); } } class B extends A {} B.g()",
  "class A { #x = 1; m() { return this.#x; } } const m = new A().m; m()",
  "class A { #x = 1; m() { return this.#x; } } new A().m.call(1)",
  "class A { #x = 1; m() { return this.#x; } } new A().m.call(null)",
  "class A { #x = 1; static m() { return this.#x; } } A.m.call({})",
  "class A { #x = 1; static m() { return this.#x; } } A.m()",
  "class A { #x = 1; } class B { static g(o) { return o.#x; } }",
  "class A { #x = 1; } class A2 { #x = 2; static g(o) { return o.#x; } } A2.g(new A)",
  "class A { #x = 1; static g(o) { return o.#y; } }",
  "class A { m() { return this.#x; } }",
  "class A { m() { return #x in this; } }",
  "class A { #x; #x; }",
  "class A { #x; get #x() {} }",
  "class A { get #x() {} set #x(v) {} }",
  "class A { get #x() {} get #x() {} }",
  "class A { static get #x() {} set #x(v) {} }",
  "class A { #constructor; }",
  "class A { static #prototype = 1; static p() { return A.#prototype; } } A.p()",
  "class A { #x = 1; static t(o) { try { o.#x; return 'ok'; } catch (e) { return e instanceof TypeError; } } } A.t({}) + ',' + A.t(new A)",
  "class A { #x = 1; static t(o) { try { o.#x = 1; return 'ok'; } catch (e) { return e.constructor === TypeError; } } } A.t({})",
  "class A { #x = 1; m() { return delete this.#x; } }",
  "class A { #x = 1; m() { return this?.#x; } } new A().m()",
  "class A { #x = 1; m() { return this.a?.#x; } } new A().m()",
  "class A { #x = 1; m() { return this?.a.#x; } } new A().m()",
  "class A { #x = 1; static m(o) { return o?.#x; } } A.m(null) + ',' + A.m(undefined) + ',' + A.m(new A)",
  "class A { #x = 1; static m(o) { return o?.#x; } } A.m({})",
  "class A { #m() { return 1; } static m(o) { return o?.#m(); } } A.m(null) + ',' + A.m(new A)",
  "class A { #x = 1; static f(o) { return o.#x++; } } const a = new A; A.f(a) + ',' + A.f(a)",
  "class A { #x = 1; static f(o) { return ++o.#x; } } const a = new A; A.f(a) + ',' + A.f(a)",
  "class A { #x = 1; static f(o) { return o.#x **= 3; } } A.f(new A)",
  "class A { #x = null; static f(o) { return o.#x ??= 7; } } A.f(new A)",
  "class A { #x = 0; static f(o) { return o.#x ||= 7; } } A.f(new A)",
  "class A { #x = 1; static f(o) { return o.#x &&= 7; } } A.f(new A)",
  "class A { get #g() { return 1; } static f(o) { return o.#g ||= 7; } } A.f(new A)",
  "class A { get #g() { return 0; } static f(o) { return o.#g ||= 7; } } A.f(new A)",
  "class A { get #g() { return 1; } static f(o) { return o.#g &&= 7; } } A.f(new A)",
  "class A { #m() {} static f(o) { return o.#m &&= 7; } } A.f(new A)",
  "class A { #m() {} static f(o) { return o.#m ??= 7; } } A.f(new A)",
  "class A { #m() {} static f(o) { return o.#m; } } A.f(new A).name",
  "class A { #m() {} static f(o) { return o.#m === o.#m; } } A.f(new A)",
  "class A { #m() {} static f(a, b) { return a.#m === b.#m; } } A.f(new A, new A)",
  "class A { get #g() { return this; } static f(o) { return o.#g === o; } } A.f(new A)",
  "class A { #x = 1; static f(o) { const { #x: y } = o; return y; } }",
  "class A { #x = 1; static f(o) { return o.#x`a`; } } A.f(new A)",
  "class A { #x = (a) => a; static f(o) { return o.#x`a`; } } A.f(new A)",
  "class A { #x = 1; static f(o) { return [o.#x] } } A.f(new A)",
  "class A { #x = 1; static f(o) { return { ...o }; } } A.f(new A)",
  "class A { #x = 1; static f(o) { return JSON.stringify(o); } } A.f(new A)",
  "class A { #x = 1; static f(o) { return Reflect.ownKeys(o); } } A.f(new A)",
  "class A { #x = 1; static f(o) { return o.hasOwnProperty('#x'); } } A.f(new A)",
  "class A { #x = 1; static f(o) { return o['#x']; } } A.f(new A)",
  "class A { #x = 1; static f(o) { return '#x' in o; } } A.f(new A)",
  "class A { #x = 1; static f(o) { return Object.isFrozen(Object.freeze(o)) && (o.#x = 2); } } A.f(new A)",
  "class A { #x = 1; static f(o) { Object.freeze(o); o.#x = 2; return o.#x; } } A.f(new A)",
  "class A { #x = 1; static f(o) { Object.seal(o); o.#x = 2; return o.#x; } } A.f(new A)",
  "class A { #x = 1; static f(o) { Object.preventExtensions(o); return o.#x; } } A.f(new A)",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; static f(o) { return o.#x; } } const f = Object.freeze({}); new B(f); B.f(f)",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; } const f = Object.preventExtensions({}); new B(f)",
  "class A { constructor(o) { return o; } } class B extends A { static #x = 1; } new B(new Proxy({}, {}))",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; static f(o) { return o.#x; } } const p = new Proxy({}, {}); new B(p); B.f(p)",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; static f(o) { return #x in o; } } const p = new Proxy({}, {}); new B(p); B.f(p) + ',' + B.f({})",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; } new B(1)",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; } new B(undefined)",
  "class A { constructor(o) { return o; } } class B extends A { #x = 1; } new B(function () {}) instanceof Function",
  "class A { #x = 1; static f(o) { return o.#x; } } function G() {} G.prototype = A.prototype; A.f(new G)",
  "class A { #x = 1; static f(o) { return o.#x; } } const a = Reflect.construct(A, [], Array); A.f(a) + ',' + Array.isArray(a)",
  "class A { #x = 1; static f(o) { return o.#x; } } const a = Reflect.construct(A, [], class extends Array {}); A.f(a)",
  "class A { #x = 1; static f(o) { return o.#x; } } const a = Reflect.construct(A, [], Function); typeof a",
  "class A { static #x = 1; static f(o) { return o.#x; } } A.f(class extends A {})",
  "class A { static #x = 1; static f(o) { return #x in o; } } A.f(class extends A {}) + ',' + A.f(A)",
  "class A { static #m() {} static f(o) { return #m in o; } } A.f(class extends A {}) + ',' + A.f(A)",
  "class A { #m() {} static f(o) { return #m in o; } } A.f(A.prototype) + ',' + A.f(new A)",
  "class A { #x; static f(o) { return #x in o; } } A.f(A.prototype)",
  "class A { constructor() { A.f = () => this.#x; } #x = 3; } new A; A.f()",
  "class A { #x = 3; constructor() { this.f = () => this.#x; } } new A().f()",
  "class A { #x = 3; constructor() { this.f = function () { return this.#x; }; } } new A().f()",
  "class A { #x = 3; constructor() { this.f = function () { return this.#x; }; } } const a = new A; const f = a.f; f()",
  "class A { #x = 3; constructor() { this.f = function () { return this.#x; }; } } const a = new A; const f = a.f; f.call(a)",
  "class A { #x = 1; ['k' + this0()] = 2; } function this0() { return 0; } 1",
  "const C = class { #x = 1; static f(o) { return o.#x; } }; C.f(new C)",
  "const C = class N { #x = 1; static f(o) { return o.#x; } }; C.f(new C) + typeof N",
  "const mk = () => class { #x = 1; static f(o) { return o.#x; } }; const A = mk(), B = mk(); A.f(new B)",
  "const mk = () => class { #x = 1; static f(o) { return #x in o; } }; const A = mk(), B = mk(); A.f(new B) + ',' + A.f(new A)",
  "const mk = () => class { static #x = 1; static f(o) { return o.#x; } }; const A = mk(), B = mk(); A.f(A) + ',' + (() => { try { return A.f(B); } catch (e) { return e.message; } })()",
  "class A { #x = 1; m() { class B { n(o) { return o.#x; } } return new B().n(this); } } new A().m()",
  "class A { #x = 1; m() { class B { #x = 2; n(o) { return o.#x; } } return new B().n(new B); } } new A().m()",
  "class A { #x = 1; m() { class B { #x = 2; n(o) { return o.#x; } } return new B().n(this); } } new A().m()",
  "class A { #x = 1; m() { return eval('this.#x'); } } new A().m()",
  "class A { m() { return eval('this.#y'); } } new A().m()",
  "class A { #x = 1; m() { return new Function('o', 'return o.#x')(this); } } new A().m()",
  "class A { #x = 1; m() { return (0, eval)('1'); } } new A().m()",
  "class A { #x = 1; m() { const f = eval('(o) => o.#x'); return f(this); } } new A().m()",
  "class A { #x = 1; m() { return eval('#x in this'); } } new A().m()",
  "class A { #x = 1; static m() { return eval('A.#x'); } } A.m()",
  "class A { static #x = 1; static m() { return eval('A.#x'); } } A.m()",
  "class A { static #x = 1; static m() { return eval('eval(\"A.#x\")'); } } A.m()",
];
for (const body of named) add(body.replace(/^/, "") + (/;\s*$/.test(body) ? "" : ""));
// Os casos acima terminam numa expressão: o programa devolve o valor da última linha.
programs.length = programs.length;

// ---- 6. Herança de builtins.
const builtins = {
  Array: ["[1, 2]", "[]"],
  Error: ["'msg'", "'msg', { cause: 1 }"],
  TypeError: ["'msg'"],
  Map: ["[[1, 2]]", ""],
  Set: ["[1, 2]", ""],
  WeakMap: ["", ""],
  Promise: ["r => r(5)", ""],
  Function: ["'return 7'", "'a', 'return a'"],
  RegExp: ["'a+', 'g'", "/b/i"],
  Date: ["0", "2020, 1, 2"],
  Uint8Array: ["[1, 2, 3]", "4"],
  Float64Array: ["2", ""],
  ArrayBuffer: ["8", ""],
  Boolean: ["true", ""],
  Number: ["5", ""],
  String: ["'ab'", ""],
  Object: ["", "{ a: 1 }"],
  Symbol0: [],
  DataView: ["new ArrayBuffer(4)"],
  WeakSet: [""],
  AggregateError: ["[1], 'm'"],
};
const builtinObs = [
  "F(Reflect.getPrototypeOf(o) === D.prototype)", "o instanceof D", "o instanceof BASE", "Object.prototype.toString.call(o)",
  "Reflect.ownKeys(o).length", "F(o.constructor === D)", "D.name", "Reflect.getPrototypeOf(D) === BASE", "D.length",
  "typeof o", "String(o)", "Reflect.ownKeys(D.prototype)", "F(Reflect.ownKeys(D))",
];
for (const [name, argsList] of Object.entries(builtins)) {
  if (name === "Symbol0") continue;
  for (const args of argsList) {
    for (const obs of builtinObs) {
      add("const BASE = " + name + ";\nclass D extends BASE {}\nconst o = new D(" + args + ");\n" + obs);
    }
  }
}
// Métodos específicos herdados.
const builtinSpecific = [
  "class D extends Array {} const d = D.from([1, 2, 3]); F([d instanceof D, d.length, d.map(x => x) instanceof D, d.filter(x => x) instanceof D, d.slice() instanceof D, d.concat([1]) instanceof D])",
  "class D extends Array {} const d = new D(3); F([d.length, Array.isArray(d), 0 in d])",
  "class D extends Array {} const d = new D(1, 2); d.length = 0; F([d.length, d.push(9), d[0]])",
  "class D extends Array {} F(D.of(1, 2).length)",
  "class D extends Array { static get [Symbol.species]() { return Array; } } const d = new D(1, 2); F(d.map(x => x) instanceof D)",
  "class D extends Array { constructor(...a) { super(...a); this.extra = 1; } } const d = new D(1, 2); F([d.extra, d.map(x => x).extra, d.length])",
  "class D extends Array { constructor() { super(); this.push(1); } } F(new D().length)",
  "class D extends Array { constructor() { super(); this.push(1); } } F(new D().map(x => x).length)",
  "class D extends Error {} const e = new D('m'); F([e.message, e.name, e instanceof Error, String(e), Object.prototype.hasOwnProperty.call(e, 'stack'), typeof e.stack])",
  "class D extends Error { constructor(m) { super(m); this.name = 'D'; } } F(String(new D('boom')))",
  "class D extends Error {} D.prototype.name = 'DD'; F(String(new D('m')))",
  "class D extends Error { get name() { return 'G'; } } F(String(new D('m')))",
  "class D extends Error {} const e = new D('m', { cause: 'c' }); F([e.cause, Reflect.ownKeys(e).includes('cause')])",
  "class D extends Error {} F(new D().message === '' && !Reflect.ownKeys(new D()).includes('message'))",
  "class D extends Error {} F(new D(undefined).message)",
  "class D extends Error {} F(new D(5).message)",
  "class D extends Error {} F(Error.prototype.isPrototypeOf(D.prototype))",
  "class D extends Error {} F(D() )",
  "class D extends Map {} const m = new D([[1, 2]]); F([m.get(1), m.size, m instanceof Map, m.set(3, 4) === m])",
  "class D extends Map { set(k, v) { return super.set(k, v * 2); } } const m = new D([[1, 2]]); F(m.get(1))",
  "class D extends Map { constructor() { super(); this.set('a', 1); } } F(new D().size)",
  "class D extends Set { add(v) { return super.add(v + 1); } } F([...new D([1, 2])])",
  "class D extends Set {} const s = new D([1, 1, 2]); F([s.size, [...s]])",
  "class D extends Promise {} const p = D.resolve(1); F([p instanceof D, p.then(() => {}) instanceof D, D.all([]) instanceof D])",
  "class D extends Promise {} F(D.resolve(1) instanceof D)",
  "class D extends Promise { static get [Symbol.species]() { return Promise; } } F(D.resolve(1).then(() => {}) instanceof D)",
  "class D extends Promise { constructor(f) { super(f); this.tag = 1; } } F(D.resolve(1).tag)",
  "class D extends Promise { constructor() { super(() => {}); } } F(D.resolve(1) instanceof D)",
  "class D extends Promise { constructor(f) { f(() => {}, () => {}); } } F(typeof D.resolve)",
  "class D extends Promise {} F(D.name + D.length)",
  "class D extends Promise {} F(typeof new D(() => {}).then)",
  "class D extends Promise {} try { D(() => {}); } catch (e) { F(e.message); }",
  "class D extends Function {} const f = new D('a', 'return a + 1'); F([f(1), f instanceof D, typeof f, f.name, f.length])",
  "class D extends Function {} const f = new D(); F([f(), f.toString()])",
  "class D extends Function { constructor() { super('return this'); } } F(new D()() === globalThis)",
  "class D extends Function { constructor() { super('x', 'return x * 2'); this.tag = 1; } } const f = new D; F([f(4), f.tag])",
  "class D extends RegExp {} const r = new D('a+', 'g'); F([r.test('aa'), r.lastIndex, r.source, r.flags, r instanceof D, String(r)])",
  "class D extends RegExp { exec(s) { return null; } } F(new D('a').test('a'))",
  "class D extends RegExp { [Symbol.replace](s, r) { return 'X'; } } F('abc'.replace(new D('b'), 'y'))",
  "class D extends RegExp {} F('abab'.replace(new D('b', 'g'), 'X'))",
  "class D extends RegExp {} F('a-b'.split(new D('-')))",
  "class D extends RegExp {} F(new D('a').constructor === D)",
  "class D extends Date {} const d = new D(0); F([d.getTime(), d instanceof Date, d.toISOString(), typeof d, Object.prototype.toString.call(d)])",
  "class D extends Date { foo() { return this.getTime(); } } F(new D(5).foo())",
  "class D extends Date {} F(String(D.now === Date.now))",
  "class D extends Date {} F(D.UTC(2000, 0) === Date.UTC(2000, 0))",
  "class D extends Date {} F(typeof D() )",
  "class D extends Uint8Array {} const t = new D([1, 2, 300]); F([t.length, t[2], t instanceof D, t.map(x => x) instanceof D, t.subarray(1) instanceof D, t.slice() instanceof D, D.BYTES_PER_ELEMENT, D.from([1]) instanceof D, D.of(1) instanceof D])",
  "class D extends Uint8Array {} F(Object.prototype.toString.call(new D(1)))",
  "class D extends Uint8Array {} F(Reflect.getPrototypeOf(D) === Uint8Array)",
  "class D extends Uint8Array { static get [Symbol.species]() { return Uint8Array; } } F(new D(2).map(x => x) instanceof D)",
  "class D extends Float32Array {} F([new D(2).byteLength, new D(2).BYTES_PER_ELEMENT, D.BYTES_PER_ELEMENT])",
  "class D extends Uint8Array {} F(new D(new ArrayBuffer(8), 2, 3).byteOffset)",
  "class D extends ArrayBuffer {} const b = new D(8); F([b.byteLength, b instanceof D, b.slice(2) instanceof D, b.slice(2).byteLength])",
  "class D extends DataView {} F(new D(new ArrayBuffer(4)).byteLength)",
  "class D extends Boolean {} F([new D(false) ? 1 : 0, new D(false).valueOf(), typeof new D(true)])",
  "class D extends Number {} F([new D(5) + 1, new D(5).toFixed(1), typeof new D(1)])",
  "class D extends String {} const s = new D('abc'); F([s.length, s[1], s + '', s instanceof D, Reflect.ownKeys(s)])",
  "class D extends Object {} F([new D() instanceof D, Reflect.getPrototypeOf(new D) === D.prototype])",
  "class D extends Object {} F(new D(1) instanceof Number)",
  "class D extends Object {} F(typeof new D(1))",
  "class D extends Object {} const o = { a: 1 }; F(new D(o) === o)",
  "class D extends Object { constructor() { super(5); } } F(typeof new D())",
  "class D extends null {} F(Reflect.getPrototypeOf(D.prototype))",
  "class D extends null {} new D",
  "class D extends null { constructor() { return Object.create(D.prototype); } } F(new D instanceof D)",
  "class D extends null { constructor() { super(); } } new D",
  "class D extends null {} F([Reflect.getPrototypeOf(D) === Function.prototype, D.prototype.constructor === D, Reflect.ownKeys(D.prototype)])",
  "class D extends WeakMap {} const k = {}; const m = new D([[k, 1]]); F([m.get(k), m.has(k)])",
  "class D extends AggregateError {} const e = new D([1, 2], 'm'); F([e.errors, e.message, e.name, e instanceof Error])",
  "class D extends Symbol {} new D",
  "class D extends BigInt {} new D(1)",
  "class D extends Math {}",
  "class D extends JSON {}",
  "class D extends 1 {}",
  "class D extends 'a' {}",
  "class D extends {} {}",
  "class D extends undefined {}",
  "class D extends (() => {}) {}",
  "class D extends (function () {}) {} F(new D instanceof D)",
  "function B() { this.a = 1; } class D extends B {} F(new D)",
  "function B() { this.a = new.target === D; } class D extends B {} F(new D)",
  "function B() { return { z: 1 }; } class D extends B {} F(new D)",
  "function B() {} B.prototype = null; class D extends B {} F(Reflect.getPrototypeOf(D.prototype) === Object.prototype)",
  "function B() {} B.prototype = 1; class D extends B {}",
  "function B() {} B.prototype = {}; class D extends B {} F(Reflect.getPrototypeOf(D.prototype) === B.prototype)",
  "const B = function () {}.bind(); class D extends B {} F(typeof D)",
  "const B = async function () {}; class D extends B {}",
  "const B = function* () {}; class D extends B {}",
  "const B = async () => {}; class D extends B {}",
  "const B = ({ m() {} }).m; class D extends B {}",
  "class B { static get [Symbol.species]() { return 1; } } class D extends B {} F(1)",
  "const B = new Proxy(class {}, {}); class D extends B {} F(new D instanceof D)",
  "const B = new Proxy(function () {}, { construct(t, a, nt) { return { px: 1 }; } }); class D extends B {} F(new D)",
  "const B = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? { gp: 1 } : t[k]; } }); class D extends B {} F(D.prototype.__proto__.gp)",
  "const B = new Proxy({}, {}); class D extends B {}",
  "const B = new Proxy(function () {}, { getPrototypeOf() { return null; } }); class D extends B {} F(typeof D)",
  "class D extends Reflect.getPrototypeOf(async function* () {}).constructor {}",
  "class D extends Reflect.getPrototypeOf(async function () {}).constructor {} F(typeof new D('return 1'))",
  "class D extends Reflect.getPrototypeOf(function* () {}).constructor {} F(typeof new D('yield 1'))",
];
for (const body of builtinSpecific) add(body);

// ---- 7. new.target.
const newTarget = [
  "function f() { return new.target; } F([f() === undefined, new f() instanceof f ? 1 : typeof new f()])",
  "function f() { return typeof new.target; } F([f(), new f() instanceof f])",
  "function f() { this.nt = new.target; } F(new f().nt === f)",
  "function f() { this.nt = new.target; } F(Reflect.construct(f, [], Object).nt === Object)",
  "function f() { this.nt = new.target; } class G extends f {} F(new G().nt === G)",
  "class A { constructor() { this.nt = new.target; } } class B extends A {} F([new A().nt === A, new B().nt === B])",
  "class A { constructor() { this.nt = new.target.name; } } class B extends A {} class C extends B {} F(new C().nt)",
  "class A { constructor() { this.nt = new.target.name; } } F(Reflect.construct(A, [], function Foo() {}).nt)",
  "class A { constructor() { this.nt = new.target; } } const nt = function () {}; nt.prototype = { tag: 1 }; const o = Reflect.construct(A, [], nt); F([o.nt === nt, o.tag])",
  "class A { constructor() { this.p = Reflect.getPrototypeOf(this) === new.target.prototype; } } class B extends A {} F(new B().p)",
  "class A { static s() { return new.target; } } F(A.s())",
  "class A { m() { return new.target; } } F(new A().m())",
  "class A { constructor() { this.f = () => new.target; } } class B extends A {} F(new B().f() === B)",
  "class A { constructor() { this.f = function () { return new.target; }; } } F(new A().f())",
  "class A { x = new.target; } F(new A().x)",
  "class A { static x = new.target; } F(A.x)",
  "class A { constructor() { this.x = eval('new.target'); } } F(new A().x === A)",
  "class A { constructor() { this.x = (0, eval)('typeof new.target'); } }",
  "class A { constructor() { this.x = new.target === A; } } F([new A().x, Reflect.construct(A, [], Array).x])",
  "class A { constructor() { if (new.target === A) throw new TypeError('abstract'); } } class B extends A {} F(new B instanceof B)",
  "class A { constructor() { if (new.target === A) throw new TypeError('abstract'); } } new A",
  "class A { constructor() { new.target = 1; } }",
  "class A { constructor() { return new.target.name; } } F(typeof new A)",
  "class A { constructor() { this.n = new.target.prototype === A.prototype; } } F(new A().n)",
  "function f() { return new.target === undefined ? 'call' : 'new'; } F([f(), new f() instanceof f, Reflect.construct(f, []) instanceof f])",
  "function f() { return new.target === undefined ? 'call' : 'new'; } F([f.call({}), f.apply({}), f.bind()()])",
  "function f() { return new.target; } const b = f.bind(null); F([new b() instanceof f, Reflect.construct(b, []) instanceof f])",
  "function f() { this.nt = new.target; } const b = f.bind(null); F(new b().nt === f)",
  "const o = { m() { return new.target; } }; F(o.m())",
  "const o = { m: function () { return new.target; } }; F([o.m(), new o.m() instanceof o.m])",
  "const o = { m() { return new.target; } }; new o.m",
  "const f = () => new.target;",
  "new.target",
  "function f() { return () => () => new.target; } F(new f()()() === f)",
  "function f() { return () => new.target; } F(f()())",
  "function f() { return eval('() => new.target'); } F(typeof f()())",
  "function f() { return new Function('return new.target')(); } F(f())",
  "function f() { return new Function('return new.target'); } F(new (f())() instanceof Object)",
  "function* g() { yield new.target; } F([...g()])",
  "async function a() { return new.target; } a().then(v => { globalThis.R = F(v); }); 1",
  "class A { static { this.nt = new.target; } } F(A.nt)",
  "class A { static { this.f = () => new.target; } } F(A.f())",
  "class A { constructor() { Reflect.construct(Object, [], new.target); this.ok = 1; } } F(new A().ok)",
  "class A { constructor(...a) { this.v = a.length; } } class B extends A { constructor() { super(...arguments); } } F(new B(1, 2).v)",
  "class A { constructor() { this.nt = new.target; } } class B extends A { constructor() { super(); this.nt2 = new.target; } } const b = new B; F(b.nt === b.nt2)",
];
for (const body of newTarget) add(body);

// ---- 8. super em objetos literais, métodos estáticos e campos.
const superCases = [
  "const p = { m() { return 'p'; } }; const o = { __proto__: p, m() { return super.m() + 'o'; } }; F(o.m())",
  "const p = { get g() { return this.v; } }; const o = { __proto__: p, v: 5, get g() { return super.g + 1; } }; F(o.g)",
  "const p = { set s(v) { this.stored = v; } }; const o = { __proto__: p, set s(v) { super.s = v * 2; } }; o.s = 2; F(o.stored)",
  "const p = { m() { return this.n; } }; const o = { __proto__: p, n: 1, m() { return super.m(); } }; const q = { n: 2, m: o.m }; F(q.m())",
  "const p = { m() { return 1; } }; const o = { __proto__: p, m() { return super['m'](); } }; F(o.m())",
  "const p = { m() { return 1; } }; const k = 'm'; const o = { __proto__: p, m() { return super[k](); } }; F(o.m())",
  "const o = { m() { return super.toString === Object.prototype.toString; } }; F(o.m())",
  "const o = { m() { return super.x; } }; F(o.m())",
  "const o = { m() { super.x = 1; return Reflect.ownKeys(this); } }; F(o.m())",
  "const o = { m() { super.x = 1; return this.x; } }; F(o.m())",
  "const p = { x: 1 }; const o = { __proto__: p, m() { super.x = 2; return [Reflect.ownKeys(this), p.x, this.x]; } }; F(o.m())",
  "const p = { set x(v) { this.y = v; } }; const o = { __proto__: p, m() { super.x = 2; return this.y; } }; F(o.m())",
  "const p = { get x() { return 1; } }; const o = { __proto__: p, m() { super.x = 2; } }; o.m()",
  "const p = {}; Reflect.defineProperty(p, 'x', { value: 1, writable: false }); const o = { __proto__: p, m() { super.x = 2; } }; o.m()",
  "const o = { m() { return () => super.toString; } }; F(o.m()() === Object.prototype.toString)",
  "const p = { m() { return 'p'; } }; const o = { m() { return super.m(); } }; Reflect.setPrototypeOf(o, p); F(o.m())",
  "const p = { m() { return 'p'; } }, q = { m() { return 'q'; } }; const o = { __proto__: p, m() { return super.m(); } }; Reflect.setPrototypeOf(o, q); F(o.m())",
  "const o = { m() { return super.m(); } }; o.m()",
  "const o = { __proto__: null, m() { return super.x; } }; o.m()",
  "const o = { __proto__: null, m() { return super.x; } }; F(typeof o.m)",
  "const o = { m: function () { return super.x; } };",
  "const o = { m: () => super.x };",
  "const o = { get m() { return super.toString === Object.prototype.toString; } }; F(o.m)",
  "const o = { *g() { yield super.toString === Object.prototype.toString; } }; F([...o.g()])",
  "const o = { async m() { return super.toString === Object.prototype.toString; } }; o.m().then(v => { globalThis.R = F(v); }); 1",
  "const o = { ['c' + 1]() { return super.toString === Object.prototype.toString; } }; F(o.c1())",
  "const o = { m() { return { n() { return super.toString === Object.prototype.toString; } }.n(); } }; F(o.m())",
  "const o = { m() { return super.constructor === Object; } }; F(o.m())",
  "class A { static m() { return 'A.m'; } } class B extends A { static m() { return super.m() + '>B.m'; } } F(B.m())",
  "class A { static get g() { return this.name; } } class B extends A { static get g() { return super.g + '!'; } } F(B.g)",
  "class A { static set s(v) { this.stored = v; } } class B extends A { static set s(v) { super.s = v + 1; } } B.s = 1; F(B.stored)",
  "class A { static m() { return this; } } class B extends A { static m() { return super.m(); } } F(B.m() === B)",
  "class A { static x = 1; } class B extends A { static y = super.x + 1; } F(B.y)",
  "class A { static x = 1; } class B extends A { static y = () => super.x; } F(B.y())",
  "class A { static m() { return 1; } } class B extends A { static { this.v = super.m(); } } F(B.v)",
  "class A { m() { return 1; } } class B extends A { x = super.m() + 1; } F(new B().x)",
  "class A { m() { return 1; } } class B extends A { x = () => super.m(); } F(new B().x())",
  "class A { get g() { return 7; } } class B extends A { x = super.g; } F(new B().x)",
  "class A { m() { return 1; } } class B extends A { ['x'] = super.m(); } F(new B().x)",
  "class A { m() { return 1; } } class B extends A { [super.m()] = 1; }",
  "class A { m() { return 'A'; } } class B extends A { m() { return super.m() + 'B'; } } class C extends B { m() { return super.m() + 'C'; } } F(new C().m())",
  "class A { m() { return this.v; } } class B extends A { constructor() { super(); this.v = 3; } m() { return super.m(); } } F(new B().m())",
  "class A {} class B extends A { m() { super.x = 1; return Reflect.ownKeys(this); } } F(new B().m())",
  "class A {} class B extends A { m() { return super.x; } } F(new B().m())",
  "class A {} class B extends A { m() { return super.m(); } } new B().m()",
  "class A {} class B extends A { m() { return delete super.x; } } new B().m()",
  "class A {} class B extends A { m() { return delete super[(() => { throw 1; })()]; } } new B().m()",
  "class A { m() { return 1; } } class B extends A { m() { return super.m; } } F(new B().m() === A.prototype.m)",
  "class A { m() { return 1; } } class B extends A { m() { return super.m`x`; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { return new super.m(); } } new B().m()",
  "class A { static C = class {}; } class B extends A { m() { return new super.constructor.C() instanceof A.C; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { const { x = super.m() } = {}; return x; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { return [super.m, super['m']].length; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { return super.m?.(); } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { return super.n?.(); } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { super.n++; return this.n; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { super.n ??= 5; return this.n; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m() { super.n += 5; return this.n; } } F(new B().m())",
  "class A { m() { return 1; } } class B extends A { m(o) { return super.m.call(o); } } F(new B().m({}))",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { super(); this.b = super.constructor === A; } } F(new B())",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { const f = () => super(); f(); this.b = 2; } } F(new B())",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { const f = () => super(); f(); f(); } } new B",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { super(); super(); } } new B",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { eval('super()'); this.b = 2; } } F(new B())",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { (() => eval('super()'))(); this.b = 2; } } F(new B())",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { super(); this.b = (() => super.constructor.name)(); } } F(new B().b)",
  "class A { constructor() { this.a = 1; } } class B extends A { constructor() { super(); } m() { super(); } }",
  "class A { constructor() { this.a = 1; } } class B extends A { m() { return eval('super()'); } } new B().m()",
  "class A { constructor() { this.a = 1; } } class B { constructor() { super(); } }",
  "class A { constructor() { super(); } }",
  "class A { m() { super(); } }",
  "function f() { super.x; }",
  "function f() { super(); }",
  "const o = { m() { super(); } };",
  "super.x",
  "class A { constructor() { super.x; } } new A",
  "class A { static m() { return super.toString === Function.prototype.toString; } } F(A.m())",
  "class A { m() { return super.toString === Object.prototype.toString; } } F(new A().m())",
  "class A { static m() { return super.name; } } F(A.m())",
  "class A extends null { m() { return super.x; } } new A().m()",
  "class A extends null { static m() { return super.x; } } F(typeof A.m)",
  "class A { m() { return 1; } } const B = class extends A { m() { return super.m() + 1; } }; F(new B().m())",
  "class A { m() { return 1; } } const o = { __proto__: A.prototype, m() { return super.m() + 10; } }; F(o.m())",
  "class A { m() { return 1; } } const o = { m() { return super.m(); }, __proto__: A.prototype }; F(o.m())",
  "const o = { __proto__: { x: 1 }, m() { return super.x; } }; const c = { ...o }; F([c.m === o.m, c.m()])",
  "const o = { __proto__: { x: 1 }, m() { return super.x; } }; const c = { __proto__: { x: 2 }, m: o.m }; F(c.m())",
  "const m = ({ m() { return super.x; } }).m; F(m())",
  "const m = ({ m() { return super.x; } }).m; F(m.call({ x: 5 }))",
  "const m = ({ m() { return super.x; } }).m; F(m.call(undefined))",
  "const m = ({ m() { 'use strict'; return super.x; } }).m; F(m.call(null))",
  "const m = ({ get g() { return super.x; } }); F(m.g)",
];
for (const body of superCases) add(body);

// ---- 9. Retorno de construtor derivado e `this` antes de super.
const returns = ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n", "{}", "{ r: 1 }", "[]", "function () {}", "new Number(1)", "this", "new.target", "super0", "NaN", "0", "''", "false"];
for (const ret of returns) {
  const baseKinds = {
    plain: "class B {}",
    withField: "class B { bf = 1; }",
    returnsObject: "class B { constructor() { return { fromBase: 1 }; } }",
    returnsPrimitive: "class B { constructor() { return 5; } }",
  };
  for (const [bk, bdecl] of Object.entries(baseKinds)) {
    for (const mode of ["super-then-return", "return-only", "return-then-super", "return-in-try"]) {
      const ctorBody = {
        "super-then-return": `super(); return ${ret};`,
        "return-only": `return ${ret};`,
        "return-then-super": `if (true) return ${ret}; super();`,
        "return-in-try": `try { super(); return ${ret}; } finally { }`,
      }[mode];
      add(
        "const super0 = 0;\n" + bdecl + "\nclass D extends B { df = 2; constructor() { " + ctorBody + " } }\n" +
          "const r = new D;\nF([typeof r, r instanceof D, r instanceof B, Reflect.ownKeys(r === null || (typeof r !== 'object' && typeof r !== 'function') ? {} : r)])",
      );
    }
  }
  // Base class retornando valores.
  add("class A { constructor() { return " + ret + "; } }\nconst r = new A;\nF([typeof r, r instanceof A])");
  add("function A() { return " + ret + "; }\nconst r = new A;\nF([typeof r, r instanceof A])");
  add("class A { x = 1; constructor() { return " + ret + "; } }\nconst r = new A;\nF([typeof r, r instanceof A, typeof r === 'object' && r && Reflect.ownKeys(r)])");
}
const thisBeforeSuper = [
  "class A {} class B extends A { constructor() { this.x = 1; super(); } } new B",
  "class A {} class B extends A { constructor() { this; super(); } } new B",
  "class A {} class B extends A { constructor() { return this; } } new B",
  "class A {} class B extends A { constructor() { super(this); } } new B",
  "class A {} class B extends A { constructor() { super(this.x); } } new B",
  "class A {} class B extends A { constructor() { const f = () => this; f(); super(); } } new B",
  "class A {} class B extends A { constructor() { const f = () => this; super(); return f(); } } F(typeof new B)",
  "class A {} class B extends A { constructor() { super(); } } F(new B instanceof B)",
  "class A {} class B extends A { constructor() { } } new B",
  "class A {} class B extends A { constructor() { return; } } new B",
  "class A {} class B extends A { constructor() { return undefined; } } new B",
  "class A {} class B extends A { constructor() { return 1; } } new B",
  "class A {} class B extends A { constructor() { return null; } } new B",
  "class A {} class B extends A { constructor() { return {}; } } F(typeof new B)",
  "class A {} class B extends A { constructor() { try { this.x; } catch (e) { super(); return { n: e.name, m: e.message }; } } } F(new B)",
  "class A {} class B extends A { constructor() { try { this.x; } catch (e) { } super(); this.ok = 1; } } F(new B)",
  "class A {} class B extends A { constructor() { try { super(); } finally { } this.ok = 1; } } F(new B)",
  "class A { constructor() { throw 1; } } class B extends A { constructor() { try { super(); } catch (e) { } return this; } } new B",
  "class A { constructor() { throw 1; } } class B extends A { constructor() { try { super(); } catch (e) { } this.x = 1; } } new B",
  "class A {} class B extends A { constructor() { super(); this.x = 1; } x = 2; } F(new B)",
  "class A {} class B extends A { x = 2; constructor() { super(); F(this.x); } } new B",
  "class A { constructor() { this.v = this.x; } } class B extends A { x = 2; } F(new B)",
  "class A { constructor() { this.v = this.m(); } } class B extends A { m() { return 'B.m'; } } F(new B)",
  "class A { constructor() { this.v = this.f; } } class B extends A { f = 1; } F([new B().v, new B().f])",
  "class A { constructor() { this.v = this.#p; } #p = 1; } F(new A)",
  "class A { constructor() { this.v = this.#p; } } F(1)",
  "class A { #p = 1; constructor() { this.v = this.#p; } } class B extends A { #p = 2; get p() { return this.#p; } } F([new B().v, new B().p])",
  "class A { constructor(o) { return o; } } class B extends A { #q = 1; static g(o) { return o.#q; } } const o = {}; new B(o); F(B.g(o))",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = { x: 0 }; new B(o); F(o)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = Object.freeze({}); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = Object.freeze({ x: 0 }); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { ['x'] = 1; } const o = Object.preventExtensions({}); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = new Proxy({}, { defineProperty() { return true; } }); F(typeof new B(o))",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = new Proxy({}, { defineProperty() { return false; } }); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const log = []; const o = new Proxy({}, { defineProperty(t, k, d) { log.push(k); return Reflect.defineProperty(t, k, d); }, set(t, k, v) { log.push('set' + k); return true; } }); new B(o); F(log)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = { set x(v) { throw 1; } }; new B(o); F(Reflect.getOwnPropertyDescriptor(o, 'x'))",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = {}; Reflect.defineProperty(o, 'x', { value: 0, configurable: false }); new B(o)",
  "class A { constructor(o) { return o; } } class B extends A { x = 1; } const o = {}; Reflect.defineProperty(o, 'x', { value: 0, configurable: true, writable: false }); new B(o); F(Reflect.getOwnPropertyDescriptor(o, 'x'))",
];
for (const body of thisBeforeSuper) add(body);

// ---- 10. Decorators ausentes e `accessor`.
const syntaxCases = [
  "@dec class A {}", "class A { @dec m() {} }", "class A { @dec x = 1; }", "const dec = x => x; @dec class A {}", "class A { accessor x = 1; }",
  "class A { accessor x; }", "class A { static accessor x = 1; }", "class A { accessor #x = 1; }", "class A { accessor; }",
  "class A { accessor = 1; } F(Reflect.ownKeys(new A))", "class A { accessor\nx = 1; } F(Reflect.ownKeys(new A))", "class A { accessor() { return 1; } } F(new A().accessor())",
  "class A { static accessor() { return 1; } } F(A.accessor())", "class A { get accessor() { return 1; } } F(new A().accessor)",
  "class A { accessor x() {} }", "class A { accessor [k] = 1; }", "class A { async accessor x = 1; }",
  "class A { static async *m() {} } F(typeof A.m)", "class A { 'constructor'() {} } F(typeof new A)",
  "class A { ['constructor']() { return 1; } } F([typeof new A, new A().constructor === A, typeof A.prototype.constructor])",
  "class A { constructor() {} constructor() {} }", "class A { constructor() {} 'constructor'() {} }", "class A { constructor() {} ['constructor']() {} } F(1)",
  "class A { get constructor() {} }", "class A { *constructor() {} }", "class A { async constructor() {} }", "class A { static constructor() { return 1; } } F(A.constructor())",
  "class A { static prototype() {} }", "class A { static ['prototype']() {} }", "class A { static prototype = 1; }", "class A { static get prototype() {} }",
  "class A { prototype() { return 1; } } F(new A().prototype())", "class A { constructor = 1; }", "class A { 'constructor' = 1; }", "class A { ['constructor'] = 1; } F(Reflect.ownKeys(new A))",
  "class A { static constructor = 1; } F(A.constructor)", "class A { #constructor() {} }", "class A { static #prototype() {} }",
  "class A { static name = 'x'; } F(A.name)", "class A { static name() {} } F(typeof A.name)", "class A { static length = 5; } F(A.length)",
  "class A { static get name() { return 'G'; } } F(A.name)", "class A { static ['name'] = 1; } F(A.name)", "class A { static name; } F([A.name, Reflect.ownKeys(A)])",
  "class A { static { this.name = 'x'; } } F(A.name)", "class A { static caller = 1; } F(A.caller)", "class A { static arguments = 1; } F(A.arguments)",
  "class A { static call = 1; } F(typeof A.call)", "class A { static toString() { return 'custom'; } } F(String(A))",
  "class A { static [Symbol.hasInstance](v) { return v === 1; } } F([1 instanceof A, new A instanceof A])",
  "class A { static [Symbol.toPrimitive]() { return 5; } } F(A + 1)",
  "class A { [Symbol.toPrimitive]() { return 5; } } F(new A + 1)", "class A { get [Symbol.toStringTag]() { return 'Z'; } } F(String(new A))",
  "class A { static async m() {} } F(A.m.constructor.name)", "class A { static *m() {} } F(A.m.constructor.name)",
  "class A { 1() { return 'one'; } 1.5() { return 'x'; } 0x10() { return 'sixteen'; } 1n() { return 'big'; } } F(Reflect.ownKeys(A.prototype))",
  "class A { 'a b'() {} 'c'() {} } F(Reflect.ownKeys(A.prototype))", "class A { get 1() { return 1; } set 1(v) {} } F(Reflect.getOwnPropertyDescriptor(A.prototype, '1'))",
  "class A { m() {} m() {} } F(Reflect.ownKeys(A.prototype))", "class A { get m() { return 1; } m() {} } F(typeof Reflect.getOwnPropertyDescriptor(A.prototype, 'm').value)",
  "class A { m() {} get m() { return 1; } } F(typeof Reflect.getOwnPropertyDescriptor(A.prototype, 'm').get)",
  "class A { get m() { return 1; } set m(v) {} } const d = Reflect.getOwnPropertyDescriptor(A.prototype, 'm'); F([typeof d.get, typeof d.set, d.enumerable, d.configurable])",
  "class A { m() {} } const d = Reflect.getOwnPropertyDescriptor(A.prototype, 'm'); F([d.enumerable, d.configurable, d.writable])",
  "class A { static m() {} } const d = Reflect.getOwnPropertyDescriptor(A, 'm'); F([d.enumerable, d.configurable, d.writable])",
  "class A { x = 1; } const d = Reflect.getOwnPropertyDescriptor(new A, 'x'); F([d.enumerable, d.configurable, d.writable])",
  "class A { static x = 1; } const d = Reflect.getOwnPropertyDescriptor(A, 'x'); F([d.enumerable, d.configurable, d.writable])",
  "class A {} const d = Reflect.getOwnPropertyDescriptor(A, 'prototype'); F([d.enumerable, d.configurable, d.writable])",
  "class A {} const d = Reflect.getOwnPropertyDescriptor(A.prototype, 'constructor'); F([d.enumerable, d.configurable, d.writable])",
  "class A {} F(Reflect.ownKeys(A))", "class A { static x = 1; static m() {} } F(Reflect.ownKeys(A))", "class A extends Object {} F(Reflect.ownKeys(A))",
  "class A { constructor(a, b) {} } F(A.length)", "class A { constructor(a, b = 1, c) {} } F(A.length)", "class A { constructor(...a) {} } F(A.length)", "class A extends Object {} F(A.length)",
  "class A { m(a, b) {} } F(A.prototype.m.length)", "class A { get g() {} } F(Reflect.getOwnPropertyDescriptor(A.prototype, 'g').get.name)",
  "class A { set g(v) {} } F(Reflect.getOwnPropertyDescriptor(A.prototype, 'g').set.name)", "class A { set g(v) {} } F(Reflect.getOwnPropertyDescriptor(A.prototype, 'g').set.length)",
  "class A { static get g() {} } F(Reflect.getOwnPropertyDescriptor(A, 'g').get.name)",
  "const s = Symbol('d'); class A { get [s]() {} } F(Reflect.getOwnPropertyDescriptor(A.prototype, s).get.name)",
  "const s = Symbol(); class A { [s]() {} } F(A.prototype[s].name)", "const s = Symbol('d'); class A { [s]() {} } F(A.prototype[s].name)",
  "const s = Symbol('d'); class A { static [s]() {} } F(A[s].name)", "const s = Symbol('d'); class A { [s] = function () {}; } F(new A()[s].name)",
  "const s = Symbol(''); class A { [s] = function () {}; } F(new A()[s].name)", "const s = Symbol(); class A { [s] = function () {}; } F(new A()[s].name)",
  "const s = Symbol('d'); class A { [s] = class {}; } F(new A()[s].name)", "const s = Symbol('d'); class A { [s] = () => {}; } F(new A()[s].name)",
  "const s = Symbol('d'); class A { static [s] = () => {}; } F(A[s].name)", "const s = Symbol('d'); class A { #p = () => {}; get p() { return this.#p; } } F(new A().p.name)",
  "class A { #p = function () {}; get p() { return this.#p; } } F(new A().p.name)", "class A { #p = class {}; get p() { return this.#p; } } F(new A().p.name)",
  "class A { static #p = () => {}; static get p() { return A.#p; } } F(A.p.name)", "class A { #m() {} get m() { return this.#m; } } F(new A().m.name)",
  "class A { get #g() { return 1; } static f(o) { return Reflect.getOwnPropertyDescriptor(o, '#g'); } } F(A.f(new A))",
  "class A { static #m() {} static get m() { return A.#m; } } F(A.m.name)", "class A { async #m() {} get m() { return this.#m; } } F(new A().m.name)",
  "class A { *#m() {} get m() { return this.#m; } } F([new A().m.name, [...new A().m()].length])", "class A { async *#m() {} get m() { return this.#m; } } F(new A().m.name)",
  "class A { #m() {} get m() { return this.#m; } } F(typeof new A().m.prototype)", "class A { #m() {} get m() { return this.#m; } } new new A().m",
  "class A { x = function () {}; } F(new A().x.name)", "class A { x = function y() {}; } F(new A().x.name)", "class A { x = (function () {}); } F(new A().x.name)",
  "class A { x = (0, function () {}); } F(new A().x.name)", "class A { x = { m() {} }.m; } F(new A().x.name)", "class A { x = { y: function () {} }.y; } F(new A().x.name)",
  "class A { x = async function () {}; } F(new A().x.name)", "class A { x = function* () {}; } F(new A().x.name)", "class A { x = async () => {}; } F(new A().x.name)",
  "class A { x = class { static name = 'N'; }; } F(new A().x.name)", "class A { x = class { static name() {} }; } F(typeof new A().x.name)",
  "class A { 'x y' = function () {}; } F(new A()['x y'].name)", "class A { 1 = function () {}; } F(new A()[1].name)", "class A { [1 + 1] = function () {}; } F(new A()[2].name)",
  "class A { ['a' + 'b'] = function () {}; } F(new A().ab.name)", "class A { static x = function () {}; } F(A.x.name)", "class A { static x = () => {}; } F(A.x.name)",
  "const f = function () {}; F(f.name)", "const f = () => {}; F(f.name)", "const C = class {}; F(C.name)", "let C; C = class {}; F(C.name)", "var C; C = (class {}); F(C.name)",
  "const o = { C: class {} }; F(o.C.name)", "const o = { ['C']: class {} }; F(o.C.name)", "const o = {}; o.C = class {}; F(o.C.name === '')",
  "const C = class D {}; F(C.name)", "const C = class { static name = 'S'; }; F(C.name)", "const C = class { static name() {} }; F(typeof C.name)",
  "const { C = class {} } = {}; F(C.name)", "const [C = class {}] = []; F(C.name)", "function f(C = class {}) { return C.name; } F(f())",
  "const C = (0, class {}); F(C.name)", "const C = (class {}); F(C.name)", "let C = class extends Object {}; F(C.name)",
  "class A {} F(A.name)", "class A {} const B = A; F(B.name)", "F((class {}).name)", "F((class { static x = 1; }).name)", "export0 = class {}; F(export0.name)",
  "const C = class { static f = function () {}; }; F(C.f.name)", "const C = class { static s = this.name; }; F(C.s)", "const C = class { static s = C.name; }; F(C.s)",
  "const C = class N { static s = N.name; }; F(C.s)", "class A { static s = A.name; } F(A.s)", "class A { static s = this.name; } F(A.s)", "const C = class { static { this.n = this.name; } }; F(C.n)",
  "const C = class { x = this.constructor.name; }; F(new C().x)", "const o = { C: class { static n = this.name; } }; F(o.C.n)",
];
for (const body of syntaxCases) add(body);

// ---- 11. Binding do nome da classe.
const bindingCases = [
  "class A { static m() { return typeof A; } } F(A.m())", "class A { static m() { A = 1; } } A.m()", "class A { m() { A = 1; } } new A().m()",
  "class A {} A = 1; F(A)", "class A {} F(typeof A)", "F(typeof A); class A {}", "A; class A {}", "class A { static x = A; } F(A.x === A)",
  "class A extends A {}", "class A extends (A, Object) {}", "const B = class A { static m() { return A; } }; F(B.m() === B)",
  "const B = class A { static m() { A = 1; } }; B.m()", "const B = class A {}; F(typeof A)", "class A { [A.name]() {} }", "class A { [typeof A]() {} }",
  "let A = 1; { class A {} } F(A)", "class A { static [(() => { return 'k'; })()] = 1; } F(A.k)", "const x = 1; class A { m() { return x; } } F(new A().m())",
  "class A { m() { return B; } } class B {} F(typeof new A().m())", "const f = () => new A(); class A {} F(typeof f())", "const f = () => new A(); f(); class A {}",
  "class A { static x = B; } class B {}", "class B {} class A { static x = B; } F(typeof A.x)", "class A { static x = new A(); } F(A.x instanceof A)",
  "class A { x = new A(); } try { new A } catch (e) { F(e.name) }", "class A { static f = () => A; } F(A.f() === A)",
  "class A { static { A.x = 1; } } F(A.x)", "class A { static { var A = 1; } } F(typeof A)", "class A { static { let A = 1; this.v = A; } } F([A.v, typeof A])",
  "class A { static { function A() {} } } F(typeof A)", "class A { static { class A {} this.i = typeof A; } } F(A.i)", "class A { static { this.t = typeof A; } } F(A.t)",
  "class A { constructor() { A = 1; } } new A", "class A { static m() { eval('A = 1'); } } A.m()", "class A { static m() { return eval('typeof A'); } } F(A.m())",
  "class A { static m() { return (() => A)(); } } F(A.m() === A)", "class A {} class A {}", "class A {} var A;", "var A; class A {}", "let A; class A {}", "class A {} function A() {}",
  "{ class A {} } F(typeof A)", "if (true) class A {}", "label: class A {}", "for (;;) class A {}", "while (0) class A {}",
  "class let {}", "class static {}", "class yield {}", "class await {}", "class implements {}", "class enum {}", "class async {} F(typeof async)", "class of {} F(typeof of)",
  "class get {} F(typeof get)", "class set {} F(typeof set)", "class A { static static() { return 1; } } F(A.static())", "class A { static get static() { return 1; } } F(A.static)",
  "class A { static async() { return 1; } } F(A.async())", "class A { async() { return 1; } } F(new A().async())", "class A { get() { return 1; } set() { return 2; } static() { return 3; } } F([new A().get(), new A().set(), new A().static()])",
  "class A { get\nx() { return 1; } } F(new A().x)", "class A { static\nx() {} } F(typeof A.x)", "class A { async\nx() {} } F(Reflect.ownKeys(A.prototype))",
  "class A { x\ny } F(Reflect.ownKeys(new A))", "class A { x = 1\ny = 2 } F(Reflect.ownKeys(new A))", "class A { x\n*y() {} }", "class A { x = 1\n*y() {} }", "class A { x\n['y']() {} }",
  "class A { x = 1\n['y'] = 2 } F(Reflect.ownKeys(new A))", "class A { get\n*x() {} }", "class A { static\n{ this.v = 1; } } F(A.v)", "class A { x;; }", "class A { ;; x() {} } F(typeof new A().x)",
  "class A { x = 1, y = 2 }", "class A { x = 1 y = 2 }", "class A { x y }", "class A { x\ny = 1 } F(Reflect.ownKeys(new A))", "class A { static x\ny } F(Reflect.ownKeys(A))",
  "class A { in\nx } F(Reflect.ownKeys(new A))", "class A { x = in }", "class A { 'x'; 'y' } F(Reflect.ownKeys(new A))", "class A { [1]; [2] } F(Reflect.ownKeys(new A))",
];
for (const body of bindingCases) add(body);

// ---- 12. toString de classe, métodos e acessores.
const toStrings = [
  "class A {}", "class A { }", "class A{}", "class  A  extends  Object  { }", "class A { m() {} }", "class A { static s() {} }", "class A { get g() { return 1; } set g(v) {} }",
  "class A { x = 1; #y = 2; static z = 3; }", "class A { static { this.a = 1; } }", "class A { constructor(a, b) { this.a = a; } }", "class A { /* c */ }", "class A { // c\n}",
  "class\nA\n{\n}", "class A { 'quoted'() {} }", "class A { [computed]() {} }", "class A { *g() {} async a() {} async *ag() {} }", "class A { #p() {} get #q() { return 1; } }",
  "class A extends (class B {}) {}", "class A extends Object { constructor() { super(); } }", "class A { é() {} }", "class A { static async *m() {} }",
  "(class {})", "(class extends Object {})", "(class N {})", "class A { a() {} }\nclass B { b() {} }",
];
const methodStrings = [
  "class A { m() {} }; A.prototype.m", "class A { m( a , b ) { return a ; } }; A.prototype.m", "class A { static s() {} }; A.s", "class A { get g() { return 1; } }; Reflect.getOwnPropertyDescriptor(A.prototype, 'g').get",
  "class A { set g(v) {} }; Reflect.getOwnPropertyDescriptor(A.prototype, 'g').set", "class A { static get g() { return 1; } }; Reflect.getOwnPropertyDescriptor(A, 'g').get",
  "class A { *g() {} }; A.prototype.g", "class A { async a() {} }; A.prototype.a", "class A { async *ag() {} }; A.prototype.ag", "class A { 'q w'() {} }; A.prototype['q w']",
  "class A { [Symbol.iterator]() {} }; A.prototype[Symbol.iterator]", "class A { 1() {} }; A.prototype[1]", "class A { #p() {} get p() { return this.#p; } }; new A().p",
  "class A { x = () => 1; }; new A().x", "class A { x = function () {}; }; new A().x", "class A { static x = () => 1; }; A.x", "class A { constructor() {} }; A.prototype.constructor",
  "class A { m() {} }; A.prototype.m.bind(null)", "class A { m() {} }; Function.prototype.toString.call(A.prototype.m.bind(null))", "({ m() {} }).m", "({ get g() { return 1; } }, 0)",
  "({ async m() {} }).m", "({ *m() {} }).m", "({ ['c']() {} }).c", "({ 'q'() {} }).q", "({ m: function () {} }).m", "({ m: () => {} }).m", "(class { static m() {} }).m",
  "class A { m() {} }; class B extends A {}; B.prototype.m", "class A { m() { /* body */ } }; A.prototype.m", "class A { m() { // c\n } }; A.prototype.m",
];
for (const src of toStrings) {
  add("const C = (0, " + (src.startsWith("(") ? src : "(" + src + ")") + ");\nconst computed = 'c';\nF(Function.prototype.toString.call(C))");
  add(src + "\nF(String(" + (/^\(/.test(src) ? src : src.match(/^class\s+(\w+)/)[1]) + "))");
  add(src + "\nF(" + (/^\(/.test(src) ? src : src.match(/^class\s+(\w+)/)[1]) + ".toString().length)");
}
for (const src of methodStrings) add("const computed = 'c';\nconst r = (() => { " + (src.startsWith("(") || src.startsWith("class") ? "" : "") + src.replace(/; (?=[A-Z(])/, ";\nreturn ").replace(/^class/, "class") + " })();\nF(typeof r)");
for (const src of methodStrings) {
  const parts = src.split(/;\s+(?=[^;]+$)/);
  if (parts.length === 2) add(parts[0] + ";\nF(Function.prototype.toString.call(" + parts[1] + "))");
  else add("F(Function.prototype.toString.call(" + src + "))");
}

// ---- 13. Getters e setters estáticos e de instância.
const accessorCases = [
  "class A { static get g() { return 1; } } F([A.g, new A().g, Reflect.ownKeys(A)])", "class A { static set s(v) { this.v = v; } } A.s = 5; F([A.v, A.s])",
  "class A { static get g() { return this; } } class B extends A {} F(B.g === B)", "class A { static get g() { return 1; } static set g(v) { this.v = v; } } A.g = 3; F([A.g, A.v])",
  "class A { static get g() { return 1; } } A.g = 2; F(A.g)", "class A { static get g() { return 1; } } (() => { A.g = 2; })()", "class A { get g() { return 1; } } const a = new A; a.g = 2; F(a.g)",
  "class A { get g() { return 1; } } const a = new A; (() => { a.g = 2; })()", "class A { set s(v) { this.v = v; } } const a = new A; a.s = 4; F([a.v, a.s])",
  "class A { get g() { return 1; } } class B extends A { set g(v) {} } F(new B().g)", "class A { get g() { return 1; } set g(v) { this.v = v; } } class B extends A { get g() { return 2; } } const b = new B; b.g = 9; F([b.g, b.v])",
  "class A { static get [Symbol.species]() { return 1; } } F(A[Symbol.species])", "class A { static get ['a' + 'b']() { return 1; } } F(A.ab)", "class A { static get 1() { return 'one'; } } F(A[1])",
  "class A { static get 'x y'() { return 1; } } F(A['x y'])", "class A { get g() { return 1; } } F(Reflect.getOwnPropertyDescriptor(A.prototype, 'g'))", "class A { get g() { return 1; } } F(typeof Reflect.getOwnPropertyDescriptor(A.prototype, 'g').set)",
  "class A { static get g() { return this.x; } static x = 5; } F(A.g)", "class A { static set s(v) { } } F(Reflect.getOwnPropertyDescriptor(A, 's').get)", "class A { static set s(v) { } } F(A.s)",
  "class A { get g() { return 1; } static get g() { return 2; } } F([new A().g, A.g])", "class A { get g() { return 1; } } const d = Reflect.getOwnPropertyDescriptor(A.prototype, 'g'); F(d.get.call({}))",
  "class A { get g() { return 1; } } new (Reflect.getOwnPropertyDescriptor(A.prototype, 'g').get)", "class A { get g() {} set g(a, b) {} }", "class A { set g() {} }", "class A { set g(...a) {} }", "class A { set g(a = 1) {} } F(1)",
  "class A { set g([a]) { this.a = a; } } const o = new A; o.g = [3]; F(o.a)", "class A { set g({ a }) { this.a = a; } } const o = new A; o.g = { a: 3 }; F(o.a)", "class A { get g(a) {} }",
  "class A { static set s(v) { this.v = v; } } class B extends A {} B.s = 2; F([Reflect.ownKeys(B), A.v, B.v])", "class A { static x = 1; } class B extends A {} B.x = 2; F([A.x, B.x, Reflect.ownKeys(B)])",
  "class A { static x = 1; } class B extends A {} F([B.x, Reflect.ownKeys(B)])", "class A { static x = []; } class B extends A {} B.x.push(1); F(A.x)", "class A { x = []; } const a = new A, b = new A; a.x.push(1); F(b.x)",
  "class A { static m() { return this.name; } } class B extends A {} F([A.m(), B.m()])", "class A { static m() { return this; } } const m = A.m; F(m())", "class A { m() { return this; } } const m = new A().m; F(m())",
  "class A { m() { return typeof this; } } F(A.prototype.m.call(1))", "class A { m() { return this; } } F(A.prototype.m.call('s'))", "class A { m() { return this; } } F(A.prototype.m.call(null))",
  "class A { static async m() { return this; } } A.m().then(v => { globalThis.R = F(v === A); }); 1",
  "class A { 'use strict'; }", "class A { m() { return this === undefined; } } F(A.prototype.m.call(undefined))", "class A { m() { with0: return 1; } } F(new A().m())",
  "class A { m() { return arguments.length; } } F(new A().m(1, 2))", "class A { m() { return arguments.callee; } } new A().m()", "class A { m() { return m; } } new A().m()",
  "class A { m() { return typeof m; } } F(new A().m())", "class A { m() { return new.target; } } F(new A().m())", "class A { static caller() {} } F(typeof A.caller)", "class A { m() { return A.prototype.m.caller; } } new A().m()",
  "class A { m() {} } F(Reflect.ownKeys(A.prototype.m))", "class A { m() {} } F(Reflect.has(A.prototype.m, 'prototype'))", "class A { m() {} } new A.prototype.m", "class A { *m() {} } F(Reflect.has(A.prototype.m, 'prototype'))",
  "class A { m() {} } F(A.prototype.m.hasOwnProperty('caller'))", "class A {} A()", "class A {} A.call({})", "class A {} A.apply(null)", "class A {} Reflect.apply(A, null, [])", "class A {} Reflect.construct(A, [])", "class A {} new A.bind()()",
  "class A {} const B = A.bind(null); F(new B() instanceof A)", "class A {} const B = A.bind(null); B()", "class A {} F(typeof A.prototype)", "class A {} A.prototype = 1; F(typeof A.prototype)", "class A {} F(Reflect.set(A, 'prototype', 1))",
  "class A {} F(Reflect.defineProperty(A, 'prototype', { value: {} }))", "class A {} F(Reflect.deleteProperty(A, 'prototype'))", "class A {} F(Reflect.deleteProperty(A.prototype, 'constructor'))",
  "class A {} F(Reflect.isExtensible(A.prototype))", "class A {} F(Reflect.getPrototypeOf(A.prototype) === Object.prototype)", "class A {} F(Reflect.getPrototypeOf(A) === Function.prototype)",
  "class A extends Array {} F([Reflect.getPrototypeOf(A) === Array, Reflect.getPrototypeOf(A.prototype) === Array.prototype])", "class A {} F(A.prototype.constructor === A)", "class A {} F(String(A.prototype))",
  "class A {} F(Object.prototype.toString.call(A))", "class A {} F(typeof A)", "class A {} F(A instanceof Function)", "class A {} F(A.constructor === Function)", "class A {} F(Reflect.ownKeys(A.prototype))",
  "class A { static async *[Symbol.asyncIterator]() {} } F(typeof A[Symbol.asyncIterator])", "class A { static *[Symbol.iterator]() { yield 1; yield 2; } } F([...A])",
  "class A { *[Symbol.iterator]() { yield 1; } } F([...new A])", "class A { async *[Symbol.asyncIterator]() { yield 1; } } F(typeof new A()[Symbol.asyncIterator])",
  "class A { [Symbol.iterator]() { return [1][Symbol.iterator](); } } F([...new A])", "class A { get [Symbol.toStringTag]() { return 'T'; } } F(Object.prototype.toString.call(new A))",
  "class A { static [Symbol.hasInstance]() { return true; } } F(1 instanceof A)", "class A { toString() { return 'ts'; } } F(`${new A}`)", "class A { valueOf() { return 7; } } F(new A + 1)",
  "class A { toJSON() { return { j: 1 }; } } F(JSON.stringify(new A))", "class A { x = 1; #y = 2; } F(JSON.stringify(new A))", "class A { get x() { return 1; } } F(JSON.stringify(new A))",
  "class A { x = 1; } F(JSON.stringify({ a: new A }))", "class A { static x = 1; } F(JSON.stringify(A))", "class A { x = 1; } F(structuredClone0(new A))",
];
const sc = "const structuredClone0 = o => { try { return structuredClone(o); } catch (e) { return e.name; } };\n";
for (const body of accessorCases) add((body.includes("structuredClone0") ? sc : "") + body);

// ---- 14. Execução: roda cada programa no bun e emite o TSV.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "class-golden-"));
const file = path.join(dir, "class_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  const original = body.replace(/(?<![.\w])R = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  const { source, executable, meta } = prepareProgram(original);
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, env: { ...process.env, TZ: "America/Sao_Paulo" } });
  const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
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
  rows.push({ source, result, meta });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("class", rows));
fs.rmSync(dir, { recursive: true, force: true });
