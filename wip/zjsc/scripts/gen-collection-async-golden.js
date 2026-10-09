// Gera tests/golden/collection_async_bun.tsv: métodos de conjunto (union, intersection, difference,
// symmetricDifference, isSubsetOf, isSupersetOf, isDisjointFrom) contra set-likes que registram cada acesso
// (size, has, keys, next, return), iteração de Map/Set com mutação entre awaits, Map.groupBy/Object.groupBy com
// registro de chamadas, Array.fromAsync (iteráveis síncronos e assíncronos, mapfn, this, ordem de microtarefas) e
// Iterator helpers (map, filter, take, drop, flatMap, reduce, toArray, some, every, find, forEach, Iterator.from,
// Iterator.concat) com erros e fechamento via return(), medidos no bun 1.4.2.
// Cada programa roda por `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do
// bun não tocar na fonte) num processo próprio, depois do prelúdio COLLECTION_ASYNC_HARNESS, também via vm. Os
// programas não usam API de host (setTimeout, process, console, require, Bun, URL, Buffer): só L, tick, S, T, TA,
// SL, KI e as funções do prelúdio. O golden é o JSON do log depois de esvaziar as microtarefas, ou
// `error<TAB>name<TAB>message JSON` se a fonte lançou de forma síncrona. O host do gerador drena as microtarefas
// com um setTimeout fora do programa. Programas repetidos de outros goldens são descartados. Caminho da máquina no
// resultado derruba a geração.
// Uso: bun scripts/gen-collection-async-golden.js > tests/golden/collection_async_bun.tsv
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Mesmo texto embutido em tests/collection_async_bun_golden.rs.
const HARNESS = `globalThis.log = [];
globalThis.L = function (x) { log.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.S = function S(v, d) {
  d = d || 0; var t = typeof v;
  if (v === null) return "null";
  if (t === "undefined") return "undefined";
  if (t === "string") return JSON.stringify(v);
  if (t === "symbol") return String(v);
  if (t === "bigint") return v + "n";
  if (t === "number") return Object.is(v, -0) ? "-0" : String(v);
  if (t === "boolean") return String(v);
  if (t === "function") return "fn";
  if (d > 4) return "...";
  if (v instanceof Error) return v.name + ":" + v.message;
  if (Array.isArray(v)) return "[" + Array.from(v, function (x) { return S(x, d + 1); }).join(",") + "]";
  if (v instanceof Set) return "Set(" + S(Array.from(v), d + 1) + ")";
  if (v instanceof Map) return "Map(" + S(Array.from(v), d + 1) + ")";
  return "{" + Object.keys(v).map(function (k) { return k + ":" + S(v[k], d + 1); }).join(",") + "}";
};
globalThis.T = function (f) {
  try { return S(f()); } catch (e) { return "throw " + (e && e.name) + ": " + (e && e.message); }
};
globalThis.TA = function (label, f) {
  try {
    return Promise.resolve(f()).then(function (v) { L(label + "=" + S(v)); }, function (e) { L(label + "!" + S(e)); });
  } catch (e) { L(label + "!!" + S(e)); }
};
globalThis.KI = function (arr, o) {
  o = o || {}; var i = 0;
  var it = {
    next: function () {
      L("next");
      if (o.throwAt === i) throw new Error("boom");
      return i < arr.length ? { value: arr[i++], done: false } : { value: undefined, done: true };
    },
  };
  if (!o.noReturn) it.return = function (v) { L("return"); if (o.returnThrows) throw new Error("rt"); return o.returnPrim ? 1 : {}; };
  it[Symbol.iterator] = function () { L("@@iterator"); return it; };
  return it;
};
globalThis.SL = function (o) {
  var r = {};
  Object.defineProperty(r, "size", { enumerable: true, get: function () { L("get size"); return "size" in o ? (typeof o.size === "function" ? o.size() : o.size) : undefined; } });
  Object.defineProperty(r, "has", { enumerable: true, get: function () {
    L("get has"); var h = o.has;
    return typeof h === "function" ? function (x) { L("has(" + S(x) + ")"); return h.call(this, x); } : h;
  } });
  Object.defineProperty(r, "keys", { enumerable: true, get: function () {
    L("get keys"); var k = o.keys;
    return typeof k === "function" ? function () { L("keys()"); return k.call(this); } : k;
  } });
  return r;
};
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(log); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
};`;

const root = path.join(__dirname, "..");
const existing = new Set();
for (const program of knownPrograms("collection_async_bun.tsv", ["collection_mutation_bun.tsv", "collections_bun.tsv", "iterator_bun.tsv", "async_gen_bun.tsv"])) existing.add(JSON.stringify(program));
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source) && !existing.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const SEE = "ok, bad";
const prefix = "globalThis.ok = function (v) { L('v:' + S(v)); }; globalThis.bad = function (e) { L('e:' + S(e)); }; ";

// ---- 1. Métodos de conjunto contra set-likes que registram acessos.
const methods = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
const receivers = ["new Set([1,2,3])", "new Set()", "new Set([3,1,'a'])"];
const has3 = "has: function (x) { return x === 3 || x === 2; }";
const kInt = (arr, extra) => `keys: function () { return KI(${arr}${extra ? ", " + extra : ""}); }`;
const likes = {
  normal: `SL({ size: 2, ${has3}, ${kInt("[3,4]")} })`,
  sizeBig: `SL({ size: 10, ${has3}, ${kInt("[3,4,5]")} })`,
  sizeZero: `SL({ size: 0, ${has3}, ${kInt("[]")} })`,
  sizeEqual: `SL({ size: 3, ${has3}, ${kInt("[1,2,9]")} })`,
  sizeInf: `SL({ size: Infinity, ${has3}, ${kInt("[1]")} })`,
  sizeNaN: `SL({ size: NaN, ${has3}, ${kInt("[1]")} })`,
  sizeUndef: `SL({ size: undefined, ${has3}, ${kInt("[1]")} })`,
  sizeMissing: `SL({ ${has3}, ${kInt("[1]")} })`,
  sizeNeg: `SL({ size: -1, ${has3}, ${kInt("[1]")} })`,
  sizeFrac: `SL({ size: 1.5, ${has3}, ${kInt("[3]")} })`,
  sizeStr: `SL({ size: '2', ${has3}, ${kInt("[3]")} })`,
  sizeNumStr: `SL({ size: 'abc', ${has3}, ${kInt("[3]")} })`,
  sizeBigInt: `SL({ size: 1n, ${has3}, ${kInt("[3]")} })`,
  sizeObj: `SL({ size: { valueOf: function () { L('valueOf'); return 2; } }, ${has3}, ${kInt("[3]")} })`,
  sizeThrows: `SL({ size: function () { throw new RangeError('sz'); }, ${has3}, ${kInt("[3]")} })`,
  sizeNegZero: `SL({ size: -0, ${has3}, ${kInt("[]")} })`,
  sizeNegHalf: `SL({ size: -0.5, ${has3}, ${kInt("[]")} })`,
  sizeTrue: `SL({ size: true, ${has3}, ${kInt("[3]")} })`,
  hasMissing: `SL({ size: 2, ${kInt("[3]")} })`,
  hasNotFn: `SL({ size: 2, has: 5, ${kInt("[3]")} })`,
  hasNull: `SL({ size: 2, has: null, ${kInt("[3]")} })`,
  keysMissing: `SL({ size: 2, ${has3} })`,
  keysNotFn: `SL({ size: 2, ${has3}, keys: 'k' })`,
  keysPrim: `SL({ size: 2, ${has3}, keys: function () { return 1; } })`,
  keysUndef: `SL({ size: 2, ${has3}, keys: function () { } })`,
  keysArray: `SL({ size: 2, ${has3}, keys: function () { return [3, 4]; } })`,
  nextMissing: `SL({ size: 2, ${has3}, keys: function () { return {}; } })`,
  nextNotFn: `SL({ size: 2, ${has3}, keys: function () { return { next: 1 }; } })`,
  nextPrim: `SL({ size: 2, ${has3}, keys: function () { return { next: function () { L('next'); return 1; } }; } })`,
  nextThrows: `SL({ size: 2, ${has3}, ${kInt("[3,4]", "{ throwAt: 1 }")} })`,
  nextThrowsFirst: `SL({ size: 2, ${has3}, ${kInt("[3,4]", "{ throwAt: 0 }")} })`,
  keysDup: `SL({ size: 4, ${has3}, ${kInt("[3,3,1,1]")} })`,
  keysNegZero: `SL({ size: 2, ${has3}, ${kInt("[-0, 0]")} })`,
  keysNaN: `SL({ size: 2, ${has3}, ${kInt("[NaN, 2]")} })`,
  hasThrows: `SL({ size: 2, has: function (x) { throw new EvalError('h' + x); }, ${kInt("[3]")} })`,
  hasTruthy: `SL({ size: 2, has: function (x) { return 'yes'; }, ${kInt("[3]")} })`,
  hasFalsy: `SL({ size: 2, has: function (x) { return 0; }, ${kInt("[3]")} })`,
  noReturn: `SL({ size: 2, ${has3}, ${kInt("[3,4]", "{ noReturn: true }")} })`,
  returnThrows: `SL({ size: 2, ${has3}, ${kInt("[3,4]", "{ returnThrows: true }")} })`,
  realSet: "new Set([2, 3, 9])",
  realMap: "new Map([[2, 'a'], [3, 'b'], [9, 'c']])",
  array: "[1, 2, 3]",
  plain: "{ size: 1, has: function () { return true; }, keys: function () { return [][Symbol.iterator](); } }",
  nullArg: "null",
  undefArg: "undefined",
  numArg: "3",
  strArg: "'abc'",
  missingArg: "",
};
for (const m of methods) {
  for (const r of receivers) {
    for (const [name, like] of Object.entries(likes)) {
      add(`L(T(function () { return ${r}.${m}(${like}); }));`);
    }
  }
}
// Mutação do próprio receptor durante has/keys.
for (const m of methods) {
  add(
    `var s = new Set([1,2,3]); var like = SL({ size: 3, has: function (x) { s.delete(2); s.add(7); return x === 1; }, keys: function () { s.delete(1); return KI([1,2,7]); } }); L(T(function () { return s.${m}(like); })); L(S(s));`,
    `var s = new Set([1,2,3]); var like = SL({ size: 1, has: function (x) { s.add(4); return true; }, keys: function () { s.add(5); return KI([1,2,3,4,5]); } }); L(T(function () { return s.${m}(like); })); L(S(s));`,
    `var s = new Set([1,2,3]); var like = SL({ size: 9, has: function (x) { s.clear(); return true; }, keys: function () { s.clear(); return KI([1,2]); } }); L(T(function () { return s.${m}(like); })); L(S(s));`,
    `class X extends Set { constructor(a) { L('ctor'); super(a); } has(x) { L('sub.has'); return super.has(x); } add(x) { L('sub.add'); return super.add(x); } } L(T(function () { return new X([1,2]).${m}(new Set([2,3])); }));`,
    `class X extends Set { static get [Symbol.species]() { L('species'); return Set; } } L(T(function () { var r = new X([1,2]).${m}(new Set([2,3])); return [r.constructor === Set, r]; }));`,
    `L(T(function () { return Set.prototype.${m}.call(new Map([[1,1]]), new Set([1])); })); L(T(function () { return Set.prototype.${m}.call({}, new Set([1])); })); L(T(function () { return Set.prototype.${m}.call(undefined, new Set()); }));`,
    `L(T(function () { return Set.prototype.${m}.call(new WeakSet(), new Set([1])); })); L(S([Set.prototype.${m}.length, Set.prototype.${m}.name]));`,
    `var s = new Set([1,2,3]); Set.prototype.add = function () { L('patched add'); return this; }; Set.prototype.has = function () { L('patched has'); return true; }; L(T(function () { return s.${m}(new Set([3,4])); }));`,
    `var like = { get size() { L('size'); return 2; }, get has() { L('has'); return function (x) { return x === 1; }; }, get keys() { L('keys'); return function () { L('call keys'); return KI([1,5]); }; } }; L(T(function () { return new Set([1,2]).${m}(like); }));`,
    `var like = new Proxy(new Set([1,5]), { get: function (t, k, r) { L('get ' + String(k)); var v = Reflect.get(t, k, t); return typeof v === 'function' ? v.bind(t) : v; } }); L(T(function () { return new Set([1,2]).${m}(like); }));`
  );
}
// Ordem de inserção do resultado.
for (const m of ["union", "intersection", "difference", "symmetricDifference"]) {
  for (const [a, b] of [["[3,1,2]", "[2,5,3]"], ["[1,2,3]", "[3,2,1]"], ["[]", "[4,3]"], ["[-0]", "[0]"], ["[NaN,1]", "[NaN]"], ["['a','b']", "['b','a','c']"], ["[1,2,3,4,5]", "[5,4]"], ["[1]", "[1,2,3,4,5,6]"]]) {
    add(`L(S(new Set(${a}).${m}(new Set(${b})))); L(S(new Set(${a}).${m}(new Map(${b}.map(function (x) { return [x, 0]; })))));`);
  }
}

// ---- 2. Map/Set: iteração com mutação entre awaits e em laços síncronos.
const colls = {
  set: "new Set([1,2,3])",
  map: "new Map([[1,'a'],[2,'b'],[3,'c']])",
};
const mutations = {
  delCurrent: "c.delete(1)",
  delNext: "c.delete(2)",
  delLast: "c.delete(3)",
  addNew: "c.add ? c.add(9) : c.set(9, 'z')",
  readdFirst: "c.delete(1); c.add ? c.add(1) : c.set(1, 'a')",
  clear: "c.clear()",
  clearAdd: "c.clear(); c.add ? c.add(8) : c.set(8, 'y')",
  none: "0",
};
const loops = {
  forOf: "for (var x of c) { L(S(x)); if (n++ < 1) { M; } if (n > 12) break; }",
  forEach: "c.forEach(function (v, k) { L(S([v, k])); if (n++ < 1) { M; } if (n > 12) throw new Error('loop'); });",
  forAwait: "(async function () { for await (var x of c) { L(S(x)); await null; if (n++ < 1) { M; } if (n > 12) break; } L('done'); })();",
  manual: "var it = c[Symbol.iterator](); var r = it.next(); L(S(r)); M; r = it.next(); L(S(r)); r = it.next(); L(S(r)); r = it.next(); L(S(r)); r = it.next(); L(S(r));",
  manualAwait: "(async function () { var it = c.values(); L(S(it.next())); await null; M; L(S(it.next())); await null; L(S(it.next())); L(S(it.next())); L(S(it.next())); })();",
  spreadLater: "var it = c.entries(); it.next(); M; L(S(Array.from(it))); L(S(Array.from(c)));",
  exhaustedThenAdd: "var it = c.keys(); Array.from(it); L(S(it.next())); if (c.add) c.add(5); else c.set(5, 'q'); L(S(it.next())); L(S(Array.from(c)));",
};
for (const [cn, ci] of Object.entries(colls)) {
  for (const [mn, mut] of Object.entries(mutations)) {
    for (const [ln, loop] of Object.entries(loops)) {
      add(`var c = ${ci}; var n = 0; ${loop.split("M;").join(mut + ";")} tick(6, 't6'); L(S(c));`);
    }
  }
}

// ---- 3. groupBy com registro de chamadas.
const groupSources = {
  arr: "[1,2,3,4,5]",
  str: "'abcab'",
  set: "new Set([1,2,3,4])",
  map: "new Map([[1,'a'],[2,'b']])",
  gen: "(function* () { L('g0'); yield 1; L('g1'); yield 2; L('g2'); })()",
  holes: "[1,,3]",
  keyed: "KI([1,2,3])",
  empty: "[]",
  nul: "null",
  undef: "undefined",
  num: "5",
  obj: "{}",
  arrlike: "{ length: 2, 0: 'x', 1: 'y' }",
};
const groupFns = {
  parity: "function (v, i) { L('cb ' + S(v) + ' ' + i); return v % 2; }",
  str: "function (v) { return String(v); }",
  sym: "function (v) { return Symbol.for('k' + v % 2); }",
  objKey: "function (v) { return { toString: function () { L('toString'); return 'k'; } }; }",
  throws: "function (v) { throw new SyntaxError('cb' + v); }",
  undef: "function () { }",
  nan: "function () { return NaN; }",
  negZero: "function (v) { return v === 1 ? -0 : 0; }",
  thisArg: "function () { return typeof this; }",
  none: "undefined",
  notFn: "5",
  arrow: "(v, i) => i < 2",
  nullKey: "function () { return null; }",
  bigint: "function (v) { return BigInt(v) % 2n; }",
  big: "function (v) { return v > 2 ? 'big' : 'small'; }",
};
for (const [sn, src] of Object.entries(groupSources)) {
  for (const [fn, f] of Object.entries(groupFns)) {
    add(
      `L(T(function () { var r = Map.groupBy(${src}, ${f}); return [r instanceof Map, Array.from(r)]; }));`,
      `L(T(function () { var r = Object.groupBy(${src}, ${f}); return [Object.getPrototypeOf(r), Reflect.ownKeys(r).map(String), r]; }));`
    );
  }
}
add(
  `L(S([Map.groupBy.length, Map.groupBy.name, Object.groupBy.length, Object.groupBy.name]));`,
  `L(T(function () { return Object.groupBy([1,2], function (v) { return v; }).hasOwnProperty; }));`,
  `L(T(function () { return Object.keys(Object.groupBy([3,1,2,1], function (v) { return v; })); }));`,
  `L(T(function () { var o = Object.groupBy(['a','b'], function () { return '__proto__'; }); return [Object.getPrototypeOf(o), Object.getOwnPropertyNames(o)]; }));`,
  `L(T(function () { return Map.groupBy.call(null, [1], function (v) { return v; }); }));`,
  `L(T(function () { return Map.groupBy.call(5, [1], function (v) { return v; }).size; }));`,
  `var thisSeen = []; Object.groupBy([1], function () { thisSeen.push(this); return 1; }); L(S(thisSeen.map(function (x) { return typeof x; })));`,
  `var thisSeen = []; Map.groupBy([1], function () { 'use strict'; thisSeen.push(this); return 1; }); L(S(thisSeen));`
);

// ---- 4. Array.fromAsync.
const fa = {
  arr: "[1,2,3]",
  arrP: "[Promise.resolve(1), 2, Promise.resolve(3)]",
  arrRej: "[1, Promise.reject(new Error('r1')), 3]",
  arrThen: "[{ then: function (res) { L('then'); res(7); } }, 8]",
  holes: "[1,,3]",
  str: "'ab\\u{1F600}'",
  set: "new Set([1,2])",
  map: "new Map([[1,2]])",
  arrayLike: "{ length: 3, 0: 'a', 1: Promise.resolve('b'), 2: 'c' }",
  arrayLikeLen: "{ length: '2', 0: 'a', 1: 'b', 2: 'c' }",
  arrayLikeBad: "{ length: -1, 0: 'a' }",
  arrayLikeInf: "{ length: Infinity }",
  empty: "[]",
  emptyObj: "{}",
  agen: "(async function* () { L('a0'); yield 1; L('a1'); yield Promise.resolve(2); L('a2'); })()",
  agenThrow: "(async function* () { yield 1; throw new Error('ag'); })()",
  gen: "(function* () { L('g0'); yield 1; L('g1'); yield Promise.resolve(2); L('g2'); })()",
  genThrow: "(function* () { yield 1; throw new Error('gt'); })()",
  genRejects: "(function* () { yield Promise.reject(new Error('gr')); L('after'); yield 2; })()",
  asyncIterLog: "{ [Symbol.asyncIterator]: function () { L('@@asyncIterator'); var i = 0; return { next: function () { L('next' + i); return Promise.resolve(i < 2 ? { value: i++, done: false } : { done: true }); }, return: function () { L('return'); return {}; } }; } }",
  asyncIterBadNext: "{ [Symbol.asyncIterator]: function () { return { next: function () { return 1; } }; } }",
  asyncIterNoNext: "{ [Symbol.asyncIterator]: function () { return {}; } }",
  asyncIterThenable: "{ [Symbol.asyncIterator]: function () { var i = 0; return { next: function () { return { then: function (res) { L('nthen'); res(i < 2 ? { value: i++, done: false } : { done: true }); } }; } }; } }",
  asyncIterNull: "{ [Symbol.asyncIterator]: null, [Symbol.iterator]: function () { L('sync used'); return [5][Symbol.iterator](); }, length: 1, 0: 'x' }",
  asyncIterUndef: "{ [Symbol.asyncIterator]: undefined, length: 1, 0: 'viaLen' }",
  asyncIterNotFn: "{ [Symbol.asyncIterator]: 5 }",
  iterNotFn: "{ [Symbol.iterator]: 5, length: 1, 0: 'q' }",
  both: "{ [Symbol.asyncIterator]: function () { L('async'); return (async function* () { yield 'A'; })(); }, [Symbol.iterator]: function () { L('sync'); return ['S'][Symbol.iterator](); } }",
  syncIterReturn: "KI([1,2,3])",
  syncIterThrows: "KI([1,2,3], { throwAt: 1 })",
  nul: "null",
  undef: "undefined",
  num: "5",
  bool: "true",
  sym: "Symbol('s')",
  nestedP: "Promise.resolve([1,2])",
  asyncOfAsync: "[(async function () { return 'in'; })(), 'x']",
  typed: "new Uint8Array([1,2,3])",
  args: "(function () { return arguments; })(1,2)",
};
const faMaps = {
  none: "",
  id: ", function (v, i) { L('map ' + S(v) + ' ' + i); return v; }",
  twice: ", function (v) { return v * 2; }",
  async: ", async function (v, i) { L('amap' + i); await null; return [v]; }",
  asyncReject: ", async function (v, i) { if (i === 1) throw new Error('mr'); return v; }",
  throws: ", function (v) { throw new RangeError('mt'); }",
  retP: ", function (v) { return Promise.resolve(v); }",
  retRej: ", function (v) { return Promise.reject(new Error('rr' + v)); }",
  thisArg: ", function () { return typeof this; }, 'ctx'",
  thisObj: ", function () { return this.tag; }, { tag: 'T' }",
  notFn: ", 5",
  nullFn: ", null",
  undefFn: ", undefined",
  objFn: ", {}",
};
for (const [sn, src] of Object.entries(fa)) {
  for (const [mn, mp] of Object.entries(faMaps)) {
    add(`${prefix}TA('r', function () { return Array.fromAsync(${src}${mp}); }); tick(14, 't14');`);
  }
}
add(
  `L(S([Array.fromAsync.length, Array.fromAsync.name, typeof Array.fromAsync]));`,
  `${prefix}var p = Array.fromAsync([1]); L(S([p instanceof Promise, Object.prototype.toString.call(p)])); p.then(ok, bad);`,
  `${prefix}TA('a', function () { return Array.fromAsync.call(null, [1,2]); }); tick(8, 't');`,
  `${prefix}TA('a', function () { return Array.fromAsync.call(undefined, [1,2]); }); tick(8, 't');`,
  `${prefix}TA('a', function () { return Array.fromAsync.call(5, [1,2]); }); tick(8, 't');`,
  `${prefix}function C() { L('C ' + arguments.length + ' ' + S(arguments[0])); this.made = true; } TA('a', function () { return Array.fromAsync.call(C, [1,2]); }); tick(8, 't');`,
  `${prefix}function C() { L('C ' + arguments.length + ' ' + S(arguments[0])); } TA('a', function () { return Array.fromAsync.call(C, { length: 2, 0: 'x', 1: 'y' }); }); tick(8, 't');`,
  `${prefix}function C() { L('C'); return Object.freeze({}); } TA('a', function () { return Array.fromAsync.call(C, [1]); }); tick(8, 't');`,
  `${prefix}class A extends Array { constructor() { super(); L('A'); } } TA('a', function () { return Array.fromAsync.call(A, [1,2]).then(function (r) { return [r instanceof A, r.length, Array.from(r)]; }); }); tick(8, 't');`,
  `${prefix}var C = function () {}; C.prototype = null; TA('a', function () { return Array.fromAsync.call(C, [1]); }); tick(8, 't');`,
  `${prefix}var arr = [1,2,3]; var p = Array.fromAsync(arr); arr.push(4); TA('a', function () { return p; }); tick(8, 't');`,
  `${prefix}var arr = [1,2,3]; var p = Array.fromAsync(arr, function (v) { arr.length = 0; return v; }); TA('a', function () { return p; }); tick(8, 't');`,
  `${prefix}var arr = [Promise.resolve(1)]; var order = []; Array.fromAsync(arr).then(function () { L('fromAsync done'); }); Promise.resolve().then(function () { L('p1'); }).then(function () { L('p2'); }).then(function () { L('p3'); }).then(function () { L('p4'); }); tick(10, 't');`,
  `${prefix}Array.fromAsync([]).then(function () { L('empty done'); }); Promise.resolve().then(function () { L('p1'); }).then(function () { L('p2'); }).then(function () { L('p3'); }); tick(10, 't');`,
  `${prefix}Array.fromAsync([1,2,3]).then(function () { L('three done'); }); Promise.resolve().then(function () { L('p1'); }).then(function () { L('p2'); }).then(function () { L('p3'); }).then(function () { L('p4'); }).then(function () { L('p5'); }).then(function () { L('p6'); }); tick(14, 't');`,
  `${prefix}Array.fromAsync({ length: 2, 0: 1, 1: 2 }).then(function () { L('al done'); }); Promise.resolve().then(function () { L('p1'); }).then(function () { L('p2'); }).then(function () { L('p3'); }).then(function () { L('p4'); }).then(function () { L('p5'); }); tick(14, 't');`,
  `${prefix}Array.fromAsync((async function* () { yield 1; yield 2; })()).then(function () { L('ag done'); }); Promise.resolve().then(function () { L('p1'); }).then(function () { L('p2'); }).then(function () { L('p3'); }).then(function () { L('p4'); }).then(function () { L('p5'); }).then(function () { L('p6'); }).then(function () { L('p7'); }); tick(14, 't');`,
  `${prefix}var calls = 0; Array.fromAsync([1,2], function (v) { calls++; L('m' + v); return v; }); L('sync calls ' + calls); tick(8, 't');`,
  `${prefix}var calls = 0; Array.fromAsync(KI([1,2])); L('sync after call'); tick(8, 't');`,
  `${prefix}Array.fromAsync((function () { L('ARG'); return [1]; })()); L('after'); tick(4, 't');`,
  `${prefix}var it = KI([1,2,3]); TA('a', function () { return Array.fromAsync(it, function (v) { if (v === 2) throw new Error('mapstop'); return v; }); }); tick(10, 't');`,
  `${prefix}var it = KI([1,2,3]); TA('a', function () { return Array.fromAsync(it, async function (v) { if (v === 2) throw new Error('amapstop'); return v; }); }); tick(10, 't');`,
  `${prefix}var it = KI([Promise.reject(new Error('item')), 2]); TA('a', function () { return Array.fromAsync(it); }); tick(10, 't');`,
  `${prefix}var it = KI([1,2], { returnThrows: true }); TA('a', function () { return Array.fromAsync(it, function () { throw new Error('mapfail'); }); }); tick(10, 't');`,
  `${prefix}var it = KI([1,2], { returnPrim: true }); TA('a', function () { return Array.fromAsync(it, function () { throw new Error('mapfail'); }); }); tick(10, 't');`,
  `${prefix}var src = { [Symbol.asyncIterator]: function () { var i = 0; return { next: function () { L('n' + i); return Promise.resolve({ value: i, done: i++ >= 2 }); }, return: function () { L('ret'); return Promise.resolve({}); } }; } }; TA('a', function () { return Array.fromAsync(src, function (v) { if (v === 1) throw new Error('stop'); return v; }); }); tick(12, 't');`,
  `${prefix}var src = { [Symbol.asyncIterator]: function () { return { next: function () { L('n'); return Promise.reject(new Error('nrej')); }, return: function () { L('ret'); return {}; } }; } }; TA('a', function () { return Array.fromAsync(src); }); tick(12, 't');`,
  `${prefix}var src = { [Symbol.asyncIterator]: function () { throw new TypeError('gi'); } }; TA('a', function () { return Array.fromAsync(src); }); tick(8, 't');`,
  `${prefix}var src = { get [Symbol.asyncIterator]() { throw new TypeError('gg'); } }; TA('a', function () { return Array.fromAsync(src); }); tick(8, 't');`,
  `${prefix}var src = { get length() { throw new TypeError('lg'); } }; TA('a', function () { return Array.fromAsync(src); }); tick(8, 't');`,
  `${prefix}var src = { length: 2, get 0() { L('get0'); return 'a'; }, get 1() { L('get1'); return 'b'; } }; TA('a', function () { return Array.fromAsync(src); }); L('sync'); tick(8, 't');`,
  `${prefix}var P = Promise; var count = 0; Promise = function () { count++; return new P(function () {}); }; var r; try { r = Array.fromAsync([1]); } catch (e) { L(e.name); } Promise = P; L('count ' + count); tick(8, 't');`,
  `${prefix}Promise.prototype.then = function () { L('patched then'); }; Array.fromAsync([1]); tick(8, 't');`,
  `${prefix}var order = []; var p1 = Array.fromAsync([1,2]).then(function (r) { L('A ' + S(r)); }); var p2 = Array.fromAsync([3]).then(function (r) { L('B ' + S(r)); }); tick(14, 't');`,
  `${prefix}var order = []; Array.fromAsync([1,2,3], async function (v) { await tick(1, 'in' + v); return v; }).then(function (r) { L('done ' + S(r)); }); tick(20, 't');`
);

// ---- 5. Iterator helpers.
const mk = (arr, opts) => `KI(${arr}${opts ? ", " + opts : ""})`;
const helperOf = (src) => `Iterator.from(${src})`;
const basesList = {
  plain: "[1,2,3,4]",
  strs: "['a','b','c']",
  empty: "[]",
  dups: "[1,1,2,2]",
  nested: "[[1,2],[3],[]]",
};
const helperCalls = {
  map: "it.map(function (v, i) { L('map ' + S(v) + ' ' + i); return v * 10; })",
  mapThrow: "it.map(function (v) { throw new RangeError('m'); })",
  filter: "it.filter(function (v, i) { L('filter ' + S(v) + ' ' + i); return v % 2 === 1; })",
  filterThrow: "it.filter(function (v) { throw new RangeError('f'); })",
  take0: "it.take(0)",
  take2: "it.take(2)",
  take10: "it.take(10)",
  takeNeg: "it.take(-1)",
  takeNaN: "it.take(NaN)",
  takeInf: "it.take(Infinity)",
  takeFrac: "it.take(1.9)",
  takeStr: "it.take('2')",
  takeUndef: "it.take()",
  takeObj: "it.take({ valueOf: function () { L('valueOf'); return 1; } })",
  takeBig: "it.take(1n)",
  drop0: "it.drop(0)",
  drop2: "it.drop(2)",
  drop10: "it.drop(10)",
  dropNeg: "it.drop(-1)",
  dropNaN: "it.drop(NaN)",
  dropInf: "it.drop(Infinity)",
  dropUndef: "it.drop()",
  dropStr: "it.drop('1')",
  flatMapArr: "it.flatMap(function (v) { L('flat ' + S(v)); return [v, v]; })",
  flatMapIt: "it.flatMap(function (v) { return KI([v, v + 100]); })",
  flatMapStr: "it.flatMap(function (v) { return 'ab'; })",
  flatMapNum: "it.flatMap(function (v) { return 5; })",
  flatMapObj: "it.flatMap(function (v) { return {}; })",
  flatMapNull: "it.flatMap(function (v) { return null; })",
  flatMapThrow: "it.flatMap(function (v) { throw new RangeError('fm'); })",
  flatMapInner: "it.flatMap(function (v) { return { [Symbol.iterator]: function () { L('inner'); return { next: function () { L('inext'); throw new EvalError('in'); } }; } }; })",
  flatMapIterable: "it.flatMap(function (v) { return new Set([v, v + 1]); })",
  mapChain: "it.map(function (v) { return v + 1; }).filter(function (v) { return v > 2; }).take(2)",
  dropTake: "it.drop(1).take(2)",
  takeDrop: "it.take(3).drop(1)",
  mapNotFn: "it.map(5)",
  mapNone: "it.map()",
  filterNotFn: "it.filter({})",
  flatMapNotFn: "it.flatMap('x')",
};
const consumers = {
  toArray: "S(h.toArray())",
  spread: "S(Array.from(h))",
  nextTwice: "S([h.next(), h.next()])",
  nextAll: "var rs = []; for (var q = 0; q < 8; q++) rs.push(h.next()); S(rs)",
  retFirst: "S([h.return(), h.next()])",
  nextRet: "S([h.next(), h.return('x'), h.next()])",
  retTwice: "S([h.next(), h.return(), h.return()])",
  forOfBreak: "var seen = []; for (var x of h) { seen.push(x); if (seen.length === 1) break; } S(seen)",
  forEach: "var seen = []; h.forEach(function (v, i) { seen.push([v, i]); }); S(seen)",
  count: "var c = 0; for (var x of h) c++; S(c)",
};
const sources = {
  array: (a) => `${a}[Symbol.iterator]()`,
  logged: (a) => mk(a),
  loggedNoRet: (a) => mk(a, "{ noReturn: true }"),
  loggedThrow1: (a) => mk(a, "{ throwAt: 1 }"),
  loggedRetThrows: (a) => mk(a, "{ returnThrows: true }"),
};
for (const [bn, base] of Object.entries(basesList)) {
  for (const [sn, srcF] of Object.entries(sources)) {
    for (const [hn, call] of Object.entries(helperCalls)) {
      if (sn !== "logged" && !["map", "filter", "take2", "flatMapArr", "mapThrow", "drop2", "flatMapIt"].includes(hn)) continue;
      if (bn !== "plain" && sn !== "logged") continue;
      if (bn !== "plain" && bn !== "empty" && !/^(map|filter|take2|drop2|flatMapArr|flatMapIt|flatMapStr|mapThrow)$/.test(hn)) continue;
      const cs = ["toArray", "nextRet", "forOfBreak", "retFirst"];
      for (const cn of cs) {
        add(`var it = Iterator.from(${srcF(base)}); L(T(function () { var h = ${call}; return ${consumers[cn]}; }));`);
      }
    }
  }
}
// Consumidores terminais.
const terminals = {
  reduceNoInit: "it.reduce(function (a, v, i) { L('red ' + a + ' ' + v + ' ' + i); return a + v; })",
  reduceInit: "it.reduce(function (a, v) { return a + v; }, 100)",
  reduceThrow: "it.reduce(function (a, v) { throw new RangeError('rd'); }, 0)",
  reduceNotFn: "it.reduce(5)",
  reduceUndefInit: "it.reduce(function (a, v) { return [a, v]; }, undefined)",
  some: "it.some(function (v, i) { L('some ' + v + ' ' + i); return v === 2; })",
  someNever: "it.some(function (v) { return false; })",
  someThrow: "it.some(function (v) { throw new RangeError('sm'); })",
  someNotFn: "it.some()",
  every: "it.every(function (v, i) { L('every ' + v + ' ' + i); return v < 3; })",
  everyAll: "it.every(function (v) { return true; })",
  everyThrow: "it.every(function (v) { throw new RangeError('ev'); })",
  everyNotFn: "it.every(null)",
  find: "it.find(function (v, i) { L('find ' + v + ' ' + i); return v === 3; })",
  findNone: "it.find(function (v) { return false; })",
  findThrow: "it.find(function (v) { throw new RangeError('fd'); })",
  findNotFn: "it.find('s')",
  forEach: "it.forEach(function (v, i) { L('fe ' + v + ' ' + i); })",
  forEachThrow: "it.forEach(function (v) { throw new RangeError('fe'); })",
  forEachNotFn: "it.forEach({})",
  toArray: "it.toArray()",
};
for (const [bn, base] of Object.entries({ plain: "[1,2,3,4]", empty: "[]", one: "[7]" })) {
  for (const [sn, srcF] of Object.entries(sources)) {
    for (const [tn, call] of Object.entries(terminals)) {
      add(`var it = Iterator.from(${srcF(base)}); L(T(function () { return ${call}; }));`);
    }
  }
}
// Iterator.from, Iterator.concat, protótipo e construtor.
const fromArgs = {
  arr: "[1,2]", str: "'ab'", set: "new Set([1])", map: "new Map([[1,2]])", gen: "(function* () { yield 1; })()", nul: "null", undef: "undefined", num: "5",
  bool: "true", sym: "Symbol()", plain: "{}", withNext: "{ next: function () { return { done: true }; } }",
  withNextLog: "{ next: function () { L('n'); return { done: true, value: 5 }; } }",
  iterObj: "{ [Symbol.iterator]: function () { L('@@it'); return { next: function () { L('n'); return { done: true }; } }; } }",
  iterNotFn: "{ [Symbol.iterator]: 5 }", iterNull: "{ [Symbol.iterator]: null, next: function () { return { done: true }; } }",
  iterReturnsPrim: "{ [Symbol.iterator]: function () { return 1; } }", iterReturnsUndef: "{ [Symbol.iterator]: function () { } }",
  strObj: "new String('xy')", arrayIterator: "[1,2][Symbol.iterator]()", helper: "[1,2][Symbol.iterator]().map(function (x) { return x; })",
  iterSub: "(function () { class MyIt extends Iterator { next() { return { done: true }; } } return new MyIt(); })()",
  getterNext: "{ get next() { L('get next'); return function () { return { done: true }; }; } }",
};
for (const [an, a] of Object.entries(fromArgs)) {
  add(
    `L(T(function () { var r = Iterator.from(${a}); return [typeof r, r instanceof Iterator, Object.prototype.toString.call(r), r === ${a}, S(r.next ? r.next() : 'nonext')]; }));`,
    `L(T(function () { var r = Iterator.from(${a}); return S(Array.from(r)); }));`,
    `L(T(function () { var r = Iterator.from(${a}); return [typeof r.return, S(r.return && r.return())]; }));`,
    `L(T(function () { return S(Array.from(Iterator.concat(${a}))); }));`,
    `L(T(function () { return S(Array.from(Iterator.concat([0], ${a}, [9]))); }));`
  );
}
add(
  `L(S([typeof Iterator, Iterator.name, Iterator.length, typeof Iterator.from, Iterator.from.length, typeof Iterator.concat, typeof Iterator.prototype.map]));`,
  `L(T(function () { return new Iterator(); }));`,
  `L(T(function () { return Iterator(); }));`,
  `L(T(function () { class Sub extends Iterator { } return Object.prototype.toString.call(new Sub()); }));`,
  `L(T(function () { return Reflect.construct(Iterator, [], Object); }));`,
  `L(T(function () { var d = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, Iterator.prototype[Symbol.toStringTag]]; }));`,
  `L(T(function () { var o = Object.create(Iterator.prototype); o[Symbol.toStringTag] = 'X'; return [Object.prototype.toString.call(o), Object.getOwnPropertyNames(o)]; }));`,
  `L(T(function () { var d = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable]; }));`,
  `L(T(function () { Iterator.prototype.constructor = 5; return Iterator.prototype.constructor; }));`,
  `L(T(function () { Iterator.prototype[Symbol.toStringTag] = 'Z'; return Iterator.prototype[Symbol.toStringTag]; }));`,
  `L(T(function () { return Object.getOwnPropertyNames(Iterator.prototype).sort(); }));`,
  `L(T(function () { var proto = Object.getPrototypeOf(Iterator.from({ next: function () { return { done: true }; } })); return [proto === Iterator.prototype, Object.getPrototypeOf(proto) === Object.prototype, Object.getOwnPropertyNames(proto)]; }));`,
  `L(T(function () { var h = [1].values().map(function (x) { return x; }); var p = Object.getPrototypeOf(h); return [Object.getOwnPropertyNames(p).sort(), Object.prototype.toString.call(h), Object.getPrototypeOf(p) === Iterator.prototype]; }));`,
  `L(T(function () { var h = [1].values().map(function (x) { return x; }); return Object.getPrototypeOf(h).next.call({}); }));`,
  `L(T(function () { var h = [1].values().map(function (x) { return x; }); return Object.getPrototypeOf(h).return.call([].values()); }));`,
  `L(T(function () { var w = Object.getPrototypeOf(Iterator.from({ next: function () { return { done: true }; } })); return [Object.getOwnPropertyNames(w).sort(), typeof w.next, typeof w.return]; }));`,
  `L(T(function () { return Iterator.prototype.map.call({ next: function () { return { done: true }; } }, function (x) { return x; }).toArray(); }));`,
  `L(T(function () { return Iterator.prototype.map.call({}, function (x) { return x; }).next(); }));`,
  `L(T(function () { return Iterator.prototype.map.call(5, function (x) { return x; }); }));`,
  `L(T(function () { return Iterator.prototype.toArray.call({ next: 5 }); }));`,
  `L(T(function () { return Iterator.prototype.take.call({ get next() { L('gn'); return function () { return { done: true }; }; } }, 1).toArray(); }));`,
  `L(T(function () { var it = KI([1,2]); it.next = null; return Iterator.prototype.toArray.call(it); }));`,
  `L(T(function () { var it = KI([1,2,3]); var h = Iterator.prototype.take.call(it, 1); var r = h.toArray(); return [r, 'ok']; }));`,
  `L(T(function () { return Iterator.prototype.take.call(KI([1]), -1); })); L('end');`,
  `L(T(function () { return Iterator.prototype.drop.call(KI([1]), NaN); })); L('end');`,
  `L(T(function () { return Iterator.prototype.map.call(KI([1]), 5); })); L('end');`,
  `L(T(function () { return Iterator.prototype.reduce.call(KI([]), function () { }); })); L('end');`,
  `L(T(function () { return Iterator.prototype.reduce.call(KI([5]), function () { L('cb'); }); })); L('end');`,
  `L(T(function () { var h = KI([1,2,3]); var m = Iterator.prototype.map.call(h, function (v) { return v; }); return [m.next(), m.return(), m.next()]; })); L('end');`,
  `L(T(function () { var h = KI([1,2,3]); var m = Iterator.prototype.map.call(h, function (v) { return v; }); return [m.return(), m.next()]; })); L('end');`,
  `L(T(function () { var h = KI([1,2,3]); var m = Iterator.prototype.take.call(h, 2); return [m.next(), m.next(), m.next(), m.next()]; })); L('end');`,
  `L(T(function () { var h = KI([1,2,3]); var m = Iterator.prototype.take.call(h, 0); return [m.next(), m.next()]; })); L('end');`,
  `L(T(function () { var h = KI([1,2,3]); var m = Iterator.prototype.take.call(h, 0); return [m.return(), m.next()]; })); L('end');`,
  `L(T(function () { var m = KI([1,2,3]); var h = Iterator.prototype.drop.call(m, 1); return [h.next(), h.return(), h.next()]; })); L('end');`,
  `L(T(function () { var inner = KI([10,20]); var outer = KI([1,2]); var h = Iterator.prototype.flatMap.call(outer, function () { return inner; }); return [h.next(), h.return(), h.next()]; })); L('end');`,
  `L(T(function () { var outer = KI([1,2]); var h = Iterator.prototype.flatMap.call(outer, function () { return KI([10,20], { throwAt: 1 }); }); return [h.next(), h.next()]; })); L('end');`,
  `L(T(function () { var outer = KI([1,2]); var h = Iterator.prototype.flatMap.call(outer, function () { return KI([10,20]); }); h.next(); try { h.throw(new Error('x')); } catch (e) { L(e.name); } return 'ok'; })); L('end');`,
  `L(T(function () { var it = KI([1,2,3]); var a = Iterator.prototype.map.call(it, function (v) { L('A' + v); return v; }); var b = Iterator.prototype.filter.call(a, function (v) { L('B' + v); return v > 1; }); return [b.next(), b.return(), b.next()]; })); L('end');`,
  `L(T(function () { var it = KI([1,2,3]); var h = Iterator.prototype.map.call(it, function (v) { return h.next(); }); return h.next(); })); L('end');`,
  `L(T(function () { var h = Iterator.prototype.map.call(KI([1,2]), function (v) { try { h.return(); } catch (e) { L('inner ' + e.name); } return v; }); return [h.next(), h.next()]; })); L('end');`,
  `L(T(function () { var gen = (function* () { try { yield 1; yield 2; } finally { L('gen fin'); } })(); var h = gen.map(function (v) { return v + 1; }); return [h.next(), h.return(), gen.next()]; }));`,
  `L(T(function () { var gen = (function* () { try { yield 1; yield 2; } finally { L('gen fin'); } })(); return S(gen.take(1).toArray()); }));`,
  `L(T(function () { var gen = (function* () { try { yield 1; yield 2; } finally { L('gen fin'); } })(); return gen.some(function (v) { return true; }); }));`,
  `L(T(function () { var gen = (function* () { try { yield 1; yield 2; } finally { throw new Error('fin'); } })(); return gen.find(function (v) { return true; }); }));`,
  `L(T(function () { var gen = (function* () { try { yield 1; yield 2; } finally { L('f'); } })(); return gen.every(function (v) { throw new RangeError('cb'); }); }));`,
  `L(T(function () { var gen = (function* () { yield 1; })(); return [gen.map(function (x) { return x; }).toString(), String(gen.take(1)[Symbol.toStringTag])]; }));`,
  `L(T(function () { return Iterator.concat([1], [2], 'ab', new Set([3])).toArray(); }));`,
  `L(T(function () { return Iterator.concat().toArray(); }));`,
  `L(T(function () { return Iterator.concat(5); }));`,
  `L(T(function () { return Iterator.concat({ [Symbol.iterator]: 5 }); }));`,
  `L(T(function () { var a = KI([1,2]); var b = KI([3]); var c = Iterator.concat(a, b); return [c.next(), c.next(), c.next(), c.next()]; })); L('end');`,
  `L(T(function () { var a = KI([1,2]); var c = Iterator.concat(a); return [c.next(), c.return(), c.next()]; })); L('end');`,
  `L(T(function () { var a = KI([1,2]); var c = Iterator.concat(a); return [c.return(), c.next()]; })); L('end');`,
  `L(T(function () { var c = Iterator.concat({ [Symbol.iterator]: function () { L('open1'); return KI([1]); } }, { [Symbol.iterator]: function () { L('open2'); return KI([2]); } }); L('made'); return c.toArray(); })); L('end');`,
  `L(T(function () { var c = Iterator.concat([1], { [Symbol.iterator]: function () { throw new EvalError('open'); } }); return c.toArray(); })); L('end');`,
  `L(T(function () { var c = Iterator.concat(KI([1], { throwAt: 0 })); return c.next(); })); L('end');`,
  `L(T(function () { var c = Iterator.concat([1]); var d = Object.getPrototypeOf(c); return [Object.prototype.toString.call(c), Object.getPrototypeOf(d) === Iterator.prototype]; }));`
);

// ---- 6. Iterator helpers dentro de laços assíncronos.
const asyncHelperBodies = {
  forAwaitSync: "(async function () { for await (var x of [1,2,3].values().map(function (v) { return Promise.resolve(v * 2); })) L('x ' + S(x)); L('end'); })()",
  forAwaitFilter: "(async function () { for await (var x of [1,2,3,4].values().filter(function (v) { return v % 2; })) L('x ' + S(x)); L('end'); })()",
  forAwaitBreak: "(async function () { for await (var x of KI([1,2,3]).take(5)) { L('x ' + S(x)); break; } L('end'); })()",
  awaitInCb: "(async function () { var r = []; for (var x of [1,2,3].values().map(function (v) { return v; })) { await null; r.push(x); } L(S(r)); })()",
  awaitToArray: "(async function () { L(S(await Promise.all([1,2,3].values().map(function (v) { return Promise.resolve(v); }).toArray()))); })()",
  mapAsyncCb: "(async function () { L(S(await Promise.all([1,2].values().map(async function (v) { await null; return v; })))); })()",
  mapReturnsPromise: "L(S([1,2].values().map(function (v) { return Promise.resolve(v); }).toArray().map(function (p) { return p instanceof Promise; })))",
  genYieldStar: "(function* () { yield* [1,2,3].values().map(function (v) { return v + 1; }); })().toArray().forEach(function (v) { L(v); })",
  asyncGenSpread: "(async function () { var out = []; for await (var x of (async function* () { yield* [1,2].values().map(function (v) { return v * 3; }); })()) out.push(x); L(S(out)); })()",
  somePromise: "L(S([1,2].values().some(function (v) { return Promise.resolve(false); })))",
  everyPromise: "L(S([1,2].values().every(function (v) { return Promise.resolve(false); })))",
  findPromise: "L(S([1,2].values().find(function (v) { return Promise.resolve(false); })))",
  reducePromise: "L(S(typeof [1,2].values().reduce(function (a, v) { return Promise.resolve(v); }, 0).then))",
  flatMapPromise: "L(T(function () { return [1].values().flatMap(function (v) { return Promise.resolve([v]); }).toArray(); }))",
  mapThenOrder: "var order = []; var h = [1,2,3].values().map(function (v) { L('cb' + v); return v; }); Promise.resolve().then(function () { L('micro'); }); L('before'); h.next(); L('after1'); h.next(); L('after2')",
  takeAsyncGen: "L(T(function () { return (async function* () { yield 1; })().take(1); }))",
  asyncGenHasNoHelpers: "L(S([typeof (async function* () { })().map, typeof (async function* () { })().take, typeof Iterator.prototype.toAsync]))",
  fromAsyncOfHelper: "TA('a', function () { return Array.fromAsync([1,2,3].values().map(function (v) { return v * 2; })); })",
  fromAsyncOfHelperAsyncMap: "TA('a', function () { return Array.fromAsync([1,2,3].values().filter(function (v) { return v > 1; }), async function (v) { return v + 1; }); })",
  fromAsyncOfConcat: "TA('a', function () { return Array.fromAsync(Iterator.concat([1], [Promise.resolve(2)])); })",
  fromAsyncOfFrom: "TA('a', function () { return Array.fromAsync(Iterator.from({ next: (function () { var i = 0; return function () { return i < 2 ? { value: Promise.resolve(i++), done: false } : { done: true }; }; })() })); })",
};
for (const [n, b] of Object.entries(asyncHelperBodies)) {
  add(`${prefix}${b}; tick(14, 't14');`);
  add(`${prefix}${b}; tick(3, 't3');`);
}

// ---- 7. Set-like contra Map/Set em laços com await e tipos de this.
const lateBodies = {
  unionAwait: "(async function () { var s = new Set([1,2]); var p = s.union(new Set([3])); await null; s.add(9); L(S(p)); L(S(s)); })()",
  intersectionAwait: "(async function () { var s = new Set([1,2,3]); var like = { size: 2, has: function (x) { return x === 2; }, keys: function () { return [2, 3][Symbol.iterator](); } }; var r = s.intersection(like); await null; s.delete(2); L(S(r)); })()",
  differenceAwait: "(async function () { var s = new Set([1,2,3]); var r = s.difference(new Map([[1, 1]])); await tick(1, 'a'); L(S(r)); })()",
  symAwait: "(async function () { var a = new Set([1,2]); var b = new Set([2,3]); var r = a.symmetricDifference(b); a.clear(); await null; L(S(r)); L(S(b)); })()",
  subsetAwait: "(async function () { var a = new Set([1]); L(S(a.isSubsetOf(new Set([1,2])))); await null; L(S(a.isSupersetOf(new Set()))); L(S(a.isDisjointFrom(new Set([5])))); })()",
  likeIterAwait: "(async function () { var a = new Set([1,2,3]); var like = { size: 3, has: function () { return true; }, keys: function () { return (function* () { L('k1'); yield 1; L('k2'); yield 2; })(); } }; L(S(a.isSubsetOf(like))); L(S(a.isSupersetOf(like))); L(S(a.union(like))); })()",
  mapKeysLike: "(async function () { var a = new Set([1,2]); L(S(a.union(new Map([[3, 'x']])))); L(S(a.intersection(new Map([[2, 'x'], [5, 'y']])))); })()",
  forAwaitSetOps: "(async function () { for await (var x of new Set([1,2]).union(new Set([2,3]))) L('x ' + x); L('end'); })()",
  thenableSet: "(async function () { var s = new Set([1]); var r = await s.union(new Set([2])); L(S(r)); })()",
  awaitMapGroup: "(async function () { var m = Map.groupBy([1,2,3], function (v) { return v % 2; }); await null; L(S(m)); for await (var [k, v] of m) L(S([k, v])); })()",
  awaitObjGroup: "(async function () { var o = Object.groupBy([1,2,3], function (v) { return v % 2 ? 'odd' : 'even'; }); L(S(o)); L(S(await Promise.all(Object.values(o)))); })()",
  groupByPromiseKeys: "(async function () { var m = Map.groupBy([1,2], function (v) { return Promise.resolve(v); }); L(S(Array.from(m.keys()).map(function (k) { return k instanceof Promise; }))); })()",
  getOrInsertAwait: "(async function () { var m = new Map(); L(S(m.getOrInsert(1, 'a'))); await null; L(S(m.getOrInsert(1, 'b'))); L(S(m.getOrInsertComputed(2, function (k) { L('cb ' + k); return 'c'; }))); L(S(m.getOrInsertComputed(2, function () { L('no'); return 'd'; }))); L(S(m)); })()",
  getOrInsertRe: "var m = new Map(); L(T(function () { return m.getOrInsertComputed(1, function () { m.set(1, 'inner'); return 'outer'; }); })); L(S(m));",
  getOrInsertThrow: "var m = new Map(); L(T(function () { return m.getOrInsertComputed(1, function () { throw new RangeError('x'); }); })); L(S(m));",
  getOrInsertWeak: "var w = new WeakMap(); var k = {}; L(T(function () { return [w.getOrInsert(k, 1), w.getOrInsert(k, 2), w.getOrInsertComputed({}, function () { return 3; })]; }));",
};
for (const [n, b] of Object.entries(lateBodies)) {
  add(`${prefix}${b}; tick(10, 't10');`);
}

// Amostra completa: todos os programas entram.
const selected = programs;

// Executa cada programa no bun, num processo próprio, pela API vm (a fonte nunca é um arquivo do projeto).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "collection-async-golden-"));
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
