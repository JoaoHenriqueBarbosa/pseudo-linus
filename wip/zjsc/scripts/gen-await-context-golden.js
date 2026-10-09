// Gera tests/golden/await_context_bun.tsv: `await` e `for await` em contexto, medidos no bun 1.4.2.
// Cobre await em posições de expressão (argumentos, template, spread, destructuring com default, chave computada,
// inicializador de campo de classe, operadores, laços, switch), `for await` sobre iteráveis síncronos com promises
// rejeitadas (fechamento do iterador, AsyncFromSyncIterator com return e throw via `yield*`), async arrow com
// `this`, `arguments` e `super`, métodos async em classes e object literals, `await using` e AsyncDisposableStack,
// Promise subclass com `then` customizado e a ordem de log de 2 a 4 atores concorrentes com awaits de valores
// não-promise, thenables e promises nativas. Script não tem `await` no topo: tudo roda em async IIFE.
// Cada programa roda por `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do
// bun não tocar na fonte) num processo bun filho próprio, depois do prelúdio AWAIT_HARNESS, também via vm. Os
// programas não usam API de host (setTimeout, process, console, require, Bun, queueMicrotask): só L, tick,
// thenable, ok e bad, mais as funções V e E que cada programa define no começo. O golden é o JSON de `globalThis.R`
// depois de esvaziar as microtarefas, ou `error<TAB>name<TAB>message JSON` se a fonte lançou de forma síncrona.
// O host do gerador drena as microtarefas com um setTimeout fora do programa.
// Programas já presentes em outros goldens de async são descartados. Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-await-context-golden.js > tests/golden/await_context_bun.tsv
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

// Mesmo texto embutido em tests/await_context_bun_golden.rs.
const HARNESS = `globalThis.R = [];
globalThis.L = function (x) { R.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.ok = function (v) { L("v:" + JSON.stringify(v)); };
globalThis.bad = function (e) { L("e:" + (e && e.name) + (e && e.name === "Error" ? ":" + e.message : "")); };
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(R); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
};`;

const PRE =
  "function V(x) { try { return typeof x === 'object' && x ? JSON.stringify(x) : String(x); } catch (e) { return 'V?'; } } " +
  "function E(e) { var s = (e && e.name) + ':' + (e && e.message); if (e && e.suppressed !== undefined) s += '[sup:' + E(e.suppressed) + ',err:' + E(e.error) + ']'; return s; } ";
const SUBP = "class SubP extends Promise { then(a, b) { L('sub.then'); return super.then(a, b); } } ";
const MK =
  "function mk(a, o) { o = o || {}; var i = 0; var it = { next() { L('n' + i); if (i < a.length) return { value: a[i++], done: false }; return { value: undefined, done: true }; } }; " +
  "if (o.ret !== false) it.return = function (v) { L('ret'); return o.retv === undefined ? {} : o.retv; }; " +
  "if (o.thr) it.throw = function (e) { L('thr'); return {}; }; var iterable = {}; iterable[Symbol.iterator] = function () { L('iter'); return it; }; return iterable; } ";

const root = path.join(__dirname, "..");
const existing = new Set();
for (const program of knownPrograms("await_context_bun.tsv", (name) => /(async|promise|microtask|dispose|generator|iterator)/.test(name) && name !== "await_context_bun.tsv")) existing.add(JSON.stringify(program));
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const body of sources) {
    const source = PRE + (/SubP/.test(body) ? SUBP : "") + (/\bmk\(/.test(body) ? MK : "") + body;
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source) && !existing.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

const BT = "`";
const OBS = [
  "Promise.resolve().then(() => L('o1')).then(() => L('o2')).then(() => L('o3')).then(() => L('o4')).then(() => L('o5')).then(() => L('o6'));",
  "(async () => { await null; L('o1'); await null; L('o2'); await null; L('o3'); await null; L('o4'); })();",
  "tick(10, 't10');",
];
let obsIndex = 0;
const obs = () => OBS[obsIndex++ % OBS.length];
const wrap = (body) => `(async () => { try { ${body} } catch (e) { L('c:' + E(e)); } })().then(ok, bad); ${obs()}`;

// 1. await em posições de expressão x operandos.
const OPERANDS = [
  "1", "null", "undefined", "Promise.resolve(2)", "thenable(3, 't')", "Promise.reject(new Error('r'))",
  "{ then(r) { L('th'); r(5); } }", "{ then(r, j) { j(new Error('j')); } }", "{ then() { throw new Error('tt'); } }",
  "new SubP(r => r(7))", "SubP.resolve(8)", "Promise.resolve(Promise.resolve(9))", "(async () => 10)()",
  "Object.assign(Promise.resolve(11), { then(a, b) { L('own.then'); return Promise.prototype.then.call(this, a, b); } })",
  "{ get then() { L('get'); return undefined; } }", "{ get then() { L('get'); return r => r(12); } }",
];
const POSITIONS = [
  "var x = await @; L(V(x));",
  "L(V([await @, await @]));",
  "function f(a, b) { return V(a) + '|' + V(b); } L(f(await @, 'z'));",
  "function f(a, b, c) { return V(a) + '|' + V(b) + '|' + V(c); } L(f(L('arg0'), await @, L('arg2')));",
  "L(" + BT + "a${await @}b${await @}c" + BT + ");",
  "L(V(Math.max(...[await @, 1])));",
  "var arr = [await @]; L(V([...arr, ...[await @]]));",
  "var { a = await @ } = {}; L(V(a));",
  "var [a = await @, b = await @] = []; L(V([a, b]));",
  "var { a = await @ } = { a: 1 }; L(V(a));",
  "var { [await @]: v } = { 1: 'one', null: 'nul', undefined: 'und', 2: 'two' }; L(V(v));",
  "var o = { [await @]: await @ }; L(V(o));",
  "class C { static [await @] = 1; } L(V(Object.getOwnPropertyNames(C)));",
  "class C { f = async () => await @; } new C().f().then(ok, bad);",
  "class C { static f = async () => (await @, 'sf'); } C.f().then(ok, bad);",
  "class C { g = (async () => await @)(); } new C().g.then(ok, bad);",
  "L(V((await @) + (await @)));",
  "L(V((await @) ? 'T' : 'F'));",
  "L(V((await @) && (await @)));",
  "L(V((await @) ?? 'dflt'));",
  "var o = { 1: 'a', null: 'n', undefined: 'u' }; L(V(o?.[await @]));",
  "L(typeof (await @));",
  "L(V(delete { a: 1 }[await @]));",
  "function tag(s, ...v) { return V(v); } L(tag" + BT + "x${await @}y" + BT + ");",
  "L(V((await @) in { 1: 1, null: 1, undefined: 1 }));",
  "L(V({} instanceof (await @)));",
  "switch (await @) { case 1: L('one'); break; case null: L('null'); break; default: L('def'); }",
  "switch (0) { case await @: L('hit'); break; default: L('miss'); }",
  "var i = 0; while (i < 2 && await @) { i++; L('w' + i); }",
  "for (var i = await @, n = 0; n < 2; n++) L(V(i));",
  "for (var i = 0; i < 2; i += (await @ ? 1 : 1)) L('i' + i);",
  "for (var x of [await @, 2]) L(V(x));",
  "for (var k in { a: 1, b: await @ }) L(k);",
  "try { throw await @; } catch (e) { L('thrown:' + V(e)); }",
  "L(V((await @, 'after')));",
  "L(V(await Promise.all([@, await @])));",
  "L(V(new (class { constructor(a) { this.a = a; } })(await @)));",
  "lbl: { L('in'); await @; break lbl; } L('out');",
  "do { L('d'); } while (await @ && false); L('end');",
  "var f = async (a = 1) => a; L(V(await f(await @)));",
  "L(V(await (await @)));",
  "async function inner() { return await @; } L(V(await inner()));",
  "async function inner() { return @; } L(V(await inner()));",
  "var s = Symbol('s'); var o = { [s]: await @ }; L(V(o[s]));",
  "var x = { ...(await @) }; L(V(x));",
  "var { ...rest } = await @; L(V(rest));",
  "L(V([...(await @)]));",
  "var o = { m(a) { return V(a); } }; L(o.m(await @));",
  "var o = { m(a) { return V(a); } }; L(o[(await @, 'm')](1));",
  "var a = [0]; a[await @ ? 0 : 0] = await @; L(V(a));",
  "var x = 5; x += await @; L(V(x));",
  "var x = 0; x ||= await @; L(V(x));",
  "var x = null; x ??= await @; L(V(x));",
  "var x = 1; x &&= await @; L(V(x));",
  "var o = {}; o.p = await @; L(V(o));",
  "var r = []; for (var x of [1, 2]) { r.push(await @); } L(V(r));",
  "var r = await Promise.race([@, new Promise(() => {})]); L(V(r));",
  "L(V(await Promise.allSettled([@])));",
  "L(V(await Promise.any([@, Promise.resolve('any')])));",
];
for (const operand of OPERANDS) {
  for (const position of POSITIONS) add(wrap(position.split("@").join(operand)));
}

// 2. for await sobre iteráveis síncronos com promises rejeitadas.
const SOURCES = [
  "[1, 2, 3]", "[Promise.resolve(1), 2]", "[1, Promise.reject(new Error('r')), 3]", "[Promise.reject(new Error('r0'))]",
  "[thenable(1, 'a'), thenable(2, 'b')]", "[{ then(r, j) { j(new Error('tj')); } }]", "mk([1, 2, 3])", "mk([1, 2, 3], { ret: false })",
  "mk([Promise.resolve(1), Promise.reject(new Error('x')), 3])", "mk([Promise.reject(new Error('x'))])", "mk([1, 2], { retv: 5 })",
  "mk([1, 2], { retv: Promise.resolve({}) })", "mk([SubP.resolve(1)])", "new Set([1, Promise.resolve(2)])", "new Map([[1, 2]])", "'ab'",
  "(function* () { try { yield 1; yield Promise.resolve(2); yield 3; } finally { L('gfin'); } })()",
  "(function* () { try { yield Promise.reject(new Error('gx')); } finally { L('gfin'); } })()", "[]", "[1, 2].values()",
  "mk([Promise.resolve(1), 2], { retv: Promise.reject(new Error('rr')) })",
];
const BODIES = [
  "L('b:' + V(x));", "L('b:' + V(x)); break;", "L('b:' + V(x)); continue;", "L('b:' + V(x)); throw new Error('body');",
  "if (x === 2) break; L(V(x));", "L(V(x)); await null; L('after');",
  "L(V(x)); try { await Promise.reject(new Error('inner')); } catch (e) { L('ic'); }",
];
for (const source of SOURCES) {
  for (const body of BODIES) {
    add(`(async () => { try { for await (var x of ${source}) { ${body} } L('done'); } catch (e) { L('c:' + E(e)); } finally { L('fin'); } })().then(ok, bad); ${obs()}`);
  }
  add(
    `async function f() { for await (var x of ${source}) { return x; } return 'none'; } f().then(ok, bad); ${obs()}`,
    `async function f() { try { for await (var x of ${source}) { return x; } } finally { L('ffin'); } } f().then(ok, bad); ${obs()}`,
    `(async () => { outer: for (var i = 0; i < 2; i++) { for await (var x of ${source}) { L(i + ':' + V(x)); if (x === 2 || i === 1) continue outer; } } L('end'); })().then(ok, bad); ${obs()}`,
    `(async () => { var x; try { for await (x of ${source}) L('x=' + V(x)); } catch (e) { L('c:' + E(e)); } L('last:' + V(x)); })().then(ok, bad); ${obs()}`,
    `(async () => { try { for await (const [a] of ${source}) L(V(a)); } catch (e) { L('c:' + E(e)); } })().then(ok, bad); ${obs()}`,
    `(async () => { var r = []; try { for await (var x of ${source}) { r.push(await x); } } catch (e) { r.push('c:' + E(e)); } L(V(r)); })().then(ok, bad); ${obs()}`
  );
}

// 3. AsyncFromSyncIterator: next, return e throw via yield*.
const YSRC = [
  "mk([1, 2, 3])", "mk([1, 2, 3], { ret: false })", "mk([1, 2, 3], { thr: true })", "mk([Promise.resolve(1), 2])",
  "mk([Promise.reject(new Error('x')), 2])", "mk([1, 2], { retv: 5 })", "mk([1, 2], { retv: Promise.resolve({ value: 'rv', done: true }) })",
  "mk([1, 2], { retv: Promise.reject(new Error('rr')) })", "mk([1, 2], { retv: { value: 'v', done: false } })",
  "mk([thenable(1, 'a')], { thr: true })", "[1, Promise.resolve(2)]", "(function* () { try { yield 1; yield 2; } finally { L('gfin'); } })()",
  "(function* () { try { yield 1; } catch (e) { L('gc:' + e.message); yield 'rec'; } })()",
];
const YOPS = ["it.return('R')", "it.throw(new Error('T'))", "it.next('N')"];
for (const source of YSRC) {
  for (const op of YOPS) {
    for (const warm of [0, 1]) {
      add(
        `async function* g() { try { var r = yield* ${source}; L('r:' + V(r)); } catch (e) { L('c:' + E(e)); } finally { L('gfin2'); } } var it = g(); (async () => { try { ${warm ? "L(V(await it.next()));" : ""} L(V(await ${op})); L(V(await it.next())); } catch (e) { L('oc:' + E(e)); } })().then(ok, bad); ${obs()}`
      );
    }
  }
}
// for await sobre iterável async customizado.
for (const next of [
  "return Promise.resolve({ value: i++, done: i > 3 });", "return { value: i++, done: i > 3 };", "return { then(r) { r({ value: i++, done: i > 3 }); } };",
  "return Promise.resolve(1);", "return 5;", "throw new Error('nx');", "return Promise.reject(new Error('nr'));", "return Promise.resolve({ value: Promise.resolve(i++), done: i > 3 });",
]) {
  for (const ret of ["", "return() { L('aret'); return {}; },", "return() { L('aret'); return Promise.reject(new Error('rr')); },", "return() { L('aret'); return 1; },"]) {
    add(`var i = 0; var it = { [Symbol.asyncIterator]() { return { next() { L('next' + i); ${next} }, ${ret} }; } }; (async () => { try { for await (var x of it) { L(V(x)); if (x === 1) break; } L('done'); } catch (e) { L('c:' + E(e)); } })().then(ok, bad); ${obs()}`);
  }
}

// 4. async arrow com this, arguments e super.
const ARROW_EXPRS = [
  "this.id", "arguments.length", "arguments[0]", "super.m()", "super.p", "(await null, this.id)", "(await null, arguments[1])", "(await null, super.m())",
  "(await (async () => this.id)())", "[this.id, arguments.length, super.m()].join()", "(() => this.id)()", "new.target",
  "(await thenable(this.id, 'th'))", "(await Promise.resolve(arguments.length))",
];
const ARROW_HOSTS = [
  (b) => `var base = { m() { return 'base.m:' + this.id; }, get p() { return 'base.p:' + this.id; } }; var o = { __proto__: base, id: 'o', async run(a, b) { ${b} } }; var go = () => o.run(1, 2);`,
  (b) => `class A { m() { return 'A.m:' + this.id; } get p() { return 'A.p:' + this.id; } } class B extends A { constructor() { super(); this.id = 'b'; } async run(a, b) { ${b} } } var go = () => new B().run(1, 2);`,
  (b) => `class A { static m() { return 'sA.m:' + this.name; } static get p() { return 'sA.p:' + this.name; } } class B extends A { static async run(a, b) { ${b} } } var go = () => B.run(1, 2);`,
  (b) => `var base = { m() { return 'base.m'; }, get p() { return 'base.p'; } }; var o = { __proto__: base, id: 'o2', run: async function (a, b) { ${b.replace(/super\.\w+(\(\))?/g, "'nosuper'")} } }; var go = () => o.run(1, 2);`,
];
for (const expr of ARROW_EXPRS) {
  ARROW_HOSTS.forEach((host, hi) => {
    add(
      host(`var f = async () => ${expr}; L(V(await f()));`) + ` go().then(ok, bad); ${obs()}`,
      host(`var f = async () => ${expr}; L(V(await f.call({ id: 'other' }, 'x', 'y')));`) + ` go().then(ok, bad); ${obs()}`,
      host(`var f = async () => ${expr}; await null; L(V(await f()));`) + ` go().then(ok, bad); ${obs()}`
    );
  });
}
add(
  "var g; var o = { id: 'o', async run() { g = async () => this.id + arguments.length; } }; (async () => { await o.run(1, 2, 3); L(await g.call({ id: 'x' })); })().then(ok, bad); " + OBS[0],
  "function F() { this.id = 'F'; this.f = async () => this.id; } var x = new F(); x.f.call({ id: 'z' }).then(ok, bad); " + OBS[1],
  "var f = async () => this === globalThis || this === undefined; f.call(5).then(ok, bad); " + OBS[2],
  "var f = async function () { return typeof this; }; f.call(5).then(ok, bad); f.call(null).then(ok, bad); " + OBS[0],
  "var f = async function () { 'use strict'; return typeof this; }; f.call(5).then(ok, bad); f.call(null).then(ok, bad); " + OBS[1],
  "var f = async function (a, b = 2) { arguments[0] = 'm'; return a; }; f('o').then(ok, bad); " + OBS[2],
  "var f = async function (a) { arguments[0] = 'm'; return a; }; f('o').then(ok, bad); " + OBS[0],
  "var f = async function (a) { 'use strict'; arguments[0] = 'm'; return a; }; f('o').then(ok, bad); " + OBS[1],
  "async function f() { await null; return arguments.length + ':' + [].slice.call(arguments).join(); } f(1, 2, 3).then(ok, bad); " + OBS[2]
);

// 5. Métodos async em classes e object literals: forma, nome, comprimento e comportamento.
const KINDS = {
  decl: ["async function f(a, b) {}", "f"],
  expr: ["var f = async function nm(a, b) {};", "f"],
  anon: ["var f = async function (a, b) {};", "f"],
  arrow: ["var f = async (a, b) => {};", "f"],
  objMethod: ["var o = { async m(a, b) {} }; var f = o.m;", "f"],
  objComputed: ["var k = 'ck'; var o = { async [k](a, b) {} }; var f = o.ck;", "f"],
  objSymbol: ["var s = Symbol('sym'); var o = { async [s](a, b) {} }; var f = o[s];", "f"],
  objGen: ["var o = { async *m(a, b) {} }; var f = o.m;", "f"],
  classMethod: ["class C { async m(a, b) {} } var f = C.prototype.m;", "f"],
  classStatic: ["class C { static async m(a, b) {} } var f = C.m;", "f"],
  classPrivate: ["class C { async #m(a, b) {} static get() { return new C().#m; } } var f = C.get();", "f"],
  classField: ["class C { f = async (a, b) => {}; } var f = new C().f;", "f"],
  classGen: ["class C { static async *m(a, b) {} } var f = C.m;", "f"],
  defaults: ["var f = async function (a, b = 1, c) {};", "f"],
  rest: ["var f = async (a, ...r) => {};", "f"],
};
const PROBES = [
  "f.name", "f.length", "'prototype' in f", "Object.prototype.toString.call(f)", "Object.getPrototypeOf(f) === Function.prototype",
  "Object.getPrototypeOf(f) === Object.getPrototypeOf(async function () {})", "Object.getPrototypeOf(f) === Object.getPrototypeOf(async function* () {})",
  "typeof f.prototype", "f.hasOwnProperty('caller')", "Object.getOwnPropertyNames(f).join()", "f.constructor.name", "String(f).slice(0, 40)",
  "Reflect.ownKeys(f).length", "f instanceof Function", "typeof f.call", "(() => { try { return V(new f()); } catch (e) { return E(e); } })()",
];
for (const [kind, [setup, name]] of Object.entries(KINDS)) {
  for (const probe of PROBES) add(`${setup} (async () => { try { L(V(${probe.replace(/\bf\b/g, name)})); } catch (e) { L('c:' + E(e)); } })(); ${obs()}`);
}
for (const [kind, [setup]] of Object.entries(KINDS)) {
  add(`${setup} var r; try { r = f(1, 2); } catch (e) { L('sync:' + E(e)); } L(V(r instanceof Promise)); L(V(r && Object.getPrototypeOf(r) === Promise.prototype)); ${obs()}`);
}
add(
  "class C { async m() { await null; return this.v; } constructor() { this.v = 7; } } var m = new C().m; m().then(ok, bad); " + OBS[0],
  "class C { async m() { return this; } } var c = new C(); c.m.call(5).then(r => L(typeof r)); " + OBS[1],
  "class C { async m() { 'use strict'; return typeof this; } } new C().m.call(5).then(ok, bad); " + OBS[2],
  "class C { async m() { return await this.n(); } async n() { await null; return 'n'; } } new C().m().then(ok, bad); " + OBS[0],
  "class C { static async s() { return this.name; } } var s = C.s; s().then(ok, bad); C.s().then(ok, bad); " + OBS[1],
  "class C { async *g() { yield this.v; yield await this.v; } constructor() { this.v = 1; } } (async () => { for await (var x of new C().g()) L(V(x)); })().then(ok, bad); " + OBS[2],
  "class C { #p = 3; async m() { await null; return this.#p; } } new C().m().then(ok, bad); " + OBS[0],
  "class C { static #p = 4; static async m() { await null; return C.#p; } } C.m().then(ok, bad); " + OBS[1],
  "class A { async m() { return 'A'; } } class B extends A { async m() { return (await super.m()) + 'B'; } } new B().m().then(ok, bad); " + OBS[2],
  "class A { async m() { return 'A'; } } class B extends A { async m() { var f = async () => super.m(); return (await f()) + 'B'; } } new B().m().then(ok, bad); " + OBS[0],
  "class A { static async m() { return 'sA'; } } class B extends A { static async m() { return (await super.m()) + 'sB'; } } B.m().then(ok, bad); " + OBS[1],
  "var o = { async m() { return 'o'; } }; var p = { __proto__: o, async m() { return (await super.m()) + 'p'; } }; p.m().then(ok, bad); " + OBS[2],
  "var o = { async m() { await null; throw new Error('om'); } }; o.m().catch(e => L('c:' + e.message)); " + OBS[0],
  "var o = { async m() { throw new Error('sync-throw'); } }; var p = o.m(); L(V(p instanceof Promise)); p.catch(e => L('c:' + e.message)); " + OBS[1],
  "var o = { async m({ a }) { return a; } }; var p = o.m(null); p.then(ok, bad); " + OBS[2],
  "var o = { async m(a = (() => { throw new Error('pd'); })()) { return a; } }; var p = o.m(); p.then(ok, bad); " + OBS[0],
  "var o = { async m(a = await null) { return a; } }",
  "var o = { async m() { var await = 1; } }",
  "class C { async constructor() {} }",
  "class C { async get x() {} }",
  "var o = { async get x() {} }",
  "async function f() { function g() { await 1; } }",
  "async function f(a = await 1) {}",
  "async function f() { var o = { m() { return await 1; } }; }",
  "async () => { await: 1 }",
  "async function f() { for await (var x in []) {} }",
  "function f() { for await (var x of []) {} }",
  "async function f() { for await (var x = 1 of []) {} }",
  "async function f() { await using x = 1, y = 2; }",
  "var async = 1; var r = async; L(V(r));"
);

// 6. await using e AsyncDisposableStack.
const RES = [
  "{ [Symbol.asyncDispose]() { L('ad'); } }",
  "{ async [Symbol.asyncDispose]() { L('ad.in'); await null; L('ad.out'); } }",
  "{ [Symbol.dispose]() { L('sd'); } }",
  "{ [Symbol.asyncDispose]() { L('ad'); }, [Symbol.dispose]() { L('sd'); } }",
  "{ [Symbol.asyncDispose]() { throw new Error('ad-throw'); } }",
  "{ [Symbol.dispose]() { throw new Error('sd-throw'); } }",
  "{ [Symbol.asyncDispose]() { return Promise.reject(new Error('ad-rej')); } }",
  "{ async [Symbol.asyncDispose]() { await null; throw new Error('ad-late'); } }",
  "null", "undefined", "1", "{}", "{ [Symbol.asyncDispose]: 1 }", "{ [Symbol.asyncDispose]: null, [Symbol.dispose]() { L('sd.fallback'); } }",
  "{ [Symbol.asyncDispose]() { return thenable(1, 'd'); } }", "{ get [Symbol.asyncDispose]() { L('get.ad'); return () => L('ad.get'); } }",
  "{ get [Symbol.asyncDispose]() { throw new Error('get-throw'); } }", "{ [Symbol.dispose]() { return Promise.resolve(L('sd.promise')); } }",
];
const USING_CTX = [
  (r) => `{ await using x = ${r}; L('body'); } L('after');`,
  (r) => `{ await using x = ${r}; L('body'); throw new Error('body-throw'); }`,
  (r) => `{ await using x = ${r}; await null; L('body2'); } L('after');`,
  (r) => `async function f() { await using x = ${r}; L('in'); return 'ret'; } L(V(await f())); L('after');`,
  (r) => `for (var i = 0; i < 2; i++) { await using x = ${r}; L('it' + i); if (i === 0) continue; break; } L('after');`,
  (r) => `for (await using x of [${r}, ${r}]) { L('forof'); } L('after');`,
  (r) => `{ using x = ${r}; L('using'); } L('after');`,
  (r) => `{ await using a = ${r}; await using b = { [Symbol.asyncDispose]() { L('b.dispose'); } }; L('two'); } L('after');`,
  (r) => `{ await using a = { [Symbol.asyncDispose]() { L('a.dispose'); } }; await using b = ${r}; L('two'); throw new Error('t'); }`,
  (r) => `L(V(await (async () => { await using x = ${r}; return await Promise.resolve('inner'); })())); L('after');`,
  (r) => `switch (1) { case 1: await using x = ${r}; L('sw'); } L('after');`,
];
for (const r of RES) USING_CTX.forEach((ctx) => add(wrap(ctx(r))));

const STACK = [
  "var s = new AsyncDisposableStack(); L(V(s.disposed)); await s.disposeAsync(); L(V(s.disposed)); await s.disposeAsync(); L('twice');",
  "var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { L('u1'); } }); s.use({ [Symbol.dispose]() { L('u2'); } }); await s.disposeAsync(); L('done');",
  "var s = new AsyncDisposableStack(); s.defer(async () => { await null; L('d1'); }); s.defer(() => L('d2')); await s.disposeAsync();",
  "var s = new AsyncDisposableStack(); s.adopt(5, async v => { L('adopt' + v); }); await s.disposeAsync();",
  "var s = new AsyncDisposableStack(); s.defer(() => { throw new Error('e1'); }); s.defer(() => { throw new Error('e2'); }); await s.disposeAsync();",
  "var s = new AsyncDisposableStack(); s.defer(() => { throw new Error('e1'); }); s.defer(() => L('ok')); await s.disposeAsync();",
  "var s = new AsyncDisposableStack(); s.defer(() => L('moved?')); var t = s.move(); L(V(s.disposed) + V(t.disposed)); await s.disposeAsync(); L('s-disposed'); await t.disposeAsync();",
  "var s = new AsyncDisposableStack(); await s.disposeAsync(); try { s.use({ [Symbol.asyncDispose]() {} }); } catch (e) { L('c:' + E(e)); }",
  "var s = new AsyncDisposableStack(); try { s.use(1); } catch (e) { L('c:' + E(e)); } L(V(s.use(null))); L(V(s.use(undefined)));",
  "var s = new AsyncDisposableStack(); try { s.use({}); } catch (e) { L('c:' + E(e)); }",
  "var s = new AsyncDisposableStack(); try { s.defer(1); } catch (e) { L('c:' + E(e)); } try { s.adopt(1, 2); } catch (e) { L('c:' + E(e)); }",
  "L(V(AsyncDisposableStack.prototype[Symbol.asyncDispose] === AsyncDisposableStack.prototype.disposeAsync)); L(V(Object.prototype.toString.call(new AsyncDisposableStack())));",
  "L(V(AsyncDisposableStack.name + AsyncDisposableStack.length)); try { AsyncDisposableStack(); } catch (e) { L('c:' + E(e)); }",
  "var s = new AsyncDisposableStack(); var p = s.disposeAsync(); L(V(p instanceof Promise)); L(V(s.disposed)); await p;",
  "var s = new AsyncDisposableStack(); var o = s.use({ [Symbol.asyncDispose]() { L('o'); }, id: 'ret' }); L(V(o.id)); L(V(s.adopt('v', () => {}))); L(V(s.defer(() => {}))); await s.disposeAsync();",
  "{ await using s = new AsyncDisposableStack(); s.defer(() => L('stack-defer')); L('body'); } L('after');",
  "var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { L('a'); return Promise.reject(new Error('ar')); } }); s.defer(() => { throw new Error('dt'); }); await s.disposeAsync();",
  "var ds = new DisposableStack(); ds.defer(() => L('ds1')); { await using s = new AsyncDisposableStack(); s.use(ds); L('b'); } L('after');",
  "var s = new AsyncDisposableStack(); s.use({ [Symbol.dispose]() { L('sync-in-async'); } }); var t = s.move(); await t[Symbol.asyncDispose](); L('end');",
  "var s = new AsyncDisposableStack(); s.defer(async () => { L('first.start'); await null; L('first.end'); }); s.defer(async () => { L('second.start'); await null; L('second.end'); }); await s.disposeAsync(); L('done');",
  "var s = new AsyncDisposableStack(); try { s.use({ [Symbol.asyncDispose]() { L('x'); } }); throw new Error('mid'); } catch (e) { L('c:' + e.message); await s.disposeAsync(); }",
  "var e1 = new SuppressedError(new Error('a'), new Error('b'), 'msg'); L(E(e1)); L(V(e1 instanceof Error)); L(V(Object.getOwnPropertyNames(e1).join()));",
  "var e1 = new SuppressedError(1, 2); L(E(e1)); L(V(e1.message)); L(V(e1.hasOwnProperty('message')));",
  "L(V(SuppressedError.length + SuppressedError.name)); L(V(Object.getPrototypeOf(SuppressedError) === Error)); L(V(SuppressedError.prototype.name));",
];
for (const body of STACK) add(wrap(body), wrap("await null; " + body));

// 7. Promise subclass com then customizado.
const SUBS = [
  "new SubP(r => r(7))", "SubP.resolve(8)", "SubP.reject(new Error('sr'))", "SubP.resolve(SubP.resolve(1))",
  "Object.assign(Promise.resolve(3), { constructor: SubP })", "Object.assign(SubP.resolve(4), { constructor: Promise })",
  "Object.assign(Promise.resolve(5), { constructor: Object })", "new SubP(r => r(Promise.resolve(6)))",
];
const SUBCTX = [
  (s) => `L(V(await ${s}));`,
  (s) => `async function f() { return ${s}; } L(V(await f()));`,
  (s) => `async function f() { return await ${s}; } L(V(await f()));`,
  (s) => `L(V(await Promise.all([${s}])));`,
  (s) => `L(V(await Promise.race([${s}, 1])));`,
  (s) => `L(V(await new Promise(r => r(${s}))));`,
  (s) => `L(V(await Promise.resolve(${s})));`,
  (s) => `for await (var x of [${s}]) L(V(x));`,
  (s) => `L(V(await ${s}.then(v => v)));`,
  (s) => `var p = ${s}; L(V(Promise.resolve(p) === p)); L(V(p.constructor === Promise)); await p;`,
];
for (const s of SUBS) SUBCTX.forEach((ctx) => add(wrap(ctx(s)), wrap("await null; " + ctx(s))));
add(
  "class P2 extends Promise { static get [Symbol.species]() { return Promise; } then(a, b) { L('p2.then'); return super.then(a, b); } } (async () => { L(V(await new P2(r => r(1)))); })().then(ok, bad); " + OBS[0],
  "class P2 extends Promise { constructor(ex) { L('ctor'); super(ex); } } (async () => { L(V(await P2.resolve(1))); })().then(ok, bad); " + OBS[1],
  "class P2 extends Promise { constructor(ex) { L('ctor'); super(ex); } } async function f() { return P2.resolve(2); } f().then(ok, bad); " + OBS[2],
  "var p = Promise.resolve(1); p.then = function (a, b) { L('inst.then'); return Promise.prototype.then.call(this, a, b); }; (async () => { L(V(await p)); })().then(ok, bad); " + OBS[0],
  "var p = Promise.resolve(1); p.then = function (a, b) { L('inst.then'); return Promise.prototype.then.call(this, a, b); }; async function f() { return p; } f().then(ok, bad); " + OBS[1],
  "var orig = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('proto.then'); return orig.call(this, a, b); }; (async () => { L(V(await Promise.resolve(1))); await null; L('x'); })().then(ok, bad); " + OBS[2],
  "var orig = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('proto.then'); return orig.call(this, a, b); }; async function f() { return 1; } f().then(ok, bad); " + OBS[0],
  "var orig = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('proto.then'); return orig.call(this, a, b); }; (async () => { for await (var x of [1, Promise.resolve(2)]) L(V(x)); })().then(ok, bad); " + OBS[1],
  "var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { L('get.ctor'); return Promise; } }); (async () => { L(V(await p)); })().then(ok, bad); " + OBS[2],
  "var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { L('get.ctor'); throw new Error('ctor-throw'); } }); (async () => { try { L(V(await p)); } catch (e) { L('c:' + E(e)); } })().then(ok, bad); " + OBS[0],
  "var orig = Promise.resolve; Promise.resolve = function (v) { L('P.resolve'); return orig.call(this, v); }; (async () => { L(V(await 1)); L(V(await Promise.resolve(2))); })().then(ok, bad); " + OBS[1],
  "var orig = Promise.resolve; Promise.resolve = function (v) { L('P.resolve'); return orig.call(this, v); }; (async () => { for await (var x of [1]) L(V(x)); })().then(ok, bad); " + OBS[2],
  "var saved = globalThis.Promise; globalThis.Promise = function () { L('replaced'); }; (async () => { L(V(await 1)); })().then(ok, bad); globalThis.Promise = saved; " + OBS[0]
);

// 8. Atores concorrentes (2 a 4) com awaits de valores não-promise, thenables e promises nativas.
let seed = 0x2f6e2b1;
const rnd = (n) => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return (((t ^ (t >>> 14)) >>> 0) % n);
};
const STEPS = [
  (id, k) => `await ${k};`,
  (id, k) => `await null;`,
  (id, k) => `await thenable(${k}, '${id}${k}');`,
  (id, k) => `await Promise.resolve(${k});`,
  (id, k) => `await SubP.resolve(${k});`,
  (id, k) => `try { await Promise.reject(new Error('e${k}')); } catch (e) { L('${id}:c'); }`,
  (id, k) => `await (async () => ${k})();`,
  (id, k) => `await { then(r) { L('${id}.th'); r(${k}); } };`,
  (id, k) => `await new Promise(r => r(${k}));`,
  (id, k) => `await Promise.all([${k}, Promise.resolve(${k})]);`,
];
const STARTS = [
  (id) => `${id}();`,
  (id) => `Promise.resolve().then(${id});`,
  (id) => `${id}().then(() => L('${id}:done'), bad);`,
  (id) => `Promise.resolve().then(() => ${id}());`,
];
for (let n = 0; n < 420; n++) {
  const actors = 2 + rnd(3);
  const ids = ["a", "b", "c", "d"].slice(0, actors);
  let source = "";
  for (const id of ids) {
    const steps = 1 + rnd(4);
    let body = `L('${id}:s');`;
    for (let s = 0; s < steps; s++) body += ` ${STEPS[rnd(STEPS.length)](id, s + 1)} L('${id}:${s}');`;
    source += `async function ${id}() { ${body} return '${id}'; } `;
  }
  let calls = "";
  for (const id of ids) calls += STARTS[rnd(STARTS.length)](id) + " ";
  add(source + calls + (rnd(2) ? OBS[rnd(OBS.length)] : "L('top');"));
}

process.stderr.write(`${programs.length} programas candidatos\n`);

// Executa cada programa no bun, num processo próprio, pela API vm (a fonte nunca é um arquivo do projeto).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "await-context-golden-"));
const driver = path.join(tmp, "driver.js");
fs.writeFileSync(
  driver,
  `const vm = require("node:vm");
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.on("unhandledRejection", () => {});
vm.runInThisContext(fs.readFileSync(process.argv[3], "utf8"), { filename: "harness" });
try { vm.runInThisContext(source, { filename: "program" }); } catch (e) { globalThis.__err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
setTimeout(() => { const out = globalThis.__final(); process.stdout.write(out); }, 0);
`
);
const harnessFile = path.join(tmp, "harness.txt");
fs.writeFileSync(harnessFile, HARNESS);

const runOne = (source, index) =>
  new Promise((resolve) => {
    const srcFile = path.join(tmp, `p${index}.txt`);
    fs.writeFileSync(srcFile, source);
    const child = spawn(process.execPath, [driver, srcFile, harnessFile], { cwd: tmp });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ source, out, err, code });
    });
  });

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  const worker = async () => {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runOne(programs[i], i);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  const lines = [];
  let failed = 0;
  results.forEach((r) => {
    if (r.code !== 0 || r.out === "") {
      failed++;
      process.stderr.write(`FALHA: ${r.source}\n${r.err}\n`);
      return;
    }
    lines.push(`${r.source}\t${r.out.replace(/[\t\n\r]+$/, "")}`);
  });
  fs.rmSync(tmp, { recursive: true, force: true });
  const output = lines.join("\n") + "\n";
  if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
  if (failed) throw new Error(`${failed} programas sem resultado do bun`);
  process.stdout.write(output);
  process.stderr.write(`${lines.length} programas\n`);
})();
