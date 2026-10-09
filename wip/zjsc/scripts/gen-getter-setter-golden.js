// Gera tests/golden/accessor_bun.tsv: acessores e descritores em objetos comuns medidos no bun 1.4.2
// (defineProperty/defineProperties/getOwnPropertyDescriptor(s), redefinição de propriedade não configurável,
// arrays e índices, freeze/seal/preventExtensions, __defineGetter__ e __proto__, getters de classe,
// entries/values/assign/fromEntries com getters que mudam o objeto, ordem de chaves, delete e set em strict,
// Reflect.set com receiver, setPrototypeOf).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Os programas rodam em strict (prefixo "use strict") e gravam `globalThis.R`; cada passo é um `T(() => ...)` que
// guarda o valor serializado ou `T:<Erro>: <mensagem>` quando lança.
// Uso: bun scripts/gen-getter-setter-golden.js > tests/golden/accessor_bun.tsv
const fs = require("fs");
const { emitFactoredLines, runPrepared, RESULT_PRELOAD } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRE =
  "const L=[],W=[];" +
  "const S=(v,d=0)=>{const t=typeof v;if(v===null)return'null';if(t==='undefined')return'undefined';" +
  "if(t==='number')return Object.is(v,-0)?'-0':String(v);if(t==='string')return JSON.stringify(v);" +
  "if(t==='boolean')return String(v);if(t==='bigint')return v+'n';if(t==='symbol')return String(v);" +
  "if(t==='function')return'fn:'+v.name+'/'+v.length;if(d>3)return'...';" +
  "if(Array.isArray(v)){if(v.length>64)return'array('+v.length+')';let o=[];for(let i=0;i<v.length;i++)o.push(i in v?S(v[i],d+1):'<hole>');" +
  "const ex=Reflect.ownKeys(v).filter(k=>typeof k==='symbol'||!/^(0|[1-9]\\d*)$/.test(k)).filter(k=>k!=='length');" +
  "return'['+o.join(',')+']'+(ex.length?'+'+ex.map(String).join(','):'')}" +
  "let o=[];for(const k of Reflect.ownKeys(v)){const x=Object.getOwnPropertyDescriptor(v,k);" +
  "o.push(String(k)+(x.enumerable?'':'~')+':'+('value' in x?S(x.value,d+1):'(get:'+S(x.get,d+1)+',set:'+S(x.set,d+1)+')'))}" +
  "return'{'+o.join(',')+'}'};" +
  "const E=f=>{try{return S(f())}catch(e){return'T:'+(e&&e.constructor&&e.constructor.name)+': '+(e&&e.message)}};" +
  "const T=f=>{L.push(E(f))};" +
  "const g=function(){return 1},s=function(v){},g2=()=>2;";

const programs = [];
// Cada passo é uma expressão, ou um bloco `{ ...; return x }`.
const prog = (pre, ...steps) =>
  programs.push(
    PRE + pre + ";" + steps.map(st => "T(()=>" + (st.startsWith("{") ? st : "(" + st + ")") + ");").join("") + 'globalThis.R=L.join(" | ");',
  );
const gopd = "Object.getOwnPropertyDescriptor";

// ---- 1. Todas as combinações de campos do descritor, em propriedade nova e em propriedade existente.
{
  const F = {
    get: ["", "get:undefined", "get:g"],
    set: ["", "set:undefined", "set:s"],
    value: ["", "value:undefined", "value:1"],
    writable: ["", "writable:true", "writable:false"],
    enumerable: ["", "enumerable:true"],
    configurable: ["", "configurable:true"],
  };
  for (const a of F.get)
    for (const b of F.set)
      for (const c of F.value)
        for (const d of F.writable)
          for (const e of F.enumerable)
            for (const f of F.configurable) {
              const desc = "{" + [a, b, c, d, e, f].filter(Boolean).join(",") + "}";
              prog("", `{const o={};Object.defineProperty(o,"p",${desc});return ${gopd}(o,"p")}`);
              prog("", `{const o={p:5};Object.defineProperty(o,"p",${desc});return ${gopd}(o,"p")}`);
            }
}

// ---- 2. get/set que não são função.
{
  const vals = ["1", '"x"', "null", "{}", "[]", "true", "Symbol()", "undefined", "class{}", "async function(){}", "function*(){}", "new Proxy(function(){},{})", "new Proxy({},{})", "0n", "NaN"];
  for (const v of vals) {
    prog("", `{const o={};Object.defineProperty(o,"p",{get:${v}});return ${gopd}(o,"p")}`);
    prog("", `{const o={};Object.defineProperty(o,"p",{set:${v}});return ${gopd}(o,"p")}`);
    prog("", `{const o={};Object.defineProperty(o,"p",{get:g,set:${v}});return ${gopd}(o,"p")}`);
    prog("", `{const o={};Object.defineProperties(o,{p:{get:${v}}});return ${gopd}(o,"p")}`);
  }
}

// ---- 3. Descritor com valores herdados, getters e Proxy.
{
  const log = n => `W.push("${n}")`;
  const descs = [
    "Object.create({value:1})",
    "Object.create({get:g})",
    "Object.create({set:s,enumerable:true})",
    "Object.create({value:1,writable:true,enumerable:true,configurable:true})",
    "Object.create(Object.create({value:2}))",
    "{__proto__:{get:g},value:1}",
    "{__proto__:{value:1},get:g}",
    "Object.create({},{value:{value:1}})",
    "Object.create({},{get:{get(){return g},enumerable:false}})",
    "Object.create({},{get:{get(){return g},enumerable:true}})",
    "{get enumerable(){" + log("enum") + ";return true},get configurable(){" + log("conf") + ";return true},get value(){" + log("val") + ";return 1},get writable(){" + log("wr") + ";return true}}",
    "{get enumerable(){" + log("enum") + ";return true},get configurable(){" + log("conf") + ";return true},get get(){" + log("get") + ";return g},get set(){" + log("set") + ";return s}}",
    "{get set(){" + log("set") + ";return s},get get(){" + log("get") + ";return g},get configurable(){" + log("conf") + ";return 1},get enumerable(){" + log("enum") + ";return 0}}",
    "{get value(){" + log("val") + ";return 1},get get(){" + log("get") + ";return undefined}}",
    "{get get(){" + log("get") + ";throw new RangeError('boom')},value:1}",
    "{get value(){" + log("val") + ";throw new EvalError('boom2')},enumerable:true}",
    "new Proxy({value:1},{has(t,k){W.push('has:'+String(k));return k in t},get(t,k){W.push('get:'+String(k));return t[k]}})",
    "new Proxy({get:g},{has(t,k){W.push('has:'+String(k));return k in t},get(t,k){W.push('get:'+String(k));return t[k]}})",
    "new Proxy({},{has(t,k){W.push('has:'+String(k));return false},get(t,k){W.push('get:'+String(k));return 1}})",
    "new Proxy({},{has(t,k){W.push('has:'+String(k));return true},get(t,k){W.push('get:'+String(k));return k==='value'?3:undefined}})",
    "new Proxy({},{has(t,k){return k==='get'},get(t,k){return g}})",
    "new Proxy({value:1},{})",
    "new Proxy([],{})",
    "Object.assign(function(){},{value:1})",
    "Object.assign([],{value:1,enumerable:true})",
    "new String('x')",
    "new Number(5)",
    "new Boolean(false)",
    "new Date(0)",
    "/x/",
    "new Map([[1,2]])",
    "1",
    '"x"',
    "null",
    "undefined",
    "true",
    "Symbol('d')",
    "1n",
    "[]",
    "function(){}",
    "class{}",
    "{}",
    "{enumerable:1,configurable:'x',writable:{}}",
    "{enumerable:0,configurable:'',writable:null,value:2}",
    "{enumerable:NaN,configurable:-0,writable:0n,value:2}",
    "{enumerable:Symbol(),value:2}",
    "{enumerable:undefined,configurable:undefined,writable:undefined,value:undefined}",
  ];
  for (const d of descs) {
    prog("", `{const o={};Object.defineProperty(o,"p",${d});return [${gopd}(o,"p"),W.slice()]}`);
    prog("", `{const o={};Object.defineProperties(o,{p:${d}});return [${gopd}(o,"p"),W.slice()]}`);
    prog("", `{const o=Object.create(null,{p:${d}});return [${gopd}(o,"p"),W.slice()]}`);
    prog("", `Reflect.defineProperty({},"p",${d})`);
  }
}

// ---- 4. Redefinição de propriedade não configurável.
{
  const bases = [
    "{value:1,writable:true}",
    "{value:1,writable:false}",
    "{get:g,set:s}",
    "{get:g}",
    "{set:s}",
    "{value:NaN,writable:false}",
    "{value:-0,writable:false}",
    "{get:undefined,set:undefined}",
    "{value:undefined,writable:false}",
  ];
  const news = [
    "{value:1}", "{value:2}", "{value:NaN}", "{value:-0}", "{value:0}", "{value:undefined}",
    "{writable:true}", "{writable:false}", "{enumerable:true}", "{enumerable:false}",
    "{configurable:true}", "{configurable:false}",
    "{get:g}", "{get:g2}", "{get:undefined}", "{set:s}", "{set:undefined}", "{get:g,set:s}",
    "{value:1,writable:false}", "{value:2,writable:true}", "{}", "{get:undefined,set:undefined}",
    "{value:1,get:g}", "{writable:true,get:g}", "{get:g,set:undefined}", "{set:s,get:undefined}",
  ];
  for (const b of bases)
    for (const n of news)
      for (const en of ["", "enumerable:true,"]) {
        const base = "{" + en + b.slice(1);
        prog(
          "",
          `{const o={};Object.defineProperty(o,"p",${base});Object.defineProperty(o,"p",${n});return ${gopd}(o,"p")}`,
        );
      }
  // Mesmas mudanças via Reflect.defineProperty (devolve booleano) e via defineProperties.
  for (const b of bases.slice(0, 4))
    for (const n of news.slice(0, 18)) {
      prog("", `{const o={};Object.defineProperty(o,"p",${b});return [Reflect.defineProperty(o,"p",${n}),${gopd}(o,"p")]}`);
    }
  // Configurável: tudo é permitido.
  for (const n of news.slice(0, 18))
    prog("", `{const o={};Object.defineProperty(o,"p",{value:1,writable:false,configurable:true});Object.defineProperty(o,"p",${n});return ${gopd}(o,"p")}`);
  for (const n of news.slice(0, 18))
    prog("", `{const o={};Object.defineProperty(o,"p",{get:g,set:s,configurable:true});Object.defineProperty(o,"p",${n});return ${gopd}(o,"p")}`);
  // Atribuição direta em propriedades não graváveis.
  for (const b of bases.slice(0, 5))
    prog("", `{const o={};Object.defineProperty(o,"p",${b});o.p=7;return ${gopd}(o,"p")}`, `{const o={};Object.defineProperty(o,"p",${b});delete o.p}`);
}

// ---- 5. Arrays e índices.
{
  const mk = ["[]", "[1,2,3]", "[1,,3]"];
  const ops = [
    "a.push(9)", "a.pop()", "a.shift()", "a.unshift(0)", "a.splice(0,1)", "a.splice(1,0,5)", "a.length=0", "a.length=1", "a.length=10",
    "a[5]=1", "a[0]=7", "a[a.length]=1", "a.reverse()", "a.sort()", "a.fill(0)", "a.copyWithin(0,1)",
    'Object.defineProperty(a,"length",{value:0})', 'Object.defineProperty(a,"length",{value:5})',
    "Object.defineProperty(a,7,{value:1})", "Object.defineProperty(a,0,{value:1})", "delete a[0]", "a.concat([1])",
    "a.slice(1)", "a.toSpliced(0,1)", "Object.freeze(a)", "Array.prototype.push.call(a,1,2)", "a.lastIndexOf(1)",
    'Reflect.defineProperty(a,"length",{value:0})', 'Reflect.set(a,"length",0)', 'Reflect.set(a,5,1)', "Reflect.deleteProperty(a,'length')",
  ];
  for (const m of mk)
    for (const op of ops) {
      prog("", `{const a=${m};Object.defineProperty(a,"length",{writable:false});return [${op},a,${gopd}(a,"length")]}`);
      prog("", `{const a=${m};${op};return [a,${gopd}(a,"length")]}`);
    }
  // Índice no limite de 2**32.
  const idx = ["4294967293", "4294967294", "4294967295", "4294967296", '"-1"', '"01"', '"1.5"', "1e21", "2**53", '"4294967294"', '"4294967295"', "-0", "0.5"];
  for (const i of idx) {
    prog("", `{const a=[];Object.defineProperty(a,${i},{value:1,configurable:true,writable:true,enumerable:true});return [a.length,Object.keys(a),${gopd}(a,${i})]}`);
    prog("", `{const a=[];a[${i}]=1;return [a.length,Object.keys(a)]}`);
    prog("", `{const a=[1];Object.defineProperty(a,"length",{writable:false});Object.defineProperty(a,${i},{value:1});return [a.length]}`);
    prog("", `{const a=[1];Object.defineProperty(a,"length",{writable:false});a[${i}]=1;return [a.length]}`);
    prog("", `{const o={};o[${i}]=1;return [Object.keys(o),Reflect.ownKeys(o)]}`);
    prog("", `{const a=[];a[${i}]=1;return [a.length,a.push(2)]}`);
    prog("", `{const a=[];a[4294967294]=1;a.length=0;return [a.length,Object.keys(a)]}`);
  }
  // Valores de length.
  const lens = ["2**32", "2**32-1", "-1", "1.5", '"3"', "NaN", "Infinity", "-0", "{valueOf(){return 2}}", "{valueOf(){return 2},toString(){return '9'}}", "1n", "null", "undefined", "true", "[]", "[5]", "Symbol()", "'abc'", "4294967295.0", "4294967296"];
  for (const l of lens) {
    prog("", `{const a=[1,2,3];a.length=${l};return a.length}`);
    prog("", `{const a=[1,2,3];Object.defineProperty(a,"length",{value:${l}});return a.length}`);
    prog("", `{const a=[];return new Array(${l}).length}`);
    prog("", `{const a=[1,2,3];return Reflect.defineProperty(a,"length",{value:${l}})}`);
  }
  const ldesc = ["{configurable:true}", "{enumerable:true}", "{get:g}", "{set:s}", "{writable:true}", "{writable:false}", "{value:1,writable:false}", "{configurable:false}", "{enumerable:false}", "{value:3}", "{value:2,writable:false}", "{}"];
  for (const d of ldesc) {
    prog("", `{const a=[1,2,3];Object.defineProperty(a,"length",${d});return [a,${gopd}(a,"length")]}`);
    prog("", `{const a=[1,2,3];Object.defineProperty(a,"length",{writable:false});Object.defineProperty(a,"length",${d});return [a,${gopd}(a,"length")]}`);
  }
  // Encolher com elementos não configuráveis no meio.
  for (const k of [0, 1, 2, 4])
    for (const nl of [0, 1, 3]) {
      prog("", `{const a=[1,2,3,4,5];Object.defineProperty(a,${k},{value:9,configurable:false});a.length=${nl};return [a,a.length]}`);
      prog("", `{const a=[1,2,3,4,5];Object.defineProperty(a,${k},{value:9,configurable:false});return [Reflect.set(a,"length",${nl}),a.length]}`);
      prog("", `{const a=[1,2,3,4,5];Object.defineProperty(a,${k},{value:9,configurable:false});Object.defineProperty(a,"length",{value:${nl}});return [a.length]}`);
    }
  // Acessor em índice de array.
  prog("", `{const a=[1,2,3];Object.defineProperty(a,1,{get:g});return [a,a[1],a.slice(),a.map(x=>x*2),JSON.stringify(a),a.indexOf(1),a.includes(1),a.join()]}`);
  prog("", `{const a=[];Object.defineProperty(a,0,{get(){W.push("g0");return 5},enumerable:true,configurable:true});return [a.length,a[0],[...a],Array.from(a),a.concat([1]),W]}`);
  prog("", `{const a=[1,2];Object.defineProperty(a,2,{set:s});return [a.length,a[2],a.join()]}`);
  prog("", `{const a=[1,2,3];Object.defineProperty(a,1,{get:g,configurable:false});a.sort();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,1,{value:5,writable:false});a.sort();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,1,{value:5,writable:false});a.reverse();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,1,{value:5,configurable:false});a.splice(0,3);return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,1,{value:5,configurable:false});a.pop();a.pop();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,1,{value:5,configurable:false});a.shift();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,2,{value:5,writable:false});a.pop();return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,2,{value:5,writable:false});a.push(1);return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,0,{value:5,writable:false});a.unshift(1);return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,0,{value:5,writable:false});a.fill(0);return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,0,{value:5,writable:false});a.copyWithin(0,1);return a}`);
  prog("", `{const a=[3,2,1];Object.defineProperty(a,0,{value:5,writable:false});a.splice(0,1,9);return a}`);
}

// ---- 6. frozen, sealed e não extensíveis.
{
  const objs = [
    "[1,2]", "[]", "{a:1}", "{}", "new String('ab')", "new String('')", "function f(){}", "class C{static s=1}", "new Map([[1,2]])", "new Set([1])",
    "new Uint8Array(0)", "new Uint8Array(2)", "new Float64Array(0)", "new Float64Array(1)", "new BigInt64Array(1)", "new DataView(new ArrayBuffer(2))", "new ArrayBuffer(2)",
    "new Uint8Array(new ArrayBuffer(8),0,0)", "new Uint8Array(new ArrayBuffer(8),8)", "new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))", "new Uint8Array(new ArrayBuffer(0,{maxByteLength:16}))",
    "Object.create(null)", "(function(){return arguments})(1,2)", "(function(){return arguments})()", "new Number(1)", "/x/g", "new Date(0)", "new Error('e')",
    "Promise.resolve()", "Math", "JSON", "Symbol.prototype", "Object(Symbol())", "Object(1n)", "new Proxy({a:1},{})", "new Proxy([1],{})", "Array.prototype",
    "{get a(){return 1},set a(v){}}", "[1,,3]", "Object.create({inherited:1})",
  ];
  for (const o of objs)
    for (const op of ["freeze", "seal", "preventExtensions"]) {
      prog(
        `const o=${o}`,
        `Object.${op}(o)===o`,
        "[Object.isFrozen(o),Object.isSealed(o),Object.isExtensible(o)]",
        "{o.newprop=1;return o.newprop}",
        "{const k=Reflect.ownKeys(o)[0];o[k]=42;return [String(k),S(o[k])]}",
        "{const k=Reflect.ownKeys(o)[0];delete o[k];return Reflect.ownKeys(o).length}",
        '{Object.defineProperty(o,"zzz",{value:1});return 1}',
        "Reflect.defineProperty(o,'zzz',{value:1})",
        "Reflect.set(o,'zzz',1)",
        "Reflect.preventExtensions(o)",
        "Reflect.isExtensible(o)",
      );
    }
  const prims = ["1", '"s"', "null", "undefined", "Symbol()", "true", "1n", "NaN"];
  for (const p of prims)
    for (const op of ["freeze", "seal", "preventExtensions", "isFrozen", "isSealed", "isExtensible"])
      prog("", `Object.${op}(${p})`);
  // Mutações específicas em frozen.
  prog("", "{const a=Object.freeze([1,2,3]);return [a.push(4)]}", "{const a=Object.freeze([1,2,3]);return a.pop()}", "{const a=Object.freeze([1,2,3]);a.length=0}", "{const a=Object.freeze([1,2,3]);a[0]=9}", "{const a=Object.freeze([1,2,3]);return a.sort()}", "{const a=Object.freeze([1,2,3]);return a.concat([4])}", "{const a=Object.freeze([1,2,3]);return a.shift()}", "{const a=Object.freeze([1,2,3]);return a.unshift(1)}", "{const a=Object.freeze([1,2,3]);return a.splice(0,1)}", "{const a=Object.freeze([1,2,3]);return a.fill(0)}", "{const a=Object.freeze([1,2,3]);return a.reverse()}", "{const a=Object.freeze([1,2,3]);return a.copyWithin(0,1)}");
  prog("", "{const a=Object.seal([1,2,3]);a[0]=9;a.length=2;return a}", "{const a=Object.seal([1,2,3]);return a.push(4)}", "{const a=Object.seal([1,2,3]);return a.pop()}", "{const a=Object.seal([1,2,3]);return a.sort((x,y)=>y-x)}", "{const a=Object.seal([1,2,3]);return a.length=0}");
  prog("", "{const a=Object.preventExtensions([1,2,3]);a.push(4)}", "{const a=Object.preventExtensions([1,2,3]);a.pop();return a}", "{const a=Object.preventExtensions([1,2,3]);a.length=5;return a}", "{const a=Object.preventExtensions([1,2,3]);a[5]=1}", "{const a=Object.preventExtensions([1,2,3]);return a.shift()}", "{const a=Object.preventExtensions([1,2,3]);return a.unshift(1)}");
  prog("", "{const s=Object.freeze(new String('ab'));s[0]='z';return s[0]}", "{const s=Object.freeze(new String('ab'));s[5]='z'}", "{const s=new String('ab');s[0]='z'}", "{const s=new String('ab');s.length=1}", "{const s=new String('ab');delete s[0]}", "{const s=new String('ab');return [Object.getOwnPropertyDescriptor(s,0),Object.getOwnPropertyDescriptor(s,'length'),Object.keys(s)]}", "{const s=new String('ab');Object.defineProperty(s,0,{value:'a'});return 1}", "{const s=new String('ab');Object.defineProperty(s,0,{value:'b'})}", "{const s=new String('ab');Object.defineProperty(s,2,{value:'c'});return [s.length,Object.keys(s)]}", "{const s=new String('ab');Object.defineProperty(s,'length',{value:5})}");
  prog("", "{const f=Object.freeze(function(){});f.x=1}", "{const f=Object.freeze(function(){});f.prototype=1}", "{const f=Object.freeze(function(){});return [Object.getOwnPropertyDescriptor(f,'prototype'),Object.getOwnPropertyDescriptor(f,'name'),Object.getOwnPropertyDescriptor(f,'length')]}", "{const f=Object.freeze(()=>1);return [Object.isFrozen(f),Reflect.ownKeys(f)]}", "{const C=Object.freeze(class{static s=1});C.s=2}", "{const C=Object.freeze(class{static s=1});return Object.isFrozen(C.prototype)}", "{class C{static get g(){return 1}};Object.freeze(C);return [Object.getOwnPropertyDescriptor(C,'g')]}", "{const C=Object.freeze(class{});C.prototype.x=1;return C.prototype.x}");
  prog("", "{const a=Object.freeze(new Uint8Array(0));return Object.isFrozen(a)}", "{const a=new Uint8Array(0);Object.seal(a);return Object.isSealed(a)}", "{const a=new Uint8Array(2);Object.seal(a);return Object.isSealed(a)}", "{const a=new Uint8Array(2);Object.freeze(a)}", "{const a=new Uint8Array(2);Object.seal(a)}", "{const a=new Uint8Array(2);Object.preventExtensions(a);return Object.isExtensible(a)}", "{const a=new Uint8Array(2);Object.defineProperty(a,0,{value:1,writable:false})}", "{const a=new Uint8Array(2);Object.defineProperty(a,0,{value:1,configurable:false})}", "{const a=new Uint8Array(2);Object.defineProperty(a,0,{value:1,enumerable:false})}", "{const a=new Uint8Array(2);Object.defineProperty(a,0,{get:g})}", "{const a=new Uint8Array(2);Object.defineProperty(a,5,{value:1})}", "{const a=new Uint8Array(2);return [Reflect.defineProperty(a,5,{value:1}),Reflect.defineProperty(a,0,{value:3}),a[0]]}", "{const a=new Uint8Array(2);return Object.getOwnPropertyDescriptor(a,0)}", "{const a=new Uint8Array(2);a[5]=1;a['-0']=1;a['1.5']=1;return [Object.keys(a),a[5]]}", "{const a=new Uint8Array(2);delete a[0]}", "{const a=new Uint8Array(2);delete a[5];return 1}", "{const a=new Uint8Array(2);return Reflect.deleteProperty(a,0)}", "{const a=new Uint8Array(2);Object.defineProperty(a,'x',{value:1});Object.freeze(a)}");
}

// ---- 7. __defineGetter__, __lookupGetter__ e __proto__ como acessor.
{
  const dg = [
    '{const o={};o.__defineGetter__("x",g);return [${gopd}(o,"x")]}'.replace("${gopd}", gopd),
    '{const o={};o.__defineSetter__("x",s);return [' + gopd + '(o,"x")]}',
    '{const o={x:1};o.__defineGetter__("x",g);return ' + gopd + '(o,"x")}',
    '{const o={x:1};o.__defineGetter__("x",g);o.__defineSetter__("x",s);return ' + gopd + '(o,"x")}',
    '{const o={};o.__defineGetter__("x",1)}',
    '{const o={};o.__defineGetter__("x")}',
    '{const o={};o.__defineSetter__("x",{})}',
    '{const o={};o.__defineGetter__("x",null)}',
    "{const o={};o.__defineGetter__(Symbol.iterator,g);return Reflect.ownKeys(o)}",
    "{const o={};o.__defineGetter__(1,g);return [Reflect.ownKeys(o),o[1]]}",
    "{const o={};o.__defineGetter__({toString(){return 'k'}},g);return Reflect.ownKeys(o)}",
    "{const o={};o.__defineGetter__({toString(){throw new RangeError('ts')}},g)}",
    "{const o={};o.__defineGetter__({toString(){W.push('ts');return 'k'}},{});return W}",
    '{const o=Object.freeze({});o.__defineGetter__("x",g)}',
    '{const o=Object.preventExtensions({});o.__defineGetter__("x",g)}',
    '{const o={};Object.defineProperty(o,"x",{value:1});o.__defineGetter__("x",g)}',
    '{const o={};Object.defineProperty(o,"x",{value:1,configurable:true});o.__defineGetter__("x",g);return ' + gopd + '(o,"x")}',
    '{const o={};Object.defineProperty(o,"x",{get:g,configurable:false});o.__defineGetter__("x",g);return 1}',
    '{const o={};Object.defineProperty(o,"x",{get:g,configurable:false});o.__defineGetter__("x",g2);return 1}',
    '{const o={};Object.defineProperty(o,"x",{get:g,configurable:false});o.__defineSetter__("x",s);return 1}',
    '{const o={};Object.defineProperty(o,"x",{get:g,configurable:true});o.__defineSetter__("x",s);return ' + gopd + '(o,"x")}',
    "Object.prototype.__defineGetter__.call(null,'x',g)",
    "Object.prototype.__defineGetter__.call(undefined,'x',g)",
    "Object.prototype.__defineGetter__.call(1,'x',g)",
    "Object.prototype.__defineGetter__.call('s','x',g)",
    "Object.prototype.__defineGetter__.call('s','0',g)",
    "Object.prototype.__defineSetter__.call(null,'x',s)",
    "Object.prototype.__lookupGetter__.call(null,'x')",
    "Object.prototype.__lookupGetter__.call(undefined,'x')",
    "Object.prototype.__lookupGetter__.call(1,'x')",
    "Object.prototype.__lookupSetter__.call(null,'x')",
    '({get x(){return 1}}).__lookupGetter__("x")',
    '({get x(){return 1}}).__lookupSetter__("x")',
    '({set x(v){}}).__lookupSetter__("x")',
    '({set x(v){}}).__lookupGetter__("x")',
    '({x:1}).__lookupGetter__("x")',
    '({x:1}).__lookupSetter__("x")',
    '({}).__lookupGetter__("nope")',
    '({__proto__:{get x(){return 1}}}).__lookupGetter__("x")',
    '({__proto__:{get x(){return 1}}}).__lookupSetter__("x")',
    '({__proto__:{x:1},get x(){return 2}}).__lookupGetter__("x")',
    '({get x(){return 1},__proto__:{set x(v){}}}).__lookupSetter__("x")',
    '({x:1,__proto__:{get x(){return 2}}}).__lookupGetter__("x")',
    "({get [Symbol.iterator](){return 1}}).__lookupGetter__(Symbol.iterator)",
    "({get 1(){return 1}}).__lookupGetter__(1)",
    "({get 1(){return 1}}).__lookupGetter__('1')",
    "({}).__lookupGetter__({toString(){throw new RangeError('ts')}})",
    "({}).__lookupGetter__()",
    "[].__lookupGetter__('length')",
    "Object.prototype.__lookupGetter__('__proto__')===Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get",
    "Object.prototype.__lookupSetter__('__proto__')===Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set",
    "(new Proxy({},{getOwnPropertyDescriptor(t,k){W.push('gopd:'+String(k));return undefined},getPrototypeOf(t){W.push('gpo');return null}})).__lookupGetter__('x')",
    "[Object.getOwnPropertyDescriptor(Object.prototype,'__proto__'),Object.getOwnPropertyDescriptor(Object.prototype,'__defineGetter__')]",
    "[Object.prototype.__defineGetter__.name,Object.prototype.__defineGetter__.length,Object.prototype.__lookupSetter__.length,Object.prototype.__defineSetter__.name]",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.call(1)===Number.prototype",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.call(null)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.call(undefined)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(null,{})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(undefined,{})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(1,{})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call({},1)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call({})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call({},null)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.freeze({}),{})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.freeze({}),Object.prototype)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.preventExtensions({}),null)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.prototype,{})",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.prototype,null)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.prototype,Object.prototype)",
    "Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.prototype,1)",
    "{const o={};o.__proto__=null;return [Object.getPrototypeOf(o),o.__proto__]}",
    "{const o={};o.__proto__=1;return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={};o.__proto__='x';return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={};o.__proto__=undefined;return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={};o.__proto__=o}",
    "{const a={},b={__proto__:a};a.__proto__=b}",
    "{const a={},b={__proto__:a},c={__proto__:b};a.__proto__=c}",
    "{const o=Object.preventExtensions({});o.__proto__={}}",
    "{const o=Object.preventExtensions({});o.__proto__=Object.prototype;return 1}",
    "{const o=Object.freeze({});o.__proto__=null}",
    "{const o={};o.__proto__=function(){};return typeof o.__proto__}",
    "{const o=Object.create(null);o.__proto__=5;return [Reflect.ownKeys(o),o.__proto__]}",
    "{const o=Object.create(null);return [o.__proto__,'__proto__' in o]}",
    "{const o={__proto__:null};return [Object.getPrototypeOf(o),'__proto__' in o]}",
    "{const o={['__proto__']:1};return [Reflect.ownKeys(o),Object.getPrototypeOf(o)===Object.prototype,o.__proto__]}",
    "{const __proto__=1;const o={__proto__};return [Reflect.ownKeys(o),Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o={__proto__(){return 1}};return [Reflect.ownKeys(o),Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o={get __proto__(){return 1}};return [Reflect.ownKeys(o),o.__proto__,Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o={__proto__:1};return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={__proto__:'x'};return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={__proto__:{a:1}};return [o.a,Reflect.ownKeys(o)]}",
    "{const o={__proto__:Array.prototype};return [o.length,Array.isArray(o)]}",
    "{const o=JSON.parse('{\"__proto__\":1}');return [Reflect.ownKeys(o),Object.getPrototypeOf(o)===Object.prototype,o.__proto__]}",
    "{const o=JSON.parse('{\"__proto__\":{\"a\":1}}');return [Reflect.ownKeys(o),o.a,o.__proto__]}",
    "{const o=Object.assign({},JSON.parse('{\"__proto__\":{\"a\":1}}'));return [Reflect.ownKeys(o),o.a]}",
    "{const o={...JSON.parse('{\"__proto__\":{\"a\":1}}')};return [Reflect.ownKeys(o),o.a]}",
    "{const o={};Object.defineProperty(o,'__proto__',{value:5,enumerable:true});return [Reflect.ownKeys(o),o.__proto__,Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o={};Object.defineProperty(o,'__proto__',{get:g});return [o.__proto__,Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o={};Object.defineProperty(o,'__proto__',{set:s});o.__proto__={};return Object.getPrototypeOf(o)===Object.prototype}",
    "{const o={};Object.defineProperty(Object.prototype,'__proto__',{get:undefined});}",
    "{class C{get __proto__(){return 7}};return [new C().__proto__,Object.getPrototypeOf(new C())===C.prototype]}",
    "{class C{static __proto__=1};return [Object.getPrototypeOf(C)===Function.prototype,Reflect.ownKeys(C)]}",
    "{const o={x:1};const p={__proto__:o};p.x=2;return [o.x,Reflect.ownKeys(p)]}",
    "{const f=function(){};f.__proto__=null;return [typeof f,Object.getPrototypeOf(f),f.call]}",
    "{const a=[];a.__proto__=Object.prototype;return [Array.isArray(a),a.push]}",
    "{const o={};const r=Reflect.setPrototypeOf(o,o);return r}",
    "{const o={};return Reflect.setPrototypeOf(o,1)}",
    "{const o={};return Reflect.setPrototypeOf(o,undefined)}",
    "{return Reflect.setPrototypeOf(1,null)}",
    "{return Reflect.setPrototypeOf(Object.freeze({}),null)}",
    "{return Reflect.setPrototypeOf(Object.freeze({}),Object.prototype)}",
    "{return Reflect.setPrototypeOf(Object.prototype,{})}",
    "{return Reflect.setPrototypeOf(Object.prototype,null)}",
    "{return Reflect.setPrototypeOf(Object.preventExtensions({}),{})}",
  ];
  for (const st of dg) prog("", st.startsWith("{") ? st : st);
}

// ---- 8. Getters e setters de classe e de literal.
{
  const cs = [
    "{class C{get x(){return 1}};return [new C().x,Object.getOwnPropertyDescriptor(C.prototype,'x')]}",
    "{class C{set x(v){}};return [new C().x,Object.getOwnPropertyDescriptor(C.prototype,'x')]}",
    "{class C{get x(){return 1} set x(v){W.push(v)}};const c=new C();c.x=5;return [c.x,W,Object.getOwnPropertyDescriptor(C.prototype,'x')]}",
    "{class C{get x(){return 1}};const c=new C();c.x=2}",
    "{class C{get x(){return 1}};const c=new C();return Reflect.set(c,'x',2)}",
    "{class C{get x(){return 1}};const c=new C();Object.defineProperty(c,'x',{value:3});return [c.x,Reflect.ownKeys(c)]}",
    "{class C{get x(){return 1}};const c=new C();Object.defineProperty(c,'x',{value:3,writable:true});c.x=4;return [c.x]}",
    "{class C{static get x(){return 1}};return [C.x,Object.getOwnPropertyDescriptor(C,'x')]}",
    "{class C{static get x(){return 1}};C.x=2}",
    "{class C{static set x(v){W.push(v)}};C.x=2;return [C.x,W]}",
    "{class C{static get name(){return 'nm'}};return [C.name,Object.getOwnPropertyDescriptor(C,'name')]}",
    "{class C{static get length(){return 9}};return [C.length,Object.getOwnPropertyDescriptor(C,'length')]}",
    "{class C{static get prototype(){return 1}}}",
    "{class C{get constructor(){return 1}};return [new C().constructor,C.prototype.constructor]}",
    "{class C{constructor(){}get constructor(){return 1}}}",
    "{class C{static get constructor(){return 1}};return C.constructor}",
    "{class C{get x(){return 1}};return [Object.getOwnPropertyDescriptor(C.prototype,'x').get.name,Object.getOwnPropertyDescriptor(C.prototype,'x').get.length]}",
    "{class C{set x(v){}};return [Object.getOwnPropertyDescriptor(C.prototype,'x').set.name,Object.getOwnPropertyDescriptor(C.prototype,'x').set.length]}",
    "{class C{get [Symbol.iterator](){return 1}};return [Object.getOwnPropertyDescriptor(C.prototype,Symbol.iterator).get.name]}",
    "{class C{get ['a'+'b'](){return 1}};return [Object.getOwnPropertyDescriptor(C.prototype,'ab').get.name]}",
    "{class C{get 1(){return 1}};return [Object.getOwnPropertyDescriptor(C.prototype,'1').get.name,new C()[1]]}",
    "{class C{get #p(){return 1}};}",
    "{class C{get #p(){return 5} read(){return this.#p}};return new C().read()}",
    "{class C{set #p(v){W.push(v)} write(v){this.#p=v}};new C().write(3);return W}",
    "{class C{get #p(){return 5} write(){this.#p=1}};new C().write()}",
    "{class C{set #p(v){} read(){return this.#p}};return new C().read()}",
    "{class C{static get #p(){return 5} static read(){return this.#p}};return C.read()}",
    "{class C{get #p(){return 5} static read(o){return o.#p}};return C.read({})}",
    "{class C{get x(){return 1}};class D extends C{get x(){return super.x+1}};return new D().x}",
    "{class C{get x(){return 1}};class D extends C{};const d=new D();d.x=5}",
    "{class C{get x(){return this.v} set x(v){this.v=v}};class D extends C{set x(v){super.x=v*2}};const d=new D();d.x=2;return [d.x,Object.keys(d)]}",
    "{class C{get x(){return 1}};class D extends C{set x(v){}};return [new D().x,Object.getOwnPropertyDescriptor(D.prototype,'x')]}",
    "{class C{get x(){return 1} set x(v){}};class D extends C{get x(){return 2}};const d=new D();d.x=1;return [d.x]}",
    "{class C{static get x(){return 1}};class D extends C{};D.x=3}",
    "{class C{static get x(){return this.n}};class D extends C{static n=5};return [C.x,D.x]}",
    "{class C{get x(){return 1}};return Object.keys(new C())}",
    "{class C{get x(){return 1}};return [Object.getOwnPropertyDescriptor(C.prototype,'x').enumerable,Object.getOwnPropertyDescriptor(C.prototype,'x').configurable]}",
    "{class C{get x(){return 1}};C.prototype.x=3}",
    "{class C{get x(){return 1}};delete C.prototype.x;return new C().x}",
    "{class C{get x(){return 1}};Object.defineProperty(C.prototype,'x',{value:2});return new C().x}",
    "{class C{get x(){return 1}};const d=Object.getOwnPropertyDescriptor(C.prototype,'x');return d.get.call({})}",
    "{class C{get x(){return 1}};const d=Object.getOwnPropertyDescriptor(C.prototype,'x');return new d.get()}",
    "{class C{get x(){return 1}};const d=Object.getOwnPropertyDescriptor(C.prototype,'x');return d.get.hasOwnProperty('prototype')}",
    "{class C{get x(){return 1}};const d=Object.getOwnPropertyDescriptor(C.prototype,'x');return d.get.toString()}",
    "{class C{static set x(v){}};const d=Object.getOwnPropertyDescriptor(C,'x');return d.set.toString()}",
    "{class C{get x(){throw new RangeError('gx')}};return new C().x}",
    "{class C{static get [Symbol.species](){return 1}};return C[Symbol.species]}",
    "{class A extends Array{static get [Symbol.species](){return Array}};const a=new A(1,2,3);return [a.map(x=>x) instanceof A]}",
    "{const o={get x(){return 1},set x(v){}};return [Object.getOwnPropertyDescriptor(o,'x'),Object.getOwnPropertyDescriptor(o,'x').get.name]}",
    "{const o={get x(){return 1},x:2};return Object.getOwnPropertyDescriptor(o,'x')}",
    "{const o={x:2,get x(){return 1}};return Object.getOwnPropertyDescriptor(o,'x')}",
    "{const o={get x(){return 1},set x(v){},get x(){return 2}};return [o.x,Object.getOwnPropertyDescriptor(o,'x').set.name]}",
    "{const o={get x(){return 1},set x(v){},y:1,get x(){return 2}};return Reflect.ownKeys(o)}",
    "{const o={get [Symbol.toStringTag](){return 'Q'}};return String(o)}",
    "{const o={get ['k'+1](){return 1}};return [Object.getOwnPropertyDescriptor(o,'k1').get.name]}",
    "{const o={get 0x10(){return 1}};return Reflect.ownKeys(o)}",
    "{const o={get 1.5(){return 1}};return Reflect.ownKeys(o)}",
    "{const o={get 'a b'(){return 1}};return Object.getOwnPropertyDescriptor(o,'a b').get.name}",
    "{const o={get get(){return 1},set set(v){},get set(){return 2},set get(v){}};return [Reflect.ownKeys(o),o.get,o.set]}",
    "{const o={get x(){return 1}};o.x=2}",
    "{const o={set x(v){}};return o.x}",
    "{const o={get x(){return 1}};return Object.assign({},o)}",
    "{const o={get x(){return 1}};return {...o}}",
    "{const o={get x(){return 1}};const p=Object.create(o);p.x=2}",
    "{const o={get x(){return 1}};const p=Object.create(o);Object.defineProperty(p,'x',{value:2});return [p.x,o.x]}",
    "{const o={set x(v){W.push(this===p)}};const p=Object.create(o);p.x=2;return [W,Reflect.ownKeys(p)]}",
    "{const o={get x(){return this===p}};const p=Object.create(o);return p.x}",
    "{const o={get x(){return typeof this}};return [Object.getOwnPropertyDescriptor(o,'x').get.call(1)]}",
    "{const o={get x(){return typeof this}};Object.defineProperty(Number.prototype,'xx',Object.getOwnPropertyDescriptor(o,'x'));return (5).xx}",
    "{Object.defineProperty(Number.prototype,'sloppy',{get(){return this},configurable:true});const r=typeof (5).sloppy;delete Number.prototype.sloppy;return r}",
    "{Object.defineProperty(String.prototype,'sl',{set(v){W.push(typeof this)},configurable:true});'abc'.sl=1;const r=W.slice();delete String.prototype.sl;return r}",
    "{Object.defineProperty(String.prototype,'sl',{get(){return 1},configurable:true});try{'abc'.sl=1}finally{delete String.prototype.sl}}",
    "{Object.defineProperty(Number.prototype,'sl',{value:1,configurable:true});try{(5).sl=2}finally{delete Number.prototype.sl}}",
    "{Object.defineProperty(Number.prototype,'sl',{value:1,configurable:true,writable:true});try{(5).sl=2}finally{delete Number.prototype.sl}}",
    "{const o=Object.defineProperties({},{a:{get:g,enumerable:true},b:{value:2,enumerable:true},c:{get:g2}});return [Object.keys(o),Object.values(o),Object.entries(o),JSON.stringify(o),{...o}]}",
  ];
  for (const st of cs) prog("", st);
}

// ---- 9. entries/values/assign/fromEntries/keys com getters que mudam o objeto.
{
  const es = [
    "{const o={get a(){delete this.b;return 1},b:2,c:3};return [Object.entries(o),Reflect.ownKeys(o)]}",
    "{const o={get a(){delete this.b;return 1},b:2,c:3};return Object.values(o)}",
    "{const o={a:0,get b(){delete this.c;return 1},c:3};return Object.entries(o)}",
    "{const o={a:0,get b(){this.d=4;return 1},c:3};return [Object.entries(o),Reflect.ownKeys(o)]}",
    "{const o={a:0,get b(){Object.defineProperty(this,'c',{enumerable:false});return 1},c:3};return Object.entries(o)}",
    "{const o={a:0,get b(){Object.defineProperty(this,'c',{enumerable:false});return 1},c:3};return Object.values(o)}",
    "{const o={get a(){Object.defineProperty(this,'b',{enumerable:false});return 1},b:2};return Object.entries(o)}",
    "{const o={get a(){this.b=9;return 1},b:2};return Object.entries(o)}",
    "{const o={get a(){Object.defineProperty(this,'b',{get(){return 7},enumerable:true});return 1},b:2};return Object.values(o)}",
    "{const o={get a(){Object.freeze(this);return 1},b:2};return [Object.entries(o),Object.isFrozen(o)]}",
    "{const o={get a(){delete this.a;return 1}};return [Object.entries(o),Reflect.ownKeys(o)]}",
    "{const o={get a(){throw new RangeError('ga')},b:1};return Object.entries(o)}",
    "{const o={a:1,get b(){throw new RangeError('gb')}};return Object.values(o)}",
    "{const o={a:1,get b(){throw new RangeError('gb')}};return Object.keys(o)}",
    "{const o={get a(){W.push('a');return 1},get b(){W.push('b');return 2}};Object.entries(o);Object.values(o);Object.keys(o);return W}",
    "{const o={get a(){W.push('a');return 1},get b(){W.push('b');return 2}};JSON.stringify(o);return W}",
    "{const o={get a(){W.push('a');return 1},get b(){W.push('b');return 2}};({...o});Object.assign({},o);return W}",
    "{const o={get a(){W.push('a');return 1},get b(){W.push('b');return 2}};Object.getOwnPropertyDescriptors(o);Object.getOwnPropertyNames(o);Reflect.ownKeys(o);return W}",
    "{const o={a:1,get b(){delete this.a;return 2}};const t=Object.assign({},o);return [t,Reflect.ownKeys(o)]}",
    "{const o={a:1,get b(){this.z=1;return 2}};const t=Object.assign({},o);return [t,Reflect.ownKeys(o)]}",
    "{const o={get a(){delete this.b;return 1},b:2};const t=Object.assign({},o);return t}",
    "{const o={get a(){delete this.b;return 1},b:2};return {...o}}",
    "{const o={get a(){delete this.b;return 1},b:2};return JSON.stringify(o)}",
    "{const t={set a(v){W.push('set');delete src.b}};const src={a:1,b:2};Object.assign(t,src);return [t,W,src]}",
    "{const t={};Object.defineProperty(t,'a',{value:0});Object.assign(t,{a:1})}",
    "{const t={};Object.defineProperty(t,'a',{value:0});Object.assign(t,{b:2,a:1})}",
    "{const t=Object.freeze({});Object.assign(t,{a:1})}",
    "{const t=Object.freeze({a:1});Object.assign(t,{})  ;return t}",
    "{const t=Object.preventExtensions({});Object.assign(t,{a:1})}",
    "{const t={get a(){return 1}};Object.assign(t,{a:2})}",
    "{const t={};Object.assign(t,'ab',[9],null,undefined,1,true);return t}",
    "{const t={};Object.assign(t,{a:1},{get a(){return 2}});return t}",
    "{const t={};Object.assign(t,Object.defineProperty({},'a',{value:1,enumerable:false}));return t}",
    "{const s=Symbol('s');const t=Object.assign({},{[s]:1,a:2});return [Reflect.ownKeys(t),t[s]]}",
    "{const s=Symbol('s');const src={};Object.defineProperty(src,s,{value:1,enumerable:false});return Reflect.ownKeys(Object.assign({},src))}",
    "Object.assign(null,{})",
    "Object.assign(undefined)",
    "Object.assign(1,{a:1}).a",
    "Object.assign('x',{a:1})",
    "{const r=Object.assign(true,{a:1});return [typeof r,r.a]}",
    "Object.assign({},new Proxy({a:1,b:2},{ownKeys(t){W.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){W.push('gopd:'+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){W.push('get:'+k);return t[k]}}))",
    "{Object.assign({},new Proxy({a:1,b:2},{ownKeys(t){W.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){W.push('gopd:'+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){W.push('get:'+k);return t[k]}}));return W}",
    "{Object.entries(new Proxy({a:1,b:2},{ownKeys(t){W.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){W.push('gopd:'+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){W.push('get:'+k);return t[k]}}));return W}",
    "{Object.values(new Proxy({a:1,b:2},{ownKeys(t){W.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){W.push('gopd:'+k);return undefined},get(t,k){W.push('get:'+k);return t[k]}}));return W}",
    "{const p=new Proxy({a:1},{ownKeys(){return ['a','a']}});return Object.keys(p)}",
    "{const p=new Proxy({a:1},{ownKeys(){return [1]}});return Object.keys(p)}",
    "{const p=new Proxy({a:1},{ownKeys(){return []}});return Object.keys(p)}",
    "{const p=new Proxy(Object.freeze({a:1}),{ownKeys(){return []}});return Object.keys(p)}",
    "{const p=new Proxy({a:1},{getOwnPropertyDescriptor(){return {value:1,configurable:false}}});return Object.getOwnPropertyDescriptor(p,'a')}",
    "{const p=new Proxy({a:1},{getOwnPropertyDescriptor(){return 1}});return Object.getOwnPropertyDescriptor(p,'a')}",
    "{const p=new Proxy({},{getOwnPropertyDescriptor(){return {value:1,configurable:true}}});return Object.getOwnPropertyDescriptor(p,'a')}",
    "{const p=new Proxy(Object.preventExtensions({}),{getOwnPropertyDescriptor(){return {value:1,configurable:true}}});return Object.getOwnPropertyDescriptor(p,'a')}",
    "{const p=new Proxy({},{defineProperty(){return false}});Object.defineProperty(p,'a',{value:1})}",
    "{const p=new Proxy({},{defineProperty(){return false}});return Reflect.defineProperty(p,'a',{value:1})}",
    "{const p=new Proxy({},{defineProperty(){return true}});Object.defineProperty(p,'a',{value:1,configurable:false})}",
    "{const p=new Proxy(Object.preventExtensions({}),{defineProperty(){return true}});Object.defineProperty(p,'a',{value:1})}",
    "{const p=new Proxy({},{defineProperty(t,k,d){W.push(Object.keys(d).join());return true}});Object.defineProperty(p,'a',{value:1,get:undefined})}",
    "{const p=new Proxy({},{defineProperty(t,k,d){W.push(Object.keys(d).join());return Reflect.defineProperty(t,k,d)}});Object.defineProperty(p,'a',{enumerable:1,value:1,writable:0,configurable:'x'});return [W,Object.getOwnPropertyDescriptor(p,'a')]}",
    "{const p=new Proxy({},{set(){return false}});p.a=1}",
    "{const p=new Proxy({},{set(){return false}});return Reflect.set(p,'a',1)}",
    "{const p=new Proxy(Object.freeze({a:1}),{set(){return true}});p.a=2}",
    "{const p=new Proxy({},{deleteProperty(){return false}});delete p.a}",
    "{const p=new Proxy(Object.freeze({a:1}),{deleteProperty(){return true}});delete p.a}",
    "{const p=new Proxy({},{get(){return 1}});Object.defineProperty(p,'x',{value:2,configurable:false,writable:false});return p.x}",
    "{const t={};Object.defineProperty(t,'x',{value:2});const p=new Proxy(t,{get(){return 1}});return p.x}",
    "{const t={};Object.defineProperty(t,'x',{get:undefined});const p=new Proxy(t,{get(){return 1}});return p.x}",
    "{const t={};Object.defineProperty(t,'x',{set:s});const p=new Proxy(t,{get(){return undefined}});return p.x}",
    "{const t={};Object.defineProperty(t,'x',{value:2});const p=new Proxy(t,{has(){return false}});return 'x' in p}",
    "{const t=Object.preventExtensions({x:1});const p=new Proxy(t,{has(){return false}});return 'x' in p}",
    "{const o={a:1,b:2,c:3};return Object.fromEntries(Object.entries(o).map(([k,v])=>[k,v*2]))}",
    "Object.fromEntries([['a',1],['a',2]])",
    "Object.fromEntries([[1,2],[Symbol.iterator,3],[{},4],[null,5],[undefined,6],[[1,2],7]])",
    "Object.fromEntries([['a']])",
    "Object.fromEntries(['ab'])",
    "Object.fromEntries([1])",
    "Object.fromEntries([null])",
    "Object.fromEntries([[]])",
    "Object.fromEntries(null)",
    "Object.fromEntries(undefined)",
    "Object.fromEntries()",
    "Object.fromEntries(1)",
    "Object.fromEntries({})",
    "Object.fromEntries('')",
    "Object.fromEntries(new Map([[1,2],['x',{}]]))",
    "Object.fromEntries({[Symbol.iterator](){let i=0;return {next(){return i++<2?{value:['k'+i,i],done:false}:{done:true}},return(){W.push('ret');return {}}}}})",
    "{try{Object.fromEntries({[Symbol.iterator](){return {next(){return {value:1,done:false}},return(){W.push('ret');return {}}}}})}catch(e){W.push(e.message)}return W}",
    "{const o=Object.fromEntries([['__proto__',1]]);return [Reflect.ownKeys(o),Object.getPrototypeOf(o)===Object.prototype]}",
    "{const o=Object.fromEntries([['__proto__',{a:1}]]);return [Reflect.ownKeys(o),o.a]}",
    "{const o=Object.fromEntries([[{toString(){return 'ts'}},1]]);return o}",
    "{const o=Object.fromEntries([[{toString(){throw new RangeError('ts')}},1]]);return o}",
    "{const o=Object.fromEntries([{get 0(){W.push('0');return 'a'},get 1(){W.push('1');return 'b'}}]);return [o,W]}",
    "Object.keys(null)",
    "Object.keys(undefined)",
    "Object.keys(1)",
    "Object.keys('ab')",
    "Object.entries('ab')",
    "Object.values('ab')",
    "Object.entries(null)",
    "Object.values(undefined)",
    "Object.entries([1,,3])",
    "Object.values([1,,3])",
    "Object.entries(function(){})",
    "Object.entries(new Uint8Array([1,2]))",
    "Object.getOwnPropertyNames('ab')",
    "Object.getOwnPropertyNames(function f(a){})",
    "Object.getOwnPropertyNames(()=>1)",
    "Object.getOwnPropertyNames(class{})",
    "Object.getOwnPropertyNames(class{static s=1;static m(){}})",
    "Object.getOwnPropertyNames([1,2])",
    "Object.getOwnPropertyNames((function(){return arguments})(1))",
    "Object.getOwnPropertyNames(null)",
    "Object.getOwnPropertySymbols(1)",
    "Object.getOwnPropertySymbols(null)",
    "Object.getOwnPropertyDescriptors(null)",
    "Object.getOwnPropertyDescriptors(1)",
    "Object.getOwnPropertyDescriptors('ab')",
    "Object.getOwnPropertyDescriptors([5])",
    "Object.getOwnPropertyDescriptors(function f(a,b){})",
    "Object.getOwnPropertyDescriptors((function(){return arguments})(1))",
    "Object.getOwnPropertyDescriptors((function(){'use strict';return arguments})(1))",
    "Object.getOwnPropertyDescriptors(class{static get x(){return 1}})",
    "Object.getOwnPropertyDescriptors({get a(){return 1},set a(v){},[Symbol.iterator]:1})",
    "Object.getOwnPropertyDescriptors(new Proxy({a:1},{}))",
    "Object.getOwnPropertyDescriptors(new Proxy({a:1},{ownKeys(){return ['a','b']},getOwnPropertyDescriptor(t,k){return k==='a'?Reflect.getOwnPropertyDescriptor(t,k):undefined}}))",
    "Object.getOwnPropertyDescriptor(null,'a')",
    "Object.getOwnPropertyDescriptor(undefined,'a')",
    "Object.getOwnPropertyDescriptor(1,'a')",
    "Object.getOwnPropertyDescriptor('ab','1')",
    "Object.getOwnPropertyDescriptor('ab','length')",
    "Object.getOwnPropertyDescriptor('ab',1)",
    "Object.getOwnPropertyDescriptor('ab',2)",
    "Object.getOwnPropertyDescriptor({a:1})",
    "Object.getOwnPropertyDescriptor({undefined:1})",
    "Object.getOwnPropertyDescriptor({a:1},{toString(){return 'a'}})",
    "Object.getOwnPropertyDescriptor({a:1},{toString(){throw new RangeError('ts')}})",
    "Object.getOwnPropertyDescriptor({1:1},1)",
    "Object.getOwnPropertyDescriptor({1:1},1.0)",
    "Object.getOwnPropertyDescriptor({'-0':1},-0)",
    "Object.getOwnPropertyDescriptor([],'length')",
    "Object.getOwnPropertyDescriptor(function(){},'prototype')",
    "Object.getOwnPropertyDescriptor(()=>1,'prototype')",
    "Object.getOwnPropertyDescriptor(function(){},'name')",
    "Object.getOwnPropertyDescriptor(function(){},'length')",
    "Object.getOwnPropertyDescriptor(function(){},'caller')",
    "Object.getOwnPropertyDescriptor(function(){'use strict'},'caller')",
    "Object.getOwnPropertyDescriptor(function(){},'arguments')",
    "Object.getOwnPropertyDescriptor(class{},'prototype')",
    "Object.getOwnPropertyDescriptor((function(){return arguments})(1),'callee')",
    "Object.getOwnPropertyDescriptor((function(){'use strict';return arguments})(1),'callee')",
    "Object.getOwnPropertyDescriptor((function(){return arguments})(1),'length')",
    "Object.getOwnPropertyDescriptor((function(){return arguments})(1),Symbol.iterator)",
    "Object.getOwnPropertyDescriptor(Math,'PI')",
    "Object.getOwnPropertyDescriptor(Math,Symbol.toStringTag)",
    "Object.getOwnPropertyDescriptor(globalThis,'undefined')",
    "Object.getOwnPropertyDescriptor(globalThis,'NaN')",
    "Object.getOwnPropertyDescriptor(Number,'MAX_VALUE')",
    "Object.getOwnPropertyDescriptor(Symbol,'iterator')",
    "Object.getOwnPropertyDescriptor(Object.prototype,'constructor')",
    "Object.getOwnPropertyDescriptor(Object,'prototype')",
    "Object.getOwnPropertyDescriptor(Map.prototype,'size')",
    "Object.getOwnPropertyDescriptor(RegExp.prototype,'flags')",
    "Object.getOwnPropertyDescriptor(RegExp.prototype,'global')",
    "Object.getOwnPropertyDescriptor(/x/g,'lastIndex')",
    "Object.getOwnPropertyDescriptor(new Error('m'),'message')",
    "Object.getOwnPropertyDescriptor(new Error('m'),'stack')===undefined",
    "Object.getOwnPropertyDescriptor(Error.prototype,'name')",
    "Object.getOwnPropertyDescriptor(Symbol.prototype,'description')",
    "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength')",
    "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array).prototype,'length')",
    "Object.getOwnPropertyDescriptor(Array.prototype,Symbol.unscopables)",
    "Object.getOwnPropertyDescriptor(Array.prototype,'length')",
    "Object.getOwnPropertyDescriptor(Function.prototype,'caller')",
    "Object.getOwnPropertyDescriptor(Function.prototype,'arguments')",
    "Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance)",
    "Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive)",
    "Object.getOwnPropertyDescriptor(Promise,Symbol.species)",
    "Object.getOwnPropertyDescriptor(Array,Symbol.species)",
    "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(function*(){}),Symbol.toStringTag)",
    "Object.getOwnPropertyDescriptor(Object.getPrototypeOf([][Symbol.iterator]()),Symbol.toStringTag)",
  ];
  for (const st of es) prog("", st);
}

// ---- 10. Ordem de chaves.
{
  const keySets = [
    ["'2'", "'1'", "'b'", "'a'"],
    ["'b'", "'2'", "'a'", "'1'", "'0'"],
    ["'01'", "'1'", "'-1'", "'1.5'", "'1e3'", "'b'", "'10'", "'9'"],
    ["'4294967295'", "'4294967294'", "'4294967296'", "'0'", "'a'"],
    ["'9007199254740993'", "'9007199254740991'", "'2'", "'x'"],
    ["Symbol.iterator", "'b'", "'1'", "Symbol.toPrimitive", "'a'", "'0'"],
    ["'-0'", "'0'", "'+1'", "' 1'", "'1 '", "'0x10'", "'16'"],
    ["'length'", "'1'", "'name'", "'0'"],
    ["'b'", "'a'", "'c'", "'a'", "'b'"],
    ["'z'", "'y'", "'x'", "'3'", "'2'", "'1'"],
  ];
  const setters = (ks, how) =>
    ks
      .map((k, i) => {
        if (how === "assign") return `o[${k}]=${i}`;
        if (how === "define") return `Object.defineProperty(o,${k},{value:${i},enumerable:true,configurable:true,writable:true})`;
        if (how === "getter") return `Object.defineProperty(o,${k},{get(){return ${i}},enumerable:true,configurable:true})`;
        return `Reflect.set(o,${k},${i})`;
      })
      .join(";");
  for (const ks of keySets)
    for (const how of ["assign", "define", "getter", "reflect"]) {
      const body = setters(ks, how);
      prog(
        `const o={};${body}`,
        "Object.keys(o)",
        "Object.getOwnPropertyNames(o)",
        "Reflect.ownKeys(o)",
        "Object.entries(o).map(e=>e[0])",
        "{const r=[];for(const k in o)r.push(k);return r}",
        "JSON.stringify(Object.assign({},o))",
        "Object.keys({...o})",
        "Object.keys(Object.getOwnPropertyDescriptors(o))",
        "{const p=Object.create(o);p.q=1;const r=[];for(const k in p)r.push(k);return r}",
        "Reflect.ownKeys(Object.fromEntries(Object.entries(o)))",
        "Reflect.ownKeys(structuredClone(Object.fromEntries(Object.getOwnPropertyNames(o).map(k=>[k,1]))))",
      );
    }
  prog("", "{const o={b:1,a:2,1:3,0:4};delete o.b;o.b=5;return Reflect.ownKeys(o)}", "{const o={b:1,a:2};delete o.a;delete o.b;o.a=1;o.b=2;return Reflect.ownKeys(o)}", "{const o={b:1,a:2};o.b=3;return Reflect.ownKeys(o)}", "{const o={b:1,a:2};Object.defineProperty(o,'b',{get:g,configurable:true});return Reflect.ownKeys(o)}", "{const o={b:1,a:2};Object.defineProperty(o,'b',{value:5,enumerable:false});return [Reflect.ownKeys(o),Object.keys(o)]}", "{const s1=Symbol('1'),s2=Symbol('2');const o={[s2]:1,x:1,[s1]:2};return Reflect.ownKeys(o)}", "{const s1=Symbol('1');const o={[s1]:1};delete o[s1];o[s1]=2;return Reflect.ownKeys(o)}");
  prog("", "{const o={a:1,b:2,c:3};const r=[];for(const k in o){r.push(k);delete o.b;o.d=4}return r}", "{const o={a:1,b:2,c:3};const r=[];for(const k in o){r.push(k);if(k==='a'){delete o.c}}return r}", "{const o={a:1,b:2};const r=[];for(const k in o){r.push(k);delete o.b;o.b=3}return r}", "{const p={x:1,y:2};const o=Object.create(p);o.x=3;o.z=4;const r=[];for(const k in o)r.push(k);return r}", "{const p={x:1};const o=Object.create(p);Object.defineProperty(o,'x',{value:3,enumerable:false});const r=[];for(const k in o)r.push(k);return r}", "{const p={x:1};const o=Object.create(p);const r=[];for(const k in o){r.push(k);delete p.x}return r}", "{const o={a:1,b:2};const r=[];for(const k in o){r.push(k);Object.defineProperty(o,'b',{enumerable:false})}return r}", "{const r=[];for(const k in 'ab')r.push(k);return r}", "{const r=[];for(const k in [1,,3])r.push(k);return r}", "{const r=[];for(const k in null)r.push(k);for(const k in undefined)r.push(k);return r}", "{const a=[1,2];a.x=1;const r=[];for(const k in a)r.push(k);return r}", "{const r=[];for(const k in new Proxy({a:1,b:2},{ownKeys(){W.push('ok');return ['b','a']}}))r.push(k);return [r,W]}");
}

// ---- 11. delete e atribuição em strict.
{
  const dels = [
    "delete Math.PI", "delete Object.prototype", "delete Array.prototype.length", "delete [].length", "delete (function(){}).prototype",
    "delete (function(){}).name", "delete (function(){}).length", "delete (()=>1).name", "delete class{}.prototype", "delete Number.MAX_VALUE", "delete globalThis.undefined", "delete globalThis.NaN",
    "delete Symbol.iterator", "delete new String('ab')[0]", "delete new String('ab').length", "delete new String('ab')[2]", "delete 'ab'[0]", "delete 'ab'.length", "delete 'ab'[5]", "delete (1).x", "delete null.x",
    "delete undefined.x", "delete [1,2][0]", "delete [1,2][5]", "delete Object.freeze([1])[0]", "delete Object.seal({a:1}).a", "delete Object.freeze({a:1}).a", "delete Object.freeze({a:1}).b", "delete Object.preventExtensions({a:1}).a",
    "delete new Uint8Array(2)[0]", "delete new Uint8Array(2)[5]", "delete new Uint8Array(2).length", "delete new Uint8Array(2)['-0']", "delete (function(){return arguments})(1)[0]", "delete (function(){return arguments})(1).length",
    "delete (function(){'use strict';return arguments})(1).callee", "delete (function(){return arguments})(1).callee", "delete Object.prototype.__proto__", "delete Function.prototype.caller", "delete (function(){'use strict'}).caller",
    "delete Math[Symbol.toStringTag]", "delete JSON[Symbol.toStringTag]", "delete Array.prototype[Symbol.unscopables]", "delete Map.prototype.size", "delete /x/.lastIndex", "delete new Error('m').message", "delete new Error('m').stack",
    "delete Reflect.ownKeys", "delete new Proxy({},{deleteProperty(){return true}}).a", "delete new Proxy({},{deleteProperty(){return false}}).a", "delete new Proxy({a:1},{}).a", "delete new Proxy(Object.freeze({a:1}),{}).a",
    "Reflect.deleteProperty(Math,'PI')", "Reflect.deleteProperty(Object.freeze({a:1}),'a')", "Reflect.deleteProperty({a:1},'a')", "Reflect.deleteProperty([],'length')", "Reflect.deleteProperty(1,'a')",
    "Reflect.deleteProperty(Object.freeze({a:1}),'b')", "Reflect.deleteProperty(new Uint8Array(1),0)", "Reflect.deleteProperty(new Uint8Array(1),5)", "Reflect.deleteProperty(new String('ab'),0)",
    "{const o={};Object.defineProperty(o,'x',{value:1});delete o.x}", "{const o={};Object.defineProperty(o,'x',{get:g});delete o.x}", "{const o={};Object.defineProperty(o,'x',{get:g,configurable:true});return [delete o.x,'x' in o]}",
    "{const o={};Object.defineProperty(o,Symbol.iterator,{value:1});delete o[Symbol.iterator]}", "{const o={};Object.defineProperty(o,1,{value:1});delete o[1]}", "{const o={};Object.defineProperty(o,'1',{value:1});delete o[1.0]}",
    "{const o={};Object.defineProperty(o,'x',{value:1});delete o['x']}", "{const o={};Object.defineProperty(o,'x',{value:1});const k='x';delete o[k]}", "{const o={};Object.defineProperty(o,'x',{value:1});return Reflect.deleteProperty(o,'x')}",
    "{const o={a:1};return [delete o.a,delete o.a,delete o.zz]}", "{const o={};return delete o[{toString(){return 'k'}}]}", "{const o={};delete o[{toString(){throw new RangeError('ts')}}]}",
    "{const a=[1,2,3];Object.defineProperty(a,1,{configurable:false});delete a[1]}", "{const a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return Reflect.deleteProperty(a,1)}",
    "{const f=function(){};Object.defineProperty(f,'x',{value:1});delete f.x}", "{class C{static x=1};return [delete C.x,C.x]}", "{class C{static get x(){return 1}};return [delete C.x,C.x]}", "{class C{m(){}};return [delete C.prototype.m,typeof new C().m]}", "{class C{};delete C.prototype}",
    "{const o={x:1,get y(){return 1}};return [delete o.x,delete o.y,Reflect.ownKeys(o)]}",
  ];
  for (const d of dels) prog("", d);
  const sets = [
    "{const o={get x(){return 1}};o.x=2}", "{const o={get x(){return 1}};o['x']=2}", "{const o={get x(){return 1}};o.x++}", "{const o={get x(){return 1}};o.x+=1}", "{const o={get x(){return 1}};o.x??=1;return o.x}", "{const o={get x(){return null}};o.x??=1}", "{const o={get x(){return 1}};o.x||=2;return o.x}", "{const o={get x(){return 0}};o.x||=2}", "{const o={get x(){return 1}};o.x&&=2}",
    "{const o={get 1(){return 1}};o[1]=2}", "{const o={get [Symbol.iterator](){return 1}};o[Symbol.iterator]=2}", "{const o={get x(){return 1}};({x:o.x}={x:2})}", "{const o={get x(){return 1}};[o.x]=[2]}", "{const o={get x(){return 1}};for(o.x of [1]);}", "{const o={get x(){return 1}};for(o.x in {a:1});}",
    "{const o=Object.freeze({x:1});o.x=2}", "{const o=Object.freeze({x:1});o.y=2}", "{const o=Object.freeze({x:1});o.x++}", "{const o=Object.freeze([1]);o[0]=2}", "{const o=Object.freeze([1]);o[1]=2}", "{const o=Object.freeze([1]);o.length=0}", "{const o=Object.freeze([1]);o.push(2)}",
    "{const o=Object.seal({x:1});o.x=2;return o.x}", "{const o=Object.seal({x:1});o.y=2}", "{const o=Object.preventExtensions({x:1});o.y=2}", "{const o=Object.preventExtensions({x:1});o[1]=2}", "{const o=Object.preventExtensions({x:1});o[Symbol.iterator]=2}", "{const o=Object.preventExtensions({});o.__proto__=null;return 1}",
    "{const o={};Object.defineProperty(o,'x',{value:1});o.x=2}", "{const o={};Object.defineProperty(o,'x',{value:1,writable:true});o.x=2;return o.x}", "{const o={};Object.defineProperty(o,'x',{value:1});o.x=1}", "{const o={};Object.defineProperty(o,'x',{value:1});return Reflect.set(o,'x',2)}", "{const o={};Object.defineProperty(o,'x',{set:undefined});o.x=2}", "{const o={};Object.defineProperty(o,'x',{get:undefined,set:undefined});o.x=2}",
    "{const o=Object.create(Object.freeze({x:1}));o.x=2}", "{const o=Object.create(Object.freeze({x:1}));o.y=2;return o.y}", "{const o=Object.create({get x(){return 1}});o.x=2}", "{const o=Object.create({set x(v){W.push(v)}});o.x=2;return [W,Reflect.ownKeys(o)]}", "{const o=Object.create(Object.create({get x(){return 1}}));o.x=2}",
    "{const p={};Object.defineProperty(p,'x',{value:1,writable:false});const o=Object.create(p);o.x=2}", "{const p={};Object.defineProperty(p,'x',{value:1,writable:true});const o=Object.create(p);o.x=2;return [Reflect.ownKeys(o),p.x]}", "{const p={};Object.defineProperty(p,'x',{value:1,writable:false});const o=Object.create(p);return Reflect.set(o,'x',2)}",
    "'abc'.x=1", "'abc'[0]='z'", "'abc'[5]='z'", "'abc'.length=1", "(1).x=1", "true.x=1", "Symbol().x=1", "(1n).x=1", "null.x=1", "undefined.x=1", "null[0]=1", "undefined[Symbol()]=1", "(void 0).x++", "{const s=Symbol();s.x=1}", "{const s='abc';s.length=1}", "{const n=1;n.valueOf=1}",
    "Math.PI=3", "Math.max=1;", "Object.prototype.__proto__=1", "undefined=1", "globalThis.undefined=1", "globalThis.NaN=1", "globalThis.Infinity=1", "NaN=1", "Infinity=1", "Number.MAX_VALUE=1", "Symbol.iterator=1", "(function(){}).name='x'", "(function(){}).length=2", "(()=>1).prototype=1", "(function(){}).prototype=1;", "(class{}).prototype=1", "(class{}).name='x'", "(class{static name='y'}).name='x'",
    "[].length='x'", "[].length=-1", "[].length=2**32", "{const a=[];a.length=1.5}", "new String('ab').length=1", "new String('ab')[0]='z'", "new String('ab')[2]='z'", "new Uint8Array(2).length=1", "new Uint8Array(2)[5]=1", "new Uint8Array(2)['x']=1", "(function(){return arguments})(1).length=5", "(function(){'use strict';return arguments})(1).callee=1", "(function(){'use strict';return arguments})(1).callee",
    "(function(){}).caller", "(function(){'use strict'}).caller", "(function(){'use strict'}).arguments", "(function(){}).arguments", "(()=>1).caller", "(class{}).caller", "(function*(){}).caller", "(async function(){}).arguments", "Object.getPrototypeOf(function(){}).caller", "Function.prototype.caller=1", "Function.prototype.arguments=1",
    "Reflect.set(Math,'PI',3)", "Reflect.set(1,'x',1)", "Reflect.set({},'x',1,1)", "Reflect.set({},'x',1,null)", "Reflect.set({set x(v){W.push(this)}},'x',1,5)||W", "Reflect.set(new Proxy({},{set(){return 1}}),'x',1)", "Reflect.set(new Proxy({},{set(){return 0}}),'x',1)", "Reflect.set(Object.freeze([1]),0,2)", "Reflect.set(Object.freeze([1]),'length',0)", "Reflect.set(Object.freeze([1]),'zz',0)",
    "{const o={x:1};Object.defineProperty(o,'x',{writable:false});o.x=2}", "{const o={x:1};Object.defineProperty(o,'x',{writable:false});o.x=1}", "{const o={x:1};Object.defineProperty(o,'x',{writable:false});o['x']++}", "{const o={x:1};Object.defineProperty(o,'x',{writable:false});({x:o.x}={x:2})}", "{const o={x:1};Object.defineProperty(o,'x',{writable:false});with0=1}",
  ];
  for (const st of sets) prog("", st);
}

// ---- 12. Reflect.set com receiver diferente.
{
  const targets = {
    protoSetter: "const t=Object.create({set k(v){W.push('set:'+(this===r?'r':this===t?'t':typeof this))}})",
    protoGetterOnly: "const t=Object.create({get k(){return 1}})",
    protoReadonly: "const t=Object.create(Object.defineProperty({},'k',{value:1,writable:false}))",
    protoWritable: "const t=Object.create({k:1})",
    ownData: "const t={k:1}",
    ownReadonly: "const t=Object.defineProperty({},'k',{value:1,writable:false})",
    ownSetter: "const t={set k(v){W.push('own:'+(this===r?'r':this===t?'t':typeof this))}}",
    ownGetterOnly: "const t={get k(){return 1}}",
    absent: "const t={}",
    arrayTarget: "const t=[1,2]",
  };
  const receivers = {
    same: "const r=t",
    plain: "const r={}",
    ownData: "const r={k:0}",
    ownReadonly: "const r=Object.defineProperty({},'k',{value:0,writable:false})",
    ownAccessor: "const r={get k(){return 0},set k(v){W.push('racc')}}",
    ownNonConfigWritable: "const r=Object.defineProperty({},'k',{value:0,writable:true,configurable:false,enumerable:false})",
    frozen: "const r=Object.freeze({})",
    frozenWithK: "const r=Object.freeze({k:0})",
    nonExtensible: "const r=Object.preventExtensions({})",
    prim: "const r=1",
    str: "const r='abc'",
    nullish: "const r=null",
    arr: "const r=[]",
    fn: "const r=function(){}",
    proxy: "const r=new Proxy({},{defineProperty(t,k,d){W.push('dp:'+Object.keys(d).join());return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){W.push('gopd');return Reflect.getOwnPropertyDescriptor(t,k)}})",
  };
  for (const [tn, tc] of Object.entries(targets))
    for (const [rn, rc] of Object.entries(receivers)) {
      // `r` precisa existir antes do alvo para o setter comparar `this`; declarado por função hoisting-free: r antes de t quando não depende de t.
      const rDependsOnT = rc === "const r=t";
      const pre = rDependsOnT ? `${tc};${rc}` : `${rc};${tc}`;
      prog(
        pre,
        "Reflect.set(t,'k',5,r)",
        "[typeof r==='object'&&r!==null||typeof r==='function'?" + gopd + "(r,'k'):'-',W]",
        "(()=>{const x=Object.getOwnPropertyDescriptor(t,'k');return x})()",
      );
    }
}

// ---- 13. setPrototypeOf e Object.create.
{
  const sp = [
    "Object.setPrototypeOf({},null)", "Object.setPrototypeOf({},{})", "Object.setPrototypeOf({},1)", "Object.setPrototypeOf({},'x')", "Object.setPrototypeOf({},undefined)", "Object.setPrototypeOf({})", "Object.setPrototypeOf({},true)", "Object.setPrototypeOf({},Symbol())", "Object.setPrototypeOf({},1n)",
    "Object.setPrototypeOf({},function(){})", "Object.setPrototypeOf({},[])", "Object.setPrototypeOf(null,{})", "Object.setPrototypeOf(undefined,{})", "Object.setPrototypeOf(null,null)", "Object.setPrototypeOf(1,null)", "Object.setPrototypeOf(1,{})", "Object.setPrototypeOf('x',{})", "Object.setPrototypeOf(Symbol(),{})",
    "Object.setPrototypeOf(1,1)", "Object.setPrototypeOf(1)", "Object.setPrototypeOf()", "Object.setPrototypeOf(Object.freeze({}),null)", "Object.setPrototypeOf(Object.freeze({}),Object.prototype)", "Object.setPrototypeOf(Object.preventExtensions({}),null)",
    "Object.setPrototypeOf(Object.preventExtensions({}),Object.prototype)", "Object.setPrototypeOf(Object.preventExtensions({}),{})", "Object.setPrototypeOf(Object.seal({}),{})", "Object.setPrototypeOf(Object.prototype,null)", "Object.setPrototypeOf(Object.prototype,{})", "Object.setPrototypeOf(Object.prototype,Object.prototype)",
    "{const o={};return Object.setPrototypeOf(o,o)}", "{const a={},b=Object.create(a);return Object.setPrototypeOf(a,b)}", "{const a={},b=Object.create(a),c=Object.create(b);return Object.setPrototypeOf(a,c)}", "{const a={};return Object.setPrototypeOf(a,Object.create(a))}",
    "{const a=[];return Object.setPrototypeOf(a,Array.prototype)===a}", "{const f=function(){};return Object.setPrototypeOf(f,null).call}", "{const o=Object.setPrototypeOf({},null);return [String(Object.getPrototypeOf(o)),typeof o.toString]}", "{const o=Object.setPrototypeOf({},null);return `${o}`}", "{const o=Object.setPrototypeOf({},null);return o+''}",
    "{const o=Object.setPrototypeOf({a:1},null);return Object.keys(o)}", "{const o=Object.setPrototypeOf({a:1},null);return JSON.stringify(o)}", "{const o=Object.setPrototypeOf({a:1},null);return o instanceof Object}", "{const o=Object.setPrototypeOf({},{get x(){return this}});return o.x===o}",
    "{const p=new Proxy({},{setPrototypeOf(){return false}});Object.setPrototypeOf(p,null)}", "{const p=new Proxy({},{setPrototypeOf(){return true}});return Object.setPrototypeOf(p,null)===p}", "{const p=new Proxy({},{setPrototypeOf(){return false}});return Reflect.setPrototypeOf(p,null)}", "{const p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(){return true}});Object.setPrototypeOf(p,null)}",
    "{const p=new Proxy({},{getPrototypeOf(){return 1}});return Object.getPrototypeOf(p)}", "{const p=new Proxy(Object.preventExtensions({}),{getPrototypeOf(){return null}});return Object.getPrototypeOf(p)}", "{const p=new Proxy({},{getPrototypeOf(){return null}});return Object.getPrototypeOf(p)}", "{const p=new Proxy({},{getPrototypeOf(){return Array.prototype}});return [p instanceof Array,Array.isArray(p),Object.prototype.isPrototypeOf.call(Array.prototype,p)]}",
    "Reflect.setPrototypeOf({},1)", "Reflect.setPrototypeOf(1,{})", "Reflect.setPrototypeOf({})", "Reflect.setPrototypeOf({},null)", "Reflect.getPrototypeOf(1)", "Reflect.getPrototypeOf(null)", "Reflect.getPrototypeOf()", "Reflect.getPrototypeOf('x')===String.prototype",
    "Object.getPrototypeOf(null)", "Object.getPrototypeOf(undefined)", "Object.getPrototypeOf(1)===Number.prototype", "Object.getPrototypeOf('x')===String.prototype", "Object.getPrototypeOf(Symbol())===Symbol.prototype", "Object.getPrototypeOf(1n)===BigInt.prototype", "Object.getPrototypeOf(Object.prototype)", "Object.getPrototypeOf(Object.create(null))", "Object.getPrototypeOf(function(){})===Function.prototype", "Object.getPrototypeOf(Function.prototype)===Object.prototype", "Object.getPrototypeOf(class A extends Array{})===Array", "Object.getPrototypeOf(class{})===Function.prototype", "Object.getPrototypeOf(class extends null{})===Function.prototype", "Object.getPrototypeOf((class extends null{}).prototype)",
    "Object.getPrototypeOf(async function(){})===Function.prototype", "Object.getPrototypeOf(function*(){})===Function.prototype", "Object.getPrototypeOf(()=>1)===Function.prototype", "Object.getPrototypeOf(Math)===Object.prototype", "Object.getPrototypeOf(Uint8Array).name", "Object.getPrototypeOf(Error)===Function.prototype", "Object.getPrototypeOf(TypeError)===Error", "Object.getPrototypeOf(globalThis)===Object.prototype",
    "Object.create()", "Object.create(undefined)", "Object.create(1)", "Object.create('x')", "Object.create(true)", "Object.create(Symbol())", "Object.create(null)", "Object.create({})", "Object.create(function(){})", "Object.create(null,undefined)", "Object.create(null,null)", "Object.create({},1)", "Object.create({},'ab')", "Object.create({},true)",
    "Object.create({},{a:1})", "Object.create({},{a:null})", "Object.create({},{a:undefined})", "Object.create({},{a:{value:1}})", "Object.create({},{a:{get:1}})", "Object.create({},{a:{get:g,value:1}})", "Object.create({},{a:{value:1,enumerable:true},b:{value:2}})", "Object.create({},{[Symbol.iterator]:{value:1}})", "Object.create({},{a:{value:1,enumerable:true}}).a", "Object.create({x:1},{y:{value:2,enumerable:true}})",
    "Object.create({},new Proxy({a:{value:1}},{}))", "Object.create({},new Proxy({a:{value:1}},{ownKeys(){return ['a','b']},getOwnPropertyDescriptor(t,k){return k==='a'?{value:t.a,enumerable:true,configurable:true}:undefined}}))", "Object.create({},{get a(){return {value:1}}})", "Object.create({},Object.defineProperty({},'a',{value:{value:1}}))", "Object.create({},Object.create({a:{value:1}}))",
    "Object.defineProperties({},{a:{value:1},b:{get:1}})", "{const o={};try{Object.defineProperties(o,{a:{value:1},b:{get:1}})}catch(e){}return Reflect.ownKeys(o)}", "{const o={};try{Object.defineProperties(o,{a:{value:1},b:1,c:{value:3}})}catch(e){}return Reflect.ownKeys(o)}", "{const o=Object.freeze({});try{Object.defineProperties(o,{a:{value:1}})}catch(e){return e.message}}",
    "Object.defineProperties({},null)", "Object.defineProperties({},undefined)", "Object.defineProperties({})", "Object.defineProperties(null,{})", "Object.defineProperties(1,{})", "Object.defineProperties('x',{})", "Object.defineProperties({},1)", "Object.defineProperties({},'ab')", "Object.defineProperties({},[{value:1}])", "Object.defineProperties({},{a:'x'})", "Object.defineProperties({},{a:1})",
    "{const o={};Object.defineProperties(o,{get a(){W.push('a');return {value:1}},get b(){W.push('b');return {value:2}}});return [W,Reflect.ownKeys(o)]}", "{const src=Object.create({inh:{value:1}});Object.defineProperty(src,'hid',{value:{value:2},enumerable:false});src.own={value:3};return Reflect.ownKeys(Object.defineProperties({},src))}",
    "{const o={};Object.defineProperties(o,{[Symbol.iterator]:{value:1,enumerable:true},a:{value:2}});return Reflect.ownKeys(o)}", "{const o={};Object.defineProperties(o,new Proxy({a:{value:1}},{ownKeys(t){W.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){W.push('gopd:'+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){W.push('get:'+k);return t[k]}}));return W}",
    "Object.defineProperty()", "Object.defineProperty({})", "Object.defineProperty({},'a')", "Object.defineProperty(null,'a',{})", "Object.defineProperty(1,'a',{})", "Object.defineProperty('x','a',{})", "Object.defineProperty(Symbol(),'a',{})", "Object.defineProperty([],'a',{})===undefined", "Object.defineProperty({},'a',{value:1})", "Object.defineProperty({},{toString(){return 'k'}},{value:1})", "Object.defineProperty({},{toString(){throw new RangeError('ts')}},{value:1})",
    "Object.defineProperty({},Symbol.iterator,{value:1})", "Object.defineProperty({},1,{value:1})", "Object.defineProperty({},-0,{value:1})", "Object.defineProperty({},null,{value:1})", "Object.defineProperty({},undefined,{value:1})", "Object.defineProperty({},'a',{value:1}).a", "Object.defineProperty(function(){},'name',{value:'q'}).name", "Object.defineProperty(function(){},'name',{get:g}).name", "Object.defineProperty(function(){},'length',{value:'q'}).length",
    "Object.defineProperty(function(){},'prototype',{value:1})", "Object.defineProperty(function(){},'prototype',{writable:false}).prototype===undefined", "Object.defineProperty(function(){},'prototype',{enumerable:true})", "Object.defineProperty(function(){},'prototype',{get:g})", "Object.defineProperty(class{},'prototype',{value:1})", "Object.defineProperty(class{},'name',{value:'q'}).name", "Object.defineProperty(class{},'name',{writable:true}).name",
    "Object.defineProperty(Math,'PI',{value:3})", "Object.defineProperty(Math,'PI',{value:Math.PI})===Math", "Object.defineProperty(Math,'PI',{writable:true})", "Object.defineProperty(globalThis,'undefined',{value:1})", "Object.defineProperty(globalThis,'undefined',{value:undefined})===globalThis", "Object.defineProperty(Object.prototype,'__proto__',{value:1})", "Object.defineProperty(Object.prototype,'constructor',{get:g})", "Object.defineProperty(Array.prototype,'length',{value:3}).length",
    "Object.defineProperty(Symbol,'iterator',{value:1})", "Object.defineProperty(Number,'MAX_VALUE',{writable:true})", "Object.defineProperty(String.prototype,'length',{value:3}).length", "Object.defineProperty(Object.defineProperty({},'x',{value:1}),'x',{value:2})", "Object.defineProperty((function(){return arguments})(1),'0',{get:g})[0]", "{const a=(function(){return arguments})(1,2);Object.defineProperty(a,'0',{writable:false});return [a[0],a.length]}",
    "{const f=function(x){Object.defineProperty(arguments,'0',{value:9});return x};return f(1)}", "{const f=function(x){Object.defineProperty(arguments,'0',{writable:false});x=5;return arguments[0]};return f(1)}", "{const f=function(x){arguments[0]=7;return x};return f(1)}", "{const f=function(x){'use strict';arguments[0]=7;return x};return f(1)}", "{const f=function(x){Object.defineProperty(arguments,'0',{get(){return 3}});return [arguments[0],x]};return f(1)}", "{const f=function(x){delete arguments[0];arguments[0]=2;return x};return f(1)}",
  ];
  for (const st of sp) prog("", st);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "accessor-golden-"));
const file = path.join(dir, "accessor_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, RESULT_PRELOAD);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const rows = [];
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body;
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  const { prepared: { source, meta }, marked } = runPrepared(original, file, preload, dir, 20000);
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRE.length)) + "\n");
    continue;
  }
  kept++;
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("accessor", rows));
fs.rmSync(dir, { recursive: true, force: true });
