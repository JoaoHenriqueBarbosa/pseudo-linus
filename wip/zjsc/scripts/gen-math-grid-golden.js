// Gera tests/golden/math_grid_bun.tsv: grade de Math.* sobre valores exatos e de fronteira, medido no bun 1.4.2.
// Cobre round/trunc/sign/abs/floor/ceil/max/min/clz32/imul/fround/f16round/sqrt/cbrt/hypot/pow/atan2/sumPrecise em
// grade (±0, subnormais, MIN_VALUE, MAX_VALUE, 2**53±1, 0.5±ulp, múltiplos de π, NaN, ±Infinity), pow com os casos
// especiais de NaN/±0/±Infinity/1, hypot com muitos argumentos, poucas transcendentais em valores de fronteira e
// a coerção dos argumentos registrando a ordem das chamadas de valueOf. O resultado sai com toString (e "-0" para
// o zero negativo); o último ulp das transcendentais não é medido à parte: vale o que o bun devolve.
// Programas cuja instrução final já aparece em outro golden com `Math.` são descartados (dedup).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-math-grid-golden.js > tests/golden/math_grid_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var L=[];function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);return "obj"}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?L.join()+"=>"+r:r}\n' +
  'function O(v,n){return{valueOf(){L.push(n);return v}}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const call = (f, ...args) => `T(()=>Math.${f}(${args.join(",")}))`;

// Valores exatos e de fronteira, como literais JavaScript.
const V = [
  "0", "-0", "1", "-1", "0.5", "-0.5", "1.5", "-1.5", "2.5", "-2.5", "0.49999999999999994", "0.5000000000000001",
  "-0.49999999999999994", "-0.5000000000000001", "1.4999999999999998", "2.5000000000000004", "NaN", "Infinity", "-Infinity",
  "5e-324", "-5e-324", "2.225073858507201e-308", "2.2250738585072014e-308", "1.7976931348623157e308", "-1.7976931348623157e308",
  "2**53-1", "2**53", "2**53+2", "-(2**53-1)", "-(2**53)", "4503599627370495.5", "4503599627370496", "2**31", "-(2**31)", "2**32",
  "2**32-1", "-(2**32)", "2**31-1", "65504", "65520", "65519.99", "6.103515625e-5", "5.960464477539063e-8", "2.9802322387695312e-8",
  "3e-8", "1e21", "1e-7", "100", "27", "0.1", "-0.1", "123456.789", "Math.PI", "-Math.PI", "Math.PI/2", "Math.PI*2", "3*Math.PI",
  "Math.E", "4", "9", "-8", "64", "1e300", "1e-300", "16777216", "16777217", "33554434", "0.30000000000000004",
];

// ---- 1. Funções exatas unárias sobre toda a grade.
for (const f of ["abs", "ceil", "floor", "round", "trunc", "sign", "sqrt", "cbrt", "fround", "f16round", "clz32"]) {
  for (const v of V) add(call(f, v));
}

// ---- 2. cbrt de cubos exatos, sqrt de quadrados exatos, hypot de inteiros.
for (const n of [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 20, 25, 50, 100, 1000, 1024, 99999]) {
  add(call("cbrt", `${n}**3`), call("cbrt", `-(${n}**3)`), call("sqrt", `${n}**2`), call("sqrt", `${n}**2+1`), call("sqrt", `${n}**2-1`));
}
for (const [a, b] of [[3, 4], [5, 12], [8, 15], [7, 24], [20, 21], [9, 40], [0, 5], [5, 0], [1, 1], [-3, -4], [3e200, 4e200], [3e-200, 4e-200], [1e308, 1e308], [2**53, 1]]) {
  add(call("hypot", a, b), call("hypot", a, b, 0));
}
for (const [a, b, c] of [[1, 2, 2], [2, 3, 6], [1, 4, 8], [4, 4, 7], [2, 10, 11], [-1, -2, -2], [6, 6, 7]]) add(call("hypot", a, b, c));

// ---- 3. clz32 com coerções e inteiros de 32 bits.
for (const v of ["0", "1", "2", "3", "7", "8", "255", "256", "65535", "65536", "2**24", "2**30", "2**31", "2**31+1", "2**32", "2**32+1", "-1", "-2", "-(2**31)",
  "0.9", "1.9", "-0.9", "NaN", "Infinity", "'1'", "'0x10'", "' 12 '", "''", "'abc'", "null", "undefined", "true", "false", "[]", "[5]", "[1,2]", "{}", "1n", "2**53", "1e10", "-1e10", "4294967295.9", "0.5", "0xffffffff", "0x7fffffff", "0x80000000"]) {
  add(call("clz32", v));
}
add(call("clz32"), call("clz32", "1", "2"), "T(()=>Math.clz32(Symbol()))", "T(()=>Math.clz32(1n))");
for (let shift = 0; shift < 34; shift += 1) add(call("clz32", `2**${shift}`), call("clz32", `2**${shift}-1`));

// ---- 4. imul sobre pares.
const I = ["0", "-0", "1", "-1", "2", "-2", "3", "0x7fffffff", "0x80000000", "0xffffffff", "-(2**31)", "2**31-1", "2**32", "2**32+1", "65536", "65535",
  "0xdeadbeef", "NaN", "Infinity", "-Infinity", "0.9", "-0.9", "1.9", "2**53", "'5'", "null", "undefined"];
for (const a of I) for (const b of I) add(call("imul", a, b));
add(call("imul"), call("imul", "5"), call("imul", "5", "6", "7"), "T(()=>Math.imul(1n,2))", "T(()=>Math.imul(Symbol(),2))");

// ---- 5. max e min: zeros com sinal, NaN, ordem, muitos argumentos.
const M = ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "5e-324", "-5e-324", "1.7976931348623157e308", "0.5", "2**53", "undefined", "null", "'3'", "''", "true", "[]", "[7]"];
for (const f of ["max", "min"]) {
  for (const a of M) for (const b of M) add(call(f, a, b));
  add(call(f), call(f, "0"), call(f, "-0"), call(f, "NaN"), call(f, "0", "-0", "0"), call(f, "-0", "0", "-0"), call(f, "-0", "-0"), call(f, "0", "0"));
  add(call(f, "1", "2", "3", "NaN", "5"), call(f, "NaN", "Infinity", "-Infinity"), call(f, "...[1,2,3,4,5,6,7,8,9,10]"), call(f, "...[]"), call(f, "...[-0,0,-0]"));
  add(call(f, "...Array.from({length:1000},(_,i)=>i%7-3)"), call(f, "...Array.from({length:50000},(_,i)=>i)"));
  add(`T(()=>Math.${f}(O(1,'a'),O(NaN,'b'),O(3,'c')))`, `T(()=>Math.${f}(O(NaN,'a'),O(2,'b')))`, `T(()=>Math.${f}(O(1,'a'),{valueOf(){throw new RangeError('x')}},O(3,'c')))`,
    `T(()=>Math.${f}(O(0,'a'),O(-0,'b')))`, `T(()=>Math.${f}(O('5','a'),O(null,'b'),O(undefined,'c')))`, `T(()=>Math.${f}(1n))`, `T(()=>Math.${f}(1,Symbol()))`,
    `T(()=>Math.${f}.length+Math.${f}.name)`);
}

// ---- 6. pow: casos especiais da especificação e inteiros.
const P = ["0", "-0", "1", "-1", "2", "-2", "0.5", "-0.5", "3", "-3", "Infinity", "-Infinity", "NaN", "5e-324", "1.7976931348623157e308", "1.5", "-1.5",
  "2**53", "2**31", "10", "-10", "1.0000000000000002", "0.9999999999999999", "-0.9999999999999999", "-1.0000000000000002", "0.1", "2**-1074"];
for (const a of P) for (const b of P) add(call("pow", a, b));
for (const base of [2, 3, 5, 7, 10, -2, -3, -10]) for (const e of [0, 1, 2, 3, 5, 10, 20, 30, 52, 53, 62, 63, 64, 100, 300, 1023, 1024, -1, -2, -10, -1074, -1075]) add(call("pow", base, e));
for (const [a, b] of [["NaN", "0"], ["NaN", "-0"], ["1", "NaN"], ["-1", "Infinity"], ["-1", "-Infinity"], ["1", "Infinity"], ["1", "-Infinity"], ["1", "NaN"], ["0", "NaN"],
  ["-0", "-3"], ["-0", "-2"], ["-0", "3"], ["-0", "2"], ["-0", "-Infinity"], ["0", "-Infinity"], ["-Infinity", "3"], ["-Infinity", "2"], ["-Infinity", "-3"], ["-Infinity", "-2"],
  ["-Infinity", "0.5"], ["-8", "1/3"], ["-8", "0.5"], ["-1", "0.5"], ["2", "-0"], ["-0", "-0"], ["2", "0.5"], ["4", "0.5"], ["4", "-0.5"], ["0.5", "-1074"], ["0.5", "1074"],
  ["1.0000000000000002", "2**52"], ["1.0000000000000002", "2**53"], ["0.9999999999999999", "2**53"], ["'2'", "'3'"], ["null", "null"], ["undefined", "1"], ["[]", "[]"], ["[2]", "[3]"], ["true", "true"]]) {
  add(call("pow", a, b));
}
add("T(()=>Math.pow())", "T(()=>Math.pow(2))", "T(()=>Math.pow(2,3,4))", "T(()=>(-8)**(1/3))", "T(()=>2**-1074)", "T(()=>(-0)**-1)", "T(()=>NaN**0)", "T(()=>1**Infinity)",
  "T(()=>Math.pow(1n,2))", "T(()=>Math.pow(2,Symbol()))", "T(()=>Math.pow(O(2,'a'),O(3,'b')))", "T(()=>Math.pow(O(NaN,'a'),O(0,'b')))", "T(()=>Math.pow(O(2,'a'),{valueOf(){throw new RangeError('b')}}))");

// ---- 7. atan2 em quadrantes e zeros/infinitos.
const A = ["0", "-0", "1", "-1", "2", "-2", "0.5", "5e-324", "-5e-324", "1.7976931348623157e308", "-1.7976931348623157e308", "Infinity", "-Infinity", "NaN", "1e-300", "-1e-300", "3", "-3"];
for (const y of A) for (const x of A) add(call("atan2", y, x));
add(call("atan2"), call("atan2", "1"), "T(()=>Math.atan2(O(1,'y'),O(2,'x')))", "T(()=>Math.atan2(O(NaN,'y'),O(2,'x')))", "T(()=>Math.atan2(1n,1))", "T(()=>Math.atan2(1,Symbol()))");

// ---- 8. hypot: zeros, Infinity vence NaN, muitos argumentos, escala.
const H = ["0", "-0", "1", "-3", "4", "NaN", "Infinity", "-Infinity", "5e-324", "1.7976931348623157e308", "1e200", "1e-200", "0.5", "'3'", "null", "undefined", "[]"];
for (const a of H) for (const b of H) add(call("hypot", a, b));
add(call("hypot"), call("hypot", "0"), call("hypot", "-0"), call("hypot", "-0", "-0"), call("hypot", "NaN"), call("hypot", "-5"), call("hypot", "Infinity"), call("hypot", "NaN", "Infinity"),
  call("hypot", "Infinity", "NaN"), call("hypot", "NaN", "-Infinity", "NaN"), call("hypot", "1", "NaN", "3"), call("hypot", "1e308", "1e308", "1e308"), call("hypot", "5e-324", "5e-324"),
  call("hypot", "...Array.from({length:10},()=>3)"), call("hypot", "...Array.from({length:100},()=>1)"), call("hypot", "...Array.from({length:10000},()=>1)"),
  call("hypot", "...Array.from({length:4},()=>1e200)"), call("hypot", "...Array.from({length:9},()=>1e-200)"), call("hypot", "...Array.from({length:16},(_,i)=>i)"),
  call("hypot", "...[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20]"), call("hypot", "...[2,3,6,0,0,0,0,0,0,0,0,0,0,0,0]"), call("hypot", "...[1e154,1e154,1e154]"),
  call("hypot", "...[-1,-1,-1,-1]"), call("hypot", "...[0,0,0,0,0,0]"), call("hypot", "...[-0,-0,-0,-0,-0]"), call("hypot", "...[3,4,NaN,Infinity]"), call("hypot", "...[NaN,NaN,NaN,Infinity]"),
  "T(()=>Math.hypot(O(3,'a'),O(NaN,'b'),O(Infinity,'c')))", "T(()=>Math.hypot(O(3,'a'),O(4,'b')))", "T(()=>Math.hypot(O(Infinity,'a'),{valueOf(){throw new RangeError('b')}}))",
  "T(()=>Math.hypot(1n,2))", "T(()=>Math.hypot(Symbol()))", "T(()=>Math.hypot.length+Math.hypot.name)");

// ---- 9. sumPrecise: precisão exata, zeros, NaN, infinitos, iteráveis.
const sums = [
  "[]", "[0]", "[-0]", "[-0,-0]", "[0,-0]", "[-0,0]", "[1,2,3]", "[0.1,0.2,0.3]", "[0.1,0.2]", "[1e20,0.1,-1e20]", "[1e308,1e308]", "[1e308,1e308,-1e308]",
  "[1e308,1e308,-1e308,-1e308]", "[1.7976931348623157e308,1.7976931348623157e308]", "[1.7976931348623157e308,-1.7976931348623157e308]", "[5e-324,5e-324]", "[5e-324,-5e-324]",
  "[Infinity,1]", "[Infinity,-Infinity]", "[-Infinity,-Infinity]", "[NaN,1]", "[1,NaN,Infinity]", "[Infinity,NaN]", "[2**53,1,1]", "[2**53,1]", "[1,2**53,1]", "[2**53,-1,-1]",
  "[1,1e100,1,-1e100]", "[1e100,1,-1e100,1]", "[0.5,2**53]", "[0.5,0.5,2**53]", "[1,2**-53]", "[1,2**-53,2**-53]", "[1,2**-54,2**-54]", "[1,2**-53,2**-105]", "[-1,-(2**-53),-(2**-105)]",
  "[1.1,2.2,3.3]", "[0.1,0.2,0.3,0.4,0.5,0.6,0.7]", "Array.from({length:10},()=>0.1)", "Array.from({length:1000},()=>0.1)", "Array.from({length:100},(_,i)=>i%2?-1e16:1e16)",
  "Array.from({length:3},()=>1e308)", "[1e308,-1e308,1e-308]", "[-5e-324]", "[5e-324]", "[2**1023,2**1023]", "[2**1023,2**1023,-(2**1023)]", "[9007199254740993,-9007199254740992]",
  "[4503599627370495.5,0.5]", "[0.9999999999999999,5.551115123125783e-17]", "[0.9999999999999999,1.1102230246251565e-16]", "[1,-1]", "[-1,1]", "[0,0,0]", "[-0,-0,-0]", "[-0,-0,0]",
  "new Set([1,2,3])", "new Set([0.1,0.2,0.3])", "(function*(){yield 1;yield 2})()", "(function*(){yield 0.1;yield 0.2;yield 0.3})()", "'abc'", "new Map()", "[,]", "[1,,3]",
];
for (const s of sums) add(call("sumPrecise", s));
add("T(()=>Math.sumPrecise())", "T(()=>Math.sumPrecise(undefined))", "T(()=>Math.sumPrecise(null))", "T(()=>Math.sumPrecise(1))", "T(()=>Math.sumPrecise({}))", "T(()=>Math.sumPrecise({length:1,0:1}))",
  "T(()=>Math.sumPrecise([1n]))", "T(()=>Math.sumPrecise(['1']))", "T(()=>Math.sumPrecise([true]))", "T(()=>Math.sumPrecise([null]))", "T(()=>Math.sumPrecise([undefined]))", "T(()=>Math.sumPrecise([Symbol()]))",
  "T(()=>Math.sumPrecise([{valueOf(){return 1}}]))", "T(()=>Math.sumPrecise([1],[2]))", "T(()=>Math.sumPrecise.length+Math.sumPrecise.name)",
  "T(()=>{var log=[];var it={[Symbol.iterator](){log.push('iter');return{next(){log.push('next');return{done:true}},return(){log.push('return')}}}};var r=Math.sumPrecise(it);return log.join()+'|'+r})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{i:0,next(){log.push('next');return this.i++<2?{done:false,value:this.i}:{done:true}},return(){log.push('return')}}}};var r=Math.sumPrecise(it);return log.join()+'|'+r})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{i:0,next(){log.push('next');return this.i++<2?{done:false,value:'x'}:{done:true}},return(){log.push('return')}}}};try{Math.sumPrecise(it)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{next(){log.push('next');return{done:false,value:{valueOf(){return 1}}}},return(){log.push('return')}}}};try{Math.sumPrecise(it)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var it={[Symbol.iterator](){return{i:0,next(){return this.i++<3?{done:false,value:O(1,'v'+this.i)}:{done:true}}}}};return Math.sumPrecise(it)})",
  "T(()=>Math.sumPrecise([1,2,3,NaN,Infinity]))", "T(()=>Math.sumPrecise([-Infinity,1,Infinity]))", "T(()=>Math.sumPrecise([Infinity,Infinity,-1e308]))");

// ---- 10. fround e f16round: arredondamento, ties, subnormais, overflow.
const R32 = ["16777217", "16777219", "33554435", "1.0000000596046448", "1.00000005960464478", "1.00000017881393432", "3.4028234663852886e38", "3.4028235677973366e38", "3.4028235677973362e38",
  "1.401298464324817e-45", "7.006492321624085e-46", "7.006492321624087e-46", "-7.006492321624085e-46", "1.1754943508222875e-38", "0.1", "0.3", "1e-46", "1e39", "-1e39", "5e-324", "0.1+0.2"];
for (const v of R32) add(call("fround", v), call("f16round", v));
const R16 = ["65519", "65519.99999999999", "65520", "65535", "65536", "1.0004882812500001", "1.00048828125", "1.000244140625", "1.0002441406250002", "2049", "2050", "2051", "4097", "4098", "4099",
  "6.097555160522461e-5", "6.103515625e-5", "5.9604644775390625e-8", "2.9802322387695312e-8", "2.98023223876953125e-8", "2.9802322387695316e-8", "8.940696716308594e-8", "1.1920928955078125e-7",
  "-65520", "-2.9802322387695312e-8", "-2.9802322387695316e-8", "0.1", "0.333333333333", "1e-8", "1e-5", "1000.5", "2047.5", "2048.5", "2049.5"];
for (const v of R16) add(call("f16round", v), call("fround", v));
add(call("fround"), call("f16round"), "T(()=>Math.fround(1n))", "T(()=>Math.f16round(Symbol()))", "T(()=>Math.fround(O(0.1,'a')))", "T(()=>Math.f16round(O(0.1,'a'),O(2,'b')))",
  "T(()=>Math.fround.length+Math.fround.name+Math.f16round.length+Math.f16round.name)");

// ---- 11. Transcendentais em valores de fronteira (poucas).
const TV = ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "5e-324", "-5e-324", "1.7976931348623157e308", "0.5", "-0.5", "2", "Math.PI", "Math.PI/2", "Math.PI/4", "1e22", "2**53", "709.782712893384", "-745.1332191019412", "1e-300"];
for (const f of ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "exp", "expm1", "log", "log10", "log2", "log1p"]) {
  for (const v of TV) add(call(f, v));
}
for (const k of [0, 1, 2, 3, 4, 6, 8, 12, 100, 1000]) add(call("sin", `${k}*Math.PI`), call("cos", `${k}*Math.PI`), call("tan", `${k}*Math.PI/4`));
for (const f of ["sin", "cos", "tan", "exp", "log", "sqrt", "cbrt", "abs", "sign", "trunc", "floor", "ceil", "round", "asin", "atan", "tanh", "log2", "log10", "expm1", "log1p"]) {
  add(`T(()=>Math.${f}(O(0.5,'a')))`, `T(()=>Math.${f}(O('0.5','a'),O(9,'b')))`, `T(()=>Math.${f}({valueOf(){throw new RangeError('v')},toString(){return '1'}}))`,
    `T(()=>Math.${f}({valueOf(){return {}},toString(){return '2'}}))`, `T(()=>Math.${f}({valueOf:null,toString(){return '3'}}))`, `T(()=>Math.${f}({valueOf(){return {}},toString(){return {}}}))`,
    `T(()=>Math.${f}(1n))`, `T(()=>Math.${f}(Symbol()))`, `T(()=>Math.${f}())`, `T(()=>Math.${f}(null))`, `T(()=>Math.${f}(undefined))`, `T(()=>Math.${f}(true))`, `T(()=>Math.${f}('  0x1F  '))`,
    `T(()=>Math.${f}([]))`, `T(()=>Math.${f}([4]))`, `T(()=>Math.${f}([1,2]))`, `T(()=>Math.${f}(new Date(5)))`, `T(()=>Math.${f}({[Symbol.toPrimitive](h){L.push(h);return 0.25}}))`,
    `T(()=>Math.${f}.length+':'+Math.${f}.name)`);
}

// ---- 12. Forma do objeto Math.
add("T(()=>Math[Symbol.toStringTag]+Object.prototype.toString.call(Math))", "T(()=>typeof Math+typeof Math.max)", "T(()=>new Math())", "T(()=>Math())", "T(()=>new Math.max())",
  "T(()=>[Math.PI,Math.E,Math.LN2,Math.LN10,Math.LOG2E,Math.LOG10E,Math.SQRT2,Math.SQRT1_2].join())", "T(()=>Object.getOwnPropertyNames(Math).sort().join())",
  "T(()=>Object.getOwnPropertyNames(Math).sort().map(k=>{var d=Object.getOwnPropertyDescriptor(Math,k);return k+':'+d.writable+d.enumerable+d.configurable}).join())",
  "T(()=>{'use strict';Math.PI=3;return Math.PI})", "T(()=>{Math.PI=3;return Math.PI})", "T(()=>delete Math.PI)", "T(()=>Object.keys(Math).length)", "T(()=>Object.getPrototypeOf(Math)===Object.prototype)",
  "T(()=>Object.getOwnPropertyNames(Math).sort().map(k=>typeof Math[k]==='function'?k+'='+Math[k].length:k).join())");

// ---- 13. round, floor, ceil, trunc com sinais e frações sobre grade de inteiros e metades.
for (const n of [0, 1, 2, 3, 4, 7, 8, 15, 16, 100, 1000, 2**24, 2**31, 2**32, 2**52 - 1, 2**52, 2**52 + 1]) {
  for (const f of ["round", "floor", "ceil", "trunc"]) {
    add(call(f, `${n}+0.5`), call(f, `-(${n}+0.5)`), call(f, `${n}-0.5`), call(f, `-${n}+0.25`), call(f, `${n}+0.75`));
  }
}
for (const f of ["round", "floor", "ceil", "trunc", "sign"]) {
  add(`T(()=>Object.is(Math.${f}(-0.1),-0))`, `T(()=>Object.is(Math.${f}(-0),-0))`, `T(()=>Object.is(Math.${f}(0.1),0))`, `T(()=>1/Math.${f}(-0.4))`, `T(()=>1/Math.${f}(-0.5))`, `T(()=>1/Math.${f}(0.49999999999999994))`);
}

// ---- Execução: dedup contra as instruções finais dos goldens existentes e filhos em paralelo (máximo 8, 5 s cada).
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const baseLines = [];
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "math_grid_bun.tsv") continue;
  try {
    for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
      const tab = line.indexOf("\t");
      if (tab > 0 && line.lastIndexOf("Math.", tab) >= 0) baseLines.push(line.slice(0, tab));
    }
  } catch (e) {}
}
const baseText = baseLines.join("\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let dup = 0;
const todo = [];
for (const expr of unique) {
  const needle = "globalThis.R = " + expr;
  if (baseText.includes(JSON.stringify(needle).slice(1, -1) + '"')) { dup++; continue; }
  todo.push({ expr, source: '"use strict";\n' + PRELUDE + needle });
}

function runChild(source) {
  return new Promise((resolve) => {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", d => { err += d; });
    child.on("close", code => { clearTimeout(timer); const decoded = code === 0 ? decodeResult(out) : null; resolve(decoded !== null ? { out: decoded } : { error: err || "código " + code }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(todo.length);
  let next = 0;
  async function worker() {
    for (;;) {
      const i = next++;
      if (i >= todo.length) return;
      results[i] = await runChild(todo[i].source);
    }
  }
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < todo.length; i++) {
    const { expr, source } = todo[i];
    const r = results[i];
    if (r.error !== undefined) {
      process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + r.error.slice(0, 200) + "\n");
      dropped++;
      continue;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push(JSON.stringify(source) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("math_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
