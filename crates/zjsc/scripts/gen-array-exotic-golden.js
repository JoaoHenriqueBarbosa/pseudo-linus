// Gera tests/golden/array_exotic_bun.tsv: semântica exótica de Array, medida no bun.
// Cobre a atribuição a `length` (encolher com elementos não configuráveis, `length` não gravável, ToNumber/ToUint32 com
// `valueOf` logado e o RangeError "Invalid array length"), `defineProperty` de `length` com descritores variados,
// índices como chaves (2**32-2, 2**32-1, "-0", "01", "1.0"...), arrays esparsos e buracos em quase todos os métodos
// (`in`, `hasOwnProperty`, `forEach`, `map`, `sort` com buracos e `undefined`, `join`/`toString`), `Array(n)` com `n`
// inválido, estouro de comprimento em `push`/array-likes, `Array.prototype` como array e arrays congelados (selados,
// não extensíveis, `length` ou índice não gravável) em métodos que escrevem, com as mensagens exatas.
// Cada programa roda num bun filho novo (no máximo 6 ao mesmo tempo, timeout de 8 s), sem APIs de host, e grava o texto
// em `globalThis.R`. Programas já presentes em outros goldens (`knownPrograms`) são descartados.
// Colunas: sufixo do programa (JSON), valor de `R` (JSON) e índice do prelúdio (omitido quando 0). Uso:
//   bun scripts/gen-array-exotic-golden.js > tests/golden/array_exotic_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE = [
  "var HAS = Object.prototype.hasOwnProperty, JN = Array.prototype.join, NAMES = Object.getOwnPropertyNames;",
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
  "    for (var i = 0; i < n; i++) o[i] = HAS.call(v, i) ? S(v[i], d + 1) : '<hole>';",
  "    return '[' + JN.call(o, ',') + ']#' + v.length + '|' + JN.call(NAMES(v).slice(0, 30), ',') + (Object.getPrototypeOf(v) === Array.prototype ? '' : '~sub');",
  "  }",
  "  return '{' + Object.keys(v).slice(0, 20).map(function (k) { return k + ':' + S(v[k], d + 1); }).join(',') + '}';",
  "}",
  "function T(f) { try { return S(f()); } catch (e) { return 'throw ' + e.name + ': ' + e.message; } }",
  "function Q(o, k) {",
  "  var d = Object.getOwnPropertyDescriptor(o, k);",
  "  if (!d) return 'none';",
  "  return ('value' in d ? 'v=' + S(d.value) : 'g=' + typeof d.get + ',s=' + typeof d.set) + (d.writable ? ' W' : '') + (d.enumerable ? ' E' : '') + (d.configurable ? ' C' : '');",
  "}",
  "",
].join("\n");
const STRICT = '"use strict";\n';
const DASHES = [String.fromCharCode(0x2013), String.fromCharCode(0x2014)];

const progs = [];
function add(body, strict) {
  progs.push({ body: "globalThis.R = T(function () { var L = []; " + body + " });", strict: !!strict });
}
const both = (body) => { add(body, false); add(body, true); };
const TRY = (expr) => "var r; try { r = " + expr + "; } catch (e) { r = 'throw ' + e.name + ': ' + e.message; } ";

// ---- 1. Atribuição a length.
const NC = (idx, extra) => `(function () { var a = [1, 2, 3, 4, 5]; Object.defineProperty(a, ${idx}, {configurable: false${extra || ""}}); return a; })()`;
const LBASES = {
  full: "[1, 2, 3, 4, 5]", empty: "[]", holes: "[1, , 3, , 5]", sparse: "new Array(5)", one: "[7]",
  nc0: NC(0), nc2: NC(2), nc4: NC(4),
  lenRo: "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); return a; })()",
  frozen: "Object.freeze([1, 2, 3])", sealed: "Object.seal([1, 2, 3])", noExt: "Object.preventExtensions([1, 2, 3])",
  accessor: "(function () { var a = [1, 2, 3, 4]; Object.defineProperty(a, 2, {get: function () { return 'g'; }, configurable: false}); return a; })()",
  tail: "(function () { var a = []; a[100] = 1; return a; })()",
};
const LENV = ["0", "1", "2", "3", "4", "5", "6", "10", "-1", "-2", "1.5", "0.5", "2.5", "'3'", "'abc'", "''", "' 5 '", "'0x10'", "'1e1'", "'-0'",
  "NaN", "Infinity", "-Infinity", "2**32", "2**32-1", "2**32-2", "2**32+1", "2**31", "2**31-1", "-(2**31)", "-0", "true", "false", "null",
  "undefined", "{}", "[]", "[5]", "[1, 2]", "[null]", "1n", "Symbol()", "new Number(2)", "new String('2')", "new Boolean(true)", "2**53", "1e21",
  "'4294967295'", "'4294967296'", "'-1'"];
for (const [name, base] of Object.entries(LBASES)) {
  for (const v of LENV) both(`var a = ${base}; ${TRY(`(a.length = ${v})`)}return [r, a, Q(a, 'length')];`);
}
const LOGV = [
  "{valueOf() { L.push('v'); return 2; }, toString() { L.push('s'); return '3'; }}",
  "{valueOf() { L.push('v'); return {}; }, toString() { L.push('s'); return '3'; }}",
  "{valueOf() { L.push('v'); return {}; }, toString() { L.push('s'); return {}; }}",
  "{valueOf() { L.push('v'); throw new Error('boom'); }}",
  "{valueOf: undefined, toString() { L.push('s'); return '1'; }}",
  "{[Symbol.toPrimitive](h) { L.push(h); return 3; }}",
  "{[Symbol.toPrimitive](h) { L.push(h); return -1; }}",
  "{[Symbol.toPrimitive](h) { L.push(h); return {}; }}",
  "{[Symbol.toPrimitive]: 1}",
  "{[Symbol.toPrimitive]: null, valueOf() { L.push('v'); return 1; }}",
  "(function () { var n = 0; return {valueOf() { L.push('v' + n); return [2, 4294967297, 1.5][n++ % 3]; }}; })()",
  "(function () { var n = 0; return {valueOf() { L.push('v' + n); return n++ ? 2 : -1; }}; })()",
  "(function () { var n = 0; return {valueOf() { L.push('v' + n); return n++ ? -1 : 2; }}; })()",
  "Object.create(null)",
  "{valueOf() { L.push('v'); return 5n; }}",
  "{valueOf() { L.push('v'); return '2'; }}",
  "{valueOf() { L.push('v'); return NaN; }}",
];
for (const base of ["full", "empty", "nc2", "lenRo", "frozen", "holes"]) {
  for (const v of LOGV) both(`var a = ${LBASES[base]}; ${TRY(`(a.length = ${v})`)}return [r, a, L];`);
}
// Encolher com elementos não configuráveis em várias posições.
const NCSETS = {
  none: "", n3: "Object.defineProperty(a, 3, {configurable: false});", n7: "Object.defineProperty(a, 7, {configurable: false});",
  n37: "Object.defineProperty(a, 3, {configurable: false}); Object.defineProperty(a, 7, {configurable: false});",
  n0: "Object.defineProperty(a, 0, {configurable: false});", n9: "Object.defineProperty(a, 9, {configurable: false});",
  acc5: "Object.defineProperty(a, 5, {get: function () { return 'g'; }, configurable: false});",
  ro3: "Object.defineProperty(a, 3, {writable: false, configurable: false});",
  holeNc: "delete a[4]; delete a[3]; Object.defineProperty(a, 3, {value: 'n', configurable: false});",
};
for (const [name, setup] of Object.entries(NCSETS)) {
  for (let n = 0; n <= 11; n++) {
    const pre = `var a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]; ${setup} `;
    both(`${pre}${TRY(`(a.length = ${n})`)}return [r, a, Q(a, 'length')];`);
    add(`${pre}${TRY(`Object.defineProperty(a, 'length', {value: ${n}})`)}return [r === a ? 'same' : r, a, Q(a, 'length')];`);
    add(`${pre}${TRY(`Object.defineProperty(a, 'length', {value: ${n}, writable: false})`)}return [r === a ? 'same' : r, a, Q(a, 'length')];`);
    add(`${pre}${TRY(`Reflect.defineProperty(a, 'length', {value: ${n}})`)}return [r, a, Q(a, 'length')];`);
    add(`${pre}${TRY(`Reflect.set(a, 'length', ${n})`)}return [r, a, Q(a, 'length')];`);
  }
}

// ---- 2. defineProperty de length.
const DESCS = ["{value: 0}", "{value: 1}", "{value: 3}", "{value: 5}", "{value: 3, writable: false}", "{value: 2, writable: false}", "{value: 5, writable: false}",
  "{writable: false}", "{writable: true}", "{enumerable: true}", "{enumerable: false}", "{configurable: true}", "{configurable: false}",
  "{get() { return 1; }}", "{set(v) {}}", "{get: undefined}", "{value: 3, get() {}}", "{writable: false, set(v) {}}", "{value: -1}", "{value: 1.5}",
  "{value: '2'}", "{value: 'abc'}", "{value: NaN}", "{value: undefined}", "{value: null}", "{value: 2**32}", "{value: 2**32-1}",
  "{value: {valueOf() { return 2; }}}", "{value: {valueOf() { return -2; }}}", "{value: 3, enumerable: true}", "{value: 3, configurable: true}",
  "{value: 0, writable: false, enumerable: false, configurable: false}", "{}", "{value: 2, writable: true, enumerable: false, configurable: false}",
  "{value: Symbol()}", "{value: 1n}", "{value: true}", "{value: [2]}", "{value: -0}", "{value: 3.0}", "{value: '3'}", "{writable: undefined}"];
const DBASES = {
  len3: "[1, 2, 3]", empty: "[]", holes: "[1, , 3]",
  lenRo: "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); return a; })()",
  frozen: "Object.freeze([1, 2, 3])", sealed: "Object.seal([1, 2, 3])",
  nc1: "(function () { var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); return a; })()",
};
for (const base of Object.values(DBASES)) {
  for (const d of DESCS) {
    add(`var a = ${base}; ${TRY(`Object.defineProperty(a, 'length', ${d})`)}return [r === a ? 'same' : r, a, Q(a, 'length')];`);
    add(`var a = ${base}; ${TRY(`Reflect.defineProperty(a, 'length', ${d})`)}return [r, a, Q(a, 'length')];`);
  }
}
for (const d of ["{value: 7}", "{value: 7, writable: false}", "{value: 7, enumerable: true}", "{get() {}}", "{value: 1, configurable: true}", "{value: 7, writable: false, configurable: true}"]) {
  for (const k of ["0", "2", "3", "5", "4294967294", "4294967295", "'length'", "'4294967296'"]) {
    add(`var a = [1, 2, 3]; ${TRY(`Object.defineProperty(a, ${k}, ${d})`)}return [r === a ? 'same' : r, a, Q(a, ${k})];`);
    add(`var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); ${TRY(`Reflect.defineProperty(a, ${k}, ${d})`)}return [r, a, Q(a, ${k})];`);
  }
}

// ---- 3. Índices como chaves.
const KEYS = ["'0'", "'1'", "'2'", "'3'", "'01'", "'1.0'", "'-0'", "'+1'", "'1e0'", "'0x1'", "' 1'", "'1 '", "'00'", "'0.0'", "'-1'", "'1.5'",
  "'4294967293'", "'4294967294'", "'4294967295'", "'4294967296'", "'9007199254740991'", "'9007199254740992'", "4294967294", "4294967295",
  "4294967296", "-0", "0", "1", "1.5", "-1", "NaN", "Infinity", "'Infinity'", "'NaN'", "'length'", "'Length'", "'1000'", "1e3", "'constructor'",
  "Symbol.iterator", "'__proto__'", "true", "null", "undefined", "[1]", "[0]", "{toString() { L.push('ts'); return '1'; }}",
  "{toString() { return '4294967295'; }}", "{toString() { return '4294967294'; }}", "2**32-3", "'4294967294.0'"];
for (const k of KEYS) {
  both(`var a = [1, 2, 3]; ${TRY(`(a[${k}] = 9)`)}return [r, a, a.length, L];`);
  add(`var a = [1, 2, 3]; return [${k} in a, a.hasOwnProperty(${k}), Object.hasOwn(a, ${k}), a[${k}]];`);
  add(`var a = [1, 2, 3]; return [delete a[${k}], a];`);
  add(`var a = [1, 2, 3]; Object.defineProperty(a, ${k}, {value: 7, writable: true, enumerable: true, configurable: true}); return [a, a.length, Q(a, ${k})];`);
  add(`var a = []; a[${k}] = 1; return [a, a.length, Object.keys(a)];`);
  both(`var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); ${TRY(`(a[${k}] = 9)`)}return [r, a, Q(a, ${k})];`);
  both(`var a = [1, 2, 3]; Object.freeze(a); ${TRY(`(a[${k}] = 9)`)}return [r, a];`);
  add(`var a = [1, 2, 3]; a[${k}] = 9; a.length = 1; return [a, a.length, Object.getOwnPropertyNames(a)];`);
  add(`var o = Object.create([1, 2, 3]); o[${k}] = 5; return [o.length, Object.keys(o), o[${k}]];`);
  add(`var a = [1, 2, 3]; return [Reflect.set(a, ${k}, 9), a, Reflect.defineProperty(a, ${k}, {value: 1}), a];`);
}

// ---- 4. Buracos em métodos.
const FX = ["[1, , 3]", "[, , ]", "[, 1]", "[1, , ]", "new Array(3)", "[1, undefined, 3]", "[undefined, , 2]", "[3, , 1, , 2]", "[, 'a', , 'b', , ]",
  "[null, , undefined, 0, '', NaN]", "[, ]", "[]", "[1, 2, 3]"];
const CB = (ret) => `function (v, i, o) { L.push(i + ':' + S(v)); return ${ret}; }`;
const RED = "function (p, v, i) { L.push(i); return p + '|' + S(v); }";
const CMP = "function (x, y) { L.push(S(x) + ',' + S(y)); return String(x) < String(y) ? -1 : String(x) > String(y) ? 1 : 0; }";
const METHODS = ["(0 in a)", "(1 in a)", "(2 in a)", "a.hasOwnProperty(1)", "a.hasOwnProperty(0)", "Object.hasOwn(a, 2)",
  `a.forEach(${CB("0")})`, `a.map(${CB("v")})`, `a.map(${CB("i")})`, `a.filter(${CB("true")})`, `a.filter(${CB("v")})`, `a.some(${CB("false")})`,
  `a.every(${CB("true")})`, `a.find(${CB("false")})`, `a.findIndex(${CB("false")})`, `a.findLast(${CB("false")})`, `a.findLastIndex(${CB("false")})`,
  `a.flatMap(${CB("[v, v]")})`, `a.reduce(${RED})`, `a.reduce(${RED}, 'init')`, `a.reduceRight(${RED})`, `a.reduceRight(${RED}, 'init')`,
  "a.join()", "a.join('-')", "a.join(undefined)", "a.join(null)", "a.join('')", "a.toString()", "a.toLocaleString()", "a.indexOf(undefined)", "a.indexOf(1)",
  "a.lastIndexOf(undefined)", "a.lastIndexOf(3)", "a.includes(undefined)", "a.includes(NaN)", "a.includes(1)", "a.sort()", `a.sort(${CMP})`,
  "a.sort(function (x, y) { return y > x ? 1 : -1; })", "a.reverse()", "a.concat()", "a.concat([9])", "a.concat(a)", "a.slice()", "a.slice(1)", "a.slice(-2)",
  "a.splice(0)", "a.splice(1, 1)", "a.splice(1, 0, 'x')", "a.splice(1, 1, 'x', 'y')", "a.flat()", "a.fill(0)", "a.fill(0, 1, 2)", "a.copyWithin(0, 1)",
  "a.copyWithin(1, 0)", "[...a.keys()]", "[...a.entries()]", "[...a.values()]", "[...a]", "Array.from(a)", "Object.keys(a)", "Object.entries(a)",
  "JSON.stringify(a)", "a.at(1)", "a.at(-1)", "a.push(1)", "a.pop()", "a.shift()", "a.unshift(0)", "a.toSorted()", "a.toReversed()", "a.toSpliced(1, 1)",
  "a.with(0, 'w')", "a.with(1, 'w')", `a.toSorted(${CMP})`, "a.flat(Infinity)", "(function () { for (var k in a) L.push(k); return L.length; })()",
  "Object.getOwnPropertyNames(a)", "Reflect.ownKeys(a)", "a.length", "Array.prototype.slice.call(a)", "Array.prototype.map.call(a, function (v) { return v; })",
  "Array.of.apply(null, a)", "String(a)", "a + ''", "[].concat(a, a)", "Math.max.apply(null, a)", "Object.assign([], a)", "Array.from(a, function (v) { return S(v); })",
  "Object.fromEntries(a.entries())", "a.entries().next()", "Array.prototype.every.call(a, function (v) { return v === undefined; })"];
for (const fx of FX) for (const m of METHODS) add(`var a = ${fx}; ${TRY(m)}return [r, a, L];`);
const PROTOS = ["Array.prototype[1] = 'P';", "Object.prototype[0] = 'O'; Object.prototype[2] = 'Q';"];
for (const fx of ["[1, , 3]", "new Array(3)", "[, 'a', , 'b', , ]", "[3, , 1, , 2]"]) {
  for (const p of PROTOS) for (const m of METHODS) add(`${p} var a = ${fx}; ${TRY(m)}return [r, a, L];`);
}
// sort com buracos e undefined.
const SFX = ["[3, undefined, , 1, , 2]", "[undefined, undefined]", "[, , undefined]", "[undefined, , ]", "[5, , 4, undefined, 3, , 2]", "['b', , 'a', undefined]",
  "[10, 9, 1, , undefined, null, NaN]", "[, 'z', , 'y']", "[2, 1]", "[1, , 0]"];
const SCMPS = ["", CMP, "function (x, y) { return x - y; }", "function (x, y) { return y - x; }", "function (x, y) { L.push(S(x) + ',' + S(y)); return 0; }",
  "function (x, y) { return NaN; }", "function (x, y) { return undefined; }", "function (x, y) { return '1'; }", "function (x, y) { return -1; }",
  "function (x, y) { return 1; }", "function (x, y) { L.push('c'); throw new Error('stop'); }", "function (x, y) { return {valueOf() { L.push('vo'); return 1; }}; }",
  "function (x, y) { return x === undefined ? 0 : -1; }", "undefined", "null", "1", "{}", "Symbol()", "function (x, y) { return 1n; }"];
for (const fx of SFX) for (const c of SCMPS) {
  add(`var a = ${fx}; ${TRY(`a.sort(${c})`)}return [r, a, L];`);
  add(`var a = ${fx}; ${TRY(`a.toSorted(${c})`)}return [r, a, L];`);
}
// join e toString com buracos, null, undefined e separadores exóticos.
const JARR = ["[null, undefined, , 1]", "[[1, , 2], , [null, undefined]]", "[{toString() { L.push('t'); return 'x'; }}, , {toString() { L.push('u'); return 'y'; }}]",
  "[, ]", "[[], [[]], [[], []]]", "[1, [2, [3, [4]]]]", "[, , , ]", "[{toString: null, valueOf() { return 'vo'; }}]"];
const SEPS = ["", "undefined", "null", "','", "''", "1", "{toString() { L.push('sep'); return ';'; }}", "Symbol()", "'--'", "[]"];
for (const arr of JARR) for (const sep of SEPS) add(`var a = ${arr}; ${TRY(`a.join(${sep})`)}return [r, L];`);
add("var a = [1, 2]; a[2] = a; return [a.join(), String(a), a.toString()];");
add("var a = [1, 2]; a.push([3, a]); return [a.join('+'), a.toLocaleString()];");
add("var a = [1, , 3]; a.join = null; return [String(a)];");
add("var a = [1, , 3]; a.join = function () { return 'J'; }; return [String(a), a + '', `${a}`];");
add("return [Array.prototype.toString.call({join: 1}), Array.prototype.toString.call({join() { return 'z'; }}), Array.prototype.toString.call(1)];");
add("return [Array.prototype.join.call({length: 3, 1: 'a'}, '/'), Array.prototype.join.call({length: -1}), Array.prototype.join.call({length: '2', 0: 'q'}, '.'), Array.prototype.join.call('abc', '|')];");
add("return [[, 'a'].toLocaleString(), [1234.5, , new Date(0)].toLocaleString().length > 0, [null, undefined].toLocaleString()];");
add("var a = [{toLocaleString() { L.push('tl'); return 'L'; }}, , {toLocaleString: null}]; return [T(function () { return a.toLocaleString(); }), L];");

// ---- 5. Array(n) com n inválido e estouro de comprimento.
const NV = ["-1", "-0", "0", "1", "1.5", "0.1", "2**32", "2**32-1", "2**32-2", "2**32+1", "NaN", "Infinity", "-Infinity", "'3'", "'abc'", "''", "'0'", "null",
  "undefined", "true", "false", "1n", "Symbol()", "{}", "[]", "[3]", "[1, 2]", "new Number(3)", "-1e-7", "1e10", "4294967295.5", "4294967295.1", "2**53",
  "3.0000000001", "'4294967296'", "'4294967295'", "2", "5", "-5", "1e21", "4294967296.5", "1.0000000000000002"];
for (const n of NV) {
  add(`return Array(${n});`);
  add(`return new Array(${n});`);
  add(`return Array.apply(null, [${n}]);`);
  add(`return Reflect.construct(Array, [${n}]);`);
  add(`class A extends Array {} return [new A(${n}), new A(${n}) instanceof A];`);
  add(`return Array.of(${n});`);
  add(`return [new Array(${n}).length, Array(${n}).hasOwnProperty(0)];`);
  add(`var a = []; ${TRY(`(a.length = ${n})`)}return [r, a];`);
  add(`return Object.defineProperty([], 'length', {value: ${n}});`);
}
for (const n of ["-1", "0", "3", "NaN", "'2'", "2.9", "-Infinity", "true", "null", "undefined", "{valueOf() { return 2; }}", "'abc'"]) {
  add(`return Array.from({length: ${n}});`);
  add(`return Array.from({length: ${n}}, function (v, i) { return i * 2; });`);
  add(`return Array.prototype.map.call({length: ${n}}, function (v) { return v; });`);
  add(`return Array.prototype.slice.call({length: ${n}, 0: 'a', 1: 'b'});`);
  add(`return Array.prototype.fill.call({length: ${n}}, 1);`);
  add(`return Array.prototype.concat.call({length: ${n}}, [1]);`);
}
const OVERFLOW_CALLS = [["map", "function (v) { return v; }"], ["slice", ""], ["slice", "0, 2**33"],
  ["toSorted", ""], ["toReversed", ""], ["with", "0, 1"], ["toSpliced", "0, 0"], ["splice", "0, 0"],
  ["splice", "0, 2**33"], ["concat", ""], ["at", "0"]];
for (const len of ["2**32", "2**32+1", "2**53-1", "2**53", "Infinity", "2**40"]) {
  for (const [name, args] of OVERFLOW_CALLS) {
    add(`${TRY(`Array.prototype.${name}.call({length: ${len}}${args ? ", " + args : ""})`)}return [r];`);
  }
}
for (const len of ["2**32-1", "2**32-2", "2**32-3"]) {
  add(`var a = []; a.length = ${len}; ${TRY("a.push(1)")}return [r, a.length, Q(a, 4294967295), Q(a, 4294967294)];`);
  add(`var a = []; a.length = ${len}; ${TRY("a.push()")}return [r, a.length];`);
  add(`var a = []; a.length = ${len}; ${TRY("a.push(1, 2)")}return [r, a.length, Q(a, 4294967294), Q(a, 4294967295)];`);
  add(`var a = []; a.length = ${len}; ${TRY("a.pop()")}return [r, a.length];`);
  add(`var a = []; a.length = ${len}; a[4294967294] = 'last'; ${TRY("a.pop()")}return [r, a.length, Q(a, 4294967294)];`);
  add(`var a = []; a.length = ${len}; ${TRY("(a[4294967295] = 1)")}return [r, a.length, Q(a, 4294967295)];`);
  add(`var a = []; a.length = ${len}; ${TRY("(a[4294967294] = 1)")}return [r, a.length];`);
  add(`var a = []; a.length = ${len}; ${TRY("a.at(-1)")}return [r];`);
  add(`var a = []; a.length = ${len}; ${TRY("Array.prototype.push.call(a, 1)")}return [r, a.length];`);
}
for (const len of ["2**53-1", "2**53-2", "2**53-3", "2**53", "2**53+2", "Infinity", "-1", "'7'"]) {
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.push.call(o, 1)")}return [r, o.length, Object.keys(o)];`);
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.push.call(o)")}return [r, o.length, Object.keys(o)];`);
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.push.call(o, 1, 2)")}return [r, o.length, Object.keys(o)];`);
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.pop.call(o)")}return [r, o.length, Object.keys(o)];`);
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.unshift.call(o)")}return [r, o.length];`);
  if (["2**53-1", "2**53", "2**53+2", "Infinity", "-1"].includes(len)) add(`var o = {length: ${len}}; ${TRY("Array.prototype.unshift.call(o, 1)")}return [r, o.length];`);
  if (["2**53-1", "2**53", "2**53+2", "Infinity", "-1"].includes(len)) add(`var o = {length: ${len}}; ${TRY("Array.prototype.splice.call(o, 0, 0, 1)")}return [r, o.length];`);
  add(`var o = {length: ${len}}; ${TRY("Array.prototype.concat.call([], o)")}return [r];`);
}

// ---- 6. Array.prototype como array e Array.isArray.
const ISARR = ["[]", "new Array(3)", "Array.prototype", "Object.create(Array.prototype)", "Object.setPrototypeOf({}, Array.prototype)", "new Proxy([], {})",
  "new Proxy({}, {})", "new Proxy(new Proxy([], {}), {})", "new Proxy(Array.prototype, {})", "(function () { var p = Proxy.revocable([], {}); p.revoke(); return p.proxy; })()",
  "(function () { class A extends Array {} return new A(); })()", "(function () { class A extends Array {} return A.prototype; })()", "Array.from('ab')",
  "(function () { return arguments; })(1, 2)", "new Uint8Array(2)", "new ArrayBuffer(2)", "Object.setPrototypeOf([], null)", "Reflect.construct(Array, [], Object)",
  "Reflect.construct(Array, [3], Function)", "Array", "Array.prototype.concat", "{length: 0}", "'abc'", "null", "undefined", "1", "Object.create([])", "[].entries()",
  "Object.freeze([])", "Object.assign([], {a: 1})", "JSON.parse('[1]')", "JSON.parse('{}')", "'a'.split('')", "/a/.exec('a')", "'a'.match(/a/g)", "Array.prototype.slice.call([])",
  "Object.getPrototypeOf([])", "Object.getPrototypeOf(Array.prototype)", "Object.create(Object.getPrototypeOf([]))", "[].concat.call(1)", "new Proxy(function () {}, {})",
  "(function () { var p = Proxy.revocable({}, {}); p.revoke(); return p.proxy; })()", "Reflect.construct(Array, [], Array)"];
for (const v of ISARR) {
  add(`return [Array.isArray(${v})];`);
  add(`var v = ${v}; return [Array.isArray(v), Object.prototype.toString.call(v), typeof v];`);
  add(`return [Array.isArray([${v}]), Array.isArray(Array.of(${v}))];`);
}
const AP = [
  "Array.prototype.length", "Object.getOwnPropertyDescriptor(Array.prototype, 'length')", "Object.getOwnPropertyNames(Array.prototype).slice(0, 5)",
  "Object.prototype.toString.call(Array.prototype)", "Array.prototype.join()", "String(Array.prototype)", "JSON.stringify(Array.prototype)",
  "JSON.stringify({a: Array.prototype})", "Array.prototype.concat([1])", "[].concat(Array.prototype)", "[...Array.prototype]", "Array.from(Array.prototype)",
  "Array.prototype.map(function (v) { return v; })", "Array.prototype.push(1, 2)", "(Array.prototype.push(1), [Array.prototype.length, Array.prototype[0]])",
  "(Array.prototype.length = 3, [Array.prototype.length, Object.keys(Array.prototype).length, 0 in Array.prototype])",
  "(Array.prototype[5] = 'x', [Array.prototype.length, [].length, [][5], 5 in []])", "(Array.prototype.length = 2**32, 1)",
  "(Array.prototype.length = -1, 1)", "Array.prototype.indexOf(undefined)", "Array.prototype.includes(undefined)",
  "Object.keys(Array.prototype)", "Reflect.ownKeys(Array.prototype).length > 20", "Array.prototype.toString === Object.prototype.toString",
  "Array.prototype.constructor === Array", "Object.getPrototypeOf(Array.prototype) === Object.prototype", "Array.prototype instanceof Array",
  "Array.prototype.isPrototypeOf([])", "Array.isArray(Array.prototype)", "Array.prototype.at(0)", "Array.prototype.pop()", "Array.prototype.shift()",
  "Array.prototype.reverse() === Array.prototype", "Array.prototype.sort() === Array.prototype", "Array.prototype.slice()", "Array.prototype.fill(1).length",
  "(Array.prototype.length = 2, Array.prototype.fill(1), [[].length, Array.prototype[1], [].concat([]).length])",
  "(Array.prototype[0] = 'p', [[][0], [,].hasOwnProperty(0), 0 in [], [1, , 3][1], [, 1].indexOf(undefined)])",
  "(Array.prototype[0] = 'p', [[,].join(), [, 1].map(function (v) { return v; }), [,].forEach(function (v) { L.push(v); }), L])",
  "(Object.defineProperty(Array.prototype, 'length', {writable: false}), [T(function () { Array.prototype.push(1); }), Array.prototype.length])",
  "(Object.defineProperty(Array.prototype, 'length', {value: 4, writable: false}), [Array.prototype.length, T(function () { return Array.prototype.pop(); })])",
  "(Object.freeze(Array.prototype), [Object.isFrozen(Array.prototype), T(function () { return Array.prototype.push(1); })])",
  "(Object.freeze(Array.prototype), [T(function () { var a = []; a[0] = 1; return a; }), T(function () { var a = [1]; a.push(2); return a; })])",
  "(Object.freeze(Array.prototype), T(function () { Array.prototype.length = 5; return Array.prototype.length; }))",
  "(Object.freeze(Array.prototype), T(function () { 'use strict'; Array.prototype.length = 5; }))",
  "Object.create(Array.prototype).length", "(function () { var o = Object.create(Array.prototype); o.length = 5; return [o.length, Object.keys(o), Array.prototype.length]; })()",
  "(function () { var o = Object.create(Array.prototype); o.push('a'); return [o.length, Object.keys(o), Array.prototype.length]; })()",
  "(function () { var o = Object.create(Array.prototype); o[3] = 1; return [o.length, Object.keys(o)]; })()",
  "(function () { var o = Object.create(Array.prototype); return [o.concat([1]), Array.isArray(o), o.map]; })()",
  "(function () { var o = Object.setPrototypeOf({0: 'a', 1: 'b', length: 2}, Array.prototype); return [o.join('+'), o.slice(1), o.map(function (v) { return v + v; })]; })()",
  "(function () { var a = [1, 2, 3]; Object.setPrototypeOf(a, null); a.length = 1; return [a.length, a[1], Array.isArray(a), Object.keys(a)]; })()",
  "(function () { var a = [1, 2, 3]; Object.setPrototypeOf(a, {length: 10}); return [a.length, Object.keys(a)]; })()",
  "(function () { var a = [1, 2, 3]; Object.setPrototypeOf(a, {set length(v) { L.push('set'); }}); a.length = 1; return [a.length, L]; })()",
  "(function () { var a = []; Object.setPrototypeOf(a, {set 0(v) { L.push('set0'); }}); a[0] = 1; return [a.length, L]; })()",
  "(function () { var a = []; Object.setPrototypeOf(a, {get 0() { return 'g'; }}); return [a[0], 0 in a, a.length, a.indexOf('g'), a.join()]; })()",
  "(function () { var a = []; Object.setPrototypeOf(a, Object.freeze({0: 'f'})); a[0] = 1; return [a.length, Object.keys(a)]; })()",
  "(function () { class A extends Array {} var a = new A(); a.length = 3; return [a.length, a instanceof A, Array.isArray(a), a.map(function (v) { return v; }) instanceof A]; })()",
  "(function () { class A extends Array {} A.prototype.length = 7; return [new A().length, A.prototype.length, Array.isArray(A.prototype)]; })()",
  "(function () { function F() {} F.prototype = Array.prototype; var o = new F(); return [Array.isArray(o), o.length, o.push(1), o.length, Array.prototype.length]; })()",
  "Reflect.getPrototypeOf(Array.prototype.concat) === Function.prototype",
  "(function () { var a = []; a.constructor = undefined; return [a.map(function (v) { return v; }) instanceof Array]; })()",
];
for (const e of AP) both(`return [${e}];`);

// ---- 7. Arrays congelados e afins em métodos que escrevem.
const STATES = {
  none: "", frozen: "Object.freeze(a);", sealed: "Object.seal(a);", noExt: "Object.preventExtensions(a);",
  lenRo: "Object.defineProperty(a, 'length', {writable: false});", idx0Ro: "if (a.length) Object.defineProperty(a, 0, {writable: false});",
  idx1Nc: "if (a.length > 1) Object.defineProperty(a, 1, {configurable: false});", idx1Get: "if (a.length > 1) Object.defineProperty(a, 1, {get: function () { return 'g'; }, configurable: true});",
  idx1Set: "if (a.length > 1) Object.defineProperty(a, 1, {set: function (v) { L.push('set' + v); }, configurable: true});",
  lastNc: "if (a.length) Object.defineProperty(a, a.length - 1, {configurable: false});",
};
const WBASES = ["[1, 2, 3]", "[]", "[1, , 3]", "[3, 1, 2]"];
const WOPS = [["a.push(4)", 0], ["a.push()", 0], ["a.pop()", 0], ["a.shift()", 0], ["a.unshift(0)", 0], ["a.unshift()", 0], ["a.splice(0, 1)", 0], ["a.splice(1, 0, 9)", 0],
  ["a.splice(0, 0)", 0], ["a.splice(1, 1, 9)", 0], ["a.splice()", 0], ["a.splice(0)", 0], ["a.reverse()", 0], ["a.sort()", 0], ["a.sort(function (x, y) { return y - x; })", 0],
  ["a.fill(0)", 0], ["a.fill(0, 1, 2)", 0], ["a.copyWithin(0, 1)", 0], ["a.copyWithin(0, 0)", 0], ["(a.length = 0)", 1], ["(a.length = 3)", 1], ["(a.length = 1)", 1],
  ["(a[3] = 4)", 1], ["(a[0] = 9)", 1], ["(a[1] = 9)", 1], ["delete a[0]", 1], ["delete a[1]", 1], ["Object.defineProperty(a, 0, {value: 5})", 0],
  ["Object.defineProperty(a, 5, {value: 5})", 0], ["Array.prototype.push.call(a, 1)", 0], ["Array.prototype.pop.call(a)", 0], ["a.toSorted()", 0],
  ["a.toReversed()", 0], ["a.toSpliced(0, 1)", 0], ["a.with(0, 9)", 0], ["Object.assign(a, {0: 9})", 0], ["Reflect.set(a, 1, 9)", 0],
  ["Reflect.deleteProperty(a, 1)", 0], ["a.concat([1])", 0], ["Array.prototype.fill.call(a, 7, 0, 1)", 0], ["(a[a.length] = 1)", 1],
  ["a.length--", 1], ["a.length++", 1], ["a[1]++", 1], ["Object.setPrototypeOf(a, null)", 0], ["Object.freeze(a)", 0]];
for (const [sn, state] of Object.entries(STATES)) {
  for (const base of WBASES) {
    for (const [op, assign] of WOPS) {
      const body = `var a = ${base}; ${state} ${TRY(op)}return [r, a, L, Object.isFrozen(a), Object.isSealed(a), Object.isExtensible(a)];`;
      if (assign) both(body); else add(body);
    }
  }
}

// ---- 8. Setter em Array.prototype[0] e [1]: os métodos que criam o resultado com `putDirectIndex` (CreateDataProperty) não
// o disparam; os que escrevem em `this` com `putByIndex` (Set) disparam.
const SETTER_OPS = [
  "a.slice()", "a.slice(1)", "a.splice(0, 2)", "a.splice(0, 0, 7)", "a.concat([4, 5])", "a.concat(a)", "[[1], [2]].flat()", "a.flatMap(function (v) { return [v, v]; })",
  "a.map(function (v) { return v; })", "a.filter(function () { return true; })", "Array.from(a)", "Array.from({length: 3, 0: 'x', 1: 'y'})",
  "Array.of(1, 2, 3)", "Array.from(new Set([1, 2, 3]))", "a.toSorted()", "a.toReversed()", "a.toSpliced(0, 1)", "a.with(0, 9)", "a.push(9)", "a.unshift(9)",
  "a.reverse()", "a.fill(0)", "a.copyWithin(0, 1)", "a.sort()", "Array.prototype.slice.call({length: 3, 0: 'x', 1: 'y'})", "Array.prototype.map.call('abc', function (c) { return c; })",
  "Object.fromEntries([[0, 'a'], [1, 'b']])", "'a,b,c'.split(',')", "/(a)(b)/.exec('ab')", "'ab'.match(/(a)(b)/)", "[...'ab'.matchAll(/./g)].length", "JSON.parse('[1,2,3]')", "JSON.parse('[1,2,3]', function (k, v) { return v; })",
  "'abc'.split('')", "Array.apply(null, [1, 2])", "new Array(1, 2)", "[...a]", "Object.entries({x: 1, y: 2})", "Object.keys({x: 1, y: 2})", "Object.getOwnPropertyNames('ab')",
];
for (const base of ["[1, 2, 3]", "[1, , 3]", "[]"]) {
  for (const op of SETTER_OPS) {
    for (const idx of ["0", "1"]) {
      both(`var a = ${base}; Object.defineProperty(Array.prototype, ${idx}, {set: function (v) { L.push('set${idx}:' + v); }, get: function () { return 'g${idx}'; }, configurable: true}); try { ${TRY(op)}return [r, L]; } finally { delete Array.prototype[${idx}]; }`);
    }
  }
}
both(`var a = [1, 2, 3]; Object.defineProperty(Array.prototype, 0, {set: function (v) { L.push('s0:' + v); }, configurable: true}); Object.defineProperty(Array.prototype, 1, {set: function (v) { L.push('s1:' + v); }, configurable: true}); try { ${TRY("[a.slice(), a.concat(a), a.toSorted(), a.map(function (v) { return v; })]")}return [r, L]; } finally { delete Array.prototype[0]; delete Array.prototype[1]; }`);

// ---- Dedupe contra os goldens existentes, execução em até 6 filhos e escrita.
const known = new Set();
for (const source of knownPrograms("array_exotic_bun.tsv", () => true)) {
  known.add(source);
  for (const line of source.split("\n")) if (line.length > 24) known.add(line.trim());
}
const seen = new Set();
const jobs = [];
let dup = 0, host = 0;
for (const p of progs) {
  const key = (p.strict ? "S:" : "") + p.body;
  if (seen.has(key)) continue;
  seen.add(key);
  if (usesHostApi(p.body)) { host++; continue; }
  if (known.has(p.body)) { dup++; continue; }
  jobs.push({ strict: p.strict, source: (p.strict ? STRICT : "") + PRELUDE + p.body });
}
// Sem diretiva primeiro: o prelúdio 0 é o sem "use strict".
jobs.sort((a, b) => Number(a.strict) - Number(b.strict));

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => { out += d; });
    child.on("close", (code, signal) => { clearTimeout(timer); resolve({ code, signal, out: decodeResult(out) }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i].source);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  const rows = [];
  let dropped = 0;
  jobs.forEach((job, i) => {
    const r = results[i];
    if (r.code !== 0 || r.signal || r.out === null) { dropped++; process.stderr.write(`filho falhou (código ${r.code}, sinal ${r.signal}): ` + JSON.stringify(job.source.slice(job.source.indexOf("globalThis.R = T(")))+ "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || r.out.includes(DASHES[0]) || r.out.includes(DASHES[1])) { dropped++; process.stderr.write("resultado com caminho, marca ou travessão: " + JSON.stringify(job.source.slice(PRELUDE.length)).slice(0, 160) + "\n"); return; }
    rows.push({ source: job.source, result: r.out });
  });
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos ${dup}, host ${host}\n`);
  process.stdout.write(emitFactored("array_exotic", rows));
}
main();
