// Gera tests/golden/number_edge_bun.tsv: bordas de Number, Math e operadores numéricos, medido no bun 1.4.2.
// Cobre toFixed/toPrecision/toExponential/toString(radix) nas bordas (dígitos, arredondamento, expoentes, erros de
// intervalo), parseFloat/parseInt/Number() com texto difícil, todas as funções de Math com valores especiais e bits
// exatos (via Float64Array), e os operadores **, %, >>>, shifts, ++/-- e aritmética em BigInt e Number misto.
// Programas que já aparecem (como trecho de fonte) em number_format*, bigint_bun e number_compact são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-number-edge-golden.js > tests/golden/number_edge_bun.tsv
const fs = require("fs");
const { emitRow, sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'var F64=new Float64Array(1),U64=new BigUint64Array(F64.buffer);\n' +
  'function B(v){F64[0]=v;return U64[0].toString(16)}\n';

const exprs = [];
// As matrizes (laços que chamam add com uma expressão só) entram por amostragem por hash (sampleByHash), para o golden
// ficar perto de 600 programas; as listas escritas à mão (add com várias expressões) entram inteiras. `kinds` guarda,
// em paralelo a `exprs`, a origem de cada expressão ("all", "matrix" ou "some"); a amostragem roda depois da geração.
const STRIDE = 40;
const kinds = [];
const push = (kind, list) => { for (const text of list) { exprs.push(text); kinds.push(kind); } };
const add = (...list) => push(list.length === 1 ? "matrix" : "all", list);
// Laços que emitem várias expressões por item: entra cerca de uma a cada três.
const addSome = (...list) => push("some", list);
const thin = (kind, step) => {
  const pool = exprs.filter((_, i) => kinds[i] === kind);
  const kept = new Set(sampleByHash(pool, Math.ceil(pool.length / step)));
  return (text, i) => kinds[i] !== kind || kept.has(text);
};
const lit = v => (Object.is(v, -0) ? "-0" : typeof v === "bigint" ? v + "n" : String(v));
const q = s => JSON.stringify(s);

const specials = [0, -0, 1, -1, 0.5, -0.5, 1.5, 2.5, -2.5, 0.1, 0.3, 1e21, 1e-7, 123.456, 5e-324, 1.7976931348623157e308, Infinity, -Infinity, NaN, 2 ** 53, 2 ** 31, -(2 ** 31), 2 ** 32, 4294967295.5];

// ---- 1. toFixed.
for (const v of [0, -0, 1.005, 1.45, 8.345, 0.5, 1.5, 2.5, -1.5, 123.456, 1e21, 1e-7, 0.000001, 1.255, 10.235, NaN, Infinity, -Infinity, 999.995, 0.1 + 0.2, 5e-324, 2 ** 53, 1e20, -1e-10])
  for (const d of [0, 1, 2, 5, 20, 100]) add(`T(()=>(${lit(v)}).toFixed(${d}))`);
for (const d of [-1, 101, 1.9, "2", NaN, undefined, null, Infinity, "x", -0.5]) add(`T(()=>(1.5).toFixed(${typeof d === "string" ? q(d) : lit(d)}))`);
add("T(()=>Number.prototype.toFixed.call('1',1))", "T(()=>Number.prototype.toFixed.call(new Number(1.25),1))", "T(()=>(25).toFixed.length)");

// ---- 2. toPrecision.
for (const v of [0, -0, 1, 123456, 123.456, 0.000123, 1e21, 1e-7, 1.5, 2.5, 99.99, 0.00001, 1e100, 5e-324, 1.7976931348623157e308, NaN, Infinity, -Infinity, 123456789, 0.1 + 0.2])
  for (const p of [1, 2, 3, 7, 21, 100]) add(`T(()=>(${lit(v)}).toPrecision(${p}))`);
for (const p of [0, 101, -1, "x", NaN, undefined, null, 1.9, Infinity]) add(`T(()=>(1.5).toPrecision(${typeof p === "string" ? q(p) : lit(p)}))`);
add("T(()=>(NaN).toPrecision(0))", "T(()=>(Infinity).toPrecision(1000))", "T(()=>(123).toPrecision())");

// ---- 3. toExponential.
for (const v of [0, -0, 1, 123456, 0.000123, 1e21, 1e-7, 1.5, 2.5, 99.99, 1e100, 5e-324, 1.7976931348623157e308, NaN, Infinity, -Infinity, 12345.6789, 0.1 + 0.2])
  for (const d of [undefined, 0, 1, 2, 5, 20, 100]) add(`T(()=>(${lit(v)}).toExponential(${d === undefined ? "" : d}))`);
for (const d of [-1, 101, "x", NaN, null, 1.9, Infinity]) add(`T(()=>(1.5).toExponential(${typeof d === "string" ? q(d) : lit(d)}))`);
add("T(()=>(NaN).toExponential(1000))", "T(()=>(Infinity).toExponential(-1))");

// ---- 4. toString(radix).
for (const v of [0, -0, 1, -1, 255, 0.5, 0.1, -0.1, 1 / 3, 1e21, 1e-7, 2 ** 53, 2 ** 64, 123.456, 5e-324, 1.7976931348623157e308, NaN, Infinity, -Infinity, 0.000001, 35, 36, 1e300])
  for (const r of [2, 3, 7, 8, 16, 32, 36]) add(`T(()=>(${lit(v)}).toString(${r}))`);
for (const r of [0, 1, 37, -1, 2.9, "16", NaN, null, undefined, Infinity, "x", 1e10]) add(`T(()=>(255).toString(${typeof r === "string" ? q(r) : lit(r)}))`);

// ---- 5. parseFloat.
const pf = ["", " ", "1", " 1 ", "\t\n1.5x", "1e3", "1e", "1e+", "1e-", ".5", "5.", ".", "-.5", "+.5", "--1", "Infinity", "-Infinity", "+Infinity", "infinity", "Infinityx",
  "NaN", "0x10", "0b1", "1_0", "1,5", "1.2.3", "1e1000", "-1e1000", "1e-1000", "0.0000001", "123456789012345678901234567890", "-0", "+0", "-0.0e5", " 1", "﻿1",
  " 1", "  2", "1\u0000", "٣", "0.1e1", "9007199254740993", "4.9e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1.7976931348623158e308", "1.7976931348623159e308", "1e21", "1e+21", "00012", "1e0000001"];
for (const s of pf) add(`T(()=>parseFloat(${q(s)}))`);
add("T(()=>parseFloat(null))", "T(()=>parseFloat(undefined))", "T(()=>parseFloat(true))", "T(()=>parseFloat([1.5,2]))", "T(()=>parseFloat({toString(){return '7x'}}))", "T(()=>parseFloat(1e21))",
  "T(()=>parseFloat(0.0000001))", "T(()=>parseFloat(-0))", "T(()=>1/parseFloat('-0'))", "T(()=>parseFloat(Symbol()))", "T(()=>parseFloat(1n))", "T(()=>parseFloat===Number.parseFloat)");

// ---- 6. parseInt.
const pi = ["", "0", "-0", "  42", "42abc", "0x1f", "0X1F", "-0x1f", "0x", "0xg", "0b11", "0o7", "1e3", "1.9", "-1.9", "9007199254740993", "123456789012345678901234567890", "ZZ", "zz", "z", "10", "  \n7", "+7", "--7", "1_0", " 5", "٣", "Infinity", "NaN", "0.0000005", "1e21", "-1e21", "0x" + "f".repeat(20)];
for (const s of pi) for (const r of [undefined, 0, 2, 8, 10, 16, 36, 37, 1, -1, 2.9, 4294967312]) add(`T(()=>parseInt(${q(s)}${r === undefined ? "" : "," + r}))`);
add("T(()=>parseInt(0.0000005))", "T(()=>parseInt(1e21))", "T(()=>parseInt(-0))", "T(()=>1/parseInt('-0'))", "T(()=>parseInt(null,36))", "T(()=>parseInt(15.99,10))", "T(()=>parseInt('11',null))", "T(()=>parseInt('11','2'))",
  "T(()=>parseInt('11',{valueOf(){return 3}}))", "T(()=>parseInt(1n))", "T(()=>parseInt(Symbol()))", "T(()=>parseInt===Number.parseInt)", "T(()=>parseInt.length)");

// ---- 7. Number().
const nums = ["", " ", "\n", "1", " 1 ", "1.5x", "0x10", "0X10", "0b101", "0B101", "0o17", "0O17", "-0x10", "+0x10", "0x", "0b2", "0o8", "1e3", "1e", ".5", "5.", ".", "-.5", "+.5", "--1", "1_0", "Infinity", "-Infinity", "+Infinity", "infinity", "NaN", "1e1000", "-1e1000", "1e-1000", " 1 ", "﻿1", " 1 ", "᠎1", "1\u0000", "٣", "0.1e1", "9007199254740993", "00012", "-0", "+0", "0e0", "1n", "0x1n", "123456789012345678901234567890", "0b" + "1".repeat(60), "0x" + "f".repeat(20)];
for (const s of nums) addSome(`T(()=>Number(${q(s)}))`, `T(()=>+${q(s)})`);
add("T(()=>Number())", "T(()=>Number(undefined))", "T(()=>Number(null))", "T(()=>Number(true))", "T(()=>Number(false))", "T(()=>Number([]))", "T(()=>Number([5]))", "T(()=>Number([1,2]))", "T(()=>Number({}))",
  "T(()=>Number(new Date(5)))", "T(()=>Number(1n))", "T(()=>Number(2n**64n))", "T(()=>Number(2n**1024n))", "T(()=>Number(-(2n**1024n)))", "T(()=>Number(2n**53n+1n))", "T(()=>Number(2n**53n+3n))", "T(()=>Number(0x1fffffffffffffn))",
  "T(()=>Number(Symbol()))", "T(()=>+Symbol())", "T(()=>+1n)", "T(()=>Number({valueOf(){return 3}}))", "T(()=>Number({valueOf(){return {}},toString(){return '4'}}))", "T(()=>Number({valueOf(){return {}},toString(){return {}}}))",
  "T(()=>Number({[Symbol.toPrimitive](h){return h==='number'?8:9}}))", "T(()=>Number(new Number(7)))", "T(()=>new Number(7)+1)", "T(()=>typeof new Number(1))", "T(()=>Number('12px'))", "T(()=>Number('0x'+'f'.repeat(300)))");

// ---- 8. Constantes e predicados de Number.
add("T(()=>Number.MAX_SAFE_INTEGER)", "T(()=>Number.MIN_SAFE_INTEGER)", "T(()=>Number.EPSILON)", "T(()=>Number.MIN_VALUE)", "T(()=>Number.MAX_VALUE)", "T(()=>B(Number.EPSILON))");
for (const v of specials.concat([2 ** 53 - 1, 2 ** 53 + 2, -(2 ** 53 - 1), 1.0000000000000002, "1", null, undefined, 1n, [1], new Number(1)]))
  add(`T(()=>[Number.isInteger(${lit(v)}),Number.isSafeInteger(${lit(v)}),Number.isFinite(${lit(v)}),Number.isNaN(${lit(v)}),isFinite(${lit(v)}),isNaN(${lit(v)})].join())`);

// ---- 9. Math: unárias com valores especiais e bits.
const unary = ["abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "cbrt", "ceil", "clz32", "cos", "cosh", "exp", "expm1", "floor", "fround", "log", "log1p", "log10", "log2", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc"];
const mv = [0, -0, 1, -1, 0.5, -0.5, 1.5, -1.5, 2.5, -2.5, 0.49999999999999994, -0.49999999999999994, 4503599627370495.5, -4503599627370495.5, 2 ** 52, 2 ** 53, 1e300, -1e300, 5e-324, -5e-324, Infinity, -Infinity, NaN, 2, 10, 100, 0.1, Math.PI, Math.E];
for (const f of unary) for (const v of mv) add(`T(()=>B(Math.${f}(${lit(v)})))`);
for (const f of unary) addSome(`T(()=>Math.${f}.length)`, `T(()=>Math.${f}())`, `T(()=>Math.${f}("2"))`, `T(()=>Math.${f}(null))`, `T(()=>Math.${f}({valueOf(){return 1}}))`);
add("T(()=>Math.abs(1n))", "T(()=>Math.sqrt(Symbol()))");
for (const v of [0.1, 0.9999999999999999, 1e-10, 1e10, 3, 8, 27, -8, 1e-300, 709.78, 710, -745, -746, 21, 22, 710.4758600739439, 0.5, 2 ** 31, 2 ** 32, 2 ** 32 + 1, -1, 4294967295, 65536, 0.3])
  add(`T(()=>[B(Math.exp(${v})),B(Math.expm1(${v})),B(Math.cbrt(${v})),B(Math.log1p(${v}))].join())`, `T(()=>[Math.clz32(${v}),B(Math.fround(${v})),Math.round(${v}),Math.sign(${v})].join())`);

// ---- 10. Math: binárias e variádicas.
const bv = [0, -0, 1, -1, 2, 0.5, -0.5, 3, 10, 2 ** 31, 1e308, 5e-324, Infinity, -Infinity, NaN];
for (const f of ["atan2", "pow", "imul", "hypot", "max", "min"]) for (const a of bv) for (const b of [0, -0, 1, -1, 0.5, 2, Infinity, -Infinity, NaN, 1e308]) add(`T(()=>B(Math.${f}(${lit(a)},${lit(b)})))`);
add("T(()=>Math.max())", "T(()=>Math.min())", "T(()=>1/Math.max(-0,0))", "T(()=>1/Math.max(0,-0))", "T(()=>1/Math.min(0,-0))", "T(()=>1/Math.min(-0,0))", "T(()=>Math.max(1,NaN,3))", "T(()=>Math.min(NaN,1))",
  "T(()=>Math.max('3',2))", "T(()=>Math.max(...[1,5,3]))", "T(()=>Math.max.apply(null,[]))", "T(()=>Math.max(1,{valueOf(){return 9}}))", "T(()=>Math.max(1n))",
  "T(()=>Math.hypot())", "T(()=>Math.hypot(3,4))", "T(()=>Math.hypot(-0))", "T(()=>Math.hypot(NaN,Infinity))", "T(()=>Math.hypot(Infinity,NaN))", "T(()=>Math.hypot(1e200,1e200))", "T(()=>Math.hypot(1e-200,1e-200))",
  "T(()=>Math.hypot(3,4,12))", "T(()=>Math.hypot.length)", "T(()=>Math.imul(0xffffffff,5))", "T(()=>Math.imul(2**31,2))", "T(()=>Math.imul(1.9,2.9))", "T(()=>Math.imul())", "T(()=>Math.pow(1,Infinity))", "T(()=>Math.pow(-1,Infinity))",
  "T(()=>Math.pow(NaN,0))", "T(()=>Math.pow(-8,1/3))", "T(()=>B(Math.pow(2,-1074)))", "T(()=>Math.pow(2,1024))", "T(()=>Math.pow(-0,-1))", "T(()=>Math.pow(-0,-2))", "T(()=>Math.pow(-0,3))", "T(()=>Math.atan2(0,-0))", "T(()=>Math.atan2(-0,-0))", "T(()=>Math.atan2(-0,0))");
add("T(()=>Math.random()>=0&&Math.random()<1)", "T(()=>typeof Math.random())", "T(()=>Math.random.length)", "T(()=>Object.prototype.toString.call(Math))", "T(()=>Math[Symbol.toStringTag])", "T(()=>Object.getOwnPropertyNames(Math).sort().join())",
  "T(()=>[Math.E,Math.LN10,Math.LN2,Math.LOG10E,Math.LOG2E,Math.PI,Math.SQRT1_2,Math.SQRT2].map(B).join())", "T(()=>Object.getOwnPropertyDescriptor(Math,'PI').writable)", "T(()=>new Math.abs())", "T(()=>Math())");

// ---- 11. ** e %.
const pw = [[2, 10], [2, -1], [2, 0.5], [-2, 0.5], [-2, 3], [-2, -3], [0, 0], [-0, 0], [NaN, 0], [0, NaN], [1, Infinity], [-1, Infinity], [1, NaN], [0.5, Infinity], [2, Infinity], [2, -Infinity], [-Infinity, 3], [-Infinity, 2], [-Infinity, -3], [Infinity, -1],
  [0, -1], [-0, -1], [-0, -2], [10, 308], [10, 309], [10, -324], [10, -325], [2, 1023], [2, 1024], [2, -1074], [2, -1075], [1e154, 2], [-1, 0.5], [-8, 1 / 3], [0.1, 3], [3, 40], [7, 0.5], [2 ** 53, 2], [-(2 ** 31), 2]];
for (const [a, b] of pw) add(`T(()=>B((${lit(a)})**(${lit(b)})))`);
add("T(()=>2**3**2)", "T(()=>(-2)**2)", "T(()=>2**-1)", "T(()=>{var x=2;x**=3;return x})", "T(()=>{var x=2;return x**=x**=2})", "T(()=>2n**64n)", "T(()=>(-2n)**3n)", "T(()=>2n**0n)", "T(()=>0n**0n)", "T(()=>2n**-1n)",
  "T(()=>2n**2)", "T(()=>2**2n)", "T(()=>2n**(2n**64n))", "T(()=>1n**(2n**64n))", "T(()=>(-1n)**(2n**64n+1n))", "T(()=>0n**(2n**64n))", "T(()=>10n**30n)", "T(()=>(-3n)**5n)", "T(()=>'2'**'3')", "T(()=>null**0)", "T(()=>undefined**0)", "T(()=>[]**2)");
const md = [[5, 3], [-5, 3], [5, -3], [-5, -3], [5.5, 2], [-5.5, 2], [0, 5], [-0, 5], [5, 0], [5, -0], [0, 0], [Infinity, 2], [-Infinity, 2], [2, Infinity], [-2, Infinity], [2, -Infinity], [NaN, 1], [1, NaN], [1e308, 3], [5e-324, 3], [3, 5e-324], [0.3, 0.1], [1, 0.1],
  [2 ** 53, 3], [-(2 ** 53), 3], [1e21, 7], [-1e21, 7], [4294967296, 4294967295], [-4, 4], [4, -4], [-0, 1], [-1, 1], [1.5, 0.5], [7, 2 ** -1074], [0.5, 1], [-0.5, 1]];
for (const [a, b] of md) add(`T(()=>B((${lit(a)})%(${lit(b)})))`);
add("T(()=>5n%3n)", "T(()=>-5n%3n)", "T(()=>5n%-3n)", "T(()=>-5n%-3n)", "T(()=>5n%0n)", "T(()=>0n%5n)", "T(()=>5n%3)", "T(()=>5%3n)", "T(()=>2n**100n%1000007n)", "T(()=>-(2n**100n)%1000007n)", "T(()=>1/(-0%5))", "T(()=>1/(-4%2))", "T(()=>1/(4%-2))",
  "T(()=>{var x=7;x%=4;return x})", "T(()=>{var x=7n;x%=4n;return x})", "T(()=>'7'%'4')", "T(()=>null%5)", "T(()=>undefined%5)", "T(()=>[]%5)", "T(()=>5n/2n)", "T(()=>-5n/2n)", "T(()=>5n/0n)", "T(()=>5n/2)");

// ---- 12. Shifts e bit a bit.
const sv = [0, -0, 1, -1, 2 ** 31, -(2 ** 31), 2 ** 32, 2 ** 32 + 1, 2 ** 31 - 1, 4294967295, 4294967296.9, -4294967297.5, 1.9, -1.9, 2 ** 53, 1e21, -1e21, 1e300, NaN, Infinity, -Infinity, 0.5, 123456789.987, "5", null, undefined, true, "0x10", "abc", [], [7]];
const sh = [0, 1, 5, 31, 32, 33, 63, -1, -31, -32, 1.9, 2 ** 32 + 3, NaN, Infinity, "4", null];
for (const a of sv) for (const b of [0, 1, 31, 32, 33, -1, NaN]) add(`T(()=>${typeof a === "string" ? q(a) : a === null || a === undefined || typeof a === "boolean" ? String(a) : Array.isArray(a) ? JSON.stringify(a) : lit(a)}>>>${lit(b)})`);
for (const a of [1, -1, 2 ** 31, -(2 ** 31), 2 ** 32 + 5, 0x7fffffff, 1.9, NaN, Infinity, 2 ** 53 + 2, -(2 ** 53) - 2, 1e21, "7"]) for (const b of sh) {
  const A = typeof a === "string" ? q(a) : lit(a), Bv = typeof b === "string" ? q(b) : b === null ? "null" : lit(b);
  add(`T(()=>(${A})<<(${Bv}))`, `T(()=>(${A})>>(${Bv}))`);
  if (b !== "4" && b !== null) add(`T(()=>(${A})>>>(${Bv}))`);
}
for (const a of [0, -1, 2 ** 31, 2 ** 32 + 7, 1.9, -1.9, NaN, Infinity, 1e21, "12", null, undefined]) for (const b of [0, 5, -1, 2 ** 31, 0xffffffff, 1.9, NaN, "3"]) {
  const A = typeof a === "string" ? q(a) : a === null || a === undefined ? String(a) : lit(a), Bv = typeof b === "string" ? q(b) : lit(b);
  add(`T(()=>[(${A})&(${Bv}),(${A})|(${Bv}),(${A})^(${Bv})].join())`);
}
add("T(()=>~0)", "T(()=>~-1)", "T(()=>~2147483648)", "T(()=>~4294967295)", "T(()=>~1.9)", "T(()=>~NaN)", "T(()=>~Infinity)", "T(()=>~'5')", "T(()=>~null)", "T(()=>~undefined)", "T(()=>~~-0.5)", "T(()=>1/(~~-0.5))", "T(()=>~5n)", "T(()=>~-1n)", "T(()=>~(2n**64n))",
  "T(()=>1n<<64n)", "T(()=>1n<<-1n)", "T(()=>-1n>>1n)", "T(()=>-1n>>1000n)", "T(()=>1n>>1000n)", "T(()=>-5n>>1n)", "T(()=>5n>>-2n)", "T(()=>(2n**100n)>>99n)", "T(()=>1n>>>0n)", "T(()=>1n<<1)", "T(()=>1<<1n)", "T(()=>5n&3n)", "T(()=>-5n&3n)", "T(()=>-5n|3n)", "T(()=>-5n^3n)",
  "T(()=>(-(2n**70n))&(2n**65n-1n))", "T(()=>(2n**70n)|-1n)", "T(()=>5n&3)", "T(()=>5n|1)", "T(()=>5n^1n)", "T(()=>{var x=1;x<<=33;return x})", "T(()=>{var x=-1;x>>>=0;return x})", "T(()=>{var x=-8;x>>=1;return x})", "T(()=>{var x=1n;x<<=70n;return x})",
  "T(()=>1<<31)", "T(()=>1<<32)", "T(()=>-1>>>31)", "T(()=>-1>>>32)", "T(()=>-1>>>0)", "T(()=>(-1>>>0)+1)", "T(()=>1/(0>>>0))", "T(()=>1/(-0>>>0))", "T(()=>1/(-0|0))");

// ---- 13. ++, --, e aritmética mista com BigInt.
const incs = ["0", "-0", "1", "'5'", "'abc'", "''", "' 7 '", "null", "undefined", "true", "false", "[]", "[3]", "({})", "({valueOf(){return 4}})", "1n", "-1n", "0n", "2n**64n", "Number.MAX_SAFE_INTEGER", "Number.MAX_SAFE_INTEGER+1", "NaN", "Infinity", "'1n'", "new Number(2)", "Object(1n)", "Symbol()", "1e308", "0.1"];
for (const e of incs) {
  addSome(`T(()=>{var x=${e};var o=x++;return S(o)+"|"+S(x)+"|"+typeof o})`, `T(()=>{var x=${e};var o=++x;return S(o)+"|"+S(x)})`, `T(()=>{var x=${e};var o=x--;return S(o)+"|"+S(x)})`, `T(()=>{var x=${e};var o=--x;return S(o)+"|"+S(x)})`,
    `T(()=>{var o={p:${e}};var r=o.p++;return S(r)+"|"+S(o.p)})`, `T(()=>{var a=[${e}];var r=--a[0];return S(r)+"|"+S(a[0])})`);
}
const mixedOps = ["+", "-", "*", "/", "%", "**", "<", ">", "<=", ">=", "==", "!=", "===", "&", "|", "^", "<<", ">>", ">>>"];
const mixedVals = ["1n", "2n", "0n", "-1n", "1", "0", "1.5", "'1'", "'1n'", "true", "null", "undefined", "NaN", "Infinity", "2**53", "2n**53n", "(2n**53n)+1n", "9007199254740993n", "[]", "({})", "Object(1n)", "Symbol()"];
for (const op of mixedOps) for (const a of ["1n", "2n**64n", "-3n", "1", "'1'", "0n", "9007199254740993n"]) for (const b of mixedVals) add(`T(()=>${a}${op}${b})`);
add("T(()=>-(2n**64n))", "T(()=>-0n)", "T(()=>Object.is(-0n,0n))", "T(()=>+(1n))", "T(()=>1n+'1')", "T(()=>'1'+1n)", "T(()=>1n+[])", "T(()=>`${2n**64n}`)", "T(()=>1n<2)", "T(()=>2n>1.5)", "T(()=>1n==1)", "T(()=>1n=='1')", "T(()=>1n===1)",
  "T(()=>9007199254740993n==9007199254740992)", "T(()=>9007199254740993n>9007199254740992)", "T(()=>9007199254740993n<9007199254740994)", "T(()=>2n**1024n>Number.MAX_VALUE)", "T(()=>2n**1024n==Infinity)", "T(()=>2n**1024n<Infinity)", "T(()=>1n<NaN)", "T(()=>1n==NaN)",
  "T(()=>0n==-0)", "T(()=>0n==false)", "T(()=>1n=='1.5')", "T(()=>1n<'x')", "T(()=>1n<'2')", "T(()=>'0x10'==16n)", "T(()=>[1n,2,3n,1.5,0n,-1n].sort((a,b)=>a<b?-1:a>b?1:0).join())", "T(()=>BigInt(1.5))", "T(()=>BigInt(2**53))", "T(()=>BigInt('0x10'))",
  "T(()=>BigInt(' 12 '))", "T(()=>BigInt(''))", "T(()=>BigInt('1e3'))", "T(()=>BigInt(NaN))", "T(()=>BigInt(Infinity))", "T(()=>BigInt(true))", "T(()=>BigInt(null))", "T(()=>BigInt(undefined))", "T(()=>BigInt(1e21))", "T(()=>BigInt(-0))",
  "T(()=>BigInt.asIntN(8,255n))", "T(()=>BigInt.asUintN(8,-1n))", "T(()=>BigInt.asIntN(64,2n**63n))", "T(()=>BigInt.asUintN(64,-1n))", "T(()=>BigInt.asIntN(0,5n))", "T(()=>BigInt.asUintN(0,5n))", "T(()=>BigInt.asIntN(-1,5n))", "T(()=>BigInt.asUintN(2**53,5n))",
  "T(()=>(255n).toString(2))", "T(()=>(-255n).toString(36))", "T(()=>(255n).toString(37))", "T(()=>(2n**64n).toString(16))", "T(()=>0n.toString(2))", "T(()=>(2n**200n).toString(36))", "T(()=>(10n**30n).toLocaleString('en-US'))");

// ---- Execução.
const bases = ["number_format_bun", "number_format_more_bun", "number_compact_bun", "bigint_bun"].map(n => {
  try { return fs.readFileSync(path.join(__dirname, "..", "tests", "golden", n + ".tsv"), "utf8"); } catch (e) { return ""; }
}).join("\n");
const seen = new Set();
const keepMatrix = thin("matrix", STRIDE);
const keepSome = thin("some", 3);
const unique = exprs.filter((e, i) => keepMatrix(e, i) && keepSome(e, i) && !seen.has(e) && seen.add(e));
let kept = 0, dropped = 0, dup = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{return ${expr}})`);
  if (bases.includes(JSON.stringify(expr).slice(1, -1))) { dup++; continue; }
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) { dropped++; continue; }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos de outros goldens ${dup}\n`);
