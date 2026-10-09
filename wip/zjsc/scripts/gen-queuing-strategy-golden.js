// Gera tests/golden/queuing_strategy_bun.tsv: `CountQueuingStrategy` e `ByteLengthQueuingStrategy` do global medidos no
// bun 1.4.2 (descritor do global, `length`, `name`, chaves do construtor e do protótipo, acessores, a função `size`
// compartilhada, o dicionário `QueuingStrategyInit` com todas as conversões, chamada sem `new`, `this` inválido).
// Inclui o `Symbol(nodejs.util.inspect.custom)` do protótipo e a mensagem de `size(chunk)` com `chunk` que não é
// objeto (o bun embute o texto da chamada).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-queuing-strategy-golden.js > tests/golden/queuing_strategy_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

for (const N of ["CountQueuingStrategy", "ByteLengthQueuingStrategy"]) {
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
  expr(`${N}.length`);
  expr(`${N}.name`);
  expr(`${N}.toString()`);
  expr(`Object.getOwnPropertyNames(${N})`);
  expr(`Object.getPrototypeOf(${N}) === Function.prototype`);
  expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
  expr(`Reflect.ownKeys(${N}.prototype).filter(function (k) { return typeof k === 'string' || k === Symbol.toStringTag })`);
  expr(`Object.getOwnPropertyNames(${N}.prototype)`);
  expr(`${N}.prototype.constructor === ${N}`);
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable] })(Object.getOwnPropertyDescriptor(${N}.prototype, 'constructor'))`);
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.toStringTag))`);
  // Symbol(nodejs.util.inspect.custom): descritor, função própria da classe, resultado.
  const K = "Symbol.for('nodejs.util.inspect.custom')";
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.name, d.value.length, d.value.toString(), Reflect.ownKeys(d.value)] })(Object.getOwnPropertyDescriptor(${N}.prototype, ${K}))`);
  expr(`Reflect.ownKeys(${N}.prototype).map(function (k) { return typeof k === 'symbol' ? k.toString() : k })`);
  expr(`${N}.prototype[${K}] === ${N === "CountQueuingStrategy" ? "ByteLengthQueuingStrategy" : "CountQueuingStrategy"}.prototype[${K}]`);
  expr(`Object.getPrototypeOf(${N}.prototype[${K}]) === Function.prototype`);
  for (const h of ["3", "3.5", "-0", "NaN", "Infinity", "'7'", "1e21", "0"]) {
    expr(`new ${N}({ highWaterMark: ${h} })[${K}](0, {})`);
  }
  expr(`new ${N}({ highWaterMark: 1 })[${K}]()`);
  expr(`${N}.prototype[${K}].call({}, 0, {})`);
  expr(`${N}.prototype[${K}].call(5)`);
  expr(`${N}.prototype[${K}].call(null)`);
  expr(`${N}.prototype[${K}].call(undefined)`);
  expr(`${N}.prototype[${K}].call('x')`);
  expr(`(function () { var o = {}; return ${N}.prototype[${K}].call(o) === o })()`);
  expr(`(function () { var s = new ${N === "CountQueuingStrategy" ? "ByteLengthQueuingStrategy" : "CountQueuingStrategy"}({ highWaterMark: 1 }); return ${N}.prototype[${K}].call(s) === s })()`);
  for (const p of ["highWaterMark", "size"]) {
    expr(`(function(d){ return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.get.toString(), Object.getOwnPropertyNames(d.get)] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${p}'))`);
  }
  const make = `new ${N}({ highWaterMark: 3.5 })`;
  expr(`Reflect.ownKeys(${make})`);
  expr(`${make}.highWaterMark`);
  expr(`typeof ${make}.size`);
  expr(`${make}.size.name`);
  expr(`${make}.size.length`);
  expr(`${make}.size.toString()`);
  expr(`Object.getOwnPropertyNames(${make}.size)`);
  expr(`'prototype' in ${make}.size`);
  expr(`${make}.size === new ${N}({ highWaterMark: 1 }).size`);
  expr(`${make}.size === ${make}.size`);
  expr(`Object.getPrototypeOf(${make}) === ${N}.prototype`);
  expr(`Object.prototype.toString.call(${make})`);
  expr(`${make} instanceof ${N}`);
  // Dicionário QueuingStrategyInit.
  for (const a of ["", "undefined", "null", "1", "{}", "{ highWaterMark: '7' }", "{ highWaterMark: NaN }", "{ highWaterMark: -1 }",
    "{ highWaterMark: Infinity }", "{ highWaterMark: undefined }", "{ highWaterMark: { valueOf() { return 4 } } }", "{ highWaterMark: Symbol() }",
    "{ highWaterMark: 1n }", "'str'", "true", "{ highWaterMark: null }", "{ highWaterMark: true }", "{ highWaterMark: [] }", "{ highWaterMark: [5] }",
    "{ highWaterMark: '' }", "{ highWaterMark: 'abc' }", "{ highWaterMark: -0 }", "{ highWaterMark: 1e400 }", "{ get highWaterMark() { return 9 } }",
    "{ get highWaterMark() { throw new RangeError('g') } }", "{ highWaterMark: { valueOf() { throw new RangeError('v') } } }",
    "function () {}", "[]", "{ highWaterMark: 1 }, 'ignorado'", "new Proxy({ highWaterMark: 2 }, {})"]) {
    expr(`new ${N}(${a}).highWaterMark`);
  }
  // Chamada sem `new`, `this` inválido, subclasse e Reflect.construct.
  expr(`${N}({ highWaterMark: 1 })`);
  expr(`${N}()`);
  for (const p of ["highWaterMark", "size"]) {
    expr(`${N}.prototype.${p}`);
    expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${p}').get.call({})`);
    expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${p}').get.call(undefined)`);
    expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${p}').get.call(new ${N === "CountQueuingStrategy" ? "ByteLengthQueuingStrategy" : "CountQueuingStrategy"}({ highWaterMark: 1 }))`);
  }
  expr(`(function () { class X extends ${N} {} var x = new X({ highWaterMark: 2 }); return [x.highWaterMark, x instanceof ${N}, Object.getPrototypeOf(x) === X.prototype, typeof x.size] })()`);
  expr(`(function () { function F() {} F.prototype = { marker: 1 }; var x = Reflect.construct(${N}, [{ highWaterMark: 2 }], F); return [x.highWaterMark, Object.getPrototypeOf(x) === F.prototype] })()`);
  // Setter ausente: a atribuição é ignorada no modo sloppy e lança no estrito.
  expr(`(function () { var x = ${make}; x.highWaterMark = 9; return x.highWaterMark })()`);
  expr(`(function () { 'use strict'; var x = ${make}; x.highWaterMark = 9; return x.highWaterMark })()`);
  expr(`(function () { var x = ${make}; x.own = 1; return Reflect.ownKeys(x) })()`);
}

// A função `size` de cada classe.
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size()");
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size('abc')");
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size({ byteLength: 5 })");
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size.call(null)");
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size.call(undefined, 1, 2)");
expr("new CountQueuingStrategy({ highWaterMark: 1 }).size === new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size({ byteLength: 5 })");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size({ byteLength: 'x' })");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size({})");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(new ArrayBuffer(3))");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(new Uint16Array(4))");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size({ get byteLength() { return 9 } })");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size({ get byteLength() { throw new RangeError('b') } })");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size.call(null, { byteLength: 2 })");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(function () {})");
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size([1, 2])");
expr("(function () { try { new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(1) } catch (e) { return [e.name, e instanceof TypeError] } })()");
expr("(function () { try { new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size() } catch (e) { return [e.name, e instanceof TypeError] } })()");
// `size(chunk)` com não objeto: undefined/null lançam com o texto da chamada, o resto devolve undefined.
for (const c of ["undefined", "null", "1", "'a'", "true", "1n", "Symbol()", "0", "''"]) {
  expr(`new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(${c})`);
}
expr("new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size()");
expr("(function () { var f = new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size; return f(null) })()");
expr("(function () { var f = new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size; return f.call(null, undefined) })()");
expr("(function () { var f = new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size; var a = [f]; return a[0](null) })()");
expr("(function () { Number.prototype.byteLength = 42; try { return new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(1) } finally { delete Number.prototype.byteLength } })()");
expr("(function () { try { new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size(null) } catch (e) { return [e.name, e instanceof TypeError, e.message] } })()");
// Ordem entre os globais do host.
expr("Object.getOwnPropertyNames(globalThis).indexOf('ByteLengthQueuingStrategy') < Object.getOwnPropertyNames(globalThis).indexOf('CountQueuingStrategy')");
expr("Object.keys(globalThis).indexOf('CountQueuingStrategy')");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
