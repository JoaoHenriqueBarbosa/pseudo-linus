// Gera tests/golden/typedarray_ctor_bun.tsv: construtores de TypedArray (os 11 tipos mais Float16Array) contra
// argumentos variados, conversões de elemento (ToNumber/ToBigInt com a ordem das chamadas), clamp, arredondamento,
// Atomics básico, from/of com mapfn e this, métodos com índices extremos, subclasses com species, buffer
// redimensionável e mensagens exatas, medido no bun 1.4.2. Programas cuja expressão já aparece nos goldens
// typedarray_*, atomics e sab são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// O prelúdio comum sai em tests/golden/typedarray_ctor.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-typedarray-ctor-golden.js > tests/golden/typedarray_ctor_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(ArrayBuffer.isView(v)&&!(v instanceof DataView))return v.constructor.name+"["+Array.from({length:v.length},(_,i)=>S(v[i],d+1)).join(",")+"]";' +
  'if(v instanceof ArrayBuffer)return "AB("+v.byteLength+(v.resizable?"/"+v.maxByteLength:"")+")";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

const NUM = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array"];
const BIG = ["BigInt64Array", "BigUint64Array"];
const ALL = NUM.concat(BIG);
const subset = ["Uint8Array", "Uint8ClampedArray", "Int16Array", "Float16Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
const isBig = n => n.startsWith("Big");
const lit = (n, i) => (isBig(n) ? i + "n" : String(i));

// ---- 1. construtores x argumentos.
const ctorArgs = [
  "", "0", "1", "3", "2.7", "-0", "'2'", "'abc'", "true", "null", "undefined", "NaN", "-1", "-1.5", "Infinity", "-Infinity", "2**53", "2**32", "2**31", "2**40",
  "1e20", "{}", "{length:2,0:1,1:2}", "{length:'2',0:'7'}", "{length:-1}", "{length:NaN}", "{length:2**53}", "{length:3,1:5}", "[]", "[1,2,3]", "[1.9,-1.9]",
  "['1','2']", "[null,undefined]", "[true,false]", "[1n]", "['1n']", "[{}]", "[[1]]", "[[]]", "[Symbol()]", "Symbol()", "1n", "new Set([1,2])", "new Map([[1,2]])",
  "'12'", "[1,2][Symbol.iterator]()", "(function*(){yield 1;yield 2})()", "{[Symbol.iterator]:function*(){yield 4;yield 5}}", "{[Symbol.iterator]:1}",
  "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined,length:1,0:9}", "{[Symbol.iterator](){throw new RangeError('it')}}",
  "{[Symbol.iterator](){return {next(){throw new EvalError('nx')}}}}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}",
  "{[Symbol.iterator](){return {next(){return 1}}}}", "{[Symbol.iterator](){return {next(){return {done:true}},return(){throw 1}}}}",
  "new ArrayBuffer(8)", "new ArrayBuffer(8),4", "new ArrayBuffer(8),8", "new ArrayBuffer(8),9", "new ArrayBuffer(8),3", "new ArrayBuffer(8),1,2", "new ArrayBuffer(8),0,0",
  "new ArrayBuffer(8),0,100", "new ArrayBuffer(8),-1", "new ArrayBuffer(8),'1'", "new ArrayBuffer(8),NaN,1", "new ArrayBuffer(8),undefined,2", "new ArrayBuffer(8),2,undefined",
  "new ArrayBuffer(8),0,-1", "new ArrayBuffer(8),0,2**53", "new ArrayBuffer(8),Infinity", "new ArrayBuffer(8),1.9,1.9", "new ArrayBuffer(8),{valueOf(){return 4}}",
  "new ArrayBuffer(8),0,{valueOf(){return 2}}", "new ArrayBuffer(8),Symbol()", "new ArrayBuffer(0)", "new ArrayBuffer(0),0", "new ArrayBuffer(0),1",
  "new ArrayBuffer(6)", "new ArrayBuffer(6),2", "new ArrayBuffer(3),1", "new ArrayBuffer(8,{maxByteLength:16})", "new ArrayBuffer(8,{maxByteLength:16}),4",
  "new ArrayBuffer(8,{maxByteLength:16}),4,1", "new ArrayBuffer(8,{maxByteLength:16}),0,undefined", "new ArrayBuffer(8,{maxByteLength:16}),12",
  "new ArrayBuffer(7,{maxByteLength:16})", "new ArrayBuffer(8,{maxByteLength:16}),0,9",
  "(()=>{var b=new ArrayBuffer(8);structuredClone(b,{transfer:[b]});return b})()", "new SharedArrayBuffer(8)", "new SharedArrayBuffer(8),4,1",
  "new Uint8Array([1,2,3])", "new Int8Array([-1,-2])", "new Float64Array([1.5,NaN,-0])", "new Uint8ClampedArray([300,-5])", "new BigInt64Array([-1n,2n])",
  "new BigUint64Array([1n])", "new Float32Array([0.1])", "new Uint16Array([65535,1])", "new Int32Array(0)", "new DataView(new ArrayBuffer(4))",
  "new Proxy([1,2],{})", "new Proxy({length:2,0:1,1:2},{})", "Object.create({length:1,0:5})", "{get length(){throw new TypeError('len')}}",
  "{length:1,get 0(){throw new TypeError('el')}}", "{length:{valueOf(){return 2}}}", "{length:{valueOf(){throw new SyntaxError('lv')}}}",
  "Object.assign([1,2],{length:1})", "'\u{1F600}x'", "new String('hi')", "Object(1)", "function(){}", "class{}", "/x/", "new Date(0)",
];
for (const n of ALL) for (const a of ctorArgs) {
  add(`T(()=>new ${n}(${a}))`);
}
for (const n of ALL) {
  add(
    `T(()=>${n}(1))`, `T(()=>${n}())`, `T(()=>${n}.call({},1))`, `T(()=>Reflect.construct(${n},[2],Object).length)`,
    `T(()=>Object.getPrototypeOf(Reflect.construct(${n},[2],Object))===Object.prototype)`, `T(()=>Reflect.construct(${n},[2],Array) instanceof Array)`,
    `T(()=>${n}.BYTES_PER_ELEMENT+','+${n}.prototype.BYTES_PER_ELEMENT)`, `T(()=>${n}.length+','+${n}.name)`, `T(()=>${n}.prototype.constructor===${n})`,
    `T(()=>Object.getPrototypeOf(${n})===Object.getPrototypeOf(Int8Array))`, `T(()=>Object.getOwnPropertyNames(${n}).sort().join())`,
    `T(()=>Object.getOwnPropertyNames(${n}.prototype).sort().join())`, `T(()=>Object.prototype.toString.call(new ${n}(1)))`,
    `T(()=>new ${n}(2)[Symbol.toStringTag])`, `T(()=>Object.getOwnPropertyDescriptor(${n},'BYTES_PER_ELEMENT').writable)`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b,8);return a.byteOffset+','+a.length+','+a.byteLength+','+(a.buffer===b)})`,
    `T(()=>{var a=new ${n}(3);return a.byteOffset+','+a.length+','+a.byteLength+','+a.buffer.byteLength})`,
    `T(()=>{var a=new ${n}(2);a[5]=1;return Object.keys(a).join()+'|'+(5 in a)+'|'+a[5]})`,
    `T(()=>{var a=new ${n}(2);a['-0']=1;a['1.5']=1;a.foo=1;return Object.keys(a).join()})`,
    `T(()=>{var a=new ${n}(2);return ('-0' in a)+','+('1.5' in a)+','+('0' in a)+','+(2 in a)+','+(-1 in a)})`,
    `T(()=>Object.getOwnPropertyDescriptor(new ${n}(2),'1')&&JSON.stringify(Object.getOwnPropertyDescriptor(new ${n}(2),'1')))`,
    `T(()=>Reflect.defineProperty(new ${n}(2),'1',{value:${lit(n, 3)},writable:true,enumerable:true,configurable:true}))`,
    `T(()=>Reflect.defineProperty(new ${n}(2),'1',{value:${lit(n, 3)},configurable:false}))`,
    `T(()=>Reflect.defineProperty(new ${n}(2),'2',{value:${lit(n, 3)}}))`, `T(()=>Reflect.defineProperty(new ${n}(2),'1',{get(){}}))`,
    `T(()=>Reflect.defineProperty(new ${n}(2),'1',{value:${lit(n, 3)},enumerable:false}))`, `T(()=>Reflect.deleteProperty(new ${n}(2),'1'))`,
    `T(()=>Reflect.deleteProperty(new ${n}(2),'5'))`, `T(()=>{'use strict';delete new ${n}(2)[0]})`, `T(()=>Object.freeze(new ${n}(2)))`, `T(()=>Object.freeze(new ${n}(0)).length)`,
    `T(()=>Object.seal(new ${n}(2)).length)`, `T(()=>Object.isFrozen(Object.preventExtensions(new ${n}(0))))`,
    `T(()=>{var a=new ${n}(2);return Reflect.set(a,'1',${lit(n, 4)},{})+','+a[1]})`, `T(()=>{var a=new ${n}(2);var r={};Reflect.set(a,'5',${lit(n, 4)},r);return Object.keys(r).join()})`,
    `T(()=>{var a=new ${n}(2);var r=Object.create(a);r[1]=${lit(n, 9)};return a[1]+','+Object.keys(r).join()})`,
    `T(()=>{var a=new ${n}(2);var r=Object.create(a);r[7]=${lit(n, 9)};return Object.keys(r).join()+','+r[7]})`,
  );
}

// ---- 2. conversões de elemento por tipo.
const numVals = [
  "0", "-0", "1", "-1", "0.5", "-0.5", "1.5", "2.5", "-1.5", "-2.5", "0.4999999", "254.5", "255.5", "255.4999", "127.5", "128", "-128", "-129", "255", "256", "257", "-255",
  "32767", "32768", "-32769", "65535", "65536", "65537", "2**31", "2**31-1", "-(2**31)", "-(2**31)-1", "2**32", "2**32-1", "2**32+1", "2**53", "2**53+2", "-(2**53)", "1e21", "1e308",
  "-1e308", "Infinity", "-Infinity", "NaN", "0.1", "0.3", "1/3", "16777217", "16777216.5", "3.4028235e38", "3.4028236e38", "3.5e38", "1e-45", "1e-46", "7e-46", "1.4e-45",
  "65504", "65519", "65520", "65536.5", "6.1e-5", "5.96e-8", "2.98e-8", "2.99e-8", "1+2**-11", "1+2**-10", "2049", "2050", "2051", "4095.5", "'3'", "' 12 '", "'0x10'", "'1e3'",
  "''", "'abc'", "'Infinity'", "'-0'", "null", "undefined", "true", "false", "[]", "[5]", "[1,2]", "{}", "{valueOf(){return 6}}", "{toString(){return '7'}}",
  "{valueOf(){return {}},toString(){return '8'}}", "{valueOf:null,toString(){return '9'}}", "new Number(4)", "new String('5')", "new Date(3)", "Symbol()", "1n", "Object(1n)",
  "{[Symbol.toPrimitive](h){return h==='number'?11:99}}", "{[Symbol.toPrimitive](){return {}}}", "{valueOf(){throw new RangeError('vo')}}",
];
const bigVals = [
  "0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "-(2n**63n)-1n", "2n**64n", "2n**64n-1n", "2n**64n+1n", "2n**65n+5n", "-(2n**64n)", "-(2n**64n)-1n", "10n**30n",
  "-(10n**30n)", "2n**128n+7n", "'12'", "' 7 '", "'0x1f'", "'-5'", "''", "'1.5'", "'abc'", "'1n'", "'1e3'", "1", "1.5", "NaN", "Infinity", "'9007199254740993'", "'18446744073709551617'",
  "true", "false", "null", "undefined", "[]", "[5]", "['7']", "[1,2]", "{}", "{valueOf(){return 6n}}", "{valueOf(){return 6}}", "{toString(){return '7'}}",
  "{[Symbol.toPrimitive](){return 8n}}", "{[Symbol.toPrimitive](){return '9'}}", "{[Symbol.toPrimitive](){return 9}}", "Object(5n)", "Symbol()", "new String('5')", "0", "-0",
  "{valueOf(){throw new RangeError('vo')}}", "new Number(1)",
];
for (const n of NUM) for (const v of numVals) {
  add(
    `T(()=>{var a=new ${n}(1);a[0]=${v};return S(a[0])})`,
    `T(()=>{'use strict';var a=new ${n}(1);a[0]=${v};return S(a[0])})`,
  );
}
for (const n of ["Uint8Array", "Uint8ClampedArray", "Int16Array", "Float16Array", "Float32Array"]) for (const v of numVals.slice(0, 90)) {
  add(`T(()=>S(new ${n}([${v}])))`, `T(()=>S(${n}.of(${v})))`, `T(()=>{var a=new ${n}(2);a.fill(${v});return S(a)})`);
}
for (const n of BIG) for (const v of bigVals) {
  add(
    `T(()=>{var a=new ${n}(1);a[0]=${v};return S(a[0])})`, `T(()=>S(new ${n}([${v}])))`, `T(()=>S(${n}.of(${v})))`,
    `T(()=>{var a=new ${n}(2);a.fill(${v});return S(a)})`,
  );
}
for (const n of NUM) for (const v of ["1n", "Object(1n)", "0n"]) add(`T(()=>S(new ${n}([${v}])))`, `T(()=>${n}.of(${v}).length)`);
// Elemento fora do intervalo e leitura de índice.
for (const n of ALL) {
  add(
    `T(()=>{var a=new ${n}(2);a[2]=${lit(n, 1)};a[-1]=${lit(n, 1)};a[1.5]=${lit(n, 1)};return S(a)+Object.keys(a).length})`,
    `T(()=>{var a=new ${n}(2);return [a[2],a[-1],a[1.5],a['1.0'],a['01'],a[' 1'],a['1e0']].map(S).join()})`,
    `T(()=>{var a=new ${n}([${lit(n, 1)},${lit(n, 2)}]);return [a['0'],a[1],a[0.0],a['+1'],a['-0']].map(S).join()})`,
    `T(()=>{var a=new ${n}(1);a[0]=${lit(n, 5)};a.length=9;return a.length+','+S(a)})`,
    `T(()=>{'use strict';var a=new ${n}(1);a.length=9})`, `T(()=>{'use strict';var a=new ${n}(1);a[3]=${lit(n, 1)};return 'ok'})`,
    `T(()=>{'use strict';var a=new ${n}(1);a[-1]=${lit(n, 1)};return 'ok'})`, `T(()=>{'use strict';var a=new ${n}(1);a['foo']=${lit(n, 1)};return a.foo})`,
  );
}

// ---- 3. ordem de efeitos da conversão (log).
const logv = (n, tag) => `{valueOf(){L.push('${tag}');return ${lit(n, 1)}}}`;
for (const n of ["Uint8Array", "Int32Array", "Float64Array", "Uint8ClampedArray", "BigInt64Array"]) {
  const o = (t) => logv(n, t);
  add(
    `T(()=>{var L=[];new ${n}([${o("a")},${o("b")},${o("c")}]);return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.fill(${o("f")});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(0);a.fill(${o("f")});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.fill(${o("f")},{valueOf(){L.push('s');return 1}},{valueOf(){L.push('e');return 2}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.fill(${o("f")},{valueOf(){L.push('s');throw 1}},{valueOf(){L.push('e');return 2}})}catch(e){L.push('caught')}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.with(1,${o("w")});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.with({valueOf(){L.push('i');return 9}},${o("w")})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.with({valueOf(){L.push('i');return 1}},{valueOf(){L.push('v');throw new RangeError('x')}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a[0]=${o("s")};a[7]=${o("o")};return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a['-1']=${o("neg")};a['1.5']=${o("frac")};return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.set([${o("x")},${o("y")}],1);return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.set([${o("x")},${o("y")}],2)}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.set({length:2,0:${o("x")},1:${o("y")}},{valueOf(){L.push('off');return 1}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);try{a.set({get length(){L.push('len');return 2},get 0(){L.push('g0');return ${lit(n, 1)}},get 1(){L.push('g1');return ${lit(n, 1)}}},1)}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];${n}.from([${o("a")},${o("b")}]);return L.join()})`,
    `T(()=>{var L=[];${n}.from([1,2],x=>{L.push('m'+x);return ${o("v")}});return L.join()})`,
    `T(()=>{var L=[];${n}.from({length:2,0:${o("a")},1:${o("b")}});return L.join()})`,
    `T(()=>{var L=[];${n}.of(${o("a")},${o("b")});return L.join()})`,
    `T(()=>{var L=[];new ${n}({length:{valueOf(){L.push('len');return 2}},0:${o("a")},1:${o("b")}});return L.join()})`,
    `T(()=>{var L=[];var it={[Symbol.iterator](){L.push('iter');var i=0;return {next(){L.push('next');return {done:i++>1,value:${o("v")}}}}}};new ${n}(it);return L.join()})`,
    `T(()=>{var L=[];var it={get [Symbol.iterator](){L.push('getiter');return undefined},get length(){L.push('len');return 1},get 0(){L.push('g0');return ${lit(n, 3)}}};new ${n}(it);return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}([${lit(n, 1)},${lit(n, 2)},${lit(n, 3)}]);a.indexOf(${o("n")});a.includes(${o("n")});a.lastIndexOf(${o("n")});return L.length})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.copyWithin({valueOf(){L.push('t');return 0}},{valueOf(){L.push('s');return 1}},{valueOf(){L.push('e');return 3}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.slice({valueOf(){L.push('s');return 1}},{valueOf(){L.push('e');return 3}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.subarray({valueOf(){L.push('s');return 1}},{valueOf(){L.push('e');return 3}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.at({valueOf(){L.push('at');return -1}});a.indexOf(${lit(n, 0)},{valueOf(){L.push('from');return 1}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(3);a.join({toString(){L.push('sep');return '-'}});return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(0);a.join({toString(){L.push('sep');return '-'}});return L.join()})`,
    `T(()=>{var L=[];var b=new ArrayBuffer(8);new ${n}(b,{valueOf(){L.push('off');return 0}},{valueOf(){L.push('len');return 1}});return L.join()})`,
    `T(()=>{var L=[];var b=new ArrayBuffer(8);try{new ${n}(b,{valueOf(){L.push('off');return 99}},{valueOf(){L.push('len');return 1}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var b=new ArrayBuffer(8);try{new ${n}(b,{valueOf(){L.push('off');throw new EvalError('o')}},{valueOf(){L.push('len');return 1}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var p=new Proxy([${lit(n, 1)},${lit(n, 2)}],{get(t,k,r){L.push(String(k));return Reflect.get(t,k,r)},has(t,k){L.push('has '+String(k));return k in t}});new ${n}(p);return L.join()})`,
    `T(()=>{var L=[];var p=new Proxy({length:2,0:${lit(n, 1)},1:${lit(n, 2)}},{get(t,k,r){L.push(String(k));return Reflect.get(t,k,r)}});new ${n}(p);return L.join()})`,
  );
}

// ---- 4. construtor a partir de outro typed array (todas as combinações de tipo).
for (const a of ALL) for (const b of ALL) {
  const src = isBig(a) ? ["[-1n,2n,3n]", "[2n**63n-1n]"] : ["[-1.5,2.5,300]", "[NaN,Infinity,-0]"];
  add(
    `T(()=>S(new ${b}(new ${a}(${src[0]}))))`, `T(()=>S(new ${b}(new ${a}(${src[1]}))))`,
    `T(()=>S(new ${b}(new ${a}(${src[0]}).buffer)))`, `T(()=>S(new ${b}(new ${a}(${src[0]}).buffer,${Math.min(8, 8)})))`,
  );
}
for (const a of ALL) {
  add(
    `T(()=>{var s=new ${a}(2);var p=new Proxy(s,{});return S(new Uint8Array(p))})`,
    `T(()=>{var s=new ${a}(2);Object.defineProperty(s,Symbol.iterator,{value:undefined});return S(new Uint8Array(s))})`,
    `T(()=>{var L=[];var s=new ${a}(2);s[Symbol.iterator]=function*(){L.push('it');yield ${lit(a, 5)}};return S(new ${a}(s))+L.join()})`,
    `T(()=>{var L=[];var s=new ${a}(2);Object.defineProperty(s,'length',{get(){L.push('len');return 1}});return S(new ${a}(s))+L.join()})`,
    `T(()=>{var L=[];var s=new ${a}(2);var o=Object.getPrototypeOf(${a}.prototype);var old=Object.getOwnPropertyDescriptor(o,Symbol.iterator);try{Object.defineProperty(o,Symbol.iterator,{value:function*(){L.push('patched')},configurable:true});S(new ${a}(s))}finally{Object.defineProperty(o,Symbol.iterator,old)}return L.join()+S(new ${a}(s))})`,
    `T(()=>{var s=new ${a}(2);s.constructor=Array;return S(new ${a}(s))})`,
    `T(()=>{var s=new ${a}(2);s.buffer;Object.defineProperty(s,'buffer',{get(){throw new EvalError('buf')}});return S(new ${a}(s))})`,
  );
}

// ---- 5. métodos com índices extremos.
const idx = ["undefined", "0", "-0", "1", "2", "3", "4", "5", "-1", "-2", "-3", "-4", "-5", "-6", "1.9", "-1.9", "NaN", "Infinity", "-Infinity", "2**32", "2**53", "-(2**53)", "2**64", "1e21", "'2'", "'x'", "null", "true", "{valueOf(){return 2}}", "[1]", "-(2**32)", "2**31", "2**31-1", "-(2**31)"];
const mk = n => `new ${n}([${[1, 2, 3, 4].map(i => lit(n, i)).join(",")}])`;
for (const n of subset) {
  const base = mk(n);
  for (const i of idx) {
    add(
      `T(()=>S(${base}.slice(${i})))`, `T(()=>S(${base}.subarray(${i})))`, `T(()=>S(${base}.fill(${lit(n, 9)},${i})))`, `T(()=>S(${base}.copyWithin(${i},0)))`,
      `T(()=>S(${base}.copyWithin(0,${i})))`, `T(()=>S(${base}.with(${i},${lit(n, 7)})))`, `T(()=>S(${base}.at(${i})))`, `T(()=>S(${base}.indexOf(${lit(n, 3)},${i})))`,
      `T(()=>S(${base}.lastIndexOf(${lit(n, 3)},${i})))`, `T(()=>S(${base}.includes(${lit(n, 3)},${i})))`, `T(()=>S(${base}.set([${lit(n, 8)}],${i})))`,
      `T(()=>S(${base}.slice(0,${i})))`, `T(()=>S(${base}.subarray(0,${i})))`, `T(()=>S(${base}.fill(${lit(n, 9)},0,${i})))`, `T(()=>S(${base}.copyWithin(1,0,${i})))`,
      `T(()=>S(new ${n}(new ArrayBuffer(16),${i})))`, `T(()=>S(new ${n}(new ArrayBuffer(16),0,${i})))`,
    );
  }
  const fns = ["x=>x>" + lit(n, 2), "x=>x===" + lit(n, 9), "()=>true", "()=>false"];
  for (const f of fns) {
    add(
      `T(()=>S(${base}.findLast(${f})))`, `T(()=>S(${base}.findLastIndex(${f})))`, `T(()=>S(${base}.find(${f})))`, `T(()=>S(${base}.findIndex(${f})))`,
      `T(()=>S(${base}.filter(${f})))`, `T(()=>S(${base}.some(${f})))`, `T(()=>S(${base}.every(${f})))`,
    );
  }
  add(
    `T(()=>S(${base}.toSorted()))`, `T(()=>S(${base}.toReversed()))`, `T(()=>S(${base}.reverse()))`, `T(()=>S(${base}.sort()))`, `T(()=>S(${base}.sort((a,b)=>a<b?1:a>b?-1:0)))`,
    `T(()=>S(${base}.toSorted((a,b)=>a<b?1:a>b?-1:0)))`, `T(()=>S(${base}.sort(undefined)))`, `T(()=>S(${base}.sort(null)))`, `T(()=>S(${base}.sort(1)))`, `T(()=>S(${base}.sort({})))`,
    `T(()=>S(${base}.toSorted(null)))`, `T(()=>S(${base}.toSorted('a')))`, `T(()=>S(${base}.sort(()=>{throw new RangeError('c')})))`, `T(()=>S(${base}.toSorted(()=>NaN)))`,
    `T(()=>S(${base}.sort(()=>0)))`, `T(()=>S(${base}.sort((a,b)=>{return {valueOf(){return -1}}})))`, `T(()=>S(${base}.sort((a,b)=>'1')))`,
    `T(()=>{var a=${base};return S(a.sort((x,y)=>{a.fill(${lit(n, 0)});return 1}))})`, `T(()=>{var a=${base};return S(a.sort((x,y)=>{a.buffer.transfer?.();return 1}))})`,
    `T(()=>S(new ${n}([${lit(n, 3)},${lit(n, 1)},${lit(n, 2)},${lit(n, 1)}]).sort()))`, `T(()=>S(new ${n}(0).sort()))`, `T(()=>S(new ${n}(0).toSorted()))`, `T(()=>S(new ${n}(0).with(0,${lit(n, 1)})))`,
    `T(()=>S(new ${n}(1).with(1,${lit(n, 1)})))`, `T(()=>S(new ${n}(1).with(-2,${lit(n, 1)})))`, `T(()=>S(new ${n}(1).with(-1,${lit(n, 1)})))`,
    `T(()=>S(${base}.toSpliced?.(0,1)))`, `T(()=>S(${base}.map(x=>x)))`, `T(()=>S(${base}.map(x=>1.5)))`, `T(()=>S(${base}.map(x=>'3')))`, `T(()=>S(${base}.map(x=>${lit(n, 300)})))`,
    `T(()=>S(${base}.map(x=>${isBig(n) ? "1" : "1n"})))`, `T(()=>S(${base}.reduce((a,b)=>a+b)))`, `T(()=>S(${base}.reduceRight((a,b)=>a+'|'+b)))`, `T(()=>S(new ${n}(0).reduce((a,b)=>a+b))))`,
    `T(()=>S(${base}.join()))`, `T(()=>S(${base}.join(undefined)))`, `T(()=>S(${base}.join(null)))`, `T(()=>S(${base}.toString()))`, `T(()=>S(${base}.toLocaleString()))`,
    `T(()=>S([...${base}.keys()]))`, `T(()=>S([...${base}.entries()]))`, `T(()=>S([...${base}.values()]))`, `T(()=>S(Array.from(${base})))`, `T(()=>S(Object.entries(${base})))`,
    `T(()=>S(Array.prototype.slice.call(${base},1)))`, `T(()=>S(Array.prototype.concat.call([],${base})))`, `T(()=>S(JSON.stringify(${base})))`, `T(()=>S(Object.assign({},${base})))`,
    `T(()=>S(${base}.forEach(x=>x)))`, `T(()=>S(${base}.lastIndexOf(${lit(n, 1)},-0)))`, `T(()=>S(${base}.indexOf('1')))`, `T(()=>S(${base}.includes(undefined)))`, `T(()=>S(${base}.indexOf(${isBig(n) ? "1" : "1n"})))`,
    `T(()=>S(${base}.set(${base})))`, `T(()=>S(${base}.set(${base},1)))`, `T(()=>S(${base}.set(${base},0)))`, `T(()=>S(${base}.set('12')))`, `T(()=>S(${base}.set(1)))`, `T(()=>S(${base}.set(null)))`,
    `T(()=>S(${base}.set(undefined)))`, `T(()=>S(${base}.set()))`, `T(()=>S(${base}.set({length:2,0:${lit(n, 5)}})))`, `T(()=>S(${base}.set({length:-1})))`, `T(()=>S(${base}.set([],4)))`, `T(()=>S(${base}.set([],5)))`,
    `T(()=>{var a=${base};a.set(a.subarray(0,3),1);return S(a)})`, `T(()=>{var a=${base};a.set(a.subarray(1),0);return S(a)})`, `T(()=>{var a=${base};a.copyWithin(1,0);return S(a)})`,
    `T(()=>{var a=${base};a.copyWithin(0,1,3);return S(a)})`, `T(()=>{var a=${base};return S(a.copyWithin(-2,-4,-1))})`, `T(()=>{var a=${base};return S(a.copyWithin(3,0,100))})`,
    `T(()=>{var a=${base};return S(a.fill(${lit(n, 5)},-3,-1))})`, `T(()=>{var a=${base};return S(a.fill(${lit(n, 5)},3,1))})`, `T(()=>{var a=${base};return S(a.fill(${lit(n, 5)},-100,100))})`,
    `T(()=>{var a=${base};return S(a.subarray(-2).byteOffset+','+a.subarray(1,3).length+','+a.subarray(3,1).length+','+(a.subarray(1).buffer===a.buffer))})`,
    `T(()=>{var a=${base};var s=a.subarray(1,3);s[0]=${lit(n, 0)};return S(a)})`, `T(()=>{var a=${base};var s=a.slice(1,3);s[0]=${lit(n, 0)};return S(a)+(s.buffer===a.buffer)})`,
    `T(()=>S(${base}.subarray(1,3).subarray(1)))`, `T(()=>S(${base}.subarray(1,3).slice(-1)))`, `T(()=>${base}.subarray(1,3).byteOffset)`,
  );
}
add("T(()=>S(new Uint8Array([1,2]).toSpliced))");

// ---- 6. from / of com mapfn e this.
const fromSrc = ["[1,2,3]", "'123'", "{length:2,0:5,1:6}", "new Set([4,5])", "new Uint8Array([7,8])", "[]", "{}", "1", "null", "undefined", "true", "(function*(){yield 1;yield 2})()", "{length:3}", "[1.5,'2',null]", "new Map([[1,2]]).keys()"];
const mapfns = ["", ",undefined", ",null", ",1", ",{}", ",x=>x*2", ",(x,i)=>i", ",function(x,i){return this&&this.k}", ",function(x,i){return this&&this.k},{k:7}", ",(x,i,a)=>arguments_len(arguments)", ",x=>{throw new RangeError('m')}", ",x=>'5'", ",x=>({valueOf(){return 3}})", ",x=>1n"];
for (const n of ["Uint8Array", "Int16Array", "Float32Array", "BigInt64Array"]) for (const s of fromSrc) for (const m of mapfns.slice(0, 13)) {
  add(`T(()=>S(${n}.from(${s}${m})))`);
}
for (const n of ["Uint8Array", "BigUint64Array"]) for (const s of fromSrc.slice(0, 6)) {
  add(`T(()=>{'use strict';var t;${n}.from(${s},function(){t=this});return typeof t})`, `T(()=>{var t;${n}.from(${s},function(){t=this});return typeof t})`, `T(()=>{var t;${n}.from(${s},()=>1,5);return typeof t})`);
}
const thisVals = ["undefined", "null", "1", "'s'", "{}", "Array", "Object", "function(){}", "()=>{}", "class{}", "Uint8Array", "Float64Array", "Int8Array.bind(null)", "Math.max", "Symbol", "new Proxy(Uint8Array,{})", "class extends Uint8Array{}",
  "class extends Uint8Array{constructor(){super(1)}}", "class extends Uint8Array{constructor(n){super(n+1)}}", "function(n){return new Uint8Array(n+1)}", "function(n){return new Uint8Array(1)}", "function(n){return {length:n}}", "function(){return new Uint16Array(8)}",
  "function(){return new Float16Array(2)}", "function(){return 1}", "function(){return new Uint8Array(new ArrayBuffer(4),0,2)}", "function(n){var a=new Uint8Array(n);Object.freeze(a);return a}", "function(n){return new BigInt64Array(n)}"];
for (const t of thisVals) {
  add(
    `T(()=>S(Uint8Array.from.call(${t},[1,2])))`, `T(()=>S(Uint8Array.of.call(${t},1,2)))`, `T(()=>S(Uint8Array.of.call(${t})))`, `T(()=>S(Uint8Array.from.call(${t},{length:3,0:1})))`,
    `T(()=>S(Uint8Array.from.call(${t},new Set([9]),x=>x+1)))`, `T(()=>S(Uint8Array.of.call(${t},1,2,3)))`, `T(()=>S(Uint8Array.from.call(${t},[])))`, `T(()=>S(BigInt64Array.of.call(${t},1n)))`,
  );
}
for (const n of ALL) {
  add(
    `T(()=>S(${n}.of()))`, `T(()=>S(${n}.of(${lit(n, 1)},${lit(n, 2)})))`, `T(()=>${n}.of.length+','+${n}.from.length)`, `T(()=>${n}.from.call(undefined,[]))`, `T(()=>${n}.from.call({},[]))`,
    `T(()=>${n}.from())`, `T(()=>S(${n}.from([],undefined,1)))`, `T(()=>{var o=Object.create(${n}.prototype);return S(${n}.of.call(function(){return o},${lit(n, 1)}))})`,
    `T(()=>S(${n}.from({[Symbol.iterator]:null,length:1,0:${lit(n, 4)}})))`, `T(()=>S(${n}.from('')))`, `T(()=>S(${n}.from(new ${n}([${lit(n, 1)}]),x=>x)))`,
    `T(()=>{var L=[];var r=${n}.from.call(function(n){L.push('ctor '+n);return new ${n}(n)},[${lit(n, 1)},${lit(n, 2)}],x=>{L.push('map');return x});return L.join()})`,
    `T(()=>{var L=[];${n}.from.call(function(n){L.push('ctor '+n);return new ${n}(n)},{get length(){L.push('len');return 1},get 0(){L.push('g0');return ${lit(n, 1)}}});return L.join()})`,
    `T(()=>{var L=[];${n}.from.call(function(n){L.push('ctor '+n);return new ${n}(n)},new Set([${lit(n, 1)}]));return L.join()})`,
    `T(()=>{var L=[];${n}.of.call(function(n){L.push('ctor '+n+' '+arguments.length);return new ${n}(n)},${lit(n, 1)},${lit(n, 2)});return L.join()})`,
  );
}

// ---- 7. Atomics básico.
const intT = ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "BigInt64Array", "BigUint64Array"];
const ops = ["add", "sub", "and", "or", "xor", "exchange"];
const atomVals = ["0", "1", "-1", "5", "255", "256", "257", "65535", "65536", "2**31", "2**32", "2**32+3", "2**53", "-(2**31)", "1.9", "-1.9", "NaN", "Infinity", "'3'", "'x'", "null", "undefined", "true", "[]", "{}", "{valueOf(){return 4}}", "Symbol()", "1n"];
const atomBig = ["0n", "1n", "-1n", "5n", "2n**63n", "2n**64n", "2n**64n+3n", "-(2n**63n)", "'3'", "'x'", "1", "null", "undefined", "true", "{valueOf(){return 4n}}", "Symbol()"];
for (const n of intT) {
  const vs = isBig(n) ? atomBig : atomVals;
  const init = isBig(n) ? "[10n,20n]" : "[10,20]";
  for (const op of ops) for (const v of vs) {
    add(`T(()=>{var a=new ${n}(${init});var r=Atomics.${op}(a,0,${v});return S(r)+','+S(a[0])})`);
  }
  for (const v of vs.slice(0, 12)) {
    add(
      `T(()=>{var a=new ${n}(${init});return S(Atomics.store(a,1,${v}))+','+S(a[1])})`, `T(()=>{var a=new ${n}(${init});return S(Atomics.compareExchange(a,0,${isBig(n) ? "10n" : "10"},${v}))+','+S(a[0])})`,
      `T(()=>{var a=new ${n}(${init});return S(Atomics.compareExchange(a,0,${v},${isBig(n) ? "99n" : "99"}))+','+S(a[0])})`,
    );
  }
  const ix = ["0", "1", "2", "-1", "1.5", "'1'", "'x'", "NaN", "Infinity", "-0", "undefined", "null", "true", "{valueOf(){return 1}}", "2**32", "Symbol()", "1n"];
  for (const i of ix) {
    add(
      `T(()=>{var a=new ${n}(${init});return S(Atomics.load(a,${i}))})`, `T(()=>{var a=new ${n}(${init});return S(Atomics.add(a,${i},${lit(n, 1)}))+','+S(a)})`,
      `T(()=>{var a=new ${n}(${init});return S(Atomics.store(a,${i},${lit(n, 1)}))+','+S(a)})`, `T(()=>{var a=new ${n}(${init});return S(Atomics.exchange(a,${i},${lit(n, 1)}))})`,
    );
  }
  add(
    `T(()=>Atomics.isLockFree(${n}.BYTES_PER_ELEMENT))`, `T(()=>{var a=new ${n}(2);return S(Atomics.load(a,0))})`, `T(()=>Atomics.notify(new ${n}(2),0))`, `T(()=>Atomics.notify(new ${n}(2),0,1))`,
    `T(()=>Atomics.wait(new ${n}(2),0,0,0))`, `T(()=>Atomics.add(${n}.prototype,0,1))`, `T(()=>Atomics.add([],0,1))`, `T(()=>Atomics.add({},0,1))`, `T(()=>Atomics.add(new DataView(new ArrayBuffer(2)),0,1))`,
    `T(()=>Atomics.add())`, `T(()=>Atomics.load())`, `T(()=>Atomics.add(1,0,1))`, `T(()=>Atomics.add(null,0,1))`,
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return S(Atomics.add(a,0,${lit(n, 3)}))+','+S(Atomics.load(a,0))+','+S(Atomics.notify(a,0))})`,
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.wait(a,0,${lit(n, 1)},0)})`, `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.wait(a,0,${lit(n, 0)},0)})`,
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.wait(a,9,${lit(n, 0)},0)})`, `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.wait(a,0,${lit(n, 0)},-5)})`,
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.wait(a,0,${lit(n, 0)},NaN)})`.replace("NaN", "0"),
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.notify(a,0,-1)})`, `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.notify(a,9)})`,
    `T(()=>{var a=new ${n}(new SharedArrayBuffer(16));return Atomics.notify(a,0,'x')})`,
    `T(()=>{var a=new ${n}(new ArrayBuffer(16));return Atomics.notify(a,0)})`, `T(()=>{var a=new ${n}(new ArrayBuffer(16));return Atomics.wait(a,0,0,0)})`,
    `T(()=>{var L=[];var a=new ${n}(2);Atomics.add({valueOf(){L.push('x')}},0,1);return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(2);try{Atomics.add(a,{valueOf(){L.push('i');return 0}},{valueOf(){L.push('v');return ${lit(n, 1)}}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(2);try{Atomics.add(a,{valueOf(){L.push('i');return 9}},{valueOf(){L.push('v');return ${lit(n, 1)}}})}catch(e){L.push(e.name)}return L.join()})`,
    `T(()=>{var L=[];var a=new ${n}(2);try{Atomics.compareExchange(a,0,{valueOf(){L.push('e');return ${lit(n, 0)}}},{valueOf(){L.push('r');return ${lit(n, 1)}}})}catch(e){L.push(e.name)}return L.join()})`,
  );
}
for (const n of ["Float32Array", "Float64Array", "Uint8ClampedArray", "Float16Array"]) {
  for (const op of ops.concat(["load", "store", "compareExchange"])) add(`T(()=>Atomics.${op}(new ${n}(2),0,1,1))`);
  add(`T(()=>Atomics.notify(new ${n}(2),0))`, `T(()=>Atomics.wait(new ${n}(new SharedArrayBuffer(8)),0,0,0))`, `T(()=>Atomics.isLockFree(${n}.BYTES_PER_ELEMENT))`);
}
for (const k of [1, 2, 3, 4, 5, 8, 16, 0, -1, "'4'", "NaN", "undefined", "null", "1n"]) add(`T(()=>Atomics.isLockFree(${k}))`);
add("T(()=>Object.getOwnPropertyNames(Atomics).sort().join())", "T(()=>Object.prototype.toString.call(Atomics))", "T(()=>typeof Atomics.waitAsync)", "T(()=>{Atomics()})", "T(()=>new Atomics())",
  "T(()=>Atomics.add.length+','+Atomics.compareExchange.length+','+Atomics.wait.length+','+Atomics.notify.length+','+Atomics.isLockFree.length)");

// ---- 8. subclasses com species: ordem de efeitos e mensagens.
const spMethods = [
  ["slice", "1,3"], ["slice", ""], ["subarray", "1,3"], ["subarray", ""], ["map", "x=>x"], ["filter", "x=>true"], ["filter", "x=>false"], ["toSorted", ""], ["toReversed", ""], ["with", "0,5"],
];
const ctorsSp = [
  ["plain", "class X extends Uint8Array{}", ""],
  ["species-undefined", "class X extends Uint8Array{static get [Symbol.species](){return undefined}}", ""],
  ["species-null", "class X extends Uint8Array{static get [Symbol.species](){return null}}", ""],
  ["species-other", "class X extends Uint8Array{static get [Symbol.species](){return Uint16Array}}", ""],
  ["species-big", "class X extends Uint8Array{static get [Symbol.species](){return BigInt64Array}}", ""],
  ["species-float16", "class X extends Uint8Array{static get [Symbol.species](){return Float16Array}}", ""],
  ["species-throw", "class X extends Uint8Array{static get [Symbol.species](){throw new EvalError('sp')}}", ""],
  ["species-nonctor", "class X extends Uint8Array{static get [Symbol.species](){return 1}}", ""],
  ["species-fn", "class X extends Uint8Array{static get [Symbol.species](){return function(){return new Uint8Array(1)}}}", ""],
  ["species-small", "class X extends Uint8Array{static get [Symbol.species](){return class extends Uint8Array{constructor(n){super(1)}}}}", ""],
  ["species-large", "class X extends Uint8Array{static get [Symbol.species](){return class extends Uint8Array{constructor(n){super(n+2)}}}}", ""],
  ["species-nonta", "class X extends Uint8Array{static get [Symbol.species](){return function(){return {}}}}", ""],
  ["species-arrow", "class X extends Uint8Array{static get [Symbol.species](){return ()=>1}}", ""],
  ["ctor-undefined", "class X extends Uint8Array{}", "x.constructor=undefined;"],
  ["ctor-null", "class X extends Uint8Array{}", "x.constructor=null;"],
  ["ctor-num", "class X extends Uint8Array{}", "x.constructor=1;"],
  ["ctor-obj-species", "class X extends Uint8Array{}", "x.constructor={[Symbol.species]:Int8Array};"],
  ["ctor-obj-species-null", "class X extends Uint8Array{}", "x.constructor={[Symbol.species]:null};"],
  ["ctor-obj-nospecies", "class X extends Uint8Array{}", "x.constructor={};"],
  ["ctor-throw", "class X extends Uint8Array{}", "Object.defineProperty(x,'constructor',{get(){throw new EvalError('c')}});"],
  ["ctor-frozen", "class X extends Uint8Array{static get [Symbol.species](){return class extends Uint8Array{constructor(n){super(n);Object.freeze(this)}}}}", ""],
  ["ctor-resize", "class X extends Uint8Array{}", ""],
];
for (const [name, cls, pre] of ctorsSp) for (const [m, args] of spMethods) {
  add(`T(()=>{${cls};var x=new X([1,2,3,4]);${pre}var r=x.${m}(${args});return S(r)+','+(r instanceof X)+','+r.constructor.name})`);
}
for (const [m, args] of spMethods) {
  add(
    `T(()=>{var L=[];class X extends Uint8Array{static get [Symbol.species](){L.push('species');return class extends Uint8Array{constructor(...a){L.push('ctor '+a.length+':'+a[0]);super(...a)}}}}var x=new X([1,2,3,4]);var r=x.${m}(${args});return L.join()+'|'+S(r)})`,
    `T(()=>{var L=[];class X extends Uint8Array{get constructor(){L.push('get ctor');return super.constructor}}var x=new X([1,2,3,4]);var r=x.${m}(${args});return L.join()+'|'+S(r)})`,
    `T(()=>{var L=[];var x=new Uint8Array([1,2,3,4]);Object.defineProperty(x,'constructor',{get(){L.push('ctor');return {[Symbol.species]:Uint8Array}}});x.${m}(${args.replace(/x=>/, "x=>(L.push('cb'),")}${args.startsWith("x=>") ? ")" : ""});return L.join()})`,
    `T(()=>{var x=new Uint8Array([1,2,3,4]);x.constructor={[Symbol.species]:function(n){return new Float32Array(n)}};return S(x.${m}(${args}))})`,
    `T(()=>{var x=new Float64Array([1.5,2,3,4]);x.constructor={[Symbol.species]:function(n){return new Uint8Array(n)}};return S(x.${m}(${args}))})`,
    `T(()=>{var x=new BigInt64Array([1n,2n]);x.constructor={[Symbol.species]:function(n){return new Uint8Array(n)}};return S(x.${m}(${args.replace("0,5", "0,5n")}))})`,
  );
}
add(
  "T(()=>{var L=[];class X extends Uint8Array{constructor(...a){L.push('X '+a.length);super(...a)}}var x=new X(2);x.set([1]);return L.join()})",
  "T(()=>{class X extends Uint8Array{}return Object.getPrototypeOf(X)===Uint8Array&&X.BYTES_PER_ELEMENT})",
  "T(()=>{class X extends Uint8Array{constructor(){}}return new X})", "T(()=>{class X extends Uint8Array{constructor(){super();super()}}return new X})",
  "T(()=>{class X extends Uint8Array{constructor(){super(-1)}}return new X})", "T(()=>{class X extends Uint8Array{constructor(){super('a')}}return new X})",
  "T(()=>{class X extends Uint8Array{constructor(){return new Uint16Array(2)}}return S(new X)})", "T(()=>{class X extends Uint8Array{constructor(){return {}}}return S(new X)})",
  "T(()=>{class X extends Uint8Array{}return S(new X(2))+X.name+new X(1).constructor.name})", "T(()=>{class X extends Uint8Array{}return S(X.from([1,2]))+S(X.of(1,2))})",
  "T(()=>{class X extends Uint8Array{}return X.from([1,2]) instanceof X})", "T(()=>{class X extends Uint8Array{}return Object.prototype.toString.call(new X)})",
  "T(()=>{class X extends Uint8Array{get [Symbol.toStringTag](){return 'Q'}}return Object.prototype.toString.call(new X)+Uint8Array.prototype[Symbol.toStringTag]})",
  "T(()=>{class X extends Uint8Array{}return Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get.call(new X)})",
  "T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get.call({}))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get.call(1))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array),Symbol.species).get.call(5))",
  "T(()=>Object.getPrototypeOf(Uint8Array)()", "T(()=>new (Object.getPrototypeOf(Uint8Array))())", "T(()=>Object.getPrototypeOf(Uint8Array).from)", "T(()=>Object.getPrototypeOf(Uint8Array).name+Object.getPrototypeOf(Uint8Array).length)",
);

// ---- 9. buffer redimensionável.
const rbuf = (len, max) => `new ArrayBuffer(${len},{maxByteLength:${max}})`;
for (const n of ["Uint8Array", "Int16Array", "Float64Array", "BigInt64Array", "Float16Array"]) {
  const e = isBig(n) ? 8 : n === "Int16Array" || n === "Float16Array" ? 2 : n === "Float64Array" ? 8 : 1;
  const l = i => lit(n, i);
  add(
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.resize(32);return a.length+','+a.byteLength+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.resize(8);return a.length+','+a.byteLength+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.resize(0);return a.length+','+a.byteLength+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e});b.resize(${e});return a.length+','+a.byteLength+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return a.length+','+a.byteLength+','+a.byteOffset+','+S(a.at(0))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});a.fill(${l(1)});return 1})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return S(a)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return Object.keys(a).length+','+(0 in a)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return a.slice()})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return a.subarray(0)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return a.with(0,${l(1)})})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return [...a]})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});return a.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,${e * 2});b.resize(${e});b.resize(16);return a.length+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,0,2);b.resize(${e});return a.length+','+a.byteLength+','+a.byteOffset})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,0,2);b.resize(${e * 2});return a.length+','+a.byteLength})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,0,2);b.resize(${e});b.resize(16);return a.length+','+a.byteLength})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,0,2);b.resize(${e});return a.indexOf(${l(0)})})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var r=[];b.resize(${e * 3});for(var x of a){r.push(S(x))}return r.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var r=0;for(var x of a){if(r++===0)b.resize(${e});}return r})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var r=0;a.forEach(()=>{if(r++===0)b.resize(${e})});return r})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var r=[];a.forEach((x,i)=>{if(i===0)b.resize(${e}*3);r.push(i)});return r.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.map((x,i)=>{if(i===0)b.resize(${e});return ${l(1)}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.filter((x,i)=>{if(i===0)b.resize(${e});return true}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.find((x,i)=>{if(i===0)b.resize(${e});return x===undefined}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.findLast((x,i)=>{if(i===${16 / e - 1})b.resize(${e});return x===undefined}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.sort((x,y)=>{b.resize(${e});return 0}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.toSorted((x,y)=>{b.resize(${e});return 0}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.join({toString(){b.resize(${e});return '-'}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.fill(${l(1)},{valueOf(){b.resize(${e});return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.fill({valueOf(){b.resize(${e});return ${l(1)}}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.slice({valueOf(){b.resize(${e});return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.subarray(0,{valueOf(){b.resize(${e});return 4}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.copyWithin(0,{valueOf(){b.resize(${e});return 1}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.set([${l(1)}],{valueOf(){b.resize(0);return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);a[0]={valueOf(){b.resize(0);return ${l(1)}}};return a.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.at({valueOf(){b.resize(0);return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.includes(undefined,{valueOf(){b.resize(0);return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(a.indexOf(${l(0)},{valueOf(){b.resize(0);return 0}}))})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var t=new ${n}(2);t.set(a);b.resize(${e});return S(t)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return S(new ${n}(a))+','+b.resizable})`, `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);return new ${n}(a).buffer.resizable})`,
    `T(()=>{var b=${rbuf(16, 32)};b.resize(33)})`, `T(()=>{var b=${rbuf(16, 32)};b.resize(-1)})`, `T(()=>{var b=${rbuf(16, 32)};b.resize(NaN);return b.byteLength})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,{valueOf(){b.resize(0);return ${e}}});return a.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b,0,{valueOf(){b.resize(0);return 1}});return a.length})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);var d=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),'length').get;b.resize(${e * 3});return d.call(a)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.transfer();return a.length+','+a.byteLength+','+a.byteOffset+','+a.buffer.byteLength})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.transfer();return a.fill(${l(1)})})`, `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.transfer();return S(a)})`,
    `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.transfer();return a.at(0)})`, `T(()=>{var b=${rbuf(16, 32)};var a=new ${n}(b);b.transfer();return Object.keys(a).length})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();a[0]=${l(1)};return a.length+','+a[0]})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return new ${n}(a)})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return new ${n}(b)})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.subarray(0)})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.toString()})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.with(0,${l(1)})})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.set([])})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.keys().next()})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);var it=a.values();b.transfer();return it.next()})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return Atomics.load(a,0)})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return Reflect.has(a,0)})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return Reflect.ownKeys(a).length})`, `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return a.toSorted()})`,
    `T(()=>{var b=new ArrayBuffer(16);var a=new ${n}(b);b.transfer();return ${n}.from(a)})`,
  );
}
add(
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:8});return b.resizable+','+b.maxByteLength})", "T(()=>new ArrayBuffer(8,{maxByteLength:4}))", "T(()=>new ArrayBuffer(8,{maxByteLength:-1}))", "T(()=>new ArrayBuffer(8,{maxByteLength:2**53}))",
  "T(()=>new ArrayBuffer(8,{maxByteLength:undefined}).resizable)", "T(()=>new ArrayBuffer(8,1).resizable)", "T(()=>new ArrayBuffer(8,null).resizable)", "T(()=>new ArrayBuffer(-1))", "T(()=>new ArrayBuffer(2**53))", "T(()=>new ArrayBuffer(NaN).byteLength)",
  "T(()=>new ArrayBuffer(1.9).byteLength)", "T(()=>new ArrayBuffer('3').byteLength)", "T(()=>new ArrayBuffer(Symbol()))", "T(()=>ArrayBuffer(1))", "T(()=>new ArrayBuffer(8,{get maxByteLength(){throw new EvalError('m')}}))",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var s=b.slice(2);return S(s)+s.resizable})", "T(()=>{var b=new ArrayBuffer(8);b.resize(4)})",
  "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var t=b.transfer(4);return S(t)+b.detached+b.byteLength})", "T(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var t=b.transferToFixedLength();return S(t)+t.resizable})",
  "T(()=>{var b=new ArrayBuffer(8);var t=b.transfer(16);return S(t)+t.resizable+b.detached})", "T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.transfer()})", "T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.slice(0)})",
  "T(()=>{var b=new ArrayBuffer(8);b.transfer();return b.byteLength+','+b.maxByteLength+','+b.resizable})",
  "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(12);return S(b)+b.growable+b.maxByteLength})".replace("S(b)", "b.byteLength"),
  "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b);b.grow(12);return a.length})", "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(4)})",
  "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(17)})", "T(()=>{var b=new SharedArrayBuffer(8);b.grow(9)})", "T(()=>{var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Uint8Array(b,0,4);b.grow(16);return a.length})",
);

// ---- 10. mensagens exatas de erro.
for (const n of ["Uint8Array", "Float64Array", "BigInt64Array"]) {
  const l = i => lit(n, i);
  add(
    `T(()=>new ${n}(new ArrayBuffer(5)))`, `T(()=>new ${n}(new ArrayBuffer(8),1))`, `T(()=>new ${n}(new ArrayBuffer(8),0,9))`, `T(()=>new ${n}(new ArrayBuffer(8),16))`, `T(()=>new ${n}(-1))`, `T(()=>new ${n}(2**53))`,
    `T(()=>new ${n}(2**40))`, `T(()=>new ${n}(Symbol()))`, `T(()=>new ${n}([Symbol()]))`, `T(()=>${n}.prototype.fill.call([],${l(1)}))`, `T(()=>${n}.prototype.length)`, `T(()=>${n}.prototype.byteLength)`,
    `T(()=>${n}.prototype.buffer)`, `T(()=>${n}.prototype.byteOffset)`, `T(()=>${n}.prototype[Symbol.toStringTag])`, `T(()=>${n}.prototype.at.call({},0))`, `T(()=>${n}.prototype.set.call({},[]))`,
    `T(()=>${n}.prototype.slice.call(new DataView(new ArrayBuffer(1))))`, `T(()=>${n}.prototype.map.call(null,x=>x))`, `T(()=>${n}.prototype.join.call(undefined))`, `T(()=>${n}.prototype.values.call(1))`,
    `T(()=>new ${n}(1).map(1))`, `T(()=>new ${n}(1).map())`, `T(()=>new ${n}(1).forEach({}))`, `T(()=>new ${n}(1).reduce(()=>1,undefined))`, `T(()=>new ${n}(0).reduce(()=>1))`, `T(()=>new ${n}(0).reduceRight(()=>1))`,
    `T(()=>new ${n}(1).find('x'))`, `T(()=>new ${n}(1).findLast())`, `T(()=>new ${n}(1).filter(null))`, `T(()=>new ${n}(1).some(1))`, `T(()=>new ${n}(1).every(Symbol()))`, `T(()=>new ${n}(1).sort(1))`,
    `T(()=>new ${n}(1).with(1,${l(1)}))`, `T(()=>new ${n}(1).with(-2,${l(1)}))`, `T(()=>new ${n}(1).with(0,'x'))`, `T(()=>new ${n}(1).with(0,Symbol()))`, `T(()=>new ${n}(1).set([1],2))`, `T(()=>new ${n}(1).set([1],-1))`,
    `T(()=>new ${n}(1).set(new ${n}(2)))`, `T(()=>new ${n}(1).set(new Uint8Array(2)))`, `T(()=>new ${n}(1).set(new BigInt64Array(1)))`, `T(()=>new ${n}(1).set(new Float64Array(1)))`, `T(()=>new ${n}(1).set(new Float16Array(1)))`,
    `T(()=>new ${n}(1).subarray(Symbol()))`, `T(()=>new ${n}(1).slice(Symbol()))`, `T(()=>new ${n}(1).fill(Symbol()))`, `T(()=>new ${n}(1).copyWithin(Symbol()))`, `T(()=>new ${n}(1).indexOf(Symbol()))`,
    `T(()=>new ${n}(1).at(Symbol()))`, `T(()=>new ${n}(1).join(Symbol()))`, `T(()=>new ${n}(1).toLocaleString(1))`, `T(()=>new ${n}(2).toLocaleString())`, `T(()=>${n}.from(1,1))`, `T(()=>${n}.from([],1))`,
    `T(()=>${n}.from.call(1,[]))`, `T(()=>${n}.of.call(1))`, `T(()=>${n}.from({length:1,0:Symbol()}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{value:Symbol()}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{value:${l(1)}}))[0]`.replace(")[0]", ")"),
    `T(()=>Object.defineProperty(new ${n}(1),'1',{value:${l(1)}}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{get(){}}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{value:${l(1)},writable:false}))`,
    `T(()=>Object.defineProperty(new ${n}(1),'0',{value:${l(1)},enumerable:false}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{value:${l(1)},configurable:false}))`, `T(()=>Object.defineProperty(new ${n}(1),'0',{value:${l(1)},configurable:true}))`,
    `T(()=>{'use strict';var a=new ${n}(1);Object.freeze(a)})`, `T(()=>{'use strict';var a=new ${n}(1);Object.defineProperty(a,'length',{value:3})})`, `T(()=>{'use strict';var a=new ${n}(1);a.buffer=1})`, `T(()=>{'use strict';var a=new ${n}(1);a.byteLength=1})`,
    `T(()=>{'use strict';var a=new ${n}(1);a[Symbol.toStringTag]='x'})`, `T(()=>{'use strict';var a=new ${n}(1);delete a[0]})`, `T(()=>{'use strict';var a=new ${n}(1);delete a[1];return 'ok'})`,
    `T(()=>{'use strict';var a=new ${n}(1);delete a.length;return 'ok'})`, `T(()=>{'use strict';var a=new ${n}(1);return delete a.foo})`,
    `T(()=>{var a=new ${n}(1);Object.setPrototypeOf(a,null);return a.length})`, `T(()=>{var a=new ${n}(1);Object.setPrototypeOf(a,null);return a[0]+','+Object.keys(a)})`,
    `T(()=>{var a=new ${n}(1);Object.setPrototypeOf(a,Array.prototype);return a.length+','+Array.isArray(a)})`, `T(()=>{var a=new ${n}(1);Object.setPrototypeOf(a,Array.prototype);return a.push(1)})`,
    `T(()=>{var a=new ${n}(1);Object.setPrototypeOf(a,Array.prototype);return S(a.map(x=>x))})`,
    `T(()=>Array.prototype.push.call(new ${n}(1),1))`, `T(()=>Array.prototype.pop.call(new ${n}(1)))`, `T(()=>Array.prototype.shift.call(new ${n}(1)))`, `T(()=>Array.prototype.splice.call(new ${n}(1),0,1))`,
    `T(()=>S(Array.prototype.reverse.call(new ${n}([${l(1)},${l(2)}]))))`, `T(()=>S(Array.prototype.map.call(new ${n}([${l(1)}]),x=>x)))`, `T(()=>S(Array.prototype.fill.call(new ${n}(2),${l(3)})))`,
    `T(()=>S(Array.prototype.concat.call(new ${n}([${l(1)}]),1)))`, `T(()=>S(Array.prototype.flat.call(new ${n}([${l(1)}]))))`, `T(()=>S(Array.prototype.includes.call(new ${n}([${l(1)}]),${l(1)})))`,
    `T(()=>S(Array.prototype.sort.call(new ${n}([${l(2)},${l(1)}]))))`, `T(()=>S(Array.prototype.unshift.call(new ${n}(1),${l(1)})))`, `T(()=>S(Array.prototype.copyWithin.call(new ${n}([${l(1)},${l(2)}]),0,1)))`,
  );
}
for (const n of ALL) {
  add(
    `T(()=>new ${n}(new ArrayBuffer(3)))`, `T(()=>new ${n}(new ArrayBuffer(8),3))`, `T(()=>new ${n}(new ArrayBuffer(8),0,5))`, `T(()=>new ${n}(new ArrayBuffer(8),0,-1))`, `T(()=>new ${n}(new ArrayBuffer(8),-1))`,
    `T(()=>new ${n}(-1))`, `T(()=>new ${n}(2**53))`, `T(()=>new ${n}(2**32+1).length)`, `T(()=>new ${n}({length:2**33}).length)`, `T(()=>new ${n}(new ${isBig(n) ? "Float64Array" : "BigInt64Array"}(1)))`,
    `T(()=>${n}.from(new ${isBig(n) ? "Float64Array" : "BigInt64Array"}(1)))`, `T(()=>new ${n}(1).set(new ${isBig(n) ? "Float64Array" : "BigInt64Array"}(1)))`, `T(()=>new ${n}(1).set([${isBig(n) ? "1" : "1n"}]))`,
    `T(()=>new ${n}(1).with(0,${isBig(n) ? "1" : "1n"}))`, `T(()=>new ${n}(1).fill(${isBig(n) ? "1" : "1n"}))`, `T(()=>new ${n}(1).includes(${isBig(n) ? "1" : "1n"}))`, `T(()=>new ${n}(1).indexOf(${isBig(n) ? "1" : "1n"}))`,
  );
}

// ---- 11. Arredondamento de Float16 / Float32 e leitura de volta pelos bytes.
const f16vals = ["0.1", "0.2", "0.3", "1/3", "2/3", "1.0009765625", "1.00048828125", "1.000732421875", "1.0004882813", "2047.5", "2048.5", "2049", "2050", "4097", "8193", "65504", "65504.1", "65519.99", "65520", "65535", "70000", "1e-8", "5.960464477539063e-8", "2.9802322387695312e-8", "2.98023223876953125e-8", "2.9802322387695313e-8", "6.097555160522461e-5", "6.103515625e-5", "6.1e-5", "-0.0000001", "NaN", "Infinity", "-Infinity", "-0", "3.140625", "3.1416", "100.1", "1000.3", "0.00006", "0.00003"];
for (const v of f16vals) {
  add(
    `T(()=>S(new Float16Array([${v}])))`, `T(()=>S(new Float32Array([${v}])))`, `T(()=>S(Math.f16round(${v})))`, `T(()=>S(Math.fround(${v})))`,
    `T(()=>{var a=new Float16Array([${v}]);return Array.from(new Uint8Array(a.buffer)).join()})`, `T(()=>{var a=new Float32Array([${v}]);return Array.from(new Uint8Array(a.buffer)).join()})`,
    `T(()=>{var a=new Float16Array([${v}]);return Array.from(new Uint16Array(a.buffer)).map(x=>x.toString(16)).join()})`, `T(()=>{var a=new Float32Array([${v}]);return Array.from(new Uint32Array(a.buffer)).map(x=>x.toString(16)).join()})`,
    `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setFloat16(0,${v});return d.getUint16(0).toString(16)+','+d.getFloat16(0)})`, `T(()=>{var d=new DataView(new ArrayBuffer(8));d.setFloat16(0,${v},true);return d.getUint16(0).toString(16)})`,
  );
}
for (const bits of ["0x0000", "0x8000", "0x0001", "0x03ff", "0x0400", "0x3c00", "0x3c01", "0x7bff", "0x7c00", "0xfc00", "0x7c01", "0x7e00", "0xffff", "0x8001", "0xbc00", "0x5640", "0x2e66"]) {
  add(`T(()=>{var u=new Uint16Array([${bits}]);var f=new Float16Array(u.buffer);return S(f[0])+','+Object.is(f[0],-0)})`, `T(()=>{var u=new Uint16Array([${bits}]);var f=new Float16Array(u.buffer);return S(new Float32Array(f)[0])}`.replace("}", ")"));
}
for (const bits of ["0x00000000", "0x80000000", "0x00000001", "0x007fffff", "0x00800000", "0x7f7fffff", "0x7f800000", "0xff800000", "0x7fc00000", "0x7f800001", "0xffffffff", "0x3f800001", "0x33800000", "0x33000000"]) {
  add(`T(()=>{var u=new Uint32Array([${bits}]);var f=new Float32Array(u.buffer);return S(f[0])+','+S(new Float64Array(f)[0])})`);
}
for (const bits of ["0x7ff0000000000001n", "0x7ff8000000000000n", "0xfff8000000000001n", "0x8000000000000000n", "0x0000000000000001n", "0x7fefffffffffffffn"]) {
  add(`T(()=>{var u=new BigUint64Array([${bits}]);var f=new Float64Array(u.buffer);return S(f[0])+','+S(new Float32Array(f)[0])+','+S(new Float16Array(f)[0])})`,
    `T(()=>{var f=new Float64Array(1);var u=new BigUint64Array(f.buffer);f[0]=NaN;return u[0].toString(16)}`.replace("}", ")"));
}
// Float16Array: superfície.
add(
  "T(()=>Float16Array.BYTES_PER_ELEMENT)", "T(()=>Float16Array.name+Float16Array.length)", "T(()=>typeof Math.f16round)", "T(()=>Math.f16round.length)", "T(()=>typeof DataView.prototype.getFloat16)",
  "T(()=>DataView.prototype.setFloat16.length)", "T(()=>Object.getPrototypeOf(Float16Array)===Object.getPrototypeOf(Int8Array))", "T(()=>S(new Float16Array([1,2,3]).map(x=>x/3)))", "T(()=>S(new Float16Array([1,2,3]).toSorted((a,b)=>b-a)))",
  "T(()=>S(Float16Array.from([0.1,0.2],x=>x*3)))", "T(()=>S(new Float16Array([3,1,NaN,-0,0,-Infinity]).sort()))", "T(()=>S(new Float32Array([3,1,NaN,-0,0,-Infinity]).sort()))", "T(()=>S(new Float64Array([3,1,NaN,-0,0,-Infinity]).sort()))",
  "T(()=>S(new Float64Array([3,1,NaN,-0,0,-Infinity]).toSorted((a,b)=>b-a)))", "T(()=>S(new Float64Array([NaN,1]).indexOf(NaN)))", "T(()=>S(new Float64Array([NaN,1]).includes(NaN)))", "T(()=>S(new Float64Array([-0]).includes(0)))",
  "T(()=>S(new Float64Array([-0]).indexOf(0)))", "T(()=>S(new Float64Array([0]).lastIndexOf(-0)))", "T(()=>S(new Float64Array([1,2]).fill(-0)))", "T(()=>S(new Float64Array([1,2]).with(0,-0)))",
  "T(()=>S(new Uint8Array([3,1,2]).sort((a,b)=>b-a)))", "T(()=>S(new Int8Array([-1,1,-128,127]).sort()))", "T(()=>S(new Uint8ClampedArray([-1,1,300]).sort()))", "T(()=>S(new BigInt64Array([3n,-1n,2n]).sort()))",
  "T(()=>S(new BigUint64Array([3n,2n**64n-1n,2n]).sort()))", "T(()=>S(new BigInt64Array([3n,-1n,2n]).toSorted((a,b)=>a<b?1:-1)))", "T(()=>S(new BigInt64Array([3n,-1n,2n]).toReversed()))",
  "T(()=>S(new Float64Array([1,2,3]).sort((a,b)=>b-a)))", "T(()=>S(new Float64Array([1,2,3]).sort(()=>{return undefined})))",
);

// ---- 12. Atomics e conversões de elemento cruzando tipos.
for (const n of intT.slice(0, 6)) for (const v of ["255", "256", "-1", "65537", "2**32+5", "2**31", "1.9", "'3'", "NaN"]) {
  add(`T(()=>{var a=new ${n}(1);a[0]=${v};return S(Atomics.add(a,0,${v}))+','+S(Atomics.sub(a,0,${v}))+','+S(Atomics.and(a,0,${v}))+','+S(Atomics.or(a,0,${v}))+','+S(Atomics.xor(a,0,${v}))+','+S(a[0])})`);
}

// ---- Emissão: dedup, execução em paralelo, um bun filho novo por programa.
const baseSources = [];
for (const file of fs.readdirSync(path.join(__dirname, "..", "tests", "golden"))) {
  if (!/^(typedarray.*|atomics|sab|dataview|buffer.*)_bun\.tsv$/.test(file)) continue;
  for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}

function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 20000);
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => { clearTimeout(timer); resolve({ code, out: code === 0 ? decodeResult(out) : null, err }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(4, require("os").cpus().length) }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i].source);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const { code, out, err } = results[i];
    const { expr, source } = jobs[i];
    if (code !== 0 || out === null) { dropped++; process.stderr.write("filho falhou: " + JSON.stringify(expr).slice(0, 160) + " " + err.slice(0, 120) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(out)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n"); continue; }
    if (/[\u2013\u2014]/.test(out)) { dropped++; continue; }
    kept++;
    lines.push({ source: source, result: out });
  }
  process.stdout.write(emitFactored("typedarray_ctor", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
