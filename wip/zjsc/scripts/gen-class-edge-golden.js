// Gera tests/golden/class_edge_bun.tsv: complemento de borda de gen-class-golden.js e gen-brand-check-golden.js,
// medido no bun 1.4.2. Matrizes de: campos e métodos privados (#x, #m(), static #s, `#x in o`), acessores privados
// (só get, só set, atribuição a método, escrita em getter), static blocks (this, ordem, escopo, await/arguments),
// ordem de inicialização com herança (campos, computed keys, retorno de objeto no construtor base), new.target,
// super em métodos, estáticos e literais, extends null/function/proxy/não construtor, Symbol.species, erros (acesso a
// #x em objeto errado, redeclaração, construtor sem new, derived sem super, super duas vezes) e toString de classe.
// Programas cujo código já está em class_bun.tsv ou brand_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-class-edge-golden.js > tests/golden/class_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function:"+v.name:' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const bodies = [];
let tick = 0;
// Amostra determinística: mantém um a cada n chamadas, para as matrizes não dominarem o golden.
const pick = n => tick++ % n === 0;
const add = (...list) => bodies.push(...list);

// ---- 1. Privados: forma da declaração x operação x receptor.
const decls = [
  ["field", "#p = 1;", "this.#p", "this.#p = 2", "#p in o"],
  ["field-uninit", "#p;", "this.#p", "this.#p = 2", "#p in o"],
  ["method", "#p() { return 7; }", "this.#p()", "this.#p = 2", "#p in o"],
  ["getter", "get #p() { return 8; }", "this.#p", "this.#p = 2", "#p in o"],
  ["setter", "set #p(v) { this.log = v; }", "this.#p", "this.#p = 2", "#p in o"],
  ["accessor-pair", "get #p() { return 9; } set #p(v) { this.log = v; }", "this.#p", "this.#p = 3", "#p in o"],
  ["static-field", "static #p = 10;", "A.#p", "A.#p = 2", "#p in o"],
  ["static-method", "static #p() { return 11; }", "A.#p()", "A.#p = 2", "#p in o"],
  ["static-getter", "static get #p() { return 12; }", "A.#p", "A.#p = 2", "#p in o"],
  ["static-setter", "static set #p(v) { A.log = v; }", "A.#p", "A.#p = 2", "#p in o"],
  ["async-method", "async #p() { return 1; }", "typeof this.#p", "this.#p = 2", "#p in o"],
  ["gen-method", "*#p() { yield 1; }", "[...this.#p()]", "this.#p = 2", "#p in o"],
  ["async-gen-method", "async *#p() { yield 1; }", "typeof this.#p().next", "this.#p = 2", "#p in o"],
];
const receivers = [
  ["inst", "new A"], ["class", "A"], ["plain", "{}"], ["proto", "A.prototype"], ["sub", "new B"], ["null", "null"],
  ["undef", "undefined"], ["num", "1"], ["str", "'s'"], ["fn", "function(){}"], ["proxy", "new Proxy(new A, {})"],
  ["create", "Object.create(new A)"], ["arr", "[]"],
];
for (const [name, decl, read, write, has] of decls) {
  const cls = `class A { ${decl} static read(o) { return ${read.replace(/this|A(?=\.#)/g, "o")}; } static write(o) { ${write.replace(/this|A(?=\.#)/g, "o")}; } static has(o) { return ${has}; } static call(o) { return typeof o.#p; } }\nclass B extends A {}\n`;
  for (const [rname, rx] of receivers) {
    if (pick(8)) add(cls + `return [A.read(${rx})]`, cls + `A.write(${rx}); return "ok"`, cls + `return A.has(${rx})`);
  }
}
for (const [name, decl] of decls) {
  add(`class A { ${decl} static t(o) { return #p in o; } }\nclass B extends A {}\nreturn [A.t(new A),A.t(new B),A.t(A),A.t(B),A.t(A.prototype),A.t(Object.create(new A)),A.t(new Proxy(new A,{}))]`);
  add(`class A { ${decl} static t(o) { return #p in o; } }\nreturn A.t(1)`, `class A { ${decl} static t(o) { return #p in o; } }\nreturn A.t(null)`,
    `class A { ${decl} static t(o) { return #p in o; } }\nreturn A.t("x")`, `class A { ${decl} static t(o) { return #p in o; } }\nreturn A.t(Symbol())`);
}

// ---- 2. Privados: erros de sintaxe e semântica estática (cada fonte vira new Function para capturar o SyntaxError).
const syntaxCases = [
  "class A { #x; #x; }", "class A { #x; #x() {} }", "class A { #x() {} #x() {} }", "class A { get #x() {} get #x() {} }",
  "class A { get #x() {} set #x(v) {} }", "class A { get #x() {} static set #x(v) {} }", "class A { static get #x() {} set #x(v) {} }",
  "class A { static #x; #x; }", "class A { #x; get #x() {} }", "class A { #constructor; }", "class A { #constructor() {} }", "class A { static #constructor; }",
  "class A { m() { this.#y; } }", "class A { m() { #y in this; } }", "class A { #x; m() { delete this.#x; } }", "class A { #x; m() { delete (this.#x); } }",
  "class A { #x; m() { delete ((this.#x)); } }", "class A { #x; m() { delete this?.#x; } }", "class A { #x; m() { return this?.#x; } }", "class A { #x; m() { return this?.a.#x; } }",
  "class A { #x; m() { return #x; } }", "class A { #x; m() { return #x in #x in this; } }", "class A { #x; m() { return (#x) in this; } }", "class A { #x; m() { return 1 + #x in this; } }",
  "class A { #x; m() { return #x in this in this; } }", "class A { #x; m() { return #x < this; } }", "class A { #x; m() { for (#x in this); } }", "class A { #x; m() { return #x in this + 1; } }",
  "class A { #x; m() { return super.#x; } }", "class A { #x; m() { return this.# x; } }", "class A { #x; m() { return this.#\\u0078; } }", "class A { #\\u0078; m() { return this.#x; } }",
  "class A { # x; }", "class A { #1; }", "class A { #; }", "class A { #x = arguments; }", "class A { #x = () => arguments; }", "class A { x = arguments; }", "class A { static { arguments; } }",
  "class A { static { await; } }", "class A { static { return; } }", "class A { static { yield; } }", "class A { static { super(); } }", "class A { static { var await; } }", "class A { static { function await() {} } }",
  "class A { static { var x; var x; } }", "class A { static { let x; var x; } }", "class A { static { break; } }", "class A { static { x: { break x; } } }", "class A { static { new.target; } }",
  "class A { static { this; super.x; } }", "class A { static { async function f() { await 1; } } }", "class A { static { class B { static { await; } } } }", "class A { static { ({ await }); } }",
  "class A { static { (await) => 1; } }", "class A { static { await: 1; } }", "class A { static { () => { arguments; }; } }", "class A { static { function f() { arguments; } } }",
  "class A { constructor() {} constructor() {} }", "class A { constructor() {} 'constructor'() {} }", "class A { get constructor() {} }", "class A { set constructor(v) {} }", "class A { *constructor() {} }",
  "class A { async constructor() {} }", "class A { async *constructor() {} }", "class A { static constructor() {} }", "class A { static prototype() {} }", "class A { static get prototype() {} }",
  "class A { static prototype; }", "class A { static 'prototype'; }", "class A { static ['prototype']() {} }", "class A { constructor; }", "class A { 'constructor'; }", "class A { ['constructor'] = 1; }",
  "class A { static constructor; }", "class A { static 'constructor' = 1; }", "class A { constructor() { super(); } }", "class A extends B { constructor() { super(); } }", "class A { m() { super(); } }",
  "class A extends B { m() { super(); } }", "class A extends B { x = super(); }", "class A extends B { x = super.y; }", "class A { x = super.y; }", "class A { static x = super.y; }",
  "class A extends B { static m() { super(); } }", "class A extends B { constructor() { (() => super())(); } }", "class A extends B { constructor() { function f() { super(); } } }",
  "class A extends B { constructor() { eval('super()'); } }", "class A extends B { m() { eval('super()'); } }", "class A extends B { m() { eval('super.x'); } }", "class A { m() { eval('super.x'); } }",
  "class A { m() { eval('new.target'); } }", "class A { x = eval('new.target'); }", "class A { x = eval('arguments'); }", "class A { x = () => eval('arguments'); }",
  "class A extends B, C {}", "class A extends {} {}", "class A extends () => {} {}", "class A extends async function(){} {}", "class A extends (B, C) {}", "class A extends B ? C : D {}", "class A extends B.C {}",
  "class A extends B() {}", "class A extends B`x` {}", "class A extends new B {}", "class A extends new B() {}", "class A extends -B {}", "class A extends B++ {}", "class A extends typeof B {}",
  "class A extends B = C {}", "class A extends (B = C) {}", "class A extends class {} {}", "class A extends class extends B {} {}", "class A extends function(){} {}", "class A extends null {}",
  "class A extends null { constructor() { super(); } }", "class A extends null { constructor() { return Object.create(A.prototype); } }", "class A extends null { constructor() { } }",
  "class { }", "class extends B { }", "class A { , }", "class A { ; ; }", "class A { m() {}; ; n() {} }", "class A { static }", "class A { static static }", "class A { static static() {} }",
  "class A { get }", "class A { set }", "class A { async }", "class A { get; set; async; static; }", "class A { get\n x() {} }", "class A { async\n x() {} }", "class A { static async\n x() {} }",
  "class A { get = 1; set = 2; static = 3; async = 4; }", "class A { x\n y }", "class A { x\n *y() {} }", "class A { x = 1\n *y() {} }", "class A { x\n [y] }", "class A { x = 1\n [y] = 2 }", "class A { get\n *x() {} }",
  "class A { x y }", "class A { x = 1 y = 2 }", "class A { x = 1, y = 2 }", "class A { 'x' = 1; 2 = 3; [4] = 5; }", "class A { in; instanceof; }", "class A { x = in }", "class A { yield = 1 }", "class A { await = 1 }",
  "class A { x = yield }", "function* g() { class A { [yield] = 1 } }", "function* g() { class A { x = yield; } }", "async function f() { class A { [await 1] = 1 } }", "async function f() { class A { x = await 1; } }",
  "async function f() { class A { static { await 1; } } }", "class A { static async *#m() {} }", "class A { static async #m() {} static async *#n() {} }", "class A { get #m() {} get #m() {} }",
  "class let {}", "class yield {}", "class await {}", "class static {}", "class implements {}", "class eval {}", "class arguments {}", "class async {}", "class of {}", "class get {}", "class A extends eval {}",
  "class A { m(eval) {} }", "class A { m(a, a) {} }", "class A { m(a = 1, a) {} }", "class A { m() { var let; } }", "class A { m() { with (x) {} } }", "class A { m() { 010; } }", "class A { m() { '\\01'; } }", "class A { m() { delete x; } }",
  "class A { m() { arguments = 1; } }", "class A { m() { eval = 1; } }", "class A { m() { var package; } }", "class A { m() { var public; } }", "class A { [m]() {} }", "class A { [m] }", "class A { [m] = 1 }", "class A { [a, b]() {} }",
  "class A { [(a, b)]() {} }", "class A { static async *[Symbol.iterator]() {} }", "class A { static get [Symbol.species]() { return 1; } }", "class A { get [Symbol.species]() { return 1; } static get [Symbol.species]() { return 2; } }",
  "(class A { #x; static m(o) { return #x in o; } })", "(class { #x; static m(o) { return #x in o; } })", "(class A { static #x = 1; static m() { return A.#x; } })",
  "class A { #x; m() { class B { m(o) { return o.#x; } } } }", "class A { m() { class B { #y; } return this.#y; } }", "class A { m() { class B { #y; n() { return this.#y; } } } }",
  "class A { #x; m() { class B { #x; n(o) { return o.#x; } } } }", "class A { #x; m() { return eval('this.#x'); } }", "class A { m() { return eval('this.#z'); } }", "class A { #x; m() { return new Function('return this.#x'); } }",
  "class A { #x; m() { return (0, eval)('this.#x'); } }",
];
for (const src of syntaxCases) {
  if (pick(3)) add(`return typeof new Function(${JSON.stringify(src)})`);
}

// ---- 3. Static blocks e ordem.
const blockCases = [
  "class A { static { R2 = this === A; } } return R2", "class A { static x = 1; static { this.y = this.x + 1; } static z = this.y + 1; } return [A.x,A.y,A.z]",
  "const l=[]; class A { static a = l.push('a'); static { l.push('b'); } static c = l.push('c'); static { l.push('d'); } } return l",
  "const l=[]; class A { static { l.push(typeof A); } } return l", "const l=[]; class A { static { l.push(A === this); } } return l",
  "const l=[]; class A { static { l.push(typeof B); } } class B {} return l", "var t; class A { static { try { t = B; } catch (e) { t = e.name; } } } class B {} return t",
  "let r; class A { static { r = (() => this)(); } } return r === A", "let r; class A { static { r = (function() { return this; })(); } } return r",
  "let r; class A { static { var v = 1; r = typeof v; } } return [r, typeof v]", "let r; class A { static { let v = 1; r = v; } static { r += typeof v; } } return r",
  "class A { static { var v = 1; } static { return typeof v; } }", "class A { static #p = 1; static { A.q = A.#p + 1; } } return A.q",
  "class A { static { this.#p = 3; } static #p; } return 1", "class A { static #p; static { A.q = A.#p; this.#p = 3; A.r = A.#p; } } return [A.q, A.r]",
  "class A { static { this.constructor2 = 1; } } return Object.getOwnPropertyNames(A).join()", "class A { static { this.name2 = this.name; } } return A.name2",
  "const A = class { static { this.n = this.name; } }; return A.n", "const A = class B { static { this.n = this.name; B.k = 1; } }; return [A.n, A.k]",
  "let o = { A: class { static { this.n = this.name; } } }; return o.A.n", "class A { static { throw new Error('boom'); } }", "try { class A { static { throw new RangeError('boom'); } } } catch (e) { return e.name; }",
  "let c = 0; try { class A { static { c++; throw 1; } } } catch (e) {} try { class A { static { c++; } } } catch (e) {} return c",
  "class A { static { A.x = 1; } static x2 = A.x; } return A.x2", "class A { static x2 = A.x; static { A.x = 1; } } return [A.x2, A.x]",
  "class A { static { function f() { return 1; } A.f = f; } } return A.f()", "class A { static { class B { static { B.v = 1; } } A.b = B.v; } } return A.b",
  "class A { static { let x = 1; { let x = 2; } A.x = x; } } return A.x", "class A { static { label: { A.x = 1; break label; } } } return A.x", "class A { static { for (let i = 0; i < 3; i++) A['k' + i] = i; } } return Object.keys(A).join()",
  "class A { static { A.f = () => this; } } return A.f() === A", "class A { static { A.f = () => new.target; } } return A.f()", "class A { static { A.v = new.target; } } return typeof A.v",
  "class A { static { A.v = typeof super.toString; } } return A.v", "class B { static m() { return 'bm'; } } class A extends B { static { A.v = super.m(); } } return A.v",
  "class B { static m() { return this === A; } } class A extends B { static { A.v = super.m(); } } return A.v", "class B { static x = 1; } class A extends B { static { A.v = super.x; } } return A.v",
  "class B { static x = 1; } class A extends B { static { super.x = 5; } } return [A.x, B.x, Object.hasOwn(A, 'x')]", "class A { static { eval('A.v = 1'); } } return A.v",
  "class A { static { A.v = eval('this') === A; } } return A.v", "class A { static { eval('var q = 1'); A.q = typeof q; } } return A.q", "class A { static { A.v = eval('typeof arguments'); } } return A.v",
  "class A { static { A.v = [1,2].map(x => this.name + x); } } return A.v", "class A { static { var t = this; setTimeout; A.v = t === A; } } return A.v", "class A { static [(() => 'k')()] = 1; static { A.n = Object.keys(A).join(); } } return A.n",
  "const l = []; class A { static [(l.push('k1'), 'a')] = l.push('v1'); static [(l.push('k2'), 'b')] = l.push('v2'); static { l.push('blk'); } } return l",
  "const l = []; class A { [(l.push('k1'), 'a')] = l.push('v1'); [(l.push('k2'), 'b')] = l.push('v2'); } l.push('defined'); new A; return l",
  "const l = []; class A { static m() {} [(l.push('k1'), 'a')]() {} static [(l.push('k2'), 'b')]() {} } return l",
  "const l = []; class A extends (l.push('ext'), Object) { [(l.push('key'), 'a')]() {} } return l", "const l = []; try { class A extends (l.push('ext'), 1) { [(l.push('key'), 'a')]() {} } } catch (e) { l.push(e.name); } return l",
  "const l = []; class A { [(l.push('k'), 'a')] = 1; static { l.push('blk'); } } return l", "let n = 0; class A { static [n++] = n; static [n++] = n; } return [A[0], A[1], n]",
  "let x = 'outer'; class A { static [x] = 1; static { x = 'inner'; } static [x] = 2; } return Object.keys(A).join()", "class A { static [Symbol.for('s')] = 1; } return A[Symbol.for('s')]",
  "class A { static [1+1] = 'two'; static [`t${1}`] = 3; } return Object.keys(A).join()", "class A { static [{toString() { return 'ts'; }}] = 1; } return Object.keys(A).join()",
  "class A { static [{toString() { throw new Error('keyerr'); }}] = 1; }", "class A { [{toString() { throw new Error('keyerr'); }}]() {} }", "class A { static [{valueOf() { return 'vo'; }, toString() { return 'ts'; }}] = 1; } return Object.keys(A).join()",
  "let k = 0; class A { [++k] = k; [++k] = k; } const a = new A; return [a[1], a[2], k]",
];
add(...blockCases);

// ---- 4. Ordem de inicialização com herança.
const initParts = [
  ["base field", "class B { bf = L('bf'); constructor() { L('bctor'); } }"],
  ["base no ctor", "class B { bf = L('bf'); }"],
  ["base ctor returns obj", "class B { bf = L('bf'); constructor() { L('bctor'); return { r: 1 }; } }"],
  ["base ctor returns this", "class B { bf = L('bf'); constructor() { return this; } }"],
  ["base private", "class B { #b = L('b#'); bf = L('bf'); }"],
];
const derivedParts = [
  ["derived field", "class D extends B { df = L('df'); constructor() { L('pre'); super(); L('post'); } }"],
  ["derived no ctor", "class D extends B { df = L('df'); }"],
  ["derived private", "class D extends B { #d = L('d#'); df = L('df'); constructor() { super(); L('post'); } }"],
  ["derived arrow super", "class D extends B { df = L('df'); constructor() { const f = () => super(); L('pre'); f(); L('post'); } }"],
  ["derived eval super", "class D extends B { df = L('df'); constructor() { eval('super()'); L('post'); } }"],
  ["derived super args", "class D extends B { df = L('df'); constructor() { super(L('arg')); L('post'); } }"],
  ["derived this before super", "class D extends B { df = L('df'); constructor() { try { this.x; } catch (e) { L(e.name + ': ' + e.message); } super(); } }"],
  ["derived super twice", "class D extends B { df = L('df'); constructor() { super(); try { super(); } catch (e) { L(e.name + ': ' + e.message); } } }"],
  ["derived no super", "class D extends B { df = L('df'); constructor() { L('pre'); } }"],
  ["derived return obj", "class D extends B { df = L('df'); constructor() { return { d: 1 }; } }"],
  ["derived return undefined", "class D extends B { df = L('df'); constructor() { super(); return undefined; } }"],
  ["derived return prim", "class D extends B { df = L('df'); constructor() { super(); return 1; } }"],
  ["derived return prim no super", "class D extends B { df = L('df'); constructor() { return 1; } }"],
  ["derived return null", "class D extends B { constructor() { super(); return null; } }"],
  ["derived return this before super", "class D extends B { constructor() { return this; } }"],
  ["derived throw in field", "class D extends B { df = (() => { throw new Error('fx'); })(); }"],
  ["derived field this", "class D extends B { df = L(this instanceof D); }"],
  ["derived field new.target", "class D extends B { df = L(new.target === D); }"],
  ["derived field super prop", "class D extends B { df = L(super.toString === Object.prototype.toString); }"],
];
for (const [bn, b] of initParts) {
  for (const [dn, d] of derivedParts) {
    if (pick(3)) add(`const l = []; const L = v => (l.push(v), v);\n${b}\n${d}\nlet e = 'ok'; let o; try { o = new D(); } catch (x) { e = x.name + ': ' + x.message; }\nreturn [e, l.join('|'), o === undefined ? 'u' : Reflect.ownKeys(o).map(String).join()]`);
  }
}
add(
  "const l = []; class A { constructor() { l.push(new.target.name); } } class B extends A {} class C extends B {} new A; new B; new C; return l",
  "const l = []; class A { constructor() { l.push(new.target === A); } } class B extends A { constructor() { super(); l.push(new.target === B); } } new B; new A; return l",
  "class A { constructor() { this.t = new.target; } } return new A().t === A", "function F() { return new.target; } return [F(), typeof new F, new F instanceof F]",
  "function F() { return new.target; } return Reflect.construct(F, [], Object) === Object", "class A { constructor() { this.t = new.target; } } class B {} return Reflect.construct(A, [], B).t === B",
  "class A { constructor() { this.t = new.target; } } class B {} const o = Reflect.construct(A, [], B); return [o instanceof B, o instanceof A, Object.getPrototypeOf(o) === B.prototype]",
  "class A {} return Reflect.construct(A, [], () => {})", "class A {} return Reflect.construct(A, [], Math.max)", "class A {} function F() {} F.prototype = null; return Object.getPrototypeOf(Reflect.construct(A, [], F)) === Object.prototype",
  "class A {} function F() {} F.prototype = 1; return Object.getPrototypeOf(Reflect.construct(A, [], F)) === Object.prototype", "class A {} const P = new Proxy(function(){}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k]; } }); return Array.isArray(Reflect.construct(A, [], P))",
  "class A { constructor() { this.t = new.target; } } const f = A.bind(null); return new f().t === A", "class A { constructor() { this.t = new.target; } } const f = A.bind(null); class B {} return Reflect.construct(f, [], B).t === B",
  "class A { constructor() { this.t = new.target; } } const f = A.bind(null); return Reflect.construct(f, [], f).t === A", "class A { constructor() { this.t = (() => new.target)(); } } return new A().t === A",
  "class A { constructor() { this.t = eval('new.target'); } } return new A().t === A", "class A { constructor() { this.t = new Function('return typeof new.target')(); } } return new A().t", "class A { m() { return new.target; } } return new A().m()",
  "class A { static m() { return new.target; } } return A.m()", "class A { get g() { return new.target; } } return new A().g", "class A { x = new.target; } return new A().x", "class A { static x = new.target; } return A.x",
  "class A { constructor() { return new.target; } } return typeof new A", "class A { constructor() { const f = () => () => new.target; this.t = f()(); } } return new A().t === A",
  "class A { constructor() { this.t = new.target; } } class B extends A { constructor() { return Reflect.construct(A, [], B); } } return new B().t === B", "class A { constructor() { this.t = new.target; } } class B extends A { constructor() { super(); } } return new B().t === B",
  "class A { constructor() { this.t = new.target; } } class B extends A { constructor() { const o = Reflect.construct(A, [], Array); return o; } } return Array.isArray(new B)",
);

// ---- 5. Chamada sem new, construtores e herança estranha.
const callCases = [
  "class A {} return A()", "class A {} return A.call({})", "class A {} return A.apply(null, [])", "class A {} return Reflect.apply(A, null, [])", "class A {} return (0, A)()", "class A { constructor() {} } return A()",
  "class A extends Object {} return A()", "class A extends null {} return A()", "class A extends Array {} return A()", "class A extends Error {} return A()", "class A extends Function {} return A()",
  "const A = class {}; return A()", "const A = class N {}; return A()", "return (class {})()", "return (class Named {})()", "class A { static m() {} } return A.m.call()", "class A { m() {} } return new A().m.call()",
  "class A { m() {} } return new (new A().m)", "class A { static m() {} } return new A.m", "class A { get g() { return 1; } } return new (Object.getOwnPropertyDescriptor(A.prototype, 'g').get)", "class A { *g() {} } return new (new A().g)",
  "class A { async m() {} } return new (new A().m)", "class A { constructor() {} } return new (A.prototype.constructor)", "class A { constructor() {} } return Reflect.construct(A.prototype.constructor, [])",
  "class A {} return Reflect.construct(A, [], A) instanceof A", "class A {} return new A(...[1,2,3]) instanceof A", "class A { constructor(...a) { this.n = a.length; } } return new A(...[1,2,3]).n", "class A { constructor(a, b = 2, ...c) {} } return A.length",
  "class A { constructor(a, b) {} } return [A.length, A.name]", "class A {} return [A.length, A.name, typeof A]", "class A { static name = 'custom'; } return [A.name, Object.getOwnPropertyNames(A).join()]",
  "class A { static name() { return 1; } } return [typeof A.name, Object.getOwnPropertyNames(A).join()]", "class A { static get name() { return 'g'; } } return A.name", "class A { static length = 5; } return [A.length, Object.getOwnPropertyNames(A).join()]",
  "class A { static ['name'] = 1; } return A.name", "class A { static #name = 1; } return A.name", "class A { name = 1; } return [A.name, new A().name]", "const o = { A: class {} , B: class { static name = 1; } }; return [o.A.name, o.B.name]",
  "let a; a = class {}; return a.name", "let a = class {}; return a.name", "var [a = class {}] = []; return a.name", "var { a = class {} } = {}; return a.name", "let a; [a = class {}] = []; return a.name", "let a; ({ a = class {} } = {}); return a.name",
  "let a; a ||= class {}; return a.name", "let a = null; a ??= class {}; return a.name", "let a = 1; a &&= class {}; return a.name", "const o = {}; o.a = class {}; return o.a.name === ''", "const o = { ['k' + 1]: class {} }; return o.k1.name",
  "const s = Symbol('d'); const o = { [s]: class {} }; return o[s].name", "const s = Symbol(); const o = { [s]: class {} }; return JSON.stringify(o[s].name)", "const o = { a: (class {}) }; return o.a.name", "const o = { a: (0, class {}) }; return JSON.stringify(o.a.name)",
  "function f(a = class {}) { return a.name; } return f()", "return (class {}).name === ''", "return Object.getOwnPropertyNames(class {}).join()", "return Object.getOwnPropertyNames(class { static x; }).join()",
  "return Object.getOwnPropertyNames(class { static m() {} }).join()", "return Object.getOwnPropertyNames(class { m() {} }.prototype).join()", "return Object.getOwnPropertyNames(class { #p; }).join()",
  "class A {} return JSON.stringify(Object.getOwnPropertyDescriptor(A, 'prototype'))", "class A {} return JSON.stringify([Object.getOwnPropertyDescriptor(A, 'name'), Object.getOwnPropertyDescriptor(A, 'length')])",
  "class A { m() {} static s() {} get g() { return 1; } } return JSON.stringify([Object.getOwnPropertyDescriptor(A.prototype, 'm'), Object.getOwnPropertyDescriptor(A, 's')].map(d => [d.writable, d.enumerable, d.configurable]))",
  "class A { get g() { return 1; } set g(v) {} } const d = Object.getOwnPropertyDescriptor(A.prototype, 'g'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.set.name]",
  "class A { x = 1; } return JSON.stringify(Object.getOwnPropertyDescriptor(new A, 'x'))", "class A { static x = 1; } return JSON.stringify(Object.getOwnPropertyDescriptor(A, 'x'))",
  "class A { m() {} } const f = A.prototype.m; return [f.hasOwnProperty('prototype'), f.name, f.length]", "class A { static async *m(a) {} } return [A.m.name, A.m.length, Object.getPrototypeOf(A.m).constructor.name]",
  "class A { m() { return typeof this; } } return [A.prototype.m.call(1), A.prototype.m.call(undefined), A.prototype.m.call(null)]", "class A { m() { return this; } } const m = new A().m; return m()",
  "class A { m() { return this; } } const { m } = new A(); return m()", "class A { static m() { return this; } } const { m } = A; return m()", "class A { m() { return this === undefined; } } return (0, new A().m)()",
  "class A { constructor() { this.f = () => this; this.g = function() { return this; }; } } const a = new A; const { f, g } = a; return [f() === a, g()]",
  "class A { f = () => this; } const a = new A; const { f } = a; return f() === a", "class A { f = function() { return this; }; } const a = new A; const { f } = a; return f()",
  "class A { static f = () => this; } const { f } = A; return f() === A", "class A { static f = function() { return this; }; } const { f } = A; return f()",
  "class A { toString() { return 'ts'; } } return `${new A}` + new A + String(new A)", "class A { valueOf() { return 5; } } return new A + 1", "class A { [Symbol.toPrimitive](h) { return h; } } return [`${new A}`, new A + '', +new A]",
  "class A { get [Symbol.toStringTag]() { return 'TT'; } } return String(new A)", "class A { static get [Symbol.toStringTag]() { return 'TT'; } } return String(A)", "class A {} return [String(new A), String(A.prototype), Object.prototype.toString.call(A)]",
  "class A { static [Symbol.hasInstance](v) { return v === 1; } } return [1 instanceof A, new A instanceof A]", "class A { static [Symbol.hasInstance]() { return 1; } } return {} instanceof A",
  "class A { static [Symbol.hasInstance] = 1; } return {} instanceof A", "class A { static get [Symbol.hasInstance]() { throw new Error('hi'); } } return {} instanceof A",
  "class A {} A.prototype = {}; return Object.getOwnPropertyDescriptor(A, 'prototype').writable", "'use strict'; class A {} A.prototype = {}", "class A {} return delete A.prototype", "class A {} return Reflect.deleteProperty(A, 'prototype')",
  "class A {} A.name = 'z'; return A.name", "class A {} return Reflect.defineProperty(A, 'name', { value: 'z' }) && A.name", "class A {} A.x = 1; return Object.keys(A).join()",
  "class A { } return Object.isFrozen(A.prototype) + ',' + Object.isExtensible(A)", "class A { constructor() { Object.freeze(this); } x = 1; } return Object.isFrozen(new A)",
  "class A { x = 1; } class B extends A { constructor() { super(); Object.freeze(this); } y = 2; } return new B",
  "class A { constructor() { Object.preventExtensions(this); } } class B extends A { x = 1; } return new B",
  "class A { constructor() { Object.preventExtensions(this); } } class B extends A { #x = 1; static has(o) { return #x in o; } } return B.has(new B)",
  "class A { constructor() { return Object.freeze({}); } } class B extends A { #x = 1; static has(o) { return #x in o; } } return B.has(new B)",
  "class A { constructor() { return Object.freeze({}); } } class B extends A { x = 1; } return new B",
  "class A { constructor() { return new Proxy({}, { defineProperty() { return false; } }); } } class B extends A { x = 1; } return new B",
  "class A { constructor() { return new Proxy({}, { defineProperty(t, k, d) { console; return Reflect.defineProperty(t, k, d); } }); } } class B extends A { x = 1; } return Object.keys(new B)",
  "const l = []; class A { constructor() { return new Proxy({}, { defineProperty(t, k, d) { l.push(k + ':' + d.writable + d.enumerable + d.configurable); return Reflect.defineProperty(t, k, d); } }); } } class B extends A { x = 1; y; } new B; return l",
  "class A { constructor() { return new Proxy({}, { has(t, k) { return true; } }); } } class B extends A { #x = 1; static has(o) { return #x in o; } } return B.has(new B)",
  "const P = new Proxy({}, {}); class A { constructor() { return P; } } class B extends A { #x = 1; static g(o) { return o.#x; } } const b = new B; return [b === P, B.g(P)]",
  "const P = new Proxy({}, {}); class A { constructor() { return P; } } class B extends A { #x = 1; } class C extends A { #x = 1; } new B; try { new B; } catch (e) { return e.name + ': ' + e.message; } return 'no'",
  "const o = {}; class A { constructor() { return o; } } class B extends A { #x = 1; } new B; try { new B; } catch (e) { return e.name + ': ' + e.message; } return 'no'",
  "const o = {}; class A { constructor() { return o; } } class B extends A { #m() {} } new B; try { new B; } catch (e) { return e.name + ': ' + e.message; } return 'no'",
  "const o = {}; class A { constructor() { return o; } } class B extends A { get #g() { return 1; } } new B; try { new B; } catch (e) { return e.name + ': ' + e.message; } return 'no'",
  "const o = {}; class A { constructor() { return o; } } class B extends A { x = 1; } new B; new B; return o.x",
  "const o = {}; class A { constructor() { return o; } } class B extends A { static #s = 1; #x = 1; static t(v) { return #x in v; } } new B; return B.t(o)",
  "class A { #x = 1; static t(o) { return #x in o; } } class B extends A { constructor() { return new A; } } return A.t(new B)",
  "class A { #x = 1; static t(o) { return o.#x; } constructor(o) { return o; } } class B extends A { #y = 2; static u(o) { return o.#y; } } const o = {}; new B(o); return B.u(o)",
  "class A { constructor(o) { return o; } } class B extends A { #y = 2; static u(o) { return o.#y; } } const o = {}; new B(o); try { new B(o); } catch (e) { return e.name; }",
  "class Stamp { constructor(o) { return o; } } class Mark extends Stamp { #m = 1; static has(o) { return #m in o; } } const f = Object.freeze({}); new Mark(f); return Mark.has(f)",
];
add(...callCases);

// ---- 6. extends: null, função, proxy, não construtor, getters de prototype.
const heritages = [
  "null", "undefined", "1", "'s'", "{}", "[]", "Symbol", "Object", "Array", "Function", "Error", "Map", "Promise", "RegExp", "Date", "Boolean", "Number", "String", "Uint8Array", "ArrayBuffer", "WeakMap",
  "function () {}", "function F() {}", "() => {}", "async function () {}", "function* () {}", "async function* () {}", "class {}", "(class { constructor() { return {}; } })", "Math.max", "parseInt", "Symbol.prototype.toString",
  "Object.prototype.hasOwnProperty", "(function () {}).bind()", "class {}.bind?.()", "new Proxy(function () {}, {})", "new Proxy(class {}, {})", "new Proxy({}, {})", "new Proxy(() => {}, {})", "new Proxy(Object, {})",
  "new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? null : t[k]; } })", "new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? 1 : t[k]; } })",
  "new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? undefined : t[k]; } })", "new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? {} : t[k]; } })",
  "new Proxy(function () {}, { get(t, k) { throw new Error('trap ' + String(k)); } })", "new Proxy(function () {}, { construct(t, a, nt) { return { viaTrap: nt === D }; } })",
  "new Proxy(function () {}, { construct() { return 1; } })", "new Proxy(function () {}, { getPrototypeOf() { return null; } })", "Object.setPrototypeOf(function () {}, null)", "Object.assign(function () {}, { prototype: null })",
  "(() => { function F() {} F.prototype = null; return F; })()", "(() => { function F() {} F.prototype = 1; return F; })()", "(() => { function F() {} F.prototype = F; return F; })()", "(() => { function F() {} F.prototype = Object.create(null); return F; })()",
  "(() => { function F() {} Object.defineProperty(F, 'prototype', { get() { return Array.prototype; } }); return F; })()", "(() => { function F() {} delete F.prototype; return F; })()",
  "(() => { const f = function () {}; Object.setPrototypeOf(f, Array); return f; })()", "(() => { const f = function () {}; Object.setPrototypeOf(f, null); return f; })()",
  "Object.create(Function.prototype)", "{ prototype: {} }", "(() => { const o = function () {}; o.prototype = Object.create(Array.prototype); return o; })()", "Reflect.construct", "Reflect", "globalThis", "eval", "Function.prototype",
  "Function.prototype.call", "Object.getPrototypeOf(async function () {}).constructor", "Object.getPrototypeOf(function* () {}).constructor", "(class { static m() {} })", "(class A { static #p = 1; static g() { return A.#p; } })",
];
for (const h of heritages) {
  if (!pick(2)) continue;
  const base = `class D extends (${h}) {}`;
  add(`${base}\nreturn [typeof D, Object.getPrototypeOf(D) === Function.prototype, Object.getPrototypeOf(D.prototype) === null, typeof D.prototype]`);
  add(`${base}\nreturn typeof new D`, `${base}\nreturn Object.getPrototypeOf(new D) === D.prototype`);
  add(`class D extends (${h}) { constructor() { super(); this.z = 1; } }\nconst d = new D; return [d.z, d instanceof D]`);
  add(`class D extends (${h}) { constructor() { } }\nreturn typeof new D`, `class D extends (${h}) { constructor() { return {}; } }\nreturn typeof new D`);
  add(`class D extends (${h}) { m() { return super.m; } static s() { return super.s; } }\nreturn [typeof D.prototype.m, typeof D.s]`);
  add(`class D extends (${h}) { static x = super.name; }\nreturn D.x`);
}
add(
  "class D extends null { constructor() { return Object.create(D.prototype); } } const d = new D; return [d instanceof D, Object.getPrototypeOf(D.prototype), Object.getPrototypeOf(D) === Function.prototype]",
  "class D extends null { constructor() { super(); } } return new D", "class D extends null { constructor() { return {}; } } return typeof new D", "class D extends null { constructor() { return 1; } } return new D",
  "class D extends null { } return new D", "class D extends null { constructor() { } } return new D", "class D extends null { m() { return super.toString; } } return D.prototype.m()", "class D extends null { static m() { return super.toString; } } return D.m() === Function.prototype.toString",
  "class D extends null { static m() { return super.x; } } return D.m()", "class D extends null { m() { return super.x; } } return D.prototype.m.call({})", "class D extends null { m() { super.x = 1; } } const o = {}; D.prototype.m.call(o); return o.x",
  "class D extends null { m() { super.x = 1; } } return D.prototype.m.call(1)", "class D extends null { m() { return super['y']; } } return D.prototype.m()",
  "class D extends null { static #p = 1; static g() { return D.#p; } } return D.g()", "class D extends null { #p = 1; static g(o) { return o.#p; } constructor() { return Object.create(D.prototype); } } return (() => { try { return D.g(new D); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class D extends null { x = 1; constructor() { return Object.create(D.prototype); } } return new D().x", "class D extends null { static { D.v = Object.getPrototypeOf(D.prototype); } } return D.v",
  "class D extends null { static x = Object.getPrototypeOf(this) === Function.prototype; } return D.x", "class D extends null { toString() { return 'x'; } } return D.prototype.toString()",
  "class D extends null {} return Object.getOwnPropertyNames(D.prototype).join()", "class D extends null {} return D.prototype.constructor === D", "class D extends null {} return D.prototype instanceof Object",
  "class D extends null {} return [D.name, D.length, String(D)]", "class D extends null {} return Reflect.construct(D, [], Object)", "class D extends null { constructor() { super(); } } return Reflect.construct(D, [], Object)",
  "class D extends null { constructor() { return Reflect.construct(Object, [], new.target); } } return Object.getPrototypeOf(new D) === D.prototype",
  "class D extends null { constructor() { return Reflect.construct(Object, [], new.target); } } class E extends D { x = 1; } return [new E().x, Object.getPrototypeOf(new E) === E.prototype]",
  "class D extends null { constructor() { return Reflect.construct(Object, [], new.target); } } class E extends D { constructor() { super(); this.y = 2; } } return new E().y",
  "class D extends null { constructor() { return Reflect.construct(Object, [], new.target); } } return Object.getPrototypeOf(D) === Function.prototype",
  "class D extends null { constructor() { return Reflect.construct(Object, [], new.target); } } class E extends D {} return Object.getPrototypeOf(E) === D",
  "function F() {} class D extends F { constructor() { super(); this.t = new.target; } } return new D().t === D", "function F() { this.f = new.target; } class D extends F {} return new D().f === D",
  "function F() { return { custom: 1 }; } class D extends F { x = 1; } const d = new D; return [d.custom, d.x, d instanceof D]", "function F() { return 1; } class D extends F {} return [new D instanceof D, new D instanceof F]",
  "function F() { return; } class D extends F {} return new D instanceof D", "function F() { throw new Error('ctorerr'); } class D extends F {} return new D",
  "function F() {} F.prototype.m = function() { return 'fm'; }; class D extends F { m() { return super.m() + 'd'; } } return new D().m()", "function F() {} F.staticM = function() { return 'sm'; }; class D extends F { static staticM() { return super.staticM() + 'd'; } } return D.staticM()",
  "function F() {} class D extends F {} F.late = 1; return D.late", "function F() {} class D extends F {} F.prototype.late = 1; return new D().late", "function F() {} class D extends F {} Object.setPrototypeOf(D, null); return typeof D.late",
  "function F() {} class D extends F {} Object.setPrototypeOf(D, Object); try { return new D instanceof D; } catch (e) { return e.name; }",
  "class B {} class D extends B { constructor() { super(); } } Object.setPrototypeOf(D, class { constructor() { this.swapped = 1; } }); return new D().swapped",
  "class B {} class D extends B { constructor() { super(); } } Object.setPrototypeOf(D, () => {}); try { return new D; } catch (e) { return e.name + ': ' + e.message; }",
  "class B {} class D extends B { constructor() { super(); } } Object.setPrototypeOf(D, null); try { return new D; } catch (e) { return e.name + ': ' + e.message; }",
  "class B {} class D extends B { constructor() { super(); } } Object.setPrototypeOf(D, {}); try { return new D; } catch (e) { return e.name + ': ' + e.message; }",
  "class B {} class D extends B { constructor() { super(); } } Object.setPrototypeOf(D, Math.max); try { return new D; } catch (e) { return e.name + ': ' + e.message; }",
  "class B {} class D extends B { m() { return super.x; } } Object.setPrototypeOf(D.prototype, { x: 'swapped' }); return new D().m()",
  "class B { x = 'b'; } const o = { x: 'o' }; class D extends B { m() { return super.x; } } Object.setPrototypeOf(D.prototype, o); return new D().m()",
  "class B { m() { return 'b'; } } class D extends B { m() { return super.m(); } } const m = D.prototype.m; return m.call({ __proto__: null })",
  "class B { get g() { return this.v; } } class D extends B { get g() { return super.g; } } return Reflect.get(D.prototype, 'g', { v: 42 })",
  "class B { set s(v) { this._s = v; } } class D extends B { set s(v) { super.s = v + 1; } } const d = new D; d.s = 1; return [d._s, Object.keys(d).join()]",
  "class B { set s(v) { this._s = v; } } class D extends B { m() { super.s = 5; return this._s; } } return new D().m()", "class B {} class D extends B { m() { super.x = 5; return [this.x, Object.hasOwn(this, 'x'), B.prototype.x]; } } return new D().m()",
  "class B { get x() { return 1; } } class D extends B { m() { super.x = 5; return this.x; } } return (() => { try { return new D().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class B {} class D extends B { m() { 'use strict'; super.toString = 1; } } return (() => { try { return new D().m(); } catch (e) { return e.name; } })()",
  "class B {} class D extends B { m() { delete super.x; } } return (() => { try { return new D().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class B {} class D extends B { m() { delete super['x']; } } return (() => { try { return new D().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class B {} class D extends B { m() { return super.x++; } } return (() => { try { return new D().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class B { x = 1; } class D extends B { m() { return super.x; } } return new D().m()", "class B { constructor() { this.x = 1; } } class D extends B { m() { return super.x; } } return new D().m()",
  "class B { m() { return this.tag; } } class D extends B { tag = 'dt'; m() { return super.m(); } } return new D().m()", "class B {} class D extends B { m() { return super.constructor === B; } } return new D().m()",
  "class B {} class D extends B { m() { return super.m?.(); } } return new D().m()", "class B { m() { return 1; } } class D extends B { m() { return super.m?.(); } } return new D().m()",
  "class B {} class D extends B { m() { return super[Symbol.iterator]; } } return new D().m()", "class B { *[Symbol.iterator]() { yield 1; } } class D extends B { m() { return [...super[Symbol.iterator].call(this)]; } } return new D().m()",
  "class B { m() { return 'b'; } } class D extends B { m = () => super.m(); } return new D().m()", "class B { m() { return 'b'; } } class D extends B { static m = () => super.m; } return typeof D.m()",
  "class B { static m() { return 'bs'; } } class D extends B { static m = () => super.m(); } return D.m()", "class B { m() { return 'b'; } } class D extends B { x = super.m(); } return new D().x",
  "class B { m() { return 'b'; } } class D extends B { x = () => super.m(); } return new D().x()", "class B { m() { return 'b'; } } class D extends B { ['k'] = super.m(); } return new D().k",
  "class B { m() { return 'b'; } } class D extends B { constructor() { super(); this.v = super.m(); } } return new D().v", "class B { m() { return 'b'; } } class D extends B { constructor() { const r = super.m; super(); this.v = r; } } return typeof new D().v",
  "class B { m() { return 'b'; } } class D extends B { constructor() { try { super.m(); } catch (e) { var er = e.name + ': ' + e.message; } super(); this.er = er; } } return new D().er",
  "class B {} class D extends B { constructor() { const f = () => this; try { f(); } catch (e) { var er = e.name + ': ' + e.message; } super(); this.er = er; } } return new D().er",
  "class B {} class D extends B { constructor() { try { eval('this'); } catch (e) { var er = e.name + ': ' + e.message; } super(); this.er = er; } } return new D().er",
  "class B {} class D extends B { constructor() { try { new.target; super(); } catch (e) { return e; } } } return typeof new D",
  "class B {} class D extends B { constructor() { return super(), 1; } } return new D instanceof D", "class B {} class D extends B { constructor() { super(), super(); } } return (() => { try { new D; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class B { constructor() { this.n = (this.n || 0) + 1; } } class D extends B { constructor() { super(); try { super(); } catch (e) {} } } return new D().n",
  "class B { constructor() { throw new Error('first'); } } class D extends B { constructor() { try { super(); } catch (e) {} try { super(); } catch (e) { return e.message; } } } return new D",
  "let count = 0; class B { constructor() { count++; } } class D extends B { constructor() { super(); super(); } } try { new D; } catch (e) {} return count",
  "class B {} class D extends B { constructor(...a) { super(...a); this.a = a; } } return new D(1, 2, 3).a", "class B { constructor(a) { this.a = a; } } class D extends B { constructor() { super(...arguments); } } return new D(7).a",
  "class B { constructor(a) { this.a = a; } } class D extends B { constructor() { super(arguments[0]); } } return new D(8).a", "class B { constructor() { this.n = arguments.length; } } class D extends B {} return new D(1, 2, 3).n",
  "class B { constructor() { this.n = arguments.length; } } class D extends B { constructor() { super(...[], ...[1]); } } return new D().n", "class B { constructor() { this.n = arguments.length; } } class D extends B { constructor() { super(...'abc'); } } return new D().n",
  "class B { constructor() { this.n = arguments.length; } } class D extends B { constructor() { super(...1); } } return new D().n", "class B { constructor() { this.n = arguments.length; } } class D extends B { constructor() { super(...null); } } return new D().n",
);

// ---- 7. Symbol.species.
const speciesBases = ["Array", "Map", "Set", "Promise", "RegExp", "ArrayBuffer", "Uint8Array", "Float64Array"];
for (const base of speciesBases) {
  add(`class S extends ${base} {}\nreturn [S[Symbol.species] === S, ${base}[Symbol.species] === ${base}, Object.getOwnPropertyDescriptor(${base}, Symbol.species).get.name, Object.getOwnPropertyDescriptor(${base}, Symbol.species).set]`);
  add(`class S extends ${base} { static get [Symbol.species]() { return ${base}; } }\nreturn [S[Symbol.species] === ${base}, S[Symbol.species] === S]`);
  add(`return Object.getOwnPropertyDescriptor(${base}, Symbol.species).get.call(1)`, `return Object.getOwnPropertyDescriptor(${base}, Symbol.species).get.call(undefined)`);
  add(`class S extends ${base} { static get [Symbol.species]() { return 1; } }\nreturn S[Symbol.species]`);
}
add(
  "class S extends Array {} const s = S.from([1,2,3]); return [s.map(x => x) instanceof S, s.filter(x => x) instanceof S, s.slice() instanceof S, s.splice(0, 1) instanceof S, s.concat([]) instanceof S, s.flat() instanceof S, s.flatMap(x => x) instanceof S]",
  "class S extends Array { static get [Symbol.species]() { return Array; } } const s = S.from([1,2,3]); return [s.map(x => x) instanceof S, s.filter(x => x).constructor === Array, s.slice().constructor === Array, s.splice(0, 1).constructor === Array, s.concat([]).constructor === Array, s.flat().constructor === Array]",
  "class S extends Array { static get [Symbol.species]() { return undefined; } } const s = S.from([1,2,3]); return [s.map(x => x).constructor === Array, s.slice().constructor === Array]",
  "class S extends Array { static get [Symbol.species]() { return null; } } const s = S.from([1,2,3]); return [s.map(x => x).constructor === Array, s.slice().constructor === Array]",
  "class S extends Array { static get [Symbol.species]() { return Object; } } const s = S.from([1,2,3]); return (() => { try { return Object.keys(s.map(x => x)).join(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Array { static get [Symbol.species]() { return 1; } } const s = S.from([1,2,3]); return (() => { try { return s.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Array { static get [Symbol.species]() { return () => {}; } } const s = S.from([1,2,3]); return (() => { try { return s.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Array { static get [Symbol.species]() { return function () { return { length: 0, custom: 1 }; }; } } const s = S.from([1,2,3]); return (() => { try { return s.map(x => x).custom; } catch (e) { return e.name + ': ' + e.message; } })()",
  "const l = []; class S extends Array { static get [Symbol.species]() { return function (n) { l.push('n=' + n); return []; }; } } const s = S.from([1,2,3]); s.map(x => x); s.filter(x => x); s.slice(1); s.splice(0, 1); s.concat([4]); s.flat(); s.flatMap(x => [x]); return l",
  "const l = []; class S extends Array { constructor(...a) { super(...a); l.push('c' + a.length); } } const s = new S(1, 2, 3); l.push('--'); s.map(x => x); s.filter(x => x); s.slice(); return l",
  "class S extends Array { constructor() { super(); } } const s = new S; s.push(1, 2); return [s.map(x => x * 2) instanceof S, s.length, s.map(x => x * 2).join()]",
  "class S extends Array { constructor(a) { super(); this.a = a; } } const s = new S('tag'); s.push(1); const m = s.map(x => x); return [m.a, m instanceof S, m.length]",
  "const a = [1,2,3]; a.constructor = { [Symbol.species]: function (n) { return { length: n, tagged: true }; } }; return a.map(x => x).tagged",
  "const a = [1,2,3]; a.constructor = undefined; return a.map(x => x) instanceof Array", "const a = [1,2,3]; a.constructor = 1; return (() => { try { return a.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "const a = [1,2,3]; a.constructor = null; return (() => { try { return a.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()", "const a = [1,2,3]; a.constructor = { [Symbol.species]: null }; return a.map(x => x) instanceof Array",
  "const a = [1,2,3]; a.constructor = { [Symbol.species]: undefined }; return a.map(x => x) instanceof Array", "const a = [1,2,3]; a.constructor = { [Symbol.species]: 1 }; return (() => { try { return a.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "const a = [1,2,3]; Object.defineProperty(a, 'constructor', { get() { throw new Error('ctorget'); } }); return (() => { try { return a.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "const a = [1,2,3]; a.constructor = Array; return a.map(x => x).length", "class S extends Array {} const s = new S; s.length = 3; return s.fill(0).map(x => x + 1) instanceof S",
  "class S extends Array {} const a = [1,2,3]; a.constructor = S; return a.map(x => x) instanceof S", "class S extends Array {} const a = Array.of(1, 2); a.constructor = S; return a.filter(x => 1).constructor === S",
  "class S extends Map {} return [new S().constructor === S, S[Symbol.species] === S, new S([[1, 2]]).get(1)]", "class S extends Set {} return [new S([1]).has(1), S[Symbol.species] === S, new S().union(new Set([1])) instanceof S]",
  "class S extends Set {} return new S([1]).intersection(new Set([1])) instanceof S", "class S extends Map {} return new S([[1, 2]]).constructor.name", "class S extends Promise {} const p = S.resolve(1); return [p instanceof S, p.then(() => {}) instanceof S, p.catch(() => {}) instanceof S, p.finally(() => {}) instanceof S]",
  "class S extends Promise { static get [Symbol.species]() { return Promise; } } const p = S.resolve(1); return [p instanceof S, p.then(() => {}) instanceof S, p.then(() => {}) instanceof Promise, p.finally(() => {}) instanceof S]",
  "class S extends Promise { static get [Symbol.species]() { return 1; } } const p = S.resolve(1); return (() => { try { return p.then(() => {}); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Promise { static get [Symbol.species]() { return undefined; } } const p = S.resolve(1); return p.then(() => {}) instanceof Promise",
  "class S extends Promise { constructor(ex) { super(ex); this.made = true; } } const p = S.resolve(1); return [p.made, p.then().made, S.all([]).made, S.race([]).made, S.reject(1).catch(() => {}).made]",
  "class S extends Promise { constructor(ex) { super(ex); } } return Promise.resolve.call(S, 1) instanceof S", "return (() => { try { return Promise.resolve.call(1, 1); } catch (e) { return e.name + ': ' + e.message; } })()",
  "return (() => { try { return Promise.resolve.call({}, 1); } catch (e) { return e.name + ': ' + e.message; } })()", "return (() => { try { return Promise.resolve.call(function () {}, 1); } catch (e) { return e.name + ': ' + e.message; } })()",
  "return (() => { try { return Promise.resolve.call(class {}, 1); } catch (e) { return e.name + ': ' + e.message; } })()", "class S extends RegExp {} const r = new S('a', 'g'); return [r instanceof S, r.flags, r.source, 'aaa'.replace(r, 'b'), S[Symbol.species] === S]",
  "class S extends RegExp {} const r = new S('a'); return [[...'aa'.matchAll(new S('a', 'g'))].length, 'abc'.split(new S('b')).join()]", "class S extends RegExp { static get [Symbol.species]() { return RegExp; } } return ['a-b'.split(new S('-')).join()]",
  "const l = []; class S extends RegExp { constructor(p, f) { super(p, f); l.push(String(f)); } } 'a-b-c'.split(new S('-')); 'x'.match(new S('x', 'g')); return l",
  "class S extends ArrayBuffer {} const b = new S(8); return [b instanceof S, b.slice(0, 4) instanceof S, b.slice(0, 4).byteLength, b.resize === undefined]",
  "class S extends ArrayBuffer { static get [Symbol.species]() { return ArrayBuffer; } } const b = new S(8); return [b.slice(0, 4) instanceof S, b.slice(0, 4) instanceof ArrayBuffer]",
  "class S extends ArrayBuffer { static get [Symbol.species]() { return function () { return {}; }; } } const b = new S(8); return (() => { try { return b.slice(0, 4); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends ArrayBuffer { static get [Symbol.species]() { return class extends ArrayBuffer { constructor() { super(2); } }; } } const b = new S(8); return (() => { try { return b.slice(0, 4).byteLength; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Uint8Array {} const t = new S([1, 2, 3]); return [t.map(x => x) instanceof S, t.filter(x => x) instanceof S, t.slice() instanceof S, t.subarray(1) instanceof S, t.toSorted() instanceof S, t.toReversed() instanceof S, t.with(0, 1) instanceof S]",
  "class S extends Uint8Array { static get [Symbol.species]() { return Uint16Array; } } const t = new S([1, 2, 3]); return [t.map(x => x) instanceof Uint16Array, t.slice().constructor.name, t.subarray(1).constructor.name]",
  "class S extends Uint8Array { static get [Symbol.species]() { return Array; } } const t = new S([1, 2, 3]); return (() => { try { return t.map(x => x); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Uint8Array { static get [Symbol.species]() { return function () { return new Uint8Array(1); }; } } const t = new S([1, 2, 3]); return (() => { try { return t.map(x => x).length; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class S extends Float64Array {} const t = new S(2); return [t.constructor.name, Object.getPrototypeOf(S) === Float64Array, S.BYTES_PER_ELEMENT, new S(1).BYTES_PER_ELEMENT]",
  "return Object.getOwnPropertyNames(Array).filter(k => typeof k === 'string').sort().join()", "return Reflect.ownKeys(Array).filter(k => typeof k === 'symbol').map(String).join()",
  "return Reflect.ownKeys(Map).map(String).sort().join()", "return Reflect.ownKeys(Promise).map(String).sort().join()",
);

// ---- 8. super em métodos, estáticos e objetos literais.
add(
  "const p = { m() { return 'p'; } }; const o = { __proto__: p, m() { return super.m() + 'o'; } }; return o.m()", "const p = { m() { return this.tag; } }; const o = { __proto__: p, tag: 'ot', m() { return super.m(); } }; return o.m()",
  "const p = { x: 1 }; const o = { __proto__: p, g() { return super.x; } }; const q = { x: 2, g: o.g }; return q.g()", "const o = { m() { return super.toString === Object.prototype.toString; } }; return o.m()",
  "const o = { m: function () { return super.x; } }", "const o = { m: () => super.x }", "const o = { get g() { return super.toString === Object.prototype.toString; } }; return o.g", "const o = { set s(v) { super.x = v; } }; o.s = 3; return o.x",
  "const o = { m() { super.x = 1; return Object.hasOwn(this, 'x'); } }; return o.m()", "const o = { m() { return super.m; } }; return o.m()", "const o = { m() { return () => super.toString === Object.prototype.toString; } }; return o.m()()",
  "const o = { m() { return { n() { return typeof super.m; } }.n(); } }; return o.m()", "const o = { *g() { yield super.toString === Object.prototype.toString; } }; return [...o.g()]",
  "const o = { async m() { return super.toString === Object.prototype.toString; } }; return typeof o.m().then", "const o = { async *m() { yield super.constructor === Object; } }; return typeof o.m().next",
  "const o = { ['c' + 1]() { return super.hasOwnProperty('c1'); } }; return o.c1()", "const p = {}; const o = { m() { return Object.getPrototypeOf(this) === p; } }; Object.setPrototypeOf(o, p); return o.m()",
  "const p1 = { n: 1 }, p2 = { n: 2 }; const o = { __proto__: p1, m() { return super.n; } }; Object.setPrototypeOf(o, p2); return o.m()", "const o = { m() { return super.x; } }; const m = o.m; return m.call({ x: 9 })",
  "const o = { m() { return super.x; } }; return o.m.call(null)", "const o = { m() { return super.x; } }; return o.m.call(1)", "class A { static m() { return 'am'; } } const o = { __proto__: A, m() { return super.m(); } }; return o.m()",
  "class A { static m() { return this === o; } } const o = { __proto__: A, m() { return super.m(); } }; return o.m()", "class A { m() { return 'am'; } } const o = Object.setPrototypeOf({ m() { return super.m(); } }, A.prototype); return o.m()",
  "class A { static m() { return 'a'; } } class B extends A { static m() { return super.m() + 'b'; } } class C extends B { static m() { return super.m() + 'c'; } } return C.m()",
  "class A { m() { return 'a'; } } class B extends A { m() { return super.m() + 'b'; } } class C extends B { m() { return super.m() + 'c'; } } return new C().m()",
  "class A { m() { return this.n; } } class B extends A { constructor() { super(); this.n = 'bn'; } m() { return super.m(); } } return new B().m()",
  "class A { static m() { return this.name; } } class B extends A { static m() { return super.m(); } } return B.m()", "class A { static m() { return this.name; } } class B extends A { static m = () => super.m(); } return B.m()",
  "class A { get g() { return this.v; } set g(x) { this.w = x; } } class B extends A { v = 'bv'; get g() { return super.g; } set g(x) { super.g = x; } } const b = new B; b.g = 5; return [b.g, b.w, Object.hasOwn(b, 'g')]",
  "class A { static get g() { return this.v; } } class B extends A { static v = 'bv'; static get g() { return super.g; } } return B.g", "class A {} class B extends A { m() { return super.m; } } return new B().m()",
  "class A { m() { return 'a'; } } class B extends A { m() { return super.m; } } return new B().m().call(null)", "class A { m() { return this; } } class B extends A { m() { return super.m(); } } const b = new B; return b.m() === b",
  "class A { m() { return this; } } class B extends A { m() { const f = super.m; return f(); } } return new B().m()", "class A { m() { return this; } } class B extends A { m() { return (0, super.m)(); } } return new B().m()",
  "class A { m() { return this; } } class B extends A { m() { return super.m`x`; } } const b = new B; return b.m() === b", "class A { m() { return arguments.length; } } class B extends A { m() { return super.m(...arguments); } } return new B().m(1, 2, 3)",
  "class A { m() { return 'a'; } } class B extends A { ['m']() { return super['m'](); } } return new B().m()", "class A { m() { return 'a'; } } class B extends A { m() { const k = 'm'; return super[k](); } } return new B().m()",
  "const l = []; class A { m() { return 'a'; } } class B extends A { m() { return super[(l.push('key'), 'm')](l.push('arg')); } } new B().m(); return l",
  "class A { m() { return 'a'; } } class B extends A { m() { return super[{ toString() { return 'm'; } }](); } } return new B().m()", "class A {} class B extends A { m() { return super[{ toString() { throw new Error('ks'); } }]; } } return (() => { try { return new B().m(); } catch (e) { return e.message; } })()",
  "class A {} class B extends A { m() { return super.x = 1, this.x; } } return new B().m()", "class A {} class B extends A { m() { return (super.x = 1, super.x); } } return new B().m()",
  "class A {} class B extends A { m() { super.x ||= 5; return this.x; } } return new B().m()", "class A { get x() { return 1; } } class B extends A { m() { super.x ??= 5; return this.x; } } return new B().m()",
  "class A {} class B extends A { m() { super.x ??= 5; return [this.x, Object.hasOwn(this, 'x')]; } } return new B().m()", "class A {} class B extends A { m() { super.x += 1; return this.x; } } return new B().m()",
  "class A { x = 1; } class B extends A { m() { super.x += 1; return this.x; } } return new B().m()", "class A {} class B extends A { m() { for (super.x of [1, 2]); return this.x; } } return new B().m()",
  "class A {} class B extends A { m() { [super.x, super.y] = [1, 2]; return [this.x, this.y]; } } return new B().m()", "class A {} class B extends A { m() { ({ a: super.x } = { a: 3 }); return this.x; } } return new B().m()",
  "class A {} class B extends A { m() { return typeof super.x; } } return new B().m()", "class A {} class B extends A { m() { return super.x?.y; } } return new B().m()", "class A {} class B extends A { m() { return super.x ?? 'dflt'; } } return new B().m()",
  "class A { static #p = 1; static g() { return this.#p; } } class B extends A {} return (() => { try { return B.g(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { static #p = 1; static g() { return A.#p; } } class B extends A {} return B.g()", "class A { static #m() { return 'sm'; } static g() { return this.#m(); } } class B extends A {} return (() => { try { return B.g(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { static #p = 1; static g() { return this.#p; } } return A.g.call({})", "class A { static #p = 1; static s() { this.#p = 2; } } class B extends A {} return (() => { try { B.s(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { static #p = 1; static s() { this.#p = 2; return A.#p; } } return A.s()", "class A { static #p = 1; static g() { return this.#p; } static { A.v = this.g(); } } return A.v",
  "class A { static #p = 1; static x = A.#p; } return A.x", "class A { static x = A.#p; static #p = 1; }", "class A { static x = this.#p; static #p = 1; }", "class A { static x = () => A.#p; static #p = 1; } return A.x()",
  "class A { static x = A.#m(); static #m() { return 'hoisted'; } } return A.x", "class A { static x = A.#g; static get #g() { return 'gg'; } } return A.x", "class A { x = this.#m(); #m() { return 'im'; } } return new A().x",
  "class A { x = this.#p; #p = 1; } return new A().x", "class A { #p = 1; x = this.#p; } return new A().x", "class A { x = this.#g; get #g() { return 'ig'; } } return new A().x", "class A { x = this.#s; set #s(v) {} } return new A().x",
  "class A { #p = this.#q; #q = 1; } return new A", "class A { #q = 1; #p = this.#q; get p() { return this.#p; } } return new A().p", "class A { #p = (() => this.#q)(); #q = 1; } return new A",
  "class A { #p = 1; constructor() { this.#p = 2; } get p() { return this.#p; } } return new A().p", "class A { constructor() { this.#p = 2; } #p = 1; get p() { return this.#p; } } return new A().p",
  "class A { #p; constructor() { this.#p++; } get p() { return this.#p; } } return Number.isNaN(new A().p)", "class A { #p = 1; inc() { return this.#p++ + ++this.#p + (this.#p += 2) + (this.#p **= 2); } } return new A().inc()",
  "class A { #p = null; m() { this.#p ??= 'n'; this.#p ||= 'o'; this.#p &&= 'a'; return this.#p; } } return new A().m()", "class A { #p = 0; m() { this.#p ||= 5; return this.#p; } } return new A().m()",
  "class A { get #g() { return 1; } m() { this.#g ||= 2; return 'ok'; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { get #g() { return 0; } set #g(v) { this.set = v; } m() { this.#g ||= 2; return this.set; } } return new A().m()", "class A { get #g() { return 1; } set #g(v) { this.set = v; } m() { this.#g &&= 3; return this.set; } } return new A().m()",
  "class A { get #g() { return 1; } set #g(v) { this.set = v; } m() { this.#g ??= 3; return this.set; } } return new A().m()", "class A { get #g() { return 1; } set #g(v) { this.set = v; } m() { this.#g += 3; return this.set; } } return new A().m()",
  "class A { get #g() { return 1; } set #g(v) { this.set = v; } m() { return [this.#g++, this.set]; } } return new A().m()", "class A { set #s(v) { this.set = v; } m() { return [this.#s = 5, this.set]; } } return new A().m()",
  "class A { set #s(v) { this.set = v; } m() { return this.#s += 1; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() {} m() { this.#m = 1; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #m() {} m() { this.#m++; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() {} m() { this.#m ||= 1; return 'ok'; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #m() {} m() { this.#m &&= 1; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() {} m() { [this.#m] = [1]; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #m() {} m() { ({ a: this.#m } = { a: 1 }); } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() {} m() { for (this.#m of [1]); } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { get #g() { return 1; } m() { this.#g = 1; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { get #g() { return 1; } m() { [this.#g] = [1]; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { set #s(v) {} m() { return this.#s; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { set #s(v) {} m() { return this.#s++; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { set #s(v) {} m() { return typeof this.#s; } } return (() => { try { return new A().m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() {} m() { return this.#m === this.#m; } } return new A().m()", "class A { #m() {} static m(a, b) { return a.#m === b.#m; } } return A.m(new A, new A)", "class A { #m() { return this; } m() { const f = this.#m; return f(); } } return new A().m()",
  "class A { #m() { return this; } m() { return this.#m`x` === this; } } return new A().m()", "class A { #m() { return 'x'; } m() { return this?.#m(); } } return new A().m()", "class A { #m() { return 'x'; } m() { return this.#m?.(); } } return new A().m()",
  "class A { #m() { return 'x'; } m(o) { return o?.#m(); } } return [new A().m(null), new A().m(undefined), new A().m(new A)]", "class A { #m() { return 'x'; } m(o) { return o?.#m(); } } return (() => { try { return new A().m({}); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; m(o) { return o?.a.#p; } } return new A().m(null)", "class A { #p = 1; m(o) { return (o?.a).#p; } } return (() => { try { return new A().m(null); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; m(o) { return o.#p; } } return (() => { try { return new A().m(null); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #p = 1; m(o) { return o.#p; } } return (() => { try { return new A().m(undefined); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; static m(o) { o.#p = 2; } } return (() => { try { A.m(null); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #p = 1; static m(o) { o.#p = 2; } } return (() => { try { A.m({}); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; static m(o) { o.#p++; } } return (() => { try { A.m({}); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #p = 1; static m(o) { return o.#p; } } return (() => { try { A.m(Object.create(new A)); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; static m(o) { return o.#p; } } class B { #p = 2; static m(o) { return o.#p; } } return [A.m(new A), B.m(new B), (() => { try { return A.m(new B); } catch (e) { return e.name + ': ' + e.message; } })()]",
  "class A { #p = 1; static m(o) { return o.#p; } } function mk() { return class { #p = 2; static m(o) { return o.#p; } }; } const C1 = mk(), C2 = mk(); return [C1.m(new C1), (() => { try { return C1.m(new C2); } catch (e) { return e.name + ': ' + e.message; } })()]",
  "function mk() { return class { #p = 2; static has(o) { return #p in o; } }; } const C1 = mk(), C2 = mk(); return [C1.has(new C1), C1.has(new C2), C2.has(new C2)]",
  "class A { #p = 1; static has(o) { return #p in o; } } return [A.has(new A), A.has(Object.create(new A)), A.has(new Proxy(new A, {})), A.has(A)]",
  "class A { #p = 1; static has(o) { return #p in o; } } const l = []; try { A.has(new Proxy({}, { has() { l.push('trap'); return true; } })); } catch (e) {} return l",
  "class A { #p = 1; static has(o) { return #p in o; } } return [(() => { try { return A.has(); } catch (e) { return e.name + ': ' + e.message; } })(), (() => { try { return A.has(1n); } catch (e) { return e.name + ': ' + e.message; } })(), (() => { try { return A.has('str'); } catch (e) { return e.name + ': ' + e.message; } })()]",
  "class A { #p = 1; static has(o) { return #p in o; } } const o = new A; return [A.has(o), (() => { Object.freeze(o); return A.has(o); })(), (() => { Object.setPrototypeOf(o, null); return A.has(o); })()]",
  "class A { #p = 1; get p() { return this.#p; } } const o = new A; Object.freeze(o); Object.setPrototypeOf(o, null); return o.p === undefined", "class A { #p = 1; get p() { return this.#p; } } const a = new A; return Object.getOwnPropertyDescriptor(A.prototype, 'p').get.call(a)",
  "class A { #p = 1; get p() { return this.#p; } } return (() => { try { return Object.getOwnPropertyDescriptor(A.prototype, 'p').get.call({}); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; get p() { return this.#p; } } const a = new A; return JSON.stringify(a) + Object.keys(a).length + Reflect.ownKeys(a).length", "class A { #p = 1; } return Object.getOwnPropertyNames(new A).length + Object.getOwnPropertySymbols(new A).length",
  "class A { #p = 1; } const a = new A; return structuredClone ? typeof structuredClone(a) : 'n/a'", "class A { #p = 1; static m(o) { return o.#p; } } const a = new A; const c = Object.assign({}, a); return (() => { try { return A.m(c); } catch (e) { return e.name; } })()",
  "class A { #p = 1; static m(o) { return o.#p; } } const a = new A; const c = { ...a }; return (() => { try { return A.m(c); } catch (e) { return e.name; } })()",
  "class A { #p = 1; static m(o) { return o.#p; } } const a = new A; const c = Object.create(Object.getPrototypeOf(a)); return (() => { try { return A.m(c); } catch (e) { return e.name; } })()",
  "class A { #p = 1; static m(o) { return o.#p; } } const a = new A; const px = new Proxy(a, {}); return (() => { try { return A.m(px); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; get p() { return this.#p; } } const px = new Proxy(new A, {}); return (() => { try { return px.p; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; get p() { return this.#p; } } const px = new Proxy(new A, { get(t, k, r) { return Reflect.get(t, k, t); } }); return px.p",
  "class A { #m() { return 1; } m() { return this.#m(); } } const px = new Proxy(new A, {}); return (() => { try { return px.m(); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #m() { return 1; } m() { return this.#m(); } } const px = new Proxy(new A, { get(t, k) { const v = t[k]; return typeof v === 'function' ? v.bind(t) : v; } }); return px.m()",
);

// ---- 9. toString de classe.
const toStringCases = [
  "class A {}", "class A { }", "class  A  {  }", "class A { x = 1; }", "class A { #x = 1; }", "class A { static { } }", "class A { m() {} }", "class A extends Object {}", "class A extends (class {}) {}",
  "class A { /* c */ }", "class A { // c\n }", "class\nA\n{\n}", "class A { ['computed']() {} }", "class A { get g() { return 1; } set g(v) {} }", "class A { static async *gen() {} }", "class A { 'str'() {} 1() {} }",
  "class A { constructor() { super2; } }", "class A {;}", "class A { a;b;c }", "class A { é() {} }", "class A { \u{1F600}() {} }", "class é {}", "class A { m() { return `t${1}`; } }", "class A { m() { return /re/g; } }",
  "class A { x = class B {}; }", "class A { m() { return class {}; } }",
];
for (const src of toStringCases) {
  add(`const C = (${src}); return String(C)`, `const C = (${src}); return [Function.prototype.toString.call(C).length, C.toString() === String(C), String(C) === ${JSON.stringify(src)}]`);
}
add(
  "class A { m(a, b) { return a + b; } } return [String(A.prototype.m), String(Object.getOwnPropertyDescriptor(A.prototype, 'm').value)]", "class A { get g() { return 1; } set g(v) {} } const d = Object.getOwnPropertyDescriptor(A.prototype, 'g'); return [String(d.get), String(d.set)]",
  "class A { static async *s() {} } return String(A.s)", "class A { #p() {} static f(o) { return String(o.#p); } } return A.f(new A)", "class A { get #g() { return 1; } static f(o) { return Object.keys(o).length; } } return A.f(new A)",
  "class A { ['a' + 'b']() {} } return String(A.prototype.ab)", "class A { *[Symbol.iterator]() {} } return String(A.prototype[Symbol.iterator])", "class A { 'quoted'() {} } return String(A.prototype.quoted)",
  "class A { static x = () => 1; } return String(A.x)", "class A { x = function () {}; } return String(new A().x)", "class A { constructor(a) {} } return String(A.prototype.constructor) === String(A)",
  "const A = class {}; return String(A)", "const A = class N { }; return String(A)", "return String(class {})", "return String(class extends Object {})", "return `${class A {}}`", "return String(new Proxy(class A {}, {}))",
  "class A {} return String(A.bind(null))", "class A {} return String(A.bind(null)).includes('native code')", "class A {} return Function.prototype.toString.call(new Proxy(A, {}))",
  "class A {} return Function.prototype.toString.call(new Proxy({}, {}))", "class A {} return Object.prototype.toString.call(A) + Object.prototype.toString.call(new A)", "class A { static toString() { return 'custom'; } } return [String(A), Function.prototype.toString.call(A)]",
  "class A { static toString() { return 'custom'; } } return `${A}` + A", "class A { static [Symbol.toPrimitive]() { return 'prim'; } } return `${A}` + A", "class A { toString() { return 'inst'; } } return [String(A), String(new A)]",
  "class A {} A.toString = () => 'patched'; return String(A)", "class A {} Object.defineProperty(A, 'name', { value: 'Renamed' }); return [String(A), A.name]", "class A { static name = 'N'; } return String(A)",
  "return typeof Function.prototype.toString.call(class { static #p = 1; })", "return String(class { 'use strict'; })", "return String(class A { static { this.v = 1; } })",
  "return (class A { m() {} }).prototype.m.toString()", "return (class A { static m() {} }).m.toString()", "return (class A { static async m() {} }).m.toString()", "return (class A { static get x() { return 1; } }, Object.getOwnPropertyDescriptor(class B { static get x() { return 1; } }, 'x').get.toString())",
  "return Function.prototype.toString.call(Reflect.getOwnPropertyDescriptor(class { set x(v) {} }.prototype, 'x').set)", "return String(class { static *g() {} }.g)", "return String(class { async m() {} }.prototype.m)",
);

// ---- 10. Acessores e métodos privados: identidade, nome e atributos observáveis.
add(
  "class A { #m() {} static n(o) { return o.#m.name; } } return A.n(new A)", "class A { get #g() { return 1; } static n() { return 'x'; } } return A.n()", "class A { #m() {} static n(o) { return o.#m.length; } } return A.n(new A)",
  "class A { #m(a, b) {} static n(o) { return o.#m.length; } } return A.n(new A)", "class A { static #m() {} static n() { return A.#m.name; } } return A.n()", "class A { static async #m() {} static n() { return A.#m.name; } } return A.n()",
  "class A { *#m() {} static n(o) { return o.#m.name; } } return A.n(new A)", "class A { #m = function () {}; static n(o) { return o.#m.name; } } return A.n(new A)", "class A { #m = () => {}; static n(o) { return o.#m.name; } } return A.n(new A)",
  "class A { #m = class {}; static n(o) { return o.#m.name; } } return A.n(new A)", "class A { static #m = function () {}; static n() { return A.#m.name; } } return A.n()", "class A { static #m = class {}; static n() { return A.#m.name; } } return A.n()",
  "class A { #m() {} static n(o) { return Object.getOwnPropertyNames(o.#m).join(); } } return A.n(new A)", "class A { #m() {} static n(o) { return o.#m.hasOwnProperty('prototype'); } } return A.n(new A)",
  "class A { #m() {} static n(o) { return new o.#m; } } return (() => { try { return A.n(new A); } catch (e) { return e.name + ': ' + e.message; } })()", "class A { #m() { return new.target; } static n(o) { return o.#m(); } } return A.n(new A)",
  "class A { #m() { return arguments.length; } static n(o) { return o.#m(1, 2); } } return A.n(new A)", "class A { #m() { return this; } static n(o) { return o.#m.call(7); } } return A.n(new A)",
  "class A { #m() { return typeof this; } static n(o) { return o.#m.call(7); } } return A.n(new A)", "class A { get #g() { return typeof this; } static n(o) { return o.#g; } } return A.n(new A)",
  "class A { static get #g() { return this === A; } static n() { return A.#g; } } return A.n()", "class A { #m() {} static n(o) { return o.#m === o.#m; } } return A.n(new A)", "class A { #m() {} static n(a) { return a.#m; } } return A.n(new A) === A.n(new A)",
  "class A { #p = 1; static n(o) { return delete o.x; } } return A.n(new A)", "class A { #p = 1; static n(o) { return o.#p; } } const a = new A; Object.preventExtensions(a); return A.n(a)",
  "class A { #p = 1; static n(o) { return o.#p; } } const a = new A; Object.freeze(a); return A.n(a)", "class A { #p = 1; static s(o) { o.#p = 3; return o.#p; } } const a = new A; Object.freeze(a); return A.s(a)",
  "class A { #p = 1; static s(o) { o.#p = 3; return o.#p; } } const a = new A; Object.seal(a); return A.s(a)", "class A { #p = 1; static n(o) { return o.#p; } } const a = new A; Object.defineProperty(a, '#p', { value: 9 }); return A.n(a)",
  "class A { #p = 1; static n(o) { return o['#p']; } } return A.n(new A)", "class A { #p = 1; static n(o) { return o['#p'] = 5; } } const a = new A; A.n(a); return [Object.keys(a).join(), a['#p']]",
  "class A { static #p = 1; static n() { return Reflect.ownKeys(A).join(); } } return A.n()", "class A { static #p = 1; static x = 2; static n() { return JSON.stringify(Object.getOwnPropertyDescriptors(A).x); } } return A.n()",
  "class A { #p = 1; } return Object.getOwnPropertyNames(Object.getPrototypeOf(new A)).join()", "class A { #p = 1; m() {} } class B extends A { #p = 2; static n(o) { return o.#p; } } return B.n(new B)",
  "class A { #p = 1; static a(o) { return o.#p; } } class B extends A { #p = 2; static b(o) { return o.#p; } } const b = new B; return [A.a(b), B.b(b)]",
  "class A { #p = 1; static a(o) { return o.#p; } } class B extends A { #p = 2; static b(o) { return o.#p; } } return (() => { try { return B.b(new A); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #p = 1; static a(o) { return #p in o; } } class B extends A { static b(o) { return #p in o; } }",
  "class A { #p = 1; static a(o) { return #p in o; } } class B extends A { #p = 2; static b(o) { return #p in o; } } return [A.a(new B), B.b(new A), B.b(new B)]",
  "class A { #p = 1; } class B { #p = 1; static t(o) { return #p in o; } } return B.t(new A)", "class A { static #p = 1; static t(o) { return #p in o; } } return [A.t(A), A.t(class extends A {}), A.t(new A)]",
  "class A { #p = 1; static t() { return #p in A; } } return A.t()", "class A { #p = 1; static t() { return #p in A.prototype; } } return A.t()", "class A { #m() {} static t(o) { return #m in o; } } return [A.t(A.prototype), A.t(new A), A.t(Object.create(A.prototype))]",
  "class A { static #m() {} static t(o) { return #m in o; } } return [A.t(A), A.t(Object.create(A)), A.t(class extends A {})]", "class A { get #g() { return 1; } static t(o) { return #g in o; } } return [A.t(new A), A.t({})]",
  "class A { #a; #b; static t(o) { return [#a in o, #b in o]; } constructor(x) { if (x) return Object.create(null); } } return [A.t(new A), A.t(new A(true))]",
  "class A { #a; static t(o) { return #a in o && o.#a === undefined; } } return [A.t(new A), A.t({})]", "class A { #a = 1; static t(o) { return #a in o ? o.#a : 'none'; } } return [A.t(new A), A.t({})]",
  "class A { #a = 1; static t(o) { return !(#a in o); } } return [A.t(new A), A.t({})]", "class A { #a = 1; static t(o) { return (#a in o) === true; } } return A.t(new A)", "class A { #a = 1; static t(o, p) { return #a in o && #a in p; } } return [A.t(new A, new A), A.t(new A, {})]",
  "class A { #a = 1; static t(o) { return typeof (#a in o); } } return A.t({})", "class A { #a = 1; static t(o) { return (#a in o) + 1; } } return A.t(new A)", "class A { #a = 1; static t(o) { return #a in o ? 'y' : 'n'; } } return [A.t(new A), A.t({})]",
  "class A { #a = 1; static t(o) { return [#a in o, #a in o,].length; } } return A.t(new A)", "class A { #a = 1; static t(o) { if (#a in o) return 'in'; return 'out'; } } return [A.t(new A), A.t({})]",
  "class A { #a = 1; static t(o) { return o && #a in o; } } return [A.t(new A), A.t(null)]", "class A { #a = 1; static t(o) { return (#a in o) in { true: 1 }; } } return A.t(new A)",
  "class A { #a = 1; static t(o, k) { return k in o && #a in o; } } return A.t(new A, 'zz')", "class A { #a = 1; static t(o) { return 'x' in o || #a in o; } } return A.t(new A)",
  "class A { #a = 1; static t(o) { return #a in o instanceof Object; } } return (() => { try { return A.t(new A); } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { #a = 1; static t(o) { return #a in (o) in {}; } } return (() => { try { return A.t(new A); } catch (e) { return e.name; } })()",
  "class A { #a = 1; static t(o) { return #a in Object(o); } } return [A.t(new A), A.t(1)]", "class A { #a = 1; static t(o) { return #a in (0, o); } } return A.t(new A)",
  "class A { #a = 1; static t(o) { return #a in o.p; } } return A.t({ p: new A })", "class A { #a = 1; static t(o) { return #a in o?.p; } } return [A.t({ p: new A }), (() => { try { return A.t(null); } catch (e) { return e.name + ': ' + e.message; } })()]",
);

// ---- 11. Mais erros de acesso a #x em objeto errado (mensagens por forma da operação).
const wrongOps = [
  ["read field", "o.#p"], ["write field", "o.#p = 1"], ["compound field", "o.#p += 1"], ["incr field", "o.#p++"], ["call method", "o.#m()"], ["read method", "o.#m"], ["write method", "o.#m = 1"],
  ["read getter", "o.#g"], ["write getter", "o.#g = 1"], ["read setter", "o.#s"], ["write setter", "o.#s = 1"], ["call field", "o.#p()"], ["new field", "new o.#p"], ["delete-like", "o?.#p"],
  ["template tag", "o.#m`x`"], ["spread", "[...o.#p]"], ["destructure target", "[o.#p] = [1]"], ["typeof", "typeof o.#p"], ["logical or", "o.#p ||= 1"], ["nullish", "o.#p ??= 1"],
];
const wrongRecv = ["{}", "null", "undefined", "1", "[]", "Object.create(new A)", "new B", "A", "A.prototype", "function () {}", "Symbol()", "new Proxy(new A, {})"];
for (const [name, expr] of wrongOps) {
  for (const r of wrongRecv) {
    if (pick(6)) add(`class A { #p = 1; #m() {} get #g() { return 1; } set #s(v) {} static t(o) { return ${expr}; } }\nclass B { #p = 1; }\nreturn A.t(${r})`);
  }
}
for (const [name, expr] of wrongOps.slice(0, 8)) {
  add(`class A { static #p = 1; static #m() {} static get #g() { return 1; } static set #s(v) {} static t(o) { return ${expr}; } }\nclass B extends A {}\nreturn [A.t(A), (() => { try { return A.t(B); } catch (e) { return e.name + ': ' + e.message; } })(), (() => { try { return A.t(new A); } catch (e) { return e.name + ': ' + e.message; } })()]`);
}

// ---- 12. Campos: avaliação, enumeração e define semantics.
add(
  "class A { x = 1; } class B extends A { x = 2; } return [new B().x, Object.keys(new B).join()]", "class A { set x(v) { this.set = v; } } class B extends A { x = 1; } const b = new B; return [b.set, Object.hasOwn(b, 'x'), b.x]",
  "class A { x = 1; } class B extends A { get x() { return 'g'; } } const b = new B; return [b.x, Object.hasOwn(b, 'x')]", "class A { get x() { return 'g'; } } class B { x = 1; } return new B().x",
  "class A { x = 1; static x = 2; } return [new A().x, A.x]", "class A { 'a b' = 1; 3 = 2; [Symbol.for('s')] = 3; } return Reflect.ownKeys(new A).map(String)", "class A { b = 1; a = 2; 1 = 3; 0 = 4; } return Reflect.ownKeys(new A).map(String)",
  "class A { x = 1; x = 2; } const a = new A; return [a.x, Object.keys(a).length]", "class A { x = this; } const a = new A; return a.x === a", "class A { x = 1; y = this.x + 1; z = this.y + 1; } const a = new A; return [a.x, a.y, a.z]",
  "class A { y = this.x; x = 1; } return new A().y", "class A { x = () => this.y; y = 5; } return new A().x()", "class A { x = (() => this.y)(); y = 5; } return new A().x", "class A { x = new A; } return (() => { try { new A; } catch (e) { return e.name; } })()",
  "let n = 0; class A { x = n++; } new A; new A; return n", "class A { x = {}; } return new A().x === new A().x", "class A { static x = {}; } return A.x === A.x", "class A { x = []; } const a = new A, b = new A; a.x.push(1); return b.x.length",
  "class A { [Symbol.toPrimitive] = () => 'p'; } return `${new A}`", "class A { x = 1; } const a = new A; delete a.x; return [a.x, 'x' in a]", "class A { x = 1; } const a = new A; a.x = 2; return a.x", "class A { x = 1; } const a = new A; Object.freeze(a); return (() => { try { a.x = 2; return a.x; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { x; } const a = new A; return [Object.hasOwn(a, 'x'), a.x, Object.keys(a).join()]", "class A { static x; } return [Object.hasOwn(A, 'x'), A.x]", "class A { 'x'; } return Object.hasOwn(new A, 'x')", "class A { x = undefined; } return Object.hasOwn(new A, 'x')",
  "const key = { toString() { return 'dyn'; } }; class A { [key] = 1; } return Object.keys(new A).join()", "let c = 0; const key = { toString() { c++; return 'dyn'; } }; class A { [key] = 1; } new A; new A; return c",
  "class A { static async = 1; static get = 2; static set = 3; static static = 4; } return Object.keys(A).join()", "class A { async; get; set; static; } return Object.keys(new A).join()", "class A { get x() { return 1; } static get x() { return 2; } } return [new A().x, A.x]",
  "class A { 'constructor2' = 1; } return Object.keys(new A).join()", "class A { ['constructor'] = 1; } return Object.keys(new A).join()", "class A { static ['constructor'] = 1; } return A.constructor", "class A { ['prototype'] = 1; } return Object.keys(new A).join()",
  "class A { static ['prototype'] = 1; }", "class A { static ['name'] = 'nn'; } return A.name", "class A { static 'length' = 9; } return A.length", "class A { static x = A.name; } return A.x", "class A { static x = this.length; constructor(a, b) {} } return A.x",
  "const f = (class { static x = 1; }); return f.x", "class A { static x = 1; static y = A.x + 1; static z = this.y + 1; } return [A.x, A.y, A.z]", "class A extends (class { static b = 1; }) { static x = super.b + 1; } return A.x",
  "class A extends (class { static b = 1; }) { static x = this.b + 1; } return A.x", "class A extends (class { static b = 1; }) { static b = 5; static x = super.b; } return A.x", "class A { static x = () => { class B { static y = this; } return B.y === A; }; } return A.x()",
  "class A { static x = class { static y = 1; }; } return [A.x.name, A.x.y]", "class A { static x = class N { static y = 1; }; } return [A.x.name, A.x.y]", "class A { static x = (class {}).name; } return JSON.stringify(A.x)",
  "class A { static x = function () {}; static y = function () {}.name; } return [A.x.name, A.y]", "class A { static [Symbol.iterator] = function () {}; } return A[Symbol.iterator].name", "class A { static #p = function () {}; static n() { return A.#p.name; } } return A.n()",
  "class A { static x = (() => {}); } return A.x.name", "class A { x = (() => {}); } return new A().x.name", "class A { static x = (0, function () {}); } return JSON.stringify(A.x.name)", "class A { static x = async () => {}; } return A.x.name",
  "class A { static x = function* () {}; } return A.x.name", "class A { constructor() { this.v = 1; } x = this.v; } return new A().x", "class A { x = this.v; constructor() { this.v = 1; } } return new A().x",
  "class A { constructor() { this.x = 'ctor'; } x = 'field'; } return new A().x", "class A { x = 'field'; constructor() { this.x = 'ctor'; } } return new A().x", "class A { x = 'a'; } class B extends A { constructor() { super(); this.seen = this.x; } x = 'b'; } return new B().seen",
  "class A { constructor() { this.seen = this.x; } x = 'a'; } class B extends A { x = 'b'; } return new B().seen", "class A { constructor() { this.seen = this.x; } } class B extends A { x = 'b'; } return [new B().seen, new B().x]",
  "class A { constructor() { this.m(); } m() { this.called = typeof this.x; } } class B extends A { x = 1; m() { this.called2 = typeof this.x; } } const b = new B; return [b.called, b.called2]",
  "class A { constructor() { this.hook(); } hook() {} } class B extends A { x = 1; hook() { this.seen = this.x; } } return new B().seen", "class A { constructor() { this.hook(); } hook() {} } class B extends A { #x = 1; hook() { this.#x; } } return (() => { try { return new B; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { constructor() { this.hook(); } hook() {} } class B extends A { #m() { return 1; } hook() { this.seen = typeof this.#m; } } return (() => { try { return new B; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { constructor() { this.hook(); } hook() {} } class B extends A { get #g() { return 1; } hook() { this.seen = this.#g; } } return (() => { try { return new B; } catch (e) { return e.name + ': ' + e.message; } })()",
  "class A { constructor() { this.hook(); } hook() {} } class B extends A { #x = 1; hook() { this.seen = #x in this; } } return new B().seen",
);

// ---- Executa no bun e emite o tsv.
const baseSources = new Set();
for (const program of knownPrograms("class_edge_bun.tsv", ["class_bun.tsv", "brand_bun.tsv", "proxy_class_bun.tsv"])) baseSources.add(JSON.stringify(program));
const seen = new Set();
let kept = 0;
let dropped = 0;
let dup = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of bodies) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>{${body}})`;
  if (baseSources.has(JSON.stringify(source))) { dup++; continue; }
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(body).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|file:\/\//.test(result)) {
    dropped++;
    process.stderr.write("caminho no resultado: " + JSON.stringify(body).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos do base ${dup}\n`);
