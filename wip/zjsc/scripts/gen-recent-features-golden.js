// Gera tests/golden/recent_features_bun.tsv: APIs recentes do ECMAScript medidas no bun 1.4.2 (typeof, name/length,
// descritores, mensagens de erro exatas, valores): Uint8Array base64/hex, Math.sumPrecise, Error.isError, Promise.try,
// Array.fromAsync, Object.groupBy, Iterator.concat/zip e helpers, cópia de Array, isWellFormed, Atomics.waitAsync,
// RegExp.escape, JSON.rawJSON, Float16, Intl.Locale, Symbol.dispose, DisposableStack, AsyncDisposableStack,
// SuppressedError, `using`/`await using`, Temporal básico, ArrayBuffer transfer/resize/detached.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-getter-setter-golden.js.
// Cada passo é um `T(() => ...)` que guarda o valor serializado ou `T:<Erro>: <mensagem>` quando lança; `A(async)`
// registra o resultado depois das microtarefas (o resultado sai no evento `exit`, com a fila já esvaziada).
// Uso: bun scripts/gen-recent-features-golden.js > tests/golden/recent_features_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRE =
  "const L=[];" +
  "const S=(v,d=0)=>{const t=typeof v;if(v===null)return'null';if(t==='undefined')return'undefined';" +
  "if(t==='number')return Object.is(v,-0)?'-0':String(v);if(t==='string')return JSON.stringify(v);" +
  "if(t==='boolean')return String(v);if(t==='bigint')return v+'n';if(t==='symbol')return String(v);" +
  "if(t==='function')return'fn:'+v.name+'/'+v.length;if(d>3)return'...';" +
  "if(v instanceof Error)return'E:'+v.constructor.name+':'+v.message;" +
  "if(ArrayBuffer.isView(v)&&!(v instanceof DataView))return v.constructor.name+'['+Array.from(v,x=>S(x)).join(',')+']';" +
  "if(v instanceof ArrayBuffer)return'AB('+v.byteLength+')';" +
  "if(Array.isArray(v)){if(v.length>64)return'array('+v.length+')';return'['+Array.from(v,x=>S(x,d+1)).join(',')+']'}" +
  "if(v instanceof Map)return'Map{'+Array.from(v,([a,b])=>S(a,d+1)+'=>'+S(b,d+1)).join(',')+'}';" +
  "if(v instanceof Set)return'Set{'+Array.from(v,x=>S(x,d+1)).join(',')+'}';" +
  "let o=[];for(const k of Reflect.ownKeys(v)){const x=Object.getOwnPropertyDescriptor(v,k);" +
  "o.push(String(k)+(x.enumerable?'':'~')+':'+('value' in x?S(x.value,d+1):'(accessor)'))}" +
  "return'{'+o.join(',')+'}'};" +
  "const E=f=>{try{return S(f())}catch(e){return'T:'+(e&&e.constructor&&e.constructor.name)+': '+(e&&e.message)}};" +
  "const T=f=>{L.push(E(f))};" +
  "const A=async f=>{const i=L.push('pending')-1;try{L[i]=S(await f())}catch(e){L[i]='T:'+(e&&e.constructor&&e.constructor.name)+': '+(e&&e.message)}};";

const programs = [];
// Cada passo é uma expressão, ou um bloco `{ ...; return x }`. Prefixo `async:` marca passo assíncrono.
const prog = (pre, ...steps) =>
  programs.push(
    PRE +
      pre +
      ";" +
      steps
        .map(st => {
          const fn = st.startsWith("async:") ? "A" : "T";
          const body = st.replace(/^async:/, "");
          return fn + "(" + (fn === "A" ? "async " : "") + "()=>" + (body.startsWith("{") ? body : "(" + body + ")") + ");";
        })
        .join("") +
      'globalThis.R=L.join(" | ");',
  );
const each = (list, make) => list.forEach(x => prog("", make(x)));

// ---- 1. typeof, name e length de cada API.
const apis = [
  "Uint8Array.fromBase64", "Uint8Array.fromHex", "Uint8Array.prototype.toBase64", "Uint8Array.prototype.toHex",
  "Uint8Array.prototype.setFromBase64", "Uint8Array.prototype.setFromHex", "Math.sumPrecise", "Math.f16round",
  "Error.isError", "Promise.try", "Promise.withResolvers", "Array.fromAsync", "Object.groupBy", "Map.groupBy",
  "Iterator", "Iterator.from", "Iterator.concat", "Iterator.zip", "Iterator.zipKeyed", "Iterator.prototype.map",
  "Iterator.prototype.filter", "Iterator.prototype.take", "Iterator.prototype.drop", "Iterator.prototype.flatMap",
  "Iterator.prototype.reduce", "Iterator.prototype.toArray", "Iterator.prototype.forEach", "Iterator.prototype.some",
  "Iterator.prototype.every", "Iterator.prototype.find", "Array.prototype.toSorted", "Array.prototype.toReversed",
  "Array.prototype.toSpliced", "Array.prototype.with", "Array.prototype.findLast", "Array.prototype.findLastIndex",
  "Array.prototype.at", "Array.prototype.flat", "Array.prototype.flatMap", "Array.prototype.includes",
  "Uint8Array.prototype.toSorted", "Uint8Array.prototype.toReversed", "Uint8Array.prototype.with",
  "Uint8Array.prototype.findLast", "String.prototype.isWellFormed", "String.prototype.toWellFormed",
  "String.prototype.at", "String.prototype.replaceAll", "String.prototype.matchAll", "Atomics.waitAsync",
  "Atomics.pause", "Atomics.wait", "Atomics.notify", "RegExp.escape", "JSON.rawJSON", "JSON.isRawJSON",
  "Float16Array", "DataView.prototype.getFloat16", "DataView.prototype.setFloat16", "Intl.Locale",
  "Intl.Locale.prototype.maximize", "Intl.Locale.prototype.minimize", "Intl.Locale.prototype.getWeekInfo",
  "Intl.Locale.prototype.getCalendars", "Intl.Locale.prototype.getCollations", "Intl.Locale.prototype.getHourCycles",
  "Intl.Locale.prototype.getNumberingSystems", "Intl.Locale.prototype.getTextInfo", "Intl.Locale.prototype.getTimeZones",
  "Intl.Locale.prototype.toString", "DisposableStack", "AsyncDisposableStack", "SuppressedError",
  "DisposableStack.prototype.use", "DisposableStack.prototype.adopt", "DisposableStack.prototype.defer",
  "DisposableStack.prototype.move", "DisposableStack.prototype.dispose", "AsyncDisposableStack.prototype.use",
  "AsyncDisposableStack.prototype.adopt", "AsyncDisposableStack.prototype.defer", "AsyncDisposableStack.prototype.move",
  "AsyncDisposableStack.prototype.disposeAsync", "Temporal", "Temporal.Instant", "Temporal.Now",
  "Temporal.Now.instant", "Temporal.Now.timeZoneId", "Temporal.Now.plainDateISO", "Temporal.Instant.from",
  "Temporal.Instant.fromEpochMilliseconds", "Temporal.Instant.fromEpochNanoseconds", "Temporal.Instant.prototype.add",
  "Temporal.Instant.prototype.since", "Temporal.Instant.prototype.toZonedDateTimeISO",
  "ArrayBuffer.prototype.transfer", "ArrayBuffer.prototype.transferToFixedLength", "ArrayBuffer.prototype.resize",
  "SharedArrayBuffer.prototype.grow", "Map.prototype.getOrInsert", "Map.prototype.getOrInsertComputed",
  "Set.prototype.union", "Set.prototype.intersection", "Set.prototype.difference",
  "Set.prototype.symmetricDifference", "Set.prototype.isSubsetOf", "Set.prototype.isSupersetOf",
  "Set.prototype.isDisjointFrom", "Object.hasOwn", "Date.prototype.toTemporalInstant", "Error.captureStackTrace",
  "WeakRef", "FinalizationRegistry", "ShadowRealm", "Symbol.dispose", "Symbol.asyncDispose", "Symbol.prototype.description",
];
const resolve = path => `path.split('.').reduce((o,k)=>o[k],globalThis)`.replace("path", JSON.stringify(path));
for (const api of apis) {
  prog("", `typeof (${resolve(api)})`);
  prog("", `{const f=${resolve(api)};return [f.name,f.length]}`);
}
// Descritor das funções e dos acessores.
for (const api of ["Math.sumPrecise", "Error.isError", "Promise.try", "Iterator.concat", "RegExp.escape", "JSON.rawJSON", "Object.groupBy"]) {
  const [owner, name] = [api.slice(0, api.lastIndexOf(".")), api.slice(api.lastIndexOf(".") + 1)];
  prog("", `Object.getOwnPropertyDescriptor(${owner},${JSON.stringify(name)})`);
}
for (const sym of ["dispose", "asyncDispose"])
  prog("", `{const d=Object.getOwnPropertyDescriptor(Symbol,"${sym}");return [d.writable,d.enumerable,d.configurable,String(d.value),d.value.description]}`);
prog("", "Object.prototype.toString.call(new DisposableStack())", "Object.prototype.toString.call(new AsyncDisposableStack())",
  "Object.prototype.toString.call(Math)", "Object.prototype.toString.call(Iterator.prototype)", "Object.prototype.toString.call(new Float16Array(1))",
  "Object.prototype.toString.call(new Intl.Locale('en'))", "Object.prototype.toString.call(Temporal)", "Object.prototype.toString.call(Temporal.Now)",
  "Object.prototype.toString.call(Temporal.Instant.fromEpochMilliseconds(0))", "Object.prototype.toString.call(new SuppressedError(1,2))",
  "Object.prototype.toString.call(Iterator.concat([]))", "Object.prototype.toString.call(Iterator.zip([]))");
prog("", "Reflect.ownKeys(DisposableStack.prototype).map(String)", "Reflect.ownKeys(AsyncDisposableStack.prototype).map(String)",
  "Reflect.ownKeys(SuppressedError.prototype).map(String)", "Reflect.ownKeys(Iterator).map(String)",
  "Reflect.ownKeys(Iterator.prototype).map(String)", "Reflect.ownKeys(Intl.Locale.prototype).map(String)",
  "Reflect.ownKeys(Temporal.Now).map(String)", "Reflect.ownKeys(Temporal.Instant).map(String)",
  "Reflect.ownKeys(Temporal.Instant.prototype).map(String)", "Reflect.ownKeys(ArrayBuffer.prototype).map(String)",
  "Reflect.ownKeys(Float16Array).map(String)", "Reflect.ownKeys(JSON).map(String)", "Reflect.ownKeys(Atomics).map(String)");

// ---- 2. Uint8Array base64 e hex.
{
  const enc = ["", "f", "fo", "foo", "foob", "fooba", "foobar", "ÿþý", "\u0000\u0001"];
  for (const s of enc) {
    const bytes = `new Uint8Array(Array.from(${JSON.stringify(s)},c=>c.charCodeAt(0)))`;
    prog("", `${bytes}.toBase64()`, `${bytes}.toBase64({alphabet:"base64url"})`, `${bytes}.toBase64({omitPadding:true})`,
      `${bytes}.toBase64({alphabet:"base64url",omitPadding:true})`, `${bytes}.toHex()`);
  }
  const b64 = ["", "Zg==", "Zg", "Zm8=", "Zm9v", "Zm9vYg==", "Zm9vYg", "Zm9v\nYg==", " Zm9v ", "Zm9v!", "Zg=", "Z", "Zh==", "Zm9=", "-_-_", "+/+/", "====", "Zm9vYmFy", "Zm9vY", "Zg==Zg=="];
  for (const s of b64) {
    const q = JSON.stringify(s);
    prog("", `Uint8Array.fromBase64(${q})`, `Uint8Array.fromBase64(${q},{alphabet:"base64url"})`,
      `Uint8Array.fromBase64(${q},{lastChunkHandling:"strict"})`, `Uint8Array.fromBase64(${q},{lastChunkHandling:"stop-before-partial"})`,
      `{const u=new Uint8Array(4);const r=u.setFromBase64(${q});return [r,u]}`,
      `{const u=new Uint8Array(2);const r=u.setFromBase64(${q},{lastChunkHandling:"stop-before-partial"});return [r,u]}`);
  }
  const hex = ["", "00", "ff", "FF", "abcd", "ABCD", "abc", "zz", "0g", "a b", "0x00", "deadBEEF"];
  for (const s of hex) {
    const q = JSON.stringify(s);
    prog("", `Uint8Array.fromHex(${q})`, `{const u=new Uint8Array(2);const r=u.setFromHex(${q});return [r,u]}`);
  }
  prog("", "Uint8Array.fromBase64(1)", "Uint8Array.fromBase64()", "Uint8Array.fromBase64({})", "Uint8Array.fromHex(1)", "Uint8Array.fromHex()",
    'Uint8Array.fromBase64("",{alphabet:"x"})', 'Uint8Array.fromBase64("",{lastChunkHandling:"x"})', 'Uint8Array.fromBase64("",1)',
    'new Uint8Array(1).toBase64({alphabet:"x"})', "new Uint8Array(1).toBase64(1)", "Uint8Array.prototype.toBase64.call([])",
    "Uint8Array.prototype.toHex.call(new Int8Array(1))", "Uint8Array.prototype.toHex.call({})", "new Uint8Array(1).setFromBase64(1)",
    "new Uint8Array(1).setFromHex(1)", "Uint8Array.prototype.setFromHex.call(new Int8Array(1),'00')", "Uint8Array.fromBase64.call(Array,'Zg==')",
    "Uint8Array.fromHex.call(Int8Array,'00')", "typeof Int8Array.fromBase64", "typeof Uint8ClampedArray.fromHex", "Uint8Array.fromBase64('Zg==').buffer.byteLength",
    "{const u=new Uint8Array(4);u.buffer.transfer();return u.setFromBase64('Zg==')}", "{const u=new Uint8Array(4);u.buffer.transfer();return u.toHex()}",
    "{const o={};Object.defineProperty(o,'alphabet',{get(){throw new RangeError('boom')}});return Uint8Array.fromBase64('',o)}");
}

// ---- 3. Math.sumPrecise, Math.f16round.
{
  const lists = ["[]", "[1,2,3]", "[0.1,0.2,0.3]", "[1e308,1e308,-1e308]", "[1e308,1e308]", "[-0]", "[-0,-0]", "[0,-0]", "[NaN,1]", "[Infinity,-Infinity]",
    "[Infinity,1]", "[-Infinity,1]", "[1e20,1,-1e20]", "[0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1]", "[Number.MAX_VALUE,Number.MAX_VALUE]",
    "[5e-324,5e-324]", "[1,1e-17,-1]", "new Set([1,2])", "'ab'", "[1,'2']", "[1n]", "[undefined]", "[null]", "[true]", "[{}]", "123", "null", "(function*(){yield 1;yield 2})()"];
  for (const l of lists) prog("", `Math.sumPrecise(${l})`);
  for (const x of ["0", "-0", "1", "1.5", "65504", "65519", "65520", "65536", "1e-8", "6e-8", "5.960464477539063e-8", "2.98e-8", "2.9802322387695312e-8", "NaN",
    "Infinity", "-Infinity", "0.1", "1.0009765625", "1.00048828125", "1.0004882812500001", "'x'", "undefined", "1n", "{valueOf(){return 3.14159}}", "Symbol()"])
    prog("", `Math.f16round(${x})`);
  prog("", "Math.f16round()", "Math.f16round.call(null,2)");
}

// ---- 4. Error.isError, SuppressedError.
{
  const vals = ["new Error()", "new TypeError()", "new AggregateError([])", "new SuppressedError(1,2)", "Object.create(Error.prototype)", "{__proto__:Error.prototype}",
    "Error.prototype", "{name:'Error',message:'x'}", "null", "undefined", "1", "'Error'", "Symbol()", "new Proxy(new Error(),{})", "class E extends Error{}", "new (class E extends Error{})",
    "new DOMException('x')", "(()=>{try{null.x}catch(e){return e}})()", "Error", "Error('x')", "new Proxy({},{})", "(()=>{const e=new Error();Object.setPrototypeOf(e,null);return e})()",
    "(()=>{const e=new Error();e[Symbol.toStringTag]='x';return e})()"];
  for (const v of vals) prog("", `Error.isError(${v})`);
  prog("", "Error.isError()", "Error.isError.call(null,new Error())", "new SuppressedError()", "new SuppressedError(1)", "new SuppressedError(1,2)",
    "new SuppressedError(1,2,'m')", "new SuppressedError(1,2,'m',{cause:3})", "SuppressedError(1,2,'m')", "SuppressedError.length", "SuppressedError.name",
    "SuppressedError.prototype.name", "SuppressedError.prototype.message", "Object.getPrototypeOf(SuppressedError)===Error", "Object.getPrototypeOf(SuppressedError.prototype)===Error.prototype",
    "Object.getOwnPropertyNames(new SuppressedError(1,2,'m')).join()", "{const e=new SuppressedError(1,2,'m');return Object.getOwnPropertyDescriptor(e,'error')}",
    "{const e=new SuppressedError(1,2,'m');return Object.getOwnPropertyDescriptor(e,'suppressed')}", "String(new SuppressedError(1,2,'m'))",
    "new SuppressedError(1,2,{toString(){return 'obj'}}).message", "new SuppressedError(1,2,undefined).hasOwnProperty('message')",
    "new SuppressedError(1,2,'m') instanceof Error", "Object.prototype.toString.call(SuppressedError.prototype)", "new SuppressedError(1,2,'m').stack.split('\\n')[0]");
}

// ---- 5. Promise.try, withResolvers, Array.fromAsync, Object.groupBy.
{
  prog("", "async:Promise.try(()=>1)", "async:Promise.try(()=>{throw new Error('x')})", "async:Promise.try((a,b)=>[a,b],1,2)", "async:Promise.try(()=>Promise.resolve(5))",
    "async:Promise.try(()=>Promise.reject(7))", "async:Promise.try(1)", "async:Promise.try()", "async:Promise.try.call(1,()=>1)", "async:Promise.try.call(undefined,()=>1)",
    "async:Promise.try.call(function(e){e(()=>{},()=>{})},()=>1)", "Promise.try(()=>1) instanceof Promise", "async:{class P extends Promise{};const p=P.try(()=>1);return [p instanceof P,await p]}",
    "{const o=Promise.withResolvers();return Object.keys(o)}", "{const o=Promise.withResolvers();return [typeof o.resolve,typeof o.reject,o.resolve.length,o.reject.length,o.resolve.name]}",
    "Promise.withResolvers.call(1)", "Promise.withResolvers.call(function(){})", "Promise.withResolvers.call(undefined)",
    "async:{const o=Promise.withResolvers();o.resolve(3);return o.promise}", "async:{const o=Promise.withResolvers();o.reject(new Error('r'));return o.promise}",
    "async:{class P extends Promise{};const o=P.withResolvers();return o.promise instanceof P}", "Object.getPrototypeOf(Promise.withResolvers())===Object.prototype");
  const inputs = ["[1,2,3]", "[Promise.resolve(1),2]", "(async function*(){yield 1;yield 2})()", "(function*(){yield 1;yield Promise.resolve(2)})()", "{length:2,0:'a',1:'b'}",
    "new Set([1,2])", "new Map([[1,2]])", "'abc'", "[]", "{}", "[Promise.reject(new Error('rej'))]", "null", "undefined", "1", "{length:-1}", "{length:Infinity}",
    "(async function*(){yield 1;throw new Error('gen')})()", "{[Symbol.asyncIterator](){return {next(){return {done:true}}}}}", "{[Symbol.iterator]:1}", "{[Symbol.asyncIterator]:1}"];
  for (const i of inputs) {
    prog("", `async:Array.fromAsync(${i})`, `async:Array.fromAsync(${i},x=>x+'!')`, `async:Array.fromAsync(${i},function(x){return this.p+x},{p:'t'})`);
  }
  prog("", "async:Array.fromAsync([1],1)", "async:Array.fromAsync([1],null)", "async:Array.fromAsync.call(Object,[1,2])", "async:Array.fromAsync.call(1,[1])",
    "async:Array.fromAsync([1,2],async x=>x*2)", "Array.fromAsync([]) instanceof Promise", "async:Array.fromAsync()", "async:Array.fromAsync([1,2],()=>{throw new Error('m')})",
    "async:{class A extends Array{};const r=await Array.fromAsync.call(A,[1]);return [r instanceof A,r.length]}");
  const gb = ["[1,2,3,4]", "'abca'", "new Set([1,2,3])", "[]", "[{k:'a'},{k:'b'},{k:'a'}]"];
  for (const g of gb) {
    prog("", `Object.groupBy(${g},x=>typeof x==='object'?x.k:(x%2?'odd':'even'))`, `Map.groupBy(${g},x=>typeof x==='object'?x.k:x%2)`,
      `Object.getPrototypeOf(Object.groupBy(${g},x=>x))`, `Object.keys(Object.groupBy(${g},(x,i)=>i))`);
  }
  prog("", "Object.groupBy(null,x=>x)", "Object.groupBy([1],null)", "Object.groupBy([1])", "Object.groupBy(1,x=>x)", "Map.groupBy([1],1)", "Map.groupBy(undefined,x=>x)",
    "Object.groupBy([1,2],()=>Symbol.iterator)", "Object.groupBy([1],()=>({toString(){return 'k'}}))", "Object.groupBy([-0,0],x=>x)", "Map.groupBy([-0,0],x=>x)",
    "Object.groupBy([1],()=>{throw new Error('g')})", "Object.groupBy({},x=>x)", "Object.groupBy('ab',x=>x)", "Object.getOwnPropertyDescriptor(Object.groupBy([1],x=>x),'1')");
}

// ---- 6. Iterator e helpers.
{
  const it = "(function*(){yield 1;yield 2;yield 3;yield 4})()";
  prog("", `${it}.map(x=>x*2).toArray()`, `${it}.filter(x=>x%2).toArray()`, `${it}.take(2).toArray()`, `${it}.drop(2).toArray()`, `${it}.flatMap(x=>[x,x]).toArray()`,
    `${it}.reduce((a,b)=>a+b)`, `${it}.reduce((a,b)=>a+b,10)`, `${it}.some(x=>x>3)`, `${it}.every(x=>x>0)`, `${it}.find(x=>x>2)`,
    `{const r=[];${it}.forEach((x,i)=>r.push([x,i]));return r}`, `${it}.map((x,i)=>i).toArray()`, `${it}.take(0).toArray()`, `${it}.drop(10).toArray()`,
    `${it}.take(Infinity).toArray()`, `${it}.drop(Infinity).toArray()`, `${it}.take(-1)`, `${it}.take(NaN)`, `${it}.drop(-1)`, `${it}.drop('x')`, `${it}.take()`,
    `${it}.map()`, `${it}.map(1)`, `${it}.filter()`, `${it}.flatMap(x=>1).toArray()`, `${it}.flatMap(x=>'ab').toArray()`, `${it}.reduce()`, `${it}.reduce(1)`,
    `[].values().reduce((a,b)=>a+b)`, `[].values().reduce((a,b)=>a+b,5)`, `${it}.some()`, `${it}.every(1)`, `${it}.find()`, `${it}.forEach()`,
    `Iterator.prototype.map.call({},x=>x)`, `Iterator.prototype.map.call(1,x=>x)`, `Iterator.prototype.toArray.call({next(){return {done:true}}})`,
    `Iterator.prototype.toArray.call({next:1})`, `Iterator.from([1,2]).toArray()`, `Iterator.from({next(){return {done:true}}}).toArray()`, `Iterator.from('ab').toArray()`,
    `Iterator.from(1)`, `Iterator.from(null)`, `Iterator.from({})`, `Iterator.from({next:1})`, `Iterator.from([1]) instanceof Iterator`, `Iterator.from({next(){return {done:true}}}) instanceof Iterator`,
    `Object.getPrototypeOf(Iterator.from({next(){return {done:true}}}))===Iterator.prototype`, `new Iterator()`, `Iterator()`, `Iterator.prototype[Symbol.toStringTag]`,
    `{class I extends Iterator{};return new I() instanceof Iterator}`, `Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).get.name`,
    `Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').get.name`, `Iterator.prototype[Symbol.iterator].call(5)`, `typeof Iterator.prototype[Symbol.iterator]`,
    `[1,2].values().map(x=>x).next()`, `{const m=[1,2].values().map(x=>x);return [m.next(),m.next(),m.next(),m.next()]}`, `{const m=[1].values().map(x=>x);return [m.return(),m.next()]}`,
    `Object.getPrototypeOf([].values().map(x=>x))[Symbol.toStringTag]`, `Object.getPrototypeOf(Object.getPrototypeOf([].values().map(x=>x)))===Iterator.prototype`,
    `{let c=0;const i={next(){return {done:false,value:++c}},return(){c=-1;return {}}};i.__proto__=Iterator.prototype;i.take(1).toArray();return c}`,
    `{let c=0;const i={next(){return {done:false,value:1}},return(){c++;return {}}};Object.setPrototypeOf(i,Iterator.prototype);i.map(x=>{throw 1}).next.call;try{i.map(x=>{throw 1}).next()}catch(e){}return c}`,
    `{let c=0;const i={next(){return {done:false,value:1}},return(){c++;return {}}};Object.setPrototypeOf(i,Iterator.prototype);try{i.map(x=>{throw 1}).next()}catch(e){}return c}`,
    `{let c=0;const i={next(){return {done:false,value:1}},return(){c++;return {}}};Object.setPrototypeOf(i,Iterator.prototype);i.some(x=>true);return c}`,
    `{const i=Object.setPrototypeOf({next(){return 1}},Iterator.prototype);return i.toArray()}`, `{const i=Object.setPrototypeOf({next(){return {done:false,value:1}}},Iterator.prototype);return i.map(x=>x).next()}`);
  // Iterator.concat e Iterator.zip.
  prog("", "Iterator.concat([1,2],[3]).toArray()", "Iterator.concat().toArray()", "Iterator.concat('ab').toArray()", "Iterator.concat(1)", "Iterator.concat({})",
    "Iterator.concat([1],null)", "Iterator.concat([1],new Set([2])).toArray()", "Iterator.concat({[Symbol.iterator]:1})", "Iterator.concat({[Symbol.iterator](){return 1}})",
    "Iterator.concat.length", "Iterator.concat([]) instanceof Iterator", "{const c=Iterator.concat([1]);return [c.next(),c.next(),c.next()]}",
    "{const c=Iterator.concat([1,2]);return [c.return(),c.next()]}", "Iterator.zip([[1,2],[3,4]]).toArray()", "Iterator.zip([[1,2],[3]]).toArray()",
    "Iterator.zip([[1,2],[3]],{mode:'longest'}).toArray()", "Iterator.zip([[1,2],[3]],{mode:'longest',padding:[0,9]}).toArray()", "Iterator.zip([[1,2],[3]],{mode:'strict'}).toArray()",
    "Iterator.zip([[1],[1]],{mode:'x'})", "Iterator.zip([[1]],{mode:'longest',padding:1})", "Iterator.zip(1)", "Iterator.zip()", "Iterator.zip([1])", "Iterator.zip([],1)",
    "Iterator.zip([]).toArray()", "Iterator.zipKeyed({a:[1,2],b:[3,4]}).toArray()", "Iterator.zipKeyed({a:[1],b:[3,4]},{mode:'longest'}).toArray()", "Iterator.zipKeyed(1)",
    "Iterator.zipKeyed({}).toArray()", "Iterator.zip.length", "Iterator.zipKeyed.length", "Object.getPrototypeOf(Iterator.zipKeyed({a:[1]}).toArray()[0])");
}

// ---- 7. Array e String.
{
  const arr = ["[3,1,2]", "[]", "[1,,3]", "[undefined,3,1]", "['b','a','C']", "{length:3,0:3,1:1,2:2}"];
  for (const a of arr) {
    prog("", `Array.prototype.toSorted.call(${a})`, `Array.prototype.toReversed.call(${a})`, `Array.prototype.toSpliced.call(${a},1,1)`,
      `Array.prototype.toSpliced.call(${a},0,0,9,8)`, `Array.prototype.with.call(${a},0,'x')`, `Array.prototype.with.call(${a},-1,'x')`, `Array.prototype.with.call(${a},9,'x')`,
      `Array.prototype.findLast.call(${a},x=>x>1)`, `Array.prototype.findLastIndex.call(${a},x=>x>1)`, `Array.prototype.at.call(${a},-1)`, `Array.prototype.toSorted.call(${a},(x,y)=>y-x)`);
  }
  prog("", "[1].toSorted(1)", "[1].toSorted(null)", "[1].with(1,0)", "[1].with(-2,0)", "[1].with(NaN,0)", "[1].with('0',9)", "[1].with()", "[1].toSpliced()", "[1].toSpliced(0)",
    "[1,2,3].toSpliced(1)", "[1,2,3].toSpliced(-1,5,'a')", "Array.prototype.toSorted.call(null)", "Array.prototype.with.call(undefined,0,0)", "{const a=[1,2];const b=a.toReversed();return [a===b,a]}",
    "{class A extends Array{};const a=A.from([3,1]);return [a.toSorted() instanceof A,a.toSorted().constructor.name]}", "{const a={length:2**32};return Array.prototype.toReversed.call(a)}",
    "{const a={length:2**32-1};return Array.prototype.with.call(a,0,0)}", "[1,2,3].findLast(x=>x<3)", "[1,2,3].findLast(1)", "[].findLast(x=>x)", "[1,2,3].findLastIndex(x=>x>9)",
    "Array.prototype[Symbol.unscopables].toSorted", "Array.prototype[Symbol.unscopables].with", "Array.prototype[Symbol.unscopables].findLast", "Object.getPrototypeOf(Array.prototype[Symbol.unscopables])",
    "new Int8Array([3,1,2]).toSorted()", "new Int8Array([3,1,2]).toReversed()", "new Int8Array([3,1,2]).with(1,9)", "new Int8Array([3,1,2]).with(5,9)", "new Int8Array([3,1,2]).with(0,'x')",
    "new Int8Array([3,1,2]).toSorted(1)", "new BigInt64Array(1).with(0,1)", "new Int8Array([3,1,2]).findLast(x=>x<3)", "Int8Array.prototype.toSorted.call([])",
    "new Float64Array([NaN,-0,0,1]).toSorted()");
  const strs = ["'abc'", "'a\\uD800b'", "'\\uDC00'", "'\\uD83D\\uDE00'", "'\\uD83D'", "''", "'\\uD83D\\uDE00\\uD800'", "'\\uDE00\\uD83D'"];
  for (const s of strs) prog("", `${s}.isWellFormed()`, `${s}.toWellFormed().length`, `${s}.toWellFormed().charCodeAt(1)`, `escape(${s}.toWellFormed())`, `encodeURIComponent(${s}.toWellFormed())`);
  prog("", "String.prototype.isWellFormed.call(null)", "String.prototype.toWellFormed.call(undefined)", "String.prototype.isWellFormed.call(1)", "String.prototype.toWellFormed.call({toString(){return 'x\\uD800'}}).charCodeAt(1)",
    "'abc'.at(-1)", "'abc'.at(3)", "'abc'.at(NaN)", "'abc'.replaceAll('b','$&$&')", "'abc'.replaceAll(/b/,'x')", "'abc'.replaceAll(/b/g,'x')", "'aaa'.replaceAll('','-')", "'abc'.matchAll(/b/)", "[...'abab'.matchAll(/b/g)].length",
    "'abc'.matchAll('b').next().value[0]", "'x'.matchAll(1).next()");
}

// ---- 8. Atomics, RegExp.escape, JSON.rawJSON.
{
  prog("", "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)),0,1)", "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)),0,0,0)", "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)),0,0,1)",
    "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)),0,0)", "Atomics.waitAsync(new Int32Array(8),0,0)", "Atomics.waitAsync(new Int16Array(new SharedArrayBuffer(8)),0,0)",
    "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)),9,0)", "Atomics.waitAsync(new BigInt64Array(new SharedArrayBuffer(16)),0,0n,0)", "Atomics.waitAsync(new BigInt64Array(new SharedArrayBuffer(16)),0,0)",
    "async:{const i=new Int32Array(new SharedArrayBuffer(8));const r=Atomics.waitAsync(i,0,0,10);return [r.async,await r.value]}",
    "async:{const i=new Int32Array(new SharedArrayBuffer(8));const r=Atomics.waitAsync(i,0,0);Atomics.notify(i,0);return await r.value}",
    "async:{const i=new Int32Array(new SharedArrayBuffer(8));const r=Atomics.waitAsync(i,0,0);return [Atomics.notify(i,0,1),await r.value]}",
    "Atomics.pause()", "Atomics.pause(1)", "Atomics.pause(1.5)", "Atomics.pause('1')", "Atomics.pause(-1)", "Atomics.pause(Infinity)", "Atomics.pause(-0)", "Atomics.pause(NaN)",
    "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)),0,1)", "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)),0,0,1)", "Atomics.wait(new Int32Array(8),0,0,1)");
  const esc = ["''", "'a.b'", "'^$\\\\.*+?()[]{}|/'", "'1abc'", "'a1'", "' '", "'\\n'", "'-'", "',=<>#&!%:;@~\\'\"`'", "'\\u00e9'", "'\\u2028'", "'\\uD800'", "'_'", "'abc'", "'\\t\\v\\f\\r'", "'\\ufeff'", "'a-b'", "'/'"];
  for (const e of esc) prog("", `RegExp.escape(${e})`, `new RegExp(RegExp.escape(${e})).test(${e})`);
  prog("", "RegExp.escape(1)", "RegExp.escape()", "RegExp.escape(null)", "RegExp.escape({})", "RegExp.escape(Symbol())", "RegExp.escape(new String('a'))", "RegExp.escape.call(null,'a')");
  const raw = ["'1'", "'-1.5e3'", "'\"x\"'", "'true'", "'null'", "'[]'", "'{}'", "'\"a\\\\nb\"'", "' 1'", "'1 '", "''", "'\\n1'", "'undefined'", "'NaN'", "'01'", "'1n'", "'abc'", "'[1]'", "'{\"a\":1}'", "'\\t1'", "'-0'", "'1e999'"];
  for (const r of raw) prog("", `JSON.stringify(JSON.rawJSON(${r}))`, `JSON.stringify({a:JSON.rawJSON(${r})})`, `JSON.isRawJSON(JSON.rawJSON(${r}))`, `Object.isFrozen(JSON.rawJSON(${r}))`);
  prog("", "JSON.rawJSON(1)", "JSON.rawJSON()", "JSON.rawJSON(null)", "JSON.rawJSON({})", "Object.getPrototypeOf(JSON.rawJSON('1'))", "Reflect.ownKeys(JSON.rawJSON('1'))", "JSON.rawJSON('1').rawJSON",
    "Object.getOwnPropertyDescriptor(JSON.rawJSON('1'),'rawJSON')", "JSON.isRawJSON({rawJSON:'1'})", "JSON.isRawJSON(1)", "JSON.isRawJSON()", "JSON.isRawJSON(null)", "JSON.stringify([JSON.rawJSON('1'),JSON.rawJSON('\"s\"')])",
    "JSON.stringify(JSON.rawJSON('1'),null,2)", "JSON.stringify({a:JSON.rawJSON('1')},(k,v)=>v,2)", "JSON.stringify({a:JSON.rawJSON('9007199254740993')})",
    "JSON.parse('[9007199254740993]',(k,v,c)=>c&&c.source)", "JSON.parse('{\"a\":1.0}',(k,v,c)=>c&&c.source)", "JSON.parse('\"x\"',(k,v,c)=>[typeof c,c&&c.source])", "JSON.parse('[1,[2]]',(k,v,c)=>Array.isArray(v)?typeof c:c.source)");
}

// ---- 9. Float16Array e DataView.
{
  prog("", "new Float16Array([1,1.5,65504,65520,1e-8,NaN,-0]).join()", "Float16Array.BYTES_PER_ELEMENT", "Float16Array.name", "Float16Array.length", "new Float16Array(2).byteLength",
    "Object.getPrototypeOf(Float16Array)===Object.getPrototypeOf(Int8Array)", "Float16Array.from([0.1,0.2]).join()", "Float16Array.of(0.1,1/3).join()", "new Float16Array([3,1,2]).toSorted().join()",
    "new Float16Array([1,2]).map(x=>x/3).join()", "new Float16Array(new Float32Array([0.1,0.2])).join()", "new Float64Array(new Float16Array([0.1,0.2])).join()",
    "new Float16Array([1,2,3]).with(0,0.1).join()", "new Float16Array([1,NaN]).includes(NaN)", "new Float16Array([1,2]).indexOf(2)", "new Float16Array([1]).fill(0.3).join()",
    "new Float16Array(new Uint8Array([0,60]).buffer).join()", "new Float16Array(new ArrayBuffer(3))", "new Float16Array(new ArrayBuffer(4),1)", "Float16Array()", "new Float16Array(-1)", "new Float16Array('x').length",
    "{const f=new Float16Array(1);f[0]=1e5;return f[0]}", "{const f=new Float16Array(1);f[0]=-1e5;return f[0]}", "{const f=new Float16Array(1);f[0]=1/3;return f[0]}", "{const f=new Float16Array(1);f[0]=0.1;return f[0]}",
    "Object.prototype.toString.call(Float16Array.prototype)", "Float16Array.prototype.constructor===Float16Array", "Float16Array.prototype.BYTES_PER_ELEMENT",
    "new Float16Array([5e-8,6e-8,3e-8,2e-8]).join()", "new Float16Array([65519.99,65520.0001]).join()", "new Float16Array([1.0009765625,1.0004882812]).join()");
  const dv = "new DataView(new ArrayBuffer(8))";
  prog("", `${dv}.getFloat16(0)`, `{const d=${dv};d.setFloat16(0,1.5);return [d.getUint8(0),d.getUint8(1),d.getFloat16(0),d.getFloat16(0,true)]}`,
    `{const d=${dv};d.setFloat16(0,1.5,true);return [d.getUint8(0),d.getUint8(1)]}`, `{const d=${dv};d.setFloat16(0,0.1);return d.getFloat16(0)}`, `{const d=${dv};d.setFloat16(0,65520);return d.getFloat16(0)}`,
    `{const d=${dv};d.setFloat16(0,NaN);return [d.getUint8(0),d.getUint8(1)]}`, `{const d=${dv};d.setFloat16(0,-0);return Object.is(d.getFloat16(0),-0)}`, `${dv}.getFloat16(7)`, `${dv}.getFloat16(-1)`,
    `${dv}.getFloat16()`, `${dv}.setFloat16(0)`, `{const d=${dv};return d.setFloat16(0,1)}`, `${dv}.setFloat16(7,1)`, `DataView.prototype.getFloat16.call({},0)`,
    `DataView.prototype.setFloat16.call(new Uint8Array(2),0,1)`, "DataView.prototype.getFloat16.length", "DataView.prototype.setFloat16.length", `{const b=new ArrayBuffer(8);const d=new DataView(b);b.transfer();return d.getFloat16(0)}`);
}

// ---- 10. Intl.Locale.
{
  const locs = ["en", "en-US", "pt-BR", "zh", "zh-TW", "ar", "he", "ja-JP", "und", "sr", "de-AT", "en-u-ca-gregory-hc-h12", "fa-IR", "ru-Latn", "es-419", "fr-CA", "ko", "hi", "th-u-nu-thai", "tr", "und-Cyrl", "az-Arab", "ps-AF"];
  for (const l of locs) {
    const q = JSON.stringify(l);
    prog("", `new Intl.Locale(${q}).maximize().toString()`, `new Intl.Locale(${q}).minimize().toString()`,
      `{const l=new Intl.Locale(${q});return [l.language,l.script,l.region,l.baseName,l.calendar,l.collation,l.hourCycle,l.caseFirst,l.numeric,l.numberingSystem,l.firstDayOfWeek]}`,
      `new Intl.Locale(${q}).getWeekInfo()`, `new Intl.Locale(${q}).getCalendars()`, `new Intl.Locale(${q}).getCollations()`, `new Intl.Locale(${q}).getHourCycles()`,
      `new Intl.Locale(${q}).getNumberingSystems()`, `new Intl.Locale(${q}).getTextInfo()`, `new Intl.Locale(${q}).getTimeZones()`);
  }
  prog("", "new Intl.Locale()", "new Intl.Locale(1)", "new Intl.Locale('')", "new Intl.Locale('x')", "new Intl.Locale('en_US')", "Intl.Locale('en')", "new Intl.Locale('en',{calendar:'x y'})",
    "new Intl.Locale('en',{calendar:'islamic'}).calendar", "new Intl.Locale('en',{hourCycle:'h13'})", "new Intl.Locale('en',{hourCycle:'h23'}).hourCycle", "new Intl.Locale('en',{caseFirst:'upper'}).toString()",
    "new Intl.Locale('en',{numeric:true}).toString()", "new Intl.Locale('en',{region:'BR'}).toString()", "new Intl.Locale('en',{script:'Latn'}).toString()", "new Intl.Locale('en',{language:'pt'}).toString()",
    "new Intl.Locale('en',{region:'x'})", "new Intl.Locale('en',{firstDayOfWeek:'mon'}).firstDayOfWeek", "new Intl.Locale('en',{firstDayOfWeek:1}).toString()", "new Intl.Locale('en-u-fw-mon').firstDayOfWeek",
    "new Intl.Locale('en',{firstDayOfWeek:8})", "new Intl.Locale(new Intl.Locale('pt-BR')).toString()", "new Intl.Locale(['en'])", "String(new Intl.Locale('en-US'))",
    "Intl.Locale.prototype.maximize.call({})", "Intl.Locale.prototype.getWeekInfo.call(1)", "Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'weekInfo')",
    "Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'language').get.name", "Intl.Locale.prototype[Symbol.toStringTag]", "Intl.Locale.length", "JSON.stringify(new Intl.Locale('en-US').getWeekInfo())",
    "Object.keys(new Intl.Locale('en-US').getWeekInfo())", "Object.keys(new Intl.Locale('en-US').getTextInfo())");
}

// ---- 11. Symbol.dispose e DisposableStack.
{
  const order = "const o=[]";
  prog(order, "{const s=new DisposableStack();s.defer(()=>o.push(1));s.defer(()=>o.push(2));s.dispose();return o}",
    "{const s=new DisposableStack();s.use({[Symbol.dispose](){o.push('a')}});s.adopt(5,v=>o.push(v));s.dispose();return o}",
    "{const s=new DisposableStack();return [s.disposed,s.dispose(),s.disposed,s.dispose()]}", "{const s=new DisposableStack();s.dispose();return s.use({[Symbol.dispose](){}})}",
    "{const s=new DisposableStack();s.dispose();return s.defer(()=>{})}", "{const s=new DisposableStack();s.dispose();return s.adopt(1,()=>{})}", "{const s=new DisposableStack();s.dispose();return s.move()}",
    "{const s=new DisposableStack();s.defer(()=>o.push('m'));const t=s.move();return [s.disposed,t.disposed,t===s,t instanceof DisposableStack,o]}",
    "{const s=new DisposableStack();s.defer(()=>o.push('m'));const t=s.move();s.dispose();t.dispose();return o}", "new DisposableStack().use(null)", "new DisposableStack().use(undefined)",
    "new DisposableStack().use(1)", "new DisposableStack().use({})", "new DisposableStack().use({[Symbol.dispose]:1})", "{const x={[Symbol.dispose](){}};return new DisposableStack().use(x)===x}",
    "new DisposableStack().adopt(1)", "new DisposableStack().adopt(1,1)", "new DisposableStack().adopt()", "new DisposableStack().adopt(1,()=>{})", "new DisposableStack().defer()", "new DisposableStack().defer(1)",
    "new DisposableStack().defer(()=>{})", "DisposableStack()", "DisposableStack.length", "new DisposableStack(1)", "DisposableStack.prototype.dispose.call({})", "DisposableStack.prototype.use.call(new AsyncDisposableStack(),null)",
    "Object.getOwnPropertyDescriptor(DisposableStack.prototype,'disposed').get.name", "DisposableStack.prototype[Symbol.dispose]===DisposableStack.prototype.dispose", "DisposableStack.prototype[Symbol.toStringTag]",
    "{const s=new DisposableStack();s.defer(()=>{throw new Error('a')});s.dispose()}", "{const s=new DisposableStack();s.defer(()=>{throw new Error('a')});s.defer(()=>{throw new Error('b')});try{s.dispose()}catch(e){return [e.constructor.name,e.error.message,e.suppressed.message]}}",
    "{const s=new DisposableStack();s.defer(()=>{throw 1});s.defer(()=>{throw 2});s.defer(()=>{throw 3});try{s.dispose()}catch(e){return [e.error,e.suppressed.error,e.suppressed.suppressed]}}",
    "{const s=new DisposableStack();s.defer(()=>{throw 1});s.defer(()=>o.push('ran'));try{s.dispose()}catch(e){return [e,o]}}",
    "{const s=new DisposableStack();s.defer(function(){return this});s.defer(function(){o.push(this)});s.dispose();return o}",
    "{const s=new DisposableStack();s.adopt(7,function(v){o.push([v,arguments.length,this])});s.dispose();return o}",
    "{const s=new DisposableStack();const d={[Symbol.dispose](){o.push(this===d)}};s.use(d);s.dispose();return o}",
    "{const s=new DisposableStack();s.defer(()=>{s.dispose();o.push('in')});s.dispose();return [o,s.disposed]}",
    "{const s=new DisposableStack();s.defer(()=>{o.push(s.disposed)});s.dispose();return o}",
    "{const s=new DisposableStack();s.defer(()=>{try{s.defer(()=>{})}catch(e){o.push(e.constructor.name)}});s.dispose();return o}");
  prog("const o=[]", "async:{const s=new AsyncDisposableStack();s.defer(async()=>o.push(1));s.defer(()=>o.push(2));await s.disposeAsync();return o}",
    "async:{const s=new AsyncDisposableStack();s.use({[Symbol.asyncDispose](){o.push('a')}});s.use({[Symbol.dispose](){o.push('s')}});s.adopt(5,async v=>o.push(v));await s.disposeAsync();return o}",
    "async:{const s=new AsyncDisposableStack();return [s.disposed,await s.disposeAsync(),s.disposed,await s.disposeAsync()]}",
    "AsyncDisposableStack.prototype.disposeAsync.call({})", "async:AsyncDisposableStack.prototype.disposeAsync.call({})", "async:{const s=new AsyncDisposableStack();await s.disposeAsync();return s.use({[Symbol.asyncDispose](){}})}",
    "async:{const s=new AsyncDisposableStack();await s.disposeAsync();return s.defer(()=>{})}", "new AsyncDisposableStack().use(1)", "new AsyncDisposableStack().use({})", "new AsyncDisposableStack().use({[Symbol.asyncDispose]:1})",
    "new AsyncDisposableStack().use(null)", "new AsyncDisposableStack().adopt(1)", "new AsyncDisposableStack().defer()", "AsyncDisposableStack()", "AsyncDisposableStack.length",
    "async:{const s=new AsyncDisposableStack();s.defer(()=>{throw new Error('a')});await s.disposeAsync()}", "async:{const s=new AsyncDisposableStack();s.defer(()=>{throw 1});s.defer(async()=>{throw 2});try{await s.disposeAsync()}catch(e){return [e.constructor.name,e.error,e.suppressed]}}",
    "async:{const s=new AsyncDisposableStack();s.defer(()=>o.push('x'));const t=s.move();await s.disposeAsync();await t.disposeAsync();return [o,t instanceof AsyncDisposableStack]}",
    "AsyncDisposableStack.prototype[Symbol.asyncDispose]===AsyncDisposableStack.prototype.disposeAsync", "AsyncDisposableStack.prototype[Symbol.toStringTag]",
    "Object.getOwnPropertyDescriptor(AsyncDisposableStack.prototype,'disposed').get.name", "async:{const s=new AsyncDisposableStack();s.defer(async()=>{await null;o.push(1)});s.defer(async()=>{await null;o.push(2)});await s.disposeAsync();return o}",
    "async:{const s=new AsyncDisposableStack();const p=s.disposeAsync();return [s.disposed,p instanceof Promise]}");
}

// ---- 12. using e await using.
{
  const D = "const o=[];const d=n=>({[Symbol.dispose](){o.push(n)}});const ad=n=>({async [Symbol.asyncDispose](){await null;o.push(n)}});";
  const blocks = [
    "{using a=d(1);o.push('body')}", "{using a=d(1),b=d(2);o.push('body')}", "{using a=null;using b=undefined;o.push('body')}", "{using a=1}", "{using a={}}", "{using a=d(1);throw new Error('x')}",
    "{using a=d(1);return 5}", "{using a={[Symbol.dispose](){throw new Error('d')}};o.push('b')}", "{using a={[Symbol.dispose](){throw new Error('d')}};throw new Error('x')}",
    "{using a=d(1);{using b=d(2);o.push('in')}o.push('out')}", "{for(using a of [d(1),d(2)])o.push('l')}", "{for(using a of [d(1),null])o.push('l')}", "{for(using a of [1])o.push('l')}",
    "{switch(1){case 1:using a=d(1);o.push('c')}}", "{try{using a=d(1);throw 1}catch(e){o.push('catch')}finally{o.push('fin')}}", "{using a=d(1);a=d(2)}",
    "{using a=d(1);return ()=>a}", "{label:{using a=d(1);break label}o.push('after')}", "{for(let i=0;i<2;i++){using a=d(i);if(i==0)continue}}", "{using a={[Symbol.dispose]:undefined}}",
    "{using a={[Symbol.dispose]:null}}", "{using a={[Symbol.dispose]:1}}", "{using a=Symbol()}", "{using a=()=>{}}", "{using a={[Symbol.asyncDispose](){}}}", "{using a=d(1);using a2=d(1)}",
    "{const x=d(7);using a=x;o.push(a===x)}", "{using a=d(1);yield_=1}", "{using [a]=[d(1)]}", "{using {a}={a:d(1)}}", "{using a;}", "{using\na=d(1)}", "{using a=d(1) ,b}",
    "{var using=1;using\nx=2;o.push(using)}", "{let using=3;o.push(using)}", "{using in {};o.push(1)}", "{const using=1;o.push(using)}", "{for(using of of [d(1)]){}}", "{for(using a in {x:1}){}}",
    "{for(using a=d(1);;){break}}", "{using a=d(1);o.push((()=>{using b=d(2);return 3})())}",
  ];
  for (const b of blocks) prog(D, `{try{(()=>${b.startsWith("{") ? b : "{" + b + "}"})()}catch(e){o.push(e.constructor.name+':'+e.message)}return o}`);
  // Funções, genéricos e escopo de topo via eval.
  const evals = ["{using a=d(1)}", "using a=d(1);o.push('top')", "{using a=d(1);o.push('x')}", "using a=1", "await using a=1", "{await using a=ad(1)}", "{await using a=1}"];
  for (const e of evals) prog(D, `{try{(0,eval)(${JSON.stringify(e)});return o}catch(x){return [x.constructor.name,x.message,o]}}`);
  const abl = [
    "{await using a=ad(1);o.push('body')}", "{await using a=d(1);o.push('body')}", "{await using a=ad(1),b=ad(2);o.push('body')}", "{await using a=null;o.push('body')}", "{await using a=1}", "{await using a={}}",
    "{await using a=ad(1);throw new Error('x')}", "{await using a={async [Symbol.asyncDispose](){throw new Error('d')}};throw new Error('x')}", "{using a=d(1);await using b=ad(2);o.push('body')}",
    "{await using a=ad(1);using b=d(2);o.push('body')}", "{for await(await using a of [ad(1),ad(2)])o.push('l')}", "{for(await using a of [ad(1)])o.push('l')}", "{for await(using a of [d(1)])o.push('l')}",
    "{await using a={[Symbol.asyncDispose]:null,[Symbol.dispose](){o.push('sync')}}}", "{await using a={[Symbol.asyncDispose]:undefined,[Symbol.dispose](){o.push('sync')}}}",
    "{await using a={[Symbol.dispose](){throw new Error('sd')}}}", "{await using a={[Symbol.asyncDispose](){return Promise.reject(new Error('rej'))}}}", "{await using a=ad(1);await null;o.push('b2')}",
    "{await using x=ad(1);return 9}", "{await using a=ad(1);{await using b=ad(2)}o.push('mid')}", "{await using\na=ad(1);o.push('b')}",
  ];
  for (const b of abl) prog(D, `async:{try{await(async()=>${b.startsWith("{") ? b : "{" + b + "}"})()}catch(e){o.push(e.constructor.name+':'+(e.message||''))}return o}`);
  prog(D, "async:{try{await(async()=>{using a={[Symbol.dispose](){throw 1}};using b={[Symbol.dispose](){throw 2}}})()}catch(e){return [e.constructor.name,e.error,e.suppressed]}}",
    "async:{try{await(async()=>{await using a={async [Symbol.asyncDispose](){throw 1}};throw 0})()}catch(e){return [e.constructor.name,e.error,e.suppressed]}}",
    "async:{try{await(async()=>{using a={[Symbol.dispose](){throw 1}};throw 0})()}catch(e){return [e.constructor.name,e.error,e.suppressed,e.message]}}",
    "{function*g(){using a=d(1);yield 1;yield 2}const it=g();it.next();it.return();return o}", "{function*g(){using a=d(1);yield 1;yield 2}const it=g();it.next();it.next();it.next();return o}",
    "{function*g(){using a=d(1);yield 1}const it=g();it.next();try{it.throw(new Error('t'))}catch(e){o.push(e.message)}return o}", "{function*g(){using a=d(1);yield 1}g();return o}",
    "{async function*g(){await using a=ad(1);yield 1;yield 2}const it=g();it.next();return it.return().then(()=>o)}",
    "{class C{static{using a=d(1);o.push('s')}};return o}", "{const f=()=>{using a=d(1);return o.length};return [f(),o]}", "{const f=function(){using a=d(1);return arguments.length};return [f(1,2),o]}",
    "{class C{m(){using a=d(1);return 1}};return [new C().m(),o]}", "{const f=async()=>{await using a=ad(1);return 1};return f().then(v=>[v,o])}", "{const x={get g(){using a=d(1);return 2}};return [x.g,o]}",
    "{const f=()=>{using a=d(1);throw 1};try{f()}catch(e){}return o}", "{const f=()=>{try{using a=d(1);return 1}finally{o.push('f')}};return [f(),o]}",
    "{const f=()=>{using a=d(1);try{return 1}finally{o.push('f')}};return [f(),o]}");
}

// ---- 13. Temporal.Instant e Temporal.Now básicos.
{
  const ins = ["0", "1700000000000", "-1", "8.64e15", "8.64e15+1"];
  for (const i of ins) prog("", `Temporal.Instant.fromEpochMilliseconds(${i}).toString()`, `Temporal.Instant.fromEpochMilliseconds(${i}).epochMilliseconds`,
    `Temporal.Instant.fromEpochMilliseconds(${i}).epochNanoseconds`);
  const strs = ["'1970-01-01T00:00:00Z'", "'2020-02-29T12:34:56.789123456Z'", "'2020-02-29T12:34:56+01:00'", "'2020-02-29T12:34:56'", "'2020-02-29'", "'x'", "'2020-02-29T12:34:56Z[UTC]'",
    "'2020-02-29T12:34:56+01:00[Europe/Paris]'", "'2020-02-29T24:00:00Z'", "'2020-02-30T00:00:00Z'", "'+275760-09-13T00:00:00Z'", "'+275760-09-13T00:00:00.000000001Z'", "'-271821-04-20T00:00:00Z'",
    "'1970-01-01T00:00:00.1Z'", "'1970-01-01T00:00Z'", "'19700101T000000Z'", "'1970-01-01 00:00:00Z'", "'1970-01-01t00:00:00z'", "1", "undefined", "null", "{}"];
  for (const s of strs) prog("", `Temporal.Instant.from(${s}).toString()`, `Temporal.Instant.from(${s}).epochNanoseconds`);
  prog("", "Temporal.Instant.fromEpochNanoseconds(0n).toString()", "Temporal.Instant.fromEpochNanoseconds(1)", "Temporal.Instant.fromEpochNanoseconds(8640000000000000000001n)",
    "Temporal.Instant.fromEpochNanoseconds('1')", "new Temporal.Instant(0n).toString()", "new Temporal.Instant(0)", "Temporal.Instant(0n)", "Temporal.Instant.from(Temporal.Instant.fromEpochMilliseconds(5)).epochMilliseconds",
    "Temporal.Instant.fromEpochMilliseconds(0).add({hours:1}).toString()", "Temporal.Instant.fromEpochMilliseconds(0).add({days:1})", "Temporal.Instant.fromEpochMilliseconds(0).add({nanoseconds:-1}).toString()",
    "Temporal.Instant.fromEpochMilliseconds(0).subtract({seconds:1}).toString()", "Temporal.Instant.fromEpochMilliseconds(0).add('PT1H').toString()", "Temporal.Instant.fromEpochMilliseconds(0).add(1)",
    "Temporal.Instant.fromEpochMilliseconds(0).add({})", "Temporal.Instant.fromEpochMilliseconds(0).until(Temporal.Instant.fromEpochMilliseconds(3600000)).toString()",
    "Temporal.Instant.fromEpochMilliseconds(3600000).since(Temporal.Instant.fromEpochMilliseconds(0)).toString()", "Temporal.Instant.fromEpochMilliseconds(0).until(Temporal.Instant.fromEpochMilliseconds(90061001),{largestUnit:'hour'}).toString()",
    "Temporal.Instant.fromEpochMilliseconds(0).until(Temporal.Instant.fromEpochMilliseconds(90061001),{smallestUnit:'second',roundingMode:'ceil'}).toString()", "Temporal.Instant.fromEpochMilliseconds(0).until(Temporal.Instant.fromEpochMilliseconds(1),{largestUnit:'day'})",
    "Temporal.Instant.fromEpochMilliseconds(0).round({smallestUnit:'hour'}).toString()", "Temporal.Instant.fromEpochMilliseconds(1800000).round({smallestUnit:'hour'}).toString()", "Temporal.Instant.fromEpochMilliseconds(0).round()",
    "Temporal.Instant.fromEpochMilliseconds(0).round('minute').toString()", "Temporal.Instant.fromEpochMilliseconds(0).round({smallestUnit:'day'}).toString()", "Temporal.Instant.fromEpochMilliseconds(0).round({smallestUnit:'year'})",
    "Temporal.Instant.fromEpochMilliseconds(0).equals(Temporal.Instant.from('1970-01-01T00:00:00Z'))", "Temporal.Instant.compare(Temporal.Instant.fromEpochMilliseconds(0),Temporal.Instant.fromEpochMilliseconds(1))",
    "Temporal.Instant.compare('1970-01-01T00:00:00Z','1970-01-01T00:00:00Z')", "Temporal.Instant.fromEpochMilliseconds(0).toString({timeZone:'Asia/Tokyo'})", "Temporal.Instant.fromEpochMilliseconds(0).toString({fractionalSecondDigits:3})",
    "Temporal.Instant.fromEpochMilliseconds(1).toString({smallestUnit:'microsecond'})", "Temporal.Instant.fromEpochMilliseconds(0).toString({fractionalSecondDigits:10})", "Temporal.Instant.fromEpochMilliseconds(0).toJSON()",
    "JSON.stringify({a:Temporal.Instant.fromEpochMilliseconds(0)})", "Temporal.Instant.fromEpochMilliseconds(0).valueOf()", "Temporal.Instant.fromEpochMilliseconds(0)<Temporal.Instant.fromEpochMilliseconds(1)",
    "Temporal.Instant.fromEpochMilliseconds(0).toLocaleString('en-US',{timeZone:'UTC'})", "Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO('Asia/Tokyo').toString()",
    "Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO('x')", "Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO()", "Temporal.Instant.fromEpochMilliseconds(0).epochSeconds",
    "Temporal.Instant.fromEpochMilliseconds(0).epochMicroseconds", "Temporal.Instant.prototype[Symbol.toStringTag]", "Temporal.Instant.prototype.toString.call({})", "Temporal.Instant.length", "Temporal.Instant.from.length",
    "typeof Temporal.Now.instant().epochNanoseconds", "Temporal.Now.instant() instanceof Temporal.Instant", "typeof Temporal.Now.timeZoneId()", "Temporal.Now.plainDateISO('UTC') instanceof Temporal.PlainDate",
    "Temporal.Now.plainDateISO('x')", "typeof Temporal.Now.plainTimeISO().hour", "Temporal.Now.zonedDateTimeISO('UTC').timeZoneId", "Temporal.Now.zonedDateTimeISO('UTC').calendarId", "typeof Temporal.Now.plainDateTimeISO().year",
    "new Date(0).toTemporalInstant().toString()", "new Date(NaN).toTemporalInstant()", "Date.prototype.toTemporalInstant.call({})", "Temporal.Now()", "new Temporal.Now()", "Object.isExtensible(Temporal.Now)",
    "Temporal[Symbol.toStringTag]", "Object.keys(Temporal)", "Reflect.ownKeys(Temporal).map(String).sort().join()");
}

// ---- 14. ArrayBuffer transfer, resize, detached.
{
  prog("", "{const b=new ArrayBuffer(8);const c=b.transfer();return [b.detached,b.byteLength,c.byteLength,c.detached]}", "{const b=new ArrayBuffer(8);const c=b.transfer(16);return [c.byteLength,c.resizable]}",
    "{const b=new ArrayBuffer(8);const c=b.transfer(4);return c.byteLength}", "{const b=new ArrayBuffer(8);new Uint8Array(b)[0]=7;const c=b.transfer(2);return new Uint8Array(c)}", "{const b=new ArrayBuffer(8);const c=b.transfer(0);return c.byteLength}",
    "{const b=new ArrayBuffer(8,{maxByteLength:16});const c=b.transfer();return [c.resizable,c.maxByteLength,c.byteLength]}", "{const b=new ArrayBuffer(8,{maxByteLength:16});const c=b.transferToFixedLength();return [c.resizable,c.maxByteLength,c.byteLength]}",
    "{const b=new ArrayBuffer(8);const c=b.transferToFixedLength(12);return [c.resizable,c.byteLength]}", "{const b=new ArrayBuffer(8);b.transfer();return b.transfer()}", "{const b=new ArrayBuffer(8);b.transfer();return b.slice(0)}",
    "{const b=new ArrayBuffer(8);b.transfer();return b.resize(1)}", "{const b=new ArrayBuffer(8);b.transfer();return [b.maxByteLength,b.resizable,b.detached]}", "{const b=new ArrayBuffer(8);b.transfer();return new Uint8Array(b)}",
    "{const b=new ArrayBuffer(8);const u=new Uint8Array(b);b.transfer();return [u.length,u.byteLength,u.byteOffset]}", "{const b=new ArrayBuffer(8);const u=new Uint8Array(b);b.transfer();return u[0]}", "{const b=new ArrayBuffer(8);b.transfer(-1)}",
    "{const b=new ArrayBuffer(8);b.transfer(2**53)}", "{const b=new ArrayBuffer(8);b.transfer('x')}", "{const b=new ArrayBuffer(8);b.transfer(undefined);return b.detached}", "ArrayBuffer.prototype.transfer.call({})", "ArrayBuffer.prototype.transfer.call(new SharedArrayBuffer(8))",
    "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'detached').get.call(new SharedArrayBuffer(1))", "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'detached').get.name", "new ArrayBuffer(8,{maxByteLength:4})",
    "{const b=new ArrayBuffer(8,{maxByteLength:16});b.resize(12);return [b.byteLength,b.maxByteLength,b.resizable]}", "{const b=new ArrayBuffer(8,{maxByteLength:16});b.resize(17)}", "{const b=new ArrayBuffer(8,{maxByteLength:16});b.resize(-1)}",
    "{const b=new ArrayBuffer(8);b.resize(4)}", "{const b=new ArrayBuffer(8,{maxByteLength:16});const u=new Uint8Array(b);b.resize(12);return u.length}", "{const b=new ArrayBuffer(8,{maxByteLength:16});const u=new Uint8Array(b,0,8);b.resize(4);return [u.length,u.byteLength,u.byteOffset]}",
    "{const b=new ArrayBuffer(8,{maxByteLength:16});const u=new Uint8Array(b,4);b.resize(2);return [u.length,u.byteOffset]}", "{const b=new ArrayBuffer(8,{maxByteLength:16});b.resize(0);return b.byteLength}", "{const b=new ArrayBuffer(8,{maxByteLength:16});const d=new DataView(b);b.resize(4);return d.byteLength}",
    "{const b=new ArrayBuffer(8,{maxByteLength:16});const d=new DataView(b,0,8);b.resize(4);return d.byteLength}", "{const b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(12);return [b.byteLength,b.growable,b.maxByteLength]}",
    "{const b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(4)}", "{const b=new SharedArrayBuffer(8);b.grow(9)}", "{const b=new SharedArrayBuffer(8);return [b.growable,b.maxByteLength]}", "new ArrayBuffer(8).resizable", "new ArrayBuffer(8).maxByteLength",
    "{const b=new ArrayBuffer(8);const s=structuredClone?0:0;return b.transfer.length}", "ArrayBuffer.prototype.transferToFixedLength.length", "ArrayBuffer.prototype.resize.length", "ArrayBuffer.isView(new Uint8Array(1))",
    "{const u=new Uint8Array(new ArrayBuffer(4,{maxByteLength:8}));return [u.length]}", "{const b=new ArrayBuffer(4,{maxByteLength:8});const u=new Uint8Array(b);b.resize(8);u.fill(1);return u}",
    "{const b=new ArrayBuffer(4,{maxByteLength:8});const u=new Uint8Array(b);const r=Array.from(u);b.resize(8);return [r,Array.from(u)]}", "{const b=new ArrayBuffer(4);return b.slice(1,3).byteLength}", "{const b=new ArrayBuffer(4,{maxByteLength:8});return b.slice(1).resizable}");
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "recent-features-golden-"));
const file = path.join(dir, "recent_features_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
const lines = [];
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body;
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  // O horário e o fuso da máquina variam: programas que os imprimem não entram.
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("dependente da máquina: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("recent_features", lines));
fs.rmSync(dir, { recursive: true, force: true });
