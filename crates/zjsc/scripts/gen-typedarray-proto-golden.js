// Gera tests/golden/typedarray_proto_bun.tsv: TypedArray.prototype e ArrayBuffer.prototype, medido no bun 1.4.2.
// Complementa gen-typedarray-golden.js, gen-typedarray-edge-golden.js e gen-typedarray-more-golden.js com grades de
// set com sobreposição de buffer entre tipos diferentes, subarray, sort com comparadores hostis, toSorted/toReversed/with,
// fill/copyWithin com índices negativos, from/of, BigInt64Array, ArrayBuffer redimensionável com length-tracking,
// detach via transfer, Float16Array, e slice/resize/transfer/transferToFixedLength/detached do ArrayBuffer.
// Cada programa roda num processo bun novo, sem APIs de host, e a saída é o texto da global `globalThis.R`.
// Programas que já aparecem nos goldens typedarray_*.tsv e buffer*.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor de `R` (JSON), igual a gen-array-more-golden.js.
// O prelúdio comum sai em tests/golden/typedarray_proto.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-typedarray-proto-golden.js > tests/golden/typedarray_proto_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitFactored, readRows, GOLDEN_DIR } = require("./golden-prelude.js");

// Programas completos dos goldens vizinhos (o tsv novo guarda só o sufixo, readRows recompõe o prelúdio).
const existingSources = new Set(
  fs
    .readdirSync(GOLDEN_DIR)
    .filter((name) => /^(typedarray|buffer)[a-z_]*\.tsv$/.test(name))
    .flatMap((name) => readRows(name.replace(/(_bun)?\.tsv$/, ""), fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8")).map((row) => row.source)),
);

const PRELUDE = [
  "function S(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'function') return 'fn';",
  "  if (v === null || typeof v !== 'object') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (d > 3) return '...';",
  "  if (ArrayBuffer.isView(v) && !(v instanceof DataView)) {",
  "    var o = [];",
  "    for (var i = 0; i < v.length; i++) o.push(S(v[i], d + 1));",
  "    return v.constructor.name + '(' + v.length + ')@' + v.byteOffset + '[' + o.join(',') + ']';",
  "  }",
  "  if (v instanceof ArrayBuffer) return 'AB(' + v.byteLength + (v.resizable ? ',max' + v.maxByteLength : '') + (v.detached ? ',detached' : '') + ')[' + Array.from(new Uint8Array(v.detached ? 0 : v.byteLength)).join(',') + ']';",
  "  if (Array.isArray(v)) return '[' + v.map(function (x) { return S(x, d + 1); }).join(',') + ']';",
  "  return '{' + Object.keys(v).map(function (k) { return k + ':' + S(v[k], d + 1); }).join(',') + '}';",
  "}",
  "function T(f) { try { globalThis.R = S(f()); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }",
].join("\n");

const bodies = [];
const P = (body) => bodies.push(`T(function () { ${body} });`);

const NUM = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array"];
const BIG = ["BigInt64Array", "BigUint64Array"];
const ALL = [...NUM, ...BIG];
const isBig = (t) => t.startsWith("Big");
const sz = (t) => ({ Int8Array: 1, Uint8Array: 1, Uint8ClampedArray: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, Float16Array: 2, Float32Array: 4, Float64Array: 8, BigInt64Array: 8, BigUint64Array: 8 })[t];
const lit = (t, vs) => vs.map((v) => (isBig(t) ? `${v}n` : `${v}`)).join(",");
const ser = (name) => `S(${name})`;

// ---- 1. set com sobreposição de buffer entre tipos diferentes.
const setPairs = [["Uint8Array", "Uint8Array"], ["Uint8Array", "Uint16Array"], ["Uint16Array", "Uint8Array"], ["Int8Array", "Uint8Array"], ["Uint8Array", "Int8Array"],
  ["Uint8Array", "Float32Array"], ["Float32Array", "Uint8Array"], ["Uint16Array", "Float32Array"], ["Float32Array", "Int16Array"], ["Int32Array", "Float64Array"],
  ["Float64Array", "Int32Array"], ["Uint8ClampedArray", "Int16Array"], ["Int16Array", "Uint8ClampedArray"], ["Float16Array", "Uint8Array"], ["Uint8Array", "Float16Array"],
  ["Uint32Array", "Uint16Array"], ["Uint16Array", "Uint32Array"], ["BigInt64Array", "BigUint64Array"], ["BigUint64Array", "BigInt64Array"], ["Float32Array", "Float32Array"]];
for (const [dt, st] of setPairs) {
  const sd = sz(dt), ss = sz(st);
  for (const [dOff, sOff, n, at] of [[0, 0, 3, 0], [0, 1, 3, 0], [1, 0, 3, 0], [1, 0, 3, 1], [0, 2, 2, 1], [2, 0, 2, 0], [0, 1, 2, 2], [1, 1, 3, 0]]) {
    P(`var b = new ArrayBuffer(64); var u = new Uint8Array(b); for (var i = 0; i < 64; i++) u[i] = i * 7 + 3; var d = new ${dt}(b, ${dOff * sd}, 6); var s = new ${st}(b, ${sOff * ss}, ${n}); d.set(s, ${at}); return [u, d];`);
  }
  P(`var b = new ArrayBuffer(32); var u = new Uint8Array(b); for (var i = 0; i < 32; i++) u[i] = i + 1; var d = new ${dt}(b); d.set(new ${st}(b, ${ss}), 1); return u;`);
  P(`var b = new ArrayBuffer(32); var d = new ${dt}(b); var s = new ${st}(b); return d.set(s, 1);`);
}
for (const t of ALL) {
  const v = lit(t, [1, 2, 3]);
  for (const off of [0, 1, 2, -1, 3, 4, 1.5, "'1'", "undefined", "null", "NaN", "Infinity", "-0", "{valueOf(){return 1}}"]) {
    P(`var a = new ${t}(4); a.set([${v}], ${off}); return a;`);
    P(`var a = new ${t}(4); a.set(new ${t}([${v}]), ${off}); return a;`);
  }
  P(`var a = new ${t}(3); a.set({length: 2, 0: ${isBig(t) ? "7n" : "7"}, 1: ${isBig(t) ? "8n" : "8"}}); return a;`);
  P(`var a = new ${t}(3); a.set('12'); return a;`);
  P(`var a = new ${t}(3); a.set(5); return a;`);
  P(`var a = new ${t}(3); a.set(null); return a;`);
  P(`var a = new ${t}(3); a.set(); return a;`);
  P(`var a = new ${t}(3); a.set({length: -1}); return a;`);
  P(`var a = new ${t}(3); var log = []; a.set({get length() { log.push('l'); return 2 }, get 0() { log.push(0); return ${isBig(t) ? "1n" : "1"} }, get 1() { log.push(1); return ${isBig(t) ? "2n" : "2"} }}); return [a, log];`);
  P(`var a = new ${t}(3); a.set([${isBig(t) ? "1" : "1n"}]); return a;`);
  P(`var a = new ${t}(3); a.set(new ${isBig(t) ? "Int8Array" : "BigInt64Array"}(1)); return a;`);
}
P(`var a = new Uint8Array(4); a.set(new Uint8Array(5)); return a;`);
P(`var a = new Uint8Array(4); a.set([1, 2], 3); return a;`);
P(`var a = new Uint8Array(4); a.set([1, 2], -1); return a;`);
P(`var a = new Uint8Array(4); a.set([], 4); return a;`);
P(`var a = new Uint8Array(4); a.set([], 5); return a;`);
P(`var a = new Uint8Array(4); a.set([1, {valueOf() { a.buffer.transfer(); return 5 }}]); return a;`);

// ---- 2. subarray.
const idx = ["undefined", "0", "1", "2", "-1", "-2", "-9", "9", "NaN", "Infinity", "-Infinity", "1.9", "'1'", "null", "{valueOf(){return 2}}", "-0"];
for (const t of ["Uint8Array", "Int16Array", "Float32Array", "Float64Array", "BigInt64Array"]) {
  const v = lit(t, [10, 20, 30, 40, 50]);
  for (const b of idx) for (const e of idx) {
    P(`var a = new ${t}([${v}]); var s = a.subarray(${b}, ${e}); return [s, s.byteOffset, s.buffer === a.buffer];`);
  }
}
for (const t of ALL) {
  P(`var a = new ${t}(8); var s = a.subarray(2, 6); s[0] = ${isBig(t) ? "5n" : "5"}; return [a, s.byteOffset, s.length, s.byteLength];`);
  P(`var a = new ${t}(8); return a.subarray(2).subarray(1, -1);`);
  P(`class X extends ${t} {} var a = new X(4); var s = a.subarray(1); return [s instanceof X, s.constructor.name];`);
  P(`class X extends ${t} { static get [Symbol.species]() { return ${t}; } } var a = new X(4); var s = a.subarray(1); return [s instanceof X, s.constructor.name];`);
  P(`var a = new ${t}(4); a.constructor = {[Symbol.species]: function () { return 1 }}; return a.subarray(1);`);
  P(`var a = new ${t}(4); a.constructor = undefined; return a.subarray(1).constructor.name;`);
  P(`var a = new ${t}(4); a.constructor = 1; return a.subarray(1);`);
  // Caminho rápido de speciesWatchpointIsValid: subclasse, `constructor` trocado na instância e no protótipo, @@species trocado.
  const z = isBig(t) ? "0n" : "0";
  P(`class X extends ${t} {} var a = new X(4); var s = a.slice(1); var m = a.map(function (v) { return v; }); var f = a.filter(function () { return true; }); return [s instanceof X, m instanceof X, f instanceof X, s.length, m.length, f.length];`);
  P(`class X extends ${t} { static get [Symbol.species]() { return ${t}; } } var a = new X(4); return [a.slice(1) instanceof X, a.map(function (v) { return v; }) instanceof X, a.filter(function () { return true; }).constructor.name];`);
  P(`var a = new ${t}(4); a.constructor = ${t}; var s = a.slice(1); return [s.constructor === ${t}, s.length];`);
  P(`var a = new ${t}(4); a.constructor = {[Symbol.species]: function (n) { return new ${t}(n + 1); }}; return [a.slice(1).length, a.map(function (v) { return v; }).length];`);
  P(`var a = new ${t}(4); a.constructor = undefined; return [a.slice(1).constructor.name, a.filter(function () { return true; }).length];`);
  P(`var a = new ${t}(4); var before = a.slice(1).length; ${t}.prototype.constructor = {[Symbol.species]: function (n) { return new ${t}(n + 2); }}; return [before, a.slice(1).length, a.map(function (v) { return v; }).length];`);
  P(`var a = new ${t}(4); var before = a.slice(1).length; Object.defineProperty(${t}, Symbol.species, {value: function (n) { return new ${t}(n + 3); }, configurable: true}); return [before, a.slice(1).length, a.filter(function () { return true; }).length];`);
  P(`var a = new ${t}(4); var before = a.slice(1).length; var TA = Object.getPrototypeOf(${t}); Object.defineProperty(TA, Symbol.species, {get() { return function (n) { return new ${t}(n + 4); }; }, configurable: true}); return [before, a.slice(1).length, a.map(function (v) { return v; }).length];`);
  P(`var a = new ${t}(4); a.x = ${z}; return [a.slice(1).length, a.slice(1) instanceof ${t}];`);
  P(`var a = new ${t}(4); var before = a.slice(1).length; Object.setPrototypeOf(a, {__proto__: ${t}.prototype, constructor: {[Symbol.species]: function (n) { return new ${t}(n + 5); }}}); return [before, a.slice(1).length];`);
}
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var a = new Uint8Array(b); var s = a.subarray(2); b.resize(12); return [a.length, s.length];`);
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var a = new Uint8Array(b); var s = a.subarray(2, 6); b.resize(12); return [a.length, s.length];`);
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var a = new Uint8Array(b, 4); var s = a.subarray(1); b.resize(2); return [a.length, s.length, a.byteOffset, s.byteOffset];`);
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var a = new Uint8Array(b, 0, 4); b.resize(2); return a.subarray(1);`);
P(`var a = new Uint8Array(4); a.buffer.transfer(); return a.subarray(1);`);

// ---- 3. sort com comparadores.
const cmps = ["undefined", "function (a, b) { return a - b }", "function (a, b) { return b - a }", "function () { return NaN }", "function () { return 0 }", "function () { return -1 }",
  "function () { return 1 }", "function (a, b) { return a < b ? -1 : 1 }", "function () { return '1' }", "function () { return {valueOf() { return -1 }} }", "function () { return undefined }",
  "function () { return null }", "function () { return true }", "function () { return -Infinity }", "function () { return -0 }", "function () { throw new RangeError('c') }",
  "null", "1", "{}", "'x'", "true", "Symbol()", "class {}", "function* () {}", "async function () { return 0 }", "Math.max", "(a, b) => b > a", "(a, b) => a > b ? 1 : -1"];
const sortInputs = ["3, 1, 2", "1", "", "2, 2, 1, 1", "5, -1, 0, -0, 3", "NaN, 1, -Infinity, Infinity, -0, 0, NaN", "1.5, 1.25, 1.75", "100, 9, 80, 1"];
for (const c of cmps) {
  for (const t of ["Float64Array", "Int8Array", "Uint16Array", "BigInt64Array"]) {
    for (const input of sortInputs.slice(0, 4)) {
      const vs = input.split(",").filter((x) => x.trim()).map((x) => x.trim());
      const lv = isBig(t) ? vs.filter((x) => /^-?\d+$/.test(x)).map((x) => x + "n").join(",") : vs.join(",");
      P(`var a = new ${t}([${lv}]); var r = a.sort(${c}); return [r === a, a];`);
      P(`var a = new ${t}([${lv}]); return a.toSorted(${c});`);
    }
  }
}
for (const input of sortInputs) {
  for (const t of ["Float64Array", "Float32Array", "Float16Array"]) {
    P(`var a = new ${t}([${input}]); a.sort(); return a;`);
    P(`var a = new ${t}([${input}]); return a.toSorted((x, y) => y - x);`);
    P(`var a = new ${t}([${input}]); return a.toSorted().toReversed();`);
    P(`var a = new ${t}([${input}]); a.sort(); return a.map((x) => Object.is(x, -0) ? 'm0' : x);`);
  }
}
P(`var a = new Uint8Array([3, 1, 2]); a.sort(function (x, y) { a[0] = 99; return x - y }); return a;`);
P(`var a = new Uint8Array([3, 1, 2]); a.sort(function (x, y) { a.buffer.transfer(); return x - y }); return a.length;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([4, 3, 2, 1]); a.sort(function (x, y) { b.resize(2); return x - y }); return [a, new Uint8Array(b)];`);
P(`var a = new Uint8Array([3, 1, 2]); var calls = 0; a.sort(function (x, y) { calls++; return x - y }); return calls > 0;`);
P(`var a = new Uint8Array([3, 1, 2]); return a.toSorted(function (x, y) { a[1] = 50; return x - y });`);
P(`var a = new Uint8Array([3, 1, 2]); return Uint8Array.prototype.sort.call([3, 1, 2]);`);
P(`return Uint8Array.prototype.toSorted.call(new Int8Array([3, 1, 2]), 0);`);
P(`var o = {s: 0}; var a = new Int8Array([3, 1, 2]); a.sort({}.valueOf); return a;`);
P(`var order = []; var a = new Int8Array([3, 1, 2]); try { a.sort(5) } catch (e) { order.push(e.name) } return order;`);
P(`var a = new Int8Array(0); return [a.sort(5), 1];`);
P(`var a = new Int8Array(0); return a.toSorted(5);`);
P(`return new Int8Array(1).toSorted(5);`);

// ---- 4. toSorted, toReversed, with.
for (const t of ALL) {
  const v = lit(t, [4, 1, 3]);
  P(`var a = new ${t}([${v}]); var r = a.toReversed(); return [r, r === a, r.buffer === a.buffer, a];`);
  P(`var a = new ${t}([${v}]); var r = a.toSorted(); return [r, a, r.constructor === a.constructor];`);
  P(`class X extends ${t} {} var a = new X([${v}]); var r = a.toReversed(); return [r.constructor.name, r instanceof X];`);
  P(`class X extends ${t} {} var a = new X([${v}]); return [a.toSorted().constructor.name, a.with(0, ${isBig(t) ? "1n" : "1"}).constructor.name];`);
  P(`return new ${t}(0).toReversed();`);
  for (const i of [0, 1, 2, 3, -1, -2, -3, -4, 1.9, -1.1, "'1'", "'x'", "NaN", "undefined", "null", "Infinity", "-Infinity", "{valueOf(){return 1}}", "2**32", "-0", "true"]) {
    P(`var a = new ${t}([${v}]); var r = a.with(${i}, ${isBig(t) ? "9n" : "9"}); return [r, a];`);
  }
  for (const val of ["undefined", "null", "'5'", "1.5", "-1", "300", "{valueOf(){return 7}}", "true", "Symbol()", "10n", "NaN", "2**40", "-(2**31)-1"]) {
    P(`var a = new ${t}([${v}]); return a.with(1, ${val});`);
  }
  P(`var a = new ${t}([${v}]); return a.with();`);
  P(`var a = new ${t}([${v}]); return a.with(1);`);
  P(`var order = []; var a = new ${t}([${v}]); try { a.with({valueOf() { order.push('i'); return 9 }}, {valueOf() { order.push('v'); return ${isBig(t) ? "1n" : "1"} }}) } catch (e) { order.push(e.name) } return order;`);
  P(`var a = new ${t}([${v}]); return a.with({valueOf() { a.buffer.transfer(); return 0 }}, ${isBig(t) ? "1n" : "1"});`);
  P(`var a = new ${t}([${v}]); return a.with(0, {valueOf() { a.buffer.transfer(); return ${isBig(t) ? "1n" : "1"} }});`);
}
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.with({valueOf() { b.resize(2); return 3 }}, 9);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.with(1, {valueOf() { b.resize(2); return 9 }});`);

// ---- 5. fill e copyWithin com negativos.
const fidx = ["undefined", "0", "1", "3", "-1", "-3", "-9", "9", "NaN", "Infinity", "-Infinity", "1.5", "'2'", "null", "-0"];
for (const t of ["Uint8Array", "Int16Array", "Float32Array", "BigUint64Array"]) {
  const v = lit(t, [1, 2, 3, 4, 5, 6]);
  const f = isBig(t) ? "9n" : "9";
  for (const s of fidx) for (const e of fidx) {
    P(`var a = new ${t}([${v}]); var r = a.fill(${f}, ${s}, ${e}); return [r === a, a];`);
  }
  for (const tg of fidx.slice(0, 11)) for (const s of fidx.slice(0, 11)) for (const e of ["undefined", "-1", "4", "-3", "Infinity"]) {
    P(`var a = new ${t}([${v}]); return a.copyWithin(${tg}, ${s}, ${e});`);
  }
}
for (const t of ALL) {
  for (const val of ["undefined", "null", "'7'", "1.7", "-1", "256", "257", "-129", "65537", "2**32+3", "NaN", "Infinity", "-Infinity", "{valueOf(){return 3}}", "true", "Symbol()", "5n", "-1n", "2n**64n+5n", "2n**63n"]) {
    P(`var a = new ${t}(3); return a.fill(${val});`);
  }
  P(`var a = new ${t}(3); return a.fill();`);
  P(`var a = new ${t}(3); var order = []; try { a.fill({valueOf() { order.push('v'); return ${isBig(t) ? "1n" : "1"} }}, {valueOf() { order.push('s'); return 0 }}, {valueOf() { order.push('e'); return 1 }}) } catch (e) { order.push(e.name) } return [order, a];`);
  P(`var a = new ${t}(3); return a.fill(${isBig(t) ? "1n" : "1"}, {valueOf() { a.buffer.transfer(); return 0 }});`);
  P(`var a = new ${t}(6); return a.fill(${isBig(t) ? "1n" : "1"}, 1, 3).copyWithin(3, 0);`);
  P(`return new ${t}(0).copyWithin(0, 0);`);
  P(`var a = new ${t}(4); return a.copyWithin({valueOf() { a.buffer.transfer(); return 0 }}, 1);`);
}
P(`var b = new ArrayBuffer(6, {maxByteLength: 12}); var a = new Uint8Array(b); a.set([1, 2, 3, 4, 5, 6]); return a.copyWithin(0, {valueOf() { b.resize(3); return 2 }});`);
P(`var b = new ArrayBuffer(6, {maxByteLength: 12}); var a = new Uint8Array(b); return a.fill(1, 0, {valueOf() { b.resize(3); return 6 }});`);
P(`var b = new ArrayBuffer(6, {maxByteLength: 12}); var a = new Uint8Array(b); return a.fill({valueOf() { b.resize(3); return 1 }});`);

// ---- 6. from e of.
for (const t of ALL) {
  const one = isBig(t) ? "1n" : "1";
  P(`return ${t}.of(${isBig(t) ? "1n, 2n, 3n" : "1, 2, 3"});`);
  P(`return ${t}.of();`);
  P(`return ${t}.of(${isBig(t) ? "1" : "1n"});`);
  P(`return ${t}.of.call(Object, ${one});`);
  P(`return ${t}.of.call(function () { return new ${t}(1) }, ${one}, ${one});`);
  P(`return ${t}.of.call(function () { return new ${t}(2) }, ${one}, ${one});`);
  P(`return ${t}.of.call(function () { return new ${t === "Int8Array" ? "Uint8Array" : "Int8Array"}(2) }, 1, 2);`);
  P(`return ${t}.of.call(undefined, ${one});`);
  P(`return ${t}.from([${lit(t, [1, 2, 3])}]);`);
  P(`return ${t}.from(new Set([${lit(t, [1, 2, 2])}]));`);
  P(`return ${t}.from({length: 2, 0: ${one}, 1: ${one}});`);
  P(`return ${t}.from({length: 2});`);
  P(`return ${t}.from('12');`);
  P(`return ${t}.from(5);`);
  P(`return ${t}.from(null);`);
  P(`return ${t}.from();`);
  P(`return ${t}.from([${one}], null);`);
  P(`return ${t}.from([${one}], 5);`);
  P(`return ${t}.from([${lit(t, [1, 2])}], function (x, i) { return x + ${isBig(t) ? "BigInt(i)" : "i"} });`);
  P(`var th; ${t}.from([${one}], function () { th = this; return ${one} }, 'q'); return typeof th;`);
  P(`var th; ${t}.from([${one}], function () { 'use strict'; th = this; return ${one} }, 'q'); return th;`);
  P(`return ${t}.from([${one}], function () { return ${isBig(t) ? "1" : "1n"} });`);
  P(`var log = []; ${t}.from({length: 2, 0: ${one}, 1: ${one}}, function (x, i) { log.push(i, arguments.length); return x }); return log;`);
  P(`var it = {[Symbol.iterator]() { var n = 0; return {next() { return n++ < 2 ? {value: ${one}, done: false} : {done: true} }, return() { log.push('r'); return {} }} }}; var log = []; ${t}.from(it, function () { throw 1 }); return log;`);
  P(`var it = {[Symbol.iterator]: null, length: 1, 0: ${one}}; return ${t}.from(it);`);
  P(`var it = {[Symbol.iterator]: 5}; return ${t}.from(it);`);
  P(`var it = {[Symbol.iterator]: undefined, length: 2, 0: ${one}}; return ${t}.from(it);`);
  P(`return ${t}.from(new ${t}([${lit(t, [1, 2])}]));`);
  P(`return ${t}.from.call(Object, [${one}]);`);
  P(`return ${t}.from.call(function (n) { return new ${t}(n + 1) }, [${one}, ${one}]);`);
  P(`return ${t}.from.call(function (n) { return new ${t}(1) }, [${one}, ${one}]);`);
  P(`class X extends ${t} {} var r = X.from([${one}]); return [r.constructor.name, r];`);
  P(`class X extends ${t} {} var r = X.of(${one}); return [r.constructor.name, r];`);
}
P(`return Object.getPrototypeOf(Uint8Array).from.call(Uint8Array, [1, 2]);`);
P(`return Object.getPrototypeOf(Uint8Array).from([1]);`);
P(`return Object.getPrototypeOf(Uint8Array).of(1);`);
P(`return new (Object.getPrototypeOf(Uint8Array))();`);
P(`return Object.getPrototypeOf(Uint8Array).prototype.constructor === Object.getPrototypeOf(Uint8Array);`);

// ---- 7. BigInt64Array e BigUint64Array.
const bigVals = ["0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "2n**64n", "2n**64n-1n", "2n**64n+1n", "-(2n**63n)-1n", "2n**100n", "-(2n**100n)", "0x7fffffffffffffffn", "0xffffffffffffffffn"];
for (const t of BIG) {
  for (const v of bigVals) {
    P(`var a = new ${t}(1); a[0] = ${v}; return a[0];`);
    P(`return new ${t}([${v}]);`);
    P(`var a = new ${t}(2); a.fill(${v}); return [a, a[1] === a[0]];`);
    P(`var a = new ${t}([${v}, 1n]); return [a.indexOf(${v}), a.includes(${v}), a.lastIndexOf(${v}), a.at(-2)];`);
    P(`var a = new ${t}(1); a[0] = ${v}; return [new DataView(a.buffer).getBigInt64(0, true), new DataView(a.buffer).getBigUint64(0, true)];`);
    P(`var a = new ${t}([${v}]); return a.map((x) => x + 1n);`);
    P(`var a = new ${t}([${v}, 2n, 1n]); return [a.toSorted(), a.toSorted((x, y) => (x < y ? 1 : x > y ? -1 : 0)), a.reduce((p, c) => p + c, 0n)];`);
  }
  for (const bad of ["1", "1.5", "'1'", "true", "null", "undefined", "Symbol()", "{}", "[]", "NaN", "'x'", "''", "'0x10'", "' 5 '", "'1n'", "{valueOf(){return 3n}}", "{valueOf(){return 3}}", "[1n]"]) {
    P(`var a = new ${t}(1); a[0] = ${bad}; return a[0];`);
    P(`return new ${t}([${bad}]);`);
    P(`var a = new ${t}(2); return a.fill(${bad});`);
  }
  P(`var a = new ${t}(2); return a.indexOf(1);`);
  P(`var a = new ${t}([1n, 2n]); return [a.indexOf(1), a.includes(2), a.join('-'), a.toString(), a.toLocaleString()];`);
  P(`var a = new ${t}([1n, 2n]); return a.map((x) => 1);`);
  P(`var a = new ${t}([1n, 2n]); return a.filter((x) => x > 1n);`);
  P(`var a = new ${t}([1n, 2n, 3n]); return [a.find((x) => x > 1n), a.findIndex((x) => x > 1n), a.findLast((x) => x < 3n), a.findLastIndex((x) => x < 3n), a.some((x) => x == 2), a.every((x) => x > 0n)];`);
  P(`return new ${t}(new ${t === "BigInt64Array" ? "BigUint64Array" : "BigInt64Array"}([-1n, 1n]));`);
  P(`return new ${t}(new Int8Array(2));`);
  P(`return ${t}.BYTES_PER_ELEMENT + ${t}.prototype.BYTES_PER_ELEMENT;`);
  P(`return new ${t}(new ArrayBuffer(12));`);
  P(`return new ${t}(new ArrayBuffer(16), 4);`);
  P(`return new ${t}(new ArrayBuffer(16), 8, 1);`);
  P(`var a = new ${t}(3); return Object.keys(a).concat(JSON.stringify(Object.entries(a), (k, v) => typeof v === 'bigint' ? v + 'n' : v));`);
  P(`return JSON.stringify(new ${t}(1));`);
  P(`var a = new ${t}([3n, 1n, 2n]); return Array.from(a).sort();`);
  P(`var a = new ${t}([3n, 1n, 2n]); return [...a.entries()].map((e) => e.join(':'));`);
  P(`return Atomics.add(new ${t}(1), 0, 5n);`);
  P(`return Atomics.add(new ${t}(1), 0, 5);`);
}
P(`return [new BigInt64Array([2n**63n])[0], new BigUint64Array([-1n])[0]];`);
P(`var a = new BigInt64Array(2); a.set([1n, 2n]); a.set(new BigInt64Array([3n]), 1); return a;`);
P(`var a = new BigInt64Array(2); a.set(new Uint8Array(1));`);
P(`var a = new BigUint64Array(2); a.set(new BigInt64Array([-1n, 5n])); return a;`);
P(`var a = new BigInt64Array(2); a.set(new BigUint64Array([2n**64n-1n, 2n**63n])); return a;`);

// ---- 8. ArrayBuffer redimensionável com length-tracking.
const resizeTo = [0, 1, 2, 3, 4, 7, 8, 9, 12, 15, 16];
for (const t of ["Uint8Array", "Int16Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "Float16Array"]) {
  const s = sz(t);
  for (const n of resizeTo) {
    for (const [name, ctor] of [["track", `new ${t}(b)`], ["trackOff", `new ${t}(b, ${s})`], ["fixed", `new ${t}(b, 0, 2)`], ["fixedOff", `new ${t}(b, ${s}, 1)`]]) {
      P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = ${ctor}; try { b.resize(${n * s}); } catch (e) { return e.name } return [a.length, a.byteLength, a.byteOffset, a.byteLength === 0 ? 0 : 1];`);
    }
  }
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); b.resize(${8 * s}); return [a.length, a.at(-1), a.slice(-2).length, a.subarray(1).length, a.toReversed().length, a.with(0, ${isBig(t) ? "1n" : "1"}).length];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); var out = []; for (var x of a) { out.push(x); if (out.length === 2) b.resize(${6 * s}); if (out.length > 20) break } return out.length;`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); var out = []; for (var x of a) { out.push(x); if (out.length === 2) b.resize(${s}); } return out.length;`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); return [a.length, a.byteLength, a.byteOffset, Object.keys(a), a.at(0), a[0], 0 in a, a.join()];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); return a.map((x) => x);`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); return a.fill(${isBig(t) ? "1n" : "1"});`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); return a.entries().next();`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); return a.slice();`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, 0, 2); b.resize(${s}); b.resize(${4 * s}); return [a.length, a.byteOffset];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); var it = a.values(); it.next(); b.resize(0); return it.next();`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); var it = a.keys(); b.resize(${s}); return [...it];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, ${8 * s});`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b, ${4 * s}); return [a.length, a.byteLength];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); return Object.getOwnPropertyDescriptor(${t}.prototype.__proto__, 'length').get.call(new ${t}(b, ${2 * s}));`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); return new ${t}(new ${t}(b));`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); var c = new ${t}(a); b.resize(${8 * s}); return [a.length, c.length, c.buffer.resizable];`);
  P(`var b = new ArrayBuffer(${4 * s}, {maxByteLength: ${16 * s}}); var a = new ${t}(b); return [a.slice().buffer.resizable, a.map((x) => x).buffer.resizable, a.toSorted().buffer.resizable, a.subarray(1).buffer === b];`);
}
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); b.resize(6); return a;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); b.resize(2); b.resize(4); return a;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.slice({valueOf() { b.resize(1); return 0 }});`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.map(function (x, i) { if (i === 0) b.resize(2); return x * 2 });`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); var seen = []; a.forEach(function (x, i) { if (i === 0) b.resize(2); seen.push(x) }); return seen;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.reduce(function (p, x, i) { if (i === 1) b.resize(2); return p + x }, 0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.filter(function (x, i) { if (i === 0) b.resize(2); return true });`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.find(function (x, i) { if (i === 0) b.resize(2); return x === 4 });`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.indexOf(4, {valueOf() { b.resize(2); return 0 }});`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.includes(undefined, {valueOf() { b.resize(2); return 3 }});`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.join({toString() { b.resize(2); return '-' }});`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); a.set([1, 2, 3, 4]); return a.set([9, 9], {valueOf() { b.resize(3); return 2 }});`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 0, 4); b.resize(8); return a;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 0, 4); b.resize(3); return [a.length, Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), 'byteLength').get.call(a)];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return [a.length, a.byteLength, a.byteOffset];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(2); return [a.length, a.byteLength, a.byteOffset];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.at(0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Array.prototype.slice.call(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Object.getOwnPropertyNames(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return JSON.stringify(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return [...a];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return new Uint8Array(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.sort();`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.toSorted();`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.with(0, 1);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.toReversed();`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.reverse();`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.subarray(0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.copyWithin(0, 0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.set([]);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.keys().next();`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a.every(() => true);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); a[0] = 1; return a[0];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Object.getOwnPropertyDescriptor(a, 0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Reflect.ownKeys(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Object.prototype.toString.call(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return a[Symbol.toStringTag];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Object.isFrozen(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 2); b.resize(1); return Object.freeze(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); return Object.freeze(a);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b, 0, 2); return Object.freeze(a).length;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); return Object.seal(a).length;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); return Object.isSealed(Object.seal(a));`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(b); Object.preventExtensions(a); return Object.isFrozen(a);`);
P(`return new Uint8Array(new SharedArrayBuffer(4, {maxByteLength: 8})).length;`);
P(`var s = new SharedArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(s); s.grow(6); return [a.length, s.growable, s.byteLength, s.maxByteLength];`);
P(`var s = new SharedArrayBuffer(4, {maxByteLength: 8}); var a = new Uint8Array(s, 0, 2); s.grow(6); return [a.length, a.byteLength];`);
P(`var s = new SharedArrayBuffer(4, {maxByteLength: 8}); return s.grow(2);`);
P(`var s = new SharedArrayBuffer(4, {maxByteLength: 8}); return s.grow(9);`);
P(`var s = new SharedArrayBuffer(4); return [s.growable, s.maxByteLength, s.byteLength];`);
P(`var s = new SharedArrayBuffer(4); return s.grow(4);`);

// ---- 9. detach via transfer, e os métodos de TypedArray desanexado.
const methodCalls = ["a.at(0)", "a.entries().next()", "a.every(() => 1)", "a.fill(1)", "a.filter(() => 1)", "a.find(() => 1)", "a.findIndex(() => 1)", "a.findLast(() => 1)", "a.findLastIndex(() => 1)",
  "a.forEach(() => 1)", "a.includes(0)", "a.indexOf(0)", "a.join()", "a.keys().next()", "a.lastIndexOf(0)", "a.map(() => 1)", "a.reduce((p, c) => p, 0)", "a.reduceRight((p, c) => p, 0)",
  "a.reverse()", "a.set([1])", "a.slice()", "a.some(() => 1)", "a.sort()", "a.subarray(0)", "a.toLocaleString()", "a.toString()", "a.values().next()", "a.toReversed()", "a.toSorted()", "a.with(0, 1)",
  "a.copyWithin(0, 1)", "a.length", "a.byteLength", "a.byteOffset", "a[0]", "a[Symbol.toStringTag]", "0 in a", "Object.keys(a)", "[...a]", "Array.from(a)", "JSON.stringify(a)", "Object.getOwnPropertyDescriptor(a, 0)",
  "Reflect.ownKeys(a)", "a.buffer.byteLength", "a.buffer.detached", "new Uint8Array(a)", "new Uint16Array(a)", "Uint8Array.from(a)", "(a[0] = 1, a[0])", "delete a[0]", "Object.freeze(a)", "Object.isFrozen(a)",
  "Atomics.load(a, 0)", "new DataView(a.buffer)", "Array.prototype.slice.call(a)", "a.subarray(0, 0).buffer.detached", "structuredClone === undefined"];
for (const t of ["Uint8Array", "Float64Array", "BigInt64Array"]) {
  for (const m of methodCalls) {
    P(`var a = new ${t}(4); a.buffer.transfer(); return ${m};`);
    P(`var a = new ${t}(0); a.buffer.transfer(); return ${m};`);
  }
}
P(`var a = new Uint8Array([1, 2, 3]); var b = a.buffer.transfer(); return [a.length, a.byteLength, a.byteOffset, new Uint8Array(b)];`);
P(`var a = new Uint8Array([1, 2, 3]); var b = a.buffer.transfer(5); return [a.length, new Uint8Array(b)];`);
P(`var a = new Uint8Array([1, 2, 3]); var b = a.buffer.transfer(1); return [new Uint8Array(b)];`);
P(`var a = new Uint8Array([1, 2, 3]); var b = a.buffer.transfer(0); return [b.byteLength, a.length];`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return a.buffer.transfer();`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return [a.buffer.detached, a.buffer.byteLength, a.buffer.maxByteLength, a.buffer.resizable];`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return a.buffer.slice();`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return a.buffer.resize(1);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return a.buffer.transferToFixedLength();`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return new Uint8Array(a.buffer);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return new Uint8Array(a.buffer, 0, 0);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return new DataView(a.buffer);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return ArrayBuffer.isView(a);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return Object.getOwnPropertyNames(a);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); a[5] = 1; return [a[5], a.length];`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return a.hasOwnProperty(0);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return Object.getOwnPropertyDescriptor(a, 'length');`);
P(`var a = new Uint8Array([1, 2, 3]); var it = a.values(); a.buffer.transfer(); return it.next();`);
P(`var a = new Uint8Array([1, 2, 3]); var it = a.values(); it.next(); a.buffer.transfer(); try { return it.next() } catch (e) { return e.name + ': ' + e.message }`);
P(`var a = new Uint8Array([1, 2, 3]); var it = a.entries(); it.next(); it.next(); it.next(); it.next(); a.buffer.transfer(); return it.next();`);
P(`var a = new Uint8Array([1, 2, 3]); return a.map(function () { a.buffer.transfer(); return 1 });`);
P(`var a = new Uint8Array([1, 2, 3]); var s = []; a.forEach(function (x) { s.push(x); a.buffer.transfer() }); return s;`);
P(`var a = new Uint8Array([1, 2, 3]); return a.filter(function () { a.buffer.transfer(); return true });`);
P(`var a = new Uint8Array([1, 2, 3]); return a.slice({valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.slice(0, {valueOf() { a.buffer.transfer(); return 2 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.join({toString() { a.buffer.transfer(); return '-' }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.includes(undefined, {valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.indexOf(undefined, {valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.lastIndexOf(0, {valueOf() { a.buffer.transfer(); return 2 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.at({valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.subarray({valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.set([1], {valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return a.reverse.call(new Uint8Array(0));`);
P(`var a = new Uint8Array([1, 2, 3]); return new Uint8Array(a.buffer, {valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); return new Uint8Array(4).set(a, {valueOf() { a.buffer.transfer(); return 0 }});`);
P(`var a = new Uint8Array([1, 2, 3]); var b = new Uint8Array(4); b.set(a); return b;`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); var b = new Uint8Array(4); return b.set(a);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return Uint8Array.from.call(Uint8Array, a);`);
P(`var a = new Uint8Array([1, 2, 3]); a.buffer.transfer(); return new Float32Array(a);`);

// ---- 10. Float16Array e Math.f16round.
const f16vals = ["0", "-0", "1", "-1", "0.1", "1/3", "65504", "65505", "65519", "65520", "65535", "70000", "-65520", "Infinity", "-Infinity", "NaN", "6.103515625e-5", "5.960464477539063e-8", "2.98023223876953125e-8",
  "2.980232238769532e-8", "2.9802322387695313e-8", "1e-8", "1e-7", "1.0009765625", "1.00048828125", "1.0004882812500001", "1.00146484375", "2049", "2050", "2051", "4097", "8193", "32768.5", "0.333251953125", "3.140625", "Math.PI",
  "Math.E", "1e5", "1e-5", "-1e-5", "0.00006103515625", "0.00006097555160522461", "0.000030517578125", "123.456", "-123.456", "1023", "1024", "1025", "2047.5", "2048.5", "4095.5", "4096.5", "8191.5", "16383.75", "16384.25"];
for (const v of f16vals) {
  P(`return [Math.f16round(${v}), new Float16Array([${v}])[0], (function () { var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, ${v}); return [d.getUint16(0), d.getFloat16(0)] })()];`);
  P(`var a = new Float16Array(1); a[0] = ${v}; return [new Uint16Array(a.buffer)[0], Object.is(a[0], -0)];`);
  P(`var a = new Float16Array(1); a.fill(${v}); var b = new Float32Array(a); return [b[0], Math.f16round(b[0]) === b[0] || Number.isNaN(b[0])];`);
  P(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, ${v}, true); return [d.getUint8(0), d.getUint8(1), d.getFloat16(0, true), d.getFloat16(0, false)];`);
  P(`return new Float16Array([${v}, 1]).join();`);
  P(`return [new Float16Array([${v}]).includes(${v}), new Float16Array([${v}]).indexOf(${v})];`);
  P(`var a = new Float16Array(2); a.set(new Float64Array([${v}, ${v}])); return a;`);
  P(`var a = new Float64Array([${v}]); var b = new Float16Array(a); var c = new Float64Array(b); return [b, c, c[0] === a[0]];`);
}
for (let bits = 0; bits < 0x10000; bits += 997) {
  P(`var u = new Uint16Array([${bits}]); var f = new Float16Array(u.buffer); return [f[0], Object.is(f[0], -0), Math.f16round(f[0]) === f[0] || Number.isNaN(f[0]), new Float32Array(f)[0]];`);
}
for (const bits of [0, 1, 0x3ff, 0x400, 0x7bff, 0x7c00, 0x7c01, 0x7e00, 0x7fff, 0x8000, 0x8001, 0xfbff, 0xfc00, 0xfe00, 0xffff, 0x3c00, 0x3555, 0x4248, 0x5640, 0x03ff, 0x0400]) {
  P(`var u = new Uint16Array([${bits}]); var f = new Float16Array(u.buffer); var d = new DataView(u.buffer); return [f[0], d.getFloat16(0, true), String(f[0]), f.toString(), 1 / f[0]];`);
}
P(`return [Math.f16round(), Math.f16round(undefined), Math.f16round('1.5'), Math.f16round(null), Math.f16round({valueOf() { return 1.0005 }}), Math.f16round(1n === 1n)];`);
P(`return Math.f16round(1n);`);
P(`return Math.f16round.length + Math.f16round.name;`);
P(`return [Float16Array.BYTES_PER_ELEMENT, Float16Array.name, Float16Array.length, Float16Array.prototype[Symbol.toStringTag], Object.getPrototypeOf(Float16Array) === Object.getPrototypeOf(Uint8Array)];`);
P(`var a = new Float16Array([3, 1, 2]); return [a.sort(), a.toSorted((x, y) => y - x), a.toReversed(), a.with(1, 5), a.at(-1)];`);
P(`var a = new Float16Array([1.5, NaN, -0, 0]); return [a.indexOf(NaN), a.includes(NaN), a.lastIndexOf(0), a.indexOf(-0), a.findLast(Number.isNaN)];`);
P(`return new Float16Array(new ArrayBuffer(6), 2);`);
P(`return new Float16Array(new ArrayBuffer(5));`);
P(`return new Float16Array(new ArrayBuffer(6), 1);`);
P(`var d = new DataView(new ArrayBuffer(4)); return [d.getFloat16(0), d.getFloat16.length, d.setFloat16.length, d.setFloat16(0, 1.5), d.getFloat16(0)];`);
P(`var d = new DataView(new ArrayBuffer(2)); return d.getFloat16(1);`);
P(`var d = new DataView(new ArrayBuffer(2)); return d.setFloat16(1, 1);`);
P(`var d = new DataView(new ArrayBuffer(2)); return d.getFloat16(-1);`);
P(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, 1n);`);
P(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0);  return d.getFloat16(0);`);
P(`var d = new DataView(new ArrayBuffer(2)); d.setFloat16(0, '2.5'); return d.getFloat16(0);`);

// ---- 11. ArrayBuffer.prototype: slice, resize, transfer, transferToFixedLength, detached.
const sidx = ["undefined", "0", "1", "3", "-1", "-3", "-9", "9", "10", "NaN", "Infinity", "-Infinity", "1.9", "'2'", "null", "-0", "{valueOf(){return 4}}", "true"];
for (const b of sidx) for (const e of sidx) {
  P(`var b = new ArrayBuffer(8); var u = new Uint8Array(b); for (var i = 0; i < 8; i++) u[i] = i + 1; var s = b.slice(${b}, ${e}); return [s, s === b, s.byteLength];`);
}
for (const a of sidx) {
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var s = b.slice(${a}); return [s, s.resizable];`);
  P(`var b = new ArrayBuffer(8); return b.slice(${a}).byteLength;`);
}
P(`return new ArrayBuffer(8).slice.call(new Uint8Array(1))`);
P(`return ArrayBuffer.prototype.slice.call({}, 0)`);
P(`return ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(4), 0)`);
P(`var b = new ArrayBuffer(8); b.constructor = undefined; return b.slice(1).byteLength;`);
P(`var b = new ArrayBuffer(8); b.constructor = 1; return b.slice(1).byteLength;`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new ArrayBuffer(n) }}; return b.slice(1, 4).byteLength;`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new ArrayBuffer(n - 1) }}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new ArrayBuffer(n + 2) }}; return b.slice(1, 4).byteLength;`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return b }}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return {} }}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { var x = new ArrayBuffer(n); x.transfer(); return x }}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: function (n) { return new SharedArrayBuffer(n) }}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: null}; return b.slice(1, 4).byteLength;`);
P(`var b = new ArrayBuffer(8); b.constructor = {[Symbol.species]: 1}; return b.slice(1, 4);`);
P(`var b = new ArrayBuffer(8); return b.slice({valueOf() { b.transfer(); return 0 }});`);
P(`var b = new ArrayBuffer(8); return b.slice(0, {valueOf() { b.transfer(); return 4 }});`);
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.slice(0, {valueOf() { b.resize(2); return 6 }});`);
P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); return b.slice({valueOf() { b.resize(2); return 4 }});`);
for (const n of ["0", "1", "4", "8", "16", "17", "-1", "1.5", "'8'", "undefined", "null", "NaN", "Infinity", "{valueOf(){return 3}}", "2**53", "true"]) {
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var r = b.resize(${n}); return [r, b.byteLength];`);
  P(`var b = new ArrayBuffer(8); return b.resize(${n});`);
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var u = new Uint8Array(b); u.fill(5); try { b.resize(${n}) } catch (e) { return e.name } return u;`);
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var u = new Uint8Array(b); u.fill(5); b.resize(${n} > 8 ? 8 : 4); b.resize(8); return u;`);
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var r = b.transfer(${n}); return [r, b.detached, r.resizable, r.maxByteLength];`);
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var r = b.transferToFixedLength(${n}); return [r, b.detached, r.resizable, r.maxByteLength];`);
  P(`var b = new ArrayBuffer(8); var r = b.transfer(${n}); return [r, b.detached, r.resizable];`);
  P(`var b = new ArrayBuffer(8); var r = b.transferToFixedLength(${n}); return [r, b.detached, r.resizable];`);
  P(`var b = new ArrayBuffer(${n}); return [b.byteLength, b.resizable, b.maxByteLength];`);
  P(`var b = new ArrayBuffer(2, {maxByteLength: ${n}}); return [b.byteLength, b.resizable, b.maxByteLength];`);
  P(`var b = new ArrayBuffer(${n}, {maxByteLength: 16}); return [b.byteLength, b.resizable, b.maxByteLength];`);
  P(`var b = new ArrayBuffer(8, {maxByteLength: 16}); var r = b.transfer(${n}); try { r.resize(${n}) } catch (e) { return e.name } return r;`);
}
P(`return new ArrayBuffer(2, {maxByteLength: 1});`);
P(`return new ArrayBuffer(2, {});`);
P(`return new ArrayBuffer(2, {maxByteLength: undefined}).resizable;`);
P(`return new ArrayBuffer(2, null);`);
P(`return new ArrayBuffer(2, 5).resizable;`);
P(`return new ArrayBuffer(2, {get maxByteLength() { throw new EvalError('g') }});`);
P(`return new ArrayBuffer(2, {maxByteLength: 2}).resizable;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var r = b.transfer(); return [r, b, r.resizable, r.maxByteLength];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var r = b.transfer(6); r.resize(8); return [r, r.byteLength];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var r = b.transfer(9); return r;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var r = b.transferToFixedLength(9); return [r, r.resizable];`);
P(`var b = new ArrayBuffer(4); new Uint8Array(b).set([1, 2, 3, 4]); var r = b.transfer(2); return [r, b.byteLength, b.detached];`);
P(`var b = new ArrayBuffer(4); new Uint8Array(b).set([1, 2, 3, 4]); var r = b.transfer(6); return [r, b.byteLength, b.detached];`);
P(`var b = new ArrayBuffer(4); new Uint8Array(b).set([1, 2, 3, 4]); var r = b.transferToFixedLength(); return [r, b.detached];`);
P(`var b = new ArrayBuffer(4); b.transfer(); return [b.detached, b.byteLength, Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get.call(b)];`);
P(`return ArrayBuffer.prototype.transfer.call({});`);
P(`return ArrayBuffer.prototype.transfer.call(new SharedArrayBuffer(2));`);
P(`return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get.call(new SharedArrayBuffer(2));`);
P(`return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'resizable').get.call({});`);
P(`return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'maxByteLength').get.call({});`);
P(`return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call(new SharedArrayBuffer(2));`);
P(`return [ArrayBuffer.prototype.transfer.length, ArrayBuffer.prototype.transferToFixedLength.length, ArrayBuffer.prototype.resize.length, ArrayBuffer.prototype.slice.length, ArrayBuffer.length];`);
P(`return Object.getOwnPropertyNames(ArrayBuffer.prototype).sort();`);
P(`return Object.getOwnPropertyNames(ArrayBuffer).sort();`);
P(`return [ArrayBuffer.isView(new Uint8Array(1)), ArrayBuffer.isView(new DataView(new ArrayBuffer(1))), ArrayBuffer.isView(new ArrayBuffer(1)), ArrayBuffer.isView(), ArrayBuffer.isView({})];`);
P(`var b = new ArrayBuffer(4); var v = new DataView(b); b.transfer(); try { return v.byteLength } catch (e) { return e.name + ': ' + e.message }`);
P(`var b = new ArrayBuffer(4); var v = new DataView(b); b.transfer(); try { return v.byteOffset } catch (e) { return e.name + ': ' + e.message }`);
P(`var b = new ArrayBuffer(4); var v = new DataView(b); b.transfer(); try { return v.buffer === b } catch (e) { return e.name }`);
P(`var b = new ArrayBuffer(4); var v = new DataView(b); b.transfer(); return v.getInt8(0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b); b.resize(8); return v.byteLength;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 1); b.resize(8); return [v.byteLength, v.byteOffset];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 1, 2); b.resize(2); return v.byteLength;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 1, 2); b.resize(2); return v.byteOffset;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 2); b.resize(1); return v.byteLength;`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 2); b.resize(1); return v.getInt8(0);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b, 2); b.resize(3); return [v.byteLength, v.getInt8(0)];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b); b.resize(0); return [v.byteLength, v.byteOffset];`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b); b.resize(8); v.setInt8(7, 3); return new Uint8Array(b);`);
P(`var b = new ArrayBuffer(4, {maxByteLength: 8}); var v = new DataView(b); b.resize(2); return v.setInt8(2, 3);`);

// ---- execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "typedarray-proto-golden-"));
const source_file = path.join(dir, "case_source.js");
const file = path.join(dir, "case.js");
fs.writeFileSync(file, `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n");
const prefix = dir + "/";
const seen = new Set();
const rows = [];
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
bodies.splice(0, bodies.length, ...bodies.filter((p) => !usesHostApi(p)));
for (const body of bodies) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = PRELUDE + "\n" + body;
  if (existingSources.has(source)) { dropped++; continue; }
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find((line) => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stdout.write(emitFactored("typedarray_proto", rows));
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
