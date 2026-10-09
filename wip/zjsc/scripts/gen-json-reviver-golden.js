// Gera tests/golden/json_reviver_bun.tsv: JSON.parse com reviver observável (ordem das chamadas com holder, key e this;
// reviver que apaga, adiciona, troca irmãs ainda não visitadas, devolve undefined, muta arrays com length e buracos,
// lança, holder congelado ou com propriedade não configurável, `context.source`) e JSON.parse de entradas inválidas com
// a mensagem exata de SyntaxError do bun. Medido no bun 1.4.2, cada programa num bun filho novo (no máximo 6 em
// paralelo, timeout de 8 s). Programas já presentes nos goldens json vizinhos são descartados (knownPrograms).
// Uso: bun scripts/gen-json-reviver-golden.js > tests/golden/json_reviver_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
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
const lit = (s) => JSON.stringify(s);
const W = (body) => `T(()=>{${body}})`;
const EN = String.fromCharCode(0x2013);
const EM = String.fromCharCode(0x2014);

// ---- 1. Textos válidos usados nos reviver.
const objects = ['{"a":1,"b":2}', '{"a":1,"b":2,"c":3}', '{"b":1,"a":2}', '{"1":1,"0":2,"a":3}', '{"a":{"x":1,"y":2},"b":{"z":3}}',
  '{"a":[1,2],"b":[3,4]}', '{"a":1,"b":{"c":2,"d":[3]},"e":null}', '{"2":"x","1":"y","z":"w"}', '{"a":"s","b":true,"c":null,"d":1.5}'];
const arrays = ["[1,2,3]", "[1,2,3,4,5]", "[[1,2],[3,4]]", "[{\"a\":1},{\"a\":2}]", "[1,[2,[3]]]", '["a","b","c"]', "[null,true,false]", "[[],{}]"];
const texts = objects.concat(arrays);

// 1a. Ordem e argumentos.
const logVariants = {
  kv: `var L=[];var r=JSON.parse(%T,function(k,v){L.push(k+"="+S(v));return v});return L.join(" ")+" => "+S(r)`,
  kvc: `var L=[];JSON.parse(%T,function(k,v,c){L.push(k+"|"+(c&&Object.keys(c).join("/"))+"|"+(c&&"source" in c?c.source:"-"));return v});return L.join(" ")`,
  holder: `var L=[];JSON.parse(%T,function(k,v){L.push(k+"@"+(Array.isArray(this)?"A":"O")+Object.keys(this).join("/"));return v});return L.join(" ")`,
  holderSnap: `var L=[];JSON.parse(%T,function(k,v){L.push(k+"@"+S(this));return v});return L.join(" ")`,
  argc: `var L=[];JSON.parse(%T,function(k,v){L.push(arguments.length+typeof arguments[2]);return v});return L.join("")`,
  thisIdent: `var H=[];JSON.parse(%T,function(k,v){H.push(this);return v});var U=H.filter((h,i)=>H.indexOf(h)===i);return H.length+"/"+U.length`,
  rootHolder: `var R0;JSON.parse(%T,function(k,v){if(k==="")R0=this;return v});return S(R0)+Object.getPrototypeOf(R0===Object(R0)?R0:{})===Object.prototype`,
  depth: `var D=0,M=0;JSON.parse(%T,function(k,v){D++;M=Math.max(M,D);D--;return v});return M`,
  valueTypes: `var L=[];JSON.parse(%T,function(k,v){L.push(v===null?"null":Array.isArray(v)?"arr":typeof v);return v});return L.join()`,
  strictThis: `var L=[];JSON.parse(%T,function(k,v){"use strict";L.push(typeof this);return v});return L.join()`,
  arrowThis: `var L=[];var self=this;JSON.parse(%T,(k,v)=>{L.push(this===self);return v});return L.join()`,
  boundThis: `var L=[];JSON.parse(%T,(function(k,v){L.push(S(this));return v}).bind(7));return L.join()`,
  keysOrder: `var L=[];JSON.parse(%T,function(k,v){L.push(k);return v});return L.join()`,
  rebuild: `return S(JSON.parse(%T,function(k,v){return v}))`,
  doubleNum: `return S(JSON.parse(%T,function(k,v){return typeof v==="number"?v*2:v}))`,
  wrapEach: `return S(JSON.parse(%T,function(k,v){return k===""?v:[k,v]}))`,
  topOnly: `return S(JSON.parse(%T,function(k,v){return k===""?"top":v}))`,
  depthFirst: `var L=[];JSON.parse(%T,function(k,v){L.push(typeof v==="object"&&v!==null?"close:"+k:"leaf:"+k);return v});return L.join()`,
};
for (const text of texts) for (const body of Object.values(logVariants)) exprs.push(W(body.replace(/%T/g, lit(text))));

// 1b. Retorno undefined (delete) por chave e por tipo.
for (const text of texts) {
  for (const key of ["a", "b", "c", "0", "1", "2", "x", "z", "d", "e"]) {
    exprs.push(W(`return S(JSON.parse(${lit(text)},function(k,v){return k===${lit(key)}?undefined:v}))`));
    exprs.push(W(`var L=[];var r=JSON.parse(${lit(text)},function(k,v){L.push(k);return k===${lit(key)}?undefined:v});return L.join()+" "+S(r)`));
  }
  for (const [name, test] of [["num", 'typeof v==="number"'], ["str", 'typeof v==="string"'], ["null", "v===null"], ["bool", 'typeof v==="boolean"'], ["obj", 'typeof v==="object"&&v!==null&&k!==""'], ["root", 'k===""']]) {
    exprs.push(W(`return "${name}"+S(JSON.parse(${lit(text)},function(k,v){return (${test})?undefined:v}))`));
  }
  exprs.push(W(`var n=0;return S(JSON.parse(${lit(text)},function(k,v){return ++n%2?undefined:v}))`));
  exprs.push(W(`var n=0;return S(JSON.parse(${lit(text)},function(k,v){return ++n%3===0?undefined:v}))`));
}

// 1c. Mutação de irmãs, holder e arrays pelo reviver, disparada na primeira chave visitada ou numa chave escolhida.
const mutations = {
  delNext: `delete this[%N]`,
  delSelf: `delete this[k]`,
  addKey: `this.added=1`,
  addIndexKey: `this[9]="n"`,
  setNext: `this[%N]="SET"`,
  setNextObj: `this[%N]={q:[1]}`,
  setNextArr: `this[%N]=[1,2]`,
  setNextNull: `this[%N]=null`,
  readdNext: `var t=this[%N];delete this[%N];this[%N]=t`,
  nonEnumNext: `Object.defineProperty(this,%N,{enumerable:false})`,
  nonConfNext: `Object.defineProperty(this,%N,{configurable:false,value:"NC"})`,
  getterNext: `Object.defineProperty(this,%N,{get(){L.push("get");return "G"},configurable:true,enumerable:true})`,
  throwingGetter: `Object.defineProperty(this,%N,{get(){throw new RangeError("g")},configurable:true,enumerable:true})`,
  nonWritable: `Object.defineProperty(this,k,{writable:false})`,
  nonConfSelf: `Object.defineProperty(this,k,{configurable:false})`,
  freezeHolder: `Object.freeze(this)`,
  sealHolder: `Object.seal(this)`,
  preventExt: `Object.preventExtensions(this)`,
  setProto: `Object.setPrototypeOf(this,{inh:1})`,
  lengthZero: `if(Array.isArray(this))this.length=0`,
  lengthOne: `if(Array.isArray(this))this.length=1`,
  lengthGrow: `if(Array.isArray(this))this.length=7`,
  pushArr: `if(Array.isArray(this))this.push("P")`,
  popArr: `if(Array.isArray(this))this.pop()`,
  shiftArr: `if(Array.isArray(this))this.shift()`,
  unshiftArr: `if(Array.isArray(this))this.unshift("U")`,
  spliceArr: `if(Array.isArray(this))this.splice(0,1)`,
  holeArr: `if(Array.isArray(this))delete this[%N]`,
  reverseArr: `if(Array.isArray(this))this.reverse()`,
  sortArr: `if(Array.isArray(this))this.sort().reverse()`,
  lengthFrozenArr: `if(Array.isArray(this)){Object.freeze(this)}`,
  lengthNonWritable: `if(Array.isArray(this))Object.defineProperty(this,"length",{writable:false})`,
};
const nextOf = (text) => {
  const keys = [];
  const v = JSON.parse(text);
  if (Array.isArray(v)) return ["1", "0", "2"];
  for (const key of Object.keys(v)) keys.push(key);
  return keys.length > 1 ? [keys[1], keys[0]] : keys;
};
for (const text of texts) {
  const firsts = Array.isArray(JSON.parse(text)) ? ["0", "1"] : Object.keys(JSON.parse(text)).slice(0, 2);
  for (const [name, body] of Object.entries(mutations)) {
    for (const trigger of firsts) {
      for (const next of nextOf(text).slice(0, name.includes("Next") || body.includes("%N") ? 2 : 1)) {
        const code = body.replace(/%N/g, lit(next));
        exprs.push(W(`var L=[];var D=0;var r=JSON.parse(${lit(text)},function(k,v){L.push(k);if(k===${lit(trigger)}&&!D){D=1;${code}}return v});return L.join()+" "+S(r)`));
      }
    }
  }
}

// 1d. Reviver que lança, na n-ésima chamada, com vários tipos de exceção, e valor de retorno não primitivo.
for (const text of texts) {
  for (let n = 1; n <= 4; n++) {
    exprs.push(W(`var L=[];try{JSON.parse(${lit(text)},function(k,v){L.push(k);if(L.length===${n})throw new RangeError("stop"+${n});return v})}catch(e){L.push(e.name+e.message)}return L.join()`));
    exprs.push(W(`var c=0;JSON.parse(${lit(text)},function(k,v){if(++c===${n})throw {tag:${n}};return v})`));
    exprs.push(W(`var c=0;return S(JSON.parse(${lit(text)},function(k,v){if(++c===${n})return {valueOf(){throw new EvalError("vo")}};return v}))`));
  }
  exprs.push(W(`JSON.parse(${lit(text)},function(k,v){throw 7})`));
  exprs.push(W(`JSON.parse(${lit(text)},function(k,v){throw null})`));
  exprs.push(W(`return S(JSON.parse(${lit(text)},function(k,v){return Symbol.iterator}))`));
  exprs.push(W(`return S(JSON.parse(${lit(text)},function(k,v){return 10n}))`));
}

// 1e. Holder congelado, selado e propriedade não configurável: CreateDataProperty falha em silêncio.
const hold = {
  freezeAll: `Object.freeze(this)`,
  freezeFirst: `if(!D){D=1;Object.freeze(this)}`,
  sealAll: `Object.seal(this)`,
  ncAll: `try{Object.defineProperty(this,k,{configurable:false})}catch(e){}`,
  ncWritableOff: `try{Object.defineProperty(this,k,{configurable:false,writable:false})}catch(e){}`,
  nonWritableOnly: `try{Object.defineProperty(this,k,{writable:false})}catch(e){}`,
  accessorNC: `try{Object.defineProperty(this,k,{get(){return 5},set(x){L.push("set")},configurable:false})}catch(e){}`,
  accessorC: `try{Object.defineProperty(this,k,{get(){return 5},set(x){L.push("set")},configurable:true})}catch(e){}`,
  protoSetter: `try{Object.defineProperty(Object.prototype,k,{set(x){L.push("protoset")},configurable:true})}catch(e){}`,
};
const returns = { undef: "undefined", num: "42", str: '"R"', obj: "{r:1}", arr: "[9]", same: "v" };
for (const text of texts) {
  for (const [hName, h] of Object.entries(hold)) {
    for (const [rName, ret] of Object.entries(returns)) {
      exprs.push(W(`var L=[];var D=0;var r=JSON.parse(${lit(text)},function(k,v){L.push(k);${h};return k===""?v:${ret}});return L.join()+" "+S(r)`));
      if (rName === "undef" || rName === "num") {
        exprs.push(W(`var L=[];var D=0;var r=JSON.parse(${lit(text)},function(k,v){L.push(k);${h};return k===""?v:${ret}});return L.join()+" "+S(r)+" "+S(Object.isFrozen(r))`));
      }
    }
  }
}
// holder interno congelado por irmão visitado antes.
for (const text of ['{"a":{"x":1,"y":2},"b":{"z":3}}', '{"a":[1,2],"b":[3,4]}', '[[1,2],[3,4]]', '[{"a":1},{"a":2}]']) {
  for (const rv of ["undefined", "0", "null", '"s"']) {
    exprs.push(W(`var L=[];var r=JSON.parse(${lit(text)},function(k,v){L.push(k);if(typeof v==="object"&&v!==null&&k!=="")Object.freeze(v);return typeof v==="object"?v:${rv}});return L.join()+" "+S(r)`));
    exprs.push(W(`var L=[];var r=JSON.parse(${lit(text)},function(k,v){L.push(k);if(k==="")return v;if(typeof v==="object"&&v!==null)return v;if(this.constructor===Array?k==="0":k==="x"){Object.freeze(this)}return ${rv}});return L.join()+" "+S(r)`));
    exprs.push(W(`var L=[];var r=JSON.parse(${lit(text)},function(k,v){L.push(k);if(k===""||typeof v==="object")return v;Object.defineProperty(this,k,{configurable:false});return ${rv}});return L.join()+" "+S(r)`));
  }
}

// 1f. context.source (medido no bun).
const srcTexts = ["1", "-0", "0.0", "1.50", "1e2", "1E+2", "123456789012345678901234567890", "9007199254740993", "0.1", "-1.5e-3", "1e400", "5e-324",
  "true", "false", "null", '"s"', '"a\\u0041\\n"', '"\\ud83d\\ude00"', '""', "[1,2]", '{"a":1}', "[1.0,1.00]", '{"a":1.0,"b":"x"}', " 12 ", "\t[ 1 , 2 ]\n", "[true,null,false]"];
for (const text of srcTexts) {
  exprs.push(
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){L.push(k+":"+(c&&Object.keys(c).join("/"))+":"+(c&&c.source));return v});return L.join(" ")`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){L.push(typeof c+":"+(c===null)+":"+Object.getPrototypeOf(c)===Object.prototype);return v});return L.join(" ")`),
    W(`var C=[];JSON.parse(${lit(text)},function(k,v,c){C.push(c);return v});var U=C.filter((c,i)=>C.indexOf(c)===i);return C.length+"/"+U.length`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){L.push(Object.getOwnPropertyDescriptor(c,"source")&&JSON.stringify(Object.getOwnPropertyDescriptor(c,"source")));return v});return L.join(" ")`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){if(c&&Object.isFrozen(c))L.push("frozen");L.push(Object.isExtensible(c));return v});return L.join(" ")`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){L.push(typeof v==="number"?String(v)===c.source:"-");return v});return L.join(" ")`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){if(k!==""){this[k]=typeof v==="number"?v+1:v}L.push(c&&c.source);return v});return L.join(" ")`),
    W(`var L=[];JSON.parse(${lit(text)},function(k,v,c){c.source="x";c.extra=1;L.push(Object.keys(c).join("/"));return v});return L.join(" ")`),
  );
}
// source depois de mutação do valor pelo reviver nas irmãs.
for (const text of ["[1,2,3]", '{"a":1,"b":2,"c":3}', '[1.0,2.0,3.0]', '{"a":"x","b":"y"}', '[1,[2,3],4]']) {
  for (const set of ["this[%N]=9", "this[%N]=this[%N]", "this[%N]=String(this[%N])", "delete this[%N]", "this[%N]=null", "this[%N]={}"]) {
    for (const next of nextOf(text).slice(0, 2)) {
      exprs.push(W(`var L=[];var D=0;JSON.parse(${lit(text)},function(k,v,c){L.push(k+":"+(c&&c.source));if(!D){D=1;${set.replace(/%N/g, lit(next))}}return v});return L.join(" ")`));
    }
  }
}

// ---- 2. Entradas inválidas: mensagem exata. Prefixos de textos válidos, trocas de caractere, inserções.
const baseTexts = ['{"a":1,"b":[true,null]}', '[1,"x",{"k":-2.5e3}]', '"str\\u0041\\n"', "-12.5e+3", '{"a":{"b":{}}}', "[[],[1],[[2]]]", 'tr' + 'ue', "null", '[false,"é"]', '{"":0}'];
const invalid = new Set();
for (const t of baseTexts) {
  for (let i = 0; i <= t.length; i++) invalid.add(t.slice(0, i));
  for (let i = 0; i < t.length; i++) {
    for (const ch of ["x", ",", ":", "}", "]", "{", "[", '"', "'", "\\", "\n", " ", "0", "-", ".", "e", "\u0000", " ", "é", "\ud83d"]) {
      invalid.add(t.slice(0, i) + ch + t.slice(i + 1));
      invalid.add(t.slice(0, i) + ch + t.slice(i));
    }
    invalid.add(t.slice(0, i) + t.slice(i + 1));
  }
  invalid.add(t + " x");
  invalid.add(t + t);
  invalid.add(t + ",");
  invalid.add(t + "\u0000");
}
const extraInvalid = ["", " ", "\n", "undefined", "NaN", "Infinity", "-Infinity", "+1", "01", "-01", "1.", ".5", "-.5", "1e", "1e+", "0x1", "1_0", "'a'", '"a', '"\\', '"\\x41"', '"\\u12"', '"\\u12G4"', '"\\ud800"', '"a\nb"', '"a\tb"', "  1", "1 ", "﻿1", "{a:1}", "{'a':1}", '{"a"}', '{"a":}', '{"a":1,}', '{,}', "[,]", "[1,]", "[1 2]", "[1,,2]", '{"a":1 "b":2}', "tru", "nul", "fals", "True", "NULL", "// c\n1", "/* c */1", "1 // c", "[1]]", "{}}", "[}", "{]", "-", "--1", "- 1", "1 2", '"a""b"', " ", '" "', "[ ]", "\\u0031", "﻿", "{\"a\":1}x", "[1,2,3", '{"a":[1,2', '{"a":{"b":', "9".repeat(5) + "x", "\u0000", "\"\u0000\"", "\"\u001f\"", '"\u007f"', '"\\/"', '"\\a"', '"\\\n"', '[1e+]', '[0e]', '[-]', '[+]', '[.]', '{"a":01}', '{"a":-}', 'nulll', 'truefalse', '[truefalse]'];
for (const t of extraInvalid) invalid.add(t);
const invalidList = [...invalid].filter((t) => { try { JSON.parse(t); return false; } catch (e) { return true; } });
const validFromMutation = [...invalid].filter((t) => { try { JSON.parse(t); return true; } catch (e) { return false; } });
for (const t of invalidList) {
  exprs.push(W(`return JSON.parse(${lit(t)})`));
}
// variantes de contexto para uma amostra: reviver, dentro de try aninhado, name e instância.
for (let i = 0; i < invalidList.length; i += 3) {
  const t = invalidList[i];
  exprs.push(W(`return JSON.parse(${lit(t)},function(k,v){return v})`));
  exprs.push(W(`try{JSON.parse(${lit(t)})}catch(e){return e.name+"|"+(e instanceof SyntaxError)+"|"+Object.keys(e).length+"|"+Object.getPrototypeOf(e)===SyntaxError.prototype+"|"+typeof e.stack}`));
}
for (const t of validFromMutation) {
  if (/[\ud800-\udfff]/.test(t) && !/^[\x00-\x7f]*$/.test(t) && false) continue;
  exprs.push(W(`var L=[];var r=JSON.parse(${lit(t)},function(k,v){L.push(k);return v});return L.join()+" "+S(r)`));
}
// reviver inválido com texto inválido: o texto falha antes de chamar o reviver.
for (const t of invalidList.slice(0, 120)) exprs.push(W(`var c=0;try{JSON.parse(${lit(t)},function(k,v){c++;return v})}catch(e){return c+e.message}`));

// ---- 3. Dedup, filtro de host, execução.
const known = new Set(knownPrograms("json_reviver_bun.tsv", (name) => /json/.test(name)));
const seen = new Set();
const sources = [];
for (const expr of exprs) {
  const source = '"use strict";\n' + PRELUDE + "globalThis.R = " + expr;
  if (seen.has(source) || known.has(source) || usesHostApi(expr) || /[–—]/.test(source)) continue;
  seen.add(source);
  sources.push(source);
}

// O resultado sai do filho pelo preload em JSON (surrogate solitário vira \udXXX, sem perda no pipe UTF-8).
const PRELOAD = writeResultPreload();
const runChild = (source) =>
  new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); reject(new Error("timeout")); }, 8000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      clearTimeout(timer);
      const result = decodeResult(out);
      code === 0 && result !== null ? resolve(result) : reject(new Error(err || "filho falhou " + code));
    });
    child.stdin.end(source);
  });

(async () => {
  const results = new Array(sources.length).fill(null);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < sources.length) {
      const i = next++;
      try {
        results[i] = await runChild(sources[i]);
      } catch (e) {
        process.stderr.write("erro de programa: " + JSON.stringify(sources[i].slice(PRELUDE.length + 40)).slice(0, 160) + " " + String(e).slice(0, 100) + "\n");
      }
    }
  });
  await Promise.all(workers);
  const lines = [];
  let dropped = 0;
  for (let i = 0; i < sources.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[–—]/.test(result)) { dropped++; continue; }
    lines.push(JSON.stringify(sources[i]) + "\t" + JSON.stringify(result));
  }
  process.stdout.write(emitFactoredLines("json_reviver", lines));
  process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}, candidatos ${exprs.length}\n`);
})();
