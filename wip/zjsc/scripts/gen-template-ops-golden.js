// Gera tests/golden/template_ops_bun.tsv: strings, template literals e operadores, medido no bun 1.4.2.
// Cobre tagged templates (strings.raw, cache por site, escapes inválidos em tagged e untagged), String.raw,
// concatenação, comparação relacional entre strings/números/objetos, `in`, `instanceof` com Symbol.hasInstance,
// typeof, void/delete/vírgula, `**` e sua associatividade, atribuição composta com getters que registram a ordem,
// optional chaining em chamadas e delete, nullish com atribuição, spread em chamadas/arrays/objetos e destructuring
// em parâmetros com defaults que têm efeito. Programas cuja expressão já aparece nos goldens vizinhos são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo (sem APIs de host), em paralelo, com limite de 6 s por filho.
// O prelúdio comum sai em tests/golden/template_ops.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-template-ops-golden.js > tests/golden/template_ops_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var L=[];function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?r+" |"+L.join(","):r}\n' +
  'var CR=(s,...v)=>S([s.length,Object.isFrozen(s),Object.isFrozen(s.raw),Array.isArray(s),Array.from(s,x=>x===undefined?"<u>":x),Array.from(s.raw),v]);\n';

const progs = []; // { strict, body }
const add = (body, strict) => progs.push({ body, strict: strict !== false });
const E = (expr, strict) => add(`return ${expr}`, strict);

// ---- 1. Tagged templates: cooked/raw de várias fontes (fonte crua entre crases).
const texts = [
  "", "a", "a\\nb", "a\\\\b", "\\t\\v\\f\\b\\r", "\\x41", "\\u0041", "\\u{41}", "\\u{1F600}", "\\0", "\\01", "\\1", "\\8", "\\9", "\\xZ", "\\x4", "\\u00", "\\u{", "\\u{}", "\\u{110000}",
  "\\u{zz}", "\\uD83D\\uDE00", "\\`", "\\${", "$", "$$", "$ {", "\\$", "a\\\nb", "a\\\r\nb", "a\nb", "a\r\nb", "a\rb", "a b", "a b", "\\u2028", "\\'", '\\"', "\\a", "\\z", "\\\\u0041", "\\\\x", "x\\u12", "\\0a", "\\00", "\\07", "\\08", "é", "😀", "\\ud800",
];
const subs = ["", "${1}", "${1}b${2}", "${'x'}${'y'}", "${[]}", "${{}}", "${null}${undefined}", "${1+1}"];
const tags = { CR: "CR", id: "((s)=>s)" };
for (const t of texts) {
  for (const sb of ["", "${1}", "${1}b${2}"]) {
    const lit = "`" + t + sb + "`";
    E(`CR${lit}`);
    E(`(0,eval)(${JSON.stringify("CR" + lit)})`);
    E(`(0,eval)(${JSON.stringify(lit)})`);
    E(`String.raw${lit}`);
  }
}
for (const t of texts) {
  E(`(()=>{try{return eval(${JSON.stringify("(s=>s)`" + t + "`")})[0]}catch(e){return e.name}})()`);
  E(`(()=>{try{return eval(${JSON.stringify("(s=>s.raw[0])`" + t + "`")})}catch(e){return e.name}})()`);
  E(`(()=>{try{return eval(${JSON.stringify("'use strict';(s=>s[0])`" + t + "`")})}catch(e){return e.name}})()`);
  E(`(()=>{try{return eval(${JSON.stringify("`" + t + "`")}).length}catch(e){return e.name+': '+e.message}})()`);
}
for (const sb of subs) for (const t of ["a", "\\u{", "x\\n"]) {
  E(`CR\`${t}${sb}${t}\``);
  E(`String.raw\`${t}${sb}${t}\``);
  E(`\`${t.includes("u{") ? "" : t}${sb}\``);
}

// ---- 2. Cache por site, identidade, tag como método, tag com this, new tag.
const siteCases = [
  "var f=()=>CR`a`;return f()===f()",
  "var f=()=>(s=>s)`a`;return f()===f()",
  "var g=s=>s;var f=()=>g`a`;var h=()=>g`a`;return f()===h()",
  "var g=s=>s;var a=[];for(var i=0;i<3;i++)a.push(g`x${i}`);return a[0]===a[1]&&a[1]===a[2]",
  "var g=s=>s;var a=[];for(var i=0;i<2;i++)a.push(g`x`);return [a[0]===a[1],a[0].raw===a[1].raw]",
  "var g=s=>s;return g`a`===g`a`",
  "var g=s=>s;return g`${1}`===g`${1}`",
  "var g=s=>s;var f=x=>g`a${x}b`;return f(1)===f(2)",
  "var g=s=>s;function mk(){return ()=>g`z`}return mk()()===mk()()",
  "var g=s=>s;var a=g`q`;try{a.push(1)}catch(e){return e.name}",
  "var g=s=>s;var a=g`q`;a[0]='z';return a[0]",
  "var g=s=>s;var a=g`q`;try{a.raw[0]='z'}catch(e){return e.name}",
  "var g=s=>s;var a=g`q`;return Object.getOwnPropertyDescriptor(a,'raw')",
  "var g=s=>s;var a=g`q${1}r`;return Reflect.ownKeys(a).map(String).join()",
  "var g=s=>s;var a=g`q${1}r`;return Reflect.ownKeys(a.raw).map(String).join()",
  "var g=s=>s;var a=g`q`;return [Object.isExtensible(a),Object.isSealed(a),Object.isExtensible(a.raw)]",
  "var g=s=>s;var a=g`q`;return Object.getPrototypeOf(a)===Array.prototype&&Object.getPrototypeOf(a.raw)===Array.prototype",
  "var g=s=>s;var a=g`q`;return Object.keys(a).join()+'|'+Object.getOwnPropertyNames(a).join()",
  "var g=s=>s;var a=g`q`;return JSON.stringify(Object.getOwnPropertyDescriptor(a,'0'))",
  "var g=s=>s;var a=g`q`;return JSON.stringify(Object.getOwnPropertyDescriptor(a,'length'))",
  "var g=s=>s;var a=g`q`;return JSON.stringify(Object.getOwnPropertyDescriptor(a.raw,'0'))",
  "var o={m(s){return this===o}};return o.m`a`",
  "var o={m(s){return this===o}};return (o.m)`a`",
  "var o={m(s){return this===o}};return (0,o.m)`a`",
  "var o={m(s){return typeof this}};return o['m']`a`",
  "var o={a:{m(s){return this===o.a}}};return o.a.m`a`",
  "var o={m(){return s=>this===o}};return o.m()`a`",
  "function f(s){return this}return typeof f`a`",
  "function f(s){return this===globalThis}return f`a`",
  "var f=function(s){'use strict';return this};return f`a`",
  "var f=s=>this;return typeof f`a`",
  "var f=(s,...v)=>v.length;return f`${1}${2}${3}`",
  "var f=(s,...v)=>v;return f`${1}${'a'}${null}${undefined}`",
  "var f=(s,...v)=>s.length;return f`${1}${2}`",
  "var f=(s,...v)=>s.length;return f``",
  "var f=(s,...v)=>[s.length,v.length];return f`${1}`",
  "var f=(s,...v)=>[s.length,v.length];return f`${1}${2}`",
  "var f=s=>s;return typeof f`a`",
  "var f=function(s){return arguments.length};return f`a${1}b`",
  "class C{static t(s){return this===C}}return C.t`x`",
  "class C{t(s){return this instanceof C}}return new C().t`x`",
  "var f=s=>s;return new f`a`",
  "var C=function(s){this.v=s[0]};return new C`a`",
  "var C=function(s){this.v=s[0]};return new C`a`.v",
  "var C=function(){return {k:1}};return new C`a`.k",
  "var C=class{constructor(s){this.v=s[0]}};return new C`ab`.v",
  "var f=()=>s=>s[0];return f()`ab`",
  "var f=s=>s=>s[0];return f`a``b`",
  "var f=s=>(...a)=>a[0][0];return f`a``b`",
  "var f=s=>s[0];return f`a`.length",
  "var f=s=>({m:t=>t[0]+s[0]});return f`a`.m`b`",
  "var f=s=>s[0];return f`a`+f`b`",
  "var f=s=>s[0];return `${f`a`}${f`b`}`",
  "var f=s=>s[0];return f`a${f`b`}c`",
  "var f=(s,...v)=>v[0];return f`${f`${1}`}`",
  "var f=(s,...v)=>v.join();return f`${[1,2]}${{}}`",
  "var f=(s,...v)=>v.join();return f`${1}${function(){}}`.length",
  "var f=(s,...v)=>v[0]();return f`${()=>7}`",
  "var f=(s,...v)=>v.length;return f`${...[]}`",
  "var t=s=>{throw new RangeError('t')};try{t`a`}catch(e){return e.name}",
  "var t=1;try{t`a`}catch(e){return e.name+': '+e.message}",
  "var t={};try{t`a`}catch(e){return e.name+': '+e.message}",
  "var t=null;try{t`a`}catch(e){return e.name+': '+e.message}",
  "var t;try{t`a`}catch(e){return e.name+': '+e.message}",
  "var o={};try{o.m`a`}catch(e){return e.name+': '+e.message}",
  "var o=null;try{o.m`a`}catch(e){return e.name+': '+e.message}",
  "var o={m:1};try{o.m`a`}catch(e){return e.name+': '+e.message}",
  "try{undefinedTag`a`}catch(e){return e.name+': '+e.message}",
  "var o={get m(){L.push('get');return s=>L.push('call')}};o.m`${L.push('arg')}`;return L.join()",
  "var o={get m(){L.push('get');return s=>1}};o.m`${L.push('a')}${L.push('b')}`;return 0",
  "var f=(s,...v)=>0;f`${L.push(1)}${L.push(2)}${L.push(3)}`;return L",
  "function t(){L.push('tag');return s=>L.push('call')}t()`${L.push('x')}`;return L",
  "var s=Symbol('q');var f=(a,...v)=>typeof v[0];try{return f`${s}`}catch(e){return e.name}",
  "var s=Symbol('q');try{return `${s}`}catch(e){return e.name+': '+e.message}",
  "var s=Symbol('q');try{return ''+s}catch(e){return e.name+': '+e.message}",
  "var s=Symbol('q');try{return s+''}catch(e){return e.name+': '+e.message}",
  "var s=Symbol('q');try{return `a${s}b`}catch(e){return e.name+': '+e.message}",
  "var s=Symbol('q');return String(s)+`${String(s)}`",
  "var s=Symbol('q');return `${s.toString()}${s.description}`",
  "return `${1n}${-0}${0.1+0.2}${1e21}${-1e-7}${NaN}${Infinity}`",
  "return `${[1,[2,3]]}${{}}${()=>1}${null}${undefined}${true}`",
  "return `${new Date(NaN)}`",
  "return `${{toString(){return 'ts'},valueOf(){return 'vo'}}}`",
  "return `${{valueOf(){return 'vo'}}}`",
  "return `${{[Symbol.toPrimitive](h){return h}}}`",
  "return `${{[Symbol.toPrimitive](h){return h}}}`+{[Symbol.toPrimitive](h){return h}}",
  "return `${{toString:null,valueOf(){return 'v'}}}`",
  "return `${{toString(){return {}},valueOf(){return 3}}}`",
  "try{return `${{toString(){return {}},valueOf(){return {}}}}`}catch(e){return e.name+': '+e.message}",
  "try{return `${Object.create(null)}`}catch(e){return e.name+': '+e.message}",
  "return `${L.push(1)}${L.push(2)}`+L.join('')",
  "return `${{toString(){L.push('a');return 'A'}}}${{toString(){L.push('b');return 'B'}}}`",
  "return `a${1}b${2}c`.length",
  "return `\\u{41}`+`\\x41`+`\\101`.length",
  "return `\n`.length+`\r\n`.length+`\r`.length",
  "return `a\\\nb`.length",
  "return `${`${`${1}`}`}`",
  "return `${`a${`b${'c'}`}`}`",
  "return `${1}${2}`+`${3}`",
  "return typeof `a`",
  "return `a`===`a`",
  "return `a`instanceof String",
  "return `abc`[1]+`abc`.length",
  "return `abc`.at(-1)",
  "return ({`a`:1})",
];
siteCases.forEach(b => add(b));
for (const b of siteCases) add(b, false);

// ---- 3. String.raw.
const rawCases = [
  "String.raw``", "String.raw`a`", "String.raw`a${1}b`", "String.raw`\\n${1}\\t`", "String.raw`\\u{`", "String.raw`\\xZ${1}\\u00`", "String.raw({raw:[]})", "String.raw({raw:['a']})",
  "String.raw({raw:['a','b']})", "String.raw({raw:['a','b']},1)", "String.raw({raw:['a','b','c']},1)", "String.raw({raw:['a','b','c']},1,2,3)", "String.raw({raw:'abc'},1,2)",
  "String.raw({raw:{length:2,0:'x',1:'y'}},'-')", "String.raw({raw:{length:3,0:'x'}},'-','+')", "String.raw({raw:{length:-1}})", "String.raw({raw:{length:'2',0:1,1:2}},0)",
  "String.raw({raw:{length:1.9,0:'q'}})", "String.raw({raw:{length:Infinity}})", "String.raw({raw:{length:NaN}})", "String.raw({raw:null})", "String.raw({raw:undefined})", "String.raw({})", "String.raw()",
  "String.raw(null)", "String.raw(undefined)", "String.raw(1)", "String.raw('abc')", "String.raw(true)", "String.raw([])", "String.raw({raw:1})", "String.raw({raw:true})", "String.raw({raw:Symbol()})",
  "String.raw({raw:['a','b']},{toString(){L.push('t');return 'T'}})", "String.raw({raw:[{toString(){L.push('r0');return 'R'},},'b']},1)",
  "String.raw({raw:['a',{toString(){L.push('r1');return 'R'}},'c']},{toString(){L.push('s0');return 'S'}},{toString(){L.push('s1');return 'U'}})",
  "String.raw({raw:[1,2,3]},'x','y')", "String.raw({raw:[null,undefined]},0)", "String.raw({raw:[Symbol()]})", "String.raw({raw:['a','b']},Symbol())", "String.raw({raw:['a','b']},1n)",
  "String.raw({raw:new Proxy(['a','b'],{get(t,k){L.push(String(k));return t[k]}})},1)",
  "String.raw({get raw(){L.push('raw');return ['p','q']}},0)", "String.raw({raw:{get length(){L.push('len');return 2},get 0(){L.push('i0');return 'a'},get 1(){L.push('i1');return 'b'}}},{toString(){L.push('s');return 's'}})",
  "String.raw`${1}${2}`", "String.raw`${1}${2}`.length", "String.raw`\\``", "String.raw`\\${x}`", "String.raw`$\\{`", "String.raw`\\\n`.length", "String.raw`\r\n`.length", "String.raw`\\\r\n`.length",
  "String.raw.length", "String.raw.name", "String.raw.call(null,{raw:['a']})", "String.raw.call(1,{raw:['b','c']},'-')", "new String.raw", "typeof String.raw`x`", "String.raw`a`===String.raw`a`",
  "String.raw`\\u{1F600}`", "String.raw`\\ud83d`", "String.raw`😀`.length", "String.raw`é\\é`", "(s=>s.raw)`\\u{zzz}`", "(s=>s[0])`\\u{zzz}`", "(s=>s.raw[0])`\\0\\00\\1`",
];
for (const r of rawCases) add(`try{return ${r}}catch(e){return e.name+': '+e.message}`);

// ---- 4. Operadores binários sobre uma grade de valores.
const vals = [
  '""', '"a"', '"b"', '"abc"', '"ABC"', '"10"', '"9"', '"2"', '" 1 "', '"1e3"', '"0x10"', '"\\u00e9"', '"\\ud800"', '"\\ud83d\\ude00"', '"\\uffff"', '"0"', '"-0"', '"Infinity"', '"NaN"',
  "0", "-0", "1", "2", "10", "1.5", "-1", "NaN", "Infinity", "-Infinity", "2**53", "1e21", "null", "undefined", "true", "false", "[]", "[1]", "[1,2]", "[[]]", "{}", "new Date(0)", "1n", "10n",
  "{valueOf(){L.push('v');return 1},toString(){L.push('s');return 'x'}}", "{toString(){L.push('s');return '5'}}", "{valueOf(){L.push('v');return 7}}",
  "{[Symbol.toPrimitive](h){L.push(h);return h==='number'?3:'str'}}", "{[Symbol.toPrimitive]:undefined,valueOf(){L.push('v');return 4}}", "Symbol('q')", "function(){}", "new String('a')", "new Number(3)",
  "{valueOf(){L.push('v');return {}},toString(){L.push('s');return {}}}", "Object.create(null)",
];
const binops = ["+", "<", ">", "<=", ">=", "==", "!=", "===", "-", "*", "%", "**", "&", "<<", ">>>", "in"];
// subconjuntos para limitar o total: grade completa só para + < <= ==; o resto com valores selecionados
const core = ["+", "<", "<=", "=="];
const fewIdx = [1, 3, 4, 5, 7, 11, 13, 19, 20, 22, 25, 26, 31, 32, 33, 34, 36, 38, 40, 41, 42, 46, 47, 50, 51];
const few = fewIdx.map(i => vals[i]).filter(Boolean);
for (const a of vals) for (const b of vals) {
  if (a.includes("Symbol(") && b.includes("Symbol(")) { /* ainda interessa */ }
  for (const op of core) add(`return (${a})${op}(${b})`);
}
for (const op of ["===", "-", "*", "**", "&", "<<", "in", ">", ">="]) {
  for (const a of few) for (const b of few) add(`return (${a})${op}(${b})`);
}

// ---- 5. Comparação relacional entre strings/números/objetos, escolha de hint.
const cmpStrs = ["a", "b", "aa", "ab", "B", "", " ", "10", "9", "\\u00e9", "e", "z", "\\ud800", "\\uffff", "\\ud83d\\ude00", "\\ue000", "1", "01"];
for (const a of cmpStrs) for (const b of cmpStrs) {
  add(`return ["${a}"<"${b}","${a}">"${b}","${a}"<="${b}","${a}">="${b}","${a}"=="${b}"].join()`);
}
const cmpNums = ["1", "2", "10", "'10'", "'9'", "'a'", "''", "' '", "null", "undefined", "true", "NaN", "-0", "0", "[]", "[2]", "({})", "1n", "'1'"];
for (const a of cmpNums) for (const b of cmpNums) add(`return [${a}<${b},${a}>${b},${a}<=${b},${a}>=${b},${a}==${b},${a}!=${b},${a}===${b}].join()`);
// Ordem de avaliação de ToPrimitive em comparações.
const po = (n, v) => `{valueOf(){L.push('${n}');return ${v}}}`;
for (const op of ["<", ">", "<=", ">="]) for (const [x, y] of [[1, 2], [2, 1], ["'a'", "'b'"], [1, "'a'"], ["undefined", 1]]) {
  add(`return (${po("l", x)})${op}(${po("r", y)})`);
  add(`return (${po("l", x)})${op}${y}`);
  add(`return ${x}${op}(${po("r", y)})`);
}

// ---- 6. `in`.
const inCases = [
  "'a' in {a:1}", "'a' in {}", "'toString' in {}", "'toString' in Object.create(null)", "0 in [1]", "1 in [1]", "'0' in [1]", "'length' in []", "'length' in 'abc'", "'x' in 1", "'x' in null", "'x' in undefined",
  "'x' in 'str'", "'x' in true", "'x' in Symbol()", "'x' in 1n", "Symbol.iterator in []", "Symbol.iterator in {}", "Symbol.iterator in 'a'", "null in {null:1}", "undefined in {undefined:1}", "true in {true:1}",
  "1 in {1:1}", "1.5 in {'1.5':1}", "-0 in {0:1}", "NaN in {NaN:1}", "[] in {'':1}", "[1,2] in {'1,2':1}", "({}) in {'[object Object]':1}", "'a' in function(){}", "'prototype' in function(){}", "'prototype' in (()=>1)",
  "'caller' in function(){}", "'length' in function(a,b){}", "'name' in class{}", "'x' in new Proxy({},{has(t,k){L.push('has '+String(k));return true}})", "'x' in new Proxy({},{has(){return 0}})",
  "'x' in new Proxy({},{has(){throw new RangeError('h')}})", "({toString(){L.push('ts');return 'k'}}) in {k:1}", "({[Symbol.toPrimitive](h){L.push(h);return 'k'}}) in {k:1}",
  "(L.push('l'),'a') in (L.push('r'),{a:1})", "'a' in (L.push('r'),null)", "(L.push('l'),'a') in 5", "Symbol('s') in {}", "'#x' in {}", "'a' in [].constructor", "'a' in Object.freeze({a:1})", "'__proto__' in {}",
  "'__proto__' in Object.create(null)", "'constructor' in 1", "0 in new String('a')", "1 in new String('a')", "'length' in new String('a')", "0 in new Uint8Array(1)", "1 in new Uint8Array(1)", "'1.5' in new Uint8Array(4)", "'-0' in new Uint8Array(4)",
  "0 in [,1]", "1 in [,1]", "0 in {get 0(){throw 1}}", "'a' in Object.create({a:1})", "'a' in Object.create({},{a:{value:1}})", "'x' in Reflect", "'x' in globalThis", "'globalThis' in globalThis", "'undefined' in globalThis",
  "'a' in {a:1} in {true:1}", "!'a' in {}", "!('a' in {})", "'a' in {} === false", "1 + 1 in [0,0,0]", "'a' in {a:1} ? 1 : 2", "typeof 'a' in {string:1}", "'a' in {a:1}&&'b' in {b:1}",
];
inCases.forEach(c => { E(`(${c})`); });
add("class A{#p=1;static h(o){return #p in o}}return [A.h(new A),A.h({}),A.h(Object.create(new A))]");
add("class A{#p=1;static h(o){return #p in o}}try{return A.h(1)}catch(e){return e.name+': '+e.message}");
add("class A{#p=1;static h(o){return #p in o}}try{return A.h(null)}catch(e){return e.name+': '+e.message}");
add("class A{#p=1;static h(o){return #p in o}}try{return A.h('s')}catch(e){return e.name+': '+e.message}");
add("class A{static #p=1;static h(o){return #p in o}}return [A.h(A),A.h(class extends A{}),A.h({})]");
add("class A{#m(){}static h(o){return #m in o}}return [A.h(new A),A.h({})]");
add("class A{get #g(){return 1}static h(o){return #g in o}}return [A.h(new A),A.h(A)]");
add("class A{#p=1;static h(o){return #p in o in {true:1}}}try{return A.h(new A)}catch(e){return e.name}");

// ---- 7. instanceof e Symbol.hasInstance.
const hi = [
  "({}) instanceof Object", "[] instanceof Array", "[] instanceof Object", "(()=>1) instanceof Function", "(()=>1) instanceof Object", "Object.create(null) instanceof Object", "1 instanceof Number", "new Number(1) instanceof Number",
  "'a' instanceof String", "new String('a') instanceof String", "Symbol() instanceof Symbol", "Object(Symbol()) instanceof Symbol", "1n instanceof BigInt", "Object(1n) instanceof BigInt", "null instanceof Object", "undefined instanceof Object",
  "({}) instanceof null", "({}) instanceof undefined", "({}) instanceof 1", "({}) instanceof {}", "({}) instanceof 'a'", "({}) instanceof Symbol()", "({}) instanceof (()=>1)", "({}) instanceof Math.max", "({}) instanceof class{}",
  "({}) instanceof function(){}.bind()", "({}) instanceof (function(){}.bind())", "(new (function F(){})) instanceof (function F(){}.bind())",
  "({}) instanceof {[Symbol.hasInstance](v){L.push(typeof v);return 1}}", "({}) instanceof {[Symbol.hasInstance](v){return 0}}", "({}) instanceof {[Symbol.hasInstance](v){return 'x'}}", "({}) instanceof {[Symbol.hasInstance]:1}",
  "({}) instanceof {[Symbol.hasInstance]:null}", "({}) instanceof {[Symbol.hasInstance]:undefined}", "({}) instanceof {[Symbol.hasInstance](){return this===undefined}}",
  "1 instanceof {[Symbol.hasInstance](v){L.push(v);return true}}", "null instanceof {[Symbol.hasInstance](v){L.push(v);return true}}",
  "({}) instanceof {[Symbol.hasInstance](){throw new RangeError('hi')}}", "({}) instanceof {get [Symbol.hasInstance](){L.push('get');return ()=>true}}",
  "(L.push('l'),{}) instanceof (L.push('r'),{[Symbol.hasInstance](){L.push('call');return true}})", "(L.push('l'),{}) instanceof (L.push('r'),null)",
  "Function.prototype[Symbol.hasInstance].call(Object,{})", "Function.prototype[Symbol.hasInstance].call(Object,1)", "Function.prototype[Symbol.hasInstance].call({},{})", "Function.prototype[Symbol.hasInstance].call(null,{})",
  "Function.prototype[Symbol.hasInstance].call(Array,[])", "Function.prototype[Symbol.hasInstance].call(function(){}.bind(),{})", "Function.prototype[Symbol.hasInstance].length", "Function.prototype[Symbol.hasInstance].name",
  "Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance).writable", "Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance).configurable", "Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance).enumerable",
  "Symbol.hasInstance in Function.prototype", "Symbol.hasInstance in Object", "Object.hasOwn(Object,Symbol.hasInstance)", "Symbol.hasInstance.toString()", "Symbol.hasInstance.description",
  "(function(){function F(){}F.prototype=1;return {} instanceof F})()", "(function(){function F(){}F.prototype=null;return {} instanceof F})()", "(function(){function F(){}F.prototype=undefined;return {} instanceof F})()",
  "(function(){function F(){}F.prototype=1;return 1 instanceof F})()", "(function(){function F(){}var o=new F;F.prototype={};return o instanceof F})()", "(function(){function F(){}var o=new F;Object.setPrototypeOf(o,null);return o instanceof F})()",
  "(function(){function F(){}function G(){}G.prototype=Object.create(F.prototype);return new G instanceof F})()", "(function(){class A{}class B extends A{}return [new B instanceof A,new A instanceof B]})()",
  "(function(){class A{static [Symbol.hasInstance](v){return v===1}}return [1 instanceof A,2 instanceof A,new A instanceof A]})()", "(function(){class A{static [Symbol.hasInstance](v){return true}}class B extends A{}return [1 instanceof B,{} instanceof B]})()",
  "(function(){var p=new Proxy(function(){},{get(t,k){L.push(String(k));return t[k]}});return {} instanceof p})()", "(function(){var p=new Proxy({},{get(t,k){L.push(String(k));return t[k]}});try{return {} instanceof p}catch(e){return e.name}})()",
  "(function(){var o=new Proxy({},{getPrototypeOf(){L.push('gpo');return Array.prototype}});return o instanceof Array})()", "(function(){var f=function(){};f.prototype=new Proxy({},{});return {} instanceof f})()",
  "(function(){var b=function(){}.bind();return {} instanceof b})()", "(function(){function F(){}var b=F.bind();return [new F instanceof b,new b instanceof F,new b instanceof b]})()",
  "(function(){var f=Object.defineProperty(function(){},'prototype',{get(){L.push('p');return {}}});return {} instanceof f})()", "(function(){var f=Object.defineProperty(function(){},'prototype',{get(){L.push('p');return {}}});return 1 instanceof f})()",
  "!({} instanceof Object)", "!{} instanceof Object", "({} instanceof Object) instanceof Object", "[] instanceof Array instanceof Object", "typeof {} instanceof Object", "new Date instanceof Date", "/x/ instanceof RegExp",
  "new Error instanceof Error", "new TypeError instanceof Error", "new Error instanceof TypeError", "Promise.resolve() instanceof Promise", "new Map instanceof Map", "new Map instanceof Object", "function*(){} instanceof Function",
  "(function*(){})() instanceof Object", "(async()=>{}) instanceof Function", "Object instanceof Function", "Function instanceof Object", "Function instanceof Function", "Object instanceof Object",
  "Math instanceof Object", "JSON instanceof Object", "Reflect instanceof Object", "globalThis instanceof Object", "(function(){return arguments})() instanceof Object", "Symbol.prototype instanceof Symbol",
  "new Proxy([],{}) instanceof Array", "new Proxy(function(){},{}) instanceof Function", "Object.create(Array.prototype) instanceof Array", "Array.isArray(Object.create(Array.prototype))",
];
hi.forEach(c => E(`(${c})`));

// ---- 8. typeof.
const typeofs = [
  "undefined", "null", "true", "0", "-0", "NaN", "''", "'a'", "1n", "Symbol()", "Symbol.iterator", "{}", "[]", "()=>1", "function(){}", "class{}", "async()=>{}", "function*(){}", "async function*(){}", "new Date", "/x/", "new Map",
  "new Proxy({},{})", "new Proxy(function(){},{})", "new Proxy(()=>{},{})", "new Proxy(class{},{})", "new Proxy([],{})", "Math", "JSON", "Reflect", "globalThis", "Object", "Object.prototype", "Function.prototype", "Array.prototype",
  "String.prototype", "Symbol.prototype", "Number.prototype", "Boolean.prototype", "BigInt.prototype", "Date.prototype", "RegExp.prototype", "Error.prototype", "Promise.prototype", "Map.prototype", "new String('a')", "new Number(1)",
  "new Boolean(false)", "Object(1n)", "Object(Symbol())", "function(){}.bind()", "Math.max", "Symbol", "BigInt", "parseInt", "eval", "Function", "(function(){return arguments})()", "(function(){return typeof arguments})()",
  "typeof 1", "typeof typeof 1", "typeof undeclaredVariable", "typeof undeclaredVariable.x", "typeof void 0", "typeof null", "typeof typeof undeclaredVariable", "typeof (()=>{})()", "typeof (1,undeclaredVariable)",
  "typeof new (class{})", "typeof new Function", "typeof Function()", "typeof Object('a')", "typeof Object(null)", "typeof new Object(1)", "typeof String(1)", "typeof new String(1)", "typeof Number('1')", "typeof BigInt(1)",
  "typeof Symbol.for('a')", "typeof [].values()", "typeof [][Symbol.iterator]", "typeof Array.from", "typeof Array.prototype[Symbol.unscopables]", "typeof Symbol.toPrimitive", "typeof globalThis.undeclaredVariable",
  "typeof `a`", "typeof `${1}`", "typeof -'1'", "typeof +'1'", "typeof ~1", "typeof !1", "typeof (1+1n === 2)", "typeof (1n+1n)", "typeof (1,'a')", "typeof ('a' in {})", "typeof (1<2)", "typeof ({}).x", "typeof ({}).toString",
  "typeof Intl", "typeof WebAssembly", "typeof SharedArrayBuffer", "typeof Atomics", "typeof WeakRef", "typeof FinalizationRegistry", "typeof structuredClone", "typeof queueMicrotask", "typeof setTimeout", "typeof console",
  "typeof document", "typeof window", "typeof process", "typeof require", "typeof module", "typeof exports", "typeof this", "typeof new.target", "typeof arguments",
];
typeofs.forEach(t => {
  const bare = /^(typeof |undefined$|null$|true$|0$|-0$|NaN$|''$|'a'$|1n$|Symbol\(\)$|Symbol\.iterator$|Math$|JSON$|Reflect$|globalThis$|Object$|Symbol$|BigInt$|parseInt$|eval$|Function$)/.test(t);
  const expr = t.startsWith("typeof ") ? t : "typeof " + (bare ? t : "(" + t + ")");
  add(`return ${expr}`); add(`return ${expr}`, false);
});
add("try{return typeof tdz;let tdz=1}catch(e){return e.name+': '+e.message}");
add("try{return typeof tdz;const tdz=1}catch(e){return e.name+': '+e.message}");
add("try{return typeof tdz;class tdz{}}catch(e){return e.name+': '+e.message}");
add("try{return typeof C;class C{}}catch(e){return e.name}");
add("{let x=typeof y;var r=x;let y}return r");
add("return typeof f;function f(){}");
add("return typeof v;var v=1");
add("class C{static m(){return typeof C}}return C.m()");
add("var C=class D{m(){return typeof D}};return new C().m()");
add("var f=function g(){return typeof g};return f()");
add("return (function(){return typeof arguments})()");
add("return (()=>typeof arguments)()");
add("return (function(a=typeof b,b){return a})()");
add("return (function(a=typeof a){return a})()");
add("try{return (function(a=a){return a})()}catch(e){return e.name+': '+e.message}");
add("try{return (function(a=b,b){return a})()}catch(e){return e.name+': '+e.message}");
add("var o={get x(){L.push('get');return 1}};return [typeof o.x,L.length]");
add("var o={get x(){throw new RangeError('x')}};try{return typeof o.x}catch(e){return e.name}");
add("var p=new Proxy({},{get(t,k){L.push(String(k));return 1}});return [typeof p.a,typeof p[Symbol.iterator],L.join()]");
add("return typeof new Proxy(function(){},{apply(){return 1}})()");
add("return typeof Object.assign(()=>{},{x:1})");
add("var s=Symbol();return [typeof s,typeof Object(s),typeof s.description,typeof s.toString()]");
add("return [typeof 1n,typeof Object(1n),typeof (1n*2n),typeof BigInt.asUintN(8,1n)]");

// ---- 9. void / delete / vírgula.
const vd = [
  "void 0", "void 'a'", "void {}", "void (L.push(1))", "void L.push(1),L.length", "[void 0]", "[void 0].length", "void void 0", "typeof void 0", "void 0 === undefined", "(void 0)?.x", "void 0 ?? 'd'", "(void 0)||'x'",
  "(1,2,3)", "(L.push('a'),L.push('b'),L.length)", "(1,2,3).toString()", "((1,2),3)", "[(1,2),(3,4)]", "(0,1)?.toString()", "(0,undefined)?.x", "(L.push(1),undefined)??5", "(1,{a:1}).a", "(1,function(){return this})()===undefined",
  "(0,Math.max)(1,2)", "(0,Math).max(1,2)", "(Math.max)(1,2)", "(0,eval)('1+1')", "((0,eval))('1+1')", "eval('1+1')", "var o={f(){return this}};(0,o.f)()===globalThis||(0,o.f)()===undefined", "var o={f(){return this===o}};[(o.f)(),(0,o.f)(),(o.f=o.f)()]",
  "var a=1,b=(a++,a++,a);[a,b]", "var i=0;(i++,i++,i--);i", "for(var i=0,j=10;i<j;i++,j--);[i,j]", "var x=(1,2)?3:4;x", "(L.push(1),L.push(2))+(L.push(3),L.push(4))", "((a,b)=>a+b)((1,2),(3,4))", "[1,2,3].map((x)=>(x,x*2))",
  "delete 1", "delete 'a'", "delete {}", "delete undefined", "delete null", "delete NaN", "delete Infinity", "delete void 0", "delete (1,2)", "delete ({a:1}).a", "delete ({a:1})['a']", "delete ({a:1})['b']", "delete [1,2][0]", "delete [1,2].length",
  "delete 'abc'.length", "delete 'abc'[0]", "delete 'abc'[5]", "delete 'abc'.x", "delete Math.PI", "delete Math.max", "delete Object.prototype", "delete Object.prototype.toString", "delete globalThis.undefined", "delete globalThis.NaN", "delete globalThis.globalThis",
  "delete Array.prototype.length", "delete (function(){}).prototype", "delete (function(){}).name", "delete (function(){}).length", "delete (()=>1).prototype", "delete (class{}).prototype", "delete (class{static x=1}).x",
  "delete Symbol.iterator", "delete Symbol.prototype", "delete [].constructor", "delete [][Symbol.iterator]", "delete [][Symbol.unscopables]", "delete Reflect[Symbol.toStringTag]", "delete Math[Symbol.toStringTag]", "delete JSON[Symbol.toStringTag]",
  "delete new Proxy({a:1},{deleteProperty(t,k){L.push('del '+k);return true}}).a", "delete new Proxy({a:1},{deleteProperty(t,k){L.push('del '+k);return false}}).a", "delete new Proxy({a:1},{}).a", "delete new Proxy({a:1},{deleteProperty(){throw new RangeError('d')}}).a",
  "delete Object.freeze({a:1}).a", "delete Object.seal({a:1}).a", "delete Object.preventExtensions({a:1}).a", "delete Object.freeze([1])[0]", "delete Object.freeze([1]).length", "delete new Uint8Array(2)[0]", "delete new Uint8Array(2)[5]", "delete new Uint8Array(2)['-0']", "delete new Uint8Array(2).length",
  "delete new Uint8Array(2).x", "delete Object.create({a:1}).a", "delete Object.create(Object.freeze({a:1})).a", "delete (function(){return arguments})(1,2)[0]", "delete (function(){return arguments})(1,2).length", "delete (function(){return arguments})(1,2).callee",
  "delete (function(){'use strict';return arguments})(1,2).length", "delete (function(a){delete arguments[0];return a})(5)", "delete (function(a){delete arguments[0];arguments[0]=9;return a})(5)", "delete new String('ab')[0]", "delete new String('ab').length", "delete new String('ab')[2]",
  "delete (L.push('obj'),{})[(L.push('key'),'k')]", "delete (L.push('obj'),{})[{toString(){L.push('ts');return 'k'}}]", "delete [][{toString(){L.push('ts');return 0}}]", "delete null?.x", "delete undefined?.x", "delete ({a:1})?.a", "delete ({a:{b:1}}).a?.b",
  "delete ({a:null}).a?.b", "delete ({a:null}).a?.b.c", "delete ({a:null}).a?.[L.push('k')]", "delete ({a:{}}).a?.[L.push('k')]", "delete (L.push('x'),null)?.[L.push('k')]", "delete ({a:{b:{c:1}}}).a?.b?.c", "delete ({a:{b:{c:1}}}).x?.b?.c",
  "delete ({a:1}).a?.b", "delete 'abc'?.length", "delete (1)?.x", "var o={a:1,b:2};[delete o.a,delete o.a,Object.keys(o).join()]", "var o={a:1};[delete o?.a,'a' in o]", "var a=[1,2,3];[delete a[1],a.length,1 in a,S(a)]", "var a=[1,2,3];[delete a[5],a.length]",
  "var a=[1,2,3];a.length=1;[delete a[0],a.length]", "var o=Object.create({a:1});o.a=2;[delete o.a,o.a,delete o.a,o.a]", "var o={};Object.defineProperty(o,'a',{value:1,configurable:true});[delete o.a,'a' in o]", "var o={};Object.defineProperty(o,'a',{value:1});[delete o.a,'a' in o]",
  "var o={get a(){return 1}};[delete o.a,'a' in o]", "var s=Symbol();var o={[s]:1};[delete o[s],Object.getOwnPropertySymbols(o).length]", "var o={1:1,'01':2};[delete o[1],delete o['1'],delete o['01'],Object.keys(o).join()]",
  "var o={a:1};delete o.a;o.a=2;Object.keys(o).join()", "var o={a:1,b:2,c:3};delete o.b;o.b=4;Object.keys(o).join()", "var o={a:1,b:2,c:3};delete o.a;delete o.c;Object.keys(o).join()", "var o={};[delete o.x,delete o['x'],delete o[0],delete o[Symbol()]]",
  "var o={a:{b:1}};[delete o.a.b,delete o.a,delete o.a.b]", "var x=1;delete x", "var f=function(){};delete f", "delete globalThis.eval", "globalThis.zz=1;[delete globalThis.zz,typeof zz]", "globalThis.zz=1;delete zz;typeof zz",
];
vd.forEach(c => { add(c.includes(";") ? c.replace(/;([^;]*)$/, ";return ($1)") : `return (${c})`); add(c.includes(";") ? c.replace(/;([^;]*)$/, ";return ($1)") : `return (${c})`, false); });
add("'use strict';var x=1;try{eval('delete x')}catch(e){return e.name+': '+e.message}", false);
add("try{return eval(\"'use strict';delete x\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';var x;delete x\")}catch(e){return e.name+': '+e.message}");
add("try{return eval('delete (x)')}catch(e){return e.name+': '+e.message}", false);
add("try{return eval(\"'use strict';delete ((x))\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete x.y\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete this.zzz\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete Object.prototype\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete 'abc'.length\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete 'abc'[0]\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete Object.freeze({a:1}).a\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete Object.freeze([1])[0]\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete new Uint8Array(1)[0]\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete new Proxy({a:1},{deleteProperty(){return false}}).a\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete undefined\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete globalThis.undefined\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete null.x\")}catch(e){return e.name+': '+e.message}");
add("try{return eval(\"'use strict';delete (void 0).x\")}catch(e){return e.name+': '+e.message}");
add("try{return eval('class A{#p=1;m(){return delete this.#p}}')}catch(e){return e.name+': '+e.message}");
add("try{return eval('class A{#p=1;m(){return delete (this.#p)}}')}catch(e){return e.name+': '+e.message}");
add("try{return eval('class A{#p=1;m(){return delete this?.#p}}')}catch(e){return e.name+': '+e.message}");
add("class A{m(){return delete super.x}}try{return new A().m()}catch(e){return e.name+': '+e.message}");
add("class A{m(){return delete super[(L.push('k'),'x')]}}try{return new A().m()}catch(e){return e.name+': '+e.message+L}");
add("var o={__proto__:{x:1},m(){return delete super.x}};try{return o.m()}catch(e){return e.name+': '+e.message}", false);

// ---- 10. Exponenciação.
const bases = ["2", "-2", "0", "-0", "1", "0.5", "NaN", "Infinity", "-Infinity", "'3'", "null", "undefined", "true", "[]", "[2]", "1n", "2n", "-1n", "0n"];
for (const a of bases) for (const b of bases) add(`return (${a})**(${b})`);
for (const a of ["2", "3", "-1", "0.5"]) for (const b of ["2", "3", "0", "-1"]) for (const c of ["2", "0", "-1", "0.5"]) {
  add(`return ${a}**${b}**${c}`); add(`return (${a}**${b})**${c}`); add(`return ${a}**(${b}**${c})`);
}
for (const x of ["-(2)**2", "(-2)**2", "-(2**2)", "+2**2", "!2**2", "~2**2", "typeof 2**2", "void 2**2", "delete 2**2", "await 2**2", "-2**2", "2**-2", "2**+2", "2**-(-2)", "2**~1", "2**!1", "2** typeof 1", "(-2)**-2", "2**3**-1", "-(2)**-2"]) {
  add(`try{return eval(${JSON.stringify(x)})}catch(e){return e.name+': '+e.message}`);
}
for (const x of ["var a=2;a**=3;a", "var a=2;a**=3**2;a", "var a=2;a**=a**=2;a", "var a=-2;a**=2;a", "var a=2n;a**=3n;a", "var a=2n;a**=-1n;a", "var a=2;a**='2';a", "var a=2;a**=undefined;a", "var a='2';a**=2;a", "var a=null;a**=2;a",
  "var a=2;a**=0;a", "var a=1;a**=Infinity;a", "var a=-1;a**=Infinity;a", "var a=0;a**=-1;a", "var a=-0;a**=-1;a", "var a=-0;a**=-2;a", "var a=-0;a**=3;a", "var a=NaN;a**=0;a", "var o={a:2};o.a**=3;o.a", "var o={a:2};o['a']**=o.a;o.a",
  "var a=[2];a[0]**=3;a[0]", "var a=2;(a**=2)**2", "var a=2;[a**=2,a]", "var a=2;(a)**=2;a", "(2**53)**2", "2**1023*2", "2**1024", "2**-1074", "2**-1075", "(-8)**(1/3)", "8**(1/3)", "10**21", "10**-7", "10**15+0.1", "0.1**2", "Math.pow(0.1,2)===0.1**2",
  "1**Infinity", "1**NaN", "(-1)**Infinity", "(-1)**0.5", "0**0", "NaN**0", "(0)**(-0)", "(-0)**0", "Infinity**0", "Infinity**-1", "(-Infinity)**3", "(-Infinity)**2", "(-Infinity)**-3", "(-Infinity)**-2", "2**0.5", "4**0.5", "9**0.5", "27**(1/3)",
  "2n**64n", "(-2n)**3n", "0n**0n", "2n**0n", "3n**-1n", "2n**2", "2**2n", "(2n**100n).toString()", "10n**30n", "(-1n)**(2n**64n)", "1n**(2n**64n)", "0n**(2n**64n)",
  "var L2=[];var o={valueOf(){L.push('l');return 2}};var p={valueOf(){L.push('r');return 3}};o**p", "var o={valueOf(){L.push('l');return 2}};var p={valueOf(){L.push('r');return 3}};p**o",
  "(L.push('a'),2)**(L.push('b'),3)**(L.push('c'),2)", "var o={valueOf(){L.push('l');return 2n}};o**1", "var a=2;a**=(L.push('r'),2);a"]) {
  add(x.includes(";") ? `try{${x.replace(/;([^;]*)$/, ";return ($1)")}}catch(e){return e.name+': '+e.message}` : `try{return (${x})}catch(e){return e.name+': '+e.message}`);
}

// ---- 11. Atribuição composta com getters/setters que registram a ordem.
const ops = ["+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=", "??="];
const inits = ["0", "1", "null", "undefined", "'s'", "''", "NaN", "2n", "({valueOf(){L.push('iv');return 5}})"];
const rhs = ["(L.push('rhs'),3)", "'z'", "undefined", "3n", "({valueOf(){L.push('rv');return 2}})"];
const targets = [
  (i, op, r) => `var x=${i};x ${op} ${r};return x`,
  (i, op, r) => `var o={get p(){L.push('get');return ${i}},set p(v){L.push('set '+S(v))}};o.p ${op} ${r};return L.length`,
  (i, op, r) => `var o={get p(){L.push('get');return ${i}},set p(v){L.push('set '+S(v))}};return (o.p ${op} ${r})`,
  (i, op, r) => `var o={get p(){L.push('get');return ${i}}};try{o.p ${op} ${r}}catch(e){L.push(e.name)}return 0`,
  (i, op, r) => `var o={set p(v){L.push('set '+S(v))}};o.p ${op} ${r};return 0`,
  (i, op, r) => `var o={p:${i}};var k=()=>(L.push('key'),'p');(L.push('obj'),o)[k()] ${op} ${r};return o.p`,
  (i, op, r) => `var a=[${i}];var k={toString(){L.push('ts');return '0'}};a[k] ${op} ${r};return a[0]`,
  (i, op, r) => `var o=Object.freeze({p:${i}});o.p ${op} ${r};return o.p`,
  (i, op, r) => `'use strict';var o=Object.freeze({p:${i}});try{o.p ${op} ${r}}catch(e){return e.name+': '+e.message}return o.p`,
  (i, op, r) => `var p=new Proxy({p:${i}},{get(t,k){L.push('get '+k);return t[k]},set(t,k,v){L.push('set '+k+'='+S(v));t[k]=v;return true}});p.p ${op} ${r};return 0`,
  (i, op, r) => `const c=${i};try{c ${op} ${r}}catch(e){return e.name+': '+e.message}return c`,
  (i, op, r) => `var o={__proto__:{set p(v){L.push('psetter')}},p0:1};o.p ${op} ${r};return Object.keys(o).join()`,
  (i, op, r) => `class A{get p(){L.push('super get');return ${i}}set p(v){L.push('super set '+S(v))}}class B extends A{m(){super.p ${op} ${r}}}new B().m();return 0`,
  (i, op, r) => `var s='abc';var o={p:${i}};o.p ${op} s.length;return o.p`,
  (i, op, r) => `var o=null;try{o.p ${op} ${r}}catch(e){return e.name+': '+e.message}`,
  (i, op, r) => `var o=null;try{o[(L.push('k'),'p')] ${op} ${r}}catch(e){return e.name+': '+e.message}`,
  (i, op, r) => `var u;try{u.p ${op} ${r}}catch(e){return e.name+': '+e.message}`,
  (i, op, r) => `try{undeclaredZ ${op} ${r}}catch(e){return e.name+': '+e.message}return typeof undeclaredZ`,
  (i, op, r) => `'use strict';try{undeclaredZ ${op} ${r}}catch(e){return e.name+': '+e.message}return typeof undeclaredZ`,
  (i, op, r) => `var x=${i};var f=()=>{x ${op} (x=9,${r})};f();return x`,
];
let ti = 0;
for (const t of targets) {
  ti++;
  for (const op of ops) for (const i of inits) {
    // rhs escolhida por rotação para controlar o total
    const r = rhs[(ops.indexOf(op) + inits.indexOf(i) + ti) % rhs.length];
    add(t(i, op, r), !/^'use strict'/.test(t(i, op, r)) ? true : true);
  }
}
for (const op of ops) {
  add(`var o={get p(){L.push('get');return 1},set p(v){L.push('set')}};o.p ${op} (L.push('rhs'),2);return 0`);
  add(`var o={get p(){L.push('get');return null},set p(v){L.push('set')}};o.p ${op} (L.push('rhs'),2);return 0`);
  add(`var o={get p(){L.push('get');return 0},set p(v){L.push('set')}};o.p ${op} (L.push('rhs'),2);return 0`);
  add(`var a=1,b=2,c=3;a ${op} b ${op} c;return [a,b,c]`);
  add(`var a=null,b=null,c=3;a ${op} b ${op} c;return [a,b,c]`);
  add(`var x=1;(x) ${op} 2;return x`);
  add(`try{return eval('1 ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('(1) ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('x+1 ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('f() ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('this ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('[a] ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('({a}) ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('a?.b ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('new.target ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('a++ ${op} 2')}catch(e){return e.name+': '+e.message}`);
  add(`try{return eval('eval ${op} 2')}catch(e){return e.name+': '+e.message}`, true);
  add(`try{return eval("'use strict';eval ${op} 2")}catch(e){return e.name+': '+e.message}`, false);
  add(`try{return eval("'use strict';arguments ${op} 2")}catch(e){return e.name+': '+e.message}`, false);
  add(`var f=function(){};try{f.name ${op} 'q'}catch(e){return e.name}return f.name`);
  add(`var s='ab';try{s.length ${op} 5}catch(e){return e.name+': '+e.message}return s.length`);
  add(`var s='ab';try{s[0] ${op} 'z'}catch(e){return e.name+': '+e.message}return s`);
  add(`var a=[1];a.length ${op} 3;return S(a)`);
}

// ---- 12. Optional chaining.
const ocBase = [
  "null?.a", "undefined?.a", "0?.a", "''?.a", "false?.a", "NaN?.a", "({})?.a", "({a:1})?.a", "({a:null})?.a?.b", "({a:{b:2}})?.a?.b", "null?.[0]", "null?.[(L.push('k'),0)]", "({})?.[(L.push('k'),'a')]", "null?.()", "null?.(L.push('arg'))",
  "undefined?.(1,2)", "(()=>1)?.()", "(void 0)?.()", "({f(){return this===o}}).f?.()", "var o={f(){return this===o}};o.f?.()", "var o={f(){return this===o}};o?.f()", "var o={f(){return this===o}};o?.f?.()", "var o={f(){return this===o}};(o?.f)()",
  "var o={f(){return this===o}};(o.f)?.()", "var o={f(){return this===o}};(o?.f)?.()", "var o={f(){return this===o}};o?.['f']()", "var o={f(){return this===o}};(o?.['f'])()", "var o={};o.f?.()", "var o={};o?.f?.()", "var o={f:null};o.f?.()",
  "var o={f:1};try{o.f?.()}catch(e){e.name+': '+e.message}", "var o={};try{o.f()}catch(e){e.name+': '+e.message}", "var o={};try{o?.f()}catch(e){e.name+': '+e.message}", "var o={};try{o.a?.b.c}catch(e){e.name}",
  "var o={a:{}};try{o.a?.b.c}catch(e){e.name+': '+e.message}", "var o={a:null};o.a?.b.c.d.e", "var o={a:null};o.a?.b().c().d()", "var o={a:null};o.a?.[L.push('x')].b[L.push('y')]", "var o={a:null};o.a?.b[L.push('y')]", "var o=null;o?.a.b.c(L.push('arg'))",
  "var o=null;o?.a[L.push('k')](L.push('arg'))", "var o=null;(o?.a).b", "var o=null;try{(o?.a).b}catch(e){e.name+': '+e.message}", "var o=null;try{(o?.a.b).c}catch(e){e.name+': '+e.message}", "var o=null;(o?.a)?.b", "var o={a:null};try{(o.a?.b).c}catch(e){e.name}",
  "var o={a:1};o?.a++", "var o={get a(){L.push('get');return null}};o?.a?.b", "var o={get a(){L.push('get');return null}};o.a?.b;o.a?.b", "var o=null;o?.a=1", "var o=null;o?.[0]=1", "var o=null;o?.a`x`", "var o=null;new o?.a", "var o=null;o?.a.b=1", "var o=null;o?.a++",
  "var o=null;delete o?.a", "var o={a:1};delete o?.a", "var o={a:1};[delete o?.a,o.a]", "var o={a:{b:1}};[delete o?.a?.b,o.a.b]", "var o={a:null};[delete o.a?.b,'a' in o]", "var o={a:1};[delete o?.['a'],'a' in o]", "var o=null;[delete o?.[L.push('k')],L.length]",
  "var o={a:1};o?.a ?? L.push('rhs')", "var o=null;o?.a ?? 'dflt'", "var o={a:0};o?.a ?? 'dflt'", "var o={a:0};o?.a || 'dflt'", "var o=null;(o?.a)+1", "var o=null;o?.a+1", "var o=null;o?.a+L.push('rhs')", "var o=null;o?.a===undefined", "var o=null;typeof o?.a",
  "var o=null;typeof o?.a.b.c", "var o=null;void o?.a", "var o=null;!o?.a", "var o=null;-o?.a", "var o=null;o?.a?'t':'f'", "var o=null;[o?.a,o?.b].length", "var o=null;({x:o?.a})", "var o=null;`${o?.a}`", "var o=null;o?.a?.[L.push(1)]?.(L.push(2))",
  "var o={a:{b(){return this===o.a}}};[o?.a.b(),o.a?.b(),o?.a?.b(),(o?.a.b)(),(o.a?.b)(),o?.['a']?.['b']()]", "var o={a:{b(){return this===o.a}}};[o?.a.b?.(),(o?.a.b)?.()]", "(null)?.a", "(undefined)?.[0]", "(function(){})?.name", "(class{})?.name", "'abc'?.length", "'abc'?.[1]", "1?.toString()", "1?.5:2", "true?.5:2",
  "var x=1;x?.5:2", "var x=null;x?.5:2", "var a=[1];a?.[0]", "var a=null;a?.[0]", "var a=[];a?.length", "var a=[];a?.at?.(0)", "var a=[];a?.at?.(0)?.x", "[].at?.(-1)?.x", "Math?.max?.(1,2)", "Math?.nope?.(1,2)", "globalThis?.Math?.PI", "globalThis?.nope?.PI", "Symbol?.iterator?.toString()", "JSON?.parse?.('[1]')?.[0]",
  "var p=new Proxy({},{get(t,k){L.push(String(k));return undefined}});p?.a?.b", "var p=new Proxy({},{has(){L.push('has');return true},get(t,k){L.push(String(k));return null}});p?.a?.b", "var p=new Proxy(function(){},{apply(){L.push('apply');return null}});p?.()?.a",
  "function f(){return null}f()?.a", "function f(){L.push('f');return null}f()?.a(f())", "function f(){L.push('f');return {a(){L.push('a');return this}}}f()?.a()?.a()", "async function f(){}f()?.then?.(()=>{})?.constructor===Promise", "function*g(){yield 1}g()?.next?.()?.value", "class A{static m(){return 1}}A?.m?.()", "class A{#p=1;static h(o){return o?.#p}}[A.h(new A),A.h(null)]",
  "class A{#p=1;static h(o){return o?.#p}}try{A.h({})}catch(e){e.name+': '+e.message}", "class A{#m(){return 1}static h(o){return o?.#m()}}[A.h(new A),A.h(undefined)]", "class A{x(){return 1}}class B extends A{m(){return super.x?.()}}new B().m()", "class A{}class B extends A{m(){return super.nope?.()}}new B().m()",
  "var f=null;f?.\n(1)", "var f=null;f\n?.()", "var o=null;o\n?.a", "var o={a:1};o?.\na", "var o=null;o?.[\n0\n]", "new.target?.x", "this?.x", "(function(){return this?.x})()", "(function(){'use strict';return this?.x})()", "arguments?.length",
  "var s=Symbol();var o={[s]:1};o?.[s]", "var o={0:'a'};o?.[0]+o?.['0']", "var o={a:1};o?.['a']?.toFixed?.(1)", "var o={a:1};o?.a?.toFixed(1)", "var o={a:1};o?.a.toFixed(1)", "var o={a:1};o?.a.nope?.()", "var o={a:1};try{o?.a.nope()}catch(e){e.name+': '+e.message}",
  "var o={a:1};try{o?.a.b.c}catch(e){e.name+': '+e.message}", "var o=null;o?.a.b.c", "var o={};try{o.a.b}catch(e){e.name+': '+e.message}", "var o={};try{o?.a.b}catch(e){e.name+': '+e.message}", "var o={};try{o?.a?.b.c}catch(e){e.name+': '+e.message}",
  "var f=()=>{};try{f?.().a}catch(e){e.name+': '+e.message}", "var f=()=>{};f?.()?.a", "var f=()=>({a:1});f?.().a", "var f=()=>({a:1});f?.()?.a", "var f=null;f?.().a.b.c", "var f=null;f?.()()()", "var f=null;f?.(L.push(1))(L.push(2))",
  "var g=()=>()=>1;g?.()?.()", "var g=()=>null;g?.()?.()", "var g=()=>null;try{g?.()()}catch(e){e.name+': '+e.message}", "var o={f:()=>null};try{o.f?.()()}catch(e){e.name+': '+e.message}", "var o={f:()=>null};o.f?.()?.()", "var o={f:()=>null};o?.f()?.()",
];
ocBase.forEach(c => {
  if (/^(var|function|async|class)\b/.test(c) || c.includes(";")) add(c.replace(/;([^;]*)$/, ";return ($1)").replace(/^(function .*\})([a-z].*)$/, "$1;return ($2)"));
  else add(`return (${c})`);
});
// Sintaxe inválida em torno de ?.
const ocSyn = ["a?.b`x`", "a?.`x`", "a?.b\n`x`", "new a?.b()", "new a?.()", "a?.b=1", "a?.[0]=1", "a?.b++", "++a?.b", "a?.b+=1", "[a?.b]=[1]", "({x:a?.b}=1)", "for(a?.b of []);", "for(a?.b in {});", "a?.1:2", "a?.1", "a?.\n.b", "super?.x", "import?.('x')", "a?.#p", "async()=>a?.b", "a?.b?.c?.d", "a?.[b?.c]", "a?.(b?.c)", "a??.b", "a?.?.b", "x?.y.z?.w", "a?.b.c=1", "(a?.b).c=1", "(a?.b)=1", "delete a?.b", "delete (a?.b)", "typeof a?.b", "a?.b ?? c", "a?.b || c ?? d", "a ?? b || c", "a || b ?? c", "a && b ?? c", "a ?? b && c", "(a || b) ?? c", "a ?? (b || c)", "a ?? b ?? c", "a ?? b | c", "a | b ?? c", "-a ?? b", "!a ?? b", "a ?? !b", "a ?? await b", "a ?? yield", "a ?? b ? c : d", "a ? b ?? c : d", "a ??= b", "a ??= b ??= c", "a ??= b || c", "a ||= b ?? c", "a &&= b ?? c", "a ?? b = c", "a ??= b = c", "(a ??= b)", "a.b ??= c", "a[b] ??= c", "a?.b ??= c", "a() ??= c", "this ??= c", "1 ??= c", "(a) ??= c", "((a)) ??= c", "(a.b) ??= c", "[a] ??= c", "({a}) ??= c", "a++ ??= 1", "new.target ??= 1", "`x` ??= 1", "a\n??= 1", "a ?\n?= 1", "a ? ?= 1", "a? ?b", "a ?.b", "a?. b", "a ?. b", "a?. [0]", "a?. (0)", "a?.[0]?.[1]?.(2)"];
ocSyn.forEach(c => add(`try{eval(${JSON.stringify(c)});return 'ok'}catch(e){return e.name+': '+e.message}`));

// ---- 13. Nullish com atribuição e atribuições lógicas com efeitos.
const nv = ["null", "undefined", "0", "''", "false", "NaN", "0n", "'a'", "1", "true", "{}", "[]", "-0", "Symbol.iterator"];
for (const v of nv) for (const op of ["??=", "||=", "&&="]) {
  add(`var x=${v};var r=(x ${op} (L.push('rhs'),'N'));return [r,x]`);
  add(`var o={get p(){L.push('get');return ${v}},set p(w){L.push('set '+S(w))}};var r=(o.p ${op} (L.push('rhs'),'N'));return r`);
  add(`var o=Object.freeze({p:${v}});var r;try{r=(o.p ${op} 'N')}catch(e){r=e.name}return r`);
  add(`'use strict';var o=Object.freeze({p:${v}});var r;try{r=(o.p ${op} 'N')}catch(e){r=e.name+': '+e.message}return r`);
  add(`const c=${v};var r;try{r=(c ${op} 'N')}catch(e){r=e.name+': '+e.message}return r`);
  add(`var a=[${v}];var i=0;a[i++] ${op} (L.push('rhs'),'N');return [S(a),i]`);
  add(`var f=function(){};var o={p:${v}};o.p ${op} f;return typeof o.p==='function'?o.p.name:'-'`);
  add(`var o={p:${v}};o.p ${op} function(){};return typeof o.p==='function'?JSON.stringify(o.p.name):'-'`);
  add(`var q=${v};q ${op} function(){};return typeof q==='function'?JSON.stringify(q.name):'-'`);
  add(`var q=${v};q ${op} class{};return typeof q==='function'?JSON.stringify(q.name):'-'`);
  add(`var q=${v};q ${op} (()=>1);return typeof q==='function'?JSON.stringify(q.name):'-'`);
  add(`var q=${v};q ${op} (function(){});return typeof q==='function'?JSON.stringify(q.name):'-'`);
  add(`var o={p:${v}};o.p ${op} (function(){});return typeof o.p==='function'?JSON.stringify(o.p.name):'-'`);
  add(`var o={p:${v}};o['p'] ${op} (()=>1);return typeof o.p==='function'?JSON.stringify(o.p.name):'-'`);
  add(`var q=${v};(q) ${op} function(){};return typeof q==='function'?JSON.stringify(q.name):'-'`);
  add(`var p=new Proxy({p:${v}},{has(){L.push('has');return true},get(t,k){L.push('get');return t[k]},set(t,k,w){L.push('set');t[k]=w;return true}});p.p ${op} 'N';return 0`);
  add(`var x=${v};var y=${v};x ${op} y ${op} 'N';return [S(x),S(y)]`);
  add(`var x=${v};var y=(x ${op} 'a') ${op} 'b';return [S(x),S(y)]`);
}
for (const v of nv) for (const w of nv) {
  add(`return [(${v})??(${w}),(${v})||(${w}),(${v})&&(${w})].map(S).join()`);
  add(`return (${v})??(L.push('rhs'),${w})`);
  add(`return (${v})?.constructor?.name??'none'`);
}
const mixSyn = ["a ?? b || c", "a || b ?? c", "a && b ?? c", "a ?? b && c", "(a ?? b) || c", "(a || b) ?? c", "a ?? (b && c)", "a ?? b ?? c"];
mixSyn.forEach(c => add(`var a=null,b=0,c=3;try{return eval(${JSON.stringify(c)})}catch(e){return e.name+': '+e.message}`));

// ---- 14. Spread em chamadas/arrays/objetos.
const iterables = [
  "[1,2,3]", "[]", "[,1]", "[1,,2]", "'ab'", "'\\ud83d\\ude00x'", "'\\ud800'", "''", "new Set([1,1,2])", "new Map([[1,2]])", "new Uint8Array([5,6])", "(function*(){yield 1;yield 2})()", "[1,2].values()", "[1,2].entries()", "'ab'[Symbol.iterator]()",
  "{[Symbol.iterator](){var i=0;return {next(){L.push('next');return i<2?{value:i++,done:false}:{done:true}},return(){L.push('return');return {}}}}}",
  "{[Symbol.iterator](){return {next(){L.push('next');return {value:1,done:true}}}}}", "{[Symbol.iterator](){L.push('iter');return [7][Symbol.iterator]()}}", "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined}", "{[Symbol.iterator]:1}",
  "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return 1}}}}", "{[Symbol.iterator](){return {next(){throw new RangeError('n')}}}}", "{[Symbol.iterator](){throw new RangeError('i')}}",
  "{length:2,0:'a',1:'b'}", "{}", "null", "undefined", "1", "true", "Symbol()", "1n", "()=>1", "new Date(0)", "/x/", "arguments0", "new Proxy([1,2],{get(t,k){L.push(String(k));return t[k]}})",
  "Object.assign([1,2],{extra:3})", "Object.assign([1,2],{[Symbol.iterator]:function*(){yield 'custom'}})", "Object.assign(new String('ab'),{[Symbol.iterator]:function*(){yield 'c'}})", "[1,2,3].keys()",
  "new Set([1]).values()", "new Map([[1,2]]).keys()", "'abc'.matchAll(/./g)", "(function(){return arguments})(1,2)", "Object.freeze([1,2])", "[[1],[2]]", "[undefined,null]", "{get [Symbol.iterator](){L.push('getter');return function*(){yield 1}}}",
];
const spreadCtx = [
  s => `return Math.max(...${s})`,
  s => `return [...${s}]`,
  s => `return [0,...${s},9]`,
  s => `return [...${s},...${s}]`,
  s => `return ((...a)=>a.length)(...${s})`,
  s => `return ((a,b,...r)=>S([a,b,r]))(...${s})`,
  s => `return new Array(...${s}).length`,
  s => `return String.fromCharCode(...${s})`,
  s => `return Array.of(...${s})`,
  s => `return S({...${s}})`,
  s => `return S({a:1,...${s},b:2})`,
  s => `return Object.keys({...${s}}).join()`,
  s => `var [a,...b]=${s};return [a,b]`,
  s => `return new Set(${s}).size`,
  s => `return Array.from(${s})`,
  s => `var r=[];for(var x of ${s})r.push(x);return r`,
];
for (const c of spreadCtx) for (const it of iterables) {
  const pre = it === "arguments0" ? "" : "";
  add(`${pre}return T0();function T0(){${c(it)}}`.replace(/^return T0\(\);function T0\(\)\{(.*)\}$/, "return (function(){$1})()"));
}
// Spread em chamadas: ordem de avaliação, this, argumentos com buracos, ordem com getters.
const spreadMisc = [
  "var o={m(...a){return [this===o,a.length]}};return o.m(...[1,2])", "var o={m(...a){return [this===o,a.length]}};return o?.m(...[1])", "var o={m(...a){return this===o}};return (o.m)(...[])", "var o={m(...a){return this===o}};return (0,o.m)(...[])",
  "function f(){return arguments.length}return f(...[,,])", "function f(){return arguments.length}return f(...[1,,3])", "function f(){return 1 in arguments}return f(...[1,,3])", "function f(){return Object.keys(arguments).join()}return f(...[,,])",
  "function f(...a){return Object.keys(a).join()}return f(...[,,1])", "return [...[,1]].hasOwnProperty(0)", "return [...[1,,2]].length+','+(1 in [...[1,,2]])", "return Object.keys([...[,,]]).join()", "return Object.keys({...[,,1]}).join()",
  "var a=[1,2];var f=(x,y)=>[x,y];return f(...a,...a)", "var a=[1,2];return ((...r)=>r)(...a,...'xy',...new Set([3]))", "var log=[];var f=function(){log.push('call')};f((log.push('a'),1),...(log.push('s'),[2]),(log.push('b'),3));return log.join()",
  "var f=(L.push('f'),function(){});f((L.push('a'),1),...(L.push('s'),[]));return L.join()", "var o={get m(){L.push('get m');return function(){}}};o.m(...(L.push('s'),[]));return L.join()", "try{(L.push('f'),undefined)(...(L.push('s'),[]))}catch(e){L.push(e.name)}return L.join()",
  "try{undefinedFn(...(L.push('s'),[]))}catch(e){L.push(e.name)}return L.join()", "try{var o={};o.m(...(L.push('s'),[]))}catch(e){L.push(e.name)}return L.join()", "try{(void 0)(...[1])}catch(e){return e.name+': '+e.message}",
  "try{var o={};o.m(...[1])}catch(e){return e.name+': '+e.message}", "try{Math.max(...null)}catch(e){return e.name+': '+e.message}", "try{Math.max(...undefined)}catch(e){return e.name+': '+e.message}", "try{Math.max(...1)}catch(e){return e.name+': '+e.message}",
  "try{Math.max(...{})}catch(e){return e.name+': '+e.message}", "try{[...null]}catch(e){return e.name+': '+e.message}", "try{[...{}]}catch(e){return e.name+': '+e.message}", "try{[...1]}catch(e){return e.name+': '+e.message}", "try{[...Symbol()]}catch(e){return e.name+': '+e.message}",
  "try{[...undefined]}catch(e){return e.name+': '+e.message}", "try{new Set(...[1])}catch(e){return e.name+': '+e.message}", "try{({...null,...undefined})}catch(e){return e.name}", "return S({...1,...true,...Symbol(),...1n,...'ab'})",
  "var o={get a(){L.push('ga');return 1},get b(){L.push('gb');return 2}};var c={...o};return [L.join(),Object.getOwnPropertyDescriptor(c,'a').value,Object.getOwnPropertyDescriptor(c,'a').get]",
  "var o={a:1,get b(){delete this.c;return 2},c:3};return S({...o})", "var o={a:1,get b(){this.c=4;return 2}};return S({...o})", "var o={get a(){L.push('a');return 1}};return S({...o,a:2})", "var o={get a(){L.push('a');return 1}};return S({a:2,...o})",
  "var o={set a(v){L.push('set')}};var c={...{a:1},...o};return L.length+S(c)", "var c={set a(v){L.push('setter')},...{a:1}};return L.length+','+Object.getOwnPropertyDescriptor(c,'a').value", "var c={__proto__:{set a(v){L.push('setter')}},...{a:1}};return L.length+','+Object.keys(c)",
  "var c={...{__proto__:{x:1}}};return S(c)+c.x", "var c={...JSON.parse('{\"__proto__\":1}')};return Object.getPrototypeOf(c)===Object.prototype?Object.keys(c).join():'proto'", "var s=Symbol('s');var c={...{[s]:1,b:2,1:3,a:4}};return Reflect.ownKeys(c).map(String).join()",
  "var p=new Proxy({a:1,b:2},{ownKeys(t){L.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){L.push('get '+k);return t[k]}});({...p});return L.join()",
  "var p=new Proxy({a:1},{ownKeys(){L.push('ownKeys');return ['a','a']}});try{return S({...p})}catch(e){return e.name+': '+e.message}", "var o=Object.defineProperty({a:1},'h',{value:2,enumerable:false});return S({...o})",
  "var o=Object.create({inh:1},{own:{value:2,enumerable:true}});return S({...o})", "var c={...new Map([[1,2]])};return S(c)", "var c={...new Uint8Array([7,8])};return S(c)", "var c={...(function(){return arguments})(1,2)};return S(c)", "var c={...function(){}};return S(c)",
  "class A{constructor(){this.x=1}get y(){return 2}static z=3}return S({...new A,...A})", "var c={...'\\ud83d\\ude00'};return S(c)", "var c={...[1,2,3]};return Object.keys(c).join()+Array.isArray(c)", "var c=[...'ab',...[1],...new Set([2])];return S(c)",
  "var a=[1,2,3];var b=[...a];b[0]=9;return S(a)", "var a=[[1]];var b=[...a];b[0].push(2);return S(a)", "var a=[1,2,3];var b=[...a.slice(1),...a.slice(0,1)];return S(b)", "var a=[3,1,2];return S([...a].sort())+S(a)", "return [...Array(3)].length+','+(0 in [...Array(3)])",
  "return [...Array(3).keys()].join()", "return [...Array.from({length:3},(_, i)=>i*2)].join()", "return Math.max(...[1,5,3],...[9,2])", "return Math.min(...[])", "return Math.max(...[],...[])", "return Math.hypot(...[3,4])", "return String.fromCharCode(...[72,105])",
  "return ''.concat(...['a','b'],...[1])", "return [].concat(...[[1],[2,[3]]]).length", "return Array.prototype.push.call([],...[1,2])", "var big=new Array(100000).fill(1);return Math.max(...big)", "var big=new Array(200000).fill(1);try{return Math.max(...big)}catch(e){return e.name}",
  "var f=(a,b=a,...c)=>[a,b,c];return S(f(...[1]))+S(f(...[1,2,3,4]))+S(f(...[undefined,undefined]))", "var f=function(a,b){return arguments.length};return f(...[1],2,...[3,4],5)", "var f=({a},[b])=>a+b;return f(...[{a:1},[2]])",
  "var f=function(){'use strict';return this};return f.call(...[1])", "return Function.prototype.call.apply(function(){return this.v},[{v:5}])", "return Reflect.apply(Math.max,null,[1,2])", "var i=0;return [...[i++,i++,i++]].join()+i", "var i=0;return [i++,...[i++],i++].join()",
  "var i=0;var o={[i++]:i++,...{[i++]:i++}};return S(o)+i", "var i=0;function f(...a){return a.join()}return f(i++,...[i++,i++],i++)", "var g=function*(){L.push('start');yield 1;L.push('mid');yield 2;L.push('end')};var a=[...g()];return L.join()+S(a)",
  "var g=function*(){try{yield 1;yield 2}finally{L.push('fin')}};var [x]=g();return L.join()+x", "var g=function*(){try{yield 1;yield 2}finally{L.push('fin')}};var [...x]=g();return L.join()+S(x)", "var g=function*(){try{yield 1;yield 2}finally{L.push('fin')}};Math.max(...g());return L.join()",
];
spreadMisc.forEach(c => { add(c); });

// ---- 15. Destructuring em parâmetros com defaults avaliando efeitos.
const dpat = [
  "{a}", "{a=L.push('da')}", "{a,b=a}", "{a=b,b=1}", "{a:{b}}", "{a:{b}={}}", "{a:{b=L.push('db')}={}}", "[a]", "[a=L.push('da')]", "[a,b=a]", "[a=b,b=1]", "[,a]", "[a,,b]", "[...r]", "[a,...r]", "[[a]]", "[[a]=[]]", "{a,...r}", "{...r}", "{a:[b]}", "{a:[b]=[7]}",
  "{[(L.push('k'),'a')]:a}", "{[(L.push('k'),'a')]:a=L.push('da')}", "a=L.push('da')", "a=b,b", "a,b=L.push('db')", "a=1,b=a+1", "a,[b],{c}", "a,{b}={b:L.push('db')}", "a=(L.push('a'),1),b=(L.push('b'),2)", "{a=1,b=2}={}", "[a=1,b=2]=[]", "{a}={a:L.push('dflt')}", "[a]=[L.push('dflt')]",
  "a=function(){}", "a=()=>1", "a=class{}", "{a=function(){}}", "[a=()=>1]", "{a=class{}}", "a=typeof a", "a=arguments.length", "a=this", "a=new.target", "{a=arguments.length}", "[a=b,b]", "a=eval('1')", "a=eval('var q=2;q')", "{a}={}", "[a]=[]", "{a}=null", "[a]=null", "{}", "[]", "{}=null", "[]=null",
];
const dargs = ["", "undefined", "null", "1", "{}", "{a:1}", "{a:undefined}", "{a:null}", "{a:{b:2}}", "{a:{}}", "[]", "[1]", "[undefined]", "[null]", "[1,2,3]", "[[2]]", "[[]]", "'xy'", "{get a(){L.push('ga');return 1}}", "{a:1,b:2,c:3}", "1,2", "undefined,undefined", "{},{}", "[],[]", "1,[2],{c:3}", "{a:7},{b:8}"];
for (const p of dpat) for (const a of dargs) {
  add(`function f(${p}){return [typeof a==='undefined'?'-':S(a),typeof b==='undefined'?'-':S(b),typeof r==='undefined'?'-':S(r)]}return f(${a})`);
}
for (const p of dpat.slice(0, 40)) for (const a of ["{a:1}", "[1]", "undefined", "{}"]) {
  add(`var f=(${p})=>[typeof a==='undefined'?'-':S(a),typeof b==='undefined'?'-':S(b)];return f(${a})`);
  add(`var o={m(${p}){return typeof a==='undefined'?'-':S(a)}};return o.m(${a})`);
  add(`class C{constructor(${p}){this.v=typeof a==='undefined'?'-':S(a)}}return new C(${a}).v`);
}
const dmisc = [
  "function f(a=L.push('a'),b=L.push('b')){return L.join()}return f()", "function f(a=L.push('a'),b=L.push('b')){return L.join()}return f(1)", "function f(a=L.push('a'),b=L.push('b')){return L.join()}return f(1,2)", "function f(a=L.push('a'),b=L.push('b')){return L.join()}return f(undefined,2)",
  "function f(a=L.push('a'),b=L.push('b')){return L.join()}return f(null,null)", "function f(a,b=a){return [a,b]}return f(1)", "function f(a=b,b){return 1}try{return f()}catch(e){return e.name+': '+e.message}", "function f(a=b,b){return 1}return f(1)", "function f(a,b=a){a=5;return [a,b]}return f(1)",
  "function f(a=()=>b,b=2){var b=3;return a()}return f()", "function f(a=()=>b,b=2){var b;return [a(),b]}return f()", "function f(a=()=>b,b=2){var b=3;return [a(),b]}return f()", "function f(a,b=()=>a){var a=9;return [a,b()]}return f(1)", "function f(a,b=()=>a){a=9;return [a,b()]}return f(1)",
  "function f(a=1){return arguments.length}return [f(),f(undefined),f(1,2)]", "function f(a=1){a=2;return arguments[0]}return f()", "function f(a){a=2;return arguments[0]}return f(1)", "function f(a=0){a=2;return arguments[0]}return f(1)", "function f(a,b){b=2;return arguments.length}return f(1)",
  "function f(a=1){return f.length}return f()", "function f(a,b=1,c){return f.length}return f()", "function f(a,b,...c){return f.length}return f()", "function f({a},[b]){return f.length}return f()", "function f(a=1,b){return f.length}return f()", "return ((a,b=1)=>1).length+','+((...a)=>1).length+','+(({a},b)=>1).length",
  "function f(a=eval('var z=1;z')){return typeof z}return f()", "function f(a=eval('var z=1;z')){var z;return typeof z}return f()", "function f(a=eval('var a2=1;a2'),b=()=>a2){return typeof a2}try{return f()}catch(e){return e.name}", "var x='outer';function f(a=x){var x='inner';return a}return f()",
  "var x='outer';function f(a=()=>x){var x='inner';return a()}return f()", "var x='outer';function f(a=()=>x){x='inner';return a()}return f()", "var x='outer';function f(a=()=>x,x='p'){x='inner';return a()}return f()", "function f(a=this){return a}return f.call(5)===5||typeof f.call(5)",
  "function f(a=new.target){return a}return f()===undefined", "function f(a=new.target){return a}return typeof new f()", "function f(a=arguments){return a.length}return f(undefined,2,3)", "function f(a=arguments[1]){return a}return f(undefined,'second')", "var f=(a=arguments)=>a;try{return typeof f()}catch(e){return e.name}",
  "function f({a=L.push('a')}={}){return L.join()}return f()+f({})+f({a:0})", "function f([a=L.push('a')]=[]){return L.join()}return f()+'|'+f([])+'|'+f([0])", "function f({a}={a:L.push('dflt')}){return L.join()}return f()+'|'+f({})", "function f({a}={}){return a}return [f(),f({a:1}),f(undefined),f(null)].length",
  "function f({a}={}){return a}try{return f(null)}catch(e){return e.name+': '+e.message}", "function f({a}){return a}try{return f()}catch(e){return e.name+': '+e.message}", "function f({a}){return a}try{return f(null)}catch(e){return e.name+': '+e.message}", "function f([a]){return a}try{return f()}catch(e){return e.name+': '+e.message}",
  "function f([a]){return a}try{return f({})}catch(e){return e.name+': '+e.message}", "function f([a]){return a}try{return f(1)}catch(e){return e.name+': '+e.message}", "function f([a]){return a}try{return f(null)}catch(e){return e.name+': '+e.message}", "function f({a:{b}}){return b}try{return f({})}catch(e){return e.name+': '+e.message}",
  "function f({a:{b}}){return b}try{return f({a:null})}catch(e){return e.name+': '+e.message}", "function f({a:[b]}){return b}try{return f({a:{}})}catch(e){return e.name+': '+e.message}", "function f({a:[b]}){return b}try{return f({})}catch(e){return e.name+': '+e.message}", "function f({...r}){return r}return S(f({a:1,b:2}))",
  "function f({a,...r}){return [a,r]}return S(f({a:1,b:2,c:3}))", "function f({a,...r}){return r}return S(f('xyz'))", "function f({length}){return length}return [f('abc'),f([1]),f(function(a,b){})]", "function f({0:a,1:b}){return [a,b]}return f('xy')", "function f({a,...r}){return Reflect.ownKeys(r).map(String).join()}return f({[Symbol.iterator]:1,a:1,1:2,b:3})",
  "function f([a,b]){return [a,b]}return f('xy')", "function f([a,b]){return [a,b]}return f(new Set([1,2]))", "function f([a,b]){return [a,b]}return f(new Map([[1,2],[3,4]]))", "function f([a,b]){return [a,b]}return f((function*(){yield 1})())", "function f([a,b]){return [a,b]}return f({[Symbol.iterator]:function*(){yield 'g'}})",
  "function f([a]){return L.join()}return f({[Symbol.iterator](){return {next(){L.push('n');return {value:1,done:false}},return(){L.push('r');return {}}}}})", "function f([a,b]){return L.join()}return f({[Symbol.iterator](){return {next(){L.push('n');return {value:1,done:true}},return(){L.push('r');return {}}}}})",
  "function f([...a]){return L.join()+S(a)}return f({[Symbol.iterator](){var i=0;return {next(){L.push('n');return i++<2?{value:i,done:false}:{done:true}},return(){L.push('r');return {}}}}})", "function f([a=L.push('d')]){return L.join()}return f([undefined])", "function f([a=L.push('d')]){return L.join()}return f([null])",
  "function f([a=L.push('d')]){return L.join()}return f([])", "function f({a=L.push('d'),b=L.push('e')}){return L.join()}return f({b:1})", "function f({[(L.push('k1'),'a')]:x=L.push('d1'),[(L.push('k2'),'b')]:y=L.push('d2')}){return L.join()}return f({})",
  "function f({get a(){}}){}", "function f({a:b.c}){}", "function f(a,a){return a}return f(1,2)", "function f(a,a=1){}", "function f(a,[a]){}", "function f(a,...a){}", "function f(...a,b){}", "function f(...a=[]){}", "function f(...[a,b]){return [a,b]}return f(1,2,3)", "function f(...{length}){return length}return f(1,2,3)",
  "function f(a,){return a}return f(1,)", "function f(,a){}", "function f(a,,b){}", "function f(a=1,){return a}return f()", "function f(...a,){}", "function f(a){'use strict'}return f.length", "function f(a=1){'use strict'}", "function f({a}){'use strict'}", "function f(...a){'use strict'}", "(a=1)=>{'use strict'}", "({a})=>{'use strict'}",
  "var f=({a},{b}={a})=>[a,b];return S(f({a:1},undefined))", "var f=([a],[b]=[a])=>[a,b];return S(f([1]))", "var f=({a},b=a)=>b;return f({a:3})", "var f=async({a}={})=>a;return typeof f()", "var f=function*({a}={a:1}){yield a};return f().next().value", "var f=async function*([a]=[2]){yield a};return typeof f().next",
  "var o={m({a}={a:1},[b]=[2]){return a+b}};return o.m()", "var o={set p({a}){L.push(a)}};o.p={a:5};return L", "var o={set p([a]){L.push(a)}};o.p=[6];return L", "class A{static m({a}={a:1}){return a}}return A.m()", "class A{#m({a}={a:1}){return a}static t(){return new A().#m()}}return A.t()",
  "class A{constructor({a}={a:1}){this.a=a}}class B extends A{constructor(...args){super(...args)}}return new B().a", "class A{constructor(a){this.a=a}}class B extends A{constructor({x}){super(x)}}return new B({x:4}).a", "class A{}class B extends A{constructor({x}={x:1},y=x){super();this.s=x+y}}return new B().s",
  "var f=function({a},b=a){return [a,b]}.bind(null,{a:5});return S(f())", "var f=function({a}){return a};return f.call(null,{a:2})+f.apply(null,[{a:3}])", "var f=(a,b)=>[a,b];return S(f.apply(null,{length:2,0:1}))", "var f=({a=1}={},...r)=>[a,r.length];return S(f(undefined,2,3))",
  "var x;[x=L.push('d')]=[];return L.join()", "var x,y;[x,y=x]=[1];return [x,y]", "var x,y;({x,y=x}={x:2});return [x,y]", "var o={};({a:o.p,b:o['q']}={a:1,b:2});return S(o)", "var o={};[o.a,o.b]=[1,2];return S(o)", "var o={};({...o.r}={a:1});return S(o)", "var a=[];[...a[0]]=[1,2];return S(a)",
  "var x,y;[x,y]=[y,x]=[1,2];return [x,y]", "var a=1,b=2;[a,b]=[b,a];return [a,b]", "var a=1,b=2;({a,b}={a:b,b:a});return [a,b]", "var i=0,a=[];[a[i++],a[i++]]=[7,8];return S(a)+i", "var i=0,a=[];[a[i++]=L.push('d')]=[];return S(a)+i", "var o={get x(){L.push('g');return 1}};var {x,y=x}=o;return L.join()+y",
  "var {a,a:b}={a:1};return [a,b]", "var {a:{b}={b:2}}={};return b", "var {a:[b]=[3]}={};return b", "var [{a}={a:4}]=[];return a", "var [[a]=[5]]=[];return a", "var {length}='abc';return length", "var {0:a,length:n}='xy';return [a,n]", "var {toString}=1;return typeof toString", "var {valueOf:v}=true;return typeof v",
  "var {a}=null;", "var {}=null;", "var []=null;", "var {a}=undefined;", "var [a]=undefined;", "var {a}=0;return a", "var {a}='';return a", "var {a}=Symbol();return a", "var {a}=1n;return a", "var [a]='';return a", "var [a]=0;", "var [a]=Symbol();", "var {...r}=null;", "var {...r}=1;return S(r)", "var {...r}='ab';return S(r)",
];
dmisc.forEach(c => {
  if (/^(function|var|class)\b/.test(c)) add(`try{${c.replace(/(^|;)(return[^;]*)$/, "$1$2")}}catch(e){return 'outer '+e.name+': '+e.message}`);
  else add(`try{${c}}catch(e){return 'outer '+e.name+': '+e.message}`);
  add(`try{return eval(${JSON.stringify("(function(){" + c + "})()")})}catch(e){return e.name+': '+e.message}`, false);
});

// ---- 16. Concatenação com + e conversões de string.
const cvals = ["''", "'a'", "1", "-0", "0.1", "1e21", "1e-7", "123456789012345680000", "NaN", "Infinity", "null", "undefined", "true", "[]", "[1,[2]]", "[null]", "[undefined,1]", "{}", "function f(){}", "()=>1", "class A{}", "1n", "-5n", "new Date(NaN)", "/x/g", "new Error('m')", "Symbol.iterator.description", "new String('s')", "new Number(1)", "new Boolean(false)", "[,]", "[[],[]]", "Object(1n)", "{toString(){return 't'}}", "{valueOf(){return 1}}", "{valueOf(){return 'v'},toString(){return 't'}}", "{[Symbol.toPrimitive](h){return '<'+h+'>'}}", "{[Symbol.toPrimitive]:null,toString(){return 'x'}}", "new Date(0).getTime()"];
for (const a of cvals) for (const b of cvals) { add(`return (${a})+(${b})`); }
for (const a of cvals) { add(`return \`\${${a}}\``); add(`return String(${a})`); add(`return ''+${a}`); add(`return ${a}+''`); add(`return [${a}]+''`); add(`return [${a}].join()`); add(`return JSON.stringify(\`\${${a}}\`)`); add(`var s='x';s+=${a};return s`); add(`var s=${a};s+='y';return s`); add(`return 'a'.concat(${a})`); add(`return \`a\${${a}}b\${${a}}\`.length`); }
for (const a of ["1", "'1'", "null", "undefined", "true", "[]", "{}"]) for (const b of ["1", "'1'", "null", "undefined", "true", "[]", "{}"]) for (const c of ["2", "'2'", "[]"]) { add(`return (${a})+(${b})+(${c})`); add(`return (${a})+((${b})+(${c}))`); }
add("return 1+2+'3'+4+5"); add("return '1'+2+3"); add("return 1+(2+'3')"); add("return +'1'+ +'2'"); add("return '3'-'1'+'1'"); add("return [1]+[2]"); add("return {}+[]"); add("return ({})+[]"); add("return []+{}"); add("return [].concat({})+''");
add("var s='';for(var i=0;i<5;i++)s+=i;return s"); add("var s='a';s+=s+=s;return s"); add("var s='ab';return s+s.length+s[0]+s.at(-1)"); add("var x=1;x+=x+++x;return x"); add("var s='';s+=1;s+=null;s+=undefined;s+=true;return s");
add("return 'a'+L.push(1)+'b'+L.push(2)"); add("return `${L.push(1)}`+`${L.push(2)}`+L.join()"); add("return ({valueOf(){L.push('l');return 1}})+({valueOf(){L.push('r');return 2}})"); add("return ({toString(){L.push('l');return 'a'}})+({toString(){L.push('r');return 'b'}})");
add("return ({[Symbol.toPrimitive](h){L.push(h);return 1}})+1"); add("return ({[Symbol.toPrimitive](h){L.push(h);return 1}})+'a'"); add("return 'a'+({[Symbol.toPrimitive](h){L.push(h);return 1}})"); add("return ({[Symbol.toPrimitive](h){L.push(h);return 1}})*2");
add("return `${{[Symbol.toPrimitive](h){L.push(h);return 1}}}`"); add("return String({[Symbol.toPrimitive](h){L.push(h);return 1}})"); add("return ({[Symbol.toPrimitive](h){L.push(h);return 1}})==1"); add("return ({[Symbol.toPrimitive](h){L.push(h);return 1}})<1");
add("return new Date(0)+1===new Date(0).toString()+'1'"); add("return typeof (new Date(0)+1)"); add("return typeof (new Date(0)-1)"); add("return new Date(5)*1"); add("return new Date(5)<new Date(6)"); add("return new Date(5)==new Date(5)"); add("return new Date(5)>=new Date(5)");
add("try{return 1n+1}catch(e){return e.name+': '+e.message}"); add("try{return 1n+'1'}catch(e){return e.name+': '+e.message}"); add("return 1n+'1'+1n"); add("try{return Symbol()+1}catch(e){return e.name+': '+e.message}"); add("try{return +Symbol()}catch(e){return e.name+': '+e.message}"); add("try{return +1n}catch(e){return e.name+': '+e.message}");
add("try{return 'a'+Object.create(null)}catch(e){return e.name+': '+e.message}"); add("try{return `${Object.create(null)}`}catch(e){return e.name+': '+e.message}"); add("try{return 'a'<Object.create(null)}catch(e){return e.name+': '+e.message}");

// ---- Geração: dedup, execução em paralelo, escrita.
const baseDir = path.join(__dirname, "..", "tests", "golden");
let baseText = "";
for (const f of ["operator_edge_bun.tsv", "operator_grid_bun.tsv", "template_edge_bun.tsv", "destructuring_bun.tsv", "call_edge_bun.tsv", "number_matrix_bun.tsv", "numeric_limits_bun.tsv", "bigint_symbol_bun.tsv", "object_edge_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(baseDir, f), "utf8").split("\n")) {
      if (!line) continue;
      try { baseText += JSON.parse(line.split("\t")[0]) + "\n\u0000\n"; } catch (e) {}
    }
  } catch (e) {}
}
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
progs.splice(0, progs.length, ...progs.filter((p) => !usesHostApi(p.body)));
const unique = [];
let dup = 0;
for (const p of progs) {
  const key = (p.strict ? "S:" : "N:") + p.body;
  if (seen.has(key)) continue;
  seen.add(key);
  // O corpo inteiro já aparecer num golden vizinho conta como repetido.
  if (p.body.length > 28 && baseText.includes(p.body)) { dup++; continue; }
  unique.push(p);
}
const sources = unique.map(p => (p.strict ? '"use strict";\n' : "") + PRELUDE + `globalThis.R = T(()=>{${p.body}});`);

const results = new Array(sources.length);
let next = 0;
let active = 0;
let finished = 0;
const limit = Math.max(4, os.cpus().length);
function launch(i) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let timedOut = false;
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", () => {});
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 6000);
    child.on("close", code => {
      clearTimeout(timer);
      results[i] = timedOut || code !== 0 ? null : decodeResult(out);
      resolve();
    });
    child.stdin.on("error", () => {});
    child.stdin.end(sources[i]);
  });
}
async function pool() {
  const workers = [];
  for (let w = 0; w < limit; w++) {
    workers.push((async () => {
      while (next < sources.length) { const i = next++; await launch(i); finished++; }
    })());
  }
  await Promise.all(workers);
}
pool().then(() => {
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < sources.length; i++) {
    const result = results[i];
    if (result === null) { dropped++; process.stderr.write("filho falhou ou estourou o tempo: " + JSON.stringify(unique[i].body).slice(0, 140) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(unique[i].body).slice(0, 140) + "\n"); continue; }
    kept++;
    lines.push({ source: sources[i], result: result });
  }
  process.stdout.write(emitFactored("template_ops", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens vizinhos ${dup}\n`);
});
