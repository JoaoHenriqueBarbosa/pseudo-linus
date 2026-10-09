// Gera tests/golden/reflect_grid_bun.tsv: grade de Reflect.* e operadores de objeto, medida no bun 1.4.2.
// Cobre Reflect.get/set com receiver (diferente do alvo, primitivo, com propriedade não gravável, accessor,
// inexistente, Proxy, typed array, array) contra alvos com dado, accessor, somente leitura, protótipo com setter e
// Proxy; Reflect.defineProperty/deleteProperty/has/ownKeys/getPrototypeOf/setPrototypeOf/isExtensible/
// preventExtensions/apply/construct/getOwnPropertyDescriptor contra alvos exóticos (arrays, strings, typed arrays,
// arguments, funções, bound, classes, Proxy, Proxy revogado); argumentos inválidos com as mensagens exatas;
// CreateListFromArrayLike com array-like exótico e comprimento enorme controlado; newTarget inválido; e a ordem dos
// efeitos observada por um Proxy que registra cada trap, em Reflect.* e nos operadores de objeto (in, delete,
// atribuição, spread, for-in, Object.keys, JSON.stringify, freeze, instanceof...).
// Programas cuja expressão já aparece em object_edge_bun, proxy_reflect_bun, proxy_invariants_bun, reflect_bun,
// reflection_bun ou proxy_trace_bun são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host.
// O prelúdio comum sai em tests/golden/reflect_grid.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-reflect-grid-golden.js > tests/golden/reflect_grid_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

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
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function PN(p){return p===null?"null":(typeof p==="object"||typeof p==="function")?"ctor:"+(typeof p.constructor==="function"?p.constructor.name:"?"):typeof p}\n' +
  'var LOG=[],TG,PX;\n' +
  'function LP(t){TG=t;var h={};["get","set","has","deleteProperty","defineProperty","getOwnPropertyDescriptor","ownKeys","getPrototypeOf","setPrototypeOf","isExtensible","preventExtensions","apply","construct"].forEach(function(n){h[n]=function(){var a=[].slice.call(arguments,1).map(function(x){return typeof x==="symbol"?String(x):x===PX?"P":x===TG?"T":typeof x==="function"?"fn":x!==null&&typeof x==="object"?"o{"+Object.keys(x).join("/")+"}":String(x)});LOG.push(n+"("+a.join(",")+")");return Reflect[n].apply(null,arguments)}});PX=new Proxy(t,h);return PX}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const body = (code) => `T(()=>{${code}})`;

const GT = "[typeof this,this&&this.tag,this===PX?'P':''].join()";
const KEYS = ["'a'", "'0'", "Symbol.for('k')", "'length'"];

// ---- A1. Reflect.get com receiver: alvo x receiver x chave.
const getTargets = {
  absent: "{}",
  data: "{[K]:1}",
  getter: `{get [K](){return ${GT}}}`,
  setterOnly: "{set [K](v){}}",
  protoGetter: `Object.create({get [K](){return ${GT}}})`,
  protoData: "Object.create({[K]:2})",
  proxyTrap: "(function(){var p=new Proxy({[K]:1},{get(t,k,r){LOG.push('get '+String(k)+' '+(r===p?'p':typeof r));return Reflect.get(t,k,r)}});return p})()",
  proxyOfGetter: `new Proxy({get [K](){return ${GT}}},{})`,
  array: "[10,20]",
  stringObject: "new String('ab')",
  typed: "new Uint8Array([7,8])",
  args: "(function(){return arguments})(5,6)",
  definedUndefinedGetter: "Object.defineProperty({},K,{get:undefined,set(v){},configurable:true})",
  protoProxy: "Object.create(new Proxy({},{get(t,k,r){LOG.push('pget '+String(k)+' '+typeof r);return 'pv'}}))",
};
const getReceivers = [
  null, "{tag:'r'}", "1", "'s'", "null", "undefined", "true", "Symbol()", "1n", "[]", "()=>1", "new Proxy({tag:'p'},{})", "t",
];
for (const [name, target] of Object.entries(getTargets)) {
  for (const key of KEYS) {
    for (const receiver of getReceivers) {
      const call = receiver === null ? "Reflect.get(t,K)" : `Reflect.get(t,K,${receiver})`;
      add(body(`var K=${key};var t=${target};return [T(()=>${call}),LOG.join()].join(' || ')`));
    }
  }
}

// ---- A2. Reflect.set com receiver: alvo x receiver x chave.
const setTargets = {
  absent: "{}",
  data: "{[K]:1}",
  readonly: "Object.defineProperty({},K,{value:1})",
  setter: "{set [K](v){LOG.push('set '+typeof this+':'+(this&&this.tag))}}",
  getterOnly: "{get [K](){return 1}}",
  protoData: "Object.create({[K]:1})",
  protoFrozen: "Object.create(Object.freeze({[K]:1}))",
  protoSetter: "Object.create({set [K](v){LOG.push('pset '+typeof this+':'+(this&&this.tag))}})",
  protoReadonly: "Object.create(Object.defineProperty({},K,{value:1}))",
  proxy: "new Proxy({},{})",
  proxyData: "new Proxy({[K]:1},{})",
  noExtend: "Object.preventExtensions({})",
  noExtendData: "Object.preventExtensions({[K]:1})",
};
const logProxyRec = (init) =>
  `new Proxy(${init},{defineProperty(t,k,d){LOG.push('dp '+String(k)+' '+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){LOG.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})`;
const setReceivers = [
  null, "{tag:'r'}", "{[K]:0,tag:'r'}", "Object.defineProperty({tag:'r'},K,{value:0})", "Object.defineProperty({tag:'r'},K,{value:0,writable:true})",
  "Object.defineProperty({tag:'r'},K,{value:0,configurable:true})", "{get [K](){return 0},tag:'r'}", "{set [K](v){},tag:'r'}", "Object.freeze({})",
  "Object.preventExtensions({})", "Object.preventExtensions({[K]:0})", "1", "'s'", "null", "undefined", "new Proxy({},{})", "new Proxy({[K]:0},{})",
  logProxyRec("{}"), logProxyRec("{[K]:0}"), "[]", "new Uint8Array(1)",
];
for (const [name, target] of Object.entries(setTargets)) {
  for (const key of KEYS) {
    for (const receiver of setReceivers) {
      const call = receiver === null ? "Reflect.set(t,K,5)" : `Reflect.set(t,K,5,r)`;
      const setup = receiver === null ? "" : `var r=${receiver};`;
      add(body(
        `var K=${key};var t=${target};${setup}var res=T(()=>${call});` +
        `var rs=${receiver === null ? "'-'" : "T(()=>S(r))"};var rd=${receiver === null ? "'-'" : "T(()=>(r!==null&&(typeof r==='object'||typeof r==='function'))?D(r,K):'-')"};` +
        `return [res,LOG.join(),T(()=>S(t)),rs,rd].join(' || ')`,
      ));
    }
  }
}

// ---- B. Operações de Reflect.* contra alvos exóticos.
const exotic = {
  array: "[1,2,3]", emptyArray: "[]", sparse: "[1,,3]", frozenArray: "Object.freeze([1])", sealedArray: "Object.seal([1,2])",
  stringPrimitive: "'str'", stringObject: "new String('ab')", u8: "new Uint8Array([1,2])", f64Empty: "new Float64Array(0)",
  resizable: "new Uint8Array(new ArrayBuffer(2,{maxByteLength:8}))", sloppyArgs: "(function(a,b){return arguments})(1,2)",
  strictArgs: "(function(a,b){'use strict';return arguments})(1,2)", fn: "(function f(a,b){})", arrow: "(()=>1)", asyncFn: "(async function g(){})",
  genFn: "(function*(){})", cls: "(class C{static s=1})", derived: "(class D extends Array{})", bound: "(function f(a,b){}).bind(null,1)",
  native: "Math.max", ctorObject: "Object", proxyObj: "new Proxy({a:1},{})", proxyArr: "new Proxy([1],{})", proxyFn: "new Proxy(function(){},{})",
  proxyCls: "new Proxy(class{},{})", revoked: "(function(){var r=Proxy.revocable({},{});r.revoke();return r.proxy})()",
  nullProto: "Object.create(null)", sym: "Symbol()", num: "1", nul: "null", und: "undefined", big: "1n", bool: "true", map: "new Map", date: "new Date(0)",
  regexp: "/x/g", err: "new Error('m')", frozen: "Object.freeze({a:1})", sealed: "Object.seal({a:1})", noExtend: "Object.preventExtensions({a:1})",
  buffer: "new ArrayBuffer(2)", dataView: "new DataView(new ArrayBuffer(2))", promise: "Promise.resolve()", weak: "new WeakMap",
};
const ops = [
  "Reflect.defineProperty(t,'a',{value:1})", "Reflect.defineProperty(t,'0',{value:9,configurable:true})", "Reflect.defineProperty(t,'1',{get(){return 1},configurable:true})",
  "Reflect.defineProperty(t,'length',{value:0})", "Reflect.defineProperty(t,'length',{value:1,writable:false})",
  "Reflect.defineProperty(t,'a',{value:1,writable:true,enumerable:true,configurable:true})", "Reflect.defineProperty(t,'name',{value:'z'})",
  "Reflect.defineProperty(t,'prototype',{value:{}})", "Reflect.defineProperty(t,Symbol.toStringTag,{value:'Q'})", "Reflect.defineProperty(t,'a',{get:1})", "Reflect.defineProperty(t,'a')",
  "Reflect.deleteProperty(t,'a')", "Reflect.deleteProperty(t,'0')", "Reflect.deleteProperty(t,'1')", "Reflect.deleteProperty(t,'length')",
  "Reflect.deleteProperty(t,'prototype')", "Reflect.deleteProperty(t,'name')", "Reflect.deleteProperty(t,Symbol.iterator)",
  "Reflect.has(t,'a')", "Reflect.has(t,'0')", "Reflect.has(t,'length')", "Reflect.has(t,'prototype')", "Reflect.has(t,'name')", "Reflect.has(t,'toString')", "Reflect.has(t,Symbol.iterator)",
  "Reflect.ownKeys(t)", "PN(Reflect.getPrototypeOf(t))", "Reflect.setPrototypeOf(t,null)", "Reflect.setPrototypeOf(t,Object.prototype)", "Reflect.setPrototypeOf(t,t)",
  "Reflect.setPrototypeOf(t,Array.prototype)", "Reflect.setPrototypeOf(t,Object.create(t))", "Reflect.setPrototypeOf(t,Function.prototype)", "Reflect.setPrototypeOf(t,1)",
  "Reflect.setPrototypeOf(t)", "Reflect.isExtensible(t)", "Reflect.preventExtensions(t)",
  "Reflect.apply(t,undefined,[])", "Reflect.apply(t,null,[1,2])", "Reflect.apply(t,{},[3])", "Reflect.construct(t,[])", "Reflect.construct(t,[1,2])",
  "Reflect.construct(t,[],Object)", "Reflect.construct(t,[],Array)", "Reflect.construct(t,[],()=>{})", "Reflect.construct(t,[],Math.max)",
  "Reflect.construct(t,[],function(){}.bind())", "Reflect.get(t,'length')", "Reflect.get(t,'0')", "Reflect.get(t,'name')",
  "Reflect.getOwnPropertyDescriptor(t,'0')", "Reflect.getOwnPropertyDescriptor(t,'length')", "Reflect.getOwnPropertyDescriptor(t,'name')",
  "Reflect.getOwnPropertyDescriptor(t,'prototype')", "Reflect.getOwnPropertyDescriptor(t,'a')",
  "Reflect.set(t,'0',7)", "Reflect.set(t,'a',7)", "Reflect.set(t,'length',1)", "Reflect.set(t,'name','q')",
];
for (const [name, target] of Object.entries(exotic)) {
  for (const op of ops) {
    add(body(
      `var t=${target};return [T(()=>${op}),T(()=>Reflect.ownKeys(t).map(String).join()),T(()=>PN(Reflect.getPrototypeOf(t))),T(()=>Reflect.isExtensible(t)),` +
      `T(()=>D(t,'a')+'|'+D(t,'0')+'|'+D(t,'length'))].join(' || ')`,
    ));
  }
}

// ---- C1. Argumentos inválidos: cada função de Reflect contra cada primeiro argumento que não é objeto.
const reflectFns = [
  "get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf", "setPrototypeOf", "isExtensible",
  "preventExtensions", "apply", "construct",
];
const badFirst = ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n", "NaN"];
for (const fn of reflectFns) {
  add(body(`return Reflect.${fn}()`), body(`return Reflect.${fn}.length+Reflect.${fn}.name`));
  for (const bad of badFirst) {
    add(body(`return Reflect.${fn}(${bad},'a',{value:1},{})`), body(`return Reflect.${fn}(${bad})`));
  }
  add(body(`return Reflect.${fn}({})`), body(`return Reflect.${fn}({},'a')`), body(`return Reflect.${fn}(function(){},{})`));
}
// Efeito de coerção da chave contra validação do alvo.
const keyKinds = [
  "1", "-0", "1.5", "null", "undefined", "true", "1n", "Symbol.for('q')", "[1,2]", "{toString(){LOG.push('toString');return 'k'}}",
  "{valueOf(){LOG.push('valueOf');return 7},toString:undefined}", "{[Symbol.toPrimitive](h){LOG.push('prim '+h);return 'p'}}",
  "{toString(){throw new RangeError('key')}}", "{toString(){return {}},valueOf(){return {}}}", "{[Symbol.toPrimitive]:1}",
];
const keyOps = [
  "Reflect.get(TARGET,K)", "Reflect.set(TARGET,K,1)", "Reflect.has(TARGET,K)", "Reflect.deleteProperty(TARGET,K)", "Reflect.defineProperty(TARGET,K,{value:1,configurable:true})",
  "Reflect.getOwnPropertyDescriptor(TARGET,K)",
];
for (const key of keyKinds) {
  for (const op of keyOps) {
    for (const target of ["{}", "[]", "1", "new Uint8Array(2)", "new Proxy({},{get(t,k){LOG.push('trap get '+String(k));return undefined}})"]) {
      add(body(`var K=${key};var res=T(()=>${op.replace(/TARGET/g, target)});return res+' || '+LOG.join()+' || '+typeof K`));
    }
  }
}

// ---- C2. CreateListFromArrayLike.
const calleeF = "function(){return arguments.length+':'+[].map.call(arguments,function(x){return typeof x==='symbol'?'sym':typeof x==='object'&&x!==null?'obj':String(x)}).join()}";
const arrayLikes = [
  "[1,2,3]", "[]", "[1,,3]", "[undefined,undefined]", "{length:2,0:'a',1:'b'}", "{length:'2',0:'a',1:'b'}", "{length:-1}", "{length:1.9,0:'x',1:'y'}", "{length:NaN}",
  "{length:Infinity}", "{length:0,0:'x'}", "{}", "{length:2}", "{length:true,0:1}", "{length:null,0:1}", "{length:undefined}", "{length:{valueOf(){return 2}},0:'a',1:'b'}",
  "{length:-0}", "{length:3,1:'m'}", "{get length(){throw new RangeError('len')}}", "{length:1,get 0(){throw new EvalError('el')}}", "{length:1,get 0(){return this===undefined?'u':typeof this}}",
  "'ab'", "new String('ab')", "1", "true", "null", "undefined", "Symbol()", "1n", "()=>1", "function(a,b){}", "new Uint8Array([4,5])", "new Float32Array(0)",
  "(function(){return arguments})(7,8,9)", "new Map", "new Set([1])", "[1,2,3].values()", "new Proxy([1,2],{})", "{length:1,0:'big',__proto__:null}",
  "Object.assign([],{length:5})", "Object.create({length:2,0:'i',1:'j'})", "Object.defineProperty({length:2},0,{get(){return 'acc'},enumerable:false})",
  "new Proxy({length:2,0:'a',1:'b'},{get(t,k){LOG.push('get '+String(k));return t[k]},has(t,k){LOG.push('has '+String(k));return k in t}})",
  "new Proxy([1,2],{get(t,k,r){LOG.push('get '+String(k));return Reflect.get(t,k,r)},getOwnPropertyDescriptor(t,k){LOG.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},has(t,k){LOG.push('has '+String(k));return k in t}})",
  "{length:2,get 0(){LOG.push('g0');return 'a'},get 1(){LOG.push('g1');return 'b'}}",
  "{get length(){LOG.push('len');return 2},0:'a',1:'b'}",
  "(function(){var a=[1,2,3];return {get length(){a.length=1;return 3},0:'z',1:'y',2:'x'}})()",
  "(function(){var o={length:3,get 0(){delete o[1];return 'a'},1:'b',2:'c'};return o})()",
];
for (const al of arrayLikes) {
  add(
    body(`var F=${calleeF};var al=${al};return T(()=>Reflect.apply(F,null,al))+' || '+LOG.join()`),
    body(`var F=${calleeF};var al=${al};return T(()=>F.apply(null,al))+' || '+LOG.join()`),
    body(`var F=function(){this.n=arguments.length;this.v=[].join.call(arguments)};var al=${al};return T(()=>Reflect.construct(F,al))+' || '+LOG.join()`),
    body(`var al=${al};return T(()=>Reflect.apply(Math.max,null,al))+' || '+LOG.join()`),
    body(`var al=${al};return T(()=>Reflect.apply(String.fromCharCode,null,al))+' || '+LOG.join()`),
    body(`var al=${al};return T(()=>Reflect.construct(Array,al))+' || '+LOG.join()`),
    body(`var al=${al};return T(()=>Reflect.apply(Function.prototype.call,function(){return arguments.length},al))+' || '+LOG.join()`),
  );
}
// Comprimento enorme controlado: abaixo do teto da pilha funciona, acima dele é RangeError antes de tocar nos elementos.
for (const n of ["0", "1", "255", "65535", "65536", "99999", "1e6", "2**24", "2**31-1", "2**31", "2**32-1", "2**32", "2**32+1", "2**53-1", "2**53", "2**53+2", "1e21", "Infinity", "-Infinity", "-1", "1e-7", "'65536'", "'1e6'"]) {
  add(
    body(`var F=function(){return arguments.length};return T(()=>Reflect.apply(F,null,{length:${n}}))`),
    body(`var F=function(){return arguments.length};return T(()=>F.apply(null,{length:${n}}))`),
    body(`var al=new Proxy({length:${n}},{get(t,k){LOG.push('get '+String(k));return t[k]}});return T(()=>Reflect.apply(function(){return arguments.length},null,al))+' || '+LOG.length`),
  );
}
for (const n of ["0", "3", "100", "1000"]) {
  add(
    body(`var F=function(){return arguments.length};return T(()=>Reflect.construct(F,{length:${n}}))`),
    body(`var F=function(){this.k=arguments.length};return T(()=>Reflect.construct(F,new Proxy({length:${n}},{get(t,k){return k==='length'?t.length:'e'}})))`),
  );
}

// ---- C3. newTarget e alvo de Reflect.construct.
const ctors = {
  fn: "function F(){this.x=1}", cls: "class C{constructor(){this.k=new.target===undefined?'u':'nt'}}", derived: "class E extends Array{}",
  array: "Array", map: "Map", error: "TypeError", date: "Date", promise: "Promise", u8: "Uint8Array", regexp: "RegExp", object: "Object", func: "Function",
  string: "String", number: "Number", bool: "Boolean", symbol: "Symbol", bigint: "BigInt", proxyCtor: "Proxy", bound: "(function B(){this.b=1}).bind(null)",
  asyncFn: "(async function(){})", arrow: "(()=>1)", method: "({m(){}}).m", genFn: "(function*(){})", math: "Math.max", weakRef: "WeakRef", set: "Set",
  proxyOfFn: "new Proxy(function P(){this.p=1},{})", proxyConstruct: "new Proxy(function P(){this.p=1},{construct(t,a,nt){LOG.push('construct '+a.length+' '+(typeof nt));return {made:1}}})",
  proxyConstructBad: "new Proxy(function P(){},{construct(){return 1}})", classStatic: "class Q{static #p=1;static has(o){return #p in o}}",
};
const newTargets = [
  null, "undefined", "null", "1", "'s'", "{}", "[]", "()=>{}", "({m(){}}).m", "(async function(){})", "(function*(){})", "class{}", "(function(){}).bind()",
  "new Proxy(function(){},{})", "Math.max", "Symbol", "Object", "Array", "Date", "Function.prototype", "Map",
  "(function(){var r=Proxy.revocable(function(){},{});r.revoke();return r.proxy})()",
  "(function(){var f=function(){};f.prototype=null;return f})()", "(function(){var f=function(){};f.prototype=1;return f})()",
  "(function(){var f=function(){};f.prototype=Array.prototype;return f})()", "(function(){var f=function(){};Object.defineProperty(f,'prototype',{get(){LOG.push('proto');return Date.prototype}});return f})()",
  "new Proxy(class{},{get(t,k,r){LOG.push('nt get '+String(k));return Reflect.get(t,k,r)}})",
  "(function(){var f=function(){};f.prototype=Object.create(null);return f})()",
];
for (const [name, ctor] of Object.entries(ctors)) {
  for (const nt of newTargets) {
    const call = nt === null ? "Reflect.construct(C,[])" : "Reflect.construct(C,[],NT)";
    add(body(
      `var C=${ctor};var NT=${nt === null ? "undefined" : nt};var res;try{res=${call}}catch(e){return 'throw '+e.name+': '+e.message+' || '+LOG.join()}` +
      `var pr=Object.getPrototypeOf(res);return S(res)+' '+(NT&&pr===NT.prototype?'ntproto':pr===C.prototype?'cproto':PN(pr))+' || '+LOG.join()`,
    ));
  }
}

// ---- D1. Ordem dos efeitos: Proxy que registra cada trap, em Reflect.* e nos operadores.
const logTargets = {
  data: "{a:1}", empty: "{}", array: "[1,2]", fn: "function(){return 1}", cls: "class{}", frozen: "Object.freeze({a:1})", noExtend: "Object.preventExtensions({a:1})",
  u8: "new Uint8Array(2)", args: "(function(){return arguments})(1)", inherited: "Object.create({a:1})", getter: "{get a(){return this===PX}}",
  setter: "{set a(v){LOG.push('setter '+(this===PX))}}", stringObject: "new String('ab')", symbols: "{[Symbol.iterator]:1,a:1,0:'z'}",
};
const logOps = [
  "'a' in PX", "PX.a", "PX.a=1", "delete PX.a", "PX[0]", "PX[0]=1", "PX.a++", "Object.keys(PX)", "Object.entries(PX)", "Object.values(PX)", "Object.assign({},PX)", "({...PX})",
  "(function(){var r=[];for(var k in PX)r.push(k);return r.join()})()", "JSON.stringify(PX)", "Object.getOwnPropertyDescriptors(PX)", "Object.getOwnPropertyNames(PX)",
  "Object.getOwnPropertySymbols(PX)", "Object.freeze(PX)", "Object.isFrozen(PX)", "Object.seal(PX)", "Object.isSealed(PX)", "Object.preventExtensions(PX)", "Object.isExtensible(PX)",
  "Object.hasOwn(PX,'a')", "PX.hasOwnProperty('a')", "PX.propertyIsEnumerable('a')", "Object.getPrototypeOf(PX)===Object.prototype", "Object.setPrototypeOf(PX,null)", "PX instanceof Object",
  "Object.prototype.toString.call(PX)", "typeof PX", "Array.isArray(PX)", "PX?.a", "PX.__proto__", "PX.__proto__=null", "Object.defineProperty(PX,'a',{value:3})",
  "Object.defineProperty(PX,'z',{value:3})", "Object.defineProperties(PX,{z:{value:3}})", "Reflect.ownKeys(PX).length", "[...PX]", "Array.from(PX)", "[].concat(PX)",
  "Object.create(PX).a", "'a' in Object.create(PX)", "(function(){var o=Object.create(PX);o.a=1;return Object.keys(o).join()})()", "PX()", "new PX()",
  "Reflect.apply(PX,null,[])", "Reflect.construct(PX,[])", "Function.prototype.call.call(PX)", "Function.prototype.bind.call(PX).name",
  "Reflect.get(PX,'a')", "Reflect.get(PX,'a',{})", "Reflect.get(PX,'a',PX)", "Reflect.set(PX,'a',1)", "Reflect.set(PX,'a',1,{})", "Reflect.set(PX,'a',1,PX)", "Reflect.set({},'a',1,PX)",
  "Reflect.set({a:0},'a',1,PX)", "Reflect.get({get a(){return this===PX}},'a',PX)", "Reflect.has(PX,'a')", "Reflect.deleteProperty(PX,'a')", "Reflect.defineProperty(PX,'a',{value:1})",
  "Reflect.getOwnPropertyDescriptor(PX,'a')", "Reflect.getPrototypeOf(PX)===Object.prototype", "Reflect.setPrototypeOf(PX,null)", "Reflect.isExtensible(PX)", "Reflect.preventExtensions(PX)",
  "Object.getOwnPropertyDescriptor(PX,'a')", "Object.fromEntries(Object.entries(PX))", "Object.groupBy([PX],x=>typeof x)",
  "(function(){var s=new Set;s.add(PX);return s.has(PX)})()", "Object.is(PX,PX)", "PX==PX", "String(PX)", "PX+''", "PX*1", "`${PX}`", "[PX].toString()", "Number(PX)",
  "PX.length", "PX.length=0", "PX.constructor===Object", "Symbol.iterator in PX", "PX[Symbol.iterator]", "PX[Symbol.toPrimitive]", "PX instanceof Function",
  "Object.prototype.isPrototypeOf.call(Object.prototype,PX)", "Object.prototype.isPrototypeOf.call(PX,{})", "Object.prototype.valueOf.call(PX)===PX",
];
for (const [name, target] of Object.entries(logTargets)) {
  for (const op of logOps) {
    add(body(`LP(${target});var r=T(()=>${op});var n=LOG.length;return r+' || '+LOG.join()+' || '+T(()=>S(TG))`));
  }
}

// ---- D2. Proxy como protótipo e como receiver: ordem de efeitos em cadeias.
const protoSides = [
  "{}", "{a:1}", "{get a(){return this===o?'self':'other'}}", "{set a(v){LOG.push('setter '+(this===o?'self':'other'))}}", "Object.freeze({a:1})", "[1,2]", "new Uint8Array(1)",
];
const chainOps = [
  "o.a", "o.a=2", "'a' in o", "delete o.a", "Object.keys(o).join()", "(function(){var r=[];for(var k in o)r.push(k);return r.join()})()", "o[0]", "o[0]=3", "o.length",
  "Reflect.get(o,'a')", "Reflect.set(o,'a',2)", "Reflect.has(o,'a')", "Reflect.defineProperty(o,'a',{value:5})", "Reflect.ownKeys(o).length", "Reflect.getPrototypeOf(o)===PX",
  "Object.getPrototypeOf(o)===PX", "o instanceof Object", "Reflect.set(o,'a',2,{})", "Reflect.get(o,'a',{})", "Reflect.set(o,'b',9)", "o.b=9", "o.b",
];
for (const side of protoSides) {
  for (const op of chainOps) {
    add(
      body(`LP(${side});var o=Object.create(PX);var r=T(()=>${op});return r+' || '+LOG.join()+' || '+T(()=>S(o))+' || '+T(()=>S(TG))`),
      body(`LP(${side});var o={get z(){return 1}};Object.setPrototypeOf(o,PX);var r=T(()=>${op});return r+' || '+LOG.join()+' || '+T(()=>Reflect.ownKeys(o).map(String).join())`),
    );
  }
}

// ---- Execução.
const baseSources = [];
for (const file of ["object_edge_bun.tsv", "proxy_reflect_bun.tsv", "proxy_invariants_bun.tsv", "reflect_bun.tsv", "reflection_bun.tsv", "proxy_trace_bun.tsv"]) {
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
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}

function runChild(source) {
  return new Promise((resolve) => {
    // Processo fresco por programa: a ordem de Reflect.ownKeys das tabelas estáticas depende do que rodou antes.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", (chunk) => (err += chunk));
    child.on("close", (status) => { clearTimeout(timer); const decoded = status === 0 ? decodeResult(out) : null; resolve({ status: decoded !== null ? 0 : status || 1, out: decoded, err }); });
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await runChild(jobs[index].source);
    }
  };
  await Promise.all(Array.from({ length: 10 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  const outputs = new Set();
  jobs.forEach((job, index) => {
    const { status, out, err } = results[index];
    if (status !== 0) {
      dropped++;
      process.stderr.write("filho falhou: " + JSON.stringify(job.expr).slice(0, 160) + " " + err.slice(0, 120) + "\n");
      return;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push({ source: job.source, result: out });
  });
  process.stdout.write(emitFactored("reflect_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
