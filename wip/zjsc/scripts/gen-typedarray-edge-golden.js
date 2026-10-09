const { emitRow, sampleByHash } = require("./golden-prelude.js");
// Gera tests/golden/typedarray_edge_bun.tsv: borda de TypedArray e DataView medida no bun 1.4.2.
// Métodos (set com overlap, subarray, slice, fill, copyWithin, sort com NaN e -0, toSorted/toReversed/with, findLast,
// includes/indexOf com NaN, join, from/of), conversões entre tipos (Uint8Clamped, Float16Array, BigInt64),
// DataView com endianness e bordas, Math.f16round, erros e mensagens.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-typedarray-edge-golden.js > tests/golden/typedarray_edge_bun.tsv
const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'ArrayBuffer.isView(v)||Array.isArray(v)?"["+Array.from(v,S).join()+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const ints = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array"];
const floats = ["Float32Array", "Float64Array"];
if (typeof Float16Array !== "undefined") floats.unshift("Float16Array");
const bigs = ["BigInt64Array", "BigUint64Array"];
const nums = [...ints, ...floats];

// ---- 1. set com overlap, mesmo buffer e tipos diferentes.
for (const t of nums) {
  for (const off of [0, 1, 2, 3, 4]) {
    add(`T(()=>{var a=new ${t}([1,2,3,4,5,6]);a.set(a.subarray(0,4),${off});return a})`);
    add(`T(()=>{var a=new ${t}([1,2,3,4,5,6]);a.set(a.subarray(2,6),${off});return a})`);
  }
  add(`T(()=>{var a=new ${t}(4);a.set([1,2,3,4,5])})`, `T(()=>{var a=new ${t}(4);a.set([1,2],3)})`, `T(()=>{var a=new ${t}(4);a.set([1],-1)})`,
    `T(()=>{var a=new ${t}(4);a.set([1,2],1.9);return a})`, `T(()=>{var a=new ${t}(4);a.set("12");return a})`, `T(()=>{var a=new ${t}(4);a.set({length:2,0:7,1:8},2);return a})`,
    `T(()=>{var a=new ${t}(4);a.set(null)})`, `T(()=>{var a=new ${t}(4);a.set()})`, `T(()=>{var a=new ${t}(4);a.set([1],Infinity)})`);
  for (const u of nums) add(`T(()=>{var b=new ArrayBuffer(16);var a=new ${t}(b);var c=new ${u}(b,0,2);a.set(c,1);return a})`, `T(()=>{var a=new ${t}(4);a.set(new ${u}([300,-1,1.5,NaN]));return a})`);
  for (const u of bigs) add(`T(()=>{var a=new ${t}(2);a.set(new ${u}(2))})`);
}
for (const t of bigs) {
  add(`T(()=>{var a=new ${t}(4);a.set([1n,2n]);return a})`, `T(()=>{var a=new ${t}(4);a.set([1,2])})`, `T(()=>{var a=new ${t}(2);a.set(new Int8Array(2))})`,
    `T(()=>{var a=new ${t}(2);a.set(new ${t}([5n,6n]),0);return a})`, `T(()=>{var a=new ${t}([1n,2n,3n,4n]);a.set(a.subarray(0,3),1);return a})`);
}

// ---- 2. subarray, slice e fill com índices de borda.
const idx = ["undefined", "0", "1", "-1", "-100", "100", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'2'", "null", "-0", "({valueOf(){return 2}})"];
for (const t of ["Int8Array", "Uint16Array", "Float64Array", "BigInt64Array"]) {
  const lit = t.startsWith("Big") ? "[1n,2n,3n,4n,5n]" : "[1,2,3,4,5]";
  for (const s of idx) {
    for (const e of ["undefined", "2", "-1", "100", "NaN", "-100"]) {
      add(`T(()=>new ${t}(${lit}).subarray(${s},${e}))`, `T(()=>new ${t}(${lit}).slice(${s},${e}))`);
    }
    const v = t.startsWith("Big") ? "9n" : "9";
    add(`T(()=>new ${t}(${lit}).fill(${v},${s}))`, `T(()=>new ${t}(${lit}).fill(${v},${s},-1))`);
  }
}
add(`T(()=>{var a=new Uint8Array([1,2,3,4]);var s=a.subarray(1,3);s[0]=9;return [a,s,s.byteOffset,s.length,s.buffer===a.buffer]})`,
  `T(()=>{var a=new Uint8Array([1,2,3,4]);var s=a.slice(1,3);s[0]=9;return [a,s,s.byteOffset,s.buffer===a.buffer]})`,
  `T(()=>new Uint8Array(4).fill(300))`, `T(()=>new Uint8ClampedArray(4).fill(300))`, `T(()=>new Uint8ClampedArray(4).fill(-5))`, `T(()=>new Uint8ClampedArray(4).fill(1.5))`,
  `T(()=>new Uint8ClampedArray(4).fill(2.5))`, `T(()=>new Uint8ClampedArray(4).fill(0.5))`, `T(()=>new Uint8ClampedArray(4).fill(NaN))`, `T(()=>new Uint8ClampedArray(4).fill(254.5))`,
  `T(()=>new Int8Array(4).fill(128))`, `T(()=>new Int8Array(4).fill("x"))`, `T(()=>new BigInt64Array(2).fill(1))`, `T(()=>new Int8Array(2).fill(1n))`,
  `T(()=>new Uint8Array(0).subarray(0,0).length)`, `T(()=>new Uint8Array(new ArrayBuffer(8),4).subarray(1).byteOffset)`, `T(()=>new Uint16Array(new ArrayBuffer(8),2).subarray(1).byteOffset)`,
  `T(()=>new Float64Array(4).subarray(1,3).byteLength)`);

// ---- 3. copyWithin.
for (const [to, from, end] of [[0, 3, "undefined"], [1, 0, "undefined"], [0, 1, "undefined"], [2, 0, 3], [-2, 0, "undefined"], [0, -2, "undefined"], [0, 1, -1], [3, 0, 100],
  [1, 3, 2], [0, 0, 0], ["NaN", 1, "undefined"], [1, "NaN", "undefined"], [100, 0, "undefined"], [0, 100, "undefined"], [-100, 2, "undefined"], [1, 2, "Infinity"], [0, "-Infinity", 2]]) {
  for (const t of ["Int8Array", "Uint32Array", "Float32Array", "BigUint64Array"]) {
    const lit = t.startsWith("Big") ? "[1n,2n,3n,4n,5n]" : "[1,2,3,4,5]";
    add(`T(()=>new ${t}(${lit}).copyWithin(${to},${from},${end}))`);
  }
}
add(`T(()=>new Uint8Array([1,2,3]).copyWithin())`, `T(()=>{var a=new Uint8Array([1,2,3]);return a.copyWithin(0,1)===a})`);

// ---- 4. sort com NaN, -0 e comparador.
const floatLits = ["[3,NaN,1,-0,0,-Infinity,Infinity,NaN,2]", "[0,-0,0,-0]", "[-0,0,-0,0]", "[NaN,NaN,1]", "[1.5,-1.5,0.1,0.01]", "[5,4,3,2,1]", "[]", "[7]"];
for (const t of floats) for (const l of floatLits) {
  add(`T(()=>{var a=new ${t}(${l});a.sort();return a})`, `T(()=>new ${t}(${l}).toSorted())`, `T(()=>new ${t}(${l}).sort((a,b)=>b-a))`,
    `T(()=>new ${t}(${l}).toSorted((a,b)=>a<b?1:a>b?-1:0))`, `T(()=>new ${t}(${l}).toReversed())`, `T(()=>new ${t}(${l}).reverse())`);
}
for (const t of [...ints, ...bigs]) {
  const l = t.startsWith("Big") ? "[3n,-1n,2n,0n,10n,-5n]" : "[3,-1,2,0,10,-5,255,128]";
  add(`T(()=>new ${t}(${l}).sort())`, `T(()=>new ${t}(${l}).toSorted((a,b)=>a>b?-1:1))`, `T(()=>new ${t}(${l}).sort(undefined))`, `T(()=>new ${t}(${l}).sort(null))`,
    `T(()=>new ${t}(${l}).sort(1))`, `T(()=>new ${t}(${l}).toSorted({}))`, `T(()=>new ${t}(${l}).sort(()=>0))`, `T(()=>new ${t}(${l}).sort(()=>{throw new RangeError("c")}))`);
}
add(`T(()=>new Float64Array([1,2,3]).sort(()=>NaN))`, `T(()=>new Float64Array([3,2,1]).sort((a,b)=>a-b===0?0:undefined))`,
  `T(()=>{var a=new Float64Array([3,1,2]);var seen=[];a.sort((x,y)=>{seen.push(x+":"+y);return x-y});return seen.length>0&&a})`,
  `T(()=>{var a=new Float64Array([3,1,2]);a.sort((x,y)=>{a[0]=99;return x-y});return a.length})`,
  `T(()=>new Float64Array([Object.is(-0,-0)?-0:0,0]).sort().map(x=>1/x))`, `T(()=>Array.from(new Float64Array([0,-0]).sort(),x=>1/x))`,
  `T(()=>new Uint8Array([10,9,1]).sort().join())`, `T(()=>[10,9,1].sort().join())`);

// ---- 5. toSorted, toReversed, with, at, findLast e companhia.
for (const t of ["Int8Array", "Uint8ClampedArray", "Float32Array", "BigInt64Array"]) {
  const big = t.startsWith("Big");
  const lit = big ? "[1n,2n,3n]" : "[1,2,3]";
  const nine = big ? "9n" : "9";
  for (const i of [0, 1, 2, 3, -1, -3, -4, "NaN", "1.9", "-0", "Infinity", "'1'", "undefined"]) add(`T(()=>new ${t}(${lit}).with(${i},${nine}))`, `T(()=>new ${t}(${lit}).at(${i}))`);
  add(`T(()=>new ${t}(${lit}).with(0,{valueOf(){return ${nine}}}))`, `T(()=>{var a=new ${t}(${lit});var b=a.with(0,${nine});return [a,b,a===b,a.buffer===b.buffer]})`,
    `T(()=>new ${t}(${lit}).findLast(x=>x<3${big ? "n" : ""}))`, `T(()=>new ${t}(${lit}).findLastIndex(x=>x<3${big ? "n" : ""}))`, `T(()=>new ${t}(${lit}).findLast(x=>false))`,
    `T(()=>new ${t}(${lit}).findLastIndex(x=>false))`, `T(()=>new ${t}(${lit}).find(x=>x>1${big ? "n" : ""}))`, `T(()=>new ${t}(${lit}).findIndex(x=>x>1${big ? "n" : ""}))`,
    `T(()=>new ${t}(${lit}).findLast(1))`, `T(()=>new ${t}(${lit}).toReversed().constructor===${t})`, `T(()=>new ${t}(${lit}).toSorted().constructor===${t})`,
    `T(()=>${t}.prototype.with.call([1],0,1))`, `T(()=>${t}.prototype.toReversed.call({}))`, `T(()=>${t}.prototype.at.call(new Uint8Array(2),1))`);
}
add(`T(()=>new Int8Array(2).with(0,1n))`, `T(()=>new BigInt64Array(2).with(0,1))`, `T(()=>new Int8Array(2).with(2,1))`, `T(()=>new Int8Array(2).with(-3,1))`,
  `T(()=>new Uint8Array(3).with(5,{valueOf(){throw new TypeError("v")}}))`, `T(()=>new Uint8Array(3).with(1,{valueOf(){throw new TypeError("v")}}))`);

// ---- 6. includes, indexOf, lastIndexOf, join.
const searchLits = ["[1,NaN,3,-0,0]", "[NaN]", "[]"];
for (const t of floats) for (const l of searchLits) for (const needle of ["NaN", "0", "-0", "3", "'3'", "undefined", "1", "Infinity"]) {
  add(`T(()=>{var a=new ${t}(${l});return [a.includes(${needle}),a.indexOf(${needle}),a.lastIndexOf(${needle})]})`);
}
for (const t of ["Int8Array", "Float64Array"]) for (const from of ["undefined", "0", "1", "-1", "-100", "100", "NaN", "Infinity", "-Infinity", "1.5"]) {
  add(`T(()=>{var a=new ${t}([1,2,3,2,1]);return [a.includes(2,${from}),a.indexOf(2,${from}),a.lastIndexOf(2,${from})]})`);
}
add(`T(()=>new BigInt64Array([1n,2n]).includes(1))`, `T(()=>new BigInt64Array([1n,2n]).includes(1n))`, `T(()=>new BigInt64Array([1n,2n]).indexOf(2n))`, `T(()=>new BigInt64Array([1n,2n]).indexOf(2))`,
  `T(()=>new Int8Array([1,2]).includes(1n))`, `T(()=>new Int8Array(3).includes(undefined))`, `T(()=>new Int8Array(3).includes(0,3))`, `T(()=>new Int8Array(3).indexOf(0,-0))`,
  `T(()=>new Uint8Array([1,2,3]).includes(2,{valueOf(){return 2}}))`);
for (const t of [...nums.slice(0, 3), "Float32Array", "BigInt64Array"]) {
  const l = t.startsWith("Big") ? "[1n,-2n,3n]" : "[1,-2,3]";
  for (const sep of ["undefined", "''", "'-'", "', '", "null", "1", "{toString(){return '|'}}", "Symbol()"]) add(`T(()=>new ${t}(${l}).join(${sep}))`);
  add(`T(()=>new ${t}(${l}).toString())`, `T(()=>new ${t}(${l}).toLocaleString())`, `T(()=>String(new ${t}(0)))`, `T(()=>new ${t}(1).join())`);
}
add(`T(()=>new Float64Array([-0,NaN,Infinity,1e21,1e-7,0.1]).join())`, `T(()=>new Float32Array([0.1,16777217,1e10,-0]).join())`, `T(()=>Array.prototype.join.call(new Float64Array([-0,1])))`,
  `T(()=>new Float64Array([1234.5,-0]).toLocaleString("en-US"))`, `T(()=>Object.prototype.toString.call(new Uint8Array(1)))`, `T(()=>new Uint8Array(1)[Symbol.toStringTag])`,
  `T(()=>Object.getPrototypeOf(Uint8Array.prototype)[Symbol.toStringTag])`, `T(()=>Object.getPrototypeOf(Uint8Array)[Symbol.toStringTag])`);

// ---- 7. from, of e construtores.
for (const t of [...ints, ...floats, ...bigs]) {
  const big = t.startsWith("Big");
  const one = big ? "1n" : "1";
  add(`T(()=>${t}.of(${one},${one}))`, `T(()=>${t}.of())`, `T(()=>${t}.from([${one},${one}]))`, `T(()=>${t}.from(new Set([${one}])))`, `T(()=>${t}.from({length:2,0:${one},1:${one}}))`,
    `T(()=>${t}.from([${one},${one}],x=>x+x))`, `T(()=>${t}.from([${one}],function(x,i){return this.k+i+x},{k:${big ? "5n" : "5"}}))`, `T(()=>${t}.from("12"))`, `T(()=>${t}.from(5))`,
    `T(()=>${t}.from(null))`, `T(()=>${t}.from([${one}],1))`, `T(()=>${t}.from({length:-1}))`, `T(()=>${t}.from(function*(){yield ${one};yield ${one}}()))`,
    `T(()=>${t}.BYTES_PER_ELEMENT)`, `T(()=>${t}.prototype.BYTES_PER_ELEMENT)`, `T(()=>${t}.name+${t}.length)`, `T(()=>${t}())`, `T(()=>new ${t}(-1))`, `T(()=>new ${t}(1.5).length)`,
    `T(()=>new ${t}(NaN).length)`, `T(()=>new ${t}("3").length)`, `T(()=>new ${t}(null).length)`, `T(()=>new ${t}(undefined).length)`, `T(()=>new ${t}(Infinity))`,
    `T(()=>new ${t}(new ArrayBuffer(7)))`, `T(()=>new ${t}(new ArrayBuffer(8),1))`, `T(()=>new ${t}(new ArrayBuffer(8),0,100))`, `T(()=>new ${t}(new ArrayBuffer(8),16))`,
    `T(()=>new ${t}(new ArrayBuffer(8),-1))`, `T(()=>new ${t}(new ArrayBuffer(8),0,-1))`, `T(()=>new ${t}(new ArrayBuffer(0)).length)`, `T(()=>new ${t}(new ArrayBuffer(16),8,undefined).length)`,
    `T(()=>new ${t}({length:2,0:${one}}))`, `T(()=>new ${t}([${one},,${one}]))`, `T(()=>new ${t}(Symbol()))`, `T(()=>Reflect.construct(${t},[2],Object).constructor===Object)`,
    `T(()=>new ${t}(new ${big ? "Int8Array" : "BigInt64Array"}(2)))`, `T(()=>Object.keys(new ${t}(2)).join())`, `T(()=>${t}.from.call(Array,[${one}]))`, `T(()=>${t}.of.call(Object,${one}))`);
}

// ---- 8. conversões entre tipos.
const probes = ["0", "-0", "1.5", "-1.5", "2.5", "255", "256", "-1", "-129", "127.9", "65535", "65536", "2**31", "2**32", "2**32+1", "-(2**31)-1", "NaN", "Infinity", "-Infinity",
  "1e21", "1e-10", "0.5", "254.5", "255.5", "3.4028235e38", "3.5e38", "65504", "65520", "65519.99", "6e-8", "2**-24", "2**-25", "(2**-25)*1.0001", "16777217", "9007199254740993", "-1e-50"];
for (const t of nums) add(`T(()=>{var a=new ${t}(${probes.length});${probes.map((p, i) => `a[${i}]=${p};`).join("")}return a})`);
for (const p of probes) add(`T(()=>Array.from([new Int8Array([${p}]),new Uint8Array([${p}]),new Uint8ClampedArray([${p}]),new Int16Array([${p}]),new Uint16Array([${p}]),new Int32Array([${p}]),new Uint32Array([${p}]),new Float32Array([${p}]),new Float64Array([${p}])],x=>S(x[0])).join())`);
const bigProbes = ["0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "-(2n**63n)-1n", "2n**64n", "2n**64n-1n", "2n**64n+5n", "2n**100n", "-(2n**100n)+7n", "255n", "256n", "-129n", "2n**53n+1n"];
for (const t of [...bigs, "Int8Array"]) for (const p of bigProbes) {
  if (t === "Int8Array") add(`T(()=>new Int8Array([${p}]))`);
  else add(`T(()=>new ${t}([${p}]))`, `T(()=>{var a=new ${t}(1);a[0]=${p};return a[0]})`);
}
add(`T(()=>new BigInt64Array([2n**63n])[0])`, `T(()=>BigInt.asIntN(64,2n**63n))`, `T(()=>new BigUint64Array([-1n])[0])`, `T(()=>new BigInt64Array(new BigUint64Array([2n**64n-1n]))[0])`,
  `T(()=>new BigInt64Array(1)[0]=1)`, `T(()=>{var a=new BigInt64Array(1);a[0]="5";return a[0]})`, `T(()=>{var a=new BigInt64Array(1);a[0]="x";return a[0]})`, `T(()=>{var a=new BigInt64Array(1);a[0]=true;return a[0]})`,
  `T(()=>{var a=new BigInt64Array(1);a[0]=Symbol();return a[0]})`, `T(()=>{var a=new Int8Array(1);a[0]=1n;return a[0]})`, `T(()=>{var a=new Int8Array(1);a[0]="7";return a[0]})`,
  `T(()=>{var a=new Int8Array(1);a[0]={valueOf(){return 3}};return a[0]})`, `T(()=>{var a=new Int8Array(1);a[0]=Symbol();return a[0]})`, `T(()=>{var a=new Int8Array(1);a[5]=1;return [a[5],5 in a,a.length]})`,
  `T(()=>{var a=new Int8Array(1);a[-1]=1;return [a[-1],"-1" in a,Object.keys(a).join()]})`, `T(()=>{var a=new Int8Array(1);a["1.5"]=1;return ["1.5" in a,a["1.5"]]})`,
  `T(()=>{var a=new Int8Array(1);a.foo=1;return [a.foo,Object.keys(a).join()]})`, `T(()=>{var a=new Int8Array(2);return [Object.getOwnPropertyDescriptor(a,0).writable,Object.getOwnPropertyDescriptor(a,0).configurable,Object.getOwnPropertyDescriptor(a,0).enumerable]})`,
  `T(()=>Object.defineProperty(new Int8Array(2),0,{value:1,configurable:false}))`, `T(()=>Object.defineProperty(new Int8Array(2),0,{value:7,writable:true,enumerable:true,configurable:true})[0])`,
  `T(()=>Object.defineProperty(new Int8Array(2),5,{value:1}))`, `T(()=>Object.defineProperty(new Int8Array(2),0,{get(){return 1}}))`, `T(()=>delete new Int8Array(2)[0])`,
  `T(()=>{"use strict";return delete new Int8Array(2)[0]})`, `T(()=>delete new Int8Array(2)[5])`, `T(()=>Object.freeze(new Int8Array(2)))`, `T(()=>Object.freeze(new Int8Array(0)).length)`,
  `T(()=>Object.isFrozen(new Int8Array(0)))`, `T(()=>Object.seal(new Int8Array(2)).length)`, `T(()=>Object.isSealed(new Int8Array(2)))`, `T(()=>Object.preventExtensions(new Int8Array(2)).length)`);
// Float16Array
if (typeof Float16Array !== "undefined") {
  add(`T(()=>Float16Array.BYTES_PER_ELEMENT)`, `T(()=>new Float16Array([1.337,65504,65520,1e-8,5.96e-8,-0,NaN]))`, `T(()=>new Float16Array(new Float32Array([0.1,1/3,2049,2051])))`,
    `T(()=>new Float32Array(new Float16Array([0.1,1/3])))`, `T(()=>new Float16Array(new Uint8Array([1,2,255,256])))`, `T(()=>new Uint8Array(new Float16Array([1.9,-1,300.5])))`,
    `T(()=>new Float16Array([1,2]).buffer.byteLength)`, `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setFloat16(0,1.5);return [d.getFloat16(0),d.getUint16(0),d.getUint8(0)]})`,
    `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setFloat16(0,1.5,true);return [d.getFloat16(0,true),d.getUint16(0),d.getUint16(0,true)]})`, `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setFloat16(0,65520);return d.getFloat16(0)})`,
    `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setFloat16(0,-0);return 1/d.getFloat16(0)})`, `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setFloat16(0,NaN);return [d.getUint16(0),d.getFloat16(0)]})`,
    `T(()=>{var d=new DataView(new ArrayBuffer(1));return d.getFloat16(0)})`, `T(()=>{var d=new DataView(new ArrayBuffer(1));d.setFloat16(0,1)})`, `T(()=>new Float16Array([3,1,NaN,-0,0]).sort())`,
    `T(()=>new Float16Array([1,2,3]).toReversed())`, `T(()=>Float16Array.from([1.1,2.2]))`, `T(()=>Float16Array.of(0.1,0.2))`, `T(()=>new Float16Array([1,2]).includes(1.0009765625))`,
    `T(()=>new Float16Array([0.1]).includes(0.1))`, `T(()=>new Float16Array([0.1])[0]===0.1)`, `T(()=>new Float16Array([0.1])[0]===Math.f16round(0.1))`, `T(()=>Float16Array.name+Float16Array.length)`,
    `T(()=>Object.prototype.toString.call(new Float16Array(1)))`, `T(()=>new Float16Array(new BigInt64Array(1)))`, `T(()=>new Float16Array(3).fill(0.3))`, `T(()=>new Float16Array([1,2,3]).with(1,0.7))`);
}

// ---- 9. Math.f16round e Math.fround.
const roundProbes = ["0", "-0", "1", "1.337", "0.1", "-0.1", "1/3", "65504", "65519.99", "65520", "65535", "1e5", "-1e5", "Infinity", "-Infinity", "NaN", "5.960464477539063e-8", "2.9802322387695312e-8",
  "2.980232238769532e-8", "3e-8", "1e-8", "6.097555160522461e-5", "6.103515625e-5", "6.1e-5", "2049", "2050", "2051", "2052", "1.0009765625", "1.00048828125", "1.000488281250001", "1.0004882812499999",
  "4095.5", "4096.5", "8191", "'3.7'", "null", "undefined", "true", "[]", "[5]", "({valueOf(){return 1.1}})", "1n", "Symbol()", "16777217", "3.4028235677973366e38", "3.4028234663852886e38", "1e-46", "1.401298464324817e-45", "7e-46"];
for (const p of roundProbes) add(`T(()=>Math.f16round(${p}))`, `T(()=>Math.fround(${p}))`, `T(()=>1/Math.f16round(${p}))`);
add(`T(()=>Math.f16round.length)`, `T(()=>Math.f16round.name)`, `T(()=>Math.f16round())`, `T(()=>Object.getOwnPropertyDescriptor(Math,"f16round").enumerable)`,
  `T(()=>Object.getOwnPropertyDescriptor(Math,"f16round").writable)`, `T(()=>new Math.f16round(1))`, `T(()=>Math.f16round(1,2))`);

// ---- 10. DataView com endianness e bordas.
const dvTypes = ["Int8", "Uint8", "Int16", "Uint16", "Int32", "Uint32", "Float32", "Float64", "BigInt64", "BigUint64"];
const sizes = { Int8: 1, Uint8: 1, Int16: 2, Uint16: 2, Int32: 4, Uint32: 4, Float32: 4, Float64: 8, BigInt64: 8, BigUint64: 8 };
const dvVals = {
  Int8: ["-1", "127", "128", "-129", "1.9", "NaN"], Uint8: ["-1", "255", "256", "0.5", "NaN"], Int16: ["-2", "32767", "32768", "-32769", "0x1234"], Uint16: ["-1", "65535", "65536", "0xABCD"],
  Int32: ["-2", "2**31", "2**31-1", "-(2**31)-1", "0x12345678"], Uint32: ["-1", "2**32", "2**32-1", "0xDEADBEEF"],
  Float32: ["1.5", "-0", "NaN", "Infinity", "1e40", "0.1", "1e-50", "16777217"], Float64: ["1.5", "-0", "NaN", "-Infinity", "0.1", "5e-324", "1.7976931348623157e308", "1e-400"],
  BigInt64: ["-1n", "2n**63n-1n", "2n**63n", "0x0102030405060708n", "-(2n**63n)"], BigUint64: ["-1n", "2n**64n-1n", "2n**64n", "0x0102030405060708n"],
};
for (const t of dvTypes) {
  const n = sizes[t];
  for (const v of dvVals[t]) for (const le of ["undefined", "false", "true", "1", "0", "null", "''", "'x'"]) {
    add(`T(()=>{var d=new DataView(new ArrayBuffer(16));d.set${t}(3,${v},${le});return [d.get${t}(3,${le}),d.get${t}(3,${le === "true" ? "false" : "true"}),Array.from(new Uint8Array(d.buffer,3,${n}))]})`);
  }
  for (const off of ["-1", "0", `${16 - n}`, `${17 - n}`, "16", "100", "NaN", "1.9", "'2'", "undefined", "null", "Infinity", "-0", "2**53", "({valueOf(){return 1}})"]) {
    add(`T(()=>new DataView(new ArrayBuffer(16)).get${t}(${off}))`, `T(()=>new DataView(new ArrayBuffer(16)).set${t}(${off},${t.startsWith("Big") ? "1n" : "1"}))`);
  }
  add(`T(()=>new DataView(new ArrayBuffer(16),4,${n}).get${t}(0))`, `T(()=>new DataView(new ArrayBuffer(16),4,${n}).get${t}(1))`, `T(()=>new DataView(new ArrayBuffer(16),4).get${t}(${12 - n}))`,
    `T(()=>new DataView(new ArrayBuffer(16),4).get${t}(${13 - n}))`, `T(()=>new DataView(new ArrayBuffer(16)).get${t}())`, `T(()=>new DataView(new ArrayBuffer(16)).set${t}(0))`,
    `T(()=>new DataView(new ArrayBuffer(16)).set${t}())`, `T(()=>DataView.prototype.get${t}.call(new Uint8Array(16),0))`, `T(()=>DataView.prototype.set${t}.call({},0,1))`,
    `T(()=>DataView.prototype.get${t}.length+DataView.prototype.set${t}.length)`, `T(()=>DataView.prototype.get${t}.name)`,
    `T(()=>new DataView(new ArrayBuffer(16)).set${t}(0,${t.startsWith("Big") ? "1" : "1n"}))`, `T(()=>new DataView(new ArrayBuffer(16)).set${t}(0,Symbol()))`,
    `T(()=>new DataView(new ArrayBuffer(16)).set${t}(0,${t.startsWith("Big") ? "1n" : "1"}))`);
}
add(`T(()=>new DataView(new ArrayBuffer(8)))`, `T(()=>new DataView(new ArrayBuffer(8),9))`, `T(()=>new DataView(new ArrayBuffer(8),8).byteLength)`, `T(()=>new DataView(new ArrayBuffer(8),0,9))`,
  `T(()=>new DataView(new ArrayBuffer(8),-1))`, `T(()=>new DataView(new ArrayBuffer(8),0,-1))`, `T(()=>new DataView(new ArrayBuffer(8),2,undefined).byteLength)`, `T(()=>new DataView(new ArrayBuffer(8),2,null).byteLength)`,
  `T(()=>new DataView(new ArrayBuffer(8),"2",NaN).byteLength)`, `T(()=>new DataView(new ArrayBuffer(8),1.9,2.9).byteLength)`, `T(()=>new DataView({}))`, `T(()=>new DataView(8))`, `T(()=>new DataView())`,
  `T(()=>DataView(new ArrayBuffer(8)))`, `T(()=>new DataView(new Uint8Array(8)))`, `T(()=>new DataView(new Uint8Array(8).buffer,3).byteOffset)`, `T(()=>new DataView(new ArrayBuffer(8)).byteOffset)`,
  `T(()=>DataView.prototype.byteLength)`, `T(()=>Object.getOwnPropertyDescriptor(DataView.prototype,"byteLength").get.call({}))`, `T(()=>DataView.prototype[Symbol.toStringTag])`,
  `T(()=>Object.prototype.toString.call(new DataView(new ArrayBuffer(1))))`, `T(()=>DataView.length+DataView.name)`, `T(()=>new DataView(new ArrayBuffer(8)).buffer.byteLength)`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setUint32(0,0x01020304);return [d.getUint8(0),d.getUint8(1),d.getUint8(2),d.getUint8(3)]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setUint32(0,0x01020304,true);return [d.getUint8(0),d.getUint8(1),d.getUint8(2),d.getUint8(3)]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setFloat64(0,1);return [d.getUint32(0).toString(16),d.getUint32(4)]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setFloat64(0,NaN);return [d.getUint32(0).toString(16),d.getUint32(4)]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setFloat32(0,-0);return d.getUint32(0).toString(16)})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setBigUint64(0,0x0102030405060708n);return [d.getUint8(0),d.getUint8(7),d.getBigUint64(0,true)]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setInt16(1,-2,true);return [d.getUint8(1),d.getUint8(2),d.getInt16(1,true),d.getUint16(1)]})`,
  `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);new Uint8Array(b).set([1,2,3,4,5,6,7,8]);return [d.getUint16(1),d.getUint16(1,true),d.getUint32(2),d.getUint32(2,true),d.getFloat32(0).toString(),d.getBigInt64(0).toString()]})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(4));return d.setUint8(0,1)})`, `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setUint8(0,{valueOf(){return 7}});return d.getUint8(0)})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setUint8({valueOf(){throw new EvalError("o")}},{valueOf(){throw new TypeError("v")}})})`,
  `T(()=>{var d=new DataView(new ArrayBuffer(4));d.setUint8(99,{valueOf(){throw new TypeError("v")}})})`);

// ---- 11. Erros e mensagens, ArrayBuffer e protótipo compartilhado.
const TA = Object.getPrototypeOf(Uint8Array);
add(`T(()=>{var T=Object.getPrototypeOf(Uint8Array);return T()})`, `T(()=>{var T=Object.getPrototypeOf(Uint8Array);return new T()})`, `T(()=>{var T=Object.getPrototypeOf(Uint8Array);return T.name+T.length})`,
  `T(()=>Object.getPrototypeOf(Uint8Array.prototype).constructor.prototype===Object.getPrototypeOf(Int8Array.prototype)))`.replace(")))", "))"),
  `T(()=>Object.getPrototypeOf(Uint8Array.prototype)===Object.getPrototypeOf(Float64Array.prototype))`,
  `T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),"length").get.call([]))`,
  `T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),"byteLength").get.call(new DataView(new ArrayBuffer(1))))`,
  `T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get.call(1))`,
  `T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get.call(new Int16Array(1)))`,
  `T(()=>Uint8Array.prototype.length)`, `T(()=>Uint8Array.prototype.fill.call([],1))`, `T(()=>Uint8Array.prototype.map.call("ab",x=>x))`, `T(()=>Uint8Array.prototype.set.call(new Uint8Array(1),[1]))`,
  `T(()=>Uint8Array.prototype.subarray.call(new Int8Array(2),1))`, `T(()=>Uint8Array.prototype.slice.call(new Int8Array([1,2]),1))`,
  `T(()=>Object.getPrototypeOf(Uint8Array.prototype).values===Object.getPrototypeOf(Uint8Array.prototype)[Symbol.iterator])`,
  `T(()=>Array.from(new Uint8Array([1,2,3]).entries(),e=>e.join(":")).join())`, `T(()=>Array.from(new Uint8Array([4,5]).keys()).join())`, `T(()=>[...new Int8Array([7,8])].join())`,
  `T(()=>Object.prototype.toString.call(new Uint8Array(1).values()))`, `T(()=>new Uint8Array(3).map((x,i)=>i*100).join())`, `T(()=>new Uint8ClampedArray(3).map((x,i)=>i*200-50).join())`,
  `T(()=>new Uint8Array(3).filter(x=>true).constructor===Uint8Array)`, `T(()=>new Uint8Array([1,2,3]).filter(x=>x>1))`, `T(()=>new Uint8Array([1,2,3]).reduce((a,b)=>a+b))`, `T(()=>new Uint8Array(0).reduce((a,b)=>a+b))`,
  `T(()=>new Uint8Array([1,2,3]).reduceRight((a,b)=>a+"-"+b))`, `T(()=>new Uint8Array([1,2,3]).every(x=>x>0))`, `T(()=>new Uint8Array([1,2,3]).some(x=>x>2))`, `T(()=>{var s=[];new Uint8Array([1,2]).forEach(function(x,i,a){s.push(x+i+a.length+typeof this)},"s");return s})`,
  `T(()=>new Uint8Array([1,2,3]).map(x=>x*2).buffer.byteLength)`, `T(()=>new Uint8Array(2).map(()=>1n))`, `T(()=>new BigInt64Array(2).map(()=>1))`,
  `T(()=>{class M extends Uint8Array{static get [Symbol.species](){return Int16Array}};return new M([1,2]).map(x=>x*1000)})`,
  `T(()=>{class M extends Uint8Array{};var m=new M([1,2,3]);return [m.slice(1).constructor===M,m.subarray(1).constructor===M,m.map(x=>x).constructor===M,m.filter(x=>x).constructor===M]})`,
  `T(()=>{class M extends Uint8Array{static get [Symbol.species](){return Array}};return new M(2).slice()})`,
  `T(()=>{class M extends Uint8Array{static get [Symbol.species](){return function(n){return new Uint8Array(1)}}};return new M(4).slice()})`,
  `T(()=>{class M extends Uint8Array{static get [Symbol.species](){return BigInt64Array}};return new M(2).slice()})`,
  `T(()=>{class M extends Uint8Array{};return [new M(2).toSorted().constructor===Uint8Array,new M(2).toReversed().constructor===Uint8Array,new M(2).with(0,1).constructor===Uint8Array]})`,
  `T(()=>new Uint8Array(new ArrayBuffer(8,{maxByteLength:16})).length)`, `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b);b.resize(12);return [a.length,a.byteLength]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b,0,4);b.resize(2);return [a.length,a.byteLength,a.byteOffset,a[0]]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b,4);b.resize(2);return [a.length,a.byteLength,a.byteOffset]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b,4);b.resize(2);return a.fill(1)})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b,4);b.resize(2);return a.at(0)})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,4);b.resize(2);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,4);b.resize(12);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,4);b.resize(2);return d.byteOffset})`,
  `T(()=>{var b=new ArrayBuffer(8);b.transfer();var a=new Uint8Array(b)})`, `T(()=>{var b=new ArrayBuffer(8);var a=new Uint8Array(b);b.transfer();return [a.length,a.byteLength,a.byteOffset,a[0]]})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Uint8Array(b);b.transfer();return a.fill(1)})`, `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.getInt8(0)})`, `T(()=>{var b=new ArrayBuffer(8);var a=new Uint8Array(b);var c=b.transfer(4);return [b.detached,c.byteLength,a.length]})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Uint8Array([1,2,3]);a.set(new Uint8Array(b),0)})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);a.fill({valueOf(){a.buffer.transfer();return 1}});return a.length})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.slice({valueOf(){a.buffer.transfer();return 0}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.includes(0,{valueOf(){a.buffer.transfer();return 0}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.indexOf(undefined,{valueOf(){a.buffer.transfer();return 0}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.join({toString(){a.buffer.transfer();return "-"}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.with(0,{valueOf(){a.buffer.transfer();return 5}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);return a.subarray({valueOf(){a.buffer.transfer();return 0}})})`,
  `T(()=>{var a=new Uint8Array([1,2,3]);var r=a.copyWithin(0,{valueOf(){a.buffer.transfer();return 1}});return r})`,
  `T(()=>{var a=new Uint8Array([3,1,2]);return a.sort((x,y)=>{a.buffer.transfer();return x-y})})`,
  `T(()=>{var a=new Uint8Array([3,1,2]);var out=a.map((x,i)=>{if(i==0)a.buffer.transfer();return x});return out})`,
  `T(()=>{var a=new Uint8Array([3,1,2]);var seen=[];a.forEach((x,i)=>{if(i==0)a.buffer.transfer();seen.push(x)});return seen})`,
  `T(()=>new Uint8Array(new ArrayBuffer(8)).buffer.slice(2,6).byteLength)`, `T(()=>new ArrayBuffer(8).slice(-3).byteLength)`, `T(()=>new ArrayBuffer(8).slice(4,2).byteLength)`,
  `T(()=>ArrayBuffer.isView(new DataView(new ArrayBuffer(1))))`, `T(()=>ArrayBuffer.isView(new ArrayBuffer(1)))`, `T(()=>ArrayBuffer.isView([]))`, `T(()=>ArrayBuffer.isView())`,
  `T(()=>new ArrayBuffer(-1))`, `T(()=>new ArrayBuffer(2**53))`, `T(()=>new ArrayBuffer(8,{maxByteLength:4}))`, `T(()=>new ArrayBuffer(1.9).byteLength)`, `T(()=>new ArrayBuffer("3").byteLength)`, `T(()=>ArrayBuffer(1))`,
  `T(()=>new Float64Array(2**53))`, `T(()=>new Uint8Array(2**53))`);

// A matriz completa passa de 3700 programas; a amostra por hash (sampleByHash) mantém ~530.
const STRIDE = 7;
const allExprs = [...new Set(exprs)];
const unique = sampleByHash(allExprs, Math.ceil(allExprs.length / STRIDE));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
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
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) {
    dropped++;
    process.stderr.write("caminho no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
