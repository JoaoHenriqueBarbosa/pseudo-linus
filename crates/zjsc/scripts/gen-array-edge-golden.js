// Gera tests/golden/array_edge_bun.tsv: complemento de borda de gen-array-golden.js, medido no bun 1.4.2.
// Foca nos cantos de Array.prototype: splice e copyWithin em array-likes de length gigante, flat/flatMap com proxies e
// buracos, sort estável com comparadores inconsistentes, toSorted/toSpliced/with (limites e RangeError), findLast, at,
// includes com NaN e -0, species (constructor ausente, não objeto, species nulo, resultado não array), buracos com
// índices herdados do protótipo, Symbol.isConcatSpreadable e Array.from com iterables e mapFn (fechamento do iterador).
// Array.fromAsync fica de fora (exige microtarefas). Programas que já estão em array_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-array-golden.js, com o mesmo
// prelúdio (`S`, `T`, `D`).
// Uso: bun scripts/gen-array-edge-golden.js > tests/golden/array_edge_bun.tsv
const fs = require("fs");
const { emitFactoredLines, knownProgramSet, prepareProgram, sampleByHash } = require("./golden-prelude.js");
const lines = [];
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
  "function T(f) { try { return S(f()); } catch (e) { return 'throw ' + e.name + ': ' + e.message; } }",
  "function D(it) { var o = []; for (var x of it) { o.push(S(x)); if (o.length > 40) break; } return o.join(';'); }",
].join("\n");

const programs = [];
const E = expr => programs.push(`R = T(function () { return (${expr}); });`);
const F = (...rows) => programs.push(`R = T(function () {\n${rows.join("\n")}\n});`);

const MAX = "9007199254740991"; // 2**53-1
const P = "Array.prototype";

// ---- 1. splice em array-likes de length gigante.
for (const len of [MAX, "2**53", "2**53+10", "2**32", "2**32+1", "2**31", "Infinity", "-1", "'3'", "1.9"]) {
  E(`${P}.splice.call({length: ${len}}, 0, 0)`);
  // Só os comprimentos que estouram 2**53-1 aceitam inserção no início sem varrer o array (a checagem de TypeError vem antes).
  if (len === MAX || len === "2**53" || len === "2**53+10" || len === "Infinity") {
    E(`${P}.splice.call({length: ${len}}, 0, 0, 'x')`);
    E(`${P}.splice.call({length: ${len}}, 1, 0, 'x', 'y')`);
  }
  E(`${P}.splice.call({length: ${len}}, -1, 0, 'x')`);
  E(`${P}.splice.call({length: ${len}}, -1, 0)`);
  // toSpliced cria o resultado inteiro: só os comprimentos que lançam antes do laço (ou os pequenos) entram.
  if (len !== "2**31") E(`${P}.toSpliced.call({length: ${len}}, 0, 0)`);
}
for (const [s, c] of [["2**53-3", "2"], ["2**53-3", "5"], ["2**53-2", "1"], ["-2", "10"], ["-2", "1"], ["2**53-1", "0"], ["2**53", "0"]]) {
  F(`var o = {length: ${MAX}, [2**53-3]: 'a', [2**53-2]: 'b'};`, `var r = ${P}.splice.call(o, ${s}, ${c});`, `return [r, o.length, Object.keys(o).join()];`);
  F(`var o = {length: ${MAX}, [2**53-3]: 'a', [2**53-2]: 'b'};`, `var r = ${P}.splice.call(o, ${s}, ${c}, 'n');`, `return [r, o.length, Object.keys(o).join()];`);
}
F(`var o = {length: 5, 0: 'a', 1: 'b', 3: 'd'};`, `var r = ${P}.splice.call(o, 1, 2, 'x', 'y', 'z');`, `return [r, o];`);
F(`var o = {length: 5, 0: 'a', 1: 'b', 3: 'd'};`, `var r = ${P}.splice.call(o, 1, 3);`, `return [r, o];`);
F(`var o = {length: {valueOf() { return 3; }}, 0: 1, 1: 2, 2: 3};`, `var r = ${P}.splice.call(o, 0, 1);`, `return [r, o];`);
F(`var o = {length: 3, 0: 1, 1: 2, 2: 3};`, `var r = ${P}.splice.call(o, {valueOf() { return 1; }}, {valueOf() { return 1; }});`, `return [r, o];`);
F(`var log = [];`, `var p = new Proxy([1, 2, 3, 4], {get(t, k, r) { log.push('get ' + String(k)); return Reflect.get(t, k, r); }, set(t, k, v, r) { log.push('set ' + String(k)); return Reflect.set(t, k, v, r); }, has(t, k) { log.push('has ' + String(k)); return k in t; }, deleteProperty(t, k) { log.push('del ' + String(k)); return delete t[k]; }, defineProperty(t, k, d) { log.push('def ' + String(k)); return Reflect.defineProperty(t, k, d); }});`, `p.splice(1, 2, 'a');`, `return log;`);
F(`var a = [1, 2, 3]; Object.freeze(a);`, `return [${P}.splice.call(a, 0, 0), a];`);
F(`var a = [1, 2, 3]; Object.freeze(a);`, `return ${P}.splice.call(a, 0, 1);`);
F(`var a = [1, 2, 3]; Object.freeze(a);`, `return ${P}.splice.call(a, 0, 0, 9);`);
F(`var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false});`, `return ${P}.splice.call(a, 0, 1);`);
F(`var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false});`, `return ${P}.splice.call(a, 0, 0);`);
F(`var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false});`, `return ${P}.splice.call(a, 0, 3);`);
F(`var a = [1, 2, 3]; Object.seal(a);`, `return ${P}.splice.call(a, 0, 1);`);
F(`var a = [1, 2, 3]; Object.preventExtensions(a);`, `return ${P}.splice.call(a, 1, 0, 'x');`);
F(`var a = [1, 2, 3]; Object.preventExtensions(a);`, `return ${P}.splice.call(a, 1, 1, 'x');`);

// ---- 2. copyWithin em array-likes e com buracos.
for (const [t, s, e] of [["0", "1", "3"], ["1", "0", "3"], ["0", "2", "undefined"], ["2", "0", "undefined"], ["-1", "0", "undefined"], ["0", "-2", "-1"], ["0", "1", "1"], ["0", "3", "1"], ["1", "1", "3"], ["4", "0", "2"], ["0", "0", "5"], ["Infinity", "0", "2"], ["0", "-Infinity", "Infinity"], ["NaN", "NaN", "NaN"], ["'1'", "'0'", "'2'"]]) {
  F(`var o = {length: 5, 0: 'a', 1: 'b', 3: 'd'};`, `${P}.copyWithin.call(o, ${t}, ${s}, ${e});`, `return o;`);
  F(`var a = [1, , 3, , 5];`, `a.copyWithin(${t}, ${s}, ${e});`, `return a;`);
}
for (const [t, s, e] of [["2**53-4", "2**53-6", "2**53-3"], ["2**53-6", "2**53-4", "2**53-1"], ["0", "2**53-3", "2**53-1"], ["2**53-3", "0", "2"], ["2**53", "0", "1"]]) {
  F(`var o = {length: ${MAX}, [2**53-6]: 'a', [2**53-5]: 'b', [2**53-3]: 'c', [2**53-2]: 'd', 0: 'z', 1: 'y'};`, `${P}.copyWithin.call(o, ${t}, ${s}, ${e});`, `return Object.keys(o).map(function (k) { return k + '=' + o[k]; }).join();`);
}
F(`var a = [1, 2, 3, 4, 5]; Object.freeze(a);`, `return a.copyWithin(0, 3);`);
F(`var a = [1, 2, 3, 4, 5]; Object.freeze(a);`, `return a.copyWithin(0, 0);`);
F(`var a = [1, 2, 3, 4, 5]; Object.freeze(a);`, `return a.copyWithin(0, 5);`);
F(`var a = [1, 2, 3]; var n = 0;`, `return a.copyWithin({valueOf() { n++; return 0; }}, {valueOf() { n++; return 1; }}, {valueOf() { n++; a.length = 1; return 3; }});`);
F(`var a = [1, 2, 3, 4];`, `${P}[1] = 'proto'; try { return a.copyWithin(0, 1); } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3, 4]; `, `${P}[1] = 'proto'; try { a.copyWithin(2, 0); return a; } finally { delete ${P}[1]; }`);

// ---- 3. flat e flatMap.
const flatInputs = ["[1, [2, [3, [4, [5]]]]]", "[[], [[]], [[[]]]]", "[1, , [2, , [3, , ]]]", "[[1, 2], [3, 4], 5]", "[[[1]], [[2]]]", "[[,], [, 1]]", "[{length: 1, 0: 'a'}, 'str', 1]", "[new Proxy([1, [2]], {}), [new Proxy([3], {})]]", "[[undefined, null], [NaN, -0]]", "[['a'], 'b', ['c', ['d']]]"];
for (const a of flatInputs) for (const d of ["undefined", "0", "1", "2", "3", "Infinity", "-Infinity", "NaN", "-1", "'2'", "{valueOf() { return 1; }}", "null", "true", "2**32", "1.9"]) E(`${a}.flat(${d})`);
for (const a of ["[1, 2, 3]", "[1, , 3]", "[[1], [2]]", "[]", "[1, [2]]"]) {
  E(`${a}.flatMap(function (x) { return [x, [x]]; })`);
  E(`${a}.flatMap(function (x) { return {length: 2, 0: x, 1: x}; })`);
  E(`${a}.flatMap(function (x) { return new Proxy([x, x], {}); })`);
  E(`${a}.flatMap(function (x) { return x === 2 ? [] : x; })`);
  E(`${a}.flatMap(function (x, i, arr) { return [i, arr === ${a}]; })`);
  E(`${a}.flatMap(function (x) { return [[x]]; })`);
  E(`${a}.flatMap(function () { return this.v; }, {v: [7, 8]})`);
  E(`${a}.flatMap(function () { 'use strict'; return [typeof this]; })`);
  E(`${a}.flatMap(function () { 'use strict'; return [typeof this]; }, 5)`);
}
for (const f of ["undefined", "null", "1", "'f'", "{}", "[]", "Symbol()", "class {}"]) E(`[1].flatMap(${f})`);
E(`${P}.flatMap.call(null, function () {})`);
E(`${P}.flatMap.call({length: 2, 0: 1, 1: 2}, function (x) { return [x, x]; })`);
E(`${P}.flat.call({length: 2, 0: [1], 1: [2]})`);
E(`${P}.flat.call('abc')`);
E(`${P}.flat.call(undefined)`);
F(`var r = Proxy.revocable([1], {}); r.revoke();`, `return [r.proxy].flat();`);
F(`var r = Proxy.revocable([1], {}); r.revoke();`, `return [1].flatMap(function () { return r.proxy; });`);
F(`var o = {[Symbol.isConcatSpreadable]: true, length: 1, 0: 'x'};`, `return [o, [o]].flat(2);`);
F(`var a = [1, [2]]; a.constructor = {[Symbol.species]: function (n) { return {length: 0, n: n}; }};`, `return S(a.flat());`);
F(`var a = [1, [2]]; a.constructor = {[Symbol.species]: function (n) { return {length: 0, n: n}; }};`, `return S(a.flatMap(function (x) { return x; }));`);
F(`var a = [1, [2]]; a.constructor = undefined;`, `return a.flat();`);
F(`var a = [1, [2]]; a.constructor = null;`, `return a.flat();`);
F(`var a = [1, [2]]; a.constructor = 1;`, `return a.flat();`);
F(`var a = [1, [2]]; a.constructor = {[Symbol.species]: null};`, `return a.flat();`);
F(`var a = [1, [2]]; a.constructor = {[Symbol.species]: 1};`, `return a.flat();`);
F(`var a = [1, [2]]; var depth = 0; a.constructor = {[Symbol.species]: function () { depth++; return []; }};`, `a.flat(); return depth;`);
F(`var deep = []; for (var i = 0; i < 100; i++) deep = [deep, i];`, `return deep.flat(Infinity).length;`);
F(`var cyc = [1]; cyc.push(cyc);`, `return T(function () { return cyc.flat(Infinity); });`);
F(`var cyc = [1]; cyc.push(cyc);`, `return cyc.flat(3);`);

// ---- 4. sort estável e comparadores inconsistentes.
const keyed = n => `Array.from({length: ${n}}, function (_, i) { return {k: i % 3, i: i}; })`;
const cmps = {
  byK: "function (a, b) { return a.k - b.k; }",
  byKDesc: "function (a, b) { return b.k - a.k; }",
  alwaysZero: "function () { return 0; }",
  alwaysPos: "function () { return 1; }",
  alwaysNeg: "function () { return -1; }",
  nan: "function () { return NaN; }",
  undef: "function () { }",
  bool: "function (a, b) { return a.k > b.k; }",
  boolLess: "function (a, b) { return a.k < b.k; }",
  str: "function (a, b) { return String(a.k - b.k); }",
  infinity: "function (a, b) { return (a.k - b.k) * Infinity; }",
  negZero: "function () { return -0; }",
  alternate: "(function () { var n = 0; return function () { return (n++ % 2) ? 1 : -1; }; })()",
  inverted: "function (a, b) { return a.k === b.k ? 1 : a.k - b.k; }",
  invertedNeg: "function (a, b) { return a.k === b.k ? -1 : a.k - b.k; }",
  ties: "function (a, b) { return (a.i % 2) - (b.i % 2); }",
  bigint: "function (a, b) { return BigInt(a.k - b.k); }",
  obj: "function (a, b) { return {valueOf() { return a.k - b.k; }}; }",
  strNum: "function (a, b) { return '' + (a.k - b.k); }",
  nullRet: "function () { return null; }",
};
for (const [name, cmp] of Object.entries(cmps)) {
  for (const n of [0, 1, 2, 3, 7, 12, 25]) {
    if (name === "bigint" && n > 2) continue;
    F(`var a = ${keyed(n)};`, `a.sort(${cmp});`, `return a.map(function (x) { return x.k + ':' + x.i; }).join();`);
  }
  F(`var a = ${keyed(9)};`, `return a.toSorted(${cmp}).map(function (x) { return x.k + ':' + x.i; }).join();`);
}
for (const arr of ["[3, , 1, undefined, 2, , undefined]", "[undefined, undefined, 1]", "[, , 1, 0]", "['b', undefined, 'a', , 'c']", "[null, undefined, NaN, 0, -0, '', false]", "[10, 9, 1, 100, 25]", "['10', '9', '1', 100, 25]", "[true, false, 'true', 'false', 1, 0]"]) {
  E(`${arr}.sort()`);
  E(`${arr}.sort(undefined)`);
  E(`${arr}.sort(function (a, b) { return a < b ? -1 : a > b ? 1 : 0; })`);
  E(`${arr}.sort(function (a, b) { return 0; })`);
  E(`${arr}.toSorted()`);
  E(`${arr}.toSorted(function (a, b) { return a - b; })`);
  F(`var seen = [];`, `${arr}.sort(function (a, b) { seen.push(typeof a + typeof b); return 0; });`, `return seen.length;`);
}
for (const bad of ["null", "1", "'f'", "{}", "[]", "true", "Symbol()", "0", "NaN", "class {}"]) {
  E(`[2, 1].sort(${bad})`);
  E(`[2, 1].toSorted(${bad})`);
  E(`[].sort(${bad})`);
}
E(`${P}.sort.call({length: 3, 0: 'c', 1: 'a', 2: 'b'})`);
F(`var o = {length: 4, 0: 'c', 2: 'a', 3: undefined};`, `${P}.sort.call(o);`, `return o;`);
F(`var o = {length: 3, 0: 3, 1: 1, 2: 2};`, `${P}.sort.call(o, function (a, b) { return b - a; });`, `return o;`);
E(`${P}.sort.call({length: ${MAX} - ${MAX} + 0})`);
E(`${P}.sort.call('abc')`);
E(`${P}.sort.call(null)`);
E(`${P}.toSorted.call({length: 3, 0: 'c', 1: 'a', 2: 'b'})`);
E(`${P}.toSorted.call({length: 2 ** 32})`);
E(`${P}.toSorted.call({length: 2 ** 32 - 1}, 1)`);
F(`var a = [3, 2, 1];`, `a.sort(function (x, y) { a.length = 0; return x - y; });`, `return a;`);
F(`var a = [3, 2, 1];`, `a.sort(function (x, y) { a.push(0); return x - y; });`, `return a;`);
F(`var a = [3, 2, 1];`, `a.sort(function (x, y) { throw new Error('cmp'); });`, `return a;`);
F(`var a = [3, 2, 1];`, `try { a.sort(function (x, y) { if (x === 2 || y === 2) throw new Error('cmp'); return x - y; }); } catch (e) {}`, `return a;`);
F(`var a = [3, 2, 1];`, `var r = a.sort(function (x, y) { return x - y; });`, `return r === a;`);
F(`var a = Object.freeze([3, 2, 1]);`, `return a.sort();`);
F(`var a = Object.freeze([1]);`, `return a.sort();`);
F(`var a = Object.freeze([]);`, `return a.sort();`);
F(`var a = [3, 2, 1]; Object.defineProperty(a, 1, {get() { return 5; }, configurable: true});`, `a.sort(); return a;`);
F(`var log = [];`, `var p = new Proxy([3, 1, 2], {get(t, k, r) { log.push('g' + String(k)); return Reflect.get(t, k, r); }, set(t, k, v, r) { log.push('s' + String(k)); return Reflect.set(t, k, v, r); }, has(t, k) { log.push('h' + String(k)); return k in t; }, deleteProperty(t, k) { log.push('d' + String(k)); return delete t[k]; }});`, `p.sort(); return log;`);
F(`var a = [1, , 3]; ${P}[1] = 0;`, `try { a.sort(); return a; } finally { delete ${P}[1]; }`);
F(`var a = ['b', , 'a']; ${P}[1] = 'c';`, `try { return a.toSorted(); } finally { delete ${P}[1]; }`);
F(`var a = [2, 1]; var order = [];`, `a.sort(function (x, y) { order.push(x + ',' + y); return x - y; });`, `return order;`);
F(`var a = [5, 4, 3, 2, 1]; var order = [];`, `a.sort(function (x, y) { order.push(x + ',' + y); return x - y; });`, `return order;`);
F(`var a = [1, 2, 3, 4, 5]; var order = [];`, `a.sort(function (x, y) { order.push(x + ',' + y); return x - y; });`, `return order;`);
F(`var a = ['\\ud83d\\ude00', '\\uffff', 'a', '\\u00e9', 'z', 'Z', '\\ud800'];`, `return a.sort();`);
F(`var a = [1, 10, 2, 21, 3];`, `return a.sort().concat(a.toSorted(function (x, y) { return y - x; }));`);
F(`var a = [{toString() { return 'b'; }}, {toString() { return 'a'; }}];`, `return a.sort().map(String);`);
F(`var a = [{toString() { throw new Error('ts'); }}, 1];`, `return a.sort();`);
F(`var a = [Symbol('a'), 1];`, `return a.sort();`);
F(`var a = [1n, 2n, 10n, -1n];`, `return a.sort();`);
F(`var a = [3n, 1, 2n, 0];`, `return a.sort(function (x, y) { return x < y ? -1 : x > y ? 1 : 0; });`);
F(`var a = [3, 1, 2];`, `return a.sort(function (x, y) { return y - x; }).toSorted();`);
F(`var a = Array.from({length: 130}, function (_, i) { return {k: (i * 7) % 5, i: i}; });`, `a.sort(function (x, y) { return x.k - y.k; });`, `return a.every(function (x, i) { return i === 0 || a[i - 1].k < x.k || (a[i - 1].k === x.k && a[i - 1].i < x.i); });`);
F(`var a = Array.from({length: 400}, function (_, i) { return {k: i % 2, i: i}; });`, `a.sort(function (x, y) { return x.k - y.k; });`, `return a.every(function (x, i) { return i === 0 || a[i - 1].k < x.k || (a[i - 1].k === x.k && a[i - 1].i < x.i); });`);

// ---- 5. toSorted, toSpliced, toReversed e with.
for (const idx of ["0", "1", "2", "3", "-1", "-3", "-4", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'1'", "undefined", "null", "true", "{valueOf() { return 1; }}", "2**32", "2**53", "-(2**53)"]) {
  E(`[1, 2, 3].with(${idx}, 'x')`);
  E(`[1, , 3].with(${idx}, 'x')`);
  E(`[].with(${idx}, 'x')`);
}
E(`[1, 2, 3].with()`);
E(`[1, 2, 3].with(0)`);
E(`${P}.with.call({length: 3, 0: 'a', 1: 'b', 2: 'c'}, 1, 'x')`);
E(`${P}.with.call({length: 2 ** 32}, 0, 'x')`);
E(`${P}.with.call({length: 2 ** 32 - 1}, 2 ** 32, 'x')`);
E(`${P}.with.call({length: 2 ** 32 - 1}, -(2 ** 32), 'x')`);
E(`${P}.with.call('abc', 1, 'x')`);
E(`${P}.with.call(null, 0, 1)`);
F(`var a = [1, 2, 3]; var b = a.with(0, 'x');`, `return [a, b, a === b];`);
F(`var a = [1, 2, 3]; a.constructor = {[Symbol.species]: function () { throw new Error('used'); }};`, `return [a.with(0, 0), a.toSorted(), a.toReversed(), a.toSpliced(0, 1)];`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { return [a.toReversed(), a.toSorted(), a.with(0, 0), a.toSpliced(0, 0)]; } finally { delete ${P}[1]; }`);
for (const [s, c] of [["0", "0"], ["0", "undefined"], ["1", "1"], ["-1", "1"], ["-10", "1"], ["10", "1"], ["1", "10"], ["1", "-1"], ["NaN", "NaN"], ["Infinity", "Infinity"], ["-Infinity", "-Infinity"], ["'1'", "'1'"], ["1.9", "1.9"], ["undefined", "undefined"], ["null", "null"], ["true", "true"]]) {
  E(`[1, 2, 3].toSpliced(${s}, ${c})`);
  E(`[1, 2, 3].toSpliced(${s}, ${c}, 'a', 'b', 'c')`);
  E(`[1, , 3].toSpliced(${s}, ${c}, undefined, undefined)`);
}
E(`[1, 2, 3].toSpliced()`);
E(`[1, 2, 3].toSpliced(undefined)`);
E(`[1, 2, 3].toSpliced(1)`);
E(`${P}.toSpliced.call({length: 2 ** 32 - 1}, 0, 0, 'x')`);
E(`${P}.toSpliced.call({length: ${MAX}}, 0, 0, 'x')`);
E(`${P}.toSpliced.call({length: ${MAX}}, 0, 0)`);
E(`${P}.toSpliced.call({length: ${MAX}}, 0, 1, 'x')`);
E(`${P}.toSpliced.call({length: 2 ** 32 - 2}, 0, 0, 'x', 'y')`);
E(`${P}.toReversed.call({length: 2 ** 32})`);
E(`${P}.toReversed.call({length: 3, 0: 1, 2: 3})`);
E(`${P}.toReversed.call({length: -5})`);
E(`${P}.toReversed.call('abc')`);
E(`${P}.toReversed.call(null)`);
F(`var calls = [];`, `var p = new Proxy([1, 2, 3], {get(t, k, r) { calls.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { calls.push('has:' + String(k)); return k in t; }});`, `p.toReversed(); return calls;`);
F(`var calls = [];`, `var p = new Proxy([1, 2, 3], {get(t, k, r) { calls.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { calls.push('has:' + String(k)); return k in t; }});`, `p.with(1, 'x'); return calls;`);
F(`var calls = [];`, `var p = new Proxy([3, 2, 1], {get(t, k, r) { calls.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { calls.push('has:' + String(k)); return k in t; }});`, `p.toSorted(); return calls;`);
F(`var calls = [];`, `var p = new Proxy([1, 2, 3], {get(t, k, r) { calls.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { calls.push('has:' + String(k)); return k in t; }});`, `p.toSpliced(1, 1, 'x'); return calls;`);

// ---- 6. find, findIndex, findLast, findLastIndex.
const finders = ["find", "findIndex", "findLast", "findLastIndex"];
for (const m of finders) {
  for (const a of ["[1, 2, 3, 4]", "[1, , 3, ,]", "[undefined, 2]", "[]", "[NaN]", "[0, -0]"]) {
    E(`${a}.${m}(function (x) { return x === undefined; })`);
    E(`${a}.${m}(function (x, i, arr) { return i === 1; })`);
    E(`${a}.${m}(function () { return true; })`);
    E(`${a}.${m}(function () { return false; })`);
    E(`${a}.${m}(function (x) { return x !== x; })`);
    E(`${a}.${m}(function (x) { return Object.is(x, -0); })`);
    E(`${a}.${m}(function () { return ''; })`);
    E(`${a}.${m}(function () { return 'a'; })`);
    E(`${a}.${m}(function () { return {}; })`);
    E(`${a}.${m}(function () { return 0n; })`);
  }
  for (const f of ["undefined", "null", "1", "'f'", "{}", "Symbol()"]) E(`[1].${m}(${f})`);
  E(`${P}.${m}.call({length: 3, 0: 'a', 2: 'c'}, function (x) { return x === undefined; })`);
  E(`${P}.${m}.call({length: ${MAX}}, function (x, i) { return true; })`);
  E(`${P}.${m}.call({length: 2 ** 53 + 10}, function (x, i) { return true; })`);
  if (m.startsWith("findLast")) E(`${P}.${m}.call({length: ${MAX}, [2**53-2]: 'last'}, function (x, i) { return x === 'last'; })`);
  E(`${P}.${m}.call({length: 2 ** 32}, function (x, i) { return true; })`);
  E(`${P}.${m}.call('abc', function (x) { return x === 'b'; })`);
  E(`${P}.${m}.call(null, function () {})`);
  E(`${P}.${m}.call(undefined, function () {})`);
  F(`var a = [1, 2, 3, 4]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 1 || i === 2) a.length = 2; return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); a.push(9); return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); a[i + 1 < 3 ? i + 1 : 0] = 'm'; return false; });`, `return seen;`);
  F(`var a = [1, , 3]; var seen = [];`, `${P}[1] = 'p'; try { a.${m}(function (x) { seen.push(x); return false; }); return seen; } finally { delete ${P}[1]; }`);
  F(`var a = [1, 2]; var ctx;`, `a.${m}(function () { 'use strict'; ctx = this; return true; }); return ctx;`);
  F(`var a = [1, 2]; var ctx;`, `a.${m}(function () { 'use strict'; ctx = this; return true; }, 'th'); return ctx;`);
  F(`var a = [1, 2]; var ctx;`, `a.${m}(function () { ctx = this; return true; }, 'th'); return typeof ctx;`);
  F(`var a = [1, 2]; var ctx;`, `a.${m}(() => { ctx = this; return true; }, 'th'); return typeof ctx;`);
  F(`var order = [];`, `var o = {length: 3, get 0() { order.push(0); return 'a'; }, get 1() { order.push(1); return 'b'; }, get 2() { order.push(2); return 'c'; }};`, `${P}.${m}.call(o, function () { return false; }); return order;`);
  F(`var order = [];`, `var o = {length: 3, get 0() { order.push(0); return 'a'; }, get 1() { order.push(1); return 'b'; }, get 2() { order.push(2); return 'c'; }};`, `${P}.${m}.call(o, function (x) { return x === 'b'; }); return order;`);
  F(`var a = [1, 2, 3];`, `return a.${m}(function (x) { if (x === 2) throw new Error('boom'); return false; });`);
}
E(`[1, 2, 3, 2].findLast(function (x) { return x === 2; })`);
E(`[1, 2, 3, 2].findLastIndex(function (x) { return x === 2; })`);
E(`[1, 2, 3].findLastIndex(function (x) { return x > 5; })`);
E(`[].findLast(function () { return true; })`);
E(`Array.prototype.findLast.length + Array.prototype.findLastIndex.length`);
E(`Array.prototype.findLast.name + ',' + Array.prototype.findLastIndex.name`);
E(`Object.keys(Array.prototype[Symbol.unscopables]).join()`);
E(`Array.prototype[Symbol.unscopables].findLast + ',' + Array.prototype[Symbol.unscopables].toSorted + ',' + Array.prototype[Symbol.unscopables].with`);

// ---- 7. at.
for (const idx of ["0", "-0", "1", "2", "3", "-1", "-3", "-4", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'1'", "'-1'", "'x'", "undefined", "null", "true", "false", "[]", "[1]", "{}", "{valueOf() { return -1; }}", "{valueOf() { return {}; }, toString() { return '1'; }}", "2**32", "2**53", "-(2**53)", "1e300", "-1e300"]) {
  E(`[10, 20, 30].at(${idx})`);
  E(`[10, , 30].at(${idx})`);
}
E(`[1].at()`);
E(`[1].at(Symbol())`);
E(`[1].at(1n)`);
E(`[1].at({valueOf() { throw new Error('at'); }})`);
E(`${P}.at.call({length: 3, 2: 'c'}, -1)`);
E(`${P}.at.call({length: 3, 2: 'c'}, 2)`);
E(`${P}.at.call({length: ${MAX}, [2**53-2]: 'last'}, -1)`);
E(`${P}.at.call({length: ${MAX}, [2**53-2]: 'last'}, ${MAX} - 1)`);
E(`${P}.at.call({length: 2 ** 53 + 100, [2**53-2]: 'last'}, -1)`);
E(`${P}.at.call({length: Infinity, [2**53-2]: 'last'}, -1)`);
E(`${P}.at.call({length: -1}, 0)`);
E(`${P}.at.call({length: '2', 1: 'b'}, -1)`);
E(`${P}.at.call({length: {valueOf() { return 2; }}, 1: 'b'}, -1)`);
E(`${P}.at.call({length: 1.9, 0: 'a', 1: 'b'}, 1)`);
E(`${P}.at.call('abc', -1)`);
E(`${P}.at.call(1, 0)`);
E(`${P}.at.call(null, 0)`);
E(`${P}.at.call(undefined, 0)`);
F(`var a = [1, 2, 3];`, `return a.at({valueOf() { a.length = 0; return 0; }});`);
F(`var a = [1, 2, 3];`, `return a.at({valueOf() { a.length = 1; return -1; }});`);
E(`'abc'.at(-1) + [].at.name + [].at.length`);

// ---- 8. includes, indexOf, lastIndexOf com NaN, -0 e buracos.
const needles = ["NaN", "0", "-0", "undefined", "null", "'a'", "1n", "1", "'1'", "{}", "Infinity"];
const hay = ["[NaN]", "[0]", "[-0]", "[, ]", "[undefined]", "[null, undefined]", "[1n]", "[1]", "['1']", "[Infinity, -Infinity]", "[, , NaN, , ]"];
for (const h of hay) for (const n of needles) {
  E(`${h}.includes(${n})`);
  E(`${h}.indexOf(${n})`);
  E(`${h}.lastIndexOf(${n})`);
}
for (const from of ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "undefined", "null", "'1'", "1.9", "-1.9", "2**53", "-(2**53)", "{valueOf() { return 1; }}", "true"]) {
  E(`[NaN, 1, NaN].includes(NaN, ${from})`);
  E(`[NaN, 1, NaN].indexOf(NaN, ${from})`);
  E(`[1, 2, 1, 2].lastIndexOf(2, ${from})`);
  E(`[1, , 3].includes(undefined, ${from})`);
  E(`[1, , 3].indexOf(undefined, ${from})`);
}
E(`[1].includes()`);
E(`[undefined].includes()`);
E(`[, ].includes()`);
E(`[, ].indexOf()`);
E(`[1].includes(1, Symbol())`);
E(`[1].includes(Symbol())`);
E(`[1].indexOf(1, 1n)`);
E(`[1].includes(1, {valueOf() { throw new Error('fi'); }})`);
E(`${P}.includes.call({length: ${MAX}, [2**53-2]: 'x'}, 'x', ${MAX} - 2)`);
E(`${P}.includes.call({length: ${MAX}, [2**53-2]: 'x'}, 'x', -1)`);
E(`${P}.includes.call({length: ${MAX}}, 'x', ${MAX})`);
E(`${P}.includes.call({length: ${MAX}}, undefined, ${MAX} - 3)`);
E(`${P}.indexOf.call({length: ${MAX}, [2**53-2]: 'x'}, 'x', -1)`);
E(`${P}.indexOf.call({length: ${MAX}, [2**53-2]: 'x'}, 'x', ${MAX} - 2)`);
E(`${P}.indexOf.call({length: 2 ** 53 + 100, [2**53-2]: 'x'}, 'x', -1)`);
E(`${P}.lastIndexOf.call({length: ${MAX}, [2**53-2]: 'x'}, 'x')`);
E(`${P}.lastIndexOf.call({length: ${MAX}, [2**53-2]: 'x'}, 'x', -1)`);
E(`${P}.lastIndexOf.call({length: ${MAX}, [2**53-2]: 'x', 0: 'x'}, 'x', 5)`);
E(`${P}.lastIndexOf.call({length: ${MAX}, 0: 'x'}, 'x', -${MAX})`);
E(`${P}.lastIndexOf.call({length: ${MAX}, 0: 'x'}, 'x', -${MAX} - 1)`);
E(`${P}.includes.call({length: 0}, undefined, ${MAX})`);
E(`${P}.includes.call({length: 3, get 0() { throw new Error('g0'); }}, 1)`);
E(`${P}.includes.call({length: 3, 1: NaN}, NaN)`);
E(`${P}.includes.call('abc', 'b')`);
E(`${P}.includes.call(null, 1)`);
F(`var a = [1, 2, 3];`, `return a.includes(3, {valueOf() { a.length = 2; return 0; }});`);
F(`var a = [1, 2, 3];`, `return a.includes(undefined, {valueOf() { a.length = 2; return 0; }});`);
F(`var a = [1, 2, 3];`, `return a.indexOf(3, {valueOf() { a.length = 2; return 0; }});`);
F(`var a = [1, , 3];`, `${P}[1] = 'p'; try { return [a.includes('p'), a.indexOf('p'), a.lastIndexOf('p')]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3];`, `${P}[1] = undefined; try { return [a.includes(undefined), a.indexOf(undefined), a.lastIndexOf(undefined)]; } finally { delete ${P}[1]; }`);
F(`var log = [];`, `var p = new Proxy([1, , 3], {get(t, k, r) { log.push('g' + String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('h' + String(k)); return k in t; }});`, `p.includes(3); p.indexOf(3); return log;`);

// ---- 9. species.
const speciesCtors = {
  none: "delete a.constructor;",
  undef: "a.constructor = undefined;",
  nul: "a.constructor = null;",
  num: "a.constructor = 1;",
  str: "a.constructor = 'x';",
  emptyObj: "a.constructor = {};",
  spUndef: "a.constructor = {[Symbol.species]: undefined};",
  spNull: "a.constructor = {[Symbol.species]: null};",
  spNum: "a.constructor = {[Symbol.species]: 1};",
  spObj: "a.constructor = {[Symbol.species]: {}};",
  spArrow: "a.constructor = {[Symbol.species]: () => []};",
  spFn: "a.constructor = {[Symbol.species]: function (n) { this.n = n; }};",
  spRetArr: "a.constructor = {[Symbol.species]: function (n) { return ['ret', n]; }};",
  spRetPrim: "a.constructor = {[Symbol.species]: function (n) { return 1; }};",
  spThrow: "a.constructor = {[Symbol.species]: function () { throw new Error('sp'); }};",
  spGetterThrow: "a.constructor = {get [Symbol.species]() { throw new Error('spget'); }};",
  ctorGetterThrow: "Object.defineProperty(a, 'constructor', {get() { throw new Error('cget'); }});",
  subclass: "class M extends Array {} a = M.from(a);",
  subclassNoSpecies: "class M extends Array { static get [Symbol.species]() { return Array; } } a = M.from(a);",
  subclassObj: "class M extends Array { static get [Symbol.species]() { return Object; } } a = M.from(a);",
  sameCtor: "a.constructor = Array;",
  objectCtor: "a.constructor = Object;",
  boundArray: "a.constructor = Array.bind(null);",
  proxyArray: "a.constructor = new Proxy(Array, {});",
};
const speciesMethods = ["map(function (x) { return x; })", "filter(function () { return true; })", "slice(0)", "slice(1, 2)", "splice(0, 1)", "concat([4])", "flat()", "flatMap(function (x) { return [x]; })"];
for (const [name, setup] of Object.entries(speciesCtors)) {
  for (const m of speciesMethods) {
    F(`var a = [1, 2, 3];`, setup, `var r = a.${m};`, `return [r, Array.isArray(r), Object.getPrototypeOf(r) === Array.prototype, r.length];`);
  }
}
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.map(function (x) { return x; }); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.filter(function (x) { return x > 1; }); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.slice(1); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.splice(1, 1); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.concat([1]); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.flat(); return args;`);
F(`var a = [1, 2, 3]; var args;`, `a.constructor = {[Symbol.species]: function () { args = Array.prototype.slice.call(arguments); return []; }};`, `a.flatMap(function (x) { return x; }); return args;`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return Object.freeze([]); }};`, `return a.map(function (x) { return x; });`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return Object.freeze([]); }};`, `return a.slice(0, 0);`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return Object.freeze([]); }};`, `return a.splice(0, 0);`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return Object.freeze([]); }};`, `return a.splice(0, 1);`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { var r = []; Object.defineProperty(r, 'length', {writable: false}); return r; }};`, `return a.slice(0, 0);`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { var r = []; Object.defineProperty(r, 'length', {writable: false}); return r; }};`, `return a.slice(0, 1);`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 0}; }};`, `return S(a.splice(0, 2));`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 0}; }};`, `var r = a.slice(1); return [r.length, r[0], r[1]];`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 99}; }};`, `var r = a.slice(1); return [r.length, r[0], r[1]];`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 99}; }};`, `var r = a.filter(function (x) { return x > 1; }); return [r.length, r[0], r[1]];`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 99}; }};`, `var r = a.map(function (x) { return x * 2; }); return [r.length, r[0], r[2]];`);
F(`var a = [1, , 3];`, `a.constructor = {[Symbol.species]: function () { return {length: 0}; }};`, `var r = a.map(function (x) { return x; }); return [Object.keys(r).join()];`);
F(`var a = [1, , 3];`, `a.constructor = {[Symbol.species]: function () { return {}; }};`, `var r = a.slice(); return [Object.keys(r).join(), r.length];`);
F(`var a = [1, , 3];`, `a.constructor = {[Symbol.species]: function () { return {}; }};`, `var r = a.splice(0, 3); return [Object.keys(r).join(), r.length];`);
F(`var a = [1, 2, 3];`, `a.constructor = {[Symbol.species]: function () { return new Proxy([], {defineProperty() { return false; }}); }};`, `return a.slice(0);`);
F(`var a = {length: 3, 0: 1, 1: 2, 2: 3, constructor: {[Symbol.species]: function () { return ['obj']; }}};`, `return ${P}.slice.call(a, 1);`);
F(`var a = {length: 3, 0: 1, 1: 2, 2: 3};`, `return ${P}.slice.call(a, 1);`);
F(`var a = {length: 3, 0: 1, 1: 2, 2: 3, constructor: Array};`, `return ${P}.map.call(a, function (x) { return x; });`);
F(`var a = new Proxy([1, 2, 3], {});`, `a.constructor = {[Symbol.species]: function () { return ['px']; }};`, `return a.slice(1);`);
F(`var r = Proxy.revocable([1, 2], {}); r.revoke();`, `return ${P}.slice.call(r.proxy);`);
F(`var a = [1, 2, 3]; a.constructor = {[Symbol.species]: function (n) { return new Array(n + 2); }};`, `return a.slice(1);`);
F(`var a = [1, 2, 3]; a.constructor = {[Symbol.species]: function (n) { return new Array(n + 2); }};`, `return a.map(function (x) { return x; });`);
F(`class M extends Array {}`, `var m = new M(1, 2, 3); var r = m.map(function (x) { return x; }); return [r instanceof M, r.length, m.slice(1) instanceof M, m.filter(Boolean) instanceof M, m.concat() instanceof M, m.flat() instanceof M, m.splice(0, 1) instanceof M, m.toSorted() instanceof M, m.toReversed() instanceof M, m.with(0, 1) instanceof M, m.toSpliced(0, 0) instanceof M];`);
F(`class M extends Array { constructor(n) { super(n); this.made = n; } }`, `var m = M.from([1, 2, 3]); var r = m.map(function (x) { return x; }); return [r.made, r.length, M.of(1, 2).made, M.from({length: 4}).made, M.from(new Set([1])).made];`);
F(`class M extends Array { static get [Symbol.species]() { return Array; } }`, `var m = M.from([1, 2, 3]); return [m.map(function (x) { return x; }) instanceof M, m.slice() instanceof M, Array.isArray(m.filter(Boolean))];`);
F(`class M extends Array {}`, `var m = new M(3); return [m.length, m.fill(1).concat([2]).length, Object.getPrototypeOf(m.concat([2])) === M.prototype];`);
F(`class M extends Array {}`, `var m = M.of(1, 2); m[Symbol.isConcatSpreadable] = false; return [m.concat([3]).length, [0].concat(m).length];`);

// ---- 10. buracos e protótipo com índices.
const holeMethods = ["forEach", "map", "filter", "some", "every", "find", "findIndex", "findLast", "findLastIndex", "reduce", "reduceRight", "flatMap"];
for (const m of holeMethods) {
  const cb = ["reduce", "reduceRight"].includes(m) ? "function (acc, x, i) { seen.push(i + ':' + String(x)); return acc; }, 0" : "function (x, i) { seen.push(i + ':' + String(x)); return false; }";
  F(`var seen = [];`, `[1, , 3, , ].${m}(${cb});`, `return seen;`);
  F(`var seen = [];`, `${P}[1] = 'p'; ${P}[3] = 'q'; try { [1, , 3, , ].${m}(${cb}); return seen; } finally { delete ${P}[1]; delete ${P}[3]; }`);
  F(`var seen = [];`, `${P}[0] = 'z'; try { new Array(3).${m}(${cb}); return seen; } finally { delete ${P}[0]; }`);
  F(`var seen = [];`, `Object.prototype[1] = 'op'; try { [1, , 3].${m}(${cb}); return seen; } finally { delete Object.prototype[1]; }`);
  F(`var seen = [];`, `var a = [1, 2, 3]; delete a[1];`, `a.${m}(${cb}); return seen;`);
}
for (const m of ["join", "toString", "toLocaleString", "reverse", "toReversed", "toSorted", "keys", "values", "entries", "at", "concat", "slice", "fill", "lastIndexOf", "indexOf", "includes", "pop", "shift"]) {
  F(`var a = [1, , 3, , 5];`, `${P}[1] = 'p'; try { var r = a.${m}(); return [S(typeof r === 'object' && r && !Array.isArray(r) && r[Symbol.iterator] ? Array.from(r) : r), Object.keys(a).join()]; } finally { delete ${P}[1]; }`);
}
E(`[, 1, , 2, , ].join('-')`);
E(`[, , ].join()`);
E(`[null, undefined, , 1].join('|')`);
E(`[null, undefined, , 1].toString()`);
E(`[, 'a', , ].toLocaleString()`);
E(`[1, , 3].reverse()`);
E(`[, 1].reverse()`);
E(`[1, , , 4].reverse()`);
E(`[1, , 3, , 5].reverse()`);
E(`${P}.reverse.call({length: 4, 0: 'a', 3: 'd'})`);
E(`${P}.reverse.call({length: 5, 1: 'b', 2: 'c'})`);
F(`var o = {length: 4, 0: 'a', 3: 'd'};`, `${P}.reverse.call(o);`, `return o;`);
F(`var o = {length: 5, 1: 'b', 2: 'c'};`, `${P}.reverse.call(o);`, `return o;`);
F(`var o = {length: ${MAX}, 0: 'a', [2**53-2]: 'z'};`, `try { return ${P}.reverse.call({length: 2, 0: 'a'}); } finally { }`);
F(`var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false, writable: false, value: 2});`, `return a.reverse();`);
F(`var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false, writable: true, value: 2});`, `return a.reverse();`);
F(`var a = [1, , 3]; Object.defineProperty(a, 1, {get() { return 'g'; }, configurable: true});`, `return a.reverse();`);
F(`var a = [1, , 3]; Object.defineProperty(a, 1, {get() { return 'g'; }, set(v) { }, configurable: true});`, `a.reverse(); return [a[0], a[2], 1 in a];`);
E(`[, 1, , 2].shift()`);
E(`[, , 1].unshift(0)`);
F(`var a = [, , 1]; a.unshift(0);`, `return [a, Object.keys(a).join()];`);
F(`var a = [1, , 3]; a.unshift(0, 0);`, `return [a, Object.keys(a).join()];`);
F(`var a = [1, , 3]; a.shift();`, `return [a, Object.keys(a).join()];`);
F(`var a = [1, , 3, , 5]; a.pop(); a.pop();`, `return [a, Object.keys(a).join(), a.length];`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { a.shift(); return [a, Object.keys(a).join()]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { a.unshift(0); return [a, Object.keys(a).join()]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { var r = a.splice(0, 3); return [r, Object.keys(r).join(), a]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { var r = a.slice(); return [r, Object.keys(r).join()]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { var r = a.concat([4]); return [r, Object.keys(r).join()]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { var r = a.map(function (x) { return x; }); return [r, Object.keys(r).join()]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { var r = a.filter(function () { return true; }); return r; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { return a.flat(); } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { return [Array.from(a), [...a], Array.from(a.entries()).join('|')]; } finally { delete ${P}[1]; }`);
F(`var a = [1, , 3]; ${P}[1] = 'p';`, `try { return [Object.keys(a).join(), JSON.stringify(a), 1 in a, a.hasOwnProperty(1)]; } finally { delete ${P}[1]; }`);
F(`var a = new Array(5);`, `return [a.length, Object.keys(a).length, a.indexOf(undefined), a.includes(undefined), a.findIndex(function (x) { return x === undefined; }), a.join().length, a.fill(0, 1, 3)];`);
F(`var a = [1, , 3];`, `return [a.map(function () { return 1; }), a.filter(function () { return true; }), a.slice(), a.concat()].map(function (x) { return Object.keys(x).join(); });`);
F(`var a = [, 'a'];`, `return [a.flat(), a.flatMap(function (x) { return x; }), [[, 1], [, 2]].flat()].map(function (x) { return Object.keys(x).join() + '/' + x.length; });`);
F(`var a = [1, 2, 3, 4]; delete a[1]; a.length = 6;`, `return [Object.keys(a).join(), a.lastIndexOf(undefined), a.includes(undefined), a.indexOf(undefined)];`);
F(`var a = []; a[2 ** 32 - 2] = 'x';`, `return [a.length, Object.keys(a).join(), a.at(-1), a.lastIndexOf('x')];`);
F(`var a = []; a[2 ** 32 - 1] = 'x';`, `return [a.length, Object.keys(a).join(), a.at(-1), a[2 ** 32 - 1]];`);
F(`var a = []; a[2 ** 32 - 2] = 'x';`, `return a.push('y');`);
F(`var a = []; a[2 ** 32 - 2] = 'x';`, `a.push('y'); return [a.length, a[2 ** 32 - 1]];`);
F(`var a = []; a[2 ** 32 - 2] = 'x';`, `try { a.push('y', 'z'); } catch (e) { return [e.name, e.message, a.length, a[2 ** 32 - 1], a[2 ** 32]]; }`);
F(`var a = new Array(2 ** 32 - 1);`, `try { a.push(1); } catch (e) { return [e.name, e.message, a.length, a[2 ** 32 - 1]]; }`);
F(`var a = new Array(2 ** 32 - 1);`, `try { return a.unshift(); } catch (e) { return [e.name, e.message]; }`);
F(`var o = {length: ${MAX}};`, `try { return ${P}.push.call(o, 1); } catch (e) { return [e.name, e.message, o.length]; }`);
F(`var o = {length: ${MAX} - 1};`, `try { return [${P}.push.call(o, 1), o.length, o[${MAX} - 1]]; } catch (e) { return [e.name, e.message, o.length]; }`);
F(`var o = {length: ${MAX} - 1};`, `try { return [${P}.push.call(o, 1, 2), o.length]; } catch (e) { return [e.name, e.message, o.length, Object.keys(o).join()]; }`);
F(`var o = {length: ${MAX}};`, `try { return ${P}.unshift.call(o, 1); } catch (e) { return [e.name, e.message, o.length]; }`);
F(`var o = {length: ${MAX}};`, `try { return ${P}.unshift.call(o); } catch (e) { return [e.name, e.message, o.length]; }`);
F(`var o = {length: ${MAX}};`, `try { return ${P}.pop.call(o); } catch (e) { return [e.name, e.message]; } finally { }`);
F(`var o = {length: ${MAX}, [2**53-2]: 'last'};`, `var r = ${P}.pop.call(o); return [r, o.length];`);
F(`var o = {length: 2 ** 53 + 10, [2**53-2]: 'last'};`, `var r = ${P}.pop.call(o); return [r, o.length];`);
F(`var o = {length: -5};`, `var r = ${P}.pop.call(o); return [r, o.length];`);
F(`var o = {length: 'abc'};`, `var r = ${P}.pop.call(o); return [r, o.length];`);
F(`var o = {length: Infinity};`, `var r = ${P}.pop.call(o); return [r, o.length];`);

// ---- 11. Symbol.isConcatSpreadable.
const spreadables = {
  arrTrue: "[1, 2]",
  arrFalse: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = false; return a; })()",
  arrUndef: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = undefined; return a; })()",
  arrNull: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = null; return a; })()",
  arrZero: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = 0; return a; })()",
  arrStr: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = ''; return a; })()",
  arrEmptyStr: "(function () { var a = [1, 2]; a[Symbol.isConcatSpreadable] = 'no'; return a; })()",
  objTrue: "{[Symbol.isConcatSpreadable]: true, length: 2, 0: 'a', 1: 'b'}",
  objTrueNoLen: "{[Symbol.isConcatSpreadable]: true, 0: 'a'}",
  objTrueHoles: "{[Symbol.isConcatSpreadable]: true, length: 3, 0: 'a', 2: 'c'}",
  objTrueStrLen: "{[Symbol.isConcatSpreadable]: true, length: '2', 0: 'a', 1: 'b'}",
  objTrueNegLen: "{[Symbol.isConcatSpreadable]: true, length: -2, 0: 'a'}",
  objTrueNaNLen: "{[Symbol.isConcatSpreadable]: true, length: NaN, 0: 'a'}",
  objTrueFracLen: "{[Symbol.isConcatSpreadable]: true, length: 1.9, 0: 'a', 1: 'b'}",
  objOne: "{[Symbol.isConcatSpreadable]: 1, length: 1, 0: 'a'}",
  objStr: "{[Symbol.isConcatSpreadable]: 'x', length: 1, 0: 'a'}",
  objFalse: "{[Symbol.isConcatSpreadable]: false, length: 1, 0: 'a'}",
  objNoFlag: "{length: 1, 0: 'a'}",
  fnTrue: "Object.assign(function () {}, {[Symbol.isConcatSpreadable]: true, length: 1, 0: 'a'})",
  strObj: "Object.assign(new String('ab'), {[Symbol.isConcatSpreadable]: true})",
  strPrim: "'ab'",
  protoTrue: "Object.create({[Symbol.isConcatSpreadable]: true, length: 1, 0: 'p'})",
  proxyArr: "new Proxy([1, 2], {})",
  proxyObjTrue: "new Proxy({length: 1, 0: 'q'}, {get(t, k) { return k === Symbol.isConcatSpreadable ? true : t[k]; }})",
  proxyArrFalse: "new Proxy([1, 2], {get(t, k) { return k === Symbol.isConcatSpreadable ? false : t[k]; }})",
  getterThrows: "{get [Symbol.isConcatSpreadable]() { throw new Error('ics'); }}",
  lengthThrows: "{[Symbol.isConcatSpreadable]: true, get length() { throw new Error('len'); }}",
  elemThrows: "{[Symbol.isConcatSpreadable]: true, length: 1, get 0() { throw new Error('el'); }}",
  typed: "new Uint8Array([1, 2])",
  typedTrue: "Object.assign(new Uint8Array([1, 2]), {[Symbol.isConcatSpreadable]: true})",
  hugeLen: "{[Symbol.isConcatSpreadable]: true, length: 2 ** 53 - 1}",
  hugeLen2: "{[Symbol.isConcatSpreadable]: true, length: 2 ** 53}",
  holesArr: "[1, , 3]",
  emptyArr: "[]",
  nested: "[[1], [2]]",
  nullArg: "null",
  undefArg: "undefined",
  symArg: "Symbol('s')",
  bigArg: "1n",
};
for (const [name, v] of Object.entries(spreadables)) {
  if (name.startsWith("huge")) {
    E(`[0].concat(${v})`);
    continue;
  }
  E(`[0].concat(${v})`);
  E(`[].concat(${v}, [9])`);
  E(`${v === "null" || v === "undefined" ? "[]" : v}.concat === undefined ? 0 : T(function () { return ${P}.concat.call(${v}, [7]); })`);
  F(`var x = ${v};`, `var r = [0].concat(x); return [r, Object.keys(r).join(), r.length];`);
}
F(`var o = {[Symbol.isConcatSpreadable]: true, length: 2 ** 53 - 1};`, `try { return [0].concat(o).length; } catch (e) { return [e.name, e.message]; }`);
F(`var o = {[Symbol.isConcatSpreadable]: true, length: 3, 0: 'a', 2: 'c'};`, `var r = [].concat(o); return [r.length, Object.keys(r).join()];`);
F(`var log = [];`, `var o = {get [Symbol.isConcatSpreadable]() { log.push('flag'); return true; }, get length() { log.push('length'); return 2; }, get 0() { log.push('0'); return 'a'; }, get 1() { log.push('1'); return 'b'; }};`, `[1].concat(o, o); return log;`);
F(`var log = [];`, `var p = new Proxy([1, 2], {get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); }, has(t, k) { log.push('has:' + String(k)); return k in t; }, getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }});`, `[].concat(p); return log;`);
F(`var r = Proxy.revocable([1], {}); r.revoke();`, `return [0].concat(r.proxy);`);
F(`Array.prototype[Symbol.isConcatSpreadable] = false;`, `try { return [[1, 2].concat([3]).length, [].concat([1]).length, [0].concat(Array.prototype.slice.call([5, 6])).length]; } finally { delete Array.prototype[Symbol.isConcatSpreadable]; }`);
F(`Object.prototype[Symbol.isConcatSpreadable] = true;`, `try { return [[1].concat({length: 2, 0: 'a'}).length, [1].concat(function () {}).length, [1].concat(new Date(0)).length]; } finally { delete Object.prototype[Symbol.isConcatSpreadable]; }`);
F(`Number.prototype[Symbol.isConcatSpreadable] = true; Number.prototype.length = 2; Number.prototype[0] = 'n';`, `try { return [1].concat(5, 6); } finally { delete Number.prototype[Symbol.isConcatSpreadable]; delete Number.prototype.length; delete Number.prototype[0]; }`);
F(`var a = [1, 2]; a.constructor = {[Symbol.species]: function () { return {length: 0}; }};`, `return S(a.concat([3]));`);
F(`var a = [1, 2];`, `return [a.concat(a, a).length, a.concat([[3]]).length, a.concat([], [], []).length, a.concat().length, a.concat(undefined).length, a.concat(null).length, a.concat([undefined]).length];`);
F(`var a = [1, 2];`, `return a.concat(...[[3], [4]]);`);
E(`[].concat.length + [].concat.name`);
E(`${P}.concat.call(1, 2)`);
E(`${P}.concat.call('s', 't')`);
E(`${P}.concat.call(null)`);
E(`${P}.concat.call(true, false).length`);
E(`${P}.concat.call({length: 1, 0: 'a'}, [1])`);
E(`[].concat.call(new Boolean(true))`);
E(`Array.isArray(Symbol.isConcatSpreadable)`);
E(`typeof Symbol.isConcatSpreadable + String(Symbol.isConcatSpreadable)`);
E(`Object.getOwnPropertyDescriptor(Symbol, 'isConcatSpreadable').writable`);

// ---- 12. Array.from, Array.of e iterables.
const iterables = {
  arr: "[1, 2, 3]",
  holes: "[1, , 3]",
  str: "'abc'",
  strSur: "'a\\ud83d\\ude00b'",
  strLone: "'\\ud800x\\udc00'",
  set: "new Set([1, 2, 2, 3])",
  map: "new Map([[1, 'a'], [2, 'b']])",
  mapKeys: "new Map([[1, 'a'], [2, 'b']]).keys()",
  gen: "(function* () { yield 1; yield 2; yield 3; })()",
  genThrow: "(function* () { yield 1; throw new Error('g'); })()",
  genReturn: "(function* () { yield 1; return 9; })()",
  typed: "new Uint8Array([1, 2, 3])",
  args: "(function () { return arguments; })(1, 2, 3)",
  arrayLike: "{length: 3, 0: 'a', 1: 'b', 2: 'c'}",
  arrayLikeHoles: "{length: 3, 0: 'a', 2: 'c'}",
  arrayLikeStr: "{length: '2', 0: 'a', 1: 'b'}",
  arrayLikeNeg: "{length: -1, 0: 'a'}",
  arrayLikeNaN: "{length: NaN, 0: 'a'}",
  arrayLikeFrac: "{length: 2.9, 0: 'a', 1: 'b', 2: 'c'}",
  arrayLikeObjLen: "{length: {valueOf() { return 2; }}, 0: 'a', 1: 'b'}",
  arrayLikeNoLen: "{0: 'a'}",
  arrayLikeInf: "{length: Infinity}",
  arrayLikeBig: "{length: 2 ** 32}",
  num: "5",
  bool: "true",
  nul: "null",
  undef: "undefined",
  sym: "Symbol('s')",
  big: "3n",
  fn: "function (a, b) {}",
  emptyObj: "{}",
  iterNull: "{[Symbol.iterator]: null, length: 2, 0: 'x', 1: 'y'}",
  iterUndef: "{[Symbol.iterator]: undefined, length: 1, 0: 'x'}",
  iterNum: "{[Symbol.iterator]: 1, length: 1, 0: 'x'}",
  iterObj: "{[Symbol.iterator]: {}, length: 1, 0: 'x'}",
  iterThrows: "{get [Symbol.iterator]() { throw new Error('gi'); }}",
  iterRetPrim: "{[Symbol.iterator]() { return 1; }}",
  iterRetNoNext: "{[Symbol.iterator]() { return {}; }}",
  iterNextNotFn: "{[Symbol.iterator]() { return {next: 1}; }}",
  iterNextRetPrim: "{[Symbol.iterator]() { return {next() { return 1; }}; }}",
  iterNextThrows: "{[Symbol.iterator]() { return {next() { throw new Error('nx'); }}; }}",
  iterDoneGetterThrows: "{[Symbol.iterator]() { return {next() { return {get done() { throw new Error('dn'); }}; }}; }}",
  iterValueGetterThrows: "{[Symbol.iterator]() { return {next() { return {done: false, get value() { throw new Error('vl'); }}; }}; }}",
  iterCustom: "{[Symbol.iterator]() { var i = 0; return {next() { return i < 3 ? {value: i++, done: false} : {done: true, value: 'x'}; }}; }}",
  iterNextCached: "{[Symbol.iterator]() { var i = 0; var it = {next() { return {value: i++, done: i > 3}; }}; return it; }}",
  iterDoneTruthy: "{[Symbol.iterator]() { var i = 0; return {next() { return {value: i++, done: i > 2 ? 'yes' : 0}; }}; }}",
  iterNoValue: "{[Symbol.iterator]() { var i = 0; return {next() { return i++ < 2 ? {done: false} : {done: true}; }}; }}",
  iterAlsoLength: "{length: 5, [Symbol.iterator]: function* () { yield 'it'; }}",
  strObj: "new String('xyz')",
  strObjSur: "new String('\\ud83d\\ude00')",
};
for (const [name, v] of Object.entries(iterables)) {
  E(`Array.from(${v})`);
  E(`Array.from(${v}, function (x, i) { return [x, i]; })`);
  E(`Array.from(${v}, function () { return arguments.length; })`);
}
const mapFns = {
  id: "function (x) { return x; }",
  index: "function (x, i) { return i; }",
  thisCheck: "function () { return typeof this; }",
  thisStrict: "function () { 'use strict'; return typeof this; }",
  arrow: "(x) => [x]",
  nan: "function () { return NaN; }",
  throws: "function () { throw new Error('mf'); }",
  hole: "function () { }",
  promise: "async function (x) { return x; }",
  gen: "function* (x) { yield x; }",
};
for (const [name, fn] of Object.entries(mapFns)) {
  for (const src of ["[1, 2]", "'ab'", "new Set([1])", "{length: 2, 0: 'a', 1: 'b'}", "[1, , 3]"]) {
    E(`Array.from(${src}, ${fn}, 'T')`);
    E(`Array.from(${src}, ${fn}, {tag: 1}).length`);
  }
}
for (const bad of ["null", "1", "'f'", "{}", "[]", "true", "Symbol()", "class {}", "0", "NaN", "{call() {}}"]) {
  E(`Array.from([1], ${bad})`);
  E(`Array.from({length: 1}, ${bad})`);
  E(`Array.from(null, ${bad})`);
  E(`Array.from(undefined, ${bad})`);
}
E(`Array.from([1], undefined)`);
E(`Array.from([1], undefined, 'x')`);
E(`Array.from()`);
E(`Array.from.length + Array.from.name + Array.of.length + Array.of.name`);
E(`Array.from.call(undefined, [1, 2])`);
E(`Array.from.call(null, [1, 2])`);
E(`Array.from.call(1, [1, 2])`);
E(`Array.from.call('x', [1, 2])`);
E(`Array.from.call({}, [1, 2])`);
E(`Array.from.call(function () {}, [1, 2])`);
E(`Array.from.call(() => {}, [1, 2])`);
E(`Array.from.call(Math.max, [1, 2])`);
E(`Array.from.call(Symbol, [1, 2])`);
E(`Array.from.call(Object, [1, 2])`);
E(`Array.from.call(Object, {length: 2, 0: 'a'})`);
E(`Array.from.call(String, [1, 2])`);
E(`Array.from.call(Number, {length: 1})`);
E(`Array.from.call(Date, {length: 1})`);
E(`Array.from.call(Map, [[1, 2]])`);
E(`Array.from.call(Array, [1, 2]).length`);
E(`Array.from.call(Uint8Array, [1, 2, 300])`);
E(`Array.from.call(Uint8Array, {length: 2, 0: 5})`);
E(`Array.from.call(Uint8Array, 'abc')`);
E(`Array.from.call(Uint8Array, new Set([7]))`);
E(`Array.from.call(Int8Array, [200], function (x) { return x; })`);
E(`Array.from.call(new Proxy(Array, {}), [1, 2])`);
E(`Array.from.call(Array.bind(null), [1, 2])`);
E(`Array.from.call(class extends Array {}, [1, 2]) instanceof Array`);
for (const ctor of ["function () { return {}; }", "function () { return {length: 0}; }", "function () { return Object.freeze([]); }", "function () { return new Proxy([], {defineProperty() { return false; }}); }", "function () { this.made = arguments.length + ':' + Array.prototype.join.call(arguments); }", "function () { return 1; }", "function () { var r = []; Object.defineProperty(r, 'length', {writable: false}); return r; }", "function () { return {set length(v) { throw new Error('setlen'); }}; }", "function () { return {get 0() { return 1; }}; }", "function () { return Object.defineProperty([], 0, {value: 1, configurable: false, writable: false}); }", "function () { return []; }"]) {
  F(`var C = ${ctor};`, `var r = Array.from.call(C, [1, 2]); return [S(r), r.length, Object.keys(r).join()];`);
  F(`var C = ${ctor};`, `var r = Array.from.call(C, {length: 2, 0: 'a', 1: 'b'}); return [S(r), r.length, Object.keys(r).join()];`);
  F(`var C = ${ctor};`, `var r = Array.from.call(C, [1, 2], function (x) { return x * 2; }); return [S(r), r.length];`);
  F(`var C = ${ctor};`, `var r = Array.of.call(C, 1, 2); return [S(r), r.length];`);
  F(`var C = ${ctor};`, `var r = Array.of.call(C); return [S(r), r.length];`);
}
// Fechamento do iterador quando o mapFn ou a definição lançam.
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); return {}; }}; }};`, `try { Array.from(it, function (x) { if (x === 1) throw new Error('mf'); return x; }); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); throw new Error('ret'); }}; }};`, `try { Array.from(it, function (x) { if (x === 1) throw new Error('mf'); return x; }); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); return 1; }}; }};`, `try { Array.from(it, function (x) { if (x === 1) throw new Error('mf'); return x; }); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return: null}; }};`, `try { Array.from(it, function (x) { if (x === 1) throw new Error('mf'); return x; }); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, get return() { log.push('getret'); return function () { log.push('return'); return {}; }; }}; }};`, `try { Array.from(it, function (x) { if (x === 1) throw new Error('mf'); return x; }); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); return {}; }}; }};`, `Array.from(it); return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); return {}; }}; }};`, `var C = function () { return Object.freeze([]); }; try { Array.from.call(C, it); } catch (e) { log.push(e.name); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {value: i++, done: i > 3}; }, return() { log.push('return'); return {}; }}; }};`, `var C = function () { return {set length(v) { throw new Error('sl'); }}; }; try { Array.from.call(C, it); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); if (i === 1) throw new Error('nx'); return {value: i++, done: false}; }, return() { log.push('return'); return {}; }}; }};`, `try { Array.from(it); } catch (e) { log.push(e.message); } return log;`);
F(`var log = [];`, `var it = {[Symbol.iterator]() { var i = 0; return {next() { log.push('next'); return {get done() { log.push('done'); return i++ > 1; }, get value() { log.push('value'); return i; }}; }}; }};`, `Array.from(it, function (x) { log.push('map' + x); return x; }); return log;`);
F(`var log = [];`, `var a = [1, 2, 3]; var orig = Array.prototype[Symbol.iterator];`, `Array.prototype[Symbol.iterator] = function () { log.push('patched'); return orig.call(this); }; try { return [Array.from(a), log]; } finally { Array.prototype[Symbol.iterator] = orig; }`);
F(`var a = [1, 2, 3]; var orig = Array.prototype[Symbol.iterator];`, `Array.prototype[Symbol.iterator] = undefined; try { return Array.from(a); } finally { Array.prototype[Symbol.iterator] = orig; }`);
F(`var a = [1, 2, 3]; var proto = Object.getPrototypeOf([][Symbol.iterator]()); var orig = proto.next;`, `proto.next = function () { return {done: true}; }; try { return [Array.from(a), [...a]]; } finally { proto.next = orig; }`);
F(`var a = [1, 2, 3];`, `var seen = []; Array.from(a, function (x, i) { seen.push(x); if (i === 0) a.push(4); return x; }); return seen;`);
F(`var a = [1, 2, 3];`, `var seen = []; Array.from({length: 3, get 0() { seen.push(0); return 'a'; }, get 1() { seen.push(1); return 'b'; }, get 2() { seen.push(2); return 'c'; }}, function (x, i) { seen.push('m' + i); return x; }); return seen;`);
F(`var order = [];`, `Array.from({get length() { order.push('length'); return 1; }, get 0() { order.push('0'); return 'a'; }}, function (x) { order.push('map'); return x; }); return order;`);
F(`var order = [];`, `Array.from.call(function (n) { order.push('ctor:' + n); return []; }, {get length() { order.push('length'); return 2; }, 0: 'a', 1: 'b'}); return order;`);
F(`var order = [];`, `Array.from.call(function () { order.push('ctor:' + arguments.length); return []; }, [1, 2]); return order;`);
F(`var order = [];`, `Array.from.call(function () { order.push('ctor:' + arguments.length); return []; }, 'ab'); return order;`);
F(`var order = [];`, `Array.from.call(function () { order.push('ctor:' + arguments.length); return []; }, new Set([1])); return order;`);
F(`var order = [];`, `Array.from.call(function (n) { order.push('ctor:' + n); return []; }, {length: 2 ** 53}); return order;`);
F(`var r = Array.from({length: 2 ** 32});`, `return r;`);
F(`var r;`, `try { r = Array.from({length: 4294967296}); } catch (e) { r = [e.name, e.message]; } return r;`);
E(`Array.of()`);
E(`Array.of(undefined)`);
E(`Array.of(3)`);
E(`Array.of(1, 2, 3).length`);
E(`Array.of.call(undefined, 1)`);
E(`Array.of.call(null, 1)`);
E(`Array.of.call(1, 1)`);
E(`Array.of.call(Object, 1, 2)`);
E(`Array.of.call(String, 1, 2)`);
E(`Array.of.call(Uint8Array, 1, 2, 3)`);
E(`Array.of.call(function (n) { this.n = n; }, 'a', 'b')`);
E(`Array.of.call(function (n) { return {length: 5}; }, 'a')`);
E(`Array.of.call(function (n) { return Object.freeze([]); }, 'a')`);
E(`Array.of.call(class extends Array {}, 1, 2) instanceof Array`);
E(`Array.of.apply(null, new Array(3))`);
E(`Array.of(...'a\\ud83d\\ude00')`);
E(`Array(3).length + Array(3, 4).length + Array('3').length + Array.apply(null, {length: 2}).length`);
E(`new Array(-1)`);
E(`new Array(1.5)`);
E(`new Array(2 ** 32)`);
E(`new Array(2 ** 32 - 1).length`);
E(`new Array('1', 2).length`);
E(`new Array(NaN)`);
E(`new Array(Infinity)`);
E(`new Array(undefined).length`);
E(`new Array(null).length`);
E(`new Array(1n)`);
E(`new Array(-0).length`);
E(`Array.isArray(new Array(...[1, 2]))`);
E(`Array.isArray(Array.prototype) + ',' + Array.prototype.length + ',' + Object.prototype.toString.call(Array.prototype)`);
E(`Array.isArray(new Proxy([], {})) + ',' + Array.isArray(new Proxy({}, {}))`);
E(`(function () { var r = Proxy.revocable([], {}); r.revoke(); return Array.isArray(r.proxy); })()`);
E(`Array[Symbol.species] === Array`);
E(`Object.getOwnPropertyDescriptor(Array, Symbol.species).get.name`);
E(`Object.getOwnPropertyDescriptor(Array, Symbol.species).set`);
E(`Array.prototype.concat.call(Array[Symbol.species], 1).length`);

// ---- 13. Combinações de length gigante em outros métodos curtos.
for (const m of ["fill", "slice", "indexOf", "lastIndexOf", "includes", "at", "with", "keys", "entries", "values"]) {
  E(`T(function () { var it = ${P}.${m}.call({length: ${MAX}}, ${m === "slice" ? `${MAX} - 2` : m === "fill" ? `1, ${MAX} - 2` : m === "indexOf" || m === "includes" ? `undefined, ${MAX} - 1` : m === "lastIndexOf" ? `undefined, 1` : m === "at" ? "-1" : m === "with" ? "0, 1" : "''"}); return typeof it === 'object' && it && it[Symbol.iterator] && !Array.isArray(it) ? [it.next().value] : it; })`);
}
E(`${P}.slice.call({length: ${MAX}}, ${MAX} - 3).length`);
E(`${P}.slice.call({length: ${MAX}, [2**53-2]: 'x'}, -1)`);
E(`${P}.slice.call({length: 2 ** 53 + 100, [2**53-2]: 'x'}, -1)`);
E(`${P}.slice.call({length: 2 ** 32 + 2, [2 ** 32 + 1]: 'x'}, -1)`);
E(`${P}.slice.call({length: 2 ** 32 + 2, [2 ** 32 + 1]: 'x'}, -2, 2 ** 32 + 2)`);
E(`${P}.slice.call({length: ${MAX}}, 2 ** 32 - 1, 2 ** 32 + 1)`);
E(`${P}.slice.call({length: ${MAX}}, 2 ** 32 - 2, 2 ** 32 + 1)`);
E(`${P}.fill.call({length: ${MAX}}, 'f', ${MAX} - 2)`);
E(`${P}.fill.call({length: ${MAX}}, 'f', ${MAX} - 2, ${MAX} + 5)`);
E(`${P}.fill.call({length: 3}, 'f', -2)`);
E(`${P}.fill.call({length: 3}, 'f', 1, 2)`);
E(`${P}.fill.call({length: 3}, 'f', 5)`);
E(`${P}.fill.call({length: 3}, 'f', undefined, undefined)`);
E(`${P}.fill.call({length: 3}, 'f', undefined, null)`);
E(`${P}.fill.call({length: 3}, 'f', NaN, 2)`);
E(`${P}.fill.call({length: 3}, 'f', -Infinity, Infinity)`);
E(`${P}.fill.call('abc', 'f')`);
E(`${P}.fill.call(Object.freeze([1]), 0)`);
E(`${P}.fill.call(Object.freeze([]), 0)`);
E(`${P}.fill.call(Object.freeze([1]), 0, 1)`);
E(`${P}.join.call({length: 3, 0: 'a', 2: 'c'}, '-')`);
E(`${P}.join.call({length: 3, 0: null, 1: undefined, 2: 0}, '-')`);
E(`${P}.join.call({length: 0}, {toString() { throw new Error('sep'); }})`);
E(`${P}.join.call({length: 1, 0: 'a'}, {toString() { throw new Error('sep'); }})`);
E(`${P}.join.call({length: 2, 0: 'a', 1: 'b'}, {toString() { throw new Error('sep'); }})`);
E(`[1, 2].join(undefined) + [1, 2].join(null) + [1, 2].join(0) + [1, 2].join(Symbol.iterator.description)`);
E(`[1, 2].join(Symbol())`);
E(`[Symbol()].join()`);
E(`[1n, {toString() { return 'o'; }}, [2, [3]], () => 1].join(';')`);
F(`var a = [1, 2]; a.push(a);`, `return a.join();`);
F(`var a = [1, 2]; a.push([a, 3]);`, `return a.join('-');`);
F(`var a = [1, 2]; a.push(a);`, `return [a.toString(), String(a), a + ''];`);
F(`var a = [1, 2]; a.push(a);`, `return a.toLocaleString();`);
F(`var a = [1, [2, [3, a]]]; `, `a[1][1][1] = a; return a.join();`);
F(`var o = {toString() { return o2.join(); }}; var o2 = [1, o];`, `return o2.join();`);
F(`var a = [{toLocaleString() { return 'L'; }}, null, undefined, 1];`, `return a.toLocaleString();`);
F(`var a = [{toLocaleString() { return arguments.length + ':' + Array.prototype.join.call(arguments); }}];`, `return a.toLocaleString('pt-BR', {x: 1});`);
F(`var a = [1];`, `a.join = null; return String(a);`);
F(`var a = [1];`, `a.join = function () { return 'custom'; }; return [String(a), a.toString()];`);
F(`var a = [1];`, `a.join = 1; return Object.prototype.toString.call(a) + a.toString();`);
E(`${P}.toString.call({join() { return 'j'; }})`);
E(`${P}.toString.call({})`);
E(`${P}.toString.call(null)`);
E(`${P}.toString.call({join: 1})`);

// ---- 14. Iteradores e entries/keys/values.
F(`var a = [1, 2];`, `var it = a.values(); a.push(3); return [it.next().value, it.next().value, it.next().value, it.next().done];`);
F(`var a = [1, 2];`, `var it = a.values(); it.next(); it.next(); it.next(); a.push(3); return it.next();`);
F(`var a = [1, 2, 3];`, `var it = a.keys(); a.length = 1; return [it.next(), it.next()];`);
F(`var a = [1, 2, 3];`, `var it = a.entries(); return [it.next().value, it.next().value];`);
F(`var a = [1, , 3];`, `return Array.from(a.entries()).map(function (e) { return e.join(':'); }).join('|');`);
F(`var o = {length: 3, 0: 'a', 2: 'c'};`, `return Array.from(${P}.entries.call(o)).map(function (e) { return e.join(':'); }).join('|');`);
F(`var o = {length: 2 ** 53 - 1};`, `var it = ${P}.keys.call(o); return [it.next().value, it.next().value];`);
F(`var o = {length: 2 ** 53 + 10};`, `var it = ${P}.values.call(o); return [it.next().value, it.next().done];`);
F(`var o = {length: -1};`, `return ${P}.values.call(o).next();`);
E(`${P}.values.call(null)`);
E(`${P}.keys.call(1).next()`);
E(`${P}.values.call('ab').next()`);
E(`${P}.entries.call(undefined)`);
E(`[][Symbol.iterator] === [].values`);
E(`Object.getPrototypeOf([].keys()) === Object.getPrototypeOf([].values())`);
E(`Object.prototype.toString.call([].keys())`);
E(`Object.getPrototypeOf(Object.getPrototypeOf([].keys())) === Object.getPrototypeOf(Object.getPrototypeOf(new Set().values()))`);
E(`[].keys().next.call({})`);
E(`[].keys().next.call([].values())`);
E(`Object.getPrototypeOf([].keys()).next.call(new Set().values())`);
E(`typeof [][Symbol.iterator]().return`);
E(`[].values()[Symbol.iterator]() !== undefined`);
E(`[].values()[Symbol.iterator]()[Symbol.iterator] === [].values()[Symbol.iterator]`);

// ---- 15. Callbacks que mutam e outros iteradores de ordem alta.
for (const m of ["forEach", "map", "filter", "some", "every"]) {
  F(`var a = [1, 2, 3, 4]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) a.length = 2; return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) a.push(9); return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) { a[1] = 'm'; delete a[2]; } return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) a.reverse(); return false; });`, `return seen;`);
  F(`var a = [1, 2, 3]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) a.shift(); return false; });`, `return seen;`);
  F(`var a = [3, 2, 1]; var seen = [];`, `a.${m}(function (x, i) { seen.push(x); if (i === 0) a.sort(); return false; });`, `return seen;`);
  E(`${P}.${m}.call({length: ${MAX}, 0: 'a'}, function () { throw new Error('first'); })`);
  E(`${P}.${m}.call({length: 2 ** 53 + 3, 0: 'a'}, function () { throw new Error('first'); })`);
  E(`${P}.${m}.call({length: 3, 0: 'a', 2: 'c'}, function (x, i) { return typeof x; })`);
  E(`${P}.${m}.call('ab', function (x, i, a) { return typeof a; })`);
  E(`${P}.${m}.call(null, function () {})`);
  E(`[1].${m}()`);
}
for (const m of ["reduce", "reduceRight"]) {
  E(`[].${m}(function () {})`);
  E(`[, , ].${m}(function () {})`);
  E(`[].${m}(function () {}, undefined)`);
  E(`[5].${m}(function () { throw new Error('never'); })`);
  E(`[, 5, ,].${m}(function () { throw new Error('never'); })`);
  E(`[1, 2, 3].${m}(function (a, x, i, arr) { return a + ':' + x + ':' + i + ':' + arr.length; })`);
  E(`[1, , 3].${m}(function (a, x, i) { return a + ':' + x + ':' + i; }, 'init')`);
  E(`[1, 2].${m}(undefined)`);
  if (m === "reduceRight") {
    E(`${P}.${m}.call({length: ${MAX}, [2**53-2]: 'x'}, function (a, x) { return a + x; }, '').length`);
    E(`${P}.${m}.call({length: ${MAX}, [2**53-2]: 'x'}, function (a, x) { return a + x; })`);
  }
  E(`${P}.${m}.call({length: 2, 0: 'a', 1: 'b'}, function (a, x) { return a + x; })`);
  E(`${P}.${m}.call('abc', function (a, x) { return a + x; })`);
  F(`var a = [1, 2, 3, 4];`, `return a.${m}(function (acc, x, i) { if (i === 1 || i === 2) a.length = 1; return acc + ':' + x; });`);
}

// ---- Emissão: descarta repetidos e os que já estão em array_bun.tsv, depois amostra TARGET programas por hash
// (sampleByHash, do conjunto inteiro, antes de descontar o base). Cada programa roda no processo (modo estrito, prelúdio igual ao do base).
const TARGET = 600;
// Goldens regenerados guardam o fonte canônico: o conjunto reconhece o programa cru e o canônico.
const baseSources = knownProgramSet("array_edge_bun.tsv", ["array_bun.tsv"]);
const seen = new Set();
const everything = [];
let dup = 0;
for (const body of programs) {
  const source = '"use strict";\n' + PRELUDE + "\n" + body.replace(/\bR = /g, "globalThis.R = ");
  if (seen.has(source)) continue;
  seen.add(source);
  everything.push(source);
}
const candidates = sampleByHash(everything, TARGET).filter((source) => (baseSources.has(source) ? (dup++, false) : true));
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "array-edge-golden-"));
const file = path.join(dir, "array_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const runOnce = () => {
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 4000 });
  return (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
};
let kept = 0;
let dropped = 0;
for (const original of candidates) {
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const marked = runOnce();
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length + 14)).slice(0, 200) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(source.slice(PRELUDE.length + 14)).slice(0, 200) + "\n");
    continue;
  }
  // Resultado que depende do acaso ou do tempo não serve de golden.
  if (runOnce() !== marked) {
    dropped++;
    process.stderr.write("não determinístico: " + JSON.stringify(source.slice(PRELUDE.length + 14)).slice(0, 200) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`candidatos ${everything.length}, amostrados ${candidates.length}, mantidos ${kept}, descartados ${dropped}, repetidos do base ${dup}\n`);
process.stdout.write(emitFactoredLines("array_edge", lines));
fs.rmSync(dir, { recursive: true, force: true });

