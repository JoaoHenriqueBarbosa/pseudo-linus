// Gera tests/golden/promise_bun.tsv: ordem de eventos de Promise e microtarefas, medida no bun 1.4.2.
// Cada programa registra eventos no array global `log` (auxiliares L, tick e thenable de
// tests/golden/async_bun_harness.js, o mesmo texto que tests/promise_bun_golden.rs embute) e o golden é o
// JSON do log depois de esvaziar as microtarefas, ou `error<TAB>name<TAB>message JSON` se o programa
// lançou de forma síncrona. Cada programa roda num processo bun próprio, com timeout; rejeição não tratada
// vai para um tratador vazio de `process`, então só a ordem dos eventos conta.
// Fica de fora queueMicrotask e timers (são do host, não do JavaScriptCore).
// O programa é um arquivo (o bun transpila o fonte quando roda arquivo): harness, corpo e a global `R` que devolve o
// log, ver async-golden.js. O arquivo se chama `promise_case.js` dos dois lados.
// Uso: bun scripts/gen-promise-golden.js > tests/golden/promise_bun.tsv
const { emitFactoredLines, sampleByHash } = require("./golden-prelude.js");
const { measureBodies } = require("./async-golden.js").asyncGolden();

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
const TICKS = [1, 2, 3, 4, 5, 6];
const SEE = "v => L('ok' + v), e => L('err' + (e && e.message || e))";

// 1. Resolve com cada tipo de valor, medido em ticks de uma cadeia de comparação.
const resolvers = {
  value: "1",
  native: "Promise.resolve(1)",
  rejected: "Promise.reject(2)",
  pendingNative: "new Promise(r => Promise.resolve().then(() => r(3)))",
  thenableSync: "thenable(4, 's')",
  thenableAsync: "{ then(res) { Promise.resolve().then(() => res(5)); L('then:a'); } }",
  thenableRejects: "{ then(res, rej) { L('then:r'); rej(6); } }",
  thenableThrows: "{ then() { L('then:t'); throw new Error('tt'); } }",
  thenableThrowsAfter: "{ then(res) { L('then:ta'); res(7); throw new Error('late'); } }",
  thenableThenable: "{ then(res) { L('then:o'); res(thenable(8, 'i')); } }",
  thenableNative: "{ then(res) { L('then:n'); res(Promise.resolve(9)); } }",
  getterThrows: "{ get then() { L('get'); throw new Error('gt'); } }",
  getterNonFn: "{ get then() { L('get'); return 5; } }",
  getterFn: "{ get then() { L('get'); return res => res(10); } }",
  subclass: "(class S extends Promise {}).resolve(11)",
  subclassPending: "new (class S extends Promise {})(r => Promise.resolve().then(() => r(12)))",
  fakeBrand: "Object.assign(Object.create(Promise.prototype), { x: 1 })",
  protoThen: "Object.assign(Promise.resolve(13), { then(r) { L('own then'); r(14); } })",
  symbolResult: "Symbol.iterator",
  undef: "undefined",
};
for (const [name, v] of Object.entries(resolvers)) {
  add(`new Promise(r => r(${v})).then(${SEE});`);
  add(`new Promise((_, j) => j(${v})).then(${SEE});`);
  add(`Promise.resolve(${v}).then(${SEE});`);
  add(`Promise.reject(${v}).then(${SEE});`);
  add(`Promise.resolve().then(() => ${v}).then(${SEE});`);
  add(`Promise.resolve().then(() => { throw ${v}; }).then(${SEE});`);
  add(`Promise.reject(0).catch(() => ${v}).then(${SEE});`);
  add(`(async () => ${v})().then(${SEE});`);
  add(`(async () => { return ${v}; })().then(${SEE});`);
  add(`(async () => { return await ${v}; })().then(${SEE});`);
  add(`(async () => { try { var x = await ${v}; L('got ' + (typeof x)); } catch (e) { L('caught'); } })();`);
  add(`(async () => { throw ${v}; })().then(${SEE});`);
  for (const n of TICKS) {
    add(`new Promise(r => r(${v})).then(() => L('outer')); tick(${n}, 't${n}');`);
    add(`Promise.resolve().then(() => ${v}).then(() => L('outer'), () => L('outerErr')); tick(${n}, 't${n}');`);
    add(`(async () => ${v})().then(() => L('outer'), () => L('outerErr')); tick(${n}, 't${n}');`);
    add(`(async () => { await ${v}; L('outer'); })(); tick(${n}, 't${n}');`);
  }
}

// 2. await: ticks por tipo de operando e mistura de várias async functions.
for (const [name, v] of Object.entries(resolvers)) {
  add(`(async () => { try { await ${v}; } catch (e) {} L('after'); })(); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4')).then(() => L('p5'));`);
  add(`(async () => { try { await ${v}; await ${v}; } catch (e) {} L('after2'); })(); tick(1, 't1'); tick(3, 't3'); tick(5, 't5'); tick(7, 't7');`);
}
for (let a = 1; a <= 4; a++) {
  for (let b = 1; b <= 4; b++) {
    add(`async function f(n) { for (var i = 0; i < n; i++) await null; L('f' + n); } f(${a}); f(${b}); L('sync');`);
    add(`async function f(n) { for (var i = 0; i < n; i++) await Promise.resolve(); L('f' + n); } f(${a}); f(${b}); tick(${a}, 'a'); tick(${b}, 'b');`);
    add(`async function g() { return 1; } async function f(n) { for (var i = 0; i < n; i++) await g(); L('f' + n); } f(${a}); f(${b});`);
    add(`async function g() { return Promise.resolve(1); } async function f(n) { for (var i = 0; i < n; i++) await g(); L('f' + n); } f(${a}); f(${b});`);
  }
}

// 3. async function retornando promessa (3 ticks) vs valor, return await.
add(
  "async function f() { return 1; } f().then(() => L('f')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "async function f() { return Promise.resolve(1); } f().then(() => L('f')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "async function f() { return await Promise.resolve(1); } f().then(() => L('f')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "async function f() { try { return Promise.reject(1); } catch (e) { L('caught'); return 'c'; } } f().then(v => L('ok' + v), e => L('err' + e));",
  "async function f() { try { return await Promise.reject(1); } catch (e) { L('caught'); return 'c'; } } f().then(v => L('ok' + v), e => L('err' + e));",
  "async function f() { try { throw 1; } finally { L('fin'); } } f().catch(e => L('err' + e));",
  "async function f() { try { return 1; } finally { await null; L('fin'); } } f().then(v => L('ok' + v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "async function f() { try { await Promise.reject(1); } finally { L('fin'); } } f().catch(e => L('err' + e));",
  "async function f() { await undefined; throw new Error('x'); } f().catch(e => L(e.message)); tick(1, 't1'); tick(2, 't2');",
  "var f = async () => { L('body'); }; L('before'); f(); L('after');",
  "var o = { async m() { L(this === o); await 0; L(this === o); } }; o.m();",
  "class C { async m() { await 0; L(new.target === undefined); } static async s() { await 0; L('s'); } } new C().m(); C.s();",
  "async function f(a = L('default')) { L('body'); } f(); L('sync');",
  "async function f(a = (() => { throw new Error('param'); })()) { L('body'); } f().catch(e => L(e.message)); L('sync');",
  "async function f(a, b = a.x) { L('body'); } f(undefined).then(() => L('ok'), e => L(e.name));",
  "async function f() { L(typeof arguments); await 0; L(arguments.length); } f(1, 2, 3);",
  "async function f() { return arguments.length; } f(1, 2).then(L);",
  "L(Object.getPrototypeOf(async function () {}).constructor.name); L(Object.prototype.toString.call(async function () {}));",
  "L(typeof (async function () {}).prototype); L(Object.getPrototypeOf(async function () {}) === Function.prototype);",
  "try { new (async function () {})(); } catch (e) { L(e.name); }",
  "var p = (async () => {})(); L(p instanceof Promise); L(p.constructor === Promise);",
  "class P2 extends Promise {} var p = (async () => {}).call(); L(p instanceof P2);",
  "var AsyncFunction = Object.getPrototypeOf(async function () {}).constructor; var f = new AsyncFunction('x', 'L(\"in\"); await x; L(\"out\"); return x'); f(5).then(L);",
  "async function f() { await 1; await 2; await 3; L('end'); } f(); tick(1, 'a'); tick(2, 'b'); tick(3, 'c'); tick(4, 'd');",
  "var p = Promise.resolve(); (async () => { await p; L('a'); })(); p.then(() => L('b')); (async () => { await p; L('c'); })();",
  "var p = new Promise(r => r()); p.constructor = function () { L('ctor read'); }; (async () => { await p; L('a'); })(); tick(1, 't1');",
  "var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { L('get ctor'); return Promise; } }); (async () => { await p; L('a'); })();",
  "var p = Promise.resolve(1); p.constructor = class X extends Promise {}; (async () => { await p; L('a'); })(); tick(1, 't1'); tick(2, 't2');",
  "var p = Promise.resolve(1); p.then = function () { L('then own'); return Promise.prototype.then.apply(this, arguments); }; (async () => { await p; L('a'); })();",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; (async () => { await Promise.resolve(); L('a'); })(); Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('patched'); return then.call(this, a, b); }; Promise.resolve().then(() => L('x')); (async () => { await 1; L('a'); })(); (async () => { return Promise.resolve(); })().then(() => L('r'));"
);

// 4. Estáticos: combinadores com todas as formas de entrada e ordem de resolução.
const inputs = {
  empty: "[]",
  values: "[1, 2, 3]",
  natives: "[Promise.resolve(1), Promise.resolve(2)]",
  mixed: "[1, Promise.resolve(2), thenable(3, 'm')]",
  oneReject: "[Promise.resolve(1), Promise.reject(2), Promise.resolve(3)]",
  allReject: "[Promise.reject(1), Promise.reject(2)]",
  pending: "[new Promise(r => Promise.resolve().then(() => r('late'))), 'now']",
  lateReject: "[new Promise((_, j) => Promise.resolve().then(() => j('late'))), 'now']",
  throwingThen: "[{ then() { throw new Error('tt'); } }, 1]",
  thenReject: "[{ then(r, j) { j('tr'); } }, Promise.resolve(1)]",
  twoTicks: "[tick(2, 'x'), tick(1, 'y')]",
  generator: "(function* () { L('g1'); yield 1; L('g2'); yield Promise.resolve(2); L('g3'); })()",
  set: "new Set([1, Promise.resolve(2)])",
  string: "'ab'",
  sparse: "[1, , 3]",
  dupe: "(function () { var p = Promise.resolve(1); return [p, p, p]; })()",
  subclass: "[(class S extends Promise {}).resolve(1), 2]",
};
const combinators = ["all", "allSettled", "any", "race"];
for (const c of combinators) {
  for (const [name, input] of Object.entries(inputs)) {
    add(`Promise.${c}(${input}).then(v => L('ok' + JSON.stringify(v)), e => L('err' + JSON.stringify(e && e.errors || e && e.message || e)));`);
    for (const n of [1, 2, 3, 4, 5]) {
      add(`Promise.${c}(${input}).then(() => L('outer'), () => L('outerErr')); tick(${n}, 't${n}');`);
    }
  }
}
add(
  "Promise.all([]).then(v => L(JSON.stringify(v))); tick(1, 't1'); tick(2, 't2');",
  "Promise.allSettled([]).then(v => L(JSON.stringify(v))); tick(1, 't1');",
  "Promise.any([]).catch(e => L(e.name + ':' + e.errors.length + ':' + e.message)); tick(1, 't1');",
  "var p = Promise.race([]); p.then(() => L('never')); tick(3, 't3'); L(String(p));",
  "Promise.allSettled([1, Promise.reject(2)]).then(r => L(JSON.stringify(r)));",
  "Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => L(e instanceof AggregateError, JSON.stringify(e.errors)));",
  "Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => L(Object.getOwnPropertyNames(e).join()));",
  "Promise.any([Promise.reject(1), 2, Promise.reject(3)]).then(v => L(v));",
  "L(Promise.all.length + ',' + Promise.allSettled.length + ',' + Promise.any.length + ',' + Promise.race.length + ',' + Promise.resolve.length + ',' + Promise.reject.length);",
  "L(Promise.name + ',' + Promise.length + ',' + Promise.prototype.then.length + ',' + Promise.prototype.catch.length + ',' + Promise.prototype.finally.length);",
  "L(Object.getOwnPropertyNames(Promise).sort().join());",
  "L(Object.getOwnPropertyNames(Promise.prototype).sort().join());",
  "L(Object.prototype.toString.call(Promise.resolve()));",
  "L(Promise[Symbol.species] === Promise); L(Object.getOwnPropertyDescriptor(Promise, Symbol.species).get.name);",
  "try { Promise(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Promise(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Promise(5); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.prototype.then.call({}, 1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.all.call(1, []); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.resolve.call(undefined, 1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { Promise.race.call({}, []); } catch (e) { L(e.name + ':' + e.message); }",
  "Promise.all.call(function (ex) { ex(() => L('res'), () => L('rej')); }, []);",
  "var calls = []; function C(ex) { ex(v => calls.push(['res', v]), e => calls.push(['rej', e])); } C.resolve = v => ({ then(r) { L('then ' + v); r(v); } }); Promise.all.call(C, [1, 2]); L(JSON.stringify(calls));",
  "function C(ex) { ex(() => {}, () => {}); } C.resolve = function () { L('resolve'); throw new Error('rt'); }; var p = Promise.all.call(C, [1]); L(typeof p);",
  "var p = Promise.all([1]); p.then = 5; L('ok');",
  "var order = []; var p = { then(r) { order.push('t'); r(1); } }; Promise.all([p, p]).then(() => L(order.join()));",
  "var it = { [Symbol.iterator]() { return { next() { L('next'); return { done: false, value: 1 }; }, return() { L('return'); return {}; } }; } }; Promise.all.call(function (ex) { ex(() => {}, () => {}); }, it);",
  "var it = { [Symbol.iterator]() { return { next() { L('next'); throw new Error('n'); }, return() { L('return'); return {}; } }; } }; Promise.all(it).catch(e => L(e.message));",
  "var it = { [Symbol.iterator]() { return { next() { L('next'); return { done: false, value: 1 }; }, return() { L('return'); return {}; } }; } }; var r = Promise.resolve; Promise.resolve = function () { throw new Error('res'); }; Promise.all(it).catch(e => L(e.message)); Promise.resolve = r;",
  "Promise.all(5).catch(e => L(e.name + ':' + e.message));",
  "Promise.race(null).catch(e => L(e.name));",
  "Promise.any({}).catch(e => L(e.name));",
  "Promise.allSettled(undefined).catch(e => L(e.name));"
);

// 5. withResolvers e try (se existirem), reduzidos à ordem dos eventos.
add(
  "L(typeof Promise.withResolvers); L(typeof Promise.try);",
  "if (Promise.withResolvers) { var w = Promise.withResolvers(); w.promise.then(v => L('v' + v)); w.resolve(1); w.resolve(2); L(Object.keys(w).join()); }",
  "if (Promise.withResolvers) { var w = Promise.withResolvers(); w.promise.then(v => L('v' + v), e => L('e' + e)); w.reject(1); tick(1, 't1'); }",
  "if (Promise.withResolvers) { var w = Promise.withResolvers(); w.resolve(Promise.resolve(1)); w.promise.then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); }",
  "if (Promise.withResolvers) { var w = Promise.withResolvers(); w.resolve(thenable(1, 'w')); w.promise.then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); }",
  "if (Promise.withResolvers) { class S extends Promise {} var w = S.withResolvers(); L(w.promise instanceof S); }",
  "if (Promise.withResolvers) { try { Promise.withResolvers.call(undefined); } catch (e) { L(e.name); } }",
  "if (Promise.withResolvers) { function C(ex) { ex(() => {}, () => {}); } var w = Promise.withResolvers.call(C); L(w.promise instanceof C); }",
  "if (Promise.withResolvers) { var w = Promise.withResolvers(); L(w.resolve.name + ',' + w.resolve.length + ',' + w.reject.name + ',' + w.reject.length); }",
  "if (Promise.try) { Promise.try(() => { L('body'); return 1; }).then(v => L('v' + v)); L('sync'); }",
  "if (Promise.try) { Promise.try(() => { throw new Error('x'); }).catch(e => L(e.message)); L('sync'); }",
  "if (Promise.try) { Promise.try((a, b) => L(a + b), 1, 2); }",
  "if (Promise.try) { Promise.try(() => Promise.resolve(1)).then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); }",
  "if (Promise.try) { Promise.try(() => 1).then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); }",
  "if (Promise.try) { try { Promise.try(); } catch (e) { L(e.name); } }",
  "if (Promise.try) { Promise.try(5).catch(e => L(e.name)); }"
);

// 6. then/catch/finally: finally com valor de passagem, species e subclasses.
const fins = {
  none: "() => {}",
  value: "() => 'fv'",
  throws: "() => { throw new Error('ft'); }",
  rejects: "() => Promise.reject('fr')",
  resolvesPromise: "() => Promise.resolve('fp')",
  thenable: "() => thenable('fth', 'f')",
  nonFunction: "5",
  undef: "undefined",
  argCount: "function () { L('args' + arguments.length); }",
};
const origins = {
  resolved: "Promise.resolve('o')",
  rejected: "Promise.reject('r')",
  late: "new Promise(r => Promise.resolve().then(() => r('l')))",
  lateReject: "new Promise((_, j) => Promise.resolve().then(() => j('lr')))",
};
for (const [on, o] of Object.entries(origins)) {
  for (const [fn, f] of Object.entries(fins)) {
    add(`${o}.finally(${f}).then(${SEE});`);
    add(`${o}.finally(${f}).finally(() => L('f2')).then(${SEE});`);
    for (const n of [2, 3, 4, 5]) {
      add(`${o}.finally(${f}).then(() => L('outer'), () => L('outerErr')); tick(${n}, 't${n}');`);
    }
  }
}
add(
  "var calls = []; var p = Promise.resolve(1); p.then = function (a, b) { calls.push(typeof a + typeof b); return Promise.prototype.then.call(this, a, b); }; p.finally(() => {}); L(calls.join());",
  "var p = Promise.resolve(1); p.then = function (a, b) { L(a.name === '' ? 'anon' : a.name); L(a.length); return 0; }; p.finally(() => {});",
  "var p = Promise.resolve(1); p.then = function (a, b) { L(a === b); L(a.length + ',' + b.length); }; p.finally(5);",
  "var p = Promise.resolve(1); p.then = function (a, b) { L(a === 5 && b === 5); }; p.finally(5);",
  "try { Promise.prototype.finally.call(1); } catch (e) { L(e.name + ':' + e.message); }",
  "var o = { then(a, b) { L('then ' + typeof a); return 'ret'; } }; L(Promise.prototype.finally.call(o, () => {}));",
  "var o = { constructor: undefined, then(a, b) { L('then'); return 1; } }; L(Promise.prototype.finally.call(o, () => {}));",
  "var o = { constructor: 5, then() {} }; try { Promise.prototype.finally.call(o, () => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var o = { constructor: { [Symbol.species]: function (ex) { L('species'); ex(() => {}, () => {}); } }, then(a, b) { L('then'); return 1; } }; Promise.prototype.finally.call(o, () => {});",
  "class S extends Promise { static get [Symbol.species]() { L('species'); return Promise; } } S.resolve(1).finally(() => {}).then(v => L('v' + v));",
  "class S extends Promise { constructor(ex) { L('S ctor'); super(ex); } } S.resolve(1).finally(() => L('f')).then(v => L('v' + v));",
  "class S extends Promise { constructor(ex) { L('S ctor'); super(ex); } } var p = S.resolve(1).finally(() => {}); L(p instanceof S);",
  "var p = Promise.resolve(1); L(p.finally(() => {}) !== p);",
  "Promise.resolve(1).finally(function () { L(this === undefined ? 'undef' : typeof this); });",
  "Promise.resolve(1).finally(() => { L('f1'); }).finally(() => { L('f2'); }).finally(() => { L('f3'); }); tick(2, 'a'); tick(4, 'b'); tick(6, 'c');",
  "Promise.reject(1).finally(() => { L('f'); }).catch(e => L('c' + e)); Promise.reject(2).catch(e => L('d' + e)).finally(() => L('g'));",
  "Promise.resolve(1).finally(() => new Promise(r => Promise.resolve().then(r))).then(v => L('v' + v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5'); tick(6, 't6');"
);

// 7. then: species, subclasses e constructor customizado.
add(
  "class S extends Promise {} var p = S.resolve(1); L(p.then(() => {}) instanceof S); L(p.catch(() => {}) instanceof S); L(p.finally(() => {}) instanceof S);",
  "class S extends Promise { static get [Symbol.species]() { return Promise; } } var p = S.resolve(1); L(p.then(() => {}) instanceof S); L(p.then(() => {}) instanceof Promise);",
  "class S extends Promise { static get [Symbol.species]() { return undefined; } } var p = S.resolve(1); L(p.then(() => {}).constructor === Promise);",
  "class S extends Promise { static get [Symbol.species]() { return null; } } var p = S.resolve(1); L(p.then(() => {}).constructor === Promise);",
  "class S extends Promise { static get [Symbol.species]() { return 5; } } try { S.resolve(1).then(() => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "class S extends Promise { static get [Symbol.species]() { throw new Error('sp'); } } try { S.resolve(1).then(() => {}); } catch (e) { L(e.message); }",
  "var p = Promise.resolve(1); p.constructor = undefined; L(p.then(() => {}) instanceof Promise);",
  "var p = Promise.resolve(1); p.constructor = 5; try { p.then(() => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: undefined }; L(p.then(() => {}) instanceof Promise);",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { L('sp ctor'); ex(r => L('res'), e => L('rej')); } }; p.then(v => L('h' + v));",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { L('sp ctor'); ex(r => L('res ' + r), e => L('rej')); } }; p.then(v => 'x');",
  "var p = Promise.reject(1); p.constructor = { [Symbol.species]: function (ex) { ex(r => L('res'), e => L('rej ' + e)); } }; p.then(null, v => { throw 'again'; });",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { } }; try { p.then(() => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { ex(() => {}, () => {}); ex(() => {}, () => {}); } }; try { p.then(() => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { ex(5, 6); } }; try { p.then(() => {}); } catch (e) { L(e.name + ':' + e.message); }",
  "var p = Promise.resolve(1); p.constructor = { [Symbol.species]: function (ex) { throw new Error('ctor'); } }; try { p.then(() => {}); } catch (e) { L(e.message); }",
  "var p = new Promise(() => {}); p.constructor = { [Symbol.species]: function (ex) { L('sp'); throw new Error('ctor'); } }; try { p.then(() => {}); } catch (e) { L(e.message); }",
  "class S extends Promise { constructor(ex) { L('S'); super(ex); } } S.resolve(1).then(() => L('h')); S.reject(1).catch(() => L('c'));",
  "class S extends Promise { constructor(ex) { L('S'); super((r, j) => { L('exec'); ex(r, j); }); } } new S(r => r(1)).then(v => L('v' + v));",
  "class S extends Promise { constructor(ex) { super(ex); this.tag = 's'; } } var q = S.resolve(1).then(() => {}); L(q.tag);",
  "class S extends Promise { then(a, b) { L('S.then'); return super.then(a, b); } } S.resolve(1).then(() => L('h')); S.resolve(2).finally(() => L('f')); S.all([1]).then(() => L('all'));",
  "class S extends Promise { then(a, b) { L('S.then'); return super.then(a, b); } } (async () => { await S.resolve(1); L('a'); })();",
  "class S extends Promise { then(a, b) { L('S.then'); return super.then(a, b); } } Promise.resolve(S.resolve(1)).then(() => L('r'));",
  "class S extends Promise {} var s = S.resolve(1); L(Promise.resolve(s) === s); L(S.resolve(s) === s); L(S.resolve(Promise.resolve(1)) instanceof S);",
  "class S extends Promise {} L(Promise.resolve(S.resolve(1)) instanceof S); L(Promise.resolve(S.resolve(1)).constructor === S);",
  "class S extends Promise {} Promise.resolve(S.resolve(1)).then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "class S extends Promise {} new Promise(r => r(S.resolve(1))).then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "class S extends Promise {} S.resolve(Promise.resolve(1)).then(() => L('outer')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "class S extends Promise {} S.reject(1).catch(() => L('c')); S.all([S.resolve(1)]).then(v => L(v instanceof Array)); S.race([1]).then(v => L('race'));",
  "class S extends Promise {} L(S.all([]) instanceof S); L(S.allSettled([]) instanceof S); L(S.any([]) instanceof S); L(S.race([]) instanceof S);",
  "class S extends Promise {} L(Object.getPrototypeOf(S) === Promise); L(S.name); L(S.length);",
  "class S extends Promise { static resolve(v) { L('S.resolve'); return super.resolve(v); } } S.all([1, 2]).then(v => L(v.length)); S.race([1]); S.any([1]); S.allSettled([1]);",
  "var p = Promise.resolve(1); var q = p.then(); L(p !== q); q.then(v => L('v' + v));",
  "var p = Promise.resolve(1); p.then(5, 6).then(v => L('v' + v)); Promise.reject(1).then(5, 6).catch(e => L('e' + e));",
  "var p = Promise.resolve(1); p.then(undefined, () => L('never')).then(v => L('v' + v)); tick(2, 't2');",
  "var p = Promise.resolve(); Promise.prototype.then.call(p, function () { L(this === undefined); });",
  "Promise.resolve().then(function () { 'use strict'; L(this === undefined); }); Promise.resolve().then(function () { L(this === globalThis); });",
  "var p = Promise.resolve(); p.then(() => L(1)); p.then(() => L(2)).then(() => L(4)); p.then(() => L(3)).then(() => L(5));"
);

// 8. Promise.resolve === e identidade.
add(
  "var p = Promise.resolve(1); L(Promise.resolve(p) === p);",
  "var p = Promise.reject(1); L(Promise.resolve(p) === p); p.catch(() => {});",
  "var p = new Promise(() => {}); L(Promise.resolve(p) === p);",
  "var p = Promise.resolve(1); p.constructor = Object; L(Promise.resolve(p) === p);",
  "var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { L('get'); return Promise; } }); L(Promise.resolve(p) === p);",
  "var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { throw new Error('gc'); } }); try { Promise.resolve(p); } catch (e) { L(e.message); }",
  "var p = Promise.resolve(1); L(Promise.resolve.call(Promise, p) === p); L(Promise.resolve.call(Object.setPrototypeOf(function (ex) { ex(() => {}, () => {}); }, Promise), p) === p);",
  "var p = Promise.resolve(1); class S extends Promise {} L(S.resolve(p) === p); L(Promise.resolve(S.resolve(1)) instanceof S);",
  "var p = Promise.resolve(1); L(Promise.resolve(p).then(() => {}) !== p);",
  "L(Promise.resolve(Promise.resolve(Promise.resolve(1))) instanceof Promise);",
  "Promise.resolve(Promise.resolve(Promise.resolve(1))).then(v => L(v));",
  "var p = Promise.resolve(1); Promise.resolve(p).then(() => L('a')); p.then(() => L('b'));",
  "var p = Promise.resolve(1); Promise.reject(p).catch(v => L(v === p));",
  "var p = Promise.reject(1); Promise.reject(p).catch(v => L(v === p)); p.catch(() => {});",
  "Promise.resolve(Promise.reject(1)).catch(e => L('c' + e)); tick(1, 't1');",
  "var p = Promise.resolve(); p.then(() => L('a')); Promise.resolve(p).then(() => L('b')); new Promise(r => r(p)).then(() => L('c')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');"
);

// 9. Executor: lançando depois de resolver, resolve/reject duplicados, funções de resolução.
add(
  "new Promise((res, rej) => { res(1); throw new Error('x'); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { rej(1); throw new Error('x'); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { throw new Error('x'); res(1); }).then(v => L('v' + v), e => L('e' + e.message));",
  "new Promise((res, rej) => { res(Promise.reject(1)); throw new Error('x'); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { res(thenable(1, 'a')); throw new Error('x'); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { res(thenable(1, 'a')); res(2); rej(3); }).then(v => L('v' + v), e => L('e' + e));",
  "new Promise((res, rej) => { res(Promise.resolve(1)); res(2); }).then(v => L('v' + v));",
  "new Promise((res, rej) => { rej(thenable(1, 'a')); }).catch(e => L(typeof e));",
  "new Promise((res, rej) => { rej(Promise.resolve(1)); }).catch(e => L(e instanceof Promise));",
  "new Promise((res, rej) => { res(); res(); rej(); }).then(() => L('v'), () => L('e'));",
  "var r1, r2; new Promise((res, rej) => { r1 = res; r2 = rej; }).then(v => L('v' + v), e => L('e' + e)); r2(1); r1(2); L('sync');",
  "var r1; var p = new Promise(res => r1 = res); r1(p); p.then(() => L('v'), e => L(e.name + ':' + e.message));",
  "var r1; var p = new Promise(res => r1 = res); var q = p.then(() => q); r1(1); q.catch(e => L(e.name + ':' + e.message));",
  "var r1; var p = new Promise(res => r1 = res); r1({ get then() { L('get'); return undefined; } }); p.then(v => L(typeof v));",
  "var r1; var p = new Promise(res => r1 = res); r1({ get then() { L('get'); throw 'gt'; } }); L('sync'); p.catch(e => L('e' + e));",
  "var r1; var p = new Promise(res => r1 = res); var n = 0; r1({ get then() { n++; L('get' + n); return res => res(n); } }); p.then(v => L('v' + v));",
  "var r1; var p = new Promise(res => r1 = res); r1({ then: 5 }); p.then(v => L(typeof v.then));",
  "var r1; var p = new Promise(res => r1 = res); r1([1, 2]); p.then(v => L(JSON.stringify(v)));",
  "var r1; var p = new Promise(res => r1 = res); r1(function () {}); p.then(v => L(typeof v));",
  "var r1; var p = new Promise(res => r1 = res); var f = function () {}; f.then = r => { L('fn then'); r(1); }; r1(f); p.then(v => L('v' + v));",
  "new Promise(function (res, rej) { L(typeof res + typeof rej); L(res.length + ',' + rej.length); L(res.name === '' ? 'noname' : res.name); L(this === undefined); });",
  "new Promise((res, rej) => { L(Object.prototype.hasOwnProperty.call(res, 'prototype')); L(Object.getPrototypeOf(res) === Function.prototype); });",
  "new Promise((res, rej) => { try { new res(); } catch (e) { L(e.name); } });",
  "var p = new Promise(res => res(1)); var q = new Promise(res => res(p)); var r = new Promise(res => res(q)); r.then(v => L('v' + v)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5'); tick(6, 't6'); tick(7, 't7');",
  "var depth = 0; function nest(n) { return n == 0 ? Promise.resolve('end') : new Promise(r => r(nest(n - 1))); } nest(3).then(v => L(v)); tick(4, 't4'); tick(6, 't6'); tick(8, 't8'); tick(10, 't10');",
  "Promise.resolve().then(() => { L('a'); return Promise.reject('x'); }).then(() => L('no'), e => L('e' + e)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "var p = new Promise((_, rej) => rej(thenable(1, 'rt'))); p.catch(e => L(typeof e.then));",
  "var thenCalls = 0; var th = { then(r) { thenCalls++; r(1); } }; var a = Promise.resolve(th); var b = Promise.resolve(th); a.then(() => L('a' + thenCalls)); b.then(() => L('b' + thenCalls));"
);

// 10. Rejeição não tratada com tratador tardio (só a ordem).
add(
  "var p = Promise.reject(1); Promise.resolve().then(() => p.catch(e => L('late ' + e))); L('sync');",
  "var p = Promise.reject(1); tick(2, 't2'); tick(3, 't3').then(() => p.catch(e => L('late ' + e)));",
  "var p = Promise.reject(1); var q = p.then(() => {}); Promise.resolve().then(() => q.catch(e => L('q ' + e)));",
  "var p = Promise.reject(1); p.then(() => {}); p.catch(e => L('c' + e));",
  "var p = Promise.reject(1); var q = p.then(() => {}); L('sync');",
  "Promise.reject(1).finally(() => L('f')); L('sync');",
  "Promise.all([Promise.reject(1)]); Promise.race([Promise.reject(2)]); L('sync');",
  "Promise.all([Promise.reject(1), Promise.reject(2)]).catch(e => L('first ' + e));",
  "Promise.race([Promise.reject(1), Promise.reject(2)]).catch(e => L('first ' + e));",
  "Promise.race([Promise.resolve(1), Promise.reject(2)]).then(v => L('v' + v), e => L('e' + e));",
  "Promise.race([Promise.reject(2), Promise.resolve(1)]).then(v => L('v' + v), e => L('e' + e));",
  "Promise.race([tick(2, 'slow').then(() => 'slow'), tick(1, 'fast').then(() => 'fast')]).then(v => L('won ' + v));",
  "Promise.any([Promise.reject(1), tick(2, 'slow').then(() => 'slow')]).then(v => L('any ' + v));",
  "Promise.allSettled([Promise.reject(1)]).then(r => L(r[0].status + r[0].reason));",
  "var p = Promise.reject(1); (async () => { try { await p; } catch (e) { L('caught ' + e); } })(); L('sync');",
  "(async () => { await Promise.reject(1); })(); L('sync');",
  "(async () => { await null; throw 1; })().catch(e => L('c' + e)); L('sync');",
  "async function f() { throw 1; } var p = f(); L('after'); Promise.resolve().then(() => p.catch(e => L('late ' + e)));",
  "var p = Promise.reject(1); p.catch(() => {}); Promise.resolve().then(() => p.then(null, e => L('again ' + e)));",
  "var a = Promise.reject(1); var b = a.catch(e => { throw e; }); var c = b.catch(e => L('c' + e)); L('sync');",
  "new Promise((_, j) => j(1)).then(() => {}).then(() => {}).catch(e => L('end' + e)); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "new Promise((_, j) => j(1)).catch(e => { L('c1'); throw 2; }).catch(e => { L('c2' + e); }).then(() => L('after'));"
);

// 11. Async generators.
const agBodies = {
  yieldValue: "yield 1; yield 2; return 3;",
  yieldPromise: "yield Promise.resolve(1); yield Promise.resolve(2); return 3;",
  yieldRejected: "try { yield Promise.reject('r'); } catch (e) { L('caught ' + e); yield 'after'; }",
  yieldThenable: "yield thenable('t', 'g'); return 'done';",
  returnPromise: "yield 1; return Promise.resolve('rp');",
  returnAwait: "yield 1; return await Promise.resolve('ra');",
  returnRejected: "yield 1; return Promise.reject('rr');",
  throwsEarly: "L('start'); throw new Error('boom');",
  throwsAfterYield: "yield 1; throw new Error('boom');",
  awaitInside: "var x = await Promise.resolve(1); yield x; var y = await 2; yield y;",
  tryFinally: "try { yield 1; yield 2; } finally { L('fin'); }",
  finallyYields: "try { yield 1; } finally { yield 'cleanup'; L('after cleanup'); }",
  finallyAwaits: "try { yield 1; } finally { await null; L('fin awaited'); }",
  yieldStar: "yield* [1, 2]; yield* (async function* () { yield 'a'; return 'r'; })();",
  yieldStarSync: "var r = yield* [Promise.resolve(1), 2]; L('r' + r);",
  yieldStarAsyncReturn: "var r = yield* (async function* () { yield 1; return 'inner'; })(); L('r' + r); return r;",
  empty: "",
  yieldUndefined: "yield; yield undefined;",
  receive: "var a = yield 1; L('a=' + a); var b = yield 2; L('b=' + b); return a + b;",
};
const driver = {
  nextSeq: (g) => `var it = ${g}(); it.next('x').then(r => L('1:' + JSON.stringify(r)), e => L('1e:' + e && e.message || e)); `,
};
for (const [name, body] of Object.entries(agBodies)) {
  const g = `(async function* () { ${body} })`;
  const R = "r => L(JSON.stringify(r)), e => L('E:' + (e && e.message || e))";
  add(
    `var it = ${g}(); it.next().then(${R}); it.next().then(${R}); it.next().then(${R}); it.next().then(${R});`,
    `var it = ${g}(); it.next().then(${R}).then(() => it.next()).then(${R}).then(() => it.next()).then(${R}).then(() => it.next()).then(${R});`,
    `var it = ${g}(); it.next(1).then(${R}); it.next(2).then(${R}); it.next(3).then(${R}); tick(1, 't1'); tick(3, 't3'); tick(6, 't6');`,
    `var it = ${g}(); it.return('rv').then(${R}); it.next().then(${R});`,
    `var it = ${g}(); it.throw(new Error('th')).then(${R}); it.next().then(${R});`,
    `var it = ${g}(); it.next().then(() => it.return('rv')).then(${R}); it.next().then(${R});`,
    `var it = ${g}(); it.next().then(() => it.return(Promise.resolve('rv'))).then(${R}); tick(5, 't5');`,
    `var it = ${g}(); it.next().then(() => it.return(Promise.reject('rj'))).then(${R});`,
    `var it = ${g}(); it.next().then(() => it.throw('tv')).then(${R}).then(() => it.next()).then(${R});`,
    `var it = ${g}(); var a = it.return(1), b = it.return(2), c = it.next(); a.then(${R}); b.then(${R}); c.then(${R});`,
    `var it = ${g}(); var a = it.throw(1), b = it.next(); a.then(${R}); b.then(${R});`,
    `(async () => { for await (var v of ${g}()) L('v' + v); L('end'); })().catch(e => L('E:' + e));`,
    `(async () => { for await (var v of ${g}()) { L('v' + v); break; } L('end'); })().catch(e => L('E:' + e));`,
    `(async () => { for await (var v of ${g}()) { L('v' + v); throw 'x'; } })().catch(e => L('E:' + e));`,
    `(async () => { for await (var v of ${g}()) { L('v' + v); } })().then(() => L('done')); tick(3, 't3'); tick(6, 't6'); tick(9, 't9');`
  );
}
add(
  "async function* g() { L('body'); yield 1; } var it = g(); L('created'); it.next(); L('called');",
  "async function* g() { L('body'); } var it = g(); L(Object.prototype.toString.call(it)); L(it[Symbol.asyncIterator]() === it);",
  "async function* g() {} L(Object.getPrototypeOf(g).constructor.name); L(Object.prototype.toString.call(g)); L(typeof g.prototype);",
  "async function* g() {} L(Object.getPrototypeOf(g.prototype) === Object.getPrototypeOf(async function* () {}).prototype);",
  "async function* g() {} var AGP = Object.getPrototypeOf(g.prototype); L(Object.getOwnPropertyNames(AGP).sort().join());",
  "async function* g() {} var AGP = Object.getPrototypeOf(g.prototype); L(AGP.next.length + ',' + AGP.return.length + ',' + AGP.throw.length);",
  "async function* g() {} var AGP = Object.getPrototypeOf(g.prototype); AGP.next.call({}).catch(e => L(e.name + ':' + e.message));",
  "async function* g() {} var AGP = Object.getPrototypeOf(g.prototype); AGP.return.call(1).catch(e => L(e.name));",
  "async function* g() {} var AGP = Object.getPrototypeOf(g.prototype); AGP.throw.call(undefined).catch(e => L(e.name));",
  "async function* g() { yield 1; } var it = g(); AGP = Object.getPrototypeOf(Object.getPrototypeOf(it)); L(typeof AGP.next);",
  "async function* g() { try { yield 1; } finally { L('fin'); } } var it = g(); it.next().then(() => it.return('x')).then(r => L(JSON.stringify(r)));",
  "async function* g() { try { yield 1; } finally { return 'override'; } } var it = g(); it.next().then(() => it.return('x')).then(r => L(JSON.stringify(r)));",
  "async function* g() { try { yield 1; } catch (e) { L('c' + e); yield 'recovered'; } } var it = g(); it.next().then(() => it.throw('boom')).then(r => L(JSON.stringify(r)));",
  "async function* g() { yield 1; } var it = g(); it.next().then(() => it.next()).then(() => it.next()).then(r => L(JSON.stringify(r)));",
  "async function* g() { var x = yield 1; L('x=' + x); } var it = g(); it.next('ignored').then(() => it.next('real'));",
  "async function* g() { yield* [1, 2]; } var it = g(); it.next().then(r => L(JSON.stringify(r))); it.next().then(r => L(JSON.stringify(r))); it.next().then(r => L(JSON.stringify(r)));",
  "var inner = { [Symbol.asyncIterator]() { return { next(v) { L('inner next ' + v); return Promise.resolve({ done: false, value: 'iv' }); }, return(v) { L('inner return ' + v); return Promise.resolve({ done: true, value: 'ir' }); }, throw(v) { L('inner throw ' + v); return Promise.resolve({ done: true, value: 'it' }); } }; } }; async function* g() { var r = yield* inner; L('r' + r); return r; } var it = g(); it.next('a').then(r => { L(JSON.stringify(r)); return it.next('b'); }).then(r => { L(JSON.stringify(r)); return it.return('c'); }).then(r => L(JSON.stringify(r)));",
  "var inner = { [Symbol.asyncIterator]() { return { next(v) { return { done: false, value: 'iv' }; } }; } }; async function* g() { yield* inner; } var it = g(); it.next().then(r => L(JSON.stringify(r)), e => L(e.name));",
  "var inner = { [Symbol.asyncIterator]() { return { next(v) { return 5; } }; } }; async function* g() { yield* inner; } g().next().catch(e => L(e.name));",
  "var inner = { [Symbol.asyncIterator]() { return { next(v) { return Promise.resolve(5); } }; } }; async function* g() { yield* inner; } g().next().catch(e => L(e.name));",
  "var inner = { [Symbol.asyncIterator]: undefined, [Symbol.iterator]() { L('sync fallback'); return [1][Symbol.iterator](); } }; async function* g() { yield* inner; } g().next().then(r => L(JSON.stringify(r)));",
  "async function* g() { yield* 5; } g().next().catch(e => L(e.name));",
  "async function* g() { yield* undefined; } g().next().catch(e => L(e.name));",
  "async function* g() { yield await Promise.reject('ar'); } g().next().catch(e => L('E' + e));",
  "async function* g() { yield Promise.reject('yr'); } var it = g(); it.next().catch(e => L('E' + e)); it.next().then(r => L(JSON.stringify(r)));",
  "async function* g() { try { yield Promise.reject('yr'); } catch (e) { L('in ' + e); } } var it = g(); it.next().then(r => L(JSON.stringify(r)));",
  "async function* g() { return Promise.reject('rr'); } g().next().catch(e => L('E' + e));",
  "async function* g() { try { return Promise.reject('rr'); } catch (e) { L('in'); } } g().next().then(r => L(JSON.stringify(r)), e => L('E' + e));",
  "async function* g() { try { return await Promise.reject('rr'); } catch (e) { L('in'); } } g().next().then(r => L(JSON.stringify(r)), e => L('E' + e));",
  "async function* g() { yield 1; yield 2; yield 3; } var it = g(); var ps = [it.next(), it.next(), it.next(), it.next(), it.next()]; ps.forEach((p, i) => p.then(r => L(i + ':' + JSON.stringify(r))));",
  "async function* g() { L('s'); await null; L('m'); yield 1; L('e'); } var it = g(); it.next().then(() => L('n1')); it.next().then(() => L('n2')); it.next().then(() => L('n3'));",
  "async function* g() { yield 1; } var it = g(); it.return(Promise.resolve('x')).then(r => L(JSON.stringify(r)));",
  "async function* g() { yield 1; } var it = g(); it.return(thenable('x', 'r')).then(r => L(JSON.stringify(r)));",
  "async function* g() { yield 1; } var it = g(); it.return(Promise.reject('x')).then(r => L(JSON.stringify(r)), e => L('E' + e));",
  "async function* g() { yield 1; } var it = g(); it.return({ get then() { throw 'gt'; } }).then(r => L(JSON.stringify(r)), e => L('E' + e));",
  "async function* g() { yield 1; } var it = g(); it.next().then(() => it.return(Promise.resolve('x'))).then(r => L(JSON.stringify(r))); tick(2, 't2'); tick(4, 't4'); tick(6, 't6');",
  "async function* g() { try { yield 1; } finally { L('fin'); } } var it = g(); it.return(1).then(r => L(JSON.stringify(r)));",
  "async function* g() { try { yield 1; } finally { L('fin'); } } var it = g(); it.throw(1).catch(e => L('E' + e));",
  "async function* g() { try { yield 1; } finally { L('fin'); await null; L('fin2'); } } var it = g(); var a = it.next(); var b = it.return('r'); var c = it.next(); a.then(() => L('a')); b.then(() => L('b')); c.then(() => L('c'));",
  "var order = []; async function* g() { order.push('g1'); yield 1; order.push('g2'); yield 2; order.push('g3'); } var it = g(); it.next().then(() => order.push('n1')); it.next().then(() => order.push('n2')); it.next().then(() => { order.push('n3'); L(order.join()); });",
  "async function* g() { var i = 0; while (true) { var cmd = yield i++; if (cmd) break; } return 'stopped'; } var it = g(); it.next().then(() => it.next()).then(() => it.next(true)).then(r => L(JSON.stringify(r)));",
  "async function* g() { yield 1; } (async () => { var it = g(); L(JSON.stringify(await it.next())); L(JSON.stringify(await it.next())); L(JSON.stringify(await it.next())); })();",
  "async function* g() { yield 1; await new Promise(r => Promise.resolve().then(r)); yield 2; } (async () => { var out = []; for await (var v of g()) out.push(v); L(JSON.stringify(out)); })();",
  "class C { async *m() { yield this.x; } constructor() { this.x = 'cx'; } static async *s() { yield 'st'; } } new C().m().next().then(r => L(r.value)); C.s().next().then(r => L(r.value));",
  "var o = { async *m() { yield 'om'; } }; o.m().next().then(r => L(r.value)); L(Object.getOwnPropertyNames(o.m).join());",
  "async function* g() { L(typeof arguments); yield arguments.length; } g(1, 2).next().then(r => L(r.value));",
  "async function* g(a = L('default')) { L('body'); yield a; } var it = g(); L('created'); it.next();",
  "async function* g(a = (() => { throw new Error('p'); })()) { yield a; } try { g(); L('no throw'); } catch (e) { L('thrown ' + e.message); }",
  "async function* g() { yield 1; } try { new g(); } catch (e) { L(e.name); }"
);

// 12. for await.
const syncIters = {
  values: "[1, 2, 3]",
  promises: "[Promise.resolve(1), Promise.resolve(2)]",
  mixed: "[1, Promise.resolve(2), thenable(3, 'fa')]",
  rejected: "[1, Promise.reject('rej'), 3]",
  lateRejected: "[1, new Promise((_, j) => Promise.resolve().then(() => j('late')))]",
  empty: "[]",
  set: "new Set([1, Promise.resolve(2)])",
  string: "'ab'",
  generator: "(function* () { try { yield 1; yield Promise.resolve(2); yield 3; } finally { L('gen fin'); } })()",
  throwingGen: "(function* () { yield 1; throw new Error('gt'); })()",
  nested: "[[1], Promise.resolve([2])]",
};
for (const [name, it] of Object.entries(syncIters)) {
  const D = "catch (e) { L('E:' + (e && e.message || e)); }";
  add(
    `(async () => { try { for await (var v of ${it}) L('v' + JSON.stringify(v)); L('end'); } ${D} })();`,
    `(async () => { try { for await (var v of ${it}) { L('v' + JSON.stringify(v)); break; } L('end'); } ${D} })();`,
    `(async () => { try { for await (var v of ${it}) { L('v' + JSON.stringify(v)); throw 'body'; } } ${D} })();`,
    `(async () => { try { for await (var v of ${it}) { L('v' + JSON.stringify(v)); continue; } L('end'); } ${D} })();`,
    `(async () => { try { for await (var [a] of ${it}) L('a' + a); L('end'); } ${D} })();`,
    `(async () => { try { for await (const v of ${it}) L('v' + JSON.stringify(v)); L('end'); } ${D} })(); tick(2, 't2'); tick(4, 't4'); tick(6, 't6'); tick(8, 't8');`,
    `(async () => { try { for await (var v of ${it}) L('v' + JSON.stringify(v)); } ${D} L('end'); })(); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4')).then(() => L('p5')).then(() => L('p6'));`
  );
}
add(
  "var it = { [Symbol.asyncIterator]() { var i = 0; return { next() { L('next'); return Promise.resolve({ done: i++ >= 2, value: i }); }, return() { L('return'); return Promise.resolve({}); } }; } }; (async () => { for await (var v of it) L('v' + v); L('end'); })();",
  "var it = { [Symbol.asyncIterator]() { var i = 0; return { next() { L('next'); return Promise.resolve({ done: false, value: i++ }); }, return() { L('return'); return Promise.resolve({}); } }; } }; (async () => { for await (var v of it) { if (v == 1) break; } L('end'); })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); return Promise.resolve({}); } }; } }; (async () => { for await (var v of it) { L('v' + v); break; } L('end'); })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: true, value: 1 }; } }; } }; (async () => { for await (var v of it) L('never'); L('end'); })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return 5; } }; } }; (async () => { try { for await (var v of it) L('never'); } catch (e) { L(e.name); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve(5); } }; } }; (async () => { try { for await (var v of it) L('never'); } catch (e) { L(e.name); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { throw 'sync throw'; } }; } }; (async () => { try { for await (var v of it) L('never'); } catch (e) { L('E ' + e); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return Promise.reject('rej'); }, return() { L('return'); return {}; } }; } }; (async () => { try { for await (var v of it) L('never'); } catch (e) { L('E ' + e); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); throw 'rt'; } }; } }; (async () => { try { for await (var v of it) { throw 'body'; } } catch (e) { L('E ' + e); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); throw 'rt'; } }; } }; (async () => { try { for await (var v of it) { break; } } catch (e) { L('E ' + e); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); return 5; } }; } }; (async () => { try { for await (var v of it) { break; } L('end'); } catch (e) { L('E ' + e.name); } })();",
  "var it = { [Symbol.asyncIterator]() { return { next() { return { done: false, value: 1 }; }, return() { L('return'); return Promise.resolve(5); } }; } }; (async () => { try { for await (var v of it) { break; } L('end'); } catch (e) { L('E ' + e.name); } })();",
  "var it = { [Symbol.asyncIterator]: null, [Symbol.iterator]() { L('sync'); return [1, 2][Symbol.iterator](); } }; (async () => { for await (var v of it) L('v' + v); })();",
  "var it = { [Symbol.asyncIterator]: 5 }; (async () => { try { for await (var v of it) L('never'); } catch (e) { L(e.name); } })();",
  "(async () => { try { for await (var v of 5) L('never'); } catch (e) { L(e.name); } })();",
  "(async () => { try { for await (var v of undefined) L('never'); } catch (e) { L(e.name); } })();",
  "(async () => { try { for await (var v of {}) L('never'); } catch (e) { L(e.name); } })();",
  "var syncReturn = { [Symbol.iterator]() { var i = 0; return { next() { return { done: i++ > 5, value: i }; }, return() { L('sync return'); return {}; } }; } }; (async () => { for await (var v of syncReturn) { L('v' + v); if (v == 2) break; } })();",
  "var syncReturn = { [Symbol.iterator]() { var i = 0; return { next() { return { done: i++ > 5, value: Promise.reject('r' + i) }; }, return() { L('sync return'); return {}; } }; } }; (async () => { try { for await (var v of syncReturn) { L('v' + v); } } catch (e) { L('E ' + e); } })();",
  "(async () => { var out = []; for await (var x of (async function* () { yield 1; yield 2; })()) { for await (var y of [10, 20]) out.push(x * y); } L(JSON.stringify(out)); })();",
  "(async () => { for await (var x of [Promise.resolve('a')]) { L(x); } })(); (async () => { for await (var x of [Promise.resolve('b')]) { L(x); } })();",
  "(async () => { for await (var x of (async function* () { yield 'a'; })()) L(x); })(); (async () => { for await (var x of (async function* () { yield 'b'; })()) L(x); })();",
  "async function* g() { for await (var x of [1, 2]) yield x * 2; } (async () => { var out = []; for await (var v of g()) out.push(v); L(JSON.stringify(out)); })();",
  "async function* g() { yield* [Promise.resolve(1), 2]; } (async () => { var out = []; for await (var v of g()) out.push(v); L(JSON.stringify(out)); })();",
  "var log2 = []; (async () => { for await (var x of [1, 2, 3]) { log2.push(x); await null; } L(log2.join()); })();"
);

// 13. Passagem entre microtarefas: conclusões gerais de ordem.
add(
  "Promise.resolve().then(() => L('a1')).then(() => L('a2')).then(() => L('a3')); Promise.resolve().then(() => L('b1')).then(() => L('b2')).then(() => L('b3'));",
  "var p = Promise.resolve(); p.then(() => { L('a'); p.then(() => L('c')); }); p.then(() => L('b'));",
  "var p = Promise.resolve(); p.then(() => { L('a'); Promise.resolve().then(() => L('nested')); }); p.then(() => L('b')).then(() => L('b2'));",
  "Promise.resolve().then(() => { Promise.reject('x').catch(() => L('inner')); L('outer'); }); tick(1, 't1'); tick(2, 't2');",
  "var seq = []; for (var i = 0; i < 5; i++) { (function (i) { Promise.resolve().then(() => seq.push('a' + i)).then(() => seq.push('b' + i)); })(i); } tick(3, 't3').then(() => L(seq.join()));",
  "Promise.resolve().then(() => L(1)); (async () => { L(2); await undefined; L(3); })(); Promise.resolve().then(() => L(4)); L(5);",
  "(async () => { L(1); await Promise.resolve(); L(2); await Promise.resolve(); L(3); })(); Promise.resolve().then(() => L('a')).then(() => L('b')).then(() => L('c'));",
  "var p = Promise.resolve().then(() => L('first')); (async () => { await p; L('after first'); })(); p.then(() => L('plain'));",
  "new Promise(r => { L('exec'); r(); }); L('after exec');",
  "Promise.resolve(1).then(L).then(L);",
  "Promise.resolve().then(L, L); Promise.reject(2).then(L, L);",
  "var r; new Promise(res => r = res).then(() => L('first')); new Promise(res => r = res).then(() => L('second')); r();",
  "var resolvers = []; var ps = [0, 1, 2].map(i => new Promise(r => resolvers.push(r)).then(() => L('p' + i))); resolvers[2](); resolvers[0](); resolvers[1]();",
  "var resolvers = []; Promise.all([0, 1].map(i => new Promise(r => resolvers.push(r)))).then(() => L('all')); resolvers[1](); resolvers[0](); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');",
  "Promise.all([1, 2, 3].map(x => Promise.resolve(x).then(v => { L('m' + v); return v; }))).then(v => L('all' + v));",
  "Promise.all([tick(1, 'x'), tick(2, 'y'), tick(3, 'z')]).then(() => L('all'));",
  "Promise.race([tick(3, 'x'), tick(1, 'y')]).then(() => L('race'));",
  "Promise.allSettled([tick(2, 'x'), Promise.reject('r')]).then(() => L('settled'));",
  "Promise.any([Promise.reject('a'), tick(2, 'x')]).then(() => L('any'));",
  "Promise.all([Promise.resolve(1)]).then(() => L('all')); Promise.resolve(1).then(() => L('p1')).then(() => L('p2')).then(() => L('p3'));",
  "Promise.race([Promise.resolve(1)]).then(() => L('race')); Promise.resolve(1).then(() => L('p1')).then(() => L('p2'));",
  "Promise.allSettled([Promise.resolve(1)]).then(() => L('as')); Promise.resolve(1).then(() => L('p1')).then(() => L('p2')).then(() => L('p3'));",
  "Promise.any([Promise.resolve(1)]).then(() => L('any')); Promise.resolve(1).then(() => L('p1')).then(() => L('p2')).then(() => L('p3'));",
  "Promise.all([1]).then(() => L('all')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  "Promise.all([]).then(() => L('all0')); Promise.race([1]).then(() => L('race1')); tick(1, 't1'); tick(2, 't2');"
);

// 14. Modificação de protótipos e getters observáveis.
add(
  "var then = Promise.prototype.then; var n = 0; Promise.prototype.then = function (a, b) { n++; return then.call(this, a, b); }; Promise.all([1, 2]); Promise.resolve().finally(() => {}); Promise.prototype.then = then; L(n);",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then'); return then.call(this, a, b); }; Promise.race([1]); Promise.allSettled([1]); Promise.any([1]); Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then'); return then.call(this, a, b); }; Promise.resolve(thenable(1, 'x')); tick(1, 't1'); Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then'); return then.call(this, a, b); }; new Promise(r => r(Promise.resolve(1))); Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then'); return then.call(this, a, b); }; (async () => { return Promise.resolve(1); })(); Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = function (a, b) { L('then'); return then.call(this, a, b); }; (async () => { for await (var x of [1]) ; })(); tick(4, 't4'); Promise.prototype.then = then;",
  "var res = Promise.resolve; var n = 0; Promise.resolve = function (v) { n++; return res.call(this, v); }; Promise.all([1, 2]); Promise.race([1]); Promise.any([1]); Promise.allSettled([1]); Promise.resolve = res; L(n);",
  "var res = Promise.resolve; Promise.resolve = function (v) { L('resolve ' + v); return res.call(this, v); }; (async () => { await 5; L('awaited'); })(); Promise.resolve = res;",
  "var res = Promise.resolve; Promise.resolve = function (v) { L('resolve ' + v); return res.call(this, v); }; (async () => { for await (var x of [7]) ; })(); Promise.resolve = res;",
  "var res = Promise.resolve; Promise.resolve = function (v) { L('resolve ' + v); return res.call(this, v); }; (async function* () { yield 8; })().next(); (async () => { return 9; })(); Promise.resolve = res;",
  "var P = Promise; Promise = function () { L('replaced'); }; (async () => { await 1; L('a'); })(); new P(r => r()); Promise = P;",
  "var then = Promise.prototype.then; delete Promise.prototype.then; try { (async () => { await Promise.resolve(); L('a'); })(); L('started'); } catch (e) { L(e.name); } Promise.prototype.then = then;",
  "var then = Promise.prototype.then; Promise.prototype.then = 5; try { Promise.resolve().finally(() => {}); } catch (e) { L(e.name); } Promise.prototype.then = then;",
  "var desc = Object.getOwnPropertyDescriptor(Promise, 'prototype'); L(desc.writable + ',' + desc.enumerable + ',' + desc.configurable);",
  "var d = Object.getOwnPropertyDescriptor(Promise.prototype, Symbol.toStringTag); L(d.value + ',' + d.writable + ',' + d.configurable);",
  "var d = Object.getOwnPropertyDescriptor(globalThis, 'Promise'); L(d.writable + ',' + d.enumerable + ',' + d.configurable);",
  "Object.defineProperty(Promise.prototype, 'then', { value: Promise.prototype.then, writable: false }); Promise.prototype.then = 1; L(typeof Promise.prototype.then);",
  "L(Promise.prototype.constructor === Promise); L(Object.keys(Promise).length); L(Object.keys(Promise.prototype).length);",
  "L(typeof Promise.prototype.then.call); L(Promise.prototype.then.name); L(Promise.prototype.finally.name); L(Promise.prototype.catch.name);",
  "Promise.prototype.catch.call({ then(a, b) { L(a === undefined); L(typeof b); return 'r'; } }, () => {});",
  "L(Promise.prototype.catch.call({ then(a, b) { return [a, b]; } }, 1)[1]);",
  "try { Promise.prototype.catch.call(undefined); } catch (e) { L(e.name); }",
  "Promise.prototype.catch.call(Promise.resolve(1), () => {}).then(v => L('v' + v));",
  "L(Promise.prototype.then.call(Promise.resolve(1)) instanceof Promise);",
  "L(Object.getOwnPropertyNames(Promise.resolve()).length); L(Reflect.ownKeys(Promise.resolve()).length);",
  "L(String(Promise.resolve())); L(String(Promise.reject(1).catch(() => {}))); L(String(new Promise(() => {})));",
  "var p = Promise.resolve(); L(Object.isExtensible(p)); p.x = 1; L(p.x); L(JSON.stringify(p));"
);

// 15. Mais mistura de thenables: ticks exatos de thenable job.
for (let n = 1; n <= 6; n++) {
  add(
    `new Promise(r => r({ then(res) { res(1); } })).then(() => L('outer')); tick(${n}, 't${n}');`,
    `new Promise(r => r({ then(res) { Promise.resolve().then(() => res(1)); } })).then(() => L('outer')); tick(${n}, 't${n}');`,
    `new Promise(r => r({ then(res, rej) { rej(1); } })).catch(() => L('outer')); tick(${n}, 't${n}');`,
    `new Promise(r => r({ then() { throw 1; } })).catch(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.resolve({ then(res) { res(1); } }).then(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.resolve().then(() => ({ then(res) { res(1); } })).then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async () => ({ then(res) { res(1); } }))().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async () => { await { then(res) { res(1); } }; L('outer'); })(); tick(${n}, 't${n}');`,
    `Promise.all([{ then(res) { res(1); } }]).then(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.race([{ then(res) { res(1); } }]).then(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.allSettled([{ then(res) { res(1); } }]).then(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.any([{ then(res) { res(1); } }]).then(() => L('outer')); tick(${n}, 't${n}');`,
    `Promise.resolve(1).finally(() => ({ then(res) { res(1); } })).then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield { then(res) { res(1); } }; })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { return { then(res) { res(1); } }; })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `var p = Promise.resolve(); new Promise(r => r(p)).then(() => L('outer')); p.then(() => L('p')); tick(${n}, 't${n}');`,
    `var p = Promise.reject(1); new Promise(r => r(p)).catch(() => L('outer')); tick(${n}, 't${n}');`,
    `var p = Promise.reject(1); (async () => { await p; })().catch(() => L('outer')); tick(${n}, 't${n}');`,
    `var p = Promise.reject(1); (async () => p)().catch(() => L('outer')); tick(${n}, 't${n}');`,
    `(async () => { throw 1; })().catch(() => L('outer')); tick(${n}, 't${n}');`,
    `(async () => { try { await Promise.reject(1); } catch (e) { L('caught'); } L('outer'); })(); tick(${n}, 't${n}');`,
    `(async function* () { yield 1; })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield Promise.resolve(1); })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield* [1]; })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield* (async function* () { yield 1; })(); })().next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield 1; })().return(1).then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async function* () { yield 1; })().throw(1).catch(() => L('outer')); tick(${n}, 't${n}');`,
    `var it = (async function* () { yield 1; })(); it.next().then(() => it.return(1)).then(() => L('outer')); tick(${n}, 't${n}');`,
    `var it = (async function* () { yield 1; yield 2; })(); it.next(); it.next().then(() => L('outer')); tick(${n}, 't${n}');`,
    `(async () => { for await (var x of [1]) ; L('outer'); })(); tick(${n}, 't${n}');`,
    `(async () => { for await (var x of [Promise.resolve(1)]) ; L('outer'); })(); tick(${n}, 't${n}');`,
    `(async () => { for await (var x of (async function* () { yield 1; })()) ; L('outer'); })(); tick(${n}, 't${n}');`
  );
}

// Amostra determinística por hash (sampleByHash) até o alvo.
const TARGET = 1200;
const selected = sampleByHash(programs, TARGET);
programs.length = 0;
programs.push(...selected);

// Executa cada programa no bun, num processo próprio, como arquivo (ver async-golden.js).
measureBodies(programs, "promise_case.js").then((lines) => {
  process.stdout.write(emitFactoredLines("promise", lines));
  process.stderr.write(`${programs.length} programas\n`);
});
