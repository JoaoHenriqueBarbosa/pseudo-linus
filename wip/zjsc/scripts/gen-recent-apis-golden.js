// Gera tests/golden/recent_apis_bun.tsv: APIs recentes (Promise.withResolvers/try, Array.fromAsync, Iterator helpers,
// groupBy, métodos de Set, captureStackTrace/cause/AggregateError, base64/hex, Math.sumPrecise, RegExp.escape),
// avaliadas no bun 1.4.2. Cada API é medida antes (`typeof`), e as ausentes são puladas. Os programas usam L, tick e
// thenable de tests/golden/async_bun_harness.js e S, T, P de tests/golden/recent_apis_prelude.js (o mesmo texto que
// tests/recent_apis_bun_golden.rs embute). Colunas: fonte, depois o JSON do log depois de esvaziar as microtarefas,
// ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona. Cada programa roda num bun próprio.
// Uso: bun scripts/gen-recent-apis-golden.js > tests/golden/recent_apis_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness =
  fs.readFileSync(path.join(__dirname, "../tests/golden/async_bun_harness.js"), "utf8") +
  "\n" +
  fs.readFileSync(path.join(__dirname, "../tests/golden/recent_apis_prelude.js"), "utf8");
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
const has = (expr) => {
  try {
    return new Function("return typeof (" + expr + ") !== 'undefined'")();
  } catch (e) {
    return false;
  }
};
const section = (expr, body) => {
  if (has(expr)) body();
  else process.stderr.write(`ausente no bun: ${expr}\n`);
};

// Fontes de elementos usadas em várias APIs.
const syncSources = {
  array: "[1, 2, 3]",
  empty: "[]",
  holes: "[1, , 3]",
  string: "'abc'",
  set: "new Set([1, 2, 3])",
  map: "new Map([[1, 'a'], [2, 'b']])",
  generator: "(function* () { yield 1; yield 2; yield 3; })()",
  arrayLike: "({ length: 3, 0: 'a', 1: 'b', 2: 'c' })",
  arrayLikeBig: "({ length: 2, 0: Promise.resolve('p'), 1: 7 })",
  promises: "[Promise.resolve(1), 2, Promise.resolve(3)]",
  rejectedElem: "[1, Promise.reject(new Error('x')), 3]",
  nullish: "[null, undefined]",
};

// 1. Promise.withResolvers.
section("Promise.withResolvers", () => {
  add(
    "T(() => Object.keys(Promise.withResolvers()))",
    "T(() => Promise.withResolvers.length)",
    "T(() => Promise.withResolvers.name)",
    "const r = Promise.withResolvers(); T(() => [r.promise instanceof Promise, typeof r.resolve, typeof r.reject, r.resolve.length, r.reject.length])",
    "const r = Promise.withResolvers(); P(r.promise); r.resolve(5); r.resolve(6); r.reject(7);",
    "const r = Promise.withResolvers(); P(r.promise); r.reject(5); r.resolve(6);",
    "const r = Promise.withResolvers(); P(r.promise); r.resolve(Promise.resolve(9)); L('sync');",
    "const r = Promise.withResolvers(); P(r.promise); r.resolve(thenable(3, 'th')); L('sync');",
    "const r = Promise.withResolvers(); P(r.promise); r.resolve(r.promise);",
    "const r = Promise.withResolvers(); tick(1, 'a'); P(r.promise); Promise.resolve().then(() => r.resolve('late'));",
    "const { resolve } = Promise.withResolvers(); T(() => resolve(1))",
    "const { promise, resolve } = Promise.withResolvers.call(Promise); P(promise); resolve('x');",
    "T(() => Promise.withResolvers.call(undefined))",
    "T(() => Promise.withResolvers.call({}))",
    "T(() => Promise.withResolvers.call(function () {}))",
    "class MyP extends Promise {} const r = MyP.withResolvers(); T(() => r.promise instanceof MyP); r.resolve(1); P(r.promise);",
    "function C(ex) { ex(function (v) { L('res' + v); }, function (e) { L('rej' + e); }); } const r = Promise.withResolvers.call(C); T(() => Object.keys(r)); r.resolve(1); r.reject(2);",
    "function C(ex) { ex(function () {}, undefined); } T(() => Promise.withResolvers.call(C))",
    "function C(ex) { ex(1, 2); } T(() => Promise.withResolvers.call(C))",
    "T(() => Object.getOwnPropertyDescriptor(Promise, 'withResolvers').enumerable)",
    "const a = Promise.withResolvers(), b = Promise.withResolvers(); T(() => a.promise !== b.promise && a.resolve !== b.resolve)",
  );
  for (let n = 0; n <= 4; n++) {
    add(`const r = Promise.withResolvers(); r.promise.then(() => L('p')); tick(${n}, 't'); r.resolve(1);`);
    add(`const r = Promise.withResolvers(); r.promise.catch(() => L('c')); tick(${n}, 't'); r.reject(1);`);
    add(`const r = Promise.withResolvers(); r.resolve(Promise.resolve(1)); r.promise.then(() => L('p')); tick(${n}, 't');`);
  }
});

// 2. Promise.try.
section("Promise.try", () => {
  add(
    "T(() => Promise.try.length)",
    "T(() => Promise.try.name)",
    "P(Promise.try(() => 1)); L('sync');",
    "P(Promise.try(() => { throw new Error('e'); })); L('sync');",
    "P(Promise.try(() => Promise.resolve(2))); L('sync');",
    "P(Promise.try(() => Promise.reject(3))); L('sync');",
    "P(Promise.try((a, b) => [a, b], 1, 2));",
    "P(Promise.try((...args) => args.length, 1, 2, 3, 4));",
    "P(Promise.try(function () { 'use strict'; return this; }));",
    "P(Promise.try(() => thenable(4, 'th')));",
    "P(Promise.try(() => { L('inner'); return 1; })); L('after');",
    "T(() => Promise.try(5))",
    "T(() => Promise.try())",
    "P(Promise.try(5)); L('x');",
    "T(() => Promise.try.call(undefined, () => 1))",
    "T(() => Promise.try.call({}, () => 1))",
    "class MyP extends Promise {} const p = MyP.try(() => 1); T(() => p instanceof MyP); P(p);",
    "function C(ex) { ex(function (v) { L('res' + v); }, function (e) { L('rej' + e); }); } Promise.try.call(C, () => 8);",
    "function C(ex) { ex(function (v) { L('res' + v); }, function (e) { L('rej' + e); }); } Promise.try.call(C, () => { throw 9; });",
    "const p = Promise.resolve(1); P(Promise.try(() => p)); P(p);",
    "const p = Promise.resolve(1); T(() => Promise.try(() => p) === p)",
    "P(Promise.try(() => { throw undefined; }));",
    "P(Promise.try(async () => { await null; return 'a'; })); tick(1, 't1'); tick(2, 't2'); tick(3, 't3');",
  );
  for (let n = 0; n <= 4; n++) {
    add(`Promise.try(() => 1).then(() => L('p')); tick(${n}, 't');`);
    add(`Promise.try(() => Promise.resolve(1)).then(() => L('p')); tick(${n}, 't');`);
    add(`Promise.try(() => { throw 1; }).catch(() => L('c')); tick(${n}, 't');`);
  }
});

// 3. Array.fromAsync.
section("Array.fromAsync", () => {
  add("T(() => Array.fromAsync.length)", "T(() => Array.fromAsync.name)");
  for (const [name, src] of Object.entries(syncSources)) {
    add(`P(Array.fromAsync(${src})); L('${name}');`);
    add(`P(Array.fromAsync(${src}, async (x, i) => [x, i])); L('${name}');`);
    add(`P(Array.fromAsync(${src}, (x, i) => i + ':' + x));`);
    add(`P(Array.fromAsync(${src}, x => x, { tag: 1 }));`);
    add(`P(Array.fromAsync(${src}, function (x) { return this.k + String(x); }, { k: 'k' }));`);
  }
  const asyncSources = {
    asyncGen: "(async function* () { yield 1; yield Promise.resolve(2); yield 3; })()",
    asyncGenThrow: "(async function* () { yield 1; throw new Error('mid'); })()",
    asyncIterable: "{ [Symbol.asyncIterator]() { let i = 0; return { next() { L('next' + i); return Promise.resolve({ done: i >= 2, value: i++ }); } }; } }",
    both: "{ [Symbol.asyncIterator]() { return (async function* () { yield 'async'; })(); }, [Symbol.iterator]() { return ['sync'][Symbol.iterator](); } }",
    syncOnlyThenables: "{ [Symbol.iterator]() { let i = 0; return { next() { return { done: i >= 2, value: thenable(i++, 'e') }; } }; } }",
    nextNotObject: "{ [Symbol.asyncIterator]() { return { next() { return 5; } }; } }",
    nextRejects: "{ [Symbol.asyncIterator]() { return { next() { return Promise.reject(new Error('nr')); } }; } }",
    nullAsync: "{ [Symbol.asyncIterator]: null, length: 2, 0: 'x', 1: 'y' }",
    undefinedAsync: "{ [Symbol.asyncIterator]: undefined, [Symbol.iterator]: null, length: 1, 0: 'z' }",
    notCallable: "{ [Symbol.asyncIterator]: 5 }",
    lengthString: "{ length: '2', 0: 'a', 1: 'b' }",
    lengthNeg: "{ length: -1 }",
    lengthHuge: "{ length: 0, 5: 'ignored' }",
  };
  for (const [name, src] of Object.entries(asyncSources)) {
    add(`P(Array.fromAsync(${src})); L('${name}');`);
    add(`P(Array.fromAsync(${src}, x => x * 2));`);
    add(`P(Array.fromAsync(${src}, async x => x)); tick(3, 't');`);
  }
  add(
    "P(Array.fromAsync(null))",
    "P(Array.fromAsync(undefined))",
    "P(Array.fromAsync(5))",
    "P(Array.fromAsync({}))",
    "P(Array.fromAsync([1], 5))",
    "P(Array.fromAsync([1], null))",
    "P(Array.fromAsync([1], undefined))",
    "P(Array.fromAsync([1], x => { throw new Error('mapfn'); })); L('s');",
    "P(Array.fromAsync([1, 2, 3], x => Promise.reject(x))); L('s');",
    "P(Array.fromAsync([Promise.reject(1), Promise.resolve(2)]));",
    "P(Array.fromAsync([1, 2, 3], async x => { await tick(x, 'm' + x); return x; }));",
    "P(Array.fromAsync([3, 2, 1], async x => { await tick(x, 'm' + x); return x; }));",
    "tick(1, 'a'); P(Array.fromAsync([1, 2])); tick(2, 'b'); tick(5, 'c');",
    "tick(1, 'a'); P(Array.fromAsync([])); tick(2, 'b');",
    "tick(1, 'a'); P(Array.fromAsync((async function* () { yield 1; })())); tick(2, 'b'); tick(4, 'c');",
    "tick(1, 'a'); P(Array.fromAsync({ length: 1, 0: 1 })); tick(2, 'b'); tick(4, 'c');",
    "P(Array.fromAsync([1, 2, 3].values()));",
    "P(Array.fromAsync(new Map([[1, 2]]).keys()));",
    "P(Array.fromAsync('a\\u{1F600}b'));",
    "P(Array.fromAsync(new Uint8Array([1, 2, 3])));",
    "P(Array.fromAsync({ length: 3 }, (x, i) => i));",
    "P(Array.fromAsync({ length: 2 }, (x, i) => Promise.resolve(i * 10)));",
    "class A extends Array {} A.fromAsync([1, 2]).then(r => L(r instanceof A + ':' + r.length));",
    "function C() { L('ctor' + arguments.length); } C.fromAsync = Array.fromAsync; C.fromAsync([1, 2]).then(r => L(S(r)));",
    "function C(n) { L('ctor' + n); } C.fromAsync = Array.fromAsync; C.fromAsync({ length: 2, 0: 'a', 1: 'b' }).then(r => L(S(Object.keys(r))));",
    "const o = Array.fromAsync.call({}, [1, 2]); P(o);",
    "const o = Array.fromAsync.call(undefined, [1, 2]); P(o);",
    "const o = Array.fromAsync.call(5, [1, 2]); P(o);",
    "P(Array.fromAsync([1, 2], function () { return arguments.length; }));",
    "let c = 0; const it = { [Symbol.iterator]() { return { next() { c++; return { done: c > 3, value: c }; }, return() { L('ret'); return {}; } }; } }; P(Array.fromAsync(it, x => { if (x === 2) throw new Error('stop'); return x; }));",
    "const a = [1, 2, 3]; P(Array.fromAsync(a)); a.push(4);",
    "const a = [1, 2, 3]; P(Array.fromAsync(a, x => { if (x === 1) a.push(9); return x; }));",
    "P(Array.fromAsync(Object.defineProperty({ length: 2 }, 0, { get() { L('get0'); return 'g'; } })));",
  );
});

// 4. Iterator helpers.
section("Iterator.prototype.map", () => {
  const it = {
    arr: "[1, 2, 3, 4, 5].values()",
    gen: "(function* () { L('s'); yield 1; L('a'); yield 2; L('b'); yield 3; L('c'); })()",
    empty: "[].values()",
    str: "'ab'[Symbol.iterator]()",
    mapIt: "new Map([[1, 2]]).entries()",
    tracked: "({ i: 0, next() { L('n' + this.i); return { done: this.i > 2, value: this.i++ }; }, return() { L('return'); return {}; }, __proto__: Iterator.prototype })",
  };
  const fns = {
    map: ["x => x * 2", "(x, i) => x + ':' + i", "x => { throw new Error('f'); }", "x => [x]", "5"],
    filter: ["x => x % 2", "(x, i) => i > 0", "x => { throw new Error('f'); }", "() => true", "null"],
    flatMap: ["x => [x, x]", "x => 'ab'", "x => x", "x => new Set([x])", "x => ({ [Symbol.iterator]() { return [x].values(); } })", "x => (function* () { yield x; })()", "x => { throw new Error('f'); }", "x => [].values()"],
    forEach: ["x => L('e' + x)", "(x, i) => L(i + ':' + x)", "x => { throw new Error('f'); }"],
    some: ["x => x > 2", "x => false", "x => { throw new Error('f'); }"],
    every: ["x => x > 0", "x => x < 3", "x => { throw new Error('f'); }"],
    find: ["x => x > 2", "x => x > 100", "(x, i) => i === 1"],
  };
  for (const [method, list] of Object.entries(fns)) {
    for (const [name, src] of Object.entries(it)) {
      for (const f of list) {
        if (method === "forEach" || method === "some" || method === "every" || method === "find") add(`T(() => ${src}.${method}(${f}));`);
        else add(`T(() => Array.from(${src}.${method}(${f})));`);
      }
    }
  }
  const reducers = ["(a, b) => a + b", "(a, b, i) => a + b + i", "(a, b) => { throw new Error('r'); }"];
  for (const [name, src] of Object.entries(it)) {
    for (const r of reducers) {
      add(`T(() => ${src}.reduce(${r}));`, `T(() => ${src}.reduce(${r}, 10));`);
    }
    add(`T(() => ${src}.toArray());`);
  }
  for (const [name, src] of Object.entries(it)) {
    for (const n of ["0", "1", "2", "10", "-1", "NaN", "Infinity", "'2'", "undefined", "2.7", "{ valueOf() { return 1; } }", "-0"]) {
      add(`T(() => Array.from(${src}.take(${n})));`, `T(() => Array.from(${src}.drop(${n})));`);
    }
  }
  add(
    "T(() => Iterator.prototype.map.length)",
    "T(() => Iterator.prototype.reduce.length)",
    "T(() => Iterator.prototype.take.name)",
    "T(() => Object.getPrototypeOf(Iterator.prototype.map.call([].values()).constructor === undefined))",
    "T(() => { const h = [1].values().map(x => x); return [Object.prototype.toString.call(h), h[Symbol.toStringTag], typeof h.next, typeof h.return, h instanceof Iterator]; })",
    "T(() => { const h = [1, 2].values().map(x => x); return [h.next(), h.next(), h.next(), h.return()]; })",
    "T(() => { const g = (function* () { try { yield 1; yield 2; } finally { L('fin'); } })(); const h = g.map(x => x); h.next(); return h.return(); })",
    "T(() => { const g = (function* () { try { yield 1; yield 2; } finally { L('fin'); } })(); const h = g.take(1); return [h.next(), h.next(), h.next()]; })",
    "T(() => { const g = (function* () { try { yield 1; yield 2; } finally { L('fin'); } })(); const h = g.take(0); return [h.next()]; })",
    "T(() => { const g = (function* () { yield 1; yield 2; })(); const h = g.map(x => x); h.next(); h.next(); h.next(); return h.next(); })",
    "T(() => { const g = (function* () { yield 1; })(); const h = g.flatMap(x => 5); return h.next(); })",
    "T(() => { const g = (function* () { yield 1; })(); const h = g.flatMap(x => 'ab'); return Array.from(h); })",
    "T(() => { const h = [1].values().map(x => h2.next()); const h2 = h; return h.next(); })",
    "T(() => Iterator.prototype.map.call({ next() { return { done: true }; } }, x => x).next())",
    "T(() => Iterator.prototype.map.call(5, x => x))",
    "T(() => Iterator.prototype.map.call(undefined, x => x))",
    "T(() => Iterator.prototype.toArray.call({ i: 0, next() { return { done: this.i++ > 1, value: this.i }; } }))",
    "T(() => Iterator.prototype.some.call({ next() { return 5; } }, x => x))",
    "T(() => Iterator.prototype.forEach.call({ next() { return { done: true }; } }, x => x))",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; Iterator.prototype.some.call(o, x => true); return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; Iterator.prototype.find.call(o, x => true); return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; try { Iterator.prototype.map.call(o, 5); } catch (e) {} return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; try { Iterator.prototype.take.call(o, NaN); } catch (e) {} return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; try { Iterator.prototype.take.call(o, -1); } catch (e) {} return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; Iterator.prototype.take.call(o, 1).next(); return r; })",
    "T(() => { let r = 0; const o = { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; const h = Iterator.prototype.take.call(o, 1); h.next(); h.next(); return r; })",
    "T(() => { const o = { next() { return { done: false, value: 1 }; }, get return() { L('getret'); return undefined; } }; const h = Iterator.prototype.drop.call(o, 0); return h.return(); })",
    "T(() => Iterator.prototype[Symbol.toStringTag])",
    "T(() => Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag).get === undefined)",
    "T(() => Iterator.prototype.constructor === Iterator)",
    "T(() => { try { Iterator.prototype[Symbol.toStringTag] = 'x'; } catch (e) { return e.constructor.name; } return 'ok'; })",
    "T(() => new Iterator())",
    "T(() => Iterator())",
    "T(() => { class I extends Iterator {} const i = new I(); return [i instanceof Iterator, typeof i.map, i[Symbol.iterator]() === i]; })",
    "T(() => { class I extends Iterator { next() { return { done: true }; } } return new I().toArray(); })",
    "T(() => Iterator.prototype[Symbol.iterator].call(5))",
    "T(() => Iterator.prototype[Symbol.iterator].name)",
    "T(() => [].values().flatMap(x => x).next())",
    "T(() => [[1], [2]].values().flatMap(x => x).toArray())",
    "T(() => Array.from([1, 2].values().flatMap(x => [[x]])))",
    "T(() => [1].values().flatMap(x => ({ next() { return { done: true }; } })).toArray())",
    "T(() => [1].values().flatMap(x => Object('str')).toArray())",
    "T(() => [1].values().flatMap(x => Symbol()).toArray())",
  );
});

section("Iterator.from", () => {
  add(
    "T(() => Iterator.from.length)",
    "T(() => Iterator.from([1, 2]).toArray())",
    "T(() => Iterator.from('ab').toArray())",
    "T(() => Iterator.from(new Set([1, 2])).toArray())",
    "T(() => Iterator.from([1].values()) instanceof Iterator)",
    "T(() => { const a = [1].values(); return Iterator.from(a) === a; })",
    "T(() => { const o = { next() { return { done: true }; } }; const w = Iterator.from(o); return [w === o, w instanceof Iterator, typeof w.map, Object.getPrototypeOf(Object.getPrototypeOf(w)) === Iterator.prototype]; })",
    "T(() => { const o = { next() { return { done: true, value: 1 }; }, return() { L('r'); return { v: 1 }; } }; const w = Iterator.from(o); return [w.next(), w.return(), w.next === o.next]; })",
    "T(() => { const o = { next() { return { done: true }; } }; const w = Iterator.from(o); return w.return(); })",
    "T(() => Iterator.from(5))",
    "T(() => Iterator.from(null))",
    "T(() => Iterator.from(undefined))",
    "T(() => Iterator.from({}))",
    "T(() => Iterator.from({ [Symbol.iterator]: 5 }))",
    "T(() => Iterator.from({ [Symbol.iterator]: null, next() { return { done: true }; } }).next())",
    "T(() => Iterator.from({ [Symbol.iterator]() { return 5; } }))",
    "T(() => Iterator.from({ [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; } }; } }).take(2).toArray())",
    "T(() => Iterator.from(new String('xy')).toArray())",
    "T(() => Iterator.from(Object('ab')).toArray())",
    "T(() => Iterator.from({ next: 5 }).next)",
    "T(() => Iterator.from({ length: 1, 0: 'a' }))",
    "T(() => Iterator.from.call(undefined, [1]).toArray())",
    "T(() => { const w = Iterator.from({ next() { return { done: false, value: 7 }; } }); return [w.take(2).toArray(), w.next()]; })",
    "T(() => { const w = Iterator.from({ next() { return 5; } }); return w.next(); })",
    "T(() => Object.prototype.toString.call(Iterator.from({ next() {} })))",
  );
});

section("Iterator.concat", () => {
  add(
    "T(() => Iterator.concat.length)",
    "T(() => Iterator.concat().toArray())",
    "T(() => Iterator.concat([1, 2], [3]).toArray())",
    "T(() => Iterator.concat([1].values(), 'ab', new Set([9])).toArray())",
    "T(() => Iterator.concat(5))",
    "T(() => Iterator.concat([1], 5))",
    "T(() => Iterator.concat('ab'))",
    "T(() => Iterator.concat({ [Symbol.iterator]: 5 }))",
    "T(() => Iterator.concat({ next() {} }))",
    "T(() => { const c = Iterator.concat([1], [2]); return [c instanceof Iterator, c.next(), c.next(), c.next(), c.return()]; })",
    "T(() => { const g = (function* () { try { yield 1; yield 2; } finally { L('fin'); } })(); const c = Iterator.concat(g, [3]); c.next(); return c.return(); })",
    "T(() => { const c = Iterator.concat([1], [2]); c.next(); c.next(); return c.next(); })",
    "T(() => { const c = Iterator.concat([1], [2]); return Object.prototype.toString.call(c); })",
    "T(() => Iterator.concat([1].values(), [2].values()).map(x => x * 2).toArray())",
    "T(() => { let n = 0; const o = { [Symbol.iterator]() { n++; return [1][Symbol.iterator](); } }; const c = Iterator.concat(o, o); return [n, c.toArray(), n]; })",
    "T(() => Iterator.concat([1], { [Symbol.iterator]() { L('opened'); return [2][Symbol.iterator](); } }).next())",
    "T(() => Iterator.concat.call(undefined, [1]).toArray())",
  );
});

// 5. groupBy.
section("Object.groupBy", () => {
  const groupSources = { array: "[1, 2, 3, 4, 5]", empty: "[]", string: "'abcab'", set: "new Set([1, 2, 3])", gen: "(function* () { yield 1; yield 2; yield 3; })()", holes: "[1, , 3]", mapIt: "new Map([[1, 'a'], [2, 'b']])" };
  const keyFns = ["x => x % 2 ? 'odd' : 'even'", "x => typeof x", "(x, i) => i % 2", "x => x", "x => Symbol.for('s')", "x => ({ toString() { return 'obj'; } })", "x => 1.5", "x => -0", "x => undefined", "x => null", "x => { throw new Error('k'); }", "x => 5n", "x => true", "x => [x]"];
  for (const [name, src] of Object.entries(groupSources)) {
    for (const f of keyFns) {
      add(`T(() => Object.groupBy(${src}, ${f}));`);
      add(`T(() => Map.groupBy(${src}, ${f}));`);
    }
  }
  add(
    "T(() => Object.groupBy.length)",
    "T(() => Map.groupBy.length)",
    "T(() => Object.groupBy(null, x => x))",
    "T(() => Object.groupBy(undefined, x => x))",
    "T(() => Object.groupBy(5, x => x))",
    "T(() => Object.groupBy([1], 5))",
    "T(() => Object.groupBy([1]))",
    "T(() => Map.groupBy([1]))",
    "T(() => Map.groupBy(5, x => x))",
    "T(() => Object.getPrototypeOf(Object.groupBy([1], x => x)))",
    "T(() => Object.getOwnPropertyNames(Object.groupBy([1, 2, 10], x => x)))",
    "T(() => Object.keys(Object.groupBy(['b', 'a', '2', '1', 'a'], x => x)))",
    "T(() => Object.groupBy([1], x => '__proto__'))",
    "T(() => Object.getOwnPropertyDescriptor(Object.groupBy([1], x => 'k'), 'k'))",
    "T(() => { const m = Map.groupBy([1, 2, 3], x => x % 2); return [m.get(1), m.get(0), [...m.keys()]]; })",
    "T(() => { const m = Map.groupBy([NaN, NaN, 0, -0], x => x); return [...m.keys()].map(String).join(); })",
    "T(() => { const k = {}; const m = Map.groupBy([1, 2], x => k); return [m.size, m.get(k)]; })",
    "T(() => { const calls = []; Object.groupBy([5, 6], function (x, i) { calls.push([this === undefined, x, i]); return 'k'; }); return calls; })",
    "T(() => { const calls = []; Map.groupBy([5, 6], (...a) => { calls.push(a.length); return 1; }); return calls; })",
    "T(() => Object.groupBy('a\\u{1F600}', x => x.length))",
    "T(() => { let r = 0; const o = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; } }; try { Object.groupBy(o, () => { throw 1; }); } catch (e) {} return r; })",
    "T(() => { let r = 0; const o = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; } }; try { Map.groupBy(o, () => { throw 1; }); } catch (e) {} return r; })",
    "T(() => Object.groupBy(new Map([[1, 2]]), ([k, v]) => k + v))",
    "T(() => Object.groupBy({ length: 1, 0: 'a' }, x => x))",
    "T(() => typeof Array.prototype.group)",
    "T(() => typeof Array.prototype.groupBy)",
    "T(() => typeof Array.prototype.groupToMap)",
    "P(Promise.resolve([1, 2, 3]).then(a => Object.groupBy(a, x => x > 1)));",
  );
});

// 6. Set methods com set-likes exóticos.
section("Set.prototype.union", () => {
  const methods = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
  const base = "new Set([1, 2, 3])";
  const likes = {
    set: "new Set([2, 3, 4])",
    emptySet: "new Set()",
    map: "new Map([[3, 'x'], [5, 'y']])",
    plain: "{ size: 2, has(x) { return x === 1 || x === 9; }, keys() { return [1, 9].values(); } }",
    traced: "{ get size() { L('size'); return 2; }, get has() { L('has'); return x => x === 1; }, get keys() { L('keys'); return () => [1, 9].values(); } }",
    sizeNaN: "{ size: NaN, has() { return true; }, keys() { return [].values(); } }",
    sizeUndef: "{ has() { return true; }, keys() { return [].values(); } }",
    sizeString: "{ size: '2', has() { return true; }, keys() { return [1].values(); } }",
    sizeNeg: "{ size: -1, has() { return true; }, keys() { return [].values(); } }",
    sizeInf: "{ size: Infinity, has(x) { return x < 2; }, keys() { return [1, 5].values(); } }",
    sizeFrac: "{ size: 2.9, has() { return true; }, keys() { return [1].values(); } }",
    sizeBig: "{ size: 5n, has() { return true; }, keys() { return [].values(); } }",
    hasNotFn: "{ size: 1, has: 5, keys() { return [].values(); } }",
    keysNotFn: "{ size: 1, has() { return true; }, keys: null }",
    keysBadIter: "{ size: 1, has() { return true; }, keys() { return 5; } }",
    keysNoNext: "{ size: 1, has() { return true; }, keys() { return {}; } }",
    keysDup: "{ size: 3, has() { return true; }, keys() { return [1, 1, 4, 4].values(); } }",
    keysLoose: "{ size: 2, has() { return true; }, keys() { return { next() { return this.d ? { done: true } : (this.d = 1, { done: false, value: 7 }); } }; } }",
    keysMinusZero: "{ size: 1, has() { return false; }, keys() { return [-0].values(); } }",
    array: "[1, 2]",
    str: "'ab'",
    num: "5",
    nul: "null",
    arrayLikeSize: "{ size: 1, length: 1, 0: 1, has() { return true; }, keys() { return [1].values(); } }",
    hasTruthy: "{ size: 3, has(x) { return x ? 'yes' : 0; }, keys() { return [1, 2, 3].values(); } }",
    proxied: "new Proxy(new Set([1, 2]), {})",
    subclass: "new (class extends Set { has(x) { L('has' + x); return super.has(x); } })([1, 2, 5])",
  };
  for (const m of methods) {
    for (const [name, src] of Object.entries(likes)) add(`T(() => ${base}.${m}(${src}));`);
  }
  add(
    "T(() => Set.prototype.union.length)",
    "T(() => Set.prototype.isSubsetOf.name)",
    "T(() => Set.prototype.union.call({}, new Set()))",
    "T(() => Set.prototype.union.call(new Map(), new Set()))",
    "T(() => Set.prototype.union.call(5, new Set()))",
    "T(() => new Set().union())",
    "T(() => { class S2 extends Set {} const r = new S2([1]).union(new Set([2])); return [r.constructor.name, r instanceof S2, r]; })",
    "T(() => { const s = new Set([3, 2, 1]); return [...s.union(new Set([1, 5, 0]))]; })",
    "T(() => { const s = new Set([3, 2, 1]); return [...s.intersection(new Set([1, 2, 3, 4]))]; })",
    "T(() => { const s = new Set([3, 2, 1]); return [...s.intersection({ size: 10, has(x) { return true; }, keys() { return [1, 3].values(); } })]; })",
    "T(() => { const s = new Set([1, 2, 3]); return [...s.difference({ size: 1, has(x) { return x === 2; }, keys() { return [2].values(); } })]; })",
    "T(() => { const s = new Set([1, 2, 3]); return [...s.symmetricDifference({ size: 2, has() { return true; }, keys() { return [3, 4].values(); } })]; })",
    "T(() => { const s = new Set([1, 2, 3]); const o = { size: 5, has(x) { s.delete(2); return true; }, keys() { return [].values(); } }; return [[...s.isSubsetOf(o) + ''], [...s]]; })",
    "T(() => { const s = new Set([1, 2, 3]); const o = { size: 1, has(x) { return true; }, keys() { s.add(9); return [1].values(); } }; return [s.isSupersetOf(o), [...s]]; })",
    "T(() => { const s = new Set([1, 2, 3]); const o = { size: 5, has(x) { if (x === 1) s.delete(2); return x !== 3; }, keys() { return [].values(); } }; return [s.difference(o), [...s]]; })",
    "T(() => { const s = new Set([1, 2]); return [s.isSubsetOf(new Set([1, 2, 3])), s.isSupersetOf(new Set()), s.isDisjointFrom(new Set([3]))]; })",
    "T(() => { let n = 0; const o = { size: 1, has(x) { n++; return false; }, keys() { return [].values(); } }; new Set([1, 2, 3]).isDisjointFrom(o); return n; })",
    "T(() => { let n = 0; const o = { size: 5, has(x) { n++; return false; }, keys() { return [1, 2].values(); } }; new Set([1, 2, 3]).isDisjointFrom(o); return n; })",
    "T(() => { let r = 0; const o = { size: 5, has() { return false; }, keys() { return { next() { return { done: false, value: 1 }; }, return() { r++; return {}; } }; } }; new Set([1]).isSupersetOf(o); new Set([1]).isDisjointFrom(o); return r; })",
    "T(() => { let r = 0; const o = { size: 5, has() { return false; }, keys() { return { next() { return { done: false, value: 99 }; }, return() { r++; return {}; } }; } }; return [new Set([1]).isSupersetOf(o), r]; })",
    "T(() => { const s = new Set([1, 2]); return s.union(s) !== s; })",
    "T(() => Object.getPrototypeOf(new Set([1]).union(new Set())) === Set.prototype)",
    "T(() => new Set([1, 2, 3]).union(new Map([[1, 1], [8, 8]])))",
    "T(() => new Set([0]).union(new Set([-0])))",
    "T(() => [...new Set([NaN]).union(new Set([NaN]))])",
  );
});

// 7. Error.captureStackTrace, cause, AggregateError, Error.isError.
section("Error.captureStackTrace", () => {
  add(
    "T(() => Error.captureStackTrace.length)",
    "T(() => { const o = {}; Error.captureStackTrace(o); return [typeof o.stack, Object.getOwnPropertyDescriptor(o, 'stack') !== undefined]; })",
    "T(() => { const o = {}; Error.captureStackTrace(o); const d = Object.getOwnPropertyDescriptor(o, 'stack'); return [d.enumerable, d.configurable, d.writable, 'value' in d]; })",
    "T(() => Error.captureStackTrace(5))",
    "T(() => Error.captureStackTrace())",
    "T(() => Error.captureStackTrace(null))",
    "T(() => Error.captureStackTrace({}, 5))",
    "T(() => Error.captureStackTrace({}, function () {}))",
    "T(() => { const o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); return o.stack.split('\\n')[0]; })",
    "T(() => { const o = {}; Error.captureStackTrace(o); return o.stack.split('\\n')[0]; })",
    "T(() => { const o = Object.freeze({}); Error.captureStackTrace(o); return 1; })",
    "T(() => { const o = Object.preventExtensions({}); Error.captureStackTrace(o); return 1; })",
    "T(() => { const e = new Error('m'); Error.captureStackTrace(e); return e.stack.split('\\n')[0]; })",
    "T(() => { class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, E); } } return [new E('q').message, new E('q').stack.split('\\n')[0]]; })",
    "T(() => { function f() { const o = {}; Error.captureStackTrace(o, f); return o.stack.split('\\n').length > 0; } return f(); })",
    "T(() => typeof Error.stackTraceLimit)",
    "T(() => { const o = {}; const p = new Proxy(o, {}); Error.captureStackTrace(p); return typeof o.stack; })",
    "T(() => { const o = { get stack() { return 1; } }; Error.captureStackTrace(o); return typeof o.stack; })",
    "T(() => { const o = {}; Error.captureStackTrace(o); o.stack = 'custom'; return o.stack; })",
    "T(() => { const o = function () {}; Error.captureStackTrace(o); return typeof o.stack; })",
    "T(() => { const o = []; Error.captureStackTrace(o); return [o.length, typeof o.stack]; })",
    "T(() => { const o = {}; Error.captureStackTrace.call(null, o); return typeof o.stack; })",
  );
});
section("AggregateError", () => {
  add(
    "T(() => AggregateError.length)",
    "T(() => AggregateError.name)",
    "T(() => Object.getPrototypeOf(AggregateError) === Error)",
    "T(() => new AggregateError([1, 2]).errors)",
    "T(() => new AggregateError([1, 2], 'm').message)",
    "T(() => Object.getOwnPropertyNames(new AggregateError([1], 'm', { cause: 'c' })))",
    "T(() => Object.getOwnPropertyNames(new AggregateError([1])))",
    "T(() => Object.getOwnPropertyDescriptor(new AggregateError([1]), 'errors'))",
    "T(() => new AggregateError())",
    "T(() => new AggregateError(5))",
    "T(() => new AggregateError(null))",
    "T(() => new AggregateError('ab').errors)",
    "T(() => new AggregateError(new Set([1, 2])).errors)",
    "T(() => new AggregateError((function* () { yield 1; yield 2; })()).errors)",
    "T(() => AggregateError([3]).errors)",
    "T(() => AggregateError([3], 'x').message)",
    "T(() => new AggregateError([1], undefined).hasOwnProperty('message'))",
    "T(() => new AggregateError([1], null).message)",
    "T(() => new AggregateError([1], { toString() { return 'ts'; } }).message)",
    "T(() => new AggregateError([1], 'm', {}).hasOwnProperty('cause'))",
    "T(() => new AggregateError([1], 'm', { cause: undefined }).hasOwnProperty('cause'))",
    "T(() => new AggregateError([1], 'm', 5).hasOwnProperty('cause'))",
    "T(() => new AggregateError([1], 'm', { get cause() { L('getc'); return 1; } }).cause)",
    "T(() => AggregateError.prototype.name)",
    "T(() => AggregateError.prototype.message)",
    "T(() => Object.prototype.toString.call(new AggregateError([])))",
    "T(() => String(new AggregateError([], 'boom')))",
    "T(() => new AggregateError([]) instanceof Error)",
    "T(() => { class A extends AggregateError {} const a = new A([1], 'z'); return [a.name, a.errors, a instanceof AggregateError, a.constructor.name]; })",
    "T(() => Reflect.construct(AggregateError, [[1]], Object).constructor === Object)",
    "T(() => { const a = new AggregateError({ [Symbol.iterator]() { L('it'); return [1][Symbol.iterator](); } }, { toString() { L('msg'); return 'm'; } }); return a.message; })",
    "T(() => { const a = new AggregateError([1]); a.errors.push(2); return a.errors; })",
    "T(() => Object.keys(new AggregateError([1])))",
    "P(Promise.any([Promise.reject(1), Promise.reject(2)]));",
    "P(Promise.any([]));",
    "Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => L(S([e instanceof AggregateError, e.errors, e.message, Object.getOwnPropertyNames(e)])));",
    "Promise.any([Promise.reject(1), Promise.resolve(2)]).then(v => L(v));",
    "Promise.any([3, Promise.reject(1)]).then(v => L(v));",
    "Promise.any(5).catch(e => L(S(e)));",
    "Promise.any([Promise.reject(1)]).catch(e => { e.errors.push(5); L(S(e.errors)); });",
    "T(() => { const e = new AggregateError([new Error('a'), new TypeError('b')]); return e.errors.map(x => x.constructor.name); })",
  );
  // cause em Error e nos subtipos.
  for (const ctor of ["Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError"]) {
    add(
      `T(() => new ${ctor}('m', { cause: 1 }).cause)`,
      `T(() => Object.getOwnPropertyNames(new ${ctor}('m', { cause: 1 })))`,
      `T(() => Object.getOwnPropertyDescriptor(new ${ctor}('m', { cause: 1 }), 'cause'))`,
      `T(() => new ${ctor}('m', {}).hasOwnProperty('cause'))`,
      `T(() => new ${ctor}('m', { cause: undefined }).hasOwnProperty('cause'))`,
      `T(() => new ${ctor}('m', 5).hasOwnProperty('cause'))`,
      `T(() => new ${ctor}(undefined, { cause: 2 }).hasOwnProperty('message'))`,
      `T(() => ${ctor}('m', { cause: 3 }).cause)`,
      `T(() => Object.create({ cause: 4 }) && new ${ctor}('m', Object.create({ cause: 4 })).cause)`,
      `T(() => new ${ctor}('m', new Proxy({ cause: 6 }, { has(t, k) { L('has' + String(k)); return k in t; } })).cause)`,
      `T(() => ${ctor}.length)`,
    );
  }
});
section("Error.isError", () => {
  add(
    "T(() => Error.isError.length)",
    "T(() => Error.isError(new Error()))",
    "T(() => Error.isError(new TypeError()))",
    "T(() => Error.isError(new AggregateError([])))",
    "T(() => Error.isError({ __proto__: Error.prototype }))",
    "T(() => Error.isError(Object.create(Error.prototype)))",
    "T(() => Error.isError({ name: 'Error', message: '' }))",
    "T(() => Error.isError(Error.prototype))",
    "T(() => Error.isError(5))",
    "T(() => Error.isError())",
    "T(() => Error.isError(new Proxy(new Error(), {})))",
    "T(() => { class E extends Error {} return Error.isError(new E()); })",
    "T(() => { try { null.x; } catch (e) { return Error.isError(e); } })",
    "T(() => { const e = new Error(); Object.setPrototypeOf(e, null); return Error.isError(e); })",
    "T(() => Error.isError(new DOMException('x')))",
    "T(() => { function F() {} F.prototype = Error.prototype; return Error.isError(new F()); })",
  );
});

// 8. Uint8Array base64 e hex.
section("Uint8Array.fromBase64", () => {
  const b64 = ["''", "'AA=='", "'AAE='", "'AAEC'", "'SGVsbG8='", "'SGVsbG8'", "'SGVsbG8=='", "'SGVs bG8='", "'SGVs\\nbG8='", "'_-8='", "'/+8='", "'@@@@'", "'AB=='", "'AB='", "'A'", "'AAA'", "'AA==AA=='", "'  AA==  '", "'AA=='+'='", "'=AA='", "'\\u00e9AAA'", "'AAAA='", "'TWFu'", "'TWE='", "'TQ=='", "'TQ'", "'TR=='", "'TWF='"];
  const opts = ["", ", { alphabet: 'base64' }", ", { alphabet: 'base64url' }", ", { lastChunkHandling: 'loose' }", ", { lastChunkHandling: 'strict' }", ", { lastChunkHandling: 'stop-before-partial' }", ", { alphabet: 'x' }", ", { lastChunkHandling: 'x' }", ", { alphabet: 'base64url', lastChunkHandling: 'stop-before-partial' }"];
  for (const s of b64) {
    for (const o of opts) add(`T(() => Uint8Array.fromBase64(${s}${o}));`);
  }
  add(
    "T(() => Uint8Array.fromBase64.length)",
    "T(() => Uint8Array.fromBase64(5))",
    "T(() => Uint8Array.fromBase64())",
    "T(() => Uint8Array.fromBase64(new String('AA==')))",
    "T(() => Uint8Array.fromBase64('AA==', null))",
    "T(() => Uint8Array.fromBase64('AA==', 5))",
    "T(() => Uint8Array.fromBase64('AA==', { alphabet: undefined, lastChunkHandling: undefined }))",
    "T(() => Uint8Array.fromBase64('AA==', { alphabet: new String('base64') }))",
    "T(() => Uint8Array.prototype.setFromBase64.length)",
    "T(() => { const u = new Uint8Array(4); return [u.setFromBase64('SGVsbG8='), u]; })",
    "T(() => { const u = new Uint8Array(8); return [u.setFromBase64('SGVsbG8='), u]; })",
    "T(() => { const u = new Uint8Array(0); return [u.setFromBase64('SGVsbG8='), u]; })",
    "T(() => { const u = new Uint8Array(3); return [u.setFromBase64('SGVsbG8=', { lastChunkHandling: 'stop-before-partial' }), u]; })",
    "T(() => { const u = new Uint8Array(3); return [u.setFromBase64('SGV!'), u]; })",
    "T(() => { const u = new Uint8Array(3); return u.setFromBase64.call({}, 'AA=='); })",
    "T(() => { const u = new Uint16Array(3); return u.setFromBase64('AA=='); })",
    "T(() => Uint8Array.prototype.toBase64.length)",
    "T(() => new Uint8Array([]).toBase64())",
    "T(() => new Uint8Array([72, 101, 108, 108, 111]).toBase64())",
    "T(() => new Uint8Array([255, 254, 253, 252]).toBase64())",
    "T(() => new Uint8Array([255, 254, 253, 252]).toBase64({ alphabet: 'base64url' }))",
    "T(() => new Uint8Array([255, 254, 253, 252]).toBase64({ alphabet: 'base64url', omitPadding: true }))",
    "T(() => new Uint8Array([255, 254, 253, 252]).toBase64({ omitPadding: true }))",
    "T(() => new Uint8Array([1]).toBase64({ omitPadding: 1 }))",
    "T(() => new Uint8Array([1]).toBase64({ alphabet: 'x' }))",
    "T(() => new Uint8Array([1]).toBase64(5))",
    "T(() => new Uint8Array([1]).toBase64(null))",
    "T(() => Uint8Array.prototype.toBase64.call([1]))",
    "T(() => Uint8Array.prototype.toBase64.call(new Uint16Array([1])))",
    "T(() => { const u = new Uint8Array([1, 2, 3]); return Uint8Array.fromBase64(u.toBase64()).join(); })",
    "T(() => { const u = new Uint8Array(256).map((_, i) => i); return Uint8Array.fromBase64(u.toBase64({ alphabet: 'base64url' }), { alphabet: 'base64url' }).every((x, i) => x === i); })",
    "T(() => { const b = new ArrayBuffer(8, { maxByteLength: 16 }); const u = new Uint8Array(b); b.resize(2); return u.toBase64(); })",
    "T(() => { const u = new Uint8Array([1, 2]); const o = { get alphabet() { L('alpha'); u.buffer.transfer(); return 'base64'; } }; return u.toBase64(o); })",
  );
});
section("Uint8Array.fromHex", () => {
  const hex = ["''", "'00'", "'ff'", "'FF'", "'aBcD'", "'0'", "'abc'", "'zz'", "'0x00'", "'00 01'", "'0g'", "'\\u00e9\\u00e9'", "'0102030405060708090a0b0c0d0e0f'", "'ffff00'"];
  for (const s of hex) add(`T(() => Uint8Array.fromHex(${s}));`);
  add(
    "T(() => Uint8Array.fromHex.length)",
    "T(() => Uint8Array.fromHex(5))",
    "T(() => Uint8Array.fromHex())",
    "T(() => Uint8Array.fromHex(new String('00')))",
    "T(() => Uint8Array.prototype.setFromHex.length)",
    "T(() => { const u = new Uint8Array(2); return [u.setFromHex('010203'), u]; })",
    "T(() => { const u = new Uint8Array(4); return [u.setFromHex('0102'), u]; })",
    "T(() => { const u = new Uint8Array(4); return [u.setFromHex('01g2'), u]; })",
    "T(() => { const u = new Uint8Array(4); return [u.setFromHex('012'), u]; })",
    "T(() => new Uint8Array([]).toHex())",
    "T(() => new Uint8Array([0, 1, 15, 16, 127, 128, 255]).toHex())",
    "T(() => Uint8Array.prototype.toHex.call([1]))",
    "T(() => Uint8Array.prototype.toHex.call(new Int8Array([1])))",
    "T(() => Uint8Array.prototype.toHex.length)",
    "T(() => new Uint8Array(1000).toHex().length)",
    "T(() => { const u = new Uint8Array(300).map((_, i) => i); return Uint8Array.fromHex(u.toHex()).every((x, i) => x === i % 256); })",
    "T(() => typeof Uint8Array.fromBase64.call(class extends Uint8Array {}, 'AA==').constructor)",
    "T(() => Uint8Array.fromHex.call(Uint16Array, '00'))",
  );
});

// 9. Math.sumPrecise.
section("Math.sumPrecise", () => {
  const lists = ["[]", "[1]", "[1, 2, 3]", "[0.1, 0.2, 0.3]", "[0.1, 0.2, 0.3, -0.6]", "[1e308, 1e308]", "[1e308, 1e308, -1e308]", "[1e308, 1e308, -1e308, -1e308]", "[-0]", "[-0, -0]", "[0, -0]", "[-0, 0]", "[NaN]", "[Infinity]", "[-Infinity]", "[Infinity, -Infinity]", "[Infinity, 1]", "[NaN, Infinity]", "[1e100, 1, -1e100]", "[1e-320, 1e-320]", "[5e-324, 5e-324, -5e-324]", "[2 ** 53, 1, 1]", "[2 ** 53, 1]", "[1, 2 ** 53, 1]", "[0.5, 0.25, 0.125]", "[1.7976931348623157e308, 1.7976931348623157e308 * 2 ** -53]", "[1.7976931348623157e308, 9.979201547673598e291]", "[1.7976931348623157e308, 9.979201547673599e291]", "[3, 4].values()", "new Set([1, 2, 3])", "'abc'", "[1, 'a']", "[1, '2']", "[1n]", "[undefined]", "[null]", "[true]", "[{}]", "[1, , 3]", "5", "null", "undefined", "{}", "[1e16, 1, 1, 1, 1, 1, 1, 1, 1]", "[-1e16, 1, 1, 1]", "[0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]", "(function* () { yield 1; yield 2; })()", "[2 ** 1023, 2 ** 1023]", "[-(2 ** 1023), -(2 ** 1023)]", "[2 ** 1023, 2 ** 1023, -(2 ** 1023)]"];
  for (const l of lists) add(`T(() => Math.sumPrecise(${l}));`);
  add(
    "T(() => Math.sumPrecise.length)",
    "T(() => Math.sumPrecise())",
    "T(() => { let r = 0; const it = { [Symbol.iterator]() { return { next() { return { done: false, value: 'x' }; }, return() { r++; return {}; } }; } }; try { Math.sumPrecise(it); } catch (e) {} return r; })",
    "T(() => { let r = 0; const it = { [Symbol.iterator]() { return { i: 0, next() { return this.i++ < 2 ? { done: false, value: 1 } : { done: true }; }, return() { r++; return {}; } }; } }; return [Math.sumPrecise(it), r]; })",
    "T(() => Math.sumPrecise([{ valueOf() { L('vo'); return 1; } }]))",
    "T(() => Math.sumPrecise([Number.MAX_VALUE, Number.MAX_VALUE, -Number.MAX_VALUE]))",
    "T(() => Object.is(Math.sumPrecise([-0, -0, -0]), -0))",
    "T(() => Object.is(Math.sumPrecise([1, -1]), 0))",
  );
});

// 10. RegExp.escape.
section("RegExp.escape", () => {
  const strs = ["''", "'abc'", "'a.b'", "'a*b+c?'", "'[a-z]'", "'(x|y)'", "'^$'", "'\\\\'", "'/'", "'-'", "'a-b'", "'  '", "' a'", "'a b'", "'\\n'", "'\\t'", "'\\r'", "'\\u2028'", "'\\u2029'", "'\\ufeff'", "'\\u00a0'", "'1abc'", "'9'", "'_'", "'a_b'", "',=<>:;@#%&!~`\\\"\\''", "'{}'", "'\\u00e9'", "'\\u{1F600}'", "'\\ud83d'", "'\\ude00x'", "'a\\ud83d'", "'\\0'", "'\\x7f'", "'\\x1f'", "'0'", "'a1'", "'aB9'", "'\\u180e'", "'\\u3000'", "'\\u200b'", "'.'.repeat(5)", "'A'", "'Z'", "'z'", "' '", "'\\u00ff'", "'\\u0100'"];
  for (const s of strs) add(`T(() => RegExp.escape(${s}));`);
  add(
    "T(() => RegExp.escape.length)",
    "T(() => RegExp.escape.name)",
    "T(() => RegExp.escape(5))",
    "T(() => RegExp.escape())",
    "T(() => RegExp.escape(null))",
    "T(() => RegExp.escape(new String('a.b')))",
    "T(() => RegExp.escape({ toString() { return 'a.'; } }))",
    "T(() => RegExp.escape(Symbol()))",
    "T(() => new RegExp(RegExp.escape('a.b*c(d)[e]{f}|g^h$i\\\\j')).test('a.b*c(d)[e]{f}|g^h$i\\\\j'))",
    "T(() => new RegExp('^' + RegExp.escape('1+1=2') + '$').test('1+1=2'))",
    "T(() => new RegExp('^' + RegExp.escape('a-b') + '$', 'u').test('a-b'))",
    "T(() => new RegExp('^' + RegExp.escape('a b\\n') + '$', 'u').test('a b\\n'))",
    "T(() => new RegExp('^' + RegExp.escape('\\u{1F600}') + '$', 'u').test('\\u{1F600}'))",
    "T(() => new RegExp('^' + RegExp.escape('\\ud83d') + '$').test('\\ud83d'))",
    "T(() => new RegExp('^[' + RegExp.escape('a-z') + ']+$', 'u').test('-az'))",
    "T(() => 'x.y'.replace(new RegExp(RegExp.escape('.'), 'g'), '!'))",
    "T(() => RegExp.escape('a'.repeat(100)).length)",
  );
});

// 11. Combinações assíncronas de ordem entre as APIs.
add(
  "const r = Promise.withResolvers(); Array.fromAsync([r.promise, 2]).then(v => L(S(v))); r.resolve(1); tick(1, 'a');",
  "const r = Promise.withResolvers(); Array.fromAsync([r.promise, 2]).then(v => L(S(v))); tick(1, 'a'); tick(6, 'b'); r.resolve(1);",
  "Promise.try(() => Array.fromAsync([1, 2])).then(v => L(S(v))); tick(2, 'a'); tick(6, 'b');",
  "Array.fromAsync(Promise.try(() => [1, 2]).then(x => x)).then(v => L(S(v)));",
  "Array.fromAsync([1, 2, 3].values().map(x => x * 2)).then(v => L(S(v)));",
  "Array.fromAsync([1, 2, 3].values().filter(x => x > 1), async x => x + 1).then(v => L(S(v)));",
  "Array.fromAsync((async function* () { yield 1; yield 2; })(), x => x * 3).then(v => L(S(v))); tick(3, 'a'); tick(8, 'b');",
  "Promise.all([Array.fromAsync([1]), Array.fromAsync([2])]).then(v => L(S(v))); tick(2, 'a'); tick(8, 'b');",
  "Promise.race([Array.fromAsync([1]), Promise.resolve('r')]).then(v => L(S(v)));",
  "Promise.allSettled([Array.fromAsync([Promise.reject(1)]), Promise.try(() => 2)]).then(v => L(JSON.stringify(v)));",
  "Promise.any([Promise.try(() => { throw 1; }), Array.fromAsync([5])]).then(v => L(S(v)));",
  "const r = Promise.withResolvers(); r.promise.finally(() => L('fin')).then(() => L('after')); r.resolve(1); tick(3, 't');",
  "const r = Promise.withResolvers(); Promise.race([r.promise, Promise.resolve(2)]).then(v => L(S(v))); r.resolve(1);",
  "const r = Promise.withResolvers(); Promise.all([r.promise, 1]).then(v => L(S(v))); r.resolve(thenable(7, 'th')); tick(4, 't');",
  "const rs = [1, 2, 3].map(() => Promise.withResolvers()); Promise.all(rs.map(r => r.promise)).then(v => L(S(v))); rs[2].resolve(3); rs[0].resolve(1); rs[1].resolve(2);",
  "const rs = [1, 2, 3].map(() => Promise.withResolvers()); Promise.allSettled(rs.map(r => r.promise)).then(v => L(JSON.stringify(v))); rs[1].reject('b'); rs[0].resolve('a'); rs[2].resolve('c');",
  "(async () => { const r = Promise.withResolvers(); setTimeout; Promise.resolve().then(() => r.resolve('v')); L(await r.promise); L('end'); })(); L('sync');",
  "(async () => { try { await Promise.try(() => { throw new Error('t'); }); } catch (e) { L(e.message); } })(); tick(2, 'a');",
  "(async () => { L(S(await Array.fromAsync({ length: 2, 0: 1, 1: Promise.resolve(2) }))); })(); tick(1, 'a'); tick(5, 'b');",
  "(async () => { for await (const x of Iterator.from([1, 2]).map(x => x * 2)) L(x); })(); tick(2, 'a'); tick(6, 'b');",
  "(async () => { for await (const x of [1, 2].values().flatMap(x => [x, x])) L(x); })(); tick(2, 'a');",
  "(async () => { for await (const x of Iterator.from([Promise.resolve(1), 2])) L(S(x)); })(); tick(2, 'a');",
  "Iterator.from([1, 2, 3]).map(async x => x).toArray().forEach(p => p.then(v => L(v))); tick(1, 'a');",
  "const it = (async function* () { yield 1; yield 2; })(); T(() => typeof it.map); T(() => typeof it.toArray);",
  "T(() => typeof AsyncIterator);",
  "T(() => typeof Iterator.prototype.toAsync);",
  "T(() => Object.getOwnPropertyNames(Iterator.prototype).sort());",
  "T(() => Object.getOwnPropertyNames(Iterator).sort());",
  "T(() => Object.getOwnPropertyNames(Promise).sort());",
  "T(() => Object.getOwnPropertyNames(Array).sort());",
  "T(() => Object.getOwnPropertyNames(Set.prototype).sort());",
  "T(() => Object.getOwnPropertyNames(Object).sort());",
  "T(() => Object.getOwnPropertyNames(Math).sort());",
  "T(() => Object.getOwnPropertyNames(RegExp).sort());",
  "T(() => Object.getOwnPropertyNames(Error).sort());",
  "T(() => Object.getOwnPropertyNames(Uint8Array).sort());",
  "T(() => Object.getOwnPropertyNames(Uint8Array.prototype).sort());",
  "T(() => Object.getOwnPropertyNames(Map).sort());",
);

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "recent-apis-golden-"));
const lines = [];
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (let i = programs.length - 1; i >= 0; i--) if (HOST.test(programs[i])) programs.splice(i, 1);
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness + `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 0);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
