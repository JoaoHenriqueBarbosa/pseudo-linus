// Gera tests/golden/collection_grid_bun.tsv: grade de chaves e receptores de Map/Set/WeakMap/WeakSet, medida no bun 1.4.2.
// Cobre SameValueZero (-0/+0, NaN, BigInt, strings, objetos, símbolos, boxed), ordem de inserção após delete e
// reinserção, mutação durante forEach e iteradores, clear durante iteração, construtores com iteráveis (entries
// inválidas, adder sobrescrito, iterador fechado em erro), receptores inválidos, chaves inválidas de WeakMap/WeakSet,
// getter size, descritores e subclasses. Programas cuja expressão já aparece nos goldens de coleções são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-collection-grid-golden.js > tests/golden/collection_grid_bun.tsv
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
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const VARS = "var o={},s=Symbol('s'),f=function(){};";

// ---- 1. Grade de chaves: cada par (a, b) em Map, Set e construtor de Map.
const keys = [
  "0", "-0", "+0", "NaN", "0/0", "1", "1.0", "'1'", "'a'", "'a'+''", "''", "null", "undefined", "true", "false",
  "1n", "0n", "-0n", "BigInt(1)", "2n**64n", "BigInt(2**53)", "2**53", "Infinity", "-Infinity", "0.1+0.2", "0.3",
  "o", "{}", "[]", "f", "s", "Symbol('s')", "Symbol.for('x')", "Symbol.iterator", "Object(1)", "new Number(1)",
  "Object('a')", "Object(1n)", "'\\u00e9'", "'e\\u0301'", "'\\ud800'", "Object(s)", "new Date(0)", "/x/",
];
for (const a of keys) {
  for (const b of keys) {
    add(
      `T(()=>{${VARS}var m=new Map;m.set(${a},'v');return [m.get(${b}),m.has(${b}),m.size,m.delete(${b}),m.size].map(S).join()})`,
      `T(()=>{${VARS}var m=new Set;m.add(${a});return [m.has(${b}),m.add(${b})===m,m.size,m.delete(${b}),m.size].map(S).join()})`,
    );
  }
}
// Ordem e normalização de -0 na chave guardada.
for (const a of keys) {
  add(
    `T(()=>{${VARS}var m=new Map([[${a},1]]);return S([...m.keys()])+S([...m])})`,
    `T(()=>{${VARS}var m=new Set([${a},${a}]);return S([...m])+m.size})`,
    `T(()=>{${VARS}var m=new Map;m.set(${a},1).set(${a},2);return S([...m.entries()])+m.size})`,
    `T(()=>{${VARS}var m=new Map;m.set(1,1).set(${a},2).set(3,3);m.delete(${a});m.set(${a},4);return S([...m])})`,
    `T(()=>{${VARS}var m=new Set([1,${a},3]);m.delete(${a});m.add(${a});return S([...m])})`,
    `T(()=>{${VARS}var m=new Set([${a}]);return S([...m.entries()])+S([...m.keys()])+S([...m.values()])})`,
  );
}

// ---- 2. WeakMap e WeakSet com cada chave.
const weakKeys = keys.concat([
  "class{}", "new Proxy({},{})", "globalThis", "new WeakMap", "new Map", "(()=>{var r=Proxy.revocable({},{});r.revoke();return r.proxy})()",
  "Symbol.for('')", "Symbol.asyncIterator", "Symbol()", "Symbol('desc')", "new Error('x')", "function*(){}", "async()=>1",
  "Object.create(null)", "new Uint8Array(1)", "Math", "JSON", "Reflect", "Symbol.prototype", "Object.prototype",
]);
for (const k of weakKeys) {
  add(
    `T(()=>{${VARS}var m=new WeakMap;var r=m.set(${k},1);return S(r===m)})`,
    `T(()=>{${VARS}var m=new WeakMap;return S(m.has(${k}))+S(m.get(${k}))+S(m.delete(${k}))})`,
    `T(()=>{${VARS}var m=new WeakMap;var k=${k};try{m.set(k,5)}catch(e){return e.name+': '+e.message}return S([m.has(k),m.get(k),m.delete(k),m.has(k)])})`,
    `T(()=>{${VARS}var m=new WeakSet;var r=m.add(${k});return S(r===m)})`,
    `T(()=>{${VARS}var m=new WeakSet;return S(m.has(${k}))+S(m.delete(${k}))})`,
    `T(()=>{${VARS}var m=new WeakSet;var k=${k};try{m.add(k)}catch(e){return e.name+': '+e.message}return S([m.has(k),m.delete(k),m.has(k)])})`,
    `T(()=>{${VARS}return S(new WeakMap([[${k},1]]) instanceof WeakMap)})`,
    `T(()=>{${VARS}return S(new WeakSet([${k}]) instanceof WeakSet)})`,
    `T(()=>{${VARS}var k=${k};var w=new WeakMap;try{w.set(k,1)}catch(e){}var x=new WeakMap;try{x.set(k,1);x.set(k,2)}catch(e){return "t"}return S(x.get(k))})`,
    `T(()=>{${VARS}var m=new WeakMap;var k=${k};try{m.set(k,1)}catch(e){return "no"}var w=new WeakSet;try{w.add(k)}catch(e){return "diff"}return "ok"})`,
  );
}
// Pares de chaves em WeakMap/WeakSet (identidade).
const wpair = ["o", "{}", "s", "Symbol('s')", "Symbol.for('x')", "f", "Object(1)", "[]"];
for (const a of wpair) for (const b of wpair) {
  add(
    `T(()=>{${VARS}var m=new WeakMap;try{m.set(${a},1)}catch(e){return "t"}return S([m.has(${b}),m.get(${b})])})`,
    `T(()=>{${VARS}var m=new WeakSet;try{m.add(${a})}catch(e){return "t"}return S(m.has(${b}))})`,
  );
}

// ---- 3. Receptores inválidos.
const receivers = [
  "new Map", "new Set", "new WeakMap", "new WeakSet", "{}", "[]", "null", "undefined", "1", "'s'", "true", "Symbol()", "1n",
  "Map.prototype", "Set.prototype", "WeakMap.prototype", "WeakSet.prototype", "Object.create(Map.prototype)",
  "Object.create(Set.prototype)", "new Proxy(new Map,{})", "new Proxy(new Set,{})", "function(){}", "class extends Map{}",
  "new (class extends Map{})", "new (class extends Set{})", "Object.assign(new Set,{get:1})", "globalThis",
];
const protos = {
  Map: ["get", "set", "has", "delete", "clear", "forEach", "keys", "values", "entries", "size", "Symbol.iterator"],
  Set: ["add", "has", "delete", "clear", "forEach", "keys", "values", "entries", "size", "Symbol.iterator"],
  WeakMap: ["get", "set", "has", "delete"],
  WeakSet: ["add", "has", "delete"],
};
for (const [c, ms] of Object.entries(protos)) {
  for (const m of ms) {
    for (const r of receivers) {
      const acc = m === "size" ? `Object.getOwnPropertyDescriptor(${c}.prototype,'size').get.call(R0)` : m === "Symbol.iterator" ? `${c}.prototype[Symbol.iterator].call(R0)` : `${c}.prototype.${m}.call(R0,{},()=>{})`;
      add(`T(()=>{${VARS}var R0=${r};var x=${acc};return S(typeof x==="object"&&x!==null&&x.next?[...x].length:x)})`);
    }
  }
}
// Receptor com chave primitiva e argumentos faltando.
for (const c of ["Map", "Set"]) for (const m of ["forEach", "get", "set", "has", "delete", "add"]) {
  if (c === "Map" && m === "add" || c === "Set" && (m === "get" || m === "set")) continue;
  add(
    `T(()=>new ${c}().${m}())`, `T(()=>new ${c}().${m}(1))`, `T(()=>new ${c}().${m}(null))`, `T(()=>new ${c}().${m}.call())`,
    `T(()=>new ${c}().${m}.length+new ${c}().${m}.name)`, `T(()=>new ${c}().${m}.call(new ${c}===1))`,
  );
}
for (const c of ["Map", "Set"]) for (const cb of ["undefined", "null", "1", "'f'", "{}", "[]", "Symbol()", "class{}", "function(){}", "(a,b,c)=>[a,b,c]", "async()=>1"]) {
  add(
    `T(()=>{var r=[];new ${c}([1]).forEach(${cb});return "ok"})`,
    `T(()=>{var m=new ${c}([[1,2]]);var r=[];try{m.forEach(${cb},{t:1})}catch(e){return e.name+': '+e.message}return "ok"})`,
    `T(()=>{var m=new ${c}([1]);var r=[];m.forEach(function(a,b,c){r.push(S(a),S(b),c===m,typeof this)},${cb});return r.join()})`,
  );
}

// ---- 4. Mutação durante iteração.
const mutations = [
  "m.delete(1)", "m.delete(2)", "m.delete(3)", "m.clear()", "m.add(9)", "m.set(9,9)", "m.delete(1),m.add(1)", "m.delete(2),m.add(2)",
  "m.delete(3),m.add(3)", "m.clear(),m.add(7)", "m.add(2)", "m.add(1)", "m.add(4)", "m.delete(4)", "m.delete(1),m.delete(2),m.delete(3)",
  "m.delete(9)",
];
for (const c of ["Map", "Set"]) {
  const mk = c === "Map" ? "new Map([[1,1],[2,2],[3,3]])" : "new Set([1,2,3])";
  const adder = c === "Map" ? (s) => s.replace(/m\.add\(([^)]*)\)/g, "m.set($1,$1)") : (s) => s.replace(/m\.set\(([^,)]*),[^)]*\)/g, "m.add($1)");
  for (const mut of mutations) {
    const mm = adder(mut);
    for (const when of [1, 2, 3, 4]) {
      add(
        `T(()=>{var m=${mk};var r=[];m.forEach((v,k)=>{r.push(k);if(r.length===${when})${mm}; if(r.length>20)throw 1});return r.join()+"|"+m.size})`,
        `T(()=>{var m=${mk};var r=[];for(var x of m){r.push(S(x));if(r.length===${when}){${mm}}if(r.length>20)break}return r.join()+"|"+m.size})`,
        `T(()=>{var m=${mk};var it=m.keys();var r=[];for(var i=0;i<${when};i++)r.push(S(it.next().value));${mm};var n=it.next();r.push(S(n.value),n.done);var q=it.next();r.push(q.done);return r.join()})`,
        `T(()=>{var m=${mk};var it=m.entries();it.next();${mm};return S([...it])})`,
      );
    }
    add(`T(()=>{var m=${mk};var it=m.values();var a=[...it];${mm};var n=it.next();return S(a)+n.done+S(n.value)})`);
  }
}
add(
  "T(()=>{var m=new Set([1]);var r=[];m.forEach(v=>{r.push(v);if(v<5)m.add(v+1)});return r.join()})",
  "T(()=>{var m=new Map([[1,1]]);var r=[];m.forEach((v,k)=>{r.push(k);if(k<5)m.set(k+1,0)});return r.join()})",
  "T(()=>{var m=new Set([1,2,3]);var r=[];var n=0;m.forEach(v=>{r.push(v);if(n++<2){m.delete(v);m.add(v)}});return r.join()})",
  "T(()=>{var m=new Map([[1,1],[2,2]]);var r=[];m.forEach((v,k)=>{r.push(v);m.set(k,v+10)});return r.join()+S([...m])})",
  "T(()=>{var m=new Set([1,2]);var it=m[Symbol.iterator]();m.clear();m.add(5);return S(it.next())+S(it.next())})",
  "T(()=>{var m=new Set([1,2]);var it=m[Symbol.iterator]();it.next();it.next();it.next();m.add(5);return S(it.next())})",
  "T(()=>{var m=new Map([[1,1]]);var it=m[Symbol.iterator]();it.next();it.next();m.set(2,2);return S(it.next())})",
  "T(()=>{var m=new Map([[1,1],[2,2]]);var r=[];for(var [k] of m){r.push(k);m.clear();m.set(k+10,0);if(r.length>4)break}return r.join()})",
  "T(()=>{var m=new Set([1,2,3]);m.forEach(function(){m.clear()});return m.size})",
  "T(()=>{var m=new Set([1,2,3]);var r=[];for(var x of m){m.clear();r.push(x)}return r.join()})",
  "T(()=>{var m=new Set([1,2,3]);var r=[];for(var x of m){m.delete(x);r.push(x)}return r.join()+m.size})",
  "T(()=>{var m=new Map([[1,1],[2,2],[3,3]]);var r=[];for(var [k] of m){m.delete(k+1);r.push(k)}return r.join()})",
  "T(()=>{var m=new Set;var it=m.values();m.add(1);return S(it.next())+S(it.next())})",
  "T(()=>{var m=new Set;var it=m.values();it.next();m.add(1);return S(it.next())})",
  "T(()=>{var m=new Set([1]);var it=m.values();it.next();it.next();return S(it.next())+m.add(2).size+S(it.next())})",
  "T(()=>{var m=new Set([1,2,3]);var i1=m.values(),i2=m.values();i1.next();m.delete(1);return S(i1.next())+S(i2.next())})",
  "T(()=>{var m=new Set([NaN]);var r=[];m.forEach(v=>{r.push(v);m.delete(NaN);m.add(NaN);if(r.length>3)throw 1});return 'x'})",
  "T(()=>{var m=new Set([1]);m.forEach(function(v,k,s){r=[v,k,s===m,arguments.length]});return S(r)})",
);

// ---- 5. Construtores com iteráveis.
const iterables = [
  "[]", "[[1,2]]", "[[1,2],[3,4]]", "[1]", "[[]]", "[[1]]", "[[1,2,3]]", "['ab']", "'ab'", "[null]", "[undefined]", "[[,]]", "[0]", "[true]",
  "[Symbol()]", "[1n]", "[()=>1]", "[{}]", "[new String('ab')]", "[[1,2],3]", "[[1,2],null]", "new Set([[1,2]])", "new Map([[1,2]])",
  "new Map([[1,2]]).entries()", "(function*(){yield [1,2];yield [3,4]})()", "(function*(){yield 1})()", "{}", "{length:1,0:[1,2]}",
  "1", "true", "Symbol()", "null", "undefined", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}",
  "{[Symbol.iterator](){return {next:1}}}", "{[Symbol.iterator](){return {next(){return 1}}}}", "{[Symbol.iterator](){return {next(){return {done:true}}}}}",
  "{[Symbol.iterator](){return {next(){return {done:false,value:[1,2]}},return(){return {}}}}}", "[[1,2]].values()", "new Uint8Array(2)",
  "[['a',1],['a',2]]", "[[NaN,1],[NaN,2]]", "[[0,1],[-0,2]]", "[[1,2],[1,3]]", "Object.keys({a:1})", "[...'abc']", "Array(3)", "[[1,2],,[3,4]]",
  "[1,2,3,2,1]", "[NaN,NaN,0,-0]", "(function(){return arguments})(1,2)", "new Map", "new Set", "[[{},1]]", "[['k',{get 0(){return 1}}]]",
  "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined}", "{[Symbol.iterator]:function*(){yield [1,1]}}",
  "[{0:'a',1:'b'}]", "[{0:'a'}]", "[new Number(1)]", "['x']", "[[1,2]].keys()", "new String('ab')", "123",
];
for (const c of ["Map", "Set", "WeakMap", "WeakSet"]) {
  for (const it of iterables) {
    const view = c === "Map" || c === "Set" ? "S([...m])+m.size" : "typeof m";
    add(
      `T(()=>{var m=new ${c}(${it});return ${view}})`,
      `T(()=>{var m=${c}(${it});return typeof m})`,
      `T(()=>{var r=[];var m=new ${c}(${it});return r.length+${view}})`,
    );
  }
  for (const nv of ["undefined", "null", "0", "''", "false", "NaN"]) add(`T(()=>{var m=new ${c}(${nv});return typeof m+(${c==="Map"||c==="Set"?"m.size":"0"})})`);
}
add(
  "T(()=>Map())", "T(()=>Set())", "T(()=>WeakMap())", "T(()=>WeakSet())", "T(()=>Map.call({}))", "T(()=>Set.call(new Set))",
  "T(()=>Reflect.construct(Map,[],Object))", "T(()=>Object.getPrototypeOf(Reflect.construct(Map,[],Object))===Object.prototype)",
  "T(()=>Object.getPrototypeOf(Reflect.construct(Set,[],Array))===Array.prototype)", "T(()=>Reflect.construct(Map,[],()=>{}))",
  "T(()=>{function F(){}F.prototype=null;return Object.getPrototypeOf(Reflect.construct(Map,[],F))===Map.prototype})",
  "T(()=>{function F(){}F.prototype=1;return Object.getPrototypeOf(Reflect.construct(Set,[],F))===Set.prototype})",
  "T(()=>Map.length+Set.length+WeakMap.length+WeakSet.length)", "T(()=>Map.name+Set.name+WeakMap.name+WeakSet.name)",
  "T(()=>Map.prototype.constructor===Map&&Set.prototype.constructor===Set)",
  "T(()=>Map.groupBy([1,2,3],x=>x%2).size)", "T(()=>S([...Map.groupBy([1,2,3,4],x=>x%2)]))", "T(()=>S([...Map.groupBy('abc',x=>x==='a')]))",
  "T(()=>Map.groupBy())", "T(()=>Map.groupBy(null,x=>x))", "T(()=>Map.groupBy([1],1))", "T(()=>S([...Map.groupBy([NaN,NaN,0,-0],x=>x)]))",
  "T(()=>Map.groupBy.length+Map.groupBy.name)", "T(()=>Set.groupBy)", "T(()=>S([...Map.groupBy([1,2],(x,i)=>i)]))",
);

// ---- 6. Adder sobrescrito, registrado em log.
for (const c of ["Map", "Set"]) {
  const adder = c === "Map" ? "set" : "add";
  const other = c === "Map" ? "add" : "set";
  const shape = c === "Map" ? "[[1,2],[3,4]]" : "[1,2,3]";
  add(
    `T(()=>{var log=[];var orig=${c}.prototype.${adder};${c}.prototype.${adder}=function(...a){log.push(S(a));return orig.apply(this,a)};try{new ${c}(${shape})}finally{${c}.prototype.${adder}=orig}return log.join("|")})`,
    `T(()=>{var log=[];var orig=${c}.prototype.${adder};${c}.prototype.${adder}=function(...a){log.push(S(a));return orig.apply(this,a)};try{new ${c}()}finally{${c}.prototype.${adder}=orig}return log.join("|")+log.length})`,
    `T(()=>{var log=[];var orig=${c}.prototype.${adder};${c}.prototype.${adder}=function(...a){log.push(a.length);return 1};try{var m=new ${c}(${shape});return log.join()+m.size}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};${c}.prototype.${adder}=1;try{return S(new ${c}(${shape}))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};${c}.prototype.${adder}=1;try{return S(new ${c}([]))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};${c}.prototype.${adder}=1;try{return S(new ${c}(null))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};${c}.prototype.${adder}=undefined;try{return S(new ${c}(${shape}))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};delete ${c}.prototype.${adder};try{return S(new ${c}(${shape}))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var orig=${c}.prototype.${adder};${c}.prototype.${adder}=function(){throw new RangeError('adder')};try{return S(new ${c}(${shape}))}catch(e){return e.name+': '+e.message}finally{${c}.prototype.${adder}=orig}})`,
    `T(()=>{var log=[];var orig=${c}.prototype.${adder};${c}.prototype.${adder}=function(){throw new RangeError('adder')};var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${c==="Map"?"[1,2]":"1"}}},return(){log.push('return');return {}}}}};try{new ${c}(it)}catch(e){log.push(e.name)}finally{${c}.prototype.${adder}=orig}return log.join()})`,
    `T(()=>{var log=[];var orig=${c}.prototype.${other};${c}.prototype.${other}=function(){log.push('other')};try{new ${c}(${shape})}finally{if(orig)${c}.prototype.${other}=orig;else delete ${c}.prototype.${other}}return log.length})`,
    `T(()=>{var log=[];var g=Object.getOwnPropertyDescriptor(${c}.prototype,'${adder}');Object.defineProperty(${c}.prototype,'${adder}',{get(){log.push('get');return g.value},configurable:true});try{new ${c}(${shape});new ${c}()}finally{Object.defineProperty(${c}.prototype,'${adder}',g)}return log.join()})`,
    `T(()=>{var log=[];var g=Object.getOwnPropertyDescriptor(${c}.prototype,'${adder}');Object.defineProperty(${c}.prototype,'${adder}',{get(){log.push('get');return g.value},configurable:true});try{new ${c}(null);new ${c}(undefined);new ${c}([])}finally{Object.defineProperty(${c}.prototype,'${adder}',g)}return log.join()})`,
    `T(()=>{var log=[];class X extends ${c}{${adder}(...a){log.push('X'+S(a));return super.${adder}(...a)}}var x=new X(${shape});return log.join("|")+x.size})`,
    `T(()=>{var log=[];class X extends ${c}{constructor(i){super();log.push('after super '+this.size);this.${adder}(1${c==="Map"?",1":""})}}var x=new X;return log.join()+x.size})`,
    `T(()=>{var log=[];class X extends ${c}{constructor(i){super(i);log.push(this.size)}}new X(${shape});return log.join()})`,
    `T(()=>{var log=[];var m=new ${c}(${shape});m.${adder}=function(){log.push('own')};var n=new ${c}(m);return log.length+","+n.size})`,
    `T(()=>{var o=Object.create(${c}.prototype);return ${c}.prototype.${adder}.call(o,1,1)})`,
  );
}
// Entradas inválidas de Map.
const badEntries = ["1", "'ab'", "true", "null", "undefined", "Symbol()", "1n", "()=>1", "[]", "[1]", "{}", "{0:1}", "new Number(1)", "new String('ab')", "[[]]", "{length:2}"];
for (const e of badEntries) {
  add(
    `T(()=>new Map([${e}]))`, `T(()=>new Map([[1,2],${e}]))`, `T(()=>S([...new Map([${e}])]))`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${e}}},return(){log.push('return');return {}}}}};try{new Map(it)}catch(ex){log.push(ex.name+': '+ex.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${e}}},return(){log.push('return');throw new EvalError('r')}}}};try{new Map(it)}catch(ex){log.push(ex.name+': '+ex.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${e}}},get return(){log.push('get return');return undefined}}}};try{new Map(it)}catch(ex){log.push(ex.name)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${e}}},return:null}}};try{new Map(it)}catch(ex){log.push(ex.name)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${e}}},return:1}}};try{new Map(it)}catch(ex){log.push(ex.name+': '+ex.message)}return log.join()})`,
    `T(()=>new WeakMap([${e}]))`,
  );
}
// Iteradores que lançam em pontos diferentes.
for (const c of ["Map", "Set", "WeakMap", "WeakSet"]) {
  const val = c === "Map" || c === "WeakMap" ? "[{},1]" : "{}";
  add(
    `T(()=>{var log=[];var it={[Symbol.iterator](){log.push('iter');return {next(){log.push('next');throw new RangeError('n')},return(){log.push('return');return {}}}}};try{new ${c}(it)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){throw new RangeError('i')}};try{new ${c}(it)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {get done(){throw new RangeError('d')}}},return(){log.push('return');return {}}}}};try{new ${c}(it)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,get value(){throw new RangeError('v')}}},return(){log.push('return');return {}}}}};try{new ${c}(it)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var n=0;var it={[Symbol.iterator](){return {next(){log.push('next');return n++<2?{done:false,value:${val}}:{done:true}},return(){log.push('return');return {}}}}};var m=new ${c}(it);return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {done:false,value:${c==="WeakMap"||c==="WeakSet"?"1":"[1,2]"}}},return(){log.push('return');return {}}}}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){return {done:false,value:${c==="WeakMap"||c==="WeakSet"?"1":"[1,2]"}}},return(){log.push('return');return 5}}}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){return 1}}}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {next(){return {done:true,value:1}}}}};var m=new ${c}(it);return typeof m})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {get next(){log.push('get next');return ()=>({done:true})}}}};new ${c}(it);return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return {get next(){log.push('get next');return 5}}}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return 5}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator](){return null}};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={[Symbol.iterator]:5};try{new ${c}(it)}catch(e){log.push(e.name+': '+e.message)}return log.join()})`,
    `T(()=>{var log=[];var it={get [Symbol.iterator](){log.push('get iter');return function*(){}}};new ${c}(it);return log.join()})`,
    `T(()=>{var log=[];var it=[${val}];it[Symbol.iterator]=function*(){log.push('own');yield ${val}};new ${c}(it);return log.join()})`,
    `T(()=>{var log=[];var orig=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function(){log.push('patched');return orig.call(this)};try{new ${c}([${val}])}finally{Array.prototype[Symbol.iterator]=orig}return log.join()})`,
  );
}

// ---- 7. Identidade, descritores, size e símbolos.
for (const [c, ms] of Object.entries({
  Map: ["get", "set", "has", "delete", "clear", "forEach", "keys", "values", "entries", "size", "constructor", "groupBy"],
  Set: ["add", "has", "delete", "clear", "forEach", "keys", "values", "entries", "size", "constructor", "union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"],
  WeakMap: ["get", "set", "has", "delete", "constructor"],
  WeakSet: ["add", "has", "delete", "constructor"],
})) {
  for (const m of ms) {
    const target = m === "groupBy" ? c : `${c}.prototype`;
    add(
      `T(()=>D(${target},'${m}'))`, `T(()=>{var d=Object.getOwnPropertyDescriptor(${target},'${m}');if(!d)return 'none';var f=d.value||d.get;return typeof f==="function"?f.name+'/'+f.length:'x'})`,
      `T(()=>{var d=Object.getOwnPropertyDescriptor(${target},'${m}');if(!d)return 'none';var f=d.value||d.get;return typeof f==="function"?[Object.getOwnPropertyNames(f).join(),Object.getPrototypeOf(f)===Function.prototype,Object.isExtensible(f)].join():'x'})`,
      `T(()=>{var d=Object.getOwnPropertyDescriptor(${target},'${m}');if(!d)return 'none';var f=d.value||d.get;try{new f}catch(e){return e.name+': '+e.message}return 'ok'})`,
    );
  }
  add(
    `T(()=>Reflect.ownKeys(${c}.prototype).map(String).join())`, `T(()=>D(${c}.prototype,Symbol.toStringTag))`, `T(()=>Object.prototype.toString.call(new ${c}))`,
    `T(()=>Object.getPrototypeOf(${c}.prototype)===Object.prototype)`, `T(()=>Object.getPrototypeOf(${c})===Function.prototype)`,
    `T(()=>Reflect.ownKeys(${c}).map(String).join())`, `T(()=>D(${c},Symbol.species))`, `T(()=>D(globalThis,'${c}'))`, `T(()=>${c}[Symbol.species]===${c})`,
    `T(()=>D(${c},'prototype'))`, `T(()=>{class X extends ${c}{};return X[Symbol.species]===X})`, `T(()=>Object.getOwnPropertyNames(new ${c}).length)`,
    `T(()=>${c}.prototype.toString===Object.prototype.toString)`, `T(()=>String(new ${c}))`, `T(()=>JSON.stringify(new ${c}))`, `T(()=>new ${c} instanceof ${c})`,
    `T(()=>'size' in new ${c})`, `T(()=>Object.keys(new ${c}).length)`,
  );
}
add(
  "T(()=>Map.prototype[Symbol.iterator]===Map.prototype.entries)", "T(()=>Set.prototype[Symbol.iterator]===Set.prototype.values)", "T(()=>Set.prototype.keys===Set.prototype.values)",
  "T(()=>Map.prototype.keys===Map.prototype.values)", "T(()=>Map.prototype[Symbol.iterator].name)", "T(()=>Set.prototype[Symbol.iterator].name)", "T(()=>Set.prototype.keys.name)",
  "T(()=>Map.prototype.entries.name)", "T(()=>D(Map.prototype,Symbol.iterator))", "T(()=>D(Set.prototype,Symbol.iterator))",
  "T(()=>Object.getPrototypeOf(new Map().entries())===Object.getPrototypeOf(new Map()[Symbol.iterator]()))", "T(()=>Object.getPrototypeOf(new Map().keys())===Object.getPrototypeOf(new Map().values()))",
  "T(()=>Object.getPrototypeOf(new Set().values())===Object.getPrototypeOf(new Set().entries()))", "T(()=>Object.getPrototypeOf(new Set().values())===Object.getPrototypeOf(new Map().values()))",
  "T(()=>Object.prototype.toString.call(new Map().entries())+Object.prototype.toString.call(new Set().values()))", "T(()=>D(Object.getPrototypeOf(new Map().keys()),Symbol.toStringTag))",
  "T(()=>Reflect.ownKeys(Object.getPrototypeOf(new Map().keys())).map(String).join())", "T(()=>Reflect.ownKeys(Object.getPrototypeOf(new Set().keys())).map(String).join())",
  "T(()=>Object.getPrototypeOf(Object.getPrototypeOf(new Map().keys()))===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())))",
  "T(()=>new Map().keys()[Symbol.iterator]()!==undefined)", "T(()=>{var i=new Set().values();return i[Symbol.iterator]()===i})", "T(()=>S(new Map([[1,2]]).entries().next()))",
  "T(()=>S(new Set([1]).entries().next()))", "T(()=>S(new Set([1]).keys().next()))", "T(()=>S(new Map([[1,2]]).keys().next()))", "T(()=>S(new Map([[1,2]]).values().next()))",
  "T(()=>{var e=new Map([[1,2]]).entries().next().value;return e===e&&Array.isArray(e)})", "T(()=>{var m=new Map([[1,2]]);return m.entries().next().value!==m.entries().next().value})",
  "T(()=>Object.getOwnPropertyNames(new Map().entries().next()).join())", "T(()=>Object.getPrototypeOf(new Map().keys().next())===Object.prototype)",
  "T(()=>new Map().keys().next.call({}))", "T(()=>new Map().keys().next.call(new Set().keys()))", "T(()=>new Set().values().next.call(new Map().keys()))", "T(()=>new Set().values().next.call(null))",
  "T(()=>new Map().keys().next.call([][Symbol.iterator]()))", "T(()=>[][Symbol.iterator]().next.call(new Map().keys()))", "T(()=>new Map().keys().next.call(1))", "T(()=>new Map().entries().next.call(undefined))",
  "T(()=>new Map().entries().next.length+new Map().entries().next.name)", "T(()=>new Map().entries().next.call(Object.create(Object.getPrototypeOf(new Map().entries()))))",
  "T(()=>Object.getOwnPropertyDescriptor(Map.prototype,'size').set)", "T(()=>{var m=new Map;m.size=5;return m.size})", "T(()=>{'use strict';var m=new Map;m.size=5;return m.size})",
  "T(()=>{var m=new Set;'use strict';return Object.getOwnPropertyDescriptor(Set.prototype,'size').get.name})", "T(()=>Object.getOwnPropertyDescriptor(Map.prototype,'size').get.length)",
  "T(()=>{class X extends Map{};var x=new X([[1,2]]);return x.size+','+x.get(1)})", "T(()=>{class X extends Map{get size(){return 99}};return new X([[1,2]]).size+','+Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call(new X([[1,2]]))})",
  "T(()=>{var m=new Map([[1,2]]);Object.defineProperty(m,'size',{value:7});return m.size+','+Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call(m)})",
  "T(()=>{var m=new Map;m.get=function(){return 'own'};return new Map([[1,2]]).get(1)+m.get(1)})",
  "T(()=>{var m=new Proxy(new Map,{});return m.size})", "T(()=>{var m=new Proxy(new Map,{});return m.get(1)})", "T(()=>{var m=new Proxy(new Map,{get(t,k){var v=Reflect.get(t,k,t);return typeof v==='function'?v.bind(t):v}});m.set(1,2);return m.get(1)+','+m.size})",
  "T(()=>{var m=new Proxy(new Set,{});return m.has(1)})", "T(()=>{var m=new Proxy(new Set,{});return m.add(1)})", "T(()=>{var m=new Proxy(new Set,{});return m.forEach(()=>{})})", "T(()=>{var m=new Proxy(new Map,{});return [...m]})",
  "T(()=>{var m=new Proxy(new Map,{});return Map.prototype.get.call(m,1)})", "T(()=>{var m=new Proxy(new WeakMap,{});return m.has({})})", "T(()=>{var m=new WeakMap;return new Proxy(m,{get(t,k){return Reflect.get(t,k,t)}}).has({})})",
  "T(()=>{var k={};var m=new Proxy(new WeakMap,{get(t,p){var v=Reflect.get(t,p,t);return typeof v==='function'?v.bind(t):v}});m.set(k,1);return m.get(k)})",
  "T(()=>{var k=new Proxy({},{});var m=new WeakMap([[k,1]]);return m.get(k)})", "T(()=>{var k=new Proxy({},{});var w=new WeakSet([k]);return w.has(k)+','+w.has({})})",
  "T(()=>typeof Map.prototype.getOrInsert)", "T(()=>typeof Map.prototype.getOrInsertComputed)", "T(()=>typeof WeakMap.prototype.getOrInsert)", "T(()=>typeof Set.prototype.union)",
  "T(()=>typeof Map.prototype.emplace)", "T(()=>typeof WeakRef)+typeof FinalizationRegistry",
  "T(()=>S(Object.getOwnPropertyNames(WeakMap.prototype)))", "T(()=>S(Object.getOwnPropertyNames(WeakSet.prototype)))", "T(()=>S(Object.getOwnPropertyNames(Map.prototype)))", "T(()=>S(Object.getOwnPropertyNames(Set.prototype)))",
  "T(()=>S(Object.getOwnPropertySymbols(Map.prototype).map(String)))", "T(()=>S(Object.getOwnPropertySymbols(Set.prototype).map(String)))", "T(()=>S(Object.getOwnPropertySymbols(WeakMap.prototype).map(String)))",
);

// ---- 8. Subclasses com super chamado de várias formas.
add(
  "T(()=>{class X extends Map{};return new X([[1,2]]) instanceof Map&&new X().constructor===X})",
  "T(()=>{class X extends Map{constructor(){super([[1,2]])}};return new X().size})",
  "T(()=>{class X extends Map{constructor(...a){super(...a)}};return new X([[1,2],[3,4]]).size})",
  "T(()=>{class X extends Map{constructor(){return new Map([[9,9]])}};return S([new X() instanceof X,new X().size])})",
  "T(()=>{class X extends Map{constructor(){return {}}};return S(new X())})",
  "T(()=>{class X extends Map{constructor(){return 1}};return S(new X())})",
  "T(()=>{class X extends Map{constructor(){}};return new X()})",
  "T(()=>{class X extends Map{constructor(){this.x=1;super()}};return new X()})",
  "T(()=>{class X extends Map{constructor(){super();super()}};return new X()})",
  "T(()=>{class X extends Map{constructor(){const f=()=>super([[1,1]]);f();f()}};return new X()})",
  "T(()=>{class X extends Map{constructor(){const f=()=>super([[1,1]]);f()}};return new X().size})",
  "T(()=>{class X extends Map{constructor(){super(null)}};return new X().size})",
  "T(()=>{class X extends Map{constructor(){super(1)}};return new X().size})",
  "T(()=>{class X extends Map{constructor(){super([[1,2]]);this.extra=1}};var x=new X;return Object.keys(x).join()+x.size})",
  "T(()=>{class X extends Set{constructor(){super([1,2,2])}};return new X().size})",
  "T(()=>{class X extends Set{constructor(){super();this.add(1);this.add(1)}};return new X().size})",
  "T(()=>{class X extends Set{add(v){return super.add(v*2)}};return S([...new X([1,2])])})",
  "T(()=>{class X extends Map{set(k,v){return super.set(k,v+1)}};return S([...new X([[1,1]])])})",
  "T(()=>{class X extends Map{set(k,v){return this}};return new X([[1,1]]).size})",
  "T(()=>{class X extends Map{set(k,v){return 1}};return new X([[1,1]]).size})",
  "T(()=>{class X extends Map{static get [Symbol.species](){return Array}};return new X().constructor===X})",
  "T(()=>{class X extends Map{constructor(){super();Object.setPrototypeOf(this,Set.prototype)}};try{return new X().size}catch(e){return e.name+': '+e.message}})",
  "T(()=>{class X extends Map{};var x=new X;Object.setPrototypeOf(x,Set.prototype);try{return x.size}catch(e){return e.name+': '+e.message}})",
  "T(()=>{class X extends Map{};var x=new X;Object.setPrototypeOf(x,null);return Map.prototype.has.call(x,1)})",
  "T(()=>{class X extends Map{};var x=new X([[1,2]]);Object.setPrototypeOf(x,Object.prototype);return Map.prototype.get.call(x,1)})",
  "T(()=>{class X extends WeakMap{constructor(){super()}};var k={};var x=new X;x.set(k,1);return x.get(k)})",
  "T(()=>{class X extends WeakSet{constructor(){super([{}])}};return new X() instanceof WeakSet})",
  "T(()=>{class X extends WeakMap{constructor(){super([[1,1]])}};return new X()})",
  "T(()=>{class X extends WeakMap{set(k,v){return super.set(k,v)}};return new X([[1,1]])})",
  "T(()=>{class X extends WeakMap{};return Object.getPrototypeOf(X)===WeakMap&&X.name})",
  "T(()=>{class X extends Map{};return Object.getPrototypeOf(X.prototype)===Map.prototype})",
  "T(()=>{class X extends Map{};return Object.getOwnPropertyNames(X.prototype).join()})",
  "T(()=>{class X extends Map{};return Reflect.ownKeys(X).map(String).join()})",
  "T(()=>{class X extends Map{};return X.groupBy===Map.groupBy})",
  "T(()=>{class X extends Map{};return X.groupBy([1],x=>x) instanceof X})",
  "T(()=>{class X extends Map{};return X.groupBy([1],x=>x).constructor===Map})",
  "T(()=>{function F(){}F.prototype=Map.prototype;var m=Reflect.construct(Map,[],F);return Map.prototype.size===undefined})",
  "T(()=>{function F(){return Reflect.construct(Map,[[[1,2]]],F)}F.prototype=Object.create(Map.prototype);var m=new F;return m.size+','+(m instanceof F)})",
  "T(()=>{var m=Reflect.construct(Set,[[1]],Object);return Set.prototype.has.call(m,1)})",
  "T(()=>{var m=Reflect.construct(Set,[[1]],Object);return m.has})",
  "T(()=>{class X extends Map{constructor(){super();return this}};return new X().size})",
  "T(()=>{class X extends Map{constructor(){var r=super();return r}};return new X().size})",
  "T(()=>{class X extends Map{constructor(){super();this.set(1,1)}set(k,v){return super.set(k,v*10)}};return S([...new X])})",
  "T(()=>{class X extends Set{constructor(){super();this.add(1)}add(v){return super.add(v+1)}};return S([...new X])})",
  "T(()=>{class X extends Set{constructor(i){super(i)}add(v){return super.add(v+1)}};return S([...new X([1,2])])})",
  "T(()=>{class X extends Set{constructor(i){super();this.tag='t';for(var v of i)this.add(v)}};var x=new X([1,2]);return x.tag+x.size})",
  "T(()=>{class X extends Map{get [Symbol.toStringTag](){return 'XX'}};return Object.prototype.toString.call(new X)})",
  "T(()=>{class X extends Map{static [Symbol.hasInstance](){return false}};return new X() instanceof X})",
  "T(()=>{class X extends Map{};class Y extends X{};return new Y([[1,2]]).get(1)})",
  "T(()=>{class X extends Map{};class Y extends X{constructor(){super([[3,4]])}};return new Y().get(3)})",
  "T(()=>{var B=class extends Map{};return B.name})", "T(()=>{class X extends Map{};return X.length})", "T(()=>{class X extends Map{};return Map.length})",
  "T(()=>{class X extends Map{};return (()=>{try{X()}catch(e){return e.name+': '+e.message}})()})",
  "T(()=>{class X extends Set{};return (()=>{try{X.call(new Set)}catch(e){return e.name+': '+e.message}})()})",
  "T(()=>{class X extends Map{};return Object.getOwnPropertyNames(new X).length})",
  "T(()=>{var o=Object.create(Map.prototype);o.set=Map.prototype.set;try{o.set(1,2)}catch(e){return e.name+': '+e.message}})",
  "T(()=>{var o=Object.create(new Map([[1,2]]));try{return o.get(1)}catch(e){return e.name+': '+e.message}})",
  "T(()=>{var o=Object.create(new Set([1]));try{return o.has(1)}catch(e){return e.name+': '+e.message}})",
  "T(()=>{var o=Object.create(new Set([1]));try{return o.size}catch(e){return e.name+': '+e.message}})",
  "T(()=>{var m=new Map;var r=Map.prototype.set.call(m,1,2);return r===m})", "T(()=>{var m=new Set;var r=Set.prototype.add.call(m,1);return r===m})",
  "T(()=>{var m=new Map([[1,2]]);return Map.prototype.delete.call(m,1)+','+Map.prototype.delete.call(m,1)})", "T(()=>{var m=new Map([[1,2]]);return Map.prototype.clear.call(m)})",
  "T(()=>{var m=new Set([1]);return Set.prototype.clear.call(m)})", "T(()=>{var m=new WeakMap;var k={};m.set(k,1);return WeakMap.prototype.delete.call(m,k)+','+m.has(k)})",
);

// ---- Execução.
const baseSources = [];
for (const file of ["collections_bun.tsv", "collection_async_bun.tsv", "collection_mutation_bun.tsv", "weak_more_bun.tsv", "symbol_species_bun.tsv", "symbol_weak_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
      if (!line) continue;
      try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
    }
  } catch (e) {}
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
const seenSource = new Set();
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSources.includes(source) || seenSource.has(source)) { dup++; continue; }
  seenSource.add(source);
  jobs.push({ expr, source });
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve, reject) => {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      const result = decodeResult(out);
      if (code === 0 && result !== null) resolve(result); else reject(new Error(err || "filho falhou"));
    });
    child.stdin.end(source);
    const timer = setTimeout(() => child.kill("SIGKILL"), 6000);
    child.on("close", () => clearTimeout(timer));
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      try { results[i] = await runChild(jobs[i].source); } catch (e) { results[i] = null; process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].expr).slice(0, 160) + " " + e + "\n"); }
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; continue; }
    kept++;
    lines.push(JSON.stringify(jobs[i].source) + "\t" + JSON.stringify(result));
  }
  process.stdout.write(emitFactoredLines("collection_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
