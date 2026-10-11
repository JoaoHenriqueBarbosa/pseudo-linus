const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/buffer_edge_bun.tsv: borda de buffers redimensionáveis e Atomics, medido no bun 1.4.2.
// Cobre Atomics (add/and/compareExchange/exchange/load/or/store/sub/xor em todos os tipos inteiros, notify, wait com
// timeout 0, waitAsync), SharedArrayBuffer (growable, grow, maxByteLength), ArrayBuffer redimensionável (resize,
// transfer, transferToFixedLength, detached), typed arrays com length-tracking e DataView sobre eles, erros e mensagens.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-golden.js.
// Uso: bun scripts/gen-buffer-edge-golden.js > tests/golden/buffer_edge_bun.tsv
const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'ArrayBuffer.isView(v)||v instanceof ArrayBuffer||(typeof SharedArrayBuffer!=="undefined"&&v instanceof SharedArrayBuffer)?"<"+Object.prototype.toString.call(v)+">"+Array.from(new Uint8Array(v.buffer||v)).join():' +
  'Array.isArray(v)?"["+v.map(S).join()+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

const intTypes = ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array"];
const bigTypes = ["BigInt64Array", "BigUint64Array"];
const badTypes = ["Uint8ClampedArray", "Float32Array", "Float64Array"];
const ops = ["add", "and", "compareExchange", "exchange", "or", "sub", "xor"];
const vals = ["0", "1", "5", "-1", "255", "256", "65535", "2**31", "2**32+3", "1.9", "NaN", "'7'", "undefined"];

// ---- 1. Operações de Atomics por tipo, em buffer comum e compartilhado.
for (const t of intTypes) {
  for (const sab of [false, true]) {
    const mk = `new ${t}(new ${sab ? "SharedArrayBuffer" : "ArrayBuffer"}(16))`;
    for (const op of ops) {
      for (const v of ["1", "5", "-1", "255", "2**32+3", "1.9", "'7'"]) {
        const args = op === "compareExchange" ? `0,0,${v}` : `0,${v}`;
        add(`T(()=>{var a=${mk};a[0]=3;var r=Atomics.${op}(a,${args});return [r,a[0]]})`);
      }
    }
    add(`T(()=>{var a=${mk};return [Atomics.store(a,1,300),a[1],Atomics.load(a,1)]})`,
      `T(()=>{var a=${mk};return [Atomics.store(a,0,-0),Object.is(a[0],0)]})`,
      `T(()=>Atomics.store(${mk},0,1.5))`, `T(()=>Atomics.store(${mk},0,"x"))`, `T(()=>Atomics.load(${mk},4096))`,
      `T(()=>Atomics.load(${mk},-1))`, `T(()=>Atomics.load(${mk},"1"))`, `T(()=>Atomics.load(${mk},1.5))`, `T(()=>Atomics.load(${mk},NaN))`,
      `T(()=>Atomics.add(${mk},0))`, `T(()=>Atomics.add(${mk}))`, `T(()=>Atomics.add())`, `T(()=>Atomics.isLockFree(${mk}.BYTES_PER_ELEMENT))`);
  }
}
for (const t of bigTypes) {
  for (const op of ops) {
    for (const v of ["1n", "-1n", "2n**64n+3n", "2n**63n"]) {
      const args = op === "compareExchange" ? `0,0n,${v}` : `0,${v}`;
      add(`T(()=>{var a=new ${t}(new SharedArrayBuffer(16));a[0]=3n;var r=Atomics.${op}(a,${args});return [r,a[0]]})`);
    }
    add(`T(()=>Atomics.${op}(new ${t}(2),0,1))`, `T(()=>Atomics.${op}(new ${t}(2),0,"x"))`);
  }
  add(`T(()=>[Atomics.store(new ${t}(2),0,5n),Atomics.load(new ${t}(2),0)])`, `T(()=>Atomics.store(new ${t}(2),0,1))`, `T(()=>Atomics.load(new ${t}(2),2))`);
}
for (const t of badTypes) {
  for (const op of ["add", "load", "store", "exchange", "compareExchange", "wait", "notify"]) add(`T(()=>Atomics.${op}(new ${t}(4),0,0,0))`);
}
for (const op of ops) for (const bad of ["null", "undefined", "{}", "[]", "1", "'a'", "new DataView(new ArrayBuffer(8))", "new ArrayBuffer(8)"]) add(`T(()=>Atomics.${op}(${bad},0,0,0))`);
for (const v of vals) add(`T(()=>{var a=new Int32Array(2);return [Atomics.add(a,0,${v}),a[0]]})`, `T(()=>Atomics.compareExchange(new Int8Array(2),0,${v},1))`);

// ---- 2. notify, wait com timeout 0 e waitAsync.
add(`T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0))`, `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0,0))`,
  `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0,-5))`, `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0,Infinity))`,
  `T(()=>Atomics.notify(new Int32Array(8),0))`, `T(()=>Atomics.notify(new Int32Array(8),9))`, `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),2))`,
  `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),-1))`, `T(()=>Atomics.notify(new BigInt64Array(new SharedArrayBuffer(16)),0,1))`,
  `T(()=>Atomics.notify(new Int16Array(new SharedArrayBuffer(8)),0))`, `T(()=>Atomics.notify(new Uint32Array(new SharedArrayBuffer(8)),0))`,
  `T(()=>Atomics.notify(new BigUint64Array(new SharedArrayBuffer(16)),0))`, `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0,NaN))`,
  `T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(8)),0,"2"))`);
for (const arr of ["new Int32Array(new SharedArrayBuffer(8))", "new BigInt64Array(new SharedArrayBuffer(16))"]) {
  const big = arr.includes("Big");
  const z = big ? "0n" : "0", one = big ? "1n" : "1";
  for (const [label, t] of [["0", "0"], ["neg", "-5"], ["nan0", "0.4"], ["str0", "'0'"], ["negInf", "-Infinity"], ["null", "null"]]) {
    add(`T(()=>Atomics.wait(${arr},0,${z},${t}))`, `T(()=>Atomics.wait(${arr},0,${one},${t}))`);
  }
  add(`T(()=>Atomics.wait(${arr},0,${z},undefined===1?0:0))`, `T(()=>Atomics.wait(${arr},9,${z},0))`, `T(()=>Atomics.wait(${arr},-1,${z},0))`,
    `T(()=>Atomics.wait(${arr},0,${z},0,1))`, `T(()=>Atomics.wait(${arr},"0",${z},0))`, `T(()=>Atomics.wait(${arr},0,undefined,0))`);
}
add(`T(()=>Atomics.wait(new Int32Array(4),0,0,0))`, `T(()=>Atomics.wait(new BigInt64Array(4),0,0n,0))`, `T(()=>Atomics.wait(new Int16Array(new SharedArrayBuffer(8)),0,0,0))`,
  `T(()=>Atomics.wait(new Uint32Array(new SharedArrayBuffer(8)),0,0,0))`, `T(()=>Atomics.wait(new BigUint64Array(new SharedArrayBuffer(16)),0,0n,0))`,
  `T(()=>Atomics.wait(new Int32Array(new SharedArrayBuffer(8)),0,0,Symbol()))`, `T(()=>Atomics.wait(new Int32Array(new SharedArrayBuffer(8)),0,0n,0))`,
  `T(()=>Atomics.wait(new BigInt64Array(new SharedArrayBuffer(16)),0,0,0))`);
const wa = (arr, rest) => `Atomics.waitAsync(${arr},${rest})`;
for (const arr of ["new Int32Array(new SharedArrayBuffer(8))", "new BigInt64Array(new SharedArrayBuffer(16))"]) {
  const big = arr.includes("Big");
  const z = big ? "0n" : "0", one = big ? "1n" : "1";
  // Timeout infinito deixaria o processo do bun vivo esperando o timer, então só finitos.
  for (const t of ["0", "-1", "10", "'5'", "null", "-Infinity"]) {
    add(`T(()=>{var r=${wa(arr, `0,${z},${t}`)};return [r.async,r.value instanceof Promise,Object.keys(r).join()]})`,
      `T(()=>{var r=${wa(arr, `0,${one},${t}`)};return [r.async,r.value]})`);
  }
  add(`T(()=>{var r=${wa(arr, `0,${z},0`)};return [r.async,r.value]})`, `T(()=>Object.getPrototypeOf(${wa(arr, `0,${z},0`)})===Object.prototype)`,
    `T(()=>${wa(arr, `9,${z},0`)})`, `T(()=>${wa(arr, `-1,${z},0`)})`, `T(()=>${wa(arr, `0,${z},5`)}.value instanceof Promise)`,
    `T(()=>{var a=${arr};var r=Atomics.waitAsync(a,0,${z},1000);return [r.async,Atomics.notify(a,0)]})`,
    `T(()=>{var a=${arr};var r=Atomics.waitAsync(a,0,${z},1000);return [Atomics.notify(a,0,1),Atomics.notify(a,0)]})`);
}
add(`T(()=>Atomics.waitAsync(new Int32Array(4),0,0,0))`, `T(()=>Atomics.waitAsync(new Int16Array(new SharedArrayBuffer(8)),0,0,0))`,
  `T(()=>Atomics.waitAsync(new Float64Array(new SharedArrayBuffer(8)),0,0,0))`, `T(()=>Atomics.waitAsync({},0,0,0))`, `T(()=>Atomics.waitAsync())`,
  `T(()=>Atomics.waitAsync.length)`, `T(()=>Atomics.wait.length)`, `T(()=>Atomics.notify.length)`, `T(()=>Atomics.waitAsync.name)`);

// ---- 3. Atomics com buffer redimensionável, destacado ou crescido.
add(`T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);b.resize(4);return [a.length,Atomics.load(a,0)]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);b.resize(4);return Atomics.load(a,1)})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b,4,1);b.resize(4);return Atomics.load(a,0)})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Int32Array(b);b.transfer();return Atomics.load(a,0)})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Int32Array(b);b.transfer();return Atomics.store(a,0,1)})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Int32Array(b);b.transfer();return Atomics.notify(a,0)})`,
  `T(()=>{var b=new ArrayBuffer(8);var a=new Int32Array(b);b.transfer();return Atomics.wait(a,0,0,0)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);b.grow(16);return [a.length,Atomics.add(a,3,7),Atomics.load(a,3)]})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);return Atomics.load(a,3)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);b.grow(16);return Atomics.notify(a,3)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);return Atomics.wait(a,0,1,0)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b,0,2);b.grow(16);return [a.length,Atomics.exchange(a,1,9),a[1]]})`);

// ---- 4. SharedArrayBuffer: growable, grow, maxByteLength.
const sabArgs = ["", "0", "8", "8,{maxByteLength:8}", "8,{maxByteLength:16}", "0,{maxByteLength:0}", "0,{maxByteLength:4}", "4,{maxByteLength:2}", "4,{maxByteLength:undefined}",
  "4,{}", "4,null", "4,1", "4,{maxByteLength:'8'}", "4,{maxByteLength:8.9}", "4,{maxByteLength:-1}", "4,{maxByteLength:NaN}", "4,{maxByteLength:Infinity}",
  "4,{maxByteLength:2**53}", "4,{maxByteLength:2**32}", "-1", "1.5", "'3'", "NaN", "2**53", "{}", "Symbol()", "4,{get maxByteLength(){return 6}}",
  "4,{get maxByteLength(){throw new EvalError('g')}}", "4,{maxByteLength:null}", "4,{maxByteLength:{valueOf(){return 12}}}"];
for (const a of sabArgs) {
  add(`T(()=>{var b=new SharedArrayBuffer(${a});return [b.byteLength,b.growable,b.maxByteLength]})`);
}
for (const [a, g] of [["4,{maxByteLength:8}", "8"], ["4,{maxByteLength:8}", "4"], ["4,{maxByteLength:8}", "3"], ["4,{maxByteLength:8}", "9"], ["4,{maxByteLength:8}", "-1"],
  ["4,{maxByteLength:8}", "5.5"], ["4,{maxByteLength:8}", "'6'"], ["4,{maxByteLength:8}", "NaN"], ["4,{maxByteLength:8}", "undefined"], ["4,{maxByteLength:8}", "{valueOf(){return 7}}"],
  ["4,{maxByteLength:8}", "2**53"], ["4", "4"], ["4", "8"], ["4", "2"], ["0,{maxByteLength:8}", "0"], ["0,{maxByteLength:8}", "8"], ["4,{maxByteLength:8}", "Symbol()"],
  ["4,{maxByteLength:8}", "8n"]]) {
  add(`T(()=>{var b=new SharedArrayBuffer(${a});var r=b.grow(${g});return [r,b.byteLength,b.growable,b.maxByteLength]})`);
}
add(`T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});b.grow(6);b.grow(6);b.grow(8);return b.byteLength})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});b.grow(6);return b.grow(5)})`,
  `T(()=>SharedArrayBuffer.prototype.grow.call(new ArrayBuffer(4,{maxByteLength:8}),6))`, `T(()=>SharedArrayBuffer.prototype.grow.call({},6))`,
  `T(()=>SharedArrayBuffer.prototype.grow.call(undefined,6))`, `T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,"growable").get.call(new ArrayBuffer(1)))`,
  `T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,"maxByteLength").get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,"byteLength").get.call(new ArrayBuffer(1)))`,
  `T(()=>SharedArrayBuffer.prototype.slice.call(new ArrayBuffer(1)))`, `T(()=>SharedArrayBuffer())`, `T(()=>SharedArrayBuffer.length)`, `T(()=>SharedArrayBuffer.prototype.grow.length)`,
  `T(()=>Object.prototype.toString.call(new SharedArrayBuffer(1)))`, `T(()=>SharedArrayBuffer.prototype[Symbol.toStringTag])`, `T(()=>Object.getOwnPropertyNames(SharedArrayBuffer.prototype).sort().join())`,
  `T(()=>new SharedArrayBuffer(8,{maxByteLength:16}).slice(2,6).growable)`, `T(()=>new SharedArrayBuffer(8,{maxByteLength:16}).slice(2,6).byteLength)`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var s=b.slice(-4);b.grow(16);return [s.byteLength,b.slice(10).byteLength,b.slice(0,100).byteLength]})`,
  `T(()=>new SharedArrayBuffer(8)[Symbol.species])`, `T(()=>SharedArrayBuffer[Symbol.species]===SharedArrayBuffer)`, `T(()=>ArrayBuffer.isView(new SharedArrayBuffer(1)))`,
  `T(()=>new SharedArrayBuffer(8)instanceof ArrayBuffer)`, `T(()=>Object.getPrototypeOf(SharedArrayBuffer.prototype)===Object.prototype)`,
  `T(()=>{var b=new SharedArrayBuffer(8);b.byteLength=3;return b.byteLength})`, `T(()=>{"use strict";var b=new SharedArrayBuffer(8);b.byteLength=3})`);

// ---- 5. ArrayBuffer redimensionável: resize, transfer, detached.
const abArgs = ["8,{maxByteLength:16}", "0,{maxByteLength:0}", "0,{maxByteLength:8}", "8,{maxByteLength:8}", "9,{maxByteLength:8}", "4,{maxByteLength:-1}", "4,{maxByteLength:2**53}",
  "4,{maxByteLength:undefined}", "4,{maxByteLength:'8'}", "4,{maxByteLength:8.5}", "4,{maxByteLength:NaN}", "4,{maxByteLength:null}", "4,{maxByteLength:Infinity}", "4,{get maxByteLength(){return 5}}", "4,5", "4,undefined", "4,{}"];
for (const a of abArgs) {
  add(`T(()=>{var b=new ArrayBuffer(${a});return [b.byteLength,b.resizable,b.maxByteLength,b.detached]})`);
}
for (const n of ["8", "4", "0", "16", "17", "-1", "1.5", "'6'", "NaN", "undefined", "2**53", "{valueOf(){return 3}}", "Symbol()", "1n", "null", "Infinity"]) {
  add(`T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var r=b.resize(${n});return [r,b.byteLength,b.resizable,b.maxByteLength]})`,
    `T(()=>{var b=new ArrayBuffer(8);return b.resize(${n})})`);
  add(`T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);u.fill(7);var t=b.transfer(${n});return [b.detached,b.byteLength,b.maxByteLength,b.resizable,t.byteLength,t.resizable,t.maxByteLength,Array.from(new Uint8Array(t)).join("")]})`,
    `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);u.fill(7);var t=b.transferToFixedLength(${n});return [b.detached,t.byteLength,t.resizable,t.maxByteLength,Array.from(new Uint8Array(t)).join("")]})`,
    `T(()=>{var b=new ArrayBuffer(8);var t=b.transfer(${n});return [t.byteLength,t.resizable,b.detached]})`);
}
add(`T(()=>{var b=new ArrayBuffer(8);var t=b.transfer();return [b.detached,b.byteLength,t.byteLength,t.detached]})`,
  `T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.transfer()})`, `T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.transferToFixedLength()})`,
  `T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.resize(4)})`, `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});b.transfer();return b.resize(4)})`,
  `T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.slice(0)})`, `T(()=>{var b=new ArrayBuffer(8);b.transfer();return [b.byteLength,b.maxByteLength,b.resizable,b.detached]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});b.transfer();return [b.byteLength,b.maxByteLength,b.resizable,b.detached]})`,
  `T(()=>ArrayBuffer.prototype.transfer.call(new SharedArrayBuffer(8)))`, `T(()=>ArrayBuffer.prototype.resize.call(new SharedArrayBuffer(8,{maxByteLength:16}),4))`,
  `T(()=>ArrayBuffer.prototype.transfer.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"detached").get.call(new SharedArrayBuffer(1)))`,
  `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"resizable").get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"maxByteLength").get.call(new SharedArrayBuffer(1)))`,
  `T(()=>ArrayBuffer.prototype.transfer.length+ArrayBuffer.prototype.transferToFixedLength.length+ArrayBuffer.prototype.resize.length)`,
  `T(()=>Object.getOwnPropertyNames(ArrayBuffer.prototype).sort().join())`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});return [b.slice(2).resizable,b.slice(2).byteLength,b.slice(2).maxByteLength]})`,
  `T(()=>{var b=new ArrayBuffer(8);var t=b.transfer(4);return [t.byteLength,Array.from(new Uint8Array(t))]})`,
  `T(()=>{var b=new ArrayBuffer(4);new Uint8Array(b).set([1,2,3,4]);var t=b.transfer(8);return Array.from(new Uint8Array(t)).join()})`,
  `T(()=>{var b=new ArrayBuffer(4);new Uint8Array(b).set([1,2,3,4]);var t=b.transfer(2);return Array.from(new Uint8Array(t)).join()})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var t=b.transfer(8);return [t.resizable,t.maxByteLength]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var t=b.transfer(9);return [t.resizable,t.maxByteLength]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var t=b.transfer(0);return [t.resizable,t.byteLength,t.maxByteLength]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});b.resize(6);b.resize(0);b.resize(8);return b.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});new Uint8Array(b).fill(1);b.resize(8);return Array.from(new Uint8Array(b)).join()})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});new Uint8Array(b).fill(1);b.resize(2);b.resize(6);return Array.from(new Uint8Array(b)).join()})`,
  `T(()=>ArrayBuffer(4,{maxByteLength:8}))`, `T(()=>ArrayBuffer.isView(new ArrayBuffer(1,{maxByteLength:2})))`,
  `T(()=>structuredClone===undefined?"nosc":typeof structuredClone)`);

// ---- 6. Typed arrays com length-tracking e com comprimento fixo sobre redimensionável.
const taAll = intTypes.concat(["Uint8ClampedArray", "Float32Array", "Float64Array"]);
for (const t of taAll) {
  add(`T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var r=[a.length,a.byteLength,a.byteOffset];b.resize(32);r.push(a.length,a.byteLength);b.resize(3);r.push(a.length,a.byteLength);b.resize(0);r.push(a.length,a.byteLength);return r})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,0,2);var r=[a.length,a.byteLength];b.resize(32);r.push(a.length);b.resize(${t.includes("8") ? 1 : 3});r.push(a.length,a.byteLength,a.byteOffset);return r})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);var r=[a.length,a.byteOffset];b.resize(4);r.push(a.length,a.byteOffset,a.byteLength);b.resize(24);r.push(a.length,a.byteOffset);return r})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);b.resize(5);return [a.length,Array.from(a).length]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);b.resize(4);return Array.from(a)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);b.resize(4);return a.fill(1)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);b.resize(4);return a.at(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);b.resize(4);return [a[0],0 in a,Object.keys(a).length]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b,8);b.resize(4);b.resize(16);return [a.length,a.byteOffset]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);b.resize(32);a[a.length-1]=1;return [a.length,a[a.length-1],a[a.length]]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);b.transfer();return [a.length,a.byteLength,a.byteOffset]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);b.transfer();return a.fill(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var c=a.subarray(1);b.resize(32);return [a.length,c.length]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var c=a.subarray(1,2);b.resize(32);return [c.length]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var c=a.slice(1);b.resize(32);return [c.length,c.buffer.resizable]})`,
    `T(()=>new ${t}(new ArrayBuffer(16,{maxByteLength:32}),17))`, `T(()=>new ${t}(new ArrayBuffer(16,{maxByteLength:32}),0,17))`,
    `T(()=>new ${t}(new ArrayBuffer(16,{maxByteLength:32}),1))`, `T(()=>new ${t}(new ArrayBuffer(16,{maxByteLength:32}),0,undefined).length)`,
    `T(()=>new ${t}(new ArrayBuffer(16,{maxByteLength:32}),16).length)`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);return [Object.keys(a).length,JSON.stringify(Array.from(a.keys()).length)]})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var it=a.values();it.next();b.resize(0);return it.next()})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var it=a.values();b.transfer();return it.next()})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);return a.map(function(x,i){if(i==0)b.resize(0);return 1}).length})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);var n=0;a.forEach(function(){if(n++==0)b.resize(4)});return n})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new ${t}(b);return a.sort(function(x,y){b.resize(0);return 0}).length})`);
}
for (const t of bigTypes) {
  add(`T(()=>{var b=new ArrayBuffer(32,{maxByteLength:64});var a=new ${t}(b);var r=[a.length];b.resize(64);r.push(a.length);b.resize(7);r.push(a.length,a.byteLength);return r})`,
    `T(()=>{var b=new ArrayBuffer(32,{maxByteLength:64});var a=new ${t}(b,8,2);b.resize(16);return [a.length,a.byteOffset,a.byteLength]})`,
    `T(()=>{var b=new ArrayBuffer(32,{maxByteLength:64});var a=new ${t}(b);b.resize(64);a[7]=5n;return [a[7],a.length]})`);
}
add(`T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new Uint8Array(b);a.set([1,2,3]);b.resize(2);return Array.from(a).join()})`,
  `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var a=new Uint8Array(b);var s=a.toString();b.resize(2);return [s.length,a.toString()]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);a.set([1,2,3,4]);b.resize(8);return [Array.from(a).join(),a.toReversed().join()]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);b.resize(8);return [a.with(7,1).length]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);b.resize(8);return Object.getOwnPropertyDescriptor(a,7)})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return [Object.isExtensible(a),Object.isFrozen(a)]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return Object.freeze(a)})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b,0,2);return Object.freeze(a).length})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return Object.seal(a).length})`,
  `T(()=>{var b=new ArrayBuffer(4);var a=new Uint8Array(b);return Object.freeze(a).length})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return Object.isFrozen(Object.freeze(new Uint8Array(0)))})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return [a.buffer===b,a.buffer.resizable]})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return Uint8Array.from(a).buffer.resizable})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return new Uint8Array(a).buffer.resizable})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return a.map(x=>x).buffer.resizable})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return a.filter(x=>1).buffer.resizable})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return a.toSorted().buffer.resizable})`,
  `T(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return a.subarray(1).buffer===b})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);b.grow(8);return [a.length,a.byteLength]})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b,2);b.grow(8);return [a.length,a.byteOffset]})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b,0,4);b.grow(8);return [a.length]})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);var s=a.subarray(1);b.grow(8);return [s.length]})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Int32Array(b);b.grow(7);return [a.length,a.byteLength]})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);a.set([1,2,3,4]);b.grow(8);return Array.from(a).join()})`,
  `T(()=>{var b=new SharedArrayBuffer(4,{maxByteLength:8});var a=new Uint8Array(b);return a.buffer.growable})`,
  `T(()=>{var b=new SharedArrayBuffer(4);var a=new Uint8Array(b);return [a.length,b.growable]})`);

// ---- 7. DataView sobre buffers redimensionáveis.
const dvGets = ["getInt8", "getUint8", "getInt16", "getUint16", "getInt32", "getUint32", "getFloat32", "getFloat64", "getBigInt64", "getBigUint64"];
add(`T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);var r=[d.byteLength,d.byteOffset];b.resize(16);r.push(d.byteLength);b.resize(2);r.push(d.byteLength);b.resize(0);r.push(d.byteLength);return r})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2);var r=[d.byteLength];b.resize(16);r.push(d.byteLength);b.resize(4);r.push(d.byteLength);b.resize(2);r.push(d.byteLength);return r})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2);b.resize(1);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2);b.resize(1);return d.byteOffset})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2,4);b.resize(5);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2,4);b.resize(5);return d.byteOffset})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2,4);b.resize(6);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2,4);b.resize(6);b.resize(5);b.resize(8);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,2,4);b.resize(5);return d.getInt8(0)})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,8);b.resize(4);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,8);return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.transfer();return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.transfer();return d.byteOffset})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.transfer();return d.buffer===b})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.transfer();return d.getInt8(0)})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.transfer();return d.setInt8(0,1)})`,
  `T(()=>new DataView(new ArrayBuffer(8,{maxByteLength:16}),9))`, `T(()=>new DataView(new ArrayBuffer(8,{maxByteLength:16}),0,9))`,
  `T(()=>new DataView(new ArrayBuffer(8,{maxByteLength:16}),-1))`, `T(()=>new DataView(new ArrayBuffer(8,{maxByteLength:16}),0,-1))`,
  `T(()=>{var b=new ArrayBuffer(8);b.transfer();return new DataView(b)})`, `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.byteLength})`,
  `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b);b.transfer();return d.byteOffset})`, `T(()=>{var b=new ArrayBuffer(8);var d=new DataView(b,2,2);b.transfer();return d.getUint8(0)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.grow(16);return [d.byteLength,d.byteOffset]})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,4);b.grow(16);return d.byteLength})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(b,4,2);b.grow(16);return d.byteLength})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);b.grow(16);d.setUint8(15,9);return d.getUint8(15)})`,
  `T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);return d.getUint8(8)})`,
  `T(()=>{var b=new SharedArrayBuffer(8);var d=new DataView(b);return [d.byteLength,d.buffer===b]})`,
  `T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);return [d.buffer===b,d.buffer.resizable]})`);
for (const g of dvGets) {
  const size = /64|Float64/.test(g) ? 8 : /32/.test(g) ? 4 : /16/.test(g) ? 2 : 1;
  add(`T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);return d.${g}(${16 - size})})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);return d.${g}(${17 - size})})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.resize(32);return d.${g}(${32 - size})})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.resize(${size - 1});return d.${g}(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.resize(${size});return d.${g}(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b,8);b.resize(4);return d.${g}(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.transfer();return d.${g}(0)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);return d.${g}(-1)})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);return d.${g}()})`);
  const s = g.replace("get", "set"), v = g.includes("Big") ? "1n" : "1";
  add(`T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.resize(${size - 1});return d.${s}(0,${v})})`,
    `T(()=>{var b=new ArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.resize(32);d.${s}(${32 - size},${v});return d.${g}(${32 - size})})`,
    `T(()=>{var b=new SharedArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);b.grow(32);d.${s}(${32 - size},${v});return d.${g}(${32 - size})})`,
    `T(()=>{var b=new SharedArrayBuffer(16,{maxByteLength:32});var d=new DataView(b);return d.${s}(${17 - size},${v})})`);
}

// ---- 8. Mensagens de erro e propriedades das funções.
add(`T(()=>new ArrayBuffer(8,{maxByteLength:4}))`, `T(()=>new ArrayBuffer(1,{maxByteLength:2**60}))`, `T(()=>new SharedArrayBuffer(8,{maxByteLength:4}))`,
  `T(()=>new SharedArrayBuffer(1,{maxByteLength:2**60}))`, `T(()=>new ArrayBuffer(2**60))`, `T(()=>new SharedArrayBuffer(2**60))`,
  `T(()=>new ArrayBuffer(8,{maxByteLength:Symbol()}))`, `T(()=>new SharedArrayBuffer(8,{maxByteLength:Symbol()}))`, `T(()=>new ArrayBuffer(8,{maxByteLength:1n}))`,
  `T(()=>new ArrayBuffer(Symbol()))`, `T(()=>new ArrayBuffer(1n))`, `T(()=>new SharedArrayBuffer(1n))`, `T(()=>new ArrayBuffer(8,{maxByteLength:{valueOf(){throw new TypeError("v")}}}))`,
  `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"resizable").get.name)`, `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"detached").get.name)`,
  `T(()=>Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"maxByteLength").get.name)`, `T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,"growable").get.name)`,
  `T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,"maxByteLength").get.name)`, `T(()=>ArrayBuffer.prototype.resize.name+ArrayBuffer.prototype.transfer.name+ArrayBuffer.prototype.transferToFixedLength.name)`,
  `T(()=>SharedArrayBuffer.prototype.grow.name)`, `T(()=>Object.getOwnPropertyNames(Atomics).sort().join())`, `T(()=>Object.getOwnPropertyDescriptor(Atomics,Symbol.toStringTag).value)`,
  `T(()=>Object.getOwnPropertyNames(Atomics).map(k=>Atomics[k].length).join())`, `T(()=>Atomics.isLockFree(1)+","+Atomics.isLockFree(2)+","+Atomics.isLockFree(4)+","+Atomics.isLockFree(8)+","+Atomics.isLockFree(3)+","+Atomics.isLockFree(0)`+`)`,
  `T(()=>Atomics.isLockFree("4"))`, `T(()=>Atomics.isLockFree())`, `T(()=>Atomics.isLockFree(Symbol()))`, `T(()=>Atomics.isLockFree(4n))`, `T(()=>new Atomics())`, `T(()=>Atomics())`,
  `T(()=>Atomics.pause===undefined?"nopause":typeof Atomics.pause)`, `T(()=>Atomics.add.call(null,new Int8Array(1),0,1))`);
for (const [a, g] of [["8", "{maxByteLength:16}"], ["0", "{maxByteLength:1}"]]) {
  add(`T(()=>{var b=new ArrayBuffer(${a},${g});return Object.getOwnPropertyNames(b).length+","+Object.keys(b).length})`);
}

// ---- Execução.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa. Buffer é o objeto sob teste deste golden e fica.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const expr of unique) {
  if (HOST.test(expr)) continue;
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
