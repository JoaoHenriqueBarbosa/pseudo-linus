// Gera tests/golden/object_model_bun.tsv: modelo de propriedades (defineProperty, ordem de chaves, frozen/sealed,
// arrays, arguments, funções, classes) medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa é um corpo de função
// (`sloppy` ou `strict`) cujo retorno é serializado por `S`; exceção vira "ERR Nome: mensagem".
// Uso: bun scripts/gen-object-model-golden.js > tests/golden/object_model_bun.tsv
const { emitFactored, prepareProgram } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  "function S(v,d){d=d||0;var t=typeof v;if(t==='string')return JSON.stringify(v);if(t==='symbol')return String(v);" +
  "if(t==='function')return 'fn';if(v===null||t!=='object')return Object.is(v,-0)?'-0':String(v);if(d>3)return '...';" +
  "if(Array.isArray(v)){var r=[];for(var i=0;i<v.length&&i<20;i++)r.push(i in v?S(v[i],d+1):'<hole>');" +
  "return '['+r.join(',')+(v.length>20?',..'+v.length:'')+']'}" +
  "return '{'+Reflect.ownKeys(v).map(function(x){return String(x)+':'+S(v[x],d+1)}).join(',')+'}'}\n" +
  "function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);return d===undefined?'undefined':S(d)}\n";

const programs = [];
const sloppy = body => programs.push(["sloppy", body]);
const strict = body => programs.push(["strict", body]);
const both = body => {
  sloppy(body);
  strict(body);
};

const J = JSON.stringify;
const vals = {
  value: ["", "value:1", "value:undefined"],
  writable: ["", "writable:true", "writable:false"],
  get: ["", "get:function(){return 7}", "get:undefined"],
  set: ["", "set:function(v){}", "set:undefined"],
  enumerable: ["", "enumerable:true", "enumerable:false"],
  configurable: ["", "configurable:true", "configurable:false"],
};

// ---- 1. defineProperty com todas as combinações de descritor (729).
const keys = Object.keys(vals);
function* combos(i, acc) {
  if (i === keys.length) {
    yield acc.filter(Boolean).join(",");
    return;
  }
  for (const v of vals[keys[i]]) yield* combos(i + 1, [...acc, v]);
}
for (const desc of combos(0, [])) {
  sloppy(`var o = {}; Object.defineProperty(o, 'p', {${desc}}); return D(o, 'p')`);
}

// ---- 2. Redefinição: descritor inicial x mudança.
const initials = [];
for (const w of [true, false]) for (const e of [true, false]) for (const c of [true, false]) initials.push(`{value:1,writable:${w},enumerable:${e},configurable:${c}}`);
for (const e of [true, false]) for (const c of [true, false]) initials.push(`{get:function(){return 1},set:function(v){},enumerable:${e},configurable:${c}}`);
initials.push("{get:function(){return 1},enumerable:false,configurable:false}");
initials.push("{set:function(v){},enumerable:false,configurable:false}");
const changes = [
  "{}",
  "{value:1}",
  "{value:2}",
  "{value:NaN}",
  "{value:-0}",
  "{writable:true}",
  "{writable:false}",
  "{enumerable:true}",
  "{enumerable:false}",
  "{configurable:true}",
  "{configurable:false}",
  "{value:2,writable:false}",
  "{value:1,writable:true,enumerable:true,configurable:true}",
  "{get:function(){return 2}}",
  "{get:undefined}",
  "{set:function(v){}}",
  "{set:undefined}",
  "{get:undefined,set:undefined}",
  "{get:function(){return 2},set:function(v){}}",
  "{get:function(){return 2},enumerable:true}",
  "{get:function(){return 2},configurable:true}",
  "{value:1,get:function(){}}",
  "{writable:false,set:function(){}}",
  "{enumerable:true,configurable:true}",
];
for (const init of initials) {
  for (const ch of changes) {
    sloppy(`var o = {}; Object.defineProperty(o, 'p', ${init}); var r; try { Object.defineProperty(o, 'p', ${ch}); r = 'ok' } catch (e) { r = e.name + ': ' + e.message } return [r, D(o, 'p')]`);
  }
}
// Mesma coisa com a getter fixa (mesma função), que redefinir é permitido.
sloppy("var g = function(){}; var o = {}; Object.defineProperty(o, 'p', {get: g}); Object.defineProperty(o, 'p', {get: g}); return D(o, 'p')");
sloppy("var g = function(){}; var o = {}; Object.defineProperty(o, 'p', {get: g}); Object.defineProperty(o, 'p', {get: function(){}}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: NaN}); Object.defineProperty(o, 'p', {value: NaN}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: 0}); Object.defineProperty(o, 'p', {value: -0}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: -0}); Object.defineProperty(o, 'p', {value: 0}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: 1, writable: true}); Object.defineProperty(o, 'p', {writable: false}); Object.defineProperty(o, 'p', {writable: false}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: 1, writable: true}); Object.defineProperty(o, 'p', {writable: false}); Object.defineProperty(o, 'p', {writable: true}); return D(o, 'p')");

// ---- 3. Descritores inválidos e defineProperties.
const badDescs = ["undefined", "null", "1", "'a'", "true", "Symbol('s')", "1n", "NaN"];
for (const d of badDescs) {
  sloppy(`var o = {}; Object.defineProperty(o, 'p', ${d}); return D(o, 'p')`);
  sloppy(`var o = {}; Object.defineProperties(o, {p: ${d}}); return D(o, 'p')`);
  sloppy(`return Reflect.defineProperty({}, 'p', ${d})`);
}
const badTargets = ["undefined", "null", "1", "'a'", "true", "Symbol('s')", "function(){}"];
for (const t of badTargets) {
  sloppy(`return S(Object.defineProperty(${t}, 'p', {value: 1}))`);
  sloppy(`return S(Object.defineProperties(${t}, {p: {value: 1}}))`);
  sloppy(`return Reflect.defineProperty(${t}, 'p', {value: 1})`);
  sloppy(`return S(Object.getOwnPropertyDescriptor(${t}, 'p'))`);
  sloppy(`return S(Object.getOwnPropertyDescriptors(${t}))`);
  sloppy(`return S(Object.keys(${t}))`);
  sloppy(`return S(Object.getOwnPropertyNames(${t}))`);
  sloppy(`return S(Object.entries(${t}))`);
  sloppy(`return S(Object.getPrototypeOf(${t}))`);
  sloppy(`return S(Object.freeze(${t}))`);
  sloppy(`return S(Object.isFrozen(${t}))`);
  sloppy(`return S(Object.isSealed(${t}))`);
  sloppy(`return S(Object.isExtensible(${t}))`);
  sloppy(`return S(Object.seal(${t}))`);
  sloppy(`return S(Object.preventExtensions(${t}))`);
  sloppy(`return S(Object.setPrototypeOf(${t}, null))`);
  sloppy(`return S(Object.setPrototypeOf({}, ${t}))`);
  sloppy(`return S(Object.create(${t}))`);
  sloppy(`return S(Object.assign(${t}, {a: 1}))`);
  sloppy(`return S(Object.fromEntries(${t}))`);
}
const getSetBad = ["1", "'a'", "null", "true", "{}", "[]", "Symbol()", "{call(){}}"];
for (const g of getSetBad) {
  sloppy(`return D(Object.defineProperty({}, 'p', {get: ${g}}), 'p')`);
  sloppy(`return D(Object.defineProperty({}, 'p', {set: ${g}}), 'p')`);
  sloppy(`return D(Object.defineProperty({}, 'p', {get: ${g}, set: function(){}}), 'p')`);
}
sloppy("return S(Object.defineProperty({}, 'p', {get: function(){}, value: 1}))");
sloppy("return S(Object.defineProperty({}, 'p', {get: function(){}, writable: false}))");
sloppy("return S(Object.defineProperty({}, 'p', {set: function(){}, value: undefined}))");
sloppy("return S(Object.defineProperty({}, 'p', {set: function(){}, writable: true}))");
sloppy("return S(Object.defineProperty({}, 'p', {get: undefined, value: 1}))");
sloppy("return S(Object.defineProperty({}, 'p', {get: undefined, set: undefined, writable: false}))");
sloppy("return Reflect.defineProperty({}, 'p', {get: function(){}, value: 1})");
sloppy("return S(Object.defineProperties({}, {a: {value: 1}, b: 5}))");
sloppy("var o = {}; try { Object.defineProperties(o, {a: {value: 1}, b: 5}) } catch (e) {} return S(Object.getOwnPropertyNames(o))");
sloppy("return D(Object.defineProperties({}, {a: {value: 1, enumerable: true}, b: {get: function(){}}}), 'b')");
sloppy("return S(Object.keys(Object.defineProperties({}, {b: {value: 1, enumerable: true}, a: {value: 2, enumerable: true}, 1: {value: 3, enumerable: true}})))");
sloppy("return S(Object.keys(Object.defineProperties({}, Object.create({inherited: {value: 1, enumerable: true}}, {own: {value: {value: 2}, enumerable: false}}))))");
sloppy("return S(Object.getOwnPropertyNames(Object.defineProperties({}, Object.create({inherited: {value: 1}}, {own: {value: {value: 2}, enumerable: true}}))))");
sloppy("return S(Object.getOwnPropertyNames(Object.defineProperties({}, 'ab')))");
sloppy("return S(Object.getOwnPropertyNames(Object.defineProperties({}, [{value: 1}])))");
sloppy("var s = Symbol('k'); return S(Object.getOwnPropertySymbols(Object.defineProperties({}, {[s]: {value: 1}})))");
sloppy("var p = Object.create({inh: 1}); return S(Object.defineProperty({}, 'x', p))");
sloppy("var d = Object.create({value: 5, enumerable: true}); return D(Object.defineProperty({}, 'x', d), 'x')");
sloppy("var d = new Proxy({}, {has(t, k) { return k === 'value' }, get(t, k) { return 9 }}); return D(Object.defineProperty({}, 'x', d), 'x')");
sloppy("var order = []; var d = new Proxy({}, {has(t, k) { order.push(k); return false }}); Object.defineProperty({}, 'x', d); return order");
sloppy("var o = {}; Object.defineProperty(o, 'p', {value: 1, extra: 2, foo: 3}); return D(o, 'p')");
sloppy("var o = {}; Object.defineProperty(o, 1, {value: 1}); Object.defineProperty(o, {toString(){return 'k'}}, {value: 2}); return S(Object.getOwnPropertyNames(o))");
sloppy("var o = {}; Object.defineProperty(o, Symbol.iterator, {value: 1}); return S(Object.getOwnPropertySymbols(o))");

// ---- Reflect.defineProperty: devolve false em vez de lançar.
for (const init of initials.slice(0, 8)) {
  for (const ch of ["{value:2}", "{writable:true}", "{enumerable:true}", "{configurable:true}", "{get:function(){}}"]) {
    sloppy(`var o = {}; Object.defineProperty(o, 'p', ${init}); return [Reflect.defineProperty(o, 'p', ${ch}), D(o, 'p')]`);
  }
}
// Extensibilidade.
for (const how of ["preventExtensions", "seal", "freeze"]) {
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); try { Object.defineProperty(o, 'b', {value: 1}); return 'ok' } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); return Reflect.defineProperty(o, 'b', {value: 1})`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); try { Object.defineProperty(o, 'a', {value: 2}); return D(o, 'a') } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); try { Object.defineProperty(o, 'a', {enumerable: false}); return D(o, 'a') } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); try { Object.defineProperty(o, 'a', {writable: false}); return D(o, 'a') } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]`);
  sloppy(`var o = {}; Object[${J(how)}](o); return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]`);
  sloppy(`var o = {get a() {return 1}}; Object[${J(how)}](o); return [D(o, 'a'), Object.isFrozen(o), Object.isSealed(o)]`);
  sloppy(`var o = {a: 1}; Object[${J(how)}](o); return [D(o, 'a'), S(Object.getOwnPropertyDescriptors(o))]`);
  sloppy(`var o = {a: 1, 5: 2, [Symbol.iterator]: 3}; Object[${J(how)}](o); return S(Object.getOwnPropertyDescriptors(o))`);
  sloppy(`var o = Object[${J(how)}](function f(a){}); return [D(o, 'length'), D(o, 'name'), D(o, 'prototype')]`);
  sloppy(`var o = Object[${J(how)}]([1, 2, 3]); return [D(o, 'length'), D(o, 0), Object.isFrozen(o), Object.isSealed(o)]`);
  sloppy(`var o = Object[${J(how)}]([]); return [D(o, 'length'), Object.isFrozen(o), Object.isSealed(o)]`);
  sloppy(`var o = Object[${J(how)}]([1, , 3]); return [D(o, 1), D(o, 'length'), Object.isFrozen(o)]`);
  sloppy(`var o = Object[${J(how)}](new Error('x')); return [D(o, 'message'), Object.isFrozen(o), Object.isSealed(o)]`);
  sloppy(`var o = Object[${J(how)}](/a/g); return [D(o, 'lastIndex'), Object.isFrozen(o)]`);
  sloppy(`var o = Object[${J(how)}](Object('s')); return [D(o, 'length'), D(o, 0), Object.isFrozen(o)]`);
  sloppy(`var o = Object[${J(how)}](Object.create({inh: 1})); return [Object.isFrozen(o), Object.isSealed(o), o.inh]`);
}

// Typed arrays.
for (const ta of ["Uint8Array", "Float64Array", "Int32Array", "Uint8ClampedArray", "BigInt64Array"]) {
  const len = ta.startsWith("Big") ? "new " + ta + "(2)" : "new " + ta + "(2)";
  sloppy(`var o = ${len}; return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]`);
  sloppy(`var o = new ${ta}(0); return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]`);
  sloppy(`var o = new ${ta}(0); Object.freeze(o); return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]`);
  sloppy(`var o = ${len}; try { Object.freeze(o); return 'ok' } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = ${len}; Object.seal(o); return [Object.isSealed(o), Object.isFrozen(o), D(o, 0)]`);
  sloppy(`var o = ${len}; Object.preventExtensions(o); return [Object.isExtensible(o), Object.isSealed(o), Object.isFrozen(o)]`);
  sloppy(`var o = ${len}; return D(o, 0)`);
  sloppy(`var o = ${len}; return [D(o, 2), D(o, -1), D(o, '1.5'), D(o, 'x')]`);
  sloppy(`var o = ${len}; return Reflect.defineProperty(o, 0, {value: 1, writable: true, enumerable: true, configurable: true})`);
  sloppy(`var o = ${len}; return Reflect.defineProperty(o, 0, {value: 1, configurable: false})`);
  sloppy(`var o = ${len}; return Reflect.defineProperty(o, 0, {value: 1, enumerable: false})`);
  sloppy(`var o = ${len}; return Reflect.defineProperty(o, 0, {get(){}})`);
  sloppy(`var o = ${len}; return Reflect.defineProperty(o, 5, {value: 1})`);
  sloppy(`var o = ${len}; o[5] = 1; return [o[5], S(Object.keys(o)), 5 in o]`);
  sloppy(`var o = ${len}; o.x = 1; return S(Object.keys(o))`);
  sloppy(`var o = ${len}; return [delete o[0], delete o[7], 0 in o]`);
  strict(`var o = ${len}; return delete o[0]`);
  sloppy(`var o = ${len}; return S(Object.keys(o)) + S(Object.getOwnPropertyNames(o))`);
  strict(`var o = ${len}; o[3] = 1; return S(Object.keys(o))`);
  sloppy(`var o = ${len}; Object.defineProperty(o, 'length', {value: 1}); return o.length`);
  sloppy(`var o = ${len}; return Object.getOwnPropertyDescriptor(o, 'length') === undefined`);
}
sloppy("var o = new Uint8Array(3); o[0] = 300; o[1] = -1; o['2'] = 2.7; return S(Array.from(o))");
sloppy("var o = new Uint8Array([1, 2, 3]); return S(Object.entries(o))");
sloppy("var o = new Float32Array(2); return S(Object.getOwnPropertyDescriptors(o))");
sloppy("var o = new Uint8Array(2); o.foo = 1; o[Symbol.iterator] = 1; return S(Reflect.ownKeys(o))");
sloppy("var o = new Uint8Array(2); return S(Object.assign({}, o))");
sloppy("var o = new Uint8Array(2); return JSON.stringify(o) + JSON.stringify({...o})");
sloppy("var o = new Uint8Array(2); var r = []; for (var k in o) r.push(k); return r");
sloppy("var o = new Uint8Array(2); Object.defineProperty(o, 'x', {value: 1, enumerable: true}); return Object.keys(o)");
sloppy("var o = new Uint8Array(2); o['-0'] = 5; o['1e3'] = 5; o['Infinity'] = 5; o['NaN'] = 5; return [o['-0'], o['1e3'], o['Infinity'], o['NaN'], S(Object.keys(o))]");
sloppy("var o = new Uint8Array(2); o['01'] = 5; o['1.0'] = 5; return [o['01'], o['1.0'], S(Object.keys(o))]");
sloppy("var o = new Uint8Array(2); return [Reflect.set(o, 5, 1), Reflect.set(o, 1, 1), Reflect.set(o, '-0', 1)]");
strict("var o = new Uint8Array(2); o[5] = 1; o['-0'] = 1; return 'ok'");
sloppy("var o = Object.freeze(new Uint8Array(0)); return Object.isFrozen(o)");
sloppy("var o = new Uint8Array(2); return Object.isFrozen(Object.preventExtensions(o))");

// ---- 4. Arrays: length, índices.
const arrLen = ["0", "1", "2", "3", "10", "-1", "1.5", "'3'", "'abc'", "NaN", "Infinity", "2**32", "2**32-1", "2**32-2", "null", "undefined", "true", "{valueOf(){return 2}}", "-0", "'0x2'", "[]", "[2]", "1n"];
for (const l of arrLen) {
  sloppy(`var a = [1, 2, 3]; try { a.length = ${l}; return [a.length, S(a)] } catch (e) { return e.name + ': ' + e.message }`);
  strict(`var a = [1, 2, 3]; try { a.length = ${l}; return [a.length, S(a)] } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var a = [1, 2, 3]; try { Object.defineProperty(a, 'length', {value: ${l}}); return [a.length, S(a)] } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return new Array(${l}).length } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return Array(${l}).length } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return S(Array.from({length: ${l}}).length) } catch (e) { return e.name + ': ' + e.message }`);
}
const lenDescs = [
  "{value: 1}", "{value: 5}", "{writable: false}", "{writable: true}", "{enumerable: true}", "{enumerable: false}", "{configurable: true}",
  "{configurable: false}", "{get(){}}", "{value: 3, writable: false}", "{value: 2, writable: false}", "{value: 4, writable: false}",
  "{value: 1, enumerable: false, configurable: false, writable: true}", "{set(v){}}", "{}",
];
for (const d of lenDescs) {
  sloppy(`var a = [1, 2, 3]; try { Object.defineProperty(a, 'length', ${d}); return [D(a, 'length'), S(a)] } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var a = [1, 2, 3]; return [Reflect.defineProperty(a, 'length', ${d}), D(a, 'length'), S(a)]`);
  sloppy(`var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); return [Reflect.defineProperty(a, 'length', ${d}), D(a, 'length'), S(a)]`);
  sloppy(`var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); return [Reflect.defineProperty(a, 'length', ${d}), D(a, 'length'), S(a)]`);
}
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); a.length = 0; return [a.length, S(a)]");
strict("var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); a.length = 0; return [a.length, S(a)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); return [Reflect.defineProperty(a, 'length', {value: 0}), a.length, S(a)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 1, {configurable: false}); try { Object.defineProperty(a, 'length', {value: 0}) } catch (e) { return e.name + ': ' + e.message + a.length }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a.push(4); return 1");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); try { a.push(4) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); try { a.pop() } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a[5] = 1; return [a.length, S(a)]");
strict("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a[5] = 1; return [a.length, S(a)]");
strict("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a[1] = 9; return [a.length, S(a)]");
strict("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a.length = 3; return 'ok'");
strict("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); a.length = 2; return 'ok'");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); try { a.shift() } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); try { a.splice(0, 1) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); try { a.unshift(0) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.sort() } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.reverse() } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.push(1) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.pop() } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.fill(0) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.freeze(a); try { a.length = 0 } catch (e) { return e.name + ': ' + e.message + S(a) } return S(a)");
strict("var a = [3, 1, 2]; Object.freeze(a); a.length = 0");
strict("var a = [3, 1, 2]; Object.freeze(a); a[0] = 0");
strict("var a = [3, 1, 2]; Object.freeze(a); a[5] = 0");
strict("var a = [3, 1, 2]; Object.freeze(a); delete a[0]");
strict("var a = [3, 1, 2]; Object.seal(a); delete a[0]");
strict("var a = [3, 1, 2]; Object.seal(a); a[0] = 7; return S(a)");
strict("var a = [3, 1, 2]; Object.seal(a); a[3] = 7; return S(a)");
strict("var a = [3, 1, 2]; Object.preventExtensions(a); a[3] = 7; return S(a)");
strict("var a = [3, 1, 2]; Object.preventExtensions(a); delete a[0]; return S(a)");
strict("var a = [3, 1, 2]; Object.preventExtensions(a); a.length = 1; return S(a)");
strict("var a = [3, 1, 2]; Object.preventExtensions(a); a.push(1)");
sloppy("var a = [3, 1, 2]; Object.preventExtensions(a); try { a.push(1) } catch (e) { return e.name + ': ' + e.message + S(a) }");
sloppy("var a = [3, 1, 2]; Object.seal(a); a.length = 0; return S(a)");
sloppy("var a = [1, , 3]; return [D(a, 0), D(a, 1), D(a, 'length'), 1 in a, S(Object.keys(a))]");
sloppy("var a = [1, 2, 3]; a.foo = 1; a[-1] = 2; a['01'] = 3; a['1.0'] = 4; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a['4294967294'] = 1; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a['4294967295'] = 1; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[4294967296] = 1; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[-1] = 1; a[1.5] = 2; a['1e3'] = 3; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[2**32-2] = 1; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[2**32-2] = 1; a[2**32-1] = 2; a[2**32] = 3; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[0] = 1; a['00'] = 2; a[' 1'] = 3; a['+1'] = 4; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[4294967295] = 1; return [a.length, a[4294967295], 4294967295 in a, S(Object.getOwnPropertyNames(a))]");
sloppy("var a = []; a.length = 4294967295; try { a.push(1) } catch (e) { return e.name + ': ' + e.message + a.length + S(Object.keys(a)) } return 'ok'");
sloppy("var a = []; a.length = 4294967295; try { a.push(1, 2) } catch (e) { return e.name + ': ' + e.message + a.length + S(Object.keys(a)) } return 'ok'");
sloppy("var a = []; a.length = 4294967295; try { a.push() ; return a.length } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 4294967295; try { a.unshift(1) } catch (e) { return e.name + ': ' + e.message + a.length }");
sloppy("var a = []; a.length = 4294967295; try { a.concat([1]) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 4294967295; return [a.pop(), a.length]");
sloppy("var a = []; a.length = 4294967295; return [a.indexOf(1), a.lastIndexOf(1, 2), a.length]");
sloppy("var a = []; a.length = 4294967295; a[4294967294] = 'x'; return [a.length, a.at(-1), a.lastIndexOf('x'), a.includes('x')]");
sloppy("var a = []; a.length = 4294967295; try { a.length = 4294967296 } catch (e) { return e.name + ': ' + e.message + a.length }");
sloppy("var a = []; a.length = 4294967295; return [a.length, S(Object.keys(a)), D(a, 'length')]");
sloppy("var a = []; try { a.length = -1 } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; try { new Array(-1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; try { new Array(1.5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; try { new Array(4294967296) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = new Array(4294967295); return [a.length, S(Object.keys(a))]");
sloppy("var a = new Array('3'); return [a.length, S(a)]");
sloppy("var a = new Array(2, 3); return [a.length, S(a)]");
sloppy("var o = {length: 4294967296}; try { Array.prototype.push.call(o, 1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = {length: 9007199254740991}; try { Array.prototype.push.call(o, 1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = {length: 9007199254740991}; try { Array.prototype.push.call(o) ; return o.length } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = {length: 9007199254740990}; Array.prototype.push.call(o, 1); return [o.length, o[9007199254740990]]");
sloppy("var o = {length: 2**53}; Array.prototype.push.call(o); return o.length");
sloppy("var o = {length: 3}; Array.prototype.splice.call(o, 0, 0); return o.length");
sloppy("var a = [1, 2, 3]; a[10] = 5; return [a.length, S(a)]");
sloppy("var a = [1, 2, 3]; a.length = 5; a[7] = 1; return [a.length, S(Object.keys(a))]");
sloppy("var a = [1, 2, 3]; delete a[1]; return [a.length, S(a), 1 in a]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 1, {get(){return 9}}); return [S(a), D(a, 1), a.length]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 1, {value: 9, enumerable: false}); return [S(a), S(Object.keys(a)), JSON.stringify(a)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 5, {value: 9}); return [a.length, D(a, 5), S(Object.keys(a))]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 5, {value: 9, writable: false}); a[5] = 1; return [a.length, a[5]]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 4294967294, {value: 9}); return [a.length, D(a, 4294967294)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 4294967295, {value: 9}); return [a.length, D(a, 4294967295)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {value: 5}); return [a.length, S(a), S(Object.keys(a))]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {value: 1}); return [a.length, S(a), 2 in a]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 2, {writable: false}); a.length = 1; return [a.length, S(a)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 2, {get(){return 1}, configurable: false}); Object.defineProperty(a, 'length', {value: 1, writable: false}); return 1");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 0, {configurable: false}); try { Object.defineProperty(a, 'length', {value: 0, writable: false}) } catch (e) { return e.name + ': ' + e.message + [a.length, D(a, 'length')] }");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 0, {configurable: false}); Reflect.defineProperty(a, 'length', {value: 0, writable: false}); return [a.length, D(a, 'length')]");
sloppy("var a = [1, 2, 3]; return [S(Object.getOwnPropertyNames(a)), S(Reflect.ownKeys(a))]");
sloppy("var a = [3, 2, 1]; a.x = 1; a[Symbol.iterator] = 2; return S(Reflect.ownKeys(a))");
sloppy("var a = []; a.b = 1; a[2] = 1; a.a = 1; a[0] = 1; a[1] = 1; return S(Reflect.ownKeys(a))");
sloppy("var a = Array.from({length: 3}); return [S(a), S(Object.keys(a))]");
sloppy("var a = Array(3); return [S(a), S(Object.keys(a)), JSON.stringify(a)]");
sloppy("var a = [,]; return [a.length, S(Object.keys(a))]");
sloppy("var a = [1, 2, 3]; a.length = 1; a.length = 3; return [S(a), 1 in a]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); return [Object.isFrozen(a), Object.isSealed(a), Object.isExtensible(a)]");
sloppy("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); Object.freeze(a); return [Object.isFrozen(a), D(a, 'length')]");
sloppy("var a = []; Object.defineProperty(a, 'length', {writable: false}); return [Object.isFrozen(a), Object.isSealed(a)]");
sloppy("var a = []; Object.preventExtensions(a); return [Object.isFrozen(a), Object.isSealed(a)]");
sloppy("var a = [1]; Object.preventExtensions(a); Object.defineProperty(a, 0, {writable: false, configurable: false}); Object.defineProperty(a, 'length', {writable: false}); return [Object.isFrozen(a), Object.isSealed(a)]");
sloppy("var a = [1]; Object.freeze(a); Object.defineProperty(a, 'length', {configurable: false}); return D(a, 'length')");
sloppy("var a = [1,2,3]; return S(Object.getOwnPropertyDescriptors(a))");
sloppy("var a = [1,2,3]; a.length = 2; return S(Object.getOwnPropertyDescriptors(a))");
sloppy("var a = [1,,3]; return S(Object.getOwnPropertyDescriptors(a))");

// ---- 5. Ordem de chaves.
const keyLists = [
  "['b', 'a', 2, 1, 'c', 0]",
  "['z', '10', '9', 'a', '1', '-1', '01', '1.0', '4294967294', '4294967295', '4294967296', '18446744073709551615']",
  "['b', 1, 'a', 0, Symbol.iterator, 'c', Symbol.toStringTag, 3]",
  "['1e3', '1000', '2', '0x10', '16', '-0', '0', '+1', '1']",
  "['b', 'a', '2', '1', '', ' ', '0']",
  "[3, 2, 1, 0, -1, -2]",
  "[0.5, 1.5, 1, 2**31, 2**32, 2**32-1, 2**53]",
  "['__proto__', 'constructor', 'toString', 'a']",
  "['a', 'b', 'a', 'c', 'b']",
];
const ops = [
  "S(Object.keys(o))", "S(Object.getOwnPropertyNames(o))", "S(Reflect.ownKeys(o))", "S(Object.entries(o).map(function(e){return e[0]}))",
  "S(Object.values(o))", "JSON.stringify(o)", "S(Object.assign({}, o))", "S({...o})", "S(Object.getOwnPropertyDescriptors(o))",
  "(function(){var r = []; for (var k in o) r.push(k); return r.join()})()", "S(Object.getOwnPropertySymbols(o))",
  "S(Object.fromEntries(Object.entries(o)))", "S(structuredClone(o))",
];
for (const kl of keyLists) {
  for (const op of ops) {
    sloppy(`var o = {}; var ks = ${kl}; ks.forEach(function(k, i){ o[k] = i }); return ${op}`);
  }
  sloppy(`var o = {}; var ks = ${kl}; ks.forEach(function(k, i){ Object.defineProperty(o, k, {value: i, enumerable: i % 2 === 0, configurable: true}) }); return [S(Object.keys(o)), S(Reflect.ownKeys(o)), JSON.stringify(o)]`);
  sloppy(`var o = {}; var ks = ${kl}; ks.forEach(function(k, i){ o[k] = i }); ks.forEach(function(k){ delete o[k] }); ks.slice().reverse().forEach(function(k, i){ o[k] = i }); return S(Reflect.ownKeys(o))`);
  sloppy(`var o = {}; var ks = ${kl}; ks.forEach(function(k, i){ o[k] = i }); delete o[ks[0]]; o[ks[0]] = 'again'; return S(Reflect.ownKeys(o))`);
}
sloppy("var o = {b: 1, a: 2, 1: 3, 0: 4}; return JSON.stringify(o)");
sloppy("var o = {b: 1, a: 2, 1: 3, 0: 4}; var r = []; for (var k in o) r.push(k); return r");
sloppy("var o = {b: 1, [Symbol('s')]: 2, a: 3, 1: 4}; return [S(Reflect.ownKeys(o)), S(Object.keys(o))]");
sloppy("var s1 = Symbol('1'), s2 = Symbol('2'); var o = {[s2]: 1, [s1]: 2}; return S(Reflect.ownKeys(o))");
sloppy("var o = {a: 1, a: 2, b: 3}; return S(o)");
sloppy("var o = {a: 1, b: 2, a: 3}; return S(Object.keys(o))");
sloppy("var o = {get a() {return 1}, a: 2}; return D(o, 'a')");
sloppy("var o = {a: 2, get a() {return 1}}; return D(o, 'a')");
sloppy("var o = {get a() {return 1}, set a(v) {}}; return D(o, 'a')");
sloppy("var o = {set a(v) {}, get a() {return 1}}; return D(o, 'a')");
sloppy("var o = {get a() {return 1}, set a(v) {}, a: 3}; return D(o, 'a')");
sloppy("var o = {__proto__: {inh: 1}, own: 2}; return [S(Object.keys(o)), o.inh]");
sloppy("var o = {'__proto__': {inh: 1}}; return [S(Object.keys(o)), o.inh]");
sloppy("var o = {['__proto__']: {inh: 1}}; return [S(Object.keys(o)), o.inh, D(o, '__proto__')]");
sloppy("var __proto__ = {inh: 1}; var o = {__proto__}; return [S(Object.keys(o)), o.inh, D(o, '__proto__')]");
sloppy("var o = {__proto__: null}; return [Object.getPrototypeOf(o), 'toString' in o]");
sloppy("var o = {__proto__: 1}; return [Object.getPrototypeOf(o) === Object.prototype, S(Object.keys(o))]");
sloppy("var o = {__proto__: 'str'}; return Object.getPrototypeOf(o) === Object.prototype");
sloppy("var o = {__proto__: undefined}; return Object.getPrototypeOf(o) === Object.prototype");
sloppy("var o = {__proto__: function(){}}; return typeof o.call");
sloppy("var o = {__proto__: null, __proto__: null}; return 1");
sloppy("var o = {__proto__: {a: 1}, get __proto__() {return 5}}; return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__]");
sloppy("var o = {__proto__() {}}; return [Object.getPrototypeOf(o) === Object.prototype, typeof o.__proto__]");
sloppy("var o = {}; o.__proto__ = {a: 1}; return [o.a, S(Object.keys(o))]");
sloppy("var o = {}; o['__proto__'] = 5; return [Object.getPrototypeOf(o) === Object.prototype, S(Object.keys(o))]");
sloppy("var o = Object.create(null); o.__proto__ = 5; return [S(Object.keys(o)), D(o, '__proto__')]");
sloppy("var o = {}; Object.defineProperty(o, '__proto__', {value: 5, enumerable: true}); return [Object.getPrototypeOf(o) === Object.prototype, D(o, '__proto__'), JSON.stringify(o)]");
sloppy("return D(Object.prototype, '__proto__')");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); return [d.get.call(5) === Number.prototype, d.set.call(5, {}), d.get.name, d.set.name]");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.get.call(null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call(undefined, {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); return d.set.call({}, 3)");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call(Object.freeze({}), {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call(Object.preventExtensions({}), {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); var o = Object.preventExtensions({}); return d.set.call(o, Object.prototype)");

// for-in com protótipo, shadowing e enumerabilidade.
sloppy("var p = {a: 1, b: 2}; var o = Object.create(p); o.c = 3; o.a = 4; var r = []; for (var k in o) r.push(k); return r");
sloppy("var p = {a: 1}; var o = Object.create(p); Object.defineProperty(o, 'a', {value: 2, enumerable: false}); var r = []; for (var k in o) r.push(k); return r");
sloppy("var p = {a: 1}; var o = Object.create(p); Object.defineProperty(o, 'a', {value: 2, enumerable: false}); return [S(Object.keys(o)), JSON.stringify(o), S({...o}), S(Object.assign({}, o))]");
sloppy("var p = {}; Object.defineProperty(p, 'a', {value: 1, enumerable: false}); var o = Object.create(p); o.a2 = 1; var r = []; for (var k in o) r.push(k); return r");
sloppy("var p = {}; Object.defineProperty(p, 'a', {value: 1, enumerable: false}); var o = Object.create(p); o.a = 5; var r = []; for (var k in o) r.push(k); return [r, D(o, 'a'), D(p, 'a')]");
sloppy("var p = {a: 1, 1: 1}; var o = Object.create(p); o.b = 1; o[0] = 1; var r = []; for (var k in o) r.push(k); return r");
sloppy("var g = {z: 1, 5: 1}; var p = Object.create(g); p.y = 1; p[2] = 1; var o = Object.create(p); o.x = 1; o[1] = 1; var r = []; for (var k in o) r.push(k); return r");
sloppy("var p = {a: 1}; var o = Object.create(p); var r = []; for (var k in o) { r.push(k); delete p.a } return r");
sloppy("var o = {a: 1, b: 2, c: 3}; var r = []; for (var k in o) { r.push(k); delete o.b } return r");
sloppy("var o = {a: 1, b: 2}; var r = []; for (var k in o) { r.push(k); o.c = 3 } return r");
sloppy("var o = {a: 1}; o[Symbol('s')] = 1; var r = []; for (var k in o) r.push(k); return r");
sloppy("var r = []; for (var k in 'ab') r.push(k); return r");
sloppy("var r = []; for (var k in [1, , 3]) r.push(k); return r");
sloppy("var a = [1]; a.x = 1; a[5] = 1; var r = []; for (var k in a) r.push(k); return r");
sloppy("var r = []; for (var k in null) r.push(k); for (var k in undefined) r.push(k); return r");
sloppy("var r = []; for (var k in 5) r.push(k); return r");
sloppy("var r = []; for (var k in function(){}) r.push(k); return r");
sloppy("var f = function(){}; f.a = 1; var r = []; for (var k in f) r.push(k); return r");
sloppy("var r = []; for (var k in Object.create({1: 1}, {1: {value: 2, enumerable: false}})) r.push(k); return r");
sloppy("var r = []; for (var k in new Proxy({a: 1, b: 2}, {})) r.push(k); return r");
sloppy("var o = Object.create({x: 1}); o.y = 2; return [S(Object.keys(o)), S(Object.entries(o)), JSON.stringify(o), S({...o}), S(Object.assign({}, o))]");
sloppy("var o = {get a() {return 1}, b: 2}; Object.defineProperty(o, 'c', {get() {return 3}, enumerable: true}); return [S(Object.assign({}, o)), S({...o}), JSON.stringify(o), D(Object.assign({}, o), 'a')]");
sloppy("var src = {}; Object.defineProperty(src, 'a', {get() {return 1}, enumerable: true}); var t = Object.assign({}, src); return D(t, 'a')");
sloppy("var t = {set a(v) { this.got = v }}; Object.assign(t, {a: 5}); return S(t)");
sloppy("var t = Object.freeze({a: 1}); try { Object.assign(t, {a: 2}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.freeze({a: 1}); try { Object.assign(t, {b: 2}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.defineProperty({}, 'a', {value: 1}); try { Object.assign(t, {a: 2}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = {}; Object.assign(t, 'ab', null, undefined, 5, true, [7]); return S(t)");
sloppy("var t = {}; try { Object.assign(null, {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.assign(1, {a: 1}); return [typeof t, t.a]");
sloppy("var s = Symbol('s'); var t = Object.assign({}, {[s]: 1, a: 2}); return S(Reflect.ownKeys(t))");
sloppy("var s = Symbol('s'); var src = {[s]: 1}; Object.defineProperty(src, Symbol('h'), {value: 1, enumerable: false}); return S(Reflect.ownKeys(Object.assign({}, src)))");
sloppy("var t = Object.assign({}, {__proto__: {x: 1}, y: 2}); return S(t)");
sloppy("var t = Object.assign({}, JSON.parse('{\"__proto__\": {\"x\": 1}}')); return [Object.getPrototypeOf(t) === Object.prototype, t.x]");
sloppy("var t = {...JSON.parse('{\"__proto__\": {\"x\": 1}}')}; return [Object.getPrototypeOf(t) === Object.prototype, t.x, D(t, '__proto__')]");
sloppy("var t = JSON.parse('{\"__proto__\": 5, \"a\": 1}'); return [S(Object.keys(t)), D(t, '__proto__')]");
sloppy("var t = {...'ab', ...[9], ...null, ...5}; return S(t)");
sloppy("var t = {a: 1, ...{a: 2, b: 3}, b: 4}; return S(t)");
sloppy("var t = {...{get a() {return 1}}}; return D(t, 'a')");
sloppy("var t = {set a(v) {}, ...{a: 1}}; return D(t, 'a')");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); Object.assign({}, src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); ({...src}); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); Object.entries(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); Object.keys(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); Object.getOwnPropertyDescriptors(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }}); JSON.stringify(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, get(t, k) { order.push('get:' + k); return t[k] }, getPrototypeOf(t) { order.push('gpo'); return null }}); for (var k in src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }}); Object.freeze(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { order.push('ownKeys'); return Reflect.ownKeys(t) }, getOwnPropertyDescriptor(t, k) { order.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k) }, isExtensible(t) { order.push('isExt'); return Reflect.isExtensible(t) }}); Object.isFrozen(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {defineProperty(t, k, d) { order.push('def:' + k + ':' + S(d)); return Reflect.defineProperty(t, k, d) }, preventExtensions(t) { order.push('pe'); return Reflect.preventExtensions(t) }}); Object.freeze(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, b: 2}, {defineProperty(t, k, d) { order.push('def:' + k + ':' + S(d)); return Reflect.defineProperty(t, k, d) }, preventExtensions(t) { order.push('pe'); return Reflect.preventExtensions(t) }}); Object.seal(src); return order");
sloppy("var order = []; var src = new Proxy({a: 1, get b() {return 1}}, {defineProperty(t, k, d) { order.push('def:' + k + ':' + S(d)); return Reflect.defineProperty(t, k, d) }}); Object.freeze(src); return order");

// ---- 6. Herança de getters e setters, put em frozen/sealed/preventExtensions.
for (const mode of [sloppy, strict]) {
  mode("var p = {get x() {return 1}}; var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o))]");
  mode("var p = {get x() {return 1}, set x(v) {this._x = v}}; var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o)), o._x]");
  mode("var p = {set x(v) {this._x = v}}; var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o)), o._x]");
  mode("var p = {}; Object.defineProperty(p, 'x', {value: 1, writable: false}); var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o))]");
  mode("var p = {}; Object.defineProperty(p, 'x', {value: 1, writable: true}); var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o)), p.x]");
  mode("var p = Object.freeze({x: 1}); var o = Object.create(p); o.x = 5; return [o.x, S(Object.keys(o))]");
  mode("var p = Object.freeze({x: 1}); var o = Object.create(p); o.y = 5; return [o.y, S(Object.keys(o))]");
  mode("var p = Object.preventExtensions({}); var o = Object.create(p); o.y = 5; return [o.y, S(Object.keys(o))]");
  mode("var o = Object.freeze({x: 1}); o.x = 2; return o.x");
  mode("var o = Object.freeze({x: 1}); o.y = 2; return o.y");
  mode("var o = Object.freeze({x: 1}); delete o.x; return o.x");
  mode("var o = Object.freeze({x: 1}); delete o.nope; return 'ok'");
  mode("var o = Object.seal({x: 1}); o.x = 2; return o.x");
  mode("var o = Object.seal({x: 1}); o.y = 2; return o.y");
  mode("var o = Object.seal({x: 1}); delete o.x; return o.x");
  mode("var o = Object.seal({x: 1}); delete o.nope; return 'ok'");
  mode("var o = Object.preventExtensions({x: 1}); o.x = 2; return o.x");
  mode("var o = Object.preventExtensions({x: 1}); o.y = 2; return o.y");
  mode("var o = Object.preventExtensions({x: 1}); delete o.x; return o.x");
  mode("var o = Object.preventExtensions({}); o[0] = 2; return o[0]");
  mode("var o = Object.preventExtensions({}); o[Symbol.iterator] = 2; return 'ok'");
  mode("var o = Object.preventExtensions({}); o['a b'] = 2; return 'ok'");
  mode("var o = Object.freeze({get x() {return 1}}); o.x = 2; return o.x");
  mode("var o = Object.freeze({set x(v) {this.z = 1}}); o.x = 2; return S(o)");
  mode("var o = {}; Object.defineProperty(o, 'x', {get() {return 1}}); o.x = 2; return o.x");
  mode("var o = {}; Object.defineProperty(o, 'x', {value: 1}); o.x = 2; return o.x");
  mode("var o = {}; Object.defineProperty(o, 'x', {value: 1}); delete o.x; return o.x");
  mode("var o = {}; Object.defineProperty(o, 'x', {value: 1, configurable: true}); delete o.x; return o.x");
  mode("var o = {}; Object.defineProperty(o, 5, {value: 1}); delete o[5]; return o[5]");
  mode("var o = {}; Object.defineProperty(o, 5, {value: 1}); o[5] = 2; return o[5]");
  mode("var o = {}; Object.defineProperty(o, Symbol.iterator, {value: 1}); o[Symbol.iterator] = 2; return 'ok'");
  mode("var o = {}; Object.defineProperty(o, Symbol.iterator, {value: 1}); delete o[Symbol.iterator]; return 'ok'");
  mode("var o = 'abc'; o.x = 1; return o.x");
  mode("var o = 'abc'; o[0] = 'z'; return o[0]");
  mode("var o = 'abc'; o.length = 1; return o.length");
  mode("var o = 'abc'; o[5] = 'z'; return o[5]");
  mode("var o = 5; o.x = 1; return o.x");
  mode("var o = true; o.x = 1; return o.x");
  mode("var o = Symbol(); o.x = 1; return o.x");
  mode("var o = 1n; o.x = 1; return o.x");
  mode("var o = Object('abc'); o[0] = 'z'; return o[0]");
  mode("var o = Object('abc'); o.length = 1; return o.length");
  mode("var o = Object('abc'); delete o[0]; return o[0]");
  mode("var o = Object('abc'); delete o.length; return o.length");
  mode("var o = Object('abc'); o[3] = 'z'; return o[3]");
  mode("var o = 'abc'; return delete o[0]");
  mode("var o = 'abc'; return delete o.x");
  mode("var o = 5; return delete o.x");
  mode("var o = null; o.x = 1");
  mode("var o = undefined; o.x = 1");
  mode("var o = null; o[0] = 1");
  mode("var o = null; return o.x");
  mode("var o = undefined; return o[Symbol.iterator]");
  mode("var o = null; delete o.x");
  mode("var f = function(){}; f.length = 5; return f.length");
  mode("var f = function(){}; f.name = 'q'; return f.name");
  mode("var f = function(){}; f.prototype = 5; return f.prototype");
  mode("var f = function(){}; delete f.prototype; return 1");
  mode("var f = function(){}; delete f.length; return f.length");
  mode("var f = function(){}; delete f.name; return f.name");
  mode("var f = () => 1; delete f.name; return [f.name, f.length]");
  mode("var f = class {}; f.prototype = 5; return typeof f.prototype");
  mode("var f = class {}; delete f.prototype; return 1");
  mode("var f = class {}; f.name = 'x'; return f.name");
  mode("var a = []; a.length = 1.5");
  mode("var o = Object.freeze([1]); o.length = 0");
  mode("var o = Object.freeze([1]); o.push");
  mode("var o = Object.freeze([1]); o[0] = 2; return o[0]");
  mode("var o = Math; o.PI = 3; return Math.PI");
  mode("NaN = 5; return NaN");
  mode("undefined = 5; return undefined");
  mode("Infinity = 5; return Infinity");
  mode("var u = globalThis; delete u.NaN; return typeof NaN");
  mode("Object.prototype = 5; return typeof Object.prototype");
  mode("var o = {}; Object.defineProperty(o, 'x', {set: undefined, configurable: true}); o.x = 1; return S(o)");
  mode("var o = {}; Object.defineProperty(o, 'x', {get: undefined, set: undefined}); o.x = 1; return o.x");
  mode("var o = {get x() {return 1}}; o.x = 2; return o.x");
  mode("var o = {get x() {return 1}}; var q = Object.create(o); q.x = 2; return [q.x, S(Object.keys(q))]");
  mode("var o = {get x() {return 1}}; var q = Object.create(o); Object.defineProperty(q, 'x', {value: 3, writable: true}); q.x = 4; return q.x");
  mode("var o = {x: 1}; var r = Reflect.set(Object.freeze(o), 'x', 2); return r");
  mode("var o = Object.freeze({x: 1}); var r = Reflect.set(o, 'y', 2); return r");
  mode("var o = Object.freeze({x: 1}); return Reflect.deleteProperty(o, 'x')");
  mode("var o = Object.freeze({x: 1}); return Reflect.deleteProperty(o, 'y')");
  mode("var o = {a: 1}; var r = {}; return [Reflect.set(o, 'a', 5, r), S(o), S(r), D(r, 'a')]");
  mode("var o = {get a() {return 1}}; var r = {}; return [Reflect.set(o, 'a', 5, r), S(r)]");
  mode("var o = {set a(v) {this.got = v}}; var r = {}; return [Reflect.set(o, 'a', 5, r), S(r), S(o)]");
  mode("var o = {a: 1}; var r = Object.freeze({}); return [Reflect.set(o, 'a', 5, r), S(r)]");
  mode("var o = {a: 1}; return [Reflect.set(o, 'a', 5, 5), S(o)]");
  mode("var o = {a: 1}; var r = {}; Object.defineProperty(r, 'a', {get() {return 1}, configurable: true}); return Reflect.set(o, 'a', 5, r)");
  mode("var o = {a: 1}; var r = {}; Object.defineProperty(r, 'a', {value: 1, writable: false, configurable: true}); return Reflect.set(o, 'a', 5, r)");
  mode("var o = {a: 1}; var r = {}; Object.defineProperty(r, 'a', {value: 1, writable: true, enumerable: false, configurable: true}); Reflect.set(o, 'a', 5, r); return D(r, 'a')");
  mode("var o = [1, 2]; var r = {}; return [Reflect.set(o, 'length', 0, r), S(r), S(o)]");
}
sloppy("'use strict'; var o = Object.freeze({x: 1}); try { o.x = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { o.x = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { o.y = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { o['a b'] = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { o[3] = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { o[Symbol('s')] = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { delete o.x } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({x: 1}); try { delete o[Symbol.iterator]; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze([1]); try { delete o[0] } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze([1]); try { delete o.length } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.seal({x: 1}); try { o.y = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.seal({x: 1}); try { delete o.x } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.preventExtensions({x: 1}); try { o.y = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {get x() {return 1}}; try { o.x = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {get x() {return 1}}; var q = Object.create(o); try { q.x = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {}; Object.defineProperty(o, 'x', {value: 1}); try { o.x = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {}; Object.defineProperty(o, 'x', {value: 1}); try { delete o.x } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {}; Object.defineProperty(o, 5, {value: 1}); try { o[5] = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {}; Object.defineProperty(o, Symbol('s'), {value: 1}); var s = Object.getOwnPropertySymbols(o)[0]; try { o[s] = 2 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = {}; Object.defineProperty(o, Symbol('s'), {value: 1}); var s = Object.getOwnPropertySymbols(o)[0]; try { delete o[s] } catch (e) { return e.name + ': ' + e.message }");
strict("var o = 'abc'; try { o.x = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = 'abc'; try { o[0] = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = 'abc'; try { o.length = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = 'abc'; try { delete o[0] } catch (e) { return e.name + ': ' + e.message }");
strict("var o = 5; try { o.x = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = null; try { o.x = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = undefined; try { o[0] = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var o = null; try { delete o.x } catch (e) { return e.name + ': ' + e.message }");
strict("var f = function(){}; try { f.length = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var f = function(){}; try { f.name = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("var f = function(){}; try { delete f.length; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
strict("var f = function(){}; try { delete f.prototype } catch (e) { return e.name + ': ' + e.message }");
strict("try { NaN = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("try { undefined = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("try { Math.PI = 1 } catch (e) { return e.name + ': ' + e.message }");
strict("try { delete Math.PI } catch (e) { return e.name + ': ' + e.message }");
strict("try { Object.prototype.__proto__ = {} } catch (e) { return e.name + ': ' + e.message }");
strict("try { Object.setPrototypeOf(Object.prototype, {}) } catch (e) { return e.name + ': ' + e.message }");
strict("try { Object.prototype.__proto__ = null; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
strict("try { arguments.callee } catch (e) { return e.name + ': ' + e.message }");
strict("try { (function(){ return arguments.callee })() } catch (e) { return e.name + ': ' + e.message }");
strict("try { (function(){ arguments.callee = 1 })() } catch (e) { return e.name + ': ' + e.message }");
strict("try { (function f(){}).caller } catch (e) { return e.name + ': ' + e.message }");
strict("try { (function f(){}).arguments } catch (e) { return e.name + ': ' + e.message }");
strict("try { return (function f(){ return f.caller })() } catch (e) { return e.name + ': ' + e.message }");
strict("try { return (function f(){ 'use strict'; return f.arguments })() } catch (e) { return e.name + ': ' + e.message }");
strict("try { var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); return [S(d), d.get === d.set] } catch (e) { return e.name + ': ' + e.message }");
strict("var d = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); return [D(Function.prototype, 'arguments'), d.get === d.set]");
strict("try { Function.prototype.caller } catch (e) { return e.name + ': ' + e.message }");
strict("try { ({}).__proto__ = 1; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.preventExtensions({}); try { o.__proto__ = {} } catch (e) { return e.name + ': ' + e.message }");
strict("var o = Object.freeze({}); try { o.__proto__ = Object.prototype; return 'ok' } catch (e) { return e.name + ': ' + e.message }");

// ---- 7. Object.setPrototypeOf / getPrototypeOf / create e ciclos.
sloppy("var a = {}, b = Object.create(a); try { Object.setPrototypeOf(a, b) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}; try { Object.setPrototypeOf(a, a) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}; try { a.__proto__ = a } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}, b = Object.create(a); try { a.__proto__ = b } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}, b = Object.create(a), c = Object.create(b); try { a.__proto__ = c } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}, b = Object.create(a); return Reflect.setPrototypeOf(a, b)");
sloppy("var a = {}; return Reflect.setPrototypeOf(a, a)");
sloppy("var a = {}; return [Reflect.setPrototypeOf(a, null), Object.getPrototypeOf(a)]");
sloppy("var a = Object.preventExtensions({}); return [Reflect.setPrototypeOf(a, null), Reflect.setPrototypeOf(a, Object.prototype)]");
sloppy("var a = Object.preventExtensions({}); try { Object.setPrototypeOf(a, null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = Object.preventExtensions({}); return S(Object.setPrototypeOf(a, Object.prototype) === a)");
sloppy("var a = Object.freeze({}); try { Object.setPrototypeOf(a, {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.setPrototypeOf(Object.prototype, {}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("return Reflect.setPrototypeOf(Object.prototype, {})");
sloppy("return Reflect.setPrototypeOf(Object.prototype, null)");
sloppy("try { Object.setPrototypeOf(Object.prototype, Object.create(null)) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}; return Object.setPrototypeOf(a, null) === a");
sloppy("var a = {}; try { Object.setPrototypeOf(a) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = {}; try { Object.setPrototypeOf(a, undefined) } catch (e) { return e.name + ': ' + e.message }");
for (const p of ["1", "'s'", "true", "Symbol()", "function(){}", "[]", "undefined", "NaN", "1n"]) {
  sloppy(`try { return Object.getPrototypeOf(Object.setPrototypeOf({}, ${p})) === ${p} } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return Object.getPrototypeOf(Object.create(${p})) === ${p} } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return Reflect.setPrototypeOf({}, ${p}) } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`try { return Reflect.getPrototypeOf(${p}) === Object.getPrototypeOf(${p}) } catch (e) { return e.name + ': ' + e.message }`);
  sloppy(`var o = {}; o.__proto__ = ${p}; return Object.getPrototypeOf(o) === Object.prototype`);
}
sloppy("return [Object.getPrototypeOf(1) === Number.prototype, Object.getPrototypeOf('s') === String.prototype, Object.getPrototypeOf(true) === Boolean.prototype, Object.getPrototypeOf(Symbol()) === Symbol.prototype, Object.getPrototypeOf(1n) === BigInt.prototype]");
sloppy("return [Object.getPrototypeOf(function(){}) === Function.prototype, Object.getPrototypeOf(async function(){}) === Function.prototype, Object.getPrototypeOf(Object.getPrototypeOf(function*(){})) === Function.prototype]");
sloppy("return [Object.getPrototypeOf(Object.prototype), Object.getPrototypeOf(Function.prototype) === Object.prototype, Object.getPrototypeOf(Object) === Function.prototype]");
sloppy("return [Object.getPrototypeOf([]) === Array.prototype, Object.getPrototypeOf(new Map) === Map.prototype, Object.getPrototypeOf(class A extends Array {}) === Array]");
sloppy("class A {} class B extends A {} return [Object.getPrototypeOf(B) === A, Object.getPrototypeOf(B.prototype) === A.prototype, Object.getPrototypeOf(A) === Function.prototype]");
sloppy("class A extends null {} return [Object.getPrototypeOf(A) === Function.prototype, Object.getPrototypeOf(A.prototype)]");
sloppy("try { class A extends 5 {} } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { class A extends undefined {} } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { class A extends (()=>1) {} } catch (e) { return e.name + ': ' + e.message }");
sloppy("function F(){} F.prototype = 5; try { class A extends F {} } catch (e) { return e.name + ': ' + e.message }");
sloppy("function F(){} F.prototype = null; class A extends F {} return Object.getPrototypeOf(A.prototype)");
sloppy("function F(){} F.prototype = 5; var o = new F; return Object.getPrototypeOf(o) === Object.prototype");
sloppy("var o = Object.create({a: 1}, {b: {value: 2, enumerable: true}, c: {get() {return 3}}}); return [S(Object.keys(o)), o.a, o.b, o.c, D(o, 'c')]");
sloppy("var o = Object.create(null, {a: {value: 1}}); return [S(Object.getOwnPropertyNames(o)), D(o, 'a')]");
sloppy("try { Object.create({}, null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.create({}, {a: 1}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.create({}, 1); return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.create() } catch (e) { return e.name + ': ' + e.message }");
sloppy("return S(Object.create({}, undefined))");
sloppy("return S(Object.getOwnPropertyNames(Object.create(null)))");
sloppy("var o = Object.create(Object.create(null)); return [typeof o.toString, 'x' in o]");
sloppy("var o = new Proxy({}, {getPrototypeOf() { return 5 }}); try { Object.getPrototypeOf(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {getPrototypeOf() { return null }}); return Object.getPrototypeOf(o)");
sloppy("var o = new Proxy(Object.preventExtensions({}), {getPrototypeOf() { return null }}); try { Object.getPrototypeOf(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {setPrototypeOf() { return false }}); try { Object.setPrototypeOf(o, null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {setPrototypeOf() { return false }}); return Reflect.setPrototypeOf(o, null)");
sloppy("var o = new Proxy({}, {defineProperty() { return false }}); try { Object.defineProperty(o, 'a', {value: 1}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {defineProperty() { return false }}); return Reflect.defineProperty(o, 'a', {value: 1})");
sloppy("var o = new Proxy({}, {defineProperty() { return true }}); try { Object.defineProperty(o, 'a', {value: 1}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {deleteProperty() { return false }}); return [Reflect.deleteProperty(o, 'a'), delete o.a]");
strict("var o = new Proxy({}, {deleteProperty() { return false }}); try { delete o.a } catch (e) { return e.name + ': ' + e.message }");
strict("var o = new Proxy({}, {set() { return false }}); try { o.a = 1 } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {set() { return false }}); o.a = 1; return 'ok'");
sloppy("var o = new Proxy({}, {preventExtensions() { return false }}); try { Object.preventExtensions(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {preventExtensions() { return true }}); try { Object.preventExtensions(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {ownKeys() { return [1] }}); try { Object.keys(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {ownKeys() { return ['a', 'a'] }}); try { Object.keys(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {ownKeys() { return 5 }}); try { Object.keys(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy(Object.freeze({a: 1}), {ownKeys() { return [] }}); try { Object.keys(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy(Object.preventExtensions({a: 1}), {ownKeys() { return ['a', 'b'] }}); try { Object.keys(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {getOwnPropertyDescriptor() { return 5 }}); try { Object.getOwnPropertyDescriptor(o, 'a') } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {getOwnPropertyDescriptor() { return {value: 1, configurable: false} }}); try { Object.getOwnPropertyDescriptor(o, 'a') } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = new Proxy({}, {has() { return false }}); var t = Object.freeze({a: 1}); var p = new Proxy(t, {has() { return false }}); try { 'a' in p } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.freeze({a: 1}); var p = new Proxy(t, {get() { return 2 }}); try { p.a } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.freeze({a: 1}); var p = new Proxy(t, {set() { return true }}); try { p.a = 2 } catch (e) { return e.name + ': ' + e.message }");
sloppy("var t = Object.freeze({a: 1}); var p = new Proxy(t, {deleteProperty() { return true }}); try { delete p.a } catch (e) { return e.name + ': ' + e.message }");

// ---- 8. Object.is, fromEntries, entries, values.
const isVals = ["0", "-0", "NaN", "1", "'1'", "null", "undefined", "true", "{}", "1n", "Infinity", "-Infinity", "''", "[]"];
for (const a of isVals) for (const b of isVals) {
  sloppy(`return [Object.is(${a}, ${b}), ${a} === ${b}, ${a} == ${b}]`);
}
sloppy("return [Object.is(), Object.is(undefined), Object.is(undefined, undefined), Object.is(NaN, 0/0)]");
sloppy("var o = {}; return [Object.is(o, o), Object.is(o, {}), Object.is(Symbol.iterator, Symbol.iterator), Object.is(Symbol(), Symbol())]");
sloppy("return S(Object.fromEntries([['a', 1], ['b', 2], ['a', 3]]))");
sloppy("return S(Object.fromEntries(new Map([[1, 'a'], ['x', 'b'], [Symbol.iterator, 'c']])))");
sloppy("return S(Object.fromEntries([[{toString(){return 'k'}}, 1]]))");
sloppy("return S(Object.fromEntries([['__proto__', 1]])) + (Object.getPrototypeOf(Object.fromEntries([['__proto__', {a: 1}]])) === Object.prototype)");
sloppy("return D(Object.fromEntries([['a', 1]]), 'a')");
sloppy("try { Object.fromEntries([1]) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries([null]) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries(['ab']) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { return S(Object.fromEntries(['ab', 'cd'])) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries(5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries() } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Object.fromEntries(null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("return S(Object.fromEntries([[], ['a'], ['b', 1, 2]]))");
sloppy("return S(Object.fromEntries({[Symbol.iterator]: function*() { yield ['a', 1]; yield ['b', 2] }}))");
sloppy("return S(Object.fromEntries([[Symbol.iterator, 1]]))");
sloppy("return S(Object.fromEntries([[1, 'a'], [0, 'b'], ['x', 'c']]))");
sloppy("return S(Object.fromEntries(Object.entries({a: 1, b: [2], c: {d: 3}})))");
sloppy("return S(Object.entries({b: 1, a: 2, 1: 3, 0: 4}))");
sloppy("return S(Object.entries('ab'))");
sloppy("return S(Object.entries([5, , 6]))");
sloppy("return S(Object.entries(5))");
sloppy("return S(Object.entries(true))");
sloppy("return S(Object.entries(function(){}))");
sloppy("return S(Object.entries(Symbol()))");
sloppy("var o = {a: 1}; Object.defineProperty(o, 'h', {value: 1, enumerable: false}); o[Symbol('s')] = 1; return [S(Object.entries(o)), S(Object.values(o)), S(Object.keys(o))]");
sloppy("var o = {get a() {delete this.b; return 1}, b: 2}; return [S(Object.entries(o)), S(Object.values(o))]");
sloppy("var o = {get a() {Object.defineProperty(this, 'b', {enumerable: false}); return 1}, b: 2}; return [S(Object.entries(o)), S(Object.values(o))]");
sloppy("var o = {a: 1, get b() {this.c = 5; return 2}}; return S(Object.values(o))");
sloppy("return S(Object.values('ab')) + S(Object.values([1, 2])) + S(Object.values(5))");
sloppy("var o = Object.create({inh: 1}); o.own = 2; return [S(Object.values(o)), S(Object.entries(o))]");
sloppy("return S(Object.getOwnPropertyNames('ab')) + S(Object.getOwnPropertyNames(5)) + S(Object.getOwnPropertyNames(true))");
sloppy("return S(Object.getOwnPropertyDescriptors('ab'))");
sloppy("return S(Object.getOwnPropertyDescriptors(5))");
sloppy("return S(Object.getOwnPropertyDescriptors({a: 1, get b() {return 1}, set b(v) {}, c: undefined}))");
sloppy("var s = Symbol('s'); return S(Object.getOwnPropertyDescriptors({[s]: 1, a: 2, 1: 3}))");
sloppy("return S(Object.getOwnPropertyDescriptors(Object.create({a: 1})))");
sloppy("var p = new Proxy({a: 1}, {getOwnPropertyDescriptor() { return undefined }}); return S(Object.getOwnPropertyDescriptors(p))");
sloppy("return S(Object.getOwnPropertyDescriptors([1, 2]))");
sloppy("return S(Object.getOwnPropertyDescriptors(function f(a, b) {}))");
sloppy("return S(Object.getOwnPropertyDescriptors(class A { static m() {} static get g() { return 1 } static x = 1 }))");
sloppy("return S(Object.getOwnPropertyDescriptors(class A { m() {} get g() { return 1 } set g(v) {} static x = 1; y = 2 }.prototype))");
sloppy("return S(Object.getOwnPropertyDescriptors(new (class A { x = 1; #p = 2; y = 3 })))");
sloppy("var d = Object.getOwnPropertyDescriptors({a: 1}); d.a.value = 2; return d.a");
sloppy("var o = Object.defineProperties({}, Object.getOwnPropertyDescriptors({get a() {return 1}, b: 2})); return S(Object.getOwnPropertyDescriptors(o))");
sloppy("return S(Object.getOwnPropertyDescriptors(Object.create(null, {a: {value: 1, enumerable: true}})))");
sloppy("return S(Object.getOwnPropertyDescriptor('abc', 1)) + S(Object.getOwnPropertyDescriptor('abc', 'length')) + S(Object.getOwnPropertyDescriptor('abc', 3))");
sloppy("return S(Object.getOwnPropertyDescriptor(Object('abc'), 1)) + S(Object.getOwnPropertyDescriptor(Object('abc'), 'length'))");
sloppy("return S(Object.getOwnPropertyDescriptor({a: 1}, {toString(){return 'a'}}))");
sloppy("var n = 0; Object.getOwnPropertyDescriptor({}, {toString(){ n++; return 'a'}}); return n");
sloppy("return S(Object.getOwnPropertyDescriptor({}, undefined)) + S(Object.getOwnPropertyDescriptor({undefined: 1}, undefined))");
sloppy("return S(Object.getOwnPropertyDescriptor({1: 1}, 1)) + S(Object.getOwnPropertyDescriptor({1: 1}, '1')) + S(Object.getOwnPropertyDescriptor({1: 1}, 1.0)) + S(Object.getOwnPropertyDescriptor({'1.5': 1}, 1.5))");
sloppy("var o = {}; o[Symbol.iterator] = 1; return S(Object.getOwnPropertyDescriptor(o, Symbol.iterator))");

// ---- 9. Propriedades numéricas / chaves canônicas.
const numKeys = ["'01'", "'1.0'", "'-1'", "'-0'", "'+1'", "' 1'", "'1 '", "'1e1'", "'0x1'", "'4294967295'", "'4294967294'", "'4294967296'", "'9007199254740991'", "'9007199254740992'", "'1.5'", "'Infinity'", "'NaN'", "''", "'00'", "'0'", "1.0", "-0", "0.1+0.2", "1e21", "2**32", "2**32-1", "-1", "2**53", "-(2**31)", "1e-7"];
for (const k of numKeys) {
  sloppy(`var o = {}; o[${k}] = 1; return [S(Object.keys(o)), o[${k}], o[String(${k})]]`);
  sloppy(`var o = {[${k}]: 1}; return [S(Reflect.ownKeys(o)), D(o, ${k})]`);
  sloppy(`var a = []; a[${k}] = 1; return [a.length, S(Object.keys(a))]`);
  sloppy(`var a = [0, 1, 2]; return [${k} in a, a[${k}], Object.hasOwn(a, ${k}), a.hasOwnProperty(${k}), a.propertyIsEnumerable(${k})]`);
  sloppy(`var o = {}; Object.defineProperty(o, ${k}, {value: 1, enumerable: true}); return [S(Object.keys(o)), D(o, ${k})]`);
  sloppy(`var o = {1: 'a', 'x': 'b'}; o[${k}] = 'c'; return [S(Object.keys(o)), S(Object.values(o)), delete o[${k}], S(Object.keys(o))]`);
  sloppy(`var s = 'abc'; return [s[${k}], ${k} in Object(s), Object.hasOwn(s, ${k})]`);
  sloppy(`var a = new Uint8Array(3); a[${k}] = 7; return [a[${k}], S(Object.keys(a)), ${k} in a]`);
  sloppy(`var a = [1, 2, 3]; a.length = ${k}; return a.length`);
}
sloppy("var a = []; a[2**32-1] = 'x'; return [a.length, a[4294967295], S(Object.keys(a))]");
sloppy("var a = []; a[2**32-2] = 'x'; return [a.length, S(Object.keys(a))]");
sloppy("var a = []; a[2**32-2] = 'x'; a.push(1)");
sloppy("var a = []; a[2**32-2] = 'x'; try { a.push(1) } catch (e) { return e.name + ': ' + e.message + [a.length, S(Object.keys(a))] }");
sloppy("var a = []; a[2**32-2] = 'x'; try { a.push(1, 2) } catch (e) { return e.name + ': ' + e.message + [a.length, S(Object.keys(a))] }");
sloppy("var a = [1, 2, 3]; a.length = 2**32-1; return [a.length, a.indexOf(3)]");
sloppy("var a = [1, 2, 3]; a.length = 2**32; ");
sloppy("var a = new Array(2**32-1); return [a.length, a.slice(-2).length, a.concat().length]");
sloppy("var a = new Array(2**32-1); try { a.concat([1]) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = new Array(2**32-1); try { a.splice(0, 0, 1); return a.length } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = new Array(2**32-1); return [a.slice(2**32-3).length, a.slice(2**32-3, 2**32-1).length]");
sloppy("var a = new Array(2**32-1); a[2**32-2] = 1; return [a.reverse().length, a[0]]");
sloppy("var a = new Array(2**32-1); a[2**32-2] = 'z'; return [a.at(-1), a.findLast(function(x){return x === 'z'}), a.lastIndexOf('z')]");
sloppy("var a = []; a.length = 2**32-1; a.fill(0, 2**32-3); return S(Object.keys(a))");
sloppy("var a = []; a.length = 2**32-1; a.copyWithin(0, 2**32-3); return S(Object.keys(a))");
sloppy("var a = []; a.length = 2**32-1; return [a.toString().length, Array.isArray(a)]");
sloppy("var a = []; a.length = 2**32-1; return JSON.stringify(a).length");
sloppy("var a = []; a.length = 2**32-1; return [a.join('').length]");
sloppy("var a = []; a.length = 2**32-1; try { a.toReversed() } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 2**32-1; try { a.with(0, 1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 2**32-1; try { a.toSorted() } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 2**32-1; try { a.flat() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = []; a.length = 2**32-1; try { Array.from(a) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var a = [,]; a.length = 2**32-1; try { [...a] ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Array.from({length: 2**32}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Array.from({length: 2**32-1}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Array.of.call(Object, 1, 2); return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("return Array.of.call(Object, 1, 2).length");
sloppy("var o = {length: 2**32}; return [Array.prototype.slice.call(o, 0, 1).length]");
sloppy("var o = {length: 2**32+2}; return [Array.prototype.indexOf.call(o, 1)]");
sloppy("var o = {length: 2**53}; return [Array.prototype.indexOf.call(o, 1, 2**53-3)]");
sloppy("var o = {length: Infinity}; return [Array.prototype.lastIndexOf.call(o, 1, 3)]");
sloppy("var o = {length: -5}; Array.prototype.push.call(o, 1); return [o.length, o[0]]");
sloppy("var o = {length: 'abc'}; Array.prototype.push.call(o, 1); return [o.length, o[0]]");
sloppy("var o = {length: 2.9}; Array.prototype.push.call(o, 1); return [o.length, o[2]]");
sloppy("var o = {length: 3}; Array.prototype.pop.call(o); return [o.length]");
sloppy("var o = {}; Array.prototype.pop.call(o); return [o.length]");
sloppy("var o = {}; Array.prototype.shift.call(o); return [o.length]");
sloppy("var o = {}; Array.prototype.unshift.call(o); return [o.length]");
sloppy("var o = {}; Array.prototype.push.call(o); return [o.length]");
sloppy("var o = Object.freeze({length: 3}); try { Array.prototype.pop.call(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.pop.call(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.push.call(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.push.call(o, 1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.unshift.call(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.shift.call(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.reverse.call(o); return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze({length: 0}); try { Array.prototype.splice.call(o, 0, 0); return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([]); try { o.splice(0, 0); return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([]); try { o.push() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([]); try { o.pop() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([]); try { o.shift() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([]); try { o.sort() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.sort() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.reverse() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.copyWithin(0, 0) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1, 2]); try { o.copyWithin(0, 1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.fill(1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.fill(1, 1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.unshift() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.unshift(1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.splice(0, 1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.freeze([1]); try { o.splice(1, 0, 1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.seal([1]); try { o.pop() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.seal([1]); try { o.shift() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.seal([1]); try { o.push(1) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = Object.seal([1]); try { o.sort() ; return 'ok' } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.seal([2, 1]); try { o.sort() ; return S(o) } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.seal([1]); try { o.length = 0 ; return S(o) } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1]); try { o.pop() ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1, 2]); try { o.length = 1 ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1, 2]); try { o.length = 5 ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1, 2]); try { o.unshift(0) ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1, 2]); try { o.splice(0, 1) ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");
sloppy("var o = Object.preventExtensions([1, 2]); try { o.shift() ; return S(o) + o.length } catch (e) { return e.name + ': ' + e.message + S(o) }");

// ---- 10. arguments mapeado (sloppy) com defineProperty.
sloppy("function f(a) { arguments[0] = 2; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { a = 2; return [a, arguments[0]] } return f(1)");
strict("function f(a) { arguments[0] = 2; return [a, arguments[0]] } return f(1)");
strict("function f(a) { a = 2; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { 'use strict'; arguments[0] = 2; return [a, arguments[0]] } return f(1)");
sloppy("function f(a = 0) { arguments[0] = 2; return [a, arguments[0]] } return f(1)");
sloppy("function f(a, ...r) { arguments[0] = 2; return [a, arguments[0]] } return f(1)");
sloppy("function f({a}) { arguments[0] = 2; return [a, S(arguments[0])] } return f({a: 1})");
sloppy("function f(a) { arguments[0] = 2; return [a, arguments[0]] } return f()");
sloppy("function f(a, b) { arguments[1] = 2; return [b, arguments[1], arguments.length] } return f(1)");
sloppy("function f(a, b) { b = 5; return [arguments[1], arguments.length] } return f(1)");
sloppy("function f(a, a) { arguments[0] = 9; arguments[1] = 8; return [a, S(arguments)] } return f(1, 2)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {value: 5}); return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false}); arguments[0] = 9; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {value: 5, writable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {value: 5, writable: false}); return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {get() {return 7}}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {get() {return 7}}); arguments[0] = 3; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {enumerable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {configurable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {configurable: false, writable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {value: 5, configurable: false}); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { delete arguments[0]; a = 9; return [a, arguments[0], 0 in arguments] } return f(1)");
sloppy("function f(a) { delete arguments[0]; arguments[0] = 7; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { delete arguments[0]; Object.defineProperty(arguments, 0, {value: 3, writable: true, enumerable: true, configurable: true}); a = 8; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { Object.freeze(arguments); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.freeze(arguments); try { arguments[0] = 5 } catch (e) { return e.message } return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { Object.seal(arguments); a = 9; return [a, arguments[0], D(arguments, 0)] } return f(1)");
sloppy("function f(a) { Object.preventExtensions(arguments); a = 9; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { return [Object.isFrozen(arguments), Object.isSealed(arguments), Object.isExtensible(arguments)] } return f(1)");
sloppy("function f(a) { return S(Object.getOwnPropertyDescriptors(arguments)) } return f(1, 2)");
sloppy("function f(a) { return S(Reflect.ownKeys(arguments)) } return f(1, 2)");
sloppy("function f(a) { return S(Object.keys(arguments)) + S(Object.getOwnPropertyNames(arguments)) } return f(1, 2)");
strict("function f(a) { return S(Object.getOwnPropertyDescriptors(arguments)) } return f(1, 2)");
strict("function f(a) { return S(Reflect.ownKeys(arguments)) } return f(1, 2)");
strict("function f(a) { return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set } return f(1)");
strict("function f(a) { return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get } return f(1)");
sloppy("function f(a) { return [typeof arguments.callee, arguments.callee === f, D(arguments, 'callee'), D(arguments, 'length')] } return f(1)");
sloppy("function f(a) { arguments.length = 0; return [arguments.length, a, S(arguments)] } return f(1, 2)");
sloppy("function f(a) { arguments.length = 5; return [arguments.length, S(Array.from(arguments))] } return f(1, 2)");
sloppy("function f(a) { return [arguments[Symbol.iterator] === Array.prototype.values, D(arguments, Symbol.iterator)] } return f(1)");
sloppy("function f(a) { return Object.prototype.toString.call(arguments) } return f(1)");
sloppy("function f(a) { return [Object.getPrototypeOf(arguments) === Object.prototype, Array.isArray(arguments)] } return f(1)");
sloppy("function f(a) { arguments.x = 1; return [S(Object.keys(arguments)), JSON.stringify(arguments)] } return f(1, 2)");
sloppy("function f(a, b) { var r = []; for (var k in arguments) r.push(k); return r } return f(1, 2, 3)");
sloppy("function f(a) { return S({...arguments}) + S(Object.assign({}, arguments)) + S(Object.entries(arguments)) } return f(1, 2)");
sloppy("function f(a) { return [a, (function(){ return arguments[0] })(5)] } return f(1)");
sloppy("function f(a) { (function(){ arguments[0] = 9 })(); return a } return f(1)");
sloppy("function f(a) { var g = () => { arguments[0] = 9 }; g(); return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { var g = () => { a = 9 }; g(); return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { eval('arguments[0] = 9'); return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { eval('a = 9'); return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { var a; return [a, arguments[0]] } return f(1)");
sloppy("function f(a) { var arguments; return [a, typeof arguments, arguments.length] } return f(1)");
sloppy("function f(a) { var arguments = 5; return [a, arguments] } return f(1)");
sloppy("function f(a) { function arguments() {} return [typeof arguments] } return f(1)");
sloppy("function f(arguments) { return [arguments] } return f(1)");
sloppy("function f(a) { arguments = 5; return [a, arguments] } return f(1)");
sloppy("function f(a) { arguments[0] = 2; return [a, arguments[0]] } return f.apply(null, [1])");
sloppy("function f(a) { arguments[0] = 2; return [a, arguments[0]] } return f.call(null, 1, 2)");
sloppy("function f(a) { return Array.prototype.slice.call(arguments, 0) } return S(f(1, 2, 3))");
sloppy("function f(a) { a = 9; return Array.prototype.slice.call(arguments) } return S(f(1, 2, 3))");
sloppy("function f(a) { a = 9; return [].concat(arguments).length } return f(1)");
sloppy("function f(a) { a = 9; return Array.from(arguments) } return f(1)");
sloppy("function f(a) { a = 9; return [...arguments] } return f(1)");
sloppy("function f(a) { a = 9; return JSON.stringify(arguments) } return f(1)");
sloppy("function f(a) { a = 9; return Array.prototype.map.call(arguments, function(x){return x}) } return f(1)");
sloppy("function f(a) { Object.defineProperty(arguments, 0, {get() {return 4}, configurable: true}); Object.defineProperty(arguments, 0, {value: 6}); a = 1; return [arguments[0], D(arguments, 0)] } return f(0)");
sloppy("function f(a, b) { Object.defineProperties(arguments, {0: {value: 'x'}, 1: {value: 'y'}}); return [a, b, S(arguments)] } return f(1, 2)");
sloppy("function f(a, b) { Object.defineProperty(arguments, 'length', {value: 0}); return [arguments.length, S(Array.from(arguments)), a, b] } return f(1, 2)");
sloppy("function f(a, b) { Object.defineProperty(arguments, 'length', {value: 1, enumerable: true}); return [S(Object.keys(arguments)), D(arguments, 'length')] } return f(1, 2)");
sloppy("function f(a, b) { Object.defineProperty(arguments, 'callee', {value: 1}); return [arguments.callee, D(arguments, 'callee')] } return f(1, 2)");
sloppy("function f(a, b) { Object.defineProperty(arguments, 'callee', {get(){return 1}}); return [arguments.callee, D(arguments, 'callee')] } return f(1, 2)");
sloppy("function f(a, b) { delete arguments.callee; return [arguments.callee, 'callee' in arguments] } return f(1, 2)");
sloppy("function f(a, b) { delete arguments.length; return [arguments.length, 'length' in arguments] } return f(1, 2)");
strict("function f(a, b) { try { delete arguments.callee } catch (e) { return e.name + ': ' + e.message } } return f(1, 2)");
strict("function f(a, b) { try { Object.defineProperty(arguments, 'callee', {value: 1}) } catch (e) { return e.name + ': ' + e.message } } return f(1, 2)");
strict("function f(a, b) { try { arguments.callee = 1 } catch (e) { return e.name + ': ' + e.message } } return f(1, 2)");
strict("function f(a, b) { return [D(arguments, 'callee') === 'undefined', 'callee' in arguments, Object.hasOwn(arguments, 'callee')] } return f(1, 2)");
sloppy("function f(a, b) { 'use strict'; return [Object.hasOwn(arguments, 'callee')] } return f(1, 2)");
sloppy("function f(a) { return Reflect.defineProperty(arguments, '0', {value: 1, writable: false, enumerable: false, configurable: false}) && [a, D(arguments, 0)] } return f(0)");
sloppy("function f(a) { Reflect.defineProperty(arguments, '0', {value: 1, writable: false}); a = 4; return [a, arguments[0]] } return f(0)");
sloppy("function f(a) { Reflect.defineProperty(arguments, '0', {writable: false}); Reflect.defineProperty(arguments, '0', {value: 3}); return [a, arguments[0]] } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false}); return Reflect.defineProperty(arguments, '0', {value: 3, configurable: true}) } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false, configurable: false}); return Reflect.defineProperty(arguments, '0', {value: 3}) } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false, configurable: false}); return Reflect.defineProperty(arguments, '0', {value: 0}) } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false, configurable: false}); a = 5; return [arguments[0], a] } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false}); Object.defineProperty(arguments, '0', {writable: true}); a = 5; return [arguments[0], a] } return f(0)");
sloppy("function f(a) { Object.defineProperty(arguments, '0', {writable: false}); Object.defineProperty(arguments, '0', {writable: true}); arguments[0] = 7; return [arguments[0], a] } return f(0)");
sloppy("function f(a, b, c) { Object.defineProperty(arguments, 1, {value: 'v'}); return [a, b, c, S(arguments)] } return f(1, 2, 3)");
sloppy("function f(a, b, c) { return [arguments.length, f.length] } return f(1)");
sloppy("function f(a, b, c) { arguments[5] = 1; return [arguments.length, S(Object.keys(arguments))] } return f(1)");
sloppy("function f(a, b, c) { arguments[1] = 1; return [arguments.length, b, S(Object.keys(arguments))] } return f(1)");
sloppy("function f(a, b, c) { Object.defineProperty(arguments, 1, {value: 1, enumerable: true, writable: true, configurable: true}); return [arguments.length, b, S(Object.keys(arguments))] } return f(1)");
sloppy("function f(a) { 'use strict'; return Object.getOwnPropertyNames(arguments) } return S(f(1))");
sloppy("function f(a) { return Object.getOwnPropertyNames(arguments) } return S(f(1, 2))");
sloppy("function f() { return Object.getOwnPropertyNames(arguments) } return S(f())");
sloppy("function f() { return Reflect.ownKeys(arguments) } return S(f())");
sloppy("function f() { return S(Object.getOwnPropertyDescriptors(arguments)) } return f()");
strict("function f() { return S(Object.getOwnPropertyDescriptors(arguments)) } return f()");
sloppy("function f(a) { return Object.getOwnPropertyDescriptor(arguments, 'length').value } return f(1, 2, 3)");

// ---- 11. Funções, bound, classes: length/name/prototype.
const fnExprs = [
  "function f(a, b) {}", "function(){}", "(function(){})", "() => 1", "async function f(a) {}", "function* g(a, b, c) {}", "async function* ag() {}",
  "class A {}", "class A { constructor(a, b) {} }", "class A extends Object {}", "class A { static x = 1 }", "class A { static name = 'z' }",
  "class A { static name() {} }", "class A { static length = 5 }", "class A { static get name() { return 'g' } }", "class A { static prototype() {} }",
  "({m(a) {}}).m", "({get g() {return 1}}).__lookupGetter__('g')", "({set s(v) {}}).__lookupSetter__('s')", "({async m() {}}).m", "({*m() {}}).m",
  "({m: function(){}}).m", "({m: () => 1}).m", "({[Symbol.iterator]() {}})[Symbol.iterator]", "({[Symbol('desc')]() {}})[Object.getOwnPropertySymbols({[Symbol('desc')]() {}})[0]]",
  "(function f(a, b = 1, c) {})", "(function f(a, ...r) {})", "(function f({a}, [b]) {})", "(function f(a, b = 1) {})",
  "Function.prototype.bind.call(function f(a, b, c) {}, null, 1)", "(function f(a, b, c) {}).bind(null)", "(function f(a, b, c) {}).bind(null, 1, 2, 3, 4)",
  "(function f() {}).bind().bind()", "(() => 1).bind()", "(class A { constructor(a) {} }).bind(null)", "(async function af(){}).bind()", "(function* gf(){}).bind()",
  "Math.max", "Object", "Array.prototype.push", "Symbol", "Function.prototype", "Promise", "parseInt", "Object.prototype.hasOwnProperty",
  "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get", "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
  "new Function('a', 'b', 'return 1')", "new Function()", "Function('a, b', 'c', '')", "(0, eval)('(function(){})')", "new (class { constructor() {} })().constructor",
  "Proxy", "new Proxy(function(a){}, {})", "(function(){}).bind().bind", "Reflect.apply", "Array.from", "Array.prototype[Symbol.iterator]",
  "Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get", "Array[Symbol.species]", "Symbol.prototype[Symbol.toPrimitive]", "Date.prototype[Symbol.toPrimitive]",
  "(function(){ return arguments.callee.constructor })()",
];
for (const e of fnExprs) {
  sloppy(`var f = ${e}; return [D(f, 'length'), D(f, 'name'), D(f, 'prototype')]`);
  sloppy(`var f = ${e}; return [S(Reflect.ownKeys(f)), typeof f]`);
  sloppy(`var f = ${e}; return [Object.isFrozen(f), Object.isSealed(f), Object.isExtensible(f)]`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'name', {value: 'n'}).name } catch (x) { r = x.name + ': ' + x.message } return [r, D(f, 'name')]`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'length', {value: 9}).length } catch (x) { r = x.name + ': ' + x.message } return [r, D(f, 'length')]`);
  sloppy(`var f = ${e}; return [delete f.name, delete f.length, S(Reflect.ownKeys(f)), f.name, f.length]`);
  sloppy(`var f = ${e}; delete f.name; f.name = 'x'; return [D(f, 'name')]`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'prototype', {value: 1}).prototype } catch (x) { r = x.name + ': ' + x.message } return r`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'prototype', {writable: false}) ; r = D(f, 'prototype') } catch (x) { r = x.name + ': ' + x.message } return r`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'prototype', {enumerable: true}) ; r = D(f, 'prototype') } catch (x) { r = x.name + ': ' + x.message } return r`);
  sloppy(`var f = ${e}; var r; try { r = Object.defineProperty(f, 'prototype', {configurable: true}) ; r = D(f, 'prototype') } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { f.name = 'x'; r = f.name } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { f.length = 5; r = f.length } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { f.prototype = 5; r = typeof f.prototype } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { delete f.prototype; r = 'ok' } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { f.caller; r = 'ok' } catch (x) { r = x.name + ': ' + x.message } return r`);
  strict(`var f = ${e}; var r; try { f.arguments; r = 'ok' } catch (x) { r = x.name + ': ' + x.message } return r`);
  sloppy(`var f = ${e}; return [Object.hasOwn(f, 'caller'), Object.hasOwn(f, 'arguments')]`);
  sloppy(`var f = ${e}; return [S(Object.keys(f)), S(Object.entries(f)), JSON.stringify(f)]`);
  sloppy(`var f = ${e}; var r = []; for (var k in f) r.push(k); return r`);
  sloppy(`var f = ${e}; return S(Object.getOwnPropertyDescriptors(f))`);
}
sloppy("function f() {} return [S(Object.getOwnPropertyDescriptors(f.prototype)), f.prototype.constructor === f]");
sloppy("function* g() {} return [S(Object.getOwnPropertyDescriptors(g.prototype)), Object.getPrototypeOf(g.prototype) === Object.getPrototypeOf(function*(){}).prototype]");
sloppy("var g = async function*() {}; return [S(Reflect.ownKeys(g.prototype))]");
sloppy("return [S(Reflect.ownKeys(Function.prototype)), D(Function.prototype, 'length'), D(Function.prototype, 'name')]");
sloppy("return S(Reflect.ownKeys(function(){}.bind()))");
sloppy("return [D(function(){}.bind(), 'prototype'), 'prototype' in function(){}.bind()]");
sloppy("var b = function f(a, b) {}.bind(null, 1); return [b.name, b.length, D(b, 'name')]");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: 5}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: Infinity}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: -Infinity}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: 2.7}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: '2'}); return f.bind(null).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: NaN}); return f.bind(null).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: -1}); return f.bind(null).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {value: 2**32}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'length', {get() { return 3 }}); return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; delete f.length; return f.bind(null, 1).length");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'name', {value: 5}); return f.bind().name");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'name', {value: 'zed'}); return f.bind().name");
sloppy("var f = function(a, b) {}; delete f.name; return f.bind().name");
sloppy("var f = function(a, b) {}; Object.defineProperty(f, 'name', {get() { return 'getter' }}); return f.bind().name");
sloppy("var f = function(){}; Object.setPrototypeOf(f, null); return [typeof f.bind, Object.getPrototypeOf(Function.prototype.bind.call(f)) === null]");
sloppy("var f = function(){}; var p = {}; Object.setPrototypeOf(f, p); return Object.getPrototypeOf(f.bind()) === p");
sloppy("var b = function(){ return this }.bind(5); return [typeof b(), typeof b.call(7)]");
strict("var b = function(){ return this }.bind(5); return [typeof b(), b.call(7)]");
sloppy("function F(a) { this.a = a } var B = F.bind(null, 1); var o = new B; return [o.a, o instanceof F, o instanceof B, Object.getPrototypeOf(o) === F.prototype]");
sloppy("function F() {} var B = F.bind(); return [B.prototype, 'prototype' in B, new B instanceof F]");
sloppy("var B = (() => 1).bind(); try { new B } catch (e) { return e.name + ': ' + e.message }");
sloppy("var B = (class A {}).bind(); try { B() } catch (e) { return e.name + ': ' + e.message }");
sloppy("var B = (class A {}).bind(); return new B instanceof Object");
sloppy("var B = Math.max.bind(null, 3); return [B(1, 2), B.name, B.length]");
sloppy("var B = Function.prototype.call.bind(Array.prototype.slice); return S(B([1, 2, 3], 1))");
sloppy("try { Function.prototype.bind.call({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Function.prototype.bind.call(5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Function.prototype.bind.call(undefined) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Function.prototype.call.call(5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Function.prototype.apply.call({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Function.prototype.toString.call({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { (function(){}).apply(null, 5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("return [(function(){ return arguments.length }).apply(null, {length: 2}), (function(){ return arguments.length }).apply(null, null)]");
sloppy("try { Reflect.apply(function(){}, null) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Reflect.construct(function(){}, []) ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Reflect.construct(() => 1, []) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Reflect.construct(function(){}, [], 5) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new 5 } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new ({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new (()=>1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new Math.max } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new ({m(){}}).m } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new (async function(){}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { new (function*(){}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { (class A {})() } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { var A = class {}; A() } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { Symbol() instanceof 5 } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { ({}) instanceof ({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { ({}) instanceof (()=>1) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var F = function(){}; F.prototype = 5; try { ({}) instanceof F } catch (e) { return e.name + ': ' + e.message }");
sloppy("var F = function(){}; F.prototype = 5; return 5 instanceof F");
sloppy("var F = function(){}; Object.defineProperty(F, Symbol.hasInstance, {value: () => true}); return {} instanceof F");
sloppy("return [D(Function.prototype, Symbol.hasInstance), Function.prototype[Symbol.hasInstance].name]");

// Classes.
const clsBody = "class A { constructor(a) { this.a = a } m() {} static s() {} get g() { return 1 } set g(v) {} static get sg() { return 1 } [Symbol.iterator]() {} ['comp' + 'uted']() {} async am() {} *gm() {} async *agm() {} static x = 1; y = 2; static #priv = 3; #p = 4; 'quoted'() {} 42() {} static { this.blk = 5 } }";
sloppy(`${clsBody} return S(Reflect.ownKeys(A))`);
sloppy(`${clsBody} return S(Reflect.ownKeys(A.prototype))`);
sloppy(`${clsBody} return S(Object.getOwnPropertyDescriptors(A.prototype))`);
sloppy(`${clsBody} return S(Object.getOwnPropertyDescriptors(A))`);
sloppy(`${clsBody} return S(Object.getOwnPropertyDescriptors(new A(1)))`);
sloppy(`${clsBody} return S(Object.keys(A.prototype)) + S(Object.keys(A)) + S(Object.keys(new A(1)))`);
sloppy(`${clsBody} var r = []; for (var k in new A(1)) r.push(k); return r`);
sloppy(`${clsBody} return JSON.stringify(new A(1))`);
sloppy(`${clsBody} return D(A, 'prototype')`);
sloppy(`${clsBody} return D(A.prototype, 'constructor')`);
sloppy(`${clsBody} return [D(A.prototype, 'm'), D(A, 's'), D(A.prototype, 'g'), D(A, 'sg'), D(A.prototype, Symbol.iterator), D(A.prototype, 'computed')]`);
sloppy(`${clsBody} return [A.prototype.m.name, A.s.name, A.prototype.computed.name, A.prototype[Symbol.iterator].name, A.prototype.am.name, A.prototype.gm.name, A.prototype.quoted.name, A.prototype[42].name]`);
sloppy(`${clsBody} return [Object.getOwnPropertyDescriptor(A.prototype, 'g').get.name, Object.getOwnPropertyDescriptor(A.prototype, 'g').set.name, Object.getOwnPropertyDescriptor(A, 'sg').get.name]`);
sloppy(`${clsBody} return [D(A.prototype.m, 'prototype'), D(A.prototype.gm, 'prototype'), D(A.prototype.agm, 'prototype'), D(A.prototype.am, 'prototype'), D(A.s, 'prototype')]`);
sloppy(`${clsBody} try { new A.prototype.m } catch (e) { return e.name + ': ' + e.message }`);
sloppy(`${clsBody} try { new A.s } catch (e) { return e.name + ': ' + e.message }`);
sloppy(`${clsBody} try { A.prototype.constructor() } catch (e) { return e.name + ': ' + e.message }`);
sloppy(`${clsBody} A.prototype = {}; A.x = 2; return [typeof A.prototype.m, A.x]`);
strict(`${clsBody} try { A.prototype = {} } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { A.name = 'z' } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { A.prototype.m = 1; return typeof A.prototype.m } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { A.prototype.g = 1; return 'ok' } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { A.sg = 1; return 'ok' } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { delete A.prototype } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { delete A.s; return typeof A.s } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} try { delete A.prototype.constructor; return typeof A.prototype.constructor } catch (e) { return e.name + ': ' + e.message }`);
strict(`${clsBody} var o = new A(1); try { o.g = 5 ; return o.g } catch (e) { return e.name + ': ' + e.message }`);
sloppy("class A { static name = 5 } return [A.name, D(A, 'name'), S(Reflect.ownKeys(A))]");
sloppy("class A { static name() {} } return [typeof A.name, D(A, 'name'), S(Reflect.ownKeys(A))]");
sloppy("class A { static get name() { return 'gg' } } return [A.name, D(A, 'name'), S(Reflect.ownKeys(A))]");
sloppy("class A { static ['name'] = 1 } return [A.name, D(A, 'name')]");
sloppy("class A { static length = 3 } return [A.length, D(A, 'length'), S(Reflect.ownKeys(A))]");
sloppy("class A { static prototype() {} }");
sloppy("class A { static ['prototype']() {} }");
sloppy("try { class A { static ['prototype']() {} } } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { class A { static ['prototype'] = 1 } } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { class A { constructor() {} constructor() {} } } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { eval('class A { constructor() {} constructor() {} }') } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { eval('class A { get constructor() {} }') } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { eval('class A { static prototype() {} }') } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { eval('class A { *constructor() {} }') } catch (e) { return e.name + ': ' + e.message }");
sloppy("try { eval('class A { async constructor() {} }') } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { ['constructor']() { return 1 } } return [typeof A.prototype.constructor, A.prototype.constructor === A, D(A.prototype, 'constructor')]");
sloppy("class A { constructor() {} } return [A.prototype.constructor === A, A.length, A.name]");
sloppy("class A { static m() { return this } } return [A.m() === A, (0, A.m)()]");
sloppy("class A { m() { return this } } var m = new A().m; return m()");
sloppy("class A { m() { return typeof this } } var m = new A().m; return [m(), m.call(5)]");
sloppy("var C = class {}; return [C.name, D(C, 'name')]");
sloppy("var C = class Named {}; return [C.name, typeof Named]");
sloppy("return [(class {}).name, D(class {}, 'name')]");
sloppy("var o = {C: class {}, D: class { static name = 'x' }}; return [o.C.name, o.D.name]");
sloppy("var C = class { static x = this.name }; return C.x");
sloppy("class A { static a = this; static b = A } return [A.a === A, A.b === A]");
sloppy("class A { x = 1; static y = 2 } var o = new A; return [D(o, 'x'), D(A, 'y')]");
sloppy("class A { x = 1; constructor() { this.y = 2 } } return S(Object.keys(new A))");
sloppy("class A { constructor() { this.y = 2 } x = 1 } return S(Object.keys(new A))");
sloppy("class A { x = 1 } class B extends A { z = 3; constructor() { super(); this.w = 4 } } return S(Object.keys(new B))");
sloppy("class A { get x() { return 1 } } var o = new A; o.x = 2; return [o.x, S(Object.keys(o))]");
sloppy("class A { set x(v) { this._x = v } } var o = new A; o.x = 2; return [o.x, S(Object.keys(o))]");
sloppy("class A { get x() { return 1 } } class B extends A { x = 2 } return [new B().x, D(new B, 'x')]");
sloppy("class A { x = 1 } class B extends A { get x() { return 2 } } return [new B().x, D(new B, 'x')]");
sloppy("class A { constructor() { Object.freeze(this) } } class B extends A { x = 1 } try { new B } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { constructor() { return Object.freeze({}) } } class B extends A { x = 1 } try { new B } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { constructor() { return Object.preventExtensions({}) } } class B extends A { #x = 1 } try { new B ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { constructor() { return Object.freeze({}) } } class B extends A { #x = 1 } try { new B } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { constructor() { return 5 } } return typeof new A");
sloppy("class A extends Object { constructor() { return 5 } } try { new A } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A extends Object { constructor() { } } try { new A } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A extends Object { constructor() { super(); super() } } try { new A } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A extends Object { constructor() { this.x = 1; super() } } try { new A } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { #p = 1; static has(o) { return #p in o } } return [A.has(new A), A.has({}), (function(){ try { return A.has(1) } catch (e) { return e.name + ': ' + e.message } })()]");
sloppy("class A { #p = 1; static get(o) { return o.#p } } try { A.get({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { #p = 1; static set(o) { o.#p = 2 } } try { A.set({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { #m() {} static call(o) { o.#m() } } try { A.call({}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { #m() {} static set(o) { o.#m = 1 } } try { A.set(new A) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { get #g() { return 1 } static set(o) { o.#g = 1 } } try { A.set(new A) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { set #g(v) {} static get(o) { return o.#g } } try { A.get(new A) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { #p = 1; static get(o) { return o.#p } } var o = Object.freeze(new A); return A.get(o)");
sloppy("class A { constructor(o) { return o } } class B extends A { #p = 1; static has(o) { return #p in o } } var o = {}; new B(o); try { new B(o) } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { static #p = 1; static g() { return A.#p } } class B extends A {} try { B.g() ; return 'ok' } catch (e) { return e.name + ': ' + e.message }");
sloppy("class A { static #p = 1; static g() { return this.#p } } class B extends A {} try { B.g() } catch (e) { return e.name + ': ' + e.message }");

// ---- Descritores de métodos em literais de objeto.
sloppy("var o = {m() {}, get g() { return 1 }, set s(v) {}, f: function() {}, a: () => 1, async am() {}, *gm() {}, [Symbol.iterator]() {}}; return S(Object.getOwnPropertyDescriptors(o))");
sloppy("var o = {m() {}}; return [D(o.m, 'prototype'), 'prototype' in o.m, S(Reflect.ownKeys(o.m))]");
sloppy("var o = {m() {}}; try { new o.m } catch (e) { return e.name + ': ' + e.message }");
sloppy("var o = {get g() { return 1 }}; var d = Object.getOwnPropertyDescriptor(o, 'g'); return [d.get.name, d.set, D(d.get, 'prototype'), S(Reflect.ownKeys(d.get))]");
sloppy("var o = {f: function() {}, g: function h() {}, i: () => 1, j: class {}}; return [o.f.name, o.g.name, o.i.name, o.j.name]");
sloppy("var o = {}; o.f = function() {}; o.g = () => 1; return [o.f.name, o.g.name]");
sloppy("var f; f = function() {}; var g = () => 1; let h = class {}; return [f.name, g.name, h.name]");
sloppy("var [a = function() {}] = []; var {b = () => 1} = {}; return [a.name, b.name]");
sloppy("var o = {[Symbol('d')]: function() {}, [Symbol()]: function() {}}; var s = Object.getOwnPropertySymbols(o); return [o[s[0]].name, o[s[1]].name]");
sloppy("var k = 'dyn'; var o = {[k]: function() {}, [k + 1]() {}, get [k + 2]() { return 1 }}; return [o.dyn.name, o.dyn1.name, Object.getOwnPropertyDescriptor(o, 'dyn2').get.name]");
sloppy("var o = {1: function() {}, 2() {}, 0.5: () => 1, 1e3: class {}}; return [o[1].name, o[2].name, o[0.5].name, o[1e3].name]");
sloppy("var o = {'a b': function() {}}; return o['a b'].name");
sloppy("var f = function() {}; var g = f; return [f.name, g.name]");
sloppy("var o = {f: (function() {})}; return o.f.name");
sloppy("var o = {f: (0, function() {})}; return o.f.name");
sloppy("var o = {f: function() {}.bind()}; return o.f.name");
sloppy("var o = {f: new Function}; return o.f.name");
sloppy("var o = {f: eval('(function(){})')}; return o.f.name");
sloppy("var o = {f: (function() {}), g: o}; return 1");
sloppy("var x = (function() {}); var y = (() => 1); var z = (class {}); return [x.name, y.name, z.name]");
sloppy("var x = (0, function() {}); return x.name");
sloppy("var x = true ? function() {} : 1; return x.name");
sloppy("var x = null || function() {}; return x.name");
sloppy("var x = null ?? function() {}; return x.name");
sloppy("var x; x ||= function() {}; return x.name");
sloppy("var x; x ??= () => 1; return x.name");
sloppy("var o = {}; o.x ||= function() {}; return o.x.name");
sloppy("var f = function g() {}; return [f.name, typeof g]");
sloppy("(function () { return arguments.callee.name })(); return 1");
sloppy("var o = {get [Symbol.iterator]() { return 1 }}; return Object.getOwnPropertyDescriptor(o, Symbol.iterator).get.name");
sloppy("var o = {set [Symbol('q')](v) {}}; var s = Object.getOwnPropertySymbols(o)[0]; return Object.getOwnPropertyDescriptor(o, s).set.name");
sloppy("var o = {[Symbol()]() {}}; var s = Object.getOwnPropertySymbols(o)[0]; return JSON.stringify(o[s].name)");
sloppy("var o = {[Symbol('')]() {}}; var s = Object.getOwnPropertySymbols(o)[0]; return JSON.stringify(o[s].name)");

// Funções internas e descritores.
sloppy("return [D(Object, 'length'), D(Object, 'name'), D(Object, 'prototype'), D(Object.prototype, 'constructor')]");
sloppy("return [D(Array, 'length'), D(Array.prototype, 'length'), D(Array.prototype, 'constructor'), D(Array.prototype, Symbol.unscopables)]");
sloppy("return S(Reflect.ownKeys(Array.prototype[Symbol.unscopables]))");
sloppy("return [D(Math, 'PI'), D(Math, Symbol.toStringTag), D(JSON, Symbol.toStringTag), D(Reflect, Symbol.toStringTag)]");
sloppy("return [D(Symbol, 'iterator'), D(Number, 'MAX_SAFE_INTEGER'), D(Number, 'EPSILON'), D(globalThis, 'NaN'), D(globalThis, 'undefined'), D(globalThis, 'Infinity')]");
sloppy("return [D(globalThis, 'globalThis'), D(globalThis, 'Object'), D(globalThis, 'parseInt'), D(globalThis, 'Math')]");
sloppy("return [D(String.prototype, 'length'), D(String.prototype, 'constructor'), D(Number.prototype, 'constructor')]");
sloppy("return [D(RegExp.prototype, 'lastIndex'), D(/a/, 'lastIndex'), D(/a/g, 'lastIndex'), S(Reflect.ownKeys(/a/))]");
sloppy("return [S(Reflect.ownKeys(new Error('x')).filter(function(k){return k !== 'stack' && k !== 'line' && k !== 'column' && k !== 'sourceURL'})), D(new Error('x'), 'message')]");
sloppy("return [D(Error.prototype, 'name'), D(Error.prototype, 'message'), D(TypeError.prototype, 'name'), D(TypeError.prototype, 'message'), D(TypeError, 'prototype')]");
sloppy("return [Object.getPrototypeOf(TypeError) === Error, Object.getPrototypeOf(TypeError.prototype) === Error.prototype]");
sloppy("return [D(Promise.prototype, Symbol.toStringTag), D(Map.prototype, Symbol.toStringTag), D(Map.prototype, 'size') === undefined]");
sloppy("return [D(Map, Symbol.species) === undefined, S(Reflect.ownKeys(Map)), S(Reflect.ownKeys(Set.prototype)).length > 5]");
sloppy("return [D(Function.prototype, 'constructor'), D(Function, 'prototype'), D(Function, 'length')]");
sloppy("return [S(Reflect.ownKeys(Symbol.prototype)), D(Symbol.prototype, 'description') === undefined]");
sloppy("return [S(Reflect.ownKeys(Object)), S(Reflect.ownKeys(Reflect))]");
sloppy("return [S(Reflect.ownKeys(Number)), S(Reflect.ownKeys(Boolean.prototype))]");
sloppy("return [S(Reflect.ownKeys(Math)).length, S(Reflect.ownKeys(JSON)), S(Reflect.ownKeys(Boolean))]");

// ---- extras: Object.defineProperty em tipos exóticos.
sloppy("var s = Object('ab'); return [Reflect.defineProperty(s, 0, {value: 'a'}), Reflect.defineProperty(s, 0, {value: 'b'}), Reflect.defineProperty(s, 2, {value: 'c'}), Reflect.defineProperty(s, 'length', {value: 2}), Reflect.defineProperty(s, 'length', {value: 3})]");
sloppy("var s = Object('ab'); try { Object.defineProperty(s, 0, {value: 'z'}) } catch (e) { return e.name + ': ' + e.message }");
sloppy("var s = Object('ab'); Object.defineProperty(s, 5, {value: 'z', enumerable: true}); return [S(Object.keys(s)), s.length]");
sloppy("var s = Object('ab'); s.x = 1; s[3] = 1; s[2] = 5; return [S(Reflect.ownKeys(s)), S(Object.keys(s))]");
sloppy("var s = Object('ab'); return [S(Object.getOwnPropertyDescriptors(s)), JSON.stringify(s), S({...s})]");
sloppy("var r = []; for (var k in Object('ab')) r.push(k); return r");
sloppy("var n = Object(5); n.x = 1; return [S(Object.keys(n)), n + 1]");
sloppy("var d = new Date(0); d.x = 1; return [S(Object.keys(d)), S(Reflect.ownKeys(d))]");
sloppy("var m = new Map; m.x = 1; return [S(Object.keys(m)), S(Reflect.ownKeys(m))]");
sloppy("var p = Promise.resolve(); p.x = 1; return [S(Object.keys(p)), S(Reflect.ownKeys(p))]");
sloppy("var r = /a/g; r.x = 1; return [S(Object.keys(r)), S(Reflect.ownKeys(r))]");
sloppy("var s = Symbol('q'); return [S(Reflect.ownKeys(Object(s))), Object(s) == s, typeof Object(s)]");
sloppy("var b = Object(1n); return [typeof b, b == 1n, S(Reflect.ownKeys(b))]");
sloppy("var g = (function*(){})(); g.x = 1; return [S(Object.keys(g)), S(Reflect.ownKeys(g))]");
sloppy("var m = new WeakMap; return [S(Reflect.ownKeys(m)), Object.isExtensible(m)]");
sloppy("var ab = new ArrayBuffer(4); ab.x = 1; return [S(Reflect.ownKeys(ab)), Object.isFrozen(Object.freeze(ab))]");
sloppy("var dv = new DataView(new ArrayBuffer(4)); return [S(Reflect.ownKeys(dv)), Object.isFrozen(Object.freeze(dv))]");
sloppy("return [S(Reflect.ownKeys(globalThis).slice(0, 0)), Object.getPrototypeOf(globalThis) !== null]");
sloppy("var o = {}; Object.defineProperty(o, 'a', {value: 1, enumerable: true}); Object.defineProperty(o, 'b', {value: 2, enumerable: true, writable: true}); return [JSON.stringify(o), S(Object.assign({}, o)), D(Object.assign({}, o), 'a')]");
sloppy("var o = {}; Object.defineProperty(o, 'a', {get() { return 1 }, enumerable: true}); var c = structuredClone(o); return D(c, 'a')");
sloppy("var o = {a: 1, b: {c: 2}}; Object.freeze(o); return [Object.isFrozen(o), Object.isFrozen(o.b), Object.isFrozen(Object.freeze(o))]");
sloppy("var o = Object.freeze(Object.create({a: 1})); return [Object.isFrozen(o), o.a]");
sloppy("return [Object.isFrozen(1), Object.isFrozen('s'), Object.isFrozen(null), Object.isFrozen(undefined), Object.isSealed(1), Object.isExtensible(1), Object.isExtensible(null)]");
sloppy("return [Object.freeze(1), Object.freeze('s'), Object.freeze(null), Object.seal(true), Object.preventExtensions(5)]");
sloppy("var s = Symbol(); return [Object.freeze(s) === s, Object.isFrozen(s)]");
sloppy("var o = {}; Object.defineProperty(o, 'a', {value: 1}); return [Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o)]");
sloppy("var o = {a: 1}; Object.defineProperty(o, 'a', {configurable: false}); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = {a: 1}; Object.defineProperty(o, 'a', {configurable: false, writable: false}); Object.preventExtensions(o); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = {get a() {return 1}}; Object.defineProperty(o, 'a', {configurable: false}); Object.preventExtensions(o); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = {a: 1, b: 2}; Object.defineProperty(o, 'a', {configurable: false}); Object.preventExtensions(o); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = Object.preventExtensions({}); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = Object.preventExtensions({[Symbol()]: 1}); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = Object.preventExtensions({1: 1}); return [Object.isFrozen(o), Object.isSealed(o)]");
sloppy("var o = Object.freeze({[Symbol()]: 1, 1: 1, a: 1}); return S(Object.getOwnPropertyDescriptors(o))");
sloppy("var o = Object.seal({[Symbol()]: 1, 1: 1, a: 1}); return S(Object.getOwnPropertyDescriptors(o))");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "object-model-golden-"));
const file = path.join(dir, "object_model_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const [mode, body] of programs) {
  const key = mode + "\u0000" + body;
  if (seen.has(key)) continue;
  seen.add(key);
  const wrapped =
    PRELUDE +
    "globalThis.R = (function () { try { return S((function () { " +
    (mode === "strict" ? '"use strict"; ' : "") +
    body +
    " })()) } catch (e) { return 'ERR ' + (e && e.name) + ': ' + (e && e.message) } })();\n";
  const original = mode === "strict" ? '"use strict";\n' + wrapped : wrapped;
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result, meta });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("object_model", rows));
fs.rmSync(dir, { recursive: true, force: true });
