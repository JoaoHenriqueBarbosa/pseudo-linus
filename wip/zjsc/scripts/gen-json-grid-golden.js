// Gera tests/golden/json_grid_bun.tsv: grade de JSON.parse e JSON.stringify, medida no bun 1.4.2.
// Complementa json_bun, json_more_bun e json_number_bun: parse inválido com mensagem exata de SyntaxError (posição,
// caractere inesperado, fim inesperado, string não terminada, escape inválido, número inválido, vírgula sobrando,
// comentários, NaN/Infinity, BOM, aspas simples), parse válido de borda (chaves duplicadas, __proto__, ordem de
// chaves inteiras, profundidade), reviver com `context.source`, stringify com toJSON/replacer/space sobre tipos exóticos
// (Proxy, TypedArray, Map, Set, Date inválida, wrappers, símbolos, getters que lançam, ciclos indiretos com a mensagem
// completa), BigInt, surrogates isolados, JSON.rawJSON/isRawJSON. Programas cuja expressão já aparece nos três goldens
// de JSON são descartados (dedup por fonte).
// Cada programa roda num bun filho novo (a ordem de reificação das tabelas estáticas do JSC depende do que rodou
// antes), sem APIs de host, e grava o resultado em `globalThis.R`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-json-grid-golden.js > tests/golden/json_grid_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>4)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const parse = s => add(`T(()=>JSON.parse(${q(s)}))`);

// ---- 1. Parse inválido: prefixo estrutural e cauda ruim.
const prefixes = ["", "[", "[1,", "[[", "{\"a\":", "{\"a\":[", "[{\"a\":", "{\"a\":{\"b\":", "{\"a\":1,", "[1,2,"];
const tails = [
  "x", "'a'", "NaN", "Infinity", "-Infinity", "-NaN", "/*c*/1", "//c\n1", "\u00a01", "\ufeff1", "\u20281", "\u20291", "\u000b1", "\u000c1",
  "é", "\ud83d\ude00", "undefined", "nul", "nulll", "True", "FALSE", "+1", ".5", "1.", "01", "-", "--1", "1e", "1e+", "0x1F", "1_0", "\"a", "\"\\", "\"\\x41\"",
  "\"\\u12\"", "\"\\u12G4\"", "\"\n\"", "\"\t\"", "\"\u0000\"", "\"\u001f\"", "}", "]", ",", ":", "[,", "{,", "{:", "{\"a\"", "{\"a\":", "{'a':1}", "{a:1}",
];
for (const p of prefixes) for (const t of tails) parse(p + t);

// ---- 2. Parse inválido: depois de um valor completo.
const completes = ["1", "\"s\"", "null", "true", "false", "[]", "{}", "[1]", "{\"a\":1}", "-0", "1e5", "0.5"];
const trailers = [" 1", "1", "x", ",", ":", "]", "}", " //", " /*", "\n\n,", "\"", "'", " NaN", "\u0000", "\ufeff", "é", "  \t\r\n x", "0", ".5", "e1"];
for (const c of completes) for (const t of trailers) parse(c + t);

// ---- 3. Vírgula sobrando, vírgula faltando, dois-pontos faltando, chave não string.
const structural = [
  "[1,]", "[,1]", "[,]", "[1,,2]", "[1 2]", "[1;2]", "[1,2,]", "[[],]", "[{},]", "{\"a\":1,}", "{,\"a\":1}", "{\"a\":1,,\"b\":2}", "{\"a\" 1}", "{\"a\"}",
  "{\"a\":}", "{\"a\":,}", "{\"a\"::1}", "{\"a\":1 \"b\":2}", "{1:1}", "{null:1}", "{true:1}", "{[1]:1}", "{\"a\":1;}", "{\"a\":1]", "[1}", "[}", "{]", "[\"a\":1]",
  "{\"a\":1}}", "[1]]", "[[1]", "{{}}", "{\"a\":{}", "{\"a\":[}", "[{]", "[\"a\"", "[\"a\",", "{\"a\":\"b\"", "{\"a\":\"b\",", "{\"a\":\"b\",\"c\"",
  "{\"a\":\"b\",\"c\":", "[1,2,3", "[1,2,3,", "[true,false,nul", "{\"a\":tru", "{\"a\":-", "{\"a\":1.", "{\"a\":1e", "{\"a\":\"\\u", "{\"a\":\"\\u00",
  "\t\n\r [ \t\n\r 1 \t\n\r , \t\n\r ] \t\n\r x", "[\n1,\n2,\n]", "{\n\"a\": 1,\n}", "[\r\n1,\r\n,2]", "{\n  \"a\": [\n    1,\n    2,\n  ]\n}",
];
for (const s of structural) parse(s);

// ---- 4. Strings e escapes.
const escapes = ["\\", "\\a", "\\v", "\\0", "\\x00", "\\x41", "\\'", "\\ ", "\\\n", "\\u", "\\u0", "\\u00", "\\u000", "\\u000G", "\\uGGGG", "\\u+123", "\\u 123", "\\U0041", "\\u{41}", "\\u{1F600}", "\\ud800", "\\udc00", "\\ud800\\ud800", "\\ud800\\u0041", "\\udc00\\ud800", "\\ud83d\\ude00", "\\/", "\\\"", "\\\\", "\\b\\f\\n\\r\\t"];
for (const e of escapes) {
  parse(`"${e}"`);
  parse(`"a${e}b"`);
  parse(`{"${e}":1}`);
}
for (let c = 0; c < 0x20; c++) parse(`"a${String.fromCharCode(c)}b"`);
parse("\"\u007f\""); parse("\"\u0080\""); parse("\"\u2028\""); parse("\"\u2029\""); parse("\"\ud800\""); parse("\"\udc00\""); parse("\"\ud800a\"");
parse("\"unterminated"); parse("\"unterminated\\"); parse("\"unterminated\\\""); parse("'single'"); parse("\"a\" \"b\""); parse("\"a\"\"b\"");
parse("\"" + "x".repeat(100) + "\\q\""); parse("{\"" + "k".repeat(50) + "\":\"" + "v".repeat(50) + "\\z\"}");

// ---- 5. Números inválidos.
const nums = ["+0", "-+1", "+-1", "1+1", "1-1", "0.", "-.5", ".", "-.", "1..2", "1.2.3", "1e1.5", "1e+-1", "1ee1", "1E", "1E+", "1E-", "0e", "0E+", "00", "-00", "01.5", "-01", "0b1", "0o7", "0x", "0X1", "1n", "1_000", "١٢٣", "１２３", "1 .5", "1. 5", "- 1", "-\t1", "Infinity", "-Infinity", "+Infinity", "NaN", "-NaN", "0xFF", "1e1000x", "1.5e", "-", "--", "1e0.0"];
for (const n of nums) { parse(n); parse(`[${n}]`); parse(`{"a":${n}}`); }

// ---- 6. Comentários, aspas simples, BOM, espaços fora da gramática.
const odd = [
  "/**/1", "1/**/", "[1/**/]", "[/*x*/1]", "{/*x*/\"a\":1}", "//x\n[1]", "[1]//x", "/* unterminated", "/", "//", "/*", "[1]/*", "#1", "# c\n1", "<!-- c\n1", "--> c\n1",
  "\ufeff[1]", "\ufeff", "[\ufeff1]", "\ufeff\ufeff1", "1\ufeff", "\u00a0", "\u00a0[1]", "[\u00a01]", "\u2028[1]", "\u2029[1]", "\u3000[1]", "\u180e1", "\u200b1", "\ufeffnull",
  "'a'", "'a", "['a']", "{'a':1}", "{\"a\":'b'}", "[\"a\",'b']", "`a`", "{\"a\":`b`}", "\"a\" 'b'", "''", "\"\"\"",
  "", " ", "\n", "\t", "\r\n", "  \n  ", "\u0000", "\u0001", "[\u0000]", "\"\\u0000", "null\u0000", "\u001f1",
  "undefined", "void 0", "function(){}", "()=>1", "new Date", "this", "globalThis", "Symbol()", "1n", "BigInt(1)", "[undefined]", "{\"a\":undefined}", "[NaN]", "[Infinity]", "[-Infinity]", "{\"a\":NaN}",
  "{\"__proto__\":", "{\"__proto__\":1,", "[\"__proto__\"", "{\"a\":1,\"__proto__\"", "{\"constructor\"", "{\"toString\":", "\"__proto__\"\"",
  "tru", "fals", "nul", "truee", "falsee", "nullx", "t", "f", "n", "tr", "fa", "nu", "TRUE", "NULL", "True", "False", "Null", "true false", "null null",
];
for (const s of odd) parse(s);

// ---- 7. Parse válido de borda.
const valid = [
  "{\"a\":1,\"a\":2}", "{\"a\":1,\"b\":2,\"a\":3}", "{\"__proto__\":1}", "{\"__proto__\":null}", "{\"__proto__\":{\"x\":1}}", "{\"__proto__\":[]}", "{\"a\":1,\"__proto__\":2,\"__proto__\":3}",
  "{\"__proto__\":{\"__proto__\":{}}}", "{\"constructor\":1,\"toString\":2,\"hasOwnProperty\":3,\"valueOf\":4}", "{\"\":1}", "{\"\":1,\"\":2}", "{\" \":1}", "{\"\\u0000\":1}",
  "{\"b\":1,\"a\":2,\"1\":3,\"0\":4}", "{\"-1\":1,\"1.5\":2,\"01\":3,\"1\":4,\"4294967294\":5,\"4294967295\":6,\"4294967296\":7}", "{\"2\":1,\"1\":1,\"a\":1,\"0\":1}",
  "{\"9007199254740991\":1,\"9007199254740992\":2,\"1e21\":3,\"1e-7\":4}", "{\"10\":1,\"9\":2,\"8\":3,\"100\":4,\"1\":5}", "{\"4294967295\":1,\"4294967294\":2,\"2147483648\":3,\"2147483647\":4}",
  "{\"length\":1}", "[1,2,3]", "[]", "{}", "[[]]", "[{}]", "{\"a\":{}}", "{\"a\":[]}", "[[[]]]", "[null,null]", "[true,false]",
  "  [  1  ,  2  ]  ", "\t\r\n[\t\r\n1\t\r\n]\t\r\n", "\"\"", "\" \"", "\"\\u0041\"", "\"\\u00e9\"", "\"é\"", "\"\ud83d\ude00\"", "\"\\ud83d\\ude00\"", "\"\\ud83d\"", "\"\\ude00\"", "\"\\ude00\\ud83d\"", "\"\\u2028\\u2029\"",
  "\"\\/\\/\"", "\"\\\"\\\\\\b\\f\\n\\r\\t\"", "\"\\u0000\"", "\"\\u001f\"", "\"\\uFFFF\"", "\"\\uffff\"", "\"\\uD834\\uDD1E\"", "\"\\uD834\\udd1e\"", "\"\\ud834\\uDD1E\"",
  "-0", "0", "-0.0", "0e0", "0E0", "0e+0", "0e-0", "1E2", "1e+2", "1e-2", "123456789012345678901234567890", "1e308", "1e309", "-1e309", "1e-324", "5e-324", "2e-324", "1.7976931348623157e308", "9007199254740993",
  "0.1", "0.30000000000000004", "4.35", "1.005", "123.456e-2", "1E400", "-1E400", "0.0000001", "1e21", "1e20", "123456789.123456789",
  "[-0]", "[0,-0]", "{\"a\":-0}", "null", "true", "false", "[true,false,null]",
];
for (const s of valid) parse(s);
for (const s of valid) add(`T(()=>JSON.stringify(JSON.parse(${q(s)})))`);
for (const s of valid) add(`T(()=>{var o=JSON.parse(${q(s)});return o!==null&&typeof o==="object"?Object.getOwnPropertyNames(o).join()+"|"+Object.getPrototypeOf(o)===Object.prototype:typeof o})`);
// Profundidade.
for (const n of [1, 2, 10, 50, 100, 250, 500, 1000, 2000, 4000]) {
  add(`T(()=>{var d=${n};var s="[".repeat(d)+"]".repeat(d);var o=JSON.parse(s);var k=0;while(Array.isArray(o)&&o.length){o=o[0];k++}return k+"/"+typeof o})`);
  add(`T(()=>{var d=${n};var s="[".repeat(d)+"1"+"]".repeat(d);var o=JSON.parse(s);var k=0;while(Array.isArray(o)){o=o[0];k++}return k+"/"+o})`);
  add(`T(()=>{var d=${n};var s="{\\"a\\":".repeat(d)+"1"+"}".repeat(d);var o=JSON.parse(s);var k=0;while(typeof o==="object"){o=o.a;k++}return k+"/"+o})`);
  add(`T(()=>{var d=${n};var s="[".repeat(d)+"1"+"]".repeat(d-1);return JSON.parse(s)})`);
  add(`T(()=>{var d=${n};var o=[];for(var i=0;i<d;i++)o=[o];return JSON.stringify(o).length})`);
  add(`T(()=>{var d=${n};var o={};for(var i=0;i<d;i++)o={a:o};return JSON.stringify(o).length})`);
  add(`T(()=>{var d=${n};var o=[];for(var i=0;i<d;i++)o=[o];return JSON.stringify(o,null,1).length})`);
}

// ---- 8. Reviver e context.source.
const reviverInputs = [
  "1", "1.0", "1e2", "-0", "0.10", "12345678901234567890", "1E+2", "\"a\"", "\"\\u0041\"", "\"a\\nb\"", "true", "false", "null", "[]", "{}", "[1,2]", "{\"a\":1}", "[1,[2,[3]]]",
  "{\"a\":{\"b\":1.50}}", "[1.0,2.00,\"x\",null]", "{\"a\":1,\"a\":2}", "{\"1\":1,\"0\":2,\"a\":3}", "  7  ", "\t\"s\"\n", "[ 1 , 2 ]", "{\"__proto__\":1.0}", "[9007199254740993]", "[1e400]", "[-1e400]", "[5e-324]",
];
const revivers = [
  "(k,v,c)=>{log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{log.push(k+\":\"+Object.keys(c).join());return v}",
  "(k,v,c)=>{log.push(k+\":\"+typeof c.source);return v}",
  "(k,v,c)=>{log.push(S(k)+\"|\"+S(v)+\"|\"+(\"source\" in c));return v}",
  "(k,v,c)=>typeof v===\"number\"?c.source:v",
  "(k,v,c)=>typeof v===\"number\"?BigInt(c.source.split(/[.eE]/)[0]):v",
  "(k,v,c)=>typeof v===\"object\"&&v!==null?v:[c.source]",
  "(k,v,c)=>{if(Array.isArray(this))return v;return v}",
  "function(k,v,c){log.push(k+\"/\"+Array.isArray(this)+\"/\"+Object.keys(this).join());return v}",
  "(k,v,c)=>{log.push(Object.getPrototypeOf(c)===Object.prototype);return v}",
  "(k,v,c)=>{log.push(Object.isFrozen(c)+\",\"+Object.isExtensible(c));return v}",
  "(k,v,c)=>{log.push(JSON.stringify(Object.getOwnPropertyDescriptor(c,\"source\")||null));return v}",
];
for (const r of revivers) for (const s of reviverInputs) {
  add(`T(()=>{var log=[];var r=JSON.parse(${q(s)},${r});return log.join(\";\")+\" => \"+S(r)})`);
}
// Mutação durante o reviver e `source` sumindo.
const mutations = [
  "(k,v,c)=>{if(k===\"a\")this.b=99;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"a\")delete this.b;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"a\")this.b=this.b;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"a\")this.b=2;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"0\")this[1]=7;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"0\")this.length=1;log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{if(k===\"0\")this.push(5);log.push(k+\"=\"+S(c));return v}",
  "(k,v,c)=>{log.push(k+\"=\"+S(c));return k===\"\"?v:undefined}",
  "(k,v,c)=>{log.push(k+\"=\"+S(c));return k===\"b\"?v+1:v}",
  "(k,v,c)=>{log.push(k+\"=\"+S(c));if(typeof v===\"number\")throw new RangeError(\"r\"+v);return v}",
];
const mutInputs = ["{\"a\":1,\"b\":2}", "{\"a\":1,\"b\":2,\"c\":3}", "[1,2,3]", "[1,2]", "{\"a\":{\"b\":1},\"b\":2}", "{\"a\":[1,2],\"b\":[3]}"];
for (const r of mutations) for (const s of mutInputs) add(`T(()=>{var log=[];var r=JSON.parse(${q(s)},${r});return log.join(\";\")+\" => \"+S(r)})`);
add(
  "T(()=>JSON.parse('1',1))", "T(()=>JSON.parse('1',null))", "T(()=>JSON.parse('1',undefined))", "T(()=>JSON.parse('1','x'))", "T(()=>JSON.parse('1',{}))", "T(()=>JSON.parse('1',[]))",
  "T(()=>JSON.parse('[1]',function(){return this}))", "T(()=>JSON.parse('[1]',function(k,v){return k===''?v:this}).length)", "T(()=>JSON.parse('[1,2]',(k,v)=>k===''?v:undefined).length)",
  "T(()=>JSON.parse('{\"a\":1}',(k,v)=>k===''?v:undefined))", "T(()=>Object.keys(JSON.parse('{\"a\":1,\"b\":2}',(k,v)=>k===\"a\"?undefined:v)).join())",
  "T(()=>JSON.parse('{\"a\":1}',(k,v,c)=>arguments.length))", "T(()=>JSON.parse('{\"a\":1}',function(){return arguments.length}))", "T(()=>JSON.parse('{\"a\":1}',function(){return Object.keys(arguments[2]).length}))",
  "T(()=>JSON.parse.length)", "T(()=>JSON.parse.name)", "T(()=>JSON.stringify.length)", "T(()=>JSON.stringify.name)", "T(()=>JSON.rawJSON.length)", "T(()=>JSON.isRawJSON.length)",
  "T(()=>JSON[Symbol.toStringTag])", "T(()=>Object.prototype.toString.call(JSON))", "T(()=>Reflect.ownKeys(JSON).map(String).join())", "T(()=>typeof JSON.parse(\"1\",Proxy))",
  "T(()=>JSON.parse('[1]',new Proxy(function(k,v){return v},{})))", "T(()=>JSON.parse('[1]',new Proxy({},{})))",
  "T(()=>JSON.parse({toString(){return '[1]'}}))", "T(()=>JSON.parse({toString(){throw new EvalError('t')}}))", "T(()=>JSON.parse({valueOf(){return '1'},toString(){return '2'}}))",
  "T(()=>JSON.parse(1))", "T(()=>JSON.parse(null))", "T(()=>JSON.parse(undefined))", "T(()=>JSON.parse())", "T(()=>JSON.parse(true))", "T(()=>JSON.parse([]))", "T(()=>JSON.parse([1]))", "T(()=>JSON.parse([[1]]))",
  "T(()=>JSON.parse({}))", "T(()=>JSON.parse(Symbol()))", "T(()=>JSON.parse(1n))", "T(()=>JSON.parse(new String('[1]')))", "T(()=>JSON.parse(new Number(5)))", "T(()=>JSON.parse(function(){}))",
  "T(()=>JSON.parse(NaN))", "T(()=>JSON.parse(-0))", "T(()=>JSON.parse(Infinity))", "T(()=>JSON.parse('\"'+'a'.repeat(100000)+'\"').length)", "T(()=>JSON.parse('['+'1,'.repeat(50000)+'1]').length)",
);

// ---- 9. Stringify: tipos exóticos em contextos.
const values = [
  "new Proxy({a:1},{})", "new Proxy([1,2],{})", "new Proxy(function(){},{})", "new Proxy({a:1},{ownKeys(){return ['b','a']},getOwnPropertyDescriptor(t,k){return {value:k,enumerable:true,configurable:true}},get(t,k){return k}})",
  "new Proxy({a:1},{get(t,k){if(k==='a')throw new TypeError('g');return t[k]}})", "new Proxy({a:1},{ownKeys(){throw new RangeError('k')}})", "(function(){var r=Proxy.revocable({},{});r.revoke();return r.proxy})()",
  "(function(){var r=Proxy.revocable([],{});r.revoke();return r.proxy})()", "new Proxy([],{get(t,k){return k==='length'?2:k}})", "new Proxy({toJSON(){return 7}},{})", "new Proxy({a:1},{getOwnPropertyDescriptor(){return undefined}})",
  "new Uint8Array([1,2])", "new Uint8ClampedArray([300,-5])", "new Int16Array([-1,2])", "new Float32Array([1.5,NaN,Infinity])", "new Float64Array([-0,0.1])", "new BigInt64Array([1n])", "new Uint8Array(0)", "new Uint8Array(new ArrayBuffer(4),1,2)",
  "new ArrayBuffer(2)", "new SharedArrayBuffer(2)", "new DataView(new ArrayBuffer(2))", "new Map([[1,2]])", "new Set([1,2])", "new WeakMap()", "new WeakSet()", "new Map()", "new Set()",
  "new Date(NaN)", "new Date(0)", "new Date(8.64e15)", "new Date(-1)", "new Date(2020,0,1,12)", "Object.assign(new Date(0),{toJSON:null})", "Object.assign(new Date(0),{toJSON(){return 'x'}})", "Object.assign(new Date(NaN),{toJSON(){return 'inv'}})",
  "(function(){var d=new Date(0);d.toISOString=function(){return 'iso'};return d})()", "(function(){var d=new Date(NaN);d.toISOString=function(){throw new Error('i')};return d})()",
  "new Number(1)", "new Number(NaN)", "new Number(-0)", "new Number(Infinity)", "new String('a')", "new String('')", "new Boolean(false)", "new Boolean(true)", "Object(Symbol('s'))", "Object(1n)", "Object(Symbol.iterator)",
  "Object.assign(new Number(1),{valueOf(){return 9}})", "Object.assign(new String('a'),{toString(){return 'b'}})", "Object.assign(new Number(1),{valueOf(){throw new Error('v')}})", "Object.assign(new String('a'),{toString(){throw new Error('s')}})",
  "Object.assign(new Boolean(false),{valueOf(){return true}})", "(function(){var n=new Number(1);Object.setPrototypeOf(n,null);return n})()", "(function(){var s=new String('ab');Object.setPrototypeOf(s,null);return s})()",
  "Symbol('s')", "Symbol.for('f')", "Symbol.iterator", "{[Symbol('k')]:1}", "{a:Symbol('v')}", "[Symbol('v')]", "{[Symbol('k')]:1,a:2}", "{a:1,[Symbol.for('z')]:{b:2}}", "(function(){var o={};o[Symbol()]=1;return o})()",
  "{get a(){throw new RangeError('boom')}}", "{get a(){return 1},get b(){return undefined}}", "{get a(){return {toJSON(){throw new SyntaxError('j')}}}}", "{a:1,get b(){delete this.c;return 2},c:3}", "{a:1,get b(){this.z=1;return 2}}",
  "{toJSON(){throw new URIError('u')}}", "{toJSON(){return undefined}}", "{toJSON(){return Symbol()}}", "{toJSON(){return function(){}}}", "{toJSON(){return 1n}}", "{toJSON(){return {toJSON(){return 'inner'}}}}", "{toJSON(){return [this]}}",
  "{toJSON:1}", "{toJSON:null}", "{toJSON:'s'}", "{toJSON(k){return 'k='+S(k)}}", "{toJSON(...a){return a.length}}", "{toJSON(){return this===undefined?'u':typeof this}}",
  "function(){}", "()=>1", "class A{}", "async function f(){}", "function*g(){}", "Math.sin", "Symbol", "(function*(){})()", "Promise.resolve(1)", "/re/g", "new RegExp('a','gi')", "new Error('e')", "Object.assign(new Error('e'),{extra:1})", "new TypeError('t')", "(function(){return arguments})(1,2)",
  "Infinity", "-Infinity", "NaN", "-0", "0", "1e21", "1e-7", "123456789012345680000", "0.1", "undefined", "null", "true", "''", "'a'", "'\"'", "'\\\\'", "'\\u2028\\u2029'", "'\\u007f'", "'\\u0000'", "'\\ud800'", "'\\udc00'", "'\\ud83d\\ude00'", "'\\ud83d'", "'\\ude00\\ud83d'",
  "[undefined]", "[,1]", "[1,,2]", "[function(){}]", "[Symbol()]", "[NaN,Infinity,-0]", "(function(){var a=[1,2];a.x=3;return a})()", "(function(){var a=[];a[3]=1;return a})()", "(function(){var a=[1];a.length=3;return a})()", "Object.assign([1,2],{toJSON(){return 'arr'}})",
  "Object.create({a:1})", "Object.create(null)", "Object.assign(Object.create(null),{a:1})", "Object.create({toJSON(){return 'inh'}})", "Object.create({get a(){return 1}})", "Object.defineProperty({},'a',{value:1})", "Object.defineProperty({},'a',{value:1,enumerable:true})", "Object.defineProperty({b:2},'a',{get(){return 1},enumerable:true})",
  "Object.freeze({a:1})", "Object.freeze([1])", "globalThis.JSON", "Math", "Reflect", "Atomics", "new (class A{x=1;y=[2]})", "new (class A{toJSON(){return 'cls'}})", "new (class A extends Array{})", "new (class A extends Map{})", "new (class A extends Date{})(0)", "new (class A extends Number{})(3)", "new (class A extends String{})('s')",
  "Object(1)", "Object('x')", "Object(true)", "Object(null)", "{a:[{b:[{c:1}]}]}", "{'1':1,'0':2,b:3,a:4}", "{a:undefined,b:function(){},c:Symbol()}", "{a:NaN,b:Infinity,c:-Infinity,d:-0}", "{'':1}", "{' ':[ ]}", "{'\"':1,'\\\\':2,'\\n':3}",
];
const ctxs = [
  v => `JSON.stringify(${v})`,
  v => `JSON.stringify([${v}])`,
  v => `JSON.stringify({a:${v}})`,
  v => `JSON.stringify({a:${v}},null,2)`,
  v => `JSON.stringify(${v},null,"\\t")`,
  v => `JSON.stringify({a:${v}},(k,v)=>typeof v==="number"?v+1:v)`,
  v => `JSON.stringify([${v}],["a"])`,
  v => `JSON.stringify(${v},(k,v)=>k===""?v:undefined)`,
];
for (const v of values) for (const c of ctxs) add(`T(()=>${c(v)})`);
// Símbolo como chave e como valor, com replacer retornando símbolos.
add(
  "T(()=>JSON.stringify({a:1},(k,v)=>k===\"a\"?Symbol():v))", "T(()=>JSON.stringify([1],(k,v)=>k===\"0\"?Symbol():v))", "T(()=>JSON.stringify(1,(k,v)=>Symbol()))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"\"?Symbol():v))",
  "T(()=>JSON.stringify({a:1},(k,v)=>k===\"\"?function(){}:v))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"a\"?function(){}:v))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"a\"?undefined:v))", "T(()=>JSON.stringify([1],(k,v)=>k===\"0\"?undefined:v))",
  "T(()=>JSON.stringify({[Symbol('a')]:1},[Symbol('a')]))", "T(()=>JSON.stringify({a:1,b:2},[Symbol('a'),'a']))", "T(()=>JSON.stringify({a:1,b:2},['b','a','b']))", "T(()=>JSON.stringify({1:1,a:2},[1,'a']))", "T(()=>JSON.stringify({1:1,a:2},[new Number(1),new String('a')]))",
  "T(()=>JSON.stringify({1:1,a:2},[{},[],null,undefined,true]))", "T(()=>JSON.stringify({a:{a:1,b:2},b:3},['a']))", "T(()=>JSON.stringify([{a:1,b:2}],['a']))", "T(()=>JSON.stringify({a:1},[]))", "T(()=>JSON.stringify({a:1},['a','a']))",
  "T(()=>JSON.stringify({'1.0':1,1:2},[1.0,'1.0']))", "T(()=>JSON.stringify({a:1},{0:'a',length:1}))", "T(()=>JSON.stringify({a:1},new Proxy(['a'],{})))", "T(()=>JSON.stringify({a:1},new Proxy(function(k,v){return v},{})))",
  "T(()=>JSON.stringify({a:1},{}))", "T(()=>JSON.stringify({a:1},1))", "T(()=>JSON.stringify({a:1},'a'))", "T(()=>JSON.stringify({a:1},true))", "T(()=>JSON.stringify({a:1},Symbol()))", "T(()=>JSON.stringify({a:1},null))",
  "T(()=>JSON.stringify({a:1},[{toString(){return 'a'}}]))", "T(()=>JSON.stringify({a:1},[Object('a')]))", "T(()=>JSON.stringify({a:1},[{valueOf(){return 1}}]))", "T(()=>JSON.stringify({a:1},[1n]))", "T(()=>JSON.stringify({a:1,1:2},[1n]))",
  "T(()=>{var log=[];JSON.stringify({a:1,b:[2,{c:3}]},function(k,v){log.push(S(k)+':'+typeof this+':'+Array.isArray(this));return v});return log.join()})",
  "T(()=>{var log=[];JSON.stringify({a:{toJSON(k){log.push('tj '+S(k));return 1}}},function(k,v){log.push('r '+S(k)+'='+S(v));return v});return log.join()})",
  "T(()=>{var log=[];JSON.stringify([{toJSON(k){log.push('tj '+S(k));return 1}}],(k,v)=>{log.push('r '+S(k));return v});return log.join()})",
  "T(()=>{var log=[];JSON.stringify({b:1,a:2},(k,v)=>{log.push(k);return v});return log.join()})", "T(()=>{var log=[];JSON.stringify({2:1,1:2,b:3,a:4},(k,v)=>{log.push(k);return v});return log.join()})",
  "T(()=>{var h;JSON.stringify(1,function(k,v){h=this;return v});return S(h)+Object.getPrototypeOf(h)===Object.prototype})", "T(()=>{var h;JSON.stringify(1,function(k,v){h=this;return v});return Reflect.ownKeys(h).join()})",
  "T(()=>JSON.stringify({a:1},(k,v)=>k===\"\"?{b:v}:v))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"\"?[v]:v))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"a\"?{b:2}:v))", "T(()=>JSON.stringify({a:1},(k,v)=>typeof v===\"object\"?{...v,z:1}:v))",
  "T(()=>JSON.stringify({a:1},(k,v)=>{throw new RangeError('r')}))", "T(()=>JSON.stringify({a:1},(k,v)=>k===\"a\"?{toJSON(){return 'tj'}}:v))", "T(()=>JSON.stringify({a:new Date(0)},(k,v)=>typeof v))",
  "T(()=>JSON.stringify({a:new Date(0)},function(k,v){return typeof this[k]+':'+typeof v}))", "T(()=>JSON.stringify(new Date(0),function(k,v){return typeof this[k]+':'+typeof v}))",
);

// ---- 10. space.
const spaces = ["0", "1", "2", "9", "10", "11", "12", "100", "-1", "-0", "0.5", "1.9", "2.5", "10.9", "11.5", "NaN", "Infinity", "-Infinity", "1e21", "'abc'", "'abcdefghijklmnop'", "''", "' '", "'\\t'", "'\\n'", "'\\u2028'", "'\\ud800'", "'\\ud83d\\ude00'", "'a\\ud83d\\ude00b'", "'1234567890x'", "'12345678901'", "'é'", "new Number(2)", "new String('xy')", "new Number(NaN)", "new Boolean(true)", "true", "false", "null", "undefined", "{}", "[]", "Symbol()", "1n", "{toString(){return 'x'}}", "{valueOf(){return 3}}", "Object.assign(new Number(2),{valueOf(){return 4}})", "Object.assign(new String('z'),{toString(){return 'w'}})", "()=>1", "'\\u0000'", "'\"'", "'\\\\'"];
const spaceTargets = ["{a:[1,{b:2}],c:'x'}", "[]", "{}", "[[]]", "[1,[2]]", "{a:{}}", "{a:[]}", "[{}]", "[null,undefined,function(){}]", "{a:1,b:undefined}", "'s'", "1"];
for (const sp of spaces) for (const t of spaceTargets) add(`T(()=>JSON.stringify(${t},null,${sp}))`);
add(
  "T(()=>JSON.stringify({a:1},null,{valueOf(){throw new Error('sv')}}))", "T(()=>JSON.stringify({a:1},null,{toString(){throw new Error('ss')}}))", "T(()=>JSON.stringify({a:1},null,Object.assign(new Number(1),{valueOf(){throw new Error('nv')}})))",
  "T(()=>JSON.stringify({a:1},null,Object.assign(new String('a'),{toString(){throw new Error('ns')}})))", "T(()=>JSON.stringify({a:{b:1}},null,Object.create(null)))",
);

// ---- 11. Ciclos e a mensagem completa.
const cycles = [
  "var a={};a.a=a;JSON.stringify(a)", "var a=[];a[0]=a;JSON.stringify(a)", "var a={b:{}};a.b.c=a;JSON.stringify(a)", "var a={b:{c:{d:{}}}};a.b.c.d.e=a;JSON.stringify(a)", "var a={b:{c:{}}};a.b.c.d=a.b;JSON.stringify(a)",
  "var a={x:[{}]};a.x[0].y=a;JSON.stringify(a)", "var a=[{}];a[0].z=a;JSON.stringify(a)", "var a=[[[]]];a[0][0][0]=a[0];JSON.stringify(a)", "var a={b:[{c:{d:[]}}]};a.b[0].c.d[0]=a;JSON.stringify(a)",
  "var a={};var b={a:a};a.b=b;JSON.stringify(b)", "var a={};var b={a:a};a.b=b;JSON.stringify(a)", "var a={};var b={};a.b=b;b.a=a;JSON.stringify([a])", "var a={};var b={};a.b=b;b.a=a;JSON.stringify({k:[a]})",
  "var a={};a.self=a;JSON.stringify({x:a})", "var a={};a.self=a;JSON.stringify([a,a])", "var a={};JSON.stringify([a,a])", "var a={};JSON.stringify({p:a,q:a})", "var a={};JSON.stringify([[a],[a]])",
  "var a={toJSON(){return a}};JSON.stringify(a)", "var a={toJSON(){return {x:a}}};JSON.stringify(a)", "var a={};a.toJSON=function(){return [this]};JSON.stringify(a)", "var a={x:{toJSON(){return a}}};JSON.stringify(a)",
  "var a={};JSON.stringify(a,(k,v)=>k==='x'?a:v)", "var a={x:1};JSON.stringify(a,(k,v)=>k==='x'?a:v)", "var a={x:1};JSON.stringify(a,(k,v)=>k==='x'?[a]:v)", "var a=[1];JSON.stringify(a,(k,v)=>k==='0'?a:v)", "JSON.stringify({x:1},function(k,v){return k==='x'?this:v})",
  "var a={get g(){return a}};JSON.stringify(a)", "var a={get g(){return {h:a}}};JSON.stringify(a)", "var a={b:{get c(){return a.b}}};JSON.stringify(a)",
  "var a=new Proxy({},{get(t,k){return k==='p'?a:undefined},ownKeys(){return ['p']},getOwnPropertyDescriptor(){return {value:a,enumerable:true,configurable:true}}});JSON.stringify(a)", "var t={};var a=new Proxy(t,{});t.p=a;JSON.stringify(a)", "var t={};var a=new Proxy(t,{});t.p=t;JSON.stringify(a)", "var t=[];var a=new Proxy(t,{});t[0]=a;JSON.stringify(a)",
  "var a={};a.n=new Number(1);a.n.c=a;JSON.stringify(a)", "var a={};a.s=new String('x');a.s.c=a;JSON.stringify(a)", "var a=Object.create({inh:null});a.inh=a;JSON.stringify(a)", "var p={};var a=Object.create(p);p.c=a;JSON.stringify(a)",
  "var a={};a.b={};a.b.c=a.b;JSON.stringify(a,null,2)", "var a=[];a.push(a);JSON.stringify(a,null,'\\t')", "var a={};a.a=a;JSON.stringify(a,['a'])", "var a={};a.a=a;JSON.stringify(a,null,10)", "var a={};a.a=a;JSON.stringify(a,(k,v)=>v)",
  "var a={m:new Map()};a.m.set(1,a);JSON.stringify(a)", "var a={s:new Set()};a.s.add(a);JSON.stringify(a)", "var a=new Date(0);a.c=a;JSON.stringify(a)", "var a=[1,2];a.x=a;JSON.stringify(a)", "var a={};a[0]=a;JSON.stringify(a)", "var a={};a['']=a;JSON.stringify(a)", "var a={};a['with space']=a;JSON.stringify(a)",
  "var a={};a['\"q\"']=a;JSON.stringify(a)", "var a={};a['\\n']=a;JSON.stringify(a)", "var a={};a['é']=a;JSON.stringify(a)", "var a={};a['\\ud800']=a;JSON.stringify(a)", "var a={};a[Symbol('s')]=a;a.k=a;JSON.stringify(a)",
  "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return [e.name,e.constructor===TypeError,e.message.split('\\n').length,e.message.length,Object.keys(e).join()].join('|')}",
  "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return JSON.stringify(e.message)}", "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return e instanceof TypeError&&String(e)}", "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return typeof e.stack}",
  "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return Object.getOwnPropertyNames(e).join()}", "var a={};a.k=a;try{JSON.stringify(a)}catch(e){return e.cause===undefined}",
  "var a={};a.k=a;try{JSON.stringify(a)}catch(e){try{JSON.stringify(a)}catch(f){return e.message===f.message&&e!==f}}",
];
for (const c of cycles) add(/\breturn\b/.test(c) ? `T(()=>{${c}})` : `T(()=>{${c.replace(/;([^;]*)$/, ";return $1")}})`);

// ---- 12. BigInt.
const bigs = ["1n", "0n", "-1n", "2n**64n", "BigInt(Number.MAX_SAFE_INTEGER)", "Object(1n)", "Object(0n)", "-(2n**70n)"];
const bigCtx = [v => v, v => `[${v}]`, v => `{a:${v}}`, v => `{a:[{b:${v}}]}`, v => `{toJSON(){return ${v}}}`, v => `{a:{toJSON(){return ${v}}}}`, v => `[{x:{y:${v}}}]`];
for (const b of bigs) for (const c of bigCtx) {
  add(`T(()=>JSON.stringify(${c(b)}))`);
  add(`T(()=>JSON.stringify(${c(b)},null,2))`);
  add(`T(()=>JSON.stringify(${c(b)},(k,v)=>typeof v==="bigint"?String(v):v))`);
}
add(
  "T(()=>{BigInt.prototype.toJSON=function(){return String(this)+'n'};try{return JSON.stringify({a:1n,b:[2n]})}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>{BigInt.prototype.toJSON=function(){return typeof this};try{return JSON.stringify([1n,Object(2n)])}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>{BigInt.prototype.toJSON=function(k){return k};try{return JSON.stringify({a:1n,b:[1n]})}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>{Object.defineProperty(BigInt.prototype,'toJSON',{get(){throw new Error('g')},configurable:true});try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>{BigInt.prototype.toJSON=1;try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>{BigInt.prototype.toJSON=function(){return this};try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})",
  "T(()=>JSON.stringify({a:1n},['a']))", "T(()=>JSON.stringify({a:1n},[]))", "T(()=>JSON.stringify({b:1,a:1n},['b']))", "T(()=>JSON.stringify({a:1n},(k,v)=>k==='a'?undefined:v))", "T(()=>JSON.stringify({a:1},(k,v)=>k==='a'?2n:v))",
  "T(()=>JSON.stringify(1,(k,v)=>2n))", "T(()=>JSON.stringify(1,(k,v)=>Object(2n)))", "T(()=>JSON.stringify([],(k,v)=>2n))", "T(()=>JSON.stringify({},null,1n))", "T(()=>JSON.stringify([1n,NaN],null,1))",
  "T(()=>JSON.stringify({a:1n,get b(){throw new Error('b')}}))", "T(()=>JSON.stringify({get b(){throw new Error('b')},a:1n}))", "T(()=>JSON.rawJSON(1n).rawJSON)", "T(()=>JSON.stringify(JSON.rawJSON(1n)))",
  "T(()=>JSON.parse('1',(k,v,c)=>typeof v))", "T(()=>JSON.parse('12345678901234567890',(k,v,c)=>BigInt(c.source)))", "T(()=>JSON.stringify(JSON.parse('12345678901234567890',(k,v,c)=>JSON.rawJSON(c.source))))",
);

// ---- 13. Surrogates isolados e well-formed stringify.
const lone = ["\\ud800", "\\udbff", "\\udc00", "\\udfff", "\\udc00\\ud800", "\\ud800\\ud800", "\\udc00\\udc00", "\\ud800\\ud83d\\ude00", "\\ud83d\\ude00\\udc00", "\\ud83d\\ude00", "a\\ud800b", "a\\udc00b", "\\ud800\\u0041", "\\u0041\\udc00", "\\ud7ff", "\\ue000", "\\ud800\\udbff\\udc00"];
const loneCtx = [s => `'${s}'`, s => `['${s}']`, s => `{a:'${s}'}`, s => `{'${s}':1}`, s => `{'${s}':'${s}'}`, s => `new String('${s}')`, s => `{toJSON(){return '${s}'}}`, s => `Object.assign(Object('${s}'),{})`];
for (const s of lone) for (const c of loneCtx) {
  add(`T(()=>JSON.stringify(${c(s)}))`);
  add(`T(()=>JSON.stringify(${c(s)}).length)`);
}
for (const s of lone) {
  add(`T(()=>JSON.stringify('${s}',null,'${s}'))`);
  add(`T(()=>JSON.stringify([1],null,'${s}'))`);
  add(`T(()=>JSON.stringify({'${s}':1},['${s}']))`);
  add(`T(()=>JSON.parse(JSON.stringify('${s}'))==='${s}')`);
  add(`T(()=>JSON.parse('"'+JSON.stringify('${s}').slice(1,-1)+'"').length)`);
  add(`T(()=>Array.from(JSON.stringify('${s}'),c=>c.charCodeAt(0).toString(16)).join())`);
  add(`T(()=>JSON.stringify('x${s}y').split('').map(c=>c.charCodeAt(0).toString(16)).join())`);
  add(`T(()=>Object.keys(JSON.parse('{"${s}":1}')).map(k=>k.length))`);
  add(`T(()=>Object.keys(JSON.parse(JSON.stringify({'${s}':1}))).map(k=>k.length))`);
}
const rawLone = ["\ud800", "\udc00", "a\ud800", "\ud800a", "\udc00\ud800", "\ud83d\ude00", "\ud800\ud800"];
for (const s of rawLone) {
  add(`T(()=>JSON.stringify(${q(s)}))`);
  add(`T(()=>JSON.stringify(${q(s)}).length)`);
  add(`T(()=>JSON.parse(${q('"' + s + '"')}).length)`);
  add(`T(()=>JSON.stringify({${q(s)}:${q(s)}}))`);
  add(`T(()=>JSON.stringify([${q(s)}],null,${q(s)}))`);
}
for (let c = 0; c < 0x20; c++) add(`T(()=>JSON.stringify(String.fromCharCode(${c})))`);
for (const c of [0x7f, 0x80, 0x9f, 0xa0, 0xad, 0x2028, 0x2029, 0xfeff, 0xfffe, 0xffff, 0x22, 0x5c, 0x2f, 0xd7ff, 0xe000]) {
  add(`T(()=>JSON.stringify(String.fromCharCode(${c})))`);
  add(`T(()=>JSON.stringify({[String.fromCharCode(${c})]:1}))`);
}

// ---- 14. Ordem de chaves com inteiros.
const orderSets = [
  "{b:1,a:2,1:3,0:4}", "{'-1':1,'1.5':2,'01':3,'1':4,'4294967294':5,'4294967295':6,'4294967296':7}", "{[Symbol('s')]:1,x:2,3:3,[Symbol.iterator]:4,1:5}", "{z:1,y:2,10:1,9:1}", "{'2':1,'1':1,a:1,'0':1,length:1}",
  "{'':1,' ':2,'0':3}", "{9007199254740991:1,9007199254740992:2,1e21:3,1e-7:4}", "{'0x10':1,'16':2,'1e3':3,'1000':4,'Infinity':5,'NaN':6,'-0':7,'0':8}", "{'4294967295':1,'4294967294':2,'2147483648':3,'2147483647':4}",
  "{100:1,20:2,3:3,a:4,b:5,'+1':6,'1 ':7,' 1':8}", "{1:1,'1':2}", "{0:1,'00':2,'0':3}", "{'-0':1,0:2}", "{5:1,4:2,3:3,2:4,1:5,0:6}", "{b:1,2:1,a:1,1:1,c:1,0:1}",
];
for (const s of orderSets) {
  add(
    `T(()=>JSON.stringify(${s}))`, `T(()=>JSON.stringify(JSON.parse(JSON.stringify(${s}))))`, `T(()=>JSON.stringify(${s},null,1))`, `T(()=>JSON.stringify(${s},(k,v)=>v))`, `T(()=>Object.keys(JSON.parse(JSON.stringify(${s}))).join())`,
    `T(()=>{var r=[];JSON.parse(JSON.stringify(${s}),function(k,v){r.push(k);return v});return r.join()})`, `T(()=>{var r=[];JSON.stringify(${s},function(k,v){r.push(k);return v});return r.join()})`,
    `T(()=>JSON.stringify(${s},Object.keys(${s}).reverse()))`, `T(()=>JSON.stringify([${s}]))`, `T(()=>JSON.stringify(Object.assign({},${s}),null,'-'))`,
  );
}
add(
  "T(()=>JSON.stringify({b:1,a:2},['a','b']))", "T(()=>JSON.stringify({b:1,a:2,0:3},['a',0,'b']))", "T(()=>JSON.stringify({b:1,a:2,0:3},[0,'a']))", "T(()=>JSON.stringify([{a:1,b:2,c:3}],['c','a']))",
  "T(()=>JSON.stringify({a:{c:1,b:2}},['a','b','c']))", "T(()=>JSON.stringify({a:{c:1,b:2}},['a','c']))", "T(()=>JSON.parse('{\"b\":1,\"a\":2,\"1\":3}',function(k,v){return typeof v==='number'?v*2:v}))",
  "T(()=>{var r=[];JSON.parse('{\"b\":{\"d\":1,\"c\":2},\"a\":[1,2]}',function(k,v){r.push(k);return v});return r.join()})", "T(()=>{var r=[];JSON.parse('[[1,2],[3,[4]]]',function(k,v){r.push(k);return v});return r.join()})",
  "T(()=>{var r=[];JSON.parse('{\"2\":1,\"b\":2,\"1\":3,\"a\":4}',function(k,v){r.push(k);return v});return r.join()})",
);

// ---- 15. JSON.rawJSON e isRawJSON.
const raws = ["1", "-1", "0", "-0", "1.5", "1e3", "1E-3", "123456789012345678901234567890", "0.1", "\"a\"", "\"\"", "\"\\u0041\"", "\"\\n\"", "\"é\"", "\"\ud83d\ude00\"", "\"\\ud800\"", "null", "true", "false",
  "{}", "[]", "[1]", "{\"a\":1}", "", " ", " 1", "1 ", "\t1", "1\t", "\n1", "1\n", "\r1", "1\r", "\"a", "a\"", "'a'", "a", "undefined", "NaN", "Infinity", "-Infinity", "+1", "01", "1.", ".5", "1e", "0x1", "1n", "--1", "- 1", "tru", "nulll", "True", "\u00a01", "\ufeff1", "/**/1", "1/**/", "\"\u0000\"", "\"\n\"", "\"\\x\"", "-", "[", "{", "}", "]", ",", ":", "1,2", "1 2", "\"a\"\"b\"", "\"a\" ", " \"a\""];
for (const r of raws) {
  const lit = q(r);
  add(
    `T(()=>JSON.rawJSON(${lit}).rawJSON)`, `T(()=>JSON.stringify(JSON.rawJSON(${lit})))`, `T(()=>JSON.stringify({a:JSON.rawJSON(${lit})}))`, `T(()=>JSON.stringify([JSON.rawJSON(${lit})],null,2))`,
    `T(()=>JSON.isRawJSON(JSON.rawJSON(${lit})))`, `T(()=>Object.getOwnPropertyNames(JSON.rawJSON(${lit})).join()+Object.isFrozen(JSON.rawJSON(${lit}))+Object.getPrototypeOf(JSON.rawJSON(${lit})))`,
  );
}
add(
  "T(()=>JSON.rawJSON())", "T(()=>JSON.rawJSON(undefined))", "T(()=>JSON.rawJSON(null).rawJSON)", "T(()=>JSON.rawJSON(true).rawJSON)", "T(()=>JSON.rawJSON(1).rawJSON)", "T(()=>JSON.rawJSON(-0).rawJSON)", "T(()=>JSON.rawJSON(NaN))", "T(()=>JSON.rawJSON(Infinity))",
  "T(()=>JSON.rawJSON(1.5e300).rawJSON)", "T(()=>JSON.rawJSON({}))", "T(()=>JSON.rawJSON([]))", "T(()=>JSON.rawJSON([1]))", "T(()=>JSON.rawJSON(Symbol()))", "T(()=>JSON.rawJSON({toString(){return '5'}}).rawJSON)", "T(()=>JSON.rawJSON({toString(){throw new Error('t')}}))",
  "T(()=>JSON.rawJSON(new String('1')).rawJSON)", "T(()=>JSON.rawJSON(new Number(1)).rawJSON)", "T(()=>new JSON.rawJSON('1'))", "T(()=>JSON.rawJSON.call(null,'1').rawJSON)", "T(()=>JSON.rawJSON.prototype)",
  "T(()=>JSON.isRawJSON())", "T(()=>JSON.isRawJSON(undefined))", "T(()=>JSON.isRawJSON(null))", "T(()=>JSON.isRawJSON({rawJSON:'1'}))", "T(()=>JSON.isRawJSON(Object.freeze({rawJSON:'1'})))", "T(()=>JSON.isRawJSON(Object.create(null)))", "T(()=>JSON.isRawJSON(1))", "T(()=>JSON.isRawJSON('1'))",
  "T(()=>JSON.isRawJSON(JSON))", "T(()=>JSON.isRawJSON(new Proxy(JSON.rawJSON('1'),{})))", "T(()=>JSON.isRawJSON(Object.create(JSON.rawJSON('1'))))", "T(()=>JSON.isRawJSON(Object.assign({},JSON.rawJSON('1'))))", "T(()=>JSON.isRawJSON(structuredClone))",
  "T(()=>{var r=JSON.rawJSON('1');r.x=2;return Object.keys(r).join()})", "T(()=>{'use strict';var r=JSON.rawJSON('1');r.x=2})", "T(()=>{'use strict';var r=JSON.rawJSON('1');r.rawJSON='2'})", "T(()=>{'use strict';var r=JSON.rawJSON('1');delete r.rawJSON})", "T(()=>{'use strict';var r=JSON.rawJSON('1');Object.setPrototypeOf(r,{})})",
  "T(()=>JSON.stringify(JSON.rawJSON('1'),null,5))", "T(()=>JSON.stringify([JSON.rawJSON('1'),JSON.rawJSON('\"s\"')],null,'\\t'))", "T(()=>JSON.stringify({a:JSON.rawJSON('1'),b:JSON.rawJSON('null')},['a']))", "T(()=>JSON.stringify({a:JSON.rawJSON('1')},(k,v)=>k==='a'?JSON.isRawJSON(v):v))",
  "T(()=>JSON.stringify({a:1},(k,v)=>k==='a'?JSON.rawJSON('99999999999999999999'):v))", "T(()=>JSON.stringify({a:1},(k,v)=>k===''?JSON.rawJSON('[1]'):v))", "T(()=>JSON.stringify(1,(k,v)=>JSON.rawJSON('2')))", "T(()=>JSON.stringify({toJSON(){return JSON.rawJSON('3')}}))",
  "T(()=>JSON.stringify({a:{toJSON(){return JSON.rawJSON('\"t\"')}}}))", "T(()=>JSON.stringify(Object.assign(JSON.rawJSON('1'),{})))", "T(()=>JSON.stringify(new Proxy(JSON.rawJSON('1'),{})))", "T(()=>JSON.stringify({a:Object.create(JSON.rawJSON('1'))}))",
  "T(()=>JSON.stringify({a:{rawJSON:'1'}}))", "T(()=>JSON.stringify(Object.freeze({rawJSON:'1'})))", "T(()=>JSON.stringify(JSON.rawJSON('1'),(k,v)=>typeof v))", "T(()=>{var r=JSON.rawJSON('1');return JSON.stringify([r,r,{r}])})",
  "T(()=>JSON.parse(JSON.stringify({a:JSON.rawJSON('12345678901234567890')})).a)", "T(()=>JSON.stringify(JSON.parse('{\"a\":12345678901234567890}',(k,v,c)=>typeof v==='number'?JSON.rawJSON(c.source):v)))",
  "T(()=>JSON.stringify(JSON.parse('[1.0,2.50,1e2,-0]',(k,v,c)=>typeof v==='number'?JSON.rawJSON(c.source):v)))", "T(()=>JSON.stringify(JSON.parse('{\"a\":1.10,\"b\":[0.0]}',(k,v,c)=>typeof v==='number'?JSON.rawJSON(c.source):v),null,1))",
  "T(()=>{var o={};Object.defineProperty(o,'rawJSON',{value:'1'});return JSON.isRawJSON(o)})", "T(()=>Object.getOwnPropertyDescriptor(JSON.rawJSON('1'),'rawJSON'))", "T(()=>JSON.stringify(Object.getOwnPropertyDescriptor(JSON.rawJSON('1'),'rawJSON')))",
  "T(()=>Object.prototype.toString.call(JSON.rawJSON('1')))", "T(()=>typeof JSON.rawJSON('1'))", "T(()=>String(JSON.rawJSON('1')))", "T(()=>JSON.rawJSON('1')+'')", "T(()=>Object.keys(JSON.rawJSON('1')).join())", "T(()=>Reflect.ownKeys(JSON.rawJSON('1')).length)",
  "T(()=>JSON.rawJSON('1')===JSON.rawJSON('1'))", "T(()=>Object.isExtensible(JSON.rawJSON('1')))", "T(()=>Object.isSealed(JSON.rawJSON('1')))", "T(()=>Reflect.getPrototypeOf(JSON.rawJSON('1')))", "T(()=>JSON.rawJSON('1') instanceof Object)",
);

// ---- 16. toJSON e replacer em protótipos nativos.
add(
  "T(()=>{Number.prototype.toJSON=function(){return 'n'};try{return JSON.stringify([1,new Number(2),NaN])}finally{delete Number.prototype.toJSON}})",
  "T(()=>{String.prototype.toJSON=function(){return 's'};try{return JSON.stringify(['a',new String('b')])}finally{delete String.prototype.toJSON}})",
  "T(()=>{Boolean.prototype.toJSON=function(){return 'b'};try{return JSON.stringify([true,new Boolean(false)])}finally{delete Boolean.prototype.toJSON}})",
  "T(()=>{Symbol.prototype.toJSON=function(){return 'sym'};try{return JSON.stringify([Symbol(),{a:Symbol()}])}finally{delete Symbol.prototype.toJSON}})",
  "T(()=>{Object.prototype.toJSON=function(){return 'o'};try{return JSON.stringify([{},[],new Date(0),1,'a'])}finally{delete Object.prototype.toJSON}})",
  "T(()=>{Array.prototype.toJSON=function(){return 'arr'};try{return JSON.stringify({a:[1]})}finally{delete Array.prototype.toJSON}})",
  "T(()=>{Date.prototype.toJSON=function(){return 'd'};try{return JSON.stringify(new Date(NaN))}finally{delete Date.prototype.toJSON}})",
  "T(()=>{var o=Date.prototype.toJSON;Date.prototype.toJSON=undefined;try{return JSON.stringify(new Date(0))}finally{Date.prototype.toJSON=o}})",
  "T(()=>{var o=Date.prototype.toISOString;Date.prototype.toISOString=function(){return 'iso'};try{return JSON.stringify(new Date(0))}finally{Date.prototype.toISOString=o}})",
  "T(()=>{var o=Date.prototype.toISOString;Date.prototype.toISOString=undefined;try{return JSON.stringify(new Date(0))}finally{Date.prototype.toISOString=o}})",
  "T(()=>Date.prototype.toJSON.call({toISOString(){return 'x'}}))", "T(()=>Date.prototype.toJSON.call({toISOString:1}))", "T(()=>Date.prototype.toJSON.call({valueOf(){return NaN},toISOString(){return 'x'}}))", "T(()=>Date.prototype.toJSON.call({valueOf(){return Infinity},toISOString(){return 'x'}}))",
  "T(()=>Date.prototype.toJSON.call({valueOf(){return 1},toISOString(){return 'x'}}))", "T(()=>Date.prototype.toJSON.call(1))", "T(()=>Date.prototype.toJSON.call(null))", "T(()=>Date.prototype.toJSON.call('s'))", "T(()=>Date.prototype.toJSON.call({toISOString(){return 1}}))",
  "T(()=>Date.prototype.toJSON.call({toISOString(){return {}}}))", "T(()=>Date.prototype.toJSON.call({valueOf(){return 1},toISOString(){throw new Error('i')}}))", "T(()=>Date.prototype.toJSON.call(new Date(NaN)))", "T(()=>Date.prototype.toJSON.length)",
  "T(()=>JSON.stringify(Object.assign(()=>1,{toJSON(){return 'fnj'}})))", "T(()=>JSON.stringify({f:Object.assign(()=>1,{toJSON(){return 'fnj'}})}))", "T(()=>JSON.stringify(Object.assign(function(){},{a:1})))", "T(()=>JSON.stringify({f:Object.assign(function(){},{a:1})}))",
  "T(()=>JSON.stringify(new Proxy({},{get(t,k){return k==='toJSON'?()=>'pj':undefined}})))", "T(()=>JSON.stringify(new Proxy([],{get(t,k){return k==='toJSON'?()=>'pa':t[k]}})))", "T(()=>JSON.stringify({a:new Proxy({},{get(t,k){log=1;return undefined}})}))",
  "T(()=>JSON.stringify(new Proxy({},{has(){throw new Error('h')}})))", "T(()=>JSON.stringify(new Proxy({a:1},{getPrototypeOf(){throw new Error('gp')}})))", "T(()=>JSON.stringify(new Proxy([1],{getPrototypeOf(){throw new Error('gp')}})))",
  "T(()=>JSON.stringify(new Proxy([1],{get(t,k){if(k==='length')throw new Error('len');return t[k]}})))", "T(()=>JSON.stringify(new Proxy({},{ownKeys(){return [1]}})))", "T(()=>JSON.stringify(new Proxy({},{ownKeys(){return ['a','a']}})))",
  "T(()=>JSON.stringify(new Proxy({a:1},{ownKeys(){return []}})))", "T(()=>JSON.stringify(new Proxy(Object.freeze({a:1}),{ownKeys(){return []}})))", "T(()=>JSON.stringify(new Proxy({a:1},{getOwnPropertyDescriptor(t,k){return {value:1,enumerable:false,configurable:true}}})))",
  "T(()=>Array.isArray(new Proxy([],{})))", "T(()=>JSON.stringify(new Proxy(new Proxy([1,2],{}),{})))", "T(()=>JSON.stringify([new Proxy([],{})]))", "T(()=>JSON.stringify(new Proxy(new Date(0),{})))", "T(()=>JSON.stringify(new Proxy(new Number(1),{})))", "T(()=>JSON.stringify(new Proxy(new Map([[1,2]]),{})))",
);

const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));

// Dedup contra os três goldens de JSON: a expressão sozinha já aparecer num fonte existente conta como repetida.
const baseDir = path.join(__dirname, "..", "tests", "golden");
let baseText = "";
for (const program of knownPrograms("json_grid_bun.tsv", ["json_bun.tsv", "json_more_bun.tsv", "json_number_bun.tsv"])) baseText += program + "\n";

let kept = 0, dropped = 0, dup = 0;
const RESULT_PRELOAD_FILE = writeResultPreload();
for (const expr of unique) {
  if (baseText.includes(`globalThis.R = ${expr}`) || baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    // Processo fresco por programa: o JSC reifica a tabela estática de JSON, Symbol, Reflect etc. por ordem de acesso.
    // O resultado sai do preload em JSON (surrogate solitário vira \udXXX, sem perda no pipe UTF-8).
    const child = spawnSync(process.execPath, ["--preload", RESULT_PRELOAD_FILE, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + String(e).slice(0, 200) + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("json_grid", rows));
