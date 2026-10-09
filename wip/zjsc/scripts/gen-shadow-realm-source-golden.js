// Gera tests/golden/shadow_realm_source_bun.tsv: o que sobra de `ShadowRealm` depois de shadow_realm_bun e
// shadow_realm_more_bun, medido no bun 1.4.2. Como o porte não tem `vm`, o ShadowRealm é a única forma de analisar e
// executar fonte num segundo realm sem APIs de host, então esta matriz usa o realm remoto para exercitar o parser e
// os construtores de função em fonte exótica: comentários HTML-like, octais legados, `let`/`yield`/`await` como
// identificador, ASI, regex contra divisão, labels, `Function`/`GeneratorFunction`/`AsyncFunction`/
// `AsyncGeneratorFunction` com parâmetros e corpos exóticos (defaults, destructuring, rest, 'use strict' com
// parâmetros não simples, nomes duplicados, `arguments`/`eval` em strict, U+2028/U+2029, `anonymous` no toString),
// intrínsecos de cada global vistos pelo wrapper, e a forma de funções nativas do realm remoto.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-shadow-realm-more-golden.js.
// Programas cuja expressão já aparece nos goldens shadow_realm, shadow_realm_more, dynamic_fn ou sloppy_syntax caem fora.
// Uso: bun scripts/gen-shadow-realm-source-golden.js > tests/golden/shadow_realm_source_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored } = require("./golden-prelude.js");
const rows = [];
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.constructor&&e.constructor.name)+": "+(e&&e.message)}}\n' +
  "var sr = new ShadowRealm();\n";

const programs = [];
const q = s => JSON.stringify(s);
const expr = e => programs.push(PRELUDE + `globalThis.R = T(() => ${e});\n`);
// Avalia `code` no realm remoto, de quatro formas: direto, em strict, por eval indireto e por new Function.
const forms = code => {
  expr(`sr.evaluate(${q(code)})`);
  expr(`sr.evaluate(${q('"use strict";' + code)})`);
  expr(`sr.evaluate(${q("(0, eval)(" + q(code) + ")")})`);
  expr(`sr.evaluate(${q("new Function(" + q("return (" + code + "\n)") + ")()")})`);
};

// ---- 1. Fonte exótica: comentários HTML-like.
const html = [
  "1<!--2\n3", "1\n-->2\n3", "var x=1;\n-->y\nx", "/*\n*/-->c\n5", "/**/ --> d\n6", "<!-- a\n7", "3 //x\n-->y\n4", "var a=1;a<!--a\na",
  "var a=5,b=2;a-->b", "var a=5;a-->0", "var a=5;a --> 0", "var a=5;a-- >0", "<!--\n-->\n8", "9<!-- 1\n-->2", "'<!--'", "'-->'",
  "`<!--\n-->`", "/<!--/.source", "/-->/.source", "var s='x';s+\n<!--\n's'", "1 /* a\nb */ --> c\n2", "1 /* a */ --> c\n2", "\n-->x\n10",
  "  \t-->x\n11", "/* c */ \n--> x\n12", "/* c */ --> x\n13", "0\n/* \n */-->x\n14", "x=3\n<!-- y\nx", "var f=function(){return 15\n-->1\n};f()",
  "var o={a:1<!--2\n};o.a", "[1<!--2\n,3].length", "if(1)<!--2\n16", "eval('1<!--2\\n3')", "eval('1\\n-->2\\n3')", "new Function('<!--\\nreturn 17')()",
  "new Function('-->\\nreturn 18')()", "new Function('a','-->\\nreturn a')(19)", "new Function('a -->','return 1')", "new Function('a','b','-->')", "Function('<!-- x','return 20')()",
  "var a=1,b=1;a<!--b\na", "var a=1;a<!-- -->2\na", "1\u2028-->x\n21", "1\u2029-->x\n22", "1\r-->x\n23", "1\r\n-->x\n24", "1\n<!--x\u202825",
];
for (const c of html) forms(c);

// ---- 2. Octais legados e escapes.
const octal = [
  "010", "08", "09.5", "0o10", "0O17", "0b101", "0B11", "0x1F", "00", "000", "0_1", "01_0", "08_1", "0.1_0", "1_0", "1__0", "1_", "0x_1", "010.5", "010..toString()", "08.5", "09e1",
  "'\\08'", "'\\1'", "'\\7'", "'\\8'", "'\\9'", "'\\00'", "'\\000'", "'\\377'", "'\\400'", "'\\477'", "'\\0'", "'\\01'", "'\\08'.length", "'\\3777'", "'\\u{41}'", "'\\u{110000}'",
  "'\\x4'", "'\\xg0'", "'\\u00'", "`\\1`", "`\\0`", "`\\00`", "`\\u{`", "String.raw`\\1\\u{`", "(x=>x)`\\01`", "010+010", "-010", "+08", "typeof 010", "010===8", "0.0", "00.5", "07.5", "5.toString()", "5..toString()", "5 .toString()",
  "(function(){'use strict';return 010})", "(function(){return 010;'use strict'})()", "(function(){'use strict';return '\\1'})", "(function(){'\\1';'use strict'})", "(function(){'use strict';'\\1'})",
  "(function(a=1){'use strict'})", "(function(a){'use strict';return 010})", "'\\1'+'use strict'", "(function(){'\\01';return typeof this})()", "(function(){'use strict';'\\01';})", "(function(){'\\01';'use strict';return typeof this})()",
  "(function(){'use\\x20strict';return typeof this})()", "(function(){'use strict'+1;return typeof this})()", "(function(){('use strict');return typeof this})()", "(function(){\"use strict\";return typeof this})()", "(function(){`use strict`;return typeof this})()",
];
for (const c of octal) forms(c);

// ---- 3. let, yield, await, async, static, of, get, set como identificador.
const idents = [
  "var let=3;let", "let = 5; let", "(function(){var let=1;return let})()", "(function(let){return let})(4)", "var o={let:1};o.let", "var o={let};o.let", "for(let in {a:1});1", "var let;for(let of [1]);2",
  "for(let=0;let<2;let++);let", "let\nx=1;x", "let\n[x]=[1];x", "let \n{x}=({x:2});x", "let[a]=[3];a", "let\nlet=1", "let let=1", "const let=1", "var yield=4;yield", "var await=5;await", "var async=6;async",
  "var of=7;of", "var get=8;get", "var set=9;set", "var static=10;static", "var implements=11;implements", "var interface=12", "var package=13;package", "var private=14;private", "var protected=15;protected",
  "var public=16;public", "var enum=1", "var yield;yield=1;yield", "function yield(){return 17}yield()", "function await(){return 18}await()", "(function*(){var yield})", "(function*(){yield\n1})().next().value", "(function*(){yield\n*1})",
  "(function*(){yield*[1]})().next().value", "(function*(){var x=yield;return x})().next(5).value", "(function*(){yield yield 1})().next().value", "(function*(yield){})", "(function*(){function yield(){}})", "(function*(){(yield)=>1})",
  "(function*(){yield => 1})", "(async function(){var await})", "(async function(){await\n1})", "(async function(await){})", "(async function(){function await(){}})", "(async function(){(await)=>1})", "(async function(){await => 1})",
  "async\nfunction f(){}typeof async", "async function f(){}typeof f", "var async=function(x){return x};async\n(1)", "var async=function(x){return x+1};async(1)", "async x=>x", "async (x)=>x", "async\n(x)=>x", "async\nx=>x", "(async)=>1", "async=>async", "async async=>1",
  "var o={async(){return 1}};o.async()", "var o={async:1};o.async", "var o={async\n(){}};o.async", "var o={async\nm(){}}", "var o={get\nm(){return 1}};o.m", "var o={get(){return 1}};o.get()", "var o={set(v){}};typeof o.set", "var o={static(){return 1}};o.static()",
  "class A{static\nm(){return 1}};typeof A.m", "class A{static static(){return 2}};A.static()", "class A{get get(){return 3}};new A().get", "class A{async\nm(){}};typeof A.prototype.m", "class A{static async*m(){}};typeof A.m",
  "var x={yield:1,await:2,let:3};x.yield+x.await+x.let", "var yield=1,await=2,let=3;yield+await+let", "'use strict';var yield=1", "'use strict';var let=1", "'use strict';var await=1;await", "'use strict';var static=1", "'use strict';var async=1;async",
  "'use strict';var of=1;of", "'use strict';var implements=1", "'use strict';function f(yield){}", "'use strict';function f(let){}", "'use strict';(function eval(){})", "'use strict';(function arguments(){})", "'use strict';var eval=1", "'use strict';arguments=1",
  "'use strict';eval=1", "'use strict';({eval}=1)", "'use strict';({eval:eval}=1)", "'use strict';({a:arguments}=1)", "'use strict';[eval]=[1]", "'use strict';eval++", "'use strict';--arguments", "'use strict';for(eval in {});", "'use strict';for(arguments of []);",
  "'use strict';(eval)=>1", "'use strict';(arguments)=>1", "'use strict';eval=>1", "'use strict';(a,eval)=>1", "'use strict';function f(a,eval){}", "'use strict';function f(a,a){}", "function f(a,a){return a}f(1,2)", "(function(a,a){return a})(1,2)", "(function(a,a){'use strict'})",
  "(function(a,a=1){})", "((a,a)=>1)", "(function(a,[a]){})", "(function(a,...a){})", "(function(a,{a}){})", "function f(a,a){return arguments[0]}f(1,2)", "function f(a,a){a=9;return arguments[1]}f(1,2)", "function f(a,a){a=9;return arguments[0]}f(1,2)",
  "var o={m(a,a){}}", "var o={set s(a){}};1", "class A{m(a,a){}}", "(function(a,b,a){return b})(1,2,3)", "(function(a,b,a){return a})(1,2,3)", "(function(a,b,a){return arguments.length})(1,2,3)",
  "typeof let", "typeof yield", "typeof await", "typeof async", "void let", "let\n++\nx", "var x=1;let\n++x", "if(1)let\nx=2;x", "if(1)let\n[0];3",
];
for (const c of idents) forms(c);

// ---- 4. ASI.
const asi = [
  "var a=1\nvar b=2\na+b", "var a=1,b=2\n;[a,b].length", "var a=1\n[a].length", "var a=1\n(function(){return 2})", "var f=function(){return 5}\n(function(){return 6})()", "var a=1\n+1\na", "var a=1\n++a", "var a=1,b=2\na\n++\nb\n[a,b].join()",
  "var a=1,b=2\na\n++b\n[a,b].join()", "return 1", "(function(){return\n1})()", "(function(){return /*\n*/1})()", "(function(){return /* */1})()", "(function(){return //\n1})()", "(function(){throw\n1})", "(function(){break})", "while(0){break\nx}1", "a:while(1){break\na}2",
  "a:while(1){break a\n}3", "a:while(1){continue\na}", "do;while(0)1", "do;while(0) 1", "do{}while(0)1", "do 1\nwhile(0)2", "for(;\n;){break}3", "for(\n;;){break}", "for(;;\n){break}4", "for(var i=0\ni<2;i++);i", "for(var i=0;i<2\ni++);i",
  "if(1)\n;2", "if(1) 3\nelse 4", "if(0) 3;else 4", "if(0) 3\n;else 4", "if(1)3;\nelse 4", "var a=1;a\n++\n;a", "var a={b:1}\n/foo/g", "var a=1,g=1\na\n/foo/g", "1\n/2/1", "var a=4,b=2,g=1\na\n/b/g", "var x=1\n`a`", "var x=function(){return 1}\n`a`", "var t=x=>x\n`a`",
  "var o={}\n[0]=1;o[0]", "var o={},a=[1]\no\n[0]", "'a'\n'b'", "'a'\n.length", "1\n.toString()", "1\n..toString", "x\n=>1", "(x)\n=>1", "async x\n=>1", "var f=x=>\n1;f()", "var f=(x)=>\n1;f()", "var f=x=>{}\n(1)", "var f=x=>{}\n1", "var f=x=>{}\n+1",
  "var f=x=>({})\n(1)", "var f=()=>1\n(2)", "class A{a=1\nb=2}", "class A{a=1\n[b]=2}", "class A{a\n*m(){}}", "class A{a\nm(){}}", "class A{static\n*m(){}}", "class A{get\n*m(){}}", "class A{a=1\n*m(){}}", "class A{a\nstatic m(){}}", "class A{get\nstatic(){}}",
  "class A{static\nstatic\nm(){}}", "class A{'a'\n'b'}", "class A{1\n2}", "class A{a;b}", "class A{a,b}", "class A{;;;}", "class A{static a\nb}", "class A{in\nx}", "class A{a=1\nin\nx}", "class A{a=b\nin\n1}", "class A{x=1\n++y}", "class A{x\n++y}",
  "var o={a:1\n,b:2};o.b", "var o={a:1\nb:2}", "var a=[1\n,2];a.length", "var a=[1\n2]", "new\nFunction('return 1')()", "new\n.target", "(function(){new\n.target})", "function f(){return new\n.target}f()", "typeof\n1", "void\n1", "delete\nx", "!\n1", "1\n!\n1", "1\n!1", "1\n!==1",
  "var i=1;i\n++\ni", "var i=1,j=1;i\n++j;j", "var i=1,j=1;i\n--j;j", "var i=1;i\n--\ni", "var a=1;a\n?\n2\n:\n3", "var a=1\n,b=2\n;b", "1\n,2", "1,\n2", "yield\n1", "(function*(){yield\n/1/})", "(function*(){yield /1/})", "(function*(){yield/1/g})",
  "(function*(){var x=yield\n/1/g})", "(function*(){return yield\n1})().next().value", "(function*(){yield\n}).constructor.name", "var o={*g(){yield\n1}};o.g().next().value", "var o={*\ng(){}};typeof o.g", "var o={async\n*g(){}}", "var o={async*\ng(){}};typeof o.g",
  "label:\nfunction f(){}typeof f", "label:\n1", "a:\nb:\nc:\n1", "function f(){}\n/1/.source", "var f=function(){}\n/1/.source", "({})\n/1/.source", "{}\n/1/.source", "({}\n/1/)", "1\n/**/\n/1/.test('1')", "1\n/**/\n/1/g.test('1')",
  "'use strict'\n010", "'use strict'\n+1", "'use strict'\n'\\1'", "'use strict'\n.length", "'use strict'\n[0]", "'use strict'\n(1)", "'use strict'\nvar x=010", "('use strict')\nvar x=010", "'use strict'\n,1", "'use strict'\n;var x=010",
  "(function(){'use strict'\n+1;return typeof this})()", "(function(){'use strict'\n;return typeof this})()", "(function(){'use strict'\n.length;return typeof this})()", "(function(){'use strict'\n[0];return typeof this})()", "(function(){'use strict'\n(1);return typeof this})()",
];
for (const c of asi) forms(c);

// ---- 5. Regex contra divisão.
const slash = [
  "4/2/1", "var g=2;4 /g/ 1", "/=/.test('=')", "var a=6,g=2;a/=g;a", "/=/g.source", "typeof /a/ /1", "typeof /a/\n/1", "var x={}\n/foo/g.test('foo')", "{}/1/2", "({})/1/2", "(1)/2/1", "var a=1;a++ /2/ 1", "var a=1;a++\n/2/1", "if(1)/a/.test('a')",
  "if(0);/a/.test('a')", "while(0);/a/.source", "do/a/.test('a');while(0)", "var f=function(){}/1/1", "function f(){}/1/.source", "[1]/1/1", "[]/1/1", "a=>{}/1/.source", "var a=4;a\n/2/1", "var a=4;a/2\n/1", "1/\n2", "4 / /2/", "4 /*c*/ / 2", "4//c\n/ 2",
  "'a'/'b'", "'a'.length/2", "typeof /=/", "/[/]/.test('/')", "/[/]/.source", "/\\//.source", "/a\\/b/.source", "/[\\]/]/.source", "/[^]/.test('\\n')", "/a/ /b/", "/a/g.flags", "/a/gg", "/a/x", "/a/\\u0067", "/a/g\\u0069", "/(?:)/.source", "new RegExp('').source", "new RegExp('/').source", "new RegExp('\\n').source",
  "new RegExp('\\\\n').source", "new RegExp('[/]').source", "RegExp('a','gimsuyd').flags", "RegExp('a','v').flags", "RegExp('a','uv')", "/a/\n.test('a')", "/a/.\ntest('a')", "/*/", "//", "/a\n/", "/a\u2028/", "/[\n]/", "/a/ in {}", "/a/ instanceof RegExp", "typeof /a/", "void /a/", "!/a/", "-/1/", "+/1/", "~/1/",
  "1 + /1/.source", "1 + /1/ .source", "var x=1;x /1/ 1", "var x=1;x /1/g", "var x=4,g=2,i=1;x /g/ i", "var re=/a/;re /1", "this /1/ 1", "null /1/ 1", "true /1/ 1", "typeof /1/ /1/", "x=>1/1/1", "(x=>x)/1/1", "[,/1/]", "[,/1/].length", "[/1/].length", "[/1/,/2/].length", "({a:/1/}).a.source",
  "`${/1/.source}`", "`${1/1}`", "`${1}/${1}`", "`/${/a/.source}/`", "`${`${/a/.source}`}`", "(/1/)", "(/1/).source", "/1/.source+/2/.source", "/1/.source/2/", "/1/.source\n/2/", "`a`/1/1", "`a`\n/1/g", "1\n/1/g", "1\n/1/g\n/1/g",
  "var a=1;a?/b/.source:2", "var a=0;a?1:/b/.source", "var a=1;a&&/b/.source", "var a=1;a||/b/.source", "var a=0;a??/b/.source", "var o={a:1};o?.a/1/1", "var o={a:4};o?.a/2", "var a=[4];a[0]/2", "var a=[4];a[0]\n/2/1", "var a={b:4};a.b/2/1", "var a={b:4};a.b\n/2/1",
  "(function(){return/1/})().source", "(function(){return /1/})().source", "(function(){return\n/1/})()", "(function(){return/*c*/-1})()", "(function(){throw/1/})", "(function*(){yield/1/})().next().value.source", "(async function(){await/1/})", "typeof/1/", "void/1/", "delete/1/.x", "new/1/.constructor", "new /1/.constructor()",
  "1 in/1/", "1 instanceof/1/", "do/1/;while(0)", "else/1/", "if(0)1;else/1/.source", "case/1/:", "switch(1){case/1/.source:}", "switch(1){case 1:/1/.source}", "x=>/1/", "(x=>/1/)().source", "(x=>/1/g)().flags", "(x=>x/1/1)(4)",
  "var a=1;a/*c*/++/*c*/;a", "var a=1;a/**/ /1", "var x=1;x<!--/1/\n2", "var x=4;x-->/2/\n2", "1//1/\n2", "1/*/*/2", "1/*/2*/3", "/*/*/ 2",
];
for (const c of slash) forms(c);

// ---- 6. Labels.
const labels = [
  "a:b:c:1", "a:{break a;}2", "a:for(;;){break a}3", "a:b:for(;;){break a}4", "a:b:for(;;){continue b}", "a:{a:1}", "a:a:1", "a:{b:{break a}}5", "a:{continue a}", "a:{b:for(;;){break a}}6", "a:function f(){}typeof f", "'use strict';a:function f(){}", "a:function*f(){}", "a:async function f(){}", "a:class A{}", "a:let x", "a:let\nx", "a:const x=1", "a:var x=1;x", "a:if(1)function f(){}", "a:while(0)function f(){}",
  "yield:1", "await:1", "let:1", "async:1", "of:1", "static:1", "'use strict';yield:1", "'use strict';let:1", "'use strict';await:1", "(function*(){yield:1})", "(async function(){await:1})", "a:{break a}\na:{break a}\n7", "a:{}\na:{}\n8", "a:{a:{}}", "a:(function(){a:1})()", "a:(function(){break a})", "a:(function(){continue a})", "a:(()=>{break a})",
  "a:for(var i=0;i<3;i++){for(var j=0;j<3;j++){if(j==1)continue a}}i+j", "a:for(var i=0;i<3;i++){b:for(var j=0;j<3;j++){if(j==1)continue a;if(i==2)break b}}i+','+j", "a:do{break a}while(0)9", "a:switch(1){case 1:break a}10", "a:switch(1){case 1:continue a}", "a:if(1){break a}11", "a:try{break a}finally{}12",
  "a:try{}finally{break a}13", "var r=0;a:try{r=1;break a}finally{r+=10}r", "var r=0;a:{try{throw 1}catch(e){break a}finally{r=5}}r", "var r=0;a:for(;;){try{continue a}finally{r++;if(r>2)break a}}r", "var r=0;a:for(;;){try{break a}finally{r=7}}r", "var r=0;a:for(;;){try{break a}finally{continue a}}",
  "var r=0;a:for(;r<2;){try{break a}finally{r++;continue a}}r", "a:{b:{c:{break b}}14}", "a:{b:{c:{break c}}15}", "a:break a;16", "a:;17", "a:\n;18", "a:\n\n1", "a:/*c*/1", "a:/*\n*/1", "a\n:1", "a\n:\n1", "a :1", "a : 1", "a:2\n:3", "1:2", "a.b:1", "a?b:c", "var a=1,b=2,c=3;a?b:c", "var b=1;a:b",
  "a:b", "a:1 ? 2 : 3", "a:{break\na}", "a:{break /*\n*/ a}", "a:for(;;){break // c\na}", "a:for(;;){continue\na}", "a:while(1){break\n}", "a:{break;}", "a:{break}", "a:{break a b}", "a:{break 1}", "a:{break a.b}", "break", "continue", "while(1){break}", "while(0){continue}", "if(1)break", "function f(){break}", "function f(){continue}",
  "a:{function f(){break a}}", "a:{(function(){break a})}", "a:{class A{static{break a}}}", "a:{class A{static{return}}}", "a:{class A{static{await 1}}}", "class A{static{var await}}", "class A{static{await}}", "class A{static{yield}}", "class A{static{arguments}}", "class A{static{var arguments}}", "class A{static{this.x=1}};A.x", "class A{static{this.x=1}};1",
  "var l=1;l:l", "l:{var l=2}l", "var o={l:1};o.l", "var o={l:{}};l:o", "({a:1})", "({a:1}).a", "{a:1}", "{a:1,b:2}", "{a:1;b:2}", "{a:1;b:2}\n3", "{a:{b:1}}", "{a:1}\n{a:2}\n3", "{a:{}}", "{}", "({}).constructor===Object", "{;}", "{;;}", "{\n}", "{}{}", "{{}}1", "{1}2", "{1;2}", "{'a'}", "{'use strict';010}",
];
for (const c of labels) forms(c);

// ---- 7. Construtores de função dentro do realm: parâmetros x corpos x chamada.
const params = [
  [], ["a"], ["a,b"], ["a", "b"], ["a=1"], ["a", "b=a+1"], ["{a}"], ["{a,b=2}"], ["[a,b]"], ["[a,,b]"], ["...r"], ["a", "...r"], ["a,...r"], ["{a:[b]}"], ["a=1", "{b}={b:2}"], ["a,"], ["a,b,"], ["a /*c*/"], ["/*c*/ a"], ["a //c\n"], ["//c\na"], ["a,/*x*/b"],
  ["a", "a"], ["a,a"], ["a", "a=1"], ["eval"], ["arguments"], ["yield"], ["await"], ["let"], ["static"], ["async"], ["of"], ["a\u2028"], ["a\u2029,b"], ["\u2028a"], ["a\n,b"], ["a\r\n"], ["a/*\n*/"], ["a-->"], ["<!--a"], ["a<!--"], ["a\n-->"],
  ["a)", "{"], ["a){"], ["a", "){"], ["/*", "*/"], ["a=/*", "*/1"], ["a=`", "`"], ["a='", "'"], ["a=1", "a"], ["{a}", "a"], ["{a}", "{a}"], ["[a]", "[a]"], ["...a", "b"], ["...a,"], ["...a", "..."], ["...[a,b]"], ["...{length}"], ["a=arguments"], ["a=this"],
  ["a=new.target"], ["a=super.x"], ["a=super()"], ["a=function(){return a}"], ["a=()=>a"], ["a=eval('1')"], ["a=eval('var z=1')", "b=z"], ["a=yield"], ["a=await"], ["a=1;"], ["a;b"], ["a b"], ["1"], ["'a'"], ["a.b"], ["a?.b"], ["[a.b]"], ["{a:b.c}"], ["{a:1}"], ["[1]"], ["a=1,b"], ["(a)"], ["(a,b)"], ["a,(b)"],
  ["\u00e9"], ["\\u0061"], ["a\\u0062"], ["\\u{61}"], ["\\u0031"], ["\ud835\udc9c"], ["\u{1d49c}"], ["a\u200c"], ["a\u200d"], ["\u200ca"], ["a\u00b7"], ["a\u2118"], ["\u212e"], ["\u309b"],
];
const bodies = [
  "", "return 1", "return a", "return typeof a", "return arguments.length", "return [].slice.call(arguments).join()", "return this===undefined", "return typeof this", "return typeof new.target", "'use strict';return this===undefined", "'use strict';return typeof this",
  "return b", "return r", "return r&&r.length", "return a+b", "return a===undefined", "return eval('a')", "return eval('typeof a')", "var a=7;return a", "var a;return a", "let a=7;return a", "let a;return typeof a", "function a(){}return typeof a", "return function(){return a}()",
  "-->\nreturn 1", "-->x\nreturn 2", "<!--\nreturn 3", "//\nreturn 4", "/*\n*/return 5", "return 6//", "return 7/*", "return 8\n-->", "return 9<!--\n", "}", "{", "} {", "}{", "/*}*/return 10", "//}\nreturn 11", "`}`", "return `}`", "return '}'", "return /}/.source",
  "'use strict';010", "'use strict';return 010", "return 010", "'\\1';'use strict'", "'use strict';'\\1'", "'use strict';var eval", "'use strict';var arguments", "'use strict';with(a){}", "with(a){}", "'use strict';delete a", "delete a", "'use strict';a=1;return a", "'use strict';a=1", "'use strict';return arguments.length", "'use strict';return eval('var z=1;typeof z')",
  "return eval('var z=1;typeof z')", "eval('var z=1');return typeof z", "'use strict';eval('var z=1');return typeof z", "return arguments[0]", "a=2;return arguments[0]", "'use strict';a=2;return arguments[0]", "return a=3", "return (a,b)", "return [a,b]", "return {a,b}", "return {a}", "return{a:a}", "return a\n+1", "return\na", "return/*\n*/a",
  "yield 1", "await 1", "return yield", "return await", "var yield=1;return yield", "var await=1;return await", "return yield=>1", "return await=>1", "return async=>1", "return async()", "return async function(){}", "return class{}", "return class{static{}}", "class A{}return typeof A", "return new.target", "return new.target===undefined", "return super.x", "super()", "return import.meta", "import('x')", "export var x", "import x from 'y'",
  "return this", "return this.x", "this.x=1;return this.x", "return typeof globalThis", "var globalThis=1;return globalThis", "return typeof Object", "return Object.name", "return Function.name", "return (function(){}).name", "return (function f(){}).name", "return (()=>{}).name", "return ({m(){}}).m.name", "return (class{}).name", "return (class C{}).name",
  "return arguments.callee", "'use strict';return arguments.callee", "return arguments.caller", "return typeof arguments.callee", "return arguments.length+arguments.callee.length", "return arguments.callee.length", "return arguments.callee.name", "return arguments.callee.toString()", "return arguments.callee.toString().length", "return Object.prototype.toString.call(arguments)",
  "return typeof arguments", "arguments=1", "'use strict';arguments=1", "var arguments=1;return arguments", "var arguments;return typeof arguments", "function arguments(){}return typeof arguments", "return typeof eval", "eval=1", "'use strict';eval=1", "var eval=1;return eval", "return a\u2028", "return\u2028a", "return 1\u2029+1", "return '\u2028'.length", "return '\\\u2028'.length", "return '\u2029'.charCodeAt(0)", "return `\u2028`.length", "return /\u2028/",
  "return \u2028 1", "\u2028return 1", "return /*\u2028*/ 1", "return //\u20281", "return 1 //\u2028+1", "// c\u2028return 1", "return '\\\u2029a'", "return 1;\u2028-->x\n", "return '\\u2028'.length", "return eval('\\u2028')", "return eval('1\u2028+1')", "return eval('1//\u2028+1')", "return eval('\"\u2028\"').length", "return eval('\"\\\u2028\"').length",
];
const callArgs = ["", "1", "1,2", "1,2,3", "undefined", "{x:1}", "[1,2]", "'s'", "null,null"];
// Os corpos relevantes para um par de parâmetros são amostrados em rotação: matriz completa só para parâmetros curtos.
let rot = 0;
for (const p of params) {
  const plist = p.map(q).join(", ");
  const sample = [];
  for (let i = 0; i < 9; i++) sample.push(bodies[(rot++) % bodies.length]);
  for (const b of sample) {
    expr(`sr.evaluate(${q(`Function(${plist}${plist ? ", " : ""}${q(b)})`)})(${callArgs[(rot) % callArgs.length]})`);
  }
  expr(`sr.evaluate(${q(`Function(${plist}${plist ? ", " : ""}"return 1").toString()`)})`);
  expr(`sr.evaluate(${q(`Function(${plist}${plist ? ", " : ""}"return 1").length`)})`);
  expr(`sr.evaluate(${q(`Function(${plist}${plist ? ", " : ""}"/*b*/").toString().length`)})`);
}
for (const b of bodies) {
  expr(`sr.evaluate(${q(`Function(${q(b)}).toString()`)})`);
  expr(`sr.evaluate(${q(`Function("a", "b", ${q(b)}).toString()`)})`);
  expr(`sr.evaluate(${q(`Function(${q(b)})()`)})`);
  expr(`sr.evaluate(${q(`Function("a", ${q(b)})(1)`)})`);
}

// ---- 8. As quatro classes de construtor dentro do realm: nome, protótipos, corpo e chamada por next().
const ctorExpr = {
  Function: "Function",
  Generator: "Object.getPrototypeOf(function*(){}).constructor",
  Async: "Object.getPrototypeOf(async function(){}).constructor",
  AsyncGenerator: "Object.getPrototypeOf(async function*(){}).constructor",
};
for (const [kind, c] of Object.entries(ctorExpr)) {
  const probes = [
    `${c}.name`, `${c}.length`, `typeof ${c}`, `${c}.prototype===${c}.prototype`, `Object.getPrototypeOf(${c})===Function`, `Object.getPrototypeOf(${c})===Function.prototype`, `${c}.prototype.constructor===${c}`, `Object.prototype.toString.call(${c}.prototype)`,
    `Object.getOwnPropertyNames(${c}).join()`, `Object.getOwnPropertyNames(${c}.prototype).join()`, `String(${c})`, `${c}.toString()`, `${c}("return 1").name`, `${c}("return 1").length`, `new ${c}("return 1").name`, `new ${c}("a","return 1").length`, `new ${c}("a","b","return 1").length`,
    `${c}("a=1","b","return 1").length`, `${c}("...a","return 1").length`, `${c}("{a,b}","return 1").length`, `${c}("[a]","b","return 1").length`, `Object.getPrototypeOf(${c}("")).constructor===${c}`, `Object.getPrototypeOf(${c}(""))===${c}.prototype`, `${c}("").hasOwnProperty("prototype")`,
    `Object.getOwnPropertyNames(${c}("")).join()`, `Object.getOwnPropertyNames(${c}("a","")).join()`, `${c}("").toString()`, `${c}("a","b","return a").toString()`, `${c}("a,b","return a").toString()`, `${c}("a","b","").toString()`, `${c}("a\\n","").toString()`, `${c}("a //\\n","").toString()`,
    `${c}("a,b","/*x*/").toString()`, `${c}("/*p*/a","//b\\nreturn a").toString()`, `Object.prototype.toString.call(${c}(""))`, `Object.prototype.toString.call(${c}("").prototype)`, `typeof ${c}("").prototype`, `Object.getPrototypeOf(${c}("").prototype)===Object.prototype`,
    `Object.getPrototypeOf(${c}("").prototype)===Object.getPrototypeOf(function*(){}).prototype`, `Object.getPrototypeOf(${c}("").prototype)===Object.getPrototypeOf(async function*(){}).prototype`, `${c}("").prototype===${c}("").prototype`, `Reflect.ownKeys(${c}("").prototype).length`,
    `${c}("yield 1")`, `${c}("await 1")`, `${c}("yield\\n1")`, `${c}("a=yield","")`, `${c}("a=await","")`, `${c}("yield","")`, `${c}("await","")`, `${c}("a","yield")`, `${c}("a","await")`, `${c}("a","var yield")`, `${c}("a","var await")`, `${c}("a","function yield(){}")`, `${c}("a","function await(){}")`,
    `${c}("super()")`, `${c}("super.x")`, `${c}("new.target")`, `${c}("a=new.target","")`, `${c}("a","'use strict'; with(a){}")`, `${c}("a=1","'use strict'")`, `${c}("[a]","'use strict'")`, `${c}("{a}","'use strict'")`, `${c}("...a","'use strict'")`, `${c}("a","a","'use strict'")`, `${c}("a","a","")`,
    `${c}("eval","'use strict'")`, `${c}("arguments","'use strict'")`, `${c}("a","'use strict';var eval")`, `${c}("a","'use strict';eval=1")`, `${c}("a","'use strict';010")`, `${c}("a","'use strict';return '\\\\1'")`, `${c}("static","'use strict'")`, `${c}("let","'use strict'")`, `${c}("yield","'use strict'")`, `${c}("a","}")`, `${c}("a","}{")`, `${c}("a){","")`, `${c}("a","/*")`, `${c}("/*","*/")`,
    `${c}("a","-->\\n")`, `${c}("-->","")`, `${c}("<!--","")`, `${c}("a","<!--\\n")`, `${c}("a","//")`, `${c}("a","//\\u2028")`, `${c}("a","return '\\u2028'.length")`,
  ];
  for (const e of probes) expr(`sr.evaluate(${q(`(function(){try{return ${e}}catch(e){return e.name+": "+e.message}})()`)})`);
}
// next() dos geradores dinâmicos devolve um primitivo.
for (const b of ["yield 1", "yield 1; yield 2", "var x = yield 1; return x", "yield* [1,2]", "return 5", "", "yield", "yield a", "yield arguments.length", "yield this===undefined", "yield typeof this", "try{yield 1}finally{yield 2}", "yield yield 3", "var o={*g(){yield 4}};yield* o.g()"]) {
  for (const n of [1, 2, 3]) {
    const steps = Array.from({ length: n }, (_, i) => `g.next(${i + 10})`).map(s => `r.push(JSON.stringify(${s}))`).join(";");
    expr(`sr.evaluate(${q(`(function(){var g=Object.getPrototypeOf(function*(){}).constructor("a",${q(b)})(5,6);var r=[];${steps};return r.join("|")})()`)})`);
  }
}
for (const b of ["return 1", "await 1", "return await 2", "var x = await 3; return x", "throw 4", "return arguments.length", "await Promise.reject(5)", "return typeof this", "for await (var x of [1]) return x", "yield 1", "yield await 2", "return await new Promise(r=>r(7))"]) {
  for (const kind of ["Async", "AsyncGenerator"]) {
    expr(`sr.evaluate(${q(`(function(){try{var f=${ctorExpr[kind]}("a",${q(b)});return typeof f(1)+"|"+Object.prototype.toString.call(f(1))}catch(e){return e.name+": "+e.message}})()`)})`);
  }
}

// ---- 9. Globais do realm remoto vistos pelo wrapper.
const globals = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Date", "RegExp", "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError", "Promise", "Proxy", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry",
  "ArrayBuffer", "SharedArrayBuffer", "DataView", "Uint8Array", "Int8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Float16Array", "Iterator", "ShadowRealm", "Intl", "Intl.DateTimeFormat", "Intl.NumberFormat", "Intl.Collator", "Intl.PluralRules", "Intl.Locale", "Intl.Segmenter",
  "Math", "JSON", "Reflect", "Atomics", "WebAssembly", "globalThis", "eval", "parseInt", "parseFloat", "isNaN", "isFinite", "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent", "escape", "unescape", "Math.max", "Math.random", "JSON.parse", "JSON.stringify", "Reflect.ownKeys", "Object.keys", "Array.isArray",
  "Array.prototype.map", "String.prototype.at", "Symbol.iterator", "Symbol.for", "Promise.resolve", "Date.now", "Object.prototype.toString", "Function.prototype.call", "Function.prototype.bind", "Function.prototype.toString", "Array.from", "Number.parseFloat", "Number.isInteger", "Map.prototype.get", "Atomics.add", "BigInt.asUintN", "Proxy.revocable",
];
for (const g of globals) {
  const idn = g;
  const evals = [
    `typeof ${idn}`, `${idn}.name`, `${idn}.length`, `String(${idn}).length>0`, `${idn}===globalThis.${idn.split(".")[0]}${idn.includes(".") ? idn.slice(idn.indexOf(".")) : ""}`, `Object.prototype.toString.call(${idn})`, `Object.getOwnPropertyNames(${idn}).length>0`,
    `typeof ${idn}.prototype`, `Object.getPrototypeOf(${idn})===Function.prototype`, `Object.getPrototypeOf(${idn})===Object.prototype`, `Object.isExtensible(${idn})`, `Object.isFrozen(${idn})`, `${idn}.constructor===Object`,
    `Object.getOwnPropertyDescriptor(globalThis,${q(idn.split(".")[0])}).writable`, `Object.getOwnPropertyDescriptor(globalThis,${q(idn.split(".")[0])}).enumerable`, `Object.getOwnPropertyDescriptor(globalThis,${q(idn.split(".")[0])}).configurable`,
  ];
  for (const e of evals) expr(`sr.evaluate(${q(`(function(){try{return ${e}}catch(e){return e.name+": "+e.message}})()`)})`);
  // O wrapper de uma função do realm remoto, quando é função.
  expr(`sr.evaluate(${q(`(function(){try{return ${idn}}catch(e){return 1}})()`)})`);
  expr(`typeof sr.evaluate(${q(idn)})`);
  expr(`sr.evaluate(${q(idn)}) === ${idn.split(".")[0] === "ShadowRealm" || idn === "Intl" ? "1" : idn}`);
  expr(`sr.evaluate(${q(idn)}).name`);
  expr(`sr.evaluate(${q(idn)}).length`);
  expr(`Object.getPrototypeOf(sr.evaluate(${q(idn)})) === Function.prototype`);
  expr(`Object.getOwnPropertyNames(sr.evaluate(${q(idn)})).join()`);
  expr(`Object.prototype.toString.call(sr.evaluate(${q(idn)}))`);
  expr(`String(sr.evaluate(${q(idn)}))`);
  expr(`sr.evaluate(${q(idn)}).prototype`);
}

// ---- 10. Chamar funções nativas do realm pelo wrapper com argumentos primitivos.
const natives = [
  ["Math.max", ["1,2", "", "NaN,1", "-0,0", "'3',2", "1n"]], ["Math.min", ["", "1,2", "-0,0"]], ["Math.hypot", ["3,4", ""]], ["Math.atan2", ["1,1", "0,-0"]], ["Math.round", ["2.5", "-2.5", "-0.5"]], ["Math.sign", ["-3", "0", "-0"]], ["Math.fround", ["5.5", "5.05"]], ["Math.clz32", ["1", "0"]], ["Math.imul", ["2,3", "0xffffffff,5"]],
  ["parseInt", ["'12px'", "'0x1f'", "'z',36", "'  9'", "''", "'1e3'"]], ["parseFloat", ["'1.5e2x'", "'.5'", "'-.5e-1'", "'Infinityx'"]], ["isNaN", ["'x'", "''"]], ["isFinite", ["'1'", "Infinity"]], ["escape", ["'a b\\u00e9'"]], ["unescape", ["'%u00e9%20'"]],
  ["encodeURIComponent", ["'a b&c'", "'\\u00e9'", "'\\ud800'"]], ["decodeURIComponent", ["'%41'", "'%'", "'%e9'"]], ["encodeURI", ["'a b#c'"]], ["decodeURI", ["'%23'"]], ["Number", ["'0x10'", "'1_0'", "''", "' 5 '", "'5n'", "10n"]], ["String", ["Symbol('a')", "1n", "-0", "null"]], ["Boolean", ["''", "'0'", "0n"]],
  ["BigInt", ["10", "'0x10'", "1.5", "''", "'  3 '", "true"]], ["Symbol", ["'d'", ""]], ["Symbol.for", ["'k'"]], ["Date.UTC", ["2020,0,1", "99", "NaN"]], ["Date.parse", ["'2020-01-01'", "'x'", "'2020-01-01T00:00:00Z'"]], ["Number.isInteger", ["5.0", "5.5", "'5'"]], ["Number.isSafeInteger", ["2**53", "2**53-1"]], ["Number.parseFloat", ["'1.5'"]],
  ["Object.is", ["NaN,NaN", "0,-0"]], ["Array.isArray", ["1"]], ["JSON.stringify", ["1", "'a'", "undefined", "null", "1n"]], ["JSON.parse", ["'1'", "'\"a\"'", "'x'", "'{'", "'[1,'"]], ["String.fromCharCode", ["65,66", "0x10041"]], ["String.fromCodePoint", ["65", "0x110000", "-1"]], ["String.raw", ["{raw:'abc'},1,2"]],
  ["Atomics.add", ["1,2"]], ["BigInt.asUintN", ["8,257n", "64,-1n"]], ["BigInt.asIntN", ["8,255n"]], ["Reflect.ownKeys", ["1"]], ["Reflect.has", ["1,'a'"]], ["Object.keys", ["1"]], ["Object.getPrototypeOf", ["1"]], ["Object.getPrototypeOf", ["null"]], ["Function.prototype.toString", ["1"]], ["Function.prototype.call", [""]],
  ["Function.prototype.bind", [""]], ["Function.prototype.apply", [""]], ["Function", ["'return 1'"]], ["Function", ["'a','return a'"]], ["eval", ["'1+1'", "'var q=1;q'", "'throw 1'", "'('", "1", "{}"]], ["Error", ["'m'"]], ["TypeError", ["'m'"]], ["RegExp", ["'a'"]], ["Map", [""]], ["Promise", [""]], ["Proxy", [""]], ["Symbol.keyFor", ["1"]],
  ["Array", ["3"]], ["Array.of", ["1"]], ["Array.from", ["'ab'"]], ["Object", ["1"]], ["Object", [""]], ["Object.assign", ["1"]], ["Object.freeze", ["1"]], ["Object.isFrozen", ["1"]], ["structuredCloneNope", [""]],
];
for (const [fn, argsList] of natives) {
  for (const a of argsList) {
    expr(`sr.evaluate(${q(fn)})(${a})`);
    expr(`new (sr.evaluate(${q(fn)}))(${a})`);
    expr(`sr.evaluate(${q(`(function(){try{return ${fn}(${a})}catch(e){return e.name+": "+e.message}})()`)})`);
  }
  expr(`sr.evaluate(${q(fn)}).call(undefined)`);
  expr(`sr.evaluate(${q(fn)}).bind(null).name`);
  expr(`sr.evaluate(${q(fn)}).apply(null,[])`);
}

// ---- 11. Programas multi-statement no realm: declarações globais persistem entre evaluates.
const stmts = [
  ["var a=1", "a"], ["let a=1", "a"], ["const a=1", "a"], ["function a(){return 1}", "typeof a"], ["class a{}", "typeof a"], ["var a=1", "var a=2;a"], ["let a=1", "var a"], ["var a", "let a"], ["let a", "let a"], ["const a=1", "a=2"], ["let a=1", "a=2;a"], ["a=1", "typeof a"], ["a=1", "delete globalThis.a;typeof a"],
  ["var a=1", "delete globalThis.a;typeof a"], ["this.a=1", "typeof a"], ["globalThis.a=1", "a"], ["function f(){return this}", "f()===globalThis"], ["function f(){return typeof this}", "f()"], ["'use strict';function f(){return typeof this}", "f()"], ["'use strict'", "typeof this"], ["'use strict';var x", "typeof x"],
  ["var a=1;var b=2", "a+b"], ["let a=1;{let a=2}", "a"], ["let a=1", "{let a=2;a}"], ["var a=1", "{var a=2}a"], ["function f(){}", "function f(){return 2}f()"], ["function f(){return 1}", "var f=2;typeof f"], ["var f=1", "function f(){}typeof f"], ["let f", "function f(){}"], ["function f(){}", "let f"], ["class C{}", "class C{}"],
  ["Object.defineProperty(globalThis,'z',{value:1})", "z"], ["Object.defineProperty(globalThis,'z',{value:1})", "var z=2;z"], ["Object.defineProperty(globalThis,'z',{value:1})", "let z=2;z"], ["Object.defineProperty(globalThis,'z',{value:1,configurable:true})", "let z=2;z"], ["Object.defineProperty(globalThis,'z',{value:1})", "function z(){}"],
  ["Object.defineProperty(globalThis,'z',{value:1,writable:true,enumerable:true,configurable:false})", "function z(){}typeof z"], ["Object.defineProperty(globalThis,'z',{get(){return 4},configurable:true})", "z"], ["Object.defineProperty(globalThis,'z',{get(){return 4},configurable:true})", "z=5;z"], ["Object.defineProperty(globalThis,'z',{set(v){this.w=v},configurable:true})", "z=5;w"],
  ["Object.freeze(globalThis)", "var q=1;typeof q"], ["Object.freeze(globalThis)", "q=1"], ["Object.preventExtensions(globalThis)", "var q=1"], ["Object.preventExtensions(globalThis)", "function q(){}"], ["Object.preventExtensions(globalThis)", "let q=1;q"], ["Object.seal(globalThis)", "var q"],
  ["Object.setPrototypeOf(globalThis,{inherited:1})", "inherited"], ["Object.setPrototypeOf(globalThis,{inherited:1})", "typeof inherited"], ["Object.setPrototypeOf(globalThis,null)", "typeof Object"], ["Object.setPrototypeOf(globalThis,null)", "typeof toString"], ["Object.setPrototypeOf(globalThis,{get x(){return 3}})", "x"],
  ["Object.setPrototypeOf(globalThis,new Proxy({},{get(t,k){return 'p'+String(k)}}))", "foo"], ["Object.setPrototypeOf(globalThis,new Proxy({},{has(t,k){return k==='foo'}}))", "typeof foo"], ["var g=globalThis", "g===this"], ["var g=this", "g===globalThis"], ["var g=(0,eval)('this')", "g===globalThis"], ["var g=Function('return this')()", "g===globalThis"],
  ["var g=(function(){return this})()", "g===globalThis"], ["var g=(function(){'use strict';return this})()", "typeof g"], ["var g=(()=>this)()", "g===globalThis"], ["var o={m(){return this}}", "o.m()===o"], ["var o={m(){return this}};var m=o.m", "m()===globalThis"], ["var o={m(){'use strict';return this}};var m=o.m", "typeof m()"],
  ["var o={m(){return this}}", "(0,o.m)()===globalThis"], ["var o={m(){return this}}", "(o.m)()===o"], ["var o={m(){return this}}", "(o.m=o.m)()===globalThis"], ["var o={m(){return this}}", "(o?.m)()===o"], ["var o={m(){return this}}", "o?.m()===o"], ["var o={m(){return this}}", "o['m']()===o"], ["var o={m(){return this}}", "(1,o.m)()===globalThis"],
  ["var o={m(){return this}}", "new o.m()!==o"], ["var x=1", "eval('var x=2');x"], ["var x=1", "(0,eval)('var x=2');x"], ["var x=1", "(function(){eval('var x=2');return x})()+x"], ["var x=1", "(function(){'use strict';eval('var x=2');return x})()+x"], ["var x=1", "(function(){(0,eval)('var x=2')})();x"], ["var x=1", "new Function('var x=2;return x')()+x"],
  ["let x=1", "eval('var x=2')"], ["let x=1", "(0,eval)('var x=2')"], ["let x=1", "(0,eval)('let x=2;x')"], ["let x=1", "eval('let x=2;x')+x"], ["const x=1", "(0,eval)('x=2')"], ["var x=1", "(0,eval)('let x=2')"], ["var x=1", "(0,eval)('function x(){}');typeof x"], ["function x(){}", "(0,eval)('var x=2');x"], ["var x", "(0,eval)('let x')"], ["(0,eval)('var x=1')", "x"],
  ["(0,eval)('let x=1')", "typeof x"], ["(0,eval)('function f(){}')", "typeof f"], ["(0,eval)('class C{}')", "typeof C"], ["eval('var x=1')", "x"], ["eval('let x=1')", "typeof x"], ["eval('function f(){}')", "typeof f"], ["(function(){eval('var x=1')})()", "typeof x"], ["'use strict';eval('var x=1')", "typeof x"], ["'use strict';(0,eval)('var x=1')", "typeof x"],
  ["(0,eval)('\"use strict\";var x=1')", "typeof x"], ["(0,eval)('this.y=1')", "y"], ["(0,eval)('\"use strict\";this.y=1')", "y"], ["'use strict';(0,eval)('this.y=1')", "y"], ["'use strict';(0,eval)('y=1')", "typeof y"], ["'use strict';(0,eval)('\"use strict\";y=1')", "typeof y"], ["(0,eval)('\"use strict\";y=1')", "typeof y"],
];
for (const [a, b] of stmts) {
  expr(`(sr.evaluate(${q(a)}), sr.evaluate(${q(b)}))`);
  expr(`(sr.evaluate(${q(a + "\n")}), sr.evaluate(${q(b + ";")}))`);
  expr(`(sr.evaluate(${q('"use strict";' + a)}), sr.evaluate(${q(b)}))`);
  expr(`(sr.evaluate(${q(a)}), sr.evaluate(${q('"use strict";' + b)}))`);
  expr(`sr.evaluate(${q(a + ";" + b)})`);
}

// O importValue fica de fora: sob vm.runInThisContext o host rejeita o import dinâmico, e a mensagem é do host.

// ---- Execução, igual a gen-shadow-realm-more-golden.js.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
let baseText = "";
for (const program of knownPrograms("shadow_realm_source_bun.tsv", ["shadow_realm_bun.tsv", "shadow_realm_more_bun.tsv", "dynamic_fn_bun.tsv", "sloppy_syntax_bun.tsv"])) baseText += program + "\n\u0000\n";
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "shadow-source-golden-"));
const source_file = path.join(dir, "shadow_source.js");
const file = path.join(dir, "shadow_case.js");
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
let dup = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const source of programs) {
  const body = source.slice(PRELUDE.length);
  if (HOST.test(body)) continue;
  if (seen.has(source)) continue;
  seen.add(source);
  if (body.length > 24 && baseText.includes(body)) { dup++; continue; }
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\/|\bbun\b/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("shadow_realm_source", rows));
fs.rmSync(dir, { recursive: true, force: true });
