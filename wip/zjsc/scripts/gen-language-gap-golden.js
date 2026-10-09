// Gera tests/golden/language_gap_bun.tsv: os dois maiores buracos de cobertura da LINGUAGEM medidos no bun 1.4.2.
//   A. Operadores e referências: ordem de coerção (valueOf/toString/Symbol.toPrimitive, BigInt misto, Symbol), ordem de
//      avaliação de atribuição composta/lógica/update em membro (com Proxy registrando get/set/has), optional chaining,
//      precedência e erros de sintaxe de operadores.
//   B. Destructuring: arrays e objetos em declaração, atribuição, parâmetro, for-of e catch, com iteradores que
//      registram next/return, defaults com efeito, chaves computadas, rest, alvos que são membros, erro no meio
//      (IteratorClose), e parâmetros default (escopo, TDZ, arguments, length).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), como em gen-function-error-golden.js.
// Cada caso roda em `new Function(corpo)` dentro de `run`, que devolve "<log de efeitos> => <resultado ou T:Erro: msg>".
// O log vive no array global `L`; os ajudantes (Q, P, X, It, Tr) acrescentam a ele.
// Uso: bun scripts/gen-language-gap-golden.js > tests/golden/language_gap_bun.tsv
const fs = require("fs");
const { emitRow, stepSampler } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRE =
  "(function(g){" +
  "g.L=[];" +
  "g.S=function S(v,d){d=d||0;var t=typeof v;if(v===null)return'null';if(t==='undefined')return'undefined';" +
  "if(t==='number')return Object.is(v,-0)?'-0':String(v);if(t==='string')return JSON.stringify(v);" +
  "if(t==='boolean')return String(v);if(t==='bigint')return v+'n';if(t==='symbol')return String(v);" +
  "if(t==='function')return'fn:'+v.name+'/'+v.length;if(d>3)return'...';" +
  "if(Array.isArray(v)){var o=[];for(var i=0;i<v.length;i++)o.push(i in v?S(v[i],d+1):'<hole>');return'['+o.join(',')+']'}" +
  "var o=[];for(var k of Reflect.ownKeys(v)){var x=Object.getOwnPropertyDescriptor(v,k);" +
  "o.push(String(k)+':'+('value' in x?S(x.value,d+1):'(accessor)'))}return'{'+o.join(',')+'}'};" +
  "g.Q=function(n,v){L.push(n);return v};" +
  "g.P=function(n,v){return{valueOf:function(){L.push('v'+n);return v},toString:function(){L.push('s'+n);return String(v)}}};" +
  "g.X=function(n,v){var o={};o[Symbol.toPrimitive]=function(h){L.push(n+':'+h);return v};return o};" +
  "g.It=function(n,a,m){var o={};o[Symbol.iterator]=function(){L.push(n+'.iter');var i=0;" +
  "return{next:function(){L.push(n+'.next');if(m==='throwNext'&&i===a.length)throw new Error('nx');" +
  "return i<a.length?{value:a[i++],done:false}:{value:undefined,done:true}}," +
  "return:m==='noReturn'?undefined:function(){L.push(n+'.return');if(m==='badReturn')return 1;" +
  "if(m==='throwReturn')throw new Error('rt');return{}}}};return o};" +
  "g.Tr=function(n,t){return new Proxy(t,{" +
  "get:function(t,k,r){L.push(n+'.get '+String(k));return Reflect.get(t,k,r)}," +
  "set:function(t,k,v,r){L.push(n+'.set '+String(k));return Reflect.set(t,k,v,r)}," +
  "has:function(t,k){L.push(n+'.has '+String(k));return Reflect.has(t,k)}," +
  "deleteProperty:function(t,k){L.push(n+'.delete '+String(k));return Reflect.deleteProperty(t,k)}," +
  "ownKeys:function(t){L.push(n+'.ownKeys');return Reflect.ownKeys(t)}," +
  "getOwnPropertyDescriptor:function(t,k){L.push(n+'.gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})};" +
  "g.run=function(code){L=[];var r;try{r=S(new Function(code)())}catch(e){" +
  "r='T:'+(e&&e.constructor&&e.constructor.name)+': '+(e&&e.message)}return L.join(' ')+' => '+r};" +
  "})(globalThis);\n";

// Os candidatos entram em `pool`; as grades de padrão x fonte x contexto entram com densidade (`thin(passo, corpo)`, 1 em
// `passo`) e a escolha é por hash do programa (`sampleByHash` dentro do `stepSampler`), nunca pela posição no laço.
const pool = stepSampler();
const program = body => PRE + "globalThis.R=run(" + JSON.stringify(body) + ");";
const add = body => pool.push(1, program(body));
const thin = (step, body) => pool.push(step, program(body));

// ---------------------------------------------------------------- A1. Operadores binários com coerção.
{
  const arith = ["+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "&", "|", "^"];
  const arithPairs = [
    ["P('a',1)", "P('b',2)"], ["X('a',1)", "X('b',2)"], ["P('a',1n)", "P('b',2n)"], ["P('a',1n)", "P('b',2)"],
    ["P('a','x')", "P('b','y')"], ["X('a',1n)", "2n"], ["1n", "2"], ["1n", "'2'"], ["Q('a',null)", "Q('b',undefined)"],
    ["Symbol('s')", "1"], ["1", "Symbol('s')"], ["P('a',NaN)", "P('b',1)"], ["0n", "-0"], ["{}", "[]"],
  ];
  for (const op of arith) for (const [a, b] of arithPairs) add(`return ${a} ${op} ${b}`);
  const rel = ["<", ">", "<=", ">=", "==", "!="];
  const relPairs = [
    ["P('a',1)", "P('b',2)"], ["X('a',1)", "X('b',2)"], ["P('a',1n)", "P('b',2)"], ["P('a','1')", "P('b',1n)"],
    ["1n", "'x'"], ["Symbol('s')", "1"], ["null", "undefined"], ["null", "0"], ["NaN", "NaN"], ["'a'", "'b'"],
    ["2n**64n", "2**64"], ["[1]", "1"],
  ];
  for (const op of rel) for (const [a, b] of relPairs) add(`return ${a} ${op} ${b}`);
  for (const op of ["in", "instanceof"]) {
    const pairs =
      op === "in"
        ? [["P('k','a')", "Tr('o',{a:1})"], ["'a'", "1"], ["'a'", "null"], ["Symbol.iterator", "[]"], ["X('k','a')", "{a:1}"], ["'x'", "'str'"], ["1", "[5]"]]
        : [["{}", "Object"], ["{}", "{}"], ["{}", "()=>{}"], ["{}", "Tr('C',function(){})"], ["1", "Object"], ["{}", "Object.assign(function(){},{prototype:1})"],
           ["{}", "{[Symbol.hasInstance](v){L.push('hi');return 1}}"], ["{}", "null"], ["{}", "class{static[Symbol.hasInstance](){return false}}"]];
    for (const [a, b] of pairs) add(`return ${a} ${op} ${b}`);
  }
  for (const u of ["+", "-", "~", "!", "typeof ", "void "]) for (const a of ["P('a',1)", "X('a',1n)", "Symbol()", "1n", "'x'", "null"]) add(`return ${u}${a}`);
  add("return -2 ** 2"); add("return (-2) ** 2"); add("return 2 ** 3 ** 2"); add("return 2 ** -1"); add("return -(2 ** 2)");
  add("return (-1n) ** 2n"); add("return 2n ** -1n"); add("return 1n / 0n"); add("return 1n % 0n"); add("return 1n >>> 1n"); add("return 5n >> 70n");
  add("return +1n"); add("return 1n + 'a'"); add("return `${P('a',1)}${P('b',2)}`"); add("return `${Symbol()}`");
  add("return '' + {[Symbol.toPrimitive](h){return h}}"); add("return `${{[Symbol.toPrimitive](h){return h}}}`"); add("return {[Symbol.toPrimitive](h){return h}} * 1");
  add("return {[Symbol.toPrimitive]:1} + 1"); add("return {[Symbol.toPrimitive](){return {}}} + 1"); add("return {valueOf(){return {}},toString(){return {}}} + 1");
  add("return new Date(0) + 1"); add("return new Date(5) - 1"); add("return [] + {}"); add("return [1,2] + [3]");
}

// ---------------------------------------------------------------- A2. Referências: atribuição composta, lógica, update.
{
  const compound = ["+=", "-=", "*=", "**=", "<<=", ">>>=", "&=", "||=", "&&=", "??="];
  for (const op of compound)
    for (const v of ["0", "1", "null", "'s'", "undefined"])
      add(`var o=Tr('o',{a:${v}});o[Q('k','a')]${op}Q('v',2);return [o.a,L.length]`);
  for (const op of ["+=", "||=", "??=", "**="]) add(`var o=Tr('o',{a:1});o[P('k','a')]${op}Q('v',2);return o.a`);
  for (const op of ["+=", "||=", "&&=", "="]) {
    add(`null[Q('k','a')]${op}Q('v',1)`);
    add(`undefined[Q('k','a')]${op}Q('v',1)`);
    add(`Q('b',null).a${op}Q('v',1)`);
    add(`Q('b',null)[P('k','a')]${op}Q('v',1)`);
  }
  for (const v of ["P('x',1)", "P('x',1n)", "'5'", "'a'", "null", "undefined", "1n", "Symbol()"])
    for (const u of ["o.a++", "++o.a", "o.a--", "--o.a"]) add(`var o=Tr('o',{a:${v}});var r=${u};return [r,o.a]`);
  add("const c=1;c||=Q('v',2);return c"); add("const c=1;c&&=Q('v',2);return c"); add("const c=1;c+=Q('v',2);return c");
  add("const c=null;c??=Q('v',2);return c"); add("const c=1;c++;return c"); add("'use strict';undeclared||=1;return undeclared");
  add("'use strict';var o=Object.freeze({a:0});o.a||=Q('v',2);return o.a"); add("'use strict';var o=Object.freeze({a:1});o.a||=Q('v',2);return o.a");
  add("'use strict';var o=Object.freeze({a:1});o.a&&=Q('v',2);return o.a"); add("var o=Object.freeze({a:1});o.a&&=Q('v',2);return o.a");
  add("var f;f??=function(){};return f.name"); add("var o={};o.f??=function(){};return o.f.name"); add("var f;f||=class{};return f.name");
  add("var f;f&&=function(){};return f"); add("var f=1;f&&=()=>{};return f.name"); add("var o={get a(){L.push('g');return 1},set a(v){L.push('s'+v)}};o.a||=2;o.a&&=3;o.a??=4;o.a+=5;return o.a");
  add("var a=1,b=2,c=3;a=b=c;return [a,b,c]"); add("var o=Tr('o',{});o[Q('k','a')]=Q('v',1);return L.length");
  add("var o=Tr('o',{});(Q('o2',o))[Q('k','a')]=(Q('v',1),Q('w',2));return o.a");
  add("var a=[1,2,3];var i=0;a[i++]+=a[i++];return [a,i]"); add("var a=[1,2,3];var i=0;a[i++]**=a[i++];return [a,i]"); add("var i=0;var a=[i++,i++,i++];return a");
  add("var x=1;x+=x++;return x"); add("var x=1;x=x++ + ++x;return x"); add("var x=1;return x+++x"); add("var x=1,y=2;return x+++ +y");
  add("var o={a:{b:1}};o.a.b+=Q('v',1);return o.a.b"); add("var o={};o.a.b=1"); add("var o={};o.a.b.c=1"); add("var o={};return o.a.b");
  add("var o={};return o.a()"); add("var o={};return o.a.b()"); add("return undeclared.x"); add("return null.x"); add("return undefined[0]");
  add("null.x=1"); add("undefined[Symbol.iterator]"); add("var s=Symbol('d');null[s]"); add("var f=null;f()"); add("var o={f:1};o.f()"); add("var o={};new o.f()");
  add("new (Q('C',function(){this.a=Q('x',1)}))(Q('arg',2));return L"); add("var f=Q('f',function(){});f(Q('a',1),Q('b',2),Q('c',3))");
  add("var o=Tr('o',{f(){return this===o}});return o.f(Q('a'))"); add("return [Q('a',1),,Q('b',2)]"); add("return {[Q('k1','a')]:Q('v1',1),[Q('k2','b')]:Q('v2',2)}");
  add("return Q('a',1)+Q('b',2)*Q('c',3)"); add("return Q('a',0)&&Q('b',1)||Q('c',2)"); add("return Q('a',null)??Q('b',1)"); add("return (Q('a',0)||Q('b',null))??Q('c',3)");
  add("return Q('c',1)?Q('t',2):Q('e',3)"); add("return (Q('a',1),Q('b',2),Q('c',3))"); add("return [...It('i',[1,2]),Q('b',3)]"); add("return Math.max(Q('a',1),...It('i',[5]),Q('b',2))");
  add("return {...Tr('s',{a:1,b:2}),c:Q('c',3)}"); add("return {a:1,...null,...undefined,...'xy',...[7]}"); add("return {...{get a(){L.push('ga');return 1}},b:2}");
  add("return [...'ab',...new Set([1])]"); add("return [...{}]"); add("return [...null]"); add("return Math.max(...undefined)"); add("return Math.max(...5)");
}

// ---------------------------------------------------------------- A3. Optional chaining, comma, typeof/delete/void.
{
  add("var o={f(){return this===o}};return [o?.f(),(o?.f)(),o?.['f'](),(o.f)(),(0,o.f)()]");
  add("var o=null;return [o?.a,o?.[Q('k')],o?.a.b.c,o?.a(Q('arg')),o?.a?.b]"); add("var o={a:null};return [o.a?.b.c.d,o?.a?.[Q('k')],o.a?.()]");
  add("var o={a:{b(){return this===o.a}}};return [o.a?.b(),o?.a.b(),(o?.a).b()]"); add("var o=null;return delete o?.a"); add("var o={a:1};return [delete o?.a,o]");
  add("var o=null;return typeof o?.a"); add("var f=null;return f?.(Q('a'))"); add("var f=function(){return this};return f?.()===globalThis");
  add("'use strict';var f=function(){return this};return f?.()"); add("var o=Tr('o',{a:{b:1}});return o?.a?.b");
  add("var o={};return o?.a.b"); add("return 1?.a"); add("return ''?.length"); add("var a=1;a?.b=1"); add("new a?.b()"); add("var a=1;a?.b`x`"); add("var a=1;return a?.5:1");
  add("return 1?.5:2"); add("var a={};return a?.[0]?.[1]"); add("var a;return a?.b.c.d.e.f.g"); add("var o={a:undefined};return (o?.a).b");
  add("return typeof undeclared"); add("return typeof typeof 1"); add("return void Q('a')"); add("return delete 1"); add("return delete undeclaredName"); add("'use strict';return delete undeclaredName");
  add("var o={};Object.defineProperty(o,'a',{value:1});return delete o.a"); add("'use strict';var o={};Object.defineProperty(o,'a',{value:1});return delete o.a");
  add("var o=Tr('o',{a:1});return [delete o[Q('k','a')],o.a]"); add("return delete [][Q('k','length')]"); add("'use strict';return delete [].length");
  add("var a=1;return delete a"); add("return delete (0,1)"); add("'use strict';return delete this"); add("var x=1;return [x++,x--,++x,--x,x]");
  add("return typeof tdz;let tdz=1"); add("return tdz;let tdz=1"); add("tdz=1;let tdz"); add("let a=a;"); add("const c=c+1"); add("var f=()=>g;let g=1;return f()");
  add("var f=()=>g;try{f()}catch(e){L.push(e.message)}let g=1;return f()"); add("{function f(){return 1}}return typeof f"); add("'use strict';{function f(){return 1}}return typeof f");
  add("return typeof f;function f(){}var f=1"); add("var f=1;function f(){}return typeof f"); add("return typeof f;{function f(){}}"); add("if(1)function f(){}return typeof f");
  add("return this===globalThis"); add("'use strict';return this"); add("return (function(){return this===globalThis})()"); add("return (function(){'use strict';return this})()");
  add("return (function(){return typeof this}).call(1)"); add("return (function(){'use strict';return typeof this}).call(1)"); add("return (()=>this===globalThis)()");
  add("with({a:1}){return a}"); add("'use strict';with({a:1}){return a}"); add("var o={a:1,[Symbol.unscopables]:{a:true}};var a=2;with(o){return a}");
  add("var o=Tr('o',{a:1});with(o){a}return L"); add("var o=Tr('o',{a:1});with(o){a=2}return L"); add("var o=Tr('o',{a:1});with(o){typeof a}return L");
  add("var o=Tr('o',{});with(o){typeof zz}return L"); add("var o=Tr('o',{a:1});with(o){delete a}return L");
}

// ---------------------------------------------------------------- B1. Destructuring de arrays.
const contexts = {
  decl: (pat, vars, src) => `let ${pat}=${src};return [${vars}]`,
  assign: (pat, vars, src) => `let ${vars || "_"};(${pat}=${src});return [${vars}]`,
  param: (pat, vars, src) => `return ((${pat})=>[${vars}])(${src})`,
  forof: (pat, vars, src) => `for(let ${pat} of [${src}]){return [${vars}]}`,
  catch_: (pat, vars, src) => `try{throw ${src}}catch(${pat}){return [${vars}]}`,
};
{
  const patterns = [
    ["[a]", "a"], ["[a,b]", "a,b"], ["[a,,b]", "a,b"], ["[,]", ""], ["[]", ""], ["[...r]", "r"], ["[a,...r]", "a,r"], ["[a,b,...r]", "a,b,r"],
    ["[a=Q('d1',9)]", "a"], ["[a=Q('d1',9),b=Q('d2',8)]", "a,b"], ["[a=b,b=1]", "a,b"], ["[[x]]", "x"], ["[{x}]", "x"],
    ["[x,[y]=It('n',[5,6])]", "x,y"], ["[...[a,b]]", "a,b"], ["[...{length:a}]", "a"], ["[a=function(){}]", "a"], ["[a=class{}]", "a"],
    ["[a=Q('d',()=>{})]", "a"], ["[,,a]", "a"], ["[a,,]", "a"],
  ];
  const sources = [
    "It('i',[1,2,3])", "It('i',[1])", "It('i',[])", "It('i',[1,2],'noReturn')", "It('i',[1,2,3],'badReturn')", "It('i',[1,2,3],'throwReturn')",
    "It('i',[1,2],'throwNext')", "5", "null", "undefined", "{}", "'ab'", "new Set([1,2])", "[1,2,3]", "[,1]", "[undefined,undefined]",
    "(function*(){try{yield 1;yield 2}finally{L.push('gfin')}})()", "It('i',[[7],{x:8}])",
  ];
  patterns.forEach(([pat, vars], i) =>
    sources.forEach((src, j) => {
      thin(6, contexts.decl(pat, vars, src));
      thin(10, contexts.assign(pat, vars, src));
      thin(7, contexts.param(pat, vars, src));
      thin(11, contexts.forof(pat, vars, src));
      thin(13, contexts.catch_(pat, vars, src));
    }),
  );
  // Alvos que são membros, erro no meio (IteratorClose), defaults que lançam.
  add("var o=Tr('o',{});[o.p,o[Q('k','q')]]=It('i',[1,2,3]);return L");
  add("var o=Tr('o',{});[o[Q('k1','p')],o[Q('k2','q')]]=It('i',[1]);return L");
  add("var o={set p(v){throw new RangeError('set')}};[o.p]=It('i',[1,2]);return L");
  add("var o={set p(v){throw new RangeError('set')}};[o.p]=It('i',[]);return L");
  add("var o={set p(v){throw new RangeError('set')}};try{[o.p]=It('i',[1],'throwReturn')}catch(e){return [L,e.message]}");
  add("[Q('t',{}).p=Q('d',5)]=It('i',[])");
  add("var a;[a=(()=>{throw new RangeError('def')})()]=It('i',[undefined,2]);");
  add("var a;try{[a=(()=>{throw new RangeError('def')})()]=It('i',[undefined,2],'throwReturn')}catch(e){return [L,e.message]}");
  add("var a,b;[a,b=Q('d',2)]=It('i',[1,undefined]);return [a,b,L]");
  add("var a,b;[a=Q('d1',1),b=Q('d2',2)]=It('i',[0,null]);return [a,b,L]");
  add("let a,b;[a,b]=[b,a]=[1,2];return [a,b]"); add("var a=1,b=2;[a,b]=[b,a];return [a,b]");
  add("var r=[];[r[r.length],r[r.length]]=It('i',[1,2]);return r"); add("var x;[x]=(Q('src',[1]));return [x,L]");
  add("var a;var r=([a]=[1,2,3]);return [r,a]"); add("var a;var r=([a]=It('i',[1,2,3]));return [r===undefined,a]");
  add("var it=It('i',[1,2,3]);var [a]=it;var [b]=it;return [a,b,L]"); add("var [a,[b]]=[1,[2]];return [a,b]"); add("var [a,[b]]=[1];");
  add("var [a,{b}]=[1,{}];return [a,b]"); add("var [a,{b}]=[1,null];"); add("var [{a}]=[undefined];"); add("var {a}=null;"); add("var {a}=undefined;"); add("var {}=null;");
  add("var [...[a,...b]]=[1,2,3];return [a,b]"); add("var [a,...b]=It('i',[1,2,3]);return [a,b,L]"); add("var [...a]=It('i',[1,2,3],'throwNext');");
  add("function*g(){var [a,b]=yield;return [a,b]};var it=g();it.next();return it.next([1,2]).value"); add("var [a=1]=[null];return a"); add("var [a=1]=[undefined];return a");
  add("var [a=1]=[,];return a"); add("var [a=b,b]=[];"); add("var [a=a]=[];"); add("var a=1;{var [a=a]=[]}return a"); add("let [a=(()=>a)()]=[];");
  add("var [a=function(){},b=()=>{},c=class{},d=class{static name='x'},e=(function(){})]=[];return [a.name,b.name,c.name,d.name,e.name]");
  add("var [a,b]=It('i',[1,2,3],'throwReturn');"); add("var [a]=It('i',[1],'badReturn');return L"); add("var [a]=It('i',[1,2],'badReturn');");
  add("var [a]=It('i',[1,2],'noReturn');return L"); add("var it={[Symbol.iterator](){return {next(){return 1}}}};var [a]=it;"); add("var it={[Symbol.iterator](){return 1}};var [a]=it;");
  add("var it={[Symbol.iterator]:1};var [a]=it;"); add("var it={[Symbol.iterator](){return {}}};var [a]=it;"); add("var it={[Symbol.iterator](){return {next:1}}};var [a]=it;");
  add("var it={[Symbol.iterator](){return {next(){return {done:true,get value(){L.push('val');return 1}}}}}};var [a]=it;return [a,L]");
  add("var it={[Symbol.iterator](){return {next(){return {done:false,get value(){L.push('val');return 1},get done(){L.push('done');return false}}}}}};var [a]=it;return L");
  add("var [a]=[1],[b]=[a+1];return [a,b]"); add("for (var [a,b] of [[1,2],[3,4]]){} return [a,b]"); add("var r=[];for (var [a,b=a] of [[1],[3,4]]) r.push(a+b);return r");
  add("var r=[];for ([a,b] of [[1,2]]) r.push(a);var a,b;return r"); add("var r=[];for (let [a,b] of It('i',[[1,2],[3]])) r.push(b);return [r,L]");
  add("var r=[];for (let [a] of It('i',[[1],[2]])) {r.push(a);break}return [r,L]"); add("for (let [a] of [null]);"); add("for (let {a} of [undefined]);");
}

// ---------------------------------------------------------------- B2. Destructuring de objetos.
{
  const patterns = [
    ["{a}", "a"], ["{a,b}", "a,b"], ["{a:b}", "b"], ["{a=Q('d',1)}", "a"], ["{a:b=Q('d',1)}", "b"], ["{[Q('k','a')]:a}", "a"], ["{a:{b}}", "b"],
    ["{...r}", "r"], ["{a,...r}", "a,r"], ["{[Q('k','a')]:x,...r}", "x,r"], ["{a,b,...r}", "a,b,r"], ["{}", ""], ["{a:[b]}", "b"],
    ["{'a':a}", "a"], ["{1:a}", "a"], ["{a=b,b=1}", "a,b"], ["{a:b=a}", "b"], ["{[Q('k1','a')]:x=Q('d',5),[Q('k2','b')]:y}", "x,y"],
    ["{length}", "length"], ["{[Symbol.iterator]:a}", "a"], ["{a=function(){}}", "a"],
  ];
  const sources = [
    "Tr('s',{a:1,b:2})", "{a:1,b:2,c:3}", "Tr('s',{a:undefined})", "null", "undefined", "5", "'str'", "{get a(){L.push('ga');return 1}}",
    "Object.create({a:1})", "{[Symbol.iterator]:7,a:2}", "Tr('s',Object.create(Tr('p',{a:1})))", "[9,8]", "()=>{}", "true", "Symbol()", "1n",
  ];
  patterns.forEach(([pat, vars], i) =>
    sources.forEach((src, j) => {
      thin(6, contexts.decl(pat, vars, src));
      thin(10, contexts.assign(pat, vars, src));
      thin(9, contexts.param(pat, vars, src));
      thin(13, contexts.forof(pat, vars, src));
      thin(11, contexts.catch_(pat, vars, src));
    }),
  );
  add("var o=Tr('o',{});({a:o.p,b:o[Q('k','q')]}=Tr('s',{a:1,b:2}));return L");
  add("var o=Tr('o',{});({[Q('k','a')]:o[Q('t','p')]=Q('d',3)}=Tr('s',{}));return L");
  add("var o={set p(v){throw new RangeError('set')}};({a:o.p}={a:1});"); add("var a,b;({a,b}={a:1});return [a,b]"); add("var a,b;var r=({a,b}={a:1,b:2});return Object.keys(r)");
  add("var a,r;({a,...r}=Tr('s',{a:1,b:2,c:3}));return [a,r,L]"); add("var r;({...r}=Tr('s',Object.create({z:1},{a:{value:1,enumerable:true},b:{value:2}})));return [r,L]");
  add("var r;({...r}={[Symbol.for('z')]:1,a:2});return [r,Reflect.ownKeys(r)]"); add("var r;({...r}='ab');return r"); add("var r;({...r}=5);return r"); add("var r;({...r}=null);");
  add("var o={};({...o.p}={a:1});return o"); add("var o={};({...o[Q('k','p')]}={a:1});return [o,L]"); add("var {a,...r}={get a(){L.push('ga');return 1},get b(){L.push('gb');return 2}};return [a,r,L]");
  add("var {[Q('k1','a')]:a,[Q('k2','b')]:b}={a:1,b:2};return [a,b,L]"); add("var {[(Q('k','a'),'a')]:a=Q('d',9)}={};return [a,L]"); add("var {a:{b}}={};");
  add("var {a:{b}}={a:null};"); add("var {a:[b]}={a:{}};"); add("var {a:{b}={b:4}}={};return b"); add("var {[{toString(){L.push('ts');return 'a'}}]:a}={a:5};return [a,L]");
  add("var {[Symbol.iterator]:a}=[];return typeof a"); add("var {length}='abc';return length"); add("var {0:a,length:l}='xyz';return [a,l]"); add("var {toString}=1;return toString===Number.prototype.toString");
  add("var {a}=Object.create(null);return a"); add("var {a=1}={a:null};return a"); add("var {a=1}={a:undefined};return a"); add("var {a=1}={};return a"); add("var {a=b,b}={};");
  add("var {a=(()=>{throw new RangeError('def')})()}={a:undefined};"); add("var {a=Q('d1',1),b=Q('d2',2)}={};return [a,b,L]"); add("var {a,a:b}={a:3};return [a,b]");
  add("var {a:a,a:a}={a:3};return a"); add("let {a,a:b}={a:3};return [a,b]"); add("let {a,a}={a:3};"); add("let [a,a]=[1,2];"); add("let [a,...a]=[1,2];"); add("var [a,...b,c]=[];");
  add("var {...a,b}={};"); add("var {...{a}}={};"); add("var {...[a]}={};"); add("var [...a=1]=[];"); add("var [...a,]=[];"); add("var {a,}={a:1};return a"); add("var [a,,]=[1,2,3];return a");
  add("({a}={})=>1"); add("({a:1}={})"); add("({a}) = {}"); add("[a,b]+=1"); add("[a]++"); add("({a})++"); add("({a:b.c}=1)"); add("({a:(b)}={a:1});var b"); add("[(a)]=[1];var a");
  add("({a:(b.c)}={a:1})"); add("[(a=1)]=[]"); add("[(a,b)]=[]"); add("var o={};[o.a=1]=[];return o"); add("var x;[x=1,...[x]]=[];return x"); add("var f=function(){};[f.name]=['x'];return f.name");
  add("'use strict';var [eval]=[1];"); add("'use strict';[arguments]=[1];"); add("'use strict';({eval}={});"); add("'use strict';var {a:eval}={};"); add("var [yield]=[1];return yield");
  add("function*g(){var [yield]=[1]}"); add("async function f(){var [await]=[1]}"); add("var [await]=[1];return await"); add("let [let]=[1]"); add("var [let]=[1];return let");
  add("var {if:a}={if:1};return a"); add("var {if}={if:1};"); add("var {await}={await:1};return await"); add("var {async,get,set,of,static}={};return [async,get,set,of,static]");
}

// ---------------------------------------------------------------- B3. Parâmetros default, rest, arguments, escopo.
{
  add("function f(a=Q('d1',1),b=Q('d2',a+1)){return [a,b]}return [f(),f(5),f(undefined,undefined),f(null,null),L]");
  add("function f(a=b,b=1){return [a,b]}return f()"); add("function f(a=b,b=1){return [a,b]}return f(1)"); add("function f(a=a){}return f()"); add("function f(a=a){}return f(1)");
  add("function f(a,b=a){var a=2;return [a,b]}return f(1)"); add("function f(a,g=()=>a){var a=2;return [a,g()]}return f(1)"); add("function f(a,g=()=>a){a=2;return [a,g()]}return f(1)");
  add("function f(a,g=()=>a){var a;return [a,g()]}return f(1)"); add("function f(a=1){var a;return a}return f()"); add("function f(a=1){var a=a;return a}return f(3)");
  add("function f(a=1){function a(){}return typeof a}return f()"); add("function f(a=1){function a(){}return typeof a}return f(2)"); add("function f(g=()=>x){var x=1;return g()}return f()");
  add("var x='outer';function f(g=()=>x){var x='inner';return g()}return f()"); add("var x='outer';function f(a=x){var x='inner';return a}return f()");
  add("function f(a=eval('var z=1;z')){return [a,typeof z]}return f()"); add("function f(a=eval('var a2=1')){var a2=2;return a2}return f()");
  add("function f(a=arguments.length){return a}return [f(),f(undefined),f(1,2)]"); add("function f(a=1){arguments[0]=9;return a}return f(2)"); add("function f(a){arguments[0]=9;return a}return f(2)");
  add("function f(a){a=9;return arguments[0]}return f(2)"); add("function f(a){'use strict';a=9;return arguments[0]}return f(2)"); add("function f(a,...r){a=9;return arguments[0]}return f(2)");
  add("function f(a,{b}={}){a=9;return arguments[0]}return f(2)"); add("function f(a){a=9;return arguments.length}return f()"); add("function f(a,b){b=9;return arguments[1]}return f(1)");
  add("function f(a,a){return a}return f(1,2)"); add("function f(a,a){'use strict'}"); add("function f(a,a=1){}"); add("(a,a)=>1"); add("function f([a],a){}"); add("function f(a,...a){}");
  add("return [(function(a,b){}).length,(function(a,b=1){}).length,(function(a=1,b){}).length,(function(...a){}).length,(function({a},[b]){}).length,(function(a,{b}={}){}).length]");
  add("return [((a,b)=>{}).length,((a=1)=>{}).length,((...a)=>{}).length,(async function(a,b=1){}).length,(class{constructor(a,b){}}).length]");
  add("function f(a,b=1,c){}return f.length"); add("function f(...a){return a}return [f(),f(1,2)]"); add("function f(a,...[b,c]){return [a,b,c]}return f(1,2,3,4)"); add("function f(...{length}){return length}return f(1,2,3)");
  add("function f({a,b}={a:1,b:2}){return [a,b]}return [f(),f({a:5}),f(undefined)]"); add("function f({a,b}={a:1,b:2}){return [a,b]}return f(null)"); add("function f([a,b]=[1,2]){return [a,b]}return [f(),f([5])]");
  add("function f([a]=It('i',[7])){return a}return [f(),L]"); add("function f(a,b=Q('d',2)){return b}return f(1,3)"); add("function f(a=Q('d1',1),{b=Q('d2',2)}={}){return [a,b]}return [f(),L]");
  add("function f(a=(()=>{throw new RangeError('p')})()){}f()"); add("function f(a=(()=>{throw new RangeError('p')})()){}try{f()}catch(e){return e.message}"); add("function f(a=this){return a}return f.call(7)===7");
  add("'use strict';function f(a=this){return a}return f.call(7)"); add("function f(a=new.target){return a}return [f(),new f()===undefined]"); add("function f(a=f){return a===f}return f()");
  add("var f=(a=1)=>a;return [f(),f(2)]"); add("var f=(a,b=a)=>[a,b];return f(3)"); add("var f=({a}={})=>a;return [f(),f({a:1})]"); add("var f=(a=arguments)=>a;return f()");
  add("function g(){return (a=arguments[0])=>a}return g(5)()"); add("function g(){return (a=arguments[0])=>a}return g(5)(6)"); add("var f=async(a=Q('d',1))=>a;var p=f();return L");
  add("function*g(a=Q('d',1)){yield a}var it=g();return [L,it.next()]"); add("function*g(a=(()=>{throw new RangeError('gp')})()){}try{g()}catch(e){return e.message}"); add("async function f(a=(()=>{throw new RangeError('ap')})()){}var p=f();return p instanceof Promise");
  add("class C{constructor(a=Q('d',1)){this.a=a}m(b=Q('e',2)){return b}}return [new C().a,new C().m(),L]"); add("class C{static m(a=C){return a===C}}return C.m()"); add("var o={m(a=1){return a}};return o.m()");
  add("function f(a){var a;return a}return f(1)"); add("function f(a){var a=2;return a}return f(1)"); add("function f(a){let a}"); add("function f(a){{let a=2}return a}return f(1)"); add("function f(a=1){let a}");
  add("function f(){let arguments=1;return arguments}return f()"); add("function f(a=1){'use strict'}"); add("function f(a=1){'use strict'}return 1"); add("function f(a,b){'use strict'}return f.length");
  add("function f(a){return arguments.callee===f}return f()"); add("function f(a){'use strict';return arguments.callee}f()"); add("function f(){return Object.prototype.toString.call(arguments)}return f()");
  add("function f(a,b){return Object.getOwnPropertyNames(arguments)}return f(1)"); add("function f(a,b){return [arguments.length,Object.keys(arguments)]}return f(1,2,3)");
  add("function f(a){delete arguments[0];arguments[0]=5;return a}return f(1)"); add("function f(a){Object.defineProperty(arguments,'0',{writable:false});a=3;return arguments[0]}return f(1)");
  add("function f(a){return arguments[Symbol.iterator]===Array.prototype.values}return f()"); add("function f(a=1){return arguments[Symbol.iterator]===Array.prototype.values}return f()");
}

// ---------------------------------------------------------------- Execução.
const programs = pool.resolve();
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "language-gap-golden-"));
const file = path.join(dir, "language_gap_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  fs.writeFileSync(file, body);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(body) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
