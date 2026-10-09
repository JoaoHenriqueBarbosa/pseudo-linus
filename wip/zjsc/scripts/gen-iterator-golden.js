// Gera tests/golden/iterator_bun.tsv: Iterator.prototype helpers (map, filter, take, drop, flatMap, reduce, toArray,
// forEach, some, every, find), Iterator.from, Iterator.concat, fechamento do iterador subjacente (quando `return` é
// chamado), mensagens de erro exatas, ordem de efeitos, helpers encadeados e reentrância, geradores (yield*, return e
// throw em todos os estados, finally que yielda), destructuring, spread, Array.from, Symbol.iterator de
// Map/Set/String/arguments/TypedArray, %ArrayIteratorPrototype% e os métodos de Set (union, intersection, difference,
// symmetricDifference, isSubsetOf, isSupersetOf, isDisjointFrom) com set-likes inválidos, medidos no bun 1.4.2.
// O que já está em collections_bun.tsv e control_flow_bun.tsv (casos simples de helper, yield* básico) não se repete:
// aqui entram as matrizes cruzadas (método x argumento x fonte x modo de consumo) com o log de chamadas em `L`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), como em gen-function-error-golden.js.
// Qualquer resultado com caminho da máquina é descartado.
// Uso: timeout 900 bun scripts/gen-iterator-golden.js > tests/golden/iterator_bun.tsv
const { emitFactored, prepareProgram } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Prelúdio: serializa o valor (tipo, buracos, -0, símbolos, ordem de chaves) e define `mk`, um iterador que registra
// cada `next`/`return` em L, herdando de Iterator.prototype (ou não, com `plain`).
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
  "const F=e=>e&&e.name?e.name+': '+e.message:'thrown '+String(e);",
  "const L=[];",
  "const mk=(n,vals,o={})=>{let i=0;const it={next(){L.push(n+'.next');if(o.throwAt===i)throw new Error(n+'-next-boom');",
  "return i<vals.length?{done:false,value:vals[i++]}:{done:true,value:undefined}}};",
  "if(!o.noReturn)it.return=function(){L.push(n+'.return');if(o.returnThrows)throw new Error(n+'-return-boom');return o.returnValue===undefined?{}:o.returnValue};",
  "return o.plain?it:Object.setPrototypeOf(it,Iterator.prototype)};",
  "function*G(n,vals){try{for(const v of vals){L.push(n+'.y'+v);yield v}}finally{L.push(n+'.fin')}}",
].join("");

const programs = [];
// Expressão com log: o resultado e L, ou a exceção e L.
const T = expr => programs.push(PRELUDE + "try{const v=(" + expr + ");globalThis.R=S([v,L])}catch(e){globalThis.R='throws '+F(e)+' '+S(L)}");
// Corpo livre (declarações): o corpo grava em R via S.
const B = body => programs.push(PRELUDE + "try{" + body + "}catch(e){globalThis.R='throws '+F(e)+' '+S(L)}");

// ---- Matriz de helpers: fonte x método x argumento x modo de consumo.
const sources = {
  rt: "mk('a',[1,2,3,4,5])",
  nort: "mk('a',[1,2,3,4,5],{noReturn:true})",
  empty: "mk('a',[])",
  arr: "[1,2,3,4,5].values()",
  gen: "G('g',[1,2,3,4,5])",
  nxtthrow: "mk('a',[1,2,3],{throwAt:1})",
  rtthrow: "mk('a',[1,2,3,4,5],{returnThrows:true})",
};
const boom = "x=>{throw new Error('cb')}";
const lazy = {
  map: [`x=>x*2`, `(x,i)=>[x,i]`, boom, `5`, `undefined`, `null`, `{}`, `function(){return this}`, `x=>{L.push('cb'+x);return x}`],
  filter: [`x=>x%2`, `(x,i)=>i>0`, boom, `5`, `undefined`, `x=>{L.push('cb'+x);return x>2}`, `Symbol()`],
  take: [`0`, `1`, `2`, `3`, `10`, `-1`, `NaN`, `Infinity`, `-Infinity`, `'2'`, `undefined`, `null`, `1.9`, `-0.5`, `2**32`, `'x'`, `1n`, `{valueOf(){L.push('vo');return 2}}`, `true`, `[]`, `Symbol()`],
  drop: [`0`, `1`, `2`, `3`, `10`, `-1`, `NaN`, `Infinity`, `-Infinity`, `'2'`, `undefined`, `null`, `1.9`, `-0.5`, `2**32`, `'x'`, `1n`, `{valueOf(){L.push('vo');return 2}}`, `true`, `[]`],
  flatMap: [`x=>[x,x]`, `x=>x`, `x=>'ab'`, `x=>new String('ab')`, `x=>[x].values()`, `x=>mk('in'+x,[x,x])`, `x=>5`, boom, `x=>null`, `x=>undefined`, `5`,
    `x=>({[Symbol.iterator](){return mk('o'+x,[x])}})`, `x=>({next(){return{done:true}}})`, `x=>new Set([x])`, `x=>({})`, `x=>x>2?'z':[x]`, `x=>mk('in'+x,[x],{noReturn:true})`],
};
const modes = {
  full: r => `[...${r}]`,
  once: r => `(i=>{const a=i.next();const b=i.return();const c=i.next();return[a,b,c]})(${r})`,
  none: r => `typeof ${r}`,
};
for (const [m, args] of Object.entries(lazy)) {
  for (const [sn, src] of Object.entries(sources)) {
    for (const a of args) {
      for (const [mn, mode] of Object.entries(modes)) {
        // Cobertura: o modo `once` só nas fontes principais, `none` só em rt/arr, para o total não explodir.
        if (!["rt", "gen"].includes(sn) && args.indexOf(a) > 3) continue;
        if (mn === "once" && !["rt", "gen"].includes(sn)) continue;
        if (mn === "none" && !["rt"].includes(sn)) continue;
        T(mode(`${src}.${m}(${a})`));
      }
    }
  }
}
const eager = {
  reduce: [`(a,x)=>a+x,0`, `(a,x)=>a+x`, `(a,x,i)=>a+i,10`, `${boom},0`, `5`, `(a,x)=>a+x,undefined`, `(a,x)=>{L.push('cb'+x);return a+x},100`, ``, `null,1`],
  toArray: [``, `1`],
  forEach: [`x=>L.push('e'+x)`, `(x,i)=>L.push(i)`, boom, `5`, `undefined`, ``],
  some: [`x=>x>2`, `x=>x>9`, `x=>{L.push('cb'+x);return false}`, boom, `5`, `undefined`, `x=>'s'`, `x=>0`],
  every: [`x=>x>0`, `x=>x>2`, `x=>{L.push('cb'+x);return true}`, boom, `5`, `undefined`, `x=>''`, `x=>1`],
  find: [`x=>x>2`, `x=>x>9`, `x=>{L.push('cb'+x);return false}`, boom, `5`, `undefined`, `x=>'s'`, `x=>0`],
};
for (const [m, args] of Object.entries(eager)) {
  for (const [sn, src] of Object.entries(sources)) for (const a of args) T(`${src}.${m}(${a})`);
}
// Fonte vazia e fonte com um elemento nos métodos que dependem de contagem.
for (const src of ["mk('a',[])", "mk('a',[7])", "[].values()", "[7].values()"]) {
  for (const c of ["reduce((a,x)=>a+x)", "reduce((a,x)=>a+x,0)", "toArray()", "some(x=>true)", "every(x=>false)", "find(x=>true)", "forEach(x=>x)"]) T(`${src}.${c}`);
}

// ---- `this` inválido e protocolo de leitura de `next`.
const recv = ["undefined", "null", "5", "'str'", "true", "Symbol()", "1n", "{}", "[]", "function(){}", "{next:5}", "{next(){return{done:true}}}", "new Proxy({},{get(t,k){L.push('get '+String(k));return undefined}})", "{get next(){L.push('get next');return ()=>({done:true})}}", "Object.create(Iterator.prototype)"];
for (const r of recv) {
  for (const m of ["map(x=>x)", "filter(x=>x)", "take(1)", "drop(1)", "flatMap(x=>[x])", "reduce((a,b)=>a)", "toArray()", "forEach(x=>x)", "some(x=>x)", "every(x=>x)", "find(x=>x)"]) {
    T(`Iterator.prototype.${m.replace("(", ".call(" + r + (m.endsWith("()") ? "" : ","))}`.replace(",)", ")"));
  }
}
B("const it={get next(){L.push('get next');return function(){L.push('call');return{done:false,value:1}}},return(){L.push('ret');return{}}};Object.setPrototypeOf(it,Iterator.prototype);const h=it.map(x=>x);L.push('made');h.next();h.next();h.return();globalThis.R=S(L)");
B("const it=Object.setPrototypeOf({next(){L.push('n');return{done:false,value:1}}},Iterator.prototype);const h=it.take(2);it.next=()=>{L.push('swapped');return{done:true}};h.next();globalThis.R=S(L)");
B("const it=Object.setPrototypeOf({next(){return 5}},Iterator.prototype);try{it.map(x=>x).next()}catch(e){L.push(F(e))}try{it.filter(x=>x).next()}catch(e){L.push(F(e))}try{it.toArray()}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const it=Object.setPrototypeOf({next(){return{get done(){L.push('done');return false},get value(){L.push('value');return 1}}}},Iterator.prototype);it.take(1).next();globalThis.R=S(L)");
B("const it=Object.setPrototypeOf({next(){return{get done(){L.push('done');return true},get value(){L.push('value');return 1}}}},Iterator.prototype);it.toArray();globalThis.R=S(L)");
B("const it=mk('a',[1,2,3]);const h=it.map(x=>x);const r=[h.next(),h.return(),h.return(),h.next()];globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3]);const h=it.map(x=>x);const r=[h.return(),h.next()];globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3]);const h=it.take(1);const r=[h.next(),h.next(),h.next()];globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3]);const h=it.take(0);const r=[h.next(),h.next()];globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3]);const h=it.take(1);const r=[h.return(),h.next()];globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3],{returnThrows:true});const h=it.take(1);h.next();try{h.next()}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const it=mk('a',[1,2,3],{returnValue:5});const h=it.take(1);h.next();const r=h.next();globalThis.R=S([r,L])");
B("const it=mk('a',[1,2,3],{returnValue:5});const h=it.map(x=>x);h.next();try{globalThis.R=S([h.return(),L])}catch(e){globalThis.R=F(e)+S(L)}");
B("const h=[1,2,3].values().map(x=>x);globalThis.R=S([Object.getPrototypeOf(h)===Object.getPrototypeOf([].values().filter(x=>x)),h[Symbol.toStringTag],Object.getPrototypeOf(Object.getPrototypeOf(h))===Iterator.prototype,Reflect.ownKeys(Object.getPrototypeOf(h)),typeof h.next,typeof h.return,typeof h[Symbol.iterator],h[Symbol.iterator]()===h])");
B("const P=Object.getPrototypeOf([].values().map(x=>x));globalThis.R=S([Object.getOwnPropertyDescriptor(P,'next'),Object.getOwnPropertyDescriptor(P,'return'),Object.getOwnPropertyDescriptor(P,Symbol.toStringTag)])");
B("const P=Object.getPrototypeOf([].values().map(x=>x));try{P.next.call({})}catch(e){L.push(F(e))}try{P.return.call({})}catch(e){L.push(F(e))}try{P.next.call([].values())}catch(e){L.push(F(e))}try{P.next.call(undefined)}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const a=Object.getOwnPropertyNames(Iterator.prototype).sort();globalThis.R=S([a,Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor'),Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag)])");
B("globalThis.R=S([Iterator.name,Iterator.length,typeof Iterator,Object.getOwnPropertyNames(Iterator).sort(),Iterator.prototype===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())),Object.getOwnPropertyDescriptor(Iterator,'prototype')])");
for (const m of ["map", "filter", "take", "drop", "flatMap", "reduce", "toArray", "forEach", "some", "every", "find"]) {
  T(`[Iterator.prototype.${m}.name,Iterator.prototype.${m}.length,Object.getOwnPropertyDescriptor(Iterator.prototype,'${m}').enumerable,Object.getOwnPropertyDescriptor(Iterator.prototype,'${m}').writable,Object.getOwnPropertyDescriptor(Iterator.prototype,'${m}').configurable]`);
  T(`new Iterator.prototype.${m}(()=>{})`);
}

// ---- Iterator, Iterator.from, Iterator.concat.
T("new Iterator()");
T("Iterator()");
T("class A extends Iterator{};new A() instanceof Iterator");
T("class A extends Iterator{};[new A().map(x=>x)[Symbol.toStringTag],Object.getPrototypeOf(new A())===A.prototype]");
T("Reflect.construct(Iterator,[],Object)");
T("Reflect.construct(Iterator,[],class{})");
T("Reflect.construct(Iterator,[],Iterator)");
T("(()=>{function F(){};F.prototype=Iterator.prototype;return Reflect.construct(Iterator,[],F)})()");
const fromArgs = [
  "5", "undefined", "null", "'abc'", "'a\\ud83d\\ude00b'", "new String('xy')", "[1,2]", "new Set([1])", "new Map([[1,2]])", "{}", "{next(){return{done:true}}}",
  "{next(){return{done:true}},return(){L.push('r');return{}}}", "{[Symbol.iterator](){return mk('s',[1,2])}}", "{[Symbol.iterator](){return 5}}", "{[Symbol.iterator]:5}",
  "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined,next(){return{done:true}}}", "{[Symbol.iterator](){return{}}}", "{[Symbol.iterator](){return{next:5}}}",
  "mk('a',[1,2])", "mk('a',[1,2],{plain:true})", "[1].values()", "G('g',[1,2])", "{[Symbol.iterator]:function*(){yield 1}}", "Symbol()", "1n", "true", "function(){}",
  "{[Symbol.iterator](){L.push('get');return{next(){return{done:true}}}}}", "Object.setPrototypeOf({next(){return{done:true}}},Iterator.prototype)",
  "new Proxy({next(){return{done:true}}},{get(t,k){L.push('p '+String(k));return t[k]}})", "(function(){return arguments})(1,2)", "new Uint8Array([1,2])",
];
for (const a of fromArgs) {
  T(`Iterator.from(${a})`);
  T(`[...Iterator.from(${a})]`);
  T(`(w=>[Object.getPrototypeOf(w)===Iterator.prototype,w instanceof Iterator,Reflect.ownKeys(Object.getPrototypeOf(w)),typeof w.return])(Iterator.from(${a}))`);
  T(`(w=>[w.next(),w.return(),w.next()])(Iterator.from(${a}))`);
  T(`Iterator.from(${a}).map(x=>x).toArray()`);
}
B("const w=Iterator.from({next(){L.push('n');return{done:false,value:1}},return(){L.push('r');return 5}});globalThis.R=S([w.next(),w.return(),L])");
B("const w=Iterator.from({next(){L.push('n');return{done:false,value:1}}});globalThis.R=S([w.return(),w.return(),w.next(),L])");
B("const w=Iterator.from({next(){L.push('n');return 5}});try{w.next()}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const w=Iterator.from({next(){L.push('n:'+this.tag);return{done:true}},tag:'t'});w.next();globalThis.R=S(L)");
B("const P=Object.getPrototypeOf(Iterator.from({next(){}}));try{P.next.call({})}catch(e){L.push(F(e))}try{P.return.call({})}catch(e){L.push(F(e))}globalThis.R=S([L,P[Symbol.toStringTag],Object.getOwnPropertyNames(P)])");
if (typeof Iterator.concat === "function") {
  const concatArgs = [
    "", "[1,2]", "[1,2],[3]", "[],[]", "[1],[],[2]", "'ab'", "'ab',[1]", "new Set([1,2]),new Map([[3,4]])", "5", "undefined", "null", "{}", "{[Symbol.iterator]:5}",
    "{[Symbol.iterator](){return mk('a',[1,2])}}", "{[Symbol.iterator](){return mk('a',[1])}},{[Symbol.iterator](){return mk('b',[2])}}",
    "{[Symbol.iterator](){return mk('a',[1,2])}},5", "[1].values()", "mk('a',[1])", "{[Symbol.iterator](){return 5}}", "{[Symbol.iterator](){return{}}}",
    "{[Symbol.iterator]:function*(){yield 1;yield 2}},[3]", "{[Symbol.iterator](){L.push('open');return mk('a',[1])}},{[Symbol.iterator](){L.push('open2');return mk('b',[2])}}",
    "Object.assign(()=>{},{[Symbol.iterator]:function*(){yield 1}})", "{[Symbol.iterator]:undefined}", "{[Symbol.iterator]:null}",
  ];
  for (const a of concatArgs) {
    T(`Iterator.concat(${a})`);
    T(`[...Iterator.concat(${a})]`);
    T(`(h=>[h.next(),h.return(),h.next()])(Iterator.concat(${a}))`);
    T(`(h=>[Object.getPrototypeOf(h)===Iterator.prototype,Object.getPrototypeOf(h)===Object.getPrototypeOf(Iterator.concat()),h[Symbol.toStringTag]])(Iterator.concat(${a}))`);
    T(`Iterator.concat(${a}).take(2).toArray()`);
  }
  B("const h=Iterator.concat({[Symbol.iterator](){return mk('a',[1,2])}},{[Symbol.iterator](){return mk('b',[3])}});h.next();h.return();globalThis.R=S(L)");
  B("const h=Iterator.concat({[Symbol.iterator](){return mk('a',[1,2])}});try{h.next();h.next();h.next()}catch(e){}globalThis.R=S(L)");
  B("let h;h=Iterator.concat({[Symbol.iterator](){return{next(){try{h.next()}catch(e){L.push(F(e))}return{done:true}}}}});h.next();globalThis.R=S(L)");
  B("globalThis.R=S([Iterator.concat.name,Iterator.concat.length,Object.getOwnPropertyDescriptor(Iterator,'concat')])");
  T("new Iterator.concat()");
  T("Iterator.concat.call(undefined,[1]).toArray()");
} else {
  T("typeof Iterator.concat");
}
T("typeof Iterator.zip");
T("typeof Iterator.zipKeyed");
T("typeof Iterator.range");
T("typeof Iterator.prototype.chunks");
T("typeof Iterator.prototype.windows");
T("typeof Iterator.prototype.join");
T("typeof Iterator.prototype.flat");

// ---- Helpers encadeados e reentrância.
const chains = [
  "src.map(x=>x*2).filter(x=>x>2).take(2)", "src.filter(x=>x%2).map(x=>x+1).drop(1)", "src.take(3).map(x=>x).take(2)", "src.drop(1).take(2).drop(1)",
  "src.flatMap(x=>[x,x]).take(3)", "src.take(2).flatMap(x=>[x,x])", "src.map(x=>x).flatMap(x=>mk('in'+x,[x,x])).take(3)", "src.take(1).take(1).take(1)",
  "src.drop(1).drop(1)", "src.map(x=>{L.push('m'+x);return x}).filter(x=>{L.push('f'+x);return x>1}).take(1)",
  "src.filter(x=>{L.push('f'+x);return true}).map(x=>{L.push('m'+x);return x})", "src.take(0).map(x=>L.push('never'))",
  "src.map(x=>x).map(x=>x).map(x=>x)", "src.drop(10).map(x=>x)", "src.flatMap(x=>[]).map(x=>x)", "Iterator.from(src).map(x=>x).take(2)",
];
for (const c of chains) {
  for (const sn of ["rt", "gen"]) {
    const src = sources[sn];
    for (const end of [".toArray()", ".next()", ".return()", ".some(x=>x>2)", ".reduce((a,x)=>a+x,0)"]) {
      T(`(src=>${c})(${src})${end}`);
    }
  }
}
for (const sn of ["rt", "gen"]) {
  T(`(src=>src.map(x=>{throw new Error('m')}).take(1))(${sources[sn]}).toArray()`);
  T(`(src=>src.take(2).map(x=>{if(x>1)throw new Error('m');return x}))(${sources[sn]}).toArray()`);
}
// Reentrância.
B("let h;const it=mk('a',[1,2,3]);h=it.map(x=>{try{h.next()}catch(e){L.push(F(e))}return x});h.next();globalThis.R=S(L)");
B("let h;const it=mk('a',[1,2,3]);h=it.filter(x=>{try{h.next()}catch(e){L.push(F(e))}return true});h.next();globalThis.R=S(L)");
B("let h;const it=mk('a',[1,2,3]);h=it.map(x=>{try{h.return()}catch(e){L.push(F(e))}return x});h.next();globalThis.R=S([L,h.next()])");
B("let h;const it=mk('a',[1,2,3]);h=it.flatMap(x=>{try{h.next()}catch(e){L.push(F(e))}return[x]});h.next();globalThis.R=S(L)");
B("let h;const it=mk('a',[1,2,3]);h=it.take(2).map(x=>{try{h.next()}catch(e){L.push(F(e))}return x});h.next();globalThis.R=S(L)");
B("let h;const it=mk('a',[1,2,3]);h=it.drop(1);const m=h.map(x=>{try{h.next()}catch(e){L.push(F(e))}return x});m.next();globalThis.R=S(L)");
B("let h;const inner={[Symbol.iterator](){return{next(){try{h.next()}catch(e){L.push(F(e))}return{done:true}}}}};h=mk('a',[1]).flatMap(x=>inner);h.next();globalThis.R=S(L)");
B("let h;h=Object.setPrototypeOf({next(){try{h.next()}catch(e){L.push(F(e))}return{done:true}}},Iterator.prototype).map(x=>x);h.next();globalThis.R=S(L)");
B("let h;h=Object.setPrototypeOf({next(){try{h.return()}catch(e){L.push(F(e))}return{done:true}}},Iterator.prototype).map(x=>x);h.next();globalThis.R=S(L)");
B("let h;h=Object.setPrototypeOf({next(){try{h.return()}catch(e){L.push(F(e))}return{done:false,value:1}},return(){L.push('rt');return{}}},Iterator.prototype).map(x=>x);h.next();globalThis.R=S(L)");
B("let w;w=Iterator.from({next(){try{w.next()}catch(e){L.push(F(e))}return{done:true}}});w.next();globalThis.R=S(L)");
B("let g;function*q(){try{g.next()}catch(e){L.push(F(e))}yield 1}g=q();g.next();globalThis.R=S(L)");
B("let g;function*q(){try{g.return()}catch(e){L.push(F(e))}yield 1}g=q();g.next();globalThis.R=S(L)");
B("let g;function*q(){try{g.throw(new Error('t'))}catch(e){L.push(F(e))}yield 1}g=q();g.next();globalThis.R=S(L)");
B("let g;function*q(){try{[...g]}catch(e){L.push(F(e))}yield 1}g=q();g.next();globalThis.R=S(L)");
B("let g;function*q(){for(const x of g){}}g=q();try{g.next()}catch(e){L.push(F(e))}globalThis.R=S(L)");

// ---- Geradores.
const genBodies = {
  basic: "function*q(){L.push('s');const a=yield 1;L.push('a'+a);const b=yield 2;L.push('b'+b);return 3}",
  fin: "function*q(){try{L.push('s');yield 1;yield 2}finally{L.push('fin')}}",
  finyield: "function*q(){try{yield 1;yield 2}finally{L.push('fin');yield 'f'}}",
  finyield2: "function*q(){try{yield 1}finally{L.push('fin');yield 'f1';yield 'f2';L.push('fin-end')}}",
  finret: "function*q(){try{yield 1}finally{return 'override'}}",
  finthrow: "function*q(){try{yield 1}finally{throw new Error('fin-boom')}}",
  catchy: "function*q(){try{yield 1}catch(e){L.push('caught '+F(e));yield 'c'}yield 'after'}",
  catchret: "function*q(){try{yield 1}catch(e){return 'cr'}}",
  nested: "function*q(){try{try{yield 1}finally{L.push('in')}}finally{L.push('out')}}",
  loop: "function*q(){for(let i=0;i<3;i++){try{yield i}finally{L.push('f'+i)}}}",
  noyield: "function*q(){L.push('body');return 'r'}",
  throws: "function*q(){L.push('body');throw new Error('gen-throw')}",
};
const genOps = [
  "g.next()", "g.next(1)", "g.return(9)", "g.throw(new Error('t'))", "g.return()", "g.throw(5)",
  "[g.next(),g.next('x'),g.next('y'),g.next()]", "[g.next(),g.return(7),g.next()]", "[g.next(),g.return(7),g.return(8),g.next()]",
  "[g.return(7),g.next()]", "[g.next(),g.throw(new Error('t')),g.next()]", "[g.throw(new Error('t'))]",
  "[g.next(),g.next(),g.return(5),g.next()]", "[g.next(),g.next(),g.throw(new Error('t2')),g.next()]", "[...g]", "[g.next(),...g]",
  "(()=>{const r=[];for(const x of g){r.push(x);if(r.length>=1)break}return r})()", "(()=>{const r=[];try{for(const x of g){r.push(x);throw new Error('body')}}catch(e){r.push(F(e))}return r})()",
  "Array.from(g)", "(()=>{const [a]=g;return a})()", "(()=>{const [a,b,c]=g;return[a,b,c]})()", "(()=>{const [...r]=g;return r})()",
  "(()=>{const [,]=g;return 1})()", "g.next.call({})", "g.return.call(5)", "g[Symbol.iterator]()===g", "g.map(x=>x).toArray()", "g.take(1).toArray()", "g.drop(1).toArray()",
];
for (const [name, body] of Object.entries(genBodies)) {
  for (const op of genOps) B(`${body};const g=q();globalThis.R=S([${op},L])`);
}
for (const [name, body] of Object.entries(genBodies)) {
  for (const op of ["g.return(5)", "g.throw(new Error('first'))", "[g.return(5),g.next()]", "[g.throw(new Error('first')),g.next()]"]) {
    B(`${body};const g=q();try{globalThis.R=S([${op},L])}catch(e){globalThis.R='throws '+F(e)+S(L)}`);
  }
}
// yield*.
const delegates = {
  arr: "[1,2]", str: "'ab'", gen: "G('d',[1,2])", mkrt: "mk('d',[1,2],{plain:true})", mknort: "mk('d',[1,2],{plain:true,noReturn:true})",
  rtnonobj: "mk('d',[1,2],{plain:true,returnValue:5})", rtdone: "mk('d',[1,2],{plain:true,returnValue:{done:true,value:'rv'}})", rtnotdone: "mk('d',[1,2],{plain:true,returnValue:{done:false,value:'nd'}})",
  rtthrows: "mk('d',[1,2],{plain:true,returnThrows:true})", nxtthrows: "mk('d',[1,2],{plain:true,throwAt:1})",
  nothrow: "{[Symbol.iterator](){return{next(){L.push('n');return{done:false,value:1}},return(){L.push('r');return{}}}}}",
  withthrow: "{[Symbol.iterator](){return{next(){L.push('n');return{done:false,value:1}},throw(e){L.push('t:'+F(e));return{done:true,value:'tv'}},return(){L.push('r');return{}}}}}",
  withthrowcont: "{[Symbol.iterator](){return{next(){L.push('n');return{done:false,value:1}},throw(e){L.push('t:'+F(e));return{done:false,value:'tc'}}}}}",
  throwbad: "{[Symbol.iterator](){return{next(){return{done:false,value:1}},throw(e){return 5},return(){L.push('r');return{}}}}}",
  noiter: "5", nulliter: "null", undefiter: "undefined", baditer: "{[Symbol.iterator](){return 5}}",
  nextbad: "{[Symbol.iterator](){return{next(){return 5}}}}",
};
for (const [dn, d] of Object.entries(delegates)) {
  for (const op of ["[g.next(),g.next(),g.next()]", "[g.next(),g.return('R')]", "[g.next(),g.throw(new Error('T'))]", "[g.return('R')]", "[g.throw(new Error('T'))]", "[...g]", "[g.next('a'),g.next('b'),g.next('c'),g.next('d')]", "[g.next(),g.return('R'),g.next()]"]) {
    B(`function*q(){const r=yield* (${d});L.push('r='+S(r));return 'end'}const g=q();globalThis.R=S([${op},L])`);
  }
}
B("function*a(){try{yield 1;yield 2;return 'ra'}finally{L.push('afin')}}function*b(){const r=yield* a();L.push('got '+r);yield 'b'}globalThis.R=S([[...b()],L])");
B("function*a(){try{yield 1;yield 2}finally{L.push('afin')}}function*b(){try{yield* a()}finally{L.push('bfin')}}const g=b();g.next();globalThis.R=S([g.return('x'),L])");
B("function*a(){try{yield 1}finally{yield 'af'}}function*b(){yield* a()}const g=b();g.next();globalThis.R=S([g.return('x'),g.next(),g.next(),L])");
B("function*a(){try{yield 1}catch(e){yield 'caught'}}function*b(){yield* a()}const g=b();g.next();globalThis.R=S([g.throw(new Error('t')),g.next(),L])");
B("function*a(){yield 1}function*b(){yield* a();yield* a()}globalThis.R=S([...b()])");
B("function*a(){const x=yield 1;L.push('x='+x);const y=yield 2;L.push('y='+y)}function*b(){yield* a()}const g=b();g.next('ignored');g.next('X');g.next('Y');globalThis.R=S(L)");
B("function*b(){yield* [1,2,3]}const g=b();g.next();globalThis.R=S([g.return(7),g.next()])");
B("function*b(){yield* [1,2,3]}const g=b();g.next();try{g.throw(new Error('t'))}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const p=Object.getPrototypeOf(function*(){});const gp=p.prototype;globalThis.R=S([p[Symbol.toStringTag],gp[Symbol.toStringTag],Reflect.ownKeys(gp),Reflect.ownKeys(p),Object.getPrototypeOf(gp)===Iterator.prototype,typeof p.constructor])");
B("function*q(){}globalThis.R=S([Object.getPrototypeOf(q.prototype)===Object.getPrototypeOf(function*(){}).prototype,q.prototype.constructor,Reflect.ownKeys(q.prototype),Object.getOwnPropertyDescriptor(q,'prototype'),new q.constructor('yield 1')().next()])");
B("function*q(){}try{new q()}catch(e){L.push(F(e))}try{q.call(5).next();L.push('ok')}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("function*q(){yield this}const g=q.call({a:1});globalThis.R=S(g.next())");
B("function*q(){yield arguments.length}globalThis.R=S([...q(1,2,3)])");
B("function*q(){yield 1}q.prototype=null;const g=q();globalThis.R=S([Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){}).prototype,[...g]])");
B("function*q(){yield 1}q.prototype=5;const g=q();globalThis.R=S([Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){}).prototype])");
B("const o={*[Symbol.iterator](){yield 1;yield 2}};globalThis.R=S([[...o],Array.from(o),Math.max(...o)])");
B("class K{*m(){yield 1}static *s(){yield 2}}globalThis.R=S([[...new K().m()],[...K.s()],typeof new K().m().next])");
B("const g=(function*(){yield 1})();globalThis.R=S([g.toString(),Object.prototype.toString.call(g),String(g),g+''])");
B("function*q(){const x=yield;L.push(typeof x);yield}const g=q();g.next();g.next();globalThis.R=S(L)");
B("function*q(){yield yield yield 1}const g=q();globalThis.R=S([g.next(),g.next('a'),g.next('b'),g.next('c')])");
B("function*q(){return yield* (function*(){return 5})()}globalThis.R=S([...q()],q().next())");
B("function*q(){try{yield 1}finally{L.push('f')}}for(const x of q()){break}globalThis.R=S(L)");
B("function*q(){try{yield 1}finally{L.push('f');yield 2}}const r=[];for(const x of q()){r.push(x);break}globalThis.R=S([r,L])");
B("function*q(){try{yield 1}finally{L.push('f');throw new Error('ft')}}try{for(const x of q()){break}}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("function*q(){try{yield 1}finally{L.push('f');throw new Error('ft')}}try{for(const x of q()){throw new Error('body')}}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const g=(function*(){yield 1;yield 2;yield 3})();for(const x of g){if(x===1)break}globalThis.R=S([[...g],g.next()])");
B("const g=(function*(){yield 1;yield 2;yield 3})();const [a]=g;globalThis.R=S([a,[...g],g.next()])");
B("const g=(function*(){yield 1;yield 2;yield 3})();const [a,b,c,d]=g;globalThis.R=S([a,b,c,d,g.next()])");
B("const g=(function*(){yield 1;yield 2;yield 3})();const [a,...r]=g;globalThis.R=S([a,r,g.next()])");

// ---- Destructuring, spread, Array.from com iteradores instrumentados.
const dsrc = (n = "a", o = "") => `{[Symbol.iterator](){L.push('iter');return mk('${n}',[1,2,3],{plain:true${o}})}}`;
const dpats = ["[a]", "[a,b]", "[a,b,c]", "[a,b,c,d]", "[,]", "[,,]", "[a,,b]", "[...r]", "[a,...r]", "[a,b,c,...r]", "[a=9]", "[,,,d=7]", "[]", "[a,[b]]",
  "[{x}]", "[a=(()=>{throw new Error('def')})()]", "[a,b=L.push('def-b')]", "[o.p]", "[o.q.r]", "[...[a,b]]", "[...{length:n}]"];
for (const p of dpats) {
  for (const [lab, o] of [["", ""], ["rt", ""], ["nort", ",noReturn:true"], ["rtthrows", ",returnThrows:true"], ["nxtthrows", ",throwAt:1"], ["rtbad", ",returnValue:5"]]) {
    B(`const o={};let a,b,c,d,r,n,x;try{(${p}=${dsrc("a", o)})}catch(e){L.push('E:'+F(e))}globalThis.R=S([L,[a,b,c,d,r,n,x]])`);
    if (lab === "rt") B(`try{const ${p}=${dsrc("a", o)}}catch(e){L.push('E:'+F(e))}globalThis.R=S(L)`.replace("[o.p]", "[a]").replace("[o.q.r]", "[a]"));
  }
}
const spreadCases = [
  "[...src]", "[...src,...src]", "Math.max(...src)", "((...a)=>a)(...src)", "new Array(...src)", "new Set(src).size", "Array.from(src)", "Array.from(src,x=>x*2)",
  "Array.from(src,function(x,i){return [x,i,this.k]},{k:1})", "Array.from(src,5)", "Array.from(src,null)", "Array.from(src,undefined)", "new Map(src)", "Object.fromEntries(src)",
  "Promise.all(src)&&1", "Array.of(...src)", "String.raw`x`+[...src]", "[...src].length", "Object.assign({},...src)", "new Uint8Array(src)", "Uint8Array.from(src)", "new WeakSet(src)",
];
const spreadSrcs = [
  "{[Symbol.iterator](){L.push('iter');return mk('a',[1,2,3],{plain:true})}}", "{[Symbol.iterator](){return mk('a',[[1,2],[3,4]],{plain:true})}}",
  "{[Symbol.iterator](){return{next(){return{done:true}}}}}", "{[Symbol.iterator]:null,length:2,0:'x',1:'y'}", "{length:2,0:'x',1:'y'}", "5", "null", "undefined", "'ab'",
  "{[Symbol.iterator](){return mk('a',[1,2],{plain:true,returnThrows:true})}}", "{[Symbol.iterator](){return mk('a',[{},{}],{plain:true})}}",
];
for (const c of spreadCases) for (const s of spreadSrcs) T(`(src=>${c})(${s})`);
for (const c of ["Array.from(src,x=>{throw new Error('m')})", "new Map(src)", "new Set(src)", "Object.fromEntries(src)", "[...src].length", "new Uint8Array(src)"]) {
  B(`const src={[Symbol.iterator](){return mk('a',[[1,2],5,[3,4]],{plain:true})}};try{${c}}catch(e){L.push(F(e))}globalThis.R=S(L)`);
  B(`const src={[Symbol.iterator](){return mk('a',[[1,2],5,[3,4]],{plain:true,returnThrows:true})}};try{${c}}catch(e){L.push(F(e))}globalThis.R=S(L)`);
}
T("Array.from({length:3,0:'a',[Symbol.iterator]:undefined})");
T("Array.from({length:2,[Symbol.iterator]:function*(){yield 'g'}})");
T("Array.from.call(function(n){L.push('ctor '+n);return{}},{[Symbol.iterator](){return mk('a',[1],{plain:true})}})");
T("Array.from.call(function(n){L.push('ctor '+n);return{}},{length:2,0:1,1:2})");
T("Array.from.call(5,[1])");
T("[...'a\\ud83d\\ude00b\\ud83d']");
T("Array.from('\\ud83d\\ude00\\ude00\\ud83d')");
T("[...new String('x\\ud800')].map(s=>s.charCodeAt(0))");

// ---- Symbol.iterator em Map, Set, String, arguments, TypedArray, Array e os protótipos de iterador.
const builtinIters = {
  arr: "[1,,3]", set: "new Set([1,'a',NaN])", map: "new Map([[1,'a'],[2,'b']])", str: "'a\\ud83d\\ude00\\ud83dz'", args: "(function(){return arguments})(1,2,3)",
  u8: "new Uint8Array([5,6,7])", f64: "new Float64Array([1.5,-0])", big: "new BigInt64Array(2)", empty: "[]",
};
const iterMethods = {
  arr: ["[Symbol.iterator]()", "keys()", "values()", "entries()"], set: ["[Symbol.iterator]()", "keys()", "values()", "entries()"],
  map: ["[Symbol.iterator]()", "keys()", "values()", "entries()"], str: ["[Symbol.iterator]()"], args: ["[Symbol.iterator]()"],
  u8: ["[Symbol.iterator]()", "keys()", "values()", "entries()"], f64: ["[Symbol.iterator]()", "values()"], big: ["entries()"], empty: ["[Symbol.iterator]()", "entries()"],
};
for (const [k, expr] of Object.entries(builtinIters)) {
  for (const m of iterMethods[k]) {
    const it = `(${expr}).${m}`;
    T(`[...${it}]`);
    T(`(i=>[i.next(),i.next(),i.next(),i.next(),i.next(),i.next()])(${it})`);
    T(`(i=>[Object.prototype.toString.call(i),i[Symbol.toStringTag],i[Symbol.iterator]()===i,Object.getPrototypeOf(Object.getPrototypeOf(i))===Iterator.prototype,typeof i.return,Reflect.ownKeys(i)])(${it})`);
    T(`${it}.map((x,i)=>[x,i]).toArray()`);
    T(`${it}.drop(1).take(2).toArray()`);
    T(`(i=>[i.return,i.next.call(${it}),i.toArray(),i.toArray()])(${it})`);
  }
}
const protoOf = e => `Object.getPrototypeOf(${e})`;
for (const e of ["[].values()", "new Set().values()", "new Map().entries()", "''[Symbol.iterator]()", "'a'.matchAll(/a/g)", "[].values().map(x=>x)", "(function*(){})()"]) {
  T(`(P=>[Object.getOwnPropertyNames(P),Object.getOwnPropertySymbols(P).map(String),P[Symbol.toStringTag],Object.getOwnPropertyDescriptor(P,'next'),Object.getOwnPropertyDescriptor(P,Symbol.toStringTag)])(${protoOf(e)})`);
  T(`(P=>{try{return P.next.call({})}catch(e){return F(e)}})(${protoOf(e)})`);
  T(`(P=>{try{return P.next.call(undefined)}catch(e){return F(e)}})(${protoOf(e)})`);
  T(`(P=>{try{return P.next.call([].values().map(x=>x))}catch(e){return F(e)}})(${protoOf(e)})`);
  T(`(P=>[P.next.name,P.next.length,P.hasOwnProperty('constructor'),P.hasOwnProperty('return'),P.hasOwnProperty('throw')])(${protoOf(e)})`);
}
B("const a=[1,2];const i=a.values();i.next();i.next();i.next();a.push(3);globalThis.R=S([i.next(),[...i]])");
B("const a=[1,2];const i=a.values();a.push(3);globalThis.R=S([...i])");
B("const a=[1,2,3];const i=a.entries();i.next();a.length=1;globalThis.R=S([i.next(),i.next()])");
B("const t=new Uint8Array(3);const i=t.values();i.next();t.buffer.resizable;globalThis.R=S([i.next(),i.next(),i.next()])");
B("const rab=new ArrayBuffer(4,{maxByteLength:8});const t=new Uint8Array(rab);const i=t.values();i.next();rab.resize(1);globalThis.R=S([i.next()])");
B("const rab=new ArrayBuffer(4,{maxByteLength:8});const t=new Uint8Array(rab);const i=t.values();rab.resize(8);globalThis.R=S([[...i].length])");
B("const t=new Uint8Array(4);const i=t.values();structuredClone;t.buffer.transfer&&t.buffer.transfer();try{globalThis.R=S(i.next())}catch(e){globalThis.R=F(e)}");
B("const t=new Uint8Array(4);const i=t.values();t.buffer.transfer&&t.buffer.transfer();try{t.values()}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const s=new Set([1,2,3]);const i=s.values();s.delete(1);s.add(4);globalThis.R=S([...i])");
B("const s=new Set([1,2,3]);const i=s.values();i.next();s.clear();s.add(9);globalThis.R=S([...i])");
B("const m=new Map([[1,1],[2,2]]);const i=m.keys();m.delete(1);m.set(1,'again');globalThis.R=S([...i])");
B("const m=new Map([[1,1]]);const i=m.entries();[...i];m.set(2,2);globalThis.R=S([i.next(),[...i]])");
B("const s=new Set([1]);const i=s[Symbol.iterator]();globalThis.R=S([i===s.values(),Set.prototype[Symbol.iterator]===Set.prototype.values,Set.prototype.keys===Set.prototype.values,Map.prototype[Symbol.iterator]===Map.prototype.entries,Array.prototype[Symbol.iterator]===Array.prototype.values,String.prototype[Symbol.iterator].name,Set.prototype.values.name,Map.prototype.entries.name])");
B("const a=function(){return arguments}(1,2);globalThis.R=S([a[Symbol.iterator]===Array.prototype.values,Object.getOwnPropertyDescriptor(a,Symbol.iterator),Object.prototype.toString.call(a)])");
B("const t=Uint8Array.prototype;globalThis.R=S([t[Symbol.iterator]===t.values,Object.getPrototypeOf(Uint8Array.prototype)[Symbol.iterator].name,Object.getPrototypeOf(Uint8Array).prototype.values===Object.getPrototypeOf(Uint8Array.prototype).values])");
B("const i=''[Symbol.iterator]();globalThis.R=S([Object.prototype.toString.call(i),i[Symbol.toStringTag],i.next(),i.next()])");
B("try{String.prototype[Symbol.iterator].call(null)}catch(e){L.push(F(e))}try{String.prototype[Symbol.iterator].call(undefined)}catch(e){L.push(F(e))}globalThis.R=S([L,[...String.prototype[Symbol.iterator].call(5)],[...String.prototype[Symbol.iterator].call({toString(){return 'ts'}})]])");
B("try{Array.prototype.values.call(null)}catch(e){L.push(F(e))}globalThis.R=S([L,[...Array.prototype.values.call('ab')],[...Array.prototype.values.call({length:2,0:'x'})],[...Array.prototype.keys.call({length:2})]])");
B("try{Map.prototype.entries.call(new Set)}catch(e){L.push(F(e))}try{Set.prototype.values.call(new Map)}catch(e){L.push(F(e))}try{Map.prototype[Symbol.iterator].call({})}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("try{Uint8Array.prototype.values.call([])}catch(e){L.push(F(e))}try{Uint8Array.prototype.keys.call(5)}catch(e){L.push(F(e))}try{Uint8Array.prototype.entries.call(new DataView(new ArrayBuffer(1)))}catch(e){L.push(F(e))}globalThis.R=S(L)");
B("const o={};o[Symbol.iterator]=()=>({next:()=>({done:true})});globalThis.R=S([[...o],Array.from(o)])");
B("const o={[Symbol.iterator](){return{next(){return{done:true}}}}};globalThis.R=S(Array.from(o))");
B("const i=[1,2][Symbol.iterator]();const j=Object.create(i);globalThis.R=S([j.next(),i.next()])");
B("const P=Object.getPrototypeOf([][Symbol.iterator]());const orig=P.next;P.next=function(){L.push('patched');return orig.call(this)};globalThis.R=S([[...[1,2]],Array.from([3]),L])");
B("const P=Object.getPrototypeOf([][Symbol.iterator]());const orig=P.next;P.next=function(){L.push('patched');return orig.call(this)};const [a,b]=[1,2,3];globalThis.R=S([a,b,L])");
B("const orig=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function*(){L.push('patched');yield 'p'};const r=[[...[1,2]],(([a])=>a)([5]),Array.from([9])];Array.prototype[Symbol.iterator]=orig;globalThis.R=S([r,L])");
B("const orig=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function*(){L.push('patched');yield 'p'};const r=[Math.max(...[1,2]),new Set([1,2]).size,[...new Map([[1,2]])]];Array.prototype[Symbol.iterator]=orig;globalThis.R=S([r,L])");
B("const orig=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function*(){L.push('patched');yield 'p'};function f(...r){return r}const q=f(1,2);const[w]=(function(){return arguments})(7);Array.prototype[Symbol.iterator]=orig;globalThis.R=S([q,w,L])");
B("const orig=Array.prototype[Symbol.iterator];delete Array.prototype[Symbol.iterator];try{[...[1]]}catch(e){L.push(F(e))}try{const[a]=[1]}catch(e){L.push(F(e))}try{for(const x of [1]);}catch(e){L.push(F(e))}Array.prototype[Symbol.iterator]=orig;globalThis.R=S(L)");
B("const [a,b]='\\ud83d\\ude00x';globalThis.R=S([a.length,b])");
B("let n=0;for(const ch of 'a\\ud83d\\ude00\\ud83d\\ude00'){n++}globalThis.R=S(n)");
B("const it=new Map([[1,2]])[Symbol.iterator]();globalThis.R=S([it.next().value,Object.prototype.toString.call(it),it[Symbol.toStringTag]])");

// ---- Métodos de Set com set-likes.
const setRecv = ["new Set([1,2,3])", "new Set([3,4,5])", "new Set()"];
const setMethods = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
const lg = "L.push";
const setLikes = {
  ok: "{size:2,has(x){L.push('has '+x);return x===1||x===9},keys(){L.push('keys');return mk('k',[1,9],{plain:true})}}",
  okset: "new Set([2,3,9])",
  okmap: "new Map([[1,'a'],[7,'b']])",
  empty: "new Set()",
  big: "{size:Infinity,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[1,2,3],{plain:true})}}",
  logall: "new Proxy({size:2,has(x){return x===1},keys(){return mk('k',[1,9],{plain:true})}},{get(t,k){L.push('get '+String(k));return t[k]}})",
  nosize: "{has(){return true},keys(){return[].values()}}",
  nan: "{size:NaN,has(){return true},keys(){return[].values()}}",
  neg: "{size:-1,has(){return true},keys(){return[].values()}}",
  strsize: "{size:'2',has(){return true},keys(){return[].values()}}",
  objsize: "{size:{valueOf(){L.push('valueOf');return 2}},has(){return true},keys(){return[].values()}}",
  bigsize: "{size:2n,has(){return true},keys(){return[].values()}}",
  fracsize: "{size:1.5,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[1],{plain:true})}}",
  negzero: "{size:-0,has(){return false},keys(){return[].values()}}",
  nohas: "{size:1,keys(){return[].values()}}",
  hasnotfn: "{size:1,has:5,keys(){return[].values()}}",
  nokeys: "{size:1,has(){return true}}",
  keysnotfn: "{size:1,has(){return true},keys:'k'}",
  keysnoiter: "{size:1,has(){return true},keys(){return 5}}",
  keysnonext: "{size:1,has(){return true},keys(){return{}}}",
  keysnextnotfn: "{size:1,has(){return true},keys(){return{next:5}}}",
  keysnextbad: "{size:1,has(){return true},keys(){return{next(){return 5}}}}",
  keysarray: "{size:1,has(){return true},keys(){return[1]}}",
  keysthrow: "{size:2,has(x){L.push('has '+x);return false},keys(){L.push('keys');return mk('k',[1,2,3],{plain:true,throwAt:1})}}",
  hasthrow: "{size:2,has(x){L.push('has '+x);throw new Error('has-boom')},keys(){return mk('k',[1],{plain:true})}}",
  keysrt: "{size:3,has(x){L.push('has '+x);return x===1},keys(){L.push('keys');return mk('k',[1,2,3],{plain:true})}}",
  keysdup: "{size:3,has(){return true},keys(){return mk('k',[1,1,2,2],{plain:true})}}",
  hasbool: "{size:2,has(x){L.push('has '+x);return x===1?'yes':0},keys(){return mk('k',[1],{plain:true})}}",
  arr: "[1,2]", str: "'ab'", num: "5", undef: "undefined", nul: "null", fn: "function(){}", sym: "Symbol()",
  getsizethrow: "{get size(){throw new Error('size-boom')},has(){return true},keys(){return[].values()}}",
  gethasthrow: "{size:1,get has(){throw new Error('has-get-boom')},keys(){return[].values()}}",
  getkeysthrow: "{size:1,has(){return true},get keys(){throw new Error('keys-get-boom')}}",
  mutate: "(()=>{const s=new Set([1,2,3]);return{size:3,has(x){return s.has(x)},keys(){return s.keys()}}})()",
  mutrecv: "{size:3,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,2,1],{plain:true})}}",
  sizeorder: "{get size(){L.push('size');return 1},get has(){L.push('has');return()=>false},get keys(){L.push('keys');return()=>[].values()}}",
  sizeorder2: "{get size(){L.push('size');return NaN},get has(){L.push('has');return()=>false},get keys(){L.push('keys');return()=>[].values()}}",
  sizeorder3: "{get size(){L.push('size');return 1},get has(){L.push('has');return 5},get keys(){L.push('keys');return()=>[].values()}}",
  mapsub: "(()=>{class M extends Map{get size(){L.push('size');return super.size}has(k){L.push('has '+k);return super.has(k)}keys(){L.push('keys');return super.keys()}}return new M([[1,1],[5,5]])})()",
  setsub: "(()=>{class M extends Set{get size(){L.push('size');return super.size}has(k){L.push('has '+k);return super.has(k)}keys(){L.push('keys');return super.keys()}}return new M([1,5])})()",
};
for (const r of setRecv) {
  for (const m of setMethods) {
    for (const [ln, l] of Object.entries(setLikes)) {
      if (r !== "new Set([1,2,3])" && !["ok", "keysthrow", "mapsub"].includes(ln)) continue;
      T(`(${r}).${m}(${l})`);
    }
  }
}
for (const m of setMethods) {
  T(`Set.prototype.${m}.call(5,new Set())`);
  T(`Set.prototype.${m}.call(new Map(),new Set())`);
  T(`Set.prototype.${m}.call([],new Set())`);
  T(`Set.prototype.${m}.call({},new Set())`);
  T(`Set.prototype.${m}.call(undefined,new Set())`);
  T(`Set.prototype.${m}.call(new Set())`);
  T(`Set.prototype.${m}.call(new Set([1]),new Set([1]),new Set([2]))`);
  T(`[Set.prototype.${m}.name,Set.prototype.${m}.length,Object.getOwnPropertyDescriptor(Set.prototype,'${m}').enumerable]`);
  T(`new Set([1,2,3]).${m}(new Set([2])).constructor===Set`);
  T(`(class X extends Set{}).prototype.${m}.call(new (class X extends Set{})([1]),new Set([1])).constructor.name`);
  T(`(()=>{class X extends Set{static get [Symbol.species](){L.push('species');return Set}};const s=new X([1,2]);return[s.${m}(new Set([2])) instanceof X,s.${m}(new Set([2])).constructor.name]})()`);
  T(`(()=>{const s=new Set([1,2]);s.constructor=function(){L.push('ctor');return new Set()};return s.${m}(new Set([2]))})()`);
  T(`new Set([1,2,3]).${m}(new Set([3,2,1]))`);
  T(`new Set([-0,0,NaN]).${m}(new Set([0,NaN]))`);
  T(`new Set([-0]).${m}(new Set([0]))`);
  T(`new Set([0]).${m}({size:1,has(){return true},keys(){return[-0].values()}})`);
  T(`new Set([1,2,3]).${m}(new Map([[1,'x'],[4,'y']]))`);
}
B("const s=new Set([1,2,3]);const r=s.union({size:1,has(){return true},keys(){s.add(99);return[4].values()}});globalThis.R=S([r,s])");
B("const s=new Set([1,2,3]);const r=s.intersection({size:5,has(x){s.delete(2);return true},keys(){return[].values()}});globalThis.R=S([r,s])");
B("const s=new Set([1,2,3]);const r=s.difference({size:5,has(x){L.push(x);s.delete(3);return false},keys(){return[].values()}});globalThis.R=S([r,s,L])");
B("const s=new Set([1,2,3]);const r=s.isSubsetOf({size:5,has(x){L.push(x);s.clear();return true},keys(){return[].values()}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.isSupersetOf({size:1,has(){return true},keys(){return mk('k',[1,2,3],{plain:true,returnValue:undefined})}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.isSupersetOf({size:1,has(){return true},keys(){return mk('k',[9,2,3],{plain:true})}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.isDisjointFrom({size:9,has(){return true},keys(){return mk('k',[7,8,1,2],{plain:true})}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.isDisjointFrom({size:1,has(x){L.push('has '+x);return x===3},keys(){return mk('k',[7],{plain:true})}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.isSubsetOf({size:2,has(x){L.push('has '+x);return true},keys(){return[].values()}});globalThis.R=S([r,L])");
B("const s=new Set([1,2,3]);const r=s.intersection({size:2,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,1,3,5],{plain:true})}});globalThis.R=S([[...r],L])");
B("const s=new Set([1,2,3]);const r=s.intersection({size:3,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,1,3,5],{plain:true})}});globalThis.R=S([[...r],L])");
B("const s=new Set([1,2,3]);const r=s.difference({size:2,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,1,5],{plain:true})}});globalThis.R=S([[...r],L])");
B("const s=new Set([1,2,3]);const r=s.difference({size:3,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,1,5],{plain:true})}});globalThis.R=S([[...r],L])");
B("const s=new Set([1,2,3]);const r=s.symmetricDifference({size:3,has(x){L.push('has '+x);return true},keys(){L.push('keys');return mk('k',[3,4,4,1],{plain:true})}});globalThis.R=S([[...r],L])");
B("const a=new Set([3,1,2]),b=new Set([2,5,3]);globalThis.R=S([[...a.union(b)],[...a.intersection(b)],[...b.intersection(a)],[...a.difference(b)],[...a.symmetricDifference(b)],[...b.symmetricDifference(a)]])");
B("const big=new Set([1,2,3,4,5]),small=new Set([5,1]);globalThis.R=S([[...big.intersection(small)],[...small.intersection(big)]])");
B("const s=new Set([1,2,3]);const u=s.union(s);globalThis.R=S([u===s,[...u],s.union(new Set())!==s])");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "iterator-golden-"));
const file = path.join(dir, "iterator_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const original of programs) {
  // O programa gravado é o fonte já transpilado pelo bun (as mensagens de erro citam o mesmo texto no porte); o que o bun
  // executa é `executableSource(original)`, para as posições do stack saírem no fonte original. `meta` leva o modo e o
  // mapa de posições (quinta coluna do tsv, ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
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
process.stdout.write(emitFactored("iterator", rows));
fs.rmSync(dir, { recursive: true, force: true });
