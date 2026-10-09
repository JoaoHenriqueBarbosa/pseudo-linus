// Gera tests/golden/async_order_bun.tsv: ordem de microtarefas e protocolos de iteração, medidos no bun 1.4.2.
// Cobre await de thenable e de promise nativa vs não nativa, Promise.all/allSettled/any/race com iteráveis
// customizados, Promise.withResolvers, Promise.try, resolução com thenable que lança, rejeição tardia, generators
// síncronos (return/throw em yield*, delegação a iterador sem return/throw, spread e destructuring, generator como
// método e computado), async functions com try/finally e return await, e a ordem de log de atores concorrentes no
// topo do programa, tudo capturado no array global `globalThis.R`.
// Cada programa roda por `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do
// bun não tocar na fonte) num processo bun filho próprio, depois do prelúdio ORDER_HARNESS, também via vm. Os
// programas não usam API de host (setTimeout, process, console, require, Bun, queueMicrotask): só L, tick, thenable,
// ok e bad. O golden é o JSON de R depois de esvaziar as microtarefas, ou `error<TAB>name<TAB>message JSON` se a
// fonte lançou de forma síncrona. O host do gerador drena as microtarefas com um setTimeout fora do programa.
// Programas já presentes em outros goldens de async são descartados. Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-async-order-golden.js > tests/golden/async_order_bun.tsv
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Mesmo texto embutido em tests/async_order_bun_golden.rs.
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

const root = path.join(__dirname, "..");
const existing = new Set();
for (const program of knownPrograms("async_order_bun.tsv", (name) => /(async|promise|microtask)/.test(name) && name !== "async_order_bun.tsv")) existing.add(program);
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const SEE = "ok, bad";
// Observadores: cadeias de then de tamanhos diferentes, para marcar em que rodada cada coisa acontece.
const OBS = {
  chain: "Promise.resolve().then(() => L('o1')).then(() => L('o2')).then(() => L('o3')).then(() => L('o4')).then(() => L('o5'));",
  asyncObs: "(async () => { await null; L('o1'); await null; L('o2'); await null; L('o3'); await null; L('o4'); })();",
};

// 1. await de cada tipo de valor, em cada contexto de uso, contra dois observadores.
const awaitables = {
  value: "5",
  nativeOk: "Promise.resolve(5)",
  nativeBad: "Promise.reject(new Error('r'))",
  nativePending: "new Promise(r => r(5))",
  nativeOwnThen: "(() => { var p = Promise.resolve(5); p.then = function (a, b) { L('own'); return Promise.prototype.then.call(this, a, b); }; return p; })()",
  nativeCtor: "(() => { var p = Promise.resolve(5); p.constructor = Object; return p; })()",
  subclass: "(() => { class P extends Promise {} return P.resolve(5); })()",
  thenSync: "thenable(5, 't')",
  thenAsync: "{ then(res) { L('then'); Promise.resolve().then(() => res(5)); } }",
  thenThrows: "{ then() { throw new Error('tt'); } }",
  thenGetterThrows: "{ get then() { throw new Error('tg'); } }",
  thenGetterCount: "{ get then() { L('get'); return function (res) { res(5); }; } }",
  thenNotFn: "{ then: 7 }",
  thenResolvesThenable: "{ then(res) { res(thenable(5, 'inner')); } }",
  thenBoth: "{ then(res, rej) { res(1); rej(new Error('late')); L('both'); } }",
  thenRejects: "{ then(res, rej) { rej(new Error('rj')); } }",
};
const contexts = {
  plain: x => `(async () => { var v = await ${x}; L('got:' + v); })().then(${SEE}); `,
  catcher: x => `(async () => { try { var v = await ${x}; L('got:' + v); } catch (e) { L('c:' + e.name); } })().then(${SEE}); `,
  ret: x => `(async () => { return ${x}; })().then(${SEE}); `,
  retAwait: x => `(async () => { return await ${x}; })().then(${SEE}); `,
  retCatch: x => `(async () => { try { return ${x}; } catch (e) { return 'c:' + e.name; } })().then(${SEE}); `,
  retAwaitCatch: x => `(async () => { try { return await ${x}; } catch (e) { return 'c:' + e.name; } })().then(${SEE}); `,
  agYield: x => `(async function* () { yield ${x}; })().next().then(${SEE}); `,
  resolveWith: x => `new Promise(r => r(${x})).then(${SEE}); `,
};
for (const [an, a] of Object.entries(awaitables)) {
  for (const [cn, c] of Object.entries(contexts)) {
    for (const [on, o] of Object.entries(OBS)) {
      add(c(a) + o + " tick(12, 't12');");
    }
  }
}

// 2. Combinadores com iteráveis customizados.
const combos = ["all", "allSettled", "any", "race"];
const iterables = {
  array: "[1, Promise.resolve(2), thenable(3, 'x')]",
  allRejected: "[Promise.reject(new Error('a')), Promise.reject(new Error('b'))]",
  mixed: "[Promise.reject(new Error('a')), Promise.resolve(2), 3]",
  empty: "[]",
  set: "new Set([1, Promise.resolve(2)])",
  string: "'ab'",
  generator: "(function* () { L('g1'); yield 1; L('g2'); yield Promise.resolve(2); L('g3'); })()",
  genThrows: "(function* () { yield 1; throw new Error('gt'); })()",
  nonIterable: "5",
  noSymbol: "{}",
  nullArg: "null",
  customLogged: "{ [Symbol.iterator]() { var i = 0; return { next() { L('next' + i); return i < 2 ? { value: i++, done: false } : { value: undefined, done: true }; }, return() { L('return'); return {}; } }; } }",
  nextThrows: "{ [Symbol.iterator]() { return { next() { L('next'); throw new Error('nt'); }, return() { L('return'); return {}; } }; } }",
  iteratorGetterThrows: "{ get [Symbol.iterator]() { throw new Error('ig'); } }",
  resultNotObject: "{ [Symbol.iterator]() { return { next() { return 1; }, return() { L('return'); return {}; } }; } }",
  doneGetterOrder: "{ [Symbol.iterator]() { var n = 0; return { next() { n++; return { get done() { L('done' + n); return n > 2; }, get value() { L('value' + n); return n; } }; } }; } }",
  sparse: "[, 1, , 2]",
  lateReject: "[new Promise((_, j) => j(new Error('late'))), Promise.resolve(1)]",
};
for (const c of combos) {
  for (const [n, it] of Object.entries(iterables)) {
    add(`Promise.${c}(${it}).then(${SEE}); ${OBS.chain} tick(14, 't14');`);
    add(`(async () => { try { ok(await Promise.${c}(${it})); } catch (e) { bad(e); if (e && e.errors) L('errors:' + e.errors.length); } })(); tick(14, 't14');`);
  }
}
// then espiado: quantas vezes o combinador chama then e em que ordem.
const spy = "var __t = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then#'); return __t.call(this, a, b); }; ";
const elems = {
  nums: "[1, 2]",
  natives: "[Promise.resolve(1), Promise.resolve(2)]",
  thens: "[thenable(1, 'a'), thenable(2, 'b')]",
  rej: "[Promise.reject(new Error('x')), Promise.resolve(1)]",
};
for (const c of combos) {
  for (const [n, e] of Object.entries(elems)) {
    add(`${spy}Promise.${c}(${e}).then(${SEE}); tick(10, 't10');`);
    // subclasse com resolve e then observáveis
    add(`class P extends Promise { static resolve(v) { L('res:' + v); return super.resolve(v); } } P.${c}(${e}).then(${SEE}); tick(10, 't10');`);
    add(`class P extends Promise { then(a, b) { L('then'); return super.then(a, b); } } P.${c}(${e}).then(${SEE}); tick(10, 't10');`);
    add(`var r = Promise.resolve; Promise.resolve = function (v) { L('res:' + v); return r.call(this, v); }; var q = Promise.${c}(${e}); Promise.resolve = r; q.then(${SEE}); tick(10, 't10');`);
    add(`var r = Promise.resolve; Promise.resolve = function () { throw new Error('nores'); }; var q = Promise.${c}(${e}); Promise.resolve = r; q.then(${SEE}); tick(10, 't10');`);
    add(`var r = Promise.resolve; Promise.resolve = 5; var q = Promise.${c}(${e}); Promise.resolve = r; q.then(${SEE}); tick(10, 't10');`);
    add(`var it = { [Symbol.iterator]() { var i = 0, a = ${e}; return { next() { L('n'); return i < a.length ? { value: a[i++], done: false } : { done: true }; }, return() { L('ret'); return {}; } }; } }; var r = Promise.resolve; Promise.resolve = function () { throw new Error('nores'); }; var q = Promise.${c}(it); Promise.resolve = r; q.then(${SEE}); tick(10, 't10');`);
    add(`var d = Promise.${c}.call(function (ex) { L('ctor'); ex(function () {}, function () {}); }, ${e}); L(typeof d);`);
    add(`try { Promise.${c}.call(undefined, ${e}); } catch (e) { L(e.name); } try { Promise.${c}.call({}, ${e}); } catch (e) { L(e.name); }`);
  }
}

// 3. Promise.withResolvers e Promise.try.
add(
  `var d = Promise.withResolvers(); d.resolve(1); d.resolve(2); d.reject(new Error('x')); d.promise.then(${SEE}); tick(3, 't3');`,
  `var d = Promise.withResolvers(); d.reject(new Error('x')); d.resolve(2); d.promise.then(${SEE}); tick(3, 't3');`,
  `var d = Promise.withResolvers(); d.resolve(thenable(7, 'w')); d.promise.then(${SEE}); ${OBS.chain} tick(8, 't8');`,
  `var d = Promise.withResolvers(); d.resolve(Promise.resolve(7)); d.promise.then(${SEE}); ${OBS.chain} tick(8, 't8');`,
  `var d = Promise.withResolvers(); d.resolve(d.promise); d.promise.then(${SEE}); tick(4, 't4');`,
  `var d = Promise.withResolvers(); d.reject(d.promise); d.promise.then(${SEE}); tick(4, 't4');`,
  `var d = Promise.withResolvers(); d.resolve({ then() { throw new Error('tt'); } }); d.promise.then(${SEE}); tick(5, 't5');`,
  `var d = Promise.withResolvers(); d.resolve({ then(r) { r(1); throw new Error('tt'); } }); d.promise.then(${SEE}); tick(5, 't5');`,
  `var d = Promise.withResolvers(); var { resolve } = d; resolve.call(null, 3); d.promise.then(${SEE}); tick(3, 't3');`,
  `var d = Promise.withResolvers(); L(Object.keys(d).join()); L(String(d.promise instanceof Promise)); L(String(Object.getPrototypeOf(d) === Object.prototype));`,
  `var { promise, resolve, reject } = Promise.withResolvers(); resolve(1); promise.then(${SEE}); L('sync'); tick(3, 't3');`,
  `class P extends Promise {} var d = P.withResolvers(); L(String(d.promise instanceof P)); d.resolve(1); d.promise.then(${SEE}); tick(3, 't3');`,
  `try { Promise.withResolvers.call(undefined); } catch (e) { L(e.name); } try { Promise.withResolvers.call({}); } catch (e) { L(e.name); } try { Promise.withResolvers.call(1); } catch (e) { L(e.name); }`,
  `var d = Promise.withResolvers.call(function (ex) { L('ctor'); ex(function (v) { L('res:' + v); }, function (v) { L('rej:' + v); }); }); d.resolve(4); d.reject(5); L(typeof d.promise);`,
  `var d = Promise.withResolvers.call(function (ex) { ex(undefined, undefined); }); L(typeof d.resolve);`,
  `try { Promise.withResolvers.call(function (ex) { ex(function () {}, function () {}); ex(function () {}, function () {}); }); L('ok'); } catch (e) { L(e.name); }`,
  `var seen = []; var d1 = Promise.withResolvers(), d2 = Promise.withResolvers(); d2.promise.then(() => L('p2')); d1.promise.then(() => L('p1')); d2.resolve(); d1.resolve(); tick(3, 't3');`,
  `var d = Promise.withResolvers(); (async () => { L('before'); var v = await d.promise; L('after:' + v); })(); L('sync'); d.resolve(9); L('resolved'); tick(4, 't4');`,
  `var d = Promise.withResolvers(); d.promise.finally(() => L('fin')).then(${SEE}); d.reject(new Error('e')); tick(6, 't6');`,
  `var d = Promise.withResolvers(); d.reject(new Error('late')); tick(2, 't2'); Promise.resolve().then(() => Promise.resolve()).then(() => d.promise.catch(bad)); tick(8, 't8');`
);
const tryArgs = {
  ret: "() => 1",
  retPromise: "() => Promise.resolve(2)",
  retRej: "() => Promise.reject(new Error('r'))",
  throws: "() => { throw new Error('t'); }",
  retThenable: "() => thenable(3, 'tr')",
  async: "async () => { L('in'); await null; return 4; }",
  asyncThrows: "async () => { throw new Error('at'); }",
  args: "(a, b) => a + b, 1, 2",
  noArgs: "function () { return arguments.length; }",
  thisCheck: "function () { return this === undefined; }",
  thisSloppy: "function () { return typeof this; }",
  nonFn: "5",
  undef: "undefined",
  generatorFn: "function* () { yield 1; }",
};
for (const [n, a] of Object.entries(tryArgs)) {
  add(
    `Promise.try(${a}).then(${SEE}); L('sync'); ${OBS.chain} tick(10, 't10');`,
    `(async () => { try { ok(await Promise.try(${a})); } catch (e) { bad(e); } })(); L('sync'); tick(10, 't10');`,
    `class P extends Promise {} var p = P.try(${a}); L(String(p instanceof P)); p.then(${SEE}); tick(8, 't8');`,
    `var p = Promise.try.call(function (ex) { L('ctor'); ex(function (v) { L('res:' + (v && v.name || v)); }, function (v) { L('rej:' + (v && v.name || v)); }); }, ${a}); L(typeof p);`
  );
}
add(
  `try { Promise.try.call(undefined, () => 1); } catch (e) { L(e.name); } try { Promise.try.call(1, () => 1); } catch (e) { L(e.name); }`,
  `var d = Promise.try(() => L('ran')); L('after'); d.then(() => L('then')); tick(3, 't3');`,
  `Promise.try(() => { L('a'); return Promise.try(() => { L('b'); return 1; }); }).then(${SEE}); L('c'); tick(6, 't6');`,
  `L(Promise.try.length + '|' + Promise.try.name);`,
  `var p = Promise.resolve(1); L(String(Promise.try(() => p) === p)); tick(3, 't3');`,
  `var p = Promise.resolve(1); L(String(Promise.resolve(p) === p));`
);

// 4. Resolução com thenable que lança, resolve duplo, auto-resolução e rejeição tardia.
const thens = {
  throwBefore: "{ then(r) { throw new Error('tb'); } }",
  throwAfterRes: "{ then(r) { r(1); throw new Error('ta'); } }",
  throwAfterRej: "{ then(r, j) { j(new Error('rj')); throw new Error('ta'); } }",
  resTwice: "{ then(r) { r(1); r(2); } }",
  resRej: "{ then(r, j) { r(1); j(new Error('late')); } }",
  rejRes: "{ then(r, j) { j(new Error('first')); r(1); } }",
  deferred: "{ then(r) { Promise.resolve().then(() => r(1)); } }",
  deferredTwice: "{ then(r) { Promise.resolve().then(() => { r(1); r(2); }); } }",
  never: "{ then() { L('then'); } }",
  nested: "{ then(r) { r({ then(r2) { r2('deep'); } }); } }",
  nestedThrows: "{ then(r) { r({ then() { throw new Error('nt'); } }); } }",
  getterOnce: "{ get then() { L('get'); return function (r) { r(1); }; } }",
  thisCheck: "{ tag: 'T', then(r) { r(this.tag); } }",
  retVal: "{ then(r) { r(1); return Promise.reject(new Error('ignored')); } }",
  nativeInside: "{ then(r) { r(Promise.resolve('n')); } }",
  rejInside: "{ then(r) { r(Promise.reject(new Error('ri'))); } }",
};
for (const [n, t] of Object.entries(thens)) {
  add(
    `new Promise(r => r(${t})).then(${SEE}); ${OBS.chain} tick(14, 't14');`,
    `Promise.resolve(${t}).then(${SEE}); ${OBS.chain} tick(14, 't14');`,
    `Promise.resolve().then(() => (${t})).then(${SEE}); ${OBS.chain} tick(14, 't14');`,
    `Promise.reject(${t}).then(${SEE}); tick(6, 't6');`,
    `Promise.resolve(1).finally(() => (${t})).then(${SEE}); ${OBS.chain} tick(14, 't14');`,
    `(async () => { return ${t}; })().then(${SEE}); ${OBS.chain} tick(14, 't14');`
  );
}
add(
  `var p = new Promise(r => r(p2)); var p2 = Promise.resolve(1); p.then(${SEE}); tick(5, 't5');`,
  `var p = Promise.resolve().then(() => p); p.then(${SEE}); tick(6, 't6');`,
  `var r1; var p = new Promise(r => { r1 = r; }); r1(p); p.then(${SEE}); tick(5, 't5');`,
  `var p = Promise.reject(new Error('late')); tick(3, 't3'); Promise.resolve().then(() => Promise.resolve()).then(() => Promise.resolve()).then(() => p.catch(bad)); tick(10, 't10');`,
  `var p = Promise.reject(new Error('x')); var q = p.then(() => L('never')); q.catch(bad); p.catch(bad); tick(6, 't6');`,
  `var p = Promise.reject(new Error('x')); p.then(() => L('never')).catch(bad); tick(6, 't6');`,
  `new Promise((_, j) => j(new Error('x'))).finally(() => L('fin')).catch(bad); tick(6, 't6');`,
  `new Promise((r, j) => { r(1); j(new Error('late')); throw new Error('thrown'); }).then(${SEE}); tick(4, 't4');`,
  `new Promise((r, j) => { throw new Error('thrown'); r(1); }).then(${SEE}); tick(4, 't4');`,
  `new Promise((r, j) => { j(new Error('first')); throw new Error('thrown'); }).then(${SEE}); tick(4, 't4');`,
  `Promise.resolve(1).then(() => { throw new Error('a'); }).catch(e => { L('c1:' + e.message); throw new Error('b'); }).catch(e => L('c2:' + e.message)).then(() => L('end')); tick(8, 't8');`,
  `Promise.resolve(1).then(2).then(${SEE}); Promise.reject(new Error('r')).then(1).then(null, ${SEE}); tick(6, 't6');`,
  `Promise.resolve(1).then(() => Promise.reject(new Error('inner'))).catch(${SEE}); ${OBS.chain} tick(10, 't10');`,
  `Promise.resolve(1).then(() => Promise.resolve(2)).then(${SEE}); ${OBS.chain} tick(10, 't10');`,
  `Promise.resolve(1).then(() => ({ then(r) { r(2); } })).then(${SEE}); ${OBS.chain} tick(10, 't10');`
);

// 5. Generators síncronos: yield* com iteradores internos variados, nos três métodos, parado ou não.
const inners = {
  full: "{ [Symbol.iterator]() { return { next(v) { L('in.next:' + v); return { value: 'n', done: false }; }, return(v) { L('in.return:' + v); return { value: 'r', done: true }; }, throw(v) { L('in.throw:' + v); return { value: 't', done: false }; } }; } }",
  noReturn: "{ [Symbol.iterator]() { return { next(v) { L('in.next:' + v); return { value: 'n', done: false }; } }; } }",
  noThrow: "{ [Symbol.iterator]() { return { next(v) { L('in.next:' + v); return { value: 'n', done: false }; }, return(v) { L('in.return:' + v); return { value: 'r', done: true }; } }; } }",
  noBoth: "{ [Symbol.iterator]() { return { next(v) { L('in.next:' + v); return { value: 'n', done: false }; } }; } }",
  returnNotObj: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return() { L('in.return'); return 1; } }; } }",
  returnNotDone: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return(v) { L('in.return:' + v); return { value: 'rv', done: false }; } }; } }",
  returnThrows: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return() { L('in.return'); throw new Error('rt'); } }; } }",
  throwDone: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, throw(v) { L('in.throw:' + v); return { value: 'td', done: true }; }, return() { L('in.return'); return {}; } }; } }",
  throwNotObj: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, throw() { return 1; }, return() { L('in.return'); return {}; } }; } }",
  throwThrows: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, throw() { throw new Error('tt'); }, return() { L('in.return'); return {}; } }; } }",
  throwNullReturn: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, throw: null, return(v) { L('in.return:' + v); return { done: true }; } }; } }",
  returnNull: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return: null }; } }",
  returnUndef: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return: undefined, throw: undefined }; } }",
  returnNotFn: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, return: 5 }; } }",
  throwNotFn: "{ [Symbol.iterator]() { return { next() { return { value: 'n', done: false }; }, throw: 5, return() { L('in.return'); return {}; } }; } }",
  getters: "{ [Symbol.iterator]() { return { get next() { L('get next'); return function () { return { get done() { L('get done'); return false; }, get value() { L('get value'); return 1; } }; }; }, get return() { L('get return'); return function () { return { done: true }; }; } }; } }",
  generator: "(function* () { try { yield 'i1'; yield 'i2'; } finally { L('inner fin'); } })()",
  generatorRet: "(function* () { try { yield 'i1'; } finally { L('inner fin'); return 'iret'; } })()",
  generatorCatch: "(function* () { try { yield 'i1'; } catch (e) { L('inner catch:' + e); yield 'recov'; } return 'idone'; })()",
  array: "['a', 'b']",
  string: "'xy'",
  empty: "[]",
};
const outers = {
  plain: "var g1 = (function* () { var r = yield* INNER; L('r=' + r); return 'end'; })();",
  fin: "var g1 = (function* () { try { var r = yield* INNER; L('r=' + r); } finally { L('outer fin'); } return 'end'; })();",
  catcher: "var g1 = (function* () { try { var r = yield* INNER; L('r=' + r); } catch (e) { L('outer catch:' + e); } return 'end'; })();",
};
const drive = {
  nextNext: "L(JSON.stringify(g1.next('a'))); L(JSON.stringify(g1.next('b')));",
  retMid: "L(JSON.stringify(g1.next('a'))); L(JSON.stringify(g1.return('R'))); L(JSON.stringify(g1.next('c')));",
  thrMid: "L(JSON.stringify(g1.next('a'))); L(JSON.stringify(g1.throw('T'))); L(JSON.stringify(g1.next('c')));",
  retStart: "L(JSON.stringify(g1.return('R'))); L(JSON.stringify(g1.next('c')));",
  thrStart: "L(JSON.stringify(g1.throw('T'))); L(JSON.stringify(g1.next('c')));",
  drain: "for (var i = 0; i < 4; i++) L(JSON.stringify(g1.next(i)));",
};
for (const [inn, inner] of Object.entries(inners)) {
  for (const [on, outer] of Object.entries(outers)) {
    for (const [dn, d] of Object.entries(drive)) {
      // Para reduzir o total mantendo cobertura: só 'plain' com todos os drives, os demais com 3.
      if (on !== "plain" && !["retMid", "thrMid", "drain"].includes(dn)) continue;
      add(`${outer.replace("INNER", inner)} try { ${d} } catch (e) { L('c:' + e.name + ':' + e.message); } L('end');`);
    }
  }
}

// 6. Spread, destructuring, for-of, Array.from e outros consumidores de generator.
const gens = {
  logged: "function* g() { try { L('s'); yield 1; L('m1'); yield 2; L('m2'); yield 3; L('e'); } finally { L('fin'); } }",
  retVal: "function* g() { yield 1; yield 2; return 'ret'; }",
  throws: "function* g() { yield 1; throw new Error('gt'); }",
  inf: "function* g() { var i = 0; try { while (true) yield i++; } finally { L('closed'); } }",
  empty: "function* g() { }",
  echo: "function* g() { var x = yield 1; L('x=' + x); var y = yield 2; L('y=' + y); }",
};
const consumers = {
  spreadArr: "L(JSON.stringify([...g()]));",
  spreadCall: "L(JSON.stringify(Math.max(...g())));",
  spreadNew: "L(new Array(...g()).length);",
  spreadObj: "L(JSON.stringify(Object.assign({}, [...g()])));",
  destrFull: "var [a, b, c] = g(); L(JSON.stringify([a, b, c]));",
  destrPartial: "var [a] = g(); L(JSON.stringify(a));",
  destrHoles: "var [, , c] = g(); L(JSON.stringify(c));",
  destrRest: "var [a, ...r] = g(); L(JSON.stringify([a, r]));",
  destrDefault: "var [a = 'd', b = 'e', c = 'f', d = 'g'] = g(); L(JSON.stringify([a, b, c, d]));",
  destrEmpty: "var [] = g(); L('done');",
  destrAssign: "var a, b; [a, b] = g(); L(JSON.stringify([a, b]));",
  destrNested: "var [[x] = [9], y] = g(); L(JSON.stringify([x, y]));",
  destrDefaultThrows: "try { var [a = (() => { throw new Error('dd'); })()] = (function* () { try { yield undefined; } finally { L('closed'); } })(); } catch (e) { L('c:' + e.message); }",
  forOf: "for (var x of g()) L('x' + x);",
  forOfBreak: "for (var x of g()) { L('x' + x); break; }",
  forOfThrow: "try { for (var x of g()) { L('x' + x); throw new Error('body'); } } catch (e) { L('c:' + e.message); }",
  forOfLabel: "o: for (var i = 0; i < 2; i++) { for (var x of g()) { L('x' + x); continue o; } }",
  forOfReturn: "(function () { for (var x of g()) { L('x' + x); return; } })();",
  arrayFrom: "L(JSON.stringify(Array.from(g())));",
  arrayFromMap: "L(JSON.stringify(Array.from(g(), x => x * 2)));",
  setCtor: "L(JSON.stringify([...new Set(g())]));",
  mapCtorBad: "try { new Map(g()); } catch (e) { L(e.name); }",
  promiseAll: "Promise.all(g()).then(ok, bad);",
  objectFromEntries: "try { L(JSON.stringify(Object.fromEntries(g()))); } catch (e) { L(e.name); }",
  nextManual: "var it = g(); L(JSON.stringify(it.next('a'))); L(JSON.stringify(it.next('b'))); L(JSON.stringify(it.next('c'))); L(JSON.stringify(it.next('d')));",
  yieldStarMine: "function* h() { var r = yield* g(); L('r=' + r); } L(JSON.stringify([...h()]));",
};
for (const [gn, g] of Object.entries(gens)) {
  for (const [cn, c] of Object.entries(consumers)) {
    // gerador infinito só com consumidores que fecham ou limitam o consumo
    if (gn === "inf" && !/Partial|Holes|Break|Throw|Return|Label|destrEmpty|destrDefault|nextManual|destrNested/.test(cn)) continue;
    add(`${g} try { ${c.replace(/g\(\)/g, "g()")} } catch (e) { L('c:' + e.name + ':' + e.message); } tick(8, 't8');`);
  }
}

// 7. Generator como método, computado, estático, em classe e em objeto.
add(
  `var o = { *g() { yield this === o; } }; L(JSON.stringify([...o.g()]));`,
  `var k = 'dyn'; var o = { *[k]() { yield 1; yield 2; } }; L(JSON.stringify([...o.dyn()])); L(o.dyn.name);`,
  `var s = Symbol('sym'); var o = { *[s]() { yield 1; } }; L(o[s].name); L(JSON.stringify([...o[s]()]));`,
  `var o = { *[Symbol.iterator]() { yield 'a'; yield 'b'; } }; L(JSON.stringify([...o])); var [x, y] = o; L(x + y);`,
  `class C { *[Symbol.iterator]() { yield 1; yield 2; } } L(JSON.stringify([...new C()]));`,
  `class C { static *s() { yield 'st'; } *m() { yield this.constructor.name; } } L(JSON.stringify([...C.s(), ...new C().m()]));`,
  `class C { static *['a' + 'b']() { yield 1; } } L(JSON.stringify([...C.ab()])); L(C.ab.name);`,
  `class C { *#p() { yield 'priv'; } run() { return [...this.#p()]; } } L(JSON.stringify(new C().run()));`,
  `class C { static *#p() { yield 'sp'; } static run() { return [...C.#p()]; } } L(JSON.stringify(C.run()));`,
  `var o = { *g() { } }; try { new o.g(); } catch (e) { L(e.name); }`,
  `function* g() {} try { new g(); } catch (e) { L(e.name); }`,
  `class C { *m() { } } try { new (new C().m)(); } catch (e) { L(e.name); }`,
  `var o = { *g() { yield arguments.length; } }; L(JSON.stringify([...o.g(1, 2, 3)]));`,
  `var o = { v: 3, *g() { var self = () => this.v; yield self(); } }; L(JSON.stringify([...o.g()]));`,
  `function* g() { yield this; } L(JSON.stringify(typeof g().next().value)); L(String(g.call(1).next().value === 1)); L(String(typeof g.call(1).next().value));`,
  `function* g() { 'use strict'; yield this; } L(String(g.call(1).next().value === 1)); L(String(g().next().value === undefined));`,
  `function* g() {} L(String(Object.getPrototypeOf(g()) === g.prototype)); L(String(Object.getPrototypeOf(g.prototype) === Object.getPrototypeOf(function* () {}).prototype));`,
  `function* g() {} g.prototype = null; L(String(Object.getPrototypeOf(g()) === Object.getPrototypeOf(function* () {}).prototype));`,
  `function* g() {} g.prototype = 5; L(String(Object.getPrototypeOf(g()) === Object.getPrototypeOf(function* () {}).prototype));`,
  `function* g() {} var p = { next() { return { done: true }; } }; g.prototype = p; var it = g(); L(String(Object.getPrototypeOf(it) === p)); L(JSON.stringify(it.next === p.next));`,
  `function* g() {} L(Object.prototype.toString.call(g())); L(Object.prototype.toString.call(g)); L(String(g()[Symbol.iterator]() !== undefined));`,
  `var GP = Object.getPrototypeOf(function* () {}).prototype; L(Object.getOwnPropertyNames(GP).join()); L(GP[Symbol.toStringTag]);`,
  `function* g() { yield 1; } var it = g(); L(String(it[Symbol.iterator]() === it)); L(typeof it.next + typeof it.return + typeof it.throw);`,
  `function* g() { var x = yield; L(String(x)); x = yield; L(String(x)); } var it = g(); it.next(1); it.next(2); it.next(3);`,
  `function* g() { yield yield 1; } var it = g(); L(JSON.stringify([it.next('a'), it.next('b'), it.next('c'), it.next('d')]));`,
  `function* g() { var a = yield 1, b = yield 2; return a + b; } var it = g(); it.next(); it.next(10); L(JSON.stringify(it.next(20)));`,
  `function* g() { return yield* h(); } function* h() { yield 1; return 'hret'; } L(JSON.stringify([...g()])); var it = g(); it.next(); L(JSON.stringify(it.next()));`,
  `function* g() { try { yield 1; } finally { return 'fin'; } } var it = g(); it.next(); L(JSON.stringify(it.return('r'))); L(JSON.stringify(it.next()));`,
  `function* g() { try { yield 1; } finally { yield 'in-fin'; } } var it = g(); it.next(); L(JSON.stringify(it.return('r'))); L(JSON.stringify(it.next())); L(JSON.stringify(it.next()));`,
  `function* g() { try { yield 1; } finally { throw new Error('fe'); } } var it = g(); it.next(); try { it.return('r'); } catch (e) { L(e.message); } L(JSON.stringify(it.next()));`,
  `function* g() { yield 1; } var it = g(); L(JSON.stringify(it.return(5))); L(JSON.stringify(it.next())); try { it.throw(new Error('x')); } catch (e) { L(e.message); }`,
  `function* g() { yield 1; } var it = g(); try { it.throw(new Error('first')); } catch (e) { L(e.message); } L(JSON.stringify(it.next()));`,
  `function* g() { yield* g2(); } function* g2() { yield 1; throw new Error('deep'); } var it = g(); it.next(); try { it.next(); } catch (e) { L(e.message); } L(JSON.stringify(it.next()));`,
  `function* g() { try { yield* g2(); } catch (e) { L('outer:' + e.message); yield 'after'; } } function* g2() { try { yield 1; } finally { L('g2 fin'); } } var it = g(); it.next(); L(JSON.stringify(it.throw(new Error('t')))); L(JSON.stringify(it.next()));`,
  `function* g() { yield* g2(); } function* g2() { try { yield 1; } catch (e) { L('g2 caught:' + e.message); return 'g2r'; } } var it = g(); it.next(); L(JSON.stringify(it.throw(new Error('t'))));`,
  `function* g() { var r = yield* g2(); L('r=' + r); yield 'x'; } function* g2() { try { yield 1; } finally { L('g2 fin'); } return 'g2r'; } var it = g(); it.next(); L(JSON.stringify(it.return('R'))); L(JSON.stringify(it.next()));`,
  `function* g() { yield* 5; } try { g().next(); } catch (e) { L(e.name); }`,
  `function* g() { yield* undefined; } try { g().next(); } catch (e) { L(e.name); }`,
  `function* g() { yield* { [Symbol.iterator]: null }; } try { g().next(); } catch (e) { L(e.name); }`,
  `function* g() { yield* { [Symbol.iterator]() { return 1; } }; } try { g().next(); } catch (e) { L(e.name); }`,
  `function* g() { yield* { [Symbol.iterator]() { return {}; } }; } try { g().next(); } catch (e) { L(e.name); }`,
  `function* g() { yield* { [Symbol.iterator]() { return { next: 1 }; } }; } try { g().next(); } catch (e) { L(e.name); }`
);

// 8. async functions com try/finally e return await.
const results = {
  value: "'v'",
  nativeOk: "Promise.resolve('p')",
  nativeBad: "Promise.reject(new Error('pr'))",
  thenable: "thenable('t', 'tt')",
  throwing: "(() => { throw new Error('sync'); })()",
};
const shapes = {
  retFin: x => `async function f() { try { return ${x}; } finally { L('fin'); } }`,
  retAwaitFin: x => `async function f() { try { return await ${x}; } finally { L('fin'); } }`,
  retFinAwait: x => `async function f() { try { return ${x}; } finally { await null; L('fin'); } }`,
  retAwaitFinAwait: x => `async function f() { try { return await ${x}; } finally { await null; L('fin'); } }`,
  retCatchFin: x => `async function f() { try { return ${x}; } catch (e) { L('c'); return 'caught'; } finally { L('fin'); } }`,
  retAwaitCatchFin: x => `async function f() { try { return await ${x}; } catch (e) { L('c'); return 'caught'; } finally { L('fin'); } }`,
  finOverride: x => `async function f() { try { return ${x}; } finally { return 'over'; } }`,
  finOverrideAwait: x => `async function f() { try { return await ${x}; } finally { await null; return 'over'; } }`,
  finThrow: x => `async function f() { try { return ${x}; } finally { throw new Error('ft'); } }`,
  finRetPromise: x => `async function f() { try { return ${x}; } finally { return Promise.resolve('fp'); } }`,
  awaitInFinal: x => `async function f() { try { await null; } finally { await ${x}; L('fin'); } }`,
  awaitInCatch: x => `async function f() { try { throw 1; } catch (e) { await ${x}; L('c'); } finally { L('fin'); } }`,
  loopFin: x => `async function f() { for (var i = 0; i < 2; i++) { try { if (i === 0) continue; return await ${x}; } finally { L('fin' + i); await null; } } }`,
  nestedFin: x => `async function f() { try { try { return await ${x}; } finally { L('in'); await null; L('in2'); } } finally { L('out'); } }`,
};
for (const [sn, s] of Object.entries(shapes)) {
  for (const [rn, r] of Object.entries(results)) {
    add(`${s(r)} f().then(${SEE}); ${OBS.chain} tick(14, 't14');`);
    add(`${s(r)} (async () => { try { ok(await f()); } catch (e) { bad(e); } finally { L('outer fin'); } })(); ${OBS.asyncObs} tick(14, 't14');`);
  }
}

// 9. Atores concorrentes: pares e triplas, só com L e as filas de microtarefa, ordem de log em R.
const actors = [
  "(async () => { await null; L('Q1'); await null; L('Q2'); })();",
  "Promise.resolve().then(() => L('Q1')).then(() => L('Q2'));",
  "(async () => { await Promise.resolve(); L('Q1'); })();",
  "(async () => { return Promise.resolve(1); })().then(() => L('Q1'));",
  "Promise.resolve(thenable(1, 'Q')).then(() => L('Q1'));",
  "new Promise(r => r(Promise.resolve())).then(() => L('Q1'));",
  "(async () => { await thenable(1, 'Q'); L('Q1'); })();",
  "Promise.reject(new Error('x')).catch(() => L('Q1')).finally(() => L('Q2'));",
  "Promise.all([1, Promise.resolve(2)]).then(() => L('Q1'));",
  "Promise.race([Promise.resolve(1)]).then(() => L('Q1'));",
  "(async function* () { yield 1; })().next().then(() => L('Q1'));",
  "(async () => { try { await Promise.reject(1); } catch (e) { L('Q1'); } finally { L('Q2'); } })();",
  "Promise.allSettled([Promise.reject(1)]).then(() => L('Q1'));",
  "Promise.any([Promise.reject(1), 2]).then(() => L('Q1'));",
  "Promise.resolve().finally(() => L('Q1')).then(() => L('Q2'));",
  "L('Q1');",
  "(async () => { for await (var x of [1, 2]) L('Q' + x); })();",
  "(async () => { var d = Promise.withResolvers(); d.resolve(1); await d.promise; L('Q1'); })();",
  "Promise.try(() => 1).then(() => L('Q1'));",
  "Promise.try(async () => 1).then(() => L('Q1'));",
  "(async () => { await (async () => { await null; })(); L('Q1'); })();",
  "(async () => { await { then(r) { r(1); } }; L('Q1'); })();",
];
const label = (code, name) => code.replace(/Q/g, name);
for (let i = 0; i < actors.length; i++) {
  for (let j = 0; j < actors.length; j++) {
    if (i === j) continue;
    add(`${label(actors[i], "A")} ${label(actors[j], "B")} L('sync'); tick(10, 't10');`);
  }
}
// Tríplas: amostra determinística com passo coprimo, para cobrir combinações variadas.
let count = 0;
const N = actors.length;
for (let n = 0; n < N * (N - 1) * (N - 2) && count < 220; n += 17) {
  const i = n % N;
  const j = Math.floor(n / N) % N;
  const k = (Math.floor(n / (N * N)) + n) % N;
  if (i === j || j === k || i === k) continue;
  const before = programs.length;
  add(`${label(actors[i], "A")} ${label(actors[j], "B")} ${label(actors[k], "C")} L('sync'); tick(12, 't12');`);
  if (programs.length > before) count++;
}

// 10. Ordem de unhandled e rejeição tardia, sem API de host: o que se observa é só a ordem do log.
add(
  `var p = Promise.reject(new Error('u')); L('sync'); tick(3, 't3');`,
  `var p = Promise.reject(new Error('u')); Promise.resolve().then(() => p.catch(bad)); tick(5, 't5');`,
  `var p = Promise.reject(new Error('u')); p.then(() => {}); L('sync'); tick(4, 't4');`,
  `var p = Promise.reject(new Error('u')); var q = p.finally(() => L('fin')); L('sync'); tick(5, 't5');`,
  `(async () => { throw new Error('u'); })(); L('sync'); tick(3, 't3');`,
  `(async () => { await Promise.reject(new Error('u')); })().catch(bad); tick(5, 't5');`,
  `Promise.all([Promise.reject(new Error('a')), Promise.reject(new Error('b'))]).catch(bad); tick(5, 't5');`,
  `Promise.any([Promise.reject(new Error('a')), Promise.reject(new Error('b'))]).catch(e => L(e.name + ':' + e.errors.map(x => x.message).join())); tick(6, 't6');`,
  `Promise.any([]).catch(e => L(e.name + ':' + e.errors.length + ':' + e.message)); tick(4, 't4');`,
  `Promise.race([]).then(${SEE}); tick(4, 't4'); L('never settles');`,
  `Promise.all([]).then(${SEE}); L('sync'); tick(4, 't4');`,
  `Promise.allSettled([]).then(${SEE}); L('sync'); tick(4, 't4');`,
  `Promise.allSettled([Promise.reject(1), 2, thenable(3, 'x')]).then(r => L(JSON.stringify(r))); tick(8, 't8');`
);

// Amostra determinística por hash (sampleByHash) do conjunto candidato inteiro; só depois saem os que os goldens vizinhos já têm.
const TARGET = 1400;
const selected = sampleByHash(programs, TARGET).filter((source) => !existing.has(source));

// Executa cada programa no bun, num processo próprio, pela API vm (a fonte nunca é um arquivo do projeto).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "async-order-golden-"));
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
const lines = [];
selected.forEach((source, index) => {
  const srcFile = path.join(tmp, `p${index}.txt`);
  fs.writeFileSync(srcFile, source);
  const run = spawnSync(process.execPath, [driver, srcFile, harnessFile], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") process.stderr.write(`FALHA: ${source}\n${run.stderr}\n`);
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
const output = lines.join("\n") + "\n";
if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
if (/error\tHarness/.test(output)) throw new Error("programa sem resultado do bun (timeout ou falha)");
process.stdout.write(output);
process.stderr.write(`${selected.length} de ${programs.length} programas\n`);
