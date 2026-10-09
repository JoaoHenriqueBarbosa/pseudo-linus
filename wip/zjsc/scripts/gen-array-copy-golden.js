// Gera tests/golden/array_copy_bun.tsv: métodos de cópia e agrupamento de Array, medidos no bun.
// Cobre toSorted, toReversed, toSpliced, with, findLast, findLastIndex, at, Object.groupBy, Map.groupBy, a estabilidade
// de sort/toSorted, comparadores inconsistentes, que lançam e que mutam o array, receptores com buracos, array-likes com
// length estranho (negativo, 2**53, valueOf que lança), Proxy que registra os traps, typed arrays (RangeError e TypeError
// exatos) e receptores null/undefined. Cada programa grava em `globalThis.R` o resultado, o estado final do receptor e o
// log dos traps. Um bun filho por programa, sem APIs de host, no máximo 6 em paralelo, com timeout de 8 s.
// Programas que já existem em goldens vizinhos de array (`knownPrograms`) são descartados.
// Uso: bun scripts/gen-array-copy-golden.js > tests/golden/array_copy_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'var L=[],Z=0,AP=Array.prototype,TAP=Object.getPrototypeOf(Uint8Array.prototype);function g(s){Z||L.push(s)}\n' +
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return"fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>2)return"..";' +
  'if(Array.isArray(v)){var o=[];for(var i=0;i<Math.min(v.length,12);i++)o.push(i in v?S(v[i],d+1):"<h>");return"["+o.join()+"]"+v.length+(Object.getPrototypeOf(v)===AP?"":"~sub")}' +
  'if(ArrayBuffer.isView(v))return Object.prototype.toString.call(v)+"("+v.length+")["+Array.prototype.map.call(v,function(x){return S(x)}).join()+"]";' +
  'if(v instanceof Map){var m=[];v.forEach(function(x,k){m.push(S(k,d+1)+"=>"+S(x,d+1))});return"Map{"+m.join()+"}"}' +
  'return"{"+Object.keys(v).slice(0,12).map(k=>k+":"+S(v[k],d+1)).join()+"}"+(Object.getPrototypeOf(v)===null?"^null":"")}\n' +
  'function P(o){return new Proxy(o,{get(t,k,r){g("g:"+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){g("s:"+String(k));return Reflect.set(t,k,v,r)},' +
  'has(t,k){g("h:"+String(k));return Reflect.has(t,k)},deleteProperty(t,k){g("d:"+String(k));return Reflect.deleteProperty(t,k)},' +
  'defineProperty(t,k,d){g("D:"+String(k));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){g("o:"+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})}\n' +
  'function Q(f){try{var v=f();Z=1;return S(v)}catch(e){return"throw "+e.name+": "+e.message}finally{Z=0}}\n' +
  'function K(r){Z=1;try{var o=Object(r);return Reflect.ownKeys(o).map(k=>String(k)+"="+S(o[k])).join()}catch(e){return"err"}finally{Z=0}}\n' +
  'function T(f){var x;try{x=f()}catch(e){x="throw "+e.name+": "+e.message}return x+"|"+L.join()}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const run = (recv, call) => `T(()=>{var r=${recv};var x=Q(()=>${call});return x+" st="+K(r)})`;

// ---- Receptores genéricos x métodos de cópia.
const receivers = [
  "[3,1,2]", "[1,,3]", "[,,]", "[]", "[undefined,3,,1]", "['b','a',10,9,1]", "[NaN,1,-0,0,-1]", "[true,false,null,undefined,'x']",
  "P([3,1,2])", "P([1,,3])", "P([])", 'P({length:3,0:"c",1:"a",2:"b"})', 'P({length:2,0:"a",2:"z"})',
  '{length:3,0:"c",1:"a",2:"b"}', '{get length(){g("len");return 3},0:"c",1:"a",2:"b"}',
  '{get length(){g("len");return 3},get 0(){g("g0");return"c"},get 1(){g("g1");return"a"},2:"b"}',
  '{length:2.7,0:"a",1:"b",2:"c"}', '{length:-5,0:"a"}', '{length:"2",0:"b",1:"a"}', '{length:{valueOf(){g("vo");return 2}},0:"b",1:"a"}',
  '{length:{valueOf(){throw new EvalError("vo")}},0:"a"}', '{length:{toString(){return"2"}},0:"b",1:"a"}', "{length:NaN,0:1}", "{length:null,0:1}",
  "{length:true,0:1,1:2}", "{length:[3],0:3,1:2,2:1}", "{0:'a'}", "{}", 'new String("cba")', 'Object("")',
  "(function(){return arguments})(3,1,2)", "(function(){return arguments})()", "Object.freeze([3,1,2])", "Object.seal([3,1,2])",
  "Object.create([3,1,2])", "Object.assign(Object.create({1:'p',length:2}),{0:'a'})",
  'Object.defineProperty([3,1,2],"1",{get(){g("g1");return 9},set(v){g("s1")}})', "Object.assign([3,1,2],{extra:1})",
  "Object.assign([3,1],{length:5})", "P(Object.freeze([3,1,2]))", "new (class A extends Array{})(3,1,2)", "Object.setPrototypeOf([3,1,2],null)",
  "null", "undefined", "5", '"xyz"', "true", "Symbol()", "1n", "P(new Uint8Array([3,1]))", '{get length(){throw new RangeError("len boom")}}',
  "{length:Symbol()}", "{length:1n}", "{length:2**32}", "{length:2**32+1}", "{length:2**53-1}", "{length:2**53}", "{length:Infinity}",
  "{length:-Infinity}", "{length:1e300}", "{length:4294967295,0:1}", "P({length:2**53-1})", "P({length:2**32})",
];
const methods = {
  toSorted: ["", "(a,b)=>b>a?1:-1", "(a,b)=>a-b", "undefined", "null", "1", "{}", "'f'", "()=>0", "()=>{throw new RangeError('c')}"],
  toReversed: [""],
  toSpliced: ["", "1", "1,1", "1,1,'x'", "0,0,'a','b'", "-1,1", "undefined", "0,undefined", "-5,2", "1,-1", "1.9,1.2", "'1','1'", "NaN,NaN", "0,Infinity", "Infinity", "-Infinity,1", "{valueOf(){g('v');return 1}},1"],
  with: ["0,'w'", "-1,'w'", "5,'w'", "-9,'w'", "1.5,'w'", "NaN,'w'", "'1','w'", "undefined,'w'", "0", "", "Infinity,'w'", "-0,'w'", "{valueOf(){g('v');return 1}},'w'", "{valueOf(){throw new EvalError('i')}},'w'"],
  findLast: ["v=>v==='a'", "v=>true", "v=>false", "(v,i,a)=>{g('f'+i);return false}", "undefined", "null", "1", "()=>{throw new RangeError('p')}", "function(){return this===undefined}", "function(){return typeof this}"],
  findLastIndex: ["v=>v==='a'", "v=>true", "v=>false", "(v,i,a)=>{g('f'+i);return false}", "undefined", "null", "1", "()=>{throw new RangeError('p')}"],
  at: ["0", "-1", "5", "'1'", "NaN", "", "1.9", "-1.9", "Infinity", "-Infinity", "{valueOf(){g('v');return 1}}", "2**53", "-(2**53)", "-0"],
};
for (const recv of receivers) {
  for (const [name, argList] of Object.entries(methods)) {
    for (const args of argList) add(run(recv, `AP.${name}.call(r${args ? "," + args : ""})`));
  }
}

// ---- Receptores enormes só com chamadas que não percorrem tudo.
const bigCalls = [
  "AP.at.call(r,-1)", "AP.at.call(r,0)", "AP.at.call(r,2**53-2)", "AP.with.call(r,-1,1)", "AP.with.call(r,0,1)", "AP.toReversed.call(r)",
  "AP.toSorted.call(r)", "AP.toSpliced.call(r,0,0)", "AP.toSpliced.call(r,0,0,1)", "AP.toSpliced.call(r,0,1,1)", "AP.toSpliced.call(r,-1,1)",
  "AP.toSpliced.call(r,2**53-2,1)", "AP.findLast.call(r,()=>true)", "AP.findLastIndex.call(r,()=>true)",
];
for (const recv of ["{length:2**53-1}", "{length:2**53}", "{length:Infinity}", "{length:2**32}", "{length:2**32+1}", "{length:2**32-1}", "P({length:2**32-1})", "{length:2**32-1,[2**32-2]:'z'}", "{length:2**53-1,[2**53-2]:'z'}"]) {
  for (const call of bigCalls) add(run(recv, call));
}

// ---- Typed arrays: RangeError e TypeError exatos.
const taTypes = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
const taCalls = [
  "r.toSorted()", "r.toSorted((a,b)=>b>a?1:-1)", "r.toSorted(undefined)", "r.toSorted(null)", "r.toSorted(1)", "r.toSorted({})", "r.toSorted(()=>{throw new RangeError('c')})",
  "r.toReversed()", "r.with(0,V)", "r.with(-1,V)", "r.with(3,V)", "r.with(-4,V)", "r.with(1.5,V)", "r.with(NaN,V)", "r.with(Infinity,V)", "r.with(-Infinity,V)",
  "r.with(0,'x')", "r.with(0,{valueOf(){g('v');return V}})", "r.with(9,{valueOf(){g('v');return V}})", "r.with(0)", "r.with()", "r.with(0,Symbol())", "r.with(0,1n)", "r.with(0,1)",
  "r.at(0)", "r.at(-1)", "r.at(3)", "r.at(-4)", "r.at(NaN)", "r.at(1.9)", "r.at()",
  "r.findLast(v=>v>1)", "r.findLastIndex(v=>v>1)", "r.findLast(()=>false)", "r.findLastIndex(()=>false)", "r.findLast(1)", "r.findLastIndex(null)",
  "r.findLast((v,i,a)=>{g('f'+i+typeof v);return false})", "r.toReversed().buffer===r.buffer", "r.toSorted().buffer===r.buffer", "r.with(0,V).byteLength",
  "r.toSorted.length+':'+r.with.length+':'+r.toReversed.length",
  "TAP.toSorted.call([1,2])", "TAP.toSorted.call({length:1,0:1})", "TAP.toSorted.call(null)", "TAP.toSorted.call(new DataView(new ArrayBuffer(2)))", "TAP.toReversed.call([])",
  "TAP.with.call([1],0,1)", "TAP.with.call(undefined,0,1)", "TAP.at.call([1],0)", "TAP.at.call('s',0)", "TAP.findLast.call([1],()=>1)", "TAP.findLastIndex.call({},()=>1)",
  "(r.buffer.transfer(),r.toSorted())", "(r.buffer.transfer(),r.toReversed())", "(r.buffer.transfer(),r.with(0,V))", "(r.buffer.transfer(),r.at(0))",
  "(r.buffer.transfer(),r.findLast(()=>1))", "(r.buffer.transfer(),r.findLastIndex(()=>1))", "r.with(0,{valueOf(){r.buffer.transfer();return V}})",
  "r.toSorted((a,b)=>{r.buffer.transfer();return a<b?-1:1})",
  "r.findLast((v,i)=>{if(i===2)r.buffer.transfer();g('i'+i+':'+v);return false})",
  "r.at({valueOf(){r.buffer.transfer();return 0}})",
];
for (const ty of taTypes) {
  const big = ty.startsWith("Big");
  const init = big ? "[3n,1n,2n]" : ty.startsWith("Float") ? "[3.5,NaN,-0]" : "[3,1,2]";
  const V = big ? "7n" : "7";
  for (const c of taCalls) add(run(`new ${ty}(${init})`, c.replace(/V/g, V)));
  for (const c of ["r.toSorted()", "r.toReversed()", "r.with(0,V)", "r.with(1,V)", "r.at(0)", "r.findLast(()=>1)"]) {
    add(run(`new ${ty}(0)`, c.replace(/V/g, V)));
    add(run(`new ${ty}(new ArrayBuffer(16),8,1)`, c.replace(/V/g, V)));
    add(run(`new ${ty}(new ArrayBuffer(16,{maxByteLength:32}))`, c.replace(/V/g, V)));
  }
}
for (const ty of ["Float32Array", "Float64Array"]) {
  for (const init of ["[NaN,1,-0,0,-1,Infinity,-Infinity]", "[0,-0,0,-0]", "[-0,0]", "[NaN,NaN,1]"]) {
    add(run(`new ${ty}(${init})`, "r.toSorted()"), run(`new ${ty}(${init})`, "r.toSorted((a,b)=>a-b)"), run(`new ${ty}(${init})`, "r.toSorted((a,b)=>b-a)"),
      run(`new ${ty}(${init})`, "r.toReversed()"), run(`new ${ty}(${init})`, "r.with(0,-0)"), run(`new ${ty}(${init})`, "r.with(1,NaN)"));
  }
}
for (const ty of ["BigInt64Array", "BigUint64Array"]) {
  for (const v of ["1", "'1'", "true", "null", "undefined", "1.5", "Symbol()", "{valueOf(){return 2n}}", "2n**64n", "-1n", "'x'", "'0x10'", "'  5  '"]) add(run(`new ${ty}(3)`, `r.with(0,${v})`));
}
for (const ty of ["Uint8Array", "Float64Array", "Int32Array"]) {
  for (const v of ["1n", "'1'", "true", "null", "undefined", "Symbol()", "{valueOf(){return 2}}", "{valueOf(){throw new EvalError('i')}}", "300", "-1", "1e10", "NaN", "'x'", "2**53"]) add(run(`new ${ty}(3)`, `r.with(0,${v})`));
}

// ---- Object.groupBy e Map.groupBy.
const iterables = [
  "[1,2,3,4,5]", "[1,,3]", "[]", "'abcab'", "new Set([1,2,2,3])", "new Map([[1,'a'],[2,'b']])", "[[1,2],[3,4]]", "(function*(){yield 1;yield 2;yield 3})()",
  "{length:2,0:1,1:2}", "{}", "5", "null", "undefined", "true", "Symbol()", "1n", "new Uint8Array([1,2,3,4])", "P([1,2,3])", "[{k:'a',v:1},{k:'b',v:2},{k:'a',v:3}]",
  "{[Symbol.iterator]:function*(){g('it');yield 1;yield 2}}", "{[Symbol.iterator](){g('it');return{next(){g('next');return{done:true}},return(){g('ret')}}}}",
  "{[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){g('ret');return{}}}}}", "[NaN,NaN,0,-0,'0',0n]", "[1.5,2.5,1.5]",
  "[undefined,null,undefined,null]", "[true,false,true]", "[{},{}]", "[[],[]]", "new String('aab')",
];
const keyFns = [
  "v=>v%2?'odd':'even'", "v=>v", "v=>String(v)", "v=>typeof v", "v=>null", "v=>undefined", "v=>NaN", "v=>-0", "v=>0", "v=>1n", "v=>true",
  "v=>({})", "v=>[1]", "v=>Symbol.iterator", "v=>Symbol('s')", "(v,i)=>i%2", "(v,i)=>'k'+i", "(v,i)=>i>1?'big':'small'", "v=>v&&v.k", "v=>{throw new RangeError('kf')}",
  "(v,i)=>{g('cb'+i);return i}", "v=>{g('cb'+String(v));return 'x'}", "v=>({toString(){g('ts');return 'o'}})", "v=>({toString(){throw new EvalError('ts')}})",
  "v=>({valueOf(){g('vo');return 1},toString(){g('ts');return 's'}})", "v=>'__proto__'", "v=>'constructor'", "v=>'toString'", "v=>'length'", "v=>1.5", "v=>1e21",
  "v=>2**32", "v=>'01'", "v=>Math.floor(v/2)",
  "function(){return this===undefined?'u':typeof this}", "undefined", "null", "1", "'f'", "{}", "Symbol()",
];
for (const it of iterables) {
  for (const f of keyFns) {
    add(run("0", `Object.groupBy(${it},${f})`), run("0", `Map.groupBy(${it},${f})`));
  }
  add(run("0", `Object.groupBy(${it})`), run("0", `Map.groupBy(${it})`), run("0", "Object.groupBy()"), run("0", "Map.groupBy()"));
}
add(
  run("0", "Object.groupBy.length+':'+Map.groupBy.length+':'+Object.groupBy.name+':'+Map.groupBy.name"),
  run("0", "Object.getPrototypeOf(Object.groupBy([1],v=>'a'))"), run("0", "Object.getPrototypeOf(Map.groupBy([1],v=>'a'))===Map.prototype"),
  run("0", "Object.getOwnPropertyDescriptor(Object.groupBy([1],v=>'a'),'a')"), run("0", "Object.isExtensible(Object.groupBy([1],v=>'a'))"),
  run("0", "Object.groupBy([1,2,3],v=>v>1?'a':'b').a.length"), run("0", "Reflect.ownKeys(Object.groupBy([1,2,3],v=>v>1?Symbol.iterator:'z')).map(String)"),
  run("0", "Object.keys(Object.groupBy([3,1,2,'10','2'],v=>v)).join()"), run("0", "Object.keys(Object.groupBy([1,2],v=>v===1?'b':'a')).join()"),
  run("0", "Map.groupBy([1,2,3],v=>v%2).get(1)"), run("0", "[...Map.groupBy([0,-0,1],v=>v).keys()].map(k=>Object.is(k,-0)?'-0':String(k)).join()"),
  run("0", "[...Map.groupBy([1,2],v=>({})).keys()].length"), run("0", "Map.groupBy.call(null,[1],v=>v).size"), run("0", "Object.groupBy.call(null,[1],v=>v)"),
  run("0", "Object.groupBy.call(5,[1],v=>v)"), run("0", "new Map.groupBy([1],v=>v)"), run("0", "new Object.groupBy([1],v=>v)"),
  run("0", "(Array.prototype.push=function(){throw new EvalError('push')},Object.groupBy([1,2],v=>'a'))"),
  run("0", "(Array.prototype[Symbol.iterator]=function*(){yield 9},Object.groupBy([1,2],v=>'a'))"),
  run("0", "Object.groupBy([1,2,3],(v,i)=>{if(i===0)Object.prototype.a=1;return 'a'})"),
  run("0", "Object.groupBy([1,2,3],v=>'x').hasOwnProperty"), run("0", "'x' in Object.groupBy([1],v=>'x')"), run("0", "Object.groupBy('ab',v=>v)"),
  run("0", "Object.groupBy([1,2],v=>'__proto__').__proto__"), run("0", "Object.getPrototypeOf(Object.groupBy([1,2],v=>'__proto__'))"),
  run("0", "Object.groupBy(Object.assign([1,2,3],{get 1(){g('g1');return 9}}),v=>v)"),
);

// ---- Estabilidade e comparadores em sort/toSorted.
const objs = "[{k:1,n:'a'},{k:0,n:'b'},{k:1,n:'c'},{k:0,n:'d'},{k:1,n:'e'},{k:0,n:'f'},{k:2,n:'g'},{k:1,n:'h'}]";
const stable = [objs, "Array.from({length:40},(_, i)=>({k:i%3,n:i}))", "Array.from({length:130},(_, i)=>({k:i%2,n:i}))", "Array.from({length:12},(_, i)=>({k:0,n:i}))",
  "Array.from({length:20},(_, i)=>({k:(i*7)%4,n:i}))", "Array.from({length:600},(_, i)=>({k:i%5,n:i}))"];
for (const recv of stable) {
  for (const name of ["sort", "toSorted"]) {
    for (const cmp of ["(a,b)=>a.k-b.k", "(a,b)=>b.k-a.k", "(a,b)=>a.k<b.k?-1:a.k>b.k?1:0", "(a,b)=>a.k>b.k", "(a,b)=>a.k<b.k", "(a,b)=>(a.k>b.k)-(a.k<b.k)", "()=>0", "(a,b)=>a.n<b.n?1:-1"]) {
      add(`T(()=>{var r=${recv};var x=Q(()=>AP.${name}.call(r,${cmp}).map(o=>o.k+':'+o.n).join());return x+" st="+(r.length>20?r.length:S(r.map(o=>o.n)))})`);
    }
  }
}
const sortInputs = [
  "[3,1,2]", "[undefined,3,,1]", "['b','a',10,9,1]", "[NaN,1,-0,0,-1]", "[5,4,3,2,1,0,9,8,7,6,5,4]", "[true,false,null,undefined,'x']", "[[2],[1,1],[1],[]]",
  "[,,3,,1,,]", "[undefined,undefined]", "[1,,undefined,,0]", "['a','B','c','A']", "[10,9,1,100,'1']", "[1n,3n,2n]", "[{},{}]", "Array.from({length:50},(_, i)=>(i*37)%50)",
];
const comparators = [
  "undefined", "(a,b)=>a-b", "(a,b)=>b-a", "()=>0", "()=>1", "()=>-1", "()=>NaN", "()=>'x'", "(a,b)=>a<b", "()=>null", "()=>undefined", "()=>Infinity", "()=>-Infinity",
  "()=>true", "()=>({})", "()=>1n", "(a,b)=>{g(String(a)+','+String(b));return 0}", "(a,b)=>Math.random()*0",
  "()=>({valueOf(){return -1}})", "()=>({valueOf(){throw new EvalError('vo')}})", "()=>({valueOf(){g('vo');return 1}})", "()=>{throw new RangeError('cmp')}",
  "(a,b)=>{if(a===1)throw new RangeError('one');return 0}", "()=>Symbol()", "(a,b)=>String(a)<String(b)?-1:1", "(a,b)=>a===undefined?-1:1",
  "null", "{}", "1", "'f'", "false", "Symbol()", "[]", "class{}", "async()=>0", "function*(){}", "Math.max",
];
for (const recv of sortInputs) {
  for (const cmp of comparators) {
    add(run(recv, `AP.toSorted.call(r,${cmp})`), run(recv, `AP.sort.call(r,${cmp})`));
  }
}
// Comparador que muta o array durante o sort/toSorted.
const mutations = [
  "none", "AP.push.call(r,9)", "AP.pop.call(r)", "AP.shift.call(r)", "AP.unshift.call(r,0)", "r.length=1", "r.length=0", "delete r[2]", "AP.splice.call(r,1,1)",
  "AP.reverse.call(r)", "r[7]=7", "AP.sort.call(r)", "Object.freeze(r)", "r.length=10", "r[1]=100", "AP.fill.call(r,0)",
];
for (const recv of ["[3,1,2,5,4]", "P([3,1,2,5,4])", "{length:5,0:3,1:1,2:2,3:5,4:4}", "[3,,2,5,4]", "[3,1,undefined,5,4]", "Array.from({length:20},(_, i)=>(i*7)%20)"]) {
  for (const name of ["sort", "toSorted"]) {
    for (const m of mutations) {
      const body = m === "none" ? "" : `if(n++==0){${m}}`;
      add(`T(()=>{var n=0;var r=${recv};var x=Q(()=>AP.${name}.call(r,function(x,y){g("c"+x+","+y);${body}return x<y?-1:x>y?1:0}));return x+" st="+K(r)})`);
    }
  }
}
// Mutação durante callbacks de findLast/findLastIndex e getters que mudam o length.
for (const recv of ["[1,2,3,4]", "[1,,3,4]", "P([1,2,3,4])", "P({length:4,0:1,1:2,3:4})", "{length:4,0:1,1:2,2:3,3:4}"]) {
  for (const name of ["findLast", "findLastIndex"]) {
    for (const m of mutations.filter((x) => x !== "none" && x !== "AP.sort.call(r)")) {
      add(`T(()=>{var n=0;var r=${recv};var x=Q(()=>AP.${name}.call(r,function(v,i){g("cb"+i+":"+v);if(n++==0){${m}}return false}));return x+" st="+K(r)})`);
    }
  }
  for (const m of ["AP.push.call(r,9)", "r.length=1", "delete r[0]", "AP.pop.call(r)"]) {
    add(`T(()=>{var r=${recv};var x=Q(()=>AP.toSpliced.call(r,{valueOf(){${m};return 1}},1));return x+" st="+K(r)})`);
    add(`T(()=>{var r=${recv};var x=Q(()=>AP.with.call(r,{valueOf(){${m};return 1}},'w'));return x+" st="+K(r)})`);
    add(`T(()=>{var r=${recv};var x=Q(()=>AP.at.call(r,{valueOf(){${m};return 1}}));return x+" st="+K(r)})`);
  }
}
// Buracos herdados do protótipo.
for (const proto of ["{1:'p'}", "{0:'p',1:'q',2:'r'}", "{get 1(){g('pg');return 'p'}}"]) {
  for (const call of ["r.toSorted()", "r.toReversed()", "r.toSpliced(0,0)", "r.with(0,'w')", "r.findLast(v=>g('f'+v))", "r.findLastIndex(v=>false)", "r.at(1)", "r.toSorted((a,b)=>String(a)<String(b)?-1:1)", "r.toSpliced(1,1)"]) {
    add(`T(()=>{var h=Object.setPrototypeOf([]  ,null);var o=${proto};var p=Object.create(AP);Object.setPrototypeOf(AP,Object.prototype);Object.setPrototypeOf(p,o);var r=Object.setPrototypeOf([1,,3],p);var x=Q(()=>${call});return x+"|"+Object.keys(r).join()})`);
  }
}
// Subclasses, species e construtor: os métodos de cópia devolvem Array comum.
add(
  run("new (class A extends Array{})(3,1,2)", "r.toSorted() instanceof Array&&r.toSorted().constructor===Array"),
  run("new (class A extends Array{})(3,1,2)", "r.toReversed().constructor===Array"), run("new (class A extends Array{})(3,1,2)", "r.toSpliced(0,1).constructor===Array"),
  run("new (class A extends Array{})(3,1,2)", "r.with(0,1).constructor===Array"),
  run("Object.assign([3,1,2],{constructor:{[Symbol.species]:function(){throw new EvalError('sp')}}})", "r.toSorted()"),
  run("Object.assign([3,1,2],{constructor:{[Symbol.species]:function(){throw new EvalError('sp')}}})", "r.toReversed()"),
  run("Object.assign([3,1,2],{constructor:{[Symbol.species]:function(){throw new EvalError('sp')}}})", "r.toSpliced(0,1)"),
  run("Object.assign([3,1,2],{constructor:{[Symbol.species]:function(){throw new EvalError('sp')}}})", "r.with(0,1)"),
  run("[3,1,2]", "AP.toSorted.name+AP.toSorted.length+AP.toReversed.length+AP.toSpliced.length+AP.with.length+AP.findLast.length+AP.findLastIndex.length+AP.at.length"),
  run("[3,1,2]", "Object.keys(AP[Symbol.unscopables]).join()"), run("[3,1,2]", "AP[Symbol.unscopables].toSorted+':'+AP[Symbol.unscopables].with+':'+AP[Symbol.unscopables].at+':'+AP[Symbol.unscopables].findLast"),
  run("[3,1,2]", "new AP.toSorted()"), run("[3,1,2]", "new AP.with(0,1)"), run("[3,1,2]", "AP.toSorted.hasOwnProperty('prototype')"),
  run("[1,2,3]", "r.toSpliced(0,0,...Array(10).fill(1)).length"), run("[1,2,3]", "r.toSpliced(1,1,r).length"), run("[1,2,3]", "r.toSpliced(1,1,r)[1]===r"),
  run("[3,1,2]", "r.toSorted()===r"), run("[3,1,2]", "r.toReversed()===r"), run("[3,1,2]", "r.with(0,0)===r"),
  run("[1,2,3]", "(delete AP[0],r.with(0,5))"), run("[,1]", "r.toSorted().hasOwnProperty(1)"), run("[,1]", "r.toReversed().hasOwnProperty(1)"),
  run("[,1]", "r.with(0,1).hasOwnProperty(0)"), run("[,1]", "r.toSpliced(1,0).hasOwnProperty(0)"), run("[1,,2]", "r.with(1,5)"),
  run("[1,,2]", "r.toSpliced(0,0)"), run("[1,,2]", "r.toSpliced(0,1)"), run("[1,,2]", "r.toSorted()"), run("[1,,2]", "r.toReversed()"),
  run("Object.assign([1,2,3],{length:2**32-1})", "r.toSorted()"), run("Object.assign([1,2,3],{length:2**32-1})", "r.with(0,1)"),
  run("new Array(2**32-1)", "r.toReversed()"), run("new Array(2**32-1)", "r.with(0,1)"), run("new Array(2**32-1)", "r.toSpliced(0,0,1)"),
  run("new Array(2**32-1)", "r.toSorted()"), run("new Array(2**31)", "r.toSpliced(0,0,1,2)"), run("new Array(2**32-1)", "r.at(-1)"),
  run("new Array(2**32-1)", "r.findLast(v=>true)"), run("new Array(2**32-1)", "r.findLastIndex(v=>true)"),
  run("[1,2,3]", "r.toSpliced(0,0,...new Array(2**20))"), run("{length:2**32-3}", "AP.toSpliced.call(r,0,0,1,2,3)"), run("{length:2**53-1}", "AP.toSpliced.call(r,0,0,1)"),
  run("{length:2**53-1}", "AP.toSpliced.call(r,0,1,1)"), run("{length:2**53-2}", "AP.toSpliced.call(r,0,0,1)"), run("{length:2**53-2}", "AP.toSpliced.call(r,0,0,1,2)"),
);

// ---- Mensagens exatas em receptores nulos e primitivos, para cada método.
for (const name of [...Object.keys(methods), "sort", "toLocaleString"]) {
  for (const th of ["undefined", "null", "Symbol()", "1n", "5", "'s'", "true", "{}", "[]", "new Proxy({}, {})", "()=>{}"]) {
    add(`T(()=>Q(()=>AP.${name}.call(${th})))`, `T(()=>Q(()=>AP.${name}.call(${th},()=>0)))`);
  }
  add(`T(()=>Q(()=>AP.${name}.call()))`, `T(()=>Q(()=>AP.${name}.apply(null)))`, `T(()=>Q(()=>AP.${name}()))`, `T(()=>Q(()=>(0,AP.${name})()))`,
    `T(()=>Q(()=>Reflect.apply(AP.${name},undefined,[])))`);
}
for (const name of ["toSorted", "toReversed", "with", "at", "findLast", "findLastIndex"]) {
  for (const th of ["undefined", "null", "5", "[]", "Symbol()"]) add(`T(()=>Q(()=>TAP.${name}.call(${th})))`);
}
for (const th of ["undefined", "null"]) {
  add(`T(()=>Q(()=>Object.groupBy.call(${th},[1],v=>v)))`, `T(()=>Q(()=>Map.groupBy.call(${th},[1],v=>v)))`);
}

// ---- Execução.
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const programs = unique.map((expr) => '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);

const known = new Set(knownPrograms("array_copy_bun.tsv", (name) => /^(array|typedarray|object|map)/.test(name)));
let dup = 0;
const todo = programs.filter((p) => (known.has(p) ? (dup++, false) : true));

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 8000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", () => {});
    child.on("close", (code) => {
      clearTimeout(timer);
      const result = decodeResult(out);
      resolve({ ok: code === 0 && !timedOut && result !== null, out: result, timedOut });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(todo.length);
  let next = 0;
  async function worker() {
    while (next < todo.length) {
      const i = next++;
      results[i] = await runChild(todo[i]);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  const dash = String.fromCharCode(0x2014);
  const endash = String.fromCharCode(0x2013);
  for (let i = 0; i < todo.length; i++) {
    const r = results[i];
    if (!r.ok || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || r.out.includes(dash) || r.out.includes(endash)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(todo[i].slice(todo[i].indexOf("globalThis.R"))).slice(0, 160) + (r.timedOut ? " timeout" : "") + "\n");
      continue;
    }
    kept++;
    lines.push(JSON.stringify(todo[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("array_copy", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
