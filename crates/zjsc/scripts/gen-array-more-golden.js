// Gera tests/golden/array_more_bun.tsv: terceira camada de Array, medida no bun 1.4.2. Complementa gen-array-golden.js e
// gen-array-edge-golden.js com grades de argumentos extremos (sort/toSorted contra comparadores inconsistentes, with,
// at, fill, copyWithin, indexOf, includes, flat, slice, toSpliced), array-likes com length acima de 2**32, Array.from,
// Array.of, species, concat com isConcatSpreadable, join cíclico e o construtor com um argumento.
// Programas que já estão em array_bun.tsv ou array_edge_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-array-golden.js.
// Uso: bun scripts/gen-array-more-golden.js > tests/golden/array_more_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const { knownPrograms } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
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
  "function T(f) { try { R = S(f()); } catch (e) { R = e.name + ': ' + e.message; } }",
].join("\n");

const bodies = [];
const P = body => bodies.push(`T(function () { ${body} });`);

// ---- sort e toSorted: arrays x comparadores (estabilidade, inconsistência, valores não numéricos).
const sortInputs = [
  "[3, 1, 2]",
  "[undefined, 3, , 1]",
  "['b', 'a', 10, 9, 1]",
  "[NaN, 1, -0, 0, -1]",
  "[5, 4, 3, 2, 1, 0, 9, 8, 7, 6, 5, 4]",
  "[true, false, null, undefined, 'x']",
  "[[2], [1, 1], [1], []]",
];
const comparators = [
  "undefined",
  "function (a, b) { return a - b; }",
  "function (a, b) { return b - a; }",
  "function () { return 0; }",
  "function () { return 1; }",
  "function () { return -1; }",
  "function () { return NaN; }",
  "function () { return 'x'; }",
  "function (a, b) { return a < b; }",
  "function (a, b) { return { valueOf: function () { return a < b ? -1 : 1; } }; }",
  "function () { throw new RangeError('cmp'); }",
  "null",
  "{}",
  "function (a, b) { return a === undefined ? 1 : b === undefined ? -1 : 0; }",
];
for (const input of sortInputs) {
  for (const cmp of comparators) {
    P(`var a = ${input}; var r = a.sort(${cmp}); return [r === a, a];`);
    P(`var a = ${input}; return [a.toSorted(${cmp}), a];`);
  }
}
// estabilidade em lote
for (const m of [2, 3, 5, 7]) {
  P(`var a = []; for (var i = 0; i < 40; i++) a.push({ k: i % ${m}, i: i }); a.sort(function (x, y) { return x.k - y.k; }); return a.map(function (o) { return o.i; }).join(',');`);
  P(`var a = []; for (var i = 0; i < 40; i++) a.push({ k: i % ${m}, i: i }); return a.toSorted(function (x, y) { return y.k - x.k; }).map(function (o) { return o.i; }).join(',');`);
}
P("var a = []; for (var i = 0; i < 300; i++) a.push({ k: (i * 7) % 5, i: i }); a.sort(function (x, y) { return x.k - y.k; }); for (var j = 1; j < a.length; j++) if (a[j - 1].k === a[j].k && a[j - 1].i > a[j].i) return 'unstable'; return 'stable';");
// comparador que muta o array, que reentra, que conta chamadas
P("var a = [3, 2, 1]; a.sort(function (x, y) { a.length = 0; return x - y; }); return a;");
P("var a = [3, 2, 1, 5, 4]; var n = 0; a.sort(function (x, y) { n++; return x - y; }); return [n > 0, a];");
P("var a = [1, 2, 3]; a.sort(function (x, y) { a.push(0); return y - x; }); return a.length > 3;");
P("var a = [3, 1, 2]; return a.sort(function (x, y) { return [2, 1, 3].sort(function (p, q) { return p - q; })[0] * (x - y); });");
P("var o = { 0: 'b', 1: 'a', length: 2 }; Array.prototype.sort.call(o); return o;");
P("var o = { 0: 'b', 2: 'a', length: 3 }; Array.prototype.sort.call(o); return [o[0], o[1], o[2], 1 in o, 2 in o];");
P("return Array.prototype.sort.call('ba');");
P("return Array.prototype.sort.call(5);");
P("return Array.prototype.toSorted.call({ length: 3, 0: 'z', 1: 'y' });");
P("return Array.prototype.toSorted.call({ length: 2 ** 32 });");
P("return Array.prototype.toSorted.call({ length: 2 ** 53 });");
P("return [1, 2].toSorted.length + ',' + [].sort.length + ',' + [].toReversed.length + ',' + [].with.length;");
P("var a = [1, 2, 3]; Object.freeze(a); return a.sort();");
P("var a = [1, 2, 3]; Object.freeze(a); return a.toSorted(function (x, y) { return y - x; });");
P("var a = [1]; Object.freeze(a); return a.sort();");
P("var a = []; Object.freeze(a); return a.sort();");
P("var a = [2, 1]; Object.defineProperty(a, 0, { writable: false }); return a.sort();");
P("var a = [3, 2, 1]; Object.defineProperty(a, 1, { get: function () { return 9; }, set: function (v) {}, configurable: true }); a.sort(); return a;");

// ---- sort/toSorted com undefined e buracos (herança de protótipo incluída).
P("var a = [undefined, , 3, , undefined, 1]; a.sort(); return [a, 1 in a, 2 in a, 3 in a, 4 in a, 5 in a];");
P("var a = [, , 2, 1]; return a.toSorted();");
P("var a = [, 'b', , 'a']; return a.toSorted().map(function (x) { return x === undefined ? 'U' : x; });");
P("var a = [3, , 1]; a.sort(function () { throw 1; });");
P("var a = [3, , 1]; var n = 0; a.sort(function (x, y) { n++; return x - y; }); return [n, a];");
P("var a = [undefined, undefined]; var n = 0; a.sort(function () { n++; return 0; }); return n;");
P("Array.prototype[1] = 'p'; var a = [3, , 1]; a.sort(); delete Array.prototype[1]; return a;");
P("Array.prototype[1] = 'p'; var r = [3, , 1].toSorted(); delete Array.prototype[1]; return r;");
P("Array.prototype[0] = 'p'; var r = [, 2, 1].toSorted(); delete Array.prototype[0]; return r;");
P("var a = new Array(5); a[3] = 'x'; a.sort(); return [a, Object.keys(a)];");
P("var a = new Array(5); return a.toSorted();");
P("var a = [3, 2, 1]; a.length = 2; return a.sort();");
P("var a = [10, 9, 1, 100, 25, 'a', 'B', '', ' ']; return a.sort();");
P("var a = ['\\uD83D\\uDE00', '\\uFFFF', 'z', '\\u00e9', 'e']; return a.sort();");
P("var a = [{ toString: function () { return 'b'; } }, { toString: function () { return 'a'; } }]; return a.sort().map(String);");
P("var a = [Symbol('x'), 1]; return a.sort();");
P("var a = [1n, 2, 0n, -1]; return a.sort();");
P("var a = [1n, 2, 0n, -1]; return a.sort(function (x, y) { return x < y ? -1 : x > y ? 1 : 0; });");
P("var a = [-0, 0, -0, 0]; a.sort(function (x, y) { return Object.is(x, y) ? 0 : Object.is(x, -0) ? -1 : 1; }); return a.map(function (x) { return Object.is(x, -0) ? '-0' : '0'; });");

// ---- grade de argumentos extremos x métodos.
const extremes = ["undefined", "null", "NaN", "-0", "0", "1", "-1", "1.9", "-1.9", "'2'", "'x'", "Infinity", "-Infinity",
  "2 ** 32", "2 ** 32 + 1", "2 ** 53", "-(2 ** 53)", "2 ** 31", "-(2 ** 31)", "true", "{ valueOf: function () { return 2; } }", "[3]"];
const arr5 = "[1, 2, 3, 4, 5]";
for (const x of extremes) {
  P(`return ${arr5}.at(${x});`);
  P(`return ${arr5}.with(${x}, 9);`);
  P(`return ${arr5}.fill(0, ${x});`);
  P(`return ${arr5}.fill(0, 1, ${x});`);
  P(`return ${arr5}.slice(${x});`);
  P(`return ${arr5}.slice(1, ${x});`);
  P(`return ${arr5}.toSpliced(${x});`);
  P(`return ${arr5}.toSpliced(1, ${x});`);
  P(`return ${arr5}.splice(${x}, 1);`);
  P(`return ${arr5}.indexOf(3, ${x});`);
  P(`return ${arr5}.lastIndexOf(3, ${x});`);
  P(`return ${arr5}.includes(3, ${x});`);
  P(`return ${arr5}.copyWithin(${x}, 3);`);
  P(`return ${arr5}.copyWithin(0, ${x});`);
  P(`return ${arr5}.copyWithin(1, 2, ${x});`);
  P(`return [1, [2, [3, [4]]]].flat(${x});`);
}
// findLast, findLastIndex e toReversed
for (const arr of ["[1, 2, 3, 4]", "[]", "[, 2, , 4]", "[undefined, null, 0, '']", "[NaN, 5]"]) {
  P(`var seen = []; var r = ${arr}.findLast(function (v, i, a) { seen.push(i); return v > 2; }); return [r, seen];`);
  P(`var seen = []; var r = ${arr}.findLastIndex(function (v, i, a) { seen.push(i); return v > 2; }); return [r, seen];`);
  P(`return ${arr}.findLastIndex(function (v) { return v === undefined; });`);
  P(`return ${arr}.findLast(function (v) { return v === undefined; });`);
  P(`return ${arr}.toReversed();`);
  P(`return ${arr}.findLast();`);
  P(`return ${arr}.findLastIndex(null);`);
}
P("return [1, 2, 3].findLast(function (v) { return v < 3; }, 'thisArg');");
P("return [1, 2, 3].findLast(function () { return this; }, 7) === Object(7) ? 'boxed' : 'raw';");
P("'use strict'; return [1].findLast(function () { return typeof this === 'number'; }, 7);");
P("var a = [1, 2, 3]; return a.findLast(function (v, i) { a.length = 1; return i === 1; });");
P("var a = [1, 2, 3]; return a.findLastIndex(function (v, i) { a.pop(); return false; });");
P("var a = [1, 2, 3]; var seen = []; a.findLast(function (v, i) { seen.push(v); a[0] = 'm'; return false; }); return [seen, a];");
P("return Array.prototype.findLast.call({ length: 3, 0: 'a', 1: 'b', 2: 'c' }, function (v) { return v < 'c'; });");
P("return Array.prototype.findLastIndex.call({ length: -5 }, function () { return true; });");
P("return Array.prototype.findLast.call({ length: 2 ** 53 + 10, [2 ** 53 - 2]: 'last' }, function () { return true; });");
P("return Array.prototype.findLastIndex.call({ length: 2 ** 53, [2 ** 53 - 2]: 'q' }, function () { return true; });");
P("return Array.prototype.toReversed.call({ length: 3, 0: 'a', 2: 'c' });");
P("return Array.prototype.toReversed.call({ length: 2 ** 32 });");
P("return Array.prototype.toReversed.call('abc');");
P("var a = [1, , 3]; var r = a.toReversed(); return [r, 1 in r, a];");
P("Array.prototype[1] = 'p'; var r = [1, , 3].toReversed(); delete Array.prototype[1]; return r;");
P("var a = [1, 2]; a.constructor = { [Symbol.species]: function () { throw 1; } }; return a.toReversed();");

// ---- at, with, toSpliced em array-likes e subclasses.
P("return Array.prototype.at.call('abc', -1);");
P("return Array.prototype.at.call({ length: 3, 2: 'x' }, -1);");
P("return Array.prototype.at.call({ length: 2 ** 53 + 5, [2 ** 53 - 2]: 'e' }, -1);");
P("return Array.prototype.at.call({ length: 2 ** 32 + 1, 4294967296: 'big' }, 4294967296);");
P("return Array.prototype.at.call({ length: Infinity, 9007199254740990: 'z' }, -2);");
P("return Array.prototype.at.call(null, 0);");
P("return Array.prototype.at.call(undefined);");
P("return [1, 2, 3].at();");
P("return [1, 2, 3].at(-4);");
P("return [1, 2, 3].at(3);");
P("return [,'a'].at(0);");
P("Array.prototype[0] = 'p'; var r = [, 'a'].at(0); delete Array.prototype[0]; return r;");
P("return [1, 2, 3].with(-4, 0);");
P("return [1, 2, 3].with(3, 0);");
P("return [1, 2, 3].with(-3, 0);");
P("return [1, , 3].with(0, 0);");
P("var r = [1, , 3].with(0, 0); return 1 in r;");
P("return [1, 2, 3].with(1);");
P("return [1, 2, 3].with();");
P("return Array.prototype.with.call({ length: 2, 0: 'a', 1: 'b' }, 1, 'z');");
P("return Array.prototype.with.call({ length: 2 ** 32 }, 0, 0);");
P("return Array.prototype.with.call({ length: 2 ** 32 - 1 }, 0, 0);");
P("var a = [1, 2, 3]; var i = { valueOf: function () { a.length = 0; return 1; } }; return a.with(i, 'z');");
P("class A extends Array {} var r = A.from([3, 1, 2]); return [r.toSorted() instanceof A, r.toReversed() instanceof A, r.with(0, 1) instanceof A, r.toSpliced(0, 1) instanceof A];");
P("class A extends Array {} var r = A.from([3, 1, 2]); return [r.slice() instanceof A, r.map(function (x) { return x; }) instanceof A, r.flat() instanceof A, r.filter(Boolean) instanceof A, r.concat() instanceof A];");
P("return [1, 2, 3].toSpliced(1, 1, 'a', 'b');");
P("return [1, 2, 3].toSpliced(-1, 5, 'x');");
P("return [1, 2, 3].toSpliced();");
P("return [1, 2, 3].toSpliced(undefined);");
P("return [1, 2, 3].toSpliced(1, undefined);");
P("return [1, , 3].toSpliced(0, 0);");
P("return Array.prototype.toSpliced.call({ length: 2 ** 32 }, 0, 0);");
P("return Array.prototype.toSpliced.call({ length: 2 ** 53 - 1 }, 0, 0, 1);");
P("return Array.prototype.toSpliced.call({ length: 2 ** 53 - 1 }, 0, 1, 1);");

// ---- includes e indexOf com NaN, -0, tipos exóticos.
const needles = ["NaN", "-0", "0", "undefined", "null", "'0'", "1n", "0n", "Symbol.iterator", "{}", "[]", "Infinity"];
const hays = ["[NaN]", "[-0]", "[0]", "[, 1]", "[undefined]", "[null]", "['0']", "[1n]", "[0n]", "[{}]", "[Infinity, -Infinity]"];
for (const n of needles) for (const h of hays) {
  P(`return [${h}.includes(${n}), ${h}.indexOf(${n}), ${h}.lastIndexOf(${n})];`);
}
P("var o = {}; return [[o].includes(o), [o].indexOf(o), [{}].includes(o)];");
P("var s = Symbol('q'); return [[s].includes(s), [s].indexOf(s), [Symbol('q')].includes(s)];");
P("return [[,].includes(undefined), [,].indexOf(undefined), new Array(3).includes(undefined), new Array(3).indexOf(undefined)];");
P("return [[1, 2, 3].includes(1, -Infinity), [1, 2, 3].includes(3, -1), [1, 2, 3].includes(3, -0.5), [1, 2, 3].includes(1, Infinity)];");
P("return [[1, 2, 3].indexOf(1, -4), [1, 2, 3].indexOf(1, -3), [1, 2, 3].indexOf(1, -2), [1, 2, 3].indexOf(3, -1), [1, 2, 3].indexOf(3, -0)];");
P("return [[1, 2, 3, 1].lastIndexOf(1, -1), [1, 2, 3, 1].lastIndexOf(1, -2), [1, 2, 3, 1].lastIndexOf(1, -4), [1, 2, 3, 1].lastIndexOf(1, -5), [1, 2, 3, 1].lastIndexOf(1, 0), [1, 2, 3, 1].lastIndexOf(1)];");
P("return [[1, 2, 3, 1].lastIndexOf(1, undefined), [1, 2, 3, 1].lastIndexOf(1, null), [1, 2, 3, 1].lastIndexOf(1, NaN), [1, 2, 3].lastIndexOf(3, -0)];");
P("return [[1, 2].indexOf(2, { valueOf: function () { return -1; } }), [1, 2].includes(1, { valueOf: function () { return 1; } })];");
P("return [1].indexOf(1, { valueOf: function () { throw new TypeError('v'); } });");
P("return [1].includes(1, Symbol());");
P("return [1].includes(1, 1n);");
P("var a = [1, 2, 3]; return a.indexOf(3, { valueOf: function () { a.length = 0; return 0; } });");
P("var a = [1, 2, 3]; return a.includes(undefined, { valueOf: function () { a.length = 0; a.length = 3; return 0; } });");
P("return Array.prototype.includes.call({ length: 3, 0: NaN }, NaN);");
P("return Array.prototype.indexOf.call({ length: 3, 0: NaN }, NaN);");
P("return Array.prototype.includes.call('abc', 'b');");
P("return Array.prototype.indexOf.call('abc', 'b', -1);");
P("return Array.prototype.lastIndexOf.call('abca', 'a', -2);");
P("Array.prototype[1] = 'p'; var r = [[, , ].includes('p'), [, , ].indexOf('p')]; delete Array.prototype[1]; return r;");
P("var a = [0]; a[2 ** 32 - 2] = 'e'; return [a.indexOf('e'), a.lastIndexOf('e'), a.includes('e', 4294967290), a.indexOf('e', 4294967294), a.length];");
P("var p = new Proxy([1, 2, 3], { has: function (t, k) { return k !== '1'; } }); return [Array.prototype.indexOf.call(p, 2), Array.prototype.includes.call(p, 2)];");
P("var log = []; var p = new Proxy([1, 2], { get: function (t, k) { log.push(String(k)); return t[k]; } }); p.includes(9); return log;");
P("var log = []; var p = new Proxy([1, 2], { get: function (t, k) { log.push(String(k)); return t[k]; }, has: function (t, k) { log.push('has:' + String(k)); return k in t; } }); p.indexOf(9); return log;");

// ---- array-likes com length acima de 2**32 (intervalos estreitos para não rodar eternamente).
const big = "{ length: 2 ** 32 + 5, 4294967296: 'a', 4294967297: 'b', 4294967299: 'd' }";
P(`return Array.prototype.indexOf.call(${big}, 'b', 4294967290);`);
P(`return Array.prototype.indexOf.call(${big}, 'a', 4294967296);`);
P(`return Array.prototype.lastIndexOf.call(${big}, 'd');`);
P(`return Array.prototype.lastIndexOf.call(${big}, 'a', 4294967296);`);
P(`return Array.prototype.includes.call(${big}, 'd', 4294967295);`);
P(`return Array.prototype.includes.call(${big}, undefined, 4294967300);`);
P(`return Array.prototype.slice.call(${big}, 4294967296, 4294967300);`);
P(`return Array.prototype.slice.call(${big}, 4294967296);`);
P(`return Array.prototype.slice.call(${big}, 2 ** 32 + 4, 2 ** 32 + 100);`);
P(`return Array.prototype.slice.call(${big}, -1);`);
P(`return Array.prototype.fill.call({ length: 2 ** 32 + 2 }, 'x', 2 ** 32, 2 ** 32 + 2);`);
P(`var o = { length: 2 ** 32 + 2 }; Array.prototype.fill.call(o, 'x', 2 ** 32, 2 ** 32 + 2); return [o[4294967296], o[4294967297], o[4294967295], o.length];`);
P(`var o = { length: 2 ** 40 }; Array.prototype.fill.call(o, 'x', 2 ** 40 - 1); return [o[2 ** 40 - 1], o[2 ** 40 - 2]];`);
P(`var o = { length: 2 ** 53 - 1 }; Array.prototype.fill.call(o, 7, 2 ** 53 - 3); return [o[2 ** 53 - 2], o[2 ** 53 - 3], o[2 ** 53 - 4]];`);
P(`var o = { length: 2 ** 32 + 3, 4294967296: 'a', 4294967297: 'b' }; Array.prototype.copyWithin.call(o, 4294967298, 4294967296, 4294967298); return [o[4294967298], o[4294967299], o[4294967296]];`);
P(`var o = { length: 2 ** 53 - 1, 9007199254740989: 'a' }; Array.prototype.copyWithin.call(o, 9007199254740990, 9007199254740989, 9007199254740990); return [o[9007199254740989], o[9007199254740990]];`);
P(`var o = { length: 2 ** 32 + 3, 4294967296: 'a', 4294967297: 'b', 4294967298: 'c' }; Array.prototype.copyWithin.call(o, 4294967297, 4294967296); return [o[4294967296], o[4294967297], o[4294967298]];`);
P(`return Array.prototype.pop.call({ length: 2 ** 32 + 2, 4294967297: 'p' });`);
P(`var o = { length: 2 ** 32 + 2, 4294967297: 'p' }; Array.prototype.pop.call(o); return [o.length, 4294967297 in o];`);
P(`var o = { length: 2 ** 53 + 9, 9007199254740990: 'z' }; var r = Array.prototype.pop.call(o); return [r, o.length];`);
P(`var o = { length: 2 ** 53 - 1 }; return Array.prototype.push.call(o, 1);`);
P(`var o = { length: 2 ** 53 - 2 }; var r = Array.prototype.push.call(o, 1); return [r, o.length, o[2 ** 53 - 2]];`);
P(`var o = { length: 2 ** 53 - 2 }; return Array.prototype.push.call(o, 1, 2);`);
P(`var o = { length: 2 ** 53 - 2 }; try { Array.prototype.push.call(o, 1, 2); } catch (e) { return [e.name, o.length, o[2 ** 53 - 2]]; }`);
P(`var o = { length: 2 ** 32 }; var r = Array.prototype.push.call(o, 'x'); return [r, o.length, o[4294967296]];`);
P(`var o = { length: 2 ** 53 - 1 }; return Array.prototype.unshift.call(o, 1);`);
P(`var o = { length: 2 ** 53 - 1 }; return Array.prototype.unshift.call(o);`);
P(`var o = { length: 2 ** 53 - 1 }; return Array.prototype.splice.call(o, 0, 0, 1);`);
P(`var o = { length: 2 ** 53 - 1 }; return Array.prototype.concat.call([], o).length;`);
P(`var o = { length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true }; return [].concat(o);`);
P(`var o = { length: 2 ** 32, [Symbol.isConcatSpreadable]: true }; return [].concat(o);`);
P(`var o = { length: 2 ** 32 - 1, [Symbol.isConcatSpreadable]: true }; return [].concat(o).length;`);
P(`var o = { length: 2 ** 32 + 1, [Symbol.isConcatSpreadable]: true, 4294967296: 1 }; return [1].concat(o);`);
P(`var o = { length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true }; return [1].concat(o);`);
P(`var o = { length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true }; return [].concat(o, [1]);`);
P(`return Array.prototype.reverse.call({ length: 2 ** 32 + 1, 0: 'x' }) === undefined;`);
P(`return Array.prototype.shift.call({ length: -1 });`);
P(`var o = { length: -1 }; Array.prototype.shift.call(o); return o.length;`);
P(`var o = { length: 'abc' }; Array.prototype.pop.call(o); return o.length;`);
P(`var o = { length: 2.9 }; Array.prototype.pop.call(o); return o.length;`);
P(`var o = { length: -0.5 }; return [Array.prototype.pop.call(o), o.length];`);
P(`var o = { length: 2 ** 53 }; var r = Array.prototype.pop.call(o); return [r, o.length];`);
P(`var o = {}; Array.prototype.push.call(o); return o.length;`);
P(`var o = { length: '3' }; Array.prototype.push.call(o, 'a'); return [o.length, o[3]];`);
P(`return Array.prototype.indexOf.call({ length: Infinity, 9007199254740990: 'e' }, 'e', 9007199254740989);`);
P(`return Array.prototype.lastIndexOf.call({ length: Infinity, 9007199254740990: 'e' }, 'e');`);
P(`return Array.prototype.includes.call({ length: 2 ** 53, 9007199254740990: 'e' }, 'e', 9007199254740990);`);
P(`return Array.prototype.join.call({ length: 3, 0: 'a', 2: 'c' }, '-');`);
P(`return Array.prototype.join.call({ length: 2 ** 32 }, '');`);
P(`return Array.prototype.keys.call({ length: 2 ** 53 + 1 }).next();`);
P(`var it = Array.prototype.keys.call({ length: 2 ** 32 + 1 }); for (var i = 0; i < 3; i++) it.next(); return it.next();`);
P(`return [].lastIndexOf.call({ length: 2 ** 32 + 2, 4294967297: 'q' }, 'q', -1);`);
P(`return Array.prototype.every.call({ length: 2 ** 32 + 1, 4294967296: 1 }, function (v, i) { return i < 4294967296; });`);
P(`return Array.prototype.find.call({ length: 2 ** 33, 8589934591: 'z' }, function (v, i) { return v === 'z'; }) === undefined ? 'skip' : 'found';`);

// ---- flat e flatMap.
P("return [1, [2, [3, [4, [5]]]]].flat(Infinity);");
P("return [1, [2, [3, [4, [5]]]]].flat();");
P("return [1, [2, [3]]].flat(-1);");
P("return [1, [2, [3]]].flat(0.9);");
P("return [1, [2, [3]]].flat(1.9);");
P("return [1, , [2, , 3], , ].flat();");
P("var r = [1, , [2, , 3]].flat(); return [r.length, 1 in r, 3 in r];");
P("return [[], [[]], [[], []]].flat(Infinity);");
P("return [[1], 'ab', { length: 1, 0: 'x' }, new String('s')].flat();");
P("var a = []; a[0] = a; return a.flat(2).length;");
P("var a = [1]; a.push(a); return a.flat(3).length;");
P("class A extends Array {} return [A.from([[1], [2]]).flat() instanceof A, A.from([1]).flatMap(function (x) { return [x, x]; }) instanceof A];");
P("var a = [1]; a.constructor = { [Symbol.species]: function (n) { return { length: 0, n: n }; } }; var r = a.flat(); return [Array.isArray(r), r.n, r.length, r[0]];");
P("var a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return { length: 0, n: n }; } }; var r = a.flatMap(function (x) { return [x, x]; }); return [r.n, r[0], r[3], r.length];");
P("var a = [1, 2]; a.constructor = { [Symbol.species]: Object.freeze([]).constructor }; return a.flat();");
P("return [1, 2].flatMap(function (x) { return x; });");
P("return [1, 2].flatMap(function (x) { return [[x]]; });");
P("return [1, 2].flatMap(function (x, i, a) { return [x, i, a.length, this.t]; }, { t: 't' });");
P("return [1, 2].flatMap();");
P("return [1, 2].flatMap({});");
P("return [, 1].flatMap(function (x) { return [x]; });");
P("return [1, 2].flatMap(function (x) { return { length: 2, 0: 'a', 1: 'b' }; });");
P("return [1, 2].flatMap(function (x) { return 'ab'; });");
P("return [1].flatMap(function (x) { var r = [x]; r[Symbol.isConcatSpreadable] = false; return r; });");
P("return [1].flatMap(function () { return new Proxy([7, 8], {}); });");
P("var a = [1, [2]]; var r = a.flat(); r[1] = 'm'; return a;");
P("return Array.prototype.flat.call({ length: 2, 0: [1], 1: [2] });");
P("return Array.prototype.flatMap.call('ab', function (c) { return [c, c]; });");
P("return Array.prototype.flat.call({ length: 2 ** 53 });");
P("var depth = []; var a = [1]; for (var i = 0; i < 50; i++) a = [a]; return a.flat(Infinity);");
P("var depth = 0; var a = [1]; for (var i = 0; i < 5000; i++) a = [a]; try { return a.flat(Infinity).length; } catch (e) { return e.name; }");
P("return [1, [2, [3]]].flat({ valueOf: function () { return 2; } });");
P("return [1, [2, [3]]].flat('1');");
P("return [1, [2, [3]]].flat(null);");
P("return [1, [2, [3]]].flat(Symbol());");

// ---- fill e copyWithin: casos de combinação.
for (const [t, s, e] of [[0, 3, 5], [3, 0, 2], [1, 0, 4], [-2, 0, 2], [0, -2], [2, 0, -1], [4, 0, 5], [0, 4, 2], [1, 1, 1], [-1, -1, -1], [10, 0, 5], [0, 10, 5], [-10, -10, -10]]) {
  P(`return ${arr5}.copyWithin(${[t, s, e].filter(v => v !== undefined).join(", ")});`);
}
P("return [1, 2, 3, 4, 5].copyWithin(0, 1, undefined);");
P("return [1, 2, 3, 4, 5].copyWithin(0, 1, null);");
P("return [1, , 3, , 5].copyWithin(0, 1);");
P("var r = [1, , 3, , 5].copyWithin(0, 1); return [r, 0 in r, 1 in r, 2 in r, 3 in r];");
P("var r = [1, 2, 3, 4, 5].copyWithin(1, 0); return r;");
P("var r = [, 2, 3].copyWithin(1, 0); return [r, 1 in r];");
P("Array.prototype[1] = 'p'; var r = [1, , 3].copyWithin(0, 1); delete Array.prototype[1]; return r;");
P("return [].copyWithin();");
P("return [1].copyWithin();");
P("return [1, 2, 3].copyWithin(Symbol());");
P("return Array.prototype.copyWithin.call({ length: 3, 0: 'a', 1: 'b', 2: 'c' }, 0, 1);");
P("return Array.prototype.copyWithin.call('abc', 0, 1);");
P("var o = Object.freeze([1, 2, 3]); return o.copyWithin(0, 1);");
P("var o = Object.freeze([1, 2, 3]); return o.copyWithin(0, 0);");
P("var o = Object.freeze([1, 2, 3]); return o.copyWithin(0, 3);");
P("var o = Object.freeze([1, 2, 3]); return o.fill(1);");
P("var o = Object.freeze([1, 2, 3]); return o.fill(1, 3);");
P("var o = Object.freeze([]); return o.fill(1);");
P("var a = [1, 2, 3]; return a.fill({ x: 1 }).map(function (v, i, arr) { return v === arr[0]; });");
P("var a = new Array(3); a.fill(undefined); return [a, 0 in a];");
P("var a = [1, 2, 3]; return a.fill(0, { valueOf: function () { a.length = 1; return 0; } });");
P("var a = [1, 2, 3]; return a.fill();");
P("var a = Array(3); a.fill(); return Object.keys(a);");
P("return Array.prototype.fill.call({ length: 3 }, 'x');");
P("return Array.prototype.fill.call({ length: 3 }, 'x', 1, 2);");
P("return Array.prototype.fill.call('ab', 'x');");
P("return Array.prototype.fill.call(1, 'x');");
P("return Array.prototype.fill.call({ get length() { return 2; }, set 0(v) { this.z = v; } }, 'k');");
P("var a = [1, 2, 3]; Object.defineProperty(a, 1, { value: 0, writable: false }); return a.fill(9);");
P("var a = [1, 2, 3]; Object.defineProperty(a, 1, { value: 0, writable: false }); try { a.fill(9); } catch (e) { return [e.name, a]; }");
P("var a = [1, 2, 3]; Object.preventExtensions(a); return a.fill(9);");
P("var a = []; a.length = 3; Object.preventExtensions(a); return a.fill(9);");

// ---- Array.from.
P("return Array.from([1, 2, 3]);");
P("return Array.from('a\\uD83D\\uDE00b');");
P("return Array.from({ length: 3 });");
P("return Array.from({ length: 3 }, function (v, i) { return i * 2; });");
P("return Array.from({ length: 2, 0: 'a', 1: 'b' }, function (v, i) { return v + i + this.s; }, { s: '!' });");
P("return Array.from(new Set([1, 1, 2]), function (x) { return x * 3; });");
P("return Array.from(new Map([[1, 2]]));");
P("return Array.from([1, , 3]);");
P("var r = Array.from([1, , 3]); return 1 in r;");
P("return Array.from({ length: 2 ** 32 });");
P("return Array.from({ length: 2 ** 32 - 1 });");
P("return Array.from({ length: -1 });");
P("return Array.from({ length: 'abc' });");
P("return Array.from({ length: '2' });");
P("return Array.from({ length: 2.9 });");
P("return Array.from({ length: Infinity });");
P("return Array.from(5);");
P("return Array.from(true);");
P("return Array.from(null);");
P("return Array.from(undefined);");
P("return Array.from();");
P("return Array.from([1], null);");
P("return Array.from([1], {});");
P("return Array.from([1], undefined);");
P("return Array.from({ length: 1 }, 5);");
P("return Array.from(function (a, b) {});");
P("return Array.from(function () {}.bind());");
P("return Array.from(new String('xy'));");
P("var o = {}; o[Symbol.iterator] = function () { var i = 0; return { next: function () { return i < 3 ? { value: i++, done: false } : { done: true }; } }; }; return Array.from(o);");
P("var o = {}; o[Symbol.iterator] = function () { var i = 0; return { next: function () { return i < 3 ? { value: i++, done: false } : { done: true }; }, return: function () { throw 1; } }; }; return Array.from(o, function (x) { if (x === 1) throw new Error('m'); return x; });");
P("var log = []; var o = {}; o[Symbol.iterator] = function () { var i = 0; return { next: function () { return { value: i++, done: false }; }, return: function () { log.push('ret'); return {}; } }; }; try { Array.from(o, function (x) { if (x === 2) throw new Error('stop'); return x; }); } catch (e) { return [e.message, log]; }");
P("var log = []; var o = {}; o[Symbol.iterator] = function () { var i = 0; return { next: function () { return { value: i++, done: false }; }, return: function () { log.push('ret'); throw new Error('ret'); } }; }; try { Array.from(o, function (x) { if (x === 1) throw new Error('map'); return x; }); } catch (e) { return [e.message, log]; }");
P("var o = {}; o[Symbol.iterator] = function () { return { next: function () { return 5; } }; }; return Array.from(o);");
P("var o = {}; o[Symbol.iterator] = function () { return 5; }; return Array.from(o);");
P("var o = {}; o[Symbol.iterator] = 5; return Array.from(o);");
P("var o = { length: 2, 0: 'a', 1: 'b' }; o[Symbol.iterator] = null; return Array.from(o);");
P("var o = { length: 2, 0: 'a', 1: 'b' }; o[Symbol.iterator] = undefined; return Array.from(o);");
P("var o = { length: 1, 0: 'a' }; o[Symbol.iterator] = {}; return Array.from(o);");
P("function C() { this.made = arguments.length; } var r = Array.from.call(C, [1, 2]); return [r instanceof C, r.made, r.length, r[1]];");
P("function C(n) { this.args = [].slice.call(arguments); } var r = Array.from.call(C, { length: 2, 0: 'a', 1: 'b' }); return [r.args, r.length, r[0]];");
P("function C() { return Object.freeze({}); } return Array.from.call(C, [1]);");
P("var r = Array.from.call({}, [1]); return r;");
P("var r = Array.from.call(undefined, [1, 2]); return r;");
P("var r = Array.from.call(Object, [1]); return Object.prototype.toString.call(r);");
P("var r = Array.from.call(function () {}.bind(), [1]); return r.length;");
P("var seen = []; Array.from({ length: 2, 0: 'a', 1: 'b' }, function () { seen.push(arguments.length); }); return seen;");
P("var seen = []; Array.from([5], function () { seen.push(arguments.length); }); return seen;");
P("var calls = 0; var o = { get length() { calls++; return 1; }, 0: 'x' }; Array.from(o); return calls;");
P("var log = []; var p = new Proxy({ length: 2, 0: 'a', 1: 'b' }, { get: function (t, k) { log.push(String(k)); return t[k]; } }); Array.from(p); return log;");
P("return Array.from({ length: 1, 0: 1 }, function (x) { return [x]; });");
P("var r = Array.from([1, 2, 3], function (x) { return undefined; }); return r;");
P("var a = [1, 2, 3]; var r = Array.from(a, function (x, i) { if (i === 0) a.push(4); return x; }); return r;");
P("var r = Array.from('abc', function (c, i) { return c + i; }); return r;");
P("var r = Array.from(new Uint8Array([1, 2, 300])); return r;");
P("return Array.from(function* () { yield 1; yield 2; }());");
P("return Array.from(Array(3).keys());");
P("return Array.from([1, 2, 3].entries());");
P("return Array.from(arguments_test());\nfunction arguments_test() { return arguments; }");
P("return (function () { return Array.from(arguments); })(1, 2, 3);");
P("return Array.from.length + ',' + Array.from.name + ',' + Array.of.length + ',' + Array.of.name;");

// ---- Array.of e o construtor.
P("return Array.of();");
P("return Array.of(7);");
P("return Array.of(undefined);");
P("return Array.of(1, , 3);");
P("return Array.of(1, 2, 3);");
P("function C() { this.n = arguments.length; this.a = arguments[0]; } var r = Array.of.call(C, 'x', 'y'); return [r instanceof C, r.n, r.a, r.length, r[0], r[1]];");
P("function C() {} var r = Array.of.call(C, 1); return [r.length, r[0], Object.keys(r)];");
P("return Array.of.call(Object, 1, 2);");
P("return Array.of.call(undefined, 1);");
P("return Array.of.call(function () { return Object.freeze({}); }, 1);");
P("var r = Array.of.call(function () { return Object.preventExtensions({}); }, 1);");
P("var r = Array.of.call(function () { return { set length(v) { throw new RangeError('len'); } }; }, 1);");
P("class A extends Array {} var r = A.of(1, 2); return [r instanceof A, r.length, Array.isArray(r)];");
P("class A extends Array { constructor(n) { super(); this.mark = n; } } var r = A.of(1, 2, 3); return [r.mark, r.length];");
P("return new Array(5);");
P("return new Array(0);");
P("return new Array(-0);");
P("return new Array(-1);");
P("return new Array(1.5);");
P("return new Array(NaN);");
P("return new Array(Infinity);");
P("return new Array(2 ** 32);");
P("return new Array(2 ** 32 - 1).length;");
P("return new Array(2 ** 32 - 2).length;");
P("return Array(2 ** 31).length;");
P("return new Array('5');");
P("return new Array('x');");
P("return new Array(null);");
P("return new Array(undefined);");
P("return new Array(true);");
P("return new Array(5n);");
P("return new Array(Symbol.iterator).length;");
P("return new Array({ valueOf: function () { return 3; } });");
P("return new Array([3]);");
P("return new Array(3, 4);");
P("return new Array(undefined, undefined);");
P("return Array(3);");
P("return Array(3, 4);");
P("return Array('3');");
P("var a = new Array(3); return [Object.keys(a), 0 in a, a.length];");
P("var a = new Array(1e3); return a.length;");
P("var a = new Array(2 ** 32 - 1); a[2 ** 32 - 2] = 1; return a.length;");
P("var a = []; a[2 ** 32 - 1] = 'x'; return [a.length, a[2 ** 32 - 1], Object.keys(a)];");
P("var a = []; a[2 ** 32 - 2] = 'x'; return a.length;");
P("var a = []; a[2 ** 32] = 'x'; return [a.length, Object.keys(a)];");
P("var a = [1, 2, 3]; a.length = 2 ** 32 - 1; return a.length;");
P("var a = [1, 2, 3]; a.length = 2 ** 32;");
P("var a = [1, 2, 3]; a.length = -1;");
P("var a = [1, 2, 3]; a.length = 1.5;");
P("var a = [1, 2, 3]; a.length = '2'; return a;");
P("var a = [1, 2, 3]; a.length = { valueOf: function () { return 1; } }; return a;");
P("var a = [1, 2, 3]; a.length = NaN;");
P("var a = [1, 2, 3]; a.length = null; return a;");
P("var a = [1, 2, 3]; a.length = true; return a;");
P("var a = [1, 2, 3]; a.length = [2]; return a;");
P("var a = [1, 2, 3]; a.length = [1, 2];");
P("var a = [1, 2, 3]; a.length = 1n;");
P("var a = [1, 2, 3]; a.length = Symbol();");
P("var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a.length = 1; return a.length;");
P("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a.length = 1;");
P("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a.push(4);");
P("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); a[3] = 4;");
P("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a.pop();");
P("var a = [1, 2, 3]; Object.defineProperty(a, 1, { configurable: false }); a.length = 0; return a;");
P("'use strict'; var a = [1, 2, 3]; Object.defineProperty(a, 1, { configurable: false }); a.length = 0;");
P("var a = [1, 2, 3]; Object.defineProperty(a, 1, { configurable: false }); try { 'use strict'; a.length = 0; } catch (e) { return e.name; } return [a, a.length];");

// ---- species.
P("class A extends Array {} var r = new A(1, 2, 3).map(function (x) { return x; }); return [r instanceof A, r.length];");
P("class A extends Array { static get [Symbol.species]() { return Array; } } var r = new A(1, 2, 3).filter(Boolean); return [r instanceof A, r.constructor === Array];");
P("class A extends Array { static get [Symbol.species]() { return null; } } return new A(1, 2).slice() instanceof A;");
P("class A extends Array { static get [Symbol.species]() { return undefined; } } return Array.isArray(new A(1, 2).slice());");
P("class A extends Array { static get [Symbol.species]() { return 5; } } return new A(1, 2).slice();");
P("class A extends Array { static get [Symbol.species]() { return {}; } } return new A(1, 2).slice();");
P("class A extends Array { static get [Symbol.species]() { return function () { return { length: 7 }; }; } } var r = new A(1, 2).slice(); return r;");
P("var a = [1, 2, 3]; a.constructor = undefined; return Array.isArray(a.slice());");
P("var a = [1, 2, 3]; a.constructor = null; return a.slice();");
P("var a = [1, 2, 3]; a.constructor = 5; return a.slice();");
P("var a = [1, 2, 3]; a.constructor = {}; return Array.isArray(a.slice());");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: null }; return Array.isArray(a.slice());");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: 7 }; return a.slice();");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function () { return Object.freeze([]); } }; return a.slice();");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.length = 0; this.n = n; } }; var r = a.slice(1); return [r.n, r.length, r[0], r[1]];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.map(function (x) { return x * 2; }); return [r.n, r[0], r[2], r.length];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.filter(Boolean); return [r.n, r[2], r.length];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.splice(1, 1); return [r.n, r[0], r.length];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.concat([4]); return [r.n, r[3], r.length];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.toSorted(); return [Array.isArray(r), r.n];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.with(0, 0); return [Array.isArray(r), r.n];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.toReversed(); return [Array.isArray(r), r.n];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.toSpliced(0, 1); return [Array.isArray(r), r.n];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { this.n = n; } }; var r = a.flat(); return [Array.isArray(r), r.n];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function () { throw new RangeError('sp'); } }; return a.reverse().length;");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function () { throw new RangeError('sp'); } }; return a.sort().length;");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function () { throw new RangeError('sp'); } }; return a.fill(0).length;");
P("var a = [1, 2, 3]; a.constructor = { get [Symbol.species]() { throw new SyntaxError('get'); } }; return a.length;");
P("var a = [1, 2, 3]; Object.defineProperty(a, 'constructor', { get: function () { throw new EvalError('c'); } }); return a.slice();");
P("var a = [1, 2, 3]; Object.defineProperty(a, 'constructor', { get: function () { throw new EvalError('c'); } }); return a.map(function (x) { return x; });");
P("var a = [1, 2, 3]; Object.defineProperty(a, 'constructor', { get: function () { throw new EvalError('c'); } }); return a.toSorted();");
P("var a = []; Object.defineProperty(a, 'constructor', { get: function () { throw new EvalError('c'); } }); return a.slice();");
P("var other = Function('return Array')(); var a = new other(1, 2, 3); a.constructor = other; return a.slice().constructor === Array;");
P("var a = [1, 2, 3]; a.constructor = Array; return a.slice(0, 1);");
P("class A extends Array {} var a = new A(3); a.constructor = Array; return a.slice() instanceof A;");
P("return Array[Symbol.species] === Array;");
P("return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.name;");
P("return Object.getOwnPropertyDescriptor(Array, Symbol.species).set;");
P("return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.call(5);");
P("var o = {}; return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.call(o) === o;");
P("return Array.prototype.slice.call({ length: 2, 0: 'a', 1: 'b', constructor: Array }).length;");
P("var o = { length: 2, 0: 'a', 1: 'b', constructor: { [Symbol.species]: function (n) { this.n = n; } } }; return Array.isArray(Array.prototype.slice.call(o));");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return { length: 0 }; } }; var r = a.slice(); return [r.length, r[0], Array.isArray(r)];");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { var r = []; Object.defineProperty(r, 0, { value: 'ro' }); return r; } }; return a.slice();");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return new Proxy([], { defineProperty: function () { return false; } }); } }; return a.map(function (x) { return x; });");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return new Proxy([], {}); } }; var r = a.map(function (x) { return x + 1; }); return r;");
P("var a = [1, 2, 3]; a.constructor = { [Symbol.species]: function (n) { return new Proxy([], {}); } }; return Array.isArray(a.slice());");

// ---- concat com isConcatSpreadable.
P("var o = { length: 2, 0: 'a', 1: 'b', [Symbol.isConcatSpreadable]: true }; return [1].concat(o);");
P("var o = { length: 2, 0: 'a', 1: 'b' }; return [1].concat(o);");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = false; return [0].concat(a).length;");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = undefined; return [0].concat(a);");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = null; return [0].concat(a).length;");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = 0; return [0].concat(a).length;");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = ''; return [0].concat(a).length;");
P("var a = [1, 2]; a[Symbol.isConcatSpreadable] = 'x'; return [0].concat(a).length;");
P("var o = { length: 1, 0: 'z', [Symbol.isConcatSpreadable]: 1 }; return [].concat(o);");
P("var o = { length: 1, 0: 'z', [Symbol.isConcatSpreadable]: 0 }; return [].concat(o).length;");
P("var o = { length: 1, 0: 'z', [Symbol.isConcatSpreadable]: undefined }; return [].concat(o).length;");
P("var o = { length: 3, 1: 'm', [Symbol.isConcatSpreadable]: true }; var r = [].concat(o); return [r, 0 in r, 1 in r];");
P("var o = { [Symbol.isConcatSpreadable]: true }; return [].concat(o);");
P("var o = { length: -3, [Symbol.isConcatSpreadable]: true }; return [].concat(o);");
P("var o = { length: 'a', [Symbol.isConcatSpreadable]: true }; return [].concat(o);");
P("var o = { length: 2.5, 0: 1, 1: 2, 2: 3, [Symbol.isConcatSpreadable]: true }; return [].concat(o);");
P("var o = { get length() { throw new EvalError('len'); }, [Symbol.isConcatSpreadable]: true }; return [].concat(o);");
P("var o = { get [Symbol.isConcatSpreadable]() { throw new EvalError('spr'); } }; return [].concat(o);");
P("var o = { get [Symbol.isConcatSpreadable]() { throw new EvalError('spr'); } }; return [].concat(1, o);");
P("var o = 'str'; String.prototype[Symbol.isConcatSpreadable] = true; try { return [].concat(o); } finally { delete String.prototype[Symbol.isConcatSpreadable]; }");
P("Number.prototype[Symbol.isConcatSpreadable] = true; try { return [].concat(5); } finally { delete Number.prototype[Symbol.isConcatSpreadable]; }");
P("Boolean.prototype[Symbol.isConcatSpreadable] = true; Boolean.prototype.length = 2; Boolean.prototype[0] = 'x'; try { return [].concat(false); } finally { delete Boolean.prototype[Symbol.isConcatSpreadable]; delete Boolean.prototype.length; delete Boolean.prototype[0]; }");
P("Object.prototype[Symbol.isConcatSpreadable] = true; try { return [].concat({ length: 1, 0: 'q' }, {}); } finally { delete Object.prototype[Symbol.isConcatSpreadable]; }");
P("Array.prototype[Symbol.isConcatSpreadable] = false; try { return [1].concat([2]).length; } finally { delete Array.prototype[Symbol.isConcatSpreadable]; }");
P("var f = function () {}; f.length = 1; f[0] = 'f'; f[Symbol.isConcatSpreadable] = true; return [].concat(f);");
P("var p = new Proxy([1, 2], {}); return [0].concat(p);");
P("var p = new Proxy({}, { get: function (t, k) { return k === Symbol.isConcatSpreadable ? true : k === 'length' ? 1 : 'px'; } }); return [].concat(p);");
P("var r = new Proxy([], {}); var rv = Proxy.revocable([], {}); rv.revoke(); try { return [].concat(rv.proxy); } catch (e) { return e.name; }");
P("class A extends Array {} return [].concat(new A(1, 2));");
P("class A extends Array {} var r = new A(1, 2).concat([3]); return [r instanceof A, r.length];");
P("class A extends Array {} var a = new A(); a[Symbol.isConcatSpreadable] = false; return [1].concat(a).length;");
P("return [].concat([1], [[2]], 3, [[[4]]]);");
P("return [].concat();");
P("return [1].concat(undefined, null);");
P("return [1, , 3].concat([, 5]);");
P("var r = [1, , 3].concat([, 5]); return [1 in r, 3 in r];");
P("Array.prototype[1] = 'p'; var r = [1, , 3].concat([4]); delete Array.prototype[1]; return [r, Object.keys(r)];");
P("var a = [1]; return a.concat(a, a);");
P("var a = []; a[2 ** 32 - 2] = 1; return [].concat(a).length;");
P("var a = []; a[2 ** 32 - 2] = 1; return a.concat([1]).length;");
P("var a = new Array(2 ** 32 - 1); return a.concat([1]);");
P("var a = new Array(2 ** 32 - 1); return a.concat(1);");
P("var a = new Array(2 ** 32 - 1); return a.concat([]).length;");
P("var a = new Array(2 ** 31); return a.concat(a).length;");
P("var a = new Array(2 ** 31); return a.concat(a, [1]);");
P("var a = new Array(2 ** 31); return a.concat(a, []).length;");
P("return Array.prototype.concat.call(1, 2).length;");
P("return Array.prototype.concat.call('a', 'b');");
P("return Array.prototype.concat.call({ a: 1 }, [2]).length;");
P("return Array.prototype.concat.call(null, 1);");
P("return Array.prototype.concat.call(undefined);");
P("var r = Array.prototype.concat.call(true, 2); return typeof r[0];");
P("var r = Array.prototype.concat.call(1); return [r.length, typeof r[0], r[0] instanceof Number];");
P("return Array.prototype.concat.length;");
P("var o = Object.freeze([1]); return o.concat(2);");

// ---- join e toString cíclicos.
P("var a = [1, 2]; a.push(a); return a.join();");
P("var a = [1, 2]; a.push(a); return a.toString();");
P("var a = [1, 2]; a.push(a); return String(a);");
P("var a = [1, 2]; a.push(a); return a + '';");
P("var a = [1, 2]; a.push(a); return `${a}`;");
P("var a = []; a[0] = a; return a.join('-');");
P("var a = []; a[0] = a; a[1] = a; return a.join('-');");
P("var a = [1]; var b = [2, a]; a.push(b); return [a.join(), b.join()];");
P("var a = [1]; var b = [2, a]; a.push(b); return a.join(';');");
P("var a = [[1, [2]], 3]; return a.join('|');");
P("var a = [1, [2, [3, [4]]]]; return a.toString();");
P("var a = []; a.push(a, [a]); return a.join();");
P("var a = [1]; a.push({ toString: function () { return a.join(); } }); return a.join();");
P("var a = [1]; a.push({ toString: function () { return a.toString(); } }); return a.toString();");
P("var a = [1, 2]; a.push({ toString: function () { return 'x' + a.length; } }); return a.join();");
P("var a = [1, 2, 3]; a.join = function () { return 'custom'; }; return a.toString();");
P("var a = [1, 2, 3]; a.join = 5; return a.toString();");
P("var a = [1, 2, 3]; a.join = null; return a.toString();");
P("var a = [1, 2, 3]; a.join = undefined; return a.toString();");
P("var a = [1, 2, 3]; a.join = function () { return this === a; }; return a.toString();");
P("return Array.prototype.toString.call({ join: function () { return 'jj'; } });");
P("return Array.prototype.toString.call({});");
P("return Array.prototype.toString.call({ join: 1 });");
P("return Array.prototype.toString.call(null);");
P("return Array.prototype.toString.call('x');");
P("return Array.prototype.toString.call(1);");
P("return Array.prototype.toString.call(function () {});");
P("return Array.prototype.toString.call({ join: Array.prototype.join, length: 2, 0: 'a', 1: 'b' });");
P("return Array.prototype.toString.call({ join: Array.prototype.join, length: 2, 0: 'a', 1: 'b' });");
P("return [null, undefined, 1, true, 'x', {}, [], [[]], [null]].join('-');");
P("return [null, undefined].join();");
P("return [, ,].join('x');");
P("return new Array(4).join('ab');");
P("return [1, 2, 3].join(undefined);");
P("return [1, 2, 3].join(null);");
P("return [1, 2, 3].join('');");
P("return [1, 2, 3].join({ toString: function () { return '+'; } });");
P("return [1, 2, 3].join(Symbol());");
P("return [Symbol()].join();");
P("return [1n, 2n].join();");
P("return [-0, 0, NaN, 1e21, 1e-7, 0.1 + 0.2].join();");
P("return [1, 2, 3].join(1);");
P("return [1, 2, 3].join(['a']);");
P("return [1, 2, 3].join(function () {}) .length > 0;");
P("var a = [1, 2, 3]; return a.join({ toString: function () { a.length = 1; return '-'; } });");
P("var a = [1, 2, 3]; a.length; return a.join({ toString: function () { a.push(4); return '-'; } });");
P("var a = [{ toString: function () { a.length = 0; return 'x'; } }, 2, 3]; return a.join();");
P("return Array.prototype.join.call({ length: 3, 0: 'a', 1: undefined, 2: null }, '/');");
P("return Array.prototype.join.call('abc', '.');");
P("return Array.prototype.join.call({ length: -1 }, '.');");
P("return Array.prototype.join.call({ length: 'x' }, '.');");
P("return Array.prototype.join.call({ length: 1.9, 0: 'a', 1: 'b' }, '.');");
P("return Array.prototype.join.call(null);");
P("return Array.prototype.join.call({ length: 2 ** 53 }, '').length;");
P("return Array.prototype.join.call({ length: 2 ** 32 + 1 }, '').length;");
P("return Array.prototype.join.call({ length: 2 ** 30 }, 'abcdefgh');");
P("return Array.prototype.join.call({ length: 2 ** 29 }, 'abcdefgh');");
P("return Array.prototype.join.call({ length: 2 ** 31 }, '');");
P("return Array.prototype.join.call({ length: 2 ** 31 + 1 }, 'a');");
P("return new Array(2 ** 31).join('a');");
P("return new Array(2 ** 30 + 2).join('abcdefgh');");
P("return [[1, 2], [3]].toString();");
P("return String([[], [[]], []]);");
P("return Array.prototype.toLocaleString.call([1, 'a', null, undefined, {}]);");
P("var a = [1]; a.push(a); return a.toLocaleString();");
P("var a = [1, 2]; a.push(a); return a.toLocaleString();");
P("var a = [{ toLocaleString: function () { return 'L'; } }, null, undefined, 3]; return a.toLocaleString();");
P("return [{ toLocaleString: 5 }].toLocaleString();");
P("return [{ toLocaleString: function () { return {}; } }].toLocaleString();");
P("return [{ toLocaleString: function () { return 7; } }].toLocaleString();");
P("return [{ toLocaleString: function () { return this === undefined; } }].toLocaleString();");
P("return Array.prototype.toLocaleString.call({ length: 2, 0: 'a', 1: 'b' });");
P("return Array.prototype.toLocaleString.call(null);");
P("return Array.prototype.toLocaleString.call('ab');");
P("var o = [{ toLocaleString: function () { return [].slice.call(arguments).length; } }]; return o.toLocaleString();");
P("var o = [{ toLocaleString: function () { return [].slice.call(arguments).length; } }]; return o.toLocaleString('pt', {});");

// ---- execução.
function stripPrelude(source) { return source.startsWith(PRELUDE) ? source.slice(PRELUDE.length) : source; }
const existing = knownPrograms("array_more_bun.tsv", ["array_bun.tsv", "array_edge_bun.tsv"]);
const existingText = existing.join("\u0000");

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "array-more-golden-"));
const source_file = path.join(dir, "array_source.js");
const file = path.join(dir, "array_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const body of bodies) {
  if (seen.has(body)) continue;
  seen.add(body);
  if (existingText.includes(body)) { dropped++; continue; }
  const source = PRELUDE + "\n" + body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source.replace(/\bR = /g, "globalThis.R = "));
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source.replace(/\bR = /g, "globalThis.R = ")) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
