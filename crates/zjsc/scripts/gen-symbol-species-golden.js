// Gera tests/golden/symbol_species_bun.tsv: produtos cartesianos de Symbol (coerções, toPrimitive, chave de propriedade,
// chave de WeakMap/WeakSet/WeakRef/FinalizationRegistry), ordem de chaves de Object.keys/entries/fromEntries com inteiros,
// strings e símbolos, Object.defineProperty em arrays (length, índices, não configurável), os getters Symbol.species dos
// construtores embutidos e subclasses com species (Array, Promise, RegExp, ArrayBuffer, TypedArray), medidos no bun 1.4.2.
// Programas cuja expressão já aparece em outro golden (tests/golden/*.tsv) são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host, e só lê `globalThis.R` ao final.
// Uso: bun scripts/gen-symbol-species-golden.js > tests/golden/symbol_species_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, sampleByHash } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
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

// ---- 1. Coerções de Symbol: valor x operação.
const symbolValues = [
  "Symbol('a')", "Symbol()", "Symbol('')", "Symbol.for('k')", "Symbol.iterator", "Object(Symbol('o'))", "Symbol(undefined)",
  "Symbol(null)", "Symbol(1)", "Symbol({toString(){return 'ts'}})", "Symbol.toPrimitive", "Symbol.for('')",
];
const symbolOps = [
  "String(v)", "v.toString()", "v.description", "`${v}`", "''+v", "v+''", "+v", "v+1", "1+v", "-v", "v*2", "v==v", "v===v",
  "v==Object(v)", "Object(v)==v", "Object(v)===v", "v.valueOf()===v", "typeof v", "typeof Object(v)", "JSON.stringify(v)",
  "JSON.stringify([v])", "JSON.stringify({a:v})", "JSON.stringify({[v]:1})", "Number(v)", "Boolean(v)", "!v", "v<1", "v>1n",
  "[v].join()", "[v]+''", "Symbol.keyFor(v)", "Object.is(v,Object(v).valueOf())", "Symbol.prototype.toString.call(v)",
  "Symbol.prototype.valueOf.call(v)===v", "Object.prototype.toString.call(v)", "v instanceof Symbol", "Object(v) instanceof Symbol",
  "Object(v).description", "Object(v)[Symbol.toPrimitive]('number')===v", "v[Symbol.toPrimitive]('string')===v",
  "Symbol.prototype[Symbol.toPrimitive].call(v,'x')===v", "parseInt(v)", "isNaN(v)", "Math.abs(v)", "new Array(v).length",
  "[1,2].includes(v)", "[v].indexOf(v)", "new Set([v]).has(v)", "new Map([[v,1]]).get(v)", "({[v]:1})[v]", "String(Object(v))",
  "`${Object(v)}`", "Object(v)+''", "[...Object(v).toString()].length", "Object(v) == Object(v)", "v ? 1 : 2",
];
for (const v of symbolValues) for (const op of symbolOps) add(`T(()=>{var v=${v};return ${op}})`);

// ---- 2. Symbol.toPrimitive: variante x operação.
const primitives = [
  "{[Symbol.toPrimitive](h){log.push(h);return 1}}", "{[Symbol.toPrimitive](h){log.push(h);return 'x'}}",
  "{[Symbol.toPrimitive](h){log.push(h);return {}}}", "{[Symbol.toPrimitive](h){log.push(h)}}", "{[Symbol.toPrimitive](h){log.push(h);return null}}",
  "{[Symbol.toPrimitive](h){log.push(h);return Symbol.iterator}}", "{[Symbol.toPrimitive](h){log.push(h);return 2n}}",
  "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive]:null,valueOf(){log.push('vo');return 3},toString(){log.push('ts');return 's'}}",
  "{[Symbol.toPrimitive]:undefined,valueOf(){log.push('vo');return 3},toString(){log.push('ts');return 's'}}",
  "{[Symbol.toPrimitive](){throw new RangeError('tp')}}", "{get [Symbol.toPrimitive](){log.push('get');return h=>{log.push(h);return 4}}}",
  "{[Symbol.toPrimitive]:{}}", "{[Symbol.toPrimitive]:function(){log.push(arguments.length);return 5}}",
  "new Date(0)", "Object.assign(new Date(0),{[Symbol.toPrimitive]:undefined})", "Object.assign(new Date(0),{[Symbol.toPrimitive](h){log.push(h);return 6}})",
];
const primOps = [
  "o+1", "o+''", "`${o}`", "+o", "-o", "Number(o)", "String(o)", "o==1", "o=='x'", "o<2", "o>'a'", "o*2", "[o]+''", "[o].join()",
  "({[o]:1})", "Object.keys({[o]:1})", "o==o", "o+o", "isNaN(o)", "Math.max(o)", "new Date(o).getTime()", "BigInt(o)", "parseInt(o)",
  "new Array(o).length", "'abc'.at(o)", "'abc'.charAt(o)", "[1,2,3].at(o)", "Symbol.prototype[Symbol.toPrimitive].call(o)",
];
for (const p of primitives) for (const op of primOps) add(`T(()=>{var log=[];var o=${p};var r=${op};return log.join()+'|'+S(r)})`);

// ---- 3. Propriedades por símbolo: alvo x operação.
const targets = [
  "{}", "[]", "function(){}", "class{}", "new Map", "new Proxy({},{})", "Object.create(null)", "Object.freeze({})", "Object.preventExtensions({})",
  "new Number(1)", "new String('ab')", "[1,2]", "Object.seal({})", "new Uint8Array(2)", "Object.create({[Symbol.for('inh')]:1})",
];
const targetOps = [
  "(o[s]=1,Reflect.ownKeys(o).length)", "(o[s]=1,Object.getOwnPropertySymbols(o).length)", "(o[s]=1,Object.keys(o).length)",
  "(o[s]=1,Object.assign({},o)[s])", "(o[s]=1,({...o})[s])", "(o[s]=1,JSON.stringify(o))", "(o[s]=1,Object.entries(o).length)",
  "(o[s]=1,Object.hasOwn(o,s))", "(o[s]=1,s in o)", "(o[s]=1,delete o[s])", "(o[s]=1,D(o,s))", "(o[s]=1,Object.getOwnPropertyDescriptors(o)[s].value)",
  "(Object.defineProperty(o,s,{value:2}),D(o,s))", "(Object.defineProperty(o,s,{get(){return 3},enumerable:true}),Object.assign({},o)[s])",
  "(o[s]=1,Object.getOwnPropertyNames(o).length)", "(o[s]=1,Object.fromEntries(Object.entries(o)).constructor===Object)",
  "(o[s]=1,Object.propertyIsEnumerable.call(o,s))", "(o[s]=1,Reflect.has(o,s))", "(o[s]=1,Reflect.get(o,s))", "(o[s]=1,Reflect.deleteProperty(o,s))",
  "(Reflect.set(o,s,7),Reflect.get(o,s))", "(o[s]=1,Object.isFrozen(Object.freeze(o)))", "(o[s]=1,Object.freeze(o),o[s]=2,o[s])",
  "(o[s]=1,o.hasOwnProperty(s))", "(o[s]=1,Object.prototype.propertyIsEnumerable.call(o,s))", "(o[s]=1,Object.groupBy([1],()=>s)[s].length)",
  "(Object.defineProperty(o,s,{value:1,enumerable:false}),Object.assign({},o)[s])", "(o[s]=1,structuredCloneLike(o))",
];
for (const t of targets) for (const op of targetOps) {
  add(`T(()=>{var s=Symbol('t');var o=${t};function structuredCloneLike(x){return Object.getOwnPropertySymbols(Object.assign({},x)).length}return ${op}})`);
}

// ---- 4. Chaves fracas: tipo de chave x operação (símbolos registrados são rejeitados).
const weakKeys = [
  "Symbol('w')", "Symbol()", "Symbol.for('r')", "Symbol.iterator", "Symbol.asyncIterator", "Object(Symbol('w'))", "{}", "[]",
  "function(){}", "1", "'a'", "null", "undefined", "true", "1n", "Symbol.for('')", "new Proxy({},{})", "class{}", "Symbol.hasInstance", "Object.create(null)",
];
const weakOps = [
  "new WeakMap().set(k,1).has(k)", "new WeakMap().get(k)", "new WeakMap().has(k)", "new WeakMap().delete(k)", "new WeakMap([[k,1]]).get(k)",
  "new WeakSet().add(k).has(k)", "new WeakSet().has(k)", "new WeakSet().delete(k)", "new WeakSet([k]).has(k)", "new WeakRef(k).deref()===k",
  "new FinalizationRegistry(()=>{}).register(k,1)", "new FinalizationRegistry(()=>{}).register({},1,k)",
  "new FinalizationRegistry(()=>{}).unregister(k)", "(f=>{f.register({},1,k);return f.unregister(k)})(new FinalizationRegistry(()=>{}))",
  "new FinalizationRegistry(()=>{}).register({},k)", "new FinalizationRegistry(()=>{}).register(k,k)", "new Map([[k,1]]).get(k)",
  "(m=>{m.set(k,1);m.set(k,2);return m.get(k)})(new WeakMap)", "(w=>{w.add(k);w.add(k);return w.has(k)})(new WeakSet)",
  "Object.prototype.toString.call(new WeakRef(k))",
];
for (const k of weakKeys) for (const op of weakOps) add(`T(()=>{var k=${k};return ${op}})`);

// ---- 5. Ordem de Object.keys/entries/values com inteiros, strings e símbolos (listas de chaves pseudoaleatórias fixas).
const keyPool = [
  "1", "0", "2", "10", "'b'", "'a'", "'01'", "'-1'", "'1.5'", "'4294967294'", "'4294967295'", "'4294967296'", "Symbol.iterator", "s1", "s2",
  "'__proto__'", "''", "' 1'", "'1e3'", "'-0'", "100", "'z'", "s3", "'length'", "'constructor'",
];
let seed = 123456789;
const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
// O gerador só produz um conjunto de listas de chaves quatro vezes maior; as 60 que entram saem por hash do texto da lista
// (`sampleByHash`), nunca pela ordem em que o gerador as produziu.
const keyListCandidates = [];
for (let i = 0; i < 240; i++) {
  const n = 4 + Math.floor(rnd() * 5);
  const list = [];
  while (list.length < n) {
    const k = keyPool[Math.floor(rnd() * keyPool.length)];
    if (!list.includes(k)) list.push(k);
  }
  keyListCandidates.push(list);
}
const keyLists = sampleByHash(keyListCandidates, 60, (list) => list.join("\n"));
const keyOps = [
  "Object.keys(o)", "Object.entries(o)", "Object.values(o)", "Reflect.ownKeys(o)", "JSON.stringify(o)", "(()=>{var r=[];for(var k in o)r.push(k);return r})()",
  "Object.getOwnPropertyNames(o)", "Object.getOwnPropertySymbols(o)", "Reflect.ownKeys(Object.assign({},o))", "Reflect.ownKeys({...o})",
  "Reflect.ownKeys(Object.fromEntries(Reflect.ownKeys(o).map(k=>[k,o[k]])))", "Object.entries(Object.getOwnPropertyDescriptors(o)).length",
  "Reflect.ownKeys(Object.defineProperties({},Object.getOwnPropertyDescriptors(o)))", "Object.entries(o).flat().length",
  "Reflect.ownKeys(new Proxy(o,{}))",
];
for (const list of keyLists) {
  const body = list.map((k, i) => `o[${k}]=${i};`).join("");
  for (const op of keyOps) add(`T(()=>{var s1=Symbol('s1'),s2=Symbol('s2'),s3=Symbol('s3');var o={};${body}return ${op}})`);
}
// fromEntries e entries com iteráveis e entradas inválidas.
const entrySources = [
  "[[1,'a'],['b',2],[Symbol.iterator,3]]", "[['b',1],[2,2],['a',3],[1,4]]", "new Map([[2,'x'],['a','y'],[1,'z']])", "[]", "''", "'ab'", "[[1]]", "[[]]", "[1]",
  "[['a',1],['a',2]]", "[[1,1],['1',2]]", "[[-0,1],[0,2]]", "[[null,1],[undefined,2]]", "[[{toString(){return 'k'}},1]]", "[[Symbol.for('q'),1],[Symbol.for('q'),2]]",
  "(function*(){yield ['a',1];yield ['b',2]})()", "new Set([['a',1]])", "{}", "null", "undefined", "1", "[['__proto__',1]]", "[[1.5,1],[1e21,2]]", "[[true,1],[false,2]]",
  "{[Symbol.iterator](){return {next(){return {done:true}}}}}", "{[Symbol.iterator](){return {next(){return {value:['x',1],done:false}},return(){log.push('ret');return {}}}}}",
  "[['a',1],5]", "[['a',1],'xy']", "[['a',1],null]", "new Map([[{},1]])", "Object.entries({a:1,b:2})", "Object.entries({b:1,1:2,a:3,0:4})",
];
const entryOps = [
  "Object.fromEntries(e)", "Reflect.ownKeys(Object.fromEntries(e))", "Object.getPrototypeOf(Object.fromEntries(e))===Object.prototype",
  "Object.entries(Object.fromEntries(e))", "JSON.stringify(Object.fromEntries(e))", "D(Object.fromEntries(e),Reflect.ownKeys(Object.fromEntries(e))[0])",
];
for (const e of entrySources) for (const op of entryOps) add(`T(()=>{var log=[];var e=${e};var r=${op};return S(r)+'|'+log.join()})`);

// ---- 6. defineProperty em arrays: estado inicial x chave x descritor.
const arrays = ["[]", "[1,2,3]", "[,,]", "Array(3)", "[1,2,3,4,5]", "Object.freeze([1,2])"];
const arrayKeys = ["'0'", "2", "'3'", "5", "'4294967294'", "'4294967295'", "'-1'", "'1.5'", "'length'", "Symbol.iterator", "'01'"];
const arrayDescs = [
  "{value:9}", "{value:9,configurable:true,writable:true,enumerable:true}", "{get(){return 7}}", "{value:1,configurable:false}",
  "{writable:false}", "{configurable:false,enumerable:false}", "{value:undefined,writable:true}",
];
for (const a of arrays) for (const k of arrayKeys) for (const d of arrayDescs) {
  add(`T(()=>{var a=${a};Object.defineProperty(a,${k},${d});return S(a)+'|'+a.length+'|'+D(a,${k})})`);
}
// length: valores x estado.
const lengthValues = ["0", "1", "2", "5", "'3'", "3.0", "-0", "{valueOf(){return 2}}", "undefined", "null", "NaN", "true", "[]", "'x'", "1n"];
const lengthStates = [
  "var a=[1,2,3];", "var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});", "var a=[1,2,3];Object.defineProperty(a,2,{configurable:false});",
  "var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});", "var a=[1,,3];", "var a=[];a[10]=1;",
  "var a=[1,2,3];Object.defineProperty(a,0,{get(){return 5},configurable:true});", "var a=Object.seal([1,2,3]);",
];
for (const st of lengthStates) for (const v of lengthValues) {
  add(`T(()=>{${st}Object.defineProperty(a,'length',{value:${v}});return S(a)+'|'+a.length+'|'+D(a,'length')})`);
  add(`T(()=>{${st}a.length=${v};return S(a)+'|'+a.length+'|'+D(a,'length')})`);
}
const lengthDescs = [
  "{writable:false}", "{enumerable:true}", "{configurable:true}", "{get(){return 1}}", "{value:2,writable:false}", "{value:2,enumerable:false}",
  "{writable:true}", "{value:1,configurable:false}",
];
for (const st of lengthStates) for (const d of lengthDescs) add(`T(()=>{${st}Object.defineProperty(a,'length',${d});return S(a)+'|'+D(a,'length')})`);
const arrayMutators = ["a.push(0)", "a.pop()", "a.shift()", "a.unshift(0)", "a.splice(0,1)", "a.reverse()", "a.sort()", "a.fill(0)", "a.length=0", "a[a.length]=0", "a.copyWithin(0,1)"];
const arrayLockStates = [
  "Object.defineProperty(a,'length',{writable:false})", "Object.defineProperty(a,1,{configurable:false})", "Object.defineProperty(a,1,{writable:false})",
  "Object.preventExtensions(a)", "Object.freeze(a)", "Object.seal(a)",
];
for (const lock of arrayLockStates) for (const m of arrayMutators) add(`T(()=>{var a=[3,2,1];${lock};var r=${m};return S(a)+'|'+S(r)})`);

// ---- 7. Getters de Symbol.species dos construtores embutidos.
const speciesCtors = ["Array", "Promise", "RegExp", "ArrayBuffer", "SharedArrayBuffer", "Map", "Set", "Uint8Array", "Object.getPrototypeOf(Uint8Array)", "Int32Array", "Float64Array", "BigInt64Array", "Uint8ClampedArray", "Float32Array", "WeakMap", "Object", "Function", "Date", "Symbol", "String"];
const speciesOps = [
  "D(C,Symbol.species)", "C[Symbol.species]===C", "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.name", "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.length",
  "Object.getOwnPropertySymbols(C).map(String)", "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call(undefined)",
  "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call(null)", "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call(1)",
  "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call('a')", "typeof Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call({})",
  "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call(Symbol.iterator)===Symbol.iterator",
  "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.hasOwnProperty('prototype')", "(()=>{'use strict';C[Symbol.species]=1})()",
  "Reflect.set(C,Symbol.species,1)", "delete C[Symbol.species]", "Object.isFrozen(C)", "Object.getOwnPropertyDescriptor(C.prototype??{},Symbol.species)",
  "class X extends C{}; X[Symbol.species]===X", "Object.getOwnPropertyDescriptor(class X extends C{},Symbol.species)", "Object.create(C)[Symbol.species]===C",
  "Object.getOwnPropertyDescriptor(C,Symbol.species)?.get.call(C)===C", "Symbol.species in C", "Object.hasOwn(C,Symbol.species)",
];
for (const c of speciesCtors) for (const op of speciesOps) add(`T(()=>{var C=${c};return ${op}})`);

// ---- 8. Subclasse de Array com species: variante de species x método.
const arraySpecies = {
  default: "", undef: "static get [Symbol.species](){return undefined}", nul: "static get [Symbol.species](){return null}",
  arr: "static get [Symbol.species](){return Array}", self: "static get [Symbol.species](){return A}", plain: "static get [Symbol.species](){return function F(n){this.made=n}}",
  arrow: "static get [Symbol.species](){return ()=>({})}", throws: "static get [Symbol.species](){throw new RangeError('sp')}", num: "static get [Symbol.species](){return 1}",
  other: "static get [Symbol.species](){return class B extends Array{}}", obj: "static get [Symbol.species](){return {}}",
  str: "static get [Symbol.species](){return 'A'}", zero: "static get [Symbol.species](){return function(n){return {length:0,tag:'z'}}}",
  frozen: "static get [Symbol.species](){return function(n){return Object.freeze([])}}",
};
const arrayMethods = [
  "a.map(x=>x)", "a.filter(x=>true)", "a.slice(1)", "a.slice(0,0)", "a.splice(1,1)", "a.splice(0,0)", "a.concat([4])", "a.flat()", "a.flatMap(x=>[x])",
  "a.toSorted()", "a.toReversed()", "a.with(0,9)", "a.toSpliced(0,1)", "A.from([1,2])", "A.of(1,2)", "a.map(x=>x+1).map(x=>x)", "a.filter(x=>x>1).slice(0)",
];
for (const [name, body] of Object.entries(arraySpecies)) for (const m of arrayMethods) {
  add(`T(()=>{class A extends Array{${body}}var a=new A(3);a[0]=1;a[1]=2;a[2]=3;var r=${m};return S([Object.getPrototypeOf(r)===A.prototype,Array.isArray(r),r.length,Reflect.ownKeys(r),r.constructor===A,S(r)])})`);
}
// constructor/species definidos na própria instância de um Array comum.
const ctorOverrides = [
  "undefined", "null", "1", "'x'", "{}", "{[Symbol.species]:undefined}", "{[Symbol.species]:null}", "{[Symbol.species]:function(n){return {length:0,tag:n}}}",
  "Array", "function(){}", "class extends Array{}", "{[Symbol.species]:Array}", "{[Symbol.species]:1}", "{get [Symbol.species](){throw new EvalError('g')}}",
  "Object.assign(function(){},{[Symbol.species]:Array})", "Object.assign(function(){},{[Symbol.species]:undefined})", "Object", "Promise",
];
const plainMethods = ["a.map(x=>x)", "a.filter(x=>true)", "a.slice(1)", "a.splice(1,1)", "a.concat([4])", "a.flat()", "a.flatMap(x=>[x])", "a.slice(0,0)"];
for (const c of ctorOverrides) for (const m of plainMethods) {
  add(`T(()=>{var a=[1,2,3];a.constructor=${c};var r=${m};return S([Array.isArray(r),r.length,Object.getPrototypeOf(r)===Array.prototype,S(r)])})`);
}
// Array de outro realm-like: constructor herdado de protótipo trocado.
for (const m of plainMethods) {
  add(`T(()=>{var a=[1,2,3];Object.setPrototypeOf(a,{__proto__:Array.prototype,constructor:undefined});var r=${m};return S([Array.isArray(r),r.length,S(r)])})`);
  add(`T(()=>{var a=[1,2,3];var log=[];a.constructor=new Proxy(Array,{get(t,k,r){log.push(String(k));return Reflect.get(t,k,r)}});var r=${m};return S([log,r.length])})`);
}

// ---- 9. Promise, RegExp, ArrayBuffer e TypedArray com species.
const speciesTargets = {
  promise: ["undefined", "null", "Promise", "P", "function F(ex){log.push('new');ex(()=>{},()=>{})}", "()=>1", "function(){return 1}", "1", "{}", "function F(ex){log.push('new');ex(1,2);}"],
  regexp: ["undefined", "null", "RegExp", "R", "function F(re,fl){log.push(String(re)+'/'+fl);return new RegExp(re,fl)}", "()=>1", "1", "function F(){return {}}"],
  arraybuffer: ["undefined", "null", "ArrayBuffer", "AB", "function F(n){log.push(n);return new ArrayBuffer(n)}", "function F(n){return new ArrayBuffer(1)}", "function F(n){return {}}", "()=>1", "function F(n){return new ArrayBuffer(n+4)}", "function F(n){return new SharedArrayBuffer(n)}"],
  typed: ["undefined", "null", "Uint8Array", "U", "Int16Array", "function F(...a){log.push(a.length);return new Uint8Array(...a)}", "()=>1", "function F(){return new Uint8Array(1)}", "function F(){return {}}", "function F(...a){return new Uint8Array(a[0]+1)}"],
};
const speciesRuns = {
  promise: ["P.resolve(1).then(()=>1)", "P.resolve(1).catch(()=>1)", "P.resolve(1).finally(()=>1)", "P.reject(1).then(null,()=>1)", "P.resolve(1).then()"],
  regexp: ["'a,b'.split(new R(','))", "'a,b'.split(new R(',','g'))", "[...'abab'.matchAll(new R('a','g'))].length", "'abab'.replaceAll(new R('a','g'),'x')", "new R('a','y')[Symbol.split]('aab')", "'aab'.split(new R('a','u'))"],
  arraybuffer: ["new AB(8).slice(2,4)", "new AB(8).slice()", "new AB(8).slice(4)", "new AB(0).slice(0)", "new AB(8).slice(-2)"],
  typed: ["new U(4).map(x=>x)", "new U(4).filter(x=>true)", "new U(4).slice(1)", "new U(4).subarray(1,3)", "new U(4).slice(0,0)", "U.from([1,2])", "U.of(1,2)"],
};
const speciesDefs = {
  promise: "class P extends Promise{static get [Symbol.species](){log.push('sp');return SP}}",
  regexp: "class R extends RegExp{static get [Symbol.species](){log.push('sp');return SP}}",
  arraybuffer: "class AB extends ArrayBuffer{static get [Symbol.species](){log.push('sp');return SP}}",
  typed: "class U extends Uint8Array{static get [Symbol.species](){log.push('sp');return SP}}",
};
const describeResult = {
  promise: "[log.join(),r instanceof Promise,Object.getPrototypeOf(r)===P.prototype,Object.getPrototypeOf(r)===Promise.prototype]",
  regexp: "[log.join(),S(r)]",
  arraybuffer: "[log.join(),r.byteLength,Object.getPrototypeOf(r)===AB.prototype,Object.getPrototypeOf(r)===ArrayBuffer.prototype,r.constructor===AB]",
  typed: "[log.join(),r.length,Object.getPrototypeOf(r)===U.prototype,Object.getPrototypeOf(r)===Uint8Array.prototype,r.constructor===U,S(Array.from(r))]",
};
for (const kind of Object.keys(speciesTargets)) for (const sp of speciesTargets[kind]) for (const run of speciesRuns[kind]) {
  // SP é declarado com var para a classe poder referenciá-lo; funções nomeadas viram expressões.
  add(`T(()=>{var log=[];var SP;${speciesDefs[kind]}SP=${sp.startsWith("function F") ? "(" + sp + ")" : sp};var r=${run};return S(${describeResult[kind]})})`);
}

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
baseSources.push(...knownPrograms("symbol_species_bun.tsv", (file) => !(!file.endsWith(".tsv") || file === "symbol_species_bun.tsv")));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
const CONCURRENCY = 12;

// Roda um programa num bun filho novo (assíncrono, para paralelizar), com timeout.
function runChild(source) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [__filename, "--child"], { env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); reject(new Error("timeout")); }, 15000);
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => { clearTimeout(timer); code === 0 ? resolve(out) : reject(new Error(err || "filho falhou")); });
    child.stdin.end(source);
  });
}

(async () => {
  const todo = [];
  for (const expr of unique) {
    if (baseText.includes(expr)) { dup++; continue; }
    todo.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
  }
  const rows = new Array(todo.length);
  let next = 0;
  async function worker() {
    while (next < todo.length) {
      const i = next++;
      const { expr, source } = todo[i];
      try {
        // Processo fresco por programa: a ordem de reificação das tabelas estáticas depende do que rodou antes.
        const result = await runChild(source);
        if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
          dropped++;
          process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
        } else {
          kept++;
          rows[i] = { source, result };
        }
      } catch (e) {
        dropped++;
        process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
      }
    }
  }
  await Promise.all(Array.from({ length: CONCURRENCY }, worker));
  process.stdout.write(emitFactored("symbol_species", rows.filter(Boolean)));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
