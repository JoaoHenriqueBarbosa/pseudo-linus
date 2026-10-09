// Gera tests/golden/json_more_bun.tsv: complemento de borda de gen-json-golden.js, medido no bun 1.4.2.
// Matriz de erros de posição do JSON.parse (token ruim em cada ponto da gramática, gramática de números, escapes e
// caracteres de controle em strings, literais, espaços em branco, BOM, profundidade), reviver (arrays com buracos,
// deleção, `context.source`, holder), stringify (valores especiais em contextos que o golden base não cruza,
// replacer array e função com `this`, space, toJSON, ciclos, Proxy, wrappers, Map/Set, typed arrays, getters) e
// JSON.rawJSON/isRawJSON. Programas que já estão em json_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-golden.js.
// Uso: bun scripts/gen-json-more-golden.js > tests/golden/json_more_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");

// O golden tem datas locais: o fuso é fixo para o oráculo não depender da máquina (o teste Rust fixa o mesmo).
process.env.TZ = "America/Sao_Paulo";
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const parse = s => add(`JSON.parse(${q(s)})`);

// ---- 1. Token ruim em cada ponto da gramática.
const badTokens = ["}", "]", ",", ":", "'", "x", "+", ".", "/", "\\", "NaN", "undefined", "\u0001", "tru", "-", "\"", "0x1", "[", "{"];
const contexts = [
  ["", ""], ["[", ""], ["[1,", ""], ["[1", ""], ["[1", "]"], ["{", ""], ["{", "}"], ["{\"a\"", ""], ["{\"a\"", ":1}"], ["{\"a\":", ""],
  ["{\"a\":", "}"], ["{\"a\":1", ""], ["{\"a\":1,", ""], ["{\"a\":1,", "}"], ["{\"a\":[", ""], ["[{\"a\":1}", ""], ["[[", "]]"],
];
for (const [pre, post] of contexts) for (const bad of badTokens) parse(pre + bad + post);

// ---- 2. Gramática de números.
const numbers = [
  "-", "--1", "- 1", "-+1", "+-1", "-.5", "-.", "0.", "0.e1", "0.0.0", "1.2.3", "1e", "1e+", "1e-", "1e+-1", "1ee1", "1e1.5", "1E", "1E+", "1.E1",
  "01", "-01", "00.5", "007", "0x0", "0X1F", "0b1", "0o7", "1n", "1_000", "1,5", "1e5e5", "0e", "-0e", "-0.0e-0", "1.e", "1.0e+", ".e1", "e1", "E1",
  "١٢", "１", "1 ", "Infinity", "-Infinity", "+Infinity", "infinity", "NaN", "-NaN", "0.1e", "9".repeat(400), "-" + "9".repeat(400),
  "0." + "0".repeat(400) + "1", "1e-400", "1e400", "-1e400", "1e+400", "123456789e-400", "4.9e-324", "2.4e-324", "1.7976931348623157e308",
  "1.7976931348623159e308", "9007199254740992", "9007199254740993", "-9007199254740993", "0.30000000000000004", "1E+2", "1E-2", "1e0", "-0", "-0.0", "-0e1",
  "0e1", "0.0e-5", "1.0", "100", "1e2", "1.5e+3", "-1.5E-3",
];
for (const n of numbers) {
  parse(n);
  add(`JSON.parse(${q("[" + n + "]")})`, `JSON.parse(${q("{\"a\":" + n + "}")})`, `JSON.parse(${q("[" + n + ",1]")})`, `JSON.parse(${q(" " + n + " ")})`);
}
for (const n of ["1e999", "-0", "1E+2", "-0.0", "0e0", "1e-999", "0.1", "5e-324", "1.7976931348623157e308", "9007199254740993", "100e-2", "-1E-2"]) {
  add(`T(()=>Object.is(JSON.parse(${q(n)}),${n}))`, `T(()=>1/JSON.parse(${q(n)}))`, `T(()=>JSON.stringify(JSON.parse(${q("[" + n + "]")})))`);
}

// ---- 3. Strings: escapes e caracteres de controle.
for (let c = 0; c < 128; c++) {
  const ch = String.fromCharCode(c);
  if (/[\\"]/.test(ch)) continue;
  add(`JSON.parse(${q('"\\' + ch + '"')})`);
}
for (const e of ["\\u", "\\u1", "\\u12", "\\u123", "\\u0041", "\\u00G1", "\\uZZZZ", "\\u{41}", "\\U0041", "\\x41", "\\0", "\\00", "\\01", "\\8", "\\\n", "\\\r",
  "\\ ", "\\ ", "\\/", "\\b\\f\\n\\r\\t", "\\u0000", "\\uD83D", "\\uDE00", "\\uD83D\\uDE00", "\\uDE00\\uD83D", "\\uD83D\\u0041", "\\uD83D\\uD83D\\uDE00",
  "\\uD83D\\uDE", "\\uFFFF", "\\ufeff", "\\u2028", "\\u0022", "\\u005c", "\\u005C\\u005C", "\\\\u0041", "\\uD800\\uDBFF", "\\uDBFF\\uDFFF"]) {
  add(`JSON.parse(${q('"' + e + '"')})`, `T(()=>JSON.parse(${q('"a' + e + 'b"')}).length)`, `T(()=>JSON.parse(${q('{"' + e + '":1}')}))`);
}
for (let c = 0; c < 0x20; c++) add(`JSON.parse(${q('"a' + String.fromCharCode(c) + 'b"')})`, `JSON.parse(${q('{"a' + String.fromCharCode(c) + '":1}')})`);
for (const c of [0x7f, 0x80, 0x85, 0xa0, 0x2028, 0x2029, 0xfeff, 0xd800, 0xdc00, 0xffff]) add(`T(()=>JSON.parse(${q('"' + String.fromCharCode(c) + '"')}).charCodeAt(0))`);
for (const s of ['"', '"abc', '"abc\\', '"abc\\"', '"\\"', '""', '"" ', '"" x', '"a" "b"', '"a""b"', '["a"b]', '{"a"b:1}', '{"a" "b"}', '{"a":"b" "c"}', '{"a":"b","c"}',
  '{"a":"b","c":}', '{"a":"b","c"', '{"a":"b","c":"d"', '{"a":"b","c":"d",', '{"a":"b","c":"d"}x', '{"a":"b"}}', "{'a':1}", "['a']", '{"a":\'b\'}',
  '{a}', '{a:}', '{1}', '{true:1}', '{null:1}', '{[1]:2}', '{"a":1,,}', '{,"a":1}', '{"a":1;}', '{"a"=1}', '{"a"::1}', '{"a":1,"a"}', '["a":1]', '[1:2]',
  '[1;2]', '[1 2 3]', '[1,2,,3]', '[,,]', '[1,2,]', '[[],]', '[{},]', '[{"a":1},,]', '{"a":[1,]}', '{"a":{"b":1,}}', '{"a":1}{"b":2}', '[1][2]', '1 1', '"a" 1',
  'true false', 'null,', ',null', ':', '{}:', '[]:', '[]{}', '{}[]', '{}x', '[]x', 'x[]', 'x{}']) parse(s);

// ---- 4. Literais, caixa e truncamentos.
for (const lit of ["true", "false", "null"]) {
  for (let i = 0; i <= lit.length + 1; i++) {
    parse(lit.slice(0, i) + "x");
    parse(lit.slice(0, i));
    parse("[" + lit.slice(0, i));
    parse("[" + lit.slice(0, i) + "]");
    parse("{\"a\":" + lit.slice(0, i) + "}");
  }
  parse(lit.toUpperCase());
  parse(lit[0].toUpperCase() + lit.slice(1));
  parse(lit + lit);
  parse(lit + "1");
  parse(lit + "_");
  parse(lit + "$");
  parse("[" + lit + lit + "]");
}
for (const w of ["undefined", "NaN", "Infinity", "-Infinity", "nil", "None", "void 0", "this", "function(){}", "()=>1", "new Date", "Symbol()", "1n", "`a`", "/a/", "a.b", "a[0]", "(1)", "[1].x"]) {
  parse(w); parse("[" + w + "]"); parse("{\"a\":" + w + "}");
}

// ---- 5. Espaços em branco e BOM.
for (const c of [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x200b, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000, 0xfeff, 0x00]) {
  const ch = String.fromCharCode(c);
  add(`JSON.parse(${q(ch + "1")})`, `JSON.parse(${q("1" + ch)})`, `JSON.parse(${q("[" + ch + "1" + ch + "]")})`, `JSON.parse(${q('{"a"' + ch + ":" + ch + "1}")})`,
    `JSON.parse(${q("[1" + ch + ",2]")})`, `JSON.parse(${q("tr" + ch + "ue")})`, `JSON.parse(${q("-" + ch + "1")})`);
}
for (const s of ["", " ", "  \n\t\r ", "\n", "\t", "\r\n", "﻿", "﻿ ", "﻿﻿1", "﻿\"a\"", "﻿{}", "\u0000", "\u0000 ", " \u0000", "// c", "/* c */",
  "1 /* c */", "[1, /* c */ 2]", "[1, // c\n2]", "{\"a\":1 // c\n}", "# c\n1", "<!-- c\n1", "1 <!-- c", "--> c\n1", "1;", ";", "[1];", "{};", "1,", ",1"]) parse(s);

// ---- 6. Entradas que não são string, e segundo argumento.
add('JSON.parse("1",1,2,3)', 'JSON.parse(undefined,function(){})', 'T(()=>JSON.parse({toString(){return "{"}}))', 'T(()=>JSON.parse({valueOf(){return "1"},toString(){return "2"}}))',
  'T(()=>JSON.parse({[Symbol.toPrimitive](){return "[7]"}}))', 'T(()=>JSON.parse(new String("{\\"a\\":1}")).a)', 'T(()=>JSON.parse(["[1]"]))', 'T(()=>JSON.parse([[1]]))',
  'T(()=>JSON.parse(["a"]))', 'T(()=>JSON.parse(new Date(NaN)))', 'T(()=>JSON.parse(new Error("x")))', 'T(()=>JSON.parse(1e21))', 'T(()=>JSON.parse(1e-7))', 'T(()=>JSON.parse(0.1))',
  'T(()=>JSON.parse(-0))', 'T(()=>JSON.parse(Infinity))', 'T(()=>JSON.parse(10n))', 'T(()=>JSON.parse(false))', 'T(()=>JSON.parse(function(){}))', 'T(()=>JSON.parse(/1/))',
  'T(()=>JSON.parse(new Proxy({},{get(){throw new Error("px")}})))', 'T(()=>JSON.parse(Object.create(null)))', 'T(()=>JSON.parse(Object.assign(Object.create(null),{toString(){return "3"}})))',
  'T(()=>JSON.parse("1",function(){throw new TypeError("rv")}))', 'T(()=>JSON.parse("[",function(){throw new TypeError("never")}))');

// ---- 7. Profundidade.
for (const n of [10, 500, 2000, 4999, 5000, 5001, 9000, 12000, 50000]) {
  add(`T(()=>{var s="{\\"a\\":".repeat(${n})+"1"+"}".repeat(${n});return typeof JSON.parse(s)})`,
    `T(()=>{var s="[{\\"a\\":".repeat(${n})+"1"+"}]".repeat(${n});return typeof JSON.parse(s)})`,
    `T(()=>{var s="[".repeat(${n});return JSON.parse(s)})`,
    `T(()=>{var s="{\\"a\\":".repeat(${n});return JSON.parse(s)})`,
    `T(()=>{var s="[".repeat(${n})+"]".repeat(${n}-1);return JSON.parse(s)})`,
    `T(()=>{var s="[".repeat(${n})+"x"+"]".repeat(${n});return JSON.parse(s)})`,
    `T(()=>{var s="[".repeat(${n})+"]".repeat(${n});return JSON.parse(s,(k,v)=>v) instanceof Array})`);
}
add('T(()=>{try{JSON.parse("[".repeat(100000)+"]".repeat(100000))}catch(e){return e.constructor===RangeError||e.constructor===SyntaxError}return "ok"})',
  'T(()=>{try{JSON.parse("[".repeat(100000))}catch(e){return e.name}return "ok"})',
  'T(()=>{var o={};var l=o;for(var i=0;i<20000;i++){l.a={};l=l.a}try{JSON.stringify(o)}catch(e){return e.name}return "ok"})');

// ---- 8. Reviver.
const revSrc = 'var L=[];function rv(k,v,c){L.push(k+"="+(v&&typeof v==="object"?(Array.isArray(v)?"A"+v.length:"O"):S(v))+(c&&"source" in c?"#"+c.source:""));return v}';
for (const doc of ['[1,[2,[3,[4]]]]', '{"a":{"b":{"c":{"d":1}}}}', '[[],{},[[]],{"a":[]}]', '{"x":[{"y":1},{"y":2}],"z":null}', '[1.50,2e0,-0.0,"\\u0041",true,null,12345678901234567890]',
  '{"b":1,"a":2,"2":3,"1":4,"-1":5,"01":6}', '{"__proto__":{"a":1},"b":2}', '[["a","b"],["c"]]', '{"":0,"a":{"":1}}', ' [ 1 , 2 ] ', '[1e2,1E2,1e+2,100]', '{"a":1,"a":2,"b":3}',
  '[0.1,0.10,1.0,10e-1]', '"\\ud83d\\ude00"', '[-0,0,-0.0]', '123456789012345678901234567890', '{"a":[1,2,3]}']) {
  add(`(()=>{${revSrc};JSON.parse(${q(doc)},rv);return L.join("|")})()`);
}
const revFns = [
  ['holes', 'function(k,v){if(Array.isArray(this)&&k==="1")return undefined;return v}'],
  ['holes-all', 'function(k,v){return Array.isArray(this)?undefined:v}'],
  ['delete-other', 'function(k,v){if(k==="0"&&Array.isArray(this))delete this[1];return v}'],
  ['delete-later-key', 'function(k,v){if(k==="a"&&!Array.isArray(this))delete this.c;return v}'],
  ['add-later-key', 'function(k,v){if(k==="a"&&!Array.isArray(this))this.d=4;return v}'],
  ['replace-holder-value', 'function(k,v){if(k==="a"&&!Array.isArray(this))this.b="new";return v}'],
  ['swap-type-obj-to-arr', 'function(k,v){if(k==="a"&&!Array.isArray(this))this.b=[7,8];return v}'],
  ['shrink', 'function(k,v){if(k==="0"&&Array.isArray(this))this.length=0;return v}'],
  ['grow', 'function(k,v){if(k==="0"&&Array.isArray(this))this[5]=1;return v}'],
  ['define-getter', 'function(k,v){if(k==="a"&&!Array.isArray(this))Object.defineProperty(this,"b",{get(){return "g"},enumerable:true,configurable:true});return v}'],
  ['nonconfigurable-undefined', 'function(k,v){if(k==="b"&&!Array.isArray(this)){Object.defineProperty(this,"c",{value:1,configurable:false,writable:true,enumerable:true})}if(k==="c")return undefined;return v}'],
  ['set-holder-null', 'function(k,v){if(k==="a")Object.setPrototypeOf(this,null);return v}'],
  ['return-this', 'function(k,v){return k===""?v:this}'],
  ['return-root', 'function(k,v){return k===""?v:v}'],
  ['return-key', 'function(k,v){return k}'],
  ['return-array-of-pair', 'function(k,v){return [k,typeof v]}'],
  ['return-bigint', 'function(k,v){return typeof v==="number"?BigInt(v):v}'],
  ['return-symbol', 'function(k,v){return k==="a"?Symbol("s"):v}'],
  ['return-nan', 'function(k,v){return typeof v==="number"?NaN:v}'],
  ['return-neg0', 'function(k,v){return typeof v==="number"?-0:v}'],
  ['throw-string', 'function(k,v){if(k==="b")throw "str";return v}'],
  ['throw-late', 'function(k,v){if(k==="")throw new Error("root");return v}'],
  ['bound', 'function(k,v){return v}.bind({})'],
  ['proxy-fn', 'new Proxy(function(k,v){return v},{})'],
  ['async-ish', 'async function(k,v){return v}'],
  ['gen', 'function*(k,v){yield v}'],
  ['class-ctor', 'class{}'],
  ['toString-only', '{toString(){return "x"}}'],
  ['callable-proxy-arrow', 'new Proxy((k,v)=>v,{})'],
];
for (const [name, fn] of revFns) {
  for (const doc of ['{"a":1,"b":2,"c":3}', '[1,2,3]', '[[1,2],[3,4]]', '{"a":[1,2],"b":{"c":3}}', '5']) add(`T(()=>JSON.stringify(JSON.parse(${q(doc)},${fn})))`);
}
add('T(()=>{var a=[];JSON.parse("{\\"a\\":[1,{\\"b\\":2}],\\"c\\":3}",function(k,v){a.push(k);return v});return a.join()})',
  'T(()=>{var a=[];JSON.parse("[[],[[]],{}]",function(k,v){a.push(k+":"+Array.isArray(this));return v});return a.join()})',
  'T(()=>{var h=[];JSON.parse("[1,[2]]",function(k,v){h.push(this);return v});return h[0]===h[h.length-1]})',
  'T(()=>{var r;var out=JSON.parse("{\\"a\\":1}",function(k,v){if(k==="")r=this;return v});return Object.keys(r).join()+"|"+typeof r[""]})',
  'T(()=>{var r;JSON.parse("{\\"a\\":1}",function(k,v){if(k==="")r=this;return v});return Object.getPrototypeOf(r)===Object.prototype&&Object.getOwnPropertyNames(r).length})',
  'T(()=>{var r,t;var out=JSON.parse("[1]",function(k,v){if(k==="")r=this;return k===""?"new":v});return JSON.stringify(out)+Array.isArray(r)})',
  'T(()=>{var th=[];JSON.parse("[1]",function(){"use strict";th.push(typeof this)});return th.join()})',
  'T(()=>{var th=[];JSON.parse("1",function(){th.push(this===globalThis)});return th.join()})',
  'T(()=>{var c;JSON.parse("[1,2]",function(k,v,ctx){c=arguments.length});return c})',
  'T(()=>{var s=[];JSON.parse("{\\"a\\":[1,2.0,\\"x\\",null,true,false,{}],\\"b\\":-1.5e2}",function(k,v,c){s.push(k+":"+(c&&c.source))});return s.join("|")})',
  'T(()=>{var s=[];JSON.parse("[1,2]",function(k,v,c){if(k==="0")this[1]=99;s.push(c&&c.source);return v});return s.join()})',
  'T(()=>{var s=[];JSON.parse("[1,2]",function(k,v,c){if(k==="0")this[1]=2;s.push(c&&c.source);return v});return s.join()})',
  'T(()=>{var s=[];JSON.parse("{\\"a\\":1,\\"b\\":2}",function(k,v,c){if(k==="a")this.b=2;s.push(k+":"+(c&&c.source));return v});return s.join()})',
  'T(()=>{var s=[];JSON.parse("{\\"a\\":1,\\"b\\":2}",function(k,v,c){if(k==="a")this.b="2";s.push(k+":"+(c&&c.source));return v});return s.join()})',
  'T(()=>{var s=[];JSON.parse("{\\"a\\":[1]}",function(k,v,c){if(k==="a")this.a=[1];s.push(k+":"+(c&&Object.keys(c).length));return v});return s.join()})',
  'T(()=>{var s=[];JSON.parse("[1]",function(k,v,c){s.push(Object.getOwnPropertyDescriptor(c,"source")&&JSON.stringify(Object.getOwnPropertyDescriptor(c,"source")))});return s.join()})',
  'T(()=>{var s=[];JSON.parse("[1]",function(k,v,c){s.push(Object.isExtensible(c)+","+Object.isFrozen(c)+","+Object.isSealed(c))});return s.join("|")})',
  'T(()=>{var s=[];JSON.parse("\\"\\\\u0041\\\\n\\"",function(k,v,c){s.push(c.source)});return s[0]})',
  'T(()=>{var s=[];JSON.parse("-0.0e+0",function(k,v,c){s.push(c.source,Object.is(v,-0))});return s.join()})',
  'T(()=>{var s=[];JSON.parse("1e999",function(k,v,c){s.push(c.source,v)});return s.join()})',
  'T(()=>JSON.parse("{\\"__proto__\\":1,\\"a\\":2}",function(k,v){return k==="__proto__"?"p":v}).__proto__)',
  'T(()=>Object.keys(JSON.parse("{\\"__proto__\\":1,\\"a\\":2}",function(k,v){return v})).join())',
  'T(()=>Object.getPrototypeOf(JSON.parse("{\\"__proto__\\":{\\"x\\":1}}",(k,v)=>v))===Object.prototype)',
  'T(()=>JSON.stringify(JSON.parse("{\\"__proto__\\":{\\"x\\":1}}",(k,v)=>k==="x"?2:v)))',
  'T(()=>{var o=JSON.parse("{\\"a\\":1}",function(k,v){if(k==="a")return {toJSON(){return 5}};return v});return JSON.stringify(o)})',
  'T(()=>{var cnt=0;JSON.parse("[".repeat(500)+"]".repeat(500),function(){cnt++});return cnt})',
  'T(()=>{var cnt=0;JSON.parse(JSON.stringify(Array.from({length:2000},(_,i)=>({i}))),function(){cnt++});return cnt})',
  'T(()=>{var keys=[];JSON.parse(JSON.stringify(Object.fromEntries(Array.from({length:20},(_,i)=>["k"+(19-i),i]))),function(k){keys.push(k)});return keys.join()})',
  'T(()=>{var keys=[];JSON.parse(\'{"10":1,"2":2,"b":3,"a":4,"1":5}\',function(k){keys.push(k)});return keys.join()})',
  'T(()=>JSON.parse("[1,2,3]",function(k,v){return Array.isArray(v)?v.length:v}))',
  'T(()=>JSON.stringify(JSON.parse("[1,2,3]",function(k,v){if(k==="2"&&Array.isArray(this)){this.length=1}return v})))',
  'T(()=>{var a=JSON.parse("[1,2,3]",function(k,v){if(k==="0"&&Array.isArray(this))delete this[2];return v});return JSON.stringify(a)+(2 in a)+a.length})',
  'T(()=>{var a=JSON.parse("[1,2,3]",function(k,v){return k==="1"?undefined:v});return JSON.stringify(a)+(1 in a)+a.length})',
  'T(()=>{var a=JSON.parse("{\\"x\\":[1,2,3]}",function(k,v){return k==="1"?undefined:v});return JSON.stringify(a)+(1 in a.x)+a.x.length})',
  'T(()=>{var a=JSON.parse("{\\"a\\":1}",function(k,v){return k==="a"?undefined:v});return JSON.stringify(a)+("a" in a)})');

// ---- 9. Valores especiais de stringify em contextos que o golden base não cruza.
const special = [
  'undefined', 'null', 'NaN', '-0', 'Infinity', '1n', 'Symbol("s")', '()=>1', 'new Date(NaN)', 'new Date(0)', 'new Number(-0)', 'new String("a")', 'new Boolean(false)', 'Object(1n)', 'Object(Symbol())',
  '"\\ud800"', '"\\u2028\\u2029"', '"\\u007f"', '[]', '{}', '[undefined]', '{a:undefined}', 'new Map', 'new Set', 'new Map([[1,2]])', 'new Set([1])', 'new Uint8Array(2)', 'new Float64Array([-0,NaN])',
  'new Error("m")', '/x/', 'Promise.resolve(1)', 'new Proxy({a:1},{})', 'new Proxy([1],{})', 'new Proxy(()=>{},{})', 'Object.create({a:1})', 'Object.create(null)', '{toJSON(){return undefined}}',
  '{toJSON(){return 1n}}', 'JSON.rawJSON("1")', 'JSON.rawJSON("\\"s\\"")', 'Object.assign(()=>{},{a:1})', 'Math', 'JSON', 'globalThis.nope', 'new (class{})', 'new (class{a=1})', '(function(){return arguments})(1)',
];
const wrappers = [
  ['arr-repl', v => `T(()=>JSON.stringify({k:${v},z:1},["k","z"]))`],
  ['arr-repl-top', v => `T(()=>JSON.stringify(${v},["k"]))`],
  ['fn-identity', v => `T(()=>JSON.stringify({k:${v}},(k,x)=>x))`],
  ['fn-identity-top', v => `T(()=>JSON.stringify(${v},(k,x)=>x))`],
  ['fn-log', v => `T(()=>{var l=[];var r=JSON.stringify([${v}],function(k,x){l.push(typeof k+":"+k+":"+typeof x);return x});return r+"|"+l.join()})`],
  ['nested-indent', v => `T(()=>JSON.stringify({a:[{b:${v}}],c:${v}},null,"--"))`],
  ['sparse-array', v => `T(()=>JSON.stringify([,${v},,]))`],
  ['toJSON-wrapped', v => `T(()=>JSON.stringify({toJSON(){return ${v}}}))`],
  ['toJSON-in-prop', v => `T(()=>JSON.stringify({a:{toJSON(k){return ${v}}}}))`],
  ['repl-returns', v => `T(()=>JSON.stringify({a:1},(k,x)=>k==="a"?${v}:x))`],
  ['repl-returns-root', v => `T(()=>JSON.stringify(1,(k,x)=>(${v})))`],
  ['proxy-holder', v => `T(()=>JSON.stringify(new Proxy({k:${v}},{})))`],
  ['array-of-two', v => `T(()=>JSON.stringify([${v},${v}],null,1))`],
];
for (const v of special) for (const [, w] of wrappers) add(w(v));

// ---- 10. Replacer array: lista de propriedades.
const objects = ['{a:1,b:2,c:{a:3,d:4}}', '[{a:1,b:2}]', '{1:"x",2:"y",a:"z"}', '{"":1}'];
const lists = [
  '["a","a","a"]', '["a",1,"a"]', '[1,1,1]', '[1,"1"]', '["1",1]', '[1,2]', '[2,1]', '[-1]', '[1.0]', '[1.5]', '[1e0]', '[0x1]', '[new Number(1),1]', '[new String("a"),new String("a")]',
  '[{toString(){return "a"}}]', '[{toString(){return "a"},valueOf(){return "b"}}]', '[Symbol()]', '[Symbol.iterator]', '[undefined,"a"]', '[null,"a"]', '[true,"a"]', '[false]',
  '[10n]', '[[]]', '[["a"]]', '[()=>"a"]', '[""]', '["","a"]', '["a b"]', '["d"]', '["c","d"]', '["d","c"]', '[new Number(NaN)]', '[new Boolean(1)]', '[new Date(0)]',
  'Object.assign([],{0:"a",length:1,x:"b"})', 'Object.assign([],{length:3})', '(()=>{var a=["a"];a.push("b");return a})()', 'Object.assign(["a","b"],{length:1})',
  'new Proxy(["a"],{get(t,k,r){return k==="0"?"b":Reflect.get(t,k,r)}})', 'new Proxy(["a"],{})', 'new Proxy({0:"a",length:1},{})', '{length:1,0:"a"}',
  'Object.assign(Object.create(Array.prototype),{0:"a",length:1})', 'Object.defineProperty(["a"],"0",{get(){return "b"}})', 'Object.defineProperty(["a"],"0",{get(){throw new Error("g")}})',
  '(()=>{class A extends Array{};var a=new A;a.push("a");return a})()', 'new Uint8Array([1])', 'new Set(["a"])', 'new String("a")', '"a"', '5', 'true', '{}', '/a/', 'new Date(0)',
];
for (const l of lists) for (const o of objects.slice(0, 3)) add(`T(()=>JSON.stringify(${o},${l}))`);
for (const l of lists.slice(0, 20)) add(`T(()=>JSON.stringify(${objects[0]},${l},2))`);
add('T(()=>JSON.stringify({a:1},["a"],"xx"))', 'T(()=>JSON.stringify({a:{a:1}},["a"],1))', 'T(()=>JSON.stringify([{a:1}],["a"],1))');

// ---- 11. Replacer função com this e ordem.
add('T(()=>{var l=[];JSON.stringify({a:1,b:{c:2},d:[3]},function(k,v){l.push(k+"@"+(Array.isArray(this)?"A":Object.keys(this).join("")));return v});return l.join("|")})',
  'T(()=>{var h=[];var o={a:{b:1}};JSON.stringify(o,function(k,v){h.push(this);return v});return h[0]!==o&&h[1]===o&&h[2]===o.a})',
  'T(()=>{var r;JSON.stringify(5,function(k,v){r=this});return Object.keys(r).join()+typeof r[""]+Object.getPrototypeOf(r)===Object.prototype})',
  'T(()=>{var r;JSON.stringify(5,function(k,v){r=this});return JSON.stringify(Object.getOwnPropertyDescriptor(r,""))})',
  'T(()=>{var r;JSON.stringify({a:1},function(k,v){if(k==="")r=this;return v});return JSON.stringify(r)})',
  'T(()=>{var t=[];JSON.stringify([1],function(k,v){"use strict";t.push(typeof this);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify(1,function(k,v){t.push(this===globalThis);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify({a:1},(k,v)=>{t.push(typeof this);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify({a:{toJSON(k){t.push("tj:"+k);return 1}}},(k,v)=>{t.push("r:"+k);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify({a:new Date(0)},(k,v)=>{t.push(typeof v);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify({a:Object(1n)},(k,v)=>{t.push(typeof v);return v});return t.join()})',
  'T(()=>{var t=[];JSON.stringify({a:new Number(1)},(k,v)=>{t.push(typeof v);return typeof v==="object"?v:v});return t.join()})',
  'T(()=>{var n=0;JSON.stringify({a:{b:{c:{}}}},(k,v)=>{n++;return v});return n})',
  'T(()=>JSON.stringify({a:1,b:2,c:3},(k,v)=>k==="b"?undefined:v))', 'T(()=>JSON.stringify([1,2,3],(k,v)=>k==="1"?undefined:v))',
  'T(()=>JSON.stringify({a:1,b:2},function(k,v){if(k==="a")this.b=9;return v}))', 'T(()=>JSON.stringify([1,2],function(k,v){if(k==="0")this[1]=9;return v}))',
  'T(()=>JSON.stringify({a:1},function(k,v){if(k==="")return {a:1,b:2};return v}))', 'T(()=>JSON.stringify({a:1},function(k,v){if(k==="")return [1,2];return v}))',
  'T(()=>JSON.stringify({a:1},function(k,v){if(k==="")return "root";return v}))', 'T(()=>JSON.stringify({a:1},function(k,v){if(k==="")return function(){};return v}))',
  'T(()=>JSON.stringify({a:{b:1}},function(k,v){return k==="a"?{b:v.b+1,c:3}:v}))', 'T(()=>JSON.stringify({a:{b:1}},function(k,v){return k==="b"?{n:v}:v}))',
  'T(()=>JSON.stringify({a:1},async function(k,v){return v}))', 'T(()=>JSON.stringify({a:1},function*(k,v){yield v}))', 'T(()=>JSON.stringify({a:1},new Proxy(function(k,v){return v},{})))',
  'T(()=>JSON.stringify({a:1},Function.prototype))', 'T(()=>JSON.stringify({a:1},Math.abs))', 'T(()=>JSON.stringify({a:-1},Math.abs))', 'T(()=>JSON.stringify({a:1},String))',
  'T(()=>JSON.stringify({a:1},Number))', 'T(()=>JSON.stringify({a:1},Boolean))', 'T(()=>JSON.stringify({a:1},Array))', 'T(()=>JSON.stringify({a:1},Object))',
  'T(()=>JSON.stringify({a:1},Symbol))', 'T(()=>JSON.stringify({a:1},BigInt))', 'T(()=>JSON.stringify({a:1},Date))', 'T(()=>JSON.stringify({a:1},Error))',
  'T(()=>JSON.stringify({a:1},class{}))', 'T(()=>JSON.stringify({a:1},Function.prototype.call.bind(function(){return 1})))');

// ---- 12. Space.
const spaceVals2 = ['0', '-0', '0.5', '1', '1.99', '9', '10', '10.5', '11', '20', '21', '1e9', '1e21', '2**31', '2**32+2', '-5', 'NaN', 'Infinity', 'new Number(2)', 'new Number(20)',
  'new Number("3")', 'new String("   ")', '"          "', '"           "', '"-----------"', '"\\t\\t"', '"a\\nb"', '"\\u2028"', '"\\ud83d\\ude00"', '"\\ud83d\\ude00".repeat(5)', '"\\ud83d\\ude00".repeat(6)',
  '"\\ud800".repeat(11)', '"é".repeat(10)', '"é".repeat(12)', '"\\u0000"', '" ".repeat(100)', '"x".repeat(10)', '"x".repeat(9)+"\\ud83d\\ude00"', '"x".repeat(9)+"\\ud83d"', '"x".repeat(10)+"\\ud83d"',
  'true', 'new Boolean(true)', 'null', 'undefined', '[]', '[2]', '{}', '()=>2', 'Symbol()', '1n', 'Object(1n)', '{valueOf(){return 4}}', '{toString(){return "~"}}',
  'Object.assign(new Number(1),{valueOf(){return 6}})', 'Object.assign(new String("z"),{toString(){return "yy"}})', 'new Proxy(new Number(3),{})', 'new Proxy(new String("q"),{})', 'new Date(2)',
  'Object.assign(new Number(1),{[Symbol.toPrimitive](){return 7}})', 'Object.assign(new String("a"),{[Symbol.toPrimitive](){return "pp"}})'];
for (const s of spaceVals2) add(`T(()=>JSON.stringify({a:[1,{b:[]},{}],c:"s"},null,${s}))`, `T(()=>JSON.stringify([[1,[2]],{}],null,${s}))`);
add('T(()=>JSON.stringify({a:[]},null,2).split("\\n").length)', 'T(()=>JSON.stringify([[],{}],null,2))', 'T(()=>JSON.stringify([{}],null,"\\t"))', 'T(()=>JSON.stringify({a:{}},null,1))',
  'T(()=>JSON.stringify({a:[1]},["a"],"\\t"))', 'T(()=>JSON.stringify(1,null,2))', 'T(()=>JSON.stringify("s",null,2))', 'T(()=>JSON.stringify(null,null,2))', 'T(()=>JSON.stringify(undefined,null,2))',
  'T(()=>JSON.stringify([undefined],null,2))', 'T(()=>JSON.stringify({a:undefined},null,2))', 'T(()=>JSON.stringify({a:undefined,b:1},null,2))', 'T(()=>JSON.stringify([function(){},1],null,2))',
  'T(()=>JSON.stringify({a:1},null,"\\"\\\\"))', 'T(()=>JSON.stringify({"a\\nb":1},null,2))', 'T(()=>JSON.stringify({a:1},(k,v)=>v,2))', 'T(()=>JSON.stringify({a:1},[],2))', 'T(()=>JSON.stringify({a:1},["a"],0))');

// ---- 13. Ciclos: mais formas e a mensagem exata.
const cyc = [
  'var a={};a.a=a;JSON.stringify(a)', 'var a=[];a.push(a);JSON.stringify(a)', 'var a={b:{}};a.b.c=a.b;JSON.stringify(a)', 'var a={b:[{}]};a.b[0].c=a;JSON.stringify(a)',
  'var a={};a.a=a;JSON.stringify(a,null,2)', 'var a={};a.a=a;JSON.stringify(a,["a"])', 'var a={};a.a=a;JSON.stringify(a,(k,v)=>v)', 'var a={};var b={a};a.b=b;JSON.stringify([a,b])',
  'var a={};a.x=[a];JSON.stringify({r:a})', 'var a=[[]];a[0].push(a);JSON.stringify(a)', 'var a={toJSON(){return {x:a}}};JSON.stringify(a)', 'var a={};a.x={toJSON(){return a}};JSON.stringify(a)',
  'var a={};JSON.stringify(a,function(k,v){return k==="z"?a:v})', 'var a={z:1};JSON.stringify(a,function(k,v){return k==="z"?a:v})', 'var a={z:1};JSON.stringify(a,function(k,v){return k==="z"?[a]:v})',
  'var a=Object.create(null);a.a=a;JSON.stringify(a)', 'var a=new Proxy({},{get(t,k,r){return k==="a"?r:undefined},ownKeys(){return["a"]},getOwnPropertyDescriptor(){return{value:1,enumerable:true,configurable:true}}});JSON.stringify(a)',
  'var a=new Proxy([],{get(t,k,r){return k==="length"?1:k==="0"?r:undefined}});JSON.stringify(a)', 'var m={};m.m=m;JSON.stringify(Object(m))',
  'var a={};a.a=a;(()=>{try{JSON.stringify(a)}catch(e){return e.message}})()', 'var a={};a.a=a;(()=>{try{JSON.stringify(a)}catch(e){return e instanceof TypeError}})()',
  'var a={};a.a=a;(()=>{try{JSON.stringify(a)}catch(e){return e.name+e.constructor.name}})()', 'var a={};a.a=a;(()=>{try{JSON.stringify(a)}catch(e){return Object.getOwnPropertyNames(e).join()}})()',
  'var a={};a.a=a;(()=>{try{JSON.stringify(a)}catch(e){return e.stack.split("\\n")[0]}})()',
  'var a=Object(1);a.a=a;JSON.stringify(a)', 'var a=new String("s");a.a=a;JSON.stringify(a)', 'var a=[];a[3]=a;JSON.stringify(a)', 'var a={};a[2**31]=a;JSON.stringify(a)',
  'var a={x:{y:{z:{}}}};a.x.y.z.w=a.x;JSON.stringify(a)', 'var a={};a.b={};a.b.c={};a.b.c.d=a.b.c;JSON.stringify(a)', 'var a={};a.b={};a.c=a.b;a.b.d=a.c;JSON.stringify(a)',
  'var s={};var a={p:s,q:s};JSON.stringify(a)', 'var s={};var a=[s,[s,[s]]];JSON.stringify(a)', 'var s={};JSON.stringify({a:{b:s},c:{d:s}})',
  'var a={};a.a=a;JSON.stringify(a,function(k,v){return k===""?v:undefined})', 'var a={};a.a=a;JSON.stringify(a,(k,v)=>k==="a"?1:v)', 'var a={};a.a=a;JSON.stringify(a,(k,v)=>k==="a"?{b:1}:v)',
  'var a={};a.a=a;JSON.stringify(a,(k,v)=>k==="a"?[]:v)', 'var a=[];a[0]=a;JSON.stringify(a,(k,v)=>k==="0"?null:v)', 'var a={};a.a={};a.a.a=a;JSON.stringify(a,null,"x")',
];
for (const c of cyc) add(`T(()=>{${c.replace(/;([^;]*)$/, ";return $1")}})`);

// ---- 14. Getters, não enumeráveis, herdadas, class, Symbol.
add('T(()=>JSON.stringify({get a(){throw new RangeError("ra")}}))', 'T(()=>JSON.stringify([{get a(){throw new RangeError("rb")}}]))', 'T(()=>JSON.stringify({a:{get b(){throw new EvalError("rc")}}}))',
  'T(()=>JSON.stringify({get a(){throw undefined}}))', 'T(()=>JSON.stringify({get a(){throw null}}))', 'T(()=>JSON.stringify({get a(){throw 1}}))', 'T(()=>JSON.stringify({get a(){throw {x:1}}}))',
  'T(()=>JSON.stringify({get a(){throw Symbol("s")}}))', 'T(()=>JSON.stringify({a:1,get b(){throw new Error("b")},get c(){throw new Error("c")}}))',
  'T(()=>JSON.stringify({b:1,get a(){throw new Error("a")}},["b"]))', 'T(()=>JSON.stringify({b:1,get a(){throw new Error("a")}},["a"]))',
  'T(()=>JSON.stringify({get a(){return {get b(){throw new Error("deep")}}}}))', 'T(()=>{var l=[];JSON.stringify({get a(){l.push("a");return 1},get b(){l.push("b");return 2}});return l.join()})',
  'T(()=>{var l=[];JSON.stringify({get b(){l.push("b");return 2},get a(){l.push("a");return 1},get 1(){l.push("1");return 0}});return l.join()})',
  'T(()=>{var l=[];JSON.stringify({get a(){l.push("a");return {toJSON(){l.push("tj");return 1}}}});return l.join()})',
  'T(()=>{var l=[];JSON.stringify({a:1,toJSON(){l.push("tj");return {get b(){l.push("b");return 2}}}});return l.join()})',
  'T(()=>JSON.stringify(Object.defineProperties({},{a:{value:1,enumerable:true},b:{value:2,enumerable:false},c:{get(){return 3},enumerable:true}})))',
  'T(()=>JSON.stringify(Object.defineProperties([],{0:{value:1,enumerable:false},length:{value:1}})))', 'T(()=>JSON.stringify(Object.defineProperty([5],"0",{enumerable:false})))',
  'T(()=>JSON.stringify(Object.defineProperty({},"a",{get(){return 1}})))', 'T(()=>JSON.stringify(Object.create({a:1,b:2},{c:{value:3,enumerable:true}})))',
  'T(()=>JSON.stringify(Object.create({get a(){return 1}})))', 'T(()=>{class A{constructor(){this.x=1}get y(){return 2}static z=3;w=4;#p=5;toString(){return "A"}}return JSON.stringify(new A)})',
  'T(()=>{class A{toJSON(){return {cls:1}}}class B extends A{}return JSON.stringify([new A,new B])})', 'T(()=>{class A{static toJSON(){return "s"}}return JSON.stringify(A)})',
  'T(()=>{class A extends Array{}var a=new A;a.push(1,2);return JSON.stringify(a)})', 'T(()=>{class A extends Map{}return JSON.stringify(new A([[1,2]]))})',
  'T(()=>{class A extends Number{}return JSON.stringify(new A(4))})', 'T(()=>{class A extends String{}return JSON.stringify(new A("s"))})', 'T(()=>{class A extends Boolean{}return JSON.stringify(new A(true))})',
  'T(()=>{class A extends Date{}return JSON.stringify(new A(0))})', 'T(()=>{class A extends Error{}return JSON.stringify(new A("m"))})', 'T(()=>{class A extends Object{constructor(){super();this.a=1}}return JSON.stringify(new A)})',
  'T(()=>JSON.stringify({[Symbol("a")]:1,[Symbol.for("b")]:2,c:3}))', 'T(()=>JSON.stringify({a:Symbol.for("x"),b:[Symbol("y")]}))', 'T(()=>JSON.stringify({[Symbol.toJSON]:1}))',
  'T(()=>JSON.stringify({a:1,[Symbol("s")]:{toJSON(){throw new Error("never")}}}))', 'T(()=>JSON.stringify({a:1},[Symbol.for("a")]))', 'T(()=>JSON.stringify(Object(Symbol("s"))))',
  'T(()=>JSON.stringify({a:Object(Symbol("s"))}))', 'T(()=>JSON.stringify([Object(Symbol("s"))]))', 'T(()=>JSON.stringify(Symbol.iterator))', 'T(()=>JSON.stringify([Symbol.iterator,1]))',
  'T(()=>JSON.stringify({a:1},(k,v)=>typeof v==="number"?Symbol():v))', 'T(()=>JSON.stringify([1],(k,v)=>typeof v==="number"?Symbol():v))', 'T(()=>JSON.stringify(1,(k,v)=>typeof v==="number"?Symbol():v))');

// ---- 15. Proxy, array-likes, Map/Set, typed arrays, wrappers.
add('T(()=>JSON.stringify(new Proxy({a:1},{ownKeys(){return[]}})))', 'T(()=>JSON.stringify(new Proxy({a:1},{ownKeys(){return["b","a"]},getOwnPropertyDescriptor(t,k){return Reflect.getOwnPropertyDescriptor(t,k)}})))',
  'T(()=>JSON.stringify(new Proxy({a:1},{getOwnPropertyDescriptor(){return undefined}})))', 'T(()=>JSON.stringify(new Proxy({a:1},{getOwnPropertyDescriptor(){return {value:1,configurable:true}}})))',
  'T(()=>JSON.stringify(new Proxy({a:1},{get(){return 42}})))', 'T(()=>JSON.stringify(new Proxy({a:1},{get(t,k){return k==="a"?{toJSON(){return "x"}}:undefined}})))',
  'T(()=>JSON.stringify(new Proxy([1,2,3],{get(t,k,r){return k==="length"?1:Reflect.get(t,k,r)}})))', 'T(()=>JSON.stringify(new Proxy([1],{get(t,k,r){return k==="length"?3:Reflect.get(t,k,r)}})))',
  'T(()=>JSON.stringify(new Proxy([],{get(t,k,r){return k==="length"?2**32:Reflect.get(t,k,r)}})))', 'T(()=>JSON.stringify(new Proxy([],{get(t,k){if(k==="length")throw new Error("len");return undefined}})))',
  'T(()=>{var l=[];JSON.stringify(new Proxy({a:1},{get(t,k,r){l.push(typeof k+":"+String(k));return Reflect.get(t,k,r)}}));return l.join()})',
  'T(()=>{var l=[];JSON.stringify(new Proxy([1],{get(t,k,r){l.push(String(k));return Reflect.get(t,k,r)}}));return l.join()})',
  'T(()=>{var l=[];JSON.stringify(new Proxy({a:1,b:2},{ownKeys(t){l.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push("gopd:"+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push("get:"+String(k));return Reflect.get(t,k,r)}}),["a"]);return l.join()})',
  'T(()=>{var l=[];JSON.stringify({a:new Proxy({},{get(t,k){l.push(String(k));return undefined}})});return l.join()})',
  'T(()=>{var r=Proxy.revocable({},{});r.revoke();return JSON.stringify({a:r.proxy})})', 'T(()=>{var r=Proxy.revocable({},{});r.revoke();return JSON.stringify([r.proxy])})',
  'T(()=>{var r=Proxy.revocable(()=>{},{});r.revoke();return JSON.stringify(r.proxy)})', 'T(()=>{var r=Proxy.revocable({},{});r.revoke();return JSON.stringify(1,r.proxy)})',
  'T(()=>{var p=new Proxy({},{});return JSON.stringify(Object.create(p))})', 'T(()=>JSON.stringify(new Proxy(new Proxy({a:1},{}),{})))', 'T(()=>JSON.stringify(new Proxy(new Proxy([1],{}),{})))',
  'T(()=>JSON.stringify(new Proxy(new Map([[1,2]]),{})))', 'T(()=>JSON.stringify(new Proxy(new Uint8Array(2),{})))', 'T(()=>JSON.stringify(new Proxy(Object(1n),{})))', 'T(()=>JSON.stringify(new Proxy(JSON.rawJSON("1"),{})))',
  'T(()=>JSON.stringify({length:2,0:"a",1:"b"}))', 'T(()=>JSON.stringify(Object.assign(Object.create(null),{length:1,0:"a"})))', 'T(()=>JSON.stringify(Array.prototype.slice.call({length:2,0:"a"})))',
  'T(()=>JSON.stringify(new String("abc")))', 'T(()=>JSON.stringify(Object.assign(new String("abc"),{x:1})))', 'T(()=>JSON.stringify({a:new String("abc")}))', 'T(()=>JSON.stringify([...new String("ab")]))',
  'T(()=>JSON.stringify((function(){return arguments})(1,"a")))', 'T(()=>JSON.stringify((function(){"use strict";return arguments})(1,"a")))', 'T(()=>JSON.stringify(Array.from("ab")))',
  'T(()=>JSON.stringify(new Map))', 'T(()=>JSON.stringify(new Set))', 'T(()=>JSON.stringify(new WeakMap))', 'T(()=>JSON.stringify(new WeakSet))', 'T(()=>JSON.stringify({m:new Map,s:new Set,a:[]}))',
  'T(()=>JSON.stringify([new Map([["a",1]]),new Set(["a"])]))', 'T(()=>JSON.stringify(Object.fromEntries(new Map([["a",1],["b",2]]))))', 'T(()=>JSON.stringify([...new Set([1,2,2])]))',
  'T(()=>JSON.stringify(new Map,null,2))', 'T(()=>JSON.stringify(Object.assign(new Map,{x:1})))', 'T(()=>JSON.stringify(Object.assign(new Set,{x:1})))',
  'T(()=>JSON.stringify(new Uint8Array(0)))', 'T(()=>JSON.stringify(new Uint8Array([1,2])))', 'T(()=>JSON.stringify(new Uint8Array([1,2]),null,1))', 'T(()=>JSON.stringify(new Uint8Array([1,2]),(k,v)=>typeof v==="number"?v*2:v))',
  'T(()=>JSON.stringify(new Float32Array([0.1,-0,NaN])))', 'T(()=>JSON.stringify(new Int16Array([-1,32768])))', 'T(()=>JSON.stringify(new BigInt64Array(1)))', 'T(()=>JSON.stringify(new BigUint64Array([1n])))',
  'T(()=>JSON.stringify(new Uint8ClampedArray([300,-5])))', 'T(()=>JSON.stringify(new Uint8Array(4).subarray(1,3)))', 'T(()=>JSON.stringify(new Uint8Array(new ArrayBuffer(4),2)))',
  'T(()=>JSON.stringify(new Uint8Array(2).buffer))', 'T(()=>JSON.stringify(new DataView(new ArrayBuffer(2))))', 'T(()=>JSON.stringify([new Uint8Array(1),new Uint16Array(1)]))',
  'T(()=>JSON.stringify(Object.assign(new Uint8Array(1),{a:1})))', 'T(()=>JSON.stringify(new Uint8Array(1),["0"]))', 'T(()=>JSON.stringify(new Uint8Array(2),["a"]))',
  'T(()=>JSON.stringify(Object.defineProperty(new Uint8Array(1),"toJSON",{value(){return "t"}})))',
  'T(()=>JSON.stringify(new Number(5)))', 'T(()=>JSON.stringify(new Number(-0)))', 'T(()=>JSON.stringify(new Number(NaN)))', 'T(()=>JSON.stringify(new Number(Infinity)))', 'T(()=>JSON.stringify(new Number(1e21)))',
  'T(()=>JSON.stringify(new String("")))', 'T(()=>JSON.stringify(new String("a\\nb")))', 'T(()=>JSON.stringify(new Boolean(0)))', 'T(()=>JSON.stringify(new Boolean("false")))', 'T(()=>JSON.stringify(Object(10n)))',
  'T(()=>JSON.stringify(Object(10n),(k,v)=>typeof v==="object"?String(v):v))', 'T(()=>JSON.stringify({a:Object(10n)},(k,v)=>typeof v==="object"&&v!==null&&!Object.keys(v).length?String(v):v))',
  'T(()=>{BigInt.prototype.toJSON=function(){return this.toString()+"n"};try{return JSON.stringify({a:1n,b:[2n],c:Object(3n)})}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{BigInt.prototype.toJSON=function(k){return typeof this+":"+typeof k+":"+k};try{return JSON.stringify({a:1n,b:[2n]})}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{BigInt.prototype.toJSON=function(){"use strict";return typeof this};try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{BigInt.prototype.toJSON=function(){return undefined};try{return JSON.stringify([1n,{a:2n}])}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{BigInt.prototype.toJSON=function(){return 3n};try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{BigInt.prototype.toJSON=function(){throw new Error("bj")};try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>{Object.defineProperty(BigInt.prototype,"toJSON",{get(){return function(){return "g"}},configurable:true});try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>JSON.stringify(1n))', 'T(()=>JSON.stringify([1n]))', 'T(()=>JSON.stringify({a:{b:[1n]}}))', 'T(()=>JSON.stringify(BigInt(2**53)))', 'T(()=>JSON.stringify(0n))', 'T(()=>JSON.stringify(-0n))',
  'T(()=>JSON.stringify({a:1n},null,2))', 'T(()=>JSON.stringify({a:1n},["a"]))', 'T(()=>JSON.stringify({a:1n},["b"]))', 'T(()=>JSON.stringify({a:1n},()=>1))', 'T(()=>JSON.stringify(1n,()=>1))',
  'T(()=>JSON.stringify({toJSON(){return 1n}}))', 'T(()=>JSON.stringify(Object.assign(Object(1n),{toJSON(){return 2}})))',
  'T(()=>{Date.prototype.toJSON=function(){return "dj"};try{return JSON.stringify([new Date(0),new Date(NaN)])}finally{delete Date.prototype.toJSON}})',
  'T(()=>{Object.defineProperty(Date.prototype,"toISOString",{value(){return "iso"},configurable:true,writable:true});try{return JSON.stringify(new Date(0))}finally{delete Date.prototype.toISOString}})',
  'T(()=>JSON.stringify(new Date(2e12)))', 'T(()=>JSON.stringify(new Date(-1e12)))', 'T(()=>JSON.stringify(new Date(-62167219200000)))', 'T(()=>JSON.stringify(new Date(-62167219200001)))',
  'T(()=>JSON.stringify(new Date(253402300799999)))', 'T(()=>JSON.stringify(new Date(253402300800000)))', 'T(()=>JSON.stringify(new Date(8.64e15)))', 'T(()=>JSON.stringify(new Date(8.64e15+1)))',
  'T(()=>JSON.stringify(new Date(-8.64e15)))', 'T(()=>JSON.stringify(new Date(-8.64e15-1)))', 'T(()=>JSON.stringify({d:new Date(0)},null,1))', 'T(()=>JSON.stringify(new Date(0),["x"]))',
  'T(()=>JSON.stringify(Object.assign(new Date(0),{toJSON:null})))', 'T(()=>JSON.stringify(Object.assign(new Date(0),{toJSON:undefined})))', 'T(()=>JSON.stringify(Object.assign(new Date(0),{x:1})))',
  'T(()=>JSON.stringify(Object.create(Date.prototype)))', 'T(()=>JSON.stringify({__proto__:Date.prototype}))', 'T(()=>Date.prototype.toJSON.call(new Date(0)))', 'T(()=>Date.prototype.toJSON.call({toISOString:()=>"z"}))');

// ---- 16. Strings e números na saída.
for (const s of ["\\ud800", "\\udc00", "\\ud800\\ud800\\udc00", "x\\ud800", "\\udc00x", "\\ud83d\\ude00\\ude00", "\\ud83d\\ud83d", "\\ude00\\ud83d"]) {
  add(`T(()=>JSON.stringify(["${s}"],null,"${s}"))`, `T(()=>JSON.stringify({"${s}":1},null,1))`, `T(()=>JSON.stringify("${s}",(k,v)=>v))`, `T(()=>JSON.stringify(Object("${s}")))`,
    `T(()=>JSON.stringify({a:"${s}"},["a"]))`, `T(()=>JSON.stringify(JSON.rawJSON('"${s}"')))`, `T(()=>[...JSON.stringify("${s}")].length)`);
}
for (const c of [0, 7, 8, 9, 10, 11, 12, 13, 31, 32, 34, 47, 92, 127, 0x2028, 0x2029, 0xfeff]) {
  add(`T(()=>JSON.stringify({["a"+String.fromCharCode(${c})]:String.fromCharCode(${c})}))`, `T(()=>JSON.stringify([String.fromCharCode(${c})],null,String.fromCharCode(${c})))`);
}
for (const n of ["1e21", "1e-7", "123e-20", "1.5e-10", "2**70", "-(2**70)", "0.1+0.7", "1/3", "-1/3", "1e300*10", "-(1e300*10)", "0/0", "5e-324", "2**-1074", "(2**53)+2", "1e22", "1e23", "4.35*100", "0.000001", "0.0000001"]) {
  add(`T(()=>JSON.stringify(${n}))`, `T(()=>JSON.stringify([${n}],null,1))`, `T(()=>JSON.stringify({a:${n}},["a"]))`, `T(()=>JSON.stringify(new Number(${n})))`);
}

// ---- 17. rawJSON e isRawJSON.
for (const s of ['1', '-1', '1.5e+10', '0', '-0', '"a"', '"a\\"b"', '"\\u0041"', '"\\ud800"', 'true', 'false', 'null', ' 1', '1 ', '\n1', '1\n', '\t"a"', '[]', '{}', '[1]', '{"a":1}', '',
  '1e', '01', '+1', '.5', '1.', 'NaN', 'Infinity', 'undefined', '"a', 'a"', "'a'", 'tru', 'nulll', '12345678901234567890123456789', '1e400', '" "', '"\\u2028"', '"\x01"', ' 1', '1 ']) {
  add(`T(()=>JSON.stringify(JSON.rawJSON(${q(s)})))`, `T(()=>JSON.stringify([JSON.rawJSON(${q(s)})],null,1))`, `T(()=>JSON.rawJSON(${q(s)}).rawJSON)`,
    `T(()=>JSON.stringify({a:JSON.rawJSON(${q(s)})},["a"]))`, `T(()=>JSON.isRawJSON(JSON.rawJSON(${q(s)})))`);
}
add('T(()=>JSON.rawJSON(1))', 'T(()=>JSON.rawJSON(undefined))', 'T(()=>JSON.rawJSON(null))', 'T(()=>JSON.rawJSON({}))', 'T(()=>JSON.rawJSON([]))', 'T(()=>JSON.rawJSON(Symbol()))',
  'T(()=>JSON.rawJSON(1n))', 'T(()=>JSON.rawJSON(new String("2")))', 'T(()=>JSON.rawJSON({toString(){return "[1]"}}))', 'T(()=>JSON.rawJSON({toString(){throw new Error("rj")}}))',
  'T(()=>new JSON.rawJSON("1"))', 'T(()=>JSON.rawJSON.call(null,"1"))', 'T(()=>JSON.isRawJSON.call(null,JSON.rawJSON("1")))', 'T(()=>JSON.isRawJSON(Object.create(JSON.rawJSON("1"))))',
  'T(()=>JSON.isRawJSON(Object.assign({},JSON.rawJSON("1"))))', 'T(()=>JSON.isRawJSON(JSON.parse(JSON.stringify({rawJSON:"1"}))))', 'T(()=>JSON.isRawJSON(new Proxy(JSON.rawJSON("1"),{})))',
  'T(()=>JSON.isRawJSON(structuredClone({a:1})))', 'T(()=>JSON.isRawJSON(1,2))', 'T(()=>JSON.isRawJSON(undefined))', 'T(()=>JSON.isRawJSON(Symbol()))', 'T(()=>JSON.isRawJSON(1n))',
  'T(()=>{var r=JSON.rawJSON("1");r.x=2;return Object.keys(r).join()+r.x})', 'T(()=>{"use strict";var r=JSON.rawJSON("1");r.x=2})', 'T(()=>{"use strict";var r=JSON.rawJSON("1");r.rawJSON="2"})',
  'T(()=>{"use strict";var r=JSON.rawJSON("1");delete r.rawJSON})', 'T(()=>{var r=JSON.rawJSON("1");return JSON.stringify(Object.getOwnPropertyDescriptors(r))})',
  'T(()=>Reflect.ownKeys(JSON.rawJSON("1")).length)', 'T(()=>Object.getPrototypeOf(JSON.rawJSON("1")))', 'T(()=>Object.isFrozen(JSON.rawJSON("1")))', 'T(()=>Object.isExtensible(JSON.rawJSON("1")))',
  'T(()=>JSON.stringify(JSON.rawJSON("1"),(k,v)=>2))', 'T(()=>JSON.stringify([1],(k,v)=>k==="0"?JSON.rawJSON("99999999999999999999"):v))', 'T(()=>JSON.stringify({a:1},(k,v)=>k==="a"?JSON.rawJSON("\\"x\\""):v))',
  'T(()=>JSON.stringify({a:JSON.rawJSON("1")},(k,v)=>typeof v))', 'T(()=>{var t=[];JSON.stringify({a:JSON.rawJSON("1")},(k,v)=>{t.push(typeof v+Object.keys(v||{}).join());return v});return t.join("|")})',
  'T(()=>JSON.stringify({toJSON(){return JSON.rawJSON("7")}},null,2))', 'T(()=>JSON.stringify({a:{toJSON(){return JSON.rawJSON("7")}}}))', 'T(()=>JSON.stringify(Object.assign(Object.create(null),{a:JSON.rawJSON("1")})))',
  'T(()=>JSON.stringify(JSON.parse("[1.0]",(k,v,c)=>typeof v==="number"?JSON.rawJSON(c.source):v)))', 'T(()=>JSON.stringify(JSON.parse("{\\"a\\":12345678901234567890}",(k,v,c)=>typeof v==="number"?JSON.rawJSON(c.source):v)))',
  'T(()=>JSON.stringify(JSON.parse("[1e2,0.50,-0]",(k,v,c)=>typeof v==="number"?JSON.rawJSON(c.source):v)))', 'T(()=>JSON.stringify(Object.assign(Object.create(JSON.rawJSON("1")),{a:1})))',
  'T(()=>JSON.stringify({a:JSON.rawJSON("1"),b:JSON.rawJSON("1")},["b"]))', 'T(()=>JSON.stringify(JSON.rawJSON("1"),["rawJSON"]))', 'T(()=>JSON.stringify(Object(JSON.rawJSON("1"))))');

// ---- 18. Meta de JSON.
add('JSON[Symbol.toStringTag]', 'T(()=>{"use strict";JSON[Symbol.toStringTag]="x"})', 'T(()=>{"use strict";delete JSON[Symbol.toStringTag]})', 'JSON.stringify(Object.getOwnPropertyDescriptor(JSON,Symbol.toStringTag))',
  'Object.prototype.toString.call(JSON)', 'Object.prototype.toString.call(Object.create(JSON))', 'String(Object.create(JSON))', 'Object.getOwnPropertyNames(JSON).join()',
  'JSON.stringify(Object.getOwnPropertyNames(JSON).map(k=>[k,typeof JSON[k]==="function"?JSON[k].length:-1]))',
  'JSON.stringify(Object.getOwnPropertyNames(JSON).map(k=>Object.getOwnPropertyDescriptor(JSON,k)).map(d=>[d.writable,d.enumerable,d.configurable]))',
  'Object.keys(JSON).length', 'JSON.stringify(JSON)', 'JSON.parse===JSON.parse', 'JSON.parse.call(null,"1")', 'JSON.parse.call(undefined,"1")', 'JSON.stringify.call(null,1)', 'JSON.stringify.call(1,1)',
  'JSON.parse.apply(null,["[1]"])[0]', 'T(()=>new JSON.parse("1"))', 'T(()=>new JSON.stringify(1))', 'T(()=>new JSON)', 'T(()=>JSON())', 'T(()=>JSON.parse.prototype)', 'T(()=>JSON.parse.toString().includes("native code"))',
  'Object.getOwnPropertyNames(JSON.parse).join()', 'Object.getOwnPropertyNames(JSON.stringify).join()', 'JSON.parse.length+JSON.stringify.length', 'Object.getPrototypeOf(JSON.parse)===Function.prototype',
  'T(()=>{var p=JSON.parse;JSON.parse=null;JSON.parse=p;return JSON.parse("1")})', 'T(()=>{var j=globalThis.JSON;globalThis.JSON=1;globalThis.JSON=j;return typeof JSON})',
  'T(()=>JSON.stringify.length)', 'T(()=>JSON.stringify())', 'T(()=>JSON.stringify(undefined))', 'T(()=>JSON.stringify(1,2,3,4))');

// ---- Execução.
const baseSources = new Set();
for (const program of knownPrograms("json_more_bun.tsv", ["json_bun.tsv"])) baseSources.add(JSON.stringify(program));
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const expr of unique) {
  if (HOST.test(expr)) continue;
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{${/^(var|class)\b/.test(expr) ? expr.replace(/;([^;]*)$/, ";return $1") : "return " + expr}})`);
  if (baseSources.has(JSON.stringify(source))) { dup++; continue; }
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) {
    dropped++;
    process.stderr.write("caminho no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos do base ${dup}\n`);
