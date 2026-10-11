// Gera tests/golden/array_generic_bun.tsv: Array.prototype em receptores array-like exóticos, medido no bun 1.4.2.
// Cobre objetos com getter de length que loga, length negativo/fracionário/2**53, Proxy que loga os traps, String boxed,
// arguments, typed array via call, mutadores com a ordem exata de get/set/has/delete, callbacks que mutam o receptor durante
// a iteração, comparadores de sort que mutam, flat/flatMap, Symbol.isConcatSpreadable, toSpliced/with/toSorted e mensagens.
// Cada programa grava em `globalThis.R` o resultado, o estado final do receptor e o log dos traps. Um bun filho por
// programa (a reificação das tabelas estáticas depende da ordem de acesso), no máximo 8 em paralelo, com timeout de 5 s.
// Programas cuja fonte já existe em algum tests/golden/*.tsv são descartados.
// Uso: bun scripts/gen-array-generic-golden.js > tests/golden/array_generic_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'var L=[],Z=0,AP=Array.prototype;function g(s){Z||L.push(s)}\n' +
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return"fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>2)return"..";' +
  'if(Array.isArray(v)){var o=[];for(var i=0;i<Math.min(v.length,12);i++)o.push(i in v?S(v[i],d+1):"<h>");return"["+o.join()+"]"+v.length}' +
  'return"{"+Object.keys(v).slice(0,12).map(k=>k+":"+S(v[k],d+1)).join()+"}"}\n' +
  'function P(o){return new Proxy(o,{get(t,k,r){g("g:"+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){g("s:"+String(k));return Reflect.set(t,k,v,r)},' +
  'has(t,k){g("h:"+String(k));return Reflect.has(t,k)},deleteProperty(t,k){g("d:"+String(k));return Reflect.deleteProperty(t,k)},' +
  'defineProperty(t,k,d){g("D:"+String(k));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){g("o:"+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})}\n' +
  'function Q(f){try{var v=f();Z=1;return S(v)}catch(e){return"throw "+e.name+": "+e.message}finally{Z=0}}\n' +
  'function K(r){Z=1;try{var o=Object(r);return Reflect.ownKeys(o).map(k=>String(k)+"="+S(o[k])).join()}catch(e){return"err"}finally{Z=0}}\n' +
  'function T(f){var x;try{x=f()}catch(e){x="throw "+e.name+": "+e.message}return x+"|"+L.join()}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const run = (recv, call) => `T(()=>{var r=${recv};var x=Q(()=>${call});return x+" st="+K(r)})`;

// ---- Receptores de length pequeno, seguros para métodos que iteram.
const receivers = [
  "[1,2,3]", "[1,,3]", "P([1,2,3])", "P([1,,3])", "P([])", 'P({length:3,0:"a",1:"b",2:"c"})', 'P({length:2,0:"a",2:"z"})',
  '{length:3,0:"a",1:"b",2:"c"}', '{get length(){g("len");return 3},0:"a",1:"b",2:"c"}',
  '{get length(){g("len");return 3},get 0(){g("g0");return"a"},get 1(){g("g1");return"b"},2:"c"}',
  '{length:2.7,0:"a",1:"b",2:"c"}', '{length:-5,0:"a"}', '{length:"2",0:"a",1:"b"}', '{length:{valueOf(){g("vo");return 2}},0:"a",1:"b"}',
  '{0:"a"}', 'new String("abc")', 'Object("")', "(function(){return arguments})(1,2,3)", "(function(){return arguments})()",
  "new Uint8Array([3,1,2])", "new Float64Array([1.5,-0,NaN])", "Object.freeze([1,2,3])", 'Object.freeze({length:2,0:"a",1:"b"})',
  "Object.seal([1,2,3])", "Object.create([1,2,3])", 'Object.assign(Object.create({1:"p",length:2}),{0:"a"})',
  'Object.defineProperty([1,2,3],"1",{get(){g("g1");return 9},set(v){g("s1")}})',
  'Object.defineProperty({length:2,0:"a",1:"b"},"1",{writable:false})', "P(Object.freeze([1,2,3]))", "null", "undefined", "5", '"xyz"', "true",
  "P(new Uint8Array([1,2]))", '{get length(){throw new RangeError("len boom")}}', "{length:Symbol()}", "{length:1n}", "{length:[2],0:1,1:2}",
];
const methods = {
  push: ["4", "", "1,2", "undefined"], pop: [""], shift: [""], unshift: ["0", "", "1,2"],
  splice: ["1,1", "0", "1,0,'x'", "", "-1", "1,1,'x','y'", "undefined", "0,undefined", "-5,2", "1,-1", "1.9,1.2", "'1','1'", "NaN,NaN"],
  copyWithin: ["0,1", "1,0", "0,1,2", "-1,0", "1", "0,-1,-2"], fill: ["9", "9,1", "9,1,2", "9,-1", "9,NaN", "9,undefined,1"], reverse: [""],
  sort: ["", "(a,b)=>b>a?1:-1", "undefined", "null", "{}", "(a,b)=>{L.push('c'+a+b);return 0}"],
  slice: ["", "1", "1,2", "-1", "0,-1"], concat: ["", "4", "[4,5]", "[4,[5]]"],
  indexOf: ["'b'", "'b',1", "'b',-1", "'a',{valueOf(){g('vf');return 1}}"], lastIndexOf: ["'b'", "'b',1", "'a',-5"],
  includes: ["'b'", "undefined", "NaN", "'a',-1"], join: ["", "'-'", "undefined", "null"], toString: [""], toLocaleString: [""],
  at: ["0", "-1", "5", "'1'", "NaN"], with: ["0,'w'", "-1,'w'", "5,'w'", "-9,'w'", "1.5,'w'"],
  toSpliced: ["", "1", "1,1", "1,1,'x'", "0,0,'a','b'", "-1,1"], toSorted: ["", "(a,b)=>b>a?1:-1", "undefined", "null", "1"], toReversed: [""],
  flat: ["", "0", "2", "Infinity"],
};
for (const recv of receivers) {
  for (const [name, argList] of Object.entries(methods)) {
    for (const args of argList) add(run(recv, `AP.${name}.call(r${args ? "," + args : ""})`));
  }
  for (const it of ["keys", "values", "entries"]) add(run(recv, `[...AP.${it}.call(r)]`));
}

// ---- Receptores enormes (length 2**53, Infinity, 2**32) só com métodos que não percorrem tudo.
const bigReceivers = [
  "{length:2**53-1}", "{length:2**53}", "{length:Infinity}", "{length:2**32}", "{length:2**32+1}", "{length:4294967295}",
  'P({length:2**53-1,[2**53-2]:"z"})', '{length:2**53-1,[2**53-2]:"z"}', '{length:4294967296,4294967295:"x"}', "{length:-Infinity}", "{length:1e300}",
];
const bigCalls = [
  "AP.push.call(r,1)", "AP.push.call(r)", "AP.push.call(r,1,2)", "AP.pop.call(r)", "AP.at.call(r,-1)", "AP.at.call(r,0)", "AP.slice.call(r,-2)",
  "AP.splice.call(r,-1,1)", "AP.fill.call(r,0,-2)", "AP.copyWithin.call(r,-2,-1)", "AP.includes.call(r,1,-2)", "AP.indexOf.call(r,1,-2)",
  "AP.lastIndexOf.call(r,1,1)", "AP.with.call(r,-1,1)", "AP.toReversed.call(r)", "AP.toSorted.call(r)", "AP.toSpliced.call(r,0,0)",
  "AP.unshift.call(r)", "AP.unshift.call(r,1)", "AP.concat.call(1,r)", "AP.splice.call(r,0,0,1)", "AP.slice.call(r,2**53-3)",
];
for (const recv of bigReceivers) for (const call of bigCalls) add(run(recv, call));

// ---- Callbacks que mutam o receptor durante a iteração.
const cbReceivers = ["[1,2,3,4]", "[1,,3,4]", "P([1,2,3,4])", "P({length:4,0:1,1:2,3:4})"];
const mutations = [
  "none", "AP.push.call(r,9)", "AP.pop.call(r)", "AP.shift.call(r)", "AP.unshift.call(r,0)", "r.length=1", "delete r[2]", "AP.splice.call(r,1,1)",
  "AP.reverse.call(r)", "r[5]=7", "r.length=0",
];
const cbMethods = {
  forEach: ["function(v,i){BODY;}", 0], map: ["function(v,i){BODY;return v}", 0], filter: ["function(v,i){BODY;return true}", 0],
  some: ["function(v,i){BODY;return false}", 0], every: ["function(v,i){BODY;return true}", 0], find: ["function(v,i){BODY;return false}", 0],
  findIndex: ["function(v,i){BODY;return false}", 0], findLast: ["function(v,i){BODY;return false}", 0], findLastIndex: ["function(v,i){BODY;return false}", 0],
  reduce: ["function(a,v,i){BODY;return v}", 0], reduceRight: ["function(a,v,i){BODY;return v}", 0], flatMap: ["function(v,i){BODY;return[v,[v]]}", 0],
};
for (const recv of cbReceivers) {
  for (const [name, [tpl]] of Object.entries(cbMethods)) {
    for (const m of mutations) {
      const body = `L.push("cb"+i+":"+v);${m === "none" ? "" : `if(n++==0){${m}}`}`;
      add(`T(()=>{var n=0;var r=${recv};var x=Q(()=>AP.${name}.call(r,${tpl.replace("BODY", body)}));return x+" st="+K(r)})`);
    }
  }
}
// thisArg e callback inválido.
for (const name of Object.keys(cbMethods)) {
  for (const cb of ["undefined", "null", "1", "{}", "'f'", "Symbol()"]) add(run("[1,2]", `AP.${name}.call(r,${cb})`));
  add(run("P([1,2])", `AP.${name}.call(r,function(){L.push(typeof this+":"+(this&&this.t));return 0},{t:1})`));
  add(run('{length:2,0:"a",1:"b"}', `AP.${name}.call(r,function(){"use strict";L.push(String(this));return 0})`));
  add(run("[]", `AP.${name}.call(r,function(){L.push("never");return 0})`));
}

// ---- sort/toSorted com comparador que muta o receptor.
const sortReceivers = ["[3,1,2,5,4]", "P([3,1,2,5,4])", "{length:5,0:3,1:1,2:2,3:5,4:4}", "[3,,2,5,4]", "[3,1,undefined,5,4]"];
for (const recv of sortReceivers) {
  for (const name of ["sort", "toSorted"]) {
    for (const m of mutations) {
      const body = m === "none" ? "" : `if(n++==0){${m}}`;
      add(`T(()=>{var n=0;var r=${recv};var x=Q(()=>AP.${name}.call(r,function(x,y){L.push("c"+x+","+y);${body}return x<y?-1:x>y?1:0}));return x+" st="+K(r)})`);
    }
    add(run(recv, `AP.${name}.call(r,function(){throw new RangeError("cmp")})`));
    add(run(recv, `AP.${name}.call(r,function(){return {valueOf(){L.push("vo");return 1}}})`));
    add(run(recv, `AP.${name}.call(r,function(){return NaN})`));
    add(run(recv, `AP.${name}.call(r,function(){return "-1"})`));
  }
}

// ---- flat / flatMap: profundidade e Symbol.isConcatSpreadable.
const nested = [
  "[1,[2,[3,[4,[5]]]]]", "[1,,[2,,3]]", "[[],[[]],[[[]]]]", '[{length:1,0:"a"},[1]]', "[P([1,[2]]),[3]]", "[[1,2],P([3,P([4])])]",
  "P([[1],[2,[3]]])", "(function(){return[arguments,[arguments]]})(1,2)", "[new String('ab'),['ab']]", "[[1],,[2]]", "[1,[2,[3,[4,[5,[6,[7]]]]]]]",
];
const depths = ["", "0", "1", "2", "Infinity", "-1", "'1'", "NaN", "1.9", "{valueOf(){g('vo');return 1}}", "undefined", "null", "2**53", "-Infinity", "true"];
for (const recv of nested) for (const d of depths) add(run(recv, `AP.flat.call(r${d === "" ? "" : "," + d})`));
for (const recv of nested.slice(0, 7)) {
  for (const f of ["v=>v", "v=>[v]", "v=>[[v]]", "v=>P([v])", "(v,i,a)=>{L.push('m'+i);return[i]}", "v=>({length:1,0:v})", "v=>{throw new EvalError('fm')}", "v=>undefined"]) {
    add(run(recv, `AP.flatMap.call(r,${f})`));
  }
}
const spreadItems = [
  '{length:2,0:"a",1:"b",[Symbol.isConcatSpreadable]:true}', "Object.assign([1,2],{[Symbol.isConcatSpreadable]:false})", "{[Symbol.isConcatSpreadable]:true}",
  '{length:2,0:"x",[Symbol.isConcatSpreadable]:true}', 'P({length:2,0:"a",1:"b",[Symbol.isConcatSpreadable]:true})', "P([1,2])",
  "{get [Symbol.isConcatSpreadable](){g('spread');return true},length:1,0:'z'}", "{[Symbol.isConcatSpreadable]:1,length:1,0:'o'}",
  "{[Symbol.isConcatSpreadable]:undefined,length:1,0:'u'}", "{[Symbol.isConcatSpreadable]:null,length:1,0:'n'}", "new String('hi')",
  "Object.assign(new String('hi'),{[Symbol.isConcatSpreadable]:true})", "(function(){return arguments})(7,8)",
  "Object.assign((function(){return arguments})(7,8),{[Symbol.isConcatSpreadable]:true})", "new Uint8Array([1,2])",
  "Object.assign(new Uint8Array([1,2]),{[Symbol.isConcatSpreadable]:true})", "{length:2**53-1,[Symbol.isConcatSpreadable]:true}",
  "{length:2**53,[Symbol.isConcatSpreadable]:true}", "{length:-1,[Symbol.isConcatSpreadable]:true}", "{length:1.5,0:'f',[Symbol.isConcatSpreadable]:true}",
  "[,]", "Object.assign([1],{length:3})", "{get length(){g('len');return 2},0:'p',1:'q',[Symbol.isConcatSpreadable]:true}",
  "P(Object.assign([1,2],{[Symbol.isConcatSpreadable]:false}))", "{get [Symbol.isConcatSpreadable](){throw new TypeError('sp')}}",
];
const concatThis = ["[0]", "P([0])", '{length:1,0:"t"}', "new String('s')", "(function(){return arguments})(0)", "1", "null", "Object.assign([0],{[Symbol.isConcatSpreadable]:false})", "P({length:1,0:'t',[Symbol.isConcatSpreadable]:true})"];
for (const it of spreadItems) {
  for (const th of concatThis) add(run(th, `AP.concat.call(r,${it})`));
}
for (const it of spreadItems.slice(0, 12)) add(run("[0]", `r.concat(${it},${it})`));
add(
  run("[0]", "r.concat()"), run("[]", "r.concat([],[],[])"), run("[,1]", "r.concat([2,,3])"), run("[1]", "r.concat(Object.assign([2],{[Symbol.isConcatSpreadable]:undefined}))"),
  run("[1]", "(AP[Symbol.isConcatSpreadable]=true,r.concat('ab'))"), run("[1]", "(Object.prototype[Symbol.isConcatSpreadable]=true,r.concat({length:1,0:'w'}))"),
);
add(
  run("[0]", "(delete Object.prototype[Symbol.isConcatSpreadable],AP.concat.call(r,{length:1,0:'n'}))"),
);

// ---- Mutadores com valores de índice e length cruzados (ordem exata de get/set/has/delete).
const mutProxies = ["P([1,2,3,4,5])", "P([1,,3,,5])", "P({length:5,0:1,2:3,4:5})", "P({length:3,0:'a',1:'b',2:'c'})"];
const mutCalls = [
  "AP.reverse.call(r)", "AP.shift.call(r)", "AP.unshift.call(r,'u')", "AP.unshift.call(r,'u','v')", "AP.pop.call(r)", "AP.push.call(r,'p')",
  "AP.splice.call(r,1,2)", "AP.splice.call(r,1,2,'x')", "AP.splice.call(r,1,0,'x','y')", "AP.splice.call(r,1,1,'x')", "AP.splice.call(r,0)", "AP.splice.call(r,2)",
  "AP.copyWithin.call(r,0,2)", "AP.copyWithin.call(r,2,0)", "AP.copyWithin.call(r,1,3,5)", "AP.fill.call(r,0,1,3)", "AP.fill.call(r,0)",
  "AP.sort.call(r)", "AP.sort.call(r,(a,b)=>b-a)", "AP.toSorted.call(r)", "AP.toSpliced.call(r,1,2,'z')", "AP.with.call(r,1,'w')", "AP.toReversed.call(r)",
  "AP.slice.call(r,1,3)", "AP.indexOf.call(r,undefined)", "AP.includes.call(r,undefined)", "AP.lastIndexOf.call(r,3)", "AP.join.call(r)", "AP.at.call(r,-2)",
];
for (const recv of mutProxies) for (const call of mutCalls) add(run(recv, call));
// Proxy com traps que falham ou devolvem false.
const failTraps = {
  "set false": "set(t,k,v){g('s:'+String(k));return false}", "deleteProperty false": "deleteProperty(t,k){g('d:'+String(k));return false}",
  "defineProperty false": "defineProperty(t,k,d){g('D:'+String(k));return false}", "set throws": "set(t,k,v){g('s:'+String(k));throw new EvalError('set')}",
  "has throws": "has(t,k){g('h:'+String(k));throw new EvalError('has')}", "get throws": "get(t,k,r){g('g:'+String(k));if(k==='1')throw new EvalError('get');return Reflect.get(t,k,r)}",
  "delete throws": "deleteProperty(t,k){g('d:'+String(k));throw new EvalError('del')}", "has lies": "has(t,k){g('h:'+String(k));return false}",
};
for (const trap of Object.values(failTraps)) {
  for (const call of ["AP.reverse.call(r)", "AP.shift.call(r)", "AP.unshift.call(r,0)", "AP.pop.call(r)", "AP.push.call(r,0)", "AP.splice.call(r,1,1)", "AP.copyWithin.call(r,0,1)", "AP.fill.call(r,0)", "AP.sort.call(r)", "AP.slice.call(r)", "AP.indexOf.call(r,2)", "AP.map.call(r,v=>v)", "AP.toSpliced.call(r,0,1)", "AP.with.call(r,0,1)"]) {
    add(`T(()=>{var r=new Proxy([1,2,3],{${trap}});var x=Q(()=>${call});return x+" st="+K(r)})`);
  }
}

// ---- Mensagens exatas de erro em receptores e argumentos inválidos.
for (const name of Object.keys(methods)) {
  for (const th of ["undefined", "null"]) add(`T(()=>Q(()=>AP.${name}.call(${th})))`);
  add(`T(()=>Q(()=>AP.${name}.call(Symbol())))`, `T(()=>Q(()=>AP.${name}.call(1n)))`, `T(()=>Q(()=>AP.${name}.apply()))`);
}
add(
  "T(()=>Q(()=>AP.with.call([1,2],2,0)))", "T(()=>Q(()=>AP.with.call([1,2],-3,0)))", "T(()=>Q(()=>AP.with.call({length:2**32},0,0)))",
  "T(()=>Q(()=>AP.toSpliced.call({length:2**32},0,0)))", "T(()=>Q(()=>AP.toSpliced.call({length:2**53-1},0,0,1)))", "T(()=>Q(()=>AP.toSorted.call({length:2**32})))",
  "T(()=>Q(()=>AP.toReversed.call({length:2**32})))", "T(()=>Q(()=>AP.toSorted.call([2,1],'x')))", "T(()=>Q(()=>AP.sort.call([2,1],'x')))",
  "T(()=>Q(()=>AP.sort.call([2,1],{})))", "T(()=>Q(()=>AP.push.call({length:2**53-1},1)))", "T(()=>Q(()=>AP.unshift.call({length:2**53-1},1)))",
  "T(()=>Q(()=>AP.splice.call({length:2**53-1},0,0,1)))", "T(()=>Q(()=>AP.concat.call([],{length:2**53-1,[Symbol.isConcatSpreadable]:true},[1])))",
  "T(()=>Q(()=>AP.flat.call([[1]],Symbol())))", "T(()=>Q(()=>AP.flat.call([[1]],1n)))", "T(()=>Q(()=>AP.fill.call([1],0,Symbol())))",
  "T(()=>Q(()=>AP.at.call([1],Symbol())))", "T(()=>Q(()=>AP.slice.call([1],1n)))", "T(()=>Q(()=>AP.join.call([1,2],Symbol())))",
  "T(()=>Q(()=>AP.join.call([1,Symbol()])))", "T(()=>Q(()=>AP.toString.call(null)))", "T(()=>Q(()=>AP.toString.call({join:1})))",
  "T(()=>Q(()=>AP.toString.call({join(){return 'J'}})))", "T(()=>Q(()=>AP.toLocaleString.call([1,null,undefined,{toLocaleString(){return 'tl'}}])))",
  "T(()=>Q(()=>AP.toLocaleString.call([{toLocaleString(){return 1}}])))", "T(()=>Q(()=>AP.toLocaleString.call([{toLocaleString:1}])))",
  "T(()=>Q(()=>AP.join.call(Object.assign([1,2],{length:3}),'-')))", "T(()=>Q(()=>AP.reduce.call([],(a,b)=>a)))", "T(()=>Q(()=>AP.reduceRight.call({length:0},(a,b)=>a)))",
  "T(()=>Q(()=>AP.reduce.call({length:3,1:'x'},(a,b)=>a+b)))", "T(()=>Q(()=>AP.reduceRight.call({length:3,1:'x'},(a,b)=>a+b)))",
  "T(()=>Q(()=>AP.reduce.call([1,2],(a,b)=>a+b,undefined)))", "T(()=>Q(()=>AP.at.call('str',-1)))", "T(()=>Q(()=>AP.map.call('ab',v=>v+v)))",
  "T(()=>Q(()=>AP.push.call('ab','c')))", "T(()=>Q(()=>AP.pop.call('ab')))", "T(()=>Q(()=>AP.reverse.call('ab')))", "T(()=>Q(()=>AP.fill.call('ab',0)))",
  "T(()=>Q(()=>AP.sort.call('ba')))", "T(()=>Q(()=>AP.shift.call('ab')))", "T(()=>Q(()=>AP.splice.call('ab',0,1)))", "T(()=>Q(()=>AP.copyWithin.call('ab',0,1)))",
  "T(()=>Q(()=>AP.push.call(Object.freeze([]),1)))", "T(()=>Q(()=>AP.push.call(Object.preventExtensions([]),1)))", "T(()=>Q(()=>AP.push.call(Object.defineProperty([],'length',{writable:false}),1)))",
  "T(()=>Q(()=>AP.pop.call(Object.defineProperty([1],'length',{writable:false}))))", "T(()=>Q(()=>AP.pop.call(Object.defineProperty([],'length',{writable:false}))))",
  "T(()=>Q(()=>AP.shift.call(Object.defineProperty([],'length',{writable:false}))))", "T(()=>Q(()=>AP.unshift.call(Object.defineProperty([1],'length',{writable:false}))))",
  "T(()=>Q(()=>AP.splice.call(Object.defineProperty([1,2],'length',{writable:false}),0,1)))", "T(()=>Q(()=>AP.splice.call(Object.defineProperty([1,2],'length',{writable:false}),0,0)))",
  "T(()=>Q(()=>AP.push.call({get length(){return 1},set length(v){throw new EvalError('setlen')}},1)))",
  "T(()=>Q(()=>AP.pop.call({get length(){return 1},0:'a',set length(v){g('setlen:'+v)}})))",
  "T(()=>Q(()=>AP.shift.call({get length(){return 2},0:'a',1:'b',set length(v){g('setlen:'+v)}})))",
  "T(()=>Q(()=>AP.unshift.call({get length(){return 2},0:'a',1:'b',set length(v){g('setlen:'+v)}},'z')))",
  "T(()=>Q(()=>AP.splice.call({get length(){return 2},0:'a',1:'b',set length(v){g('setlen:'+v)}},0,1)))",
  "T(()=>Q(()=>AP.reverse.call({get length(){g('len');return 3},0:'a',2:'c'})))",
  "T(()=>Q(()=>AP.fill.call({get length(){g('len');return 2},set 0(v){g('s0')},set 1(v){g('s1')}},7)))",
  "T(()=>Q(()=>AP.sort.call({get length(){g('len');return 3},0:'c',1:'a',2:'b'})))",
  "T(()=>Q(()=>AP.sort.call({get length(){g('len');return 3},0:'c',2:'b'})))",
  "T(()=>Q(()=>AP.sort.call({length:3,0:undefined,2:'a'})))",
);

// ---- Execução.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const programs = unique.map(expr => '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);

const goldenDir = path.join(__dirname, "..", "tests", "golden");
const candidates = new Set(programs.map(p => JSON.stringify(p)));
let dup = 0;
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "array_generic_bun.tsv") continue;
  const text = fs.readFileSync(path.join(goldenDir, file), "latin1");
  let start = 0;
  while (start < text.length) {
    let end = text.indexOf("\n", start);
    if (end < 0) end = text.length;
    const tab = text.indexOf("\t", start);
    if (tab > 0 && tab < end) {
      const key = Buffer.from(text.slice(start, tab), "latin1").toString("utf8");
      if (candidates.delete(key)) dup++;
    }
    start = end + 1;
  }
}
const todo = programs.filter(p => candidates.has(JSON.stringify(p)));

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 5000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", () => {});
    child.on("close", code => {
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
  let dropped = 0;
  async function worker() {
    while (next < todo.length) {
      const i = next++;
      results[i] = await runChild(todo[i]);
    }
  }
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  const lines = [];
  for (let i = 0; i < todo.length; i++) {
    const r = results[i];
    if (!r.ok || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("descartado: " + JSON.stringify(todo[i].slice(todo[i].indexOf("globalThis.R"))).slice(0, 160) + (r.timedOut ? " timeout" : "") + "\n"); continue; }
    kept++;
    lines.push(JSON.stringify(todo[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("array_generic", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
