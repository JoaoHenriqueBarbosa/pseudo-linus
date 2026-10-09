// Gera tests/golden/generator_bun.tsv: ciclo de vida de geradores síncronos, medido no bun 1.4.2.
// Cobre a matriz corpo x sequência de next/return/throw (try/catch/finally, yield no finally, return e throw no
// finally), yield* contra iteradores manuais com e sem return/throw, reentrância ("already running"), protótipos e
// descritores, spread e destructuring com fechamento, this/arguments/new, parâmetros e o estado depois de concluir.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-generator-golden.js > tests/golden/generator_bun.tsv
const fs = require("fs");
const { emitRow, writeResultPreload, decodeResult } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(v instanceof Error)return v.name+": "+v.message;' +
  'if(Array.isArray(v))return "["+v.map(x=>S(x,d+1)).join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  // RUN aplica as operações ("n", "n:v", "r", "r:v", "t:v") ao gerador e junta resultados e o log do corpo.
  'var LOG=[];function L(x){LOG.push(String(x))}\n' +
  'function RUN(mk,ops){LOG=[];var g=mk();var out=[];for(var op of ops){var a=op.split(":");var r;try{var x=a[0]==="n"?g.next(a[1]):a[0]==="r"?g.return(a[1]):g.throw(a[1]);r=x.done+"/"+S(x.value)}catch(e){r="E "+S(e)}out.push(op+"="+r)}return out.join(" ")+" | "+LOG.join(",")}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. Matriz de corpos x sequências de operações.
const bodies = {
  plain: "function*(){L('s');var x=yield 1;L('x='+x);var y=yield 2;L('y='+y);return 3}",
  tryCatch: "function*(){try{L('s');var x=yield 1;L('x='+x);yield 2}catch(e){L('c='+e);yield 'c'}L('after');return 4}",
  tryFinally: "function*(){try{L('s');yield 1;yield 2}finally{L('f')}L('after');return 5}",
  yieldInFinally: "function*(){try{yield 1}finally{L('f1');yield 'f';L('f2')}L('after');return 6}",
  returnInFinally: "function*(){try{yield 1;yield 2}finally{L('f');return 'over'}}",
  throwInFinally: "function*(){try{yield 1;yield 2}finally{L('f');throw new Error('fin')}}",
  nested: "function*(){try{try{yield 1;yield 2}finally{L('in');yield 'i'}}finally{L('out');yield 'o'}return 7}",
  catchFinally: "function*(){try{yield 1}catch(e){L('c');yield 'c'}finally{L('f');yield 'f'}return 8}",
  loop: "function*(){for(var i=0;i<3;i++){try{yield i}finally{L('f'+i)}}return 'end'}",
  forOf: "function*(){for(var v of [10,20,30]){L('v'+v);yield v}return 9}",
};
const opsList = ["n", "n:a", "r", "r:z", "t:E"];
const seqs = [];
for (const a of opsList) {
  seqs.push([a]);
  for (const b of opsList) {
    seqs.push([a, b]);
    for (const c of opsList) if (a === "n" || b === "n") seqs.push([a, b, c]);
  }
}
for (const [name, body] of Object.entries(bodies)) {
  for (const s of seqs) add(`T(()=>RUN(${body},${JSON.stringify(s)}))`);
}

// ---- 2. yield* contra iteradores manuais.
const inners = {
  gen: "()=>(function*(){try{L('i0');var a=yield 'a';L('a='+a);yield 'b'}finally{L('ifin')}return 'ir'})()",
  nextOnly: "()=>({[Symbol.iterator](){var n=0;return{next(v){L('nx'+v);return{value:'p'+n,done:++n>2}}}}})",
  withReturn: "()=>({[Symbol.iterator](){var n=0;return{next(v){return{value:'p'+n,done:++n>2}},return(v){L('ret'+v);return{value:'rv',done:true}}}}})",
  returnNonObject: "()=>({[Symbol.iterator](){return{next(){return{value:1,done:false}},return(v){L('ret');return 5}}}})",
  returnUndefined: "()=>({[Symbol.iterator](){return{next(){return{value:1,done:false}},return(v){L('ret')}}}})",
  withThrow: "()=>({[Symbol.iterator](){return{next(){return{value:1,done:false}},throw(e){L('thr'+e);return{value:'tv',done:true}},return(){L('ret');return{}}}}})",
  throwNonObject: "()=>({[Symbol.iterator](){return{next(){return{value:1,done:false}},throw(e){L('thr');return 1}}}})",
  noThrowWithReturn: "()=>({[Symbol.iterator](){return{next(){return{value:1,done:false}},return(){L('ret');return{value:0,done:true}}}}})",
  nextNonObject: "()=>({[Symbol.iterator](){return{next(){return 1}}}})",
  nextThrows: "()=>({[Symbol.iterator](){return{next(){throw new RangeError('nt')}}}})",
  doneGetter: "()=>({[Symbol.iterator](){return{next(){return{get done(){L('d');return false},get value(){L('v');return 7}}}}}})",
  array: "()=>[1,2]",
  string: "()=>'xy'",
  notIterable: "()=>({})",
  nullIter: "()=>null",
};
const outerOps = [["n"], ["n", "n"], ["n", "n", "n"], ["n", "n", "n", "n"], ["n:q", "n:w"], ["n", "r:z"], ["r:z"], ["n", "t:E"], ["t:E"], ["n", "r:z", "n"], ["n", "t:E", "n"], ["n", "n", "r:z"], ["n", "n", "t:E"]];
for (const [name, mk] of Object.entries(inners)) {
  for (const ops of outerOps) {
    add(`T(()=>{var mk=${mk};return RUN(function*(){var r=yield* mk();L('r='+S(r));return 'o'},${JSON.stringify(ops)})})`);
  }
}
// yield* com try/finally e catch no externo
for (const name of ["gen", "withReturn", "withThrow", "nextOnly"]) {
  for (const ops of [["n", "r:z"], ["n", "t:E"], ["n", "t:E", "n"], ["n", "r:z", "n"], ["n", "n", "n", "n"]]) {
    add(`T(()=>{var mk=${inners[name]};return RUN(function*(){try{var r=yield* mk();L('r='+S(r))}catch(e){L('c='+S(e));yield 'c'}finally{L('of')}return 'o'},${JSON.stringify(ops)})})`);
    add(`T(()=>{var mk=${inners[name]};return RUN(function*(){try{yield* mk()}finally{L('of');yield 'F'}return 'o'},${JSON.stringify(ops)})})`);
  }
}

// ---- 3. Reentrância e estado terminal.
add(
  "T(()=>{var g;function* f(){g.next()}g=f();return g.next()})",
  "T(()=>{var g;function* f(){yield g.next()}g=f();return g.next()})",
  "T(()=>{var g;function* f(){g.return(1)}g=f();return g.next()})",
  "T(()=>{var g;function* f(){g.throw(1)}g=f();return g.next()})",
  "T(()=>{var g;function* f(){try{g.next()}catch(e){yield e.message}}g=f();return g.next().value})",
  "T(()=>{var g;function* f(){try{g.next()}catch(e){yield e.constructor===TypeError}}g=f();return g.next().value})",
  "T(()=>{function* f(){yield 1}var g=f();g.next();g.next();return [g.next(),g.return(5),g.next()]})",
  "T(()=>{function* f(){yield 1}var g=f();return [g.return(5),g.next(),g.return(6)]})",
  "T(()=>{function* f(){yield 1}var g=f();try{g.throw(new Error('x'))}catch(e){}return g.next()})",
  "T(()=>{function* f(){yield 1}var g=f();return g.throw(7)})",
  "T(()=>{function* f(){yield 1}var g=f();g.next();g.next();return g.throw(7)})",
  "T(()=>{function* f(){throw 1}var g=f();try{g.next()}catch(e){}return [g.next(),g.return(2)]})",
  "T(()=>{function* f(){L('start');yield 1}var g=f();LOG=[];g.return(1);g.next();return LOG.length})",
  "T(()=>{function* f(){L('start');yield 1}var g=f();LOG=[];try{g.throw(1)}catch(e){}return LOG.length})",
  "T(()=>{function* f(){L('body')}LOG=[];var g=f();return LOG.length})",
  "T(()=>{function* f(a=L('param')){L('body')}LOG=[];var g=f();return LOG.join()})",
  "T(()=>{function* f(a=(()=>{throw new RangeError('p')})()){}return f()})",
  "T(()=>eval('(function*(a=yield){})'))",
  "T(()=>eval('(function*(){var yield})'))",
  "T(()=>eval('(function*(yield){})'))",
  "T(()=>eval('function*f(){function yield(){}}'))",
  "T(()=>eval('(function*(){yield\\n*2})'))",
  "T(()=>eval('(function*(){yield\\n1})'))",
  "T(()=>eval('(function*(){yield = 1})'))",
  "T(()=>eval('(function*(){(yield) => 1})'))",
  "T(()=>eval('(function*(){var f=()=>yield 1})'))",
  "T(()=>eval('(function*(){yield yield 1})'))",
  "T(()=>eval('(function*(){yield *\\n[]})'))",
  "T(()=>eval('(function*(){1+yield})'))",
  "T(()=>eval('(function*(){1+(yield)})'))",
  "T(()=>eval('(function*(){yield ? 1 : 2})'))",
  "T(()=>eval('(function*(){return yield})'))",
  "T(()=>eval('(function*(){var x=yield,y=1})'))",
  "T(()=>eval('(function*(){[yield]})'))",
  "T(()=>eval('(function*(){({a:yield})})'))",
  "T(()=>eval('(function*(){`${yield}`})'))",
  "T(()=>eval('(function*(){yield\\n++x})'))",
  "T(()=>eval('(function*(){new yield})'))",
  "T(()=>eval('(function*(){yield => 1})'))",
  "T(()=>eval('(function*(){class C extends (yield){}})'))",
  "T(()=>eval('(function*(){class yield{}})'))",
  "T(()=>eval('function*g(){} g'))",
);
for (const kw of ["yield", "yield 1", "yield*[]", "(yield)", "await 1"]) {
  for (const ctx of ["function*(){%}", "function*(){ function h(){%} }", "function*(){ ()=>% }", "async function*(){%}", "function(){%}"]) {
    add(`T(()=>{var f=eval(${JSON.stringify("(" + ctx.replace("%", kw) + ")")});return typeof f})`);
  }
}

// ---- 4. Protótipos, descritores, construção.
add(
  "T(()=>{function* f(){}return [Object.getPrototypeOf(f)===Object.getPrototypeOf(function*(){}), Object.getPrototypeOf(f).constructor.name]})",
  "T(()=>{function* f(){}return [typeof f.prototype, Object.getPrototypeOf(f.prototype)===Object.getPrototypeOf(function*(){}).prototype]})",
  "T(()=>{function* f(){}return D(f,'prototype')})",
  "T(()=>{function* f(){}return Reflect.ownKeys(f)})",
  "T(()=>{function* f(){}return Reflect.ownKeys(f.prototype)})",
  "T(()=>{function* f(){}return D(f,'name')+'|'+D(f,'length')})",
  "T(()=>{function* f(a,b=1,c){}return f.length})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){});return [D(GF,'prototype'),D(GF,Symbol.toStringTag),GF[Symbol.toStringTag]]})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return [Reflect.ownKeys(GP),D(GP,Symbol.toStringTag),D(GP,'constructor')]})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return [D(GP,'next'),D(GP,'return'),D(GP,'throw')]})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return [GP.next.length,GP.return.length,GP.throw.length,GP.next.name,GP.return.name,GP.throw.name]})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return Object.getPrototypeOf(GP)===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))})",
  "T(()=>{function* f(){}var g=f();return [Object.prototype.toString.call(g),String(g),g[Symbol.iterator]()===g]})",
  "T(()=>{function* f(){}var g=f();return [Object.getPrototypeOf(g)===f.prototype,g instanceof f]})",
  "T(()=>{function* f(){}f.prototype=null;var g=f();return [Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){}).prototype]})",
  "T(()=>{function* f(){}f.prototype=5;var g=f();return [Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){}).prototype]})",
  "T(()=>{function* f(){}f.prototype={next(){return 'mine'}};var g=f();return g.next()})",
  "T(()=>{function* f(){}return new f()})",
  "T(()=>{function* f(){}return Reflect.construct(f,[])})",
  "T(()=>{var o={*m(){}};return new o.m()})",
  "T(()=>{var o={*m(){}};return [D(o.m,'prototype'),o.m.name,'prototype' in o.m]})",
  "T(()=>{class C{*m(){}static *s(){}}return [typeof C.prototype.m.prototype,C.s.name,new C.s()]})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;var f=GF('a','yield a;yield a*2');return [f.name,f.toString(),[...f(2)]]})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return [GF.name,GF.length,Object.getPrototypeOf(GF).name]})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return GF('yield 1','yield')})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return GF('a=yield','')})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return new GF('yield 1').toString()})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.next.call({})})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.next.call(undefined)})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.return.call(1)})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.throw.call([])})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.next.call((async function*(){})())})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.next.call([][Symbol.iterator]())})",
  "T(()=>{var GP=Object.getPrototypeOf(function*(){}).prototype;return GP.next.call(Object.create((function*(){})()))})",
  "T(()=>{function* f(){yield this}var g=f.call(5);return typeof g.next().value})",
  "T(()=>{function* f(){yield this}var g=f.call(undefined);return g.next().value===undefined})",
  "T(()=>{function* f(){'use strict';yield this}var g=f.call(5);return typeof g.next().value})",
  "T(()=>{var o={*m(){yield this}};return o.m().next().value===o})",
  "T(()=>{function* f(){yield arguments.length;yield arguments[1]}var g=f(1,2,3);return [g.next().value,g.next().value]})",
  "T(()=>{function* f(a){arguments[0]=9;yield a}return f(1).next().value})",
  "T(()=>{function* f(a){'use strict';arguments[0]=9;yield a}return f(1).next().value})",
  "T(()=>{function* f(){yield new.target}return f().next().value})",
  "T(()=>{function* f(){yield typeof new.target}return f().next().value})",
  "T(()=>{function* f(){return f}return f().next().value===f})",
  "T(()=>{var f=function* named(){yield named};return f().next().value===f})",
  "T(()=>{var o={*[Symbol.iterator](){yield 1;yield 2}};return [...o]})",
  "T(()=>{var o={*['a'+'b'](){}};return o.ab.name})",
  "T(()=>{var o={*[Symbol('s')](){}};return Object.getOwnPropertySymbols(o).map(s=>o[s].name)})",
  "T(()=>{class C{static*[Symbol.iterator](){yield 1}}return [...C]})",
  "T(()=>{class C extends (function*(){}){}return 1})",
  "T(()=>{class C extends Object.getPrototypeOf(function*(){}).constructor{}return typeof new C('yield 1')})",
);

// ---- 5. Consumidores: spread, destructuring, for-of, Array.from, Map/Set, Promise.all sobre geradores.
const gens = {
  g3: "function*(){try{yield 1;yield 2;yield 3}finally{L('closed')}}",
  gThrow: "function*(){try{yield 1;throw new Error('boom')}finally{L('closed')}}",
  gRet: "function*(){yield 1;return 'ret'}",
  gEmpty: "function*(){}",
};
const consumers = [
  "[...g()]", "Array.from(g())", "Array.from(g(),x=>x*2)", "new Set(g()).size", "new Map((function*(){for(var v of g())yield [v,v]})()).size",
  "Math.max(...g())", "(()=>{var [a]=g();return a})()", "(()=>{var [a,b]=g();return [a,b]})()", "(()=>{var [a,...r]=g();return [a,r]})()",
  "(()=>{var [,,,,d]=g();return d})()", "(()=>{var [a]=[];[a]=g();return a})()", "(()=>{var r=[];for(var v of g()){r.push(v);if(v==1)break}return r})()",
  "(()=>{var r=[];for(var v of g()){r.push(v);if(v==1)continue}return r})()", "(()=>{var r=[];for(var v of g()){r.push(v);throw 1}})()",
  "(()=>{var r=[];a:for(var v of g()){for(var w of g()){r.push(v+':'+w);continue a}}return r})()", "(()=>{for(var v of g()){return v}})()",
  "Object.fromEntries((function*(){for(var v of g())yield ['k'+v,v]})())", "Array.prototype.concat.call([],g())",
  "[].concat(...g())", "String.fromCharCode(...g())", "Reflect.apply(Math.min,null,[...g()])", "(()=>{var it=g();it.next();return [...it]})()",
  "(()=>{var it=g();it.next();return Array.from(it)})()", "(()=>{var it=g();for(var x of it){break}return [...it]})()",
  "(()=>{var it=g();var [a]=it;return [a,...it]})()", "(()=>{var it=g();var [a,b,c,d]=it;return [a,b,c,d]})()",
  "(()=>{var it=g();return [it.next(),it.next(),it.next(),it.next(),it.next()].map(r=>r.done+':'+r.value)})()",
  "g().map(x=>x*2).toArray()", "g().filter(x=>x>1).toArray()", "g().take(2).toArray()", "g().drop(1).toArray()", "g().flatMap(x=>[x,x]).toArray()",
  "g().reduce((a,b)=>a+(b||0),0)", "g().some(x=>x==2)", "g().every(x=>x<3)", "g().find(x=>x==2)", "g().forEach(x=>L('e'+x))",
  "Iterator.from(g()).toArray()", "g().take(0).toArray()", "g().take(1).toArray()", "g().drop(5).toArray()",
];
for (const [name, body] of Object.entries(gens)) {
  for (const c of consumers) add(`T(()=>{var g=${body};LOG=[];var r;try{r=S(${c})}catch(e){r='E '+e.name+': '+e.message}return r+' | '+LOG.join()})`);
}

// ---- 6. yield como expressão: precedência, valores, ordem de avaliação.
add(
  "T(()=>RUN(function*(){var x=(yield 1)+(yield 2);L('x='+x);return x},['n','n:3','n:4']))",
  "T(()=>RUN(function*(){var o={[yield 'k']:yield 'v'};L(S(o))},['n','n:key','n:val']))",
  "T(()=>RUN(function*(){var a=[yield 1,yield 2];L(S(a))},['n','n:x','n:y']))",
  "T(()=>RUN(function*(){L(`${yield 1}-${yield 2}`)},['n','n:x','n:y']))",
  "T(()=>RUN(function*(){var f=yield 'f';L(f(yield 'a'))},['n','n:String','n:77']))",
  "T(()=>RUN(function*(){var o={};o[yield 'k']=yield 'v';L(S(o))},['n','n:p','n:q']))",
  "T(()=>RUN(function*(){var {a=yield 'd'}={};L(a)},['n','n:def']))",
  "T(()=>RUN(function*(){var [a=yield 'd']=[];L(a)},['n','n:def']))",
  "T(()=>RUN(function*(){var [a]=yield 'it';L(a)},['n','n:xyz']))",
  "T(()=>RUN(function*(){L((yield 1)?'t':'f')},['n','n:0']))",
  "T(()=>RUN(function*(){L((yield 1)||(yield 2))},['n','n:','n:z']))",
  "T(()=>RUN(function*(){L((yield 1)&&(yield 2))},['n','n:a','n:z']))",
  "T(()=>RUN(function*(){L((yield 1)??(yield 2))},['n','n:a']))",
  "T(()=>RUN(function*(){var x=0;x+=yield 1;L(x)},['n','n:5']))",
  "T(()=>RUN(function*(){var x=10;x+=(yield 1)+(yield 2);L(x)},['n','n:5','n:6']))",
  "T(()=>RUN(function*(){var x=1;var r=x+(yield 1);L(r)},['n','n:5']))",
  "T(()=>RUN(function*(){var x=1;var r=x+(yield (x=100));L(r)},['n','n:5']))",
  "T(()=>RUN(function*(){var o={get a(){L('ga');return 1}};var r=o.a+(yield 1);L(r)},['n','n:2']))",
  "T(()=>RUN(function*(){try{yield 1}finally{L('f')}},['n','r:q']))",
  "T(()=>RUN(function*(){yield yield 1},['n','n:a','n:b','n']))",
  "T(()=>RUN(function*(){yield* yield* [1]},['n','n']))",
  "T(()=>RUN(function*(){var r=yield* (function*(){return 'x'})();L(r);return r},['n']))",
  "T(()=>RUN(function*(){yield* [1,2];L('end')},['n','n','n']))",
  "T(()=>RUN(function*(){yield* 'ab'},['n','n','n']))",
  "T(()=>RUN(function*(){yield* new Set([1,2])},['n','n','n']))",
  "T(()=>RUN(function*(){yield* new Map([[1,2]])},['n','n']))",
  "T(()=>RUN(function*(){yield* arguments},['n','n']))",
  "T(()=>RUN(function*(){yield* 5},['n']))",
  "T(()=>RUN(function*(){yield* undefined},['n']))",
  "T(()=>RUN(function*(){yield* {}},['n']))",
  "T(()=>RUN(function*(){yield* {[Symbol.iterator](){return 1}}},['n']))",
  "T(()=>RUN(function*(){yield* {[Symbol.iterator]:1}},['n']))",
  "T(()=>RUN(function*(){yield* {get [Symbol.iterator](){L('get');return function(){return[][Symbol.iterator]()}}}},['n']))",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return this},next(){log.push('next');return{done:true,value:'v'}}};function*f(){var r=yield* it;log.push('r='+r)}f().next();return log.join()})",
  "T(()=>{var it={[Symbol.iterator](){return this},next(){return{done:false,value:{get x(){return 1}}}}};function*f(){yield* it}var r=f().next();return typeof r.value})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return this},next(){return{get done(){log.push('done');return false},get value(){log.push('value');return 1}}}};function*f(){yield* it}var r=f().next();return log.join()+'|'+S(r)})",
  "T(()=>{var it={[Symbol.iterator](){return this},next(){return{done:false,value:1}}};function*f(){yield* it}var r=f().next();return S(r)})",
  "T(()=>{function*f(){yield 1;yield 2}var g=f();var r=g.next();return Object.keys(r).join()+'|'+Reflect.ownKeys(r).join()})",
  "T(()=>{function*f(){return 1}var r=f().next();return Object.keys(r).join()+'|'+Object.getPrototypeOf(r)===Object.prototype})",
  "T(()=>{function*f(){yield 1}var g=f();return [g.return().value,g.return().done]})",
  "T(()=>{function*f(){yield 1}var g=f();g.next();return S(g.return({a:1}))})",
  "T(()=>{function*f(){try{yield 1}finally{yield 2}}var g=f();g.next();var r1=g.return(9);var r2=g.next();return S([r1,r2])})",
  "T(()=>{function*f(){try{yield 1}finally{return 'w'}}var g=f();g.next();return S(g.return(9))})",
  "T(()=>{function*f(){try{yield 1}finally{throw 'x'}}var g=f();g.next();try{g.return(9)}catch(e){return e}})",
  "T(()=>{function*f(){try{yield 1}catch(e){return 'caught '+e}}var g=f();g.next();return S(g.throw('boom'))})",
  "T(()=>{function*f(){try{yield 1}catch(e){yield 'c'}return 'r'}var g=f();g.next();return S([g.throw(1),g.next(),g.next()])})",
);

// ---- 7. Geradores aninhados, recursão e laços com closures.
add(
  "T(()=>{function*walk(t){if(!t)return;yield* walk(t.l);yield t.v;yield* walk(t.r)}var t={v:2,l:{v:1},r:{v:3,r:{v:4}}};return [...walk(t)]})",
  "T(()=>{function*fib(){var a=0,b=1;for(;;){yield a;[a,b]=[b,a+b]}}var r=[];for(var v of fib()){if(v>50)break;r.push(v)}return r})",
  "T(()=>{function*cnt(n){for(var i=0;i<n;i++)yield ()=>i}return [...cnt(3)].map(f=>f())})",
  "T(()=>{function*cnt(n){for(let i=0;i<n;i++)yield ()=>i}return [...cnt(3)].map(f=>f())})",
  "T(()=>{function*f(){var x=1;yield ()=>x;x=2;yield ()=>x}var g=f();var a=g.next().value;var b=g.next().value;return [a(),b()]})",
  "T(()=>{function*inner(){try{yield 1;yield 2}finally{L('i')}}function*outer(){try{yield* inner();yield 3}finally{L('o')}}LOG=[];for(var v of outer()){break}return LOG.join()})",
  "T(()=>{function*inner(){try{yield 1}finally{L('i');throw new Error('ie')}}function*outer(){try{yield* inner()}finally{L('o')}}LOG=[];var g=outer();g.next();try{g.return(1)}catch(e){L(e.message)}return LOG.join()})",
  "T(()=>{function*a(){yield* b()}function*b(){yield* c()}function*c(){yield 'deep';return 'cr'}var g=a();return [S(g.next()),S(g.next())]})",
  "T(()=>{function*a(){var r=yield* b();L('b->'+r);return 'a'}function*b(){var r=yield* c();L('c->'+r);return 'b'}function*c(){return 'c'}LOG=[];var r=a().next();return S(r)+LOG.join()})",
  "T(()=>{var order=[];function*f(){order.push(1);yield;order.push(2);yield;order.push(3)}var g=f();order.push('a');g.next();order.push('b');g.next();order.push('c');g.next();return order.join()})",
  "T(()=>{var g1=(function*(){yield 1;yield 2})(),g2=(function*(){yield 'a';yield 'b'})();var r=[];for(var i=0;i<2;i++)r.push(g1.next().value,g2.next().value);return r})",
  "T(()=>{function*f(){var x=yield 1;var y=yield x*2;return x+y}var g=f();g.next();var a=g.next(10);var b=g.next(5);return S([a,b])})",
  "T(()=>{function*f(){while(true){var c=yield;if(c==='stop')return 'done';L(c)}}LOG=[];var g=f();g.next();g.next('a');g.next('b');return S(g.next('stop'))+LOG.join()})",
  "T(()=>{function*f(){var i=0;while(true){var r=yield i++;if(r)i=0}}var g=f();return [g.next().value,g.next().value,g.next(true).value,g.next().value]})",
  "T(()=>{function*f(){yield 1;yield 2}var g=f();return [...g].length+[...g].length})",
  "T(()=>{var g=(function*(){yield 1;yield 2;yield 3})();var [a]=g;return [a,g.next()].map(S)})",
  "T(()=>{function*f(){try{yield 1;yield 2}finally{L('f')}}var g=f();LOG=[];var [a,b]=g;return [a,b,LOG.join()]})",
  "T(()=>{function*f(){try{yield 1}finally{L('f')}}var g=f();LOG=[];var [a,b]=g;return [a,b,LOG.join()]})",
  "T(()=>{function*f(){yield 1}var a=f(),b=f();return [a===b,a.next===b.next]})",
  "T(()=>{var seen=[];function*f(){seen.push(this===undefined)}f.call(undefined).next();return seen})",
  "T(()=>{'use strict';var seen=[];function*f(){seen.push(this===undefined)}f.call(undefined).next();return seen})",
  "T(()=>{function*f(){yield 1}return f.call.call(f)})",
  "T(()=>{function*f(){yield 1}return Function.prototype.call.call(f).next()})",
  "T(()=>{function*f(a,b){yield a+b}return f.apply(null,[1,2]).next().value})",
  "T(()=>{function*f(a,b){yield a+b}return f.bind(null,1)(2).next().value})",
  "T(()=>{function*f(){yield 1}var b=f.bind(null);return [typeof b.prototype,b().next().value]})",
  "T(()=>{function*f(){yield 1}var b=f.bind(null);try{new b()}catch(e){return e.message}})",
  "T(()=>{function*f(){}return Reflect.construct(function(){},[],f).constructor===undefined})",
  "T(()=>{function*f(){}return Reflect.construct(Object,[],f)})",
  "T(()=>{function*f(){}try{return Reflect.construct(f,[])}catch(e){return e.message}})",
  "T(()=>{function*f(){}try{return new f}catch(e){return e.message}})",
  "T(()=>{function*f(){}try{return class extends f{}}catch(e){return e.message}})",
  "T(()=>{function*f(){}return typeof (class extends Object.getPrototypeOf(f){})})",
  "T(()=>{function*f(){}return f instanceof Function && !(f instanceof Object.getPrototypeOf(f).constructor)})",
  "T(()=>{function*f(){}return [f.constructor.name,f.constructor===Function]})",
  "T(()=>{async function*f(){}function*g(){}return [Object.getPrototypeOf(f)===Object.getPrototypeOf(g)]})",
  "T(()=>{var f=function*(){};return [f.name,(function*(){}).name,(()=>function*(){})().name]})",
  "T(()=>{var o={f:function*(){}};return o.f.name})",
  "T(()=>{var o={*f(){}};return [D(o,'f'),o.f.toString()]})",
  "T(()=>{function*f ( a ) { yield a }return f.toString()})",
  "T(()=>{class C{*m(){}}return C.prototype.m.toString()})",
  "T(()=>{class C{static *m(){}}return C.m.toString()})",
);

// ---- Execução.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const { spawnSync } = require("child_process");
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    // Processo fresco por programa, como nos outros geradores: a reificação de tabelas estáticas depende da ordem.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
