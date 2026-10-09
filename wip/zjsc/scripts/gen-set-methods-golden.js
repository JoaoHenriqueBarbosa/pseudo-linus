// Gera tests/golden/set_methods_bun.tsv: métodos de conjunto de Set (union, intersection, difference, symmetricDifference,
// isSubsetOf, isSupersetOf, isDisjointFrom) e Map.groupBy/Object.groupBy, medidos no bun 1.4.2.
// Cobre: grade de receptores Set de tamanhos 0..4 contra argumentos Set, Map e set-like com log de chamadas (size, has,
// keys, next, return), `size` NaN/negativo/string/getter que lança, `has` que não é função, `keys` devolvendo iterador que
// lança ou devolve não-objeto, ordem de leitura das propriedades do set-like, receptores e argumentos inválidos, subclasses
// de Set (species e métodos sobrescritos nunca consultados pelo receptor), mutação do receptor durante size/keys/next/has,
// ordem dos elementos no resultado e mensagens exatas de TypeError/RangeError; groupBy com chaves exóticas, callbacks que
// lançam e iteradores fechados.
// Formato: `JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice]`, prelúdio fatorado (ver golden-prelude.js). Um bun filho novo por
// programa; o programa grava o texto em `globalThis.R`.
// Uso: bun scripts/gen-set-methods-golden.js > tests/golden/set_methods_bun.tsv
const { emitFactored, knownPrograms, GOLDEN_DIR, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  let source = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (d) => (source += d));
  process.stdin.on("end", () => {
    (0, eval)(source);
    process.exit(0);
  });
  return;
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function V(x){return x instanceof Set?"Set"+S(Array.from(Set.prototype.values.call(x))):x instanceof Map?"Map"+S(Array.from(Map.prototype.entries.call(x))):S(x)}\n' +
  'function E(m,meth,a){var r=m[meth](a);return V(r)+"|"+(r===m)+"|"+(r instanceof Set&&Object.getPrototypeOf(r)===Set.prototype)+"|"+S(a&&a.log)+"|"+V(m)}\n' +
  'function SL(items,o){o=o||{};var h=o.hook||{};var log=[];var r={log:log};' +
  'Object.defineProperty(r,"size",{enumerable:true,configurable:true,get(){log.push("size");if(h.size)h.size();if("size" in o){var z=o.size;return typeof z==="function"?z():z}return items.length}});' +
  'r.has=function(v){log.push("has:"+S(v)+(this===r?"":"!"));if(h.has)h.has(v);if(o.hasF)return o.hasF.call(this,v,log);return items.includes(v)};' +
  'r.keys=function(){log.push("keys"+(this===r?"":"!"));if(h.keys)h.keys();var i=0;return {next(){log.push("next");if(h.next)h.next();if(o.nextF)return o.nextF(i++,items,log);if(i<items.length)return {done:false,value:items[i++]};return {done:true,value:undefined}},return(){log.push("return");return {}}}};' +
  'if("has" in o)r.has=o.has;if("keys" in o)r.keys=o.keys;return r}\n' +
  'var SUB=class extends Set{constructor(i){super(i);K.push("ctor")}add(v){K.push("add");return super.add(v)}has(v){K.push("has:"+S(v));return super.has(v)}get size(){K.push("size");return super.size}keys(){K.push("keys");return super.keys()}delete(v){K.push("delete");return super.delete(v)}static get [Symbol.species](){K.push("species");return Set}};var K=[];\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const METHODS = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
const prog = (recv, arg, meth, pre = "") => `T(()=>{${pre}var m=new Set(${recv});var a=${arg};return E(m,'${meth}',a)})`;

// ---- 1. Grade: receptor 0..4 e exóticos, contra Set, Map e set-like com log.
const receivers = ["[]", "[1]", "[1,2]", "[1,2,3]", "[1,2,3,4]", "[0,NaN,'a']", "[-0]", "[3,1,2]"];
const argLists = ["[]", "[1]", "[2]", "[5]", "[1,2]", "[2,5]", "[4,3,2,1]", "[1,2,3,4,5]", "[NaN,0]", "[-0,'a']", "[5,6,7,8]"];
for (const meth of METHODS) {
  for (const recv of receivers) {
    for (const list of argLists) {
      add(
        prog(recv, `new Set(${list})`, meth),
        prog(recv, `new Map(${list}.map(x=>[x,'v']))`, meth),
        prog(recv, `SL(${list})`, meth),
      );
    }
  }
}

// ---- 2. size: valores exóticos.
const sizes = [
  "NaN", "-1", "'3'", "''", "'abc'", "undefined", "null", "1.5", "-0", "0", "Infinity", "-Infinity", "2**53", "true", "false",
  "{valueOf(){return 2}}", "{valueOf(){return -2}}", "{valueOf(){throw new RangeError('vo')}}", "{valueOf(){return {}}}",
  "Symbol()", "1n", "()=>1", "[]", "[5]", "{}", "'  2  '", "'0x2'", "2", "3", "0.5", "-0.5", "1e400", "{toString(){return '2'}}",
  "-1e-9", "4294967296", "new Number(2)", "new String('2')",
];
for (const meth of METHODS) {
  for (const size of sizes) {
    for (const recv of ["[]", "[1,2]", "[1,2,3]"]) {
      add(prog(recv, `SL([2,3],{size:${size}})`, meth));
    }
  }
  add(
    prog("[1,2]", "SL([2,3],{size(){throw new RangeError('sz')}})", meth),
    prog("[1,2]", "SL([2,3],{size:function(){throw new TypeError('sz')}})", meth),
    prog("[1,2]", "SL([2,3],{size:()=>{throw 7}})", meth),
  );
}

// ---- 3. has e keys: valores e retornos exóticos, em três tamanhos relativos ao receptor [1,2,3].
const hasVariants = [
  "undefined", "null", "1", "'f'", "{}", "[]", "Symbol()", "class{}", "function(){}", "async()=>1", "function*(){}", "()=>true", "()=>false",
  "()=>1", "()=>0", "()=>''", "()=>'x'", "()=>undefined", "()=>({})", "()=>{throw new RangeError('has')}", "(v)=>v===2", "(v)=>v>1",
  "new Proxy(function(){return true},{})", "function(){return this===undefined}", "function(){return arguments.length===1}",
];
const keysVariants = [
  "undefined", "null", "1", "'k'", "{}", "[]", "()=>undefined", "()=>null", "()=>1", "()=>'ab'", "()=>({})", "()=>[]",
  "()=>[2,3]", "()=>[2,3][Symbol.iterator]()", "()=>({next:undefined})", "()=>({next:1})", "()=>({next:null})",
  "()=>({next(){throw new RangeError('nx')}})", "()=>({next(){return 1}})", "()=>({next(){return null}})",
  "()=>({next(){return 'x'}})", "()=>({next(){return undefined}})", "()=>({next(){return {done:true}}})",
  "()=>({next(){return {done:1,value:2}}})", "()=>({next(){return {done:0,value:2}}})", "()=>({next(){return {done:false}}})",
  "()=>({next(){return {done:false,value:2}},return(){return 1}})",
  "()=>({next(){return {get done(){throw new RangeError('dn')}}}})",
  "()=>({next(){return {done:false,get value(){throw new RangeError('vl')}}}})",
  "()=>({next(){throw 5}})", "function*(){yield 2;yield 3}", "function*(){yield 2;throw new RangeError('gen')}",
  "function*(){yield 2;yield 2;yield 3}", "function*(){yield 9;yield 8}", "function*(){}",
  "function*(){yield NaN;yield -0}", "()=>(function*(){yield 2})()", "()=>new Set([2,3]).values()", "()=>new Map([[2,3]]).keys()",
  "()=>new Map([[2,3]]).values()", "()=>'23'[Symbol.iterator]()", "()=>({[Symbol.iterator](){return [2]}})",
  "()=>({next(){return {done:false,value:2}}})", "function(){return this}", "()=>new Proxy({next(){return {done:true}}},{})",
];
for (const meth of METHODS) {
  for (const size of ["0", "2", "5"]) {
    for (const recv of ["[]", "[1,2,3]"]) {
      for (const hv of hasVariants) add(prog(recv, `SL([2,3],{size:${size},has:${hv}})`, meth));
      for (const kv of keysVariants) add(prog(recv, `SL([2,3],{size:${size},keys:${kv}})`, meth));
    }
  }
}

// ---- 4. Argumentos que não são set-like.
const badArgs = [
  "undefined", "null", "1", "'ab'", "true", "Symbol()", "1n", "[]", "[1,2]", "function(){}", "()=>1", "class{}", "{}", "{size:1}",
  "{size:1,has(){}}", "{size:1,keys(){}}", "{has(){},keys(){}}", "new WeakSet", "new WeakMap", "new Proxy(new Set([1]),{})",
  "new Proxy({size:1,has(){return true},keys(){return [][Symbol.iterator]()}},{})", "Object.create(Set.prototype)",
  "Object.create(new Set([1]))", "new Set", "new Map", "new Uint8Array(2)", "{get size(){return 1},has:1,keys:2}",
  "new String('ab')", "Object(1)", "arguments", "Set.prototype", "Map.prototype", "globalThis",
];
for (const meth of METHODS) for (const arg of badArgs) for (const recv of ["[]", "[1,2]"]) add(prog(recv, arg, meth));
for (const meth of METHODS) {
  add(
    `T(()=>new Set([1,2])['${meth}']())`, `T(()=>new Set([1,2])['${meth}'](undefined))`,
    `T(()=>Set.prototype['${meth}'].length+Set.prototype['${meth}'].name)`,
    `T(()=>S(Object.getOwnPropertyDescriptor(Set.prototype,'${meth}')&&(function(d){return [typeof d.value,d.writable,d.enumerable,d.configurable]})(Object.getOwnPropertyDescriptor(Set.prototype,'${meth}'))))`,
    `T(()=>Object.getPrototypeOf(Set.prototype['${meth}'])===Function.prototype)`,
    `T(()=>Set.prototype['${meth}'].hasOwnProperty('prototype'))`,
    `T(()=>new Set.prototype['${meth}'](new Set))`,
    `T(()=>Object.getOwnPropertyNames(Set.prototype).indexOf('${meth}')>=0)`,
    `T(()=>Set.prototype['${meth}'].toString().replace(/\\s+/g,' '))`,
  );
  for (const r of ["null", "undefined", "1", "'s'", "{}", "[]", "new Map", "new WeakSet", "Set.prototype", "Object.create(Set.prototype)",
    "new Proxy(new Set,{})", "function(){}", "Symbol()", "new Map([[1,1]])", "Object.assign(new Set([1]),{size:3})"]) {
    add(
      `T(()=>V(Set.prototype['${meth}'].call(${r},new Set([1]))))`,
      `T(()=>V(Set.prototype['${meth}'].call(${r},SL([1]))))`,
      `T(()=>V(Set.prototype['${meth}'].call(${r})))`,
    );
  }
  // Receptor com métodos próprios e prototype trocado.
  add(
    `T(()=>{var m=new Set([1,2]);m.has=()=>{throw 1};m.keys=()=>{throw 2};m.add=()=>{throw 3};Object.defineProperty(m,'size',{value:99});return E(m,'${meth}',new Set([2,3]))})`,
    `T(()=>{var m=new Set([1,2]);Object.setPrototypeOf(m,null);return V(Set.prototype['${meth}'].call(m,new Set([2,3])))})`,
    `T(()=>{var m=new Set([1,2]);Object.setPrototypeOf(m,Map.prototype);return V(Set.prototype['${meth}'].call(m,new Set([2,3])))})`,
    `T(()=>{var m=new Set([1,2]);var orig=Set.prototype.add;var r=Set.prototype['${meth}'].call(m,new Set([2,3]));return V(r)})`,
    `T(()=>{var o=Set.prototype.add;var n=0;Set.prototype.add=function(v){n++;return o.call(this,v)};try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Set.prototype.add=o}return n})`,
    `T(()=>{var o=Set.prototype.has;var n=0;Set.prototype.has=function(v){n++;return o.call(this,v)};try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Set.prototype.has=o}return n})`,
    `T(()=>{var o=Set.prototype.keys;var n=0;Set.prototype.keys=function(){n++;return o.call(this)};try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Set.prototype.keys=o}return n})`,
    `T(()=>{var o=Set.prototype[Symbol.iterator];var n=0;Set.prototype[Symbol.iterator]=function(){n++;return o.call(this)};try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Set.prototype[Symbol.iterator]=o}return n})`,
    `T(()=>{var o=Set.prototype.delete;var n=0;Set.prototype.delete=function(v){n++;return o.call(this,v)};try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Set.prototype.delete=o}return n})`,
    `T(()=>{var o=Object.getOwnPropertyDescriptor(Set.prototype,'size');Object.defineProperty(Set.prototype,'size',{get(){throw new RangeError('szp')},configurable:true});try{return V(Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3])))}finally{Object.defineProperty(Set.prototype,'size',o)}})`,
    `T(()=>{var o=Object.getOwnPropertyDescriptor(Set.prototype,'size');var n=0;Object.defineProperty(Set.prototype,'size',{get(){n++;return o.get.call(this)},configurable:true});try{Set.prototype['${meth}'].call(new Set([1,2]),new Set([2,3]))}finally{Object.defineProperty(Set.prototype,'size',o)}return n})`,
  );
}

// ---- 5. Ordem de leitura das propriedades size, has e keys do set-like (cada uma boa ou ruim).
const sizeSlots = { ok: "return 2", nan: "return NaN", throws: "throw new RangeError('sz')", neg: "return -1", str: "return 'a'" };
const hasSlots = { ok: "return function(v){return v===2}", undef: "return undefined", throws: "throw new RangeError('hs')", num: "return 1", nul: "return null" };
const keysSlots = { ok: "return function(){return [2,3][Symbol.iterator]()}", undef: "return undefined", throws: "throw new RangeError('ky')", num: "return 1", nul: "return null" };
for (const meth of METHODS) {
  for (const [sn, sb] of Object.entries(sizeSlots)) {
    for (const [hn, hb] of Object.entries(hasSlots)) {
      for (const [kn, kb] of Object.entries(keysSlots)) {
        add(`T(()=>{var log=[];var a={get size(){log.push('size');${sb}},get has(){log.push('has');${hb}},get keys(){log.push('keys');${kb}}};var m=new Set([1,2]);var r;try{r=V(m[${JSON.stringify(meth)}](a))}catch(e){r=e.name+': '+e.message}return r+'|'+log.join()})`);
      }
    }
  }
}

// ---- 6. Subclasses de Set como receptor e como argumento.
for (const meth of METHODS) {
  for (const recv of ["[]", "[1,2]", "[1,2,3]", "[3,2,1]"]) {
    for (const arg of ["new Set([2,3])", "new Map([[2,1],[3,1]])", "SL([2,3])", "SL([2,3,4,5])", "new Set"]) {
      add(
        `T(()=>{K.length=0;var m=new SUB(${recv});K.length=0;var a=${arg};var r=m['${meth}'](a);return V(r)+'|'+(Object.getPrototypeOf(r)===Set.prototype)+'|'+(r instanceof SUB)+'|'+S(K)+'|'+S(a&&a.log)})`,
      );
    }
    for (const items of ["[]", "[2]", "[2,3]", "[2,3,4,5]", "[1,2,3]"]) {
      add(
        `T(()=>{K.length=0;var a=new SUB(${items});K.length=0;var m=new Set(${recv});var r=m['${meth}'](a);return V(r)+'|'+(Object.getPrototypeOf(r)===Set.prototype)+'|'+S(K)})`,
      );
    }
  }
  add(
    `T(()=>{class X extends Set{get size(){return 1}};return E(new Set([1,2,3]),'${meth}',new X([5,6,7]))})`,
    `T(()=>{class X extends Set{has(){return true}};return E(new Set([1,2,3]),'${meth}',new X([5,6,7]))})`,
    `T(()=>{class X extends Set{keys(){return [1,2,9][Symbol.iterator]()}};return E(new Set([1,2,3]),'${meth}',new X([5,6,7]))})`,
    `T(()=>{class X extends Set{keys(){return [1,2,9][Symbol.iterator]()}};return E(new X([1,2,3]),'${meth}',new Set([2,9]))})`,
    `T(()=>{class X extends Set{};var m=new X([1,2,3]);var r=m['${meth}'](new Set([2,9]));return V(r)+(r.constructor===Set)+(r instanceof X)})`,
    `T(()=>{class X extends Set{static get [Symbol.species](){throw new RangeError('species')}};return E(new X([1,2,3]),'${meth}',new Set([2,9]))})`,
    `T(()=>{class X extends Set{constructor(){throw new RangeError('ctor')}};var m=Reflect.construct(Set,[[1,2,3]],X);return E(m,'${meth}',new Set([2,9]))})`,
    `T(()=>{var m=new Set([1,2,3]);Object.defineProperty(m,'constructor',{get(){throw new RangeError('c')}});return E(m,'${meth}',new Set([2,9]))})`,
    `T(()=>{var a=new Set([2,9]);Object.defineProperty(a,'size',{value:'x'});return E(new Set([1,2,3]),'${meth}',a)})`,
    `T(()=>{var a=new Set([2,9]);a.has=()=>true;a.keys=()=>[7][Symbol.iterator]();a.size=1;return E(new Set([1,2,3]),'${meth}',a)})`,
    `T(()=>{var a=new Map([[2,'a'],[9,'b']]);a.has=()=>true;a.keys=()=>[7][Symbol.iterator]();return E(new Set([1,2,3]),'${meth}',a)})`,
    `T(()=>{class M extends Map{keys(){return [9,2,1][Symbol.iterator]()}};return E(new Set([1,2,3]),'${meth}',new M([[2,1],[4,1],[5,1]]))})`,
    `T(()=>{class M extends Map{get size(){return 2}};return E(new Set([1,2,3]),'${meth}',new M([[2,1],[4,1],[5,1],[6,1]]))})`,
    `T(()=>{var a=new Map([[NaN,1],[-0,2]]);return E(new Set([0,NaN,1]),'${meth}',a)})`,
    `T(()=>{var a=new Map([[1,1],[1.0,2]]);return E(new Set([1,2]),'${meth}',a)})`,
  );
}

// ---- 7. Mutação do receptor durante size, keys, next e has do argumento.
const mutations = [
  "m.delete(1)", "m.delete(2)", "m.delete(3)", "m.clear()", "m.add(9)", "m.delete(1),m.add(1)", "m.add(3)", "m.delete(2),m.add(2)",
  "m.clear(),m.add(5)", "m.delete(9)",
];
for (const meth of METHODS) {
  for (const mut of mutations) {
    for (const hook of ["size", "keys", "next", "has"]) {
      for (const [items, size] of [["[2,3,4]", "3"], ["[2]", "1"], ["[2,3,4,5]", "6"]]) {
        add(`T(()=>{var m=new Set([1,2,3]);var a=SL(${items},{size:${size},hook:{${hook}:()=>{${mut}}}});return E(m,'${meth}',a)})`);
      }
    }
  }
  for (const mut of mutations.slice(0, 6)) {
    add(
      `T(()=>{var m=new Set([1,2,3]);var n=0;var a=SL([1,2,3,4],{size:4,hook:{next:()=>{if(n++===1){${mut}}}}});return E(m,'${meth}',a)})`,
      `T(()=>{var m=new Set([1,2,3]);var a=new Set([2,3,4]);Object.defineProperty(a,'has',{value(v){${mut};return Set.prototype.has.call(this,v)}});return E(m,'${meth}',a)})`,
      `T(()=>{var m=new Set([1,2,3]);var a=new Set([2,3,4,5,6]);Object.defineProperty(a,'keys',{value(){${mut};return Set.prototype.keys.call(this)}});return E(m,'${meth}',a)})`,
      `T(()=>{var m=new Set([1,2,3]);var a=new Set([2]);Object.defineProperty(a,'size',{get(){${mut};return 1}});return E(m,'${meth}',a)})`,
      `T(()=>{var m=new Set([1,2,3]);var a=new Map([[2,1]]);Object.defineProperty(a,'has',{value(v){${mut};return Map.prototype.has.call(this,v)}});return E(m,'${meth}',a)})`,
    );
  }
  // Mutação do argumento durante a operação (argumento Set real iterado por keys).
  add(
    `T(()=>{var m=new Set([1,2,3]);var a=new Set([2,3,4,5,6]);var o=a.keys;Object.defineProperty(a,'keys',{value(){var it=o.call(this);return {next(){var r=it.next();a.add(99);return r}}}});return E(m,'${meth}',a)})`,
    `T(()=>{var m=new Set([1,2,3]);var a=new Set([2,3,4,5,6]);var o=a.keys;Object.defineProperty(a,'keys',{value(){var it=o.call(this);return {next(){var r=it.next();a.delete(3);return r}}}});return E(m,'${meth}',a)})`,
    `T(()=>{var m=new Set([1,2,3]);return V(m['${meth}'](m))+'|'+V(m)}`.replace(/\}$/, "})"),
    `T(()=>{var m=new Set([1,2,3]);return V(m['${meth}'](SL([1,2,3],{hasF(v){return m.has(v)}})))})`,
    `T(()=>{var m=new Set;return V(m['${meth}'](m))})`,
  );
}

// ---- 8. Conjuntos maiores e ordem dos elementos.
const range = (n, from = 0) => `[${Array.from({ length: n }, (_, i) => from + i).join(",")}]`;
for (const meth of METHODS) {
  for (const [rn, rf, an, af] of [[6, 0, 3, 2], [6, 0, 6, 0], [6, 0, 9, 3], [6, 0, 6, 3], [8, 0, 2, 5], [3, 0, 9, 0], [10, 0, 10, 5], [12, 0, 4, 0], [0, 0, 5, 0], [5, 0, 0, 0]]) {
    add(
      prog(range(rn, rf), `new Set(${range(an, af)})`, meth),
      prog(range(rn, rf), `SL(${range(an, af)})`, meth),
      prog(range(rn, rf), `SL(${range(an, af)}.reverse())`, meth),
      prog(range(rn, rf), `new Map(${range(an, af)}.map(x=>[x,x]))`, meth),
    );
  }
  add(
    prog("['b','a','c']", "new Set(['c','d','a','e'])", meth),
    prog("['b','a','c']", "SL(['c','d','a','e'])", meth),
    prog("[3,1,2]", "SL([2,1,5,4],{size:Infinity})", meth),
    prog("[3,1,2]", "SL([2,1,5,4],{size:2**53})", meth),
    prog("[1,'1',1n,true]", "new Set(['1',1n,1,false])", meth),
    prog("[{},[]]", "new Set([{},[]])", meth),
    prog("[Symbol.iterator]", "new Set([Symbol.iterator,Symbol()])", meth),
    prog("[0]", "SL([-0])", meth),
    prog("[-0]", "SL([0])", meth),
    prog("[-0]", "SL([0],{size:5})", meth),
    prog("[1,2,3]", "SL([3,2,1,3,2,1])", meth),
    prog("[1,2,3]", "SL([3,3,3,3],{size:4})", meth),
    prog("[1,2,3]", "SL([3,3,3,3],{size:1})", meth),
    prog("[1,2,3]", "SL([4,4,4],{size:3})", meth),
    prog("[1,2,3]", "SL([1,1,1,1,1],{size:5})", meth),
    prog("[1,2,3]", "SL([],{size:0,hasF:()=>true})", meth),
    prog("[1,2,3]", "SL([1,2,3],{size:3,hasF:()=>false})", meth),
    prog("[1,2,3]", "SL([1,2,3],{size:99,hasF:()=>false})", meth),
    prog("[1,2,3]", "SL([1,2,3],{size:99,hasF:()=>true})", meth),
    prog("[1,2,3]", "SL([1,2,3],{size:2,hasF:()=>true})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,hasF:()=>1})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,hasF:()=>'x'})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,hasF:()=>0})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,hasF:()=>null})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,hasF:()=>({})})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,nextF:(i,items)=>i<2?{done:false,value:items[i]}:{done:true}})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,nextF:(i,items)=>i<2?{done:false,value:items[i]}:{done:false,value:9}})", meth),
    prog("[1,2,3]", "SL([7,8],{size:2,nextF:(i,items,log)=>{if(i===1)throw new RangeError('n1');return {done:false,value:items[i]}}})", meth),
    prog("[1,2,3]", "SL([1,2],{size:2,nextF:(i,items,log)=>{if(i===1)throw new RangeError('n1');return {done:false,value:items[i]}}})", meth),
    prog("[1,2,3]", "SL([1,2],{size:2,nextF:(i,items,log)=>i===0?{done:false,value:1}:i===1?5:{done:true}})", meth),
    prog("[1,2,3]", "SL([1,2],{size:2,nextF:(i,items,log)=>({done:i>=2,value:items[i]})})", meth),
    prog("[1,2,3]", "SL([1,2],{size:2,nextF:(i,items,log)=>({done:undefined,value:items[i%2]})})", meth),
    prog("[1,2,3]", "SL([1,2],{size:2,nextF:(i,items,log)=>({done:i>=2})})", meth),
  );
}

// ---- 9. groupBy.
const itemsList = [
  "[1,2,3,4]", "'abca'", "new Set([1,2,2,3])", "new Map([[1,2],[3,4]])", "(function*(){yield 1;yield 2;yield 3})()", "[]", "''", "[,,1]",
  "{length:2,0:1,1:2}", "null", "undefined", "1", "{}", "true", "Symbol()", "[NaN,0,-0]", "new Uint8Array([1,2,3])", "'\\ud83d\\ude00a'",
  "[1n,2n]", "[{},{}]", "[undefined,null]", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}",
  "{[Symbol.iterator](){return {next:1}}}", "{[Symbol.iterator](){return {next(){return 1}}}}", "new String('ab')", "Object('x')",
];
const cbs = [
  "x=>x", "x=>x%2", "(x,i)=>i", "x=>String(x)", "x=>typeof x", "()=>undefined", "()=>null", "()=>NaN", "()=>-0", "()=>1n", "()=>Symbol.iterator",
  "()=>({})", "()=>'__proto__'", "()=>({toString(){return 'k'}})", "()=>({toString(){throw new RangeError('ts')}})", "(x,i,...r)=>r.length",
  "function(){return this===undefined}", "undefined", "null", "1", "{}", "class{}", "async()=>1", "function*(){}", "(x,i)=>i%2?'odd':'even'",
  "x=>({valueOf(){return 1},toString(){return 'ts'}})", "x=>[x]", "x=>[1,2]", "()=>true", "x=>x>1",
];
for (const fn of ["Map.groupBy", "Object.groupBy"]) {
  for (const items of itemsList) {
    for (const cb of cbs) add(`T(()=>{var r=${fn}(${items},${cb});return V(r)+'|'+(Object.getPrototypeOf(r)===null)})`);
  }
  add(`T(()=>${fn}())`, `T(()=>${fn}([1]))`, `T(()=>${fn}.length+${fn}.name)`, `T(()=>new ${fn}([],x=>x))`,
    `T(()=>S(Object.getOwnPropertyDescriptor(${fn.split(".")[0]},'groupBy')))`,
    `T(()=>${fn}.call(undefined,[1,2],x=>x%2).constructor===undefined)`,
    `T(()=>{var f=${fn};return V(f.call(null,[1,2],x=>x%2))})`,
    `T(()=>{var f=${fn};return V(f.call({},[1,2],x=>x%2))})`,
    `T(()=>{class X{};X.groupBy=${fn};return V(X.groupBy([1,2],x=>x%2))})`);
}
// Chaves exóticas e valores de ordem.
const groupKeys = [
  "0", "-0", "NaN", "1", "'1'", "1.5", "-1", "2**32", "2**32-1", "2**32-2", "'4294967295'", "'4294967294'", "'-0'", "'01'", "'1.0'", "''", "' '",
  "true", "false", "null", "undefined", "1n", "'__proto__'", "'constructor'", "'toString'", "'hasOwnProperty'", "'length'", "Symbol.iterator",
  "Symbol.for('a')", "Symbol('a')", "[]", "[1]", "[[]]", "{}", "{toString(){return '5'}}", "{valueOf(){return 6},toString(){return '7'}}",
  "{[Symbol.toPrimitive](){return 'tp'}}", "{[Symbol.toPrimitive](){return Symbol.for('ps')}}", "{[Symbol.toPrimitive](){throw new RangeError('tp')}}",
  "new Number(1)", "new String('s')", "function(){}", "1e21", "1e-7", "0.1+0.2", "Infinity", "-Infinity", "'\\u00e9'", "2**53", "-(2**31)",
];
for (const key of groupKeys) {
  for (const fn of ["Map.groupBy", "Object.groupBy"]) {
    add(
      `T(()=>V(${fn}([1,2,3],()=>${key})))`,
      `T(()=>V(${fn}([1,2,3,4],(x,i)=>i%2?${key}:x)))`,
      `T(()=>V(${fn}(['a','b'],x=>x==='a'?${key}:${key})))`,
      `T(()=>{var k=${key};var r=${fn}([1,2,3],x=>x===2?k:'z');return V(r)+(r instanceof Map?r.has(k):0)})`,
    );
  }
  add(`T(()=>{var r=Object.groupBy([1],()=>${key});return S(Reflect.ownKeys(r).map(k=>[typeof k,String(k)]))+S(Object.getOwnPropertyDescriptor(r,Reflect.ownKeys(r)[0]))})`);
}
// Callback que lança, iterador fechado, log de chamadas.
for (const fn of ["Map.groupBy", "Object.groupBy"]) {
  for (const k of [0, 1, 2, 3, 9]) {
    add(
      `T(()=>{var log=[];var g=(function*(){try{yield 1;yield 2;yield 3;yield 4}finally{log.push('closed')}})();try{${fn}(g,(x,i)=>{log.push(i);if(i===${k})throw new RangeError('cb'+i);return x})}catch(e){log.push(e.name+e.message)}return log.join()})`,
      `T(()=>{var log=[];var it={[Symbol.iterator](){log.push('iter');var i=0;return {next(){log.push('next');return {done:i>3,value:i++}},return(){log.push('return');return {}}}}};try{${fn}(it,(x,i)=>{log.push('cb'+i);if(i===${k})throw 5;return x})}catch(e){log.push('caught'+e)}return log.join()})`,
      `T(()=>{var log=[];var it={[Symbol.iterator](){return {i:0,next(){return {done:this.i>3,value:this.i++}},return(){log.push('return');throw new TypeError('ret')}}}};try{${fn}(it,(x,i)=>{if(i===${k})throw new RangeError('cb');return x})}catch(e){log.push(e.name+e.message)}return log.join()})`,
      `T(()=>{var log=[];var it={[Symbol.iterator](){return {i:0,next(){return {done:this.i>3,value:this.i++}},return(){log.push('return');return 1}}}};try{${fn}(it,(x,i)=>{if(i===${k})throw new RangeError('cb');return x})}catch(e){log.push(e.name+e.message)}return log.join()})`,
      `T(()=>{var log=[];try{${fn}([1,2,3,4],(x,i)=>{log.push(i);if(i===${k})return {toString(){throw new RangeError('key'+i)}};return x})}catch(e){log.push(e.name+e.message)}return log.join()})`,
    );
  }
  add(
    `T(()=>{var log=[];var r=${fn}([5,6,7],function(x,i,...rest){log.push([x,i,rest.length,this===undefined]);return x%2});return log.join('|')+V(r)})`,
    `T(()=>{var log=[];${fn}([5,6,7],(...a)=>{log.push(a.length);return 1});return log.join()})`,
    `T(()=>{var arr=[1,2,3];var r=${fn}(arr,(x,i)=>{if(i===0)arr.push(4);return x%2});return V(r)})`,
    `T(()=>{var log=[];var g={get [Symbol.iterator](){log.push('get');return function*(){log.push('start');yield 1}}};${fn}(g,x=>{log.push('cb');return x});return log.join()})`,
    `T(()=>{var log=[];try{${fn}({get [Symbol.iterator](){log.push('get');return undefined}},()=>{log.push('cb')})}catch(e){log.push(e.name+e.message)}return log.join()})`,
    `T(()=>{var log=[];try{${fn}(null,{get call(){log.push('call')}})}catch(e){log.push(e.name+e.message)}return log.join()})`,
    `T(()=>{var log=[];try{${fn}({get [Symbol.iterator](){log.push('iter');return function*(){}}},1)}catch(e){log.push(e.name+e.message)}return log.join()})`,
    `T(()=>{var r=${fn}([1,2,3,4,5,6],x=>x%3);return V(r)})`,
    `T(()=>{var r=${fn}('hello world',c=>c);return V(r)})`,
    `T(()=>{var r=${fn}([3,1,2,10,20],x=>x);return V(r)})`,
    `T(()=>{var r=${fn}([1,2],()=>'a');var s=${fn}([1,2],()=>'a');return r!==s})`,
  );
}
add(
  "T(()=>{var r=Map.groupBy([1,2],()=>0);return Object.getPrototypeOf(r)===Map.prototype&&r.size+''+[...r.keys()].map(k=>Object.is(k,-0))})",
  "T(()=>{var r=Map.groupBy([1,2],()=>-0);return [...r.keys()].map(k=>Object.is(k,-0)).join()})",
  "T(()=>{var o={};var r=Map.groupBy([1,2,3],x=>o);return r.get(o).length})",
  "T(()=>{var r=Map.groupBy([1,2,3],x=>x%2);r.get(1).push(9);return V(r)})",
  "T(()=>{var r=Object.groupBy([1,2,3],x=>x%2?'odd':'even');return Object.keys(r).join()+Object.getPrototypeOf(r)+Object.prototype.hasOwnProperty.call(r,'odd')})",
  "T(()=>{var r=Object.groupBy([1,2,3],x=>x%2?'odd':'even');r.odd.push(0);return V(r)})",
  "T(()=>{var r=Object.groupBy([1],()=>'__proto__');return Object.getPrototypeOf(r)===null&&Object.getOwnPropertyNames(r).join()+Array.isArray(r['__proto__'])})",
  "T(()=>{var r=Object.groupBy([1,2],x=>x);return Object.isExtensible(r)+','+Object.isFrozen(r)+','+Object.getOwnPropertyNames(r).join()})",
  "T(()=>{var r=Object.groupBy([1,2],x=>x);var d=Object.getOwnPropertyDescriptor(r,'1');return d.writable+','+d.enumerable+','+d.configurable+','+Array.isArray(d.value)})",
  "T(()=>{var r=Map.groupBy([1,2],x=>x);return Array.isArray(r.get(1))+','+Object.getPrototypeOf(r.get(1))===Array.prototype})",
  "T(()=>{Object.defineProperty(Array.prototype,'0',{set(v){throw new RangeError('setter')},configurable:true});try{return V(Object.groupBy([1,2],x=>x))}finally{delete Array.prototype[0]}})",
  "T(()=>{var o=Array.prototype.push;Array.prototype.push=function(){throw new RangeError('push')};try{return V(Object.groupBy([1,2],x=>x))}finally{Array.prototype.push=o}})",
  "T(()=>{var o=Map.prototype.set;Map.prototype.set=function(){throw new RangeError('mset')};try{return V(Map.groupBy([1,2],x=>x))}finally{Map.prototype.set=o}})",
  "T(()=>{var o=Map.prototype.get;Map.prototype.get=function(){throw new RangeError('mget')};try{return V(Map.groupBy([1,2],x=>x))}finally{Map.prototype.get=o}})",
  "T(()=>{var o=Map.prototype.has;Map.prototype.has=function(){throw new RangeError('mhas')};try{return V(Map.groupBy([1,2],x=>x))}finally{Map.prototype.has=o}})",
);

// ---- Execução.
// Um golden vizinho fora do formato JSON não pode derrubar a deduplicação dos outros: lê arquivo por arquivo.
const known = new Set();
for (const name of require("fs").readdirSync(GOLDEN_DIR).filter((n) => n.endsWith(".tsv") && n !== "set_methods_bun.tsv")) {
  for (const source of knownPrograms("set_methods_bun.tsv", [name])) known.add(source);
}
const seen = new Set();
const jobs = [];
let dup = 0;
for (const expr of exprs) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source) || known.has(source) || usesHostApi(source.slice(PRELUDE.length))) { dup++; continue; }
  seen.add(source);
  jobs.push({ expr, source });
}

function runChild(source) {
  return new Promise((resolve, reject) => {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      clearTimeout(timer);
      const decoded = code === 0 ? decodeResult(out) : null;
      decoded !== null ? resolve(decoded) : reject(new Error(err || "filho falhou"));
    });
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 8 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      try { results[i] = await runChild(jobs[i].source); } catch (e) { results[i] = null; process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].expr).slice(0, 160) + " " + e + "\n"); }
    }
  });
  await Promise.all(workers);
  const rows = [];
  let dropped = 0;
  for (let i = 0; i < jobs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; continue; }
    rows.push({ source: jobs[i].source, result });
  }
  process.stdout.write(emitFactored("set_methods", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos ${dup}\n`);
})();
