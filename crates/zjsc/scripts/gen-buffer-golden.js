// Gera tests/golden/buffer_bun.tsv: o global `Buffer` medido no bun 1.4.2 (descritores, `length`, `name`, `poolSize`,
// `Buffer.from` de string/array/array-like/ArrayBuffer/Buffer, `alloc`/`allocUnsafe`, `isBuffer`, `isEncoding`,
// `byteLength`, `toString` e as mensagens de erro de `ERR_INVALID_ARG_TYPE`, `ERR_OUT_OF_RANGE`,
// `ERR_BUFFER_OUT_OF_BOUNDS` e `ERR_UNKNOWN_ENCODING`). Sem rede, sem arquivo e sem caminho da máquina. O resultado é o
// valor de `R`; cada programa roda duas vezes e as saídas têm de ser idênticas (senão o gerador falha).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-buffer-golden.js > tests/golden/buffer_bun.tsv
// A ordem dos casos importa: `tests/buffer_bun_golden.rs` escolhe faixas por índice (base 0). Os casos novos entram
// SEMPRE no fim de cada bloco numerado abaixo, e o bloco "chaves" fica por último (fora do escopo até as fatias 5 a 9).
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'undefined') return 'undefined'; if (typeof v === 'function') return 'function'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e && e.name + '|' + e.code + '|' + e.message + '|' + (e instanceof Error) };\n" +
  "var H = function (b) { return Array.prototype.map.call(b, function (x) { return (x < 16 ? '0' : '') + x.toString(16) }).join('') };\n" +
  "var DESC = function (o, k) { var d = Object.getOwnPropertyDescriptor(o, k); if (!d) return 'none'; " +
  "return [typeof d.value, typeof d.get, typeof d.set, d.writable, d.enumerable, d.configurable, typeof d.value === 'function' ? d.value.length + ':' + d.value.name : (d.get ? d.get.name : '')] };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = 'throw ' + E(e) }`);

// Bloco 1: forma do global.
expr(`DESC(globalThis, 'Buffer')`);
expr(`[Buffer.length, Buffer.name, typeof Buffer]`);
expr(`Object.getPrototypeOf(Buffer) === Uint8Array`);
expr(`Object.getPrototypeOf(Buffer.prototype) === Uint8Array.prototype`);
expr(`Buffer.prototype.constructor === Buffer`);
expr(`DESC(Buffer.prototype, 'constructor')`);
expr(`[Buffer.poolSize, DESC(Buffer, 'poolSize')]`);
expr(`['alloc', 'allocUnsafe', 'allocUnsafeSlow', 'byteLength', 'from', 'isBuffer', 'isEncoding'].map(function (k) { return [k, DESC(Buffer, k)] })`);
expr(`DESC(Buffer, 'length')`);
expr(`DESC(Buffer, 'name')`);
expr(`DESC(Buffer, 'prototype')`);
expr(`Buffer.from('a') instanceof Buffer && Buffer.from('a') instanceof Uint8Array`);
expr(`Buffer.from('a').constructor === Buffer`);
// 0 a 12

// Bloco 2: `Buffer.from` de string e codificações.
expr(`H(Buffer.from('ab'))`);
expr(`H(Buffer.from('6162', 'hex'))`);
expr(`H(Buffer.from('YWI=', 'base64'))`);
expr(`H(Buffer.from('YWI_Pg', 'base64url'))`);
expr(`H(Buffer.from('a-_b', 'base64'))`);
expr(`H(Buffer.from('YW\\nJj=Zg', 'base64'))`);
expr(`H(Buffer.from('\\u00e9', 'latin1'))`);
expr(`H(Buffer.from('\\u00e9'))`);
expr(`H(Buffer.from('h\\u00e9', 'utf16le'))`);
expr(`H(Buffer.from('zz', 'hex')) + '|' + H(Buffer.from('abc', 'hex'))`);
expr(`H(Buffer.from('a', undefined)) + '|' + H(Buffer.from('a', null))`);
expr(`H(Buffer.from('a', 'ASCII')) + '|' + H(Buffer.from('a', 'UTF-8'))`);
expr(`Buffer.from('a', 'nope')`);
expr(`Buffer.from('abc').toString('hex')`);
expr(`Buffer.from('ab?>').toString('base64') + '|' + Buffer.from('ab?>').toString('base64url')`);
expr(`Buffer.from('\\u00e9').toString('latin1')`);
expr(`Buffer.from([0xe9, 0xff]).toString('ascii')`);
expr(`Buffer.from('h\\u00e9', 'utf16le').toString('utf16le')`);
expr(`Buffer.from([0x68, 0, 0xe9, 0, 0x41]).toString('utf16le').length`);
expr(`Buffer.from('abc').toString('utf8', 1, 2)`);
expr(`Buffer.from('abc').toString(undefined, 1)`);
expr(`Buffer.from('abc').toString('nope')`);
expr(`Object.prototype.toString.call(Buffer.from('a'))`);
// 13 a 35

// Bloco 3: `Buffer.from` de array, array-like, ArrayBuffer, visão e objeto `toJSON`.
expr(`H(Buffer.from([97, 98, 300]))`);
expr(`H(Buffer.from([1.7, -1, '3', null]))`);
expr(`H(Buffer.from({ length: 2 }))`);
expr(`H(Buffer.from({ length: 2, 0: 5, 1: 6 }))`);
expr(`H(Buffer.from({ type: 'Buffer', data: [1, 2] }))`);
expr(`H(Buffer.from(new Uint16Array([1, 258])))`);
expr(`H(Buffer.from(Buffer.from('ab')))`);
expr(`(function () { var a = new Uint8Array([1, 2]); var b = Buffer.from(a); b[0] = 9; return [a[0], b[0]] })()`);
expr(`(function () { var ab = new ArrayBuffer(8); var b = Buffer.from(ab, 2, 4); b[0] = 9; return [new Uint8Array(ab)[2], b.length, b.byteOffset] })()`);
expr(`(function () { var ab = new ArrayBuffer(8); var b = Buffer.from(ab); return [b.length, b.byteOffset, b.buffer === ab] })()`);
expr(`(function () { var ab = new ArrayBuffer(8); return [Buffer.from(ab, 3).length, Buffer.from(ab, 3, 0).length, Buffer.from(ab, undefined, 2).length, Buffer.from(ab, NaN, 2).length] })()`);
expr(`Buffer.from(new ArrayBuffer(8), 9)`);
expr(`Buffer.from(new ArrayBuffer(8), 2, 10)`);
expr(`Buffer.from(new ArrayBuffer(8), -1)`);
expr(`Buffer.from()`);
expr(`Buffer.from(undefined)`);
expr(`Buffer.from(null)`);
expr(`Buffer.from(true)`);
expr(`Buffer.from(5)`);
expr(`Buffer.from(-0)`);
expr(`Buffer.from(Symbol('s'))`);
expr(`Buffer.from(10n)`);
expr(`Buffer.from({})`);
expr(`Buffer.from(function foo() {})`);
expr(`Buffer.from(() => 1)`);
expr(`Buffer.from(new Date(0))`);
expr(`Buffer.from(new Map())`);
// 36 a 62

// Bloco 4: `alloc`, `allocUnsafe`, `isBuffer`, `isEncoding`, `byteLength`.
expr(`H(Buffer.alloc(3))`);
expr(`H(Buffer.alloc(3, 1))`);
expr(`H(Buffer.alloc(3, 257))`);
expr(`H(Buffer.alloc(3, 'ab'))`);
expr(`H(Buffer.alloc(4, '6162', 'hex'))`);
expr(`H(Buffer.alloc(3, ''))`);
expr(`H(Buffer.alloc(3, Buffer.from('xy')))`);
expr(`H(Buffer.alloc(1.5))`);
expr(`Buffer.alloc(0).length`);
expr(`Buffer.alloc(2, 'a', 'nope')`);
expr(`Buffer.alloc('a')`);
expr(`Buffer.alloc(-1)`);
expr(`Buffer.alloc(NaN)`);
expr(`Buffer.alloc(2 ** 53)`);
expr(`Buffer.alloc(Infinity)`);
expr(`Buffer.alloc()`);
expr(`Buffer.alloc(undefined)`);
expr(`Buffer.alloc(null)`);
expr(`Buffer.alloc(true)`);
expr(`Buffer.alloc({})`);
expr(`Buffer.alloc(Symbol('s'))`);
expr(`Buffer.allocUnsafe(2).length`);
expr(`Buffer.allocUnsafe(2) instanceof Buffer`);
expr(`Buffer.allocUnsafe(-1)`);
expr(`Buffer.allocUnsafe('a')`);
expr(`Buffer.allocUnsafeSlow(2).length`);
expr(`Buffer.allocUnsafeSlow(-1)`);
expr(`Buffer.alloc(3) instanceof Buffer`);
expr(`[Buffer.isBuffer(Buffer.alloc(1)), Buffer.isBuffer(new Uint8Array(1)), Buffer.isBuffer(1), Buffer.isBuffer(), Buffer.isBuffer(null), Buffer.isBuffer({}), Buffer.isBuffer(Buffer.from('a').subarray(0))]`);
expr(`[Buffer.isEncoding('UTF8'), Buffer.isEncoding('utf-8'), Buffer.isEncoding('hex'), Buffer.isEncoding('base64url'), Buffer.isEncoding('binary'), Buffer.isEncoding('ucs-2'), Buffer.isEncoding('nope'), Buffer.isEncoding(1), Buffer.isEncoding(''), Buffer.isEncoding()]`);
expr(`[Buffer.byteLength('\\u00e9'), Buffer.byteLength('YWI=', 'base64'), Buffer.byteLength('abc', 'hex'), Buffer.byteLength('\\u00e9', 'latin1'), Buffer.byteLength('ab', 'utf16le'), Buffer.byteLength('a', 'nope'), Buffer.byteLength('\\ud83d\\ude00')]`);
expr(`[Buffer.byteLength(new ArrayBuffer(5)), Buffer.byteLength(Buffer.alloc(3)), Buffer.byteLength(new Uint16Array(2))]`);
expr(`Buffer.byteLength(1)`);
expr(`Buffer.byteLength()`);
expr(`Buffer.byteLength({})`);
expr(`H(Buffer(3)) + '|' + H(new Buffer(2)) + '|' + H(Buffer('ab'))`);
// 63 a 98

// Bloco chaves (FORA do escopo até as fatias 5 a 9: `compare`, `concat`, `copyBytesFrom`, protótipo completo e a ordem
// `alloc ... isEncoding, length, name, prototype, poolSize`).
expr(`Object.getOwnPropertyNames(Buffer)`);
expr(`Reflect.ownKeys(Buffer.prototype).map(String)`);

// Bloco 5 (índices 101 em diante; entra DEPOIS do bloco chaves para não deslocar os índices já usados): `Buffer.from`
// de ArrayBuffer compartilha a memória, preenchimento inválido, mensagens de size, `new Buffer(n)`, `compare`,
// `concat`, `copyBytesFrom` e os métodos de leitura do protótipo (`equals`, `compare`, `indexOf`, `lastIndexOf`,
// `includes`, `slice`, `subarray`, `toJSON`, `write`). Cada linha foi medida no bun 1.4.2.
expr(`(function () { var ab = new ArrayBuffer(4); var b = Buffer.from(ab); b[0] = 7; return [b.buffer === ab, new Uint8Array(ab)[0]] })()`);
expr(`(function () { var ab = new ArrayBuffer(4); var b = Buffer.from(ab); new Uint8Array(ab)[1] = 5; return b[1] })()`);
expr(`H(Buffer.alloc(3, '')) + '|' + H(Buffer.alloc(3, '', 'hex'))`);
expr(`Buffer.alloc(3, Buffer.alloc(0))`);
expr(`H(Buffer.alloc(3, true)) + '|' + H(Buffer.alloc(3, {})) + '|' + H(Buffer.alloc(3, null)) + '|' + H(Buffer.alloc(3, [5])) + '|' + H(Buffer.alloc(3, '0'))`);
expr(`Buffer.alloc(3, 'zz', 'hex')`);
expr(`new Buffer(-1)`);
expr(`new Buffer(NaN)`);
expr(`new Buffer(Infinity)`);
expr(`new Buffer(2 ** 33)`);
for (const size of ["Infinity", "-Infinity", "undefined", "null", "new Date(0)", "[]", "new Map()", "'3'", "2 ** 32 + 1", "1e10", "-5", "1.5e300"]) {
  expr(`Buffer.alloc(${size})`);
  expr(`Buffer.allocUnsafe(${size})`);
}
expr(`[Buffer.compare(Buffer.from('a'), Buffer.from('b')), Buffer.compare(Buffer.from('a'), Buffer.from('a')), Buffer.compare(new Uint8Array([1]), new Uint8Array([1, 2]))]`);
expr(`Buffer.compare(1, 2)`);
expr(`Buffer.compare(Buffer.from('a'), 2)`);
expr(`Buffer.compare()`);
expr(`H(Buffer.concat([Buffer.from('a'), new Uint8Array([2])])) + '|' + H(Buffer.concat([Buffer.from('ab'), Buffer.from('cd')], 3)) + '|' + H(Buffer.concat([Buffer.from('a')], 5)) + '|' + Buffer.concat([]).length + '|' + Buffer.concat([Buffer.from('a')], 0).length`);
expr(`Buffer.concat('x')`);
expr(`Buffer.concat([1])`);
expr(`Buffer.concat([Buffer.from('a')], -1)`);
expr(`Buffer.concat([Buffer.from('a')], 'x')`);
expr(`Buffer.concat([Buffer.from('a')], 1.5)`);
expr(`Buffer.concat()`);
expr(`H(Buffer.copyBytesFrom(new Uint16Array([1, 258]))) + '|' + H(Buffer.copyBytesFrom(new Uint16Array([1, 258]), 1)) + '|' + H(Buffer.copyBytesFrom(new Uint16Array([1, 258]), 0, 1)) + '|' + Buffer.copyBytesFrom(new Uint8Array(2), 3).length`);
expr(`Buffer.copyBytesFrom([1])`);
expr(`Buffer.copyBytesFrom(new Uint8Array(2), -1)`);
expr(`Buffer.copyBytesFrom(new Uint8Array(2), 0, -1)`);
expr(`Buffer.copyBytesFrom(new Uint8Array(2), 'a')`);
expr(`(function () { var x = Buffer.from('abcabc'); return [x.equals(Buffer.from('abcabc')), x.equals(new Uint8Array(6)), x.compare(Buffer.from('abd')), x.compare(Buffer.from('abc'), 0, 3, 3, 6), x.compare(Buffer.from('a'), 9)] })()`);
expr(`Buffer.from('abc').equals('a')`);
expr(`Buffer.from('abc').compare(1)`);
expr(`(function () { var x = Buffer.from('abcabc'); return [x.indexOf('c'), x.indexOf('c', 3), x.lastIndexOf('c'), x.lastIndexOf('c', 4), x.indexOf(99), x.indexOf(''), x.indexOf('', 9), x.lastIndexOf(''), x.includes('bc'), x.indexOf('6263', 'hex'), x.indexOf('YmM=', 'base64'), x.indexOf(Buffer.from('ca')), x.indexOf('c', -2), x.indexOf('x')] })()`);
expr(`(function () { var x = Buffer.from('abcabc'); return [x.indexOf(99.5), x.indexOf(355), x.lastIndexOf('c', -1), x.indexOf('c', undefined, 'hex'), x.indexOf('c', null), x.lastIndexOf('c', undefined), x.lastIndexOf('c', NaN), x.indexOf('b', 'utf8')] })()`);
expr(`Buffer.from('abcabc').indexOf({})`);
expr(`Buffer.from('abcabc').indexOf('c', 'nope')`);
expr(`(function () { var x = Buffer.from('abcabc'); var s = x.slice(1, 3); s[0] = 0x7a; return [x.slice(1, 3).constructor === Buffer, H(x.slice(-2)), H(x.subarray(2, -1)), x.toString(), x.slice(1, 3).byteOffset, x.slice(4, 1).length, H(x.slice('1', '3')), H(x.slice(NaN, 2))] })()`);
expr(`[JSON.stringify(Buffer.from('ab')), JSON.stringify(Buffer.alloc(0)), Object.keys(Buffer.from('ab').toJSON())]`);
expr(`(function () { var w = function () { return Buffer.alloc(6) }; var q; var out = []; q = w(); out.push(q.write('ab'), H(q)); q = w(); out.push(q.write('ab', 2), H(q)); q = w(); out.push(q.write('abcd', 1, 2), H(q)); q = w(); out.push(q.write('6162', 'hex'), H(q)); q = w(); out.push(q.write('ab', 2, 'hex'), H(q)); q = w(); out.push(q.write('ab', 2, 1, 'latin1'), H(q)); q = w(); out.push(q.write('\\u00e9\\u00e9\\u00e9\\u00e9', 4), H(q)); q = w(); out.push(q.write('abcdefgh'), H(q)); return out })()`);
expr(`(function () { var w = function () { return Buffer.alloc(6) }; var q; var out = []; q = w(); out.push(q.write('ab', 0, 'utf16le'), H(q)); q = w(); out.push(q.write('abc', 0, 3, 'utf16le'), H(q)); q = w(); out.push(q.write('\\ud83d\\ude00', 4), H(q)); q = w(); out.push(q.write('a', undefined, undefined, 'hex'), H(q)); q = w(); out.push(q.write('a', 0, undefined), H(q)); out.push(w().write('a', 6)); return out })()`);
expr(`Buffer.alloc(6).write('a', 7)`);
expr(`Buffer.alloc(6).write('a', -1)`);
expr(`Buffer.alloc(6).write(1)`);
expr(`Buffer.alloc(6).write('a', 1, 9)`);
expr(`Buffer.alloc(6).write('a', 'nope')`);
expr(`Buffer.alloc(6).write('a', 1, 1, 'nope')`);
expr(`Buffer.alloc(6).write('a', 1.5)`);
expr(`Buffer.alloc(6).write('a', '1')`);
expr(`Buffer.alloc(6).write('a', 0, -1)`);
// 101 a 170

// Bloco 6 (índices 171 em diante, depois do bloco 5 para não deslocar os índices): ordem das chaves do construtor,
// `name` vazio, `read*`/`write*`, `fill`, `copy`, `swap*`, `xxxSlice`/`xxxWrite`, `toLocaleString` e `inspect`.
expr(`Object.getOwnPropertyNames(Buffer)`);
expr(`[DESC(Buffer, 'from'), DESC(Buffer, 'isBuffer'), DESC(Buffer.prototype, 'toJSON'), DESC(Buffer.prototype, 'toLocaleString'), DESC(Buffer.prototype, 'readUint8'), DESC(Buffer.prototype, 'writeBigUint64LE'), DESC(Buffer.prototype, 'readUInt8')]`);
expr(`(function () { var b = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]); return [b.readUInt8(0), b.readInt8(7), b.readUInt16BE(0), b.readUInt16LE(0), b.readInt16BE(1), b.readInt16LE(1), b.readUInt32BE(0), b.readUInt32LE(0), b.readInt32BE(2), b.readInt32LE(2), b.readUint8(1), b.readUint16LE(2), b.readInt16(0), b.readInt32(0), b.readUInt8()] })()`);
expr(`(function () { var b = Buffer.from([0xff, 0xfe, 0xfd, 0xfc, 0xfb, 0xfa, 0x12, 0x34]); return [b.readUIntBE(0, 5), b.readUIntLE(0, 5), b.readIntBE(0, 6), b.readIntLE(0, 3), b.readIntBE(0, 1), b.readIntLE(1, 6), b.readUIntBE(2, 1), b.readUintLE(0, 2), b.readUInt32LE(4), b.readInt8(0)] })()`);
expr(`(function () { var b = Buffer.alloc(8); var out = [b.writeDoubleLE(1.5, 0), H(b), b.readDoubleBE(0), b.readDoubleLE(0), b.readDouble(0), b.writeDoubleBE(-2.25), H(b), b.readDoubleBE(), b.writeFloatLE(1.1, 0), H(b), b.readFloatLE(0), b.readFloatBE(0), b.readFloat(0), b.writeFloatBE(0.5, 4), H(b), b.writeFloat(3, 0), b.writeDouble(7)]; return out })()`);
expr(`(function () { var b = Buffer.from('fedcba9876543210', 'hex'); return [b.readBigUInt64BE(0) + '', b.readBigInt64BE(0) + '', b.readBigUint64LE(0) + '', b.readBigInt64LE() + '', b.readBigInt64() + '', b.readBigUInt64() + '', typeof b.readBigInt64BE(0)] })()`);
expr(`(function () { var b = Buffer.alloc(8); var out = [b.writeBigInt64LE(-2n, 0), H(b), b.writeBigUInt64BE(0xfedcba9876543210n), H(b), b.writeBigUint64LE(1n), H(b), b.writeBigInt64BE(-(2n ** 63n)), H(b), b.writeBigUInt64LE(2n ** 64n - 1n), H(b)]; return out })()`);
for (const code of [
  `Buffer.alloc(8).readUInt8(8)`, `Buffer.alloc(8).readUInt8(-1)`, `Buffer.alloc(8).readUInt8(1.5)`, `Buffer.alloc(8).readUInt8('1')`,
  `Buffer.alloc(8).readUInt16LE(7)`, `Buffer.alloc(8).readUInt8(NaN)`, `Buffer.alloc(0).readUInt8(0)`, `Buffer.alloc(1).readUInt16LE(0)`,
  `Buffer.alloc(3).readUInt32LE(0)`, `Buffer.alloc(8).readUIntLE(0, 7)`, `Buffer.alloc(8).readUIntLE(0, 0)`, `Buffer.alloc(8).readUIntLE(0)`,
  `Buffer.alloc(8).readUIntBE(2, 6)`, `Buffer.alloc(8).readUIntBE(3, 6)`, `Buffer.alloc(8).readIntLE(0, 'a')`, `Buffer.alloc(8).readUIntLE(0, 3.5)`,
  `Buffer.alloc(8).readBigInt64LE(1)`, `Buffer.alloc(8).readBigInt64LE(-1)`, `Buffer.alloc(8).readFloatBE(5)`, `Buffer.alloc(8).readDoubleLE(1)`,
  `Buffer.alloc(8).readUInt32LE(-1)`, `Buffer.alloc(8).readInt8(Infinity)`, `Buffer.alloc(7).readBigInt64LE(0)`,
]) expr(code);
expr(`(function () { var b = Buffer.alloc(8); var out = [b.writeUInt8(255, 0), b.writeInt8(-128, 1), b.writeUInt16BE(0x102, 2), b.writeUInt16LE(0x304, 4), b.writeInt16BE(-2, 6), H(b), b.writeUInt32LE(0x01020304, 2), H(b), b.writeUInt32BE(0xa0b0c0d0, 0), H(b), b.writeInt32LE(-5, 4), H(b), b.writeInt32BE(-5), H(b), b.writeUint8(1, 7), b.writeUInt16(0x102, 1), H(b), b.writeUint32(0x0a0b0c0d, 2), H(b), b.writeUInt8(5), b.writeUInt8(1.5, 0), b.writeInt8(-1.5, 1), H(b)]; return out })()`);
expr(`(function () { var b = Buffer.alloc(8); var out = [b.writeUIntBE(0x123456789a, 0, 5), H(b), b.writeUIntLE(0x123456789a, 0, 5), H(b), b.writeIntBE(-2, 1, 6), H(b), b.writeIntLE(-2, 0, 3), H(b), b.writeUintBE(258, 6, 2), H(b), b.writeUIntLE(1, 7, 1), H(b)]; return out })()`);
for (const code of [
  `Buffer.alloc(8).writeUInt8(256, 0)`, `Buffer.alloc(8).writeUInt8(-1, 0)`, `Buffer.alloc(8).writeUInt8(1, 8)`, `Buffer.alloc(8).writeUInt8('a', 0)`,
  `Buffer.alloc(8).writeUInt8(1, '1')`, `Buffer.alloc(8).writeInt8(128, 0)`, `Buffer.alloc(8).writeInt8(127.9, 0)`, `Buffer.alloc(8).writeInt16BE(40000, 0)`,
  `Buffer.alloc(8).writeUInt32LE(2 ** 32, 0)`, `Buffer.alloc(8).writeInt32LE(1e10, 0)`, `Buffer.alloc(8).writeUIntLE(2 ** 48, 0, 6)`,
  `Buffer.alloc(8).writeIntBE(-(2 ** 47) - 1, 0, 6)`, `Buffer.alloc(8).writeUIntLE(2 ** 40, 0, 5)`, `Buffer.alloc(8).writeUIntBE(2 ** 24, 0, 3)`,
  `Buffer.alloc(8).writeIntBE(-8388609, 0, 3)`, `Buffer.alloc(8).writeUIntLE(1, 0, 7)`, `Buffer.alloc(8).writeUIntLE(1, 0, 0)`,
  `Buffer.alloc(8).writeUIntLE(1, 0, 'a')`, `Buffer.alloc(8).writeUIntLE(1, 0)`, `Buffer.alloc(8).writeIntLE(-1, 6, 3)`,
  `Buffer.alloc(8).writeBigInt64LE(2n ** 63n, 0)`, `Buffer.alloc(8).writeBigUInt64LE(-1n, 0)`, `Buffer.alloc(8).writeBigInt64LE(1, 0)`,
  `Buffer.alloc(8).writeBigInt64LE(1n, 1)`, `Buffer.alloc(8).writeFloatBE('x', 0)`, `Buffer.alloc(8).writeDoubleLE(1, 1)`,
  `Buffer.alloc(8).writeDoubleBE(1, 1.5)`, `Buffer.alloc(8).writeFloatLE(1, NaN)`, `Buffer.alloc(3).writeUInt32LE(1, 0)`, `Buffer.alloc(0).writeUInt8(1, 0)`,
]) expr(code);
expr(`(function () { var out = []; var f = function () { return Buffer.alloc(5) }; out.push(H(f().fill('ab')), H(f().fill('ab', 1, 3)), H(f().fill(257)), H(f().fill('6162', 'hex')), H(f().fill('ab', 'latin1')), H(f().fill(1, 6)), H(f().fill(1, 0, 2, 'nope')), H(f().fill(true)), H(f().fill({})), H(f().fill(3, 2)), H(f().fill('a', 3, 2)), H(f().fill('\\u00e9')), H(f().fill('')), H(f().fill(Buffer.from('xy'))), H(f().fill('ab', 1, 'latin1')), H(f().fill(-1)), H(f().fill(1.9)), H(f().fill(undefined))); var x = f(); out.push(x.fill(1) === x); return out })()`);
for (const code of [
  `Buffer.alloc(5).fill(1, -1)`, `Buffer.alloc(5).fill(1, 0, 6)`, `Buffer.alloc(5).fill(1, 'a')`, `Buffer.alloc(5).fill('zz', 'hex')`, `Buffer.alloc(5).fill('a', 1.5)`,
  `Buffer.alloc(5).fill(Buffer.alloc(0))`, `Buffer.alloc(5).fill('a', 0, 5, 'nope')`, `Buffer.prototype.fill.call({}, 0)`,
]) expr(code);
expr(`(function () { var s = Buffer.from('abcdef'); var d = Buffer.alloc(4); var out = [s.copy(d), d.toString(), s.copy(d, 1, 2), H(d), s.copy(d, 0, 1, 3), H(d), s.copy(d, 5), s.copy(d, 0, 0, 7), s.copy(d, 'a'), s.copy(d, 0, 'a'), s.copy(d, 0, 2, 1), s.copy(new Uint8Array(3), 0, 1), s.copy(d, 4), s.copy(d, 0, 6), s.copy(d, 1.5), s.copy(d, 0, 0, NaN), s.copy(d, Infinity)]; var o = Buffer.from('abcdef'); out.push(o.copy(o, 2, 0), o.toString()); return out })()`);
for (const code of [
  `Buffer.from('abcdef').copy(Buffer.alloc(4), -1)`, `Buffer.from('abcdef').copy(Buffer.alloc(4), 0, 7)`, `Buffer.from('abcdef').copy(Buffer.alloc(4), 0, -1)`,
  `Buffer.from('abcdef').copy(1)`, `Buffer.from('abcdef').copy()`, `Buffer.prototype.copy.call({}, Buffer.alloc(1))`,
]) expr(code);
expr(`(function () { var out = []; var a = Buffer.from('abcdefgh'); var s = a.swap16(); out.push(s === a, a.toString()); out.push(Buffer.from('abcdefgh').swap32().toString(), Buffer.from('abcdefgh').swap64().toString(), Buffer.alloc(0).swap16().length); return out })()`);
for (const code of [
  `Buffer.from('abc').swap16()`, `Buffer.from('abcd').swap64()`, `Buffer.from('abcde').swap32()`, `Buffer.prototype.swap16.call({})`,
]) expr(code);
expr(`(function () { var s = Buffer.from('abcdef'); return [s.asciiSlice(), s.hexSlice(1, 3), s.base64Slice(), s.base64urlSlice(0, 4), s.latin1Slice(1, 2), s.ucs2Slice(0, 4), s.utf16leSlice(0, 2), s.utf8Slice(2), s.hexSlice(4, 2), s.hexSlice(1.5), s.utf8Slice('a'), s.utf8Slice(7), s.utf8Slice(2, 1), s.utf8Slice(1, undefined), s.utf8Slice(0, null), s.utf8Slice(0, '2'), s.utf8Slice(0, 2.9), s.base64Slice(1)] })()`);
for (const code of [
  `Buffer.from('abcdef').asciiSlice(-1, 99)`, `Buffer.from('abcdef').utf8Slice(0, 7)`, `Buffer.from('abcdef').hexSlice(-1)`, `Buffer.prototype.utf8Slice.call({})`,
]) expr(code);
expr(`(function () { var w = function () { return Buffer.alloc(6) }; var q = w(); var out = [q.asciiWrite('ab'), q.hexWrite('0102', 1), q.latin1Write('\\u00e9', 3), q.utf8Write('\\u00e9', 4), q.ucs2Write('a', 0), q.base64Write('YWI=', 2), q.base64urlWrite('YWI', 5), H(q)]; out.push(w().utf8Write('abc', 1, 2.5), w().utf8Write('abc', '1'), w().utf8Write('abc', null), w().hexWrite('abc'), w().utf8Write('abc', undefined, 2), w().utf8Write('abc', 0, undefined), w().ucs2Write('abcd', 1), w().utf8Write('\\u00e9\\u00e9\\u00e9\\u00e9', 4), w().utf8Write('a', 6), w().utf8Write('a', 1, 0), w().asciiWrite('\\u00e9'), w().utf8Write(1), w().utf8Write('a', 'x'), w().utf8Write('a', 1, 'x'), w().utf16leWrite('ab')); return out })()`);
for (const code of [
  `Buffer.alloc(6).utf8Write('a', 7)`, `Buffer.alloc(6).utf8Write('a', -1)`, `Buffer.alloc(6).utf8Write('abc', 1, 9)`, `Buffer.alloc(6).utf8Write('abc', 1, -1)`,
  `Buffer.prototype.utf8Write.call({}, 'a')`,
]) expr(code);
expr(`[Buffer.from('abc').toLocaleString(), Buffer.from('abc').toLocaleString('hex'), Buffer.prototype.toLocaleString === Buffer.prototype.toString, Buffer.from('abc').toLocaleString('utf8', 1, 2)]`);
expr(`[Buffer.from('ab').inspect(), Buffer.alloc(60).inspect(), Buffer.alloc(51).inspect(), Buffer.alloc(50).inspect(), Buffer.alloc(0).inspect(), Buffer.prototype.inspect.call(Buffer.from('ab').subarray(1)), Buffer.prototype.inspect.call(new Uint8Array(2)), Buffer.from('abc').inspect(0)]`);
expr(`Buffer.prototype.inspect.call({})`);
expr(`Buffer.prototype.inspect.call(1)`);
expr(`Buffer.prototype.readUInt8.call({}, 0)`);
expr(`Buffer.prototype.writeUInt8.call({}, 1, 0)`);
expr(`Buffer.from('abcdef').copy(Buffer.alloc(4), 0, 0, -1)`);
expr(`(function () { var s = Buffer.from('abcdef'); var d = function () { return Buffer.alloc(4) }; return [s.copy(d(), 0, Infinity), s.copy(d(), 0, 0, Infinity), s.copy(d(), -Infinity), s.copy(d(), 0, -Infinity), s.copy(d(), 0, 0, -Infinity)] })()`);
// 171 a 272

// Bloco 8 (índices 298 em diante; o texto fica aqui, mas `bufferEncodingBlock()` só roda depois do bloco 293 a 297, para
// não deslocar os índices já usados): base64 leniente, hex ímpar e inválido (byte baixo da unidade), `byteLength` de
// hex/base64 por tamanho, escrita parcial em latin1/ascii/utf16le/utf8, surrogate solto, utf8 inválido em `toString`
// e `readIntBE`/`writeIntBE` com `byteLength` variável.
const bufferEncodingBlock = () => {
expr(`['YW Jj\\n Zg==', 'Y*W!Jj', 'Y', 'YW', 'YWJ', 'YW=Jj', 'YW\\u00e9Jj', '\\u0141\\u0157', '\\uff41\\uff42\\uff43\\uff44'].map(function (s) { return H(Buffer.from(s, 'base64')) })`);
expr(`H(Buffer.from('YWI_Pg', 'base64url')) + '|' + H(Buffer.from('YWI/Pg==', 'base64url'))`);
expr(`['YWJ', 'YWJjZ', 'YWJjZG', 'YWJjZGV', 'Y', 'YWJ=', 'YWJ==', '=', '==', '===', 'YW=Jj=', 'YW-_', 'aaaaaaaaaa', 'YWJj\\n', '  ', 'Y*W!', 'YQ'].map(function (s) { return Buffer.byteLength(s, 'base64') })`);
expr(`['YWJ', 'YWJjZ', 'Y*W!', 'YQ==', 'YQ='].map(function (s) { return Buffer.byteLength(s, 'base64url') })`);
expr(`(function () { var b = Buffer.alloc(6); var c = Buffer.alloc(4); var d = Buffer.alloc(2); var e = Buffer.alloc(8); return [b.write('YWJjZGVm', 'base64'), H(b), c.write('YW Jj ZGVm', 'base64'), H(c), d.write('YWJjZGVm', 'base64'), H(d), e.write('YWI_Pg', 'base64url'), H(e)] })()`);
expr(`['abc', 'a', 'zz12', '12zz34', '1g', '0x12', ' 12', 'ABcd', '12\\u0661\\u0662', '\\u0131\\u0132', '\\u0161\\u0162', '\\u1f31\\u0132', '\\uff41\\uff42'].map(function (s) { return H(Buffer.from(s, 'hex')) })`);
expr(`[Buffer.byteLength('abc', 'hex'), Buffer.byteLength('zz', 'hex'), Buffer.byteLength('1234', 'hex'), Buffer.byteLength('a', 'hex'), Buffer.byteLength('', 'hex'), Buffer.byteLength('\\u0131\\u0132\\u0133', 'hex')]`);
expr(`(function () { var b = Buffer.alloc(4); return [b.write('1234zz56', 'hex'), H(b), b.write('abc', 'hex'), b.write('12345678aa', 'hex'), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(4); return [b.write('12345678', 2, 'hex'), H(b), b.write('123456', 1, 1, 'hex'), H(b)] })()`);
expr(`[Buffer.from([1, 255, 16]).toString('hex'), Buffer.from([1, 2, 3]).toString('hex', 1, 2)]`);
expr(`Buffer.alloc(3, 'Y*', 'base64')`);
expr(`H(Buffer.alloc(5, 'YWI', 'base64'))`);
expr(`(function () { var b = Buffer.alloc(3); var out = [b.write('\\u00e9\\u00e9\\u00e9\\u00e9', 'latin1'), H(b), b.write('\\u0141ab', 1, 'latin1'), H(b)]; var c = Buffer.alloc(3); out.push(c.write('\\u00e9\\u0141z', 'ascii'), H(c)); return out })()`);
expr(`[Buffer.from('\\u0141\\u00e9\\u20ac', 'latin1').toString('hex'), Buffer.from('\\u0141\\u00e9', 'ascii').toString('hex'), Buffer.from('\\ud83d\\ude00', 'latin1').toString('hex'), Buffer.from('\\ud83d', 'ascii').toString('hex')]`);
expr(`(function () { var out = []; var w = function (n, text, a, b) { var q = Buffer.alloc(n); out.push(q.write(text, a, b, 'ucs2'), H(q)) }; w(5, 'abc', 0); w(5, 'abc', 0, 1); w(5, 'abc', 0, 3); w(6, 'a\\ud83d\\ude00', 0); w(8, 'a\\ud83d\\ude00', 0); w(3, '\\ud83d\\ude00', 0); w(3, '\\ud83d', 0); return out })()`);
expr(`[Buffer.from('\\ud83d', 'ucs2').toString('hex'), Buffer.from('a\\u00e9', 'utf16le').toString('hex'), Buffer.byteLength('\\ud800', 'ucs2'), Buffer.from([0x3d, 0xd8]).toString('ucs2').length, Buffer.from([0x3d, 0xd8, 0x00, 0xde, 0x61]).toString('ucs2'), Buffer.from([0, 0xdc]).toString('utf16le').length, Buffer.from([0x61]).toString('ucs2')]`);
expr(`(function () { var b = Buffer.alloc(4); return [b.write('a\\u20ac\\u20ac', 0), H(b), b.write('\\ud83d\\ude00', 1), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(6); return [b.write('\\ud83d\\ude00', 0, 3), H(b), b.write('a\\ud83d\\ude00', 0, 4), H(b), b.write('a\\ud83d\\ude00', 0, 5), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(6); return [b.write('\\ud83da', 0, 3), H(b), b.write('\\ud83d', 0, 2), b.write('\\ud83d', 0, 3), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(8); return [b.write('\\ud800a'), H(b)] })()`);
expr(`['a\\ud800b', '\\udc00\\ud800', '\\ud800', '\\ude00x', '\\ud83d\\ud83d', '\\ude00\\ud83d'].map(function (s) { return H(Buffer.from(s)) }).concat([Buffer.byteLength('\\ud800'), Buffer.byteLength('a\\udc00\\ud800'), Buffer.from('\\ud83d\\ude00').length, H(Buffer.alloc(6, '\\ud800'))])`);
expr(`[[0xff], [0xc3], [0xe2, 0x82], [0xe2, 0x82, 0x41], [0xf0, 0x9f, 0x98], [0xf0, 0x9f, 0x98, 0x41], [0xc0, 0x80], [0xed, 0xa0, 0x80], [0xf4, 0x90, 0x80, 0x80], [0x80, 0x80], [0xe0, 0x80, 0x80], [0xf8, 0x88, 0x80, 0x80, 0x80], [0xc1, 0xbf], [0xf0, 0x80, 0x80, 0x80], [0xe2, 0x28, 0xa1], [0xf0, 0x28, 0x8c, 0xbc], [0xe0, 0xa0], [0xf4, 0x8f, 0xbf], [0xed, 0xa0], [0xf5, 0x80], [0xe0, 0x9f, 0xbf], [0xf0, 0x8f, 0xbf, 0xbf], [0xf4, 0x8f, 0xbf, 0xbf], [0xc2], [0xef, 0xbb, 0xbf, 0x61], [0xed, 0xbf, 0xbf], [0xf0, 0x90, 0x80], [0xf1, 0x80, 0x80, 0xc0]].map(function (a) { return JSON.stringify(Buffer.from(a).toString()) })`);
expr(`(function () { var b = Buffer.from([0x61, 0xff, 0x62, 0xc3, 0xa9, 0xe2, 0x82]); return [b.toString('utf8', 0, 3), b.toString('utf8', 3, 6), b.toString('utf8', 5), b.toString('utf8', 4, 5), b.toString('utf-8', 1, 2).length] })()`);
expr(`[Buffer.from([0xe9, 0xc1, 0x80, 0x41]).toString('ascii'), Buffer.from([0xe9, 0xc1]).toString('latin1')]`);
expr(`(function () { var b = Buffer.from([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]); return [b.readIntBE(0, 1), b.readIntBE(0, 2), b.readIntBE(0, 3), b.readIntBE(0, 4), b.readIntBE(0, 5), b.readIntBE(0, 6), b.readIntLE(0, 4), b.readIntLE(0, 5), b.readUIntLE(0, 6), b.readUIntBE(0, 6)] })()`);
expr(`(function () { var b = Buffer.from([0x92, 0x34, 0x56, 0x78, 0x9a, 0xbc]); return [b.readIntBE(0, 1), b.readIntBE(0, 2), b.readIntBE(0, 3), b.readIntBE(0, 4), b.readIntBE(0, 5), b.readIntBE(0, 6), b.readIntLE(0, 6), b.readIntLE(5, 1), b.readIntLE(4, 2), b.readIntLE(3, 3)] })()`);
expr(`(function () { var b = Buffer.from([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); var c = Buffer.from([0x80, 0, 0, 0, 0, 0]); var d = Buffer.from([0x7f, 0xff, 0xff, 0xff, 0xff, 0xff]); return [b.readIntBE(0, 6), b.readIntLE(0, 6), b.readUIntBE(0, 6), b.readIntBE(0, 5), b.readIntBE(1, 4), c.readIntBE(0, 6), c.readIntBE(0, 5), c.readIntBE(0, 1), c.readIntLE(5, 1), c.readIntBE(0, 4), c.readIntBE(0, 3), c.readIntBE(0, 2), d.readIntBE(0, 6), d.readUIntBE(0, 6), d.readIntLE(0, 6)] })()`);
expr(`(function () { var b = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]); return [b.readUIntBE(2, 6), b.readIntLE(0, 6), b.readUIntLE(2, 6), b.readIntBE(2, 5), b.readUIntBE(1, 3), b.readIntLE(2, 4), b.readIntBE(-0, 1)] })()`);
for (const code of [
  `Buffer.alloc(6).readIntBE(1, {})`, `Buffer.alloc(8).readIntBE(0, 7)`, `Buffer.alloc(8).readIntBE(0, null)`, `Buffer.alloc(8).readIntBE('0', 1)`,
  `Buffer.alloc(8).readIntBE(0, 2 ** 33)`, `Buffer.alloc(8).readIntBE(undefined, 1)`, `Buffer.alloc(8).readUIntBE(undefined, 'x')`, `Buffer.alloc(2).readUIntBE(undefined, 3)`,
  `Buffer.alloc(2).readUIntBE(9, 3)`, `Buffer.alloc(2).readUIntBE(9, 7)`, `Buffer.alloc(8).readUIntBE(1.5, 'x')`, `Buffer.alloc(8).readUIntBE(1.5)`, `Buffer.alloc(8).readUIntBE()`,
  `Buffer.alloc(8).readUIntBE(null, 1)`, `Buffer.alloc(8).readIntBE(0, 2.5)`, `Buffer.alloc(8).readUIntBE(0, '2')`, `Buffer.alloc(8).readUIntBE(0, 0)`,
  `Buffer.alloc(8).readUIntLE(0, -1)`, `Buffer.alloc(8).readUIntBE(6, 3)`, `Buffer.alloc(8).readIntLE(-1, 3)`, `Buffer.alloc(2).readIntLE(0, 3)`,
  `Buffer.alloc(2).readIntLE(0, 6)`, `Buffer.alloc(8).readIntLE(Infinity, 3)`, `Buffer.alloc(8).readIntLE(NaN, 3)`, `Buffer.alloc(8).readIntBE(0, Infinity)`,
  `Buffer.alloc(8).writeIntBE(2 ** 47, 0, 6)`, `Buffer.alloc(8).writeIntLE(-129, 0, 1)`, `Buffer.alloc(8).writeUIntBE(2 ** 48, 0, 6)`, `Buffer.alloc(8).writeIntBE(0x80000000, 0, 4)`,
  `Buffer.alloc(8).writeUIntBE(-1, 0, 5)`, `Buffer.alloc(8).writeIntBE(1, 5, 5)`, `Buffer.alloc(8).writeUIntLE(2 ** 32, 0, 4)`, `Buffer.alloc(8).writeIntBE(2 ** 39, 0, 5)`,
  `Buffer.alloc(8).writeUIntBE(Infinity, 0, 5)`, `Buffer.alloc(8).writeUIntBE(1, undefined, 5)`, `Buffer.alloc(2).writeUIntBE(1, undefined, 3)`, `Buffer.alloc(8).writeUIntBE(1, 1.5, 2)`,
  `Buffer.alloc(8).writeUIntBE(1, 0, 7)`, `Buffer.alloc(8).writeUIntBE(1, 0, undefined)`, `Buffer.alloc(8).writeUIntBE(1, 0, 0)`, `Buffer.alloc(8).writeUIntBE(1, 7, 2)`,
  `Buffer.alloc(8).writeUIntBE(2 ** 50, 9, 7)`, `Buffer.alloc(8).writeUIntBE(2 ** 50, 9, 3)`, `Buffer.alloc(8).writeUIntBE(2 ** 50, 0, 3)`, `Buffer.alloc(2).writeUIntBE(1, 0, 3)`,
  `Buffer.alloc(2).writeUIntBE(2 ** 50, 0, 3)`, `Buffer.alloc(8).writeUIntBE(1)`, `Buffer.alloc(8).writeUIntBE(1, 2)`, `Buffer.alloc(8).writeUIntBE(1, 'x', 3)`,
  `Buffer.alloc(8).writeUIntBE(1, 1, 1.5)`, `Buffer.alloc(8).writeUIntBE(1, 1, NaN)`, `Buffer.alloc(8).writeUIntBE(1, -1, 2)`, `Buffer.alloc(8).writeIntLE(Number.MAX_SAFE_INTEGER, 0, 6)`,
]) expr(code);
expr(`(function () { var b = Buffer.alloc(8); return [b.writeIntBE(-0x123456, 0, 3), H(b), b.writeIntLE(-0x123456, 2, 3), H(b), b.writeIntBE(0x12345678, 0, 4), H(b), b.writeUIntBE(0xffffffffffff, 0, 6), H(b), b.writeIntLE(-1, 0, 6), H(b), b.writeIntBE(0x7fffffffff, 1, 5), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(8); return [b.writeIntBE(1.9, 0, 3), H(b), b.writeUIntLE(2.5, 0, 2), H(b), b.writeIntBE(-1.9, 0, 2), H(b)] })()`);
expr(`(function () { var b = Buffer.alloc(8); return [b.writeIntBE('1', 0, 5), H(b), b.writeIntBE(NaN, 0, 5), H(b), b.writeUIntBE('0x10', 0, 2), H(b), b.writeIntLE(true, 0, 2), H(b), b.writeUIntLE(null, 0, 2), b.writeUIntLE(undefined, 0, 2), H(b), b.writeIntBE(-0, 0, 2)] })()`);
expr(`[Buffer.alloc(8).writeIntBE('abc', 0, 5), Buffer.alloc(8).writeIntBE({}, 0, 5)]`);
expr(`(function () { var b = Buffer.alloc(8); return [b.writeIntLE(-129, 0, 2), H(b), b.writeIntBE(-32768, 2, 2), H(b), b.writeUIntLE(0xabcdef, 0, 3), H(b), b.writeIntBE(1.5, 0, 1)] })()`);
};

// Símbolos do protótipo, `inspect` custom, acessores `offset`/`parent` e receptores de equals/compare/indexOf (273 a 292).
expr(`Reflect.ownKeys(Buffer.prototype).slice(-3).map(String).join()`);
expr(`JSON.stringify(Object.getOwnPropertyDescriptor(Buffer.prototype, Symbol.toStringTag))`);
expr(`(function () { var d = Object.getOwnPropertyDescriptor(Buffer.prototype, Symbol.species); return [d.value === Buffer, d.writable, d.enumerable, d.configurable].join() })()`);
expr(`(function () { var d = Object.getOwnPropertyDescriptor(Buffer.prototype, Symbol.for('nodejs.util.inspect.custom')); return [d.value === Buffer.prototype.inspect, d.writable, d.enumerable, d.configurable, d.value.length].join() })()`);
expr(`Object.prototype.toString.call(Buffer.from('abc'))`);
expr(`(function () { var a = Object.getOwnPropertyDescriptor(Buffer.prototype, 'offset'), p = Object.getOwnPropertyDescriptor(Buffer.prototype, 'parent'); return [typeof a.get, a.set, a.enumerable, a.configurable, a.get.name, typeof p.get, p.set, p.enumerable, p.configurable, p.get.name].join() })()`);
expr(`Object.getOwnPropertyNames(Buffer.prototype).slice(17, 21).join()`);
expr(`(function () { var b = Buffer.from('abcdef').subarray(2); return [b.offset, b.parent === b.buffer, b.parent instanceof ArrayBuffer].join() })()`);
expr(`(function () { var g = Object.getOwnPropertyDescriptor(Buffer.prototype, 'offset').get, h = Object.getOwnPropertyDescriptor(Buffer.prototype, 'parent').get; return [g.call({}), h.call({}), g.call(null) ].join() })()`);
expr(`Bun.inspect(Buffer.from('abc'))`);
expr(`Bun.inspect(Buffer.alloc(0))`);
expr(`Bun.inspect(Buffer.alloc(60))`);
expr(`(function () { var b = Buffer.from('abc'); b.x = 1; b.y = 'q'; return Bun.inspect(b) })()`);
expr(`(function () { var b = Buffer.alloc(2); b.a = { b: { c: 1 } }; return Bun.inspect(b) })()`);
expr(`(function () { var b = Buffer.from('abc'); b.x = 1; return [Buffer.prototype.inspect.call(b), b[Symbol.for('nodejs.util.inspect.custom')](), Buffer.prototype.inspect.call(b, 2, {})].join('|') })()`);
expr(`Bun.inspect([Buffer.from('ab')])`);
expr(`(function () { var out = []; for (var m of ['equals', 'compare', 'indexOf', 'lastIndexOf', 'includes']) for (var t of [{}, 1, null, undefined]) { try { out.push(String(Buffer.prototype[m].call(t, Buffer.from('a')))) } catch (e) { out.push(e.name + ':' + e.code + ':' + e.message) } } return out.join('\\n') })()`);
expr(`(function () { var out = []; for (var m of ['equals', 'compare', 'indexOf', 'lastIndexOf', 'includes']) { try { out.push(String(Buffer.prototype[m].call(new Uint8Array(2), Buffer.from('a')))) } catch (e) { out.push(e.message) } } return out.join() })()`);
expr(`(function () { try { Buffer.from('ab').equals(1) } catch (e) { return e.code + ':' + e.message } })()`);
expr(`(function () { try { Buffer.from('ab').compare(1) } catch (e) { return e.code + ':' + e.message } })()`);
// 273 a 292

// Bloco: erros de receptor de `toString`/`slice`/`subarray`/`toLocaleString` e extras do `inspect` custom.
expr(`(function () { var out = []; for (var m of ['toString', 'slice', 'subarray', 'toLocaleString']) for (var t of [null, undefined, {}, new Uint16Array(2), 5]) { try { out.push(String(Buffer.prototype[m].call(t))) } catch (e) { out.push(m + ':' + e.name + ':' + e.code + ':' + e.message) } } return out.join('\\n') })()`);
expr(`(function () { var out = []; for (var m of ['toString', 'slice', 'subarray', 'toLocaleString']) out.push(String(Buffer.prototype[m].call(new Uint8Array([97])))); return out.join() })()`);
expr(`(function () { var b = Buffer.from('ab'); b.x = 1; b['a-b'] = 2; b['3x'] = 3; Object.defineProperty(b, 'g', { get: function () { return 7 }, enumerable: true }); Object.defineProperty(b, 'h', { value: 1, enumerable: false }); return b.inspect(0, {}) })()`);
expr(`(function () { var b = Buffer.from('ab'); b[Symbol('s')] = 1; b.t = 2; b[Symbol()] = 3; b[Symbol('')] = 4; b[Symbol.iterator] = 5; b[Symbol.for('k')] = 6; return b.inspect(0, {}) })()`);
expr(`(function () { var b = Buffer.from('ab'); b.o = { a: { b: { c: { d: { e: 1 } } } } }; b.arr = [[[[1]]]]; return b.inspect(0, {}) + '|' + b.inspect(0, { depth: 0 }) + '|' + b.inspect() })()`);
// 293 a 297

// Bloco: receptores ruins de todos os métodos de `Buffer.prototype` (this nulo, undefined, {}, Uint16Array, número,
// Uint8Array comum), `toJSON` genérico (`Array.from(this)`) e `read*`/`write*`/`xxxSlice` sobre outro typed array.
expr(`(function () { var out = []; var ms = Object.getOwnPropertyNames(Buffer.prototype).filter(function (k) { return typeof Buffer.prototype[k] === 'function' && k !== 'constructor' }); for (var m of ms) { var row = []; for (var t of [null, undefined, {}, new Uint16Array(4), 5, new Uint8Array(16)]) { try { Buffer.prototype[m].call(t, 0, 0); row.push('ok') } catch (e) { row.push(e.name + ':' + e.code + ':' + e.message) } } out.push(m + '=' + row.join('|')) } return out.join('\\n') })()`);
expr(`(function () { var out = []; for (var t of [null, undefined, {}, new Uint16Array([1, 258]), 5, new Uint8Array([1, 2]), 'ab', { length: 2, 0: 7, 1: 8 }, [3, 4], '\\ud83d\\ude00\\ud800', true]) { try { out.push(JSON.stringify(Buffer.prototype.toJSON.call(t))) } catch (e) { out.push(e.name + ':' + e.code + ':' + e.message) } } return out.join('\\n') })()`);
expr(`(function () { var u = new Uint16Array([0x0102, 0x0304, 0x0506, 0x0708]); var r = Buffer.prototype; var o = []; var t = function (f) { try { o.push(String(f())) } catch (e) { o.push(e.code + ':' + e.message) } }; t(function () { return r.readUInt8.call(u, 0) }); t(function () { return r.readUInt8.call(u, 3) }); t(function () { return r.readUInt8.call(u, 4) }); t(function () { return r.readUInt16LE.call(u, 0) }); t(function () { return r.readUInt16LE.call(u, 6) }); t(function () { return r.readUInt32LE.call(u, 4) }); t(function () { return r.readDoubleLE.call(u, 0) }); t(function () { return r.readUIntLE.call(u, 0, 2) }); t(function () { return r.writeUInt8.call(u, 0xaa, 1) }); t(function () { return u[0].toString(16) }); t(function () { return r.writeUInt8.call(u, 0xbb, 4) }); t(function () { return r.readUInt8.call(new Float32Array(2), 4) }); t(function () { return r.hexSlice.call(u, 0, 4) }); return o.join('\\n') })()`);
expr(`(function () { var out = []; for (var m of ['fill', 'swap16', 'swap32', 'swap64', 'inspect', 'write', 'toJSON']) for (var t of [null, undefined]) { try { Buffer.prototype[m].call(t) } catch (e) { out.push(m + ':' + e.name + ':' + e.code + ':' + e.message) } } return out.join('\\n') })()`);
// DataView como this de read*/write*: o bun mede o limite com length indefinido (teto NaN) e nada passa.
expr(`(function () { var r = Buffer.prototype; var o = []; var t = function (n, f) { try { o.push(n + '=' + String(f())) } catch (e) { o.push(n + '=' + e.name + ':' + e.code + ':' + e.message) } }; var dv = new DataView(new ArrayBuffer(8)); for (var m of ['readDoubleLE', 'readUInt8', 'readInt16BE', 'readUInt32LE', 'readFloatBE', 'readBigInt64LE']) t(m, function () { return r[m].call(dv, 0) }); t('u', function () { return r.readUInt8.call(dv) }); t('n', function () { return r.readUInt8.call(dv, -1) }); t('f', function () { return r.readUInt8.call(dv, 1.5) }); t('s', function () { return r.readUInt8.call(dv, 'a') }); t('wu8', function () { return r.writeUInt8.call(dv, 5, 0) }); t('wd', function () { return r.writeDoubleLE.call(dv, 1.5, 0) }); t('wi32', function () { return r.writeInt32BE.call(dv, 1, 0) }); t('wb', function () { return r.writeBigInt64LE.call(dv, 1n, 0) }); t('after', function () { return Array.from(new Uint8Array(dv.buffer)).join() }); var d2 = new DataView(new ArrayBuffer(8), 2, 4); t('d2r', function () { return r.readUInt8.call(d2, 0) }); t('d2w', function () { return r.writeUInt8.call(d2, 9, 0) }); return o.join('\\n') })()`);
// DataView como this: a ordem dos erros (value, tipo do offset, byteLength) é a do Buffer, e só o offset numérico falha
expr(`(function () { var r = Buffer.prototype; var o = []; var t = function (n, f) { try { o.push(n + '=' + String(f())) } catch (e) { o.push(n + '=' + e.name + ':' + e.code + ':' + e.message) } }; var dv = new DataView(new ArrayBuffer(8)); t('u8v', function () { return r.writeUInt8.call(dv, 300, 0) }); t('u8vu', function () { return r.writeUInt8.call(dv, 300) }); t('u8o', function () { return r.writeUInt8.call(dv, 5, 0) }); t('u8ou', function () { return r.writeUInt8.call(dv, 5) }); t('u8s', function () { return r.writeUInt8.call(dv, 5, 'a') }); t('u8vs', function () { return r.writeUInt8.call(dv, 300, 'a') }); t('u8vf', function () { return r.writeUInt8.call(dv, 300, 1.5) }); t('u8of', function () { return r.writeUInt8.call(dv, 5, 1.5) }); t('u8sym', function () { return r.writeUInt8.call(dv, Symbol(), 0) }); t('u8big', function () { return r.writeUInt8.call(dv, 1n, 0) }); t('u8str', function () { return r.writeUInt8.call(dv, 'x', 0) }); t('i32', function () { return r.writeInt32LE.call(dv, 2 ** 40, 0) }); t('dsym', function () { return r.writeDoubleLE.call(dv, Symbol(), 0) }); t('dbig', function () { return r.writeDoubleLE.call(dv, 1n, 0) }); t('fo', function () { return r.writeFloatLE.call(dv, 1, 0) }); t('fs', function () { return r.writeFloatLE.call(dv, 1, 'a') }); t('ib1', function () { return r.writeIntBE.call(dv, 300, 0, 1) }); t('ib7', function () { return r.writeIntBE.call(dv, 5, 0, 7) }); t('ib7s', function () { return r.writeIntBE.call(dv, 5, 'a', 7) }); t('ib7v', function () { return r.writeIntBE.call(dv, 300, 0, 7) }); t('ibla', function () { return r.writeIntBE.call(dv, 5, 0, 'a') }); t('ibu', function () { return r.writeIntBE.call(dv, 5, undefined, 3) }); t('ibo', function () { return r.writeIntBE.call(dv, 5, 0, 3) }); t('ibn', function () { return r.writeIntBE.call(dv, 5, -1, 3) }); t('ibvs', function () { return r.writeIntBE.call(dv, 300, 'a', 1) }); t('ibsym', function () { return r.writeIntBE.call(dv, Symbol(), 'a', 1) }); t('ul2', function () { return r.writeUIntLE.call(dv, 2 ** 30, 0, 2) }); t('b1', function () { return r.writeBigInt64LE.call(dv, 1n, 0) }); t('b1u', function () { return r.writeBigInt64LE.call(dv, 1n) }); t('b1s', function () { return r.writeBigInt64LE.call(dv, 1n, 'a') }); t('bnum', function () { return r.writeBigInt64LE.call(dv, 1, 0) }); t('bnums', function () { return r.writeBigInt64LE.call(dv, 1, 'a') }); t('bbig', function () { return r.writeBigInt64LE.call(dv, 2n ** 64n, 0) }); t('bbigs', function () { return r.writeBigInt64LE.call(dv, 2n ** 64n, 'a') }); t('bneg', function () { return r.writeBigUInt64BE.call(dv, -1n, 0) }); t('bsym', function () { return r.writeBigUInt64BE.call(dv, Symbol(), 0) }); t('rl7', function () { return r.readUIntLE.call(dv, 0, 7) }); t('rls', function () { return r.readUIntLE.call(dv, 'a', 2) }); t('rlo', function () { return r.readUIntLE.call(dv, 0, 2) }); t('rlu', function () { return r.readUIntLE.call(dv, undefined, 2) }); t('rla', function () { return r.readUIntLE.call(dv, 0, 'a') }); t('rl0', function () { return r.readUIntBE.call(dv, 0.5, 0) }); t('rlf', function () { return r.readUIntBE.call(dv, 0.5, 2) }); t('rsym', function () { return r.readUInt8.call(dv, Symbol()) }); t('rdf', function () { return r.readDoubleLE.call(dv, 1.5) }); return o.join('\\n') })()`);
// Typed array não Uint8 com byteOffset: leitura e escrita partem do byteOffset, o limite vem de length em elementos.
expr(`(function () { var r = Buffer.prototype; var o = []; var t = function (n, f) { try { o.push(n + '=' + String(f())) } catch (e) { o.push(n + '=' + e.code + ':' + e.message) } }; var b = Buffer.from([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]); var u = new Uint16Array(b.buffer, b.byteOffset + 2, 3); t('r0', function () { return r.readUInt8.call(u, 0) }); t('r2', function () { return r.readUInt8.call(u, 2) }); t('r3', function () { return r.readUInt8.call(u, 3) }); t('r16', function () { return r.readUInt16LE.call(u, 1) }); t('r16b', function () { return r.readUInt16LE.call(u, 2) }); t('w', function () { return r.writeUInt8.call(u, 0xaa, 1) }); t('a1', function () { return Array.from(b).join() }); t('w2', function () { return r.writeUInt16BE.call(u, 0x1234, 2) }); t('a2', function () { return Array.from(b).join() }); t('w3', function () { return r.writeUInt8.call(u, 1, 3) }); t('rd', function () { return r.readDoubleLE.call(u, 0) }); return o.join('\\n') })()`);
// 298 a 301
bufferEncodingBlock();
// 298 a 383

// Bloco 9 (a partir do índice 384): `BigInt` onde se espera número em `write*` (TypeError sem code), ordem das checagens
// de `value`, `offset` e `byteLength`, `Symbol` como valor e as variações de argumentos de `buf.write`.
for (const code of [
  `Buffer.alloc(16).writeUIntBE(1n, 0, 1)`, `Buffer.alloc(16).writeInt8(1n)`, `Buffer.alloc(16).writeUInt32LE(1n)`, `Buffer.alloc(16).writeUInt8(1n, 0)`,
  `Buffer.alloc(16).writeInt16BE(1n, 0)`, `Buffer.alloc(16).writeIntLE(1n, 0, 3)`, `Buffer.alloc(16).writeUIntLE(1n, 0, 6)`, `Buffer.alloc(16).writeFloatLE(1n, 0)`,
  `Buffer.alloc(16).writeDoubleBE(1n, 0)`, `Buffer.alloc(16).writeUInt8(1, 1n)`, `Buffer.alloc(16).readUInt8(1n)`, `Buffer.alloc(16).readUIntBE(0, 1n)`,
  `Buffer.alloc(16).writeUInt8(Symbol(), 0)`, `Buffer.alloc(16).writeIntBE(1n, 100, 2)`, `Buffer.alloc(16).writeIntBE(1n, 0, 7)`, `Buffer.alloc(16).writeUInt8(1n, 100)`,
  `Buffer.alloc(16).writeIntBE(1e20, undefined, 2)`, `Buffer.alloc(16).writeIntBE('x', 'y', 2)`, `Buffer.alloc(16).writeUInt8(300, -1)`, `Buffer.alloc(16).writeUInt8('x', -1)`,
  `Buffer.alloc(16).writeUInt8(300, 'y')`, `Buffer.alloc(16).writeIntBE(1, 0, 7)`, `Buffer.alloc(16).writeIntBE(1e20, 0, 7)`, `Buffer.alloc(16).writeIntBE(1, 0, 'x')`,
  `Buffer.alloc(16).writeIntBE(1, 0)`, `Buffer.alloc(16).writeIntBE(1e20, 100, 2)`, `Buffer.alloc(16).writeUInt32LE(1e20, 100)`, `Buffer.alloc(16).writeUInt16BE(70000, 15)`,
  `Buffer.alloc(16).writeUInt16BE(1, 15)`, `Buffer.alloc(16).writeUInt16BE(1, 1.5)`, `Buffer.alloc(16).writeUInt8(1, null)`, `Buffer.alloc(16).writeUInt8(-1, 0)`,
  `Buffer.alloc(16).writeBigInt64LE(1, 0)`, `Buffer.alloc(16).writeBigInt64LE(1n, 10)`, `Buffer.alloc(16).writeBigInt64LE(2n ** 64n, 100)`, `Buffer.alloc(16).writeBigUInt64LE(-1n, 0)`,
  `Buffer.alloc(16).readUIntBE(0, 0)`, `Buffer.alloc(16).readInt32LE(14)`, `Buffer.alloc(16).readInt32LE(NaN)`, `Buffer.alloc(16).readInt32LE(Infinity)`,
  `Buffer.alloc(16).write('ab', 0, 2, 'bogus')`, `Buffer.alloc(16).write('ab', 0, undefined, 'bogus')`, `Buffer.alloc(16).write('ab', 17)`, `Buffer.alloc(16).write('ab', -1)`,
  `Buffer.alloc(16).write('ab', 0, -1)`, `Buffer.alloc(16).write('ab', 0, 100)`, `Buffer.alloc(16).write('ab', 0, 'x')`, `Buffer.alloc(16).write('ab', 'x', 1)`,
  `Buffer.alloc(16).write('ab', null)`, `Buffer.alloc(16).write(1)`, `Buffer.alloc(16).write('ab', 1.5)`, `Buffer.alloc(16).write('ab', 0, 1.5)`,
  `Buffer.alloc(16).write('ab', 0, undefined, 5)`, `Buffer.alloc(16).write('ab', {}, 1)`, `Buffer.alloc(16).write('ab', 0, 'ab', 'utf8')`, `Buffer.alloc(16).write('ab', 'x')`,
  `Buffer.alloc(16).write('ab', 1, 'x')`, `Buffer.alloc(16).write('ab', 0, null)`, `Buffer.alloc(16).utf8Write('ab', 'x')`, `Buffer.alloc(16).utf8Write('ab', 17)`,
  `Buffer.alloc(16).utf8Write('ab', 1.5)`,
]) expr(code);
expr(`(function () { var b = Buffer.alloc(16); var out = []; var w = function (a, c, d, e) { b.fill(0); out.push(b.write('ab', a, c, d), H(b)) }; w(0, undefined, 'ucs2'); w(0, 'ucs2'); w('ucs2'); w(1, 'hex'); w(1, undefined, 'hex'); w(undefined, 2); w(undefined, undefined, 'ucs2'); w(3, undefined, 'ucs2'); w(15, undefined, 'ucs2'); w(0, 3, 'ucs2'); w(0, 2, null); w(0, 0); w(16, 0); w(16); w(0, 1, 'ucs2'); w(0, undefined, undefined); w(0, undefined, null); w(0, 2, 'UCS-2'); w(undefined, undefined, 'latin1'); return out })()`);
expr(`(function () { var b = Buffer.alloc(8); var out = [b.write('abcd', 1, 2, 'hex'), H(b), b.write('\\u20ac\\u20ac', 0, 4, 'utf8'), H(b)]; return out })()`);
// 384 em diante

// Execução: duas rodadas idênticas em cada programa.
(async () => {
  const run = async (source) => {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 20; i++) await Promise.resolve();
    return [sourceAscii, String(globalThis.R === undefined ? "<undefined>" : globalThis.R)];
  };
  let unstable = 0;
  for (const source of programs) {
    const [src, first] = await run(source);
    const [, second] = await run(source);
    if (first !== second) {
      unstable++;
      process.stderr.write(`INSTÁVEL: ${src.slice(HELPER.length, HELPER.length + 120)}\n  1: ${first}\n  2: ${second}\n`);
    }
    emitRow(JSON.stringify(src) + "\t" + JSON.stringify(first));
  }
  process.stderr.write(`${programs.length} programas, ${unstable} instáveis\n`);
  if (unstable) process.exitCode = 1;
})();
