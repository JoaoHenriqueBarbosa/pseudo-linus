// Gera tests/golden/collection_mutation_bun.tsv: Map/Set/WeakMap/WeakSet sob mutação durante a iteração (delete,
// re-add, clear, add em forEach, for-of e iterador manual), iteradores esgotados que voltam após add, ordem de
// inserção, chaves -0/+0/NaN/objeto/símbolo/BigInt, Map.groupBy, size, os métodos novos de Set (union e companhia)
// com set-likes que registram acessos a size/has/keys e as mensagens de erro, subclasses e species, construtores com
// iterável que lança no meio (IteratorClose) e getOrInsert/getOrInsertComputed de Map e WeakMap, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro) e grava `R`; exceção vira
// `throw Nome: mensagem | log`. Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-collection-mutation-golden.js > tests/golden/collection_mutation_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  "const S=(v,d=0)=>{const t=typeof v;if(v===null)return'null';if(t==='undefined')return'undefined';" +
  "if(t==='number')return Object.is(v,-0)?'-0':String(v);if(t==='string')return JSON.stringify(v);" +
  "if(t==='boolean')return String(v);if(t==='bigint')return v+'n';if(t==='symbol')return String(v);" +
  "if(t==='function')return'fn';if(d>3)return'...';if(Array.isArray(v))return'['+v.map(x=>S(x,d+1)).join(',')+']';" +
  "if(v instanceof Map)return'Map{'+[...Map.prototype.entries.call(v)].map(([a,b])=>S(a,d+1)+'=>'+S(b,d+1)).join(',')+'}';" +
  "if(v instanceof Set)return'Set{'+[...Set.prototype.values.call(v)].map(a=>S(a,d+1)).join(',')+'}';" +
  "if(v instanceof Error)return v.name+': '+v.message;" +
  "return'{'+Object.keys(v).map(k=>k+':'+S(v[k],d+1)).join(',')+'}'};" +
  "const L=[];const O=x=>{globalThis.R=S(x)};" +
  // Set-like que registra os acessos a size, has e keys; `o` sobrescreve cada parte.
  "const SL=(items,o={})=>{const t={};" +
  "Object.defineProperty(t,'size',{get(){L.push('size');if('sizeThrow' in o)throw new Error('sizeboom');return 'size' in o?o.size:items.length},enumerable:true});" +
  "Object.defineProperty(t,'has',{get(){L.push('has');return 'has' in o?o.has:(x=>{L.push('has('+S(x)+')');return items.includes(x)})}});" +
  "Object.defineProperty(t,'keys',{get(){L.push('keys');return 'keys' in o?o.keys:(()=>{L.push('keys()');let i=0;" +
  "return{next(){L.push('next');return i<items.length?{value:items[i++],done:false}:{value:undefined,done:true}},return(){L.push('return');return{}}}})}});return t};" +
  // Iterável que registra iter/next/return; throwAt faz next lançar; returnThrows faz return lançar.
  "const IT=(items,o={})=>({[Symbol.iterator](){L.push('iter');let i=0;return{next(){L.push('next'+i);" +
  "if(o.throwAt===i)throw new Error('boom');return i<items.length?{value:items[i++],done:false}:{value:undefined,done:true}}," +
  "return(){L.push('return');if(o.returnThrows)throw new Error('retboom');return{}}}}});";

const programs = [];
const P = body =>
  programs.push(`${PRELUDE}try{${body}}catch(e){globalThis.R='throw '+e.name+': '+e.message+' | '+L.join(',')}`);

// ---- 1. Mutação durante a iteração: contêiner x estilo x mutação x passo.
const mk = (type, keys) => (type === "Map" ? `new Map(${JSON.stringify(keys)}.map(k=>[k,k*10]))` : `new Set(${JSON.stringify(keys)})`);
const put = (type, key) => (type === "Map" ? `c.set(${key},${key}*10)` : `c.add(${key})`);
const mutations = {
  delCur: () => "c.delete(k)",
  delNext: () => "c.delete(k+1)",
  delPrev: () => "c.delete(k-1)",
  delReadd: type => `c.delete(k);${put(type, "k")}`,
  addNew: type => put(type, "100+k"),
  clear: () => "c.clear()",
  clearAdd: type => `c.clear();${put(type, "99")}`,
  delFirstReadd: type => `c.delete(1);${put(type, "1")}`,
};
for (const type of ["Map", "Set"]) {
  for (const style of ["forEach", "forOf", "manual"]) {
    for (const [name, mut] of Object.entries(mutations)) {
      for (const step of [0, 1]) {
        const m = mut(type);
        const key = type === "Map" ? "e[0]" : "e";
        let loop;
        if (style === "forEach") loop = `c.forEach((a,k)=>{L.push(k);if(i===${step}){${m}}i++;if(i>12)throw new Error('loop')});`;
        else if (style === "forOf") loop = `for(const e of c){const k=${key};L.push(k);if(i===${step}){${m}}i++;if(i>12)break}`;
        else
          loop = `const it=c[Symbol.iterator]();let r;while(!(r=it.next()).done){const e=r.value;const k=${key};L.push(k);if(i===${step}){${m}}i++;if(i>12)break}` +
            `${put(type, "200")};L.push('post:'+S(it.next()));`;
        P(`const c=${mk(type, [1, 2, 3])};let i=0;${loop}O([L,c])`);
      }
    }
  }
}
// Laços que crescem, trocam e aninham.
for (const type of ["Map", "Set"]) {
  const key = type === "Map" ? "e[0]" : "e";
  P(`const c=${mk(type, [1, 2, 3])};let n=0;for(const e of c){L.push(${key});if(c.size<7){${put(type, "50+n")};n++}}O([L,c.size])`);
  P(`const c=${mk(type, [1, 2, 3])};let n=0;for(const e of c){const k=${key};L.push(k);if(n<4){c.delete(k);${put(type, "k")};n++}}O([L,c])`);
  P(`const c=${mk(type, [1, 2, 3])};for(const e of c){const a=${key};for(const f of c){const b=${type === "Map" ? "f[0]" : "f"};L.push(a+':'+b);if(b===2)c.delete(3)}}O([L,c])`);
  P(`const c=${mk(type, [1, 2, 3])};let n=0;c.forEach((v,k)=>{L.push(k);if(n++<2){c.delete(k);${put(type, "k+10")}}});O([L,c])`);
  P(`const c=${mk(type, [1, 2, 3, 4, 5])};const it=c.values();it.next();c.delete(1);c.delete(2);L.push(S(it.next()));c.delete(3);c.delete(4);L.push(S(it.next()));${put(type, "6")};L.push(S(it.next()));L.push(S(it.next()));O([L,c])`);
  P(`const c=${mk(type, [1, 2, 3])};const it=c.entries();const a=[];for(let i=0;i<10;i++){const r=it.next();a.push(r.done);if(r.done)break;if(i===0)c.clear();}O([a,c.size])`);
  P(`const c=${mk(type, [1, 2, 3])};c.forEach((v,k,self)=>{L.push(self===c);if(k===1){c.delete(1);${put(type, "1")}}});O([L,c])`);
  P(`const c=${mk(type, [3, 1, 2])};O([[...c].length,c.size,Array.from(c).length])`);
}
P(`const m=new Map([[1,'a'],[2,'b'],[3,'c']]);for(const [k] of m){L.push(k);m.set(k,'z'+k);if(k===1)m.set(4,'d')}O([L,m])`);
P(`const m=new Map([[1,'a'],[2,'b']]);m.forEach((v,k)=>{L.push(v);m.set(k,v+v)});O([L,m])`);
P(`const s=new Set([1,2,3]);s.forEach(function(v){L.push(this.tag+v)},{tag:'t'});O(L)`);
P(`const s=new Set([1,2,3]);s.forEach((v)=>{L.push(v);if(v===1)s.add(1)});O([L,s])`);
P(`const s=new Set([1,2,3]);s.forEach(v=>{L.push(v);if(v===3){s.delete(1);s.add(1)}});O([L,s])`);

// ---- 2. Iteradores esgotados que voltam após add, por tipo de iterador.
const iterKinds = { Map: ["keys", "values", "entries", "[Symbol.iterator]"], Set: ["values", "keys", "entries", "[Symbol.iterator]"] };
for (const type of ["Map", "Set"]) {
  for (const kind of iterKinds[type]) {
    const get = `c${kind.startsWith("[") ? kind : "." + kind}()`;
    const base = `const c=${mk(type, [1, 2])};const it=${get};`;
    P(`${base}while(!it.next().done);${put(type, "9")};O([it.next(),c.size])`);
    P(`${base}it.next();${put(type, "9")};it.next();L.push(S(it.next()));${put(type, "10")};O([L,S(it.next())])`);
    P(`const c=${mk(type, [])};const it=${get};L.push(S(it.next()));${put(type, "5")};O([L,S(it.next())])`);
    P(`${base}it.next();c.clear();${put(type, "7")};O([it.next(),it.next()])`);
    P(`${base}while(!it.next().done);c.clear();${put(type, "7")};O([it.next(),c.size])`);
    P(`${base}O([Object.prototype.toString.call(it),typeof it.next,it[Symbol.iterator]()===it,Object.getPrototypeOf(Object.getPrototypeOf(it))===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))])`);
  }
}

// ---- 3. Ordem de inserção.
P("const m=new Map([[1,'a'],[2,'b'],[3,'c']]);m.delete(1);m.set(1,'z');O([...m.keys()])");
P("const m=new Map([[1,'a'],[2,'b'],[3,'c']]);m.set(1,'z');O([...m])");
P("const m=new Map([[1,'a'],[2,'b'],[3,'c']]);m.delete(2);m.set(2,'b2');m.delete(1);m.set(1,'a2');O([...m])");
P("const s=new Set([1,2,3]);s.delete(1);s.add(1);O([...s])");
P("const s=new Set([1,2,3]);s.add(1);O([...s])");
P("const s=new Set([1,2,3]);s.delete(2);s.delete(2);s.add(2);s.add(4);s.delete(3);s.add(3);O([...s])");
P("const m=new Map();for(let i=0;i<20;i++)m.set(i%7,i);O([...m])");
P("const m=new Map();for(let i=0;i<30;i++){m.set(i,i);if(i%3===0)m.delete(i-1)}O([...m.keys()])");
P("const s=new Set();for(let i=0;i<40;i++){s.add(i%11);if(i%4===0)s.delete((i+3)%11)}O([...s])");
P("const m=new Map([['b',1],['a',2],[2,3],[1,4],['1',5]]);O([...m.keys()])");
P("const s=new Set(['b','a',2,1,'1',-1]);O([...s])");
P("const m=new Map([[1,1],[2,2]]);m.clear();m.set(2,2);m.set(1,1);O([...m.keys()])");
P("const s=new Set([1,2,3]);const a=[...s];s.clear();s.add(3);s.add(2);s.add(1);O([a,[...s]])");
P("const m=new Map([[1,1]]);for(let i=0;i<5;i++){m.delete(1);m.set(1,i);m.set(i+10,i)}O([...m])");
P("const m=new Map([[1,'a']]);const r=m.set(2,'b');O([r===m,m.delete(1),m.delete(1),m.size,m.clear(),m.size])");
P("const s=new Set([1]);const r=s.add(2);O([r===s,s.delete(1),s.delete(1),s.size,s.clear(),s.size])");
P("const m=new Map([[1,1],[2,2],[3,3]]);O([[...m.keys()],[...m.values()],[...m.entries()],[...m]])");
P("const s=new Set([1,2,3]);O([[...s.keys()],[...s.values()],[...s.entries()],s.keys===s.values,Set.prototype[Symbol.iterator]===Set.prototype.values])");
P("O([Map.prototype[Symbol.iterator]===Map.prototype.entries,Map.prototype.keys===Map.prototype.values])");

// ---- 4. Chaves especiais: matriz de pares.
const vals = ["0", "-0", "NaN", "1n", "1", "'1'", "null", "undefined", "0n"];
for (const a of vals) {
  for (const b of vals) {
    P(`const A=${a},B=${b};O([new Set([A,B]).size,new Map([[A,1],[B,2]]).size,new Map([[A,1]]).has(B),new Map([[A,1],[B,2]]).get(A),[...new Set([A])].map(x=>Object.is(x,A))])`);
  }
}
P("const o={};O([new Set([o,o]).size,new Set([{},{}]).size,new Map([[o,1]]).get(o),new Map([[{},1]]).has({})])");
P("const a=Symbol('a');O([new Set([a,a,Symbol('a')]).size,new Map([[a,1]]).get(a),new Map([[Symbol('a'),1]]).has(a)])");
P("O([new Set([Symbol.for('x'),Symbol.for('x')]).size,new Map([[Symbol.iterator,1]]).get(Symbol.iterator)])");
P("const m=new Map();m.set(-0,'neg');O([Object.is([...m.keys()][0],0),Object.is([...m.keys()][0],-0),m.get(0),m.get(-0),m.has(0n)])");
P("const s=new Set();const r=s.add(-0);O([Object.is([...s][0],0),s.has(0),s.has(-0),r===s])");
P("const m=new Map([[-0,1]]);m.forEach((v,k)=>L.push(Object.is(k,0)));for(const [k] of m)L.push(Object.is(k,-0));O(L)");
P("const m=new Map([[NaN,1]]);m.set(NaN,2);m.set(0/0,3);O([m.size,m.get(NaN),m.delete(NaN),m.size])");
P("const s=new Set([NaN,NaN,0/0,Number('x')]);O([s.size,s.has(NaN),s.delete(NaN),s.size])");
P("const m=new Map([[1n,'a'],[1,'b'],['1','c'],[true,'d']]);O([m.size,m.get(1n),m.get(1),m.get(BigInt(1)),m.get(BigInt('01')),m.get(2n**64n)])");
P("const m=new Map([[2n**64n,1]]);O([m.get(18446744073709551616n),m.get(2n**64n+1n),m.has(BigInt.asUintN(65,2n**64n))])");
P("const s=new Set([0n,-0n,0,-0]);O([s.size,[...s]])");
P("const s=new Set([1,'1',1n,true,'true',1.0,'1.0']);O([s.size,[...s]])");
P("const m=new Map();const f=()=>{};m.set(f,1);m.set(Math.max,2);m.set(Map,3);O([m.get(f),m.get(()=>{}),m.get(Math.max),m.size])");
P("const m=new Map([[undefined,1],[null,2]]);O([m.get(undefined),m.get(null),m.get(),m.has(),m.delete(),m.size])");
P("const m=new Map();m.set();O([[...m],m.has(undefined)])");
P("const s=new Set();s.add();O([[...s],s.has(undefined),s.has()])");
P("O([Map.groupBy([-0,0,NaN,NaN],x=>x).size,[...Map.groupBy([1,2],x=>x===1?-0:0).keys()].map(k=>Object.is(k,0))])");
P("const keys=[0,-0,NaN,1n,'a',Symbol.iterator,{},[]];const m=new Map(keys.map((k,i)=>[k,i]));O([m.size,keys.map(k=>m.has(k))])");
P("const o1={},o2={};const m=new Map([[o1,1],[o2,2]]);O([m.get(o1),m.get(o2),m.get({}),m.size])");
P("const m=new Map([[{valueOf(){return 1}},1]]);O([m.get(1),m.size])");
P("const m=new Map([['1',1],[1,2]]);O([m.get('1'),m.get(1),m.get(new String('1'))])");
P("const m=new Map([[1.0000000000000002,1],[1,2]]);O([m.size,m.get(1+Number.EPSILON)])");
P("const m=new Map([[Infinity,1],[-Infinity,2],[Number.MAX_VALUE,3],[Number.MIN_VALUE,4]]);O([m.get(1/0),m.get(-1/0),m.get(1e308*10),m.get(5e-324)])");

// ---- 5. Weak: chaves válidas e inválidas, mensagens de erro.
const weakKeys = ["1", "'a'", "null", "undefined", "Symbol('s')", "Symbol.for('r')", "Symbol.iterator", "1n", "true", "({})", "(()=>1)", "[]", "new Map", "new Number(1)"];
for (const k of weakKeys) {
  P(`const w=new WeakMap();O([w.set(${k},1)===w,w.has(${k}),w.get(${k}),w.delete(${k}),w.has(${k})])`);
  P(`const w=new WeakSet();O([w.add(${k})===w,w.has(${k}),w.delete(${k}),w.has(${k})])`);
  P(`const k=${k};O([new WeakMap().has(k),new WeakMap().get(k),new WeakMap().delete(k),new WeakSet().has(k),new WeakSet().delete(k)])`);
  P(`new WeakMap([[${k},1]]);O('ok')`);
  P(`new WeakSet([${k}]);O('ok')`);
}
P("const w=new WeakMap();const a={},b={};w.set(a,1);w.set(b,2);w.set(a,3);O([w.get(a),w.get(b),w.delete(a),w.get(a),w.has(b)])");
P("const w=new WeakSet();const a={};w.add(a);w.add(a);O([w.has(a),w.delete(a),w.delete(a)])");
P("O([typeof WeakMap.prototype.clear,typeof WeakSet.prototype.clear,typeof WeakMap.prototype.size,'size' in WeakMap.prototype,typeof WeakMap.prototype.forEach,typeof WeakSet.prototype.values,WeakMap.prototype[Symbol.toStringTag],WeakSet.prototype[Symbol.toStringTag]])");
P("O([Object.getOwnPropertyNames(WeakMap.prototype).sort(),Object.getOwnPropertyNames(WeakSet.prototype).sort()])");
P("O([Object.getOwnPropertyNames(Map.prototype).sort(),Object.getOwnPropertyNames(Set.prototype).sort()])");

// ---- 6. Map.groupBy e Object.groupBy.
P("O(Map.groupBy([1,2,3,4,5],x=>x%2))");
P("O(Map.groupBy(['a','bb','cc','d'],s=>s.length))");
P("O(Map.groupBy([1,2,3],(x,i)=>i))");
P("O(Map.groupBy('abcab',c=>c))");
P("O(Map.groupBy(new Set([3,1,2]),x=>x>1))");
P("O(Map.groupBy(new Map([[1,'a'],[2,'b']]),([k,v])=>v))");
P("const a={},b={};O(Map.groupBy([1,2,3,4],x=>x%2?a:b).get(a))");
P("O(Map.groupBy([NaN,NaN,1],x=>x))");
P("O(Map.groupBy((function*(){yield 1;yield 2;yield 3})(),x=>x%2))");
P("O(Map.groupBy([],x=>x))");
P("O([Map.groupBy([1,2],x=>x)instanceof Map,Map.groupBy.length,Map.groupBy.name])");
P("Map.groupBy(null,x=>x)");
P("Map.groupBy(undefined,x=>x)");
P("Map.groupBy([1],5)");
P("Map.groupBy([1])");
P("Map.groupBy(5,x=>x)");
P("O(Map.groupBy([1,2,3],x=>{L.push(x);if(x===2)throw new Error('cb');return x}))");
P("const it={[Symbol.iterator](){return{next(){L.push('n');return{value:1,done:false}},return(){L.push('ret');return{}}}}};Map.groupBy(it,x=>{throw new Error('cb')})");
P("O(Object.groupBy([1,2,3,4,5],x=>x%2?'odd':'even'))");
P("O(Object.groupBy([1,2,3],x=>x%2?Symbol.iterator:'e').e)");
P("const g=Object.groupBy([1,2],x=>'k'+x);O([Object.getPrototypeOf(g),Object.keys(g)])");
P("O(Object.keys(Object.groupBy([3,1,2,10,'b','a'],x=>x)))");

// ---- 7. size após operações.
P("const m=new Map();O([m.size,m.set(1,1).size,m.set(1,2).size,m.delete(2),m.size,m.delete(1),m.size])");
P("const s=new Set([1,1,2,2]);O([s.size,s.add(3).size,s.add(3).size,s.delete(9),s.size,s.clear(),s.size])");
P("const d=Object.getOwnPropertyDescriptor(Map.prototype,'size');O([typeof d.get,d.set,d.enumerable,d.configurable,d.get.name,d.get.length])");
P("Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call({})");
P("Object.getOwnPropertyDescriptor(Set.prototype,'size').get.call(new Map)");
P("Map.prototype.size");
P("Set.prototype.size");
P("const m=new Map([[1,1],[2,2]]);const it=m.keys();m.clear();O([m.size,it.next()])");
P("const s=new Set();for(let i=0;i<1000;i++)s.add(i);for(let i=0;i<1000;i+=2)s.delete(i);O([s.size,[...s].slice(0,3)])");
P("const m=new Map();for(let i=0;i<2000;i++){m.set(i,i);m.delete(i-5)}O([m.size,[...m.keys()].slice(0,3)])");

// ---- 8. Métodos novos de Set com set-likes.
const methods = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
const variants = [
  ["default", "SL([2,3,4])"],
  ["smaller", "SL([2])"],
  ["empty", "SL([])"],
  ["sizeNaN", "SL([2,3,4],{size:NaN})"],
  ["sizeUndefined", "SL([2,3,4],{size:undefined})"],
  ["sizeNegative", "SL([2,3,4],{size:-1})"],
  ["sizeAbc", "SL([2,3,4],{size:'abc'})"],
  ["sizeStringNumber", "SL([2,3,4],{size:'3'})"],
  ["sizeBigInt", "SL([2,3,4],{size:3n})"],
  ["sizeInfinity", "SL([2,3,4],{size:Infinity})"],
  ["sizeFraction", "SL([2,3,4],{size:2.7})"],
  ["sizeNegFraction", "SL([2,3,4],{size:-0.5})"],
  ["sizeValueOf", "SL([2,3,4],{size:{valueOf(){L.push('valueOf');return 3}}})"],
  ["sizeSymbol", "SL([2,3,4],{size:Symbol()})"],
  ["sizeNull", "SL([2,3,4],{size:null})"],
  ["sizeTrue", "SL([2,3,4],{size:true})"],
  ["sizeThrows", "SL([2,3,4],{sizeThrow:1})"],
  ["hasNumber", "SL([2,3,4],{has:5})"],
  ["hasUndefined", "SL([2,3,4],{has:undefined})"],
  ["hasNull", "SL([2,3,4],{has:null})"],
  ["hasString", "SL([2,3,4],{has:'x'})"],
  ["keysNumber", "SL([2,3,4],{keys:5})"],
  ["keysUndefined", "SL([2,3,4],{keys:undefined})"],
  ["keysReturnsNumber", "SL([2,3,4],{keys:()=>1})"],
  ["keysReturnsNull", "SL([2,3,4],{keys:()=>null})"],
  ["keysNoNext", "SL([2,3,4],{keys:()=>({})})"],
  ["keysNextNotFn", "SL([2,3,4],{keys:()=>({next:1})})"],
  ["nextReturnsNumber", "SL([2,3,4],{keys:()=>({next(){L.push('next');return 1}})})"],
  ["nextThrows", "SL([2,3,4],{keys:()=>({next(){L.push('next');throw new Error('nx')}})})"],
  ["hasThrows", "SL([2,3,4],{has(){L.push('has()');throw new Error('hs')}})"],
  ["hasTruthy", "SL([2,3,4],{has:x=>{L.push('has('+x+')');return 1}})"],
  ["keysDuplicates", "SL([2,2,3,3,-0,0,NaN,NaN],{})"],
  ["bigLie", "SL([2,3,4],{size:100})"],
  ["smallLie", "SL([2,3,4],{size:0})"],
];
for (const m of methods) for (const [, v] of variants) P(`const t=new Set([1,2,3]);const r=t.${m}(${v});O([r,t,L])`);
for (const m of methods) {
  P(`const t=new Set([1,2,3]);O([t.${m}([1,2]),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}(new Map([[2,'x'],[9,'y']])),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}(new Set([2,9])),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}(5),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}(null),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}('abc'),L])`);
  P(`const t=new Set([1,2,3]);O([t.${m}(),L])`);
  P(`O([Set.prototype.${m}.length,Set.prototype.${m}.name])`);
  P(`Set.prototype.${m}.call(new Map,new Set)`);
  P(`Set.prototype.${m}.call({},new Set)`);
}
// Mutação de this durante os métodos.
P("const t=new Set([1,2,3]);const o={size:5,has(x){L.push('has'+x);t.delete(2);return true},keys(){return[].values()}};O([t.intersection(o),t,L])");
P("const t=new Set([1,2,3]);const o={size:1,has(){return true},keys(){let i=0;return{next(){L.push('n');t.add(10+i);return i<2?{value:[1,2][i++],done:false}:{done:true}}}}};O([t.intersection(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:5,has(x){L.push('has'+x);if(x===1)t.clear();return true},keys(){return[].values()}};O([t.difference(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:1,has(){return true},keys(){return{next(){L.push('n');t.delete(2);return{done:true}}}}};O([t.difference(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:2,has(x){L.push('has'+x);t.add(7);return false},keys(){return[].values()}};O([t.isSubsetOf(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:2,has(x){L.push('has'+x);t.delete(1);return true},keys(){return[5,6].values()}};O([t.isSupersetOf(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:1,has(x){return true},keys(){return{next(){L.push('n');t.add(8);return{done:true}}}}};O([t.union(o),[...t],L])");
P("const t=new Set([1,2,3]);const o={size:5,has(){return false},keys(){return[4,1].values()}};const r=t.union(o);r.add(99);O([[...r],[...t]])");
P("const t=new Set([3,1,2]);const o=new Set([2,5,3]);O([[...t.union(o)],[...t.intersection(o)],[...o.intersection(t)],[...t.difference(o)],[...t.symmetricDifference(o)],[...o.symmetricDifference(t)]])");
P("const t=new Set([1,2,3]);const o=new Set([3,2,1,0]);O([[...t.intersection(o)],[...o.intersection(t)],[...t.intersection(new Set([3,2]))]])");
P("const t=new Set([-0,NaN,1n]);O([[...t.union(new Set([0,NaN,1]))].map(x=>Object.is(x,-0)?'-0':String(x)),[...t.intersection(new Set([0,NaN]))].map(x=>Object.is(x,-0)?'-0':String(x))])");
P("const t=new Set([1,2]);O([t.isSubsetOf(t),t.isSupersetOf(t),t.isDisjointFrom(t),t.union(t)===t,t.intersection(t)===t,t.difference(t).size,t.symmetricDifference(t).size])");
P("const t=new Set();O([t.isSubsetOf(new Set),t.isSupersetOf(new Set),t.isDisjointFrom(new Set),t.union(new Set).size])");
P("const t=new Set([1,2,3]);const o=SL([9],{keys:()=>({next(){L.push('n');return{value:1,done:false}},return(){L.push('ret');return{}}})});O([t.isDisjointFrom(o),L])");
P("const t=new Set([1,2,3]);const o=SL([1],{size:1,keys:()=>({next(){L.push('n');return{value:1,done:false}},return(){L.push('ret');return{}}})});O([t.isSupersetOf(o),L])");
P("const t=new Set([1,2,3]);const o=SL([5],{size:2,keys:()=>({next(){L.push('n');return{value:5,done:false}},return(){L.push('ret');return{}}})});O([t.isSupersetOf(o),L])");
P("const t=new Set([1,2,3]);const o=SL([5],{size:2,has:x=>{L.push('has'+x);return false}});O([t.isSubsetOf(o),L])");
P("const t=new Set([1,2,3]);const o=SL([5],{size:2,has:x=>{L.push('has'+x);return x===1}});O([t.isSubsetOf(o),L])");
P("const t=new Set([1,2,3]);const o=SL([1,2],{size:1,has:x=>{L.push('has'+x);return true}});O([t.isDisjointFrom(o),L])");
P("const t=new Set([1,2,3]);const o=SL([1,2],{size:10,has:x=>{L.push('has'+x);return x===3}});O([t.isDisjointFrom(o),L])");

// ---- 9. Subclasses e species.
P("class MS extends Set{}const t=new MS([1,2]);const r=t.union(new Set([3]));O([r instanceof MS,r instanceof Set,r.constructor===Set,t.intersection(new Set([1])).constructor===Set,t.difference(new Set).constructor===Set,t.symmetricDifference(new Set([5])).constructor===Set])");
P("class MM extends Map{}O([MM.groupBy([1,2],x=>x)instanceof MM,MM.groupBy([1,2],x=>x).constructor===Map,new MM([[1,2]]).get(1),MM.name,MM.length])");
P("class MS extends Set{static get [Symbol.species](){L.push('species');return Array}}const t=new MS([1]);O([t.union(new Set([2])).constructor===Set,L])");
P("O([Map[Symbol.species]===Map,Set[Symbol.species]===Set,WeakMap[Symbol.species],class A extends Map{}[Symbol.species]===undefined])");
P("class A extends Map{}O([A[Symbol.species]===A,Object.getOwnPropertyDescriptor(Map,Symbol.species).get.name,Object.getOwnPropertyDescriptor(Set,Symbol.species).set])");
P("class MS extends Set{has(x){L.push('has'+x);return super.has(x)}get size(){L.push('size');return super.size}keys(){L.push('keys');return super.keys()}}const t=new MS([1,2,3]);O([[...t.intersection(new Set([2,3]))],t.isSubsetOf(new Set([1,2,3,4])),[...t.union(new Set([4]))],L])");
P("class MS extends Set{add(x){L.push('add'+x);return super.add(x)}}const t=new MS([1,2]);L.push('built');const r=t.union(new Set([3]));O([r.size,L])");
P("class MM extends Map{set(k,v){L.push('set'+k);return super.set(k,v*2)}}const m=new MM([[1,1],[2,2]]);O([m,L])");
P("class MM extends Map{get(k){L.push('get'+k);return super.get(k)}}const m=new MM([[1,1]]);const g=Map.groupBy([1],x=>x);O([m.get(1),m.has(1),L])");
P("class MS extends Set{forEach(){L.push('forEach')}[Symbol.iterator](){L.push('iter');return super[Symbol.iterator]()}}const t=new MS([1,2]);O([[...t],Array.from(t),new Set(t).size,L])");
P("class MM extends Map{constructor(){super();L.push('ctor')}}O([new MM() instanceof Map,L,Map.prototype.constructor===Map])");
P("class MS extends Set{}const t=new MS([1,2]);delete t.constructor;MS.prototype.constructor=undefined;O(t.union(new Set([3])).size)");
P("function F(){}F.prototype=Map.prototype;const m=Reflect.construct(Map,[[[1,2]]],F);O([m instanceof F,m.get(1),Object.getPrototypeOf(m)===F.prototype])");
P("const m=Reflect.construct(Set,[[1,2]],Object);O([Object.getPrototypeOf(m)===Object.prototype,Set.prototype.has.call(m,1)])");
P("class A extends WeakMap{}const o={};const w=new A([[o,1]]);O([w.get(o),w instanceof WeakMap])");
P("Map()");
P("Set()");
P("WeakMap()");
P("WeakSet()");
P("Map.call({})");
P("Map.prototype.get.call({},1)");
P("Map.prototype.get.call(new Set,1)");
P("Set.prototype.add.call(new Map,1)");
P("Set.prototype.values.call(1)");
P("Map.prototype.forEach.call(new Set,()=>1)");
P("Map.prototype.set.call(new WeakMap,{},1)");
P("WeakMap.prototype.get.call(new Map,{})");
P("WeakSet.prototype.add.call(new WeakMap,{})");
P("new Map().forEach(5)");
P("new Set().forEach()");
P("new Set([1]).forEach(null)");
P("new Map().keys.call(new Set)");
P("const it=new Map().keys();it.next.call({})");
P("const it=new Set().values();it.next.call(new Map().keys())");
P("const it=new Set([1]).values();Object.getPrototypeOf(it).next.call(new Map([[1,1]]).entries())");

// ---- 10. Construtor com iterável que lança no meio (IteratorClose).
const kinds = {
  Map: { valid: "[[ko[0],1],[ko[1],2],[ko[2],3]]", bad: "[[ko[0],1],5,[ko[2],3]]", adder: "set" },
  Set: { valid: "[1,2,3]", bad: null, adder: "add" },
  WeakMap: { valid: "[[ko[0],1],[ko[1],2],[ko[2],3]]", bad: "[[ko[0],1],5,[ko[2],3]]", adder: "set" },
  WeakSet: { valid: "ko", bad: "[ko[0],5,ko[2]]", adder: "add" },
};
for (const [C, { valid, bad, adder }] of Object.entries(kinds)) {
  const ko = "const ko=[{},{},{}];";
  P(`${ko}const c=new ${C}(IT(${valid}));O([S(c),L])`);
  P(`${ko}new ${C}(IT(${valid},{throwAt:1}))`);
  P(`${ko}new ${C}(IT(${valid},{throwAt:0}))`);
  P(`${ko}new ${C}(IT(${valid},{throwAt:3}))`);
  if (bad) {
    P(`${ko}new ${C}(IT(${bad}))`);
    P(`${ko}new ${C}(IT(${bad},{returnThrows:true}))`);
  }
  P(`${ko}class X extends ${C}{${adder}(){L.push('adder');throw new Error('adder')}}new X(IT(${valid}))`);
  P(`${ko}class X extends ${C}{${adder}(){L.push('adder');throw new Error('adder')}}new X(IT(${valid},{returnThrows:true}))`);
  P(`${ko}class X extends ${C}{${adder}(...a){L.push('adder'+a.length);return super.${adder}(...a)}}const x=new X(IT(${valid}));O([x instanceof ${C},L])`);
  P(`${ko}const orig=${C}.prototype.${adder};${C}.prototype.${adder}=5;try{new ${C}(IT(${valid}))}finally{${C}.prototype.${adder}=orig}`);
  P(`${ko}const orig=${C}.prototype.${adder};delete ${C}.prototype.${adder};try{new ${C}(IT(${valid}))}finally{${C}.prototype.${adder}=orig}`);
  P(`${ko}const orig=${C}.prototype.${adder};${C}.prototype.${adder}=5;try{new ${C}()}finally{${C}.prototype.${adder}=orig}O('ok-empty')`);
  P(`${ko}const orig=${C}.prototype.${adder};${C}.prototype.${adder}=5;try{new ${C}(null)}finally{${C}.prototype.${adder}=orig}O('ok-null')`);
  P(`new ${C}({[Symbol.iterator](){L.push('getter');throw new Error('itergetter')}})`);
  P(`new ${C}({[Symbol.iterator](){return{next(){return 1}}}})`);
  P(`new ${C}({[Symbol.iterator](){return{next:5}}})`);
  P(`new ${C}({[Symbol.iterator](){return 5}})`);
  P(`new ${C}({[Symbol.iterator]:5})`);
  P(`O([S(new ${C}(null)),S(new ${C}(undefined)),S(new ${C}())])`);
  P(`new ${C}(5)`);
  P(`new ${C}(true)`);
  P(`new ${C}({})`);
  P(`new ${C}('ab')`);
  P(`const ko=[{}];O([S(new ${C}([])),S(new ${C}(new Set))])`);
  P(`new ${C}([ko=0])`);
}
P("const m=new Map(IT([['a',1],['b',2]]));O([m,L])");
P("const e={get 0(){L.push('g0');return 'k'},get 1(){L.push('g1');return 'v'}};O([new Map([e]),L])");
P("const e={get 0(){L.push('g0');throw new Error('g0')},get 1(){L.push('g1');return 'v'}};new Map(IT([e]))");
P("const e={get 0(){L.push('g0');return 'k'},get 1(){L.push('g1');throw new Error('g1')}};new Map(IT([e]))");
P("new Map(IT(['ab']))");
P("O(new Map(IT([[1,2,3],[4]])))");
P("O(new Map(IT([{0:'a',1:'b',length:0}])))");
P("O(new Map(new Set([[1,2],[3,4]])))");
P("O(new Map(new Map([[1,2]])))");
P("O(new Set(new Map([[1,2]])))");
P("O(new Set('hello'))");
P("O(new Map(Object.entries({a:1,b:2})))");
P("O(new Set((function*(){yield 1;yield 1;yield 2})()))");
P("const g=(function*(){try{yield [1,2];yield 5}finally{L.push('fin')}})();try{new Map(g)}catch(e){L.push(e.name)}O(L)");
P("const g=(function*(){try{yield 1;yield 2;yield 3}finally{L.push('fin')}})();class X extends Set{add(v){if(v===2)throw new Error('two');return super.add(v)}}try{new X(g)}catch(e){L.push(e.message)}O(L)");

// ---- 11. getOrInsert / getOrInsertComputed (Map e WeakMap).
for (const C of ["Map", "WeakMap"]) {
  const k = C === "Map" ? "1" : "ko";
  const ko = "const ko={};";
  P(`${ko}const m=new ${C}([[${k},'old']]);O([m.getOrInsert(${k},'new'),m.get(${k})])`);
  P(`${ko}const m=new ${C}();O([m.getOrInsert(${k},'new'),m.get(${k}),m.has(${k})])`);
  P(`${ko}const m=new ${C}([[${k},'old']]);O([m.getOrInsertComputed(${k},()=>{L.push('cb');return 'new'}),m.get(${k}),L])`);
  P(`${ko}const m=new ${C}();O([m.getOrInsertComputed(${k},key=>{L.push(key===${k});return 'new'}),m.get(${k}),L])`);
  P(`${ko}const m=new ${C}();m.getOrInsertComputed(${k},5)`);
  P(`${ko}const m=new ${C}([[${k},1]]);m.getOrInsertComputed(${k},5)`);
  P(`${ko}const m=new ${C}([[${k},1]]);m.getOrInsertComputed(${k})`);
  P(`${ko}const m=new ${C}();m.getOrInsertComputed(${k},()=>{throw new Error('cb')})`);
  P(`${ko}const m=new ${C}();try{m.getOrInsertComputed(${k},()=>{throw new Error('cb')})}catch(e){}O([m.has(${k})])`);
  P(`${ko}const m=new ${C}();O([m.getOrInsertComputed(${k},()=>{m.set(${k},'inner');return 'outer'}),m.get(${k})])`);
  P(`${ko}const m=new ${C}();O([m.getOrInsert(${k}),m.get(${k})])`);
  P(`${ko}const m=new ${C}();let n=0;m.getOrInsertComputed(${k},()=>++n);m.getOrInsertComputed(${k},()=>++n);m.getOrInsert(${k},9);O([n,m.get(${k})])`);
  P(`O([${C}.prototype.getOrInsert.length,${C}.prototype.getOrInsert.name,${C}.prototype.getOrInsertComputed.length,${C}.prototype.getOrInsertComputed.name])`);
  P(`${C}.prototype.getOrInsert.call({},1,2)`);
  P(`${C}.prototype.getOrInsertComputed.call(new Set,1,()=>1)`);
  P(`new ${C}().getOrInsert(${k},1)===undefined;O(Object.getOwnPropertyDescriptor(${C}.prototype,'getOrInsert').enumerable)`);
}
P("const m=new Map();m.getOrInsert(-0,'a');O([Object.is([...m.keys()][0],0),m.get(0)])");
P("const m=new Map();m.getOrInsertComputed(-0,k=>{L.push(Object.is(k,-0));return 'a'});O([Object.is([...m.keys()][0],0),L])");
P("const m=new Map();O([m.getOrInsert(NaN,1),m.getOrInsert(NaN,2),m.size])");
P("const m=new Map([[1,1]]);O([m.getOrInsertComputed(2,()=>{m.delete(1);return 'x'}),[...m]])");
P("const m=new Map([[1,1]]);O([m.getOrInsertComputed(2,()=>{m.clear();m.set(3,3);return 'x'}),[...m]])");
P("const m=new Map([[1,1],[2,2]]);const it=m.keys();it.next();m.getOrInsert(3,3);O([[...it],[...m]])");
P("const m=new Map();O([m.getOrInsertComputed(1,function(){return this===undefined}),m.getOrInsertComputed(2,()=>undefined),m.has(2)])");
P("new WeakMap().getOrInsert(1,2)");
P("new WeakMap().getOrInsert('a',2)");
P("new WeakMap().getOrInsert(Symbol.for('r'),2)");
P("O(new WeakMap().getOrInsert(Symbol('s'),2))");
P("new WeakMap().getOrInsertComputed(1,()=>{L.push('cb');return 2})");
P("new WeakMap().getOrInsertComputed(null,5)");
P("O([typeof Set.prototype.getOrInsert,typeof WeakSet.prototype.getOrInsert,typeof Map.prototype.emplace])");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "collection-mutation-golden-"));
// Programa por `vm.runInThisContext` (ProgramExecutable do JSC puro, sem o transpilador do bun); o SyntaxError de
// compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "collection_source.js");
const file = path.join(dir, "collection_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const source of programs) {
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
