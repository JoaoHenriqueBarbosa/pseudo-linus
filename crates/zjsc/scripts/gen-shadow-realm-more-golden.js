// Gera tests/golden/shadow_realm_more_bun.tsv: complemento de gen-shadow-realm-golden.js, medido no bun 1.4.2.
// Cobre a matriz de retorno do evaluate (primitivos, objetos, funções), erros entre realms (tipo e mensagem),
// wrappers de função (name, length, protótipo, this, new, argumentos), isolamento de globais, instanceof entre
// realms, toStringTag, subclasses, evaluate com não-string e evaluate recursivo.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-shadow-realm-more-golden.js > tests/golden/shadow_realm_more_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
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
const stmt = body => programs.push(PRELUDE + body + "\n");

// ---- 1. Retorno do evaluate: primitivos.
const primitives = [
  "1", "-0", "0", "NaN", "Infinity", "-Infinity", "1.5", "2**53", "123n", "-5n", "0n", "'abc'", "''", "'\\u0000'", "'\\ud800'", "true", "false",
  "null", "undefined", "void 0", "typeof 1", "1+1", "'a'+'b'", "[1,2].length", "Symbol.iterator === Symbol.iterator", "this === undefined", "typeof this",
  "typeof globalThis", "typeof window", "typeof process", "typeof require", "typeof console", "typeof Bun", "typeof setTimeout", "typeof queueMicrotask",
  "typeof structuredClone", "typeof fetch", "typeof ShadowRealm", "typeof Symbol", "typeof WeakRef", "typeof Intl",
];
for (const p of primitives) expr(`sr.evaluate(${q(p)})`);

// ---- 2. Retorno que não é primitivo ou função: objetos são TypeError do realm chamador.
const objects = [
  "({})", "[]", "[1]", "new Date(0)", "/a/", "new Map", "new Set", "new Error('x')", "Symbol('a')", "Symbol.iterator", "Object(1)", "Object('a')",
  "Object(1n)", "new Proxy({}, {})", "new Proxy(function(){}, {})", "Promise.resolve(1)", "new WeakRef({})", "new Uint8Array(1)", "new ArrayBuffer(1)",
  "globalThis", "Math", "JSON", "Reflect", "(function*(){})()", "arguments", "class A{}", "null", "[][Symbol.iterator]()", "Object.create(null)",
];
for (const o of objects) expr(`sr.evaluate(${q(o)})`);

// ---- 3. Funções viram wrappers.
const functions = [
  "(function(){})", "(function f(a,b){})", "(()=>1)", "(async function(){})", "(function*(){})", "(async function*(){})", "class A{}", "(class B{ constructor(a){} })",
  "Math.max", "Array", "Object", "Function", "Symbol", "parseInt", "(function(){}).bind(null)", "(function(a,b,c){}).bind(null,1)", "({m(){}}).m",
  "({get a(){return 1}}).__lookupGetter__('a')", "new Function('a','b','return a+b')", "Object.assign(function(){}, {x:1})",
  "Object.defineProperty(function(){}, 'name', {value:'zz'})", "Object.defineProperty(function(){}, 'length', {value:7})",
  "Object.defineProperty(function(){}, 'name', {get(){return 'g'}})", "Object.defineProperty(function(){}, 'length', {value:-1})",
  "Object.defineProperty(function(){}, 'length', {value:Infinity})", "Object.defineProperty(function(){}, 'length', {value:'3'})",
  "Object.defineProperty(function(){}, 'name', {value:5})", "Object.defineProperty(function(){}, 'length', {value:2.7})",
  "Object.defineProperty(function(){}, 'length', {value:NaN})", "Object.defineProperty(function(){}, 'length', {get(){throw new Error('boom')}})",
  "Object.defineProperty(function(){}, 'name', {get(){throw new Error('boom')}})", "new Proxy(function(a,b){}, {})", "Reflect.apply", "(function(){}).call",
];
for (const f of functions) {
  const g = `sr.evaluate(${q(f)})`;
  expr(g);
  expr(`typeof ${g}`);
  expr(`${g}.name`);
  expr(`${g}.length`);
  expr(`Object.getPrototypeOf(${g}) === Function.prototype`);
  expr(`Object.getOwnPropertyNames(${g}).join()`);
  expr(`${g} instanceof Function`);
}

// ---- 4. Chamar wrappers com argumentos.
const wrap = body => `sr.evaluate(${q(body)})`;
const ident = wrap("(function(x){return x})");
const args = ["1", "'s'", "null", "undefined", "true", "1n", "Symbol('a')", "{}", "[]", "()=>1", "function f(){}", "class A{}", "new Date", "Math.max", "async()=>1", "new Proxy(function(){}, {})", "new Proxy({}, {})"];
for (const a of args) {
  expr(`${ident}(${a})`);
  expr(`typeof ${ident}(${a})`);
}
const retvals = ["1", "{}", "[]", "()=>1", "null", "undefined", "Symbol('a')", "1n", "'x'", "NaN", "-0", "new Error('e')", "function(){}", "Promise.resolve(1)", "Object(1)"];
for (const r of retvals) {
  expr(`${wrap(`(function(){ return ${r} })`)}()`);
  expr(`typeof ${wrap(`(function(){ return ${r} })`)}()`);
}
expr(`${wrap("(function(){return arguments.length})")}(1,2,3)`);
expr(`${wrap("(function(a,b){return a+b})")}(1,2)`);
expr(`${wrap("(function(a,b){return a+b})")}(1)`);
expr(`${wrap("(function(a,b){return a+b})")}('x','y')`);
expr(`${wrap("(function(){return typeof this})")}()`);
expr(`${wrap("(function(){'use strict'; return typeof this})")}()`);
expr(`${wrap("(function(){return this === globalThis})")}()`);
expr(`${wrap("(function(){return this === globalThis})")}.call({})`);
expr(`${wrap("(function(){'use strict'; return this})")}.call(1)`);
expr(`${wrap("(function(){'use strict'; return this})")}.call(undefined)`);
expr(`${wrap("(function(){'use strict'; return typeof this})")}.call({})`);
expr(`${wrap("(function(){'use strict'; return typeof this})")}.call(()=>1)`);
expr(`${wrap("(function(f){return f(2)})")}(x=>x*3)`);
expr(`${wrap("(function(f){return typeof f})")}(()=>1)`);
expr(`${wrap("(function(f){return f.name})")}(function nm(){})`);
expr(`${wrap("(function(f){return f.length})")}(function(a,b){})`);
expr(`${wrap("(function(f){return f()})")}(()=>{throw new TypeError('inner')})`);
expr(`${wrap("(function(f){return f()})")}(()=>({}))`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.constructor===TypeError}})")}(()=>{throw new RangeError('r')})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.message}})")}(()=>{throw new RangeError('r')})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.message}})")}(()=>{throw 1})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.message}})")}(()=>{throw {a:1}})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.message}})")}(()=>{throw Symbol('s')})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e.message}})")}(()=>{throw null})`);
expr(`${wrap("(function(f){try{return f()}catch(e){return e instanceof TypeError}})")}(()=>{throw new Error('r')})`);

// ---- 5. Lançamentos dentro do realm viram TypeError do chamador.
const throws = ["new Error('m')", "new TypeError('m')", "new RangeError('m')", "new SyntaxError('m')", "1", "'s'", "null", "undefined", "{}", "Symbol('q')", "10n",
  "function(){}", "new Error('')", "Object.assign(new Error('a'), {name:'N'})", "{message:'obj'}", "new Proxy({}, {})", "[1,2]"];
for (const t of throws) {
  expr(`${wrap(`(function(){ throw ${t} })`)}()`);
  expr(`sr.evaluate(${q(`throw ${t}`)})`);
  stmt(`try { ${wrap(`(function(){ throw ${t} })`)}() } catch (e) { globalThis.R = S(e instanceof TypeError) + " " + S(e.constructor === TypeError) + " " + S(Object.getPrototypeOf(e) === TypeError.prototype) + " " + S(e.message); }`);
}

// ---- 6. SyntaxError do realm vira SyntaxError do chamador.
const bad = ["(", ")", "1 +", "var", "let let", "function", "{", "}", "'", "`", "/", "/(/", "a b", "1 2", "if", "return 1", "await 1", "yield 1", "import 'x'", "export default 1",
  "class", "new", "({a:1,})}", "0++", "1=1", "for(;;", "break", "continue", "x => {", "let a; let a", "const a", "'use strict'; with(a){}", "'use strict'; 010", "a?.b = 1", "super()", "new.target", "#a in {}",
  "@", "\\", " ", "1_", "0b2", "1e", "00n", "<!--", "-->", "/*", "/a/gg", "({a:1}) = 1", "async function(){}", "function*(){}", "class A { constructor(){} constructor(){} }", "label: label: 1"];
for (const b of bad) {
  expr(`sr.evaluate(${q(b)})`);
  stmt(`try { sr.evaluate(${q(b)}) } catch (e) { globalThis.R = S(e instanceof SyntaxError) + " " + S(e.constructor === SyntaxError) + " " + S(e instanceof TypeError); }`);
}

// ---- 7. Globais isolados.
stmt(`sr.evaluate("var iso = 1"); globalThis.R = S(typeof iso);`);
stmt(`sr.evaluate("globalThis.iso = 1"); globalThis.R = S(typeof iso);`);
stmt(`sr.evaluate("iso2 = 1"); globalThis.R = S(typeof iso2);`);
stmt(`sr.evaluate("let lx = 1"); globalThis.R = T(() => sr.evaluate("lx"));`);
stmt(`sr.evaluate("var vx = 1"); globalThis.R = T(() => sr.evaluate("vx"));`);
stmt(`sr.evaluate("function fx(){return 3}"); globalThis.R = T(() => sr.evaluate("fx()"));`);
stmt(`var outer = 5; globalThis.R = T(() => sr.evaluate("typeof outer"));`);
stmt(`globalThis.outer2 = 5; globalThis.R = T(() => sr.evaluate("typeof outer2"));`);
stmt(`var outer3 = 5; globalThis.R = T(() => sr.evaluate("outer3"));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); a.evaluate("var shared = 1"); globalThis.R = T(() => b.evaluate("typeof shared"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var k = 1"); globalThis.R = T(() => a.evaluate("k"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var k = 1"); globalThis.R = T(() => a.evaluate("k + 1"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var k = 1"); a.evaluate("k++"); globalThis.R = T(() => a.evaluate("k"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("let k = 1"); globalThis.R = T(() => a.evaluate("let k = 2; k"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("const k = 1"); globalThis.R = T(() => a.evaluate("var k = 2"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("globalThis === this"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("typeof globalThis.ShadowRealm"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("globalThis.R"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("typeof R"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.keys(globalThis).length"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Array')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('console')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('ShadowRealm')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('eval')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('SharedArrayBuffer')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Atomics')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('WebAssembly')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Intl')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Temporal')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('FinalizationRegistry')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('setTimeout')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('queueMicrotask')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Proxy')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Reflect')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('Iterator')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyNames(globalThis).includes('structuredClone')"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyDescriptor(globalThis,'Array').enumerable"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyDescriptor(globalThis,'Array').writable"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getOwnPropertyDescriptor(globalThis,'globalThis').writable"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("globalThis.Array = 1"); globalThis.R = T(() => Array.isArray([]));`);
stmt(`var a = new ShadowRealm(); a.evaluate("Array.prototype.zz = 1"); globalThis.R = T(() => [].zz);`);
stmt(`var a = new ShadowRealm(); a.evaluate("Object.prototype.zz = 1"); globalThis.R = T(() => ({}).zz);`);
stmt(`var a = new ShadowRealm(); a.evaluate("Function.prototype.zz = 1"); globalThis.R = T(() => (function(){}).zz);`);
stmt(`Array.prototype.zz = 1; var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("[].zz")); delete Array.prototype.zz;`);
stmt(`Object.prototype.zz = 1; var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("({}).zz")); delete Object.prototype.zz;`);
stmt(`Function.prototype.zz = 1; var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("typeof (function(){}).zz")); delete Function.prototype.zz;`);
stmt(`var a = new ShadowRealm(); a.evaluate("Object.freeze(Object.prototype)"); globalThis.R = T(() => Object.isFrozen(Object.prototype));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array") === Array);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array") === a.evaluate("Array"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})") === a.evaluate("(function(){})"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var fn = function(){}"); globalThis.R = T(() => a.evaluate("fn") === a.evaluate("fn"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array.prototype") === Array.prototype);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object")([]) instanceof Object);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.getPrototypeOf")(1));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.keys")([]));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.keys")("ab"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Object.is")(NaN, NaN));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array.isArray")([]));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array.of")(1,2));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array")(3));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => typeof a.evaluate("Array")(3));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array")(3).length);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("String")(12));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Number")("12"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Boolean")(0));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("BigInt")(10));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Symbol")("d"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => typeof a.evaluate("Symbol")("d"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Symbol.for")("d") === Symbol.for("d"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Symbol.for('d')") === Symbol.for("d"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Symbol.iterator") === Symbol.iterator);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Symbol.asyncIterator") === Symbol.asyncIterator);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("parseInt")("12px"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("eval")("1+1"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("eval")("var ev = 1; ev"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("eval")("var ev = 1"); globalThis.R = T(() => a.evaluate("ev"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("new Function('return 7')()"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Function")("return 8")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Function")("return this")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Function")("return typeof this")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Function")("return globalThis")());`);

// ---- 8. instanceof e identidade entre realms.
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("[] instanceof Array"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("Array.isArray")([]) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => x instanceof Array")([]) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => Array.isArray(x)")([]) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => x instanceof Object")(function(){}) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => x instanceof Function")(function(){}) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => Object.getPrototypeOf(x) === Function.prototype")(function(){}) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(x) => Object.getPrototypeOf(x) === Function.prototype")(a.evaluate("(function(){})")) );`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})") instanceof Function);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})") instanceof a.evaluate("Function"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getPrototypeOf(a.evaluate("(function(){})")) === Function.prototype);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getPrototypeOf(a.evaluate("(async function(){})")) === Function.prototype);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getPrototypeOf(a.evaluate("(function*(){})")) === Function.prototype);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})").hasOwnProperty("prototype"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(()=>1)").hasOwnProperty("prototype"));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})").prototype);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})").toString());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(()=>1)").toString());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Function.prototype.toString.call(a.evaluate("(function foo(){ return 1 })")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => String(a.evaluate("Math.max")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.prototype.toString.call(a.evaluate("(function(){})")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.prototype.toString.call(a.evaluate("(async function(){})")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.prototype.toString.call(a.evaluate("class A{}")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})").call.name);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.ownKeys(a.evaluate("(function(){})")).join());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.isExtensible(a.evaluate("(function(){})")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.isFrozen(a.evaluate("(function(){})")));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => { var f = a.evaluate("(function(){})"); f.x = 1; return f.x; });`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => { var f = a.evaluate("(function(){})"); f.x = 1; return a.evaluate("(function(){})").x; });`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => { var f = a.evaluate("(function(){})"); f.name = 'z'; return f.name; });`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => { "use strict"; var f = a.evaluate("(function(){})"); f.name = 'z'; return f.name; });`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getOwnPropertyDescriptor(a.evaluate("(function foo(a){})"), "name").configurable);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getOwnPropertyDescriptor(a.evaluate("(function foo(a){})"), "name").writable);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getOwnPropertyDescriptor(a.evaluate("(function foo(a){})"), "length").configurable);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getOwnPropertyDescriptor(a.evaluate("(function foo(a){})"), "length").writable);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Object.getOwnPropertyDescriptor(a.evaluate("(function foo(a){})"), "length").enumerable);`);

// ---- 9. new, this e protótipo de wrappers.
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("(function(){ this.a = 1 })"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => typeof new (a.evaluate("(function(){ this.a = 1 })"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("(function(){ return 1 })"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("(()=>1)"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("class A{}"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("class A{}")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("Math.max"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("Array"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("Object"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("Date"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("Number"))(1));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => new (a.evaluate("(function(){ return {} })"))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.construct(a.evaluate("(function(){})"), []));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.construct(a.evaluate("(function(){})"), [], Object));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => (function(){}).call.call(a.evaluate("(function(x){return x+1})"), null, 1));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(x){return x+1})").apply(null, [1]));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(x){return x+1})").bind(null, 1)());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(x){return x+1})").bind(null).name);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(x){return x+1})").bind(null, 1).length);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.apply(a.evaluate("(function(){return 4})"), 1, []));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.apply(a.evaluate("(function(){return 4})"), 1, {}));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => Reflect.apply(a.evaluate("(function(){return arguments.length})"), 1, [1,{},3]));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){ return new.target === undefined })")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return new f })")(function(){ this.q = 1 }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return typeof new f })")(function(){ this.q = 1 }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return typeof f.prototype })")(function(){ }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return Object.getPrototypeOf(f) === Function.prototype })")(function(){ }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f.call(1) })")(function(){ 'use strict'; return typeof this }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f.bind(null)() })")(function(){ return 9 }));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f() })")(a.evaluate("(function(){ return 10 })")));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); globalThis.R = T(() => b.evaluate("(function(f){ return f() })")(a.evaluate("(function(){ return 11 })")));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); globalThis.R = T(() => b.evaluate("(function(f){ return f() })")(a.evaluate("(function(){ return {} })")));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); globalThis.R = T(() => b.evaluate("(function(f){ return f.name })")(a.evaluate("(function nm(){})")));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); globalThis.R = T(() => b.evaluate("(function(f){ return typeof f })")(a.evaluate("(function nm(){})")));`);
stmt(`var a = new ShadowRealm(), b = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){})") === b.evaluate("(function(){})"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var counter = 0; function inc(){ return ++counter }"); var inc = a.evaluate("inc"); inc(); inc(); globalThis.R = T(() => inc());`);
stmt(`var a = new ShadowRealm(); a.evaluate("var counter = 0; function inc(){ return ++counter }"); var inc = a.evaluate("inc"); inc(); globalThis.R = T(() => a.evaluate("counter"));`);
stmt(`var a = new ShadowRealm(); a.evaluate("var o = {}; function set(v){ o.v = v } function get(){ return o.v }"); a.evaluate("set")(5); globalThis.R = T(() => a.evaluate("get")());`);
stmt(`var a = new ShadowRealm(); a.evaluate("var o = {}; function set(v){ o.v = v } function get(){ return o.v }"); a.evaluate("set")({}); globalThis.R = T(() => a.evaluate("get")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(){ return [1,2,3] })")());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f([1,2]) })")(x => x));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f(1,2) })")((x,y) => x+y));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f.length })")((x,y) => x+y));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f.name })")((x,y) => x+y));`);
stmt(`var a = new ShadowRealm(); var fn = function named(){ return 1 }; globalThis.R = T(() => a.evaluate("(function(f){ return f.name })")(fn));`);
stmt(`var a = new ShadowRealm(); var fn = () => 1; globalThis.R = T(() => a.evaluate("(function(f){ return f === f })")(fn));`);
stmt(`var a = new ShadowRealm(); var fn = () => 1; globalThis.R = T(() => a.evaluate("(function(f, g){ return f === g })")(fn, fn));`);
stmt(`var a = new ShadowRealm(); var fn = () => 1; var w = a.evaluate("(function(f){ return f })")(fn); globalThis.R = T(() => w === fn);`);
stmt(`var a = new ShadowRealm(); var fn = () => 1; var w = a.evaluate("(function(f){ return f })")(fn); globalThis.R = T(() => typeof w);`);
stmt(`var a = new ShadowRealm(); var fn = () => 1; var w = a.evaluate("(function(f){ return f })")(fn); globalThis.R = T(() => w());`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Math.max); globalThis.R = T(() => w(1, 3, 2));`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Math.max); globalThis.R = T(() => w.name + w.length);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(class K{ constructor(a,b){} }); globalThis.R = T(() => w.name + w.length);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(class K{ constructor(a,b){} }); globalThis.R = T(() => new w());`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(class K{ constructor(a,b){} }); globalThis.R = T(() => w());`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(function(){}.bind()); globalThis.R = T(() => w.name);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'name', {value: 1})); globalThis.R = T(() => w.name);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'length', {value: -5})); globalThis.R = T(() => w.length);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'length', {value: 3.9})); globalThis.R = T(() => w.length);`);
stmt(`var a = new ShadowRealm(); var w = a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'length', {value: Infinity})); globalThis.R = T(() => w.length);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'name', {get(){ throw new RangeError('x') }})));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(Object.defineProperty(function(){}, 'length', {get(){ throw new RangeError('x') }})));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy(function(){}, { get(){ throw new RangeError('x') } })));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy(function(){}, { getOwnPropertyDescriptor(){ throw new RangeError('x') } })).name);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy(function(){}, {})).name);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy(function(){}, {})).length);`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy(function(){ return 3 }, {}))());`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(new Proxy({}, {})));`);
stmt(`var a = new ShadowRealm(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(Proxy.revocable(function(){}, {}).proxy).name);`);
stmt(`var a = new ShadowRealm(); var r = Proxy.revocable(function(){}, {}); r.revoke(); globalThis.R = T(() => a.evaluate("(function(f){ return f })")(r.proxy));`);
stmt(`var a = new ShadowRealm(); var r = Proxy.revocable(function(){ return 1 }, {}); var w = a.evaluate("(function(f){ return f })")(r.proxy); r.revoke(); globalThis.R = T(() => w());`);

// ---- 10. Estrutura do ShadowRealm.
expr(`typeof ShadowRealm`);
expr(`ShadowRealm.name`);
expr(`ShadowRealm.length`);
expr(`ShadowRealm()`);
expr(`ShadowRealm.call({})`);
expr(`new ShadowRealm(1, 2) instanceof ShadowRealm`);
expr(`Object.prototype.toString.call(new ShadowRealm)`);
expr(`ShadowRealm.prototype[Symbol.toStringTag]`);
expr(`String(new ShadowRealm)`);
expr(`Object.getOwnPropertyDescriptor(ShadowRealm.prototype, Symbol.toStringTag).writable`);
expr(`Object.getOwnPropertyDescriptor(ShadowRealm.prototype, Symbol.toStringTag).enumerable`);
expr(`Object.getOwnPropertyDescriptor(ShadowRealm.prototype, Symbol.toStringTag).configurable`);
expr(`Object.getOwnPropertyNames(ShadowRealm.prototype).join()`);
expr(`Object.getOwnPropertyNames(ShadowRealm).join()`);
expr(`Reflect.ownKeys(ShadowRealm.prototype).map(String).join()`);
expr(`Object.getPrototypeOf(ShadowRealm) === Function.prototype`);
expr(`Object.getPrototypeOf(ShadowRealm.prototype) === Object.prototype`);
expr(`ShadowRealm.prototype.constructor === ShadowRealm`);
expr(`ShadowRealm.prototype.evaluate.name`);
expr(`ShadowRealm.prototype.evaluate.length`);
expr(`ShadowRealm.prototype.importValue.name`);
expr(`ShadowRealm.prototype.importValue.length`);
expr(`typeof ShadowRealm.prototype.evaluate`);
expr(`ShadowRealm.prototype.evaluate.call({}, '1')`);
expr(`ShadowRealm.prototype.evaluate.call(undefined, '1')`);
expr(`ShadowRealm.prototype.evaluate.call(ShadowRealm.prototype, '1')`);
expr(`ShadowRealm.prototype.evaluate.call(new Proxy(new ShadowRealm, {}), '1')`);
expr(`ShadowRealm.prototype.evaluate.call(new ShadowRealm, '1')`);
expr(`ShadowRealm.prototype.importValue.call({}, 'x', 'y')`);
expr(`ShadowRealm.prototype.importValue.call(undefined, 'x', 'y')`);
expr(`new ShadowRealm().importValue.length`);
expr(`new ShadowRealm().hasOwnProperty('evaluate')`);
expr(`Object.getOwnPropertyNames(new ShadowRealm).length`);
expr(`Object.isExtensible(new ShadowRealm)`);
expr(`JSON.stringify(new ShadowRealm)`);
expr(`Object.keys(new ShadowRealm).length`);
expr(`new ShadowRealm === new ShadowRealm`);
expr(`new ShadowRealm().constructor === ShadowRealm`);
expr(`Reflect.construct(ShadowRealm, [], Object) instanceof ShadowRealm`);
expr(`Reflect.construct(ShadowRealm, [], Object) instanceof Object`);
expr(`Object.getPrototypeOf(Reflect.construct(ShadowRealm, [], Array)) === Array.prototype`);
expr(`Reflect.construct(ShadowRealm, [], function(){}) instanceof ShadowRealm`);
expr(`Reflect.construct(ShadowRealm, [], Object.assign(function(){}, {prototype: 1})) instanceof ShadowRealm`);
expr(`Reflect.construct(ShadowRealm, [], Object.assign(function(){}, {prototype: null})) instanceof ShadowRealm`);
expr(`ShadowRealm.prototype.evaluate.call(Reflect.construct(ShadowRealm, [], Object), '1')`);
expr(`ShadowRealm.prototype.evaluate.call(Object.create(ShadowRealm.prototype), '1')`);
expr(`Object.create(ShadowRealm.prototype).evaluate('1')`);
expr(`Object.setPrototypeOf(new ShadowRealm, null).constructor`);
expr(`ShadowRealm.prototype.evaluate.call(Object.setPrototypeOf(new ShadowRealm, null), '1+2')`);

// ---- 11. Subclasses.
stmt(`class Sub extends ShadowRealm {} var s = new Sub; globalThis.R = T(() => s instanceof ShadowRealm) + " " + T(() => s.evaluate("1+1")) + " " + T(() => s.constructor.name);`);
stmt(`class Sub extends ShadowRealm { constructor(){ super(); this.extra = 1 } } var s = new Sub; globalThis.R = T(() => s.extra) + " " + T(() => s.evaluate("typeof globalThis"));`);
stmt(`class Sub extends ShadowRealm { evaluate(c){ return 'over:' + super.evaluate(c) } } globalThis.R = T(() => new Sub().evaluate("2"));`);
stmt(`class Sub extends ShadowRealm {} globalThis.R = T(() => Sub());`);
stmt(`class Sub extends ShadowRealm {} globalThis.R = T(() => Object.prototype.toString.call(new Sub));`);
stmt(`class Sub extends ShadowRealm {} globalThis.R = T(() => Object.getPrototypeOf(Sub) === ShadowRealm);`);
stmt(`class Sub extends ShadowRealm { constructor(){ } } globalThis.R = T(() => new Sub);`);
stmt(`class Sub extends ShadowRealm { constructor(){ super(); super() } } globalThis.R = T(() => new Sub);`);
stmt(`class Sub extends ShadowRealm { get [Symbol.toStringTag](){ return 'Z' } } globalThis.R = T(() => String(new Sub));`);
stmt(`class Sub extends ShadowRealm {} var s = new Sub; s.evaluate("var sv = 1"); globalThis.R = T(() => s.evaluate("sv"));`);
stmt(`class Sub extends ShadowRealm {} var s = new Sub; globalThis.R = T(() => s.importValue.name);`);
stmt(`function F(){} F.prototype = ShadowRealm.prototype; globalThis.R = T(() => new F instanceof ShadowRealm) + T(() => new F().evaluate('1'));`);
stmt(`var s = new ShadowRealm; s.evaluate = 5; globalThis.R = T(() => typeof s.evaluate);`);
stmt(`var s = new ShadowRealm; var ev = s.evaluate; globalThis.R = T(() => ev('1'));`);
stmt(`var s = new ShadowRealm; var ev = s.evaluate.bind(s); globalThis.R = T(() => ev('1+2'));`);
stmt(`var s = new ShadowRealm; var t = Object.create(s); globalThis.R = T(() => t.evaluate('1'));`);
stmt(`var s = new ShadowRealm; var p = new Proxy(s, {}); globalThis.R = T(() => p.evaluate('1'));`);
stmt(`var s = new ShadowRealm; var p = new Proxy(s, { get(t, k){ var v = Reflect.get(t, k); return typeof v === 'function' ? v.bind(t) : v } }); globalThis.R = T(() => p.evaluate('1'));`);

// ---- 12. evaluate com não-string e argumentos.
const nonStrings = ["undefined", "null", "1", "1n", "true", "Symbol('s')", "{}", "[]", "[1]", "()=>1", "new String('1')", "{toString(){return '1'}}", "{valueOf(){return '1'}}",
  "{toString(){ throw new RangeError('ts') }}", "new Proxy({}, {})", "Object(Symbol())", "new Number(1)", "[ '1' ]", "new Date(0)", "/1/"];
for (const n of nonStrings) {
  expr(`sr.evaluate(${n})`);
  expr(`sr.importValue(${n}, 'a')`);
  expr(`sr.importValue('./x.js', ${n})`);
}
expr(`sr.evaluate()`);
expr(`sr.evaluate('1', 2)`);
expr(`sr.evaluate('1', 2, 3)`);
expr(`sr.evaluate('')`);
expr(`sr.evaluate(' ')`);
expr(`sr.evaluate('// c')`);
expr(`sr.evaluate('/* c */ 3')`);
expr(`sr.evaluate('1;2;3')`);
expr(`sr.evaluate('1;;')`);
expr(`sr.evaluate('"use strict"; typeof this')`);
expr(`sr.evaluate('this === globalThis')`);
expr(`sr.evaluate('var a = 1')`);
expr(`sr.evaluate('let a = 1')`);
expr(`sr.evaluate('function f(){}; 5')`);
expr(`sr.evaluate('if (true) { 7 }')`);
expr(`sr.evaluate('for (var i = 0; i < 3; i++) i')`);
expr(`sr.evaluate('try { throw 1 } catch (e) { e + 1 }')`);
expr(`sr.evaluate('label: { 4; break label }')`);
expr(`sr.evaluate('switch (1) { case 1: "one" }')`);
expr(`sr.evaluate('(async () => 1)()')`);
expr(`sr.evaluate('Promise.resolve(1)')`);
expr(`sr.evaluate('1 + 1n')`);
expr(`sr.evaluate('null.x')`);
expr(`sr.evaluate('undefinedVariable')`);
expr(`sr.evaluate('x.y.z')`);
expr(`sr.evaluate('(void 0)()')`);
expr(`sr.evaluate('new (void 0)')`);
expr(`sr.evaluate('1()')`);
expr(`sr.evaluate('({}).x.y')`);
expr(`sr.evaluate('Symbol() + ""')`);
expr(`sr.evaluate('BigInt(1.5)')`);
expr(`sr.evaluate('new Array(-1)')`);
expr(`sr.evaluate('"a".repeat(-1)')`);
expr(`sr.evaluate('(function f(){ f() })()')`);
expr(`sr.evaluate('decodeURIComponent("%")')`);
expr(`sr.evaluate('JSON.parse("{")')`);
expr(`sr.evaluate('new Intl.NumberFormat("zz-")')`);
expr(`sr.evaluate('Object.defineProperty(1, "a", {})')`);
expr(`sr.evaluate('class A { constructor(){ this.x } } ; class B extends A { constructor(){ this.x } }; new B')`);
expr(`sr.evaluate('const c = 1; c = 2')`);
expr(`sr.evaluate('x; let x')`);
expr(`sr.evaluate('"use strict"; undeclared = 1')`);
expr(`sr.evaluate('Object.freeze([]).push(1)')`);
expr(`sr.evaluate('throw new Error("a")')`);
expr(`sr.evaluate('throw new AggregateError([1], "ag")')`);
expr(`sr.evaluate('throw new DOMException("d")')`);

// ---- 13. evaluate recursivo e aninhado.
expr(`sr.evaluate('new ShadowRealm().evaluate("1+1")')`);
expr(`sr.evaluate('new ShadowRealm().evaluate("(function(){})")')`);
expr(`typeof sr.evaluate('new ShadowRealm().evaluate("(function(){})")')`);
expr(`sr.evaluate('new ShadowRealm().evaluate("({})")')`);
expr(`sr.evaluate('new ShadowRealm().evaluate("(")')`);
expr(`sr.evaluate('typeof new ShadowRealm().evaluate')`);
expr(`sr.evaluate('new ShadowRealm().evaluate("new ShadowRealm().evaluate(\\"2*3\\")")')`);
expr(`sr.evaluate('globalThis === new ShadowRealm().evaluate("globalThis")')`);
expr(`sr.evaluate('try { new ShadowRealm().evaluate("throw 1") } catch (e) { e instanceof TypeError }')`);
expr(`sr.evaluate('try { new ShadowRealm().evaluate("(") } catch (e) { e instanceof SyntaxError }')`);
expr(`sr.evaluate('try { new ShadowRealm().evaluate("({})") } catch (e) { e.message }')`);
expr(`sr.evaluate('typeof ShadowRealm.prototype.evaluate')`);
expr(`sr.evaluate('ShadowRealm.prototype[Symbol.toStringTag]')`);
expr(`sr.evaluate('Object.prototype.toString.call(new ShadowRealm)')`);
expr(`sr.evaluate('ShadowRealm === ShadowRealm')`);
expr(`sr.evaluate('1')`) ;
expr(`sr.evaluate('var s = new ShadowRealm(); s.evaluate("var z = 3"); typeof z')`);
expr(`sr.evaluate('var s = new ShadowRealm(); s.evaluate("var z = 3"); s.evaluate("z")')`);
expr(`sr.evaluate('new ShadowRealm().evaluate("(function(a){return a*2})")(21)')`);
stmt(`var depth = 0, r = sr; for (; depth < 5; depth++) { r = r.evaluate("new ShadowRealm()"); } globalThis.R = T(() => depth);`);
stmt(`var r = sr; globalThis.R = T(() => sr.evaluate("new ShadowRealm()"));`);
stmt(`var f = sr.evaluate("(function(g){ return g(g) })"); globalThis.R = T(() => f(f));`);
stmt(`var f = sr.evaluate("(function(g){ return g(g) })"); globalThis.R = T(() => f(function(h){ return typeof h }));`);
stmt(`var f = sr.evaluate("(function(g, n){ return n > 0 ? g(g, n - 1) : 'done' })"); globalThis.R = T(() => f(f, 5));`);
stmt(`var f = sr.evaluate("(function(g, n){ return n > 0 ? g(n - 1) : 'done' })"); var h = function(n){ return f(h, n) }; globalThis.R = T(() => h(5));`);
stmt(`var f = sr.evaluate("(function(g, n){ return n > 0 ? g(n - 1) : 'done' })"); var h = function(n){ return f(h, n) }; globalThis.R = T(() => h(200));`);
stmt(`var f = sr.evaluate("(function(g){ return g() })"); globalThis.R = T(() => f(function(){ return f(function(){ return 'deep' }) }));`);
stmt(`var f = sr.evaluate("(function(g){ return g() })"); globalThis.R = T(() => f(function(){ return f(function(){ throw new Error('e') }) }));`);
stmt(`var f = sr.evaluate("(function(g){ try { return g() } catch (e) { return e.message } })"); globalThis.R = T(() => f(function(){ return f(function(){ throw new Error('e2') }) }));`);
stmt(`var f = sr.evaluate("(function(g){ try { return g() } catch (e) { throw e } })"); globalThis.R = T(() => f(function(){ throw new Error('e3') }));`);
stmt(`var f = sr.evaluate("(function(g){ try { return g() } catch (e) { throw e } })"); try { f(function(){ throw new RangeError('e4') }) } catch (e) { globalThis.R = S(e.constructor === TypeError) + " " + S(e.message) + " " + S(e.stack === undefined) }`);
stmt(`var f = sr.evaluate("(function(g){ return g() })"); try { f(function(){ throw new RangeError('e5') }) } catch (e) { globalThis.R = S(e.constructor.name) + " " + S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw new RangeError('e6') })"); try { f() } catch (e) { globalThis.R = S(e.constructor.name) + " " + S(e.message) + " " + S(Object.getOwnPropertyNames(e).sort().join()) }`);
stmt(`var f = sr.evaluate("(function(){ var e = new RangeError('e7'); e.extra = 1; throw e })"); try { f() } catch (e) { globalThis.R = S(e.extra) + " " + S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw { toString(){ return 'custom' } } })"); try { f() } catch (e) { globalThis.R = S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw { get message(){ return 'getter' } } })"); try { f() } catch (e) { globalThis.R = S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw Symbol('boom') })"); try { f() } catch (e) { globalThis.R = S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw 42n })"); try { f() } catch (e) { globalThis.R = S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw undefined })"); try { f() } catch (e) { globalThis.R = S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw new Proxy({}, { get(){ throw 1 } }) })"); try { f() } catch (e) { globalThis.R = S(e.constructor.name) + " " + S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ throw new Error('a', { cause: 1 }) })"); try { f() } catch (e) { globalThis.R = S(e.cause) + " " + S(e.message) }`);
stmt(`var f = sr.evaluate("(function(){ return Promise.reject(1) })"); globalThis.R = T(() => f());`);

// ---- 14. importValue (módulos fora do escopo do golden; só a forma e os erros síncronos).
expr(`typeof sr.importValue('./nope.js', 'x')`);
expr(`sr.importValue('./nope.js', 'x') instanceof Promise`);
expr(`Object.getPrototypeOf(sr.importValue('./nope.js', 'x')) === Promise.prototype`);
expr(`sr.importValue()`);
expr(`sr.importValue('x')`);
expr(`sr.importValue(1, 'x')`);
expr(`sr.importValue('x', 1)`);
expr(`sr.importValue('x', Symbol())`);
expr(`sr.importValue('x', {})`);
expr(`sr.importValue(Symbol(), 'x')`);
expr(`sr.importValue({toString(){ throw new RangeError('spec') }}, 'x')`);
expr(`sr.importValue('x', {toString(){ throw new RangeError('exp') }})`);
stmt(`var p = sr.importValue('./nope.js', 'x'); p.catch(e => { globalThis.R = S(e.constructor.name) + " " + S(e instanceof TypeError); });`);
stmt(`var p = sr.importValue('./nope.js', 'x'); p.then(() => { globalThis.R = 'ok' }, e => { globalThis.R = S(typeof e) + " " + S(e instanceof TypeError); });`);
stmt(`var p = sr.importValue('data:text/javascript,export var v = 1', 'v'); p.then(v => { globalThis.R = S(v) }, e => { globalThis.R = 'rej ' + S(e.constructor.name) });`);
stmt(`var p = sr.importValue('data:text/javascript,export var v = {}', 'v'); p.then(v => { globalThis.R = S(v) }, e => { globalThis.R = 'rej ' + S(e.constructor.name) });`);
stmt(`var p = sr.importValue('data:text/javascript,export var v = 1', 'nope'); p.then(v => { globalThis.R = S(v) }, e => { globalThis.R = 'rej ' + S(e.constructor.name) });`);

// ---- Execução, igual a gen-scope-golden.js.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "shadow-more-golden-"));
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
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const source of programs) {
  if (HOST.test(source.slice(PRELUDE.length))) continue;
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
