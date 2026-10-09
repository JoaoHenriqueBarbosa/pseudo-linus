// Gera tests/golden/microtask_bun.tsv: ordem exata de microtarefas (await, async generator, for await,
// yield*, combinadores de Promise, finally, species, fila de requests do async generator), medida no bun 1.4.2.
// Cada programa registra eventos no array global `log` (auxiliares L, tick e thenable de
// tests/golden/async_bun_harness.js, o mesmo texto que tests/microtask_bun_golden.rs embute) e o golden é o
// JSON do log depois de esvaziar as microtarefas (a global `R`), ou `error<TAB>name<TAB>message JSON` se lançou de
// forma síncrona. Os marcadores `tick(n, 'tn')` fixam a posição relativa de cada evento na fila de microtarefas.
// Fica de fora queueMicrotask, process.nextTick e timers (são do host, não do JavaScriptCore) e rejeição
// não tratada. Programas já presentes em promise_bun.tsv e async_bun.tsv são descartados.
// O programa é um arquivo (o bun transpila o fonte quando roda arquivo): harness, corpo e a global `R` que devolve o
// log, ver async-golden.js. O arquivo se chama `microtask_case.js` dos dois lados.
// Uso: bun scripts/gen-microtask-golden.js > tests/golden/microtask_bun.tsv
const { emitFactoredLines, sampleByHash } = require("./golden-prelude.js");
const { knownBodies, measureBodies, originalProgram } = require("./async-golden.js").asyncGolden({ own: "microtask_bun.tsv" });

const existing = knownBodies(["promise_bun.tsv", "async_bun.tsv"]);
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
const TICKS = [1, 2, 3, 4, 5, 6, 7, 8];
const operands = {
  value: "1",
  native: "Promise.resolve(1)",
  thenable: "thenable(1, 'th')",
  rejected: "Promise.reject(new Error('r'))",
  thenableGetter: "{ get then() { L('get'); return res => { L('call'); res(1); }; } }",
};

// 1. await em cada forma de função: ticks do operando contra marcadores.
const forms = {
  fn: x => `(async function () { await ${x}; L('after'); })()`,
  arrow: x => `(async () => { await ${x}; L('after'); })()`,
  method: x => `({ async m() { await ${x}; L('after'); } }).m()`,
  genNext: x => `(async function* () { await ${x}; L('after'); })().next()`,
  retAwait: x => `(async () => { return await ${x}; })().then(() => L('after'))`,
  retPlain: x => `(async () => { return ${x}; })().then(() => L('after'), () => L('after'))`,
  yieldAwait: x => `(async function* () { yield await ${x}; })().next().then(() => L('after'))`,
  yieldPlain: x => `(async function* () { yield ${x}; })().next().then(() => L('after'), () => L('after'))`,
  retGen: x => `(async function* () { return ${x}; })().next().then(() => L('after'), () => L('after'))`,
  finallyAwait: x => `(async () => { try { L('try'); } finally { await ${x}; L('fin'); } L('after'); })()`,
};
for (const [fname, form] of Object.entries(forms)) {
  for (const [oname, x] of Object.entries(operands)) {
    const wrapped = fname.startsWith("ret") || fname.startsWith("yield") || fname === "genNext" ? form(x) : form(x);
    const guard = oname === "rejected" && !/retPlain|yieldPlain|retGen/.test(fname) ? `${wrapped.replace(/^\(/, "(").replace(/\)$/, ")")}.catch(() => L('caught'))` : wrapped;
    add(`${guard}; L('sync');`);
    for (const n of [1, 2, 3, 4, 5]) add(`${guard}; tick(${n}, 't${n}');`);
  }
}

// 2. Duas funções intercaladas (async, async arrow, gerador) e ordem entre geradores.
const kinds = {
  fn: (id, n) => `(async function () { for (var i = 0; i < ${n}; i++) { await null; L('${id}' + i); } })()`,
  arrow: (id, n) => `(async () => { for (var i = 0; i < ${n}; i++) { await null; L('${id}' + i); } })()`,
  gen: (id, n) => `(async function* () { for (var i = 0; i < ${n}; i++) { yield i; L('${id}y' + i); } })()`,
  prom: (id, n) => `(async () => { for (var i = 0; i < ${n}; i++) { await Promise.resolve(i); L('${id}' + i); } })()`,
  th: (id, n) => `(async () => { for (var i = 0; i < ${n}; i++) { await thenable(i, '${id}'); L('${id}' + i); } })()`,
};
const names = Object.keys(kinds);
for (const a of names) for (const b of names) {
  const drive = (k, id) => k === "gen" ? `(async () => { var g = ${kinds.gen(id, 3)}; for (var i = 0; i < 3; i++) await g.next(); L('${id}done'); })()` : kinds[k](id, 3);
  add(`${drive(a, "a")}; ${drive(b, "b")}; L('sync');`);
  add(`${drive(a, "a")}; ${drive(b, "b")}; tick(2, 't2'); tick(5, 't5');`);
}

// 3. Fila de requests do async generator em cada estado.
const G = "(async function* () { try { var a = yield 1; L('a=' + a); var b = yield 2; L('b=' + b); } finally { L('fin'); } return 9; })()";
const seeAll = "v => L('v:' + JSON.stringify(v)), e => L('e:' + (e && e.message || e))";
const calls = {
  next: "g.next('x')", nextP: "g.next(Promise.resolve('p'))", nextT: "g.next(thenable('t', 'tn'))",
  ret: "g.return('r')", retP: "g.return(Promise.resolve('rp'))", retT: "g.return(thenable('rt', 'rtn'))", retRej: "g.return(Promise.reject(new Error('rr')))",
  thr: "g.throw(new Error('tt'))",
};
const cn = Object.keys(calls);
for (const c1 of cn) {
  add(`var g = ${G}; ${calls[c1]}.then(${seeAll}); tick(3, 't3'); tick(6, 't6');`);
  for (const c2 of cn) {
    add(`var g = ${G}; ${calls[c1]}.then(${seeAll}); ${calls[c2]}.then(${seeAll}); L('sync'); tick(4, 't4'); tick(8, 't8');`);
  }
}
// estado suspendedYield / completed / executing
for (const c of cn) {
  add(`var g = ${G}; g.next().then(() => { ${calls[c]}.then(${seeAll}); }); tick(3, 't3'); tick(6, 't6');`);
  add(`var g = ${G}; g.next().then(() => g.return()).then(() => { ${calls[c]}.then(${seeAll}); }); tick(5, 't5'); tick(9, 't9');`);
  add(`var g = (async function* () { yield 1; })(); g.next(); g.next().then(() => { ${calls[c]}.then(${seeAll}); }); tick(6, 't6');`);
}
add(
  `var g; g = (async function* () { g.next().then(${seeAll}); L('in'); yield 1; L('out'); })(); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g; g = (async function* () { try { g.return(7).then(${seeAll}); } finally { L('fin'); } yield 1; })(); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g; g = (async function* () { g.throw(new Error('x')).then(${seeAll}); yield 1; })(); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g = (async function* () { throw new Error('boom'); })(); g.next().then(${seeAll}); g.next().then(${seeAll}); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g = (async function* () { yield Promise.reject(new Error('yr')); L('unreached'); })(); g.next().then(${seeAll}); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g = (async function* () { try { yield Promise.reject(new Error('yr')); } catch (e) { L('caught ' + e.message); yield 'again'; } })(); g.next().then(${seeAll}); g.next().then(${seeAll}); g.next().then(${seeAll}); tick(6, 't6');`,
  `var g = (async function* () { yield 1; })(); var p = g.return(Promise.reject(new Error('x'))); p.then(${seeAll}); g.next().then(${seeAll}); tick(5, 't5');`,
  `var g = (async function* () { yield 1; })(); g.return({ get then() { L('get'); return undefined; } }).then(${seeAll}); tick(4, 't4');`,
  `var g = (async function* () { yield 1; })(); g.return({ get then() { L('get'); throw new Error('gt'); } }).then(${seeAll}); tick(4, 't4');`,
  `var g = (async function* () { yield 1; })(); g.next.call({}).then(${seeAll}, e => L('rej ' + e.constructor.name)); g.return.call(1).catch(e => L('rej2 ' + e.constructor.name)); tick(3, 't3');`,
  `var g = (async function* () { yield 1; })(); Promise.all([g.next(), g.next(), g.return(5), g.next()]).then(v => L(JSON.stringify(v))); tick(8, 't8');`
);

// 4. yield / yield* com async e sync iterator, IteratorClose via AsyncFromSyncIterator.
const syncIt = (tag, extra = "") => `{ [Symbol.iterator]() { var i = 0; return { next(v) { L('${tag}.next'); return i++ < 2 ? { value: i, done: false } : { value: 'ret', done: true }; }, return(v) { L('${tag}.return'); return { value: v, done: true }; }${extra} }; } }`;
const asyncIt = (tag) => `{ [Symbol.asyncIterator]() { var i = 0; return { next(v) { L('${tag}.next'); return Promise.resolve(i++ < 2 ? { value: i, done: false } : { value: 'ret', done: true }); }, return(v) { L('${tag}.return'); return Promise.resolve({ value: v, done: true }); }, throw(e) { L('${tag}.throw'); return Promise.resolve({ value: 'th', done: false }); } }; } }`;
const promiseValIt = `{ [Symbol.iterator]() { var i = 0; return { next() { return i++ < 2 ? { value: Promise.resolve(i), done: false } : { done: true }; }, return() { L('close'); return {}; } }; } }`;
const rejectValIt = `{ [Symbol.iterator]() { var i = 0; return { next() { return i++ < 2 ? { value: Promise.reject(new Error('rv' + i)), done: false } : { done: true }; }, return() { L('close'); return {}; } }; } }`;
const sources = { sync: syncIt("s"), async: asyncIt("a"), promVal: promiseValIt, rejVal: rejectValIt, array: "[1, 2]", arrPromises: "[Promise.resolve(1), thenable(2, 'x')]" };
for (const [sn, src] of Object.entries(sources)) {
  add(`var g = (async function* () { var r = yield* ${src}; L('r=' + r); })(); g.next().then(${seeAll}); g.next().then(${seeAll}); g.next().then(${seeAll}); g.next().then(${seeAll}); tick(9, 't9');`);
  add(`var g = (async function* () { try { yield* ${src}; } finally { L('fin'); } })(); g.next().then(${seeAll}); g.return('R').then(${seeAll}); g.next().then(${seeAll}); tick(9, 't9');`);
  add(`var g = (async function* () { try { yield* ${src}; } catch (e) { L('c ' + (e && e.message)); } })(); g.next().then(${seeAll}); g.throw(new Error('T')).then(${seeAll}); g.next().then(${seeAll}); tick(9, 't9');`);
  add(`(async () => { try { for await (var x of ${src}) { L('x=' + (x && x.message || x)); break; } } catch (e) { L('c ' + e.message); } L('end'); })(); tick(3, 't3'); tick(7, 't7'); tick(12, 't12');`);
  add(`(async () => { try { for await (var x of ${src}) { L('x=' + (x && x.message || x)); return 'R'; } } catch (e) { L('c ' + e.message); } L('end'); })().then(${seeAll}); tick(3, 't3'); tick(7, 't7'); tick(12, 't12');`);
  add(`(async () => { try { for await (var x of ${src}) { L('x=' + (x && x.message || x)); throw new Error('body'); } } catch (e) { L('c ' + e.message); } L('end'); })(); tick(3, 't3'); tick(7, 't7'); tick(12, 't12');`);
  add(`(async () => { var n = 0; try { for await (var x of ${src}) { L('x=' + (x && x.message || x)); if (++n === 1) continue; } } catch (e) { L('c ' + e.message); } L('end'); })(); tick(3, 't3'); tick(9, 't9'); tick(14, 't14');`);
  add(`outer: (async () => { try { for (var i = 0; i < 2; i++) for await (var x of ${src}) { L('x' + i); continue outer; } } catch (e) { L('c ' + e.message); } L('end'); })(); tick(4, 't4'); tick(10, 't10');`);
}
// return() do iterador async/sync com retornos inválidos ou lançando.
const badReturns = {
  nonObject: "return() { L('ret'); return 1; }",
  throws: "return() { L('ret'); throw new Error('rt'); }",
  rejects: "return() { L('ret'); return Promise.reject(new Error('rrej')); }",
  thenableRet: "return() { L('ret'); return thenable({ done: true }, 'rth'); }",
  absent: "",
  nullRet: "return: null",
};
for (const [name, body] of Object.entries(badReturns)) {
  for (const mode of ["sync", "async"]) {
    const sym = mode === "sync" ? "Symbol.iterator" : "Symbol.asyncIterator";
    const wrap = v => mode === "sync" ? v : `Promise.resolve(${v})`;
    const it = `{ [${sym}]() { return { next() { return ${wrap("{ value: 1, done: false }")}; }${body ? ", " + body : ""} }; } }`;
    add(`(async () => { try { for await (var x of ${it}) break; L('broke'); } catch (e) { L('c ' + e.message); } L('end'); })(); tick(2, 't2'); tick(6, 't6');`);
    add(`(async () => { try { for await (var x of ${it}) throw new Error('b'); } catch (e) { L('c ' + e.message); } L('end'); })(); tick(2, 't2'); tick(6, 't6');`);
    add(`var g = (async function* () { try { yield* ${it}; } catch (e) { L('c ' + (e && e.constructor.name)); } })(); g.next().then(${seeAll}); g.return(4).then(${seeAll}); tick(3, 't3'); tick(8, 't8');`);
    add(`var g = (async function* () { try { yield* ${it}; } catch (e) { L('c ' + (e && e.constructor.name)); } })(); g.next().then(${seeAll}); g.throw(new Error('T')).then(${seeAll}); tick(3, 't3'); tick(8, 't8');`);
  }
}
// next do iterador com resultado não objeto, getters com efeito em done/value.
add(
  `(async () => { try { for await (var x of { [Symbol.asyncIterator]() { return { next() { return 1; } }; } }) ; } catch (e) { L(e.constructor.name); } })();`,
  `(async () => { var i = 0; for await (var x of { [Symbol.iterator]() { return { next() { return { get done() { L('done'); return i++ > 1; }, get value() { L('value'); return Promise.resolve(5); } }; } }; } }) L('x' + x); L('end'); })(); tick(8, 't8');`,
  `(async () => { var i = 0; for await (var x of { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ get done() { L('done'); return i++ > 1; }, get value() { L('value'); return 5; } }); } }; } }) L('x' + x); L('end'); })(); tick(8, 't8');`,
  `(async () => { for await (var x of { get [Symbol.asyncIterator]() { L('getAI'); return undefined; }, [Symbol.iterator]() { L('getI'); return [1][Symbol.iterator](); } }) L('x' + x); L('end'); })(); tick(6, 't6');`,
  `(async () => { try { for await (var x of { [Symbol.asyncIterator]: null, [Symbol.iterator]: null }) ; } catch (e) { L(e.constructor.name); } })();`,
  `(async () => { for await (var x of [1, 2, 3]) L('x' + x); })(); (async () => { for await (var x of [4, 5, 6]) L('y' + x); })(); tick(10, 't10');`,
  `(async () => { for await (var x of (async function* () { yield 1; yield 2; })()) L('x' + x); })(); (async () => { for await (var x of (async function* () { yield 4; yield 5; })()) L('y' + x); })(); tick(10, 't10');`
);

// 5. Combinadores com thenables, getters de then com efeito, species e finally.
const items = {
  plain: "1", native: "Promise.resolve(1)", thenable: "thenable(1, 'th')", rejNative: "Promise.reject(new Error('n'))",
  rejThenable: "{ then(_, rej) { L('then:rej'); rej(new Error('t')); } }",
  getter: "{ get then() { L('get'); return res => res('g'); } }", getterThrows: "{ get then() { L('get'); throw new Error('gt'); } }",
  late: "{ then(res) { Promise.resolve().then(() => res('late')); } }", never: "{ then() { L('never'); } }",
  twice: "{ then(res, rej) { res('a'); res('b'); rej(new Error('c')); } }",
};
const combos = ["all", "allSettled", "any", "race"];
const sawArr = "v => L('v:' + JSON.stringify(v)), e => L('e:' + (e && (e.errors ? 'agg' + e.errors.length : e.message)))";
for (const c of combos) {
  for (const [n1, i1] of Object.entries(items)) {
    add(`Promise.${c}([${i1}]).then(${sawArr}); tick(3, 't3'); tick(6, 't6');`);
    for (const n2 of ["plain", "native", "thenable", "rejNative", "late"]) {
      add(`Promise.${c}([${i1}, ${items[n2]}]).then(${sawArr}); tick(4, 't4'); tick(8, 't8');`);
    }
  }
  add(`Promise.${c}([]).then(${sawArr}); tick(2, 't2');`);
  add(`Promise.${c}([1, 2, 3].values()).then(${sawArr}); tick(5, 't5');`);
  add(`Promise.${c}(new Set([1, 2])).then(${sawArr}); tick(5, 't5');`);
  add(`Promise.${c}(5).then(${sawArr}, e => L('rej ' + e.constructor.name)); tick(2, 't2');`);
  add(`Promise.${c}({ [Symbol.iterator]() { L('iter'); return { next() { L('next'); return { done: true }; }, return() { L('closed'); return {}; } }; } }).then(${sawArr}); tick(3, 't3');`);
  add(`var P = class extends Promise { static resolve(v) { L('resolve ' + v); return super.resolve(v); } }; P.${c}([1, 2]).then(${sawArr}); tick(5, 't5');`);
  add(`var P = class extends Promise { then(a, b) { L('then'); return super.then(a, b); } }; P.${c}([1, P.resolve(2)]).then(${sawArr}); tick(6, 't6');`);
  add(`var P = function (ex) { return new Promise(ex); }; P.resolve = function (v) { L('res ' + v); return Promise.resolve(v); }; Promise.${c}.call(P, [1, 2]).then(${sawArr}); tick(6, 't6');`);
  add(`var P = function (ex) { ex(() => L('R'), () => L('J')); L('ctor'); }; P.resolve = v => ({ then(a, b) { L('inner then ' + v); a(v); } }); Promise.${c}.call(P, [1, 2]); L('sync');`);
  add(`Promise.${c}([Promise.resolve(1), Promise.resolve(2)]).then(() => L('A')); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4')).then(() => L('p5')).then(() => L('p6'));`);
}
// finally
const fins = {
  undef: "() => {}", value: "() => 'v'", thrower: "() => { throw new Error('ft'); }", rejNative: "() => Promise.reject(new Error('fr'))",
  native: "() => Promise.resolve('fp')", thenable: "() => thenable('ft', 'ftn')", nonFn: "5", getterThen: "() => ({ get then() { L('get'); return undefined; } })",
};
const sources2 = { ful: "Promise.resolve('x')", rej: "Promise.reject(new Error('o'))", pend: "new Promise(r => Promise.resolve().then(() => r('late')))" };
for (const [fname, f] of Object.entries(fins)) {
  for (const [sname, s] of Object.entries(sources2)) {
    add(`${s}.finally(${f}).then(${seeAll}); tick(3, 't3'); tick(6, 't6');`);
    add(`${s}.finally(${f}).finally(${f}).then(${seeAll}); tick(4, 't4'); tick(8, 't8');`);
    add(`(async () => { try { await ${s}.finally(${f}); L('ok'); } catch (e) { L('c ' + e.message); } })(); tick(3, 't3'); tick(6, 't6');`);
  }
}
add(
  `class S extends Promise { static get [Symbol.species]() { L('species'); return Promise; } } S.resolve(1).finally(() => L('f')).then(() => L('t')); tick(4, 't4');`,
  `class S extends Promise { constructor(ex) { L('ctor'); super(ex); } } S.resolve(1).then(() => L('t')); tick(3, 't3');`,
  `class S extends Promise { constructor(ex) { L('ctor'); super(ex); } } S.resolve(1).finally(() => L('f')).then(() => L('t')); tick(5, 't5');`,
  `class S extends Promise { static get [Symbol.species]() { return Object; } } try { S.resolve(1).then(() => {}); } catch (e) { L(e.constructor.name); }`,
  `class S extends Promise { static get [Symbol.species]() { return undefined; } } S.resolve(1).then(() => L('t')).then(() => L('t2')); tick(3, 't3');`,
  `class S extends Promise { static get [Symbol.species]() { return null; } } S.resolve(1).then(() => L('t')); tick(2, 't2');`,
  `class S extends Promise {} var p = S.resolve(1); L(String(Promise.resolve(p) === p) + String(S.resolve(p) === p) + String(p.then() instanceof S));`,
  `class S extends Promise { then(a, b) { L('then'); return super.then(a, b); } } (async () => { await S.resolve(1); L('after'); })(); tick(3, 't3');`,
  `var p = Promise.resolve(1); p.constructor = function (ex) { L('ctor'); ex(() => {}, () => {}); }; p.constructor[Symbol.species] = undefined; (async () => { await p; L('after'); })(); tick(3, 't3');`,
  `var p = Promise.resolve(1); p.constructor = Object; (async () => { await p; L('after'); })(); tick(3, 't3');`,
  `var p = Promise.resolve(1); Object.defineProperty(p, 'then', { get() { L('getThen'); return Promise.prototype.then; } }); (async () => { await p; L('after'); })(); tick(3, 't3');`,
  `var p = Promise.resolve(1); p.then = function (a, b) { L('own then'); return Promise.prototype.then.call(this, a, b); }; (async () => { await p; L('after'); })(); tick(3, 't3');`,
  `var p = Promise.resolve(1); p.then = function (a, b) { L('own then'); return Promise.prototype.then.call(this, a, b); }; Promise.resolve(p).then(() => L('r')); tick(3, 't3');`,
  `var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; (async () => { await Promise.resolve(1); L('after'); await 2; L('after2'); })(); tick(4, 't4');`,
  `var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; (async function* () { yield 1; })().next().then(() => L('n')); tick(4, 't4');`,
  `var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; Promise.all([1]).then(() => L('all')); tick(4, 't4');`,
  `var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; Promise.resolve(1).finally(() => 0).then(() => L('f')); tick(4, 't4');`,
  `var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; (async () => { for await (var x of [1]) L('x'); })(); tick(5, 't5');`
);

// 6. Lançamento síncrono antes do primeiro await, argumentos e default values, top-level await (amostra).
add(
  `(async () => { L('a'); throw new Error('s'); })().catch(e => L('c ' + e.message)); L('sync');`,
  `(async function (a = (() => { throw new Error('d'); })()) { L('body'); })().catch(e => L('c ' + e.message)); L('sync');`,
  `(async function ({ a }) { L('body'); })(null).catch(e => L('c ' + e.constructor.name)); L('sync');`,
  `try { (async function* (a = (() => { throw new Error('d'); })()) { L('body'); })(); L('created'); } catch (e) { L('thrown ' + e.message); }`,
  `try { (async function* ({ a }) { })(null); L('created'); } catch (e) { L('thrown ' + e.constructor.name); }`,
  `async function f() { L('f1'); await null; L('f2'); } async function g() { L('g1'); await f(); L('g2'); } g(); L('sync'); tick(2, 't2'); tick(4, 't4');`,
  `async function f() { return Promise.resolve(1); } f().then(() => L('r')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`,
  `async function f() { return await Promise.resolve(1); } f().then(() => L('r')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`,
  `async function f() { try { return Promise.reject(new Error('x')); } catch (e) { L('caught'); } } f().then(() => L('r'), () => L('rej')); tick(4, 't4');`,
  `async function f() { try { return await Promise.reject(new Error('x')); } catch (e) { L('caught'); } } f().then(() => L('r'), () => L('rej')); tick(4, 't4');`,
  `async function f() { try { throw 1; } finally { await null; L('fin'); } } f().catch(() => L('rej')); tick(3, 't3');`,
  `async function f() { try { return 1; } finally { await null; L('fin'); } } f().then(() => L('r')); tick(3, 't3');`,
  `async function f() { try { return await 1; } finally { L('fin'); } } f().then(() => L('r')); tick(3, 't3');`,
  `async function f() { for (var i = 0; i < 3; i++) { try { await i; continue; } finally { await null; L('fin' + i); } } } f().then(() => L('r')); tick(9, 't9');`,
  `async function f() { try { await Promise.reject(1); } catch { await null; L('c'); } finally { await null; L('f'); } } f().then(() => L('r')); tick(5, 't5'); tick(8, 't8');`,
  `(async () => { await undefined; L('1'); await undefined; L('2'); })(); (async () => { L('b'); await undefined; L('3'); })(); L('sync');`,
  `async function* g() { try { yield 1; } finally { await null; L('fin'); } } var it = g(); it.next().then(() => it.return('r')).then(v => L(JSON.stringify(v))); tick(8, 't8');`,
  `async function* g() { try { yield 1; } finally { yield 'f'; L('after'); } } var it = g(); it.next().then(() => it.return('r')).then(v => { L(JSON.stringify(v)); return it.next(); }).then(v => L(JSON.stringify(v))); tick(10, 't10');`,
  `async function* g() { try { yield 1; } finally { return 'override'; } } var it = g(); it.next().then(() => it.return('r')).then(v => L(JSON.stringify(v))); tick(8, 't8');`,
  `async function* g() { try { yield 1; } finally { throw new Error('fin'); } } var it = g(); it.next().then(() => it.return('r')).then(v => L(JSON.stringify(v)), e => L('e ' + e.message)); tick(8, 't8');`,
  `async function* g() { var x = yield 1; L('got ' + x); } var it = g(); it.next('ignored').then(v => L(JSON.stringify(v))); it.next('second').then(v => L(JSON.stringify(v))); tick(6, 't6');`,
  `async function* g() { yield* [Promise.resolve(1)]; } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(5, 't5');`,
  `async function* g() { yield Promise.resolve(Promise.resolve(1)); } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(5, 't5');`,
  `async function* g() { yield (async () => 1)(); } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(5, 't5');`,
  `async function* g() { yield await Promise.resolve(1); } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(5, 't5');`,
  `async function* g() { yield thenable(1, 'y'); } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(6, 't6');`,
  `async function* g() { return thenable(1, 'r'); } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(6, 't6');`,
  `async function* g() { await thenable(1, 'a'); yield 2; } var it = g(); it.next().then(v => L(JSON.stringify(v))); tick(6, 't6');`,
  `var AG = Object.getPrototypeOf(async function* () {}).prototype; var it = (async function* () { yield 1; })(); L(String(Object.getPrototypeOf(it) !== AG)); L(typeof AG.next);`,
  `(async () => { await Promise.resolve(); L('top'); })(); Promise.resolve().then(() => L('p0')); L('sync');`
);

// Amostra determinística por hash (sampleByHash) do conjunto candidato inteiro; só depois saem os que os goldens vizinhos já têm.
const TARGET = 500;
const selectedPrograms = sampleByHash(programs, TARGET).filter((source) => !existing.has(originalProgram(source)));

// Executa cada programa no bun, num processo próprio, como arquivo (ver async-golden.js).
measureBodies(selectedPrograms, "microtask_case.js").then((lines) => {
  process.stdout.write(emitFactoredLines("microtask", lines));
  process.stderr.write(`${selectedPrograms.length} de ${programs.length} programas\n`);
});
