// Gera tests/golden/eval_forin_bun.tsv: eval com var/function/let em global, função e with; global não extensível e
// com setters; for-in sobre Proxy, objetos exóticos, mutação durante o laço, protótipos com chaves sombreadas e todos os
// alvos de for-in. Medido no bun 1.4.2, um bun filho novo por programa, sem APIs de host. Os programas rodam em
// modo sloppy (a menos que o caso peça "use strict" dentro de função). Programas já presentes em outros goldens são
// descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-eval-forin-golden.js > tests/golden/eval_forin_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  // Toca process.stdout antes: o programa pode deixar o global não extensível.
  const out = process.stdout;
  const write = out.write.bind(out);
  const text = fs.readFileSync(0, "utf8");
  // Como script global (o que o teste faz), não como eval indireto: `var` e `function` do programa ficam não
  // configuráveis no global, e `let` entra no escopo de declarações do script.
  require("vm").runInThisContext(text);
  write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function FI(o){var r=[];for(var k in o)r.push(typeof k+":"+String(k));return r.join()}\n' +
  'var R="";\n';

const programs = [];
const add = (...list) => programs.push(...list);
const q = s => JSON.stringify(s);

// ---- 1. eval: declaração em contexto x código x estado prévio do global.
const evalCodes = [
  "var x=1", "var x", "function x(){return 1}", "let x=1", "const x=1", "class x{}", "var x=1;var x=2", "var x;function x(){}",
  "function x(){} var x", "let x; var x", "var x; let x", "x=5", "var x=typeof x", "function x(){return 1};x=2", "for(var x in {a:1});",
  "for(var x of [1]);", "try{}catch(x){var x=2}", "if(1)function x(){}", "{function x(){}}", "var [x]=[1]", "var {x}={x:1}",
  "'use strict';var x=1", "'use strict';function x(){}", "var x=1;delete x", "var x=1;x=undefined;typeof x", "function x(){};delete x",
  "var arguments", "var undefined", "var NaN=1", "function undefined(){}", "var eval", "let undefined", "var x=this===globalThis",
];
const evalContexts = {
  indirect: c => `(0,eval)(${q(c)})`,
  globalDirect: c => `eval(${q(c)})`,
  fnDirect: c => `(function(){eval(${q(c)});return [typeof x,D(globalThis,"x")].join()})()`,
  fnDirectStrict: c => `(function(){"use strict";eval(${q(c)});return [typeof x,D(globalThis,"x")].join()})()`,
  fnVarShadow: c => `(function(x){eval(${q(c)});return [typeof x,String(x),D(globalThis,"x")].join()})(7)`,
  fnLetShadow: c => `(function(){let x=7;eval(${q(c)});return String(x)})()`,
  withEmpty: c => `(function(){with({}){eval(${q(c)})}return [typeof x,D(globalThis,"x")].join()})()`,
  withHas: c => `(function(){var o={x:0};with(o){eval(${q(c)})}return S(o)+","+typeof x+","+D(globalThis,"x")})()`,
  arrow: c => `(()=>{eval(${q(c)});return typeof x})()`,
  block: c => `(function(){{eval(${q(c)})}return typeof x})()`,
  catchParam: c => `(function(){try{throw 1}catch(x){eval(${q(c)});return String(x)}})()`,
  paramDefault: c => `(function(a=eval(${q(c)}),b=typeof x){return [a,b,typeof x].join()})()`,
  newFunction: c => `Function(${q("eval(" + q(c) + ");return typeof x")})()`,
  evalInEval: c => `eval(${q("eval(" + q(c) + ");typeof x")})`,
};
const statePrior = {
  none: "",
  globalVar: "var x=0;",
  globalLet: "let x=0;",
  globalFn: "function x(){return 'old'}",
  accessorConfigurable: "Object.defineProperty(globalThis,'x',{get(){return 'g'},set(v){},configurable:true,enumerable:false});",
  dataNonConfigurable: "Object.defineProperty(globalThis,'x',{value:'k',writable:false,configurable:false,enumerable:true});",
};
const priorsPerContext = ["none", "globalVar", "globalLet", "globalFn"];
for (const [cname, ctx] of Object.entries(evalContexts)) {
  for (const code of evalCodes) {
    for (const p of cname === "indirect" || cname === "globalDirect" ? Object.keys(statePrior) : priorsPerContext.slice(0, 2)) {
      add(`${statePrior[p]}globalThis.R=T(()=>${ctx(code)});globalThis.R+="|"+T(()=>typeof x+","+D(globalThis,"x"))`);
    }
  }
}

// ---- 2. global não extensível, programa isolado (a extensibilidade vale até o fim do programa).
const neCodes = [
  "var y=1", "var y", "var x=2", "var x", "function y(){}", "function x(){return 3}", "let y=1", "const y=2", "class y{}", "y=1", "'use strict';y=1",
  "var y;y=3", "this.y=1", "globalThis.y=1", "Object.defineProperty(globalThis,'y',{value:1})", "Reflect.set(globalThis,'y',1)", "Reflect.set(globalThis,'x',1)",
  "var undefined", "function NaN(){}", "var Object", "var Object=1", "function Object(){}", "var arguments", "if(1)function y(){}",
  "var y=1;delete y", "var x=2;delete x", "for(var y in {a:1});", "var [y]=[1]", "var y=typeof y",
];
const neContexts = {
  indirect: c => `(0,eval)(${q(c)})`,
  direct: c => `eval(${q(c)})`,
  fn: c => `(function(){eval(${q(c)})})()`,
  fnStrict: c => `(function(){"use strict";eval(${q(c)})})()`,
  withObj: c => `(function(){with({}){eval(${q(c)})}})()`,
  newFunction: c => `Function(${q(c)})()`,
};
const nePriors = {
  none: "",
  existingVar: "var x=1;",
  getterOnly: "Object.defineProperty(globalThis,'x',{get(){return 4},configurable:true});",
  frozenData: "Object.defineProperty(globalThis,'x',{value:5});",
  accessorSetter: "var log=[];Object.defineProperty(globalThis,'x',{get(){return 6},set(v){log.push('set '+v)},configurable:true});",
  existingLet: "let x=1;",
};
for (const [cname, ctx] of Object.entries(neContexts)) {
  for (const code of neCodes) {
    for (const p of cname === "indirect" || cname === "direct" ? ["none", "existingVar", "getterOnly", "frozenData"] : ["none", "existingVar"]) {
      add(`${nePriors[p]}Object.preventExtensions(globalThis);globalThis.R=T(()=>{${ctx(code)};return "ok"})+"|"+Object.isExtensible(globalThis)+"|"+Object.getOwnPropertyNames(globalThis).filter(k=>k==="y"||k==="x").join()`);
    }
  }
}
add(
  "Object.preventExtensions(globalThis);globalThis.R=T(()=>{(0,eval)('var y=1');return typeof y})",
  "Object.preventExtensions(globalThis);var z=1;globalThis.R=typeof z",
  "Object.freeze(globalThis);globalThis.R=T(()=>{(0,eval)('var x=1');return 'ok'})",
  "Object.seal(globalThis);globalThis.R=T(()=>{(0,eval)('var y=1');return 'ok'})",
  "Object.seal(globalThis);var x=1;globalThis.R=T(()=>{(0,eval)('var x=2');return x})",
  "var x=1;Object.seal(globalThis);globalThis.R=T(()=>{(0,eval)('function x(){}');return typeof x})",
  "var x=1;Object.freeze(globalThis);globalThis.R=T(()=>{(0,eval)('var x=2');return x})",
  "Object.preventExtensions(globalThis);globalThis.R=T(()=>{(0,eval)('let w=1;w');return Object.keys(globalThis).includes('w')})",
  "Object.preventExtensions(globalThis);globalThis.R=T(()=>Function('return typeof q;var q')())",
  "Object.preventExtensions(globalThis);globalThis.R=T(()=>{var o=eval('(function(){var m=1;return m})()');return o})",
);

// ---- 3. setters e getters no global durante eval.
const setterDefs = {
  throwSetter: "Object.defineProperty(globalThis,'x',{set(v){throw new RangeError('set '+v)},get(){return 'got'},configurable:true})",
  throwSetterFixed: "Object.defineProperty(globalThis,'x',{set(v){throw new RangeError('set '+v)},get(){return 'got'},configurable:false})",
  logSetter: "var log=[];Object.defineProperty(globalThis,'x',{set(v){log.push('set '+v)},get(){log.push('get');return 'got'},configurable:true,enumerable:true})",
  logSetterFixed: "var log=[];Object.defineProperty(globalThis,'x',{set(v){log.push('set '+v)},get(){log.push('get');return 'got'},configurable:false})",
  getterOnly: "var log=[];Object.defineProperty(globalThis,'x',{get(){log.push('get');return 1},configurable:true})",
  getterOnlyFixed: "Object.defineProperty(globalThis,'x',{get(){return 1}})",
  readonly: "Object.defineProperty(globalThis,'x',{value:1,writable:false,configurable:true})",
  readonlyFixed: "Object.defineProperty(globalThis,'x',{value:1,writable:false,configurable:false})",
  writableFixed: "Object.defineProperty(globalThis,'x',{value:1,writable:true,configurable:false,enumerable:true})",
  writableFixedHidden: "Object.defineProperty(globalThis,'x',{value:1,writable:true,configurable:false,enumerable:false})",
};
const setterCodes = [
  "var x", "var x=2", "var x;x=3", "function x(){}", "function x(){};x", "x=4", "typeof x", "delete x", "var x=x", "for(var x in {a:1});", "for(var x of [5]);",
  "var [x]=[6]", "var {x}={x:7}", "let x=8", "var x=1,y=x", "x++", "x+=1", "x||=9", "x??=9", "if(1)function x(){}", "'use strict';x=4", "'use strict';var x=1",
];
for (const [dname, def] of Object.entries(setterDefs)) {
  for (const code of setterCodes) {
    for (const how of ["(0,eval)", "eval", "Function"]) {
      const run = how === "Function" ? `Function(${q(code)})()` : `${how}(${q(code)})`;
      add(`${def};globalThis.R=T(()=>{${run};return "ok"})+"|"+D(globalThis,"x")+"|"+(typeof log==="undefined"?"":log.join())`);
    }
  }
}

// ---- 4. for-in sobre Proxy, com log de traps.
const trapSets = {
  all: "ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},has(t,k){log.push('has '+String(k));return k in t},get(t,k){log.push('get '+String(k));return t[k]},isExtensible(t){log.push('isExt');return Reflect.isExtensible(t)}",
  ownKeysOnly: "ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)}",
  gopdOnly: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}",
  gpoOnly: "getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)}",
  gpoNull: "getPrototypeOf(t){log.push('gpo');return null}",
  gpoObj: "getPrototypeOf(t){log.push('gpo');return {inh:1,a:'shadow'}}",
  gpoThrows: "getPrototypeOf(t){log.push('gpo');throw new EvalError('gpo')}",
  gpoBad: "getPrototypeOf(t){log.push('gpo');return 1}",
  ownKeysThrows: "ownKeys(t){log.push('ownKeys');throw new EvalError('ok')}",
  gopdThrows: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));throw new EvalError('gopd')}",
  gopdUndef: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return undefined}",
  gopdHidden: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));var d=Reflect.getOwnPropertyDescriptor(t,k);if(d)d.enumerable=false;return d}",
  gopdShow: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return {value:1,enumerable:true,configurable:true}}",
  ownKeysExtra: "ownKeys(t){log.push('ownKeys');return [...Reflect.ownKeys(t),'extra','7',Symbol.for('s')]}",
  ownKeysNumber: "ownKeys(t){log.push('ownKeys');return [1]}",
  ownKeysObject: "ownKeys(t){log.push('ownKeys');return [{}]}",
  ownKeysUndefined: "ownKeys(t){log.push('ownKeys');return [undefined]}",
  ownKeysDup: "ownKeys(t){log.push('ownKeys');return ['a','a']}",
  ownKeysNonArray: "ownKeys(t){log.push('ownKeys');return 'ab'}",
  ownKeysArrayLike: "ownKeys(t){log.push('ownKeys');return {length:1,0:'a'}}",
  ownKeysSymbols: "ownKeys(t){log.push('ownKeys');return [Symbol('a'),'a']}",
  ownKeysEmpty: "ownKeys(t){log.push('ownKeys');return []}",
  ownKeysReverse: "ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t).reverse()}",
  ownKeysIndexLate: "ownKeys(t){log.push('ownKeys');return ['b','2','a','1']}",
  ownKeysMutating: "ownKeys(t){log.push('ownKeys');t.late=1;return Reflect.ownKeys(t)}",
  gopdDeleting: "getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));delete t.b;return Reflect.getOwnPropertyDescriptor(t,k)}",
  enumerateTrap: "enumerate(t){log.push('enumerate');return ['z'][Symbol.iterator]()}",
  getTrap: "get(t,k){log.push('get '+String(k));return t[k]}",
  hasTrap: "has(t,k){log.push('has '+String(k));return k in t}",
  deleteTrap: "deleteProperty(t,k){log.push('del '+String(k));return delete t[k]}",
};
const trapTargets = {
  plain: "{a:1,b:2,3:3,[Symbol('s')]:4}",
  inherited: "Object.create({inh:1,a:0})",
  array: "[1,2]",
  fn: "function(){}",
  empty: "{}",
  frozen: "Object.freeze({a:1,b:2})",
  nonExt: "Object.preventExtensions({a:1})",
  hiddenKey: "Object.defineProperty({a:1},'h',{value:1,enumerable:false})",
  nonConfigurable: "Object.defineProperty({a:1},'n',{value:1,enumerable:true,configurable:false})",
  proxy: "new Proxy({a:1,b:2},{})",
  string: "new String('ab')",
};
for (const [tn, traps] of Object.entries(trapSets)) {
  for (const [gn, target] of Object.entries(trapTargets)) {
    add(`var log=[];globalThis.R=T(()=>{var p=new Proxy(${target},{${traps}});var r=[];try{for(var k in p)r.push(typeof k+":"+String(k))}catch(e){r.push(e.name+": "+e.message)}return r.join()+" // "+log.join()})`);
  }
}
add(
  "var log=[];var p=new Proxy({a:1},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)}});var o=Object.create(p);o.own=1;globalThis.R=T(()=>{var r=[];for(var k in o)r.push(k);return r.join()+' // '+log.join()})",
  "var log=[];var p=new Proxy({a:1},{getPrototypeOf(t){log.push('gpo');return null},ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)}});var o=Object.create(p);globalThis.R=T(()=>{var r=[];for(var k in o)r.push(k);return r.join()+' // '+log.join()})",
  "var log=[];var q=new Proxy({},{ownKeys(){log.push('qkeys');return ['q']},getOwnPropertyDescriptor(t,k){log.push('qgopd '+k);return {value:1,enumerable:true,configurable:true}}});var p=new Proxy({},{getPrototypeOf(){log.push('pgpo');return q},ownKeys(){log.push('pkeys');return ['p']},getOwnPropertyDescriptor(t,k){log.push('pgopd '+k);return {value:1,enumerable:true,configurable:true}}});globalThis.R=T(()=>FI(p)+' // '+log.join())",
  "var r=Proxy.revocable({a:1},{});r.revoke();globalThis.R=T(()=>FI(r.proxy))",
  "var r=Proxy.revocable({a:1},{ownKeys(t){r.revoke();return Reflect.ownKeys(t)}});globalThis.R=T(()=>FI(r.proxy))",
  "var r=Proxy.revocable({a:1,b:2},{getOwnPropertyDescriptor(t,k){if(k==='a')r.revoke();return Reflect.getOwnPropertyDescriptor(t,k)}});globalThis.R=T(()=>FI(r.proxy))",
  "var r=Proxy.revocable({a:1},{});var o=Object.create(r.proxy);r.revoke();globalThis.R=T(()=>FI(o))",
  "var log=[];var p=new Proxy({a:1,b:2,c:3},{getOwnPropertyDescriptor(t,k){log.push(k);if(k==='a')delete t.c;return Reflect.getOwnPropertyDescriptor(t,k)}});globalThis.R=T(()=>FI(p)+' // '+log.join())",
  "var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)}});globalThis.R=T(()=>{var n=0;for(var k in p){for(var j in p)n++}return n+' // '+log.join()})",
  "var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)}});globalThis.R=T(()=>{for(var k in p){break}return log.join()})",
  "var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});globalThis.R=T(()=>{for(var k in p){break}return log.join()})",
  "var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});globalThis.R=T(()=>{var r=[];for(var k in p){r.push(k);log.push('body '+k)}return log.join()})",
  "var p=new Proxy([1,2,3],{});globalThis.R=T(()=>FI(p))", "var p=new Proxy(function(){},{});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy(new Proxy({a:1},{}),{});globalThis.R=T(()=>FI(p))", "var p=new Proxy(Object.create(null),{});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy({a:1},{});p.b=2;globalThis.R=T(()=>FI(p))", "var p=new Proxy(Object.create({inh:1}),{});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy({a:1},{ownKeys(){return ['b','a']},getOwnPropertyDescriptor(t,k){return k==='b'?{value:1,enumerable:true,configurable:true}:Reflect.getOwnPropertyDescriptor(t,k)}});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy({},{ownKeys(){return ['b']},getOwnPropertyDescriptor(){return {value:1,enumerable:true,configurable:false}}});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy(Object.defineProperty({},'a',{value:1,configurable:false,enumerable:true}),{ownKeys(){return []}});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy(Object.defineProperty({},'a',{value:1,configurable:false,enumerable:true}),{getOwnPropertyDescriptor(){return undefined}});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return ['a','b']}});globalThis.R=T(()=>FI(p))",
  "var p=new Proxy({a:1},{get(){throw new Error('no get')},set(){throw new Error('no set')},has(){throw new Error('no has')},deleteProperty(){throw new Error('no del')}});globalThis.R=T(()=>FI(p))",
);

// ---- 5. for-in com mutação durante o laço.
const mut = [
  ["delete next", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);delete o.b}"],
  ["delete current", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);delete o[k]}"],
  ["delete visited", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);delete o.a}"],
  ["add key", "var o={a:1,b:2};for(var k in o){r.push(k);o.z=1}"],
  ["add index key", "var o={a:1,b:2};for(var k in o){r.push(k);o[5]=1}"],
  ["delete and re-add", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);delete o.b;o.b=9}"],
  ["delete and re-add current", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);delete o[k];o[k]=1}"],
  ["make nonenumerable", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);Object.defineProperty(o,'b',{enumerable:false})}"],
  ["make nonenumerable then enumerable", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);Object.defineProperty(o,'c',{enumerable:false});Object.defineProperty(o,'c',{enumerable:true})}"],
  ["replace proto", "var o=Object.create({p:1});o.a=1;for(var k in o){r.push(k);Object.setPrototypeOf(o,{q:1})}"],
  ["delete from proto", "var p={p1:1,p2:2};var o=Object.create(p);o.a=1;for(var k in o){r.push(k);delete p.p2}"],
  ["add to proto", "var p={p1:1};var o=Object.create(p);o.a=1;for(var k in o){r.push(k);p.p2=2}"],
  ["shadow proto key later", "var p={x:1,y:2};var o=Object.create(p);o.a=1;for(var k in o){r.push(k);if(k==='a')Object.defineProperty(o,'y',{value:1,enumerable:false})}"],
  ["own key added shadows proto", "var p={x:1};var o=Object.create(p);o.a=1;for(var k in o){r.push(k);o.x=2}"],
  ["delete own, proto revealed", "var p={x:'p'};var o=Object.create(p);o.x='o';o.a=1;for(var k in o){r.push(k+o[k]);delete o.x}"],
  ["array push", "var o=[1,2];for(var k in o){r.push(k);if(o.length<5)o.push(0)}"],
  ["array pop", "var o=[1,2,3,4];for(var k in o){r.push(k);o.pop()}"],
  ["array length zero", "var o=[1,2,3];for(var k in o){r.push(k);o.length=0}"],
  ["array shift", "var o=[1,2,3];for(var k in o){r.push(k);o.shift()}"],
  ["array splice", "var o=[1,2,3,4];for(var k in o){r.push(k);o.splice(1,1)}"],
  ["array hole made", "var o=[1,2,3];for(var k in o){r.push(k);delete o[2]}"],
  ["array extra prop", "var o=[1,2];o.x=1;for(var k in o){r.push(k);o.y=2;o[9]=1}"],
  ["typed array length", "var o=new Uint8Array(3);for(var k in o){r.push(k)}"],
  ["typed array extra prop", "var o=new Uint8Array(2);o.x=1;for(var k in o){r.push(k);o.y=2}"],
  ["resizable typed array shrink", "var b=new ArrayBuffer(4,{maxByteLength:8});var o=new Uint8Array(b);for(var k in o){r.push(k);if(b.resizable)b.resize(1)}"],
  ["resizable typed array grow", "var b=new ArrayBuffer(2,{maxByteLength:8});var o=new Uint8Array(b);for(var k in o){r.push(k);b.resize(6)}"],
  ["detached typed array", "var b=new ArrayBuffer(4);var o=new Uint8Array(b);for(var k in o){r.push(k);b.transfer()}"],
  ["string object extra", "var o=new String('ab');o.x=1;for(var k in o){r.push(k);o.y=1}"],
  ["arguments delete", "(function(){for(var k in arguments){r.push(k);delete arguments[1]}})(1,2,3)"],
  ["arguments add", "(function(){for(var k in arguments){r.push(k);arguments.z=1}})(1,2)"],
  ["arguments length", "(function(){for(var k in arguments){r.push(k);arguments.length=0}})(1,2)"],
  ["getter deletes", "var o={a:1,get b(){delete this.c;return 1},c:3};for(var k in o){r.push(k);o.b}"],
  ["key becomes accessor", "var o={a:1,b:2};for(var k in o){r.push(k);Object.defineProperty(o,'b',{get(){return 1},enumerable:true,configurable:true})}"],
  ["freeze midway", "var o={a:1,b:2};for(var k in o){r.push(k);Object.freeze(o)}"],
  ["preventExtensions midway", "var o={a:1,b:2};for(var k in o){r.push(k);Object.preventExtensions(o);o.z=1}"],
  ["reassign object variable", "var o={a:1,b:2};for(var k in o){r.push(k);o={z:1}}"],
  ["body reads subject", "var o={a:1,b:2};for(var k in o){r.push(k+o[k]);o[k]=o[k]*2}"],
  ["nested same object", "var o={a:1,b:2};for(var k in o){for(var j in o){r.push(k+j)}}"],
  ["nested delete", "var o={a:1,b:2,c:3};for(var k in o){for(var j in o){r.push(k+j);delete o.c}}"],
  ["nested add", "var o={a:1};for(var k in o){for(var j in o){r.push(k+j);if(r.length<4)o['n'+r.length]=1}}"],
  ["break", "var o={a:1,b:2,c:3};for(var k in o){r.push(k);if(k==='b')break}r.push('end'+k)"],
  ["continue", "var o={a:1,b:2,c:3};for(var k in o){if(k==='b')continue;r.push(k)}"],
  ["labelled", "var o={a:1,b:2};outer:for(var k in o){for(var j in o){r.push(k+j);continue outer}}"],
  ["labelled break", "var o={a:1,b:2};outer:for(var k in o){for(var j in o){r.push(k+j);break outer}}"],
  ["throw in body", "var o={a:1,b:2};try{for(var k in o){r.push(k);throw 1}}catch(e){r.push('caught'+e)}"],
  ["return in body", "var o={a:1,b:2};r.push((function(){for(var k in o){return k}})())"],
  ["generator yield", "var o={a:1,b:2};var g=(function*(){for(var k in o){yield k;delete o.b}})();r.push(...g)"],
  ["async-free closure", "var o={a:1,b:2};var fs=[];for(let k in o)fs.push(()=>k);r.push(fs.map(f=>f()).join())"],
  ["closure var", "var o={a:1,b:2};var fs=[];for(var k in o)fs.push(()=>k);r.push(fs.map(f=>f()).join())"],
  ["number keys order", "var o={b:1,2:1,a:1,1:1,[Symbol()]:1};for(var k in o){r.push(k)}"],
  ["key reassigned in body", "var o={a:1,b:2};for(var k in o){r.push(k);k='changed'}r.push(k)"],
  ["key typeof", "var o={1:1,a:2};for(var k in o){r.push(typeof k)}"],
  ["Map entries not enumerated", "var o=new Map([[1,2]]);o.x=1;for(var k in o){r.push(k)}"],
  ["delete proto key with own shadow", "var p={x:1};var o=Object.create(p);o.x=2;for(var k in o){r.push(k);delete o.x}"],
  ["proto chain three deep", "var a={a:1,s:'a'};var b=Object.create(a);b.b=1;b.s='b';var c=Object.create(b);c.c=1;c.s='c';for(var k in c)r.push(k)"],
  ["accessor in proto", "var p={get g(){return 1},set s(v){}};var o=Object.create(p);for(var k in o)r.push(k)"],
  ["class instance", "class C{x=1;m(){}static s=1;get g(){return 1}}var o=new C;for(var k in o)r.push(k);for(var k in C)r.push('C'+k)"],
  ["class extended statics", "class A{static a=1}class B extends A{static b=2}for(var k in B)r.push(k)"],
  ["function props", "function f(){}f.x=1;f.prototype.y=2;for(var k in f)r.push(k);for(var k in f.prototype)r.push('p'+k)"],
  ["null and undefined", "for(var k in null)r.push('n');for(var k in undefined)r.push('u');r.push('done'+k)"],
  ["primitives", "for(var k in 'ab')r.push(k);for(var k in 12)r.push(k);for(var k in true)r.push(k);for(var k in Symbol())r.push(k);for(var k in 5n)r.push(k)"],
  ["primitive proto props", "String.prototype.zz=1;for(var k in 'a')r.push(k);delete String.prototype.zz"],
  ["array proto props", "Array.prototype.zz=1;for(var k in [5])r.push(k);delete Array.prototype.zz"],
  ["array proto index", "Array.prototype[3]='p';for(var k in [5])r.push(k);delete Array.prototype[3]"],
  ["object proto symbol and hidden", "Object.prototype.zz=1;Object.defineProperty(Object.prototype,'hh',{value:1,configurable:true});for(var k in {a:1})r.push(k);delete Object.prototype.zz;delete Object.prototype.hh"],
  ["object proto shadowed hidden", "Object.prototype.zz=1;var o={};Object.defineProperty(o,'zz',{value:2,enumerable:false});for(var k in o)r.push(k);delete Object.prototype.zz"],
  ["proto index and string overlap", "var p={1:'p',x:'p',2:'p'};var o=Object.create(p);o[2]='o';o.y='o';o[0]='o';for(var k in o)r.push(k)"],
  ["null proto", "var o=Object.create(null);o.a=1;o[1]=1;for(var k in o)r.push(k)"],
  ["proto null chain mid", "var o=Object.setPrototypeOf({a:1},Object.setPrototypeOf({b:1},null));for(var k in o)r.push(k)"],
  ["symbol keys skipped", "var o={[Symbol.for('s')]:1,a:1};Object.prototype[Symbol.for('t')]=1;for(var k in o)r.push(typeof k+k);delete Object.prototype[Symbol.for('t')]"],
  ["large index", "var o={4294967294:1,4294967295:1,4294967296:1,1:1,'-1':1,'01':1};for(var k in o)r.push(k)"],
  ["sparse array", "var o=[1,,3];o[10]=1;for(var k in o)r.push(k)"],
  ["array length prop not enumerated", "var o=[1];for(var k in o)r.push(k);r.push('length' in o)"],
  ["getter on instance", "var o={get a(){return 1},set b(v){},c:1};for(var k in o)r.push(k)"],
  ["error object", "var o=new Error('m');o.extra=1;for(var k in o)r.push(k)"],
  ["regexp lastIndex", "var o=/x/g;o.extra=1;for(var k in o)r.push(k)"],
  ["date extra", "var o=new Date(0);o.extra=1;for(var k in o)r.push(k)"],
  ["boxed number", "var o=new Number(1);o.x=1;for(var k in o)r.push(k)"],
  ["boxed symbol", "var o=Object(Symbol());o.x=1;for(var k in o)r.push(k)"],
  ["globalThis subset", "var o={};for(var k in globalThis)if(k==='S'||k==='T'||k==='FI'||k==='R'||k==='log')r.push(k)"],
  ["arguments mapped", "(function(a,b){for(var k in arguments)r.push(k)})(1,2,3)"],
  ["arguments strict", "(function(a,b){'use strict';for(var k in arguments)r.push(k)})(1,2,3)"],
  ["arguments extra", "(function(){arguments.x=1;for(var k in arguments)r.push(k)})(1)"],
  ["arguments deleted", "(function(){delete arguments[0];for(var k in arguments)r.push(k)})(1,2)"],
  ["arguments length 0", "(function(){for(var k in arguments)r.push(k)})()"],
  ["typed arrays all kinds", "for(var C of [Int8Array,Uint8ClampedArray,Float32Array,Float64Array,BigInt64Array,Uint16Array]){var o=new C(2);for(var k in o)r.push(C.name+k)}"],
  ["typed array length zero", "var o=new Uint8Array(0);for(var k in o)r.push(k);r.push('empty')"],
  ["typed array subarray", "var o=new Uint8Array(8).subarray(2,4);for(var k in o)r.push(k)"],
  ["typed array proto", "var o=Object.create(new Uint8Array(2));o.own=1;for(var k in o)r.push(k)"],
  ["typed array symbol and canonical", "var o=new Uint8Array(1);o['-0']=1;o['1.5']=1;o['01']=1;for(var k in o)r.push(k)"],
  ["dataview and buffer", "var o=new DataView(new ArrayBuffer(2));for(var k in o)r.push(k);for(var k in new ArrayBuffer(2))r.push(k)"],
  ["string object index", "var o=new String('héy');for(var k in o)r.push(k)"],
  ["string object proto", "var o=Object.create(new String('ab'));o.x=1;for(var k in o)r.push(k)"],
  ["string object delete index", "var o=new String('ab');for(var k in o){r.push(k);r.push(delete o[k])}"],
  ["string surrogate", "for(var k in '\\ud83d\\ude00x')r.push(k)"],
  ["empty string", "for(var k in '')r.push(k);r.push('end')"],
];
for (const [name, body] of mut) {
  add(`var log=[];globalThis.R=T(()=>{var r=[];${body};return r.join()})`);
  add(`var log=[];globalThis.R=T(()=>{"use strict";var r=[];${body};return r.join()})`);
}

// ---- 6. for-in com todos os alvos.
const targets = [
  ["var", "var k in o", "k"], ["let", "let k in o", "typeof k"], ["const", "const k in o", "typeof k"], ["bare declared", "k in o", "k"],
  ["var destructuring array", "var [a,b] in o", "a+b"], ["let destructuring array", "let [a,b] in o", "a+b"], ["bare destructuring array", "[a,b] in o", "a+b"],
  ["var destructuring object", "var {length:n} in o", "n"], ["let destructuring object", "let {length} in o", "length"], ["bare destructuring object", "({length:n} in o)", "n"],
  ["destructuring index", "var {0:a} in o", "a"], ["destructuring default", "var {zz:z=9} in o", "z"], ["destructuring rest", "var {...rest} in o", "S(rest)"],
  ["array rest", "var [h,...t] in o", "h+'/'+S(t)"], ["array hole", "var [,second] in o", "second"], ["array default", "var [a,b='d'] in o", "b"],
  ["member dot", "t.p in o", "t.p"], ["member computed", "t['q'+1] in o", "t.q1"], ["member index", "t[i++] in o", "S(t)+i"], ["member of call", "g().p in o", "S(t)+log.join()"],
  ["member of this", "this.p in o", "String(this&&this.p)"], ["parenthesized", "(k) in o", "k"], ["parenthesized member", "(t.p) in o", "t.p"],
  ["setter target", "sv.v in o", "log.join()"], ["frozen target", "fr.v in o", "fr.v"], ["symbol member", "t[Symbol.for('s')] in o", "t[Symbol.for('s')]"],
  ["annexb initializer", "var k=1 in o", "k"], ["let nested pattern", "let [{length:L}] in o", "L"], ["object pattern string key", "var {['0']:z} in o", "z"],
  ["var redeclared in body", "var k in o", "k"], ["let closure", "let k in o", "k"], ["global implicit", "gk in o", "gk"],
  ["destructuring array of string", "var [c0,c1,c2] in o", "[c0,c1,c2].join()"], ["destructuring getter", "var {get:gt} in o", "typeof gt"],
  ["destructuring length of key", "var {length} in o", "length"], ["member proxy target", "px.p in o", "log.join()"],
];
const subjects = ["{ab:1,c:2}", "{}", "[5,6]", "'xyz'", "{10:1,9:1,a:1}", "Object.create({inh:1})", "null", "undefined", "42"];
const targetPrelude = "var t={},i=0,log=[],sv={set v(x){log.push('set '+x)}},fr=Object.freeze({v:0}),px=new Proxy({},{set(tt,kk,vv){log.push('pset '+kk+vv);return true}});function g(){log.push('g');return t}var gk,k;";
for (const [name, head, probe] of targets) {
  for (const subj of subjects) {
    const loop = name === "bare destructuring object" ? `for(${head.slice(1, -1)})r.push(${probe});` : `for(${head})r.push(${probe});`;
    add(`${targetPrelude}globalThis.R=T(()=>{var o=${subj};var r=[];${loop}return r.join()+" // "+log.join()})`);
    add(`${targetPrelude}globalThis.R=T(()=>{"use strict";var o=${subj};var r=[];${loop}return r.join()+" // "+log.join()})`);
  }
}
add(
  "globalThis.R=T(()=>{var r=[];for(let k in (r.push(typeof k),{}));return r.join()})",
  "globalThis.R=T(()=>{for(let k in k);})", "globalThis.R=T(()=>{for(let k in {a:1})k=2;return 1})", "globalThis.R=T(()=>{for(const k in {a:1})k=2;return 1})",
  "globalThis.R=T(()=>{'use strict';for(const k in {a:1})k=2;return 1})", "globalThis.R=T(()=>{var r=[];for(let k in {a:1,b:2}){let k2=k;r.push(()=>k2)}return r.map(f=>f()).join()})",
  "globalThis.R=T(()=>{var r=[];for(let k in {a:1}){r.push(typeof k);var k2=1}return r.join()+typeof k2})", "globalThis.R=T(()=>{for(var k in {a:1}){}return k})",
  "globalThis.R=T(()=>{for(var k in {}){}return typeof k})", "globalThis.R=T(()=>{var k='init';for(var k in {}){}return k})",
  "globalThis.R=T(()=>{var k='init';for(var k in null){}return k})", "globalThis.R=T(()=>{for(var k=7 in {}){}return k})", "globalThis.R=T(()=>{for(var k=7 in null){}return k})",
  "globalThis.R=T(()=>{'use strict';eval('for(var k=7 in {});')})", "globalThis.R=T(()=>{eval('for(let k=7 in {});')})", "globalThis.R=T(()=>{eval('for(var [a]=1 in {});')})",
  "globalThis.R=T(()=>{eval('for(let of in {});')})", "globalThis.R=T(()=>{eval('for(let in {});');return 1})", "globalThis.R=T(()=>{eval('for(async in {});');return 1})",
  "globalThis.R=T(()=>{var r=[];var async;for(async in {a:1})r.push(async);return r.join()})", "globalThis.R=T(()=>{var r=[];var let_;eval('var let;for(let in {a:1})r.push(let)');return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var k in {a:1,b:2})for(var k in {c:1})r.push(k);return r.join()})",
  "globalThis.R=T(()=>{var o={x:1};var r=[];for(o.k in {a:1,b:2})r.push(o.k);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var x in {a:1})for(var y in x)r.push(y);return r.join()})",
  "globalThis.R=T(()=>{var log=[];var o={a:1};function f(){log.push('f');return o}for(f().k in {a:1,b:2});return log.join()})",
  "globalThis.R=T(()=>{var log=[];function f(){log.push('f');return {}}function s(){log.push('s');return {a:1}}for(f().k in s());return log.join()})",
  "globalThis.R=T(()=>{var n=null;for(n.k in {});return 'no iteration, no throw'})", "globalThis.R=T(()=>{var n=null;for(n.k in {a:1});return 'x'})",
  "globalThis.R=T(()=>{var n=null;for(n[(()=>{throw new RangeError('key')})()] in {a:1});return 'x'})",
  "globalThis.R=T(()=>{var n={};for(n[(()=>{throw new RangeError('key')})()] in {});return 'x'})",
  "globalThis.R=T(()=>{var log=[];var n={};for(n[(log.push('key'),'k')] in {a:1,b:2});return log.join()})",
  "globalThis.R=T(()=>{var log=[];var n={set k(v){log.push('set '+v);throw new RangeError('set')}};for(n.k in {a:1,b:2});return log.join()})",
  "globalThis.R=T(()=>{var log=[];var n={set k(v){log.push('set '+v)}};try{for(n.k in {a:1,b:2}){log.push('body');break}}catch(e){}return log.join()})",
  "globalThis.R=T(()=>{var r=[];for(var k in {a:1})for(let k in {b:1})r.push(k);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var {length:n} in {abc:1,de:2})r.push(n);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var {x:{y}} in {a:1})r.push(y);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var [a=(r.push('def'),'z')] in {'':1})r.push(a);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var {length:n=(r.push('d'),5)} in {a:1})r.push(n);return r.join()})",
  "globalThis.R=T(()=>{var it=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function*(){yield 'X'};var r=[];try{for(var [a] in {zz:1})r.push(a)}finally{Array.prototype[Symbol.iterator]=it}return r.join()})",
  "globalThis.R=T(()=>{var it=String.prototype[Symbol.iterator];String.prototype[Symbol.iterator]=function*(){yield 'S'};var r=[];try{for(var [a] in {zz:1})r.push(a)}finally{String.prototype[Symbol.iterator]=it}return r.join()})",
  "globalThis.R=T(()=>{var log=[];var o={};Object.defineProperty(Object.prototype,'pp',{value:1,enumerable:true,configurable:true});try{for(var k in {a:1})log.push(k)}finally{delete Object.prototype.pp}return log.join()})",
  "globalThis.R=T(()=>{var log=[];Object.prototype.pp=1;try{for(var k in {a:1})log.push(k);for(var j in [])log.push('arr'+j);for(var j in 'a')log.push('s'+j)}finally{delete Object.prototype.pp}return log.join()})",
  "globalThis.R=T(()=>{var r=[];var o={a:1,b:2};for(var k in o){delete o.a;delete o.b;o.c=3;r.push(k)}return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var k in new Proxy({a:1,b:2},{ownKeys:()=>['b','a']}))r.push(k);return r.join()})",
  "globalThis.R=T(()=>{var r=[];for(var k in Object.create(new Proxy({a:1},{})))r.push(k);return r.join()})",
  "globalThis.R=T(()=>{var r=[];var o=Object.create(new Proxy({a:1,b:2},{getOwnPropertyDescriptor(t,k){r.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}}));o.a=0;for(var k in o)r.push(k);return r.join()})",
  "globalThis.R=T(()=>{var r=[];var o=Object.create(new Proxy({a:1,b:2},{ownKeys(t){r.push('ownKeys');return Reflect.ownKeys(t)}}));for(var k in o){r.push(k);break}return r.join()})",
);

// ---- Execução.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const existing = new Set();
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "eval_forin_bun.tsv") continue;
  try {
    for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
      if (!line) continue;
      try { existing.add(JSON.parse(line.split("\t")[0])); } catch (e) {}
    }
  } catch (e) {}
}
const seen = new Set();
let kept = 0;
let dropped = 0;
let dup = 0;
const lines = [];
for (const body of programs) {
  const source = PRELUDE + body;
  if (seen.has(source)) continue;
  seen.add(source);
  if (existing.has(source)) { dup++; continue; }
  let result;
  try {
    const child = spawnSync(process.execPath, [__filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000 });
    if (child.status !== 0) throw new Error((child.stderr || "filho falhou").slice(0, 200));
    result = child.stdout;
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
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stdout.write(emitFactoredLines("eval_forin", lines));
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
