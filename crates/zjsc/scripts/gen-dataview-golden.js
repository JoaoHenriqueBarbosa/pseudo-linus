// Gera tests/golden/dataview_bun.tsv: grade de DataView e ArrayBuffer, medida no bun 1.4.2.
// Cobre get/set de todos os tipos (Int8 a BigUint64, com Float16) contra offsets (zero, negativo, fracionário, NaN,
// fora de faixa, 2**53) e littleEndian (undefined, true, 1, 'x'), valores extremos (NaN com payload, -0, infinitos,
// estouro de inteiro, BigInt fora de faixa), o construtor com offset e length inválidos (mensagens exatas), DataView
// sobre buffer detached ou redimensionado (length-tracking, getter de byteOffset que lança), os getters em receptores
// inválidos, o construtor de ArrayBuffer com maxByteLength, isView, species de slice, limites de resize e transfer e
// @@toStringTag. Programas cujo fonte da expressão já aparece nos goldens de buffer, typedarray e sab são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-dataview-golden.js > tests/golden/dataview_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function H(b){return Array.from(new Uint8Array(b),x=>x.toString(16).padStart(2,"0")).join("")}\n' +
  "function NP(bits){var u=new BigUint64Array(1);u[0]=bits;return new Float64Array(u.buffer)[0]}\n" +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const tf = body => `T(()=>{${body}})`;

const INT_TYPES = ["Int8", "Uint8", "Int16", "Uint16", "Int32", "Uint32"];
const NUM_TYPES = [...INT_TYPES, "Float16", "Float32", "Float64"];
const BIG_TYPES = ["BigInt64", "BigUint64"];
const ALL_TYPES = [...NUM_TYPES, ...BIG_TYPES];
const isBig = t => t.startsWith("Big");
const FILL = "var b=new ArrayBuffer(16),u=new Uint8Array(b);for(var i=0;i<16;i++)u[i]=(i*37+201)&255;var d=new DataView(b);";

// ---- 1. get: tipo x offset x littleEndian.
const getOffsets = [
  "0", "1", "3", "7", "8", "9", "12", "15", "16", "17", "-1", "-0", "0.5", "1.9", "NaN", "'1'", "'x'", "''", "null", "undefined", "true",
  "2**53", "2**53-1", "2**32", "2**31", "Infinity", "-Infinity", "{valueOf(){return 2}}", "{valueOf(){throw new RangeError('v')}}",
  "[3]", "1n", "Symbol()", "[]", "{}",
];
const littleEndians = ["undefined", "true", "1", "'x'", "0", "''", "null", "NaN", "{}", "Symbol()"];
for (const type of ALL_TYPES) {
  for (const offset of getOffsets) {
    for (const le of littleEndians.slice(0, 4)) add(tf(`${FILL}return d.get${type}(${offset},${le})`));
  }
  for (const le of littleEndians) add(tf(`${FILL}return d.get${type}(1,${le})`));
  add(tf(`${FILL}return d.get${type}()`));
  add(tf(`${FILL}return d.get${type}.length+d.get${type}.name`));
  add(tf(`${FILL}return d.set${type}.length+d.set${type}.name`));
  add(tf(`${FILL}return d.get${type}.call(d,0,true)===d.get${type}.call(d,0,false)`));
}

// ---- 2. set: valores extremos x littleEndian, e offsets x littleEndian.
const numValues = [
  "0", "-0", "1", "-1", "NaN", "NP(0x7ff8000000001234n)", "NP(0xfff8000000000001n)", "NP(0x7ff0000000000001n)", "Infinity", "-Infinity",
  "127.9", "128", "-128.9", "-129", "255", "256", "255.9", "65535", "65536", "65536.5", "32767.5", "32768", "-32769", "2**31", "2**31-1", "-(2**31)-1",
  "2**32", "2**32+1", "2**32-1", "2**53", "2**64", "1e21", "-1e21", "1e308", "0.1", "1/3", "65504", "65519.99", "65520", "65535.9", "6.103515625e-5",
  "5.960464477539063e-8", "2.98e-8", "2.99e-8", "1e-8", "3.4028234663852886e38", "3.4028235677973366e38", "3.4028236e38", "1.401298464324817e-45",
  "7e-46", "7.1e-46", "1.5", "2.5", "-1.5", "'12'", "'x'", "''", "null", "undefined", "true", "false", "[7]", "{valueOf(){return 300}}",
  "{valueOf(){throw new EvalError('val')}}", "1n", "Symbol()", "0x7fffffff", "0xffffffff", "-0xffffffff",
];
const bigValues = [
  "0n", "-0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "-(2n**63n)-1n", "2n**64n", "2n**64n-1n", "2n**64n+1n", "2n**128n+5n", "-(2n**128n)-5n",
  "BigInt.asUintN(64,-1n)", "BigInt.asIntN(64,2n**63n)", "'12'", "'-12'", "'0x10'", "''", "'x'", "'1.5'", "1", "1.5", "NaN", "0", "null", "undefined",
  "true", "false", "[5n]", "{valueOf(){return 9n}}", "{valueOf(){throw new RangeError('b')}}", "Symbol()", "Object(7n)", "'18446744073709551617'",
];
const setLes = ["undefined", "true", "'x'", "1"];
for (const type of ALL_TYPES) {
  const values = isBig(type) ? bigValues : numValues;
  for (const value of values) {
    for (const le of setLes) add(tf(`${FILL}var r=d.set${type}(0,${value},${le});return S(r)+" "+H(b)`));
  }
  // Valor também em offset interno, para ver que o resto do buffer fica intacto.
  const probe = isBig(type) ? "0x0102030405060708n" : "0x01020304";
  for (const offset of getOffsets) {
    for (const le of ["undefined", "true"]) add(tf(`${FILL}var r=d.set${type}(${offset},${probe},${le});return S(r)+" "+H(b)`));
  }
  add(tf(`${FILL}return d.set${type}()`), tf(`${FILL}return d.set${type}(0)`), tf(`${FILL}return d.set${type}(0,${probe},true,9)+H(b)`));
  // Ordem de coerção: offset antes do valor, valor antes do teste de faixa.
  add(tf(`${FILL}var log=[];try{d.set${type}({valueOf(){log.push("o");return 99}},{valueOf(){log.push("v");return ${isBig(type) ? "1n" : "1"}}},{valueOf(){log.push("e")}})}catch(e){log.push(e.name)}return log.join()`));
  add(tf(`${FILL}var log=[];try{d.get${type}({valueOf(){log.push("o");return 99}},{valueOf(){log.push("e")}})}catch(e){log.push(e.name)}return log.join()`));
  add(tf(`${FILL}var log=[];try{d.set${type}(0,{valueOf(){log.push("v");throw new URIError("x")}},{valueOf(){log.push("e")}})}catch(e){log.push(e.name)}return log.join()`));
}

// Roundtrip entre tipos pelo mesmo buffer, com NaN canônico e payload.
for (const bits of ["0x7ff8000000000000n", "0x7ff8000000001234n", "0xfff8000000000000n", "0x7ff0000000000001n", "0x7ff4000000000000n", "0x8000000000000000n", "0x7ff0000000000000n"]) {
  for (const le of ["true", "false"]) {
    add(tf(`var b=new ArrayBuffer(8),d=new DataView(b);d.setBigUint64(0,${bits},${le});var f=d.getFloat64(0,${le});d.setFloat64(0,f,${le});return H(b)+" "+d.getFloat32(0,${le})+" "+d.getFloat16(0,${le})+" "+d.getFloat16(4,${le})`));
    add(tf(`var b=new ArrayBuffer(8),d=new DataView(b);d.setFloat64(0,NP(${bits}),${le});return H(b)+" "+d.getBigUint64(0,${le})`));
    add(tf(`var b=new ArrayBuffer(8),d=new DataView(b);d.setFloat32(0,NP(${bits}),${le});return H(b)`));
    add(tf(`var b=new ArrayBuffer(8),d=new DataView(b);d.setFloat16(0,NP(${bits}),${le});return H(b)`));
  }
}
for (const bits of ["0x7e00", "0x7c01", "0xfe00", "0x7bff", "0x0001", "0x03ff", "0x0400", "0x8000", "0xfc00", "0x7c00", "0x3555", "0x7fff", "0xffff"]) {
  add(tf(`var b=new ArrayBuffer(2),d=new DataView(b);d.setUint16(0,${bits});return S(d.getFloat16(0))+" "+S(d.getFloat16(0,true))`));
  add(tf(`var b=new ArrayBuffer(2),d=new DataView(b);d.setUint16(0,${bits});var f=d.getFloat16(0);d.setFloat16(0,f);return H(b)`));
}
for (const bits of ["0x7fc00000", "0x7fc00001", "0xffc00000", "0x7f800001", "0x00000001", "0x007fffff", "0x00800000", "0x80000000", "0x7f7fffff", "0x7f800000"]) {
  add(tf(`var b=new ArrayBuffer(4),d=new DataView(b);d.setUint32(0,${bits});var f=d.getFloat32(0);d.setFloat32(0,f);return S(f)+" "+H(b)`));
}

// ---- 3. Construtor: offset e length inválidos.
const ctorOffsets = ["undefined", "0", "1", "4", "7", "8", "9", "-1", "-0", "0.5", "1.9", "NaN", "'x'", "'2'", "''", "2**53", "2**53-1", "2**32", "Infinity", "null", "true", "1n", "Symbol()", "{valueOf(){return 3}}", "{valueOf(){throw new RangeError('o')}}"];
const ctorLengths = ["undefined", "0", "1", "4", "8", "9", "-1", "1.5", "NaN", "2**53", "2**53-1", "'2'", "'x'", "null", "true", "1n", "Symbol()", "{valueOf(){return 2}}", "{valueOf(){throw new RangeError('l')}}"];
const buffers = [
  ["fixed", "new ArrayBuffer(8)"],
  ["resizable", "new ArrayBuffer(8,{maxByteLength:16})"],
];
for (const [, make] of buffers) {
  for (const o of ctorOffsets) for (const l of ctorLengths) {
    add(tf(`var b=${make};var d=new DataView(b,${o},${l});var r=[d.byteOffset,d.byteLength];if(b.resizable){b.resize(16);r.push(d.byteLength)}return S(r)`));
  }
  for (const o of ctorOffsets) add(tf(`var b=${make};var d=new DataView(b,${o});var r=[d.byteOffset,d.byteLength];if(b.resizable){b.resize(12);r.push(d.byteLength)}return S(r)`));
}
for (const o of ["0", "4", "8", "9", "-1"]) for (const l of ["undefined", "0", "4", "5"]) {
  add(tf(`var d=new DataView(new SharedArrayBuffer(8),${o},${l});return S([d.byteOffset,d.byteLength])`));
  add(tf(`var d=new DataView(new SharedArrayBuffer(8,{maxByteLength:16}),${o},${l});return S([d.byteOffset,d.byteLength])`));
}
const ctorFirstArgs = [
  "", "undefined", "null", "0", "1", "'s'", "true", "Symbol()", "1n", "{}", "[]", "new Uint8Array(8)", "new Uint8Array(8).buffer", "new DataView(new ArrayBuffer(8))",
  "new Float64Array(2)", "ArrayBuffer", "ArrayBuffer.prototype", "Object.create(ArrayBuffer.prototype)", "new Proxy(new ArrayBuffer(8),{})", "new SharedArrayBuffer(8)",
  "{byteLength:8}", "(()=>{var b=new ArrayBuffer(8);b.transfer();return b})()", "new Number(8)", "new String('abc')", "new Date(0)", "function(){}",
];
for (const a of ctorFirstArgs) {
  add(tf(`return S(new DataView(${a}).byteLength)`), tf(`return S(DataView(${a}).byteLength)`), tf(`return S(new DataView(${a},0).byteLength)`));
  add(tf(`return S(new DataView(${a},0,0).byteLength)`));
}
add(
  "T(()=>DataView(new ArrayBuffer(8)))", "T(()=>DataView.call({},new ArrayBuffer(8)))", "T(()=>Reflect.construct(DataView,[new ArrayBuffer(8)],Object))",
  "T(()=>Reflect.construct(DataView,[new ArrayBuffer(8)],function(){}).byteLength)", "T(()=>Reflect.construct(DataView,[new ArrayBuffer(8)],Array) instanceof Array)",
  "T(()=>Reflect.construct(DataView,[new ArrayBuffer(8)],Object.assign(function(){},{prototype:null})) instanceof DataView)",
  "T(()=>{var nt=function(){};nt.prototype=1;return Object.getPrototypeOf(Reflect.construct(DataView,[new ArrayBuffer(8)],nt))===DataView.prototype})",
  "T(()=>{var nt=new Proxy(function(){},{get(t,k){if(k==='prototype')throw new EvalError('p');return t[k]}});Reflect.construct(DataView,[new ArrayBuffer(8)],nt)})",
  "T(()=>{class V extends DataView{};var v=new V(new ArrayBuffer(8),2);return S([v instanceof V,v.byteOffset,v.byteLength,Object.getPrototypeOf(v)===V.prototype])})",
  "T(()=>{class V extends DataView{constructor(){super(new ArrayBuffer(4))}};return new V().byteLength})",
  "T(()=>{class V extends DataView{constructor(){}};new V()})", "T(()=>{class V extends DataView{};new V()})",
  "T(()=>DataView.length+DataView.name)", "T(()=>D(DataView,'length')+'|'+D(DataView,'name')+'|'+D(DataView,'prototype'))",
  "T(()=>Object.getPrototypeOf(DataView)===Function.prototype)", "T(()=>Reflect.ownKeys(DataView).join())", "T(()=>Reflect.ownKeys(DataView.prototype).map(String).join())",
  "T(()=>Object.getPrototypeOf(DataView.prototype)===Object.prototype)", "T(()=>DataView.prototype.constructor===DataView)",
  "T(()=>String(Object.getPrototypeOf(new DataView(new ArrayBuffer(1)))===DataView.prototype))",
  "T(()=>{var log=[];var nt=new Proxy(function(){},{get(t,k){log.push(String(k));return t[k]}});var b=new ArrayBuffer(8);Reflect.construct(DataView,[b,{valueOf(){log.push('o');return 1}},{valueOf(){log.push('l');return 1}}],nt);return log.join()})",
  "T(()=>{var log=[];var nt=new Proxy(function(){},{get(t,k){log.push(String(k));return t[k]}});try{Reflect.construct(DataView,[new ArrayBuffer(8),9],nt)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var b=new ArrayBuffer(8);var nt=function(){};Object.defineProperty(nt,'prototype',{get(){b.transfer();return DataView.prototype}});return Reflect.construct(DataView,[b,0,4],nt)})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var nt=function(){};Object.defineProperty(nt,'prototype',{get(){b.resize(2);return DataView.prototype}});return Reflect.construct(DataView,[b,4],nt).byteLength})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var nt=function(){};Object.defineProperty(nt,'prototype',{get(){b.resize(2);return DataView.prototype}});return Reflect.construct(DataView,[b,0,4],nt).byteLength})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var nt=function(){};Object.defineProperty(nt,'prototype',{get(){b.resize(12);return DataView.prototype}});var d=Reflect.construct(DataView,[b,4],nt);return d.byteLength})",
);

// ---- 4. DataView sobre buffer detached e redimensionado.
const dvKinds = [
  ["fixed2_4", "new DataView(b,2,4)"],
  ["track2", "new DataView(b,2)"],
  ["track0", "new DataView(b)"],
  ["fixed0_8", "new DataView(b,0,8)"],
  ["trackEnd", "new DataView(b,8)"],
  ["fixed0_0", "new DataView(b,0,0)"],
];
const probes = [
  "d.byteLength", "d.byteOffset", "d.buffer===b", "d.getInt8(0)", "d.getInt8(5)", "d.setInt8(0,9)", "d.getUint16(2)", "d.getFloat64(0)", "d.getInt8(-1)", "d.getBigInt64(0)",
];
const resizes = [0, 1, 2, 3, 5, 6, 8, 10, 12, 16];
for (const [, make] of dvKinds) {
  for (const n of resizes) {
    for (const probe of probes) {
      add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);u.set([1,2,3,4,5,6,7,8]);var d=${make};b.resize(${n});return S(${probe})`));
    }
  }
  // Volta ao tamanho original depois de sair de faixa.
  for (const n of [0, 1, 3, 5]) {
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};b.resize(${n});var r=[];try{r.push(d.byteLength)}catch(e){r.push(e.name)}b.resize(8);r.push(d.byteLength,d.byteOffset);b.resize(16);r.push(d.byteLength);return S(r)`));
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};b.resize(${n});var r=[];try{r.push(d.byteOffset)}catch(e){r.push(e.message)}try{r.push(d.byteLength)}catch(e){r.push(e.message)}try{r.push(d.getInt8(0))}catch(e){r.push(e.message)}try{r.push(d.setInt8(0,1))}catch(e){r.push(e.message)}return S(r)`));
  }
  for (const probe of probes) {
    add(tf(`var b=new ArrayBuffer(8);new Uint8Array(b).set([1,2,3,4,5,6,7,8]);var d=${make};b.transfer();return S(${probe})`));
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};b.transfer();return S(${probe})`));
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};b.transferToFixedLength(4);return S(${probe})`));
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};b.resize(16);return S(${probe})`));
    add(tf(`var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=${make.replace("new DataView(b", "new DataView(b")};b.grow(16);return S(${probe})`));
  }
}
add(
  "T(()=>{var b=new ArrayBuffer(8);b.transfer();return new DataView(b)})", "T(()=>{var b=new ArrayBuffer(8);b.transfer();return new DataView(b,0,0)})",
  "T(()=>{var b=new ArrayBuffer(8);b.transfer();return new DataView(b,9)})", "T(()=>{var b=new ArrayBuffer(8);b.transfer();return new DataView(b,-1)})",
  "T(()=>{var b=new ArrayBuffer(0);b.transfer();return new DataView(b)})", "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:8});b.transfer();return new DataView(b)})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.buffer===b})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return S([b.byteLength,b.detached,b.maxByteLength,b.resizable])})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);var c=b.transfer();return S([new DataView(c).byteLength,d.byteLength])})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.getInt8({valueOf(){throw new EvalError('o')}})})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);return d.setInt8({valueOf(){b.transfer();return 0}},1)})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);return d.setInt8(0,{valueOf(){b.transfer();return 1}})})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);return d.getInt8({valueOf(){b.transfer();return 0}})})",
  "T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);return d.getInt8(0,{valueOf(){b.transfer();return 0}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);return d.setInt32(4,{valueOf(){b.resize(6);return 1}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);return d.getInt32({valueOf(){b.resize(6);return 4}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);return d.setInt32(4,{valueOf(){b.resize(16);return 1}})+' '+d.byteLength})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,0,4);return d.setInt32(0,{valueOf(){b.resize(2);return 1}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,0,4);return d.setInt32(0,{valueOf(){b.resize(4);return 1}})+H(b)})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2);return S([d.byteOffset,d.byteLength,Object.getOwnPropertyNames(d).length])})",
);

// ---- 5. Getters e métodos em receptores inválidos.
const receivers = [
  "undefined", "null", "1", "'s'", "Symbol()", "{}", "[]", "new SharedArrayBuffer(8)", "new ArrayBuffer(8)", "new Uint8Array(8)", "new DataView(new ArrayBuffer(8))",
  "new DataView(new SharedArrayBuffer(8))", "DataView.prototype", "ArrayBuffer.prototype", "Object.create(new DataView(new ArrayBuffer(8)))", "new Proxy(new DataView(new ArrayBuffer(8)),{})",
  "(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d})()", "DataView", "function(){}", "new Boolean(true)",
];
for (const g of ["byteLength", "byteOffset", "buffer"]) {
  for (const r of receivers) add(tf(`return S(Object.getOwnPropertyDescriptor(DataView.prototype,'${g}').get.call(${r}))`));
  add(tf(`return D(DataView.prototype,'${g}')`), tf(`return Object.getOwnPropertyDescriptor(DataView.prototype,'${g}').get.name`), tf(`return Object.getOwnPropertyDescriptor(DataView.prototype,'${g}').get.length`));
  add(tf(`return S(DataView.prototype.${g})`), tf(`'use strict';DataView.prototype.${g}=1`), tf(`'use strict';new DataView(new ArrayBuffer(1)).${g}=1`));
  add(tf(`return S(Object.getOwnPropertyDescriptor(DataView.prototype,'${g}').set)`));
}
for (const type of ALL_TYPES) {
  const probe = isBig(type) ? "1n" : "1";
  for (const r of receivers) {
    add(tf(`return S(DataView.prototype.get${type}.call(${r},0))`), tf(`return S(DataView.prototype.set${type}.call(${r},0,${probe}))`));
  }
  add(tf(`return D(DataView.prototype,'get${type}')`), tf(`return D(DataView.prototype,'set${type}')`));
  add(tf(`return DataView.prototype.get${type}.hasOwnProperty('prototype')`), tf(`return new DataView.prototype.get${type}()`));
  add(tf(`var d=new DataView(new ArrayBuffer(16));return d.get${type}===DataView.prototype.get${type}`));
  add(tf(`var d=new DataView(new ArrayBuffer(16));var f=d.set${type};return f(0,${probe})`));
}
add(
  "T(()=>Reflect.ownKeys(DataView.prototype).length)", "T(()=>Reflect.ownKeys(DataView.prototype).map(k=>String(k)+':'+(D(DataView.prototype,k).slice(0,12))).join('|'))",
  "T(()=>Object.getOwnPropertyNames(DataView.prototype).join())", "T(()=>Object.getOwnPropertySymbols(DataView.prototype).map(String).join())",
);

// ---- 6. @@toStringTag e Object.prototype.toString.
add(
  "T(()=>D(DataView.prototype,Symbol.toStringTag))", "T(()=>D(ArrayBuffer.prototype,Symbol.toStringTag))", "T(()=>D(SharedArrayBuffer.prototype,Symbol.toStringTag))",
  "T(()=>Object.prototype.toString.call(new DataView(new ArrayBuffer(1))))", "T(()=>Object.prototype.toString.call(DataView.prototype))",
  "T(()=>Object.prototype.toString.call(new ArrayBuffer(1)))", "T(()=>Object.prototype.toString.call(ArrayBuffer.prototype))", "T(()=>Object.prototype.toString.call(new SharedArrayBuffer(1)))",
  "T(()=>Object.prototype.toString.call(Object.create(DataView.prototype)))", "T(()=>Object.prototype.toString.call(Object.create(ArrayBuffer.prototype)))",
  "T(()=>{class V extends DataView{};return Object.prototype.toString.call(new V(new ArrayBuffer(1)))})", "T(()=>{class A extends ArrayBuffer{};return Object.prototype.toString.call(new A(1))})",
  "T(()=>{class A extends ArrayBuffer{get [Symbol.toStringTag](){return 'Q'}};return Object.prototype.toString.call(new A(1))})",
  "T(()=>{var d=new DataView(new ArrayBuffer(1));Object.defineProperty(d,Symbol.toStringTag,{value:'Z'});return Object.prototype.toString.call(d)})",
  "T(()=>{var b=new ArrayBuffer(1);Object.defineProperty(b,Symbol.toStringTag,{value:7});return Object.prototype.toString.call(b)})",
  "T(()=>{var b=new ArrayBuffer(1);b[Symbol.toStringTag]='W';return Object.prototype.toString.call(b)})",
  "T(()=>{'use strict';var b=new ArrayBuffer(1);b[Symbol.toStringTag]='W'})", "T(()=>{'use strict';DataView.prototype[Symbol.toStringTag]='W'})",
  "T(()=>String(new DataView(new ArrayBuffer(1))))", "T(()=>String(new ArrayBuffer(1)))", "T(()=>`${DataView.prototype}`)", "T(()=>String(ArrayBuffer.prototype))",
  "T(()=>new ArrayBuffer(1)+'')", "T(()=>JSON.stringify(new ArrayBuffer(4)))", "T(()=>JSON.stringify(new DataView(new ArrayBuffer(4))))", "T(()=>JSON.stringify({a:new ArrayBuffer(2)}))",
  "T(()=>Object.keys(new ArrayBuffer(4)).length+Object.keys(new DataView(new ArrayBuffer(4))).length)", "T(()=>Reflect.ownKeys(new ArrayBuffer(4)).length)",
  "T(()=>Object.prototype.toString.call(Object.getOwnPropertyDescriptor(DataView.prototype,'byteLength').get))",
  "T(()=>Object.prototype.toString.call(new Proxy(new DataView(new ArrayBuffer(1)),{})))", "T(()=>Object.prototype.toString.call(new Proxy(new ArrayBuffer(1),{})))",
  "T(()=>{var o=Object.create(null);o[Symbol.toStringTag]='DataView';return Object.prototype.toString.call(o)})",
  "T(()=>Object.prototype.toString.call(new ArrayBuffer(2).slice(0)))",
);

// ---- 7. ArrayBuffer: construtor, length e options.
const abLengths = ["", "undefined", "0", "1", "8", "-1", "-0", "1.5", "NaN", "'x'", "'3'", "''", "2**53", "2**53-1", "2**32", "2**31", "2**31-1", "Infinity", "null", "true", "{valueOf(){return 4}}", "{valueOf(){throw new RangeError('l')}}", "1n", "Symbol()", "[2]", "{}"];
const abOptions = [
  "", "undefined", "{}", "{maxByteLength:undefined}", "{maxByteLength:0}", "{maxByteLength:1}", "{maxByteLength:4}", "{maxByteLength:8}", "{maxByteLength:16}", "{maxByteLength:-1}", "{maxByteLength:1.5}",
  "{maxByteLength:'8'}", "{maxByteLength:'x'}", "{maxByteLength:2**53}", "{maxByteLength:2**53-1}", "{maxByteLength:NaN}", "{maxByteLength:null}", "{maxByteLength:Infinity}", "{maxByteLength:{valueOf(){return 8}}}",
  "{maxByteLength:1n}", "{maxByteLength:Symbol()}", "{get maxByteLength(){throw new EvalError('m')}}", "null", "1", "'x'", "true", "[]", "function(){}", "new Proxy({maxByteLength:8},{})", "{maxByteLength:8,extra:1}",
];
for (const l of abLengths) {
  for (const o of abOptions) {
    const args = o === "" ? l : l === "" ? "undefined," + o : `${l},${o}`;
    add(tf(`var b=new ArrayBuffer(${args});return S([b.byteLength,b.resizable,b.maxByteLength,b.detached])`));
  }
}
add(
  "T(()=>ArrayBuffer(8))", "T(()=>ArrayBuffer())", "T(()=>ArrayBuffer.length+ArrayBuffer.name)", "T(()=>Reflect.ownKeys(ArrayBuffer).map(String).join())",
  "T(()=>Reflect.ownKeys(ArrayBuffer.prototype).map(String).join())", "T(()=>D(ArrayBuffer,'prototype')+'|'+D(ArrayBuffer,'length')+'|'+D(ArrayBuffer,Symbol.species))",
  "T(()=>S(ArrayBuffer[Symbol.species]===ArrayBuffer))", "T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer,Symbol.species).get.name)",
  "T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer,Symbol.species).get.call(5))", "T(()=>ArrayBuffer.prototype.constructor===ArrayBuffer)",
  "T(()=>Reflect.construct(ArrayBuffer,[8],Object) instanceof ArrayBuffer)", "T(()=>Reflect.construct(ArrayBuffer,[8],Array) instanceof Array)",
  "T(()=>{var nt=function(){};nt.prototype=null;return Object.getPrototypeOf(Reflect.construct(ArrayBuffer,[8],nt))===ArrayBuffer.prototype})",
  "T(()=>{var log=[];var nt=new Proxy(function(){},{get(t,k){log.push(String(k));return t[k]}});try{Reflect.construct(ArrayBuffer,[{valueOf(){log.push('len');return 8}},{get maxByteLength(){log.push('max');return 16}}],nt)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var log=[];try{new ArrayBuffer({valueOf(){log.push('len');return 8}},{get maxByteLength(){log.push('max');return 4}})}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{class A extends ArrayBuffer{};var a=new A(4,{maxByteLength:8});a.resize(6);return S([a.byteLength,a instanceof A,a.maxByteLength])})",
  "T(()=>{class A extends ArrayBuffer{constructor(){super(2)}};return new A().byteLength})", "T(()=>{class A extends ArrayBuffer{constructor(){}};new A()})",
  "T(()=>new ArrayBuffer(2**31).byteLength)", "T(()=>new ArrayBuffer(2**32).byteLength)", "T(()=>new ArrayBuffer(2**40))", "T(()=>new ArrayBuffer(0,{maxByteLength:2**40}).maxByteLength)",
  "T(()=>new ArrayBuffer(0,{maxByteLength:2**53}))", "T(()=>new ArrayBuffer(0,{maxByteLength:2**33}).maxByteLength)",
);

// ---- 8. isView.
const isViewValues = [
  "undefined", "null", "1", "'s'", "Symbol()", "1n", "{}", "[]", "new ArrayBuffer(1)", "new SharedArrayBuffer(1)", "new DataView(new ArrayBuffer(1))", "new DataView(new SharedArrayBuffer(1))",
  "new Uint8Array(1)", "new Float64Array(1)", "new BigInt64Array(1)", "new Uint8ClampedArray(1)", "new Float16Array(1)", "DataView.prototype", "Uint8Array.prototype", "Object.create(DataView.prototype)",
  "Object.create(new Uint8Array(1))", "new Proxy(new Uint8Array(1),{})", "new Proxy(new DataView(new ArrayBuffer(1)),{})", "(()=>{var d=new DataView(new ArrayBuffer(1));return d})()",
  "(()=>{var b=new ArrayBuffer(1);var d=new DataView(b);b.transfer();return d})()", "new (class extends DataView{})(new ArrayBuffer(1))", "new (class extends Uint8Array{})(1)",
  "Object.assign([],{buffer:new ArrayBuffer(1)})", "{buffer:new ArrayBuffer(1),byteLength:1}", "ArrayBuffer", "DataView",
];
for (const v of isViewValues) add(tf(`return S(ArrayBuffer.isView(${v}))`));
add(
  "T(()=>ArrayBuffer.isView())", "T(()=>ArrayBuffer.isView.length+ArrayBuffer.isView.name)", "T(()=>D(ArrayBuffer,'isView'))", "T(()=>ArrayBuffer.isView.call(null,new Uint8Array(1)))",
  "T(()=>ArrayBuffer.isView.call(undefined,new DataView(new ArrayBuffer(1))))", "T(()=>new ArrayBuffer.isView(1))", "T(()=>ArrayBuffer.isView.hasOwnProperty('prototype'))",
);

// ---- 9. slice e species.
const sliceArgs = ["", "0", "1,3", "-2", "2,-1", "9", "0,0", "NaN,NaN", "'1','x'", "undefined,undefined", "-100,100", "1.7,3.2", "{valueOf(){return 2}}", "Infinity", "-Infinity,Infinity", "3,1", "null,null", "1n"];
const sliceBuffers = [["fixed", "new ArrayBuffer(8)"], ["resizable", "new ArrayBuffer(8,{maxByteLength:16})"], ["empty", "new ArrayBuffer(0)"], ["detached", "(()=>{var b=new ArrayBuffer(8);b.transfer();return b})()"]];
for (const [, make] of sliceBuffers) {
  for (const a of sliceArgs) {
    add(tf(`var b=${make};if(!b.detached)new Uint8Array(b).set([1,2,3,4,5,6,7,8].slice(0,b.byteLength));var r=b.slice(${a});return S([r.byteLength,r.resizable,r.maxByteLength,r===b])+" "+H(r)`));
  }
}
const speciesCtors = [
  "undefined", "null", "1", "'s'", "true", "{}", "[]", "function(){}", "()=>1", "{[Symbol.species]:undefined}", "{[Symbol.species]:null}", "{[Symbol.species]:1}", "{[Symbol.species]:function(){}}",
  "{[Symbol.species]:()=>new ArrayBuffer(4)}", "{[Symbol.species]:function(n){return new ArrayBuffer(n)}}", "{[Symbol.species]:function(n){return new ArrayBuffer(n-1)}}", "{[Symbol.species]:function(n){return new ArrayBuffer(n+1)}}",
  "{[Symbol.species]:function(n){return new ArrayBuffer(n,{maxByteLength:n+4})}}", "{[Symbol.species]:function(n){return new SharedArrayBuffer(n)}}", "{[Symbol.species]:function(n){return b}}",
  "{[Symbol.species]:function(n){return {}}}", "{[Symbol.species]:function(n){return new Uint8Array(n)}}", "{[Symbol.species]:function(n){var x=new ArrayBuffer(n);x.transfer();return x}}",
  "{[Symbol.species]:function(n){throw new EvalError('sp')}}", "{get [Symbol.species](){throw new RangeError('g')}}", "{[Symbol.species]:class extends ArrayBuffer{}}",
  "{[Symbol.species]:class extends ArrayBuffer{constructor(n){super(n);this.tag=1}}}", "{[Symbol.species]:Array}", "{[Symbol.species]:Object}", "{[Symbol.species]:ArrayBuffer}", "{[Symbol.species]:SharedArrayBuffer}",
  "{[Symbol.species]:function(n){b.resize(1);return new ArrayBuffer(n)}}", "ArrayBuffer", "Uint8Array", "class extends ArrayBuffer{}",
];
const speciesSlices = ["1,3", "", "0,0", "-2", "5"];
for (const c of speciesCtors) {
  for (const a of speciesSlices) {
    add(tf(`var b=new ArrayBuffer(8,{maxByteLength:16});new Uint8Array(b).set([1,2,3,4,5,6,7,8]);b.constructor=${c};var r=b.slice(${a});return S([r.byteLength,r.constructor===b.constructor,r===b,Object.getPrototypeOf(r)===ArrayBuffer.prototype,r.resizable])+H(r.byteLength?r:new ArrayBuffer(0))`));
  }
  add(tf(`var b=new ArrayBuffer(8);new Uint8Array(b).set([1,2,3,4,5,6,7,8]);b.constructor=${c};var r=b.slice(1,3);return S([r.byteLength,r===b])+(r instanceof ArrayBuffer?H(r):"")`));
}
add(
  "T(()=>{class A extends ArrayBuffer{};var a=new A(8);var r=a.slice(2);return S([r instanceof A,r.byteLength])})",
  "T(()=>{class A extends ArrayBuffer{static get [Symbol.species](){return ArrayBuffer}};var a=new A(8);var r=a.slice(2);return S([r instanceof A,r.byteLength])})",
  "T(()=>{var log=[];class A extends ArrayBuffer{constructor(...a){log.push('ctor '+a.length);super(...a)}};new A(8).slice(1,5);return log.join()})",
  "T(()=>{var b=new ArrayBuffer(8);b.constructor={get [Symbol.species](){b.transfer();return ArrayBuffer}};return b.slice(0)})",
  "T(()=>{var b=new ArrayBuffer(8);return b.slice({valueOf(){b.transfer();return 0}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return b.slice(0,{valueOf(){b.resize(2);return 8}}).byteLength})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return b.slice({valueOf(){b.resize(16);return 0}}).byteLength})",
  "T(()=>ArrayBuffer.prototype.slice.length+ArrayBuffer.prototype.slice.name)", "T(()=>new ArrayBuffer(8).slice.call(new SharedArrayBuffer(8)))",
  "T(()=>ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(8),0))", "T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'slice').value.call(new ArrayBuffer(8),0))",
);

// ---- 10. resize e transfer.
const inits = [["0", "0"], ["0", "8"], ["4", "8"], ["8", "8"], ["8", "16"], ["8", ""], ["16", "16"], ["1", "1"]];
const mk = ([len, max]) => (max === "" ? `new ArrayBuffer(${len})` : `new ArrayBuffer(${len},{maxByteLength:${max}})`);
const resizeArgs = ["0", "1", "4", "8", "9", "16", "17", "-1", "1.5", "NaN", "'2'", "'x'", "undefined", "2**53", "2**32", "null", "true", "Infinity", "{valueOf(){return 3}}", "1n", "Symbol()", ""];
for (const init of inits) {
  for (const a of resizeArgs) {
    add(tf(`var b=${mk(init)};new Uint8Array(b).fill(7);var r=b.resize(${a});return S([r,b.byteLength,b.resizable,b.maxByteLength])+H(b)`));
  }
  for (const method of ["transfer", "transferToFixedLength"]) {
    for (const a of ["", "undefined", "0", "4", "8", "16", "17", "-1", "1.5", "NaN", "'x'", "'3'", "null", "2**53", "{valueOf(){return 2}}", "1n"]) {
      add(tf(`var b=${mk(init)};new Uint8Array(b).fill(7);var r=b.${method}(${a});return S([r.byteLength,r.resizable,r.maxByteLength,r.detached,b.byteLength,b.maxByteLength,b.resizable,b.detached,r===b])+H(r)`));
    }
    add(tf(`var b=${mk(init)};b.${method}();return b.${method}()`), tf(`var b=${mk(init)};b.${method}();return S(b.slice)`));
    add(tf(`var b=${mk(init)};b.${method}();try{b.resize(0)}catch(e){return e.name+": "+e.message}`));
  }
  add(tf(`var b=${mk(init)};var r=b.transfer(0);return S([r.byteLength,r.resizable,r.maxByteLength,b.detached])`));
  add(tf(`var b=${mk(init)};return S([b.detached,b.byteLength,b.resizable,b.maxByteLength])`));
  add(tf(`var b=${mk(init)};b.transfer();return S([b.detached,b.byteLength,b.resizable,b.maxByteLength])`));
  add(tf(`var b=${mk(init)};b.transfer();try{b.slice(0)}catch(e){return e.name+": "+e.message}`));
  add(tf(`var b=${mk(init)};b.transfer();return S(Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get.call(b))`));
}
add(
  "T(()=>ArrayBuffer.prototype.resize.length+ArrayBuffer.prototype.resize.name)", "T(()=>ArrayBuffer.prototype.transfer.length+ArrayBuffer.prototype.transfer.name)",
  "T(()=>ArrayBuffer.prototype.transferToFixedLength.length+ArrayBuffer.prototype.transferToFixedLength.name)",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return b.resize({valueOf(){b.transfer();return 4}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return b.transfer({valueOf(){b.transfer();return 4}})})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return b.transfer({valueOf(){b.resize(2);return 4}}).byteLength})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);var d=new DataView(b);b.transfer(4);return S([u.length,u.byteLength,u.byteOffset,d.buffer===b])})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);b.resize(4);return S([u.length,u.byteLength])})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b,2,4);b.resize(4);return S([u.length,u.byteLength,u.byteOffset])})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b,2,4);b.resize(6);return S([u.length,u.byteLength,u.byteOffset])})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b,2);b.resize(1);return S([u.length,u.byteLength,u.byteOffset])})",
  "T(()=>{var b=new ArrayBuffer(8);b.resize=1;return S(b.resize)})", "T(()=>{var b=new ArrayBuffer(8);return Object.keys(b).length})",
  "T(()=>new ArrayBuffer(8).resize(8))", "T(()=>new ArrayBuffer(8,{maxByteLength:8}).resize(8))",
);

// ---- 11. Getters de ArrayBuffer em receptores inválidos.
for (const g of ["byteLength", "maxByteLength", "resizable", "detached"]) {
  for (const r of receivers) add(tf(`return S(Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'${g}').get.call(${r}))`));
  add(tf(`return D(ArrayBuffer.prototype,'${g}')`), tf(`return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'${g}').get.name`));
  add(tf(`'use strict';new ArrayBuffer(1).${g}=1`), tf(`return S(ArrayBuffer.prototype.${g})`));
}
for (const m of ["slice", "resize", "transfer", "transferToFixedLength"]) {
  for (const r of receivers) add(tf(`return S(ArrayBuffer.prototype.${m}.call(${r},0))`));
  add(tf(`return D(ArrayBuffer.prototype,'${m}')`), tf(`return new ArrayBuffer.prototype.${m}()`));
}
for (const g of ["byteLength", "maxByteLength", "growable", "grow"]) {
  add(tf(`return S(Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'${g}')&&D(SharedArrayBuffer.prototype,'${g}'))`));
}

// ---- Execução.
const baseSources = [];
for (const file of fs.readdirSync(path.join(__dirname, "..", "tests", "golden"))) {
  if (!/^(buffer|buffers|buffer_edge|typedarray|typedarray_edge|typedarray_more|typedarray_proto|sab)_bun\.tsv$/.test(file)) continue;
  for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));

const programs = [];
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  programs.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}

// Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso, então a ordem de chaves
// de ArrayBuffer, DataView e Reflect depende do que rodou antes no mesmo processo.
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", chunk => (out += chunk));
    child.stderr.on("data", chunk => (err += chunk));
    child.on("close", status => {
      const result = decodeResult(out);
      if (status === 0 && result !== null) resolve(result);
      else reject(new Error(err || "filho falhou"));
    });
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(programs.length);
  let next = 0;
  const worker = async () => {
    while (next < programs.length) {
      const index = next++;
      try { results[index] = await runChild(programs[index].source); } catch (e) { results[index] = null; process.stderr.write("erro de programa: " + JSON.stringify(programs[index].expr).slice(0, 160) + " " + e + "\n"); }
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  const seenSources = new Set();
  for (let index = 0; index < programs.length; index++) {
    const result = results[index];
    if (result === null || seenSources.has(programs[index].source)) { dropped++; continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(programs[index].expr).slice(0, 160) + "\n");
      continue;
    }
    seenSources.add(programs[index].source);
    kept++;
    lines.push(JSON.stringify(programs[index].source) + "\t" + JSON.stringify(result));
  }
  process.stdout.write(emitFactoredLines("dataview", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
