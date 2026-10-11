// Gera tests/golden/regexp_receiver_bun.tsv: getters e métodos de RegExp.prototype em RECEPTORES exóticos, medido no bun 1.4.2.
// Cobre: getters (flags, source, global, ...) no próprio RegExp.prototype, em objetos comuns, primitivos, subclasses,
// Proxy e RegExp com propriedades próprias; RegExp.prototype.flags com getters individuais registrando a ordem de leitura;
// toString com source/flags exóticos; Symbol.match/replace/search/split/matchAll chamando exec personalizado (retorno
// não-objeto, exceção, getter de exec, exec não chamável), lastIndex coagido, não gravável e acessor em objeto comum;
// RegExp(pattern, flags) com pattern RegExp, Symbol.match falso/verdadeiro, constructor trocado; compile() legado;
// subclasses com getters, exec e Symbol.species sobrescritos; Proxy de RegExp com get trap.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda num bun filho novo,
// sem APIs de host, com timeout de 5 s; no máximo 8 filhos em paralelo. Programas de fonte repetida, ou já presentes em
// outro golden de tests/golden, são descartados. O prelúdio comum sai em tests/golden/regexp_receiver.preludes.json.
// Uso: bun scripts/gen-regexp-receiver-golden.js > tests/golden/regexp_receiver_bun.tsv
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { emitFactored, readRows, GOLDEN_DIR, writeResultPreload, decodeResult } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const { usesHostApi } = require("./host-api.js");

const PRELUDE =
  '"use strict";\n' +
  'var L=[];\n' +
  'function S(v){try{if(typeof v==="string")return JSON.stringify(v);if(Object.is(v,-0))return "-0";if(typeof v==="bigint")return v+"n";' +
  'if(typeof v==="symbol")return v.toString();if(typeof v==="undefined")return "undefined";if(typeof v==="function")return "fn";' +
  'if(Array.isArray(v)){var r="["+Array.from({length:v.length},(_,i)=>i in v?S(v[i]):"<hole>").join(",")+"]";' +
  'if(v.index!==undefined)r+=" index="+v.index;if(v.input!==undefined)r+=" input="+S(v.input);if(v.groups!==undefined)r+=" groups="+S(v.groups);return r}' +
  'if(v!==null&&typeof v==="object"){return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k])).join(",")+"}"}return String(v)}catch(e){return "?"}}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?r+" | log="+L.map(S).join(","):r}\n' +
  'function E(){var a=arguments,i=0;return function(){L.push("e"+this.lastIndex+(typeof arguments[0]==="string"?":"+arguments[0]:""));return i<a.length?a[i++]:null}}\n' +
  'function F(r){try{return[r.source,r.flags,r.lastIndex]}catch(e){return e.name+": "+e.message}}\n';

const programs = new Set();
const q = (s) => JSON.stringify(s).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");
// body: corpo de uma função que devolve o valor.
const add = (body) => programs.add(PRELUDE + `globalThis.R = T(()=>{${body}})`);

const GETTERS = ["flags", "source", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky", "hasIndices"];
const FLAG_PROPS = ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"];

// ---- A. Getters em receptores exóticos.
const RECEIVERS = [
  "RegExp.prototype", "{}", "[]", "1", "'s'", "null", "undefined", "Symbol('q')", "function(){}", "Object.create(/a/g)", "new Proxy(/a/g,{})", "new Proxy({},{})",
  "new (class X extends RegExp{})('a','gi')", "/a/gimsuyd", "new RegExp('a','v')", "new RegExp('a/b\\n','')", "Object.create(RegExp.prototype)",
  "Object.setPrototypeOf({},RegExp.prototype)", "new Proxy(RegExp.prototype,{})", "true", "1n", "new Number(1)", "Object.assign(/x/,{global:true})",
  "(()=>{var r=/x/g;Object.defineProperty(r,'flags',{value:'zz'});return r})()", "(()=>{var r=/x/;Object.setPrototypeOf(r,null);return r})()",
  "(()=>{var r=/x/y;Object.setPrototypeOf(r,{});return r})()", "RegExp", "new RegExp('','d')", "/[/]/s", "Object.freeze(/f/i)", "new Proxy(/a/y,{get(t,k){L.push(String(k));return Reflect.get(t,k,t)}})",
];
for (const g of GETTERS) for (const r of RECEIVERS) {
  add(`return Object.getOwnPropertyDescriptor(RegExp.prototype,'${g}').get.call(${r})`);
  add(`return Reflect.get(RegExp.prototype,'${g}',${r})`);
  add(`return (${r}).${g}`);
}
for (const g of GETTERS) {
  add(`var d=Object.getOwnPropertyDescriptor(RegExp.prototype,'${g}');return [typeof d.get,d.set,d.enumerable,d.configurable,d.get.name,d.get.length,Object.hasOwn(d,'value')]`);
  add(`return RegExp.prototype.hasOwnProperty('${g}')+' '+(/a/.hasOwnProperty('${g}'))+' '+('${g}' in /a/)`);
  add(`'use strict';RegExp.prototype.${g}='x';return 1`);
  add(`return delete RegExp.prototype.${g}`);
  add(`var r=/a/;r.${g}='own';return [r.${g},Object.keys(r)]`);
}
for (const name of ["exec", "test", "toString", "compile", "Symbol.match", "Symbol.replace", "Symbol.search", "Symbol.split", "Symbol.matchAll"]) {
  const ref = name.startsWith("Symbol.") ? `RegExp.prototype[${name}]` : `RegExp.prototype.${name}`;
  add(`var f=${ref};return [typeof f,f.name,f.length,Object.getOwnPropertyDescriptor(RegExp.prototype,${name.startsWith("Symbol.") ? name : q(name)}).enumerable]`);
  for (const r of ["undefined", "null", "{}", "1", "'s'", "Object.create(RegExp.prototype)", "new Proxy(/a/,{})", "RegExp.prototype", "Symbol()"])
    add(`return ${ref}.call(${r},'a')`);
}

// ---- B. flags com getters individuais registrando a ordem.
const flagsGetter = "Object.getOwnPropertyDescriptor(RegExp.prototype,'flags').get";
const logObj = (vals) => `(()=>{var o={};${JSON.stringify(FLAG_PROPS)}.forEach(function(n,i){Object.defineProperty(o,n,{get:function(){L.push(n);return ${vals}[i]}})});return o})()`;
for (let mask = 0; mask < 256; mask++) {
  const vals = FLAG_PROPS.map((_, i) => ((mask >> i) & 1 ? "true" : "false")).join(",");
  add(`return ${flagsGetter}.call(${logObj(`[${vals}]`)})`);
}
for (const v of ["0", "''", "NaN", "null", "undefined", "1", "'x'", "{}", "[]", "Symbol()", "0n", "1n", "-0", "'false'", "function(){}", "new Boolean(false)", "document"]) {
  add(`return ${flagsGetter}.call(${logObj(`[${Array(8).fill(v).join(",")}]`)})`);
  for (let i = 0; i < 8; i++) {
    const arr = Array(8).fill("false");
    arr[i] = v;
    add(`return ${flagsGetter}.call(${logObj(`[${arr.join(",")}]`)})`);
  }
}
for (let k = 0; k < 8; k++) {
  add(`var o=${logObj("[true,true,true,true,true,true,true,true]")};Object.defineProperty(o,'${FLAG_PROPS[k]}',{get:function(){L.push('boom');throw new RangeError('x${k}')}});return ${flagsGetter}.call(o)`);
}
for (const n of FLAG_PROPS) for (const base of ["/a/", "/a/dgimsy", "/a/u", "/a/v"]) for (const v of ["true", "false", "'x'", "0"]) {
  add(`var r=${base};Object.defineProperty(r,'${n}',{get:function(){L.push('${n}');return ${v}}});return r.flags`);
}
for (const n of FLAG_PROPS) for (const v of ["true", "false"]) for (const base of ["/a/", "/a/g"]) {
  add(`var d=Object.getOwnPropertyDescriptor(RegExp.prototype,'${n}');Object.defineProperty(RegExp.prototype,'${n}',{get:function(){L.push('${n}');return ${v}},configurable:true});try{return ${base}.flags}finally{Object.defineProperty(RegExp.prototype,'${n}',d)}`);
  add(`var d=Object.getOwnPropertyDescriptor(RegExp.prototype,'${n}');Object.defineProperty(RegExp.prototype,'${n}',{get:function(){L.push('${n}');return ${v}},configurable:true});try{return String(${base})}finally{Object.defineProperty(RegExp.prototype,'${n}',d)}`);
}
add(`var r=/a/gi;Object.defineProperty(r,'flags',{get:function(){L.push('flags');return 'm'}});return [String(r),r.source]`);
add(`var r=/a/gi;Object.defineProperty(r,'global',{value:false});return [r.flags,String(r)]`);

// ---- C. source e toString com valores exóticos.
const PATTERNS = ["a", "", "(?:)", "/", "a/b", "[/]", "\\/", "\n", "\\n", "\u2028", "\u2029", "\r", "[\n]", "\\\\", "\\u{61}", "a|b", "\\/\\/", "/*/", "\\\n", "\\\\n", "[\\/]", "(?<n>x)\\k<n>", "\t", "\\0"];
const FLAGS = ["", "g", "i", "m", "s", "u", "v", "y", "d", "gimsuyd", "dgimsvy", "uv", "gg", "z"];
for (const p of PATTERNS) for (const f of FLAGS) {
  add(`return (()=>{var r=new RegExp(${q(p)},${q(f)});return [r.source,r.flags,String(r)]})()`);
  add(`return RegExp.prototype.toString.call(new RegExp(${q(p)},${q(f)}))`);
}
for (const p of PATTERNS) {
  add(`var r=new RegExp(${q(p)});r.lastIndex=3;return [r.source,r.lastIndex,eval(String(r)).source===r.source]`);
  add(`return new RegExp(new RegExp(${q(p)}).source).source`);
}
const TS_VALUES = ["'a'", "''", "undefined", "null", "1", "{toString(){L.push('ts');return 'x'}}", "Symbol('s')", "{toString(){throw new RangeError('src')}}", "[1,2]", "'/'", "true", "-0", "1n", "{valueOf(){return 'v'}}", "{[Symbol.toPrimitive](h){L.push(h);return 'tp'}}"];
for (const s of TS_VALUES) for (const f of TS_VALUES) {
  add(`return RegExp.prototype.toString.call({get source(){L.push('source');return ${s}},get flags(){L.push('flags');return ${f}}})`);
}
for (const r of ["undefined", "null", "1", "'s'", "{}", "{source:'x'}", "{flags:'g'}", "[]", "Symbol()", "true", "function(){}", "new Proxy({},{get(t,k){L.push(String(k));return 'k'+String(k)}})", "new Proxy(/a/g,{get(t,k,r){L.push(String(k));return Reflect.get(t,k,t)}})"])
  add(`return RegExp.prototype.toString.call(${r})`);

// ---- D. Symbol.match/replace/search/split/matchAll chamando exec personalizado.
const RES = [
  "null", "undefined", "1", "'a'", "true", "Symbol('s')", "{}", "[]", "['a']", "Object.assign(['a'],{index:0})", "{0:'a',length:1,index:0}", "{0:'',index:0,length:1}",
  "{0:{toString(){L.push('ts');return 'zz'}},index:1,length:1}", "{length:3,0:'a',1:'b',2:'c',index:1}", "{0:'a',index:-1,length:1}", "{0:'a',index:'1',length:1}", "{0:'a',index:100,length:1}",
  "{0:'a',index:1.5,length:1}", "{0:'a',index:NaN,length:1}", "{0:'ab',index:0,length:1,groups:{x:'1'}}", "{0:'a',length:2,1:undefined,index:0}", "{0:'a',index:0,length:1,groups:null}",
  "{0:'a',index:0,length:1,groups:undefined}", "function(){}", "new String('x')", "Object.assign(['a','b'],{index:1,groups:{x:'GX'}})", "{0:'a',get index(){L.push('gi');return 0},length:1}",
  "{get 0(){L.push('g0');return 'a'},index:0,length:1}", "{0:'a',index:0,length:{valueOf(){L.push('len');return 1}}}", "new Proxy(['a'],{get(t,k){L.push('p'+String(k));return Reflect.get(t,k)}})",
  "{0:'a',index:{valueOf(){L.push('iv');return 1}},length:1}", "{0:'a',index:0,length:1,groups:1}", "{0:undefined,index:0,length:1}", "{0:1,index:0,length:1}", "{0:'a',index:0,length:-1}", "{0:'a',index:0,length:2**53}",
];
const SYMS = {
  match: (r) => `RegExp.prototype[Symbol.match].call(${r},'aab')`,
  matchAll: (r) => `Array.from(RegExp.prototype[Symbol.matchAll].call(${r},'aab'))`,
  replaceS: (r) => `RegExp.prototype[Symbol.replace].call(${r},'aab','[$&|$1|$<x>|$$]')`,
  replaceF: (r) => `RegExp.prototype[Symbol.replace].call(${r},'aab',function(){L.push('f'+arguments.length+':'+typeof arguments[arguments.length-1]);return 'R'})`,
  search: (r) => `RegExp.prototype[Symbol.search].call(${r},'aab')`,
  split: (r) => `RegExp.prototype[Symbol.split].call(${r},'aab')`,
  splitLim: (r) => `RegExp.prototype[Symbol.split].call(${r},'aab',2)`,
  test: (r) => `RegExp.prototype.test.call(${r},'aab')`,
};
const mkReal = (fl, execExpr) => `var r=new RegExp('a',${q(fl)});r.exec=${execExpr};`;
const mkPlain = (fl, execExpr) => `var r={flags:${q(fl)},global:${fl.includes("g")},unicode:${fl.includes("u")},sticky:${fl.includes("y")},lastIndex:0,exec:${execExpr}};`;
for (const res of RES) for (const fl of ["", "g", "gu", "y"]) for (const m of Object.keys(SYMS)) {
  add(`${mkReal(fl, `E(${res},${res})`)}return ${SYMS[m]("r")}`);
  add(`${mkPlain(fl, `E(${res},${res})`)}return ${SYMS[m]("r")}`);
}
// exec lançando exceção, ou não chamável.
for (const execExpr of ["function(){L.push('x');throw new RangeError('boom')}", "function(){throw 1}", "function(){throw undefined}", "1", "'x'", "null", "undefined", "{}", "true", "Symbol()", "{call(){}}", "class{}", "async function(){}", "function*(){}", "()=>null", "()=>({0:'a',index:0,length:1})", "new Proxy(function(){return null},{})", "Math.abs", "Object", "function(){return arguments.length}"]) {
  for (const fl of ["", "g"]) for (const m of Object.keys(SYMS)) {
    add(`${mkReal(fl, execExpr)}return ${SYMS[m]("r")}`);
    add(`${mkPlain(fl, execExpr)}return ${SYMS[m]("r")}`);
  }
}
// exec como getter registrando a leitura, e sem exec nenhum em objeto comum.
for (const g of ["function(){L.push('getexec');return E(['a'])}", "function(){L.push('getexec');throw new TypeError('gx')}", "function(){L.push('getexec');return undefined}", "function(){L.push('getexec');return 5}"]) {
  for (const fl of ["", "g", "y"]) for (const m of Object.keys(SYMS)) {
    add(`var r=new RegExp('a',${q(fl)});Object.defineProperty(r,'exec',{get:${g}});return ${SYMS[m]("r")}`);
    add(`var r={flags:${q(fl)},global:${fl.includes("g")},lastIndex:0};Object.defineProperty(r,'exec',{get:${g}});return ${SYMS[m]("r")}`);
  }
}
for (const fl of ["", "g", "y"]) for (const m of Object.keys(SYMS)) add(`var r={flags:${q(fl)},global:${fl.includes("g")},lastIndex:0};return ${SYMS[m]("r")}`);
// flags em objeto comum: valores exóticos de `flags` lidos pelos métodos.
for (const fv of ["'g'", "''", "'gu'", "'y'", "'gy'", "'xyz'", "'G'", "{toString(){L.push('fts');return 'g'}}", "undefined", "null", "Symbol()", "1", "{toString(){throw new RangeError('fl')}}", "'gimsuyd'", "'v'"]) {
  for (const m of ["match", "matchAll", "replaceS", "split", "search", "test"]) for (const res of ["null", "{0:'a',index:0,length:1}"]) {
    add(`var r={lastIndex:0,exec:E(${res},${res}),get flags(){L.push('flags');return ${fv}}};return ${SYMS[m]("r")}`);
    add(`var r={lastIndex:0,exec:E(${res},${res}),get flags(){L.push('flags');return ${fv}},get global(){L.push('global');return true},get unicode(){L.push('unicode');return false}};return ${SYMS[m]("r")}`);
  }
}

// ---- Coerções de argumentos e ordem de conversão.
const ARGS = ["undefined", "null", "1", "Symbol('a')", "{toString(){L.push('ts');return 'aab'}}", "{toString(){L.push('ts');throw new RangeError('ts')}}", "{[Symbol.toPrimitive](h){L.push(h);return 'aab'}}", "1n", "[]", "{valueOf(){L.push('vo');return 'a'},toString:undefined}"];
for (const a of ARGS) for (const fl of ["", "g"]) {
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype[Symbol.match].call(r,${a})`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype[Symbol.search].call(r,${a})`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype[Symbol.split].call(r,${a})`);
  add(`var r=new RegExp('a',${q(fl)});return Array.from(RegExp.prototype[Symbol.matchAll].call(r,${a}))`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype[Symbol.replace].call(r,${a},'x')`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype[Symbol.replace].call(r,'aab',${a})`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype.test.call(r,${a})`);
  add(`var r=new RegExp('a',${q(fl)});return RegExp.prototype.exec.call(r,${a})`);
  add(`var r=new RegExp('a',${q(fl)});r.exec=E(null);return RegExp.prototype[Symbol.replace].call(r,${a},${a})`);
}
for (const lim of ["undefined", "0", "1", "-1", "2**32", "2**32+1", "'2'", "NaN", "{valueOf(){L.push('lim');return 1}}", "Symbol()", "1n", "null", "Infinity", "-0", "1.9", "{valueOf(){throw new RangeError('l')}}"])
  for (const p of ["a", "", "(a)", "x", "(?:)"]) for (const fl of ["", "u", "y"])
    add(`return RegExp.prototype[Symbol.split].call(new RegExp(${q(p)},${q(fl)}),'aabaa',${lim})`);
for (const rep of ["function(){L.push('f');return {toString(){L.push('rts');return 'Z'}}}", "function(){L.push('f');throw new RangeError('rf')}", "{toString(){L.push('ts');return '$&$&'}}", "'$<x>'", "'$<'", "'$0$1$2$00$01$10'", "function(m,p1,off,str){return [m,p1,off,str].join('|')}"])
  for (const res of ["{0:'a',index:0,length:1}", "{0:'a',1:'b',index:1,length:2,groups:{x:'GX'}}", "{0:'a',1:undefined,index:2,length:2}", "{0:'aa',index:0,length:1}", "{0:'a',index:5,length:1}"])
    for (const fl of ["", "g"]) add(`var r=new RegExp('a',${q(fl)});r.exec=E(${res},${res});return RegExp.prototype[Symbol.replace].call(r,'aab',${rep})`);

// ---- lastIndex coagido, não gravável e acessor.
const LI = ["-1", "1.5", "'2'", "NaN", "Infinity", "2**53", "2**53-1", "{valueOf(){L.push('livo');return 1}}", "{valueOf(){throw new RangeError('li')}}", "Symbol()", "1n", "null", "undefined", "true", "-0", "2**32", "'x'", "[]", "{toString(){L.push('lits');return '2'}}"];
const BUILTIN = {
  exec: (r) => `${r}.exec('aab')`,
  test: (r) => `${r}.test('aab')`,
  match: (r) => `${r}[Symbol.match]('aab')`,
  replace: (r) => `${r}[Symbol.replace]('aab','_')`,
  search: (r) => `${r}[Symbol.search]('aab')`,
  split: (r) => `${r}[Symbol.split]('aab')`,
  matchAll: (r) => `(function(){var it=${r}[Symbol.matchAll]('aab');return Array.from(it).length+':'+${r}.lastIndex})()`,
};
for (const li of LI) for (const fl of ["", "g", "y", "gy", "gu"]) for (const m of Object.keys(BUILTIN)) {
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${li};var v;try{v=${BUILTIN[m]("r")}}catch(e){v='throw '+e.name+': '+e.message}return [v,S(r.lastIndex)]`);
}
for (const li of ["0", "2"]) for (const how of ["Object.defineProperty(r,'lastIndex',{writable:false})", "Object.freeze(r)"])
  for (const fl of ["", "g", "y", "gy", "gu"]) for (const p of ["a", "^", "", "z"]) for (const m of Object.keys(BUILTIN)) {
    add(`var r=new RegExp(${q(p)},${q(fl)});r.lastIndex=${li};${how};var v;try{v=${BUILTIN[m]("r")}}catch(e){v='throw '+e.name+': '+e.message}return [v,S(r.lastIndex),Object.getOwnPropertyDescriptor(r,'lastIndex').writable]`);
  }
for (const get of ["1", "-1", "'2'", "{valueOf(){L.push('gvo');return 1}}", "NaN", "Symbol()", "function(){}"]) for (const set of ["", "set:function(v){L.push('set '+v)}", "set:function(v){throw new RangeError('set')}"]) for (const fl of ["", "g"])
  for (const m of ["match", "replaceS", "search", "test", "split"]) {
    add(`var r={flags:${q(fl)},global:${fl.includes("g")},exec:E({0:'a',index:0,length:1},{0:'',index:0,length:1}),get lastIndex(){L.push('get');return ${get}}${set ? "," + set : ""}};return ${SYMS[m]("r")}`);
  }
for (const fl of ["", "g"]) for (const before of ["0", "3", "-1", "'x'"]) {
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${before};r.exec=function(s){L.push('exec '+this.lastIndex);this.lastIndex=5;return null};return [RegExp.prototype[Symbol.search].call(r,'aab'),r.lastIndex]`);
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${before};r.exec=function(s){L.push('exec '+this.lastIndex);return {0:'a',index:2,length:1}};return [RegExp.prototype[Symbol.search].call(r,'aab'),r.lastIndex]`);
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${before};Object.defineProperty(r,'lastIndex',{writable:false});return RegExp.prototype[Symbol.search].call(r,'aab')`);
}

// ---- E. RegExp(pattern, flags) com pattern exótico.
const PATS = [
  "/a/g", "/a/", "new RegExp('b','i')", "Object.assign(/c/g,{constructor:Object})", "Object.assign(/d/,{[Symbol.match]:false})", "{[Symbol.match]:true,get source(){L.push('source');return 'e'},get flags(){L.push('flags');return 'g'}}",
  "{[Symbol.match]:true,source:'e',constructor:RegExp}", "{[Symbol.match]:false,toString(){return 'f'}}", "{source:'x',flags:'g',toString(){return 'ts'}}", "{[Symbol.match]:1,source:'one',flags:'i'}", "{[Symbol.match]:'x',source:'str'}",
  "Object.assign(/h/,{source:'zz'})", "Object.assign(/h/g,{flags:'i'})", "new Proxy(/p/g,{})", "new Proxy({[Symbol.match]:true,source:'q',flags:'m'},{})", "new (class K extends RegExp{})('k','y')", "Object.setPrototypeOf(/r/g,null)",
  "'str'", "undefined", "null", "1", "Symbol()", "[]", "/(?:)/", "Object.create(RegExp.prototype)", "Object.assign(/n/g,{lastIndex:4})", "{[Symbol.match]:true,source:undefined,flags:undefined}", "{[Symbol.match]:true,source:{toString(){L.push('sts');return 's'}},flags:{toString(){L.push('fts');return 'i'}}}",
  "{[Symbol.match]:true}", "/a/dgimsy", "/a/v", "Object.assign(/c/,{constructor:RegExp})", "(()=>{var r=/m/g;r.constructor=function(){};return r})()", "(()=>{var r=/m/g;r.constructor=undefined;return r})()", "(()=>{var r=/m/g;Object.defineProperty(r,'constructor',{get(){L.push('ctor');return RegExp}});return r})()",
];
const FARGS = ["", "undefined", "null", "''", "'g'", "'gi'", "'z'", "'gg'", "{toString(){L.push('f');return 'm'}}", "Symbol()", "'v'", "'uv'", "'dgimsuy'", "1", "{toString(){throw new RangeError('fa')}}"];
for (const p of PATS) for (const f of FARGS) {
  const a = f === "" ? "" : "," + f;
  add(`var p=${p};var r=RegExp(p${a});return [r===p,r.source,r.flags,r.lastIndex,Object.getPrototypeOf(r)===RegExp.prototype]`);
  add(`var p=${p};var r=new RegExp(p${a});return [r===p,r.source,r.flags,r.lastIndex,Object.getPrototypeOf(r)===RegExp.prototype]`);
}
for (const p of PATS) {
  add(`class A extends RegExp{};var r=new A(${p});return [r instanceof A,r.source,r.flags]`);
  add(`class A extends RegExp{};var r=new A(${p},'s');return [r instanceof A,r.source,r.flags]`);
  add(`class A extends RegExp{};var p=${p};return [A(p)===p,RegExp.call(Object.create(A.prototype),p)===p]`);
  add(`var r=Reflect.construct(RegExp,[${p}],Object);return [Object.getPrototypeOf(r)===Object.prototype,typeof r.exec]`);
  add(`var r=Reflect.construct(RegExp,[${p}],function(){}.bind());return Object.getPrototypeOf(r)===RegExp.prototype`);
  add(`var nt=function(){}.bind();Object.defineProperty(nt,'prototype',{get(){L.push('proto');return RegExp.prototype}});var r=Reflect.construct(RegExp,[${p}],nt);return r.source`);
  add(`var p=${p};var r=RegExp(p);return Object.is(r,p)`);
}
// Symbol.match falso/verdadeiro nos métodos de String.
for (const sm of ["false", "0", "''", "null", "undefined", "true", "1", "'x'", "{}"]) for (const m of ["includes", "startsWith", "endsWith"]) {
  add(`var r=/a/;r[Symbol.match]=${sm};return 'xaay'.${m}(r)`);
  add(`var o={[Symbol.match]:${sm},toString(){return 'a'}};return 'xaay'.${m}(o)`);
}
for (const sm of ["false", "undefined", "true"]) {
  add(`var r=/a/g;r[Symbol.match]=${sm};return RegExp(r)===r`);
  add(`var r=/a/g;r[Symbol.match]=${sm};return new RegExp(r).flags`);
  add(`var r=/a/g;r[Symbol.match]=${sm};return 'aXa'.replaceAll(r,'_')`);
  add(`var r=/a/;r[Symbol.match]=${sm};return Array.from('aXa'.matchAll(r))`);
}
for (const body of ["'a'.match(/a/)", "RegExp(/a/g)===undefined", "new RegExp(/a/g).flags", "'xax'.includes(/a/)", "RegExp(/a/g,'i').flags"])
  add(`var d=Object.getOwnPropertyDescriptor(RegExp.prototype,Symbol.match);delete RegExp.prototype[Symbol.match];try{return ${body}}finally{Object.defineProperty(RegExp.prototype,Symbol.match,d)}`);

// ---- F. compile() legado.
const CRECV = [
  "/a/g", "/a/", "new (class X extends RegExp{})('a')", "Object.create(RegExp.prototype)", "{}", "new Proxy(/a/,{})", "Object.freeze(/a/)",
  "(()=>{var r=/a/g;Object.defineProperty(r,'lastIndex',{writable:false});return r})()", "(()=>{var r=/a/g;r.lastIndex=3;return r})()", "RegExp.prototype", "(()=>{var r=/a/;r.exec=function(){return null};return r})()",
  "(()=>{class X extends RegExp{};var r=new X('a');r.compile=RegExp.prototype.compile;return r})()", "undefined", "null", "'a'",
];
const CARGS = ["", "'b'", "'b','i'", "'b',undefined", "/c/g", "/c/g,undefined", "/c/g,'i'", "undefined,'g'", "null", "'(',''", "'a','z'", "{toString(){L.push('ts');return 'o'}},{toString(){L.push('fs');return 'm'}}",
  "{source:'q',flags:'g',[Symbol.match]:true}", "Symbol()", "'x','v'", "'x','uv'", "new Proxy(/p/g,{})", "Object.assign(/r/,{source:'ss'})", "'a','gg'", "'[','u'", "undefined", "undefined,undefined", "1,2", "'a',null", "new (class Y extends RegExp{})('y','g')"];
for (const r of CRECV) for (const a of CARGS) {
  add(`var r=${r};var res;try{res=r.compile(${a})}catch(e){res=e.name+': '+e.message}return [res===r?'same':res,F(r)]`);
  add(`var r=${r};var res;try{res=RegExp.prototype.compile.call(r,${a})}catch(e){res=e.name+': '+e.message}return [res===r?'same':res,F(r)]`);
}
for (const fl of ["g", "y", "gy", ""]) for (const li of ["0", "3"]) for (const a of ["'a'", "'a','g'", "/a/"]) {
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${li};r.compile(${a});return [r.lastIndex,r.flags,r.exec('aab')]`);
  add(`var r=new RegExp('a',${q(fl)});r.lastIndex=${li};Object.defineProperty(r,'lastIndex',{writable:false});try{r.compile(${a})}catch(e){return e.name+': '+e.message+' '+r.lastIndex+r.flags}return [r.lastIndex,r.flags]`);
}

// ---- H. Subclasses com getters, exec e Symbol.species sobrescritos.
const SUBMEMBERS = [
  "get flags(){L.push('flags');return 'g'}", "get global(){L.push('global');return true}", "exec(s){L.push('exec');return super.exec(s)}", "exec(s){L.push('exec');return null}",
  "get unicode(){L.push('unicode');return true}", "get sticky(){L.push('sticky');return true}", "constructor(p,f){super(p,f);L.push('ctor '+f)}", "get source(){L.push('source');return 'zz'}",
  "get flags(){L.push('flags');return undefined}", "get flags(){L.push('flags');return 'y'}", "get lastIndex(){return 0}", "toString(){return 'custom'}", "static get [Symbol.species](){L.push('species');return RegExp}",
];
for (const member of SUBMEMBERS) for (const fl of ["g", "", "y", "gu"]) for (const m of Object.keys(SYMS)) {
  add(`class A extends RegExp{${member}};var r=new A('a',${q(fl)});return ${SYMS[m]("r")}`);
  add(`class A extends RegExp{${member}};var r=new A('a',${q(fl)});return [String(r),r.source,r.flags]`);
}
for (const sp of ["Object", "RegExp", "null", "undefined", "1", "class B extends RegExp{}", "function(){return {exec:E(null),flags:'',lastIndex:0}}", "function(){L.push('spc');return /z/y}", "{}", "function(){}"]) {
  for (const fl of ["", "g", "y"]) for (const m of ["split", "matchAll", "replaceS", "match"]) {
    add(`class A extends RegExp{static get [Symbol.species](){L.push('species');return ${sp}}};var r=new A('a',${q(fl)});return ${SYMS[m]("r")}`);
  }
}
for (const c of ["undefined", "null", "1", "{}", "{[Symbol.species]:function(p,f){L.push('c '+f);return /a/g}}", "{[Symbol.species]:null}", "{[Symbol.species]:undefined}", "{[Symbol.species]:1}", "function(){}", "'str'", "RegExp", "Object"]) {
  for (const fl of ["", "g", "y"]) for (const m of ["split", "matchAll", "replaceS"]) {
    add(`var r=new RegExp('a',${q(fl)});r.constructor=${c};return ${SYMS[m]("r")}`);
  }
}

// ---- I. Proxy de RegExp com get trap.
const key = "typeof k==='symbol'?k.toString():k";
const HANDLERS = [
  "{}", `{get(t,k,r){L.push(${key});return Reflect.get(t,k,t)}}`, `{get(t,k,r){L.push(${key});var v=Reflect.get(t,k,t);return typeof v==='function'?v.bind(t):v}}`, `{get(t,k,r){L.push(${key});return Reflect.get(t,k,r)}}`,
  `{get(t,k,r){L.push(${key});return k==='exec'?E(null):Reflect.get(t,k,t)}}`, `{has(t,k){L.push('has '+String(k));return Reflect.has(t,k)},getOwnPropertyDescriptor(t,k){L.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}}`,
];
const POPS = [
  "p.exec('aXa')", "p.test('aXa')", "String(p)", "p[Symbol.match]('aXa')", "p[Symbol.replace]('aXa','_')", "p[Symbol.search]('aXa')", "p[Symbol.split]('aXa')", "Array.from(p[Symbol.matchAll]('aXa'))", "p.flags", "p.source", "p.global", "p.compile('b')",
  "RegExp.prototype.exec.call(p,'aXa')", "RegExp.prototype.test.call(p,'aXa')", "'aXa'.match(p)", "'aXa'.replace(p,'_')", "'aXa'.replaceAll(p,'_')", "'aXa'.split(p)", "'aXa'.search(p)", "Array.from('aXa'.matchAll(p))", "RegExp(p).source", "new RegExp(p,'i').flags",
  "'aXa'.includes(p)", "p.lastIndex", "(p.lastIndex=2,p.lastIndex)", "Object.prototype.toString.call(p)", "p instanceof RegExp", "p.constructor===RegExp", "Object.keys(p)", "JSON.stringify(p)",
];
for (const h of HANDLERS) for (const pat of ["/a/g", "/a/", "/(?<x>a)/y"]) for (const op of POPS) add(`var p=new Proxy(${pat},${h});return ${op}`);

// ---- J. Receptores não-objeto nos métodos de Symbol.
for (const r of ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n", "function(){}", "[]"]) for (const m of Object.keys(SYMS)) add(`return ${SYMS[m](r)}`);
add(`return RegExp.prototype[Symbol.matchAll].call({flags:'g',constructor:{[Symbol.species]:function(){return {lastIndex:0,exec:E(null),flags:'g'}}}},'a').toString()`);
add(`return Object.prototype.toString.call(RegExp.prototype[Symbol.matchAll].call(/a/g,'a'))`);
add(`var it=RegExp.prototype[Symbol.matchAll].call(/a/g,'aa');return [it.next().value,it.next().value,it.next().done,typeof it[Symbol.iterator]]`);
add(`var it=RegExp.prototype[Symbol.matchAll].call(/a/,'aa');return [it.next().value,it.next().done]`);

// ---- Execução, com concorrência. Cada programa roda num bun filho novo.
function allSources() {
  const known = new Set();
  for (const file of fs.readdirSync(GOLDEN_DIR)) {
    if (!file.endsWith(".tsv") || file.startsWith("regexp_receiver_bun")) continue;
    const name = file.replace(/_bun\.tsv$|\.tsv$/, "");
    try {
      for (const row of readRows(name, fs.readFileSync(path.join(GOLDEN_DIR, file), "utf8"))) known.add(row.source);
    } catch (e) {
      // golden em formato próprio: ignora
    }
  }
  return known;
}

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", () => {});
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve(code === 0 ? decodeResult(out) : null);
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const known = allSources();
  let unique = [...programs].filter((p) => !usesHostApi(p) && !known.has(p));
  // Sem travessão literal e sem substituto isolado na fonte.
  unique = unique.filter((p) => !/[\u2013\u2014]/.test(p) && !/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(p) && !/[\t]/.test(p.slice(PRELUDE.length)));
  const results = new Array(unique.length);
  let next = 0;
  const workers = Array.from({ length: 8 }, async () => {
    while (next < unique.length) {
      const i = next++;
      results[i] = await runChild(unique[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < unique.length; i++) {
    const result = results[i];
    if (
      result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result) || /[\u2013\u2014\n\r\t]/.test(result) || result.length > 6000 ||
      /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(result)
    ) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(unique[i].slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
      continue;
    }
    kept++;
    lines.push({ source: unique[i], result });
  }
  process.stdout.write(emitFactored("regexp_receiver", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
})();
