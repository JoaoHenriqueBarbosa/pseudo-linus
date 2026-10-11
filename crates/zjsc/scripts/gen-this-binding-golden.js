// Gera tests/golden/this_binding_bun.tsv: grade de `this` binding medida no bun 1.4.2.
// Cruza funções sloppy, strict, arrow (em vários contextos), método, classe, bound, generator, async, Proxy, eval e
// nativas com as formas de chamada (`f()`, `o.f()`, `(o.f)()`, `(0,o.f)()`, `o?.f()`, `o.f?.()`, `call`/`apply`/`bind`
// com thisArg undefined, null e primitivos, `Reflect.apply`, `new` em bound, tagged template com membro, `with`, eval
// direto e indireto). Também cobre callbacks de Array.prototype, Map/Set, TypedArray, String.replace, JSON.parse e
// JSON.stringify com thisArg, getters e setters com receiver de Reflect.get/Reflect.set (e primitivos via protótipo),
// e o `this` global em eval, Function e inicializadores de classe. O resultado registra `typeof this` e a identidade.
// Cada programa roda num bun filho novo (a ordem de reificação das tabelas estáticas do JSC depende do que rodou
// antes), sem APIs de host, e grava o resultado em `globalThis.R`. Programas já presentes nos goldens vizinhos
// (`knownPrograms`) são descartados.
// Uso: bun scripts/gen-this-binding-golden.js > tests/golden/this_binding_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, stepSampler } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const STRICT = '"use strict";\n';
const PRELUDE =
  'var L="none";var O={tag:"O"};\n' +
  'function D(t){var y=typeof t;if(t===undefined)return "undefined";if(t===null)return "null";if(t===globalThis)return "global";if(t===O)return "O";' +
  'if(y==="object"||y==="function"){var c=Object.prototype.toString.call(t).slice(8,-1);var r=y+":"+c+(Object.getPrototypeOf(t)===null?":nullproto":"");' +
  'if(c==="Number"||c==="String"||c==="Boolean"||c==="BigInt"||c==="Symbol"){try{r+=":"+String(t.valueOf())}catch(e){}}return r}return y+":"+String(t)}\n' +
  'function W(t){L=typeof t+"|"+D(t);return L}\n' +
  'function T(f){try{return String(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

// ---- Tipos de função: [nome, definição, pós-processamento do valor `v` devolvido pela chamada].
const W_THIS = "W(this)";
const kinds = [
  ["sloppy", `function(){return ${W_THIS}}`, ""],
  ["strict", `function(){"use strict";return ${W_THIS}}`, ""],
  ["arrow", `()=>${W_THIS}`, ""],
  ["arrowInMethod", `({m(){return ()=>${W_THIS}}}).m.call(O)`, ""],
  ["arrowInStrictFn", `(function(){"use strict";return ()=>${W_THIS}})()`, ""],
  ["arrowInSloppyFn", `(function(){return ()=>${W_THIS}})()`, ""],
  ["arrowInStrictFnPrimitive", `(function(){"use strict";return ()=>${W_THIS}}).call(5)`, ""],
  ["arrowInSloppyFnPrimitive", `(function(){return ()=>${W_THIS}}).call(5)`, ""],
  ["objMethod", `({m(){return ${W_THIS}}}).m`, ""],
  ["strictObjMethod", `({m(){"use strict";return ${W_THIS}}}).m`, ""],
  ["classMethod", `(class{m(){return ${W_THIS}}}).prototype.m`, ""],
  ["classStatic", `(class{static m(){return ${W_THIS}}}).m`, ""],
  ["classCtor", `class{constructor(){${W_THIS}}}`, ""],
  ["derivedCtor", `class extends Object{constructor(){super();${W_THIS}}}`, ""],
  ["boundSloppyO", `(function(){return ${W_THIS}}).bind(O)`, ""],
  ["boundSloppy5", `(function(){return ${W_THIS}}).bind(5)`, ""],
  ["boundSloppyNull", `(function(){return ${W_THIS}}).bind(null)`, ""],
  ["boundSloppyUndefined", `(function(){return ${W_THIS}}).bind(undefined)`, ""],
  ["boundStrict5", `(function(){"use strict";return ${W_THIS}}).bind(5)`, ""],
  ["boundStrictUndefined", `(function(){"use strict";return ${W_THIS}}).bind()`, ""],
  ["boundStrictNull", `(function(){"use strict";return ${W_THIS}}).bind(null)`, ""],
  ["boundBound", `(function(){return ${W_THIS}}).bind(O).bind(5)`, ""],
  ["boundArrow", `(()=>${W_THIS}).bind(5)`, ""],
  ["generator", `function*(){${W_THIS};yield 1}`, "v.next();"],
  ["strictGenerator", `function*(){"use strict";${W_THIS};yield 1}`, "v.next();"],
  ["genMethod", `({*m(){${W_THIS};yield 1}}).m`, "v.next();"],
  ["asyncFn", `async function(){${W_THIS}}`, ""],
  ["strictAsyncFn", `async function(){"use strict";${W_THIS}}`, ""],
  ["asyncArrow", `async()=>{${W_THIS}}`, ""],
  ["asyncMethod", `({async m(){${W_THIS}}}).m`, ""],
  ["asyncGen", `async function*(){${W_THIS}}`, "v.next();"],
  ["proxyPlain", `new Proxy(function(){return ${W_THIS}},{})`, ""],
  ["proxyApplyReflect", `new Proxy(function(){return ${W_THIS}},{apply(t,th,a){return Reflect.apply(t,th,a)}})`, ""],
  ["proxyApplyFive", `new Proxy(function(){return ${W_THIS}},{apply(t,th,a){return t.apply(5,a)}})`, ""],
  ["proxyApplyThis", `new Proxy(function(){return ${W_THIS}},{apply(t,th,a){return W(th)}})`, ""],
  ["evalDirect", `function(){return eval("${W_THIS}")}`, ""],
  ["evalIndirect", `function(){return (0,eval)("${W_THIS}")}`, ""],
  ["strictEvalDirect", `function(){"use strict";return eval("${W_THIS}")}`, ""],
  ["newFunction", `function(){return new Function("return ${W_THIS}")()}`, ""],
  ["nestedPlain", `function(){return (function(){return ${W_THIS}})()}`, ""],
  ["nestedCallback", `function(){return [1].map(function(){return ${W_THIS}})[0]}`, ""],
  ["defaultParam", `function(a=${W_THIS}){}`, ""],
  ["objToString", "Object.prototype.toString", "L=String(v);"],
  ["objValueOf", "Object.prototype.valueOf", "L=D(v);"],
  ["numberValueOf", "Number.prototype.valueOf", "L=D(v);"],
  ["stringToString", "String.prototype.toString", "L=D(v);"],
  ["arrayConcat", "Array.prototype.concat", "L=D(v[0]);"],
];

// ---- Formas de chamada. `K` é a função, `o.f` e `f` são a mesma função.
const thisArgs = ["undefined", "null", "5", "'s'", "true", "1n", "Symbol.iterator", "O", "globalThis", "new Number(5)", "[1]", "0", "''", "NaN"];
const forms = [
  "f()", "o.f()", "(o.f)()", "(0,o.f)()", "o?.f()", "o.f?.()", "o?.f?.()", "o['f']()", "(o.f=o.f)()", "(o.f||0)()", "(o.f,o.f)()",
  "(o.f&&o.f)()", "(o.f??0)()", "[K][0]()", "({f:K}).f()", "(o).f()", "((o)).f()", "((o.f))()", "(+0,K)()", "K()", "(()=>K())()",
  "(function(){return K()})()", "(function(){'use strict';return K()})()", "(function(){return o.f()}).call(5)", "(()=>o.f())()",
  "new K()", "new o.f()", "new (o.f.bind(5))()", "new (K.bind(O))()", "new (K.bind())()", "Reflect.construct(K,[])", "Reflect.construct(K,[],Object)",
  "new K", "new (K)", "new Proxy(K,{}).constructor",
  "o.f`x`", "(o.f)`x`", "(0,o.f)`x`", "f`x`", "K`x${1}y`", "o['f']`x`", "(o.f||0)`x`", "o.f`x``y`",
  "o.f.bind()()", "o.f.bind(null)()", "o.f.bind(5)()", "o.f.bind(undefined)()", "o.f.bind(O).call(5)", "o.f.bind(5).call(O)", "o.f.bind(O).bind(5)()",
  "o.f.bind(5).apply(null)", "o.f.bind().call(O)", "o.f.bind('s').call(undefined)",
  "Reflect.apply(K,undefined,[])", "Reflect.apply(K,null,[])", "Reflect.apply(K,5,[])", "Reflect.apply(K,'s',[])", "Reflect.apply(K,O,[])",
  "Reflect.apply(K,true,[])", "Reflect.apply(K,globalThis,[])", "Reflect.apply(o.f,o,[])", "Reflect.apply(K,Symbol.iterator,[])", "Reflect.apply(K,1n,[])",
  "Function.prototype.call.call(K,5)", "Function.prototype.call.call(K)", "Function.prototype.call.call(K,null)", "Function.prototype.apply.call(K,null,[])",
  "Function.prototype.apply.call(K,'s',[])", "Function.prototype.call.bind(K)(5)", "Function.prototype.call.bind(K)()", "Function.prototype.call.bind(K,O)()",
  "Reflect.apply(Function.prototype.call,K,[5])", "Reflect.apply(Function.prototype.call,K,[])", "Function.prototype.bind.call(K,5)()",
  "Function.prototype.call.apply(K,[7])", "Function.prototype.apply.apply(K,[null,[]])",
  "(()=>{with(o){return f()}})()", "(()=>{with(o){return (f)()}})()", "(()=>{with(o){return (0,f)()}})()", "(()=>{with({f:K}){return f()}})()",
  "(()=>{with(o){return K()}})()", "(()=>{with(o){return o.f()}})()",
  "(()=>{with(Object.assign({},o,{[Symbol.unscopables]:{f:true}})){return f()}})()",
  "(()=>{with(O){return f()}})()", "(()=>{with(O){return f.call()}})()",
  "eval('f()')", "eval('o.f()')", "(0,eval)('f()')", "(0,eval)('o.f()')", "eval('(0,o.f)()')", "eval('(o.f)()')", "(0,eval)('K.call(5)')", "eval('K.call(5)')",
  "(function(){return eval('f()')}).call(5)", "(function(){'use strict';return eval('f()')}).call(5)",
];
for (const t of thisArgs) {
  for (const m of ["call", "apply"]) {
    forms.push(`o.f.${m}(${t})`);
    forms.push(`f.${m}(${t})`);
  }
  forms.push(`o.f.bind(${t})()`);
  forms.push(`new (K.bind(${t}))`);
}
forms.push("o.f.call()", "o.f.apply()", "o.f.apply(undefined)", "o.f.apply(null,[])", "o.f.apply(5,[1])", "o.f.apply('s',[])", "o.f.apply(O,[])", "o.f.call(undefined,1)",
  "o.f.apply(undefined,undefined)", "o.f.apply(5,null)", "o.f.apply(5,{length:1})", "o.f.bind().bind(5)()");
// Subconjunto que também roda sob "use strict" no topo do programa (sem `with`); entra com densidade 1/2 (tipos centrais: 1/4).
const strictForms = forms.filter(form => !form.includes("with(") && !form.includes("unscopables"));

// Os candidatos entram em `pool`; os que só entram em parte levam a densidade (`thinProgram(passo, ...)`, 1 em `passo`) e a
// escolha é por hash do texto do programa (`sampleByHash` dentro do `stepSampler`), nunca pela posição na lista de formas.
const pool = stepSampler();
const programText = (strict, text) => (strict ? STRICT : "") + PRELUDE + text;
const addProgram = (strict, text) => pool.push(1, programText(strict, text));
const thinProgram = (step, strict, text) => pool.push(step, programText(strict, text));
const q = s => JSON.stringify(s);

function callProgram(kind, form) {
  const [, def, post] = kind;
  return `var K=${def};var o={f:K};var f=K;globalThis.R=T(()=>{L="none";var v=${form};${post}return L})`;
}
// Os tipos centrais cruzam com todas as formas; os demais, com uma de cada quatro, para o golden não passar de ~10 mil casos.
const coreKinds = new Set(["sloppy", "strict", "arrow", "arrowInMethod", "arrowInStrictFn", "objMethod", "classMethod", "boundSloppy5", "boundStrict5", "generator", "asyncFn", "proxyApplyThis"]);
for (const kind of kinds) {
  const core = coreKinds.has(kind[0]);
  forms.forEach(form => thinProgram(core ? 1 : 4, false, callProgram(kind, form)));
  if (core) strictForms.forEach(form => thinProgram(4, true, callProgram(kind, form)));
}

// ---- Callbacks de bibliotecas com thisArg.
const cbNames = ["sloppy", "strict", "arrow", "arrowInMethod", "objMethod", "classMethod", "boundSloppy5", "boundStrict5", "asyncFn", "proxyApplyThis", "evalDirect", "nestedPlain"];
const cbKinds = kinds.filter(kind => cbNames.includes(kind[0]) && !["generator", "strictGenerator", "genMethod", "asyncGen", "classCtor", "derivedCtor", "defaultParam", "objValueOf", "numberValueOf", "stringToString", "arrayConcat", "objToString"].includes(kind[0]));
const cbThis = ["", "undefined", "null", "5", "'s'", "O"];
const withThis = (call, t) => (t === "" ? call("") : call(`,${t}`));
const callbackShapes = [
  t => `[1].map(K${t})`, t => `[1].forEach(K${t})`, t => `[1].filter(K${t})`, t => `[1].some(K${t})`, t => `[1].every(K${t})`,
  t => `[1].find(K${t})`, t => `[1].findIndex(K${t})`, t => `[1].findLast(K${t})`, t => `[1].findLastIndex(K${t})`, t => `[1].flatMap(K${t})`,
  t => `Array.from([1],K${t})`, t => `Array.from({length:1},K${t})`, t => `Array.from(new Set([1]),K${t})`, t => `Array.from('a',K${t})`,
  t => `new Set([1]).forEach(K${t})`, t => `new Map([[1,2]]).forEach(K${t})`,
  t => `new Uint8Array(1).map(K${t})`, t => `new Uint8Array(1).forEach(K${t})`, t => `new Uint8Array(1).filter(K${t})`, t => `new Uint8Array(1).some(K${t})`,
  t => `new Uint8Array(1).find(K${t})`, t => `Uint8Array.from([1],K${t})`, t => `Float64Array.from({length:1},K${t})`,
  t => `Array.prototype.map.call('a',K${t})`, t => `Array.prototype.forEach.call({length:1,0:1},K${t})`,
  t => `Reflect.apply(Array.prototype.map,[1],[K${t}])`, t => `Array.prototype.filter.apply([1],[K${t}])`,
  t => `[1,2].reduce(K${t === "" ? "" : t})`, t => `[3,1].toSorted(K)`,
];
const fixedShapes = [
  "[1,2].reduce(K)", "[1,2].reduce(K,0)", "[1,2].reduceRight(K,0)", "[2,1].sort(K)", "[2,1].toSorted(K)", "'a'.replace(/a/,K)", "'a'.replace('a',K)",
  "'a'.replaceAll('a',K)", "'a'.replace(/(a)/g,K)", "JSON.parse('[1]',K)", "JSON.parse('{\"a\":1}',K)", "JSON.parse('1',K)", "JSON.stringify([1],K)",
  "JSON.stringify({a:1},K)", "JSON.stringify(1,K)", "new Promise(K)", "Array.from({length:1,0:7},K,5)", "Object.groupBy([1],K)", "Map.groupBy([1],K)",
  "new Proxy({},{get:K}).x", "new Proxy({},{has:K}).x", "'x' in new Proxy({},{has:K})", "new Proxy(function(){},{apply:K})()",
  "new Proxy(function(){},{construct:K})", "Object.defineProperty({},'x',{get:K}).x", "Object.defineProperty({},'x',{set:K}).x=1",
  "new Intl.Collator().compare.call(K)", "[1].values().map(K)", "[1].values().forEach(K)", "[1].values().filter(K)", "[1].values().some(K)",
  "Iterator.from([1]).map(K)", "Symbol.iterator in {[Symbol.iterator]:K}", "[...{[Symbol.iterator]:K}]", "String.raw({raw:'a'},K)",
  "Array.prototype.toString.call({join:K})", "Array.prototype.toString.call({join:K,toString:K})", "({toString:K})+''", "({valueOf:K})*1", "({[Symbol.toPrimitive]:K})+''",
  "`${({toString:K})}`", "[{toJSON:K}].map(JSON.stringify)", "JSON.stringify({toJSON:K})", "JSON.stringify([{toJSON:K}])", "({[Symbol.hasInstance]:K}) instanceof Object",
  "Object.prototype.toString.call({[Symbol.toStringTag]:K})", "new Date({valueOf:K})", "Number({valueOf:K})", "String({toString:K})",
  "Math.max({valueOf:K})", "[1,2].join({toString:K})", "isNaN({valueOf:K})", "parseInt({toString:K})", "new Array({valueOf:K})", "Symbol.for({toString:K})",
  "Object.keys(new Proxy({},{ownKeys:K}))", "Reflect.ownKeys(new Proxy({},{ownKeys:K}))", "Object.getPrototypeOf(new Proxy({},{getPrototypeOf:K}))",
  "new Proxy({},{set:K}).x=1", "delete new Proxy({},{deleteProperty:K}).x", "Object.getOwnPropertyDescriptor(new Proxy({},{getOwnPropertyDescriptor:K}),'x')",
  "Object.defineProperty(new Proxy({},{defineProperty:K}),'x',{})", "Object.isExtensible(new Proxy({},{isExtensible:K}))",
];
for (const kind of cbKinds) {
  const def = kind[1];
  const body = shape => `var K=${def};globalThis.R=T(()=>{L="none";${shape};return L})`;
  for (const shape of callbackShapes) {
    for (const t of cbThis) addProgram(false, body(withThis(shape, t)));
  }
  for (const shape of fixedShapes) {
    addProgram(false, body(shape));
    if (cbNames.indexOf(kind[0]) < 4) addProgram(true, body(shape));
  }
  for (const t of ["", "5"]) for (const shape of callbackShapes.slice(0, 6)) addProgram(true, body(withThis(shape, t)));
}

// ---- Getters e setters com receiver.
const accessorKinds = [
  ["objLiteral", "({get x(){return W(this)},set x(v){W(this)}})"],
  ["strictLiteral", "({get x(){'use strict';return W(this)},set x(v){'use strict';W(this)}})"],
  ["classInstance", "(class{get x(){return W(this)}set x(v){W(this)}}).prototype"],
  ["classStatic", "(class{static get x(){return W(this)}static set x(v){W(this)}})"],
  ["defineFunction", "Object.defineProperty({},'x',{get:function(){return W(this)},set:function(v){W(this)},configurable:true})"],
  ["defineStrict", "Object.defineProperty({},'x',{get:function(){'use strict';return W(this)},set:function(v){'use strict';W(this)},configurable:true})"],
  ["defineArrow", "Object.defineProperty({},'x',{get:()=>W(this),set:v=>W(this),configurable:true})"],
  ["defineBound", "Object.defineProperty({},'x',{get:(function(){return W(this)}).bind(5),set:(function(v){W(this)}).bind(5),configurable:true})"],
  ["legacyDefine", "(function(){var a={};a.__defineGetter__('x',function(){return W(this)});a.__defineSetter__('x',function(v){W(this)});return a})()"],
  ["protoOfObject", "(function(){Object.defineProperty(Object.prototype,'x',{get:function(){return W(this)},set:function(v){W(this)},configurable:true});return {}})()"],
  ["protoOfObjectStrict", "(function(){Object.defineProperty(Object.prototype,'x',{get:function(){'use strict';return W(this)},set:function(v){'use strict';W(this)},configurable:true});return {}})()"],
  ["protoOfNumber", "(function(){Object.defineProperty(Number.prototype,'x',{get:function(){return W(this)},set:function(v){W(this)},configurable:true});return {}})()"],
  ["protoOfNumberStrict", "(function(){Object.defineProperty(Number.prototype,'x',{get:function(){'use strict';return W(this)},set:function(v){'use strict';W(this)},configurable:true});return {}})()"],
  ["protoOfString", "(function(){Object.defineProperty(String.prototype,'x',{get:function(){return W(this)},set:function(v){W(this)},configurable:true});return {}})()"],
];
const getPatterns = [
  "A.x", "Object.create(A).x", "Object.create(Object.create(A)).x", "Reflect.get(A,'x')", "Reflect.get(A,'x',5)", "Reflect.get(A,'x',undefined)", "Reflect.get(A,'x',null)",
  "Reflect.get(A,'x','s')", "Reflect.get(A,'x',true)", "Reflect.get(A,'x',O)", "Reflect.get(A,'x',1n)", "Reflect.get(A,'x',Symbol.iterator)", "Reflect.get(Object.create(A),'x',5)",
  "Reflect.get(A,'x',Object.create(A))", "A['x']", "A?.x", "(0,A).x", "({__proto__:A}).x", "new Proxy(A,{}).x", "new Proxy(A,{get(t,k,r){return Reflect.get(t,k,r)}}).x",
  "new Proxy(A,{get(t,k,r){return Reflect.get(t,k)}}).x", "new Proxy(A,{get(t,k,r){return Reflect.get(t,k,5)}}).x", "Object.create(new Proxy(A,{})).x",
  "Object.getOwnPropertyDescriptor(A,'x')?.get?.call(5)", "Object.getOwnPropertyDescriptor(A,'x')?.get?.call()", "Object.getOwnPropertyDescriptor(A,'x')?.get?.call(null)",
  "Reflect.apply(Object.getOwnPropertyDescriptor(A,'x')?.get||Function.prototype,5,[])", "(5).x", "'s'.x", "true.x", "(1n).x", "Symbol.iterator.x", "(5)['x']", "(5)?.x",
  "Reflect.get(Object(5),'x')", "Object(5).x", "Object('s').x", "(()=>{with(A){return x}})()", "(function(){return A.x}).call(5)", "(0,eval)('A.x')", "eval('A.x')",
  "Object.assign({},A).x", "({...A}).x", "[A.x][0]", "`${A.x}`", "A.x+''", "A.x||0", "typeof A.x", "delete A.x", "'x' in A", "A.x?.y",
  "(({x})=>x)(A)", "(({x})=>x)(5)", "(({x})=>x)('s')", "(({x}=A)=>x)()", "(function({x}){return x})(Object.create(A))", "Reflect.get(5,'x')",
];
const setPatterns = [
  "A.x=1", "Object.create(A).x=1", "Object.create(Object.create(A)).x=1", "Reflect.set(A,'x',1)", "Reflect.set(A,'x',1,5)", "Reflect.set(A,'x',1,undefined)", "Reflect.set(A,'x',1,null)",
  "Reflect.set(A,'x',1,'s')", "Reflect.set(A,'x',1,true)", "Reflect.set(A,'x',1,O)", "Reflect.set(A,'x',1,Object.create(A))", "Reflect.set(Object.create(A),'x',1)",
  "Reflect.set(Object.create(A),'x',1,5)", "Reflect.set(A,'x',1,{})", "A['x']=1", "(0,A).x=1", "({__proto__:A}).x=1", "new Proxy(A,{}).x=1",
  "new Proxy(A,{set(t,k,v,r){return Reflect.set(t,k,v,r)}}).x=1", "new Proxy(A,{set(t,k,v,r){return Reflect.set(t,k,v)}}).x=1", "new Proxy(A,{set(t,k,v,r){return Reflect.set(t,k,v,5)}}).x=1",
  "Object.getOwnPropertyDescriptor(A,'x')?.set?.call(5,1)", "Object.getOwnPropertyDescriptor(A,'x')?.set?.call(undefined,1)", "A.x+=1", "A.x++", "++A.x", "A.x??=1", "A.x||=1", "A.x&&=1",
  "(5).x=1", "'s'.x=1", "true.x=1", "(1n).x=1", "Object(5).x=1", "(()=>{with(A){x=1}})()", "(function(){A.x=1}).call(5)", "Object.assign(A,{x:1})", "Object.assign(Object.create(A),{x:1})",
  "[A.x]=[1]", "({a:A.x}={a:1})", "[...A.x]=[1]", "({...A.x}={a:1})", "(()=>{for(A.x of [1]);})()", "(()=>{for(A.x in {k:1});})()", "Object.defineProperties(A,{})",
  "Reflect.defineProperty(Object.create(A),'x',{value:1})", "Object.setPrototypeOf({},A).x=1", "Object.fromEntries([['x',1]])", "A.x=A.x",
  "(()=>{'use strict';A.x=1})()", "(()=>{'use strict';(5).x=1})()", "(()=>{'use strict';Object.create(A).x=1})()", "A.x=1,A.x=2",
];
for (const [name, def] of accessorKinds) {
  for (const p of getPatterns) {
    thinProgram(2, false, `var A=${def};globalThis.R=T(()=>{L="none";var v=${p};return L})`);
    if (!p.includes("with(")) thinProgram(2, true, `var A=${def};globalThis.R=T(()=>{L="none";var v=${p};return L})`);
  }
  for (const p of setPatterns) {
    thinProgram(2, false, `var A=${def};globalThis.R=T(()=>{L="none";${p};return L})`);
    if (!p.includes("with(")) thinProgram(2, true, `var A=${def};globalThis.R=T(()=>{L="none";${p};return L})`);
  }
}

// ---- Derivações de `this` dentro de função chamada com thisArg.
const derivations = [
  "this", "typeof this", "(()=>this)()", "(()=>(()=>this)())()", "eval('this')", "(0,eval)('this')", "eval('(()=>this)()')", "new Function('return this')()",
  "new Function('\"use strict\";return this')()", "Function('return this')()", "(function(){return this})()", "(function(){'use strict';return this})()", "[this][0]",
  "({t:this}).t", "`${typeof this}`", "this?.constructor?.name", "Object(this)===this", "this==null", "this===undefined", "this===null", "this instanceof Object",
  "Object.prototype.toString.call(this)", "(class{static s=this}).s===this", "(class{static s=(()=>this)()}).s===this", "new (class{x=this;})().x instanceof Object",
  "({[typeof this]:1})", "[1].map(()=>this)[0]", "[1].map(function(){return this})[0]", "[1].map(function(){return this},this)[0]", "[1].map(function(){'use strict';return this},this)[0]",
  "this+''", "typeof Object(this)", "(()=>typeof this)()", "((a=this)=>a)()", "(function(a=this){return a})()", "(function(a=this){return a}).call(7)", "this.valueOf()",
  "Reflect.apply(function(){return this},this,[])", "Reflect.apply(function(){'use strict';return this},this,[])", "(function(){return this}).call(this)", "(function(){return this}).apply(this)",
  "(function(){return this}).bind(this)()", "typeof (function(){return this}).call(this)", "Object.getPrototypeOf(Object(this))===Object.prototype",
  "(async()=>this)()===undefined", "(function*(){yield this})().next().value", "(function*(){'use strict';yield this})().next().value",
];
const deriveThisArgs = ["undefined", "null", "5", "'s'", "true", "1n", "Symbol.iterator", "O", "globalThis", "new Number(5)", "[1]", "0", "''", "NaN", "function(){}"];
for (const e of derivations) {
  for (const t of deriveThisArgs) {
    addProgram(false, `globalThis.R=T(()=>{var h=function(){return D(${e})};var s=function(){"use strict";return D(${e})};return h.call(${t})+" / "+s.call(${t})})`);
    addProgram(false, `globalThis.R=T(()=>W((function(){return ${e}}).call(${t})))`);
  }
  addProgram(false, `globalThis.R=T(()=>W(${e}))`);
  addProgram(false, `globalThis.R=T(()=>D(eval(${q(e)})))`);
  addProgram(false, `globalThis.R=T(()=>D((0,eval)(${q(e)})))`);
  addProgram(true, `globalThis.R=T(()=>D(eval(${q(e)})))`);
  addProgram(true, `globalThis.R=T(()=>D((0,eval)(${q(e)})))`);
  addProgram(false, `globalThis.R=T(()=>D((function(){return eval(${q(e)})}).call(5)))`);
  addProgram(false, `globalThis.R=T(()=>D(({m(){return eval(${q(e)})}}).m()))`);
  addProgram(false, `globalThis.R=T(()=>D(new (class{constructor(){this.r=eval(${q(e)})}})().r))`);
}

// ---- Filtros: API de host, duplicata interna e duplicata contra os goldens vizinhos.
const seen = new Set();
let unique = pool.resolve().filter(p => !usesHostApi(p) && !seen.has(p) && seen.add(p));
const known = knownPrograms("this_binding_bun.tsv", name => name !== "this_binding_bun.tsv");
const knownSet = new Set(known);
const knownLines = new Set(known.flatMap(p => (p.includes("this") ? p.split("\n") : [])));
let dup = 0;
unique = unique.filter(program => {
  const body = program.slice(program.indexOf("\nfunction T(") + 1).replace(/^function T\([^\n]*\n/, "");
  if (knownSet.has(program) || knownLines.has(body)) { dup++; return false; }
  return true;
});

// ---- Execução: no máximo 6 filhos ao mesmo tempo, cada um com timeout de 8 s.
const MAX_CHILDREN = 6;
function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let finished = false;
    const done = result => { if (!finished) { finished = true; clearTimeout(timer); resolve(result); } };
    const timer = setTimeout(() => { child.kill("SIGKILL"); done(null); }, 8000);
    child.stdout.on("data", chunk => { out += chunk; });
    child.stderr.on("data", () => {});
    child.on("error", () => done(null));
    child.on("close", code => done(code === 0 ? out : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(unique.length);
  let next = 0;
  await Promise.all(Array.from({ length: MAX_CHILDREN }, async () => {
    while (next < unique.length) {
      const index = next++;
      results[index] = await runChild(unique[index]);
    }
  }));
  const rows = [];
  let dropped = 0;
  unique.forEach((source, index) => {
    const result = results[index];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || result.includes("—") || result.includes("–")) { dropped++; return; }
    rows.push({ source, result });
  });
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
  process.stdout.write(emitFactored("this_binding", rows));
})();
