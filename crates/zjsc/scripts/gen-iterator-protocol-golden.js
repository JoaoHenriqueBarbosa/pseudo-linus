// Gera tests/golden/iterator_protocol_bun.tsv: protocolo de geradores e iteradores nativos de borda, medido no bun 1.4.2.
// Generator.prototype.next/return/throw em todos os estados (suspendedStart, suspendedYield, executing, completed) com as
// mensagens de TypeError, yield* com iteradores sem return/throw, AsyncGenerator com fila de pedidos, helpers de
// Iterator.prototype (map/filter/take/drop/flatMap/reduce/toArray/forEach/some/every/find) com iteradores que lançam e
// `return()` observado, ArrayIterator/StringIterator/MapIterator/SetIterator/RegExpStringIterator (toString,
// Symbol.toStringTag, cadeia de protótipos, next com this errado), %IteratorPrototype%[Symbol.iterator] e fechamento de
// iterador em destructuring e for-of com break/throw.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` (texto) dentro de try/catch (`Nome: mensagem` quando lança); as microtarefas são esvaziadas
// antes da leitura. Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-iterator-protocol-golden.js > tests/golden/iterator_protocol_bun.tsv
const fs = require("fs");
const { emitRow, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Programa síncrono: o corpo é uma expressão cujo resultado vira texto (erro vira `Nome: mensagem`).
const E = expr => add(`try { R = String(${expr}) } catch (e) { R = e.name + ': ' + e.message }`);
// Programa com corpo livre que atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Programa assíncrono: o corpo é o de uma async function que devolve o texto.
const A = body => add(`(async () => { ${body} })().then(v => { R = String(v) }, e => { R = e.name + ': ' + e.message })`);
const J = "JSON.stringify";

// Prelúdio comum: log de eventos e fábricas de iteradores observáveis.
const PRE =
  "var L=[];" +
  "function mk(n,o){o=o||{};var i=0;return{[Symbol.iterator](){return this},next(v){L.push('n');if(o.throwNext&&i==o.throwNext)throw new Error('boom');return i<n?{value:i++,done:false}:{value:'end',done:true}}," +
  "return:o.noReturn?undefined:function(v){L.push('r');if(o.throwReturn)throw new Error('rboom');return{value:v,done:true}}}}" +
  "function bare(n){var i=0;return{[Symbol.iterator](){return this},next(){L.push('n');return i<n?{value:i++,done:false}:{value:undefined,done:true}}}}";
const P = body => T(`${PRE}${body}`);

// ---- 1. Generator.prototype.next/return/throw em cada estado.
const states = {
  start: "var g=gen();",
  yielded: "var g=gen();g.next();",
  done: "var g=gen();g.next();g.next();g.next();",
  thrown: "var g=gen();g.next();try{g.throw(new Error('x'))}catch(e){}",
  returned: "var g=gen();g.next();g.return(7);",
};
const gens = {
  plain: "function* gen(){var a=yield 1;L.push('a'+a);yield 2;return 3}",
  tryfin: "function* gen(){try{yield 1;yield 2}finally{L.push('fin')}return 3}",
  trycatch: "function* gen(){try{yield 1}catch(e){L.push('c:'+e.message);yield 'caught'}return 4}",
  finyield: "function* gen(){try{yield 1}finally{yield 'f'}return 5}",
  finthrow: "function* gen(){try{yield 1}finally{throw new Error('inner')}}",
  finret: "function* gen(){try{yield 1}finally{return 'over'}}",
};
const calls = ["g.next()", "g.next(5)", "g.return(9)", "g.return()", "g.throw(new Error('t'))", "g.throw(1)"];
for (const [gn, gsrc] of Object.entries(gens)) {
  for (const [sn, ssrc] of Object.entries(states)) {
    for (const c of calls) {
      P(`${gsrc}${ssrc}var o=${c};R=${J}(o)+L.join()`);
      P(`${gsrc}${ssrc}${c};R=${J}([${c},${c},${c}])+L.join()`);
    }
  }
}

// ---- 2. "Generator is already running" e this errado.
P("function* gen(){g.next()}var g=gen();g.next();R='x'");
P("function* gen(){try{g.next()}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){try{g.return(1)}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){try{g.throw(1)}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){try{yield* g}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){try{[...g]}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){try{for(var x of g);}catch(e){yield e.name+': '+e.message}}var g=gen();R=g.next().value");
P("function* gen(){yield 1;g.next()}var g=gen();g.next();R=g.next()");
P("function* gen(){var s=g.next.bind(g);try{s()}catch(e){yield e.message}}var g=gen();R=g.next().value");
P("var o={*gen(){try{this.g.next()}catch(e){yield e.message}}};o.g=o.gen();R=o.g.next().value");
for (const m of ["next", "return", "throw"]) {
  for (const t of ["undefined", "null", "1", "'s'", "{}", "[]", "function*(){}", "(function*(){})()[Symbol.iterator]", "Object.create((function*(){})().__proto__)",
    "Symbol()", "new Proxy({},{})", "(async function*(){})()", "[][Symbol.iterator]()", "new Map().entries()"]) {
    E(`(function*(){}).prototype.__proto__[${JSON.stringify(m)}].call(${t})`);
  }
  E(`Object.getPrototypeOf(function*(){}).prototype[${JSON.stringify(m)}].length`);
  E(`Object.getPrototypeOf(function*(){}).prototype[${JSON.stringify(m)}].name`);
}
E("Object.prototype.toString.call((function*(){})())");
E("Object.prototype.toString.call(function*(){})");
E("Object.prototype.toString.call(Object.getPrototypeOf(function*(){}))");
E("Object.prototype.toString.call(Object.getPrototypeOf(function*(){}).prototype)");
E("(function*(){})()[Symbol.toStringTag]");
E("Object.getPrototypeOf(function*(){})[Symbol.toStringTag]");
E("Object.getPrototypeOf(Object.getPrototypeOf(function*(){}).prototype)===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))");
E("Object.getPrototypeOf(function*(){}).prototype.constructor===Object.getPrototypeOf(function*(){})");
E("Object.getOwnPropertyNames(Object.getPrototypeOf(function*(){}).prototype).join()");
E("typeof (function*(){})().next");
E("(function*(){})().hasOwnProperty('next')");
E("new (function*(){})");
E("Reflect.construct(function*(){},[])");
E("(function*(){}).prototype===(function*(){}).prototype");
E("Object.getPrototypeOf((function*(){})()) === (function*(){}).prototype");
E("(function*(){}).hasOwnProperty('prototype')");
E("(function*(){}).prototype.hasOwnProperty('constructor')");
E("(()=>{var f=function*(){};f.prototype=null;return Object.getPrototypeOf(f())===Object.getPrototypeOf(function*(){}).prototype})()");
E("(()=>{var f=function*(){};f.prototype=5;return Object.getPrototypeOf(f())===Object.getPrototypeOf(function*(){}).prototype})()");
E("(function*(){})()[Symbol.iterator]()!==undefined");
E("(()=>{var g=(function*(){})();return g[Symbol.iterator]()===g})()");

// ---- 3. yield* com iteradores sem return/throw e ordem de checagem.
const delegates = {
  full: "mk(3)",
  noret: "mk(3,{noReturn:true})",
  bare: "bare(3)",
  retthrow: "mk(3,{throwReturn:true})",
  nextthrow: "mk(3,{throwNext:1})",
  nullret: "(()=>{var m=mk(3);m.return=null;return m})()",
  numret: "(()=>{var m=mk(3);m.return=5;return m})()",
  nothrow: "(()=>{var m=mk(3);m.throw=undefined;return m})()",
  withthrow: "(()=>{var m=mk(3);m.throw=function(e){L.push('t');return{value:'T',done:true}};return m})()",
  throwbad: "(()=>{var m=mk(3);m.throw=function(e){L.push('t');return 1};return m})()",
  retbad: "(()=>{var m=mk(3);m.return=function(){L.push('r');return 1};return m})()",
  nextbad: "{[Symbol.iterator](){return{next(){return 1}}}}",
  nonext: "{[Symbol.iterator](){return{}}}",
  iternum: "{[Symbol.iterator](){return 1}}",
  noiter: "{}",
};
for (const [dn, d] of Object.entries(delegates)) {
  const body = `function* gen(){var r=yield* ${d};yield 'after:'+r}`;
  P(`${body}var g=gen();var out=[];try{out.push(${J}(g.next()));out.push(${J}(g.next()));out.push(${J}(g.next()))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
  P(`${body}var g=gen();var out=[];try{g.next();out.push(${J}(g.return(8)))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
  P(`${body}var g=gen();var out=[];try{g.next();out.push(${J}(g.throw(new Error('tt'))))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
  P(`${body}var g=gen();var out=[];try{out.push(${J}(g.return(2)))}catch(e){out.push(e.name+': '+e.message)}try{out.push(${J}(g.next()))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
}
E("(function*(){yield* [1,2,3]})().next().value");
E("[...(function*(){yield* 'ab'})()].join()");
E("[...(function*(){yield* new Set([1,2])})()].join()");
E("[...(function*(){var r=yield* (function*(){yield 1;return 'ret'})();yield r})()].join()");
E("(function*(){try{yield* [1]}finally{}})().return(3).value");
E("(()=>{var g=(function*(){yield* [1,2]})();g.next();try{return JSON.stringify(g.throw(new Error('z')))}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var g=(function*(){yield* [1,2]})();g.next();return JSON.stringify(g.return(4))})()");
E("(()=>{var g=(function*(){yield* (function*(){try{yield 1}finally{yield 'f'}})()})();g.next();return JSON.stringify([g.return(4),g.next()])})()");
E("(()=>{var g=(function*(){yield* (function*(){try{yield 1}catch(e){yield 'c'+e}})()})();g.next();return JSON.stringify([g.throw('E'),g.next()])})()");
E("(()=>{var seen;var it={[Symbol.iterator](){return this},next(v){seen=arguments.length+':'+v;return{done:false,value:1}}};var g=(function*(){yield* it})();g.next('a');g.next('b');return seen})()");
E("(()=>{var seen=[];var it={[Symbol.iterator](){return this},next(){return{done:false,get value(){seen.push('v');return 1},get done2(){return 0}}}};var g=(function*(){yield* it})();g.next();return seen.join()})()");

// ---- 4. AsyncGenerator com fila de pedidos.
const agens = {
  plain: "async function* ag(){var a=yield 1;L.push('a'+a);yield 2;return 3}",
  tryfin: "async function* ag(){try{yield 1;yield 2}finally{L.push('fin')}}",
  awaitret: "async function* ag(){try{yield 1}finally{L.push('f');await null;L.push('f2')}}",
  throws: "async function* ag(){yield 1;throw new Error('ag')}",
  yieldprom: "async function* ag(){yield Promise.resolve('p');yield Promise.reject(new Error('rej'))}",
  retprom: "async function* ag(){return Promise.resolve('rp')}",
  yieldstar: "async function* ag(){yield* [Promise.resolve('x'),'y'];yield* (async function*(){yield 'z'})()}",
};
const asyncSeq = {
  queue3: "var g=ag();var ps=[g.next('a'),g.next('b'),g.next('c'),g.next('d')];var r=[];for(var p of ps){try{r.push(J(await p))}catch(e){r.push(e.name+': '+e.message)}}return r.join('|')+'#'+L.join()",
  retfirst: "var g=ag();var ps=[g.return('R'),g.next(),g.next()];var r=[];for(var p of ps){try{r.push(J(await p))}catch(e){r.push(e.name+': '+e.message)}}return r.join('|')+'#'+L.join()",
  thrfirst: "var g=ag();var ps=[g.throw(new Error('T')),g.next()];var r=[];for(var p of ps){try{r.push(J(await p))}catch(e){r.push(e.name+': '+e.message)}}return r.join('|')+'#'+L.join()",
  retmid: "var g=ag();var ps=[g.next(),g.return('R'),g.next()];var r=[];for(var p of ps){try{r.push(J(await p))}catch(e){r.push(e.name+': '+e.message)}}return r.join('|')+'#'+L.join()",
  thrmid: "var g=ag();var ps=[g.next(),g.throw(new Error('T')),g.next()];var r=[];for(var p of ps){try{r.push(J(await p))}catch(e){r.push(e.name+': '+e.message)}}return r.join('|')+'#'+L.join()",
  retprom: "var g=ag();await g.next();var r=[];try{r.push(J(await g.return(Promise.resolve('RP'))))}catch(e){r.push(e.name+': '+e.message)}try{r.push(J(await g.next()))}catch(e){r.push(e.name+': '+e.message)}return r.join('|')+'#'+L.join()",
  retrej: "var g=ag();await g.next();var r=[];try{r.push(J(await g.return(Promise.reject(new Error('RR')))))}catch(e){r.push(e.name+': '+e.message)}try{r.push(J(await g.next()))}catch(e){r.push(e.name+': '+e.message)}return r.join('|')+'#'+L.join()",
  doneretrej: "var g=ag();await g.return();var r=[];try{r.push(J(await g.return(Promise.reject(new Error('RR')))))}catch(e){r.push(e.name+': '+e.message)}return r.join('|')",
  forawait: "var r=[];try{for await(var x of ag()){r.push(J(x));if(x==1)break}}catch(e){r.push(e.name+': '+e.message)}return r.join('|')+'#'+L.join()",
  forawaitall: "var r=[];try{for await(var x of ag()){r.push(J(x))}}catch(e){r.push(e.name+': '+e.message)}return r.join('|')+'#'+L.join()",
  order: "var g=ag();var o=[];g.next().then(()=>o.push('a'));g.next().then(()=>o.push('b'));Promise.resolve().then(()=>o.push('p1')).then(()=>o.push('p2')).then(()=>o.push('p3')).then(()=>o.push('p4'));for(var i=0;i<12;i++)await Promise.resolve();return o.join()",
};
for (const [an, asrc] of Object.entries(agens)) {
  for (const [sn, ssrc] of Object.entries(asyncSeq)) {
    A(`${PRE}var J=JSON.stringify;${asrc};${ssrc}`);
  }
}
for (const m of ["next", "return", "throw"]) {
  for (const t of ["undefined", "null", "1", "{}", "(function*(){})()", "[][Symbol.iterator]()", "Promise.resolve()"]) {
    A(`var AGP=Object.getPrototypeOf(async function*(){}).prototype;try{var p=AGP[${JSON.stringify(m)}].call(${t});var r=await p.then(v=>'ok '+JSON.stringify(v),e=>'rej '+e.name+': '+e.message);return (p instanceof Promise)+' '+r}catch(e){return 'sync '+e.name+': '+e.message}`);
  }
}
E("Object.prototype.toString.call((async function*(){})())");
E("Object.prototype.toString.call(Object.getPrototypeOf(async function*(){}))");
E("Object.getPrototypeOf(async function*(){}).prototype[Symbol.toStringTag]");
E("(async function*(){})()[Symbol.asyncIterator]()!==undefined");
E("(()=>{var g=(async function*(){})();return g[Symbol.asyncIterator]()===g})()");
E("Object.getOwnPropertyNames(Object.getPrototypeOf(async function*(){}).prototype).join()");
E("Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)[Symbol.asyncIterator].name");
E("Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype))===Object.prototype");
A("var g=(async function*(){yield 1})();var p1=g.next();var p2=g.next();return [await p1,await p2].map(x=>JSON.stringify(x)).join('|')");
A("async function* ag(){var x=yield 1;return x}var g=ag();await g.next();return JSON.stringify(await g.next('arg'))");
A("var g=(async function*(){try{yield 1}catch(e){yield 'c'+e}})();await g.next();return JSON.stringify(await g.throw('E'))");
A("var g=(async function*(){try{yield 1}finally{await null;yield 'f'}})();await g.next();return JSON.stringify([await g.return('R'),await g.next()])");
A("var g=(async function*(){yield await Promise.resolve('aw')})();return JSON.stringify(await g.next())");
A("var g=(async function*(){return yield 'a'})();await g.next();return JSON.stringify(await g.next(Promise.resolve('b')))");
A("var it={[Symbol.asyncIterator](){return{next(){return Promise.resolve({done:true,value:'v'})}}}};var g=(async function*(){return yield* it})();return JSON.stringify(await g.next())");
A("var L=[];var it={[Symbol.iterator](){return{next(){return{done:false,value:Promise.resolve(1)}},return(){L.push('r');return{}}}}};var r=[];for await(var x of it){r.push(x);break}return r.join()+L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{next(){return Promise.resolve({done:false,value:1})},return(){L.push('r');return Promise.resolve(5)}}}};try{for await(var x of it){break}}catch(e){L.push(e.name+': '+e.message)}return L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{next(){return Promise.resolve({done:false,value:1})},return(){L.push('r');return {then(r){r({})}}}}}};try{for await(var x of it){break}}catch(e){L.push(e.name+': '+e.message)}return L.join()");

// ---- 5. Iterator helpers.
const hasHelpers = true;
const srcs = {
  ok: "mk(5)",
  short: "mk(2)",
  noret: "mk(5,{noReturn:true})",
  retthrow: "mk(5,{throwReturn:true})",
  nextthrow: "mk(5,{throwNext:2})",
  bare: "bare(5)",
};
const helpers = {
  map: "it.map(x=>x*2)",
  mapthrow: "it.map(x=>{if(x==1)throw new Error('cb');return x})",
  filter: "it.filter(x=>x%2)",
  filterthrow: "it.filter(x=>{if(x==1)throw new Error('cb');return true})",
  take2: "it.take(2)",
  take0: "it.take(0)",
  takeInf: "it.take(Infinity)",
  drop2: "it.drop(2)",
  drop0: "it.drop(0)",
  dropBig: "it.drop(99)",
  flatMap: "it.flatMap(x=>[x,x])",
  flatMapIter: "it.flatMap(x=>mk(x))",
  flatMapThrow: "it.flatMap(x=>{if(x==1)throw new Error('cb');return [x]})",
  flatMapString: "it.flatMap(x=>'ab')",
  flatMapNonIter: "it.flatMap(x=>1)",
  flatMapRetThrow: "it.flatMap(x=>mk(3,{throwReturn:true}))",
};
for (const [sn, s] of Object.entries(srcs)) {
  for (const [hn, h] of Object.entries(helpers)) {
    P(`var it=Iterator.from(${s});var h=${h};var out=[];try{for(var i=0;i<4;i++)out.push(${J}(h.next()))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
    P(`var it=Iterator.from(${s});var h=${h};var out=[];try{out.push(${J}(h.next()));out.push(${J}(h.return(7)));out.push(${J}(h.next()))}catch(e){out.push(e.name+': '+e.message)}R=out.join('|')+'#'+L.join()`);
  }
}
const reducers = {
  toArray: "it.toArray()",
  forEach: "(it.forEach(x=>L.push('f'+x)))",
  reduce: "it.reduce((a,x)=>a+x)",
  reduceInit: "it.reduce((a,x)=>a+x,10)",
  reduceThrow: "it.reduce((a,x)=>{throw new Error('cb')},0)",
  some: "it.some(x=>x==2)",
  someNever: "it.some(x=>false)",
  every: "it.every(x=>x<2)",
  everyAll: "it.every(x=>true)",
  find: "it.find(x=>x==3)",
  findThrow: "it.find(x=>{throw new Error('cb')})",
};
for (const [sn, s] of Object.entries(srcs)) {
  for (const [rn, r] of Object.entries(reducers)) {
    P(`var it=Iterator.from(${s});var out;try{out=${J}(${r})}catch(e){out=e.name+': '+e.message}R=out+'#'+L.join()`);
  }
}
const badArgs = ["undefined", "null", "-1", "NaN", "'a'", "{}", "Infinity", "-Infinity", "1.5", "2n", "{valueOf(){return 2}}", "{valueOf(){throw new Error('vo')}}", "Symbol()"];
for (const m of ["take", "drop"]) for (const a of badArgs) P(`var it=mk(3);try{var h=Iterator.prototype.${m}.call(it,${a});R='ok '+${J}(h.next())}catch(e){R=e.name+': '+e.message}R+='#'+L.join()`);
for (const m of ["map", "filter", "flatMap", "forEach", "some", "every", "find"]) {
  for (const a of ["undefined", "null", "1", "'s'", "{}", "Symbol()", "class{}", "async function(){}", "function*(){}", "()=>{}"]) {
    P(`var it=mk(3);try{var h=Iterator.prototype.${m}.call(it,${a});R='ok '+(typeof h)}catch(e){R=e.name+': '+e.message}R+='#'+L.join()`);
  }
}
for (const a of ["undefined", "null", "1", "()=>0"]) P(`var it=mk(3);try{R='ok '+Iterator.prototype.reduce.call(it,${a})}catch(e){R=e.name+': '+e.message}R+='#'+L.join()`);
P("try{R=Iterator.prototype.reduce.call(mk(0),(a,x)=>a)}catch(e){R=e.name+': '+e.message}");
for (const m of ["map", "filter", "take", "drop", "flatMap", "toArray", "forEach", "reduce", "some", "every", "find"]) {
  for (const t of ["undefined", "null", "1", "'s'", "{}", "{next(){return{done:true}}}", "[]", "Symbol()"]) {
    E(`Iterator.prototype.${m}.call(${t},x=>x)`);
  }
}
E("Iterator.name+Iterator.length");
E("typeof Iterator");
E("Iterator.prototype===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))");
E("new Iterator()");
E("Iterator()");
E("class X extends Iterator{};new X() instanceof Iterator");
E("Reflect.construct(Iterator,[],class{}) instanceof Iterator");
E("Iterator.prototype[Symbol.toStringTag]");
E("Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).get===undefined");
E("typeof Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).set");
E("Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').get===undefined");
E("(()=>{try{Iterator.prototype[Symbol.toStringTag]='x';return Iterator.prototype[Symbol.toStringTag]}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var o=Object.create(Iterator.prototype);o[Symbol.toStringTag]='x';return Object.getOwnPropertyDescriptor(o,Symbol.toStringTag).value})()");
E("(()=>{try{Iterator.prototype[Symbol.toStringTag]='x';return 1}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{Iterator.prototype.constructor=1;return Iterator.prototype.constructor===Iterator}catch(e){return e.name+': '+e.message}})()");
E("Object.getOwnPropertyNames(Iterator.prototype).sort().join()");
E("Object.getOwnPropertyNames(Iterator).sort().join()");
E("Iterator.from.length+Iterator.from.name");
E("Iterator.from([1,2]).toArray().join()");
E("Iterator.from('ab').toArray().join()");
E("Iterator.from({next(){return{done:true}}}).toArray().length");
E("Iterator.from({next(){return{done:true}}}) instanceof Iterator");
E("(()=>{var a=[1][Symbol.iterator]();return Iterator.from(a)===a})()");
E("Object.prototype.toString.call(Iterator.from({next(){return{done:true}}}))");
E("Iterator.from(1)");
E("Iterator.from(null)");
E("Iterator.from(undefined)");
E("Iterator.from({})");
E("Iterator.from({[Symbol.iterator]:1})");
E("Iterator.from({[Symbol.iterator](){return 1}})");
E("Iterator.from(new String('ab')).toArray().join()");
E("Iterator.from(Symbol())");
E("Object.getPrototypeOf(Iterator.from({next(){return{done:true}}})).constructor===undefined");
E("(()=>{var w=Iterator.from({next(){return{done:false,value:1}},return(){return{done:true,value:'r'}}});return JSON.stringify([w.next(),w.return(),w.next()])})()");
E("(()=>{var w=Iterator.from({next(){return{done:false,value:1}}});return JSON.stringify([w.next(),w.return(),w.next()])})()");
E("(()=>{var w=Iterator.from({next(){return{done:false,value:1}}});return Object.getOwnPropertyNames(Object.getPrototypeOf(w)).join()})()");
E("(()=>{var h=mk0().map(x=>x);function mk0(){return Iterator.from([1,2])};return Object.prototype.toString.call(h)})()");
E("(()=>{var h=[1].values().map(x=>x);return h[Symbol.toStringTag]+'|'+Object.getOwnPropertyNames(Object.getPrototypeOf(h)).join()+'|'+(Object.getPrototypeOf(Object.getPrototypeOf(h))===Iterator.prototype)})()");
E("(()=>{var h=[1].values().map(x=>x);return h[Symbol.iterator]()===h})()");
E("(()=>{var P=Object.getPrototypeOf([1].values().map(x=>x));try{return P.next.call({})}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var P=Object.getPrototypeOf([1].values().map(x=>x));try{return P.return.call({})}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var P=Object.getPrototypeOf([1].values().filter(x=>x));try{return P.next.call([1].values().map(x=>x))}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var h=[1,2,3].values().map(x=>x);return JSON.stringify([h.next(),h.return(),h.next(),h.return()])})()");
E("(()=>{var h=[1,2,3].values().take(1);return JSON.stringify([h.next(),h.next(),h.next()])})()");
E("(()=>{var a=[1,2,3].values();var h=a.take(1);h.next();h.next();return JSON.stringify(a.next())})()");
E("(()=>{var h=[1,2,3].values().map(function(x,i){return i+':'+arguments.length});return JSON.stringify(h.toArray())})()");
E("(()=>{var h=[1,2,3].values().filter(function(x,i){return i%2==0});return JSON.stringify(h.toArray())})()");
E("(()=>{var h=[1,2,3].values().flatMap(function(x,i){return [i]});return JSON.stringify(h.toArray())})()");
E("(()=>{var h=[[1],[2]].values().flatMap(x=>x);return JSON.stringify(h.toArray())})()");
E("(()=>{var h=[1].values().flatMap(x=>new String('ab'));return JSON.stringify(h.toArray())})()");
E("(()=>{try{return JSON.stringify([1].values().flatMap(x=>'ab').toArray())}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var h=[1].values().flatMap(x=>({next(){return{done:true}}}));try{return JSON.stringify(h.toArray())}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var h=[1].values().flatMap(x=>({[Symbol.iterator](){return{next(){return{done:true}}}}}));return JSON.stringify(h.toArray())})()");

// ---- 6. Iteradores nativos: toString, toStringTag, cadeia de protótipos, next com this errado.
const natives = {
  array: "[1,2][Symbol.iterator]()",
  arrayKeys: "[1,2].keys()",
  arrayEntries: "[1,2].entries()",
  string: "'ab'[Symbol.iterator]()",
  map: "new Map([[1,2]])[Symbol.iterator]()",
  mapKeys: "new Map([[1,2]]).keys()",
  mapValues: "new Map([[1,2]]).values()",
  set: "new Set([1])[Symbol.iterator]()",
  setEntries: "new Set([1]).entries()",
  regexp: "'a1b2'.matchAll(/\\d/g)",
  typed: "new Uint8Array(2)[Symbol.iterator]()",
  args: "(function(){return arguments[Symbol.iterator]()})(1)",
};
for (const [nn, n] of Object.entries(natives)) {
  const proto = `Object.getPrototypeOf(${n})`;
  E(`Object.prototype.toString.call(${n})`);
  E(`String(${n})`);
  E(`${n}[Symbol.toStringTag]`);
  E(`Object.getOwnPropertyNames(${proto}).join()`);
  E(`Object.getOwnPropertySymbols(${proto}).map(String).join()`);
  E(`${proto}.hasOwnProperty(Symbol.toStringTag)`);
  E(`JSON.stringify(Object.getOwnPropertyDescriptor(${proto},Symbol.toStringTag))`);
  E(`JSON.stringify(Object.getOwnPropertyDescriptor(${proto},'next'))`);
  E(`${proto}.next.name+'/'+${proto}.next.length`);
  E(`Object.getPrototypeOf(${proto})===Iterator.prototype`);
  E(`${n}[Symbol.iterator]()===${n}`);
  E(`${n}.hasOwnProperty('next')`);
  E(`Object.getOwnPropertyNames(${n}).length`);
  E(`Object.isExtensible(${proto})+','+Object.isFrozen(${proto})`);
  E(`typeof ${proto}.constructor+'|'+(${proto}.constructor===Object.getPrototypeOf(${proto}).constructor)`);
  for (const t of ["undefined", "null", "1", "'s'", "{}", "[]", "Symbol()", "new Proxy({},{})", "Object.create(" + proto + ")", "[][Symbol.iterator]()", "new Map().entries()", "new Set().values()", "'a'[Symbol.iterator]()", "'a'.matchAll(/a/g)"]) {
    E(`${proto}.next.call(${t})`);
  }
  E(`JSON.stringify([${n}.next(),${n}.next(),${n}.next(),${n}.next()])`);
  E(`JSON.stringify([...${n}])`);
  E(`(()=>{var i=${n};var a=[...i];return JSON.stringify([a.length,i.next()])})()`);
  E(`(()=>{var i=${n};i.next();i.next();i.next();return JSON.stringify(i.next())+Object.keys(i.next()).join()})()`);
  E(`(()=>{var i=${n};return Array.from(i).length})()`);
  E(`(()=>{var [a,b]=${n};return JSON.stringify([a,b])})()`);
  E(`(()=>{var i=${n};return typeof i.return+typeof i.throw})()`);
  E(`(()=>{var i=${n};return JSON.stringify(i.next.call(i))})()`);
}
E("(()=>{var m=new Map([[1,1]]);var i=m.keys();m.set(2,2);return JSON.stringify([...i])})()");
E("(()=>{var m=new Map([[1,1],[2,2]]);var i=m.keys();i.next();m.delete(2);return JSON.stringify(i.next())})()");
E("(()=>{var m=new Map([[1,1],[2,2]]);var i=m.keys();i.next();m.clear();m.set(3,3);return JSON.stringify(i.next())})()");
E("(()=>{var s=new Set([1]);var i=s.values();s.add(2);s.add(3);return JSON.stringify([...i])})()");
E("(()=>{var s=new Set([1,2]);var i=s.values();i.next();i.next();i.next();s.add(9);return JSON.stringify(i.next())})()");
E("(()=>{var a=[1];var i=a.values();a.push(2);return JSON.stringify([...i])})()");
E("(()=>{var a=[1,2];var i=a.values();i.next();i.next();i.next();a.push(3);return JSON.stringify(i.next())})()");
E("(()=>{var a=[1,2,3];var i=a.values();a.length=1;return JSON.stringify([...i])})()");
E("(()=>{var s='a\\ud83d\\ude00b';return JSON.stringify([...s[Symbol.iterator]()])})()");
E("(()=>{var s='a\\ud83db';return JSON.stringify([...s].map(c=>c.charCodeAt(0)))})()");
E("(()=>{try{return String.prototype[Symbol.iterator].call(null)}catch(e){return e.name+': '+e.message}})()");
E("String.prototype[Symbol.iterator].call(12)[Symbol.toStringTag]");
E("(()=>{var i=Array.prototype[Symbol.iterator].call({length:2,0:'a',1:'b'});return JSON.stringify([...i])})()");
E("(()=>{try{return Array.prototype.values.call(null)}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return Map.prototype.keys.call({})}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return Set.prototype.values.call(new Map)}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return Map.prototype[Symbol.iterator].call(new Set)}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return Map.prototype.entries.call(1)}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return RegExp.prototype[Symbol.matchAll].call(1,'a')}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return 'a'.matchAll(/a/)}catch(e){return e.name+': '+e.message}})()");
E("(()=>{try{return 'a'.matchAll('a').next().value[0]}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var r=/a/g;r.lastIndex=1;var i='aaa'.matchAll(r);return JSON.stringify([...i].map(m=>m.index))+r.lastIndex})()");
E("(()=>{var i='aXa'.matchAll(/a/g);i.next();var r=i.next();return r.value.index})()");
E("(()=>{var i=''.matchAll(/(?:)/g);return JSON.stringify([...i].map(m=>m.index))})()");
E("(()=>{var i='ab'.matchAll(/(?:)/gu);return JSON.stringify([...i].map(m=>m.index))})()");
E("(()=>{var i='a'.matchAll(/(?<n>a)/g);return JSON.stringify(i.next().value.groups)})()");
E("Object.getPrototypeOf('a'.matchAll(/a/g)).hasOwnProperty('next')");
E("Object.getPrototypeOf('a'.matchAll(/a/g))[Symbol.toStringTag]");
E("Object.getPrototypeOf(Object.getPrototypeOf('a'.matchAll(/a/g)))===Iterator.prototype");
E("Object.getPrototypeOf([][Symbol.iterator]())===Object.getPrototypeOf([].keys())");
E("Object.getPrototypeOf(new Map().keys())===Object.getPrototypeOf(new Map().entries())");
E("Object.getPrototypeOf(new Set().keys())===Object.getPrototypeOf(new Set().values())");
E("Object.getPrototypeOf(new Map().keys())===Object.getPrototypeOf(new Set().keys())");
E("Object.getPrototypeOf(new Uint8Array(1).values())===Object.getPrototypeOf([].values())");
E("Object.getPrototypeOf(Float64Array.prototype.keys.call(new Float64Array(1)))===Object.getPrototypeOf([].keys())");
E("Set.prototype.keys===Set.prototype.values");
E("Set.prototype[Symbol.iterator]===Set.prototype.values");
E("Map.prototype[Symbol.iterator]===Map.prototype.entries");
E("Array.prototype[Symbol.iterator]===Array.prototype.values");
E("Uint8Array.prototype[Symbol.iterator]===Uint8Array.prototype.values");
E("String.prototype[Symbol.iterator].name");
E("Map.prototype[Symbol.iterator].name+Set.prototype[Symbol.iterator].name+Array.prototype[Symbol.iterator].name");
E("Object.getOwnPropertyDescriptor(Object.getPrototypeOf([][Symbol.iterator]()),'next').enumerable");
E("Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.iterator).enumerable");

// ---- 7. %IteratorPrototype%[Symbol.iterator].
E("Iterator.prototype[Symbol.iterator].name");
E("Iterator.prototype[Symbol.iterator].length");
E("Iterator.prototype[Symbol.iterator].call(5)");
E("Iterator.prototype[Symbol.iterator].call(undefined)");
E("Iterator.prototype[Symbol.iterator].call(null)");
E("typeof Iterator.prototype[Symbol.iterator].call('s')");
E("(()=>{var o={};return Iterator.prototype[Symbol.iterator].call(o)===o})()");
E("Object.getOwnPropertySymbols(Iterator.prototype).map(String).sort().join()");
E("Object.getPrototypeOf(Iterator.prototype)===Object.prototype");
E("Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))===Iterator.prototype");
E("(()=>{class It extends Iterator{next(){return{done:true}}};var i=new It;return i[Symbol.iterator]()===i})()");
E("(()=>{class It extends Iterator{next(){return{done:true}}};return Object.prototype.toString.call(new It)})()");
E("(()=>{class It extends Iterator{};try{return new It().toArray()}catch(e){return e.name+': '+e.message}})()");
E("(()=>{class It extends Iterator{constructor(){super();this.i=0}next(){return this.i<3?{value:this.i++,done:false}:{done:true}}};return JSON.stringify(new It().map(x=>x*2).toArray())})()");
E("(()=>{class It extends Iterator{constructor(){super();this.i=0}next(){return this.i<3?{value:this.i++,done:false}:{done:true}}};return JSON.stringify([...new It])})()");
E("(()=>{var o=Object.create(Iterator.prototype);o.next=()=>({done:true});return JSON.stringify(o.toArray())})()");
E("(()=>{var o=Object.create(Iterator.prototype);try{return o.toArray()}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var o=Object.create(Iterator.prototype);o.next=1;try{return o.toArray()}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var o=Object.create(Iterator.prototype);o.next=()=>1;try{return o.toArray()}catch(e){return e.name+': '+e.message}})()");
E("(()=>{var o=Object.create(Iterator.prototype);o.next=()=>({done:false,value:1});var h=o.take(2);return JSON.stringify(h.toArray())})()");
E("(()=>{var n=0;var o=Object.create(Iterator.prototype);Object.defineProperty(o,'next',{get(){n++;return()=>({done:true})}});o.map(x=>x);o.filter(x=>x).toArray();return n})()");
E("(()=>{var n=0;var o=Object.create(Iterator.prototype);Object.defineProperty(o,'next',{get(){n++;return()=>({done:true})}});o.toArray();return n})()");
E("(()=>{var n=0;var o=Object.create(Iterator.prototype);Object.defineProperty(o,'next',{get(){n++;return()=>({done:true})}});var h=o.map(x=>x);h.next();h.next();return n})()");
E("(()=>{var f=Iterator.prototype.map;try{return f.call({next(){return{done:true}}},x=>x).toArray().length}catch(e){return e.name+': '+e.message}})()");

// ---- 8. Fechamento de iterador em destructuring, for-of, spread e consumidores nativos.
const closing = {
  forBreak: "for(var x of mk(5)){break}",
  forThrow: "for(var x of mk(5)){throw new Error('body')}",
  forReturn: "(function(){for(var x of mk(5)){return 1}})()",
  forContinue: "for(var x of mk(2)){continue}",
  forContinueOuter: "o:for(var i of [1]){for(var x of mk(5)){continue o}}",
  forBreakOuter: "o:for(var i of [1]){for(var x of mk(5)){break o}}",
  forBreakNoRet: "for(var x of mk(5,{noReturn:true})){break}",
  forBreakRetThrow: "for(var x of mk(5,{throwReturn:true})){break}",
  forThrowRetThrow: "for(var x of mk(5,{throwReturn:true})){throw new Error('body')}",
  forNextThrow: "for(var x of mk(5,{throwNext:1})){}",
  forRetBad: "var m=mk(5);m.return=function(){L.push('r');return 1};for(var x of m){break}",
  forRetNull: "var m=mk(5);m.return=null;for(var x of m){break}",
  forRetNum: "var m=mk(5);m.return=1;for(var x of m){break}",
  forDestruct: "for(var [a] of [mk(5)]){}",
  forDestructBreak: "for(var [a] of [mk(5)]){break}",
  forAssignThrow: "var o={set p(v){throw new Error('set')}};for(o.p of mk(5)){}",
  forLetTdz: "for(let x of mk(5)){break}",
  forOfExprThrow: "for(var x of (()=>{throw new Error('e')})()){}",
  destr1: "var [a]=mk(5)",
  destr2: "var [a,b]=mk(5)",
  destrEmpty: "var []=mk(5)",
  destrHole: "var [,]=mk(5)",
  destrRest: "var [...r]=mk(3)",
  destrDefaultThrow: "var [a=(()=>{throw new Error('def')})()]=mk(0)",
  destrDefaultThrow2: "var [a,b=(()=>{throw new Error('def')})()]=mk(1)",
  destrTargetThrow: "var o={set p(v){throw new Error('set')}};[o.p]=mk(5)",
  destrExhaust: "var [a,b,c,d]=mk(2)",
  destrNextThrow: "var [a,b,c]=mk(5,{throwNext:1})",
  destrRetThrow: "var [a]=mk(5,{throwReturn:true})",
  destrRetThrowAfterThrow: "var o={set p(v){throw new Error('set')}};[o.p]=mk(5,{throwReturn:true})",
  destrNested: "var [[a]]=[mk(5)]",
  destrObjInArr: "var [{x}]=[{x:1}]",
  destrNestedThrow: "var [[a=(()=>{throw new Error('in')})()]]=[mk(0)]",
  destrAssignExpr: "var a;[a]=mk(5)",
  destrParam: "(function([a]){})(mk(5))",
  destrParamDefaultThrow: "(function([a=(()=>{throw new Error('pd')})()]){})(mk(0))",
  destrCatch: "try{throw mk(5)}catch([a]){}",
  spread: "[...mk(3)]",
  spreadNextThrow: "[...mk(5,{throwNext:1})]",
  spreadCall: "Math.max(...mk(3))",
  arrayFrom: "Array.from(mk(3))",
  arrayFromMapThrow: "Array.from(mk(5),x=>{throw new Error('map')})",
  newSet: "new Set(mk(3))",
  newMap: "new Map(mk(2))",
  newMapBad: "new Map(mk(2,{}))",
  newMapAdder: "var old=Map.prototype.set;Map.prototype.set=function(){throw new Error('adder')};try{new Map([[1,2]])}finally{Map.prototype.set=old}",
  newMapAdderIter: "var old=Map.prototype.set;Map.prototype.set=function(){throw new Error('adder')};try{new Map({[Symbol.iterator](){return mk(5)[Symbol.iterator]()},})}finally{Map.prototype.set=old}",
  newSetAdderIter: "var old=Set.prototype.add;Set.prototype.add=function(){throw new Error('adder')};try{new Set(mk(5))}finally{Set.prototype.add=old}",
  promiseAll: "Promise.all(mk(3))",
  objectFromEntries: "Object.fromEntries(mk(2))",
  objectFromEntriesBad: "Object.fromEntries({[Symbol.iterator](){return mk(5)}})",
  weakSet: "new WeakSet(mk(5))",
  yieldStarBreak: "function* g(){yield* mk(5)}for(var x of g()){break}",
  yieldStarThrow: "function* g(){yield* mk(5)}for(var x of g()){throw new Error('body')}",
  genFinallyBreak: "function* g(){try{yield 1;yield 2}finally{L.push('gfin')}}for(var x of g()){break}",
  genFinallyThrow: "function* g(){try{yield 1;yield 2}finally{L.push('gfin')}}for(var x of g()){throw new Error('body')}",
  genFinallyRetThrow: "function* g(){try{yield 1;yield 2}finally{throw new Error('gf')}}for(var x of g()){break}",
  genFinallyYield: "function* g(){try{yield 1;yield 2}finally{yield 'f'}}for(var x of g()){L.push('x'+x);break}",
  genSpreadDone: "function* g(){try{yield 1}finally{L.push('gfin')}}[...g()]",
  genDestr: "function* g(){try{yield 1;yield 2}finally{L.push('gfin')}}var [a]=g()",
  genDestrAll: "function* g(){try{yield 1}finally{L.push('gfin')}}var [a,b]=g()",
  genDestrRest: "function* g(){try{yield 1}finally{L.push('gfin')}}var [...a]=g()",
  genForOfNested: "function* g(){try{yield 1}finally{L.push('g')}}for(var x of g()){for(var y of g()){break}}",
};
for (const [cn, c] of Object.entries(closing)) {
  P(`${c};R='ok#'+L.join()`);
}
// Variantes: o corpo captura o erro e registra qual saiu.
for (const [cn, c] of Object.entries(closing)) {
  if (/^(forBreak|forContinue|destr1|destr2|spread|arrayFrom)/.test(cn)) continue;
  P(`try{${c};L.push('done')}catch(e){L.push(e.name+': '+e.message)}R=L.join()`);
}
// Iteradores async no fechamento.
A("var L=[];var it={[Symbol.asyncIterator](){return{i:0,next(){return Promise.resolve({done:this.i>2,value:this.i++})},return(){L.push('r');return Promise.resolve({})}}}};for await(var x of it){if(x==1)break}return L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{i:0,next(){return Promise.resolve({done:this.i>2,value:this.i++})},return(){L.push('r');return Promise.resolve({})}}}};try{for await(var x of it){throw new Error('b')}}catch(e){L.push(e.message)}return L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{next(){return Promise.reject(new Error('n'))},return(){L.push('r');return Promise.resolve({})}}}};try{for await(var x of it){}}catch(e){L.push(e.message)}return L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{next(){return Promise.resolve(1)},return(){L.push('r');return Promise.resolve({})}}}};try{for await(var x of it){}}catch(e){L.push(e.name+': '+e.message)}return L.join()");
A("var L=[];var it={[Symbol.asyncIterator](){return{next(){return Promise.resolve({done:false,value:1})}}}};try{for await(var x of it){break}}catch(e){L.push(e.name+': '+e.message)}return L.join()+'ok'");
A("var L=[];var it={[Symbol.asyncIterator]:null,[Symbol.iterator](){return{i:0,next(){return{done:this.i++>1,value:Promise.resolve(this.i)}},return(){L.push('r');return{}}}}};var r=[];for await(var x of it){r.push(x)}return r.join()+L.join()");
A("var L=[];var it={[Symbol.iterator](){return{i:0,next(){return{done:false,value:Promise.reject(new Error('v'))}},return(){L.push('r');return{}}}}};try{for await(var x of it){}}catch(e){L.push(e.message)}return L.join()");
A("try{for await(var x of 1){}}catch(e){return e.name+': '+e.message}");
A("try{for await(var x of {}){}}catch(e){return e.name+': '+e.message}");
A("try{for await(var x of null){}}catch(e){return e.name+': '+e.message}");
A("try{for(var x of 1){}}catch(e){return e.name+': '+e.message}");
A("try{for(var x of {}){}}catch(e){return e.name+': '+e.message}");
A("try{var [a]={}}catch(e){return e.name+': '+e.message}");
A("try{var [a]=null}catch(e){return e.name+': '+e.message}");
A("try{var [a]=undefined}catch(e){return e.name+': '+e.message}");
A("try{var [a]=1}catch(e){return e.name+': '+e.message}");
A("try{var [a]={[Symbol.iterator]:1}}catch(e){return e.name+': '+e.message}");
A("try{var [a]={[Symbol.iterator](){return 1}}}catch(e){return e.name+': '+e.message}");
A("try{var [a]={[Symbol.iterator](){return{}}}}catch(e){return e.name+': '+e.message}");
A("try{var [a]={[Symbol.iterator](){return{next(){return 1}}}}}catch(e){return e.name+': '+e.message}");
A("try{[...{[Symbol.iterator](){return{next(){return null}}}}]}catch(e){return e.name+': '+e.message}");
A("try{for(var x of {[Symbol.iterator](){return{next(){return undefined}}}}){}}catch(e){return e.name+': '+e.message}");
A("try{new Set(1)}catch(e){return e.name+': '+e.message}");
A("try{new Map([1])}catch(e){return e.name+': '+e.message}");
A("try{Array.from({[Symbol.iterator]:1})}catch(e){return e.name+': '+e.message}");
A("try{Math.max(...1)}catch(e){return e.name+': '+e.message}");
A("try{Math.max(...{})}catch(e){return e.name+': '+e.message}");
A("try{(function*(){yield* 1})().next()}catch(e){return e.name+': '+e.message}");
A("try{(function*(){yield* {}})().next()}catch(e){return e.name+': '+e.message}");
A("try{(function*(){yield* null})().next()}catch(e){return e.name+': '+e.message}");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "iterproto-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (o bun transpila arquivos e muda a semântica de script).
// As microtarefas são esvaziadas pelo laço de eventos antes do `exit`, quando `R` é lido.
const source_file = path.join(dir, "iter_source.js");
const file = path.join(dir, "iter_case.js");
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
// A matriz completa tem milhares de programas; o gerador amostra por hash (sampleByHash) 1 em cada STRIDE (padrão mira ~450) e `STRIDE=1`
// amplia a cobertura.
const unique = [...new Set(programs)];
const STRIDE = Number(process.env.STRIDE) || Math.max(1, Math.round(unique.length / 450));
process.stderr.write(`matriz ${unique.length}, STRIDE ${STRIDE}\n`);
for (const body of sampleByHash(unique, Math.ceil(unique.length / STRIDE))) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ").replace(/\bR\+= /g, "globalThis.R += ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
