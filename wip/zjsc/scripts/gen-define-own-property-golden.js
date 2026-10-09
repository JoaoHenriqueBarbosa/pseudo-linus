// Gera tests/golden/define_own_property_bun.tsv: combinatórias de [[DefineOwnProperty]] medidas no bun 1.4.2.
// Cobre ValidateAndApplyPropertyDescriptor (todas as transições dado<->acessor, configurable false, writable
// true->false, SameValue com NaN e -0, descritor parcial, descritor com campos herdados do protótipo do descritor,
// ordem de leitura dos campos), em objetos comuns, arrays (índice, length, 2**32-2), arguments mapeados, objetos String,
// TypedArrays, funções (prototype/name/length), globalThis, Proxy com trap defineProperty e invariantes (mensagens
// exatas), Object.freeze/seal/isFrozen em exóticos e Object.defineProperties com ordem de efeitos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Programas cuja expressão já aparece em outro golden são descartados.
// Uso: bun scripts/gen-define-own-property-golden.js > tests/golden/define_own_property_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, sampleByHash, stepSampler } = require("./golden-prelude.js");
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
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'var G=globalThis.G=function(){return 1},G2=globalThis.G2=function(){return 2},S1=globalThis.S1=function(v){},S2=globalThis.S2=function(v){};\n' +
  'function B(o,k,d){var r=Reflect.defineProperty(o,k,d);return r+" "+D(o,k)}\n' +
  'function SL(b){return new Function("D","S","Object","Reflect",b)(D,S,Object,Reflect)}\n';

// Os candidatos entram em `pool`; as matrizes grandes entram com densidade (`thin(passo, ...)`, 1 em `passo`) e a escolha
// de cada um é por hash do texto (`sampleByHash`), nunca pela posição no laço.
const pool = stepSampler();
const add = (...list) => pool.push(1, ...list);
const thin = (step, ...list) => pool.push(step, ...list);
const cart = (...lists) => lists.reduce((acc, l) => acc.flatMap(a => l.map(x => a.concat([x]))), [[]]);
const lit = parts => "{" + parts.filter(Boolean).join(",") + "}";

// ---- 1. Matriz ordinária: prior (32 estados) x descritor (dado e acessor, todos os campos opcionais).
const bools = ["true", "false"];
const priors = [];
for (const e of bools) for (const c of bools) for (const w of bools) for (const v of ["0", "NaN"]) {
  priors.push(`Object.defineProperty(o,'p',{value:${v},writable:${w},enumerable:${e},configurable:${c}})`);
}
for (const g of ["G", "undefined"]) for (const s of ["S1", "undefined"]) for (const e of bools) for (const c of bools) {
  priors.push(`Object.defineProperty(o,'p',{get:${g},set:${s},enumerable:${e},configurable:${c}})`);
}
const vals = [null, "value:1", "value:NaN", "value:-0", "value:0"];
const ws = [null, "writable:true", "writable:false"];
const es = [null, "enumerable:true", "enumerable:false"];
const cs = [null, "configurable:true", "configurable:false"];
const gs = [null, "get:undefined", "get:G", "get:G2"];
const ss = [null, "set:undefined", "set:S1", "set:S2"];
const descs = [];
for (const [v, w, e, c] of cart(vals, ws, es, cs)) descs.push(lit([v, w, e, c]));
for (const [g, s, e, c] of cart(gs, ss, es, cs)) if (g || s) descs.push(lit([g, s, e, c]));
descs.push("{value:1,get:G}", "{writable:true,get:G}", "{writable:false,set:S1}", "{value:1,set:undefined}", "{get:G,value:undefined}", "{value:1,writable:true,get:G,set:S1}");
priors.forEach((prior, i) => descs.forEach((d, j) => {
  thin(6, `T(()=>{var o={};${prior};return B(o,'p',${d})})`);
  thin(16, `T(()=>{var o={};${prior};Object.defineProperty(o,'p',${d});return D(o,'p')})`);
}));
// Repetição da mesma definição: idempotência e SameValue.
for (const v of ["0", "-0", "NaN", "1", "'s'", "undefined", "null", "{}", "G", "1n", "Symbol.iterator"]) {
  for (const w of bools) for (const c of bools) {
    add(`T(()=>{var o={};var x=${v};Object.defineProperty(o,'p',{value:x,writable:${w},configurable:${c}});return B(o,'p',{value:x})})`);
    add(`T(()=>{var o={};Object.defineProperty(o,'p',{value:${v},writable:${w},configurable:${c}});return B(o,'p',{value:${v}})})`);
  }
}
for (const [a, b] of [["0", "-0"], ["-0", "0"], ["NaN", "NaN"], ["1", "1.0"], ["'a'", "'a'"], ["1n", "1n"], ["[]", "[]"], ["null", "undefined"]]) {
  for (const w of bools) add(`T(()=>{var o={};Object.defineProperty(o,'p',{value:${a},writable:${w}});return B(o,'p',{value:${b}})+" "+(Object.is(o.p,${a}))})`);
}

// ---- 1b. Descritor com campos herdados, getters, proxies e valores de tipos variados.
const descShapes = [
  "Object.create({value:1})", "Object.create({value:1,writable:true,enumerable:true,configurable:true})", "Object.create({get:G})", "Object.create({get:G,set:S1,enumerable:true})",
  "Object.create({value:1},{writable:{value:true}})", "Object.create({value:1,get:G})", "Object.create({get:1})", "Object.create({set:'x'})", "Object.create(null)", "Object.create(null,{value:{value:3}})",
  "{__proto__:{value:9},writable:true}", "{get value(){return 5},get writable(){return true}}", "new Proxy({value:1},{})", "new Proxy({},{has(){return true},get(t,k){return k==='value'?7:undefined}})",
  "new Proxy({},{has(t,k){return k==='get'},get(){return G}})", "Object.assign(function(){},{value:1})", "Object.assign([],{value:1})", "Object.assign(new String('s'),{value:2})", "Object.assign(new Number(1),{writable:true})",
  "new Date(0)", "/x/", "new Map", "new Error", "(function(){return arguments})(1)", "Object.defineProperty({},'value',{value:1})", "Object.defineProperty({},'value',{value:1,enumerable:true})",
  "Object.defineProperty({},'get',{get(){return G}})", "{value:1,enumerable:undefined,configurable:null,writable:''}", "{value:1,enumerable:0,configurable:'false',writable:NaN}", "{enumerable:{},configurable:[],writable:Symbol.iterator}",
  "{get:null}", "{get:[]}", "{get:class{}}", "{get:async function(){}}", "{get:G,set:null}", "{set:{}}", "{set:()=>{}}", "{get:new Proxy(function(){},{})}", "{get:new Proxy({},{})}", "{get:new Proxy(class{},{})}",
];
for (const shape of descShapes) {
  add(
    `T(()=>{var o={};return B(o,'p',${shape})})`,
    `T(()=>{var o={};Object.defineProperty(o,'p',${shape});return D(o,'p')})`,
    `T(()=>{var o={};Object.defineProperty(o,'p',{value:0,writable:true,configurable:true});return B(o,'p',${shape})})`,
    `T(()=>{var o={};Object.defineProperty(o,'p',{get:G,configurable:true});return B(o,'p',${shape})})`,
    `T(()=>{var o={};Object.defineProperty(o,'p',{value:0});return B(o,'p',${shape})})`,
  );
}
// Ordem de leitura dos campos do descritor.
for (const keys of [["enumerable", "configurable", "value", "writable", "get", "set"], ["get", "set"], ["value", "get"], ["set", "writable"], ["configurable"], []]) {
  const present = keys.map(k => `'${k}'`).join(",");
  add(
    `T(()=>{var log=[];var d=new Proxy({},{has(t,k){log.push('has '+k);return [${present}].includes(k)},get(t,k){log.push('get '+k);return k==='get'||k==='set'?undefined:true}});try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var d=new Proxy({},{has(t,k){log.push('has '+k);return [${present}].includes(k)},get(t,k){log.push('get '+k);return undefined}});try{Reflect.defineProperty({},'p',d)}catch(e){log.push(e.name)}return log.join()})`,
    `T(()=>{var log=[];var d={};[${present}].forEach(k=>Object.defineProperty(d,k,{get(){log.push('get '+k);return k==='get'||k==='set'?undefined:1},enumerable:true}));Object.defineProperty({},'p',d);return log.join()})`,
    `T(()=>{var log=[];var o=new Proxy({},{defineProperty(t,k,d){log.push(k+':'+Object.keys(d).join('/')+':'+Reflect.ownKeys(d).length);return Reflect.defineProperty(t,k,d)}});var d={};[${present}].forEach(k=>d[k]=k==='get'||k==='set'?undefined:1);Object.defineProperty(o,'p',d);return log.join()})`,
  );
}
add(
  "T(()=>{var log=[];var d={get enumerable(){log.push('e');return true},get value(){log.push('v');throw new SyntaxError('boom')}};try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var log=[];var d={get get(){log.push('g');return G},get value(){log.push('v');return 1}};try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var d={get get(){log.push('g');return 1},get value(){log.push('v');return 1}};try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var d={get set(){log.push('s');return 1},get writable(){log.push('w');return 1}};try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var k={toString(){log.push('key');return 'p'}};var d={get value(){log.push('d');return 1}};Object.defineProperty({},k,d);return log.join()})",
  "T(()=>{var log=[];var k={toString(){log.push('key');return 'p'}};var d={get value(){log.push('d');throw 1}};try{Object.defineProperty({},k,d)}catch(e){log.push('c'+e)}return log.join()})",
  "T(()=>{var log=[];var o={};var k={toString(){log.push('key');return 'p'}};try{Object.defineProperty(o,k,null)}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var k={toString(){log.push('key');return 'p'}};try{Object.defineProperty(1,k,{})}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var k={toString(){log.push('key');return 'p'}};try{Reflect.defineProperty(1,k,{})}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>{var log=[];var k={toString(){log.push('key');return 'p'}};try{Reflect.defineProperty({},k,1)}catch(e){log.push(e.name+':'+e.message)}return log.join()})",
  "T(()=>Reflect.defineProperty({},'p',1))", "T(()=>Reflect.defineProperty({},'p'))", "T(()=>Reflect.defineProperty(1,'p',{}))", "T(()=>Reflect.defineProperty({}))", "T(()=>Reflect.defineProperty())",
  "T(()=>Reflect.defineProperty({},Symbol.iterator,{value:1}))", "T(()=>Reflect.defineProperty({},1n,{value:1}))", "T(()=>Reflect.defineProperty({},Symbol('x'),{get:1}))",
  "T(()=>Object.defineProperty({},Symbol('x'),1))", "T(()=>Object.defineProperty({},1n,1))", "T(()=>Object.defineProperty({},-0,{value:1}).hasOwnProperty('0'))",
  "T(()=>{var o={};Object.defineProperty(o,-0,{value:1,enumerable:true});return Object.keys(o).join()})", "T(()=>{var o={};Object.defineProperty(o,1e21,{value:1,enumerable:true});return Object.keys(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,{},{value:1,enumerable:true});return Object.keys(o).join()})", "T(()=>{var o={};Object.defineProperty(o,[1,2],{value:1,enumerable:true});return Object.keys(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,null,{value:1,enumerable:true});return Object.keys(o).join()})", "T(()=>{var o={};Object.defineProperty(o,undefined,{value:1,enumerable:true});return Object.keys(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,true,{value:1,enumerable:true});return Object.keys(o).join()})", "T(()=>{var o={};Object.defineProperty(o,{[Symbol.toPrimitive](h){return h},},{value:1,enumerable:true});return Object.keys(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,{[Symbol.toPrimitive](h){return Symbol.for(h)}},{value:1,enumerable:true});return Reflect.ownKeys(o).map(String).join()})",
  "T(()=>{var o={};Object.defineProperty(o,{[Symbol.toPrimitive](){return {}}},{value:1})})", "T(()=>{var o={};Object.defineProperty(o,{[Symbol.toPrimitive]:1},{value:1})})",
);

// ---- 2. Arrays: length e índices.
const lengthDescs = ["{value:0}", "{value:1}", "{value:2}", "{value:3}", "{value:5}", "{value:'2'}", "{value:2.0}", "{value:-1}", "{value:2**32}", "{value:4294967295}", "{value:{valueOf(){log.push('vo');return 1}}}",
  "{writable:false}", "{writable:true}", "{value:1,writable:false}", "{enumerable:true}", "{configurable:true}", "{get(){}}", "{value:2,enumerable:false,configurable:false,writable:true}", "{value:3,writable:false}", "{}",
  "{value:NaN}", "{value:undefined}", "{value:null}", "{value:true}", "{value:-0}", "{value:1.5}", "{value:'abc'}", "{value:3,writable:true}", "{value:0,writable:false}", "{set:S1}", "{value:2,configurable:true}"];
const arrayPriors = [
  "[1,2,3]", "[]", "[1,,3]", "(a=[1,2,3],Object.defineProperty(a,'length',{writable:false}),a)", "(a=[1,2,3],Object.defineProperty(a,1,{configurable:false}),a)", "(a=[1,2,3],Object.defineProperty(a,2,{configurable:false}),a)",
  "(a=[1,2,3],Object.defineProperty(a,0,{configurable:false}),a)", "Object.preventExtensions([1,2,3])", "Object.freeze([1,2,3])", "Object.seal([1,2,3])", "(a=[1,2,3],Object.defineProperty(a,2,{get(){return 1},configurable:true}),a)",
  "(a=[1,2,3],Object.defineProperty(a,2,{value:9,writable:false,configurable:true}),a)",
];
for (const prior of arrayPriors) for (const d of lengthDescs) {
  add(
    `T(()=>{var log=[];var a;a=${prior};var r=Reflect.defineProperty(a,'length',${d});return r+" "+a.length+" "+Object.keys(a).join()+" "+D(a,'length')+" "+log.join()})`,
    `T(()=>{var log=[];var a;a=${prior};Object.defineProperty(a,'length',${d});return a.length+" "+Object.keys(a).join()+" "+log.join()})`,
  );
}
const indexKeys = ["0", "2", "3", "5", "'4294967294'", "'4294967295'", "'4294967296'", "'-1'", "'1.5'", "'01'", "'1e3'", "'+1'", "' 1'", "'0x1'", "1", "'4294967293'"];
const idxDescs = ["{value:1}", "{value:'v',writable:true,enumerable:true,configurable:true}", "{get:G,configurable:true}", "{get:G}", "{value:1,writable:false,configurable:true}", "{configurable:false}", "{enumerable:true}", "{}", "{value:undefined,writable:true,enumerable:true,configurable:true}", "{set:S1,enumerable:true,configurable:true}"];
const idxPriors = ["[]", "[1,2,3]", "(a=[1,2,3],Object.defineProperty(a,'length',{writable:false}),a)", "Object.preventExtensions([1])", "[,,,,,,]"];
for (const prior of idxPriors) for (const k of indexKeys) for (const d of idxDescs) {
  thin(3, `T(()=>{var a;a=${prior};var r=Reflect.defineProperty(a,${k},${d});return r+" "+a.length+" "+D(a,${k})})`);
}
add(
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true,writable:true,enumerable:true});return a.length+' '+Object.keys(a).join()})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true,writable:true,enumerable:true});a.length=0;return a.length+' '+Object.keys(a).length})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:false,writable:true,enumerable:true});a.length=0;return a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:false,writable:true,enumerable:true});return Reflect.defineProperty(a,'length',{value:5})+' '+a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:false});return Reflect.defineProperty(a,'length',{value:4294967295})+' '+a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true});a.push(1)})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true});a[4294967295]=1;return a.length+' '+Object.keys(a).join()})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true});Object.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'4294967295',{value:1})+' '+a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'length',{value:4294967295});return a.length+' '+Reflect.defineProperty(a,'4294967294',{value:1})+' '+Object.keys(a).join()})",
  "T(()=>{var a=[];Object.defineProperty(a,'length',{value:4294967295,writable:false});return Reflect.defineProperty(a,'4294967294',{value:1})+' '+Object.keys(a).join()})",
  "T(()=>{var a=[];a[4294967294]=1;return a.length+' '+a.indexOf(1)+' '+a.lastIndexOf(1)+' '+a.at(-1)})",
  "T(()=>{var a=[];a[4294967294]=1;return Array.prototype.slice.call(a,4294967293).length})",
  "T(()=>{var a=[];a[4294967294]=1;return a.slice(-1)[0]+' '+a.includes(1)})",
  "T(()=>{var a=[];a[4294967295]=1;return a.length+' '+Object.keys(a).join()})", "T(()=>{var a=[];a[4294967296]=1;return a.length+' '+Object.keys(a).join()})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:1});return D(a,'1')+' '+a.length})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:1,writable:false});return a.length+' '+Reflect.defineProperty(a,'3',{value:1})+' '+Reflect.set(a,'3',1)+' '+Reflect.set(a,'0',7)+' '+a[0]})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'length',{value:3})+' '+Reflect.defineProperty(a,'length',{value:2})+' '+Reflect.defineProperty(a,'length',{writable:true})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'length',{value:3,writable:false})+' '+Reflect.defineProperty(a,'length',{enumerable:false})+' '+Reflect.defineProperty(a,'length',{configurable:false})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});Object.defineProperty(a,'length',{value:2})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});Object.defineProperty(a,'length',{writable:true})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});Object.defineProperty(a,5,{value:1})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});Object.defineProperty(a,1,{value:5});return S(a)})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{enumerable:true})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{configurable:true})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{get(){}})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{set(v){}})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:-1})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:1.5})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:2**32})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:'x'})})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:1n})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:Symbol()})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:{valueOf(){throw new RangeError('q')}}})})",
  "T(()=>{var a=[1,2,3];var n=0;Object.defineProperty(a,'length',{value:{valueOf(){n++;return 2}}});return n+' '+a.length})", "T(()=>{var a=[1,2,3];var n=0;Object.defineProperty(a,'length',{value:{valueOf(){n++;return 2},toString(){n+=10;return '2'}}});return n+' '+a.length})",
  "T(()=>{var a=[1,2,3];var n=[];try{Object.defineProperty(a,'length',{value:{valueOf(){n.push('a');return 2}},get(){}})}catch(e){n.push(e.name)}return n.join()})",
  "T(()=>{var a=[1,2,3];var n=[];Reflect.defineProperty(a,'length',{value:{valueOf(){n.push('a');a.push(9);return 1}}});return n.join()+S(a)})",
  "T(()=>{var a=[1,2,3];Reflect.defineProperty(a,'length',{value:{valueOf(){Object.defineProperty(a,'length',{writable:false});return 1}}});return S(a)+' '+D(a,'length')})",
  "T(()=>{var a=[1,2,3];Reflect.defineProperty(a,'length',{value:{valueOf(){Object.defineProperty(a,0,{configurable:false});return 0}}});return S(a)+' '+D(a,'length')})",
  "T(()=>{var a=[1,2,3];return Reflect.defineProperty(a,'length',{value:{valueOf(){a.length=10;return 5}}})+' '+a.length})",
  "T(()=>{class A extends Array{}var a=new A(3);Object.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'length',{value:3})+' '+Reflect.defineProperty(a,'length',{value:1})+' '+a.length})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:2,writable:false});return S(a)+Reflect.set(a,'length',0)+a.length})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});a.length=3;return a.length})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});a.length=2})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});a[3]=1})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});a.length=0})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});a.length=0;return a.length})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});a.length=0;return a.length+' '+S(a)})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});a.length=0;return D(a,'length')})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return Reflect.defineProperty(a,'length',{value:0,writable:false})+' '+D(a,'length')})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return Reflect.defineProperty(a,'length',{value:1,writable:false})+' '+D(a,'length')+S(a)})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:0,writable:false});return S(a)+' '+D(a,'length')})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false,value:{valueOf(){return 1}}});return S(a)+' '+D(a,'length')})",
);

// ---- 3. Arguments mapeado (sloppy) e não mapeado (strict).
const argsKeys = ["'0'", "'1'", "'2'", "'length'", "'callee'", "'3'", "Symbol.iterator"];
const argsDescs = ["{value:9}", "{value:9,writable:false}", "{writable:false}", "{writable:true}", "{enumerable:false}", "{configurable:false}", "{configurable:true}", "{get:G}", "{get:G,configurable:true}", "{set:S1}", "{}", "{value:undefined}",
  "{value:9,enumerable:false}", "{writable:false,value:5}", "{value:1,writable:true,enumerable:true,configurable:true}", "{get:undefined,set:undefined}", "{value:1,configurable:false}"];
const argsPre = ["", "delete arguments[K];", "Object.defineProperty(arguments,K,{writable:false});", "Object.defineProperty(arguments,K,{configurable:false});", "Object.defineProperty(arguments,K,{get:G,configurable:true});", "Object.defineProperty(arguments,K,{enumerable:false});"];
argsKeys.forEach((k, ki) => argsDescs.forEach((d, di) => argsPre.forEach((pre, pi) => {
  const body = `var r=[];(function(a,b){${pre.replace(/K/g, k)}var ok=Reflect.defineProperty(arguments,${k},${d});r.push(ok);a=9;b=8;r.push(String(arguments[${k}]));arguments[${k}]=7;r.push(String(a)+String(b));r.push(D(arguments,${k}));r.push(arguments.length)})(1,2,3);return r.join(' ')`;
  thin(3, `T(()=>SL(${JSON.stringify(body)}))`);
})));
for (const d of argsDescs) {
  add(
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a){"use strict";var ok=Reflect.defineProperty(arguments,'0',${d});a=9;r.push(ok,String(arguments[0]),D(arguments,'0'))})(1);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a){"use strict";var ok=Reflect.defineProperty(arguments,'callee',${d});r.push(ok,D(arguments,'callee'))})(1);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a,a2){var ok=Reflect.defineProperty(arguments,'0',${d});r.push(ok);Object.defineProperty(arguments,'0',{value:3});r.push(a);Object.defineProperty(arguments,'0',{writable:false});a=4;r.push(arguments[0]);Object.defineProperty(arguments,'0',{value:5});r.push(a)})(1);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a,a){var ok=Reflect.defineProperty(arguments,'0',${d});arguments[0]=5;r.push(ok,a,arguments[0])})(1,2);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a=0,b){var ok=Reflect.defineProperty(arguments,'0',${d});a=5;r.push(ok,String(arguments[0]),D(arguments,'0'))})(1,2);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a){var ok=Reflect.defineProperty(arguments,'0',${d});r.push(ok);Object.freeze(arguments);a=7;r.push(String(arguments[0]),D(arguments,'0'))})(1);return r.join(' ')`)}))`,
    `T(()=>SL(${JSON.stringify(`var r=[];(function(a){var ok=Reflect.defineProperty(arguments,'0',${d});r.push(ok);Object.seal(arguments);a=7;r.push(String(arguments[0]),D(arguments,'0'),Object.isFrozen(arguments),Object.isSealed(arguments))})(1);return r.join(' ')`)}))`,
  );
}
add(
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){Object.defineProperty(arguments,'0',{get(){return 5}});a=3;r.push(arguments[0],a);Object.defineProperty(arguments,'0',{value:8});r.push(a,arguments[0]);a=1;r.push(arguments[0])})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){Object.defineProperty(arguments,'0',{writable:false});r.push(a);a=3;r.push(arguments[0]);arguments[0]=4;r.push(a,arguments[0])})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){Object.defineProperty(arguments,'0',{value:2,writable:false});r.push(a,arguments[0]);a=3;r.push(arguments[0])})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){delete arguments[0];Object.defineProperty(arguments,'0',{value:2,writable:true,enumerable:true,configurable:true});a=3;r.push(a,arguments[0])})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){Object.defineProperty(arguments,'length',{value:0});r.push(arguments.length,Array.prototype.slice.call(arguments).length,[...arguments].length)})(1,2);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(a){Object.defineProperty(arguments,Symbol.iterator,{value:undefined});r.push(typeof arguments[Symbol.iterator]);try{[...arguments]}catch(e){r.push(e.name)}})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(){Object.defineProperty(arguments,'callee',{get(){return 1}});r.push(arguments.callee)})(1);return r.join(' ')")}))`,
  `T(()=>SL(${JSON.stringify("var r=[];(function(){'use strict';Object.defineProperty(arguments,'callee',{value:1});r.push(arguments.callee)})(1);return r.join(' ')")}))`,
);

// ---- 4. Objetos String.
const strKeys = ["'0'", "'1'", "'2'", "'length'", "'-0'", "'01'", "'1.5'", "0", "'4294967295'"];
const strDescs = ["{value:'a'}", "{value:'b'}", "{value:'a',writable:false,enumerable:true,configurable:false}", "{writable:true}", "{enumerable:false}", "{configurable:true}", "{get:G}", "{}", "{value:2}", "{value:2,writable:false}", "{value:'x',writable:true,enumerable:true,configurable:true}",
  "{value:undefined}", "{set:S1,enumerable:true}", "{value:'a',enumerable:true}"];
for (const k of strKeys) for (const d of strDescs) {
  add(
    `T(()=>{var s=new String('ab');var r=Reflect.defineProperty(s,${k},${d});return r+' '+D(s,${k})+' '+Object.getOwnPropertyNames(s).join()})`,
    `T(()=>{var s=new String('ab');Object.defineProperty(s,${k},${d});return D(s,${k})+' '+s.length+s[0]+s[1]+s[2]})`,
    `T(()=>{var s=new String('');var r=Reflect.defineProperty(s,${k},${d});return r+' '+D(s,${k})+' '+Object.getOwnPropertyNames(s).join()})`,
  );
}
add(
  "T(()=>Object.defineProperty('ab','0',{value:'a'}))", "T(()=>Reflect.defineProperty('ab','0',{value:'a'}))", "T(()=>Object.defineProperty(new String('ab'),'x',{value:1}).x)",
  "T(()=>{var s=new String('ab');s.length=9;return s.length})", "T(()=>{'use strict';var s=new String('ab');s.length=9})", "T(()=>{'use strict';var s=new String('ab');s[0]='z'})", "T(()=>{'use strict';var s=new String('ab');s[5]='z';return Object.keys(s).join()})",
  "T(()=>{var s=new String('ab');s[5]='z';return Object.keys(s).join()+s.length})", "T(()=>{var s=new String('ab');Object.defineProperty(s,'5',{value:1,enumerable:true});return Object.keys(s).join()+s.length})",
  "T(()=>{var s=new String('ab');Object.defineProperty(s,'2',{value:1,enumerable:true});Object.defineProperty(s,'1',{value:'b'});return Object.keys(s).join()})",
  "T(()=>{var s=Object.preventExtensions(new String('ab'));return Reflect.defineProperty(s,'2',{value:1})+' '+Reflect.defineProperty(s,'0',{value:'a'})+' '+Reflect.defineProperty(s,'length',{value:2})})",
  "T(()=>{var s=Object.freeze(new String('ab'));return Reflect.defineProperty(s,'0',{value:'a'})+' '+Reflect.defineProperty(s,'0',{value:'b'})+' '+Object.isFrozen(s)+Object.isSealed(s)})",
  "T(()=>{var s=Object.seal(new String('ab'));return Object.isFrozen(s)+' '+Object.isSealed(s)+' '+Object.isExtensible(s)})", "T(()=>{var s=Object.freeze(new String(''));return Object.isFrozen(s)+' '+Object.getOwnPropertyNames(s).join()})",
  "T(()=>{var s=Object.preventExtensions(new String(''));return Object.isFrozen(s)+' '+Object.isSealed(s)})", "T(()=>{var s=Object.preventExtensions(new String('a'));return Object.isFrozen(s)+' '+Object.isSealed(s)})",
  "T(()=>{var s=new String('ab');Object.defineProperty(s,'x',{value:1,configurable:true});Object.preventExtensions(s);return Object.isFrozen(s)+' '+Object.isSealed(s)})",
  "T(()=>{class SS extends String{}var s=new SS('ab');return Reflect.defineProperty(s,'0',{value:'a'})+' '+Reflect.defineProperty(s,'0',{value:'q'})})",
  "T(()=>{var s=new String('\\ud800\\udc00');return Reflect.defineProperty(s,'0',{value:'\\ud800'})+' '+Reflect.defineProperty(s,'2',{value:1})+' '+s.length})",
);

// ---- 5. TypedArrays.
const taCtors = ["Uint8Array", "Float32Array", "BigInt64Array", "Uint8ClampedArray"];
const taKeys = ["'0'", "'1'", "'2'", "'-0'", "'1.5'", "'Infinity'", "'NaN'", "'-1'", "'x'", "'4294967295'", "'1e3'", "0", "Symbol.iterator", "'-Infinity'", "'0.0'", "'9007199254740992'"];
const taDescs = ["{value:5}", "{value:5n}", "{value:1,writable:true,enumerable:true,configurable:true}", "{value:1,writable:false}", "{configurable:false}", "{enumerable:false}", "{get(){}}", "{set(){}}", "{}", "{value:1,configurable:true}", "{value:1,enumerable:true}", "{writable:true}",
  "{value:{valueOf(){return 3}}}", "{value:'7'}", "{value:undefined}", "{configurable:true}", "{writable:false}", "{enumerable:true,writable:true}"];
for (const c of taCtors) taKeys.forEach((k, ki) => taDescs.forEach((d, di) => {
  const zero = c === "BigInt64Array" ? "0n" : "0";
  thin(4, `T(()=>{var u=new ${c}(2);var r=Reflect.defineProperty(u,${k},${d});return r+' '+D(u,${k})+' '+u.length+' '+Object.keys(u).join()})`);
  thin(20, `T(()=>{var u=new ${c}(2);u[0]=${zero};Object.defineProperty(u,${k},${d});return D(u,${k})+' '+String(u[0])+String(u[1])})`);
}));
add(
  "T(()=>{var ab=new ArrayBuffer(4);var u=new Uint8Array(ab);ab.transfer();return Reflect.defineProperty(u,'0',{value:1})+' '+Reflect.defineProperty(u,'x',{value:1})+' '+Reflect.defineProperty(u,'0',{})})",
  "T(()=>{var ab=new ArrayBuffer(4);var u=new Uint8Array(ab);ab.transfer();Object.defineProperty(u,'0',{value:1})})", "T(()=>{var ab=new ArrayBuffer(4);var u=new Uint8Array(ab);ab.transfer();return Object.getOwnPropertyDescriptor(u,'0')+' '+Object.keys(u).length+' '+Object.isFrozen(u)+Object.isSealed(u)})",
  "T(()=>{var ab=new ArrayBuffer(2,{maxByteLength:8});var u=new Uint8Array(ab);ab.resize(0);return Reflect.defineProperty(u,'0',{value:1})+' '+Reflect.defineProperty(u,'-0',{value:1})})",
  "T(()=>{var ab=new ArrayBuffer(2,{maxByteLength:8});var u=new Uint8Array(ab);var r=Reflect.defineProperty(u,'3',{value:1});ab.resize(8);return r+' '+Reflect.defineProperty(u,'3',{value:1,writable:true,enumerable:true,configurable:true})+' '+u[3]})",
  "T(()=>{var ab=new ArrayBuffer(2,{maxByteLength:8});var u=new Uint8Array(ab);Object.freeze(u)})", "T(()=>{var ab=new ArrayBuffer(2,{maxByteLength:8});var u=new Uint8Array(ab,0,2);Object.freeze(u);return u.length})",
  "T(()=>{var ab=new ArrayBuffer(0,{maxByteLength:8});var u=new Uint8Array(ab);return Object.isFrozen(u)+' '+Object.isSealed(Object.preventExtensions(u))+' '+Object.isFrozen(Object.preventExtensions(u))})",
  "T(()=>{var ab=new ArrayBuffer(2,{maxByteLength:8});var u=new Uint8Array(ab,0,2);Object.seal(u);return Object.isFrozen(u)+' '+Object.isSealed(u)})",
  "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'0',{value:300});return u[0]})", "T(()=>{var u=new Uint8ClampedArray(2);Object.defineProperty(u,'0',{value:300});return u[0]})",
  "T(()=>{var u=new Float32Array(2);Object.defineProperty(u,'0',{value:0.1});return u[0]})", "T(()=>{var u=new Float64Array(2);Object.defineProperty(u,'1',{value:-0});return 1/u[1]})",
  "T(()=>{var u=new BigInt64Array(2);Object.defineProperty(u,'1',{value:2n**64n+3n});return u[1]})", "T(()=>{var u=new BigUint64Array(2);Object.defineProperty(u,'1',{value:-1n});return u[1]})",
  "T(()=>{var u=new BigInt64Array(2);Object.defineProperty(u,'1',{value:1})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'1',{value:1n})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'1',{value:Symbol()})})",
  "T(()=>{var log=[];var u=new Uint8Array(2);Reflect.defineProperty(u,'5',{value:{valueOf(){log.push('v');return 1}}});return log.join()})",
  "T(()=>{var log=[];var u=new Uint8Array(2);Reflect.defineProperty(u,'1',{value:{valueOf(){log.push('v');return 1}}});return log.join()})",
  "T(()=>{var log=[];var u=new Uint8Array(2);Reflect.defineProperty(u,'1',{value:{valueOf(){log.push('v');return 1}},get(){}});return log.join()})",
  "T(()=>{var u=new Uint8Array(2);var ab=u.buffer;Reflect.defineProperty(u,'1',{value:{valueOf(){ab.transfer();return 1}}});return u.length+' '+u.byteLength})",
  "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'x',{value:1});Object.defineProperty(u,'x',{value:2})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'x',{value:1,configurable:true});Object.defineProperty(u,'x',{get(){return 4}});return u.x})",
  "T(()=>{var u=Object.freeze(new Uint8Array(0));return Object.isFrozen(u)+' '+Object.getOwnPropertyNames(u).length})", "T(()=>{var u=new Uint8Array(0);Object.defineProperty(u,'x',{value:1});Object.freeze(u);return Object.isFrozen(u)})",
  "T(()=>{var u=new Uint8Array(0);Object.defineProperty(u,'x',{value:1,configurable:true});Object.freeze(u);return Object.isFrozen(u)+' '+D(u,'x')})",
  "T(()=>{var u=new Uint8Array(0);Object.defineProperty(u,'x',{value:1,configurable:true});Object.seal(u);return Object.isFrozen(u)+' '+Object.isSealed(u)+' '+D(u,'x')})",
  "T(()=>{var u=new Uint8Array(1);Object.defineProperty(u,'x',{value:1,configurable:true,writable:true});Object.seal(u);return Object.isFrozen(u)+' '+Object.isSealed(u)+' '+D(u,'x')})",
  "T(()=>{var u=new Uint8Array(1);Object.preventExtensions(u);return Reflect.defineProperty(u,'0',{value:3})+' '+Reflect.defineProperty(u,'1',{value:3})+' '+Reflect.defineProperty(u,'y',{value:3})+' '+u[0]})",
  "T(()=>{class U extends Uint8Array{}var u=new U(2);return Reflect.defineProperty(u,'0',{value:1})+' '+Reflect.defineProperty(u,'0',{value:1,writable:false})})",
);

// ---- 6. Funções: prototype, name, length.
const fnKinds = ["function f(a,b){}", "(a,b)=>1", "class C{static m(){}}", "(function f(){}).bind(null)", "async function f(){}", "function* g(){}", "({m(){}}).m", "Math.max", "class D extends Array{}", "class E{static name='x'}", "new Function('a','b','return 1')", "async function* ag(){}", "Object.assign(function(){},{x:1})"];
const fnKeys = ["'prototype'", "'name'", "'length'", "'caller'", "'arguments'", "'x'", "Symbol.hasInstance", "'m'"];
const fnDescs = ["{value:1}", "{value:'z'}", "{writable:true}", "{writable:false}", "{enumerable:true}", "{configurable:true}", "{configurable:false}", "{get(){}}", "{value:1,writable:true,enumerable:true,configurable:true}", "{}", "{value:2,writable:false,configurable:true}", "{set:S1,configurable:true}", "{value:undefined,configurable:true}"];
fnKinds.forEach((fk, fi) => fnKeys.forEach((k, ki) => fnDescs.forEach((d, di) => {
  thin(3, `T(()=>{var f=${fk};var r=Reflect.defineProperty(f,${k},${d});return r+' '+D(f,${k})})`);
  thin(18, `T(()=>{var f=${fk};Object.defineProperty(f,${k},${d});return Reflect.ownKeys(f).map(String).join()})`);
})));
add(
  "T(()=>{function f(){}Object.defineProperty(f,'prototype',{value:{x:1}});return new f().x})", "T(()=>{function f(){}Object.defineProperty(f,'prototype',{value:1});return Object.getPrototypeOf(new f())===Object.prototype})",
  "T(()=>{function f(){}Object.defineProperty(f,'prototype',{writable:false});f.prototype=3;return typeof f.prototype})", "T(()=>{'use strict';function f(){}Object.defineProperty(f,'prototype',{writable:false});f.prototype=3})",
  "T(()=>{function f(){}Object.defineProperty(f,'prototype',{get(){return {y:2}},configurable:true});return new f().y})", "T(()=>{function f(){}Object.defineProperty(f,'prototype',{get(){return {y:2}},configurable:true});return new f()instanceof f})",
  "T(()=>{function f(){}Object.defineProperty(f,'name',{value:'zz'});return f.name+' '+f.toString()})", "T(()=>{function f(){}Object.defineProperty(f,'length',{value:7});return f.length+' '+f.bind().length})",
  "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{value:'7'});return f.bind().length})", "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{value:-5});return f.bind().length})", "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{value:Infinity});return f.bind().length})",
  "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{value:2.7});return f.bind(null,1).length})", "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{get(){return 1},configurable:true});return f.bind().length})",
  "T(()=>{function f(a,b,c){}Object.defineProperty(f,'length',{value:1n,configurable:true});return f.bind().length})", "T(()=>{function f(a,b,c){}delete f.length;return f.bind().length+' '+f.length})", "T(()=>{function f(){}Object.defineProperty(f,'name',{value:7});return f.bind().name})",
  "T(()=>{function f(){}delete f.name;return f.bind().name+'|'+f.name})", "T(()=>{class A{static m(){}}return Reflect.defineProperty(A,'prototype',{value:A.prototype})+' '+Reflect.defineProperty(A,'prototype',{value:{}})})",
  "T(()=>{class A{}return Reflect.defineProperty(A,'prototype',{writable:true})+' '+Reflect.defineProperty(A,'prototype',{writable:false})+' '+D(A,'prototype')})", "T(()=>{class A{}Object.defineProperty(A,'name',{value:'B'});return A.name+' '+new A().constructor.name})",
  "T(()=>{class A{static name(){}}return D(A,'name')})", "T(()=>{class A{static length=3}return D(A,'length')})", "T(()=>{class A{static prototype2=1}return D(A,'prototype2')})", "T(()=>{try{eval('class A{static prototype=1}')}catch(e){return e.name+': '+e.message}})",
  "T(()=>{try{eval('class A{static prototype(){}}')}catch(e){return e.name+': '+e.message}})", "T(()=>{try{eval('class A{static get prototype(){}}')}catch(e){return e.name+': '+e.message}})",
  "T(()=>{var o={};o.f=function(){};return D(o.f,'name')})", "T(()=>{var o={f(){}};return D(o.f,'prototype')+' '+D(o.f,'name')})", "T(()=>{var o={get g(){return 1}};var f=Object.getOwnPropertyDescriptor(o,'g').get;return D(f,'name')+' '+D(f,'prototype')})",
  "T(()=>{return D(Function.prototype,'name')+' '+D(Function.prototype,'length')+' '+D(Function.prototype,'caller')})", "T(()=>Reflect.defineProperty(Function.prototype,'length',{value:3})+' '+Function.prototype.length)",
  "T(()=>Reflect.defineProperty(Function.prototype,'caller',{value:3}))", "T(()=>Reflect.defineProperty(function(){},'caller',{value:3}))", "T(()=>Reflect.defineProperty(function(){'use strict'},'caller',{value:3}))",
  "T(()=>{'use strict';var f=function(){};return Reflect.defineProperty(f,'arguments',{value:3})+' '+D(f,'arguments')})", "T(()=>{var f=function(){'use strict'};return Reflect.defineProperty(f,'arguments',{value:3})+' '+Object.hasOwn(f,'arguments')})",
  "T(()=>{var f=(function(){}).bind();return Reflect.defineProperty(f,'name',{value:'q',configurable:true})+' '+f.name+' '+D(f,'length')})", "T(()=>{var f=(function(){}).bind();return Reflect.ownKeys(f).map(String).join()})",
);

// ---- 7. globalThis.
const globalKeys = ["'undefined'", "'NaN'", "'Infinity'", "'Math'", "'JSON'", "'GN'", "'GV'", "'GF'", "'GL'", "Symbol.toStringTag", "'globalThis2'"];
const globalDescs = ["{value:1}", "{value:undefined}", "{value:NaN}", "{value:Infinity}", "{value:1,writable:true}", "{writable:true}", "{writable:false}", "{enumerable:true}", "{configurable:true}", "{configurable:false}", "{get(){return 1}}", "{}",
  "{value:1,writable:true,enumerable:true,configurable:true}", "{value:-0}", "{get:undefined}", "{set:S1,configurable:true}"];
const globalPre = "var GV=1;function GF(){}globalThis.GL=2;";
for (const k of globalKeys) for (const d of globalDescs) {
  add(`T(()=>{${globalPre}var r=Reflect.defineProperty(globalThis,${k},${d});return r+' '+D(globalThis,${k})})`);
  thin(3, `T(()=>{${globalPre}Object.defineProperty(globalThis,${k},${d});return D(globalThis,${k})})`);
}
add(
  "T(()=>{var o=Object.getOwnPropertyDescriptor(globalThis,'globalThis');return S(Object.keys(o))+o.writable+o.enumerable+o.configurable})", "T(()=>D(globalThis,'NaN')+' '+D(globalThis,'undefined')+' '+D(globalThis,'Infinity'))",
  "T(()=>{return Reflect.defineProperty(globalThis,'NaN',{value:NaN})+' '+Reflect.defineProperty(globalThis,'NaN',{value:-NaN})+' '+Reflect.defineProperty(globalThis,'undefined',{value:undefined})+' '+Reflect.defineProperty(globalThis,'Infinity',{value:Infinity})})",
  "T(()=>{Object.defineProperty(globalThis,'GQ',{get(){return 11},configurable:true});return GQ})", "T(()=>{Object.defineProperty(globalThis,'GQ2',{value:5});GQ2=6;return GQ2})", "T(()=>{'use strict';Object.defineProperty(globalThis,'GQ3',{value:5});GQ3=6})",
  "T(()=>{Object.defineProperty(globalThis,'GQ4',{value:5,configurable:true});delete globalThis.GQ4;return typeof GQ4})", "T(()=>{Object.defineProperty(globalThis,'GQ5',{value:5});return delete globalThis.GQ5})",
  "T(()=>{'use strict';Object.defineProperty(globalThis,'GQ6',{value:5});delete globalThis.GQ6})", "T(()=>{Object.defineProperty(globalThis,'GQ7',{value:5});return typeof GQ7+' '+('GQ7' in globalThis)+' '+Object.keys(globalThis).includes('GQ7')})",
  "T(()=>{var r=(0,eval)('var GE=1;Object.getOwnPropertyDescriptor(globalThis,\"GE\").configurable');return r})", "T(()=>{var r=(0,eval)('function GE2(){};Object.getOwnPropertyDescriptor(globalThis,\"GE2\").configurable');return r})",
  "T(()=>{var r=(0,eval)('let GE3=1;Object.getOwnPropertyDescriptor(globalThis,\"GE3\")');return r})", "T(()=>{Object.defineProperty(globalThis,'GQ8',{value:1});try{(0,eval)('var GQ8=2');return GQ8}catch(e){return e.name+': '+e.message}})",
  "T(()=>{Object.defineProperty(globalThis,'GQ9',{value:1});try{(0,eval)('function GQ9(){}');return typeof GQ9}catch(e){return e.name+': '+e.message}})", "T(()=>{Object.defineProperty(globalThis,'GQ10',{value:1,writable:true,enumerable:true});try{(0,eval)('function GQ10(){}');return typeof GQ10}catch(e){return e.name+': '+e.message}})",
  "T(()=>{Object.defineProperty(globalThis,'GQ11',{value:1,configurable:true});(0,eval)('function GQ11(){}');return D(globalThis,'GQ11').replace(/fn/,'F')})", "T(()=>{Object.defineProperty(globalThis,'GQ12',{value:1,writable:true,enumerable:true});(0,eval)('var GQ12');return D(globalThis,'GQ12')})",
  "T(()=>{Object.preventExtensions(Object.create(null));return Reflect.defineProperty(globalThis,'GQ13',{value:1})})",
);

// ---- 8. Proxy com trap defineProperty e invariantes.
const proxyTargets = ["{}", "Object.preventExtensions({})", "{p:1}", "Object.defineProperty({},'p',{value:1,configurable:false,writable:true})", "Object.defineProperty({},'p',{value:1,configurable:false,writable:false})", "Object.defineProperty({},'p',{get:G,configurable:false})",
  "Object.defineProperty({},'p',{get:G,configurable:true})", "Object.preventExtensions({p:1})", "Object.preventExtensions(Object.defineProperty({},'p',{value:1,writable:true,configurable:true}))", "Object.freeze({p:1})", "Object.seal({p:1})", "Object.defineProperty({},'p',{set:S1,configurable:false})"];
const proxyRets = ["true", "false", "undefined", "1", "0", "''", "'x'", "null", "{}", "NaN", "Symbol()"];
const proxyDescs = ["{value:1}", "{value:2,configurable:false}", "{value:1,configurable:false,writable:false}", "{get:G,configurable:false}", "{configurable:false}", "{configurable:true,value:1}", "{}", "{value:1,writable:false}", "{writable:false}", "{value:1,configurable:false,writable:true}", "{get:G,configurable:true}", "{enumerable:true}"];
proxyTargets.forEach((t, ti) => proxyRets.forEach((r, ri) => proxyDescs.forEach((d, di) => {
  thin(4, `T(()=>{var p=new Proxy(${t},{defineProperty(t,k,d){return ${r}}});return Reflect.defineProperty(p,'p',${d})})`);
  thin(12, `T(()=>{var p=new Proxy(${t},{defineProperty(t,k,d){return ${r}}});Object.defineProperty(p,'p',${d});return 'ok'})`);
})));
proxyTargets.forEach((t, ti) => proxyDescs.forEach((d, di) => {
  add(`T(()=>{var p=new Proxy(${t},{defineProperty(t,k,d){Reflect.defineProperty(t,k,d);return true}});return Reflect.defineProperty(p,'p',${d})+' '+D(t_(p),'p')})`.replace("t_(p)", "p"));
  thin(2, `T(()=>{var p=new Proxy(${t},{defineProperty(t,k,d){return true}});try{return Reflect.defineProperty(p,'p',${d})}catch(e){return e.name+': '+e.message}})`);
  thin(3, `T(()=>{var p=new Proxy(${t},{});return Reflect.defineProperty(p,'p',${d})+' '+D(p,'p')})`);
}));
const trapDescShapes = ["{value:1}", "{get:G}", "{get:undefined}", "{set:S1,enumerable:true}", "Object.create({value:1,writable:true})", "{value:1,enumerable:undefined}", "{value:1,get:undefined}", "{}", "{writable:false,enumerable:false,configurable:false}", "{configurable:true,enumerable:true,writable:true,value:3}",
  "Object.defineProperty({},'value',{get(){return 4},enumerable:false})", "new Proxy({value:2},{})", "{get:undefined,set:undefined}", "{enumerable:'yes',configurable:0}"];
for (const d of trapDescShapes) add(
  `T(()=>{var seen;var p=new Proxy({},{defineProperty(t,k,x){seen=x;return true}});Reflect.defineProperty(p,'p',${d});return S(Reflect.ownKeys(seen))+' '+S(Object.getPrototypeOf(seen)===Object.prototype)+S(seen)})`,
  `T(()=>{var seen;var p=new Proxy({},{defineProperty(t,k,x){seen=x;return true}});Reflect.defineProperty(p,'p',${d});var r=[];for(var k of Reflect.ownKeys(seen))r.push(k+':'+D(seen,k));return r.join('|')})`,
  `T(()=>{var p=new Proxy({},{defineProperty(t,k,x){return Reflect.defineProperty(t,k,x)}});var r=Reflect.defineProperty(p,'p',${d});return r+' '+D(p,'p')})`,
);
add(
  "T(()=>{var p=new Proxy({},{defineProperty:1});Object.defineProperty(p,'p',{value:1})})", "T(()=>{var p=new Proxy({},{defineProperty:'x'});Reflect.defineProperty(p,'p',{value:1})})", "T(()=>{var p=new Proxy({},{defineProperty:null});return Reflect.defineProperty(p,'p',{value:1})})",
  "T(()=>{var p=new Proxy({},{defineProperty:undefined});return Reflect.defineProperty(p,'p',{value:1})})", "T(()=>{var p=new Proxy({},{defineProperty:{}});Reflect.defineProperty(p,'p',{value:1})})", "T(()=>{var p=new Proxy({},{get defineProperty(){throw new RangeError('gt')}});Reflect.defineProperty(p,'p',{value:1})})",
  "T(()=>{var p=new Proxy({},{defineProperty(){throw new EvalError('tr')}});Reflect.defineProperty(p,'p',{value:1})})", "T(()=>{var r=Proxy.revocable({},{});r.revoke();Reflect.defineProperty(r.proxy,'p',{value:1})})", "T(()=>{var r=Proxy.revocable({},{});r.revoke();Object.defineProperty(r.proxy,'p',{value:1})})",
  "T(()=>{var r=Proxy.revocable({},{defineProperty(){return true}});r.revoke();Object.defineProperty(r.proxy,'p',1)})", "T(()=>{var r=Proxy.revocable({},{defineProperty(t,k,d){r.revoke();return true}});return Reflect.defineProperty(r.proxy,'p',{value:1})})",
  "T(()=>{var r=Proxy.revocable({},{defineProperty(t,k,d){r.revoke();return Reflect.defineProperty(t,k,d)}});return Reflect.defineProperty(r.proxy,'p',{value:1})+' '+Reflect.ownKeys(r.proxy)})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('dp '+String(k));return true},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return undefined},preventExtensions(t){log.push('pe');return Reflect.preventExtensions(t)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)}});Reflect.defineProperty(p,'p',{value:1});return log.join()})",
  "T(()=>{var log=[];var t={};var p=new Proxy(t,{defineProperty(t,k,d){log.push('dp '+String(k));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)}});Reflect.defineProperty(p,'p',{value:1});return log.join()})",
  "T(()=>{var log=[];var t=Object.defineProperty({},'p',{value:1,configurable:false});var p=new Proxy(t,{defineProperty(t,k,d){log.push('dp');return true},getOwnPropertyDescriptor(t,k){log.push('gopd');return Reflect.getOwnPropertyDescriptor(t,k)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)}});try{Reflect.defineProperty(p,'p',{value:1})}catch(e){log.push(e.message)}return log.join()})",
  "T(()=>{var log=[];var t=Object.defineProperty({},'p',{value:1,configurable:true});var p=new Proxy(t,{defineProperty(t,k,d){log.push('dp');return true},getOwnPropertyDescriptor(t,k){log.push('gopd');return Reflect.getOwnPropertyDescriptor(t,k)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)}});try{Reflect.defineProperty(p,'p',{value:1,configurable:false})}catch(e){log.push(e.message)}return log.join()})",
  "T(()=>{var p=new Proxy(new Proxy({},{defineProperty(t,k,d){return false}}),{});return Reflect.defineProperty(p,'p',{value:1})})", "T(()=>{var p=new Proxy(new Proxy({},{defineProperty(t,k,d){return false}}),{});Object.defineProperty(p,'p',{value:1})})",
  "T(()=>{var p=new Proxy(new Proxy({},{}),{defineProperty(t,k,d){return Reflect.defineProperty(t,k,d)}});return Reflect.defineProperty(p,'p',{value:1,enumerable:true})+' '+D(p,'p')})",
  "T(()=>{var p=new Proxy([],{defineProperty(t,k,d){return Reflect.defineProperty(t,k,d)}});Reflect.defineProperty(p,'length',{value:3});return p.length+' '+Array.isArray(p)})", "T(()=>{var p=new Proxy([],{defineProperty(t,k,d){return Reflect.defineProperty(t,k,d)}});p[2]=1;return p.length})",
  "T(()=>{var log=[];var p=new Proxy([],{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});p.push(1,2);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});p.a=1;p.a=2;return log.join()})", "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});Object.assign(p,{a:1,b:2});return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({a:1},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});p.a=2;return log.join()})", "T(()=>{var log=[];var p=new Proxy({get a(){return 1},set a(v){}},{defineProperty(t,k,d){log.push(String(k));return Reflect.defineProperty(t,k,d)}});p.a=2;return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});var o=Object.create(p);o.x=1;return log.join()+' '+Object.keys(o).join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});Reflect.set(p,'x',1,{});return log.join()})", "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});var recv={a:1};Reflect.set(p,'a',2,recv);return log.join()+' '+recv.a})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});var recv=new Proxy({a:1},{defineProperty(t,k,d){log.push('r '+String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});Reflect.set(p,'a',2,recv);return log.join()})",
  "T(()=>{var log=[];class A{constructor(){return new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}})}}class B extends A{x=1;static y=2}new B;return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});var o={...{a:1},__proto__:p};return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});Object.defineProperties(p,{a:{value:1},b:{get:G}});return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});Object.freeze(p);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({a:1,b:2},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)},ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},preventExtensions(t){log.push('pe');return Reflect.preventExtensions(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});Object.freeze(p);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({a:1,get b(){return 1}},{defineProperty(t,k,d){log.push(String(k)+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)},ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},preventExtensions(t){log.push('pe');return Reflect.preventExtensions(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});Object.seal(p);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({a:1},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},preventExtensions(t){log.push('pe');return Reflect.preventExtensions(t)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});var r=[Object.isFrozen(p),Object.isSealed(p)];return log.join()+' '+r})",
  "T(()=>{var log=[];var p=new Proxy(Object.freeze({a:1}),{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},isExtensible(t){log.push('ie');return Reflect.isExtensible(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});var r=[Object.isFrozen(p),Object.isSealed(p)];return log.join()+' '+r})",
  "T(()=>{var p=new Proxy({a:1},{preventExtensions(t){return false}});Object.freeze(p)})", "T(()=>{var p=new Proxy({a:1},{preventExtensions(t){return false}});return Reflect.preventExtensions(p)})", "T(()=>{var p=new Proxy({a:1},{preventExtensions(t){return true}});Object.seal(p)})",
  "T(()=>{var p=new Proxy({a:1},{defineProperty(t,k,d){return false}});Object.freeze(p)})", "T(()=>{var p=new Proxy({a:1},{defineProperty(t,k,d){return false}});Object.seal(p)})", "T(()=>{var p=new Proxy({a:1},{defineProperty(t,k,d){return false}});return Object.preventExtensions(p)===p})",
  "T(()=>{var p=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){return undefined}});Object.freeze(p);return Object.isFrozen(p)})", "T(()=>{var p=new Proxy({},{ownKeys(){return ['a']},getOwnPropertyDescriptor(){return {value:1,configurable:true}}});Object.freeze(p)})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['a']},getOwnPropertyDescriptor(){return {value:1,configurable:true}},defineProperty(){return true},preventExtensions(t){return Reflect.preventExtensions(t)}});Object.freeze(p);return 'ok'})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['a']},getOwnPropertyDescriptor(){return {get:G,configurable:true}},defineProperty(t,k,d){return true},preventExtensions(t){return true}});return Object.isFrozen(p)+' '+Object.isSealed(p)})",
);

// ---- 9. freeze / seal / isFrozen / isSealed em exóticos, com estados parciais.
const exoticTargets = {
  mappedArgs: "(function(a,b){return arguments})(1,2)", unmappedArgs: "(function(a,b){'use strict';return arguments})(1,2)", argsDeleted: "(function(a){delete arguments[0];return arguments})(1)",
  arrNonWritableLen: "Object.defineProperty([1,2],'length',{writable:false})", arrAccessor: "Object.defineProperty([1,2],0,{get:G,configurable:true})", arrSparse: "Object.defineProperty([],'4294967294',{value:1,configurable:true,writable:true,enumerable:true})",
  arrHuge: "Object.defineProperty([],'length',{value:4294967295})", arrNamed: "Object.assign([1],{x:1})", strObj: "Object.assign(new String('ab'),{x:1})", strEmpty: "new String('')", boundFn: "(function(a){}).bind(null)", asyncFn: "async function f(){}", genFn: "function* g(){}",
  classStatic: "class A{static get g(){return 1}static x=1}", classEmpty: "class B{}", nullProto: "Object.create(null,{a:{value:1,enumerable:true}})", symKeys: "{[Symbol.iterator]:G,[Symbol.for('s')]:1}", u8ab: "new Uint8Array(new ArrayBuffer(2,{maxByteLength:4}))",
  u8off: "new Uint8Array(new ArrayBuffer(4),2)", f32: "new Float32Array(3)", u8extra: "Object.assign(new Uint8Array(1),{x:1})", sab: "new Float64Array(0)", errObj: "Object.assign(new Error('m'),{x:1})", promise: "Promise.resolve(1)",
  weakmap: "new WeakMap", dateObj: "Object.assign(new Date(0),{x:1})", regexp: "/x/g", numObj: "new Number(1)", boolObj: "Object(true)", symObj: "Object(Symbol.iterator)", bigObj: "Object(1n)", mathLike: "Math", jsonObj: "JSON", reflectObj: "Reflect",
  fnProto: "Function.prototype", arrProto: "Array.prototype", ns: "Object.create(Object.create(null))",
};
const mutations = [
  "", "Object.defineProperty(x,'w',{value:1,writable:true,configurable:false});", "Object.defineProperty(x,'c',{value:1,writable:false,configurable:true});", "Object.defineProperty(x,'a',{get:G,configurable:false});",
  "Object.defineProperty(x,'a',{get:G,configurable:true});", "Object.defineProperty(x,'wc',{value:1,writable:true,configurable:true});", "Object.defineProperty(x,'ro',{value:1,writable:false,configurable:false});",
];
Object.entries(exoticTargets).forEach(([name, src], ni) => mutations.forEach((m, mi) => {
  (mi === 0 ? add : thin.bind(null, 2))(
    `T(()=>{var x=${src};${m}return [Object.isFrozen(x),Object.isSealed(x),Object.isExtensible(x)].join()})`,
    `T(()=>{var x=${src};${m}Object.preventExtensions(x);return [Object.isFrozen(x),Object.isSealed(x)].join()})`,
    `T(()=>{var x=${src};${m}Object.seal(x);return [Object.isFrozen(x),Object.isSealed(x)].join()+' '+Reflect.ownKeys(x).length})`,
    `T(()=>{var x=${src};${m}Object.freeze(x);return [Object.isFrozen(x),Object.isSealed(x)].join()})`,
  );
}));
for (const [name, src] of Object.entries(exoticTargets)) {
  if (["mathLike", "jsonObj", "reflectObj", "fnProto", "arrProto"].includes(name)) continue;
  add(
    `T(()=>{var x=${src};Object.freeze(x);var r=[];for(var k of Reflect.ownKeys(x)){if(typeof k==='symbol'||k.length<12)r.push(String(k)+':'+D(x,k))}return r.join(' | ')})`,
    `T(()=>{var x=${src};Object.seal(x);var r=[];for(var k of Reflect.ownKeys(x)){if(typeof k==='symbol'||k.length<12)r.push(String(k)+':'+D(x,k))}return r.join(' | ')})`,
    `T(()=>{var x=${src};Object.preventExtensions(x);return Reflect.defineProperty(x,'zz',{value:1})+' '+Reflect.defineProperty(x,'0',{value:1,writable:true,enumerable:true,configurable:true})+' '+Reflect.isExtensible(x)})`,
    `T(()=>{var x=${src};Object.freeze(x);return Reflect.defineProperty(x,'0',{value:9})+' '+Reflect.defineProperty(x,'length',{value:0})+' '+Reflect.defineProperty(x,'0',{})+' '+Reflect.defineProperty(x,'name',{configurable:false})})`,
    `T(()=>{var x=${src};Object.freeze(x);Object.freeze(x);Object.seal(x);return Object.isFrozen(x)+' '+Object.preventExtensions(x)===x})`,
    `T(()=>{var x=${src};Object.seal(x);for(var k of Reflect.ownKeys(x)){if(typeof k==='string'&&k.length>12)continue;var d=Object.getOwnPropertyDescriptor(x,k);if(d&&'value' in d&&d.writable)Reflect.defineProperty(x,k,{writable:false})}return Object.isFrozen(x)+' '+Object.isSealed(x)})`,
    `T(()=>{var x=${src};Object.seal(x);for(var k of Reflect.ownKeys(x)){if(typeof k==='string'&&k.length>12)continue;var d=Object.getOwnPropertyDescriptor(x,k);if(d&&'value' in d&&d.writable)Reflect.defineProperty(x,k,{writable:false});else if(d&&d.get)Reflect.defineProperty(x,k,{get:d.get})}return Object.isFrozen(x)})`,
  );
}
add(
  "T(()=>{var a=Object.freeze([1,2]);return Reflect.defineProperty(a,'0',{value:1})+' '+Reflect.defineProperty(a,'0',{value:2})+' '+Reflect.defineProperty(a,'length',{value:2})+' '+Reflect.defineProperty(a,'length',{value:1})+' '+Reflect.defineProperty(a,'2',{value:1})})",
  "T(()=>{var a=Object.seal([1,2]);return Reflect.defineProperty(a,'0',{value:5})+' '+Reflect.defineProperty(a,'0',{writable:false})+' '+Reflect.defineProperty(a,'0',{writable:true})+' '+Reflect.defineProperty(a,'0',{enumerable:false})+' '+Reflect.defineProperty(a,'length',{value:1})+' '+a.length})",
  "T(()=>{var a=Object.seal([1,2]);return Reflect.defineProperty(a,'length',{writable:false})+' '+Reflect.defineProperty(a,'length',{value:2})+' '+Object.isFrozen(a)+' '+Reflect.defineProperty(a,'0',{writable:false})+' '+Reflect.defineProperty(a,'1',{writable:false})+' '+Object.isFrozen(a)})",
  "T(()=>{var a=Object.preventExtensions([1,2]);return Reflect.defineProperty(a,'length',{value:1})+' '+a.length+' '+Reflect.defineProperty(a,'length',{value:5})+' '+a.length+' '+Reflect.defineProperty(a,'3',{value:1})+' '+Reflect.defineProperty(a,'0',{value:3})})",
  "T(()=>{var a=Object.preventExtensions([1,,3]);return Reflect.defineProperty(a,'1',{value:3})+' '+Reflect.defineProperty(a,'length',{value:1})+' '+Object.isFrozen(a)+' '+Object.isSealed(a)})",
  "T(()=>{var a=Object.preventExtensions([,,]);return Object.isFrozen(a)+' '+Object.isSealed(a)+' '+a.length})", "T(()=>{var a=Object.preventExtensions([,,]);Object.defineProperty(a,'length',{writable:false});return Object.isFrozen(a)+' '+Object.isSealed(a)})",
  "T(()=>{var a=Object.preventExtensions([]);return Object.isFrozen(a)+' '+Object.isSealed(a)})", "T(()=>{var a=Object.preventExtensions([]);Object.defineProperty(a,'length',{writable:false});return Object.isFrozen(a)+' '+Object.isSealed(a)})",
  "T(()=>{var a=Object.preventExtensions([1]);Object.defineProperty(a,'length',{writable:false});return Object.isFrozen(a)+' '+Object.isSealed(a)})", "T(()=>{var a=Object.seal([1]);Object.defineProperty(a,'length',{writable:false});return Object.isFrozen(a)+' '+Object.isSealed(a)})",
  "T(()=>{var a=Object.seal([1]);Object.defineProperty(a,'0',{writable:false});return Object.isFrozen(a)+' '+Object.isSealed(a)})", "T(()=>{var a=Object.seal([1]);Object.defineProperty(a,'0',{writable:false});Object.defineProperty(a,'length',{writable:false});return Object.isFrozen(a)})",
  "T(()=>{var o=Object.seal({a:1});Object.defineProperty(o,'a',{writable:false});return Object.isFrozen(o)})", "T(()=>{var o=Object.seal({a:1});Object.defineProperty(o,'a',{get:G});return Object.isFrozen(o)+' '+D(o,'a')})",
  "T(()=>{var o=Object.seal({get a(){return 1}});Object.defineProperty(o,'a',{value:1});return Object.isFrozen(o)+' '+Object.isSealed(o)+' '+D(o,'a')})", "T(()=>{var o=Object.seal({get a(){return 1}});return Reflect.defineProperty(o,'a',{value:1,writable:false})+' '+D(o,'a')})",
  "T(()=>{var o=Object.seal({a:1});return Reflect.defineProperty(o,'a',{get:G})+' '+D(o,'a')})", "T(()=>{var o=Object.freeze({a:1});return Reflect.defineProperty(o,'a',{value:1})+' '+Reflect.defineProperty(o,'a',{value:2})+' '+Reflect.defineProperty(o,'a',{get:G})+' '+Reflect.defineProperty(o,'a',{writable:true})})",
  "T(()=>{var o=Object.freeze({get a(){return 1}});return Reflect.defineProperty(o,'a',{get:o.__lookupGetter__('a')})+' '+Reflect.defineProperty(o,'a',{get:G})+' '+Reflect.defineProperty(o,'a',{set:undefined})+' '+Reflect.defineProperty(o,'a',{set:S1})})",
  "T(()=>{var o=Object.freeze(Object.create(null));return Object.isFrozen(o)+' '+Reflect.ownKeys(o).length})", "T(()=>{var o=Object.freeze({});o.x=1;return Object.isFrozen(o)+' '+Object.keys(o).length})",
  "T(()=>Object.isFrozen(Object.freeze(function(){})))", "T(()=>Object.isFrozen(Object.freeze(class{static x=1})))", "T(()=>Object.isSealed(Object.seal(async()=>1)))", "T(()=>Object.isFrozen(Object.seal(()=>1)))",
  "T(()=>{var f=function(){};Object.defineProperty(f,'prototype',{writable:false});Object.defineProperty(f,'name',{writable:false});Object.defineProperty(f,'length',{writable:false});Object.seal(f);return Object.isFrozen(f)+' '+Object.isSealed(f)})",
  "T(()=>{var f=()=>1;Object.defineProperty(f,'name',{writable:false});Object.defineProperty(f,'length',{writable:false});Object.preventExtensions(f);return Object.isFrozen(f)+' '+Object.isSealed(f)})",
  "T(()=>{var f=()=>1;Object.defineProperty(f,'name',{writable:false,configurable:false});Object.defineProperty(f,'length',{writable:false,configurable:false});Object.preventExtensions(f);return Object.isFrozen(f)+' '+Object.isSealed(f)})",
  "T(()=>{var f=()=>1;delete f.name;delete f.length;Object.preventExtensions(f);return Object.isFrozen(f)+' '+Object.isSealed(f)+' '+Reflect.ownKeys(f).length})",
  "T(()=>{var x=new Number(1);Object.freeze(x);x.valueOf=null;return typeof x.valueOf})", "T(()=>{var f=Object.freeze(function(){});return Object.isFrozen(f.prototype)})", "T(()=>{class A{}Object.freeze(A);return Object.isFrozen(A.prototype)+' '+Object.isFrozen(A)})",
  "T(()=>{var a=Object.freeze(Object.assign([1],{[Symbol.iterator]:G}));return Object.isFrozen(a)+' '+Reflect.defineProperty(a,Symbol.iterator,{value:G})+' '+Reflect.defineProperty(a,Symbol.iterator,{value:G2})})",
  "T(()=>{var s=Symbol('q');var o=Object.seal({[s]:1});return Object.isFrozen(o)+' '+Reflect.defineProperty(o,s,{writable:false})+' '+Object.isFrozen(o)})",
);

// ---- 10. Object.defineProperties / Object.create: ordem de efeitos.
const propsSets = [
  "{a:{value:1},b:{value:2}}", "{a:{value:1},b:2}", "{a:{value:1},b:{get:1}}", "{b:{value:1},a:{value:2},1:{value:3},0:{value:4}}", "{[Symbol.for('s')]:{value:1},z:{value:2},3:{value:3}}",
  "{a:{value:1,enumerable:true},a2:{get:G,enumerable:true}}", "{a:{value:1},a:{value:2}}", "{a:{get value(){throw new EvalError('x')}},b:{value:2}}", "{a:{value:1},b:{get value(){throw new EvalError('x')}}}",
  "{a:{value:1},get b(){throw new RangeError('b')}}", "{get a(){this.zz={value:9};return {value:1}},b:{value:2}}", "{get a(){delete this.b;return {value:1}},b:{value:2}}", "{a:{value:1},get b(){Object.defineProperty(this,'c',{value:{value:5},enumerable:true});return {value:2}}}",
  "{get a(){return null}}", "{a:undefined}", "{a:'str'}", "{a:{value:1,get:G}}", "{a:{configurable:false},b:{get:G,value:1}}", "{a:{value:1},[Symbol.iterator]:{get:1}}",
  "Object.defineProperty({x:{value:1}},'hid',{value:{value:2},enumerable:false})", "Object.create({inh:{value:1}},{own:{value:{value:2},enumerable:true}})", "new Proxy({a:{value:1}},{ownKeys(){return ['a','a']}})", "new Proxy({a:{value:1}},{ownKeys(){return ['a','b']}})",
  "new Proxy({a:{value:1}},{getOwnPropertyDescriptor(){return undefined}})", "new Proxy({a:{value:1}},{getOwnPropertyDescriptor(t,k){return {value:t[k],configurable:true,enumerable:false}}})", "new Proxy({a:{value:1}},{get(){return {value:'g'}}})", "new Proxy({a:{value:1}},{get(){throw new URIError('g')}})",
  "new Proxy({a:{value:1}},{ownKeys(){throw new URIError('ok')}})", "new Proxy({a:{value:1}},{ownKeys(){return [1]}})", "'ab'", "[{value:1},{value:2}]", "Object.assign([],{3:{value:1}})", "new Map", "function(){}", "Object.assign(function(){},{a:{value:1}})",
];
const dpTargets = ["{}", "[]", "[1,2,3]", "function(){}", "Object.preventExtensions({})", "{a:1}", "Object.freeze({a:1})", "new Proxy({},{defineProperty(t,k,d){log.push('dp '+String(k));return Reflect.defineProperty(t,k,d)}})", "new Proxy({},{defineProperty(t,k,d){log.push('dp '+String(k));return k!=='b'}})", "new Uint8Array(2)", "Object.defineProperty({},'a',{value:0})"];
propsSets.forEach((ps, pi) => dpTargets.forEach((t, ti) => {
  (ti > 1 ? thin.bind(null, 2) : add)(`T(()=>{var log=[];var o=${t};var r;try{Object.defineProperties(o,${ps});r='ok'}catch(e){r=e.name+': '+e.message}return r+' | '+log.join()+' | '+Reflect.ownKeys(o).map(k=>String(k)+':'+(log.length?'':D(o,k))).join()})`);
}));
propsSets.forEach(ps => add(
  `T(()=>{var o;try{o=Object.create({},${ps});}catch(e){return e.name+': '+e.message}return Reflect.ownKeys(o).map(String).join()})`,
  `T(()=>{var log=[];var o;var props=${ps};try{o=Object.create(Object.prototype,props)}catch(e){return e.name+': '+e.message}return Reflect.ownKeys(o).map(k=>String(k)+':'+D(o,k)).join(' | ')})`,
));
add(
  "T(()=>{var log=[];var props=new Proxy({a:{value:1},b:{value:2},1:{value:3}},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});var o=new Proxy({},{defineProperty(t,k,d){log.push('dp '+k);return Reflect.defineProperty(t,k,d)}});Object.defineProperties(o,props);return log.join()})",
  "T(()=>{var log=[];var props=new Proxy({a:{value:1},b:{value:2}},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});Object.create(null,props);return log.join()})",
  "T(()=>{var log=[];var d1=new Proxy({value:1},{has(t,k){log.push('has '+k);return k in t},get(t,k){log.push('get '+k);return t[k]}});var d2=new Proxy({get:G},{has(t,k){log.push('has2 '+k);return k in t},get(t,k){log.push('get2 '+k);return t[k]}});Object.defineProperties({},{a:d1,b:d2});return log.join()})",
  "T(()=>{var log=[];var props={};['z','y',2,1,Symbol.for('s')].forEach(k=>Object.defineProperty(props,k,{get(){log.push('get '+String(k));return {value:1}},enumerable:true}));Object.defineProperties({},props);return log.join()})",
  "T(()=>{var log=[];var props={};['z','y',2,1,Symbol.for('s')].forEach(k=>Object.defineProperty(props,k,{get(){log.push('get '+String(k));return {get value(){log.push('v '+String(k));return 1}}},enumerable:true}));Object.defineProperties({},props);return log.join()})",
  "T(()=>{var log=[];var props={a:{get value(){log.push('a.v');return 1}},b:{get value(){log.push('b.v');throw new Error('stop')}},c:{get value(){log.push('c.v');return 3}}};var o={};try{Object.defineProperties(o,props)}catch(e){log.push(e.message)}return log.join()+' '+Reflect.ownKeys(o).join()})",
  "T(()=>{var log=[];var o=new Proxy({},{defineProperty(t,k,d){log.push('dp '+k);if(k==='b')return false;return Reflect.defineProperty(t,k,d)}});try{Object.defineProperties(o,{a:{value:1},b:{value:2},c:{value:3}})}catch(e){log.push(e.name+': '+e.message)}return log.join()+' '+Reflect.ownKeys(o).join()})",
  "T(()=>{var log=[];var o=new Proxy({},{defineProperty(t,k,d){log.push('dp '+k);return Reflect.defineProperty(t,k,d)}});try{Object.defineProperties(o,{a:{value:1},b:{get:1},c:{value:3}})}catch(e){log.push(e.name+': '+e.message)}return log.join()})",
  "T(()=>{var o={};Object.defineProperties(o,{a:{value:1,configurable:true},b:{get(){return this.a},configurable:true}});return o.b+' '+Object.keys(o).length+' '+D(o,'b')})",
  "T(()=>{var a=[1,2,3];try{Object.defineProperties(a,{length:{value:1},5:{value:9,enumerable:true,configurable:true,writable:true}})}catch(e){return e.name+': '+e.message}return a.length+' '+Object.keys(a).join()})",
  "T(()=>{var a=[1,2,3];Object.defineProperties(a,{length:{value:1,writable:false},0:{value:7}});return a.length+' '+a[0]+' '+D(a,'length')})", "T(()=>{var a=[1,2,3];try{Object.defineProperties(a,{length:{value:1,writable:false},0:{value:7},3:{value:7}})}catch(e){return e.name+': '+e.message}return a.length})",
  "T(()=>{var a=[1,2,3];try{Object.defineProperties(a,{3:{value:7},length:{writable:false},4:{value:1}})}catch(e){return e.name+': '+e.message}return a.length+' '+S(a)})", "T(()=>{var a=[];Object.defineProperties(a,{b:{value:1},0:{value:2,enumerable:true},a:{value:3}});return a.length+' '+Reflect.ownKeys(a).join()})",
  "T(()=>{var o={};Object.defineProperties(o,{b:{value:1,enumerable:true},a:{value:2,enumerable:true},1:{value:3,enumerable:true},0:{value:4,enumerable:true}});return Object.keys(o).join()})", "T(()=>Object.defineProperties({},{}))", "T(()=>Object.defineProperties({a:1},{a:{enumerable:false}}).propertyIsEnumerable('a'))",
  "T(()=>{var o={};var r=Object.defineProperties(o,{});return r===o})", "T(()=>Object.defineProperties(Object.freeze({a:1}),{}))", "T(()=>Object.defineProperties(Object.freeze({a:1}),{a:{value:1}}).a)", "T(()=>Object.defineProperties(Object.freeze({a:1}),{a:{value:2}}))",
  "T(()=>Object.defineProperties(Object.freeze({a:1}),{b:{}}))", "T(()=>Object.defineProperties(Object.preventExtensions({a:1}),{a:{value:2},b:{value:3}}))", "T(()=>{var o=Object.preventExtensions({a:1});try{Object.defineProperties(o,{a:{value:2},b:{value:3}})}catch(e){}return o.a})",
  "T(()=>{var o=Object.preventExtensions({a:1});try{Object.defineProperties(o,{b:{value:3},a:{value:2}})}catch(e){}return o.a})", "T(()=>{var o={};Object.defineProperty(o,'a',{value:1});try{Object.defineProperties(o,{b:{value:3},a:{value:2}})}catch(e){}return Object.getOwnPropertyNames(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1});try{Object.defineProperties(o,{a:{value:2},b:{value:3}})}catch(e){}return Object.getOwnPropertyNames(o).join()})", "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:null})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})",
  "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{get:1}})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})", "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{get:G,value:1}})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})",
  "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{set:1}})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})", "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{value:1,set:S1}})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})",
  "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{writable:true,get:G}})}catch(e){return e.name+': '+e.message+' '+Object.getOwnPropertyNames(o).join()}})",
  "T(()=>Object.defineProperties.length+Object.defineProperties.name+Object.defineProperty.length+Reflect.defineProperty.length+Object.create.length)",
  "T(()=>{var o={};Object.defineProperties(o,{a:{value:1,configurable:true}});Object.defineProperties(o,{a:{get(){return 2}}});return o.a+' '+D(o,'a')})",
  "T(()=>{var o={};Object.defineProperties(o,{a:{get:G,configurable:true}});Object.defineProperties(o,{a:{value:2}});return o.a+' '+D(o,'a')})",
  "T(()=>{var o=Object.create(Object.defineProperties({},{x:{value:1}}),{y:{value:2}});o.x=5;return o.x+' '+o.y+' '+Object.getOwnPropertyNames(o).join()})",
  "T(()=>{'use strict';var o=Object.create(Object.defineProperties({},{x:{value:1}}));o.x=5})", "T(()=>{'use strict';var o=Object.create(Object.defineProperties({},{x:{get:G}}));o.x=5})", "T(()=>{var o=Object.create(Object.defineProperties({},{x:{set:S1,enumerable:true}}));o.x=5;return Object.keys(o).length+D(o,'x')})",
  "T(()=>{var o=Object.create(Object.defineProperties({},{x:{value:1,writable:true}}));o.x=5;return D(o,'x')})", "T(()=>{var o=Object.create(Object.defineProperties({},{x:{value:1,writable:false,configurable:true}}));o.x=5;return D(o,'x')})",
  "T(()=>{var o=Object.create(Object.defineProperties({},{x:{value:1,writable:false,configurable:true}}));return Reflect.defineProperty(o,'x',{value:5})+' '+D(o,'x')})",
);

// ---- Execução, com deduplicação contra os goldens existentes e concorrência controlada.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const existing = new Set();
for (const src of knownPrograms("define_own_property_bun.tsv", (file) => file !== "define_own_property_bun.tsv")) {
  const at = src.indexOf("globalThis.R = ");
  if (at >= 0) existing.add(src.slice(at + 15));
}
const seen = new Set();
let dup = 0;
// A matriz inteira passa de 7 mil programas, o golden fica com cerca de 43% dela.
// A amostra (cerca de 43% do conjunto candidato sem repetição) sai antes de descontar os goldens vizinhos.
const candidates = pool.resolve().filter(e => {
  if (seen.has(e)) return false;
  seen.add(e);
  return true;
});
const unique = sampleByHash(candidates, Math.ceil(candidates.length * 0.43)).filter(e => {
  if (existing.has(e)) { dup++; return false; }
  return true;
});

const runOne = expr => new Promise(resolve => {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
  const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
  let out = "", err = "";
  child.stdout.on("data", d => (out += d));
  child.stderr.on("data", d => (err += d));
  const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
  child.on("close", status => {
    clearTimeout(timer);
    resolve({ expr, source, status, out, err });
  });
  child.stdin.end(source);
});

(async () => {
  const results = new Array(unique.length);
  let next = 0;
  const worker = async () => {
    while (next < unique.length) {
      const i = next++;
      results[i] = await runOne(unique[i]);
    }
  };
  await Promise.all(Array.from({ length: 10 }, worker));
  let kept = 0, dropped = 0;
  const rows = [];
  const outSeen = new Set();
  for (const r of results) {
    if (r.status !== 0) { dropped++; process.stderr.write("filho falhou: " + JSON.stringify(r.expr).slice(0, 160) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("caminho ou marca: " + JSON.stringify(r.expr).slice(0, 160) + "\n"); continue; }
    if (r.out === "undefined") { dropped++; process.stderr.write("R indefinido: " + JSON.stringify(r.expr).slice(0, 160) + "\n"); continue; }
    kept++;
    rows.push({ source: r.source, result: r.out });
  }
  process.stdout.write(emitFactored("define_own_property", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
