// Gera tests/golden/number_proto_bun.tsv: Number.prototype medido no bun 1.4.2.
// Cobre toFixed, toPrecision, toExponential e toString(radix) em grades de valor por argumento (limites do argumento,
// arredondamento de metades, expoentes, zero negativo, NaN e infinitos, RangeError), o receptor (primitivo, objeto Number,
// receptor inválido), a coerção do argumento (valueOf, toString, símbolo, BigInt), valueOf e as constantes de Number.
// Programas cuja expressão já aparece nos goldens number_edge, number_format e number_compact são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-number-proto-golden.js > tests/golden/number_proto_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

const values = [
  "0", "-0", "1", "-1", "0.5", "1.5", "2.5", "-2.5", "0.05", "0.045", "1.005", "1.45", "8.345", "10.235", "123.456", "-123.456",
  "0.000001", "0.0000001", "1e21", "1e-7", "123456789012345678901234", "1.7976931348623157e308", "5e-324", "2**53", "2**31", "0.1+0.2",
  "NaN", "Infinity", "-Infinity", "255", "4294967296", "1/3", "100", "999.9999", "0.9999", "9.5", "99.5", "1e20", "123456", "0.00123",
];

// ---- 1. toFixed: valor por dígitos.
const fixedDigits = ["undefined", "0", "1", "2", "3", "5", "10", "20", "99", "100", "101", "-1", "-0", "1.9", "'2'", "'x'", "NaN", "null", "true", "Infinity", "-Infinity", "{valueOf(){return 3}}"];
for (const v of values) for (const d of fixedDigits) add(`T(()=>(${v}).toFixed(${d}))`);

// ---- 2. toPrecision: valor por precisão.
const precs = ["undefined", "0", "1", "2", "3", "4", "6", "10", "21", "22", "100", "101", "-1", "1.9", "'3'", "'x'", "NaN", "null", "Infinity", "{valueOf(){return 5}}"];
for (const v of values) for (const p of precs) add(`T(()=>(${v}).toPrecision(${p}))`);

// ---- 3. toExponential: valor por dígitos.
const expDigits = ["undefined", "0", "1", "2", "3", "5", "10", "20", "100", "101", "-1", "1.9", "'2'", "'x'", "NaN", "null", "Infinity", "{valueOf(){return 4}}"];
for (const v of values) for (const d of expDigits) add(`T(()=>(${v}).toExponential(${d}))`);

// ---- 4. toString(radix).
const radices = ["undefined", "2", "3", "7", "8", "10", "16", "32", "36", "1", "37", "0", "-1", "2.9", "'16'", "'x'", "NaN", "null", "Infinity", "{valueOf(){return 8}}"];
const radixValues = ["0", "-0", "1", "-1", "255", "-255", "0.5", "0.1", "1/3", "123.456", "-123.456", "2**53", "1e21", "1e-7", "NaN", "Infinity", "-Infinity", "4294967295", "0.000001", "3.14159"];
for (const v of radixValues) for (const r of radices) add(`T(()=>(${v}).toString(${r}))`);

// ---- 5. Receptor e coerção.
const methods = ["toFixed", "toPrecision", "toExponential", "toString", "valueOf", "toLocaleString"];
const receivers = ["1", "new Number(1.5)", "Object(2)", "'1'", "true", "null", "undefined", "{}", "[]", "Symbol()", "1n", "function(){}", "new Boolean(true)", "{valueOf(){return 1}}"];
for (const m of methods) for (const r of receivers) add(`T(()=>Number.prototype.${m}.call(${r}))`, `T(()=>Number.prototype.${m}.call(${r},2))`);
add(
  "T(()=>Number.prototype.toString())", "T(()=>Number.prototype.valueOf())", "T(()=>Number.prototype.toFixed())", "T(()=>Number.prototype.toFixed(2))",
  "T(()=>Object.prototype.toString.call(Number.prototype))", "T(()=>Number.prototype.constructor===Number)", "T(()=>Number.prototype instanceof Number)",
  "T(()=>typeof Number.prototype)", "T(()=>Number.prototype+1)", "T(()=>Object.getPrototypeOf(Number.prototype)===Object.prototype)",
  "T(()=>Reflect.ownKeys(Number.prototype).map(String).join())", "T(()=>Reflect.ownKeys(Number).map(String).join())",
  "T(()=>[Number.prototype.toFixed.length,Number.prototype.toPrecision.length,Number.prototype.toExponential.length,Number.prototype.toString.length,Number.prototype.valueOf.length,Number.prototype.toLocaleString.length].join())",
  "T(()=>[Number.prototype.toFixed.name,Number.prototype.toPrecision.name,Number.prototype.toExponential.name,Number.prototype.toString.name].join())",
  "T(()=>new Number(5).toFixed(1))", "T(()=>{var n=new Number(5);n.x=1;return n.toFixed(1)+n.x})", "T(()=>{class N extends Number{}return new N(7).toFixed(2)})",
  "T(()=>{class N extends Number{}return new N(7).toString(2)})", "T(()=>{class N extends Number{}return N.prototype.valueOf.call(new N(3))})",
  "T(()=>{var o=Object.create(Number.prototype);return o.valueOf()})", "T(()=>new Proxy(new Number(1),{}).valueOf())",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 2}};(1).toFixed(d);return log.join()})",
  "T(()=>{var log=[];var d={toString(){log.push('t');return '2'}};(1).toFixed(d);return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 2}};NaN.toFixed(d);return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 200}};try{(1).toFixed(d)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 2}};Infinity.toExponential(d);return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 2}};Infinity.toPrecision(d);return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 200}};Infinity.toExponential(d);return log.join()})",
  "T(()=>{var log=[];var d={valueOf(){log.push('v');return 200}};try{Infinity.toPrecision(d)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>(1).toFixed(Symbol()))", "T(()=>(1).toFixed(1n))", "T(()=>(1).toPrecision(Symbol()))", "T(()=>(1).toExponential(1n))", "T(()=>(1).toString(Symbol()))",
  "T(()=>(1).toString(2n))", "T(()=>(1).toFixed({valueOf(){throw new RangeError('q')}}))", "T(()=>(1).toString({toString(){throw new EvalError('q')}}))",
  "T(()=>(1).toFixed({valueOf(){return {}},toString(){return '3'}}))", "T(()=>(1).toFixed({valueOf(){return {},toString(){return {}}}}))",
  "T(()=>(1).toFixed({[Symbol.toPrimitive](h){return h==='number'?4:9}}))",
  "T(()=>(255).toString(16).toUpperCase())", "T(()=>(0.1).toString(2))", "T(()=>(0.1).toString(3))", "T(()=>(-0).toString(2))", "T(()=>(2**-1074).toString(2).length)",
  "T(()=>(2**1023).toString(36))", "T(()=>(1.7976931348623157e308).toString(2).length)", "T(()=>(1e300).toString(7))", "T(()=>(123456789.123456789).toString(36))",
  "T(()=>(0.5).toString(36))", "T(()=>(1/3).toString(36))", "T(()=>(-1/3).toString(3))", "T(()=>(Math.PI).toString(16))", "T(()=>(Math.E).toString(8))",
  "T(()=>(1e21).toString(10))", "T(()=>(1e21).toString(2).length)", "T(()=>(123456789012345680000).toString(10))", "T(()=>(1e-7).toString(10))",
  "T(()=>(4.35).toFixed(1))", "T(()=>(4.45).toFixed(1))", "T(()=>(1.255).toFixed(2))", "T(()=>(1000000000000000128).toFixed(0))", "T(()=>(1000000000000000128).toString())",
  "T(()=>(0.000001).toFixed(7))", "T(()=>(-0.0000001).toFixed(2))", "T(()=>(-0.5).toFixed(0))", "T(()=>(0.5).toFixed(0))", "T(()=>(1.5).toFixed(0))", "T(()=>(2.5).toFixed(0))",
  "T(()=>(1e21).toFixed(2))", "T(()=>(-1e21).toFixed(2))", "T(()=>(1e21-1).toFixed(2))", "T(()=>(5e-324).toFixed(100))", "T(()=>(1.7976931348623157e308).toFixed(20))",
  "T(()=>(25).toPrecision(1))", "T(()=>(35).toPrecision(1))", "T(()=>(99.99).toPrecision(3))", "T(()=>(1e21).toPrecision(21))", "T(()=>(1e21).toPrecision(22))",
  "T(()=>(123456).toPrecision(2))", "T(()=>(0.000123).toPrecision(2))", "T(()=>(0.0000001).toPrecision(2))", "T(()=>(5e-324).toPrecision(21))", "T(()=>(1.7976931348623157e308).toPrecision(21))",
  "T(()=>(0).toPrecision(1))", "T(()=>(-0).toPrecision(5))", "T(()=>(0).toExponential())", "T(()=>(-0).toExponential(2))", "T(()=>(0).toExponential(100))",
  "T(()=>(123456).toExponential())", "T(()=>(123456).toExponential(0))", "T(()=>(0.00015).toExponential(1))", "T(()=>(5e-324).toExponential())", "T(()=>(5e-324).toExponential(100))",
  "T(()=>(1.7976931348623157e308).toExponential(100))", "T(()=>(25).toExponential(0))", "T(()=>(35).toExponential(0))", "T(()=>(9.5).toExponential(0))", "T(()=>(99.5).toExponential(1))",
  "T(()=>NaN.toExponential(1000))", "T(()=>Infinity.toExponential(-1))", "T(()=>NaN.toPrecision(0))", "T(()=>NaN.toPrecision(101))", "T(()=>NaN.toFixed(101))", "T(()=>Infinity.toFixed(-5))",
  "T(()=>Number.MAX_SAFE_INTEGER.toString(2))", "T(()=>Number.MIN_SAFE_INTEGER.toString(36))", "T(()=>Number.EPSILON.toString(2))", "T(()=>Number.MIN_VALUE.toString(36).length)",
  "T(()=>Number.MAX_VALUE.toString(36).length)", "T(()=>[Number.MAX_SAFE_INTEGER,Number.MIN_SAFE_INTEGER,Number.EPSILON,Number.MIN_VALUE,Number.MAX_VALUE,Number.POSITIVE_INFINITY,Number.NEGATIVE_INFINITY].join())",
  "T(()=>Object.getOwnPropertyDescriptor(Number,'MAX_VALUE').writable)", "T(()=>Object.getOwnPropertyDescriptor(Number,'NaN').configurable)",
  "T(()=>Object.getOwnPropertyDescriptor(Number.prototype,'toFixed').enumerable)", "T(()=>Object.getOwnPropertyDescriptor(Number.prototype,'toFixed').writable)",
  "T(()=>{'use strict';Number.MAX_VALUE=1})", "T(()=>{'use strict';Number.prototype=1})", "T(()=>{'use strict';(1).x=1})", "T(()=>{'use strict';var n=new Number(1);n.x=2;return n.x})",
  "T(()=>(1).toFixed.call(new Number(1),3))", "T(()=>{var f=(1).toFixed;return f()})", "T(()=>Reflect.apply(Number.prototype.toString,3,[2]))", "T(()=>Reflect.construct(Number.prototype.toString,[]))",
  "T(()=>new Number.prototype.toFixed())", "T(()=>Number.prototype.toString.call(Object(1n)))", "T(()=>Number.prototype.valueOf.call(Number.prototype))",
  "T(()=>Number.prototype.toFixed.call(Number.prototype,2))", "T(()=>Number.prototype.toString.call(Number.prototype,2))",
  "T(()=>[1,10,100].map(Number.prototype.toString.call.bind(Number.prototype.toString)).join())", "T(()=>[10,11,12].map(n=>n.toString(2)).join())", "T(()=>[1,2,3].map((n,i)=>n.toFixed(i)).join())",
  "T(()=>['1','2'].map(Number).map(n=>n.toPrecision(3)).join())", "T(()=>[10,20].map(Number.prototype.toFixed,2).join())",
  "T(()=>Number('0x1f').toString(2))", "T(()=>Number('1e3').toExponential())", "T(()=>(+'  12  ').toFixed(1))", "T(()=>(12).toString(2.5))", "T(()=>(12).toString(' 2 '))", "T(()=>(12).toString('0x10'))",
  "T(()=>(12).toString(''))", "T(()=>(12).toString([]))", "T(()=>(12).toString([3]))", "T(()=>(12).toString(true))", "T(()=>(12).toString(false))", "T(()=>(12).toString(undefined))", "T(()=>(12).toString(null))",
);

const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const baseSources = [];
baseSources.push(...knownPrograms("number_proto_bun.tsv", ["number_edge_bun.tsv", "number_format_bun.tsv", "number_format_more_bun.tsv", "number_compact_bun.tsv", "number_parts_bun.tsv", "number_regional_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    // Processo fresco por programa, igual aos demais geradores: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"],{ input: source, encoding: "utf8", maxBuffer: 1 << 26 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("number_proto", rows));
