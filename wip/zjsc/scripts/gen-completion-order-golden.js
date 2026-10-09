// Gera tests/golden/completion_order_bun.tsv: valores de completion de eval/Function, ordem de avaliação, hoisting,
// getters e setters em `with`, labels com continue em finally e generators com return/throw em finally aninhado,
// medidos no bun 1.4.2. Cobre completion de if/switch/try/loops/labels (sozinhos, prefixados, aninhados em try/finally,
// do-while e labels), ordem de operandos e de ToPrimitive, atribuição composta com getters, optional chaining com
// delete e chamadas, `new a.b()`, vírgula, exponenciação, template tags com cache de strings e raw, TDZ em switch,
// for e parâmetros default, function em bloco do Annex B, `with` com acessores, Symbol.unscopables e Proxy, produto
// combinatório de loops x try/catch/finally x break/continue/return, generators com finally aninhado e arrows async
// com this/arguments. Programas cuja fonte já aparece nos goldens vizinhos são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem API de host, e `R` é lido depois de esvaziar as microtarefas.
// Uso: bun scripts/gen-completion-order-golden.js > tests/golden/completion_order_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, stepSampler, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  // O setTimeout fica fora do programa: só serve para as microtarefas esvaziarem antes de ler `R`.
  setTimeout(() => {
    process.exit(0);
  }, 0);
  return;
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'var L=[];function l(x){L.push(x);return x}\n' +
  'function W(f){L.length=0;var r;try{r="="+S(f())}catch(e){r="!"+(e&&e.name)}return L.join()+"|"+r}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)}}\n' +
  'function E(src){return W(()=>(0,eval)(src))}\n' +
  'function F(body){return W(()=>new Function(body)())}\n';

const pool = stepSampler();
const add = (...list) => pool.push(1, ...list);
const q = JSON.stringify;
// Famílias combinatórias grandes entram amostradas (1 a cada `n`), de forma determinística, para o golden não crescer demais.
// A escolha é por hash do texto (`sampleByHash` dentro do `stepSampler`), separada por família, nunca por contador.
const addEvery = (family, n, ...list) => pool.pushIn(family, n, ...list);
const bt = s => s.replace(/§/g, "`");

// ---- 1. Valores de completion.
const stmts = [
  "if(true){2}", "if(false){2}", "if(true){}", "if(true);", "if(false);else 4", "if(0)2;else;", "if(1)2;else 3", "if(0)2;else 3", "if(1){2}else{3}",
  "if(true){2;if(false){3}}", "if(true){2;if(true){}}", "if(true){var zz=2}", "if(true)var zz=2", "if(1){}else{3}",
  "do{2;break}while(0)", "do{break}while(0)", "do{2;continue}while(false)", "do 2; while(false)", "do{}while(false)", "do{2}while(false)",
  "while(false);", "while(true){2;break}", "while(true){break}", "var n=0;while(n++<2){n+10}", "var n=0;while(n++<3){if(n==2)continue;n+10}",
  "var n=0;while(n++<3){if(n==2)break;n+10}", "var n=0;while(n++<3){n+10;if(n==2)break}", "var n=0;while(n++<3){n+10;if(n==2)continue}",
  "for(;;){3;break}", "for(;;){break}", "for(var i=0;i<2;i++);", "for(var i=0;i<3;i++){i}", "for(var i=0;i<3;i++){if(i==1)break;i}",
  "for(var i=0;i<3;i++){if(i==1)continue;i+10}", "for(var i=0;i<3;i++){i+10;if(i==1)continue}", "for(var i=0;i<3;i++){i+10;if(i==1)break}",
  "for(let i=0;i<2;i++){i+5}", "for(var k in {a:1,b:2}){k}", "for(var k in {}){k}", "for(var k in {a:1,b:2}){k;break}", "for(var k in {a:1,b:2}){break}",
  "for(var k in {a:1,b:2}){if(k=='b')continue;k+'!'}", "for(var v of [4,5]){v}", "for(var v of [4,5]){v;break}", "for(var v of [4,5]){break}",
  "for(var v of []){v}", "for(let v of [4,5,6]){if(v==5)continue;v}", "for(const v of [4,5,6]){v;if(v==5)break}",
  "switch(1){case 1:2;case 2:3}", "switch(1){case 1:2;break;case 2:3}", "switch(9){case 1:2}", "switch(1){case 1:}", "switch(1){case 1:5;default:}",
  "switch(2){default:7;case 1:8}", "switch(3){default:7;case 1:8}", "switch(1){}", "switch(1){default:}", "switch(1){case 1:2;case 2:}",
  "switch(1){case 1:2;case 2:break}", "switch(2){case 1:2;case 2:{}}", "switch(1){case 1:{3}case 2:4}", "switch(1){case 1:if(true){6}}",
  "switch(1){case 1:var u=1}", "switch(1){case 1:l:{8;break l}}", "switch(1){case 0:1;default:2;case 3:}",
  "try{2}finally{3}", "try{2}catch(e){3}", "try{throw 1}catch(e){3}", "try{throw 1}catch(e){}", "try{throw 1}catch(e){3}finally{4}",
  "try{}finally{3}", "try{2}catch(e){3}finally{}", "try{throw 1}catch(e){3}finally{}", "try{try{2}finally{3}}finally{4}", "try{try{throw 1}finally{3}}catch(e){5}",
  "do{try{2;break}finally{3}}while(0)", "do{try{2;continue}finally{3}}while(false)", "do{try{2}finally{break}}while(0)", "do{try{2}finally{3;break}}while(0)",
  "do{try{2;break}finally{break}}while(0)", "do{try{2}finally{continue}}while(false)", "do{try{throw 1}finally{break}}while(0)",
  "do{try{throw 1}catch(e){4;break}finally{3}}while(0)", "do{try{throw 1}catch(e){4}finally{3;break}}while(0)",
  "l:{2;break l}", "l:{break l}", "l:{2;break l;3}", "l:2", "a:b:3", "l:{l2:{4;break l}5}", "l:{4;l2:{break l2}}", "l:if(true){2}", "l:do{2;break l}while(0)",
  "l:for(var i=0;i<2;i++){i+7;continue l}", "l:for(var i=0;i<2;i++){i+7;break l}", "l:for(var i=0;i<2;i++){for(;;){i+7;continue l}}",
  "l:for(var i=0;i<2;i++){for(;;){break l}}", "l:{try{2;break l}finally{3}}", "l:{try{2}finally{3;break l}}",
  "with({}){2}", "with({}){}", "with({a:5}){a}", "with({a:5}){var wv=a}", "with({}){if(true){6}}", "with({}){try{7}finally{8}}",
  "var x=5", "function f(){}", "function f(){}7", "class C{}", "class C{}8", "let q=3", "const c=3", "{}", "{2}", ";", "{;}", "{{}}", "{2;{}}", "{2;{3}}",
  "1;{}", "debugger", "2;debugger", "var a1=1;var a2=2", "2;var a1=1", "2;function g(){}", "2;class K{}", "2;let r=1", "2;;", "2;{;}",
  "(function(){})", "x=2", "x=2;x++", "x=2;++x", "typeof 1", "void 0", "'use strict'", "'a';'b'", "2;'use strict'", "-0", "0n", "[1,2]", "({a:1})",
  "2;if(true){}", "2;if(false){}", "2;while(false);", "2;do{}while(false)", "2;for(;false;);", "2;for(var z in{});", "2;for(var z of[]);", "2;switch(1){}",
  "2;try{}catch(e){}", "2;try{}finally{}", "2;l:{}", "2;with({}){}", "2;{}", "2;var f2", "2;let g2",
  "for(var i=0;i<3;i++){switch(i){case 0:'a';break;case 1:'b';continue;default:'c'}}", "for(var i=0;i<3;i++){switch(i){case 0:'a';break;case 1:continue;default:'c'}}",
  "for(var i=0;i<3;i++){try{i+1;continue}finally{i+8}}", "for(var i=0;i<3;i++){try{i+1;break}finally{}}", "for(var i=0;i<3;i++){try{throw i}catch(e){e+20;continue}}",
  "for(var i=0;i<3;i++){try{throw i}catch(e){e+20;break}}", "for(var i=0;i<3;i++){try{i}finally{if(i==1)break}}", "for(var i=0;i<3;i++){try{i}finally{if(i==1)continue;i+9}}",
  "for(var v of [1,2,3]){try{v+1;continue}finally{v+8}}", "for(var v of [1,2,3]){try{v}finally{if(v==2)break}}",
  "for(var k in {a:1,b:2}){try{k;continue}finally{k+'f'}}", "for(var k in {a:1,b:2}){try{k}finally{break}}",
  "if(true){3}else{4}", "if(true){}", "1;if(true){}else{4}", "l1:if(true){2;break l1}", "l1:if(true){break l1}", "3;l1:if(true){break l1}",
  "var i=0;do{i++;if(i==2)continue;i+10}while(i<3)", "var i=0;do{i++;if(i==2)break;i+10}while(i<3)", "var i=0;do{i+10;i++}while(i<3)",
  "switch(1){case 1:for(var i=0;i<2;i++){i+3}}", "switch(1){case 1:for(var i=0;i<2;i++){break}}", "switch(1){case 1:do{4;break}while(0);case 2:}",
];
// Embrulhos: o completion de um statement muda quando ele fica dentro de outro.
const wrappers = [
  s => s, s => "7;" + s, s => "{" + s + "}", s => "7;{" + s + "}", s => "if(true){" + s + "}", s => "7;if(true){" + s + "}",
  s => "try{" + s + "}finally{9}", s => "try{" + s + "}catch(e){9}", s => "do{" + s + "}while(false)", s => "7;do{" + s + "}while(false)",
  s => "l9:{" + s + "}", s => "with({}){" + s + "}", s => "switch(1){case 1:" + s + "}", s => s + ";8", s => s + ";{}",
  s => "for(var wi=0;wi<1;wi++){" + s + "}", s => "for(var wk of[1]){" + s + "}", s => "if(false){}else{" + s + "}",
];
for (const s of stmts) for (const w of wrappers) addEvery("completion", 4, `E(${q(w(s))})`);
for (const s of stmts) add(`E(${q(s)})`);
// Os mesmos corpos por new Function (sem valor de completion: return explícito) e por eval direto dentro de função.
for (const s of stmts.slice(0, 120)) {
  add(`W(()=>{var loc=1;return eval(${q("loc;" + s)})})`);
  add(`F(${q("var r=eval(" + q("1;" + s) + ");return r")})`);
}
add(
  "E('1; if(true){2}')", "W(()=>eval('1; if(true){2}'))", "W(()=>eval('3;'))", "W(()=>eval(''))", "W(()=>eval('   '))", "W(()=>eval('//c'))", "W(()=>eval(7))", "W(()=>eval({}))",
  "W(()=>eval(new String('1')))", "W(()=>eval('var a=1'))", "W(()=>eval('a:1'))", "W(()=>eval('1;a:{2;break a}'))", "W(()=>(0,eval)('1;do{2;break}while(0)'))",
  "W(()=>eval('({}).x'))", "W(()=>eval('{}'))", "W(()=>eval('({})'))", "W(()=>eval('{a:1}'))", "W(()=>eval('{a:1,b:2}'))", "W(()=>eval('[]'))", "W(()=>eval('function(){}'))",
  "W(()=>eval('(function(){})'))", "W(()=>eval('async function f(){}'))", "W(()=>eval('1,2'))", "W(()=>eval('1;2,3'))", "W(()=>eval('var x=1;x'))", "W(()=>eval('let y=1;y'))",
  "W(()=>eval('let y=1;y;'))", "W(()=>eval('const y=1;y;;'))", "W(()=>eval('class A{}; new A'))", "W(()=>eval('throw 1'))", "W(()=>eval('1;throw 1'))",
  "W(()=>eval('return 1'))", "W(()=>eval('break'))", "W(()=>eval('continue'))", "W(()=>eval('yield 1'))", "W(()=>eval('await 1'))", "W(()=>eval('new.target'))", "W(()=>eval('super.x'))",
  "W(()=>eval('this'))", "W(()=>eval('arguments.length'))", "W(()=>{'use strict';return eval('var s=1;s')})", "W(()=>{'use strict';eval('var s=1');return typeof s})",
  "W(()=>{eval('var s=1');return typeof s})", "W(()=>{(0,eval)('var s2=1');return typeof s2})", "W(()=>{var e=eval;e('var s3=1');return typeof s3})", "W(()=>eval?.('1;2'))",
  "W(()=>{var eval2=eval;return eval2('typeof this')})", "W(()=>Function('1;2')())", "W(()=>Function('return 1;2')())", "W(()=>Function('1')())", "W(()=>Function('return eval(\"3;4\")')())",
  "W(()=>Function('a','b','return a+b')(1,2))", "W(()=>Function('a,b','return a+b')(1,2))", "W(()=>Function('a','b=a','return b')(5))", "W(()=>Function('...r','return r.length')(1,2))",
  "W(()=>Function('}')())", "W(()=>Function('/*','*/){')())", "W(()=>Function('a','a','return a')(1,2))", "W(()=>Function('a','\"use strict\";return typeof this')())",
  "W(()=>Function('return typeof this')())", "W(()=>Function('\"use strict\";return typeof this')())", "W(()=>Function('return arguments.length')(1,2,3))",
  "W(()=>Function('return new.target')())", "W(()=>new Function('this.x=1').prototype.constructor===undefined)", "W(()=>Function('a','return a').toString())",
  "W(()=>Function('return 1').name)", "W(()=>Function('a','b','').length)", "W(()=>Function('a=1','b','').length)", "W(()=>Function('x','y','return x*y').call(null,3,4))",
  "W(()=>typeof Function('return this')())", "W(()=>Function('return this')()===globalThis)", "W(()=>Function('l(1);l(2)')())", "W(()=>Function('l(1);return l(2);l(3)')())",
);

// ---- 2. Ordem de avaliação.
const ops = ["+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "&", "|", "^", "<", ">", "<=", ">=", "==", "!=", "===", "!==", "in", "instanceof", "&&", "||", "??", ","];
const operands = [
  ["{valueOf(){l('a');return 2}}", "{valueOf(){l('b');return 3}}"], ["{toString(){l('a');return '2'}}", "{toString(){l('b');return '3'}}"],
  ["{[Symbol.toPrimitive](h){l('a'+h);return 2}}", "{[Symbol.toPrimitive](h){l('b'+h);return 3}}"], ["l('A')&&2", "l('B')&&3"],
  ["(l('A'),1)", "(l('B'),0)"], ["{valueOf(){l('a');return 1n}}", "2n"], ["{valueOf(){l('a');throw new RangeError}}", "{valueOf(){l('b');return 3}}"],
  ["null", "{valueOf(){l('b');return 3}}"], ["{valueOf(){l('a');return 3}}", "(()=>{throw new TypeError})()"], ["'k'", "{k:1}"],
];
for (const op of ops) for (const [a, b] of operands) addEvery('ops', 2, `W(()=>{var a=${a},b=${b};return a ${op} b})`, `W(()=>(${a}) ${op} (${b}))`);
// Atribuição composta: getter e setter em ordem, mais a chave computada.
const assignOps = ["+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=", "??="];
const lhsKinds = [
  "var o={get p(){l('get');return 5},set p(v){l('set'+v)}};o.p", "var o={get p(){l('get');return 0},set p(v){l('set'+v)}};o.p",
  "var o={get p(){l('get');return null},set p(v){l('set'+v)}};o.p", "var o={p:5};var k=()=>{l('key');return 'p'};o[k()]",
  "var o={get p(){l('get');return 5}};o.p", "var o={set p(v){l('set'+v)}};o.p", "var o=Object.freeze({p:5});o.p",
  "var o=null;o.p", "var o={p:{valueOf(){l('vo');return 2}}};o.p", "var o={p:2};var oo=()=>{l('obj');return o};oo().p",
  "var o={get p(){l('get');return 5},set p(v){l('set'+v)}};var pr=new Proxy(o,{get(t,k,r){l('pget');return Reflect.get(t,k,r)},set(t,k,v,r){l('pset');return Reflect.set(t,k,v,r)}});pr.p",
  "var a=[1,2];var i=()=>{l('i');return 0};a[i()]", "var x=2;x", "var x={valueOf(){l('x');return 2}};x",
];
const rhsKinds = ["l('rhs')&&3", "(l('rhs'),0)", "{valueOf(){l('rv');return 3}}", "undefined", "(()=>{throw new EvalError})()"];
for (const op of assignOps) for (const lhs of lhsKinds) for (const rhs of rhsKinds.slice(0, 3 + (op === "+=" ? 2 : 0))) {
  addEvery('assign', 2, `W(()=>{${lhs} ${op} ${rhs}})`);
}
// Dedução de leitura: x = x + ... com getters, ++ e -- em acessores.
for (const kind of ["get p(){l('get');return 5}, set p(v){l('set'+v)}", "get p(){l('get');return '5'}, set p(v){l('set'+typeof v)}", "get p(){l('get');return 5n}, set p(v){l('set'+v)}", "get p(){l('get');return {valueOf(){l('vo');return 5}}}, set p(v){l('set'+v)}", "get p(){l('get');return 'x'}, set p(v){l('set'+v)}"]) {
  for (const e of ["o.p++", "++o.p", "o.p--", "--o.p", "-o.p", "+o.p", "~o.p", "!o.p", "typeof o.p", "delete o.p", "void o.p", "o.p=o.p", "o.p=o.p+1", "o.p+=o.p", "o.p+=o.p++", "o.p=(o.p,2)", "[o.p]=[o.p]", "({q:o.p}=({q:3}))", "o.p?.toString()", "o?.p++"]) {
    addEvery('acc', 1, `W(()=>{var o={${kind}};return ${e}})`);
  }
}
add(
  "W(()=>{var a=[l('x'),l('y'),l('z')];return a})", "W(()=>{var o={[l('k1')]:l('v1'),[l('k2')]:l('v2')};return o})", "W(()=>{var o={a:l(1),a:l(2)};return o.a})",
  "W(()=>{var o={[l('k')]:l('v'),__proto__:l(null)};return Object.getPrototypeOf(o)})", "W(()=>({...(l('s1'),{a:1}),...(l('s2'),{b:2})}))",
  "W(()=>{var f=()=>{l('f');return (a,b)=>l(a+b)};return f()(l(1),l(2))})", "W(()=>{var o={m(){l('m');return this===o}};return o.m(l('arg'))})",
  "W(()=>{var o={get m(){l('getm');return function(){return l('call')}}};return o.m(l('arg'))})", "W(()=>{var o={};return o.m(l('arg'))})", "W(()=>{var o;return o.m(l('arg'))})",
  "W(()=>{var f;return f(l('arg'))})", "W(()=>{var o={};return o[l('k')](l('arg'))})", "W(()=>{var o;return o[l('k')](l('arg'))})", "W(()=>{var o=null;return o[l('k')]})",
  "W(()=>{var o=null;o[l('k')]=l('v')})", "W(()=>{var o=null;o.p=l('v')})", "W(()=>{var o=undefined;o[l('k')]+=l('v')})", "W(()=>{var o=null;return delete o[l('k')]})",
  "W(()=>{var o=null;return l('a'),o[{toString(){l('ts');return 'p'}}]})", "W(()=>{var o={};o[{toString(){l('ts');return 'p'}}]=l('v');return o.p})",
  "W(()=>{var o={};o[{toString(){l('ts');return 'p'}}]+=l('v');return o.p})", "W(()=>{var o={p:1};o[{toString(){l('ts');return 'p'}}]++;return o.p})",
  "W(()=>{var o={};var k={toString(){l('ts');return 'p'}};o[k]??=l('v');o[k]??=l('w');return o.p})", "W(()=>{var o={p:0};var k={toString(){l('ts');return 'p'}};o[k]||=l('v');return o.p})",
  "W(()=>{var f=function(a=l('d1'),b=l('d2')){return l('body')};return f(undefined,3)})", "W(()=>{var f=function(a,b=l('d')){};return f.length})",
  "W(()=>{var x=1;x=(l('rhs'),x+1);return x})", "W(()=>{var x=1;return x+(x=5)})", "W(()=>{var x=1;return (x=5)+x})", "W(()=>{var x=1;return x+x++ + x})", "W(()=>{var x=1;return x++ + x++ + x})",
  "W(()=>{var x=1;x+=x++;return x})", "W(()=>{var x=1;x+=(x=5);return x})", "W(()=>{var x=1;x=x++ + ++x;return x})", "W(()=>{var a=[0,0];var i=0;a[i++]=i++;return a.join()+i})",
  "W(()=>{var a=[0,0,0];var i=0;a[i++]+=i++;return a.join()+i})", "W(()=>{var a=[0,0,0];var i=0;a[i]=i=2;return a.join()+i})", "W(()=>{var a=[0,0,0];var i=0;a[i=1]=i;return a.join()})",
  "W(()=>{var o={p:1};var q=o;o.p=(o={p:9}).p+10;return q.p+','+o.p})", "W(()=>{var a={},b=a;a.x=a={};return typeof b.x+typeof a.x})",
  "W(()=>{var x=l(1),y=l(2),z=l(3);return [x,y,z]})", "W(()=>{var [a=l('da'),b=l('db')]=[undefined,2];return a+b})", "W(()=>{var {a=l('da'),b=l('db')}={b:2};return a+b})",
  "W(()=>{var {[l('k1')]:a,[l('k2')]:b}={x:1,y:2};return a})", "W(()=>{var o={};[o[l('k1')],o[l('k2')]]=[l('v1'),l('v2')];return Object.keys(o).join()})",
  "W(()=>{var o={};({a:o[l('k1')],b:o[l('k2')]}={a:l('v1'),b:l('v2')});return Object.keys(o).join()})", "W(()=>{var i=0;var o={};[o[l('k1')]=l('d')]=[];return Object.keys(o).join()})",
  "W(()=>l(1)?l(2):l(3))", "W(()=>l(0)?l(2):l(3))", "W(()=>(l(1),l(2),l(3)))", "W(()=>{var x;return (x=l(1),x+l(2))})", "W(()=>l(0)||l(0)||l(3))", "W(()=>l(1)&&l(2)&&l(0)&&l(4))",
  "W(()=>l(null)??l(0)??l(3))", "W(()=>(l(null)??l(0))||l(5))", "W(()=>l(1)||l(2)&&l(3))", "W(()=>(l(1),)=>1)", "W(()=>[l(1),,l(3)].length)", "W(()=>[...(l('a'),[1]),...(l('b'),[2])].length)",
  "W(()=>((a,b,c)=>a+b+c)(...(l('x'),[1,2]),l('y')))", "W(()=>Math.max(l(1),l(5),l(3)))", "W(()=>String(l(1))+String(l(2)))", "W(()=>`${l(1)}${l(2)}`)", "W(()=>`${{toString(){l('a');return 'A'}}}${{toString(){l('b');return 'B'}}}`)",
  "W(()=>`${{valueOf(){l('v');return 1},toString(){l('s');return 's'}}}`)", "W(()=>''+{valueOf(){l('v');return 1},toString(){l('s');return 's'}})", "W(()=>({valueOf(){l('v');return 1},toString(){l('s');return 's'}})+1)",
  "W(()=>`a${l(1)}b`+l(2))", "W(()=>l(1)+`${l(2)}`+l(3))", "W(()=>new Date(l(2020),l(1),l(2)).getFullYear())",
);
// optional chaining com delete, chamadas e this.
const chains = [
  "a?.b", "a?.b.c", "a?.b?.c", "a?.[l('k')]", "a?.b()", "a?.b.c()", "a?.b?.()", "a.b?.()", "a.b?.(l('arg'))", "a?.b(l('arg'))", "(a?.b)()", "(a?.b.c)()", "(a?.b).c", "(a?.b)?.c",
  "delete a?.b", "delete a?.b.c", "delete a?.[l('k')]", "delete a.b?.c", "delete (a?.b)", "delete a?.b?.c", "typeof a?.b", "void a?.b", "a?.b.c.d", "a?.b.c?.d.e", "a?.['b']['c']",
  "a?.b``", "a?.b.c``", "(a?.b)``", "new a.b()", "new a.b", "new (a.b)()", "new a?.b()", "new (a?.b)()", "a?.b?.[l('k')]", "a?.b[l('k')]", "a?.[l('k')].c", "a?.b.c ?? l('d')", "a?.b?.c ?? l('d')",
  "a?.b.call(a)", "a?.b.call?.(a)", "a.b.call(a)", "(a?.b).call(a)", "a?.b?.call(a)", "a?.#p", "a?.b.valueOf()", "a?.b++", "a?.b=1", "[a?.b]=[1]", "a?.b.c=1", "a?.b?.c=1",
  "(a?.b).c=1", "a?.b.c++", "a?.b+=1", "a?.b??=1", "a?.b&&=1",
];
const chainEnv = [
  "var a={b:{c:{d:{e:1}},m(){return this===a.b}}}", "var a=null", "var a=undefined", "var a={b:null}", "var a={b:undefined}", "var a={b:{c:null}}", "var a={b:{c(){return 'c'+(this===a.b)}}}",
  "var a={b(){return this===a}}", "var a={get b(){l('getb');return {c:2}}}", "var a={b:function B(){this.t=1}}", "var a={b:class B{constructor(){this.t=1}}}", "var a=Object.freeze({b:{c:1}})",
  "var a={b:{call(){return 'own'}}}", "var a={b:0}", "var a=0", "var a=''", "var a=Object.create(null)", "var a=new Proxy({b:{c:1}},{get(t,k,r){l('get '+String(k));return Reflect.get(t,k,r)},deleteProperty(t,k){l('del '+String(k));return Reflect.deleteProperty(t,k)}})",
];
for (const env of chainEnv) for (const c of chains) {
  if (c.includes("#p")) continue;
  addEvery('chain', 2, `W(()=>{${env};return ${c}})`);
}
add(
  "W(()=>{var a={b:{}};return delete a?.b.c})", "W(()=>{var a={b:{c:1}};return [delete a?.b.c,'c' in a.b]})", "W(()=>{var a={b:{c:1}};return [delete a.b?.c,'c' in a.b]})",
  "W(()=>{var a=null;return delete a?.b.c.d})", "W(()=>{var a=null;l('x');return delete a?.[l('k')]})", "W(()=>{'use strict';var a=Object.freeze({b:1});return delete a?.b})",
  "W(()=>{'use strict';var a=null;return delete a?.b})", "W(()=>{var a={b:1};return delete a?.b+','+a.b})", "W(()=>{var a={};return a?.b?.c?.d})", "W(()=>{var a={};return a.b?.c.d.e.f})",
  "W(()=>{var a={};return a.b?.[l('k')].c(l('arg'))})", "W(()=>{var a;return a?.[l('k')]})", "W(()=>{var a;return a?.b(l('arg'))})", "W(()=>{var a;return a?.(l('arg'))})", "W(()=>{var a=()=>1;return a?.(l('arg'))})",
  "W(()=>{var a=null;return a?.()})", "W(()=>{var a=0;return a?.()})", "W(()=>{var a=1;return a?.b})", "W(()=>{var a={b:()=>this===undefined};return a.b?.()})",
  "W(()=>{var a={b(){return this}};return [a.b?.()===a,(a.b)?.()===a,(a?.b)()===a,((a?.b))()===a,(0,a.b)()===globalThis||(0,a.b)()===undefined]})",
  "W(()=>{var a={b(){'use strict';return this}};return [a.b?.()===a,(a?.b)()===a,(0,a.b)()===undefined]})", "W(()=>{var o={f(){return this}};return [o?.f()===o,o?.['f']()===o,(o?.f)()===o,(o.f)()===o,(o?.f).call(1)===1]})",
  "W(()=>{var o={f(){return this}};return typeof (o?.f).call(1)})", "W(()=>{var s='str';return s?.length})", "W(()=>{var n=null;return n?.length??-1})", "W(()=>{var f=function(){return this};return f?.call(5)==5})",
  "W(()=>{class A{#p=1;static g(o){return o?.#p}}return [A.g(new A),A.g(null),A.g(undefined)]})", "W(()=>{class A{#p=1;static g(o){return o?.#p}}return A.g({})})",
  "W(()=>{class A{#m(){return 1}static g(o){return o?.#m()}}return [A.g(new A),A.g(null)]})", "W(()=>{var x=0;null?.[x++];return x})", "W(()=>{var x=0;null?.b(x++);return x})", "W(()=>{var x=0;null?.b.c[x++].d(x++);return x})",
  "W(()=>{var x=0;var a={b:null};a.b?.c(x++);return x})", "W(()=>{var x=0;var a={b:null};a.b?.[x++];return x})", "W(()=>{var x=0;var a={};a?.b.c;return x})",
  "W(()=>{var x=0;(null)?.[x++].y=x++;return x})", "W(()=>{var x=0;undefined?.a=(x=5);return x})",
);
// new a.b(), vírgula, exponente.
add(
  "W(()=>{var a={b:function(){l('ctor');this.t=1}};var o=new a.b();return o.t})", "W(()=>{var a={b:function(){this.t=arguments.length}};return new a.b(l(1),l(2)).t})",
  "W(()=>{var a=()=>({b:function(){this.t=1}});return new (a().b)().t})", "W(()=>{var a={b:function(){this.t=1}};var c=()=>a;return new (c().b)().t})", "W(()=>{function c(){l('c');return {b:function(){this.t=2}}}return new (c()).b().t})",
  "W(()=>{function c(){l('c');return function(){this.t=3}}return new (c())().t})", "W(()=>{function c(){l('c');return function(){this.t=3}}return new c()().t})",
  "W(()=>{var a={b:function(){}};return new a.b().constructor===a.b})", "W(()=>{var a={get b(){l('getb');return function(){this.t=1}}};return new a.b(l('arg')).t})", "W(()=>{var a={};return new a.b(l('arg'))})",
  "W(()=>{var a;return new a.b(l('arg'))})", "W(()=>{var a=1;return new a(l('arg'))})", "W(()=>{var a=()=>1;return new a(l('arg'))})", "W(()=>{var a={b:()=>1};return new a.b(l('arg'))})", "W(()=>{var a={b(){}};return new a.b(l('arg'))})",
  "W(()=>{var a={b:class{constructor(){this.t=l('cc')}}};return new a.b().t})", "W(()=>{function F(){return {r:l('r')}}return new F().r})", "W(()=>{function F(){return 1}return typeof new F})", "W(()=>{function F(){this.x=new.target===F}return new F().x})",
  "W(()=>{function F(){}F.prototype={q:1};return new F().q})", "W(()=>{function F(){}return new F instanceof F})", "W(()=>{function F(){this.a=1}return new F+''})", "W(()=>{function F(){this.a=1}return typeof new F().a})",
  "W(()=>{function F(){this.a=1}return (new F).a})", "W(()=>{function F(){this.a=1}return new F.prototype.constructor().a})", "W(()=>{var m={F:function(){this.a=1}};return new m.F().a+new m['F']().a})",
  "W(()=>{var m={n:{F:function(){this.a=1}}};return new m.n.F().a})", "W(()=>{var o={F:function(){}};return new o.F instanceof o.F})", "W(()=>new Date(0).getTime())", "W(()=>new Date().constructor===Date)",
  "W(()=>new new Function('this.a=1')().a)", "W(()=>new (class{constructor(){this.a=1}})().a)", "W(()=>new (function(){return function(){this.b=2}}())().b)", "W(()=>new Array(3).length)", "W(()=>new Array(l(2),l(3)).length)",
  "W(()=>new (l(Array))(l(3)).length)", "W(()=>new (l(Array)),l(2))", "W(()=>new Array)", "W(()=>new Array`a`.length)", "W(()=>new Array(1,2)[1])", "W(()=>new Array[0])", "W(()=>typeof new Date().getTime)",
  "W(()=>(l(1),l(2)))", "W(()=>{var x=(l(1),l(2),l(3));return x})", "W(()=>{var f=(l(1),function(){return this});return f()===globalThis})", "W(()=>{var o={f(){return this===o}};return (l(1),o.f)()})", "W(()=>{var o={f(){return this===o}};return (o.f)()})",
  "W(()=>{var o={f(){return this===o}};return (o.f,o.f)()})", "W(()=>{var o={f(){return this===o}};return (o['f'])()})", "W(()=>{var o={f(){return this===o}};return ((o.f))()})", "W(()=>{var o={f(){return this===o}};return (o.f=o.f)()})",
  "W(()=>{var o={f(){return this===o}};return (o.f||0)()})", "W(()=>{var o={f(){return this===o}};return (true&&o.f)()})", "W(()=>{var o={f(){return this===o}};return (true?o.f:0)()})", "W(()=>{var o={f(){return this===o}};return (o?.f)()})",
  "W(()=>{var x=0;var y=(x++,x++,x);return [x,y]})", "W(()=>{for(var i=0,j=10;i<j;i++,j--);return [i,j]})", "W(()=>{var a=[1,2,3];var s=0;for(var i=0,n=a.length;i<n;s+=a[i],i++);return s})",
  "W(()=>2**3**2)", "W(()=>(2**3)**2)", "W(()=>2**-1)", "W(()=>(-2)**2)", "W(()=>(-2)**3)", "W(()=>2**0.5)", "W(()=>0**0)", "W(()=>(-0)**-1)", "W(()=>NaN**0)", "W(()=>1**Infinity)", "W(()=>(-8)**(1/3))", "W(()=>2n**3n**2n)", "W(()=>(-2n)**3n)",
  "W(()=>2n**-1n)", "W(()=>2n**0n)", "W(()=>0n**0n)", "W(()=>2**3n)", "W(()=>{var x=2;x**=3**2;return x})", "W(()=>{var x=2;return x**x++ + x})", "W(()=>{var x=2;return x++**x})", "W(()=>{var x=2;return ++x**x})",
  "W(()=>{var x=2;return (x=3)**x})", "W(()=>l(2)**l(3)**l(1))", "W(()=>{var a={valueOf(){l('a');return 2}},b={valueOf(){l('b');return 3}},c={valueOf(){l('c');return 2}};return a**b**c})", "W(()=>{var a={valueOf(){l('a');return 2}};return a**-a})",
  "W(()=>{var o={p:2};o.p**=3;return o.p})", "W(()=>{var o={get p(){l('g');return 2},set p(v){l('s'+v)}};o.p**=l(3)})", "W(()=>(+2)**2)", "W(()=>(typeof 1)**2)", "W(()=>(-1)**0.5)", "W(()=>2**2**-1)", "W(()=>[2**3,2**4].join())",
  "W(()=>{var x=-2;return [x**2,(-x)**2,-(x**2)]})", "W(()=>{var a=[2,3];return a[0]**a[1]})", "W(()=>{var a=2;a**=a**=2;return a})", "W(()=>{var a=2;a**=a++;return a})", "W(()=>-(2**2))", "W(()=>(void 0)**2)", "W(()=>(1,2)**2)", "W(()=>2**(3,2))",
  "W(()=>(2**3)**(1+1))", "W(()=>3**2**2===3**4)", "W(()=>(async()=>1)()**0)",
);
// Template tags com cache de strings e raw.
const tagPrelude = "var seen=[];function t(s){seen.push(s);return s}function id(s,...v){return [s.join('|'),v.join(','),s.raw.join('|')].join('/')}";
const tplCases = [
  "function g(){return t§a${1}b§}return g()===g()", "function g(){return t§a${1}b§}return g()===t§a${1}b§", "function g(){return t§x§}function h(){return t§x§}return g()===h()", "return t§x§===t§x§",
  "function g(){return t§x§}return Object.isFrozen(g())", "function g(){return t§x§}return Object.isFrozen(g().raw)", "function g(){return t§x§}return Array.isArray(g())+','+Array.isArray(g().raw)",
  "var r=[];for(var i=0;i<3;i++)r.push(t§a§);return r[0]===r[1]&&r[1]===r[2]", "var r=[];for(var i=0;i<3;i++)r.push(t§a${i}§);return r[0]===r[1]", "var r=[];[1,2].forEach(()=>r.push(t§q§));return r[0]===r[1]",
  "var f=()=>t§w§;return f()===f()", "var f=()=>()=>t§w§;return f()()===f()()", "var fs=[1,2].map(()=>()=>t§w§);return fs[0]()===fs[1]()", "return (function(){return t§z§})()===(function(){return t§z§})()",
  "var r=[];function g(){r.push(t§s§)}g();g();return r[0]===r[1]", "var a=t§1§,b=t§1§;return a===b", "return Object.getOwnPropertyNames(t§a${1}b§).join()", "return Reflect.ownKeys(t§a§).join()",
  "return Object.getOwnPropertyDescriptor(t§a§,'raw').writable+','+Object.getOwnPropertyDescriptor(t§a§,'raw').enumerable+','+Object.getOwnPropertyDescriptor(t§a§,'raw').configurable",
  "return Object.getOwnPropertyDescriptor(t§a§,'0').writable+','+Object.getOwnPropertyDescriptor(t§a§,'0').enumerable+','+Object.getOwnPropertyDescriptor(t§a§,'0').configurable",
  "return Object.getOwnPropertyDescriptor(t§a§,'length').writable+','+Object.getOwnPropertyDescriptor(t§a§,'length').configurable", "'use strict';t§a§[0]='z'", "t§a§[0]='z';return t§a§[0]", "t§a§.raw[0]='z';return t§a§.raw[0]",
  "t§a§.x=1;return t§a§.x", "return t§a§.length+','+t§a${1}b${2}§.length+','+t§a${1}b${2}§.raw.length", "return t§§.length+','+t§§[0]===''", "return JSON.stringify(t§a${1}b§)", "return JSON.stringify(t§a${1}b§.raw)",
  "return id§a${1}b${2}c§", "return id§${1}${2}§", "return id§\\n${1}\\t§", "return id§\\u0041${1}§", "return id§\\x41§", "return id§\\101§", "return id§\\u{41}${1}\\u{1F600}§", "return id§\\§", "return id§\\${1}§",
  "return id§a\\\\b§", "return id§\\0§", "return id§\\0${1}§", "return id§\\1§", "return id§\\u00§", "return id§\\xZ§", "return id§\\u{110000}§", "return id§\\8§", "return id§\\09§", "return t§\\u00§[0]", "return t§\\u00§.raw[0]",
  "return t§\\u00${1}ok§[0]+'|'+t§\\u00${1}ok§[1]", "return String.raw§a\\n${1}b\\u0041§", "return String.raw§\\unicode and \\xerxes§", "return String.raw§§", "return String.raw§${1}${2}§", "return String.raw§a${l('x')}b${l('y')}c§",
  "return String.raw({raw:['a','b','c']},1,2,3)", "return String.raw({raw:'abc'},1,2)", "return String.raw({raw:[]},1)", "return String.raw({raw:{length:2,0:'x',1:'y'}},9)", "return String.raw({raw:['a','b']})", "return String.raw()",
  "return String.raw({})", "return String.raw({raw:null})", "return String.raw({raw:{length:-1}},1)", "return String.raw({raw:{length:2}},1)", "return String.raw§\\r\\n§.length", "return String.raw§\r\n§.length", "return §\r\n§.length", "return §\r§.length+','+§\n§.length",
  "return id§\r\n§", "var o={m(s){return this===o}};return o.m§x§", "var o={m(s){return this===o}};return (o.m)§x§", "var o={m(s){return this===o}};return (0,o.m)§x§", "var o={get m(){l('get');return function(){return this===o}}};return o.m§x§",
  "function t2(){return t2.r=(t2.r||0)+1}return t2§a§+t2§b§", "function t2(s,a,b){return a+b}return t2§${l(1)}${l(2)}§", "function t2(s,...r){return r.length}return t2§${1}${2}${3}§", "function t2(s,a){return s.length}return t2§§",
  "function t2(s){return typeof s}return t2§a§", "function t2(){return arguments.length}return t2§a${1}b${2}c§", "function t2(){return this===undefined?'u':typeof this}return t2§a§", "function t2(){'use strict';return this}return t2§a§",
  "var f=function(){return function(){return 'inner'}};return f()§a§", "var f=()=>s=>s[0];return f()§hello§", "return (s=>s[0])§hello§", "return (function(s){return s.raw[0]})§h\\n§", "return ((s)=>s.length)§a${1}b${2}c§", "return t§a§+t§b§",
  "return new (function(){this.v=1})§a§", "return typeof (new Date)§a§", "return typeof t§a§§b§", "var f=function(){return function(s){return s[0]+'!'}};return f()§a§§b§", "function a(s){return b}function b(s){return s[0]}return a§1§§2§", "return t§a§.raw===t§a§.raw",
  "return t§a${1}b§.raw===t§a${2}b§.raw", "return [t§a§,t§a§].map(x=>x.raw)[0]===t§a§.raw", "var x=1;return id§a${x}b${x=2}c${x}§", "var x=1;return id§${x++}${x++}${x}§", "return id§${{valueOf(){return 'v'},toString(){return 's'}}}§",
  "return §${{valueOf(){return 'v'},toString(){return 's'}}}§", "return §${[1,2]}§+§${{}}§+§${null}${undefined}§", "return §${Symbol()}§", "return §${1n}§", "return §${-0}§", "return §${[]}§.length", "return §a${§b${§c§}§}§", "return §a${§b${§c${1}§}§}§",
  "return id§a${id§b${1}§}c§", "return id§${id§${1}§}§", "return §${function(){return 'f'}()}§", "return §${(()=>'a')()}§", "return §${'a'+'b'}§", "return §${1,2}§", "return §${a=5}§+typeof a", "return §${{a:1}.a}§", "return §${§§}§.length",
  "return §\\§§", "return §\\${§", "return §$§+§${'$'}§+§$$§", "return §{§+§}§", "return §${'}'}§", "return §${'§'}§.length", "return §\\u{1F600}§.length", "return §😀§.length+','+§\\uD83D§.length", "return §\\x41\\u0041\\101§",
];
for (const c of tplCases) add(bt(`W(()=>{${tagPrelude};${c}})`), bt(`T(()=>{${tagPrelude};${c}})`));
add(
  bt("W(()=>{var f=function(){return function(s){return s}};var a=f()§x§;var b=f()§x§;return a===b})"), bt("W(()=>{var a=(0,eval)('(function(t){return t§x§})')(s=>s);var b=(0,eval)('(function(t){return t§x§})')(s=>s);return a===b})"),
  bt("W(()=>{var mk=()=>(0,eval)('(s=>s)§y§');return mk()===mk()})"), bt("W(()=>{var f=new Function('t','return t§x§');return f(s=>s)===f(s=>s)})"), bt("W(()=>{var f=new Function('t','return t§x§');var g=new Function('t','return t§x§');return f(s=>s)===g(s=>s)})"),
  bt("W(()=>{var a=[];var h=s=>a.push(s);h§1§;h§1§;return a[0]===a[1]})"), bt("W(()=>{class A{static m(){return (s=>s)§c§}}return A.m()===A.m()})"), bt("W(()=>{class A{m(){return (s=>s)§c§}}return new A().m()===new A().m()})"),
  bt("W(()=>{function* g(){yield (s=>s)§c§;yield (s=>s)§c§}var a=[...g()];return a[0]===a[1]})"), bt("W(()=>{function* g(){while(true)yield (s=>s)§c§}var it=g();return it.next().value===it.next().value})"),
  bt("W(()=>{var f=async()=>(s=>s)§c§;return typeof f})"),
);

// ---- 3. Hoisting: TDZ, parâmetros default, function em bloco (Annex B).
const tdz = [
  "switch(1){case 0:let x;case 1:return typeof x}", "switch(1){case 0:let x;case 1:return x}", "switch(1){case 1:let x=l('i');default:return x}", "switch(2){case 1:let x=1;default:return typeof x}",
  "switch(2){default:let x=5;case 1:return x}", "switch(1){case 1:return x;case 2:let x}", "switch(1){case 1:x=1;case 2:let x}", "switch(1){case 1:let x=1;case 2:let y=x;return y}", "switch(2){case 1:let x=1;case 2:x=2;return x}",
  "switch(2){case 1:const x=1;case 2:return typeof x}", "switch(1){case (typeof x):let x}", "switch(typeof x){case 'undefined':let x}", "switch(1){case 1:{let x=1}default:return typeof x}", "switch(1){case 1:class C{}default:return typeof C}",
  "switch(2){case 1:class C{}default:return typeof C}", "switch(2){case 1:class C{}default:new C}", "switch(1){case 1:function f(){return 1}case 2:return f()}", "switch(2){case 1:function f(){return 1}case 2:return f()}", "switch(2){case 1:function f(){return 1}}return typeof f",
  "switch(1){case 1:function f(){return 1}}return typeof f", "switch(1){case 0:function f(){return 1}default:return typeof f}", "switch(1){case 1:let f=1;case 2:function g(){return f}return g()}",
  "for(let x=x;;){return 1}", "for(let x=typeof x;;){return x}", "for(let x=1,y=x+1;;){return y}", "for(let x=y,y=1;;){return 1}", "for(let x=0;x<1;x++){let x=5;return x}", "for(let x=(()=>x)();;){return 1}", "for(let x=1;x<2;x++){var f=()=>x;}return f()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i)}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<3;fs.push(()=>i),i++);return fs.map(f=>f()).join()", "var fs=[];for(let i=0;fs.push(()=>i),i<3;i++);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);i++}return fs.map(f=>f()).join()", "var fs=[];for(var i=0;i<3;i++){fs.push(()=>i)}return fs.map(f=>f()).join()", "var fs=[];for(let k in {a:1,b:2}){fs.push(()=>k)}return fs.map(f=>f()).join()",
  "var fs=[];for(let v of [1,2]){fs.push(()=>v)}return fs.map(f=>f()).join()", "for(let x of [x]){}", "for(let x in {x}){}", "for(let x of (()=>typeof x)()){}", "for(const x of [1]){x=2}", "for(const i=0;i<1;){break}return 1", "for(const i=0;i<1;i++){}",
  "for(let [a,b=a] of [[1]]){return b}", "for(let [a=b,b] of [[]]){return 1}", "for(let {a=1,b=a} of [{}]){return b}", "for(var x of [1])var x;return x", "for(let x of [1]){var x}", "for(let x of [1]){{var x}}", "for(let x;;){var x;break}",
  "let x=1;{let x=2}return x", "let x=1;{x=2;let y=x}return x", "{return typeof x;let x}", "{return x;let x}", "{x;let x}", "{let x=x}", "{let x=typeof x}", "let x=(()=>x)();", "let x=(()=>typeof x)();return x", "var f=()=>x;let x=1;return f()", "var f=()=>x;try{f()}catch(e){var n=e.name}let x=1;return n",
  "try{x}catch(e){}let x;return 1", "try{throw 1}catch(x){let y=x;return y}", "try{throw 1}catch(x){var x=2;return x}", "try{throw 1}catch(x){var x=2}return x", "try{throw 1}catch([x]){var y}return typeof y", "try{throw [1]}catch([x,y=x]){return y}", "try{throw 1}catch(x){{let x=2}return x}",
  "try{throw 1}catch(x){function f(){return x}return f()}", "try{throw 1}catch(x){for(var x of [5]){}return x}", "try{throw 1}catch(e){var e=2;return e}", "var e=0;try{throw 1}catch(e){var e=2}return e", "var e=0;try{throw 1}catch(e){e=3}return e",
  "class C{static x=typeof C;static y=C}return C.x+','+(C.y===C)", "class C extends (typeof C){}", "class C{[typeof C](){}}", "class C{static [C](){}}", "var D=class C{static m(){return typeof C}};return D.m()", "var D=class C{};C", "var D=class C{};return typeof C",
  "class C{}class C{}", "let C=1;class C{}", "var C=1;class C{}", "class C{};var C", "typeof C;class C{}", "C=1;class C{}", "var f=()=>new C;try{f()}catch(e){var n=e.name}class C{}return n+typeof new C",
  "function f(a=b,b){return 1}return f(undefined,2)", "function f(a=b,b){return 1}return f(1)", "function f(a=a){return a}return f()", "function f(a=a){return a}return f(2)", "function f(a,b=a){return b}return f(3)", "function f(a=b,b=1){return a}return f()",
  "function f(a=()=>b,b=2){return a()}return f()", "function f(a=()=>a){return a()===a}return f()", "function f(a=typeof b,b=1){return a}return f()", "function f(a=arguments.length,b=1){return a}return f(undefined,2,3)", "function f(a,b=arguments[0]){return b}return f(7)",
  "function f(a,b=()=>arguments[0]){a=9;return b()}return f(1)", "function f(a=1){var a;return a}return f()", "function f(a=1){var a=2;return a}return f()", "function f(a,b=()=>a){var a=2;return b()}return f(1)", "function f(a,b=()=>a){var a;return a}return f(1)",
  "function f(a,b=()=>a){var a;a=5;return b()+','+a}return f(1)", "function f(a=1){function a(){}return typeof a}return f()", "function f(a,b=()=>a){function a(){}return typeof b()+typeof a}return f(1)", "function f(a=eval('typeof z'),z){return a}return f()",
  "function f(a=eval('var z=1;z')){return typeof z}return f()", "function f(a=eval('var z=1'),b=()=>z){return b()}return f()", "function f(a=eval('var z=1')){var z=2;return z}return f()", "function f(x=eval('var q=1'),y=q){return y}return f()",
  "function f(a,b=()=>a){eval('var a=3');return b()+','+a}return f(1)", "function f(a=1,...r){return r.length}return f()", "function f({a}={a:2}){return a}return f()", "function f({a=1,b=a}={}){return b}return f()", "function f([a,b=a]=[4]){return b}return f()",
  "function f({a=b,b}={b:1}){return a}return f()", "function f({a},b=a){return b}return f({a:6})", "function f(a=l('d'),b=l('e')){}f();return 1", "function f(a=l('d'),b=l('e')){}f(1);return 1", "function f(a=l('d')){}f(undefined);f(null);f(0);return 1", "function f(a=l('d')){}f(void 0);return 1",
  "var f=(a=b,b)=>1;return f(undefined,1)", "var f=(a=b,b)=>1;return f(1)", "var f=(a=()=>b,b=1)=>a();return f()", "var f=(a=this)=>a;return f()===globalThis||f()===undefined", "var f=async(a=b,b)=>1;return typeof f", "var f=function*(a=b,b){};return f(1)", "var f=function*(a=b,b){};f()",
  "var o={m(a=b,b){}};o.m()", "var o={m(a=b,b){}};o.m(1)", "class C{constructor(a=b,b){}}new C", "class C{m(a=b,b){}}new C().m()", "class C{static m(a=b,b){}}C.m()", "var o={set p(a=1){}}", "var o={set p([a]){}};o.p=[1];return 1", "var o={set p({a}){this.v=a}};o.p={a:3};return o.v",
  "function f(a,a){return a}return f(1,2)", "function f(a,a=1){}", "'use strict';function f(a,a){}", "function f(a,b=a,a){}", "var f=(a,a)=>1", "function f(a,[a]){}", "function f(a,{a}){}", "function f(a,...a){}", "function f(a){let a}", "function f(a){var a;return a}return f(1)",
  "function f(a){{let a=2}return a}return f(1)", "function f(a){function a(){}return typeof a}return f(1)", "function f(a=1){let b=a;return b}return f()", "function f(a=1){{function a(){}}return typeof a}return f()", "function f(a){{function a(){}}return typeof a}return f(1)", "function f(){{function arguments(){}}return typeof arguments}return f()",
  "function f(a){arguments[0]=2;return a}return f(1)", "function f(a=0){arguments[0]=2;return a}return f(1)", "function f(a){'use strict';arguments[0]=2;return a}return f(1)", "function f(a){a=3;return arguments[0]}return f(1)", "function f(a=0){a=3;return arguments[0]}return f(1)",
  "function f(a,b){return arguments.length+','+f.length}return f(1)", "function f(a,b){b=2;return arguments.length}return f(1)", "function f(a,b){arguments[1]=2;return b}return f(1)", "function f(a,b){delete arguments[0];arguments[0]=5;return a}return f(1,2)",
  "function f(a){Object.defineProperty(arguments,'0',{writable:false});a=2;return arguments[0]}return f(1)", "function f(a){Object.defineProperty(arguments,'0',{value:9});return a}return f(1)", "function f(a){Object.defineProperty(arguments,'0',{get(){return 7}});a=3;return arguments[0]+','+a}return f(1)",
  "function f(a){return typeof arguments+(arguments===arguments)}return f()", "function f(){return arguments.callee===f}return f()", "function f(){'use strict';return arguments.callee}return f()", "function f(){return Object.prototype.toString.call(arguments)}return f()", "function f(){return [].concat(arguments).length}return f(1,2)",
  "function f(){return arguments[Symbol.iterator]===Array.prototype.values}return f()", "function f(){return Object.getOwnPropertyNames(arguments).join()}return f(1,2)", "function f(a,b){return Object.getOwnPropertyNames(arguments).join()}return f(1)", "var f=()=>typeof arguments;return f()",
];
for (const c of tdz) add(`W(()=>{${c}})`);
// TDZ e hoisting no escopo global via eval indireto.
for (const c of [
  "typeof x;let x", "x;let x", "let x=1;let x", "var x;let x", "let x;var x", "{let x;var x}", "{var x;let x}", "{let x}var x;typeof x", "typeof f;function f(){}", "typeof v;var v=1", "var v=1;var v=2;typeof v", "f();function f(){return 1}",
  "var f=1;function f(){}typeof f", "function f(){}var f;typeof f", "function f(){return 1}function f(){return 2}f()", "typeof g;{function g(){}}typeof g", "{function g(){}}typeof g", "typeof g;if(true){function g(){}}typeof g",
  "typeof g;if(false){function g(){}}typeof g", "if(true)function g(){}typeof g", "if(false)function g(){}typeof g", "if(true)function g(){return 1}else function g(){return 2}g()", "if(false)function g(){return 1}else function g(){return 2}g()",
  "l:function g(){}typeof g", "{l:function g(){}}typeof g", "while(false)l:function g(){}", "if(true)l:function g(){}", "for(;false;)function g(){}", "{let g;{function g(){}}}typeof g", "{let g=1;{function g(){}}}g", "let g=1;{function g(){}}g",
  "{function g(){return 1}function g(){return 2}}g()", "{function g(){return 1}var g}", "{var g;function g(){}}", "{function g(){}let g}", "{function g(){}{function g(){}}}typeof g", "{function g(){return 1}g=2}typeof g+g", "{function g(){return 1};g=2;}g",
  "var r=[];{r.push(typeof g);function g(){}r.push(typeof g)}r.push(typeof g);r.join()", "var r=[typeof g];{function g(){return 1}}r.push(typeof g);r.join()", "var r=[];{function g(){return 1}g=5;r.push(g)}r.push(g);r.join()", "var r=[];{function g(){return 1}r.push(g())}{function g(){return 2}r.push(g())}r.push(g());r.join()",
  "switch(1){case 1:function g(){}}typeof g", "switch(2){case 1:function g(){}}typeof g", "switch(1){case 1:function g(){}default:let g}", "switch(1){case 1:function g(){return 1}case 2:function g(){return 2}}g()", "try{function g(){}}catch(e){}typeof g", "try{throw 1}catch(e){function g(){}}typeof g",
  "try{throw 1}catch(g){{function g(){}}}typeof g", "try{throw 1}catch(g){function g(){}}", "try{throw 1}catch([g]){{function g(){}}}typeof g", "try{throw 1}catch(g){var g=2}typeof g", "for(var i=0;i<1;i++){function g(){}}typeof g", "for(let i=0;i<1;i++){function g(){}}typeof g",
  "for(let g=0;g<1;g++){function g(){}}", "for(var g of[1]){function g(){}}typeof g", "for(let g of[1]){function g(){}}typeof g", "for(let g of[1]){{function g(){}}}typeof g", "(function(){{function g(){}}return typeof g})()", "(function(){return typeof g;{function g(){}}})()",
  "(function(g){{function g(){}}return typeof g})(1)", "(function(g){{function g(){}}return g})(1)", "(function(){let g=1;{function g(){}}return g})()", "(function(){var g=1;{function g(){return 2}}return typeof g})()", "(function(){'use strict';{function g(){}}return typeof g})()",
  "(function(){'use strict';return typeof g;{function g(){}}})()", "(function(){'use strict';{function g(){}g();}})()", "(function(){'use strict';if(true)function g(){}})", "(function(){if(true)function g(){}return typeof g})()", "(function(){if(false)function g(){}return typeof g})()",
  "(function(){var r=typeof g;{function g(){}}return r})()", "(function(){{function g(){return 1}}{function g(){return 2}}return g()})()", "(function(){{function g(){return 1}}return g()})()", "(function(){g=5;{function g(){}}return g})()", "(function(){{g=5;function g(){}}return typeof g+g})()",
  "(function(){{function g(){}g=5}return typeof g})()", "(function(){{function g(){}g=5}return g})()", "(function(){var g=1;{function g(){}g=5}return g})()", "(function(){{function g(){}}g=7;return g})()", "(function(){{function g(){}}return g===undefined})()", "(function(arguments){{function arguments(){}}return typeof arguments})(1)",
  "(function(){{function arguments(){}}return typeof arguments})()", "(function(){var arguments;{function arguments(){}}return typeof arguments})()", "(function(){{function eval(){}}return typeof eval})()", "(function(){{function undefined(){}}return typeof undefined})()", "(function(){{function NaN(){}}return typeof NaN})()",
  "(function(){let a;{function a(){}}return typeof a})()", "(function(){{let a;{function a(){}}}return typeof a})()", "(function(){{let a;function a(){}}})", "(function(){{function a(){}let a}})", "(function(){{const a=1;{function a(){}}}return typeof a})()", "(function(){class a{}{function a(){}}return typeof a})()",
  "(function(){{class a{}{function a(){}}}return typeof a})()", "(function(){{async function a(){}}return typeof a})()", "(function(){{function* a(){}}return typeof a})()", "(function(){{async function* a(){}}return typeof a})()", "(function(){if(true)async function a(){}})", "(function(){if(true)function* a(){}})",
  "(function(){l:function* a(){}})", "(function(){{function a(){}function a(){}}})()", "(function(){{function a(){}async function a(){}}})", "(function(){{function a(){}function* a(){}}})", "(function(){'use strict';{function a(){}function a(){}}})", "(function(){{var a;function a(){}}})", "(function(){{function a(){}var a}})",
  "(function(){var f=1;{function f(){}}return f})()", "(function(f=1){{function f(){}}return f})()", "(function(f=1){{function f(){}}return typeof f})()", "(function(){{function f(){return typeof f}}return f()})()", "(function(){{function f(){return f}}return f()===f})()",
  "(function(){var r=[];{function f(){}r.push(typeof f)}r.push(typeof f);{function f(){}}return r.join()})()", "(function(){var f0=f;{function f(){}}return typeof f0})()", "(function(){eval('{function g(){}}');return typeof g})()", "(function(){eval('function g(){}');return typeof g})()",
  "(function(){eval('var g=1');return typeof g})()", "(function(){'use strict';eval('function g(){}');return typeof g})()", "(function(){let g=1;eval('{function g(){}}');return typeof g})()", "(function(){eval('{function g(){}}');var g=1;return typeof g})()", "(function(g){eval('{function g(){}}');return typeof g})(1)",
  "(function(){eval('let g=1');return typeof g})()", "(function(){eval('var g=1;{function g(){}}');return typeof g})()", "(function(){eval('if(true)function g(){}');return typeof g})()", "(function(){eval('typeof g;{function g(){}}');return typeof g})()", "(0,eval)('{function g2(){}}');typeof g2",
  "(0,eval)('var g3=1');typeof g3", "(0,eval)('let g4=1');typeof g4", "(0,eval)('function g5(){}');typeof g5", "(0,eval)('\"use strict\";function g6(){}');typeof g6", "(0,eval)('\"use strict\";var g7=1');typeof g7", "eval('var g8=1');delete g8", "(0,eval)('var g9=1');delete g9", "var g10=1;delete g10",
  "g11=1;delete g11", "(0,eval)('function g12(){}');delete g12", "var g13=1;(0,eval)('var g13=2');g13", "let g14=1;(0,eval)('var g14=2')", "(0,eval)('let g15=1');(0,eval)('var g15')", "(0,eval)('var g16');(0,eval)('let g16')", "(0,eval)('let g17=1');(0,eval)('let g17')",
  "function g18(){}(0,eval)('let g18=1')", "(0,eval)('function g19(){return 1}');(0,eval)('function g19(){return 2}');g19()", "typeof undefined;var undefined=1;typeof undefined", "var undefined;undefined", "function undefined(){}", "let undefined", "var NaN=1;NaN", "var Infinity=1;typeof Infinity",
  "var globalThis=1;typeof globalThis", "let globalThis=1;typeof globalThis", "let Object=1;typeof Object", "var Object=1;typeof Object", "function Object(){}typeof Object", "let toString=1;typeof toString", "var toString=1;typeof toString", "typeof hasOwnProperty;let hasOwnProperty=1", "let __proto__=1;__proto__",
]) add(`E(${q(c)})`);
for (const c of [
  "var fnn=eval('1;function k(){}');return typeof fnn", "return typeof eval('var k=1;k')+typeof k", "return typeof eval('let k=1;k')+typeof k", "var k=1;return eval('var k=2;k')+k", "let k=1;return eval('var k=2')", "let k=1;return eval('let k=2;k')+k",
  "const k=1;return eval('var k=2')", "var k=1;{let k=2;return eval('k')}", "var k=1;{let k=2;return eval('var k=3;k')}", "var k=1;{let k=2;return eval('{function k(){} }typeof k')}", "return eval('typeof k;{function k(){}}typeof k')", "function k(){return 1}return eval('var k;typeof k')",
  "function k(){return 1}return eval('var k=2;typeof k')+typeof k", "return eval('function k(){return 1}')===undefined", "eval('var k1=1');return k1", "eval('var k1=1');delete k1;return typeof k1", "var k1=1;delete k1;return typeof k1", "eval('var k1=1');return delete k1",
  "var a=1;return (function(){eval('var a=2');return a})()+a", "var a=1;return (function(){return eval('a')})()", "var a=1;return (function(a){eval('var a=3');return a})(2)", "var a=1;return (function(){var a=5;return eval('a')})()", "var a=1;return (function(){var a=5;return (0,eval)('a')})()",
  "var a=1;return (function(){var a=5;var e=eval;return e('a')})()", "var a=1;return (function(){var a=5;return eval?.('a')})()", "var a=1;return (function(){var a=5;return (eval)('a')})()", "var a=1;return (function(){var a=5;return eval('a')===eval('(a)')})()", "var a=1;return (function(){var a=5;return [eval][0]('a')})()",
  "var a=1;return (function(){var a=5;return window?.a})", "var a=1;return (function(){var a=5;return new Function('return typeof a')()})()", "var a=1;return (function(){var a=5;return Function('return a')()})()", "return eval('this')===this", "return (0,eval)('this')===globalThis", "return eval('arguments.length')",
  "return (()=>eval('arguments.length'))()", "return eval('new.target')", "var f=function(){return eval('new.target')};return typeof new f", "var f=function(){return eval('new.target')};return f()", "var f=function(){return (()=>eval('new.target'))()};return typeof new f", "var o={m(){return eval('super.x')}};return o.m()",
  "var o={__proto__:{x:5},m(){return eval('super.x')}};return o.m()", "var o={__proto__:{x:5},m(){return (0,eval)('super.x')}};return o.m()", "class A{static x=eval('new.target')}return A.x", "class A{x=eval('this')}return new A().x instanceof A", "class A{x=eval('arguments')}new A", "class A{x=eval('new.target')}return new A().x",
  "class A{static{eval('var q=1')}}return typeof q", "class A{static{var q=1}}return typeof q", "class A{static{this.z=eval('this')===A}}return A.z", "var v=1;return eval('v++')+v", "var v=1;return eval('++v')+v", "return eval('1+1')+eval('2*3')", "return eval('eval(\"1+2\")')", "return eval('(0,eval)(\"typeof l\")')",
  "return eval('(function(){return this})()')===globalThis", "return eval('(function(){\"use strict\";return this})()')", "return eval('\"use strict\";(function(){return this})()')===globalThis", "return eval('\"use strict\";this')===this", "'use strict';return eval('this')===this", "'use strict';eval('var q=1');return typeof q",
  "'use strict';return eval('var q=1;q')", "'use strict';return eval('function q(){}')", "'use strict';return (0,eval)('var q=1;q')+typeof q", "'use strict';return (0,eval)('var q9=1');", "'use strict';(0,eval)('var q9=1');return typeof q9", "'use strict';return eval('arguments=1')", "'use strict';return eval('with({}){}')",
  "return eval('with({a:1}){a}')", "return eval('with({a:1}){var b=a}')+typeof b", "return eval('delete x')", "return eval('delete 1')", "return eval('delete l')", "return eval('delete globalThis.undefinedProp')", "return eval('typeof delete 1')", "return eval('var x;delete x')", "return eval('x=1;delete x')",
]) add(`W(()=>{${c}})`);

// ---- 4. getter/setter em with, Symbol.unscopables, Proxy.
const withObjs = [
  "{get x(){l('get');return 1},set x(v){l('set'+v)}}", "{get x(){l('get');return 1}}", "{set x(v){l('set'+v)}}", "{x:1}", "Object.create({x:1})", "Object.freeze({x:1})", "Object.defineProperty({},'x',{value:1,writable:false})",
  "{x:1,[Symbol.unscopables]:{x:true}}", "{x:1,[Symbol.unscopables]:{x:false}}", "{x:1,[Symbol.unscopables]:{get x(){l('unscopables get');return true}}}", "{x:1,get [Symbol.unscopables](){l('u');return {x:true}}}", "{x:1,[Symbol.unscopables]:null}",
  "{x:1,[Symbol.unscopables]:1}", "{x:1,[Symbol.unscopables]:{x:1}}", "{x:1,[Symbol.unscopables]:{x:0}}", "{x:1,[Symbol.unscopables]:'x'}",
  "new Proxy({x:1},{has(t,k){l('has '+String(k));return k in t},get(t,k,r){l('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){l('set '+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){l('del '+String(k));return Reflect.deleteProperty(t,k)}})",
  "new Proxy({},{has(t,k){l('has '+String(k));return k==='x'},get(t,k){l('get '+String(k));return 9},set(t,k,v){l('set '+String(k));return true}})", "new Proxy({x:1},{has(){return true},get(){return undefined}})",
  "new Proxy({x:1},{getOwnPropertyDescriptor(t,k){l('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})", "[7]", "'str'", "function(){}", "Object.assign(function(){},{x:1})", "{x(){return this===o}}", "{x:function(){return this}}",
];
const withBodies = [
  "x", "typeof x", "x=5", "x+=5", "x++", "++x", "x--", "delete x", "x?.toString()", "x&&=3", "x||=3", "x??=3", "[x]=[3]", "({x}={x:3})", "var x", "var x=7", "var x=7;x", "(()=>x)()", "(function(){return x})()", "x=(l('rhs'),4)", "x==1", "x?x:0", "void x",
  "(x)", "x`a`", "x()", "new x", "x.y", "x[0]", "x=x+1", "x&&x", "eval('x')", "(0,eval)('typeof x')", "function f(){return x}f()", "function f(){return x}x=3;f()", "let z=x;z", "var y=x;y", "for(var i=0;i<1;i++)x", "for(var k in{a:1})x", "try{x}finally{l('f')}", "x=3;x", "x=3;delete x;typeof x", "delete x;typeof x;",
  "typeof x==='number'", "x=x", "x,x", "x**=2", "x<<=1", "x%=2", "x=undefined", "x=null;x", "x=Symbol.iterator;typeof x", "toString", "typeof toString", "valueOf===Object.prototype.valueOf", "hasOwnProperty('x')", "typeof globalThis", "this===globalThis", "typeof undefined", "undefined=1;undefined",
];
for (const o of withObjs) for (const b of withBodies) addEvery("with", 3, `W(()=>{var o=${o};with(o){return eval(${q(b)})}})`);
for (const o of withObjs.slice(0, 8)) for (const b of withBodies.slice(0, 14)) add(`W(()=>{var o=${o};var r;with(o){r=(function(){return eval(${q(b)})})()};return r})`);
add(
  "W(()=>{var o={x:1};with(o){var x=2}return o.x+','+typeof x})", "W(()=>{var o={};with(o){var x=2}return o.x+','+x})", "W(()=>{var o={x:1};with(o){var x}return o.x+','+typeof x})", "W(()=>{var o={x:1};with(o){function x(){}}return typeof o.x+typeof x})",
  "W(()=>{var o={x:1};with(o){{function x(){}}}return typeof o.x+typeof x})", "W(()=>{var o={x:1};with(o){let x=5;x++}return o.x})", "W(()=>{var o={x:1};with(o){const f=()=>x;o.x=3;return f()}})", "W(()=>{var o={x:1};var f;with(o){f=()=>x}delete o.x;return typeof f()})",
  "W(()=>{var x=0;var o={x:1};var f;with(o){f=()=>x}delete o.x;return f()})", "W(()=>{var o={x:1};var f;with(o){f=function(){x=9}}f();return o.x})", "W(()=>{var o={x:1};var f;with(o){f=function(){x=9}}delete o.x;f();return o.x+','+globalThis.x})",
  "W(()=>{var o={x:1};with(o){delete o.x;x=5}return o.x+','+globalThis.x})", "W(()=>{var o={};with(o){x=5}return o.x+','+globalThis.x})", "W(()=>{var o={x:1};with(o){x=(delete o.x,5)}return o.x+','+globalThis.x})", "W(()=>{var o={x:1};with(o){x+=(delete o.x,5)}return o.x+','+globalThis.x})",
  "W(()=>{var o={x:1};with(o){x=(o.x=7,5)}return o.x})", "W(()=>{var o={x:1};with(o){x++;x++}return o.x})", "W(()=>{var o={x:1};with(o){return [x++,x]}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x+=2}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x++}})",
  "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x=2}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x&&=2}})", "W(()=>{var o={get x(){l('g');return 0},set x(v){l('s'+v)}};with(o){x&&=2}})", "W(()=>{var o={get x(){l('g');return 0},set x(v){l('s'+v)}};with(o){x||=2}})",
  "W(()=>{var o={get x(){l('g');return null},set x(v){l('s'+v)}};with(o){x??=2}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){[x]=[3]}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){({a:x}={a:3})}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){for(x of [1,2]);}})",
  "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){for(x in {a:1,b:2});}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x=x+x}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){x=x++}})", "W(()=>{var o={get x(){l('g');return 1},set x(v){l('s'+v)}};with(o){typeof x;void x;delete x}})",
  "W(()=>{var o={get x(){l('g');return 1}};with(o){x=2}return o.x})", "W(()=>{'use strict';var o={get x(){l('g');return 1}};try{with(o){x=2}}catch(e){}})", "W(()=>{var o={f(){return this===o}};with(o){return f()}})", "W(()=>{var o={f(){return this===o}};with(o){return (f)()}})", "W(()=>{var o={f(){return this===o}};with(o){return (0,f)()}})",
  "W(()=>{var o={f(){return this===o}};with(o){return f?.()}})", "W(()=>{var o={f(){return this===o}};with(o){return f``}})", "W(()=>{var o={f(){return this===o}};with(o){return new f}})", "W(()=>{var o={f(){'use strict';return this===o}};with(o){return f()}})", "W(()=>{var o={f(){return typeof this}};with(o){return [f(),(()=>f())()]}})",
  "W(()=>{var o={f:function(){return this===o}};with(o){return eval('f()')}})", "W(()=>{var o={f:function(){return this===o}};with(o){return (function(){return f()})()}})", "W(()=>{function f(){return this===globalThis||this===undefined}var o={};with(o){return f()}})", "W(()=>{var o={};with(o){return typeof this}})", "W(()=>{var o={this:1};with(o){return typeof this}})",
  "W(()=>{var p=new Proxy({},{has(t,k){l('has '+String(k));return false}});with(p){return typeof undefinedVar+typeof l}})", "W(()=>{var p=new Proxy({},{has(t,k){l('has '+String(k));return k==='zz'},get(t,k){l('get '+String(k));return 5}});with(p){return zz+zz}})", "W(()=>{var p=new Proxy({},{has(t,k){l('has '+String(k));return k==='zz'},get(t,k){l('get '+String(k));return undefined}});with(p){return typeof zz}})",
  "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t}});with(p){zz=2;return p.zz}})", "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t},set(t,k,v,r){l('set '+String(k));return Reflect.set(t,k,v,r)},getOwnPropertyDescriptor(t,k){l('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});with(p){zz=2}})",
  "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t},get(t,k,r){l('get '+String(k));return Reflect.get(t,k,r)}});with(p){zz++}})", "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t},deleteProperty(t,k){l('del '+String(k));return delete t[k]}});with(p){return delete zz}})",
  "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t},get(t,k,r){l('get '+String(k));return k===Symbol.unscopables?{zz:true}:Reflect.get(t,k,r)}});with(p){return typeof zz}})", "W(()=>{var p=new Proxy({zz:1},{has(t,k){l('has '+String(k));return k in t},get(t,k,r){l('get '+String(k));return Reflect.get(t,k,r)}});with(p){zz;zz}})",
  "W(()=>{var o={x:1};with(o)with({x:2})return x})", "W(()=>{var o={x:1};with(o)with({y:2})return x+y})", "W(()=>{var o={x:1};with(o){with({x:2}){x=3}}return o.x})", "W(()=>{var o={x:1};with(o){with({x:2}){delete x}}return o.x+','+typeof x})", "W(()=>{var o={x:1};with(o){with({}){x=3}}return o.x})",
  "W(()=>{var o={x:1};with(o){with(o){x=4}}return o.x})", "W(()=>{with({x:1}){return (()=>{with({y:2}){return x+y}})()}})", "W(()=>{with({x:1}){return (function(){return x})()}})", "W(()=>{with({x:1}){return (class{static v=x}).v}})", "W(()=>{with({x:1}){return (class{m(){return x}}).prototype.m()}})", "W(()=>{with({x:1}){return (class extends Object{constructor(){super();this.v=x}}).name}})",
  "W(()=>{with({x:1}){function* g(){yield x}return g().next().value}})", "W(()=>{with({x:1}){return [1,2].map(i=>i+x).join()}})", "W(()=>{with({x:1}){return `${x}${typeof x}`}})", "W(()=>{with({x:1}){return {x}}})", "W(()=>{with({x:1}){return {[x]:x}}})", "W(()=>{with({x:1}){return [x,...[x]]}})",
  "W(()=>{with({x:1}){var {x:y}={x:5};return y}})", "W(()=>{with({y:1}){var {y}={y:5}}return typeof y})", "W(()=>{var o={y:1};with(o){var {y}={y:5}}return o.y+','+typeof y})", "W(()=>{var o={y:1};with(o){var [y]=[5]}return o.y+','+typeof y})", "W(()=>{var o={y:1};with(o){for(var y of[5]);}return o.y+','+typeof y})",
  "W(()=>{var o={y:1};with(o){for(var y in{a:1});}return o.y+','+typeof y})", "W(()=>{var o={y:1};with(o){for(var y=(l('init'),2);false;);}return o.y+','+typeof y})", "W(()=>{var o={y:1};with(o){try{throw 1}catch(y){var y=3}}return o.y+','+typeof y})", "W(()=>{var o={};with(o){var y=1}return Object.keys(o).length+','+y})",
  "W(()=>{var o={y:1};with(o){var f=function(){return y};}o.y=2;return f()})", "W(()=>{var o={y:1};with(o){function g(){return y}}o.y=2;return g()})", "W(()=>{var o={y:1};with(o){{function g(){return y}}}o.y=2;return g()})", "W(()=>{var o={g:5};with(o){function g(){}}return o.g+typeof g})",
  "W(()=>{var o={};with(o){function g(){}}return Object.keys(o).length+typeof g})", "W(()=>{with(Math){return [max(1,2),PI>3,typeof cos]}})", "W(()=>{with(Math){return typeof Math}})", "W(()=>{with([1,2,3]){return [length,typeof push,typeof values,typeof keys]}})", "W(()=>{with([1,2,3]){return typeof entries+typeof find+typeof includes}})",
  "W(()=>{with('abc'){return [length,typeof charAt,toUpperCase()]}})", "W(()=>{with(new String('abc')){return [length,charAt(1)]}})", "W(()=>{with(5){return typeof toFixed+typeof toString}})", "W(()=>{with(null){}})", "W(()=>{with(undefined){}})", "W(()=>{with(1){return typeof this}})", "W(()=>{with(Symbol()){return typeof description}})",
  "W(()=>{with(function(){}){return [length,name]}})", "W(()=>{function F(a,b){with(F){return [length,name]}}return F()})", "W(()=>{with(globalThis){return typeof l}})", "W(()=>{with(globalThis){var q9=1}return typeof globalThis.q9+typeof q9})", "W(()=>{with({}){return typeof withUndefined}})", "W(()=>{with({}){withUndefined}})",
  "W(()=>{with({}){withUndefined=1}return typeof withUndefined})", "W(()=>{'use strict';with({}){}})", "W(()=>{with({}){'use strict';return typeof this}})", "W(()=>{with({a:1}){return (function(){'use strict';return typeof a})()}})", "W(()=>{with({a:1}){return (function(){'use strict';a=2;return a})()}})", "W(()=>{with({a:1}){return (function(){'use strict';return eval('a')})()}})",
  "W(()=>{var o={a:1};with(o){return (function(){'use strict';a=2;return o.a})()}})", "W(()=>{var o={a:1};with(o){return (function(){'use strict';delete o.a;a=3})()}})", "W(()=>{var o={a:1};with(o){return (function(){'use strict';delete o.a;a=3})()}});",
  "W(()=>{var o={a:1};with(o){var f=function(){return a};delete o.a}return typeof f()})", "W(()=>{var a=5;var o={a:1};with(o){var f=function(){return a};delete o.a}return f()})", "W(()=>{var o={[Symbol.unscopables]:{a:true},a:1};var a=2;with(o){return a}})", "W(()=>{var o={[Symbol.unscopables]:{a:true},a:1};var a=2;with(o){a=3}return o.a+','+a})",
  "W(()=>{var o={[Symbol.unscopables]:{a:true},a:1};with(o){return typeof a}})", "W(()=>{var o={[Symbol.unscopables]:{a:true},a:1};with(o){return (typeof a)+typeof o}})", "W(()=>{var o={a:1};o[Symbol.unscopables]={a:true};with(o){return typeof a}})", "W(()=>{var o={a:1};with(o){o[Symbol.unscopables]={a:true};return a}})",
  "W(()=>{var o={a:1};with(o){o[Symbol.unscopables]={a:true};return typeof a}})", "W(()=>{var o={a:1};with(o){o[Symbol.unscopables]={a:true};a=5}return o.a+','+typeof globalThis.a})", "W(()=>{with(Array.prototype){return [typeof keys,typeof values,typeof flat,typeof at,typeof findLast,typeof toSorted,typeof includes]}})",
  "W(()=>{with([]){return [typeof keys,typeof values,typeof flat,typeof at,typeof findLast,typeof toSorted,typeof includes,typeof copyWithin,typeof entries,typeof fill,typeof find,typeof findIndex,typeof flatMap,typeof toReversed,typeof toSpliced]}})",
  "W(()=>{return Object.keys(Array.prototype[Symbol.unscopables]).join()})", "W(()=>{return Object.getPrototypeOf(Array.prototype[Symbol.unscopables])})", "W(()=>{return [typeof Object.prototype[Symbol.unscopables],typeof Map.prototype[Symbol.unscopables],typeof Array.prototype[Symbol.unscopables]]})",
  "W(()=>{var d=Object.getOwnPropertyDescriptor(Array.prototype,Symbol.unscopables);return [d.writable,d.enumerable,d.configurable]})", "W(()=>{var d=Object.getOwnPropertyDescriptor(Array.prototype[Symbol.unscopables],'at');return [d.value,d.writable,d.enumerable,d.configurable]})",
);

// ---- 5. Labels, continue, break e return em finally: produto de loops x try/catch/finally.
const loopKinds = {
  for: b => `for(var i=0;i<3;i++){${b}}`, while: b => `var i=-1;while(++i<3){${b}}`, doWhile: b => `var i=-1;do{i++;${b}}while(i<2)`, forIn: b => `var ks=['a','b','c'],i=-1;for(var k in {a:1,b:2,c:3}){i++;${b}}`,
  forOf: b => `var i=-1;for(var v of ['a','b','c']){i++;${b}}`, forLet: b => `for(let i=0;i<3;i++){${b}}`,
};
const actions = ["", "break", "continue", "return 'R'", "throw 'T'"];
const tryActs = { none: "l('t'+i)", brk: "l('t'+i);if(i==1)break", cont: "l('t'+i);if(i==1)continue", ret: "l('t'+i);if(i==1)return 'RT'", thr: "l('t'+i);if(i==1)throw 'TT'", brkAlways: "l('t'+i);break", contAlways: "l('t'+i);continue", retAlways: "l('t'+i);return 'RA'" };
const finActs = { none: "l('f'+i)", brk: "l('f'+i);if(i==1)break", cont: "l('f'+i);if(i==1)continue", ret: "l('f'+i);if(i==1)return 'RF'", thr: "l('f'+i);if(i==1)throw 'TF'", brkAlways: "l('f'+i);break", contAlways: "l('f'+i);continue", retAlways: "l('f'+i);return 'RFA'" };
const catchActs = { none: "l('c'+i+e)", brk: "l('c'+i+e);break", cont: "l('c'+i+e);continue", ret: "l('c'+i+e);return 'RC'", thr: "l('c'+i+e);throw 'TC'" };
for (const [lk, loop] of Object.entries(loopKinds)) for (const [tn, ta] of Object.entries(tryActs)) for (const [fn, fa] of Object.entries(finActs)) {
  addEvery("fin", 2, `W(()=>{${loop(`try{${ta}}finally{${fa}}l('after'+i)`)};return 'end'+i})`);
}
for (const [lk, loop] of Object.entries(loopKinds)) for (const [tn, ta] of Object.entries(tryActs)) for (const [cn, ca] of Object.entries(catchActs)) for (const fa of [finActs.none, finActs.contAlways, finActs.brkAlways]) {
  if (tn === "none" && cn !== "none") continue;
  addEvery("catchfin", 2, `W(()=>{${loop(`try{${ta};if(i==1)throw 'E'}catch(e){${ca}}finally{${fa}}l('after'+i)`)};return 'end'+i})`);
}
// Labels aninhados e continue para fora através de finally.
for (const inner of ["for(var j=0;j<2;j++)", "while(true)", "do", "for(var q in {a:1,b:2})", "for(var q of [1,2])"]) for (const fin of ["l('f')", "l('f');continue inner", "l('f');break inner", "l('f');continue outer", "l('f');break outer", "l('f');break blk", "l('f');return 'RF'"]) for (const body of ["continue outer", "continue inner", "break outer", "break inner", "break blk", "return 'RB'", "throw 'TB'"]) {
  const tail = inner === "do" ? "while(false)" : "";
  const innerBody = `{try{l('b');${body}}finally{${fin}}l('x')}`;
  add(`W(()=>{var n=0;blk:{outer:for(var i=0;i<2;i++){l('o'+i);inner:${inner}${innerBody}${tail};l('y'+i)}l('z')}return 'end'})`.replace("inner:do{", "inner:do{").replace(/inner:do\{try/, "inner:do{try"));
}
for (const lab of ["a:b:c:", "a:{b:", "a:"]) for (const stmt of ["for(var i=0;i<2;i++){l(i);continue a}", "{l(1);break a}", "if(true){l(1);break a}", "switch(1){case 1:l(1);break a}", "try{l(1);break a}finally{l(2)}", "do{l(1);continue a}while(false)", "while(true){l(1);break a}", "for(var i=0;i<2;i++){l(i);continue b}", "{l(1);break b}", "{l(1);break c}", "for(var i=0;i<2;i++){l(i);continue c}"]) {
  const close = lab.endsWith("{b:") ? "}" : "";
  add(`E(${q(`${lab}${stmt}${close}`)})`, `W(()=>{${lab}${stmt}${close};return 'end'})`);
}
add(
  "W(()=>{a:{break a}return 1})", "W(()=>{a:a:{}})", "W(()=>{a:{a:{}}})", "W(()=>{a:{b:{}}a:{}return 1})", "W(()=>{a:{};a:{};return 1})", "W(()=>{a:function f(){}return typeof f})", "W(()=>{a:{function f(){}}return typeof f})", "W(()=>{yield:1;return 1})", "W(()=>{await:1;return 1})", "W(()=>{async:1;return 1})", "W(()=>{let:1;return 1})",
  "W(()=>{'use strict';yield:1})", "W(()=>{'use strict';let:1})", "W(()=>{a:continue a})", "W(()=>{a:{continue a}})", "W(()=>{while(true){a:{break a}break}return 1})", "W(()=>{a:while(true){break a}return 1})", "W(()=>{a:while(true){(()=>{break a})()}})", "W(()=>{a:while(true){break b}})", "W(()=>{break})", "W(()=>{continue})",
  "W(()=>{if(true)break})", "W(()=>{a:if(true){break a}return 1})", "W(()=>{a:if(true)break a;else l(1);return 2})", "W(()=>{a:try{break a}catch(e){}return 1})", "W(()=>{a:try{throw 1}catch(e){break a}return 1})", "W(()=>{a:try{throw 1}catch(e){break a}finally{l('f')}return 1})",
  "W(()=>{for(var i=0;i<2;i++){try{continue}finally{l('f'+i)}}return i})", "W(()=>{for(var i=0;i<2;i++){try{break}finally{l('f'+i)}}return i})", "W(()=>{for(var i=0;i<3;i++){try{if(i<2)continue;break}finally{l('f'+i)}}return i})", "W(()=>{var i=0;do{try{continue}finally{i++;l('f'+i)}}while(i<2);return i})",
  "W(()=>{var i=0;while(i<5){try{i++;continue}finally{if(i>=2)break}}return i})", "W(()=>{var i=0;while(true){try{return i}finally{i++;if(i<3)continue;break}}return 'out'+i})", "W(()=>{function f(){try{return 1}finally{return 2}}return f()})", "W(()=>{function f(){try{throw 1}finally{return 2}}return f()})",
  "W(()=>{function f(){try{return 1}finally{throw 2}}return f()})", "W(()=>{function f(){try{throw 1}finally{throw 2}}return f()})", "W(()=>{function f(){try{return l(1)}finally{l(2)}}return f()})", "W(()=>{function f(){try{return l(1)}finally{l(2);return l(3)}}return f()})", "W(()=>{function f(){for(;;){try{return 1}finally{break}}return 2}return f()})",
  "W(()=>{function f(){for(;;){try{throw 1}finally{break}}return 2}return f()})", "W(()=>{function f(){for(;;){try{throw 1}catch(e){throw 3}finally{break}}return 2}return f()})", "W(()=>{function f(){a:{try{return 1}finally{break a}}return 2}return f()})", "W(()=>{function f(){a:{try{throw 1}finally{break a}}return 2}return f()})",
  "W(()=>{function f(){try{try{return 1}finally{l('i')}}finally{l('o')}}return f()})", "W(()=>{function f(){try{try{return 1}finally{return 2}}finally{l('o')}}return f()})", "W(()=>{function f(){try{try{return 1}finally{return 2}}finally{return 3}}return f()})", "W(()=>{function f(){try{try{throw 1}finally{l('i')}}catch(e){return 'c'+e}finally{l('o')}}return f()})",
  "W(()=>{function f(){try{try{throw 1}catch(e){throw 2}finally{l('i')}}catch(e){return 'c'+e}}return f()})", "W(()=>{function f(){try{try{throw 1}finally{throw 2}}catch(e){return 'c'+e}}return f()})", "W(()=>{function f(){try{try{throw 1}finally{return 2}}catch(e){return 'c'+e}}return f()})", "W(()=>{function f(){var x=1;try{return x}finally{x=2}}return f()})",
  "W(()=>{function f(){var o={v:1};try{return o}finally{o.v=2}}return f().v})", "W(()=>{function f(){var x=1;try{x=2;return x}finally{x=3;l(x)}}return f()})", "W(()=>{function f(){try{return (l('a'),1)}finally{l('b')}}return f()})", "W(()=>{function f(){for(var i of [1,2]){try{return i}finally{l('f'+i)}}}return f()})",
  "W(()=>{function f(){for(var i of {[Symbol.iterator](){return{next(){l('next');return{done:false,value:1}},return(){l('ret');return{}}}}}){try{return i}finally{l('f')}}}return f()})", "W(()=>{function f(){for(var i of {[Symbol.iterator](){return{next(){l('next');return{done:false,value:1}},return(){l('ret');return{}}}}}){try{break}finally{l('f')}}}return f()})",
  "W(()=>{function f(){for(var i of {[Symbol.iterator](){return{next(){l('next');return{done:false,value:1}},return(){l('ret');return{}}}}}){try{continue}finally{l('f');break}}}return f()})", "W(()=>{function f(){for(var i of {[Symbol.iterator](){return{next(){l('next');return{done:l('n')>5,value:1}},return(){l('ret');return{}}}}}){try{throw 1}finally{l('f');continue}}}return f()===undefined})",
  "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){l('ret');throw 'RE'}}}};try{for(var v of it){throw 'BODY'}}catch(e){return e}})", "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){l('ret');throw 'RE'}}}};try{for(var v of it){break}}catch(e){return e}})",
  "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){l('ret');return 1}}}};try{for(var v of it){break}}catch(e){return e.name}})", "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){l('ret');return 1}}}};try{for(var v of it){throw 'BODY'}}catch(e){return e}})",
  "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return:1}}};try{for(var v of it){break}}catch(e){return e.name}})", "W(()=>{var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return:null}}};for(var v of it){break}return 'ok'})",
);

// ---- 6. Generators com return/throw em finally aninhado.
const innerFin = ["l('i')", "yield 'yi'", "return 'ri'", "throw 'ti'", "l('i');yield 'yi';l('i2')", "try{yield 'yi'}finally{l('ii')}", "try{return 'ri'}finally{l('ii')}", "for(var q of[1,2]){yield 'q'+q}"];
const outerFin = ["l('o')", "yield 'yo'", "return 'ro'", "throw 'to'", "l('o');yield 'yo'"];
const drivers = [
  "[['next'],['next'],['next'],['next']]", "[['next'],['return','X'],['next'],['next']]", "[['next'],['throw','X'],['next'],['next']]", "[['return','X'],['next']]", "[['throw','X'],['next']]", "[['next'],['next'],['return','X'],['next']]",
  "[['next'],['next'],['throw','X'],['next']]", "[['next'],['return','X'],['return','Y'],['next']]", "[['next'],['return','X'],['throw','Y'],['next']]", "[['next'],['throw','X'],['return','Y'],['next']]", "[['next'],['next'],['next'],['return','X']]", "[['next','n1'],['next','n2'],['next','n3'],['next','n4']]",
];
const genBodies = [
  (i, o) => `try{try{yield 1;yield 2}finally{${i}}}finally{${o}}`, (i, o) => `try{try{yield 1}catch(e){l('c'+e);yield 'yc'}finally{${i}}}finally{${o}}`, (i, o) => `try{yield 1;try{yield 2}finally{${i}}}finally{${o}}`,
  (i, o) => `try{try{yield 1}finally{${i}}yield 2}finally{${o}}`, (i, o) => `try{try{yield 1}finally{${i}}}catch(e){l('oc'+e);yield 'yoc'}finally{${o}}`,
];
for (const mk of genBodies) for (const i of innerFin) for (const o of outerFin) for (const dr of drivers) {
  if (!["[['next'],['next'],['next'],['next']]", "[['next'],['return','X'],['next'],['next']]", "[['next'],['throw','X'],['next'],['next']]", "[['next'],['next'],['return','X'],['next']]"].includes(dr) && (o !== "l('o')" || i.length > 14)) continue;
  add(`W(()=>{function*g(){${mk(i, o)}}var it=g();var out=[];for(var s of ${dr}){try{out.push(S(it[s[0]](s[1])))}catch(e){out.push('!'+S(e))}}return out})`);
}
add(
  "W(()=>{function*g(){try{yield 1}finally{return 'F'}}var it=g();it.next();return [it.return('X'),it.next()]})", "W(()=>{function*g(){try{yield 1}finally{yield 'F'}}var it=g();it.next();return [it.return('X'),it.next(),it.next()]})", "W(()=>{function*g(){try{yield 1}finally{yield 'F'}}var it=g();it.next();return [it.throw('X')]})",
  "W(()=>{function*g(){try{yield 1}finally{yield 'F'}}var it=g();it.next();var r=[it.throw('X')];try{r.push(it.next())}catch(e){r.push('!'+e)}r.push(it.next());return r})", "W(()=>{function*g(){yield 1}var it=g();return [it.return('X'),it.next()]})", "W(()=>{function*g(){yield 1}var it=g();try{it.throw('X')}catch(e){return [e,it.next()]}})",
  "W(()=>{function*g(){yield 1}var it=g();it.next();it.next();return [it.return('X'),it.next()]})", "W(()=>{function*g(){yield 1}var it=g();it.next();it.next();try{it.throw('X')}catch(e){return e}})", "W(()=>{function*g(){return 'R'}var it=g();return [it.next(),it.next(),it.return('X')]})",
  "W(()=>{function*g(){try{yield 1}catch(e){return 'c'+e}}var it=g();it.next();return [it.throw('X'),it.next()]})", "W(()=>{function*g(){var x=yield 1;return x}var it=g();it.next();return [it.next('V'),it.next('W')]})", "W(()=>{function*g(){var x=yield 1;return x}var it=g();return [it.next('V'),it.next('W')]})",
  "W(()=>{function*g(){yield* [1,2];return 'r'}return [...g()]})", "W(()=>{function*g(){var r=yield* (function*(){yield 1;return 'inner'})();yield r}return [...g()]})", "W(()=>{function*g(){try{yield* (function*(){try{yield 1}finally{l('ii')}})()}finally{l('oo')}}var it=g();it.next();return [it.return('X'),it.next()]})",
  "W(()=>{function*g(){try{yield* (function*(){try{yield 1}finally{yield 'fi'}})()}finally{l('oo')}}var it=g();it.next();return [it.return('X'),it.next(),it.next()]})", "W(()=>{function*g(){try{yield* (function*(){try{yield 1}finally{return 'ri'}})()}finally{l('oo')}}var it=g();it.next();return [it.return('X'),it.next()]})",
  "W(()=>{function*g(){try{yield* (function*(){try{yield 1}catch(e){yield 'ic'+e}})()}catch(e){yield 'oc'+e}}var it=g();it.next();return [it.throw('X'),it.next(),it.next()]})", "W(()=>{var inner={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(v){l('ret'+v);return{done:true,value:'ir'}}}}};function*g(){yield* inner}var it=g();it.next();return [it.return('X'),it.next()]})",
  "W(()=>{var inner={[Symbol.iterator](){return{next(){return{done:false,value:1}}}}};function*g(){yield* inner}var it=g();it.next();try{it.return('X')}catch(e){return e.name}})", "W(()=>{var inner={[Symbol.iterator](){return{next(){return{done:false,value:1}}}}};function*g(){yield* inner}var it=g();it.next();try{it.throw('X')}catch(e){return e.name}})",
  "W(()=>{var inner={[Symbol.iterator](){return{next(){return{done:false,value:1}},throw(e){l('thr'+e);return{done:true,value:'it'}}}}};function*g(){var r=yield* inner;yield 'r'+r}var it=g();it.next();return [it.throw('X'),it.next()]})", "W(()=>{var inner={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(v){l('ret'+v);return 1}}}};function*g(){yield* inner}var it=g();it.next();try{it.return('X')}catch(e){return e.name}})",
  "W(()=>{function*g(){try{yield 1}finally{l('f')}}var it=g();return [it.return('X'),it.next()]})", "W(()=>{function*g(){try{yield 1}finally{l('f')}}var it=g();return [it.throw('X')]})", "W(()=>{function*g(){try{l('start');yield 1}finally{l('f')}}var it=g();try{it.throw('X')}catch(e){}return it.next()})",
  "W(()=>{function*g(){var it2=it;try{yield 1}finally{try{it2.next()}catch(e){l(e.name)}}}var it=g();it.next();return it.return('X')})", "W(()=>{function*g(){try{it.next()}catch(e){yield e.name}}var it=g();return it.next()})", "W(()=>{function*g(){try{yield 1}finally{try{it.return('Y')}catch(e){l(e.name)}}}var it=g();it.next();return it.return('X')})",
  "W(()=>{for(var x of (function*(){try{yield 1;yield 2}finally{l('f')}})()){break}return 'ok'})", "W(()=>{for(var x of (function*(){try{yield 1;yield 2}finally{l('f');yield 'F'}})()){break}return 'ok'})", "W(()=>{for(var x of (function*(){try{yield 1;yield 2}finally{l('f');throw 'FT'}})()){break}})",
  "W(()=>{try{for(var x of (function*(){try{yield 1;yield 2}finally{l('f');throw 'FT'}})()){throw 'B'}}catch(e){return e}})", "W(()=>{var [a]=(function*(){try{yield 1;yield 2}finally{l('f')}})();return a})", "W(()=>{var [a,b]=(function*(){try{yield 1;yield 2}finally{l('f')}})();return a+b})", "W(()=>{var [a,,b]=(function*(){try{yield 1;yield 2;yield 3}finally{l('f')}})();return a+b})",
  "W(()=>{var [...r]=(function*(){try{yield 1;yield 2}finally{l('f')}})();return r})", "W(()=>{var [a=l('d')]=(function*(){try{yield undefined}finally{l('f')}})();return a})", "W(()=>{var [a]=(function*(){try{yield 1}finally{l('f');return 5}})();return a})", "W(()=>{Array.from((function*(){try{yield 1;yield 2}finally{l('f')}})(),x=>{throw 'M'})})",
  "W(()=>{try{Array.from((function*(){try{yield 1;yield 2}finally{l('f')}})(),x=>{throw 'M'})}catch(e){return e}})", "W(()=>{try{new Map((function*(){try{yield 1}finally{l('f')}})())}catch(e){return e.name}})", "W(()=>{try{new Set((function*(){try{yield 1;yield 2}finally{l('f')}})())}catch(e){return e.name}return 'ok'})",
  "W(()=>{try{Promise.all((function*(){try{yield 1}finally{l('f')}})())}catch(e){return e.name}return 'ok'})", "W(()=>{var g=function*(){try{yield 1;yield 2}finally{l('f')}};var s=new Set(g());return s.size})", "W(()=>{var g=function*(){try{yield 1;yield 2}finally{l('f')}};return Math.max(...g())})", "W(()=>{var g=function*(){try{yield 1;yield 2}finally{l('f')}};return [...g()].length})",
  "W(()=>{var g=function*(){try{yield [1,2]}finally{l('f')}};var [[a,b]]=g();return a+b})", "W(()=>{var g=function*(){try{yield 1}finally{l('f')}};var o={};[o.x]=g();return o.x})", "W(()=>{var g=function*(){try{yield 1}finally{l('f')}};var o={};[o[l('k')]]=g();return o.k})", "W(()=>{var g=function*(){try{yield 1}finally{l('f')}};var o={set x(v){throw 'S'}};try{[o.x]=g()}catch(e){return e}})",
  "W(()=>{function*g(){var a=yield 1;l('a'+a);var b=yield 2;l('b'+b);return a+b}var it=g();it.next('ignored');it.next(10);return it.next(20)})", "W(()=>{function*g(){yield yield yield 1}var it=g();return [it.next('a').value,it.next('b').value,it.next('c').value,it.next('d').value,it.next('e').done]})",
  "W(()=>{function*g(){return yield}var it=g();it.next();return it.next('v')})", "W(()=>{function*g(){yield}var it=g();return [it.next().value,it.next().done]})", "W(()=>{function*g(){yield,yield}})", "W(()=>{function*g(){var o={[yield 'k']:yield 'v'};return o}var it=g();it.next();it.next('K');return it.next('V')})",
  "W(()=>{function*g(){return [yield 1,yield 2]}var it=g();it.next();it.next('a');return it.next('b')})", "W(()=>{function*g(){l(yield 1);l(yield 2)}var it=g();it.next();it.next('a');it.next('b');return L.slice()})", "W(()=>{function*g(){try{yield 1}finally{l('f')}}var it=g();it.next();it=null;return 'dropped'})",
  "W(()=>{function*g(){yield this}return g.call(5).next().value})", "W(()=>{function*g(){'use strict';yield this}return g.call(5).next().value})", "W(()=>{function*g(){yield arguments.length}return g(1,2,3).next().value})", "W(()=>{function*g(a=l('d')){l('body')}var it=g();l('created');it.next();return 'x'})", "W(()=>{function*g(a=l('d')){l('body')}var it=g();return Object.getPrototypeOf(it)===g.prototype})",
  "W(()=>{function*g(){}g.prototype=null;return Object.getPrototypeOf(g())===Object.getPrototypeOf(function*(){}.prototype)})", "W(()=>{function*g(){}return typeof g.prototype+Object.getOwnPropertyNames(g.prototype).length})", "W(()=>{function*g(){}return [new.target===undefined]})", "W(()=>{function*g(){}new g})", "W(()=>{var g=function*(){};new g})",
  "W(()=>{var o={*g(){yield 1}};return [...o.g()]})", "W(()=>{var o={*g(){yield 1}};new o.g})", "W(()=>{class A{*g(){yield this}}var a=new A;return a.g().next().value===a})", "W(()=>{class A{static*g(){yield this}}return A.g().next().value===A})", "W(()=>{var o={*[Symbol.iterator](){yield 1;yield 2}};return [...o]})",
);

// ---- 7. Arrows async com this e arguments (R gravado em callback, lido depois das microtarefas).
const asyncCases = [
  "function f(){return (async()=>this)()}f.call({v:1}).then(v=>{R=S(v)})", "function f(){return (async()=>arguments[0])()}f(7).then(v=>{R=S(v)})", "function f(){return (async()=>arguments.length)()}f(1,2,3).then(v=>{R=S(v)})",
  "function f(){return (async()=>{await 0;return this})()}f.call({v:2}).then(v=>{R=S(v)})", "function f(){return (async()=>{await 0;return arguments[0]})()}f(8).then(v=>{R=S(v)})", "function f(){return (async()=>{await 0;await 0;return [this.v,arguments[0]]})()}f.call({v:3},9).then(v=>{R=S(v)})",
  "function f(){var a=async()=>{return (async()=>this.v)()};return a()}f.call({v:4}).then(v=>{R=S(v)})", "function f(){var a=async()=>{await 0;return (async()=>arguments[0])()};return a()}f(5).then(v=>{R=S(v)})", "function f(){'use strict';return (async()=>this)()}f.call(undefined).then(v=>{R=S(v)})",
  "function f(){'use strict';return (async()=>this)()}f.call(5).then(v=>{R=S(v)+typeof v})", "function f(){return (async()=>this)()}f.call(5).then(v=>{R=typeof v})", "function f(){return (async()=>this)()}f.call(undefined).then(v=>{R=String(v===globalThis)})",
  "var o={m(){return (async()=>this===o)()}};o.m().then(v=>{R=S(v)})", "var o={m(){return [1].map(async()=>this===o)[0]}};o.m().then(v=>{R=S(v)})", "var o={async m(){return (()=>this===o)()}};o.m().then(v=>{R=S(v)})", "var o={async m(){await 0;return (()=>this===o)()}};o.m().then(v=>{R=S(v)})",
  "var o={async m(){return arguments.length}};o.m(1,2).then(v=>{R=S(v)})", "var o={async m(){await 0;return arguments.length}};o.m(1,2).then(v=>{R=S(v)})", "var o={async m(a){arguments[0]=9;await 0;return a}};o.m(1).then(v=>{R=S(v)})", "var o={async m(a){a=9;await 0;return arguments[0]}};o.m(1).then(v=>{R=S(v)})",
  "var o={async m(a=0){arguments[0]=9;await 0;return a}};o.m(1).then(v=>{R=S(v)})", "var o={async m(){return (async()=>(await 0,arguments[1]))()}};o.m(1,'two').then(v=>{R=S(v)})", "class A{async m(){return this instanceof A}}new A().m().then(v=>{R=S(v)})", "class A{async m(){await 0;return this instanceof A}}new A().m().then(v=>{R=S(v)})",
  "class A{static async m(){return this===A}}A.m().then(v=>{R=S(v)})", "class A{async m(){return (async()=>this instanceof A)()}}new A().m().then(v=>{R=S(v)})", "class A{x=(async()=>this)();}new A().x.then(v=>{R=S(v instanceof A)})", "class A{x=async()=>this;}new A().x().then(v=>{R=S(v instanceof A)})",
  "class A{static x=(async()=>this)()}A.x.then(v=>{R=S(v===A)})", "class A extends Object{constructor(){super();this.p=(async()=>this)()}}new A().p.then(v=>{R=S(v instanceof A)})", "class A extends Object{constructor(){var f=async()=>{await 0;return this};super();this.p=f()}}new A().p.then(v=>{R=S(v instanceof A)})",
  "class A extends Object{constructor(){var f=async()=>this;try{f().then(()=>{},()=>{});}catch(e){}super();this.p=f()}}new A().p.then(v=>{R=S(v instanceof A)})", "class A extends Object{constructor(){var f=async()=>this;this.q=f();super()}}try{new A}catch(e){R=e.name}",
  "class A extends Object{constructor(){var f=async()=>this;f().then(()=>{R='ok'},e=>{R=e.name});super()}}new A", "class A extends Object{constructor(){var f=async()=>{await 0;return this};var p=f();super();p.then(v=>{R=S(v===this)})}}new A", "class A extends Object{constructor(){var f=async()=>{super();return this};f().then(v=>{R=S(v instanceof A)})}}new A",
  "function f(){var g=async(a=arguments[0])=>a;return g()}f('d').then(v=>{R=S(v)})", "function f(){var g=async(a=this)=>a;return g()}f.call({v:1}).then(v=>{R=S(v)})", "function f(){var g=async(a=arguments.length)=>a;return g(undefined)}f(1,2).then(v=>{R=S(v)})", "function f(){return (async(...args)=>[args.length,arguments.length])(1,2,3)}f(1).then(v=>{R=S(v)})",
  "function f(){var g=async function(){return typeof this};return g()}f.call(1).then(v=>{R=S(v)})", "function f(){var g=async function(){'use strict';return typeof this};return g()}f.call(1).then(v=>{R=S(v)})", "function f(){var g=async function(){return arguments.length};return g(1,2)}f(1).then(v=>{R=S(v)})", "var g=async function(){return this===globalThis||this===undefined};g().then(v=>{R=S(v)})",
  "var g=async()=>typeof arguments;g().then(v=>{R=S(v)},e=>{R=e.name})", "var g=async()=>{await 0;return typeof this};g().then(v=>{R=S(v)})", "var g=async()=>this===globalThis;g().then(v=>{R=S(v)})", "var g=async()=>this===undefined;g().then(v=>{R=S(v)})", "var g=()=>{try{return arguments.length}catch(e){return e.name}};R=S(g())",
  "var r=[];function f(){var a=async()=>{r.push(1);await 0;r.push(3)};a();r.push(2)}f();Promise.resolve().then(()=>{R=r.join()})", "var r=[];async function f(){r.push('a');await null;r.push('c')}f();r.push('b');Promise.resolve().then(()=>Promise.resolve()).then(()=>{R=r.join()})", "var r=[];async function f(){try{await Promise.reject('x')}catch(e){r.push('c'+e)}finally{r.push('f')}return 'done'}f().then(v=>{r.push(v);R=r.join()})",
  "var r=[];async function f(){for(var i=0;i<3;i++){try{if(i==1)continue;await i;r.push('t'+i)}finally{r.push('f'+i)}}return r.join()}f().then(v=>{R=v})", "var r=[];async function f(){l1:for(var i=0;i<2;i++){for(var j=0;j<2;j++){try{await 0;if(j==0)continue l1;r.push(i+''+j)}finally{r.push('f'+i+j)}}}return r.join()}f().then(v=>{R=v})",
  "var r=[];async function f(){try{return await 'a'}finally{r.push('f')}}f().then(v=>{R=v+r.join()})", "var r=[];async function f(){try{return 'a'}finally{await 0;r.push('f')}}f().then(v=>{R=v+r.join()})", "var r=[];async function f(){try{throw 'a'}finally{return 'overridden'}}f().then(v=>{R=v},e=>{R='rej'+e})", "var r=[];async function f(){try{return Promise.reject('a')}finally{r.push('f')}}f().then(v=>{R=v},e=>{R='rej'+e+r.join()})",
  "var r=[];async function f(){try{return await Promise.reject('a')}catch(e){r.push('c'+e);return 'r'}}f().then(v=>{R=v+r.join()})", "async function f(){return {then(res){res('thenable')}}}f().then(v=>{R=S(v)})", "async function f(){return await {then(res){res('thenable')}}}f().then(v=>{R=S(v)})", "async function f(){await {then(res,rej){rej('rejected')}}}f().then(()=>{R='ok'},e=>{R=S(e)})",
  "async function f(){var x=await 1;var y=await x+1;return x+y}f().then(v=>{R=S(v)})", "async function f(){return [await 1,await 2,await 3]}f().then(v=>{R=S(v)})", "async function f(){return {a:await 1,b:await 2}}f().then(v=>{R=S(v)})", "async function f(){var a=[];for await(var x of [1,Promise.resolve(2),3])a.push(x);return a}f().then(v=>{R=S(v)})",
  "async function f(){var a=[];for await(var x of (async function*(){yield 1;yield 2})())a.push(x);return a}f().then(v=>{R=S(v)})", "async function f(){var a=[];try{for await(var x of (async function*(){try{yield 1;yield 2}finally{a.push('f')}})()){a.push(x);break}}finally{a.push('o')}return a}f().then(v=>{R=S(v)})",
  "async function* g(){try{yield 1;yield 2}finally{yield 'f'}}var it=g();it.next().then(a=>it.return('X').then(b=>it.next().then(c=>{R=S([a,b,c])})))", "async function* g(){try{yield 1}finally{try{yield 'a'}finally{yield 'b'}}}var it=g();var out=[];it.next().then(a=>{out.push(a.value);return it.return('X')}).then(b=>{out.push(b.value);return it.next()}).then(c=>{out.push(c.value);return it.next()}).then(d=>{out.push(d.value,d.done);R=S(out)})",
  "async function* g(){try{yield 1}finally{return 'ri'}}var it=g();it.next().then(()=>it.return('X')).then(b=>{R=S(b)})", "async function* g(){try{yield 1}finally{throw 'ti'}}var it=g();it.next().then(()=>it.return('X')).then(b=>{R=S(b)},e=>{R='rej'+e})", "async function* g(){try{yield 1}catch(e){yield 'c'+e}}var it=g();it.next().then(()=>it.throw('X')).then(b=>{R=S(b)})",
  "async function* g(){yield* [1,Promise.resolve(2)]}var o=[];(async()=>{for await(var x of g())o.push(x);R=S(o)})()", "async function* g(){var x=yield 1;yield x}var it=g();it.next().then(()=>it.next('v')).then(b=>{R=S(b)})", "async function* g(){return this}g.call(5).next().then(v=>{R=typeof v.value})", "async function* g(){return arguments.length}g(1,2).next().then(v=>{R=S(v)})",
  "var o={async*g(){yield this===o}};o.g().next().then(v=>{R=S(v)})", "class A{async*g(){yield this instanceof A}}new A().g().next().then(v=>{R=S(v)})", "var r=[];var it=(async function*(){r.push('start');yield 1;r.push('end')})();r.push('created');it.next().then(()=>{R=r.join()})", "var r=[];var it=(async function*(){r.push('start')})();it.return('X').then(v=>{R=S(v)+r.join()})",
  "var it=(async function*(){})();it.throw('X').then(()=>{R='ok'},e=>{R='rej'+e})", "var it=(async function*(){yield 1})();it.next();it.next();it.next().then(v=>{R=S(v)})", "var it=(async function*(){yield 1})();Promise.all([it.next(),it.next(),it.next()]).then(v=>{R=S(v)})",
  "var it=(async function*(){try{yield 1;yield 2}finally{yield 'f'}})();Promise.all([it.next(),it.return('X'),it.next(),it.next()]).then(v=>{R=S(v)})", "var it=(async function*(){try{yield 1}catch(e){yield e}})();Promise.all([it.next(),it.throw('E'),it.next()]).then(v=>{R=S(v)})",
  "var r=[];Promise.resolve().then(()=>r.push(1));(async()=>{r.push(0);await 0;r.push(2)})();Promise.resolve().then(()=>r.push(3)).then(()=>{R=r.join()})", "var r=[];(async()=>{await Promise.resolve();r.push('a')})();Promise.resolve().then(()=>r.push('b')).then(()=>r.push('c')).then(()=>{R=r.join()})", "var r=[];(async()=>{await new Promise(res=>res());r.push('a')})();Promise.resolve().then(()=>r.push('b')).then(()=>r.push('c')).then(()=>{R=r.join()})",
  "var r=[];(async()=>{await {then(res){res()}};r.push('a')})();Promise.resolve().then(()=>r.push('b')).then(()=>r.push('c')).then(()=>{R=r.join()})", "var r=[];(async()=>{r.push(await 1)})();Promise.resolve().then(()=>r.push('b')).then(()=>{R=r.join()})", "var r=[];async function f(){return 1}f().then(()=>r.push('f'));Promise.resolve().then(()=>r.push('p')).then(()=>{R=r.join()})",
  "var r=[];async function f(){return Promise.resolve(1)}f().then(()=>r.push('f'));Promise.resolve().then(()=>r.push('p')).then(()=>r.push('q')).then(()=>r.push('s')).then(()=>{R=r.join()})", "var r=[];async function f(){return {then(res){res(1)}}}f().then(()=>r.push('f'));Promise.resolve().then(()=>r.push('p')).then(()=>r.push('q')).then(()=>r.push('s')).then(()=>{R=r.join()})",
];
for (const body of asyncCases) add(`R="unset";try{${body}}catch(e){R="sync "+e.name}`);

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("completion_order_bun.tsv", ["completion_value_bun.tsv", "control_flow_bun.tsv", "control_flow_more_bun.tsv", "annexb_bun.tsv", "scope_bun.tsv", "statements_bun.tsv", "template_edge_bun.tsv", "operator_edge_bun.tsv", "call_edge_bun.tsv", "eval_bun.tsv", "eval_scope_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = pool.resolve().filter(e => !seen.has(e) && seen.add(e));
const PRELOAD = writeResultPreload();
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = PRELUDE + (/^R=/.test(expr) ? expr : `globalThis.R = ${/^[WTEF]\(/.test(expr) ? expr : `T(()=>{${expr}})`}`);
  let result;
  try {
    // Processo fresco por programa: o estado global (var, function, eval indireto) não vaza entre casos.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + String(e).slice(0, 120) + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("completion_order", rows));
