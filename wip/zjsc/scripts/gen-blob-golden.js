// Gera tests/golden/blob_bun.tsv: `Blob` do global medido no bun 1.4.2 (descritor do global, `length`, `name`, chaves do
// construtor e do protótipo, descritores, construtor com partes de texto, ArrayBuffer, typed array, DataView e Blob,
// `type` normalizado, `endings`, `size`, `slice` com índices negativos e contentType, `text`, `arrayBuffer`, `bytes`,
// `json` e os erros).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), lido depois do esvaziamento das promessas.
// Uso: bun scripts/gen-blob-golden.js > tests/golden/blob_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
// Resultado de uma promessa: `R` só é gravado quando ela se resolve ou rejeita.
const aexpr = (code) => programs.push(HELPER + `try { Promise.resolve(${code}).then(function (v) { R = S(v) }, function (e) { R = 'rejeitou ' + E(e) }) } catch (e) { R = E(e) }`);

const N = "Blob";
const BYTES = "function (b) { return Array.from(new Uint8Array(b)) }";
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
expr(`${N}.length`);
expr(`${N}.name`);
expr(`Object.getOwnPropertyNames(${N})`);
expr(`Reflect.ownKeys(${N}.prototype).map(String)`);
expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
expr(`Object.getPrototypeOf(${N}) === Function.prototype`);
expr(`${N}.prototype.constructor === ${N}`);
expr(`Object.prototype.toString.call(new ${N}())`);
expr(`String(new ${N}())`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${N}.prototype, 'constructor'))`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.toStringTag))`);
for (const m of ["arrayBuffer", "bytes", "delete", "exists", "formData", "image", "json", "slice", "stat", "stream", "text", "unlink", "write", "writer"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name, Object.getOwnPropertyNames(d.value)] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
}
for (const p of ["size", "type", "name", "lastModified"]) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.set && d.set.name, d.set && d.set.length] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${p}'))`);
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${p}').get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${p}').get.call(null)`);
}
expr(`(function(b){ return [b.name, b.lastModified, typeof b.lastModified] })(new ${N}(['a']))`);
expr(`(function(b){ b.name = 'x'; var a = b.name; b.name = 5; var c = b.name; b.name = null; var d = b.name; b.name = 'y'; b.name = undefined; return [a, c, d, b.name] })(new ${N}(['a']))`);
expr(`(function(b){ b.lastModified = 5; return b.lastModified })(new ${N}(['a']))`);
expr(`new ${N}(['a']).slice(0, 1).lastModified`);
expr(`(function(b){ b.name = 'x'; return b.slice().name })(new ${N}(['abc']))`);
expr(`Object.getOwnPropertyDescriptor(${N}.prototype, 'size').get.call(new ${N}(['abc']))`);
// Chamada e `this` alheio.
expr(`(function(){ try { ${N}() } catch (e) { return [e.name, e.message, e.code] } })()`);
for (const m of ["slice", "text", "arrayBuffer", "bytes", "json"]) {
  expr(`${N}.prototype.${m}.call({})`);
  expr(`${N}.prototype.${m}.call(null)`);
  expr(`${N}.prototype.${m}.call(5)`);
}
// Construtor: partes.
for (const parts of ["1", "'a'", "null", "undefined", "{}", "{ length: 1, 0: 'a' }", "new Set(['ab'])", "'abc'", "true", "new Uint8Array(2)"]) {
  expr(`new ${N}(${parts}).size`);
}
expr(`new ${N}().size`);
expr(`new ${N}([]).size`);
expr(`new ${N}(['abc', 'é', '\\u4f60', '\\ud800']).size`);
expr(`new ${N}([new Uint8Array([1, 2]), new ArrayBuffer(3), new DataView(new ArrayBuffer(2)), new ${N}(['xy']), 5, {}, null, undefined, true]).size`);
expr(`new ${N}([new Uint16Array([1, 2, 3]), new Float64Array(1), new Uint8Array([1, 2, 3, 4]).subarray(1, 3), new DataView(new ArrayBuffer(8), 2, 3)]).size`);
expr(`new ${N}([[1, 2], [3]]).size`);
expr(`new ${N}([{ toString: function () { return 'abcd' } }]).size`);
expr(`new ${N}([{ toString: function () { throw new RangeError('boom') } }])`);
expr(`new ${N}([Symbol()])`);
expr(`new ${N}([1n]).size`);
expr(`new ${N}(['a'], undefined).size`);
expr(`new ${N}(['a'], null).size`);
expr(`new ${N}(['a'], 1)`);
expr(`new ${N}(['a'], 'x')`);
expr(`new ${N}(['a'], {}).type`);
// `type`.
for (const t of ["'Text/PLAIN'", "'text/plain;charset=UTF-8'", "'é'", "'a\\u0100'", "5", "null", "undefined", "''", "' a'", "'a b'", "'A\\u007f'", "'\\u0019'", "'\\u0020'", "'\\u007e'", "{ toString: function () { return 'X/Y' } }", "[]", "['Q/R']", "true"]) {
  expr(`new ${N}(['a'], { type: ${t} }).type`);
}
expr(`new ${N}().type === ''`);
// `endings`.
for (const e of ["'transparent'", "'native'", "'x'", "'NATIVE'", "undefined", "null", "5"]) {
  expr(`[new ${N}(['a\\nb\\r\\nc\\rd'], { endings: ${e} }).size]`);
}
aexpr(`new ${N}(['a\\nb\\r\\nc\\rd'], { endings: 'native' }).text()`);
aexpr(`new ${N}(['a\\nb\\r\\nc\\rd'], { endings: 'transparent' }).text()`);
// `size` e `type` como valores.
expr(`(function(){ var b = new ${N}(['hello'], { type: 'a/b' }); return [b.size, b.type, Object.keys(b), JSON.stringify(b), 'name' in b, b instanceof ${N}] })()`);
expr(`(function(){ var b = new ${N}(['hello']); b.size = 9; return b.size })()`);
expr(`(function(){ 'use strict'; var b = new ${N}(['hello']); try { b.size = 9 } catch (e) { return [e.name, e.message] } })()`);
// `slice`.
const SL = (args) => `(function(){ var b = new ${N}(['hello world'], { type: 'A/B' }); var s = b.slice(${args}); return [s.size, s.type] })()`;
for (const args of ["", "0", "-5", "1, -1", "3, 2", "0, 5, 'X/Y'", "undefined, undefined, 'Q'", "1, 2, 5", "NaN, Infinity", "-Infinity, 3", "0, 100", "100, 200", "-100, -50", "1.9, 4.9", "'2', '5'", "null, null", "undefined, 3, ''", "0, 3, 'A\\u0100'", "0, 3, 'TEXT/Html'", "{ valueOf: function () { return 2 } }, 6", "Symbol()", "1n"]) {
  expr(SL(args));
}
aexpr(`new ${N}(['hello world']).slice(6).text()`);
aexpr(`new ${N}(['hello world']).slice(-5, -1).text()`);
expr(`new ${N}(['abc']).slice(0, 1) instanceof ${N}`);
expr(`(function(){ var b = new ${N}(['abc']); return b.slice() === b })()`);
expr(`(function(){ class X extends ${N} {} var x = new X(['abc']); var s = x.slice(1); return [x instanceof ${N}, x instanceof X, s instanceof X, s.size, Object.getPrototypeOf(s) === ${N}.prototype] })()`);
expr(`new ${N}(['a']).slice.length`);
// Leitura.
aexpr(`new ${N}(['hello world']).text()`);
aexpr(`new ${N}(['\\u4f60\\ud83d\\ude00 é']).text()`);
aexpr(`new ${N}([new Uint8Array([0xff, 0xfe, 0x41])]).text()`);
aexpr(`new ${N}([]).text()`);
aexpr(`new ${N}(['abc']).arrayBuffer().then(function (b) { return [b.constructor.name, b.byteLength, (${BYTES})(b)] })`);
aexpr(`new ${N}(['abc']).bytes().then(function (b) { return [b.constructor.name, b.length, b.byteLength, b.byteOffset, Array.from(b)] })`);
aexpr(`new ${N}([]).arrayBuffer().then(function (b) { return b.byteLength })`);
aexpr(`new ${N}([]).bytes().then(function (b) { return b.length })`);
aexpr(`new ${N}([new Uint8Array([1, 2, 3]), 'a', new ${N}(['bc'])]).bytes().then(function (b) { return Array.from(b) })`);
aexpr(`new ${N}(['{"a":1,"b":[2,3]}']).json()`);
aexpr(`new ${N}(['x']).json()`);
aexpr(`new ${N}(['']).json()`);
aexpr(`new ${N}(['  7  ']).json()`);
aexpr(`new ${N}(['null']).json()`);
expr(`Object.prototype.toString.call(new ${N}(['a']).text())`);
expr(`new ${N}(['a']).text() instanceof Promise`);
expr(`new ${N}(['a']).arrayBuffer() instanceof Promise`);
expr(`new ${N}(['a']).bytes() instanceof Promise`);
expr(`(function(p){ p.catch(function () {}); return p instanceof Promise })(new ${N}(['a']).json())`);
aexpr(`(function(){ var b = new ${N}(['abc']); return Promise.all([b.text(), b.text(), b.size]) })()`);
aexpr(`new ${N}([new ${N}([new ${N}(['deep'])])]).text()`);
aexpr(`new ${N}(['a'], { type: 'text/plain' }).slice(0, 1, 'x/y').text()`);
aexpr(`(function(){ var u = new Uint8Array([65, 66, 67]); var b = new ${N}([u]); u[0] = 90; return b.text() })()`);
aexpr(`(function(){ var a = new ArrayBuffer(2); new Uint8Array(a).set([65, 66]); var b = new ${N}([a]); new Uint8Array(a)[0] = 90; return b.text() })()`);
// Métodos de arquivo num Blob em memória: escrita recusada (vazio é "detached"), `exists` true, `stat` undefined síncrono,
// `formData` rejeita fora de urlencoded/multipart, `image` de Blob vazio lança.
for (const m of ["delete", "unlink", "write", "writer"]) {
  expr(`new ${N}(['abc'], { type: 'a/b' }).${m}('x')`);
  expr(`new ${N}([]).${m}('x')`);
  expr(`new ${N}(['abc']).${m}()`);
  expr(`${N}.prototype.${m}.call({})`);
}
aexpr(`new ${N}(['abc']).exists()`);
aexpr(`new ${N}([]).exists()`);
expr(`new ${N}(['abc']).stat()`);
expr(`new ${N}([]).stat()`);
expr(`${N}.prototype.stat.call({})`);
expr(`${N}.prototype.exists.call({})`);
aexpr(`new ${N}(['a']).formData()`);
aexpr(`new ${N}(['a'], { type: 'text/plain' }).formData()`);
aexpr(`new ${N}(['a'], { type: 'multipart/form-data' }).formData()`);
aexpr(`new ${N}([], { type: 'text/plain' }).formData()`);
// `formData` que parseia: urlencoded (BOM sai, `?` fica), multipart (boundary, nomes, arquivo, erros de corpo).
{
  const U = "application/x-www-form-urlencoded", M = "multipart/form-data; boundary=bb";
  const show = `.then((f) => JSON.stringify([...f.entries()].map(([k, v]) => [k, typeof v === 'string' ? v : [v.name, v.type, v.size, v.lastModified]])))`;
  const fd = (body, type) => aexpr(`new ${N}([${JSON.stringify(body)}], { type: ${JSON.stringify(type)} }).formData()${show}`);
  const mp = (...parts) => parts.map((p) => "--bb\r\n" + p).join("") + "--bb--\r\n";
  const D = (h) => `Content-Disposition: form-data; ${h}`;
  fd("a=1&b=2&a=3&c=%20x+y", U);
  fd("?a=1&b", U);
  fd("﻿a=1", U);
  fd("&&=&a&=v", U);
  fd("a=%zz&b=%4&c=%41", U);
  fd("a=1", "text/plain; application/x-www-form-urlencoded");
  fd("", "text/plain");
  fd("x", "multipart/form-data");
  fd("x", 'multipart/form-data; boundary="bb');
  fd("x", "multipart/form-data; xboundary=bb");
  fd("x", "multipart/form-data; boundary=" + "a".repeat(73));
  fd(mp(D('name="a"') + "\r\n\r\nv1\r\n"), M);
  fd(mp(D('name="a"') + "\r\n\r\n﻿v1\r\n", D('name="a"') + "\r\n\r\n2\r\n"), M);
  fd(mp(D('name="f"; filename="x.txt"') + "\r\n\r\nhello\r\n"), M);
  fd(mp(D('name="f"; filename="x.json"') + "\r\n\r\n{}\r\n"), M);
  fd(mp(D('name="f"; filename=""') + "\r\n\r\nhello\r\n"), M);
  fd(mp("Content-Type: foo/bar\r\n" + D('name="f"; filename="a.png"') + "\r\n\r\nhello\r\n"), M);
  fd(mp(D('name="f"; filename="a"') + "\r\nContent-Type: foo/bar\r\n\r\nhello\r\n"), M);
  fd(mp(D('filename="a"') + "\r\n\r\nz\r\n"), M);
  for (const ext of ["tsx", "yml", "toml", "webp", "wasm", "mp4", "md", "ico", "jsx", "ts", "xlsx", "zip", "TXT", "Png", "weird", "tar.gz", "woff2", "ico", "mjs", "jsonld", "avif", "ttf", "mp3"]) {
    fd(mp(D(`name="f"; filename="a.${ext}"`) + "\r\n\r\nhello\r\n"), M);
  }
  for (const name of [".png", "a.", "a..png", "a.b.png", "dir/a.b/c", "dir/a.png", "a\\b.png", "a\\b", "a.png/", "dir.png/x", "A.PNG", ".bashrc", "x.tar.gz", "..png"]) {
    fd(mp(D(`name="f"; filename="${name}"`) + "\r\n\r\nhello\r\n"), M);
  }
  // Sniff do conteúdo (filename sem extensão conhecida): cabeçalhos de imagem em bytes crus.
  const fdb = (name, bytes) => aexpr(`new ${N}([${JSON.stringify("--bb\r\n" + D(`name="f"; filename="${name}"`) + "\r\n\r\n")}, new Uint8Array(${JSON.stringify(bytes)}), "\\r\\n--bb--\\r\\n"], { type: ${JSON.stringify(M)} }).formData()${show}`);
  const sniffs = [[0x42, 0x4d], [0x42, 0x4d, 1, 2], [0x42], [0xff, 0xd8, 0xff, 0xe0], [0xff, 0xd8], [0x49, 0x49, 0x2a, 0x00, 1], [0x49, 0x49, 0x2a],
    [0x4d, 0x4d, 0x00, 0x2a, 1], [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1], [0x47, 0x49, 0x46, 0x38, 0x37, 0x61, 1], [0x47, 0x49, 0x46, 0x38],
    [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0], [0x89, 0x50, 0x4e, 0x47], [0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50],
    [0x25, 0x50, 0x44, 0x46, 0x2d], [0x68, 0x69]];
  for (const name of ["noext", "a.weird", "a."]) for (const bytes of sniffs) fdb(name, bytes);
  fdb("a.txt", [0x42, 0x4d, 1]);
  fd(mp(D('name="a\\"b"') + "\r\n\r\n1\r\n"), M);
  fd(mp("content-disposition: FORM-DATA; NAME=\"a\"\r\n\r\n1\r\n"), M);
  fd(mp("Content-Disposition: attachment; name=\"a\"\r\n\r\n1\r\n"), M);
  fd(mp(D('name="a"') + "\r\nxx\r\n"), M);
  fd(mp("garbage\r\n\r\nv\r\n"), M);
  fd("--bb\r\n" + D('name="a"') + "\r\n\r\n1\r\n", M);
  fd("--bb--\r\n", M);
}
expr(`new ${N}([]).image()`);
expr(`${N}.prototype.formData.call({})`);
expr(`${N}.prototype.image.call({})`);
expr(`${N}.prototype.stream.call({})`);
expr(`${N}.prototype.stream.call(5)`);
// Lista de partes: Blob e File direto lançam (só ArrayBuffer e visões passam), buraco de array esparso não conta,
// null e undefined dentro do array viram texto, SharedArrayBuffer e visão sobre ele entram.
expr(`new ${N}(new ${N}(['ab'])).size`);
expr(`new ${N}(new File(['ab'], 'f')).size`);
expr(`new ${N}([new File(['ab'], 'f')]).size`);
expr(`new ${N}([, 'a']).size`);
expr(`new ${N}(['a', , , 'b']).size`);
expr(`new ${N}([null]).size`);
expr(`new ${N}([undefined]).size`);
expr(`new ${N}([[null]]).size`);
expr(`new ${N}([[undefined, 1]]).size`);
expr(`new ${N}([new Date(NaN)]).size`);
expr(`new ${N}([new Error('x')]).size`);
expr(`new ${N}(new ArrayBuffer(3)).size`);
expr(`new ${N}(new DataView(new ArrayBuffer(3))).size`);
expr(`new ${N}(new SharedArrayBuffer(3)).size`);
expr(`new ${N}([new SharedArrayBuffer(3)]).size`);
expr(`new ${N}([new Uint8Array(new SharedArrayBuffer(3))]).size`);
expr(`new ${N}(new Proxy(['a'], {})).size`);
expr(`new ${N}(new String('ab')).size`);
expr(`new ${N}([{ toString: null, valueOf: function () { return 'vv' } }]).size`);
expr(`new ${N}([{ toString: function () { return 5 } }]).size`);
expr(`new ${N}(['a'], { get type() { throw new RangeError('tp') } })`);
expr(`new ${N}(['a'], { get endings() { throw new RangeError('en') } }).size`);
aexpr(`new ${N}([new Uint16Array([0x4142])]).text()`);
aexpr(`new ${N}([1n, -0, 1e21, [1, [2]], null, undefined]).text()`);
aexpr(`new ${N}(['abc']).stream().getReader().read().then(function (r) { return [r.done, Array.from(r.value), r.value.constructor.name] })`);
aexpr(`new ${N}([]).stream().getReader().read().then(function (r) { return [r.done, r.value] })`);
expr(`(function(s){ return [s.constructor === ReadableStream, s.locked] })(new ${N}(['a']).stream())`);
expr(`new ${N}(['a']).stream() === new ${N}(['a']).stream()`);
// Sem `new`: TypeError com `code` ERR_ILLEGAL_CONSTRUCTOR.
expr(`${N}([])`);
expr(`${N}()`);
// Função como parte: o bun usa o `toString` da função. O `bun arquivo.js` reformata o fonte (o transpilador reescreve
// `function f(){ return 1 }` como `function f() {\n  return 1;\n}`), mas o código daqui entra por indirect eval, que não
// passa pelo transpilador, e o fonte sai idêntico ao original. Por isso estes casos medem o `Function.prototype.toString`
// real e a diferença do arquivo é efeito do transpilador, fora do escopo do porte.
aexpr(`new ${N}([function f(){ return 1 }]).text()`);
aexpr(`new ${N}([function   g ( a,b ){ return  a+b }]).text()`);
aexpr(`new ${N}([() =>   1]).text()`);
aexpr(`new ${N}([class   K { }]).text()`);
// Corpo multipart de `new Response(formData).text()`: o boundary é aleatório, então o programa o lê do cabeçalho
// `content-type` e o troca por um marcador fixo antes de gravar. Mede também o Content-Type (prefixo) e `.formData()`
// de `Response` com multipart (boundary, sem cabeçalho, text/plain, sem boundary, urlencoded declarado em multipart).
{
  const fixed = (build) =>
    `(function(){ var r = new Response(${build}); var ct = r.headers.get('content-type'); var m = /boundary=(.*)$/.exec(ct || ''); ` +
    `return r.text().then(function (t) { return JSON.stringify([ct && ct.replace(/boundary=.*$/, 'boundary=<B>'), m ? t.split(m[1]).join('<B>') : t]) }) })()`;
  const mk = (stmts) => `(function(){ var f = new FormData(); ${stmts}; return f })()`;
  const bodies = [
    `new FormData()`,
    mk(`f.append('a', 'v1')`),
    mk(`f.append('a', 'v1'); f.append('b', ''); f.append('a', 'v2')`),
    mk(`f.append('f', new Blob(['hello']))`),
    mk(`f.append('f', new Blob(['hello'], { type: 'text/x-foo' }))`),
    mk(`f.append('f', new File(['hello'], 'x.txt', { type: 'text/plain' }))`),
    mk(`f.append('f', new File(['hello'], 'x.bin'))`),
    mk(`f.append('f', new File([], ''))`),
    mk(`f.append('f', new Blob(['hello']), 'nome.png')`),
    mk(`f.append('a"b', 'v')`),
    mk(`f.append('a\\rb', 'v')`),
    mk(`f.append('a\\nb', 'v')`),
    mk(`f.append('a\\r\\nb', 'v')`),
    mk(`f.append('f', new File(['x'], 'a"b.txt', { type: 'text/plain' }))`),
    mk(`f.append('f', new File(['x'], 'a\\rb.txt', { type: 'text/plain' }))`),
    mk(`f.append('f', new File(['x'], 'a\\nb.txt', { type: 'text/plain' }))`),
    mk(`f.append('f', new File(['x'], 'a\\r\\nb"c.txt', { type: 'text/plain' }))`),
    mk(`f.append('a%b', 'v\\r\\nw')`),
    mk(`f.append('', 'v')`),
    mk(`f.append('é', 'ü')`),
  ];
  for (const b of bodies) aexpr(fixed(b));
  const MP = "'multipart/form-data; boundary=bb'";
  const show = `.then(function (f) { return JSON.stringify(Array.from(f.entries()).map(function (kv) { var v = kv[1]; return [kv[0], typeof v === 'string' ? v : [v.name, v.type, v.size]] })) })`;
  const rf = (body, headers) => aexpr(`new Response(${body}, ${headers}).formData()${show}`);
  const part = `'--bb\\r\\nContent-Disposition: form-data; name="a"\\r\\n\\r\\nv1\\r\\n--bb--\\r\\n'`;
  const filePart = `'--bb\\r\\nContent-Disposition: form-data; name="f"; filename="x.txt"\\r\\nContent-Type: text/plain\\r\\n\\r\\nhi\\r\\n--bb--\\r\\n'`;
  rf(part, `{ headers: { 'content-type': ${MP} } }`);
  rf(filePart, `{ headers: { 'content-type': ${MP} } }`);
  rf(part, `{ headers: { 'content-type': 'multipart/form-data; boundary="bb"' } }`);
  rf(`new Uint8Array([97])`, `{}`);
  rf(`new Uint8Array([97])`, `{ headers: { 'content-type': '' } }`);
  rf(part, `{ headers: { 'content-type': 'text/plain' } }`);
  rf(part, `{ headers: { 'content-type': 'multipart/form-data' } }`);
  rf(part, `{ headers: { 'content-type': 'multipart/form-data; charset=utf-8' } }`);
  rf(part, `{ headers: { 'content-type': 'application/x-www-form-urlencoded' } }`);
  rf(`'a=1&b=2'`, `{ headers: { 'content-type': 'application/x-www-form-urlencoded' } }`);
  rf(`'a=1&b=2'`, `{ headers: { 'content-type': 'multipart/form-data; boundary=bb' } }`);
  rf(`''`, `{ headers: { 'content-type': ${MP} } }`);
  rf(`'--bb--\\r\\n'`, `{ headers: { 'content-type': ${MP} } }`);
  rf(`'--bb\\r\\nbroken'`, `{ headers: { 'content-type': ${MP} } }`);
  aexpr(`new Response(${mk(`f.append('a', 'v1'); f.append('f', new File(['hi'], 'x.txt', { type: 'text/plain' }))`)}).formData()${show}`);
  aexpr(`new Response(new FormData()).formData()${show}`);
}
expr(`new ${N}(['hello']).slice(1, 3, 5).type`);
expr(`new ${N}(['hello']).slice(1, 3, 'AB/CD').type`);

(async () => {
  for (const source of programs) {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 20; i++) await Promise.resolve();
    emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
  }
})();
