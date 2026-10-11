// Gera tests/golden/accessor_bun.tsv: propriedades e acessores, medido no bun 1.4.2 com
// `require("node:vm").runInThisContext(src)` capturando `globalThis.R`, sem APIs de host.
// Cobre defineProperty/defineProperties/getOwnPropertyDescriptors, getters e setters em classes e literais,
// `super.prop` em objetos e classes, `__proto__` literal, `__defineGetter__`/`__lookupGetter__`,
// freeze/seal/preventExtensions em arrays e typed arrays, propriedades indexadas contra nomeadas, ordem de chaves
// inteiras, `Symbol.toPrimitive`, herança de acessores e escrita em readonly no modo estrito contra o sloppy.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-accessor-golden.js > tests/golden/accessor_bun.tsv
const vm = require("node:vm");
const { emitRow } = require("./golden-prelude.js");

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function L(c){return T(()=>Function(c)())}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. defineProperty / defineProperties / getOwnPropertyDescriptors.
const descs = ["{}", "{value:1}", "{get(){return 1}}", "{set(v){}}", "{get(){return 1},set(v){}}", "{value:1,writable:true,enumerable:true,configurable:true}", "{get:undefined}", "{get:1}", "{get(){},value:1}", "{enumerable:true}"];
const priors = ["", "o.p=0;", "Object.defineProperty(o,'p',{value:0});", "Object.defineProperty(o,'p',{get(){return 0},configurable:true});"];
for (const prior of priors) for (const d of descs) add(`T(()=>{var o={};${prior}Object.defineProperty(o,'p',${d});return D(o,'p')})`);
add(
  "T(()=>S(Object.getOwnPropertyDescriptors({a:1,get b(){return 2},set c(v){}})))",
  "T(()=>Object.keys(Object.getOwnPropertyDescriptors({a:1,[Symbol.for('s')]:2,2:3,1:4})).length+','+Reflect.ownKeys(Object.getOwnPropertyDescriptors({b:1,a:2,1:3})).join())",
  "T(()=>S(Object.getOwnPropertyDescriptors([1,2])))", "T(()=>S(Object.getOwnPropertyDescriptors('ab')))", "T(()=>S(Object.getOwnPropertyDescriptors(1)))",
  "T(()=>S(Object.getOwnPropertyDescriptors(null)))", "T(()=>S(Object.getOwnPropertyDescriptors(function f(a){})))", "T(()=>S(Object.getOwnPropertyDescriptors(new Uint8Array(2))))",
  "T(()=>S(Object.getOwnPropertyDescriptors(Object.freeze({a:1}))))", "T(()=>S(Object.getOwnPropertyDescriptors(class{static get x(){return 1}})))",
  "T(()=>{var o=Object.defineProperties({},{a:{value:1,enumerable:true},b:{get(){return 2}}});return D(o,'a')+'|'+D(o,'b')})",
  "T(()=>{var o=Object.defineProperties({},{a:{value:1},b:1})})", "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:1})}catch(e){}return S(Object.getOwnPropertyNames(o))})",
  "T(()=>S(Object.defineProperties({},{})))", "T(()=>Object.defineProperties({},undefined))", "T(()=>Object.defineProperties({},null))", "T(()=>Object.defineProperties({},1))",
  "T(()=>Object.defineProperties({},'ab'))", "T(()=>S(Object.defineProperties({},{get a(){return {value:7,enumerable:true}}})))",
  "T(()=>{var p=Object.create({inh:{value:1}});p.own={value:2,enumerable:true};return S(Object.defineProperties({},p))})",
  "T(()=>{var p={};Object.defineProperty(p,'hid',{value:{value:1},enumerable:false});p.vis={value:2,enumerable:true};return S(Object.defineProperties({},p))})",
  "T(()=>{var p={[Symbol.for('s')]:{value:3,enumerable:true}};var o=Object.defineProperties({},p);return D(o,Symbol.for('s'))})",
  "T(()=>S(Object.getOwnPropertyDescriptor({get a(){return 1}},'a').get.name))", "T(()=>S(Object.getOwnPropertyDescriptor({set a(v){}},'a').set.name))",
  "T(()=>S(Object.getOwnPropertyDescriptor(class{get a(){return 1}}.prototype,'a').get.name))", "T(()=>S(Object.getOwnPropertyDescriptor({get [Symbol.iterator](){return 1}},Symbol.iterator).get.name))",
  "T(()=>S(Object.getOwnPropertyDescriptor({get [Symbol('d')](){return 1}},Object.getOwnPropertySymbols({get [Symbol('d')](){return 1}})[0])))",
  "T(()=>Object.getOwnPropertyDescriptor({get a(){return 1}},'a').get.length+','+Object.getOwnPropertyDescriptor({set a(v){}},'a').set.length)",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get:function(){return this===o}});return o.a})", "T(()=>{var o={};Object.defineProperty(o,'a',{get(){return typeof this}});return 'x'.constructor.prototype.__lookupGetter__?o.a:0})",
  "T(()=>{var d={value:1};Object.defineProperty({},'a',d);return S(d)})", "T(()=>{var o=Object.defineProperty({},'a',{value:1});return Object.getOwnPropertyDescriptor(o,'a')!==Object.getOwnPropertyDescriptor(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get(){return 1},configurable:true});Object.defineProperty(o,'a',{value:2});return D(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1,configurable:true,writable:true});Object.defineProperty(o,'a',{get(){return 2}});return D(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1,configurable:true,enumerable:true});Object.defineProperty(o,'a',{set(v){}});return D(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1,writable:true});Object.defineProperty(o,'a',{value:2});return D(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1,writable:true});Object.defineProperty(o,'a',{writable:false});Object.defineProperty(o,'a',{writable:true})})",
  "T(()=>{var g=function(){};var o={};Object.defineProperty(o,'a',{get:g});Object.defineProperty(o,'a',{get:g});return 'ok'})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get(){}});Object.defineProperty(o,'a',{get(){}})})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get(){},enumerable:true});Object.defineProperty(o,'a',{enumerable:false})})",
);

// ---- 2. getters e setters em literais e classes.
add(
  "T(()=>{var o={get a(){return 1}};o.a=2;return o.a})", "T(()=>{var o={set a(v){this._=v}};o.a=3;return S(o)+o.a})", "T(()=>{var o={get a(){return 1},set a(v){this._=v}};o.a=3;return S(o)})",
  "T(()=>{var o={get a(){return 1},a:2};return D(o,'a')})", "T(()=>{var o={a:2,get a(){return 1}};return D(o,'a')})", "T(()=>{var o={set a(v){},a:2};return D(o,'a')})", "T(()=>{var o={get a(){return 1},get a(){return 2}};return o.a})",
  "T(()=>{var o={get a(){return 1},set a(v){},get a(){return 3}};return D(o,'a')})", "T(()=>{var o={get 1(){return 'i'}};return o[1]+D(o,'1')})", "T(()=>{var o={get 'x y'(){return 1}};return o['x y']})",
  "T(()=>{var o={get [1+1](){return 'c'}};return o[2]})", "T(()=>{var o={get 0.5(){return 'f'}};return Object.keys(o).join()})", "T(()=>{var o={get 1n(){return 'b'}};return Object.keys(o).join()})",
  "T(()=>{var o={get get(){return 'g'},set set(v){},get set(){return 's'}};return o.get+o.set})", "T(()=>{var o={get(){return 1},set(){return 2}};return o.get()+o.set()})", "T(()=>{var o={get:1,set:2};return o.get+o.set})",
  "T(()=>{var o={get a(){return this.b},b:5};return o.a})", "T(()=>{var o={get a(){return this.b},b:5};return Object.create(o,{b:{value:6}}).a})", "T(()=>{var o={get a(){return arguments.length}};return o.a})",
  "T(()=>{var o={set a(v){return 9}};return (o.a=1)})", "T(()=>{var o={set a(v){return 9}};return Reflect.set(o,'a',1)})", "T(()=>{var o={set a([x,y]){this.s=x+y}};o.a=[1,2];return o.s})", "T(()=>{var o={set a({x}){this.s=x}};o.a={x:4};return o.s})",
  "T(()=>{var o={set a(v=5){this.s=v}};o.a=undefined;return o.s})", "T(()=>{var o={set a(...r){}}})", "T(()=>Function('return {set a(){}}')())", "T(()=>Function('return {get a(x){}}')())", "T(()=>Function('return {set a(x,y){}}')())",
  "T(()=>{var o={get a(){return 1}};return typeof Object.getOwnPropertyDescriptor(o,'a').get})", "T(()=>{var o={get a(){return 1}};return new o.constructor.getOwnPropertyDescriptor===0})",
  "T(()=>{var o={get a(){return 1}};var g=Object.getOwnPropertyDescriptor(o,'a').get;return new g})", "T(()=>{var o={get a(){return 1}};var g=Object.getOwnPropertyDescriptor(o,'a').get;return g.hasOwnProperty('prototype')})",
  "T(()=>{class A{get x(){return 1}}return D(A.prototype,'x')})", "T(()=>{class A{set x(v){}}return D(A.prototype,'x')})", "T(()=>{class A{static get x(){return 1}}return D(A,'x')+A.x})", "T(()=>{class A{static set x(v){A._x=v}}A.x=3;return A._x})",
  "T(()=>{class A{get x(){return 1}}var a=new A;a.x=2;return a.x})", "T(()=>{class A{get x(){return 1}}var a=new A;'use strict';a.x=2;return a.x})", "T(()=>{class A{get x(){return 1}}return L('class B{get x(){return 1}};var b=new B;b.x=2;return b.x')})",
  "T(()=>{class A{get x(){return 1}}var a=new A;a.x=2})", "T(()=>{class A{get x(){return 1}set x(v){this._x=v}}var a=new A;a.x=2;return S(a)+a.x})", "T(()=>{class A{get x(){return 1}}class B extends A{set x(v){this._=v}}var b=new B;b.x=1;return S(b)+b.x})",
  "T(()=>{class A{get x(){return 1}set x(v){this._=v}}class B extends A{get x(){return 2}}var b=new B;b.x=7;return S(b)+b.x})", "T(()=>{class A{get ['a'+'b'](){return 1}}return new A().ab})", "T(()=>{class A{get 1(){return 'one'}}return new A()[1]})",
  "T(()=>{class A{static get [Symbol.species](){return 3}}return A[Symbol.species]})", "T(()=>{class A{get x(){return 1}}return Object.keys(A.prototype).length+','+Object.getOwnPropertyNames(A.prototype).join()})",
  "T(()=>{class A{get x(){return 1};get x(){return 2}}return new A().x})", "T(()=>{class A{get x(){return 1};static get x(){return 2}}return new A().x+A.x})", "T(()=>{class A{get constructor(){return 1}}})",
  "T(()=>{class A{static get prototype(){return 1}}})", "T(()=>{class A{static get name(){return 'N'}}return A.name})", "T(()=>{class A{static get length(){return 7}}return A.length})", "T(()=>{class A{#p=1;get v(){return this.#p}set v(x){this.#p=x}}var a=new A;a.v=5;return a.v})",
  "T(()=>{class A{get #p(){return 1}test(){return this.#p}}return new A().test()})", "T(()=>{class A{get #p(){return 1}test(){this.#p=2}}return new A().test()})", "T(()=>{class A{set #p(v){}test(){return this.#p}}return new A().test()})",
  "T(()=>{class A{static get #p(){return 'sp'}static t(){return A.#p}}return A.t()})", "T(()=>{class A{accessor=1}return S(new A)})",
);

// ---- 3. super.prop em objetos e classes.
add(
  "T(()=>{var p={x:1};var o={__proto__:p,m(){return super.x}};return o.m()})", "T(()=>{var p={get x(){return this.y}};var o={__proto__:p,y:2,m(){return super.x}};return o.m()})",
  "T(()=>{var p={set x(v){this.s=v}};var o={__proto__:p,m(){super.x=5;return S(this)}};return o.m()})", "T(()=>{var p={};var o={__proto__:p,m(){super.x=5;return S(this)+S(p)}};return o.m()})",
  "T(()=>{var p={};Object.defineProperty(p,'x',{value:1});var o={__proto__:p,m(){super.x=5;return D(this,'x')}};return o.m()})", "T(()=>{var p={get x(){return 1}};var o={__proto__:p,m(){super.x=5}};return o.m()})",
  "T(()=>{var o={m(){return super.toString===Object.prototype.toString}};return o.m()})", "T(()=>{var o={m(){return super.nope}};return o.m()})", "T(()=>{var o={__proto__:null,m(){return super.x}};return o.m()})",
  "T(()=>{var o={m(){super.x=1;return S(this)}};return o.m()})", "T(()=>{var o={m(){return delete super.x}};return o.m()})", "T(()=>{var o={m(){return super[(()=>'toString')()]===Object.prototype.toString}};return o.m()})",
  "T(()=>{var o={m(){return (()=>super.toString===Object.prototype.toString)()}};return o.m()})", "T(()=>{var p={x:1};var o={__proto__:p,m(){return super.x}};var q={m:o.m};return q.m()})", "T(()=>{var p={x:1};var o={__proto__:p,m(){return super.x}};Object.setPrototypeOf(o,{x:2});return o.m()})",
  "T(()=>{var p={x:1};var o={__proto__:p,m(){return super.x}};var m=o.m;return m()})", "T(()=>{var p={get x(){return this}};var o={__proto__:p,m(){return super.x===this}};return o.m()})", "T(()=>{var p={m(){return 'p'}};var o={__proto__:p,m(){return super.m()+'o'}};return o.m()})",
  "T(()=>{var o={f:function(){return super.x}}})", "T(()=>Function('return {f:function(){return super.x}}')())", "T(()=>Function('return {f(){return super()}}')())", "T(()=>Function('return {f:()=>super.x}')())",
  "T(()=>{var o={get a(){return super.b},__proto__:{b:3}};return o.a})", "T(()=>{var o={set a(v){super.c=v},__proto__:{set c(v){this.got=v}}};o.a=4;return S(o)})", "T(()=>{class A{get x(){return 'A'}}class B extends A{get x(){return super.x+'B'}}return new B().x})",
  "T(()=>{class A{set x(v){this._=v}}class B extends A{set x(v){super.x=v*2}}var b=new B;b.x=2;return S(b)})", "T(()=>{class A{static s(){return 'sA'}}class B extends A{static s(){return super.s()+'B'}}return B.s()})", "T(()=>{class A{}class B extends A{m(){super.x=1;return S(this)}}return new B().m()})",
  "T(()=>{class A{}A.prototype.x=1;class B extends A{m(){return super.x}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){super.x=1;return A.prototype.hasOwnProperty('x')}}return new B().m()})", "T(()=>{class A{get x(){return this.v}}class B extends A{constructor(){super();this.v=3}m(){return super.x}}return new B().m()})",
  "T(()=>{class A{}class B extends A{static m(){return super.name}}return B.m()})", "T(()=>{class A{constructor(){this.k=1}}class B extends A{constructor(){super();super.k=2}}return new B().k})", "T(()=>{class A{}class B extends A{constructor(){super.x=1;super()}}new B})",
  "T(()=>{class A{}class B extends A{constructor(){return super.x}}new B})", "T(()=>{class A{}class B extends A{constructor(){var f=()=>super.x;super();return f()}}return typeof new B})", "T(()=>{class A{}class B extends A{f=super.constructor.name}return new B().f})",
  "T(()=>{class A{get x(){return 1}}class B extends A{static f=super.name}return B.f})", "T(()=>{class A{}class B extends A{['m'](){return super.constructor===A}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){return super.constructor===A}}return new B().m()})",
  "T(()=>{class A{}class B extends A{m(){return super[Symbol.toPrimitive]}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){super.x++;return S(this)}}return new B().m()})", "T(()=>{class A{}A.prototype.x=1;class B extends A{m(){super.x++;return S(this)+A.prototype.x}}return new B().m()})",
  "T(()=>{class A{}A.prototype.x=1;class B extends A{m(){super.x+=5;return S(this)}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){return super.x??'d'}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){super.x??=5;return S(this)}}return new B().m()})",
  "T(()=>{class A{}class B extends A{m(){return super.m?.()}}return new B().m()})", "T(()=>{class A{}class B extends A{m(){var {x}=super;return 1}}})",
);

// ---- 4. __proto__ literal.
add(
  "T(()=>{var o={__proto__:null};return Object.getPrototypeOf(o)===null})", "T(()=>{var p={};var o={__proto__:p};return Object.getPrototypeOf(o)===p})", "T(()=>{var o={__proto__:1};return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var o={__proto__:'s'};return Object.getPrototypeOf(o)===Object.prototype+','+S(o)})", "T(()=>{var o={__proto__:undefined};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={'__proto__':null};return Object.getPrototypeOf(o)===null})",
  "T(()=>{var o={['__proto__']:null};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={['__proto__']:null};return D(o,'__proto__')})", "T(()=>{var __proto__=null;var o={__proto__};return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var __proto__=null;var o={__proto__};return D(o,'__proto__')})", "T(()=>{var o={__proto__(){}};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={__proto__(){}};return typeof D(o,'__proto__')})",
  "T(()=>{var o={get __proto__(){return 1}};return o.__proto__})", "T(()=>{var o={set __proto__(v){this.s=v}};o.__proto__=3;return S(o)})", "T(()=>Function('return {__proto__:1,__proto__:2}')())", "T(()=>Function('return {__proto__:1,\"__proto__\":2}')())",
  "T(()=>Function('return {__proto__:1,[\"__proto__\"]:2}')())", "T(()=>Function('return {__proto__:1,__proto__(){}}')())", "T(()=>Function('return {__proto__:1,get __proto__(){}}')())", "T(()=>Function('var __proto__=1;return {__proto__:1,__proto__}')())",
  "T(()=>{var o={__proto__:{a:1}};return o.a+','+S(Object.keys(o))})", "T(()=>{var o={__proto__:{a:1},b:2};return S(o)})", "T(()=>{var o={a:1,__proto__:{b:2}};return o.b+S(o)})", "T(()=>{var o=JSON.parse('{\"__proto__\":1}');return D(o,'__proto__')})",
  "T(()=>{var o=JSON.parse('{\"__proto__\":{\"a\":1}}');return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={};o.__proto__={a:1};return o.a})", "T(()=>{var o={};o.__proto__=1;return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={};o.__proto__=null;return Object.getPrototypeOf(o)})",
  "T(()=>{var o=Object.create(null);o.__proto__=1;return D(o,'__proto__')})", "T(()=>{var o=Object.create(null);o.__proto__={a:1};return Object.getPrototypeOf(o)})", "T(()=>{var d=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__');return typeof d.get+typeof d.set+d.enumerable+d.configurable})",
  "T(()=>{var d=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__');return d.get.name+','+d.set.name+d.get.length+d.set.length})", "T(()=>{var g=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get;return g.call(1)===Number.prototype})",
  "T(()=>{var g=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get;return g.call(null)})", "T(()=>{var s=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set;return s.call(undefined,{})})", "T(()=>{var s=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set;return s.call(1,{})})",
  "T(()=>{var s=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set;var o={};return s.call(o,1)===undefined})", "T(()=>{var o={};o.__proto__=o})", "T(()=>{var a={},b={__proto__:a};a.__proto__=b})", "T(()=>{var o=Object.preventExtensions({});o.__proto__={}})",
  "T(()=>{var o=Object.preventExtensions({});o.__proto__=Object.prototype;return 'same'})", "T(()=>{var o=Object.freeze({});o.__proto__={}})", "T(()=>L('var o=Object.freeze({});o.__proto__={};return Object.getPrototypeOf(o)===Object.prototype'))",
  "T(()=>{var o={__proto__:Array.prototype};return Array.isArray(o)+','+(o instanceof Array)})", "T(()=>{var o={__proto__:Function.prototype};return typeof o})", "T(()=>{var o={__proto__:new Proxy({},{get(t,k){return 'px'+String(k)}})};return o.zz})",
  "T(()=>{var f=function(){};var o={__proto__:f.prototype};return o instanceof f})", "T(()=>{var o={__proto__:Object.create(null)};return o.toString})", "T(()=>Object.prototype.hasOwnProperty('__proto__')+','+('__proto__' in {})+','+Object.keys(Object.prototype).length)",
);

// ---- 5. __defineGetter__ / __defineSetter__ / __lookupGetter__ / __lookupSetter__.
add(
  "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});return D(o,'a')})", "T(()=>{var o={};o.__defineSetter__('a',function(v){this._=v});return D(o,'a')})", "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});o.__defineSetter__('a',function(){});return D(o,'a')})",
  "T(()=>{var o={};o.__defineGetter__('a',1)})", "T(()=>{var o={};o.__defineGetter__('a')})", "T(()=>{var o={};o.__defineGetter__('a',undefined)})", "T(()=>{var o={};o.__defineGetter__('a',{})})", "T(()=>{var o={};o.__defineGetter__('a',class{})})",
  "T(()=>{var o=Object.freeze({});o.__defineGetter__('a',function(){})})", "T(()=>{var o={a:1};Object.defineProperty(o,'a',{configurable:false});o.__defineGetter__('a',function(){})})", "T(()=>{var o={a:1};o.__defineGetter__('a',function(){return 2});return D(o,'a')})",
  "T(()=>{var o={a:1};Object.defineProperty(o,'a',{enumerable:false,writable:true,configurable:true});o.__defineGetter__('a',function(){return 2});return D(o,'a')})", "T(()=>Object.prototype.__defineGetter__.call(null,'a',function(){}))", "T(()=>Object.prototype.__defineGetter__.call(undefined,'a',function(){}))",
  "T(()=>{var n=Object.prototype.__defineGetter__.call(1,'a',function(){});return n})", "T(()=>{var o={};o.__defineGetter__({toString(){return 'k'}},function(){return 1});return o.k})", "T(()=>{var o={};o.__defineGetter__(Symbol.for('s'),function(){return 1});return o[Symbol.for('s')]})",
  "T(()=>{var o={};o.__defineGetter__(1,function(){return 'i'});return o[1]+Object.keys(o).join()})", "T(()=>{var o=[];o.__defineGetter__(2,function(){return 'i'});return o.length+','+o[2]})", "T(()=>{var o={};return o.__defineGetter__('a',function(){})})",
  "T(()=>{var o={};o.__defineGetter__('a',function(){return this===o});return o.a})", "T(()=>Object.prototype.__defineGetter__.length+Object.prototype.__defineSetter__.length+Object.prototype.__lookupGetter__.length+Object.prototype.__lookupSetter__.length)",
  "T(()=>Object.prototype.__defineGetter__.name+Object.prototype.__lookupSetter__.name)", "T(()=>D(Object.prototype,'__defineGetter__')+'|'+D(Object.prototype,'__lookupGetter__'))", "T(()=>{var o={get a(){return 1}};return o.__lookupGetter__('a')===Object.getOwnPropertyDescriptor(o,'a').get})",
  "T(()=>{var o={set a(v){}};return o.__lookupSetter__('a')===Object.getOwnPropertyDescriptor(o,'a').set})", "T(()=>{var o={get a(){return 1}};return o.__lookupSetter__('a')})", "T(()=>{var o={a:1};return o.__lookupGetter__('a')})", "T(()=>{var o={};return o.__lookupGetter__('a')})",
  "T(()=>{var p={get a(){return 1}};var o=Object.create(p);return o.__lookupGetter__('a')===Object.getOwnPropertyDescriptor(p,'a').get})", "T(()=>{var p={a:1};var o=Object.create(p);Object.defineProperty(o,'a',{get(){return 2}});return typeof o.__lookupGetter__('a')})",
  "T(()=>{var p={get a(){return 1}};var o=Object.create(p);o.a=5;return typeof o.__lookupGetter__('a')+S(o)})", "T(()=>{var p={get a(){return 1}};var o=Object.create(p);Object.defineProperty(o,'a',{value:3});return o.__lookupGetter__('a')})",
  "T(()=>Object.prototype.__lookupGetter__.call(null,'a'))", "T(()=>Object.prototype.__lookupGetter__.call(undefined,'a'))", "T(()=>Object.prototype.__lookupGetter__.call(1,'toString'))", "T(()=>Object.prototype.__lookupGetter__.call('s','length'))",
  "T(()=>{var o={get a(){return 1}};return o.__lookupGetter__({toString(){return 'a'}})===Object.getOwnPropertyDescriptor(o,'a').get})", "T(()=>{var o={};return o.__lookupGetter__({toString(){throw new RangeError('k')}})})", "T(()=>{var o={get [Symbol.for('s')](){return 1}};return typeof o.__lookupGetter__(Symbol.for('s'))})",
  "T(()=>{return typeof Object.prototype.__lookupGetter__.call(Object.prototype,'__proto__')+typeof Object.prototype.__lookupSetter__.call({},'__proto__')})", "T(()=>{var o=new Proxy({},{getOwnPropertyDescriptor(t,k){return {get:function(){},configurable:true}}});return typeof o.__lookupGetter__('zz')})",
  "T(()=>{var o={};o.__defineSetter__('a',function(v){this._=v});o.a=4;return S(o)})", "T(()=>{var o={};o.__defineSetter__('a',1)})", "T(()=>{var o={};o.__defineSetter__('a',function(){});return D(o,'a')==='g=undefined/s=fn e=true c=true'})",
  "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});Object.defineProperty(o,'a',{enumerable:false});o.__defineSetter__('a',function(){});return D(o,'a')})",
);

// ---- 6. freeze / seal / preventExtensions em arrays e typed arrays.
const mut = [["freeze", "Object.freeze"], ["seal", "Object.seal"], ["pe", "Object.preventExtensions"]];
for (const [n, f] of mut) {
  add(
    `T(()=>{var a=${f}([1,2,3]);a.push(4)})`, `T(()=>{var a=${f}([1,2,3]);a.pop();return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.shift();return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.unshift(0);return S(a)})`,
    `T(()=>{var a=${f}([1,2,3]);a[0]=9;return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a[5]=9;return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.length=1;return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.length=5;return S(a)})`,
    `T(()=>{var a=${f}([1,2,3]);a.sort();return S(a)})`, `T(()=>{var a=${f}([3,1,2]);a.sort();return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.reverse();return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.splice(0,1);return S(a)})`,
    `T(()=>{var a=${f}([1,2,3]);a.fill(0);return S(a)})`, `T(()=>{var a=${f}([1,2,3]);a.copyWithin(0,1);return S(a)})`, `T(()=>{var a=${f}([]);a.push();return S(a)})`, `T(()=>{var a=${f}([]);a.pop();return S(a)})`,
    `T(()=>{var a=${f}([1,2,3]);delete a[0];return S(a)})`, `T(()=>{var a=${f}([1,2,3]);return Object.isFrozen(a)+','+Object.isSealed(a)+','+Object.isExtensible(a)})`, `T(()=>{var a=${f}([,1]);return Object.isFrozen(a)+','+Object.isSealed(a)+','+Object.isExtensible(a)})`,
    `T(()=>{var a=${f}([]);return Object.isFrozen(a)+','+Object.isSealed(a)+','+Object.isExtensible(a)})`, `T(()=>{var a=${f}([1]);return D(a,0)+'|'+D(a,'length')})`, `T(()=>{var a=${f}([1,2]);return a.concat([3]).length+','+Object.isFrozen(a.slice())})`,
    `T(()=>{var a=${f}([1,2]);a.x=1;return S(a)+a.x})`, `T(()=>{var a=${f}([1,2]);return L('var a=arguments[0];a[0]=9;a.x=1;return a[0]+","+a.x')})`, `T(()=>{var a=${f}([1,2,3]);return a.map(x=>x*2).length})`,
    `T(()=>{var a=${f}([[1]]);a[0].push(2);return S(a)})`, `T(()=>{var a=${f}([1,2,3]);Object.defineProperty(a,0,{value:5});return S(a)})`, `T(()=>{var a=${f}([1,2,3]);Object.defineProperty(a,0,{value:1});return S(a)})`,
    `T(()=>{var a=${f}([1,2,3]);Object.defineProperty(a,3,{value:5});return S(a)})`, `T(()=>{var a=${f}([1,2,3]);return Reflect.set(a,0,9)+','+Reflect.set(a,3,9)+','+Reflect.deleteProperty(a,0)+','+Reflect.defineProperty(a,5,{value:1})})`,
    `T(()=>{var a=${f}([1,2,3]);return Object.assign(a,{0:9})})`, `T(()=>{var a=${f}([1,2,3]);Array.prototype.push.call(a,1)})`, `T(()=>{var a=${f}([1,2,3]);Array.prototype.splice.call(a,5,0)})`,
    `T(()=>{var a=${f}([1,2,3]);Array.prototype.at.call(a,-1);return a.at(-1)})`, `T(()=>{var a=${f}([1,2,3]);return Object.getPrototypeOf(a)===Array.prototype&&(Object.setPrototypeOf(a,Array.prototype)===a)})`, `T(()=>{var a=${f}([1,2,3]);Object.setPrototypeOf(a,{})})`,
    `T(()=>{var o=${f}({a:1,get b(){return 2}});o.b=3;return S(o)})`, `T(()=>{var o=${f}({a:1});o.z=1;return S(o)})`, `T(()=>{var o=${f}({a:1});delete o.a;return S(o)})`, `T(()=>{var o=${f}({a:{b:1}});o.a.b=2;return S(o)})`,
    `T(()=>{var o=${f}({get a(){return 1},set a(v){this.s=v}});o.a=2;return S(o)})`, `T(()=>{var o=${f}({a:1});Object.defineProperty(o,'a',{get(){return 2}})})`, `T(()=>{var o=${f}({a:1});Object.defineProperty(o,'a',{enumerable:false});return D(o,'a')})`,
    `T(()=>{var o=${f}({a:1});Object.defineProperty(o,'a',{writable:false});return D(o,'a')})`, `T(()=>{var o=${f}({a:1});Object.defineProperty(o,'a',{writable:true});return D(o,'a')})`,
  );
}
add(
  "T(()=>{var a=Object.freeze(new Uint8Array(0));return Object.isFrozen(a)})", "T(()=>{var a=Object.freeze(new Uint8Array(2))})", "T(()=>{var a=Object.seal(new Uint8Array(2));return Object.isSealed(a)+','+D(a,0)})", "T(()=>{var a=Object.preventExtensions(new Uint8Array(2));a[0]=5;a[5]=1;return S(a)+Object.isExtensible(a)})",
  "T(()=>{var a=Object.seal(new Uint8Array(2));a[0]=5;return S(a)})", "T(()=>{var a=Object.seal(new Uint8Array(2));delete a[0];return S(a)})", "T(()=>{var a=Object.seal(new Uint8Array(2));a.x=1;return S(a)})", "T(()=>{var a=Object.preventExtensions(new Uint8Array(2));a.x=1;return S(Object.keys(a))})",
  "T(()=>{var a=Object.preventExtensions(new Uint8Array(2));a.fill(3);return S(a)})", "T(()=>{var a=Object.seal(new Float64Array(2));a.set([1.5,2.5]);return S(a)})", "T(()=>{var a=Object.seal(new Uint8Array(2));return D(a,0)})", "T(()=>{var a=new Uint8Array(2);return D(a,0)+'|'+D(a,2)})",
  "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,0,{value:5});return S(a)})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,0,{value:5,writable:false})})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,0,{value:5,enumerable:false})})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,0,{value:5,configurable:false})})",
  "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,0,{get(){return 1}})})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,5,{value:1})})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,'-1',{value:1})})", "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,'1.5',{value:1});return Object.keys(a).join()})",
  "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,'x',{value:1});return D(a,'x')})", "T(()=>{var a=new Uint8Array(2);return Reflect.set(a,5,1)+','+Reflect.set(a,'-0',1)+','+Reflect.set(a,'1.5',1)+','+Reflect.set(a,'x',1)})", "T(()=>{var a=new Uint8Array(2);a[-0]=7;a['1.0']=8;return S(a)+Object.keys(a).join()})",
  "T(()=>{var a=new Uint8Array(2);a[5]=1;return (5 in a)+','+Object.keys(a).join()+a[5]})", "T(()=>{var a=new Uint8Array(2);return ('1' in a)+','+('-0' in a)+','+('1.5' in a)+','+('01' in a)})", "T(()=>{var a=new Uint8Array(2);a['01']=4;return S(a)+Object.keys(a).join()})",
  "T(()=>{var a=new Uint8Array(2);a[Symbol.for('s')]=1;return S(Reflect.ownKeys(a))})", "T(()=>{var a=new Uint8Array(2);a.b=1;a.a=2;a[1]=1;return Reflect.ownKeys(a).join()})", "T(()=>{var a=new Uint8Array([1,2,3]);a.length=1;return a.length+S(a)})", "T(()=>L('var a=new Uint8Array([1,2,3]);a.length=1;return a.length'))",
  "T(()=>{var a=new Uint8Array([1,2,3]);return D(a,'length')+Object.getOwnPropertyNames(a).join()})", "T(()=>{var d=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),'length');return typeof d.get+typeof d.set+d.enumerable+d.configurable})",
  "T(()=>{var a=new Uint8Array(2);Object.freeze(a.subarray(0,0));return 'ok'})", "T(()=>{var a=Object.freeze(new Uint8Array(0));a.x=1;return S(a)})", "T(()=>{var a=Object.freeze(new Uint8Array(0));return Object.isFrozen(a)+','+Object.isSealed(a)})",
  "T(()=>{var b=new ArrayBuffer(4);var a=new Uint8Array(b);Object.freeze(a)})", "T(()=>{var b=new ArrayBuffer(2,{maxByteLength:8});var a=new Uint8Array(b);Object.seal(a);b.resize(4);return a.length+Object.isSealed(a)})",
  "T(()=>{var a=new Uint8Array(2);Object.defineProperty(a,'x',{get(){return 1}});return a.x})", "T(()=>{var a=new Uint8Array(2);var p=Object.create(a);p[0]=9;return S(a)+S(Object.keys(p))})", "T(()=>{var a=new Uint8Array(2);var p=Object.create(a);p[5]=9;return S(Object.keys(p))+p[5]})",
  "T(()=>{var a=new Uint8Array(2);var p=Object.create(a);return Reflect.set(p,0,9,p)+S(a)+S(Object.keys(p))})", "T(()=>{var a=new Uint8Array(2);return Reflect.defineProperty(a,0,{value:3,writable:true,enumerable:true,configurable:true})+S(a)})",
  "T(()=>{var a=new Uint8Array(2);return Reflect.defineProperty(a,0,{value:3,writable:false})})", "T(()=>{var a=new Uint8Array(2);return Reflect.deleteProperty(a,0)+','+Reflect.deleteProperty(a,5)+','+Reflect.deleteProperty(a,'x')})",
  "T(()=>L('var a=new Uint8Array(2);return delete a[0]'))", "T(()=>{var a=new Uint8Array(2);delete a[0]})", "T(()=>{'use strict';var a=new Uint8Array(2);delete a[5];return 'ok'})",
);

// ---- 7. propriedades indexadas contra nomeadas, ordem de chaves.
const keyLists = [
  "{b:1,a:2,1:3,0:4}", "{b:1,2:2,a:3,1:4,[Symbol.for('s')]:5}", "{'10':1,'9':2,'-1':3,'1.5':4,'01':5}", "{4294967295:1,4294967294:2,4294967296:3}", "{9007199254740991:1,9007199254740992:2,1:3}",
  "{'':1,' ':2,'0':3}", "{[Symbol.for('a')]:1,x:2,[Symbol.for('b')]:3,1:4}", "{c:1,b:2,a:3}", "{1e3:1,1e21:2,0x10:3,0b11:4,0o7:5}", "{1:1,'1':2,1.0:3}", "{.5:1,0.5:2}", "{'1e3':1,1000:2}",
];
for (const k of keyLists) add(`T(()=>{var o=${k};return Reflect.ownKeys(o).map(String).join()})`, `T(()=>{var o=${k};var r=[];for(var x in o)r.push(x);return r.join()})`, `T(()=>S(Object.entries(${k}).map(e=>e[0])))`);
add(
  "T(()=>{var o={};o.b=1;o[2]=1;o.a=1;o[1]=1;o[Symbol.for('z')]=1;return Reflect.ownKeys(o).map(String).join()})", "T(()=>{var o={};o[1]=1;o[0]=1;delete o[1];o[1]=2;return Object.keys(o).join()})", "T(()=>{var o={b:1,a:1};delete o.b;o.b=2;return Object.keys(o).join()})",
  "T(()=>{var o={};o['4294967294']=1;o['4294967295']=2;o[3]=3;return Object.keys(o).join()})", "T(()=>JSON.stringify({b:1,2:1,a:1,1:1}))", "T(()=>JSON.stringify(Object.assign({z:1},{2:1,y:2,1:3})))", "T(()=>Object.keys('abc').join()+Object.keys(Object('ab')).join())",
  "T(()=>{var s=new String('ab');s.x=1;s[5]=2;return Reflect.ownKeys(s).join()})", "T(()=>{var s=new String('ab');return D(s,0)+'|'+D(s,'length')+'|'+D(s,2)})", "T(()=>{var s=new String('ab');s[0]='z';return s[0]})", "T(()=>L('var s=new String(\"ab\");s[0]=\"z\";return s[0]'))",
  "T(()=>{var s=new String('ab');s[0]='z'})", "T(()=>{var s=new String('ab');delete s[0]})", "T(()=>{var s=new String('ab');s[2]='c';return S(s)+s.length})", "T(()=>{var s=new String('ab');s.length=5;return s.length})", "T(()=>{var s=new String('ab');Object.defineProperty(s,0,{value:'a'});return 'ok'})",
  "T(()=>{var s=new String('ab');Object.defineProperty(s,0,{value:'q'})})", "T(()=>{var s=new String('ab');Object.defineProperty(s,2,{value:'q',enumerable:true,writable:true,configurable:true});return Reflect.ownKeys(s).join()})",
  "T(()=>{var a=[];a[3]=1;a.x=2;a[1]=3;return Reflect.ownKeys(a).join()+a.length})", "T(()=>{var a=[1,2,3];a['1']=9;a['01']=7;return S(a)+Reflect.ownKeys(a).join()})", "T(()=>{var a=[];a[-1]=1;a['1.5']=2;a[4294967295]=3;return a.length+Reflect.ownKeys(a).join()})", "T(()=>{var a=[];a[4294967294]=1;return a.length})",
  "T(()=>{var a=[];a['4294967294']=1;a[4294967295]=1;return a.length+','+Object.keys(a).join()})", "T(()=>{var a=[1,2,3];a.length=1;a[2]=5;return S(a)+a.length})", "T(()=>{var a=[1,2,3];delete a[1];return S(a)+Object.keys(a).join()+(1 in a)})", "T(()=>{var a=[,'b'];return (0 in a)+','+Object.keys(a).join()+a.length})",
  "T(()=>{var a=[1,2,3];a.length=0;return S(Reflect.ownKeys(a))})", "T(()=>{var a=[1];a.length='2';return a.length})", "T(()=>{var a=[1];a.length=-1})", "T(()=>{var a=[1];a.length=1.5})", "T(()=>{var a=[1];a.length={valueOf(){return 3}};return a.length})",
  "T(()=>{var a=[];a[Symbol.for('s')]=1;a[0]=1;a.q=1;return Reflect.ownKeys(a).map(String).join()})", "T(()=>{var a=[1,2];Object.defineProperty(a,'x',{value:1,enumerable:true});return Object.keys(a).join()+S(a)})", "T(()=>{var a=[1,2];a.x=1;return JSON.stringify(a)+Object.entries(a).length})",
  "T(()=>{var o={};o[-0]=1;o[0]=2;return Reflect.ownKeys(o).join()+o[0]})", "T(()=>{var o={};o[1.0]=1;o['1']=2;return Reflect.ownKeys(o).join()})", "T(()=>{var o={};o[NaN]=1;o[Infinity]=2;o[-Infinity]=3;return Reflect.ownKeys(o).join()})", "T(()=>{var o={};o[null]=1;o[undefined]=2;o[true]=3;return Reflect.ownKeys(o).join()})",
  "T(()=>{var o={};o[[1,2]]=1;o[{}]=2;return Reflect.ownKeys(o).join()})", "T(()=>{var o={};o[1n]=1;o[2n**64n]=2;return Reflect.ownKeys(o).join()})", "T(()=>{var o={};o[0.1+0.2]=1;o[1e21]=2;o[1e-7]=3;return Reflect.ownKeys(o).join()})", "T(()=>{var o={};o[2**31]=1;o[2**32-2]=2;o[-1]=3;o[2**32-1]=4;return Reflect.ownKeys(o).join()})",
  "T(()=>{var o=Object.create({1:'p',b:'p'});o[0]=1;o.a=1;var r=[];for(var k in o)r.push(k);return r.join()})", "T(()=>{var o={b:1,1:1};Object.defineProperty(o,'0',{value:1,enumerable:false});return Reflect.ownKeys(o).join()+Object.keys(o).join()})",
  "T(()=>{var o={};var r=[];Object.defineProperty(o,'z',{value:1,enumerable:true});Object.defineProperty(o,'5',{value:1,enumerable:true});return Reflect.ownKeys(o).join()})", "T(()=>{var o={a:1,b:2};Object.defineProperty(o,'a',{get(){return 1},enumerable:true});return Reflect.ownKeys(o).join()})",
  "T(()=>{var o={};for(var i=0;i<5;i++){o['k'+i]=i;o[i]=i}return Reflect.ownKeys(o).join()})", "T(()=>{var o={a:1,b:2,c:3};var r=[];for(var k in o){delete o.b;r.push(k)}return r.join()})", "T(()=>{var o={a:1,b:2};var r=[];for(var k in o){o.c=3;r.push(k)}return r.join()})",
  "T(()=>{class A{static b=1;static 2=2;static a=3;static 1=4}return Reflect.ownKeys(A).join()})", "T(()=>{class A{b(){}a(){}1(){}}return Reflect.ownKeys(A.prototype).join()})", "T(()=>Reflect.ownKeys(function f(a,b){}).join())", "T(()=>Reflect.ownKeys(()=>1).join())", "T(()=>Reflect.ownKeys(class{}).join())",
  "T(()=>Reflect.ownKeys(function(){'use strict'}).join())", "T(()=>Reflect.ownKeys(async function(){}).join())", "T(()=>Reflect.ownKeys(function*(){}).join())", "T(()=>Reflect.ownKeys((function(){return arguments})(1,2)).map(String).join())", "T(()=>Reflect.ownKeys((function(){'use strict';return arguments})(1)).map(String).join())",
);

// ---- 8. Symbol.toPrimitive.
const tp = (body) => `{[Symbol.toPrimitive](h){${body}}}`;
const hints = ["return h", "return 1", "return 'x'", "return {}", "return null", "return undefined", "return 1n", "return Symbol.for('q')", "throw new RangeError('tp')", "return h==='number'?7:'s'"];
for (const b of hints) for (const op of ["+({})", "`${({})}`", "({})*1", "({})+''", "String({})", "Number({})", "[{}]+''", "({})==1", "({})<2", "Object.keys({[{}]:1})[0]"]) {
  add(`T(()=>{var x=${tp(b)};return ${op.replace("({})", "x").replace("{}", "x")}})`);
}
add(
  "T(()=>{var x={[Symbol.toPrimitive]:undefined,valueOf(){return 2},toString(){return 's'}};return x+1})", "T(()=>{var x={[Symbol.toPrimitive]:null,valueOf(){return 2}};return x+1})", "T(()=>{var x={[Symbol.toPrimitive]:1};return x+1})", "T(()=>{var x={[Symbol.toPrimitive]:{}};return x+1})",
  "T(()=>{var x={[Symbol.toPrimitive]:'s'};return x+1})", "T(()=>{var x={get [Symbol.toPrimitive](){throw new EvalError('g')}};return x+1})", "T(()=>{var x={get [Symbol.toPrimitive](){return function(h){return this===x}}};return x+1})",
  "T(()=>{var x={[Symbol.toPrimitive](){return 5},valueOf(){return 1}};return x*2})", "T(()=>{var x={valueOf(){return {}},toString(){return '7'}};return x*2})", "T(()=>{var x={valueOf(){return {}},toString(){return {}}};return x*2})", "T(()=>{var x={valueOf:1,toString(){return '3'}};return x*2})",
  "T(()=>{var x={toString:1,valueOf(){return 4}};return `${x}`})", "T(()=>{var x={toString(){return 's'},valueOf(){return 4}};return `${x}`+x+String(x)+(x+'')})", "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return [`${x}`,x+'',x*1,String(x),+x,x==='default',x==1,x<1].join()})",
  "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return [x+1,x-1,x/1,x%1,x**1,x|1,-x,~x,x<<1,x>>>1].join()})", "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return [x>1,x>=1,x<1,x<=1,x==1,x!=1,x=='default',x==='default'].join()})",
  "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return [Math.abs(x),isNaN(x),parseInt(x),parseFloat(x),Number.isNaN(x),[1][x],isFinite(x)].join()})", "T(()=>{var x={[Symbol.toPrimitive](h){return h}};var o={};o[x]=1;return Reflect.ownKeys(o).join()})",
  "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return ({string:1}[x])+','+('string' in {string:1,default:2,number:3})+','+({default:1,string:2})[x]})", "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return [].concat(x).join()+[x].join()+('a'.concat(x))+('abc'.indexOf(x))})",
  "T(()=>{var x={[Symbol.toPrimitive](h){return h}};return new Date(0)[Symbol.toPrimitive]('number')+','+typeof (new Date(0)+x)})", "T(()=>{var d=new Date(0);return d[Symbol.toPrimitive]('default')===d[Symbol.toPrimitive]('string')})", "T(()=>{var d=new Date(0);return d[Symbol.toPrimitive]('number')})",
  "T(()=>{var d=new Date(0);return d[Symbol.toPrimitive]('x')})", "T(()=>{var d=new Date(0);return d[Symbol.toPrimitive]()})", "T(()=>Date.prototype[Symbol.toPrimitive].call(1,'number'))", "T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 5},toString(){return 's'}},'number'))",
  "T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 5},toString(){return 's'}},'default'))", "T(()=>D(Date.prototype,Symbol.toPrimitive))", "T(()=>Date.prototype[Symbol.toPrimitive].name+Date.prototype[Symbol.toPrimitive].length)", "T(()=>D(Symbol.prototype,Symbol.toPrimitive))",
  "T(()=>Symbol.prototype[Symbol.toPrimitive].call(Symbol.for('a'))===Symbol.for('a'))", "T(()=>Symbol.prototype[Symbol.toPrimitive].call(Object(Symbol.for('a')))===Symbol.for('a'))", "T(()=>Symbol.prototype[Symbol.toPrimitive].call(1))", "T(()=>{var s=Symbol('d');return Object(s)+''})",
  "T(()=>{var s=Symbol('d');return `${Object(s)}`})", "T(()=>{var s=Symbol('d');return Object(s)==s})", "T(()=>Symbol.toPrimitive.toString()+typeof Symbol.toPrimitive+D(Symbol,'toPrimitive'))", "T(()=>Object.getOwnPropertyNames(Date.prototype).includes('toJSON')+','+typeof Date.prototype[Symbol.toPrimitive])",
  "T(()=>{var n=Object(5);n[Symbol.toPrimitive]=()=>9;return n+1})", "T(()=>{var n=Object('a');n[Symbol.toPrimitive]=()=>9;return n+1})", "T(()=>{var n=Object(true);n.valueOf=()=>false;return n+1})", "T(()=>{var n=Object(1n);return n+1n})", "T(()=>{var n=Object(1n);return n+1})",
  "T(()=>{var x={[Symbol.toPrimitive](){return 1n}};return x+1n})", "T(()=>{var x={[Symbol.toPrimitive](){return 1n}};return x+1})", "T(()=>{var x={[Symbol.toPrimitive](){return 1n}};return +x})", "T(()=>{var x={[Symbol.toPrimitive](){return 1n}};return x==1})", "T(()=>{var x={[Symbol.toPrimitive](){return 2n}};return x>1})",
  "T(()=>{var x={[Symbol.toPrimitive](){return Symbol.for('s')}};return x+''})", "T(()=>{var x={[Symbol.toPrimitive](){return Symbol.for('s')}};return String(x)})", "T(()=>{var x={[Symbol.toPrimitive](){return Symbol.for('s')}};var o={};o[x]=1;return Reflect.ownKeys(o).map(String).join()})",
  "T(()=>{class A{[Symbol.toPrimitive](h){return h==='number'?1:'A'}}var a=new A;return a+1+','+(+a)+`${a}`})", "T(()=>{class A{static [Symbol.toPrimitive](){return 'sA'}}return A+''})", "T(()=>{class A{get [Symbol.toPrimitive](){return h=>h}}return new A+''})",
  "T(()=>{class A{valueOf(){return 3}}class B extends A{[Symbol.toPrimitive](){return 4}}return new B+1})", "T(()=>{class A{[Symbol.toPrimitive](){return 4}}class B extends A{valueOf(){return 3}}return new B+1})", "T(()=>{var log=[];var x={get [Symbol.toPrimitive](){log.push('get');return h=>{log.push(h);return 1}}};x+x;return log.join()})",
  "T(()=>{var log=[];var x={get valueOf(){log.push('vo');return ()=>({})},get toString(){log.push('ts');return ()=>'s'}};x+1;return log.join()})", "T(()=>{var log=[];var x={get valueOf(){log.push('vo');return ()=>({})},get toString(){log.push('ts');return ()=>'s'}};`${x}`;return log.join()})",
  "T(()=>{var p=new Proxy({},{get(t,k){return k===Symbol.toPrimitive?undefined:()=>({})}});return p+1})", "T(()=>{var p=new Proxy({},{get(t,k){return k===Symbol.toPrimitive?(h=>'px'+h):undefined}});return p+1})", "T(()=>{var f=function(){};f[Symbol.toPrimitive]=()=>3;return f+1})",
  "T(()=>{var a=[1];a[Symbol.toPrimitive]=()=>3;return a+1})", "T(()=>{return [1,2]+[3]+{}+null+undefined+true})", "T(()=>{return [] + {}})", "T(()=>{return ({}) + []})", "T(()=>{return String([1,[2,3]])+String({})+String(function(){})})",
);

// ---- 9. herança de acessores.
add(
  "T(()=>{var p={get a(){return this.n},set a(v){this.n=v}};var o=Object.create(p);o.a=3;return S(o)+o.a+p.n})", "T(()=>{var p={get a(){return 1}};var o=Object.create(p);o.a=3;return o.a+S(o)})", "T(()=>{var p={set a(v){this.n=v}};var o=Object.create(p);o.a=3;return S(o)+o.a})",
  "T(()=>{var p={a:1};var o=Object.create(p);o.a=3;return S(o)+p.a})", "T(()=>{var p={};Object.defineProperty(p,'a',{value:1,writable:false});var o=Object.create(p);o.a=3;return S(o)})", "T(()=>{var p={};Object.defineProperty(p,'a',{value:1,writable:false});var o=Object.create(p);Object.defineProperty(o,'a',{value:3});return S(o)})",
  "T(()=>{var p={};Object.defineProperty(p,'a',{value:1,writable:false});var o=Object.create(p);o.a=3})", "T(()=>L('var p={};Object.defineProperty(p,\"a\",{value:1,writable:false});var o=Object.create(p);o.a=3;return JSON.stringify(o)'))",
  "T(()=>{var g=0;var p={get a(){g++;return 1}};var o=Object.create(Object.create(p));o.a;o.a;return g})", "T(()=>{var p={get a(){return this}};var o=Object.create(p);return o.a===o})", "T(()=>{var p={get a(){return this}};return p.a===p})", "T(()=>{var p={get a(){return typeof this}};return (1).__proto__===Number.prototype&&(Object.defineProperty(Number.prototype,'zz',{get:p.__lookupGetter__('a'),configurable:true}),(1).zz)})",
  "T(()=>{Object.defineProperty(Number.prototype,'zz',{get(){'use strict';return typeof this},configurable:true});var r=(1).zz;delete Number.prototype.zz;return r})", "T(()=>{Object.defineProperty(Number.prototype,'zz',{get(){return typeof this},configurable:true});var r=(1).zz;delete Number.prototype.zz;return r})",
  "T(()=>{Object.defineProperty(String.prototype,'zz',{get(){'use strict';return this},configurable:true});var r=typeof 'a'.zz;delete String.prototype.zz;return r})", "T(()=>{Object.defineProperty(String.prototype,'zz',{set(v){'use strict';this.q=v},configurable:true});'a'.zz=1;delete String.prototype.zz;return 'ok'})",
  "T(()=>{Object.defineProperty(Object.prototype,'zz',{set(v){Object.defineProperty(this,'zz',{value:v*2,enumerable:true})},configurable:true});var o={};o.zz=2;var r=S(o);delete Object.prototype.zz;return r})", "T(()=>{Object.defineProperty(Object.prototype,'zz',{value:1,writable:false,configurable:true});var o={};o.zz=2;var r=S(o);delete Object.prototype.zz;return r})",
  "T(()=>{Object.defineProperty(Object.prototype,'zz',{value:1,writable:false,configurable:true});var o={zz:2};var r=S(o);delete Object.prototype.zz;return r})", "T(()=>{Object.defineProperty(Object.prototype,'zz',{get(){return 'inh'},configurable:true});var r=({}).zz+Object.keys({}).length+('zz' in {});delete Object.prototype.zz;return r})",
  "T(()=>{Object.defineProperty(Array.prototype,'1',{get(){return 'ah'},configurable:true});var r=[0,,2][1]+','+S([0,,2]);delete Array.prototype[1];return r})", "T(()=>{Object.defineProperty(Array.prototype,'1',{set(v){},configurable:true});var a=[];a[1]=5;var r=S(a)+a.length;delete Array.prototype[1];return r})",
  "T(()=>{Object.defineProperty(Array.prototype,'0',{value:'x',writable:false,configurable:true});try{var a=[];a[0]=5;var r=S(a);a.push(1);r+=S(a);return r}finally{delete Array.prototype[0]}})", "T(()=>{Object.defineProperty(Array.prototype,'1',{get(){return 'ah'},configurable:true});var r=[0,,2].map(x=>x).join();delete Array.prototype[1];return r})",
  "T(()=>{Object.defineProperty(Array.prototype,'1',{get(){return 'ah'},configurable:true});var r=[0,,2].indexOf('ah')+','+[0,,2].includes('ah')+','+(1 in [0,,2]);delete Array.prototype[1];return r})", "T(()=>{var p={get a(){return 1}};var q=Object.create(p);Object.defineProperty(q,'a',{get(){return 2}});var o=Object.create(q);return o.a})",
  "T(()=>{var p={get a(){return 1}};var q=Object.create(p);q.a=undefined;var o=Object.create(q);return o.a+','+S(q)})", "T(()=>{var p={get a(){return 1}};var q=Object.create(p,{a:{value:5,writable:true,enumerable:true,configurable:true}});var o=Object.create(q);return o.a})",
  "T(()=>{var p={get a(){return 1},set a(v){this.z=v}};var q=Object.create(p);var o=Object.create(q);o.a=1;return S(o)+S(q)})", "T(()=>{var p={get a(){return 1},set a(v){this.z=v}};var o=Object.create(p);delete o.a;return o.a})", "T(()=>{var p={get a(){return 1}};var o=Object.create(p);delete p.a;return o.a})",
  "T(()=>{var p={get a(){return 1}};var o=Object.create(p);Object.defineProperty(p,'a',{value:2});return o.a})", "T(()=>{var p={};Object.defineProperty(p,'a',{get(){return 1},configurable:true});var o=Object.create(p);Object.defineProperty(p,'a',{get(){return 2}});return o.a})",
  "T(()=>{var o=Object.create({get a(){return 1}},{b:{get(){return this.a+1}}});return o.b})", "T(()=>{class A{get x(){return 1}}class B extends A{}var b=new B;return b.x+D(B.prototype,'x')+('x' in b)+b.hasOwnProperty('x')})", "T(()=>{class A{get x(){return 1}}class B extends A{x=2}return new B().x})",
  "T(()=>{class A{get x(){return 1}}class B extends A{constructor(){super();this.x=2}}new B})", "T(()=>{class A{set x(v){this.y=v}}class B extends A{x=2}var b=new B;return S(b)})", "T(()=>{class A{set x(v){this.y=v}}class B extends A{constructor(){super();this.x=2}}var b=new B;return S(b)})",
  "T(()=>{class A{static get x(){return 'sx'}}class B extends A{}return B.x+D(B,'x')})", "T(()=>{class A{static set x(v){this.y=v}}class B extends A{}B.x=1;return S(B.y)+A.y})", "T(()=>{class A{static x=1}class B extends A{}B.x=2;return A.x+','+B.x+','+Object.hasOwn(B,'x')})",
  "T(()=>{class A{static get x(){return this.name}}class B extends A{}return B.x})", "T(()=>{var o={get a(){return 1}};var c=Object.assign({},o);return D(c,'a')})", "T(()=>{var o={get a(){return 1}};var c={...o};return D(c,'a')})", "T(()=>{var o={set a(v){}};var c={...o};return D(c,'a')})",
  "T(()=>{var o={get a(){return 1}};var c=Object.defineProperties({},Object.getOwnPropertyDescriptors(o));return D(c,'a')})", "T(()=>{var o=Object.defineProperty({},'a',{get(){return 1}});var c={...o};return S(c)})", "T(()=>{var o={get a(){this.n=(this.n||0)+1;return 1}};var c={...o};return o.n+','+S(c)})",
  "T(()=>{var {a}={get a(){return 4}};return a})", "T(()=>{var {a,...r}={get a(){return 4},get b(){return 5}};return a+S(r)+D(r,'b')})", "T(()=>{var o=Object.create({get a(){return 1}});var {a}=o;return a})", "T(()=>{var o=Object.create({get a(){return 1}});var c={...o};return S(c)})",
  "T(()=>{var o=Object.create({a:1});o.b=2;return JSON.stringify(o)+Object.keys(o)+Object.entries(o)})", "T(()=>{var o=Object.create({a:1});o.b=2;return Object.assign({},o).a})", "T(()=>{var o={a:{get x(){return 1}}};return typeof JSON.parse(JSON.stringify(o)).a.x})",
);

// ---- 10. escrita em readonly: estrito contra sloppy.
const ro = [
  "var o=Object.freeze({a:1});o.a=2", "var o=Object.freeze({a:1});o.b=2", "var o=Object.freeze({a:1});delete o.a", "var o=Object.seal({a:1});delete o.a", "var o=Object.seal({a:1});o.b=1", "var o=Object.seal({a:1});o.a=2",
  "var o=Object.preventExtensions({a:1});o.b=1", "var o=Object.preventExtensions({a:1});delete o.a", "var o={get a(){return 1}};o.a=2", "var o={get a(){return 1}};delete o.a", "var o={};Object.defineProperty(o,'a',{value:1});o.a=2",
  "var o={};Object.defineProperty(o,'a',{value:1});delete o.a", "var o={};Object.defineProperty(o,'a',{value:1,writable:true});o.a=2", "var a=Object.freeze([1]);a[0]=2", "var a=Object.freeze([1]);a[1]=2", "var a=Object.freeze([1]);a.length=0",
  "var a=Object.freeze([1]);a.push(2)", "var a=Object.freeze([1]);delete a[0]", "var a=Object.freeze([1]);a.x=1", "var a=Object.seal([1]);a.length=0", "var a=Object.seal([1,2]);a.pop()", "var a=Object.seal([1]);a[1]=1",
  "var s='abc';s[0]='z'", "var s='abc';s.length=1", "var s='abc';s.x=1", "var s='abc';delete s[0]", "var s='abc';delete s.length", "var n=1;n.x=1", "var n=1;n.toString=1", "var b=true;b.x=1", "var y=Symbol();y.x=1", "var g=1n;g.x=1",
  "undefined.x=1", "null.x=1", "var u;u.x=1", "Math.PI=3", "delete Math.PI", "NaN=1", "undefined=1", "Infinity=1", "globalThis.NaN=1", "globalThis.undefined=1", "delete globalThis.NaN", "Object.prototype=1", "Array.prototype=1",
  "var f=function(){};f.name='z'", "var f=function(){};f.length=3", "var f=function(){};delete f.name", "var f=function(){};delete f.prototype", "function f(){}f.prototype=1;f.caller=1", "var f=()=>1;f.prototype=1", "class A{}A.prototype=1", "class A{}A.name='z'",
  "class A{static x=1}Object.freeze(A);A.x=2", "class A{}Object.freeze(A);A.y=2", "class A{}Object.freeze(A.prototype);A.prototype.m=1", "class A{m(){}}A.prototype.m=1;var m=A.prototype.m", "class A{get x(){return 1}}new A().x=1", "class A{static get x(){return 1}}A.x=1",
  "var o={a:1};Object.defineProperty(o,'a',{writable:false});o.a=2;return o.a", "var o={a:1};Object.defineProperty(o,'a',{writable:false});o.a++", "var o={a:1};Object.defineProperty(o,'a',{writable:false});o.a+=1", "var o={a:1};Object.defineProperty(o,'a',{writable:false});o.a??=1;return o.a",
  "var o={a:0};Object.defineProperty(o,'a',{writable:false});o.a||=1", "var o={a:1};Object.defineProperty(o,'a',{writable:false});o.a&&=1", "var o={a:1};Object.defineProperty(o,'a',{writable:false});[o.a]=[5]", "var o={a:1};Object.defineProperty(o,'a',{writable:false});({a:o.a}={a:5})",
  "var o={a:1};Object.defineProperty(o,'a',{writable:false});for(o.a of [1]);", "var o={a:1};Object.defineProperty(o,'a',{writable:false});for(o.a in {x:1});", "var o={a:1};Object.defineProperty(o,'a',{writable:false});with(o){a=2}", "var o={a:1};Object.defineProperty(o,'a',{writable:false});Object.assign(o,{a:2})",
  "var o=Object.freeze({a:1});Object.assign(o,{a:2})", "var o=Object.freeze({a:1});Object.assign(o,{})", "var o=Object.freeze({a:1});Reflect.set(o,'a',2)", "var o=Object.freeze({a:1});o.__proto__=null", "var o=Object.freeze({a:1});Object.setPrototypeOf(o,null)", "var o=Object.freeze({a:1});Object.setPrototypeOf(o,Object.prototype)",
  "var o=Object.freeze(function(){});o.x=1", "var o=Object.freeze(new Map);o.set(1,2);return o.size", "var o=Object.freeze(new Date(0));o.setFullYear(2000);return o.getFullYear()", "var o=Object.freeze(/x/g);o.lastIndex=3", "var o=Object.freeze(/x/g);o.test('x');return o.lastIndex", "var o=Object.freeze(new Error('m'));o.message='n'",
  "var o=Object.freeze(Symbol.prototype);o.x=1", "arguments.callee=1", "(function(){'use strict';return arguments.callee})()", "(function(){return arguments.callee===undefined})()", "(function(){'use strict';arguments.callee=1})()", "(function(a){'use strict';arguments[0]=2;return a})(1)", "(function(a){arguments[0]=2;return a})(1)",
  "(function(a){'use strict';a=2;return arguments[0]})(1)", "(function(a){a=2;return arguments[0]})(1)", "(function(a){delete arguments[0];arguments[0]=5;return a})(1)", "(function(a){Object.defineProperty(arguments,'0',{writable:false});a=3;return arguments[0]})(1)", "(function(a){Object.defineProperty(arguments,'0',{value:9});return a})(1)",
  "(function(a){Object.defineProperty(arguments,'0',{get(){return 7}});a=3;return arguments[0]+','+a})(1)", "(function(){Object.freeze(arguments);arguments[0]=2;return arguments[0]})(1)", "(function(a){Object.freeze(arguments);a=2;return arguments[0]})(1)", "(function(){'use strict';Object.freeze(arguments);arguments[0]=2})(1)",
  "var o={};Object.defineProperty(o,'a',{get(){return 1},set:undefined});o.a=2", "var o={};Object.defineProperty(o,'a',{set(v){},get:undefined});return o.a", "var o={set a(v){}};delete o.a;return 'a' in o", "var o={a:1};delete o.a;delete o.a;return 'ok'", "var o={};delete o.zz", "var a=[1];delete a.length", "var a=[1];delete a[5]",
  "delete Object.prototype", "delete Object.prototype.hasOwnProperty", "delete [].length", "delete 'abc'.length", "delete 'abc'[5]", "delete (1).x", "var O=Object;try{delete globalThis.Object;return typeof Object}finally{O.defineProperty(globalThis,'Object',{value:O,writable:true,configurable:true,enumerable:false})}", "var o={};return delete o.a.b", "var o=Object.freeze({a:{b:1}});delete o.a.b;return JSON.stringify(o)",
];
for (const body of ro) {
  const withRet = /\breturn\b/.test(body) ? body : body + ";return 'done'";
  add(`L(${JSON.stringify("'use strict';" + withRet)})`);
  add(`L(${JSON.stringify(withRet)})`);
}
add(
  "T(()=>{var r=[];try{(function(){'use strict';undeclared_zz=1})()}catch(e){r.push(e.name)}return r.join()+typeof undeclared_zz})", "L('undeclared_zz2=1;var r=typeof undeclared_zz2;delete globalThis.undeclared_zz2;return r')", "L('\"use strict\";undeclared_zz3=1')",
  "T(()=>{'use strict';var o=Object.freeze({a:1});return Reflect.set(o,'a',2)+','+o.a})", "T(()=>{'use strict';var o={};Object.defineProperty(o,'a',{value:1});return Reflect.defineProperty(o,'a',{value:2})+','+Reflect.deleteProperty(o,'a')})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get(){return 1}});o.a=5;return o.a+Object.keys(o).length})", "T(()=>{var n=0;var o={get a(){n++;return 1}};o.a=2;o.a;return n})", "T(()=>{var o={a:1};Object.freeze(o);var r=Object.isFrozen(o)+','+delete o.a;return r})",
  "L('var o=Object.freeze({a:1});return delete o.a+\",\"+(o.a=2)+\",\"+o.a')", "L('\"use strict\";var o=Object.freeze({a:1});var r=[];try{o.a=2}catch(e){r.push(e.constructor===TypeError)}try{delete o.a}catch(e){r.push(e.constructor===TypeError)}return r.join()')",
);

// ---- Execução.
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    globalThis.R = undefined;
    vm.runInThisContext(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
