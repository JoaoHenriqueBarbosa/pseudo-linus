// Gera tests/golden/collections_bun.tsv: Array, Object, Map/Set/WeakMap/WeakSet/WeakRef/FinalizationRegistry,
// métodos de Array em arrays esparsos e array-likes, sort estável com comparadores inconsistentes,
// Object.groupBy/Map.groupBy, métodos novos de Set, Array.fromAsync, iterator helpers, Iterator.from/concat,
// defineProperty/freeze/seal e descritores, medidos no bun 1.4.2. Só ECMAScript (sem structuredClone).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), como em gen-function-error-golden.js.
// Cada programa é uma expressão (síncrona, `E`) ou uma expressão que devolve promessa (`A`), embrulhada no mesmo
// prelúdio serializador dos dois lados. O que já está em builtins_bun.tsv (casos simples de um método) não se
// repete: aqui entram os cenários cruzados (esparsos, array-likes, comparadores ruins, ordem de efeitos).
// Qualquer resultado com caminho da máquina é descartado.
// Uso: timeout 600 bun scripts/gen-collections-golden.js > tests/golden/collections_bun.tsv
const { emitFactored, runPrepared } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Prelúdio: serializa o valor preservando tipo, buracos, -0, símbolos, descritores e ordem de chaves.
const PRELUDE = [
  "'use strict';",
  "const S=(v,d=0)=>{const t=typeof v;if(v===null)return'null';if(t==='undefined')return'undefined';",
  "if(t==='number')return Object.is(v,-0)?'-0':String(v);if(t==='string')return JSON.stringify(v);",
  "if(t==='boolean'||t==='bigint')return String(v)+(t==='bigint'?'n':'');if(t==='symbol')return String(v);",
  "if(t==='function')return'fn:'+v.name+'/'+v.length;if(d>4)return'...';",
  "if(Array.isArray(v)){let o=[],h=0;for(let i=0;i<v.length;i++){if(i in v){if(h){o.push('<'+h+' holes>');h=0}o.push(S(v[i],d+1))}else h++}",
  "if(h)o.push('<'+h+' holes>');const ex=Object.keys(v).filter(k=>!/^(0|[1-9]\\d*)$/.test(k)).map(k=>k+':'+S(v[k],d+1));return'['+o.concat(ex).join(',')+']'}",
  "if(v instanceof Map)return'Map{'+[...v].map(([a,b])=>S(a,d+1)+'=>'+S(b,d+1)).join(',')+'}';",
  "if(v instanceof Set)return'Set{'+[...v].map(a=>S(a,d+1)).join(',')+'}';",
  "if(v instanceof Error)return v.name+': '+v.message;",
  "const ks=Reflect.ownKeys(v).map(k=>(typeof k==='symbol'?String(k):k)+':'+(Object.getOwnPropertyDescriptor(v,k).get?'<accessor>':S(v[k],d+1)));",
  "return'{'+ks.join(',')+'}'};",
  "const F=e=>e.name+': '+e.message;",
  "const L=[];",
].join("");

const programs = [];
const E = expr => programs.push(PRELUDE + "try{globalThis.R=S(" + expr + ")}catch(e){globalThis.R='throws '+F(e)}");
const A = expr =>
  programs.push(
    PRELUDE +
      "try{(" + expr + ").then(v=>{globalThis.R=S(v)},e=>{globalThis.R='rejects '+F(e)})}catch(e){globalThis.R='throws '+F(e)}",
  );
// Corpo livre (declarações): o corpo grava em R via S.
const B = body => programs.push(PRELUDE + "try{" + body + "}catch(e){globalThis.R='throws '+F(e)}");

// ---- Métodos de Array em arrays esparsos (cada método contra vários formatos de esparso).
const sparse = ["[1,,3]", "[,,1]", "[1,,]", "[,]", "Array(3)", "[1,,,4,,6]", "[,'a',,'b']", "(()=>{const a=[1,2,3];a.length=5;return a})()"];
const sparseCalls = [
  "x.map(v=>v*2)", "x.filter(()=>true)", "x.forEach(v=>L.push(v))||L", "x.every(v=>v!==undefined)", "x.some(v=>v===undefined)",
  "x.indexOf(undefined)", "x.includes(undefined)", "x.lastIndexOf(undefined)", "x.find(v=>true)", "x.findIndex(v=>v===undefined)",
  "x.findLast(v=>true)", "x.findLastIndex(v=>v===undefined)", "x.reduce((a,v)=>a+1,0)", "x.reduceRight((a,v)=>a+1,0)",
  "x.join('-')", "x.toString()", "x.concat([9])", "x.slice(1)", "x.slice(-2)", "x.flat()", "x.flatMap(v=>[v])",
  "x.reverse()", "x.sort()", "x.sort((a,b)=>b-a)", "x.toSorted()", "x.toReversed()", "x.toSpliced(0,1)", "x.with(0,'w')",
  "x.fill(0)", "x.copyWithin(0,1)", "x.splice(1,1)", "x.keys().next()", "[...x.keys()]", "[...x.entries()]", "[...x.values()]",
  "[...x]", "Array.from(x)", "x.at(1)", "Object.keys(x)", "Object.entries(x)", "1 in x", "x.length", "JSON.stringify(x)",
  "x.unshift(0)", "x.pop()", "x.shift()", "x.push(1)", "Object.getOwnPropertyNames(x)", "x.entries().toArray()",
];
for (const s of sparse) for (const c of sparseCalls) E("(x=>" + c.replace(/^/, "") + ")(" + s + ")");

// ---- Array-likes: os mesmos métodos via Array.prototype.X.call.
const likes = ["{length:3,0:'a',2:'c'}", "{length:'2',0:1,1:2}", "'abc'", "{length:-1}", "{length:2**53}", "{length:3.9,0:1,1:2,2:3}", "{0:1,length:{valueOf(){return 2}}}", "new String('xyz')"];
const likeCalls = [
  "map.call(x,v=>v)", "filter.call(x,()=>true)", "indexOf.call(x,'a')", "join.call(x,'+')", "slice.call(x,1)", "reverse.call(x)",
  "includes.call(x,undefined)", "reduce.call(x,(a,v)=>a+String(v),'')", "every.call(x,v=>v!==undefined)", "concat.call(x,[1])",
  "at.call(x,-1)", "toSorted.call(x)", "findLast.call(x,v=>true)", "lastIndexOf.call(x,'c')", "flat.call(x)", "keys.call(x).next()",
];
for (const s of likes) for (const c of likeCalls) E("(x=>{const {" + c.split(".")[0] + "}=Array.prototype;return " + c.replace(/^(\w+)\./, "$1.") + "})(" + s + ")");

// ---- Sort estável e comparadores inconsistentes.
const sortCases = [
  "[3,1,2].sort(()=>0)", "[3,1,2].sort(()=>1)", "[3,1,2].sort(()=>-1)", "[3,1,2].sort(()=>NaN)", "[3,1,2].sort(()=>undefined)",
  "[3,1,2].sort(()=>'1')", "[3,1,2].sort(()=>true)", "[3,1,2].sort(()=>({valueOf(){return -1}}))", "[3,1,2].sort(()=>null)",
  "[3,1,2].sort(()=>Math.random()>2)", "[1,2,3,4,5].sort(()=>-1)", "[1,2,3,4,5].sort(()=>1)", "[1,2,3,4,5,6,7,8,9,10,11,12].sort(()=>-1)",
  "[1,2,3,4,5,6,7,8,9,10,11,12].sort(()=>1)", "[1,2,3,4,5,6,7,8,9,10,11,12].sort(()=>0)",
  "[1,2,3,4,5,6,7,8,9,10,11,12].sort((a,b)=>a%2-b%2)", "[1,2,3,4,5,6,7,8,9,10,11,12].sort((a,b)=>b%3-a%3)",
  "Array.from({length:30},(_,i)=>({k:i%3,i})).sort((a,b)=>a.k-b.k).map(o=>o.i)",
  "Array.from({length:100},(_,i)=>({k:i%4,i})).sort((a,b)=>a.k-b.k).map(o=>o.i).join()",
  "Array.from({length:100},(_,i)=>({k:i%4,i})).sort((a,b)=>b.k-a.k).map(o=>o.i).join()",
  "Array.from({length:50},(_,i)=>i).sort((a,b)=>(a%5)-(b%5)).join()", "Array.from({length:50},(_,i)=>i).sort(()=>0).join()",
  "[3,1,2].sort(function(){throw new RangeError('cmp')})", "[1].sort(()=>{throw new Error('never')})", "[1,2].sort(()=>{throw new Error('boom')})",
  "[3,undefined,1,,2].sort()", "[3,undefined,1,,2].sort((a,b)=>b-a)", "[3,undefined,1,,2].toSorted()", "[undefined,undefined].sort(()=>{throw 1})",
  "[3,1,2].sort(null)", "[3,1,2].sort(undefined)", "[3,1,2].sort(5)", "[3,1,2].sort({})", "[3,1,2].sort('x')", "[3,1,2].toSorted(null)", "[3,1,2].toSorted(5)",
  "[10,9,1,'a','B',undefined,null,true].sort()", "[-1,-2,0,-0,1].sort()", "[-1,-2,0,-0,1].sort((a,b)=>a-b).map(x=>Object.is(x,-0))",
  "['b','a','C','A'].sort()", "['b','a','C','A'].sort((a,b)=>a.localeCompare(b))", "[2n,1n,3].sort()", "[Symbol.iterator].length",
  "[1,2,3].sort((a,b)=>{L.push([a,b].join());return a-b})&&L.length>0",
  "(()=>{const a=[3,2,1];a.sort((x,y)=>{a.length=0;return x-y});return a})()",
  "(()=>{const a=[3,2,1];a.sort((x,y)=>{a.push(9);return x-y});return a})()",
  "(()=>{const a=[3,2,1];a.sort((x,y)=>{a[0]=100;return x-y});return a})()",
  "(()=>{const a=[3,2,1];Object.freeze(a);return a.sort()})()", "(()=>{const a=[1];Object.freeze(a);return a.sort()})()",
  "(()=>{const a=[];Object.freeze(a);return a.sort()})()", "[1,2,3].toSorted((a,b)=>b-a)",
  "Array.prototype.sort.call({length:3,0:'c',1:'a',2:'b'})", "Array.prototype.sort.call({length:3,0:'c',2:'b'})",
  "Array.prototype.toSorted.call({length:2,0:'b',1:'a'})", "Array.prototype.sort.call('abc')", "Array.prototype.sort.call(null)",
  "[5,1,4].sort((a,b)=>a<b)", "[5,1,4].sort((a,b)=>a>b)", "[5,1,4,2,3].sort((a,b)=>a>b)", "[5,1,4,2,3].sort((a,b)=>a<b?-1:1)",
  "[5,1,4,2,3].sort((a,b)=>a===b?0:Math.random()-2)", "[1,2,3,4].sort((a,b)=>b>a?1:0)",
  "Array.from({length:20},(_,i)=>i).sort((a,b)=>a<10?1:-1).join()", "Array.from({length:20},(_,i)=>i).sort((a,b)=>a>b?-1:1).join()",
  "Array.from({length:20},(_,i)=>i%2?i:-i).sort((a,b)=>Math.abs(a)-Math.abs(b)).join()",
  "Array.from({length:20},(_,i)=>i).sort((a,b)=>a-b>0?1:a-b<0?-1:NaN).join()",
];
for (const c of sortCases) E(c);
// Estabilidade com tamanhos variados.
for (const n of [2, 3, 7, 10, 11, 16, 17, 33, 64, 65, 129, 300]) {
  for (const m of [2, 3, 5]) E("Array.from({length:" + n + "},(_,i)=>({k:(i*7)%" + m + ",i})).sort((a,b)=>a.k-b.k).map(o=>o.i).join()");
  E("Array.from({length:" + n + "},(_,i)=>i).sort(()=>-1).join()");
  E("Array.from({length:" + n + "},(_,i)=>i).sort(()=>1).join()");
  E("Array.from({length:" + n + "},(_,i)=>(i*37)%" + n + ").sort((a,b)=>a-b).join()");
}

// ---- Object.groupBy / Map.groupBy.
const groupCases = [
  "Object.groupBy([1,2,3,4,5],x=>x%3)", "Object.groupBy([],x=>x)", "Object.groupBy('hello',c=>c)", "Object.groupBy(new Set([1,2,3]),x=>x>1)",
  "Object.groupBy(new Map([[1,2],[3,4]]),([k,v])=>k>1)", "Object.groupBy([1,2,3],(x,i)=>i)", "Object.groupBy([1,2,3],function(){return this===undefined})",
  "Object.groupBy([1,2],x=>Symbol.for('s'+x))", "Object.groupBy([1,2],x=>({toString(){return 'k'+x}}))", "Object.groupBy([1],()=>{throw new TypeError('kt')})",
  "Object.groupBy([1,2],x=>-0)", "Object.groupBy([1],x=>1n)", "Object.groupBy([1],x=>null)", "Object.groupBy([1],x=>undefined)", "Object.groupBy([1],x=>NaN)",
  "Object.groupBy({},x=>x)", "Object.groupBy(1,x=>x)", "Object.groupBy(undefined,x=>x)", "Object.groupBy([1],undefined)", "Object.groupBy([1],{})",
  "Object.groupBy([1,2,3],x=>'__proto__')", "Object.getPrototypeOf(Object.groupBy([1],x=>x))", "Object.groupBy([1,2,3],x=>'constructor').constructor",
  "Object.groupBy([1,2,3],x=>x%2?'a':'b').a", "Object.keys(Object.groupBy([3,1,2],x=>x))", "Object.keys(Object.groupBy([1,2,3,4],x=>x%2?'b':'a'))",
  "Object.keys(Object.groupBy([1,2],x=>x===1?'10':'2'))", "Object.keys(Object.groupBy(['a','b'],x=>x==='a'?'z':1))",
  "Object.groupBy((function*(){yield 1;yield 2})(),x=>x)", "Object.groupBy({[Symbol.iterator](){return{next(){return{done:true}}}}},x=>x)",
  "Object.groupBy({[Symbol.iterator]:5},x=>x)", "Object.groupBy({[Symbol.iterator](){return 5}},x=>x)",
  "Object.groupBy({[Symbol.iterator](){return{next(){return 5}}}},x=>x)",
  "(()=>{let c=0;try{Object.groupBy({[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){c++;return{}}}}},()=>{throw 1})}catch(e){}return c})()",
  "Map.groupBy([1,2,3,4,5],x=>x%3)", "Map.groupBy([],x=>x)", "Map.groupBy('hello',c=>c)", "Map.groupBy([0,-0,NaN,NaN],x=>x)", "Map.groupBy([1,2],x=>({}))",
  "Map.groupBy([1,2,3],(x,i)=>i)", "Map.groupBy([1,2,3],x=>x%2?null:undefined)", "Map.groupBy([1],()=>{throw new TypeError('mk')})", "Map.groupBy([1],5)",
  "Map.groupBy(null,x=>x)", "Map.groupBy(new Set([1,2]),x=>x%2)", "Map.groupBy([1,2,3],x=>x%2).get(1)", "Map.groupBy([1,2,3],x=>x%2).size",
  "Map.groupBy([-0,0],x=>x).keys().next().value", "Object.is(Map.groupBy([-0],x=>x).keys().next().value,0)", "Map.groupBy([1,2,3],x=>x%2)instanceof Map",
  "Object.groupBy.length", "Map.groupBy.length", "Object.groupBy.name", "Map.groupBy.name", "Object.getOwnPropertyDescriptor(Object,'groupBy').enumerable",
  "Object.getOwnPropertyDescriptor(Map,'groupBy').writable", "Object.groupBy([1,2,3],x=>x).hasOwnProperty('1')", "Object.getOwnPropertyDescriptor(Object.groupBy([1],x=>x),'1')",
  "Object.isFrozen(Object.groupBy([1],x=>x))", "Object.isExtensible(Object.groupBy([1],x=>x))",
];
for (const c of groupCases) E(c);

// ---- Map / Set / Weak*: ordem, igualdade SameValueZero, mutação durante iteração, subclasses.
const mapCases = [
  "new Map([[NaN,1]]).get(NaN)", "new Map([[0,1]]).get(-0)", "new Map([[-0,1]]).keys().next().value", "Object.is(new Map([[-0,1]]).keys().next().value,0)",
  "new Set([NaN,NaN,0,-0]).size", "Object.is([...new Set([-0])][0],0)", "new Map([[1,'a'],[1,'b']]).size", "new Map([[1,'a']]).set(1,'b').get(1)",
  "[...new Map([[3,1],[1,2],[2,3]]).keys()]", "[...new Set([3,1,3,2,1])]", "new Map().set(1,1).set(2,2).delete(1)", "new Map().delete(1)",
  "new Set().add(1).add(1).size", "new Set([1]).add(2)instanceof Set", "new Map([[1,2]]).forEach(function(v,k,m){L.push(v,k,m instanceof Map,this)},'t')||L.length",
  "(()=>{const s=new Set([1,2,3]);const o=[];for(const x of s){o.push(x);if(x<3)s.add(x+10);if(x===1)s.delete(2)}return o})()",
  "(()=>{const m=new Map([[1,1],[2,2],[3,3]]);const o=[];for(const [k] of m){o.push(k);if(k===1){m.delete(1);m.set(1,9)}if(o.length>10)break}return o})()",
  "(()=>{const s=new Set([1,2,3]);const it=s.values();it.next();s.clear();s.add(7);return [...it]})()",
  "(()=>{const s=new Set([1]);const o=[];s.forEach(x=>{o.push(x);if(x<5)s.add(x+1)});return o})()",
  "(()=>{const m=new Map([[1,1]]);const it=m.entries();m.delete(1);m.set(2,2);return [...it]})()",
  "(()=>{const s=new Set([1,2]);const it=s[Symbol.iterator]();it.next();it.next();it.next();s.add(3);return it.next()})()",
  "new Map(null).size", "new Map(undefined).size", "new Map([]).size", "new Map(5)", "new Map([1])", "new Map(['ab'])", "new Map([[1]]).get(1)", "new Map({})",
  "new Set(5)", "new Set('aab').size", "new Set(null).size", "new Set({length:1,0:1})", "Map()", "Set()", "WeakMap()", "WeakSet()", "WeakRef({})",
  "(()=>{let c=0;class M extends Map{set(k,v){c++;return super.set(k,v)}};new M([[1,1],[2,2]]);return c})()",
  "(()=>{let c=0;class S extends Set{add(v){c++;return super.add(v)}};new S([1,2,2]);return c})()",
  "(()=>{const o=Map.prototype.set;Map.prototype.set=5;try{return new Map([[1,1]])}catch(e){return F(e)}finally{Map.prototype.set=o}})()",
  "(()=>{const o=Set.prototype.add;Set.prototype.add=null;try{return new Set([1])}catch(e){return F(e)}finally{Set.prototype.add=o}})()",
  "Map.prototype.get.call({},1)", "Map.prototype.get.call(new Set,1)", "Set.prototype.add.call(new Map,1)", "Map.prototype.size", "Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call({})",
  "Object.getOwnPropertyDescriptor(Set.prototype,'size').get.call(new Set([1]))", "Map.prototype[Symbol.iterator]===Map.prototype.entries", "Set.prototype[Symbol.iterator]===Set.prototype.values",
  "Set.prototype.keys===Set.prototype.values", "Object.prototype.toString.call(new Map)", "Object.prototype.toString.call(new WeakMap)", "Object.prototype.toString.call(new WeakSet)",
  "Object.prototype.toString.call(new WeakRef({}))", "Object.prototype.toString.call(new FinalizationRegistry(()=>{}))", "Object.prototype.toString.call(new Map().entries())",
  "Object.getOwnPropertyNames(Map.prototype).sort()", "Object.getOwnPropertyNames(Set.prototype).sort()", "Object.getOwnPropertyNames(WeakMap.prototype).sort()",
  "Object.getOwnPropertyNames(WeakSet.prototype).sort()", "Object.getOwnPropertyNames(WeakRef.prototype).sort()", "Object.getOwnPropertyNames(FinalizationRegistry.prototype).sort()",
  "Reflect.ownKeys(Map).map(String).sort()", "Reflect.ownKeys(Set).map(String).sort()", "Reflect.ownKeys(WeakMap).map(String).sort()", "Reflect.ownKeys(WeakRef).map(String).sort()",
  "Reflect.ownKeys(FinalizationRegistry).map(String).sort()", "Reflect.ownKeys(Map.prototype).map(String)", "Reflect.ownKeys(Set.prototype).map(String)",
  "Map.length", "Set.length", "WeakMap.length", "WeakSet.length", "WeakRef.length", "FinalizationRegistry.length", "Map.name", "FinalizationRegistry.name",
  "Map.prototype.set.length", "Map.prototype.forEach.length", "Set.prototype.add.length", "WeakMap.prototype.set.length", "FinalizationRegistry.prototype.register.length",
  "FinalizationRegistry.prototype.unregister.length", "WeakRef.prototype.deref.length",
  // WeakMap/WeakSet
  "(()=>{const w=new WeakMap;const k={};w.set(k,1);return [w.get(k),w.has(k),w.delete(k),w.has(k)]})()", "new WeakMap().set(1,1)", "new WeakMap().set('a',1)",
  "new WeakMap().set(null,1)", "new WeakMap().set(Symbol('s'),1)instanceof WeakMap", "new WeakMap().set(Symbol.for('s'),1)", "new WeakMap().set(Symbol.iterator,1)instanceof WeakMap",
  "new WeakMap().get(1)", "new WeakMap().has(1)", "new WeakMap().delete(1)", "new WeakSet().add(1)", "new WeakSet().add(Symbol.for('x'))", "new WeakSet().add(Symbol('x'))instanceof WeakSet",
  "new WeakSet().has(1)", "new WeakSet().delete(1)", "new WeakMap([[{},1]])instanceof WeakMap", "new WeakMap([[1,1]])", "new WeakSet([{}])instanceof WeakSet", "new WeakSet([1])",
  "new WeakMap(5)", "new WeakMap(null)instanceof WeakMap", "WeakMap.prototype.get.call(new Map,{})", "WeakSet.prototype.add.call(new WeakMap,{})",
  "(()=>{const w=new WeakSet;const k={};w.add(k).add(k);return w.has(k)})()", "(()=>{const f=Object.freeze({});const w=new WeakMap;w.set(f,1);return w.get(f)})()",
  "typeof WeakMap.prototype.clear", "typeof WeakSet.prototype.forEach", "typeof WeakMap.prototype[Symbol.iterator]", "Object.keys(new WeakMap)",
  // WeakRef / FinalizationRegistry
  "new WeakRef({a:1}).deref().a", "new WeakRef(1)", "new WeakRef(null)", "new WeakRef(Symbol('s')).deref().toString()", "new WeakRef(Symbol.for('s'))", "new WeakRef()",
  "WeakRef.prototype.deref.call({})", "WeakRef.prototype.deref.call(new WeakMap)", "Object.getPrototypeOf(new WeakRef({}))===WeakRef.prototype", "new WeakRef({})[Symbol.toStringTag]",
  "(()=>{const o={};return new WeakRef(o).deref()===o})()", "(()=>{class X extends WeakRef{};return new X({}).deref()!==undefined})()", "FinalizationRegistry()", "new FinalizationRegistry()", "new FinalizationRegistry(5)",
  "new FinalizationRegistry(()=>{}).register(1)", "new FinalizationRegistry(()=>{}).register({},1)instanceof Object", "(()=>{const o={};return new FinalizationRegistry(()=>{}).register(o,o)})()",
  "new FinalizationRegistry(()=>{}).register({},1,1)", "new FinalizationRegistry(()=>{}).register({},1,{})", "new FinalizationRegistry(()=>{}).register({},1,Symbol('t'))",
  "new FinalizationRegistry(()=>{}).register({},1,Symbol.for('t'))", "new FinalizationRegistry(()=>{}).unregister(1)", "new FinalizationRegistry(()=>{}).unregister({})",
  "(()=>{const r=new FinalizationRegistry(()=>{});const t={};r.register({},1,t);return [r.unregister(t),r.unregister(t)]})()",
  "new FinalizationRegistry(()=>{}).unregister(Symbol('x'))", "FinalizationRegistry.prototype.register.call({},{},1)", "FinalizationRegistry.prototype.unregister.call(new WeakRef({}),{})",
  "typeof FinalizationRegistry.prototype.cleanupSome", "typeof FinalizationRegistry.prototype.cleanup", "new FinalizationRegistry(()=>{})[Symbol.toStringTag]", "typeof WeakRef.prototype.constructor",
];
for (const c of mapCases) E(c);

// ---- Métodos novos de Set: matriz de operações contra tipos de argumento.
const setOps = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
const setArgs = [
  "new Set([2,3,4])", "new Set", "new Set([1,2,3])", "new Set([1,2,3,4,5])", "new Map([[2,'a'],[9,'b']])", "[2,3]", "{size:2,has:x=>x===2||x===3,keys(){return [2,3][Symbol.iterator]()}}",
  "{size:Infinity,has:x=>x===2,keys(){return [][Symbol.iterator]()}}", "{size:-1,has(){},keys(){}}", "{size:NaN,has(){},keys(){}}", "{size:'2',has:x=>true,keys(){return [7,8][Symbol.iterator]()}}",
  "{size:2,has:5,keys(){}}", "{size:2,has(){},keys:5}", "{size:2,has(){return 1},keys(){return {next(){return {done:true}}}}}", "{size:2,has(){return 0},keys(){return {next:()=>({done:true})}}}",
  "{size:2,has(){return true},keys(){return 5}}", "{size:2,has(){return true},keys(){return {}}}", "5", "null", "'ab'", "{}", "{size:1n,has(){},keys(){}}",
  "{get size(){L.push('size');return 1},get has(){L.push('has');return x=>false},get keys(){L.push('keys');return()=>[][Symbol.iterator]()}}",
  "{size:1,has(x){L.push('h'+x);return x===1},keys(){L.push('k');return [1][Symbol.iterator]()}}",
  "{size:3,has:x=>x===0,keys(){return [-0,0][Symbol.iterator]()}}", "new Set([-0])", "new Set([NaN])",
];
for (const op of setOps) for (const a of setArgs) E("(()=>{const r=new Set([1,2,3])." + op + "(" + a + ");return [r,L]})()");
const setMore = [
  "new Set([1,2,3]).union(new Set([4])).constructor===Set", "(()=>{class S extends Set{};return new S([1]).union(new Set([2])).constructor===Set})()",
  "(()=>{class S extends Set{};return new S([1]).union(new Set([2])) instanceof S})()", "(()=>{const s=new Set([1,2,3]);const r=s.intersection(new Set([3,2]));return [...r]})()",
  "[...new Set([1,2,3]).intersection(new Set([3,2,1,0]))]", "[...new Set([3,2,1,0]).intersection(new Set([1,2,3]))]", "[...new Set([1,2,3]).symmetricDifference(new Set([4,3,0]))]",
  "[...new Set([1,2,3]).difference({size:1,has:x=>x==2,keys(){return[2][Symbol.iterator]()}})]", "[...new Set([1,2]).union({size:1,has(){return false},keys(){return [1,5,1][Symbol.iterator]()}})]",
  "Set.prototype.union.call({},new Set)", "Set.prototype.union.call(new Map,new Set)", "Set.prototype.union.call(new WeakSet,new Set)", "Set.prototype.isSubsetOf.call(null,new Set)",
  "Set.prototype.union.length", "Set.prototype.symmetricDifference.name", "Object.getOwnPropertyDescriptor(Set.prototype,'union').enumerable",
  "(()=>{const s=new Set([1,2,3]);return s.difference({size:5,has(x){s.delete(3);return false},keys(){return[][Symbol.iterator]()}})})()",
  "(()=>{const s=new Set([1,2,3]);return s.intersection({size:1,has(x){s.delete(2);return true},keys(){return[][Symbol.iterator]()}})})()",
  "(()=>{const s=new Set([1,2,3]);return s.isSubsetOf({size:5,has(x){s.add(9);return true},keys(){return[][Symbol.iterator]()}})})()",
  "(()=>{const s=new Set([1,2,3]);return s.isSupersetOf({size:1,has(){return true},keys(){return [1,2,3,4].values()}})})()",
  "(()=>{let r=0;const s=new Set([1,2,3]);s.isSupersetOf({size:2,has(){return true},keys(){return {next(){return {done:false,value:99}},return(){r++;return {}}}}});return r})()",
  "(()=>{let r=0;const s=new Set([1,2,3]);s.isDisjointFrom({size:1,has(){return true},keys(){return {next(){return {done:false,value:1}},return(){r++;return {}}}}});return r})()",
];
for (const c of setMore) E(c);

// ---- Array.fromAsync.
const fa = [
  "Array.fromAsync([1,2,3])", "Array.fromAsync([Promise.resolve(1),2])", "Array.fromAsync([Promise.reject(new Error('r')),2])", "Array.fromAsync((async function*(){yield 1;yield 2})())",
  "Array.fromAsync((function*(){yield 1;yield Promise.resolve(2)})())", "Array.fromAsync({length:2,0:'a',1:Promise.resolve('b')})", "Array.fromAsync({length:2,0:'a',1:'b'},x=>x+'!')",
  "Array.fromAsync([1,2],async x=>x*2)", "Array.fromAsync([1,2],x=>{throw new Error('m')})", "Array.fromAsync([1],5)", "Array.fromAsync(null)", "Array.fromAsync(undefined)", "Array.fromAsync(5)",
  "Array.fromAsync('ab')", "Array.fromAsync(new Set([1,2]))", "Array.fromAsync(new Map([[1,2]]))", "Array.fromAsync([],x=>x)", "Array.fromAsync({})", "Array.fromAsync({length:-1})",
  "Array.fromAsync([1,2],function(x,i){return [x,i,this]},'t')", "Array.fromAsync([3,4],(x,i)=>i)", "Array.fromAsync(function*(){yield 1}())", "Array.fromAsync({[Symbol.asyncIterator](){let i=0;return{next(){return Promise.resolve(i<2?{done:false,value:i++}:{done:true})}}}})",
  "Array.fromAsync({[Symbol.iterator]:5})", "Array.fromAsync({[Symbol.asyncIterator]:5})", "Array.fromAsync({[Symbol.asyncIterator]:null,length:1,0:'z'})",
  "Array.fromAsync({[Symbol.asyncIterator](){return 5}})", "Array.fromAsync({[Symbol.asyncIterator](){return {next(){return 5}}}})", "Array.fromAsync({[Symbol.asyncIterator](){return {next(){throw new Error('nx')}}}})",
  "Array.fromAsync([1,2,3]) instanceof Promise", "Array.fromAsync.length", "Array.fromAsync.name", "Array.fromAsync.call(Object,[1,2])", "Array.fromAsync.call(function(){this.x=1},[1])",
  "Array.fromAsync.call(undefined,[1])", "Array.fromAsync.call(5,[1])", "Array.fromAsync.call(class{constructor(){return {}}},[1,2])",
  "Array.fromAsync([1,2,3].values())", "Array.fromAsync([,1])", "Array.fromAsync([1,,2])", "Array.fromAsync({length:3,1:1})",
  "(async()=>{const o=[];await Array.fromAsync([1,2],x=>{o.push('m'+x);return x});o.push('after');return o})()",
  "(async()=>{const o=[];const p=Array.fromAsync([1,2]);o.push('sync');await p;return o})()",
  "(async()=>{let c=0;try{await Array.fromAsync({[Symbol.asyncIterator](){return{next(){return Promise.resolve({done:false,value:1})},return(){c++;return {}}}}},()=>{throw 1})}catch(e){}return c})()",
  "(async()=>{let c=0;try{await Array.fromAsync({length:1,0:Promise.reject(1)})}catch(e){c=e}return c})()",
  "Array.fromAsync([Promise.resolve(1),Promise.resolve(2)],async x=>x+1)", "Array.fromAsync([1,2],x=>Promise.reject(new Error('mr')))",
  "Array.fromAsync(new Proxy([1,2],{}))", "Array.fromAsync(new Uint8Array([1,2]))", "Array.fromAsync(Object.freeze([1]))", "Array.fromAsync({length:2**32})",
];
for (const c of fa) A(c);

// ---- Iterator helpers e Iterator.from/concat.
const it = ["[1,2,3,4,5].values()", "[].values()", "(function*(){yield 1;yield 2;yield 3})()", "new Set([1,2,3]).values()", "'abc'[Symbol.iterator]()", "new Map([[1,2]]).entries()"];
const itOps = [
  "map(x=>x*2).toArray()", "filter(x=>x%2).toArray()", "take(2).toArray()", "take(0).toArray()", "drop(2).toArray()", "drop(0).toArray()", "drop(100).toArray()", "take(100).toArray()",
  "flatMap(x=>[x,x]).toArray()", "flatMap(x=>'ab').toArray()", "flatMap(x=>5).toArray()", "flatMap(x=>[x].values()).toArray()", "flatMap(x=>({[Symbol.iterator]:null,next(){return{done:true}}})).toArray()",
  "reduce((a,x)=>a+x)", "reduce((a,x)=>a+x,10)", "reduce((a,x)=>[a,x])", "some(x=>x>2)", "some(x=>false)", "every(x=>x>0)", "every(x=>false)", "find(x=>x>1)", "find(x=>false)",
  "forEach(x=>L.push(x))", "toArray()", "map(5)", "filter(null)", "take(-1)", "take(NaN)", "take('a')", "take(Infinity).toArray()", "take(1.9).toArray()", "drop(-1)", "drop(NaN)", "drop(2.5).toArray()",
  "map((x,i)=>i).toArray()", "filter((x,i)=>i>0).toArray()", "flatMap((x,i)=>[i]).toArray()", "some((x,i)=>i>1)", "every((x,i)=>i<9)", "find((x,i)=>i===1)", "reduce((a,x,i)=>a+i,0)", "forEach((x,i)=>L.push(i))",
  "map(x=>x).next()", "map(x=>x)[Symbol.toStringTag]", "map(x=>x)[Symbol.iterator]()===undefined", "map(x=>x).return()", "map(x=>x).return().done", "map(function(){return this},1).next()",
  "map(x=>{throw new Error('mt')}).next()", "filter(x=>{throw new Error('ft')}).next()", "take(1).drop(1).toArray()", "drop(1).take(1).toArray()", "map(x=>x+1).filter(x=>x%2).map(String).toArray()",
  "Object.getPrototypeOf(map(x=>x))===Object.getPrototypeOf(filter(x=>x))", "Object.getPrototypeOf(Object.getPrototypeOf(map(x=>x)))===Iterator.prototype", "Reflect.ownKeys(Object.getPrototypeOf(map(x=>x))).map(String)",
  "(()=>{const h=map(x=>x);h.next();h.return();return h.next()})()", "(()=>{const h=map(x=>x);return [h.next().value,h.next().value]})()",
  "map(x=>x).toArray().length+take(1).toArray().length",
];
for (const s of it) for (const o of itOps) E("(" + s + ")." + o + "");
const itMore = [
  "Iterator.prototype.map.call({next(){return{done:true}}},x=>x).toArray()", "Iterator.prototype.map.call(5,x=>x)", "Iterator.prototype.map.call(null,x=>x)", "Iterator.prototype.map.call({},x=>x).next()",
  "Iterator.prototype.toArray.call({next(){return{done:true}}})", "Iterator.prototype.toArray.call({next:5})", "Iterator.prototype.toArray.call({})", "Iterator.prototype.toArray.call('abc')",
  "Iterator.prototype.reduce.call({next(){return{done:true}}},(a,b)=>a)", "Iterator.prototype.reduce.call({next(){return{done:true}}},(a,b)=>a,7)", "[].values().reduce((a,b)=>a)",
  "Iterator.prototype.some.call({next(){return{done:false,value:1}},return(){L.push('ret');return {}}},x=>true)&&L", "Iterator.prototype.every.call({next(){return{done:false,value:1}},return(){L.push('ret');return {}}},x=>false)&&L",
  "Iterator.prototype.find.call({next(){return{done:false,value:1}},return(){L.push('ret');return {}}},x=>true)&&L", "Iterator.prototype.take.call({next(){return{done:false,value:1}},return(){L.push('ret');return {}}},1).toArray()&&L",
  "Iterator.prototype.map.call({next(){return{done:false,value:1}},return(){L.push('ret');return {}}},5)", "(()=>{const it={next(){return{done:false,value:1}},return(){L.push('ret');return {}}};try{Iterator.prototype.take.call(it,-1)}catch(e){}return L})()",
  "(()=>{const it={next(){return{done:false,value:1}},return(){L.push('ret');return {}}};try{Iterator.prototype.map.call(it,5)}catch(e){}return L})()",
  "(()=>{const it={next(){return{done:false,value:1}},return(){L.push('ret');return {}}};const h=Iterator.prototype.map.call(it,x=>x);h.next();h.return();h.return();return L})()",
  "typeof Iterator", "Iterator()", "new Iterator()", "(()=>{class I extends Iterator{};return new I instanceof Iterator})()", "Iterator.prototype[Symbol.toStringTag]", "Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).get!==undefined",
  "Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').get!==undefined", "Iterator.prototype.constructor===Iterator", "Iterator.length", "Iterator.name", "Reflect.ownKeys(Iterator).map(String).sort()",
  "Reflect.ownKeys(Iterator.prototype).map(String).sort()", "[].values().__proto__.__proto__===Iterator.prototype", "Iterator.prototype.map.length", "Iterator.prototype.reduce.length", "Iterator.prototype.take.length",
  "Iterator.prototype.flatMap.name", "Iterator.prototype.toArray.length", "Iterator.from.length", "Iterator.from.name",
  "Iterator.from([1,2]).toArray()", "Iterator.from('ab').toArray()", "Iterator.from(5)", "Iterator.from(null)", "Iterator.from({})", "Iterator.from({next(){return{done:true}}}).toArray()",
  "Iterator.from({next(){return{done:true}}}) instanceof Iterator", "Iterator.from([1].values()) instanceof Iterator", "(()=>{const i=[1].values();return Iterator.from(i)===i})()",
  "(()=>{const i={next(){return{done:true}}};const w=Iterator.from(i);return [w===i,Object.getPrototypeOf(w)===Iterator.prototype]})()",
  "Iterator.from({[Symbol.iterator](){return{next(){return{done:true}}}}}).toArray()", "Iterator.from({[Symbol.iterator]:5})", "Iterator.from({[Symbol.iterator](){return 5}})", "Iterator.from(new Set([1]))",
  "(()=>{const w=Iterator.from({next(){return{done:false,value:1}},return(){L.push('r');return{value:9,done:true}}});return [w.return(),L]})()", "Iterator.from({next(){return{done:false,value:1}}}).return()",
  "Iterator.from({next(){return{done:false,value:1}}}).take(2).toArray()", "Object.getOwnPropertyNames(Iterator.from({next(){}}))", "Reflect.ownKeys(Object.getPrototypeOf(Iterator.from({next(){}}))).map(String)",
  "Iterator.from(new String('xy')).toArray()", "Iterator.from(Object('x')).toArray()",
  "typeof Iterator.concat", "typeof Iterator.zip", "typeof Iterator.zipKeyed", "Iterator.concat.length", "Iterator.concat.name",
  "Iterator.concat([1,2],[3]).toArray()", "Iterator.concat().toArray()", "Iterator.concat([1],'ab',new Set([5])).toArray()", "Iterator.concat(5)", "Iterator.concat(null)", "Iterator.concat([1].values())",
  "Iterator.concat({[Symbol.iterator]:5})", "Iterator.concat({})", "Iterator.concat({[Symbol.iterator](){return{next(){return{done:true}}}}}).toArray()",
  "Iterator.concat([1,2],[3]).take(2).toArray()", "Iterator.concat([1],[2])[Symbol.toStringTag]", "Iterator.concat([1],[2]) instanceof Iterator", "Object.getPrototypeOf(Iterator.concat([1]))===Object.getPrototypeOf([1].values().map(x=>x))",
  "Iterator.concat([1],[2]).map(x=>x*2).toArray()", "(()=>{const c=Iterator.concat([1]);c.next();c.next();return c.next()})()", "Iterator.concat([1]).return()", "Iterator.concat([1],[2]).drop(1).toArray()",
  "Iterator.concat.call(undefined,[1]).toArray()",
  "(()=>{const o=[];const mk=n=>({[Symbol.iterator](){o.push('open'+n);return[n].values()}});const c=Iterator.concat(mk(1),mk(2));o.push('made');c.toArray();return o})()",
];
for (const c of itMore) E(c);
// Iteradores em cadeia e protocolo.
for (const n of [0, 1, 2, 3, 5]) {
  E("[1,2,3,4,5,6].values().take(" + n + ").map(x=>x*x).toArray()");
  E("[1,2,3,4,5,6].values().drop(" + n + ").filter(x=>x%2).toArray()");
  E("[[1,2],[3],[],[4,5]].values().flatMap(x=>x).drop(" + n + ").toArray()");
  E("Iterator.concat([1,2],[3,4],[5]).drop(" + n + ").take(2).toArray()");
}
A("(async()=>{const o=[];const a=[1,2,3].values().map(x=>{o.push(x);return x});o.push('lazy');a.next();return o})()");
A("(async()=>{return (async function*(){yield 1}()).map===undefined})()");
A("(async()=>typeof AsyncIterator)()");
A("(async()=>Object.getPrototypeOf(Object.getPrototypeOf((async function*(){})())).hasOwnProperty('map'))()");

// ---- Array e Object: construtores, estáticos, atributos, descritores, freeze/seal.
const arrCases = [
  "Array(3).length", "Array(-1)", "Array(1.5)", "Array(2**32)", "Array(2**32-1).length", "Array('3').length", "Array(1,2).length", "new Array(0).length", "Array.of(7).length", "Array.of()", "Array.of(undefined)",
  "Array.from({length:3})", "Array.from({length:3},(_,i)=>i*i)", "Array.from('héy')", "Array.from(new Set([1,1,2]))", "Array.from(new Map([[1,2]]))", "Array.from([1,2,3],function(x){return x*this.k},{k:2})",
  "Array.from({length:2,0:1,1:2})", "Array.from(5)", "Array.from(null)", "Array.from([1],5)", "Array.from({length:-5})", "Array.from({length:2**32})", "Array.from.call(Object,[1,2])", "Array.from.call(function(n){return {n}},{length:2,0:'a',1:'b'})",
  "Array.from({[Symbol.iterator]:null,length:1,0:'x'})", "Array.from({[Symbol.iterator]:5})", "Array.from((function*(){yield 1;yield 2})())", "Array.of.call(Object,1,2)", "Array.of.call(undefined,1)", "Array.isArray(new Proxy([],{}))",
  "Array.isArray(Array.prototype)", "Array.isArray({length:0})", "Array.isArray(new Uint8Array)", "Array.isArray(Object.create([]))", "(()=>{const r=Proxy.revocable([],{});r.revoke();return Array.isArray(r.proxy)})()",
  "Array[Symbol.species]===Array", "(()=>{class A extends Array{};return [new A(1,2).map(x=>x) instanceof A,new A(1,2).filter(x=>x) instanceof A,new A(1,2).slice() instanceof A,new A(1,2).concat() instanceof A]})()",
  "(()=>{class A extends Array{static get [Symbol.species](){return Array}};return new A(1,2).map(x=>x) instanceof A})()", "(()=>{class A extends Array{static get [Symbol.species](){return null}};return new A(1,2).map(x=>x)})()",
  "(()=>{class A extends Array{static get [Symbol.species](){return 5}};return new A(1,2).map(x=>x)})()", "(()=>{class A extends Array{static get [Symbol.species](){return function(){return {length:0}}}};return new A(1,2).map(x=>x)})()",
  "(()=>{const a=[1,2];a.constructor={[Symbol.species]:function(n){return {length:0,n}}};return a.map(x=>x)})()", "(()=>{const a=[1,2];a.constructor=5;return a.map(x=>x)})()", "(()=>{const a=[1,2];a.constructor=undefined;return a.slice()})()",
  "(()=>{const a=[1,2,3];a.length=1;return a})()", "(()=>{const a=[1];a.length=3;return a})()", "(()=>{const a=[1];a[5]=2;return [a.length,a]})()", "(()=>{const a=[];a['2']=1;return a.length})()", "(()=>{const a=[];a['02']=1;return [a.length,Object.keys(a)]})()",
  "(()=>{const a=[];a[2**32-2]=1;return a.length})()", "(()=>{const a=[];a[2**32-1]=1;return [a.length,Object.keys(a)]})()", "(()=>{const a=[];a.length=2**32})()", "(()=>{const a=[];a.length=-1})()", "(()=>{const a=[];a.length=1.5})()", "(()=>{const a=[1,2];a.length='1';return a})()",
  "(()=>{const a=[1,2];Object.defineProperty(a,'length',{writable:false});a.push(1)})()", "(()=>{const a=[1,2];Object.defineProperty(a,'length',{writable:false});a[5]=1;return a.length})()", "(()=>{'use strict';const a=[1,2];Object.defineProperty(a,'length',{writable:false});a.length=0})()",
  "(()=>{const a=[1,2];Object.defineProperty(a,'length',{writable:false});return a.pop()})()", "(()=>{const a=[1,2];Object.defineProperty(a,1,{configurable:false});a.length=0;return [a.length,a]})()",
  "(()=>{const a=[1,2,3];Object.defineProperty(a,1,{configurable:false});try{a.length=0}catch(e){}return [a.length,a]})()", "(()=>{const a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return a.pop()})()", "(()=>{const a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return a.shift()})()",
  "(()=>{const a=[1,2,3];Object.defineProperty(a,2,{get(){return 'g'}});return [a.join(),a.indexOf('g'),a.slice(2)]})()", "(()=>{const a=[1,2,3];Object.defineProperty(a,1,{get(){return 'g'},enumerable:false});return [Object.keys(a),a.map(x=>x)]})()",
  "(()=>{const a=[1,2,3];Object.seal(a);return [a.push(4)]})()", "(()=>{const a=[1,2,3];Object.seal(a);return a.pop()})()", "(()=>{const a=[1,2,3];Object.seal(a);a[0]=9;return a})()", "(()=>{const a=[1,2,3];Object.seal(a);return a.reverse()})()", "(()=>{const a=[1,2,3];Object.freeze(a);return a.reverse()})()",
  "(()=>{const a=[1,2,3];Object.freeze(a);return a.fill(0)})()", "(()=>{const a=[1,2,3];Object.freeze(a);return a.splice(0,1)})()", "(()=>{const a=[1,2,3];Object.freeze(a);return a.unshift()})()", "(()=>{const a=[1,2,3];Object.freeze(a);return a.push()})()",
  "(()=>{const a=[1,2,3];Object.freeze(a);return a.copyWithin(0,1)})()", "(()=>{const a=[1,2,3];Object.freeze(a);return [a.slice(1),a.map(x=>x),a.toSorted(),a.with(0,5)]})()", "(()=>{const a=[1,2,3];Object.preventExtensions(a);return a.push(4)})()", "(()=>{const a=[1,2,3];Object.preventExtensions(a);a.pop();return a})()",
  "(()=>{const a=[1,2,3];Object.preventExtensions(a);return a.concat([4])})()", "(()=>{const a=[1,2,3];Object.preventExtensions(a);a.length=5;return [a.length,a]})()", "(()=>{const a=[];Object.preventExtensions(a);a[0]=1;return a})()",
  "[1,2,3].with(1,9)", "[1,2,3].with(-1,9)", "[1,2,3].with(3,9)", "[1,2,3].with(-4,9)", "[1,2,3].with(1.5,9)", "[1,2,3].with('1',9)", "[1,2,3].with(NaN,9)", "[1,2,3].with()", "[1,,3].with(0,9)", "[1,2,3].toSpliced(1)", "[1,2,3].toSpliced()", "[1,2,3].toSpliced(1,1,'a','b')", "[1,2,3].toSpliced(-1,5)",
  "[1,2,3].toSpliced(1,undefined)", "[1,2,3].toSpliced(undefined)", "[1,2,3].splice()", "[1,2,3].splice(1)", "[1,2,3].splice(undefined)", "[1,2,3].splice(1,undefined)", "[1,2,3].splice(-1,1,'a','b')", "[1,2,3].toReversed()", "[1,,3].toReversed()", "[1,,3].reverse()", "[,1].reverse()",
  "[1,2,3,4,5].copyWithin(0,3)", "[1,2,3,4,5].copyWithin(1,0,3)", "[1,2,3,4,5].copyWithin(-2,-4,-3)", "[1,2,3,4,5].copyWithin(2,0)", "[1,,3,4,5].copyWithin(0,1)", "[1,2,3].fill(0,1)", "[1,2,3].fill(0,-1)", "[1,2,3].fill(0,1,2)", "[1,2,3].fill()", "[1,2,3].fill(0,NaN,NaN)",
  "[1,2,3].at(-1)", "[1,2,3].at(5)", "[1,2,3].at(NaN)", "[1,2,3].at(1.9)", "[1,2,3].at('x')", "[1,2,3].at(-0)", "[1,[2,[3,[4]]]].flat()", "[1,[2,[3,[4]]]].flat(Infinity)", "[1,[2,[3,[4]]]].flat(0)", "[1,[2,[3,[4]]]].flat(-1)", "[1,[2,[3,[4]]]].flat('2')", "[[1],,[2]].flat()", "[1,2].flatMap(x=>[[x]])", "[1,2].flatMap(5)",
  "[1,2].flatMap(x=>({length:1,0:x}))", "[1,2,3].includes(NaN)", "[NaN].includes(NaN)", "[NaN].indexOf(NaN)", "[-0].includes(0)", "[1,2,3].includes(1,-1)", "[1,2,3].includes(3,-1)", "[1,2,3].indexOf(1,-5)", "[1,2,3].lastIndexOf(1,-5)", "[1,2,1].lastIndexOf(1,-2)", "[1,2,1].lastIndexOf(1)", "[1,2,1].lastIndexOf(1,undefined)",
  "[1,2,3].join(undefined)", "[1,2,3].join(null)", "[null,undefined,1].join()", "[[1,2],[3]].join(';')", "(()=>{const a=[1];a.push(a);return a.join()})()", "(()=>{const a=[1];a.push(a);return String(a)})()", "[1,2,3].toString.call({join(){return 'j'}})", "[1,2,3].toString.call({})", "[1,2].toLocaleString()", "[1,[2,3]].toLocaleString()",
  "[3,2,1].reduce()", "[].reduce((a,b)=>a)", "[].reduceRight((a,b)=>a)", "[,].reduce((a,b)=>a)", "[1].reduce((a,b)=>a)", "[1,2,3].reduceRight((a,b)=>a+'-'+b)", "[1,2,3].reduce((a,b,i,arr)=>a+i+arr.length,0)",
  "[1,2,3].findLast(x=>x<3)", "[1,2,3].findLastIndex(x=>x>5)", "[1,2,3].find(function(){return this===5},5)", "[1,2,3].every(()=>{throw new RangeError('ev')})", "[1,2,3].find(5)", "[1,2,3].map()", "[1,2,3].filter(null)", "[1,2,3].forEach({})",
  "(()=>{const a=[1,2,3];return a.map((x,i)=>{if(i===0)a.push(9);return x})})()", "(()=>{const a=[1,2,3];return a.map((x,i)=>{if(i===0)a.pop();return x})})()", "(()=>{const a=[1,2,3];return a.map((x,i)=>{if(i===0)delete a[1];return x})})()", "(()=>{const a=[1,2,3];const o=[];a.forEach((x,i)=>{o.push(x);if(i===0)a.length=1});return o})()",
  "(()=>{const a=[1,2,3];const o=[];a.forEach((x,i)=>{o.push(x);if(i===0)a.unshift(0)});return o})()", "(()=>{const a=[1,2,3];const o=[];for(const x of a){o.push(x);if(x===1)a.push(4)}return o})()", "(()=>{const a=[1,2,3];const it=a.values();it.next();a.length=0;return it.next()})()",
  "(()=>{const a=[1,2,3];const it=a.keys();a.length=0;return it.next()})()", "(()=>{const a=[1,2,3];const it=a.values();while(!it.next().done);a.push(4);return it.next()})()", "[].entries().toString()", "Object.prototype.toString.call([].values())", "[][Symbol.iterator]===[].values", "Array.prototype[Symbol.unscopables]",
  "Object.getPrototypeOf(Array.prototype[Symbol.unscopables])", "Object.keys(Array.prototype[Symbol.unscopables])", "Object.getOwnPropertyDescriptor(Array.prototype,Symbol.unscopables)", "Array.prototype.length", "Array.prototype.concat.length", "Array.prototype.push.length", "Array.prototype.splice.length", "Array.prototype.indexOf.length", "Array.prototype.reduce.length",
  "Array.prototype.slice.length", "Array.prototype.with.length", "Array.prototype.toSpliced.length", "Array.prototype.at.length", "Array.prototype.flat.length", "Array.prototype.fill.length", "Array.prototype.includes.length", "Array.prototype.copyWithin.length", "Array.prototype.sort.length",
  "Reflect.ownKeys(Array.prototype).map(String)", "Reflect.ownKeys(Array).map(String)", "Object.getOwnPropertyNames([])", "Object.getOwnPropertyDescriptor([],'length')", "Object.getOwnPropertyDescriptor([1],0)", "Object.getOwnPropertyDescriptor(Array.prototype,'map')",
  "[1,2,3].concat(4,[5,[6]])", "[1].concat({length:1,0:'x'})", "[1].concat({length:1,0:'x',[Symbol.isConcatSpreadable]:true})", "[1].concat(Object.assign([2,3],{[Symbol.isConcatSpreadable]:false}))", "[1].concat('ab')", "[,1].concat([,2])", "[1].concat(new Proxy([2,3],{}))", "[].concat.call(1,2)", "[].concat.call('a','b')",
  "(()=>{const a=[1];a[Symbol.isConcatSpreadable]=false;return [].concat(a)})()", "[1,2,3].slice(1,-1)", "[1,2,3].slice(-100,100)", "[1,2,3].slice(2,1)", "[1,2,3].slice(undefined,undefined)", "[1,2,3].slice('1')", "[1,2,3].slice(NaN)", "[1,2,3].slice(Infinity)", "[1,2,3].slice(-Infinity)",
  "[1,2,3].push(4,5)", "[1,2,3].unshift(-1,0)", "[].pop()", "[].shift()", "[1,2,3].shift()", "(()=>{const a=[1,2,3];a.length=0;return a.pop()})()", "Array.prototype.push.call({length:2**53-1},1)", "Array.prototype.push.call({length:2**53-2},1)", "Array.prototype.unshift.call({length:2**53-1},1)", "Array.prototype.splice.call({length:2**53-1},0,0,1)",
  "Array.prototype.push.call({},1,2)", "(()=>{const o={};Array.prototype.push.call(o,1,2);return o})()", "(()=>{const o={length:2,0:'a',1:'b'};Array.prototype.pop.call(o);return o})()", "(()=>{const o={length:2,0:'a',1:'b'};Array.prototype.shift.call(o);return o})()", "(()=>{const o={length:2,0:'a',1:'b'};Array.prototype.unshift.call(o,'z');return o})()",
  "(()=>{const o={length:3,0:'a',1:'b',2:'c'};Array.prototype.reverse.call(o);return o})()", "(()=>{const o={length:3,0:'a',2:'c'};Array.prototype.reverse.call(o);return o})()", "(()=>{const o={length:3,0:'a',1:'b',2:'c'};Array.prototype.splice.call(o,1,1);return o})()", "(()=>{const o={length:3,0:'a',1:'b',2:'c'};Array.prototype.fill.call(o,0,1);return o})()",
  "(()=>{const o={length:3,0:'a',1:'b',2:'c'};Array.prototype.copyWithin.call(o,0,1);return o})()", "(()=>{const o={length:3,0:'a',2:'c'};Array.prototype.copyWithin.call(o,0,1);return o})()", "(()=>{const o={length:3,0:'a',2:'c'};Array.prototype.sort.call(o);return o})()",
  "Array.prototype.map.call('abc',x=>x+x)", "Array.prototype.filter.call('abc',x=>x>'a')", "Array.prototype.join.call({length:3},'-')", "Array.prototype.indexOf.call({length:3,2:'x'},'x')", "Array.prototype.lastIndexOf.call({length:3,2:'x',5:'y'},'y')", "Array.prototype.every.call(5,x=>true)", "Array.prototype.some.call(null,x=>true)", "Array.prototype.map.call(undefined,x=>x)",
  "Array.prototype.concat.call(null)", "Array.prototype.toString.call(null)", "Array.prototype.at.call('abc',-1)", "Array.prototype.includes.call('abc','b')", "Array.prototype.flat.call({length:2,0:[1],1:[2]})", "Array.prototype.with.call({length:2,0:'a',1:'b'},0,'z')", "Array.prototype.toSpliced.call({length:2,0:'a',1:'b'},0,1)", "Array.prototype.toReversed.call({length:2,0:'a',1:'b'})",
  "Array.prototype.with.call({length:2**32},0,1)", "Array.prototype.toReversed.call({length:2**32})", "Array.prototype.toSorted.call({length:2**32})", "Array.prototype.toSpliced.call({length:2**32},0,0)", "[].with.call({length:2**32-1},0,1)",
  "Array.prototype.entries.call({length:2,0:'a'}).toArray()", "Array.prototype.keys.call('ab').toArray()", "Array.prototype.values.call({length:1,0:'q'}).toArray()", "Array.prototype.values.call(null)",
];
for (const c of arrCases) E(c);

const objCases = [
  "Object.defineProperty({}, 'a', {value:1})", "Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'a', {value:1}),'a')", "Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'a', {get(){}}),'a')", "Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'a', {set(v){}}),'a')",
  "Object.defineProperty({}, 'a', {get(){},value:1})", "Object.defineProperty({}, 'a', {set(){},writable:true})", "Object.defineProperty({}, 'a', {get:5})", "Object.defineProperty({}, 'a', {set:5})", "Object.defineProperty({}, 'a', {get:undefined})", "Object.defineProperty({}, 'a', 5)", "Object.defineProperty({}, 'a')", "Object.defineProperty(1, 'a', {})",
  "Object.defineProperty({}, 'a', {get:undefined,set:undefined})", "Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'a', {}),'a')", "Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'a', {get:undefined}),'a')",
  "(()=>{const o=Object.defineProperty({}, 'a', {value:1});return Object.defineProperty(o,'a',{value:1})===o})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:1});return Object.defineProperty(o,'a',{value:2})})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:NaN});return Object.defineProperty(o,'a',{value:NaN})===o})()",
  "(()=>{const o=Object.defineProperty({}, 'a', {value:0});return Object.defineProperty(o,'a',{value:-0})})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:1,writable:true});return Object.defineProperty(o,'a',{value:2}).a})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:1,writable:true});Object.defineProperty(o,'a',{writable:false});return Object.defineProperty(o,'a',{writable:true})})()",
  "(()=>{const o=Object.defineProperty({}, 'a', {value:1,writable:true});Object.defineProperty(o,'a',{writable:false});o.a=5;return o.a})()", "(()=>{'use strict';const o=Object.defineProperty({}, 'a', {value:1});o.a=5})()", "(()=>{'use strict';const o=Object.defineProperty({}, 'a', {get(){return 1}});o.a=5})()", "(()=>{'use strict';const o=Object.freeze({a:1});delete o.a})()", "(()=>{'use strict';const o=Object.freeze({});o.x=1})()", "(()=>{'use strict';const o=Object.preventExtensions({});o.x=1})()", "(()=>{'use strict';const o=Object.seal({a:1});o.a=2;return o.a})()",
  "(()=>{'use strict';const o=Object.seal({a:1});delete o.a})()", "(()=>{'use strict';const o=Object.seal({});o.x=1})()", "(()=>{'use strict';Object.defineProperty(Object.freeze({}),'x',{value:1})})()", "(()=>{'use strict';let s='abc';s.length=1})()", "(()=>{'use strict';'abc'[0]='x'})()", "(()=>{'use strict';(5).x=1})()", "(()=>{'use strict';undefined.x=1})()", "(()=>{'use strict';null.x})()",
  "(()=>{const o=Object.defineProperty({}, 'a', {value:1,configurable:true});Object.defineProperty(o,'a',{get(){return 2}});return Object.getOwnPropertyDescriptor(o,'a')})()", "(()=>{const o=Object.defineProperty({}, 'a', {get(){return 2},configurable:true});Object.defineProperty(o,'a',{value:3});return Object.getOwnPropertyDescriptor(o,'a')})()",
  "(()=>{const o=Object.defineProperty({}, 'a', {get(){return 2},configurable:false});Object.defineProperty(o,'a',{value:3})})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:1,enumerable:true});Object.defineProperty(o,'a',{enumerable:false})})()", "(()=>{const o=Object.defineProperty({}, 'a', {value:1,configurable:true});Object.defineProperty(o,'a',{enumerable:true});return Object.getOwnPropertyDescriptor(o,'a')})()",
  "Object.getOwnPropertyDescriptors({a:1,get b(){return 1},[Symbol.iterator]:2})", "Object.getOwnPropertyDescriptors([1])", "Object.getOwnPropertyDescriptors('ab')", "Object.getOwnPropertyDescriptors(null)", "Object.getOwnPropertyDescriptors(1)", "Object.getOwnPropertyDescriptor('abc',1)", "Object.getOwnPropertyDescriptor('abc','length')", "Object.getOwnPropertyDescriptor(null,'a')",
  "Object.getOwnPropertyDescriptor(1,'a')", "Object.getOwnPropertyDescriptor(function f(a){},'length')", "Object.getOwnPropertyDescriptor(function f(a){},'name')", "Object.getOwnPropertyDescriptor(function f(a){},'prototype')", "Object.getOwnPropertyDescriptor(()=>1,'prototype')", "Object.getOwnPropertyDescriptor(class{},'prototype')", "Object.getOwnPropertyDescriptor(Math,'PI')", "Object.getOwnPropertyDescriptor(globalThis,'undefined')", "Object.getOwnPropertyDescriptor(globalThis,'Array')",
  "Object.defineProperties({}, {a:{value:1,enumerable:true},b:{get(){return 2},enumerable:true}})", "Object.defineProperties({}, {a:1})", "Object.defineProperties({}, null)", "Object.defineProperties({}, 'ab')", "Object.defineProperties({}, {a:{value:1},b:5})", "(()=>{const o={};try{Object.defineProperties(o,{a:{value:1},b:5})}catch(e){}return Object.getOwnPropertyNames(o)})()",
  "Object.create(null,{a:{value:1}})", "Object.create({}, null)", "Object.create(5)", "Object.create(undefined)", "Object.create(null)", "Object.getPrototypeOf(Object.create(null))", "Object.create(Array.prototype) instanceof Array", "Object.create({x:1},{y:{value:2,enumerable:true}})",
  "Object.freeze(1)", "Object.freeze('a')", "Object.freeze(null)", "Object.freeze(undefined)", "Object.isFrozen(1)", "Object.isFrozen('a')", "Object.isFrozen({})", "Object.isFrozen(Object.preventExtensions({}))", "Object.isFrozen(Object.preventExtensions({a:1}))", "Object.isFrozen(Object.freeze({a:1}))", "Object.isFrozen(Object.seal({a:1}))", "Object.isFrozen(Object.seal({}))", "Object.isFrozen(Object.freeze([1,2]))", "Object.isFrozen(Object.freeze(new Map))",
  "Object.isSealed(Object.freeze({a:1}))", "Object.isSealed(Object.seal({a:1}))", "Object.isSealed(Object.preventExtensions({a:1}))", "Object.isSealed(1)", "Object.isExtensible(1)", "Object.isExtensible(Object.preventExtensions({}))", "Object.isExtensible(Object.seal({}))", "Object.seal(1)", "Object.preventExtensions(1)", "Object.preventExtensions(null)",
  "Object.getOwnPropertyDescriptor(Object.freeze({a:1}),'a')", "Object.getOwnPropertyDescriptor(Object.seal({a:1}),'a')", "Object.getOwnPropertyDescriptor(Object.freeze([1]),'length')", "Object.getOwnPropertyDescriptor(Object.seal([1]),'length')", "Object.getOwnPropertyDescriptor(Object.freeze({get a(){return 1}}),'a').configurable", "Object.getOwnPropertyDescriptor(Object.freeze({get a(){return 1}}),'a').set",
  "(()=>{const o=Object.freeze({a:{b:1}});o.a.b=2;return o})()", "(()=>{const o=Object.freeze(new Map([[1,2]]));o.set(3,4);return o.size})()", "(()=>{const o=Object.freeze(new Set);o.add(1);return o.size})()", "(()=>{const o=Object.freeze(new Uint8Array(0));return o})()", "(()=>{const o=Object.freeze(new Uint8Array(1));return o})()", "(()=>{const o=Object.seal(new Uint8Array(1));return Object.isSealed(o)})()", "(()=>{const o=Object.preventExtensions(new Uint8Array(1));return Object.isFrozen(o)})()",
  "(()=>{const p=new Proxy({},{preventExtensions(){return false}});return Object.preventExtensions(p)})()", "(()=>{const p=new Proxy({},{defineProperty(){return false}});return Object.defineProperty(p,'a',{})})()", "(()=>{const p=new Proxy({a:1},{});return Object.freeze(p).a})()", "(()=>{const p=new Proxy({},{ownKeys(){return ['a','a']}});return Object.keys(p)})()",
  "Object.keys('ab')", "Object.keys(5)", "Object.keys(null)", "Object.keys([1,,3])", "Object.keys({b:1,a:2,1:3,0:4,[Symbol.iterator]:5})", "Object.keys({'-1':1,'01':2,'4294967295':3,'4294967294':4,'1.5':5})", "Object.getOwnPropertyNames({b:1,a:2,1:3,0:4,'-1':5})", "Object.getOwnPropertySymbols({[Symbol.iterator]:1,[Symbol('x')]:2,a:3})", "Object.values({b:1,a:2,1:3})", "Object.values('ab')", "Object.entries({a:1,b:[2]})", "Object.entries('ab')", "Object.entries([5,6])",
  "Object.fromEntries([['a',1],['b',2]])", "Object.fromEntries(new Map([['a',1]]))", "Object.fromEntries([[1,2]])", "Object.fromEntries([['a']])", "Object.fromEntries([1])", "Object.fromEntries(['ab'])", "Object.fromEntries()", "Object.fromEntries(null)", "Object.fromEntries(5)", "Object.fromEntries([['__proto__',1]])", "Object.getPrototypeOf(Object.fromEntries([['__proto__',null]]))===Object.prototype", "Object.fromEntries([[Symbol.iterator,1]])", "Object.fromEntries({[Symbol.iterator]:function*(){yield['x',1]}})",
  "Object.assign({a:1},{b:2},null,undefined,'xy',5)", "Object.assign(null)", "Object.assign(1,{a:1})", "Object.assign({}, {get a(){return 1}})", "Object.getOwnPropertyDescriptor(Object.assign({}, {get a(){return 1}}),'a')", "(()=>{'use strict';return Object.assign(Object.freeze({a:1}),{a:2})})()", "(()=>{const t={};Object.assign(t,{[Symbol.iterator]:1,a:2});return Reflect.ownKeys(t).map(String)})()", "Object.assign({}, Object.defineProperty({}, 'h', {value:1,enumerable:false}))", "Object.assign([1,2],[3])", "Object.assign({b:0},{a:1,b:2,1:0})",
  "Object.is(NaN,NaN)", "Object.is(0,-0)", "Object.is()", "Object.is(undefined)", "Object.entries({get a(){L.push('g');return 1}})&&L", "Object.hasOwn({a:1},'a')", "Object.hasOwn('abc',1)", "Object.hasOwn(null,'a')", "Object.hasOwn([],'length')", "Object.hasOwn({},{toString(){return 'a'}})", "Object.prototype.hasOwnProperty.call(null,'a')", "Object.prototype.hasOwnProperty.call(undefined,{toString(){throw new Error('ts')}})",
  "Object.getPrototypeOf(1)===Number.prototype", "Object.getPrototypeOf(null)", "Object.setPrototypeOf(1,null)", "Object.setPrototypeOf(null,{})", "Object.setPrototypeOf({},1)", "Object.setPrototypeOf({})", "(()=>{const a={};const b=Object.create(a);return Object.setPrototypeOf(a,b)})()", "Object.setPrototypeOf(Object.preventExtensions({}),{})", "Object.setPrototypeOf(Object.preventExtensions({}),Object.prototype)instanceof Object", "Object.setPrototypeOf(Object.prototype,{})", "Object.setPrototypeOf(Object.prototype,null)instanceof Object",
  "({}).__proto__===Object.prototype", "(()=>{const o={};o.__proto__=5;return Object.getPrototypeOf(o)===Object.prototype})()", "(()=>{const o={};o.__proto__=null;return Object.getPrototypeOf(o)})()", "({__proto__:null}) instanceof Object", "({'__proto__':[]}) instanceof Array", "({['__proto__']:[]}) instanceof Array", "(()=>{const __proto__=[];return {__proto__} instanceof Array})()", "Object.getOwnPropertyNames({__proto__:1,a:2})",
  "Object.prototype.__lookupGetter__.call({get a(){return 1}},'a').name", "({}).__defineGetter__('a',()=>1)", "(()=>{const o={};o.__defineGetter__('a',()=>7);return [o.a,Object.keys(o)]})()", "(()=>{const o={};o.__defineSetter__('a',function(v){this.b=v});o.a=3;return o.b})()", "({}).__lookupSetter__('a')", "({}).__defineGetter__('a',5)", "Object.prototype.__defineGetter__.call(null,'a',()=>1)",
  "Object.prototype.toString.call(null)", "Object.prototype.toString.call(undefined)", "Object.prototype.toString.call([])", "Object.prototype.toString.call(()=>1)", "Object.prototype.toString.call(new Date(0))", "Object.prototype.toString.call(/x/)", "Object.prototype.toString.call(new Error)", "Object.prototype.toString.call(1n)", "Object.prototype.toString.call(Symbol())", "Object.prototype.toString.call({[Symbol.toStringTag]:'Z'})", "Object.prototype.toString.call({[Symbol.toStringTag]:5})", "Object.prototype.toString.call(Object.assign([],{[Symbol.toStringTag]:'Q'}))", "Object.prototype.toString.call(new Proxy([],{}))", "Object.prototype.toString.call(new Proxy(function(){},{}))", "Object.prototype.toString.call(JSON)", "Object.prototype.toString.call(Math)", "Object.prototype.toString.call(globalThis)", "Object.prototype.toString.call(Reflect)", "Object.prototype.toString.call(Promise.resolve())", "Object.prototype.toString.call(function*(){})", "Object.prototype.toString.call(async()=>{})", "Object.prototype.toString.call(new ArrayBuffer(1))", "Object.prototype.toString.call(new Uint8Array)", "Object.prototype.toString.call(new Boolean(1))", "Object.prototype.toString.call('s')", "Object.prototype.toString.call(Object(1n))",
  "Object.prototype.valueOf.call(1) instanceof Number", "Object.prototype.valueOf.call(null)", "Object.prototype.toLocaleString.call(1)", "Object.prototype.toLocaleString.call(null)", "Object.prototype.isPrototypeOf.call(null,{})", "Object.prototype.isPrototypeOf.call(Object.prototype,1)", "Object.prototype.isPrototypeOf.call(null,1)", "Object.prototype.propertyIsEnumerable.call('ab',0)", "Object.prototype.propertyIsEnumerable.call([1],'length')", "Object.prototype.propertyIsEnumerable.call(null,'a')",
  "Object.prototype.constructor===Object", "Object(1) instanceof Number", "Object('a').length", "Object(null)", "Object(undefined)", "Object(Symbol.iterator) instanceof Symbol", "Object(1n) instanceof BigInt", "new Object(1) instanceof Number", "(()=>{const o={};return Object(o)===o})()", "(()=>{class A extends Object{constructor(){super(5)}};return new A() instanceof Number})()", "(()=>{class B extends Object{};return new B(5) instanceof Number})()", "Reflect.construct(Object,[1],class{}) instanceof Number",
  "Object.length", "Object.name", "Reflect.ownKeys(Object).map(String).sort()", "Reflect.ownKeys(Object.prototype).map(String).sort()", "Object.defineProperty.length", "Object.assign.length", "Object.create.length", "Object.fromEntries.length", "Object.groupBy.length", "Object.getOwnPropertyDescriptors.length", "Object.entries.length", "Object.setPrototypeOf.length", "Object.is.length", "Object.hasOwn.length", "Object.prototype.__defineGetter__.length", "Object.prototype.hasOwnProperty.length",
  "(()=>{const o={};Object.defineProperty(o,'x',{value:1,enumerable:true,configurable:true,writable:true});return [Object.keys(o),JSON.stringify(o),{...o}]})()", "(()=>{const o={};Object.defineProperty(o,'x',{value:1});return [Object.keys(o),JSON.stringify(o),{...o},Object.getOwnPropertyNames(o)]})()", "(()=>{const o={a:1};Object.defineProperty(o,'a',{enumerable:false});return [Object.keys(o),{...o}]})()",
  "(()=>{const o={};Object.defineProperty(o,'a',{get(){return this===o}});return o.a})()", "(()=>{const o={};Object.defineProperty(o,'a',{get(){return 1},configurable:true});delete o.a;return Object.getOwnPropertyNames(o)})()", "(()=>{const o=[];Object.defineProperty(o,'0',{value:1});return [o.length,o]})()", "(()=>{const o=[];Object.defineProperty(o,'length',{value:3});return [o.length,o]})()", "(()=>{const o=[];Object.defineProperty(o,'length',{value:-1})})()", "(()=>{const o=[];Object.defineProperty(o,'length',{get(){return 1}})})()", "(()=>{const o=[];Object.defineProperty(o,'length',{enumerable:true})})()", "(()=>{const o=[1,2,3];Object.defineProperty(o,'length',{value:1});return o})()", "(()=>{const o=[1,2,3];Object.defineProperty(o,'length',{value:{valueOf(){L.push('v');return 2}}});return [o,L]})()",
  "(()=>{const o=[1,2,3];Object.defineProperty(o,'1',{value:'x',writable:false});o[1]='y';return o})()", "(()=>{const o=[1,2,3];Object.defineProperty(o,'5',{value:'x',writable:true,enumerable:true,configurable:true});return [o.length,o]})()", "(()=>{const o=[1,2,3];Object.defineProperty(o,'length',{writable:false});Object.defineProperty(o,'5',{value:1})})()", "(()=>{const o=[1,2,3];Object.defineProperty(o,'length',{writable:false});Object.defineProperty(o,'1',{value:1})})()",
  "(()=>{const o=[1,2,3];Object.defineProperty(o,'length',{writable:false});return Object.getOwnPropertyDescriptor(o,'length')})()", "(()=>{const s=new String('ab');return [Object.getOwnPropertyDescriptor(s,'0'),Object.getOwnPropertyDescriptor(s,'length'),Object.getOwnPropertyNames(s)]})()", "(()=>{const s=new String('ab');s[5]=1;s.x=1;return Object.getOwnPropertyNames(s)})()", "(()=>{const s=new String('ab');Object.defineProperty(s,'0',{value:'a'});return 1})()", "(()=>{const s=new String('ab');Object.defineProperty(s,'0',{value:'b'})})()", "(()=>{const s=new String('ab');return delete s[0]})()", "(()=>{'use strict';const s=new String('ab');delete s[0]})()",
  "(()=>{function f(a,b){}return Object.getOwnPropertyNames(f)})()", "(()=>{function f(a,b){'use strict'}return Object.getOwnPropertyNames(f)})()", "(()=>{class A{static m(){}}return Reflect.ownKeys(A).map(String)})()", "(()=>{class A{get x(){return 1}}return Object.getOwnPropertyDescriptor(A.prototype,'x')})()", "(()=>{class A{static x=1}return Object.getOwnPropertyDescriptor(A,'x')})()", "(()=>{class A{x=1}return Object.getOwnPropertyDescriptor(new A,'x')})()", "(()=>{class A{m(){}}return Object.getOwnPropertyDescriptor(A.prototype,'m')})()", "(()=>{class A{}return Object.getOwnPropertyDescriptor(A,'prototype')})()",
  "(()=>{const o={a:1,b:2};const out=[];for(const k in o){out.push(k);delete o.b}return out})()", "(()=>{const o={a:1};const out=[];for(const k in o){out.push(k);o.z=1}return out})()", "(()=>{const p={x:1};const o=Object.create(p);o.y=2;const out=[];for(const k in o)out.push(k);return out})()", "(()=>{const p={x:1};const o=Object.create(p);Object.defineProperty(o,'x',{value:1,enumerable:false});const out=[];for(const k in o)out.push(k);return out})()", "(()=>{const out=[];for(const k in 'ab')out.push(k);return out})()", "(()=>{const out=[];for(const k in [5,,6])out.push(k);return out})()", "(()=>{const out=[];for(const k in null)out.push(k);return out})()", "(()=>{const out=[];for(const k in {b:1,2:1,a:1,1:1})out.push(k);return out})()",
  "JSON.stringify(Object.freeze({a:[1,{b:2}]}))", "JSON.stringify({a:undefined,b:()=>1,c:Symbol(),d:NaN,e:-0,f:new Date(0)})", "JSON.stringify([undefined,()=>1,Symbol()])", "JSON.stringify(new Map([[1,2]]))", "JSON.stringify(new Set([1]))", "JSON.stringify(Object.create({a:1}))", "JSON.stringify({[Symbol.iterator]:1,a:1})", "JSON.stringify({a:1n})", "JSON.stringify(Object.assign(Object.create(null),{a:1}))", "JSON.stringify([,1])", "JSON.stringify({toJSON(){return 5}})", "JSON.stringify(Object.defineProperty({},'a',{get(){return 1},enumerable:true}))",
  "structuredClone", "typeof queueMicrotask", "(()=>{const o={};const s=Symbol('k');o[s]=1;o.a=2;return Reflect.ownKeys(o).map(String)})()", "(()=>{const o={};o[2]=1;o.b=1;o[1]=1;o.a=1;o[Symbol.iterator]=1;return Reflect.ownKeys(o).map(String)})()", "(()=>{const o={b:1,a:2};delete o.b;o.b=3;return Object.keys(o)})()", "(()=>{const o={a:1,b:2};o.a=5;return Object.keys(o)})()", "(()=>{const o={};o['4294967294']=1;o['4294967295']=1;o['4294967293']=1;return Object.keys(o)})()", "(()=>{const o={};o[-1]=1;o[1]=1;o['1e3']=1;o[1000]=1;return Object.keys(o)})()", "(()=>{const o={};o[0.5]=1;o['0.5']=2;o[1.0]=3;return Object.keys(o)})()", "(()=>{const o={};o[-0]=1;return Object.keys(o)})()", "(()=>{const o={};o[1n]=1;return Object.keys(o)})()", "(()=>{const o={};o[{}]=1;return Object.keys(o)})()", "(()=>{const o={};o[[1,2]]=1;return Object.keys(o)})()", "(()=>{const o={};o[null]=1;o[undefined]=2;return Object.keys(o)})()", "(()=>{const o={};o[Symbol.for('a')]=1;return Object.getOwnPropertySymbols(o).map(s=>Symbol.keyFor(s))})()",
];
for (const c of objCases) E(c);

// ---- Corpos livres: efeitos observáveis em ordem (Proxy como espião).
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.map.call(p,x=>x);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.includes.call(p,2);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.indexOf.call(p,2);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.join.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.reverse.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.shift.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.unshift.call(p,0);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([3,1,2],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.sort.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.splice.call(p,1,1);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k));return Reflect.set(t,k,v,r)},defineProperty(t,k,d){l.push('def '+String(k));return Reflect.defineProperty(t,k,d)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.fill.call(p,0);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.slice.call(p,1);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.concat.call([],p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.toSorted.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.with.call(p,1,9);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.flat.call(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.at.call(p,-1);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.findLast.call(p,x=>x===2);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});Array.prototype.reduceRight.call(p,(a,b)=>a+b);globalThis.R=S(l)");
B("const l=[];const p=new Proxy([1,2,3],{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)},has(t,k){l.push('has '+String(k));return Reflect.has(t,k)}});[...Array.prototype.entries.call(p)];globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)}});Object.entries(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)}});Object.assign({},p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)}});Object.freeze(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},defineProperty(t,k,d){l.push('def '+String(k)+JSON.stringify(d));return Reflect.defineProperty(t,k,d)},preventExtensions(t){l.push('pe');return Reflect.preventExtensions(t)}});Object.freeze(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},defineProperty(t,k,d){l.push('def '+String(k)+JSON.stringify(d));return Reflect.defineProperty(t,k,d)},preventExtensions(t){l.push('pe');return Reflect.preventExtensions(t)}});Object.seal(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},isExtensible(t){l.push('ie');return Reflect.isExtensible(t)}});Object.isFrozen(p);globalThis.R=S(l)");
B("const l=[];const p=new Proxy({a:1,b:2},{ownKeys(t){l.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)}});Object.getOwnPropertyDescriptors(p);globalThis.R=S(l)");
B("const l=[];const d={get enumerable(){l.push('enumerable');return true},get configurable(){l.push('configurable');return true},get value(){l.push('value');return 1},get writable(){l.push('writable');return true},get get(){l.push('get');return undefined},get set(){l.push('set');return undefined}};try{Object.defineProperty({},'a',d)}catch(e){}globalThis.R=S(l)");
B("const l=[];const p=new Proxy({},{get(t,k){l.push(String(k));return undefined}});Object.defineProperty({},'a',p);globalThis.R=S(l)");
B("const l=[];const src={get a(){l.push('ga');return 1},get b(){l.push('gb');return 2}};const t=new Proxy({},{set(t,k,v){l.push('set '+k);return true},defineProperty(t,k,d){l.push('def '+k);return true}});Object.assign(t,src);globalThis.R=S(l)");
B("const l=[];const m=new Map([[1,1]]);const o={get size(){l.push('size');return 1},has(k){l.push('has');return true},keys(){l.push('keys');return[1].values()}};new Set([1,2]).union(o);globalThis.R=S(l)");
B("const l=[];const mk=n=>({[Symbol.iterator](){l.push('iter '+n);return{next(){l.push('next '+n);return{done:true}}}}});Object.groupBy(mk(1),x=>x);globalThis.R=S(l)");
B("const l=[];const mk=n=>({[Symbol.iterator](){l.push('iter '+n);return{next(){l.push('next '+n);return{done:true}}}}});new Map(mk(1));new Set(mk(2));Array.from(mk(3));globalThis.R=S(l)");
B("const l=[];class M extends Map{constructor(it){l.push('ctor');super(it)} set(k,v){l.push('set '+k);return super.set(k,v)}};new M([[1,2],[3,4]]);globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:[1,2]}},return(){l.push('return');return{}}};try{new Map(it)}catch(e){l.push(e.name)}globalThis.R=S(l.slice(0,6))");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:5}},return(){l.push('return');return{}}};try{new Map(it)}catch(e){l.push(e.name)}globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');return{}}};try{new WeakSet(it)}catch(e){l.push(e.name)}globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');return{}}};const [a]=it;globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');return{}}};for(const x of it){break}globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');throw new Error('rt')}};try{for(const x of it){throw new Error('body')}}catch(e){l.push(e.message)}globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');return 5}};try{for(const x of it){break}}catch(e){l.push(e.name)}globalThis.R=S(l)");
B("const l=[];const it={[Symbol.iterator](){return this},next(){l.push('next');return{done:false,value:1}},return(){l.push('return');return 5}};try{Array.from(it,()=>{throw new Error('m')})}catch(e){l.push(e.message)}globalThis.R=S(l)");
B("const a=[1,2,3];const r=[];const it=a[Symbol.iterator]();r.push(it.next().value);a.splice(0,1);r.push(it.next().value);globalThis.R=S(r)");
B("const a=[1,2,3];const r=[];for(const x of a){r.push(x);if(a.length<6)a.push(x*10)}globalThis.R=S(r)");
B("const a=[0,1,2,3,4,5];const r=[];for(const x of a){r.push(x);a.shift()}globalThis.R=S(r)");
B("const s=new Set([1,2,3,4]);const r=[];for(const x of s){r.push(x);s.delete(x+1);if(x===1)s.add(1)}globalThis.R=S(r)");
B("const m=new Map([[1,'a'],[2,'b'],[3,'c']]);const r=[];m.forEach((v,k)=>{r.push(k);m.delete(k+1)});globalThis.R=S(r)");
B("const m=new Map([[1,'a']]);const r=[];m.forEach((v,k)=>{r.push(k);if(k<4){m.delete(k);m.set(k+1,'x')}});globalThis.R=S(r)");
B("const s=new Set([1,2,3]);const r=[];s.forEach(x=>{r.push(x);if(x===1){s.clear();s.add(9)}});globalThis.R=S(r)");
B("const m=new Map();const o={};m.set(o,1);m.set({},2);globalThis.R=S([m.get(o),m.get({}),m.size])");
B("const m=new Map([[1,1],[2,2]]);const it=m.keys();m.clear();m.set(5,5);globalThis.R=S([...it])");
B("const o=Object.create({inherited:1});o.own=2;globalThis.R=S([Object.keys(o),'inherited' in o,Object.hasOwn(o,'inherited'),JSON.stringify(o),{...o}])");
B("const a=Object.freeze([3,1,2]);try{a.sort();globalThis.R='no'}catch(e){globalThis.R=F(e)}");
B("const a=[1,2,3];Object.defineProperty(a,'1',{writable:false,value:2});try{a.reverse()}catch(e){globalThis.R=F(e)}");
B("const a=[1,2,3];Object.defineProperty(a,'1',{configurable:false});try{a.reverse();globalThis.R=S(a)}catch(e){globalThis.R=F(e)}");
B("const a=[1,2,3];Object.defineProperty(a,'0',{get(){return 'g'},set(v){L.push(v)},configurable:true});a.reverse();globalThis.R=S([L,a[2]])");
B("const a=[1,2,3];Object.defineProperty(a,'2',{get(){return 'g'},set(v){L.push(v)},configurable:true});a.sort();globalThis.R=S([L,Object.getOwnPropertyDescriptor(a,'2').set!==undefined])");
B("Array.prototype[1]='proto';const a=[0,,2];const r=[a.map(x=>x),a.indexOf('proto'),a.join(),a.includes('proto'),Object.keys(a),a.slice(),a.concat(),a.flat(),a.toSorted(),a.toReversed(),[...a]];delete Array.prototype[1];globalThis.R=S(r)");
B("Array.prototype[1]='proto';const a=[0,,2];const r=[a.reverse(),a.sort(),a.fill(7,1,2),a.shift(),a.pop(),a.copyWithin(0,1),a.with(0,5),a.at(1)];delete Array.prototype[1];globalThis.R=S(r)");
B("Object.prototype[1]='op';const a=[0,,2];const r=[a.join(),a.indexOf('op'),[...a],Array.from(a)];delete Object.prototype[1];globalThis.R=S(r)");
B("Array.prototype[0]='p';const a=Array(2);const r=[a.map(x=>x),a.filter(()=>true),a.every(x=>x==='p'),a.includes('p'),a.some(x=>x==='p'),a.find(x=>true),Array.from(a),a.toSpliced(0,0),a.toSorted()];delete Array.prototype[0];globalThis.R=S(r)");
B("Object.defineProperty(Array.prototype,'0',{set(v){L.push('set '+v)},configurable:true});const a=[];a[0]=1;a.push(2);const b=[5].concat([6]);const c=Array.of(7);const d=[8].map(x=>x);delete Array.prototype[0];globalThis.R=S([L,a,b,c,d])");
B("Object.defineProperty(Array.prototype,'0',{set(v){L.push('set '+v)},configurable:true});const r=[[1,2].slice(),Array.from([3]),[4,5].filter(()=>true),[6].flat(),[7].toSpliced(0,0),[8,9].toReversed()];delete Array.prototype[0];globalThis.R=S([L,r])");
B("const a=[1,2,3];a.constructor=undefined;globalThis.R=S([a.map(x=>x).constructor===Array,a.filter(x=>x).constructor===Array,a.slice().constructor===Array])");
B("let calls=[];class A extends Array{constructor(...n){calls.push(n.join());super(...n)}};const a=new A(1,2,3);calls=[];a.map(x=>x);a.filter(x=>x);a.slice(1);a.splice(0,1);a.flat();a.flatMap(x=>[x]);a.concat();globalThis.R=S(calls)");
B("let calls=[];class A extends Array{constructor(...n){calls.push(n.join());super(...n)}};A.from([1,2]);A.of(1,2);A.from({length:2});A.from(new Set([1]));globalThis.R=S(calls)");
B("let calls=[];class A extends Array{constructor(...n){calls.push(n.join());super(...n)}};const a=new A(3);calls=[];a.toSorted();a.toReversed();a.toSpliced(0,0);a.with(0,1);globalThis.R=S([calls,a.toSorted() instanceof A])");
B("const a=[5,1,4];const r=a.toSorted();globalThis.R=S([a,r,a===r])");
B("class A extends Array{};const a=A.from([3,1]);globalThis.R=S([a.toSorted() instanceof A,a.toReversed() instanceof A,a.with(0,1) instanceof A,a.toSpliced(0,0) instanceof A,a.slice() instanceof A])");

// ---- getOrInsert / getOrInsertComputed (Map e WeakMap).
{
  const kinds = [["Map", "1"], ["WeakMap", "k"]];
  for (const [C, key] of kinds) {
    const pre = "const k={};const m=new " + C + ";";
    const key1 = C === "Map" ? "1" : "k";
    E("(()=>{" + pre + "return [m.getOrInsert(" + key1 + ",'a'),m.getOrInsert(" + key1 + ",'b'),m.get(" + key1 + ")]})()");
    E("(()=>{" + pre + "return [m.getOrInsertComputed(" + key1 + ",x=>'c'+(x===" + key1 + ")),m.getOrInsertComputed(" + key1 + ",()=>{throw 1}),m.get(" + key1 + ")]})()");
    E("(()=>{" + pre + "m.set(" + key1 + ",undefined);return [m.getOrInsert(" + key1 + ",5),m.getOrInsertComputed(" + key1 + ",()=>6)]})()");
    E("(()=>{" + pre + "return m.getOrInsertComputed(" + key1 + ",5)})()");
    E("(()=>{" + pre + "return m.getOrInsertComputed(" + key1 + ")})()");
    E("(()=>{" + pre + "try{m.getOrInsertComputed(" + key1 + ",()=>{throw new RangeError('boom')})}catch(e){return [F(e),m.has(" + key1 + ")]}})()");
    E("(()=>{" + pre + "return m.getOrInsertComputed(" + key1 + ",()=>{m.set(" + key1 + ",'inner');return 'outer'})+'|'+m.get(" + key1 + ")})()");
    E("(()=>{" + pre + "return m.getOrInsertComputed(" + key1 + ",function(){return typeof this+String(arguments.length)})})()");
    E(C + ".prototype.getOrInsert.length+'|'+" + C + ".prototype.getOrInsertComputed.length+'|'+" + C + ".prototype.getOrInsert.name");
    E(C + ".prototype.getOrInsert.call({},1,2)");
    E(C + ".prototype.getOrInsertComputed.call(new " + (C === "Map" ? "WeakMap" : "Map") + ",{},()=>1)");
    E("(()=>{const m=new " + C + ";return m.getOrInsert()})()");
    E("(()=>{const m=new " + C + ";return m.getOrInsert(1,2)})()");
    E("(()=>{const m=new " + C + ";return m.getOrInsertComputed(Symbol('s'),()=>1) instanceof Symbol})()");
    E("(()=>{const m=new " + C + ";return m.getOrInsert(Symbol.for('r'),1)})()");
    E("(()=>{const m=new " + C + ";return m.getOrInsert(Symbol('w'),1)})()");
  }
  E("(()=>{const m=new Map;m.getOrInsert(NaN,1);m.getOrInsert(-0,2);return [m.getOrInsert(NaN,9),m.getOrInsert(0,9),[...m.keys()].map(x=>Object.is(x,-0))]})()");
  E("(()=>{const m=new Map([[1,'a']]);const it=m.keys();m.getOrInsert(2,'b');m.getOrInsertComputed(3,()=>'c');return [...it]})()");
  E("(()=>{class M extends Map{set(k,v){L.push('set');return super.set(k,v)}};const m=new M;m.getOrInsert(1,2);m.getOrInsertComputed(2,()=>3);return L})()");
  E("(()=>{const m=new Map;return Object.is(m.getOrInsertComputed(-0,x=>x),0)&&Object.is([...m.keys()][0],0)})()");
  E("(()=>{const m=new Map;m.getOrInsertComputed(-0,x=>L.push(Object.is(x,-0)));return L})()");
  E("typeof Set.prototype.getOrInsert+typeof WeakSet.prototype.getOrInsert");
  E("Object.getOwnPropertyNames(Map.prototype).filter(n=>/getOr/.test(n))");
  E("Object.getOwnPropertyNames(WeakMap.prototype)");
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "collections-golden-"));
const file = path.join(dir, "collections_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const original of programs) {
  if (HOST.test(original.slice(PRELUDE.length))) continue;
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  const { prepared: { source, meta }, marked } = runPrepared(original, file, preload, dir, 10000);
  if (seen.has(source)) continue;
  seen.add(source);
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(source.slice(PRELUDE.length)) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result, meta });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("collections", rows));
fs.rmSync(dir, { recursive: true, force: true });
