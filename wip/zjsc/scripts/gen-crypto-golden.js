// Gera tests/golden/crypto_bun.tsv: `crypto`, `Crypto` e `SubtleCrypto` do global medidos no bun 1.4.2 (descritores, protótipo,
// toStringTag, getter `subtle`, `getRandomValues` com todos os tipos, o `TypeMismatchError`, `randomUUID` por regex, `timingSafeEqual`,
// `this` alheio e os métodos de `SubtleCrypto` que rejeitam por falta de argumento ou `this`). Valores aleatórios entram só por
// forma (regex, tamanho, identidade), nunca por valor.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), lido depois do esvaziamento das promessas.
// Uso: bun scripts/gen-crypto-golden.js > tests/golden/crypto_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) + '|' + e.constructor.name };\n" +
  "var D = function (d) { return d ? [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get, typeof d.set] : 'none' };\n" +
  "var Y = function (f) { try { return String(f()) } catch (e) { return '!' + E(e) } };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const aexpr = (code) => programs.push(HELPER + `try { Promise.resolve(${code}).then(function (v) { R = S(v) }, function (e) { R = 'rejeitou ' + E(e) }) } catch (e) { R = E(e) }`);

for (const n of ["crypto", "Crypto", "SubtleCrypto"]) expr(`D(Object.getOwnPropertyDescriptor(globalThis, '${n}'))`);
expr(`Object.prototype.toString.call(crypto)`);
expr(`String(crypto.subtle)`);
expr(`Object.getPrototypeOf(crypto) === Crypto.prototype`);
expr(`Object.getPrototypeOf(crypto.subtle) === SubtleCrypto.prototype`);
expr(`Object.getOwnPropertyNames(crypto)`);
expr(`Reflect.ownKeys(Crypto.prototype).map(String)`);
expr(`Reflect.ownKeys(SubtleCrypto.prototype).map(String)`);
expr(`Object.getOwnPropertyNames(Crypto)`);
expr(`[Crypto.name, Crypto.length, SubtleCrypto.name, SubtleCrypto.length]`);
expr(`Object.getPrototypeOf(Crypto) === Function.prototype`);
for (const k of ["getRandomValues", "randomUUID", "timingSafeEqual", "constructor", "subtle"]) {
  expr(`D(Object.getOwnPropertyDescriptor(Crypto.prototype, '${k}'))`);
  expr(`(function(d){ var f = d.value || d.get; return [f.name, f.length, d.set && d.set.name, d.set && d.set.length] })(Object.getOwnPropertyDescriptor(Crypto.prototype, '${k}'))`);
}
expr(`D(Object.getOwnPropertyDescriptor(Crypto.prototype, Symbol.toStringTag))`);
expr(`Crypto.prototype[Symbol.toStringTag]`);
expr(`SubtleCrypto.prototype[Symbol.toStringTag]`);
expr(`crypto.subtle === crypto.subtle`);
expr(`(function(){ var s = crypto.subtle; crypto.subtle = 5; return [crypto.subtle === s, Object.getOwnPropertyNames(crypto)] })()`);
expr(`Object.getOwnPropertyDescriptor(Crypto.prototype, 'subtle').set.call({}, 1)`);
expr(`Object.getOwnPropertyDescriptor(Crypto.prototype, 'subtle').get.call({})`);
expr(`Object.getOwnPropertyDescriptor(Crypto.prototype, 'subtle').get.call(null)`);
for (const m of ["encrypt", "decrypt", "sign", "verify", "digest", "generateKey", "deriveKey", "deriveBits", "importKey", "exportKey", "wrapKey", "unwrapKey", "getPublicKey", "encapsulateBits", "encapsulateKey", "decapsulateBits", "decapsulateKey"]) {
  expr(`D(Object.getOwnPropertyDescriptor(SubtleCrypto.prototype, '${m}'))`);
  expr(`[SubtleCrypto.prototype.${m}.name, SubtleCrypto.prototype.${m}.length]`);
  aexpr(`crypto.subtle.${m}()`);
  aexpr(`SubtleCrypto.prototype.${m}.call({}, 1, 2, 3, 4, 5, 6, 7)`);
}
// Construtores.
for (const c of ["Crypto", "SubtleCrypto"]) {
  expr(`new ${c}()`);
  expr(`${c}()`);
}
// getRandomValues.
for (const t of ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "BigInt64Array", "BigUint64Array", "Float32Array", "Float64Array"]) {
  expr(`(function(a){ return [crypto.getRandomValues(a) === a, a.constructor.name, a.length] })(new ${t}(4))`);
}
expr(`(function(a){ return [crypto.getRandomValues(a) === a, a.length, a.some(function (x) { return x !== 0 })] })(new Uint8Array(32))`);
expr(`(function(a){ return [crypto.getRandomValues(a) === a, a.length] })(new Uint8Array(65537))`);
expr(`(function(a){ return [crypto.getRandomValues(a) === a, a.length] })(new Uint32Array(16385))`);
expr(`crypto.getRandomValues(new Uint8Array(0)).length`);
expr(`(function(b){ var v = new Uint8Array(b); v.fill(7); crypto.getRandomValues(v.subarray(4, 8)); return [Array.from(v.subarray(0, 4)).join(), Array.from(v.subarray(8)).join(), v.subarray(4, 8).some(function (x) { return x !== 7 }) || 'igual'] })(new ArrayBuffer(12))`);
expr(`crypto.getRandomValues(Buffer.alloc(4)) instanceof Buffer`);
expr(`crypto.getRandomValues(new Uint8Array(new SharedArrayBuffer(4))).length`);
// Buffer desanexado não lança: o comprimento é 0. Buffer redimensionável e limites de 65536 bytes.
expr(`(function(){ var b = new ArrayBuffer(8); var v = new Uint8Array(b); structuredClone(b, { transfer: [b] }); return crypto.getRandomValues(v).length })()`);
expr(`crypto.getRandomValues(new Uint8Array(new ArrayBuffer(8, { maxByteLength: 16 }))).length`);
expr(`crypto.getRandomValues(new Uint8Array(65536)).length`);
expr(`crypto.getRandomValues(new Uint32Array(16384)).length`);
expr(`(function(){ var f = crypto.getRandomValues; try { f(new Uint8Array(2)) } catch (e) { return E(e) } })()`);
expr(`(function(){ try { crypto.randomUUID.call({}) } catch (e) { return E(e) } })()`);
expr(`(function(){ try { Crypto() } catch (e) { return E(e) } })()`);
expr(`(function(){ try { new Crypto() } catch (e) { return E(e) } })()`);
expr(`[typeof crypto, crypto instanceof Crypto, crypto.randomUUID.length, crypto.getRandomValues.length]`);
for (const v of ["new DataView(new ArrayBuffer(4))", "new ArrayBuffer(4)", "undefined", "null", "[1]", "5", "{}", "'abc'"]) {
  expr(`crypto.getRandomValues(${v})`);
  expr(`(function(){ try { crypto.getRandomValues(${v}) } catch (e) { return [e instanceof DOMException, e.name, e.code, e.message, Object.prototype.toString.call(e)] } })()`);
}
expr(`crypto.getRandomValues()`);
for (const t of ["undefined", "null", "{}", "1", "'x'", "globalThis"]) {
  expr(`Crypto.prototype.getRandomValues.call(${t}, new Uint8Array(4))`);
  expr(`Crypto.prototype.randomUUID.call(${t})`);
  expr(`Crypto.prototype.timingSafeEqual.call(${t}, new Uint8Array(1), new Uint8Array(1))`);
}
// randomUUID.
expr(`/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID())`);
expr(`(function(u){ return [typeof u, u.length] })(crypto.randomUUID(1, 2))`);
expr(`new Set([crypto.randomUUID(), crypto.randomUUID(), crypto.randomUUID()]).size`);
// timingSafeEqual.
expr(`crypto.timingSafeEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]))`);
expr(`crypto.timingSafeEqual(new Uint8Array([1, 2]), new Uint8Array([1, 3]))`);
expr(`crypto.timingSafeEqual(new ArrayBuffer(2), new Uint16Array(1))`);
expr(`crypto.timingSafeEqual(new Uint8Array(2), new Uint8Array(3))`);
expr(`crypto.timingSafeEqual()`);
expr(`crypto.timingSafeEqual(1, 2)`);
expr(`crypto.timingSafeEqual(new Uint8Array(1), 2)`);
expr(`crypto.timingSafeEqual(new Uint8Array(1))`);
expr(`crypto.timingSafeEqual(new Uint8Array(1), 2)`);
expr(`crypto.timingSafeEqual(1, new Uint8Array(1))`);
expr(`crypto.timingSafeEqual('a', new Uint8Array(1))`);
// SubtleCrypto.digest: hashes por forma (hex fixo), nomes, dicionários e dados.
const hex = (code) => `crypto.subtle.digest(${code}).then(function (b) { return [b.constructor.name, Array.from(new Uint8Array(b)).map(function (x) { return ('0' + x.toString(16)).slice(-2) }).join('')] })`;
for (const a of ["sha-1", "SHA-1", "Sha-224", "SHA-256", "sha-384", "SHA-512", "sha3-256", "SHA3-384", "sha3-512", "{name:'sha-256'}", "{name:'SHA-512',x:1}"]) {
  aexpr(hex(`${a.startsWith("{") ? a : `'${a}'`}, new Uint8Array([1])`));
}
aexpr(hex(`'SHA-256', new Uint8Array(0)`));
aexpr(hex(`'SHA-1', new Uint8Array([9, 9, 1, 2, 3, 9]).subarray(2, 5)`));
aexpr(hex(`'SHA-1', new ArrayBuffer(2)`));
aexpr(hex(`'SHA-1', new DataView(new ArrayBuffer(2))`));
aexpr(hex(`'SHA-1', new Uint16Array(1)`));
aexpr(hex(`'SHA-1', Buffer.from('abc')`));
for (const a of ["'md5'", "'sha1'", "'SHA3-224'", "'SHA-512/256'", "'SHA-256 '", "''", "{}", "{name:5}", "[]", "5", "null", "undefined", "Symbol.iterator", "{name:Symbol.iterator}"]) {
  aexpr(`crypto.subtle.digest(${a}, new Uint8Array([1]))`);
}
for (const d of ["new SharedArrayBuffer(2)", "'abc'", "1", "null", "undefined", "{}", "[1]", "Symbol.iterator"]) {
  aexpr(`crypto.subtle.digest('SHA-1', ${d})`);
  aexpr(`crypto.subtle.digest('md5', ${d})`);
}
aexpr(`SubtleCrypto.prototype.digest.call({}, 'SHA-1', new Uint8Array(1))`);
// Caminho de erro dos demais métodos (antes de exigir CryptoKey).
const keyMethods = { encrypt: 3, decrypt: 3, sign: 3, verify: 4, deriveKey: 5, deriveBits: 2, getPublicKey: 2, encapsulateBits: 2, encapsulateKey: 5, decapsulateBits: 3, decapsulateKey: 6 };
for (const [m, n] of Object.entries(keyMethods)) {
  for (const v of ["1", "'foo'", "{name:'foo'}", "undefined", "null"]) aexpr(`crypto.subtle.${m}(${Array(n).fill(v).join(", ")})`);
}
for (const m of ["importKey", "exportKey", "wrapKey", "unwrapKey"]) {
  const n = { importKey: 5, exportKey: 2, wrapKey: 4, unwrapKey: 7 }[m];
  for (const v of ["1", "'foo'", "'RAW'", "{name:'foo'}", "undefined", "null", "Symbol.iterator"]) aexpr(`crypto.subtle.${m}(${Array(n).fill(v).join(", ")})`);
}
for (const f of ["raw", "jwk", "spki", "pkcs8", "raw-secret", "raw-public", "raw-seed"]) {
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), 'foo', true, [])`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), {name:'foo'}, true, [])`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), {}, true, [])`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), 'foo', true, 5)`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), 'foo', true, {})`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), 'foo', true, ['x'])`);
  aexpr(`crypto.subtle.importKey('${f}', new Uint8Array(1), 'foo', true, [1])`);
  aexpr(`crypto.subtle.importKey('${f}', 5, 'foo', true, [])`);
  aexpr(`crypto.subtle.exportKey('${f}', 1)`);
  aexpr(`crypto.subtle.wrapKey('${f}', 1, 2, 3)`);
  aexpr(`crypto.subtle.unwrapKey('${f}', new Uint8Array(1), 1, 'foo', 'foo', true, [])`);
  aexpr(`crypto.subtle.unwrapKey('${f}', 5, 1, 'foo', 'foo', true, [])`);
}
aexpr(`crypto.subtle.importKey('jwk', {}, 'foo', true, [])`);
aexpr(`crypto.subtle.importKey('jwk', 5, 'foo', true, [])`);
for (const a of ["'foo'", "{}", "5", "undefined", "{name:'foo'}", "Symbol.iterator"]) {
  aexpr(`crypto.subtle.generateKey(${a}, true, [])`);
}
for (const u of ["1", "'encrypt'", "{}", "undefined", "null", "['x']", "[1]", "[undefined]"]) aexpr(`crypto.subtle.generateKey('foo', true, ${u})`);

// Registro de algoritmos e o que cada operação aceita (CryptoAlgorithmRegistryOpenSSL.cpp e o `switch` de
// `normalizeCryptoAlgorithmParameters` em SubtleCrypto.cpp): o erro é igual para todo algoritmo que a operação recusa, e o
// caminho feliz dos que não são HMAC não existe aqui, então só os nomes que recusam entram.
for (const n of ["SHA-1", "SHA-256", "SHA3-512", "rsaes-pkcs1-v1_5", "AES-OCB", "aes-cfb", "kmac128", "ed448", "x448", "ml-kem-512", "argon2id", "cshake128", "turboshake128", "sha3-224", "md5"]) {
  aexpr(`crypto.subtle.importKey('raw', new Uint8Array(1), '${n}', true, ['sign'])`);
  aexpr(`crypto.subtle.generateKey('${n}', true, ['sign'])`);
}
for (const n of ["hkdf", "pbkdf2", "HKDF"]) aexpr(`crypto.subtle.generateKey('${n}', true, ['sign'])`);
for (const n of ["foo", "SHA-256", "rsaes-pkcs1-v1_5"]) aexpr(`crypto.subtle.importKey('raw', new Uint8Array([1, 2, 3, 4]), '${n}', true, ['sign'])`);

// CryptoKey e HMAC: chaves importadas fixas, valores determinísticos.
const HEX = (b) => `Array.from(new Uint8Array(${b})).map(function (x) { return ('0' + x.toString(16)).slice(-2) }).join('')`;
const KEY = (hash, usages = "['sign', 'verify']", extractable = "true", bytes = "[1, 2, 3, 4]") =>
  `crypto.subtle.importKey('raw', new Uint8Array(${bytes}), {name: 'HMAC', hash: '${hash}'}, ${extractable}, ${usages})`;
const WITH = (key, body) => aexpr(`${key}.then(function (k) { return ${body} })`);
const KEY_DESC = "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(k), 'type')";
WITH(KEY("SHA-256"), `[Object.prototype.toString.call(k), Object.getOwnPropertyNames(k), Reflect.ownKeys(Object.getPrototypeOf(k)).filter(function (n) { return typeof n === 'string' }), k.type, k.extractable, k.algorithm, k.usages, k.usages === k.usages, k.algorithm === k.algorithm, k instanceof CryptoKey, Object.keys(k.algorithm)]`);
WITH(KEY("SHA-256"), `[D(${KEY_DESC}), D(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(k), 'usages')), (function (g) { return [g.name, g.length, g.set] })(${KEY_DESC}.get)]`);
WITH(KEY("SHA-256"), `[Object.getPrototypeOf(k) === CryptoKey.prototype, CryptoKey.prototype[Symbol.toStringTag], Object.prototype.toString.call(CryptoKey.prototype), D(Object.getOwnPropertyDescriptor(CryptoKey.prototype, 'constructor')), D(Object.getOwnPropertyDescriptor(CryptoKey.prototype, Symbol.toStringTag))]`);
WITH(KEY("SHA-256"), `(function () { k.extractable = false; k.type = 'public'; return [k.extractable, k.type] })()`);
expr(`D(Object.getOwnPropertyDescriptor(globalThis, 'CryptoKey'))`);
expr(`[typeof CryptoKey, CryptoKey.name, CryptoKey.length, Object.getPrototypeOf(CryptoKey) === Function.prototype, Object.getOwnPropertyNames(CryptoKey)]`);
expr(`new CryptoKey()`);
expr(`CryptoKey()`);
for (const a of ["type", "extractable", "algorithm", "usages"]) {
  expr(`Object.getOwnPropertyDescriptor(CryptoKey.prototype, '${a}').get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(CryptoKey.prototype, '${a}').get.call(crypto.subtle)`);
  expr(`Object.getOwnPropertyDescriptor(CryptoKey.prototype, '${a}').get.call(undefined)`);
}
// Assinatura HMAC com cada hash: vetores fixos (chave 01020304, mensagem 05 06), nome em qualquer caixa e dicionário.
for (const h of ["SHA-1", "SHA-224", "SHA-256", "SHA-384", "SHA-512", "SHA3-256", "SHA3-384", "SHA3-512", "sha-256", "Sha3-256"]) {
  WITH(KEY(h), `crypto.subtle.sign('HMAC', k, new Uint8Array([5, 6])).then(function (s) { return [s.constructor.name, ${HEX("s")}, k.algorithm] })`);
  WITH(KEY(h), `crypto.subtle.exportKey('jwk', k).then(function (j) { return [j, Object.keys(j)] })`);
}
WITH(KEY("SHA-256"), `crypto.subtle.sign({name: 'hmac'}, k, new Uint8Array([5, 6])).then(function (s) { return ${HEX("s")} })`);
WITH(KEY("SHA-256", "['sign']", "true", "[107, 101, 121]"), `crypto.subtle.sign('HMAC', k, new TextEncoder().encode('The quick brown fox jumps over the lazy dog')).then(function (s) { return ${HEX("s")} })`);
WITH(KEY("SHA-256"), `crypto.subtle.sign('HMAC', k, new ArrayBuffer(2)).then(function (s) { return ${HEX("s")} })`);
WITH(KEY("SHA-256"), `crypto.subtle.sign('HMAC', k, new Uint8Array(0)).then(function (s) { return ${HEX("s")} })`);
WITH(KEY("SHA-256", "['sign']", "true", `new Array(200).fill(7)`), `crypto.subtle.sign('HMAC', k, new Uint8Array([1])).then(function (s) { return ${HEX("s")} })`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('HMAC', k, new Uint8Array([5, 6])).then(function (s) { return crypto.subtle.verify('HMAC', k, s, new Uint8Array([5, 6])) })`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('HMAC', k, new Uint8Array([5, 6])).then(function (s) { return crypto.subtle.verify('HMAC', k, s, new Uint8Array([5, 7])) })`);
WITH(KEY("SHA-1"), `crypto.subtle.verify('HMAC', k, new Uint8Array([1]), new Uint8Array([5, 6]))`);
WITH(KEY("SHA-1"), `crypto.subtle.verify('HMAC', k, new ArrayBuffer(0), new Uint8Array([5]))`);
WITH(KEY("SHA-1"), `crypto.subtle.verify({name: 'hmac'}, k, new Uint8Array([1]), new Uint8Array([2]))`);
// Erros de sign/verify na ordem do bun.
WITH(KEY("SHA-1"), `crypto.subtle.sign('RSASSA-PKCS1-v1_5', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('Ed25519', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('foo', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('SHA-256', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('AES-GCM', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('rsaes-pkcs1-v1_5', k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign({}, k, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('HMAC', k, 5)`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('foo', k, 5)`);
WITH(KEY("SHA-1"), `crypto.subtle.sign('HMAC', k, 'abc')`);
WITH(KEY("SHA-1"), `crypto.subtle.verify('HMAC', k, 5, new Uint8Array(1))`);
WITH(KEY("SHA-1"), `crypto.subtle.verify('HMAC', k, new Uint8Array(1), 5)`);
WITH(KEY("SHA-1"), `crypto.subtle.verify('foo', k, new Uint8Array(1), new Uint8Array(1))`);
WITH(KEY("SHA-1", "['verify']"), `crypto.subtle.sign('HMAC', k, new Uint8Array(1))`);
WITH(KEY("SHA-1", "['sign']"), `crypto.subtle.verify('HMAC', k, new Uint8Array(1), new Uint8Array(1))`);
WITH(KEY("SHA-1", "['sign']", "false"), `crypto.subtle.sign('HMAC', k, new Uint8Array(1)).then(function (s) { return s.byteLength })`);
aexpr(`crypto.subtle.sign('HMAC', {}, new Uint8Array(1))`);
aexpr(`crypto.subtle.sign('HMAC', null, new Uint8Array(1))`);
aexpr(`crypto.subtle.verify('HMAC', crypto.subtle, new Uint8Array(1), new Uint8Array(1))`);
// exportKey.
for (const f of ["raw", "raw-secret", "jwk", "raw-public", "raw-seed", "spki", "pkcs8"]) {
  WITH(KEY("SHA-256"), `crypto.subtle.exportKey('${f}', k).then(function (r) { return r instanceof ArrayBuffer ? [r.constructor.name, ${HEX("r")}] : r })`);
  WITH(KEY("SHA-256", "['sign']", "false"), `crypto.subtle.exportKey('${f}', k)`);
}
WITH(KEY("SHA-256", "['verify', 'sign']"), `crypto.subtle.exportKey('jwk', k).then(function (j) { return [j.key_ops, k.usages] })`);
WITH(KEY("SHA-256", "['sign']", "true", "[251, 255, 254, 62, 63]"), `crypto.subtle.exportKey('jwk', k).then(function (j) { return j.k })`);
// importKey de HMAC: raw.
const IMPORT = (format, data, algorithm, extractable, usages) => aexpr(`crypto.subtle.importKey('${format}', ${data}, ${algorithm}, ${extractable}, ${usages}).then(function (k) { return [k.type, k.extractable, k.algorithm, k.usages] })`);
const U8 = "new Uint8Array([1, 2, 3, 4])";
const H256 = "{name: 'HMAC', hash: 'SHA-256'}";
IMPORT("raw", U8, "{name: 'HMAC'}", "true", "['sign']");
IMPORT("raw", U8, "'HMAC'", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'foo'}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'MD5'}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'HMAC'}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 5}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: {}}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: null}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: {name: 'sha-512'}}", "true", "['sign']");
IMPORT("raw", "new Uint8Array(0)", H256, "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: 32}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: 8}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: 0}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: 12}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: '32'}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: 32.9}", "true", "['sign']");
IMPORT("raw", U8, "{name: 'HMAC', hash: 'SHA-256', length: undefined}", "true", "['sign']");
IMPORT("raw", U8, H256, "true", "['encrypt']");
IMPORT("raw", U8, H256, "true", "['sign', 'deriveBits']");
IMPORT("raw", U8, H256, "true", "['wrapKey']");
IMPORT("raw", U8, H256, "true", "['encapsulateKey']");
IMPORT("raw", U8, H256, "true", "[]");
IMPORT("raw", U8, H256, "true", "['verify', 'sign', 'verify']");
IMPORT("raw", U8, H256, "0", "['sign']");
IMPORT("raw", U8, H256, "'yes'", "['sign']");
IMPORT("raw", U8, H256, "undefined", "['sign']");
IMPORT("raw", "new Uint8Array([1, 2, 3, 4]).buffer", H256, "true", "['sign']");
IMPORT("raw", "new DataView(new ArrayBuffer(3))", H256, "true", "['sign']");
IMPORT("raw", "new Uint8Array([9, 1, 2, 3, 9]).subarray(1, 4)", H256, "true", "['sign']");
IMPORT("raw-secret", U8, H256, "true", "['sign']");
for (const f of ["raw-public", "raw-seed", "spki", "pkcs8"]) {
  IMPORT(f, U8, H256, "true", "['sign']");
  IMPORT(f, U8, H256, "true", "['encrypt']");
  IMPORT(f, U8, H256, "true", "[]");
}
// importKey de HMAC: jwk.
const JWK = (jwk, usages = "['sign', 'verify']", extractable = "true", algorithm = H256) => IMPORT("jwk", jwk, algorithm, extractable, usages);
JWK("{kty: 'oct', k: 'AQIDBA'}");
JWK("{kty: 'oct', k: 'AQIDBA', alg: 'HS256', ext: true, use: 'sig', key_ops: ['sign', 'verify']}");
JWK("{kty: 'oct', k: 'AQIDBA==', ext: 1}");
JWK("{kty: 'oct', k: 'AQIDBA', ext: 0}", "['sign']", "false");
JWK("{kty: 'oct', k: 'AQIDBA', ext: false}", "['sign']", "false");
JWK("{kty: 'oct', k: 'AQIDBA', ext: false}");
JWK("{kty: 'oct', k: 'AQIDBA', alg: 'HS256'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA-256', length: 32}");
JWK("{kty: 'oct', k: 'AQIDBA'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA-256', length: 24}");
JWK("{kty: 'oct', k: 'AQIDBA', alg: 'HS1'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA-1'}");
JWK("{kty: 'oct', k: 'AQIDBA', alg: 'HS512'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA-512'}");
JWK("{kty: 'oct', k: 'AQIDBA', alg: 'HS256'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA3-256'}");
JWK("{kty: 'oct', k: 'AQIDBA'}", "['sign']", "true", "{name: 'HMAC', hash: 'SHA3-256'}");
for (const j of [
  "{}", "{kty: 'RSA', k: 'AQIDBA'}", "{kty: 'oct'}", "{k: 'AQIDBA'}", "{kty: 'oct', k: 'AQIDBA', use: 'enc'}", "{kty: 'oct', k: 'AQIDBA', use: 'sig'}",
  "{kty: 'oct', k: 'AQIDBA', key_ops: ['sign', 'sign']}", "{kty: 'oct', k: 'AQIDBA', key_ops: ['sign']}", "{kty: 'oct', k: 'AQIDBA', key_ops: ['verify', 'sign', 'encrypt']}",
  "{kty: 'oct', k: 'AQIDBA', key_ops: ['zzz']}", "{kty: 'oct', k: 'AQIDBA', key_ops: [1]}", "{kty: 'oct', k: 'AQIDBA', key_ops: 'sign'}", "{kty: 'oct', k: 'AQIDBA', key_ops: {}}",
  "{kty: 'oct', k: 'AQIDBA', key_ops: []}", "{kty: 'oct', k: 'AQIDBA', alg: 'HS512'}", "{kty: 'oct', k: 'AQIDBA', alg: 'hs256'}", "{kty: 'oct', k: ''}", "{kty: 'oct', k: '!!!!'}",
  "{kty: 'oct', k: 5}", "{kty: 'oct', k: null}", "{kty: null, k: 'AQIDBA'}", "{kty: 'oct', k: 'AQIDBA', ext: false}", "{kty: 'oct', k: 'AQID+A'}", "{kty: 'oct', k: 'AQID-A_w'}",
  "{kty: 'oct', k: Symbol.iterator}", "new Uint8Array(1)", "new ArrayBuffer(1)", "[]", "5", "'x'", "null", "undefined",
]) JWK(j);
JWK("{kty: 'oct', k: 'AQIDBA'}", "['encrypt']");
JWK("{kty: 'oct', k: 'AQIDBA'}", "[]");
JWK("{kty: 'oct', k: 'AQIDBA', use: 'enc'}", "[]");
JWK("{kty: 'oct', k: 'AQIDBA', key_ops: ['sign']}", "[]");
// A ordem entre erros: o jwk inválido vence o algoritmo; o algoritmo vence o formato.
aexpr(`crypto.subtle.importKey('jwk', {kty: 'oct', k: 'AQIDBA', key_ops: ['zzz']}, 'foo', true, [])`);
aexpr(`crypto.subtle.importKey('jwk', {kty: 'oct', k: 'AQIDBA'}, 'foo', true, ['x'])`);
aexpr(`crypto.subtle.importKey('jwk', new Uint8Array(1), 'foo', true, [])`);
aexpr(`crypto.subtle.importKey('jwk', new Uint8Array(1), {name: 'HMAC'}, true, [])`);
aexpr(`crypto.subtle.importKey('jwk', new Uint8Array(1), ${H256}, true, ['encrypt'])`);
aexpr(`crypto.subtle.importKey('spki', new Uint8Array(1), {name: 'HMAC'}, true, [])`);
aexpr(`crypto.subtle.importKey('raw', new Uint8Array(1), {name: 'HMAC'}, true, ['x'])`);
// generateKey de HMAC: só a forma (a chave é aleatória).
const GEN = (algorithm, extractable, usages) => aexpr(`crypto.subtle.generateKey(${algorithm}, ${extractable}, ${usages}).then(function (k) { return crypto.subtle.exportKey('raw', k).then(function (r) { return [k.type, k.extractable, k.algorithm, k.usages, r.byteLength] }, function (e) { return [k.type, k.extractable, k.algorithm, k.usages, E(e)] }) })`);
for (const h of ["SHA-1", "SHA-224", "SHA-256", "SHA-384", "SHA-512", "SHA3-256", "SHA3-384", "SHA3-512"]) GEN(`{name: 'HMAC', hash: '${h}'}`, "true", "['sign', 'verify']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 24}", "false", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 24}", "true", "['verify']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 8}", "true", "['verify', 'sign']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 0}", "true", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 12}", "true", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: 1000}", "true", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256', length: '16'}", "1", "['sign']");
GEN("{name: 'hmac', hash: {name: 'sha-1'}}", "0", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256'}", "true", "[]");
GEN("{name: 'HMAC', hash: 'SHA-256'}", "true", "['encrypt']");
GEN("{name: 'HMAC', hash: 'SHA-256'}", "true", "['sign', 'wrapKey']");
GEN("'HMAC'", "true", "['sign']");
GEN("{name: 'HMAC'}", "true", "['sign']");
GEN("{name: 'HMAC', hash: 'foo'}", "true", "['sign']");
GEN("{name: 'HMAC', hash: 'SHA-256'}", "true", "5");
GEN("{name: 'HMAC', hash: 'SHA-256'}", "true", "['x']");
aexpr(`crypto.subtle.generateKey({name: 'HMAC', hash: 'SHA-256'}, true, ['sign']).then(function (a) { return crypto.subtle.generateKey({name: 'HMAC', hash: 'SHA-256'}, true, ['sign']).then(function (b) { return a === b })})`);
// Chaves de outro tipo não são CryptoKey; as de HMAC passam pelos demais métodos só até a LACUNA, então nenhum caso entra.

// AES-CBC, AES-CTR, AES-GCM e AES-KW: generateKey, importKey, exportKey, encrypt, decrypt, wrapKey e unwrapKey. Chaves e vetores fixos
// (a saída é medida no bun, nunca calculada aqui); generateKey entra só pela forma.
const AES = ["AES-CBC", "AES-CTR", "AES-GCM", "AES-KW"];
const AES_USAGES = (n) => (n === "AES-KW" ? "['wrapKey', 'unwrapKey']" : "['encrypt', 'decrypt']");
const AGEN = (algorithm, extractable, usages) => aexpr(`crypto.subtle.generateKey(${algorithm}, ${extractable}, ${usages}).then(function (k) { return crypto.subtle.exportKey('raw', k).then(function (r) { return [k.type, k.extractable, k.algorithm, k.usages, r.byteLength] }, function (e) { return [k.type, k.algorithm, k.usages, E(e)] }) })`);
const B16 = "new Uint8Array(16).fill(1)";
const AIMPORT = (format, data, algorithm, extractable, usages) => aexpr(`crypto.subtle.importKey('${format}', ${data}, ${algorithm}, ${extractable}, ${usages}).then(function (k) { return crypto.subtle.exportKey('jwk', k).then(function (j) { return [k.type, k.extractable, k.algorithm, k.usages, j] }, function (e) { return [k.type, k.algorithm, k.usages, E(e)] }) })`);
const AKEY = (n, usages = AES_USAGES(n), extractable = "true", bytes = B16) => `crypto.subtle.importKey('raw', ${bytes}, '${n}', ${extractable}, ${usages})`;
for (const n of AES) {
  const u = AES_USAGES(n);
  for (const len of [128, 192, 256, 100, 0, 1.5, 300, 65536, "'128'", "'x'", "-1", "null", "undefined"]) AGEN(`{name: '${n}', length: ${len}}`, "true", u);
  AGEN(`{name: '${n}'}`, "true", u);
  AGEN(`'${n}'`, "true", u);
  AGEN(`{name: '${n.toLowerCase()}', length: 128}`, "false", u);
  for (const us of ["[]", "['sign']", "['encrypt']", "['decrypt']", "['wrapKey']", "['unwrapKey']", "['deriveKey']", "['encrypt', 'wrapKey', 'unwrapKey', 'decrypt']", "['encrypt', 'encrypt']", "5", "['x']"]) AGEN(`{name: '${n}', length: 128}`, "true", us);
  for (const bytes of [16, 24, 32, 0, 5, 15, 17, 33]) AIMPORT("raw", `new Uint8Array(${bytes}).fill(3)`, `'${n}'`, "true", u);
  AIMPORT("raw-secret", B16, `'${n}'`, "true", u);
  AIMPORT("raw", `${B16}.buffer`, `{name: '${n}'}`, "0", u);
  for (const f of ["raw-public", "raw-seed", "spki", "pkcs8"]) { AIMPORT(f, B16, `'${n}'`, "true", u); AIMPORT(f, B16, `'${n}'`, "true", "[]"); }
  for (const us of ["[]", "['sign']", "['encrypt']", "['wrapKey']", "['verify', 'sign']"]) AIMPORT("raw", B16, `'${n}'`, "true", us);
  AIMPORT("raw", "{}", `'${n}'`, "true", u);
  AIMPORT("raw", "'abc'", `'${n}'`, "true", u);
  const alg = { "AES-CBC": "CBC", "AES-CTR": "CTR", "AES-GCM": "GCM", "AES-KW": "KW" }[n];
  const K = "AAAAAAAAAAAAAAAAAAAAAA";
  for (const j of [
    `{kty: 'oct', k: '${K}'}`, `{kty: 'oct', k: '${K}', alg: 'A128${alg}', ext: true, key_ops: ['wrapKey', 'unwrapKey', 'encrypt', 'decrypt'], use: 'enc'}`,
    `{kty: 'oct', k: '${K}', alg: 'A256${alg}'}`, `{kty: 'oct', k: '${K}', alg: 'A128XYZ'}`, `{kty: 'oct', k: '${K}', alg: 'a128${alg.toLowerCase()}'}`,
    `{kty: 'oct', k: '${K}AAAAAAAA', alg: 'A192${alg}'}`, `{kty: 'oct', k: 'AAAA'}`, `{kty: 'oct', k: '!!!'}`, `{kty: 'oct', k: ''}`, `{kty: 'oct', k: 5}`, `{kty: 'oct'}`, `{k: '${K}'}`, `{}`,
    `{kty: 'RSA', k: '${K}'}`, `{kty: 'oct', k: '${K}', use: 'sig'}`, `{kty: 'oct', k: '${K}', use: 'enc'}`, `{kty: 'oct', k: '${K}', ext: false}`, `{kty: 'oct', k: '${K}', ext: 0}`,
    `{kty: 'oct', k: '${K}', key_ops: ['encrypt', 'encrypt']}`, `{kty: 'oct', k: '${K}', key_ops: ['encrypt']}`, `{kty: 'oct', k: '${K}', key_ops: ['sign']}`, `{kty: 'oct', k: '${K}', key_ops: []}`,
    `{kty: 'oct', k: '${K}', key_ops: ['zzz']}`, `{kty: 'oct', k: '${K}', key_ops: 'encrypt'}`, "new Uint8Array(1)", "5", "null", "undefined", "'x'",
  ]) { AIMPORT("jwk", j, `'${n}'`, "true", u); AIMPORT("jwk", j, `'${n}'`, "false", "['wrapKey']"); AIMPORT("jwk", j, `'${n}'`, "true", "[]"); }
  AIMPORT("jwk", `{kty: 'oct', k: '${K}', ext: false}`, `'${n}'`, "false", u);
  // exportKey.
  for (const f of ["raw", "raw-secret", "jwk", "raw-public", "raw-seed", "spki", "pkcs8"]) {
    WITH(AKEY(n), `crypto.subtle.exportKey('${f}', k).then(function (r) { return r instanceof ArrayBuffer ? [r.constructor.name, ${HEX("r")}] : r })`);
    WITH(AKEY(n, u, "false"), `crypto.subtle.exportKey('${f}', k)`);
  }
  WITH(AKEY(n, u, "true", "[251, 255, 254, 62, 63, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]"), `crypto.subtle.exportKey('jwk', k).then(function (j) { return j.k })`);
  WITH(AKEY(n), `[k.type, k.extractable, k.algorithm, k.usages, k.algorithm === k.algorithm, k.usages === k.usages]`);
}
// encrypt e decrypt.
const PT = "new TextEncoder().encode('hello world 12345!!')";
const ENC = (key, algorithm, data = PT) => WITH(key, `crypto.subtle.encrypt(${algorithm}, k, ${data}).then(function (r) { return [r.byteLength, ${HEX("r")}] })`);
const ROUND = (key, algorithm, data = PT) => WITH(key, `crypto.subtle.encrypt(${algorithm}, k, ${data}).then(function (r) { return crypto.subtle.decrypt(${algorithm}, k, r).then(function (p) { return ${HEX("p")} }) })`);
const IV16 = "new Uint8Array(16).fill(2)";
const IV12 = "new Uint8Array(12).fill(4)";
const CBC = (extra = "") => `{name: 'AES-CBC', iv: ${IV16}${extra}}`;
for (const bytes of [16, 24, 32]) {
  const key = AKEY("AES-CBC", undefined, "true", `new Uint8Array(${bytes}).fill(5)`);
  ENC(key, CBC());
  ROUND(key, CBC());
  ENC(AKEY("AES-CTR", undefined, "true", `new Uint8Array(${bytes}).fill(5)`), `{name: 'AES-CTR', counter: ${IV16}, length: 64}`);
  ENC(AKEY("AES-GCM", undefined, "true", `new Uint8Array(${bytes}).fill(5)`), `{name: 'AES-GCM', iv: ${IV12}}`);
}
for (const len of [0, 1, 15, 16, 17, 31, 32, 33]) {
  const data = `new Uint8Array(${len}).fill(9)`;
  ENC(AKEY("AES-CBC"), CBC(), data);
  ROUND(AKEY("AES-CBC"), CBC(), data);
  ENC(AKEY("AES-CTR"), `{name: 'AES-CTR', counter: ${IV16}, length: 128}`, data);
  ENC(AKEY("AES-GCM"), `{name: 'AES-GCM', iv: ${IV12}}`, data);
  ROUND(AKEY("AES-GCM"), `{name: 'AES-GCM', iv: ${IV12}}`, data);
}
ENC(AKEY("AES-CBC"), CBC(), "new Uint8Array(5).buffer");
ENC(AKEY("AES-CBC"), CBC(), "new DataView(new ArrayBuffer(3))");
ENC(AKEY("AES-CBC"), `{name: 'aes-cbc', iv: ${IV16}.buffer}`);
for (const iv of ["new Uint8Array(0)", "new Uint8Array(8)", "new Uint8Array(15)", "new Uint8Array(17)", "'x'", "5", "null", "{}"]) { ENC(AKEY("AES-CBC"), `{name: 'AES-CBC', iv: ${iv}}`); }
ENC(AKEY("AES-CBC"), "{name: 'AES-CBC'}");
ENC(AKEY("AES-CBC"), "'AES-CBC'");
ENC(AKEY("AES-CBC"), "{}");
for (const bad of ["'x'", "5", "null", "undefined", "{}", "[]"]) ENC(AKEY("AES-CBC"), CBC(), bad);
for (const data of ["new Uint8Array(0)", "new Uint8Array(5)", "new Uint8Array(16)", "new Uint8Array(32).fill(7)", "new Uint8Array(15)"]) {
  WITH(AKEY("AES-CBC"), `crypto.subtle.decrypt(${CBC()}, k, ${data}).then(function (r) { return ${HEX("r")} })`);
}
WITH(AKEY("AES-CBC", "['encrypt']"), `crypto.subtle.decrypt(${CBC()}, k, new Uint8Array(16))`);
WITH(AKEY("AES-CBC", "['decrypt']"), `crypto.subtle.encrypt(${CBC()}, k, new Uint8Array(16))`);
WITH(AKEY("AES-CBC", "['wrapKey']"), `crypto.subtle.encrypt(${CBC()}, k, new Uint8Array(16))`);
WITH(AKEY("AES-CBC"), `crypto.subtle.encrypt({name: 'AES-CTR', counter: ${IV16}, length: 64}, k, new Uint8Array(16))`);
WITH(AKEY("AES-CBC"), `crypto.subtle.encrypt({name: 'AES-GCM', iv: ${IV12}}, k, new Uint8Array(16))`);
WITH(AKEY("AES-KW"), `crypto.subtle.encrypt({name: 'AES-KW'}, k, new Uint8Array(16))`);
WITH(AKEY("AES-KW"), `crypto.subtle.encrypt(${CBC()}, k, new Uint8Array(16))`);
WITH(KEY("SHA-256"), `crypto.subtle.encrypt(${CBC()}, k, new Uint8Array(16))`);
WITH(KEY("SHA-256"), `crypto.subtle.decrypt(${CBC()}, k, new Uint8Array(16))`);
for (const a of ["'foo'", "{name: 'HMAC'}", "'SHA-256'", "'AES-KW'", "'ECDSA'", "{}"]) { WITH(AKEY("AES-CBC"), `crypto.subtle.encrypt(${a}, k, new Uint8Array(16))`); WITH(AKEY("AES-CBC"), `crypto.subtle.decrypt(${a}, k, new Uint8Array(16))`); }
aexpr(`crypto.subtle.encrypt(${CBC()}, {}, new Uint8Array(16))`);
aexpr(`crypto.subtle.decrypt(${CBC()}, null, new Uint8Array(16))`);
// AES-CTR.
const CTR = (counter, length) => `{name: 'AES-CTR', counter: ${counter}, length: ${length}}`;
for (const length of [1, 2, 8, 32, 64, 127, 128]) { ENC(AKEY("AES-CTR"), CTR(IV16, length), "new Uint8Array(48).fill(1)"); ROUND(AKEY("AES-CTR"), CTR(IV16, length), "new Uint8Array(48).fill(1)"); }
for (const length of [0, 129, 255, 256, 300, -1, 1.5, "'x'", "'64'", "null", "undefined", "NaN", "Infinity"]) ENC(AKEY("AES-CTR"), CTR(IV16, length));
for (const counter of ["new Uint8Array(0)", "new Uint8Array(8)", "new Uint8Array(15)", "new Uint8Array(17)", "'x'", "null", "undefined", "5"]) ENC(AKEY("AES-CTR"), CTR(counter, 64));
for (const [length, bytes] of [[8, 16 * 256], [8, 16 * 257], [1, 32], [1, 33], [1, 48], [2, 64], [2, 65], [3, 128], [3, 129]]) {
  ENC(AKEY("AES-CTR"), CTR("new Uint8Array(16).fill(255)", length), `new Uint8Array(${bytes}).fill(1)`);
  ENC(AKEY("AES-CTR"), CTR(IV16, length), `new Uint8Array(${bytes}).fill(1)`);
}
WITH(AKEY("AES-CTR"), `crypto.subtle.encrypt({name: 'AES-CTR', length: 64}, k, new Uint8Array(1))`);
WITH(AKEY("AES-CTR"), `crypto.subtle.encrypt({name: 'AES-CTR', counter: ${IV16}}, k, new Uint8Array(1))`);
// AES-GCM.
const GCM = (iv, extra = "") => `{name: 'AES-GCM', iv: ${iv}${extra}}`;
for (const tag of [0, 1, 31, 32, 33, 64, 95, 96, 104, 112, 120, 128, 129, 200, 255, 256, 300, -1, 1.5, "'x'", "'96'", "null", "undefined"]) {
  ENC(AKEY("AES-GCM"), GCM(IV12, `, tagLength: ${tag}`));
  ROUND(AKEY("AES-GCM"), GCM(IV12, `, tagLength: ${tag}`));
}
for (const iv of [0, 1, 8, 11, 12, 13, 16, 100]) { ENC(AKEY("AES-GCM"), GCM(`new Uint8Array(${iv}).fill(6)`)); ROUND(AKEY("AES-GCM"), GCM(`new Uint8Array(${iv}).fill(6)`)); }
ENC(AKEY("AES-GCM"), GCM(IV12, ", additionalData: new Uint8Array(3).fill(1)"));
ROUND(AKEY("AES-GCM"), GCM(IV12, ", additionalData: new Uint8Array(40).fill(1), tagLength: 96"));
ENC(AKEY("AES-GCM"), GCM(IV12, ", additionalData: 'x'"));
ENC(AKEY("AES-GCM"), GCM(IV12, ", additionalData: new Uint8Array(0).buffer"));
ENC(AKEY("AES-GCM"), GCM("'x'"));
ENC(AKEY("AES-GCM"), "{name: 'AES-GCM'}");
ENC(AKEY("AES-GCM"), "'AES-GCM'");
// Vetores NIST SP 800-38D (caso 2: chave e IV zerados, texto zerado) e adulteração.
ENC(AKEY("AES-GCM", undefined, "true", "new Uint8Array(16)"), GCM("new Uint8Array(12)"), "new Uint8Array(16)");
ENC(AKEY("AES-GCM", undefined, "true", "new Uint8Array(16)"), GCM("new Uint8Array(12)"), "new Uint8Array(0)");
for (const data of ["new Uint8Array(0)", "new Uint8Array(5)", "new Uint8Array(15)", "new Uint8Array(16)", "new Uint8Array(20)"]) {
  WITH(AKEY("AES-GCM"), `crypto.subtle.decrypt(${GCM(IV12)}, k, ${data}).then(function (r) { return ${HEX("r")} })`);
  WITH(AKEY("AES-GCM"), `crypto.subtle.decrypt(${GCM(IV12, ", tagLength: 32")}, k, ${data}).then(function (r) { return ${HEX("r")} })`);
}
WITH(AKEY("AES-GCM"), `crypto.subtle.encrypt(${GCM(IV12)}, k, ${PT}).then(function (r) { var b = new Uint8Array(r); b[0] ^= 1; return crypto.subtle.decrypt(${GCM(IV12)}, k, b) })`);
WITH(AKEY("AES-GCM"), `crypto.subtle.encrypt(${GCM(IV12)}, k, ${PT}).then(function (r) { var b = new Uint8Array(r); b[b.length - 1] ^= 1; return crypto.subtle.decrypt(${GCM(IV12)}, k, b) })`);
WITH(AKEY("AES-GCM"), `crypto.subtle.encrypt(${GCM(IV12)}, k, ${PT}).then(function (r) { return crypto.subtle.decrypt(${GCM(IV12, ", additionalData: new Uint8Array(1)")}, k, r) })`);
WITH(AKEY("AES-GCM"), `crypto.subtle.encrypt(${GCM(IV12)}, k, ${PT}).then(function (r) { return crypto.subtle.decrypt(${GCM(IV16)}, k, r) })`);
// AES-KW: wrapKey e unwrapKey.
const KWP = "{name: 'AES-KW'}";
const KWK = (bytes = B16) => AKEY("AES-KW", undefined, "true", bytes);
const WRAP = (format, inner, wrapping, algorithm) => aexpr(`${wrapping}.then(function (w) { return ${inner}.then(function (k) { return crypto.subtle.wrapKey('${format}', k, w, ${algorithm}).then(function (r) { return [r.byteLength, ${HEX("r")}] }) }) })`);
for (const n of AES) for (const format of ["raw", "raw-secret", "jwk", "spki", "pkcs8", "raw-public", "raw-seed", "xx"]) {
  WRAP(format, AKEY(n), KWK(), KWP);
  WRAP(format, AKEY(n, undefined, "false"), KWK(), KWP);
}
for (const bytes of [16, 24, 32]) for (const ops of ["['encrypt']", "['encrypt', 'decrypt']", "['wrapKey']"]) {
  WRAP("jwk", AKEY("AES-CBC", ops, "true", `new Uint8Array(${bytes}).fill(8)`), KWK(), KWP);
  WRAP("raw", AKEY("AES-CBC", ops, "true", `new Uint8Array(${bytes}).fill(8)`), KWK(`new Uint8Array(${bytes}).fill(9)`), KWP);
}
WRAP("jwk", "crypto.subtle.importKey('raw', new Uint8Array(32), {name: 'HMAC', hash: 'SHA-256'}, true, ['sign'])", KWK(), KWP);
for (const bytes of [1, 5, 8, 16, 20, 24]) WRAP("raw", `crypto.subtle.importKey('raw', new Uint8Array(${bytes}).fill(1), {name: 'HMAC', hash: 'SHA-256'}, true, ['sign'])`, KWK(), KWP);
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-CBC", "['wrapKey', 'unwrapKey']"), CBC());
WRAP("jwk", AKEY("AES-CBC"), AKEY("AES-CBC", "['wrapKey', 'unwrapKey']"), CBC());
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-CTR", "['wrapKey', 'unwrapKey']"), CTR(IV16, 64));
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-GCM", "['wrapKey', 'unwrapKey']"), GCM(IV12));
WRAP("jwk", AKEY("AES-CBC"), AKEY("AES-GCM", "['wrapKey', 'unwrapKey']"), GCM(IV12, ", tagLength: 96"));
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-CBC"), CBC());
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-KW", "['unwrapKey']"), KWP);
WRAP("raw", AKEY("AES-CBC"), KWK(), CBC());
WRAP("raw", AKEY("AES-CBC"), AKEY("AES-CBC"), KWP);
WRAP("raw", AKEY("AES-CBC"), KWK(), "'AES-CBC'");
WRAP("raw", AKEY("AES-CBC"), KWK(), "{name: 'HMAC'}");
WRAP("raw", AKEY("AES-CBC"), KWK(), "'foo'");
WRAP("raw", AKEY("AES-CBC"), KWK(), "'AES-KW'");
WRAP("raw", AKEY("AES-CBC"), KWK(), "{name: 'aes-kw'}");
WRAP("raw", KEY("SHA-256"), KWK(), KWP);
WRAP("raw", AKEY("AES-CBC"), KEY("SHA-256"), KWP);
aexpr(`crypto.subtle.wrapKey('raw', {}, {}, ${KWP})`);
aexpr(`${KWK()}.then(function (w) { return crypto.subtle.wrapKey('raw', {}, w, ${KWP}) })`);
aexpr(`${AKEY("AES-CBC")}.then(function (k) { return crypto.subtle.wrapKey('raw', k, {}, ${KWP}) })`);
// unwrapKey: ida e volta, e as validações.
const UNWRAP = (format, inner, wrapping, algorithm, target, extractable = "true", usages = "['encrypt', 'decrypt']", wrapped = null) =>
  aexpr(`${wrapping}.then(function (w) { return ${inner}.then(function (k) { return crypto.subtle.wrapKey('${format}', k, w, ${algorithm}) }).then(function (data) { return crypto.subtle.unwrapKey('${format}', ${wrapped || "data"}, w, ${algorithm}, ${target}, ${extractable}, ${usages}) }).then(function (u) { return crypto.subtle.exportKey('jwk', u).then(function (j) { return [u.type, u.extractable, u.algorithm, u.usages, j] }, function (e) { return [u.type, u.algorithm, u.usages, E(e)] }) }) })`);
for (const format of ["raw", "raw-secret", "jwk"]) {
  for (const target of ["'AES-CBC'", "'AES-CTR'", "'AES-GCM'", "{name: 'AES-KW'}", "'aes-cbc'", "{name: 'HMAC', hash: 'SHA-256'}", "{name: 'HMAC', hash: 'SHA-256', length: 128}", "{name: 'HMAC', hash: 'SHA-256', length: 64}", "'foo'", "'SHA-256'", "{name: 'HMAC'}"]) {
    UNWRAP(format, AKEY("AES-CBC"), KWK(), KWP, target);
    UNWRAP(format, AKEY("AES-CBC"), KWK(), KWP, target, "false", "['sign']");
  }
  UNWRAP(format, AKEY("AES-CBC", "['encrypt']"), KWK(), KWP, "'AES-CBC'", "true", "['encrypt']");
  UNWRAP(format, AKEY("AES-CBC"), KWK(), KWP, "'AES-CBC'", "true", "[]");
  UNWRAP(format, AKEY("AES-CBC"), KWK(), KWP, "'AES-CBC'", "true", "['sign']");
  UNWRAP(format, AKEY("AES-CBC"), KWK(), KWP, "'AES-CBC'", "true", "5");
  UNWRAP(format, AKEY("AES-CBC", undefined, "true", "new Uint8Array(32).fill(8)"), AKEY("AES-GCM", "['wrapKey', 'unwrapKey']"), GCM(IV12), "'AES-CTR'");
  UNWRAP(format, AKEY("AES-CBC"), AKEY("AES-CBC", "['wrapKey', 'unwrapKey']"), CBC(), "'AES-GCM'");
  UNWRAP(format, AKEY("AES-CBC"), AKEY("AES-CTR", "['wrapKey', 'unwrapKey']"), CTR(IV16, 64), "'AES-CBC'");
}
for (const data of ["new Uint8Array(0)", "new Uint8Array(5)", "new Uint8Array(8)", "new Uint8Array(16)", "new Uint8Array(24)", "new Uint8Array(24).fill(1)", "'x'", "5", "{}"]) {
  aexpr(`${KWK()}.then(function (w) { return crypto.subtle.unwrapKey('raw', ${data}, w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
  aexpr(`${KWK()}.then(function (w) { return crypto.subtle.unwrapKey('jwk', ${data}, w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
}
aexpr(`${AKEY("AES-CBC", "['wrapKey', 'unwrapKey']")}.then(function (w) { return crypto.subtle.encrypt(${CBC()}, w, new TextEncoder().encode('not json')) })`);
aexpr(`${AKEY("AES-CBC", "['wrapKey', 'unwrapKey', 'encrypt']")}.then(function (w) { return crypto.subtle.encrypt(${CBC()}, w, new TextEncoder().encode('not json')).then(function (d) { return crypto.subtle.unwrapKey('jwk', d, w, ${CBC()}, 'AES-CBC', true, ['encrypt']) }) })`);
aexpr(`${AKEY("AES-CBC", "['wrapKey', 'unwrapKey', 'encrypt']")}.then(function (w) { return crypto.subtle.encrypt(${CBC()}, w, new TextEncoder().encode('[1]')).then(function (d) { return crypto.subtle.unwrapKey('jwk', d, w, ${CBC()}, 'AES-CBC', true, ['encrypt']) }) })`);
aexpr(`${AKEY("AES-CBC", "['wrapKey', 'unwrapKey', 'encrypt']")}.then(function (w) { return crypto.subtle.encrypt(${CBC()}, w, new TextEncoder().encode('{"kty":"oct","k":"AAAAAAAAAAAAAAAAAAAAAA"}')).then(function (d) { return crypto.subtle.unwrapKey('jwk', d, w, ${CBC()}, 'AES-CBC', true, ['encrypt']) }).then(function (u) { return [u.algorithm, u.usages] }) })`);
aexpr(`${KWK()}.then(function (w) { return crypto.subtle.unwrapKey('raw', new Uint8Array(24), w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
aexpr(`${AKEY("AES-KW", "['wrapKey']")}.then(function (w) { return crypto.subtle.unwrapKey('raw', new Uint8Array(24), w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
aexpr(`${AKEY("AES-CBC")}.then(function (w) { return crypto.subtle.unwrapKey('raw', new Uint8Array(24), w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
aexpr(`${KWK()}.then(function (w) { return crypto.subtle.unwrapKey('xx', new Uint8Array(24), w, ${KWP}, 'AES-CBC', true, ['encrypt']) })`);
aexpr(`crypto.subtle.unwrapKey('raw', new Uint8Array(24), {}, ${KWP}, 'AES-CBC', true, ['encrypt'])`);
aexpr(`${KWK()}.then(function (w) { return crypto.subtle.unwrapKey('raw', new Uint8Array(24), w, 'foo', 'AES-CBC', true, ['encrypt']) })`);
// Curvas elípticas: ECDSA, ECDH (P-256, P-384, P-521), Ed25519 e X25519. As assinaturas ECDSA são aleatórias no bun: entram só
// o tamanho e o resultado de `verify`, nunca os bytes. As chaves geradas entram por forma; os vetores fixos (RFC 8032 e RFC 7748)
// e o JWK fixo de P-256 pelo valor.
const EC = (name, curve, usages = "['sign', 'verify']", extractable = "true") =>
  `crypto.subtle.generateKey({name: '${name}', namedCurve: '${curve}'}, ${extractable}, ${usages})`;
const OKP = (name, usages = "['sign', 'verify']", extractable = "true") => `crypto.subtle.generateKey('${name}', ${extractable}, ${usages})`;
const SHAPE = `function (k) { return [Object.keys(k), k.publicKey.type, k.publicKey.extractable, k.publicKey.usages, k.publicKey.algorithm, k.privateKey.type, k.privateKey.extractable, k.privateKey.usages, k.privateKey.algorithm] }`;
const EC_HELPERS = "var X = function (h) { var s = ''; for (var i = 0; i < h.length; i += 2) s += String.fromCharCode(parseInt(h.substr(i, 2), 16)); return btoa(s).replace(/\\+/g, '-').replace(/\\//g, '_').replace(/=+$/, '') }; var U = function (h) { var a = new Uint8Array(h.length / 2); for (var i = 0; i < a.length; i++) a[i] = parseInt(h.substr(i * 2, 2), 16); return a }; var T = function (b) { return Array.from(new Uint8Array(b), function (x) { return ('0' + x.toString(16)).slice(-2) }).join('') };\n";
const hexExpr = (code) => programs.push(HELPER + EC_HELPERS + `try { Promise.resolve(${code}).then(function (v) { R = S(v) }, function (e) { R = 'rejeitou ' + E(e) }) } catch (e) { R = E(e) }`);
for (const [name, usages] of [["ECDSA", "['sign', 'verify']"], ["ECDSA", "['sign']"], ["ECDSA", "['verify']"], ["ECDH", "['deriveBits', 'deriveKey']"], ["ECDH", "['deriveBits']"], ["ECDH", "['deriveKey']"]]) {
  for (const curve of ["P-256", "P-384", "P-521"]) aexpr(`${EC(name, curve, usages)}.then(${SHAPE})`);
}
aexpr(`${EC("ECDSA", "P-256", "['sign', 'verify']", "false")}.then(${SHAPE})`);
for (const name of ["Ed25519", "X25519"]) {
  for (const usages of ["['sign', 'verify']", "['sign']", "['verify']", "['deriveBits']", "['deriveKey', 'deriveBits']", "[]"]) aexpr(`${OKP(name, usages)}.then(${SHAPE})`);
}
// generateKey: erros.
aexpr(EC("ECDSA", "P-256", "['encrypt']"));
aexpr(EC("ECDSA", "P-256", "['deriveBits']"));
aexpr(EC("ECDH", "P-256", "['sign']"));
aexpr(EC("ECDSA", "P-256", "[]"));
aexpr(EC("ECDH", "P-256", "[]"));
aexpr(EC("ECDSA", "P-1"));
aexpr(EC("ECDSA", "p-256"));
aexpr(`crypto.subtle.generateKey({name: 'ECDSA'}, true, ['sign'])`);
aexpr(`crypto.subtle.generateKey('ECDSA', true, ['sign'])`);
aexpr(`crypto.subtle.generateKey({name: 'ECDH', namedCurve: 5}, true, ['deriveBits'])`);
aexpr(OKP("Ed25519", "['encrypt']"));
aexpr(OKP("X25519", "['sign']"));
// exportKey: formatos de cada tipo e extraível.
for (const gen of [EC("ECDSA", "P-256"), EC("ECDSA", "P-384"), EC("ECDSA", "P-521"), EC("ECDH", "P-256", "['deriveBits']"), OKP("Ed25519"), OKP("X25519", "['deriveBits']")]) {
  for (const part of ["publicKey", "privateKey"]) {
    for (const format of ["raw", "raw-public", "raw-secret", "raw-seed", "spki", "pkcs8", "jwk"]) {
      aexpr(`${gen}.then(function (k) { return crypto.subtle.exportKey('${format}', k.${part}).then(function (d) { return d instanceof ArrayBuffer ? ['ArrayBuffer', d.byteLength] : [Object.keys(d), d.kty, d.crv, d.alg, d.key_ops, d.ext, typeof d.d, typeof d.x, typeof d.y] }) })`);
    }
  }
}
for (const part of ["publicKey", "privateKey"]) {
  for (const format of ["raw", "spki", "pkcs8", "jwk"]) aexpr(`${EC("ECDSA", "P-256", "['sign', 'verify']", "false")}.then(function (k) { return crypto.subtle.exportKey('${format}', k.${part}).then(function (d) { return d instanceof ArrayBuffer ? d.byteLength : Object.keys(d) }) })`);
}
// Ida e volta de importKey/exportKey (bloco próprio: `ROUND` já existe para o AES acima).
{
const ROUND = (gen, algorithm, format, part, usages, extractable = "true") =>
  aexpr(`${gen}.then(function (g) { return crypto.subtle.exportKey('${format}', g.${part}).then(function (data) { return crypto.subtle.importKey('${format}', data, ${algorithm}, ${extractable}, ${usages}) }).then(function (k) { return crypto.subtle.exportKey('${format}', k).then(function (again) { return crypto.subtle.exportKey('${format}', g.${part}).then(function (first) { return [k.type, k.extractable, k.algorithm, k.usages, JSON.stringify(again instanceof ArrayBuffer ? Array.from(new Uint8Array(again)) : again) === JSON.stringify(first instanceof ArrayBuffer ? Array.from(new Uint8Array(first)) : first)] }) }) }) })`);
for (const curve of ["P-256", "P-384", "P-521"]) {
  const algorithm = `{name: 'ECDSA', namedCurve: '${curve}'}`;
  ROUND(EC("ECDSA", curve), algorithm, "raw", "publicKey", "['verify']");
  ROUND(EC("ECDSA", curve), algorithm, "spki", "publicKey", "['verify']");
  ROUND(EC("ECDSA", curve), algorithm, "pkcs8", "privateKey", "['sign']");
  ROUND(EC("ECDSA", curve), algorithm, "jwk", "publicKey", "['verify']");
  ROUND(EC("ECDSA", curve), algorithm, "jwk", "privateKey", "['sign']");
  ROUND(EC("ECDSA", curve), algorithm, "raw", "publicKey", "['verify']", "false");
  ROUND(EC("ECDH", curve, "['deriveBits']"), `{name: 'ECDH', namedCurve: '${curve}'}`, "raw", "publicKey", "[]");
  ROUND(EC("ECDH", curve, "['deriveBits']"), `{name: 'ECDH', namedCurve: '${curve}'}`, "pkcs8", "privateKey", "['deriveBits']");
  ROUND(EC("ECDH", curve, "['deriveBits']"), `{name: 'ECDH', namedCurve: '${curve}'}`, "jwk", "privateKey", "['deriveKey', 'deriveBits']");
}
for (const name of ["Ed25519", "X25519"]) {
  const usages = name === "Ed25519" ? ["['verify']", "['sign']"] : ["[]", "['deriveBits']"];
  const gen = name === "Ed25519" ? OKP(name) : OKP(name, "['deriveBits']");
  ROUND(gen, `'${name}'`, "raw", "publicKey", usages[0]);
  ROUND(gen, `'${name}'`, "spki", "publicKey", usages[0]);
  ROUND(gen, `'${name}'`, "pkcs8", "privateKey", usages[1]);
  ROUND(gen, `'${name}'`, "jwk", "publicKey", usages[0]);
  ROUND(gen, `'${name}'`, "jwk", "privateKey", usages[1]);
}
}
// importKey: erros.
const P256_X = "jrKxwpWuwBXLQP0zk5R7HhC4ihwMDC6WGIJlyRERkzc";
const P256_Y = "YAlA1Pqm5YaIixppiuTab2slB1rC-oWnydQwjJNrgDc";
const P256_D = "HojZY4OHftQVOsYHUMk-lN336d1I9ak23pkWKRuCWPg";
const JWKP = (extra = "") => `{kty: 'EC', crv: 'P-256', x: '${P256_X}', y: '${P256_Y}'${extra}}`;
const JWKS = (extra = "") => `{kty: 'EC', crv: 'P-256', x: '${P256_X}', y: '${P256_Y}', d: '${P256_D}'${extra}}`;
const IMP = (format, data, algorithm, usages = "['verify']", extractable = "true") =>
  aexpr(`crypto.subtle.importKey('${format}', ${data}, ${algorithm}, ${extractable}, ${usages}).then(function (k) { return crypto.subtle.exportKey('jwk', k).then(function (j) { return [k.type, k.algorithm, k.usages, k.extractable, j] }, function (e) { return [k.type, k.algorithm, k.usages, k.extractable, E(e)] }) })`);
const ECP = "{name: 'ECDSA', namedCurve: 'P-256'}";
IMP("jwk", JWKP(), ECP);
IMP("jwk", JWKS(), ECP, "['sign']");
IMP("jwk", JWKP(", alg: 'ES256'"), ECP);
IMP("jwk", JWKP(", alg: 'ES384'"), ECP);
IMP("jwk", JWKP(", use: 'sig'"), ECP);
IMP("jwk", JWKP(", use: 'enc'"), ECP);
IMP("jwk", JWKP(", ext: false"), ECP);
IMP("jwk", JWKP(", ext: false"), ECP, "['verify']", "false");
IMP("jwk", JWKP(", key_ops: ['sign']"), ECP);
IMP("jwk", JWKP(", key_ops: ['verify']"), ECP);
IMP("jwk", JWKP(", key_ops: ['verify', 'verify']"), ECP);
IMP("jwk", `{kty: 'oct', crv: 'P-256', x: '${P256_X}', y: '${P256_Y}'}`, ECP);
IMP("jwk", `{crv: 'P-256', x: '${P256_X}', y: '${P256_Y}'}`, ECP);
IMP("jwk", `{kty: 'EC', crv: 'P-384', x: '${P256_X}', y: '${P256_Y}'}`, ECP);
IMP("jwk", `{kty: 'EC', x: '${P256_X}', y: '${P256_Y}'}`, ECP);
IMP("jwk", `{kty: 'EC', crv: 'P-256', y: '${P256_Y}'}`, ECP);
IMP("jwk", `{kty: 'EC', crv: 'P-256', x: '${P256_X}'}`, ECP);
IMP("jwk", `{kty: 'EC', crv: 'P-256', x: '${P256_X}', y: '${P256_X}'}`, ECP);
IMP("jwk", JWKS(", x: 'AAAA'"), ECP, "['sign']");
IMP("jwk", JWKS(), ECP, "['verify']");
IMP("jwk", JWKS(), ECP, "[]");
IMP("jwk", JWKP(), ECP, "['sign']");
IMP("jwk", JWKP(), "{name: 'ECDH', namedCurve: 'P-256'}", "[]");
IMP("jwk", JWKP(", alg: 'ES384'"), "{name: 'ECDH', namedCurve: 'P-256'}", "[]");
IMP("jwk", JWKP(), "{name: 'ECDH', namedCurve: 'P-256'}", "['deriveBits']");
IMP("jwk", JWKS(), "{name: 'ECDH', namedCurve: 'P-256'}", "['deriveBits']");
IMP("jwk", JWKS(), "{name: 'ECDH', namedCurve: 'P-256'}", "[]");
IMP("jwk", "new Uint8Array(4)", ECP);
IMP("jwk", "5", ECP);
IMP("raw", "new Uint8Array(65)", ECP);
IMP("raw", "new Uint8Array(0)", ECP);
IMP("raw", "5", ECP);
IMP("raw", "new Uint8Array(32)", "{name: 'ECDSA'}");
IMP("raw", "new Uint8Array(65)", "{name: 'ECDSA', namedCurve: 'P-1'}");
IMP("raw", "new Uint8Array(65)", "'ECDSA'");
IMP("spki", "new Uint8Array(10)", ECP);
IMP("spki", "new Uint8Array(0)", ECP);
IMP("pkcs8", "new Uint8Array(10)", ECP, "['sign']");
IMP("pkcs8", "new Uint8Array(0)", ECP, "['sign']");
IMP("raw-secret", "new Uint8Array(65)", ECP);
IMP("raw-seed", "new Uint8Array(32)", ECP);
for (const name of ["Ed25519", "X25519"]) {
  const pub = name === "Ed25519" ? "['verify']" : "[]";
  IMP("raw", "new Uint8Array(32)", `'${name}'`, pub);
  IMP("raw", "new Uint8Array(31)", `'${name}'`, pub);
  IMP("raw", "new Uint8Array(33)", `'${name}'`, pub);
  IMP("raw", "new Uint8Array(32)", `'${name}'`, name === "Ed25519" ? "['sign']" : "['deriveBits']");
  IMP("jwk", `{kty: 'OKP', crv: '${name}', x: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'}`, `'${name}'`, pub);
  IMP("jwk", `{kty: 'EC', crv: '${name}', x: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'}`, `'${name}'`, pub);
  IMP("jwk", `{kty: 'OKP', crv: 'Ed448', x: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'}`, `'${name}'`, pub);
  IMP("jwk", `{kty: 'OKP', crv: '${name}', x: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', alg: 'EdDSA2'}`, `'${name}'`, pub);
  IMP("jwk", `{kty: 'OKP', crv: '${name}'}`, `'${name}'`, pub);
  IMP("spki", "new Uint8Array(10)", `'${name}'`, pub);
  IMP("pkcs8", "new Uint8Array(10)", `'${name}'`, name === "Ed25519" ? "['sign']" : "['deriveBits']");
}
IMP("pkcs8", "new Uint8Array([48,46,2,1,0,48,5,6,3,43,101,112,4,34,4,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0])", "'Ed25519'", "['sign']");
IMP("pkcs8", "new Uint8Array([48,46,2,1,0,48,5,6,3,43,101,112,4,34,4,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0])", "'X25519'", "['deriveBits']");
IMP("pkcs8", "new Uint8Array([48,46,2,1,0,48,5,6,3,43,101,110,4,34,4,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0])", "'Ed25519'", "['sign']");
// Troca de curva em pkcs8 e spki.
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.exportKey('pkcs8', g.privateKey) }).then(function (d) { return crypto.subtle.importKey('pkcs8', d, {name: 'ECDSA', namedCurve: 'P-384'}, true, ['sign']) })`);
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.exportKey('spki', g.publicKey) }).then(function (d) { return crypto.subtle.importKey('spki', d, {name: 'ECDSA', namedCurve: 'P-521'}, true, ['verify']) })`);
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.exportKey('pkcs8', g.privateKey) }).then(function (d) { return crypto.subtle.importKey('pkcs8', d, {name: 'ECDSA', namedCurve: 'P-256'}, true, []) })`);
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.exportKey('pkcs8', g.privateKey) }).then(function (d) { return crypto.subtle.importKey('pkcs8', d, {name: 'ECDSA', namedCurve: 'P-256'}, true, ['verify']) })`);
// sign/verify de ECDSA: ida e volta (a assinatura é aleatória).
const SIGN = (curve, hash, body) =>
  aexpr(`${EC("ECDSA", curve)}.then(function (g) { var a = {name: 'ECDSA', hash: ${hash}}; return crypto.subtle.sign(a, g.privateKey, new Uint8Array([1, 2, 3])).then(function (s) { return ${body} }) })`);
for (const curve of ["P-256", "P-384", "P-521"]) {
  for (const hash of ["'SHA-1'", "'SHA-256'", "'SHA-384'", "'SHA-512'", "'SHA3-256'", "{name: 'SHA-256'}", "'sha-256'"]) {
    SIGN(curve, hash, `crypto.subtle.verify(a, g.publicKey, s, new Uint8Array([1, 2, 3])).then(function (ok) { return [s instanceof ArrayBuffer, s.byteLength, ok] })`);
  }
  SIGN(curve, "'SHA-256'", `crypto.subtle.verify(a, g.publicKey, s, new Uint8Array([1, 2, 4]))`);
  SIGN(curve, "'SHA-256'", `crypto.subtle.verify({name: 'ECDSA', hash: 'SHA-384'}, g.publicKey, s, new Uint8Array([1, 2, 3]))`);
  SIGN(curve, "'SHA-256'", `crypto.subtle.verify(a, g.publicKey, new Uint8Array(3), new Uint8Array([1, 2, 3]))`);
  SIGN(curve, "'SHA-256'", `crypto.subtle.verify(a, g.publicKey, new Uint8Array(s.byteLength), new Uint8Array([1, 2, 3]))`);
  SIGN(curve, "'SHA-256'", `crypto.subtle.verify(a, g.publicKey, new Uint8Array(0), new Uint8Array([1, 2, 3]))`);
  SIGN(curve, "'SHA-256'", `crypto.subtle.sign(a, g.privateKey, new Uint8Array([1, 2, 3])).then(function (t) { return [Array.from(new Uint8Array(s)).join() === Array.from(new Uint8Array(t)).join()] })`);
}
SIGN("P-256", "'SHA-256'", `crypto.subtle.verify(a, g.privateKey, s, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign(a, g.publicKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign({name: 'ECDSA'}, g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign({name: 'ECDSA', hash: 'MD5'}, g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign({name: 'ECDSA', hash: {}}, g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign('ECDSA', g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign('Ed25519', g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign('HMAC', g.privateKey, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, 5, new Uint8Array(1))`);
SIGN("P-256", "'SHA-256'", `crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, g.privateKey, 5)`);
aexpr(`${EC("ECDH", "P-256", "['deriveBits']")}.then(function (g) { return crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, g.privateKey, new Uint8Array(1)) })`);
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, g.privateKey, new Uint8Array(1)) })`);
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.sign('HMAC', g.privateKey, new Uint8Array(1)) })`);
// Ed25519: ida e volta e o vetor 1 da RFC 8032 (mensagem vazia).
for (const data of ["new Uint8Array(0)", "new Uint8Array([1, 2, 3])", "new Uint8Array(1000)"]) {
  aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.sign('Ed25519', g.privateKey, ${data}).then(function (s) { return crypto.subtle.sign('Ed25519', g.privateKey, ${data}).then(function (t) { return crypto.subtle.verify('Ed25519', g.publicKey, s, ${data}).then(function (ok) { return crypto.subtle.verify('Ed25519', g.publicKey, s, new Uint8Array([9])).then(function (bad) { return [s.byteLength, ok, bad, Array.from(new Uint8Array(s)).join() === Array.from(new Uint8Array(t)).join()] }) }) }) }) })`);
}
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.verify('Ed25519', g.publicKey, new Uint8Array(63), new Uint8Array(1)) })`);
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.verify('Ed25519', g.publicKey, new Uint8Array(64), new Uint8Array(1)) })`);
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.sign('Ed25519', g.publicKey, new Uint8Array(1)) })`);
aexpr(`${OKP("Ed25519")}.then(function (g) { return crypto.subtle.verify('Ed25519', g.privateKey, new Uint8Array(64), new Uint8Array(1)) })`);
hexExpr(`crypto.subtle.importKey('pkcs8', U('302e020100300506032b657004220420' + '9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60'), 'Ed25519', true, ['sign']).then(function (k) { return crypto.subtle.sign('Ed25519', k, new Uint8Array(0)) }).then(T)`);
hexExpr(`crypto.subtle.importKey('raw', U('d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'), 'Ed25519', true, ['verify']).then(function (k) { return crypto.subtle.verify('Ed25519', k, U('e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b'), new Uint8Array(0)) })`);
hexExpr(`crypto.subtle.importKey('raw', U('d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'), 'Ed25519', true, ['verify']).then(function (k) { return crypto.subtle.verify('Ed25519', k, U('e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100c'), new Uint8Array(0)) })`);
hexExpr(`crypto.subtle.importKey('jwk', {kty: 'OKP', crv: 'Ed25519', d: X('9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60'), x: X('d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a')}, 'Ed25519', true, ['sign']).then(function (k) { return crypto.subtle.sign('Ed25519', k, new Uint8Array(0)) }).then(T)`);
hexExpr(`crypto.subtle.importKey('jwk', {kty: 'OKP', crv: 'Ed25519', d: X('9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60'), x: X('d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511b')}, 'Ed25519', true, ['sign'])`);
// ECDSA com o JWK fixo de P-256.
aexpr(`crypto.subtle.importKey('jwk', ${JWKS()}, ${ECP}, true, ['sign']).then(function (k) { return crypto.subtle.sign({name: 'ECDSA', hash: 'SHA-256'}, k, new Uint8Array([1, 2, 3])).then(function (s) { return crypto.subtle.importKey('jwk', ${JWKP()}, ${ECP}, true, ['verify']).then(function (p) { return crypto.subtle.verify({name: 'ECDSA', hash: 'SHA-256'}, p, s, new Uint8Array([1, 2, 3])) }) }) })`);
// ECDH e X25519: segredos iguais nos dois lados, tamanhos e erros.
for (const curve of ["P-256", "P-384", "P-521"]) {
  for (const length of ["undefined", "null", "0", "1", "7", "8", "9", "128", "255", "256", "264", "300", "384", "512", "521", "528", "536", "1024", "-1", "NaN", "'16'"]) {
    aexpr(`Promise.all([${EC("ECDH", curve, "['deriveBits']")}, ${EC("ECDH", curve, "['deriveBits']")}]).then(function (p) { var T = function (b) { return Array.from(new Uint8Array(b)).join() }; return crypto.subtle.deriveBits({name: 'ECDH', public: p[1].publicKey}, p[0].privateKey, ${length}).then(function (x) { return crypto.subtle.deriveBits({name: 'ECDH', public: p[0].publicKey}, p[1].privateKey, ${length}).then(function (y) { return [x.byteLength, T(x) === T(y)] }) }) })`);
  }
}
for (const length of ["undefined", "null", "0", "8", "128", "256", "264", "512"]) {
  aexpr(`Promise.all([${OKP("X25519", "['deriveBits']")}, ${OKP("X25519", "['deriveBits']")}]).then(function (p) { var T = function (b) { return Array.from(new Uint8Array(b)).join() }; return crypto.subtle.deriveBits({name: 'X25519', public: p[1].publicKey}, p[0].privateKey, ${length}).then(function (x) { return crypto.subtle.deriveBits({name: 'X25519', public: p[0].publicKey}, p[1].privateKey, ${length}).then(function (y) { return [x.byteLength, T(x) === T(y)] }) }) })`);
}
hexExpr(`crypto.subtle.importKey('jwk', {kty: 'OKP', crv: 'X25519', d: X('77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a'), x: X('8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a')}, 'X25519', true, ['deriveBits']).then(function (a) { return crypto.subtle.importKey('raw', U('de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f'), 'X25519', true, []).then(function (b) { return crypto.subtle.deriveBits({name: 'X25519', public: b}, a, 256) }) }).then(T)`);
hexExpr(`crypto.subtle.importKey('pkcs8', U('302e020100300506032b656e04220420' + '77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a'), 'X25519', true, ['deriveBits']).then(function (a) { return crypto.subtle.importKey('spki', U('302a300506032b656e032100' + 'de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f'), 'X25519', true, []).then(function (b) { return crypto.subtle.deriveBits({name: 'X25519', public: b}, a, 128) }) }).then(T)`);
hexExpr(`crypto.subtle.importKey('jwk', {kty: 'EC', crv: 'P-256', d: '${P256_D}', x: '${P256_X}', y: '${P256_Y}'}, {name: 'ECDH', namedCurve: 'P-256'}, true, ['deriveBits']).then(function (a) { return crypto.subtle.deriveBits({name: 'ECDH', public: a}, a, 256) })`);
hexExpr(`crypto.subtle.importKey('jwk', {kty: 'EC', crv: 'P-256', d: '${P256_D}', x: '${P256_X}', y: '${P256_Y}'}, {name: 'ECDH', namedCurve: 'P-256'}, true, ['deriveBits']).then(function (a) { return crypto.subtle.importKey('jwk', {kty: 'EC', crv: 'P-256', x: '${P256_X}', y: '${P256_Y}'}, {name: 'ECDH', namedCurve: 'P-256'}, true, []).then(function (b) { return crypto.subtle.deriveBits({name: 'ECDH', public: b}, a, 256) }) }).then(T)`);
// deriveBits/deriveKey: validações, na ordem do bun.
const PAIR = (name, curve) => (name === "X25519" ? OKP("X25519", "['deriveBits', 'deriveKey']") : EC("ECDH", curve, "['deriveBits', 'deriveKey']"));
const DERIVED = (algorithm, code, name = "ECDH", curve = "P-256") =>
  aexpr(`Promise.all([${PAIR(name, curve)}, ${PAIR(name, curve)}, ${EC("ECDSA", "P-256")}, ${EC("ECDH", "P-384", "['deriveBits']")}, ${OKP("X25519", "['deriveBits']")}, ${EC("ECDH", "P-256", "['deriveBits']")}, ${EC("ECDH", "P-256", "['deriveKey']")}]).then(function (p) { var a = p[0], b = p[1], ecdsa = p[2], p384 = p[3], x = p[4], bits = p[5], dkey = p[6]; var alg = {name: '${algorithm}', public: b.publicKey}; return ${code} })`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH'}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: {}}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: 5}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: a.privateKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: ecdsa.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: x.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDH', public: p384.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, a.publicKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, ecdsa.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, bits.publicKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, dkey.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, x.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits(alg, {}, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'ECDSA', public: b.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'Ed25519', public: b.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'AES-GCM', public: b.publicKey}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits({name: 'SHA-256'}, a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits('ECDH', a.privateKey, 256)`);
DERIVED("ECDH", `crypto.subtle.deriveBits('foo', a.privateKey, 256)`);
DERIVED("X25519", `crypto.subtle.deriveBits({name: 'X25519'}, a.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits({name: 'X25519', public: {}}, a.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits({name: 'X25519', public: a.privateKey}, a.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits({name: 'X25519', public: ecdsa.publicKey}, a.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits({name: 'ECDH', public: b.publicKey}, a.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits(alg, bits.privateKey, 256)`, "X25519");
DERIVED("X25519", `crypto.subtle.deriveBits(alg, dkey.privateKey, 256)`, "X25519");
// deriveKey.
const DK = (derived, usages = "['encrypt', 'decrypt']", extractable = "true", algorithm = "ECDH", name = "ECDH") =>
  DERIVED(algorithm, `crypto.subtle.deriveKey(alg, a.privateKey, ${derived}, ${extractable}, ${usages}).then(function (k) { return crypto.subtle.exportKey('raw', k).then(function (r) { return [k.type, k.extractable, k.algorithm, k.usages, r.byteLength] }, function (e) { return [k.type, k.extractable, k.algorithm, k.usages, E(e)] }) })`, name);
for (const name of ["ECDH", "X25519"]) {
  const run = (derived, usages, extractable = "true") => DK(derived, usages, extractable, name, name);
  run("{name: 'AES-GCM', length: 256}");
  run("{name: 'AES-GCM', length: 128}");
  run("{name: 'AES-GCM', length: 192}");
  run("{name: 'AES-CBC', length: 256}", "['encrypt']");
  run("{name: 'AES-CTR', length: 128}", "['decrypt']", "false");
  run("{name: 'AES-KW', length: 256}", "['wrapKey', 'unwrapKey']");
  run("{name: 'AES-GCM', length: 512}");
  run("{name: 'AES-GCM', length: 100}");
  run("{name: 'AES-GCM', length: 0}");
  run("{name: 'AES-GCM'}");
  run("'AES-GCM'");
  run("{name: 'HMAC', hash: 'SHA-256'}", "['sign']");
  run("{name: 'HMAC', hash: 'SHA-256', length: 128}", "['sign', 'verify']");
  run("{name: 'HMAC', hash: 'SHA-256', length: 256}", "['sign']");
  run("{name: 'HMAC', hash: 'SHA-256', length: 100}", "['sign']");
  run("{name: 'HMAC', hash: 'SHA-256', length: 0}", "['sign']");
  run("{name: 'HMAC', hash: 'SHA-512', length: 128}", "['sign']");
  run("{name: 'HMAC'}", "['sign']");
  run("{name: 'AES-GCM', length: 256}", "[]");
  run("{name: 'AES-GCM', length: 256}", "['sign']");
  run("{name: 'ECDSA', namedCurve: 'P-256'}", "['sign']");
  run("{name: 'Ed25519'}", "['sign']");
  run("{name: 'ECDH', namedCurve: 'P-256'}", "[]");
  run("'foo'", "['encrypt']");
  run("{name: 'SHA-256'}", "['encrypt']");
}
DERIVED("ECDH", `crypto.subtle.deriveKey(alg, a.publicKey, {name: 'AES-GCM', length: 256}, true, ['encrypt'])`);
DERIVED("ECDH", `crypto.subtle.deriveKey(alg, bits.privateKey, {name: 'AES-GCM', length: 256}, true, ['encrypt'])`);
DERIVED("ECDH", `crypto.subtle.deriveKey(alg, dkey.privateKey, {name: 'AES-GCM', length: 256}, true, ['encrypt']).then(function (k) { return k.algorithm })`);
DERIVED("ECDH", `crypto.subtle.deriveKey(alg, a.privateKey, {name: 'AES-GCM', length: 256}, true, 5)`);
DERIVED("ECDH", `crypto.subtle.deriveKey({name: 'ECDH'}, a.privateKey, {name: 'AES-GCM', length: 256}, true, ['encrypt'])`);
DERIVED("ECDH", `crypto.subtle.deriveKey(alg, {}, {name: 'AES-GCM', length: 256}, true, ['encrypt'])`);
// Uma chave de curva elíptica não faz o que as outras fazem.
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.encrypt({name: 'AES-GCM', iv: new Uint8Array(12)}, g.publicKey, new Uint8Array(1)) })`);
aexpr(`${EC("ECDSA", "P-256")}.then(function (g) { return crypto.subtle.decrypt({name: 'AES-GCM', iv: new Uint8Array(12)}, g.privateKey, new Uint8Array(32)) })`);
for (const format of ["raw", "spki", "pkcs8", "jwk"]) {
  aexpr(`Promise.all([${EC("ECDSA", "P-256")}, ${AKEY("AES-GCM", "['wrapKey', 'unwrapKey']")}]).then(function (p) { return crypto.subtle.wrapKey('${format}', p[0].${format === "pkcs8" ? "privateKey" : "publicKey"}, p[1], {name: 'AES-GCM', iv: new Uint8Array(12)}).then(function (w) { return crypto.subtle.unwrapKey('${format}', w, p[1], {name: 'AES-GCM', iv: new Uint8Array(12)}, ${ECP}, true, ['${format === "pkcs8" ? "sign" : "verify"}']) }).then(function (k) { return [k.type, k.algorithm, k.usages] }) })`);
}
// Uma chave AES não assina nem verifica.
WITH(AKEY("AES-CBC"), `crypto.subtle.sign('HMAC', k, new Uint8Array(1))`);
WITH(AKEY("AES-KW"), `crypto.subtle.verify('HMAC', k, new Uint8Array(1), new Uint8Array(1))`);
// getPublicKey, raw-secret/raw-seed em curva elíptica e a ordem entre usos e dados (medidos no bun 1.4.2).
for (const [alg, curve, usage] of [["ECDSA", "P-256", "verify"], ["ECDH", "P-384", "deriveBits"], ["Ed25519", "", "verify"], ["X25519", "", "deriveBits"]]) {
  const sign = alg === "ECDSA" || alg === "Ed25519" ? "['sign', 'verify']" : "['deriveBits']";
  const dict = curve ? `{name: '${alg}', namedCurve: '${curve}'}` : `'${alg}'`;
  const generated = `crypto.subtle.generateKey(${dict}, true, ${sign})`;
  const priv = (body) => aexpr(`${generated}.then(function (g) { return ${body} })`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, ${usage === "verify" ? "['verify']" : "[]"}).then(function (k) { return [k.type, k.extractable, k.algorithm, k.usages] })`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, []).then(function (k) { return [k.type, k.extractable, k.usages] })`);
  priv(`Promise.all([crypto.subtle.getPublicKey(g.privateKey, []), crypto.subtle.exportKey('spki', g.publicKey)]).then(function (p) { return crypto.subtle.exportKey('spki', p[0]).then(function (s) { return [s.byteLength, Array.from(new Uint8Array(s)).join() === Array.from(new Uint8Array(p[1])).join()] }) })`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, ['deriveBits', 'sign'])`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, ['bogus'])`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, 5)`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, null)`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, new Set([]))`);
  priv(`crypto.subtle.getPublicKey(g.publicKey, [])`);
  priv(`crypto.subtle.getPublicKey(g.privateKey)`);
  priv(`crypto.subtle.getPublicKey(g.privateKey, []).then(function (k) { return crypto.subtle.getPublicKey(k, []) })`);
  priv(`crypto.subtle.exportKey('raw-secret', g.privateKey)`);
  priv(`crypto.subtle.exportKey('raw-secret', g.publicKey)`);
  priv(`crypto.subtle.exportKey('raw-seed', g.privateKey)`);
  priv(`crypto.subtle.exportKey('raw-seed', g.publicKey)`);
  priv(`crypto.subtle.exportKey('raw-public', g.privateKey)`);
  aexpr(`crypto.subtle.generateKey(${dict}, false, ${sign}).then(function (g) { return crypto.subtle.exportKey('raw-secret', g.privateKey) })`);
  aexpr(`crypto.subtle.generateKey(${dict}, false, ${sign}).then(function (g) { return crypto.subtle.exportKey('raw-seed', g.privateKey) })`);
  aexpr(`crypto.subtle.importKey('raw-seed', new Uint8Array(32), ${dict}, true, ${sign})`);
  aexpr(`crypto.subtle.importKey('raw-seed', new Uint8Array(3), ${dict}, true, [])`);
  aexpr(`crypto.subtle.importKey('raw-secret', new Uint8Array(32), ${dict}, true, ${usage === "verify" ? "['verify']" : "[]"})`);
  aexpr(`crypto.subtle.importKey('raw-secret', new Uint8Array(3), ${dict}, true, ['${usage === "verify" ? "sign" : "verify"}'])`);
  priv(`crypto.subtle.exportKey('raw', g.publicKey).then(function (r) { return crypto.subtle.importKey('raw-secret', r, ${dict}, true, []) }).then(function (k) { return [k.type, k.usages] })`);
  // Os usos vêm antes dos dados, pelo tipo que o formato produz.
  aexpr(`crypto.subtle.importKey('spki', new Uint8Array(3), ${dict}, true, ['${usage === "verify" ? "sign" : "verify"}'])`);
  aexpr(`crypto.subtle.importKey('spki', new Uint8Array(3), ${dict}, true, ${usage === "verify" ? "['verify']" : "['deriveBits']"})`);
  aexpr(`crypto.subtle.importKey('spki', new Uint8Array(3), ${dict}, true, [])`);
  aexpr(`crypto.subtle.importKey('pkcs8', new Uint8Array(3), ${dict}, true, ['verify'])`);
  aexpr(`crypto.subtle.importKey('pkcs8', new Uint8Array(3), ${dict}, true, ${sign})`);
  aexpr(`crypto.subtle.importKey('pkcs8', new Uint8Array(3), ${dict}, true, [])`);
  aexpr(`crypto.subtle.importKey('raw', new Uint8Array(3), ${dict}, true, ['${usage === "verify" ? "sign" : "verify"}'])`);
  priv(`crypto.subtle.exportKey('jwk', g.privateKey).then(function (j) { return crypto.subtle.importKey('jwk', Object.assign({}, j, {d: 'AA'}), ${dict}, true, ['${usage === "verify" ? "verify" : "bogus"}']) })`);
  priv(`crypto.subtle.exportKey('jwk', g.privateKey).then(function (j) { return crypto.subtle.importKey('jwk', Object.assign({}, j, {d: 'AA'}), ${dict}, true, ${usage === "verify" ? "['sign']" : "['deriveBits']"}) })`);
  priv(`crypto.subtle.exportKey('jwk', g.privateKey).then(function (j) { return crypto.subtle.importKey('jwk', Object.assign({}, j, {d: 'AA'}), ${dict}, true, []) })`);
}
aexpr(`crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']).then(function (g) { return crypto.subtle.exportKey('jwk', g.privateKey).then(function (j) { return crypto.subtle.importKey('jwk', Object.assign({}, j, {alg: 'EdDSA'}), 'Ed25519', true, ['sign']) }) }).then(function (k) { return [k.type, k.algorithm, k.usages] })`);
aexpr(`crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']).then(function (g) { return crypto.subtle.exportKey('jwk', g.privateKey) }).then(function (j) { return j.alg })`);
aexpr(`crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']).then(function (g) { return crypto.subtle.exportKey('jwk', g.privateKey).then(function (j) { return crypto.subtle.importKey('jwk', Object.assign({}, j, {alg: 'ES256'}), 'Ed25519', true, ['sign']) }) })`);
// deriveBits com comprimento que não é múltiplo de 8: o último byte perde os bits de baixo.
for (const bits of [0, 1, 3, 7, 8, 9, 12, 20, 255, 256, 257, 264, -8]) {
  aexpr(`crypto.subtle.generateKey({name: 'ECDH', namedCurve: 'P-256'}, false, ['deriveBits']).then(function (a) { return Promise.all([crypto.subtle.deriveBits({name: 'ECDH', public: a.publicKey}, a.privateKey, 256), crypto.subtle.deriveBits({name: 'ECDH', public: a.publicKey}, a.privateKey, ${bits})]) }).then(function (p) { var f = new Uint8Array(p[0]), s = new Uint8Array(p[1]), n = s.length; return [n, n === 0 ? true : (s[n - 1] === (f[n - 1] & ((0xff << (8 - (${bits} % 8 || 8))) & 0xff)))] })`);
}

// HKDF e PBKDF2: importKey raw, deriveBits e deriveKey (HMAC e AES).
const KDFIMP = (n, extra = "") => `crypto.subtle.importKey('raw', ${U8}, '${n}', false, ['deriveBits', 'deriveKey']${extra})`;
for (const n of ["HKDF", "PBKDF2"]) {
  aexpr(`${KDFIMP(n)}.then(function (k) { return [k.type, k.extractable, k.algorithm, k.usages] })`);
  aexpr(`crypto.subtle.importKey('raw', ${U8}, '${n}', true, ['deriveBits'])`);
  aexpr(`crypto.subtle.importKey('raw', ${U8}, '${n}', true, [])`);
  for (const usages of ["['sign']", "[]", "['deriveBits', 'encrypt']"]) aexpr(`crypto.subtle.importKey('raw', ${U8}, '${n}', false, ${usages})`);
  for (const format of ["raw-secret", "raw-public", "raw-seed", "spki", "pkcs8"]) aexpr(`crypto.subtle.importKey('${format}', ${U8}, '${n}', false, ['deriveBits']).then(function (k) { return k.type })`);
  aexpr(`crypto.subtle.importKey('jwk', {kty: 'oct', k: 'AQ'}, '${n}', false, ['deriveBits'])`);
  aexpr(`crypto.subtle.importKey('jwk', ${U8}, '${n}', false, ['deriveBits'])`);
  aexpr(`crypto.subtle.importKey('raw', new Uint8Array(0), '${n}', false, ['deriveBits']).then(function (k) { return k.type })`);
  aexpr(`crypto.subtle.generateKey('${n}', false, ['deriveBits'])`);
  WITH(KDFIMP(n), `crypto.subtle.exportKey('raw', k)`);
  WITH(KDFIMP(n), `crypto.subtle.sign('${n}', k, new Uint8Array(1))`);
}
const KH = (extra = "") => `{name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array([9, 8]), info: new Uint8Array([7])${extra}}`;
const KP = (extra = "") => `{name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array([9, 8]), iterations: 10${extra}}`;
const DBITS = (algorithm, name, length, key = KDFIMP(name)) => WITH(key, `crypto.subtle.deriveBits(${algorithm}, k, ${length}).then(function (b) { return [b.byteLength, ${HEX("b")}] })`);
for (const length of ["256", "8", "0", "12", "null", "undefined", "NaN", "'64'", "255 * 32 * 8", "255 * 32 * 8 + 8"]) DBITS(KH(), "HKDF", length);
for (const length of ["256", "8", "0", "12", "null", "undefined", "'64'", "520"]) DBITS(KP(), "PBKDF2", length);
for (const hash of ["SHA-1", "SHA-224", "SHA-384", "SHA-512", "SHA3-256", "MD5", "{name: 'SHA-256'}"]) {
  DBITS(`{name: 'HKDF', hash: ${hash.startsWith("{") ? hash : `'${hash}'`}, salt: new Uint8Array([9, 8]), info: new Uint8Array([7])}`, "HKDF", 64);
  DBITS(`{name: 'PBKDF2', hash: ${hash.startsWith("{") ? hash : `'${hash}'`}, salt: new Uint8Array([9, 8]), iterations: 3}`, "PBKDF2", 64);
}
for (const extra of [", hash: undefined", ", salt: undefined", ", info: undefined", ", salt: 5", ", info: 'x'", ", salt: new Uint8Array(0)"]) DBITS(KH(extra), "HKDF", 64);
for (const extra of [", hash: undefined", ", salt: undefined", ", iterations: undefined", ", iterations: 0", ", iterations: -1", ", iterations: 1.5", ", iterations: NaN", ", salt: new Uint8Array(0)"]) DBITS(KP(extra), "PBKDF2", 64);
DBITS(KP(), "PBKDF2", 64, `crypto.subtle.importKey('raw', new Uint8Array(0), 'PBKDF2', false, ['deriveBits'])`);
DBITS(KH(), "HKDF", 64, `crypto.subtle.importKey('raw', ${U8}, 'HKDF', false, ['deriveKey'])`);
DBITS(KP(), "PBKDF2", 64, `crypto.subtle.importKey('raw', ${U8}, 'PBKDF2', false, ['deriveKey'])`);
DBITS(KH(), "HKDF", 64, KDFIMP("PBKDF2"));
DBITS(KP(), "PBKDF2", 64, KDFIMP("HKDF"));
DBITS(KH(), "HKDF", 64, `crypto.subtle.importKey('raw', ${U8}, {name: 'HMAC', hash: 'SHA-256'}, false, ['sign'])`);
aexpr(`${KDFIMP("HKDF")}.then(function (k) { return crypto.subtle.deriveBits(${KH()}, {}, 64) })`);
const DKEY = (algorithm, name, target, extractable = "true", usages = "['encrypt']", key = KDFIMP(name)) =>
  WITH(key, `crypto.subtle.deriveKey(${algorithm}, k, ${target}, ${extractable}, ${usages}).then(function (d) { return crypto.subtle.exportKey('raw', d).then(function (r) { return [d.type, d.extractable, d.algorithm, d.usages, ${HEX("r")}] }, function (e) { return [d.type, d.extractable, d.algorithm, d.usages, E(e)] }) })`);
for (const [name, algorithm] of [["HKDF", KH()], ["PBKDF2", KP()]]) {
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 256}");
  DKEY(algorithm, name, "{name: 'AES-CBC', length: 128}");
  DKEY(algorithm, name, "{name: 'AES-KW', length: 192}", "true", "['wrapKey']");
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 128}", "false");
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 100}");
  DKEY(algorithm, name, "{name: 'AES-GCM'}");
  DKEY(algorithm, name, "{name: 'HMAC', hash: 'SHA-256'}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'HMAC', hash: 'SHA-512'}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'HMAC', hash: 'SHA-1', length: 8}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'HMAC', hash: 'SHA-256', length: 100}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'HMAC', hash: 'SHA-256', length: 0}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'HMAC'}", "true", "['sign']");
  DKEY(algorithm, name, "'HKDF'", "false", "['deriveBits']");
  DKEY(algorithm, name, "'PBKDF2'", "false", "['deriveBits']");
  DKEY(algorithm, name, "'SHA-256'", "false", "['deriveBits']");
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 128}", "true", "[]");
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 128}", "true", "['sign']");
  DKEY(algorithm, name, "{name: 'AES-GCM', length: 128}", "true", "['encrypt']", `crypto.subtle.importKey('raw', ${U8}, '${name}', false, ['deriveBits'])`);
}
DKEY(`{name: 'HKDF'}`, "HKDF", "{name: 'AES-GCM', length: 128}");
DKEY(`{name: 'PBKDF2', hash: 'SHA-256', salt: ${U8}}`, "PBKDF2", "{name: 'AES-GCM', length: 128}");
DKEY(KH(", hash: 'MD5'"), "HKDF", "{name: 'AES-GCM', length: 128}");

// RSA (RSASSA-PKCS1-v1_5, RSA-PSS, RSA-OAEP): generateKey, importKey e exportKey (spki, pkcs8, jwk). As chaves são aleatórias:
// entram só a forma, os erros e a ida e volta (igualdade, nunca os bytes). Gerações só de 1024 bits e uma de 2048.
const RSA_NAMES = [["RSASSA-PKCS1-v1_5", "sign", "verify", "RS"], ["RSA-PSS", "sign", "verify", "PS"], ["RSA-OAEP", "decrypt", "encrypt", "RSA-OAEP"]];
const RSA_EXP = "new Uint8Array([1, 0, 1])";
const rsaDict = (name, overrides = {}) => {
  const members = { modulusLength: "1024", publicExponent: RSA_EXP, hash: "'SHA-256'", ...overrides };
  return `{name: '${name}', ${Object.entries(members).filter(([, value]) => value !== null).map(([key, value]) => `${key}: ${value}`).join(", ")}}`;
};
const rsaGen = (dict, usages, extractable = "true") => `crypto.subtle.generateKey(${dict}, ${extractable}, ${usages})`;
for (const [name, priv, pub, prefix] of RSA_NAMES) {
  const both = `['${priv}', '${pub}']`;
  // Erros de generateKey que não chegam a gerar (ou falham rápido).
  for (const overrides of [{ hash: null }, { hash: "'MD5'" }, { hash: "'SHA-1'" }, { modulusLength: null }, { publicExponent: null }, { modulusLength: "0" }, { modulusLength: "100" }, { modulusLength: "384" }, { modulusLength: "-1" }, { modulusLength: "'x'" }, { modulusLength: "4294967296" }, { publicExponent: "new Uint8Array([2])" }, { publicExponent: "new Uint8Array(0)" }, { publicExponent: "new Uint8Array([1, 0, 0, 0, 1])" }, { publicExponent: "65537" }, { publicExponent: "new Uint8Array([1, 0, 1]).buffer" }, { publicExponent: "new Uint16Array([3])" }, { modulusLength: null, hash: null }, { publicExponent: "5", hash: null }]) {
    if (overrides.hash === "'SHA-1'") continue;
    aexpr(rsaGen(rsaDict(name, overrides), both));
  }
  for (const usages of ["[]", "['encrypt', 'sign']", "['wrapKey']", "['deriveBits']", `['${pub}']`, "'sign'", "5"]) aexpr(rsaGen(rsaDict(name), usages));
  aexpr(rsaGen(rsaDict(name, { modulusLength: "100" }), "[]"));
  aexpr(rsaGen(rsaDict(name, { modulusLength: "100" }), "['deriveBits']"));
  aexpr(rsaGen(rsaDict(name, { hash: "'MD5'" }), "['deriveBits']"));
  aexpr(`crypto.subtle.generateKey('${name}', true, ${both})`);
  aexpr(`crypto.subtle.generateKey({name: '${name.toLowerCase()}', modulusLength: 1024, publicExponent: ${RSA_EXP}, hash: 'sha-256'}, true, ${both}).then(${SHAPE})`);
  // Forma do par gerado.
  for (const [dict, usages, extractable] of [[rsaDict(name), both, "true"], [rsaDict(name), both, "false"], [rsaDict(name, { hash: "{name: 'sha-384'}" }), both, "true"], [rsaDict(name, { publicExponent: "new Uint8Array([3])", hash: "'SHA-512'" }), both, "true"], [rsaDict(name, { publicExponent: "new Uint8Array([0, 1, 0, 1])", hash: "'SHA3-256'" }), both, "true"], [rsaDict(name, { publicExponent: "Buffer.from([1, 0, 1])", hash: "'SHA-1'" }), `['${priv}']`, "true"], [rsaDict(name), `['${pub}']`, "true"]]) {
    aexpr(`${rsaGen(dict, usages, extractable)}.then(${SHAPE})`);
  }
  // exportKey de todos os formatos, com a chave extraível e com a não extraível.
  const exportCases = (target) => ["jwk", "spki", "pkcs8", "raw", "raw-public", "raw-secret", "raw-seed"].map((format) => `[${JSON.stringify(format)}, p.${target}]`).join(", ");
  for (const extractable of ["true", "false"]) {
    aexpr(`${rsaGen(rsaDict(name), both, extractable)}.then(function (p) { var out = []; var cases = [${exportCases("privateKey")}, ${exportCases("publicKey")}]; return cases.reduce(function (chain, c) { return chain.then(function () { return crypto.subtle.exportKey(c[0], c[1]).then(function (x) { out.push(c[0] === 'jwk' ? [Object.keys(x), x.alg, x.kty, x.ext, x.key_ops, typeof x.n, typeof x.e, typeof x.d, typeof x.qi] : [x instanceof ArrayBuffer, x.byteLength > 100]) }, function (e) { out.push(E(e)) }) }) }, Promise.resolve()).then(function () { return out }) })`);
  }
  // importKey: material válido, usos, hash, formatos e cada defeito do JWK.
  const alg = (hash) => (prefix === "RSA-OAEP" ? (hash === "1" ? "RSA-OAEP" : `RSA-OAEP-${hash}`) : `${prefix}${hash}`);
  const cases = [
    `['spki', spki, A('SHA-512'), ['${pub}']]`, `['pkcs8', pk8, A('SHA-1'), ['${priv}']]`, `['jwk', jp, A('SHA-256'), ['${pub}']]`, `['jwk', jk, A('SHA-256'), ['${priv}']]`,
    `['spki', spki, A('SHA-256'), ['${pub}', 'wrapKey']]`, `['pkcs8', pk8, A('SHA-256'), ['${priv}', 'unwrapKey']]`, `['jwk', jk, A('SHA-256'), ['${priv}', 'unwrapKey']]`,
    `['spki', spki, {name: '${name}'}, ['${pub}']]`, `['spki', spki, '${name}', ['${pub}']]`, `['spki', spki, A('MD5'), ['${pub}']]`, `['spki', spki, A('SHA3-256'), ['${pub}']]`,
    `['spki', spki, A('SHA-256'), ['${priv}']]`, `['pkcs8', pk8, A('SHA-256'), ['${pub}']]`, `['spki', spki, A('SHA-256'), []]`, `['pkcs8', pk8, A('SHA-256'), []]`, `['jwk', jk, A('SHA-256'), []]`, `['jwk', jp, A('SHA-256'), []]`,
    `['jwk', jk, A('SHA-256'), ['${pub}']]`, `['jwk', jp, A('SHA-256'), ['${priv}']]`,
    `['spki', new Uint8Array([1, 2, 3]), A('SHA-256'), ['${pub}']]`, `['pkcs8', new Uint8Array([1, 2, 3]), A('SHA-256'), ['${priv}']]`, `['spki', pk8, A('SHA-256'), ['${pub}']]`, `['pkcs8', spki, A('SHA-256'), ['${priv}']]`,
    `['spki', ecSpki, A('SHA-256'), ['${pub}']]`, `['pkcs8', ecPk8, A('SHA-256'), ['${priv}']]`,
    `['raw', spki, A('SHA-256'), ['${pub}']]`, `['raw-public', spki, A('SHA-256'), ['${pub}']]`, `['raw-secret', spki, A('SHA-256'), ['${pub}']]`, `['raw-seed', spki, A('SHA-256'), ['${pub}']]`,
    `['jwk', spki, A('SHA-256'), ['${pub}']]`, `['spki', jp, A('SHA-256'), ['${pub}']]`,
    `['jwk', J({kty: 'EC'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({kty: undefined}), A('SHA-256'), ['${pub}']]`, `['jwk', J({kty: 'oct'}), A('SHA-256'), ['${pub}']]`,
    `['jwk', J({n: undefined}), A('SHA-256'), ['${pub}']]`, `['jwk', J({e: undefined}), A('SHA-256'), ['${pub}']]`, `['jwk', J({n: 'AA'}), A('SHA-256'), ['${pub}']]`,
    `['jwk', J({alg: '${alg("256")}'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: '${alg("1")}'}), A('SHA-1'), ['${pub}']]`, `['jwk', J({alg: '${alg("224")}'}), A('SHA-224'), ['${pub}']]`, `['jwk', J({alg: '${alg("384")}'}), A('SHA-384'), ['${pub}']]`, `['jwk', J({alg: '${alg("512")}'}), A('SHA-512'), ['${pub}']]`,
    `['jwk', J({alg: '${alg("512")}'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: '${alg("1")}'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: 'HS256'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: 'RS256'}), A('SHA-1'), ['${pub}']]`, `['jwk', J({alg: 'PS256'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: 'RSA-OAEP-256'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: ''}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: '${alg("256")}'}), A('SHA3-256'), ['${pub}']]`, `['jwk', J({alg: undefined}), A('SHA3-256'), ['${pub}']]`,
    `['jwk', J({use: 'enc'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({use: 'sig'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({use: 'x'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({use: 'x'}), A('SHA-256'), []]`,
    `['jwk', J({ext: false}), A('SHA-256'), ['${pub}']]`, `['jwk', J({ext: true}), A('SHA-256'), ['${pub}']]`, `['jwk', J({key_ops: ['${priv}']}), A('SHA-256'), ['${pub}']]`, `['jwk', J({key_ops: ['${pub}', '${pub}']}), A('SHA-256'), ['${pub}']]`, `['jwk', J({key_ops: []}), A('SHA-256'), ['${pub}']]`, `['jwk', J({key_ops: []}), A('SHA-256'), []]`,
    `['jwk', J({alg: 'x', ext: false}), A('SHA-256'), ['${pub}']]`, `['jwk', J({alg: 'x', use: 'x'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({kty: 'EC', use: 'x'}), A('SHA-256'), ['${pub}']]`, `['jwk', J({kty: 'EC', alg: 'x'}), A('SHA-256'), ['${pub}']]`,
    `['jwk', JK({p: undefined}), A('SHA-256'), ['${priv}']]`, `['jwk', JK({q: undefined}), A('SHA-256'), ['${priv}']]`, `['jwk', JK({dp: undefined}), A('SHA-256'), ['${priv}']]`, `['jwk', JK({dq: undefined}), A('SHA-256'), ['${priv}']]`, `['jwk', JK({qi: undefined}), A('SHA-256'), ['${priv}']]`, `['jwk', JK({d: undefined}), A('SHA-256'), ['${priv}']]`,
    `['jwk', JK({ext: false}), A('SHA-256'), ['${priv}']]`,
  ];
  const roundTrips = [["jwk", "jp", pub], ["jwk", "jk", priv], ["spki", "spki", pub], ["pkcs8", "pk8", priv]].map(([format, source, usage]) => `crypto.subtle.importKey('${format}', ${source}, A('SHA-256'), true, ['${usage}']).then(function (k) { return crypto.subtle.exportKey('${format}', k) }).then(function (x) { out.push(['volta ${format}', eq(x, ${source})]) }, function (e) { out.push(E(e)) })`);
  aexpr(
    `${rsaGen(rsaDict(name), both)}.then(function (p) { return Promise.all([crypto.subtle.exportKey('jwk', p.publicKey), crypto.subtle.exportKey('jwk', p.privateKey), crypto.subtle.exportKey('spki', p.publicKey), crypto.subtle.exportKey('pkcs8', p.privateKey), crypto.subtle.generateKey({name: 'ECDSA', namedCurve: 'P-256'}, true, ['sign', 'verify'])]).then(function (m) { return Promise.all([crypto.subtle.exportKey('spki', m[4].publicKey), crypto.subtle.exportKey('pkcs8', m[4].privateKey)]).then(function (ec) { var jp = m[0], jk = m[1], spki = m[2], pk8 = m[3], ecSpki = ec[0], ecPk8 = ec[1]; var out = []; ` +
      `var A = function (h) { return {name: '${name}', hash: h} }; var J = function (o) { return Object.assign({}, jp, o) }; var JK = function (o) { return Object.assign({}, jk, o) }; ` +
      `var eq = function (a, b) { var f = function (v) { return JSON.stringify(v instanceof ArrayBuffer ? Array.from(new Uint8Array(v)) : v) }; return f(a) === f(b) }; ` +
      `var cases = [${cases.join(", ")}]; ` +
      `return cases.reduce(function (chain, c) { return chain.then(function () { return crypto.subtle.importKey(c[0], c[1], c[2], true, c[3]).then(function (k) { out.push([k.type, k.extractable, k.usages, k.algorithm]) }, function (e) { out.push(E(e)) }) }) }, Promise.resolve()).then(function () { return ${roundTrips.reduce((chain, step) => `${chain}.then(function () { return ${step} })`, "Promise.resolve()")} }).then(function () { return out }) }) }) })`,
  );
}
aexpr(`crypto.subtle.generateKey({name: 'RSA-PSS', modulusLength: 2048, publicExponent: ${RSA_EXP}, hash: 'SHA-256'}, true, ['sign', 'verify']).then(${SHAPE})`);

// deriveKey com base ECDH/X25519 e alvo HKDF/PBKDF2 (o bun deriva o segredo inteiro e importa como raw não extraível), conferido
// contra o segredo de deriveBits com length null; os erros de extractable e usos vêm depois da derivação.
for (const [name, curve] of [["ECDH", "P-256"], ["ECDH", "P-384"], ["X25519", ""]]) {
  for (const [target, parameters] of [["HKDF", KH()], ["PBKDF2", KP()]]) {
    const same = `crypto.subtle.deriveBits(${parameters}, d, 128).then(function (x) { return crypto.subtle.deriveBits({name: '${name}', public: b.publicKey}, a.privateKey, null).then(function (raw) { return crypto.subtle.importKey('raw', raw, '${target}', false, ['deriveBits']).then(function (k2) { return crypto.subtle.deriveBits(${parameters}, k2, 128).then(function (y) { return [d.type, d.extractable, d.algorithm, d.usages, ${HEX("x")} === ${HEX("y")}] }) }) }) })`;
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, '${target}', false, ['deriveBits']).then(function (d) { return ${same} })`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, {name: '${target}'}, false, ['deriveKey', 'deriveBits']).then(function (d) { return [d.type, d.extractable, d.algorithm, d.usages] })`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, '${target}', true, ['deriveBits'])`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, '${target}', false, [])`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, '${target}', false, ['sign'])`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, bits.privateKey, '${target}', false, ['deriveBits'])`, name, curve);
    DERIVED(name, `crypto.subtle.deriveKey(alg, a.privateKey, '${target}', true, ['sign']).then(function () { return 1 })`, name, curve);
  }
}

// Ordem de validação do HKDF/PBKDF2 com parâmetros inválidos e chave inválida: os membros do dicionário (hash presente, depois
// os demais em ordem alfabética), o nome do hash, o uso da chave base, o algoritmo da chave, o tamanho, e no deriveKey o alvo.
const HKB = (n, usages = "['deriveBits', 'deriveKey']") => `crypto.subtle.importKey('raw', ${U8}, '${n}', false, ${usages})`;
const HMK = `crypto.subtle.importKey('raw', ${U8}, {name: 'HMAC', hash: 'SHA-256'}, false, ['sign'])`;
const SALT = "salt: new Uint8Array([9, 8])";
for (const [, params] of [
  ["h1", "{name: 'HKDF', hash: 'FOO'}"], ["h2", `{name: 'HKDF', hash: 'FOO', ${SALT}}`], ["h3", "{name: 'HKDF', hash: 5}"],
  ["h4", `{name: 'HKDF', hash: 'SHA-256', info: 'x'}`], ["h5", `{name: 'HKDF', hash: 'SHA-256', ${SALT}}`], ["h6", `{name: 'HKDF', hash: 'SHA-256', ${SALT}, info: 5}`],
  ["h7", `{name: 'HKDF', hash: {}, ${SALT}, info: ${U8}}`], ["h8", `{name: 'HKDF', hash: {}, ${SALT}}`], ["h9", `{name: 'HKDF', hash: null, ${SALT}, info: ${U8}}`],
  ["p1", "{name: 'PBKDF2', hash: 'FOO'}"], ["p2", "{name: 'PBKDF2', hash: 'FOO', iterations: -1}"], ["p3", "{name: 'PBKDF2', hash: 'SHA-256', iterations: -1}"],
  ["p4", "{name: 'PBKDF2', hash: 'SHA-256', iterations: NaN}"], ["p5", `{name: 'PBKDF2', hash: 'SHA-256', iterations: 4294967296, ${SALT}}`],
  ["p6", "{name: 'PBKDF2', hash: 'SHA-256', iterations: 0}"], ["p7", `{name: 'PBKDF2', hash: 'FOO', iterations: 0, ${SALT}}`], ["p8", `{name: 'PBKDF2', hash: 'SHA-256', iterations: 1, salt: 'x'}`],
  ["p9", `{name: 'PBKDF2', hash: {}, iterations: 1, ${SALT}}`], ["p10", `{name: 'PBKDF2', hash: 'SHA3-256', iterations: 1, ${SALT}}`], ["p11", `{name: 'PBKDF2', hash: 'SHA-256', iterations: '5', ${SALT}}`],
]) {
  for (const key of [HKB("HKDF"), HKB("PBKDF2"), HMK, HKB("HKDF", "['deriveBits']"), HKB("PBKDF2", "['deriveKey']")]) {
    DBITS(params, "", "null", key);
    DBITS(params, "", "64", key);
    WITH(key, `crypto.subtle.deriveKey(${params}, k, {name: 'AES-GCM', length: 100}, true, ['sign']).then(function (d) { return d.type })`);
  }
}
for (const [, algorithm] of [["h", KH()], ["p", KP()], ["b", "{name: 'HKDF'}"]]) {
  for (const key of [HKB("HKDF"), HKB("PBKDF2"), HMK, HKB("HKDF", "['deriveBits']"), HKB("PBKDF2", "['deriveKey']")]) {
    for (const [target, usages] of [
      ["{name: 'AES-GCM'}", "['encrypt']"], ["{name: 'AES-GCM', length: 100}", "['encrypt']"], ["{name: 'AES-GCM', length: 'x'}", "['encrypt']"],
      ["{name: 'AES-GCM', length: -1}", "['encrypt']"], ["{name: 'AES-GCM', length: 4294967424}", "['encrypt']"], ["{name: 'AES-GCM', length: 128}", "['encrypt', 'encrypt']"],
      ["{name: 'AES-GCM', length: 128}", "['sign']"], ["{name: 'AES-GCM', length: 128}", "[]"], ["{name: 'AES-GCM', length: 128}", "['bogus']"],
      ["{name: 'AES-GCM', length: 128}", "[5]"], ["{name: 'AES-GCM', length: 128}", "5"], ["'FOO'", "['bogus']"], ["'FOO'", "['encrypt']"], ["'SHA-256'", "['sign']"],
      ["{name: 'HMAC'}", "['sign']"], ["{name: 'HMAC', hash: 'FOO'}", "['sign']"], ["{name: 'HMAC', hash: 'SHA-256', length: 0}", "['sign']"],
      ["{name: 'HMAC', hash: 'SHA-256', length: -1}", "['sign']"], ["'HKDF'", "['deriveBits']"], ["'PBKDF2'", "['deriveBits']"],
    ]) WITH(key, `crypto.subtle.deriveKey(${algorithm}, k, ${target}, true, ${usages}).then(function (d) { return [d.type, d.usages] })`);
  }
}
aexpr(`${HKB("HKDF")}.then(function (k) { return crypto.subtle.deriveKey({name: 'FOO'}, k, {name: 'AES-GCM', length: 128}, true, ['bogus']) })`);
aexpr(`${HKB("HKDF")}.then(function (k) { return crypto.subtle.deriveKey({name: 'SHA-256'}, k, {name: 'AES-GCM', length: 128}, true, ['encrypt']) })`);
aexpr(`${HKB("HKDF")}.then(function (k) { return crypto.subtle.deriveKey(${KH()}, {}, {name: 'AES-GCM', length: 128}, true, ['bogus']) })`);

// deriveBits com length fora do comum (unsigned long do WebIDL: módulo 2^32, NaN e infinito viram 0, string vira número).
// Não entram: PBKDF2 com length -8, 2^31 ou 2^32-8 (o bun calcula centenas de MiB de PBKDF2 e trava), nem HKDF com 2^31
// (recusado, mas sem custo). O porte falha nesses casos como o bun falharia por tamanho, sem prometer terminar.
const LENGTHS = ["-1", "-8", "2 ** 32", "2 ** 32 + 8", "NaN", "1.5", "8.9", "'8'", "'abc'", "Infinity", "-Infinity", "true", "[]", "({})", "-0", "0.5", "-0.5", "2 ** 53", "2 ** 31", "2 ** 32 - 8"];
for (const length of LENGTHS) {
  DBITS(KH(), "HKDF", length);
  if (!["-8", "2 ** 31", "2 ** 32 - 8"].includes(length)) DBITS(KP(), "PBKDF2", length);
  aexpr(`${EC("ECDH", "P-256", "['deriveBits']")}.then(function (a) { return crypto.subtle.deriveBits({name: 'ECDH', public: a.publicKey}, a.privateKey, ${length}).then(function (b) { return b.byteLength }) })`);
  aexpr(`${OKP("X25519", "['deriveBits']")}.then(function (a) { return crypto.subtle.deriveBits({name: 'X25519', public: a.publicKey}, a.privateKey, ${length}).then(function (b) { return b.byteLength }) })`);
}

// RSA: sign/verify (RSASSA-PKCS1-v1_5 e RSA-PSS) e encrypt/decrypt (RSA-OAEP). PSS e OAEP são aleatórios, então saem em ida e volta
// e em erros; o PKCS1-v1_5 é determinístico e sai com vetor fixo. As chaves de 1024 bits são geradas na hora.
const RG = (name, hash, usages) => `crypto.subtle.generateKey({name: '${name}', modulusLength: 1024, publicExponent: ${RSA_EXP}, hash: '${hash}'}, true, ${usages})`;
const RS_SIGN = "['sign', 'verify']";
const RS_DATA = "new Uint8Array([1, 2, 3])";
for (const hash of ["SHA-1", "SHA-256", "SHA-384", "SHA-512"]) {
  aexpr(`${RG("RSASSA-PKCS1-v1_5", hash, RS_SIGN)}.then(function (k) { var s = crypto.subtle; return s.sign('RSASSA-PKCS1-v1_5', k.privateKey, ${RS_DATA}).then(function (g) { return Promise.all([g.byteLength, s.verify('RSASSA-PKCS1-v1_5', k.publicKey, g, ${RS_DATA}), s.verify('RSASSA-PKCS1-v1_5', k.publicKey, g, new Uint8Array([9])), s.verify('RSASSA-PKCS1-v1_5', k.publicKey, g.slice(1), ${RS_DATA}), s.verify({name: 'RSASSA-PKCS1-v1_5'}, k.publicKey, new Uint8Array(128), ${RS_DATA}), s.sign('RSASSA-PKCS1-v1_5', k.privateKey, ${RS_DATA}).then(function (g2) { return new Uint8Array(g).join() === new Uint8Array(g2).join() })]) }) })`);
  aexpr(`${RG("RSA-PSS", hash, RS_SIGN)}.then(function (k) { var s = crypto.subtle; var size = {'SHA-1': 20, 'SHA-256': 32, 'SHA-384': 48, 'SHA-512': 64}['${hash}']; return Promise.all([0, 1, size, 1.5, null].map(function (salt) { return s.sign({name: 'RSA-PSS', saltLength: salt}, k.privateKey, ${RS_DATA}).then(function (g) { return Promise.all([g.byteLength, s.verify({name: 'RSA-PSS', saltLength: salt}, k.publicKey, g, ${RS_DATA}), s.verify({name: 'RSA-PSS', saltLength: salt}, k.publicKey, g, new Uint8Array([9])), s.verify({name: 'RSA-PSS', saltLength: size + 1}, k.publicKey, g, ${RS_DATA})]) }) })) })`);
  aexpr(`${RG("RSA-OAEP", hash, "['encrypt', 'decrypt']")}.then(function (k) { var s = crypto.subtle; var size = {'SHA-1': 20, 'SHA-256': 32, 'SHA-384': 48, 'SHA-512': 64}['${hash}']; var room = 128 - 2 * size - 2; return Promise.all([undefined, new Uint8Array(0), new Uint8Array([1, 2]), new Uint8Array([255, 128, 0]).buffer].map(function (label) { return s.encrypt({name: 'RSA-OAEP', label: label}, k.publicKey, ${RS_DATA}).then(function (c) { return Promise.all([c.byteLength, s.decrypt({name: 'RSA-OAEP', label: label}, k.privateKey, c).then(function (p) { return new Uint8Array(p).join() }), s.decrypt({name: 'RSA-OAEP', label: new Uint8Array([7])}, k.privateKey, c).then(function () { return 'ok' }, E)]) }, E) })).then(function (x) { return room < 0 ? x : s.encrypt('RSA-OAEP', k.publicKey, new Uint8Array(room)).then(function (c) { return s.decrypt('RSA-OAEP', k.privateKey, c).then(function (p) { return p.byteLength }) }).then(function (len) { return s.encrypt('RSA-OAEP', k.publicKey, new Uint8Array(room + 1)).then(function () { return [x, len, 'ok'] }, function (e) { return [x, len, E(e)] }) }) }, E) }, E)`);
}
{
  const RSA_ERRORS = (keyName, hash) => `Promise.all([${RG("RSA-PSS", "SHA-256", RS_SIGN)}, ${RG("RSASSA-PKCS1-v1_5", "SHA-256", RS_SIGN)}, ${RG("RSA-OAEP", "SHA-256", "['encrypt', 'decrypt']")}]).then(function (ks) { var s = crypto.subtle; var pss = ks[0], rs = ks[1], oa = ks[2]; var d = ${RS_DATA}; var T = function (f) { return f().then(function (r) { return r instanceof ArrayBuffer ? 'ArrayBuffer ' + r.byteLength : r }, E) }; return Promise.all([`;
  const cases = [
    "s.sign({name: 'RSA-PSS'}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: -1}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: 'abc'}, pss.privateKey, d)",
    "s.sign({name: 'RSA-PSS', saltLength: 1000}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: 4294967296}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: 94}, pss.privateKey, d)",
    "s.sign({name: 'RSA-PSS', saltLength: 95}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: NaN}, pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: undefined}, pss.privateKey, d)",
    "s.sign('RSA-PSS', pss.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: 32}, pss.publicKey, d)", "s.sign({name: 'RSA-PSS', saltLength: 32}, rs.privateKey, d)",
    "s.sign({name: 'RSA-PSS', saltLength: -1}, rs.privateKey, d)", "s.sign({name: 'RSA-PSS'}, rs.privateKey, d)", "s.sign({name: 'RSA-PSS', saltLength: -1}, pss.publicKey, d)",
    "s.sign('RSASSA-PKCS1-v1_5', pss.privateKey, d)", "s.sign('RSASSA-PKCS1-v1_5', rs.publicKey, d)", "s.sign({name: 'HMAC'}, rs.privateKey, d)",
    "s.verify('RSASSA-PKCS1-v1_5', rs.privateKey, new Uint8Array(128), d)", "s.verify('RSASSA-PKCS1-v1_5', rs.publicKey, new Uint8Array(5), d)", "s.verify('RSASSA-PKCS1-v1_5', rs.publicKey, new Uint8Array(129), d)",
    "s.verify({name: 'RSA-PSS'}, pss.publicKey, new Uint8Array(128), d)", "s.verify({name: 'RSA-PSS', saltLength: -3}, pss.publicKey, new Uint8Array(128), d)", "s.verify({name: 'RSA-PSS', saltLength: 5000}, pss.publicKey, new Uint8Array(128), d)",
    "s.verify({name: 'RSA-PSS', saltLength: 32}, pss.privateKey, new Uint8Array(128), d)", "s.verify({name: 'RSA-PSS', saltLength: 32}, rs.publicKey, new Uint8Array(128), d)",
    "s.encrypt({name: 'RSA-OAEP', label: 'abc'}, oa.publicKey, d)", "s.encrypt({name: 'RSA-OAEP', label: null}, oa.publicKey, d)", "s.encrypt({name: 'RSA-OAEP', label: 5}, oa.publicKey, d)",
    "s.encrypt({name: 'RSA-OAEP', label: 5}, rs.publicKey, d)", "s.encrypt({name: 'RSA-OAEP', label: 5}, oa.privateKey, d)", "s.encrypt({name: 'RSA-OAEP'}, oa.publicKey, 'x')",
    "s.encrypt({name: 'RSA-OAEP'}, oa.privateKey, d)", "s.encrypt({name: 'RSA-OAEP'}, rs.publicKey, d)", "s.encrypt({name: 'RSA-OAEP'}, pss.publicKey, d)",
    "s.decrypt({name: 'RSA-OAEP'}, oa.publicKey, new Uint8Array(128))", "s.decrypt({name: 'RSA-OAEP'}, oa.privateKey, new Uint8Array(5))", "s.decrypt({name: 'RSA-OAEP'}, oa.privateKey, new Uint8Array(0))",
    "s.decrypt({name: 'RSA-OAEP'}, oa.privateKey, new Uint8Array(128))", "s.decrypt({name: 'RSA-OAEP'}, oa.privateKey, new Uint8Array(128).fill(255))", "s.decrypt({name: 'RSA-OAEP'}, oa.privateKey, new Uint8Array(129))",
    "s.encrypt({name: 'RSAES-PKCS1-v1_5'}, oa.publicKey, d)", "s.encrypt({name: 'RSASSA-PKCS1-v1_5'}, rs.publicKey, d)", "s.encrypt({name: 'RSA-PSS'}, pss.publicKey, d)", "s.sign({name: 'RSA-OAEP'}, oa.privateKey, d)",
  ];
  aexpr(`${RSA_ERRORS()}${cases.map((c) => `T(function () { return ${c} })`).join(", ")}]) })`);
  aexpr(`${RG("RSA-OAEP", "SHA-512", "['encrypt', 'decrypt']")}.then(function (k) { return Promise.all([crypto.subtle.encrypt('RSA-OAEP', k.publicKey, new Uint8Array(1)).then(function () { return 'ok' }, E), crypto.subtle.decrypt('RSA-OAEP', k.privateKey, new Uint8Array(128)).then(function () { return 'ok' }, E)]) })`);
}

// PKCS1-v1_5 com vetor fixo: a chave e a assinatura de [1, 2, 3, 4] (SHA-256) saíram do bun.
{
  const FIXED = `{"kty":"RSA","n":"-yeOYlCcl1-duG1nEvlTVHyFVdtebp3h1U9KWPADnoOKI9rmd2Pg27FpuCJ3qpZmSHLXb2R9MKZ17C5o26OXXSKWU240HzJlM7dllySgSEZYcrlYXU97ilBrNElZQ7jLMBBvHwF38f5WH0oB3PidOddGr-Oo-NCMhqsfVUddDWE","e":"AQAB","d":"IjD-Z1AGIW148VSjhae_um7BUDDvKCwCRKHowzbZp0jNE5iHa5WDVSVP-StoEycqgY5w2c9aY7clsqOWzt_0iQpUCHCx5RFOC00HkmLXeivvvyma1ZKzW-HWUu5Hfn2NZCMoX89X0hBfXIHcivVUCEoLZ6RWy1d6FngInNT8deE","p":"_fzZBE7rs-yDm6Vhc3oXuUvqBX_mVnyiy6IipUFJFLF77HQlD28lfbNGWz01yraVnhxjP2PEROXx6mErOKDkxQ","q":"_ST2Siavh1WbiIezeINMZm4sgF7FqZqTlfAgfJmI_HqF7csH9K9LF0qel9rz6IzJHtVvI15Kzbf4fJgmVsZn7Q","dp":"RkM2ffyfM-0QE3TS2rFB8t7PZKoXPIHKP28hCnpfDzxyPd17iyOCSZ3YrtDmGqgcB9tukVC2MSEzpVUwMcBAyQ","dq":"3u1X_4EF_xaCu79Va4GlHGdVxU6wn2XDJr2qvk-vdTipDPpJbU-Zv081TuHA_kBNNVwcXXdCRNwIdiC_UpezGQ","qi":"ZJ6ZmPMgmsJDLSpLM9rasYivD0BSevQPDdLqrPYmhAZ59fRR4dTS49B0ah1QocH1vpljZ0NiHKKY92aAbXRclA"}`;
  const FIXED_SIGNATURE = "212,15,206,18,113,114,132,174,244,235,136,50,1,100,168,128,116,184,144,247,197,13,156,85,142,215,105,212,164,189,100,135,126,237,152,161,160,34,59,54,149,149,56,228,205,19,252,74,113,25,63,27,208,92,82,75,97,200,91,169,91,13,230,67,179,251,239,56,126,127,178,147,79,232,208,20,191,22,135,142,177,255,3,149,243,106,228,6,166,19,103,143,178,62,24,144,206,49,123,2,131,182,80,250,94,27,176,38,187,204,239,30,3,143,187,59,241,128,196,238,236,228,124,58,134,87,231,240";
  const IMPORT = (jwkExpression, usages = "['sign']") => `crypto.subtle.importKey('jwk', ${jwkExpression}, {name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256'}, true, ${usages})`;
  const FIXED_DATA = "new Uint8Array([1, 2, 3, 4])";
  aexpr(`${IMPORT(FIXED)}.then(function (k) { return crypto.subtle.sign('RSASSA-PKCS1-v1_5', k, ${FIXED_DATA}).then(function (g) { return new Uint8Array(g).join() === '${FIXED_SIGNATURE}' }) })`);
  aexpr(`${IMPORT(`(function (j) { return {kty: j.kty, n: j.n, e: j.e}; })(${FIXED})`, "['verify']")}.then(function (k) { return Promise.all([crypto.subtle.verify('RSASSA-PKCS1-v1_5', k, new Uint8Array([${FIXED_SIGNATURE}]), ${FIXED_DATA}), crypto.subtle.verify('RSASSA-PKCS1-v1_5', k, new Uint8Array([${FIXED_SIGNATURE}]), new Uint8Array([1]))]) })`);
  // O bun aceita um JWK privado cujo `d` não bate com p e q: guarda o `d` como veio (sai igual no exportKey), assina com o CRT
  // (mesma assinatura da chave certa) e só falha na assinatura quando p, q, dp, dq ou qi (ou n e e) são incoerentes.
  const MUTATED = (mutation) => `(function (j) { ${mutation}; return j })(${FIXED})`;
  const ROUND = (jwkExpression) => `${IMPORT(jwkExpression)}.then(function (k) { return crypto.subtle.exportKey('jwk', k).then(function (x) { var o = ${FIXED}; return [['d', 'p', 'q', 'dp', 'dq', 'qi', 'n', 'e'].map(function (f) { return f + ':' + (x[f] === o[f] ? 'orig' : x[f] === ${jwkExpression}[f] ? 'given' : 'other') }).join(), Object.keys(x).join()] }).then(function (shape) { return crypto.subtle.sign('RSASSA-PKCS1-v1_5', k, ${FIXED_DATA}).then(function (g) { return [shape, new Uint8Array(g).join() === '${FIXED_SIGNATURE}'] }, function (e) { return [shape, E(e)] }) }) }, E)`;
  for (const mutation of [
    "j.d = 'AQ'", "j.d = 'AA'", "j.d = j.q", "j.dp = 'AQ'", "j.dq = 'AQ'", "j.qi = 'AQ'", "j.p = 'AQ'", "j.q = j.p", "var x = j.p; j.p = j.q; j.q = x",
    "j.p = 'AA' + j.p", "j.n = 'AQAB'", "j.e = 'Aw'", "j.e = 'Ag'", "delete j.dp", "delete j.qi", "delete j.p", "delete j.q", "j.d = ''", "j.d = '!!!'", "j.oth = []",
  ]) aexpr(ROUND(MUTATED(mutation)));
  aexpr(`${IMPORT(MUTATED("j.d = 'AQ'"), "['sign']")}.then(function (k) { return Promise.all([crypto.subtle.exportKey('pkcs8', k).then(function (b) { return b.byteLength }), crypto.subtle.exportKey('spki', k).then(function (b) { return b.byteLength }, E)]) }, E)`);
}

// AES-CFB-8 e ChaCha20-Poly1305: chave, formatos, usos, parâmetros, cifragem e embrulho.
{
  const RAW32 = "new Uint8Array(32).fill(7)";
  const K43 = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc";
  const IV16 = "new Uint8Array(16).fill(1)";
  const IV12 = "new Uint8Array(12).fill(2)";
  const PT = "new Uint8Array([1, 2, 3, 4, 5])";
  const JOIN = "function (b) { return new Uint8Array(b).join() }";
  const imp = (alg, format, data, usages, extractable = "true") => `crypto.subtle.importKey('${format}', ${data}, '${alg}', ${extractable}, ${usages})`;
  const ALL = "['encrypt', 'decrypt', 'wrapKey', 'unwrapKey']";
  for (const alg of ["AES-CFB-8", "ChaCha20-Poly1305"]) {
    aexpr(`crypto.subtle.generateKey({name: '${alg}', length: 256}, true, ['encrypt', 'decrypt']).then(function (k) { return [k.algorithm, k.usages, k.type, k.extractable] }, E)`);
    aexpr(`crypto.subtle.generateKey({name: '${alg}', length: 128}, true, ['sign']).catch(E)`);
    aexpr(`crypto.subtle.generateKey({name: '${alg}', length: 256}, true, []).catch(E)`);
    aexpr(`crypto.subtle.generateKey({name: '${alg}', length: 256}, true, ['wrapKey']).then(function (k) { return k.usages })`);
    aexpr(`${imp(alg, "raw-secret", RAW32, "['sign']")}.catch(E)`);
    aexpr(`${imp(alg, "raw-secret", RAW32, "[]")}.catch(E)`);
    aexpr(`${imp(alg, "raw-secret", "new Uint8Array(5)", "['encrypt']")}.catch(E)`);
    aexpr(`${imp(alg, "raw-secret", "new Uint8Array(5)", "[]")}.catch(E)`);
    aexpr(`${imp(alg, "raw-public", RAW32, "['encrypt']")}.catch(E)`);
    aexpr(`${imp(alg, "spki", RAW32, "['encrypt']")}.catch(E)`);
    aexpr(`${imp(alg, "raw", RAW32, "['sign']")}.catch(E)`);
    aexpr(`${imp(alg, "raw", RAW32, "[]")}.catch(E)`);
    aexpr(`${imp(alg, "raw", "new Uint8Array(3)", "['encrypt']")}.catch(E)`);
    aexpr(`${imp(alg, "raw-secret", RAW32, "['encrypt', 'decrypt']")}.then(function (k) { return [k.algorithm, k.usages, k.type] })`);
    for (const jwk of [
      `{kty: 'oct', k: '${K43}'}`, `{kty: 'oct', k: '${K43}', alg: 'A256CFB8'}`, `{kty: 'oct', k: '${K43}', alg: 'C20P'}`, `{kty: 'oct', k: '${K43}', alg: 'A256GCM'}`,
      `{kty: 'oct', k: '${K43}', alg: 'A256CBC'}`, `{kty: 'oct', k: 'AAAA'}`, `{kty: 'RSA', k: '${K43}'}`, `{kty: 'oct', k: '${K43}', ext: false}`,
      `{kty: 'oct', k: '${K43}', key_ops: ['decrypt']}`, `{kty: 'oct', k: '${K43}', use: 'sig'}`, `{kty: 'oct', k: '${K43}', use: 'enc'}`,
      `{kty: 'oct', k: '${K43}', key_ops: ['encrypt', 'encrypt']}`, `{k: '${K43}'}`, `{kty: 'oct'}`,
    ]) {
      aexpr(`${imp(alg, "jwk", jwk, "['encrypt']")}.then(function (k) { return [k.algorithm, k.usages] }, E)`);
    }
    const KEY = `${imp(alg, "raw-secret", RAW32, ALL)}`;
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.exportKey('raw', k) }).then(${JOIN}, E)`);
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.exportKey('raw-secret', k) }).then(${JOIN}, E)`);
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.exportKey('jwk', k) }).then(function (j) { return j }, E)`);
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.exportKey('spki', k) }).catch(E)`);
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.exportKey('raw-public', k) }).catch(E)`);
    aexpr(`${imp(alg, "raw-secret", RAW32, "['encrypt']", "false")}.then(function (k) { return crypto.subtle.exportKey('raw-secret', k) }).catch(E)`);
    aexpr(`${imp(alg, "raw-secret", RAW32, "['decrypt']")}.then(function (k) { return crypto.subtle.encrypt({name: '${alg}', iv: ${alg === "AES-CFB-8" ? IV16 : IV12}}, k, ${PT}) }).catch(E)`);
    aexpr(`${imp("AES-GCM", "raw", RAW32, "['encrypt']")}.then(function (k) { return crypto.subtle.encrypt({name: '${alg}', iv: ${alg === "AES-CFB-8" ? IV16 : IV12}}, k, ${PT}) }).catch(E)`);
    aexpr(`${KEY}.then(function (k) { return crypto.subtle.sign({name: '${alg}'}, k, ${PT}) }).catch(E)`);
  }
  const CFB_KEY = imp("AES-CFB-8", "raw", RAW32, ALL);
  const CFB = (iv) => `{name: 'AES-CFB-8', iv: ${iv}}`;
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, ${PT}) }).then(${JOIN})`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, new Uint8Array(40).fill(5)) }).then(${JOIN})`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, new Uint8Array(0)) }).then(${JOIN})`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, ${PT}).then(function (c) { return crypto.subtle.decrypt(${CFB(IV16)}, k, c) }) }).then(${JOIN})`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.decrypt(${CFB(IV16)}, k, new Uint8Array([58, 93, 50, 73, 174])) }).then(${JOIN})`);
  aexpr(`${imp("AES-CFB-8", "raw", "new Uint8Array(16).fill(9)", "['encrypt']")}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, ${PT}) }).then(${JOIN})`);
  aexpr(`${imp("AES-CFB-8", "raw", "new Uint8Array(24).fill(9)", "['encrypt']")}.then(function (k) { return crypto.subtle.encrypt(${CFB(IV16)}, k, ${PT}) }).then(${JOIN})`);
  for (const iv of ["new Uint8Array(8)", "new Uint8Array(17)", "new Uint8Array(0)"]) aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt(${CFB(iv)}, k, ${PT}) }).catch(E)`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt({name: 'AES-CFB-8'}, k, ${PT}) }).catch(E)`);
  aexpr(`${CFB_KEY}.then(function (k) { return crypto.subtle.encrypt({name: 'AES-CBC', iv: ${IV16}}, k, ${PT}) }).catch(E)`);
  aexpr(`${imp("AES-CFB-8", "raw", RAW32, "['encrypt']")}.then(function (k) { return crypto.subtle.decrypt(${CFB(IV16)}, k, new Uint8Array(3)) }).catch(E)`);
  aexpr(`${imp("AES-CFB-8", "raw", "new Uint8Array(16)", "['encrypt']")}.then(function (k) { return crypto.subtle.exportKey('jwk', k) }).then(function (j) { return j.alg })`);
  aexpr(`${imp("AES-CFB-8", "raw", "new Uint8Array(24)", "['encrypt']")}.then(function (k) { return crypto.subtle.exportKey('jwk', k) }).then(function (j) { return j.alg })`);
  aexpr(`${imp("AES-CFB-8", "raw", "new Uint8Array(16)", "['encrypt']")}.then(function (k) { return k.algorithm })`);
  aexpr(`crypto.subtle.generateKey({name: 'AES-CFB-8'}, true, ['encrypt']).catch(E)`);
  aexpr(`crypto.subtle.generateKey({name: 'AES-CFB-8', length: 100}, true, ['encrypt']).catch(E)`);
  aexpr(`crypto.subtle.generateKey({name: 'AES-CFB-8', length: 192}, true, ['encrypt']).then(function (k) { return k.algorithm })`);
  aexpr(`${CFB_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('raw', k2, k, ${CFB(IV16)}) }) }).then(${JOIN})`);
  aexpr(`${CFB_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('raw', k2, k, ${CFB(IV16)}) }).then(function (w) { return crypto.subtle.unwrapKey('raw', w, k, ${CFB(IV16)}, 'AES-CBC', true, ['encrypt']) }) }).then(function (u) { return u.algorithm })`);
  aexpr(`${CFB_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('jwk', k2, k, ${CFB(IV16)}) }) }).then(function (w) { return w.byteLength })`);
  aexpr(`${imp("AES-CFB-8", "raw", RAW32, "['encrypt']")}.then(function (k) { return crypto.subtle.unwrapKey('raw', new Uint8Array(16), k, ${CFB(IV16)}, 'AES-CBC', true, ['encrypt']) }).catch(E)`);

  const CC = (extra = "") => `{name: 'ChaCha20-Poly1305', iv: ${IV12}${extra}}`;
  const CC_KEY = imp("ChaCha20-Poly1305", "raw-secret", RAW32, ALL);
  const ccEncrypt = (params, data = PT) => `${CC_KEY}.then(function (k) { return crypto.subtle.encrypt(${params}, k, ${data}) })`;
  aexpr(`${ccEncrypt(CC())}.then(${JOIN})`);
  aexpr(`${ccEncrypt(CC(", additionalData: new Uint8Array([9])"))}.then(${JOIN})`);
  aexpr(`${ccEncrypt(CC(), "new Uint8Array(0)")}.then(function (b) { return b.byteLength })`);
  aexpr(`${ccEncrypt(CC(", tagLength: 128"))}.then(function (b) { return b.byteLength })`);
  for (const params of [
    CC(", tagLength: 96"), CC(", tagLength: 0"), CC(", tagLength: 256"), CC(", tagLength: -1"), CC(", tagLength: NaN"), CC(", additionalData: 5"),
    "{name: 'ChaCha20-Poly1305', iv: new Uint8Array(8), tagLength: 64}", "{name: 'ChaCha20-Poly1305', iv: new Uint8Array(8)}",
    "{name: 'ChaCha20-Poly1305', iv: new Uint8Array(16)}", "{name: 'ChaCha20-Poly1305', iv: new Uint8Array(0)}", "{name: 'ChaCha20-Poly1305'}",
  ]) aexpr(`${ccEncrypt(params)}.catch(E)`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.encrypt(${CC()}, k, ${PT}).then(function (c) { return crypto.subtle.decrypt(${CC()}, k, c) }) }).then(${JOIN})`);
  aexpr(`${CC_KEY}.then(function (k) { var p = ${CC(", additionalData: new Uint8Array(3)")}; return crypto.subtle.encrypt(p, k, ${PT}).then(function (c) { return crypto.subtle.decrypt(p, k, c) }) }).then(${JOIN})`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.encrypt(${CC()}, k, ${PT}).then(function (c) { return crypto.subtle.decrypt(${CC(", additionalData: new Uint8Array(1)")}, k, c) }) }).catch(E)`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.encrypt(${CC()}, k, ${PT}).then(function (c) { var d = new Uint8Array(c); d[0] ^= 1; return crypto.subtle.decrypt(${CC()}, k, d) }) }).catch(E)`);
  for (const size of [0, 5, 15, 16]) aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.decrypt(${CC()}, k, new Uint8Array(${size})) }).catch(E)`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.decrypt(${CC(", tagLength: 96")}, k, new Uint8Array(30)) }).catch(E)`);
  aexpr(`${CC_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('raw', k2, k, ${CC()}) }) }).then(${JOIN})`);
  aexpr(`${CC_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('raw', k2, k, ${CC()}).then(function (w) { return crypto.subtle.unwrapKey('raw', w, k, ${CC()}, 'AES-CBC', true, ['encrypt']) }) }) }).then(function (u) { return u.algorithm })`);
  aexpr(`${CC_KEY}.then(function (k) { return ${imp("AES-CBC", "raw", "new Uint8Array(16).fill(3)", "['encrypt']")}.then(function (k2) { return crypto.subtle.wrapKey('jwk', k2, k, ${CC()}) }) }).then(function (w) { return w.byteLength })`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.wrapKey('raw-secret', k, k, ${CC()}).then(function (w) { return crypto.subtle.unwrapKey('raw-secret', w, k, ${CC()}, 'ChaCha20-Poly1305', true, ['encrypt']) }).then(function (u) { return u.algorithm }) })`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.wrapKey('raw', k, k, ${CC()}) }).catch(E)`);
  aexpr(`${CC_KEY}.then(function (k) { return crypto.subtle.unwrapKey('raw', new Uint8Array(40), k, ${CC()}, 'AES-CBC', true, ['encrypt']) }).catch(E)`);
  aexpr(`${imp("ChaCha20-Poly1305", "raw-secret", RAW32, "['encrypt']")}.then(function (k) { return crypto.subtle.unwrapKey('raw', new Uint8Array(40), k, ${CC()}, 'AES-CBC', true, ['encrypt']) }).catch(E)`);
}

// ML-KEM (FIPS 203) e ML-DSA (FIPS 204) no WebCrypto do bun 1.4.2, medidos em wip/notes/crypto-pq-plan.md. O bun 1.4.2 NÃO tem
// ML-KEM-512 (`Unrecognized algorithm name`), mas tem 768 e 1024, ML-DSA 44, 65 e 87 e o `SubtleCrypto.supports` estático.
// Os vetores determinísticos vêm de `raw-seed` (64 bytes no ML-KEM, 32 no ML-DSA) com a rampa 0, 1, 2, ...: a chave pública, o
// pkcs8 e o spki saem iguais em qualquer implementação correta, então o golden guarda o SHA-256 deles. O `encapsulate` e o
// `sign` do bun são aleatórios (sign "hedged"), por isso a validade de `decapsulate`/`verify` contra o bun entra por vetores
// medidos uma vez no bun e fixos em `crypto-pq-known-answers.json` (`pqKnownAnswers`): o programa e o resultado não mudam entre geração.
let pqKnownAnswers;
{
  const PQH =
    "var Z = async function (fs) { var r = []; for (var i = 0; i < fs.length; i++) { try { r.push(await fs[i]()) } catch (e) { r.push('!' + E(e)) } } return r };\n" +
    "var Y = function (f) { try { return String(f()) } catch (e) { return '!' + E(e) } };\n" +
    "var Q = async function (f) { try { return await f() } catch (e) { return undefined } };\n" +
    "var H = function (b) { return Array.from(new Uint8Array(b)).map(function (x) { return (x + 256).toString(16).slice(1) }).join('') };\n" +
    "var RAMP = function (n) { var u = new Uint8Array(n); for (var i = 0; i < n; i++) u[i] = i; return u };\n" +
    "var SH = async function (b) { return H(await crypto.subtle.digest('SHA-256', b)) };\n" +
    "var G = function (n, e, u) { return crypto.subtle.generateKey(n, e, u).then(function (k) { return [k.publicKey.type, k.publicKey.extractable, k.publicKey.algorithm, k.publicKey.usages, k.privateKey.type, k.privateKey.extractable, k.privateKey.algorithm, k.privateKey.usages] }) };\n" +
    "var X = function (f, k) { return crypto.subtle.exportKey(f, k).then(async function (v) { if (f === 'jwk') return [Object.keys(v).join(), await SH(new TextEncoder().encode(JSON.stringify(v))), v.priv, v.kty, v.alg, v.key_ops, v.ext]; return [v.byteLength, await SH(v)] }) };\n" +
    "var I = function (f, d, n, e, u) { return crypto.subtle.importKey(f, d, n, e, u).then(function (k) { return [k.type, k.extractable, k.algorithm, k.usages] }) };\n";
  const pq = (code) => programs.push(HELPER + PQH + `try { Promise.resolve(${code}).then(function (v) { R = S(v) }, function (e) { R = 'rejeitou ' + E(e) }) } catch (e) { R = E(e) }`);
  const thunks = (list) => `Z([${list.map((c) => `function () { return ${c} }`).join(", ")}])`;
  const KEM = [["ML-KEM-512", 64], ["ML-KEM-768", 64], ["ML-KEM-1024", 64]];
  const DSA = [["ML-DSA-44", 32], ["ML-DSA-65", 32], ["ML-DSA-87", 32]];
  const BITS = "['encapsulateBits', 'decapsulateBits']";
  // Preâmbulo de um algoritmo: a chave privada da rampa, a pública, e os dados exportados (undefined onde o bun recusa).
  const PRE = (n, seedLength, privUsages, pubUsages) =>
    `var n = '${n}'; var seed = RAMP(${seedLength}); var pk = await Q(function () { return crypto.subtle.importKey('raw-seed', seed, n, true, ${privUsages}) }); ` +
    `var pub = await Q(function () { return crypto.subtle.getPublicKey(pk, ${pubUsages}) }); var raw = await Q(function () { return crypto.subtle.exportKey('raw-public', pub) }); ` +
    `var spki = await Q(function () { return crypto.subtle.exportKey('spki', pub) }); var p8 = await Q(function () { return crypto.subtle.exportKey('pkcs8', pk) }); ` +
    `var jw = await Q(function () { return crypto.subtle.exportKey('jwk', pk) }); var jwp = await Q(function () { return crypto.subtle.exportKey('jwk', pub) }); `;
  const KEM_PRIV = "['decapsulateBits', 'decapsulateKey']";
  const KEM_PUB = "['encapsulateBits', 'encapsulateKey']";

  for (const [n, seedLength] of KEM) {
    // generateKey: todas as combinações de uso (o ML-KEM-512 recusa o nome antes de olhar os usos).
    pq(thunks([
      `G('${n}', true, ['encapsulateBits', 'decapsulateBits', 'encapsulateKey', 'decapsulateKey'])`, `G({name: '${n}'}, false, ['encapsulateBits', 'decapsulateBits'])`,
      `G('${n}', true, [])`, `G('${n}', true, ['encapsulateBits'])`, `G('${n}', true, ['decapsulateBits'])`, `G('${n}', true, ['encapsulateKey'])`,
      `G('${n}', true, ['decapsulateKey'])`, `G('${n}', true, ['encapsulateBits', 'encapsulateBits'])`, `G('${n}', true, ['sign'])`,
      `G('${n}', true, ['encapsulateBits', 'sign'])`, `G('${n}', true, ['foo'])`, `G('${n.toLowerCase()}', true, ['decapsulateBits'])`,
      `crypto.subtle.generateKey('${n}', true)`, `G({name: '${n}', foo: 1}, true, ['decapsulateBits'])`,
    ]));
    pq(`(async function () { ${PRE(n, seedLength, KEM_PRIV, KEM_PUB)} return ${thunks([
      ...["spki", "pkcs8", "raw-public", "raw-seed", "jwk", "raw", "raw-secret"].flatMap((f) => [`X('${f}', pub)`, `X('${f}', pk)`]),
      `crypto.subtle.getPublicKey(pk, ['encapsulateBits']).then(function (p) { return [p.type, p.extractable, p.algorithm, p.usages] })`,
      `crypto.subtle.getPublicKey(pk, []).then(function (p) { return p.usages })`, `crypto.subtle.getPublicKey(pk, ['decapsulateBits'])`,
      `crypto.subtle.getPublicKey(pub, ['encapsulateBits'])`, `crypto.subtle.getPublicKey(pk)`,
    ])} })()`);
    pq(`(async function () { ${PRE(n, seedLength, KEM_PRIV, KEM_PUB)} return ${thunks([
      `I('raw-seed', seed, n, true, ['decapsulateBits'])`, `I('raw-seed', seed, n, false, ['decapsulateBits', 'decapsulateKey'])`, `I('raw-seed', seed, n, true, ['encapsulateBits'])`,
      `I('raw-seed', seed, n, true, [])`, `I('raw-public', raw, n, true, ['encapsulateBits'])`, `I('raw-public', raw, n, true, ['decapsulateBits'])`, `I('raw-public', raw, n, true, [])`,
      `I('spki', spki, n, true, ['encapsulateKey', 'encapsulateBits'])`, `I('spki', spki, n, true, [])`, `I('spki', spki, n, true, ['decapsulateBits'])`,
      `I('pkcs8', p8, n, true, ['decapsulateKey', 'decapsulateBits'])`, `I('pkcs8', p8, n, true, [])`, `I('pkcs8', p8, n, true, ['encapsulateBits'])`,
      `I('raw-seed', seed.slice(1), n, true, ['decapsulateBits'])`, `I('raw-public', raw && raw.slice(1), n, true, ['encapsulateBits'])`, `I('spki', p8, n, true, ['encapsulateBits'])`,
      `I('pkcs8', spki, n, true, ['decapsulateBits'])`, `I('raw', seed, n, true, ['decapsulateBits'])`, `I('raw-secret', seed, n, true, ['decapsulateBits'])`, `I('jwk', {}, n, true, ['decapsulateBits'])`,
      `I('raw-seed', seed, {name: n}, true, ['decapsulateBits'])`, `I('raw-seed', seed, '${n.toLowerCase()}', true, ['decapsulateBits'])`,
      `I('raw-seed', seed, n, true, ['decapsulateBits']).then(function () { return crypto.subtle.importKey('raw-seed', seed, n, false, ['decapsulateBits']).then(function (k) { return crypto.subtle.exportKey('raw-seed', k) }) })`,
    ])} })()`);
    const JMUT = (mutation) => `(function () { var j = Object.assign({}, jw); ${mutation}; return I('jwk', j, n, true, ['decapsulateBits']) })()`;
    pq(`(async function () { ${PRE(n, seedLength, KEM_PRIV, KEM_PUB)} return ${thunks([
      `I('jwk', jw, n, true, ['decapsulateBits'])`, `I('jwk', jwp, n, true, ['encapsulateBits'])`, `I('jwk', jwp, n, true, ['decapsulateBits'])`,
      ...[["no alg", "delete j.alg"], ["bad alg", "j.alg = 'ML-KEM-512'"], ["no kty", "delete j.kty"], ["bad kty", "j.kty = 'OKP'"], ["no priv", "delete j.priv"], ["no pub", "delete j.pub"],
        ["bad priv", "j.priv = 'AA'"], ["ext false", "j.ext = false"], ["key_ops wrong", "j.key_ops = ['sign']"], ["key_ops empty", "j.key_ops = []"], ["use enc", "j.use = 'enc'"],
        ["priv other", "j.priv = jw.priv.slice(0, 10) + 'AAAA' + jw.priv.slice(14)"]].map(([, mutation]) => JMUT(mutation)),
    ])} })()`);
    // encapsulate/decapsulate: formas, tamanhos e erros.
    pq(`(async function () { ${PRE(n, seedLength, `['decapsulateBits', 'decapsulateKey']`, KEM_PUB)} var z = new Uint8Array(${n.endsWith("1024") ? 1568 : 1088}); return ${thunks([
      `crypto.subtle.encapsulateBits(n, pub).then(function (r) { return [Object.getPrototypeOf(r) === Object.prototype, Object.keys(r), r.sharedKey.byteLength, r.ciphertext.byteLength, r.sharedKey instanceof ArrayBuffer] })`,
      `crypto.subtle.encapsulateBits({name: n}, pub).then(function (r) { return r.ciphertext.byteLength })`,
      `crypto.subtle.encapsulateBits(n, pub).then(function (r) { return crypto.subtle.decapsulateBits(n, pk, r.ciphertext).then(function (d) { return [H(d) === H(r.sharedKey), d.byteLength] }) })`,
      `crypto.subtle.encapsulateBits(n, pub).then(function (r) { return crypto.subtle.decapsulateBits(n, pk, new Uint8Array(r.ciphertext)).then(function (d) { return H(d) === H(r.sharedKey) }) })`,
      `crypto.subtle.encapsulateBits(n, pub).then(function (r) { var c = new Uint8Array(r.ciphertext.slice(0)); c[0] ^= 1; return crypto.subtle.decapsulateBits(n, pk, c).then(function (d) { return [d.byteLength, H(d) === H(r.sharedKey)] }) })`,
      `crypto.subtle.decapsulateBits(n, pk, z).then(H)`,
      `crypto.subtle.decapsulateBits(n, pk, new Uint8Array(10))`, `crypto.subtle.decapsulateBits(n, pk, new Uint8Array(0))`, `crypto.subtle.decapsulateBits(n, pk, new Uint8Array(z.length + 1))`,
      `crypto.subtle.decapsulateBits(n, pk, 'abc')`, `crypto.subtle.decapsulateBits(n, pk, undefined)`, `crypto.subtle.decapsulateBits(n, pk)`,
      `crypto.subtle.encapsulateBits(n)`, `crypto.subtle.encapsulateBits()`, `crypto.subtle.encapsulateBits(n, pk)`, `crypto.subtle.decapsulateBits(n, pub, z)`,
      `crypto.subtle.encapsulateBits('ML-KEM-512', pub)`, `crypto.subtle.encapsulateBits('${n === "ML-KEM-768" ? "ML-KEM-1024" : "ML-KEM-768"}', pub)`,
      `crypto.subtle.encapsulateBits('AES-GCM', pub)`, `crypto.subtle.encapsulateBits('foo', pub)`, `crypto.subtle.encapsulateBits(n, {})`, `crypto.subtle.encapsulateBits(n, null)`,
      `crypto.subtle.importKey('raw-public', raw, n, true, ['encapsulateKey']).then(function (k) { return crypto.subtle.encapsulateBits(n, k) })`,
      `crypto.subtle.importKey('raw-public', raw, n, true, ['encapsulateBits']).then(function (k) { return crypto.subtle.encapsulateKey(n, k, 'AES-GCM', true, ['encrypt']) })`,
      `crypto.subtle.importKey('raw-public', raw, n, true, []).then(function (k) { return crypto.subtle.encapsulateBits(n, k) })`,
      `crypto.subtle.importKey('pkcs8', p8, n, true, ['decapsulateBits']).then(function (k) { return crypto.subtle.decapsulateKey(n, k, z, 'AES-GCM', true, ['encrypt']) })`,
    ])} })()`);
    // encapsulateKey/decapsulateKey: o algoritmo da chave simétrica resultante.
    pq(`(async function () { ${PRE(n, seedLength, KEM_PRIV, KEM_PUB)} var z = new Uint8Array(${n.endsWith("1024") ? 1568 : 1088}); var EK = function (a, e, u) { return crypto.subtle.encapsulateKey(n, pub, a, e, u).then(function (r) { return [Object.keys(r), r.sharedKey.constructor.name, r.sharedKey.type, r.sharedKey.extractable, r.sharedKey.algorithm, r.sharedKey.usages, r.ciphertext.byteLength] }) }; return ${thunks([
      `EK('AES-GCM', true, ['encrypt', 'decrypt'])`, `EK({name: 'AES-GCM', length: 128}, true, ['encrypt'])`, `EK({name: 'AES-GCM', length: 192}, true, ['encrypt'])`,
      `EK('AES-CBC', false, ['encrypt'])`, `EK({name: 'HMAC', hash: 'SHA-256'}, true, ['sign'])`, `EK('HKDF', false, ['deriveBits'])`, `EK('PBKDF2', false, ['deriveBits'])`,
      `EK('ChaCha20-Poly1305', true, ['encrypt'])`, `EK('AES-KW', true, ['wrapKey'])`, `EK({name: 'ECDSA', namedCurve: 'P-256'}, true, ['sign'])`,
      `EK('AES-GCM', true, ['sign'])`, `EK('AES-GCM', true, [])`, `EK({name: 'AES-GCM', length: 100}, true, ['encrypt'])`, `EK('foo', true, ['encrypt'])`,
      `crypto.subtle.encapsulateKey(n, pub)`, `crypto.subtle.encapsulateKey(n, pub, 'AES-GCM', true)`, `crypto.subtle.decapsulateKey(n, pk, z)`, `crypto.subtle.decapsulateKey(n, pk, z, 'AES-GCM', true, ['encrypt'])`,
      `crypto.subtle.decapsulateKey(n, pub, z, 'AES-GCM', true, ['encrypt'])`,
      `crypto.subtle.encapsulateKey(n, pub, 'AES-GCM', true, ['encrypt', 'decrypt']).then(function (r) { return crypto.subtle.decapsulateKey(n, pk, r.ciphertext, 'AES-GCM', true, ['encrypt', 'decrypt']).then(function (d) { return Promise.all([crypto.subtle.exportKey('raw', d), crypto.subtle.exportKey('raw', r.sharedKey)]).then(function (x) { return [H(x[0]) === H(x[1]), d.algorithm, d.usages, d.type, d.extractable] }) }) })`,
    ])} })()`);
    // vetores da rampa: o SHA-256 da chave pública, do spki, do pkcs8, e o decapsulate do ciphertext zerado (rejeição implícita).
    pq(`(async function () { ${PRE(n, seedLength, KEM_PRIV, KEM_PUB)} var z = new Uint8Array(${n.endsWith("1024") ? 1568 : 1088}); var pz = await Q(function () { return crypto.subtle.importKey('raw-seed', new Uint8Array(${seedLength}), n, true, ['decapsulateBits']) }); var pzp = await Q(function () { return crypto.subtle.getPublicKey(pz, []) }); return ${thunks([
      `raw.byteLength`, `SH(raw)`, `H(raw).slice(0, 64)`, `H(raw).slice(-64)`, `SH(spki)`, `H(spki).slice(0, 48)`, `SH(p8)`, `H(p8)`, `crypto.subtle.decapsulateBits(n, pk, z).then(H)`,
      `crypto.subtle.exportKey('raw-public', pzp).then(function (v) { return Promise.all([SH(v), H(v).slice(0, 64)]) })`, `crypto.subtle.decapsulateBits(n, pz, z).then(H)`,
      `crypto.subtle.exportKey('raw-seed', pk).then(H)`,
    ])} })()`);
  }

  for (const [n, seedLength] of DSA) {
    pq(thunks([
      `G('${n}', true, ['sign', 'verify'])`, `G('${n}', false, ['sign', 'verify'])`, `G('${n}', true, ['sign'])`, `G('${n}', true, ['verify'])`, `G('${n}', true, [])`,
      `G('${n}', true, ['encapsulateBits'])`, `G('${n}', true, ['sign', 'encapsulateBits'])`, `G('${n}', true, ['sign', 'sign'])`, `G('${n}', true, ['foo'])`,
      `G('${n.toLowerCase()}', true, ['sign'])`, `G({name: '${n}'}, true, ['verify', 'sign'])`, `crypto.subtle.generateKey('${n}', true)`,
    ]));
    const DPRE = PRE(n, seedLength, "['sign']", "['verify']");
    pq(`(async function () { ${DPRE} return ${thunks([
      ...["spki", "pkcs8", "raw-public", "raw-seed", "jwk", "raw"].flatMap((f) => [`X('${f}', pub)`, `X('${f}', pk)`]),
      `crypto.subtle.getPublicKey(pk, ['verify']).then(function (p) { return [p.type, p.algorithm, p.usages, p.extractable] })`, `crypto.subtle.getPublicKey(pk, []).then(function (p) { return p.usages })`,
      `crypto.subtle.getPublicKey(pk, ['sign'])`, `crypto.subtle.getPublicKey(pub, ['verify'])`,
    ])} })()`);
    pq(`(async function () { ${DPRE} return ${thunks([
      `I('raw-seed', seed, n, true, ['sign'])`, `I('raw-seed', seed, n, true, ['verify'])`, `I('raw-seed', seed, n, true, [])`, `I('raw-public', raw, n, true, ['verify'])`,
      `I('raw-public', raw, n, true, ['sign'])`, `I('raw-public', raw, n, true, [])`, `I('spki', spki, n, true, ['verify'])`, `I('pkcs8', p8, n, true, ['sign'])`, `I('pkcs8', p8, n, true, [])`,
      `I('pkcs8', p8, n, true, ['verify'])`, `I('raw-seed', seed.slice(1), n, true, ['sign'])`, `I('raw-seed', new Uint8Array(33), n, true, ['sign'])`,
      `I('raw-public', raw.slice(1), n, true, ['verify'])`, `I('spki', p8, n, true, ['verify'])`, `I('pkcs8', spki, n, true, ['sign'])`,
      `I('jwk', jw, n, true, ['sign'])`, `I('jwk', jwp, n, true, ['verify'])`, `I('jwk', Object.assign({}, jw, {alg: 'ML-DSA-0'}), n, true, ['sign'])`,
      `I('jwk', Object.assign({}, jw, {kty: 'OKP'}), n, true, ['sign'])`, `I('jwk', Object.assign({}, jw, {pub: jwp.pub.slice(0, 40) + 'AAAA' + jwp.pub.slice(44)}), n, true, ['sign'])`,
      `I('jwk', Object.assign({}, jw, {key_ops: ['verify']}), n, true, ['sign'])`, `I('jwk', Object.assign({}, jw, {ext: false}), n, true, ['sign'])`,
      `I('raw-seed', seed, {name: n}, true, ['sign'])`,
    ])} })()`);
    pq(`(async function () { ${DPRE} var d = new Uint8Array([1, 2, 3, 4]); var gen = await crypto.subtle.generateKey(n, true, ['sign', 'verify']); var other = '${n === "ML-DSA-65" ? "ML-DSA-44" : "ML-DSA-65"}'; return ${thunks([
      `crypto.subtle.sign(n, pk, d).then(function (g) { return [g.constructor.name, g.byteLength] })`, `crypto.subtle.sign({name: n}, pk, d).then(function (g) { return g.byteLength })`,
      `crypto.subtle.sign({name: n, context: new Uint8Array(0)}, pk, d).then(function (g) { return g.byteLength })`, `crypto.subtle.sign({name: n, context: new Uint8Array(255)}, pk, d).then(function (g) { return g.byteLength })`,
      `crypto.subtle.sign({name: n, context: new Uint8Array(256)}, pk, d)`, `crypto.subtle.sign({name: n, context: 'abc'}, pk, d)`, `crypto.subtle.sign({name: n, context: undefined}, pk, d).then(function (g) { return g.byteLength })`,
      `crypto.subtle.sign(n, pk, new Uint8Array(0)).then(function (g) { return g.byteLength })`, `crypto.subtle.sign(n, pub, d)`, `crypto.subtle.verify(n, pk, new Uint8Array(10), d)`,
      `crypto.subtle.sign(n, pk, 'abc')`, `crypto.subtle.sign(other, pk, d)`, `crypto.subtle.sign('Ed25519', pk, d)`, `crypto.subtle.sign(n, pk)`,
      `crypto.subtle.sign(n, pk, d).then(function (g) { var bad = new Uint8Array(g.slice(0)); bad[0] ^= 1; return Promise.all([crypto.subtle.verify(n, pub, g, d), crypto.subtle.verify(n, pub, g, new Uint8Array([9])), crypto.subtle.verify(n, pub, bad, d), crypto.subtle.verify(n, pub, new Uint8Array(5), d), crypto.subtle.verify(n, pub, new Uint8Array(0), d), crypto.subtle.verify(n, pub, new Uint8Array(g.byteLength + 1), d), crypto.subtle.verify(n, pub, new Uint8Array(g.byteLength), d)]) })`,
      `crypto.subtle.sign({name: n, context: new Uint8Array([1])}, pk, d).then(function (g) { return Promise.all([crypto.subtle.verify({name: n, context: new Uint8Array([1])}, pub, g, d), crypto.subtle.verify(n, pub, g, d), crypto.subtle.verify({name: n, context: new Uint8Array([2])}, pub, g, d)]) })`,
      `crypto.subtle.verify({name: n, context: new Uint8Array(256)}, pub, new Uint8Array(1), d)`, `crypto.subtle.sign(n, pk, d).then(function (a) { return crypto.subtle.sign(n, pk, d).then(function (b) { return H(a) === H(b) }) })`,
      `crypto.subtle.importKey('raw-public', raw, n, true, []).then(function (k) { return crypto.subtle.verify(n, k, new Uint8Array(3), d) })`,
      `crypto.subtle.sign(n, pk, d).then(function (g) { return crypto.subtle.verify(n, gen.publicKey, g, d) })`, `crypto.subtle.verify(n, pub, 'abc', d)`, `crypto.subtle.verify(n, pub, new Uint8Array(1), 'abc')`, `crypto.subtle.verify(n, pub, new Uint8Array(1))`,
      `crypto.subtle.sign('ML-KEM-768', pk, d)`, `crypto.subtle.generateKey('ML-KEM-768', true, ['encapsulateBits']).then(function (kk) { return crypto.subtle.sign(n, kk.privateKey, d) })`,
    ])} })()`);
    pq(`(async function () { ${DPRE} return ${thunks([
      `raw.byteLength`, `SH(raw)`, `H(raw).slice(0, 64)`, `H(raw).slice(-64)`, `SH(spki)`, `H(spki).slice(0, 48)`, `SH(p8)`, `H(p8)`, `jw.priv`, `jw.pub.length`,
      `crypto.subtle.importKey('raw-seed', new Uint8Array(${seedLength}), n, true, ['sign']).then(function (z) { return crypto.subtle.getPublicKey(z, []).then(function (p) { return crypto.subtle.exportKey('raw-public', p).then(function (v) { return Promise.all([SH(v), H(v).slice(0, 64)]) }) }) })`,
    ])} })()`);
  }
  // Entre famílias: chave de um algoritmo usada no outro, e importação com o tamanho errado.
  pq(`(async function () { var kem = await crypto.subtle.generateKey('ML-KEM-768', true, ['encapsulateBits', 'decapsulateBits']); var dsa = await crypto.subtle.generateKey('ML-DSA-65', true, ['sign', 'verify']); return ${thunks([
    `crypto.subtle.sign('ML-DSA-65', kem.privateKey, new Uint8Array(1))`, `crypto.subtle.encapsulateBits('ML-KEM-768', dsa.publicKey)`,
    `crypto.subtle.exportKey('spki', dsa.publicKey).then(function (b) { return I('spki', b, 'ML-KEM-768', true, ['encapsulateBits']) })`,
    `crypto.subtle.exportKey('spki', kem.publicKey).then(function (b) { return I('spki', b, 'ML-DSA-65', true, ['verify']) })`,
    `crypto.subtle.exportKey('spki', dsa.publicKey).then(function (b) { return I('spki', b, 'ML-DSA-44', true, ['verify']) })`,
    `crypto.subtle.exportKey('pkcs8', kem.privateKey).then(function (b) { return I('pkcs8', b, 'ML-KEM-1024', true, ['decapsulateBits']) })`,
    `Object.prototype.toString.call(dsa.privateKey)`, `dsa.privateKey.algorithm === dsa.privateKey.algorithm`,
    `Promise.resolve(structuredClone(dsa.privateKey)).then(function (c) { return [c.type, c.algorithm, c.usages] })`,
  ])} })()`);
  // Forma das funções novas do SubtleCrypto.
  expr(`['encapsulateBits', 'encapsulateKey', 'decapsulateBits', 'decapsulateKey', 'getPublicKey'].map(function (m) { var d = Object.getOwnPropertyDescriptor(SubtleCrypto.prototype, m); return [m, d.value.name, d.value.length, d.enumerable, d.writable, d.configurable, Object.getOwnPropertyDescriptor(SubtleCrypto, m)] })`);
  expr(`Object.getOwnPropertyNames(SubtleCrypto.prototype)`);
  expr(`Object.getOwnPropertyNames(SubtleCrypto)`);
  expr(`(function (d) { return [d.value.name, d.value.length, d.enumerable, d.writable, d.configurable, typeof crypto.subtle.supports, typeof SubtleCrypto.prototype.supports] })(Object.getOwnPropertyDescriptor(SubtleCrypto, 'supports'))`);

  // SubtleCrypto.supports (estático, síncrono, devolve boolean): a tabela de operações por algoritmo, em string e em dicionário.
  const SUPPORT_OPS = ["generateKey", "importKey", "exportKey", "sign", "verify", "digest", "encrypt", "decrypt", "deriveBits", "deriveKey", "wrapKey", "unwrapKey", "encapsulateBits", "encapsulateKey", "decapsulateBits", "decapsulateKey", "getPublicKey", "get key length"];
  const SUPPORT_NAMES = ["RSASSA-PKCS1-v1_5", "RSA-PSS", "RSA-OAEP", "ECDSA", "ECDH", "Ed25519", "X25519", "AES-CTR", "AES-CBC", "AES-GCM", "AES-KW", "AES-CFB-8", "HMAC", "HKDF", "PBKDF2", "SHA-1", "SHA-256", "SHA-384", "SHA-512", "SHA3-256", "ChaCha20-Poly1305",
    "ML-KEM-512", "ML-KEM-768", "ML-KEM-1024", "ML-DSA-44", "ML-DSA-65", "ML-DSA-87", "ml-kem-768", "ML-KEM-999", "foo", "cSHAKE128", "Argon2id", "KMAC128", "AES-OCB", "SLH-DSA-SHA2-128s", "TurboSHAKE128", "KT128"];
  for (const name of SUPPORT_NAMES) expr(`${JSON.stringify(SUPPORT_OPS)}.filter(function (o) { try { return SubtleCrypto.supports(o, ${JSON.stringify(name)}) } catch (e) { return true } })`);
  const SUPPORT_DICTS = {
    "RSASSA-PKCS1-v1_5": "{name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1])}", "RSA-PSS": "{name: 'RSA-PSS', hash: 'SHA-256', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), saltLength: 32}",
    "RSA-OAEP": "{name: 'RSA-OAEP', hash: 'SHA-256', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1])}", ECDSA: "{name: 'ECDSA', namedCurve: 'P-256', hash: 'SHA-256'}", ECDH: "{name: 'ECDH', namedCurve: 'P-256'}",
    Ed25519: "{name: 'Ed25519'}", X25519: "{name: 'X25519'}", "AES-CTR": "{name: 'AES-CTR', length: 256, counter: new Uint8Array(16)}", "AES-CBC": "{name: 'AES-CBC', length: 256, iv: new Uint8Array(16)}", "AES-GCM": "{name: 'AES-GCM', length: 256, iv: new Uint8Array(12)}",
    "AES-KW": "{name: 'AES-KW', length: 256}", "AES-CFB-8": "{name: 'AES-CFB-8', length: 256, iv: new Uint8Array(16)}", HMAC: "{name: 'HMAC', hash: 'SHA-256'}", HKDF: "{name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}",
    PBKDF2: "{name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(0), iterations: 1}", "SHA-256": "{name: 'SHA-256'}", "ChaCha20-Poly1305": "{name: 'ChaCha20-Poly1305', iv: new Uint8Array(12)}",
    "ML-KEM-512": "{name: 'ML-KEM-512'}", "ML-KEM-768": "{name: 'ML-KEM-768'}", "ML-KEM-1024": "{name: 'ML-KEM-1024'}", "ML-DSA-44": "{name: 'ML-DSA-44'}", "ML-DSA-65": "{name: 'ML-DSA-65'}", "ML-DSA-87": "{name: 'ML-DSA-87'}",
  };
  for (const dict of Object.values(SUPPORT_DICTS)) expr(`${JSON.stringify(SUPPORT_OPS)}.filter(function (o) { try { return SubtleCrypto.supports(o, ${dict}) } catch (e) { return true } })`);
  // Argumentos inválidos, o terceiro argumento (comprimento do deriveBits/deriveKey, algoritmo da chave do encapsulateKey) e o this.
  const SUPPORT_CALLS = [
    "", "'generateKey'", "undefined, 'ML-KEM-768'", "null, null", "'generateKey', null", "'generateKey', undefined", "'generateKey', {}", "'generateKey', 5", "'generateKey', []", "'generateKey', Symbol()", "'generateKey', {name: 5}",
    "'GENERATEKEY', 'ML-KEM-768'", "'generatekey', 'ML-KEM-768'", "5, 'ML-KEM-768'", "{}, 'ML-KEM-768'", "'foo', 'ML-KEM-768'",
    "'generateKey', 'ML-KEM-768', {}", "'generateKey', 'ML-KEM-768', 5", "'generateKey', 'ML-KEM-768', null", "'importKey', 'ML-KEM-768', {}", "'importKey', 'AES-GCM', 5", "'importKey', 'AES-GCM', 'foo'",
    "'generateKey', 'AES-GCM'", "'generateKey', {name: 'AES-GCM', length: 256}", "'generateKey', {name: 'AES-GCM', length: 100}", "'generateKey', {name: 'AES-GCM'}", "'generateKey', {name: 'HMAC', hash: 'SHA-256'}", "'generateKey', {name: 'HMAC'}",
    "'generateKey', {name: 'HMAC', hash: 'SHA-256', length: 0}", "'generateKey', {name: 'ECDSA', namedCurve: 'P-256'}", "'generateKey', {name: 'ECDSA', namedCurve: 'P-1'}", "'generateKey', {name: 'ECDSA'}",
    "'generateKey', 'Ed25519'", "'generateKey', 'X25519'", "'generateKey', 'ChaCha20-Poly1305'", "'importKey', 'ECDSA'", "'importKey', 'HMAC'", "'sign', 'ECDSA'", "'sign', {name: 'ECDSA', hash: 'SHA-256'}", "'sign', 'HMAC'", "'sign', 'Ed25519'",
    "'sign', {name: 'ML-DSA-65', context: new Uint8Array(300)}", "'sign', {name: 'ML-DSA-65', context: 'x'}", "'digest', 'sha-256'", "'digest', {name: 'SHA-256', foo: 1}", "'digest', 'MD5'", "'digest', {name: 'cSHAKE128', outputLength: 256}",
    "'encrypt', 'AES-GCM'", "'encrypt', {name: 'AES-GCM', length: 256, iv: new Uint8Array(12)}", "'encrypt', 'RSA-OAEP'", "'wrapKey', 'AES-KW'", "'wrapKey', {name: 'AES-KW', length: 256}",
    "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}", "'deriveBits', 'HKDF', 256", "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 256",
    "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 0", "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 7",
    "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 8", "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 9",
    "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, null", "'deriveBits', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 1048576",
    "'deriveBits', {name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(0), iterations: 1}, 256", "'deriveBits', 'X25519', 256", "'deriveBits', {name: 'ECDH', namedCurve: 'P-256'}, 256",
    "'deriveKey', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, {name: 'AES-GCM', length: 256}", "'deriveKey', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, {name: 'AES-GCM', length: 100}",
    "'deriveKey', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, 'AES-GCM'", "'deriveKey', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, {name: 'HMAC', hash: 'SHA-256'}",
    "'deriveKey', {name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}, {name: 'ML-KEM-768'}", "'deriveKey', {name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(0), iterations: 1}, {name: 'HMAC', hash: 'SHA-256'}",
    "'deriveKey', {name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(0), iterations: 1}, 'HKDF'", "'deriveKey', 'X25519', {name: 'AES-GCM', length: 256}",
    "'encapsulateBits', 'ML-KEM-768'", "'encapsulateBits', 'ML-KEM-512'", "'encapsulateBits', 'ML-KEM-768', 256", "'encapsulateKey', 'ML-KEM-768'", "'encapsulateKey', 'ML-KEM-768', undefined", "'encapsulateKey', 'ML-KEM-768', null",
    "'encapsulateKey', 'ML-KEM-768', 'AES-GCM'", "'encapsulateKey', 'ML-KEM-768', {name: 'AES-GCM', length: 256}", "'encapsulateKey', 'ML-KEM-768', {name: 'AES-GCM', length: 100}", "'encapsulateKey', 'ML-KEM-768', {name: 'HMAC', hash: 'SHA-256'}",
    "'encapsulateKey', 'ML-KEM-768', 'HKDF'", "'encapsulateKey', 'ML-KEM-768', {name: 'ECDSA', namedCurve: 'P-256'}", "'encapsulateKey', 'ML-KEM-768', 'foo'", "'encapsulateKey', 'ML-KEM-512', 'AES-GCM'", "'encapsulateKey', 'ML-DSA-65', 'AES-GCM'",
    "'decapsulateKey', {name: 'ML-KEM-768'}, {name: 'AES-GCM', length: 256}", "'decapsulateBits', 'ML-KEM-1024'", "'getPublicKey', 'ML-DSA-65'", "'getPublicKey', 'ML-KEM-768'", "'getPublicKey', 'AES-GCM'",
    "'get key length', 'AES-GCM'", "'get key length', {name: 'AES-GCM', length: 256}", "'get key length', {name: 'HMAC', hash: 'SHA-256'}",
  ];
  for (let i = 0; i < SUPPORT_CALLS.length; i += 25) expr(`[${SUPPORT_CALLS.slice(i, i + 25).map((c) => `Y(function () { return SubtleCrypto.supports(${c}) })`).join(", ")}]`);
  expr(`[Y(function () { return SubtleCrypto.supports.call(undefined, 'digest', 'SHA-256') }), Y(function () { return SubtleCrypto.supports.call({}, 'digest', 'SHA-256') }), Y(function () { return SubtleCrypto.supports.call(null, 'digest', 'SHA-256') }), Y(function () { var f = SubtleCrypto.supports; return f('digest', 'SHA-256') }), Y(function () { return typeof SubtleCrypto.supports('digest', 'SHA-256') }), Y(function () { return new SubtleCrypto() })]`);


  // Vetores de resposta conhecida GERADOS NA HORA pelo bun: um ciphertext/assinatura válido para a chave da rampa, embutido no fonte como lista de bytes.
  // O porte tem de decapsular/verificar o que o bun produziu (o `encapsulate` e o `sign` do bun são aleatórios, então a linha muda a cada regeneração).
  pqKnownAnswers = async function () {
    // Vetores fixos: medidos uma vez no bun 1.4.2 e gravados em crypto-pq-known-answers.json (hex), para o programa de cada caso
    // e o resultado serem estáveis entre regenerações. Para trocá-los, gere de novo com o bun e revise o diff do golden inteiro.
    const vectors = JSON.parse(require("fs").readFileSync(require("path").join(__dirname, "crypto-pq-known-answers.json"), "utf8"));
    const list = (bytes) => "[" + Array.from(Buffer.from(bytes, "hex")).join(",") + "]";
    for (const n of ["ML-KEM-768", "ML-KEM-1024"]) {
      const ciphertext = vectors.kem[n].ciphertext;
      const sharedKey = vectors.kem[n].sharedKey;
      pq(`(async function () { var n = '${n}'; var pk = await crypto.subtle.importKey('raw-seed', RAMP(64), n, true, ['decapsulateBits', 'decapsulateKey']); var ct = new Uint8Array(${list(ciphertext)}); return ${thunks([
        `crypto.subtle.decapsulateBits(n, pk, ct).then(function (d) { return H(d) === '${sharedKey}' })`,
        `crypto.subtle.decapsulateKey(n, pk, ct, 'AES-GCM', true, ['encrypt']).then(function (k) { return crypto.subtle.exportKey('raw', k).then(function (r) { return H(r) === '${sharedKey}' }) })`,
        `crypto.subtle.decapsulateBits(n, pk, ct.slice(0, ct.length - 1))`,
      ])} })()`);
    }
    for (const n of ["ML-DSA-44", "ML-DSA-65", "ML-DSA-87"]) {
      const data = "01020304";
      const signature = vectors.dsa[n].signature;
      const withContext = vectors.dsa[n].withContext;
      pq(`(async function () { var n = '${n}'; var pk = await crypto.subtle.importKey('raw-seed', RAMP(32), n, true, ['sign']); var pub = await crypto.subtle.getPublicKey(pk, ['verify']); var d = new Uint8Array(${list(data)}); var g = new Uint8Array(${list(signature)}); var gc = new Uint8Array(${list(withContext)}); return ${thunks([
        `crypto.subtle.verify(n, pub, g, d)`, `crypto.subtle.verify(n, pub, g, new Uint8Array([9]))`, `crypto.subtle.verify({name: n, context: new Uint8Array([7, 8])}, pub, gc, d)`, `crypto.subtle.verify(n, pub, gc, d)`,
        `crypto.subtle.verify({name: n, context: new Uint8Array([7, 9])}, pub, gc, d)`, `(function () { var b = g.slice(); b[b.length - 1] ^= 1; return crypto.subtle.verify(n, pub, b, d) })()`,
      ])} })()`);
    }
  };
}

// deriveKey com derivedKeyType ChaCha20-Poly1305: o bun deriva sempre 32 bytes, com ou sem `length`, em qualquer base.
for (const [label, params, base] of [
  ["PBKDF2", "{name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(4), iterations: 2}", "PBKDF2"],
  ["HKDF", "{name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(4), info: new Uint8Array(1)}", "HKDF"],
]) {
  for (const type of ["{name: 'ChaCha20-Poly1305'}", "{name: 'ChaCha20-Poly1305', length: 128}"]) {
    aexpr(`crypto.subtle.importKey('raw', new Uint8Array(8).fill(1), '${base}', false, ['deriveKey']).then(function (k) { return crypto.subtle.deriveKey(${params}, k, ${type}, true, ['encrypt']) }).then(function (d) { return crypto.subtle.exportKey('raw-secret', d).then(function (r) { return [d.algorithm.name, d.extractable, d.usages.join(), r.byteLength] }) })`);
  }
}
aexpr(`crypto.subtle.generateKey({name: 'ECDH', namedCurve: 'P-256'}, true, ['deriveKey']).then(function (a) { return crypto.subtle.generateKey({name: 'ECDH', namedCurve: 'P-256'}, true, ['deriveKey']).then(function (b) { return crypto.subtle.deriveKey({name: 'ECDH', public: b.publicKey}, a.privateKey, 'ChaCha20-Poly1305', true, ['decrypt']) }) }).then(function (d) { return crypto.subtle.exportKey('raw-secret', d).then(function (r) { return [d.algorithm.name, d.usages.join(), r.byteLength] }) })`);
aexpr(`crypto.subtle.generateKey('X25519', true, ['deriveKey']).then(function (a) { return crypto.subtle.generateKey('X25519', true, ['deriveKey']).then(function (b) { return crypto.subtle.deriveKey({name: 'X25519', public: b.publicKey}, a.privateKey, {name: 'ChaCha20-Poly1305'}, true, ['encrypt', 'decrypt']) }) }).then(function (d) { return crypto.subtle.exportKey('raw-secret', d).then(function (r) { return [d.algorithm.name, d.usages.join(), r.byteLength] }) })`);
for (const [usages, extractable] of [["['sign']", "true"], ["[]", "true"], ["['encrypt']", "false"]]) {
  aexpr(`crypto.subtle.importKey('raw', new Uint8Array(8).fill(1), 'PBKDF2', false, ['deriveKey']).then(function (k) { return crypto.subtle.deriveKey({name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(4), iterations: 2}, k, 'ChaCha20-Poly1305', ${extractable}, ${usages}) }).then(function (d) { return [d.extractable, d.usages.join()] })`);
}

// importKey jwk AKP: a ordem entre os defeitos (medida no bun 1.4.2: kty e pub/alg ausentes, use, key_ops duplicado e incompatível,
// ext, alg, usos do tipo, decodificação, usos vazios da privada) e o `use` que cada família aceita (`enc` no ML-KEM, `sig` no ML-DSA).
for (const [n, usage, use, other] of [["ML-KEM-768", "decapsulateBits", "enc", "sig"], ["ML-DSA-44", "sign", "sig", "enc"]]) {
  const pair = n.startsWith("ML-KEM") ? "['decapsulateBits', 'encapsulateBits']" : "['sign', 'verify']";
  const mutations = [
    "alg: undefined, kty: undefined", "alg: undefined, kty: 'OKP'", "alg: 'x', kty: undefined", "alg: 'x', kty: 'OKP'", `kty: 'OKP', use: '${other}'`, `alg: 'x', use: '${other}'`,
    `alg: undefined, use: '${other}'`, `use: '${other}', key_ops: ['${usage}', '${usage}']`, `use: '${other}', ext: false`, `use: '${use}'`, `pub: undefined, use: '${other}'`,
    `alg: 'x', key_ops: ['${usage}', '${usage}']`, "kty: undefined, key_ops: ['verify', 'encrypt']", "ext: false, alg: 'x'", "ext: false, priv: 'AA'", "ext: false, pub: undefined",
    "pub: undefined, alg: 'x'", "pub: undefined, key_ops: ['encrypt']", "priv: undefined, pub: 'AA'", "priv: 'AA', key_ops: ['encrypt']", "alg: 'x', key_ops: ['encrypt']", "key_ops: undefined",
  ];
  for (const usages of [`['${usage}']`, "[]"]) {
    aexpr(`(async function () { var g = await crypto.subtle.generateKey('${n}', true, ${pair}); var jw = await crypto.subtle.exportKey('jwk', g.privateKey); var out = []; ` +
      `var ms = [${mutations.map((m) => `{${m}}`).join(", ")}]; for (var i = 0; i < ms.length; i++) { try { var k = await crypto.subtle.importKey('jwk', Object.assign({}, jw, ms[i]), '${n}', true, ${usages}); out.push([k.type, k.usages]) } catch (e) { out.push(E(e)) } } return out })()`);
  }
}
// getPublicKey de ML-KEM e ML-DSA: usos de privada recusam com `a` (importação), pública extraível mesmo de privada não extraível.
for (const [n, priv, pub] of [["ML-KEM-768", "['decapsulateBits']", "encapsulateKey"], ["ML-DSA-65", "['sign']", "verify"]]) {
  aexpr(`(async function () { var g = await crypto.subtle.generateKey('${n}', false, ${n.startsWith("ML-KEM") ? "['decapsulateBits', 'encapsulateBits']" : "['sign', 'verify']"}); var out = []; var fs = [function () { return crypto.subtle.getPublicKey(g.privateKey, ['${pub}']) }, function () { return crypto.subtle.getPublicKey(g.privateKey, ${priv}) }, function () { return crypto.subtle.getPublicKey(g.publicKey, ['${pub}']) }, function () { return crypto.subtle.exportKey('raw-seed', g.privateKey) }, function () { return crypto.subtle.exportKey('spki', g.privateKey) }, function () { return crypto.subtle.exportKey('raw', g.privateKey) }]; for (var i = 0; i < fs.length; i++) { try { var k = await fs[i](); out.push(k.type ? [k.type, k.extractable, k.usages] : k.byteLength) } catch (e) { out.push(E(e)) } } return out })()`);
}

// SubtleCrypto.supports, segunda rodada (medida no bun 1.4.2): ChaCha20-Poly1305 como alvo, os conversores do RSA no generateKey, o
// comprimento do deriveBits (ToNumber, módulo 2^32) e a ordem entre símbolo, operação e algoritmo (nome ilegível dentro do dicionário é false).
{
  const HK = "{name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: new Uint8Array(0)}";
  const PB = "{name: 'PBKDF2', hash: 'SHA-256', salt: new Uint8Array(0), iterations: 1}";
  const calls = [];
  for (const target of ["'ChaCha20-Poly1305'", "{name: 'ChaCha20-Poly1305'}", "{name: 'ChaCha20-Poly1305', length: 256}", "{name: 'chacha20-poly1305'}"]) {
    calls.push(`'deriveKey', ${HK}, ${target}`, `'deriveKey', ${PB}, ${target}`, `'encapsulateKey', 'ML-KEM-768', ${target}`, `'decapsulateKey', 'ML-KEM-768', ${target}`, `'deriveKey', 'X25519', ${target}`);
  }
  const exponent = "new Uint8Array([1, 0, 1])";
  const moduli = ["2048", "0", "-1", "1.5", "'2048'", "undefined", "null", "NaN", "Infinity", "4294967296", "1", "2**32-1", "{}", "true", "[]", "512", "-0.5", "-1.5", "'0x10'", "{valueOf() { return 2048 }}",
    "{valueOf() { throw new Error('vo') }}", "Symbol()", "1n", "new Number(5)", "' 12 '", "''", "'abc'", "4294967295.9", "4294967296.5"];
  const exponents = [exponent, "new Uint8Array(0)", "new Uint8Array([0])", "new Uint8Array([3])", "new Uint8Array([2])", "[1, 0, 1]", "65537", "'AQAB'", "undefined", "null", "new Uint8Array(8).fill(255)", "new ArrayBuffer(3)",
    "new Uint16Array([1])", "new DataView(new ArrayBuffer(3))", "{}", "new Uint8ClampedArray(3)", "new Int8Array(3)", "new (class extends Uint8Array {})(3)", "new Uint8Array(new SharedArrayBuffer(3))", "new Proxy(new Uint8Array(3), {})"];
  for (const n of ["RSASSA-PKCS1-v1_5", "RSA-OAEP", "RSA-PSS"]) {
    for (const m of moduli) calls.push(`'generateKey', {name: '${n}', hash: 'SHA-256', modulusLength: ${m}, publicExponent: ${exponent}}`);
    for (const e of exponents) calls.push(`'generateKey', {name: '${n}', hash: 'SHA-256', modulusLength: 2048, publicExponent: ${e}}`);
    calls.push(`'generateKey', {name: '${n}', hash: 'SHA-256', modulusLength: 2048}`, `'generateKey', {name: '${n}', hash: 'SHA-256', publicExponent: ${exponent}}`, `'generateKey', {name: '${n}', modulusLength: 2048, publicExponent: ${exponent}}`,
      `'generateKey', {name: '${n}', hash: 'MD5', modulusLength: 2048, publicExponent: ${exponent}}`, `'generateKey', {name: '${n}', hash: 'SHA-256', get modulusLength() { throw new Error('g') }, publicExponent: ${exponent}}`,
      `'importKey', {name: '${n}', hash: 'SHA-256', modulusLength: 'x', publicExponent: 5}`, `'importKey', {name: '${n}'}`, `'importKey', {name: '${n}', hash: 5}`, `'importKey', {name: '${n}', hash: 'MD5'}`,
      `'importKey', {name: '${n}', hash: {name: 'SHA-1'}}`, `'importKey', {name: '${n}', hash: {}}`, `'exportKey', {name: '${n}', hash: 'SHA-256'}`, `'exportKey', {name: '${n}'}`);
  }
  for (const l of ["'256'", "'8'", "null", "undefined", "true", "[8]", "['8']", "{valueOf() { return 8 }}", "{valueOf() { return 7 }}", "{valueOf() { throw new Error('len') }}", "8.5", "Infinity", "NaN", "Symbol()", "2**32", "2**32-8", "2**32-16",
    "1e10", "-8", "0n", "8n", "''", "' 16 '", "'0x10'", "{}", "new Number(16)", "-0", "1e-320"]) calls.push(`'deriveBits', ${HK}, ${l}`, `'deriveBits', ${PB}, ${l}`);
  const thrower = "{get name() { throw new Error('g') }}";
  calls.push(`Symbol(), 'AES-GCM'`, `'foo', Symbol()`, `Symbol(), Symbol()`, `'generateKey', Symbol()`, `Symbol(), {name: 5}`, `'zzz', ${thrower}`, `Symbol(), ${thrower}`, `'generateKey', ${thrower}`, `'zzz', {name: Symbol()}`,
    `'generateKey', {name: Symbol()}`, `{toString() { throw new Error('op') }}, ${thrower}`, `{toString() { throw new Error('op') }}, 'AES-GCM'`, `'generateKey', {toString() { throw new Error('alg') }}`,
    `{toString() { return 'generateKey' }}, {toString() { return 'AES-GCM' }}`, `'generateKey', {name: {toString() { return 'AES-GCM' }}, length: 256}`, `'generateKey', {name: 'AES-GCM', get length() { throw new Error('len') }}`,
    `'zzz', {name: 'AES-GCM', get length() { throw new Error('len') }}`, `'generateKey', {name: 'AES-GCM', length: {valueOf() { throw new Error('vo') }}}`, `'generateKey', {name: 'HMAC', get hash() { throw new Error('hash') }}`,
    `'importKey', {name: 'HMAC', get hash() { throw new Error('hash') }}`, `'sign', {name: 'ECDSA', get hash() { throw new Error('hash') }}`, `'deriveBits', {name: 'HKDF', get hash() { throw new Error('h') }}, 8`,
    `'deriveKey', ${HK}, ${thrower}`, `'encapsulateKey', 'ML-KEM-768', ${thrower}`, `'encapsulateKey', 'zzz', ${thrower}`, `'deriveKey', 'zzz', ${thrower}`, `'digest', {name: 'SHA-256', get length() { throw new Error('l') }}`);
  for (let i = 0; i < calls.length; i += 25) expr(`[${calls.slice(i, i + 25).map((c) => `Y(function () { return SubtleCrypto.supports(${c}) })`).join(", ")}]`);
  // Grade ampla: toda operação contra todo nome, em string, e com o 3º argumento que cada operação lê.
  const ops = ["generateKey", "importKey", "exportKey", "sign", "verify", "digest", "encrypt", "decrypt", "deriveBits", "deriveKey", "wrapKey", "unwrapKey", "encapsulateBits", "encapsulateKey", "decapsulateBits", "decapsulateKey", "getPublicKey", "get key length"];
  for (const extra of ["256", "'AES-GCM'", "{name: 'ChaCha20-Poly1305'}", "{name: 'HMAC', hash: 'SHA-256'}", "'HKDF'", "'PBKDF2'", "{name: 'AES-KW', length: 192}", "{name: 'AES-CTR', length: 127}", "'ML-KEM-768'", "{name: 'ECDH', namedCurve: 'P-256'}"]) {
    expr(`${JSON.stringify(ops)}.map(function (o) { return [${["'HKDF'", "'PBKDF2'", "'ML-KEM-768'", "'X25519'", "'ECDH'", "'ChaCha20-Poly1305'", "'AES-KW'", "'HMAC'"].map((a) => `Y(function () { return SubtleCrypto.supports(o, ${a}, ${extra}) })`).join(", ")}].join('') })`);
  }
}

(async () => {
  await pqKnownAnswers();
  for (const source of programs) {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 20; i++) await Promise.resolve();
    // O digest do bun termina fora da fila de microtarefas: espera o resultado de verdade.
    for (let i = 0; i < 100 && globalThis.R === undefined; i++) await new Promise((resolve) => setTimeout(resolve, 2));
    emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
  }
})();
