// Gera tests/golden/sab_bun.tsv: SharedArrayBuffer (construtor, growable, grow, maxByteLength, slice, species, subclasses,
// @@toStringTag, receptores errados), Atomics.wait/notify/waitAsync nos caminhos que não bloqueiam, erros de tipo e de
// faixa do Atomics, DataView sobre SharedArrayBuffer redimensionável (length-tracking) e TypedArray sobre ele depois de
// `grow`, medidos no bun 1.4.2. Complementa atomics_bun.tsv e buffers_bun.tsv: programas cujo texto já aparece nos goldens
// de atomics, buffer e typedarray são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem API de host. Os programas assíncronos usam `A(fn, n)`: roda `fn`, esvazia as
// microtarefas (cadeia de `n` then) e só então grava `R` com o log. A resolução da promessa de `waitAsync` por `notify`
// acontece numa tarefa do host (depois das microtarefas), então o log de microtarefas não a vê: isso também é medido.
// Uso: bun scripts/gen-sab-golden.js > tests/golden/sab_bun.tsv
const { emitFactoredLines, sampleByHash, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

// O filho só avalia o programa; quem imprime `R` na saída é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  process.on("unhandledRejection", () => {});
  (0, eval)(fs.readFileSync(0, "utf8"));
  setTimeout(() => {
    process.exit(0);
  }, 0);
} else {
  main();
}

async function main() {
  const PRELOAD = writeResultPreload();
  const PRELUDE =
    'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
    'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";' +
    'if(v instanceof Promise)return "Promise";if(v instanceof SharedArrayBuffer)return "SAB("+v.byteLength+")";if(v instanceof ArrayBuffer)return "AB("+v.byteLength+")";' +
    'if(ArrayBuffer.isView(v)&&!(v instanceof DataView))return Object.prototype.toString.call(v).slice(8,-1)+"["+Array.prototype.map.call(v,function(x){return S(x,d+1)}).join(",")+"]";' +
    'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
    'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
    'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
    'var log=[];function L(x){log.push(typeof x==="string"?x:S(x))}\n' +
    'function A(f,n){try{f()}catch(e){L("throw "+(e&&e.name)+": "+(e&&e.message))}globalThis.R="pending";var p=Promise.resolve();for(var i=0;i<(n||10);i++)p=p.then(function(){});p.then(function(){globalThis.R=S(log)})}\n';

  const exprs = [];
  const add = (...list) => exprs.push(...list);
  const q = (v) => (typeof v === "string" ? JSON.stringify(v) : String(v));

  // ---- 1. Construtor: comprimento x opções.
  const lens = ["0", "1", "8", "16", "'4'", "1.9", "-1", "NaN", "undefined", "null", "true", "{valueOf(){return 3}}", "2**53", "Infinity", "[5]", "'abc'", "-0", "0.5"];
  const opts = ["undefined", "{}", "{maxByteLength:16}", "{maxByteLength:8}", "{maxByteLength:4}", "{maxByteLength:undefined}", "{maxByteLength:'16'}",
    "{maxByteLength:-1}", "{maxByteLength:1.5}", "{maxByteLength:2**53}", "null", "1", "'x'", "{maxByteLength:{valueOf(){return 32}}}", "{maxByteLength:0}"];
  for (const l of lens) for (const o of opts) {
    add(`T(()=>{var s=new SharedArrayBuffer(${l},${o});return [s.byteLength,s.growable,s.maxByteLength]})`);
  }
  add("T(()=>SharedArrayBuffer(8))", "T(()=>new SharedArrayBuffer())", "T(()=>new SharedArrayBuffer().byteLength)", "T(()=>new SharedArrayBuffer(Symbol()))",
    "T(()=>new SharedArrayBuffer(1n))", "T(()=>new SharedArrayBuffer(8,{get maxByteLength(){throw new RangeError('g')}}))",
    "T(()=>{var l=[];new SharedArrayBuffer({valueOf(){l.push('len');return 4}},{get maxByteLength(){l.push('max');return 8}});return l})",
    "T(()=>{var l=[];Reflect.construct(SharedArrayBuffer,[{valueOf(){l.push('len');return 4}},{get maxByteLength(){l.push('max');return 8}}],Object.defineProperty(function(){},'prototype',{get(){l.push('proto');return SharedArrayBuffer.prototype}}));return l})",
    "T(()=>new SharedArrayBuffer(2**33).byteLength)", "T(()=>new SharedArrayBuffer(2**31,{maxByteLength:2**32}).byteLength)");

  // ---- 2. grow: alvo x estado.
  const targets = ["0", "4", "7", "8", "9", "12", "16", "17", "'12'", "NaN", "undefined", "-1", "1.5", "2**32", "Infinity", "null", "{valueOf(){return 12}}", "-0", "12.9"];
  for (const t of targets) {
    add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var r=s.grow(${t});return [r,s.byteLength,s.growable,s.maxByteLength]})`,
      `T(()=>{var s=new SharedArrayBuffer(8);return s.grow(${t})})`,
      `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:8});return [s.grow(${t}),s.byteLength]})`,
      `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});s.grow(12);return [s.grow(${t}),s.byteLength]})`,
      `T(()=>{var s=new SharedArrayBuffer(0,{maxByteLength:16});s.grow(${t});return [s.byteLength,Array.from(new Uint8Array(s))]})`);
  }
  const receivers = ["new ArrayBuffer(8)", "new ArrayBuffer(8,{maxByteLength:16})", "{}", "null", "undefined", "1", "'s'", "SharedArrayBuffer.prototype", "new Proxy(new SharedArrayBuffer(8,{maxByteLength:16}),{})",
    "new DataView(new ArrayBuffer(8))", "new Uint8Array(new SharedArrayBuffer(8))", "Object.create(new SharedArrayBuffer(8,{maxByteLength:16}))", "SharedArrayBuffer", "[]", "Symbol()", "8n"];
  for (const r of receivers) {
    for (const g of ["byteLength", "growable", "maxByteLength"]) {
      add(`T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'${g}').get.call(${r}))`);
    }
    add(`T(()=>SharedArrayBuffer.prototype.grow.call(${r},12))`, `T(()=>SharedArrayBuffer.prototype.slice.call(${r},0,2))`,
      `T(()=>Object.prototype.toString.call(${r}))`);
  }

  // ---- 3. slice: início x fim x estado.
  const starts = ["undefined", "0", "2", "-2", "-100", "100", "NaN", "'3'", "1.5", "Infinity", "-Infinity", "{valueOf(){return 1}}", "null", "true", "8", "7", "-8", "-9"];
  const ends = ["undefined", "0", "4", "-1", "-100", "100", "NaN", "'5'", "Infinity", "-Infinity", "8", "2.9", "null", "-8"];
  const fill8 = "var u=new Uint8Array(s);for(var i=0;i<u.length;i++)u[i]=i+1;";
  for (const a of starts) for (const b of ends) {
    add(`T(()=>{var s=new SharedArrayBuffer(8);${fill8}var r=s.slice(${a},${b});return [r.byteLength,r.growable,r.maxByteLength,Array.from(new Uint8Array(r)),r instanceof SharedArrayBuffer,r!==s]})`);
  }
  for (const a of starts.slice(0, 10)) for (const b of ends.slice(0, 7)) {
    add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});${fill8}s.grow(12);var u2=new Uint8Array(s);u2[10]=99;var r=s.slice(${a},${b});return [r.byteLength,r.growable,Array.from(new Uint8Array(r))]})`);
  }
  add("T(()=>new SharedArrayBuffer(8).slice.length)", "T(()=>new SharedArrayBuffer(8).slice.name)", "T(()=>new SharedArrayBuffer(8).slice.call)",
    "T(()=>{var s=new SharedArrayBuffer(8);return s.slice(1,3).slice(1).byteLength})", "T(()=>{var s=new SharedArrayBuffer(8);var r=s.slice();new Uint8Array(r)[0]=5;return new Uint8Array(s)[0]})",
    "T(()=>{var l=[];var s=new SharedArrayBuffer(8);s.slice({valueOf(){l.push('a');return 1}},{valueOf(){l.push('b');return 2}});return l})",
    "T(()=>new SharedArrayBuffer(0).slice(0).byteLength)", "T(()=>new SharedArrayBuffer(8).slice(Symbol()))", "T(()=>new SharedArrayBuffer(8).slice(1n))");

  // ---- 4. species.
  const speciesBodies = {
    undef: "undefined", nul: "null", sameSab: "function(){return s}", smaller: "function(n){return new SharedArrayBuffer(n-1)}",
    bigger: "function(n){return new SharedArrayBuffer(n+4)}", exact: "function(n){return new SharedArrayBuffer(n)}",
    plainAb: "function(n){return new ArrayBuffer(n)}", prim: "function(){return 1}", obj: "function(){return {}}", thrower: "function(){throw new EvalError('sp')}",
    growableRet: "function(n){return new SharedArrayBuffer(n,{maxByteLength:32})}", notCtor: "()=>1", num: "5", str: "'x'", ctorObj: "{}", symb: "Symbol()",
    arrowSpecies: "function(n){return new Proxy(new SharedArrayBuffer(n),{})}", asClass: "class extends SharedArrayBuffer{}",
    zero: "function(){return new SharedArrayBuffer(0)}", prelen: "function(n){var r=new SharedArrayBuffer(n);new Uint8Array(r).fill(7);return r}",
  };
  for (const [name, body] of Object.entries(speciesBodies)) {
    add(`T(()=>{var s=new SharedArrayBuffer(8);var lg=[];s.constructor={[Symbol.species]:${body}};var r=s.slice(2,6);return [r.byteLength,r===s,r instanceof SharedArrayBuffer,Array.from(new Uint8Array(r))]})`,
      `T(()=>{var s=new SharedArrayBuffer(8);s.constructor=${body};var r=s.slice(2,6);return [r.byteLength,r===s,Object.prototype.toString.call(r)]})`,
      `T(()=>{var s=new SharedArrayBuffer(8);Object.defineProperty(SharedArrayBuffer,Symbol.species,{value:${body},configurable:true});try{var r=s.slice(1,5);return [r.byteLength,r===s]}finally{Object.defineProperty(SharedArrayBuffer,Symbol.species,{get(){return this},configurable:true})}})`);
  }
  add("T(()=>SharedArrayBuffer[Symbol.species]===SharedArrayBuffer)", "T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer,Symbol.species).get.name)",
    "T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer,Symbol.species).set)", "T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer,Symbol.species).get.call(5))",
    "T(()=>{class X extends SharedArrayBuffer{}return [new X(8).slice(1,5) instanceof X,new X(8).slice(1,5).byteLength,X[Symbol.species]===X]})",
    "T(()=>{class X extends SharedArrayBuffer{static get [Symbol.species](){return SharedArrayBuffer}}var r=new X(8).slice(1,5);return [r instanceof X,r instanceof SharedArrayBuffer,r.byteLength]})",
    "T(()=>{class X extends SharedArrayBuffer{constructor(n){super(n+1)}}return [new X(8).byteLength,new X(8).slice(0,4).byteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8);var lg=[];s.constructor={get [Symbol.species](){lg.push('sp');return SharedArrayBuffer}};s.slice();return lg})",
    "T(()=>{var s=new SharedArrayBuffer(8);var lg=[];Object.defineProperty(s,'constructor',{get(){lg.push('ctor');return SharedArrayBuffer}});s.slice();return lg})",
    "T(()=>{var s=new SharedArrayBuffer(8);s.constructor=undefined;return s.slice(2).byteLength})",
    "T(()=>{var s=new SharedArrayBuffer(8);s.constructor=ArrayBuffer;return s.slice(2)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});s.constructor={[Symbol.species]:function(n){return new SharedArrayBuffer(n,{maxByteLength:16})}};var r=s.slice(1,3);return [r.growable,r.byteLength,r.maxByteLength]})");

  // ---- 5. Subclasses e byteLength.
  const subs = {
    plain: "class X extends SharedArrayBuffer{}", ctorSuper: "class X extends SharedArrayBuffer{constructor(){super(12)}}",
    ctorMax: "class X extends SharedArrayBuffer{constructor(n){super(n,{maxByteLength:32})}}", withField: "class X extends SharedArrayBuffer{f=1}",
    getterShadow: "class X extends SharedArrayBuffer{get byteLength(){return -1}}", ownLen: "class X extends SharedArrayBuffer{constructor(n){super(n);this.byteLength=5}}",
    methodShadow: "class X extends SharedArrayBuffer{slice(){return 'sliced'}}", tagShadow: "class X extends SharedArrayBuffer{get [Symbol.toStringTag](){return 'Xtag'}}",
    growShadow: "class X extends SharedArrayBuffer{grow(n){return super.grow(n+1)}}",
  };
  for (const [name, cls] of Object.entries(subs)) {
    const ctorArgs = name === "ctorMax" ? "8" : "8";
    add(`T(()=>{${cls};var x=new X(${ctorArgs});return [x.byteLength,x instanceof SharedArrayBuffer,Object.getPrototypeOf(x)===X.prototype,Object.prototype.toString.call(x),x.growable,x.maxByteLength]})`,
      `T(()=>{${cls};var x=new X(${ctorArgs});return [Object.getOwnPropertyNames(x),Reflect.ownKeys(X.prototype).length]})`,
      `T(()=>{${cls};var x=new X(${ctorArgs});var r=x.slice(1,3);return [typeof r==='string'?r:r.byteLength,r instanceof X]})`,
      `T(()=>{${cls};var x=new X(${ctorArgs});return Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'byteLength').get.call(x)})`,
      `T(()=>{${cls};return X.length+':'+X.name+':'+(Object.getPrototypeOf(X)===SharedArrayBuffer)})`,
      `T(()=>{${cls};var x=new X(${ctorArgs});try{return [x.grow(16),x.byteLength]}catch(e){return e.name+': '+e.message}})`,
      `T(()=>{${cls};return Reflect.construct(SharedArrayBuffer,[4],X).constructor===X})`,
      `T(()=>{${cls};var x=new X(${ctorArgs});return structuredClone(x) instanceof X})`);
  }
  add("T(()=>{class X extends SharedArrayBuffer{}return X()})", "T(()=>{class X extends SharedArrayBuffer{constructor(){}}return new X()})",
    "T(()=>Reflect.construct(SharedArrayBuffer,[8],Object).constructor===Object)", "T(()=>Reflect.construct(SharedArrayBuffer,[8],Array).byteLength)",
    "T(()=>Reflect.construct(SharedArrayBuffer,[8],ArrayBuffer) instanceof ArrayBuffer)", "T(()=>Object.getPrototypeOf(Reflect.construct(SharedArrayBuffer,[8],ArrayBuffer))===ArrayBuffer.prototype)",
    "T(()=>Reflect.construct(SharedArrayBuffer,[8],Object.assign(function(){},{prototype:null})) instanceof SharedArrayBuffer)",
    "T(()=>Reflect.construct(SharedArrayBuffer,[8],()=>1))", "T(()=>Reflect.construct(SharedArrayBuffer,[8],Object.assign(function(){},{prototype:Object.create(SharedArrayBuffer.prototype)})).byteLength)",
    "T(()=>{function F(){}F.prototype=Object.create(SharedArrayBuffer.prototype,{x:{value:1}});var o=Reflect.construct(SharedArrayBuffer,[2],F);return [o.x,o.byteLength,o.growable]})");

  // ---- 6. @@toStringTag, forma do construtor e do protótipo.
  add("T(()=>Object.prototype.toString.call(new SharedArrayBuffer(8)))", "T(()=>String(new SharedArrayBuffer(8)))", "T(()=>`${new SharedArrayBuffer(8)}`)",
    "T(()=>new SharedArrayBuffer(8)+'')", "T(()=>new SharedArrayBuffer(8)[Symbol.toStringTag])", "T(()=>SharedArrayBuffer.prototype[Symbol.toStringTag])",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,Symbol.toStringTag);return [d.value,d.writable,d.enumerable,d.configurable]})",
    "T(()=>Object.getOwnPropertyNames(SharedArrayBuffer.prototype))", "T(()=>Object.getOwnPropertySymbols(SharedArrayBuffer.prototype).map(String))",
    "T(()=>Object.getOwnPropertyNames(SharedArrayBuffer))", "T(()=>Object.getOwnPropertySymbols(SharedArrayBuffer).map(String))",
    "T(()=>[SharedArrayBuffer.length,SharedArrayBuffer.name,typeof SharedArrayBuffer])", "T(()=>Object.getPrototypeOf(SharedArrayBuffer)===Function.prototype)",
    "T(()=>Object.getPrototypeOf(SharedArrayBuffer.prototype)===Object.prototype)", "T(()=>SharedArrayBuffer.prototype.constructor===SharedArrayBuffer)",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer,'prototype');return [d.writable,d.enumerable,d.configurable]})",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'byteLength');return [typeof d.get,d.set,d.enumerable,d.configurable,d.get.name]})",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'growable');return [typeof d.get,d.set,d.enumerable,d.configurable,d.get.name]})",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'maxByteLength');return [typeof d.get,d.set,d.enumerable,d.configurable,d.get.name]})",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'slice');return [d.writable,d.enumerable,d.configurable,d.value.length]})",
    "T(()=>{var d=Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'grow');return [d.writable,d.enumerable,d.configurable,d.value.length,d.value.name]})",
    "T(()=>Object.keys(new SharedArrayBuffer(8)))", "T(()=>JSON.stringify(new SharedArrayBuffer(8)))", "T(()=>JSON.stringify({a:new SharedArrayBuffer(8,{maxByteLength:16})}))",
    "T(()=>Object.isFrozen(new SharedArrayBuffer(8)))", "T(()=>Object.isExtensible(new SharedArrayBuffer(8)))",
    "T(()=>{var s=new SharedArrayBuffer(8);s.x=1;return Object.keys(s)})", "T(()=>Object.freeze(new SharedArrayBuffer(8)).byteLength)",
    "T(()=>ArrayBuffer.isView(new SharedArrayBuffer(8)))", "T(()=>new SharedArrayBuffer(8) instanceof ArrayBuffer)", "T(()=>ArrayBuffer.prototype.isPrototypeOf(new SharedArrayBuffer(8)))",
    "T(()=>Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'resize'))", "T(()=>'resize' in SharedArrayBuffer.prototype)", "T(()=>'transfer' in SharedArrayBuffer.prototype)",
    "T(()=>'detached' in SharedArrayBuffer.prototype)", "T(()=>'isView' in SharedArrayBuffer)", "T(()=>SharedArrayBuffer.isView)", "T(()=>structuredClone(new SharedArrayBuffer(8,{maxByteLength:16})).growable)",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var c=structuredClone(s);s.grow(12);return [c.byteLength,c.maxByteLength,c===s]})",
    "T(()=>new SharedArrayBuffer(8).hasOwnProperty('byteLength'))", "T(()=>Object.prototype.hasOwnProperty.call(SharedArrayBuffer.prototype,'byteLength'))",
    "T(()=>Reflect.ownKeys(SharedArrayBuffer.prototype).length)", "T(()=>typeof globalThis.SharedArrayBuffer)", "T(()=>Object.getOwnPropertyDescriptor(globalThis,'SharedArrayBuffer').enumerable)");

  // ---- 7. Atomics.wait/notify/waitAsync nos caminhos que não bloqueiam.
  const types = { Int8Array: 1, Uint8Array: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, BigInt64Array: 8, BigUint64Array: 8, Float16Array: 2, Float32Array: 4, Float64Array: 8, Uint8ClampedArray: 1 };
  const backs = { shared: (b) => `new SharedArrayBuffer(${b})`, plain: (b) => `new ArrayBuffer(${b})`, growable: (b) => `new SharedArrayBuffer(${b},{maxByteLength:${b * 2}})`, resizable: (b) => `new ArrayBuffer(${b},{maxByteLength:${b * 2}})` };
  const zero = (C) => (C.startsWith("Big") ? "0n" : "0");
  const one = (C) => (C.startsWith("Big") ? "1n" : "1");
  for (const [C, sz] of Object.entries(types)) for (const [bn, bf] of Object.entries(backs)) {
    for (const op of ["wait", "waitAsync"]) {
      add(`T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.${op}(a,0,${one(C)},0)})`,
        `T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.${op}(a,0,${zero(C)},0)})`,
        `T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.${op}(a,4,${zero(C)},0)})`);
    }
    add(`T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.notify(a,0,1)})`, `T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.notify(a,0)})`,
      `T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.notify(a,4,1)})`, `T(()=>{var a=new ${C}(${bf(sz * 4)});return Atomics.notify(a)})`);
  }
  const idxs = ["0", "1", "3", "4", "-1", "'1'", "1.5", "NaN", "undefined", "Infinity", "2**32", "-0", "null", "true", "{valueOf(){return 2}}", "Symbol()", "1n", "'x'"];
  const vals = ["0", "1", "'0'", "undefined", "NaN", "2**32", "-4294967296", "{valueOf(){return 0}}", "null", "0.5", "Symbol()", "0n"];
  for (const i of idxs) for (const op of ["wait", "waitAsync"]) {
    add(`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),${i},0,0))`, `T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),${i},1,0))`,
      `T(()=>Atomics.${op}(new BigInt64Array(new SharedArrayBuffer(32)),${i},0n,0))`);
  }
  for (const i of idxs) add(`T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),${i},1))`, `T(()=>Atomics.notify(new BigInt64Array(new SharedArrayBuffer(32)),${i}))`,
    `T(()=>Atomics.notify(new Int32Array(new ArrayBuffer(16)),${i},1))`);
  for (const v of vals) for (const op of ["wait", "waitAsync"]) {
    add(`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),0,${v},0))`, `T(()=>Atomics.${op}(new BigInt64Array(new SharedArrayBuffer(32)),0,${v},0))`,
      `T(()=>{var a=new Int32Array(new SharedArrayBuffer(16));a[0]=1;return Atomics.${op}(a,0,${v},0)})`);
  }
  const touts = ["0", "-0", "-1", "-Infinity", "NaN", "'0'", "{valueOf(){return 0}}", "0.4", "1e-9", "false", "null", "Symbol()", "1n", "'x'", "[0]", "{}", "undefined", "Infinity", "2**53"];
  for (const t of touts) for (const op of ["wait", "waitAsync"]) {
    const safe = ["undefined", "Infinity", "2**53", "NaN", "{}", "'x'", "Infinity"].includes(t);
    // Timeout infinito só com valor diferente: nunca bloqueia.
    const val = safe ? "1" : "0";
    add(`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),0,${val},${t}))`, `T(()=>Atomics.${op}(new BigInt64Array(new SharedArrayBuffer(32)),0,${val}n,${t}))`);
  }
  for (const t of ["1", "2", "0.9", "'1'", "1.9"]) add(`T(()=>Atomics.wait(new Int32Array(new SharedArrayBuffer(16)),0,0,${t}))`, `T(()=>Atomics.wait(new BigInt64Array(new SharedArrayBuffer(16)),0,0n,${t}))`);
  const counts = ["undefined", "0", "1", "2", "Infinity", "-1", "'2'", "NaN", "-Infinity", "1.9", "2**32", "{valueOf(){return 1}}", "Symbol()", "1n", "null", "true"];
  for (const c of counts) add(`T(()=>Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),0,${c}))`, `T(()=>Atomics.notify(new BigInt64Array(new SharedArrayBuffer(16)),0,${c}))`,
    `T(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,0,0);return Atomics.notify(a,0,${c})})`,
    `T(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,1,0);return [Atomics.notify(a,0,${c}),Atomics.notify(a,1,${c}),Atomics.notify(a,0,${c})]})`);
  // Resultados de waitAsync: forma do objeto.
  add("T(()=>{var r=Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),0,1,0);return [Object.getPrototypeOf(r)===Object.prototype,Reflect.ownKeys(r),Object.getOwnPropertyDescriptor(r,'async'),Object.getOwnPropertyDescriptor(r,'value')]})",
    "T(()=>{var r=Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),0,0,0);return [Reflect.ownKeys(r),r.async,r.value]})",
    "T(()=>{var r=Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),0,0,10);return [Reflect.ownKeys(r),r.async,r.value instanceof Promise,Object.getPrototypeOf(r.value)===Promise.prototype]})",
    "T(()=>{var r=Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),0,0);var d=Object.getOwnPropertyDescriptor(r,'value');return [d.writable,d.enumerable,d.configurable]})",
    "T(()=>Atomics.waitAsync.length+':'+Atomics.waitAsync.name+':'+Atomics.wait.length+':'+Atomics.notify.length+':'+Atomics.notify.name)",
    "T(()=>[Object.getOwnPropertyDescriptor(Atomics,'wait').enumerable,Object.getOwnPropertyDescriptor(Atomics,'waitAsync').writable,Object.getOwnPropertyDescriptor(Atomics,'notify').configurable])",
    "T(()=>{var r=Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),0,0,10);return Object.prototype.toString.call(r.value)})",
    "T(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r1=Atomics.waitAsync(a,0,0,Infinity);var r2=Atomics.waitAsync(a,0,0,Infinity);return [r1.value===r2.value,r1.value instanceof Promise]})");
  // Visões diferentes sobre o mesmo SharedArrayBuffer e deslocamentos.
  add("T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b,4);var c=new Int32Array(b);Atomics.waitAsync(a,0,0);return [Atomics.notify(c,0),Atomics.notify(c,1),Atomics.notify(a,0)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b,8,2);var c=new Int32Array(b,0,4);Atomics.waitAsync(c,2,0);return [Atomics.notify(a,0),Atomics.notify(c,2)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b);var c=new Int16Array(b);Atomics.waitAsync(a,0,0);return [Atomics.notify(c,0),Atomics.notify(c,1),Atomics.notify(a,0)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new BigInt64Array(b);var c=new Int32Array(b);Atomics.waitAsync(a,0,0n);return [Atomics.notify(c,0),Atomics.notify(c,1),Atomics.notify(a,0)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b);var c=new Uint32Array(b);Atomics.waitAsync(a,1,0);return [Atomics.notify(c,1),Atomics.notify(a,1)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b);Atomics.waitAsync(a,0,0);return [Atomics.notify(new Int32Array(b),0),Atomics.notify(a,0)]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b);a[0]=5;var r=Atomics.waitAsync(a,0,5,0);Atomics.store(a,0,6);return [r.async,r.value,Atomics.waitAsync(a,0,5,0).value,Atomics.waitAsync(a,0,6,0).value]})",
    "T(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b,4,2);return [Atomics.wait(a,2,0,0)]})",
    "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);var r=[];try{Atomics.wait(a,3,0,0)}catch(e){r.push(e.name+': '+e.message)}b.grow(16);r.push(Atomics.wait(a,3,0,0),Atomics.wait(a,3,1,0));return r})",
    "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);var r=[];try{Atomics.waitAsync(a,3,0,0)}catch(e){r.push(e.name+': '+e.message)}b.grow(16);r.push(Atomics.waitAsync(a,3,0,0).value,Atomics.notify(a,3));return r})",
    "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);var w=Atomics.waitAsync(a,1,0);b.grow(16);return [w.async,Atomics.notify(a,1),Atomics.notify(a,3)]})",
    "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b,0,2);b.grow(16);return [Atomics.notify(a,1),Atomics.wait(a,1,0,0)]})");

  // ---- 8. Async: ordem de microtarefas com waitAsync/notify.
  const asyncProgs = [];
  asyncProgs.push(
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,1,0);L(r);Promise.resolve().then(()=>L('m1'));L('sync')})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,0);L(r);Promise.resolve().then(()=>L('m1'));L('sync')})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);r.value.then(v=>L('res:'+v));Promise.resolve().then(()=>L('m1')).then(()=>L('m2'));L('n='+Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);L('n='+Atomics.notify(a,0));r.value.then(v=>L('res:'+v));Promise.resolve().then(()=>L('m1'))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);(async()=>{L('before');var v=await r.value;L('after:'+v)})();L('n='+Atomics.notify(a,0));L('sync')})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));(async()=>{var r=Atomics.waitAsync(a,0,0);L(r.async);var v=await r.value;L(v)})();L('n='+Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));(async()=>{var r=Atomics.waitAsync(a,0,1,0);L(r.async+':'+r.value);var v=await r.value;L('v:'+v)})()})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);var p=r.value.then(v=>v+'!');L(p instanceof Promise);L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var ws=[0,1,2].map(i=>Atomics.waitAsync(a,0,0).value.then(v=>L('w'+i+':'+v)));L(Atomics.notify(a,0,2));L(Atomics.notify(a,0));L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0).value.then(v=>L('w0'));Atomics.waitAsync(a,1,0).value.then(v=>L('w1'));L(Atomics.notify(a,1));L(Atomics.notify(a,0));L(Atomics.notify(a,2))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var p=Atomics.waitAsync(a,0,0).value;Promise.race([p,Promise.resolve('race')]).then(v=>L('race:'+v));Atomics.notify(a,0)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var p=Atomics.waitAsync(a,0,0).value;Promise.all([p,1]).then(v=>L('all:'+v));Atomics.notify(a,0)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var p=Atomics.waitAsync(a,0,0).value;Promise.allSettled([p,Promise.reject(1)]).then(v=>L(v));Atomics.notify(a,0)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var p=Atomics.waitAsync(a,0,0).value;p.finally(()=>L('fin'));p.catch(()=>L('catch'));L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var p=Atomics.waitAsync(a,0,0).value;p.then(()=>L('t1'));p.then(()=>L('t2'));L(Atomics.notify(a,0));Promise.resolve().then(()=>L('mid'))})",
    "A(()=>{var a=new BigInt64Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0n);r.value.then(v=>L('res:'+v));L(Atomics.notify(a,0));Promise.resolve().then(()=>L('m1'))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,Infinity);r.value.then(v=>L('res:'+v));L(Atomics.notify(a,0,1));L(r.async)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,5000);r.value.then(v=>L('res:'+v));L(Atomics.notify(a,0));L(r.async)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);Atomics.store(a,0,9);L(Atomics.notify(a,0));L(Atomics.load(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);L(Atomics.add(a,0,1));L(Atomics.notify(a,0));r.value.then(v=>L('res:'+v))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);L(Atomics.notify(a,0));L(Atomics.notify(a,0));L(Atomics.waitAsync(a,0,0).async)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));L(Atomics.wait(a,0,1,0));L(Atomics.wait(a,0,0,0));L(Atomics.wait(a,0,0,1))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);var th={then(f){L('then called');f('thenable')}};r.value.then(()=>L('real'));Promise.resolve(th).then(v=>L(v));L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);Promise.resolve(r.value).then(v=>L('same:'+v));L(Promise.resolve(r.value)===r.value);L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);Object.defineProperty(r.value,'then',{value:function(f){L('own then');return Promise.prototype.then.call(this,f)}});(async()=>{await r.value;L('done')})();L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);r.value.then=function(){L('patched');};L(Atomics.notify(a,0));Promise.resolve(r.value).then(()=>L('x'))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var rs=[];for(var i=0;i<4;i++)rs.push(Atomics.waitAsync(a,i,0));L(rs.map(r=>r.async));L([0,1,2,3].map(i=>Atomics.notify(a,i)))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var rs=[];for(var i=0;i<4;i++)rs.push(Atomics.waitAsync(a,i,0));L(Atomics.notify(a,0,Infinity));L(Atomics.notify(a,1,0));L(Atomics.notify(a,3))})",
    "A(()=>{var b=new SharedArrayBuffer(16);var a=new Int32Array(b),c=new Int32Array(b);Atomics.waitAsync(a,2,0).value.then(()=>L('viaA'));L(Atomics.notify(c,2))})",
    "A(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);var w=Atomics.waitAsync(a,1,0);b.grow(16);L(w.async);L(Atomics.notify(a,1));L(Atomics.waitAsync(a,3,0).async);L(Atomics.notify(a,3))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var order=[];var p1=Atomics.waitAsync(a,0,0).value.then(()=>order.push(1));var p2=Atomics.waitAsync(a,0,0).value.then(()=>order.push(2));L(Atomics.notify(a,0,1));L(Atomics.notify(a,0,1));L(order)})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var f=async()=>{var r=Atomics.waitAsync(a,0,0);return r.value};var p=f();p.then(v=>L('f:'+v));L(Atomics.notify(a,0))})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));try{Atomics.waitAsync(new Float64Array(2),0,0,0)}catch(e){L(e.name)}try{Atomics.waitAsync(new Int8Array(new SharedArrayBuffer(4)),0,0,0)}catch(e){L(e.name)}L(Atomics.waitAsync(a,0,1,0).value)})",
    "A(()=>{var a=new Int32Array(new ArrayBuffer(16));try{Atomics.waitAsync(a,0,0,0)}catch(e){L(e.name+': '+e.message)}try{Atomics.wait(a,0,0,0)}catch(e){L(e.name+': '+e.message)}try{L(Atomics.notify(a,0))}catch(e){L(e.name+': '+e.message)}})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,0);var s=Atomics.waitAsync(a,0,0,-5);var t=Atomics.waitAsync(a,0,0,0.5);L([r,s,t])})",
    "A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Promise.reject(1).catch(()=>L('rej'));var r=Atomics.waitAsync(a,0,0);Promise.resolve().then(()=>{L('m1');L(Atomics.notify(a,0))}).then(()=>L('m2'))})"
  );
  for (const n of ["undefined", "0", "1", "2", "3", "Infinity"]) {
    asyncProgs.push(`A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));var ids=[0,1,2];ids.forEach(i=>Atomics.waitAsync(a,0,0).value.then(v=>L('r'+i+v)));L(Atomics.notify(a,0,${n}));Promise.resolve().then(()=>L('m1'))})`,
      `A(()=>{var a=new BigInt64Array(new SharedArrayBuffer(16));var ids=[0,1,2];ids.forEach(i=>Atomics.waitAsync(a,0,0n).value.then(v=>L('r'+i+v)));L(Atomics.notify(a,0,${n}));Promise.resolve().then(()=>L('m1'))})`);
  }
  for (const k of ["1", "2", "3", "5"]) {
    asyncProgs.push(`A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0).value.then(()=>L('w'));var p=Promise.resolve();for(var i=0;i<${k};i++)p=p.then(()=>L('t'+i));Atomics.notify(a,0)})`,
      `A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0).value.then(()=>L('w'));Atomics.notify(a,0);var p=Promise.resolve();for(var i=0;i<${k};i++)p=p.then(()=>L('t'+i))})`,
      `A(()=>{var a=new Int32Array(new SharedArrayBuffer(16));(async()=>{for(var i=0;i<${k};i++){L('loop'+i);await null}})();Atomics.waitAsync(a,0,0).value.then(()=>L('w'));L(Atomics.notify(a,0))})`);
  }
  for (const a of asyncProgs) add(a);

  // ---- 9. Erros de tipo e de faixa do Atomics.
  const ops = { add: "0,1", and: "0,1", compareExchange: "0,0,1", exchange: "0,1", isLockFree: "4", load: "0", notify: "0,1", or: "0,1", store: "0,1", sub: "0,1", wait: "0,0,0", waitAsync: "0,0,0", xor: "0,1" };
  const badArrays = {
    float64: "new Float64Array(new SharedArrayBuffer(32))", float32: "new Float32Array(new SharedArrayBuffer(16))", clamped: "new Uint8ClampedArray(new SharedArrayBuffer(8))",
    float16: "new Float16Array(new SharedArrayBuffer(8))", float64plain: "new Float64Array(4)", dataview: "new DataView(new SharedArrayBuffer(8))", array: "[0,0,0,0]",
    object: "({})", undef: "undefined", nul: "null", num: "5", sab: "new SharedArrayBuffer(8)", proxy: "new Proxy(new Int32Array(new SharedArrayBuffer(16)),{})", fakeTag: "({[Symbol.toStringTag]:'Int32Array',length:4})",
    detachedLike: "(()=>{var b=new ArrayBuffer(8);var a=new Int32Array(b);b.transfer();return a})()", str: "'abc'", buffer: "new ArrayBuffer(8)",
  };
  for (const [op, args] of Object.entries(ops)) {
    if (op === "isLockFree") continue;
    for (const [bn, arr] of Object.entries(badArrays)) add(`T(()=>Atomics.${op}(${arr},${args}))`);
    add(`T(()=>Atomics.${op}())`, ...(op === "wait" ? [] : [`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16))))`]), `T(()=>Atomics.${op}.call(null,new Int32Array(new SharedArrayBuffer(16)),${args}))`,
      `T(()=>new Atomics.${op}())`, `T(()=>Atomics.${op}.call(undefined,undefined,${args}))`);
  }
  const intIdx = ["-1", "-0", "4", "5", "100", "2**32", "2**53", "2**53-1", "Infinity", "-Infinity", "'x'", "NaN", "{}", "Symbol()", "1n", "4.5", "'4'", "3.9", "'-1'", "undefined"];
  for (const [op, args] of Object.entries(ops)) {
    if (op === "isLockFree") continue;
    const rest = args.split(",").slice(1).join(",");
    for (const i of intIdx) {
      const tail = rest ? "," + rest : "";
      add(`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),${i}${tail}))`);
      if (op !== "wait" && op !== "waitAsync" && op !== "notify") add(`T(()=>Atomics.${op}(new BigInt64Array(new SharedArrayBuffer(32)),${i}${tail.replace(/\b(\d+)\b/g, "$1n")}))`);
    }
  }
  const valueTypes = ["1n", "Symbol()", "{valueOf(){throw new RangeError('v')}}", "'x'", "NaN", "undefined", "null", "2**53", "-0", "{valueOf(){return 3}}", "'0x10'", "true", "[]", "1.9"];
  for (const op of ["add", "and", "exchange", "or", "store", "sub", "xor"]) for (const v of valueTypes) {
    add(`T(()=>Atomics.${op}(new Int32Array(new SharedArrayBuffer(16)),0,${v}))`);
    add(`T(()=>Atomics.${op}(new BigInt64Array(new SharedArrayBuffer(16)),0,${v}))`);
  }
  for (const v of valueTypes) add(`T(()=>Atomics.compareExchange(new Int32Array(new SharedArrayBuffer(16)),0,${v},1))`, `T(()=>Atomics.compareExchange(new Int32Array(new SharedArrayBuffer(16)),0,0,${v}))`,
    `T(()=>Atomics.compareExchange(new BigInt64Array(new SharedArrayBuffer(16)),0,${v},1n))`);
  // Ordem de validação: array, índice, valor.
  const arrs2 = { good: "new Int32Array(new SharedArrayBuffer(16))", bad: "new Float64Array(2)" };
  const idx2 = { good: "0", bad: "99", sym: "Symbol()" };
  const val2 = { good: "1", bad: "Symbol()", thrower: "{valueOf(){throw new EvalError('val')}}" };
  for (const [an, a] of Object.entries(arrs2)) for (const [in_, i] of Object.entries(idx2)) for (const [vn, v] of Object.entries(val2)) {
    for (const op of ["add", "store", "exchange", "compareExchange"]) {
      add(`T(()=>{var l=[];var idx={valueOf(){l.push('idx');return ${i === "Symbol()" ? "0" : i}}};var val={valueOf(){l.push('val');return 1}};try{return [Atomics.${op}(${a},${i === "Symbol()" ? "idx" : i},${v === "1" ? "val" : v}${op === "compareExchange" ? ",val" : ""}),l]}catch(e){return [e.name,l]}})`);
    }
  }
  for (const [an, a] of Object.entries(arrs2)) for (const t of ["{valueOf(){throw new EvalError('t')}}", "{valueOf(){return 0}}"]) {
    add(`T(()=>{var l=[];try{return [Atomics.wait(${a},{valueOf(){l.push('idx');return 0}},{valueOf(){l.push('val');return 0}},{valueOf(){l.push('time');return 0}}),l]}catch(e){return [e.name,l]}})`,
      `T(()=>{var l=[];try{return [Atomics.waitAsync(${a},{valueOf(){l.push('idx');return 0}},{valueOf(){l.push('val');return 1}},{valueOf(){l.push('time');return 0}}),l]}catch(e){return [e.name,l]}})`,
      `T(()=>{var l=[];try{return [Atomics.notify(${a},{valueOf(){l.push('idx');return 0}},{valueOf(){l.push('count');return 1}}),l]}catch(e){return [e.name,l]}})`,
      `T(()=>Atomics.wait(${a},0,0,${t}))`);
  }
  for (const n of ["1", "2", "3", "4", "8", "16", "0", "-1", "'4'", "NaN", "undefined", "3.9", "2**32", "{valueOf(){return 8}}", "Symbol()", "1n"]) add(`T(()=>Atomics.isLockFree(${n}))`);
  add("T(()=>Atomics.isLockFree())", "T(()=>[1,2,4,8,16,32,64].map(Atomics.isLockFree))", "T(()=>Atomics.pause)", "T(()=>typeof Atomics.pause)");
  // Atomics em views de SharedArrayBuffer redimensionável, depois do grow.
  for (const C of ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "BigInt64Array", "BigUint64Array"]) {
    const sz = types[C];
    const one_ = one(C);
    add(`T(()=>{var b=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 8}});var a=new ${C}(b);var r=[];try{Atomics.add(a,3,${one_})}catch(e){r.push(e.name+': '+e.message)}b.grow(${sz * 4});r.push(Atomics.add(a,3,${one_}),Atomics.load(a,3),a.length);return r})`,
      `T(()=>{var b=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 8}});var a=new ${C}(b,${sz},1);b.grow(${sz * 4});var r=[a.length];try{Atomics.add(a,1,${one_})}catch(e){r.push(e.name+': '+e.message)}return r})`,
      `T(()=>{var b=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 8}});var a=new ${C}(b);b.grow(${sz * 4});return [Atomics.store(a,3,${one_}),Atomics.exchange(a,3,${zero(C)}),Atomics.compareExchange(a,3,${zero(C)},${one_}),Atomics.load(a,3)]})`,
      `T(()=>{var b=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 8}});var a=new ${C}(b);b.grow(${sz * 4});return [Atomics.waitAsync(a,3,${zero(C)},0).async,Atomics.waitAsync(a,3,${one_},0).value,Atomics.wait(a,3,${one_},0),Atomics.notify(a,3)]})`);
  }

  // ---- 10. DataView sobre SharedArrayBuffer redimensionável.
  const dvOff = ["0", "2", "8"];
  const dvLen = ["undefined", "2", "4"];
  const growTo = ["8", "12", "16"];
  const dvIdx = ["0", "1", "6", "7", "8", "11", "15", "16", "-1"];
  for (const o of dvOff) for (const l of dvLen) for (const g of growTo) {
    const cons = l === "undefined" ? `new DataView(s,${o})` : `new DataView(s,${o},${l})`;
    add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{var d=${cons}}catch(e){return e.name+': '+e.message}var r=[d.byteLength,d.byteOffset];s.grow(${g});r.push(d.byteLength,d.byteOffset,d.buffer===s);return r})`);
    for (const i of dvIdx) {
      add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(s);for(var k=0;k<8;k++)u[k]=k+1;try{var d=${cons}}catch(e){return e.name+': '+e.message}s.grow(${g});var u2=new Uint8Array(s);for(var k=8;k<u2.length;k++)u2[k]=k+1;try{return d.getInt8(${i})}catch(e){return e.name+': '+e.message}})`,
        `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{var d=${cons}}catch(e){return e.name+': '+e.message}s.grow(${g});try{d.setUint16(${i},0xabcd);return Array.from(new Uint8Array(s))}catch(e){return e.name+': '+e.message}})`,
        `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{var d=${cons}}catch(e){return e.name+': '+e.message}var r=[];try{r.push(d.getFloat64(${i}))}catch(e){r.push(e.name+': '+e.message)}s.grow(${g});try{r.push(d.getFloat64(${i}))}catch(e){r.push(e.name+': '+e.message)}return r})`);
    }
  }
  for (const o of ["0", "1", "7", "8", "9", "12", "16", "17"]) for (const l of ["undefined", "0", "1", "8", "9", "16", "-1"]) {
    const cons = l === "undefined" ? `new DataView(s,${o})` : `new DataView(s,${o},${l})`;
    add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{var d=${cons};var r=[d.byteLength,d.byteOffset];s.grow(16);r.push(d.byteLength);return r}catch(e){return e.name+': '+e.message}})`,
      `T(()=>{var s=new SharedArrayBuffer(8);try{var d=${cons};return [d.byteLength,d.byteOffset]}catch(e){return e.name+': '+e.message}})`);
  }
  add("T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);var r=[];for(var g of [10,16])s.grow(g),r.push(d.byteLength);return r})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);s.grow(16);d.setFloat64(8,1.5);return [d.getFloat64(8),d.getUint8(8),d.getInt32(12)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,4);s.grow(16);d.setBigInt64(4,-2n,true);return [d.getBigInt64(4,true),d.getBigUint64(4,true),d.byteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,4,4);s.grow(16);try{d.setUint8(4,1)}catch(e){return [e.name,d.byteLength]}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);var dd=new DataView(s,0,8);s.grow(16);return [d.byteLength,dd.byteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,8);return [d.byteLength,d.byteOffset]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,8);try{d.getUint8(0)}catch(e){var m=e.name+': '+e.message}s.grow(9);return [m,d.getUint8(0),d.byteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);return Object.prototype.toString.call(d)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);return [d.buffer instanceof SharedArrayBuffer,d.buffer.growable]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);var g=Object.getOwnPropertyDescriptor(DataView.prototype,'byteLength').get;s.grow(16);return g.call(d)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,2);var g=Object.getOwnPropertyDescriptor(DataView.prototype,'byteOffset').get;s.grow(16);return g.call(d)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});return new DataView(s,0,undefined).byteLength})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);s.grow(16);var u=new Uint8Array(s);u.fill(255);return [d.getInt16(14),d.getUint32(12),d.getFloat32(12)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,0,4);s.grow(16);return [d.byteLength,d.getUint8(3)]})");
  for (const m of ["Int8", "Uint8", "Int16", "Uint16", "Int32", "Uint32", "Float32", "Float64", "BigInt64", "BigUint64", "Float16"]) {
    const big = m.startsWith("Big");
    const sz = { Int8: 1, Uint8: 1, Int16: 2, Uint16: 2, Int32: 4, Uint32: 4, Float32: 4, Float64: 8, BigInt64: 8, BigUint64: 8, Float16: 2 }[m];
    const v = big ? "5n" : "5";
    add(`T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s);var r=[];try{d.set${m}(8,${v})}catch(e){r.push(e.name+': '+e.message)}s.grow(16);d.set${m}(${16 - sz},${v});r.push(d.get${m}(${16 - sz}),d.get${m}(${16 - sz},true));try{d.get${m}(${17 - sz})}catch(e){r.push(e.name+': '+e.message)}return r})`,
      `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,4);s.grow(16);d.set${m}(${12 - sz},${v},true);return [Array.from(new Uint8Array(s,16-${sz})),d.byteLength]})`,
      `T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var d=new DataView(s,0,8);s.grow(16);try{d.get${m}(${9 - sz})}catch(e){return e.name+': '+e.message}return 'ok'})`);
  }

  // ---- 11. TypedArray sobre SharedArrayBuffer redimensionável depois de grow.
  const tCons = { Int8Array: 1, Uint8Array: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, Float32Array: 4, Float64Array: 8, BigInt64Array: 8, BigUint64Array: 8, Uint8ClampedArray: 1, Float16Array: 2 };
  const shapes = { tracking: "(s)", trackingOff: "(s,8)", trackingOff2: "(s,2)", fixed2: "(s,0,2)", fixed2off: "(s,8,2)", trackingEmpty: "(s,16)", fixedBig: "(s,0,4)" };
  for (const [C, sz] of Object.entries(tCons)) for (const [sh, args] of Object.entries(shapes)) for (const g of ["none", "12", "16", "24"]) {
    const growStmt = g === "none" ? "" : `try{s.grow(${g})}catch(e){}`;
    add(`T(()=>{var s=new SharedArrayBuffer(16,{maxByteLength:32});try{var a=new ${C}${args}}catch(e){return e.name+': '+e.message}${growStmt}return [a.length,a.byteLength,a.byteOffset,a.buffer===s,0 in a,5 in a,Object.keys(a).length]})`);
  }
  for (const [C, sz] of Object.entries(tCons)) {
    const val = C.startsWith("Big") ? "7n" : "7";
    add(`T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);a[0]=${val};s.grow(${sz * 4});a[3]=${val};var b=new ${C}(s);return [a.length,Array.from(a),b.length,a[3]===b[3],a[4],a.at(-1),Object.keys(a)]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);var it=a.entries();var r=[it.next().value,it.next().value];s.grow(${sz * 4});r.push(it.next().value,it.next().value,it.next().done);return r})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);var out=[];for(var x of a){out.push(Number(x));if(out.length===1)s.grow(${sz * 4})}return out})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);var out=[];a.forEach((x,i)=>{out.push(i);if(i===0)s.grow(${sz * 4})});return out})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 4});return [a.fill(${val}).length,a.indexOf(${val}),a.lastIndexOf(${val}),a.join('-'),a.includes(${val}),a.findLast(x=>true)==${val}]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 4});var m=a.map(x=>x);var sl=a.slice(1);var sub=a.subarray(1);var ss=a.subarray(1,3);s.grow(${sz * 6});return [m.length,sl.length,sub.length,ss.length,sub.buffer===s,sl.buffer===s,sub.byteOffset,m.buffer instanceof SharedArrayBuffer]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);var b=a.subarray(1);s.grow(${sz * 4});return [b.length,b.byteLength,a.length]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);var t=new ${C}(${sz * 0 + 2});t.fill(${val});s.grow(${sz * 3});a.set(t,1);return [Array.from(a).length,a[1]==${val},a[2]==${val}]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 3});try{a.set([1,2,3,4],0)}catch(e){return e.name+': '+e.message}return a.length})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 3});return [${C}.from(a).length,Array.from(a).length,[...a].length,new ${C}(a).length,new ${C}(a).buffer===s,a.toString().split(',').length]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 3});return [a.reverse().length,a.toReversed().length,a.toSorted().length,a.with(0,${val}).length,a.copyWithin(0,1).length,a.sort().length]})`,
      `T(()=>{var s=new SharedArrayBuffer(${sz * 2},{maxByteLength:${sz * 6}});var a=new ${C}(s);s.grow(${sz * 3});return [a.every(x=>true),a.some(x=>false),a.filter(x=>true).length,a.reduce((p,x)=>p+1,0),a.reduceRight((p,x)=>p+1,0),a.find(x=>false),a.findIndex(x=>false),a.findLastIndex(x=>false),a.keys().next().value,[...a.keys()].length,[...a.values()].length,[...a.entries()].length]})`);
  }
  add("T(()=>{var s=new SharedArrayBuffer(7,{maxByteLength:16});var a=new Int32Array(s);var r=[a.length];s.grow(11);r.push(a.length);s.grow(12);r.push(a.length);return r})",
    "T(()=>{var s=new SharedArrayBuffer(7,{maxByteLength:16});var a=new Int16Array(s,1);var r=[a.length,a.byteOffset];s.grow(12);r.push(a.length);return r})",
    "T(()=>{var s=new SharedArrayBuffer(7,{maxByteLength:16});try{return new Int32Array(s,0,2).length}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(7,{maxByteLength:16});try{var a=new Int32Array(s,0,2);return a.length}catch(e){var m=e.name+': '+e.message}s.grow(8);return [m,new Int32Array(s,0,2).length]})",
    "T(()=>{var s=new SharedArrayBuffer(7,{maxByteLength:16});try{return new Int16Array(s,1,3).length}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{return new Int32Array(s,3).length}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{return new Int32Array(s,9).length}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});try{return new Int32Array(s,8).length}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return [Object.isFrozen(a),Object.isSealed(a),Object.isExtensible(a)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);try{Object.freeze(a)}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);try{Object.seal(a);return 'sealed'}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);try{Object.preventExtensions(a);return 'ok'}catch(e){return e.name+': '+e.message}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);Object.preventExtensions(a);s.grow(16);return [a.length,Object.isFrozen(a)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return [Object.getOwnPropertyDescriptor(a,0),Object.getOwnPropertyDescriptor(a,8)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return [Object.getOwnPropertyDescriptor(a,8),Reflect.ownKeys(a).length,Reflect.has(a,15),Reflect.has(a,16),Reflect.set(a,15,3),Reflect.set(a,16,3),a[15]]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return [Reflect.defineProperty(a,15,{value:2}),Reflect.defineProperty(a,16,{value:2}),Reflect.deleteProperty(a,15),Reflect.deleteProperty(a,16)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);try{a.length=5;return a.length}catch(e){return e.name}})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return [Object.getOwnPropertyNames(a).length,JSON.stringify(a),JSON.stringify(Object.assign({},a))]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(12);return [JSON.stringify(a),Object.entries(a).length,Object.values(a).length,Object.getOwnPropertyNames(a).length]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);var b=new Uint16Array(s);s.grow(16);return [a.length,b.length,b.byteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);var b=new Uint8Array(a);s.grow(16);return [a.length,b.length,b.buffer===s]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);var b=new Uint8Array(a.buffer,2);s.grow(16);return [b.length,b.byteOffset]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),'buffer').get.call(a)===s})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return Object.prototype.toString.call(a.buffer)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return [a.buffer.growable,a.buffer.maxByteLength]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s,0,4);s.grow(16);return [a.length,a.byteLength,Object.keys(a)]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);a.set([1,2,3],2);s.grow(16);return [Array.from(a.subarray(0,4)),Array.from(a.slice(-3))]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return structuredClone(a).buffer.growable})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);var c=structuredClone(a);s.grow(16);return [c.length,a.length]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int8Array(s);s.grow(16);a.fill(-1,8);return Array.from(new Uint8Array(s)).join()})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return Array.prototype.map.call(a,x=>x+1).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return Array.prototype.slice.call(a,6).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return [].concat(a).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return Array.prototype.concat.call([],Object.assign(a,{[Symbol.isConcatSpreadable]:true})).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);s.grow(16);return [a.toLocaleString().split(',').length,String(a).split(',').length]})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return Atomics.load(a,7)+Atomics.add(a,7,1)})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.sort((x,y)=>{s.grow(16);return 0}).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.map((x,i)=>{if(i===0)s.grow(16);return i}).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);var n=0;a.forEach(()=>{n++;s.grow(16)});return n})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.fill({valueOf(){s.grow(16);return 3}}).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);a.fill(1);return a.indexOf(1,{valueOf(){s.grow(16);return 0}})})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.subarray({valueOf(){s.grow(16);return 1}}).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.slice(0,{valueOf(){s.grow(16);return 12}}).length})",
    "T(()=>{var s=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(s);return a.at({valueOf(){s.grow(16);return -1}})})");

  // ---- Execução: cada programa num bun filho novo.
  const baseSources = [];
  for (const file of ["atomics_bun.tsv", "buffer_bun.tsv", "buffer_edge_bun.tsv", "buffers_bun.tsv", "typedarray_bun.tsv", "typedarray_edge_bun.tsv", "typedarray_more_bun.tsv", "typedarray_proto_bun.tsv"]) {
    try {
      baseSources.push(fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8"));
    } catch (e) {}
  }
  const baseText = baseSources.join("\n");
  const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
  exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
  // Amostra determinística por hash (sampleByHash) do conjunto inteiro, antes de descontar os goldens vizinhos (o prelúdio pesa em cada linha).
  const TARGET = 2400;
  const all = exprs.filter((e) => !seen.has(e) && seen.add(e));
  const unique = sampleByHash(all, TARGET);
  let kept = 0;
  let dropped = 0;
  let dup = 0;
  const todo = [];
  for (const expr of unique) {
    if (expr.length > 24 && baseText.includes(JSON.stringify(expr).slice(1, -1))) {
      dup++;
      continue;
    }
    if (/[\n\t]/.test(expr)) throw new Error("fonte com tab ou quebra de linha: " + expr);
    todo.push({ expr, source: '"use strict";\n' + PRELUDE + (/^A\(/.test(expr) ? expr + ";" : `globalThis.R = ${expr};`) });
  }
  process.stderr.write(`${todo.length} programas a medir de ${all.length}\n`);
  const results = new Array(todo.length);
  const runOne = (item) =>
    new Promise((resolve) => {
      const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
      let stdout = "";
      const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
      child.stdout.on("data", (d) => (stdout += d));
      child.on("close", (code) => {
        clearTimeout(timer);
        resolve(code === 0 ? decodeResult(stdout) : null);
      });
      child.stdin.end(item.source);
    });
  let next = 0;
  const worker = async () => {
    while (next < todo.length) {
      const i = next++;
      results[i] = await runOne(todo[i]);
    }
  };
  await Promise.all(Array.from({ length: 12 }, worker));
  const out = [];
  todo.forEach((item, i) => {
    const result = results[i];
    if (result === null) {
      dropped++;
      process.stderr.write("erro de programa: " + JSON.stringify(item.expr).slice(0, 160) + "\n");
      return;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || result === "pending") {
      dropped++;
      process.stderr.write("caminho, marca ou resultado inválido: " + JSON.stringify(item.expr).slice(0, 160) + " -> " + result.slice(0, 80) + "\n");
      return;
    }
    kept++;
    out.push(JSON.stringify(item.source) + "\t" + JSON.stringify(result));
  });
  process.stdout.write(emitFactoredLines("sab", out));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
