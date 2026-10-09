// Gera tests/golden/array_combo_bun.tsv: combinatórias de Array.prototype ainda não cobertas por array_bun,
// array_edge_bun e array_more_bun, medido no bun 1.4.2.
// Cobre sort/toSorted com comparadores inconsistentes (NaN, boolean, símbolo, mutação do array, undefined, buracos,
// getters), estabilidade, arrays esparsos e array-likes, concat/slice/splice/map/filter com Symbol.isConcatSpreadable e
// species, indexOf/lastIndexOf/includes com -0, NaN e fromIndex extremo, join/toString/toLocaleString com ciclos, flat e
// flatMap com depth extremo, Array.from com iterável, array-like, mapFn e this, Array.of em subclasses, o setter de
// length (RangeError, truncamento, não configurável), buracos nos métodos de iteração e arrays com 2**32-1 elementos
// virtuais. Programas cujo corpo já aparece nos três goldens de array são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-array-edge-golden.js, com o
// mesmo prelúdio (`S`, `T`, `D`). Cada programa roda num processo bun novo, sem APIs de host.
// Uso: bun scripts/gen-array-combo-golden.js > tests/golden/array_combo_bun.tsv
const fs = require("fs");
const { emitFactoredLines, knownProgramSet, prepareProgram, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

const PRELUDE = [
  "function S(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'function') return 'fn';",
  "  if (v === null || typeof v !== 'object') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (d > 3) return '...';",
  "  if (Array.isArray(v)) {",
  "    var o = [], n = Math.min(v.length, 40);",
  "    for (var i = 0; i < n; i++) o.push(i in v ? S(v[i], d + 1) : '<hole>');",
  "    return '[' + o.join(',') + ']#' + v.length + (Object.getPrototypeOf(v) === Array.prototype ? '' : '~sub');",
  "  }",
  "  if (ArrayBuffer.isView(v)) return Object.prototype.toString.call(v) + '[' + Array.prototype.join.call(v, ',') + ']';",
  "  return '{' + Object.keys(v).slice(0, 20).map(function (k) { return k + ':' + S(v[k], d + 1); }).join(',') + '}';",
  "}",
  "function T(f) { try { return S(f()); } catch (e) { return 'throw ' + e.name + ': ' + e.message; } }",
  "function D(it) { var o = []; for (var x of it) { o.push(S(x)); if (o.length > 40) break; } return o.join(';'); }",
].join("\n");

const programs = [];
const E = expr => programs.push(`R = T(function () { return (${expr}); });`);
const F = (...rows) => programs.push(`R = T(function () {\n${rows.join("\n")}\n});`);

const MAX = "9007199254740991"; // 2**53-1
const P = "Array.prototype";

// ---- 1. sort/toSorted com comparadores de todo tipo sobre entradas pequenas.
const comparators = [
  "undefined", "(a,b)=>a-b", "(a,b)=>b-a", "()=>NaN", "()=>0", "()=>1", "()=>-1", "(a,b)=>a>b", "(a,b)=>a<b", "(a,b)=>a>b?1:-1",
  "()=>true", "()=>false", "()=>undefined", "()=>null", "()=>'1'", "()=>'-1'", "()=>({valueOf(){return -1}})", "()=>Symbol()",
  "()=>1n", "()=>-Infinity", "()=>Infinity", "()=>0.5", "()=>-0", "null", "1", "{}", "'x'", "Symbol()", "class{}", "true",
  "function(){throw new RangeError('c')}", "async (a,b)=>0", "()=>[]", "()=>[-1]", "()=>({})", "(a,b)=>String(a)<String(b)?-1:1",
];
const sortInputs = [
  "[3,1,2]", "[3,1,2,undefined]", "[undefined,3,,1]", "[,,1]", "['b','a','B','A',10,9,1]", "[5,1,4,2,3,9,8,7,6,0,11,10]", "[2,1]",
  "[1]", "[]", "[NaN,3,NaN,1]", "[null,undefined,0,-0,'']", "[true,false,'a',1]",
];
for (const cmp of comparators) for (const input of sortInputs) {
  E(`${input}.sort(${cmp})`);
  E(`${input}.toSorted(${cmp})`);
}
// Pares que o comparador recebe, em ordem: fixa o algoritmo de ordenação, não só o resultado.
const logInputs = ["[3,1,2]", "[1,2,3]", "[2,1]", "[4,3,2,1]", "[1,2,3,4,5]", "[5,4,3,2,1]", "[3,1,4,1,5,9,2,6]", "[2,2,2]"];
const logComparators = ["(a,b)=>a-b", "(a,b)=>b-a", "()=>NaN", "()=>0", "(a,b)=>a>b", "()=>1", "()=>-1", "(a,b)=>a<b?-1:0"];
for (const input of logInputs) for (const cmp of logComparators) {
  F(`var log=[];var r=${input}.sort(function(a,b){log.push(a+':'+b);return (${cmp})(a,b)});`, `return [r,log.join()];`);
  F(`var log=[];var r=${input}.toSorted(function(a,b){log.push(a+':'+b);return (${cmp})(a,b)});`, `return [r,log.join()];`);
}

// ---- 2. o comparador muda o array durante a ordenação.
const mutators = [
  "a.push(99)", "a.pop()", "a.length=0", "a.length=1", "a.reverse()", "a.shift()", "a.unshift(7)", "delete a[0]", "delete a[1]",
  "a[7]=5", "a.sort()", "a.splice(0,1)", "Object.freeze(a)", "a.fill(0)", "a.length=10",
];
for (const m of mutators) for (const init of ["[5,3,4,1,2]", "[3,1,2]"]) {
  F(`var a=${init};var n=0;var r;`, `try{a.sort(function(x,y){if(n++===1){${m}}return x-y})}catch(e){r=e.name}`, `return [a,r,n];`);
  F(`var a=${init};var n=0;var r;`, `try{r=a.toSorted(function(x,y){if(n++===1){${m}}return x-y})}catch(e){r=e.name}`, `return [a,r,n];`);
}

// ---- 3. this variado em sort e toSorted.
const sortThis = [
  "null", "undefined", "1", "'str'", "true", "Symbol()", "{}", "{length:3,0:'c',1:'a',2:'b'}", "{length:2,0:'b'}", "{length:-1}",
  "{length:'3',0:3,1:1,2:2}", "{length:3.9,0:3,1:1,2:2}", "new String('cba')", "function(){}", "[3,1,2]", "new Proxy([3,1,2],{})",
  "Object.freeze([2,1])", "Object.freeze([1])", "Object.freeze([])", "Object.seal([2,1])", "Object.preventExtensions([2,,1])",
  "{length:3,0:3,2:1}", "{length:{valueOf(){return 2}},0:'b',1:'a'}", "{length:NaN,0:1}", "{length:'x',0:1}", "{0:1,1:0}",
];
for (const t of sortThis) {
  F(`var t=${t};var r=${P}.sort.call(t);return [r===t,t,typeof t==='object'||typeof t==='function'?Object.keys(t).join():'']`);
  F(`var t=${t};var r=${P}.toSorted.call(t);return [r,r===t]`);
  F(`var t=${t};var r=${P}.sort.call(t,(a,b)=>b>a?1:-1);return [t]`);
}
// holes, protótipo e getters.
for (const proto of ["'p'", "0", "undefined", "99"]) for (const input of ["[3,,1]", "[,3,1]", "[3,1,,]", "[,,]", "[3,undefined,1,,0]"]) {
  F(`${P}[1]=${proto};try{var a=${input};var r=a.sort();var k=Object.keys(a).join();var s=S(a)}finally{delete ${P}[1]}`, `return [s,k]`);
  F(`${P}[1]=${proto};try{var r=${input}.toSorted()}finally{delete ${P}[1]}`, `return [r]`);
}
F(`var log=[];var a=[];Object.defineProperty(a,0,{get(){log.push('g0');return 2},set(v){log.push('s0:'+v)},enumerable:true,configurable:true});a[1]=1;a.sort();`, `return [log,a.length]`);
F(`var log=[];var a=[];Object.defineProperty(a,1,{get(){log.push('g1');return 0},set(v){log.push('s1:'+v)},enumerable:true,configurable:true});a[0]=1;a[2]=-1;a.sort();`, `return [log,a.length]`);
F(`var log=[];var a=[3,2,1];Object.defineProperty(a,1,{get(){log.push('g1');return 2},configurable:true});try{a.sort()}catch(e){log.push(e.name)}`, `return [log,a]`);
F(`var log=[];var a=[3,2,1];Object.defineProperty(a,1,{value:2,writable:false,configurable:true});try{a.sort()}catch(e){log.push(e.name)}`, `return [log,a]`);
F(`var a=[3,2,1];Object.defineProperty(a,1,{value:2,writable:false,configurable:false});try{a.sort()}catch(e){return [e.name,a]}return a`);
F(`var a=[3,2,1];Object.defineProperty(a,1,{get(){throw new EvalError('g')}});return [a.toSorted]&&0`);
F(`var a=[3,2,1];Object.defineProperty(a,1,{get(){throw new EvalError('g')}});a.toSorted()`);
F(`var a=[3,2,1];Object.defineProperty(a,1,{get(){throw new EvalError('g')}});a.sort()`);
F(`var n=0;var a=[undefined,3,undefined,1,,2];a.sort((x,y)=>{n++;if(x===undefined||y===undefined)throw 1;return x-y});`, `return [a,n]`);
F(`var a=[undefined,,3,undefined,,1];return [a.sort((x,y)=>y-x),Object.keys(a).join()]`);
F(`var a=[undefined,,3,undefined,,1];return [a.toSorted((x,y)=>y-x),a]`);
F(`var a=[3,2,1];a.sort(function(){return this===undefined});return [a]`);
F(`var self;[2,1].sort(function(){self=this;return 0});return self===undefined`);
F(`'use strict';var self;[2,1].sort(function(){self=this;return 0});return self===undefined`);
F(`var args;[2,1].sort(function(){args=arguments.length;return 0});return args`);
F(`var a=[1,2,3];a.sort(()=>{throw new URIError('u')});`);
F(`var a=[3,2,1];try{a.sort(()=>{throw new URIError('u')})}catch(e){}return a`);
F(`var a=[3,2,1];try{a.sort(()=>{throw new URIError('u')})}catch(e){}return a`);

// ---- 4. estabilidade: tamanhos e chaves variados, resultado como lista de ids.
for (const n of [5, 10, 11, 12, 13, 16, 17, 20, 32, 33, 50, 100, 129, 300]) for (const mod of [1, 2, 3, 5]) {
  const build = `var a=[];for(var i=0;i<${n};i++)a.push({k:(i*7)%${mod},i:i});`;
  F(build, `return a.sort((x,y)=>x.k-y.k).map(x=>x.i).join()`);
  F(build, `return a.toSorted((x,y)=>y.k-x.k).map(x=>x.i).join()`);
  F(build, `return a.sort((x,y)=>x.k>y.k?1:x.k<y.k?-1:0).map(x=>x.k+':'+x.i).join()`);
  F(build, `return a.sort((x,y)=>(x.k>y.k)-0).map(x=>x.i).join()`);
}
for (const n of [3, 8, 24, 64]) {
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i%2?'b':'B');`, `return a.sort((x,y)=>x.toLowerCase()<y.toLowerCase()?-1:x.toLowerCase()>y.toLowerCase()?1:0).join('')`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `return a.sort(()=>0).join()`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `return a.sort(()=>-1).join()`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `return a.sort(()=>1).join()`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `return a.sort(()=>NaN).join()`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(${n}-i);`, `return a.sort().join()`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `var c=0;a.sort((x,y)=>{c++;return y-x});return c`);
  F(`var a=[];for(var i=0;i<${n};i++)a.push(i);`, `var c=0;a.sort((x,y)=>{c++;return x-y});return c`);
}

// ---- 5. sort de arrays esparsos e array-likes com length variado.
const shapes = [
  "var a=[];a[1000]=2;a[5]=1;a[100000]=0;", "var a=[];a[3]=1;a[1]=3;", "var a=[,'b',,'a'];a.length=10;", "var a=new Array(5);a[2]=1;",
  "var a=[];a[2**31]=1;a[0]=2;", "var a=[];a[4294967294]=1;a[0]=2;", "var a=[3,2,1];a.length=100;", "var a=[];a.length=50;a[49]=1;a[0]=2;",
  "var a=[1,2,3];a[10]=0;a[20]=undefined;", "var a=[];a[1e6]='z';a[1]='a';a.x='k';",
];
for (const shape of shapes) for (const m of ["sort()", "sort((x,y)=>y>x?1:-1)", "toSorted()", "toSorted((x,y)=>x-y)"]) {
  F(shape, `var r=a.${m};return [Object.keys(r).slice(0,12).join(),r.length,r[0],r[1]]`);
}
for (const len of ["0", "1", "3", "'3'", "-1", "NaN", "'x'", "3.9", "{valueOf(){return 3}}", "6", "1e3", "'0x10'", "true", "null", "undefined"]) {
  F(`var o={length:${len},0:'c',2:'a',5:'z'};${P}.sort.call(o);`, `return [Object.keys(o).join(), o.length, o[0], o[1]]`);
  F(`var o={length:${len},0:'c',2:'a',5:'z'};var r=${P}.toSorted.call(o);`, `return [r,Object.keys(o).join()]`);
  F(`var o={length:${len},0:'c',2:'a',5:'z'};${P}.sort.call(o,(a,b)=>a<b?1:-1);`, `return [Object.keys(o).join(), o[0], o[1], o[2]]`);
}
for (const len of ["2**32", "2**32+1", "2**53", MAX, "Infinity", "2**32-1"]) {
  E(`${P}.toSorted.call({length:${len}})`);
  E(`${P}.toReversed.call({length:${len}})`);
  E(`${P}.with.call({length:${len}},0,1)`);
  E(`${P}.toSpliced.call({length:${len}},0,0)`);
}

// ---- 6. concat, slice, splice, map, filter, flat com isConcatSpreadable.
const spreadables = [
  "[1,2]", "{length:2,0:'a',1:'b'}", "{length:2,0:'a',1:'b',[Symbol.isConcatSpreadable]:true}", "{length:2,0:'a',[Symbol.isConcatSpreadable]:1}",
  "(a=>{a[Symbol.isConcatSpreadable]=false;return a})([7,8])", "(a=>{a[Symbol.isConcatSpreadable]=undefined;return a})([7,8])",
  "(a=>{a[Symbol.isConcatSpreadable]=null;return a})([7,8])", "(a=>{a[Symbol.isConcatSpreadable]=0;return a})([7,8])",
  "(a=>{a[Symbol.isConcatSpreadable]='';return a})([7,8])", "(a=>{a[Symbol.isConcatSpreadable]='x';return a})([7,8])",
  "Object.assign(new String('ab'),{[Symbol.isConcatSpreadable]:true})", "Object.assign(function(a,b){},{0:'f',length:1,[Symbol.isConcatSpreadable]:true})",
  "new Proxy([1,2],{})", "new Proxy({length:1,0:'p',[Symbol.isConcatSpreadable]:true},{})", "{get [Symbol.isConcatSpreadable](){throw new EvalError('s')}}",
  "(()=>{var p=Proxy.revocable([],{});p.revoke();return p.proxy})()", "{length:-3,[Symbol.isConcatSpreadable]:true}", "{length:'2',0:1,[Symbol.isConcatSpreadable]:true}",
  "{length:{valueOf(){throw new EvalError('l')}},[Symbol.isConcatSpreadable]:true}", "[,,1]", "[[1],[2]]", "[]", "'str'", "7", "null", "undefined",
  "Symbol.iterator", "1n", "new Number(3)", "{[Symbol.isConcatSpreadable]:true}", "{[Symbol.isConcatSpreadable]:true,length:3,1:'m'}",
];
for (const t of spreadables) for (const x of spreadables.slice(0, 22)) {
  E(`${P}.concat.call(${t},${x})`);
}
E(`${P}.concat.call([1],{length:${MAX},[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call([1,2],{length:${MAX},[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call({length:${MAX},[Symbol.isConcatSpreadable]:true},[1])`);
E(`${P}.concat.call({length:2**53,[Symbol.isConcatSpreadable]:true},[1])`);
E(`${P}.concat.call([1],{length:2**53,[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call([1],{length:Infinity,[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call([],{length:2**32,[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call([],{length:2**32+1,[Symbol.isConcatSpreadable]:true})`);
E(`${P}.concat.call([],{length:2**32-1,[Symbol.isConcatSpreadable]:true})`);
F(`var a=[];a.length=4294967295;var r;try{r=a.concat([1])}catch(e){r=e.name+':'+e.message}`, `return r`);
F(`var a=[];a.length=4294967295;var r;try{r=a.concat(1)}catch(e){r=e.name+':'+e.message}`, `return r`);
F(`var a=[];a.length=4294967295;var r;try{r=a.concat()}catch(e){r=e.name+':'+e.message}`, `return typeof r==='string'?r:r.length`);

// species em cada método, com construtores de todo tipo.
const speciesConfigs = [
  "undefined", "null", "1", "'s'", "{}", "{[Symbol.species]:undefined}", "{[Symbol.species]:null}", "{[Symbol.species]:1}", "{[Symbol.species]:{}}",
  "{[Symbol.species]:()=>[]}", "{[Symbol.species]:function(n){return {len:n}}}", "{[Symbol.species]:function(n){return Object.freeze({})}}",
  "{[Symbol.species]:function(){return [9,9,9,9,9]}}", "{[Symbol.species]:function(){return 7}}", "{[Symbol.species]:function(){return null}}",
  "{[Symbol.species]:class extends Array{}}", "{[Symbol.species]:Object}", "{[Symbol.species]:Array}", "{get [Symbol.species](){throw new EvalError('sp')}}",
  "{[Symbol.species]:function(n){var a=[];Object.defineProperty(a,'length',{writable:false});return a}}",
  "{[Symbol.species]:function(n){return new Proxy([],{defineProperty(){return false}})}}",
  "Array", "Object", "function(){}", "class extends Array{}", "class A extends Array{static get [Symbol.species](){return Array}}",
  "class A extends Array{static get [Symbol.species](){return undefined}}", "class A extends Array{static get [Symbol.species](){return Object}}",
  "(()=>{var f=function(){};f[Symbol.species]=undefined;return f})()", "(()=>1)", "{[Symbol.species]:()=>1}",
];
const speciesMethods = [
  "concat(4)", "slice(0,2)", "splice(0,1)", "map(x=>x*2)", "filter(x=>x>1)", "flat()", "flatMap(x=>[x])", "splice(1,1,'n')", "slice(1)", "filter(()=>false)",
];
for (const cfg of speciesConfigs) for (const m of speciesMethods) {
  F(`var a=[1,2,3];a.constructor=${cfg};var r;try{r=a.${m}}catch(e){r=e.name+':'+e.message}`, `return [r,Array.isArray(r),r&&r.constructor===Array]`);
}
F(`class A extends Array{};var a=A.from([1,2,3]);return [a.map(x=>x) instanceof A,a.slice() instanceof A,a.filter(x=>1) instanceof A,a.concat() instanceof A,a.splice(0) instanceof A,a.flat() instanceof A,a.flatMap(x=>x) instanceof A,a.toSorted() instanceof A,a.toReversed() instanceof A,a.toSpliced() instanceof A,a.with(0,1) instanceof A]`);
F(`class A extends Array{};var a=new A(1,2,3);return [a.length,a.map(x=>x*2),a.slice(1).length,A.of(1,2,3) instanceof A,A.from('ab') instanceof A,A.from({length:1}) instanceof A]`);
F(`class A extends Array{constructor(n){super();this.made=n}};var a=A.from([1,2]);var m=a.map(x=>x);return [a.made,m.made,m.length,a.slice(0,1).made,a.filter(()=>1).made,a.splice(0,1).made]`);
F(`class A extends Array{constructor(...args){super(...args);A.count=(A.count||0)+1}};var a=new A(3);var c0=A.count;a.fill(1).map(x=>x);return [c0,A.count]`);
F(`var calls=[];var a=[1,2,3];a.constructor={[Symbol.species]:function(n){calls.push(n);return []}};a.slice(1);a.splice(0,1);a.map(x=>x);a.filter(x=>x);a.concat(1);a.flat();a.flatMap(x=>x);return calls.join()`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.map(x=>x*2);return [Object.keys(r).join(),r.length]`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.slice(1);return [Object.keys(r).join(),r.length]`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.splice(1,1);return [Object.keys(r).join(),r.length]`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.filter(x=>x>1);return [Object.keys(r).join(),r.length]`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.concat([4]);return [Object.keys(r).join(),r.length]`);
F(`var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return {}}};var r=a.flat();return [Object.keys(r).join(),r.length]`);

// ---- 7. indexOf, lastIndexOf, includes com -0, NaN e fromIndex extremo.
const searchArrays = ["[1,NaN,-0,0,undefined,,null,'1']", "[,,]", "[NaN]", "[0]", "[-0]", "[1,2,1,2,1]"];
const searchValues = ["NaN", "0", "-0", "1", "'1'", "undefined", "null", "''", "2", "[]", "1n", "Infinity"];
const fromIndexes = [
  "undefined", "0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "2**53", "-(2**53)", "2**32", "'1'", "'x'", "{valueOf(){return 2}}", "1.9", "-1.9",
  "100", "-100", "null", "true", "2", "-2", "0.5", "-0.5", "4294967295", "-4294967296",
];
for (const method of ["indexOf", "lastIndexOf", "includes"]) {
  for (const value of searchValues) for (const from of fromIndexes) {
    E(`[1,NaN,-0,0,undefined,,null,'1'].${method}(${value},${from})`);
  }
  for (const arr of searchArrays) for (const value of ["NaN", "0", "-0", "undefined", "1", "2"]) {
    E(`${arr}.${method}(${value})`);
    E(`${arr}.${method}(${value},-1)`);
    E(`${arr}.${method}(${value},1)`);
  }
  E(`[1].${method}(1,1n)`);
  E(`[1].${method}(1,Symbol())`);
  E(`[1].${method}(1,{valueOf(){throw new EvalError('f')}})`);
  E(`[].${method}(1,{valueOf(){throw new EvalError('f')}})`);
  E(`${P}.${method}.call(null,1)`);
  E(`${P}.${method}.call(undefined,1)`);
  E(`${P}.${method}.call('abc','b')`);
  E(`${P}.${method}.call({length:3,1:'b'},'b')`);
  E(`${P}.${method}.call({length:'3',2:'c'},'c',-1)`);
  // length enorme: só os que encontram ou terminam depressa.
  for (const [len, from] of [[MAX, `${MAX}-3`], [MAX, `-3`], ["2**53", "2**53-3"], ["2**32+5", "2**32+2"], ["2**32", "-2"], ["Infinity", "2**53-3"]]) {
    E(`${P}.${method}.call({length:${len},[2**53-2]:'x',[2**32+3]:'x',[2**32-1]:'x',[2**32-2]:'x'},'x',${from})`);
    E(`${P}.${method}.call({length:${len},[2**53-2]:'x',[2**32+3]:'x'},'y',${from})`);
  }
}
F(`var log=[];var o=new Proxy([1,2,3],{get(t,k){log.push(String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t}});o.indexOf(2);o.includes(2);o.lastIndexOf(2);`, `return log.join()`);
F(`var log=[];var o=new Proxy([1,,3],{get(t,k){log.push(String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t}});o.indexOf(3);o.includes(undefined);`, `return log.join()`);
F(`${P}[1]='p';try{var r=[0,,2].indexOf('p')}finally{delete ${P}[1]}`, `return r`);
F(`${P}[1]='p';try{var r=[0,,2].includes('p')}finally{delete ${P}[1]}`, `return r`);
F(`${P}[1]=undefined;try{var r=[0,,2].includes(undefined)}finally{delete ${P}[1]}`, `return r`);
F(`${P}[1]='p';try{var r=[0,,2].lastIndexOf('p')}finally{delete ${P}[1]}`, `return r`);
F(`var a=[1,2,3];var r=a.indexOf(3,{valueOf(){a.length=1;return 0}});return r`);
F(`var a=[1,2,3];var r=a.includes(undefined,{valueOf(){a.length=1;return 0}});return r`);
F(`var a=[1,2,3];var r=a.lastIndexOf(3,{valueOf(){a.length=1;return 2}});return r`);
F(`var a=[1,2,3];var r=a.lastIndexOf(1,{valueOf(){a.length=1;return 2}});return r`);

// ---- 8. join, toString e toLocaleString recursivos.
const cyclic = [
  "(a=>(a.push(a),a))([1])", "(a=>(a.push(a,a),a))([1])", "(a=>(a[0]=a,a))([])", "(a=>(a[2]=a,a))([1,2])", "(a=>(a.push([a]),a))([1])",
  "(a=>(a.push([[a]]),a))(['x'])", "(()=>{var a=[1],b=[2];a.push(b);b.push(a);return a})()", "(()=>{var a=[1],b=[2],c=[3];a.push(b);b.push(c);c.push(a);return a})()",
  "(()=>{var a=[];a.push({toString(){return a.join()}});return a})()", "(()=>{var a=[];a.push({toString(){return String(a)}},1);return a})()",
  "(()=>{var a=[];a.push({toString(){return a.toLocaleString()}});return a})()", "(a=>(a.push({toLocaleString(){return a.toString()}}),a))([1])",
  "(a=>(a.push({toLocaleString(){return a.toLocaleString()}}),a))([1])", "(()=>{var a=[1];a.join=function(){return 'J'};a.push(a);return a})()",
  "(()=>{var a=[1,2];a.toString=function(){return 'TS'};return [a,a]})()", "(()=>{var a=[1];var o={};o.a=a;a.push(o);o.toString=function(){return a.join('-')};return a})()",
  "(()=>{var a=[1];a.push({toString(){return a.length+''}});return a})()",
];
const joinMethods = [
  "join()", "join(',')", "join('-')", "join('')", "join(undefined)", "join(null)", "join(0)", "join({toString(){return '|'}})", "toString()", "toLocaleString()",
  "join(1n)", "join(true)", "join([])", "join(['a'])",
];
for (const c of cyclic) for (const m of joinMethods) {
  F(`var a=${c};var r;try{r=a.${m}}catch(e){r=e.name}`, `return [r]`);
}
for (const c of cyclic.slice(0, 8)) {
  E(`String(${c})`);
  E(`(${c})+''`);
  E("`${" + c + "}`");
  E(`[${c}].join()`);
  E(`${P}.join.call(${c},'')`);
}
const joinSeparators = ["Symbol()", "{toString(){throw new EvalError('sep')}}", "{valueOf(){return 'v'}}", "{toString:null,valueOf(){return 'v'}}", "{[Symbol.toPrimitive](){return '@'}}", "new String('s')", "NaN", "-0", "1e21", "[1,2]"];
for (const sep of joinSeparators) {
  E(`[1,2,3].join(${sep})`);
  E(`[1].join(${sep})`);
  E(`[].join(${sep})`);
  E(`[,].join(${sep})`);
}
const joinElements = ["null", "undefined", "Symbol()", "1n", "-0", "{}", "[]", "[[]]", "[null]", "[undefined,1]", "function(){}", "{toString(){return 'ts'}}", "{toString:null,valueOf(){return 'vo'}}", "new Date(NaN)", "/re/g", "Object(Symbol())", "NaN", "'\\u0000'", "1e21", "0.1"];
for (const el of joinElements) {
  E(`[${el}].join()`);
  E(`[1,${el},2].join('+')`);
  E(`[,${el}].toString()`);
  E(`[${el}].toLocaleString()`);
}
E(`${P}.toString.call({join:()=>'jj'})`);
E(`${P}.toString.call({join:1})`);
E(`${P}.toString.call({})`);
E(`${P}.toString.call(null)`);
E(`${P}.toString.call(1)`);
E(`${P}.toString.call(function(){})`);
E(`${P}.toString.call(new Date(0)).slice(0,8)`);
E(`${P}.join.call({length:3,0:'a',2:'c'},'/')`);
E(`${P}.join.call({length:'2',0:'a',1:'b'})`);
E(`${P}.join.call({length:-1,0:'a'})`);
E(`${P}.join.call({length:0})`);
E(`${P}.join.call('abc','-')`);
E(`${P}.join.call(Object.assign(()=>{},{length:2,0:1,1:2}))`);
E(`${P}.join.call({length:2**32+1,0:'x'},'').length`);
E(`${P}.join.call({length:3,get 0(){return 'g'}},'.')`);
E(`${P}.toLocaleString.call({length:2,0:{toLocaleString(){return 'L0'}},1:{toLocaleString(){return 'L1'}}})`);
E(`${P}.toLocaleString.call({length:1,0:{toLocaleString:1}})`);
E(`[{toLocaleString(){return this===undefined?'u':typeof this}}].toLocaleString()`);
E(`[1,2].toLocaleString.call([{toLocaleString(){return arguments.length}}])`);
F(`var log=[];var a=[1,{toString(){log.push('ts');return 'x'}},3];a.join({toString(){log.push('sep');return '-'}});`, `return log.join()`);
F(`var log=[];var a=[];a.length=0;a.join({toString(){log.push('sep');return '-'}});`, `return log.join()`);
F(`var a=[1,2,3];var r=a.join({toString(){a.length=1;return '-'}});return r`);
F(`var a=[1,2,3];var r=a.join({toString(){a.push(9);return '-'}});return r`);
F(`var a=[1,{toString(){a.length=1;return 'm'}},3];return a.join()`);
F(`var a=[1,{toString(){a.push(7,8);return 'm'}},3];return a.join()`);
F(`var a=new Array(3);a[1]='x';return a.join('..')`);
F(`var a=new Array(1000);return a.join('ab').length`);
F(`var a=new Array(2**20);return a.join('').length`);
F(`return new Array(2**16+1).join('xyz').length`);
F(`var a=[1,2];a[Symbol.toPrimitive]=null;return a+''`);
F(`var a=[1,2];a.join=null;return String(a)`);
F(`var a=[1,2];a.join=undefined;return String(a)`);
F(`var a=[1,2];a.join=function(){return this===a};return String(a)`);
F(`var a=[1,2];a.join=function(){return {}};return String(a)`);
F(`var a=[1,2];a.join=function(){return Symbol()};return String(a)`);

// ---- 9. flat e flatMap com depth e estruturas extremas.
const depths = ["undefined", "0", "1", "2", "-1", "Infinity", "-Infinity", "NaN", "'2'", "null", "true", "1.9", "{valueOf(){return 2}}", "2**53", "Symbol()", "1n", "-0", "0.9", "'x'", "[]", "[2]", "4294967296", "-4294967297"];
const flatInputs = [
  "[1,[2,[3,[4,[5]]]]]", "[1,,[2,,[3,,]]]", "[[],[[]],[[[]]]]", "[[1],{length:1,0:2},'ab',[[ 'c' ]]]", "[{length:1,0:2,[Symbol.isConcatSpreadable]:true}]",
  "[new Proxy([1,[2]],{})]", "[[1,[2]],new Proxy([[3]],{})]", "[,[,1],,]", "[[[[[[[[[[1]]]]]]]]]]", "[1,2,3]", "[[1,2],[3,[4,5]],6]",
  "(class A extends Array{}).from([[1],[[2]]])", "[(class A extends Array{}).of(1,2)]", "[[undefined],[null],[0],['']]", "[[[],1],[[],[]]]", "[]",
];
for (const d of depths) for (const input of flatInputs) E(`${input}.flat(${d})`);
// flatMap: callback devolvendo arrays, array-likes, buracos e this.
const flatMapResults = ["[x]", "[x,[x]]", "x", "{length:1,0:x}", "[,x]", "[]", "[[x]]", "undefined", "new Proxy([x],{})", "null", "[null,undefined]", "'ab'", "(()=>{var r=[x];r.length=3;return r})()"];
for (const r of flatMapResults) for (const input of ["[1,2,3]", "[1,,3]", "[[1],[2]]", "[]", "['a']"]) {
  E(`${input}.flatMap(x=>${r})`);
}
for (const bad of ["undefined", "null", "1", "{}", "'f'", "Symbol()", "[]", "class{}"]) { E(`[1].flatMap(${bad})`); E(`[].flatMap(${bad})`); E(`[1].flat.call(null)`); }
F(`var r=[1,2,3].flatMap(function(x){return [this.k*x]},{k:2});return r`);
F(`var r=[1,2,3].flatMap(function(x){return [this===undefined]});return r`);
F(`'use strict';var r=[1].flatMap(function(x){return [this]},5);return r`);
F(`var r=[1].flatMap(function(x){return [typeof this]},5);return r`);
F(`var args=[];[7,8].flatMap(function(x,i,a){args.push(arguments.length,i,Array.isArray(a));return []});return args`);
F(`var a=[1,2,3];var r=a.flatMap(function(x,i){if(i===0)a.push(9);return [x]});return [r,a.length]`);
F(`var a=[1,2,3];var r=a.flatMap(function(x,i){if(i===0)a.length=1;return [x]});return r`);
F(`var a=[1,[2]];a.push(a);var r;try{r=a.flat(Infinity)}catch(e){r=e.name}return r`);
F(`var a=[1];a[0]=a;var r;try{r=a.flat(Infinity)}catch(e){r=e.name}return r`);
F(`var a=[1];a[0]=a;return a.flat(1).length`);
F(`var a=[1];a[0]=a;return a.flat(5)===undefined`);
F(`var a=[1];a[0]=a;return a.flat(0)[0]===a`);
for (const t of ["null", "undefined", "1", "'ab'", "{length:2,0:[1],1:[2]}", "{length:2,0:[1]}", "{length:-1}", "{length:'2',0:[1],1:[[2]]}", "{}", "true", "function(){}"]) {
  E(`${P}.flat.call(${t})`);
  E(`${P}.flat.call(${t},Infinity)`);
  E(`${P}.flatMap.call(${t},x=>[x])`);
}
E(`${P}.flat.call({length:${MAX}})`);
E(`${P}.flat.call({length:2**32})`);
E(`${P}.flatMap.call({length:2**32},x=>x)`);
F(`var log=[];var o=new Proxy([[1],[2]],{get(t,k){log.push(String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t}});o.flat();`, `return log.join()`);
F(`var log=[];var o=new Proxy([[1],[2]],{get(t,k){log.push(String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t}});Array.prototype.flatMap.call(o,x=>x);`, `return log.join()`);

// ---- 10. Array.from com iterável, array-like, mapFn e this.
const fromSources = [
  "[1,2,3]", "'ab'", "'\\ud83d\\ude00x'", "'\\ud83d'", "new Set([1,1,2])", "new Map([[1,2]])", "{length:2,0:'a',1:'b'}", "{length:'2',0:'a',1:'b'}", "{length:-1,0:1}", "{length:1.9,0:'z'}",
  "{length:NaN,0:1}", "{length:{valueOf(){return 2}},0:1,1:2}", "{}", "{0:1}", "1", "true", "5", "NaN", "Symbol()", "null", "undefined", "(function*(){yield 1;yield 2})()",
  "{[Symbol.iterator]:null,length:1,0:'a'}", "{[Symbol.iterator]:undefined,length:1,0:'a'}", "{[Symbol.iterator]:1}", "{[Symbol.iterator]:{}}",
  "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return 1}}}}",
  "{[Symbol.iterator](){return {next(){throw new EvalError('n')}}}}", "{[Symbol.iterator](){throw new EvalError('i')}}",
  "{[Symbol.iterator](){var n=0;return {next(){return n++<3?{value:n,done:false}:{done:true}}}}}",
  "{[Symbol.iterator](){var n=0;return {next(){return {get value(){return n++},get done(){return n>2}}}}}}",
  "{length:3,0:'a',[Symbol.iterator]:function*(){yield 'it'}}", "new Proxy([1,2],{})", "[,1,,2]", "new Uint8Array([1,2])", "function(a,b){}", "new String('ab')",
  "(()=>{var a=[1,2];a[Symbol.iterator]=function*(){yield 'x'};return a})()", "[1,2,3].values()", "[1,2,3].entries()", "new Map([[1,2]]).keys()",
  "{length:3}", "{length:2,get 0(){return 'g0'},get 1(){throw new EvalError('g1')}}",
];
const fromCallbacks = ["", ",undefined", ",x=>x", ",(x,i)=>i", ",(x,i)=>[x,i]", ",()=>{throw new RangeError('m')}", ",null", ",1", ",{}", ",x=>x+1", ",function(){return this.k},{k:'T'}", ",function(){return typeof this}", ",function(){'use strict';return typeof this}", ",(x,i)=>arguments.length", ",async x=>x", ",class{}"];
for (const src of fromSources) for (const cb of fromCallbacks) E(`Array.from(${src}${cb})`);
// o iterador é fechado quando mapFn lança, e não quando chega ao fim.
F(`var log=[];var it={[Symbol.iterator](){var n=0;return {next(){log.push('next');return {value:n++,done:n>3}},return(){log.push('return');return {}}}}};try{Array.from(it,x=>{if(x===1)throw new EvalError('m');return x})}catch(e){log.push(e.name)}`, `return log.join()`);
F(`var log=[];var it={[Symbol.iterator](){var n=0;return {next(){log.push('next');return {value:n++,done:n>3}},return(){log.push('return');return {}}}}};Array.from(it,x=>x);`, `return log.join()`);
F(`var log=[];var it={[Symbol.iterator](){var n=0;return {next(){log.push('next');return {value:n++,done:n>3}},return(){log.push('return');throw new URIError('r')}}}};try{Array.from(it,x=>{throw new EvalError('m')})}catch(e){log.push(e.name)}`, `return log.join()`);
F(`var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {value:1,done:false}},return(){log.push('return');return {}}}}};var C=function(){return Object.freeze({})};try{Array.from.call(C,it)}catch(e){log.push(e.name)}`, `return log.join()`);
F(`var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');return {value:1,done:false}},return(){log.push('return');return {}}}}};var C=function(){var r={};Object.defineProperty(r,0,{value:1,writable:false,configurable:false});return r};try{Array.from.call(C,it)}catch(e){log.push(e.name)}`, `return log.join()`);
// this de Array.from.
const fromThis = [
  "Array", "Object", "function(){}", "function(n){this.arg=n}", "function(n){return {made:arguments.length,n:n}}", "()=>1", "class extends Array{}", "class{}", "null", "undefined", "1", "'s'", "{}",
  "function(){return Object.freeze([])}", "function(){return 7}", "function(){return null}", "function(){var a=[];Object.defineProperty(a,'length',{writable:false});return a}",
  "new Proxy(Array,{})", "Array.bind(null)", "Math.max", "Symbol", "Promise", "(()=>{var p=Proxy.revocable(Array,{});p.revoke();return p.proxy})()",
  "function(){return new Proxy([],{set(){return false}})}", "function(){return {set length(v){throw new EvalError('sl')}}}",
];
for (const t of fromThis) for (const src of ["[1,2]", "{length:2,0:'a',1:'b'}", "{length:0}", "'ab'", "new Set([5])", "{}"]) {
  F(`var r;try{r=Array.from.call(${t},${src})}catch(e){r=e.name+':'+e.message}`, `return [r,r&&r.constructor===Array,Object.keys(Object(r)).join(),r&&r.length]`);
}
for (const t of ["function(n){this.made=arguments.length+':'+n}", "function(){return {}}"]) for (const src of ["[1,2]", "{length:2,0:'a',1:'b'}", "{length:0}"]) {
  F(`var r=Array.from.call(${t},${src});`, `return [Object.keys(r).join(),r.length,r.made]`);
}
E(`Array.from({length:2**32})`);
E(`Array.from({length:2**32+1})`);
E(`Array.from({length:Infinity})`);
E(`Array.from({length:${MAX}})`);
E(`Array.from({length:2**53})`);
E(`Array.from.call(Object,{length:2**32,0:'z'}).length`);
E(`Array.from.call(Object,{length:2**53+5,0:'z'},x=>x).length`);
F(`var C=function(n){this.n=n};var r;try{r=Array.from.call(C,{length:${MAX}-${MAX}+3,0:'a',1:'b',2:'c'})}catch(e){r=e.name}`, `return [r.n,r.length,Object.keys(r).join()]`);
E(`Array.from.length`);
E(`Array.from.name`);
E(`Array.of.length`);
E(`Array.from(5,x=>x)`);
E(`Array.from({length:3},(x,i)=>i*i)`);
E(`Array.from({length:3},function(x,i){return [this,i]},'s')`);
E(`Array.from(new Array(3))`);
E(`Array.from(new Array(3),(x,i)=>x===undefined?i:-1)`);
E(`Array.from([1,2,3],function(x,i,a){return arguments.length})`);
E(`Array.from([,1]).hasOwnProperty(0)`);
E(`Array.from({length:2,0:1}).hasOwnProperty(1)`);
E(`Array.from(Array(3)).hasOwnProperty(2)`);
F(`var log=[];var o=new Proxy({length:2,0:'a',1:'b'},{get(t,k){log.push(String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t}});Array.from(o);`, `return log.join()`);

// ---- 11. Array.of e subclasses.
const ofThis = [
  "Array", "Object", "function(){}", "function(n){this.arg=n}", "function(n){return {made:arguments.length,n:n}}", "()=>1", "class extends Array{}", "class{}", "null", "undefined", "1", "'s'", "{}",
  "function(){return Object.freeze([])}", "function(){return 7}", "function(){return null}", "function(){var a=[];Object.defineProperty(a,'length',{writable:false});return a}",
  "new Proxy(Array,{})", "Array.bind(null)", "Math.max", "Symbol", "Promise", "function(){return Object.freeze({})}",
  "function(){return {set length(v){throw new EvalError('sl')}}}", "function(){return new Proxy([],{set(){return false}})}",
  "class extends Array{constructor(){super();this.tag='t'}}", "class extends Array{constructor(n){super(n);this.n=n}}", "class extends Array{constructor(...a){super(...a);this.argc=a.length}}",
  "function(){return {length:5,0:'old',3:'keep'}}", "function(){return [9,9,9,9]}",
];
for (const t of ofThis) for (const args of ["", "1", "1, 2, 3", "void 0", "void 0, void 0", "[1], [2]", "7, void 0, 8"]) {
  F(`var r;try{r=Array.of.call(${t},${args})}catch(e){r=e.name+':'+e.message}`, `return [r,r&&r.constructor===Array,r&&r.length,Object.keys(Object(r)).join()]`);
}
for (const args of ["", "1", "3", "1,2", "'a'", "undefined", "-1"]) {
  E(`Array.of(${args})`);
  E(`Array.of.apply(null,[${args}])`);
  E(`new (class A extends Array{})(${args}).length`);
  E(`(class A extends Array{}).of(${args}).length`);
  E(`(class A extends Array{}).of(${args}) instanceof Array`);
  E(`Reflect.construct(Array,[${args}],class{}).constructor.name`);
  E(`Reflect.construct(Array,[${args}],Object).constructor===Object`);
  E(`Reflect.construct(Array,[${args}],function(){}).length`);
  E(`Reflect.construct(Array,[${args}],Object.assign(function(){},{prototype:null})) instanceof Array`);
  E(`Reflect.getPrototypeOf(Reflect.construct(Array,[${args}],Object.assign(function(){},{prototype:null})))===Object.getPrototypeOf(Array.of())`);
  E(`Array(${args}).length`);
}
E(`Array(2**32)`);
E(`new Array(2**32-1).length`);
E(`new Array(-1)`);
E(`new Array(1.5)`);
E(`new Array('3').length`);
E(`new Array(NaN)`);
E(`new Array(1n)`);
E(`new Array(Infinity)`);
E(`new Array(-0).length`);
E(`Array(4294967295).length`);
E(`Array(4294967296)`);
E(`Array.of.call(function(n){return new Proxy({},{set(t,k,v){t[k]=v;return k!=='length'}})},1)`);

// ---- 12. setter de length: valores x estados de array.
const lengthValues = [
  "0", "1", "2", "3", "5", "-1", "1.5", "'3'", "'x'", "NaN", "Infinity", "-Infinity", "2**32", "2**32-1", "2**32-2", "2**31", "{valueOf(){return 1}}",
  "{valueOf(){return 4294967296}}", "{valueOf(){throw new EvalError('v')}}", "null", "undefined", "true", "false", "[]", "[5]", "''", "1n", "Symbol()", "-0", "0.5", "'1e1'", "' 2 '", "'0x2'", "'2.0'", "'-0'",
];
const lengthStates = [
  "var a=[1,2,3];", "var a=[,,];", "var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});", "var a=[1,2,3];Object.defineProperty(a,2,{configurable:false});",
  "var a=Object.freeze([1,2,3]);", "var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});", "var a=Object.seal([1,2,3]);",
  "var a=[1,2,3];Object.preventExtensions(a);", "var a=[];a[10]=1;", "var a=[];a[100000]=1;a[5]=2;", "var a=[1,2,3];Object.defineProperty(a,0,{get(){return 1},configurable:false});",
  "var a=[];",
];
for (const state of lengthStates) for (const v of lengthValues) {
  F(`${state}var r;try{a.length=${v};r='ok'}catch(e){r=e.name+':'+e.message}`, `return [r,a.length,Object.keys(a).slice(0,6).join()]`);
}
for (const state of lengthStates.slice(0, 8)) for (const v of ["0", "1", "5", "-1", "'2'", "2**32"]) {
  F(state, `return [Reflect.set(a,'length',${v}),a.length]`);
  F(state, `var r;try{r=Reflect.defineProperty(a,'length',{value:${v}})}catch(e){r=e.name}return [r,a.length]`);
  F(`${state}var r;try{r=Object.defineProperty(a,'length',{value:${v}}).length}catch(e){r=e.name+':'+e.message}`, `return [r,a.length]`);
}
F(`var a=[1,2,3];var n=0;a.length={valueOf(){n++;return 2}};`, `return [n,a.length]`);
F(`var a=[1,2,3];var n=0;try{a.length={valueOf(){n++;return 2.5}}}catch(e){}`, `return [n,a.length]`);
F(`var a=[1,2,3];var log=[];try{a.length={valueOf(){log.push('v');return 1},toString(){log.push('t');return '1'}}}catch(e){}`, `return [log,a.length]`);
F(`var a=[1,2,3];var log=[];try{Object.defineProperty(a,'length',{value:{valueOf(){log.push('v');return 1}}})}catch(e){}`, `return [log,a.length]`);
F(`var a=[1,2,3];var r;Object.defineProperty(a,'length',{value:1,writable:false});try{a.length=1;r='same'}catch(e){r=e.name}try{a.push(1)}catch(e){r+=e.name}`, `return [r,a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r=[];try{a.push(4)}catch(e){r.push(e.name)}try{a.pop()}catch(e){r.push(e.name)}try{a.shift()}catch(e){r.push(e.name)}try{a.unshift(0)}catch(e){r.push(e.name)}try{a.splice(0,1)}catch(e){r.push(e.name)}`, `return [r,a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r=[];try{a.reverse();r.push('ok')}catch(e){r.push(e.name)}try{a.sort();r.push('ok')}catch(e){r.push(e.name)}try{a.fill(0);r.push('ok')}catch(e){r.push(e.name)}try{a.copyWithin(0,1);r.push('ok')}catch(e){r.push(e.name)}`, `return [r,a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r;try{a[3]=1;r='ok'}catch(e){r=e.name+':'+e.message}`, `return [r,a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r;try{a[1]=9;r='ok'}catch(e){r=e.name}`, `return [r,a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return [Object.getOwnPropertyDescriptor(a,'length'),delete a[2],a]`);
F(`var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r;try{Object.defineProperty(a,'length',{writable:true});r='ok'}catch(e){r=e.name}return [r,Object.getOwnPropertyDescriptor(a,'length').writable]`);
F(`var a=[1,2,3];var r;try{Object.defineProperty(a,'length',{enumerable:true});r='ok'}catch(e){r=e.name}return r`);
F(`var a=[1,2,3];var r;try{Object.defineProperty(a,'length',{configurable:true});r='ok'}catch(e){r=e.name}return r`);
F(`var a=[1,2,3];var r;try{Object.defineProperty(a,'length',{get(){return 1}});r='ok'}catch(e){r=e.name}return r`);
F(`var a=[1,2,3];return [delete a.length,Object.getOwnPropertyDescriptor(a,'length')]`);
F(`var a=[1,2,3];return Object.getOwnPropertyNames(a).join()`);
F(`var a=[1,2,3];a.x=1;a[Symbol.iterator]=0;return Reflect.ownKeys(a).map(String).join()`);
F(`var a=[1,2,3];a.length=1;a.length=3;return [a,1 in a]`);
F(`var a=[1,2,3];a.length=0;a[2]=1;return [a,Object.keys(a).join()]`);
F(`var a=[];a['4294967295']=1;return [a.length,Object.keys(a).join()]`);
F(`var a=[];a['4294967294']=1;return [a.length,Object.keys(a).join()]`);
F(`var a=[];a['01']=1;a['1.0']=1;a['-1']=1;a['1e1']=1;return [a.length,Object.keys(a).join()]`);
F(`var a=[];a[1.5]=1;a[-0]=2;a[2**32]=3;a[NaN]=4;return [a.length,Object.keys(a).join()]`);
F(`var a=[1,2,3];Object.defineProperty(a,1,{value:2,configurable:false});a.length=0;return a`);
F(`'use strict';var a=[1,2,3];Object.defineProperty(a,1,{value:2,configurable:false});a.length=0;return a`);
F(`var a=[1,2,3];Object.defineProperty(a,1,{value:2,configurable:false});return [Reflect.set(a,'length',0),a.length]`);
F(`var a=[1,2,3];Object.defineProperty(a,1,{value:2,configurable:false});return [Reflect.defineProperty(a,'length',{value:0,writable:false}),a.length,Object.getOwnPropertyDescriptor(a,'length').writable]`);
F(`var a=[1,2,3,4];Object.defineProperty(a,1,{value:2,configurable:false});try{Object.defineProperty(a,'length',{value:0,writable:false})}catch(e){}`, `return [a.length,Object.getOwnPropertyDescriptor(a,'length').writable]`);
F(`var a=[1,2,3];var log=[];Object.defineProperty(a,2,{get(){return 1},set(v){log.push('set')},configurable:true});a.length=1;`, `return [a,log]`);

// ---- 13. buracos nos métodos de iteração.
const holeMethods = [
  "forEach(cb)", "map(cb)", "filter(cb)", "reduce(cb2)", "reduce(cb2,'i')", "reduceRight(cb2)", "reduceRight(cb2,'i')", "every(cb)", "some(cb)", "find(cb)", "findIndex(cb)", "findLast(cb)",
  "findLastIndex(cb)", "flatMap(cb)", "indexOf(undefined)", "includes(undefined)", "lastIndexOf(undefined)", "join('-')", "keys()", "entries()", "values()", "at(1)", "fill(0)", "copyWithin(0,1)",
  "reverse()", "slice(0)", "splice(0)", "concat()", "toReversed()", "toSorted()", "toSpliced(0,0)", "with(1,'w')", "flat()", "sort()", "push(1)", "unshift(1)", "shift()", "pop()",
];
const holeShapes = ["[1,,3]", "[,,]", "[,1]", "[1,,]", "new Array(3)", "[,'a',,'b',,]", "(()=>{var a=[1,2,3];delete a[1];return a})()", "(()=>{var a=[1,2,3];a.length=5;return a})()", "[undefined,,undefined]", "Object.assign([],{2:'c'})"];
for (const shape of holeShapes) for (const m of holeMethods) {
  F(`var log=[];var cb=function(v,i){log.push(i+':'+String(v));return v};var cb2=function(acc,v,i){log.push(i+':'+String(v));return String(acc)+String(v)};var a=${shape};var r;try{r=a.${m};if(r&&typeof r.next==='function')r=Array.from(r)}catch(e){r=e.name}`, `return [r,log.join(),a]`);
}
// índice herdado do protótipo e mutação durante a iteração.
for (const m of ["forEach", "map", "filter", "every", "some", "find", "findIndex", "findLast", "flatMap", "indexOf", "includes"]) {
  F(`${P}[1]='P';var log=[];try{var r=[0,,2].${m}(function(v,i){log.push(i+':'+v);return false});if(Array.isArray(r))r=S(r)}finally{delete ${P}[1]}`, `return [r,log.join()]`);
  F(`var log=[];var a=[1,2,3,4];var r=a.${m}(function(v,i){log.push(i+':'+v);if(i===0)a.length=2;return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
  F(`var log=[];var a=[1,2,3];var r=a.${m}(function(v,i){log.push(i+':'+v);if(i===0)a.push(4);return false});`, `return [Array.isArray(r)?S(r):r,log.join(),a.length]`);
  F(`var log=[];var a=[1,2,3];var r=a.${m}(function(v,i){log.push(i+':'+v);if(i===0)delete a[1];return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
  F(`var log=[];var a=[1,2,3];var r=a.${m}(function(v,i){log.push(i+':'+v);if(i===0)a[1]='N';return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
  F(`var log=[];var a=[1,2,3];var r=a.${m}(function(v,i){log.push(i+':'+v);if(i===0)a.shift();return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
  F(`var log=[];var o={length:3,0:'a',2:'c'};var r=Array.prototype.${m}.call(o,function(v,i){log.push(i+':'+v);return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
  F(`var log=[];var r=Array.prototype.${m}.call('a,b',function(v,i){log.push(i+':'+v);return false});`, `return [Array.isArray(r)?S(r):r,log.join()]`);
}
for (const m of ["reduce", "reduceRight"]) {
  for (const shape of ["[]", "[,,]", "[,1]", "[1,,]", "[1]", "[1,2]", "[,,3,,]"]) for (const init of ["", ",undefined", ",0", ",null"]) {
    E(`${shape}.${m}((a,b)=>String(a)+String(b)${init})`);
  }
  E(`[1,2].${m}()`); E(`[1,2].${m}(null)`); E(`[1,2].${m}({})`); E(`[].${m}()`);
  F(`var a=[1,2,3];var r=a.${m}(function(acc,v,i,arr){return acc+':'+v+i+(arr===a)})`, `return r`);
  F(`var a=[1,2,3];var r=a.${m}(function(acc,v,i,arr){return acc+':'+v+i+(arr===a)},'s')`, `return r`);
  F(`var a=[1,2,3];var r=a.${m}(function(acc,v){return arguments.length},0)`, `return r`);
  F(`var a=[1,2,3];var r=a.${m}(function(acc,v){if(arguments[2]===0||arguments[2]===2)a.length=1;return acc+v},0)`, `return r`);
  F(`var a=[,,3];var r=a.${m}(function(acc,v,i){return acc+i},'')`, `return r`);
}
for (const m of ["map", "filter", "every", "some", "forEach", "find", "findIndex", "findLast", "findLastIndex", "flatMap", "reduce", "reduceRight"]) {
  for (const bad of ["undefined", "null", "1", "{}", "'f'", "Symbol()", "[]", "class{}", "true", "1n"]) {
    E(`[1].${m}(${bad})`);
    E(`[].${m}(${bad})`);
  }
  F(`var r=[1].${m}(function(){return this===undefined?'u':typeof this},'s');`, `return r`);
  F(`var r=[1].${m}(function(){'use strict';return typeof this},'s');`, `return r`);
  F(`var r=[1].${m}(function(){'use strict';return this},undefined);`, `return r`);
  E(`${P}.${m}.call(null,x=>x)`);
  E(`${P}.${m}.call(undefined,x=>x)`);
  E(`${P}.${m}.call({length:{valueOf(){throw new EvalError('l')}}},x=>x)`);
}

// ---- 14. arrays com 2**32-1 elementos virtuais e length no limite.
F(`var a=[];a.length=4294967295;var r;try{r=a.push(1)}catch(e){r=e.name+':'+e.message}`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;var r;try{r=a.push()}catch(e){r=e.name+':'+e.message}`, `return [r,a.length]`);
F(`var a=[];a.length=4294967295;var r;try{r=a.push(1,2)}catch(e){r=e.name+':'+e.message}`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967294;var r;try{r=a.push(1)}catch(e){r=e.name+':'+e.message}`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967294;var r;try{r=a.push(1,2)}catch(e){r=e.name+':'+e.message}`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;a[4294967294]='last';var r=a.pop();`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;var r=a.pop();`, `return [r,a.length]`);
F(`var a=[];a.length=4294967295;a.pop();var r;try{r=a.push('x','y')}catch(e){r=e.name}`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;a.pop();var r=a.push('x');`, `return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;a[4294967294]=1;return [a.length,Object.keys(a).join()]`);
F(`var a=[];a.length=4294967295;a[4294967295]=1;return [a.length,Object.keys(a).join()]`);
F(`var a=[];a[4294967294]=1;return [a.length,a.indexOf(1),a.lastIndexOf(1),a.includes(1),a.at(-1),a.findLast(x=>true)]`);
F(`var a=[];a[4294967294]=1;return [a.slice(-1),a.slice(4294967293),a.slice(4294967294).length]`);
F(`var a=[];a[4294967294]=1;var r=a.with(-1,'w');return [r.length,r[4294967294]]`);
F(`var a=[];a[4294967294]=1;var r=a.with(4294967294,'w');return [r.length,r[4294967294]]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.with(4294967295,'w')}catch(e){r=e.name+':'+e.message}return r`);
F(`var a=[];a[4294967294]=1;return [a.at(4294967294),a.at(-4294967295),a.at(-4294967296),a.at(4294967295)]`);
F(`var a=[];a[4294967294]=1;a.fill('f',4294967293);return [a.length,Object.keys(a).join(),a[4294967293]]`);
F(`var a=[];a[4294967294]=1;a.copyWithin(4294967293,4294967294);return [a.length,Object.keys(a).join()]`);
F(`var a=[];a[4294967294]=1;return [a.splice(4294967294,1),a.length,Object.keys(a).join()]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.splice(4294967294,0,'a','b')}catch(e){r=e.name+':'+e.message}return [r,a.length]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.splice(4294967294,0,'a')}catch(e){r=e.name+':'+e.message}return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.splice(4294967294,1,'a','b')}catch(e){r=e.name+':'+e.message}return [r,a.length,Object.keys(a).join()]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.splice(-1)}catch(e){r=e.name+':'+e.message}return [r,a.length]`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.concat(2)}catch(e){r=e.name+':'+e.message}return r`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.toSpliced(0,0)}catch(e){r=e.name+':'+e.message}return r`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.toReversed()}catch(e){r=e.name}return r&&r.length`);
F(`var a=[];a[4294967294]=1;var r;try{r=a.unshift()}catch(e){r=e.name+':'+e.message}return r`);
F(`var a=[];a[4294967294]=1;var r=a.join('').length;return r`);
// array-likes no limite 2**53-1.
const bigLengths = [MAX, "2**53", `${MAX}-1`, `${MAX}-2`, "2**32", "2**32-1", "2**32+1", "Infinity", "1e300"];
for (const len of bigLengths) {
  E(`${P}.push.call({length:${len}},'a')`);
  E(`${P}.push.call({length:${len}},'a','b')`);
  E(`${P}.push.call({length:${len}})`);
  F(`var o={length:${len}};var r;try{r=${P}.push.call(o,'a')}catch(e){r=e.name+':'+e.message}`, `return [r,String(o.length),Object.keys(o).join()]`);
  F(`var o={length:${len}};var r;try{r=${P}.push.call(o,'a','b')}catch(e){r=e.name+':'+e.message}`, `return [r,String(o.length),Object.keys(o).join()]`);
  F(`var o={length:${len},[2**53-2]:'z',[2**32-2]:'y'};var r;try{r=${P}.pop.call(o)}catch(e){r=e.name+':'+e.message}`, `return [r,String(o.length),Object.keys(o).join()]`);
  E(`${P}.unshift.call({length:${len}},'a')`);
  E(`${P}.unshift.call({length:${len}},'a','b')`);
  E(`${P}.unshift.call({length:${len}})`);
  E(`${P}.splice.call({length:${len}},0,0,'a','b')`);
  E(`${P}.fill.call({length:${len}},0,${len}-1)`);
  E(`${P}.fill.call({length:${len}},0,${len}-2).length`);
  F(`var o={length:${len}};${P}.fill.call(o,'f',${len}-2);`, `return [Object.keys(o).join(),String(o.length)]`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.at.call(o,-1);`, `return r`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.slice.call(o,-1);`, `return [r,r.length]`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.findLast.call(o,x=>true);`, `return r`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.findLastIndex.call(o,x=>true);`, `return String(r)`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.lastIndexOf.call(o,'z');`, `return String(r)`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.reduceRight.call(o,(a,v)=>a+v,'');`, `return r`);
  F(`var o={length:${len},[2**53-2]:'z'};var r=${P}.with.call(o,-1,'w');`, `return r`);
  F(`var o={length:${len}};${P}.copyWithin.call(o,${len}-1,${len}-2);`, `return Object.keys(o).join()`);
}
for (const [len, idx] of [[MAX, "2**53-2"], [MAX, "2**53-3"], ["2**53", "2**53-2"]]) {
  F(`var o={length:${len},[${idx}]:'v'};var r;try{r=${P}.push.call(o,'n')}catch(e){r=e.name}`, `return [r,String(o.length),o[${idx}]]`);
  F(`var o={length:${len}-1,[${idx}]:'v'};var r;try{r=${P}.push.call(o,'n')}catch(e){r=e.name}`, `return [r,String(o.length),o[${idx}]]`);
  F(`var o={length:${len}-2,[${idx}]:'v'};var r;try{r=${P}.push.call(o,'n','m')}catch(e){r=e.name}`, `return [r,String(o.length)]`);
  F(`var o={length:${len}-2};var r;try{r=${P}.push.call(o,'n')}catch(e){r=e.name}`, `return [String(r),String(o.length),Object.keys(o).join()]`);
  F(`var o={get length(){return ${len}},set length(v){this.written=v}};var r;try{r=${P}.push.call(o)}catch(e){r=e.name}`, `return [String(r),String(o.written)]`);
  F(`var o={get length(){return ${len}},set length(v){this.written=v}};var r;try{r=${P}.pop.call(o)}catch(e){r=e.name}`, `return [String(r),String(o.written)]`);
  F(`var o={length:${len},set length(v){}};var r;try{r=${P}.pop.call(o)}catch(e){r=e.name}`, `return [String(r)]`);
}
F(`var o={length:2**53-1};Object.freeze(o);var r;try{r=${P}.pop.call(o)}catch(e){r=e.name}`, `return String(r)`);
F(`var o={length:0};Object.freeze(o);var r;try{r=${P}.push.call(o,1)}catch(e){r=e.name+':'+e.message}`, `return String(r)`);
F(`var o={length:0};Object.freeze(o);var r;try{r=${P}.pop.call(o)}catch(e){r=e.name+':'+e.message}`, `return String(r)`);
F(`var o={length:0};Object.freeze(o);var r;try{r=${P}.shift.call(o)}catch(e){r=e.name+':'+e.message}`, `return String(r)`);
F(`var o={length:0};Object.freeze(o);var r;try{r=${P}.unshift.call(o)}catch(e){r=e.name+':'+e.message}`, `return String(r)`);
F(`var o={length:0};Object.preventExtensions(o);var r;try{r=${P}.push.call(o,1)}catch(e){r=e.name+':'+e.message}`, `return String(r)`);
F(`var o={length:'3'};var r=${P}.push.call(o,'a');`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:3.7};var r=${P}.push.call(o,'a');`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:-5};var r=${P}.push.call(o,'a');`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={};var r=${P}.push.call(o,'a','b');`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:NaN};var r=${P}.pop.call(o);`, `return [r,o.length]`);
F(`var o={length:2,1:'b'};var r=${P}.pop.call(o);`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:2,0:'a',1:'b'};var r=${P}.shift.call(o);`, `return [r,o.length,Object.keys(o).join(),o[0]]`);
F(`var o={length:3,0:'a',2:'c'};var r=${P}.shift.call(o);`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:3,0:'a',2:'c'};var r=${P}.unshift.call(o,'x');`, `return [r,o.length,Object.keys(o).join()]`);
F(`var o={length:3,0:'a',2:'c'};var r=${P}.reverse.call(o);`, `return [r===o,Object.keys(o).join(),o[0]]`);
F(`var o={length:3,0:'a',1:'b'};var r=${P}.reverse.call(o);`, `return [Object.keys(o).join(),o[2]]`);
F(`var o={length:4,0:'a',3:'d'};${P}.copyWithin.call(o,1,2);`, `return [Object.keys(o).join()]`);
F(`var o={length:4,0:'a',3:'d'};${P}.fill.call(o,'f',1,3);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',-3,-1);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',NaN,Infinity);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',-Infinity,2**53);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',2,1);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f','1','3');`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',{valueOf(){return 1}},{valueOf(){return 2}});`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',undefined,undefined);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',1,undefined);`, `return [Object.keys(o).join()]`);
F(`var o={length:4};${P}.fill.call(o,'f',1,null);`, `return [Object.keys(o).join()]`);

// ---- Emissão: descarta repetidos e os que já estão nos goldens de array, depois roda cada programa num bun novo.
const bodyOf = source => {
  const start = source.indexOf("function D(");
  const rest = start < 0 ? source : source.slice(start);
  const nl = rest.indexOf("\n");
  return nl < 0 ? rest : rest.slice(nl + 1);
};
const goldenDir = path.join(__dirname, "..", "tests", "golden");
// Os goldens regenerados guardam o fonte canônico: o conjunto reconhece o programa cru e o canônico, pelo corpo.
const bodyKey = source => bodyOf(source).replace(/\bR = /g, "globalThis.R = ");
const existingBodies = knownProgramSet("array_combo_bun.tsv", ["array_bun.tsv", "array_edge_bun.tsv", "array_more_bun.tsv"], bodyKey);
const seen = new Set();
const everything = [];
let dup = 0;
for (const body of programs) {
  const adjusted = body.replace(/\bR = /g, "globalThis.R = ");
  if (seen.has(adjusted)) continue;
  seen.add(adjusted);
  everything.push('"use strict";\n' + PRELUDE + "\n" + adjusted);
}
// Amostra TARGET programas por hash (sampleByHash), do conjunto inteiro, antes de descontar os goldens vizinhos.
const TARGET = Number(process.env.TARGET || 2400);
const candidates = [];
for (const original of sampleByHash(everything, TARGET)) {
  if (existingBodies.has(original)) { dup++; continue; }
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  candidates.push(prepareProgram(original));
}
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "array-combo-golden-"));
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
// Roda um programa num processo novo, `workerId` identifica o arquivo para os pools paralelos não colidirem.
const runOnce = (file, source) =>
  new Promise(resolve => {
    fs.writeFileSync(file, source);
    const child = spawn(process.execPath, ["--preload", preload, file], { cwd: dir, stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 3000);
    child.stdout.on("data", chunk => { if (out.length < 1e6) out += chunk; });
    child.on("close", () => {
      clearTimeout(timer);
      resolve(out.split("\n").find(line => line.startsWith("\u0001")));
    });
  });
const shortSource = source => JSON.stringify(source.slice(PRELUDE.length + 14)).slice(0, 200);
const results = new Array(candidates.length);
let dropped = 0;
const process1 = async (index, file) => {
  const { source, executable, meta } = candidates[index];
  const marked = await runOnce(file, executable);
  if (!marked) { dropped++; process.stderr.write("sem resultado para: " + shortSource(source) + "\n"); return; }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++; process.stderr.write("caminho da máquina no resultado: " + shortSource(source) + "\n"); return;
  }
  // Resultado que depende do acaso ou do tempo não serve de golden.
  if ((await runOnce(file, executable)) !== marked) {
    dropped++; process.stderr.write("não determinístico: " + shortSource(source) + "\n"); return;
  }
  results[index] = JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : "");
};
(async () => {
  let next = 0;
  const workers = Array.from({ length: Number(process.env.JOBS || 8) }, async (_, w) => {
    const file = path.join(dir, `array_case_${w}.js`);
    while (next < candidates.length) await process1(next++, file);
  });
  await Promise.all(workers);
  const lines = results.filter(Boolean);
  const kept = lines.length;
  process.stdout.write(emitFactoredLines("array_combo", lines));
  process.stderr.write(`candidatos ${candidates.length}, mantidos ${kept}, descartados ${dropped}, repetidos dos goldens ${dup}\n`);
  fs.rmSync(dir, { recursive: true, force: true });
})();
