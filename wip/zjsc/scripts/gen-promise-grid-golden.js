// Gera tests/golden/promise_grid_bun.tsv: grade de Promise, medida no bun 1.4.2.
// Cobre Promise.all/allSettled/any/race/withResolvers/try com thenables que lançam, getters de then, subclasses com
// constructor customizado (e `this` que não é construtor), resolve com ciclo, rejeição não tratada com Symbol, a ordem
// das reações de then em grade de pares e trios de atores, async functions com await de thenable e de subclasse,
// finally com retorno e lançamento, Promise.prototype.then/catch/finally com receptores inválidos e as mensagens de
// erro exatas. Tudo capturado em `globalThis.R` (array de rótulos) e devolvido como JSON depois de esvaziar as
// microtarefas.
// Cada programa é um arquivo (o bun transpila o fonte quando roda arquivo, ver async-golden.js): o prelúdio HARNESS, o
// corpo e a global `R` (getter de `__final()`, depois de esvaziar as microtarefas; o array de rótulos do harness é `labels`). Os programas não usam API de host (setTimeout, process,
// console, require, Bun, queueMicrotask): só L, tick, thenable, S, ok e bad. O preload do gerador ignora
// unhandledRejection. Programas já presentes em outros goldens
// (tests/golden/*.tsv, primeira coluna) são descartados. Caminho da máquina no resultado descarta o programa.
// O arquivo se chama `promise_grid_case.js` dos dois lados.
// Uso: bun scripts/gen-promise-grid-golden.js > tests/golden/promise_grid_bun.tsv   (--count só conta os programas)
const { emitFactoredLines, sampleByHash } = require("./golden-prelude.js");

// Prelúdio do arquivo; o golden leva o texto dele no `promise_grid.preludes.json`.
const HARNESS = `globalThis.labels = [];
globalThis.L = function (x) { labels.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.S = function S(v, d) {
  d = d || 0;
  var t = typeof v;
  if (t === "symbol") return String(v);
  if (t === "function") return "fn";
  if (t === "string") return JSON.stringify(v);
  if (v === null || t !== "object") return Object.is(v, -0) ? "-0" : t === "bigint" ? v + "n" : String(v);
  if (d > 3) return "...";
  if (v instanceof Error) return v.name + ":" + v.message + (Array.isArray(v.errors) ? "[" + v.errors.map(function (x) { return S(x, d + 1); }).join() + "]" : "");
  if (Array.isArray(v)) return "[" + v.map(function (x) { return S(x, d + 1); }).join() + "]";
  return "{" + Reflect.ownKeys(v).map(function (k) { return S(k) + ":" + S(v[k], d + 1); }).join() + "}";
};
globalThis.ok = function (v) { L("v:" + S(v)); };
globalThis.bad = function (e) { L("e:" + S(e)); };
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(labels); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
};`;

const { knownBodies, measureBodies, originalProgram } = require("./async-golden.js").asyncGolden({ harness: HARNESS, own: "promise_grid_bun.tsv" });
const existing = knownBodies((name) => name !== "promise_grid_bun.tsv");
const programs = [];
const seen = new Set();
// A classe `S` dos programas esconderia o auxiliar `S(valor)` do prelúdio (e `S(e)` viraria chamada sem new):
// ela passa a se chamar `Sub`, e `new S(` também.
const renameShadowingClass = (source) =>
  /class S\b/.test(source) ? source.replace(/new S\(/g, "new Sub(").replace(/\bS\b(?!\()/g, "Sub") : source;
const add = (...sources) => {
  for (const raw of sources) {
    const source = renameShadowingClass(raw);
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    // A deduplicação contra os goldens vizinhos NÃO entra aqui: o conjunto candidato (e a amostra tirada dele) não pode
    // depender do que os outros goldens têm hoje. O filtro `existing` vem depois da seleção.
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const SEE = "ok, bad";
const T14 = " tick(14, 't14');";

// Itens: expressões frescas para cada programa.
const ITEMS = {
  v: "1",
  str: "'s'",
  undef: "undefined",
  pOk: "Promise.resolve(2)",
  pBad: "Promise.reject(new Error('r'))",
  pBadSym: "Promise.reject(Symbol('rs'))",
  pPend: "new Promise(r => Promise.resolve().then(() => r(3)))",
  tSync: "thenable(4, 'ts')",
  tAsync: "{ then(res) { L('then'); Promise.resolve().then(() => res(5)); } }",
  tThrow: "{ then() { throw new Error('tt'); } }",
  tGetThrow: "{ get then() { throw new Error('tg'); } }",
  tGetCount: "{ get then() { L('get'); return function (res) { res(6); }; } }",
  tNotFn: "{ then: 7 }",
  tBoth: "{ then(res, rej) { res(1); rej(new Error('late')); L('both'); } }",
  tRejThrow: "{ then(res, rej) { rej(new Error('rj')); throw new Error('after'); } }",
  tResThenable: "{ then(res) { res(thenable(9, 'inner')); } }",
  tReenter: "{ then(res) { res(1); res(2); L('re'); } }",
  sub: "(() => { class P extends Promise {} return P.resolve(8); })()",
};
const ITEM_KEYS = Object.keys(ITEMS);

// 1. Combinadores contra pares de itens.
for (const c of ["all", "allSettled", "any", "race"]) {
  for (const a of ITEM_KEYS) {
    for (const b of ITEM_KEYS) add(`Promise.${c}([${ITEMS[a]}, ${ITEMS[b]}]).then(${SEE});${T14}`);
  }
}
// Entradas que não são arrays, vazias e trios.
const INPUTS = {
  undef: "undefined", nul: "null", num: "1", bool: "true", sym: "Symbol('i')", obj: "{}", fn: "function () {}", str: "'ab'", empty: "[]",
  set: "new Set([1, Promise.resolve(2)])", map: "new Map([[1, 2]])", arrayLike: "{ length: 1, 0: 1 }", iterNotFn: "{ [Symbol.iterator]: 1 }",
  iterNull: "{ [Symbol.iterator]: null }", iterNonObj: "{ [Symbol.iterator]() { return 1; } }", iterNoNext: "{ [Symbol.iterator]() { return {}; } }",
  iterNextNotFn: "{ [Symbol.iterator]() { return { next: 1 }; } }", iterThrows: "{ [Symbol.iterator]() { throw new Error('it'); } }",
  iterGetterThrows: "{ get [Symbol.iterator]() { throw new Error('ig'); } }", nextThrows: "{ [Symbol.iterator]() { return { next() { throw new Error('nx'); } }; } }",
  nextNonObj: "{ [Symbol.iterator]() { return { next() { return 1; } }; } }",
  doneThrows: "{ [Symbol.iterator]() { return { next() { return { get done() { throw new Error('dg'); } }; } }; } }",
  valueThrows: "{ [Symbol.iterator]() { return { next() { return { done: false, get value() { throw new Error('vg'); } }; } }; } }",
  gen: "(function* () { yield 1; yield Promise.reject(new Error('g')); })()",
  genThrow: "(function* () { yield 1; throw new Error('gt'); })()",
  logged: "(function () { var i = 0; return { [Symbol.iterator]() { return { next() { i++; L('n' + i); return i < 3 ? { done: false, value: i } : { done: true }; }, return() { L('ret'); return {}; } }; } }; })()",
  sparse: "[1, , 3]", nested: "[[1], [Promise.resolve(2)]]",
};
const SUBR = "class S extends Promise { static resolve(v) { throw new Error('rs'); } }";
for (const c of ["all", "allSettled", "any", "race"]) {
  for (const [n, input] of Object.entries(INPUTS)) {
    add(`Promise.${c}(${input}).then(${SEE});${T14}`);
    add(`${SUBR} S.${c}(${input}).then(${SEE});${T14}`);
    add(`try { Promise.${c}(${input}).then(${SEE}); } catch (e) { L('c:' + S(e)); }${T14}`);
  }
  for (const [a, b, d] of [["pOk", "pBad", "v"], ["tSync", "pOk", "tThrow"], ["pPend", "v", "pBad"], ["tAsync", "tSync", "pOk"], ["pBadSym", "pBad", "v"], ["tGetThrow", "pOk", "pOk"], ["tBoth", "pBad", "pPend"], ["v", "v", "v"], ["pBad", "pBad", "pBad"], ["pOk", "pOk", "pOk"]]) {
    add(`Promise.${c}([${ITEMS[a]}, ${ITEMS[b]}, ${ITEMS[d]}]).then(${SEE});${T14}`);
    add(`Promise.${c}([${ITEMS[a]}, ${ITEMS[b]}, ${ITEMS[d]}]).then(${SEE}); (async () => { await null; L('o1'); await null; L('o2'); await null; L('o3'); await null; L('o4'); })();${T14}`);
  }
}

// 2. `this` customizado nos combinadores e nos estáticos.
const CS = {
  undef: "undefined", obj: "{}", nul: "null", num: "5", fn: "function () {}", arrow: "() => 1", cls: "class A {}", fake: "function (ex) { ex(() => {}, () => {}); }",
  noArgs: "function (ex) { ex(); }", extra: "function (ex) { ex(1, 2); }", twice: "function (ex) { ex(() => {}); ex(() => {}); }",
  undefThenFn: "function (ex) { ex(undefined, undefined); ex(() => {}, () => {}); }", throwsCtor: "function (ex) { throw new Error('ctor'); }",
  subLogCtor: "class S extends Promise { constructor(ex) { L('ctor'); super(ex); } }",
  subLogExec: "class S extends Promise { constructor(ex) { super((res, rej) => { L('exec'); ex(res, rej); }); } }",
  subTag: "class S extends Promise { constructor(ex) { super(ex); this.tag = 'x'; } }",
  subNever: "class S extends Promise { constructor() { super(() => {}); } }",
  subSpecies: "class S extends Promise { static get [Symbol.species]() { return Promise; } }",
  subResolveLog: "class S extends Promise { static resolve(v) { L('res'); return super.resolve(v); } }",
  subResolveNum: "class S extends Promise { static resolve = 5; }",
  subResolveUndef: "class S extends Promise { static resolve = undefined; }",
  subResolveGetterThrows: "class S extends Promise { static get resolve() { throw new Error('rg'); } }",
  subResolveThrows: "class S extends Promise { static resolve(v) { throw new Error('rt'); } }",
  subResolveRet7: "class S extends Promise { static resolve(v) { return 7; } }",
  subThenLog: "class S extends Promise { then(a, b) { L('then'); return super.then(a, b); } }",
  subThenNone: "class S extends Promise { then = undefined; }",
  subCtorArgs: "class S extends Promise { constructor(ex) { L('args:' + arguments.length + ':' + typeof ex + ':' + ex.length); super(ex); } }",
  withGetter: "(() => { var C = function (ex) { ex(() => {}, () => {}); }; Object.defineProperty(C, 'resolve', { get() { L('getres'); return () => 1; } }); return C; })()",
};
const CITEMS = ["[]", "[1]", "[Promise.resolve(2), Promise.reject(3)]"];
const report = "var p; try { p = %CALL%; L('r:' + typeof p + ':' + (p instanceof Promise) + ':' + S(p && p.constructor && p.constructor.name)); if (p && typeof p.then === 'function') p.then(ok, bad); } catch (e) { L('c:' + S(e)); }" + T14;
for (const [n, cs] of Object.entries(CS)) {
  const pre = /^class/.test(cs) ? `var C = ${cs.replace(/^class S/, "class S")}; ` : `var C = ${cs}; `;
  for (const c of ["all", "allSettled", "any", "race"]) {
    for (const it of CITEMS) add(pre + report.replace("%CALL%", `Promise.${c}.call(C, ${it})`));
  }
  add(pre + report.replace("%CALL%", "Promise.resolve.call(C, 1)"), pre + report.replace("%CALL%", "Promise.resolve.call(C, Promise.resolve(1))"),
    pre + report.replace("%CALL%", "Promise.reject.call(C, 1)"), pre + report.replace("%CALL%", "Promise.reject.call(C, Symbol('x'))"),
    pre + report.replace("%CALL%", "Promise.try.call(C, () => 1)"), pre + report.replace("%CALL%", "Promise.try.call(C, () => { throw new Error('tf'); })"),
    pre + report.replace("%CALL%", "Promise.try.call(C, function () { return arguments.length; }, 1, 2)"),
    pre + report.replace("%CALL%", "Promise.try.call(C)"), pre + report.replace("%CALL%", "Promise.try.call(C, 5)"),
    pre + "var w; try { w = Promise.withResolvers.call(C); L('w:' + Object.keys(w).join() + ':' + (w.promise instanceof Promise)); w.resolve(1); w.promise.then(ok, bad); } catch (e) { L('c:' + S(e)); }" + T14,
    pre + report.replace("%CALL%", "C.resolve === undefined ? 0 : C.resolve(4)"),
    pre + "try { new C(() => {}).then(ok, bad); } catch (e) { L('c:' + S(e)); } try { var r = C.reject(Symbol('q')); r.then(ok, bad); } catch (e) { L('c:' + S(e)); }" + T14,
  );
}
// Subclasse como receptor de then, species e await.
const SPECIES = {
  none: "", undef: "static get [Symbol.species]() { return undefined; }", nul: "static get [Symbol.species]() { return null; }",
  prom: "static get [Symbol.species]() { return Promise; }", num: "static get [Symbol.species]() { return 1; }",
  obj: "static get [Symbol.species]() { return {}; }", thrower: "static get [Symbol.species]() { throw new Error('sp'); }",
  fakeOk: "static get [Symbol.species]() { return function (ex) { L('fake'); ex(() => {}, () => {}); }; }",
  fakeBad: "static get [Symbol.species]() { return function (ex) { L('fakebad'); ex(1, 2); }; }",
  arrow: "static get [Symbol.species]() { return () => 1; }", log: "static get [Symbol.species]() { L('species'); return this; }",
};
const SUBUSE = {
  then: "p.then(ok, bad)", thenChain: "p.then(x => x + 1).then(ok, bad)", catchOnly: "p.catch(bad).then(ok)", fin: "p.finally(() => L('f')).then(ok, bad)",
  all: "S.all([p]).then(ok, bad)", resolveSame: "L(String(S.resolve(p) === p)); L(String(Promise.resolve(p) === p))", await: "(async () => { try { L('a:' + S(await p)); } catch (e) { L('c:' + S(e)); } })()",
  ret: "(async () => p)().then(ok, bad)", pthen: "Promise.resolve().then(() => p).then(ok, bad)",
};
const SUBSTATE = { ok: "S.resolve(1)", bad: "S.reject(new Error('sb'))", pend: "new S(r => r(2))" };
for (const [sn, sp] of Object.entries(SPECIES)) {
  for (const [un, use] of Object.entries(SUBUSE)) {
    for (const [stn, st] of Object.entries(SUBSTATE)) {
      add(`class S extends Promise { ${sp} } var p = ${st}; try { ${use}; } catch (e) { L('c:' + S(e)); }${T14}`);
    }
  }
}
// Construtor customizado: super com argumentos adulterados.
for (const body of ["super(ex);", "super((res, rej) => { res(1); });", "super((res, rej) => { rej(new Error('cr')); });", "super((res, rej) => ex(v => res(v + 1), rej));", "super(ex); this.then = undefined;", "super(ex); return { then(a) { a(77); } };", "super(() => {}); ex(5, 6);", "var self = {}; return self;"]) {
  for (const use of ["S.resolve(1).then(ok, bad)", "S.all([1, 2]).then(ok, bad)", "S.race([1]).then(ok, bad)", "S.reject(1).catch(bad)", "S.allSettled([1]).then(ok, bad)", "S.any([1]).then(ok, bad)", "(async () => { L('a:' + S(await S.resolve(1))); })()", "new S(r => r(1)).then(ok, bad)"]) {
    add(`class S extends Promise { constructor(ex) { ${body} } } try { ${use}; } catch (e) { L('c:' + S(e)); }${T14}`);
  }
}

// 3. await e demais contextos contra cada "awaitable".
const AW = Object.assign({}, ITEMS, {
  tResolveSelfNative: "(() => { var p = Promise.resolve(5); p.then = function (a, b) { L('own'); return Promise.prototype.then.call(this, a, b); }; return p; })()",
  nativeCtorObj: "(() => { var p = Promise.resolve(5); p.constructor = Object; return p; })()",
  nativeCtorGetter: "(() => { var p = Promise.resolve(5); Object.defineProperty(p, 'constructor', { get() { L('cget'); return Promise; } }); return p; })()",
  thenGetterReturnsPromiseThen: "{ get then() { L('get'); return Promise.prototype.then.bind(Promise.resolve(1)); } }",
  tThrowAfterResolve: "{ then(res) { res(1); throw new Error('ar'); } }",
  tResolveRejectedPromise: "{ then(res) { res(Promise.reject(new Error('rp'))); } }",
  tResolveSymbol: "{ then(res, rej) { rej(Symbol('ts')); } }",
  tProtoThen: "Object.create({ then(res) { L('proto'); res(1); } })",
  tFnThen: "Object.assign(function () {}, { then(res) { L('fnthen'); res(2); } })",
  tClassThen: "new (class { then(res) { L('cls'); res(3); } })()",
  tProxy: "new Proxy({}, { get(t, k) { L('p:' + String(k)); return k === 'then' ? res => res(1) : undefined; } })",
});
const AW_KEYS = Object.keys(AW);
const CTX = {
  plain: x => `(async () => { var v = await ${x}; L('got:' + S(v)); })().then(${SEE});`,
  catcher: x => `(async () => { try { var v = await ${x}; L('got:' + S(v)); } catch (e) { L('c:' + S(e)); } })().then(${SEE});`,
  ret: x => `(async () => { return ${x}; })().then(${SEE});`,
  retAwait: x => `(async () => { return await ${x}; })().then(${SEE});`,
  resolveWith: x => `new Promise(r => r(${x})).then(${SEE});`,
  rejectWith: x => `new Promise((r, j) => j(${x})).then(${SEE});`,
  thenCb: x => `Promise.resolve().then(() => (${x})).then(${SEE});`,
  catchCb: x => `Promise.reject(1).catch(() => (${x})).then(${SEE});`,
  finallyRet: x => `Promise.resolve(0).finally(() => (${x})).then(${SEE});`,
  finallyRetRej: x => `Promise.reject(Symbol('o')).finally(() => (${x})).then(${SEE});`,
  promiseResolve: x => `Promise.resolve(${x}).then(${SEE});`,
  promiseReject: x => `Promise.reject(${x}).then(${SEE});`,
  agYield: x => `(async function* () { yield ${x}; })().next().then(${SEE});`,
  agReturn: x => `(async function* () { return ${x}; })().next().then(${SEE});`,
  forAwait: x => `(async () => { try { for await (var v of [${x}]) L('fa:' + S(v)); } catch (e) { L('c:' + S(e)); } })();`,
  withRes: x => `var w = Promise.withResolvers(); w.promise.then(${SEE}); w.resolve(${x});`,
  tryRet: x => `Promise.try(() => (${x})).then(${SEE});`,
};
const OBS = {
  chain: "Promise.resolve().then(() => L('o1')).then(() => L('o2')).then(() => L('o3')).then(() => L('o4')).then(() => L('o5'));",
  asyncObs: "(async () => { await null; L('o1'); await null; L('o2'); await null; L('o3'); await null; L('o4'); })();",
};
for (const a of AW_KEYS) {
  for (const [cn, c] of Object.entries(CTX)) {
    for (const o of Object.values(OBS)) add(`${c(AW[a])} ${o}${T14}`);
  }
}

// 4. Resolução com ciclo.
add(
  `var res; var p = new Promise(r => { res = r; }); res(p); p.then(${SEE});${T14}`,
  `var rej; var p = new Promise((r, j) => { rej = j; }); rej(p); p.then(${SEE});${T14}`,
  `var p = Promise.resolve(1).then(() => p); p.then(${SEE});${T14}`,
  `var p = Promise.reject(1).catch(() => p); p.then(${SEE});${T14}`,
  `var p = Promise.resolve(1).finally(() => p); p.then(${SEE});${T14}`,
  `var p = Promise.resolve(1).then(() => { throw p; }); p.then(${SEE});${T14}`,
  `var p = (async () => { await null; return p; })(); p.then(${SEE});${T14}`,
  `var p = (async () => p)(); p.then(${SEE});${T14}`,
  `var p = (async () => { await p; })(); p.then(${SEE});${T14}`,
  `var a, b; var pa = new Promise(r => { a = r; }); var pb = new Promise(r => { b = r; }); a(pb); b(pa); pa.then(${SEE}); pb.then(${SEE});${T14}`,
  `var a, b; var pa = new Promise(r => { a = r; }); var pb = new Promise(r => { b = r; }); a(pb); pa.then(${SEE}); b(pa); pb.then(${SEE});${T14}`,
  `var w = Promise.withResolvers(); w.resolve(w.promise); w.promise.then(${SEE});${T14}`,
  `var w = Promise.withResolvers(); w.reject(w.promise); w.promise.then(${SEE}, bad);${T14}`,
  `var w = Promise.withResolvers(); var t = { then(r) { r(w.promise); } }; w.resolve(t); w.promise.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var t = { then(r) { r(p); } }; res(t); p.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); res({ get then() { return function (r) { r(p); }; } }); p.then(${SEE});${T14}`,
  `var p = Promise.all([1, new Promise(() => {})]); p.then(${SEE}); L(String(Promise.resolve(p) === p));${T14}`,
  `var p = Promise.race([Promise.resolve(1)]); L(String(Promise.resolve(p) === p)); p.then(${SEE});${T14}`,
  `var p = Promise.resolve(1); var q = p.then(() => q); q.catch(e => L('c:' + S(e) + ':' + (e instanceof TypeError))); ${OBS.chain} ${T14}`,
  `var p = Promise.resolve(1); var q = p.then(x => x); L(String(p === q)); L(String(Promise.resolve(p) === p)); L(String(Promise.resolve(q) === q));${T14}`,
  `var o = {}; o.then = function (r) { r(o); }; var guard = 0; var t = { then(r) { guard++; r(guard < 3 ? t : 'done'); } }; Promise.resolve(t).then(${SEE}); L(String(guard));${T14}`,
  `var res; var p = new Promise(r => { res = r; }); res(p); res(1); p.then(${SEE}); L('after');${T14}`,
  `var res, rej; var p = new Promise((r, j) => { res = r; rej = j; }); res(p); rej(1); p.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); res(Promise.resolve(p)); p.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = p.then(x => x); res(q); q.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = Promise.all([p]); res(q); q.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = Promise.race([p]); res(q); q.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = Promise.allSettled([p]); res(q); q.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = Promise.any([p]); res(q); q.then(${SEE}, bad);${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = p.finally(() => {}); res(q); q.then(${SEE});${T14}`,
  `var res; var p = new Promise(r => { res = r; }); var q = p.catch(() => {}); res(q); q.then(${SEE});${T14}`,
  `class S extends Promise {} var res; var p = new S(r => { res = r; }); res(p); p.then(${SEE});${T14}`,
  `class S extends Promise {} var res; var p = new S(r => { res = r; }); res(S.resolve(p)); p.then(${SEE});${T14}`,
);

// 5. Rejeição não tratada, com Symbol e outros valores; handler tardio ou ausente.
const RV = {
  sym: "Symbol('s')", symFor: "Symbol.for('f')", symIter: "Symbol.iterator", undef: "undefined", nul: "null", zero: "0", str: "'x'", obj: "{ a: 1 }",
  thenableObj: "{ then(r) { r(1); } }", err: "new Error('e')", typeErr: "new TypeError('te')", frozen: "Object.freeze({ z: 1 })", fn: "function () {}", big: "10n",
};
const RS = {
  await: v => `(async () => { try { await Promise.reject(${v}); } catch (e) { L('c:' + S(e)); } })();`,
  awaitThrow: v => `(async () => { try { throw ${v}; } catch (e) { L('c:' + S(e)); } })();`,
  asyncRej: v => `(async () => { throw ${v}; })().then(${SEE});`,
  asyncRej2: v => `(async () => { return Promise.reject(${v}); })().then(${SEE});`,
  all: v => `Promise.all([1, Promise.reject(${v})]).then(${SEE});`,
  allSettled: v => `Promise.allSettled([Promise.reject(${v})]).then(${SEE});`,
  any: v => `Promise.any([Promise.reject(${v})]).then(${SEE});`,
  race: v => `Promise.race([Promise.reject(${v}), 1]).then(${SEE});`,
  raceLate: v => `Promise.race([new Promise(() => {}), Promise.reject(${v})]).then(${SEE});`,
  catchRet: v => `Promise.reject(${v}).catch(e => e).then(${SEE});`,
  finallyPass: v => `Promise.reject(${v}).finally(() => L('f')).then(${SEE});`,
  tryThrow: v => `Promise.try(() => { throw ${v}; }).then(${SEE});`,
  withRes: v => `var w = Promise.withResolvers(); w.reject(${v}); w.promise.then(${SEE});`,
  thenRej: v => `Promise.resolve(1).then(() => { throw ${v}; }).then(${SEE});`,
  thenableRej: v => `Promise.resolve({ then(r, j) { j(${v}); } }).then(${SEE});`,
  nested: v => `Promise.reject(${v}).then(() => 1).then(() => 2).catch(e => { L('c:' + S(e)); throw e; }).then(${SEE});`,
};
for (const [vn, v] of Object.entries(RV)) {
  for (const [sn, shape] of Object.entries(RS)) {
    add(shape(v) + T14);
    add(`var late = Promise.reject(${v}); tick(3, 'x'); Promise.resolve().then(() => Promise.resolve()).then(() => Promise.resolve()).then(() => late.then(${SEE})); ${shape(v)}${T14}`);
  }
}
// Rejeição sem handler: nada observável além da ordem e do que continua rodando.
for (const v of Object.values(RV)) {
  add(`Promise.reject(${v}); L('sync'); (async () => { await null; L('a1'); })();${T14}`,
    `Promise.reject(${v}); Promise.reject(${v}).catch(${"bad"}); L('s');${T14}`,
    `(async () => { throw ${v}; })(); L('sync');${T14}`,
    `Promise.reject(${v}).then(() => {}); L('sync');${T14}`,
    `var p = Promise.reject(${v}); p.then(() => {}); p.catch(bad);${T14}`,
    `var p = Promise.reject(${v}); var q = p.finally(() => {}); L(String(p === q));${T14}`);
}

// 6. Ordem das reações: pares e trios de atores.
const ACTORS = {
  thenChain: l => `Promise.resolve().then(() => L('${l}1')).then(() => L('${l}2')).then(() => L('${l}3'));`,
  asyncAwait: l => `(async () => { await null; L('${l}1'); await null; L('${l}2'); await null; L('${l}3'); })();`,
  awaitPromise: l => `(async () => { await Promise.resolve(); L('${l}1'); await Promise.resolve(); L('${l}2'); })();`,
  awaitThenable: l => `(async () => { await thenable(1, '${l}'); L('${l}1'); })();`,
  retPromise: l => `(async () => Promise.resolve(1))().then(() => L('${l}1'));`,
  retAwait: l => `(async () => await Promise.resolve(1))().then(() => L('${l}1'));`,
  resolveThenable: l => `new Promise(r => r(Promise.resolve(1))).then(() => L('${l}1'));`,
  resolveThenableObj: l => `new Promise(r => r(thenable(1, '${l}'))).then(() => L('${l}1'));`,
  finallyChain: l => `Promise.resolve(1).finally(() => L('${l}1')).then(() => L('${l}2'));`,
  finallyRet: l => `Promise.resolve(1).finally(() => Promise.resolve()).then(() => L('${l}1'));`,
  all: l => `Promise.all([1, Promise.resolve(2)]).then(() => L('${l}1'));`,
  allSettled: l => `Promise.allSettled([Promise.reject(1)]).then(() => L('${l}1'));`,
  any: l => `Promise.any([Promise.reject(1), 2]).then(() => L('${l}1'));`,
  race: l => `Promise.race([Promise.resolve(1), 2]).then(() => L('${l}1'));`,
  rejectCatch: l => `Promise.reject(1).catch(() => L('${l}1')).then(() => L('${l}2'));`,
  pThenSkip: l => `Promise.resolve(1).then(null).then(() => L('${l}1'));`,
  tryThen: l => `Promise.try(() => 1).then(() => L('${l}1'));`,
  tryThenable: l => `Promise.try(() => thenable(1, '${l}')).then(() => L('${l}1'));`,
  withRes: l => `var w${l} = Promise.withResolvers(); w${l}.promise.then(() => L('${l}1')); w${l}.resolve(Promise.resolve(1));`,
  asyncThrow: l => `(async () => { throw 1; })().catch(() => L('${l}1'));`,
  asyncTryFin: l => `(async () => { try { await 1; } finally { L('${l}1'); } L('${l}2'); })();`,
  subThen: l => `class S${l} extends Promise {} S${l}.resolve(1).then(() => L('${l}1')).then(() => L('${l}2'));`,
};
const AK = Object.keys(ACTORS);
for (const a of AK) for (const b of AK) add(ACTORS[a]("A") + " " + ACTORS[b]("B") + T14);
for (const a of AK) for (const b of AK) add(ACTORS[a]("A") + " " + ACTORS[b]("B") + " " + ACTORS["thenChain"]("Z") + T14);

// 7. Estados de origem contra cadeias de then e formas de reação (retorno de handler).
const SRC = { ok: "Promise.resolve(1)", bad: "Promise.reject(new Error('b'))", pend: "new Promise(r => Promise.resolve().then(() => r(1)))", thenable: "Promise.resolve(thenable(1, 'x'))", sub: "(class P extends Promise {}).resolve(1)", late: "(() => { var w = Promise.withResolvers(); tick(4, 'rs').then(() => w.resolve(1)); return w.promise; })()" };
const HR = { val: "x => 'val'", throws: "x => { throw new Error('h'); }", throwsSym: "x => { throw Symbol('hs'); }", retPromise: "x => Promise.resolve('p')", retRej: "x => Promise.reject(new Error('pr'))", retThenable: "x => thenable('t', 'h')", retThenableThrows: "x => ({ then() { throw new Error('ht'); } })", retUndef: "x => {}", none: "undefined", nonFn: "5", obj: "{}", nul: "null", retSelfLike: "function (x) { return this; }" };
for (const [sn, s] of Object.entries(SRC)) {
  for (const [hn, h] of Object.entries(HR)) {
    add(`${s}.then(${h}).then(${SEE});${OBS.chain}${T14}`, `${s}.then(undefined, ${h}).then(${SEE});${OBS.chain}${T14}`, `${s}.then(${h}, ${h}).then(${SEE});${OBS.asyncObs}${T14}`, `${s}.catch(${h}).then(${SEE});${OBS.chain}${T14}`);
  }
}

// 8. finally: retorno, lançamento, argumentos e receptores.
const FF = { undef: "() => {}", val: "() => 'fv'", throws: "() => { throw new Error('fe'); }", throwsSym: "() => { throw Symbol('fs'); }", retP: "() => Promise.resolve('fp')", retRej: "() => Promise.reject(new Error('fr'))", retRejSym: "() => Promise.reject(Symbol('fr'))", retThenable: "() => thenable('ft', 'f')", retThenableThrows: "() => ({ then() { throw new Error('ftt'); } })", retPend: "() => new Promise(r => Promise.resolve().then(() => r('fd')))", args: "function () { L('args:' + arguments.length); }", thisIs: "function () { L('this:' + typeof this); }", asyncFn: "async () => { await null; }", asyncThrows: "async () => { throw new Error('af'); }", nonFn: "5", nul: "null", obj: "{}", none: "" };
const FS = { ok: "Promise.resolve(1)", bad: "Promise.reject(new Error('b'))", badSym: "Promise.reject(Symbol('bs'))", pend: "new Promise(r => Promise.resolve().then(() => r(1)))", sub: "(class P extends Promise {}).resolve(1)" };
for (const [fn, f] of Object.entries(FF)) {
  for (const [sn, s] of Object.entries(FS)) {
    add(`${s}.finally(${f}).then(${SEE});${OBS.chain}${T14}`, `${s}.finally(${f}).finally(${f}).then(${SEE});${T14}`, `${s}.then(x => x).finally(${f}).catch(bad).then(ok);${OBS.asyncObs}${T14}`);
  }
}
add(
  `L(String(Promise.prototype.finally.length) + Promise.prototype.finally.name);`, `var p = Promise.resolve(1); var f = p.finally(); L(String(f === p)); f.then(${SEE});${T14}`,
  `var calls = []; class S extends Promise { then(a, b) { calls.push(typeof a + ':' + typeof b + ':' + a.length + ':' + b.length); return super.then(a, b); } } S.resolve(1).finally(() => {}); L(calls.join());${T14}`,
  `var calls = []; class S extends Promise { then(a, b) { calls.push(String(a === 5) + ':' + String(b === 5)); return super.then(a, b); } } S.resolve(1).finally(5); L(calls.join());${T14}`,
  `var p = Promise.resolve(1); p.then = function (a, b) { L('own:' + typeof a); return 'r'; }; L(String(Promise.prototype.finally.call(p, () => {})));${T14}`,
  `var f = Promise.prototype.finally.call({ then(a, b) { L('fk:' + typeof a + typeof b); return 'k'; } }, () => {}); L(String(f));${T14}`,
  `var f = Promise.prototype.finally.call({ then(a, b) { return a; } }, () => 7); L(typeof f); L(String(f.length)); L(f.name);${T14}`,
  `var f = Promise.prototype.finally.call({ then(a, b) { return [a, b]; } }, () => 7); L(f.map(x => x.length + ':' + x.name).join());${T14}`,
  `var r = Promise.prototype.finally.call({ then(a, b) { return [a, b]; } }, 5); L(String(r[0] === 5) + String(r[1] === 5));${T14}`,
  `try { var r = Promise.prototype.finally.call({ then: undefined }, () => {}); L(typeof r); } catch (e) { L(S(e)); }${T14}`,
  `try { Promise.prototype.finally.call({ then: 1 }, () => {}); } catch (e) { L(S(e)); }${T14}`,
  `try { Promise.prototype.finally.call({ get then() { throw new Error('gt'); } }, () => {}); } catch (e) { L(S(e)); }${T14}`,
  `var c = 0; var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { c++; return Promise; } }); p.finally(() => {}); L(String(c));${T14}`,
  `var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { ex(() => {}, () => {}); } }; var f = p.finally(() => L('x')); L(typeof f); tick(5, 'q');${T14}`,
  `var p = Promise.resolve(1); p.constructor = undefined; p.finally(() => L('x')).then(${SEE});${T14}`,
  `var p = Promise.resolve(1); p.constructor = 1; try { p.finally(() => {}); } catch (e) { L(S(e)); }${T14}`,
);

// 9. then/catch/finally com receptores inválidos e species variados.
const RECV = {
  undef: "undefined", nul: "null", num: "1", str: "'s'", sym: "Symbol('r')", bool: "true", big: "1n", obj: "{}", arr: "[]", fn: "function () {}", arrow: "() => 1",
  ctor: "Promise", proto: "Promise.prototype", inherit: "Object.create(Promise.resolve(1))", proxy: "new Proxy(Promise.resolve(1), {})",
  proxyObj: "new Proxy({}, {})", thenOnly: "{ then: Promise.prototype.then }", date: "new Date(0)", map: "new Map()", pending: "new Promise(() => {})",
  ctorUndef: "Object.assign(Promise.resolve(1), { constructor: undefined })", ctorNull: "Object.assign(Promise.resolve(1), { constructor: null })",
  ctorNum: "Object.assign(Promise.resolve(1), { constructor: 1 })", ctorEmpty: "Object.assign(Promise.resolve(1), { constructor: {} })",
  ctorSpNull: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: null } })", ctorSpUndef: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: undefined } })",
  ctorSpNum: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: 1 } })", ctorSpFn: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: function () {} } })",
  ctorSpBadRes: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: function (ex) { ex(1, 2); } } })",
  ctorSpFake: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: function (ex) { ex(() => {}, () => {}); } } })",
  ctorSpArrow: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: () => 1 } })",
  ctorGetThrows: "(() => { var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { throw new Error('cg'); } }); return p; })()",
  spGetThrows: "Object.assign(Promise.resolve(1), { constructor: { get [Symbol.species]() { throw new Error('sg'); } } })",
  ctorFnNoSpecies: "Object.assign(Promise.resolve(1), { constructor: function (ex) { ex(() => {}, () => {}); } })",
  boundThen: "(() => { var p = Promise.resolve(1); return Object.create(p); })()",
};
const MARGS = { none: "", f: "x => x", fg: "x => x, e => e", g: "undefined, e => e", nonFn: "1, 2", nul: "null, null", thr: "() => { throw 1; }" };
for (const [rn, r] of Object.entries(RECV)) {
  for (const [an, a] of Object.entries(MARGS)) {
    const body = (m) => `var r; try { r = Promise.prototype.${m}.call(${r}${a ? ", " + a : ""}); L('ret:' + typeof r + ':' + (r instanceof Promise)); if (r && typeof r.then === 'function') r.then(ok, bad); } catch (e) { L('c:' + S(e)); }${T14}`;
    add(body("then"));
    if (an === "none" || an === "f" || an === "fg" || an === "g" || an === "thr") add(body("catch"), body("finally"));
  }
}
add(
  `try { Promise.prototype.then.call(); } catch (e) { L(S(e)); }`, `try { Promise.prototype.then(); } catch (e) { L(S(e)); }`,
  `try { Promise.prototype.catch.call(); } catch (e) { L(S(e)); }`, `try { Promise.prototype.finally.call(); } catch (e) { L(S(e)); }`,
  `try { Promise.prototype.catch.call(1, 2); } catch (e) { L(S(e)); }`, `try { Promise.prototype.catch.call({ then: 1 }, 2); } catch (e) { L(S(e)); }`,
  `L(String(Promise.prototype.catch.call({ then(a, b) { return [a, b]; } }, 5).join()));`, `L(String(Promise.prototype.catch.call({ then(a, b) { return [typeof a, typeof b]; } }, () => {}).join()));`,
  `var t = Promise.prototype.then; try { t.call(Promise.resolve(1)); L('ok'); } catch (e) { L(S(e)); } try { new t(); } catch (e) { L(S(e)); }`,
  `try { new Promise(); } catch (e) { L(S(e)); } try { new Promise(1); } catch (e) { L(S(e)); } try { new Promise({}); } catch (e) { L(S(e)); } try { Promise(() => {}); } catch (e) { L(S(e)); } try { new Promise(undefined); } catch (e) { L(S(e)); } try { new Promise(null); } catch (e) { L(S(e)); }`,
  `try { Promise.prototype.then.call(Promise.prototype, () => {}); } catch (e) { L(S(e)); }`, `try { Promise.resolve.call(); } catch (e) { L(S(e)); } try { Promise.reject.call(1); } catch (e) { L(S(e)); } try { Promise.all.call(1, []); } catch (e) { L(S(e)); }`,
  `try { Promise.withResolvers.call(1); } catch (e) { L(S(e)); } try { Promise.withResolvers.call(undefined); } catch (e) { L(S(e)); } try { Promise.try.call(1, () => {}); } catch (e) { L(S(e)); } try { Promise.try.call(undefined, 1); } catch (e) { L(S(e)); }`,
  `L(Object.getOwnPropertyNames(Promise).join()); L(Object.getOwnPropertyNames(Promise.prototype).join()); L(Promise.length + ':' + Promise.name + ':' + Promise.prototype[Symbol.toStringTag]);`,
  `L([Promise.all, Promise.allSettled, Promise.any, Promise.race, Promise.resolve, Promise.reject, Promise.withResolvers, Promise.try].map(f => f.name + f.length).join());`,
  `L([Promise.prototype.then, Promise.prototype.catch, Promise.prototype.finally].map(f => f.name + f.length).join());`,
  `var d = Object.getOwnPropertyDescriptor(Promise, Symbol.species); L(typeof d.get + ':' + d.get.name + ':' + d.set + ':' + d.enumerable + ':' + d.configurable + ':' + String(Promise[Symbol.species] === Promise));`,
  `var w = Promise.withResolvers(); L(w.resolve.name + '|' + w.resolve.length + '|' + w.reject.name + '|' + w.reject.length + '|' + Object.keys(w).join());`,
  `var r = []; var P = function (ex) { ex(function () {}, function () {}); }; P.resolve = Promise.resolve; var q = Promise.all.call(P, [1]); L(String(q instanceof P));${T14}`,
  `try { Promise.all.call(function () {}, []); } catch (e) { L(S(e)); } try { Promise.race.call(class {}, []); } catch (e) { L(S(e)); }`,
);

// 10. Promise.try e withResolvers em grade.
const TF = {
  val: "() => 'v'", undef: "() => {}", throws: "() => { throw new Error('te'); }", throwsSym: "() => { throw Symbol('ts'); }", retP: "() => Promise.resolve('p')",
  retRej: "() => Promise.reject(new Error('pr'))", retThenable: "() => thenable('t', 'x')", retThenThrows: "() => ({ then() { throw new Error('tt'); } })",
  thisIs: "function () { return typeof this + ':' + String(this === undefined); }", strictThis: "function () { 'use strict'; return typeof this; }", args: "function () { return arguments.length + ':' + Array.prototype.join.call(arguments); }",
  asyncFn: "async () => 'af'", asyncThrows: "async () => { throw new Error('at'); }", gen: "function* () { yield 1; }", nonFn: "5", obj: "{}", nul: "null", undefF: "undefined", cls: "class A {}",
};
const TARGS = { none: "", one: ", 1", two: ", 1, 2", spreadLike: ", [1, 2]", undefArg: ", undefined" };
for (const [fn, f] of Object.entries(TF)) {
  for (const [an, a] of Object.entries(TARGS)) {
    add(`var order = []; var p = Promise.try(${f}${a}); L('sync'); p.then(${SEE});${OBS.chain}${T14}`);
    add(`class S extends Promise {} var p = S.try(${f}${a}); L(String(p instanceof S)); p.then(${SEE});${T14}`);
  }
}
const WR = {
  basic: "var w = Promise.withResolvers(); w.resolve(1); w.promise.then(ok, bad);", reject: "var w = Promise.withResolvers(); w.reject(Symbol('w')); w.promise.then(ok, bad);",
  twice: "var w = Promise.withResolvers(); w.resolve(1); w.resolve(2); w.reject(3); w.promise.then(ok, bad);", thenable: "var w = Promise.withResolvers(); w.resolve(thenable(1, 'w')); w.promise.then(ok, bad);",
  thenableThrows: "var w = Promise.withResolvers(); w.resolve({ then() { throw new Error('wt'); } }); w.promise.then(ok, bad);", getterThrows: "var w = Promise.withResolvers(); w.resolve({ get then() { throw new Error('wg'); } }); w.promise.then(ok, bad);",
  detached: "var w = Promise.withResolvers(); var r = w.resolve; r.call(undefined, 7); w.promise.then(ok, bad);", detachedRej: "var w = Promise.withResolvers(); var j = w.reject; j.call(null, 8); w.promise.then(ok, bad);",
  noArg: "var w = Promise.withResolvers(); w.resolve(); w.promise.then(ok, bad);", extraArgs: "var w = Promise.withResolvers(); w.resolve(1, 2, 3); w.promise.then(ok, bad);",
  ctorNew: "var w = Promise.withResolvers(); try { new w.resolve(1); } catch (e) { L(S(e)); }", props: "var w = Promise.withResolvers(); L(JSON.stringify(Object.getOwnPropertyDescriptors(w), (k, v) => typeof v === 'function' ? 'fn' : v)); L(String(Object.getPrototypeOf(w) === Object.prototype));",
  keysOrder: "var w = Promise.withResolvers(); L(Reflect.ownKeys(w).join());", fnProps: "var w = Promise.withResolvers(); L(Object.getOwnPropertyNames(w.resolve).join() + '|' + w.resolve.hasOwnProperty('prototype') + '|' + (w.resolve === w.reject));",
  pending: "var w = Promise.withResolvers(); w.promise.then(ok, bad); L('pending');", chain: "var w = Promise.withResolvers(); w.promise.then(x => x + 1).then(ok, bad); w.resolve(1);",
  self: "var w = Promise.withResolvers(); w.resolve(w.promise); w.promise.then(ok, bad);", nativeP: "var w = Promise.withResolvers(); w.resolve(Promise.resolve(3)); w.promise.then(ok, bad);",
  nativeRej: "var w = Promise.withResolvers(); w.resolve(Promise.reject(new Error('wr'))); w.promise.then(ok, bad);",
};
for (const [n, body] of Object.entries(WR)) {
  for (const o of ["", OBS.chain, OBS.asyncObs, "(async () => { try { await Promise.reject(1); } catch (e) { L('c'); } })();"]) add(`${body} ${o}${T14}`);
}

// 11. Receptores ligados a combinadores com resolve customizado e iteradores com return.
for (const c of ["all", "allSettled", "any", "race"]) {
  for (const [rn, r] of Object.entries({
    log: "static resolve(v) { L('res:' + S(v)); return super.resolve(v); }", throws: "static resolve(v) { throw new Error('rs'); }",
    retNonP: "static resolve(v) { return { then(a, b) { L('fakethen'); a(v); } }; }", retThenThrows: "static resolve(v) { return { then() { throw new Error('ft'); } }; }",
    retNoThen: "static resolve(v) { return {}; }", retRej: "static resolve(v) { return Promise.reject(new Error('rr')); }", retSelfThen: "static resolve(v) { return { then: Promise.prototype.then.bind(Promise.resolve(v)) }; }",
  })) {
    for (const input of ["[1, 2]", "[Promise.reject(new Error('x')), 1]", "[]", INPUTS.logged, "(function* () { try { yield 1; yield 2; } finally { L('genfin'); } })()"]) {
      add(`class S extends Promise { ${r} } S.${c}(${input}).then(${SEE});${T14}`);
    }
  }
}
// Resolve lido uma vez, antes do iterador.
for (const c of ["all", "allSettled", "any", "race"]) {
  add(`var n = 0; class S extends Promise { static get resolve() { n++; L('getres'); return super.resolve; } } S.${c}([1, 2, 3]).then(${SEE}); L('n:' + n);${T14}`,
    `class S extends Promise { static get resolve() { L('getres'); return super.resolve; } } S.${c}({ [Symbol.iterator]() { L('iter'); return [][Symbol.iterator](); } }).then(${SEE});${T14}`,
    `class S extends Promise { static get resolve() { L('getres'); throw new Error('gr'); } } S.${c}({ [Symbol.iterator]() { L('iter'); return [][Symbol.iterator](); } }).then(${SEE}, bad);${T14}`,
    `class S extends Promise { static get resolve() { L('getres'); return 1; } } S.${c}([]).then(${SEE}, bad);${T14}`);
}

// 12. Elementos de all/allSettled/any: resolver chamado duas vezes, ordem e erros agregados.
add(
  `var fns = []; var T = { then(res, rej) { fns.push(res); } }; Promise.all([T, T]).then(${SEE}); Promise.resolve().then(() => { fns[0](1); fns[0](2); fns[1](3); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.allSettled([T, T]).then(${SEE}); Promise.resolve().then(() => { fns[0][0](1); fns[0][1](9); fns[1][1](2); fns[1][0](3); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.any([T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[0][1](1); fns[0][1](2); fns[1][1](3); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.any([T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[1][1](1); fns[0][1](2); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.all([T, T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[2][0]('c'); fns[0][0]('a'); fns[1][0]('b'); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.all([T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[1][1](new Error('x')); fns[0][0](1); fns[0][1](2); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.race([T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[1][0]('b'); fns[0][1](new Error('a')); });${T14}`,
  `var fns = []; var T = { then(res, rej) { fns.push([res, rej]); } }; Promise.race([T, T]).then(${SEE}, bad); Promise.resolve().then(() => { fns[1][1](Symbol('b')); fns[0][0]('a'); });${T14}`,
  `var r = Promise.all([1, 2, 3]); r.then(v => { v.push(4); L(S(v)); });${T14}`,
  `var a = [Promise.resolve(1)]; Promise.all(a).then(v => L(String(v === a) + S(v)));${T14}`,
  `Promise.all([,]).then(${SEE});${T14}`, `Promise.all([, ,]).then(${SEE});${T14}`, `Promise.allSettled([,]).then(${SEE});${T14}`, `Promise.any([,]).then(${SEE});${T14}`, `Promise.race([,]).then(${SEE});${T14}`,
  `var p = Promise.any([Promise.reject(1)]); p.catch(e => { L(S(e)); L(e.constructor === AggregateError); L(Object.getOwnPropertyNames(e).join()); L(JSON.stringify(Object.getOwnPropertyDescriptor(e, 'errors'))); });${T14}`,
  `Promise.any([]).catch(e => { L(S(e)); L(Object.getOwnPropertyNames(e).join()); L(String(e.errors.length)); L(String(Object.getPrototypeOf(e) === AggregateError.prototype)); });${T14}`,
  `Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => { L(S(e.errors)); L(e.message); L(String(Array.isArray(e.errors))); });${T14}`,
  `Promise.any(new Set([Promise.reject(1)])).catch(e => L(S(e)));${T14}`,
  `Promise.any('ab').then(${SEE}, bad);${T14}`, `Promise.any([Promise.reject(Symbol('a')), Promise.reject(Symbol('b'))]).then(${SEE}, bad);${T14}`,
  `Promise.allSettled([Promise.reject(Symbol('a')), Promise.resolve(Symbol('b'))]).then(${SEE});${T14}`,
  `Promise.allSettled([1, Promise.reject(2)]).then(r => { L(Reflect.ownKeys(r[0]).join()); L(Reflect.ownKeys(r[1]).join()); L(String(Object.getPrototypeOf(r[0]) === Object.prototype)); });${T14}`,
  `new AggregateError([1, 2], 'm').errors.length; var e = new AggregateError([1, 2], 'm', { cause: 3 }); L(S(e) + String(e.cause)); L(Object.getOwnPropertyNames(e).join());`,
  `try { new AggregateError(); } catch (x) { L(S(x)); } try { new AggregateError(1); } catch (x) { L(S(x)); } try { new AggregateError(undefined); } catch (x) { L(S(x)); } L(S(new AggregateError('s')));`,
  `L(S(new AggregateError({ [Symbol.iterator]() { return [7][Symbol.iterator](); } })));`,
  `L(String(AggregateError.length) + AggregateError.name + String(Object.getPrototypeOf(AggregateError) === Error) + String(AggregateError.prototype.name) + JSON.stringify(AggregateError.prototype.message));`,
);

// Amostra determinística (sampleByHash, do golden-prelude): os TARGET programas de menor hash SHA-1 do texto, tirados do
// conjunto candidato inteiro, antes de descontar os goldens vizinhos. Só depois saem os que já estão em outros goldens.
const TARGET = Number(process.env.GRID_TARGET || 3100);
const candidates = sampleByHash(programs, TARGET);
const selected = candidates.filter((source) => !existing.has(originalProgram(source)));
if (process.argv[2] === "--count") {
  process.stderr.write(`${programs.length} programas, ${candidates.length} amostrados, ${selected.length} selecionados\n`);
  process.exit(0);
}

// Executa cada programa como arquivo num bun próprio (ver async-golden.js), com concorrência limitada. Cada programa roda
// RUNS vezes; se as saídas diferem, ele é não determinístico no bun e é descartado (nunca por tempo, carga da máquina ou
// ordem de término: falha de execução continua sendo erro do gerador). Resultado com caminho da máquina também é descartado.
measureBodies(selected, "promise_grid_case.js", { jobs: Number(process.env.GRID_JOBS || 8), runs: Number(process.env.GRID_RUNS || 2) }).then((lines) => {
  process.stdout.write(emitFactoredLines("promise_grid", lines));
  process.stderr.write(`${selected.length} de ${programs.length} programas\n`);
});
