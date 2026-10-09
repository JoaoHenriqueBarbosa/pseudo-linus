// Gera tests/golden/json_deep_bun.tsv: JSON.parse com reviver, números extremos, escapes inválidos, surrogates soltos,
// BOM, __proto__, ordem de chaves, profundidade, mensagens de SyntaxError, JSON.stringify com gap, replacer, toJSON,
// BigInt, Symbol, ciclos, Proxy, array-like, typed arrays, Date inválida, -0, stringify bem formado e JSON.rawJSON.
// Medido no bun 1.4.2. Programas cuja expressão já aparece nos goldens json_bun, json_grid_bun, json_more_bun e
// json_number_bun são descartados. Cada programa roda num bun filho novo (processo fresco), em paralelo.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-json-deep-golden.js > tests/golden/json_deep_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

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
const lit = (s) => JSON.stringify(s);
const W = (body) => `T(()=>{${body}})`;

// ---- 1. Reviver: ordem de chamada, holder, context, delete e mutação.
const revTexts = [
  "1", "null", '"s"', "true", "[]", "{}", "[1]", "[1,2,3]", '{"a":1}', '{"a":1,"b":2}', '{"b":1,"a":2}', '{"1":1,"0":2,"a":3}',
  '[[1],[2,[3]]]', '{"a":{"b":{"c":1}}}', '{"a":[1,{"b":2}],"c":null}', '[{"a":1},{"a":2}]', '{"a":1,"a":2}', '{"__proto__":1}',
  '[1.0,1e2,-0,0.5]', '["a","b"]', '{"x":[],"y":{}}', '[null,true,false]', '{"a":"1","b":"2"}', '[[],[[]]]',
  '{"2":1,"1":2,"b":3,"a":4}', '[1,2,3,4,5,6,7,8]', '{"":1}', '{"":{"":2}}', '[-1.5e-3,12345678901234567890]', '{"a":0.1,"b":1e400}',
];
const trace = 'var L=[];';
const revs = {
  log: `${trace}var r=JSON.parse(%T,function(k,v,c){L.push(k+"="+S(v)+(c&&"source" in c?"@"+c.source:""));return v});return L.join(" ")+" => "+S(r)`,
  keys: `${trace}JSON.parse(%T,function(k,v,c){L.push(k+":"+Object.keys(c||{}).join("/")+":"+Object.keys(this).join("/"));return v});return L.join(" ")`,
  holder: `${trace}JSON.parse(%T,function(k,v){L.push(k+"@"+(Array.isArray(this)?"A":"O")+Object.keys(this).length);return v});return L.join(" ")`,
  undef: `var r=JSON.parse(%T,function(k,v){return undefined});return S(r)`,
  dropKeys: `var r=JSON.parse(%T,function(k,v){return k==="a"||k==="1"||k==="0"?undefined:v});return S(r)`,
  wrap: `var r=JSON.parse(%T,function(k,v){return typeof v==="number"?[v]:v});return S(r)`,
  num: `var r=JSON.parse(%T,function(k,v){return typeof v==="number"?v*2:v});return S(r)`,
  str: `var r=JSON.parse(%T,function(k,v){return typeof v==="string"?v+"!":v});return S(r)`,
  addSibling: `${trace}var r=JSON.parse(%T,function(k,v){if(k!==""&&!this.zz){this.zz=1;L.push("add")}L.push(k);return v});return L.join()+" "+S(r)`,
  delSibling: `${trace}var r=JSON.parse(%T,function(k,v){L.push(k);if(k==="0"||k==="a")delete this[k==="0"?"1":"b"];return v});return L.join()+" "+S(r)`,
  delSelf: `${trace}var r=JSON.parse(%T,function(k,v){L.push(k);delete this[k];return v});return L.join()+" "+S(r)`,
  replaceHolder: `${trace}var r=JSON.parse(%T,function(k,v){L.push(k);if(Array.isArray(this)&&k==="0")this[1]="X";if(k==="a")this.b="Y";return v});return L.join()+" "+S(r)`,
  thisRoot: `var n=0;var r=JSON.parse(%T,function(k,v){if(k==="")return S(this)+"|"+(Object.getPrototypeOf(this)===Object.prototype)+"|"+Object.keys(this).join();return v});return S(r)`,
  throwAt: `${trace}try{JSON.parse(%T,function(k,v){L.push(k);if(L.length===2)throw new RangeError("stop");return v})}catch(e){L.push(e.name)}return L.join()`,
  freezeHolder: `${trace}var r=JSON.parse(%T,function(k,v){try{Object.freeze(this)}catch(e){}L.push(k);return v});return L.join()+" "+S(r)`,
  defineGetter: `${trace}var r=JSON.parse(%T,function(k,v){if(k==="0"||k==="a"){Object.defineProperty(this,k,{get(){L.push("g");return 9},configurable:true,enumerable:true})}L.push(k);return v});return L.join()+" "+S(r)`,
  nonEnum: `${trace}var r=JSON.parse(%T,function(k,v){if(k==="0"||k==="a"){Object.defineProperty(this,k,{enumerable:false})}return v});return S(r)`,
  retHolderArr: `${trace}var r=JSON.parse(%T,function(k,v){L.push(arguments.length);return v});return L.join("")`,
};
for (const text of revTexts) for (const [, body] of Object.entries(revs)) add(W(body.replace(/%T/g, lit(text))));
// reviver não chamável, argumentos e retornos do topo.
for (const r of ["undefined", "null", "1", '"f"', "{}", "[]", "true", "Symbol()", "Math.max", "class{}", "async function(){}", "function*(){}", "()=>1", "new Proxy(function(){},{})", "new Proxy({},{})", "function(){return 1}.bind(null)"]) {
  for (const t of ["[1,2]", '{"a":1}', "1", "null"]) add(W(`return S(JSON.parse(${lit(t)},${r}))`));
}
for (const t of ["[1,[2,3]]", '{"a":{"b":1}}', "[[[]]]"]) {
  add(
    W(`var r=JSON.parse(${lit(t)},function(k,v){return Array.isArray(v)?v.length:v});return S(r)`),
    W(`var seen=[];JSON.parse(${lit(t)},function(k,v){seen.push(typeof v);return v});return seen.join()`),
    W(`var o=JSON.parse(${lit(t)},function(k,v){return k===""?{wrapped:v}:v});return S(o)`),
    W(`var o=JSON.parse(${lit(t)},function(k,v){return k===""?this:v});return S(o)`),
    W(`var o=JSON.parse(${lit(t)},function(k,v){return k===""?this[""]:v});return S(o)`),
    W(`var o=JSON.parse(${lit(t)},()=>{throw new TypeError("x")})`),
    W(`var o=JSON.parse(${lit(t)},function(k,v){return {valueOf(){return 1}}});return typeof o`),
  );
}
// reviver devolvendo proxy, getter que lança, array crescendo.
add(
  W(`var r=JSON.parse("[1,2,3]",function(k,v){if(k==="0")this.length=1;return v});return S(r)`),
  W(`var r=JSON.parse("[1,2,3]",function(k,v){if(k==="0")this.push(4);return v});return S(r)`),
  W(`var L=[];var r=JSON.parse("[1,2,3]",function(k,v){L.push(k);if(k==="0")this.push(4);return v});return L.join()`),
  W(`var L=[];var r=JSON.parse("[1,2,3]",function(k,v){L.push(k);if(k==="0")this.length=1;return v});return L.join()`),
  W(`var L=[];var r=JSON.parse("[1,2,3]",function(k,v){L.push(k);if(k==="1")this.length=0;return v});return L.join()+S(r)`),
  W(`var L=[];var r=JSON.parse('{"a":1,"b":2}',function(k,v){L.push(k);if(k==="a")this.c=3;return v});return L.join()+S(r)`),
  W(`var L=[];var r=JSON.parse('{"a":1,"b":2}',function(k,v){L.push(k);if(k==="a"){delete this.b;this.b=5}return v});return L.join()+S(r)`),
  W(`var r=JSON.parse('{"a":{"x":1},"b":2}',function(k,v){if(k==="a")return new Proxy({},{ownKeys(){throw new EvalError("p")}});return v});return S(r)`),
  W(`var r=JSON.parse('{"a":[1,2]}',function(k,v){if(k==="a")return new Proxy([1,2],{});return v});return S(r)`),
  W(`var L=[];JSON.parse('{"a":{"x":1},"b":2}',function(k,v){L.push(k);if(k==="b")this.a=new Proxy({y:1},{get(t,p){L.push("get "+String(p));return t[p]},ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)}});return v});return L.join()`),
  W(`var o=JSON.parse('{"a":1}',function(k,v){return k==="a"?{toJSON(){return 7}}:v});return JSON.stringify(o)`),
  W(`var s=0;var r=JSON.parse('[[[[[[1]]]]]]',function(k,v){s++;return v});return s`),
  W(`return JSON.parse.length+JSON.parse.name+JSON.stringify.length+JSON.stringify.name`),
  W(`var f=JSON.parse;return S(f("[1]"))`),
  W(`return S(JSON.parse.call(null,"[1]"))`),
  W(`return S(Reflect.apply(JSON.parse,undefined,["{\\"a\\":1}"]))`),
  W(`return S(new JSON.parse("1"))`),
  W(`return S(Object.getOwnPropertyNames(JSON).sort())`),
  W(`return S(JSON[Symbol.toStringTag])+Object.prototype.toString.call(JSON)`),
  W(`return S(Object.getOwnPropertyDescriptor(JSON,Symbol.toStringTag))`),
);

// ---- 2. Números extremos.
const nums = [
  "0", "-0", "0.0", "-0.0", "0e0", "0e+0", "0e-0", "-0e5", "1", "-1", "9007199254740991", "9007199254740992", "9007199254740993", "-9007199254740993",
  "18446744073709551615", "18446744073709551616", "123456789012345678901234567890", "4294967295", "4294967296", "2147483647", "2147483648", "-2147483648", "-2147483649",
  "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308", "1.797693134862315807e308", "1.8e308", "1e309", "-1e309", "1e1000", "1e99999999999", "1E400",
  "5e-324", "4.9e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "2.5e-324", "1e-324", "1e-400", "1e-99999999999", "-1e-400", "0.000001", "0.0000001", "1e21", "1e20", "123456789e-20",
  "0.1", "0.2", "0.30000000000000004", "1.0000000000000002", "1.00000000000000011102230246251565404236316680908203125", "1.00000000000000011102230246251565404236316680908203126",
  "0.9999999999999999", "0.99999999999999994", "0.999999999999999944488848768742172978818416595458984375", "9.999999999999999e22", "1e23", "8.41e21", "2e-7", "1.5e-7",
  "100000000000000000000", "100000000000000000000000", "0.00000000000000000000001", "123.456e5", "123.456E-5", "1e+5", "1e-5", "1E5", "1e05", "1e005", "0e00",
  "3.141592653589793238462643383279", "2.718281828459045235360287471352662497757", "1".repeat(30), "1".repeat(400), "9".repeat(400), "0." + "0".repeat(400) + "1", "1e" + "9".repeat(30),
  "0." + "9".repeat(60), "4.35", "4.349999999999999", "0.5e1", "-0.5e1", "10e-1", "1e0", "1.e1", ".1", "-.1", "+1", "01", "-01", "00", "0x10", "0b1", "0o7", "1_000", "1__0", "1e", "1e+", "1e-", "1.", "-", "--1", "- 1", "1 .5", "1. 5", "Infinity", "-Infinity", "NaN", "1n", "1e1.5", "0e", "0.e1", "٣", "１", "1\u00a0", "1,5",
  "-0.0e-0", "0.1e1", "123e-2", "-123E+2", "12345678912345678912.5", "5e-325", "7e-324", "2.2250738585072014e-308", "2.2250738585072011e-308", "4.4501477170144028e-308",
];
for (const n of nums) {
  add(
    W(`return S(JSON.parse(${lit(n)}))`),
    W(`return S(JSON.parse(${lit("[" + n + "]")}))`),
    W(`return S(JSON.parse(${lit('{"k":' + n + "}")}))`),
    W(`var L=[];JSON.parse(${lit("[" + n + "]")},function(k,v,c){L.push(S(v)+"@"+(c&&c.source));return v});return L.join(" ")`),
    W(`var v=JSON.parse(${lit(n)});return S(1/v)+" "+S(JSON.stringify(v))`),
    W(`var v=JSON.parse(${lit(n)});return S(v)+" "+S(String(v))+" "+S(v.toString(2).length)+" "+S(v.toExponential())`),
  );
}
for (const n of ["-0", "0", "0.0", "1e999", "1e-999", "-1e999"]) add(W(`return S(JSON.stringify([${n}, -${n}, 1/${n}]))`));
add(
  W(`return S(JSON.stringify([NaN,Infinity,-Infinity,-0,0,1e21,1e-7,123456789012345680000,0.1+0.2,5e-324,1.7976931348623157e308]))`),
  W(`return S(JSON.stringify({a:NaN,b:-0,c:Infinity}))`), W(`return S(JSON.stringify(-0))`), W(`return S(JSON.stringify(NaN))`), W(`return S(JSON.stringify(Infinity))`),
  W(`return S(JSON.stringify(Object(-0)))`), W(`return S(JSON.stringify(Object(NaN)))`), W(`return S(JSON.stringify(new Number(1e21)))`),
  W(`return S(JSON.stringify([2**53,2**64,2**-1074,-(2**1023)*2,0.000001,0.0000001,1e-6,1e-7]))`),
  W(`return S(JSON.parse("[1e999,-1e999]").map(x=>1/x))`),
  W(`var o=JSON.parse('{"a":-0}');return S(Object.is(o.a,-0))`),
);

// ---- 3. Strings: escapes inválidos, surrogates soltos, controles.
const strBodies = [
  "", "a", "\\n", "\\t", "\\r", "\\b", "\\f", "\\/", "\\\\", '\\"', "\\'", "\\a", "\\v", "\\0", "\\x41", "\\u", "\\u1", "\\u12", "\\u123", "\\u1234", "\\u12G4", "\\uzzzz", "\\U0041", "\\u{41}", "\\u{1F600}",
  "\\ud800", "\\udc00", "\\ud800\\udc00", "\\udc00\\ud800", "\\ud800\\ud800", "\\ud800a", "a\\ud800", "\\ud800\\u0041", "\\ud83d\\ude00", "\\uD83D\\uDE00", "\\uD83D\\uDe00", "\\ud83d\\\\ude00", "\\udbff\\udfff", "\\udbff\\ue000",
  "\\u0000", "\\u0001", "\\u001f", "\\u007f", "\\u0080", "\\u00ff", "\\u2028", "\\u2029", "\\ufeff", "\\uffff", "\\ufffe", "\\ufffd", "\\u0041\\u0042", "\\u00e9", "\\u00E9", "\\u00e", "\\\\u0041", "\\\\\\u0041",
  "\\", "\\\\\\", "\\\n", "\\\r", "\\ ", "\\\u2028", "\\é", "\\😀", "\u0000", "\u0001", "\u001f", "\u0020", "\u007f", "\u0080", "\n", "\t", "\r", "\b", "\f", "\v", "\u2028", "\u2029", "\ufeff", "\u00a0", "\ud800", "\udc00", "\ud83d\ude00", "é", "😀", "日本語",
  "a\nb", "a\tb", "a\u0000b", "tab\there", "\\u0041\n", "line\\nbreak", "\\uD800\\uDC00\\uD800", "x\\ud800y\\udc00z", "\\ud83d\\ud83d\\ude00", "\\ude00\\ud83d",
];
for (const b of strBodies) {
  const text = '"' + b + '"';
  add(
    W(`return S(JSON.parse(${lit(text)}))`),
    W(`var s=JSON.parse(${lit(text)});return S(s.length)+" "+S(Array.from(s).map(c=>c.codePointAt(0).toString(16)).join())`),
    W(`return S(Object.keys(JSON.parse(${lit("{" + text + ":1}")})))`),
    W(`return S(JSON.parse(${lit("[" + text + "]")}))`),
    W(`var L=[];JSON.parse(${lit('{"k":' + text + "}")},function(k,v,c){L.push(S(v)+"@"+S(c&&c.source));return v});return L.join(" ")`),
  );
}
const loneUnits = ["\ud800", "\udbff", "\udc00", "\udfff", "\ud83d", "\ude00"];
for (const u of loneUnits) {
  const hex = u.charCodeAt(0).toString(16);
  add(
    W(`return S(JSON.stringify(${lit(u)}))`), W(`return S(JSON.stringify(${lit("a" + u + "b")}))`), W(`return S(JSON.stringify(${lit(u + u)}))`),
    W(`return S(JSON.stringify({[${lit(u)}]:${lit(u)}}))`), W(`return S(JSON.stringify([${lit(u)}]))`), W(`return S(JSON.stringify(${lit(u)}).length)`),
    W(`return S(JSON.stringify(new String(${lit(u)})))`), W(`return S(JSON.stringify(${lit(u)},null,2))`), W(`return S(JSON.stringify({a:${lit(u)}},null,${lit(u)}))`),
    W(`return S(JSON.stringify(String.fromCharCode(0x${hex},0x${hex === "d800" ? "dc00" : "41"})))`),
    W(`return S(JSON.parse(JSON.stringify(${lit(u)}))===${lit(u)})`), W(`return S(JSON.stringify(${lit(u)}+"\\ude00"))`), W(`return S(JSON.stringify("\\ud83d"+${lit(u)}))`),
    W(`return S(JSON.stringify({a:1},null,${lit(u)}))`), W(`return S(JSON.stringify([1],null,${lit(u + "x")}))`),
  );
}
add(
  W(`return S(JSON.stringify("\\ud83d\\ude00"))`), W(`return S(JSON.stringify("\\u2028\\u2029"))`), W(`return S(JSON.stringify("\\u007f\\u0080\\u009f"))`), W(`return S(JSON.stringify("\\u0000\\u001f"))`),
  W(`var s="";for(var i=0;i<32;i++)s+=String.fromCharCode(i);return S(JSON.stringify(s))`), W(`return S(JSON.stringify("\\"\\\\/\\b\\f\\n\\r\\t"))`),
  W(`var o={};o["\\ud800"]=1;o["\\u0000"]=2;o["\\n"]=3;return S(JSON.stringify(o))`), W(`var s="";for(var i=0xd7ff;i<0xe002;i++)s+=String.fromCharCode(i);return S(JSON.stringify(s))`),
  W(`return S(JSON.stringify("\\udbff\\udfff\\udbff"))`), W(`return S(JSON.stringify("\\udc00\\ud800"))`),
);

// ---- 4. BOM e espaço em branco ao redor de valores.
const ws = ["\ufeff", "\u00a0", "\u2028", "\u2029", "\u000b", "\u000c", "\u0085", "\u1680", "\u2000", "\u200b", "\u3000", " ", "\t", "\n", "\r", "\r\n", "\u0000", "\u001f", "\u180e", "\ufeff\ufeff", " \t\r\n ", ""];
const wsDocs = ["1", '"a"', "[]", '{"a":1}', "[1,2]", "null", "true"];
for (const w of ws) for (const d of wsDocs) {
  add(
    W(`return S(JSON.parse(${lit(w + d)}))`), W(`return S(JSON.parse(${lit(d + w)}))`), W(`return S(JSON.parse(${lit(w + d + w)}))`),
  );
}
for (const w of ws) {
  add(
    W(`return S(JSON.parse(${lit("[1" + w + ",2]")}))`), W(`return S(JSON.parse(${lit("[1," + w + "2]")}))`), W(`return S(JSON.parse(${lit("{" + w + '"a"' + w + ":" + w + "1" + w + "}")}))`),
    W(`return S(JSON.parse(${lit("[" + w + "]")}))`), W(`return S(JSON.parse(${lit("{" + w + "}")}))`), W(`return S(JSON.parse(${lit('"' + w + '"')}))`),
    W(`return S(JSON.parse(${lit("-" + w + "1")}))`), W(`return S(JSON.parse(${lit("tr" + w + "ue")}))`), W(`return S(JSON.parse(${lit(w)}))`),
  );
}

// ---- 5. __proto__ e chaves especiais.
const protoTexts = [
  '{"__proto__":1}', '{"__proto__":null}', '{"__proto__":{}}', '{"__proto__":{"x":1}}', '{"__proto__":[1]}', '{"__proto__":"s"}', '{"a":1,"__proto__":{"x":1}}', '{"__proto__":{"x":1},"a":1}',
  '{"__proto__":1,"__proto__":2}', '{"__proto__":{"__proto__":{"y":2}}}', '{"\\u005f\\u005fproto\\u005f\\u005f":{"x":1}}', '{"__PROTO__":{"x":1}}', '{"constructor":{"prototype":{"x":1}}}',
  '{"toString":1}', '{"hasOwnProperty":1}', '{"valueOf":null}', '{"length":3}', '{"constructor":1}', '{"prototype":1}', '{"__defineGetter__":1}',
  '[{"__proto__":{"x":1}}]', '{"a":{"__proto__":{"x":1}}}', '{"__proto__":{"x":1},"__proto__":null}',
];
for (const t of protoTexts) {
  const p = lit(t);
  add(
    W(`var o=JSON.parse(${p});return S(Object.getPrototypeOf(o)===Object.prototype)+S(Object.keys(o))+S(Object.getOwnPropertyNames(o))`),
    W(`var o=JSON.parse(${p});return S(o)+" "+S("x" in o)+" "+S(o.x)`),
    W(`var o=JSON.parse(${p});var d=Object.getOwnPropertyDescriptor(o,"__proto__");return S(d&&d.enumerable)+S(d&&d.writable)+S(d&&d.configurable)+S(d&&d.value)`),
    W(`var o=JSON.parse(${p});return S(JSON.stringify(o))`),
    W(`var o=JSON.parse(${p});var c=Object.assign({},o);return S(Object.getPrototypeOf(c)===Object.prototype)+S(Object.keys(c))+S(c.x)`),
    W(`var o=JSON.parse(${p});var c={...o};return S(Object.getPrototypeOf(c)===Object.prototype)+S(Object.keys(c))+S(c.x)`),
    W(`var L=[];var o=JSON.parse(${p},function(k,v){L.push(k);return v});return L.join()+S(Object.getPrototypeOf(o)===Object.prototype)`),
    W(`var o=JSON.parse(${p},function(k,v){return k==="__proto__"?undefined:v});return S(o)+S(Object.keys(o))`),
    W(`var o=JSON.parse(${p},function(k,v){return k==="__proto__"?"r":v});return S(o)+S(Object.keys(o))`),
    W(`var o=JSON.parse(${p});for(var k in o)if(typeof o[k]==="number")return k;return S(Object.entries(o))`),
    W(`var o=JSON.parse(${p});return S(structuredClone===undefined?0:Object.getPrototypeOf(JSON.parse(JSON.stringify(o)))===Object.prototype)`),
  );
}
add(
  W(`var o={};o["__proto__"]=1;return S(Object.keys(o))`), W(`var o={["__proto__"]:1};return S(Object.keys(o))+S(Object.getPrototypeOf(o)===Object.prototype)`),
  W(`var o={__proto__:null,a:1};return S(JSON.stringify(o))`), W(`return S(JSON.stringify(Object.create(null,{a:{value:1,enumerable:true}})))`),
  W(`return S(JSON.stringify({["__proto__"]:{x:1}}))`), W(`return S(JSON.stringify({__proto__:{x:1}}))`), W(`return S(JSON.stringify(JSON.parse('{"__proto__":[]}')))`),
  W(`var a=JSON.parse('{"__proto__":{"polluted":1}}');var b=Object.assign({},a);return S(({}).polluted)+S(b.polluted)`),
  W(`return S(JSON.parse('{"a":1}',function(k,v){if(k==="a")this.__proto__={z:1};return v}).z)`),
);

// ---- 6. Chaves numéricas e ordem.
const keyLists = [
  ["b", "a"], ["1", "0"], ["0", "1", "a"], ["a", "1", "b", "0"], ["10", "9", "2", "1"], ["4294967294", "4294967295", "4294967296"], ["-1", "0", "1"], ["01", "1", "001"], ["1.5", "1", "2"], ["1e3", "1000", "999"],
  ["", "0", "a"], ["0", "", "00"], ["2", "1", "b", "a", "0"], ["9007199254740991", "9007199254740992", "1"], ["0", "-0", "+0"], ["Infinity", "NaN", "0"], ["x", "x", "y", "x"], ["1", "1", "0"], ["a", "A", "á", "Z"],
  ["length", "0", "1"], ["4294967295", "0"], ["100", "20", "3", "b", "a", "2000"], ["3", "2", "1", "0", "-1", "1.0"], ["\u0661", "1"], ["1 ", " 1", "1"], ["0x1", "1"], ["b", "10", "a", "9"],
  ["2147483647", "2147483648", "1"], ["4294967294", "1", "a"], ["999999999999", "99999999999", "1"], ["00", "0", "000"],
];
for (const keys of keyLists) {
  const text = "{" + keys.map((k, i) => lit(k) + ":" + i).join(",") + "}";
  const p = lit(text);
  add(
    W(`return S(Object.keys(JSON.parse(${p})))`),
    W(`return S(JSON.parse(${p}))`),
    W(`return S(JSON.stringify(JSON.parse(${p})))`),
    W(`var L=[];JSON.parse(${p},function(k,v){L.push(k);return v});return L.join("|")`),
    W(`var r=[];for(var k in JSON.parse(${p}))r.push(k);return r.join("|")`),
    W(`return S(Object.entries(JSON.parse(${p})))`),
    W(`return S(Object.getOwnPropertyNames(JSON.parse(${p})))`),
    W(`return S(JSON.stringify(JSON.parse(${p}),null,1))`),
    W(`return S(JSON.stringify(JSON.parse(${p}),Object.keys(JSON.parse(${p})).reverse()))`),
    W(`var o={};${keys.map((k, i) => `o[${lit(k)}]=${i};`).join("")}return S(JSON.stringify(o))`),
    W(`var o=JSON.parse(${p});var x=JSON.parse(JSON.stringify(o));return S(Object.keys(x))`),
  );
  const arrText = "[" + keys.map((k, i) => i).join(",") + "]";
  add(W(`var a=JSON.parse(${lit(arrText)});${keys.map((k, i) => `a[${lit(k)}]=${i + 10};`).join("")}return S(JSON.stringify(a))+a.length`));
}
add(
  W(`var o={};o[2]=1;o.b=1;o[1]=1;o.a=1;o[Symbol.for("s")]=1;return S(JSON.stringify(o))+S(Reflect.ownKeys(o))`),
  W(`var a=[];a[5]=1;a.x=2;return S(JSON.stringify(a))`), W(`var a=[1,2];a.length=4;return S(JSON.stringify(a))`), W(`return S(JSON.stringify([,]))+S(JSON.stringify([,,1]))`),
  W(`var o={};for(var i=0;i<20;i++)o["k"+i]=i;for(var i=19;i>=0;i--)o[i]=i;return S(JSON.stringify(o).length)+S(Object.keys(o).slice(18,22))`),
  W(`var o=JSON.parse('{"b":1,"a":2,"b":3}');return S(o)+S(Object.keys(o))`), W(`var o=JSON.parse('{"1":1,"1":2,"0":3}');return S(o)`),
);

// ---- 7. Profundidade.
const depths = [1, 2, 10, 50, 100, 500, 1000, 1500, 2000, 3000, 5000, 10000, 20000, 50000, 100000, 1000000];
for (const n of depths) {
  const outcome = `try{var r=%E;var d=0;while(r!==undefined&&d<2000000){r=Array.isArray(r)?r[0]:r.a;d++}return "ok "+(d>=n)}catch(e){return e.name}`;
  add(
    W(`var n=${n};${outcome.replace("%E", 'JSON.parse("[".repeat(n)+"]".repeat(n))')}`),
    W(`var n=${n};${outcome.replace("%E", 'JSON.parse(\'{"a":\'.repeat(n)+"1"+"}".repeat(n))')}`),
    W(`var n=${n};try{var s=JSON.stringify((function(){var x=[];for(var i=0;i<n;i++)x=[x];return x})());return "ok "+s.length}catch(e){return e.name}`),
    W(`var n=${n};try{var s=JSON.stringify((function(){var x={};for(var i=0;i<n;i++)x={a:x};return x})());return "ok "+s.length}catch(e){return e.name}`),
    W(`var n=${n};try{JSON.parse("[".repeat(n)+"1");return "no"}catch(e){return e.name}`),
    W(`var n=${n};try{var s=JSON.stringify((function(){var x=[];for(var i=0;i<n;i++)x=[x];return x})(),null,1);return "ok "+s.length}catch(e){return e.name}`),
    W(`var n=${n};try{var c=0;JSON.parse("[".repeat(n)+"]".repeat(n),function(k,v){c++;return v});return "ok "+c}catch(e){return e.name}`),
  );
}
for (const n of [1, 2, 3, 5, 8, 16]) {
  add(
    W(`return S(JSON.parse(${lit("[".repeat(n) + "1" + "]".repeat(n))}))`), W(`return S(JSON.parse(${lit('{"a":'.repeat(n) + "1" + "}".repeat(n))}))`),
    W(`return S(JSON.parse(${lit("[".repeat(n) + "1" + "]".repeat(n - 1))}))`), W(`return S(JSON.parse(${lit("[".repeat(n - 1) + "1" + "]".repeat(n))}))`),
    W(`return S(JSON.parse(${lit("[".repeat(n) + "]".repeat(n))}))`), W(`return S(JSON.parse(${lit('{"a":'.repeat(n) + "}".repeat(n))}))`),
  );
}
add(
  W(`var a=[];a[0]=a;return S(JSON.stringify([[[1]]],null,2))`), W(`var s="x".repeat(1e6);return JSON.stringify(s).length+" "+JSON.parse(JSON.stringify(s)).length`),
  W(`var a=new Array(1e5).fill(1);return JSON.stringify(a).length+" "+JSON.parse(JSON.stringify(a)).length`),
  W(`var o={};for(var i=0;i<5e4;i++)o["k"+i]=i;return JSON.stringify(o).length+" "+Object.keys(JSON.parse(JSON.stringify(o))).length`),
  W(`var s="[".repeat(1e5);try{JSON.parse(s)}catch(e){return e.name}return "no"`),
);

// ---- 8. Mensagens de SyntaxError.
const bad = [
  "", " ", "\n", "[", "]", "{", "}", ",", ":", '"', "'a'", "{'a':1}", "[1,]", "[,1]", "[1,,2]", "{,}", '{"a"}', '{"a":}', '{"a":1,}', '{"a":1 "b":2}', '{a:1}', '{1:1}', "{null:1}", "[1 2]", "[1;2]", "1 2", "1,2", "{}{}", "[][]", "nul", "nulll", "NULL", "tru", "True", "false1", "undefined", "NaN", "Infinity", "-Infinity", "-", "+", "- 1", "1e", "1e+", ".5", "5.", "0x1", "01", "-01", "1__0",
  '"\\x"', '"\\u12"', '"a', '"a\nb"', '"a\tb"', '"\\', '"\\u"', "/*c*/1", "//c\n1", "[1]//c", "[1/*c*/]", "#", "@", "`a`", "function(){}", "()=>1", "new Date", "1+1", "a", "A", "x=1", "{} ", " {", "[}", "{]", "[{]", "[[}]", '{"a":[}', '{"a":{]}', '["a"', '["a",', '{"a":1', '{"a":1,', '{"a"', '{"a":', "[1", "[1,", "{\"a\":\"b", '"\ud800', "\u2028", "\u2029", "\ufeff", "\u0000", "1\u0000", "\u00001", "[\u0000]", '"\u0000"', "\u007f", "é", "[é]", '{"é":é}', "😀", '"😀', '["\ud83d', "[1,2,3,", "[[[[", "]]]]", "}}}}", "{{{{", '{"a":{"b":{"c":', "tRue", "fAlse", "nUll", "[tru]", '{"a":tru}', "[nul,1]", "[1,nul]", "-a", "--1", "1-", "1.e1", "1.2.3", "1e1e1", "0.0.0", "1ee1", "1e+-1", "0e", "0e+", "-.5", "- .5", "1.5e", "2e-", "[-]", "[1.]", "[.1]", "[+1]", "[01]", "[1e]", '{"a":01}', '{"a":-}', '{"a":.5}',
];
for (const t of bad) add(W(`return S(JSON.parse(${lit(t)}))`));
// truncamento em todos os prefixos de documentos válidos e inserção de lixo.
const docs = ['{"a":[1,2,{"b":null}],"c":"d\\n","e":-1.5e+3,"f":true}', '[1,"two",[3,[4]],{"five":5},false,null]', '"a\\u00e9\\ud83d\\ude00b"', "-12.5e-3", '{"k":{"k":{"k":[]}}}'];
for (const d of docs) {
  for (let i = 0; i < d.length; i++) {
    add(W(`return S(JSON.parse(${lit(d.slice(0, i))}))`));
  }
  for (const junk of ["x", "}", "]", ",", "1", '"', "\u0000", " x", "\n[", "//", "undefined", "true"]) {
    for (const i of [0, 1, Math.floor(d.length / 3), Math.floor(d.length / 2), d.length - 1, d.length]) add(W(`return S(JSON.parse(${lit(d.slice(0, i) + junk + d.slice(i))}))`));
  }
  for (const i of [1, Math.floor(d.length / 2), d.length - 1]) add(W(`return S(JSON.parse(${lit(d.slice(0, i) + d.slice(i + 1))}))`));
}
// argumentos que não são string.
const nonStr = ["undefined", "null", "true", "false", "0", "-0", "1", "1.5", "NaN", "Infinity", "1n", "Symbol()", "Symbol.iterator", "{}", "[]", "[1]", "[[1]]", "['a']", "[null]", "[undefined]", "{toString(){return '[2]'}}", "{valueOf(){return '[3]'}}", "{toString(){return {}},valueOf(){return '4'}}", "{toString(){throw new RangeError('ts')}}", "{[Symbol.toPrimitive](){return '5'}}", "new String('[6]')", "new Number(7)", "new Boolean(false)", "function(){}", "()=>1", "class{}", "new Date(0)", "new Uint8Array(0)", "new Uint8Array([49])", "new ArrayBuffer(1)", "/x/", "new Map", "Object.create(null)", "new Proxy({},{})", "new Proxy([],{})", "new Error('e')", "'\\u00001'"];
for (const a of nonStr) add(W(`return S(JSON.parse(${a}))`), W(`return S(JSON.parse(${a},function(k,v){return v}))`), W(`return S(JSON.parse())`));
add(
  W(`return S(JSON.parse("1","x","y"))`), W(`return S(JSON.parse("[1]",undefined,function(){}))`),
  W(`try{JSON.parse("{")}catch(e){return S(e instanceof SyntaxError)+S(e.constructor===SyntaxError)+S(Object.getOwnPropertyNames(e).sort())+S(typeof e.stack)}`),
  W(`try{JSON.parse("{")}catch(e){return S(Object.prototype.hasOwnProperty.call(e,"message"))+S(e.name)+S(String(e))}`),
  W(`try{JSON.parse("[1,")}catch(e){return S(e.message)}`),
  W(`try{JSON.parse("")}catch(e){return S(e.message)}`),
  W(`try{JSON.parse(undefined)}catch(e){return S(e.message)}`),
  W(`try{JSON.parse("{\\"a\\":}")}catch(e){return S(e.message)+S(e.line)+S(e.column)}`),
);

// ---- 9. stringify com gap exótico.
const gaps = [
  "undefined", "null", "0", "1", "2", "5", "9", "10", "11", "12", "100", "1e9", "-1", "-5", "0.5", "1.5", "1.9", "2.9999", "NaN", "Infinity", "-Infinity", "-0", "10.9", "1e-7", '"1"', '"-1"', '" "', '"  "', '"\\t"', '"\\n"', '"\\r\\n"', '""', '"x"', '"ab"', '"abcdefghij"', '"abcdefghijk"', '"abcdefghijklmnop"',
  '"\\u00e9"', '"\\ud83d\\ude00"', '"\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00"', '"\\u0000"', '"\\u2028"', '"\\""', '"\\\\"', "true", "false", "{}", "[]", "[2]", "{valueOf(){return 3}}", "{toString(){return 'ts'}}",
  "new Number(3)", "new Number(-3)", "new Number(NaN)", "new Number(20)", "new String('ab')", "new String('')", "new Boolean(true)", "Object(5n)", "5n", "Symbol()", "()=>2", "new Date(0)", "new Number(2.5)", "Object.assign(new Number(1),{valueOf(){return 4}})", "Object.assign(new String('p'),{toString(){return 'q'}})",
  "new Proxy(new Number(2),{})", "new Proxy({},{})", "Object(Symbol())", "{[Symbol.toPrimitive](){return 3}}",
];
const vals = ["null", "1", '"s"', "[]", "{}", "[1]", '{"a":1}', "[1,[2,[3]]]", '{"a":{"b":[1,{}],"c":[]},"d":null}', '[[],{},[[]],{"a":{}}]', "[undefined,function(){},Symbol()]", '{"a":undefined,"b":function(){},"c":Symbol(),"d":1}'];
for (const g of gaps) for (const v of vals) add(W(`return S(JSON.stringify(${v},null,${g}))`));
for (const g of gaps.slice(0, 40)) add(
  W(`return S(JSON.stringify({a:[1,2],b:{c:3}},["a","b","c"],${g}))`), W(`return S(JSON.stringify({a:[1,2],b:{c:3}},function(k,v){return v},${g}))`),
  W(`return S(JSON.stringify([{a:1},[]],undefined,${g}))`),
);
add(
  W(`var L=[];JSON.stringify({a:1},null,{valueOf(){L.push("v");return 2},toString(){L.push("s");return "x"}});return L.join()`),
  W(`var L=[];JSON.stringify({a:1},null,new Proxy(new Number(1),{get(t,k){L.push(String(k));return t[k]}}));return L.join()`),
  W(`var L=[];JSON.stringify({a:1},{},{toString(){L.push("s");return "x"}});return L.join()`),
  W(`return S(JSON.stringify({a:1},null,{valueOf(){throw new RangeError("g")}}))`),
  W(`return S(JSON.stringify({a:1},null,{toString(){throw new RangeError("g")}}))`),
  W(`return S(JSON.stringify({a:1},null,new String("ab")))`), W(`return S(JSON.stringify({a:1},null,"ab").split("\\n"))`),
  W(`return S(JSON.stringify([1,{a:[]}],null,"--").replace(/\\n/g,"/"))`), W(`return S(JSON.stringify({a:{}},null,3).length)`),
);

// ---- 10. replacer array e função.
const repArrays = [
  '["a"]', '["a","a"]', '["a","b","a"]', '["b","a"]', '[]', '[[]]', '["a",["b"]]', "[1]", "[1,1]", "[1,'1']", "['1',1]", "[0]", "[-0]", "[1.5]", "[1e21]", "[NaN]", "[Infinity]", '[new String("a")]', '[new Number(1)]', '[new String("a"),"a"]',
  '[new Number(1),1,"1"]', "[{}]", "[{toString(){return 'a'}}]", "[{valueOf(){return 1}}]", "[true]", "[null]", "[undefined]", "[Symbol()]", "[Symbol.iterator]", "[()=>1]", "[1n]", "[new Boolean(true)]", "[new Date(0)]", "[,'a']", "['a',,'b']",
  'Object.assign(["a"],{length:3})', 'Object.assign(["a"],{1:"b"})', 'new Proxy(["a","b"],{})', "new Uint8Array([1,2])", '{length:1,0:"a"}', "{0:'a',length:2,1:'b'}", "'a'", "new String('a')", "{}", "new Set(['a'])", "['a','','b']", '["a\\n"]', "[Object('a')]", "[new String('')]", "['__proto__']", "['constructor']",
  '["a",{toString(){throw new EvalError("e")}}]', 'new Proxy(["a"],{get(t,k){if(k==="length")return 1;return t[k]}})',
];
const repVals = ['{"a":1,"b":2,"1":3,"c":{"a":4,"b":5}}', '[{"a":1,"b":2},{"a":3}]', '{"a":[{"a":1,"b":2}],"b":null}', '{"1":"x","a":"y"}', '{"":1,"a":2}', '{"a":{"a":{"a":1}}}'];
for (const r of repArrays) for (const v of repVals) add(W(`return S(JSON.stringify(${v},${r}))`));
for (const r of repArrays) add(W(`return S(JSON.stringify({a:1,b:2,1:3},${r},2))`), W(`return S(JSON.stringify([{a:1,b:2}],${r},"-"))`));
const repFns = [
  "function(k,v){return v}", "function(k,v){return undefined}", "function(k,v){return k===''?v:undefined}", "function(k,v){return typeof v==='number'?v+1:v}", "function(k,v){return typeof v==='string'?undefined:v}",
  "function(k,v){return Array.isArray(v)?v.length:v}", "function(k,v){return k==='a'?undefined:v}", "function(k,v){return k==='a'?[v]:v}", "function(k,v){return k===''?{x:v}:v}", "function(k,v){return k===''?[v]:v}",
  "function(k,v){return typeof v==='object'&&v?Object.keys(v):v}", "function(k,v){return typeof v==='bigint'?String(v):v}", "function(k,v){return v&&v.toJSON?'TJ':v}", "function(k,v){return k+'='+(typeof v)}", "function(k,v){return this===undefined?'u':v}",
  "function(k,v){return Symbol()}", "function(k,v){return function(){}}", "function(k,v){return k===''?v:NaN}", "function(k,v){return k===''?v:-0}", "function(k,v){return k===''?v:new Number(1)}", "function(k,v){return k===''?v:new String('s')}",
  "function(k,v){return k===''?v:new Boolean(false)}", "function(k,v){return k===''?v:Object(1n)}", "function(k,v){if(k==='a')delete this.b;return v}", "function(k,v){if(k==='a')this.z=1;return v}", "function(k,v){throw new RangeError('rep:'+k)}",
  "(k,v)=>v", "function(){return arguments.length}", "function(k,v){return k===''?v:arguments[2]}", "function(k,v){return Object(v)===v&&k!==''?'obj':v}", "function(k,v){return typeof v==='number'?{n:v}:v}", "function(k,v){return typeof v==='number'?[v,v]:v}",
  "new Proxy(function(k,v){return v},{})", "function(k,v){return k===''?v:{toJSON(){return 'tj'}}}", "function(k,v){return k===''?v:{toJSON(){throw new EvalError('tj')}}}", "Math.max", "String", "Boolean", "Number", "Object", "Array", "Symbol",
];
const repV2 = ['{"a":1,"b":[1,2],"c":"s","d":null,"e":true}', "[1,'a',null,[2]]", "1", "'str'", "null", "{a:{b:{c:1}}}", "[]", "{}", "{a:undefined,b:1n===1n}", "[new Date(0)]", "{a:[{b:1},{b:2}]}"];
for (const f of repFns) for (const v of repV2) add(W(`return S(JSON.stringify(${v.startsWith("{") && !v.startsWith('{"') ? "(" + v + ")" : v.startsWith('{"') ? "JSON.parse(" + lit(v) + ")" : v},${f}))`));
for (const f of repFns.slice(0, 20)) add(
  W(`var L=[];JSON.stringify({a:[1,{b:2}],c:3},function(k,v){L.push((Array.isArray(this)?"A":"O")+":"+k);return v});return L.join(" ")`),
  W(`var L=[];JSON.stringify([1,[2,{x:3}]],function(k,v){L.push(typeof k+":"+k);return v});return L.join(" ")`),
  W(`var f=${f};return S(JSON.stringify({a:1},f,1))`),
  W(`var f=${f};return S(JSON.stringify([1,{a:2}],f,"\\t"))`),
);
add(
  W(`var L=[];JSON.stringify({a:{toJSON(k){L.push("tj:"+k);return 1}},b:2},function(k,v){L.push("r:"+k+":"+S(v));return v});return L.join(" ")`),
  W(`var L=[];JSON.stringify(Object.assign(new Date(0),{x:1}),function(k,v){L.push(k+":"+typeof v);return v});return L.join(" ")`),
  W(`var L=[];JSON.stringify({a:1},function(k,v){L.push(arguments.length+typeof arguments[0]);return v});return L.join(" ")`),
  W(`var o={a:1};var r=JSON.stringify(o,function(k,v){return k===""?this:v});return S(r)`),
  W(`var o={a:1};var r=JSON.stringify(o,function(k,v){return k===""?(this[""]===o):v});return S(r)`),
  W(`var r=JSON.stringify({a:1},function(k,v){return k===""?Object.getPrototypeOf(this)===Object.prototype:v});return S(r)`),
  W(`var L=[];JSON.stringify([{}],function(k,v){L.push(Object.getOwnPropertyNames(this).join());return v});return L.join(" ")`),
  W(`return S(JSON.stringify({a:1},[],"x"))+S(JSON.stringify([1],[],"x"))`), W(`return S(JSON.stringify({a:1,b:2},["b"],undefined))`),
  W(`return S(JSON.stringify({a:1,b:2},{a:1}))`), W(`return S(JSON.stringify({a:1,b:2},1))`), W(`return S(JSON.stringify({a:1,b:2},"a"))`), W(`return S(JSON.stringify({a:1,b:2},null))`), W(`return S(JSON.stringify({a:1},true))`),
);

// ---- 11. toJSON.
const tjs = [
  "{toJSON(){return 1}}", "{toJSON(){return undefined}}", "{toJSON(){return null}}", "{toJSON(){return this}}", "{toJSON:1}", "{toJSON:null}", "{toJSON:undefined}", "{toJSON:{}}", "{toJSON:'x'}", "{toJSON(k){return k}}", "{toJSON(k){return arguments.length}}",
  "{toJSON(k){return typeof k}}", "{toJSON(){return {toJSON(){return 5}}}}", "{toJSON(){return {a:1,toJSON:null}}}", "{toJSON(){throw new RangeError('tj')}}", "{get toJSON(){throw new EvalError('get')}}", "{get toJSON(){return ()=>'g'}}", "{toJSON(){return Symbol()}}",
  "{toJSON(){return function(){}}}", "{toJSON(){return 1n}}", "{toJSON(){return NaN}}", "{toJSON(){return -0}}", "{toJSON(){return new Number(2)}}", "{toJSON(){return new String('t')}}", "{toJSON(){return [1,{toJSON(){return 'in'}}]}}",
  "Object.create({toJSON(){return 'proto'}})", "Object.create(Object.create({toJSON(){return 'deep'}}))", "Object.setPrototypeOf({a:1},{toJSON(){return 'p2'}})", "Object.create({toJSON:1})", "Object.create({get toJSON(){return ()=>'pg'}})",
  "Object.assign(function(){},{toJSON(){return 'fn'}})", "Object.assign([1,2],{toJSON(){return 'arr'}})", "Object.assign(new Date(0),{toJSON(){return 'dt'}})", "Object.assign(new Number(1),{toJSON(){return 'nm'}})", "Object.assign(new String('s'),{toJSON(){return 'st'}})",
  "Object.assign(/x/,{toJSON(){return 're'}})", "Object.assign(new Map,{toJSON(){return 'mp'}})", "Object.assign(new Error('m'),{toJSON(){return 'er'}})", "new Proxy({toJSON(){return 'px'}},{})", "new Proxy({},{get(t,k){return k==='toJSON'?()=>'pg2':undefined}})",
  "new Proxy({a:1},{get(t,k){return k==='toJSON'?undefined:t[k]}})", "{toJSON(){return this.x},x:3}", "{toJSON:function(){return Object.keys(this)},a:1,b:2}", "{toJSON:()=>'arrow'}", "{toJSON:class{}}", "{toJSON:async()=>1}", "{toJSON:function*(){}}",
  "{toJSON:new Proxy(function(){return 'pf'},{})}", "{toJSON:Math.abs}", "{toJSON:Date.prototype.toJSON}", "{toJSON:Date.prototype.toISOString}", "{toJSON:Object}", "{toJSON:String}",
];
for (const t of tjs) add(
  W(`return S(JSON.stringify(${t}))`), W(`return S(JSON.stringify([${t}]))`), W(`return S(JSON.stringify({k:${t}}))`), W(`return S(JSON.stringify({k:${t}},null,1))`),
  W(`return S(JSON.stringify({k:${t}},function(k,v){return v}))`), W(`return S(JSON.stringify({k:${t}},["k"]))`), W(`var L=[];JSON.stringify([${t}],function(k,v){L.push(k+typeof v);return v});return L.join()`),
);
const protoTj = [
  "Number.prototype", "String.prototype", "Boolean.prototype", "BigInt.prototype", "Symbol.prototype", "Object.prototype", "Array.prototype", "Function.prototype", "Date.prototype", "RegExp.prototype", "Map.prototype", "Error.prototype",
];
const protoVals = ["1", "0", "-0", "NaN", "'s'", "''", "true", "false", "1n", "Symbol('q')", "null", "undefined", "{}", "[]", "[1]", "{a:1}", "function(){}", "new Date(0)", "/x/", "new Number(3)", "new String('x')"];
for (const P of protoTj) for (const impl of ["function(k){return 'T'+typeof this+':'+String(k)}", "function(){return typeof this}", "function(){return this}", "function(){return undefined}", "function(){throw new RangeError('pj')}", "1", "function(){'use strict';return typeof this}"]) {
  for (const v of protoVals) add(W(`var P=${P};var had=Object.getOwnPropertyDescriptor(P,"toJSON");try{Object.defineProperty(P,"toJSON",{value:${impl},configurable:true,writable:true});return S(JSON.stringify(${v}))+S(JSON.stringify([${v}]))+S(JSON.stringify({k:${v}}))}finally{delete P.toJSON}`));
}
add(
  W(`return S(JSON.stringify(new Date(NaN)))+S(JSON.stringify([new Date(NaN)]))+S(JSON.stringify({d:new Date(NaN)}))`), W(`return S(new Date(NaN).toJSON())+S(new Date(0).toJSON())+S(new Date(8.64e15).toJSON())+S(new Date(-8.64e15).toJSON())`),
  W(`return S(JSON.stringify(new Date(8.64e15+1)))`), W(`return S(JSON.stringify(new Date(-62198755200000)))`), W(`return S(JSON.stringify(new Date(-62198755200001)))`), W(`return S(JSON.stringify(new Date(253402300800000)))`),
  W(`return S(Date.prototype.toJSON.call({toISOString(){return "iso"}}))`), W(`return S(Date.prototype.toJSON.call({toISOString(){return "iso"},valueOf(){return NaN}}))`), W(`return S(Date.prototype.toJSON.call({toISOString:1}))`),
  W(`return S(Date.prototype.toJSON.call({}))`), W(`return S(Date.prototype.toJSON.call(1))`), W(`return S(Date.prototype.toJSON.call(null))`), W(`return S(Date.prototype.toJSON.call(undefined))`), W(`return S(Date.prototype.toJSON.call("s"))`),
  W(`return S(Date.prototype.toJSON.call({valueOf(){return Infinity},toISOString(){return "x"}}))`), W(`return S(Date.prototype.toJSON.call({[Symbol.toPrimitive](h){return h==="number"?5:"str"},toISOString(){return "tp"}}))`),
  W(`return S(Date.prototype.toJSON.call({valueOf(){return "s"},toISOString(){return "vs"}}))`), W(`return S(Date.prototype.toJSON.call(Object.create(new Date(0))))`), W(`return S(Date.prototype.toJSON.call(new Proxy(new Date(0),{})))`),
  W(`return S(Date.prototype.toJSON.length)+S(Date.prototype.toJSON.name)`), W(`return S(Object.getOwnPropertyNames(Date.prototype).includes("toJSON"))`),
  W(`return S(JSON.stringify({a:new Date(0)},function(k,v){return typeof v}))`), W(`return S(JSON.stringify({a:new Date(0)},function(k,v){return this[k] instanceof Date})) `),
);

// ---- 12. Valores diversos: BigInt, Symbol, ciclos, Proxy, array-like, typed arrays, boxed, etc.
const kinds = [
  "undefined", "null", "true", "false", "0", "-0", "1", "-1", "1.5", "NaN", "Infinity", "-Infinity", "''", "'a'", "'\\ud800'", "1n", "0n", "-1n", "2n**64n", "BigInt(Number.MAX_SAFE_INTEGER)", "Symbol()", "Symbol('d')", "Symbol.for('f')", "Symbol.iterator",
  "function(){}", "()=>1", "class{}", "async function(){}", "function*(){}", "Math.max", "{}", "[]", "[1]", "[undefined]", "[,]", "[,1]", "[null]", "{a:undefined}", "{a:1n}", "{a:Symbol()}", "{[Symbol()]:1}", "{a(){}}", "{a:()=>1}", "{get a(){return 1}}", "{get a(){throw new RangeError('g')}}",
  "{get a(){return 1n}}", "{set a(v){}}", "{a:1,get b(){return 2},c:3}", "new Number(1)", "new Number(-0)", "new String('s')", "new Boolean(false)", "Object(1n)", "Object(Symbol())", "new Date(0)", "new Date(NaN)", "/x/g", "new Error('e')", "new RangeError('r')", "new Map([[1,2]])", "new Set([1])",
  "new WeakMap", "new WeakSet", "Promise.resolve(1)", "new ArrayBuffer(4)", "new SharedArrayBuffer(1)", "new DataView(new ArrayBuffer(2))", "new Uint8Array([1,2,3])", "new Uint8Array(0)", "new Int8Array([-1,1])", "new Uint8ClampedArray([300])", "new Int16Array(2)", "new Uint32Array([4294967295])", "new Float32Array([1.5,NaN,Infinity])",
  "new Float64Array([-0,0.1])", "new BigInt64Array(1)", "new BigUint64Array([5n])", "new Float16Array ? new Float16Array([1.5]) : 0", "Object.assign(new Uint8Array([1]),{x:1})", "{length:2,0:'a',1:'b'}", "{length:0}", "{length:'x'}", "Object.assign([1,2],{x:3})", "(function(){return arguments})(1,2)", "Object.create(null)", "Object.create({a:1})",
  "Object.create(null,{a:{value:1,enumerable:true},b:{value:2}})", "Object.defineProperty({},'a',{value:1})", "Object.freeze({a:1})", "Object.freeze([1])", "new Proxy({a:1},{})", "new Proxy([1,2],{})", "new Proxy(function(){},{})", "new Proxy(new Date(0),{})", "new Proxy({},{ownKeys(){return ['a','b']},getOwnPropertyDescriptor(t,k){return {value:k,enumerable:true,configurable:true}},get(t,k){return 'v'+String(k)}})",
  "new Proxy({a:1},{ownKeys(){throw new EvalError('ok')}})", "new Proxy({a:1},{get(){throw new EvalError('gt')}})", "new Proxy({a:1},{getOwnPropertyDescriptor(){throw new EvalError('gd')}})", "new Proxy([1],{get(t,k){if(k==='length')throw new EvalError('len');return t[k]}})", "new Proxy([],{get(t,k){return k==='length'?2:'x'+String(k)}})",
  "new Proxy({},{get(t,k){return k==='toJSON'?()=>'pj':undefined}})", "Object.assign(function(){},{a:1})", "globalThis.Math", "JSON", "Reflect", "globalThis", "new (class A{constructor(){this.x=1}})", "new (class A{#p=1;q=2})", "new (class A extends Array{})", "new (class A extends Map{})",
  "Object.assign(Object(1),{a:1})", "Object.assign(Object('ab'),{a:1})", "Object.assign([], {2:1})", "new Array(3)", "new Array(3).fill(1)", "Array.from({length:3},(_,i)=>i)", "Array.from('ab')", "[[[]]]", "[{},[],{a:[]}]",
  "Object.assign(Object.create(null),{toJSON(){return 'np'}})", "new Intl.NumberFormat", "(new Error('m',{cause:1}))", "AggregateError([1])", "Symbol.prototype", "BigInt.prototype", "Object('s')", "[Object(1n)]",
];
const wraps = [
  (v) => `JSON.stringify(${v})`, (v) => `JSON.stringify([${v}])`, (v) => `JSON.stringify({k:${v}})`, (v) => `JSON.stringify({k:[${v}]},null,2)`, (v) => `JSON.stringify(${v},null,2)`,
  (v) => `JSON.stringify({a:1,k:${v}},["k"])`, (v) => `JSON.stringify({k:${v}},function(k,v){return v})`, (v) => `JSON.stringify([${v},${v}])`,
];
for (const k of kinds) for (const w of wraps) add(W(`return S(${w(k)})`));
for (const k of kinds) add(W(`var v=${k};return S(typeof JSON.stringify(v))+S(JSON.stringify(v)===undefined)`), W(`var v=${k};var s=JSON.stringify(v);return typeof s==="string"?S(JSON.stringify(JSON.parse(s))===s):"n/a"`));
// ciclos.
const cycles = [
  "var a={};a.a=a;", "var a=[];a[0]=a;", "var a={b:{}};a.b.c=a;", "var a={b:[{}]};a.b[0].c=a;", "var a={};var b={a:a};a.b=b;", "var a=[];a.push(a,a);", "var a={};a.x=[a];", "var a={x:{y:{z:{}}}};a.x.y.z.w=a.x;", "var a={};var b={};a.b=b;b.a=a;a=b;",
  "var a={get g(){return a}};", "var a={toJSON(){return a}};", "var a={toJSON(){return {x:a}}};", "var a=new Proxy({},{get(t,k){return a}, ownKeys(){return ['k']},getOwnPropertyDescriptor(){return {value:1,enumerable:true,configurable:true}}});", "var a={k:1};var b=[a,a];", "var a={k:1};var b={x:a,y:a};",
  "var a={};var b=[a];a.b=b;", "var a=Object.create(null);a.self=a;", "var a=new Number(1);a.s=a;", "var a=new Date(0);a.s=a;", "var a=function(){};a.s=a;", "var a=[1,[2,[3]]];a[1][1][1]=a;", "var a={};a[Symbol()]=a;", "var a={self:null};a.self={self:a};", "var a={b:{c:{d:{e:{}}}}};a.b.c.d.e.f=a.b;",
];
for (const c of cycles) {
  const name = /var b=/.test(c) && /a=b/.test(c) ? "a" : "a";
  add(
    W(`${c}return S(JSON.stringify(a))`), W(`${c}return S(JSON.stringify(a,null,2))`), W(`${c}return S(JSON.stringify([a]))`), W(`${c}return S(JSON.stringify({z:a}))`), W(`${c}try{JSON.stringify(a)}catch(e){return S(e.name)+S(e.message)+S(e instanceof TypeError)}return "no throw"`),
    W(`${c}return S(JSON.stringify(a,function(k,v){return typeof v==="object"?(v===a?"ROOT":v):v}))`), W(`${c}return S(JSON.stringify(a,function(k,v){return k==="a"||k==="self"||k==="s"||k==="b"||k==="x"||k==="c"?undefined:v}))`),
    W(`${c}return S(JSON.stringify(a,["k","a","b","x","y"]))`), W(`${c}var n=0;return S(JSON.stringify(a,function(k,v){n++;if(n>40)throw new RangeError("loop");return v}))`),
  );
}
add(
  W(`var a=[];var b=[a];a.push(b);try{JSON.stringify(a)}catch(e){return e.message}`), W(`var a={};try{JSON.stringify({x:a,y:{z:a}})}catch(e){return "threw"}return "ok"`), W(`var a={};return S(JSON.stringify([a,a,[a]]))`),
  W(`var d={};var a={x:{y:d},z:d};return S(JSON.stringify(a))`), W(`var a={};a.x=a;try{JSON.stringify(a,null,"\\t")}catch(e){return S(e.message)}`), W(`var a=[];a[0]=a;try{JSON.stringify({k:a})}catch(e){return S(e.message)}`),
  W(`var a={};a.x=a;try{JSON.stringify(a)}catch(e){return S(Object.getOwnPropertyNames(e).sort())+S(e.stack===undefined)}`),
);
// BigInt, Symbol e mensagens.
const bigs = ["1n", "0n", "-1n", "123456789012345678901234567890n", "Object(1n)", "BigInt.asUintN(64,-1n)", "2n**100n"];
for (const b of bigs) add(
  W(`return S(JSON.stringify(${b}))`), W(`return S(JSON.stringify([${b}]))`), W(`return S(JSON.stringify({a:${b}}))`), W(`return S(JSON.stringify({a:{b:[${b}]}}))`), W(`return S(JSON.stringify(${b},function(k,v){return typeof v==="bigint"?String(v):v}))`),
  W(`return S(JSON.stringify({a:${b}},function(k,v){return typeof v==="bigint"?Number(v):v}))`), W(`return S(JSON.stringify({a:${b}},["a"]))`), W(`return S(JSON.stringify({a:${b}},null,2))`), W(`return S(JSON.stringify({toJSON(){return ${b}}}))`),
  W(`var L=[];try{JSON.stringify({a:${b}},function(k,v){L.push(k);return v})}catch(e){L.push(e.name)}return L.join()`), W(`BigInt.prototype.toJSON=function(){return "B"+this};try{return S(JSON.stringify({a:${b}}))}finally{delete BigInt.prototype.toJSON}`),
  W(`BigInt.prototype.toJSON=function(){return typeof this};try{return S(JSON.stringify([${b}]))}finally{delete BigInt.prototype.toJSON}`), W(`return S(JSON.stringify({a:${b}},function(k,v){return k===""?v:undefined}))`),
);
const syms = ["Symbol()", "Symbol('s')", "Symbol.for('x')", "Symbol.iterator", "Object(Symbol())"];
for (const s of syms) add(
  W(`return S(JSON.stringify(${s}))`), W(`return S(JSON.stringify([${s}]))`), W(`return S(JSON.stringify({a:${s}}))`), W(`return S(JSON.stringify({[${s}]:1}))`), W(`return S(JSON.stringify({a:1,b:${s}}))`),
  W(`return S(JSON.stringify({a:${s}},function(k,v){return typeof v==="symbol"?"sym":v}))`), W(`return S(JSON.stringify({a:[${s},1]},null,1))`), W(`return S(JSON.stringify({[${s}]:1,a:2},function(k,v){return v}))`),
  W(`var o={};o[${s}]=1;return S(JSON.stringify(o,[${s}]))`), W(`return S(JSON.stringify({a:1},[${s}]))`),
);
// Proxy, array-like, typed arrays, boxed com acessos observáveis.
add(
  W(`var L=[];var p=new Proxy({a:1,b:[2]},{get(t,k,r){L.push("get "+String(k));return Reflect.get(t,k,r)},ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push("gopd "+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},has(t,k){L.push("has "+String(k));return k in t}});JSON.stringify(p);return L.join()`),
  W(`var L=[];var p=new Proxy([1,2],{get(t,k,r){L.push("get "+String(k));return Reflect.get(t,k,r)},ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push("gopd "+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});JSON.stringify(p);return L.join()`),
  W(`var L=[];var p=new Proxy({a:1},{get(t,k,r){L.push("get "+String(k));return Reflect.get(t,k,r)},ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push("gopd "+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});JSON.stringify({p:p},["p","a"]);return L.join()`),
  W(`var L=[];var p=new Proxy({a:1,b:2},{get(t,k,r){L.push("get "+String(k));return Reflect.get(t,k,r)},ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push("gopd "+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}});JSON.stringify(p,function(k,v){return v});return L.join()`),
  W(`var p=new Proxy({a:1},{ownKeys(){return ["a","a"]}});return S(JSON.stringify(p))`), W(`var p=new Proxy({a:1},{ownKeys(){return ["a","zz"]}});return S(JSON.stringify(p))`),
  W(`var p=new Proxy({},{ownKeys(){return ["a"]},getOwnPropertyDescriptor(){return {value:1,enumerable:false,configurable:true}}});return S(JSON.stringify(p))`),
  W(`var p=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){return undefined}});return S(JSON.stringify(p))`),
  W(`var r=Proxy.revocable({a:1},{});r.revoke();try{JSON.stringify(r.proxy)}catch(e){return e.name+e.message}`), W(`var r=Proxy.revocable([1],{});r.revoke();try{JSON.stringify([r.proxy])}catch(e){return e.name}`),
  W(`var r=Proxy.revocable({a:1},{});r.revoke();try{JSON.stringify({x:r.proxy},["x","a"])}catch(e){return e.name}`), W(`var r=Proxy.revocable([],{});r.revoke();try{JSON.stringify({},r.proxy)}catch(e){return e.name+e.message}`),
  W(`var p=new Proxy([1,2,3],{get(t,k,r){if(k==="length")return 2;return Reflect.get(t,k,r)}});return S(JSON.stringify(p))`), W(`var p=new Proxy([1,2,3],{get(t,k,r){if(k==="length")return "2";return Reflect.get(t,k,r)}});return S(JSON.stringify(p))`),
  W(`var p=new Proxy([1,2,3],{get(t,k,r){if(k==="length")return -1;return Reflect.get(t,k,r)}});return S(JSON.stringify(p))`), W(`var p=new Proxy([1,2],{get(t,k,r){if(k==="length")return 2**32;return Reflect.get(t,k,r)}});try{return S(JSON.stringify(p).length)}catch(e){return e.name}`),
  W(`var p=new Proxy([1,2],{get(t,k,r){if(k==="length")return 1.5;return Reflect.get(t,k,r)}});return S(JSON.stringify(p))`), W(`var p=new Proxy({},{get(t,k){return k==="length"?2:1}});return S(JSON.stringify(p))`),
  W(`var a=[1,2,3];a.length=2;return S(JSON.stringify(a))`), W(`var a={length:2,0:"a",1:"b"};return S(JSON.stringify(a))+S(JSON.stringify(Array.from(a)))+S(JSON.stringify(Array.prototype.slice.call(a)))`),
  W(`return S(JSON.stringify(new Uint8Array([1,2,3])))+S(JSON.stringify(Array.from(new Uint8Array([1,2,3]))))`), W(`return S(JSON.stringify(new Float32Array([0.1,0.5])))`), W(`return S(JSON.stringify(new Float64Array([NaN,-0,Infinity])))`),
  W(`return S(JSON.stringify(new Uint8Array(3),null,1))`), W(`return S(JSON.stringify({a:new Uint8Array(2)},["a","0"]))`), W(`return S(JSON.stringify({a:new Uint8Array(2)},["a","1"]))`), W(`return S(JSON.stringify(new Uint8Array(2),["0","1","length"]))`),
  W(`return S(JSON.stringify(new Uint8Array([5,6]),function(k,v){return typeof v==="number"?v*2:v}))`), W(`var t=new Uint8Array(2);t.foo=1;return S(JSON.stringify(t))`), W(`return S(JSON.stringify(new BigInt64Array(1)))`),
  W(`return S(JSON.stringify(new Int8Array([1,2]).subarray(1)))`), W(`return S(JSON.stringify(new DataView(new ArrayBuffer(1))))`), W(`return S(JSON.stringify([new ArrayBuffer(1)]))`), W(`return S(JSON.stringify(Object.assign(new Uint8Array(1),{toJSON(){return "tt"}})))`),
  W(`var t=new Uint8Array([1,2]);Object.defineProperty(t,"length",{value:5});return S(JSON.stringify(t))`), W(`return S(JSON.stringify(new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))))`),
  W(`var s=new String("ab");s.x=1;return S(JSON.stringify(s))+S(JSON.stringify([s]))`), W(`var n=new Number(5);n.x=1;return S(JSON.stringify(n))`), W(`var n=new Number(5);n.valueOf=()=>7;return S(JSON.stringify(n))`), W(`var n=new Number(5);n.toString=()=>"x";return S(JSON.stringify(n))`),
  W(`var s=new String("ab");s.toString=()=>"zz";return S(JSON.stringify(s))`), W(`var s=new String("ab");s.valueOf=()=>"zz";return S(JSON.stringify(s))`), W(`var b=new Boolean(false);b.valueOf=()=>true;return S(JSON.stringify(b))`),
  W(`var s=new String("ab");s[Symbol.toPrimitive]=()=>"pp";return S(JSON.stringify(s))`), W(`var s=new String("ab");s.toString=()=>{throw new RangeError("ts")};return S(JSON.stringify(s))`), W(`var n=new Number(5);n.valueOf=()=>{throw new RangeError("vo")};return S(JSON.stringify(n))`),
  W(`class N extends Number{};return S(JSON.stringify(new N(4)))`), W(`class St extends String{};return S(JSON.stringify(new St("q")))`), W(`class B extends Boolean{};return S(JSON.stringify(new B(1)))`),
  W(`return S(JSON.stringify(Object(1n)))`), W(`return S(JSON.stringify(Object(Symbol())))`), W(`var o=Object(Symbol());return S(JSON.stringify([o]))`),
  W(`var d=new Date(0);d.toISOString=()=>"mine";return S(JSON.stringify(d))`), W(`var d=new Date(0);d.toJSON=undefined;return S(JSON.stringify(d))`), W(`return S(JSON.stringify({d:new Date(Date.UTC(2020,0,1))}))`),
  W(`return S(JSON.stringify(new Date(2020,0,1,0,0,0,0)).length)`), W(`return S(JSON.stringify(new Date(-1)))`), W(`return S(JSON.stringify(new Date(-86400000*365*1900)))`), W(`return S(JSON.stringify([new Date(NaN),new Date(0)]))`),
  W(`return S(JSON.stringify(new Date(NaN),function(k,v){return typeof v}))`), W(`return S(JSON.stringify({a:new Date(NaN)},null,2))`),
);

// ---- 13. JSON.rawJSON / isRawJSON.
const raws = ["1", "-1", "1.5", "1e5", "1E-5", "0", "-0", "0.0", "123456789012345678901234567890", "1e1000", "null", "true", "false", '"s"', '"\\n"', '"\\ud800"', '"é"', "[]", "{}", "[1]", '{"a":1}', "undefined", "NaN", "Infinity", "01", "+1", ".5", "1.", "1e", "", " ", " 1", "1 ", "\t1", "1\n", "\n1", "\u00a01", "1\u00a0", "a", "'a'", '"', '"a', "tru", "nul", "-", "--1", "0x1", "1_0", "[", "{", "1,2", "1 2", '""', '"\u0000"', '"\\u0000"', '"a\nb"', "\ufeff1", "NULL", "True", "1n", "0n", "10000000000000000000000000000000000000000000000000000"];
for (const r of raws) {
  const p = lit(r);
  add(
    W(`return S(JSON.stringify(JSON.rawJSON(${p})))`), W(`return S(JSON.stringify([JSON.rawJSON(${p})]))`), W(`return S(JSON.stringify({a:JSON.rawJSON(${p})}))`), W(`var r=JSON.rawJSON(${p});return S(JSON.isRawJSON(r))+S(Object.keys(r))+S(r.rawJSON)+S(Object.isFrozen(r))+S(Object.getPrototypeOf(r))`),
    W(`return S(JSON.stringify({a:JSON.rawJSON(${p})},null,2))`), W(`var r=JSON.rawJSON(${p});return S(JSON.stringify(r,function(k,v){return v}))`), W(`var r=JSON.rawJSON(${p});return S(JSON.stringify({a:r},["a"]))`),
    W(`var r=JSON.rawJSON(${p});return S(JSON.stringify({a:r},function(k,v){return k==="a"?typeof v:v}))`),
  );
}
const rawOthers = ["undefined", "null", "1", "'1'", "true", "{}", "[]", "Symbol()", "1n", "{toString(){return '7'}}", "{[Symbol.toPrimitive](){return '8'}}", "new String('9')", "new Number(1)", "['1']", "[1,2]", "Object.create(null)", "function(){}", "{rawJSON:'1'}", "new Date(0)", "{toString(){throw new RangeError('rj')}}", "{toString(){return '{}'}}", "{toString(){return ' '}}"];
for (const r of rawOthers) add(W(`return S(JSON.stringify(JSON.rawJSON(${r})))`), W(`return S(JSON.isRawJSON(${r}))`), W(`var r=JSON.rawJSON(${r});return S(r.rawJSON)`));
add(
  W(`return S(JSON.isRawJSON(JSON.rawJSON("1")))+S(JSON.isRawJSON({rawJSON:"1"}))+S(JSON.isRawJSON(Object.freeze({rawJSON:"1"})))+S(JSON.isRawJSON())+S(JSON.isRawJSON(null))+S(JSON.isRawJSON("1"))`),
  W(`var r=JSON.rawJSON("1");return S(Object.getOwnPropertyNames(r))+S(Object.getOwnPropertyDescriptor(r,"rawJSON"))+S(Object.isExtensible(r))`), W(`var r=JSON.rawJSON("1");try{r.rawJSON="2"}catch(e){return e.name}return S(r.rawJSON)`),
  W(`"use strict";var r=JSON.rawJSON("1");try{r.rawJSON="2"}catch(e){return e.name+e.message}return S(r.rawJSON)`), W(`var r=JSON.rawJSON("1");try{r.x=1}catch(e){return e.name}return S(r.x)`), W(`var r=JSON.rawJSON("1");try{"use strict";Object.setPrototypeOf(r,{})}catch(e){return e.name}return "ok"`),
  W(`return S(JSON.rawJSON.length)+S(JSON.rawJSON.name)+S(JSON.isRawJSON.length)+S(JSON.isRawJSON.name)`), W(`return S(Object.getOwnPropertyNames(JSON).sort())`), W(`return S(typeof JSON.rawJSON)+S(typeof JSON.isRawJSON)`),
  W(`return S(new JSON.rawJSON("1"))`), W(`var r=JSON.rawJSON("1");return S(r+"")`), W(`var r=JSON.rawJSON("1");return S(Object.prototype.toString.call(r))`), W(`var r=JSON.rawJSON("1");return S(r instanceof Object)`),
  W(`var r=JSON.rawJSON("1");return S(structuredClone===undefined?0:Object.getPrototypeOf(r))`), W(`var r=JSON.rawJSON("1");return S(JSON.stringify([r,r,{x:r}]))`),
  W(`var a=[JSON.rawJSON("1")];return S(JSON.stringify(a,function(k,v){return v}))+S(JSON.stringify(a,null,"-"))`), W(`var r=JSON.rawJSON('"a"');return S(JSON.stringify({[r.rawJSON]:r}))`),
  W(`var r=JSON.rawJSON("1");return S(JSON.stringify({a:r},function(k,v){if(k==="a")return [v];return v}))`), W(`var r=JSON.rawJSON("1");return S(JSON.stringify({a:{toJSON(){return r}}}))`),
  W(`var r=JSON.rawJSON("1");return S(JSON.stringify({a:{b:1}},function(k,v){return k==="b"?r:v}))`), W(`return S(JSON.stringify({a:JSON.rawJSON("12345678901234567890")}))`), W(`return S(JSON.parse(JSON.stringify({a:JSON.rawJSON("12345678901234567890")})).a)`),
  W(`return S(JSON.parse('{"a":12345678901234567890}',function(k,v,c){return k==="a"?BigInt(c.source):v}))`), W(`return S(JSON.parse('[1.0,1.50,0.10,1e2,-0]',function(k,v,c){return k===""?v:JSON.rawJSON(c.source)}))`),
  W(`return S(JSON.stringify(JSON.parse('[1.0,1.50,0.10,1e2,-0,12345678901234567890]',function(k,v,c){return k===""?v:JSON.rawJSON(c.source)})))`), W(`return S(JSON.stringify(JSON.parse('{"a":1.0,"b":"x","c":[1e0]}',function(k,v,c){return typeof v==="number"?JSON.rawJSON(c.source):v})))`),
  W(`var L=[];JSON.parse('{"a":[1,{"b":2}],"c":"s","d":null,"e":true}',function(k,v,c){L.push(k+":"+S(Object.keys(c))+S(c.source));return v});return L.join(" ")`),
  W(`var L=[];JSON.parse('[1,2]',function(k,v,c){if(k==="0")this[1]=99;L.push(k+":"+S(v)+S(c.source));return v});return L.join(" ")`),
  W(`var L=[];JSON.parse('{"a":1,"b":2}',function(k,v,c){if(k==="a")this.b=99;L.push(k+":"+S(v)+S(c.source));return v});return L.join(" ")`),
  W(`var L=[];JSON.parse('{"a":1,"b":2}',function(k,v,c){if(k==="a")delete this.b;L.push(k+":"+S(v)+S(c.source));return v});return L.join(" ")`),
  W(`var L=[];JSON.parse('{"a":1,"b":2}',function(k,v,c){if(k==="a"){this.b=2}L.push(k+":"+S(v)+S(c.source));return v});return L.join(" ")`),
  W(`var L=[];JSON.parse('{"a":1,"b":2}',function(k,v,c){if(k==="a"){this.b=2;this.b=[2]}L.push(k+":"+S(v)+S(c.source));return v});return L.join(" ")`),
  W(`var c0;JSON.parse('1',function(k,v,c){c0=c;return v});return S(Object.getPrototypeOf(c0)===Object.prototype)+S(Object.isFrozen(c0))+S(Object.getOwnPropertyNames(c0))`),
  W(`var c0;JSON.parse('{}',function(k,v,c){c0=c;return v});return S(Object.getPrototypeOf(c0)===Object.prototype)+S(Object.getOwnPropertyNames(c0))`),
  W(`var cs=[];JSON.parse('[1,1]',function(k,v,c){cs.push(c);return v});return S(cs[0]===cs[1])`),
  W(`var cs=[];JSON.parse('[1]',function(k,v,c){cs.push(c);return v});cs[0].source="x";return S(cs[0].source)`),
  W(`var L=[];JSON.parse('"a\\\\u0062"',function(k,v,c){L.push(S(v)+S(c.source));return v});return L.join()`), W(`var L=[];JSON.parse('  [ 1 , 2 ]  ',function(k,v,c){L.push(S(c.source));return v});return L.join()`),
  W(`var L=[];JSON.parse('1.0',function(k,v,c){L.push(S(c.source)+S(v));return v});return L.join()`), W(`var L=[];JSON.parse('-0',function(k,v,c){L.push(S(c.source)+S(v));return v});return L.join()`),
  W(`var L=[];JSON.parse('1e400',function(k,v,c){L.push(S(c.source)+S(v));return v});return L.join()`), W(`var L=[];JSON.parse('[true,false,null]',function(k,v,c){L.push(S(c.source)+S(v));return v});return L.join()`),
  W(`var L=[];JSON.parse('{"a":{"b":1}}',function(k,v,c){L.push(k+S(c.source));return typeof v==="object"?{n:1}:v});return L.join()`),
  W(`var L=[];JSON.parse('[1,2]',function(k,v,c){L.push(k+S(c.source));return k==="0"?"changed":v});return L.join()`),
  W(`var L=[];JSON.parse('[[1]]',function(k,v,c){L.push(k+S(c.source));if(k==="0"&&Array.isArray(this))this[0]=[5];return v});return L.join()`),
);

// ---- 14. Round trips e miscelânea.
const rt = ['{"a":[1,2,{"b":null}],"c":"\\u00e9\\n","d":-0,"e":1e21}', "[1e-7,1e21,123456789012345680000,0.1,5e-324,1.7976931348623157e308]", '{"":"","a b":1,"\\"":2,"\\\\":3}', '[[],{},"",0,false,null]', '"\\ud83d\\ude00\\u2028"', "[0.30000000000000004,100,1e2,1E2,0.1e1]"];
for (const t of rt) {
  const p = lit(t);
  add(
    W(`var v=JSON.parse(${p});return S(JSON.stringify(v))`), W(`var v=JSON.parse(${p});return S(JSON.stringify(v,null,2))`), W(`var v=JSON.parse(${p});return S(JSON.stringify(JSON.parse(JSON.stringify(v)))===JSON.stringify(v))`),
    W(`var v=JSON.parse(${p});return S(JSON.stringify(v,null,"\\t").split("\\n").length)`), W(`var v=JSON.parse(${p});return S(JSON.stringify(v,function(k,v){return v}))`), W(`var v=JSON.parse(${p},function(k,v){return v});return S(JSON.stringify(v))`),
    W(`var v=JSON.parse(${p});return S(JSON.stringify([v,v]))`), W(`var v=JSON.parse(${p});return S(JSON.stringify({v:v},null,1))`), W(`var v=JSON.parse(${p});return S(Object.prototype.toString.call(v))+S(typeof v)`),
  );
}
add(
  W(`return S(JSON.stringify())`), W(`return S(JSON.stringify(undefined))`), W(`return S(JSON.stringify(function(){}))`), W(`return S(JSON.stringify(Symbol()))`), W(`return S(JSON.stringify(null,null,2))`), W(`return S(JSON.stringify([],null,2))+S(JSON.stringify({},null,2))`),
  W(`return S(JSON.stringify([[]],null,2))`), W(`return S(JSON.stringify([{}],null,2))`), W(`return S(JSON.stringify({a:[]},null,2))`), W(`return S(JSON.stringify({a:{}},null,2))`), W(`return S(JSON.stringify([undefined],null,2))`),
  W(`return S(JSON.stringify({a:undefined},null,2))`), W(`return S(JSON.stringify({a:1,b:undefined,c:2}))`), W(`return S(JSON.stringify([1,undefined,2]))`), W(`return S(JSON.stringify([function(){},Symbol(),undefined]))`),
  W(`return S(JSON.stringify("a",null,2))`), W(`return S(JSON.stringify(1,null,"abc"))`), W(`return S(JSON.stringify(1,["a"]))`), W(`return S(JSON.stringify(JSON))`), W(`return S(JSON.stringify(Math))`), W(`return S(JSON.stringify(globalThis.Reflect))`),
  W(`return S(JSON.stringify.call(null,1))`), W(`return S(JSON.stringify.apply(undefined,[{a:1},null,1]))`), W(`return S(JSON.stringify(1,2,3,4))`), W(`return S(Reflect.ownKeys(JSON).map(String).join())`),
  W(`return S(String(JSON))`), W(`return S(Object.getOwnPropertyDescriptor(JSON,"parse"))`), W(`return S(Object.getOwnPropertyDescriptor(JSON,"stringify"))`), W(`return S(Object.getOwnPropertyDescriptor(globalThis,"JSON"))`),
  W(`return S(JSON.parse(" 1 "))+S(JSON.parse("\\n[\\n]\\n"))+S(JSON.parse("\\t{\\t}\\t"))`), W(`return S(JSON.parse('{"a":1}').constructor===Object)`), W(`return S(JSON.parse('[]').constructor===Array)`),
  W(`return S(Object.isExtensible(JSON.parse("{}")))+S(Object.isFrozen(JSON.parse("[]")))`), W(`var o=JSON.parse('{"a":1}');return S(Object.getOwnPropertyDescriptor(o,"a"))`), W(`var a=JSON.parse('[1]');return S(Object.getOwnPropertyDescriptor(a,"0"))+S(Object.getOwnPropertyDescriptor(a,"length"))`),
  W(`return S(JSON.parse('"\\\\u0041"'))+S(JSON.parse('"\\\\/"'))`), W(`return S(JSON.parse("1\\n"))+S(JSON.parse("\\r1"))`),
);

// ---- Execução: dedup contra os goldens existentes, filhos novos em paralelo.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const existing = new Set();
for (const file of ["json_bun.tsv", "json_grid_bun.tsv", "json_more_bun.tsv", "json_number_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
      if (!line) continue;
      try {
        const src = JSON.parse(line.split("\t")[0]);
        const i = src.indexOf("globalThis.R = ");
        if (i >= 0) existing.add(src.slice(i + 15));
      } catch (e) {}
    }
  } catch (e) {}
}
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter((e) => !seen.has(e) && !existing.has(e) && seen.add(e));
const dup = exprs.length - unique.length;

// O resultado sai do filho pelo preload em JSON (surrogate solitário vira \udXXX, sem perda no pipe UTF-8).
const PRELOAD = writeResultPreload();
const runChild = (source) =>
  new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      const result = decodeResult(out);
      code === 0 && result !== null ? resolve(result) : reject(new Error(err || "filho falhou " + code));
    });
    child.stdin.end(source);
  });

(async () => {
  const sources = unique.map((expr) => '"use strict";\n' + PRELUDE + "globalThis.R = " + expr);
  const results = new Array(unique.length);
  let next = 0;
  const workers = Array.from({ length: 24 }, async () => {
    while (next < unique.length) {
      const i = next++;
      try {
        results[i] = await runChild(sources[i]);
      } catch (e) {
        results[i] = null;
        process.stderr.write("erro de programa: " + JSON.stringify(unique[i]).slice(0, 160) + " " + String(e).slice(0, 120) + "\n");
      }
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < unique.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[\u2013\u2014]/.test(result + sources[i])) {
      dropped++;
      if (result !== null) process.stderr.write("descartado: " + JSON.stringify(unique[i]).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push(JSON.stringify(sources[i]) + "\t" + JSON.stringify(result));
  }
  process.stdout.write(emitFactoredLines("json_deep", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
