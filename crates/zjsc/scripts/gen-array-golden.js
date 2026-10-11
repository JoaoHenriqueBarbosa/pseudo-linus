// Gera tests/golden/array_bun.tsv: métodos de Array.prototype, Array.from/of/fromAsync, iteradores, arrays esparsos,
// array-likes de length gigante, subclasses com species, proxies, getters que mutam durante a iteração, TypedArray
// versus Array e mensagens de erro, medidos no bun 1.4.2 (JavaScriptCore).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Cada programa leva o mesmo prelúdio (`S` serializa valor com buraco, -0, bigint, símbolo; `T` captura exceção).
// Programa que estoura o tempo (length gigante em laço denso) é descartado, não entra no golden.
// Uso: bun scripts/gen-array-golden.js > tests/golden/array_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { emitFactoredLines, prepareProgram, sampleByHash } = require("./golden-prelude.js");
const { spawnSync } = require("child_process");

const PRELUDE = [
  "function S(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'function') return 'fn';",
  "  if (v === null || typeof v !== 'object') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (d > 3) return '...';",
  "  if (Array.isArray(v)) {",
  "    var o = [], n = Math.min(v.length, 40);",
  "    for (var i = 0; i < n; i++) o.push(i in v ? S(v[i], d + 1) : '<hole>');",
  "    return '[' + o.join(',') + ']#' + v.length + (Object.getPrototypeOf(v) === Array.prototype ? '' : '~sub');",
  "  }",
  "  if (ArrayBuffer.isView(v)) return Object.prototype.toString.call(v) + '[' + Array.prototype.join.call(v, ',') + ']';",
  "  return '{' + Object.keys(v).slice(0, 20).map(function (k) { return k + ':' + S(v[k], d + 1); }).join(',') + '}';",
  "}",
  "function T(f) { try { return S(f()); } catch (e) { return 'throw ' + e.name + ': ' + e.message; } }",
  "function D(it) { var o = []; for (var x of it) { o.push(S(x)); if (o.length > 40) break; } return o.join(';'); }",
].join("\n");

const programs = [];
// Expressão avaliada com captura de exceção.
// A parte combinatorial (antes de "Erros com mensagem exata") é amostrada: um programa em cada seis, escolhido por hash do
// texto do programa (`sampleByHash`) depois de coletar todos os candidatos, nunca por contador.
let thin = true;
const thinned = [];
const E = expr => {
  (thin ? thinned : programs).push(`R = T(function () { return (${expr}); });`);
};
// Corpo livre: o texto de `R` é definido por ele.
const B = (...rows) => programs.push(rows.join("\n"));
// Corpo que devolve valor via `function () { ... }`.
const F = (...rows) => programs.push(`R = T(function () {\n${rows.join("\n")}\n});`);

const arrays = [
  "[]", "[1,2,3]", "[1,,3]", "[,,]", "[undefined,1,,null]", "[3,1,2]", "[1,[2,[3,[4]]]]", "[NaN,0,-0,1]",
  "['b','a','c']", "[1,2,3,4,5]", "[,1,,2,,]", "[1,2,3,2,1]", "[[1],[2,[3]],,[]]", "[true,'1',1,1n,null,undefined]",
];
const idx = ["0", "1", "-1", "-4", "5", "NaN", "'1'", "1.9", "-0.5", "Infinity", "-Infinity", "undefined", "null", "2**32", "-(2**53)"];
const small = ["0", "1", "2", "-1", "-2", "10", "undefined", "NaN", "Infinity", "-Infinity", "1.5", "'2'"];

// ---- at, with: índice.
for (const a of arrays) for (const i of idx) { E(`${a}.at(${i})`); E(`${a}.with(${i}, 'x')`); }
// ---- indexOf, lastIndexOf, includes: busca com fromIndex.
for (const a of arrays) {
  for (const v of ["1", "undefined", "NaN", "0", "-0", "null", "'1'", "2"]) {
    E(`${a}.indexOf(${v})`); E(`${a}.lastIndexOf(${v})`); E(`${a}.includes(${v})`);
  }
  for (const i of small) { E(`${a}.indexOf(1, ${i})`); E(`${a}.lastIndexOf(1, ${i})`); E(`${a}.includes(undefined, ${i})`); }
}
// ---- slice, splice, copyWithin, fill, toSpliced.
for (const a of arrays.slice(0, 9)) {
  for (const s of small) for (const e of ["undefined", "0", "2", "-1", "10", "NaN"]) {
    E(`${a}.slice(${s}, ${e})`); E(`${a}.fill(9, ${s}, ${e})`); E(`${a}.copyWithin(0, ${s}, ${e})`); E(`${a}.copyWithin(1, ${s}, ${e})`);
  }
  for (const s of small) for (const c of ["undefined", "0", "1", "2", "-1", "10"]) {
    E(`${a}.splice(${s}, ${c})`); E(`${a}.toSpliced(${s}, ${c}, 'a', 'b')`);
  }
  E(`${a}.splice()`); E(`${a}.splice(1)`); E(`${a}.toSpliced()`); E(`${a}.toSpliced(1)`); E(`${a}.splice(undefined)`);
  E(`${a}.splice(0, 0, 1, 2, 3)`); E(`${a}.splice(-1, 1, 'z')`);
  F(`var a = ${a}; var r = a.splice(1, 1, 'p', 'q'); return [a, r];`);
}
// ---- flat, flatMap, concat, join, reverse, sort e cópias.
for (const a of arrays) {
  for (const d of ["undefined", "0", "1", "2", "Infinity", "-1", "'1'", "NaN", "1.5"]) E(`${a}.flat(${d})`);
  E(`${a}.flatMap(function (x) { return [x, x]; })`); E(`${a}.flatMap(function (x) { return x; })`);
  E(`${a}.flatMap(function (x, i) { return i % 2 ? [] : [[x]]; })`);
  E(`${a}.join()`); E(`${a}.join('-')`); E(`${a}.join(undefined)`); E(`${a}.join(null)`); E(`${a}.join('')`); E(`${a}.join(1)`);
  E(`${a}.reverse()`); E(`${a}.toReversed()`); E(`${a}.sort()`); E(`${a}.toSorted()`);
  E(`${a}.sort(function (x, y) { return x < y ? -1 : x > y ? 1 : 0; })`);
  E(`${a}.toSorted(function (x, y) { return y - x; })`);
  E(`${a}.concat()`); E(`${a}.concat([9])`); E(`${a}.concat(1, [2], [[3]])`); E(`${a}.concat(${a})`);
  E(`${a}.toString()`); E(`${a}.toLocaleString()`); E(`String(${a})`);
  E(`${a}.findLast(function (x) { return x > 1; })`); E(`${a}.findLastIndex(function (x) { return x === undefined; })`);
  E(`${a}.find(function (x) { return x === undefined; })`); E(`${a}.findIndex(function (x) { return x == null; })`);
  E(`D(${a}.entries())`); E(`D(${a}.keys())`); E(`D(${a}.values())`); E(`D(${a})`);
  E(`${a}.map(function (x, i) { return i; })`); E(`${a}.filter(function () { return true; })`);
  E(`${a}.every(function (x) { return x !== 0; })`); E(`${a}.some(function (x) { return x === undefined; })`);
  E(`${a}.reduce(function (p, x) { return p + '|' + x; })`); E(`${a}.reduceRight(function (p, x) { return p + '|' + x; }, '')`);
  E(`${a}.reduce(function (p, x) { return p + 1; }, 0)`);
  E(`(function () { var r = []; ${a}.forEach(function (x, i) { r.push(i); }); return r; })()`);
  E(`${a}.push(7, 8)`); E(`${a}.unshift(7, 8)`); E(`${a}.pop()`); E(`${a}.shift()`);
  E(`Array.from(${a})`); E(`Array.from(${a}, function (x, i) { return [x, i]; })`); E(`Array.of(...${a})`);
}
thin = false;
programs.push(...sampleByHash(thinned, Math.ceil(thinned.length / 6)));
// ---- Erros com mensagem exata.
for (const e of [
  "[].with(0, 1)", "[1].with(1, 1)", "[1].with(-2, 1)", "[1,2].with(2**32, 1)", "[1].toSpliced(0, 0, ...new Array(10))",
  "new Array(-1)", "new Array(1.5)", "new Array(2**32)", "Array(2**32 - 1).length", "Array(NaN)", "new Array('3')", "new Array(3)",
  "Array.from({ length: -1 })", "Array.from({ length: 2**32 })", "Array.from({ length: 2**53 })",
  "Array.of.call(Object, 1, 2)", "Array.from.call(Object, [1, 2])", "Array.from.call(function () { return Object.freeze({}); }, [1])",
  "[].reduce(function () {})", "[].reduceRight(function () {})", "[,,].reduce(function () {})",
  "[].map()", "[].map(1)", "[].map(null)", "[].forEach({})", "[].filter('x')", "[].find()", "[].findLast(1)", "[].findLastIndex({})",
  "[].every()", "[].some(undefined)", "[].flatMap()", "[].flatMap(1)", "[].sort(1)", "[].sort(null)", "[].toSorted(1)", "[].toSorted(null)",
  "[1,2].sort({})", "Array.from([], 1)", "Array.from([], null)", "Array.from(null)", "Array.from(undefined)", "Array.from()",
  "Array.prototype.at.call(null)", "Array.prototype.at.call(undefined)", "Array.prototype.map.call(null, function () {})",
  "Array.prototype.join.call(undefined)", "Array.prototype.push.call(null)", "Array.prototype.with.call(null, 0, 1)",
  "Array.prototype.toSorted.call(null)", "Array.prototype.includes.call(undefined)", "Array.prototype.flat.call(null)",
  "Array.prototype.concat.call(null)", "Array.prototype.fill.call(null)", "Array.prototype.copyWithin.call(null, 0)",
  "Array.prototype.splice.call(null)", "Array.prototype.toSpliced.call(null)", "Array.prototype.slice.call(null)",
  "Array.prototype.reverse.call(null)", "Array.prototype.toReversed.call(null)", "Array.prototype.indexOf.call(null)",
  "Array.prototype.lastIndexOf.call(null)", "Array.prototype.entries.call(null)", "Array.prototype.keys.call(undefined)",
  "Array.prototype.values.call(null)", "Array.prototype.findLast.call(null, function () {})", "Array.prototype.reduce.call(null, function () {})",
  "Array.fromAsync.call(null, [])", "Array.isArray()", "Array.isArray([])", "Array.prototype.length", "Array.prototype.constructor === Array",
  "Array.length", "Array.name", "Array.of.length", "Array.from.length", "Array.fromAsync.length", "Array.prototype.concat.length",
  "Array.prototype.splice.length", "Array.prototype.toSpliced.length", "Array.prototype.with.length", "Array.prototype.fill.length",
  "Array.prototype.copyWithin.length", "Array.prototype.indexOf.length", "Array.prototype.push.length", "Array.prototype.slice.length",
  "Array.prototype.sort.length", "Array.prototype.flat.length", "Array.prototype.reduce.length", "Array.prototype.at.length",
  "Array.prototype.join.length", "Array.prototype.includes.length", "Array.prototype.findLast.length", "Array.prototype.entries.name",
  "Array.prototype[Symbol.iterator] === Array.prototype.values", "Array.prototype.values.name", "Array.prototype[Symbol.iterator].name",
  "Object.keys(Array.prototype[Symbol.unscopables])", "Object.getPrototypeOf(Array.prototype[Symbol.unscopables])",
  "Object.getOwnPropertyNames(Array.prototype).sort()", "Object.getOwnPropertyNames(Array).sort()",
  "Object.getOwnPropertySymbols(Array.prototype)", "Object.getOwnPropertySymbols(Array)", "Array[Symbol.species] === Array",
  "Object.getOwnPropertyDescriptor(Array, Symbol.species).get.name", "[].length = 2**32", "(function () { 'use strict'; var a = []; a.length = -1; })()",
  "(function () { 'use strict'; var a = []; a.length = 1.5; })()", "(function () { 'use strict'; var a = []; a.length = 'x'; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.length; })()",
  "(function () { var a = [1,2,3]; a.length = '2'; return a; })()", "(function () { var a = [1,2,3]; a[5] = 1; return a; })()",
  "(function () { var a = []; a[2**32 - 2] = 1; return a.length; })()", "(function () { var a = []; a[2**32 - 1] = 1; return [a.length, Object.keys(a)]; })()",
  "(function () { var a = [1]; Object.defineProperty(a, 'length', { writable: false }); a.push(2); })()",
  "(function () { 'use strict'; var a = [1]; Object.freeze(a); a.push(2); })()", "(function () { 'use strict'; var a = [1]; Object.freeze(a); a.pop(); })()",
  "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.sort(); })()", "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.reverse(); })()",
  "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.fill(0); })()", "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.shift(); })()",
  "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.unshift(0); })()", "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.splice(0, 1); })()",
  "(function () { 'use strict'; var a = [1, 2]; Object.freeze(a); a.copyWithin(0, 1); })()", "(function () { var a = [1, 2]; Object.seal(a); a.push(3); })()",
  "(function () { var a = [1, 2]; Object.preventExtensions(a); a.push(3); })()", "(function () { var a = [1, 2]; Object.preventExtensions(a); return a.splice(0, 0, 9); })()",
  "(function () { var a = [1, 2]; Object.seal(a); return a.pop(); })()", "(function () { var a = [1, 2]; Object.seal(a); return a.shift(); })()",
]) E(e);

// ---- Array.from: iterável, array-like, mapFn, this.
for (const e of [
  "Array.from('abc')", "Array.from('a\\ud83d\\ude00b')", "Array.from(new Set([1, 2, 2]))", "Array.from(new Map([[1, 2]]))", "Array.from({ length: 3 })",
  "Array.from({ length: 3, 0: 'a', 2: 'c' })", "Array.from({ length: '2', 0: 1, 1: 2 })", "Array.from({ length: 2.7, 0: 1, 1: 2, 2: 3 })",
  "Array.from({ length: -5 })", "Array.from({ length: NaN })", "Array.from({ length: Infinity })", "Array.from({})", "Array.from(5)", "Array.from(true)",
  "Array.from(Symbol())", "Array.from(function () {})", "Array.from(function (a, b) {})", "Array.from([1, 2, 3], function (x) { return x * 2; })",
  "Array.from([1, 2, 3], function (x, i) { return this.k + i; }, { k: 10 })", "Array.from([1, 2], function () { return this; }, 'p')",
  "Array.from([1, 2], function () { 'use strict'; return this; }, 'p')", "Array.from([1, 2], function () { 'use strict'; return typeof this; })",
  "Array.from({ length: 2 }, function (x, i) { return i; })", "Array.from({ length: 2, 0: 'a', 1: 'b' }, function (x, i) { return arguments.length; })",
  "Array.from(new Uint8Array([1, 2, 3]))", "Array.from(new Float64Array([1.5, -0]))", "Array.from(new BigInt64Array(2))",
  "Array.from(new Uint8Array(3), function (x, i) { return i; })", "Array.from(arguments_like())",
  "Array.from.call(undefined, [1])", "Array.from.call(null, [1])", "Array.from.call(1, [1])", "Array.from.call({}, [1])",
  "Array.from.call(Array, [1, 2])", "Array.from.call(function (n) { this.args = arguments.length; this.n = n; }, [1, 2])",
  "Array.from.call(function (n) { this.args = arguments.length; this.n = n; }, { length: 2, 0: 1, 1: 2 })",
  "(function () { var C = function () {}; var r = Array.from.call(C, [1, 2]); return [r instanceof C, r.length, Object.keys(r)]; })()",
  "(function () { var C = function () {}; var r = Array.from.call(C, { length: 2 }); return [r instanceof C, r.length, Object.keys(r)]; })()",
  "(function () { class C extends Array {}; var r = C.from([1, 2]); return [r instanceof C, r.length, S(r)]; })()",
  "(function () { class C extends Array {}; var r = C.from({ length: 2 }); return [r instanceof C, r.length]; })()",
  "(function () { class C extends Array {}; var r = C.of(1, 2, 3); return [r instanceof C, S(r)]; })()",
  "Array.of()", "Array.of(7)", "Array.of(1, 2, 3)", "Array.of(undefined)", "Array.of(,)", "Array.of.call(undefined, 1)", "Array.of.call(Object, 1, 2)",
  "Array.of.call(function (n) { this.n = n; }, 1, 2)", "Array.of.call(function () { return Object.freeze({}); }, 1)",
  "Array.of.call(function () { Object.defineProperty(this, 'length', { value: 0, writable: false }); }, 1)",
  "Array.from([1, 2], undefined)", "Array.from([1, 2], undefined, 1)",
  "(function () { var o = {}; o[Symbol.iterator] = function* () { yield 1; yield 2; }; o.length = 5; return Array.from(o); })()",
  "(function () { var o = { length: 2, 0: 'a', 1: 'b' }; o[Symbol.iterator] = undefined; return Array.from(o); })()",
  "(function () { var o = { length: 2, 0: 'a', 1: 'b' }; o[Symbol.iterator] = null; return Array.from(o); })()",
  "(function () { var o = { length: 2 }; o[Symbol.iterator] = 1; return Array.from(o); })()",
  "(function () { var o = {}; o[Symbol.iterator] = function () { return {}; }; return Array.from(o); })()",
  "(function () { var o = {}; o[Symbol.iterator] = function () { return { next: function () { return 1; } }; }; return Array.from(o); })()",
  "(function () { var log = []; var o = {}; o[Symbol.iterator] = function () { return { next: function () { return { done: false, value: 1 }; }, return: function () { log.push('ret'); return {}; } }; }; try { Array.from(o, function () { throw new Error('x'); }); } catch (e) {} return log; })()",
  "(function () { var log = []; var it = { next() { log.push('n'); return { done: log.length > 3, value: log.length }; }, [Symbol.iterator]() { return this; } }; return [Array.from(it), log]; })()",
]) E(e.replace("arguments_like()", "(function () { return arguments; })(1, 2)"));

// ---- concat com Symbol.isConcatSpreadable e species.
for (const e of [
  "[1].concat({ length: 2, 0: 'a', 1: 'b', [Symbol.isConcatSpreadable]: true })",
  "[1].concat({ length: 2, 0: 'a', 1: 'b' })",
  "(function () { var a = [2, 3]; a[Symbol.isConcatSpreadable] = false; return [1].concat(a).length; })()",
  "(function () { var a = [2, 3]; a[Symbol.isConcatSpreadable] = undefined; return [1].concat(a); })()",
  "(function () { var a = [2, 3]; a[Symbol.isConcatSpreadable] = 0; return [1].concat(a).length; })()",
  "(function () { var a = [2, 3]; a[Symbol.isConcatSpreadable] = null; return [1].concat(a).length; })()",
  "(function () { var o = { length: 3, [Symbol.isConcatSpreadable]: 1 }; return [0].concat(o); })()",
  "(function () { var o = { length: 2**53 - 1, [Symbol.isConcatSpreadable]: true }; return [0].concat(o); })()",
  "(function () { var o = { length: 2**32, [Symbol.isConcatSpreadable]: true }; return [].concat(o); })()",
  "(function () { var o = { length: 2**32 - 1, [Symbol.isConcatSpreadable]: true }; return [].concat(o).length; })()",
  "(function () { var o = { length: -1, [Symbol.isConcatSpreadable]: true }; return [0].concat(o); })()",
  "(function () { var o = { length: '2', 0: 1, 1: 2, [Symbol.isConcatSpreadable]: true }; return [0].concat(o); })()",
  "(function () { var o = { get length() { throw new Error('len'); }, [Symbol.isConcatSpreadable]: true }; return [0].concat(o); })()",
  "(function () { var o = { get [Symbol.isConcatSpreadable]() { throw new RangeError('sp'); } }; return [0].concat(o); })()",
  "(function () { var p = new Proxy([1, 2], {}); return [0].concat(p); })()",
  "(function () { var p = new Proxy({ length: 2, 0: 'a', 1: 'b' }, { get(t, k) { return k === Symbol.isConcatSpreadable ? true : t[k]; } }); return [0].concat(p); })()",
  "(function () { var log = []; var p = new Proxy([1, 2], { get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); [].concat(p); return log; })()",
  "(function () { return [1].concat('ab', 2, null, undefined, true, 1n).length; })()",
  "(function () { return [].concat([,], [,1]); })()",
  "(function () { var a = [1]; a.length = 3; return [].concat(a, a); })()",
  "(function () { var s = Object('ab'); s[Symbol.isConcatSpreadable] = true; return [].concat(s); })()",
  "(function () { var f = function () {}; f.length = 0; f[Symbol.isConcatSpreadable] = true; return [].concat(f); })()",
  "(function () { var C = function () {}; C[Symbol.species] = function (n) { return { n: n, length: 0 }; }; var a = [1, 2]; a.constructor = C; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = undefined; return a.concat([3]) instanceof Array; })()",
  "(function () { var a = [1, 2]; a.constructor = null; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = 1; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: null }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: undefined }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: 1 }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return {}; } }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: Object }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { [Symbol.species]: function Foo(n) { this.n = n; } }; var r = a.concat([3]); return [r.n, r instanceof a.constructor[Symbol.species], r.length]; })()",
  "(function () { var a = [1, 2]; a.constructor = Array; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = Object; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; a.constructor = { get [Symbol.species]() { throw new TypeError('sp'); } }; return a.concat([3]); })()",
  "(function () { var a = [1, 2]; Object.defineProperty(a, 'constructor', { get() { throw new TypeError('ctor'); } }); return a.concat([3]); })()",
]) E(e);

// ---- Subclasses com species em map, filter, slice, splice, flat, flatMap, toSorted e outros.
const subclass = "class C extends Array {}\n";
for (const m of [
  "map(function (x) { return x; })", "filter(function () { return true; })", "slice(1)", "splice(0, 1)", "concat([4])", "flat()", "flatMap(function (x) { return [x]; })",
  "toSorted()", "toReversed()", "with(0, 9)", "toSpliced(0, 1)", "reverse()", "sort()", "fill(0)", "copyWithin(0, 1)",
]) {
  F(subclass + `var c = C.from([3, 1, 2]); var r = c.${m}; return [r instanceof C, S(r), Object.getPrototypeOf(r) === C.prototype];`);
  F(subclass + `class D extends Array { static get [Symbol.species]() { return Array; } }\nvar c = D.from([3, 1, 2]); var r = c.${m}; return [r instanceof D, S(r)];`);
  F(`class D extends Array { static get [Symbol.species]() { return undefined; } }\nvar c = D.from([3, 1, 2]); var r = c.${m}; return [r instanceof D, S(r)];`);
  F(`class D extends Array { static get [Symbol.species]() { return null; } }\nvar c = D.from([3, 1, 2]); var r = c.${m}; return [r instanceof D, S(r)];`);
  F(`class D extends Array { static get [Symbol.species]() { return Object; } }\nvar c = D.from([3, 1, 2]); var r = c.${m}; return [r instanceof D, S(r)];`);
  F(`class D extends Array { static get [Symbol.species]() { return 1; } }\nvar c = D.from([3, 1, 2]); var r = c.${m}; return [r instanceof D, S(r)];`);
}
F(subclass + "var c = new C(3); return [c.length, S(c), c instanceof C];");
F(subclass + "var c = new C(1, 2, 3); return [c.length, S(c)];");
F(subclass + "var c = new C(); c.push(1); return [c.length, Array.isArray(c), S(c)];");
F(subclass + "var c = new C(2**32 - 1); return c.length;");
F(subclass + "var c = new C(-1);");
F("class D extends Array { constructor(n) { super(n); this.tag = 'd'; } }\nvar r = new D(2).map(function (x) { return x; }); return [r.tag, r.length];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.length; } }\nvar r = D.from([1, 2, 3]).filter(function (x) { return x > 1; }); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from([1, 2, 3]).slice(1); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from([1, 2, 3]).splice(1, 1); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from([1, 2, 3]).flat(); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from([1, 2, 3]).concat(); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.of(5, 6); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from({ length: 2, 0: 1, 1: 2 }); return [r.args, S(r)];");
F("class D extends Array { constructor(...a) { super(...a); this.args = a.join(); } }\nvar r = D.from([1, 2]); return [r.args, S(r)];");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.freeze([]); } }; return a.map(function (x) { return x; });");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.freeze([]); } }; return a.slice(0, 0);");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.freeze([]); } }; return a.filter(function () { return false; });");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.freeze([]); } }; return a.splice(0, 0);");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.freeze([]); } }; return a.splice(0, 1);");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return { length: 0 }; } }; return a.splice(0, 2);");
F("var a = [1, 2, 3]; var calls = []; a.constructor = { [Symbol.species]: function (n) { calls.push(arguments.length + ':' + n); return []; } }; a.map(function (x) { return x; }); a.filter(function () { return true; }); a.slice(1); a.splice(0, 1); a.concat([1]); a.flat(); a.flatMap(function (x) { return x; }); return calls;");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return { length: 0, set 0(v) { throw new Error('set0'); } }; } }; return a.map(function (x) { return x; });");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.defineProperty({ length: 0 }, 0, { value: 1, writable: false, configurable: false }); } }; return a.map(function (x) { return x; });");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.defineProperty({}, 'length', { value: 0, writable: false }); } }; return a.slice();");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.defineProperty({}, 'length', { value: 0, writable: false }); } }; return a.splice(0, 1);");
F("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return Object.defineProperty({}, 'length', { value: 0, writable: false }); } }; return a.concat();");
// species de realm distinto não existe aqui; Array vindo de outro construtor com nome parecido:
F("var a = [1, 2, 3]; a.constructor = function Array() {}; return a.map(function (x) { return x; });");
F("var a = [1, 2, 3]; a.constructor = function Array() {}; a.constructor[Symbol.species] = undefined; return a.map(function (x) { return x; });");

// ---- Arrays esparsos, buracos e length gigante.
for (const e of [
  "(function () { var a = []; a[1e9] = 1; return [a.length, a.indexOf(1), a.lastIndexOf(1), a.at(-1), Object.keys(a)]; })()",
  "(function () { var a = []; a[2**32 - 2] = 'x'; return [a.length, a.at(-1), a.lastIndexOf('x'), a.indexOf('x')]; })()",
  "(function () { var a = []; a[2**32 - 2] = 'x'; return a.push(1); })()",
  "(function () { var a = []; a[2**32 - 2] = 'x'; return a.push(1, 2); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.push(1); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return [a.length, a.pop(), a.length]; })()",
  "(function () { var a = []; a.length = 2**32 - 1; a.unshift(1); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.at(-1); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.with(5, 1).length; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.toSpliced(0, 0, 1); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.toReversed(); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.toSorted(); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.slice(2**32 - 3).length; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.slice(0, 3); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.splice(2**32 - 3, 5).length; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.indexOf(1, 2**32 - 3); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.lastIndexOf(undefined, 3); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.includes(undefined, 2**32 - 3); })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.fill(1, 2**32 - 3).length; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.copyWithin(0, 2**32 - 3).length; })()",
  "(function () { var a = []; a.length = 2**32 - 1; return a.join !== undefined; })()",
  "(function () { var a = []; a.length = 2**32 - 1; a[0] = 1; return a.findLast(function (x) { return x === 1; }); })()",
  "(function () { var a = []; a.length = 2**32 - 1; a[2**32 - 2] = 1; return a.findLast(function () { return true; }); })()",
  "(function () { var a = []; a.length = 2**32 - 1; a[2**32 - 2] = 1; return a.findLastIndex(function () { return true; }); })()",
  "Array.prototype.at.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, -1)",
  "Array.prototype.at.call({ length: 2**53 + 10, [2**53 - 2]: 'e' }, -1)",
  "Array.prototype.at.call({ length: Infinity, [2**53 - 2]: 'e' }, -1)",
  "Array.prototype.at.call({ length: -5 }, 0)",
  "Array.prototype.at.call({ length: '3', 2: 'z' }, -1)",
  "Array.prototype.at.call({ length: 1.9, 0: 'z', 1: 'w' }, 1)",
  "Array.prototype.at.call('abc', -1)",
  "Array.prototype.at.call(5, 0)",
  "Array.prototype.at.call(true, 0)",
  "Array.prototype.indexOf.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, 'e', 2**53 - 3)",
  "Array.prototype.indexOf.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, 'e', -2)",
  "Array.prototype.lastIndexOf.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, 'e')",
  "Array.prototype.lastIndexOf.call({ length: 2**53 - 1, 3: 'e' }, 'e', 5)",
  "Array.prototype.lastIndexOf.call({ length: 2**53 - 1, 3: 'e' }, 'e', -(2**53 - 1) + 3)",
  "Array.prototype.includes.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, 'e', -1)",
  "Array.prototype.includes.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, 'e', 2**53 - 2)",
  "Array.prototype.slice.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, -1)",
  "Array.prototype.slice.call({ length: 2**53 - 1, [2**53 - 2]: 'e' }, -2**53, 1)",
  "Array.prototype.slice.call({ length: 2**53 - 1 }, 2**53 - 2, 2**53)",
  "Array.prototype.slice.call({ length: 2**53 - 1 }, 0, 2**32)",
  "Array.prototype.slice.call({ length: 2**53 - 1 }, 0, 2**32 - 1).length",
  "Array.prototype.splice.call({ length: 2**53 - 1 }, 0, 0, 1)",
  "Array.prototype.splice.call({ length: 2**53 - 1 }, 2**53 - 2, 1, 1, 2)",
  "Array.prototype.push.call({ length: 2**53 - 1 }, 1)",
  "Array.prototype.push.call({ length: 2**53 - 1 })",
  "Array.prototype.push.call({ length: 2**53 - 2 }, 1)",
  "(function () { var o = { length: 2**53 - 2 }; Array.prototype.push.call(o, 'a'); return [o.length, o[2**53 - 2]]; })()",
  "(function () { var o = { length: 2**53 - 1 }; try { Array.prototype.push.call(o, 'a'); } catch (e) { return [e.name, o.length, o[2**53 - 1]]; } })()",
  "(function () { var o = { length: 2**53 + 5 }; Array.prototype.push.call(o); return o.length; })()",
  "(function () { var o = { length: 2**53 - 1 }; Array.prototype.pop.call(o); return o.length; })()",
  "(function () { var o = { length: 0 }; Array.prototype.pop.call(o); return o.length; })()",
  "(function () { var o = { length: -3 }; Array.prototype.pop.call(o); return o.length; })()",
  "(function () { var o = { length: 'x' }; Array.prototype.pop.call(o); return o.length; })()",
  "(function () { var o = {}; Array.prototype.push.call(o, 1, 2); return [o.length, o[0], o[1]]; })()",
  "(function () { var o = { length: 2, 0: 'a', 1: 'b' }; Array.prototype.unshift.call(o, 'z'); return [o.length, o[0], o[1], o[2]]; })()",
  "(function () { var o = { length: 2, 0: 'a', 1: 'b' }; return [Array.prototype.shift.call(o), o.length, o[0], o[1]]; })()",
  "(function () { var o = { length: 2**53 - 1 }; Array.prototype.unshift.call(o); return o.length; })()",
  "(function () { var o = { length: 2**53 - 1 }; Array.prototype.unshift.call(o, 1); })()",
  "(function () { var o = { length: 3, 0: 'a', 2: 'c' }; Array.prototype.reverse.call(o); return [o[0], 1 in o, o[2], o.length]; })()",
  "(function () { var o = { length: 3, 0: 'a', 2: 'c' }; Array.prototype.fill.call(o, 'x'); return [o[0], o[1], o[2], o.length]; })()",
  "(function () { var o = { length: 5, 0: 'a', 1: 'b' }; Array.prototype.copyWithin.call(o, 2, 0, 2); return [o[0], o[1], o[2], o[3], o[4]]; })()",
  "(function () { var o = { length: 5, 0: 'a', 2: 'c' }; Array.prototype.copyWithin.call(o, 1, 2); return [o[0], o[1], 2 in o, 3 in o, 4 in o]; })()",
  "(function () { var o = { length: 5, 0: 'a', 2: 'c' }; Array.prototype.copyWithin.call(o, 0, 1); return [0 in o, 1 in o, o[1], 2 in o]; })()",
  "(function () { var o = { length: 3, 0: 'c', 1: 'a', 2: 'b' }; Array.prototype.sort.call(o); return [o[0], o[1], o[2]]; })()",
  "(function () { var o = { length: 4, 0: 'c', 2: 'a', 3: undefined }; Array.prototype.sort.call(o); return [o[0], o[1], 2 in o, 3 in o, o.length]; })()",
  "Array.prototype.toSorted.call({ length: 3, 0: 'c', 1: 'a' })",
  "Array.prototype.toReversed.call({ length: 3, 0: 'c', 1: 'a' })",
  "Array.prototype.toSpliced.call({ length: 3, 0: 'c', 1: 'a' }, 1, 1)",
  "Array.prototype.with.call({ length: 3, 0: 'c', 1: 'a' }, 2, 'q')",
  "Array.prototype.with.call({ length: 2**32 }, 0, 'q')",
  "Array.prototype.toSorted.call({ length: 2**32 })",
  "Array.prototype.toReversed.call({ length: 2**32 })",
  "Array.prototype.toSpliced.call({ length: 2**32 }, 0, 0)",
  "Array.prototype.toSpliced.call({ length: 2**53 - 1 }, 0, 0, 1)",
  "Array.prototype.toSpliced.call({ length: 2**53 - 1 }, 0, 2**53)",
  "Array.prototype.flat.call({ length: 2, 0: [1], 1: [2, [3]] }, 2)",
  "Array.prototype.flat.call({ length: 2**53 - 1 })",
  "Array.prototype.flatMap.call({ length: 2, 0: 1, 1: 2 }, function (x) { return [x, x]; })",
  "Array.prototype.concat.call({ length: 2, 0: 1 }, [3])",
  "Array.prototype.concat.call(1, [3])",
  "Array.prototype.concat.call('ab', 'cd').length",
  "Array.prototype.join.call({ length: 3, 0: 'a', 2: 'c' }, '-')",
  "Array.prototype.join.call({ length: 2**32 }, '')",
  "Array.prototype.join.call({ length: 0 })",
  "Array.prototype.join.call('abc', '-')",
  "Array.prototype.join.call({ length: 3, 0: null, 1: undefined, 2: 0 })",
  "Array.prototype.map.call('abc', function (c) { return c + c; })",
  "Array.prototype.map.call({ length: 3, 0: 1, 2: 3 }, function (x) { return x * 2; })",
  "Array.prototype.filter.call({ length: 3, 0: 1, 2: 3 }, function () { return true; })",
  "Array.prototype.forEach.call({ length: 2**32, 5: 1 }, function () { throw new Error('x'); })",
  "Array.prototype.forEach.call({ length: 2**53 - 1, 5: 1 }, function () { throw new Error('x'); })",
  "Array.prototype.some.call({ length: 2**53 - 1, 5: 1 }, function () { return true; })",
  "Array.prototype.find.call({ length: 2**53 - 1, 0: 1 }, function () { return true; })",
  "Array.prototype.findIndex.call({ length: 2**53 - 1, 0: 1 }, function () { return true; })",
  "Array.prototype.every.call({ length: 2**53 - 1, 0: 1 }, function () { return false; })",
  "Array.prototype.reduce.call({ length: 2**53 - 1, 5: 1 }, function (p, x) { return 7; })",
  "Array.prototype.reduceRight.call({ length: 2**53 - 1, [2**53 - 2]: 1 }, function (p, x) { return 7; })",
  "Array.prototype.findLast.call({ length: 2**53 - 1, [2**53 - 2]: 1 }, function () { return true; })",
  "Array.prototype.findLastIndex.call({ length: 2**53 - 1, [2**53 - 2]: 1 }, function () { return true; })",
  "Array.prototype.findLast.call({ length: 3, 1: 'b' }, function (x, i, o) { return i === 2; })",
  "Array.prototype.entries.call({ length: 2, 0: 'a' }).next()",
  "D(Array.prototype.entries.call({ length: 2, 0: 'a' }))",
  "D(Array.prototype.keys.call({ length: 2 }))",
  "D(Array.prototype.values.call('ab'))",
  "D(Array.prototype.keys.call({ length: 2**53 - 1 }))".replace("D(", "(function (it) { return [it.next().value, it.next().value]; })("),
  "D(Array.prototype.values.call(5))",
  "S([,].concat([,]))",
  "S(Array(3))", "S(Array(3).fill())", "S(Array.apply(null, Array(3)))", "S([...Array(3)])", "S(Array(3).map(function () { return 1; }))",
  "S(Array.from(Array(3)))", "S(Object.keys(Array(3)))", "S(Array(3).join('-'))", "S(Array(3).indexOf(undefined))", "S(Array(3).includes(undefined))",
  "S(Array(3).findIndex(function (x) { return x === undefined; }))", "S(Array(3).flat())", "S(Array(3).flatMap(function (x) { return [x]; }))",
  "S(Array(3).entries().next().value)", "S(Array(3).sort())", "S(Array(3).toSorted())", "S(Array(3).reverse())", "S(Array(3).toReversed())",
  "S(Array(3).with(1, 1))", "S(Array(3).toSpliced(1, 1))", "S(Array(3).slice(1))", "S(Array(3).splice(1, 1))", "S(Array(3).concat([1]))",
  "S(Array(3).copyWithin(0, 1))", "S(Array(3).fill(1, 1))", "S(Array(3).at(1))", "S(Array(3).lastIndexOf(undefined))", "S(Array(3).reduce(function (p) { return p; }, 0))",
  "S([1, , 3].reduce(function (p, x) { return p + x; }))", "S([, , 3].reduce(function (p, x) { return p + x; }))", "S([1, , 3].reduceRight(function (p, x) { return p + x; }))",
  "S([1, , 3].filter(function () { return true; }))", "S([1, , 3].map(function (x) { return x; }))", "S([1, , 3].every(function (x) { return x; }))",
  "S([1, , 3].some(function (x) { return x === undefined; }))", "S([1, , 3].find(function (x) { return x === undefined; }))",
  "S([1, , 3].findLast(function (x) { return x === undefined; }))", "S([1, , 3].findLastIndex(function (x) { return x === undefined; }))",
  "S([1, , 3].toString())", "S([1, , 3].flat())", "S([1, [, 2], 3].flat())", "S([1, [, 2], 3].flat(Infinity))",
  "S([3, , 1, undefined, 2].sort())", "S([3, , 1, undefined, 2].toSorted())", "S([3, , 1, undefined, 2].sort(function (a, b) { return b - a; }))",
  "S([3, , 1, undefined, 2].toSorted(function (a, b) { return b - a; }))", "S([undefined, undefined, , ].sort())", "S([, undefined].sort().length)",
  "S([3, 2, 1].sort(undefined))", "S([3, 2, 1].sort(function () { return NaN; }))", "S([3, 2, 1].sort(function () { return 0; }))", "S([3, 2, 1].sort(function () { return -1; }))",
  "S([3, 2, 1].sort(function () { return 1; }))", "S([3, 2, 1].sort(function () { return '1'; }))", "S([3, 2, 1].sort(function () { return undefined; }))",
  "S([3, 2, 1].sort(function () { return {}; }))", "S([3, 2, 1].sort(function (a, b) { return Math.random() - Math.random() ? 0 : 0; }))",
  "S([10, 9, 1, 100, 25].sort())", "S([10, 9, 1, 100, 25].sort(function (a, b) { return a - b; }))", "S(['b', undefined, 'a', , 'c'].sort())",
  "S([true, false, null, 1, 'a', undefined, NaN].sort())", "S([-1, -2, 0, -0, 1].sort())", "S([1n, 2, 3n, 1].sort())", "S([[2], [1, 5], [1]].sort())",
  "S([{}, {}].sort())", "S(['\\u00e9', 'e', 'z', 'a'].sort())", "S(['\\ud83d\\ude00', '\\uffff', 'a'].sort())",
]) E(e);

// ---- Ordenação estável e comparadores inconsistentes.
F("var a = []; for (var i = 0; i < 40; i++) a.push({ k: i % 4, i: i }); a.sort(function (x, y) { return x.k - y.k; }); return a.map(function (o) { return o.k + ':' + o.i; }).join(' ');");
F("var a = []; for (var i = 0; i < 300; i++) a.push({ k: i % 7, i: i }); a.sort(function (x, y) { return x.k - y.k; }); return a.map(function (o) { return o.k + ':' + o.i; }).join(' ');");
F("var a = []; for (var i = 0; i < 40; i++) a.push({ k: i % 4, i: i }); return a.toSorted(function (x, y) { return y.k - x.k; }).map(function (o) { return o.k + ':' + o.i; }).join(' ');");
F("var a = []; for (var i = 0; i < 40; i++) a.push(i); a.sort(function () { return 0; }); return a.join();");
F("var a = []; for (var i = 0; i < 40; i++) a.push(i); a.sort(function () { return -1; }); return a.join();");
F("var a = []; for (var i = 0; i < 40; i++) a.push(i); a.sort(function () { return 1; }); return a.join();");
F("var a = []; for (var i = 0; i < 40; i++) a.push(i); a.sort(function (x, y) { return x < y ? 1 : -1; }); return a.join();");
F("var a = []; for (var i = 0; i < 40; i++) a.push(i); a.sort(function (x, y) { return (x % 3) - (y % 3) || 0; }); return a.join();");
F("var n = 0; var a = [5, 4, 3, 2, 1]; a.sort(function (x, y) { n++; return x - y; }); return [a.join(), n];");
F("var n = 0; var a = [1, 2, 3, 4, 5]; a.sort(function (x, y) { n++; return x - y; }); return [a.join(), n];");
F("var n = 0; var a = [3, , 1, undefined, 2]; a.sort(function (x, y) { n++; return x - y; }); return [S(a), n];");
F("var seen = []; [3, 1, 2].sort(function (x, y) { seen.push(x + ',' + y); return x - y; }); return seen;");
F("var seen = []; [4, 3, 2, 1].sort(function (x, y) { seen.push(x + ',' + y); return x - y; }); return seen;");
F("var seen = []; [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].sort(function (x, y) { seen.push(x + ',' + y); return x - y; }); return seen.length;");
F("var seen = []; [12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1].sort(function (x, y) { seen.push(x + ',' + y); return x - y; }); return seen.length;");
F("var a = [3, 1, 2]; a.sort(function (x, y) { a.push(9); return x - y; }); return a;");
F("var a = [3, 1, 2]; a.sort(function (x, y) { a.length = 0; return x - y; }); return a;");
F("var a = [3, 1, 2]; a.sort(function (x, y) { a[0] = 'z'; return x - y; }); return a;");
F("var a = [3, 1, 2]; a.sort(function (x, y) { throw new Error('cmp'); });");
F("var a = [3, 1, 2]; try { a.sort(function (x, y) { throw new Error('cmp'); }); } catch (e) {} return a;");
F("var a = [3, 1, 2]; return a.toSorted(function (x, y) { a.push(9); return x - y; });");
F("var a = [3, 1, 2]; return [a.toSorted(function (x, y) { a.length = 0; return x - y; }), a];");
F("var a = [3, 1, 2]; return a.sort(function (x, y) { return { valueOf() { return x - y; } }; });");
F("var a = [3, 1, 2]; return a.sort(function (x, y) { return { valueOf() { throw new TypeError('vo'); } }; });");
F("var a = [3, 1, 2]; return a.sort(function (x, y) { return 1n; });");
F("var a = [3, 1, 2]; return a.sort(function (x, y) { return Symbol(); });");
F("var a = [{ toString() { return 'b'; } }, { toString() { return 'a'; } }]; return a.sort().map(String);");
F("var a = [{ toString() { throw new Error('ts'); } }, 1]; return a.sort();");
F("var a = [1, { toString() { throw new Error('ts'); } }]; return a.toSorted();");
F("var a = [Symbol('a'), 1]; return a.sort();");
F("var a = [Symbol('a')]; return a.sort();");
F("var a = [1, 2, 3]; var r = a.sort(); return r === a;");
F("var a = [1, 2, 3]; var r = a.reverse(); return r === a;");
F("var a = [1, 2, 3]; var r = a.fill(0); return r === a;");
F("var a = [1, 2, 3]; var r = a.copyWithin(0, 1); return r === a;");
F("var a = [1, 2, 3]; var r = a.toSorted(); return r === a;");
F("var a = [3, 1, 2]; var r = a.toSorted(); return [a, r];");
F("var a = [3, 1, 2]; var r = a.toReversed(); return [a, r];");
F("var a = [3, 1, 2]; var r = a.with(0, 0); return [a, r];");
F("var a = [3, 1, 2]; var r = a.toSpliced(0, 1); return [a, r];");
F("var a = [3, 1, , 2]; return [a.toSorted(), a.toReversed(), a.with(0, 0), a.toSpliced(0, 1)];");
F("var a = [3, 1, , 2]; return [Object.keys(a.toSorted()), Object.keys(a.toReversed()), Object.keys(a.with(0, 0)), Object.keys(a.toSpliced(0, 1))];");

// ---- Getters, setters e mutações durante a iteração.
for (const e of [
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) a.push(9); }); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) a.pop(); }); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) a.shift(); }); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) a.length = 1; }); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) delete a[1]; }); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.forEach(function (x, i) { r.push(x); if (i === 0) a[1] = 'n'; }); return r; })()",
  "(function () { var a = [1, 2, 3]; return a.map(function (x, i) { if (i === 0) a.length = 1; return x; }); })()",
  "(function () { var a = [1, 2, 3]; return a.filter(function (x, i) { if (i === 0) a.push(4); return true; }); })()",
  "(function () { var a = [1, 2, 3]; return a.every(function (x, i) { if (i === 0) a[2] = 0; return x; }); })()",
  "(function () { var a = [1, 2, 3]; return a.some(function (x, i) { if (i === 0) a[2] = 'hit'; return x === 'hit'; }); })()",
  "(function () { var a = [1, 2, 3]; return a.find(function (x, i) { if (i === 0) a.length = 1; return x === undefined; }); })()",
  "(function () { var a = [1, 2, 3]; return a.findIndex(function (x, i) { if (i === 0) a.length = 1; return x === undefined; }); })()",
  "(function () { var a = [1, 2, 3]; return a.findLast(function (x, i) { if (i === 2) a.length = 1; return x === undefined; }); })()",
  "(function () { var a = [1, 2, 3]; return a.findLastIndex(function (x, i) { if (i === 2) a.length = 1; return x === undefined; }); })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.reduce(function (p, x, i) { r.push(x); if (i === 1) a.length = 2; return p; }, 0); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; a.reduceRight(function (p, x, i) { r.push(x); if (i === 2) a.length = 1; return p; }, 0); return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; for (var x of a) { r.push(x); if (r.length === 1) a.push(4); } return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; for (var x of a) { r.push(x); if (r.length === 1) a.length = 1; } return r; })()",
  "(function () { var a = [1, 2, 3]; var r = []; for (var x of a) { r.push(x); if (r.length < 5) a.push(0); } return r.length; })()",
  "(function () { var a = [1, 2, 3]; var it = a[Symbol.iterator](); it.next(); a.length = 0; return [it.next(), it.next()]; })()",
  "(function () { var a = [1, 2, 3]; var it = a[Symbol.iterator](); a.length = 0; var r = it.next(); a.push(1); return [r, it.next()]; })()",
  "(function () { var a = [1, 2, 3]; var it = a.entries(); it.next(); a.splice(0, 1); return [it.next().value, it.next().value, it.next().done]; })()",
  "(function () { var a = [1]; var it = a.keys(); it.next(); it.next(); a.push(2); return it.next(); })()",
  "(function () { var it = [][Symbol.iterator](); return [Object.prototype.toString.call(it), it[Symbol.toStringTag], typeof it.next, Object.getPrototypeOf(Object.getPrototypeOf(it)) === Object.getPrototypeOf(Object.getPrototypeOf((function* () {})()).constructor.prototype)]; })()",
  "(function () { var it = [1][Symbol.iterator](); return [it[Symbol.iterator]() === it, it.next(), it.next(), it.next()]; })()",
  "(function () { var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return [Object.getOwnPropertyNames(AIP), AIP[Symbol.toStringTag], AIP.next.length, AIP.next.name]; })()",
  "(function () { var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return AIP.next.call({}); })()",
  "(function () { var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return AIP.next.call(null); })()",
  "(function () { var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return AIP.next.call(new Map().entries()); })()",
  "(function () { var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return AIP.next.call([]); })()",
  "(function () { var it = 'ab'[Symbol.iterator](); var AIP = Object.getPrototypeOf([][Symbol.iterator]()); return AIP.next.call(it); })()",
  "(function () { var it = new Uint8Array([5, 6]).values(); return [Object.getPrototypeOf(it) === Object.getPrototypeOf([][Symbol.iterator]()), it.next(), it.next(), it.next()]; })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 0; return 'g'; }, configurable: true }); return [a.map(function (x) { return x; }), a.length]; })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.push(0); return 'g'; }, configurable: true }); return a.slice(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.slice(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.concat([]); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.join(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.reverse(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.indexOf(undefined); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.includes(undefined, 1); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.toSorted(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.toReversed(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.with(0, 0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.toSpliced(0, 0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.flat(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.copyWithin(0, 1); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.splice(0, 3); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.shift(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { a.length = 1; return 'g'; }, configurable: true }); return a.unshift(0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.length; })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.at(1); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.indexOf(3); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.lastIndexOf(1); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.includes(3); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { get() { throw new Error('get1'); }, configurable: true }); return a.slice(2); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { set(v) { throw new Error('set1'); }, configurable: true }); return a.fill(0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { set(v) { throw new Error('set1'); }, configurable: true }); return a.reverse(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { set(v) { throw new Error('set1'); }, configurable: true }); return a.sort(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, { set(v) { throw new Error('set1'); }, configurable: true }); return a.push(4); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, writable: false }); return a.pop(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, writable: false, configurable: false }); a.length = 1; return a.length; })()",
  "(function () { 'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, writable: false, configurable: false }); a.length = 1; })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, configurable: false }); return [a.splice(1, 2), a]; })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, configurable: false }); return a.shift(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, configurable: false }); return a.pop(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 0, { value: 1, writable: false }); return a.reverse(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 0, { value: 1, writable: false }); return a.unshift(0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 0, { value: 1, writable: false }); return a.sort(function (x, y) { return y - x; }); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.pop(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.splice(0, 1); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.shift(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.unshift(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.push(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.reverse(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.fill(0); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.sort(); })()",
  "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.copyWithin(0, 1); })()",
  "(function () { var a = [1, 2, 3]; Array.prototype[1] = 'proto'; try { return [a.indexOf('proto'), a.includes('proto'), a.slice(), S(a.concat()), a.join(), a.at(1)]; } finally { delete Array.prototype[1]; } })()",
  "(function () { var a = [1, , 3]; Array.prototype[1] = 'proto'; try { return [a.indexOf('proto'), a.includes('proto'), S(a.slice()), S(a.concat()), a.join(), a.at(1), S(a.map(function (x) { return x; })), S(a.toSorted()), S(a.flat()), S(a.with(0, 0)), S([...a])]; } finally { delete Array.prototype[1]; } })()",
  "(function () { var a = [1, , 3]; Array.prototype[1] = 'proto'; try { return [S(a.reverse()), S(a.copyWithin(0, 1)), Object.keys(a), S(a.toReversed()), S(a.toSpliced(0, 0)), S(a.fill(7, 1, 2))]; } finally { delete Array.prototype[1]; } })()",
  "(function () { var a = [1, , 3]; Array.prototype[1] = 'proto'; try { var r = []; a.forEach(function (x) { r.push(x); }); return [r, a.every(function (x) { return x; }), a.reduce(function (p, x) { return p + x; })]; } finally { delete Array.prototype[1]; } })()",
  "(function () { var a = [1, , 3]; Object.prototype[1] = 'oproto'; try { return [a.indexOf('oproto'), a.includes('oproto'), a.at(1), a.join()]; } finally { delete Object.prototype[1]; } })()",
  "(function () { var a = [1, 2, 3]; Array.prototype[3] = 'p3'; try { return [a.length, a.indexOf('p3'), a.includes('p3'), S([...a]), a.concat([]).length]; } finally { delete Array.prototype[3]; } })()",
  "(function () { Object.defineProperty(Array.prototype, 1, { get() { return 'getter'; }, set(v) { this.__set = v; }, configurable: true }); try { var a = [1, , 3]; var b = []; b[1] = 'x'; return [a.join(), S(a.slice()), b.__set, b.length, Object.keys(b)]; } finally { delete Array.prototype[1]; } })()",
  "(function () { Object.defineProperty(Array.prototype, 1, { get() { return 'getter'; }, set(v) { this.__set = v; }, configurable: true }); try { var a = [1, 2, 3]; a.length = 1; a.push(5, 6); return [a.length, a.__set, S(a)]; } finally { delete Array.prototype[1]; } })()",
]) E(e);

// ---- Proxies.
for (const e of [
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); } }); p.indexOf(2); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.map(function (x) { return x; }); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.includes(3); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.slice(1); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.join(); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.reverse(); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.reverse(); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.splice(1, 1); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.push(4); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.shift(); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.unshift(0); return log; })()",
  "(function () { var log = []; var p = new Proxy([1, 2, 3], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.pop(); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }, deleteProperty(t, k) { log.push('del:' + String(k)); return Reflect.deleteProperty(t, k); }, defineProperty(t, k, d) { log.push('def:' + String(k)); return Reflect.defineProperty(t, k, d); } }); p.sort(); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.toSorted(); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.toReversed(); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.with(1, 0); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.toSpliced(1, 1); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.at(-1); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.flat(); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.findLast(function () { return false; }); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.lastIndexOf(9); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); [...p]; return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); Array.from(p); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.reduce(function (a, b) { return a + b; }); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); } }); p.fill(0); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); } }); p.fill(0); return log; })()",
  "(function () { var log = []; var p = new Proxy([3, 1, 2], { get(t, k, r) { log.push('get:' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return Reflect.has(t, k); }, set(t, k, v, r) { log.push('set:' + String(k)); return Reflect.set(t, k, v, r); } }); p.copyWithin(0, 1); return log; })()",
  "Array.isArray(new Proxy([], {}))", "Array.isArray(new Proxy({}, {}))", "Array.isArray(new Proxy(new Proxy([], {}), {}))",
  "(function () { var r = Proxy.revocable([], {}); r.revoke(); return Array.isArray(r.proxy); })()",
  "(function () { var r = Proxy.revocable([], {}); r.revoke(); return Array.prototype.map.call(r.proxy, function () {}); })()",
  "(function () { var r = Proxy.revocable([], {}); r.revoke(); return [].concat(r.proxy); })()",
  "(function () { var r = Proxy.revocable([], {}); r.revoke(); return [].concat([r.proxy]).length; })()",
  "Object.prototype.toString.call(new Proxy([], {}))", "Object.prototype.toString.call(new Proxy({}, {}))", "JSON.stringify(new Proxy([1, 2], {}))",
  "S([].concat(new Proxy([1, 2], {})))", "S(Array.prototype.slice.call(new Proxy([1, 2], {})))", "S(Array.prototype.map.call(new Proxy([1, 2], {}), function (x) { return x; }))",
  "(function () { var p = new Proxy([1, 2], {}); return [p.map(function (x) { return x; }) instanceof Array, p.slice().length, p.concat([]).length]; })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**53 : Reflect.get(t, k, r); } }); return p.at(-1); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.slice(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.map(function () {}); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.filter(function () {}); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.flat(); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.splice(0, 0); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.concat(); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.toSorted(); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.with(0, 0); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.toReversed(); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { return k === 'length' ? 2**32 : Reflect.get(t, k, r); } }); return p.toSpliced(0, 0); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { if (k === 'constructor') return { [Symbol.species]: function (n) { return { length: 0, n: n }; } }; return Reflect.get(t, k, r); } }); return p.map(function (x) { return x; }); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { if (k === 'constructor') throw new Error('ctor'); return Reflect.get(t, k, r); } }); return p.map(function (x) { return x; }); })()",
  "(function () { var p = new Proxy([1, 2], { get(t, k, r) { if (k === 'constructor') throw new Error('ctor'); return Reflect.get(t, k, r); } }); return p.indexOf(1); })()",
  "(function () { var p = new Proxy({ length: 2, 0: 'a', 1: 'b' }, {}); return Array.prototype.map.call(p, function (x) { return x; }); })()",
  "(function () { var p = new Proxy({ length: 2, 0: 'a', 1: 'b' }, { getOwnPropertyDescriptor() { throw new Error('gopd'); } }); return Array.prototype.slice.call(p); })()",
  "(function () { var p = new Proxy([1, 2], { ownKeys() { throw new Error('ok'); } }); return p.slice(); })()",
  "(function () { var p = new Proxy([1, 2], { ownKeys() { throw new Error('ok'); } }); return Array.from(p); })()",
  "(function () { var p = new Proxy([1, 2], { ownKeys() { throw new Error('ok'); } }); return p.concat(); })()",
  "(function () { var p = new Proxy([1, 2], { ownKeys() { throw new Error('ok'); } }); return [...p]; })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.map(function (x) { return x; }); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.at(0); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.includes(2); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.toSorted(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.with(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.findLast(function () { return false; }); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.join(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.flat(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.concat(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.reverse(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.fill(0); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.copyWithin(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.sort(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.reduce(function (a, b) { return a + b; }); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.indexOf(2); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.lastIndexOf(2); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.slice(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.splice(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.shift(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.unshift(0); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.pop(); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return p.push(0); })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return [...p]; })()",
  "(function () { var p = new Proxy([1, 2], { has() { throw new Error('has'); } }); return Array.from(p); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.push(0); })()",
  "(function () { 'use strict'; var p = new Proxy([1, 2], { set() { return false; } }); return p.push(0); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.fill(0); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.reverse(); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.sort(); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.pop(); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.shift(); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.unshift(1); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.splice(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { set() { return false; } }); return p.copyWithin(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { deleteProperty() { return false; } }); return p.pop(); })()",
  "(function () { var p = new Proxy([1, 2], { deleteProperty() { return false; } }); return p.shift(); })()",
  "(function () { var p = new Proxy([1, 2], { deleteProperty() { return false; } }); return p.splice(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); return p.map(function (x) { return x; }); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); return p.push(3); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); return Array.from.call(function () { return p; }, [1]); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); return Array.of.call(function () { return p; }, 1); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.map(function (x) { return x; }); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.slice(); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.concat(); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.filter(function () { return true; }); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.splice(0, 1); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.flat(); })()",
  "(function () { var p = new Proxy([1, 2], { defineProperty() { return false; } }); var a = [1, 2]; a.constructor = { [Symbol.species]: function () { return p; } }; return a.flatMap(function (x) { return x; }); })()",
]) E(e);

// ---- fromAsync: o resultado precisa sair em microtarefa, então grava R no then.
const asyncCases = [
  "Array.fromAsync([1, 2, 3])", "Array.fromAsync([Promise.resolve(1), 2, Promise.resolve(3)])", "Array.fromAsync('abc')",
  "Array.fromAsync({ length: 2, 0: 'a', 1: Promise.resolve('b') })", "Array.fromAsync([1, 2], function (x) { return x * 2; })",
  "Array.fromAsync([1, 2], function (x) { return Promise.resolve(x * 2); })", "Array.fromAsync([1, 2], function (x, i) { return this.k + i; }, { k: 5 })",
  "Array.fromAsync([1, 2], function () { return typeof this; })", "Array.fromAsync([1, 2], function () { 'use strict'; return typeof this; })",
  "Array.fromAsync(new Set([1, 2]))", "Array.fromAsync(new Map([[1, 2]]))", "Array.fromAsync(new Uint8Array([1, 2]))",
  "Array.fromAsync((function* () { yield 1; yield Promise.resolve(2); })())", "Array.fromAsync((async function* () { yield 1; yield 2; })())",
  "Array.fromAsync((async function* () { yield 1; throw new Error('boom'); })())", "Array.fromAsync((function* () { yield 1; throw new Error('boom'); })())",
  "Array.fromAsync([Promise.reject(new Error('rej'))])", "Array.fromAsync([1], function () { throw new Error('map'); })",
  "Array.fromAsync([1], function () { return Promise.reject(new Error('maprej')); })", "Array.fromAsync(null)", "Array.fromAsync(undefined)", "Array.fromAsync()",
  "Array.fromAsync(5)", "Array.fromAsync({})", "Array.fromAsync({ length: 2 })", "Array.fromAsync({ length: -1 })", "Array.fromAsync({ length: 2**53 })",
  "Array.fromAsync([1], 1)", "Array.fromAsync([1], null)", "Array.fromAsync([1], undefined)", "Array.fromAsync([1], {})",
  "Array.fromAsync.call(undefined, [1])", "Array.fromAsync.call(Object, [1, 2])", "Array.fromAsync.call(function () { this.made = true; }, [1, 2])",
  "Array.fromAsync.call(function () { this.made = true; }, { length: 2, 0: 1, 1: 2 })", "Array.fromAsync.call(1, [1])",
  "Array.fromAsync.call(function () { return Object.freeze({}); }, [1])", "Array.fromAsync.call(function () { return Object.freeze([]); }, { length: 0 })",
  "(function () { class C extends Array {} return C.fromAsync([1, 2]).then(function (r) { return [r instanceof C, S(r)]; }); })()",
  "(function () { var o = { [Symbol.asyncIterator]() { var i = 0; return { next() { return Promise.resolve({ done: i > 1, value: i++ }); } }; } }; return Array.fromAsync(o); })()",
  "(function () { var o = { [Symbol.asyncIterator]() { var i = 0; return { next() { return Promise.resolve({ done: i > 1, value: i++ }); }, return() { return Promise.resolve({}); } }; }, [Symbol.iterator]() { throw new Error('sync'); } }; return Array.fromAsync(o); })()",
  "(function () { var o = { [Symbol.asyncIterator]: null, [Symbol.iterator]: function* () { yield 'sync'; } }; return Array.fromAsync(o); })()",
  "(function () { var o = { [Symbol.asyncIterator]: undefined, length: 1, 0: 'al' }; return Array.fromAsync(o); })()",
  "(function () { var o = { [Symbol.asyncIterator]: 1 }; return Array.fromAsync(o); })()",
  "(function () { var log = []; var o = { [Symbol.asyncIterator]() { return { next() { log.push('next'); return Promise.resolve({ done: log.length > 2, value: 1 }); }, return() { log.push('return'); return Promise.resolve({}); } }; } }; return Array.fromAsync(o, function () { throw new Error('m'); }).catch(function (e) { return log; }); })()",
  "(function () { var log = []; var o = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: false, value: 1 }); }, return() { log.push('return'); return Promise.resolve({}); } }; } }; return Array.fromAsync(o, function () { throw new Error('m'); }).catch(function (e) { return [e.message, log]; }); })()",
  "(function () { var log = []; var o = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { log.push('sret'); return {}; } }; } }; return Array.fromAsync(o, function () { throw new Error('m'); }).catch(function (e) { return [e.message, log]; }); })()",
  "(function () { var order = []; var p = Array.fromAsync([1, 2], function (x) { order.push('map' + x); return x; }); order.push('sync'); return p.then(function (r) { return [order, r]; }); })()",
  "(function () { var order = []; Promise.resolve().then(function () { order.push('t1'); }).then(function () { order.push('t2'); }).then(function () { order.push('t3'); }); return Array.fromAsync([1]).then(function () { order.push('done'); return order; }).then(function (o) { return o.concat(['x']); }); })()",
  "(function () { return Array.fromAsync([1, 2]) instanceof Promise; })()",
  "(function () { return Array.fromAsync.call(null, 1) instanceof Promise; })()",
  "(function () { return Array.fromAsync([1], 5) instanceof Promise; })()",
  "(function () { var p = Array.fromAsync([1], 5); p.catch(function () {}); return p; })()",
  "(function () { return Object.getOwnPropertyDescriptor(Array, 'fromAsync').writable + ':' + Object.getOwnPropertyDescriptor(Array, 'fromAsync').enumerable + ':' + Array.fromAsync.name; })()",
];
for (const e of asyncCases) {
  B(`Promise.resolve(${e}).then(function (v) { R = 'ok ' + S(v); }, function (e) { R = 'rej ' + (e && e.name) + ': ' + (e && e.message); });`);
}

// ---- TypedArray versus Array.
const typed = ["Uint8Array", "Int8Array", "Uint8ClampedArray", "Int16Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array"];
for (const T of typed) {
  const v = T.startsWith("Big") ? "[3n, 1n, 2n]" : "[3, 1, 2]";
  for (const m of [
    "at(-1)", "indexOf(1)", "includes(2)", "join('-')", "slice(1)", "toSorted()", "toReversed()", "with(0, 9)".replace("9", T.startsWith("Big") ? "9n" : "9"),
    "sort()", "reverse()", "fill(0)".replace("0", T.startsWith("Big") ? "0n" : "0"), "copyWithin(0, 1)", "subarray(1)", "findLast(function (x) { return x > 1; })",
    "findLastIndex(function (x) { return x > 1; })", "map(function (x) { return x; })", "filter(function (x) { return x > 1; })", "lastIndexOf(1)", "entries().next().value",
    "toString()", "toLocaleString()", "every(function (x) { return x; })", "reduce(function (a, b) { return a + b; })".replace("a + b", T.startsWith("Big") ? "a + b" : "a + b"),
  ]) E(`new ${T}(${v}).${m}`);
  E(`Array.prototype.slice.call(new ${T}(${v}), 1)`);
  E(`Array.prototype.map.call(new ${T}(${v}), function (x) { return x; })`);
  E(`Array.prototype.concat.call([], new ${T}(${v}))`);
  E(`[].concat(new ${T}(${v})).length`);
  E(`Array.prototype.join.call(new ${T}(${v}), '+')`);
  E(`Array.prototype.reverse.call(new ${T}(${v}))`);
  E(`Array.prototype.sort.call(new ${T}(${v}))`);
  E(`Array.prototype.fill.call(new ${T}(${v}), ${T.startsWith("Big") ? "1n" : "1"})`);
  E(`Array.prototype.indexOf.call(new ${T}(${v}), ${T.startsWith("Big") ? "2n" : "2"})`);
  E(`Array.prototype.includes.call(new ${T}(${v}), ${T.startsWith("Big") ? "2n" : "2"})`);
  E(`Array.prototype.push.call(new ${T}(${v}), 1)`);
  E(`Array.prototype.pop.call(new ${T}(${v}))`);
  E(`Array.prototype.shift.call(new ${T}(${v}))`);
  E(`Array.prototype.splice.call(new ${T}(${v}), 0, 1)`);
  E(`Array.prototype.at.call(new ${T}(${v}), 1)`);
  E(`Array.prototype.with.call(new ${T}(${v}), 0, ${T.startsWith("Big") ? "1n" : "1"})`);
  E(`Array.prototype.toSorted.call(new ${T}(${v}))`);
  E(`Array.prototype.copyWithin.call(new ${T}(${v}), 0, 1)`);
  E(`Array.prototype.flat.call(new ${T}(${v}))`);
  E(`Array.from(new ${T}(${v}), function (x) { return typeof x; })`);
  E(`Array.isArray(new ${T}(${v}))`);
  E(`Array.prototype.concat.call(new ${T}(${v}), [1]).length`);
}
for (const e of [
  "new Uint8Array([3, 1, 2]).with(5, 1)", "new Uint8Array([3, 1, 2]).with(-4, 1)", "new Uint8Array([3, 1, 2]).with(-1, 300)", "new Uint8Array([3, 1, 2]).with(0, 'x')",
  "new Uint8Array([3, 1, 2]).with(0, 1n)", "new BigInt64Array(2).with(0, 1)", "new Uint8Array([3, 1, 2]).at(Infinity)", "new Uint8Array([3, 1, 2]).at(-Infinity)",
  "new Uint8Array([3, 1, 2]).toSorted(1)", "new Uint8Array([3, 1, 2]).toSorted(null)", "new Uint8Array([3, 1, 2]).sort(1)", "new Uint8Array([3, 1, 2]).sort(null)",
  "new Float64Array([3, NaN, -0, 0, 1]).sort()", "new Float64Array([3, NaN, -0, 0, 1]).toSorted()", "new Float64Array([3, NaN, -0, 0, 1]).includes(NaN)",
  "new Float64Array([3, NaN, -0, 0, 1]).indexOf(NaN)", "new Float64Array([3, NaN, -0, 0, 1]).indexOf(-0)", "new Float64Array([3, NaN, -0, 0, 1]).lastIndexOf(0)",
  "new Float64Array([3, NaN, -0, 0, 1]).at(2)", "new Float64Array([3, NaN, -0, 0, 1]).join()", "new Float64Array([3, NaN, -0, 0, 1]).reverse()",
  "Array.prototype.sort.call(new Float64Array([3, NaN, -0, 0, 1]))", "Array.prototype.indexOf.call(new Float64Array([NaN]), NaN)",
  "Array.prototype.includes.call(new Float64Array([NaN]), NaN)", "Array.prototype.lastIndexOf.call(new Float64Array([0, -0]), -0)",
  "Array.prototype.slice.call(new Uint8Array(3), 0, 2).constructor === Array", "new Uint8Array(3).slice(0, 2).constructor === Uint8Array",
  "Array.prototype.map.call(new Uint8Array(3), function (x) { return 300 + x; })", "new Uint8Array(3).map(function (x) { return 300 + x; })",
  "new Uint8Array([1, 2, 3]).filter(function () { return false; }).length", "Array.prototype.filter.call(new Uint8Array([1, 2, 3]), function () { return false; }).length",
  "Array.prototype.every.call(new Uint8Array([1, 2, 3]), function (x, i, o) { return o instanceof Uint8Array; })",
  "Array.prototype.forEach.call(new Uint8Array(0), function () { throw 1; })",
  "(function () { var t = new Uint8Array([1, 2, 3]); var r = []; t.forEach(function (x, i) { r.push(x); if (i === 0) t[2] = 9; }); return r; })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); var r = []; Array.prototype.forEach.call(t, function (x, i) { r.push(x); if (i === 0) t[2] = 9; }); return r; })()",
  "(function () { var b = new ArrayBuffer(8, { maxByteLength: 16 }); var t = new Uint8Array(b); var r = []; Array.prototype.forEach.call(t, function (x, i) { r.push(i); if (i === 0) b.resize(2); }); return r; })()",
  "(function () { var b = new ArrayBuffer(8, { maxByteLength: 16 }); var t = new Uint8Array(b); var r = []; t.forEach(function (x, i) { r.push(i); if (i === 0) b.resize(2); }); return r; })()",
  "(function () { var b = new ArrayBuffer(4, { maxByteLength: 16 }); var t = new Uint8Array(b); var it = t.values(); b.resize(2); return D(it); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); var it = t.values(); structuredClone(b, { transfer: [b] }); return it.next(); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); var it = t.values(); structuredClone(b, { transfer: [b] }); return Array.prototype.at.call(t, 0); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return t.at(0); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return Array.prototype.join.call(t); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return t.join(); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return t.length; })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return t.entries(); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return Array.from(t); })()",
  "(function () { var b = new ArrayBuffer(4); var t = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return [].concat(t).length; })()",
  "Object.getPrototypeOf(Uint8Array.prototype) === Object.getPrototypeOf(Int8Array.prototype)",
  "Uint8Array.prototype.join === Array.prototype.join", "Uint8Array.prototype.values === Uint8Array.prototype[Symbol.iterator]",
  "Object.getPrototypeOf(Uint8Array).prototype.toString === Array.prototype.toString", "Object.getPrototypeOf(Uint8Array).prototype.at === Array.prototype.at",
  "Object.getPrototypeOf(Uint8Array).prototype.toLocaleString === Array.prototype.toLocaleString",
  "Object.getOwnPropertyNames(Object.getPrototypeOf(Uint8Array).prototype).sort()",
  "Object.getOwnPropertyNames(Object.getPrototypeOf(Uint8Array)).sort()",
  "[Object.getPrototypeOf(Uint8Array).prototype.at.length, Object.getPrototypeOf(Uint8Array).prototype.with.length, Object.getPrototypeOf(Uint8Array).prototype.toSorted.length, Object.getPrototypeOf(Uint8Array).prototype.findLast.length, Object.getPrototypeOf(Uint8Array).prototype.set.length, Object.getPrototypeOf(Uint8Array).prototype.subarray.length]",
  "Uint8Array.from([1, 2, 3], function (x) { return x * 2; })", "Uint8Array.from({ length: 2, 0: 1, 1: 2 })", "Uint8Array.of(1, 2, 300)", "Uint8Array.from('123')",
  "Uint8Array.from.call(Array, [1, 2])", "Uint8Array.of.call(Array, 1, 2)", "Uint8Array.from.call(Object, [1])", "Uint8Array.of.call(Object, 1)",
  "Array.from.call(Uint8Array, [1, 2, 300])", "Array.of.call(Uint8Array, 1, 2, 300)", "Array.from.call(Uint8Array, { length: 2, 0: 5 })",
  "Array.from.call(Uint8Array, 'ab')", "Array.of.call(Float32Array, 1.1)", "Array.from.call(BigInt64Array, [1n, 2n])", "Array.from.call(BigInt64Array, [1, 2])",
  "(function () { var a = [1, 2, 3]; a.constructor = Uint8Array; return a.map(function (x) { return x * 100; }); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.slice(0, 2); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.splice(0, 2); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.filter(function () { return true; }); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.concat([1]); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.flat(); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: Uint8Array }; return a.flatMap(function (x) { return [x, x]; }); })()",
  "(function () { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: BigInt64Array }; return a.map(function (x) { return x; }); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: Uint16Array }; return t.map(function (x) { return x * 100; }); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: Array }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: BigInt64Array }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: function () { return new Uint8Array(1); } }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: function () { return new Uint8Array(5); } }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: function () { return new Uint8Array(5); } }; return t.map(function (x) { return x; }); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: function () { return new Uint8Array(1); } }; return t.filter(function () { return true; }); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: function () { return {}; } }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = 1; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = undefined; return t.slice().constructor === Uint8Array; })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: undefined }; return t.slice().constructor === Uint8Array; })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: null }; return t.slice().constructor === Uint8Array; })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: 1 }; return t.slice(); })()",
  "(function () { var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: Object }; return t.slice(); })()",
]) E(e);

// ---- Mais métodos com argumentos exóticos, coerção e valores especiais.
for (const e of [
  "[1, 2, 3].join({ toString() { return '#'; } })", "[1, 2, 3].join({ toString() { throw new Error('js'); } })", "[1, 2, 3].join(Symbol())",
  "[Symbol()].join()", "[{ toString() { throw new Error('el'); } }].join()", "[1, 2, 3].join(undefined)", "[1, 2, 3].join(null)", "[1, 2, 3].join(false)", "[1, 2, 3].join(0)",
  "[null, undefined, 1].join()", "[[1, 2], [3, [4, 5]]].join()", "[[1, 2], [3, [4, 5]]].join(';')", "[1n, 2n].join()", "[-0, 0].join()", "[NaN, Infinity].join()",
  "(function () { var a = [1, 2]; a.push(a); return a.join(); })()", "(function () { var a = [1, 2]; a.push(a); return String(a); })()", "(function () { var a = [1, 2]; a.push(a); return a.toString(); })()",
  "(function () { var a = [1]; var b = [a]; a.push(b); return a.join(); })()", "(function () { var a = [1]; var b = [a]; a.push(b); return a.join('-'); })()",
  "(function () { var a = [1]; var b = [a]; a.push(b); return b.join('-'); })()", "(function () { var a = [1, 2]; a.push(a); return a.toLocaleString(); })()",
  "(function () { var a = [1]; a.push({ toString() { return a.join(); } }); return a.join(); })()", "(function () { var a = [1]; a.push({ toString() { return a.join('-'); } }); return a.join(); })()",
  "(function () { var a = [1, 2]; a.push(a); return a.flat(); })().length", "(function () { var a = [1, 2]; a.push(a); return a.flat(Infinity); })()",
  "(function () { var a = [1, 2]; a[0] = a; return a.flat(5).length; })()", "(function () { var a = [1, 2]; a[0] = a; return a.flat(Infinity); })()",
  "(function () { var a = [1, 2]; a[0] = a; return a.flat(1).length; })()", "(function () { var a = [1, 2]; a[0] = a; return a.flatMap(function (x) { return x; }); })().length",
  "(function () { var a = []; var d = a; for (var i = 0; i < 5000; i++) { var n = []; d.push(n); d = n; } return a.flat(Infinity).length; })()",
  "(function () { var a = []; var d = a; for (var i = 0; i < 30000; i++) { var n = []; d.push(n); d = n; } return a.flat(Infinity).length; })()",
  "(function () { var a = []; var d = a; for (var i = 0; i < 2000; i++) { var n = []; d.push(n); d = n; } return a.toString().length; })()",
  "(function () { var a = []; var d = a; for (var i = 0; i < 100000; i++) { var n = []; d.push(n); d = n; } return a.toString(); })()",
  "[1, 2, 3].fill(0, { valueOf() { return 1; } })", "[1, 2, 3].fill(0, 1, { valueOf() { throw new Error('vo'); } })", "[1, 2, 3].fill(0, Symbol())", "[1, 2, 3].fill(0, 1n)",
  "[1, 2, 3].slice(1n)", "[1, 2, 3].at(1n)", "[1, 2, 3].at(Symbol())", "[1, 2, 3].at({ valueOf() { return 1; } })", "[1, 2, 3].at({ valueOf() { throw new Error('vo'); } })",
  "[1, 2, 3].with(1n, 0)", "[1, 2, 3].with(Symbol(), 0)", "[1, 2, 3].with({ valueOf() { return 1; } }, 0)", "[1, 2, 3].indexOf(1, 1n)", "[1, 2, 3].includes(1, Symbol())",
  "[1, 2, 3].splice(1n)", "[1, 2, 3].flat(1n)", "[1, 2, 3].flat(Symbol())", "[1, 2, 3].copyWithin(1n)", "[1, 2, 3].lastIndexOf(1, 1n)",
  "[1, 2, 3].slice({ valueOf() { throw new Error('s'); } })", "[1, 2, 3].slice(0, { valueOf() { throw new Error('e'); } })",
  "(function () { var log = []; [1, 2, 3].slice({ valueOf() { log.push('s'); return 0; } }, { valueOf() { log.push('e'); return 1; } }); return log; })()",
  "(function () { var log = []; [1, 2, 3].splice({ valueOf() { log.push('s'); return 0; } }, { valueOf() { log.push('d'); return 1; } }); return log; })()",
  "(function () { var log = []; [1, 2, 3].fill(0, { valueOf() { log.push('s'); return 0; } }, { valueOf() { log.push('e'); return 1; } }); return log; })()",
  "(function () { var log = []; [1, 2, 3].copyWithin({ valueOf() { log.push('t'); return 0; } }, { valueOf() { log.push('s'); return 1; } }, { valueOf() { log.push('e'); return 2; } }); return log; })()",
  "(function () { var log = []; [1, 2, 3].indexOf(1, { valueOf() { log.push('i'); return 0; } }); return log; })()",
  "(function () { var log = []; [].indexOf(1, { valueOf() { log.push('i'); return 0; } }); return log; })()",
  "(function () { var log = []; [].includes(1, { valueOf() { log.push('i'); return 0; } }); return log; })()",
  "(function () { var log = []; [].lastIndexOf(1, { valueOf() { log.push('i'); return 0; } }); return log; })()",
  "(function () { var log = []; [].at({ valueOf() { log.push('i'); return 0; } }); return log; })()",
  "(function () { var log = []; [].slice({ valueOf() { log.push('s'); return 0; } }); return log; })()",
  "(function () { var log = []; [1].with({ valueOf() { log.push('i'); return 0; } }, { valueOf() { log.push('v'); return 0; } }); return log; })()",
  "(function () { var log = []; [1].toSpliced({ valueOf() { log.push('s'); return 0; } }, { valueOf() { log.push('d'); return 0; } }); return log; })()",
  "(function () { var log = []; [1].flat({ valueOf() { log.push('d'); return 0; } }); return log; })()",
  "(function () { var log = []; [].flat({ valueOf() { log.push('d'); return 0; } }); return log; })()",
  "(function () { var a = [1, 2, 3]; return a.splice(1, { valueOf() { a.length = 0; return 1; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.slice(0, { valueOf() { a.length = 0; return 3; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.fill(9, 0, { valueOf() { a.length = 1; return 3; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.copyWithin(0, 1, { valueOf() { a.length = 1; return 3; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.indexOf(3, { valueOf() { a.length = 1; return 0; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.includes(undefined, { valueOf() { a.length = 1; return 0; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.lastIndexOf(3, { valueOf() { a.length = 1; return 2; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.at({ valueOf() { a.length = 0; return 0; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.with({ valueOf() { a.length = 0; return 2; } }, 'v'); })()",
  "(function () { var a = [1, 2, 3]; return a.toSpliced({ valueOf() { a.length = 0; return 1; } }, 1); })()",
  "(function () { var a = [1, 2, 3]; return a.flat({ valueOf() { a.length = 0; return 1; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.join({ toString() { a.length = 0; return '-'; } }); })()",
  "(function () { var a = [1, 2, 3]; return a.map(function (x, i) { a.length = 0; return x; }); })()",
  "[1, 2, 3].map(function () { return arguments.length; })", "[1, 2, 3].map(function (x, i, arr) { return arr.length; })", "[1, 2, 3].map(function () { return this === undefined; })",
  "[1, 2, 3].map(function () { 'use strict'; return this; }, 5)", "[1, 2, 3].map(function () { return typeof this; }, 5)", "[1, 2, 3].map(function () { return this; }, null).length",
  "[1, 2, 3].map(function () { return typeof this; }, undefined)", "[1, 2, 3].map(() => typeof this, 5)", "[1, 2, 3].forEach.call('ab', function (x) { return x; })",
  "[3, 2, 1].map(Math.sqrt)", "['1', '2', '3'].map(Number)", "['1', '2', '3'].map(parseInt)", "['a', 'b'].map(String.prototype.toUpperCase.call.bind(String.prototype.toUpperCase))",
  "[1, 2, 3].map(class {})", "[1, 2, 3].map(async function () {}).length", "[1, 2, 3].map(function* () {}).length", "[1, 2, 3].map(Symbol)", "[1, 2, 3].map(new Proxy(function (x) { return x; }, {}))",
  "[1, 2, 3].map(new Proxy({}, {}))", "[1, 2, 3].map(Function.prototype)", "[1, 2, 3].map(Function.prototype.call.bind(function () { return this; }))",
  "[1, 2, 3].map(BigInt)", "[1, 2, 3].filter(Boolean)", "[0, 1, '', 'a', null, undefined, NaN].filter(Boolean)", "[1, 2, 3].find(function (x) { return x > 5; })",
  "[1, 2, 3].findIndex(function (x) { return x > 5; })", "[1, 2, 3].findLast(function (x) { return x > 5; })", "[1, 2, 3].findLastIndex(function (x) { return x > 5; })",
  "[1, 2, 3].findLast(function (x, i, a) { return i === 1; })", "[{ a: 1 }, { a: 2 }].findLast(function (o) { return o.a; })", "[].findLast(function () { throw 1; })",
  "[1, 2, 3].findLast(function () { return this; }, 'x')", "[1, 2, 3].findLastIndex(function () { return typeof this; }, 'x')", "[1, 2, 3].findLast(function () { 'use strict'; return typeof this; }, 'x')",
  "[1, 2, 3].reduce(function (a, b) { return a + b; }, undefined)", "[1, 2, 3].reduce(function (a, b) { return a + b; })", "[1, 2, 3].reduceRight(function (a, b) { return a + b; })",
  "[[0, 1], [2, 3]].reduceRight(function (acc, cur) { return acc.concat(cur); }, [])", "[1].reduce(function () { throw 1; })", "[1].reduceRight(function () { throw 1; })",
  "[1, 2, 3].reduce(function (a, b, i, arr) { return [a, b, i, arr.length].join(); }, 'x')", "[1, 2, 3].reduceRight(function (a, b, i, arr) { return [a, b, i].join(); }, 'x')",
  "[1, 2, 3].reduce(undefined)", "[1, 2, 3].reduce(null, 1)", "[].reduce(undefined)", "[].reduceRight(1, 1)",
  "[1, 2, 3].keys().toString()", "[1, 2, 3].entries().toString()", "[1, 2, 3].values().toString()", "String([].values)", "[].values.call('ab').next()",
  "Array.prototype.keys.call('ab').next()", "Array.prototype.entries.call(true).next()", "Array.prototype.entries.call(Symbol()).next()", "Array.prototype.values.call(1n).next()",
  "[...[1, 2, 3].entries()]", "[...[1, , 3].keys()]", "[...[1, , 3].values()]", "[...[1, , 3].entries()]", "[...[, ,].keys()].length", "Object.fromEntries([1, 2].entries())",
  "Array.from([1, 2].entries())", "Array.from([1, 2].keys(), function (x) { return x * 2; })", "new Set([1, 2].values()).size", "new Map([['a', 1]].entries()).get('a')",
  "(function () { var [a, b = 5, ...c] = [1, undefined, 3, 4]; return [a, b, c]; })()", "(function () { var [a, , b] = [1, 2, 3]; return [a, b]; })()",
  "(function () { var [a, b] = [1]; return [a, b]; })()", "(function () { var [a] = []; return a; })()", "(function () { var [...a] = 'abc'; return a; })()",
  "(function () { var [a] = null; })()", "(function () { var [a] = undefined; })()", "(function () { var [a] = {}; })()", "(function () { var [a] = 1; })()", "(function () { var [a] = 1n; })()",
  "(function () { var [a] = Symbol(); })()", "(function () { var [a] = function () {}; })()", "(function () { var [a] = { [Symbol.iterator]: 1 }; })()", "(function () { var [a] = { [Symbol.iterator]() { return 1; } }; })()",
  "(function () { var [a] = { [Symbol.iterator]() { return {}; } }; })()", "(function () { var [a] = { [Symbol.iterator]() { return { next: 1 }; } }; })()",
  "(function () { var [a] = { [Symbol.iterator]() { return { next() { return 1; } }; } }; })()",
  "(function () { var log = []; var [a] = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { log.push('r'); return {}; } }; } }; return log; })()",
  "(function () { var log = []; var [a, b] = { [Symbol.iterator]() { return { next() { return { value: 1, done: true }; }, return() { log.push('r'); return {}; } }; } }; return log; })()",
  "(function () { var log = []; var [a, ...b] = { [Symbol.iterator]() { var i = 0; return { next() { return { value: i, done: i++ > 2 }; }, return() { log.push('r'); return {}; } }; } }; return [log, b]; })()",
  "(function () { var log = []; try { var [a] = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { log.push('r'); return 1; } }; } }; } catch (e) { return [e.name, e.message, log]; } })()",
  "(function () { var log = []; try { var [a = (function () { throw new Error('d'); })()] = { [Symbol.iterator]() { return { next() { return { value: undefined, done: false }; }, return() { log.push('r'); return {}; } }; } }; } catch (e) { return [e.message, log]; } })()",
  "(function () { return Math.max(...[1, 5, 3]); })()", "(function () { return Math.max(...[]); })()", "(function () { return Math.max.apply(null, [,1]); })()", "(function () { return [...'ab', ...[1, , 2]]; })()",
  "(function () { var a = new Array(100000).fill(1); return Math.max(...a); })()", "(function () { var a = new Array(300000).fill(1); return Math.max(...a); })()",
  "(function () { var a = new Array(1000000).fill(1); return Math.max(...a); })()", "(function () { var a = new Array(300000).fill(1); return Math.max.apply(null, a); })()",
  "(function () { var a = new Array(1000000).fill(1); return Math.max.apply(null, a); })()", "(function () { return Math.max.apply(null, { length: 2**32 }); })()",
  "(function () { return Math.max.apply(null, { length: 2**53 }); })()", "(function () { return Math.max.apply(null, { length: -1 }); })()", "(function () { return Math.max.apply(null, { length: 2, 0: 4, 1: 9 }); })()",
  "(function () { return Math.max.apply(null, 'ab'); })()", "(function () { return Math.max.apply(null, 1); })()", "(function () { return Math.max.apply(null, null); })()",
  "(function () { return Reflect.apply(Math.max, null, { length: 2, 0: 4, 1: 9 }); })()", "(function () { return Reflect.apply(Math.max, null, 'ab'); })()", "(function () { return Reflect.construct(Array, { length: 2, 0: 4, 1: 9 }); })()",
  "(function () { return Reflect.construct(Array, [3]).length; })()", "(function () { return new Array(...[3]).length; })()", "(function () { return new Array(...[3, 4]); })()", "(function () { return Array(...['3']); })()",
  "(function () { return Array.apply(null, { length: 3 }); })()", "(function () { return Array.apply(null, [,,]); })()", "(function () { return Array.call(null, 3).length; })()",
  "Array(3).length", "Array('3').length", "Array(3, 4).length", "Array(0).length", "Array(-0).length", "Array(1.0).length", "Array(4294967295).length", "Array(4294967296)", "Array(1e10)",
  "Array(null).length", "Array(undefined).length", "Array(true).length", "Array(1n).length", "Array({ valueOf() { return 3; } }).length", "Array(Infinity)", "Array(-Infinity)", "Array(2.5)", "Array(-1)",
  "new Array(3).length", "new Array(3, 4)", "new Array('a')", "new Array(null)", "new Array(0.5)", "new Array(NaN)", "new Array(2**32 - 1).length", "new Array(2**32 - 2).length",
  "Array.prototype.constructor.name", "Array.prototype.length", "Array.isArray(Array.prototype)", "Object.prototype.toString.call(Array.prototype)", "Array.prototype.concat === [].concat",
  "Object.getOwnPropertyDescriptor(Array.prototype, 'length')", "Object.getOwnPropertyDescriptor([], 'length')", "Object.getOwnPropertyDescriptor([1], 0)", "Object.getOwnPropertyDescriptor(Array.prototype, 'push')",
  "Object.getOwnPropertyDescriptor(Array.prototype, Symbol.iterator)", "Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables)", "Object.getOwnPropertyDescriptor(Array, 'prototype')",
  "Object.getOwnPropertyDescriptor(Array, 'isArray')", "Object.getOwnPropertyDescriptor(Array, Symbol.species).set", "Object.getOwnPropertyDescriptor(Array, Symbol.species).enumerable",
  "Object.getOwnPropertyDescriptor(Array, Symbol.species).configurable", "Object.getOwnPropertyDescriptor(Array.prototype.values, 'name')", "Object.getOwnPropertyDescriptor(Array.prototype.at, 'length')",
  "Object.keys(Array.prototype[Symbol.unscopables]).sort()", "Array.prototype[Symbol.unscopables].at", "Array.prototype[Symbol.unscopables].toSorted", "Array.prototype[Symbol.unscopables].findLast",
  "Array.prototype[Symbol.unscopables].flat", "Array.prototype[Symbol.unscopables].includes", "Array.prototype[Symbol.unscopables].with", "Array.prototype[Symbol.unscopables].toSpliced",
  "Array.prototype[Symbol.unscopables].toReversed", "Array.prototype[Symbol.unscopables].copyWithin", "Array.prototype[Symbol.unscopables].entries", "Array.prototype[Symbol.unscopables].fill",
  "Array.prototype[Symbol.unscopables].find", "Array.prototype[Symbol.unscopables].findIndex", "Array.prototype[Symbol.unscopables].findLastIndex", "Array.prototype[Symbol.unscopables].flatMap",
  "Array.prototype[Symbol.unscopables].keys", "Array.prototype[Symbol.unscopables].values", "Array.prototype[Symbol.unscopables].map", "Array.prototype[Symbol.unscopables].push",
  "(function () { with ([1, 2]) { return typeof at; } })()", "(function () { var keys = 1; with ([1, 2]) { return keys; } })()", "(function () { var values = 2; with ([1, 2]) { return values; } })()",
  "(function () { var map = 3; with ([1, 2]) { return typeof map; } })()", "(function () { var includes = 3; with ([1, 2]) { return includes; } })()", "(function () { var toSorted = 3; with ([1, 2]) { return toSorted; } })()",
]) E(e);

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "array-golden-"));
const file = path.join(dir, "array_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");

const lines = [];
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  const original = '"use strict";\n' + PRELUDE + "\n" + body.replace(/\bR = /g, "globalThis.R = ");
  if (seen.has(original)) continue;
  seen.add(original);
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 4000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  // Resultado que depende do acaso ou do tempo não serve de golden.
  const again = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 4000 });
  const marked2 = (again.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (marked2 !== marked) {
    dropped++;
    process.stderr.write("não determinístico: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("array", lines));
fs.rmSync(dir, { recursive: true, force: true });
