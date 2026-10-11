// Gera tests/golden/typedarray_bun.tsv: programas de uma linha sobre os construtores de TypedArray (todas as
// classes mais Float16Array), from/of, métodos, conversões numéricas, índices canônicos, defineProperty/freeze/seal
// em elementos, Reflect.ownKeys, getters de protótipo e acesso fora do limite, medidos no bun. O que envolve
// ArrayBuffer redimensionável e detach fica em scripts/gen-typedarray-more-golden.js.
// Colunas e serialização iguais às de scripts/gen-typedarray-more-golden.js (harness tests/golden/e2e_values_harness.js).
// Uso: bun scripts/gen-typedarray-golden.js > tests/golden/typedarray_bun.tsv
const fs = require("fs");
const { sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const programs = [];
const add = (source) => programs.push(source);
const fn = (body) => `(function () { ${body} })()`;
const tc = (body) => fn(`try { ${body} } catch (e) { return e.name + ': ' + e.message }`);

const TYPES = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
const HAS_F16 = typeof Float16Array !== "undefined";
if (HAS_F16) TYPES.push("Float16Array");
const isBig = (t) => t.startsWith("Big");
const es = (t) => ({ Int8Array: 1, Uint8Array: 1, Uint8ClampedArray: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, Float16Array: 2, Float32Array: 4, Float64Array: 8, BigInt64Array: 8, BigUint64Array: 8 })[t];
const n = (t, v) => (isBig(t) ? `${v}n` : `${v}`);

for (const t of TYPES) {
  const s = es(t);
  const v = (x) => n(t, x);
  // Construtores.
  for (const len of [0, 1, 3, 8, 100]) add(fn(`var u = new ${t}(${len}); return [u.length, u.byteLength, u.byteOffset, u[0], Array.from(u).slice(0, 3)]`));
  for (const bad of ["-1", "-0", "1.5", "'3'", "'x'", "NaN", "Infinity", "2**53", "undefined", "null", "true", "{}"]) add(tc(`return new ${t}(${bad}).length`));
  add(tc(`return new ${t}()`).replace("return new", "return new"));
  add(tc(`return ${t}(2)`));
  add(tc(`return ${t}()`));
  add(tc(`return Array.from(new ${t}([${v(1)}, ${v(2)}, ${v(3)}]))`));
  add(tc(`return Array.from(new ${t}(new Set([${v(1)}, ${v(2)}])))`));
  add(tc(`return Array.from(new ${t}({length: 2, 0: ${v(5)}, 1: ${v(6)}}))`));
  add(tc(`return Array.from(new ${t}(new ${t}([${v(1)}, ${v(2)}])))`));
  add(tc(`return Array.from(new ${t}((function* () { yield ${v(1)}; yield ${v(2)} })()))`));
  add(tc(`return new ${t}([1, 2, 'x']).length`));
  add(tc(`return new ${t}(['1', '2']).length`));
  add(tc(`return new ${t}([{valueOf() { return ${v(7)} }}])[0]`));
  add(tc(`return new ${t}([undefined])[0]`));
  add(tc(`return new ${t}([null])[0]`));
  add(tc(`return new ${t}([Symbol()])[0]`));
  // Buffer com offset e length, erros de alinhamento.
  const buf = `var b = new ArrayBuffer(${16 * s});`;
  add(fn(`${buf} var u = new ${t}(b); return [u.length, u.byteLength, u.byteOffset, u.buffer === b]`));
  add(fn(`${buf} var u = new ${t}(b, ${s}); return [u.length, u.byteLength, u.byteOffset]`));
  add(fn(`${buf} var u = new ${t}(b, ${s}, 4); return [u.length, u.byteLength, u.byteOffset]`));
  add(fn(`${buf} var u = new ${t}(b, ${16 * s}); return [u.length, u.byteLength, u.byteOffset]`));
  add(fn(`${buf} var u = new ${t}(b, ${s}, 0); return [u.length, u.byteLength]`));
  add(fn(`${buf} var u = new ${t}(b, undefined, 2); return [u.length, u.byteOffset]`));
  add(fn(`${buf} var u = new ${t}(b, '${s}', '2'); return [u.length, u.byteOffset]`));
  add(tc(`${buf} return new ${t}(b, ${17 * s}).length`));
  add(tc(`${buf} return new ${t}(b, 0, 17).length`));
  add(tc(`${buf} return new ${t}(b, ${s}, 16).length`));
  add(tc(`${buf} return new ${t}(b, -1).length`));
  add(tc(`${buf} return new ${t}(b, 0, -1).length`));
  add(tc(`${buf} return new ${t}(b, 1.5).length`));
  add(tc(`${buf} return new ${t}(b, NaN, NaN).length`));
  add(tc(`${buf} return new ${t}(b, Infinity).length`));
  if (s > 1) {
    add(tc(`${buf} return new ${t}(b, 1).length`));
    add(tc(`${buf} return new ${t}(b, ${s - 1}, 1).length`));
    add(tc(`var b = new ArrayBuffer(${16 * s + 1}); return new ${t}(b).length`));
    add(tc(`var b = new ArrayBuffer(${s + 1}); return new ${t}(b).length`));
    add(tc(`var b = new ArrayBuffer(${s - 1}); return new ${t}(b).length`));
  }
  add(tc(`var b = new SharedArrayBuffer(${8 * s}); var u = new ${t}(b); return [u.length, u.buffer === b, Object.prototype.toString.call(u.buffer)]`));
  add(tc(`return new ${t}(new DataView(new ArrayBuffer(8))).length`));
  add(tc(`return new ${t}(Symbol()).length`));
  add(tc(`return new ${t}(1n).length`));
  add(tc(`var u = new ${t}([${v(1)}]); var c = new ${t}(u); c[0] = ${v(2)}; return [u[0], c[0], c.buffer === u.buffer]`));
  add(tc(`var o = new ${t}(2); return Object.getPrototypeOf(o) === ${t}.prototype`));
  add(tc(`class C extends ${t} {} var c = new C(3); return [c.length, c instanceof ${t}, c.constructor === C, Object.prototype.toString.call(c)]`));
  add(tc(`var r = Reflect.construct(${t}, [2], Object); return [Object.getPrototypeOf(r) === Object.prototype, r.length]`));
  add(tc(`function F() {} F.prototype = Array.prototype; var r = Reflect.construct(${t}, [2], F); return [Object.getPrototypeOf(r) === Array.prototype, r.length, ArrayBuffer.isView(r)]`));
  // Propriedades da classe.
  add(fn(`return [${t}.name, ${t}.length, ${t}.BYTES_PER_ELEMENT, ${t}.prototype.BYTES_PER_ELEMENT, Object.getPrototypeOf(${t}) === Object.getPrototypeOf(Int8Array)]`));
  add(fn(`return [Object.getOwnPropertyNames(${t}).sort(), Object.getOwnPropertyNames(${t}.prototype).sort()]`));
  add(fn(`var d = Object.getOwnPropertyDescriptor(${t}, 'BYTES_PER_ELEMENT'); return [d.writable, d.enumerable, d.configurable, d.value]`));
  add(fn(`var d = Object.getOwnPropertyDescriptor(${t}.prototype, 'constructor'); return [d.writable, d.enumerable, d.configurable, d.value === ${t}]`));
  add(fn(`return Object.prototype.toString.call(new ${t}(1))`));
  add(fn(`return new ${t}(1)[Symbol.toStringTag]`));
  add(fn(`return ${t}.prototype[Symbol.toStringTag]`));
  // from / of.
  add(tc(`return Array.from(${t}.from([${v(1)}, ${v(2)}, ${v(3)}]))`));
  add(tc(`return Array.from(${t}.from([${v(1)}, ${v(2)}], function (x, i) { return x + (${isBig(t) ? "BigInt(i)" : "i"}) }))`));
  add(tc(`return Array.from(${t}.from({length: 2, 0: ${v(4)}, 1: ${v(5)}}))`));
  add(tc(`return Array.from(${t}.from(new Set([${v(1)}, ${v(2)}])))`));
  add(tc(`return Array.from(${t}.from('12'))`));
  add(tc(`return ${t}.from()`));
  add(tc(`return ${t}.from(null)`));
  add(tc(`return ${t}.from([1], 5)`));
  add(tc(`var th = {k: ${v(9)}}; return Array.from(${t}.from([${v(1)}], function () { return this.k }, th))`));
  add(tc(`return ${t}.from.call(Object, [1])`));
  add(tc(`return ${t}.from.call(function () { return new ${t}(1) }, [1, 2, 3]).length`));
  add(tc(`return ${t}.from.call(undefined, [1])`));
  add(tc(`return Array.from(${t}.of(${v(1)}, ${v(2)}, ${v(3)}))`));
  add(tc(`return ${t}.of().length`));
  add(tc(`return ${t}.of.call(Object, 1)`));
  add(tc(`return ${t}.of.call(function () { return new ${t}(1) }, 1, 2).length`));
  // Métodos.
  const a = `var u = new ${t}([${[5, 1, 4, 2, 3].map(v).join(",")}]);`;
  add(tc(`${a} var r = u.set([${v(9)}, ${v(8)}], 1); return [r, Array.from(u)]`));
  add(tc(`${a} u.set(new ${t}([${v(9)}]), 4); return Array.from(u)`));
  add(tc(`${a} u.set([${v(9)}], 5); return Array.from(u)`));
  add(tc(`${a} u.set([${v(9)}], -1); return Array.from(u)`));
  add(tc(`${a} u.set([${v(9)}], Infinity); return Array.from(u)`));
  add(tc(`${a} u.set(u.subarray(0, 3), 1); return Array.from(u)`));
  add(tc(`${a} u.set(u.subarray(1, 4), 0); return Array.from(u)`));
  add(tc(`${a} u.set({length: 2, 0: ${v(7)}, 1: ${v(6)}}); return Array.from(u)`));
  add(tc(`${a} u.set('12'); return Array.from(u)`));
  add(tc(`${a} u.set(5); return Array.from(u)`));
  add(tc(`${a} u.set(); return Array.from(u)`));
  add(tc(`${a} u.set(null)`));
  add(tc(`${a} u.set([${v(1)}], 1.5); return Array.from(u)`));
  add(tc(`${a} var s = u.subarray(1, 3); return [s.length, s.byteOffset, s.buffer === u.buffer, Array.from(s)]`));
  add(tc(`${a} var s = u.subarray(-2); return [s.length, s.byteOffset]`));
  add(tc(`${a} var s = u.subarray(3, 1); return [s.length, s.byteOffset]`));
  add(tc(`${a} var s = u.subarray(1, -1); s[0] = ${v(0)}; return [s.length, Array.from(u)]`));
  add(tc(`${a} var s = u.subarray(); return [s.length, s === u]`));
  add(tc(`${a} var s = u.subarray(NaN, undefined); return s.length`));
  add(tc(`${a} var s = u.subarray(10); return [s.length, s.byteOffset]`));
  add(tc(`${a} var s = u.slice(1, 3); s[0] = ${v(0)}; return [s.length, s.buffer === u.buffer, Array.from(s), Array.from(u)]`));
  add(tc(`${a} return Array.from(u.slice(-2))`));
  add(tc(`${a} return Array.from(u.slice(2, 1))`));
  add(tc(`${a} return Array.from(u.slice())`));
  add(tc(`${a} return Array.from(u.slice(undefined, undefined))`));
  add(tc(`${a} return [u.fill(${v(7)}) === u, Array.from(u)]`));
  add(tc(`${a} u.fill(${v(7)}, 1, 3); return Array.from(u)`));
  add(tc(`${a} u.fill(${v(7)}, -2); return Array.from(u)`));
  add(tc(`${a} u.fill(${v(7)}, 3, 1); return Array.from(u)`));
  add(tc(`${a} u.fill(${v(7)}, NaN, Infinity); return Array.from(u)`));
  add(tc(`${a} u.fill('9'); return Array.from(u)`));
  add(tc(`${a} u.fill(); return Array.from(u)`));
  add(tc(`${a} u.fill(undefined); return Array.from(u)`));
  add(tc(`${a} u.fill({valueOf() { return ${v(3)} }}); return Array.from(u)`));
  add(tc(`${a} u.copyWithin(0, 3); return Array.from(u)`));
  add(tc(`${a} u.copyWithin(1, 0, 3); return Array.from(u)`));
  add(tc(`${a} u.copyWithin(-2, -4, -3); return Array.from(u)`));
  add(tc(`${a} u.copyWithin(0, 10); return Array.from(u)`));
  add(tc(`${a} u.copyWithin(); return Array.from(u)`));
  add(tc(`${a} return [u.copyWithin(1, 2) === u]`));
  add(tc(`${a} u.sort(); return Array.from(u)`));
  add(tc(`${a} u.sort(function (x, y) { return y > x ? 1 : y < x ? -1 : 0 }); return Array.from(u)`));
  add(tc(`${a} return [u.sort() === u]`));
  add(tc(`${a} u.sort(null)`));
  add(tc(`${a} u.sort({})`));
  add(tc(`${a} u.sort(undefined); return Array.from(u)`));
  add(tc(`${a} u.sort(function () { return NaN }); return Array.from(u)`));
  add(tc(`${a} u.sort(function () { throw new Error('boom') })`));
  add(tc(`${a} return Array.from(u.toSorted())`));
  add(tc(`${a} var r = u.toSorted(function (x, y) { return y > x ? 1 : -1 }); return [Array.from(r), Array.from(u), r.buffer === u.buffer]`));
  add(tc(`${a} u.toSorted(null)`));
  add(tc(`${a} var r = u.toReversed(); return [Array.from(r), Array.from(u)]`));
  add(tc(`${a} return [u.reverse() === u, Array.from(u)]`));
  add(tc(`${a} var r = u.with(1, ${v(0)}); return [Array.from(r), Array.from(u)]`));
  add(tc(`${a} return Array.from(u.with(-1, ${v(0)}))`));
  add(tc(`${a} return u.with(5, ${v(0)})`));
  add(tc(`${a} return u.with(-6, ${v(0)})`));
  add(tc(`${a} return Array.from(u.with(1.9, ${v(0)}))`));
  add(tc(`${a} return u.with(0, 'x')`));
  add(tc(`${a} return [u.at(0), u.at(-1), u.at(5), u.at(-6), u.at(1.9), u.at(NaN), u.at()]`));
  add(tc(`${a} return [u.indexOf(${v(4)}), u.indexOf(${v(4)}, 3), u.indexOf(${v(4)}, -3), u.indexOf(${v(99)}), u.indexOf('4'), u.indexOf()]`));
  add(tc(`${a} return [u.lastIndexOf(${v(4)}), u.lastIndexOf(${v(4)}, 1), u.lastIndexOf(${v(4)}, -4), u.lastIndexOf(${v(5)}, -5), u.lastIndexOf(${v(5)}, -6)]`));
  add(tc(`${a} return [u.includes(${v(4)}), u.includes(${v(4)}, 3), u.includes(${v(99)}), u.includes(), u.includes(undefined)]`));
  add(tc(`${a} return [u.join(), u.join(''), u.join('-'), u.join(undefined), u.join(null), u.toString(), String(u)]`));
  add(tc(`${a} return [u.toLocaleString(), typeof u.toLocaleString]`));
  add(tc(`${a} return [u.find(function (x) { return x > ${v(3)} }), u.findIndex(function (x) { return x > ${v(3)} }), u.findLast(function (x) { return x > ${v(3)} }), u.findLastIndex(function (x) { return x > ${v(3)} }), u.find(function () { return false }), u.findIndex(function () { return false })]`));
  add(tc(`${a} return [u.every(function (x) { return x > ${v(0)} }), u.some(function (x) { return x > ${v(4)} }), u.some(function (x) { return x > ${v(9)} })]`));
  add(tc(`${a} var r = []; u.forEach(function (x, i, o) { r.push([typeof x, i, o === u]) }); return r`));
  add(tc(`${a} return u.reduce(function (p, x) { return p + x })`));
  add(tc(`${a} return u.reduceRight(function (p, x) { return p + '' + x }, '')`));
  add(tc(`${a} return new ${t}(0).reduce(function (p, x) { return p + x })`));
  add(tc(`${a} return u.map(5)`));
  add(tc(`${a} var m = u.map(function (x) { return x }); return [m.constructor === ${t}, m === u, m.buffer === u.buffer, Array.from(m)]`));
  add(tc(`${a} return Array.from(u.map(function (x, i) { return i }))`));
  add(tc(`${a} return Array.from(u.filter(function (x) { return x > ${v(2)} }))`));
  add(tc(`${a} return Array.from(u.filter(function () { return false })).length`));
  add(tc(`${a} u.constructor = {[Symbol.species]: Uint8Array}; var m = u.map(function (x) { return ${isBig(t) ? "1" : "x"} }); return [m.constructor === Uint8Array, m.length]`));
  add(tc(`${a} u.constructor = {[Symbol.species]: Array}; return u.map(function (x) { return x })`));
  add(tc(`${a} u.constructor = {[Symbol.species]: null}; return u.filter(function () { return true }).constructor === ${t}`));
  add(tc(`${a} u.constructor = undefined; return u.slice().constructor === ${t}`));
  add(tc(`${a} u.constructor = 5; return u.slice()`));
  add(tc(`${a} u.constructor = {[Symbol.species]: function (len) { return new ${t}(1) }}; return u.slice()`));
  add(tc(`${a} u.constructor = {[Symbol.species]: function (len) { return new ${t}(len) }}; return Array.from(u.slice(1))`));
  add(tc(`${a} u.constructor = {[Symbol.species]: function () { return {} }}; return u.slice()`));
  add(tc(`${a} u.constructor = {[Symbol.species]: function (b, o, l) { return new ${t}(b, o, l) }}; return Array.from(u.subarray(1, 3))`));
  add(tc(`${a} return [Array.from(u.entries()), Array.from(u.keys()), Array.from(u.values()), u[Symbol.iterator] === u.values]`));
  add(tc(`${a} var it = u.entries(); return [Object.prototype.toString.call(it), typeof it.next, it[Symbol.iterator]() === it]`));
  add(tc(`${a} var it = u.values(); u[0] = ${v(0)}; return it.next().value`));
  add(tc(`${a} return [...u]`));
  add(tc(`${a} var [x, y] = u; return [x, y]`));
  add(tc(`${a} return Array.prototype.slice.call(u, 1, 3)`));
  add(tc(`${a} return Array.prototype.concat.call([], u).length`));
  add(tc(`${a} return Array.isArray(u)`));
  add(tc(`${a} return JSON.stringify(u)`));
  add(tc(`${a} return JSON.stringify({u: u})`));
  add(tc(`return Int8Array.prototype.map.call([1], function (x) { return x })`));
  add(tc(`return ${t}.prototype.fill.call({}, 1)`));
  add(tc(`return ${t}.prototype.join.call(new Uint8Array(2), '+')`));
  add(tc(`return Object.getPrototypeOf(${t}.prototype).keys.call(new ${t}(2)).next().value`));
  // Índices canônicos e acesso fora do limite.
  const b = `var u = new ${t}([${v(1)}, ${v(2)}, ${v(3)}]);`;
  add(tc(`${b} return [u[0], u[2], u[3], u[-1], u[1.5], u['1'], u['01'], u['1.0'], u[1e21], u['-0'], u['Infinity'], u['NaN'], u['+1'], u[' 1'], u['1 ']]`));
  add(tc(`${b} u[3] = ${v(9)}; u[-1] = ${v(9)}; u[1.5] = ${v(9)}; u['-0'] = ${v(9)}; u['Infinity'] = ${v(9)}; u['NaN'] = ${v(9)}; return [Object.keys(u), Array.from(u)]`));
  add(tc(`${b} u['01'] = ${v(9)}; u['+1'] = ${v(9)}; u['1.0'] = ${v(9)}; u.foo = 1; return [Object.keys(u), Array.from(u)]`));
  add(tc(`${b} return ['-0' in u, '1.5' in u, 'Infinity' in u, 'NaN' in u, '3' in u, '2' in u, -1 in u, '01' in u, 0 in u, 'length' in u]`));
  add(tc(`${b} return [delete u[0], delete u[5], delete u['-0'], delete u['1.5'], delete u.foo, Array.from(u)]`));
  add(tc(`'use strict'; ${b} return delete u[0]`));
  add(tc(`'use strict'; ${b} u[5] = ${v(1)}; return Array.from(u)`));
  add(tc(`'use strict'; ${b} u['-0'] = ${v(1)}; return Array.from(u)`));
  add(tc(`${b} return [Object.getOwnPropertyDescriptor(u, '0'), Object.getOwnPropertyDescriptor(u, '3'), Object.getOwnPropertyDescriptor(u, '-0'), Object.getOwnPropertyDescriptor(u, '1.5')]`));
  add(tc(`${b} return [u.hasOwnProperty(0), u.hasOwnProperty(3), u.hasOwnProperty('-0'), Object.hasOwn(u, 2), Object.hasOwn(u, '2.0')]`));
  add(tc(`${b} return Object.keys(u)`));
  add(tc(`${b} u.foo = 1; u[Symbol.for('s')] = 2; return [Reflect.ownKeys(u), Object.getOwnPropertyNames(u), Object.getOwnPropertySymbols(u).length]`));
  add(tc(`${b} var r = []; for (var k in u) r.push(k); return r`));
  add(tc(`${b} var o = Object.create(u); o[5] = 1; o[1] = ${v(9)}; return [Object.keys(o), u[1], o[1], o[5], Array.from(u)]`));
  add(tc(`${b} var o = Object.create(u); o[-1] = 1; return [Object.keys(o), o[-1], u[-1]]`));
  add(tc(`${b} var o = Object.create(u); o.foo = 1; return [Object.keys(o), o.length]`));
  add(tc(`${b} return Reflect.set(u, 5, ${v(1)})`));
  add(tc(`${b} return [Reflect.set(u, 1, ${v(8)}), Reflect.set(u, 'x', 1), Reflect.set(u, '-0', 1), Array.from(u)]`));
  add(tc(`${b} return [Reflect.get(u, 5), Reflect.get(u, 1), Reflect.get(u, '-0'), Reflect.get(u, 1, {})]`));
  add(tc(`${b} return [Reflect.has(u, 1), Reflect.has(u, 5), Reflect.has(u, '-0'), Reflect.deleteProperty(u, 1), Reflect.deleteProperty(u, 5)]`));
  add(tc(`${b} return [Reflect.set(u, 1, ${v(8)}, {}), Array.from(u)]`));
  add(tc(`${b} var r = {}; var ok = Reflect.set(u, 1, ${v(8)}, r); return [ok, Object.getOwnPropertyDescriptor(r, '1'), Array.from(u)]`));
  add(tc(`${b} var r = {}; var ok = Reflect.set(u, 7, ${v(8)}, r); return [ok, Object.keys(r), Array.from(u)]`));
  add(tc(`${b} var r = new ${t}(3); var ok = Reflect.set(u, 1, ${v(8)}, r); return [ok, Array.from(r), Array.from(u)]`));
  add(tc(`${b} var r = new ${t}(3); var ok = Reflect.set(u, 9, ${v(8)}, r); return [ok, Array.from(r), Array.from(u)]`));
  add(tc(`${b} return [Object.isExtensible(u), Object.isFrozen(u), Object.isSealed(u)]`));
  add(tc(`${b} Object.preventExtensions(u); u[1] = ${v(7)}; u.foo = 1; return [Object.isExtensible(u), Object.keys(u), Array.from(u), Object.isFrozen(u), Object.isSealed(u)]`));
  add(tc(`${b} Object.freeze(u)`));
  add(tc(`return Object.isFrozen(new ${t}(0)) + ':' + Object.isFrozen(Object.freeze(new ${t}(0)))`));
  add(tc(`${b} Object.seal(u); return [Object.isSealed(u), Object.isFrozen(u), Array.from(u)]`));
  add(tc(`return Object.isSealed(Object.seal(new ${t}(0)))`));
  add(tc(`${b} Object.seal(u); u[0] = ${v(5)}; return Array.from(u)`));
  add(tc(`${b} return Object.defineProperty(u, '1', {value: ${v(9)}}) === u && Array.from(u)`));
  add(tc(`${b} Object.defineProperty(u, '1', {value: ${v(9)}, writable: true, enumerable: true, configurable: true}); return Array.from(u)`));
  add(tc(`${b} Object.defineProperty(u, '1', {value: ${v(9)}, writable: false})`));
  add(tc(`${b} Object.defineProperty(u, '1', {value: ${v(9)}, enumerable: false})`));
  add(tc(`${b} Object.defineProperty(u, '1', {value: ${v(9)}, configurable: false})`));
  add(tc(`${b} Object.defineProperty(u, '1', {get() { return 1 }})`));
  add(tc(`${b} Object.defineProperty(u, '1', {})`));
  add(tc(`${b} Object.defineProperty(u, '5', {value: ${v(9)}})`));
  add(tc(`${b} Object.defineProperty(u, '-0', {value: ${v(9)}})`));
  add(tc(`${b} Object.defineProperty(u, '1.5', {value: ${v(9)}})`));
  add(tc(`${b} Object.defineProperty(u, 'Infinity', {value: ${v(9)}})`));
  add(tc(`${b} Object.defineProperty(u, 'foo', {value: ${v(9)}}); return u.foo`));
  add(tc(`${b} return [Reflect.defineProperty(u, '1', {value: ${v(9)}, writable: false}), Reflect.defineProperty(u, '5', {value: ${v(9)}}), Reflect.defineProperty(u, '-0', {value: ${v(9)}}), Reflect.defineProperty(u, '1', {value: ${v(9)}}), Array.from(u)]`));
  add(tc(`${b} return Object.defineProperties(u, {0: {value: ${v(4)}}, foo: {value: 1}}) === u`));
  add(tc(`${b} return Object.assign(u, [${v(7)}, ${v(8)}]) === u && Array.from(u)`));
  add(tc(`${b} return Object.entries(u).length + ':' + Object.values(u).join()`));
  add(tc(`${b} return Object.fromEntries(Object.entries(u))`));
  add(tc(`${b} return Object.setPrototypeOf(u, null) === u && [u[0], u.length, Object.keys(u)]`));
  add(tc(`${b} return structuredClone(u).constructor === ${t}`));
  add(tc(`${b} var p = new Proxy(u, {}); return [p.length, p[0]]`));
  add(tc(`${b} var p = new Proxy(u, {}); return p.fill(${v(1)})`));
  // Getters do protótipo.
  const TA = "Object.getPrototypeOf(Int8Array)";
  add(fn(`var d = Object.getOwnPropertyDescriptor(${TA}.prototype, 'buffer'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length]`));
  add(fn(`var d = Object.getOwnPropertyDescriptor(${TA}.prototype, 'length'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name]`));
  add(fn(`var d = Object.getOwnPropertyDescriptor(${TA}.prototype, Symbol.toStringTag); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.call(1), d.get.call({}), d.get.call(new ${t}(1))]`));
  add(tc(`return ${t}.prototype.length`));
  add(tc(`return ${t}.prototype.byteLength`));
  add(tc(`return ${t}.prototype.byteOffset`));
  add(tc(`return ${t}.prototype.buffer`));
  add(tc(`return ${t}.prototype[Symbol.toStringTag]`));
  add(tc(`var g = Object.getOwnPropertyDescriptor(${TA}.prototype, 'length').get; return g.call({})`));
  add(tc(`var g = Object.getOwnPropertyDescriptor(${TA}.prototype, 'buffer').get; return g.call([])`));
  add(tc(`var g = Object.getOwnPropertyDescriptor(${TA}.prototype, 'byteLength').get; return g.call(new DataView(new ArrayBuffer(1)))`));
  add(tc(`var g = Object.getOwnPropertyDescriptor(${TA}.prototype, 'byteOffset').get; return g.call(1)`));
  add(tc(`var u = new ${t}(new ArrayBuffer(${8 * s}), ${s}, 2); return [u.length, u.byteLength, u.byteOffset, u.buffer.byteLength]`));
  add(tc(`var u = new ${t}(2); u.length = 9; return u.length`));
  add(tc(`'use strict'; var u = new ${t}(2); u.length = 9; return u.length`));
  add(tc(`'use strict'; var u = new ${t}(2); u.buffer = 1`));
  add(tc(`return ${TA}()`));
  add(tc(`return new ${TA}()`));
  add(tc(`return ${TA}.name + ${TA}.length + ${TA}.prototype.constructor.name`));
  add(tc(`return ${TA}.from === ${t}.from && ${TA}.of === ${t}.of`));
  add(tc(`return [Object.getOwnPropertyNames(${TA}.prototype).sort().join()]`));
  add(tc(`return ${TA}[Symbol.species] === ${TA}`));
  add(tc(`return ${t}[Symbol.species] === ${t}`));
  add(tc(`return ${t}.prototype.toString === Array.prototype.toString`));
  add(tc(`return ${TA}.prototype[Symbol.iterator] === ${TA}.prototype.values`));
  // Conversões numéricas.
  const conv = isBig(t)
    ? ["0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "2n**64n", "2n**64n+5n", "-(2n**63n)", "-(2n**63n)-1n", "2n**128n+3n", "true", "false", "'12'", "'0x10'", "'x'", "''", "1", "1.5", "NaN", "undefined", "null", "Symbol()", "{valueOf() { return 7n }}", "{valueOf() { return 7 }}", "[3n]", "'-0'"]
    : ["0", "-0", "1", "-1", "0.5", "1.5", "2.5", "254.5", "255.5", "-0.5", "255", "256", "257", "-129", "128", "127.9", "32768", "65535", "65536", "2**31", "2**32", "2**32+1", "-(2**31)-1", "1e21", "-1e21", "1e308", "NaN", "Infinity", "-Infinity", "Number.MAX_VALUE", "Number.MIN_VALUE", "1/3", "16777217", "0.1", "65504", "65520", "65519.9", "6.1e-5", "5.9e-8", "3e-8", "'12'", "'  7  '", "'0x10'", "'x'", "''", "true", "false", "null", "undefined", "[5]", "[]", "{valueOf() { return 9 }}", "1n", "Symbol()", "123456789.123"];
  for (const c of conv) {
    add(tc(`var u = new ${t}(1); u[0] = ${c}; return [u[0], Object.is(u[0], -0)]`));
    add(tc(`return new ${t}([${c}])[0]`));
  }
  add(tc(`var u = new ${t}(1); u[0] = ${isBig(t) ? "1" : "1n"}; return u[0]`));
  add(tc(`var u = new ${t}(1); return u.fill(${isBig(t) ? "1" : "1n"})`));
  add(tc(`var u = new ${t}(1); return Array.from(u.with(0, ${isBig(t) ? "1" : "1n"}))`));
  add(tc(`return new ${t}(new ArrayBuffer(${8 * s})).fill(${v(1)}).buffer.byteLength`));
  // Reinterpretação de bytes.
  if (!isBig(t) && t !== "Uint8ClampedArray") {
    add(fn(`var f = new ${t}([1, -1, 2.5, 300]); return Array.from(new Uint8Array(f.buffer))`));
    add(fn(`var f = new ${t}(1); new Uint8Array(f.buffer).fill(255); return [f[0], new Int8Array(f.buffer)[0]]`));
  }
}

// Mistura entre tipos.
for (const a of TYPES) {
  for (const b of TYPES) {
    if (isBig(a) !== isBig(b)) {
      add(tc(`return new ${a}(new ${b}(1))`));
      add(tc(`return new ${a}(1).set(new ${b}(1))`));
      add(tc(`return new ${a}(1).constructor.from(new ${b}(1))`));
    } else {
      add(tc(`return Array.from(new ${a}(new ${b}([1, 2, 255, 256, -1, 1.5, 70000])))`.replace("-1, 1.5", isBig(a) ? "3, 4" : "-1, 1.5").replace(/\b(\d+)(?=[,\]])/g, (m) => (isBig(a) ? m + "n" : m))));
      add(tc(`var x = new ${b}(4); var y = new ${a}(4); y.set(x); return Array.from(y)`));
    }
  }
}

// Casos gerais.
add(fn(`return [typeof Int8Array, typeof Float16Array, typeof BigInt64Array]`));
add(fn(`return ArrayBuffer.isView(new Int8Array(1)) + ':' + ArrayBuffer.isView(new DataView(new ArrayBuffer(1))) + ':' + ArrayBuffer.isView([])`));
add(fn(`return Object.getOwnPropertyNames(Object.getPrototypeOf(Int8Array)).sort().join()`));
add(fn(`return Object.getOwnPropertyNames(Object.getPrototypeOf(Int8Array.prototype)).sort().join()`));
add(fn(`return Object.getOwnPropertySymbols(Object.getPrototypeOf(Int8Array.prototype)).map(String).join()`));
add(fn(`return Reflect.ownKeys(new Int8Array(3)).join()`));
add(fn(`var u = new Int8Array(3); u.x = 1; u[Symbol.iterator] = 1; return Reflect.ownKeys(u).map(String).join()`));
add(fn(`var u = new Int8Array(2); u.b = 1; u.a = 2; return Object.keys(u).join()`));

// ---------------------------------------------------------------------------------------------
const seen = new Set();
const lines = [];
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const src of programs) {
  if (HOST.test(src)) continue;
  if (seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  const t0 = Date.now();
  const out = (0, eval)(`${harness}(${JSON.stringify(src)})`);
  if (Date.now() - t0 > 2000) throw new Error(`${src}: lento demais, entrada patológica`);
  if (typeof out !== "string") throw new Error(`${src}: o harness não devolveu string`);
  if (out.length > 20000) throw new Error(`${src}: saída grande demais`);
  if (/\/home\/|\.rs|\.js:/.test(out)) throw new Error(`${src}: saída com caminho da máquina`);
  lines.push(`${src}\t${out}`);
}
// A geração combinatória passa de 5 mil programas; guarda ~900 por hash do programa (sampleByHash, determinístico) para o
// teste Rust, que roda cada um em realm novo, ficar rápido.
const sampled = sampleByHash(lines, 900, (line) => line.slice(0, line.indexOf("\t")));
if (sampled.length < 600) throw new Error(`só ${sampled.length} programas`);
fs.writeSync(1, sampled.join("\n") + "\n");
process.exit(0);
