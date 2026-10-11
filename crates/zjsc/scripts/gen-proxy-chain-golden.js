// Gera tests/golden/proxy_chain_bun.tsv: Proxy na cadeia de protótipos e em torno de funções, classes e arrays, medido no bun.
// Cobre get/set/has/in/for-in/Object.keys/delete/instanceof/Symbol.hasInstance/JSON/spread/with com o log das traps,
// Proxy de função e de classe (apply/construct/new.target), Proxy.revocable revogado no meio de operações, Proxy sobre
// arrays (isArray, length, concat spreadable, sort, splice), Proxy como alvo de Object.assign/defineProperties/freeze,
// Proxy de Proxy, receiver nas traps e traps que devolvem valores inválidos (mensagens exatas de TypeError).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda em um bun filho novo.
// Programas cuja expressão já aparece nos goldens proxy_*_bun e reflect_*_bun são descartados.
// Uso: bun scripts/gen-proxy-chain-golden.js > tests/golden/proxy_chain_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var L=[];function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>2)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function Z(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return r+" | "+L.join(",")}\n' +
  'var TN=["get","set","has","deleteProperty","defineProperty","getOwnPropertyDescriptor","ownKeys","getPrototypeOf","setPrototypeOf","isExtensible","preventExtensions","apply","construct"];\n' +
  'function H(g,skip){var h={};TN.forEach(n=>{if(skip&&skip.indexOf(n)>=0)return;h[n]=function(t,...a){var k=a[0];var r=n==="get"?a[1]:n==="set"?a[2]:undefined;' +
  'L.push(g+n+(n==="apply"||n==="construct"||typeof k==="object"||typeof k==="function"||k===undefined?"":":"+String(k))+(r===undefined?"":r===globalThis.RC?"@o":"@x"));return Reflect[n](t,...a)}});return h}\n';

const cases = [];
const add = (body, mode) => cases.push({ body, mode: mode || "both" });
const wrap = body => (/^(with|var|return)\b/.test(body) || (!/^\(/.test(body) && body.includes(";")) ? body : "return " + body);
const Z = body => `Z(()=>{${wrap(body)}})`;

// ---- 1. Proxy no meio da cadeia de protótipos: setups por operações.
const chainSetups = {
  plain: "var o=Object.create(new Proxy({a:1,b:2},H('')));globalThis.RC=o;",
  getter: "var o=Object.create(new Proxy({get a(){L.push('getter '+(this===o));return 1},set a(v){L.push('setter '+(this===o))},b:2},H('')));globalThis.RC=o;",
  twoDeep: "var o=Object.create(Object.create(new Proxy({a:1},H(''))));globalThis.RC=o;",
  proxyOfProxy: "var o=Object.create(new Proxy(new Proxy({a:1,b:2},H('i.')),H('o.')));globalThis.RC=o;",
  deepTarget: "var o=Object.create(new Proxy(Object.create({a:'deep',c:3}),H('')));globalThis.RC=o;",
  arrayTarget: "var o=Object.create(new Proxy([1,2,3],H('')));globalThis.RC=o;",
  onlyGet: "var o=Object.create(new Proxy({a:1},{get(t,k,r){L.push('get:'+String(k)+(r===globalThis.RC?'@o':'@x'));return t[k]}}));globalThis.RC=o;",
  setFalse: "var o=Object.create(new Proxy({a:1},{set(){L.push('set');return false}}));globalThis.RC=o;",
  hasTrue: "var o=Object.create(new Proxy({a:1},{has(t,k){L.push('has:'+String(k));return true}}));globalThis.RC=o;",
  ownKeysFake: "var o=Object.create(new Proxy({a:1},{ownKeys(){L.push('ownKeys');return ['x','a']},getOwnPropertyDescriptor(t,k){L.push('gopd:'+k);return {value:k,enumerable:true,configurable:true}}}));globalThis.RC=o;",
  protoTrap: "var o=Object.create(new Proxy({a:1},{getPrototypeOf(t){L.push('gpo');return Array.prototype}}));globalThis.RC=o;",
  proxyTarget: "var o=new Proxy(Object.create(new Proxy({a:1},H('p.'))),H('o.'));globalThis.RC=o;",
  ownShadow: "var o=Object.create(new Proxy({a:1},H('')));o.a='own';globalThis.RC=o;",
  nullProtoTarget: "var o=Object.create(new Proxy(Object.create(null),H('')));o.q=1;globalThis.RC=o;",
  frozenTarget: "var o=Object.create(new Proxy(Object.freeze({a:1}),H('')));globalThis.RC=o;",
};
const chainOps = [
  "o.a", "o.b", "o.zz", "o.a=5", "o.zz=5", "o.a=5;return Object.keys(o).join()+' '+Object.getOwnPropertyNames(o).join()", "'a' in o", "'zz' in o",
  "delete o.a", "Object.keys(o).join()", "(function(){var r=[];for(var k in o)r.push(k);return r.join()})()", "JSON.stringify(o)", "JSON.stringify({o})",
  "Object.keys({...o}).join()", "Object.assign({},o)", "o instanceof Object", "Object.getPrototypeOf(o)===Object.prototype", "o.hasOwnProperty('a')",
  "Object.entries(o).join('|')", "Reflect.ownKeys(o).map(String).join()", "o+''", "o[Symbol.iterator]", "o.length", "o[0]", "o[Symbol.toPrimitive]",
  "Reflect.get(o,'a',{})", "Reflect.set(o,'a',9,{})", "Reflect.has(o,'a')", "Object.getOwnPropertyDescriptor(o,'a')", "o.constructor===Object",
  "Object.setPrototypeOf(o,null)===o", "o.__proto__===Object.prototype", "Object.create(o).a", "Object.create(o).zz=1", "Array.isArray(o)", "o instanceof Array",
  "Object.prototype.toString.call(o)", "Object.values(o).join()", "o.a++", "o.a+=1", "o['a']='s';return o.a", "Object.defineProperty(o,'a',{value:3});return o.a",
  "[...Object.keys(o)].length", "String(Object.getPrototypeOf(Object.getPrototypeOf(o))===Object.prototype)", "(()=>{for(var k in o){return k}})()",
  "Object.fromEntries(Object.entries(o)).a", "structuredClone===undefined", "o.propertyIsEnumerable('a')", "Object.hasOwn(o,'a')",
];
for (const [name, setup] of Object.entries(chainSetups)) {
  for (const op of chainOps) add(`${setup}${wrap(Z0(op))}`);
  for (const w of ["with(o){return typeof a}", "with(o){a=7;return a}", "with(o){return typeof zz}", "with(o){return a+b}", "with(o){zz=3}return typeof zz", "with(o){return (()=>a)()}", "with(o){return delete a}", "with(o){return 'a' in o}"]) {
    add(`${setup}${w}`, "sloppy");
  }
}
function Z0(op) { return op; }

// ---- 2. Traps que devolvem valores inválidos.
const invalidValues = ["undefined", "null", "1", "'s'", "true", "false", "{}", "[]", "Symbol()", "function(){}", "NaN", "0", "-0", "[1,'a']", "['a','a']", "[Symbol.iterator]", "{value:1}", "{value:2,configurable:false}", "{get(){},set(){}}", "{enumerable:true}", "Object.prototype", "Array.prototype", "new Proxy({},{})"];
const trapOps = {
  get: "p.a", set: "p.a=1", has: "'a' in p", deleteProperty: "delete p.a", defineProperty: "Object.defineProperty(p,'a',{value:2})",
  getOwnPropertyDescriptor: "Object.getOwnPropertyDescriptor(p,'a')", ownKeys: "Reflect.ownKeys(p).map(String).join()", getPrototypeOf: "Object.getPrototypeOf(p)",
  setPrototypeOf: "Reflect.setPrototypeOf(p,{})", isExtensible: "Object.isExtensible(p)", preventExtensions: "Object.preventExtensions(p)===p",
};
const objTargets = ["{a:1}", "Object.freeze({a:1})", "Object.preventExtensions({a:1})", "Object.defineProperty({},'a',{value:1})", "{}", "Object.defineProperty({},'a',{get(){return 1},configurable:false})", "Object.preventExtensions({})"];
for (const [trap, op] of Object.entries(trapOps)) {
  for (const t of objTargets) for (const v of invalidValues) {
    add(`var t=${t};var p=new Proxy(t,{${trap}(){L.push('${trap}');return ${v}}});${wrap(Z0(op))}`);
  }
}
const fnTargets = ["function(){return 1}", "class{}", "()=>1"];
for (const [trap, op] of [["apply", "p()"], ["construct", "new p()"], ["apply", "Reflect.apply(p,1,[])"], ["construct", "Reflect.construct(p,[],Object)"]]) {
  for (const t of fnTargets) for (const v of invalidValues) {
    add(`var p=new Proxy(${t},{${trap}(){L.push('${trap}');return ${v}}});${wrap(op)}`);
  }
}
// Trap que não é função.
for (const trap of [...Object.keys(trapOps), "apply", "construct"]) {
  for (const v of ["1", "'s'", "{}", "true", "Symbol()", "[]", "null", "undefined", "0n"]) {
    const target = trap === "apply" || trap === "construct" ? "function(){}" : "{a:1}";
    const op = trap === "apply" ? "p()" : trap === "construct" ? "new p()" : trapOps[trap];
    add(`var p=new Proxy(${target},{${trap}:${v}});${wrap(op)}`);
  }
}
// Trap que lança e trap com getter de handler que lança ou conta.
for (const [trap, op] of Object.entries(trapOps)) {
  add(`var p=new Proxy({a:1},{${trap}(){throw new RangeError('boom')}});${wrap(op)}`);
  add(`var n=0;var h={};Object.defineProperty(h,'${trap}',{get(){n++;L.push('read');return undefined}});var p=new Proxy({a:1},h);${wrap(op)}+' '+n`.replace(/^(.*;)return (.*)\+' '\+n$/, "$1return $2+' '+n"));
  add(`var p=new Proxy({a:1},new Proxy({},{get(t,k){L.push('h.get:'+String(k));return undefined}}));${wrap(op)}`);
  add(`var p=new Proxy({a:1},Object.create({${trap}(){L.push('inherited');return Reflect.${trap}(...arguments)}}));${wrap(op)}`);
}

// ---- 3. Proxy de função e de classe: apply, construct, new.target.
const fnSetups = {
  fn: "function f(a,b){return new.target===undefined?'call:'+a+b:'new'}",
  retObj: "function f(){return {k:new.target===undefined}}",
  cls: "class f{constructor(a){this.a=a;this.nt=new.target===f}}",
  derived: "class B{constructor(){this.base=new.target.name}}class f extends B{constructor(){super();this.d=1}}",
  arrow: "var f=(a)=>a",
  bound: "var f=(function(a){return [this&&this.x,a,new.target===undefined]}).bind({x:1},'b')",
  date: "var f=Date", array: "var f=Array", map: "var f=Map", promise: "var f=Promise", symbol: "var f=Symbol", bigint: "var f=BigInt", obj: "var f=Object",
  method: "var f=({m(){return 1}}).m", gen: "var f=function*(){yield 1}", async: "var f=async function(){return 1}",
};
const fnHandlers = {
  none: "{}", logAll: "H('')", applyOnly: "{apply(t,th,args){L.push('apply '+args.length+' '+(th===undefined));return Reflect.apply(t,th,args)}}",
  constructOnly: "{construct(t,args,nt){L.push('construct '+args.length+' '+(nt===p));return Reflect.construct(t,args,nt)}}",
  constructPrim: "{construct(){return 1}}", constructObj: "{construct(){return {cp:1}}}", applyConst: "{apply(){return 'A'}}",
  constructNt: "{construct(t,a,nt){L.push('nt.name '+nt.name);return Reflect.construct(t,a,Object)}}",
  notFn: "{apply:1,construct:2}", nullTraps: "{apply:null,construct:undefined}", throwing: "{apply(){throw new EvalError('ap')},construct(){throw new EvalError('co')}}",
  getFake: "{get(t,k,r){L.push('get '+String(k));return Reflect.get(t,k,r)}}",
};
const fnOps = [
  "p(1,2)", "new p(1,2)", "Reflect.construct(p,[1],Object)", "Reflect.construct(p,[1],p)", "Reflect.construct(Object,[],p)", "Reflect.apply(p,{x:1},[3])", "p.call({x:1},4)", "p.apply(null,[5])",
  "p.bind(null,1)()", "new (p.bind(null,1))()", "class X extends p{};return typeof new X()", "typeof p", "p.name", "p.length", "typeof p.prototype", "p instanceof Function",
  "Object.getPrototypeOf(p)===Function.prototype", "({}) instanceof p", "new p() instanceof p", "Object.prototype.toString.call(p)", "p.toString===Function.prototype.toString",
  "Function.prototype.toString.call(p).length>0", "Function.prototype.call.call(p,1)", "[1,2].map(p).length", "new p instanceof Object", "Reflect.construct(function(){return new.target===p},[],p)",
  "Object.getOwnPropertyNames(p).join()", "p.hasOwnProperty('prototype')", "p.constructor===Function", "Symbol.hasInstance in p", "p[Symbol.hasInstance]===Function.prototype[Symbol.hasInstance]",
  "(()=>{try{return new p()}catch(e){return e.constructor===TypeError}})()", "new new p()",
];
for (const [sn, su] of Object.entries(fnSetups)) {
  const decl = su.endsWith(";") ? su : su + ";";
  for (const [hn, h] of Object.entries(fnHandlers)) {
    for (const op of fnOps) {
      if ((hn === "constructPrim" || hn === "constructObj" || hn === "throwing" || hn === "constructNt") && sn !== "fn" && sn !== "cls" && sn !== "arrow") continue;
      if (hn === "logAll" && op.length > 40) continue;
      add(`${decl}var p=new Proxy(f,${h});${wrap(Z0(op))}`);
    }
  }
}

// ---- 4. Proxy.revocable revogado no meio de operações.
const revTargets = { obj: "{a:1,b:2}", arr: "[1,2,3]", fn: "function(){return 1}", cls: "class{}", nested: "new Proxy({a:1},H('in.'))" };
const revOps = [
  "r.proxy.a", "r.proxy.a=1", "'a' in r.proxy", "delete r.proxy.a", "Object.keys(r.proxy)", "Object.getPrototypeOf(r.proxy)", "Object.setPrototypeOf(r.proxy,null)", "Object.isExtensible(r.proxy)",
  "Object.preventExtensions(r.proxy)", "Object.defineProperty(r.proxy,'x',{value:1})", "Object.getOwnPropertyDescriptor(r.proxy,'a')", "Reflect.ownKeys(r.proxy)", "typeof r.proxy", "r.proxy instanceof Object",
  "Array.isArray(r.proxy)", "JSON.stringify(r.proxy)", "Object.prototype.toString.call(r.proxy)", "String(r.proxy)", "r.proxy()", "new r.proxy()", "[...r.proxy]", "({...r.proxy})", "Object.assign({},r.proxy)",
  "Object.freeze(r.proxy)", "Object.isFrozen(r.proxy)", "Object.entries(r.proxy)", "Object.create(r.proxy).a", "'a' in Object.create(r.proxy)", "Object.create(r.proxy).a=1", "[].concat(r.proxy).length",
  "Object.getOwnPropertyNames(r.proxy)", "r.proxy.hasOwnProperty('a')", "Object.hasOwn(r.proxy,'a')", "Reflect.has(r.proxy,'a')", "Reflect.get(r.proxy,'a')", "Reflect.getPrototypeOf(r.proxy)", "r.revoke()===undefined",
  "Object.keys(Object.create(r.proxy))", "(function(){var k=[];for(var x in Object.create(r.proxy))k.push(x);return k})()", "r.proxy.length", "Symbol.iterator in r.proxy", "Proxy.revocable(r.proxy,{})", "new Proxy(r.proxy,{})",
  "Object.fromEntries(r.proxy)", "Array.from(r.proxy)", "Object.values(r.proxy)", "Object.prototype.isPrototypeOf.call(r.proxy,{})", "({}).isPrototypeOf(r.proxy)", "r.proxy+''", "r.proxy.toString", "Function.prototype.call.call(r.proxy)",
];
for (const [tn, t] of Object.entries(revTargets)) for (const op of revOps) {
  add(`var r=Proxy.revocable(${t},{});r.revoke();${wrap(Z0(op))}`);
  add(`var r=Proxy.revocable(${t},H(''));r.revoke();${wrap(Z0(op))}`);
  if (tn === "obj" || tn === "arr") add(`var r=Proxy.revocable(${t},H(''));var o=Object.create(r.proxy);var q=r.proxy;r.revoke();var r={proxy:o};${wrap(Z0(op))}`);
}
// Revoga dentro da trap, no meio da operação.
const midRevoke = [
  ["get", "r.proxy.a;return 1", "get(t,k,rc){L.push('get');r.revoke();return t[k]}"],
  ["ownKeysKeys", "Object.keys(r.proxy).join()", "ownKeys(t){L.push('ownKeys');r.revoke();return Reflect.ownKeys(t)}"],
  ["gopdKeys", "Object.keys(r.proxy).join()", "getOwnPropertyDescriptor(t,k){L.push('gopd:'+k);r.revoke();return Reflect.getOwnPropertyDescriptor(t,k)}"],
  ["gopdEntries", "Object.entries(r.proxy).join()", "get(t,k){L.push('get:'+String(k));r.revoke();return t[k]}"],
  ["forIn", "(function(){var k=[];for(var x in r.proxy)k.push(x);return k.join()})()", "ownKeys(t){L.push('ownKeys');r.revoke();return Reflect.ownKeys(t)}"],
  ["forInGopd", "(function(){var k=[];for(var x in r.proxy)k.push(x);return k.join()})()", "getOwnPropertyDescriptor(t,k){L.push('gopd:'+k);r.revoke();return Reflect.getOwnPropertyDescriptor(t,k)}"],
  ["json", "JSON.stringify(r.proxy)", "get(t,k){L.push('get:'+String(k));if(k==='b')r.revoke();return t[k]}"],
  ["jsonKeys", "JSON.stringify(r.proxy)", "ownKeys(t){L.push('ownKeys');r.revoke();return Reflect.ownKeys(t)}"],
  ["assign", "Object.assign({},r.proxy)", "ownKeys(t){L.push('ownKeys');r.revoke();return Reflect.ownKeys(t)}"],
  ["assignGet", "Object.assign({},r.proxy)", "get(t,k){L.push('get:'+String(k));r.revoke();return t[k]}"],
  ["spread", "({...r.proxy})", "get(t,k){L.push('get:'+String(k));r.revoke();return t[k]}"],
  ["spreadGopd", "({...r.proxy})", "getOwnPropertyDescriptor(t,k){L.push('gopd:'+k);r.revoke();return Reflect.getOwnPropertyDescriptor(t,k)}"],
  ["has", "'a' in r.proxy", "has(t,k){L.push('has');r.revoke();return k in t}"],
  ["hasThenGet", "('a' in r.proxy)+':'+r.proxy.a", "has(t,k){L.push('has');r.revoke();return k in t}"],
  ["setThenGet", "r.proxy.a=2;return r.proxy.a", "set(t,k,v,rc){L.push('set');r.revoke();t[k]=v;return true}"],
  ["define", "Object.defineProperty(r.proxy,'z',{value:1,configurable:true});return Object.keys(r.proxy)", "defineProperty(t,k,d){L.push('define');r.revoke();return Reflect.defineProperty(t,k,d)}"],
  ["delete", "delete r.proxy.a", "deleteProperty(t,k){L.push('delete');r.revoke();return delete t[k]}"],
  ["getProto", "Object.getPrototypeOf(r.proxy)", "getPrototypeOf(t){L.push('gpo');r.revoke();return Reflect.getPrototypeOf(t)}"],
  ["instanceof", "r.proxy instanceof Object", "getPrototypeOf(t){L.push('gpo');r.revoke();return Object.prototype}"],
  ["freeze", "Object.freeze(r.proxy)", "preventExtensions(t){L.push('pe');r.revoke();return Reflect.preventExtensions(t)}"],
  ["freezeDefine", "Object.freeze(r.proxy)", "defineProperty(t,k,d){L.push('def:'+k);r.revoke();return Reflect.defineProperty(t,k,d)}"],
  ["isFrozen", "Object.isFrozen(r.proxy)", "isExtensible(t){L.push('ie');r.revoke();return Reflect.isExtensible(t)}"],
  ["concat", "[].concat(r.proxy).length", "get(t,k){L.push('get:'+String(k));r.revoke();return Reflect.get(t,k)}"],
  ["sort", "r.proxy.sort().join()", "get(t,k){L.push('get:'+String(k));if(k==='1')r.revoke();return Reflect.get(t,k)}"],
  ["splice", "r.proxy.splice(0,1).join()", "deleteProperty(t,k){L.push('del:'+k);r.revoke();return Reflect.deleteProperty(t,k)}"],
  ["push", "r.proxy.push(9)", "set(t,k,v,rc){L.push('set:'+k);if(k==='length')r.revoke();return Reflect.set(t,k,v,rc)}"],
  ["isArray", "Array.isArray(r.proxy)", "get(t,k){L.push('get');r.revoke();return t[k]}"],
  ["apply", "r.proxy()", "apply(t,th,a){L.push('apply');r.revoke();return Reflect.apply(t,th,a)}"],
  ["construct", "new r.proxy()", "construct(t,a,nt){L.push('construct');r.revoke();return Reflect.construct(t,a,nt)}"],
  ["withScope", "with(r.proxy){return typeof a}", "has(t,k){L.push('has:'+String(k));r.revoke();return k in t}"],
  ["chainGet", "Object.create(r.proxy).a", "get(t,k,rc){L.push('get');r.revoke();return t[k]}"],
  ["chainHas", "'a' in Object.create(r.proxy)", "has(t,k){L.push('has');r.revoke();return k in t}"],
  ["chainSet", "var o=Object.create(r.proxy);o.a=1;return Object.keys(o).join()+r.proxy", "set(t,k,v,rc){L.push('set');r.revoke();return Reflect.set(t,k,v,rc)}"],
  ["fromEntries", "Object.fromEntries(r.proxy)", "get(t,k){L.push('get:'+String(k));r.revoke();return Reflect.get(t,k)}"],
  ["join", "r.proxy.join()", "get(t,k){L.push('get:'+String(k));if(k==='0')r.revoke();return Reflect.get(t,k)}"],
];
for (const [name, op, trap] of midRevoke) {
  const isFn = /apply|construct/.test(name) && !/Keys/.test(name);
  const targets = isFn ? ["function(){return 1}", "class{}"] : /sort|splice|push|join|concat|isArray/.test(name) ? ["[3,1,2]", "['a','b']", "[]"] : ["{a:1,b:2}", "{a:1}", "[1,2]"];
  for (const t of targets) {
    const body = wrap(op);
    add(`var r=Proxy.revocable(${t},{${trap}});${/^with/.test(op) ? op : body}`, /^with/.test(op) ? "sloppy" : "both");
  }
}

// ---- 5. Proxy sobre arrays.
const arrays = ["[3,1,2]", "[1,,3]", "[]", "['b','a','c']", "[1,2,3,4,5]", "Object.assign([1,2],{x:'p'})"];
const arrayOps = [
  "Array.isArray(p)", "p.length", "p.length=1;return S(p)", "p.length=0;return p.length", "p.push(4)", "p.pop()", "p.shift()", "p.unshift(0)", "p.splice(1,1)", "p.splice(0,0,'x')", "p.splice(1)", "p.sort()", "p.sort((a,b)=>b-a)",
  "p.reverse()", "p.concat([9])", "[].concat(p)", "p.concat(p)", "[0].concat(p,p).length", "p.slice(1)", "p.map(x=>x)", "p.filter(x=>x)", "p.join('-')", "p.indexOf(1)", "p.includes(2)", "p.flat()", "p.fill(0)",
  "p.copyWithin(0,1)", "Array.from(p)", "[...p]", "(function(){var r=[];for(var x of p)r.push(x);return r})()", "JSON.stringify(p)", "Object.keys(p)", "p instanceof Array", "Object.prototype.toString.call(p)",
  "p.at(-1)", "p.find(x=>x>1)", "p.findLast(x=>x)", "p.some(x=>x)", "p.every(x=>x)", "p.reduce((a,b)=>a+b,0)", "p.entries().next().value", "p.keys().next().value", "p.toString()", "p.toSorted()", "p.toReversed()", "p.with(0,'w')",
  "p.toSpliced(0,1)", "p.lastIndexOf(2)", "p.flatMap(x=>[x,x])", "Array.prototype.concat.call(p,1).length", "Array.prototype.slice.call(p).length", "p.constructor===Array", "p[Symbol.iterator]===Array.prototype[Symbol.iterator]",
  "p[Symbol.isConcatSpreadable]", "delete p[0]", "p[5]=1;return p.length", "p.x=1;return Object.keys(p)", "Object.getOwnPropertyNames(p).join()", "Object.entries(p).length", "Array.of.call(function(n){return new Proxy([],H('c.'))},1,2).length",
  "Array.prototype.map.call(p,x=>x)", "Object.assign([],p)", "Array.prototype.push.apply(p,[7,8])", "[].concat.apply([],[p]).length", "p.forEach(x=>x)", "Math.max(...p)", "String(p)", "p+''", "`${p}`",
];
for (const a of arrays) for (const op of arrayOps) {
  add(`var p=new Proxy(${a},H(''));${wrap(Z0(op))}`);
}
const spreadable = [
  ["get true", "var p=new Proxy([1,2],{get(t,k,r){if(k===Symbol.isConcatSpreadable){L.push('spreadable');return true}return Reflect.get(t,k,r)}})"],
  ["get false", "var p=new Proxy([1,2],{get(t,k,r){if(k===Symbol.isConcatSpreadable){L.push('spreadable');return false}return Reflect.get(t,k,r)}})"],
  ["obj true", "var p=new Proxy({length:2,0:'a',1:'b'},{get(t,k,r){if(k===Symbol.isConcatSpreadable)return true;return Reflect.get(t,k,r)}})"],
  ["obj log", "var p=new Proxy({length:2,0:'a',1:'b',[Symbol.isConcatSpreadable]:true},H(''))"],
  ["obj undefined", "var p=new Proxy({length:1,0:'a'},H(''))"],
  ["nested array proxy", "var p=new Proxy(new Proxy([1,2],H('i.')),H('o.'))"],
  ["len huge", "var p=new Proxy({length:2**53-1,[Symbol.isConcatSpreadable]:true},{})"],
  ["len getter", "var p=new Proxy([1,2],{get(t,k,r){if(k==='length'){L.push('len');return 3}return Reflect.get(t,k,r)}})"],
  ["len string", "var p=new Proxy([1,2],{get(t,k,r){if(k==='length')return '2';return Reflect.get(t,k,r)}})"],
  ["len negative", "var p=new Proxy([1,2],{get(t,k,r){if(k==='length')return -1;return Reflect.get(t,k,r)}})"],
  ["len object", "var p=new Proxy([1,2],{get(t,k,r){if(k==='length')return {valueOf(){L.push('vo');return 1}};return Reflect.get(t,k,r)}})"],
  ["has hole", "var p=new Proxy([1,,3],{has(t,k){L.push('has:'+String(k));return k==='1'?true:k in t}})"],
];
for (const [name, setup] of spreadable) for (const op of ["[0].concat(p)", "[0].concat(p).length", "[].concat(p,p).length", "Array.prototype.concat.call(p,[1])", "[...[].concat(p)].join()", "p.slice().length", "Array.from(p)", "[...p]", "Array.isArray(p)", "p.map(x=>x)", "Array.prototype.flat.call([p])", "[p].flat()", "[p].flat(Infinity).length", "p.join()", "p.indexOf(2)", "p.includes(undefined)", "p.reverse()", "p.sort()", "p.splice(0,1)", "p.push(1)", "JSON.stringify(p)", "Object.keys(p)"]) {
  add(`${setup};${wrap(op)}`);
}

// ---- 6. Proxy como alvo de Object.assign, defineProperties, freeze e afins.
const objProxyTargets = ["{}", "{a:1}", "{a:1,[Symbol.for('s')]:2}", "[1]", "function(){}", "Object.freeze({a:1})", "Object.preventExtensions({})", "{get a(){return 1},set a(v){L.push('target setter')}}", "Object.create({a:'inh'})", "Object.defineProperty({},'a',{value:1,writable:false,configurable:true})"];
const handlerSkips = [null, ["set"], ["defineProperty"], ["getOwnPropertyDescriptor"], ["ownKeys"], ["get"], ["set", "defineProperty"]];
const assignOps = [
  "Object.assign(p,{a:2,b:3});return Object.keys(p).join()", "Object.assign({},p)", "Object.assign(p,p)", "Object.assign(p,[7])", "Object.assign(p,'xy')", "Object.assign(p,{[Symbol('q')]:1}).constructor===Object",
  "Object.defineProperties(p,{x:{value:1},y:{get(){return 2},configurable:true}});return Object.keys(p).join()", "Object.defineProperties({},p)", "Object.defineProperties(p,p)", "Object.defineProperties(p,{a:{value:5}})",
  "Object.freeze(p);return Object.isFrozen(p)", "Object.seal(p);return Object.isSealed(p)", "Object.preventExtensions(p);return Object.isExtensible(p)", "Object.isFrozen(p)", "Object.isSealed(p)", "Object.isExtensible(p)",
  "Object.getOwnPropertyDescriptors(p)", "Object.entries(p)", "Object.fromEntries(Object.entries(p))", "Object.values(p)", "Object.getOwnPropertySymbols(p).length", "Object.groupBy([1,2],x=>x%2?'o':'e')&&Object.keys(p)",
  "Object.setPrototypeOf(p,null)===p", "Object.setPrototypeOf(p,Object.prototype)===p", "Object.create(p).constructor===Object", "Object.create(null,p)", "Object.create(Object.prototype,p)",
  "var o={...p};return Object.keys(o).join()", "var {a,...rest}=p;return S(rest)", "var {a=5}=p;return a", "var [x]=Object.assign(p,{0:1,length:1});return x", "p.a=1;p.a=2;return p.a", "p['b']=2;delete p.b;return Object.keys(p)",
  "Object.defineProperty(p,'n',{value:1});return Object.getOwnPropertyDescriptor(p,'n')", "Reflect.defineProperty(p,'n',{value:1})", "Reflect.set(p,'a',1,{})", "Reflect.set(p,'a',1)", "Reflect.deleteProperty(p,'a')", "JSON.stringify(p)", "JSON.stringify({p},null,1)",
  "structuredClone===1", "Object.keys(Object.assign(Object.create(p),{z:1}))", "Object.assign(Object.create(p),{a:'x'}).a", "var o=Object.create(p);o.a='x';return Object.keys(o).join()+Object.getOwnPropertyNames(p).join()",
];
for (const t of objProxyTargets) for (const skip of handlerSkips) {
  const h = skip ? `H('',${JSON.stringify(skip)})` : "H('')";
  for (const op of assignOps) {
    if (skip && skip.length > 1 && !/assign|defineProperty/.test(op)) continue;
    add(`var p=new Proxy(${t},${h});${wrap(Z0(op))}`);
  }
}

// ---- 7. Proxy de Proxy e receiver nas traps.
const nest = {
  d2: "var p=new Proxy(new Proxy({a:1,b:2},H('1.')),H('2.'))",
  d3: "var p=new Proxy(new Proxy(new Proxy({a:1},H('1.')),H('2.')),H('3.'))",
  mixed: "var p=new Proxy(new Proxy({a:1},{get(t,k,r){L.push('inner get '+String(k)+(r===p?'@outer':'@x'));return Reflect.get(t,k,r)}}),H('o.'))",
  arr: "var p=new Proxy(new Proxy([1,2],H('1.')),H('2.'))",
  fn: "var p=new Proxy(new Proxy(function(){return new.target===undefined},H('1.')),H('2.'))",
  recv: "var p=new Proxy({get a(){L.push('getter this is p '+(this===p));return 1},set a(v){L.push('setter this is p '+(this===p))}},H(''))",
  recvChild: "var base=new Proxy({get a(){L.push('this is child '+(this===child));return 1},set a(v){L.push('set this is child '+(this===child))}},H(''));var child=Object.create(base);var p=child",
  recvSuper: "class A{get a(){return 'A'}}var px=new Proxy(A.prototype,H(''));class B extends A{get a(){return super.a}}Object.setPrototypeOf(B.prototype,px);var p=new B()",
  recvSuperSet: "var base=new Proxy({},H(''));var o={m(){super.x=1;return Object.keys(this).join()}};Object.setPrototypeOf(o,base);var p=o",
  rev: "var rv=Proxy.revocable(new Proxy({a:1},H('1.')),H('2.'));var p=rv.proxy",
  setProtoProxy: "var p=Object.setPrototypeOf({own:1},new Proxy({a:1},H('')))",
};
const nestOps = [
  "p.a", "p.a=2", "'a' in p", "delete p.a", "Object.keys(p).join()", "Reflect.get(p,'a',{})", "Reflect.set(p,'a',5,{})", "Reflect.has(p,'a')", "Reflect.ownKeys(p).map(String).join()", "Object.getPrototypeOf(p)===Object.prototype",
  "Object.isExtensible(p)", "Object.freeze(p)&&Object.isFrozen(p)", "JSON.stringify(p)", "({...p})", "Array.isArray(p)", "typeof p", "p.m&&p.m()", "p.a=1;return Object.keys(p).join()", "Object.getOwnPropertyDescriptor(p,'a')",
  "Object.defineProperty(p,'z',{value:1,configurable:true,enumerable:true});return Object.keys(p).join()", "p instanceof Object", "Object.prototype.toString.call(p)", "p.length", "p()", "new p()", "Object.entries(p).join()",
  "(function(){var r=[];for(var k in p)r.push(k);return r.join()})()", "Reflect.getOwnPropertyDescriptor(p,'a')", "Reflect.setPrototypeOf(p,null)", "Reflect.isExtensible(p)", "Reflect.preventExtensions(p)",
  "Object.assign({},p)", "p.b", "p.b=1", "p[Symbol.toStringTag]", "String(p)", "Object.create(p).a", "var o=Object.create(p);o.a=9;return Object.keys(o).join()", "Reflect.get(p,'a',p)", "Reflect.set(p,'a',1,p)", "Reflect.apply(p,null,[])",
  "Reflect.construct(p,[],Object)", "p.a;p.a;return 1", "rv.revoke();return p.a", "rv.revoke();return Object.keys(p)", "rv.revoke();return 'a' in p",
];
for (const [name, setup] of Object.entries(nest)) for (const op of nestOps) {
  add(`${setup};globalThis.RC=p;${wrap(Z0(op))}`);
}

// ---- 8. instanceof, Symbol.hasInstance e isPrototypeOf.
const instSetups = {
  classProxy: "class C{};var P=new Proxy(C,H(''))", fnProxy: "function C(){};var P=new Proxy(C,H(''))", hasInstGet: "function C(){};var P=new Proxy(C,{get(t,k,r){L.push('get:'+String(k));return Reflect.get(t,k,r)}})",
  hasInstOwn: "function C(){};C[Symbol.hasInstance]=function(v){L.push('own hi '+(this===P));return v===1};var P=new Proxy(C,H(''))",
  hasInstTrap: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k===Symbol.hasInstance)return function(v){L.push('hi');return true};return Reflect.get(t,k,r)}})",
  hasInstBad: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k===Symbol.hasInstance)return 1;return Reflect.get(t,k,r)}})",
  hasInstNull: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k===Symbol.hasInstance)return null;return Reflect.get(t,k,r)}})",
  protoBad: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k==='prototype')return 1;return Reflect.get(t,k,r)}})",
  protoProxy: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k==='prototype')return new Proxy(C.prototype,H('pp.'));return Reflect.get(t,k,r)}})",
  protoNull: "function C(){};var P=new Proxy(C,{get(t,k,r){if(k==='prototype')return null;return Reflect.get(t,k,r)}})",
  nonCallable: "var P=new Proxy({},H(''))", boundC: "function C(){};var P=new Proxy(C.bind(null),H(''))",
};
const instObjs = ["new C()", "{}", "Object.create(new C())", "new Proxy(new C(),H('x.'))", "new Proxy({},{getPrototypeOf(){L.push('gpo');return C.prototype}})", "new Proxy({},{getPrototypeOf(){return null}})", "1", "null", "Object.create(null)", "Object.create(new Proxy(new C(),H('y.')))"];
for (const [name, su] of Object.entries(instSetups)) for (const o of instObjs) {
  const guard = /C\b/.test(su) ? "" : "function C(){};";
  add(`${guard}${su};var v=${o};${wrap("v instanceof P")}`);
  add(`${guard}${su};var v=${o};${wrap("P[Symbol.hasInstance]===undefined?0:Function.prototype[Symbol.hasInstance].call(P,v)")}`);
  add(`${guard}${su};var v=${o};${wrap("C.prototype.isPrototypeOf(v)")}`);
  add(`${guard}${su};var v=${o};${wrap("Object.prototype.isPrototypeOf.call(new Proxy(C.prototype,H('ip.')),v)")}`);
}
// LHS e RHS proxies com getPrototypeOf trap em vários formatos.
const gpoValues = ["null", "Object.prototype", "Array.prototype", "{}", "undefined", "1", "Function.prototype", "new Proxy({},{})", "Object.create(null)", "[]"];
for (const v of gpoValues) {
  for (const t of ["{}", "[]", "function(){}", "Object.preventExtensions({})", "Object.preventExtensions([])"]) {
    for (const op of ["Object.getPrototypeOf(p)===Object.prototype", "p instanceof Object", "p instanceof Array", "Object.prototype.isPrototypeOf.call(Object.prototype,p)", "p.__proto__===Object.prototype", "Reflect.getPrototypeOf(p)", "Object.getPrototypeOf(p)", "Object.create(p).constructor===Object", "Object.prototype.toString.call(p)", "'x' in Object.create(p)", "Object.create(p).x", "p instanceof Function", "Array.isArray(p)"]) {
      add(`var p=new Proxy(${t},{getPrototypeOf(){L.push('gpo');return ${v}}});${wrap(op)}`);
    }
  }
}

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
baseSources.push(...knownPrograms("proxy_chain_bun.tsv", (file) => !(!/^(proxy_|reflect_)/.test(file) || file === "proxy_chain_bun.tsv" || !file.endsWith(".tsv"))));
const baseText = baseSources.join("\n\u0000\n");

const programs = [];
const seen = new Set();
let dup = 0;
for (const c of cases) {
  const modes = c.mode === "both" ? ["strict", "sloppy"] : [c.mode];
  for (const mode of modes) {
    const bodyExpr = c.body.startsWith("Z(") ? c.body : `Z(()=>{${c.body}})`;
    const source = (mode === "strict" ? '"use strict";\n' : "") + PRELUDE + `globalThis.R = ${bodyExpr}`;
    if (seen.has(source)) continue;
    seen.add(source);
    if (c.body.length > 24 && baseText.includes(c.body)) { dup++; continue; }
    programs.push(source);
  }
}

function runChild(source) {
  return new Promise(resolve => {
    // Limite de tempo: programa que não termina (laço sobre length enorme) é descartado.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 5000, killSignal: "SIGKILL", env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", status => { const decoded = status === 0 ? decodeResult(out) : null; resolve({ status: status === 0 && decoded === null ? -1 : status, out: decoded, err }); });
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (true) {
      const i = next++;
      if (i >= programs.length) return;
      results[i] = await runChild(programs[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < programs.length; i++) {
    const r = results[i];
    if (r.status !== 0) { dropped++; process.stderr.write("filho falhou: " + JSON.stringify(programs[i].slice(-160)) + " " + r.err.slice(0, 120) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; continue; }
    if (r.out.includes("\u2014") || r.out.includes("\u2013")) { dropped++; continue; }
    kept++;
    lines.push(JSON.stringify(programs[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("proxy_chain", lines)); // grava também tests/golden/proxy_chain.preludes.json
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
