// Gera tests/golden/arguments_super_bun.tsv: objeto `arguments` (mapeado e não mapeado, callee em strict,
// defineProperty em índices), `super` em object literals, getter/setter em literais, inferência de `name` em
// atribuição/destructuring/default/class fields e bind/hasInstance/toString/call/apply de borda, medidos no bun 1.4.2.
// Complementa call_edge_bun, function_proto_bun, accessor_bun e class_edge_bun: aqui a matriz de parâmetros contra
// operações em `arguments` e a matriz de contextos contra tipos de função, que os outros goldens só amostram.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-arguments-super-golden.js > tests/golden/arguments_super_bun.tsv
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
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(/^\\w{3} \\w{3} \\d\\d \\d{4} \\d\\d:\\d\\d:\\d\\d GMT[+-]\\d{4} \\(.+\\)$/.test(v)?"<date>":v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. arguments mapeado e não mapeado: parâmetros x operações x chamadas x modo.
const paramLists = ["a", "a,b", "b,a", "a,b=2", "a=1", "...a", "{a}", "a,a", "a,b,c"];
const bodies = [
  "a=9;return S([].slice.call(arguments))",
  "arguments[0]=9;return S(a)",
  "delete arguments[0];arguments[0]=5;return S(a)+S(arguments[0])",
  "arguments.length=0;return S(a)+arguments.length",
  "Object.defineProperty(arguments,'0',{writable:false});a=7;return S(arguments[0])",
  "Object.defineProperty(arguments,'0',{writable:false,value:3});a=7;return S(arguments[0])+S(a)",
  "Object.defineProperty(arguments,'0',{value:3});return S(a)",
  "Object.defineProperty(arguments,'0',{get(){return 42}});a=1;return S(arguments[0])+S(a)",
  "Object.defineProperty(arguments,'0',{enumerable:false});a=8;return S(arguments[0])+Object.keys(arguments).join()",
  "Object.freeze(arguments);a=5;return S(arguments[0])",
  "Object.seal(arguments);a=5;arguments[0]=6;return S(a)+S(arguments[0])",
  "return D(arguments,'0')",
  "return Object.getOwnPropertyNames(arguments).join()",
  "var o=arguments;a=2;return S([].map.call(o,x=>x))",
  "arguments[0]++;return S(a)",
  "a++;return S(arguments[0])",
  "[arguments[0],arguments[1]]=[7,8];return S(a)",
  "return Object.prototype.toString.call(arguments)+typeof arguments.callee",
  "Object.defineProperty(arguments,'0',{configurable:false});delete arguments[0];a=4;return S(arguments[0])",
  "Object.defineProperty(arguments,'0',{writable:false});Object.defineProperty(arguments,'0',{value:11});return S(a)",
  "var arguments;return typeof arguments+S(a)",
  "var arguments=3;return S(arguments)+S(a)",
  "function arguments(){};return typeof arguments",
  "a=undefined;return arguments.length+S(arguments[0])",
  "Object.defineProperty(arguments,'1',{value:5});return S([].slice.call(arguments))",
  "return S(Reflect.ownKeys(arguments).map(String))",
  "return (()=>S(arguments[0])+S(a))()",
  "a=3;return (()=>S(arguments[0]))()",
  "Object.defineProperty(arguments,'0',{get:undefined});return S(a)+D(arguments,'0')",
];
const callArgs = ["1,2,3", "1", "", "undefined,null"];
for (const strict of [false, true]) {
  for (const params of paramLists) {
    if (strict && (/=|\.\.\.|\{/.test(params) || params === "a,a")) continue;
    for (const body of bodies) {
      for (const args of callArgs) {
        const fnBody = (strict ? "'use strict';" : "") + body;
        if (strict && /var arguments|function arguments/.test(body)) continue;
        add(`T(()=>{function f(${params}){${fnBody}}return f(${args})})`);
      }
    }
  }
}
// Mapeamento por função expressão, método, arrow aninhada e construtor.
for (const body of bodies.slice(0, 12)) {
  add(
    `T(()=>{var f=function(a){${body}};return f(1,2)})`,
    `T(()=>{var o={m(a){${body}}};return o.m(1,2)})`,
    `T(()=>{var f=function(a){${body}};return new f(1,2)===undefined})`,
    `T(()=>{function f(a){${body}}return f.call(null,1,2)})`,
    `T(()=>{function f(a){${body}}return f.apply(null,[1,2])})`,
    `T(()=>{function f(a){${body}}return Reflect.apply(f,null,[1,2])})`,
    `T(()=>{function f(a){${body}}return f.bind(null,1)(2)})`,
  );
}

// ---- 2. callee, caller, length, iterador e toStringTag do arguments.
add(
  "T(()=>(function(){return arguments.callee===arguments.callee})())",
  "T(()=>{function f(){return arguments.callee===f}return f()})",
  "T(()=>{function f(){'use strict';return arguments.callee}return f()})",
  "T(()=>{function f(){'use strict';return D(arguments,'callee')}return f()})",
  "T(()=>{function f(){return D(arguments,'callee')}return f()})",
  "T(()=>{function f(a){return D(arguments,'callee')}return f(1)})",
  "T(()=>{function f(a=1){return D(arguments,'callee')}return f(1)})",
  "T(()=>{function f(){'use strict';var d=Object.getOwnPropertyDescriptor(arguments,'callee');return d.get===d.set&&d.get===Object.getOwnPropertyDescriptor(Function.prototype,'caller').get}return f()})",
  "T(()=>{function f(){'use strict';arguments.callee=1}return f()})",
  "T(()=>{function f(){'use strict';delete arguments.callee}return f()})",
  "T(()=>{function f(){'use strict';return Object.getOwnPropertyNames(arguments).join()}return f(1,2)})",
  "T(()=>{function f(){return Object.getOwnPropertyNames(arguments).join()}return f(1,2)})",
  "T(()=>{function f(){return Reflect.ownKeys(arguments).map(String).join()}return f(1,2)})",
  "T(()=>{function f(){return D(arguments,'length')}return f(1,2)})",
  "T(()=>{function f(){return D(arguments,Symbol.iterator)===D(Array.prototype,Symbol.iterator)}return f()})",
  "T(()=>{function f(){return arguments[Symbol.iterator]===Array.prototype.values}return f()})",
  "T(()=>{function f(){return D(arguments,Symbol.iterator)}return f()})",
  "T(()=>{function f(){return Object.getPrototypeOf(arguments)===Object.prototype}return f()})",
  "T(()=>{function f(){return arguments.constructor===Object}return f()})",
  "T(()=>{function f(){return String(arguments)}return f()})",
  "T(()=>{function f(){return arguments+''}return f()})",
  "T(()=>{function f(){return JSON.stringify(arguments)}return f(1,'a',null)})",
  "T(()=>{function f(){return Array.isArray(arguments)}return f()})",
  "T(()=>{function f(){return Array.from(arguments)}return f(1,2)})",
  "T(()=>{function f(){return [...arguments]}return f(1,2)})",
  "T(()=>{function f(){return Array.prototype.concat.call([],arguments).length}return f(1,2)})",
  "T(()=>{function f(){arguments[Symbol.toStringTag]='X';return Object.prototype.toString.call(arguments)}return f()})",
  "T(()=>{function f(){return Object.prototype.toString.call(arguments)}return f()})",
  "T(()=>{function f(){'use strict';return Object.prototype.toString.call(arguments)}return f()})",
  "T(()=>{function f(){return arguments.hasOwnProperty('length')}return f()})",
  "T(()=>{function f(){return 'callee' in arguments}return f()})",
  "T(()=>{function f(){'use strict';return 'callee' in arguments}return f()})",
  "T(()=>{function f(){return Object.isExtensible(arguments)+','+Object.isFrozen(arguments)}return f(1)})",
  "T(()=>{function f(){return Object.isFrozen(Object.freeze(arguments))}return f(1)})",
  "T(()=>{function f(){arguments.length=5;return [].slice.call(arguments).length}return f(1)})",
  "T(()=>{function f(){arguments.length=-1;return [].slice.call(arguments).length}return f(1)})",
  "T(()=>{function f(){arguments.length='2';return S([].slice.call(arguments))}return f(1,2,3)})",
  "T(()=>{function f(){arguments[5]=1;return arguments.length}return f(1)})",
  "T(()=>{function f(){arguments.x=1;return Object.keys(arguments).join()}return f(1,2)})",
  "T(()=>{function f(){return Object.keys(arguments).join()}return f(1,2)})",
  "T(()=>{function f(){var r=[];for(var k in arguments)r.push(k);return r.join()}return f(1,2)})",
  "T(()=>{function f(){var r=[];for(var v of arguments)r.push(v);return r.join()}return f(1,2)})",
  "T(()=>{function f(){return Object.entries(arguments).join('|')}return f('a','b')})",
  "T(()=>{function f(){return Object.assign({},arguments)}return f(1,2)})",
  "T(()=>{function f(){return {...arguments}}return f(1,2)})",
  "T(()=>{function f(){var {0:x,length:n}=arguments;return x+','+n}return f(5,6)})",
  "T(()=>{function f(){var [x,y]=arguments;return x+','+y}return f(5,6)})",
  "T(()=>{function f(){return Math.max.apply(null,arguments)}return f(1,9,3)})",
  "T(()=>{function f(){return Math.max(...arguments)}return f(1,9,3)})",
  "T(()=>{function f(){return g.apply(this,arguments)}function g(a,b){return a+b}return f(1,2)})",
  "T(()=>{function f(){return g(...arguments)}function g(a,b){return arguments.length+','+a+b}return f(1,2)})",
  "T(()=>{function f(a){return g.apply(null,arguments)}function g(a){a=5;return arguments[0]}return f(1)})",
  "T(()=>{var f=()=>typeof arguments;return f()})",
  "T(()=>{function f(){var g=()=>arguments[0];return g(9)}return f(1)})",
  "T(()=>{function f(){var g=()=>{arguments[0]=7};g();return arguments[0]}return f(1)})",
  "T(()=>{function f(a){var g=()=>{arguments[0]=7};g();return a}return f(1)})",
  "T(()=>{function f(){return eval('arguments.length')}return f(1,2)})",
  "T(()=>{function f(a){eval('arguments[0]=3');return a}return f(1)})",
  "T(()=>{function f(a){eval('a=4');return arguments[0]}return f(1)})",
  "T(()=>{function f(a){eval('var arguments=5');return typeof arguments+a}return f(1)})",
  "T(()=>{function f(a){return new Function('return typeof arguments')()}return f(1)})",
  "T(()=>{class C{m(){return arguments.length}static s(){return arguments.length}constructor(){this.n=arguments.length}}return new C(1,2).n+','+new C().m(1)+','+C.s(1,2,3)})",
  "T(()=>{class C{m(a){a=2;return arguments[0]}}return new C().m(1)})",
  "T(()=>{var o={get g(){return arguments.length},set g(v){this.n=arguments.length}};o.g=1;return o.g+','+o.n})",
  "T(()=>{function f(){return Object.getOwnPropertyNames(f).join()}return f()})",
  "T(()=>{function f(){'use strict';return Object.getOwnPropertyNames(f).join()}return f()})",
  "T(()=>{function f(){return f.arguments}return f(1)})",
  "T(()=>{function f(){'use strict';return f.arguments}return f(1)})",
  "T(()=>{function f(){return f.caller}return f(1)})",
  "T(()=>{function f(){return arguments.length}return f.call(1,2,3)})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,{length:3})})",
  "T(()=>{function f(){return S([].slice.call(arguments))}return f.apply(1,{length:2,0:'a',1:'b'})})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,null)})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,undefined)})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,1)})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,'ab')})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,Symbol())})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,{length:-1})})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,{length:2**32})})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,{length:3e5})})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,{get length(){throw new RangeError('l')}})})",
  "T(()=>{function f(){return S([].slice.call(arguments))}return f.apply(1,{length:1,get 0(){return 'g'}})})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,new Array(70000))})",
  "T(()=>{function f(){return arguments.length}return f.apply(1,Array.from({length:120000},(_, i)=>i))})",
  "T(()=>{function f(){return arguments[arguments.length-1]}return f.apply(1,Array.from({length:120000},(_, i)=>i))})",
  "T(()=>{function f(){return arguments.length}return f(...new Array(70000))})",
  "T(()=>{function f(){return arguments.length}return f(...Array.from({length:150000},(_,i)=>i))})",
  "T(()=>{function f(){return arguments.length}return Reflect.apply(f,1,{length:90000})})",
  "T(()=>{function f(){return arguments.length}return Reflect.construct(f,{length:90000})===undefined})",
  "T(()=>{function f(a,b){return arguments.length}return new f(...[1,2,3]) instanceof f})",
  "T(()=>{function f(a,b){this.n=arguments.length}return new f(...[1,2,3]).n})",
  "T(()=>{function f(){return arguments.length}return f.bind(null,...new Array(40000))(...new Array(40000))})",
  "T(()=>{function f(){return arguments.length}return f.bind(null,1,2,3)(4,5)})",
);

// ---- 3. super em object literals.
const superProtos = ["{x:1,m(){return 'pm'},get g(){return this.v},set g(v){this.sv=v}}", "null", "Array.prototype", "{}", "function(){}"];
const superBodies = [
  "super.x", "super['x']", "super.m()", "super.m.call({})", "super.g", "(super.g=5,this.sv)", "(super.x=5,this.x+','+Object.getPrototypeOf(this).x)",
  "(super.nope=5,D(this,'nope'))", "super.nope", "(()=>super.x)()", "(()=>super.m())()", "(function(){return typeof super})", "[super.x,super.x]",
  "super.x++", "(super.x+=2,this.x)", "(super.x??=7,this.x)", "(delete super.x)", "super[Symbol.iterator]", "typeof super.m",
  "(()=>{var k='x';return super[k]})()", "super.constructor===Object", "super.toString===Object.prototype.toString",
  "super.hasOwnProperty('x')", "Reflect.has(Object.getPrototypeOf(Object.getPrototypeOf(this)||{}),'x')", "String(super.m)",
];
for (const proto of superProtos) {
  for (const sb of superBodies) {
    if (/typeof super\)/.test(sb)) continue;
    add(`T(()=>{var p=${proto};var o={__proto__:p,v:3,t(){return ${sb}}};return o.t()})`);
    add(`T(()=>{var p=${proto};var o={v:3,t(){return ${sb}}};Object.setPrototypeOf(o,p);return o.t()})`);
    add(`T(()=>{var p=${proto};var o={v:3,get t(){return ${sb}}};Object.setPrototypeOf(o,p);return o.t})`);
  }
}
add(
  "T(()=>{var o={t(){return super.x}};o.__proto__={x:1};var q={t:o.t};q.__proto__={x:2};return q.t()+','+o.t()})",
  "T(()=>{var o={t(){return super.x}};Object.setPrototypeOf(o,{x:1});var f=o.t;return f.call({x:'this'})})",
  "T(()=>{var o={t(){return super.x}};Object.setPrototypeOf(o,{get x(){return this.y}});return o.t.call({y:'viaThis'})})",
  "T(()=>{var o={t(){return super.x}};Object.setPrototypeOf(o,{x:1});Object.setPrototypeOf(o,{x:2});return o.t()})",
  "T(()=>{var o={t(){return super.x}};return o.t()})",
  "T(()=>{var o={t(){return super.x}};Object.setPrototypeOf(o,null);return o.t()})",
  "T(()=>{var o={t(){super.x=1}};Object.setPrototypeOf(o,null);return o.t()})",
  "T(()=>{'use strict';var o={t(){super.x=1}};Object.setPrototypeOf(o,null);o.t()})",
  "T(()=>{'use strict';var o={t(){super.x=1;return Object.keys(this).join()}};return o.t()})",
  "T(()=>{'use strict';var o={t(){super.x=1;return D(this,'x')}};Object.setPrototypeOf(o,Object.freeze({x:0}));return o.t()})",
  "T(()=>{var o={t(){super.x=1;return D(this,'x')}};Object.setPrototypeOf(o,Object.freeze({x:0}));return o.t()})",
  "T(()=>{'use strict';var o={t(){super.x=1}};Object.setPrototypeOf(o,{set x(v){throw new SyntaxError('s')}});o.t()})",
  "T(()=>{var o={t(){super.x=1;return this.x}};Object.setPrototypeOf(o,{set x(v){this._x=v}});var r=o.t();return r+','+o._x})",
  "T(()=>{var o={t(){return super.x}};Object.setPrototypeOf(o,new Proxy({},{get(t,k,r){return 'px:'+String(k)+(r===o)}}));return o.t()})",
  "T(()=>{var log=[];var o={t(){super.x=1}};Object.setPrototypeOf(o,new Proxy({},{set(t,k,v,r){log.push(String(k)+v+(r===o));return true}}));o.t();return log.join()})",
  "T(()=>{var o={t(){return delete super.x}};return o.t()})",
  "T(()=>{var o={t(){return super[(()=>{throw new URIError('k')})()]}};return o.t()})",
  "T(()=>{var o={t(){return super[{toString(){return 'x'}}]}};Object.setPrototypeOf(o,{x:'ts'});return o.t()})",
  "T(()=>{var n=0;var o={t(){super[{toString(){n++;return 'x'}}]=1;return n}};Object.setPrototypeOf(o,{x:0});return o.t()})",
  "T(()=>{var n=0;var o={t(){super[{toString(){n++;return 'x'}}]+=1;return n}};Object.setPrototypeOf(o,{x:0});return o.t()})",
  "T(()=>{var o={t(){return super.m(1,2)},};Object.setPrototypeOf(o,{m(a,b){return [this===o,a,b,arguments.length].join()}});return o.t()})",
  "T(()=>{var o={t(){return super.m`a${1}b`},};Object.setPrototypeOf(o,{m(s,...v){return s.raw.join('|')+v}});return o.t()})",
  "T(()=>{var o={t(){return new super.C(1)},};Object.setPrototypeOf(o,{C:function(a){this.a=a}});return o.t().a})",
  "T(()=>{var o={t(){return new super.C},};Object.setPrototypeOf(o,{C:class{constructor(){this.k='k'}}});return o.t().k})",
  "T(()=>{var o={t(){return super.m?.()},};Object.setPrototypeOf(o,{});return o.t()})",
  "T(()=>{var o={*g(){yield super.x;yield super.x+1}};Object.setPrototypeOf(o,{x:10});return [...o.g()].join()})",
  "T(()=>{var o={async a(){return super.x}};Object.setPrototypeOf(o,{x:'as'});var r;o.a().then(v=>r=v);return typeof r})",
  "T(()=>{var o={async *ag(){yield super.x}};Object.setPrototypeOf(o,{x:'ag'});return typeof o.ag().next})",
  "T(()=>{var o={['c'+1](){return super.x}};Object.setPrototypeOf(o,{x:'cp'});return o.c1()})",
  "T(()=>{var o={[Symbol.iterator](){return super[Symbol.iterator]}};Object.setPrototypeOf(o,Array.prototype);return o[Symbol.iterator]()===Array.prototype[Symbol.iterator]})",
  "T(()=>{var o={a(){return super.a}};Object.setPrototypeOf(o,{a:'proto-a'});return o.a()})",
  "T(()=>{var o={a:function(){return typeof super.x}}})",
  "T(()=>{var o={a:()=>super.x}})",
  "T(()=>{var o={t(){return {u(){return super.x}}.u()}};Object.setPrototypeOf(o,{x:'outer'});return o.t()})",
  "T(()=>{var o={t(){var i={u(){return super.x}};Object.setPrototypeOf(i,{x:'inner'});return i.u()}};Object.setPrototypeOf(o,{x:'outer'});return o.t()})",
  "T(()=>{var o={t(){return class{static s(){return super.name}}.s()}};return o.t()})",
  "T(()=>{var o={t(){return class extends Object{static s(){return super.name}}.s()}};return o.t()})",
  "T(()=>{var o={t(){return new (class{x=super.constructor.name})().x}};return o.t()})",
  "T(()=>{var o={t(){return eval('super.x')}};Object.setPrototypeOf(o,{x:'ev'});return o.t()})",
  "T(()=>{var o={t(){return new Function('return super.x')()}};return o.t()})",
  "T(()=>{var o={t(){return (0,eval)('super.x')}};return o.t()})",
  "T(()=>{var o={get t(){return super.x},set t(v){super.x=v}};Object.setPrototypeOf(o,{x:1});o.t=5;return o.t+','+o.x+','+D(o,'x')})",
  "T(()=>{var o={t(){return super.x}};return o.t.hasOwnProperty('prototype')+','+typeof o.t})",
  "T(()=>{var o={t(){}};return (()=>{try{return new o.t()}catch(e){return e.name+': '+e.message}})()})",
  "T(()=>{var o={t(){return super.x}};return Function.prototype.toString.call(o.t)})",
  "T(()=>{var o={get t(){return super.x}};return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,'t').get)})",
  "T(()=>{var o={t(){return super.x}};var b=o.t.bind({x:'bound'});Object.setPrototypeOf(o,{x:'p'});return b()})",
);

// ---- 4. getter/setter em literais.
const keys = ["a", "'b c'", "1", "1.5", "0x10", "1e3", "[Symbol.iterator]", "['c'+1]", "[1+1]", "'__proto__'", "['__proto__']", "get", "set", "async", "static", "new", "if", "[Symbol()]", "[Symbol('d')]", "[Symbol.for('f')]", "999999999999999999999", "1n", "0.0000001"];
for (const k of keys) {
  const kk = k.startsWith("[") ? k : k;
  add(
    `T(()=>{var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);return S(k)+' '+D(o,k)+' '+d.get.name+' '+d.get.length+' '+d.set`,
    `T(()=>{var o={set ${kk}(v){}};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);return S(k)+' '+d.set.name+' '+d.set.length+' '+d.get`,
    `T(()=>{var o={get ${kk}(){return 1},set ${kk}(v){}};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);return S(k)+' '+d.get.name+','+d.set.name+' '+D(o,k)`,
    `T(()=>{var o={${kk}:1,get ${kk}(){return 2}};return D(o,Reflect.ownKeys(o)[0])`,
    `T(()=>{var o={get ${kk}(){return 2},${kk}:1};return D(o,Reflect.ownKeys(o)[0])`,
    `T(()=>{var o={set ${kk}(v){},get ${kk}(){return 2}};return D(o,Reflect.ownKeys(o)[0])`,
    `T(()=>{var o={get ${kk}(){return 1},get ${kk}(){return 2}};return Object.getOwnPropertyDescriptor(o,Reflect.ownKeys(o)[0]).get()+','+Reflect.ownKeys(o).length`,
    `T(()=>{var o={${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];return S(o[k].name)+' '+D(o,k)`,
    `T(()=>{var o={get ${kk}(){return this===o}};return o[Reflect.ownKeys(o)[0]]`,
    `T(()=>{'use strict';var o={get ${kk}(){return this}};var k=Reflect.ownKeys(o)[0];return Object.getOwnPropertyDescriptor(o,k).get.call(5)`,
    `T(()=>{var o={get ${kk}(){return typeof this}};var k=Reflect.ownKeys(o)[0];return Object.getOwnPropertyDescriptor(o,k).get.call(5)`,
    `T(()=>{var o={set ${kk}(v){this.s=v}};var k=Reflect.ownKeys(o)[0];o[k]=4;return o.s`,
    `T(()=>{var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];o[k]=4;return o[k]`,
    `T(()=>{'use strict';var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];o[k]=4`,
    `T(()=>{var o={set ${kk}(v){}};var k=Reflect.ownKeys(o)[0];return String(o[k])`,
    `T(()=>{var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];return Object.getOwnPropertyDescriptor(o,k).get.hasOwnProperty('prototype')`,
    `T(()=>{var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];var g=Object.getOwnPropertyDescriptor(o,k).get;return new g`,
    `T(()=>{var o={get ${kk}(){return 1}};var k=Reflect.ownKeys(o)[0];return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,k).get)`,
    `T(()=>{var o={set ${kk}(v){}};var k=Reflect.ownKeys(o)[0];return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,k).set)`,
    `T(()=>{var o={${kk}(){}};var k=Reflect.ownKeys(o)[0];return Function.prototype.toString.call(o[k])`,
  );
}
// As expressões acima ficam sem o fecho: fecha cada uma.
for (let i = 0; i < exprs.length; i++) if (/^T\(\(\)=>\{('use strict';)?var o=\{[^]*$/.test(exprs[i]) && !/\}\)$/.test(exprs[i])) exprs[i] += "})";
add(
  "T(()=>({get a(){return 1}}).a)", "T(()=>({get a(){return 1}}).a=5)", "T(()=>{var o={set a(v){this._=v}};return o.a=7})", "T(()=>{var o={set a(v){return 9}};return (o.a=7)})",
  "T(()=>{var o={set a(v){}};return 'a' in o&&o.a})", "T(()=>Object.keys({get a(){return 1},set b(v){},c:1}).join())",
  "T(()=>JSON.stringify({get a(){return 1},set b(v){},c:1}))", "T(()=>JSON.stringify({get a(){throw new Error('x')}}))",
  "T(()=>S({get a(){return 1}}))", "T(()=>Object.assign({},{get a(){return 1}},{set a(v){}}) )",
  "T(()=>{var o={get a(){return 1}};var c=Object.assign({},o);return D(c,'a')})", "T(()=>{var c={...{get a(){return 1}}};return D(c,'a')})",
  "T(()=>{var o={get a(){return this.b},b:2};var c=Object.create(o);c.b=3;return c.a})",
  "T(()=>{var o={set a(v){this.b=v}};var c=Object.create(o);c.a=3;return Object.keys(c).join()+o.b})",
  "T(()=>{var o={get a(){return 1}};var c=Object.create(o);c.a=3;return c.a+','+Object.keys(c).length})",
  "T(()=>{'use strict';var o={get a(){return 1}};var c=Object.create(o);c.a=3})",
  "T(()=>{var o={get __proto__(){return 1}};return D(o,'__proto__')+Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var o={__proto__:null,get x(){return 1}};return Object.getPrototypeOf(o)+','+o.x})",
  "T(()=>{var o={__proto__:{y:2},get x(){return super.y}};return o.x})",
  "T(()=>{var o={__proto__:1};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={__proto__:'s'};return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var o={__proto__:undefined};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={__proto__:null};return Object.getPrototypeOf(o)})",
  "T(()=>{var o={__proto__(){}};return typeof o.__proto__+Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var __proto__=5;var o={__proto__};return D(o,'__proto__')})",
  "T(()=>{var o={'__proto__':Array.prototype};return Array.isArray(o)+','+(Object.getPrototypeOf(o)===Array.prototype)})",
  "T(()=>{var o={['__proto__']:Array.prototype};return D(o,'__proto__').slice(0,20)})",
  "T(()=>{var o={__proto__:null,__proto__:null}})", "T(()=>eval('({__proto__:null,__proto__:null})'))", "T(()=>eval('({__proto__:null,\"__proto__\":null})'))",
  "T(()=>eval('({__proto__:null,[\"__proto__\"]:null})')===undefined)", "T(()=>eval('({__proto__:null,__proto__(){}})')===undefined)", "T(()=>eval('({__proto__:null,get __proto__(){return 1}})')===undefined)",
  "T(()=>eval('({get a(b){}})'))", "T(()=>eval('({set a(){}})'))", "T(()=>eval('({set a(b,c){}})'))", "T(()=>eval('({set a(...b){}})'))", "T(()=>eval('({set a([b]){}})')===undefined)",
  "T(()=>eval('({set a(b=1){}})')===undefined)", "T(()=>eval('({set a({b}){}})')===undefined)", "T(()=>eval('({get a(){}, a: 1, set a(v){}})')===undefined)",
  "T(()=>eval('({get(){return 1}})').get())", "T(()=>eval('({set(){return 2}})').set())", "T(()=>eval('({get:1,set:2})').get)", "T(()=>eval('({get a(){return 1}, get })'))",
  "T(()=>eval('({async get a(){}})'))", "T(()=>eval('({*get a(){}})'))", "T(()=>eval('({get *a(){}})'))", "T(()=>eval('({get async a(){}})'))",
  "T(()=>eval('({get a(){\"use strict\";return this}}).a'))", "T(()=>eval('({set a(v){\"use strict\";v=1}})')===undefined)",
  "T(()=>eval('\"use strict\";({set a(eval){}})'))", "T(()=>eval('\"use strict\";({set a(v){var v=1}})')===undefined)", "T(()=>eval('({set a(v){let v}})'))",
  "T(()=>eval('({set a(v,){}})'))", "T(()=>eval('({get a(,){}})'))", "T(()=>eval('({get [1](){return 5}})')[1])", "T(()=>eval('({get 1n(){return 5}})')[1])",
  "T(()=>eval('({get \"a\\\\u0062\"(){return 6}})').ab)", "T(()=>eval('({g\\\\u0065t a(){}})'))", "T(()=>eval('({get a(){return 1}}).a'))",
  "T(()=>{var i=0;var o={get [i++](){return 'a'},set [i++](v){}, [i++]:1};return Object.keys(o).join()+i})",
  "T(()=>{var log=[];var o={[(log.push(1),'a')]:log.push(2),get [(log.push(3),'b')](){},set [(log.push(4),'c')](v){}};return log.join()})",
  "T(()=>{var o={get a(){return 1},set a(v){}};Object.defineProperty(o,'a',{enumerable:false});return Object.keys(o).length+D(o,'a')})",
  "T(()=>{var o={get a(){return 1}};delete o.a;return 'a' in o})", "T(()=>{var o={get a(){return 1}};return Object.getOwnPropertyDescriptors(o).a.set})",
  "T(()=>{var o={get a(){return 1}};return Object.getOwnPropertyDescriptor(o,'a').get.name})", "T(()=>{var o={get a(){return 1}};return Reflect.getOwnPropertyDescriptor(o,'a').get.length})",
  "T(()=>{var o={get a(){return 1}};return o.__lookupGetter__('a')===Object.getOwnPropertyDescriptor(o,'a').get})",
  "T(()=>{var o={get a(){return 1}};return o.__lookupSetter__('a')})", "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});return D(o,'a')})",
  "T(()=>{var o={};o.__defineSetter__('a',function(v){});return D(o,'a')})", "T(()=>{var o={};o.__defineGetter__('a',1)})", "T(()=>{var o=Object.freeze({});o.__defineGetter__('a',function(){})})",
  "T(()=>{var o={a:1};o.__defineGetter__('a',function(){return 2});return D(o,'a')})", "T(()=>{var o={};o.__defineGetter__('a',function g(){});return o.__lookupGetter__('a').name})",
  "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});return o.__lookupGetter__('a').name+'|'+o.__lookupGetter__('a').length})",
);

// ---- 5. inferência de name: contextos x tipos de função.
const fnKinds = {
  fn: "function(){}", arrow: "()=>{}", cls: "class{}", clsStaticName: "class{static name(){return 1}}", clsStaticField: "class{static name='n'}",
  clsGetName: "class{static get name(){return 'g'}}", asyncFn: "async function(){}", gen: "function*(){}", asyncGen: "async function*(){}",
  asyncArrow: "async()=>{}", paren: "(function(){})", comma: "(0,function(){})", named: "function inner(){}", namedClass: "class Inner{}",
  clsExtends: "class extends Object{}", clsStaticBlock: "class{static{}}", method: "({m(){}}).m", boundFn: "(function(){}).bind()", arrowNested: "()=>()=>{}",
};
const contexts = {
  var: (f) => `var x=${f};return x`,
  let: (f) => `let x=${f};return x`,
  const: (f) => `const x=${f};return x`,
  assign: (f) => `var x;x=${f};return x`,
  assignChain: (f) => `var x,y;x=y=${f};return [x,y]`,
  member: (f) => `var o={};o.p=${f};return o.p`,
  index: (f) => `var o={};o['q']=${f};return o.q`,
  objProp: (f) => `var o={p:${f}};return o.p`,
  objStrKey: (f) => `var o={'str key':${f}};return o['str key']`,
  objNumKey: (f) => `var o={5:${f}};return o[5]`,
  objComputed: (f) => `var k='ck';var o={[k]:${f}};return o.ck`,
  objSym: (f) => `var s=Symbol('desc');var o={[s]:${f}};return o[s]`,
  objSymEmpty: (f) => `var s=Symbol();var o={[s]:${f}};return o[s]`,
  objShorthandNot: (f) => `var o={p:${f},q:(0,${f})};return [o.p,o.q]`,
  destrObjDefault: (f) => `var {a=${f}}={};return a`,
  destrObjDefaultRenamed: (f) => `var {a:b=${f}}={};return b`,
  destrArrDefault: (f) => `var [a=${f}]=[];return a`,
  destrAssignObj: (f) => `var a;({a=${f}}={});return a`,
  destrAssignArr: (f) => `var a;[a=${f}]=[];return a`,
  destrAssignMember: (f) => `var o={};[o.m=${f}]=[];return o.m`,
  paramDefault: (f) => `function g(p=${f}){return p}return g()`,
  paramDefaultDestr: (f) => `function g({p=${f}}={}){return p}return g()`,
  arrowParamDefault: (f) => `var g=(p=${f})=>p;return g()`,
  classField: (f) => `class C{fld=${f}}return new C().fld`,
  classStaticField: (f) => `class C{static sfld=${f}}return C.sfld`,
  classFieldComputed: (f) => `var k='kk';class C{[k]=${f}}return new C().kk`,
  classFieldStr: (f) => `class C{'s t'=${f}}return new C()['s t']`,
  classPrivate: (f) => `class C{#p=${f};get(){return this.#p}}return new C().get()`,
  classStaticPrivate: (f) => `class C{static #p=${f};static get(){return C.#p}}return C.get()`,
  logicalOr: (f) => `var x;x||=${f};return x`,
  logicalAnd: (f) => `var x=1;x&&=${f};return x`,
  logicalNullish: (f) => `var x;x??=${f};return x`,
  logicalMember: (f) => `var o={};o.p||=${f};return o.p`,
  condExpr: (f) => `var x=true?${f}:0;return x`,
  orExpr: (f) => `var x=0||${f};return x`,
  nullishExpr: (f) => `var x=null??${f};return x`,
  arrayElem: (f) => `var a=[${f}];return a[0]`,
  returnFn: (f) => `return ${f}`,
  callArg: (f) => `return (function(p){return p})(${f})`,
  forInit: (f) => `for(var x=${f};;)return x`,
  forOfDestr: (f) => `for(var {a=${f}} of [{}])return a`,
  catchDestr: (f) => `try{throw {}}catch({a=${f}}){return a}`,
  exportLike: (f) => `var x=(${f});return x`,
  seq: (f) => `var x=(1,${f});return x`,
  spreadObj: (f) => `var o={...{p:${f}}};return o.p`,
  templateTag: (f) => `var x=((s)=>s)\`\`;var y=${f};return y`,
  newTarget: (f) => `var x=new (${f.replace(/^\(?(class|function)/, "$1")})()===1;return 1`,
};
for (const [cn, ctx] of Object.entries(contexts)) {
  if (cn === "newTarget") continue;
  for (const [kn, kind] of Object.entries(fnKinds)) {
    let body;
    try { body = ctx(kind); } catch (e) { continue; }
    const detail = `function N(f){if(Array.isArray(f))return f.map(N).join('|');var d=Object.getOwnPropertyDescriptor(f,'name');return d?(typeof d.value==='string'?JSON.stringify(d.value):typeof d.value==='function'?'fn-name':'get/set')+' '+d.writable+d.enumerable+d.configurable:'noname'}`;
    add(`T(()=>{${detail};var r=(()=>{${body}})();return N(r)})`);
  }
}
add(
  "T(()=>{var x=function(){};return x.name})", "T(()=>{var x=function y(){};return x.name})", "T(()=>{var x=(function(){});return x.name})", "T(()=>{var x=(0,function(){});return x.name})",
  "T(()=>{var x=()=>{};var y=x;return y.name})", "T(()=>{var o={};o.p=function(){};return o.p.name})", "T(()=>{var o={p:function(){}};var q=o.p;return q.name})",
  "T(()=>{var f=function(){},g=function(){};return f.name+g.name})", "T(()=>{var a,b;a=b=function(){};return a.name+'|'+b.name})",
  "T(()=>{var {f=function(){}}={};return f.name})", "T(()=>{var [f=()=>{}]=[];return f.name})", "T(()=>{var [f=class{}]=[];return f.name})",
  "T(()=>{var {f=class{static name='x'}}={};return f.name})", "T(()=>{var {f=class{static name(){}}}={};return typeof f.name})",
  "T(()=>{class C{static m=function(){}}return C.m.name})", "T(()=>{class C{m=function(){}}return new C().m.name})", "T(()=>{class C{static #p=function(){};static g(){return C.#p.name}}return C.g()})",
  "T(()=>{class C{#p=function(){};g(){return this.#p.name}}return new C().g()})", "T(()=>{class C{#m(){};g(){return this.#m.name}}return new C().g()})",
  "T(()=>{class C{get #m(){return 1};g(){return Object.getOwnPropertyDescriptor}}return new C().g()===Object.getOwnPropertyDescriptor})",
  "T(()=>{class C{static #m(){};static g(){return C.#m.name}}return C.g()})", "T(()=>{class C{static get s(){return 1}}return Object.getOwnPropertyDescriptor(C,'s').get.name})",
  "T(()=>{class C{static set s(v){}}return Object.getOwnPropertyDescriptor(C,'s').set.name})", "T(()=>{class C{static [Symbol.iterator](){}}return C[Symbol.iterator].name})",
  "T(()=>{class C{static [Symbol()](){}}return C[Object.getOwnPropertySymbols(C)[0]].name})", "T(()=>{var s=Symbol('q');class C{[s](){}}return C.prototype[s].name})",
  "T(()=>{var s=Symbol('q');class C{get [s](){return 1}}return Object.getOwnPropertyDescriptor(C.prototype,s).get.name})",
  "T(()=>{var s=Symbol('q');var o={get [s](){return 1},set [s](v){}};var d=Object.getOwnPropertyDescriptor(o,s);return d.get.name+'|'+d.set.name})",
  "T(()=>{var s=Symbol();var o={get [s](){return 1}};return JSON.stringify(Object.getOwnPropertyDescriptor(o,s).get.name)})",
  "T(()=>{var o={f:function(){}};return Object.getOwnPropertyNames(o.f).join()})", "T(()=>{var o={f(){}};return Object.getOwnPropertyNames(o.f).join()})",
  "T(()=>{var o={f:()=>{}};return Object.getOwnPropertyNames(o.f).join()})", "T(()=>{var o={f:class{}};return Object.getOwnPropertyNames(o.f).join()})",
  "T(()=>{var o={f:class{static x=1}};return Object.getOwnPropertyNames(o.f).join()})", "T(()=>{var f=class{static name='z'};return Object.getOwnPropertyNames(f).join()})",
  "T(()=>{var f=class{static name(){}};return Object.getOwnPropertyNames(f).join()})", "T(()=>{var f=class{static get name(){return 'g'}};return Object.getOwnPropertyNames(f).join()+f.name})",
  "T(()=>{var f=class{static x=1;static y(){}};return Object.getOwnPropertyNames(f).join()})", "T(()=>{var f=class{};return Reflect.ownKeys(f).map(String).join()})",
  "T(()=>{var f=function(a,b){};return Reflect.ownKeys(f).map(String).join()})", "T(()=>{var f=function*(){};return Reflect.ownKeys(f).map(String).join()})",
  "T(()=>{var f=async function(){};return Reflect.ownKeys(f).map(String).join()})", "T(()=>{var f=async function*(){};return Reflect.ownKeys(f).map(String).join()})",
  "T(()=>{var f=()=>{};return Reflect.ownKeys(f).map(String).join()})", "T(()=>{var f=function(){}.bind();return Reflect.ownKeys(f).map(String).join()})",
  "T(()=>{var f=function(){'use strict'};return Reflect.ownKeys(f).map(String).join()})", "T(()=>{var o={m(){}};return Reflect.ownKeys(o.m).map(String).join()})",
  "T(()=>{var o={*m(){}};return Reflect.ownKeys(o.m).map(String).join()})", "T(()=>{var o={get m(){return 1}};return Reflect.ownKeys(Object.getOwnPropertyDescriptor(o,'m').get).map(String).join()})",
  "T(()=>{var f=function(){};delete f.name;return f.name+'|'+Object.getPrototypeOf(f).name})", "T(()=>{var f=function(){};Object.defineProperty(f,'name',{value:undefined});return typeof f.name+'|'+f.bind().name})",
  "T(()=>{var f=function(){};Object.defineProperty(f,'name',{value:5});return f.bind().name})", "T(()=>{var f=function(){};Object.defineProperty(f,'name',{get(){return 'gn'}});return f.bind().name})",
  "T(()=>{var f=function(){};Object.defineProperty(f,'name',{value:Symbol('s')});return f.bind().name})", "T(()=>{var f=function(){};delete f.name;return f.bind().name})",
  "T(()=>{var f=function(){};Object.defineProperty(f,'length',{value:'3'});return f.bind().length})", "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:-5});return f.bind().length})",
  "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:Infinity});return f.bind(null,1).length})", "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:-Infinity});return f.bind(null,1).length})",
  "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:2.9});return f.bind(null,1).length})", "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:NaN});return f.bind(null,1).length})",
  "T(()=>{var f=function(a,b,c){};delete f.length;return f.bind(null,1).length})", "T(()=>{var f=function(a,b,c){};delete f.length;return f.length})",
  "T(()=>{var f=function(a,b,c){};Object.defineProperty(f,'length',{value:1n});return f.bind(null,1).length})", "T(()=>{var f=function(a,b,c){};f.length=9;return f.length})",
  "T(()=>{'use strict';var f=function(a,b,c){};f.length=9})", "T(()=>{'use strict';var f=function(){};f.name='x'})", "T(()=>{var f=function(){};f.name='x';return f.name})",
  "T(()=>{var f=Object.setPrototypeOf(function(a){},null);return Function.prototype.bind.call(f,null).length})",
  "T(()=>{var f=function(){};f.__proto__=null;return Object.getPrototypeOf(f.bind())})", "T(()=>{var f=function(){};Object.setPrototypeOf(f,Array.prototype);return Object.getPrototypeOf(f.bind())===Array.prototype})",
  "T(()=>{var p=new Proxy(function(){},{getPrototypeOf(){return Array.prototype}});return Object.getPrototypeOf(Function.prototype.bind.call(p))===Array.prototype})",
  "T(()=>{var log=[];var p=new Proxy(function(a,b){},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return t[k]},has(t,k){log.push('has '+String(k));return k in t},getPrototypeOf(t){log.push('gpo');return Object.getPrototypeOf(t)}});Function.prototype.bind.call(p,null,1);return log.join()})",
);

// ---- 6. bind com new.target, length, name e instanceof/hasInstance.
const bindTargets = {
  fn: "function F(a,b){this.a=a;this.b=b;this.nt=new.target===F;}", arrow: "(a,b)=>a", cls: "class C{constructor(a,b){this.a=a;this.nt=new.target===C}}",
  clsDerived: "class D extends Array{}", method: "({m(a){return a}}).m", gen: "function*(a){}", asyncFn: "async function(a){}", native: "Date",
  nativeArr: "Array", nativeFn: "Math.max", bound: "(function(a,b,c){}).bind(null,1)", proxy: "new Proxy(function(a,b){},{})", proxyArrow: "new Proxy((a)=>a,{})",
  getterFn: "Object.getOwnPropertyDescriptor({get g(){return 1}},'g').get", symbolFn: "Symbol", bigintFn: "BigInt",
};
const bindOps = [
  (t) => `var b=(${t}).bind(null,1);return b.length+'|'+b.name+'|'+typeof b.prototype+'|'+Object.getOwnPropertyNames(b).join()`,
  (t) => `var b=(${t}).bind(null,1,2,3);return b.length+'|'+b.name`,
  (t) => `var b=(${t}).bind().bind().bind();return b.length+'|'+b.name`,
  (t) => `var b=(${t}).bind();return new b(1,2)`,
  (t) => `var b=(${t}).bind(null,'x');return new b('y') instanceof (${t})`,
  (t) => `var b=(${t}).bind();return b()`,
  (t) => `var b=(${t}).bind(5);return b(1,2)`,
  (t) => `var b=(${t}).bind();return Reflect.construct(b,[1],Array)`,
  (t) => `var b=(${t}).bind();return Object.getPrototypeOf(Reflect.construct(b,[1],Array))===Array.prototype`,
  (t) => `var b=(${t}).bind();return Object.getPrototypeOf(Reflect.construct(b,[1],function(){}.bind()))===Object.prototype`,
  (t) => `var b=(${t}).bind();return Function.prototype.toString.call(b)`,
  (t) => `var b=(${t}).bind();return Object.prototype.toString.call(b)+(typeof b)`,
  (t) => `var b=(${t}).bind();return Reflect.getPrototypeOf(b)===Reflect.getPrototypeOf(${t})`,
  (t) => `var b=(${t}).bind();return b.hasOwnProperty('caller')+','+b.hasOwnProperty('arguments')`,
  (t) => `var b=(${t}).bind();return 1 instanceof b`,
  (t) => `var b=(${t}).bind();return ({}) instanceof b`,
  (t) => `var b=(${t}).bind();return b[Symbol.hasInstance]===Function.prototype[Symbol.hasInstance]`,
  (t) => `var b=(${t}).bind();return Function.prototype[Symbol.hasInstance].call(b,{})`,
  (t) => `var t=(${t});return Function.prototype[Symbol.hasInstance].call(t,Object.create(t.prototype||null))`,
  (t) => `var t=(${t});return Function.prototype[Symbol.hasInstance].call(t,1)`,
  (t) => `var t=(${t});return D(t,'prototype')`,
  (t) => `var t=(${t});return D(t,'length')+'|'+D(t,'name')`,
  (t) => `var t=(${t});return Reflect.ownKeys(t).map(String).join()`,
  (t) => `var t=(${t});return Function.prototype.toString.call(t).slice(0,60)`,
  (t) => `var t=(${t});return t.call.length+t.apply.length+t.bind.length`,
  (t) => `var t=(${t});return Function.prototype.call.call(t,null,1,2)`,
  (t) => `var t=(${t});return Function.prototype.apply.call(t,null,[1,2])`,
  (t) => `var t=(${t});return Reflect.construct(t,[1,2]).constructor===t`,
  (t) => `var t=(${t});return Reflect.construct(t,[1,2],Object)`,
  (t) => `var t=(${t});return Reflect.construct(t,[1,2],()=>{})`,
  (t) => `var t=(${t});return Reflect.construct(t,[1,2],t)===undefined`,
  (t) => `var t=(${t});return Reflect.apply(t,undefined,[1,2])`,
  (t) => `var t=(${t});return new t(1,2)`,
  (t) => `var t=(${t});return new (t.bind(null,1))`,
  (t) => `var t=(${t});return new (t.bind(null,1).bind(null,2))`,
];
for (const [tn, target] of Object.entries(bindTargets)) {
  for (const op of bindOps) add(`T(()=>{${op(target)}})`);
}
add(
  "T(()=>{function F(){return new.target}return F.bind(null).call()})", "T(()=>{function F(){return new.target===F}return new (F.bind())})",
  "T(()=>{function F(){this.nt=new.target}var B=F.bind();return new B().nt===F})", "T(()=>{function F(){this.nt=new.target}var B=F.bind();return Reflect.construct(B,[],B).nt===F})",
  "T(()=>{function F(){this.nt=new.target}var B=F.bind();var G=function(){};return Reflect.construct(B,[],G).nt===G})",
  "T(()=>{function F(){this.nt=new.target}var B=F.bind();var G=B.bind();return new G().nt===F})", "T(()=>{function F(){this.nt=new.target}var B=F.bind();return Reflect.construct(B,[],F).nt===F})",
  "T(()=>{function F(){return new.target}var G=function(){}.bind();return Reflect.construct(F,[],G)===G})",
  "T(()=>{function F(){return new.target}return Reflect.construct(F,[],Object)===Object})", "T(()=>{function F(){return new.target}return Reflect.construct(F,[],Math.max)})",
  "T(()=>{function F(){return new.target}return Reflect.construct(F,[],class{})!==undefined})", "T(()=>{function F(){return new.target}return Reflect.construct(F,[],async function(){})})",
  "T(()=>{function F(){return new.target}return Reflect.construct(F,[],function*(){})})", "T(()=>{function F(){return new.target}return Reflect.construct(F,[],()=>{})})",
  "T(()=>{function F(){return new.target}return Reflect.construct(F,[],{m(){}}.m)})", "T(()=>{function F(){return new.target}return Reflect.construct(F,[],new Proxy(function(){},{}))!==undefined})",
  "T(()=>{function F(){return new.target}return Reflect.construct(F,[],new Proxy(()=>{},{}))})",
  "T(()=>{function F(){this.p=Object.getPrototypeOf(this)}var G=function(){};G.prototype=null;return Reflect.construct(F,[],G).p===Object.prototype})",
  "T(()=>{function F(){this.p=Object.getPrototypeOf(this)}var G=function(){};G.prototype=1;return Reflect.construct(F,[],G).p===Object.prototype})",
  "T(()=>{function F(){this.p=Object.getPrototypeOf(this)}var G=function(){};var q={};G.prototype=q;return Reflect.construct(F,[],G).p===q})",
  "T(()=>{function F(){this.p=Object.getPrototypeOf(this)}var G=function(){};Object.defineProperty(G,'prototype',{get(){return Array.prototype}});return Reflect.construct(F,[],G).p===Array.prototype})",
  "T(()=>{function F(){this.p=Object.getPrototypeOf(this)}var G=function(){};Object.defineProperty(G,'prototype',{get(){throw new EvalError('gp')}});return Reflect.construct(F,[],G).p})",
  "T(()=>{var G=new Proxy(function(){},{get(t,k){return k==='prototype'?Array.prototype:t[k]}});function F(){this.p=Object.getPrototypeOf(this)}return Reflect.construct(F,[],G).p===Array.prototype})",
  "T(()=>{class A{constructor(){this.nt=new.target}}class B extends A{}return new B().nt===B})", "T(()=>{class A{constructor(){this.nt=new.target}}return Reflect.construct(A,[],Object).nt===Object})",
  "T(()=>{class A{constructor(){this.p=Object.getPrototypeOf(this)}}var G=function(){};G.prototype=Array.prototype;return Reflect.construct(A,[],G).p===Array.prototype})",
  "T(()=>{class A extends Array{}var G=function(){};G.prototype=Object.prototype;var r=Reflect.construct(A,[3],G);return Array.isArray(r)+','+r.length+','+(Object.getPrototypeOf(r)===Object.prototype)})",
  "T(()=>{var G=function(){};G.prototype=Date.prototype;var r=Reflect.construct(Date,[0],G);return (Object.getPrototypeOf(r)===Date.prototype)+','+Date.prototype.getTime.call(r)})",
  "T(()=>{var G=function(){};G.prototype=Error.prototype;var r=Reflect.construct(Map,[],G);return Object.getPrototypeOf(r)===Error.prototype})",
  "T(()=>{var G=function(){};G.prototype=Error.prototype;var r=Reflect.construct(Map,[],G);return Map.prototype.has.call(r,1)})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Map,[],G);return Object.getPrototypeOf(r)===Map.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Array,[],G);return Object.getPrototypeOf(r)===Array.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Function,[],G);return Object.getPrototypeOf(r)===Function.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Error,['m'],G);return Object.getPrototypeOf(r)===Error.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Promise,[()=>{}],G);return Object.getPrototypeOf(r)===Promise.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(RegExp,['a'],G);return Object.getPrototypeOf(r)===RegExp.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Uint8Array,[1],G);return Object.getPrototypeOf(r)===Uint8Array.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Object,[],G);return Object.getPrototypeOf(r)===Object.prototype})",
  "T(()=>{var G=function(){};G.prototype=null;var r=Reflect.construct(Boolean,[1],G);return typeof r+(Object.getPrototypeOf(r)===Boolean.prototype)})",
  "T(()=>{var G=function(){};G.prototype=Array.prototype;var r=Reflect.construct(Object,[],G);return Object.getPrototypeOf(r)===Array.prototype})",
  "T(()=>{var G=function(){};G.prototype=Array.prototype;var r=Reflect.construct(Object,[5],G);return typeof r})",
  "T(()=>{function F(){}F.prototype={[Symbol.hasInstance](v){return true}};return 1 instanceof F})", "T(()=>{function F(){}Object.defineProperty(F,Symbol.hasInstance,{value:()=>true});return 1 instanceof F})",
  "T(()=>{var F={[Symbol.hasInstance](v){return v===1}};return 1 instanceof F})", "T(()=>{var F={[Symbol.hasInstance]:1};return 1 instanceof F})", "T(()=>{var F={[Symbol.hasInstance]:null};return 1 instanceof F})",
  "T(()=>{var F={[Symbol.hasInstance]:undefined};return 1 instanceof F})", "T(()=>{var F={};return 1 instanceof F})", "T(()=>{return 1 instanceof 1})", "T(()=>{return 1 instanceof null})",
  "T(()=>{var F={[Symbol.hasInstance](){return 'truthy'}};return (1 instanceof F)===true})", "T(()=>{var F={[Symbol.hasInstance](){return 0}};return 1 instanceof F})",
  "T(()=>{var F={get [Symbol.hasInstance](){throw new EvalError('hi')}};return 1 instanceof F})", "T(()=>{var F={[Symbol.hasInstance](v){return this===F}};return 1 instanceof F})",
  "T(()=>{function F(){}F.prototype=1;return ({}) instanceof F})", "T(()=>{function F(){}F.prototype=1;return 1 instanceof F})", "T(()=>{function F(){}F.prototype=null;return ({}) instanceof F})",
  "T(()=>{function F(){}return Object.create(F.prototype) instanceof F})", "T(()=>{function F(){}var o=Object.create(F.prototype);F.prototype={};return o instanceof F})",
  "T(()=>{function F(){}return null instanceof F})", "T(()=>{function F(){}return undefined instanceof F})", "T(()=>{function F(){}return Symbol() instanceof F})",
  "T(()=>{function F(){}return F instanceof Function})", "T(()=>{function F(){}return F instanceof Object})", "T(()=>{return Function.prototype instanceof Function})",
  "T(()=>{return Object.prototype instanceof Object})", "T(()=>{return Object.create(null) instanceof Object})", "T(()=>{return (()=>{}) instanceof Function})",
  "T(()=>{return (class{}) instanceof Function})", "T(()=>{return (async()=>{}) instanceof Function})", "T(()=>{return (function*(){}) instanceof Function})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return (function*(){}) instanceof GF})", "T(()=>{var AF=Object.getPrototypeOf(async function(){}).constructor;return (async()=>{}) instanceof AF})",
  "T(()=>{var AF=Object.getPrototypeOf(async function(){}).constructor;return AF.name+','+AF.length+','+Object.getPrototypeOf(AF)===Function})",
  "T(()=>{var AF=Object.getPrototypeOf(async function(){}).constructor;return (new AF('a','return a'))(5)===undefined})",
  "T(()=>{var GF=Object.getPrototypeOf(function*(){}).constructor;return GF('yield 1').toString()})", "T(()=>{var AF=Object.getPrototypeOf(async function(){}).constructor;return AF('a','b','return 1').toString()})",
  "T(()=>{return new Function('a','b','return a+b').toString()})", "T(()=>{return new Function('a,b','c','return a').length})", "T(()=>{return Function('a','/*','*/){').toString()})",
  "T(()=>{return new Function('a=1','return a').length})", "T(()=>{return new Function('...a','return a').length})", "T(()=>{return new Function('','').toString()})",
  "T(()=>{return new Function().name})", "T(()=>{return Function('return this')()===globalThis})", "T(()=>{return Function('\"use strict\";return this')()})",
  "T(()=>{return new Function('a','a','return a').length})", "T(()=>{return new Function('a','\"use strict\";a','return a')})", "T(()=>{return new Function('}){')})",
  "T(()=>{return new Function('a){','}')})", "T(()=>{return new Function('a','b','}{')})", "T(()=>{return new Function('//','return 1')()})", "T(()=>{return new Function('a //','return a')(7)})",
  "T(()=>{return new Function('-->','return 1').toString()})", "T(()=>{return new Function('return 1;\\n-->x')})",
);

// ---- 7. toString de funções, classes, getters, símbolos, métodos computados.
const sources = [
  "function f( a , b ) { return a }", "function  /*c*/ f /*d*/ ( ) /*e*/ { }", "function* g( ) { yield 1 }", "async  function  h( ) { }", "async function* ag( ) { }",
  "( a , b ) => a + b", "a => a", "async a => a", "async ( a ) => { }", "class  C  { }", "class C extends Object { constructor ( ) { super ( ) } }", "class C { static  m ( ) { } }",
  "class C { static { } }", "class C { x = 1 ; static y = 2 }", "class C { #p = 1 ; get ( ) { return this.#p } }", "class C { get  a ( ) { return 1 } set  a ( v ) { } }",
  "class C { * g ( ) { } async  a ( ) { } async * ag ( ) { } }", "class C { 'str' ( ) { } 1 ( ) { } [ 'c' ] ( ) { } }", "class C { static async * [ Symbol.iterator ] ( ) { } }",
  "function f ( a = ( 1 , 2 ) , { b } = { } , ... c ) { }", "function f ( ) { 'use strict' ; }", "function f ( ) { /* c */ // d\n }", "function \\u0066 ( ) { }",
  "function f ( ) { return `a${ 1 }b` }", "function f ( ) { return /re/g }", "function f ( ) { return 1 /2/ 3 }", "function f ( ) { return '\\u2028' }", "function f ( ) { return \"\\\\\" }",
  "function f ( ) { }  ", "async\nfunction f ( ) { }", "( function ( ) { } )", "( ( ) => { } )", "( class { } )", "function f ( ) { var x = class  A  { } }",
];
for (const src of sources) {
  const lit = JSON.stringify(src);
  add(
    `T(()=>{var f=(0,eval)('('+${lit}+')');return Function.prototype.toString.call(f)})`,
    `T(()=>{var f=(0,eval)('('+${lit}+')');return String(f)})`,
    `T(()=>{var f=(0,eval)('('+${lit}+')');return f.toString()===Function.prototype.toString.call(f)})`,
    `T(()=>{var f=(0,eval)('('+${lit}+')');return Function.prototype.toString.call(f.bind?f.bind():f)})`,
    `T(()=>{var f=eval('('+${lit}+')');return f+''})`,
    `T(()=>{var f=new Function('return ('+${lit}+')')();return f+''})`,
  );
}
const oneOffs = [
  "var o={ get  a ( ) { return 1 } };return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,'a').get)",
  "var o={ set  a ( v ) { } };return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,'a').set)",
  "var o={ [ 'x' + 1 ] ( ) { } };return Function.prototype.toString.call(o.x1)",
  "var o={ * [ 'g' ] ( ) { } };return Function.prototype.toString.call(o.g)",
  "var o={ async [ 'a' ] ( ) { } };return Function.prototype.toString.call(o.a)",
  "var o={ async * [ 'ag' ] ( ) { } };return Function.prototype.toString.call(o.ag)",
  "var o={ get [ 'g' ] ( ) { return 1 } };return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(o,'g').get)",
  "var o={ 'quoted key' ( ) { } };return Function.prototype.toString.call(o['quoted key'])",
  "var o={ 12 ( ) { } };return Function.prototype.toString.call(o[12])",
  "var o={ f : function ( ) { } };return Function.prototype.toString.call(o.f)",
  "var o={ f : ( ) => { } };return Function.prototype.toString.call(o.f)",
  "var s=Symbol('desc');var o={ [ s ] ( ) { } };return Function.prototype.toString.call(o[s])",
  "var s=Symbol('desc');class C{ static [ s ] ( ) { } }return Function.prototype.toString.call(C[s])",
  "class C{ static get  x ( ) { return 1 } }return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(C,'x').get)",
  "class C{ constructor ( a ) { } }return Function.prototype.toString.call(C)",
  "class C{ constructor ( a ) { } m ( ) { } }return Function.prototype.toString.call(C.prototype.m)",
  "class C{ }return Function.prototype.toString.call(C.prototype.constructor)",
  "class C extends Array{ }return Function.prototype.toString.call(C)",
  "var C=class  Named  { };return Function.prototype.toString.call(C)",
  "return Function.prototype.toString.call(Symbol)", "return Function.prototype.toString.call(Math.max)", "return Function.prototype.toString.call(function(){}.bind())",
  "return Function.prototype.toString.call(Function.prototype)", "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype,'size').get)",
  "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get)", "return Function.prototype.toString.call(Array.prototype[Symbol.iterator])",
  "return Function.prototype.toString.call(Symbol.prototype[Symbol.toPrimitive])", "return Function.prototype.toString.call(RegExp.prototype[Symbol.replace])",
  "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(RegExp,Symbol.species).get)", "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set)",
  "return Function.prototype.toString.call(new Proxy(function(){},{}))", "return Function.prototype.toString.call(new Proxy(class{},{}))", "return Function.prototype.toString.call(new Proxy({},{}))",
  "return Function.prototype.toString.call({})", "return Function.prototype.toString.call(1)", "return Function.prototype.toString.call(null)", "return Function.prototype.toString.call(Symbol())",
  "return Function.prototype.toString.call(/x/)", "return Function.prototype.toString.call([])", "return Function.prototype.toString.call(new Proxy(()=>{},{}))",
  "return Function.prototype.toString.call(Object.create(Function.prototype))", "return Function.prototype.toString.call(Object.setPrototypeOf({},Function.prototype))",
  "return Function.prototype.toString.call(function(){}) === Function.prototype.toString.call(function(){})",
  "return Function.prototype.toString.length+','+Function.prototype.toString.name+','+Function.prototype.toString.hasOwnProperty('prototype')",
  "return Function.prototype.toString.call(Function.prototype.toString)", "return Function.prototype.toString.call(Function.prototype.call)",
  "return Function.prototype.toString.call(Function.prototype[Symbol.hasInstance])", "return Function.prototype[Symbol.hasInstance].name+','+Function.prototype[Symbol.hasInstance].length+','+D(Function.prototype,Symbol.hasInstance)",
  "return D(Function.prototype,'caller')===D(Function.prototype,'arguments')", "return Object.getOwnPropertyNames(Function.prototype).join()", "return Reflect.ownKeys(Function.prototype).map(String).join()",
  "return D(Function.prototype,'length')+'|'+D(Function.prototype,'name')+'|'+typeof Function.prototype()",
  "return Function.prototype(1,2)", "return new Function.prototype", "return Function.prototype.call.call(1)", "return Function.prototype.apply.call(1)", "return Function.prototype.bind.call(1)",
  "return Function.prototype.bind.call({})", "return Function.prototype.call.call(function(){return this},1)===1", "return Function.prototype.call.call(function(){'use strict';return this},1)",
  "return Function.prototype.call.call(function(){return typeof this},1)", "return Function.prototype.call.call(function(){return typeof this},null)",
  "return Function.prototype.call.call(function(){'use strict';return typeof this},null)", "return Function.prototype.call.call(function(){return this===globalThis},undefined)",
  "return Function.prototype.call.call(function(){return this===globalThis},null)", "return Function.prototype.call.call(function(){return typeof this},'s')", "return Function.prototype.call.call(function(){return typeof this},Symbol())",
  "return Function.prototype.call.call(function(){return typeof this},1n)", "return Function.prototype.call.call(function(){return this instanceof Number},1)", "return Function.prototype.call.call(function(){return this===globalThis},void 0,1)",
  "return Function.prototype.apply.call(function(){return arguments.length},null,{length:2})", "return Function.prototype.apply.call(function(){return arguments.length},null,[,,])",
  "return Function.prototype.apply.call(function(){return 0 in arguments},null,[,,])", "return Function.prototype.apply.call(function(){return 0 in arguments},null,{length:1})",
  "return Function.prototype.apply.call(function(){return Object.keys(arguments).join()},null,{length:2})", "return Function.prototype.apply.call(Math.max,null,[1,'3',2])",
  "return Function.prototype.apply.call(Math.max,null,{length:3,0:1,2:9})", "return Function.prototype.apply.call(Math.max,null,'abc')", "return Function.prototype.apply.call(Math.max,null,new String('12'))",
  "return Function.prototype.apply.call(Math.max,null,function(a,b){})", "return Function.prototype.apply.call(Math.max,null,Object(1))", "return Function.prototype.apply.call(Math.max,null,true)",
  "return Function.prototype.apply.call(String.fromCharCode,null,new Uint8Array([72,105]))", "return Function.prototype.apply.call(String.fromCharCode,null,new Proxy([72,105],{}))",
  "return Function.prototype.apply.call(String.fromCharCode,null,(function(){return arguments})(72,105))", "return Function.prototype.apply.call(Array,null,{length:3})+''",
  "return Function.prototype.apply.call(Array,null,{length:3}).length", "return Function.prototype.apply.call(Array,null,[3]).length", "return Array.apply(null,{length:2}).hasOwnProperty(0)",
  "return Array.apply(null,Array(3)).hasOwnProperty(1)", "return Array.apply(null,[,'a']).hasOwnProperty(0)", "return Array.apply(null,new Array(5)).length", "return String.fromCharCode.apply(null,new Array(5)).length",
  "var cnt=0;var o={get length(){cnt++;return 1},get 0(){cnt+=10;return 1}};Math.max.apply(null,o);return cnt", "var cnt=[];var o=new Proxy({length:2,0:1,1:2},{get(t,k){cnt.push(String(k));return t[k]}});Math.max.apply(null,o);return cnt.join()",
  "var cnt=[];var o=new Proxy([1,2],{get(t,k){cnt.push(String(k));return t[k]},has(t,k){cnt.push('has '+String(k));return k in t}});Math.max.apply(null,o);return cnt.join()",
  "var cnt=[];var o=new Proxy([1,2],{get(t,k){cnt.push(String(k));return t[k]}});Math.max(...o);return cnt.join()",
  "var cnt=[];var o=new Proxy({length:2},{get(t,k){cnt.push(String(k));return t[k]}});try{Math.max(...o)}catch(e){cnt.push(e.name)}return cnt.join()",
];
for (const body of oneOffs) add(`T(()=>{${body}})`);

// ---- Execução: um bun filho novo por programa (a ordem de reificação das tabelas estáticas depende do processo).
const baseSources = [];
baseSources.push(...knownPrograms("arguments_super_bun.tsv", ["call_edge_bun.tsv", "function_proto_bun.tsv", "accessor_bun.tsv", "class_edge_bun.tsv", "function_source_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
const PRELOAD = writeResultPreload();
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
    if (child.status !== 0) throw new Error((child.stderr || "filho falhou").slice(0, 200));
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    dropped++;
    if (process.env.GEN_VERBOSE) process.stderr.write("descartado: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
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
process.stdout.write(emitFactored("arguments_super", rows));
