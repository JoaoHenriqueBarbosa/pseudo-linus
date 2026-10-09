// Gera tests/golden/async_bun.tsv: programas de Promise, async/await, geradores, async geradores, iterator
// helpers e for-await, avaliados no bun. Cada programa registra eventos no array global `log` (e usa os
// auxiliares L, tick e thenable de tests/golden/async_bun_harness.js, o mesmo texto que
// tests/async_bun_golden.rs embute). Colunas: fonte, depois o JSON do log depois de esvaziar as
// microtarefas, ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona.
// Cada programa roda num processo bun próprio, com timeout. Uso: bun scripts/gen-async-golden.js > tests/golden/async_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/async_bun_harness.js"), "utf8");
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

// 1. then/catch/finally: origem x manipulador.
const origins = {
  resolved: "Promise.resolve(1)",
  rejected: "Promise.reject(1)",
  pending: "new Promise(r => Promise.resolve().then(() => r(1)))",
  lateReject: "new Promise((_, j) => Promise.resolve().then(() => j(1)))",
};
const handlers = {
  value: "x => { L('h' + x); return x + 1; }",
  throws: "x => { L('h' + x); throw new Error('boom'); }",
  promise: "x => { L('h' + x); return Promise.resolve(x + 10); }",
  rejectedPromise: "x => { L('h' + x); return Promise.reject(x + 20); }",
  thenableValue: "x => { L('h' + x); return thenable(x + 30, 'a'); }",
  none: "undefined",
  nonFunction: "5",
};
for (const [on, o] of Object.entries(origins)) {
  for (const [hn, h] of Object.entries(handlers)) {
    add(`${o}.then(${h}).then(v => L('ok' + v), e => L('err' + (e && e.message || e)));`);
    add(`${o}.catch(${h}).then(v => L('ok' + v), e => L('err' + (e && e.message || e)));`);
    add(`${o}.finally(() => { L('f'); return ${hn === "throws" ? "Promise.reject(9)" : hn === "promise" ? "Promise.resolve(9)" : "3"}; }).then(v => L('ok' + v), e => L('err' + e));`);
  }
}

// 2. Interleaving de cadeias de comprimentos diferentes.
for (let a = 1; a <= 4; a++) {
  for (let b = 1; b <= 4; b++) {
    add(`tick(${a}, 'A'); tick(${b}, 'B'); L('sync');`);
    add(`Promise.reject(0).catch(() => L('c1')); Promise.resolve().then(() => L('t1')); tick(${a}, 'x${a}'); tick(${b}, 'y${b}');`);
  }
}

// 3. Resolver com promise ou thenable: número de ticks.
for (let n = 1; n <= 5; n++) {
  add(`new Promise(r => r(Promise.resolve(1))).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`new Promise(r => r(thenable(1, 'x'))).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.resolve(Promise.resolve(1)).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`var p = Promise.resolve(1); Promise.resolve(p) === p && L('same'); p.then(() => L('a')); tick(${n}, 't${n}');`);
  add(`Promise.resolve().then(() => Promise.resolve(1)).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.resolve().then(() => thenable(1, 'q')).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.reject(1).catch(() => Promise.reject(2)).catch(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.resolve().finally(() => {}).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.resolve().finally(() => Promise.resolve()).then(() => L('outer')); tick(${n}, 't${n}');`);
  add(`Promise.reject(1).finally(() => {}).catch(() => L('outer')); tick(${n}, 't${n}');`);
}
add(
  "var p = new Promise(r => r(1)); r2 = null; new Promise(r => { r(p); r(2); r(3); }).then(v => L(v));",
  "new Promise((res, rej) => { res(1); rej(2); throw new Error('x'); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { throw new Error('first'); res(1); }).catch(e => L(e.message));",
  "var p = new Promise(r => setTimeoutLess = r); L(typeof setTimeoutLess);",
  "var self; var p = new Promise(r => Promise.resolve().then(() => r(self))); self = p; p.catch(e => L(e.name + ':' + e.message));",
  "var p = Promise.resolve(); var q = p.then(() => q); q.catch(e => L(e.name + ':' + e.message));",
  "var t = { then(res) { res(1); res(2); throw new Error('late'); } }; Promise.resolve(t).then(v => L(v));",
  "var t = { then() { throw new Error('sync'); } }; Promise.resolve(t).catch(e => L(e.message));",
  "var t = { get then() { L('get'); throw new Error('getter'); } }; Promise.resolve(t).catch(e => L(e.message));",
  "var t = { get then() { L('get'); return 5; } }; Promise.resolve(t).then(v => L(v === t));",
  "var count = 0; var t = { get then() { count++; return undefined; } }; Promise.resolve(t).then(() => L(count));",
  "var t = { then(a, b) { L(typeof a + typeof b); a.length; L(a.length + ',' + b.length); a(1); } }; Promise.resolve(t);",
  "Promise.resolve(1).then(); Promise.resolve(1).then(null, null).then(v => L(v));",
  "Promise.reject(new Error('e')).then().catch(e => L(e.message));",
  "Promise.resolve(1).then(2, 3).then(v => L(v));",
  "Promise.resolve(1).finally().then(v => L(v));",
  "Promise.resolve(1).finally(5).then(v => L(v));",
  "Promise.reject(1).finally(5).catch(v => L(v));",
  "Promise.resolve(1).finally(() => 2).then(v => L(v));",
  "Promise.resolve(1).finally(() => { throw 2; }).catch(v => L(v));",
  "Promise.resolve(1).finally(() => Promise.reject(2)).catch(v => L(v));",
  "Promise.reject(1).finally(() => Promise.resolve(2)).catch(v => L(v));",
  "L(Promise.prototype.finally.length + ',' + Promise.prototype.then.length + ',' + Promise.prototype.catch.length);",
  "L(Promise.length + ',' + Promise.all.length + ',' + Promise.withResolvers.length + ',' + Promise.try.length);",
  "try { Promise(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Promise(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Promise(1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.prototype.then.call({}, () => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.resolve.call(1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.all.call(undefined, []); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.prototype.finally.call(1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.withResolvers.call(1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.try(); L('no'); } catch (e) { L(e.name + ':' + e.message); }",
  "Promise.try(() => 5).then(v => L(v)); L('sync');",
  "Promise.try(() => { throw 5; }).catch(v => L(v)); L('sync');",
  "Promise.try((a, b) => a + b, 1, 2).then(v => L(v));",
  "Promise.try(() => Promise.resolve(7)).then(v => L(v)); tick(2, 't2'); tick(3, 't3');",
  "Promise.try(function () { L(this === undefined || this === globalThis ? 'ok' : 'bad'); });",
  "var { promise, resolve, reject } = Promise.withResolvers(); promise.then(v => L(v)); resolve(1); resolve(2); reject(3);",
  "var { promise, reject } = Promise.withResolvers(); promise.catch(v => L(v)); reject(3);",
  "L(Object.keys(Promise.withResolvers()).join());",
  "class P extends Promise {} var p = P.resolve(1); L(p instanceof P); L(p.then(() => {}) instanceof P);",
  "class P extends Promise { static get [Symbol.species]() { return Promise; } } L(P.resolve(1).then(() => {}) instanceof P);",
  "class P extends Promise { constructor(ex) { L('ctor'); super(ex); } } P.resolve(1).then(() => L('t'));",
  "class P extends Promise { then(a, b) { L('then'); return super.then(a, b); } } P.resolve(1).finally(() => L('f'));",
  "L(Object.prototype.toString.call(Promise.resolve()));",
  "L(String(Promise[Symbol.species] === Promise));",
  "L(typeof Promise.prototype[Symbol.toStringTag]);"
);

// 4. Combinadores.
const inputs = {
  empty: "[]",
  values: "[1, 2, 3]",
  promises: "[Promise.resolve(1), Promise.resolve(2)]",
  mixed: "[1, Promise.resolve(2), thenable(3, 'm')]",
  oneRejected: "[Promise.resolve(1), Promise.reject(2), Promise.resolve(3)]",
  allRejected: "[Promise.reject(1), Promise.reject(2)]",
  late: "[new Promise(r => Promise.resolve().then(() => r('late'))), 'early']",
  lateReject: "[new Promise((_, j) => Promise.resolve().then(() => j('late'))), Promise.reject('early')]",
  pending: "[new Promise(() => {}), Promise.resolve(5)]",
  string: "'ab'",
  set: "new Set([1, 2])",
  generator: "(function* () { L('gen'); yield 1; yield Promise.resolve(2); })()",
};
const show = "v => L(JSON.stringify(v))";
const showErr = "e => L('E:' + (e && e.name === 'AggregateError' ? e.name + ':' + JSON.stringify(e.errors) + ':' + e.message : JSON.stringify(e)))";
for (const [kind, input] of Object.entries(inputs)) {
  for (const comb of ["all", "allSettled", "any", "race"]) {
    add(`Promise.${comb}(${input}).then(${show}, ${showErr}); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');`);
  }
}
for (const comb of ["all", "allSettled", "any", "race"]) {
  add(
    `Promise.${comb}(1).then(${show}, e => L(e.name + ':' + e.message));`,
    `Promise.${comb}().then(${show}, e => L(e.name + ':' + e.message));`,
    `Promise.${comb}(null).then(${show}, e => L(e.name + ':' + e.message));`,
    `Promise.${comb}({}).then(${show}, e => L(e.name + ':' + e.message));`,
    `Promise.${comb}([1], 2).then(${show}, ${showErr});`,
    `var r = Promise.${comb}([]); L(r instanceof Promise);`,
    `var resolveCalls = 0; var orig = Promise.resolve; Promise.resolve = function (v) { resolveCalls++; return orig.call(this, v); }; Promise.${comb}([1, 2]).then(() => L(resolveCalls));`,
    `Promise.resolve = 5; Promise.${comb}([1]).then(${show}, e => L(e.name + ':' + e.message));`,
    `var it = { [Symbol.iterator]() { return { next() { throw new Error('next'); }, return() { L('ret'); return {}; } }; } }; Promise.${comb}(it).catch(e => L(e.message));`,
    `var it = { [Symbol.iterator]() { var i = 0; return { next() { i++; return { done: i > 2, value: i }; }, return() { L('ret'); return {}; } }; } }; Promise.${comb}(it).then(${show}, ${showErr});`,
    `Promise.resolve = function () { throw new Error('resolveThrow'); }; var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('ret'); return {}; } }; } }; Promise.${comb}(it).catch(e => L(e.message));`,
    `var thens = 0; var p = Promise.resolve(1); p.then = function (a, b) { thens++; return Promise.prototype.then.call(this, a, b); }; Promise.${comb}([p]).then(() => L(thens));`
  );
}
add(
  "Promise.race([tick(1, 'a'), tick(2, 'b')]).then(() => L('race'));",
  "Promise.race([new Promise(r => setTimeoutStub = r), 1]).then(v => L(v));",
  "Promise.all([1, 2, 3]).then(v => L('all')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "Promise.all([]).then(v => L('all')); tick(1, 't1'); tick(2, 't2');",
  "Promise.allSettled([]).then(v => L('as')); tick(1, 't1'); tick(2, 't2');",
  "Promise.any([1]).then(v => L('any')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "Promise.race([1]).then(v => L('race')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "Promise.all([Promise.resolve(1)]).then(v => L('all')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "Promise.any([Promise.reject(1)]).catch(v => L('any')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "var e = null; Promise.any([]).catch(x => { e = x; L(Object.getOwnPropertyNames(e).join()); L(e.message); });",
  "Promise.any([Promise.reject(1)]).catch(e => { L(e.message); L(e.errors.length); L(e instanceof AggregateError); });",
  "Promise.allSettled([Promise.reject(new Error('x')), 2]).then(r => L(r.map(s => s.status + ':' + (s.reason ? s.reason.message : s.value)).join()));",
  "Promise.allSettled([1]).then(r => L(Object.keys(r[0]).join()));",
  "Promise.allSettled([Promise.reject(1)]).then(r => L(Object.keys(r[0]).join()));",
  "var order = []; Promise.all([1, 2, 3].map(x => tick(x, 'p' + x))).then(() => L('done'));",
  "Promise.all([tick(3, 'slow'), tick(1, 'fast')]).then(() => L('all'));",
  "Promise.race([tick(3, 'slow'), tick(1, 'fast')]).then(() => L('race'));"
);

// 5. async/await.
for (let n = 1; n <= 6; n++) {
  add(
    `(async () => { await null; L('a'); await null; L('b'); })(); tick(${n}, 't${n}');`,
    `(async () => { await Promise.resolve(); L('a'); })(); tick(${n}, 't${n}');`,
    `(async () => { await thenable(1, 'x'); L('a'); })(); tick(${n}, 't${n}');`,
    `(async () => { await new Promise(r => r(Promise.resolve())); L('a'); })(); tick(${n}, 't${n}');`,
    `(async () => { return Promise.resolve(1); })().then(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { return 1; })().then(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { return thenable(1, 'x'); })().then(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { throw 1; })().catch(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { await Promise.reject(1); })().catch(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { try { await Promise.reject(1); } catch (e) { L('c'); } L('after'); })(); tick(${n}, 't${n}');`,
    `(async () => { try { return await Promise.resolve(1); } finally { L('fin'); } })().then(() => L('r')); tick(${n}, 't${n}');`,
    `(async () => { try { return Promise.resolve(1); } finally { L('fin'); } })().then(() => L('r')); tick(${n}, 't${n}');`
  );
}
add(
  "async function a() { L('a1'); await b(); L('a2'); } async function b() { L('b1'); } a(); L('main');",
  "async function a() { L('a1'); await 1; L('a2'); } async function b() { L('b1'); await 2; L('b2'); } a(); b(); L('main');",
  "async function a() { L(1); await a2(); L(2); } async function a2() { L(3); await a3(); L(4); } async function a3() { L(5); } a(); Promise.resolve().then(() => L(6)).then(() => L(7)).then(() => L(8));",
  "async function f() { throw new Error('x'); } var p = f(); L(p instanceof Promise); p.catch(e => L(e.message)); L('sync');",
  "async function f() { null.x; } f().catch(e => L(e.name));",
  "async function f(a = (() => { throw new Error('param'); })()) {} var p = f(); L('sync'); p.catch(e => L(e.message));",
  "async function f(a, a2 = a.x) {} f(undefined).catch(e => L(e.name));",
  "async function f() { return this; } f.call(5).then(v => L(typeof v));",
  "var o = { async m() { return this === o; } }; o.m().then(v => L(v));",
  "var f = async () => this === globalThis || this === undefined; f().then(v => L(typeof v));",
  "async function f() { return arguments.length; } f(1, 2, 3).then(v => L(v));",
  "var f = async function () {}; L(Object.getPrototypeOf(f) === Object.getPrototypeOf(async function () {}));",
  "L(Object.prototype.toString.call(async function () {}));",
  "L(typeof (async function () {}).prototype);",
  "try { new (async function () {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var AsyncFunction = (async function () {}).constructor; L(AsyncFunction.name); new AsyncFunction('x', 'return await x')(3).then(v => L(v));",
  "var AsyncFunction = (async function () {}).constructor; try { AsyncFunction('await 1;', ''); L('ok'); } catch (e) { L(e.name); }",
  "async function f() { await { then(r) { L('t'); r(1); } }; L('after'); } f(); L('sync');",
  "async function f() { await { then(r, j) { j(new Error('rej')); } }; } f().catch(e => L(e.message));",
  "async function f() { var x = await { then() { throw new Error('thr'); } }; } f().catch(e => L(e.message));",
  "var p = Promise.resolve(1); p.constructor = function () { L('ctor'); }; (async () => { await p; L('after'); })();",
  "var p = Promise.resolve(1); var thens = 0; p.then = function () { thens++; return Promise.prototype.then.apply(this, arguments); }; (async () => { await p; L('after' + thens); })();",
  "class P extends Promise {} var p = P.resolve(1); var thens = 0; p.then = function () { thens++; return Promise.prototype.then.apply(this, arguments); }; (async () => { await p; L('after' + thens); })();",
  "async function f() { for (var i = 0; i < 3; i++) { await i; L(i); } } f(); tick(2, 't2'); tick(4, 't4');",
  "async function f() { await Promise.all([1, 2]); L('all'); } f(); tick(3, 't3'); tick(4, 't4');",
  "async function f() { var r = await Promise.race([tick(2, 'a'), tick(1, 'b')]); L('race'); } f();",
  "(async () => { var x = await 1; var y = await 2; L(x + y); })();",
  "(async () => { L(await (async () => { await null; return 1; })()); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "async function f() { return await (async () => { throw new Error('in'); })(); } f().catch(e => L(e.message)); tick(1, 't1'); tick(2, 't2');",
  "async function f() { await undefined; throw new Error('late'); } var p = f(); L('sync'); p.catch(e => L(e.message));",
  "async function f() { L('body'); } var p = f(); L('after call'); p.then(() => L('then'));",
  "var log2 = []; (async () => { try { await Promise.reject(new Error('a')); } catch (e) { L(e.message); throw new Error('b'); } finally { L('fin'); } })().catch(e => L(e.message));",
  "async function f() { await 1; } var p = f(); L(Object.getPrototypeOf(p) === Promise.prototype);",
  "var f = async x => x; f(Promise.resolve(2)).then(v => L(v)); tick(1, 't1'); tick(2, 't2');",
  "async function* g() {} L(Object.prototype.toString.call(g()));",
  "(async () => { await 1; L(new Error('stack').stack.split('\\n')[0]); })();",
  "async function f() { return { then(r) { L('thenable'); r(5); } }; } f().then(v => L(v)); L('sync');",
  "async function f() { return { get then() { L('get then'); return undefined; } }; } f().then(v => L(typeof v));"
);

// 6. Geradores.
add(
  "function* g() { L('start'); var x = yield 1; L('x' + x); var y = yield 2; L('y' + y); return 3; } var it = g(); L('created'); L(JSON.stringify(it.next('ignored'))); L(JSON.stringify(it.next('a'))); L(JSON.stringify(it.next('b'))); L(JSON.stringify(it.next('c')));",
  "function* g() { try { yield 1; yield 2; } finally { L('cleanup'); } } var it = g(); it.next(); L(JSON.stringify(it.return(9))); L(JSON.stringify(it.next()));",
  "function* g() { try { yield 1; } finally { yield 'f'; L('after f'); } } var it = g(); it.next(); L(JSON.stringify(it.return(9))); L(JSON.stringify(it.next())); L(JSON.stringify(it.next()));",
  "function* g() { try { yield 1; } catch (e) { L('caught ' + e); yield 'c'; } return 'r'; } var it = g(); it.next(); L(JSON.stringify(it.throw('E'))); L(JSON.stringify(it.next()));",
  "function* g() { yield 1; } var it = g(); try { it.throw(new Error('early')); } catch (e) { L(e.message); } L(JSON.stringify(it.next()));",
  "function* g() { yield 1; } var it = g(); L(JSON.stringify(it.return(5))); L(JSON.stringify(it.next()));",
  "function* g() { L('never'); } var it = g(); L(JSON.stringify(it.return(5))); L(JSON.stringify(it.next()));",
  "function* g() { var it2 = g2(); yield* it2; L('done'); } function* g2() { yield 1; return 2; } var it = g(); L(JSON.stringify([...it]));",
  "function* g2() { var x = yield 1; L('got ' + x); return 'ret'; } function* g() { var r = yield* g2(); L('r=' + r); yield r; } var it = g(); it.next(); L(JSON.stringify(it.next('A'))); L(JSON.stringify(it.next()));",
  "function* g() { yield* [1, 2]; yield* 'ab'; yield* new Set([3]); } L(JSON.stringify([...g()]));",
  "function* g() { yield* 5; } try { [...g()]; } catch (e) { L(e.name + ':' + e.message); }",
  "function* g() { yield* undefined; } try { g().next(); } catch (e) { L(e.name + ':' + e.message); }",
  "var inner = { [Symbol.iterator]() { return this; }, next(v) { L('next ' + v); return { done: false, value: 1 }; }, return(v) { L('return ' + v); return { done: true, value: 'rv' }; }, throw(e) { L('throw ' + e); return { done: true, value: 'tv' }; } }; function* g() { var r = yield* inner; L('r ' + r); return 'end'; } var it = g(); it.next('a'); it.next('b'); L(JSON.stringify(it.throw('E'))); ",
  "var inner = { [Symbol.iterator]() { return this; }, next() { return { done: false, value: 1 }; }, return(v) { L('return ' + v); return { done: true, value: 'rv' }; } }; function* g() { yield* inner; } var it = g(); it.next(); L(JSON.stringify(it.return('X')));",
  "var inner = { [Symbol.iterator]() { return this; }, next() { return { done: false, value: 1 }; }, return(v) { L('return ' + v); return { done: false, value: 'again' }; } }; function* g() { yield* inner; } var it = g(); it.next(); L(JSON.stringify(it.return('X')));",
  "var inner = { [Symbol.iterator]() { return this; }, next() { return { done: false, value: 1 }; } }; function* g() { yield* inner; } var it = g(); it.next(); try { it.throw(new Error('t')); } catch (e) { L(e.name + ':' + e.message); }",
  "var inner = { [Symbol.iterator]() { return this; }, next() { return { done: false, value: 1 }; }, return() { return 5; } }; function* g() { yield* inner; } var it = g(); it.next(); try { it.return(1); } catch (e) { L(e.name + ':' + e.message); }",
  "var inner = { [Symbol.iterator]() { return this; }, next() { return 5; } }; function* g() { yield* inner; } try { g().next(); } catch (e) { L(e.name + ':' + e.message); }",
  "function* g() { yield 1; } var it = g(); try { it.next.call({}); } catch (e) { L(e.name + ':' + e.message); }",
  "function* g() { it.next(); } var it = g(); try { it.next(); } catch (e) { L(e.name + ':' + e.message); }",
  "function* g() { try { it.return(1); } catch (e) { L(e.name + ':' + e.message); } } var it = g(); it.next();",
  "function* g() { yield this; } L(typeof g().next().value);",
  "function* g() { return arguments.length; } L(JSON.stringify(g(1, 2).next()));",
  "function* g(a = L('default')) { L('body'); } var it = g(); L('created'); it.next();",
  "function* g(a = (() => { throw new Error('p'); })()) {} try { g(); } catch (e) { L(e.message); }",
  "function* g() { yield 1; } L(Object.prototype.toString.call(g()) + Object.prototype.toString.call(g));",
  "function* g() {} L(Object.getPrototypeOf(g()) === g.prototype); L(g.prototype.constructor === undefined);",
  "function* g() {} try { new g; } catch (e) { L(e.name + ':' + e.message); }",
  "var o = { *g() { yield 1; } }; L(JSON.stringify([...o.g()]));",
  "class A { *[Symbol.iterator]() { yield 1; yield 2; } } L(JSON.stringify([...new A]));",
  "function* g() { var x = yield; L(typeof x); } var it = g(); it.next(); it.next();",
  "function* g() { yield yield 1; } var it = g(); L(JSON.stringify([it.next(), it.next('a'), it.next('b'), it.next('c')]));",
  "function* g() { for (var i = 0; i < 3; i++) yield i; } var s = 0; for (var v of g()) { s += v; if (v == 1) break; } L(s);",
  "function* g() { try { yield 1; yield 2; } finally { L('fin'); } } for (var v of g()) { break; } L('after');",
  "function* g() { try { yield 1; yield 2; } finally { L('fin'); } } try { for (var v of g()) { throw new Error('x'); } } catch (e) { L(e.message); }",
  "function* g() { try { yield 1; } finally { throw new Error('fin'); } } try { for (var v of g()) { break; } } catch (e) { L(e.message); }",
  "function* g() { var [a, b] = [yield 1, yield 2]; L(a + b); } var it = g(); it.next(); it.next(10); it.next(20);",
  "function* g() { const { x } = yield; L(x); } var it = g(); it.next(); it.next({ x: 7 });",
  "var [a, b] = (function* () { L('g1'); yield 1; L('g2'); yield 2; L('g3'); yield 3; })(); L(a + b);",
  "var it = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var [a] = it; L(a);",
  "L(JSON.stringify(Array.from((function* () { yield 1; yield 2; })(), x => x * 2)));",
  "function* g() { yield 1; return 2; } L(JSON.stringify([...g()])); L(JSON.stringify(Array.from(g())));",
  "function* fib() { var [a, b] = [0, 1]; for (;;) { yield a; [a, b] = [b, a + b]; } } var r = []; for (var x of fib()) { if (x > 50) break; r.push(x); } L(JSON.stringify(r));",
  "var g = function* () { yield 1; }; var it = g(); L(it[Symbol.iterator]() === it);",
  "var GF = Object.getPrototypeOf(function* () {}).constructor; L(GF.name); var it = new GF('a', 'yield a; yield a * 2')(4); L(JSON.stringify([...it]));",
  "L(Object.getPrototypeOf(function* () {}).constructor.length);",
  "function* g() { yield 1; } var proto = Object.getPrototypeOf(g.prototype); L(Object.getOwnPropertyNames(proto).sort().join());",
  "function* g() { return 1; } var it = g(); L(JSON.stringify(it.next())); L(JSON.stringify(it.throw === undefined)); try { it.throw(new Error('d')); } catch (e) { L(e.message); }"
);

// 7. Async geradores.
add(
  "async function* g() { L('start'); yield 1; yield 2; } var it = g(); L('created'); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v))); L('sync');",
  "async function* g() { var x = yield 1; L('x' + x); return 'r'; } var it = g(); it.next('a').then(v => L(JSON.stringify(v))); it.next('b').then(v => L(JSON.stringify(v)));",
  "async function* g() { yield Promise.resolve(1); yield thenable(2, 'y'); } var it = g(); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { yield Promise.reject(new Error('r')); } g().next().catch(e => L(e.message));",
  "async function* g() { try { yield Promise.reject(new Error('r')); } catch (e) { L('caught ' + e.message); yield 'c'; } } var it = g(); it.next().then(v => L(JSON.stringify(v)), e => L('E' + e.message)); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { return Promise.resolve(5); } g().next().then(v => L(JSON.stringify(v)));",
  "async function* g() { return Promise.reject(new Error('rr')); } g().next().catch(e => L(e.message));",
  "async function* g() { throw new Error('t'); } var it = g(); it.next().catch(e => L(e.message)); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); it.return(7).then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); it.return(Promise.resolve(7)).then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); it.return(Promise.reject(new Error('rj'))).catch(e => L(e.message));",
  "async function* g() { yield 1; } var it = g(); it.next(); it.return(7).then(v => L(JSON.stringify(v)));",
  "async function* g() { try { yield 1; } finally { L('fin'); } } var it = g(); it.next().then(() => it.return(8)).then(v => L(JSON.stringify(v)));",
  "async function* g() { try { yield 1; } finally { await null; L('fin'); } } var it = g(); it.next().then(() => it.return(8)).then(v => L(JSON.stringify(v)));",
  "async function* g() { try { yield 1; } finally { return 'override'; } } var it = g(); it.next().then(() => it.return(8)).then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); it.throw(new Error('t')).catch(e => L(e.message)); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { try { yield 1; } catch (e) { L('c' + e); yield 2; } } var it = g(); it.next().then(() => it.throw('E')).then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); try { it.next.call({}).catch(e => L(e.name + ':' + e.message)); } catch (e) { L('sync ' + e.name); }",
  "async function* g() { yield 1; } var it = g(); it.return.call(1).catch(e => L(e.name + ':' + e.message));",
  "async function* g() { yield 1; } var it = g(); it.throw.call(null).catch(e => L(e.name + ':' + e.message));",
  "async function* g() { L(await 1); L(yield 2); } var it = g(); it.next().then(v => L(JSON.stringify(v))); it.next('sent');",
  "async function* g() { await null; yield 1; await null; yield 2; } var it = g(); it.next().then(v => L('n1')); it.next().then(v => L('n2')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5'); tick(6, 't6');",
  "async function* g() { yield 1; yield 2; } var it = g(); it.next().then(v => L('n1')); it.next().then(v => L('n2')); it.next().then(v => L('n3')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');",
  "async function* g() { yield* [1, 2]; } var it = g(); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { yield* [Promise.resolve(1), 2]; } var it = g(); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g2() { yield 1; return 'r'; } async function* g() { var r = yield* g2(); L('r=' + r); yield r; } var it = g(); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v))); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g2() { try { yield 1; } finally { L('inner fin'); } } async function* g() { yield* g2(); } var it = g(); it.next().then(() => it.return('x')).then(v => L(JSON.stringify(v)));",
  "async function* g2() { try { yield 1; } catch (e) { L('inner ' + e); return 'handled'; } } async function* g() { var r = yield* g2(); yield r; } var it = g(); it.next().then(() => it.throw('T')).then(v => L(JSON.stringify(v)));",
  "var inner = { [Symbol.asyncIterator]() { return this; }, next(v) { L('next ' + v); return { done: false, value: 1 }; }, return(v) { L('return ' + v); return { done: true, value: 'rv' }; } }; async function* g() { yield* inner; } var it = g(); it.next('a').then(v => L(JSON.stringify(v))); it.next('b').then(v => L(JSON.stringify(v)));",
  "var inner = { [Symbol.asyncIterator]() { return this; }, next() { return Promise.resolve({ done: false, value: Promise.resolve(1) }); } }; async function* g() { yield* inner; } g().next().then(v => L(typeof v.value));",
  "async function* g() { yield* 5; } g().next().catch(e => L(e.name + ':' + e.message));",
  "async function* g() { yield* null; } g().next().catch(e => L(e.name + ':' + e.message));",
  "async function* g() { var x = yield* { [Symbol.iterator]() { return { next() { return { done: true, value: 'sv' }; } }; } }; L(x); } g().next().then(v => L(JSON.stringify(v)));",
  "L(Object.prototype.toString.call(async function* () {})); L(Object.prototype.toString.call((async function* () {})()));",
  "var AGF = Object.getPrototypeOf(async function* () {}).constructor; L(AGF.name);",
  "async function* g() {} var it = g(); L(it[Symbol.asyncIterator]() === it); L(typeof it[Symbol.iterator]);",
  "async function* g() {} try { new g; } catch (e) { L(e.name + ':' + e.message); }",
  "var o = { async *g() { yield this === o; } }; o.g().next().then(v => L(JSON.stringify(v)));",
  "class A { static async *[Symbol.asyncIterator]() { yield 1; yield 2; } } (async () => { for await (var v of A) L(v); })();",
  "async function* g() { var p = yield; L(typeof p); } var it = g(); it.next().then(() => it.next(1)).then(v => L(JSON.stringify(v)));",
  "async function* g() { var a = it.next(); L('inner'); yield 1; } var it = g(); it.next().then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; yield 2; yield 3; } var it = g(); Promise.all([it.next(), it.next(), it.next(), it.next()]).then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var it = g(); Promise.all([it.return('a'), it.next(), it.return('b')]).then(v => L(JSON.stringify(v)));",
  "async function* g() { try { yield 1; } finally { L('fin'); } } var it = g(); Promise.all([it.next(), it.return('a'), it.next()]).then(v => L(JSON.stringify(v)));",
  "async function* g() { yield 1; } var AsyncGenProto = Object.getPrototypeOf(g.prototype); L(Object.getOwnPropertyNames(AsyncGenProto).sort().join());"
);

// 8. for await e asyncFromSyncIterator.
add(
  "(async () => { for await (var v of [1, 2, 3]) L(v); L('done'); })(); L('sync');",
  "(async () => { for await (var v of [Promise.resolve(1), 2]) L(v); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "(async () => { try { for await (var v of [Promise.reject(new Error('r')), 2]) L(v); } catch (e) { L('c' + e.message); } })();",
  "(async () => { for await (var v of [thenable(1, 'x'), 2]) L(v); })();",
  "(async () => { for await (var v of 'ab') L(v); })();",
  "(async () => { for await (var v of new Set([1, 2])) L(v); })();",
  "(async () => { for await (var v of new Map([[1, 2]])) L(JSON.stringify(v)); })();",
  "(async () => { for await (var v of (function* () { yield 1; yield Promise.resolve(2); })()) L(v); })();",
  "(async () => { for await (var v of (async function* () { yield 1; yield 2; })()) L(v); })();",
  "(async () => { for await (var v of (async function* () { yield 1; yield 2; })()) { L(v); break; } L('after'); })();",
  "(async () => { for await (var v of (async function* () { try { yield 1; yield 2; } finally { L('fin'); } })()) { L(v); break; } L('after'); })();",
  "(async () => { try { for await (var v of (async function* () { try { yield 1; } finally { L('fin'); } })()) { throw new Error('body'); } } catch (e) { L(e.message); } })();",
  "(async () => { for await (var v of (function* () { try { yield 1; yield 2; } finally { L('fin'); } })()) { L(v); break; } L('after'); })();",
  "(async () => { try { for await (var v of 5) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { try { for await (var v of null) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { try { for await (var v of {}) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { try { for await (var v of { [Symbol.asyncIterator]: 1 }) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { try { for await (var v of { [Symbol.asyncIterator]() { return 1; } }) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { try { for await (var v of { [Symbol.asyncIterator]() { return { next() { return 1; } }; } }) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "(async () => { for await (var v of { [Symbol.asyncIterator]: undefined, [Symbol.iterator]() { return [7][Symbol.iterator](); } }) L(v); })();",
  "(async () => { for await (var v of { [Symbol.asyncIterator]: null, [Symbol.iterator]() { return [8][Symbol.iterator](); } }) L(v); })();",
  "var it = { [Symbol.asyncIterator]() { return { i: 0, next() { L('next'); return Promise.resolve({ done: this.i++ > 1, value: this.i }); }, return() { L('return'); return Promise.resolve({}); } }; } }; (async () => { for await (var v of it) L(v); })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: false, value: 1 }); }, return() { L('return'); return Promise.resolve({}); } }; } }; (async () => { for await (var v of it) { L(v); break; } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: false, value: 1 }); }, return() { L('return'); return 5; } }; } }; (async () => { try { for await (var v of it) { break; } } catch (e) { L(e.name + ':' + e.message); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: false, value: 1 }); }, return() { L('return'); throw new Error('retthrow'); } }; } }; (async () => { try { for await (var v of it) { throw new Error('body'); } } catch (e) { L(e.message); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { get done() { L('done'); return false; }, get value() { L('value'); return 1; } }; }, return() { return {}; } }; } }; (async () => { for await (var v of it) { break; } })();",
  "var it = { [Symbol.iterator]() { return { next() { return { done: false, value: Promise.reject(new Error('v')) }; }, return() { L('return'); return {}; } }; } }; (async () => { try { for await (var v of it) {} } catch (e) { L(e.message); } })();",
  "var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); return {}; } }; } }; (async () => { for await (var v of it) { break; } })();",
  "var it = { [Symbol.iterator]() { return { next() { return { done: false, value: Promise.resolve(1) }; }, return() { L('return'); return 5; } }; } }; (async () => { try { for await (var v of it) { break; } } catch (e) { L(e.name + ':' + e.message); } })();",
  "var it = { [Symbol.iterator]() { return { next() { throw new Error('nx'); } }; } }; (async () => { try { for await (var v of it) {} } catch (e) { L(e.message); } })();",
  "var it = { [Symbol.iterator]() { return { next() { return 5; } }; } }; (async () => { try { for await (var v of it) {} } catch (e) { L(e.name + ':' + e.message); } })();",
  "var it = { [Symbol.iterator]() { return { next() { return { done: true, value: Promise.resolve(1) }; } }; } }; (async () => { for await (var v of it) {} L('ok'); })();",
  "var order = []; (async () => { for await (var v of [1, 2]) { L('b' + v); } })(); (async () => { for await (var v of [3, 4]) { L('c' + v); } })();",
  "(async () => { for await (var [a, b] of [[1, 2], [3, 4]]) L(a + b); })();",
  "(async () => { for await (let { x } of [{ x: 1 }, { x: 2 }]) L(x); })();",
  "(async () => { var o = {}; for await (o.p of [1, 2]) L(o.p); })();",
  "(async () => { label: for await (var a of [1, 2]) { for await (var b of [3, 4]) { if (b == 3) continue label; L(b); } } L('end'); })();",
  "(async () => { for await (var v of [1, 2, 3]) { if (v == 2) continue; L(v); } })();",
  "(async () => { var it = (async function* () { yield 1; yield 2; })(); for await (var v of it) { L(v); } for await (var v of it) { L('again' + v); } L('end'); })();",
  "(async function () { for await (const x of [1]) { L(x); } })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "(async function () { for await (const x of (async function* () { yield 1; })()) { L(x); } })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');",
  "var AFSI = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}.prototype)); L(typeof AFSI[Symbol.asyncIterator]);",
  "async function main() { var r = []; for await (var x of [1, 2, 3].map(async v => v * 2)) r.push(x); L(JSON.stringify(r)); } main();",
  "async function main() { var a = []; for await (var x of [tick(2, 'a'), tick(1, 'b')]) a.push(1); L(a.length); } main();"
);

// 9. Iterator helpers.
const sync = "(function* () { L('p1'); yield 1; L('p2'); yield 2; L('p3'); yield 3; L('p4'); yield 4; })()";
add(
  "L(typeof Iterator); L(Iterator.name); L(Iterator.length); L(typeof Iterator.from);",
  "try { Iterator(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Iterator(); } catch (e) { L(e.name + ':' + e.message); }",
  "class I extends Iterator { next() { return { done: true }; } } L(new I() instanceof Iterator);",
  "L(Iterator.prototype === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())));",
  "L(Object.getOwnPropertyNames(Iterator.prototype).sort().join());",
  "L(Object.getOwnPropertyNames(Iterator).sort().join());",
  "L(Object.prototype.toString.call(Iterator.prototype)); L(Iterator.prototype[Symbol.toStringTag]);",
  "var d = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag); L(typeof d.get + typeof d.set);",
  "var d = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor'); L(typeof d.get + typeof d.set);",
  "L(Object.prototype.toString.call(Iterator.from([1]).map(x => x)));",
  `L(JSON.stringify(Iterator.from([1, 2, 3]).map(x => x * 2).toArray()));`,
  `L(JSON.stringify(Iterator.from({ next() { return { done: false, value: 1 }; } }).take(2).toArray()));`,
  `L(Iterator.from([1]) instanceof Iterator); L(Iterator.from({ next() {} }) instanceof Iterator);`,
  `var it = [1][Symbol.iterator](); L(Iterator.from(it) === it);`,
  `var o = { next() { return { done: true }; } }; var w = Iterator.from(o); L(w === o); L(Object.getPrototypeOf(w) === Object.getPrototypeOf(Iterator.from({ next() {} })));`,
  `var o = { [Symbol.iterator]() { L('iter'); return [1, 2][Symbol.iterator](); } }; L(JSON.stringify(Iterator.from(o).toArray()));`,
  `L(JSON.stringify(Iterator.from('ab').toArray()));`,
  `try { Iterator.from(5); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { Iterator.from(null); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { Iterator.from({}); } catch (e) { L(e.name + ':' + e.message); }`,
  `var w = Iterator.from({ next() { L('next'); return { done: false, value: 1 }; }, return(v) { L('return'); return {}; } }); w.next(); w.return(); w.return(); L(JSON.stringify(w.next()));`,
  `var w = Iterator.from({ next() { return { done: false, value: 1 }; } }); L(JSON.stringify(w.return()));`,
  `var w = Iterator.from({ next(v) { L('arg ' + v); return { done: false, value: 1 }; } }); w.next(5);`,
  `var w = Iterator.from({ next() { return 1; } }); try { w.next(); } catch (e) { L(e.name + ':' + e.message); } L(typeof w.next);`
);
const helpers = {
  map: ["x => x * 10", "x => { L('m' + x); return x; }", "(x, i) => i", "5", "undefined"],
  filter: ["x => x % 2", "x => { L('f' + x); return x > 1; }", "(x, i) => i == 0", "5", "null"],
  take: ["2", "0", "10", "-1", "NaN", "'a'", "Infinity", "1.9", "undefined", "{ valueOf() { L('vo'); return 1; } }"],
  drop: ["2", "0", "10", "-1", "NaN", "'a'", "Infinity", "1.9", "undefined", "{ valueOf() { L('vo'); return 1; } }"],
  flatMap: ["x => [x, x]", "x => 'ab'", "x => x", "x => { L('fm' + x); return [x]; }", "x => (function* () { yield x; })()", "x => new String('ab')", "x => ({ [Symbol.iterator]() { return [x][Symbol.iterator](); } })", "x => ({ next() { return { done: true }; } })"],
};
for (const [name, argsList] of Object.entries(helpers)) {
  for (const arg of argsList) {
    add(`try { L(JSON.stringify(${sync}.${name}(${arg}).toArray())); } catch (e) { L(e.name + ':' + e.message); }`);
    add(`try { var h = ${sync}.${name}(${arg}); L('made'); L(JSON.stringify(h.next())); L(JSON.stringify(h.return())); L(JSON.stringify(h.next())); } catch (e) { L(e.name + ':' + e.message); }`);
  }
}
add(
  `var h = ${sync}.map(x => x); L(JSON.stringify(h.return())); L(JSON.stringify(h.next()));`,
  `var h = ${sync}.filter(x => true); h.next(); L(JSON.stringify(h.return())); L(JSON.stringify(h.next()));`,
  `var h = ${sync}.take(1); L(JSON.stringify(h.next())); L(JSON.stringify(h.next()));`,
  `var closed = 0; var it = { next() { return { done: false, value: 1 }; }, return() { closed++; L('closed'); return {}; }, __proto__: Iterator.prototype }; it.take(2).toArray(); L(closed);`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; it.take(0).next();`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.map(5); } catch (e) { L(e.name + ':' + e.message); }`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.take(-1); } catch (e) { L(e.name + ':' + e.message); }`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.drop(NaN); } catch (e) { L(e.name + ':' + e.message); }`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.map(x => { throw new Error('cb'); }).next(); } catch (e) { L(e.message); }`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.flatMap(x => 5).next(); } catch (e) { L(e.name + ':' + e.message); }`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; var h = it.map(x => x); h.next(); h.return(); h.return(); L('end');`,
  `var it = { next() { L('next'); return { done: false, value: 1 }; }, __proto__: Iterator.prototype }; var h = it.map(x => x); L('made'); h.next();`,
  `var h = ${sync}.map(x => x); h.next(); try { h.next.call({}); } catch (e) { L(e.name + ':' + e.message); }`,
  `var h = [1].values().map(x => { try { h.next(); } catch (e) { L(e.name + ':' + e.message); } return x; }); h.next();`,
  `var h = [1, 2].values().map(x => x); L(Object.prototype.toString.call(h)); L(h[Symbol.iterator]() === h);`,
  `var P = Object.getPrototypeOf([1].values().map(x => x)); L(Object.getOwnPropertyNames(P).sort().join()); L(P[Symbol.toStringTag]); L(Object.getPrototypeOf(P) === Iterator.prototype);`,
  `L(JSON.stringify([1, 2, 3].values().reduce((a, b) => a + b)));`,
  `L(JSON.stringify([1, 2, 3].values().reduce((a, b) => a + b, 10)));`,
  `L(JSON.stringify([1, 2, 3].values().reduce((a, b, i) => a + i, 0)));`,
  `try { [].values().reduce((a, b) => a + b); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { [1].values().reduce(5); } catch (e) { L(e.name + ':' + e.message); }`,
  `L([].values().reduce((a, b) => a + b, 'init'));`,
  `L([1].values().reduce((a, b) => a + b));`,
  `try { [1, 2].values().reduce(() => { throw new Error('cb'); }); } catch (e) { L(e.message); }`,
  `L(JSON.stringify(${sync}.toArray()));`,
  `L(${sync}.some(x => x == 2)); `,
  `L(${sync}.some(x => x == 9));`,
  `L(${sync}.every(x => x < 3));`,
  `L(${sync}.every(x => x < 9));`,
  `L(${sync}.find(x => x > 1));`,
  `L(${sync}.find(x => x > 9));`,
  `L([].values().some(x => true)); L([].values().every(x => false)); L([].values().find(x => true));`,
  `var closed = 0; var it = { next() { return { done: false, value: 1 }; }, return() { closed++; return {}; }, __proto__: Iterator.prototype }; it.some(x => true); it.every(x => false); it.find(x => true); L(closed);`,
  `var it = { next() { return { done: false, value: 1 }; }, return() { L('closed'); return {}; }, __proto__: Iterator.prototype }; try { it.some(x => { throw new Error('cb'); }); } catch (e) { L(e.message); }`,
  `try { [1].values().some(5); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { [1].values().every(); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { [1].values().find(null); } catch (e) { L(e.name + ':' + e.message); }`,
  `var r = []; [1, 2, 3].values().forEach((x, i) => r.push(x + ':' + i)); L(JSON.stringify(r)); L(JSON.stringify([1].values().forEach(x => x)));`,
  `try { [1].values().forEach(5); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { Iterator.prototype.map.call(5, x => x); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { Iterator.prototype.toArray.call({}); } catch (e) { L(e.name + ':' + e.message); }`,
  `try { Iterator.prototype.toArray.call(undefined); } catch (e) { L(e.name + ':' + e.message); }`,
  `L(JSON.stringify(Iterator.prototype.toArray.call({ next() { return { done: true }; } })));`,
  `L(JSON.stringify([1, 2, 3, 4, 5].values().filter(x => x % 2).map(x => x * x).drop(1).take(5).toArray()));`,
  `L(JSON.stringify(new Set([1, 2]).values().map(x => x + 1).toArray())); L(JSON.stringify(new Map([[1, 2]]).entries().map(e => e[1]).toArray()));`,
  `L(JSON.stringify('abc'[Symbol.iterator]().map(c => c.toUpperCase()).toArray()));`,
  `function* nat() { var i = 0; for (;;) yield i++; } L(JSON.stringify(nat().filter(x => x % 3 == 0).map(x => x * 2).take(4).toArray()));`,
  `function* nat() { var i = 0; try { for (;;) yield i++; } finally { L('fin'); } } L(JSON.stringify(nat().take(2).toArray()));`,
  `function* nat() { var i = 0; try { for (;;) yield i++; } finally { L('fin'); } } L(nat().find(x => x == 2));`,
  `function* nat() { var i = 0; try { for (;;) yield i++; } finally { L('fin'); } } var h = nat().map(x => x); h.next(); h.return(); L('end');`,
  `function* nat() { try { yield 1; } finally { L('fin'); } } var h = nat().flatMap(x => [x, x]); h.next(); L(JSON.stringify(h.return()));`,
  `var inner = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('inner closed'); return {}; } }; } }; var h = [1].values().flatMap(x => inner); h.next(); h.return();`,
  `var h = [1].values().flatMap(x => inner = { [Symbol.iterator]: null, next() { return { done: true }; } }); try { L(JSON.stringify(h.toArray())); } catch (e) { L(e.name + ':' + e.message); }`,
  `L(JSON.stringify([[1], [2]].values().flatMap(x => x).toArray()));`,
  `try { [1].values().flatMap(x => 'str').toArray(); L('ok'); } catch (e) { L(e.name + ':' + e.message); }`,
  `L(Iterator.prototype.map.length + ',' + Iterator.prototype.reduce.length + ',' + Iterator.prototype.toArray.length + ',' + Iterator.prototype.flatMap.length + ',' + Iterator.from.length);`,
  `L(Iterator.prototype.map.name + ',' + Iterator.prototype.some.name);`,
  `class MyIt extends Iterator { constructor() { super(); this.i = 0; } next() { return { done: this.i > 2, value: this.i++ }; } } L(JSON.stringify(new MyIt().map(x => x * 2).toArray()));`,
  `var d = Iterator.prototype.constructor; L(d === Iterator);`,
  `Iterator.prototype[Symbol.toStringTag] = 'X'; L(Iterator.prototype[Symbol.toStringTag]);`,
  `var o = Object.create(Iterator.prototype); o[Symbol.toStringTag] = 'Y'; L(o[Symbol.toStringTag]);`,
  `try { Iterator.prototype[Symbol.toStringTag] = 1; L(Iterator.prototype[Symbol.toStringTag]); } catch (e) { L(e.name + ':' + e.message); }`
);

// 10. Rejeição não tratada, microtarefas, queueMicrotask e ordem geral.
add(
  "var p = Promise.reject(new Error('late')); Promise.resolve().then(() => p.catch(e => L('handled ' + e.message))); L('sync');",
  "var p = Promise.reject(1); p.catch(() => {}); p.then(() => {}, v => L('second ' + v)); L('sync');",
  "var p = Promise.reject(1); var q = p.then(() => {}); q.catch(v => L('q ' + v)); L('sync');",
  "Promise.reject(1).catch(v => { L('c' + v); return Promise.reject(2); }).catch(v => L('c' + v));",
  "typeof queueMicrotask === 'function' && queueMicrotask(() => L('qm')); Promise.resolve().then(() => L('p')); L('sync');",
  "queueMicrotask(() => L('qm1')); Promise.resolve().then(() => L('p1')); queueMicrotask(() => L('qm2')); Promise.resolve().then(() => L('p2'));",
  "queueMicrotask(() => { L('qm'); Promise.resolve().then(() => L('nested p')); queueMicrotask(() => L('nested qm')); });",
  "try { queueMicrotask(5); } catch (e) { L(e.name + ':' + e.message); }",
  "try { queueMicrotask(); } catch (e) { L(e.name + ':' + e.message); }",
  "Promise.resolve().then(() => { throw new Error('in then'); }).catch(e => L(e.message)); Promise.resolve().then(() => L('other'));",
  "var order = 0; Promise.resolve().then(() => L('a' + order++)).then(() => L('c' + order++)); Promise.resolve().then(() => L('b' + order++)).then(() => L('d' + order++));",
  "var p1 = Promise.resolve(); var p2 = p1.then(() => L('x')); var p3 = p1.then(() => L('y')); p2.then(() => L('x2')); p3.then(() => L('y2'));",
  "var p = Promise.resolve(); p.then(() => L(1)); p.then(() => L(2)); p.then(() => L(3));",
  "var res; var p = new Promise(r => res = r); p.then(() => L('a')); p.then(() => L('b')); L('before'); res(); L('after');",
  "var res; var p = new Promise(r => res = r); res(1); p.then(() => L('a')); L('sync');",
  "var resolveOuter; var outer = new Promise(r => resolveOuter = r); var inner = new Promise(r => r(outer)); inner.then(() => L('inner')); outer.then(() => L('outer')); resolveOuter(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "async function a() { L('a1'); await null; L('a2'); } async function b() { L('b1'); await null; L('b2'); } Promise.resolve().then(() => L('p1')).then(() => L('p2')); a(); b(); L('sync');",
  "async function a() { await 1; L('a'); } a(); new Promise(r => { L('ex'); r(); }).then(() => L('p')); L('sync');",
  "async function f() { L(1); await new Promise(r => r()); L(2); } f(); Promise.resolve().then(() => L(3)).then(() => L(4)).then(() => L(5));",
  "async function f() { L(1); await (async () => {})(); L(2); } f(); Promise.resolve().then(() => L(3)).then(() => L(4)).then(() => L(5)).then(() => L(6));",
  "async function f() { L(1); return await (async () => { await null; return 'v'; })(); } f().then(v => L(v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "async function f() { return (async () => { await null; return 'v'; })(); } f().then(v => L(v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');",
  "var thenCalls = 0; var thenable2 = { then(r) { thenCalls++; L('then called'); r('v'); } }; Promise.resolve().then(() => thenable2).then(v => L(v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "var o = { then(r) { L('then'); r(1); } }; (async () => { var v = await o; L('v' + v); })(); Promise.resolve().then(() => L('p1')).then(() => L('p2'));",
  "(async () => { await 1; L('a'); })(); (async () => { await Promise.resolve(); L('b'); })(); (async () => { await { then(r) { r(); } }; L('c'); })(); (async () => { await new Promise(r => r()); L('d'); })();",
  "async function* g() { L('g'); } g().next(); Promise.resolve().then(() => L('p'));",
  "async function* g() { await 1; L('g'); yield 1; L('g2'); } var it = g(); it.next().then(() => L('n1')); it.next().then(() => L('n2')); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4'));",
  "var order = []; Promise.resolve(1).then(v => { L(v); return 2; }).then(v => { L(v); return 3; }).finally(() => L('fin')).then(v => L(v));",
  "Promise.reject(1).then(null, v => v + 1).then(v => L(v)).finally(() => L('f')); Promise.resolve().then(() => L('x'));",
  "L(typeof globalThis.setTimeout); L(typeof globalThis.queueMicrotask);",
  "Promise.resolve().then(() => { L('a'); return Promise.resolve(); }).then(() => L('b')); Promise.resolve().then(() => L('c')).then(() => L('d')).then(() => L('e')).then(() => L('f'));",
  "var a = Promise.resolve(); var b = a.then(() => { L('a'); }); Promise.all([a, b]).then(() => L('all')); b.then(() => L('b'));",
  "var p = new Promise(r => r(1)); p.then = () => L('overridden'); Promise.resolve(p).then(v => L(v));",
  "var p = Promise.resolve(1); L(Promise.resolve(p) === p); L(Promise.resolve.call(function (ex) { return new Promise(ex); }, p) === p);",
  "function C(ex) { ex(() => {}, () => {}); } C.resolve = Promise.resolve; var c = C.resolve(1); L(c instanceof C);",
  "function C(ex) { L('exec'); ex(1, 2); } try { Promise.resolve.call(C, 1); } catch (e) { L(e.name + ':' + e.message); }",
  "function C(ex) { ex(() => {}, () => {}); ex(() => {}, () => {}); } try { Promise.resolve.call(C, 1); } catch (e) { L(e.name + ':' + e.message); }",
  "function C(ex) { } try { Promise.resolve.call(C, 1); } catch (e) { L(e.name + ':' + e.message); }",
  "function C(ex) { ex(undefined, undefined); } try { L(typeof Promise.resolve.call(C, 1)); } catch (e) { L(e.name + ':' + e.message); }"
);

// Executa cada programa no bun, num processo próprio.
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "async-golden-"));
const lines = [];
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 0);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
