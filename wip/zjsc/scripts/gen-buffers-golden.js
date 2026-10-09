// Gera tests/golden/buffers_bun.tsv: programas de uma linha sobre ArrayBuffer, SharedArrayBuffer,
// TypedArray, DataView e Atomics, avaliados no bun. Colunas: fonte do programa, KIND (o typeof do valor
// de conclusão, ou "throw") e REPR (serialização determinística, a do tests/golden/e2e_values_harness.js,
// o MESMO texto que tests/buffers_bun_golden.rs embute). Erros saem como "throw<TAB>Nome: mensagem".
// Cada programa é ASCII de uma linha; nenhuma entrada passa de alguns KB, e o gerador roda tudo sob
// um alarme de parede (Atomics.wait só com timeout curto).
// Uso: bun scripts/gen-buffers-golden.js > tests/golden/buffers_bun.tsv
const fs = require("fs");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const programs = [];
const add = (source) => programs.push(source);

const TYPES = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
const NUM = TYPES.filter((t) => !t.startsWith("Big"));
const BIG = TYPES.filter((t) => t.startsWith("Big"));
const lit = (t, values) => (t.startsWith("Big") ? values.map((v) => `${v}n`) : values).join(",");
const A = (t) => `Array.from(`; // reservado para legibilidade

// ---------------------------------------------------------------------------------------------
// ArrayBuffer.
for (const source of [
  "new ArrayBuffer(8).byteLength",
  "new ArrayBuffer(0).byteLength",
  "new ArrayBuffer().byteLength",
  "new ArrayBuffer(-1)",
  "new ArrayBuffer(1.9).byteLength",
  "new ArrayBuffer('3').byteLength",
  "new ArrayBuffer(NaN).byteLength",
  "new ArrayBuffer(2 ** 53)",
  "new ArrayBuffer(Infinity)",
  "ArrayBuffer(8)",
  "ArrayBuffer.length",
  "ArrayBuffer.name",
  "ArrayBuffer.isView(new Uint8Array(1))",
  "ArrayBuffer.isView(new DataView(new ArrayBuffer(1)))",
  "ArrayBuffer.isView(new ArrayBuffer(1))",
  "ArrayBuffer.isView()",
  "Object.prototype.toString.call(new ArrayBuffer(1))",
  "new ArrayBuffer(8, {maxByteLength: 16}).resizable",
  "new ArrayBuffer(8).resizable",
  "new ArrayBuffer(8, {maxByteLength: 16}).maxByteLength",
  "new ArrayBuffer(8).maxByteLength",
  "new ArrayBuffer(8, {maxByteLength: 4})",
  "new ArrayBuffer(8, {maxByteLength: -1})",
  "new ArrayBuffer(8, {maxByteLength: undefined}).resizable",
  "new ArrayBuffer(8, {}).resizable",
  "new ArrayBuffer(8, 5).resizable",
  "new ArrayBuffer(0, {maxByteLength: 0}).resizable",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize(12); return [b.byteLength, b.maxByteLength] })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize(17) })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize(-1) })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize() ; return b.byteLength })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize('8'); return b.byteLength })()",
  "(function () { var b = new ArrayBuffer(4); b.resize(2) })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); b.resize(0); return b.byteLength })()",
  "ArrayBuffer.prototype.resize.call({}, 1)",
  "ArrayBuffer.prototype.resize.call(new SharedArrayBuffer(1), 1)",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); var u = new Uint8Array(b); b.resize(8); return u.length })()",
  "(function () { var b = new ArrayBuffer(4, {maxByteLength: 16}); var u = new Uint8Array(b, 0, 4); b.resize(2); return [u.length, u.byteLength, u.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var u = new Uint8Array(b, 4); b.resize(2); return [u.length, u.byteLength, u.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var u = new Uint8Array(b, 4); b.resize(2); b.resize(8); return [u.length, u.byteLength, u.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8); var c = b.transfer(); return [b.detached, b.byteLength, c.byteLength, c.detached] })()",
  "(function () { var b = new ArrayBuffer(8); var c = b.transfer(4); return [b.byteLength, c.byteLength, c.resizable] })()",
  "(function () { var b = new ArrayBuffer(8); var c = b.transfer(16); return [c.byteLength, c.resizable] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 32}); var c = b.transfer(); return [c.byteLength, c.resizable, c.maxByteLength] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 32}); var c = b.transferToFixedLength(); return [c.byteLength, c.resizable, c.maxByteLength] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 32}); var c = b.transferToFixedLength(4); return [c.byteLength, c.resizable] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 32}); var c = b.transfer(40) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); b.transfer() })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); b.transferToFixedLength() })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); return [b.byteLength, b.maxByteLength, b.resizable, b.detached] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); b.transfer(); b.resize(4) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(-1) })()",
  "(function () { var b = new ArrayBuffer(8); var c = b.transfer(undefined); return c.byteLength })()",
  "(function () { var b = new ArrayBuffer(8); var c = b.transfer(null); return c.byteLength })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return [u.length, u.byteLength, u.byteOffset, u[0]] })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); return d.byteLength })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); return d.byteOffset })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); return d.buffer === b })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); u[0] = 7; var c = b.transfer(); return new Uint8Array(c)[0] })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.fill(1) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return Array.from(u) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.at(0) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.includes(0) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.subarray(0) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.join() })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.set([1]) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.sort() })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return u.with(0, 1) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return Object.keys(u) })()",
  "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b); b.transfer(); return new Uint8Array(u) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); return new Uint8Array(b) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); return new DataView(b) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); return Object.prototype.toString.call(b) })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); return ArrayBuffer.isView(b) })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); return d.getInt8(0) })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); return d.setInt8(0, 1) })()",
  "ArrayBuffer.prototype.transfer.call({}, 1)",
  "ArrayBuffer.prototype.slice.call({}, 1)",
  "ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(4), 1)",
  "ArrayBuffer.prototype.byteLength",
  "ArrayBuffer.prototype.resizable",
  "ArrayBuffer.prototype.detached",
  "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get.call(new SharedArrayBuffer(1))",
  "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call(new SharedArrayBuffer(1))",
  "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call({})",
  "Object.getOwnPropertyNames(ArrayBuffer.prototype).sort()",
  "Object.getOwnPropertyNames(ArrayBuffer).sort()",
  "ArrayBuffer.prototype[Symbol.toStringTag]",
  "ArrayBuffer[Symbol.species] === ArrayBuffer",
  "Array.from(new Uint8Array(new ArrayBuffer(8).slice(2, 6)))",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return Array.from(new Uint8Array(u.buffer.slice(2, 6))) })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return Array.from(new Uint8Array(u.buffer.slice(-3))) })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return Array.from(new Uint8Array(u.buffer.slice(-3, -1))) })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice(6, 2).byteLength })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice(100).byteLength })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice(undefined, undefined).byteLength })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice(1, Infinity).byteLength })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice(-Infinity, 3).byteLength })()",
  "(function () { var u = new Uint8Array([1,2,3,4,5,6,7,8]); return u.buffer.slice('2', '5').byteLength })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); return [b.slice(0).resizable, b.slice(0).byteLength] })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function () { return {} }}; return b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function () { return b }}; return b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function () { return new ArrayBuffer(2) }}; return b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function () { return new SharedArrayBuffer(8) }}; return b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = 1; return b.slice(0) })()",
  "(function () { var b = new ArrayBuffer(8); b.constructor = undefined; return b.slice(0).byteLength })()",
  "(function () { class B extends ArrayBuffer {}; var b = new B(8); return [b.slice(1) instanceof B, b.slice(1).byteLength] })()",
  "(function () { class B extends ArrayBuffer {}; return Object.getPrototypeOf(new B(1)) === B.prototype })()",
  "Reflect.construct(ArrayBuffer, [4], Object).byteLength",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); b.resize(16); return Array.from(new Uint8Array(b)).length })()",
  "(function () { var b = new ArrayBuffer(8, {get maxByteLength() { return 16 }}); return b.maxByteLength })()",
  "(function () { var log = []; new ArrayBuffer({valueOf() { log.push('len'); return 1 }}, {get maxByteLength() { log.push('max'); return 2 }}); return log })()",
  "(function () { var log = []; try { new ArrayBuffer(-1, {get maxByteLength() { log.push('max'); return 2 }}) } catch (e) { log.push(e.name) } return log })()",
  "(function () { var log = []; try { new ArrayBuffer(8, {maxByteLength: {valueOf() { log.push('max'); return 2 }}}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
]) {
  add(source);
}

// ---------------------------------------------------------------------------------------------
// SharedArrayBuffer.
for (const source of [
  "new SharedArrayBuffer(8).byteLength",
  "new SharedArrayBuffer(-1)",
  "SharedArrayBuffer(8)",
  "SharedArrayBuffer.length",
  "new SharedArrayBuffer(8, {maxByteLength: 16}).growable",
  "new SharedArrayBuffer(8).growable",
  "new SharedArrayBuffer(8, {maxByteLength: 16}).maxByteLength",
  "new SharedArrayBuffer(8).maxByteLength",
  "new SharedArrayBuffer(8, {maxByteLength: 4})",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow(12); return [b.byteLength, b.growable] })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow(2) })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow(17) })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow(-1) })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow(4); return b.byteLength })()",
  "(function () { var b = new SharedArrayBuffer(4); b.grow(8) })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); b.grow() })()",
  "SharedArrayBuffer.prototype.grow.call(new ArrayBuffer(1, {maxByteLength: 4}), 2)",
  "SharedArrayBuffer.prototype.grow.call({}, 2)",
  "SharedArrayBuffer.prototype.slice.call(new ArrayBuffer(4), 1)",
  "Object.prototype.toString.call(new SharedArrayBuffer(1))",
  "SharedArrayBuffer.prototype[Symbol.toStringTag]",
  "Object.getOwnPropertyNames(SharedArrayBuffer.prototype).sort()",
  "new SharedArrayBuffer(8).slice(2, 6).byteLength",
  "new SharedArrayBuffer(8).slice(-2).byteLength",
  "new SharedArrayBuffer(8).slice(6, 2).byteLength",
  "new SharedArrayBuffer(8, {maxByteLength: 16}).slice(0).growable",
  "Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'byteLength').get.call(new ArrayBuffer(1))",
  "Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'growable').get.call(new ArrayBuffer(1))",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); var u = new Uint8Array(b); b.grow(8); return u.length })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); var u = new Uint8Array(b, 0, 4); b.grow(8); return u.length })()",
  "(function () { var b = new SharedArrayBuffer(4, {maxByteLength: 16}); var d = new DataView(b); b.grow(8); return d.byteLength })()",
  "(function () { var b = new SharedArrayBuffer(8); return new Uint8Array(b).buffer === b })()",
  "ArrayBuffer.isView(new Uint8Array(new SharedArrayBuffer(1)))",
  "(function () { class S extends SharedArrayBuffer {}; return new S(4).slice(1) instanceof S })()",
]) {
  add(source);
}

// ---------------------------------------------------------------------------------------------
// TypedArray: construtores e propriedades por tipo.
for (const t of TYPES) {
  const big = t.startsWith("Big");
  const v = (...xs) => lit(t, xs);
  add(`${t}.BYTES_PER_ELEMENT`);
  add(`${t}.name + '/' + ${t}.length`);
  add(`new ${t}(3)`);
  add(`new ${t}(0).length`);
  add(`new ${t}([${v(1, 2, 3)}])`);
  add(`new ${t}(-1)`);
  add(`${t}(2)`);
  add(`new ${t}(1.5).length`);
  add(`new ${t}(new ArrayBuffer(8)).length`);
  add(`new ${t}(new ArrayBuffer(7))`);
  add(`new ${t}(new ArrayBuffer(8), 1)`);
  add(`new ${t}(new ArrayBuffer(8), 100)`);
  add(`new ${t}(new ArrayBuffer(8), 0, 100)`);
  add(`new ${t}(new ArrayBuffer(8), 0, 1).length`);
  add(`new ${t}(new ArrayBuffer(8), -1)`);
  add(`new ${t}(new ArrayBuffer(8), 8).length`);
  add(`new ${t}(new ArrayBuffer(8), undefined, 1).length`);
  add(`new ${t}({length: 2, 0: ${v(5)[0] === undefined ? 5 : v(5)}, 1: ${v(6)}})`);
  add(`new ${t}(new Set([${v(1, 2)}]))`);
  add(`new ${t}(new ${t}([${v(1, 2, 3)}]))`);
  add(`new ${t}(function () {})`);
  add(`new ${t}(null).length`);
  add(`new ${t}(undefined).length`);
  add(`new ${t}('3').length`);
  add(`new ${t}(Symbol())`);
  add(`new ${t}(new ${big ? "Float64Array" : "BigInt64Array"}(2))`);
  add(`new ${t}(new ${big ? "Int8Array" : "BigUint64Array"}([${big ? "1" : "1n"}]))`);
  add(`Object.getPrototypeOf(${t}) === Object.getPrototypeOf(Int8Array)`);
  add(`Object.getPrototypeOf(${t}.prototype) === Object.getPrototypeOf(Int8Array.prototype)`);
  add(`${t}.prototype.BYTES_PER_ELEMENT`);
  add(`Object.prototype.toString.call(new ${t}(1))`);
  add(`new ${t}(4).byteLength + ',' + new ${t}(4).byteOffset`);
  add(`new ${t}(new ArrayBuffer(16), 8, 1).byteOffset`);
  add(`Object.getOwnPropertyNames(${t}.prototype)`);
  add(`Object.getOwnPropertyNames(${t})`);
  // from / of
  add(`${t}.from([${v(1, 2, 3)}])`);
  add(`${t}.from([${v(1, 2, 3)}], function (x) { return x })`);
  add(`${t}.from({length: 2, 0: ${v(1)}, 1: ${v(2)}})`);
  add(`${t}.from(new Set([${v(3, 4)}]))`);
  add(`${t}.from('')`);
  add(`${t}.from([${v(1)}], 5)`);
  add(`${t}.from(null)`);
  add(`${t}.from([${v(1, 2)}], function (x, i) { return ${big ? "BigInt(i)" : "i"} + this.k }, {k: ${big ? "10n" : "10"}})`);
  add(`${t}.of(${v(1, 2, 3)})`);
  add(`${t}.of()`);
  add(`${t}.of.call(Array, ${v(1)})`);
  add(`${t}.from.call(Array, [${v(1)}])`);
  add(`${t}.of.call(function () { return new Int8Array(1) }, ${v(1, 2)})`);
  add(`${t}.of.call(function () { return {} }, ${v(1)})`);
  add(`${t}.from.call(function (n) { return new ${t}(n + 1) }, [${v(1)}])`);
  add(`${t}.from([${v(1)}], undefined)`);
  // acesso
  add(`(function () { var u = new ${t}([${v(1, 2, 3)}]); return [u[0], u[2], u[3], u[-1], u['1'], u['1.5'], u['-0'], 1 in u, 3 in u, '-0' in u, '1.5' in u] })()`);
  add(`(function () { var u = new ${t}(2); u[5] = ${v(1)}; u['x'] = 1; u[-1] = 1; u['1.5'] = 1; return [Object.keys(u), u.x, u[5], u[-1]] })()`);
  add(`(function () { 'use strict'; var u = new ${t}(2); u[5] = ${v(1)}; return u.length })()`);
  add(`(function () { 'use strict'; var u = new ${t}(2); delete u[0] })()`);
  add(`(function () { var u = new ${t}(2); return [delete u[0], delete u[5], Object.getOwnPropertyDescriptor(u, 0)] })()`);
  add(`(function () { var u = new ${t}(2); return Object.defineProperty(u, 0, {value: ${v(9)}, writable: true, enumerable: true, configurable: true})[0] })()`);
  add(`(function () { var u = new ${t}(2); return Object.defineProperty(u, 0, {value: ${v(9)}, writable: false})[0] })()`);
  add(`(function () { var u = new ${t}(2); return Object.defineProperty(u, 0, {get() {}}) })()`);
  add(`(function () { var u = new ${t}(2); return Object.defineProperty(u, 5, {value: 1}) })()`);
  add(`(function () { var u = new ${t}(2); return Object.freeze(u) })()`);
  add(`(function () { var u = new ${t}(0); return [Object.freeze(u).length, Object.isFrozen(u)] })()`);
  add(`(function () { var u = new ${t}(2); return [Object.isExtensible(u), Object.isSealed(Object.preventExtensions(u))] })()`);
  add(`(function () { var u = new ${t}(2); return Object.seal(u)[0] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); var o = []; for (var k in u) o.push(k); return o })()`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); return [...u] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); return [...u.entries()] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); return [...u.keys()] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); return [...u.values()].length })()`);
  add(`new ${t}([${v(1, 2)}])[Symbol.iterator] === ${t}.prototype.values`);
  add(`(function () { var u = new ${t}([${v(1, 2)}]); return JSON.stringify(u) })()`);
  add(`new ${t}([${v(1, 2, 3)}]).toString()`);
  add(`new ${t}([${v(1, 2, 3)}]).join('-')`);
  add(`new ${t}([${v(1, 2, 3)}]).toLocaleString()`);
  add(`Object.getOwnPropertyDescriptor(new ${t}([${v(1)}]), 0)`);
  add(`Object.getOwnPropertyNames(new ${t}(2))`);
  add(`Reflect.ownKeys(new ${t}(2))`);
  add(`Reflect.set(new ${t}(2), 5, ${v(1)})`);
  add(`Reflect.set(new ${t}(2), 1, ${v(1)})`);
  add(`Reflect.defineProperty(new ${t}(2), 5, {value: ${v(1)}})`);
  add(`Reflect.has(new ${t}(2), 2)`);
  add(`(function () { var u = new ${t}(2); var p = Object.create(u); p[0] = ${v(1)}; return [u[0], Object.hasOwn(p, 0)] })()`);
  add(`(function () { var u = new ${t}(2); var p = Object.create(u); return [p[0], p[5], Reflect.get(u, 0, {})] })()`);
  // métodos de leitura
  add(`new ${t}([${v(1, 2, 3, 4)}]).at(-1)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).at(10)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).at('1')`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).at()`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findLast(function (x) { return x < ${v(3)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findLastIndex(function (x) { return x < ${v(3)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findLast(function (x) { return false })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findLastIndex(function (x) { return false })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).find(function (x) { return x > ${v(1)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findIndex(function (x) { return x > ${v(1)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).findLast(1)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(${v(3)})`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(${v(3)}, 3)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(${v(3)}, -2)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(${v(3)}, -100)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(undefined)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes('1')`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).includes(1)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).indexOf(${v(3)})`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).indexOf(${v(3)}, 3)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).indexOf(${v(3)}, -2)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).indexOf(${v(9)})`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).indexOf('3')`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).lastIndexOf(${v(3)})`);
  add(`new ${t}([${v(1, 3, 3, 4)}]).lastIndexOf(${v(3)}, 1)`);
  add(`new ${t}([${v(1, 3, 3, 4)}]).lastIndexOf(${v(3)}, -1)`);
  add(`new ${t}([${v(1, 3, 3, 4)}]).lastIndexOf(${v(3)}, -100)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).reduce(function (a, b) { return a + b })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).reduceRight(function (a, b) { return a + b })`);
  add(`new ${t}(0).reduce(function (a, b) { return a + b })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).map(function (x) { return x })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).filter(function (x) { return x > ${v(2)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).every(function (x) { return x > ${v(0)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).some(function (x) { return x > ${v(3)} })`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).forEach(function () {})`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).map(5)`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).filter()`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).reverse()`);
  add(`new ${t}([${v(1, 2, 3, 4)}]).toReversed()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); var r = u.toReversed(); return [u[0], r[0], r instanceof ${t}] })()`);
  // from/set/subarray/slice
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}]); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}], 2); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}], 3) })()`);
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}], -1) })()`);
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}], Infinity) })()`);
  add(`(function () { var u = new ${t}(4); u.set([${v(1, 2)}], '1'); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set(new ${t}([${v(3, 4)}]), 1); return u })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); u.set(u.subarray(0, 3), 1); return u })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); u.set(u.subarray(1), 0); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set({length: 2, 0: ${v(7)}, 1: ${v(8)}}); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set('12'); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set(5); return u })()`);
  add(`(function () { var u = new ${t}(4); u.set(null) })()`);
  add(`(function () { var u = new ${t}(4); u.set() })()`);
  add(`(function () { var u = new ${t}(4); return u.set([]) })()`);
  add(`(function () { var u = new ${t}(4); u.set(new ${big ? "Int8Array" : "BigInt64Array"}(1)) })()`);
  add(`(function () { var u = new ${t}(4); u.set([${big ? "1" : "1n"}]) })()`);
  add(`(function () { var u = new ${t}(4); u.set(new ArrayBuffer(2)) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(1, 3) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(-2) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(3, 1).length })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(1).byteOffset })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); var s = u.subarray(1, 3); s[0] = ${v(9)}; return [u[1], s.buffer === u.buffer] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(undefined, undefined).length })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(1, undefined).length })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray(NaN, 2).length })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.subarray('1', '3').length })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.slice(1, 3) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.slice(-2) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.slice(3, 1) })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); var s = u.slice(1); s[0] = ${v(9)}; return [u[1], s.buffer === u.buffer] })()`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3, 4)}]); return u.slice(undefined, undefined).length })()`);
  // fill / copyWithin
  add(`new ${t}(4).fill(${v(7)})`);
  add(`new ${t}(4).fill(${v(7)}, 1, 3)`);
  add(`new ${t}(4).fill(${v(7)}, -2)`);
  add(`new ${t}(4).fill(${v(7)}, 3, 1)`);
  add(`new ${t}(4).fill(${v(7)}, NaN, 2)`);
  add(`new ${t}(4).fill()`);
  add(`new ${t}(4).fill(${big ? "1" : "1n"})`);
  add(`new ${t}(4).fill('7')`);
  add(`new ${t}(4).fill({valueOf() { return ${big ? "3n" : "3"} }})`);
  add(`new ${t}(4).fill(Symbol())`);
  add(`(function () { var u = new ${t}(4); var b = u.buffer; u.fill({valueOf() { return ${v(1)} }}, {valueOf() { return 1 }}); return u })()`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(0, 3)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(1, 0, 3)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(-2, 0, 2)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(0, -2)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(2, 1)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(10, 0)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin()`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(0, 1, 1)`);
  add(`new ${t}([${v(1, 2, 3, 4, 5)}]).copyWithin(1, 0, undefined)`);
  // sort / toSorted / with
  add(`new ${t}([${v(3, 1, 2)}]).sort()`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(function (a, b) { return b > a ? 1 : b < a ? -1 : 0 })`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(undefined)`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(null)`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(1)`);
  add(`new ${t}([${v(3, 1, 2)}]).sort('a')`);
  add(`new ${t}([${v(3, 1, 2)}]).toSorted()`);
  add(`new ${t}([${v(3, 1, 2)}]).toSorted(function (a, b) { return b > a ? 1 : -1 })`);
  add(`new ${t}([${v(3, 1, 2)}]).toSorted(null)`);
  add(`new ${t}([${v(3, 1, 2)}]).toSorted(1)`);
  add(`(function () { var u = new ${t}([${v(3, 1, 2)}]); var s = u.toSorted(); return [u[0], s[0], s.buffer === u.buffer, s instanceof ${t}] })()`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(function () { return NaN })`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(function () { return {valueOf() { return 1 }} })`);
  add(`new ${t}([${v(3, 1, 2)}]).sort(function () { throw new RangeError('x') })`);
  add(`new ${t}([${v(1, 2, 3)}]).with(1, ${v(9)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(-1, ${v(9)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(3, ${v(9)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(-4, ${v(9)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with('1', ${v(9)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(1)`);
  add(`new ${t}([${v(1, 2, 3)}]).with(1, ${big ? "1" : "1n"})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(Infinity, ${v(1)})`);
  add(`new ${t}([${v(1, 2, 3)}]).with(NaN, ${v(1)})`);
  add(`(function () { var u = new ${t}([${v(1, 2, 3)}]); var w = u.with(0, ${v(5)}); return [u[0], w[0], w.buffer === u.buffer] })()`);
  // length-tracking e fora do limite após resize
  const rab = (n, m) => `new ArrayBuffer(${n}, {maxByteLength: ${m}})`;
  const sz = `${t}.BYTES_PER_ELEMENT`;
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var r = [u.length]; b.resize(8 * 8); r.push(u.length); b.resize(0); r.push(u.length, u.byteLength, u.byteOffset); return r })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8); var r = [u.length, u.byteOffset]; b.resize(8); r.push(u.length, u.byteOffset); b.resize(7); r.push(u.length, u.byteLength, u.byteOffset); b.resize(40); r.push(u.length, u.byteOffset); return r })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); var r = [u.length]; b.resize(16); r.push(u.length); b.resize(15); r.push(u.length, u.byteLength, u.byteOffset); b.resize(32); r.push(u.length, u.byteOffset); return r })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.fill(${v(1)}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.at(0) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.slice() })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.subarray(0) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return [u[0], 0 in u, Object.keys(u)] })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return Array.from(u) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.join() })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return new ${t}(u) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return u.set([${v(1)}]) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); b.resize(8); return [...u.keys()] })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 2); var it = u.values(); b.resize(8); return it.next() })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var it = u.values(); it.next(); b.resize(0); return it.next() })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); b.resize(2 * 8); return u.map(function (x) { return x }).length })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.map(function (x, i) { if (i === 0) b.resize(1 * 8); return x }).length })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var seen = []; u.forEach(function (x, i) { if (i === 0) b.resize(2 * 8); seen.push(i) }); return seen })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var seen = []; u.forEach(function (x, i) { if (i === 0) b.resize(6 * 8); seen.push(i) }); return seen })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.fill({valueOf() { b.resize(8 * 2); return ${v(3)} }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 0, 4); return u.fill(${v(3)}, {valueOf() { b.resize(8 * 2); return 0 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.slice({valueOf() { b.resize(8 * 2); return 0 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.includes(${v(0)}, {valueOf() { b.resize(0); return 0 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.includes(undefined, {valueOf() { b.resize(0); return 0 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.indexOf(${v(0)}, {valueOf() { b.resize(0); return 0 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.copyWithin(0, 1, {valueOf() { b.resize(8 * 2); return 4 }}) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.subarray(1).length })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var s = u.subarray(1); b.resize(8 * 8); return s.length })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); var s = u.subarray(1, 3); b.resize(8 * 8); return s.length })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.toSorted().buffer.resizable })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return u.slice().buffer.resizable })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); u[100] = 1; b.resize(64); u[7] = ${v(3)}; return u[7] })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); return [new ${t}(b, 8).length, new ${t}(b, 4 * 8).length] })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; return new ${t}(b, 4 * 8 + 8) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b, 8, 100) })()`);
  add(`(function () { var b = ${rab(4 * 8, 64)}; var u = new ${t}(b); b.resize(0); return [u.length, u.byteLength, u.byteOffset, u[0], Object.keys(u)] })()`);
  if (t !== "Uint8Array") {
    add(`(function () { var b = ${rab(7, 64)}; return new ${t}(b).length })()`);
    add(`(function () { var b = ${rab(13, 64)}; var u = new ${t}(b); return [u.length, u.byteLength] })()`);
    add(`(function () { var b = ${rab(8 * 2, 64)}; var u = new ${t}(b); b.resize(8 + 1); return [u.length, u.byteLength] })()`);
  }
}

// Conversões numéricas por tipo.
const conv = [
  ["Int8Array", [127, 128, 255, 256, -129, 1.9, -1.9, NaN, Infinity, -Infinity, 2 ** 31, -(2 ** 31) - 1, 2 ** 53, "'12'", "'x'", "true", "null", "undefined", "[]", "[5]", "{}"]],
  ["Uint8Array", [255, 256, -1, 1.9, -1.9, NaN, Infinity, 2 ** 32 + 5, "'12'", "'0x10'", "''"]],
  ["Uint8ClampedArray", [-1, 0, 0.5, 1.5, 2.5, 254.5, 255, 255.5, 256, 1000, -0.5, NaN, Infinity, -Infinity, "'7.5'", "0.49999999999999994", "1.5000000000000002"]],
  ["Int16Array", [32767, 32768, 65535, 65536, -32769, 1.9, NaN, Infinity, 2 ** 40 + 3]],
  ["Uint16Array", [65535, 65536, -1, 1.9, NaN, 2 ** 40 + 3]],
  ["Int32Array", [2 ** 31 - 1, 2 ** 31, 2 ** 32, -(2 ** 31) - 1, 1.9, NaN, Infinity, 2 ** 53, 2 ** 64, 1e300]],
  ["Uint32Array", [2 ** 32 - 1, 2 ** 32, -1, 1.9, NaN, 2 ** 53, 1e300]],
  ["Float32Array", [1.5, 0.1, 16777217, 3.4028235677973366e38, 3.4028235677973362e38, 1e39, -1e39, 1e-46, 1e-45, 1.401298464324817e-45, NaN, -0, Infinity, "'1.1'"]],
  ["Float64Array", [0.1, -0, NaN, Infinity, 1e308, 5e-324, "'1.1'", "'x'", "undefined"]],
];
for (const [t, values] of conv) {
  for (const x of values) {
    const lit2 = typeof x === "number" ? (Object.is(x, -0) ? "-0" : String(x)) : x;
    add(`new ${t}([${lit2}])`);
  }
}
for (const t of BIG) {
  for (const x of ["0n", "1n", "-1n", "2n ** 63n", "2n ** 63n - 1n", "-(2n ** 63n)", "2n ** 64n", "2n ** 64n - 1n", "2n ** 64n + 5n", "-(2n ** 63n) - 1n", "2n ** 100n", "true", "false", "'12'", "'x'", "'0x10'", "1", "1.5", "NaN", "undefined", "null", "[]", "[7n]", "{}", "Symbol()", "{valueOf() { return 3n }}", "{valueOf() { return 3 }}"]) {
    add(`new ${t}([${x}])`);
    add(`(function () { var u = new ${t}(1); u[0] = ${x}; return u[0] })()`);
  }
  add(`(function () { var u = new ${t}(1); u[0] = 2n ** 63n; return u[0] })()`);
  add(`(function () { var u = new ${t}([1n, 2n]); return u.map(function (x) { return x * 2n }) })()`);
  add(`(function () { var u = new ${t}([1n, 2n]); return u.map(function (x) { return 1 }) })()`);
  add(`(function () { var u = new ${t}([1n, 2n]); return u.reduce(function (a, b) { return a + b }) })()`);
  add(`(function () { var u = new ${t}([3n, 1n, 2n]); return u.sort() })()`);
  add(`(function () { var u = new ${t}([3n, -1n, 2n]); return [u.sort(), u.indexOf(2n), u.includes(2), u.indexOf(2)] })()`);
  add(`(function () { var u = new ${t}([3n, -1n, 2n]); return Array.prototype.join.call(u, '+') })()`);
  add(`(function () { var u = new ${t}([3n, -1n, 2n]); return JSON.stringify(u) })()`);
  add(`(function () { var u = new ${t}([3n]); return u.fill(1) })()`);
  add(`(function () { var u = new ${t}([3n]); return Atomics.add(u, 0, 1n) })()`);
}
for (const t of NUM) {
  add(`(function () { var u = new ${t}([1, 2]); return u.fill(1n) })()`);
  add(`(function () { var u = new ${t}([1, 2]); u[0] = 1n })()`);
  add(`(function () { var u = new ${t}([1, 2]); return u.with(0, 1n) })()`);
  add(`(function () { var u = new ${t}([1, 2]); return u.map(function () { return 1n }) })()`);
  add(`(function () { var u = new ${t}([1, 2]); u[0] = Symbol() })()`);
  add(`(function () { var u = new ${t}([1, 2]); u[0] = {valueOf() { throw new SyntaxError('v') }} })()`);
  add(`(function () { var u = new ${t}([1, 2]); u[5] = {valueOf() { throw new SyntaxError('v') }} })()`);
}
// NaN e -0 em Float.
for (const t of ["Float32Array", "Float64Array"]) {
  add(`(function () { var u = new ${t}([NaN, -0, 0]); return [u.includes(NaN), u.indexOf(NaN), u.includes(0), u.indexOf(-0), Object.is(u[1], -0), u.lastIndexOf(0)] })()`);
  add(`new ${t}([3, NaN, -0, 0, -Infinity, 1]).sort()`);
  add(`new ${t}([NaN, 1, NaN, -1]).toSorted()`);
  add(`new ${t}([0, -0, 0, -0]).sort().map(function (x) { return 1 / x })`);
}
add(`new Float32Array([1.1])[0]`);
add(`new Float32Array([16777217])[0]`);
add(`new Float32Array(new Float64Array([1.1, 2.2]))`);
add(`new Uint8Array(new Float64Array([1.9, -1.9, 300, NaN]))`);
add(`new Uint8ClampedArray(new Float64Array([1.9, -1.9, 300, NaN, 2.5, 3.5]))`);
add(`new Int8Array(new Int16Array([127, 128, -129]))`);
add(`new Uint8Array(new Int8Array([-1, -128]))`);
add(`new Uint8Array(new Uint16Array([0x1234, 0xffff]).buffer)`);
add(`new Uint16Array(new Uint8Array([1, 2, 3, 4]).buffer)`);
add(`new Float32Array(new Uint32Array([0x3fc00000, 0x7fc00000, 0x7f800000, 0xff800000, 0x80000000]).buffer)`);
add(`new Uint32Array(new Float32Array([1.5, -0, Infinity]).buffer)`);
add(`new Uint32Array(new Float64Array([1.5, -0]).buffer)`);
add(`new BigInt64Array(new Uint8Array([255, 255, 255, 255, 255, 255, 255, 255]).buffer)`);
add(`new BigUint64Array(new Uint8Array([255, 255, 255, 255, 255, 255, 255, 255]).buffer)`);
add(`new BigInt64Array(new BigUint64Array([2n ** 64n - 1n, 2n ** 63n]))`);
add(`new BigUint64Array(new BigInt64Array([-1n, -(2n ** 63n)]))`);
add(`new BigInt64Array([1n]).buffer.byteLength`);
add(`new Uint8Array(new BigInt64Array([1n]))`);
add(`new BigInt64Array(new Int8Array([1]))`);
add(`new BigInt64Array([1, 2])`);
add(`BigInt64Array.from([1n, 2n], function (x) { return x + 1n })`);
add(`BigInt64Array.from([1n, 2n], function (x) { return Number(x) })`);
add(`BigInt64Array.of(1n, 2n)`);
add(`BigInt64Array.of(1, 2)`);
add(`new BigInt64Array(2).toString()`);
add(`new BigInt64Array([2n ** 63n, 1n]).toString()`);
add(`BigInt.asIntN(64, 2n ** 63n)`);
add(`BigInt.asUintN(64, -1n)`);

// %TypedArray% intrínseco.
for (const source of [
  "var T = Object.getPrototypeOf(Int8Array); new T()",
  "var T = Object.getPrototypeOf(Int8Array); T()",
  "var T = Object.getPrototypeOf(Int8Array); T.from([1])",
  "var T = Object.getPrototypeOf(Int8Array); T.of(1)",
  "var T = Object.getPrototypeOf(Int8Array); [T.name, T.length]",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyNames(T.prototype).sort()",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyNames(T).sort()",
  "var T = Object.getPrototypeOf(Int8Array); T.prototype[Symbol.toStringTag]",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyDescriptor(T.prototype, Symbol.toStringTag).get.call(new Int8Array(1))",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyDescriptor(T.prototype, Symbol.toStringTag).get.call([])",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyDescriptor(T.prototype, 'length').get.call([])",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyDescriptor(T.prototype, 'byteLength').get.call([])",
  "var T = Object.getPrototypeOf(Int8Array); Object.getOwnPropertyDescriptor(T.prototype, 'buffer').get.call({})",
  "var T = Object.getPrototypeOf(Int8Array); T.prototype.toString === Array.prototype.toString",
  "var T = Object.getPrototypeOf(Int8Array); T.prototype[Symbol.iterator] === T.prototype.values",
  "var T = Object.getPrototypeOf(Int8Array); T[Symbol.species] === T",
  "Int8Array.prototype.fill.call([], 1)",
  "Int8Array.prototype.at.call({}, 0)",
  "Int8Array.prototype.map.call(new Uint8Array(1), 1)",
  "Object.getPrototypeOf(Int8Array.prototype).at.call(new DataView(new ArrayBuffer(1)), 0)",
  "Object.getPrototypeOf(Int8Array.prototype).subarray.call([], 0)",
  "Object.getPrototypeOf(Int8Array.prototype).set.call([], [])",
  "Int8Array.prototype.length",
  "Int8Array.prototype.buffer",
  "Int8Array.prototype.byteOffset",
  "Int8Array.prototype.byteLength",
  "Int8Array.prototype.constructor === Int8Array",
  "Int8Array.prototype.toLocaleString.call([1])",
  "Int8Array.prototype.join.call({length: 1, 0: 5})",
  "Int8Array.prototype.toString.call(new Uint8Array([1, 2]))",
  "Int8Array.prototype.entries.call([])",
  "(function () { class MyArr extends Uint8Array {}; var m = new MyArr([1, 2, 3]); return [m.map(function (x) { return x }) instanceof MyArr, m.slice() instanceof MyArr, m.subarray(1) instanceof MyArr, m.filter(function () { return true }) instanceof MyArr, m.toSorted() instanceof MyArr, m.toReversed() instanceof MyArr, m.with(0, 1) instanceof MyArr] })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return new Uint16Array(n) }}; return u.map(function (x) { return 300 }) })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return new Uint8Array(1) }}; return u.slice() })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return new Uint8Array(8) }}; return u.slice().length })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return [] }}; return u.map(function (x) { return x }) })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return new BigInt64Array(n) }}; return u.slice() })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return new BigInt64Array(n) }}; return u.map(function (x) { return 1n }) })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: null}; return u.slice() instanceof Uint8Array })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: 5}; return u.slice() })()",
  "(function () { var u = new Uint8Array(4); u.constructor = 5; return u.slice() })()",
  "(function () { var u = new Uint8Array(4); u.constructor = undefined; return u.slice().length })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return u }}; return u.slice() === u })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (n) { return u }}; return u.subarray(1) === u })()",
  "(function () { var u = new Uint8Array(4); u.constructor = {[Symbol.species]: function (b, o, l) { return new Uint8Array(b, o, 1) }}; return u.subarray(1).length })()",
  "(function () { var u = new Uint8Array(4); var b = u.buffer; b.transfer(); return u.subarray(0) })()",
  "(function () { var log = []; var u = new Uint8Array(4); u.fill(1, {valueOf() { log.push('start'); return 0 }}, {valueOf() { log.push('end'); return 1 }}); return log })()",
  "(function () { var log = []; var u = new Uint8Array(4); u.fill({valueOf() { log.push('value'); return 0 }}, {valueOf() { log.push('start'); return 0 }}, {valueOf() { log.push('end'); return 1 }}); return log })()",
  "(function () { var log = []; var u = new Uint8Array(4); u.set([1], {valueOf() { log.push('offset'); return 0 }}); return log })()",
  "(function () { var log = []; var u = new Uint8Array(4); try { u.set(null, {valueOf() { log.push('offset'); return 0 }}) } catch (e) { log.push(e.name) } return log })()",
  "(function () { var log = []; var u = new Uint8Array(4); try { u.set([1, 2, 3, 4, 5], {valueOf() { log.push('offset'); return 0 }}) } catch (e) { log.push(e.name) } return log })()",
  "(function () { var log = []; var u = new Uint8Array(4); try { u.set([1], {valueOf() { log.push('offset'); return -1 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var u = new Uint8Array(4); try { u.set([1], -1) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var u = new Uint8Array(4); try { u.set([1,2,3,4,5]) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var u = new Uint8Array(4); return u.set({length: 5}) })()",
  "(function () { var u = new Uint8Array(4); var o = {get length() { return 2 }, get 0() { return 1 }, get 1() { return 2 }}; u.set(o); return u })()",
  "(function () { var log = []; var u = new Uint8Array(4); u.set({length: 2, get 0() { log.push(0); return {valueOf() { log.push('v0'); return 1 }} }, get 1() { log.push(1); return 2 }}); return log })()",
  "(function () { var log = []; var u = new Uint8Array(2); u.sort(function (a, b) { log.push([a, b]); return a - b }); return log.length >= 0 })()",
  "(function () { var u = new Uint8Array([3, 1, 2]); try { u.sort(function () { u.buffer.transfer(); return 0 }) } catch (e) { return e.name } return Array.from(u) })()",
  "(function () { var u = new Uint8Array([5, 3, 9, 1]); return u.sort(function (a, b) { return b - a }) })()",
  "(function () { var u = new Uint8Array([3, 1, 2]); u.sort(function () { return 0 }); return Array.from(u) })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var u = new Uint8Array(b); u.set([3,1,2,0,0,0,0,0]); u.sort(function (a, b2) { b.resize(3); return a - b2 }); return Array.from(u) })()",
  "(function () { var r = []; for (var i = 0; i < 30; i++) r.push((i * 7) % 11); var u = new Uint8Array(r); return u.sort(function (a, b) { return a - b }) })()",
  "(function () { var r = []; for (var i = 0; i < 40; i++) r.push((i * 37) % 101); return new Int16Array(r).sort() })()",
  "(function () { var r = []; for (var i = 0; i < 40; i++) r.push(((i * 37) % 101) - 50); return new Int8Array(r).toSorted() })()",
]) {
  add(source);
}

// ---------------------------------------------------------------------------------------------
// DataView.
for (const source of [
  "new DataView(new ArrayBuffer(8)).byteLength",
  "new DataView(new ArrayBuffer(8), 2).byteLength",
  "new DataView(new ArrayBuffer(8), 2, 3).byteLength",
  "new DataView(new ArrayBuffer(8), 8).byteLength",
  "new DataView(new ArrayBuffer(8), 9)",
  "new DataView(new ArrayBuffer(8), 2, 7)",
  "new DataView(new ArrayBuffer(8), -1)",
  "new DataView(new ArrayBuffer(8), 0, -1)",
  "new DataView(new ArrayBuffer(8), undefined, undefined).byteLength",
  "new DataView(new ArrayBuffer(8), '2').byteOffset",
  "new DataView(new ArrayBuffer(8), NaN).byteOffset",
  "new DataView()",
  "new DataView({})",
  "new DataView([])",
  "new DataView(new Uint8Array(4))",
  "new DataView(1)",
  "new DataView(new SharedArrayBuffer(8)).byteLength",
  "DataView(new ArrayBuffer(1))",
  "DataView.length",
  "DataView.name",
  "Object.prototype.toString.call(new DataView(new ArrayBuffer(1)))",
  "Object.getOwnPropertyNames(DataView.prototype).sort()",
  "Object.getOwnPropertyNames(DataView).sort()",
  "DataView.prototype[Symbol.toStringTag]",
  "DataView.prototype.byteLength",
  "DataView.prototype.getInt8.call({}, 0)",
  "DataView.prototype.getInt8.call(new Uint8Array(1), 0)",
  "DataView.prototype.setInt8.call({}, 0, 0)",
  "Object.getOwnPropertyDescriptor(DataView.prototype, 'buffer').get.call({})",
  "Object.getOwnPropertyDescriptor(DataView.prototype, 'byteOffset').get.call({})",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b, 2, 4); return [d.buffer === b, d.byteOffset, d.byteLength] })()",
  "(function () { var d = new DataView(new ArrayBuffer(8)); return [d.getInt8(), d.getInt8(-0), d.getInt8('1'), d.getInt8(1.9), d.getInt8(NaN), d.getInt8(null), d.getInt8(undefined)] })()",
  "new DataView(new ArrayBuffer(8)).getInt8(-1)",
  "new DataView(new ArrayBuffer(8)).getInt8(8)",
  "new DataView(new ArrayBuffer(8)).getInt8(Infinity)",
  "new DataView(new ArrayBuffer(8)).getInt8(2 ** 53)",
  "new DataView(new ArrayBuffer(8)).getInt8(-Infinity)",
  "new DataView(new ArrayBuffer(8)).getInt8(Symbol())",
  "new DataView(new ArrayBuffer(8)).getInt8(1n)",
  "new DataView(new ArrayBuffer(8)).getInt16(7)",
  "new DataView(new ArrayBuffer(8)).getInt32(5)",
  "new DataView(new ArrayBuffer(8)).getFloat64(1)",
  "new DataView(new ArrayBuffer(8)).getBigInt64(1)",
  "new DataView(new ArrayBuffer(8)).getInt16(6)",
  "new DataView(new ArrayBuffer(8), 2, 2).getInt16(1)",
  "new DataView(new ArrayBuffer(8), 2, 2).getInt16(0)",
  "new DataView(new ArrayBuffer(8)).setInt8(8, 1)",
  "new DataView(new ArrayBuffer(8)).setInt8(-1, 1)",
  "new DataView(new ArrayBuffer(8)).setInt16(7, 1)",
  "new DataView(new ArrayBuffer(8)).setInt8()",
  "new DataView(new ArrayBuffer(8)).setInt8(0)",
  "new DataView(new ArrayBuffer(8)).setBigInt64(0, 1)",
  "new DataView(new ArrayBuffer(8)).setBigInt64(0)",
  "new DataView(new ArrayBuffer(8)).setBigUint64(0, '5')",
  "new DataView(new ArrayBuffer(8)).setInt8(0, 1n)",
  "new DataView(new ArrayBuffer(8)).setFloat64(0, 1n)",
  "new DataView(new ArrayBuffer(8)).setInt8(0, Symbol())",
  "new DataView(new ArrayBuffer(8)).setInt8(0, 1)",
  "new DataView(new ArrayBuffer(8)).setInt8.length",
  "new DataView(new ArrayBuffer(8)).setInt16.length",
  "new DataView(new ArrayBuffer(8)).getInt16.length",
  "new DataView(new ArrayBuffer(8)).getBigInt64.length",
  "(function () { var log = []; var d = new DataView(new ArrayBuffer(8)); try { d.setInt8({valueOf() { log.push('idx'); return 99 }}, {valueOf() { log.push('val'); return 1 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var log = []; var d = new DataView(new ArrayBuffer(8)); try { d.setInt8({valueOf() { log.push('idx'); return -1 }}, {valueOf() { log.push('val'); return 1 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var d = new DataView(new ArrayBuffer(8)); try { d.setInt8(-1, Symbol()) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var d = new DataView(new ArrayBuffer(8)); try { d.setInt8(99, Symbol()) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); try { d.setInt8(0, {valueOf() { b.transfer(); return 1 }}) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); try { d.getInt8({valueOf() { b.transfer(); return 0 }}) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); try { d.getInt8(100) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); try { d.setInt8(-1, 0) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); try { d.getInt8(-1) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); try { return d.byteLength } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); var d = new DataView(b); b.transfer(); try { return d.byteOffset } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b); b.resize(12); return [d.byteLength, d.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4); b.resize(12); return [d.byteLength, d.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4); b.resize(2); try { return d.byteLength } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4); b.resize(2); try { return d.byteOffset } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4); b.resize(2); try { return d.getInt8(0) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4, 2); b.resize(5); try { return d.byteLength } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4, 2); b.resize(5); try { return d.getInt8(0) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 4, 2); b.resize(5); b.resize(8); return [d.byteLength, d.byteOffset, d.getInt8(0)] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b); b.resize(0); return [d.byteLength, d.byteOffset] })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b); b.resize(0); try { return d.getInt8(0) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); return new DataView(b, 9) })()",
  "(function () { var b = new ArrayBuffer(8, {maxByteLength: 16}); return new DataView(b, 8).byteLength })()",
  "(function () { var b = new ArrayBuffer(8); b.transfer(); try { return new DataView(b) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); try { return new DataView(b, {valueOf() { b.transfer(); return 0 }}) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); try { return new DataView(b, 0, {valueOf() { b.transfer(); return 0 }}) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { var b = new ArrayBuffer(8); try { return new DataView(b, 100, {valueOf() { throw new SyntaxError('len') }}) } catch (e) { return e.name + ': ' + e.message } })()",
  "(function () { class D extends DataView {}; return Object.getPrototypeOf(new D(new ArrayBuffer(1))) === D.prototype })()",
  "(function () { var d = Reflect.construct(DataView, [new ArrayBuffer(2)], Object); return Object.getPrototypeOf(d) === Object.prototype })()",
  "(function () { var d = Reflect.construct(DataView, [new ArrayBuffer(2)], function () {}.bind()); return typeof d })()",
  "(function () { var nt = function () {}; nt.prototype = null; var d = Reflect.construct(DataView, [new ArrayBuffer(2)], nt); return Object.getPrototypeOf(d) === DataView.prototype })()",
]) {
  add(source);
}

const DV = [
  ["Int8", 1, [0, 127, 128, 255, -1, -129, 1.9]],
  ["Uint8", 1, [0, 255, 256, -1, 1.9]],
  ["Int16", 2, [0, 0x1234, 0x8000, 0xffff, -1, 0x12345]],
  ["Uint16", 2, [0, 0x1234, 0x8000, 0xffff, 0x10000, -1]],
  ["Int32", 4, [0, 0x12345678, 0x80000000, 0xffffffff, -1, 2 ** 32 + 5]],
  ["Uint32", 4, [0, 0x12345678, 0x80000000, 0xffffffff, 2 ** 32, -1]],
  ["Float32", 4, [0, 1.5, -0, 0.1, NaN, Infinity, -Infinity, 1e39, 3.4028235677973366e38]],
  ["Float64", 8, [0, 1.5, -0, 0.1, NaN, Infinity, 5e-324, 1e308]],
  ["BigInt64", 8, ["0n", "1n", "-1n", "2n ** 63n", "2n ** 64n - 1n", "-(2n ** 63n)", "0x0102030405060708n"]],
  ["BigUint64", 8, ["0n", "1n", "-1n", "2n ** 63n", "2n ** 64n - 1n", "2n ** 64n", "0x0102030405060708n"]],
];
for (const [name, size, values] of DV) {
  for (const x of values) {
    const xs = typeof x === "number" ? (Object.is(x, -0) ? "-0" : String(x)) : x;
    // por endianness
    for (const le of ["", ", true", ", false"]) {
      add(`(function () { var d = new DataView(new ArrayBuffer(16)); d.set${name}(1${size === 1 ? "" : ""}, ${xs}${size === 1 ? "" : le}); return [d.get${name}(1${size === 1 ? "" : le}), Array.from(new Uint8Array(d.buffer, 1, ${size}))] })()`);
    }
    add(`(function () { var d = new DataView(new ArrayBuffer(16)); d.set${name}(0, ${xs}, true); return d.get${name}(0, false) })()`);
  }
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); return d.set${name}(0, ${name.startsWith("Big") ? "1n" : "1"}) })()`);
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); return d.get${name}(${16 - size}) })()`);
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); return d.get${name}(${16 - size + 1}) })()`);
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); return d.set${name}(${16 - size + 1}, ${name.startsWith("Big") ? "1n" : "1"}) })()`);
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); d.setUint8(0, 1); return [d.get${name}(0, 1), d.get${name}(0, 0), d.get${name}(0, 'x'), d.get${name}(0, undefined), d.get${name}(0, null)] })()`);
  add(`(function () { var d = new DataView(new ArrayBuffer(16)); return [d.get${name}(0, true), d.get${name}(1, true)] })()`);
  add(`DataView.prototype.get${name}.length + ',' + DataView.prototype.get${name}.name + ',' + DataView.prototype.set${name}.length + ',' + DataView.prototype.set${name}.name`);
}
add("(function () { var d = new DataView(new ArrayBuffer(8)); d.setUint32(0, 0xdeadbeef); return [d.getUint8(0), d.getUint8(3), d.getUint32(0, true), d.getInt32(0), d.getUint16(1), d.getUint16(1, true)] })()");
add("(function () { var d = new DataView(new ArrayBuffer(8)); d.setFloat32(0, 1.5); return Array.from(new Uint8Array(d.buffer)) })()");
add("(function () { var d = new DataView(new ArrayBuffer(8)); d.setFloat64(0, Math.PI, true); return [d.getFloat64(0, true), d.getFloat64(0), d.getUint32(4, true)] })()");
add("(function () { var d = new DataView(new ArrayBuffer(8)); d.setFloat32(0, NaN); return d.getUint32(0).toString(16) })()");
add("(function () { var d = new DataView(new ArrayBuffer(8)); d.setBigInt64(0, -2n); return [d.getBigUint64(0), d.getBigInt64(0, true), d.getUint8(7)] })()");
add("(function () { var d = new DataView(new ArrayBuffer(4)); d.setInt16(0, -2, true); return [d.getUint16(0, true), d.getUint16(0), d.getInt8(0)] })()");
add("(function () { var b = new ArrayBuffer(8); var d = new DataView(b, 2, 4); d.setInt32(0, 0x01020304); return Array.from(new Uint8Array(b)) })()");
add("(function () { var b = new ArrayBuffer(8); var d = new DataView(b, 2, 4); d.setInt32(1, 0x01020304) })()");
add("(function () { var b = new ArrayBuffer(8); var d = new DataView(b); var u = new Uint8Array(b); u[0] = 1; u[1] = 2; return [d.getUint16(0), d.getUint16(0, true)] })()");
add("(function () { var d = new DataView(new SharedArrayBuffer(8)); d.setUint8(0, 5); return d.getUint8(0) })()");

// ---------------------------------------------------------------------------------------------
// Atomics.
const INT = ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "BigInt64Array", "BigUint64Array"];
for (const source of [
  "Object.prototype.toString.call(Atomics)",
  "typeof Atomics",
  "Object.getOwnPropertyNames(Atomics).sort()",
  "Atomics[Symbol.toStringTag]",
  "Atomics()",
  "new Atomics()",
  "Atomics.add.length + ',' + Atomics.and.length + ',' + Atomics.compareExchange.length + ',' + Atomics.exchange.length + ',' + Atomics.isLockFree.length + ',' + Atomics.load.length + ',' + Atomics.notify.length + ',' + Atomics.or.length + ',' + Atomics.store.length + ',' + Atomics.sub.length + ',' + Atomics.wait.length + ',' + Atomics.waitAsync.length + ',' + Atomics.xor.length",
  "typeof Atomics.pause",
  "Atomics.pause.length",
  "Atomics.pause()",
  "Atomics.pause(0)",
  "Atomics.pause(1)",
  "Atomics.pause(1.5)",
  "Atomics.pause(-0)",
  "Atomics.pause('1')",
  "Atomics.pause(NaN)",
  "Atomics.pause(undefined)",
  "Atomics.pause(Infinity)",
  "Atomics.pause(null)",
  "Atomics.isLockFree(1)",
  "Atomics.isLockFree(2)",
  "Atomics.isLockFree(3)",
  "Atomics.isLockFree(4)",
  "Atomics.isLockFree(5)",
  "Atomics.isLockFree(8)",
  "Atomics.isLockFree(16)",
  "Atomics.isLockFree(0)",
  "Atomics.isLockFree(-1)",
  "Atomics.isLockFree('4')",
  "Atomics.isLockFree()",
  "Atomics.isLockFree(1.5)",
  "Atomics.isLockFree(Symbol())",
  "Atomics.isLockFree(4n)",
  "Atomics.add([1], 0, 1)",
  "Atomics.add({}, 0, 1)",
  "Atomics.add(new Float64Array(1), 0, 1)",
  "Atomics.add(new Float32Array(1), 0, 1)",
  "Atomics.add(new Uint8ClampedArray(1), 0, 1)",
  "Atomics.add(new DataView(new ArrayBuffer(4)), 0, 1)",
  "Atomics.add(new ArrayBuffer(4), 0, 1)",
  "Atomics.add(undefined, 0, 1)",
  "Atomics.load(new Float64Array(1), 0)",
  "Atomics.store(new Float64Array(1), 0, 1)",
  "Atomics.compareExchange(new Float64Array(1), 0, 0, 1)",
  "Atomics.exchange(new Float32Array(1), 0, 1)",
  "Atomics.notify(new Float64Array(1), 0, 1)",
  "Atomics.notify(new Uint8Array(1), 0, 1)",
  "Atomics.notify(new Int16Array(1), 0, 1)",
  "Atomics.notify(new Uint32Array(new SharedArrayBuffer(8)), 0, 1)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0, 1)",
  "Atomics.notify(new Int32Array(8), 0, 1)",
  "Atomics.notify(new Int32Array(8), 8, 1)",
  "Atomics.notify(new Int32Array(8), -1, 1)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 2, 1)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0, -1)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0, Infinity)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0, NaN)",
  "Atomics.notify(new Int32Array(new SharedArrayBuffer(8)), 0, 'x')",
  "Atomics.notify(new BigInt64Array(new SharedArrayBuffer(16)), 0, 1)",
  "Atomics.notify(new BigInt64Array(16), 0, 1)",
  "Atomics.notify(new BigUint64Array(16), 0, 1)",
  "Atomics.notify(new Int32Array(8), 0, Symbol())",
  "Atomics.notify(new Int32Array(8), {valueOf() { throw new SyntaxError('i') }}, 1)",
  "Atomics.wait(new Int32Array(8), 0, 0, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 1)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 5)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, -1)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 1, NaN)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, '1')",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, {valueOf() { return 1 }})",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 2, 0, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), -1, 0, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0n, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 1n)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, Symbol(), 0)",
  "Atomics.wait(new Int16Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new Uint32Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new Uint8Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new Float64Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new BigInt64Array(new SharedArrayBuffer(8)), 0, 0n, 0)",
  "Atomics.wait(new BigInt64Array(new SharedArrayBuffer(8)), 0, 1n, 0)",
  "Atomics.wait(new BigInt64Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.wait(new BigUint64Array(new SharedArrayBuffer(8)), 0, 0n, 0)",
  "Atomics.wait(new BigInt64Array(8), 0, 0n, 0)",
  "Atomics.wait([], 0, 0, 0)",
  "Atomics.wait()",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 2 ** 32, 0)",
  "Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 0, Infinity === 1 ? 0 : 0)",
  "(function () { var log = []; try { Atomics.wait(new Int32Array(8), {valueOf() { log.push('idx') ; return 0 }}, {valueOf() { log.push('val'); return 0 }}, {valueOf() { log.push('t'); return 0 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var log = []; try { Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), {valueOf() { log.push('idx') ; return 0 }}, {valueOf() { log.push('val'); return 0 }}, {valueOf() { log.push('t'); return 0 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var log = []; try { Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), {valueOf() { log.push('idx') ; return 5 }}, {valueOf() { log.push('val'); return 0 }}, {valueOf() { log.push('t'); return 0 }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var log = []; try { Atomics.wait(new Int16Array(new SharedArrayBuffer(8)), {valueOf() { log.push('idx') ; return 0 }}, 0, 0) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "(function () { var log = []; try { Atomics.wait(new Int32Array(new SharedArrayBuffer(8, {maxByteLength: 16})), 0, 0, 0) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()",
  "typeof Atomics.waitAsync",
  "Atomics.waitAsync(new Int32Array(8), 0, 0, 0)",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 0)",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 1).async",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 1).async",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 1).value",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 1).value instanceof Promise",
  "Object.keys(Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 1))",
  "Object.keys(Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 1))",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, -1).async",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0).async",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, NaN).async",
  "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 2, 0, 0)",
  "Atomics.waitAsync(new Int16Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.waitAsync(new Float64Array(new SharedArrayBuffer(8)), 0, 0, 0)",
  "Atomics.waitAsync(new BigInt64Array(new SharedArrayBuffer(16)), 0, 0n, 0).value",
  "Atomics.waitAsync(new BigInt64Array(new SharedArrayBuffer(16)), 0, 0, 0)",
  "Atomics.waitAsync([], 0, 0, 0)",
  "Atomics.waitAsync(new Int32Array(8), 0, 0, 1)",
  "(function () { var i = new Int32Array(new SharedArrayBuffer(8)); var r = Atomics.waitAsync(i, 0, 0, 10000); var n = Atomics.notify(i, 0, 1); return [r.async, n] })()",
  "(function () { var i = new Int32Array(new SharedArrayBuffer(8)); Atomics.waitAsync(i, 0, 0, 10000); Atomics.waitAsync(i, 0, 0, 10000); return [Atomics.notify(i, 0, 1), Atomics.notify(i, 0), Atomics.notify(i, 0)] })()",
  "(function () { var i = new Int32Array(new SharedArrayBuffer(8)); Atomics.waitAsync(i, 0, 0, 10000); return [Atomics.notify(i, 1, 1), Atomics.notify(i, 0, 0), Atomics.notify(i, 0, 1)] })()",
]) {
  add(source);
}
for (const t of INT) {
  const big = t.startsWith("Big");
  const n = (x) => (big ? `${x}n` : `${x}`);
  const cases = ["add", "and", "or", "sub", "xor", "exchange"];
  const alt = big ? "(() => { throw 0 })" : "";
  for (const op of cases) {
    add(`(function () { var u = new ${t}(new SharedArrayBuffer(16)); u[0] = ${n(12)}; var r = Atomics.${op}(u, 0, ${n(10)}); return [r, u[0]] })()`);
    add(`(function () { var u = new ${t}(4); u[0] = ${n(12)}; var r = Atomics.${op}(u, 0, ${n(10)}); return [r, u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, 2, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, -1, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, 0) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, 0, ${big ? "1" : "1n"}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, 0, Symbol()) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, 1.9, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, '1', ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, undefined, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, NaN, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, Infinity, ${n(1)}) })()`);
    add(`(function () { var u = new ${t}(2); return Atomics.${op}(u, {valueOf() { return 1 }}, ${n(1)}) })()`);
    add(`(function () { var log = []; var u = new ${t}(2); try { Atomics.${op}(u, {valueOf() { log.push('i'); return 5 }}, {valueOf() { log.push('v'); return ${n(1)} }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()`);
    add(`(function () { var log = []; var u = new ${t}(2); try { Atomics.${op}(u, {valueOf() { log.push('i'); return 0 }}, {valueOf() { log.push('v'); return ${n(1)} }}) } catch (e) { log.push(e.name + ': ' + e.message) } return log })()`);
  }
  add(`(function () { var u = new ${t}(2); u[0] = ${n(5)}; return [Atomics.compareExchange(u, 0, ${n(5)}, ${n(9)}), u[0], Atomics.compareExchange(u, 0, ${n(5)}, ${n(1)}), u[0]] })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.compareExchange(u, 2, ${n(0)}, ${n(1)}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.compareExchange(u, 0, ${n(0)}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.compareExchange(u, 0) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.compareExchange(u, 0, ${big ? "0" : "0n"}, ${n(1)}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.compareExchange(u, 0, ${n(0)}, ${big ? "1" : "1n"}) })()`);
  add(`(function () { var log = []; var u = new ${t}(2); try { Atomics.compareExchange(u, {valueOf() { log.push('i'); return 5 }}, {valueOf() { log.push('e'); return ${n(1)} }}, {valueOf() { log.push('r'); return ${n(1)} }}) } catch (e) { log.push(e.name) } return log })()`);
  add(`(function () { var u = new ${t}(2); u[1] = ${n(7)}; return [Atomics.load(u, 1), Atomics.load(u, 0)] })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.load(u, 2) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.load(u) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.load(u, 'x') })()`);
  add(`(function () { var u = new ${t}(2); return [Atomics.store(u, 0, ${n(5)}), u[0]] })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "'7'" : "5.7"}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "1" : "1n"}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 2, ${n(1)}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "2n ** 70n + 3n" : "2 ** 40 + 3"}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "-5n" : "-5"}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "{valueOf() { return 3n }}" : "{valueOf() { return 3.9 }}"}) })()`);
  add(`(function () { var u = new ${t}(2); return Atomics.store(u, 0, ${big ? "Object(5n)" : "NaN"}) })()`);
  if (!big) {
    add(`(function () { var u = new ${t}(2); return [Atomics.store(u, 0, -0), Object.is(Atomics.store(u, 0, -0), 0)] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.store(u, 0, Infinity), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.store(u, 0, '3'), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.add(u, 0, 255), Atomics.add(u, 0, 2), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.sub(u, 0, 1), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.and(u, 0, -1), Atomics.or(u, 0, -1), Atomics.xor(u, 0, 1), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.exchange(u, 0, 2 ** 33 + 2), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); u[0] = -1; return Atomics.compareExchange(u, 0, -1, 3) })()`);
    add(`(function () { var u = new ${t}(2); u[0] = -1; return Atomics.compareExchange(u, 0, ${t === "Int8Array" ? 255 : t === "Int16Array" ? 65535 : t === "Int32Array" ? 4294967295 : 0}, 3) })()`);
  } else {
    add(`(function () { var u = new ${t}(2); return [Atomics.add(u, 0, 2n ** 63n), Atomics.add(u, 0, 2n ** 63n), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.sub(u, 0, 1n), u[0]] })()`);
    add(`(function () { var u = new ${t}(2); return [Atomics.and(u, 0, -1n), Atomics.or(u, 0, -1n), Atomics.xor(u, 0, 1n), u[0]] })()`);
  }
  add(`Atomics.notify(new ${t}(4), 0, 1)`);
  add(`Atomics.notify(new ${t}(new SharedArrayBuffer(16)), 0, 1)`);
  add(`Atomics.wait(new ${t}(new SharedArrayBuffer(16)), 0, ${n(0)}, 0)`);
  add(`Atomics.wait(new ${t}(4), 0, ${n(0)}, 0)`);
  add(`Atomics.waitAsync(new ${t}(4), 0, ${n(0)}, 0)`);
  add(`Atomics.waitAsync(new ${t}(new SharedArrayBuffer(16)), 0, ${n(0)}, 0)`);
  add(`(function () { var b = new ArrayBuffer(16, {maxByteLength: 32}); var u = new ${t}(b); b.resize(0); return Atomics.load(u, 0) })()`);
  add(`(function () { var b = new ArrayBuffer(16); var u = new ${t}(b); b.transfer(); return Atomics.load(u, 0) })()`);
  add(`(function () { var b = new ArrayBuffer(16); var u = new ${t}(b); b.transfer(); return Atomics.store(u, 0, ${n(1)}) })()`);
  add(`(function () { var b = new ArrayBuffer(16); var u = new ${t}(b); try { Atomics.store(u, 0, {valueOf() { b.transfer(); return ${n(1)} }}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`(function () { var b = new ArrayBuffer(16); var u = new ${t}(b); try { Atomics.add(u, {valueOf() { b.transfer(); return 0 }}, ${n(1)}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`(function () { var b = new ArrayBuffer(16); var u = new ${t}(b); try { Atomics.compareExchange(u, 0, ${n(0)}, {valueOf() { b.transfer(); return ${n(1)} }}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`(function () { var b = new ArrayBuffer(16, {maxByteLength: 32}); var u = new ${t}(b); try { return Atomics.add(u, ${big ? 1 : 3}, ${n(1)}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`(function () { var b = new ArrayBuffer(16, {maxByteLength: 32}); var u = new ${t}(b); try { return Atomics.add(u, {valueOf() { b.resize(0); return 0 }}, ${n(1)}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`(function () { var b = new SharedArrayBuffer(16, {maxByteLength: 32}); var u = new ${t}(b); b.grow(32); return [u.length, Atomics.add(u, u.length - 1, ${n(1)})] })()`);
  add(`(function () { var b = new SharedArrayBuffer(16, {maxByteLength: 32}); var u = new ${t}(b); try { return Atomics.add(u, u.length, ${n(1)}) } catch (e) { return e.name + ': ' + e.message } })()`);
  add(`Atomics.isLockFree(${new Map([["Int8Array", 1], ["Uint8Array", 1], ["Int16Array", 2], ["Uint16Array", 2], ["Int32Array", 4], ["Uint32Array", 4], ["BigInt64Array", 8], ["BigUint64Array", 8]]).get(t)})`);
}
for (const t of ["Float32Array", "Float64Array", "Uint8ClampedArray"]) {
  for (const op of ["add", "and", "compareExchange", "exchange", "load", "notify", "or", "store", "sub", "xor", "wait", "waitAsync"]) {
    add(`Atomics.${op}(new ${t}(new SharedArrayBuffer(16)), 0, 0, 0)`);
  }
}

// ---------------------------------------------------------------------------------------------
const seen = new Set();
const lines = [];
for (const src of programs) {
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
if (lines.length < 500) throw new Error(`só ${lines.length} programas`);
fs.writeSync(1, lines.join("\n") + "\n");
process.exit(0);
