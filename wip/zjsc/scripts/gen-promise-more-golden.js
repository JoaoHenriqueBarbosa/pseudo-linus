// Gera tests/golden/promise_more_bun.tsv: Promise avançado medido no bun 1.4.2 (JavaScriptCore). Cobre subclasses
// com constructor hostil e species, thenables que lançam ou com getter `then` (inclusive em Object.prototype),
// ciclo de resolução ("Chaining cycle detected for promise"), Promise.all/allSettled/any/race com iteráveis
// hostis (next, done e value que lançam, return, `resolve` do construtor, ordem das chamadas e das microtarefas),
// ordem dos errors do AggregateError, finally com thenables e `this` inválido, await de thenable contra promessa
// nativa (ticks exatos), then em promessa já resolvida, executor reentrante, funções de resolução chamadas duas
// vezes, name/length das funções de resolução, ordem entre várias cadeias e fila drenada por laço de awaits.
// Cada programa registra eventos no array global `log` (auxiliares L, tick e thenable de
// tests/golden/async_bun_harness.js, o mesmo texto que tests/promise_more_bun_golden.rs embute) e o golden é o
// JSON do log depois de esvaziar as microtarefas, ou `error<TAB>name<TAB>message JSON` se lançou de forma
// síncrona. O programa roda no bun como arquivo (ver async-golden.js), sem API de host dentro do programa.
// Programas já presentes nos outros goldens de Promise e async são descartados.
// Uso: bun scripts/gen-promise-more-golden.js > tests/golden/promise_more_bun.tsv
const { emitFactoredLines, sampleByHash } = require("./golden-prelude.js");
const { knownBodies, measureBodies, originalProgram } = require("./async-golden.js").asyncGolden({ own: "promise_more_bun.tsv" });

const existing = knownBodies(["promise_bun.tsv", "microtask_bun.tsv", "async_bun.tsv", "async_gen_bun.tsv", "esnext_bun.tsv"]);
const programs = [];
const seen = new Set();
// Famílias pequenas e decisivas (ciclo, AggregateError) entram inteiras, fora da amostragem.
const kept = new Set();
let forceKeep = false;
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      if (forceKeep) kept.add(programs.length);
      programs.push(source);
    }
  }
};

// Auxiliares embutidos em cada programa (o programa é uma linha só e roda num realm novo).
const PRE =
  "var show = v => { try { return v instanceof Error ? v.constructor.name + ':' + v.message : (typeof v === 'object' && v !== null) || typeof v === 'function' ? JSON.stringify(v) : String(v); } catch (e) { return '?'; } };" +
  " var ok = t => v => L(t + ' ok ' + show(v)); var ko = t => e => L(t + ' ko ' + show(e)); var H = (p, t) => p.then(ok(t), ko(t));";
const IT =
  " var IT = (vals, o) => { o = o || {}; return { [Symbol.iterator]() { L('iter'); var i = 0; return { next() { L('next' + i); if (o.nextThrows === i) throw new Error('next' + i); if (o.doneThrows === i) return { get done() { throw new Error('done' + i); } }; if (i >= vals.length) return { done: true }; var k = i++; return { done: false, get value() { L('value' + k); if (o.valueThrows === k) throw new Error('value' + k); return vals[k]; } }; }, return(v) { L('return'); if (o.returnThrows) throw new Error('rt'); return {}; } }; } }; };";
const CS =
  " class C extends Promise { constructor(ex) { L('ctor'); super((res, rej) => { L('exec'); ex(res, rej); }); } static resolve(v) { L('C.resolve ' + show(v)); return super.resolve(v); } then(f, r) { L('C.then'); return super.then(f, r); } }";
const MKP =
  " var mkp = n => { var p = Promise.resolve(n); var t = p.then; p.then = function (f, r) { L('then' + n); return t.call(this, f, r); }; return p; };";
const prog = (body, extra = "") => PRE + extra + " " + body;

// 1. name, length e forma das funções de resolução (executor, combinadores, finally, withResolvers).
{
  const probes = {
    names: "L(res.name + '|' + res.length + '|' + rej.name + '|' + rej.length)",
    type: "L(typeof res + typeof rej + (res !== rej))",
    proto: "L(String(res.hasOwnProperty('prototype')) + String(rej.hasOwnProperty('prototype')) + String(Object.getPrototypeOf(res) === Function.prototype))",
    keys: "L(Object.getOwnPropertyNames(res).join() + '|' + Object.getOwnPropertyNames(rej).join())",
    nameDesc: "var d = Object.getOwnPropertyDescriptor(res, 'name'); L(JSON.stringify(d))",
    lengthDesc: "var d = Object.getOwnPropertyDescriptor(res, 'length'); L(JSON.stringify(d))",
    tostr: "L(Function.prototype.toString.call(res).replace(/\\s+/g, ' '))",
    construct: "try { new res(1); L('constructed'); } catch (e) { L(e.constructor.name + ':' + e.message); }",
    ctorCall: "L(String(res(1)) + String(rej === rej))",
    thisIgnored: "res.call({}, 7); L('called');",
    extensible: "L(String(Object.isExtensible(res)) + String(Object.isFrozen(res)))",
    bind: "L(res.bind(null).name)",
  };
  for (const [name, probe] of Object.entries(probes)) {
    add(`new Promise((res, rej) => { ${probe}; });`);
    add(`class S extends Promise {} new S((res, rej) => { ${probe}; });`);
    add(`var d = Promise.withResolvers(); var res = d.resolve, rej = d.reject; ${probe}; L(Object.keys(d).join());`);
  }
  const elementProbe = "L(f.name + '|' + f.length + '|' + r.name + '|' + r.length + '|' + String(f === r) + '|' + f.hasOwnProperty('prototype'))";
  for (const m of ["all", "allSettled", "any", "race"]) {
    add(`Promise.${m}([{ then(f, r) { ${elementProbe}; } }]); L('sync');`);
    add(`Promise.${m}([Promise.resolve(1)].map(p => (p.then = function (f, r) { ${elementProbe}; }, p)));`);
    add(`Promise.${m}([{ then(f, r) { L(Object.getOwnPropertyNames(f).join()); L(Function.prototype.toString.call(f).replace(/\\s+/g, ' ')); } }]);`);
  }
  add(
    `var p = Promise.resolve(1); p.then = function (f, r) { ${elementProbe}; }; Promise.prototype.finally.call(p, function () {});`,
    `var p = Promise.resolve(1); p.then = function (f, r) { L(Function.prototype.toString.call(f).replace(/\\s+/g, ' ')); }; p.finally(() => {});`,
    `var p = Promise.resolve(1); p.then = function (f, r) { ${elementProbe}; }; p.finally(5);`,
    `var f; Promise.resolve(1).then(x => 1); L(Promise.prototype.then.name + Promise.prototype.then.length + Promise.prototype.catch.name + Promise.prototype.catch.length + Promise.prototype.finally.name + Promise.prototype.finally.length);`,
    `L([Promise.all, Promise.allSettled, Promise.any, Promise.race, Promise.resolve, Promise.reject, Promise.withResolvers].map(f => f.name + f.length).join());`,
    `L(Promise.name + Promise.length + String(Promise.prototype[Symbol.toStringTag]) + Object.getOwnPropertyNames(Promise).sort().join());`,
    `L(Object.getOwnPropertyNames(Promise.prototype).sort().join()); L(String(Object.getOwnPropertyDescriptor(Promise, Symbol.species).get.name));`,
    `L(Object.getOwnPropertyDescriptor(Promise, Symbol.species).get.call(7) + '|' + typeof Object.getOwnPropertyDescriptor(Promise, Symbol.species).set);`,
    `L(AggregateError.name + AggregateError.length + Object.getPrototypeOf(AggregateError).name + AggregateError.prototype.name + JSON.stringify(Object.getOwnPropertyNames(AggregateError.prototype).sort()));`
  );
}

// 2. Funções de resolução chamadas mais de uma vez, em todas as ordens.
{
  const acts = {
    res: "res('A')",
    rej: "rej('B')",
    resP: "res(Promise.resolve('C'))",
    rejP: "rej(Promise.resolve('D'))",
    thr: "throw 'T'",
    resT: "res(thenable('E', 'th'))",
    resUndef: "res()",
    resSelfName: "res(res)",
  };
  for (const [a, x] of Object.entries(acts)) {
    for (const [b, y] of Object.entries(acts)) {
      if (a === "thr" && b === "thr") continue;
      add(prog(`H(new Promise((res, rej) => { ${x}; ${y}; }), 'p'); tick(4, 't4');`));
    }
  }
  add(
    prog(`var r1, r2; var p = new Promise((res, rej) => { r1 = res; r2 = rej; }); H(p, 'p'); r1(1); r1(2); r2(3); L(String(r1(4)));`),
    prog(`var r1; var p = new Promise(res => { r1 = res; }); H(p, 'p'); r1(thenable(1, 'a')); r1(2); tick(3, 't3');`),
    prog(`var r1; var p = new Promise(res => { r1 = res; }); H(p, 'p'); r1(thenable(1, 'a')); tick(0, 't0'); r1(2); tick(3, 't3');`),
    prog(`var r1, r2; var p = new Promise((res, rej) => { r1 = res; r2 = rej; }); H(p, 'p'); r1(Promise.resolve(1)); r2(2); tick(3, 't3');`),
    prog(`var r1, r2; var p = new Promise((res, rej) => { r1 = res; r2 = rej; }); H(p, 'p'); r1(Promise.reject(1)); r1(5); tick(3, 't3');`),
    prog(`var t = { then(f, r) { L('then'); f(1); f(2); r(3); throw new Error('after'); } }; H(Promise.resolve(t), 'p'); tick(3, 't3');`),
    prog(`var t = { then(f, r) { L('then'); r(3); f(1); } }; H(Promise.resolve(t), 'p'); tick(3, 't3');`),
    prog(`var t = { then(f, r) { L('then'); throw new Error('before'); } }; H(Promise.resolve(t), 'p'); tick(3, 't3');`),
    prog(`var t = { then(f, r) { L('then'); f(1); throw new Error('after'); } }; H(Promise.resolve(t), 'p'); tick(3, 't3');`),
    prog(`var t = { then(f, r) { L('then'); setTimeoutless = 1; f(thenable(2, 'in')); f(3); } }; H(Promise.resolve(t), 'p'); tick(5, 't5');`),
    prog(`var rs; var p = new Promise(res => { rs = res; }); H(p, 'p'); rs(1); L(String(Object.is(rs(2), undefined)));`),
    prog(`var d = Promise.withResolvers(); H(d.promise, 'p'); d.resolve(1); d.resolve(2); d.reject(3); tick(2, 't2');`),
    prog(`var d = Promise.withResolvers(); H(d.promise, 'p'); d.reject(1); d.resolve(2); d.reject(3); tick(2, 't2');`),
    prog(`var d = Promise.withResolvers(); H(d.promise, 'p'); d.resolve(d.promise); tick(2, 't2');`)
  );
}

// 3. Executor reentrante e formas do construtor.
add(
  prog(`var p = new Promise(res => { L('exec'); res(1); L('after-res'); }); L('ctor-done'); H(p, 'p');`),
  prog(`var p = new Promise((res, rej) => { res(1); throw new Error('late'); }); H(p, 'p'); tick(2, 't2');`),
  prog(`var p = new Promise((res, rej) => { throw new Error('early'); }); H(p, 'p'); tick(2, 't2');`),
  prog(`var p = new Promise((res, rej) => { rej(1); throw new Error('late'); }); H(p, 'p'); tick(2, 't2');`),
  prog(`var p = new Promise((res, rej) => { var q = new Promise(r2 => { L('inner'); r2(5); }); L('mid'); q.then(v => { L('qthen'); res(v); }); L('end'); }); H(p, 'p'); tick(3, 't3');`),
  prog(`var p = new Promise((res, rej) => { res(Promise.resolve(1)); L('x'); }); H(p, 'p'); tick(3, 't3');`),
  prog(`var p = new Promise(res => { Promise.resolve().then(() => { L('micro'); res(1); }); L('sync'); }); H(p, 'p');`),
  prog(`var p = new Promise(res => { res(p2); }); var p2 = 1; H(p, 'p');`),
  prog(`var inner; var p = new Promise(res => { inner = new Promise(r2 => r2(1)); res(inner); }); H(p, 'p'); H(inner, 'i'); tick(4, 't4');`),
  prog(`var o = { n: 0 }; var p = new Promise(function (res) { o.n++; L(typeof this); res(this === undefined ? 'u' : 'd'); }); H(p, 'p');`),
  prog(`'use strict'; var p = new Promise(function (res) { L(String(this)); res(); }); H(p, 'p');`),
  prog(`try { Promise(function () {}); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { Promise.call({}, function () {}); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { new Promise(); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { new Promise(1); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { new Promise({}); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { new Promise(class {}); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { new Promise(Math.max); L('ok'); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { Promise.prototype.then.call({}, 1, 2); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`try { Promise.prototype.catch.call(1); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`var r = Reflect.construct(Promise, [function (res) { res(1); }], function () {}); L(String(Object.getPrototypeOf(r) === Promise.prototype)); H(r, 'p');`),
  prog(`var NT = function () {}.bind(); NT.prototype = Array.prototype; var r = Reflect.construct(Promise, [function (res) { res(1); }], NT); L(String(Object.getPrototypeOf(r) === Array.prototype)); L(String(r instanceof Promise));`),
  prog(`var nt = function () {}; nt.prototype = 5; var r = Reflect.construct(Promise, [function () {}], nt); L(String(Object.getPrototypeOf(r) === Promise.prototype));`),
  prog(`var p = new Promise(() => {}); L(String(p) + Object.prototype.toString.call(p) + typeof p.then + String(Object.keys(p).length));`),
  prog(`var p = Promise.resolve(); p.then = 1; H(Promise.resolve(p), 'p'); tick(2, 't2');`)
);

// 4. Ciclo de resolução (no bun a mensagem sai medida, não presumida).
forceKeep = true;
add(
  prog(`var rs; var p = new Promise(res => { rs = res; }); H(p, 'p'); rs(p); tick(2, 't2');`),
  prog(`var p = new Promise(res => res(p)); H(p, 'p');`),
  prog(`var p = new Promise(res => setupLater = res); H(p, 'p'); setupLater(p);`),
  prog(`var p = Promise.resolve(1).then(() => p); H(p, 'p'); tick(3, 't3');`),
  prog(`var p = Promise.resolve(1).then(() => { throw p; }); H(p, 'p'); tick(3, 't3');`),
  prog(`var p = Promise.reject(1).catch(() => p); H(p, 'p'); tick(3, 't3');`),
  prog(`var p = Promise.resolve(1).finally(() => p); H(p, 'p'); tick(5, 't5');`),
  prog(`var p = Promise.resolve(1).then(() => ({ then(f) { f(p); } })); H(p, 'p'); tick(5, 't5');`),
  prog(`var p = Promise.resolve(1).then(() => ({ then(f, r) { r(p); } })); H(p, 'p'); tick(5, 't5');`),
  prog(`var f = async () => { await null; return q; }; var q = f(); H(q, 'q'); tick(6, 't6');`),
  prog(`var q = (async () => q)(); H(q, 'q'); tick(4, 't4');`),
  prog(`var a, b; a = new Promise(res => { setTimeoutless = res; }); b = a.then(() => b); H(b, 'b'); setTimeoutless(1); tick(6, 't6');`),
  prog(`var ra, rb; var a = new Promise(r => ra = r), b = new Promise(r => rb = r); H(a, 'a'); H(b, 'b'); ra(b); rb(a); tick(6, 't6');`),
  prog(`var d = Promise.withResolvers(); H(d.promise, 'p'); d.resolve(d.promise); tick(3, 't3');`),
  prog(`class S extends Promise {} var rs; var p = new S(res => { rs = res; }); H(p, 'p'); rs(p); tick(3, 't3');`),
  prog(`var rs; var p = new Promise(res => { rs = res; }); var q = p.then(v => v); H(q, 'q'); rs(q); tick(4, 't4');`),
  prog(`var p = Promise.resolve(); var q = p.then(() => q); q.catch(e => L(e instanceof TypeError ? e.message.replace(/#<[^>]*>/, '#<P>') : 'other')); tick(4, 't4');`),
  prog(`var p = Promise.resolve(); var q = p.then(() => q); q.catch(e => L(String(e.message).indexOf('Chaining cycle detected for promise') === 0)); tick(4, 't4');`),
  prog(`var p = Promise.resolve(); var q = p.then(() => q); q.catch(e => L(Object.prototype.toString.call(e) + String(Object.getPrototypeOf(e) === TypeError.prototype)));`),
  prog(`var rs; var p = new Promise(res => { rs = res; }); var o = { get then() { L('get'); return undefined; } }; rs(o); H(p, 'p');`),
  prog(`var rs; var p = new Promise(res => { rs = res; }); var o = { get then() { L('get'); return function (f) { f(p); }; } }; rs(o); H(p, 'p'); tick(4, 't4');`)
);

forceKeep = false;
// 5. Thenables: matriz de formas contra pontos de entrada.
{
  const kinds = {
    plain: "{ then(f) { L('then'); f(1); } }",
    rejects: "{ then(f, r) { L('then'); r(new Error('tr')); } }",
    throwsBefore: "{ then() { L('then'); throw new Error('tb'); } }",
    throwsAfter: "{ then(f) { L('then'); f(1); throw new Error('ta'); } }",
    getterThrows: "{ get then() { L('get'); throw new Error('gt'); } }",
    getterOnce: "{ get then() { L('get'); return f => { L('call'); f(1); }; } }",
    notCallable: "{ then: 1 }",
    nested: "{ then(f) { L('outer'); f({ then(g) { L('inner'); g(1); } }); } }",
    noCall: "{ then() { L('then'); } }",
    proxy: "new Proxy({ then(f) { L('then'); f(1); } }, { get(t, k, r) { L('trap ' + String(k)); return Reflect.get(t, k, r); } })",
    fnThenable: "Object.assign(function () {}, { then(f) { L('then'); f(1); } })",
    promiseOwnThen: "Object.assign(Promise.resolve(1), { then(f) { L('then'); f(2); } })",
  };
  const entries = {
    resolve: x => `H(Promise.resolve(${x}), 'e')`,
    ctor: x => `H(new Promise(res => res(${x})), 'e')`,
    thenReturn: x => `H(Promise.resolve().then(() => ${x}), 'e')`,
    awaitIt: x => `(async () => { try { var v = await ${x}; L('v ' + show(v)); } catch (e) { L('c ' + show(e)); } })()`,
    asyncReturn: x => `H((async () => ${x})(), 'e')`,
    all: x => `H(Promise.all([${x}]), 'e')`,
    race: x => `H(Promise.race([${x}]), 'e')`,
    any: x => `H(Promise.any([${x}]), 'e')`,
    allSettled: x => `H(Promise.allSettled([${x}]), 'e')`,
    finallyRet: x => `H(Promise.resolve(0).finally(() => ${x}), 'e')`,
    yieldIt: x => `H((async function* () { yield ${x}; })().next(), 'e')`,
  };
  for (const [kn, k] of Object.entries(kinds)) {
    for (const [en, e] of Object.entries(entries)) {
      add(prog(`${e(k)}; tick(4, 't4'); L('sync');`));
    }
  }
  // Getter then em Object.prototype: afeta qualquer objeto resolvido.
  const protoGetters = {
    returnsFn: "L('get'); return function (f) { L('call'); f('P'); };",
    throws: "L('get'); throw new Error('proto');",
    undef: "L('get');",
    rejects: "L('get'); return function (f, r) { r('R'); };",
    counts: "var n = (globalThis.n | 0) + 1; globalThis.n = n; L('get' + n); return undefined;",
  };
  const protoUses = [
    "H(Promise.resolve({}), 'e')",
    "H(Promise.resolve([1]), 'e')",
    "H(new Promise(res => res({ a: 1 })), 'e')",
    "H(Promise.all([{}, 1]), 'e')",
    "H(Promise.resolve(1).then(() => ({})), 'e')",
    "(async () => { try { L('v ' + show(await {})); } catch (e) { L('c ' + show(e)); } })()",
    "H((async () => ({}))(), 'e')",
    "H(Promise.resolve(1).finally(() => {}), 'e')",
    "H(Promise.allSettled([{ x: 1 }]), 'e')",
    "H(Promise.resolve(function () {}), 'e')",
    "H(Promise.resolve(1), 'e')",
  ];
  for (const [gn, g] of Object.entries(protoGetters)) {
    for (const use of protoUses) {
      add(prog(`Object.defineProperty(Object.prototype, 'then', { get() { ${g} }, configurable: true }); ${use}; tick(4, 't4');`));
    }
  }
}

// 6. Subclasses: constructor hostil e species.
{
  const ctors = {
    plain: "class S extends Promise {}",
    logging: "class S extends Promise { constructor(ex) { L('ctor'); super(ex); } }",
    wrapsExec: "class S extends Promise { constructor(ex) { super((res, rej) => { L('wrap'); ex(res, rej); }); } }",
    swapsResolve: "class S extends Promise { constructor(ex) { super((res, rej) => { ex(v => { L('swap-res'); res(v); }, e => { L('swap-rej'); rej(e); }); }); } }",
    neverCallsExec: "function S(ex) { L('ctor'); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    callsTwice: "function S(ex) { L('ctor'); ex(function () {}, function () {}); ex(function () {}, function () {}); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise); S[Symbol.species] = S;",
    nonCallable: "function S(ex) { ex(1, 2); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    undefRes: "function S(ex) { ex(undefined, function () {}); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    resolvesOnce: "function S(ex) { ex(function (v) { L('res ' + show(v)); }, function (e) { L('rej ' + show(e)); }); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    throwsCtor: "function S(ex) { L('ctor'); throw new Error('boom'); } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    returnsOther: "var O = Promise.resolve('other'); function S(ex) { ex(function () {}, function () {}); return O; } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    returnsPrim: "function S(ex) { ex(function () {}, function () {}); return 1; } S.prototype = Object.create(Promise.prototype); Object.setPrototypeOf(S, Promise);",
    resolveGetter: "class S extends Promise { static get resolve() { L('get-resolve'); return super.resolve; } }",
    speciesThis: "class S extends Promise { static get [Symbol.species]() { L('species'); return this === S ? Promise : S; } }",
  };
  const ops = {
    staticResolve: "H(S.resolve(1), 'e')",
    staticReject: "H(S.reject(1), 'e')",
    all: "H(S.all([1]), 'e')",
    race: "H(S.race([1]), 'e')",
    any: "H(S.any([1]), 'e')",
    allSettled: "H(S.allSettled([1]), 'e')",
    withResolvers: "var d = S.withResolvers(); d.resolve(1); H(d.promise, 'e')",
    newThenThen: "H(new S(res => res(1)).then(v => v), 'e')",
    finally: "H(new S(res => res(1)).finally(() => {}), 'e')",
  };
  for (const [cn, c] of Object.entries(ctors)) {
    for (const [on, o] of Object.entries(ops)) {
      add(prog(`try { ${c}; ${o}; } catch (e) { L('sync ' + show(e)); } tick(4, 't4');`));
    }
  }
  const species = {
    undef: "undefined",
    nul: "null",
    nonCtor: "() => {}",
    num: "1",
    obj: "{}",
    other: "class O extends Promise { constructor(ex) { L('O'); super(ex); } }",
    throws: "(() => { throw new Error('sp'); })()",
    selfFn: "function (ex) { L('plainfn'); ex(function () {}, function () {}); }",
  };
  for (const [sn, s] of Object.entries(species)) {
    const spec = sn === "throws" ? "get [Symbol.species]() { throw new Error('sp'); }" : sn === "other" ? `get [Symbol.species]() { return (${s}); }` : `get [Symbol.species]() { return ${s}; }`;
    add(
      prog(`var p = Promise.resolve(1); p.constructor = { ${spec} }; try { var q = p.then(v => v); L(String(q instanceof Promise)); H(q, 'q'); } catch (e) { L('sync ' + show(e)); } tick(3, 't3');`),
      prog(`class S extends Promise { static ${spec} } try { var q = S.resolve(1).then(v => v); L(String(q.constructor === S) + String(q instanceof Promise)); } catch (e) { L('sync ' + show(e)); } tick(3, 't3');`),
      prog(`var p = Promise.resolve(1); p.constructor = { ${spec} }; try { H(p.finally(() => {}), 'q'); } catch (e) { L('sync ' + show(e)); } tick(4, 't4');`),
      prog(`var p = Promise.resolve(1); p.constructor = { ${spec} }; try { H(Promise.resolve(p), 'q'); L('same ' + String(Promise.resolve(p) === p)); } catch (e) { L('sync ' + show(e)); } tick(4, 't4');`)
    );
  }
  add(
    prog(`var p = Promise.resolve(1); p.constructor = undefined; var q = p.then(v => v); L(String(q.constructor === Promise)); H(q, 'q');`),
    prog(`var p = Promise.resolve(1); p.constructor = 5; try { p.then(v => v); L('ok'); } catch (e) { L(show(e)); }`),
    prog(`var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { get() { L('get-ctor'); return Promise; } }); p.then(v => v); L(String(Promise.resolve(p) === p)); p.finally(() => {});`),
    prog(`var p = Promise.resolve(1); p.constructor = Object; L(String(Promise.resolve(p) === p)); H(Promise.resolve(p), 'q'); tick(4, 't4');`),
    prog(`class S extends Promise {} var p = S.resolve(1); L(String(Promise.resolve(p) === p) + String(S.resolve(p) === p) + String(S.resolve(Promise.resolve(1)) instanceof S));`),
    prog(`class S extends Promise {} var r = S.resolve(1).then(v => v); L(String(r instanceof S) + String(r.constructor === S));`),
    prog(`class S extends Promise { static get [Symbol.species]() { return Promise; } } var r = S.resolve(1).then(v => v); L(String(r instanceof S) + String(r.constructor === Promise));`),
    prog(`class S extends Promise {} H(Promise.prototype.then.call(S.resolve(2), v => v + 1), 'q'); L(String(Promise.prototype.then.call(S.resolve(2)) instanceof S));`),
    prog(`class S extends Promise {} (async () => { var v = await S.resolve(1); L('v' + v); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`class S extends Promise {} (async () => { var v = await Promise.resolve(1); L('v' + v); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`class S extends Promise {} (async () => S.resolve(1))().then(() => L('r')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`),
    prog(`var p = Promise.resolve(1); p.constructor = Promise; (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var p = Promise.resolve(1); p.constructor = Object; (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var p = Promise.resolve(1); p.constructor = Object; (async () => p)().then(() => L('after')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`),
    prog(`var p = Promise.resolve(1); Object.setPrototypeOf(p, Object.create(Promise.prototype)); (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var p = Promise.resolve(1); p.then = Promise.prototype.then; (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var p = Promise.resolve(1); p.then = function (f, r) { L('own-then'); return Promise.prototype.then.call(this, f, r); }; (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var p = Promise.resolve(1); var pr = Promise.prototype.then; Promise.prototype.then = function (f, r) { L('proto-then'); return pr.call(this, f, r); }; (async () => { await p; L('after'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
    prog(`var pr = Promise.prototype.then; Promise.prototype.then = function (f, r) { L('proto-then'); return pr.call(this, f, r); }; Promise.resolve(1).finally(() => L('f')); Promise.all([1]); Promise.race([1]); tick(4, 't4');`),
    prog(`var pr = Promise.prototype.then; Promise.prototype.then = function (f, r) { L('proto-then'); return pr.call(this, f, r); }; (async () => 1)().then(() => L('x')); Promise.resolve(thenable(1, 'a')); Promise.reject(1).catch(() => {}); tick(4, 't4');`),
    prog(`var pr = Promise.resolve; Promise.resolve = function (v) { L('P.resolve'); return pr.call(this, v); }; Promise.all([1, 2]); Promise.allSettled([3]); Promise.any([4]); Promise.race([5]); (async () => { await 6; })(); Promise.resolve(1).finally(() => {}); tick(3, 't3');`),
    prog(`var pr = Promise.resolve; Promise.resolve = function (v) { L('P.resolve'); return pr.call(this, v); }; (async () => { await Promise.resolve(1); })(); (async () => 1)(); (async function* () { yield 1; })().next(); tick(2, 't2');`),
    prog(`var pt = Promise.prototype.then; Object.defineProperty(Promise.prototype, 'then', { get() { L('get-then'); return pt; }, configurable: true }); (async () => { await 1; L('a'); })(); Promise.resolve(1).then(() => L('b')); Promise.all([1]); tick(3, 't3');`)
  );
}

// 7. Combinadores com iteráveis hostis.
{
  const scen = {
    plain: m => `H(Promise.${m}(IT([1, 2, 3])), 'r')`,
    nextThrows0: m => `H(Promise.${m}(IT([1, 2], { nextThrows: 0 })), 'r')`,
    nextThrows1: m => `H(Promise.${m}(IT([1, 2], { nextThrows: 1 })), 'r')`,
    nextThrows2: m => `H(Promise.${m}(IT([1, 2], { nextThrows: 2 })), 'r')`,
    doneThrows1: m => `H(Promise.${m}(IT([1, 2], { doneThrows: 1 })), 'r')`,
    valueThrows0: m => `H(Promise.${m}(IT([1, 2], { valueThrows: 0 })), 'r')`,
    valueThrows1: m => `H(Promise.${m}(IT([1, 2], { valueThrows: 1 })), 'r')`,
    returnThrowsOnResolveError: m => `class D extends Promise { static get resolve() { L('get'); return () => { throw new Error('res'); }; } } H(D.${m}(IT([1, 2], { returnThrows: true })), 'r')`,
    resolveThrows: m => `class D extends Promise { static get resolve() { L('get'); return () => { throw new Error('res'); }; } } H(D.${m}(IT([1, 2])), 'r')`,
    resolveGetterThrows: m => `class D extends Promise { static get resolve() { throw new Error('getter'); } } H(D.${m}(IT([1, 2])), 'r')`,
    resolveNotCallable: m => `class D extends Promise {} D.resolve = 1; H(D.${m}(IT([1])), 'r')`,
    resolveUndefined: m => `class D extends Promise {} D.resolve = undefined; H(D.${m}(IT([1])), 'r')`,
    resolveCount: m => `class R extends Promise { static resolve(v) { L('R.resolve' + v); return super.resolve(v); } } H(R.${m}(IT([1, 2])), 'r')`,
    resolveGetterOnce: m => `class R extends Promise { static get resolve() { L('get'); return super.resolve; } } H(R.${m}(IT([1, 2, 3])), 'r')`,
    resolveReturnsNonPromise: m => `class R extends Promise { static resolve(v) { L('R.resolve' + v); return { then(f, r) { L('fake then'); f(v); } }; } } H(R.${m}(IT([1, 2])), 'r')`,
    resolveReturnsThrowingThen: m => `class R extends Promise { static resolve(v) { return { then() { throw new Error('thenx'); } }; } } H(R.${m}(IT([1, 2])), 'r')`,
    resolveReturnsPrim: m => `class R extends Promise { static resolve(v) { return 5; } } H(R.${m}(IT([1, 2])), 'r')`,
    resolveReturnsUndef: m => `class R extends Promise { static resolve(v) { } } H(R.${m}(IT([1, 2], {})), 'r')`,
    elementThenThrows: m => `var q = Promise.resolve(1); q.then = () => { throw new Error('thenthrow'); }; H(Promise.${m}(IT([q, 2])), 'r')`,
    elementThenThrowsReturnThrows: m => `var q = Promise.resolve(1); q.then = () => { throw new Error('thenthrow'); }; H(Promise.${m}(IT([q, 2], { returnThrows: true })), 'r')`,
    elementThenGetterThrows: m => `var q = Promise.resolve(1); Object.defineProperty(q, 'then', { get() { throw new Error('gthen'); } }); H(Promise.${m}(IT([q])), 'r')`,
    thenOrder: m => `H(Promise.${m}([mkp(1), mkp(2), mkp(3)]), 'r')`,
    thenOrderThenables: m => `H(Promise.${m}([thenable(1, 'a'), Promise.resolve(2), thenable(3, 'c')]), 'r')`,
    empty: m => `H(Promise.${m}([]), 'r')`,
    emptyIter: m => `H(Promise.${m}(IT([])), 'r')`,
    string: m => `H(Promise.${m}('ab'), 'r')`,
    holes: m => `H(Promise.${m}([, 1, , 2]), 'r')`,
    set: m => `H(Promise.${m}(new Set([1, 2, 1])), 'r')`,
    map: m => `H(Promise.${m}(new Map([[1, 2]]).values()), 'r')`,
    generator: m => `H(Promise.${m}((function* () { L('g0'); yield 1; L('g1'); yield Promise.resolve(2); L('g2'); })()), 'r')`,
    generatorThrows: m => `H(Promise.${m}((function* () { yield 1; throw new Error('gen'); })()), 'r')`,
    generatorFinally: m => `H(Promise.${m}((function* () { try { yield 1; yield 2; } finally { L('gen-fin'); } })()), 'r')`,
    nonIterNum: m => `H(Promise.${m}(1), 'r')`,
    nonIterUndef: m => `H(Promise.${m}(), 'r')`,
    nonIterNull: m => `H(Promise.${m}(null), 'r')`,
    nonIterObj: m => `H(Promise.${m}({}), 'r')`,
    iteratorGetterThrows: m => `H(Promise.${m}({ get [Symbol.iterator]() { L('get'); throw new Error('git'); } }), 'r')`,
    iteratorThrows: m => `H(Promise.${m}({ [Symbol.iterator]() { L('iter'); throw new Error('it'); } }), 'r')`,
    iteratorNotObject: m => `H(Promise.${m}({ [Symbol.iterator]() { L('iter'); return 1; } }), 'r')`,
    nextNotCallable: m => `H(Promise.${m}({ [Symbol.iterator]() { L('iter'); return { next: 1 }; } }), 'r')`,
    nextGetterOnce: m => `var n = 0; H(Promise.${m}({ [Symbol.iterator]() { return { get next() { L('get-next'); var i = 0; return () => { L('next'); return i++ < 2 ? { done: false, value: i } : { done: true }; }; } }; } }), 'r')`,
    nextReturnsPrim: m => `H(Promise.${m}({ [Symbol.iterator]() { return { next() { L('next'); return 1; } }; } }), 'r')`,
    nextReturnsNull: m => `H(Promise.${m}({ [Symbol.iterator]() { return { next() { L('next'); return null; } }; } }), 'r')`,
    nextReturnsUndef: m => `H(Promise.${m}({ [Symbol.iterator]() { return { next() { L('next'); } }; } }), 'r')`,
    doneTruthy: m => `H(Promise.${m}({ [Symbol.iterator]() { var i = 0; return { next() { L('next'); return { done: i++ < 2 ? 0 : 'yes', value: i }; } }; } }), 'r')`,
    thisUndefined: m => `try { H(Promise.${m}.call(undefined, []), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisNumber: m => `try { H(Promise.${m}.call(1, []), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisObject: m => `try { H(Promise.${m}.call({}, []), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisArrow: m => `try { H(Promise.${m}.call(() => {}, []), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisHostileCtor: m => `try { H(Promise.${m}.call(function (ex) { L('ctor'); }, IT([1])), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisCtorCallsExecTwice: m => `try { H(Promise.${m}.call(function (ex) { ex(() => {}, () => {}); ex(() => {}, () => {}); }, IT([1])), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisCtorBadResolve: m => `try { H(Promise.${m}.call(function (ex) { ex(1, 2); }, IT([1])), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    thisCtorThrows: m => `try { H(Promise.${m}.call(function (ex) { throw new Error('cx'); }, IT([1])), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    ctorWithIterBad: m => `try { H(Promise.${m}.call(function (ex) { throw new Error('cx'); }, 1), 'r'); } catch (e) { L('sync ' + show(e)); }`,
    classC: m => `H(C.${m}(IT([1, Promise.resolve(2)])), 'r')`,
    classCThens: m => `H(C.${m}([1, 2]), 'r')`,
    twiceFns: m => `H(Promise.${m}([{ then(f, r) { f('a'); f('b'); r('c'); r('d'); } }, 2]), 'r')`,
    twiceFnsReject: m => `H(Promise.${m}([{ then(f, r) { r('a'); r('b'); f('c'); } }, Promise.resolve(2)]), 'r')`,
    outOfOrder: m => `var d = []; var P = i => new Promise(r => d[i] = r); H(Promise.${m}([P(0), P(1), P(2)]), 'r'); d[2]('c'); d[0]('a'); d[1]('b');`,
    outOfOrderRejects: m => `var d = [], e = []; var P = i => new Promise((r, j) => { d[i] = r; e[i] = j; }); H(Promise.${m}([P(0), P(1), P(2)]), 'r'); e[2]('c'); d[0]('a'); e[1]('b');`,
    mixedTicks2: m => `H(Promise.${m}([thenable(1, 'a'), Promise.resolve(2), 3, Promise.reject(4)]), 'r'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');`,
    mixedTicksReject: m => `H(Promise.${m}([Promise.reject(1), Promise.resolve(2), thenable(3, 'c')]), 'r'); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`,
    asyncItems: m => `H(Promise.${m}([(async () => { await null; return 1; })(), (async () => 2)(), (async () => { throw 3; })()]), 'r'); tick(2, 't2'); tick(4, 't4');`,
    selfRef: m => `var rs; var p = new Promise(r => rs = r); var q = Promise.${m}([p]); H(q, 'r'); rs(q); tick(3, 't3');`,
    sameItems: m => `var p = Promise.resolve(1); H(Promise.${m}([p, p, p]), 'r'); tick(3, 't3');`,
    resultIsNew: m => `var a = [1]; var q = Promise.${m}(a); L(String(q instanceof Promise)); H(q, 'r'); a.push(2);`,
    arrayMutatedDuring: m => `var a = [1, 2]; var it = a[Symbol.iterator](); var n = 0; var o = { [Symbol.iterator]() { return { next() { var r = it.next(); if (n++ === 0) a.push(3); return r; } }; } }; H(Promise.${m}(o), 'r')`,
  };
  const CSLOG = new Set(["classC", "classCThens"]);
  for (const m of ["all", "allSettled", "any", "race"]) {
    for (const [sn, s] of Object.entries(scen)) {
      const body = s(m);
      const needsIT = body.includes("IT(");
      const needsC = CSLOG.has(sn);
      const needsMkp = body.includes("mkp(");
      add(prog(`${body}; tick(5, 't5');`, (needsIT ? IT : "") + (needsC ? CS : "") + (needsMkp ? MKP : "")));
    }
  }
}

// 8. AggregateError: ordem de errors e forma.
forceKeep = true;
{
  add(
    prog(`H(Promise.any([Promise.reject(1), Promise.reject(2), Promise.reject(3)]), 'r');`),
    prog(`var d = [], j = []; var P = i => new Promise((res, rej) => { d[i] = res; j[i] = rej; }); H(Promise.any([P(0), P(1), P(2)]), 'r'); j[2]('c'); j[0]('a'); j[1]('b');`),
    prog(`Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => { L(e.constructor.name + e.name + e.message + JSON.stringify(e.errors) + Array.isArray(e.errors)); });`),
    prog(`Promise.any([Promise.reject(1)]).catch(e => { L(JSON.stringify(Object.getOwnPropertyDescriptor(e, 'errors')).replace(/\\[.*\\]/, 'arr')); L(Object.getOwnPropertyNames(e).sort().join()); });`),
    prog(`Promise.any([]).catch(e => { L(e.constructor.name + '|' + e.message + '|' + e.errors.length + '|' + Object.getOwnPropertyNames(e).sort().join()); });`),
    prog(`Promise.any(IT([])).catch(e => { L(e.message + e.errors.length); });`, IT),
    prog(`Promise.any([Promise.reject(new Error('x')), Promise.reject(new TypeError('y'))]).catch(e => { L(e.errors.map(x => x.name + x.message).join()); L(e.stack === undefined ? 'nostack' : typeof e.stack); });`),
    prog(`Promise.any([thenable(1, 'a'), Promise.reject(2)]).then(v => L('v' + v), e => L('e ' + show(e)));`),
    prog(`Promise.any([{ then(f, r) { r('a'); } }, { then(f, r) { r('b'); } }]).catch(e => L(JSON.stringify(e.errors)));`),
    prog(`Promise.any([{ then(f, r) { r('a'); r('z'); } }, { then(f, r) { r('b'); } }]).catch(e => L(JSON.stringify(e.errors)));`),
    prog(`Promise.any([{ then(f, r) { setLater = r; } }, Promise.reject('b')]).catch(e => L(JSON.stringify(e.errors) + e.errors.length)); tick(3, 't3'); setLater('late'); tick(5, 't5');`),
    prog(`Promise.any([, 1]).then(v => L('v' + v));`),
    prog(`Promise.any([Promise.reject(1), , Promise.reject(3)]).then(v => L('v' + String(v)));`),
    prog(`Promise.any([Promise.reject(1), 2, Promise.reject(3)]).then(v => L('v' + v), e => L('e' + show(e))); tick(3, 't3');`),
    prog(`Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => { L(String(e instanceof AggregateError) + String(e instanceof Error) + String(Object.getPrototypeOf(e) === AggregateError.prototype)); });`),
    prog(`var e = new AggregateError([1, 2, 3], 'msg'); L(JSON.stringify(e.errors) + e.message + e.name + String(e.cause));`),
    prog(`var e = new AggregateError([1], 'm', { cause: 'c' }); L(e.cause + JSON.stringify(Object.getOwnPropertyDescriptor(e, 'cause')) + Object.getOwnPropertyNames(e).sort().join());`),
    prog(`var e = new AggregateError([1], undefined); L(String(Object.hasOwn(e, 'message')) + String(e.message === ''));`),
    prog(`var e = AggregateError([1, 2], 'f'); L(JSON.stringify(e.errors) + e.message + String(e instanceof AggregateError));`),
    prog(`var e = new AggregateError(new Set([3, 1, 2])); L(JSON.stringify(e.errors));`),
    prog(`var e = new AggregateError('abc'); L(JSON.stringify(e.errors));`),
    prog(`try { new AggregateError(); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
    prog(`try { new AggregateError(5); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
    prog(`try { new AggregateError(IT([1, 2], { nextThrows: 1 })); } catch (e) { L(e.constructor.name + ':' + e.message); }`, IT),
    prog(`var e = new AggregateError(IT([1, 2]), { toString() { L('tostr'); return 'ts'; } }, { get cause() { L('cause'); return 1; } }); L(e.message);`, IT),
    prog(`var o = { get message() { L('msg'); return 'm'; }, get errors() { L('errors'); return []; } }; try { new AggregateError(IT([1]), o.message); } catch (e) { L(show(e)); }`, IT),
    prog(`var e = new AggregateError([1]); e.errors.push(2); L(JSON.stringify(e.errors)); e.errors = 5; L(String(e.errors));`),
    prog(`var e = new AggregateError([1]); L(Object.prototype.toString.call(e) + String(e)); L(JSON.stringify(Object.keys(e)));`),
    prog(`class MyAgg extends AggregateError { constructor(a) { super(a, 'my'); L('MyAgg'); } } var e = new MyAgg([1]); L(e.message + e.name + String(e instanceof MyAgg));`),
    prog(`var e = Reflect.construct(AggregateError, [[1], 'x'], function () {}.bind()); L(String(Object.getPrototypeOf(e) === AggregateError.prototype));`),
    prog(`class S extends Promise {} S.any([S.reject(1)]).catch(e => L(String(e instanceof AggregateError) + e.errors.length));`),
    prog(`var orig = globalThis.AggregateError; globalThis.AggregateError = function () { L('replaced'); }; Promise.any([Promise.reject(1)]).catch(e => L(String(e instanceof orig))); `),
    prog(`Promise.any(IT([Promise.reject(1), Promise.reject(2)])).catch(e => L(JSON.stringify(e.errors))); tick(4, 't4');`, IT),
    prog(`Promise.any(IT([Promise.reject(1), Promise.reject(2)], { returnThrows: true })).catch(e => L(JSON.stringify(e.errors))); tick(4, 't4');`, IT),
    prog(`var ps = []; for (var i = 0; i < 5; i++) ps.push(i % 2 ? Promise.reject(i) : new Promise((r, j) => j(i * 10))); Promise.any(ps).catch(e => L(JSON.stringify(e.errors)));`),
    prog(`var ps = [thenable(1, 'x'), thenable(2, 'y')]; Promise.any(ps).then(v => L('v' + v)); tick(4, 't4');`),
    prog(`Promise.any([new Promise((_, j) => j('slow')), Promise.resolve('fast')]).then(v => L(v)); tick(3, 't3');`),
    prog(`Promise.any([(async () => { await null; throw 'a'; })(), (async () => { throw 'b'; })()]).catch(e => L(JSON.stringify(e.errors))); tick(3, 't3');`),
    prog(`Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => { L(JSON.stringify(e.errors)); return e.errors; }).then(a => L(String(Array.isArray(a)))); tick(4, 't4');`),
    prog(`Promise.allSettled([Promise.reject(1), Promise.resolve(2), thenable(3, 't')]).then(r => L(JSON.stringify(r)));`),
    prog(`Promise.allSettled([Promise.reject(1)]).then(r => L(Object.keys(r[0]).join() + JSON.stringify(Object.getOwnPropertyDescriptor(r[0], 'status'))));`),
    prog(`Promise.allSettled([1]).then(r => L(String(Object.getPrototypeOf(r[0]) === Object.prototype) + Object.keys(r[0]).join()));`)
  );
}

forceKeep = false;
// 9. finally: thenables, `this` inválido, não chamáveis, species, ticks.
{
  const ret = {
    undef: "undefined",
    value: "5",
    native: "Promise.resolve(5)",
    nativeRej: "Promise.reject(new Error('f'))",
    thenable: "thenable(5, 'ft')",
    thenableRej: "{ then(f, r) { L('ft'); r('tr'); } }",
    throws: "(() => { throw new Error('thr'); })()",
    nestedP: "Promise.resolve(Promise.resolve(5))",
    asyncFn: "(async () => { await null; return 5; })()",
    asyncThrow: "(async () => { await null; throw 7; })()",
    getterThen: "{ get then() { L('get'); return f => f(1); } }",
  };
  const src = {
    ful: "Promise.resolve('v')",
    rej: "Promise.reject('e')",
    thenableSrc: "Promise.resolve(thenable('v', 's'))",
  };
  for (const [rn, r] of Object.entries(ret)) {
    for (const [sn, s] of Object.entries(src)) {
      const body = rn === "throws" ? `${s}.finally(() => { L('fin'); throw new Error('thr'); })` : `${s}.finally(() => { L('fin'); return ${r}; })`;
      add(prog(`H(${body}, 'r'); tick(1, 't1'); tick(3, 't3'); tick(5, 't5'); tick(7, 't7');`));
    }
  }
  const thisVals = {
    undef: "undefined",
    num: "1",
    str: "'s'",
    obj: "{}",
    objThen: "{ then(f, r) { L('then'); f(1); } }",
    objThenGetter: "{ get then() { L('get'); return undefined; } }",
    objThenThrows: "{ get then() { throw new Error('gt'); } }",
    fn: "function () {}",
    nullv: "null",
    thenableCtor: "Object.assign(Promise.resolve(1), { constructor: undefined })",
    promiseCtorObj: "Object.assign(Promise.resolve(1), { constructor: { [Symbol.species]: function (ex) { L('sp'); ex(() => {}, () => {}); } } })",
    thenOwn: "Object.assign(Promise.resolve(1), { then(f, r) { L('own'); return Promise.prototype.then.call(this, f, r); } })",
    thenReturnsPrim: "Object.assign(Promise.resolve(1), { then(f, r) { L('own ' + typeof f + typeof r + f.length + r.length); return 42; } })",
    proxyP: "new Proxy(Promise.resolve(1), { get(t, k, r) { L('get ' + String(k)); var v = Reflect.get(t, k, t); return typeof v === 'function' ? v.bind(t) : v; } })",
  };
  for (const [tn, t] of Object.entries(thisVals)) {
    add(
      prog(`try { var r = Promise.prototype.finally.call(${t}, () => L('fin')); L('ret ' + typeof r); if (r && r.then) H(r, 'r'); } catch (e) { L('sync ' + show(e)); } tick(4, 't4');`),
      prog(`try { var r = Promise.prototype.finally.call(${t}); L('ret ' + typeof r); } catch (e) { L('sync ' + show(e)); } tick(2, 't2');`)
    );
  }
  const fins = {
    notFn: "5",
    nullv: "null",
    obj: "{}",
    str: "'x'",
    cls: "class {}",
    undef: "undefined",
    sym: "Symbol()",
  };
  for (const [fn, f] of Object.entries(fins)) {
    add(
      prog(`var p = Promise.resolve(1); p.then = function (a, b) { L('then ' + (a === b) + typeof a); return Promise.prototype.then.call(this, a, b); }; try { H(p.finally(${f}), 'r'); } catch (e) { L('sync ' + show(e)); } tick(3, 't3');`),
      prog(`try { H(Promise.reject(1).finally(${f}), 'r'); } catch (e) { L('sync ' + show(e)); } tick(3, 't3');`)
    );
  }
  add(
    prog(`class S extends Promise {} var r = S.resolve(1).finally(() => {}); L(String(r instanceof S)); H(r, 'r');`),
    prog(`class S extends Promise { static get [Symbol.species]() { return Promise; } } var r = S.resolve(1).finally(() => {}); L(String(r instanceof S)); H(r, 'r');`),
    prog(`var p = Promise.resolve(1); var n = 0; p.constructor = { get [Symbol.species]() { L('species ' + ++n); return Promise; } }; p.finally(() => L('f')); tick(3, 't3'); `),
    prog(`var calls = []; var p = Promise.resolve(1); p.then = function (a, b) { calls.push(a.name, b.name); return Promise.prototype.then.call(this, a, b); }; p.finally(() => {}); L(calls.join());`),
    prog(`Promise.resolve(1).finally(() => L('a')).finally(() => L('b')).then(v => L('v' + v)); tick(7, 't7'); tick(9, 't9');`),
    prog(`Promise.reject(1).finally(() => L('a')).finally(() => L('b')).catch(v => L('e' + v)); tick(7, 't7'); tick(9, 't9');`),
    prog(`Promise.resolve(1).finally(() => 2).then(v => L('v' + v)); tick(5, 't5');`),
    prog(`Promise.resolve(1).finally(() => { throw 2; }).catch(v => L('e' + v)); tick(5, 't5');`),
    prog(`Promise.reject(1).finally(() => Promise.reject(2)).catch(v => L('e' + v)); tick(6, 't6');`),
    prog(`Promise.resolve(1).finally(function () { L(String(arguments.length) + String(this)); }); tick(2, 't2');`),
    prog(`'use strict'; Promise.resolve(1).finally(function () { L(String(arguments.length) + String(this)); }); tick(2, 't2');`),
    prog(`Promise.resolve(1).finally(function () { L(String(arguments.length)); }).then(() => L('x')); Promise.reject(1).finally(function () { L(String(arguments.length)); }).catch(() => {});`),
    prog(`(async () => { try { await Promise.reject(1).finally(() => L('f')); } catch (e) { L('c' + e); } })(); tick(5, 't5');`),
    prog(`(async () => { var v = await Promise.resolve(1).finally(() => {}); L('v' + v); })(); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');`)
  );
}

// 10. await de thenable contra promessa nativa: ticks exatos.
{
  const operands = {
    value: "1",
    native: "Promise.resolve(1)",
    thenable: "thenable(1, 'th')",
    thenableNested: "{ then(f) { L('outer'); f(thenable(1, 'in')); } }",
    nativeThenable: "Promise.resolve(thenable(1, 'th'))",
    subclass: "(class S extends Promise {}).resolve(1)",
    ctorOverride: "Object.assign(Promise.resolve(1), { constructor: Object })",
    ctorUndef: "Object.assign(Promise.resolve(1), { constructor: undefined })",
    ownThen: "Object.assign(Promise.resolve(1), { then(f) { L('own'); f(1); } })",
    pending: "new Promise(r => Promise.resolve().then(() => r(1)))",
    nativePending2: "new Promise(r => r(Promise.resolve(1)))",
    rejectedNative: "Promise.reject(1)",
    asyncResult: "(async () => 1)()",
    asyncAwaitResult: "(async () => { await 0; return 1; })()",
    getterThen: "{ get then() { L('get'); return f => f(1); } }",
    withResolversP: "(() => { var d = Promise.withResolvers(); d.resolve(1); return d.promise; })()",
  };
  for (const [on, o] of Object.entries(operands)) {
    for (const n of [1, 2, 3, 4, 5, 6]) {
      add(prog(`(async () => { try { await ${o}; } catch (e) {} L('after'); })(); tick(${n}, 't${n}');`));
    }
    add(
      prog(`(async () => { try { return ${o}; } catch (e) {} })().then(() => L('after'), () => L('after')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(5, 't5');`),
      prog(`Promise.resolve(${o}).then(() => L('after'), () => L('after')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`),
      prog(`new Promise(r => r(${o})).then(() => L('after'), () => L('after')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4');`)
    );
  }
}

// 11. then em promessa já resolvida e ordem de reações.
add(
  prog(`var p = Promise.resolve(1); p.then(v => L('a' + v)); p.then(v => L('b' + v)); p.then(v => L('c' + v)); L('sync');`),
  prog(`var p = Promise.resolve(1); L('before'); p.then(v => L('a')); L('mid'); p.then(v => L('b')); L('after'); tick(1, 't1');`),
  prog(`var p = Promise.reject(1); p.then(null, v => L('a' + v)); p.catch(v => L('b' + v)); p.then(undefined, v => L('c' + v));`),
  prog(`var p = Promise.resolve(1); var q = p.then(); H(q, 'q'); tick(2, 't2');`),
  prog(`var p = Promise.reject(1); var q = p.then(v => v); H(q, 'q'); tick(2, 't2');`),
  prog(`var p = Promise.resolve(1); var q = p.then(null, e => e); H(q, 'q'); tick(2, 't2');`),
  prog(`var p = Promise.resolve(1); var q = p.then(5, 6); H(q, 'q'); tick(2, 't2');`),
  prog(`var p = Promise.resolve(1); var q = p.then({}, {}); H(q, 'q'); tick(2, 't2');`),
  prog(`var p = Promise.resolve(1); L(String(p.then() === p) + String(p.then() instanceof Promise));`),
  prog(`var p = Promise.resolve(1); p.then(() => { L('a'); p.then(() => L('nested')); }); p.then(() => L('b')); tick(3, 't3');`),
  prog(`var p = Promise.resolve(1); p.then(() => { L('a'); throw new Error('x'); }).catch(e => L('c ' + e.message)); p.then(() => L('b')); tick(4, 't4');`),
  prog(`var p = Promise.resolve(1); var q = p.then(v => { L('a'); return Promise.resolve(2); }); q.then(v => L('q' + v)); p.then(() => L('b')).then(() => L('c')).then(() => L('d')).then(() => L('e')); tick(5, 't5');`),
  prog(`var p = Promise.resolve(1); var q = p.then(v => { L('a'); return thenable(2, 'x'); }); q.then(v => L('q' + v)); p.then(() => L('b')).then(() => L('c')).then(() => L('d')).then(() => L('e')); tick(5, 't5');`),
  prog(`var p = Promise.resolve(1); var args; p.then(function () { args = arguments.length + String(this); L(args); });`),
  prog(`var p = Promise.resolve(1); p.then.call(p, v => L('ok'));`),
  prog(`var then = Promise.prototype.then; try { then.call(1); } catch (e) { L(e.constructor.name + ':' + e.message); } try { then.call(undefined); } catch (e) { L(e.constructor.name + ':' + e.message); } try { then.call({}); } catch (e) { L(e.constructor.name + ':' + e.message); } try { then.call(Promise.prototype); } catch (e) { L(e.constructor.name + ':' + e.message); }`),
  prog(`var c = Promise.prototype.catch; try { c.call(1); } catch (e) { L(e.constructor.name + ':' + e.message); } L(String(c.call({ then(a, b) { L('then ' + typeof a + typeof b); return 'R'; } }, () => {})));`),
  prog(`var o = { then(a, b) { L('then ' + String(a) + typeof b); return 'R'; } }; L(String(Promise.prototype.catch.call(o, 5)));`),
  prog(`var o = { get then() { L('get'); return function (a, b) { L('call ' + String(a) + typeof b); }; } }; Promise.prototype.catch.call(o, 1); Promise.prototype.finally.call(o, 1);`),
  prog(`var p = Promise.resolve(1); var r = []; for (var i = 0; i < 4; i++) p.then(((i) => v => r.push(i))(i)); tick(1, 't1'); tick(2, 't2'); Promise.resolve().then(() => L(r.join()));`),
  prog(`var p = new Promise(r => r(1)); p.then(() => L('p1')); var q = new Promise(r => r(2)); q.then(() => L('q1')); p.then(() => L('p2')); q.then(() => L('q2'));`),
  prog(`var rs; var p = new Promise(r => rs = r); p.then(() => L('a')); p.then(() => L('b')); rs(1); p.then(() => L('c')); L('sync');`),
  prog(`var rs; var p = new Promise(r => rs = r); p.then(() => L('a')); rs(Promise.resolve(1)); p.then(() => L('b')); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');`),
  prog(`var p = Promise.resolve(1); p.then(() => L('x')); Promise.resolve().then(() => L('y')); p.then(() => L('z')); Promise.reject().catch(() => L('w'));`),
  prog(`var p = Promise.reject(new Error('e')); p.catch(() => {}); p.then(() => L('no'), e => L('yes ' + e.message)); L('sync');`),
  prog(`Promise.reject(1).then(null).catch(null).then(undefined, undefined).catch(v => L('e' + v)); tick(4, 't4'); tick(6, 't6');`),
  prog(`Promise.resolve(1).then(2).then(3).then(v => L('v' + v)); tick(3, 't3'); tick(5, 't5');`),
  prog(`var p = Promise.resolve(); for (var i = 0; i < 5; i++) p = p.then(() => L('s' + i)); tick(2, 't2'); tick(4, 't4');`),
  prog(`var p = Promise.resolve(); for (let i = 0; i < 5; i++) p = p.then(() => L('s' + i)); tick(2, 't2'); tick(4, 't4');`),
  prog(`Promise.resolve().then(() => { L('a'); return Promise.resolve(); }).then(() => L('b')); Promise.resolve().then(() => L('1')).then(() => L('2')).then(() => L('3')).then(() => L('4')).then(() => L('5'));`)
);

// 12. Ordem entre várias cadeias.
{
  const chains = {
    plain: id => `Promise.resolve().then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    retPromise: id => `Promise.resolve().then(() => { L('${id}1'); return Promise.resolve(); }).then(() => L('${id}2')).then(() => L('${id}3'))`,
    retThenable: id => `Promise.resolve().then(() => { L('${id}1'); return thenable(0, '${id}t'); }).then(() => L('${id}2')).then(() => L('${id}3'))`,
    throwCatch: id => `Promise.resolve().then(() => { L('${id}1'); throw 0; }).catch(() => L('${id}2')).then(() => L('${id}3'))`,
    asyncAwait: id => `(async () => { L('${id}0'); await null; L('${id}1'); await null; L('${id}2'); await null; L('${id}3'); })()`,
    asyncRetPromise: id => `(async () => { await null; L('${id}1'); return Promise.resolve(); })().then(() => L('${id}2')).then(() => L('${id}3'))`,
    finallyChain: id => `Promise.resolve().finally(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    allChain: id => `Promise.all([1, 2]).then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    raceChain: id => `Promise.race([Promise.resolve(1)]).then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    anyChain: id => `Promise.any([Promise.reject(1), 2]).then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    settledChain: id => `Promise.allSettled([1]).then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    ctorChain: id => `new Promise(r => r()).then(() => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
    rejChain: id => `Promise.reject().then(() => L('no'), () => L('${id}1')).then(() => L('${id}2')).then(() => L('${id}3'))`,
  };
  const names = Object.keys(chains);
  for (const a of names) {
    for (const b of names) {
      add(prog(`${chains[a]("a")}; ${chains[b]("b")}; L('sync');`));
    }
  }
  for (let i = 0; i < names.length; i++) {
    const a = names[i], b = names[(i + 3) % names.length], c = names[(i + 7) % names.length];
    add(prog(`${chains[a]("a")}; ${chains[b]("b")}; ${chains[c]("c")}; tick(2, 't2'); tick(4, 't4');`));
  }
}

// 13. Fila drenada por laço de awaits (sem setTimeout nem API de host).
{
  const works = {
    chain: "Promise.resolve().then(() => L('w1')).then(() => L('w2')).then(() => L('w3'))",
    asyncFn: "(async () => { await null; L('w1'); await null; L('w2'); })()",
    thenable: "Promise.resolve(thenable(1, 'w')).then(() => L('w1'))",
    all: "Promise.all([1, Promise.resolve(2), thenable(3, 'w')]).then(() => L('w-all'))",
    any: "Promise.any([Promise.reject(1), Promise.resolve(2)]).then(() => L('w-any'))",
    finallyW: "Promise.resolve(1).finally(() => L('w-fin')).then(() => L('w-after'))",
    ctorSub: "(class S extends Promise {}).resolve(1).then(() => L('w-sub'))",
    asyncGen: "(async function* () { yield 1; yield 2; })().next().then(() => L('w-gen'))",
    rejectNoHandler: "Promise.reject(1).catch(() => L('w-catch'))",
    deep: "Promise.resolve().then(() => Promise.resolve().then(() => Promise.resolve().then(() => L('w-deep'))))",
  };
  for (const [wn, w] of Object.entries(works)) {
    for (const n of [3, 6, 12]) {
      add(
        prog(`${w}; (async () => { for (var i = 0; i < ${n}; i++) await null; L('drained'); })();`),
        prog(`(async () => { ${w}; for (var i = 0; i < ${n}; i++) await undefined; L('drained ' + ${n}); })();`)
      );
    }
    add(prog(`(async () => { ${w}; var n = 0; while (log.length < 4 && n < 40) { await null; n++; } L('n=' + n); })();`));
  }
  add(
    prog(`(async () => { var order = []; for (var i = 0; i < 3; i++) Promise.resolve(i).then(v => order.push(v)); await null; await null; L(order.join()); })();`),
    prog(`(async () => { var rs; var p = new Promise(r => rs = r); p.then(() => L('p')); rs(); await p; L('after-await'); })();`),
    prog(`(async () => { var ps = [1, 2, 3].map(async x => { await null; L('x' + x); return x; }); L((await Promise.all(ps)).join()); })();`),
    prog(`(async () => { for (var i = 0; i < 3; i++) { await Promise.resolve(i); L('i' + i); } })(); (async () => { for (var j = 0; j < 3; j++) { await j; L('j' + j); } })();`),
    prog(`(async () => { for (var i = 0; i < 3; i++) { await thenable(i, 'a' + i); L('i' + i); } })(); (async () => { for (var j = 0; j < 3; j++) { await j; L('j' + j); } })();`),
    prog(`(async () => { for (var i = 0; i < 3; i++) { try { await Promise.reject(i); } catch (e) { L('c' + e); } } })(); (async () => { for (var j = 0; j < 3; j++) { await j; L('j' + j); } })();`),
    prog(`var chain = Promise.resolve(); for (var i = 0; i < 3; i++) chain = chain.then(() => (async () => { await null; L('n'); })()); chain.then(() => L('done')); tick(10, 't10');`)
  );
}

// Amostra determinística por hash (sampleByHash): as famílias forçadas entram inteiras e o resto é amostrado do conjunto
// candidato inteiro; só depois saem os que os goldens vizinhos já têm.
const TARGET = 520;
const sampledRest = new Set(sampleByHash(programs.filter((_, i) => !kept.has(i)), Math.max(0, TARGET - kept.size)));
const selectedPrograms = programs.filter((source, i) => (kept.has(i) || sampledRest.has(source)) && !existing.has(originalProgram(source)));

// Executa cada programa no bun, num processo próprio, como arquivo (ver async-golden.js).
measureBodies(selectedPrograms, "promise_more_case.js").then((lines) => {
  process.stdout.write(emitFactoredLines("promise_more", lines));
  process.stderr.write(`${selectedPrograms.length} de ${programs.length} programas\n`);
});
