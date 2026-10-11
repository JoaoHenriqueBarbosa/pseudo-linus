// Gera tests/golden/operator_edge_bun.tsv: operadores de borda que os goldens de coerção e número não cobrem, medidos
// no bun 1.4.2. Igualdade (== e ===) entre pares de tipos (null, undefined, BigInt, Symbol, wrappers, objetos),
// `document.all` inexistente, relacionais com BigInt e string numérica, `+` com objetos e Date, typeof/void/in/
// instanceof com Symbol.hasInstance, ++/-- em string/BigInt/objeto, ordem de avaliação em `a[b()] = c()`, compound
// assignment com getters/setters e Proxy, vírgula, exponenciação com negativos, shifts com BigInt, NOT bit a bit em
// NaN/Infinity, Object.is, SameValueZero (Map/Set/includes/indexOf), coerção de chave de propriedade (-0, números
// grandes, símbolos) e ToNumber/ToString/ToPrimitive com erros e hints.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// O programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro) e `R` é capturado na saída do processo.
// Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-operator-edge-golden.js > tests/golden/operator_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  "var L=[];\n" +
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":typeof v==="object"&&v!==null&&Object.prototype.toString.call(v)==="[object Object]"?"{"+Object.keys(v).map(k=>k+":"+S(v[k])).join(",")+"}":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function LOG(f){L=[];var r=T(f);return r+" | "+L.join(" ")}\n' +
  // Objeto que registra qual hook de conversão foi chamado e devolve o valor dado.
  "function mk(n,v,s){return {valueOf(){L.push(n+'.v');return v},toString(){L.push(n+'.s');return s===undefined?'S'+n:s}}}\n" +
  "function tp(n,fn){return {[Symbol.toPrimitive](h){L.push(n+':'+h);return fn(h)}}}\n";

const programs = [];
const add = body => programs.push(body);
// Expressão avaliada com captura de erro.
const E = expr => add(`globalThis.R = T(()=>${expr});`);
// Expressão com o log de ordem de avaliação.
const G = expr => add(`globalThis.R = LOG(()=>${expr});`);
// Corpo de várias instruções, com R atribuído dentro de try/catch.
const B = body => add(`try { ${body} } catch (e) { globalThis.R = e.name + ': ' + e.message }`);

// ---- 1. == e === entre pares de tipos (17 valores, pares não ordenados com repetição: 153 programas).
const eqValues = [
  "undefined", "null", "true", "false", "0", "-0", "NaN", "''", "'0'", "'a'", "1n", "0n", "'1'", "Symbol.iterator", "({})", "[]", "[0]",
];
for (let i = 0; i < eqValues.length; i++) {
  for (let j = i; j < eqValues.length; j++) {
    const a = eqValues[i];
    const b = eqValues[j];
    E(`[${a} == ${b}, ${b} == ${a}, ${a} === ${b}, ${a} != ${b}, ${a} !== ${b}]`);
  }
}
// Mesma referência, wrappers e identidade.
for (const e of [
  "(()=>{var o={};return [o==o,o===o,o==={},o=={}]})()", "(()=>{var s=Symbol();return [s==s,s===s,s==Symbol(),Symbol.for('x')==Symbol.for('x')]})()",
  "[new Number(1)==1,new Number(1)===1,new Number(1)==new Number(1),new String('a')=='a',new String('a')==new String('a'),new Boolean(false)==false,new Boolean(false)==true]",
  "[Object(1n)==1n,Object(1n)===1n,Object(Symbol.iterator)==Symbol.iterator,Object(Symbol.iterator)===Symbol.iterator]",
  "[null==0,null>=0,null<=0,null>0,undefined==0,undefined>=0,undefined<=0,NaN==NaN,NaN!=NaN,NaN===NaN]",
  "[typeof document, typeof document=='undefined', typeof globalThis.document, this.document===undefined]",
  "[null==undefined,undefined==null,null===undefined,void 0==null,(function(){})()==null]",
  "[[]==![],[]==false,[0]==false,[1]==true,[2]==true,[[]]==0,[[[]]]==0,[null]==0,[undefined]==0,[null]=='',[[1]]==1]",
  "['1'==1,'1.0'==1,' 1 '==1,'0x10'==16,'1e3'==1000,'Infinity'==Infinity,'-0'==0,'+1'==1,'1_0'==10,''==0,' '==0,'\\n'==0]",
  "[1n=='1',1n==' 1 ',1n=='1.0',1n=='0x1',1n=='1n',0n=='',0n==' ',1n==1.0,1n==1.5,2n**64n==2**64,2n**64n==18446744073709551616,2n**53n+1n==2**53]",
  "[1n==true,0n==false,2n==true,-1n==true,1n===1,1n==Object(1n),Object(1n)==Object(1n)]",
  "[10n=='10',10n=='010',10n=='1e1',10n==' 10 ',10n=='-10',-10n=='-10',-10n=='-0x1']",
  "[Infinity==1n,-Infinity==1n,NaN==1n,1n==Infinity,2n**1024n==Infinity,Number.MAX_VALUE==2n**1024n-1n]",
]) E(e);
// valueOf/toString/Symbol.toPrimitive em == (ordem e hint).
for (const [name, expr] of [
  ["ov", "mk('a',1)==1"], ["ov2", "1==mk('a',1)"], ["ov3", "mk('a',1)==mk('b',1)"], ["ov4", "mk('a',1)==='1'"], ["ov5", "mk('a','1')=='1'"],
  ["os", "mk('a',{},'1')==1"], ["on", "mk('a',null)==null"], ["ou", "mk('a',undefined)==undefined"], ["ob", "mk('a',true)==true"],
  ["tp1", "tp('a',h=>1)==1"], ["tp2", "tp('a',h=>'x')=='x'"], ["tp3", "tp('a',h=>1n)==1"], ["tp4", "tp('a',h=>1n)=='1'"], ["tp5", "tp('a',h=>Symbol.iterator)==Symbol.iterator"],
  ["tp6", "tp('a',h=>1)==tp('b',h=>1)"], ["tp7", "tp('a',h=>({}))==1"], ["tp8", "tp('a',h=>1)==null"], ["tp9", "tp('a',h=>1)==undefined"],
  ["tp10", "tp('a',h=>1)===1"], ["tp11", "1==tp('a',h=>1n)"], ["tp12", "tp('a',h=>'1')==1n"], ["tp13", "tp('a',h=>1)!=1"],
  ["tp14", "[tp('a',h=>0)==false,tp('b',h=>'')==0,tp('c',h=>null)==0]"],
  ["sym1", "Object(Symbol.iterator)==Symbol.iterator"], ["sym2", "Symbol.iterator==mk('a',Symbol.iterator)"], ["sym3", "Symbol.iterator=='Symbol(Symbol.iterator)'"],
]) G(expr);

// ---- 2. Relacionais com BigInt e string numérica (8 valores, as quatro operações juntas).
const relValues = ["1n", "2n", "'1'", "'2'", "' 2 '", "'abc'", "1.5", "NaN"];
for (const a of relValues) for (const b of relValues) E(`[${a} < ${b}, ${a} <= ${b}, ${a} > ${b}, ${a} >= ${b}]`);
for (const e of [
  "['10'<'9',10<'9','10'<9,'10'<9n,10n<'9','a'<'b','a'<'B','Z'<'a','' < 'a','a' < '', 'a'<'ab']",
  "[2n**64n>2**64,2n**64n<2**64+1,2n**64n+1n>2**64,2n**1024n>Number.MAX_VALUE,-(2n**1024n)<-Number.MAX_VALUE,2n**1024n<Infinity]",
  "[1n<'x',1n>'x',1n<=undefined,1n>=null,0n<=null,0n>=null,1n<'1n',1n<'0x2',1n<'',0n<=''] ",
  "[9007199254740993n>9007199254740992,9007199254740993n>=9007199254740992,9007199254740993n<9007199254740994,9007199254740993n==9007199254740992]",
  "[-0<0,-0<=0,0n<-0,-0n<=0,0n>=-0,1n<Infinity,-1n>-Infinity,1n<NaN,1n>NaN,1n>=NaN]",
  "[null<1,null<=0,null>=0,undefined<1,undefined>=0,undefined<=undefined,[]<1,[2]>1,[1,2]<3,({})<1,({})>=({})]",
  "[Symbol.iterator==Symbol.iterator,1<2<3,3>2>1,3>2>=1,1<2==true,2>1==1]",
  "['a'<1,'a'>1,'1'<'a','1'<=1,'01'<'1','01'<1,'01'==1,'1e2'>'99']",
  "[1n<'1.5',1n<'9007199254740993',9007199254740993n<'9007199254740994',-1n<'-0.5',0n<'  ',1n<'Infinity']",
]) E(e);
for (const expr of [
  "mk('a',1)<mk('b',2)", "mk('a',1)>mk('b',2)", "mk('a',1)<=mk('b',2)", "mk('a',1)>=mk('b',2)", "mk('a',1n)<2n", "2n>mk('a',1n)", "mk('a','1')<mk('b','2')",
  "tp('a',h=>1)<tp('b',h=>2)", "tp('a',h=>1)>tp('b',h=>2)", "tp('a',h=>1)<=tp('b',h=>2)", "tp('a',h=>1)>=tp('b',h=>2)", "tp('a',h=>1n)<2", "tp('a',h=>'1')<2n",
  "tp('a',h=>Symbol.iterator)<1", "tp('a',h=>({}))<1", "mk('a',{},{})<1", "mk('a',NaN)<mk('b',1)", "mk('a',1)<mk('b',NaN)", "mk('a',1)>=mk('b',NaN)",
  "mk('a',1)<undefined", "mk('a',undefined)<mk('b',1)", "mk('a',1)<mk('b',{},{})",
]) G(expr);

// ---- 3. `+` com objetos, Date e hints.
for (const e of [
  "[1+{},{}+1,[]+[],[]+{},({})+({}),[1]+[2],[1,2]+[3],null+[],undefined+[],1+[],1+[2],'a'+[],[]+null]",
  "[1+null,1+undefined,'1'+null,'1'+undefined,true+true,true+'1',1+true,null+null,undefined+undefined,null+undefined]",
  "[1n+1n,1n+'1','1'+1n,1n+[],[]+1n,1n+{}]",
  "T(()=>1n+1)", "T(()=>1+1n)", "T(()=>1n+null)", "T(()=>1n+true)", "T(()=>1n+Symbol.iterator)", "T(()=>Symbol.iterator+'')", "T(()=>''+Symbol.iterator)",
  "T(()=>`${Symbol.iterator}`)", "T(()=>Symbol.iterator+1)", "T(()=>+Symbol.iterator)", "T(()=>-Symbol.iterator)", "T(()=>+1n)", "T(()=>-1n)", "T(()=>~Symbol.iterator)",
  "T(()=>String(Symbol.iterator))", "T(()=>Number(Symbol.iterator))", "T(()=>Number(1n))", "T(()=>Number('1n'))", "T(()=>BigInt('1.5'))", "T(()=>BigInt(1.5))", "T(()=>BigInt(' 12 '))",
  "T(()=>BigInt(''))", "T(()=>BigInt('0x1f'))", "T(()=>BigInt('1e3'))", "T(()=>BigInt(NaN))", "T(()=>BigInt(undefined))", "T(()=>BigInt(null))", "T(()=>BigInt(true))", "T(()=>BigInt(Symbol.iterator))",
  "[new Date(0)+1,typeof (new Date(0)+1),typeof (new Date(0)-1),new Date(5)-new Date(2),+new Date(7),new Date(0)*1,new Date(NaN)+1,new Date(NaN)-1]",
  "[new Date(0)==0,new Date(0)=='Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)',new Date(1)>new Date(0),new Date(1)<=new Date(1),new Date(1)==new Date(1)]",
  "[new Date(0)[Symbol.toPrimitive]('number'),typeof new Date(0)[Symbol.toPrimitive]('default'),typeof new Date(0)[Symbol.toPrimitive]('string')]",
  "T(()=>new Date(0)[Symbol.toPrimitive]('x'))", "T(()=>new Date(0)[Symbol.toPrimitive]())", "T(()=>Date.prototype[Symbol.toPrimitive].call(1,'number'))",
  "T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3},toString(){return 'x'}},'number'))",
  "T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3},toString(){return 'x'}},'default'))",
  "T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3},toString(){return 'x'}},'string'))",
  "T(()=>Date.prototype[Symbol.toPrimitive].length+Date.prototype[Symbol.toPrimitive].name)",
  "T(()=>Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).writable)",
  "T(()=>({}+''))", "T(()=>(function(){}+'').length>0)", "T(()=>[1,[2,[3]]]+'')", "T(()=>[null,undefined,1]+'')", "T(()=>({toString:null,valueOf:()=>7})+1)",
  "T(()=>({toString:()=>'t',valueOf:null})+1)", "T(()=>({toString:null,valueOf:null})+1)", "T(()=>({toString:()=>({}),valueOf:()=>({})})+1)",
  "T(()=>Object.create(null)+1)", "T(()=>Object.create(null)=='')", "T(()=>`${Object.create(null)}`)", "T(()=>[Object.create(null)]+'')",
]) (/^T\(/.test(e) ? E(e.slice(2, -1).replace(/^\(\)=>/, "")) : E(e));
for (const expr of [
  "mk('a',1)+mk('b',2)", "mk('a','x')+mk('b','y')", "mk('a',1)+'s'", "'s'+mk('a',1)", "`${mk('a',1)}`", "mk('a',1)+1n", "mk('a',1n)+1n", "mk('a',1n)+mk('b',2)",
  "tp('a',h=>h)+tp('b',h=>h)", "tp('a',h=>h)+1", "1+tp('a',h=>h)", "`${tp('a',h=>h)}`", "tp('a',h=>h)*1", "tp('a',h=>1)*tp('b',h=>2)", "String(tp('a',h=>h))",
  "Number(tp('a',h=>1))", "tp('a',h=>h)+'x'", "'x'+tp('a',h=>h)", "[tp('a',h=>h)]+''", "({[tp('a',h=>'k')]:1})", "tp('a',h=>h)-tp('b',h=>h)",
  "tp('a',h=>null)+1", "tp('a',h=>undefined)+1", "tp('a',h=>({}))+1", "tp('a',h=>Symbol.iterator)+''", "tp('a',h=>{throw new RangeError('boom')})+1",
  "mk('a',{},{})+1", "({[Symbol.toPrimitive]:undefined,valueOf(){L.push('v');return 4}})+1", "({[Symbol.toPrimitive]:null,valueOf(){L.push('v');return 4}})+1",
  "({[Symbol.toPrimitive]:1})+1", "({[Symbol.toPrimitive]:{}})+1", "({get [Symbol.toPrimitive](){L.push('get');return h=>2}})+1",
  "new Date(0)+mk('a',1)", "mk('a',1)+new Date(0)", "[mk('a',1),mk('b',2)]+''", "mk('a',1)==mk('b',1)", "Object(mk('a',1))+1",
  "new Proxy({},{get(t,k){L.push('get:'+String(k));return undefined}})+''",
  "new Proxy({},{get(t,k){L.push('get:'+String(k));return k===Symbol.toPrimitive?h=>5:undefined}})+1",
]) G(expr);

// ---- 4. typeof, void, in, instanceof, delete e Symbol.hasInstance.
for (const e of [
  "[typeof null,typeof undefined,typeof 1,typeof 1n,typeof '',typeof Symbol(),typeof {},typeof [],typeof function(){},typeof class{},typeof Math,typeof new Number(1)]",
  "[typeof undeclared,typeof undeclared.x===undefined]", "[typeof typeof 1,typeof void 0,typeof (()=>{}),typeof async function(){},typeof function*(){},typeof Proxy,typeof new Proxy(function(){},{})]",
  "[typeof new Proxy({},{}),typeof new Proxy([],{}),typeof new Proxy(class{},{}),typeof Object(1n),typeof Object(Symbol.iterator)]",
  "[void 0,void 'x',void (L.push('v')),void 1+1,typeof void 0,(void 0)===undefined]", "[void {}, void [], void function(){}]",
  "[1 in [1,2],2 in [1,2],'length' in [],'0' in 'abc'.split(''),Symbol.iterator in [],-0 in [1],'-0' in [1],0 in [1],'00' in [1],1.0 in [1,2]]",
  "T(()=>'a' in 'abc')", "T(()=>'a' in 1)", "T(()=>'a' in null)", "T(()=>'a' in undefined)", "T(()=>'a' in Symbol.iterator)", "T(()=>1n in [1,2])", "T(()=>0n in [1])",
  "T(()=>({}) in {})", "T(()=>[] in {'':1})", "T(()=>'toString' in {})", "T(()=>'toString' in Object.create(null))", "T(()=>Symbol.toPrimitive in Date.prototype)",
  "T(()=>(1,2) in {2:1})", "T(()=>1 in new Proxy([5,6],{has(t,k){L.push('has:'+String(k));return true}}))",
  "T(()=>1 instanceof Number)", "T(()=>new Number(1) instanceof Number)", "T(()=>({}) instanceof Object)", "T(()=>Object.create(null) instanceof Object)",
  "T(()=>1 instanceof 1)", "T(()=>({}) instanceof {})", "T(()=>({}) instanceof (()=>{}))", "T(()=>({}) instanceof Math.max)", "T(()=>({}) instanceof null)",
  "T(()=>({}) instanceof undefined)", "T(()=>({}) instanceof Symbol)", "T(()=>Symbol() instanceof Symbol)", "T(()=>1n instanceof BigInt)", "T(()=>Object(1n) instanceof BigInt)",
  "T(()=>[] instanceof Array)", "T(()=>[] instanceof Object)", "T(()=>(function(){}) instanceof Function)", "T(()=>(class{}) instanceof Function)",
  "T(()=>({}) instanceof {[Symbol.hasInstance]:()=>true})", "T(()=>1 instanceof {[Symbol.hasInstance]:()=>1})", "T(()=>1 instanceof {[Symbol.hasInstance]:()=>0})",
  "T(()=>1 instanceof {[Symbol.hasInstance]:()=>'x'})", "T(()=>1 instanceof {[Symbol.hasInstance]:1})", "T(()=>1 instanceof {[Symbol.hasInstance]:null})",
  "T(()=>1 instanceof {[Symbol.hasInstance]:undefined})", "T(()=>({}) instanceof {[Symbol.hasInstance]:undefined})", "T(()=>({}) instanceof {[Symbol.hasInstance]:{}})",
  "T(()=>1 instanceof {get [Symbol.hasInstance](){throw new EvalError('g')}})", "T(()=>1 instanceof {[Symbol.hasInstance](){throw new EvalError('h')}})",
  "T(()=>{class C{static [Symbol.hasInstance](v){return v===1}};return [1 instanceof C,2 instanceof C,new C instanceof C]})",
  "T(()=>{class C{static [Symbol.hasInstance](v){return this===C}};class D extends C{};return [1 instanceof C,1 instanceof D]})",
  "T(()=>{function F(){};F.prototype=1;return {} instanceof F})", "T(()=>{function F(){};F.prototype=null;return {} instanceof F})", "T(()=>{function F(){};F.prototype=undefined;return 1 instanceof F})",
  "T(()=>{function F(){};F.prototype=1;return 1 instanceof F})", "T(()=>{function F(){};F.prototype={};var o=new F;F.prototype={};return o instanceof F})",
  "T(()=>{function F(){};var o=new F;Object.setPrototypeOf(o,null);return o instanceof F})", "T(()=>{var f=function(){}.bind();return {} instanceof f})",
  "T(()=>{function F(){};var B=F.bind();return [new F instanceof B,new B instanceof F,new B instanceof B]})",
  "T(()=>{function F(){};var B=F.bind();Object.defineProperty(B,'prototype',{value:{}});return new F instanceof B})",
  "T(()=>Function.prototype[Symbol.hasInstance].call(Array,[]))", "T(()=>Function.prototype[Symbol.hasInstance].call({},[]))", "T(()=>Function.prototype[Symbol.hasInstance].call(1,1))",
  "T(()=>Function.prototype[Symbol.hasInstance].name+Function.prototype[Symbol.hasInstance].length)",
  "T(()=>Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance))",
  "T(()=>{var p=new Proxy(function(){},{get(t,k){L.push('get:'+String(k));return t[k]}});return {} instanceof p})",
  "T(()=>({}) instanceof new Proxy({},{}))", "T(()=>[] instanceof new Proxy(Array,{}))", "T(()=>{var o=Object.create(Array.prototype);return [o instanceof Array,Array.isArray(o)]})",
  "T(()=>delete Math.PI)", "T(()=>delete 1)", "T(()=>delete undeclared)", "T(()=>delete [].length)", "T(()=>delete [][0])", "T(()=>delete {a:1}.a)", "T(()=>delete 'abc'[0])", "T(()=>delete 'abc'[5])",
  "T(()=>delete 'abc'.length)", "T(()=>{'use strict';return delete Math.PI})", "T(()=>{'use strict';return delete 'abc'[0]})", "T(()=>{'use strict';return delete [].length})",
  "T(()=>delete new Proxy({a:1},{deleteProperty(t,k){L.push('del:'+String(k));return false}}).a)",
  "T(()=>{'use strict';return delete new Proxy({a:1},{deleteProperty(t,k){return false}}).a})",
  "T(()=>{'use strict';return delete new Proxy({a:1},{deleteProperty(t,k){return 0}}).a})",
  "T(()=>!!document)", "T(()=>typeof document==='undefined'?'nodoc':document)", "T(()=>typeof globalThis.document)", "T(()=>[typeof HTMLAllCollection,typeof document])",
  "T(()=>[null==undefined,typeof null,null instanceof Object,void 0 instanceof Object])",
]) {
  if (/^T\(/.test(e)) E(e.slice(2, -1).replace(/^\(\)=>/, "")); else E(e);
}

// ---- 5. ++/-- em string, BigInt, objeto, undefined, null e propriedades.
const incValues = ["'5'", "'a'", "''", "' 3 '", "'0x10'", "'1e2'", "1n", "-1n", "0n", "null", "undefined", "true", "false", "-0", "NaN", "Infinity", "[]", "[7]", "({})", "new Number(2)", "new String('9')", "Object(3n)", "'9007199254740993'", "9007199254740992", "'1n'", "Symbol.iterator"];
for (const v of incValues) {
  E(`(()=>{var x=${v};var a=x++;return [a,x,typeof a,typeof x]})()`);
  E(`(()=>{var x=${v};var a=--x;return [a,x,typeof a,typeof x]})()`);
  E(`(()=>{var o={p:${v}};var a=o.p++;var b=++o.p;return [a,b,o.p]})()`);
  E(`(()=>{var o={p:${v}};var a=o.p--;var b=--o.p;return [a,b,o.p]})()`);
}
for (const expr of [
  "(()=>{var x=mk('a',5);var r=x++;return [typeof r,typeof x,r]})()", "(()=>{var x=mk('a',5n);var r=++x;return [typeof r,r]})()",
  "(()=>{var x=tp('a',h=>3);var r=x--;return [r,typeof x,x]})()", "(()=>{var x=tp('a',h=>'4');var r=++x;return [r]})()", "(()=>{var x=tp('a',h=>{});var r=x++;return [r]})()",
  "(()=>{var x=mk('a',{},{});return x++})()", "(()=>{var x=tp('a',h=>Symbol.iterator);return ++x})()",
  "(()=>{var o={get p(){L.push('get');return 1},set p(v){L.push('set:'+v)}};o.p++;o.p--;++o.p;--o.p;return o.p})()",
  "(()=>{var o={get p(){L.push('get');return 1n},set p(v){L.push('set:'+v)}};var a=o.p++;return a})()",
  "(()=>{var o={get p(){L.push('get');return mk('a',1)},set p(v){L.push('set:'+typeof v)}};var a=o.p++;return a})()",
  "(()=>{var o={get p(){return 1}};var r=o.p++;return [r,o.p]})()", "(()=>{'use strict';var o={get p(){return 1}};o.p++})()", "(()=>{'use strict';var o=Object.freeze({p:1});o.p++})()",
  "(()=>{'use strict';var o=Object.freeze({p:1});return o.p--})()", "(()=>{var o=Object.freeze({p:1});o.p--;return o.p})()", "(()=>{'use strict';undeclared_x++})()",
  "(()=>{undeclared_y++;return typeof undeclared_y})()", "(()=>{var a=[1,2];var i=0;a[i++]++;return [a,i]})()", "(()=>{var a=[1,2];var i=0;a[i++]+=10;return [a,i]})()",
  "(()=>{var a=[1,2];var i=0;a[(L.push('k'+i),i++)]--;return [a,i]})()", "(()=>{var o={};o[(L.push('key'),'x')]++;return o})()", "(()=>{var o={x:1};o[mk('k',0,'x')]++;return o})()",
  "(()=>{var o={x:1};o[tp('k',h=>'x')]++;return o})()", "(()=>{var o={x:1};o[mk('k',0,'x')]+=1;return o})()", "(()=>{var o={x:1};o[mk('k',0,'x')]??=5;return o})()",
  "(()=>{var x=1;var y=x+++x;return [x,y]})()", "(()=>{var x=1;var y=x---x;return [x,y]})()", "(()=>{var x=1;var y=x+ +x;return [x,y]})()", "(()=>{var x=1;var y=x - -x;return [x,y]})()",
  "(()=>{var x=2;x**=-1;return x})()", "(()=>{var x=9007199254740992;x++;return x})()", "(()=>{var x=9007199254740993n;x++;return x})()", "(()=>{var x=Number.MAX_VALUE;x++;return x==Number.MAX_VALUE})()",
  "(()=>{var x=2n**64n;x--;return x})()", "(()=>{var x=-(2n**63n);x--;return x})()", "(()=>{var x=0n;x--;x--;return x})()", "(()=>{var x='5';x+=1;x-=1;return [x,typeof x]})()",
  "(()=>{var x='5';x++;return [x,typeof x]})()", "(()=>{var x='5';x*=1n;return x})()", "(()=>{var x=1n;x+=1;return x})()", "(()=>{var x=1n;x**=-1n;return x})()", "(()=>{var x=1n;x/=0n;return x})()",
  "(()=>{var x=1n;x%=0n;return x})()", "(()=>{var x=1n;x>>>=1n;return x})()", "(()=>{var x=1n;x<<=1;return x})()", "(()=>{var x=1;x<<=1n;return x})()",
]) G(expr);

// ---- 6. Ordem de avaliação: a[b()] = c(), chamadas, operandos, argumentos, templates, desestruturação.
const ev = (...parts) => parts.join(";");
for (const e of [
  "(()=>{var a={};a[(L.push('b'),'k')]=(L.push('c'),1);return L.join()})()",
  "(()=>{var a=(L.push('a'),{});a[(L.push('b'),'k')]=(L.push('c'),1);return L.join()})()",
  "(()=>{(L.push('a'),null)[(L.push('b'),'k')]=(L.push('c'),1)})()", "(()=>{(L.push('a'),undefined)[(L.push('b'),'k')]=(L.push('c'),1)})()",
  "(()=>{(L.push('a'),null)[(L.push('b'),mk('k',0,'x'))]})()", "(()=>{var a=null;a[mk('k',0,'x')]=1})()", "(()=>{var a=null;a[mk('k',0,'x')]})()",
  "(()=>{var a=undefined;a.x=(L.push('rhs'),1)})()", "(()=>{var a=undefined;a[(L.push('key'),'x')]=(L.push('rhs'),1)})()", "(()=>{var a=undefined;a.x+=(L.push('rhs'),1)})()",
  "(()=>{var a=undefined;a[(L.push('key'),'x')]++})()", "(()=>{'use strict';var o={};Object.defineProperty(o,'x',{value:1});o[(L.push('k'),'x')]=(L.push('v'),2)})()",
  "(()=>{var o={set x(v){L.push('set:'+v)}};o[(L.push('k'),'x')]=(L.push('v'),2);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return 1},set x(v){L.push('set:'+v)}};o[(L.push('k'),'x')]+=(L.push('v'),2);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return 0},set x(v){L.push('set:'+v)}};o.x||=(L.push('v'),2);o.x&&=(L.push('w'),3);o.x??=(L.push('z'),4);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return 1},set x(v){L.push('set:'+v)}};o.x||=(L.push('v'),2);o.x&&=(L.push('w'),3);o.x??=(L.push('z'),4);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return null},set x(v){L.push('set:'+v)}};o.x??=(L.push('z'),4);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return undefined},set x(v){L.push('set:'+v)}};o.x??=(L.push('z'),4);return L.join()})()",
  "(()=>{var o={get x(){L.push('get');return 1}};o.x??=(L.push('z'),4);return L.join()})()", "(()=>{'use strict';var o={get x(){L.push('get');return 0}};o.x||=4})()",
  "(()=>{var f=(L.push('f'),function(){L.push('call')});f((L.push('a1'),1),(L.push('a2'),2));return L.join()})()",
  "(()=>{var f=(L.push('f'),null);f((L.push('a1'),1))})()", "(()=>{var o={};o.m((L.push('a1'),1))})()", "(()=>{var o={m:1};o.m((L.push('a1'),1))})()",
  "(()=>{(L.push('f'),undefined)((L.push('a'),1))})()", "(()=>{new (L.push('c'),1)((L.push('a'),1))})()", "(()=>{new (L.push('c'),function(){})((L.push('a'),1));return L.join()})()",
  "(()=>{var x=(L.push('l'),1)+(L.push('r'),2);return L.join()})()", "(()=>{var x=(L.push('l'),mk('l',1))+(L.push('r'),mk('r',2));return L.join()})()",
  "(()=>{var x=(L.push('l'),mk('l',1))-(L.push('r'),mk('r',2));return L.join()})()", "(()=>{var x=(L.push('l'),mk('l',1))**(L.push('r'),mk('r',2));return L.join()})()",
  "(()=>{var x=(L.push('l'),mk('l',1))*(L.push('r'),null);return L.join()})()", "(()=>{var x=(L.push('l'),Symbol.iterator)*(L.push('r'),mk('r',2));return L.join()})()",
  "(()=>{var x=(L.push('l'),mk('l',1n))*(L.push('r'),mk('r',2));return L.join()})()", "(()=>{var x=(L.push('l'),mk('l',1))<<(L.push('r'),mk('r',2));return L.join()})()",
  "(()=>{var x=(L.push('l'),mk('l',1))==(L.push('r'),mk('r',2));return L.join()})()", "(()=>{var x=(L.push('l'),mk('l',1))&&(L.push('r'),mk('r',2));return L.join()})()",
  "(()=>{var x=(L.push('l'),0)&&(L.push('r'),2);return L.join()})()", "(()=>{var x=(L.push('l'),1)||(L.push('r'),2);return L.join()})()", "(()=>{var x=(L.push('l'),null)??(L.push('r'),2);return L.join()})()",
  "(()=>{var x=(L.push('l'),0)??(L.push('r'),2);return L.join()})()", "(()=>{var x=(L.push('c'),1)?(L.push('t'),2):(L.push('f'),3);return L.join()})()",
  "(()=>{var x=[(L.push('1'),1),...(L.push('2'),[2,3]),(L.push('3'),4)];return L.join()+x})()", "(()=>{var x={[(L.push('k1'),'a')]:(L.push('v1'),1),[(L.push('k2'),'b')]:(L.push('v2'),2)};return L.join()})()",
  "(()=>{var x={a:(L.push('v1'),1),get b(){return 1},[(L.push('k2'),'c')]:(L.push('v2'),2)};return L.join()})()", "(()=>{var s=`${(L.push('1'),1)}${(L.push('2'),mk('t',2))}`;return L.join()})()",
  "(()=>{var [a=(L.push('d1'),1),b=(L.push('d2'),2)]=[undefined,0];return L.join()+a+b})()", "(()=>{var {a=(L.push('d1'),1),b=(L.push('d2'),2)}={a:null};return L.join()+a+b})()",
  "(()=>{var o={};[o[(L.push('k1'),'a')],o[(L.push('k2'),'b')]]=[(L.push('v1'),1),(L.push('v2'),2)];return L.join()})()",
  "(()=>{var o={};({a:o[(L.push('k1'),'x')]=(L.push('d'),9)}={});return L.join()+JSON.stringify(o)})()", "(()=>{var o={};[o.a,o.b]=(L.push('rhs'),[1,2]);return L.join()})()",
  "(()=>{var i=0;var a=[i++,i++,i++];return a})()", "(()=>{var i=0;var r=i+++i++ + ++i;return [r,i]})()", "(()=>{var i=0;var r=[i,i++,i,++i,i];return r})()",
  "(()=>{var i=1;i=i++ + i++;return i})()", "(()=>{var i=1;i+=i++;return i})()", "(()=>{var i=1;i+=(i=5);return i})()", "(()=>{var i=1;i=(i=5)+i;return i})()",
  "(()=>{var x=1;var y=(x=2)+x+(x=3)+x;return y})()", "(()=>{var a=1;var b=a+(a=2);return [a,b]})()", "(()=>{var a=[0,0];var i=0;a[i]=i=1;return [a,i]})()",
  "(()=>{var a=[0,0];var i=0;a[i]=++i;return [a,i]})()", "(()=>{var a=[0,0];var i=0;a[i++]=a[i++]=7;return [a,i]})()", "(()=>{var o={a:1};var p=o;o.a=(o={a:2});return [p.a,o.a]})()",
  "(()=>{var o={};var p=o;o.x=o={y:1};return [typeof p.x,p.x&&p.x.y,o.x]})()", "(()=>{var a,b,c;a=b=c=(L.push('v'),5);return [a,b,c]})()",
  "(()=>{var x=(L.push('a'),1),y=(L.push('b'),2);return L.join()})()", "(()=>{var r=(L.push('a'),L.push('b'),L.push('c'));return [r,L.join()]})()",
  "(()=>{function f(a=(L.push('d1'),1),b=(L.push('d2'),a+1)){return [a,b]};return [f(),f(5),f(undefined,0),L.join()]})()",
  "(()=>{function f(){return arguments.length};return [f(...[],...[1],...[2,3]),f(...'ab'),f(...new Set([1,1,2]))]})()",
  "(()=>{var o={get a(){L.push('ga');return 1},get b(){L.push('gb');return 2}};var c={...o};var {a,b}=o;return L.join()})()",
  "(()=>{var p=new Proxy({a:1,b:2},{get(t,k){L.push('get:'+String(k));return t[k]},ownKeys(t){L.push('keys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push('gopd:'+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});var c={...p};return L.join()})()",
  "(()=>{var p=new Proxy({a:1,b:2},{get(t,k){L.push('get:'+String(k));return t[k]}});var {b,a}=p;return L.join()})()",
  "(()=>{var p=new Proxy({a:1,b:2},{get(t,k){L.push('get:'+String(k));return t[k]}});var {x,...rest}=p;return L.join()})()",
  "(()=>{var it={[Symbol.iterator](){L.push('iter');var i=0;return {next(){L.push('next');return {done:i++>1,value:i}},return(){L.push('ret');return {}}}}};var [a]=it;return L.join()})()",
  "(()=>{var it={[Symbol.iterator](){L.push('iter');var i=0;return {next(){L.push('next');return {done:i++>1,value:i}},return(){L.push('ret');return {}}}}};var [a,b,c,d]=it;return L.join()})()",
  "(()=>{var it={[Symbol.iterator](){L.push('iter');var i=0;return {next(){L.push('next');return {done:i++>1,value:i}},return(){L.push('ret');return {}}}}};var [...a]=it;return L.join()+a})()",
]) E(e);

// ---- 7. Compound assignment com getters/setters e Proxy.
const ops = ["+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=", "??="];
for (const op of ops) {
  for (const [init, rhs] of [["5", "3"], ["'5'", "3"], ["0", "3"], ["null", "3"], ["2n", "3n"], ["2n", "3"]]) {
    E(`(()=>{var o={get x(){L.push('get');return ${init}},set x(v){L.push('set:'+S(v))}};var r=(o.x ${op} ${rhs});return [S(r),L.join()]})()`);
  }
  E(`(()=>{var L2=[];var p=new Proxy({x:6},{get(t,k){L2.push('get:'+String(k));return t[k]},set(t,k,v){L2.push('set:'+String(k)+'='+v);t[k]=v;return true},has(t,k){L2.push('has:'+String(k));return k in t}});var r=(p.x ${op} 2);return [r,L2.join()]})()`);
}
for (const e of [
  "(()=>{var L2=[];var p=new Proxy({x:1},{get(t,k,r){L2.push('get:'+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){L2.push('set:'+String(k));return Reflect.set(t,k,v,r)},defineProperty(t,k,d){L2.push('def:'+String(k));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){L2.push('gopd:'+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});p.x+=1;p.y=2;return L2.join()})()",
  "(()=>{'use strict';var p=new Proxy({},{set(){return false}});p.x+=1})()", "(()=>{var p=new Proxy({},{set(){return false}});return p.x+=1})()",
  "(()=>{'use strict';var p=new Proxy({},{set(){return false}});p.x=1})()", "(()=>{var p=new Proxy({},{set(){return false}});return p.x=1})()",
  "(()=>{var p=new Proxy({},{set(){throw new TypeError('s')}});p.x=1})()", "(()=>{var p=new Proxy({},{get(){throw new TypeError('g')}});p.x+=1})()",
  "(()=>{var q={set x(v){L.push('qs:'+v)}};var o=Object.create(q);o.x=5;return [L.join(),Object.keys(o)]})()", "(()=>{var q={get x(){return 1}};var o=Object.create(q);o.x=5;return [o.x,Object.keys(o)]})()",
  "(()=>{'use strict';var q={get x(){return 1}};var o=Object.create(q);o.x=5})()", "(()=>{var q=Object.freeze({x:1});var o=Object.create(q);o.x=5;return [o.x,Object.keys(o)]})()",
  "(()=>{'use strict';var q=Object.freeze({x:1});var o=Object.create(q);o.x=5})()", "(()=>{var o={x:1};Object.defineProperty(o,'x',{writable:false});o.x+=1;return o.x})()",
  "(()=>{'use strict';var o={x:1};Object.defineProperty(o,'x',{writable:false});o.x+=1})()", "(()=>{'use strict';var s='abc';s.x=1})()", "(()=>{'use strict';var s='abc';s[0]='z'})()",
  "(()=>{var s='abc';s[0]='z';s.x=1;return [s,s.x]})()", "(()=>{'use strict';var n=1;n.x=1})()", "(()=>{'use strict';var s=Symbol();s.x=1})()", "(()=>{'use strict';var b=1n;b.x=1})()",
  "(()=>{var n=1;n.x=1;return n.x})()", "(()=>{'use strict';null.x=1})()", "(()=>{'use strict';(void 0).x+=1})()",
  "(()=>{var o={x:1};with(o){x+=1;y=2};return [o.x,typeof y]})()", "(()=>{var o=new Proxy({x:1},{has(t,k){L.push('has:'+String(k));return k in t},get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k));t[k]=v;return true}});with(o){x+=1};return L.join()})()",
  "(()=>{var o={x:1,[Symbol.unscopables]:{x:true}};var x=10;with(o){x+=1};return [o.x,x]})()",
  "(()=>{var x=1;x+=(x=10);return x})()", "(()=>{var x={valueOf(){L.push('v');return 1}};x+=1;return [x,L.join()]})()", "(()=>{var x={valueOf(){L.push('v');return 1}};x+=x;return [x,L.join()]})()",
  "(()=>{var a=[1,2,3];a.length-=1;return a})()", "(()=>{var a=[1,2,3];a.length+=2;return [a.length,0 in a,3 in a]})()", "(()=>{var a=[];a[2**32-2]=1;a.length+=1;return a.length})()",
  "(()=>{var a=[];a[2**32-1]=1;return [a.length,Object.keys(a)]})()", "(()=>{'use strict';var a=[];a.length=2**32})()", "(()=>{var a=[1];a.length='1';return a.length})()", "(()=>{var a=[1];a.length=mk('l',1);return [a.length,L.join()]})()",
  "(()=>{var o={a:1};o.a**=-1;o.b=undefined;o.b**=2;o.c=null;o.c**=2;return o})()", "(()=>{var x=2n;x**=100n;return x})()", "(()=>{var x=2n;x**=0n;return x})()", "(()=>{var x=0n;x**=0n;return x})()",
]) E(e);

// ---- 8. Vírgula, exponenciação, shifts, bitwise, módulo, divisão.
for (const e of [
  "[(1,2),(1,2,3),((1,2),3),(L.push('a'),L.push('b')),(0,'x'),(null,undefined)]", "(()=>{var f=function(){return this===undefined||this===globalThis?'g':'o'};var o={f};return [(o.f)(),(0,o.f)(),(o.f=o.f)(),(o.f,o.f)()]})()",
  "(()=>{var o={f(){return this===o}};return [o.f(),(o.f)(),(0,o.f)(),(o.f||0)(),(o?.f)(),o?.f()]})()", "(()=>{'use strict';var o={f(){return this===o}};return [o.f(),(0,o.f)()]})()",
  "[2**3,2**-1,(-2)**2,(-2)**3,(-2)**-1,(-2)**0.5,2**0.5,0**0,0**-1,(-0)**-1,(-0)**-2,(-0)**3,(-0)**-3,Infinity**0,NaN**0,1**Infinity,(-1)**Infinity,1**NaN,(-1)**-Infinity,2**Infinity,2**-Infinity,0.5**Infinity,0.5**-Infinity]",
  "[2**3**2,(2**3)**2,2**-3**-1,(-8)**(1/3),10**21,10**-7,10**308,10**309,(-10)**309,2**1023*2,2**-1074,2**-1075,2**53+1,(2**53+1)%2]",
  "[1n**0n,0n**0n,2n**10n,(-2n)**3n,(-2n)**2n,0n**5n,(-1n)**1000001n,(-1n)**1000000n,1n**(2n**100n),1n**-1n,(-1n)**-1n]",
  "T(()=>2n**-1n)", "T(()=>0n**-1n)", "T(()=>2n**(2n**64n))", "T(()=>2n**2)", "T(()=>2**2n)", "T(()=>(2n**100000n).toString().length)", "T(()=>(-(2n**64n))**3n)",
  "T(()=>{var x=-2;return x**2})", "T(()=>(-2)**2)", "T(()=>(+2)**2)", "T(()=>(typeof 2)**2)", "T(()=>(await_x=2,await_x**2))", "T(()=>[2**2**0,2**(2**0),(2**2)**0])",
  "[1<<31,1<<32,1<<33,1<<-1,1<<-32,-1>>>0,-1>>>1,-1>>>32,-1>>>33,-1>>1,-1>>31,-1>>32,2**32>>0,2**32+5>>0,2**31>>0,-(2**31)>>0,2**53>>0,1.9>>0,-1.9>>0,NaN>>0,Infinity>>0,-Infinity>>>0]",
  "[1<<'2',1<<'x','8'>>'1',null<<1,undefined<<1,true<<3,[]<<1,[3]<<[1],({})<<1,1<<1.9,1<<-0.9]",
  "[1n<<1n,1n<<64n,1n<<0n,-1n>>1n,-1n>>100n,1n>>1n,1n<<-1n,1n>>-1n,-5n>>1n,5n>>1n,-5n<<1n,(1n<<100n)>>99n,-(1n<<100n)>>101n,(1n<<64n)-1n>>63n,0n<<1000n]",
  "T(()=>1n>>>1n)", "T(()=>1n>>>1)", "T(()=>1>>>1n)", "T(()=>1n<<1)", "T(()=>1<<1n)", "T(()=>1n<<(2n**64n))", "T(()=>1n>>(2n**64n))", "T(()=>-1n>>(2n**64n))", "T(()=>0n<<(2n**64n))", "T(()=>1n<<-(2n**64n))",
"T(()=>(1n<<10n).toString(2).length)", "T(()=>BigInt.asUintN(8,-1n))", "T(()=>BigInt.asIntN(8,255n))", "T(()=>BigInt.asUintN(64,-1n))", "T(()=>BigInt.asIntN(64,2n**63n))",
  "[~NaN,~Infinity,~-Infinity,~undefined,~null,~true,~'x',~'5',~[],~[5],~{},~0,~-0,~-1,~2**31,~2**32,~(2**32+1),~1.9,~-1.9,~0.5,~Number.MAX_VALUE,~Number.MIN_VALUE]",
  "[~0n,~1n,~-1n,~(2n**64n),~-(2n**64n),~~5n,~~~5n,-(~1n),~Object(1n)]", "T(()=>~mk('a',1n))", "T(()=>~mk('a',5))", "T(()=>~Symbol.iterator)", "T(()=>~{valueOf(){return 3n}})",
  "[5&3,5|3,5^3,~5&3,-1&0xff,-1|0,2**32&1,2**32|1,2**32+1&3,1.9&3.9,NaN&1,NaN|1,Infinity|0,-Infinity|0,'5'&'3','a'|0,null|1,undefined^1,true&1,[]|1,[7]&3,({})|0]",
  "[5n&3n,5n|3n,5n^3n,-5n&3n,-5n|3n,-5n^3n,-1n&0xffn,(1n<<64n)&((1n<<64n)-1n),-(1n<<64n)|1n,-(1n<<64n)^-1n,0n&-1n,0n|-1n,0n^-1n,-1n&-1n,-1n|-1n,-1n^-1n]",
  "T(()=>5n&3)", "T(()=>5&3n)", "T(()=>5n|'3')", "T(()=>5n^null)", "T(()=>5n&true)", "T(()=>5n&mk('a',3n))", "T(()=>mk('a',5n)&mk('b',3))",
  "[5%3,-5%3,5%-3,-5%-3,5.5%2,-5.5%2,5%0,0%5,-0%5,0%-5,Infinity%5,5%Infinity,-5%Infinity,5%-Infinity,NaN%1,1%NaN,2**53%3,1e21%7,0.3%0.1,1%0.1,-1%0.1,'7'%'4',null%3,undefined%3,3%null]",
  "[5n%3n,-5n%3n,5n%-3n,-5n%-3n,0n%5n,-0n%5n,(2n**64n)%7n,-(2n**64n)%7n,(2n**64n)%(2n**32n),(2n**64n+1n)%(2n**32n)]", "T(()=>5n%0n)", "T(()=>5n/0n)", "T(()=>0n/0n)", "T(()=>5n%0)", "T(()=>5n/0)",
  "[5n/3n,-5n/3n,5n/-3n,-5n/-3n,0n/5n,(2n**64n)/3n,-(2n**64n)/3n,(2n**64n)/(2n**64n),1n/(2n**64n),-1n/(2n**64n)]",
  "[5/0,-5/0,0/0,-0/5,0/-5,-0/-5,Infinity/Infinity,1/Infinity,-1/Infinity,5/'0',5/null,5/undefined,'6'/'3','a'/1,[]/1,[6]/[3],true/true,1/3,2/3,1e308*10,-1e308*10,5e-324/2,5e-324*0.5,1e-323/2]",
  "[0.1+0.2,0.1*3,1.1*1.1,9007199254740991+2,2**53+2,-(2**53)-1,1e21+1,1e-7*1,123456789012345680000+0,0.000001*1,1/3*3,4.35*100,1.005*1000]",
  "[+'',+' ',+'\\n',+'0b11',+'0o17',+'0xg',+'1_0',+'1e',+'.5',+'5.',+'.',+'+.5e1',+'-0',+'- 0',+'Infinity',+'-Infinity',+'infinity',+'INFINITY',+'1e1000',+'0x',+'0X1f',+'١']",
  "[+[],+[[]],+[5],+[1,2],+{},+null,+undefined,+true,+false,+new Date(5),+'  12  ',+'12px',+new Number(3),+new String('4'),+new Boolean(true),+function(){},+/x/]",
  "[-'',-'x',-null,-undefined,-true,-[],-[3],-{},- -1,-(-0),-0,-(0),- +1,+-1,-Infinity,-NaN]", "[1/-0,1/-(0),1/-'',1/-null,1/-false,1/(0*-1),1/(-0+0),1/(-0-0),1/(0-0),1/(-0*5),1/Math.round(-0.4),1/Math.min(0,-0),1/(-0|0),1/(+-0),1/(-0).valueOf()]",
]) {
  if (/^T\(/.test(e)) E(e.slice(2, -1).replace(/^\(\)=>/, "")); else E(e);
}

// ---- 9. Object.is, SameValueZero e chave de propriedade.
const isValues = ["0", "-0", "NaN", "1", "'1'", "1n", "undefined", "null", "''", "'a'", "Symbol.iterator", "[]"];
for (const a of isValues) for (const b of isValues) E(`[Object.is(${a},${b}),${a}===${b},[${a}].includes(${b}),[${a}].indexOf(${b}),new Set([${a}]).has(${b}),new Map([[${a},1]]).has(${b})]`);
for (const e of [
  "(()=>{var m=new Map([[-0,'z'],[NaN,'n']]);return [Object.is([...m.keys()][0],0),m.get(0),m.get(-0),m.get(NaN),Object.is([...new Set([-0]).values()][0],0)]})()",
  "(()=>{var s=new Set([0,-0,NaN,NaN,'0',0n,null,undefined,void 0]);return [s.size,[...s].map(S)]})()", "(()=>{var m=new Map();m.set(-0,1).set(0,2);return [m.size,m.get(-0)]})()",
  "[[NaN].includes(NaN),[NaN].indexOf(NaN),[NaN].lastIndexOf(NaN),[NaN].findIndex(x=>x!==x),[-0].includes(0),[0].includes(-0),[-0].indexOf(0),[0].lastIndexOf(-0)]",
  "[[1,,3].includes(undefined),[1,,3].indexOf(undefined),[,].includes(undefined),[,].indexOf(undefined),[undefined].indexOf(undefined),[1,2,3].includes(3,-1),[1,2,3].includes(1,-1),[1,2,3].includes(1,-5)]",
  "[[1n].includes(1),[1n].indexOf(1),[1n].includes(1n),[2n**64n].includes(2n**64n),[2n**64n].indexOf(2n**64n),new Set([2n**64n]).has(2n**64n),new Map([[2n**64n,1]]).get(2n**64n),[1].includes('1')]",
  "[Object.is(new String('a'),new String('a')),Object.is(Symbol.for('q'),Symbol.for('q')),Object.is(Symbol('q'),Symbol('q')),Object.is(0,+0),Object.is(-0,0-0),Object.is(-0,-0),Object.is(),Object.is(1),Object.is(undefined)]",
  "[Object.is(0.1+0.2,0.3),Object.is(1/3,1/3),Object.is(2**53,2**53+1),Object.is(1e21,1e21),Object.is(5e-324,2**-1074),Object.is(NaN,0/0),Object.is(NaN,Number('x')),Object.is(Infinity,1/0),Object.is(-Infinity,-1/0)]",
  "[Object.is.length,Object.is.name,Object.is(mk('a',1),1),Object.is({},{}),Object.is(globalThis,this),Object.is(1,Object(1))]",
  "(()=>{var o={};o[-0]=1;o[0]=2;o[1.0]=3;o['1']=4;o[1e21]=5;o[2**53]=6;o[1/3]=7;o[-1]=8;o[NaN]=9;o[Infinity]=10;o[-Infinity]=11;o[0.1+0.2]=12;o[1e-7]=13;o[2**32]=14;o[2**32-1]=15;o[2**32-2]=16;return Object.keys(o)})()",
  "(()=>{var o={};o[1e21]=1;o[123456789012345678901]=2;o[0.000001]=3;o[1e-7]=4;o[-1e-7]=5;o[100]=6;o[1e2]=7;o[4294967294]=8;o[4294967295]=9;o[4294967296]=10;return Object.keys(o)})()",
  "(()=>{var o={};o[1n]=1;o[2n**64n]=2;o[-1n]=3;o[0n]=4;o[(-0n)]=5;return [Object.keys(o),o[1],o['1'],o[2n**64n]]})()",
  "(()=>{var o={};o[null]=1;o[undefined]=2;o[true]=3;o[false]=4;o[[]]=5;o[[1,2]]=6;o[{}]=7;o[function(){}.name]=8;return Object.keys(o)})()",
  "(()=>{var s1=Symbol('d'),s2=Symbol('d');var o={[s1]:1,[s2]:2,d:3};return [o[s1],o[s2],o.d,Object.keys(o),Object.getOwnPropertySymbols(o).length,JSON.stringify(o)]})()",
  "(()=>{var o={};o[Symbol.iterator]=1;o[Symbol.for('k')]=2;return [Reflect.ownKeys(o).map(S),o[Symbol.iterator],'Symbol(Symbol.iterator)' in o]})()",
  "(()=>{var o={a:1};return [o['a'],o[mk('k',0,'a')],o[tp('k',h=>'a')],o[{toString(){return 'a'}}],o[[ 'a' ]],o[{valueOf(){return 'b'},toString(){return 'a'}}]]})()",
  "(()=>{var o={1:'one',a:'A'};return [o[1],o['1'],o[1.0],o[mk('k',1,'a')],o[tp('k',h=>1)],o[tp('k',h=>h)]]})()",
  "(()=>{var o={};try{o[tp('k',h=>({}))]=1}catch(e){return e.name+':'+e.message}})()", "(()=>{var o={};o[tp('k',h=>{throw new SyntaxError('kk')})]=1})()",
  "(()=>{var o={};o[mk('k',{},{})]=1})()", "(()=>{var o={};o[Object.create(null)]=1})()", "(()=>{var o={};o[(L.push('key'),Symbol.iterator)]=(L.push('val'),1);return L.join()})()",
  "(()=>{var o={};o[mk('k',0,'x')]=(L.push('val'),1);return L.join()})()", "(()=>{var o={};o[mk('k',0,'x')]+=(L.push('val'),1);return [L.join(),o]})()", "(()=>{var o={};o[mk('k',0,'x')]++;return [L.join(),o]})()",
  "(()=>{var o={x:1};var k=mk('k',0,'x');o[k]||=(L.push('val'),5);return L.join()})()", "(()=>{var o={x:0};var k=mk('k',0,'x');o[k]||=(L.push('val'),5);return L.join()})()", "(()=>{var o={x:null};var k=mk('k',0,'x');o[k]??=(L.push('val'),5);return L.join()})()",
  "(()=>{var o={x:1};delete o[mk('k',0,'x')];return [L.join(),o]})()", "(()=>{var o={x:1};return [mk('k',0,'x') in o,L.join()]})()", "(()=>{var o={x:1};return [Object.hasOwn(o,mk('k',0,'x')),L.join()]})()",
  "(()=>{var o={x:1};return [o.hasOwnProperty(mk('k',0,'x')),L.join()]})()", "(()=>{var o={x:1};return [Reflect.has(o,mk('k',0,'x'))]})()", "(()=>{var o={x:1};return [Object.getOwnPropertyDescriptor(o,mk('k',0,'x')).value,L.join()]})()",
  "(()=>{var a=['a','b','c'];return [a[-0],a[+0],a['-0'],a[0.0],a['0.0'],a[1e0],a[2**32],a[-1],a['01'],a[' 1'],a[1.5],a[true],a[null],a[[1]],a['1.0']]})()",
  "(()=>{var a=[];a[-0]=1;a['-0']=2;a[1.5]=3;a[2**32]=4;a[-1]=5;a['01']=6;return [a.length,Object.keys(a)]})()", "(()=>{var s='abc';return [s[-0],s[1.0],s['1'],s[3],s[-1],s['01'],s[1e0],s[mk('k',1,'2')]]})()",
  "(()=>{var m=new Map([[1,'n'],['1','s'],[1n,'b'],[true,'t'],[null,'N'],[undefined,'u']]);return [m.get(1),m.get('1'),m.get(1n),m.get(true),m.get(null),m.get(undefined),m.get(),m.get(Object(1)),m.size]})()",
  "(()=>{var wm=new WeakMap();var k={};wm.set(k,1);return [wm.get(k),wm.has({}),T(()=>wm.set(1,1)),T(()=>wm.set(Symbol.iterator,1)),T(()=>wm.set(Symbol.for('r'),1)),T(()=>wm.set(Symbol('ok'),1))]})()",
  "(()=>{var ws=new WeakSet();return [T(()=>ws.add(1)),T(()=>ws.add(null)),T(()=>ws.has(1)),T(()=>ws.add({}) instanceof WeakSet),T(()=>ws.add(Symbol('x')) instanceof WeakSet)]})()",
  "(()=>{var s=new Set([1,2,3]);var r=[];for(var x of s){r.push(x);if(x===1){s.delete(2);s.add(4)}}return r})()", "(()=>{var m=new Map([[1,1]]);var r=[];for(var [k] of m){r.push(k);if(k<4)m.set(k+1,0)}return r})()",
  "(()=>{var s=new Set();s.add(NaN).add(NaN).add(-0).add(0).add(+0);return [s.size,Object.is([...s][1],0)]})()", "(()=>{var m=new Map();m.set(NaN,1);return [m.get(NaN),m.has(NaN),m.delete(NaN),m.size]})()",
  "[Array.from(new Set([3,1,3,2,1])),Array.from(new Map([[1,2],[1,3]])),Object.fromEntries(new Map([[1,2],['1',3]])),Object.fromEntries([[-0,1],[0,2]])]",
  "[Object.groupBy([-0,0,1],x=>x),Map.groupBy([-0,0,1],x=>x).size,[...Map.groupBy([-0,0],x=>x).keys()].map(S)]",
  "[[0,-0].indexOf(-0),[0,-0].includes(-0),[-0].at(0)===0,Object.is([-0].at(0),-0),Object.is([-0].concat()[0],-0),Object.is(Array.of(-0)[0],-0),Object.is(Math.max(-0,0),0),Object.is(Math.min(0,-0),-0),Object.is([0,-0].sort()[0],0)]",
  "[Object.is(JSON.parse('-0'),-0),JSON.stringify(-0),JSON.stringify([-0]),String(-0),(-0).toString(),`${-0}`,-0+'',(-0).toFixed(1),(-0).toLocaleString(),Object.is(parseFloat('-0'),-0),Object.is(Number('-0'),-0),Object.is(parseInt('-0'),-0),Object.is(Math.round(-0.2),-0)]",
]) E(e);

// ---- 10. ToNumber/ToString/ToPrimitive com erros e hints.
for (const e of [
  "[Number(''),Number(' '),Number('0x'),Number('0b2'),Number('1__0'),Number(null),Number(undefined),Number(true),Number([]),Number([1]),Number([1,2]),Number({}),Number(new Date(8)),Number(()=>1),Number('Infinity'),Number('+Infinity'),Number('infinity'),Number('1,5')]",
  "[Number(),Number(1n),Number(2n**64n),Number(2n**1024n),Number(-(2n**1024n)),Number(2n**53n+1n),Number(2n**53n+2n),Number(-(2n**53n)-1n),Number(0n),Number(Object(5n)),Number(9007199254740993n),Number(2n**1023n*3n)]",
  "[String(),String(undefined),String(null),String(1n),String(-0),String(0.1+0.2),String(1e21),String(1e-7),String(123456789012345680000),String(-1e-7),String(2**-1074),String([null]),String([undefined,1]),String({}),String(()=>1),String(Symbol('d')),String(Object(Symbol('o')))]",
  "[`${1n}`,`${-0}`,`${[]}`,`${[[]]}`,`${[1,[2,3]]}`,`${null}`,`${undefined}`,`${true}`,`${{}}`,`${()=>1}`,`${new Date(NaN)}`,`${/x/g}`,`${new Error('m')}`,`${new RangeError}`]",
  "T(()=>`${Symbol()}`)", "T(()=>Symbol()+'')", "T(()=>[Symbol()]+'')", "T(()=>String([Symbol()]))", "T(()=>Number([Symbol()]))", "T(()=>[Symbol()].join())", "T(()=>({}[Symbol()]=1,'ok'))",
  "T(()=>new Number(Symbol.iterator))", "T(()=>new String(Symbol.iterator))", "T(()=>Object(Symbol.iterator)+1)", "T(()=>Object(Symbol.iterator)+'')", "T(()=>`${Object(Symbol.iterator)}`)", "T(()=>Object(Symbol.iterator)==Symbol.iterator)",
  "T(()=>Symbol.iterator.description)", "T(()=>Symbol().description)", "T(()=>Symbol('').description)", "T(()=>Symbol(undefined).description)", "T(()=>Symbol(null).description)", "T(()=>Symbol({toString(){return 'ts'}}).description)", "T(()=>Symbol(Symbol()))",
  "T(()=>new Symbol)", "T(()=>Symbol(1n).toString())", "T(()=>Symbol.keyFor(Symbol.iterator))", "T(()=>Symbol.keyFor(Symbol.for('kk')))", "T(()=>Symbol.keyFor('x'))", "T(()=>Symbol.for().toString())",
  "T(()=>Symbol.prototype[Symbol.toPrimitive].call(Symbol.iterator,'number')===Symbol.iterator)", "T(()=>Symbol.prototype.valueOf.call(1))", "T(()=>Symbol.prototype.toString.call({}))",
  "T(()=>BigInt.prototype.valueOf.call(1))", "T(()=>BigInt.prototype.toString.call('1'))", "T(()=>BigInt.prototype.toString.call(255n,16))", "T(()=>BigInt.prototype.toString.call(255n,1))", "T(()=>BigInt.prototype.toString.call(255n,37))",
  "T(()=>(255n).toString(36))", "T(()=>(-255n).toString(2))", "T(()=>(0n).toString(2))", "T(()=>(2n**64n).toString(16))", "T(()=>BigInt.prototype.toLocaleString.call(1234567n))", "T(()=>new BigInt(1))", "T(()=>BigInt.length+BigInt.name)",
  "T(()=>Number.prototype.valueOf.call('1'))", "T(()=>Number.prototype.toString.call({}))", "T(()=>String.prototype.valueOf.call(1))", "T(()=>Boolean.prototype.valueOf.call(1))", "T(()=>Boolean.prototype.toString.call('true'))",
  "T(()=>(1).toString(1))", "T(()=>(1).toString(37))", "T(()=>(1).toString(undefined))", "T(()=>(1).toString(null))", "T(()=>(255).toString('16'))", "T(()=>(255).toString(16.9))", "T(()=>(0.5).toString(2))", "T(()=>(-255.5).toString(16))",
  "T(()=>(NaN).toString(2))", "T(()=>(Infinity).toString(2))", "T(()=>(-0).toString(2))", "T(()=>(1e21).toString(7).length>0)", "T(()=>(0.1).toString(3).slice(0,12))",
  "T(()=>(1).toFixed(101))", "T(()=>(1).toFixed(-1))", "T(()=>(1).toFixed(100).length)", "T(()=>(1e21).toFixed(2))", "T(()=>(1.005).toFixed(2))", "T(()=>(0.5).toFixed(0))", "T(()=>(1.5).toFixed(0))", "T(()=>(2.5).toFixed(0))", "T(()=>(-1.5).toFixed(0))",
  "T(()=>(123.456).toPrecision(0))", "T(()=>(123.456).toPrecision(101))", "T(()=>(123.456).toPrecision(2))", "T(()=>(0.000123).toPrecision(2))", "T(()=>(1e21).toPrecision(3))", "T(()=>(123.456).toExponential(-1))", "T(()=>(123.456).toExponential(101))", "T(()=>(0).toExponential())", "T(()=>(123456).toExponential(2))",
  "T(()=>parseInt('0x1f'))", "T(()=>parseInt('1f',16))", "T(()=>parseInt('z',36))", "T(()=>parseInt('1',1))", "T(()=>parseInt('1',37))", "T(()=>parseInt('1',0))", "T(()=>parseInt('  -12abc'))", "T(()=>parseInt(1e21))", "T(()=>parseInt(0.0000005))", "T(()=>parseInt(null,36))", "T(()=>parseInt('9007199254740993'))", "T(()=>parseInt(1n))", "T(()=>parseInt(Symbol.iterator))",
  "T(()=>parseFloat('1e'))", "T(()=>parseFloat('.5.5'))", "T(()=>parseFloat('-.5e-2x'))", "T(()=>parseFloat('Infinityx'))", "T(()=>parseFloat('0x10'))", "T(()=>parseFloat('1_0'))", "T(()=>parseFloat(''))", "T(()=>parseFloat('  \\n 7'))", "T(()=>parseFloat({toString(){return '3.5z'}}))",
  "T(()=>Math.max('3',{valueOf(){return 4}},[5]))", "T(()=>Math.max(1n))", "T(()=>Math.max(Symbol.iterator))", "T(()=>Math.max(1,{valueOf(){throw new RangeError('mv')}}))", "T(()=>Math.max(NaN,{valueOf(){throw new RangeError('after nan')}}))",
  "T(()=>Math.pow(2,{valueOf(){throw new RangeError('p')}}))", "T(()=>Math.abs(1n))", "T(()=>Math.floor('1.5'))", "T(()=>Math.sign(-0))", "T(()=>Math.hypot(1,{valueOf(){throw new RangeError('h')}},NaN))", "T(()=>Math.hypot(Infinity,NaN))",
  "T(()=>isNaN(1n))", "T(()=>isNaN(Symbol.iterator))", "T(()=>isNaN('x'))", "T(()=>isNaN(undefined))", "T(()=>Number.isNaN('x'))", "T(()=>isFinite(null))", "T(()=>Number.isFinite(null))", "T(()=>Number.isInteger(5.0))", "T(()=>Number.isSafeInteger(2**53))",
  "T(()=>Boolean(0n))", "T(()=>!!-0n)", "T(()=>!!1n)", "T(()=>!!'')", "T(()=>!!' ')", "T(()=>!!'0')", "T(()=>!!NaN)", "T(()=>!![])", "T(()=>!!{})", "T(()=>!!Symbol())", "T(()=>!!new Boolean(false))", "T(()=>!!document_all_missing)", "T(()=>!!-0)",
  "T(()=>[0n?1:2,1n?1:2,''?1:2,'0'?1:2,NaN?1:2,null?1:2,undefined?1:2,[]?1:2,0.0?1:2,-0?1:2,0n||'z',0n??'z',null??'z',''??'z',NaN??'z',false??'z'])",
  "T(()=>[1&&2,0&&2,''||'a',null||undefined,undefined||null,NaN||0,0||NaN,1&&null,1&&undefined,(0||null)??'d',(null??0)||'e'])", "T(()=>[true+1,true-1,true*'3',false/1,true<<1,+true,-true,~true,!true,true==1,true==2,true=='1',true=='true'])",
  "T(()=>[{}.toString.call(null),{}.toString.call(undefined),{}.toString.call(1),{}.toString.call(1n),{}.toString.call(Symbol()),{}.toString.call(''),{}.toString.call(true),{}.toString.call([]),{}.toString.call(()=>1),{}.toString.call(new Date),{}.toString.call(/x/),{}.toString.call(new Error),{}.toString.call(arguments_missing_ok=1)])",
  "T(()=>{var o={[Symbol.toStringTag]:'Custom'};return [String(o),o+'',`${o}`,Object.prototype.toString.call(o),Object.prototype.toString.call({[Symbol.toStringTag]:1}),Object.prototype.toString.call(Object.assign([],{[Symbol.toStringTag]:'X'}))]})",
  "T(()=>[Object.prototype.toString.call(function*(){}),Object.prototype.toString.call(async()=>1),Object.prototype.toString.call(new Map),Object.prototype.toString.call(Promise.resolve()),Object.prototype.toString.call(Symbol.prototype),Object.prototype.toString.call(BigInt.prototype),Object.prototype.toString.call(Math),Object.prototype.toString.call(JSON),Object.prototype.toString.call(globalThis)])",
  "T(()=>[Object.prototype.toLocaleString.call(1),Object.prototype.toLocaleString.call(null)])", "T(()=>Object.prototype.valueOf.call(null))", "T(()=>Object.prototype.valueOf.call(1)===1)", "T(()=>typeof Object.prototype.valueOf.call(1))",
]) {
  if (/^T\(/.test(e)) E(e.slice(2, -1).replace(/^\(\)=>/, "")); else E(e);
}
// ToPrimitive: hints, resultados inválidos e hooks que lançam, em contextos diferentes.
const hintCtx = [
  ["add", "o=>o+''"], ["add1", "o=>o+1"], ["mul", "o=>o*1"], ["tpl", "o=>`${o}`"], ["str", "o=>String(o)"], ["num", "o=>Number(o)"], ["key", "o=>({})[o]"], ["lt", "o=>o<1"],
  ["eq", "o=>o==1"], ["neg", "o=>-o"], ["bnot", "o=>~o"], ["inc", "o=>{o++;return o}"], ["join", "o=>[o].join()"], ["date", "o=>new Date(o).getTime()"], ["bigint", "o=>BigInt(o)"],
  ["parse", "o=>parseInt(o)"], ["isnan", "o=>isNaN(o)"], ["cmp", "o=>[o>0,o<=0]"], ["sym", "o=>Symbol(o).toString()"], ["tofixed", "o=>(1).toFixed(o)"], ["arrlen", "o=>new Array(o).length"],
  ["idx", "o=>[1,2,3][o]"], ["slice", "o=>'abcdef'.slice(o)"], ["repeat", "o=>'a'.repeat(o)"], ["json", "o=>JSON.stringify({a:1},null,o)"], ["math", "o=>Math.abs(o)"], ["pad", "o=>'x'.padStart(3,o)"],
];
const hintObjs = [
  ["tpHint", "tp('o',h=>h==='number'?7:h==='string'?'s':'d')"], ["tpNum", "tp('o',h=>5)"], ["tpStr", "tp('o',h=>'5')"], ["tpBig", "tp('o',h=>5n)"], ["tpObj", "tp('o',h=>({}))"],
  ["tpSym", "tp('o',h=>Symbol.iterator)"], ["tpThrow", "tp('o',h=>{throw new RangeError('tpt')})"], ["vs", "mk('o',3,'4')"], ["vsObj", "mk('o',{},'6')"], ["svObj", "mk('o','7',{})"], ["none", "Object.create(null)"],
];
for (const [cn, ce] of hintCtx) {
  const fn = `(${ce})`;
  for (const [on, oe] of hintObjs) G(`${fn}(${oe})`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "operator-edge-golden-"));
// O bun passa arquivos pelo transpilador próprio; `vm.runInThisContext` roda como ProgramExecutable do JSC puro.
// SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "operator_source.js");
const file = path.join(dir, "operator_case.js");
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
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  // Modo sloppy por padrão (with, delete, atribuição a congelado); os programas que precisam de strict declaram 'use strict'.
  const source = PRELUDE + body;
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
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
