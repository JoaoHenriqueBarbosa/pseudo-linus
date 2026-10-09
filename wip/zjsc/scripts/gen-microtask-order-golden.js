// Gera tests/golden/microtask_order_bun.tsv: ordem de microtarefas com thenables e promises, medida no bun 1.4.2.
// Cobre await de thenable com getter de `then`, `then` que chama resolve duas vezes ou lança depois de resolver,
// Promise.resolve e await de promise subclasse (getter de `constructor` e de Symbol.species observados), await em
// promise nativa vs thenable (número de turnos), async generator com yield/return de promise, thenable e rejeição,
// filas de next/return/throw, e for await sobre iterador síncrono com promises rejeitadas.
// Formato fatorado (scripts/golden-prelude.js): o prelúdio define `L`, `tick`, `thenable`, `ok` e `bad`; o log da
// ordem é a string global `R` (cada L acrescenta "x;"). O programa é um arquivo (o bun transpila o fonte quando roda
// arquivo), num bun novo por programa (no máximo 6 ao mesmo tempo), sem API de host dentro do programa; o preload só lê
// `R` no `exit`, com as microtarefas esvaziadas. Programa que lança de forma síncrona é descartado (código de saída != 0).
// Programas que já estão nos goldens vizinhos de promise/async/microtask são descartados.
// Uso: bun scripts/gen-microtask-order-golden.js > tests/golden/microtask_order_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const { asyncGolden, ORDER_HARNESS: HARNESS } = require("./async-golden.js");
const { usesHostApi } = require("./host-api.js");

// O programa é um arquivo (o bun transpila o fonte quando roda arquivo): o harness e o corpo, sem `try`; o log é a string
// global `R`. O arquivo se chama `microtask_order_case.js` dos dois lados.
const { knownBodies, measureBodies } = asyncGolden({ harness: HARNESS, catchErrors: false, own: "microtask_order_bun.tsv" });
const existing = knownBodies((name) => /(async|promise|microtask)/.test(name) && name !== "microtask_order_bun.tsv");
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (usesHostApi(source) || usesHostApi(HARNESS)) throw new Error("programa com API de host: " + source);
    if (!seen.has(source) && !existing.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

// Observadores: marcam em que turno de microtarefa cada coisa acontece.
const OBS = {
  none: "",
  chain: " Promise.resolve().then(() => L('o1')).then(() => L('o2')).then(() => L('o3')).then(() => L('o4')).then(() => L('o5')).then(() => L('o6'));",
  asyncObs: " (async () => { await null; L('o1'); await null; L('o2'); await null; L('o3'); await null; L('o4'); await null; L('o5'); })();",
};
const END = " tick(16, 't16');";
const obsOf = (names) => names.map((n) => OBS[n]);

// 1. Thenables: getter de `then`, resolve duas vezes, throw depois de resolver.
const thenables = {
  // getter de then
  getterCount: "{ get then() { L('get'); return function (res) { L('call'); res(5); }; } }",
  getterAsync: "{ get then() { L('get'); return function (res) { L('call'); Promise.resolve().then(() => res(5)); }; } }",
  getterNotFn: "{ get then() { L('get'); return 5; } }",
  getterNull: "{ get then() { L('get'); return null; } }",
  getterUndefined: "{ get then() { L('get'); return undefined; } }",
  getterObject: "{ get then() { L('get'); return {}; } }",
  getterTwice: "(() => { var n = 0; return { get then() { n++; L('get' + n); return n === 1 ? function (res) { res('first'); } : function (res) { res('second'); }; } }; })()",
  getterProto: "Object.create({ get then() { L('pget'); return function (res) { res(5); }; } })",
  getterThrows: "{ get then() { L('get'); throw new Error('tg'); } }",
  getterProxy: "new Proxy({}, { get(t, k) { L('trap:' + String(k)); return k === 'then' ? function (res) { res(5); } : undefined; } })",
  getterRej: "{ get then() { L('get'); return function (res, rej) { rej(new Error('r')); }; } }",
  getterNativeThen: "{ get then() { L('get'); return Promise.prototype.then; } }",
  getterBound: "{ get then() { L('get'); var p = Promise.resolve(7); return p.then.bind(p); } }",
  getterSwapsOnCall: "(() => { var o = { get then() { L('get'); delete this.then; this.then = function () { L('late'); }; return function (res) { L('first'); res(1); }; } }; return o; })()",
  getterOtherKeys: "new Proxy({ then(res) { res(1); } }, { get(t, k, r) { L('trap:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { L('has:' + String(k)); return k in t; } })",
  // resolve duas vezes
  resTwice: "{ then(res, rej) { res(1); res(2); L('after'); } }",
  resRej: "{ then(res, rej) { res(1); rej(new Error('late')); } }",
  rejRes: "{ then(res, rej) { rej(new Error('first')); res(2); } }",
  rejRej: "{ then(res, rej) { rej(new Error('a')); rej(new Error('b')); } }",
  resResAsync: "{ then(res) { res(1); Promise.resolve().then(() => res(2)); } }",
  resThenableThenRes: "{ then(res) { res(thenable(1, 'in')); res(2); } }",
  resPromiseThenRes: "{ then(res) { res(Promise.resolve(1)); res(2); } }",
  resPendingThenRej: "{ then(res, rej) { res(new Promise(() => {})); rej(new Error('x')); } }",
  resRejPromiseThenRes: "{ then(res) { res(Promise.reject(new Error('inner'))); res(2); } }",
  resUndefined: "{ then(res) { res(); } }",
  resNoThis: "{ then(res) { res.call(null, 3); } }",
  resFnShape: "{ then(res, rej) { L(res.length + ':' + rej.length + ':' + res.name + ':' + typeof res.prototype); res(1); } }",
  resThisValue: "{ then(res) { L(String(this === undefined) + typeof this); res(1); } }",
  resSameFns: "{ then(res, rej) { L(String(res === rej)); res(1); } }",
  resTwiceLater: "{ then(res) { Promise.resolve().then(() => res(1)).then(() => res(2)); } }",
  resNested: "{ then(res) { res({ then(r2) { L('inner'); r2(9); } }); } }",
  resNestedGetter: "{ then(res) { res({ get then() { L('iget'); return function (r2) { r2(9); }; } }); } }",
  // lança depois de resolver
  thrAfterRes: "{ then(res) { res(1); throw new Error('after'); } }",
  thrAfterRej: "{ then(res, rej) { rej(new Error('r')); throw new Error('after'); } }",
  thrBefore: "{ then() { throw new Error('before'); } }",
  thrAfterAsyncRes: "{ then(res) { Promise.resolve().then(() => res(1)); throw new Error('sync'); } }",
  thrAfterResThenable: "{ then(res) { res(thenable(1, 'in')); throw new Error('after'); } }",
  thrAfterResPromise: "{ then(res) { res(Promise.resolve(1)); throw new Error('after'); } }",
  thrAfterResPending: "{ then(res) { res(new Promise(() => {})); throw new Error('after'); } }",
  thrInAsyncPart: "{ then(res) { Promise.resolve().then(() => { res(1); throw new Error('inner'); }); } }",
  thrNonError: "{ then() { throw 7; } }",
  thrAfterResLogged: "{ then(res) { L('in'); res(1); L('mid'); throw new Error('after'); } }",
  thrAfterRejPromise: "{ then(res, rej) { rej(Promise.resolve(1)); throw new Error('after'); } }",
};
const thenableContexts = {
  awaitPlain: (x) => `(async () => { var v = await ${x}; L('got:' + v); })().then(ok, bad);`,
  awaitCatch: (x) => `(async () => { try { var v = await ${x}; L('got:' + v); } catch (e) { L('c:' + (e && e.message)); } })();`,
  retPlain: (x) => `(async () => { return ${x}; })().then(ok, bad);`,
  retAwait: (x) => `(async () => { return await ${x}; })().then(ok, bad);`,
  agYield: (x) => `var it = (async function* () { yield ${x}; L('after'); })(); it.next().then(ok, bad); it.next().then(ok, bad);`,
  agReturn: (x) => `var it = (async function* () { return ${x}; })(); it.next().then(ok, bad); it.next().then(ok, bad);`,
  newPromise: (x) => `new Promise((r) => r(${x})).then(ok, bad);`,
  promiseResolve: (x) => `Promise.resolve(${x}).then(ok, bad);`,
  promiseAll: (x) => `Promise.all([${x}]).then(ok, bad);`,
  forAwait: (x) => `(async () => { for await (var v of [${x}]) L('it:' + v); })().then(ok, bad);`,
  thenCallback: (x) => `Promise.resolve().then(() => ${x}).then(ok, bad);`,
  yieldStarSync: (x) => `(async function* () { yield* [${x}]; })().next().then(ok, bad);`,
  finallyReturn: (x) => `Promise.resolve(1).finally(() => ${x}).then(ok, bad);`,
};
for (const x of Object.values(thenables)) {
  for (const c of Object.values(thenableContexts)) {
    for (const o of obsOf(["none", "chain", "asyncObs"])) add(c(x) + o + END);
  }
}

// 2. Promise subclasse e promises com `constructor`/`then` observados.
const val = (C, k) => (k === "ok" ? `${C}.resolve(5)` : `${C}.reject(new Error('r'))`);
const settleFn = (k) => (k === "ok" ? "res(5)" : "rej(new Error('r'))");
const promiseShapes = {
  native: (k) => val("Promise", k),
  pending: (k) => `new Promise((res, rej) => Promise.resolve().then(() => ${settleFn(k)}))`,
  sub: (k) => `(() => { class P extends Promise {} return ${val("P", k)}; })()`,
  subCtorLog: (k) => `(() => { class P extends Promise { constructor(ex) { L('P.new'); super(ex); } } return ${val("P", k)}; })()`,
  subSpeciesPromise: (k) => `(() => { class P extends Promise { static get [Symbol.species]() { L('species'); return Promise; } } return ${val("P", k)}; })()`,
  subSpeciesUndef: (k) => `(() => { class P extends Promise { static get [Symbol.species]() { L('species'); return undefined; } } return ${val("P", k)}; })()`,
  subSpeciesNull: (k) => `(() => { class P extends Promise { static get [Symbol.species]() { L('species'); return null; } } return ${val("P", k)}; })()`,
  subSpeciesThrows: (k) => `(() => { class P extends Promise { static get [Symbol.species]() { L('species'); throw new Error('sp'); } } return ${val("P", k)}; })()`,
  subThenLog: (k) => `(() => { class P extends Promise { then(a, b) { L('P.then'); return super.then(a, b); } } return ${val("P", k)}; })()`,
  ctorGetterPromise: (k) => `(() => { var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor'); return Promise; } }); return p; })()`,
  ctorGetterObject: (k) => `(() => { var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor'); return Object; } }); return p; })()`,
  ctorGetterUndef: (k) => `(() => { var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor'); return undefined; } }); return p; })()`,
  ctorGetterThrows: (k) => `(() => { var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor'); throw new Error('cg'); } }); return p; })()`,
  ctorGetterSub: (k) => `(() => { class P extends Promise {} var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor'); return P; } }); return p; })()`,
  ctorGetterOnce: (k) => `(() => { var n = 0; var p = ${val("Promise", k)}; Object.defineProperty(p, 'constructor', { get() { L('ctor' + (++n)); return n === 1 ? Promise : Object; } }); return p; })()`,
  ctorValueObject: (k) => `(() => { var p = ${val("Promise", k)}; p.constructor = Object; return p; })()`,
  ctorValuePromise: (k) => `(() => { var p = ${val("Promise", k)}; p.constructor = Promise; return p; })()`,
  ctorCustomSpecies: (k) => `(() => { var p = ${val("Promise", k)}; p.constructor = { [Symbol.species]: function (ex) { L('sp'); ex(() => {}, () => {}); } }; return p; })()`,
  thenOwn: (k) => `(() => { var p = ${val("Promise", k)}; p.then = function (a, b) { L('own.then'); return Promise.prototype.then.call(this, a, b); }; return p; })()`,
  thenOwnAndCtor: (k) => `(() => { var p = ${val("Promise", k)}; p.then = function (a, b) { L('own.then'); return Promise.prototype.then.call(this, a, b); }; Object.defineProperty(p, 'constructor', { get() { L('ctor'); return Promise; } }); return p; })()`,
  protoObject: (k) => `(() => { var p = ${val("Promise", k)}; Object.setPrototypeOf(p, Object.create(Promise.prototype)); return p; })()`,
  protoThenLog: (k) => `(() => { var p = ${val("Promise", k)}; Object.setPrototypeOf(p, Object.create(Promise.prototype, { then: { value: function (a, b) { L('proto.then'); return Promise.prototype.then.call(this, a, b); } } })); return p; })()`,
};
const promiseOps = {
  await: (p) => `(async () => { try { var v = await ${p}; L('got:' + v); } catch (e) { L('c:' + (e && e.message)); } })();`,
  resolve: (p) => `var q = ${p}; var r = Promise.resolve(q); L('same:' + (q === r)); r.then(ok, bad);`,
  subResolve: (p) => `var q = ${p}; class S extends Promise {} var r = S.resolve(q); L('same:' + (q === r) + ':' + (r instanceof S)); r.then(ok, bad);`,
  all: (p) => `Promise.all([${p}]).then(ok, bad);`,
  race: (p) => `Promise.race([${p}]).then(ok, bad);`,
  allSettled: (p) => `Promise.allSettled([${p}]).then((r) => L(JSON.stringify(r.map((x) => x.status))), bad);`,
  any: (p) => `Promise.any([${p}]).then(ok, bad);`,
  then: (p) => `var q = ${p}; var r = q.then(ok, bad); L('inst:' + (r instanceof Promise)); r.then(() => L('t2'));`,
  finally: (p) => `${p}.finally(() => L('fin')).then(ok, bad);`,
  catch: (p) => `${p}.catch(bad).then(() => L('t2'));`,
  retAsync: (p) => `(async () => { return ${p}; })().then(ok, bad);`,
  agYield: (p) => `var it = (async function* () { yield ${p}; })(); it.next().then(ok, bad);`,
  newPromise: (p) => `new Promise((r) => r(${p})).then(ok, bad);`,
  forAwait: (p) => `(async () => { for await (var v of [${p}]) L('it:' + v); })().then(ok, bad);`,
  thenCallback: (p) => `Promise.resolve().then(() => ${p}).then(ok, bad);`,
};
for (const shape of Object.values(promiseShapes)) {
  for (const k of ["ok", "bad"]) {
    for (const [opName, op] of Object.entries(promiseOps)) {
      add(op(shape(k)) + OBS.chain + END);
      if (opName === "await") add(op(shape(k)) + OBS.asyncObs + END);
    }
  }
}

// 3. Número de turnos: await em promise nativa vs thenable, em pares e contra tick(n).
const awaitables = {
  value: "5",
  nativeOk: "Promise.resolve(5)",
  nativeBad: "Promise.reject(new Error('r'))",
  nativePending: "new Promise((r) => Promise.resolve().then(() => r(5)))",
  nativeNested: "Promise.resolve(Promise.resolve(5))",
  newNested: "new Promise((r) => r(Promise.resolve(5)))",
  thenSync: "thenable(5, 't')",
  thenAsync: "{ then(res) { L('then'); Promise.resolve().then(() => res(5)); } }",
  thenResolvesNative: "{ then(res) { L('then'); res(Promise.resolve(5)); } }",
  thenResolvesThenable: "{ then(res) { L('then'); res(thenable(5, 'in')); } }",
  subclass: "(() => { class P extends Promise {} return P.resolve(5); })()",
  asyncValue: "(async () => 5)()",
  asyncReturnsPromise: "(async () => Promise.resolve(5))()",
  asyncReturnsThenable: "(async () => thenable(5, 'a'))()",
  asyncThrows: "(async () => { throw new Error('at'); })()",
};
const pairs = Object.entries(awaitables);
for (const [xn, x] of pairs) {
  for (let n = 1; n <= 10; n++) {
    add(`(async () => { try { await ${x}; L('done'); } catch (e) { L('c'); } })(); tick(${n}, 't${n}');`);
    add(`(async () => { return ${x}; })().then(() => L('done'), () => L('c')); tick(${n}, 't${n}');`);
    add(`(async () => { await ${x}; await ${x.replace(/'a'/g, "'b'")}; L('done'); })().catch(() => L('c')); tick(${n + 4}, 't${n + 4}');`);
  }
  for (const [yn, y] of pairs) {
    add(`(async () => { try { await ${x}; } catch (e) {} L('A'); })(); (async () => { try { await ${y}; } catch (e) {} L('B'); })();` + END);
    add(`(async () => { try { return ${x}; } catch (e) {} })().then(() => L('A'), () => L('A')); (async () => { try { return ${y}; } catch (e) {} })().then(() => L('B'), () => L('B'));` + END);
    add(`Promise.resolve(${x}).then(() => L('A'), () => L('A')); (async () => { try { await ${y}; } catch (e) {} L('B'); })();` + END);
  }
}

// 4. Async generators: yield/return de promise, thenable e rejeição, filas de next/return/throw.
const genBodies = {
  yieldNative: "yield Promise.resolve(1); yield 2;",
  yieldBad: "try { yield Promise.reject(new Error('yr')); } catch (e) { L('caught:' + e.message); yield 'rec'; }",
  yieldThenable: "yield thenable(1, 'th'); yield 2;",
  yieldThenableBad: "try { yield { then(res, rej) { rej(new Error('tr')); } }; } catch (e) { L('caught:' + e.message); }",
  yieldAwait: "yield await Promise.resolve(1);",
  yieldNested: "yield Promise.resolve(Promise.resolve(1));",
  yieldStarAsync: "yield* (async function* () { yield 1; yield Promise.resolve(2); })();",
  yieldStarSync: "yield* [Promise.resolve(1), 2];",
  yieldStarSyncRej: "try { yield* [Promise.reject(new Error('sr')), 2]; } catch (e) { L('caught:' + e.message); }",
  yieldStarThenable: "yield* [thenable(1, 'ys'), 2];",
  returnPromise: "return Promise.resolve('r');",
  returnBad: "return Promise.reject(new Error('rr'));",
  returnThenable: "return thenable('r', 'rt');",
  returnAwait: "return await Promise.resolve('r');",
  tryFinallyYield: "try { yield 1; } finally { L('fin'); }",
  finallyYieldAwait: "try { yield 1; } finally { await null; L('fin'); }",
  finallyReturnsPromise: "try { yield 1; } finally { return Promise.resolve('fr'); }",
  tryFinallyReturn: "try { return Promise.resolve('x'); } finally { L('fin'); }",
  tryFinallyReturnAwait: "try { return await Promise.resolve('x'); } finally { L('fin'); }",
  awaitInside: "L('b'); await null; L('a'); yield 1; await null; L('a2');",
  yieldUndefined: "yield; yield;",
  throwInside: "yield 1; throw new Error('gt');",
  thrownPromiseYield: "try { yield 1; } catch (e) { L('in:' + e.message); yield Promise.resolve('after'); }",
};
const genDrivers = {
  nextThree: "var it = g(); it.next().then(ok, bad); it.next().then(ok, bad); it.next().then(ok, bad);",
  nextAwaited: "var it = g(); (async () => { for (var i = 0; i < 3; i++) { try { var r = await it.next(); L(JSON.stringify(r)); } catch (e) { L('E:' + e.message); } } })();",
  returnFirst: "var it = g(); it.return(Promise.resolve('rv')).then(ok, bad); it.next().then(ok, bad);",
  returnAfterStart: "var it = g(); it.next().then(ok, bad); it.return('rv').then(ok, bad); it.next().then(ok, bad);",
  returnThenable: "var it = g(); it.next().then(ok, bad); it.return(thenable('R', 'rt')).then(ok, bad);",
  returnBadPromise: "var it = g(); it.next().then(ok, bad); it.return(Promise.reject(new Error('rj'))).then(ok, bad);",
  returnFirstBad: "var it = g(); it.return(Promise.reject(new Error('rj'))).then(ok, bad); it.next().then(ok, bad);",
  throwFirst: "var it = g(); it.throw(new Error('t0')).then(ok, bad); it.next().then(ok, bad);",
  throwAfter: "var it = g(); it.next().then(ok, bad); it.throw(new Error('t1')).then(ok, bad); it.next().then(ok, bad);",
  forAwait: "(async () => { try { for await (var v of g()) L('v:' + v); } catch (e) { L('E:' + e.message); } })();",
  forAwaitBreak: "(async () => { for await (var v of g()) { L('v:' + v); break; } L('out'); })().catch(bad);",
  fromAsync: "Array.fromAsync(g()).then(ok, bad);",
};
for (const body of Object.values(genBodies)) {
  for (const driver of Object.values(genDrivers)) {
    for (const o of obsOf(["none", "chain"])) add(`async function* g() { ${body} } ${driver}${o}${END}`);
  }
}

// 5. return em async generator com cada tipo de valor.
const returnShapes = {
  plain: (x) => `return ${x};`,
  awaited: (x) => `return await ${x};`,
  inTry: (x) => `try { return ${x}; } finally { L('fin'); }`,
  inTryAwait: (x) => `try { return await ${x}; } finally { L('fin'); }`,
  afterYield: (x) => `yield 1; return ${x};`,
  yieldThen: (x) => `yield ${x}; return 'end';`,
};
const returnDrivers = {
  next: "var it = g(); it.next().then(ok, bad); it.next().then(ok, bad);",
  nextTwice: "var it = g(); it.next().then(ok, bad); it.next().then(ok, bad); it.next().then(ok, bad);",
  returnValue: "var it = g(); it.next().then(ok, bad); it.return(7).then(ok, bad);",
  forAwait: "(async () => { try { for await (var v of g()) L('v:' + v); L('end'); } catch (e) { L('E:' + e.message); } })();",
};
for (const [, x] of pairs) {
  for (const shape of Object.values(returnShapes)) {
    for (const driver of Object.values(returnDrivers)) {
      for (const o of obsOf(["none", "chain"])) add(`async function* g() { ${shape(x)} } ${driver}${o}${END}`);
    }
  }
}

// 6. for await sobre iterador síncrono com promises rejeitadas.
const syncSources = {
  arrPromises: "[Promise.resolve(1), Promise.resolve(2)]",
  rejFirst: "[Promise.reject(new Error('a')), Promise.resolve(2)]",
  rejMiddle: "[Promise.resolve(1), Promise.reject(new Error('b')), Promise.resolve(3)]",
  rejLast: "[Promise.resolve(1), Promise.reject(new Error('c'))]",
  allRej: "[Promise.reject(new Error('a')), Promise.reject(new Error('b'))]",
  mixedValues: "[1, Promise.resolve(2), thenable(3, 'x'), 4]",
  thenableRej: "[1, { then(res, rej) { rej(new Error('tr')); } }, 3]",
  thenableThrows: "[1, { then() { throw new Error('tt'); } }, 3]",
  genFinally: "(function* () { try { yield Promise.resolve(1); yield Promise.reject(new Error('gr')); yield 3; } finally { L('gfin'); } })()",
  genPlainFinally: "(function* () { try { yield 1; yield 2; } finally { L('gfin'); } })()",
  customReturnLogs: "{ [Symbol.iterator]() { var i = 0; return { next() { L('n' + i); i++; return i <= 3 ? { value: i === 2 ? Promise.reject(new Error('cr')) : Promise.resolve(i), done: false } : { done: true, value: 'end' }; }, return(v) { L('return'); return {}; } }; } }",
  customReturnThrows: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return i <= 3 ? { value: i === 2 ? Promise.reject(new Error('cr')) : i, done: false } : { done: true }; }, return() { L('return'); throw new Error('retthrow'); } }; } }",
  customReturnPromise: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return i <= 3 ? { value: i === 2 ? Promise.reject(new Error('cr')) : i, done: false } : { done: true }; }, return() { L('return'); return Promise.resolve({}); } }; } }",
  customReturnNonObject: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return i <= 3 ? { value: i === 2 ? Promise.reject(new Error('cr')) : i, done: false } : { done: true }; }, return() { L('return'); return 1; } }; } }",
  customNoReturn: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return i <= 3 ? { value: i === 2 ? Promise.reject(new Error('cr')) : i, done: false } : { done: true }; } }; } }",
  valueGetterLogs: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return { get done() { L('done' + i); return i > 2; }, get value() { L('value' + i); return Promise.resolve(i); } }; }, return() { L('return'); return {}; } }; } }",
  valueGetterThrows: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return { done: false, get value() { L('value' + i); throw new Error('vg'); } }; }, return() { L('return'); return {}; } }; } }",
  nextThrows: "{ [Symbol.iterator]() { return { next() { L('next'); throw new Error('nt'); }, return() { L('return'); return {}; } }; } }",
  doneIsPromise: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return { done: i > 2 ? Promise.resolve(true) : false, value: i }; }, return() { L('return'); return {}; } }; } }",
  doneValuePromise: "{ [Symbol.iterator]() { var i = 0; return { next() { i++; return i > 2 ? { done: true, value: Promise.reject(new Error('dv')) } : { done: false, value: i }; } }; } }",
  holes: "[, Promise.reject(new Error('h')), ,]",
  setOfPromises: "new Set([Promise.resolve(1), Promise.reject(new Error('s'))])",
  stringIter: "'ab'",
  nestedPromise: "[Promise.resolve(Promise.resolve(1)), Promise.resolve(thenable(2, 'n'))]",
};
const forAwaitBodies = {
  plain: (s) => `(async () => { for await (var v of ${s}) L('v:' + v); L('end'); })().then(ok, bad);`,
  catchInside: (s) => `(async () => { try { for await (var v of ${s}) L('v:' + v); L('end'); } catch (e) { L('c:' + (e && e.message)); } })();`,
  breakFirst: (s) => `(async () => { for await (var v of ${s}) { L('v:' + v); break; } L('out'); })().then(ok, bad);`,
  continueAll: (s) => `(async () => { for await (var v of ${s}) { L('v:' + v); continue; } L('out'); })().then(ok, bad);`,
  throwInBody: (s) => `(async () => { try { for await (var v of ${s}) { L('v:' + v); throw new Error('body'); } } catch (e) { L('c:' + e.message); } })();`,
  returnInBody: (s) => `(async () => { for await (var v of ${s}) { L('v:' + v); return 'ret'; } })().then(ok, bad);`,
  labelOuter: (s) => `(async () => { o: for (var i = 0; i < 2; i++) { for await (var v of ${s}) { L('v:' + v); continue o; } } L('out'); })().then(ok, bad);`,
  destructure: (s) => `(async () => { for await (var [a] of ${s}) L('a:' + a); })().then(ok, bad);`,
  fromAsync: (s) => `Array.fromAsync(${s}).then(ok, bad);`,
  yieldStarInAsyncGen: (s) => `var it = (async function* () { yield* ${s}; })(); it.next().then(ok, bad); it.next().then(ok, bad); it.next().then(ok, bad);`,
  promiseAllSync: (s) => `Promise.all(${s}).then(ok, bad);`,
};
for (const s of Object.values(syncSources)) {
  for (const b of Object.values(forAwaitBodies)) {
    for (const o of obsOf(["none", "chain"])) add(b(s) + o + END);
  }
}

// Executa cada programa como arquivo num bun próprio, no máximo 6 ao mesmo tempo (ver async-golden.js).
measureBodies(programs, "microtask_order_case.js", { jobs: 6 }).then((lines) => {
  const text = emitFactoredLines("microtask_order", lines);
  if (/\/home\/|\/tmp\//.test(text)) throw new Error("o golden vazou um caminho da máquina");
  process.stdout.write(text);
  process.stderr.write(`${lines.length} programas (de ${programs.length}; os que lançam de forma síncrona são descartados)\n`);
});
