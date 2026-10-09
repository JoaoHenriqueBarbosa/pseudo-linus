// Gera tests/golden/ctor_this_bun.tsv: funções e this/new/super medidos no bun. Cobre Function.prototype.bind/call/apply
// (name, length, new em bound, Symbol.hasInstance), new.target em todos os contextos, construtores derivados e retorno
// de objeto/primitivo, super() duas vezes, this antes de super (ReferenceError), class fields e static blocks com this,
// métodos privados, `#x in obj`, accessors privados, herança de built-ins (Array, Error, Promise, Map, RegExp) e
// Symbol.species.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa é medido num processo
// próprio com `require('node:vm').runInThisContext(src)`, depois de esvaziadas as microtarefas. Sem APIs de host
// dentro dos programas (setTimeout, process, console, require, Bun, URL, Buffer).
// Uso: bun scripts/gen-ctor-this-golden.js > tests/golden/ctor_this_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
// Prelúdio: S serializa um valor de forma estável; T roda uma função e devolve o valor serializado ou `Nome: mensagem`.
const PRE =
  "const S=v=>{try{return typeof v==='string'?JSON.stringify(v):Object.is(v,-0)?'-0':typeof v==='bigint'?v+'n':" +
  "typeof v==='symbol'?v.toString():typeof v==='function'?'fn:'+v.name:v===undefined?'undefined':" +
  "Array.isArray(v)?'['+v.map(S).join(',')+']':v&&typeof v==='object'?Object.prototype.toString.call(v):String(v)}" +
  "catch(e){return '?'}};" +
  "const T=f=>{try{return S(f())}catch(e){return e.name+': '+e.message}};";
// Programa síncrono: o corpo devolve o valor de R.
const add = body => programs.push(`(function(){${PRE}globalThis.R=T(()=>{${body}})})()`);
// Programa assíncrono: o corpo atribui globalThis.R sozinho (cadeia de promessas).
const addAsync = body =>
  programs.push(`(function(){${PRE}try{${body}}catch(e){globalThis.R=e.name+': '+e.message}})()`);

// ---- Function.prototype.bind: name, length, new, hasInstance, partial application.
const bindTargets = [
  ["function f(a, b, c) {}", "f"],
  ["function f(a, b = 1, c) {}", "f"],
  ["function f(...r) {}", "f"],
  ["const f = (a, b) => 0;", "f"],
  ["class f { constructor(a) {} }", "f"],
  ["const f = function () {};", "f"],
  ["const f = Math.max;", "f"],
  ["const f = { m(a) {} }.m;", "f"],
  ["const f = { get g() { return 1 } }; const d = Object.getOwnPropertyDescriptor(f, 'g').get;", "d"],
  ["const f = async function (a, b) {};", "f"],
  ["function* g(a) {} const f = g;", "f"],
  ["const f = Symbol.prototype[Symbol.toPrimitive];", "f"],
];
for (const [decl, id] of bindTargets) {
  add(`${decl} const b = ${id}.bind(null); return [b.name, b.length]`);
  add(`${decl} const b = ${id}.bind(null, 1); return [b.name, b.length]`);
  add(`${decl} const b = ${id}.bind(null, 1, 2, 3, 4); return [b.name, b.length]`);
  add(`${decl} const b = ${id}.bind(null).bind(null); return [b.name, b.length]`);
  add(`${decl} const b = ${id}.bind(null); return [typeof b, b.hasOwnProperty('prototype'), Object.getPrototypeOf(b) === Function.prototype]`);
  add(`${decl} const b = ${id}.bind(null); return Object.getOwnPropertyNames(b).sort()`);
  add(`${decl} const b = ${id}.bind(null); return b.toString().replace(/\\s+/g, ' ')`);
}
add("function f() {} Object.defineProperty(f, 'name', { value: 42 }); return f.bind().name");
add("function f() {} Object.defineProperty(f, 'name', { value: Symbol('s') }); return f.bind().name");
add("function f() {} delete f.name; return f.bind().name");
add("function f() {} delete f.name; return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: 5.7 }); return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: -3 }); return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: Infinity }); return f.bind(null, 1).length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: -Infinity }); return f.bind(null, 1).length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: NaN }); return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: '3' }); return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: 2n }); return f.bind().length");
add("function f(a, b) {} Object.defineProperty(f, 'length', { value: 2 ** 40 }); return f.bind(null, 1).length");
add("function f() {} Object.setPrototypeOf(f, null); return Function.prototype.bind.call(f, null).name");
add("function f() {} Object.setPrototypeOf(f, Object.prototype); return Object.getPrototypeOf(f.bind()) === Object.prototype");
add("class A {} Object.setPrototypeOf(A, null); return Object.getPrototypeOf(A.bind ? 0 : Function.prototype.bind.call(A))");
add("return Function.prototype.bind.call({}, null)");
add("return Function.prototype.bind.call(1)");
add("return Function.prototype.bind.call(new Proxy(function () {}, {})).name");
add("const p = new Proxy(function f(a) {}, {}); const b = p.bind(null); return [b.name, b.length]");
add("const p = new Proxy(function f(a) {}, { get(t, k) { return k === 'name' ? 'zz' : t[k] } }); return p.bind(null).name");
add("const p = new Proxy({}, {}); return typeof Function.prototype.bind.call(p, null)");
add("const t = []; const p = new Proxy(function () {}, { getOwnPropertyDescriptor(o, k) { t.push('d:' + String(k)); return Reflect.getOwnPropertyDescriptor(o, k) }, get(o, k) { t.push('g:' + String(k)); return o[k] } }); p.bind(null); return t");
// new em bound
add("function F(a, b) { this.a = a; this.b = b } const B = F.bind({ x: 1 }, 10); const o = new B(20); return [o.a, o.b, o.x, o instanceof F, o instanceof B]");
add("function F() { this.t = new.target } const B = F.bind(null); const o = new B; return [o.t === F, o.t === B]");
add("function F() { return new.target } const B = F.bind(null); return [new B() === F, B() === undefined]");
add("function F() { return new.target } const B = F.bind(null); return Reflect.construct(B, [], Array) === Array");
add("function F() { return new.target } const B = F.bind(null); return Reflect.construct(B, [], B) === F");
add("function F() { return new.target } const B = F.bind(null); const C = B.bind(null); return new C() === F");
add("function F() { return new.target } const B = F.bind(null); const C = B.bind(null); return Reflect.construct(C, [], C) === F");
add("class A { constructor(x) { this.x = x } } const B = A.bind(null, 7); const o = new B(); return [o.x, o instanceof A, o.constructor === A]");
add("class A { constructor() { this.nt = new.target } } const B = A.bind(null); return [new B().nt === A]");
add("class A {} const B = A.bind(null); return B()");
add("const f = () => 1; const b = f.bind(null); return new b()");
add("const f = { m() {} }.m; return new (f.bind(null))()");
add("async function f() {} return new (f.bind(null))()");
add("function* g() {} return new (g.bind(null))()");
add("function F() {} F.prototype.z = 1; const B = F.bind(null); return [new B().z, B.prototype]");
add("function F() {} const B = F.bind(null); B.prototype = { y: 2 }; return new B().y");
add("function F() {} const B = F.bind(null); return new B() instanceof F");
add("function F() {} const B = F.bind(null); return [new F() instanceof B, ({}) instanceof B]");
add("function F() {} const B = F.bind(null); return Object.getOwnPropertyDescriptor(F, 'prototype').writable");
add("function F() { return 1 } const B = F.bind(null); return new B() instanceof F");
add("function F() { return { k: 1 } } const B = F.bind(null); return new B().k");
add("const B = Array.bind(null, 3); return [new B().length, B().length]");
add("const B = Date.bind(null, 0); return [typeof B(), new B() instanceof Date]");
add("const B = Error.bind(null, 'm'); return [new B().message, B().message, B() instanceof Error]");
add("const B = Map.bind(null); return B()");
add("const B = Map.bind(null); return new B() instanceof Map");
add("const B = Promise.bind(null); return new B(r => r(1)) instanceof Promise");
add("const B = Symbol.bind(null); return [typeof B('x'), (() => { try { new B } catch (e) { return e.message } })()]");
add("const B = BigInt.bind(null); return [B(5), (() => { try { new B(1) } catch (e) { return e.message } })()]");
add("const B = Object.bind(null, 1); return [typeof new B(), typeof B()]");
add("const B = String.bind(null, 'ab'); return [typeof B(), typeof new B(), new B().length]");
add("const B = Function.prototype.call.bind(Array.prototype.slice); return B([1, 2, 3], 1)");
add("const B = Function.prototype.apply.bind(Math.max, null); return B([1, 5, 3])");
add("const push = Function.prototype.call.bind(Array.prototype.push); const a = []; push(a, 1, 2); return a");
add("const B = Function.prototype.bind.bind(function () { return [this, ...arguments] }); const C = B(5, 6); return C(7)");
add("const B = function () { return this }.bind(5); return [typeof B(), B() instanceof Number]");
add("const B = function () { 'use strict'; return this }.bind(5); return [typeof B(), B()]");
add("const B = function () { return this }.bind(null); return B() === globalThis");
add("const B = function () { 'use strict'; return this }.bind(undefined); return B()");
add("const B = (function () { return this }.bind(1)).bind(2); return typeof B()");
add("const B = (function () { 'use strict'; return this }.bind(1)).bind(2); return B()");
add("const o = { v: 1, f() { return this.v } }; const B = o.f.bind(o); return [B.call({ v: 2 }), B.apply({ v: 3 }), new.target === undefined]");
add("const f = () => this; const B = f.bind({ z: 1 }); return B() === this");
add("function F(a) { return [this, a] } const B = F.bind('s', 1); return B(2).map(String)");
add("function F() { return arguments.length } const B = F.bind(null, 1, 2); return [B(), B(3, 4), new B().constructor === F ? 1 : 0]");
add("function F() { return arguments.length } const B = F.bind(null, ...new Array(1000).fill(0)); return B(1)");
add("function F() { return arguments.length } return F.bind(null, ...new Array(100000).fill(0))()");
// Symbol.hasInstance
add("return Function.prototype[Symbol.hasInstance].call(function () {}, {})");
add("function F() {} return Function.prototype[Symbol.hasInstance].call(F, new F)");
add("return Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance)");
add("const d = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance); return [d.writable, d.enumerable, d.configurable, d.value.name, d.value.length]");
add("return Function.prototype[Symbol.hasInstance].call({}, {})");
add("return Function.prototype[Symbol.hasInstance].call(1, {})");
add("function F() {} const B = F.bind(null); return Function.prototype[Symbol.hasInstance].call(B, new F)");
add("function F() {} F.prototype = 1; return Function.prototype[Symbol.hasInstance].call(F, {})");
add("function F() {} F.prototype = 1; return (() => { try { return {} instanceof F } catch (e) { return e.message } })()");
add("function F() {} F.prototype = 1; return Function.prototype[Symbol.hasInstance].call(F, 1)");
add("class A { static [Symbol.hasInstance](v) { return v === 1 } } return [1 instanceof A, 2 instanceof A, new A instanceof A]");
add("class A { static [Symbol.hasInstance]() { return 'x' } } return {} instanceof A");
add("class A { static [Symbol.hasInstance]() { return 0 } } return {} instanceof A");
add("class A { static get [Symbol.hasInstance]() { return undefined } } return new A instanceof A");
add("class A { static get [Symbol.hasInstance]() { return null } } return new A instanceof A");
add("class A { static get [Symbol.hasInstance]() { return 1 } } return new A instanceof A");
add("const o = { [Symbol.hasInstance]: v => v > 1 }; return [2 instanceof o, 1 instanceof o]");
add("const o = {}; return 1 instanceof o");
add("const o = { [Symbol.hasInstance]: 3 }; return 1 instanceof o");
add("return 1 instanceof (() => {})");
add("return 1 instanceof (async function () {})");
add("return ({}) instanceof { m() {} }.m");
add("function F() {} F.prototype = null; return (() => { try { return {} instanceof F } catch (e) { return e.name + e.message } })()");
add("function F() {} const o = Object.create(new F); return [o instanceof F, Object.create(null) instanceof F]");
add("function F() {} const o = new F; Object.setPrototypeOf(o, null); return o instanceof F");
add("const p = new Proxy({}, { getPrototypeOf() { return Array.prototype } }); return p instanceof Array");
add("const p = new Proxy(function () {}, { get(t, k) { return k === Symbol.hasInstance ? () => 'yes' : t[k] } }); return {} instanceof p");
add("const t = []; function F() {} const B = F.bind(null); Object.defineProperty(F, 'prototype', { get() { t.push('proto'); return {} } }); return [{} instanceof B, t]");
add("class A { static [Symbol.hasInstance](v) { return super[Symbol.hasInstance](v) } } return [new A instanceof A, ({}) instanceof A]");
add("class B { static [Symbol.hasInstance](v) { return Function.prototype[Symbol.hasInstance].call(this, v) } } class D extends B {} return [new D instanceof B, new B instanceof D]");

// ---- call/apply.
add("function f() { return this } return [typeof f.call(1), f.call(null) === globalThis, f.call(undefined) === globalThis]");
add("function f() { 'use strict'; return this } return [f.call(1), f.call(null), f.call(undefined), f.call('s')]");
add("function f() { return arguments.length } return [f.call(), f.call(null), f.call(null, 1, 2), f.apply(null), f.apply(null, []), f.apply(null, undefined), f.apply(null, null)]");
add("function f() { return arguments.length } return f.apply(null, { length: 3 })");
add("function f() { return [...arguments] } return f.apply(null, { length: 2, 0: 'a', 1: 'b' })");
add("function f() { return [...arguments] } return f.apply(null, 'ab')");
add("function f() { return arguments.length } return f.apply(null, 1)");
add("function f() { return arguments.length } return f.apply(null, true)");
add("function f() { return arguments.length } return f.apply(null, Symbol())");
add("function f() { return arguments.length } return f.apply(null, function (a, b) {})");
add("function f() { return [...arguments] } return f.apply(null, { length: -1 })");
add("function f() { return [...arguments] } return f.apply(null, { length: '2', 0: 1, 1: 2 })");
add("function f() { return arguments.length } return f.apply(null, { length: 2 ** 32 })");
add("function f() { return [...arguments] } return f.apply(null, { get length() { return 1 }, get 0() { return 'x' } })");
add("function f() { return arguments.length } return f.apply(null, new Proxy([1, 2, 3], {}))");
add("function f() { return arguments.length } return f.apply(null, new Set([1, 2]))");
add("function f() { return arguments.length } return f.apply(null, new Map([[1, 2]]))");
add("function f() { return arguments.length } return f.apply(null, new Uint8Array(4))");
add("function f() { return [...arguments] } return f.apply(null, [, 1])");
add("function f() { return [...arguments] } const a = [1, 2, 3]; return f.apply(null, a.slice(1))");
add("return Function.prototype.call.call(1)");
add("return Function.prototype.apply.call({}, null, [])");
add("return Function.prototype.call.call(Math.max, null, 4, 9)");
add("return Function.prototype.call.apply(Math.max, [null, 4, 9])");
add("return Function.prototype.apply.call(Math.max, null, [4, 9])");
add("return Function.prototype.apply.apply(Math.max, [null, [4, 9]])");
add("return Function.prototype.call.call(Function.prototype.call, Math.max, null, 3, 7)");
add("return [Function.prototype.call.length, Function.prototype.apply.length, Function.prototype.bind.length, Function.prototype.toString.length]");
add("return [Function.prototype.call.name, Function.prototype.apply.name, Function.prototype.bind.name]");
add("return new Function.prototype.call()");
add("return new Function.prototype.apply()");
add("return new Function.prototype.bind()");
add("return new (function () {}.call)()");
add("return Function.prototype()");
add("return [typeof Function.prototype, Function.prototype.length, Function.prototype.name === '', Object.getPrototypeOf(Function.prototype) === Object.prototype]");
add("return new Function.prototype");
add("function f(a, b) { return a + b } return f.call(null, ...[1, 2])");
add("const o = { f() { return this === o } }; return [o.f.call(o), o.f.apply(o), o.f.call({}), (0, o.f)(), (o.f)()]");
add("const o = { f() { 'use strict'; return this } }; return [(0, o.f)(), (o.f)() === o, (o.f = o.f)() ]");
add("function f() { return typeof this } return [f.call(1n), f.call(Symbol()), f.call(true), f.call('')]");
add("function f() { 'use strict'; return typeof this } return [f.call(1n), f.call(Symbol()), f.call(true), f.call('')]");
add("const f = () => typeof this; return [f.call(1), f.apply(2), f.bind(3)()]");
add("const f = () => arguments.length; return (function () { return f.call(null, 1, 2, 3) })(9)");

// ---- new.target em todos os contextos.
const ntContexts = [
  ["function f() { return new.target }", "f()", "undefined"],
  ["function f() { return new.target === f }", "new f", "obj"],
  ["function f() { return new.target }", "new f", "ctor"],
  ["function f() { return () => new.target }", "f()()", "undef"],
  ["function f() { return () => new.target }", "new f", "arrow-ctor"],
  ["function f() { return () => () => new.target === f }", "new f", "arrow2"],
  ["function f() { return eval('new.target') }", "f()", "eval-call"],
  ["function f() { return eval('new.target') === f }", "new f", "eval-new"],
  ["function f() { return eval('() => new.target') }", "f()()", "eval-arrow-call"],
  ["function f() { return Reflect.construct(g, []); } function g() { return new.target === f }", "f()", "reflect-g"],
  ["function f() { this.t = new.target }", "new f", "field"],
  ["function f() { return new.target }", "f.call({})", "call"],
  ["function f() { return new.target }", "f.apply({})", "apply"],
  ["function f() { return new.target }", "Reflect.apply(f, {}, [])", "reflect-apply"],
  ["function f() { return new.target === g } function g() {}", "Reflect.construct(f, [], g)", "reflect-construct"],
  ["function f() { return new.target }", "Reflect.construct(f, [], Array) === Array", "reflect-array"],
  ["function f() { return typeof new.target }", "new f instanceof f", "typeof"],
  ["function f() { return new.target ? 'new' : 'call' }", "[f(), new f().toString()]", "ternary"],
  ["function f() { return !new.target }", "f()", "bang"],
  ["function f() { return new.target?.name }", "[f(), new f]", "optional"],
  ["function f() { return { nt: new.target } }", "new f().nt === f", "objlit"],
  ["function f() { return [new.target] }", "f()", "arr"],
  ["const f = function () { return new.target }", "[f(), new f() === f]", "fnexpr"],
  ["const o = { m() { return new.target } }", "o.m()", "method"],
  ["const o = { f: function () { return new.target } }", "[o.f(), new o.f() === o.f]", "propfn"],
  ["const o = { get g() { return new.target } }", "o.g", "getter"],
  ["const o = { set s(v) { R2 = new.target } }; var R2 = 1; o.s = 1;", "R2", "setter"],
  ["class A { constructor() { this.t = new.target } }", "new A().t === A", "class"],
  ["class A { constructor() { this.t = new.target } } class B extends A {}", "new B().t === B", "derived"],
  ["class A { constructor() { this.t = new.target } } class B extends A { constructor() { super() } }", "new B().t === B", "derived-explicit"],
  ["class A { constructor() { this.t = new.target } } class B extends A { constructor() { super(); this.u = new.target } }", "[new B().u === B]", "derived-own"],
  ["class A { static m() { return new.target } }", "A.m()", "static"],
  ["class A { m() { return new.target } }", "new A().m()", "proto-method"],
  ["class A { f = new.target }", "new A().f", "field-init"],
  ["class A { static f = new.target }", "A.f", "static-field"],
  ["class A { static { R2 = new.target } } var R2 = 1;", "R2", "static-block"],
  ["class A { f = () => new.target }", "new A().f()", "field-arrow"],
  ["class A { [new.target] = 1 }", "Object.keys(new A)", "computed-key-class-body"],
  ["function f() { class A { [new.target.name]() { return 1 } } return new A().f }", "new f", "computed-key"],
  ["function f() { return class { static x = new.target } }", "f().x", "static-field-in-fn"],
  ["function f() { return new.target }", "new (f.bind(null))", "bound"],
  ["function f() { return new.target }", "new new Proxy(f, {})", "proxy-construct"],
  ["function f() { return new.target }", "new (new Proxy(f, { construct(t, a, n) { return { nt: n } } }))", "proxy-trap"],
  ["function f() { return new.target }", "Reflect.construct(new Proxy(f, {}), [], Object) === Object", "proxy-reflect"],
  ["function* g() { return new.target }", "g().next().value", "generator"],
  ["async function f() { return new.target }", "f()", "async"],
  ["function f() { return Reflect.construct(function () { return new.target }, [], f) === f }", "f()", "reflect-inner"],
  ["function f() { return new.target }", "new f instanceof Object", "instance"],
  ["function f() { return arguments.callee && new.target }", "f()", "callee"],
  ["function f() { 'use strict'; return new.target }", "f()", "strict"],
  ["function f(a = new.target) { return a }", "[f(), new f() === f]", "default-param"],
  ["function f(a = () => new.target) { return a() }", "f()", "default-param-arrow"],
  ["function f(a = new.target === undefined) { return a }", "f()", "default-param-cmp"],
  ["function f({ a = new.target } = {}) { return a }", "[f(), typeof new f()]", "destructure-default"],
];
for (const [decl, expr] of ntContexts) {
  add(`${decl} return ${expr}`);
}
add("return eval('new.target')");
add("return (0, eval)('new.target')");
add("return new Function('return new.target')()");
add("return new (new Function('return new.target'))() instanceof Object");
add("const f = new Function('return new.target'); return Reflect.construct(f, [], Array) === Array");
add("return (() => eval('new.target'))()");
add("return Function('return new.target')");
add("return Function('return () => new.target')()()");
add("function f() { return () => eval('new.target') } return new f()() === f");
add("function f() { try { return (0, eval)('new.target') } catch (e) { return e.name + ': ' + e.message } } return f()");
add("function f() { return new Function('return new.target')() } return f()");
add("function f() { return eval('(function () { return new.target })()') } return f()");
add("function f() { return eval('(() => new.target)()') } return new f() === f");
add("function f() { return new.target.prototype } return new f() === f.prototype");
add("function f() { return Object.getPrototypeOf(this) === new.target.prototype } return new f");
add("function f() { if (!new.target) return new f(); this.made = 1 } return f().made");
add("function f() { if (!new.target) return new f(); this.made = 1 } return f.call({}).made");
add("function f() { return new.target } f.prototype = null; return new f() === f");
add("function f() { return this } f.prototype = null; return Object.getPrototypeOf(new f) === Object.prototype");
add("function f() { return new.target } const g = new Proxy(f, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); return Reflect.construct(f, [], g) === g");
add("function f() { return Object.getPrototypeOf(this) } const g = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); return Reflect.construct(f, [], g) === Array.prototype");
add("function f() { return Object.getPrototypeOf(this) === Array.prototype } function g() {} g.prototype = Array.prototype; return Reflect.construct(f, [], g)");
add("function f() { this.a = 1 } function g() {} g.prototype = 5; return Object.getPrototypeOf(Reflect.construct(f, [], g)) === Object.prototype");
add("function f() { this.a = 1 } function g() {} g.prototype = 'x'; return Object.getPrototypeOf(Reflect.construct(f, [], g)) === Object.prototype");
add("return Reflect.construct(function () {}, [], 1)");
add("return Reflect.construct(function () {}, [], () => {})");
add("return Reflect.construct(() => {}, [])");
add("return Reflect.construct(function () {}, 1)");
add("return Reflect.construct(1, [])");
add("return Reflect.construct(class {}, [], null)");
add("return Reflect.construct(function () {}, [], async function () {})");
add("return Reflect.construct(function () {}, [], { m() {} }.m)");
add("class A {} return Reflect.construct(A, [], function () {}).constructor === Object");
add("class A { constructor() { this.nt = new.target } } function G() {} G.prototype = { tag: 1 }; const o = Reflect.construct(A, [], G); return [o.tag, o.nt === G, o instanceof A]");
add("class A { constructor() { this.nt = new.target } } class B extends A {} function G() {} G.prototype = { tag: 1 }; const o = Reflect.construct(B, [], G); return [o.tag, o.nt === G]");
add("class A {} return Reflect.construct(A, [], Object).constructor === Object ? 'obj' : 'a'");
add("class A {} class B extends A {} return Object.getPrototypeOf(Reflect.construct(A, [], B)) === B.prototype");
add("class A {} class B extends A {} return Object.getPrototypeOf(Reflect.construct(B, [], A)) === A.prototype");
add("class A { constructor() { return {} } } class B extends A {} return Object.getPrototypeOf(Reflect.construct(B, [], B)) === Object.prototype");
add("class B extends Array {} return Object.getPrototypeOf(Reflect.construct(Array, [], B)) === B.prototype");
add("class B extends Array {} return Object.getPrototypeOf(Reflect.construct(Array, [3], B)) === B.prototype");
add("return Object.getPrototypeOf(Reflect.construct(Date, [0], Array)) === Array.prototype");
add("return Object.getPrototypeOf(Reflect.construct(Map, [], Set)) === Set.prototype");
add("return Reflect.construct(Map, [], Set) instanceof Set");
add("return (() => { try { Reflect.construct(Map, [], Set).get(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("const r = Reflect.construct(RegExp, ['a'], Array); return [Object.getPrototypeOf(r) === Array.prototype, Array.isArray(r)]");
add("const r = Reflect.construct(Array, [], RegExp); return [Object.getPrototypeOf(r) === RegExp.prototype, Array.isArray(r)]");
add("const r = Reflect.construct(Error, ['m'], TypeError); return [r.name, r.message, r instanceof TypeError, Object.prototype.toString.call(r)]");
add("const r = Reflect.construct(Promise, [x => x(1)], Map); return [Object.prototype.toString.call(r), r instanceof Map]");
add("return Reflect.construct(Promise, [x => x], Object) instanceof Promise");
add("const f = function () {}; f.prototype = Object.create(null); return Object.getPrototypeOf(new f) === f.prototype");
add("function f() {} const p = new Proxy(f, { construct() { return 1 } }); return new p");
add("function f() {} const p = new Proxy(f, { construct() { return {} } }); return typeof new p");
add("const p = new Proxy({}, {}); return new p");
add("const p = new Proxy(() => {}, {}); return new p");
add("const p = new Proxy(function () {}, { construct: 1 }); return new p");
add("const p = new Proxy(function () {}, { construct: null }); return typeof new p");
add("const p = new Proxy(function () {}, { construct(t, args, nt) { return { n: nt === p, len: args.length } } }); const o = new p(1, 2); return [o.n, o.len]");
add("class A {} const p = new Proxy(A, { construct(t, a, nt) { return Reflect.construct(t, a, nt) } }); class B extends p {} return [new B instanceof B, new B instanceof A]");

// ---- Construtores derivados e retorno.
const returns = [
  "return 1", "return 'a'", "return null", "return undefined", "return {}", "return { x: 1 }", "return []", "return Symbol()",
  "return 1n", "return true", "return function () {}", "return new Date(0)", "return this", "return /r/", "return Object(1)",
  "return new Proxy({}, {})", "return new (class Z {})", "return () => {}", "return class {}", "return NaN", "",
];
for (const r of returns) {
  add(`function F() { ${r} } const o = new F; return [typeof o, o instanceof F]`);
  add(`class A { constructor() { ${r} } } const o = new A; return [typeof o, o instanceof A]`);
  add(`class A {} class B extends A { constructor() { super(); ${r} } } return (() => { const o = new B; return [typeof o, o instanceof B] })()`);
  add(`class A {} class B extends A { constructor() { ${r} } } return (() => { const o = new B; return [typeof o, o instanceof B] })()`);
  add(`class A {} class B extends A { constructor() { try { ${r} } finally { super() } } } return (() => { const o = new B; return [typeof o, o instanceof B] })()`);
  add(`class A { constructor() { ${r} } } class B extends A { constructor() { super(); this.q = 1 } } return (() => { const o = new B; return [typeof o, o instanceof B, o.q] })()`);
  add(`class A { constructor() { ${r} } } class B extends A {} return (() => { const o = new B; return [typeof o, o instanceof B] })()`);
  add(`class B extends null { constructor() { ${r} } } return (() => { const o = new B; return [typeof o] })()`);
}
add("class A {} class B extends A { constructor() { return {}; super() } } return typeof new B");
add("class B extends Object { constructor() { super(1) } } return [new B instanceof B, typeof new B(1)]");
add("class B extends Object { constructor() { super(1); } } return Object.getPrototypeOf(new B) === B.prototype");
add("class B extends Object {} return new B(5) instanceof Number");
add("class B extends Object {} return Object.getPrototypeOf(new B(5)) === B.prototype");
add("class B extends Object {} return typeof new B('s')");
add("class B extends Function {} const f = new B('return 7'); return [f(), f instanceof B, typeof f]");
add("class B extends Function {} const f = new B('a', 'return a + 1'); return [f(1), f.length, f.name]");
add("class B extends Function { constructor() { super('return this') } } return new B()() === globalThis");
add("class B extends Function { constructor() { super('return 5') } m() { return 'm' } } const f = new B; return [f(), f.m()]");
add("class B extends Boolean {} const b = new B(0); return [b.valueOf(), b instanceof Boolean, typeof b]");
add("class B extends Number {} const b = new B(3); return [b + 1, b.toFixed(1), Object.prototype.toString.call(b)]");
add("class B extends String {} const b = new B('ab'); return [b.length, b[1], b + 'c', Object.keys(b)]");
add("class B extends Symbol {} return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends BigInt {} return (() => { try { new B(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Date {} const d = new B(0); return [d.getTime(), d instanceof Date, Object.prototype.toString.call(d)]");
add("class B extends Date { constructor() { super(5) } } return new B().getTime()");
add("class B extends Date {} return typeof B()");
add("class B extends Date {} return (() => { try { return B() } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends WeakMap {} const k = {}; const w = new B([[k, 1]]); return [w.get(k), w instanceof WeakMap]");
add("class B extends WeakSet {} const k = {}; const w = new B([k]); return [w.has(k), w instanceof WeakSet]");
add("class B extends ArrayBuffer {} const b = new B(8); return [b.byteLength, b instanceof ArrayBuffer, b.slice(2) instanceof B]");
add("class B extends Uint8Array {} const b = new B(3); return [b.length, b.map(x => x + 1) instanceof B, b.subarray(1) instanceof B, b.slice() instanceof B]");
add("class B extends DataView {} const b = new B(new ArrayBuffer(4)); return [b.byteLength, b instanceof DataView]");
add("class B extends Proxy {} return 1");
add("class B extends Math {} return 1");
add("class B extends JSON {} return 1");
add("class B extends Reflect {} return 1");
add("class B extends Atomics {} return 1");
add("class B extends (() => {}) {} return 1");
add("class B extends (async function () {}) {} return 1");
add("class B extends (function* () {}) {} return 1");
add("class B extends ({ m() {} }).m {} return 1");
add("class B extends 1 {} return 1");
add("class B extends 'a' {} return 1");
add("class B extends undefined {} return 1");
add("class B extends {} {} return 1");
add("class B extends Symbol.iterator {} return 1");
add("function F() {} F.prototype = 1; class B extends F {} return 1");
add("function F() {} F.prototype = null; class B extends F {} return [Object.getPrototypeOf(B.prototype), Object.getPrototypeOf(B) === F]");
add("function F() {} F.prototype = undefined; class B extends F {} return 1");
add("function F() {} F.prototype = function () {}; class B extends F {} return Object.getPrototypeOf(B.prototype) === F.prototype");
add("class B extends null {} return [Object.getPrototypeOf(B) === Function.prototype, Object.getPrototypeOf(B.prototype)]");
add("class B extends null {} return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends null { constructor() { return Object.create(B.prototype) } } return new B instanceof B");
add("class B extends null { constructor() { super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends null { constructor() { return {} } } return typeof new B");
add("const p = new Proxy(class {}, { get(t, k) { return k === 'prototype' ? null : t[k] } }); class B extends p {} return Object.getPrototypeOf(B.prototype)");
add("let n = 0; const P = { get prototype() { n++; return {} } }; function F() {} Object.defineProperty(F, 'prototype', { get() { n++; return {} } }); class B extends F {} return n");
add("let log = []; class A {} class B extends (log.push('h'), A) { [(log.push('k'), 'm')]() {} static [(log.push('s'), 'n')] = log.push('sf') } return log");
add("class A {} class B extends A { constructor() { super(); return undefined } } return typeof new B");
add("class A {} class B extends A { constructor() { super(); return 1 } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { return 1 } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { return undefined } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { return null } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const o = {}; return o } } return typeof new B");
add("class A {} class B extends A { constructor() { return super() } } return new B instanceof B");
add("class A { constructor() { return { a: 1 } } } class B extends A { constructor() { return super() } } const o = new B; return [o.a, o instanceof B]");
add("class A { constructor() { return 5 } } class B extends A { constructor() { const r = super(); return r } } const o = new B; return [typeof o, o instanceof B]");
add("class A {} class B extends A { constructor() { const r = super(); this.r = r } } const o = new B; return o.r === o");
add("class A {} class B extends A { constructor() { (super(), 1) } } return typeof new B");
add("class A {} class B extends A { constructor() { void super() } } return new B instanceof B");
add("class A {} class B extends A { constructor() { super(), super.constructor } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const f = () => super(); f() } } return new B instanceof B");
add("class A {} class B extends A { constructor() { const f = () => super(); f(); f() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const f = () => this; try { f() } catch (e) { var m = e.message } super(); this.m = m } } return new B().m");
add("class A {} class B extends A { constructor() { const f = () => super(); const g = () => this; f(); this.same = g() === this } } return new B().same");
add("class A {} class B extends A { constructor() { eval('super()') } } return new B instanceof B");
add("class A {} class B extends A { constructor() { eval('super()'); eval('super()') } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { (0, eval)('super()') } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { new Function('super()') } } return 1");
add("class A { constructor() { this.v = 1 } } class B extends A { constructor() { eval('super()'); this.w = eval('this.v + 1') } } return new B().w");
add("class A {} class B extends A { constructor() { eval('this') ; super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const f = () => eval('super()'); f() } } return new B instanceof B");

// ---- super() duas vezes e this antes de super.
add("class A { constructor() { R2.n++ } } const R2 = { n: 0 }; class B extends A { constructor() { super(); super() } } try { new B } catch (e) { return [e.name, e.message, R2.n] }");
add("class A {} class B extends A { constructor() { super(); super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("let n = 0; class A { constructor() { n++ } } class B extends A { constructor() { try { super(); super() } catch (e) {} } } new B; return n");
add("let n = 0; class A { constructor() { n++ } } class B extends A { constructor() { super(); try { super() } catch (e) { n += 10 } } } new B; return n");
add("let n = 0; class A { constructor() { n++ } } class B extends A { constructor() { super(); (() => { try { super() } catch (e) { n += 10 } })() } } new B; return n");
add("class A {} class B extends A { constructor() { this.x = 1; super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { this; super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { typeof this; super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { super(this) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { super(super.x) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { super(); } m() { return super.m } } return new B().m");
add("class A {} class B extends A { constructor() { return } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { if (0) super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor(f) { if (f) super() } } return (() => { try { return typeof new B(1) } catch (e) { return e.name } })()");
add("class A {} class B extends A { constructor() { throw 1 } } return (() => { try { new B } catch (e) { return e } })()");
add("class A { constructor() { throw 2 } } class B extends A { constructor() { try { super() } catch (e) { this } } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { throw 2 } } class B extends A { constructor() { try { super() } catch (e) { super() } } } return (() => { try { new B } catch (e) { return e.name || e } })()");
add("class A {} class B extends A { constructor() { super(); delete this.x; return this } } return new B instanceof B");
add("class A {} class B extends A { constructor() { var self = this } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const o = { m: () => this }; o.m(); super() } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { const o = { m: () => this }; super(); this.ok = o.m() === this } } return new B().ok");
add("class A {} class B extends A { constructor() { const o = { m() { return typeof this } }; super(); this.t = o.m() } } return new B().t");
add("class A {} class B extends A { constructor() { function inner() { return this } super(); this.t = inner() } } return new B().t");
add("class A {} class B extends A { constructor() { super(); this.t = (function () { return this })() } } return new B().t");
add("class A {} class B extends A { constructor() { super(); return () => this } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A {} class B extends A { constructor() { return function () {} } } return typeof new B");
add("class A { constructor() { this.a = new.target.name } } class B extends A { constructor() { super(); this.b = new.target.name } } class C extends B {} const o = new C; return [o.a, o.b]");
add("class A { constructor(...a) { this.a = a } } class B extends A { constructor(...a) { super(...a, 9) } } return new B(1, 2).a");
add("class A { constructor(...a) { this.a = a } } class B extends A {} return new B(1, 2, 3).a");
add("class A { constructor(...a) { this.a = a } } class B extends A { constructor() { super(...arguments) } } return new B(1, 2).a");
add("class A { constructor(a, b) { this.s = [a, b] } } class B extends A { constructor(...x) { super(...x.reverse()) } } return new B(1, 2).s");
add("class A { constructor(a) { this.a = a } } class B extends A { constructor() { super(...[], 3, ...[4]) } } return new B().a");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...new Array(5000).fill(0)) } } return new B().n");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...'abc') } } return new B().n");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...1) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...undefined) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...null) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...{}) } } return (() => { try { new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...new Set([1, 2])) } } return new B().n");
add("class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...function* () { yield 1; yield 2; yield 3 }()) } } return new B().n");
add("let log = []; class A {} class B extends A { constructor() { super(log.push('a'), log.push('b')); log.push('c') } } new B; return log");
add("let log = []; class A { constructor() { log.push('A') } } class B extends A { constructor() { super(log.push('arg')); log.push('B') } } new B; return log");
add("let log = []; class A { constructor() { log.push('A') } } class B extends A { x = log.push('field'); constructor() { log.push('pre'); super(); log.push('post') } } new B; return log");
add("let log = []; class A { constructor() { log.push('A') } } class B extends A { x = log.push('fieldB') } class C extends B { y = log.push('fieldC'); constructor() { super(); log.push('C') } } new C; return log");
add("let log = []; class A { constructor() { this.m() } m() { log.push('A.m') } } class B extends A { f = 1; m() { log.push('B.m ' + this.f) } } new B; return log");
add("class A { constructor() { this.m() } m() {} } class B extends A { f = 1; m() { this.seen = this.f } } return new B().seen");
add("class A { constructor() { this.v = this.g() } g() { return 1 } } class B extends A { g() { return super.g() + 1 } } return new B().v");
add("class A { constructor() { Object.defineProperty(this, 'x', { value: 1 }) } } class B extends A { x = 2 } return (() => { try { return new B().x } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { Object.freeze(this) } } class B extends A { x = 2 } return (() => { try { return new B().x } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { Object.preventExtensions(this) } } class B extends A { x = 2 } return (() => { try { return new B().x } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { return Object.freeze({}) } } class B extends A { x = 2 } return (() => { try { return new B().x } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { return Object.freeze({}) } } class B extends A { #p = 1; static has(o) { return #p in o } } const o = new B; return (() => { try { return B.has(o) } catch (e) { return e.name } })()");
add("class A { constructor() { return Object.freeze({}) } } class B extends A { #p = 1 } return (() => { try { new B; return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor(o) { return o } } class B extends A { #p = 1; static g(o) { return o.#p } } const o = {}; new B(o); return B.g(o)");
add("class A { constructor(o) { return o } } class B extends A { #p = 1 } const o = {}; new B(o); return (() => { try { new B(o); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor(o) { return o } } class B extends A { f = 1 } const o = Object.create(null); new B(o); return [o.f, Object.keys(o)]");
add("class A { constructor(o) { return o } } class B extends A { f = 1 } const o = new Proxy({}, { defineProperty(t, k, d) { return Reflect.defineProperty(t, k, d) } }); new B(o); return o.f");
add("class A { constructor(o) { return o } } class B extends A { f = this } const o = {}; new B(o); return o.f === o");
add("class A { constructor(o) { return o } } class B extends A { f = (() => this)() } const o = {}; new B(o); return o.f === o");
add("class A { constructor(o) { return o } } class B extends A { static g(o) { return new B(o) } } const o = {}; return B.g(o) === o");
add("const log = []; class A { constructor() { log.push(new.target.name) } } class B extends A {} class C extends B {} Reflect.construct(C, [], A); Reflect.construct(A, [], C); return log");

// ---- class fields e static blocks com this.
add("class A { x = this; static s = this } return [new A().x instanceof A, A.s === A]");
add("class A { x = this.constructor.name } return new A().x");
add("class A { x = 1; y = this.x + 1; z = this.y + 1 } return (() => { const o = new A; return [o.x, o.y, o.z] })()");
add("class A { y = this.x; x = 1 } return (() => { const o = new A; return [o.x, o.y] })()");
add("class A { static a = 1; static b = this.a + 1; static c = A.b + 1 } return [A.a, A.b, A.c]");
add("class A { static b = this.a; static a = 1 } return [A.a, A.b]");
add("class A { static a = this.name; } return A.a");
add("class A { static a = () => this } return A.a() === A");
add("class A { static a = function () { return this } } return A.a() === A");
add("class A { static a = function () { return this } } const f = A.a; return f()");
add("class A { x = function () { return this } } const o = new A; return [o.x() === o, (0, o.x)()]");
add("class A { x = () => this } const o = new A; const f = o.x; return f() === o");
add("class A { x = () => () => this } const o = new A; return o.x()() === o");
add("class A { x = { m() { return this } } } const o = new A; return [o.x.m() === o.x, o.x.m() === o]");
add("class A { x = { m: () => this } } const o = new A; return o.x.m() === o");
add("class A { x = class { y = this } } const o = new A; return new o.x().y instanceof o.x");
add("class A { x = class { static y = this } } const o = new A; return o.x.y === o.x");
add("class A { static x = class { y = this } } return new A.x().y instanceof A.x");
add("class A { x = eval('this') } return new A().x instanceof A");
add("class A { static x = eval('this') } return A.x === A");
add("class A { x = eval('() => this') } const o = new A; return o.x() === o");
add("class A { x = eval('new.target') } return new A().x");
add("class A { x = eval('arguments') } return (() => { try { return new A().x } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { x = arguments } return 1");
add("class A { x = () => arguments } return 1");
add("class A { static x = eval('arguments') } return 1");
add("class A { x = eval('var a = 1; a'); } return new A().x");
add("class A { x = eval('var a = 1; a'); y = typeof a } return new A().y");
add("class A { x = super.y } return new A().x");
add("class A { static x = super.y } return A.x");
add("class B { y = 1 } class A extends B { x = super.y } return new A().x");
add("class B { static y = 1 } class A extends B { static x = super.y } return A.x");
add("class B { m() { return 'B' } } class A extends B { x = super.m() } return new A().x");
add("class B { m() { return this } } class A extends B { x = super.m() } return (() => { const o = new A; return o.x === o })()");
add("class B { static m() { return this } } class A extends B { static x = super.m() } return A.x === A");
add("class B { m() { return 'B' } } class A extends B { x = () => super.m() } return new A().x()");
add("class A { ['x' + 1] = 1; static ['y' + 2] = 2 } return [Object.keys(new A), Object.keys(A)]");
add("let n = 0; class A { [n++] = n } return [Object.keys(new A), n]");
add("let n = 0; class A { [n++] = n } new A; new A; return n");
add("class A { [this] = 1 } return 1");
add("const o = { toString() { return 'k' } }; class A { [o] = 1 } return Object.keys(new A)");
add("const o = { toString() { return 'k' } }; let c = 0; const T = { get [Symbol.toPrimitive]() { c++; return () => 'p' } }; class A { [T] = 1 } new A; new A; return [Object.keys(new A), c]");
add("class A { 'a b' = 1; 1 = 2; 0x10 = 3; 1n = 4 } return Object.keys(new A)");
add("class A { static 'a b' = 1; static 1 = 2; static 0x10 = 3 } return Object.keys(A).sort()");
add("class A { x; y; z = 1 } return Object.keys(new A)");
add("class A { x } return Object.getOwnPropertyDescriptor(new A, 'x')");
add("class A { x = 1 } return Object.getOwnPropertyDescriptor(new A, 'x')");
add("class A { static x = 1 } return Object.getOwnPropertyDescriptor(A, 'x')");
add("class A { x = 1; static x = 2 } return [new A().x, A.x]");
add("class A { static name = 'N' } return A.name");
add("class A { static length = 5 } return A.length");
add("class A { static prototype = 1 } return 1");
add("class A { prototype = 1 } return new A().prototype");
add("class A { constructor = 1 } return 1");
add("class A { 'constructor' = 1 } return 1");
add("class A { static constructor = 1 } return A.constructor");
add("class A { static ['prototype'] = 1 } return 1");
add("class A { ['constructor'] = 1 } return Object.keys(new A)");
add("class A { x = 1 } class B extends A { x = this.x + 1 } return new B().x");
add("class A { x = 1; constructor() { this.y = this.x } } return new A().y");
add("class A { x = (this.w = 5) } return (() => { const o = new A; return [o.w, o.x] })()");
add("class A { x = new.target } return [new A().x, Reflect.construct(A, [], Object).x === Object]");
add("class A { x = new A } return (() => { try { new A } catch (e) { return e.name } })()");
add("class A { static x = new A } return A.x instanceof A");
add("class A { static x = new A().y; y = 2 } return A.x");
add("class A { static x = A.y; static y = 2 } return A.x");
add("class A { static x = B.y } class B { static y = 1 } return 1");
add("let log = []; class A { static a = log.push('a'); static { log.push('blk') } static b = log.push('b') } return log");
add("let log = []; class A { static { log.push(this === A) } } return log");
add("class A { static { this.x = 1 } } return A.x");
add("class A { static { this.x = 1 } static y = this.x + 1 } return A.y");
add("class A { static x = 1; static { this.y = this.x + 1 } } return A.y");
add("class A { static { var v = 1; this.v = v } } return [A.v, typeof v]");
add("class A { static { let v = 1; this.v = v } } return [A.v, typeof v]");
add("class A { static { function f() { return this } this.t = f() } } return A.t");
add("class A { static { this.t = (() => this)() } } return A.t === A");
add("class A { static { this.t = { m() { return this } }.m() } } return typeof A.t");
add("class A { static { return } } return 1");
add("class A { static { await } } return 1");
add("class A { static { var await } } return 1");
add("class A { static { yield } } return 1");
add("class A { static { arguments } } return 1");
add("class A { static { super.x } } return 1");
add("class B { static x = 5 } class A extends B { static { this.y = super.x } } return A.y");
add("class A { static { new.target } } return 1");
add("class A { static { R2 = new.target } } var R2 = 1; return R2");
add("class A { static { eval('this.z = 1') } } return A.z");
add("class A { static { try { throw 1 } catch (e) { this.e = e } } } return A.e");
add("class A { static { this.a = 1 } static { this.b = this.a + 1 } static { this.c = this.b + 1 } } return [A.a, A.b, A.c]");
add("class A { static { throw new Error('boom') } } return 1");
add("let n = 0; try { class A { static { n++; throw 1 } } } catch (e) {} try { class A { static { n++; throw 1 } } } catch (e) {} return n");
add("class A { static #p = 1; static { this.r = A.#p } } return A.r");
add("class A { static #p = 1; static { this.r = this.#p } } return A.r");
add("class A { static { this.f = () => A.#p } static #p = 2 } return A.f()");
add("class A { static { A.#p = 1 } static #p } return 1");
add("class A { static { this.#p } static #p = 1 } return (() => { return 1 })()");
add("class A { static { this.r = this.#p } static #p = 1 } return A.r");
add("class A { static { this.r = this.#m() } static #m() { return 'm' } } return A.r");
add("class A { static { this.r = this.#g } static get #g() { return 'g' } } return A.r");
add("class A { static #p = 1; static m() { return this.#p } } class B extends A {} return (() => { try { return B.m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { static #p = 1; static m() { return A.#p } } class B extends A {} return B.m()");
add("class A { static #m() { return 1 } static m() { return this.#m() } } class B extends A {} return (() => { try { return B.m() } catch (e) { return e.name + ': ' + e.message } })()");

// ---- Métodos privados, #x in obj, accessors privados.
add("class A { #m() { return 1 } t() { return this.#m() } } return new A().t()");
add("class A { #m() { return this } t() { return this.#m() === this } } return new A().t()");
add("class A { #m() { return 1 } static t(o) { return o.#m() } } return (() => { try { return A.t({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() { return 1 } t() { this.#m = 2 } } return (() => { try { new A().t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() { return 1 } t() { return typeof this.#m } } return new A().t()");
add("class A { #m() { return 1 } t() { return this.#m.name } } return new A().t()");
add("class A { #m() { return 1 } t() { return this.#m.length } } return new A().t()");
add("class A { #m(a, b) { return 1 } t() { return this.#m.length } } return new A().t()");
add("class A { #m() { return 1 } t() { return this.#m === this.#m } } return new A().t()");
add("class A { #m() { return 1 } t() { return this.#m } } return new A().t() === new A().t()");
add("class A { #m() { return 1 } t() { return Object.getOwnPropertyNames(this.#m).sort() } } return new A().t()");
add("class A { #m() { return 1 } t() { return new this.#m() } } return (() => { try { return new A().t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() { return 1 } t() { return this.#m.call(2) } } return new A().t()");
add("class A { #m() { return typeof this } t() { return this.#m.call(2) } } return new A().t()");
add("class A { #m() { return arguments.length } t() { return this.#m(1, 2) } } return new A().t()");
add("class A { *#g() { yield 1; yield 2 } t() { return [...this.#g()] } } return new A().t()");
add("class A { async #a() { return 5 } t() { return this.#a() } } let v; new A().t().then(x => globalThis.R = S(x)); return 'pending'");
add("class A { static #m() { return 1 } static t() { return A.#m() } } return A.t()");
add("class A { static #m() { return this } static t() { return A.#m() === A } } return A.t()");
add("class A { static #m() { return 1 } static t() { return this.#m() } } return A.t.call({})");
add("class A { #x = 1; static has(o) { return #x in o } } return [A.has(new A), A.has({}), A.has(A)]");
add("class A { #x = 1; static has(o) { return #x in o } } return (() => { try { return A.has(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static has(o) { return #x in o } } return (() => { try { return A.has(null) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static has(o) { return #x in o } } return (() => { try { return A.has('s') } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static has(o) { return #x in o } } return (() => { try { return A.has(Symbol()) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static has(o) { return #x in o } } return A.has(function () {})");
add("class A { #x = 1; static has(o) { return #x in o } } return A.has(new Proxy(new A, {}))");
add("class A { #x = 1; static has(o) { return #x in o } } return A.has(Object.create(new A))");
add("class A { #x = 1; static has(o) { return #x in o } } class B extends A {} return [A.has(new B), B.has(new A)]");
add("class A { #m() {} static has(o) { return #m in o } } return [A.has(new A), A.has({})]");
add("class A { static #m() {} static has(o) { return #m in o } } return [A.has(A), A.has({}), A.has(class extends A {})]");
add("class A { get #g() { return 1 } static has(o) { return #g in o } } return [A.has(new A), A.has({})]");
add("class A { static #s = 1; static has(o) { return #s in o } } return [A.has(A), A.has(new A)]");
add("class A { #x; static has(o) { return #x in o } } return A.has(new A)");
add("class A { #x; static has(o) { return !(#x in o) } } return A.has({})");
add("class A { #x; static has(o) { return #x in o in o } } return 1");
add("class A { #x; static has(o) { return (#x in o) in { true: 1 } } } return A.has(new A)");
add("class A { #x; static has(o) { return #x in o ? 1 : 2 } } return [A.has(new A), A.has({})]");
add("class A { #x; static has(o) { return #x in #x in o } } return 1");
add("class A { #x; static has() { return #x } } return 1");
add("class A { #x; static has(o) { return 1 + #x in o } } return 1");
add("class A { #x; static has(o) { return (#x) in o } } return 1");
add("class A { #x; static has(o) { return #y in o } } return 1");
add("class A { #x; #x } return 1");
add("class A { #x; static #x } return 1");
add("class A { get #x() {} set #x(v) {} } return 1");
add("class A { get #x() {} get #x() {} } return 1");
add("class A { static get #x() {} set #x(v) {} } return 1");
add("class A { #x; get #x() {} } return 1");
add("class A { #constructor } return 1");
add("class A { #m() {} m() { delete this.#m } } return 1");
add("class A { #m() {} m() { return this?.#m } } return typeof new A().m()");
add("class A { #m() { return 1 } m(o) { return o?.#m() } } return [new A().m(null), new A().m(new A), new A().m(undefined)]");
add("class A { #x = 1; m(o) { return o?.#x } } return [new A().m(null), new A().m(new A)]");
add("class A { #x = 1; m(o) { return o?.a.#x } } return new A().m(null)");
add("class A { #x = 1; m(o) { return o.#x?.y } } return new A().m(new A)");
add("class A { #x = 1; m(o) { return (o?.a).#x } } return (() => { try { return new A().m(null) } catch (e) { return e.name } })()");
add("class A { #x = 1; m() { return this.#x++ + ++this.#x } } return new A().m()");
add("class A { #x = 1; m() { this.#x += 2; this.#x **= 2; return this.#x } } return new A().m()");
add("class A { #x = null; m() { this.#x ??= 5; return this.#x } } return new A().m()");
add("class A { #x = 0; m() { this.#x ||= 5; return this.#x } } return new A().m()");
add("class A { #x = 1; m() { this.#x &&= 5; return this.#x } } return new A().m()");
add("class A { #x = 1; m() { [this.#x] = [7]; return this.#x } } return new A().m()");
add("class A { #x = 1; m() { ({ a: this.#x } = { a: 8 }); return this.#x } } return new A().m()");
add("class A { #x = 1; m() { for (this.#x of [9]); return this.#x } } return new A().m()");
add("class A { #x = 1; m() { for (this.#x in { k: 1 }); return this.#x } } return new A().m()");
add("class A { #x = 1; m() { return delete this.#x } } return 1");
add("class A { #x = 1; m() { return `${this.#x}` } } return new A().m()");
add("class A { #x = 1; m() { return { [this.#x]: 1 } } } return Object.keys(new A().m())");
add("class A { #x = 1; m() { return this.#x ? 'y' : 'n' } } return new A().m()");
add("class A { #x = 1; m() { return typeof this.#x } } return new A().m()");
add("class A { #x = 1; m() { return JSON.stringify(this) } } return new A().m()");
add("class A { #x = 1; m() { return Object.keys(this) } } return new A().m()");
add("class A { #x = 1; m() { return Reflect.ownKeys(this) } } return new A().m()");
add("class A { #x = 1 } return Object.getOwnPropertyNames(new A)");
add("class A { #x = 1 } const o = new A; Object.freeze(o); return Object.isFrozen(o)");
add("class A { #x = 1; set(v) { this.#x = v; return this.#x } } const o = Object.freeze(new A); return o.set(5)");
add("class A { #x = 1; static get(o) { return o.#x } } const o = new A; return A.get(new Proxy(o, {}))");
add("class A { #x = 1; static get(o) { return o.#x } } return (() => { try { return A.get(new Proxy(new A, {})) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static get(o) { return o.#x } } return (() => { try { return A.get(Object.create(new A)) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static get(o) { return o.#x } } return (() => { try { return A.get(undefined) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static get(o) { return o.#x } } return (() => { try { return A.get(null) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static get(o) { return o.#x } } return (() => { try { return A.get(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static set(o) { o.#x = 1 } } return (() => { try { return A.set({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static set(o) { o.#x = 1 } } return (() => { try { return A.set(undefined) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; static inc(o) { o.#x++ } } return (() => { try { return A.inc({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() {} static call(o) { o.#m() } } return (() => { try { return A.call({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() {} static call(o) { o.#m() } } return (() => { try { return A.call(null) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() {} static call(o) { return o.#m } } return (() => { try { return A.call({}) } catch (e) { return e.name + ': ' + e.message } })()");
// accessors privados
add("class A { get #g() { return 1 } t() { return this.#g } } return new A().t()");
add("class A { set #s(v) { this.v = v } t() { this.#s = 4; return this.v } } return new A().t()");
add("class A { get #g() { return 1 } t() { this.#g = 2 } } return (() => { try { new A().t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { set #s(v) {} t() { return this.#s } } return (() => { try { new A().t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { get #g() { return 1 } set #g(v) { this.v = v } t() { this.#g = 9; return [this.#g, this.v] } } return new A().t()");
add("class A { get #g() { return 1 } set #g(v) { this.v = v } t() { this.#g++; return this.v } } return new A().t()");
add("class A { get #g() { return 1 } set #g(v) { this.v = v } t() { this.#g += 5; return this.v } } return new A().t()");
add("class A { get #g() { return null } set #g(v) { this.v = v } t() { this.#g ??= 5; return this.v } } return new A().t()");
add("class A { get #g() { return 1 } t() { this.#g++ } } return (() => { try { new A().t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { get #g() { return this } t() { return this.#g === this } } return new A().t()");
add("class A { get #g() { return 1 } static t(o) { return o.#g } } return (() => { try { return A.t({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { set #s(v) {} static t(o) { o.#s = 1 } } return (() => { try { return A.t({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { static get #g() { return 'sg' } static t() { return this.#g } } return A.t()");
add("class A { static set #s(v) { this.v = v } static t() { this.#s = 3; return this.v } } return A.t()");
add("class A { static get #g() { return 'sg' } static t() { return this.#g } } class B extends A {} return (() => { try { return B.t() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { get #g() { return 1 } t() { return Object.getOwnPropertyDescriptor(this, '#g') } } return new A().t()");
add("class A { get #g() { return 1 } t() { return this.#g.toFixed(1) } } return new A().t()");
add("class A { get #g() { return () => this } t() { return this.#g() === this } } return new A().t()");
add("class A { get #g() { return function () { return this } } t() { return this.#g() === this } } return new A().t()");
add("class A { get #g() { throw new Error('g') } t() { try { this.#g } catch (e) { return e.message } } } return new A().t()");
add("class A { get #g() { return 1 } get g() { return this.#g } } class B extends A { get #g() { return 2 } get h() { return this.#g } } const b = new B; return [b.g, b.h]");
add("class A { #p = 1; get p() { return this.#p } } class B extends A { #p = 2; get q() { return this.#p } } const b = new B; return [b.p, b.q]");
add("class A { #m() { return 'A' } a() { return this.#m() } } class B extends A { #m() { return 'B' } b() { return this.#m() } } const b = new B; return [b.a(), b.b()]");
add("class A { #x = 1; static f(o) { return o.#x } } class B { #x = 2; static f(o) { return o.#x } } return [A.f(new A), B.f(new B), (() => { try { return A.f(new B) } catch (e) { return e.name + ': ' + e.message } })()]");
add("function mk() { return class { #x = 1; static get(o) { return o.#x } } } const A = mk(), B = mk(); return [A.get(new A), (() => { try { return A.get(new B) } catch (e) { return e.name + ': ' + e.message } })()]");
add("function mk() { return class { #x = 1; static has(o) { return #x in o } } } const A = mk(), B = mk(); return [A.has(new A), A.has(new B)]");
add("class A { #x = 1; m() { class B { n(o) { return o.#x } } return new B().n(this) } } return new A().m()");
add("class A { #x = 1; m() { return (() => this.#x)() } } return new A().m()");
add("class A { #x = 1; m() { return (function () { return this.#x }).call(this) } } return new A().m()");
add("class A { #x = 1; m() { return eval('this.#x') } } return new A().m()");
add("class A { #x = 1; m() { return eval('() => this.#x')() } } return new A().m()");
add("class A { #x = 1; m() { return eval('#x in this') } } return new A().m()");
add("class A { m() { return eval('this.#y') } } return 1");
add("class A { #x = 1; m() { return (0, eval)('this.#x') } } return 1");
add("class A { #x = 1; m() { return new Function('return this.#x') } } return 1");
add("class A { #x = 1; static f = o => o.#x } return A.f(new A)");
add("class A { #x = 1; f = () => this.#x } return new A().f()");
add("class A { #x = 1; #y = this.#x + 1; z = this.#y } return new A().z");
add("class A { #y = this.#x; #x = 1 } return (() => { try { new A } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #m() { return 1 } #x = this.#m() } return new A() instanceof A");
add("class A { #x = this.#m(); #m() { return 1 } } return new A() instanceof A");
add("class A { #x = this.#g; get #g() { return 7 } y = this.#x } return new A().y");
add("class A { x = this.#m(); #m() { return 2 } } return new A().x");
add("class A { static x = A.#m(); static #m() { return 3 } } return A.x");
add("class A { static #m() { return 3 } static x = A.#m() } return A.x");
add("class A { static x = A.#p; static #p = 3 } return 1");
add("class A { static x = this.#p; static #p = 3 } return 1");
add("let log = []; class A { #a = log.push('a'); b = log.push('b'); #c = log.push('c'); d = log.push('d') } new A; return log");
add("let log = []; class A { constructor() { log.push('ctor') } #a = log.push('a'); static s = log.push('s') } new A; return log");
add("class A { #x = 1; static eq(a, b) { return a.#x === b.#x } } return A.eq(new A, new A)");
add("class A { #x = {}; static eq(a, b) { return a.#x === b.#x } } return A.eq(new A, new A)");
add("class A { #x = 1; m() { return this.#x } } const m = new A().m; return (() => { try { return m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; m() { return this.#x } } return (() => { try { return new A().m.call({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; m() { return this.#x } } return new A().m.call(new Proxy(new A, {}))");
add("class A { #x = 1; m() { return this.#x } } return (() => { try { return new A().m.call(new Proxy(new A, {})) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; m() { return this.#x } } class B extends A {} return new B().m()");
add("class A { #x = 1; m() { return this.#x } } return (() => { try { return Object.create(new A).m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; m() { return this.#x } } return (() => { try { return Object.setPrototypeOf({}, A.prototype).m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1 } return Object.getPrototypeOf(new A) === A.prototype");
add("class A { #x = 1; static clone(o) { return Object.assign(Object.create(A.prototype), o) } m() { return this.#x } } return (() => { try { return A.clone(new A).m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { #x = 1; m() { return this.#x } } return (() => { try { return structuredClone ? 1 : 0 } catch (e) { return e.name } })()");

// ---- Herança de built-ins.
// Array
add("class B extends Array {} const b = new B(3); return [b.length, b instanceof B, Array.isArray(b), b.constructor === B]");
add("class B extends Array {} const b = B.from([1, 2, 3]); return [b instanceof B, b.length]");
add("class B extends Array {} const b = B.of(1, 2); return [b instanceof B, b.length]");
add("class B extends Array {} const b = new B(1, 2, 3); return [b.length, b.map(x => x * 2) instanceof B, b.filter(x => x > 1) instanceof B]");
add("class B extends Array {} const b = new B(1, 2, 3); return [b.slice(1) instanceof B, b.splice(0, 1) instanceof B, b.concat([4]) instanceof B]");
add("class B extends Array {} const b = new B(1, 2, 3); return [b.flat() instanceof B, b.flatMap(x => [x]) instanceof B, b.toSorted() instanceof B]");
add("class B extends Array {} const b = new B(1, 2, 3); return [b.toReversed() instanceof B, b.toSpliced(0, 1) instanceof B, b.with(0, 9) instanceof B]");
add("class B extends Array {} const b = new B(1, 2, 3); return [b.reverse() === b, b.sort() === b, b.fill(0) === b]");
add("class B extends Array {} const b = new B(); b.push(1, 2); b[5] = 1; return [b.length, JSON.stringify(b), Object.keys(b)]");
add("class B extends Array {} const b = new B(); b.length = 3; return [b.length, b instanceof B]");
add("class B extends Array {} return [new B(0).length, new B(5).length, new B('5').length, new B(1, 2).length]");
add("class B extends Array {} return (() => { try { return new B(-1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array {} return (() => { try { return new B(1.5) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array { constructor() { super(); this.tag = 1 } } const b = new B; b.push(1); return [b.tag, b.length, b.map(x => x).tag]");
add("class B extends Array { constructor(...a) { super(...a); this.tag = 1 } } const b = new B(1, 2); return [b.map(x => x).length, b.map(x => x).tag, b.slice().tag]");
add("class B extends Array { constructor(n) { super(n); this.n = n } } const b = new B(3); return [b.map(x => x).n, b.slice(1).n, b.filter(() => 1).n]");
add("class B extends Array { constructor(n) { super(n); this.n = n } } const b = new B(3); return [b.concat([1]).n, b.flat().n]");
add("class B extends Array { static get [Symbol.species]() { return Array } } const b = new B(1, 2); return [b.map(x => x) instanceof B, b.map(x => x) instanceof Array, b.filter(Boolean).constructor === Array]");
add("class B extends Array {} const b = new B(1, 2); return [b.length, Object.getOwnPropertyDescriptor(b, 'length').writable, B.length, B.name]");
add("class B extends Array {} const b = new B(1, 2); return Object.getOwnPropertyNames(b)");
add("class B extends Array {} return [Object.getPrototypeOf(B) === Array, Object.getPrototypeOf(B.prototype) === Array.prototype, B.prototype.constructor === B]");
add("class B extends Array {} const b = new B(1, 2); return [String(b), b + '', `${b}`, b.join('-'), Object.prototype.toString.call(b)]");
add("class B extends Array {} const b = new B(1, 2); b.length = 0; return [b.length, b instanceof B]");
add("class B extends Array {} return JSON.stringify([new B(1, 2), { b: new B(3) }])");
add("class B extends Array {} return [...new B(1, 2)]");
add("class B extends Array {} const [a, c] = new B(1, 2); return [a, c]");
add("class B extends Array {} const b = new B(1, 2); return Array.prototype.concat.call([], b)");
add("class B extends Array {} const b = new B(1, 2); b[Symbol.isConcatSpreadable] = false; return [].concat(b).length");
add("class B extends Array { get length() { return 5 } } return (() => { try { return new B().length } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array { static get [Symbol.species]() { return null } } return new B(1, 2).map(x => x) instanceof Array");
add("class B extends Array { static get [Symbol.species]() { return undefined } } return new B(1, 2).map(x => x) instanceof B");
add("class B extends Array { static get [Symbol.species]() { return 1 } } return (() => { try { return new B(1, 2).map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array { static get [Symbol.species]() { return function () { return { length: 0 } } } } return Object.prototype.toString.call(new B(1, 2).map(x => x))");
add("class B extends Array { static get [Symbol.species]() { return function (n) { return new Array(n).fill('s') } } } return new B(1, 2).map(x => x)");
add("class B extends Array {} B.prototype.constructor = undefined; return Array.isArray(new B(1, 2).map(x => x))");
add("class B extends Array {} B.prototype.constructor = null; return (() => { try { return new B(1, 2).map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array {} const b = new B(1, 2); b.constructor = { [Symbol.species]: function (n) { this.length = n; this.z = 1 } }; return b.map(x => x).z");
add("class B extends Array {} const b = new B(1, 2); b.constructor = 5; return (() => { try { return b.map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array {} const b = new B(1, 2); b.constructor = Object; return Object.prototype.toString.call(b.map(x => x))");
add("const a = [1, 2]; a.constructor = Array; return a.map(x => x) instanceof Array");
add("const a = [1, 2]; a.constructor = undefined; return a.map(x => x).length");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: null }; return a.map(x => x).length");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function () { return { length: 0, 0: 'x' } } }; return Object.keys(a.map(x => x))");
add("const a = [1, 2]; let args; a.constructor = { [Symbol.species]: function (...r) { args = r; return [] } }; a.map(x => x); a.filter(x => x); a.slice(); a.splice(0, 0); a.concat(); a.flat(); a.flatMap(x => x); return args");
add("const a = [1, 2, 3]; const calls = []; a.constructor = { [Symbol.species]: function (...r) { calls.push(r); return [] } }; a.map(x => x); a.filter(x => x); a.slice(1); a.splice(0, 1); a.concat(); return calls");
add("class B extends Array {} return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.call(5)");
add("return [Array[Symbol.species] === Array, Map[Symbol.species] === Map, Set[Symbol.species] === Set, Promise[Symbol.species] === Promise, RegExp[Symbol.species] === RegExp, ArrayBuffer[Symbol.species] === ArrayBuffer]");
add("return [Object.getOwnPropertyDescriptor(Array, Symbol.species).get.name, Object.getOwnPropertyDescriptor(Array, Symbol.species).set]");
add("const d = Object.getOwnPropertyDescriptor(Array, Symbol.species); return [d.configurable, d.enumerable, typeof d.get]");
add("class B extends Array {} return [B[Symbol.species] === B, Object.hasOwn(B, Symbol.species)]");
add("class B extends Map {} return [B[Symbol.species] === B]");
add("class B extends Uint8Array {} return [B[Symbol.species] === B]");
add("return Object.getPrototypeOf(Uint8Array)[Symbol.species] === Object.getPrototypeOf(Uint8Array)");
// Error
add("class E extends Error {} const e = new E('m'); return [e.message, e.name, e instanceof E, e instanceof Error, String(e), Object.prototype.toString.call(e)]");
add("class E extends Error { constructor(m) { super(m); this.name = 'E' } } const e = new E('m'); return [String(e), e.name, Object.keys(e)]");
add("class E extends Error { get name() { return 'G' } } return String(new E('m'))");
add("class E extends Error {} E.prototype.name = 'P'; return String(new E('m'))");
add("class E extends Error {} return [Object.getOwnPropertyNames(new E('m')).sort(), Object.getOwnPropertyNames(new E).sort()]");
add("class E extends Error {} return [new E().message, new E(undefined).message, new E(null).message, new E(1).message, new E({}).message]");
add("class E extends Error {} const e = new E('m', { cause: 5 }); return [e.cause, Object.getOwnPropertyDescriptor(e, 'cause').enumerable]");
add("class E extends Error {} return Object.hasOwn(new E('m', {}), 'cause')");
add("class E extends Error {} return Object.hasOwn(new E('m', { cause: undefined }), 'cause')");
add("class E extends Error { constructor() { super('x', { cause: 1 }) } } return new E().cause");
add("class E extends Error {} return typeof new E('m').stack");
add("class E extends Error {} return E.prototype.hasOwnProperty('message')");
add("class E extends Error {} return [E.captureStackTrace === Error.captureStackTrace, typeof E.captureStackTrace]");
add("class E extends TypeError {} const e = new E('t'); return [e.name, e instanceof TypeError, e instanceof E, String(e)]");
add("class E extends RangeError {} return [new E('r').name, Object.getPrototypeOf(E) === RangeError]");
add("class E extends SyntaxError {} return new E('s') instanceof SyntaxError");
add("class E extends EvalError {} return new E('s') instanceof EvalError");
add("class E extends URIError {} return new E('s') instanceof URIError");
add("class E extends ReferenceError {} return new E('s') instanceof ReferenceError");
add("class E extends AggregateError {} const e = new E([1, 2], 'm'); return [e.errors, e.message, e.name, e instanceof AggregateError]");
add("class E extends AggregateError {} return (() => { try { return new E() } catch (e) { return e.name + ': ' + e.message } })()");
add("class E extends AggregateError {} return new E([], 'm', { cause: 1 }).cause");
add("class E extends Error { constructor() { return {} } } return typeof new E");
add("class E extends Error { constructor(m) { super(m); return Object.create(E.prototype) } } return [new E('a').message, new E('a') instanceof E]");
add("class E extends Error {} return Error.call(new E('a'), 'b') instanceof Error");
add("class E extends Error {} return Object.getPrototypeOf(Error('x')) === Error.prototype");
add("class E extends Error {} return (() => { try { return E('x') } catch (e) { return e.name + ': ' + e.message } })()");
add("function E(m) { Error.call(this, m); this.message = m } E.prototype = Object.create(Error.prototype); E.prototype.constructor = E; E.prototype.name = 'E'; const e = new E('hi'); return [String(e), e instanceof Error, Object.prototype.toString.call(e)]");
add("function E(m) { const e = Reflect.construct(Error, [m], E); return e } E.prototype = Object.create(Error.prototype); const e = new E('z'); return [e.message, e instanceof E, Object.prototype.toString.call(e)]");
add("class E extends Error {} const e = new E('a'); e.name = 'X'; return String(e)");
add("class E extends Error { toString() { return 'custom' } } return `${new E('a')}`");
add("class E extends Error {} return Object.prototype.hasOwnProperty.call(new E, 'name')");
add("class E extends Error { static name = 'Z' } return String(new E('m'))");
add("class E extends Error { name = 'F' } return [String(new E('m')), Object.keys(new E('m'))]");
add("class E extends Error { message = 'F' } return [new E('m').message, Object.keys(new E('m'))]");
add("class E extends Error {} return Object.getPrototypeOf(E.prototype) === Error.prototype && E.prototype.constructor === E");
// Promise
add("class P extends Promise {} const p = new P(r => r(1)); return [p instanceof P, p instanceof Promise, p.then(() => {}) instanceof P, p.catch(() => {}) instanceof P, p.finally(() => {}) instanceof P]");
add("class P extends Promise {} return [P.resolve(1) instanceof P, P.reject(1).catch(() => {}) instanceof P, P.all([]) instanceof P, P.race([]) instanceof P, P.allSettled([]) instanceof P, P.any([]).catch(() => {}) instanceof P]");
add("class P extends Promise {} return [Promise.resolve.call(P, 1).constructor === P, Promise.resolve(new P(r => r())) instanceof P]");
add("class P extends Promise {} const p = new P(r => r()); return Promise.resolve(p) === p");
add("class P extends Promise {} const p = new P(r => r()); return P.resolve(p) === p");
add("class P extends Promise {} const p = Promise.resolve(); return P.resolve(p) === p");
add("class P extends Promise {} return (() => { try { return P(r => r()) } catch (e) { return e.name + ': ' + e.message } })()");
add("class P extends Promise { constructor(ex) { super(ex); this.tag = 't' } } const p = new P(r => r()); return [p.tag, p.then(() => {}).tag]");
add("class P extends Promise { static get [Symbol.species]() { return Promise } } const p = new P(r => r()); return [p.then(() => {}) instanceof P, p.then(() => {}) instanceof Promise]");
add("class P extends Promise { static get [Symbol.species]() { return undefined } } const p = new P(r => r()); return p.then(() => {}) instanceof P");
add("class P extends Promise { static get [Symbol.species]() { return null } } const p = new P(r => r()); return p.then(() => {}) instanceof P");
add("class P extends Promise { static get [Symbol.species]() { return 1 } } const p = new P(r => r()); return (() => { try { return p.then(() => {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class P extends Promise { constructor(ex) { super(ex); } } let n = 0; const O = class extends P { constructor(ex) { n++; super(ex) } }; const p = new O(r => r()); p.then(() => {}); p.catch(() => {}); p.finally(() => {}); return n");
add("let n = 0; class P extends Promise { constructor(ex) { n++; super(ex) } } P.resolve(1); P.all([1, 2]); P.race([1]); return n");
add("let n = 0; class P extends Promise { constructor(ex) { n++; super(ex) } } const p = P.resolve(1); p.then(); return n");
add("class P extends Promise { constructor(ex) { super(() => {}) } } return (() => { try { return new P(r => r(1)).then(() => {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("class P extends Promise { constructor(ex) { super(ex); ex = null } } return (() => { try { return P.resolve(1) instanceof P } catch (e) { return e.name } })()");
add("class P extends Promise { constructor(ex) { super((res, rej) => ex(res, rej)); } } return P.resolve(1) instanceof P");
add("class P extends Promise { constructor(ex) { ex(() => {}, () => {}); super(() => {}) } } return (() => { try { return new P(() => {}) instanceof P } catch (e) { return e.name } })()");
add("class P extends Promise { constructor() { super(() => {}) } } return (() => { try { P.resolve(1); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("class P extends Promise { constructor(ex) { super(ex) } } return (() => { try { return Promise.resolve.call({}, 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.resolve.call(1, 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.resolve.call(function () {}, 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.resolve.call(function (ex) { ex(1, 1) }, 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.resolve.call(function (ex) { ex(() => {}, () => {}) }, 1).constructor === Object } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { const f = function (ex) { ex(() => {}, () => {}) }; f.resolve = 1; return Promise.all.call(f, []) instanceof f } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.all.call(function (ex) { ex(() => {}, () => {}) }, []) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { const f = function (ex) { ex(() => {}, () => {}) }; f.resolve = () => {}; return Object.prototype.toString.call(Promise.all.call(f, [1])) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Promise.race.call(function (ex) { ex(() => {}, () => {}) }, []) instanceof Object } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { Promise.race.call(function (ex) { ex(() => {}, () => {}) }, [1]) } catch (e) { return e.name + ': ' + e.message } })()");
add("const p = Promise.resolve(); p.constructor = { [Symbol.species]: function (ex) { ex(() => {}, () => {}); this.then = () => {} } }; return p.then(() => {}).constructor === Object");
add("const p = Promise.resolve(); p.constructor = undefined; return p.then(() => {}) instanceof Promise");
add("const p = Promise.resolve(); p.constructor = 1; return (() => { try { return p.then(() => {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("const p = Promise.resolve(); p.constructor = { [Symbol.species]: 1 }; return (() => { try { return p.then(() => {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("const p = Promise.resolve(); p.constructor = { [Symbol.species]: null }; return p.then(() => {}) instanceof Promise");
add("class P extends Promise {} const p = new P(r => r()); p.then = function () { return 'overridden' }; return [Promise.resolve(p) === p, P.resolve(p) === p]");
add("const t = Promise.resolve(1); t.then = function (a) { a(2) }; return Promise.resolve(t) === t");
addAsync("class P extends Promise {} P.resolve(1).then(v => { globalThis.R = S([v, 'then']) })");
addAsync("class P extends Promise {} P.all([1, P.resolve(2)]).then(v => { globalThis.R = S(v) })");
addAsync("class P extends Promise {} const log = []; P.resolve(1).then(() => log.push('a')); P.resolve(2).then(() => log.push('b')); Promise.resolve().then(() => 0).then(() => 0).then(() => { globalThis.R = S(log) })");
addAsync("class P extends Promise { then(a, b) { globalThis.R = 'then called'; return super.then(a, b) } } P.resolve(1).finally(() => {})");
addAsync("class P extends Promise { then(a, b) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.then(a, b) } } P.resolve(1).finally(() => {}).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { then(a, b) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.then(a, b) } } P.all([P.resolve(1)]).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { static resolve(v) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.resolve(v) } } P.all([1, 2, 3]).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { static resolve(v) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.resolve(v) } } P.race([1, 2, 3]).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { static resolve(v) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.resolve(v) } } P.any([1, 2]).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { static resolve(v) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.resolve(v) } } P.allSettled([1, 2]).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise { static resolve(v) { (globalThis.cnt = (globalThis.cnt || 0) + 1); return super.resolve(v) } } P.resolve(1).finally(() => {}).then(() => { globalThis.R = S(globalThis.cnt) })");
addAsync("class P extends Promise {} Promise.all.call(P, [1]).then(v => { globalThis.R = S([v, 'ok']) })");
addAsync("class P extends Promise {} const p = P.reject(new Error('x')); p.catch(e => { globalThis.R = S([e.message, p instanceof P]) })");
addAsync("class P extends Promise {} P.any([P.reject(1), P.reject(2)]).catch(e => { globalThis.R = S([e.name, e.errors, e instanceof AggregateError]) })");
addAsync("class P extends Promise {} P.withResolvers && (() => { const w = P.withResolvers(); w.resolve(4); w.promise.then(v => { globalThis.R = S([v, w.promise instanceof P]) }) })()");
addAsync("const w = Promise.withResolvers(); w.resolve(3); w.promise.then(v => { globalThis.R = S([v, Object.keys(w)]) })");
addAsync("(async () => { class P extends Promise {} const v = await new P(r => r(5)); globalThis.R = S(v) })()");
addAsync("(async () => { class P extends Promise {} const p = new P(r => r(5)); const q = (async () => p)(); globalThis.R = S([q instanceof P, q instanceof Promise]) })()");
addAsync("(async () => { class P extends Promise { then(a, b) { globalThis.cnt = (globalThis.cnt || 0) + 1; return super.then(a, b) } } await new P(r => r(5)); globalThis.R = S(globalThis.cnt) })()");
addAsync("(async () => { const t = { then(a) { a(7) } }; globalThis.R = S(await t) })()");
addAsync("(async () => { class P extends Promise {} const p = new P(r => r(1)); const q = Promise.resolve(p); globalThis.R = S([q === p]) })()");
// Map, Set, WeakMap
add("class M extends Map {} const m = new M([[1, 2]]); return [m.get(1), m.size, m instanceof M, Object.prototype.toString.call(m), m.set(3, 4) === m]");
add("class M extends Map { set(k, v) { return super.set(k, v * 2) } } const m = new M([[1, 2]]); return [m.get(1)]");
add("class M extends Map { set(k, v) { return super.set(k, v * 2) } } const m = new M(); m.set(1, 1); return [m.get(1)]");
add("class M extends Map { set(k, v) { (globalThis.n = (globalThis.n || 0) + 1); return super.set(k, v) } } new M([[1, 2], [3, 4]]); return globalThis.n");
add("class M extends Map { set = 1 } return (() => { try { return new M([[1, 2]]) } catch (e) { return e.name + ': ' + e.message } })()");
add("class M extends Map { constructor() { super(); this.tag = 1 } } const m = new M; return [m.tag, m.size]");
add("class M extends Map { constructor() { super([[1, 2]]) } } return new M().get(1)");
add("class M extends Map { constructor(it) { super(); for (const [k, v] of it) this.set(k, v) } } return new M([[1, 2]]).size");
add("class M extends Map { get size() { return 99 } } return new M([[1, 2]]).size");
add("class M extends Map { static get [Symbol.species]() { return Map } } return M[Symbol.species] === Map");
add("class M extends Map {} return (() => { try { return M() } catch (e) { return e.name + ': ' + e.message } })()");
add("class M extends Map {} return (() => { try { return Map.prototype.get.call(new Set, 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class M extends Map {} return (() => { try { return Map.prototype.get.call(Object.create(new M), 1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class M extends Map {} const m = new M([[1, 2], [3, 4]]); return [[...m], [...m.keys()], [...m.values()], [...m.entries()]]");
add("class M extends Map {} const m = new M([[1, 2]]); return Object.getOwnPropertyNames(m)");
add("class M extends Map {} return [M.groupBy ? M.groupBy([1, 2], x => x % 2) instanceof Map : 'none', M.groupBy ? M.groupBy([1], x => x).constructor === Map : 'none']");
add("class M extends Map {} return Object.getPrototypeOf(M) === Map && M.prototype.constructor === M");
add("class M extends Map {} const m = new M; m.constructor = Object; return m instanceof Map");
add("class S2 extends Set {} const s = new S2([1, 2, 2]); return [s.size, s.has(1), s instanceof S2, s.add(5) === s]");
add("class S2 extends Set { add(v) { return super.add(v * 2) } } return [...new S2([1, 2])]");
add("class S2 extends Set { add(v) { return super.add(v * 2) } } const s = new S2(); s.add(1); return [...s]");
add("class S2 extends Set {} const s = new S2([1, 2]); return [s.union ? s.union(new Set([3])) instanceof S2 : 'none', s.union ? s.union(new Set([3])).constructor === Set : 'none']");
add("class S2 extends Set {} const s = new S2([1, 2]); return [s.intersection ? s.intersection(new Set([2])) instanceof Set : 'none']");
add("class S2 extends Set { static get [Symbol.species]() { return Array } } const s = new S2([1]); return s.union ? s.union(new Set([2])) instanceof S2 : 'none'");
add("class S2 extends Set {} return (() => { try { return S2() } catch (e) { return e.name + ': ' + e.message } })()");
add("class S2 extends Set { constructor() { super(); this.tag = 1 } } return new S2().tag");
add("class S2 extends Set { has() { return 'overridden' } } return [new S2([1]).has(1), Set.prototype.has.call(new S2([1]), 1)]");
add("class W extends WeakMap { set(k, v) { return super.set(k, v) } } const k = {}; return new W([[k, 1]]).get(k)");
add("class W extends WeakMap {} return (() => { try { return new W([[1, 2]]) } catch (e) { return e.name + ': ' + e.message } })()");
add("class W extends WeakRef {} const t = {}; return new W(t).deref() === t");
add("class W extends WeakRef {} return (() => { try { return new W(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class F extends FinalizationRegistry {} return new F(() => {}) instanceof F");
// RegExp
add("class R extends RegExp {} const r = new R('a', 'g'); return [r.source, r.flags, r instanceof R, r instanceof RegExp, r.test('a'), r.lastIndex]");
add("class R extends RegExp {} const r = new R('a'); return [String(r), Object.prototype.toString.call(r), r.constructor === R]");
add("class R extends RegExp {} return 'abab'.replace(new R('a', 'g'), 'x')");
add("class R extends RegExp {} return 'abab'.split(new R('b'))");
add("class R extends RegExp {} return 'abab'.match(new R('b', 'g'))");
add("class R extends RegExp {} return [...'abab'.matchAll(new R('b', 'g'))].length");
add("class R extends RegExp {} return 'abab'.search(new R('b'))");
add("class R extends RegExp { exec(s) { return null } } return [new R('a').test('a'), 'a'.replace(new R('a'), 'x'), 'a'.match(new R('a'))]");
add("class R extends RegExp { exec(s) { globalThis.c = (globalThis.c || 0) + 1; return super.exec(s) } } 'aaa'.replace(new R('a', 'g'), 'b'); return globalThis.c");
add("class R extends RegExp { exec(s) { globalThis.c = (globalThis.c || 0) + 1; return super.exec(s) } } new R('a').test('a'); return globalThis.c");
add("class R extends RegExp { exec(s) { globalThis.c = (globalThis.c || 0) + 1; return super.exec(s) } } 'a-b'.split(new R('-')); return globalThis.c");
add("class R extends RegExp { exec(s) { globalThis.c = (globalThis.c || 0) + 1; return super.exec(s) } } 'a'.match(new R('a', 'g')); return globalThis.c");
add("class R extends RegExp { exec(s) { return { index: 0, 0: 'zz', length: 1 } } } return 'abc'.replace(new R('a'), '[$&]')");
add("class R extends RegExp { exec(s) { return 1 } } return (() => { try { return new R('a').test('a') } catch (e) { return e.name + ': ' + e.message } })()");
add("class R extends RegExp { exec(s) { return undefined } } return (() => { try { return new R('a').test('a') } catch (e) { return e.name + ': ' + e.message } })()");
add("class R extends RegExp { static get [Symbol.species]() { return RegExp } } return 'a-b'.split(new R('-'))");
add("class R extends RegExp { constructor(p, f) { super(p, f); globalThis.k = (globalThis.k || 0) + 1 } } 'a-b'.split(new R('-')); return globalThis.k");
add("class R extends RegExp { constructor(p, f) { super(p, f); globalThis.k = (globalThis.k || []).concat(f) } } 'a-b'.split(new R('-')); return globalThis.k");
add("class R extends RegExp { constructor(p, f) { super(p, f); globalThis.k = (globalThis.k || []).concat(String(f)) } } [...'a-b'.matchAll(new R('-', 'g'))]; return globalThis.k");
add("class R extends RegExp {} const r = new R('a', 'g'); const c = new RegExp(r); return [c.flags, c instanceof R, c.constructor === RegExp]");
add("class R extends RegExp {} const r = new R('a', 'g'); return [RegExp(r) === r, new RegExp(r) === r]");
add("class R extends RegExp {} const r = new R('a', 'g'); return [R(r) === r]");
add("class R extends RegExp {} const r = new R('a', 'g'); const c = new R(r); return [c === r, c.flags]");
add("class R extends RegExp {} return RegExp.prototype[Symbol.matchAll].call(new R('a', 'g'), 'aa').next().value[0]");
add("class R extends RegExp {} const r = new R('a'); r.constructor = undefined; return [...'aa'.matchAll(new RegExp('a', 'g'))].length");
add("class R extends RegExp { get flags() { return 'g' } } return 'aaa'.replace(new R('a'), 'b')");
add("class R extends RegExp { get global() { return true } } return 'aaa'.replace(new R('a'), 'b')");
add("class R extends RegExp { get flags() { return 'x' } } return String(new R('a'))");
add("class R extends RegExp { get source() { return 'zz' } } return String(new R('a'))");
add("class R extends RegExp { [Symbol.replace](s, r) { return 'custom' } } return 'abc'.replace(new R('a'), 'x')");
add("class R extends RegExp { [Symbol.split](s) { return ['custom'] } } return 'abc'.split(new R('a'))");
add("class R extends RegExp { [Symbol.match](s) { return 'm' } } return 'abc'.match(new R('a'))");
add("class R extends RegExp { [Symbol.search](s) { return 7 } } return 'abc'.search(new R('a'))");
add("class R extends RegExp { [Symbol.matchAll](s) { return 'ma' } } return 'abc'.matchAll(new R('a', 'g'))");
add("class R extends RegExp { get [Symbol.match]() { return false } } return ['/a/'.startsWith(new R('a'))].length");
add("class R extends RegExp { get [Symbol.match]() { return false } } return '/a/'.startsWith(new R('/a/'))");
add("class R extends RegExp {} return (() => { try { return '/a/'.startsWith(new R('a')) } catch (e) { return e.name + ': ' + e.message } })()");
add("class R extends RegExp {} const r = new R('a', 'dgimsuy'); return [r.flags, r.hasIndices, r.global, r.sticky, r.unicode, r.dotAll, r.multiline, r.ignoreCase]");
add("class R extends RegExp {} const r = new R('(?<n>a)'); return r.exec('a').groups.n");
add("class R extends RegExp {} return (() => { try { return new R('(') } catch (e) { return e.name } })()");
add("class R extends RegExp {} return (() => { try { return new R('a', 'z') } catch (e) { return e.name + ': ' + e.message } })()");
add("class R extends RegExp {} return RegExp.prototype.toString.call(new R('a', 'g'))");
add("class R extends RegExp {} return Object.getOwnPropertyNames(new R('a')).sort()");
add("class R extends RegExp {} R.prototype.lastIndex = 5; return new R('a').lastIndex");
add("class R extends RegExp {} const r = new R('a', 'y'); r.lastIndex = 1; return [r.test('ba'), r.lastIndex]");
// Outros built-ins
add("class B extends Object {} return Object.getPrototypeOf(new B) === B.prototype");
add("class B extends Object { constructor() { super(); return Object.create(null) } } return Object.getPrototypeOf(new B)");
add("class D extends Date { constructor() { super(0) } get y() { return this.getUTCFullYear() } } return new D().y");
add("class D extends Date {} const d = new D(NaN); return [String(d), d.getTime(), JSON.stringify(d)]");
add("class D extends Date { [Symbol.toPrimitive](h) { return h } } return `${new D}` + (new D + '')");
add("class D extends Date {} return Date.prototype.getTime.call(new D(5)) + Date.UTC(1970, 0)");
add("class D extends Date {} const d = new D(5); return [+d, d.valueOf(), d.toJSON ? typeof d.toJSON() : 0]");
add("class N extends Number { constructor() { super(5) } } return [new N() + 1, new N().valueOf(), typeof new N()]");
add("class N extends Number {} return (() => { try { return N.prototype.valueOf.call(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class S3 extends String { get length() { return 99 } } return new S3('a').length");
add("class S3 extends String { constructor() { super('abc') } } return [new S3().length, new S3().toUpperCase(), new S3().at(-1)]");
add("class S3 extends String {} return [new S3('ab').padStart(4), new S3('ab') == 'ab', new S3('ab') === 'ab']");
add("class S3 extends String {} return (() => { try { return S3('a') } catch (e) { return e.name + ': ' + e.message } })()");
add("class S3 extends String {} return Object.getOwnPropertyNames(new S3('ab')).sort()");
add("class B3 extends Boolean {} return [new B3(false) ? 1 : 2, !!new B3(false), new B3(false).valueOf()]");
add("class A2 extends ArrayBuffer {} const a = new A2(4); return [a.byteLength, a.slice(0, 2) instanceof A2, a.slice(0, 2).byteLength]");
add("class A2 extends ArrayBuffer { static get [Symbol.species]() { return ArrayBuffer } } return new A2(4).slice(0, 2) instanceof A2");
add("class A2 extends ArrayBuffer { static get [Symbol.species]() { return function () { return {} } } } return (() => { try { return new A2(4).slice(0, 2) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A2 extends ArrayBuffer { static get [Symbol.species]() { return function (n) { return new ArrayBuffer(1) } } } return (() => { try { return new A2(4).slice(0, 2) } catch (e) { return e.name + ': ' + e.message } })()");
add("class A2 extends ArrayBuffer { static get [Symbol.species]() { return function (n) { return new ArrayBuffer(n) } } } return new A2(4).slice(0, 2).byteLength");
add("class T extends Uint8Array {} const t = new T([1, 2, 3]); return [t.filter(x => x > 1) instanceof T, t.map(x => x) instanceof T, t.slice(1) instanceof T, t.subarray(1) instanceof T, t.toSorted() instanceof T]");
add("class T extends Uint8Array { static get [Symbol.species]() { return Uint16Array } } const t = new T([1, 2]); return [t.map(x => x) instanceof Uint16Array, t.map(x => x) instanceof T, t.subarray(0) instanceof Uint16Array]");
add("class T extends Uint8Array {} return [T.from([1, 2]) instanceof T, T.of(1) instanceof T, T.BYTES_PER_ELEMENT, T.name]");
add("class T extends Float64Array {} return [new T(2).byteLength, new T([1.5]).at(0)]");
add("class T extends BigInt64Array {} return [new T(2).byteLength, new T([1n]).at(0)]");
add("class T extends Uint8Array { constructor() { super(2); this.tag = 1 } } const t = new T; return (() => { try { return t.map(x => x).tag } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Uint8Array { constructor(...a) { super(...a); this.tag = 1 } } const t = new T(2); return t.map(x => x).tag");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = Array; return (() => { try { return t.map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = { [Symbol.species]: function () { return new Uint8Array(1) } }; return (() => { try { return t.map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = { [Symbol.species]: function () { return new Uint8Array(5) } }; return t.map(x => x).length");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = { [Symbol.species]: function () { return new BigInt64Array(2) } }; return (() => { try { return t.map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = { [Symbol.species]: null }; return t.map(x => x) instanceof Uint8Array");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = undefined; return t.map(x => x).constructor === Uint8Array");
add("class T extends Uint8Array {} const t = new T(2); t.constructor = 1; return (() => { try { return t.map(x => x) } catch (e) { return e.name + ': ' + e.message } })()");
add("class G extends (function* () {}).constructor {} return (() => { try { const g = new G('yield 1'); return [typeof g, [...g()]] } catch (e) { return e.name + ': ' + e.message } })()");
add("class AF extends (async function () {}).constructor {} return (() => { try { const f = new AF('return 1'); return [typeof f, f() instanceof Promise] } catch (e) { return e.name + ': ' + e.message } })()");
add("class F2 extends Function {} const f = new F2('a', 'b', 'return a + b'); return [f(1, 2), f.length, f.name, f.toString().replace(/\\s+/g, ' ')]");
add("class F2 extends Function { constructor() { super('return this.v'); } } const f = new F2().bind({ v: 3 }); return f()");
add("class F2 extends Function { constructor() { super('return 1'); return Object.setPrototypeOf(() => 2, F2.prototype) } m() { return 'm' } } const f = new F2; return [f(), f.m(), f instanceof F2]");
add("class F2 extends Function { constructor() { super(); return this.bind(this) } } return typeof new F2");
add("class F2 extends Function {} return typeof F2()");
add("class F2 extends Function {} return F2('return 3')()");
add("class F2 extends Function {} return new F2('return new.target')() === undefined");
add("class F2 extends Function {} return Object.getPrototypeOf(new F2) === F2.prototype");
add("class F2 extends Function {} return Object.getPrototypeOf(F2('x')) === Function.prototype");
add("class S4 extends Symbol {} return Object.getPrototypeOf(S4) === Symbol");
add("class S4 extends Symbol { constructor() { return Object(Symbol('s')) } } return (() => { try { return typeof new S4 } catch (e) { return e.name + ': ' + e.message } })()");
add("class I extends Intl.NumberFormat {} const n = new I('en'); return [n.format(1234.5), n instanceof I]");
add("class I extends Intl.DateTimeFormat {} return (() => { try { return new I('en', { timeZone: 'UTC' }).format(0) } catch (e) { return e.name + ': ' + e.message } })()");
add("class I extends Intl.Collator {} return [new I('en').compare('a', 'b')]");
add("class I extends Intl.PluralRules {} return new I('en').select(1)");
add("class I extends Intl.ListFormat {} return new I('en').format(['a', 'b'])");
add("class I extends Intl.Segmenter {} return [...new I('en').segment('ab')].length");
add("class I extends Intl.RelativeTimeFormat {} return new I('en').format(1, 'day')");
add("class I extends Intl.DisplayNames {} return new I('en', { type: 'region' }).of('BR')");
add("class I extends Intl.Locale {} return new I('pt-BR').language");
add("class I extends Iterator {} return (() => { try { return new I() instanceof Iterator } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return new Iterator() } catch (e) { return e.name + ': ' + e.message } })()");
add("class I extends Iterator { next() { return { done: true } } } return [new I() instanceof Iterator, [...new I()].length]");
add("class I extends Iterator { constructor() { super(); this.i = 0 } next() { return this.i < 3 ? { value: this.i++, done: false } : { done: true } } } return [...new I().map(x => x * 2)]");
add("class I extends Iterator { next() { return { done: true } } } return Object.getPrototypeOf(I.prototype) === Iterator.prototype");
add("class Q extends (class {}) {} return new Q instanceof Q");
add("const Mixin = B => class extends B { m() { return 'mixin' } }; class A {} class C extends Mixin(A) {} return [new C().m(), new C() instanceof A, C.name, Object.getPrototypeOf(C).name]");
add("const Mixin = B => class extends B { constructor(...a) { super(...a); this.mixed = true } }; class C extends Mixin(Array) {} const c = new C(3); return [c.length, c.mixed]");
add("const Mixin = B => class extends B {}; class C extends Mixin(Mixin(Map)) {} return [new C([[1, 2]]).get(1), C.name === '']");
add("class A {} class B extends A {} class C extends B {} return [Object.getPrototypeOf(C) === B, C.prototype instanceof A, A.isPrototypeOf(C)]");
add("function F() { this.f = 1 } class B extends F { constructor() { super(); this.b = 2 } } return Object.keys(new B)");
add("function F() { this.nt = new.target } class B extends F {} return new B().nt === B");
add("function F() { return { custom: 1 } } class B extends F {} return new B().custom");
add("function F() { return 5 } class B extends F {} return new B() instanceof B");
add("function F() {} F.prototype.m = function () { return 'fm' }; class B extends F { m() { return super.m() + '!' } } return new B().m()");
add("function F() {} F.s = function () { return 'fs' }; class B extends F { static s() { return super.s() + '!' } } return B.s()");
add("const o = { m() { return 'o' } }; class B extends Object { m() { return 'b' } } return B.prototype.m.call(o)");
add("function F() {} class B extends F {} return Reflect.construct(F, [], B) instanceof B");
add("function F() {} class B extends F {} return F.call(new B) === undefined");
add("function F() { if (!new.target) throw 1 } class B extends F {} return (() => { try { return new B instanceof F } catch (e) { return e } })()");
add("const A = new Proxy(class {}, {}); class B extends A {} return new B instanceof B");
add("const A = new Proxy(function () {}, { get(t, k) { return t[k] } }); class B extends A {} return new B instanceof B");
add("const A = new Proxy(function () {}, { construct(t, a, nt) { return Reflect.construct(t, a, nt) } }); class B extends A { constructor() { super() } } return Object.getPrototypeOf(new B) === B.prototype");
add("const A = new Proxy(function () {}, { construct(t, a, nt) { return { nt: nt.name } } }); class B extends A { constructor() { super() } } return new B().nt");
add("const A = new Proxy(function () {}, { construct(t, a, nt) { return 1 } }); class B extends A { constructor() { super() } } return (() => { try { return new B } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { constructor() { return new Proxy(this, { get(t, k) { return k === 'x' ? 'px' : t[k] } }) } } class B extends A { y = 1 } const b = new B; return [b.x, b.y]");
add("class A { constructor() { return new Proxy({}, { defineProperty(t, k, d) { globalThis.keys = (globalThis.keys || []).concat(String(k)); return Reflect.defineProperty(t, k, d) } }) } } class B extends A { f = 1; g = 2 } new B; return globalThis.keys");
add("class A { constructor() { return new Proxy({}, { set(t, k, v) { globalThis.sets = (globalThis.sets || []).concat(String(k)); return true } }) } } class B extends A { f = 1 } new B; return globalThis.sets");
add("class A { constructor() { return new Proxy({}, { defineProperty() { return false } }) } } class B extends A { f = 1 } return (() => { try { new B; return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
// Symbol.species em geral
add("class A { static get [Symbol.species]() { return this } m() { return new this.constructor[Symbol.species]() } } class B extends A {} return [new B().m() instanceof B]");
add("class A { static get [Symbol.species]() { return A } m() { return new this.constructor[Symbol.species]() } } class B extends A {} return [new B().m() instanceof B, new B().m() instanceof A]");
add("class B extends Array {} const b = new B(); b.push(1, 2, 3); return [b.length, b.slice(0, 2).length, b.slice(0, 2) instanceof B, b.map(String) instanceof B]");
add("class B extends Array {} const b = new B(); b.push(1, 2, 3); Object.defineProperty(B, Symbol.species, { value: Object }); return Object.prototype.toString.call(b.map(String))");
add("class B extends Array {} const b = new B(); b.push(1, 2, 3); Object.defineProperty(B, Symbol.species, { value: function (n) { return { length: n, tag: 1 } } }); return b.slice(1).tag");
add("class B extends Array {} const b = new B(); b.push(1, 2, 3); Object.defineProperty(B, Symbol.species, { value: function (n) { return Object.freeze([]) } }); return (() => { try { return b.slice(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends Array {} const b = new B(); b.push(1, 2, 3); Object.defineProperty(B, Symbol.species, { value: function (n) { return new Proxy([], { defineProperty() { return false } }) } }); return (() => { try { return b.slice(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("const calls = []; class B extends Array {} Object.defineProperty(B, Symbol.species, { get() { calls.push('s'); return Array } }); const b = new B(); b.push(1); b.map(x => x); b.filter(x => x); b.slice(); b.splice(0, 0); b.concat(); b.flat(); b.flatMap(x => x); return calls.length");
add("const calls = []; class B extends Array {} Object.defineProperty(B, Symbol.species, { get() { calls.push('s'); return Array } }); const b = new B(); b.push(1); b.toSorted(); b.toReversed(); b.toSpliced(); b.with(0, 1); b.at(0); b.indexOf(1); b.includes(1); return calls.length");
add("const calls = []; class B extends Array {} Object.defineProperty(B, Symbol.species, { get() { calls.push('s'); return Array } }); B.from([1]); B.of(1); new B(1); return calls.length");
add("const log = []; const a = [1, 2]; Object.defineProperty(a, 'constructor', { get() { log.push('c'); return undefined } }); a.map(x => x); a.slice(); a.splice(0, 0); return log.length");
add("const log = []; const a = [1, 2]; Object.defineProperty(a, 'constructor', { get() { log.push('c'); return undefined } }); a.forEach(x => x); a.some(x => x); a.reduce((p, c) => p + c); return log.length");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return { length: 0 } } }; return Object.prototype.toString.call(a.map(x => x))");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.map(x => x + 1)");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.slice(0, 1)");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.filter(x => x > 1)");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.splice(0, 1)");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.concat([3])");
add("const a = [1, [2]]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.flat()");
add("const a = [1, 2]; a.constructor = { [Symbol.species]: function (n) { return new Array(7).fill(0) } }; return a.flatMap(x => [x])");
add("class P extends Promise {} const calls = []; Object.defineProperty(P, Symbol.species, { get() { calls.push('s'); return Promise } }); const p = new P(r => r()); p.then(); p.catch(() => {}); p.finally(() => {}); return calls.length");
add("class M extends Map {} const calls = []; Object.defineProperty(M, Symbol.species, { get() { calls.push('s'); return Map } }); const m = new M; m.get(1); m.set(1, 1); [...m]; return calls.length");
add("class R extends RegExp {} const calls = []; Object.defineProperty(R, Symbol.species, { get() { calls.push('s'); return RegExp } }); 'a-b'.split(new R('-')); [...'a'.matchAll(new R('a', 'g'))]; return calls.length");
add("class R extends RegExp {} const calls = []; Object.defineProperty(R, Symbol.species, { get() { calls.push('s'); return RegExp } }); 'a-b'.replace(new R('-', 'g'), ''); 'a'.match(new R('a')); 'a'.search(new R('a')); return calls.length");
add("class R extends RegExp {} const r = new R('a', 'g'); r.constructor = { [Symbol.species]: function (p, f) { return new RegExp(p, f + 'i') } }; return [...'aA'.matchAll(r)].length");
add("class R extends RegExp {} const r = new R('-'); r.constructor = { [Symbol.species]: function (p, f) { globalThis.fl = f; return new RegExp(p, f) } }; 'a-b'.split(r); return globalThis.fl");
add("class R extends RegExp {} const r = new R('-', 'u'); r.constructor = { [Symbol.species]: function (p, f) { globalThis.fl = f; return new RegExp(p, f) } }; 'a-b'.split(r); return globalThis.fl");
add("const r = /-/; r.constructor = undefined; return 'a-b'.split(r)");
add("const r = /-/; r.constructor = null; return (() => { try { return 'a-b'.split(r) } catch (e) { return e.name + ': ' + e.message } })()");
add("const r = /-/; r.constructor = { [Symbol.species]: undefined }; return 'a-b'.split(r)");
add("const r = /-/; r.constructor = { [Symbol.species]: function (p, f) { return { exec() { return null }, lastIndex: 0, flags: '' } } }; return 'a-b'.split(r)");
add("const buf = new ArrayBuffer(8); buf.constructor = { [Symbol.species]: function (n) { return new ArrayBuffer(n) } }; return buf.slice(2).byteLength");
add("const buf = new ArrayBuffer(8); buf.constructor = undefined; return buf.slice(2).byteLength");
add("const buf = new ArrayBuffer(8); buf.constructor = { [Symbol.species]: function (n) { return buf } }; return (() => { try { return buf.slice(2) } catch (e) { return e.name + ': ' + e.message } })()");
add("const buf = new ArrayBuffer(8); buf.constructor = { [Symbol.species]: function (n) { return new ArrayBuffer(n - 1) } }; return (() => { try { return buf.slice(2) } catch (e) { return e.name + ': ' + e.message } })()");
add("const buf = new ArrayBuffer(8); buf.constructor = { [Symbol.species]: function (n) { return new ArrayBuffer(n + 1) } }; return buf.slice(2).byteLength");
add("const buf = new ArrayBuffer(8); buf.constructor = { [Symbol.species]: function (n) { return {} } }; return (() => { try { return buf.slice(2) } catch (e) { return e.name + ': ' + e.message } })()");
add("const buf = new ArrayBuffer(8); buf.constructor = 1; return (() => { try { return buf.slice(2) } catch (e) { return e.name + ': ' + e.message } })()");
add("class B extends ArrayBuffer { constructor(n) { super(n); this.t = 1 } } const b = new B(8); return b.slice(2).t");
add("class SAB extends SharedArrayBuffer {} const b = new SAB(8); return [b.byteLength, b.slice(2) instanceof SAB, Object.prototype.toString.call(b)]");
add("const r = Reflect.construct(Uint8Array, [2], Array); return [Object.getPrototypeOf(r) === Array.prototype, Object.prototype.toString.call(r), r.length]");
add("return Reflect.construct(Uint8Array, [2], function () {}).length");
add("const p = Object.getPrototypeOf(Uint8Array); return (() => { try { return new p() } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Object.getPrototypeOf(Uint8Array) {} return (() => { try { return new T(1) } catch (e) { return e.name + ': ' + e.message } })()");
add("class T extends Uint8Array { constructor() { super(2) } } return Reflect.construct(T, [], Uint16Array) instanceof Uint16Array");
add("class T extends Uint8Array {} const t = Reflect.construct(T, [3], Array); return [Object.getPrototypeOf(t) === Array.prototype, t.length]");
// Function.prototype extras: toString, length/name de classes e métodos
add("class A { constructor(a, b) {} } return [A.length, A.name, typeof A, A.prototype.constructor === A]");
add("class A { constructor(a, b = 1, c) {} } return A.length");
add("class A { constructor(...r) {} } return A.length");
add("class A {} class B extends A {} return [B.length, A.length]");
add("class A { constructor(a) {} } class B extends A {} return B.length");
add("class A { static length = 3 } return A.length");
add("class A { static length() {} } return typeof A.length");
add("class A { static name() {} } return typeof A.name");
add("class A { static get name() { return 'gn' } } return A.name");
add("class A { static ['na' + 'me'] = 'cn' } return A.name");
add("const A = class { static x = this.name }; return A.x");
add("const A = class B { static x = this.name }; return A.x");
add("const o = { A: class { static x = this.name } }; return o.A.x");
add("const o = { A: class { static name = 'z' } }; return o.A.name");
add("let A = class {}; return [A.name, Object.getOwnPropertyNames(A).sort()]");
add("class A { static x() {} } return Object.getOwnPropertyNames(A).sort()");
add("class A { x() {} } return [Object.getOwnPropertyNames(A.prototype), Object.getOwnPropertyDescriptor(A.prototype, 'x').enumerable]");
add("class A { get x() { return 1 } set x(v) {} } const d = Object.getOwnPropertyDescriptor(A.prototype, 'x'); return [d.get.name, d.set.name, d.enumerable, d.configurable]");
add("class A { static get x() { return 1 } } return Object.getOwnPropertyDescriptor(A, 'x').get.name");
add("class A { [Symbol.iterator]() {} } return A.prototype[Symbol.iterator].name");
add("class A { ['a' + 'b']() {} } return A.prototype.ab.name");
add("class A { #p() {} t() { return this.#p.name } } return new A().t()");
add("class A { get #p() { return 1 } t() { return Object.getOwnPropertyDescriptor ? 1 : 0 } } return new A().t()");
add("class A { x = function () {} } return new A().x.name");
add("class A { x = () => {} } return new A().x.name");
add("class A { x = class {} } return new A().x.name");
add("class A { static x = function () {} } return A.x.name");
add("class A { #x = function () {}; t() { return this.#x.name } } return new A().t()");
add("class A { #x = () => {}; t() { return this.#x.name } } return new A().t()");
add("class A { ['k' + 1] = function () {} } return new A().k1.name");
add("class A { [Symbol('d')] = function () {} } return new A()[Object.getOwnPropertySymbols(new A)[0]].name");
add("class A { 1 = function () {} } return new A()[1].name");
add("class A { x = function f() {} } return new A().x.name");
add("class A { x = (0, function () {}) } return JSON.stringify(new A().x.name)");
add("class A { x = (function () {}) } return new A().x.name");
add("class A { x; static y; } return [Object.hasOwn(new A, 'x'), Object.hasOwn(A, 'y')]");
add("class A { static x = 1; static x = 2 } return A.x");
add("class A { x = 1; ['x'] = 2 } return new A().x");
add("class A { get x() { return 1 } x = 2 } return new A().x");
add("class A { set x(v) { globalThis.sx = v } x = 2 } return [new A().x, globalThis.sx]");
add("class A { static set x(v) { globalThis.sx = v } static x = 2 } return [A.x, globalThis.sx]");
add("class A { x() { return 1 } x = 2 } return typeof new A().x");
add("class A { toString() { return 'A!' } } return `${new A}` + new A");
add("class A { valueOf() { return 7 } } return new A + 1");
add("class A { [Symbol.toPrimitive](h) { return h } } return [`${new A}`, new A + '', +new A]");
add("class A { static [Symbol.toPrimitive](h) { return h } } return `${A}`");
add("class A { get [Symbol.toStringTag]() { return 'TT' } } return String(new A)");
add("class A { static get [Symbol.toStringTag]() { return 'TS' } } return String(A)");
add("class A {} return [String(A).replace(/\\s+/g, ' '), A.toString === Function.prototype.toString]");
add("class A { m() {} } return A.prototype.m.toString()");
add("class A { static m() { return 1 } } return A.m.toString()");
add("class A { get g() { return 1 } } return Object.getOwnPropertyDescriptor(A.prototype, 'g').get.toString()");
add("class A { #p() {} t() { return this.#p.toString() } } return new A().t()");
add("class A { ['c' + 1]() {} } return A.prototype.c1.toString()");
add("class A { async *m() {} } return A.prototype.m.toString()");
add("class A { constructor() {} } return A.toString()");
add("class A extends Object { constructor() { super() } } return A.toString()");
add("const A = class  { }; return A.toString()");
add("return (function  f ( a ,b ) { }).toString()");
add("return (async  function  f ( a ) { }).toString()");
add("return (( a ) =>  1).toString()");
add("return ({ m ( ) { } }).m.toString()");
add("return ({ get  g ( ) { return 1 } }).__lookupGetter__('g').toString()");
add("return Function.prototype.toString.call(Math.max)");
add("return Function.prototype.toString.call(class {}.constructor)");
add("return Function.prototype.toString.call(function () {}.bind())");
add("return Function.prototype.toString.call(new Proxy(function () {}, {}))");
add("return (() => { try { return Function.prototype.toString.call(new Proxy({}, {})) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return Function.prototype.toString.call({}) } catch (e) { return e.name + ': ' + e.message } })()");
add("return Function.prototype.toString.call(Symbol)");
add("return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype, 'size').get)");
add("return new Function('a', 'b', 'return a').toString()");
add("return new Function('a, b', 'return a').toString()");
add("return new Function().toString()");
add("return Function('/* c */ a', 'return a').toString()");
add("return (function* g() {}).toString()");
add("return Reflect.getPrototypeOf(function* () {}).constructor.name");
add("return Object.getPrototypeOf(async function () {}).constructor.name");
add("return Object.getPrototypeOf(async function* () {}).constructor.name");
add("return [Function.name, Function.length, Function.prototype.constructor === Function]");
add("return [typeof Function.prototype.caller, typeof Function.prototype.arguments]");
add("return (() => { try { return Function.prototype.caller } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (function () { 'use strict' }).caller } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (function () { 'use strict'; return arguments.callee })() } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (() => {}).caller } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (class {}).caller } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (function () {}).arguments } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return (function () { return arguments.callee === undefined })() } catch (e) { return e.name + ': ' + e.message } })()");
add("return Object.getOwnPropertyNames(function () {}).sort()");
add("return Object.getOwnPropertyNames(function () { 'use strict' }).sort()");
add("return Object.getOwnPropertyNames(() => {}).sort()");
add("return Object.getOwnPropertyNames(class {}).sort()");
add("return Object.getOwnPropertyNames(async function () {}).sort()");
add("return Object.getOwnPropertyNames(function* () {}).sort()");
add("return Object.getOwnPropertyNames({ m() {} }.m).sort()");
add("return Object.getOwnPropertyNames(function () {}.prototype).sort()");
add("return Object.getOwnPropertyNames((function* () {}).prototype).sort()");
add("return Object.getOwnPropertyNames(class { static x = 1; static m() {} }).sort()");
add("function f(a, b) {} return Object.getOwnPropertyDescriptor(f, 'length')");
add("function f(a, b) {} return Object.getOwnPropertyDescriptor(f, 'name')");
add("function f(a, b) {} return Object.getOwnPropertyDescriptor(f, 'prototype')");
add("class A {} return Object.getOwnPropertyDescriptor(A, 'prototype')");
add("function f() {} f.length = 5; f.name = 'x'; return [f.length, f.name]");
add("'use strict'; function f() {} return (() => { try { f.length = 5 } catch (e) { return e.name + ': ' + e.message } })()");
add("'use strict'; function f() {} return (() => { try { f.name = 5 } catch (e) { return e.name + ': ' + e.message } })()");
add("'use strict'; class A {} return (() => { try { A.prototype = 5 } catch (e) { return e.name + ': ' + e.message } })()");
add("function f() {} return delete f.length && delete f.name && Object.getOwnPropertyNames(f).sort()");
add("function f() {} delete f.name; return [f.name, Object.hasOwn(f, 'name'), f.hasOwnProperty('name')]");
add("function f() {} delete f.name; f.toString(); return Object.getOwnPropertyNames(f).sort()");
add("function f() {} Object.defineProperty(f, 'name', { value: 'n', writable: true }); f.name = 'm'; return f.name");
add("const f = function () {}; Object.defineProperty(f, 'name', { value: 'a' }); return [f.name, f.bind().name]");
add("const f = function () {}; Object.defineProperty(f, 'name', { get() { return 'g' } }); return f.bind().name");
add("const f = function () {}; Object.defineProperty(f, 'name', { value: undefined }); return f.bind().name");
add("const f = function () {}; Object.defineProperty(f, 'name', { value: null }); return f.bind().name");
add("const f = function () {}; Object.defineProperty(f, 'name', { value: {} }); return f.bind().name");
add("const f = function () {}; Object.defineProperty(f, 'name', { value: 'a' }); return f.bind().bind().bind().name");
add("const f = function () {}; return [f.bind().name, f.bind().bind().name]");
add("const o = { f() {} }; return [o.f.bind().name, o.f.name]");
add("const o = { get f() { return 1 } }; return Object.getOwnPropertyDescriptor(o, 'f').get.bind().name");
add("const o = { async *f() {} }; return o.f.bind().name");
add("const s = Symbol('d'); const o = { [s]() {} }; return [o[s].name, o[s].bind().name]");
add("const s = Symbol(); const o = { [s]() {} }; return JSON.stringify([o[s].name, o[s].bind().name])");
add("const o = { [Symbol.iterator]() {} }; return o[Symbol.iterator].name");
add("const o = { get [Symbol.toStringTag]() { return 'x' } }; return Object.getOwnPropertyDescriptor(o, Symbol.toStringTag).get.name");
add("class A { static #p() {} static t() { return A.#p.name } } return A.t()");
add("class A { static get #p() { return 1 } static t() { return 1 } } return A.t()");
// this em vários contextos
add("return (function () { return typeof this })()");
add("return (function () { 'use strict'; return typeof this })()");
add("return (() => typeof this)()");
add("return [typeof this, this === globalThis]");
add("return (function () { return this === globalThis })()");
add("return (function () { return (() => this === globalThis)() })()");
add("return (function () { 'use strict'; return (() => this)() })()");
add("return (function () { return this }).call(5) instanceof Number");
add("return (function () { return this }).call('s') instanceof String");
add("return (function () { return this }).call(true) instanceof Boolean");
add("return (function () { return this }).call(Symbol()) instanceof Symbol");
add("return (function () { return this }).call(1n) instanceof BigInt");
add("return typeof (function () { return this }).call(1n)");
add("return Object.getPrototypeOf((function () { return this }).call(1n)) === BigInt.prototype");
add("const o = { f() { return this }, g: () => this }; return [o.f() === o, o.g() === this]");
add("const o = { f() { return () => this } }; return o.f()() === o");
add("const o = { f() { return function () { return this } } }; return o.f()() === globalThis");
add("const o = { f() { 'use strict'; return function () { return this } } }; return o.f()()");
add("const o = { f() { return [1].map(function () { return this }, 'thisArg')[0] } }; return typeof o.f()");
add("const o = { f() { 'use strict'; return [1].map(function () { return this }, 'thisArg')[0] } }; return o.f()");
add("return [1].map(function () { 'use strict'; return this })[0]");
add("return [1].map(function () { return this === globalThis })[0]");
add("return [1].map(() => this === globalThis)[0]");
add("return [1].map(function () { return this }, null)[0] === globalThis");
add("return [1].map(function () { 'use strict'; return this }, null)[0]");
add("return [1].map(function () { 'use strict'; return typeof this }, 5)[0]");
add("return [1].map(function () { return typeof this }, 5)[0]");
add("return Array.from([1], function () { return typeof this }, 'x')[0]");
add("return Array.from([1], function () { 'use strict'; return this }, 'x')[0]");
add("return new Set([1]).forEach(function () { globalThis.th = typeof this }, 5) || globalThis.th");
add("return new Map([[1, 1]]).forEach(function () { globalThis.th = this }, 7) || typeof globalThis.th");
add("return [3, 1, 2].sort(function (a, b) { globalThis.th = this; return a - b }) && String(globalThis.th === globalThis)");
add("return 'a'.replace('a', function () { return typeof this })");
add("return 'a'.replace('a', function () { 'use strict'; return typeof this })");
add("return 'a'.replace(/a/, function () { 'use strict'; return String(this) })");
add("return JSON.parse('[1]', function (k, v) { return k === '' ? typeof this : v })");
add("return JSON.parse('[1]', function (k, v) { 'use strict'; return k === '0' ? Array.isArray(this) : v })");
add("return JSON.stringify({ a: 1 }, function (k, v) { return k === '' ? v : typeof this })");
add("return JSON.stringify({ a: 1 }, function (k, v) { 'use strict'; return k === 'a' ? Object.keys(this) : v })");
add("return JSON.stringify({ toJSON() { return typeof this } })");
add("return JSON.stringify({ a: { toJSON() { return this === undefined ? 'u' : 'o' } } })");
add("return typeof Reflect.apply(function () { return this }, 1, [])");
add("return Reflect.apply(function () { 'use strict'; return this }, 1, [])");
add("return Reflect.apply(function () { 'use strict'; return this }, undefined, [])");
add("return Reflect.get({ get x() { return this } }, 'x', 5)");
add("return Reflect.get({ get x() { 'use strict'; return this } }, 'x', 5)");
add("return typeof Reflect.get({ get x() { return this } }, 'x', 5)");
add("const o = { set x(v) { globalThis.th = this } }; Reflect.set(o, 'x', 1, 5); return typeof globalThis.th");
add("const o = { set x(v) { 'use strict'; globalThis.th = this } }; Reflect.set(o, 'x', 1, 5); return globalThis.th");
add("const o = { set x(v) { 'use strict'; globalThis.th = this } }; Reflect.set(o, 'x', 1, undefined); return globalThis.th");
add("'use strict'; return (() => { try { Reflect.set({}, 'x', 1, 5); return 'ok' } catch (e) { return e.name } })()");
add("return Reflect.set({}, 'x', 1, 5)");
add("return Reflect.set({}, 'x', 1, undefined)");
add("return (() => { try { return Reflect.set({}, 'x', 1, null) } catch (e) { return e.name + ': ' + e.message } })()");
add("'use strict'; return (() => { try { 'str'.x = 1; return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("'use strict'; String.prototype.me = function () { return this }; return typeof 'a'.me()");
add("String.prototype.me = function () { return this }; return typeof 'a'.me()");
add("String.prototype.me = function () { 'use strict'; return this }; return typeof 'a'.me()");
add("Number.prototype.me = function () { return typeof this }; return (5).me()");
add("Number.prototype.me = function () { 'use strict'; return typeof this }; return (5).me()");
add("Boolean.prototype.me = function () { 'use strict'; return typeof this }; return true.me()");
add("Symbol.prototype.me = function () { 'use strict'; return typeof this }; return Symbol().me()");
add("BigInt.prototype.me = function () { 'use strict'; return typeof this }; return (1n).me()");
add("BigInt.prototype.me = function () { return typeof this }; return (1n).me()");
add("Object.defineProperty(Number.prototype, 'g', { get() { return typeof this } }); return (5).g");
add("Object.defineProperty(Number.prototype, 'g', { get() { 'use strict'; return typeof this } }); return (5).g");
add("Object.defineProperty(Number.prototype, 's', { set(v) { globalThis.th = typeof this } }); (5).s = 1; return globalThis.th");
add("Object.defineProperty(Number.prototype, 's', { set(v) { 'use strict'; globalThis.th = typeof this } }); (5).s = 1; return globalThis.th");
add("Object.defineProperty(Number.prototype, 's', { set(v) { 'use strict'; globalThis.th = typeof this } }); 'use strict'; (5).s = 1; return globalThis.th");
add("Object.defineProperty(String.prototype, 's', { set(v) { 'use strict'; globalThis.th = this } }); 'a'.s = 1; return globalThis.th");
add("Object.defineProperty(Object.prototype, 'tt', { get() { return this === globalThis } }); return tt");
add("Object.defineProperty(Object.prototype, 'tt', { get() { 'use strict'; return this === globalThis } }); return tt");
add("Object.defineProperty(Object.prototype, 'tt', { get() { 'use strict'; return typeof this } }); return [(1).tt, 'a'.tt, true.tt]");
add("with ({ f() { return this } }) { return typeof f().f }");
add("const o = { f() { return this } }; with (o) { return f() === o }");
add("const o = { f() { 'use strict'; return this } }; with (o) { return f() === o }");
add("const o = { f() { return this } }; with (o) { return (() => f())() === o }");
add("const o = { f() { return this } }; with (o) { return (0, f)() === o }");
add("const o = { f() { return this } }; with (o) { return eval('f()') === o }");
add("const o = { f() { return this } }; with (o) { return typeof (function () { return f() })() }");
add("const o = { [Symbol.unscopables]: { f: true }, f() { return this } }; var f = function () { return typeof this }; with (o) { return f() }");
add("var f = function () { return this }; const o = { f }; with (o) { return f() === o }");
add("const o = { f() { return this } }; return [o.f(), (o.f)(), (o.f = o.f)(), (0, o.f)(), (o?.f)(), o?.f(), o.f?.()].map(x => x === o)");
add("const o = { f() { return this } }; return [o['f'](), o[`f`]()].map(x => x === o)");
add("const o = { f() { return this } }; return `${(o.f)``}` === '[object Object]'");
add("const o = { f() { return this } }; return o.f`` === o");
add("const o = { f() { return this } }; return new (o.f)() === o");
add("const o = { f() { return this } }; return (true ? o.f : 0)() === o");
add("const o = { f() { return this } }; return (o.f || 0)() === o");
add("const o = { f() { return this } }; return (o.f, 0) || (0, o.f)() === o");
add("const o = { f() { return this } }; const g = o.f; return g() === o");
add("const o = { f() { return this } }; const { f } = o; return f() === o");
add("const o = { f() { return this } }; const [g] = [o.f]; return g() === o");
add("const o = { f() { return this } }; return [o.f].map(g => g())[0] === o");
add("const o = { f() { return this } }; return [o.f][0]() === o");
add("const o = { f() { return this } }; const a = [o.f]; return a[0]() === a");
add("const a = [function () { return this }]; return a[0]() === a");
add("const a = [() => this]; return a[0]() === this");
add("function F() { this.f = function () { return this } } const o = new F; return o.f() === o");
add("function F() { this.f = () => this } const o = new F; const f = o.f; return f() === o");
add("function F() { const self = this; this.f = function () { return self } } const o = new F; const f = o.f; return f() === o");
add("function F() { return this } return new F() instanceof F");
add("function F() { 'use strict'; return this } return new F() instanceof F");
add("function F() { this.a = 1; return this } return new F().a");
add("function F() { this.a = 1; return 1 } return new F().a");
add("function F() { this.a = 1; return null } return new F().a");
add("function F() { this.a = 1; return {} } return new F().a");
add("function F() { this.a = 1; return [] } return Array.isArray(new F)");
add("function F() { this.a = 1; return function () {} } return typeof new F");
add("function F() { this.a = 1; return Symbol() } return new F().a");
add("function F() { this.a = 1; return new Number(3) } return new F() instanceof Number");
add("function F() { this.a = 1; return undefined } return new F().a");
add("function F() { return new.target ? this : undefined } return [typeof F(), typeof new F]");
add("function F() { F.inst = this } new F; return F.inst instanceof F");
add("function F() { F.inst = this } F(); return F.inst === globalThis");
add("function F() { 'use strict'; F.inst = this } F(); return F.inst");
add("function F() { this.x = 1 } F.call(F); return F.x");
add("function F() { this.x = 1 } const o = {}; F.call(o); return o.x");
add("function F() { this.x = 1 } F.apply(null); const v = globalThis.x; delete globalThis.x; return v");
add("function F() { this.x = 1 } F(); const v = globalThis.x; delete globalThis.x; return v");
add("function F() { this.x = 1 } F.bind(undefined)(); const v = globalThis.x; delete globalThis.x; return v");
add("function F() { 'use strict'; this.x = 1 } return (() => { try { F() } catch (e) { return e.name + ': ' + e.message } })()");
add("function F() { 'use strict'; this.x = 1 } return (() => { try { F.call(null) } catch (e) { return e.name + ': ' + e.message } })()");
add("function F() { 'use strict'; this.x = 1 } return (() => { try { F.call(5); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("function F() { 'use strict'; this.x = 1 } return (() => { try { F.call('s'); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()");
add("class A { m() { return this } } const m = new A().m; return m()");
add("class A { m() { return this } } const m = A.prototype.m; return [m.call(5), typeof m.call(5)]");
add("class A { m() { return typeof this } } return A.prototype.m.call('s')");
add("class A { static m() { return this } } const m = A.m; return m()");
add("class A { static m() { return this } } class B extends A {} return B.m() === B");
add("class A { static m() { return this } } return A.m.call(1)");
add("class A { get g() { return this } } return typeof Object.getOwnPropertyDescriptor(A.prototype, 'g').get.call(1)");
add("class A { set s(v) { globalThis.th = this } } Object.getOwnPropertyDescriptor(A.prototype, 's').set.call(1, 2); return globalThis.th");
add("class A { m() { return () => this } } const o = new A; return o.m()() === o");
add("class A { m() { return function () { return this } } } return new A().m()()");
add("class A { m() { return [1].map(function () { return this })[0] } } return new A().m()");
add("class A { m() { return [1].map(() => this)[0] === this } } return new A().m()");
add("class A { m() { return [1].map(function () { return this === 5 }, 5)[0] } } return new A().m()");
add("class A { m() { return eval('this') === this } } return new A().m()");
add("class A { m() { return (0, eval)('typeof this') } } return new A().m()");
add("class A { m() { return new Function('return this')() === globalThis } } return new A().m()");
add("class A { m() { return new Function('\"use strict\"; return this')() } } return new A().m()");
add("class A { constructor() { this.a = this } } return new A().a instanceof A");
add("class A { constructor() { return this } } return new A() instanceof A");
add("class A { constructor() { this.f = function () { return this } } } const o = new A; return o.f() === o");
add("class A { constructor() { this.f = () => this } } const o = new A; return o.f.call(1) === o");
add("class A { constructor() { this.f = () => this } } const o = new A; return o.f.bind(1)() === o");
add("class A { constructor() { this.f = () => new.target } } const o = new A; return o.f() === A");
add("class A { constructor() { this.f = () => super.x } } const o = new A; return o.f()");
add("class B { x = 'bx' } class A extends B { constructor() { super(); this.f = () => super.x } } return new A().f()");
add("class B { get x() { return this.v } } class A extends B { constructor() { super(); this.v = 7; this.f = () => super.x } } return new A().f()");
add("class B { set x(v) { this.w = v } } class A extends B { constructor() { super(); super.x = 5 } } const o = new A; return [o.w, Object.hasOwn(o, 'x')]");
add("class B {} class A extends B { constructor() { super(); super.x = 5 } } const o = new A; return [o.x, Object.hasOwn(o, 'x')]");
add("class B {} class A extends B { constructor() { super(); super.x = 5; return Object.getOwnPropertyDescriptor(this, 'x') } } return new A()");
add("class B { static set x(v) { this.w = v } } class A extends B { static m() { super.x = 5 } } A.m(); return [A.w, B.w]");
add("class B {} B.prototype.x = 1; class A extends B { m() { return super.x } } return new A().m()");
add("class B {} class A extends B { m() { return super.x } } Object.setPrototypeOf(A.prototype, { x: 'new' }); return new A().m()");
add("class B { m() { return 'B' } } class C { m() { return 'C' } } class A extends B { m() { return super.m() } } Object.setPrototypeOf(A.prototype, C.prototype); return new A().m()");
add("const o = { m() { return super.x }, __proto__: { x: 'p' } }; return o.m()");
add("const o = { m() { return super.x }, __proto__: { get x() { return this === o } } }; return o.m()");
add("const o = { m() { return super.toString === Object.prototype.toString } }; return o.m()");
add("const o = { m() { super.x = 1; return Object.hasOwn(this, 'x') } }; return o.m()");
add("const o = { m() { return () => super.toString === Object.prototype.toString } }; return o.m()()");
add("const o = { m() { return eval('super.toString') === Object.prototype.toString } }; return o.m()");
add("const o = { m: function () { return super.x } }; return 1");
add("const o = { m() { return delete super.x } }; return (() => { try { return o.m() } catch (e) { return e.name + ': ' + e.message } })()");
add("const o = { m() { return typeof super.x } }; return o.m()");
add("const o = { m() { return super['to' + 'String'] === Object.prototype.toString } }; return o.m()");
add("const o = { m() { return super.x?.y } }; return o.m()");
add("const o = { m() { super.x++; return this.x } }; return o.m()");
add("const o = { m() { super.x ??= 3; return [this.x, Object.hasOwn(this, 'x')] } }; return o.m()");
add("const o = { m() { return super.m } , __proto__: { m() {} } }; return o.m().name");
add("class A { static m() { return super.name } } return A.m()");
add("class A { static m() { return super.call === Function.prototype.call } } return A.m()");
add("class A extends null { m() { return super.x } } return (() => { try { return new A().m() } catch (e) { return e.name + ': ' + e.message } })()");
add("class A extends null { static m() { return super.x } } return A.m()");
add("class A extends null { static m() { return super.toString === Function.prototype.toString } } return A.m()");
add("class A { m() { return super.x } } return new A().m()");
add("class A { m() { return super.toString === Object.prototype.toString } } return new A().m()");
add("class A { static x() { return 'sx' } } class B extends A { static y() { return super.x() } } return B.y()");
add("class A { static x() { return this } } class B extends A { static y() { return super.x() } } return B.y() === B");
add("class A { x() { return this } } class B extends A { y() { return super.x() } } const b = new B; return b.y() === b");
add("class A { x() { return this } } class B extends A { y() { return [1].map(function () { return super.x }) } } return 1");
add("class A { x() { return 'x' } } class B extends A { y() { return [1].map(() => super.x())[0] } } return new B().y()");
add("class A { x() { return 'x' } } class B extends A { async y() { return super.x() } } return 1");
addAsync("class A { x() { return 'x' } } class B extends A { async y() { return super.x() } } new B().y().then(v => { globalThis.R = S(v) })");
addAsync("class A { x() { return 'x' } } class B extends A { async y() { await 0; return super.x() } } new B().y().then(v => { globalThis.R = S(v) })");
addAsync("class A { x() { return 'x' } } class B extends A { *y() { yield super.x() } } globalThis.R = S([...new B().y()])");
addAsync("class A { x() { return 'x' } } class B extends A { async *y() { yield super.x() } } new B().y().next().then(v => { globalThis.R = S(v.value) })");
addAsync("class A { constructor() { this.r = 1 } } class B extends A { constructor() { (async () => { super() })(); } } try { new B; globalThis.R = 'nothrow' } catch (e) { globalThis.R = e.name }");
addAsync("class A {} class B extends A { constructor() { Promise.resolve().then(() => this) ; super() } } new B; Promise.resolve().then(() => { globalThis.R = 'ok' })");
addAsync("class A {} class B extends A { constructor() { Promise.resolve().then(() => { globalThis.R = S(typeof this) }); super() } } new B");
addAsync("class A {} class B extends A { constructor() { const p = Promise.resolve().then(() => this); super(); p.then(v => { globalThis.R = S(v === this) }) } } new B");
addAsync("class A {} class B extends A { constructor() { Promise.resolve().then(() => super()).catch(e => { globalThis.R = S(e.name + ': ' + e.message) }); super() } } new B");

// ---- Mistos finais: new.target e herança em combinações.
add("function F() { return new.target } class B extends F {} return new B() === B");
add("function F() { return Reflect.construct(Object, [], new.target) } class B extends F {} return Object.getPrototypeOf(new B) === B.prototype");
add("class A { constructor() { return Reflect.construct(Array, [], new.target) } } class B extends A {} const b = new B; return [Array.isArray(b), b instanceof B]");
add("class A { constructor() { return Reflect.construct(Map, [], new.target) } } class B extends A { m() { return 'm' } } const b = new B; return [b.m(), b.size, b instanceof Map]");
add("class A { constructor() { return Reflect.construct(Error, ['x'], new.target) } } class B extends A {} const b = new B; return [b.message, b instanceof B, b instanceof Error]");
add("class A { constructor() { return Reflect.construct(Promise, [r => r()], new.target) } } class B extends A {} return new B() instanceof B");
add("class A { constructor() { return Reflect.construct(Date, [0], new.target) } } class B extends A {} return [new B().getTime(), new B() instanceof B]");
add("class A { constructor() { return Reflect.construct(RegExp, ['a'], new.target) } } class B extends A {} return [new B().test('a'), new B() instanceof B]");
add("class A { constructor() { return Reflect.construct(Function, ['return 1'], new.target) } } class B extends A {} return [new B()(), new B() instanceof B]");
add("class A { constructor() { return Reflect.construct(Uint8Array, [2], new.target) } } class B extends A {} return [new B().length, new B() instanceof B]");
add("class A { constructor() { return Object.create(new.target.prototype) } } class B extends A { x = 1 } return new B().x");
add("class A { constructor() { return Object.setPrototypeOf({}, new.target.prototype) } } class B extends A { m() { return 1 } } return new B().m()");
add("class A { constructor() { this.n = new.target.name } } return [new A().n, Reflect.construct(A, [], class Z {}).n]");
add("class A { constructor() { this.n = new.target.name } } return Reflect.construct(A, [], function () {}).n");
add("class A { constructor() { this.n = new.target.name } } return Reflect.construct(A, [], (() => { function Q() {} return Q })()).n");
add("class A { constructor() { this.n = typeof new.target } } return Reflect.construct(A, [], Proxy.bind(null, {}, {})) && 1");
add("class A { static create() { return new this() } } class B extends A {} return [A.create() instanceof A, B.create() instanceof B]");
add("class A { static create() { return new this.prototype.constructor() } } class B extends A {} return B.create() instanceof B");
add("class A { clone() { return new this.constructor() } } class B extends A {} return new B().clone() instanceof B");
add("class A { clone() { return new (this.constructor[Symbol.species] || this.constructor)() } } class B extends A { static get [Symbol.species]() { return A } } return [new B().clone() instanceof B, new B().clone() instanceof A]");
add("class A { constructor() { A.count = (A.count || 0) + 1 } } class B extends A {} class C extends B {} new C; new B; new A; return A.count");
add("const log = []; class A { constructor() { log.push('A:' + new.target.name) } } class B extends A { constructor() { log.push('B1'); super(); log.push('B2') } } class C extends B { constructor() { log.push('C1'); super(); log.push('C2') } } new C; return log");
add("const log = []; class A { x = log.push('Ax') } class B extends A { y = log.push('By') } class C extends B { z = log.push('Cz'); constructor() { log.push('C-pre'); super(); log.push('C-post') } } new C; return log");
add("const log = []; class A { constructor() { log.push(this.constructor.name); log.push(Object.keys(this)) } } class B extends A { f = 1 } new B; return log");
add("const log = []; class A { constructor() { log.push(Object.getPrototypeOf(this) === B.prototype) } } class B extends A {} new B; return log");
add("class A { constructor() { this.t = this.constructor.name } } class B extends A {} class C extends B {} return new C().t");
add("class A { static name2 = this.name } class B extends A {} return [A.name2, B.name2]");
add("class A { static make = () => new this() } class B extends A {} return B.make() instanceof A");
add("class A { static make = () => new this() } class B extends A {} return [B.make() instanceof B, A.make.call(B) instanceof B]");
add("class A { static m() { return this.name } } return [A.m.call(class Z {}), (0, A.m)()]");
add("class A { static m() { return this } } return (() => { try { return (0, A.m)() } catch (e) { return e.name } })()");
add("class A { m() { return this } } return (0, A.prototype.m)()");
add("class A { constructor() { this.constructor = 1 } } return new A() instanceof A");
add("class A { constructor() { Object.setPrototypeOf(this, null) } } return new A() instanceof A");
add("class A { constructor() { Object.setPrototypeOf(this, Array.prototype) } } const o = new A; return [Array.isArray(o), o instanceof Array]");
add("class A { constructor() { return new Proxy(this, {}) } } class B extends A {} return new B instanceof B");
add("class A { constructor() { return new Proxy(this, { getPrototypeOf() { return Array.prototype } }) } } return new A instanceof Array");
add("class A { static [Symbol.hasInstance](v) { return Array.isArray(v) } } return [[] instanceof A, {} instanceof A]");
add("class A { static [Symbol.hasInstance](v) { return false } } class B extends A {} return [new B instanceof A, new B instanceof B]");
add("class A { static [Symbol.hasInstance](v) { return v === 1 } } class B extends A {} return [1 instanceof B, 1 instanceof A]");
add("class A {} Object.defineProperty(A, Symbol.hasInstance, { value: () => true }); return [1 instanceof A, null instanceof A]");
add("Object.defineProperty(Function.prototype, Symbol.hasInstance, { value: () => 'x', configurable: true }); const r = 1 instanceof function () {}; delete Function.prototype[Symbol.hasInstance]; return r");
add("function F() {} const bound = F.bind(null); Object.defineProperty(bound, Symbol.hasInstance, { value: () => 'h' }); return {} instanceof bound");
add("function F() {} const proto = F.prototype; F.prototype = {}; return [new F instanceof F, Object.create(proto) instanceof F]");
add("function F() {} const o = new F; F.prototype = {}; return o instanceof F");
add("function F() {} const o = new F; Object.setPrototypeOf(o, Object.create(F.prototype)); return o instanceof F");
add("return [[] instanceof Object, Object.create(null) instanceof Object, (() => {}) instanceof Function, (async () => {}) instanceof Function, (class {}) instanceof Function]");
add("return [(function* () {}) instanceof Function, Object.getPrototypeOf(function* () {}) === Function.prototype]");
add("return [1 instanceof Number, new Number(1) instanceof Number, 'a' instanceof String, Symbol() instanceof Symbol, 1n instanceof BigInt, Object(1n) instanceof BigInt]");
add("return (() => { try { return 1 instanceof 1 } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return 1 instanceof {} } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return 1 instanceof null } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return 1 instanceof undefined } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof Math } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof Symbol.iterator } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof { prototype: {} } } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof new Proxy({}, {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof new Proxy(function () {}, {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof new Proxy(() => {}, {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof (() => {}) } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof ({ m() {} }).m } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof function () {}.bind() } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof Math.max } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof parseInt } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof Object.prototype.toString } catch (e) { return e.name + ': ' + e.message } })()");
add("return (() => { try { return ({}) instanceof Function.prototype } catch (e) { return e.name + ': ' + e.message } })()");

// ---- Execução: um processo por programa, medido com vm.runInThisContext capturando globalThis.R.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ctor-this-golden-"));
const runner = path.join(dir, "runner.js");
fs.writeFileSync(
  runner,
  [
    "const vm = require('node:vm');",
    "const fs = require('fs');",
    "const src = fs.readFileSync(process.argv[2], 'utf8');",
    "try { vm.runInThisContext(src) } catch (e) { globalThis.R = 'SCRIPT ' + e.name + ': ' + e.message }",
    "(async () => {",
    "  for (let i = 0; i < 50; i++) await null;",
    "  const r = globalThis.R;",
    "  console.log('\\u0001' + JSON.stringify(typeof r === 'string' ? r : r === undefined ? '<undefined>' : String(r)));",
    "})();",
  ].join("\n"),
);
const source_file = path.join(dir, "program.js");
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  fs.writeFileSync(source_file, body);
  const run = spawnSync(process.execPath, [runner, source_file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRE.length, PRE.length + 200)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  // Programa que nem compila (SyntaxError no script) não entra: o golden mede comportamento, e os early errors
  // dependem de mensagem do parser, já cobertos em outros golden.
  if (result.startsWith("SCRIPT ")) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(body) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
