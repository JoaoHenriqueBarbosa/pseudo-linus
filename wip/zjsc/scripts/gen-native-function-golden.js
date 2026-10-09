// Gera tests/golden/native_function_bun.tsv: o comportamento de CADA função nativa dos builtins como objeto-função,
// medido no bun 1.4.2 (JavaScriptCore). O golden builtin_descriptor_bun já cobre os descritores das propriedades
// (writable/enumerable/configurable, length, name); este cobre a função em si: chaves próprias e atributos de
// length/name/prototype/caller/arguments, protótipo, extensibilidade, IsConstructor, `new f()`, Reflect.construct com
// newTarget, Function.prototype.toString (`function X() { [native code] }`, getters e setters) e o resultado (valor ou
// exceção com mensagem) de chamadas com receptores e argumentos fixos (undefined, {}, "ab", 255, apply com args).
// Um programa por objeto (construtor, protótipo, namespace, intrínseco) e por visão, cada um em bun filho novo, sem
// APIs de host, devolvendo o texto em `globalThis.R`. Chamadas voláteis (Math.random, Date.now, Date) ficam de fora.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-native-function-golden.js > tests/golden/native_function_bun.tsv
const { spawnSync } = require("child_process");
const { emitRow, writeResultPreload, decodeResult } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(require("fs").readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'var gopd=Object.getOwnPropertyDescriptor,otos=Object.prototype.toString;\n' +
  'function LBL(k){return typeof k==="symbol"?"@@"+k.description:k}\n' +
  'function ENT(o){var out=[];var ks=Reflect.ownKeys(o).filter(function(k){return k!=="appendStackTrace"&&k!=="prepareStackTrace"});' +
  'var strs=ks.filter(function(k){return typeof k==="string"}).sort();var syms=ks.filter(function(k){return typeof k==="symbol"}).sort(function(a,b){return String(a)<String(b)?-1:1});' +
  'if(typeof o==="function")out.push(["<self>",o]);' +
  'strs.concat(syms).forEach(function(k){var d=gopd(o,k);var l=LBL(k);if("value" in d){if(typeof d.value==="function")out.push([l,d.value])}else{if(d.get)out.push(["get "+l,d.get]);if(d.set)out.push(["set "+l,d.set])}});return out}\n' +
  'function SUM(v){var t=typeof v;if(t==="string")return JSON.stringify(v).slice(0,80);if(t==="symbol")return String(v);' +
  'if(t==="function")return "fn("+v.length+","+JSON.stringify(v.name)+")";if(v===null)return "null";' +
  'if(t==="object"){if(Array.isArray(v))return "arr["+v.length+"]"+v.slice(0,6).map(function(x){return typeof x==="object"||typeof x==="function"?typeof x:String(x)}).join("|");return "obj"+otos.call(v)}' +
  'if(Object.is(v,-0))return "-0";return t==="bigint"?v+"n":String(v)}\n' +
  'function TRY(f){try{return SUM(f())}catch(e){return "throw "+(e&&e.name)+": "+String(e&&e.message).slice(0,140)}}\n' +
  'function FL(f,k){var d=gopd(f,k);return d?("value" in d?"D":"A")+(d.writable?"w":"-")+(d.enumerable?"e":"-")+(d.configurable?"c":"-"):"none"}\n' +
  'function ISC(f){try{Reflect.construct(function(){},[],f);return true}catch(e){return false}}\n' +
  'function VOL(f,l){return l==="now"||l==="random"||f===Date}\n' +
  'function KEYSV(f){var p=Object.getPrototypeOf(f);return "own="+Reflect.ownKeys(f).map(LBL).join(",")+" proto="+(p===Function.prototype?"FP":p===null?"null":"other:"+typeof p+otos.call(p))+' +
  '" ext="+Object.isExtensible(f)+" length="+FL(f,"length")+" name="+FL(f,"name")+" prototype="+FL(f,"prototype")+" caller="+FL(f,"caller")+" arguments="+FL(f,"arguments")+" isCtor="+ISC(f)}\n' +
  'function CTORV(f){return "isCtor="+ISC(f)+" "+TRY(function(){return new f()})}\n' +
  'function CTORARGS(f){return ISC(f)?TRY(function(){return Reflect.construct(f,["ab",1])}):"notctor"}\n' +
  'function NTV(f){if(!ISC(f))return "notctor";var a=function(){};a.prototype={m:1};var b=function(){};b.prototype=1;' +
  'return TRY(function(){var r=Reflect.construct(f,[],a);return "nt="+(Object.getPrototypeOf(r)===a.prototype)})+" "+TRY(function(){var r=Reflect.construct(f,[],b);return "fallback="+(Object.getPrototypeOf(r)===f.prototype)+" tag="+otos.call(r)})}\n' +
  'function TOSV(f){return TRY(function(){return Function.prototype.toString.call(f)})}\n' +
  'function RUN(o,view){var lines=[];ENT(o).forEach(function(e){var l=e[0],f=e[1];var r;' +
  'if(view==="keys")r=KEYSV(f);else if(view==="construct")r=CTORV(f);else if(view==="ctorargs")r=CTORARGS(f);else if(view==="newtarget")r=NTV(f);else if(view==="tostring")r=TOSV(f);' +
  'else if(VOL(f,l))r="volatile";else if(view==="callundef")r=TRY(function(){return f.call(undefined)});' +
  'else if(view==="callobj")r=TRY(function(){return f.call({},{},{})});else if(view==="callprim")r=TRY(function(){return f.call("ab",1,2)});' +
  'else if(view==="callnum")r=TRY(function(){return f.call(255,16)});else if(view==="callargs")r=TRY(function(){return f.apply(undefined,["a",1,{}])});' +
  'lines.push(l+" "+r)});return lines.join("\\n")}\n';

const specials = {
  "%ArrayIteratorPrototype%": "Object.getPrototypeOf([][Symbol.iterator]())",
  "%MapIteratorPrototype%": "Object.getPrototypeOf(new Map()[Symbol.iterator]())",
  "%SetIteratorPrototype%": "Object.getPrototypeOf(new Set()[Symbol.iterator]())",
  "%StringIteratorPrototype%": "Object.getPrototypeOf(''[Symbol.iterator]())",
  "%RegExpStringIteratorPrototype%": "Object.getPrototypeOf(/a/[Symbol.matchAll](''))",
  "%GeneratorFunction%": "Object.getPrototypeOf(function* () {}).constructor",
  "%GeneratorFunctionPrototype%": "Object.getPrototypeOf(function* () {})",
  "%GeneratorPrototype%": "Object.getPrototypeOf(function* () {}).prototype",
  "%AsyncGeneratorFunction%": "Object.getPrototypeOf(async function* () {}).constructor",
  "%AsyncGeneratorFunctionPrototype%": "Object.getPrototypeOf(async function* () {})",
  "%AsyncGeneratorPrototype%": "Object.getPrototypeOf(async function* () {}).prototype",
  "%AsyncFunction%": "Object.getPrototypeOf(async function () {}).constructor",
  "%AsyncFunctionPrototype%": "Object.getPrototypeOf(async function () {})",
  "%AsyncIteratorPrototype%": "Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype)",
  "%IteratorPrototype%": "Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))",
  "%TypedArray%": "Object.getPrototypeOf(Int8Array)",
  "%TypedArrayPrototype%": "Object.getPrototypeOf(Int8Array.prototype)",
  "%ThrowTypeError%": "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get",
  "%GlobalFunctions%": "({parseInt,parseFloat,isNaN,isFinite,decodeURI,decodeURIComponent,encodeURI,encodeURIComponent,escape,unescape,eval})",
  "%BoundFunction%": "({b:Math.max.bind(null), c:(function f(a,b){}).bind(null,1)})",
  "%ArrowAndMethod%": "({a:()=>1,m(){},get g(){return 1},set s(v){},async am(){},*gm(){},async *agm(){},c:class{},f:function(){}})",
};

const ctorsWithProto = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Promise", "Map", "Set", "WeakMap", "WeakSet",
  "WeakRef", "FinalizationRegistry", "RegExp", "Date", "Error", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Proxy",
  "Iterator", "ShadowRealm", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError",
  "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array",
  "Float64Array", "BigInt64Array", "BigUint64Array", "Float16Array", "SuppressedError", "DisposableStack", "AsyncDisposableStack",
  "Intl.Collator", "Intl.DateTimeFormat", "Intl.DisplayNames", "Intl.DurationFormat", "Intl.ListFormat", "Intl.Locale",
  "Intl.NumberFormat", "Intl.PluralRules", "Intl.RelativeTimeFormat", "Intl.Segmenter",
  "WebAssembly.Module", "WebAssembly.Instance", "WebAssembly.Memory", "WebAssembly.Table", "WebAssembly.Global",
  "WebAssembly.Tag", "WebAssembly.Exception", "WebAssembly.CompileError", "WebAssembly.LinkError", "WebAssembly.RuntimeError",
];
const paths = [];
for (const c of ctorsWithProto) paths.push(c, c + ".prototype");
paths.push("Math", "JSON", "Reflect", "Atomics", "Intl", "WebAssembly", "Iterator.prototype", "Intl.Segmenter.prototype");
paths.push(...Object.keys(specials));
const exprOf = p => specials[p] || "globalThis." + p;

const views = ["keys", "construct", "ctorargs", "newtarget", "tostring", "callundef", "callobj", "callprim", "callnum", "callargs"];
const programs = [];
for (const p of paths) {
  for (const v of views) {
    programs.push(`try { var o = ${exprOf(p)}; globalThis.R = o === undefined || o === null ? "absent" : RUN(o, ${JSON.stringify(v)}); } catch (e) { globalThis.R = "throw " + e.name; }`);
  }
}
// Visão de globais: typeof, atributos e IsConstructor de cada nome padrão (APIs de host ficam de fora).
const globalNames = [
  "NaN", "Infinity", "undefined", "globalThis", "eval", "parseInt", "parseFloat", "isNaN", "isFinite", "decodeURI", "decodeURIComponent",
  "encodeURI", "encodeURIComponent", "escape", "unescape", "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt",
  "Promise", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "RegExp", "Date", "Error", "EvalError", "RangeError",
  "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError", "SuppressedError", "ArrayBuffer", "SharedArrayBuffer",
  "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float16Array",
  "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Proxy", "Reflect", "Math", "JSON", "Atomics", "Intl", "WebAssembly",
  "Iterator", "ShadowRealm", "DisposableStack", "AsyncDisposableStack", "Temporal",
];
for (let i = 0; i < globalNames.length; i += 12) {
  const chunk = JSON.stringify(globalNames.slice(i, i + 12));
  programs.push(
    'globalThis.R = ' + chunk + '.map(function(n){var d=gopd(globalThis,n);if(!d)return n+" absent";var v=d.value;return n+" typeof="+typeof v+" "+FL(globalThis,n)+" tag="+otos.call(v)+" isCtor="+(typeof v==="function"&&ISC(v))+(typeof v==="function"?" len="+v.length+" name="+JSON.stringify(v.name)+" str="+Function.prototype.toString.call(v).replace(/\\s+/g," ").slice(0,60):"")}).join("\\n")',
  );
}
programs.push('globalThis.R = ["NaN","Infinity","undefined","globalThis"].map(function(n){return n+" "+FL(globalThis,n)+" "+SUM(gopd(globalThis,n).value)}).join("\\n")');
programs.push('globalThis.R = ["NaN","Infinity","undefined"].map(function(n){"use strict";try{globalThis[n]=1;return n+" assigned"}catch(e){return n+" "+e.name+": "+e.message}}).join("\\n")');
programs.push('globalThis.R = ["NaN","Infinity","undefined"].map(function(n){try{return n+" "+delete globalThis[n]}catch(e){return n+" "+e.name+": "+e.message}}).join("\\n")');
programs.push('globalThis.R = Object.prototype.toString.call(globalThis)+" "+String(Object.getPrototypeOf(globalThis)===Object.prototype)+" "+typeof globalThis.globalThis+" "+(globalThis.globalThis===globalThis)+" "+Object.isExtensible(globalThis)');

const seen = new Set();
const unique = programs.filter(p => !seen.has(p) && seen.add(p));
const PRELOAD = writeResultPreload();
let kept = 0;
let dropped = 0;
for (const body of unique) {
  const source = '"use strict";\n' + PRELUDE + body;
  let result;
  try {
    // Processo fresco por programa: a reificação preguiçosa das tabelas estáticas do JSC depende do que rodou antes.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 30000 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(body).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(body).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
