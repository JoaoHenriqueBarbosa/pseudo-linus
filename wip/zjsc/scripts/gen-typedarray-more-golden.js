// Gera tests/golden/typedarray_more_bun.tsv: programas de uma linha sobre ArrayBuffer redimensionável,
// SharedArrayBuffer growable, TypedArray (length-tracking, fixos, fora da faixa), DataView, métodos de
// TypedArray com NaN/-0/Infinity e ordem de coerção, Float16, conversões e base64/hex, avaliados no bun.
// Colunas e serialização iguais às de scripts/gen-buffers-golden.js (harness tests/golden/e2e_values_harness.js).
// Uso: bun scripts/gen-typedarray-more-golden.js > tests/golden/typedarray_more_bun.tsv
const fs = require("fs");
const { sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const programs = [];
const add = (source) => programs.push(source);
// Corpo de função com try/catch que devolve "Nome: mensagem" (mantém o resultado parcial legível).
const fn = (body) => `(function () { ${body} })()`;
const tc = (body) => fn(`try { ${body} } catch (e) { return e.name + ': ' + e.message }`);

const TYPES = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
const NUM = TYPES.filter((t) => !t.startsWith("Big"));
const BIG = TYPES.filter((t) => t.startsWith("Big"));
const FLOATS = ["Float32Array", "Float64Array", "Float16Array"];
const isBig = (t) => t.startsWith("Big");
const es = (t) => ({ Int8Array: 1, Uint8Array: 1, Uint8ClampedArray: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, Float16Array: 2, Float32Array: 4, Float64Array: 8, BigInt64Array: 8, BigUint64Array: 8 })[t];
const lit = (t, values) => values.map((v) => (isBig(t) ? `${v}n` : `${v}`)).join(",");

// ---------------------------------------------------------------------------------------------
// Length-tracking e fixos sobre ArrayBuffer redimensionável.
for (const t of TYPES) {
  const s = es(t);
  const mk = `var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}});`;
  for (const [name, ctor] of [["track0", `new ${t}(b)`], ["trackOff", `new ${t}(b, ${2 * s})`], ["fixed", `new ${t}(b, 0, 4)`], ["fixedOff", `new ${t}(b, ${s}, 3)`]]) {
    for (const size of [0, 1, 2, 4, 6, 8, 16, 32]) {
      add(fn(`${mk} var u = ${ctor}; b.resize(${size * s}); return [u.length, u.byteLength, u.byteOffset, b.byteLength]`));
    }
    add(fn(`${mk} var u = ${ctor}; b.resize(0); b.resize(${16 * s}); return [u.length, u.byteLength, u.byteOffset]`));
    add(fn(`${mk} var u = ${ctor}; b.resize(${3 * s}); return [Object.keys(u).length, 0 in u, 1 in u, 3 in u, u[0], u[100]]`));
    add(fn(`${mk} var u = ${ctor}; b.resize(${3 * s}); return Object.getOwnPropertyNames(u)`));
    add(fn(`${mk} var u = ${ctor}; b.transfer(); return [u.length, u.byteLength, u.byteOffset, u[0]]`));
    add(fn(`${mk} var u = ${ctor}; b.resize(${3 * s}); u[0] = ${isBig(t) ? "1n" : "1"}; u[5] = ${isBig(t) ? "1n" : "1"}; return [u.length, u[0], u[5]]`));
  }
  add(fn(`${mk} var u = new ${t}(b); b.resize(${20 * s}); u.fill(${isBig(t) ? "7n" : "7"}); return u.length + ':' + Array.prototype.join.call(u, ',')`));
  add(fn(`${mk} var u = new ${t}(b, 0, 4); b.resize(${2 * s}); b.resize(${8 * s}); return [u.length, u.byteLength]`));
  add(fn(`${mk} var u = new ${t}(b, ${s}, 3); b.resize(${3 * s}); return u.length`));
  add(fn(`${mk} var u = new ${t}(b, ${s}, 3); b.resize(${4 * s}); return u.length`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); return new ${t}(b, 0, 40).length`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); return new ${t}(b, ${9 * s}).length`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); b.resize(${2 * s}); return new ${t}(b, ${3 * s}).length`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); var u = new ${t}(b, 0, 4); b.resize(${2 * s}); return new ${t}(u).length`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); var u = new ${t}(b); b.resize(${2 * s}); var c = new ${t}(u); return [c.length, c.buffer.resizable]`));
  add(tc(`var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${32 * s}}); var u = new ${t}(b); return [u.buffer === b, u.length, new ${t}(u).buffer === b]`));
}

// Métodos em array fora da faixa (OOB) ou desanexado: mensagem exata do TypeError.
const noArg = ["entries", "keys", "values", "reverse", "toReversed", "toSorted", "toLocaleString", "toString", "sort"];
const withFn = ["every", "filter", "find", "findIndex", "findLast", "findLastIndex", "forEach", "map", "some", "reduce", "reduceRight"];
const withVal = ["at", "includes", "indexOf", "lastIndexOf", "join", "fill", "copyWithin", "slice", "subarray", "set", "with"];
for (const t of ["Uint8Array", "Int16Array", "Float32Array", "Float64Array", "BigInt64Array", "Uint8ClampedArray"]) {
  const s = es(t);
  const v = isBig(t) ? "1n" : "1";
  for (const mode of ["oob", "detached", "oobTrack"]) {
    const setup =
      mode === "oob" ? `var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${16 * s}}); var u = new ${t}(b, 0, 4); b.resize(${2 * s});`
      : mode === "oobTrack" ? `var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${16 * s}}); var u = new ${t}(b, ${4 * s}); b.resize(${2 * s});`
      : `var b = new ArrayBuffer(${8 * s}); var u = new ${t}(b); b.transfer();`;
    for (const m of noArg) add(tc(`${setup} return u.${m}()`));
    for (const m of withFn) add(tc(`${setup} return u.${m}(function (x) { return x })`));
    for (const m of withVal) add(tc(`${setup} return u.${m}(${m === "set" ? "[1]" : m === "with" ? `0, ${v}` : m === "copyWithin" ? "0, 1" : m === "fill" ? v : v})`));
    add(tc(`${setup} return u[Symbol.iterator]().next()`));
    add(tc(`${setup} return [...u]`));
    add(tc(`${setup} return Array.from(u)`));
    add(tc(`${setup} return new ${t}(u)`));
    add(tc(`${setup} return ${t}.from(u)`));
    add(tc(`${setup} var d = new ${t}(4); d.set(u); return d`));
    add(tc(`${setup} return Array.prototype.slice.call(u)`));
    add(tc(`${setup} return Object.entries(u)`));
    add(tc(`${setup} return JSON.stringify(u)`));
    add(tc(`${setup} return [u.length, u.byteLength, u.byteOffset, u.buffer === b]`));
    add(tc(`${setup} return Object.getOwnPropertyDescriptor(u, 0)`));
    add(tc(`${setup} return Reflect.defineProperty(u, 0, {value: ${v}})`));
    add(tc(`${setup} return Reflect.set(u, 0, ${v})`));
    add(tc(`${setup} return Reflect.has(u, 0)`));
    add(tc(`${setup} return Reflect.ownKeys(u)`));
  }
}

// Iteração durante resize.
for (const t of ["Uint8Array", "Int32Array", "Float64Array"]) {
  const s = es(t);
  const mk = `var b = new ArrayBuffer(${8 * s}, {maxByteLength: ${16 * s}}); var u = new ${t}(b); var out = [];`;
  for (const [name, newSize] of [["shrink", 3], ["grow", 12], ["zero", 0]]) {
    add(fn(`${mk} for (var x of u) { out.push(x); if (out.length === 2) b.resize(${newSize * s}) } return out`));
    add(fn(`${mk} for (var x of u.entries()) { out.push(x[0]); if (out.length === 2) b.resize(${newSize * s}) } return out`));
    add(fn(`${mk} for (var x of u.keys()) { out.push(x); if (out.length === 2) b.resize(${newSize * s}) } return out`));
    add(fn(`${mk} u.forEach(function (x, i) { out.push(i); if (i === 1) b.resize(${newSize * s}) }); return out`));
    add(fn(`${mk} return [u.map(function (x, i) { if (i === 1) b.resize(${newSize * s}); return i }), u.length]`));
    add(fn(`${mk} u.fill(1); return u.filter(function (x, i) { if (i === 1) b.resize(${newSize * s}); return true })`));
    add(fn(`${mk} u.fill(1); return [u.reduce(function (a, x, i) { if (i === 1) b.resize(${newSize * s}); return a + (x === undefined ? 100 : 1) }, 0)]`));
    add(fn(`${mk} u.fill(1); return [u.every(function (x, i) { if (i === 1) b.resize(${newSize * s}); return true })]`));
    add(fn(`${mk} u.fill(1); return [u.findLast(function (x, i) { if (i === 7) b.resize(${newSize * s}); out.push(x); return false }), out]`));
    add(fn(`${mk} u.fill(1); return [u.find(function (x, i) { if (i === 1) b.resize(${newSize * s}); out.push(x); return false }), out]`));
    add(fn(`${mk} u.fill(1); return [u.some(function (x, i) { if (i === 1) b.resize(${newSize * s}); out.push(x); return false }), out]`));
    add(tc(`${mk} u.fill(2); return u.sort(function (x, y) { b.resize(${newSize * s}); return x - y })`));
    add(tc(`${mk} u.fill(2); return u.toSorted(function (x, y) { b.resize(${newSize * s}); return x - y })`));
    add(tc(`${mk} u.fill(2); return u.slice({valueOf() { b.resize(${newSize * s}); return 0 }})`));
    add(tc(`${mk} u.fill(2); return u.slice(0, {valueOf() { b.resize(${newSize * s}); return 8 }})`));
    add(tc(`${mk} u.fill(2); return u.subarray({valueOf() { b.resize(${newSize * s}); return 0 }}).length`));
    add(tc(`${mk} u.fill(2); return u.fill(5, {valueOf() { b.resize(${newSize * s}); return 0 }})`));
    add(tc(`${mk} u.fill(2); return u.fill({valueOf() { b.resize(${newSize * s}); return 5 }})`));
    add(tc(`${mk} u.fill(2); return u.copyWithin(0, {valueOf() { b.resize(${newSize * s}); return 4 }})`));
    add(tc(`${mk} u.fill(2); return u.includes(2, {valueOf() { b.resize(${newSize * s}); return 0 }})`));
    add(tc(`${mk} u.fill(2); return u.includes(undefined, {valueOf() { b.resize(${newSize * s}); return 0 }})`));
    add(tc(`${mk} u.fill(2); return u.indexOf(2, {valueOf() { b.resize(${newSize * s}); return 5 }})`));
    add(tc(`${mk} u.fill(2); return u.lastIndexOf(2, {valueOf() { b.resize(${newSize * s}); return 7 }})`));
    add(tc(`${mk} u.fill(2); return u.join({toString() { b.resize(${newSize * s}); return '-' }})`));
    add(tc(`${mk} u.fill(2); return u.at({valueOf() { b.resize(${newSize * s}); return 6 }})`));
    add(tc(`${mk} u.fill(2); return u.with({valueOf() { b.resize(${newSize * s}); return 6 }}, 9)`));
    add(tc(`${mk} u.fill(2); return u.with(6, {valueOf() { b.resize(${newSize * s}); return 9 }})`));
    add(tc(`${mk} u.fill(2); var d = new ${t}(8); d.set(u, {valueOf() { b.resize(${newSize * s}); return 0 }}); return d`));
    add(tc(`${mk} u.fill(2); return u.set([1, 2], {valueOf() { b.resize(${newSize * s}); return 6 }})`));
    add(tc(`${mk} u.fill(2); return u.set([{valueOf() { b.resize(${newSize * s}); return 1 }}, 2], 6)`));
  }
}

// ArrayBuffer.prototype.{slice,transfer,...} com resize durante coerção.
for (const [name, body] of [
  ["slice start", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.slice({valueOf() { b.resize(2); return 0 }}).byteLength`],
  ["slice end", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.slice(0, {valueOf() { b.resize(2); return 8 }}).byteLength`],
  ["slice neg", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.slice(-4).byteLength`],
  ["slice species resizable", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var s = b.slice(2, 6); return [s.resizable, s.byteLength, s.maxByteLength]`],
  ["slice detach", `var b = new ArrayBuffer(8); return b.slice({valueOf() { b.transfer(); return 0 }})`],
  ["transfer newLength coercion", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.transfer({valueOf() { b.resize(2); return 4 }}).byteLength`],
  ["transfer detach coercion", `var b = new ArrayBuffer(8); return b.transfer({valueOf() { b.transfer(); return 4 }}).byteLength`],
  ["transfer 0", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var c = b.transfer(0); return [c.byteLength, c.resizable, c.maxByteLength]`],
  ["transfer same max", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var c = b.transfer(16); return [c.byteLength, c.resizable, c.maxByteLength]`],
  ["transfer over max", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.transfer(17)`],
  ["transferToFixed over", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.transferToFixedLength(100).byteLength`],
  ["transfer length", `return [ArrayBuffer.prototype.transfer.length, ArrayBuffer.prototype.transferToFixedLength.length, ArrayBuffer.prototype.resize.length, ArrayBuffer.prototype.slice.length]`],
  ["detached getter", `return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get.call(new SharedArrayBuffer(1))`],
  ["detached getter obj", `return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get.call({})`],
  ["resizable getter", `return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'resizable').get.call(new SharedArrayBuffer(1))`],
  ["maxByteLength getter", `return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'maxByteLength').get.call(1)`],
  ["maxByteLength option obj", `return new ArrayBuffer(1, {maxByteLength: {valueOf() { return 4 }}}).maxByteLength`],
  ["maxByteLength option big", `return new ArrayBuffer(1, {maxByteLength: 2 ** 53})`],
  ["maxByteLength huge", `return new ArrayBuffer(1, {maxByteLength: 2 ** 60})`],
  ["maxByteLength NaN", `return new ArrayBuffer(1, {maxByteLength: NaN}).resizable`],
  ["maxByteLength null", `return new ArrayBuffer(1, {maxByteLength: null})`],
  ["maxByteLength string", `return new ArrayBuffer(1, {maxByteLength: '4'}).maxByteLength`],
  ["maxByteLength order", `var l = []; try { new ArrayBuffer({valueOf() { l.push('len'); return 1 }}, {get maxByteLength() { l.push('max'); return 4 }}) } catch (e) {} return l`],
  ["resize ret", `return new ArrayBuffer(1, {maxByteLength: 4}).resize(2)`],
  ["resize coercion detach", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.resize({valueOf() { b.transfer(); return 4 }})`],
  ["resize coercion shrink", `var b = new ArrayBuffer(8, {maxByteLength: 16}); b.resize({valueOf() { b.resize(1); return 4 }}); return b.byteLength`],
  ["tostring", `return [Object.prototype.toString.call(new ArrayBuffer(1, {maxByteLength: 2})), String(new SharedArrayBuffer(1))]`],
]) add(tc(body));

// Prototype de ArrayBuffer sobre resizable: slice de SAB e ArrayBuffer com species.
add(tc(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new ArrayBuffer(2) }}; return b.slice(0, 4)`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return b }}; return b.slice(0, 4)`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new ArrayBuffer(n) }}; return b.slice(0, 4).byteLength`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return {} }}; return b.slice(0, 4)`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new SharedArrayBuffer(n) }}; return b.slice(0, 4)`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = 1; return b.slice(0, 4)`));
add(tc(`var b = new ArrayBuffer(8); b.constructor = undefined; return b.slice(0, 4).byteLength`));

// ---------------------------------------------------------------------------------------------
// SharedArrayBuffer growable.
for (const t of TYPES) {
  const s = es(t);
  const mk = `var b = new SharedArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}});`;
  add(fn(`${mk} var u = new ${t}(b); b.grow(${8 * s}); return [u.length, u.byteLength, b.byteLength, b.growable, b.maxByteLength]`));
  add(fn(`${mk} var u = new ${t}(b, 0, 4); b.grow(${8 * s}); return [u.length, u.byteLength]`));
  add(fn(`${mk} var u = new ${t}(b, ${s}); b.grow(${16 * s}); return [u.length, u.byteLength, u.byteOffset]`));
  add(tc(`${mk} var u = new ${t}(b); b.grow(${2 * s}); return u.length`));
  add(tc(`${mk} b.grow(${16 * s + 1}); return b.byteLength`));
  add(tc(`${mk} b.grow(${4 * s}); return b.byteLength`));
  add(tc(`${mk} b.grow(${4 * s}); b.grow(${4 * s}); return b.byteLength`));
  add(tc(`${mk} return new ${t}(b, 0, 5).length`));
  add(tc(`${mk} var u = new ${t}(b); return [Object.keys(u).length, u.toString(), u.join('|')]`));
  add(tc(`${mk} var u = new ${t}(b); b.grow(${6 * s}); return u.fill(${isBig(t) ? "3n" : "3"}).join()`));
  add(tc(`${mk} var u = new ${t}(b); b.grow(${6 * s}); var q = u.slice(1, 5); return [q.length, q.buffer instanceof SharedArrayBuffer, q.buffer.growable]`));
  add(tc(`${mk} var u = new ${t}(b); var q = u.subarray(1); b.grow(${8 * s}); return [q.length, u.length]`));
  add(tc(`${mk} var u = new ${t}(b); var q = u.subarray(1, 3); b.grow(${8 * s}); return [q.length, u.length]`));
  add(tc(`${mk} var u = new ${t}(b); var q = u.subarray(1, -1); b.grow(${8 * s}); return [q.length, u.length]`));
  add(tc(`${mk} var u = new ${t}(b); var q = u.subarray(-2); b.grow(${8 * s}); return [q.length, u.length]`));
  add(tc(`${mk} return [b.slice(1).byteLength, b.slice(1).growable, b.slice(0, 2).maxByteLength]`));
}
for (const [name, body] of [
  ["growable prop", `return [new SharedArrayBuffer(1).growable, new SharedArrayBuffer(1, {maxByteLength: 2}).growable, new SharedArrayBuffer(1, {maxByteLength: 1}).growable]`],
  ["max prop", `return [new SharedArrayBuffer(1).maxByteLength, new SharedArrayBuffer(1, {maxByteLength: 9}).maxByteLength]`],
  ["max below", `return new SharedArrayBuffer(4, {maxByteLength: 2})`],
  ["max neg", `return new SharedArrayBuffer(4, {maxByteLength: -1})`],
  ["max huge", `return new SharedArrayBuffer(1, {maxByteLength: 2 ** 53})`],
  ["grow ret", `return new SharedArrayBuffer(1, {maxByteLength: 4}).grow(2)`],
  ["grow fixed", `return new SharedArrayBuffer(1).grow(2)`],
  ["grow ab", `return SharedArrayBuffer.prototype.grow.call(new ArrayBuffer(1, {maxByteLength: 2}), 2)`],
  ["grow neg", `return new SharedArrayBuffer(1, {maxByteLength: 4}).grow(-1)`],
  ["grow nan", `var b = new SharedArrayBuffer(1, {maxByteLength: 4}); b.grow(NaN); return b.byteLength`],
  ["grow str", `var b = new SharedArrayBuffer(1, {maxByteLength: 4}); b.grow('3'); return b.byteLength`],
  ["grow undefined", `var b = new SharedArrayBuffer(1, {maxByteLength: 4}); b.grow(); return b.byteLength`],
  ["grow coercion", `var b = new SharedArrayBuffer(1, {maxByteLength: 4}); b.grow({valueOf() { b.grow(3); return 2 }}); return b.byteLength`],
  ["grow length", `return [SharedArrayBuffer.prototype.grow.length, SharedArrayBuffer.length]`],
  ["growable getter ab", `return Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'growable').get.call(new ArrayBuffer(1))`],
  ["growable getter", `return Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'growable').get.call({})`],
  ["tag", `return Object.prototype.toString.call(new SharedArrayBuffer(1, {maxByteLength: 2}))`],
  ["slice sab", `var b = new SharedArrayBuffer(8, {maxByteLength: 16}); return b.slice(2, 6).byteLength`],
  ["slice detach resize", `var b = new SharedArrayBuffer(8, {maxByteLength: 16}); return b.slice({valueOf() { b.grow(16); return 0 }}).byteLength`],
  ["dv grow", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var d = new DataView(b); b.grow(8); return [d.byteLength, d.getInt8(7)]`],
  ["dv grow fixed", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var d = new DataView(b, 0, 4); b.grow(8); return [d.byteLength]`],
  ["dv off", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var d = new DataView(b, 2); b.grow(8); return [d.byteLength, d.byteOffset]`],
  ["atomics grow", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var u = new Int8Array(b); b.grow(8); return [Atomics.add(u, 7, 1), Atomics.load(u, 7)]`],
  ["atomics wait grow", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var u = new Int32Array(b); b.grow(8); return Atomics.wait(u, 1, 1, 0)`],
  ["atomics notify grow", `var b = new SharedArrayBuffer(4, {maxByteLength: 8}); var u = new Int32Array(b); b.grow(8); return Atomics.notify(u, 1, 1)`],
]) add(tc(body));

// ---------------------------------------------------------------------------------------------
// DataView sobre buffers redimensionáveis.
const DVG = ["Int8", "Uint8", "Int16", "Uint16", "Int32", "Uint32", "Float32", "Float64", "BigInt64", "BigUint64", "Float16"];
const dvs = (g) => ({ Int8: 1, Uint8: 1, Int16: 2, Uint16: 2, Int32: 4, Uint32: 4, Float16: 2, Float32: 4, Float64: 8, BigInt64: 8, BigUint64: 8 })[g];
for (const [name, ctor] of [["track", "new DataView(b)"], ["trackOff", "new DataView(b, 4)"], ["fixed", "new DataView(b, 0, 8)"], ["fixedOff", "new DataView(b, 4, 4)"]]) {
  const mk = `var b = new ArrayBuffer(16, {maxByteLength: 32}); var d = ${ctor};`;
  for (const size of [0, 3, 4, 8, 12, 16, 32]) {
    add(tc(`${mk} b.resize(${size}); return [d.byteLength, d.byteOffset]`));
    add(tc(`${mk} b.resize(${size}); return d.getInt8(0)`));
    add(tc(`${mk} b.resize(${size}); d.setInt8(0, 5); return d.getInt8(0)`));
  }
  add(tc(`${mk} b.transfer(); return d.byteLength`));
  add(tc(`${mk} b.transfer(); return d.byteOffset`));
  add(tc(`${mk} b.transfer(); return d.getInt8(0)`));
  add(tc(`${mk} b.transfer(); return d.buffer.byteLength`));
  add(tc(`${mk} b.resize(2); return d.buffer === b`));
  add(tc(`${mk} b.resize(2); b.resize(16); return [d.byteLength, d.byteOffset]`));
  for (const g of DVG) {
    const v = g.startsWith("Big") ? "1n" : "1";
    add(tc(`${mk} b.resize(${dvs(g) + 3}); return d.get${g}(0)`));
    add(tc(`${mk} b.resize(${dvs(g) + 3}); return d.set${g}(0, ${v})`));
    add(tc(`${mk} b.resize(2); return d.get${g}(0)`));
    add(tc(`${mk} b.resize(2); return d.set${g}(0, ${v})`));
    add(tc(`${mk} b.resize(2); return d.get${g}({valueOf() { b.resize(32); return 0 }})`));
    add(tc(`${mk} return d.set${g}(0, {valueOf() { b.resize(2); return ${v} }})`));
    add(tc(`${mk} return d.set${g}(0, {valueOf() { b.transfer(); return ${v} }})`));
    add(tc(`${mk} return d.get${g}({valueOf() { b.transfer(); return 0 }})`));
  }
}
for (const [name, body] of [
  ["ctor off oob", `var b = new ArrayBuffer(4, {maxByteLength: 8}); return new DataView(b, 5)`],
  ["ctor len oob", `var b = new ArrayBuffer(4, {maxByteLength: 8}); return new DataView(b, 0, 5)`],
  ["ctor detached", `var b = new ArrayBuffer(4); b.transfer(); return new DataView(b)`],
  ["ctor off coercion detach", `var b = new ArrayBuffer(4); return new DataView(b, {valueOf() { b.transfer(); return 0 }})`],
  ["ctor len coercion shrink", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return new DataView(b, 0, {valueOf() { b.resize(2); return 4 }})`],
  ["ctor off coercion shrink", `var b = new ArrayBuffer(8, {maxByteLength: 16}); return new DataView(b, {valueOf() { b.resize(2); return 4 }}).byteLength`],
  ["ctor len undefined", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 2, undefined); b.resize(12); return d.byteLength`],
  ["ctor len 0", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 2, 0); b.resize(12); return d.byteLength`],
  ["ctor off len", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 8); b.resize(4); return [d.byteLength, d.byteOffset]`],
  ["ctor off len ok", `var b = new ArrayBuffer(8, {maxByteLength: 16}); var d = new DataView(b, 8); b.resize(12); return [d.byteLength, d.byteOffset]`],
  ["ctor no buffer", `return new DataView({})`],
  ["ctor no new", `return DataView(new ArrayBuffer(1))`],
  ["ctor sab", `return new DataView(new SharedArrayBuffer(4)).byteLength`],
  ["tag", `return Object.prototype.toString.call(new DataView(new ArrayBuffer(1)))`],
  ["proto order", `var l = []; var nt = new Proxy(function () {}, {get(t, k) { l.push(String(k)); return t[k] }}); try { Reflect.construct(DataView, [new ArrayBuffer(1), 5], nt) } catch (e) { l.push(e.name) } return l`],
  ["proto order ok", `var l = []; var nt = new Proxy(function () {}, {get(t, k) { l.push(String(k)); return t[k] }}); Reflect.construct(DataView, [new ArrayBuffer(1)], nt); return l`],
  ["proto order detached", `var b = new ArrayBuffer(4); var nt = new Proxy(function () {}, {get(t, k) { b.transfer(); return t[k] }}); return Reflect.construct(DataView, [b], nt)`],
  ["proto order shrink", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var nt = new Proxy(function () {}, {get(t, k) { b.resize(1); return t[k] }}); return Reflect.construct(DataView, [b, 0, 4], nt)`],
]) add(tc(body));
for (const g of ["Int8", "Int16", "Uint32", "Float32", "Float64", "BigInt64", "Float16"]) {
  const s = dvs(g);
  for (const idx of ["-1", "-0", "0.5", "1.9", "NaN", "Infinity", "'1'", "undefined", "null", "true", "2 ** 53", "2 ** 32"]) {
    add(tc(`return new DataView(new ArrayBuffer(16)).get${g}(${idx})`));
  }
  for (const le of ["true", "false", "undefined", "1", "0", "'x'", "null", "''"]) {
    add(tc(`var d = new DataView(new ArrayBuffer(16)); d.set${g}(0, ${g.startsWith("Big") ? "258n" : "258"}, ${le}); return [d.get${g}(0, ${le}), d.get${g}(0, !${le}), new Uint8Array(d.buffer, 0, ${s}).join()]`));
  }
}

// ---------------------------------------------------------------------------------------------
// Métodos de TypedArray com NaN, -0 e Infinity.
const SPECIAL = ["NaN", "-0", "0", "Infinity", "-Infinity", "1.5", "-1.5", "0.5", "2.5", "3.5", "1e300", "1e-300", "65504", "65520", "70000", "6e-8", "3e-8", "2.9802322387695312e-8", "5e-324"];
for (const t of FLOATS) {
  for (const v of SPECIAL) {
    add(tc(`var u = new ${t}(3); u.fill(${v}); return [u[0], Object.is(u[1], -0), u.includes(${v}), u.indexOf(${v}), u.lastIndexOf(${v}), u.join(), u.at(-1)]`));
    add(tc(`var u = new ${t}([${v}, 1, ${v}]); return [u.includes(NaN), u.indexOf(NaN), u.includes(-0), u.includes(0), u.indexOf(0), u.indexOf(-0), u.lastIndexOf(-0)]`));
    add(tc(`var u = new ${t}([3, ${v}, -1, 0, -0]); u.sort(); return Array.from(u).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join()`));
    add(tc(`var u = new ${t}([3, ${v}, -1, 0, -0]); return Array.from(u.toSorted()).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join()`));
    add(tc(`var u = new ${t}([3, ${v}, -1, 0, -0]); return Array.from(u.toSorted(function (a, b) { return b - a })).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join()`));
    add(tc(`var u = new ${t}([3, ${v}, -1]); return [Array.from(u.toReversed()), Array.from(u.with(1, ${v})), u.toLocaleString(), u.toString()]`));
    add(tc(`var u = new ${t}(4); u.set([${v}, ${v}], 1); return Array.from(u).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) })`));
    add(tc(`var u = new ${t}([${v}, 2, 3]); return [u.findLast(function (x) { return x !== x }), u.findLastIndex(function (x) { return x !== x }), u.find(function (x) { return Object.is(x, -0) })]`));
    add(tc(`var u = new ${t}([1, 2, 3]); u.fill(${v}, -2, -1); return Array.from(u).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) })`));
    add(tc(`var u = new ${t}(1); u[0] = ${v}; return [u[0], Object.is(u[0], -0), String(u[0]), u.at(0), JSON.stringify(u)]`));
  }
  add(tc(`var u = new ${t}([NaN, NaN]); u.sort(); return u.join()`));
  add(tc(`var u = new ${t}([NaN, 1, -Infinity, Infinity, 0, -0, NaN]); u.sort(); return Array.from(u).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join()`));
  add(tc(`var u = new ${t}([NaN, 1, -Infinity, Infinity, 0, -0, NaN]); return Array.from(u.toSorted(function (a, b) { return a < b ? -1 : a > b ? 1 : 0 })).map(function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join()`));
  add(tc(`var u = new ${t}([0, -0]); u.sort(); return [Object.is(u[0], -0), Object.is(u[1], 0)]`));
  add(tc(`var u = new ${t}([-0, 0]); u.sort(); return [Object.is(u[0], -0), Object.is(u[1], 0)]`));
  add(tc(`var u = new ${t}([0, -0]); return Array.from(u.toSorted()).map(function (x) { return Object.is(x, -0) })`));
  add(tc(`return new ${t}([NaN, 1, -0]).toLocaleString()`));
  add(tc(`return new ${t}([1.5, 2.25]).toLocaleString('en-US')`));
  add(tc(`return new ${t}([1234.5, 2]).toLocaleString('en-US', {style: 'currency', currency: 'USD'})`));
  add(tc(`return new ${t}([1234.5, 2]).toLocaleString('de-DE')`));
  add(tc(`return new ${t}([]).toLocaleString()`));
  add(tc(`return new ${t}([1, 2, 3]).join(undefined)`));
  add(tc(`return new ${t}([1, 2, 3]).join(null)`));
  add(tc(`return new ${t}([1, 2, 3]).join('')`));
  add(tc(`return new ${t}([1, 2, 3]).join({toString() { return '+' }})`));
  add(tc(`return new ${t}([1, 2, 3]).join(Symbol())`));
}
// Inteiros: mesmas operações sem NaN.
for (const t of NUM.filter((x) => !x.startsWith("Float"))) {
  for (const v of ["NaN", "-0", "Infinity", "-Infinity", "1.5", "-1.5", "0.5", "-0.5", "2.5", "255.5", "256", "-129", "1e10", "2 ** 31", "2 ** 32", "2 ** 53 + 2", "-(2 ** 31) - 1", "'7'", "'x'", "null", "undefined", "true", "[]", "[3]", "{}"]) {
    add(tc(`var u = new ${t}(2); u[0] = ${v}; u.fill(${v}, 1); return [u[0], u[1], Object.is(u[0], -0)]`));
    add(tc(`var u = new ${t}([${v}, 1]); return [u[0], u.includes(${v}), u.indexOf(${v}), u.indexOf(0)]`));
    add(tc(`var u = new ${t}(3); u.set([${v}], 1); return Array.from(u)`));
    add(tc(`return Array.from(${t}.of(${v}, 1))`));
    add(tc(`return Array.from(${t}.from([${v}]))`));
    add(tc(`var u = new ${t}([5, 6]); return Array.from(u.with(0, ${v}))`));
  }
  add(tc(`var u = new ${t}([3, 1, 2]); return [Array.from(u.toSorted()), Array.from(u.toReversed()), Array.from(u.with(-1, 9)), u.at(-1), u.findLast(function (x) { return x < 3 })]`));
  add(tc(`return new ${t}([3, 1, 2]).with(3, 1)`));
  add(tc(`return new ${t}([3, 1, 2]).with(-4, 1)`));
  add(tc(`return new ${t}([3, 1, 2]).with(NaN, 7)`));
  add(tc(`return new ${t}([3, 1, 2]).with(Infinity, 7)`));
  add(tc(`return new ${t}([3, 1, 2]).with(-0, 7)`));
  add(tc(`return new ${t}([3, 1, 2]).with(1.9, 7)`));
  add(tc(`return new ${t}([3, 1, 2]).with(0, 7n)`));
  add(tc(`return new ${t}([3, 1, 2]).with(0, Symbol())`));
  add(tc(`return new ${t}([3, 1, 2]).at(Infinity)`));
  add(tc(`return new ${t}([3, 1, 2]).at(-Infinity)`));
  add(tc(`return new ${t}([3, 1, 2]).at(-0.5)`));
  add(tc(`return new ${t}([3, 1, 2]).at('1')`));
  add(tc(`return new ${t}([3, 1, 2]).at(2 ** 53)`));
}
for (const t of BIG) {
  for (const v of ["1", "'1'", "1n", "-1n", "2n ** 63n", "2n ** 64n", "-(2n ** 63n) - 1n", "true", "false", "null", "undefined", "NaN", "1.5", "'x'", "'0x10'", "''", "' 12 '", "'1.5'", "[]", "[1n]", "Symbol()", "{}", "{valueOf() { return 5n }}", "{valueOf() { return 5 }}", "{toString() { return '9' }}", "-0", "Object(3n)", "new Number(2)"]) {
    add(tc(`var u = new ${t}(2); u[0] = ${v}; return [u[0]]`));
    add(tc(`var u = new ${t}(2); u.fill(${v}); return Array.from(u)`));
    add(tc(`var u = new ${t}(2); u.set([${v}]); return Array.from(u)`));
    add(tc(`return Array.from(${t}.of(${v}))`));
    add(tc(`return ${t}.from([${v}]).length`));
    add(tc(`return new ${t}([${v}]).length`));
    add(tc(`var u = new ${t}([1n, 2n]); return u.includes(${v})`));
    add(tc(`var u = new ${t}([1n, 2n]); return u.indexOf(${v})`));
    add(tc(`var u = new ${t}([1n, 2n]); return Array.from(u.with(0, ${v}))`));
  }
  add(tc(`return new ${t}([1n, 2n]).join()`));
  add(tc(`return new ${t}([1n, 2n]).toLocaleString()`));
  add(tc(`return new ${t}([3n, 1n, 2n]).sort().join()`));
  add(tc(`return new ${t}([3n, -1n, 2n]).toSorted().join()`));
  add(tc(`return new ${t}([3n, -1n, 2n]).toSorted(function (a, b) { return Number(b - a) }).join()`));
  add(tc(`return new ${t}([3n, -1n, 2n]).toSorted(function (a, b) { return b - a })`));
  add(tc(`return new ${t}([3n, 1n]).toReversed().join()`));
  add(tc(`return new ${t}([3n, 1n]).at(-1)`));
  add(tc(`return new ${t}([3n, 1n]).findLast(function (x) { return x === 3n })`));
  add(tc(`return new ${t}([3n, 1n]) + ''`));
  add(tc(`return new ${t}([3n, 1n]).includes(3)`));
  add(tc(`return new ${t}([3n, 1n]).indexOf(3)`));
  add(tc(`return new ${t}([3n, 1n]).lastIndexOf(1n, -1)`));
  add(tc(`return JSON.stringify(new ${t}([3n]))`));
}
// Misturar BigInt e Number entre tipos.
for (const a of NUM.slice(0, 4).concat(["Float64Array"])) {
  for (const b of BIG) {
    add(tc(`return new ${a}(new ${b}(2))`));
    add(tc(`return new ${b}(new ${a}(2))`));
    add(tc(`var x = new ${a}(2); x.set(new ${b}(2)); return x`));
    add(tc(`var x = new ${b}(2); x.set(new ${a}(2)); return x`));
    add(tc(`return ${a}.from(new ${b}(1))`));
    add(tc(`return ${b}.from(new ${a}(1))`));
    add(tc(`var x = new ${b}(2); x.set([1])`));
    add(tc(`var x = new ${a}(2); x.set([1n])`));
    add(tc(`var x = new ${a}(2); x.set([{valueOf() { return 1n }}])`));
    add(tc(`return new ${a}(1).subarray(0).set(new ${b}(0))`));
    add(tc(`var x = new ${b}(2); x.set(new ${a}(0)); return x.length`));
    add(tc(`return ${a}.prototype.slice.call(new ${b}(1))`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.slice(0, 1)`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.subarray(0, 1)`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.map(function (v) { return v })`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.filter(function (v) { return true })`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.toSorted()`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.toReversed().constructor === ${a}`));
    add(tc(`var x = new ${a}(2); x.constructor = {[Symbol.species]: ${b}}; return x.with(0, 1).constructor === ${a}`));
  }
}
for (const a of BIG) for (const b of BIG) if (a !== b) {
  add(tc(`var x = new ${a}([1n, -1n]); var y = new ${b}(2); y.set(x); return Array.from(y)`));
  add(tc(`return Array.from(new ${b}(new ${a}([1n, -1n])))`));
}

// ---------------------------------------------------------------------------------------------
// set / subarray / slice / copyWithin com offsets e sobreposição.
for (const t of ["Uint8Array", "Int16Array", "Float32Array", "BigInt64Array"]) {
  const b = isBig(t);
  const arr = (n) => lit(t, Array.from({ length: n }, (_, i) => i + 1));
  for (const off of ["0", "1", "2", "-0", "NaN", "'1'", "1.9", "-1", "5", "6", "Infinity", "2 ** 32", "undefined", "null", "true"]) {
    add(tc(`var u = new ${t}(5); u.set([${arr(3)}], ${off}); return Array.from(u)`));
    add(tc(`var u = new ${t}(5); u.set(new ${t}([${arr(3)}]), ${off}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.set(u.subarray(0, 3), ${off}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.set(u.subarray(1, 4), ${off}); return Array.from(u)`));
  }
  for (const src of ["[]", "'abc'", "'12'", "{length: 2, 0: 1, 1: 2}", "{length: 2}", "{length: -1}", "{length: 1e10}", "1", "null", "undefined", "true", "new Set([1, 2])", "{}", "Object(1)", "new String('12')"]) {
    add(tc(`var u = new ${t}(3); u.set(${src}); return Array.from(u)`));
  }
  for (const [s, e] of [["0", "5"], ["1", "3"], ["-2", "undefined"], ["-3", "-1"], ["3", "1"], ["NaN", "NaN"], ["Infinity", "Infinity"], ["-Infinity", "Infinity"], ["undefined", "undefined"], ["'1'", "'3'"], ["1.9", "3.9"], ["-0", "-0"], ["null", "null"], ["2 ** 32", "2 ** 33"], ["10", "20"], ["-10", "-20"]]) {
    add(tc(`var u = new ${t}([${arr(5)}]); var q = u.subarray(${s}, ${e}); return [Array.from(q), q.byteOffset, q.length, q.buffer === u.buffer]`));
    add(tc(`var u = new ${t}([${arr(5)}]); var q = u.slice(${s}, ${e}); return [Array.from(q), q.byteOffset, q.length, q.buffer === u.buffer]`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.fill(${b ? "9n" : "9"}, ${s}, ${e}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.copyWithin(0, ${s}, ${e}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.copyWithin(2, ${s}, ${e}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); u.copyWithin(${s}, 1, ${e}); return Array.from(u)`));
    add(tc(`var u = new ${t}([${arr(5)}]); return [u.includes(${b ? "3n" : "3"}, ${s}), u.indexOf(${b ? "3n" : "3"}, ${s}), u.lastIndexOf(${b ? "3n" : "3"}, ${s})]`));
  }
}

// ---------------------------------------------------------------------------------------------
// Ordem de coerção observada por Proxy e valueOf.
for (const t of ["Uint8Array", "Float64Array", "BigInt64Array"]) {
  const b = isBig(t);
  const v = b ? "1n" : "1";
  const log = `var l = []; var o = function (n, r) { return {valueOf() { l.push(n); return r }} };`;
  for (const [name, call] of [
    ["fill", `u.fill({valueOf() { l.push('v'); return ${v} }}, o('s', 0), o('e', 2))`],
    ["copyWithin", `u.copyWithin(o('t', 0), o('s', 1), o('e', 3))`],
    ["slice", `u.slice(o('s', 0), o('e', 2))`],
    ["subarray", `u.subarray(o('s', 0), o('e', 2))`],
    ["set", `u.set([${v}], o('o', 0))`],
    ["includes", `u.includes(${v}, o('f', 0))`],
    ["indexOf", `u.indexOf(${v}, o('f', 0))`],
    ["lastIndexOf", `u.lastIndexOf(${v}, o('f', 0))`],
    ["join", `u.join({toString() { l.push('sep'); return ',' }})`],
    ["at", `u.at(o('i', 0))`],
    ["with", `u.with(o('i', 0), {valueOf() { l.push('v'); return ${v} }})`],
    ["with oob", `u.with(o('i', 9), {valueOf() { l.push('v'); return ${v} }})`],
    ["toSorted", `u.toSorted({valueOf() { l.push('cmp'); return 1 }})`],
    ["sort", `u.sort({valueOf() { l.push('cmp'); return 1 }})`],
    ["map", `u.map({valueOf() { l.push('cb'); return 1 }})`],
    ["forEach", `u.forEach(o('cb', 1))`],
    ["findLast", `u.findLast(o('cb', 1))`],
    ["from", `${t}.from([${v}], {valueOf() { l.push('map'); return 1 }})`],
    ["from this", `${t}.from.call({valueOf() { l.push('this'); return 1 }}, [${v}])`],
    ["ctor len", `new ${t}(o('len', 2))`],
    ["ctor off", `new ${t}(new ArrayBuffer(16), o('off', 0), o('len', 1))`],
    ["ctor off oob", `new ${t}(new ArrayBuffer(16), o('off', 100), o('len', 1))`],
    ["ctor len oob", `new ${t}(new ArrayBuffer(16), o('off', 0), o('len', 100))`],
    ["ctor off neg", `new ${t}(new ArrayBuffer(16), o('off', -1), o('len', 1))`],
    ["ctor len neg", `new ${t}(new ArrayBuffer(16), o('off', 0), o('len', -1))`],
    ["ctor iter", `new ${t}({[Symbol.iterator]() { l.push('iter'); return [${v}][Symbol.iterator]() }, get length() { l.push('length'); return 1 }})`],
    ["ctor arraylike", `new ${t}({get length() { l.push('length'); return 2 }, get 0() { l.push('0'); return ${v} }, get 1() { l.push('1'); return ${v} }})`],
    ["ctor arraylike coerce", `new ${t}({length: 2, 0: o('a', ${v}), 1: o('b', ${v})})`],
    ["set arraylike", `u.set({get length() { l.push('length'); return 2 }, get 0() { l.push('0'); return ${v} }, get 1() { l.push('1'); return ${v} }}, o('o', 1))`],
  ]) {
    add(tc(`${log} var u = new ${t}(4); var r; try { r = ${call} } catch (e) { l.push(e.name + ': ' + e.message) } return l`));
  }
  add(tc(`var l = []; var p = new Proxy([${v}, ${v}], {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }, has(t, k) { l.push('has ' + String(k)); return k in t }}); var u = new ${t}(p); return l`));
  add(tc(`var l = []; var p = new Proxy([${v}, ${v}], {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }, has(t, k) { l.push('has ' + String(k)); return k in t }}); var u = new ${t}(2); u.set(p, 1); return l`));
  add(tc(`var l = []; var p = new Proxy([${v}, ${v}], {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }}); var u = ${t}.from(p); return l`));
  add(tc(`var l = []; var p = new Proxy({length: 2, 0: ${v}, 1: ${v}}, {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }, has(t, k) { l.push('has ' + String(k)); return k in t }}); var u = ${t}.from(p); return l`));
  add(tc(`var l = []; var p = new Proxy({length: 2, 0: ${v}, 1: ${v}}, {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }}); var u = new ${t}(2); u.set(p); return l`));
  add(tc(`var l = []; var p = new Proxy({length: 2, 0: ${v}, 1: ${v}}, {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }}); return new ${t}(p).length + ':' + l`));
  add(tc(`var l = []; var p = new Proxy(new ${t}(2), {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, t) }}); try { Array.prototype.slice.call(p) } catch (e) { l.push(e.name) } return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = new Proxy(${t}, {get(t, k, r) { l.push('get ' + String(k)); return Reflect.get(t, k, r) }}); u.slice(0, 1); return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = {get [Symbol.species]() { l.push('species'); return ${t} }}; u.subarray(0, 1); return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = {get [Symbol.species]() { l.push('species'); return ${t} }}; u.map(function (x) { l.push('cb'); return x }); return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = {get [Symbol.species]() { l.push('species'); return ${t} }}; u.filter(function (x) { l.push('cb'); return true }); return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = {get [Symbol.species]() { l.push('species'); return ${t} }}; u.toSorted(); u.toReversed(); u.with(0, ${v}); return l`));
  add(tc(`var l = []; var u = new ${t}(2); u.constructor = {[Symbol.species]: function (n) { l.push('ctor ' + n); return new ${t}(n) }}; u.slice(1); u.subarray(1); u.map(function (x) { return x }); u.filter(function () { return true }); return l`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: function (n) { return new ${t}(1) }}; return u.slice(0, 2)`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: function (n) { return new ${t}(5) }}; return u.slice(0, 2).length`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: function (n) { return [] }}; return u.slice(0, 2)`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: function (n) { return new ${t}(n) }}; return u.map(function () { return ${v} }).join()`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: null}; return u.slice().constructor === ${t}`));
  add(tc(`var u = new ${t}(2); u.constructor = {[Symbol.species]: 3}; return u.slice()`));
  add(tc(`var u = new ${t}(2); u.constructor = 3; return u.slice()`));
  add(tc(`var u = new ${t}(2); u.constructor = undefined; return u.slice().constructor === ${t}`));
}

// ---------------------------------------------------------------------------------------------
// Float16Array e Math.f16round e DataView.getFloat16.
add(`typeof Float16Array`);
add(`Float16Array.BYTES_PER_ELEMENT`);
add(`Float16Array.name`);
add(`Float16Array.length`);
add(`Float16Array.prototype.BYTES_PER_ELEMENT`);
add(`Object.getPrototypeOf(Float16Array) === Object.getPrototypeOf(Int8Array)`);
add(`Object.prototype.toString.call(new Float16Array(1))`);
add(`new Float16Array(3).byteLength`);
add(`Math.f16round.length`);
add(`Math.f16round.name`);
add(`DataView.prototype.getFloat16.length`);
add(`DataView.prototype.setFloat16.length`);
add(`Float16Array(1)`);
for (const v of [...SPECIAL, "65519.99", "65519", "-65520", "65536", "1e5", "0.1", "0.2", "0.3", "1/3", "Math.PI", "-Math.PI", "6.103515625e-5", "6.097555160522461e-5", "5.960464477539063e-8", "5.960464477539062e-8", "2.98023223876953125e-8", "2.98023223876953126e-8", "1.0004882812", "1.00048828125", "1.00146484375", "1.0009765625", "2049", "2050", "2051", "4097", "4098", "4099", "-0.0000001", "NaN", "'3.5'", "null", "undefined", "true", "[]", "{}", "[2]", "'abc'", "Object(5)", "{valueOf() { return 7.7 }}"]) {
  add(tc(`return [Math.f16round(${v}), Object.is(Math.f16round(${v}), -0)]`));
  add(tc(`var u = new Float16Array(1); u[0] = ${v}; return [u[0], Object.is(u[0], -0), new Uint16Array(u.buffer)[0].toString(16)]`));
  add(tc(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, ${v}); return [d.getFloat16(0), d.getUint16(0).toString(16), d.getFloat16(0, true)]`));
  add(tc(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, ${v}, true); return [d.getFloat16(0, true), d.getUint16(0, true).toString(16)]`));
  add(tc(`return new Float16Array([${v}, 1]).join()`));
  add(tc(`return Array.from(new Float16Array([${v}, 1]).toSorted())`));
}
for (const bits of ["0000", "8000", "0001", "03ff", "0400", "7bff", "7c00", "fc00", "7c01", "7e00", "fe00", "ffff", "3c00", "3555", "3800", "bc00", "0200", "8001", "5640", "5bc0"]) {
  add(tc(`var d = new DataView(new ArrayBuffer(2)); d.setUint16(0, 0x${bits}); return [d.getFloat16(0), Object.is(d.getFloat16(0), -0)]`));
  add(tc(`var u = new Uint16Array([0x${bits}]); var f = new Float16Array(u.buffer); return [f[0], Object.is(f[0], -0), f.toString()]`));
  add(tc(`var u = new Uint16Array([0x${bits}]); var f = new Float16Array(u.buffer); f.fill(f[0]); return new Uint16Array(f.buffer)[0].toString(16)`));
}
for (const [name, body] of [
  ["from", `return Float16Array.from([1, 2.5, 1e5]).join()`],
  ["of", `return Float16Array.of(1, 2.5, 1e5).join()`],
  ["from big", `return Float16Array.from([1n])`],
  ["ctor big", `return new Float16Array(new BigInt64Array(1))`],
  ["ctor f64", `return new Float16Array(new Float64Array([1.1, 65520, 1e-8])).join()`],
  ["ctor f32", `return new Float16Array(new Float32Array([1.1, 65520, 1e-8])).join()`],
  ["to f32", `return new Float32Array(new Float16Array([1.1, 2.2])).join()`],
  ["to f64", `return new Float64Array(new Float16Array([0.1, 0.2])).join()`],
  ["set from f64", `var u = new Float16Array(2); u.set(new Float64Array([0.1, 1e5])); return u.join()`],
  ["set from int", `var u = new Float16Array(2); u.set(new Int32Array([2049, 2051])); return u.join()`],
  ["set from u32", `var u = new Float16Array(2); u.set(new Uint32Array([65519, 65520])); return u.join()`],
  ["set to int", `var u = new Int8Array(2); u.set(new Float16Array([1.9, -1.9])); return u.join()`],
  ["sort", `return new Float16Array([3, NaN, -0, 0, -Infinity, 1.5]).sort().join()`],
  ["includes nan", `return new Float16Array([NaN]).includes(NaN)`],
  ["indexof", `return new Float16Array([0.1]).indexOf(0.1)`],
  ["indexof round", `return new Float16Array([0.1]).indexOf(Math.f16round(0.1))`],
  ["includes", `return new Float16Array([0.1]).includes(Math.fround(0.1))`],
  ["fill", `return new Float16Array(2).fill(0.1).join()`],
  ["subarray", `var u = new Float16Array([1, 2, 3, 4]); var s = u.subarray(1, 3); return [s.length, s.byteOffset, s.byteLength]`],
  ["slice", `return new Float16Array([1, 2, 3, 4]).slice(1, 3).join()`],
  ["map", `return new Float16Array([1, 2]).map(function (x) { return x / 3 }).join()`],
  ["reduce", `return new Float16Array([0.1, 0.2]).reduce(function (a, b) { return a + b })`],
  ["with", `return new Float16Array([1, 2]).with(0, 0.1).join()`],
  ["toLocaleString", `return new Float16Array([1.5, 2.25]).toLocaleString('en-US')`],
  ["align", `return new Float16Array(new ArrayBuffer(4), 1)`],
  ["align ok", `return new Float16Array(new ArrayBuffer(4), 2).length`],
  ["len oob", `return new Float16Array(new ArrayBuffer(3))`],
  ["resizable", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Float16Array(b); b.resize(7); return [u.length, u.byteLength]`],
  ["atomics", `return Atomics.add(new Float16Array(new SharedArrayBuffer(4)), 0, 1)`],
  ["f16round neg", `return Math.f16round(-1.0009765625 - 0.00048828125)`],
  ["f16round no arg", `return Math.f16round()`],
  ["f16round big", `return Math.f16round(1n)`],
  ["f16round sym", `return Math.f16round(Symbol())`],
  ["dv big", `return new DataView(new ArrayBuffer(2)).setFloat16(0, 1n)`],
  ["dv oob", `return new DataView(new ArrayBuffer(1)).getFloat16(0)`],
  ["dv oob set", `return new DataView(new ArrayBuffer(1)).setFloat16(0, 1)`],
  ["dv ret", `return new DataView(new ArrayBuffer(2)).setFloat16(0, 1)`],
  ["dv idx", `return new DataView(new ArrayBuffer(2)).getFloat16(-1)`],
  ["dv idx coerce", `var d = new DataView(new ArrayBuffer(4)); d.setFloat16(2, 1.5); return d.getFloat16('2')`],
  ["species", `return new Float16Array(2).slice().constructor === Float16Array`],
  ["structure", `return Object.getOwnPropertyNames(Float16Array.prototype)`],
  ["from iter", `return Float16Array.from(new Set([1, 2, 0.1])).join()`],
  ["from map", `return Float16Array.from([1, 2], function (x) { return x * 0.1 }).join()`],
  ["json", `return JSON.stringify(new Float16Array([1, 0.5, NaN]))`],
  ["entries", `return Array.from(new Float16Array([1, 0.1]).entries())`],
  ["Object.is", `var u = new Float16Array(1); u[0] = -0; return Object.is(u[0], -0)`],
]) add(tc(body));

// ---------------------------------------------------------------------------------------------
// Conversões: Uint8Clamped half-even, Int8 wrap, etc.
const CLAMP = ["-1", "-0.5", "-0", "0", "0.4", "0.5", "0.6", "1.5", "2.5", "3.5", "4.5", "126.5", "127.5", "128.5", "253.5", "254.5", "254.4999", "254.5001", "255", "255.4", "255.5", "255.6", "256", "1e10", "-1e10", "Infinity", "-Infinity", "NaN", "0.49999999999999994", "0.5000000000000001", "1.4999999999999998", "2.5000000000000004", "'2.5'", "'abc'", "null", "undefined", "true", "false", "[]", "[7]", "{}", "{valueOf() { return 2.5 }}", "{valueOf() { return 3.5 }}", "1n"];
for (const v of CLAMP) {
  add(tc(`var u = new Uint8ClampedArray(1); u[0] = ${v}; return u[0]`));
  add(tc(`return new Uint8ClampedArray([${v}])[0]`));
  add(tc(`return Uint8ClampedArray.of(${v})[0]`));
  add(tc(`return new Uint8ClampedArray(2).fill(${v})[1]`));
  add(tc(`var u = new Uint8ClampedArray(2); u.set([${v}], 1); return u[1]`));
  add(tc(`return Uint8ClampedArray.from([${v}])[0]`));
  add(tc(`return new Uint8ClampedArray(2).with(0, ${v})[0]`));
  add(tc(`var d = new DataView(new ArrayBuffer(1)); d.setUint8(0, ${v}); return d.getUint8(0)`));
  add(tc(`var d = new DataView(new ArrayBuffer(1)); d.setInt8(0, ${v}); return d.getInt8(0)`));
}
for (const t of ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array"]) {
  const bits = es(t) * 8;
  for (const v of ["127", "128", "129", "255", "256", "257", "-128", "-129", "-255", "-256", "32767", "32768", "65535", "65536", "2 ** 31 - 1", "2 ** 31", "2 ** 32 - 1", "2 ** 32", "2 ** 32 + 1", "-(2 ** 31)", "-(2 ** 31) - 1", "2 ** 53", "2 ** 53 + 2", "2 ** 64", "2 ** 64 + 2 ** 12", "1e21", "-1e21", "1.9", "-1.9", "0.99999", "-0.99999", "4294967295.5", "4294967296.5", "-4294967295.5", "'0x10'", "'1e3'", "' 12 '", "'12px'", "''"]) {
    add(tc(`var u = new ${t}(1); u[0] = ${v}; return u[0]`));
    add(tc(`return new ${t}([${v}])[0]`));
  }
}
for (const [t, set] of [["Int8Array", "setInt8"], ["Int16Array", "setInt16"], ["Int32Array", "setInt32"], ["Uint16Array", "setUint16"], ["Uint32Array", "setUint32"], ["Float32Array", "setFloat32"]]) {
  for (const v of ["-1", "2 ** 31", "2 ** 32 + 5", "65537", "1.5", "NaN", "Infinity", "-0", "1e40", "1e-50", "16777217", "0.1", "3.4028235677973366e38", "3.4028234663852886e38", "1.401298464324817e-45", "7e-46", "7.1e-46"]) {
    add(tc(`var d = new DataView(new ArrayBuffer(8)); d.${set}(0, ${v}); var u = new ${t}(d.buffer.slice(0, ${es(t)})); return [u[0], Object.is(u[0], -0)]`));
    add(tc(`var d = new DataView(new ArrayBuffer(8)); d.${set}(0, ${v}, true); return Array.from(new Uint8Array(d.buffer))`));
  }
}
for (const t of BIG) {
  for (const v of ["0n", "1n", "-1n", "2n ** 63n", "2n ** 63n - 1n", "-(2n ** 63n)", "2n ** 64n", "2n ** 64n - 1n", "2n ** 64n + 5n", "-(2n ** 64n) - 1n", "2n ** 100n", "-(2n ** 100n) + 7n", "BigInt(Number.MAX_SAFE_INTEGER)", "BigInt.asIntN(64, 2n ** 63n)", "BigInt.asUintN(64, -1n)"]) {
    add(tc(`var u = new ${t}(1); u[0] = ${v}; return u[0]`));
    add(tc(`return new ${t}([${v}])[0]`));
    add(tc(`var d = new DataView(new ArrayBuffer(8)); d.set${t.replace("Array", "")}(0, ${v}); return [d.getBigInt64(0), d.getBigUint64(0), d.getBigInt64(0, true)]`));
    add(tc(`var u = new ${t}([${v}]); return [typeof u[0], new Uint8Array(u.buffer).join()]`));
  }
}
// Float32 arredondamento.
for (const v of ["0.1", "16777217", "1e39", "-1e39", "3.4028235677973366e38", "1e-46", "1.5e-45", "NaN", "-0", "1/3", "2 ** 24 + 1", "0.1 + 0.2", "123456789.123456789", "5e-324"]) {
  add(tc(`var u = new Float32Array(1); u[0] = ${v}; return [u[0], Object.is(u[0], -0)]`));
  add(tc(`return Math.fround(${v}) === new Float32Array([${v}])[0]`));
}
// Leitura de bits NaN e preservação.
add(tc(`var u = new Uint32Array([0x7fc00001]); return new Float32Array(u.buffer)[0]`));
add(tc(`var f = new Float64Array(1); f[0] = NaN; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float32Array(1); f[0] = NaN; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float64Array(1); f[0] = -NaN; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float64Array(1); f[0] = -0; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float32Array(1); f[0] = -0; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float64Array(1); f[0] = Infinity; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float64Array(1); f[0] = 5e-324; return new Uint8Array(f.buffer).join()`));
add(tc(`var f = new Float64Array([1]); return new BigUint64Array(f.buffer)[0].toString(16)`));

// ---------------------------------------------------------------------------------------------
// base64 e hex.
const B64 = ["''", "'AA=='", "'AAA='", "'AAAA'", "'TWFu'", "'TWE='", "'TQ=='", "'TQ'", "'TWE'", "'TW'", "'T'", "'TQ='", "'TQ==='", "'TQ=x'", "'TW Fu'", "'TW\\nFu'", "' TWFu '", "'TWFu\\t'", "'TW-_'", "'TW+/'", "'_-_-'", "'+/+/'", "'A'", "'AB'", "'ABC'", "'ABCD'", "'ABCDE'", "'ABC=D'", "'=AAA'", "'AA=A'", "'A==='", "'===='", "'TQ==TQ=='", "'TR=='", "'TWF='", "'/w=='", "'/w'", "'_w=='", "'_w'", "'\\u00e9AAA'", "'AAA\\u00e9'", "'SGVsbG8gV29ybGQ='", "'SGVsbG8gV29ybGQ'", "'SGVsbG8gV29ybGR='", "'/+8='", "'_-8='", "'AQID'", "'AQIDBA=='", "'AQIDBAU='", "'AQIDBAUG'", "'AAAA\\n'", "'AA\\r\\nAA'", "'AA\\f=='", "'AA=='+'AA=='", "'\\u0000AAA'", "'AAAAAAAAAAAAAAAAAAAA'", "{toString() { return 'AAAA' }}", "1", "null", "undefined", "[]", "new String('TWFu')", "Symbol()"];
const OPTS = ["undefined", "{}", "{alphabet: 'base64'}", "{alphabet: 'base64url'}", "{alphabet: 'x'}", "{alphabet: 1}", "{alphabet: undefined}", "{alphabet: null}", "{lastChunkHandling: 'loose'}", "{lastChunkHandling: 'strict'}", "{lastChunkHandling: 'stop-before-partial'}", "{lastChunkHandling: 'x'}", "{lastChunkHandling: undefined}", "{lastChunkHandling: 1}", "{alphabet: 'base64url', lastChunkHandling: 'strict'}", "{alphabet: 'base64', lastChunkHandling: 'stop-before-partial'}", "null", "1", "'x'", "[]", "function () {}", "true"];
for (const s of B64) {
  for (const o of ["undefined", "{alphabet: 'base64url'}", "{lastChunkHandling: 'strict'}", "{lastChunkHandling: 'stop-before-partial'}", "{lastChunkHandling: 'loose'}"]) {
    add(tc(`return Array.from(Uint8Array.fromBase64(${s}, ${o}))`));
  }
  add(tc(`var u = new Uint8Array(8); var r = u.setFromBase64(${s}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(2); var r = u.setFromBase64(${s}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(2); var r = u.setFromBase64(${s}, {lastChunkHandling: 'stop-before-partial'}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(3); var r = u.setFromBase64(${s}, {lastChunkHandling: 'strict'}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(0); var r = u.setFromBase64(${s}); return [r.read, r.written]`));
}
for (const o of OPTS) {
  add(tc(`return Array.from(Uint8Array.fromBase64('TWFu', ${o}))`));
  add(tc(`return Array.from(Uint8Array.fromBase64('TQ', ${o}))`));
  add(tc(`return Array.from(Uint8Array.fromBase64('TQ=', ${o}))`));
  add(tc(`return Array.from(Uint8Array.fromBase64('TW-_', ${o}))`));
  add(tc(`return Array.from(Uint8Array.fromBase64('TW+/', ${o}))`));
  add(tc(`return new Uint8Array([251, 255, 254, 0, 1]).toBase64(${o})`));
  add(tc(`return new Uint8Array([1, 2]).toBase64(${o})`));
  add(tc(`return new Uint8Array([1]).toBase64(${o})`));
  add(tc(`return new Uint8Array([]).toBase64(${o})`));
  add(tc(`var u = new Uint8Array(4); return [u.setFromBase64('AQID', ${o}), Array.from(u)]`));
}
for (const bytes of ["[]", "[0]", "[255]", "[0, 1, 2, 3, 4, 5]", "[255, 254, 253, 252]", "[16, 32, 48]", "[171, 205, 239]", "[0, 15, 240, 255]", "[1, 2, 3, 4, 5, 6, 7]", "[248, 255]", "[251]", "[250, 251]", "[63, 62]", "[62, 63]"]) {
  add(tc(`return new Uint8Array(${bytes}).toHex()`));
  add(tc(`return new Uint8Array(${bytes}).toBase64()`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({alphabet: 'base64url'})`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({omitPadding: true})`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({alphabet: 'base64url', omitPadding: true})`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({omitPadding: false})`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({omitPadding: 0})`));
  add(tc(`return new Uint8Array(${bytes}).toBase64({omitPadding: 'x'})`));
  add(tc(`var u = new Uint8Array(${bytes}); return Array.from(Uint8Array.fromBase64(u.toBase64())).join() === u.join()`));
  add(tc(`var u = new Uint8Array(${bytes}); return Array.from(Uint8Array.fromHex(u.toHex())).join() === u.join()`));
}
const HEX = ["''", "'00'", "'ff'", "'FF'", "'aF'", "'0'", "'abc'", "'abcd'", "'0g'", "'g0'", "'0x00'", "' 00'", "'00 '", "'00 11'", "'0011\\n'", "'\\u00e9\\u00e9'", "'0123456789abcdefABCDEF'", "'deadBEEF'", "'DEADBEEF00'", "'0000000000000000'", "{toString() { return '0a' }}", "1", "null", "undefined", "[]", "new String('0a')", "Symbol()", "'00-11'", "'0011zz'", "'00112'", "'00\\u00001'"];
for (const s of HEX) {
  add(tc(`return Array.from(Uint8Array.fromHex(${s}))`));
  add(tc(`var u = new Uint8Array(4); var r = u.setFromHex(${s}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(1); var r = u.setFromHex(${s}); return [r.read, r.written, Array.from(u)]`));
  add(tc(`var u = new Uint8Array(0); var r = u.setFromHex(${s}); return [r.read, r.written]`));
  add(tc(`var u = new Uint8Array(2); var r = u.setFromHex(${s}); return Object.keys(r)`));
}
for (const [name, body] of [
  ["fromBase64 length", `return [Uint8Array.fromBase64.length, Uint8Array.fromHex.length, Uint8Array.prototype.toBase64.length, Uint8Array.prototype.toHex.length, Uint8Array.prototype.setFromBase64.length, Uint8Array.prototype.setFromHex.length]`],
  ["fromBase64 names", `return [Uint8Array.fromBase64.name, Uint8Array.fromHex.name, Uint8Array.prototype.toBase64.name, Uint8Array.prototype.setFromHex.name]`],
  ["not on Int8", `return [typeof Int8Array.fromBase64, typeof Int8Array.prototype.toHex, typeof Uint8ClampedArray.fromHex]`],
  ["not on proto", `return [typeof Object.getPrototypeOf(Uint8Array).fromBase64, typeof Object.getPrototypeOf(Uint8Array.prototype).toHex]`],
  ["toHex on Int8", `return Uint8Array.prototype.toHex.call(new Int8Array(1))`],
  ["toHex on array", `return Uint8Array.prototype.toHex.call([1])`],
  ["toHex on obj", `return Uint8Array.prototype.toHex.call({})`],
  ["toBase64 on Uint8Clamped", `return Uint8Array.prototype.toBase64.call(new Uint8ClampedArray(1))`],
  ["toBase64 on dv", `return Uint8Array.prototype.toBase64.call(new DataView(new ArrayBuffer(1)))`],
  ["toHex detached", `var u = new Uint8Array(2); u.buffer.transfer(); return u.toHex()`],
  ["toBase64 detached", `var u = new Uint8Array(2); u.buffer.transfer(); return u.toBase64()`],
  ["setFromHex detached", `var u = new Uint8Array(2); u.buffer.transfer(); return u.setFromHex('00')`],
  ["setFromBase64 detached", `var u = new Uint8Array(2); u.buffer.transfer(); return u.setFromBase64('AA==')`],
  ["toHex oob", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Uint8Array(b, 0, 4); b.resize(2); return u.toHex()`],
  ["toHex track", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Uint8Array(b); b.resize(2); return u.toHex()`],
  ["toBase64 track", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Uint8Array(b); u.fill(255); b.resize(8); return u.toBase64()`],
  ["setFromHex track", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Uint8Array(b); b.resize(8); var r = u.setFromHex('0102030405060708'); return [r.read, r.written, u.length]`],
  ["setFromBase64 oob", `var b = new ArrayBuffer(4, {maxByteLength: 8}); var u = new Uint8Array(b, 0, 4); b.resize(2); return u.setFromBase64('AAAA')`],
  ["toBase64 opts getter order", `var l = []; new Uint8Array(1).toBase64({get alphabet() { l.push('a'); return 'base64' }, get omitPadding() { l.push('o'); return false }}); return l`],
  ["fromBase64 opts getter order", `var l = []; Uint8Array.fromBase64('AA==', {get alphabet() { l.push('a'); return 'base64' }, get lastChunkHandling() { l.push('l'); return 'loose' }}); return l`],
  ["fromBase64 string first", `var l = []; try { Uint8Array.fromBase64(1, {get alphabet() { l.push('a'); return 'base64' }}) } catch (e) { l.push(e.name) } return l`],
  ["fromBase64 detach in opts", `var u = new Uint8Array(4); return u.setFromBase64('AAAA', {get alphabet() { u.buffer.transfer(); return 'base64' }})`],
  ["fromBase64 shrink in opts", `var b = new ArrayBuffer(8, {maxByteLength: 8}); var u = new Uint8Array(b); return u.setFromBase64('AAAAAAAA', {get alphabet() { b.resize(1); return 'base64' }})`],
  ["fromBase64 this", `return Uint8Array.fromBase64.call(null, 'AA==')`],
  ["fromBase64 subclass", `class U extends Uint8Array {} return U.fromBase64('AA==').constructor === Uint8Array`],
  ["fromHex subclass", `class U extends Uint8Array {} return U.fromHex('00').constructor === Uint8Array`],
  ["setFromBase64 partial write", `var u = new Uint8Array(4); var r = u.setFromBase64('AQIDBAUG'); return [r.read, r.written, Array.from(u)]`],
  ["setFromBase64 partial exact", `var u = new Uint8Array(3); var r = u.setFromBase64('AQIDBAUG'); return [r.read, r.written, Array.from(u)]`],
  ["setFromBase64 error keeps", `var u = new Uint8Array([9, 9, 9, 9]); try { u.setFromBase64('AQIDB###') } catch (e) { return [e.name, Array.from(u)] }`],
  ["setFromBase64 error keeps2", `var u = new Uint8Array([9, 9, 9, 9]); try { u.setFromBase64('AQID=AAA') } catch (e) { return [e.name, Array.from(u)] }`],
  ["setFromHex error keeps", `var u = new Uint8Array([9, 9, 9, 9]); try { u.setFromHex('0102zz') } catch (e) { return [e.name, Array.from(u)] }`],
  ["setFromHex odd", `var u = new Uint8Array([9, 9]); try { u.setFromHex('010') } catch (e) { return [e.name, Array.from(u)] }`],
  ["result proto", `var r = new Uint8Array(2).setFromHex('00'); return [Object.getPrototypeOf(r) === Object.prototype, Object.keys(r)]`],
  ["big message", `return Uint8Array.fromBase64('A'.repeat(100)).length`],
  ["big hex", `return Uint8Array.fromHex('ab'.repeat(100)).length`],
  ["big to hex", `return new Uint8Array(100).fill(171).toHex().length`],
  ["big to b64", `return new Uint8Array(100).fill(171).toBase64().length`],
]) add(tc(body));

// ---------------------------------------------------------------------------------------------
// Array.from(typedArray), spread, iteração e outros.
for (const t of TYPES) {
  const big = isBig(t);
  const L = lit(t, [1, 2, 3]);
  add(tc(`return Array.from(new ${t}([${L}]))`));
  add(tc(`return Array.from(new ${t}([${L}]), function (x, i) { return typeof x + i })`));
  add(tc(`return Array.from(new ${t}([${L}]).entries())`));
  add(tc(`return Array.from(new ${t}([${L}]).keys())`));
  add(tc(`return [...new ${t}([${L}]).values()]`));
  add(tc(`return [...new ${t}([${L}])]`));
  add(tc(`return Array.from.call(Object, new ${t}([${L}]))`));
  add(tc(`return Array.prototype.concat.call([], new ${t}([${L}])).length`));
  add(tc(`return [].concat(new ${t}([${L}]))`));
  add(tc(`var u = new ${t}([${L}]); u[Symbol.isConcatSpreadable] = true; return [].concat(u)`));
  add(tc(`return Array.prototype.map.call(new ${t}([${L}]), function (x) { return typeof x })`));
  add(tc(`return Array.prototype.slice.call(new ${t}([${L}]), 1)`));
  add(tc(`return Array.prototype.indexOf.call(new ${t}([${L}]), ${big ? "2n" : "2"})`));
  add(tc(`return Object.entries(new ${t}([${L}]))`));
  add(tc(`return Object.values(new ${t}([${L}]))`));
  add(tc(`return Object.assign({}, new ${t}([${L}]))`));
  add(tc(`return {...new ${t}([${L}])}`));
  add(tc(`return Object.getOwnPropertyDescriptors(new ${t}([${L}]))`));
  add(tc(`return Object.isFrozen(new ${t}(0))`));
  add(tc(`return Object.freeze(new ${t}(1))`));
  add(tc(`return Object.freeze(new ${t}(0)).length`));
  add(tc(`return Object.seal(new ${t}(1)).length`));
  add(tc(`return Object.isExtensible(Object.preventExtensions(new ${t}(1)))`));
  add(tc(`'use strict'; var u = new ${t}(1); Object.preventExtensions(u); u.x = 1; return u.x`));
  add(tc(`return Object.defineProperty(new ${t}(1), 0, {value: ${big ? "5n" : "5"}})[0]`));
  add(tc(`return Object.defineProperty(new ${t}(1), 0, {value: ${big ? "5n" : "5"}, writable: false})`));
  add(tc(`return Object.defineProperty(new ${t}(1), 0, {value: ${big ? "5n" : "5"}, enumerable: false})`));
  add(tc(`return Object.defineProperty(new ${t}(1), 0, {get() { return 1 }})`));
  add(tc(`return Object.defineProperty(new ${t}(1), 1, {value: ${big ? "5n" : "5"}})`));
  add(tc(`return Reflect.defineProperty(new ${t}(1), 1, {value: ${big ? "5n" : "5"}})`));
  add(tc(`return Reflect.defineProperty(new ${t}(1), '-0', {value: ${big ? "5n" : "5"}})`));
  add(tc(`return Reflect.defineProperty(new ${t}(1), '0.5', {value: ${big ? "5n" : "5"}})`));
  add(tc(`return Reflect.defineProperty(new ${t}(1), '1e3', {value: ${big ? "5n" : "5"}})`));
  add(tc(`var u = new ${t}(1); u['-0'] = ${big ? "5n" : "5"}; return [u['-0'], Object.keys(u), '-0' in u]`));
  add(tc(`var u = new ${t}(1); u['0.5'] = ${big ? "5n" : "5"}; return [u['0.5'], Object.keys(u), '0.5' in u]`));
  add(tc(`var u = new ${t}(1); u['1'] = ${big ? "5n" : "5"}; u.foo = 1; return [Object.keys(u), '1' in u, u.hasOwnProperty('1'), u.foo]`));
  add(tc(`var u = new ${t}(2); delete u[0]`));
  add(tc(`'use strict'; var u = new ${t}(2); return delete u[0]`));
  add(tc(`'use strict'; var u = new ${t}(2); return delete u[5]`));
  add(tc(`'use strict'; var u = new ${t}(2); return delete u['-0']`));
  add(tc(`var u = new ${t}(2); Object.prototype[5] = 1; var r = u[5]; delete Object.prototype[5]; return r`));
  add(tc(`var u = new ${t}(2); Object.prototype['-0'] = 1; var r = u['-0']; delete Object.prototype['-0']; return r`));
  add(tc(`var u = new ${t}(2); Object.prototype[5] = 1; u[5] = ${big ? "7n" : "7"}; var r = [Object.prototype[5], u.hasOwnProperty(5)]; delete Object.prototype[5]; return r`));
  add(tc(`'use strict'; var o = Object.create(new ${t}(2)); o[0] = ${big ? "7n" : "7"}; o[5] = ${big ? "7n" : "7"}; return [Object.getPrototypeOf(o)[0], o.hasOwnProperty(0), o.hasOwnProperty(5)]`));
  add(tc(`var o = Object.create(new ${t}(2)); return [o.length, o[0], o[5]]`));
  add(tc(`return Reflect.set(new ${t}(2), 0, ${big ? "5n" : "5"}, {})`));
  add(tc(`var r = {}; Reflect.set(new ${t}(2), 5, ${big ? "5n" : "5"}, r); return Object.keys(r)`));
  add(tc(`var u = new ${t}(2); var r = {}; Reflect.set(u, 0, ${big ? "5n" : "5"}, r); return [Object.keys(r), u[0]]`));
  add(tc(`var u = new ${t}(2); return Reflect.set(u, 0, ${big ? "5n" : "5"}, u)`));
  add(tc(`var u = new ${t}(2); return Reflect.set(u, 5, ${big ? "5n" : "5"}, u)`));
  add(tc(`var u = new ${t}(2); return Reflect.set(u, '5', ${big ? "5n" : "5"})`));
  add(tc(`var u = new ${t}(2); return Reflect.get(u, 5, {})`));
  add(tc(`var u = new ${t}(2); return Reflect.has(u, 5)`));
  add(tc(`var u = new ${t}(2); return Reflect.deleteProperty(u, 0)`));
  add(tc(`var u = new ${t}(2); return Reflect.deleteProperty(u, 5)`));
  add(tc(`var u = new ${t}(2); return Reflect.getOwnPropertyDescriptor(u, 0)`));
  add(tc(`var u = new ${t}(2); return Reflect.getOwnPropertyDescriptor(u, 5)`));
  add(tc(`var u = new ${t}(2); return Reflect.ownKeys(Object.assign(u, {x: 1, [Symbol.iterator]: 1}))`));
  add(tc(`var u = new ${t}(2); u.x = 1; return JSON.stringify(u)`));
  add(tc(`return JSON.stringify({a: new ${t}([${L}])})`));
  add(tc(`return String(new ${t}([${L}]))`));
  add(tc(`return new ${t}([${L}]).toString === Array.prototype.toString`));
  add(tc(`return ${t}.prototype.toString === Object.getPrototypeOf(${t}).prototype.toString`));
  add(tc(`return [${t}.prototype.constructor === ${t}, ${t}.BYTES_PER_ELEMENT, ${t}.prototype.BYTES_PER_ELEMENT, ${t}.length, ${t}.name, Object.getOwnPropertyNames(${t}).sort()]`));
  add(tc(`return ${t}()`));
  add(tc(`return new ${t}(-1)`));
  add(tc(`return new ${t}(2 ** 53)`));
  add(tc(`return new ${t}(1.5).length`));
  add(tc(`return new ${t}('2').length`));
  add(tc(`return new ${t}(null).length`));
  add(tc(`return new ${t}(undefined).length`));
  add(tc(`return new ${t}(true).length`));
  add(tc(`return new ${t}(NaN).length`));
  add(tc(`return new ${t}(Symbol())`));
  add(tc(`return new ${t}(1n)`));
  add(tc(`return new ${t}({}).length`));
  add(tc(`return new ${t}(new ArrayBuffer(8), 1, 1).byteOffset`));
  add(tc(`return new ${t}(new ArrayBuffer(8), undefined, undefined).length`));
  add(tc(`return new ${t}(new ArrayBuffer(8), 0, -1)`));
  add(tc(`return new ${t}(new ArrayBuffer(8), 0, 2 ** 53)`));
  add(tc(`return new ${t}(new ArrayBuffer(8), 2 ** 53)`));
  add(tc(`return new ${t}(new ArrayBuffer(8), 'x').length`));
  add(tc(`var b = new ArrayBuffer(8); b.transfer(); return new ${t}(b)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return new ${t}(u)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return [u.length, u.byteLength, u.byteOffset, u.at(0), Object.keys(u)]`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return [u[0], 0 in u, Object.keys(u).length]`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); u[0] = ${big ? "1n" : "1"}; return u[0]`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return Reflect.defineProperty(u, 0, {value: ${big ? "1n" : "1"}})`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return Reflect.getOwnPropertyDescriptor(u, 0)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return Reflect.deleteProperty(u, 0)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return Reflect.ownKeys(u)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return u.subarray(0)`));
  add(tc(`var u = new ${t}(2); u.buffer.transfer(); return Object.freeze(u)`));
}

// Métodos estáticos, protótipo e coleções (tag, species, descritores).
for (const [name, body] of [
  ["%TypedArray% name", `var T = Object.getPrototypeOf(Int8Array); return [T.name, T.length, typeof T]`],
  ["%TypedArray% call", `var T = Object.getPrototypeOf(Int8Array); return T()`],
  ["%TypedArray% new", `var T = Object.getPrototypeOf(Int8Array); return new T()`],
  ["%TypedArray% species", `var T = Object.getPrototypeOf(Int8Array); return T[Symbol.species] === T`],
  ["%TypedArray% from non-ctor", `var T = Object.getPrototypeOf(Int8Array); return T.from.call({}, [])`],
  ["%TypedArray% of non-ctor", `var T = Object.getPrototypeOf(Int8Array); return T.of.call(function () { return {} }, 1)`],
  ["%TypedArray% from abstract", `var T = Object.getPrototypeOf(Int8Array); return T.from([])`],
  ["of args", `return Int8Array.of(1, 2, 3).join()`],
  ["of none", `return Int8Array.of().length`],
  ["of this", `return Int8Array.of.call(Float64Array, 1.5).join()`],
  ["of this bad", `return Int8Array.of.call(function (n) { return new Int8Array(0) }, 1, 2)`],
  ["of this arr", `return Int8Array.of.call(function (n) { return [] }, 1, 2)`],
  ["from this bad", `return Int8Array.from.call(function (n) { return new Int8Array(0) }, [1, 2])`],
  ["from string", `return Int8Array.from('123').join()`],
  ["from arraylike", `return Int8Array.from({length: 3, 0: 1, 2: 3}).join()`],
  ["from mapfn bad", `return Int8Array.from([1], 5)`],
  ["from mapfn undefined", `return Int8Array.from([1], undefined).join()`],
  ["from mapfn null", `return Int8Array.from([1], null)`],
  ["from this arg", `return Int8Array.from([1], function () { return this.x }, {x: 4}).join()`],
  ["from iter throw", `return Int8Array.from({[Symbol.iterator]() { throw new RangeError('boom') }})`],
  ["from iter non-callable", `return Int8Array.from({[Symbol.iterator]: 1})`],
  ["from iter null", `return Int8Array.from({[Symbol.iterator]: null, length: 1, 0: 5}).join()`],
  ["from generator", `return Int8Array.from((function* () { yield 1; yield 2 })()).join()`],
  ["from typed", `return Float32Array.from(new Int8Array([1, -1])).join()`],
  ["from typed iter patched", `var u = new Int8Array([1, 2]); u[Symbol.iterator] = function* () { yield 9 }; return Int8Array.from(u).join()`],
  ["from typed proto patched", `var old = Int8Array.prototype[Symbol.iterator]; var r; try { Object.getPrototypeOf(Int8Array.prototype)[Symbol.iterator] = function* () { yield 9 }; r = Int8Array.from(new Int8Array([1, 2])).join() } finally { Object.getPrototypeOf(Int8Array.prototype)[Symbol.iterator] = old } return r`],
  ["ctor typed iter patched", `var u = new Int8Array([1, 2]); u[Symbol.iterator] = function* () { yield 9 }; return new Int8Array(u).join()`],
  ["values === iterator", `var P = Object.getPrototypeOf(Int8Array.prototype); return [P.values === P[Symbol.iterator], P.toString === Array.prototype.toString, P[Symbol.toStringTag]]`],
  ["toStringTag getter", `var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array.prototype), Symbol.toStringTag); return [typeof d.get, d.set, d.get.call(1), d.get.call({}), d.get.call(new Uint8Array(1)), d.get.call(new Float16Array(1)), d.get.name]`],
  ["proto getters", `var P = Object.getPrototypeOf(Int8Array.prototype); return ['buffer', 'byteLength', 'byteOffset', 'length'].map(function (k) { var d = Object.getOwnPropertyDescriptor(P, k); return [typeof d.get, d.set, d.get.name, d.configurable, d.enumerable] })`],
  ["proto getter bad this", `var P = Object.getPrototypeOf(Int8Array.prototype); return ['buffer', 'byteLength', 'byteOffset', 'length'].map(function (k) { try { return Object.getOwnPropertyDescriptor(P, k).get.call({}) } catch (e) { return e.name + ': ' + e.message } })`],
  ["proto getter on proto", `var P = Object.getPrototypeOf(Int8Array.prototype); return ['buffer', 'byteLength', 'byteOffset', 'length'].map(function (k) { try { return Int8Array.prototype[k] } catch (e) { return e.name + ': ' + e.message } })`],
  ["proto methods bad this", `var P = Object.getPrototypeOf(Int8Array.prototype); return Object.getOwnPropertyNames(P).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(P, k).value === 'function' && k !== 'constructor' }).map(function (k) { try { P[k].call({}) } catch (e) { return k + ' ' + e.name + ': ' + e.message } return k + ' ok' })`],
  ["proto methods on Int8 proto", `var P = Object.getPrototypeOf(Int8Array.prototype); return Object.getOwnPropertyNames(P).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(P, k).value === 'function' && k !== 'constructor' }).map(function (k) { try { P[k].call(Int8Array.prototype) } catch (e) { return k + ' ' + e.name + ': ' + e.message } return k + ' ok' })`],
  ["proto methods on array", `var P = Object.getPrototypeOf(Int8Array.prototype); return Object.getOwnPropertyNames(P).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(P, k).value === 'function' && k !== 'constructor' }).map(function (k) { try { P[k].call([1]) } catch (e) { return k + ' ' + e.name + ': ' + e.message } return k + ' ok' })`],
  ["proto method lengths", `var P = Object.getPrototypeOf(Int8Array.prototype); return Object.getOwnPropertyNames(P).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(P, k).value === 'function' }).map(function (k) { return k + ':' + P[k].length })`],
  ["proto method names", `var P = Object.getPrototypeOf(Int8Array.prototype); return Object.getOwnPropertyNames(P).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(P, k).value === 'function' }).map(function (k) { return P[k].name })`],
  ["proto own names", `return Object.getOwnPropertyNames(Object.getPrototypeOf(Int8Array.prototype))`],
  ["proto own symbols", `return Object.getOwnPropertySymbols(Object.getPrototypeOf(Int8Array.prototype)).map(String)`],
  ["static own names", `return Object.getOwnPropertyNames(Object.getPrototypeOf(Int8Array)).sort()`],
  ["Int8 proto names", `return Object.getOwnPropertyNames(Int8Array.prototype)`],
  ["Int8 own names", `return Object.getOwnPropertyNames(Int8Array).sort()`],
  ["arraybuffer own names", `return Object.getOwnPropertyNames(ArrayBuffer.prototype).sort()`],
  ["arraybuffer static names", `return Object.getOwnPropertyNames(ArrayBuffer).sort()`],
  ["sab own names", `return Object.getOwnPropertyNames(SharedArrayBuffer.prototype).sort()`],
  ["dataview own names", `return Object.getOwnPropertyNames(DataView.prototype).sort()`],
  ["Math f16", `return typeof Math.f16round`],
  ["Uint8 own names", `return Object.getOwnPropertyNames(Uint8Array).sort()`],
  ["Uint8 proto names", `return Object.getOwnPropertyNames(Uint8Array.prototype).sort()`],
  ["species getter", `var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array), Symbol.species); return [typeof d.get, d.get.name, d.set]`],
  ["species ab", `var d = Object.getOwnPropertyDescriptor(ArrayBuffer, Symbol.species); return [typeof d.get, d.get.name, d.set, ArrayBuffer[Symbol.species] === ArrayBuffer, SharedArrayBuffer[Symbol.species] === SharedArrayBuffer]`],
]) add(tc(body));

// Mensagens de TypeError exatas em chamadas com argumentos inválidos.
for (const t of ["Uint8Array", "BigInt64Array"]) {
  const big = isBig(t);
  const v = big ? "1n" : "1";
  for (const m of ["every", "filter", "find", "findIndex", "findLast", "findLastIndex", "forEach", "map", "some", "reduce", "reduceRight"]) {
    for (const cb of ["undefined", "null", "1", "{}", "'x'", "Symbol()"]) add(tc(`return new ${t}([${v}]).${m}(${cb})`));
  }
  for (const cb of ["null", "1", "{}", "'x'", "Symbol()"]) {
    add(tc(`return new ${t}([${v}]).sort(${cb})`));
    add(tc(`return new ${t}([${v}]).toSorted(${cb})`));
  }
  add(tc(`return new ${t}(0).reduce(function () {})`));
  add(tc(`return new ${t}(0).reduceRight(function () {})`));
  add(tc(`return new ${t}(0).reduce(function () {}, 5)`));
  add(tc(`return new ${t}([${v}]).reduce(function (a) { return a })`));
  add(tc(`return new ${t}([${v}, ${v}]).reduce(function (a, b, i, arr) { return [a, b, i, arr.length] })`));
  add(tc(`return new ${t}([${v}, ${v}]).reduceRight(function (a, b, i, arr) { return [a, b, i, arr.length] })`));
  add(tc(`return new ${t}([${v}]).forEach(function () { return 1 })`));
  add(tc(`new ${t}([${v}]).forEach(function () { l = this }, 5); return typeof l`));
  add(tc(`'use strict'; var l; new ${t}([${v}]).forEach(function () { l = this }, 5); return typeof l`));
  add(tc(`return new ${t}([${v}]).map(function () { return {} })`));
  add(tc(`return new ${t}([${v}]).map(function () { return '${v}' })`));
  add(tc(`return new ${t}([${v}]).map(function () { return ${big ? "1" : "1n"} })`));
  add(tc(`return new ${t}(3).fill(${big ? "1" : "1n"})`));
  add(tc(`return new ${t}(3).fill()`));
  add(tc(`return new ${t}(3).fill(${v}, 1, 1)`));
  add(tc(`return new ${t}(3).fill(Symbol())`));
  add(tc(`return new ${t}(3).sort(function () { throw new RangeError('x') })`));
  add(tc(`return new ${t}([3, 1, 2].map(function (x) { return ${big ? "BigInt(x)" : "x"} })).sort(function (a, b) { return ${big ? "a < b ? -1 : 1" : "a - b"} }).join()`));
  add(tc(`return new ${t}([${v}, ${v}]).sort(function () { return NaN }).join()`));
  add(tc(`return new ${t}([${v}, ${v}]).sort(function () { return Symbol() })`));
  add(tc(`return new ${t}([${v}, ${v}]).sort(function () { return 1n })`));
  add(tc(`return new ${t}([${v}, ${v}]).sort(function () { return {valueOf() { return -1 }} }).join()`));
  add(tc(`return new ${t}([${v}, ${v}]).sort(function () { return '-1' }).join()`));
  add(tc(`var c = 0; new ${t}([${v}, ${v}, ${v}, ${v}]).sort(function () { c++; return 0 }); return c > 0`));
  add(tc(`var u = new ${t}([${v}, ${v}]); u.sort(function () { u.buffer.transfer(); return 0 }); return u.length`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.toSorted(function () { u.buffer.transfer(); return 0 }).length`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.map(function () { u.buffer.transfer(); return ${v} }).length`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.filter(function () { u.buffer.transfer(); return true }).length`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.forEach(function () { u.buffer.transfer() })`));
  add(tc(`var u = new ${t}([${v}, ${v}]); var o = []; u.forEach(function (x) { o.push(x); u.buffer.transfer() }); return o`));
  add(tc(`var u = new ${t}([${v}, ${v}]); var o = []; u.find(function (x) { o.push(x); u.buffer.transfer() }); return o`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.reduce(function (a, x) { u.buffer.transfer(); return String(x) }, '')`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.slice({valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.subarray({valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.fill(${v}, {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.fill({valueOf() { u.buffer.transfer(); return ${big ? "1n" : "1"} }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.copyWithin({valueOf() { u.buffer.transfer(); return 0 }}, 1)`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.includes(${v}, {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.includes(undefined, {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.indexOf(${v}, {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.lastIndexOf(${v}, {valueOf() { u.buffer.transfer(); return 1 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.join({toString() { u.buffer.transfer(); return '-' }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.at({valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.with({valueOf() { u.buffer.transfer(); return 0 }}, ${v})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.with(0, {valueOf() { u.buffer.transfer(); return ${big ? "1n" : "1"} }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.set([${v}], {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); return u.set([{valueOf() { u.buffer.transfer(); return ${big ? "1n" : "1"} }}], 0)`));
  add(tc(`var u = new ${t}([${v}, ${v}]); var d = new ${t}(2); return d.set(u, {valueOf() { u.buffer.transfer(); return 0 }})`));
  add(tc(`var u = new ${t}([${v}, ${v}]); u.constructor = {[Symbol.species]: function (n) { u.buffer.transfer(); return new ${t}(n) }}; return u.slice(0, 1)`));
  add(tc(`var u = new ${t}([${v}, ${v}]); u.constructor = {[Symbol.species]: function (n) { u.buffer.transfer(); return new ${t}(n) }}; return u.subarray(0, 1)`));
  add(tc(`var u = new ${t}([${v}, ${v}]); u.constructor = {[Symbol.species]: function (n) { u.buffer.transfer(); return new ${t}(n) }}; return u.map(function (x) { return x }).length`));
  add(tc(`var u = new ${t}([${v}, ${v}]); u.constructor = {[Symbol.species]: function (n) { u.buffer.transfer(); return new ${t}(n) }}; return u.filter(function (x) { return true }).length`));
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
// A geração combinatória passa de 10 mil programas; guarda um quarto por hash do programa (sampleByHash, determinístico)
// para o teste Rust, que roda cada um em realm novo, ficar em alguns minutos.
const sampled = sampleByHash(lines, Math.ceil(lines.length / 4), (line) => line.slice(0, line.indexOf("\t")));
if (sampled.length < 2000) throw new Error(`só ${sampled.length} programas`);
fs.writeSync(1, sampled.join("\n") + "\n");
process.exit(0);
