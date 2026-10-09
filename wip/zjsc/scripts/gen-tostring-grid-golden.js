// Gera tests/golden/tostring_grid_bun.tsv: Object.prototype.toString e String(x)/template/`+` em grade, medido no bun.
// Seções:
//  1. grade de valores (primitivos, boxed, todos os built-ins e instâncias, Proxy, revogados, arguments, subclasses de
//     Error, classes com toStringTag estático, objetos com toString/valueOf/Symbol.toPrimitive) x variantes de
//     Symbol.toStringTag (próprio, herdado, número, symbol, getter que lança, getter string, vazio, undefined, objeto,
//     removido, "Function", "Error", getter que lança no protótipo) x operações (String, template, `+`, Object.prototype
//     .toString.call, Array.prototype.toString via [x].toString());
//  2. ordem das armadilhas de um Proxy que registra tudo;
//  3. Function.prototype.toString, name e length de todos os built-ins enumerados no próprio bun (estáticos, protótipos,
//     getters e setters), mais bound e Proxy deles; bound e Proxy de funções do usuário;
//  4. Symbol.prototype.description e toString;
//  5. Array.prototype.toString com join que não é função, em grade de receptores.
// Cada programa roda num bun filho novo (sem APIs de host, resultado em globalThis.R). Programas iguais a uma fonte já
// presente em tests/golden/*.tsv são descartados.
// Uso: bun scripts/gen-tostring-grid-golden.js > /dev/null  (grava o tsv e o prelúdio em tests/golden)
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { knownPrograms, emitFactored, stepSampler, GOLDEN_DIR } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  // JSON.stringify escapa o surrogate solitário (`\ud800`); escrever a string crua em UTF-8 o trocaria por U+FFFD.
  process.stdout.write(JSON.stringify(String(globalThis.R)));
  process.exit(0);
}

const PRELUDE =
  'function T(f){try{return String(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  "function tg(o,v){\n" +
  '  if(v===0||o===null||o===undefined)return o;\n' +
  '  var prim=typeof o!=="object"&&typeof o!=="function";\n' +
  "  var K=Symbol.toStringTag,P=prim?Object.getPrototypeOf(o):o,PR=prim?P:Object.getPrototypeOf(o);\n" +
  "  function def(t,d){Object.defineProperty(t,K,Object.assign({configurable:true},d))}\n" +
  "  switch(v){\n" +
  '    case 1:def(P,{value:"Own"});break;\n' +
  '    case 2:def(PR||P,{value:"Proto"});break;\n' +
  "    case 3:def(P,{value:5});break;\n" +
  '    case 4:def(P,{value:Symbol("s")});break;\n' +
  '    case 5:def(P,{get(){throw new RangeError("tag")}});break;\n' +
  '    case 6:def(P,{get(){return "G"}});break;\n' +
  '    case 7:def(P,{value:""});break;\n' +
  "    case 8:def(P,{value:undefined});break;\n" +
  '    case 9:def(P,{value:{toString(){return "OBJ"}}});break;\n' +
  "    case 10:if(PR)delete PR[K];delete P[K];break;\n" +
  '    case 11:def(P,{value:"Function"});break;\n' +
  '    case 12:def(P,{value:"Error"});break;\n' +
  '    case 13:def(PR||P,{get(){throw new EvalError("pt")}});break;\n' +
  "  }\n" +
  "  return o;\n" +
  "}\n";

const FULL = ["S", "L", "C", "O", "A"];
const OBJ_ONLY = ["O"];

// [expressão, operações]
const bases = [];
const add = (e, ops) => bases.push([e, ops || FULL]);
[
  "{}", "[]", "[1,[2,3]]", "[null,undefined]", "function(){}", "()=>1", "class A{}", "async function(){}", "function*(){}",
  "async function*(){}", "async()=>1", "/x/g", "new Map", "new Set", "new WeakMap", "new WeakSet", "new WeakRef({})",
  "new FinalizationRegistry(()=>{})", "Promise.resolve(1)", 'new Error("m")', 'new TypeError("t")', 'new RangeError("r")',
  'new EvalError("e")', 'new URIError("u")', 'new SyntaxError("s")', 'new ReferenceError("f")', 'new AggregateError([1],"ag")',
  'new (class E1 extends Error{})("m")',
  'new (class E2 extends TypeError{constructor(m){super(m);this.name="Custom"}})("cm")',
  'Object.assign(new Error("m"),{name:""})', 'new Error("")', 'new Error("x",{cause:1})', "new ArrayBuffer(2)",
  "new SharedArrayBuffer(2)", "new DataView(new ArrayBuffer(1))", "new Int8Array(2)", "new Uint8Array(2)",
  "new Uint8ClampedArray(2)", "new Int16Array(1)", "new Uint16Array(1)", "new Int32Array(1)", "new Uint32Array(1)",
  "new Float32Array(1)", "new Float64Array(1)", "new BigInt64Array(1)", "new BigUint64Array(1)", "Math", "JSON", "Reflect",
  "Atomics", "Intl", "WebAssembly", "new Intl.Collator", "new Intl.NumberFormat", "new Intl.PluralRules",
  "new Intl.ListFormat", "new Intl.Segmenter", 'new Intl.Locale("en")', "new Intl.RelativeTimeFormat",
  "new Intl.DateTimeFormat", "[][Symbol.iterator]()", "new Map().entries()", "new Set().values()", '""[Symbol.iterator]()',
  '"a".matchAll(/a/g)', "(function*(){})()", "(async function*(){})()", "[].values().map(x=>x)",
  "(function(){return arguments})(1,2)", '(function(){"use strict";return arguments})(1)', "Object.create(null)",
  "Object.create([])", "Object.create(Array.prototype)", "Object.create(Function.prototype)", "Object.create(Error.prototype)",
  "Object.create(Map.prototype)", "Object.setPrototypeOf(function(){},null)", "Object.setPrototypeOf([],null)",
  "Object.setPrototypeOf(/x/,null)", 'Object.setPrototypeOf(new Error("m"),null)', "new (class extends Array{})",
  "new (class extends Map{})", "new (class extends Promise{constructor(e){super(e)}})(()=>{})", "new (class Foo{})",
  'class SC{static get [Symbol.toStringTag](){return "ST"}}', 'new (class SC{static get [Symbol.toStringTag](){return "ST"}})',
  'class SC2{get [Symbol.toStringTag](){return "IT"}}', 'new (class SC2{get [Symbol.toStringTag](){return "IT"}})',
  'Object.assign(function(){},{[Symbol.toStringTag]:"FT"})', 'Object.assign([],{[Symbol.toStringTag]:"AT"})',
  "new Proxy([],{})", "new Proxy({},{})", "new Proxy(function(){},{})", "new Proxy(class{},{})", "new Proxy(new Proxy([],{}),{})",
  'new Proxy(new Error("m"),{})', "new Proxy(new Map,{})", "new Proxy(/x/,{})", "new Proxy(()=>1,{})",
  "new Proxy(async function(){},{})", "new Proxy(new Number(1),{})", "new Proxy((function(){return arguments})(1),{})",
  "new Proxy(Object.create(null),{})", 'new Proxy({},{get:(t,k)=>k===Symbol.toStringTag?"PG":undefined})',
  'new Proxy([],{get:(t,k)=>k===Symbol.toStringTag?"PA":Reflect.get(t,k)})',
  'new Proxy({},{get(){throw new RangeError("trap")}})', "new Proxy({},{getPrototypeOf(){return Array.prototype}})",
  'new Proxy({},{getPrototypeOf(){throw new EvalError("gp")}})', "new Proxy([],{getPrototypeOf(){return null}})",
  'new Proxy({toString(){return "TS"}},{})', "new Proxy(function(){},{get:()=>undefined})",
  "(()=>{var r=Proxy.revocable({},{});r.revoke();return r.proxy})()",
  "(()=>{var r=Proxy.revocable([],{});r.revoke();return r.proxy})()",
  "(()=>{var r=Proxy.revocable(function(){},{});r.revoke();return r.proxy})()",
  "(()=>{var r=Proxy.revocable(class{},{});r.revoke();return r.proxy})()",
  "(()=>{var p=new Proxy([],{});var r=Proxy.revocable(p,{});r.revoke();return r.proxy})()",
  "Object(1)", "new Number(NaN)", "Object(-0)", 'Object("s")', 'new String("")', "Object(true)", "Object(false)", "Object(1n)",
  'Object(Symbol("q"))',
  "undefined", "null", "true", "false", "0", "-0", "1.5", "NaN", "Infinity", "1e21", '"str"', '""', "10n", "-0n",
  'Symbol("a")', "Symbol()", "Symbol.iterator", 'Symbol.for("k")',
  '{toString(){return "TS"}}', "{valueOf(){return 7}}", "{[Symbol.toPrimitive](h){return h}}",
  "{toString:null,valueOf:()=>1}", "{toString:1,valueOf:2}", "{toString(){return {}},valueOf(){return {}}}",
  "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive]:null}", "{[Symbol.toPrimitive](){return {}}}",
  '{[Symbol.toPrimitive](){return Symbol("p")}}', '{toString(){throw new TypeError("ts")}}',
  '{toString(){return Symbol("x")}}', "{toString:undefined,valueOf:undefined}",
  "Object.assign(Object.create(null),{a:1})", "[1,2,3].map(String)", "[[],[[]],[null,[undefined]]]",
  "[{toString(){return 'e'}},{}]", "[1,,3]", "Object.assign([1,2],{toString:undefined})",
].forEach((e) => add(e));
add("new Date(0)", OBJ_ONLY);
add("new Date(NaN)", ["O", "S", "L", "C", "A"]);
add("Object.assign(new Date(NaN),{toString:undefined})", ["S", "L", "C", "O"]);

const OPS = {
  S: (x) => "String(" + x + ")",
  L: (x) => "`${" + x + "}`",
  C: (x) => '""+' + x,
  O: (x) => "Object.prototype.toString.call(" + x + ")",
  A: (x) => "[" + x + "].toString()",
};

const exprs = [];
// Os candidatos que só entram em parte levam a densidade (`thinPool.push(passo, ...)`, 1 em `passo`) e a escolha é por hash
// do texto (`sampleByHash` dentro do `stepSampler`), nunca pelo contador da função.
const thinPool = stepSampler();
for (let bi = 0; bi < bases.length; bi++) {
  const [e, ops] = bases[bi];
  const primitive = /^(undefined|null|true|false|-?0|1\.5|NaN|Infinity|1e21|"str"|""|10n|-0n|Symbol\b.*|Symbol\.iterator)$/.test(e);
  const nullish = e === "undefined" || e === "null";
  for (let v = 0; v <= 13; v++) {
    if (v > 0 && nullish) continue;
    if (v > 0 && primitive && v === 10) continue;
    for (const op of ops) exprs.push("T(()=>" + OPS[op]("tg(" + e + "," + v + ")") + ")");
  }
}

// 2. ordem das armadilhas
const traceTargets = [
  "{}", "[]", "function(){}", 'new Error("m")', '{[Symbol.toStringTag]:"T"}', "new Map", "Object(1)", "/x/",
  "(function(){return arguments})(1)", "class{}", "Object.create(null)",
];
const traceOps = [
  (x) => "String(" + x + ")", (x) => "`${" + x + "}`", (x) => '""+' + x, (x) => "Object.prototype.toString.call(" + x + ")",
  (x) => "[" + x + "].toString()", (x) => "Array.isArray(" + x + ")", (x) => "Array.prototype.join.call(" + x + ")",
  (x) => "Function.prototype.toString.call(" + x + ")",
];
for (const t of traceTargets) {
  for (const op of traceOps) {
    exprs.push(
      "T(()=>{var l=[];var p=new Proxy(" + t + ",{get(t,k,r){l.push('get '+String(k));return Reflect.get(t,k,r)}," +
      "has(t,k){l.push('has '+String(k));return Reflect.has(t,k)},getPrototypeOf(t){l.push('gpo');return Reflect.getPrototypeOf(t)}," +
      "ownKeys(t){l.push('keys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});" +
      "var r;try{r=" + op("p") + "}catch(e){r='throw '+e.name+': '+e.message}return r+'|'+l.join()})",
    );
  }
}

// 3. Function.prototype.toString de built-ins enumerados no bun
const holders = [];
const holder = (text) => holders.push(text);
[
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Date", "RegExp", "Error", "TypeError",
  "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError", "Map", "Set", "WeakMap", "WeakSet",
  "WeakRef", "FinalizationRegistry", "Promise", "Proxy", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Int8Array",
  "Float64Array", "BigInt64Array", "Iterator",
].forEach((n) => { holder(n); holder(n + ".prototype"); });
[
  "Math", "JSON", "Reflect", "Atomics", "Intl", "WebAssembly", "Intl.Collator", "Intl.Collator.prototype", "Intl.NumberFormat",
  "Intl.NumberFormat.prototype", "Intl.DateTimeFormat", "Intl.DateTimeFormat.prototype", "Intl.PluralRules.prototype",
  "Intl.ListFormat.prototype", "Intl.Segmenter.prototype", "Intl.Locale.prototype", "Intl.RelativeTimeFormat.prototype",
  "Intl.DisplayNames.prototype", "Object.getPrototypeOf(Int8Array)", "Object.getPrototypeOf(Int8Array).prototype",
  "Object.getPrototypeOf([][Symbol.iterator]())", "Object.getPrototypeOf(new Map().entries())",
  "Object.getPrototypeOf(new Set().values())", 'Object.getPrototypeOf(""[Symbol.iterator]())',
  'Object.getPrototypeOf("a".matchAll(/a/g))', "Object.getPrototypeOf(function*(){}).prototype",
  "Object.getPrototypeOf(async function*(){}).prototype", "Object.getPrototypeOf(function*(){})",
  "Object.getPrototypeOf(async function(){})", "Object.getPrototypeOf(async function*(){})",
  "Object.getPrototypeOf(Object.getPrototypeOf(function*(){}).prototype)",
  "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)",
].forEach(holder);
const globalFns = ["parseInt", "parseFloat", "isNaN", "isFinite", "eval", "escape", "unescape", "encodeURI", "decodeURI",
  "encodeURIComponent", "decodeURIComponent", "queueMicrotask"];

const EXCLUDED_KEYS = new Set(["captureStackTrace", "prepareStackTrace", "stackTraceLimit"]);
const wellKnown = (sym) => {
  const d = sym.description;
  return typeof d === "string" && /^Symbol\.[a-zA-Z]+$/.test(d) && Symbol[d.slice(7)] === sym ? d : null;
};
function pushFunction(descExpr, part) {
  const tag = "var F=" + descExpr + "." + part + ";";
  exprs.push("T(()=>{" + tag + 'return F.name+"|"+F.length+"|"+Function.prototype.toString.call(F)})');
  thinPool.push(4, "T(()=>{" + tag + 'var B=F.bind(null,1);return B.name+"|"+B.length+"|"+Function.prototype.toString.call(B)+"|"+String(B)})');
  thinPool.push(6, "T(()=>{" + tag + 'var P=new Proxy(F,{});return Function.prototype.toString.call(P)+"|"+typeof P+"|"+String(P)})');
}
for (const h of holders) {
  let obj;
  try { obj = (0, eval)(h); } catch (e) { continue; }
  if (obj === undefined || obj === null) continue;
  for (const key of Reflect.ownKeys(obj)) {
    let keyExpr;
    if (typeof key === "symbol") {
      const name = wellKnown(key);
      if (!name) continue;
      keyExpr = name;
    } else {
      if (EXCLUDED_KEYS.has(key)) continue;
      keyExpr = JSON.stringify(key);
    }
    const d = Object.getOwnPropertyDescriptor(obj, key);
    const descExpr = "Object.getOwnPropertyDescriptor(" + h + "," + keyExpr + ")";
    if (typeof d.value === "function") pushFunction(descExpr, "value");
    if (typeof d.get === "function") pushFunction(descExpr, "get");
    if (typeof d.set === "function") pushFunction(descExpr, "set");
  }
}
for (const g of globalFns) {
  if (typeof globalThis[g] !== "function") continue;
  exprs.push("T(()=>{var F=" + g + ';return F.name+"|"+F.length+"|"+Function.prototype.toString.call(F)+"|"+String(F.bind())})');
}
const FTS = "Function.prototype.toString.call";
[
  "(function f(){}).bind()", "(function f(){}).bind().bind()", "(class A{}).bind()", "(()=>1).bind()", "(async()=>{}).bind()",
  "(function*g(){}).bind(null,1,2)", 'Object.defineProperty(function(){},"name",{value:"zz"}).bind()',
  'Object.defineProperty(function(){},"name",{value:5}).bind()', 'Object.defineProperty(function(){},"name",{value:Symbol("s")}).bind()',
  'Object.defineProperty(function(){},"name",{get(){throw new RangeError("n")}}).bind()', "Math.max.bind(null,1)",
  "Array.prototype.map.bind([])", "Function.prototype.bind.call(Function.prototype)", "Function.prototype.bind.call(Math.min,null,1,2,3)",
  "new Proxy(function f(){},{})", "new Proxy(class A{},{})", "new Proxy(new Proxy(function(){},{}),{})", "new Proxy(Math.max,{})",
  "new Proxy(()=>1,{}).bind()", "new Proxy(function(){},{}).bind(null)", "new Proxy(Math.max,{}).bind()",
  "new Proxy(Object.getOwnPropertyDescriptor(Map.prototype,'size').get,{})", "new Proxy(function(){},{get:()=>1})",
  "new Proxy(function(){},{apply:()=>1})", "new Proxy(async function(){},{})", "new Proxy(function*(){},{})",
  "(()=>{var r=Proxy.revocable(function(){},{});r.revoke();return r.proxy})()",
  "(()=>{var r=Proxy.revocable(class{},{});r.revoke();return r.proxy})()",
  "Function.prototype", "Function.prototype.bind(null)", "Object.create(Function.prototype)", "new Proxy({},{})", "new Proxy([],{})",
  "{}", "[]", "1", '"s"', "null", "undefined", "Symbol()", "1n", "true", "/x/", "new Proxy(function(){},{}).constructor",
  "Object.setPrototypeOf(function(){},null)", "Object.setPrototypeOf(Math.max,null)", "Object.assign(function(){},{toString(){return 'x'}})",
  "Object.assign(Math.max,{toString:null})", "Reflect.construct(Function,['a','return a'])", "new Function('a','b','return a+b')",
  "Function('return 1')", "new Function", "(function(){}).constructor", "Function.prototype.toString", "Function.prototype.call",
  "Function.prototype[Symbol.hasInstance]", "Object.getOwnPropertyDescriptor(Function.prototype,'caller').get",
].forEach((e) => {
  exprs.push("T(()=>" + FTS + "(" + e + "))");
  exprs.push("T(()=>String(" + e + "))");
  exprs.push("T(()=>{var v=" + e + ';return typeof v+"|"+Object.prototype.toString.call(v)+"|"+(typeof v==="function"?v.name+"|"+v.length:"")})');
});

// 4. Symbol.prototype.description e toString
const symbolValues = [
  'Symbol("a")', "Symbol()", 'Symbol("")', "Symbol(undefined)", "Symbol(null)", "Symbol(1)", "Symbol.iterator", 'Symbol.for("k")',
  'Symbol.for("")', 'Object(Symbol("x"))', 'Object(Symbol())', 'Symbol({toString(){return "o"}})', 'Symbol("\\0 x")',
  'Symbol("\\ud800")', 'Symbol("a b")', "Symbol.asyncIterator", "Symbol.toStringTag", "Symbol.prototype", "1", "undefined", "null",
  '"s"', "{}", "Object(1)",
];
const symbolOps = [
  (x) => "Symbol.prototype.toString.call(" + x + ")",
  (x) => "Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get.call(" + x + ")",
  (x) => "Symbol.prototype.valueOf.call(" + x + ")",
  (x) => "Symbol.prototype[Symbol.toPrimitive].call(" + x + ")",
  (x) => "typeof Symbol.prototype.valueOf.call(" + x + ")",
  (x) => "String(" + x + ")",
  (x) => "(" + x + ").description",
  (x) => "(" + x + ").toString()",
  (x) => "Symbol.keyFor(" + x + ")",
  (x) => "Object.prototype.toString.call(" + x + ")",
  (x) => "JSON.stringify(Object.getOwnPropertyDescriptor(Object(" + x + "),'description'))",
  (x) => "typeof (" + x + ").description",
];
for (const v of symbolValues) for (const op of symbolOps) exprs.push("T(()=>" + op(v) + ")");
for (const code of [
  'var d=Object.getOwnPropertyDescriptor(Symbol.prototype,"description");return typeof d.get+"|"+d.set+"|"+d.enumerable+"|"+d.configurable',
  'return Object.getOwnPropertyDescriptor(Symbol.prototype,"description").get.name',
  'return Symbol.prototype.description',
  'return Symbol.prototype.toString.name+Symbol.prototype.toString.length',
  'return Symbol.prototype[Symbol.toStringTag]',
  'return Object.getOwnPropertyDescriptor(Symbol.prototype,Symbol.toPrimitive).writable+""+Symbol.prototype[Symbol.toPrimitive].name',
  'var s=Symbol("a");s.description="b";return s.description',
  '"use strict";var s=Symbol("a");s.description="b";return s.description',
  'var s=Symbol("d");return Object.getOwnPropertyNames(Object(s)).join()+"|"+Object.getOwnPropertySymbols(Object(s)).length',
  'var s=Symbol("d");return s.toString===Symbol.prototype.toString',
  'Object.defineProperty(Symbol.prototype,"description",{get(){return "over"}});return Symbol("a").description',
  'Symbol.prototype.toString=function(){return "own"};return String(Object(Symbol("a")))',
  'Symbol.prototype.toString=function(){return "own"};return Symbol("a").toString()',
  'Symbol.prototype[Symbol.toPrimitive]=undefined;return String(Object(Symbol("a")))',
  'Symbol.prototype[Symbol.toPrimitive]=undefined;return Object(Symbol("a"))+""',
  'delete Symbol.prototype[Symbol.toPrimitive];return String(Object(Symbol("a")))',
  'delete Symbol.prototype[Symbol.toPrimitive];return Object(Symbol("a"))+""',
  'delete Symbol.prototype[Symbol.toPrimitive];Symbol.prototype.toString=function(){return "ts"};return String(Object(Symbol("a")))+(Object(Symbol("a"))+"")',
  'Symbol.prototype[Symbol.toStringTag]="Custom";return Object.prototype.toString.call(Symbol())+Object.prototype.toString.call(Object(Symbol()))',
  'return Object.prototype.toString.call(Symbol())+Object.prototype.toString.call(Object(Symbol()))',
  'return `${Object(Symbol("a")).description}`',
  'return Symbol("a").description===Symbol("a").description',
  'return new Symbol("a")',
  'return Symbol.length+Symbol.name+typeof Symbol.prototype.constructor',
  'return Symbol("a")+""',
  'return `${Symbol("a")}`',
  'return Symbol("a")+1',
  'return [Symbol("a")].join()',
  'return [Symbol("a")].toString()',
  'return String([Symbol("a")].map(String))',
  'return Object(Symbol("a"))+""',
  'return `${Object(Symbol("a"))}`',
  'return Object(Symbol("a")).toString()+String(Object(Symbol("a")))',
]) exprs.push("T(()=>{" + code + "})");

// 5. Array.prototype.toString com join que não é função
const joins = ["1", "null", "undefined", "{}", '"x"', "true", "Symbol()", "[]", "()=>'J'", "function(){return 'F'+arguments.length}",
  "Object.prototype.toString", "Array.prototype.join", "Array.prototype.toString", "function(){throw new RangeError('j')}",
  "class{}", "new Proxy(function(){return 'PX'},{})"];
const receivers = [
  (j) => "(()=>{var a=[1,2];a.join=" + j + ";return a})()",
  (j) => "{join:" + j + "}",
  (j) => "{}",
  (j) => '"str"',
  (j) => "1",
  (j) => "null",
  (j) => "undefined",
  (j) => "(()=>{var o=Object.create(null);o.join=" + j + ";return o})()",
  (j) => "Object.assign(function(){},{join:" + j + "})",
  (j) => "new Proxy([1,2],{get:(t,k)=>k==='join'?" + j + ":t[k]})",
  (j) => "(function(){arguments.join=" + j + ";return arguments})(1,2)",
  (j) => "Object.assign(new Number(3),{join:" + j + "})",
  (j) => "Object.assign(new Int8Array(2),{join:" + j + "})",
];
const joinOps = [
  (x) => "Array.prototype.toString.call(" + x + ")",
  (x) => "String(" + x + ")",
  (x) => "`${" + x + "}`",
];
for (const j of joins) {
  receivers.forEach((r, ri) => {
    const ops = ri === 0 ? joinOps : joinOps.slice(0, 1);
    for (const op of ops) exprs.push("T(()=>" + op(r(j)) + ")");
  });
  exprs.push("T(()=>{var saved=Array.prototype.join;Array.prototype.join=" + j + ";try{return String([1,2])+'|'+[3].toString()+'|'+Array.prototype.toString.call({})}finally{Array.prototype.join=saved}})");
  exprs.push("T(()=>{var saved=Array.prototype.join;Array.prototype.join=" + j + ";try{return Array.prototype.toString.call({join:" + j + "})+'|'+Object.prototype.toString.call([])}finally{Array.prototype.join=saved}})");
}
exprs.push("T(()=>{delete Array.prototype.join;return String([1,2])+'|'+Array.prototype.toString.call({})+'|'+Array.prototype.toString.call(1)})");
exprs.push("T(()=>{delete Array.prototype.join;return [1,[2,3]].toString()})");
exprs.push("T(()=>{delete Array.prototype.join;return Array.prototype.toString.call({join:Array.prototype.toString})})");
exprs.push("T(()=>{Array.prototype.join=Object.prototype.toString;return String([1])+String(new Int8Array(1))+`${[]}`})");
exprs.push("T(()=>{Object.prototype.join=()=>'OJ';return Array.prototype.toString.call({})+'|'+Array.prototype.toString.call(1)+'|'+Array.prototype.toString.call('s')})");
exprs.push("T(()=>Array.prototype.toString.call(undefined))");
exprs.push("T(()=>Array.prototype.toString.call(null))");
exprs.push("T(()=>Array.prototype.toString.length+Array.prototype.toString.name)");
exprs.push("T(()=>{var a=[1];a.join=()=>a;return String(a)})");
exprs.push("T(()=>{var a=[];a[0]=a;return String(a)+'|'+Array.prototype.toString.call(a)})");
exprs.push("T(()=>{var a=[1,2];a.toString=Array.prototype.toString;a.join=function(){return this===a}; return a+''})");

// Dedup contra os goldens vizinhos e execução
const known = new Set();
for (const program of knownPrograms("tostring_grid_bun.tsv", (file) => file !== "tostring_grid_bun.tsv")) known.add(JSON.stringify(program));

const seen = new Set();
const jobs = [];
let dup = 0;
exprs.push(...thinPool.resolve());
for (const expr of exprs) {
  const source = PRELUDE + "globalThis.R = " + expr;
  const key = JSON.stringify(source);
  if (seen.has(key)) continue;
  seen.add(key);
  if (known.has(key)) { dup++; continue; }
  jobs.push({ expr, source });
}

const TIMEOUT_MS = 8000;
const CHILDREN = 6;
const FORBIDDEN = new RegExp("/home/|/tmp/|/Users/|\\.js:\\d|\\bBun\\b|" + String.fromCharCode(8212) + "|" + String.fromCharCode(8211));

function runChild(job) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, TIMEOUT_MS);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.on("close", (code) => { clearTimeout(timer); resolve({ timedOut, code, out }); });
    child.stdin.on("error", () => {});
    child.stdin.end(job.source);
  });
}

async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await runChild(jobs[index]);
    }
  }
  await Promise.all(Array.from({ length: CHILDREN }, worker));
  let timeouts = 0;
  let dropped = 0;
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    const result = r.code === 0 ? JSON.parse(r.out) : "";
    if (r.code !== 0 || FORBIDDEN.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(jobs[i].expr).slice(0, 160) + "\n");
      continue;
    }
    rows.push({ source: jobs[i].source, result });
  }
  const text = emitFactored("tostring_grid", rows);
  fs.writeFileSync(path.join(GOLDEN_DIR, "tostring_grid_bun.tsv"), require("./golden-prelude.js").assertPublicResult(text));
  process.stderr.write("mantidos " + rows.length + ", descartados " + dropped + ", timeouts " + timeouts + ", repetidos dos goldens existentes " + dup + "\n");
}
main();
