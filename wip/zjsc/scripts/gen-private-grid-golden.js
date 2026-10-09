// Gera tests/golden/private_grid_bun.tsv: campos e métodos privados (#x, #m(), get/set #a, static #s, #x in obj) em grade,
// medidos no bun. Seções:
//   A. operação x tipo de membro x alvo (instância errada, objeto comum, null, Proxy, subclasse, congelado, primitivo...),
//      com a mensagem exata do TypeError;
//   B. forma de `#x in o` x membro x alvo;
//   C. return override do constructor da base: campo privado em objeto alheio, em Proxy, congelado, selado, não
//      extensível, dupla inicialização, classes irmãs e filhas no mesmo objeto;
//   D. ordem de inicialização com campos públicos, privados, computados e leitura de privado ainda não declarado;
//   E. privado em closures, eval direto (em vários sítios) e classes aninhadas com sombreamento.
// Cada programa roda num bun filho novo (no máximo 6 em paralelo, timeout de 8 s), sem APIs de host, e grava o texto em
// `globalThis.R`. Programas cuja fonte já aparece em outro golden são descartados. Uso:
//   bun scripts/gen-private-grid-golden.js > tests/golden/private_grid_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, sampleByHash } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE = [
  "function show(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (v === null || v === undefined || typeof v === 'boolean') return String(v);",
  "  if (typeof v === 'function') return 'fn';",
  "  if (d > 3) return '...';",
  "  if (Array.isArray(v)) return '[' + v.map(x => show(x, d + 1)).join(',') + ']';",
  "  if (Object.getPrototypeOf(v) === Object.prototype) return '{' + Object.keys(v).map(k => k + ':' + show(v[k], d + 1)).join(',') + '}';",
  "  return Object.prototype.toString.call(v);",
  "}",
  "function T(fn) { try { globalThis.R = show(fn()); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }",
  "",
].join("\n");

const bodies = [];
const add = (body) => bodies.push(body);

// ---- A. operação x membro x alvo.
const KINDS = {
  field: "#p = 1;",
  fieldNoInit: "#p;",
  fieldFn: "#p = () => 7;",
  method: "#p() { return 8; }",
  generator: "*#p() { yield 1; }",
  getter: "get #p() { return 9; }",
  setter: "set #p(v) { this.w = v; }",
  accessor: "get #p() { return 9; } set #p(v) { this.w = v; }",
  staticField: "static #p = 1;",
  staticMethod: "static #p() { return 8; }",
  staticGetter: "static get #p() { return 9; }",
  staticSetter: "static set #p(v) { this.w = v; }",
};
const OPS = {
  read: "o.#p",
  write: "o.#p = 5",
  call: "o.#p()",
  has: "#p in o",
  compound: "o.#p += 1",
  postInc: "o.#p++",
  preDec: "--o.#p",
  orAssign: "o.#p ||= 3",
  andAssign: "o.#p &&= 3",
  nullishAssign: "o.#p ??= 3",
  optional: "o?.#p",
  optionalCall: "o.#p?.()",
  tagged: "o.#p`x`",
  destructArray: "[o.#p] = [4]",
  destructObject: "({ a: o.#p } = { a: 4 })",
  forOf: "(() => { for (o.#p of [1]); return 'done'; })()",
  construct: "new o.#p()",
  typeofRead: "typeof o.#p",
  member: "o.#p.x",
  selfAssign: "o.#p = o.#p",
};
const TARGETS = {
  instance: "new C",
  other: "new D",
  plain: "({})",
  nul: "null",
  undef: "undefined",
  proxy: "new Proxy(new C, {})",
  proxyPlain: "new Proxy({}, {})",
  inherited: "Object.create(new C)",
  number: "1",
  string: "'s'",
  symbol: "Symbol('q')",
  func: "function () {}",
  subclass: "new S",
  frozen: "Object.freeze(new C)",
  nullProto: "Object.create(null)",
  classC: "C",
  classS: "S",
  classD: "D",
  array: "[]",
  prototype: "C.prototype",
};
for (const [kind, decl] of Object.entries(KINDS)) {
  for (const [op, expr] of Object.entries(OPS)) {
    for (const [target, init] of Object.entries(TARGETS)) {
      add(`class C { ${decl} static op(o) { return ${expr}; } } class D { ${decl} } class S extends C {} const o = ${init}; return C.op(o);`);
    }
  }
}

// ---- B. formas de `#x in o`.
const IN_FORMS = {
  paren: "(#p in o) === true",
  ternary: "#p in o ? 1 : 2",
  assign: "(() => { const r = #p in o; return r; })()",
  both: "#p in o && #q in o",
  either: "#p in o || #q in o",
  not: "!(#p in o)",
  sum: "1 + (#p in o)",
  member: "#p in o.x",
  nested: "#p in (#q in o ? o : {})",
  arrow: "(x => #p in x)(o)",
};
const IN_KINDS = {
  field: "#p = 1; #q = 2;",
  method: "#p() {} #q() {}",
  accessor: "get #p() { return 1; } set #p(v) {} get #q() { return 1; }",
  staticField: "static #p = 1; static #q = 2;",
  staticMethod: "static #p() {} static #q() {}",
  mixed: "#p = 1; static #q() {}",
};
const IN_TARGETS = ["new C", "new D", "{}", "null", "undefined", "1", "'s'", "C", "S", "new S", "new Proxy(new C, {})", "Object.create(new C)",
  "Object.freeze(new C)", "Symbol('q')", "function () {}", "{ x: new C }", "Object.create(null)", "[]"];
for (const [kind, decl] of Object.entries(IN_KINDS)) {
  for (const [form, expr] of Object.entries(IN_FORMS)) {
    for (const target of IN_TARGETS) {
      add(`class C { ${decl} static op(o) { return ${expr}; } } class D { ${decl} } class S extends C {} const o = ${target}; return C.op(o);`);
    }
  }
}

// ---- C. return override do constructor da base.
const DECLS_C = {
  field: ["#x = 1;", "o.#x"],
  fieldNoInit: ["#x;", "o.#x"],
  method: ["#x() { return 2; }", "o.#x()"],
  accessor: ["get #x() { return 3; } set #x(v) {}", "o.#x"],
  multi: ["#x = 1; #y = 2; #z() {}", "o.#y"],
  computed: ["[k()] = 1; #x = 2;", "o.#x"],
  staticToo: ["#x = 1; static #s = 2;", "o.#x"],
};
const TARGETS_C = {
  plain: "{}",
  frozen: "Object.freeze({})",
  sealed: "Object.seal({})",
  nonExtensible: "Object.preventExtensions({})",
  proxy: "new Proxy({}, {})",
  proxyTraps: "new Proxy({}, { defineProperty() { log.push('d'); return true; }, set() { log.push('s'); return true; }, has() { log.push('h'); return true; } })",
  func: "function () {}",
  array: "[]",
};
const PREFIX_C = { none: "", a: "new A(t);", a2: "new A2(t);", a3: "new A3(t);", both: "new A(t); new A2(t);" };
const ACTIONS_C = {
  again: "new A(t)",
  sibling: "new A2(t)",
  child: "new A3(t)",
  has: "[A.h(t), A2.h(t)]",
  getA: "A.g(t)",
  getA2: "A2.g(t)",
  childHas: "[A3.w(t), A.h(t)]",
  keys: "Object.getOwnPropertyNames(t).length",
  returned: "new A(t) === t",
  logged: "(new A(t), log.join())",
};
for (const [kind, [decl, get]] of Object.entries(DECLS_C)) {
  const cls = (name, base) => `class ${name} extends ${base} { ${decl} static h(o) { return #x in o; } static g(o) { return ${get}; } }`;
  for (const [target, init] of Object.entries(TARGETS_C)) {
    for (const [prefix, pre] of Object.entries(PREFIX_C)) {
      for (const [action, expr] of Object.entries(ACTIONS_C)) {
        add(`const log = []; const k = () => { log.push('k'); return 'kk'; }; class B { constructor(o) { return o; } } ${cls("A", "B")} ${cls("A2", "B")} class A3 extends A { #w = 1; static w(o) { return #w in o; } } const t = ${init}; ${pre} return ${expr};`);
      }
    }
  }
}

// ---- D. ordem de inicialização.
const permutations = (items) => items.length <= 1 ? [items] : items.flatMap((item, i) =>
  permutations([...items.slice(0, i), ...items.slice(i + 1)]).map((rest) => [item, ...rest]));
const INSTANCE_ITEMS = [
  "a = L('a');",
  "#b = L('b');",
  "[K('c')] = L('c');",
  "#d = (L('d'), this.#e);",
  "#e = L('e');",
];
const CONTEXTS = {
  base: ["", "class X {", "}", "new X"],
  derived: ["class B { constructor() { L('B'); } }", "class X extends B {", "}", "new X"],
  ctor: ["class B { constructor() { L('B'); } }", "class X extends B { constructor() { L('pre'); super(); L('post'); }", "}", "new X"],
  override: ["class B { constructor(o) { return o; } }", "class X extends B { constructor(o) { super(o); L('post'); }", "}", "new X({})"],
};
for (const [context, [head, open, close, make]] of Object.entries(CONTEXTS)) {
  for (const perm of permutations(INSTANCE_ITEMS)) {
    add(`const log = []; const L = (x) => (log.push(x), x); const K = (x) => (log.push('k' + x), x); try { ${head} ${open} ${perm.join(" ")} ${close} L('defined'); ${make}; } catch (e) { log.push(e.name + ':' + e.message); } return log;`);
  }
}
const STATIC_ITEMS = [
  "static a = L('sa');",
  "static #b = L('sb');",
  "static [K('c')] = L('sc');",
  "static { L('blk'); }",
  "static #d = (L('sd'), this.#e);",
  "static #e = L('se');",
];
// As 720 permutações são candidatas; entra um sexto delas, escolhido por hash do programa (`sampleByHash`).
const staticPrograms = permutations(STATIC_ITEMS).map((perm) =>
  `const log = []; const L = (x) => (log.push(x), x); const K = (x) => (log.push('k' + x), x); try { class X { ${perm.join(" ")} } L('defined'); } catch (e) { log.push(e.name + ':' + e.message); } return log;`);
for (const body of sampleByHash(staticPrograms, Math.ceil(staticPrograms.length / 6))) add(body);

// ---- E. closures, eval direto, classes aninhadas.
const EVAL_SOURCES = [
  "this.#x", "o.#x", "#x in this", "#x in o", "this.#m()", "o.#m()", "this.#x = 3", "o.#x = 3", "this.#nope", "delete this.#x",
  "typeof this.#x", "this.#a", "this.#a = 1", "(() => this.#x)()", "eval('this.#x')", "new Function('return this.#x')()",
  "class Q { m(t) { return t.#x; } } new Q().m(this)", "class Q { #x = 7; m(t) { return t.#x; } } new Q().m(this)",
  "class Q { #x = 7; m(t) { return t.#x; } } new Q().m(o)", "C.#s", "this.constructor.#s", "#s in this.constructor", "[this.#x, o.#x]",
];
const BODY_E = "#x = 1; #m() { return 'm'; } get #a() { return 'a'; } static #s = 2;";
const SITES = {
  method: (ev, t) => `class C { ${BODY_E} run(o) { return ${ev}; } } const O = ${t}; return new C().run(O);`,
  staticMethod: (ev, t) => `class C { ${BODY_E} static run(o) { return ${ev}; } } const O = ${t}; return C.run(O);`,
  ctorArrow: (ev, t) => `class C { ${BODY_E} constructor() { this.run = (o) => ${ev}; } } const O = ${t}; return new C().run(O);`,
  fieldArrow: (ev, t) => `class C { ${BODY_E} run = (o) => ${ev}; } const O = ${t}; return new C().run(O);`,
  staticBlock: (ev, t) => `class C { ${BODY_E} static { this.run = (o) => ${ev}; } } const O = ${t}; return C.run(O);`,
  nested: (ev, t) => `class C { ${BODY_E} nest() { return class N { run(o) { return ${ev}; } }; } } const O = ${t}; const N = new C().nest(); return new N().run(O);`,
  shadow: (ev, t) => `class C { ${BODY_E} nest() { return class N { #x = 'inner'; #m() { return 'im'; } run(o) { return ${ev}; } }; } } const O = ${t}; const N = new C().nest(); return new N().run(O);`,
  outside: (ev, t) => `class C { ${BODY_E} } const O = ${t}; return (function (o) { return ${ev}; }).call(new C, O);`,
};
const EVAL_TARGETS = ["new C", "{}", "null"];
for (const make of Object.values(SITES)) {
  for (const src of EVAL_SOURCES) {
    for (const target of EVAL_TARGETS) add(make(`eval(${JSON.stringify(src)})`, target));
  }
}

// Closures e fábricas de classe.
const CLOSURES = [
  "const make = () => class { #x = 1; static get(o) { return o.#x; } static has(o) { return #x in o; } }; const A = make(), B = make(); return [A.has(new A), A.has(new B), B.has(new B)];",
  "const make = () => class { #x = 1; static get(o) { return o.#x; } }; const A = make(), B = make(); return B.get(new A);",
  "const make = () => class { #x = 1; static get(o) { return o.#x; } }; const A = make(); return A.get(new A);",
  "class A { #x = 1; getter() { return () => this.#x; } } const f = new A().getter(); return f();",
  "class A { #x = 1; getter() { return () => this.#x; } } const f = new A().getter(); return f.call({});",
  "class A { #x = 1; getter() { return function () { return this.#x; }; } } const f = new A().getter(); return f.call(new A);",
  "class A { #x = 1; getter() { return function () { return this.#x; }; } } const f = new A().getter(); return f();",
  "class A { #x = 1; static f = (o) => o.#x; } return [A.f(new A)];",
  "class A { #x = 1; static f = (o) => o.#x; } return A.f({});",
  "class A { #m() { return 1; } static f = (o) => o.#m(); } return A.f({});",
  "class A { #x = 1; m() { return [1, 2].map(() => this.#x); } } return new A().m();",
  "class A { #x = 1; m() { return [1, 2].map(function () { return this.#x; }, this); } } return new A().m();",
  "class A { #x = 1; m() { return [1, 2].map(function () { return this.#x; }); } } return new A().m();",
  "class A { #x = 1; m() { return [1, 2].map(function () { return this.#x; }, new A); } } return new A().m();",
  "class A { #x = 1; m() { const self = this; return { f() { return self.#x; } }.f(); } } return new A().m();",
  "class A { #x = 1; m() { return { f() { return this.#x; } }.f(); } } return new A().m();",
  "class A { #x = 1; m() { return ({ get v() { return this.#x; } }).v; } } return new A().m();",
  "class A { #x = 1; m() { return `${this.#x}`; } } return new A().m();",
  "class A { #x = 1; m() { return `${this.#x}`; } } return A.prototype.m.call({});",
  "class A { #x = 1; m() { return `${this.#x}`; } } return A.prototype.m.call(Object.create(new A));",
  "class A { #x = 1; m() { return this.#x; } } return A.prototype.m.call(new Proxy(new A, {}));",
  "class A { #x = 1; m() { return this.#x; } } return A.prototype.m.call(new Proxy(new A, { get() { return 5; } }));",
  "const log = []; class A { #x = 1; static get(o) { return o.#x; } } const p = new Proxy(new A, { get(t, k) { log.push(String(k)); return t[k]; }, has(t, k) { log.push('has'); return k in t; } }); try { A.get(p); } catch (e) { log.push(e.name); } return log;",
  "const log = []; class A { #x = 1; static has(o) { return #x in o; } } const p = new Proxy(new A, { has() { log.push('has'); return true; }, getPrototypeOf() { log.push('gpo'); return null; } }); return [A.has(p), log];",
  "class A { #x = 1; y = 2; } const a = new A; return [Object.keys(a), Object.getOwnPropertyNames(a), Reflect.ownKeys(a), JSON.stringify(a), Object.getOwnPropertySymbols(a).length];",
  "class A { #x = 1; static g(o) { return o.#x; } static s(o) { o.#x = 2; } } const a = Object.freeze(new A); A.s(a); return A.g(a);",
  "class A { #x = 1; static g(o) { return o.#x; } static s(o) { o.#x = 2; } } const a = Object.seal(new A); A.s(a); return A.g(a);",
  "class A { #x = 1; static g(o) { return o.#x; } } const a = new A; return A.g(Object.assign(a, { x: 1 }));",
  "class A { #x = 1; static g(o) { return o.#x; } } const a = new A; const b = Object.create(a); return A.g(b);",
  "class A { #x = 1; static g(o) { return o.#x; } } const a = new A; const b = Object.setPrototypeOf({}, a); return A.g(b);",
  "class A { #x = 1; static g(o) { return o.#x; } } const a = new A; const b = { ...a }; return A.g(b);",
  "class A { #x = 1; static g(o) { return o.#x; } } const a = new A; const b = Object.assign({}, a); return A.g(b);",
  "class A { #x = 1; static g(o) { return o.#x; } } class B extends A {} return A.g(new B);",
  "class A { #x = 1; static g(o) { return o.#x; } } class B extends A { static h(o) { return super.g(o); } } return B.h(new B);",
  "class A { #x = 1; static g() { return this.#x; } } class B extends A {} return B.g();",
  "class A { static #x = 1; static g() { return this.#x; } } class B extends A {} return B.g();",
  "class A { static #x = 1; static g() { return A.#x; } } class B extends A {} return B.g();",
  "class A { static #m() { return 1; } static g() { return this.#m(); } } class B extends A {} return [A.g(), B.g()];",
  "class A { static #m() { return 1; } static g() { return this.#m(); } } class B extends A {} return B.g();",
  "class A { static #x = 1; static g() { return this.#x; } } const g = A.g; return g();",
  "class A { static #x = 1; static g() { return this.#x; } } const g = A.g; return g.call(A);",
  "class A { #x = 1; static g(o) { return o.#x; } } return A.g(new (class extends A { constructor() { super(); } }));",
  "class A { #x = 1; static g(o) { return o.#x; } } return A.g(new (class extends A { #x = 2; })); ",
  "class A { #x = 1; static g(o) { return o.#x; } } class B extends A { #x = 2; static h(o) { return o.#x; } } const b = new B; return [A.g(b), B.h(b)];",
  "class A { #x = 1; static g(o) { return o.#x; } } class B extends A { #x = 2; static h(o) { return o.#x; } } return B.h(new A);",
  "class A { #x = 1; } class B extends A { static h(o) { return o.#x; } } return 1;",
  "return eval('class A { #x = 1; static g(o) { return o.#x; } } A.g(new A)');",
  "return eval('class A { #x = 1; static g(o) { return o.#x; } } A.g({})');",
  "return (0, eval)('class A { #x = 1; static g(o) { return o.#x; } } A.g(new A)');",
  "return new Function('class A { #x = 1; static g(o) { return o.#x; } } return A.g(new A)')();",
  "class A { #x = 1; static g(o) { return eval('o.#x'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return (0, eval)('o.#x'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return new Function('o', 'return o.#x')(o); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('(function () { return o.#x; })')(); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('eval(\"o.#x\")'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('() => o.#x')(); } } return A.g(new A);",
  "class A { static g(o) { return eval('o.#x'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('class Z { static f(o) { return o.#x; } } Z.f(o)'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('class Z { #x = 2; static f(o) { return o.#x; } } Z.f(o)'); } } return A.g(new A);",
  "class A { #x = 1; static g(o) { return eval('class Z { #x = 2; static f(o) { return o.#x; } } Z.f(new Z)'); } } return A.g(new A);",
  "class A { #x = 1; #y = this.#x + 1; static g(o) { return o.#y; } } return A.g(new A);",
  "class A { #y = this.#x + 1; #x = 1; static g(o) { return o.#y; } } return A.g(new A);",
  "class A { #y = this.#m(); #m() { return 4; } static g(o) { return o.#y; } } return A.g(new A);",
  "class A { #y = this.#g; get #g() { return 5; } static g(o) { return o.#y; } } return A.g(new A);",
  "class A { static #y = A.#m(); static #m() { return 4; } static g() { return A.#y; } } return A.g();",
  "class A { static #y = A.#x; static #x = 4; static g() { return A.#y; } } return A.g();",
  "class A { static #x = 4; static #y = A.#x; static g() { return A.#y; } } return A.g();",
  "class A { static #y = this.#x; static #x = 4; } return 1;",
  "class A { x = this.#p; #p = 1; } return new A().x;",
  "class A { x = this.#p; #p() { return 1; } } return typeof new A().x;",
  "class A { x = this.#p; get #p() { return 6; } } return new A().x;",
  "class A { #p = 1; x = this.#p; } return new A().x;",
  "class A { x = () => this.#p; #p = 1; } return new A().x();",
  "class A { x = (() => this.#p)(); #p = 1; } return new A().x;",
  "class A { [(() => { try { return new A; } catch (e) { return 'k:' + e.name; } })()] = 1; #p = 1; } return 1;",
  "let K; class A { static #p = 1; static [(K = () => A.#p, 'k')] = K(); } return A.k;",
  "let K; class A { static [(K = () => 1, 'k')] = K(); static #p = 1; } return A.k;",
  "class A { #p = 1; static g(o) { return o.#p; } } A.g(new A); delete A.prototype; return A.g(new A);",
  "class A { #p = 1; static g(o) { return o.#p; } } const B = A; A = null; return B.g(new B);",
  "class A { #p = 1; static g(o) { return o.#p; } } Object.freeze(A); return A.g(new A);",
  "class A { #p = 1; static g(o) { return o.#p; } } Object.setPrototypeOf(A.prototype, null); return A.g(new A);",
  "class A { #p = 1; static g(o) { return o.#p; } } const a = new A; Object.setPrototypeOf(a, null); return A.g(a);",
  "class A { #p = 1; static g(o) { return o.#p; } } const a = new A; Object.defineProperty(a, 'p', { get() { return 9; } }); return A.g(a);",
  "class A { #p = 1; static g(o) { return o.#p; } } const a = new A; return A.g(new Proxy(a, {})) ;",
  "class A { #p = 1; static g(o) { return o.#p; } } const a = new A; return Reflect.apply(A.g, null, [a]);",
  "class A { #p = 1; static g(o) { return o.#p; } } const a = new A; return A.g.call(null, a);",
  "class A { #p = 1; g() { return this.#p; } } const a = new A; return Reflect.apply(a.g, a, []);",
  "class A { #p = 1; g() { return this.#p; } } const a = new A; return Reflect.apply(a.g, {}, []);",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.bind(new A)();",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.bind({})();",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.apply(1);",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.apply(null);",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.apply(undefined);",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g.apply('s');",
  "class A { #p = 1; g() { return this.#p; } } return A.prototype.g();",
  "class A { #p = 1; static g() { return this.#p; } } return A.g();",
  "class A { get #p() { return 1; } static g(o) { o.#p = 3; } } return A.g(new A);",
  "class A { set #p(v) { } static g(o) { return o.#p; } } return A.g(new A);",
  "class A { #p() { } static g(o) { o.#p = 3; } } return A.g(new A);",
  "class A { #p() { } static g(o) { o.#p++; } } return A.g(new A);",
  "class A { #p() { } static g(o) { [o.#p] = [1]; } } return A.g(new A);",
  "class A { #p() { } static g(o) { for (o.#p of [1]); } } return A.g(new A);",
  "class A { #p() { } static g(o) { return o.#p ??= 1; } } return A.g(new A);",
  "class A { #p() { } static g(o) { return o.#p ||= 1; } } return A.g(new A);",
  "class A { #p() { } static g(o) { return o.#p &&= 1; } } return A.g(new A);",
];
for (const body of CLOSURES) add(body);

// ---- Sintaxe inválida avaliada via eval/Function (SyntaxError com mensagem exata).
const SYNTAX = [
  "class A { #x; #x; }", "class A { #x; get #x() { return 1; } }", "class A { get #x() { return 1; } get #x() { return 2; } }",
  "class A { get #x() { return 1; } set #x(v) {} }", "class A { static #x; #x; }", "class A { static get #x() {} set #x(v) {} }",
  "class A { #constructor; }", "class A { #constructor() {} }", "class A { constructor() { delete this.#x; } #x; }",
  "class A { #x; m() { delete this.#x; } }", "class A { #x; m() { delete (this.#x); } }", "class A { #x; m() { delete ((this.#x)); } }",
  "class A { #x; m() { delete this?.#x; } }", "class A { m() { this.#y; } }", "class A { m() { #y in this; } }", "class A { #x; m() { #x; } }",
  "class A { #x; m() { return #x + 1; } }", "class A { #x; m() { return !#x in this; } }", "class A { #x; m() { return #x in #x in this; } }",
  "class A { #x; m() { return 1 + #x in this; } }", "class A { #x; m(o) { return #x in o in o; } }", "class A { #x; m(o) { return (#x) in o; } }",
  "class A { #x; m(o) { return #x instanceof o; } }", "class A { #x; m(o) { return o.# x; } }", "class A { # x; }", "class A { #\\u0078; m(o) { return o.#x; } }",
  "class A { #x; m(o) { return o.#\\u0078; } }", "class A { #x; m(o) { return o?.#x; } }", "class A { #x; m(o) { return o?.#x?.#x; } }",
  "class A { #x; m(o) { return super.#x; } }", "class A extends Object { #x; m(o) { return super.#x; } }", "class A { #x; m(o) { new.#x; } }",
  "class A { #x; m(o) { return o.#x`a`; } }", "class A { #x; m(o) { return o.#x = 1; } }", "class A { #x; static { this.#x; } }",
  "class A { static #x; static { A.#x = 1; } }", "class A { #x = 1; y = this.#x; }", "class A { #x = arguments; }", "class A { static #x = arguments; }",
  "class A { #x = () => arguments; }", "class A { #x = await 1; }", "async function f() { class A { #x = await 1; } }", "function* g() { class A { #x = yield; } }",
  "class A { #x; m() { class B { m(o) { return o.#x; } } } }", "class A { m() { class B { #x; } return this.#x; } }", "class A { #x; m() { { class B { #x; } } return this.#x; } }",
  "class A { #x; static m(o) { return o.#x.#y; } }", "class A { #x; m() { return function () { return this.#x; }; } }",
  "class A { #x; m() { return () => () => this.#x; } }", "class A { #x; ['#x']; }", "class A { '#x'; m() { return this.#x; } }",
  "class A { #x; m() { return this['#x']; } }", "class A { #x; m() { return this.#x; } } A.#x;", "class A { #x; } new A().#x;",
  "this.#x", "function f(o) { return o.#x; }", "o => o.#x", "({ m(o) { return o.#x; } })", "({ #x: 1 })", "({ #x() {} })",
  "class A { static async *#x() {} }", "class A { async #x() {} get #y() { return 1; } static async #z() {} }", "class A { *#x() {} async *#y() {} }",
  "class A { static #x() {} static #y; static get #z() { return 1; } }", "class A { #x\n#y }", "class A { #x = 1\n#y = 2\n }", "class A { #x\n[1] }",
  "class A { #x\n*#y() {} }", "class A { get\n#x() { return 1; } }", "class A { static\n#x = 1 }", "class A { async\n#x() {} }",
  "class A { #x = 1; #y = this.#x; static #z = A.#y; }", "class A { #await; #yield; #async; #static; #get; #set; #let; #of; #in; #if; }",
  "class A { #x; m(o) { return o.#x ?? 1; } }", "class A { #x; m(o) { return o.#x?.y; } }", "class A { #x; m(o) { return o.y.#x; } }",
  "class A { #x; m(o) { return o?.y.#x; } }", "class A { #x; m(o) { return (o?.y).#x; } }", "class A { #x; m(o) { return o?.[1].#x; } }",
  "class A { #x; m(o) { return o?.(1).#x; } }", "class A { #x; m(o) { return o.#x?.(); } }", "class A { #x; m(o) { return o.#x(); } }",
  "class A { #x; m(o) { return new o.#x; } }", "class A { #x; m(o) { return new o.#x(); } }", "class A { #x; m(o) { return typeof o.#x; } }",
  "class A { #x; m(o) { return void o.#x; } }", "class A { #x; m(o) { o.#x++; ++o.#x; o.#x--; --o.#x; } }", "class A { #x; m(o) { [o.#x] = [1]; ({ a: o.#x } = {}); } }",
  "class A { #x; m(o) { for (o.#x in {}); for (o.#x of []); } }", "class A { #x; m(o) { ({ ...o.#x } = {}); } }", "class A { #x; m(o) { [...o.#x] = []; } }",
  "class A { #x; m(o) { ({ o.#x } = {}); } }", "class A { #x; m(o) { ({ a: o.#x = 1 } = {}); } }", "class A { #x; m(o) { `${o.#x}`; } }",
  "class A { #x; m(o) { o.#x\n.y; } }", "class A { #x; m(o) { o\n.#x; } }", "class A { #x; m(o) { o.\n#x; } }", "class A { #x; m(o) { o./**/#x; } }",
];
SYNTAX.forEach((src) => {
  add(`return eval(${JSON.stringify(src)});`);
  add(`return new Function(${JSON.stringify(src)})();`);
  add(`return (0, eval)(${JSON.stringify(src)});`);
});

// ---- Dedupe, filhos e saída.
const known = new Set(knownPrograms("private_grid_bun.tsv", (name) => name !== "private_grid_bun.tsv"));
const unique = [...new Set(bodies)];
const jobs = [];
let dup = 0;
for (const body of unique) {
  if (usesHostApi(body)) continue;
  const source = PRELUDE + `T(() => { ${body} });`;
  if (known.has(source)) { dup++; continue; }
  jobs.push({ body, source });
}

const runChild = (source) => new Promise((resolve) => {
  const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 8000, killSignal: "SIGKILL" });
  const out = [];
  child.stdout.on("data", (chunk) => out.push(chunk));
  child.stderr.on("data", () => {});
  child.on("close", (code) => resolve(code === 0 ? Buffer.concat(out).toString("utf8") : null));
  child.on("error", () => resolve(null));
  child.stdin.on("error", () => {});
  child.stdin.end(source);
});

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    for (;;) {
      const index = next++;
      if (index >= jobs.length) return;
      results[index] = await runChild(jobs[index].source);
    }
  };
  await Promise.all(Array.from({ length: 6 }, worker));
  const dash = new RegExp("[" + String.fromCharCode(0x2013) + String.fromCharCode(0x2014) + "]");
  const rows = [];
  let dropped = 0;
  jobs.forEach((job, index) => {
    const result = results[index];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || dash.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(job.body).slice(0, 160) + "\n");
      return;
    }
    rows.push({ source: job.source, result });
  });
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
  process.stdout.write(emitFactored("private_grid", rows));
})();
