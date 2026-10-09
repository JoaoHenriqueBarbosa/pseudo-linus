// Gera tests/golden/fetch_offline_bun.tsv: o `fetch` global medido no bun 1.4.2 sem rede (forma da função, esquemas
// `data:`, `blob:` e `file://`, URL relativa e inválida, `Request` como argumento, `init`, ordem das promessas).
// Esquemas de rede (`http://` com host `.invalid`) NÃO são medidos: o bun resolve o nome de verdade (getaddrinfo), o que
// já é tráfego de DNS para fora da máquina.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` como string (JSON). Um processo `bun` por linha,
// rodado como `main.js` num diretório temporário que o stdout normaliza para `/app`. O programa lê o diretório por
// `process.cwd()` (no porte, `/app`). Fixtures criadas antes do programa, no diretório de trabalho:
//   hello.txt = "hello\n", empty.txt = "", data.json = {"a":1}, bin.dat = bytes 0..255, sub/ (diretório vazio),
//   sub/inner.txt = "inner", no-read.txt (modo 000), link.txt -> hello.txt (link simbólico),
//   "sp ace.txt" = "spaced", "ü.txt" = "uml".
// Cada rodada roda todos os casos duas vezes e exige saídas idênticas.
// Uso: bun scripts/gen-fetch-offline-golden.js > tests/golden/fetch_offline_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v === undefined) return 'undefined'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + (e.code === undefined ? '' : e.code) + '|' + e.message };\n" +
  "var D = function (o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x && [typeof x.value, x.writable, x.enumerable, x.configurable, typeof x.get, typeof x.set] };\n" +
  "var DIR = process.cwd();\n" +
  // Resumo de uma Response: campos observáveis mais o corpo como texto (ou o erro de leitura).
  "var F = async function (u, i) { try { var r = await fetch(u, i); var b; try { b = await r.text() } catch (e) { b = 'BODYERR ' + E(e) } " +
  "return { status: r.status, statusText: r.statusText, ok: r.ok, url: r.url, type: r.type, redirected: r.redirected, headers: [...r.headers], body: b } } catch (e) { return 'ERR ' + E(e) } };\n";

const programs = [];
// `code` é uma expressão (promessa ou valor); o resultado é aguardado e serializado. Exceção vira `ERR nome|code|msg`.
const expr = (code) => programs.push(HELPER + `(async function () { try { R = S(await (${code})) } catch (e) { R = 'ERR ' + E(e) } })()`);
// `code` é o corpo de uma função assíncrona que atribui `R` à mão.
const body = (code) => programs.push(HELPER + `(async function () { try { ${code} } catch (e) { R = 'ERR ' + E(e) } })()`);
const fileUrl = (name) => `'file://' + DIR + '/${name}'`;
const D64 = "data:text/plain;base64,";

// Forma do global.
expr(`D(globalThis, 'fetch')`);
expr("typeof fetch");
expr("fetch.length");
expr("fetch.name");
expr("Object.getOwnPropertyNames(fetch)");
expr("Reflect.ownKeys(fetch).length");
expr("D(fetch, 'name')");
expr("D(fetch, 'length')");
expr("'prototype' in fetch");
expr("String(fetch)");
expr("Object.getPrototypeOf(fetch) === Function.prototype");
expr("Object.prototype.toString.call(fetch)");
expr("Object.keys(globalThis).indexOf('fetch') >= 0");
expr("globalThis.fetch === fetch");
expr("self.fetch === fetch");
expr("new fetch('data:,x')");
expr("Reflect.construct(fetch, ['data:,x'])");
expr("typeof fetch.preconnect");
expr("fetch.preconnect.length");
expr("Object.keys(fetch)");
expr("fetch('data:,x') instanceof Promise");
expr("fetch('data:,x').constructor === Promise");
expr("Object.prototype.toString.call(fetch('data:,x'))");
expr("fetch.call(null, 'data:,x') instanceof Promise");
expr("fetch.call(undefined, 'data:,x').then(function (r) { return r.status })");
expr("fetch.call(1, 'data:,x').then(function (r) { return r.status })");
expr("fetch.call({}, 'data:,x').then(function (r) { return r.status })");
expr("(0, fetch)('data:,x').then(function (r) { return r.status })");

// Argumentos que não são URL.
expr("fetch()");
expr("fetch(undefined)");
expr("fetch(null)");
expr("fetch('')");
expr("fetch(1)");
expr("fetch({})");
expr("fetch([])");
expr("fetch(Symbol())");
expr("fetch(function () {})");
expr("fetch(true)");
expr("fetch(1n)");
expr("F({ toString: function () { return 'data:,viaToString' } })");
expr("F({ toString: function () { throw new RangeError('boom') } })");
expr("F({ toString: function () { return {} }, valueOf: function () { return 'data:,valueOf' } })");
expr("F(new URL('data:,fromURL'))");
expr("F(['data:,arr'])");
expr("F(new String('data:,boxed'))");
expr("Promise.resolve().then(function () { try { fetch() } catch (e) { return 'sync ' + E(e) } return 'no sync throw' })");
expr("(function () { try { var p = fetch(); p.catch(function () {}); return 'returned ' + (p instanceof Promise) } catch (e) { return 'sync ' + E(e) } })()");

// data:.
expr("F('data:,hello')");
expr("F('data:text/plain,hello')");
expr(`F('${D64}aGk=')`);
expr(`F('${D64}aGk')`);
expr(`F('${D64}aG k=')`);
expr(`F('${D64}aGk===')`);
expr(`F('${D64}!!!')`);
expr(`F('${D64}')`);
expr("F('data:text/plain;base64,aGk=#frag')");
expr("F('data:text/plain,hello%20world')");
expr("F('data:text/plain,hello world')");
expr("F('data:text/plain,%E2%9C%93')");
expr("F('data:text/plain,%zz')");
expr("F('data:text/plain,%')");
expr("F('data:text/plain,a%0Ab')");
expr("F('data:text/plain,a+b')");
expr("F('data:text/plain,a#b')");
expr("F('data:text/plain,a?b=c')");
expr("F('data:text/html;charset=utf-8,<b>x</b>')");
expr("F('data:text/plain;charset=iso-8859-1,%E9')");
expr("F('data:text/plain;charset=ISO-8859-1;base64,6Q==')");
expr("F('data:text/plain;charset=\"utf-8\",x')");
expr("F('data:TEXT/PLAIN,x')");
expr("F('data:Text/Plain;Charset=UTF-8,x')");
expr("F('data:application/json,{\"a\":1}')");
expr("F('data:application/octet-stream;base64,AAEC')");
expr("F('data:;base64,aGk=')");
expr("F('data:;charset=utf-8,x')");
expr("F('data:,')");
expr("F('data:')");
expr("F('data:text/plain')");
expr("F('data:text/plain;base64')");
expr("F('data:text')");
expr("F('data:/,x')");
expr("F('data:a/b/c,x')");
expr("F('data: text/plain ,x')");
expr("F('data:text/plain ; base64 ,aGk=')");
expr("F('data:text/plain;base64;charset=utf-8,aGk=')");
expr("F('data:text/plain;foo=bar,x')");
expr("F('data:text/plain;foo,x')");
expr("F('data:image/png;base64,iVBORw0KGgo=')");
expr("F('data:text/plain;BASE64,aGk=')");
expr("F('data:text/plain; base64,aGk=')");
expr("F('data://host/path,x')");
expr("F('data:text/plain,\\u00e9')");
expr("F('data:text/plain,é')");
expr("F('data:text/plain,\\ud800')");
expr("F('data:,' + 'x'.repeat(100000)).then(function (r) { return typeof r === 'string' ? r : r.body.length })");
expr("F('data:,x', { method: 'POST' })");
expr("F('data:,x', { method: 'HEAD' })");
expr("F('data:,x', { method: 'DELETE' })");
expr("F('data:,x', { method: 'TRACE' })");
expr("F('data:,x', { method: 'get' })");
expr("F('data:,x', { method: 'FOO' })");
expr("F('data:,x', { method: 'post', body: 'b' })");
expr("F('data:,x', { method: 'GET', body: 'b' })");
expr("F('data:,x', { method: 'HEAD', body: 'b' })");
expr("F('data:,x', { body: 'b' })");
expr("F('data:,x', { method: 'POST', body: 'b', headers: { 'x-a': '1' } })");
expr("F('data:,x', { headers: { 'x-a': '1' } })");
expr("F('data:,x', { headers: [['x-a', '1']] })");
expr("F('data:,x', { headers: new Headers({ 'x-a': '1' }) })");
expr("F('data:,x', { headers: 5 })");
expr("F('data:,x', { headers: [['x']] })");
expr("F('data:,x', { headers: { 'bad name': '1' } })");
expr("F('data:,x', { headers: { 'x': '\\u0100' } })");
expr("F('data:,x', undefined)");
expr("F('data:,x', null)");
expr("F('data:,x', 5)");
expr("F('data:,x', 'str')");
expr("F('data:,x', [])");
expr("F('data:,x', {})");
expr("F('data:,x', { method: undefined })");
expr("F('data:,x', { method: null })");
expr("F('data:,x', { redirect: 'follow' })");
expr("F('data:,x', { redirect: 'manual' })");
expr("F('data:,x', { redirect: 'error' })");
expr("F('data:,x', { redirect: 'bogus' })");
expr("F('data:,x', { mode: 'cors' })");
expr("F('data:,x', { mode: 'navigate' })");
expr("F('data:,x', { credentials: 'include' })");
expr("F('data:,x', { credentials: 'bogus' })");
expr("F('data:,x', { cache: 'no-store' })");
expr("F('data:,x', { cache: 'bogus' })");
expr("F('data:,x', { referrer: 'about:client' })");
expr("F('data:,x', { referrer: 'http://[' })");
expr("F('data:,x', { referrerPolicy: 'no-referrer' })");
expr("F('data:,x', { referrerPolicy: 'bogus' })");
expr("F('data:,x', { integrity: 'sha256-xxx' })");
expr("F('data:,x', { keepalive: true })");
expr("F('data:,x', { priority: 'high' })");
expr("F('data:,x', { priority: 'bogus' })");
expr("F('data:,x', { unknownOption: 1 })");
expr("F('data:,x', { get method() { throw new RangeError('getter') } })");
expr("F('data:,x', { proxy: 'http://x.invalid' })");
expr("F('data:,x', { verbose: true })");
expr("F('data:,x', { decompress: false })");
expr("F('data:,x', { timeout: 1 })");
expr("F('data:,x', { tls: { rejectUnauthorized: false } })");

// Response de data:.
expr("fetch('data:text/plain,abc').then(function (r) { return Object.getPrototypeOf(r) === Response.prototype })");
expr("fetch('data:text/plain,abc').then(function (r) { return [r.constructor.name, r.status, r.statusText, r.ok, r.type, r.redirected, r.bodyUsed, r.url] })");
expr("fetch('data:text/plain,abc').then(function (r) { return [...r.headers] })");
expr("fetch('data:text/plain,abc').then(function (r) { return r.headers.get('content-type') })");
expr("fetch('data:text/plain,abc').then(function (r) { return r.headers.get('content-length') })");
expr("fetch('data:text/plain,abc').then(function (r) { return Object.keys(r) })");
expr("fetch('data:text/plain,abc').then(function (r) { return r.body instanceof ReadableStream })");
expr("fetch('data:,').then(function (r) { return r.body === null })");
expr("fetch('data:,x').then(function (r) { return r.text().then(function (t) { return [t, r.bodyUsed] }) })");
expr("fetch('data:,x').then(function (r) { return r.text().then(function () { return r.text() }) })");
expr("fetch('data:,x').then(function (r) { return r.arrayBuffer().then(function (b) { return [b.byteLength, new Uint8Array(b)[0]] }) })");
expr("fetch('data:text/plain,x').then(function (r) { return r.blob().then(function (b) { return [b.size, b.type] }) })");
expr("fetch('data:application/json,{\"a\":1}').then(function (r) { return r.json() })");
expr("fetch('data:application/json,nope').then(function (r) { return r.json() })");
expr("fetch('data:,x').then(function (r) { return r.bytes().then(function (b) { return [b.constructor.name, b.length] }) })");
expr("fetch('data:,x').then(function (r) { var c = r.clone(); return Promise.all([r.text(), c.text()]) })");
expr("fetch('data:,x').then(function (r) { r.text(); return r.clone() })");
expr("fetch('data:text/plain,x').then(function (r) { return r.headers.set('x', 'y') })");
expr("fetch('data:text/plain,x').then(function (r) { r.headers.set('x', 'y'); return [...r.headers] })");
expr("fetch('data:,x').then(function (r) { return r.formData() })");
expr("fetch('data:application/x-www-form-urlencoded,a=1&b=2').then(function (r) { return r.formData().then(function (f) { return [...f] }) })");
expr("fetch('data:text/plain,\\u00e9').then(function (r) { return r.text() })");
expr("fetch('data:text/plain,%C3%A9').then(function (r) { return r.text().then(function (t) { return [t, t.length] }) })");
expr("fetch('data:text/plain,%FF').then(function (r) { return r.text().then(function (t) { return [t, t.length] }) })");
expr("fetch('data:text/plain,%FF').then(function (r) { return r.arrayBuffer().then(function (b) { return [...new Uint8Array(b)] }) })");
expr("fetch('data:text/plain,x', { method: 'HEAD' }).then(function (r) { return r.text() })");
expr("fetch('data:text/plain,x', { method: 'POST', body: 'zzz' }).then(function (r) { return r.text() })");

// blob:.
body("var u = URL.createObjectURL(new Blob(['blobdata'], { type: 'text/plain' })); var r = await F(u); r.url = r.url === u ? 'same-url' : r.url; R = S(r)");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(u).then(function (r) { return [r.status, r.statusText, r.headers.length, r.headers] }))");
body("var u = URL.createObjectURL(new Blob(['x'], { type: 'application/json' })); var r = await fetch(u); R = S([r.headers.get('content-type'), r.headers.get('content-length'), await r.text()])");
body("var u = URL.createObjectURL(new Blob(['x'], { type: 'Text/Plain; Charset=UTF-8' })); var r = await fetch(u); R = S([r.headers.get('content-type'), r.status])");
body("var u = URL.createObjectURL(new Blob([])); var r = await fetch(u); R = S([r.status, r.headers.get('content-type'), await r.text(), r.body === null])");
body("var u = URL.createObjectURL(new Blob([new Uint8Array([0, 255, 128])])); var r = await fetch(u); R = S([...new Uint8Array(await r.arrayBuffer())])");
body("var u = URL.createObjectURL(new Blob(['x'])); URL.revokeObjectURL(u); R = S(await F(u))");
body("var u = URL.createObjectURL(new Blob(['x'])); var p = fetch(u); URL.revokeObjectURL(u); var r = await p; R = S([r.status, await r.text()])");
body("var u = URL.createObjectURL(new Blob(['x'])); var r = await fetch(u); URL.revokeObjectURL(u); R = S([r.status, await r.text()])");
body("var u = URL.createObjectURL(new Blob(['x'])); var r1 = await fetch(u); var r2 = await fetch(u); R = S([await r1.text(), await r2.text()])");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(u, { method: 'POST' }))");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(u, { method: 'HEAD' }))");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(u, { method: 'POST', body: 'b' }))");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(new URL(u)))");
body("var u = URL.createObjectURL(new Blob(['x'])); R = S(await F(new Request(u)))");
body("var u = URL.createObjectURL(new Blob(['x'])); var r = await fetch(u); R = S([r.url === u, r.type, r.redirected])");
body("var u = URL.createObjectURL(new Blob(['abcdef'])); var r = await fetch(u, { headers: { range: 'bytes=1-2' } }); R = S([r.status, await r.text()])");
body("var u = URL.createObjectURL(new File(['x'], 'n.txt', { type: 'text/plain' })); var r = await fetch(u); R = S([r.status, r.headers.get('content-type'), await r.text()])");
body("var f = new File(['ab'], 'a.txt', { type: 'text/x' }); var u = URL.createObjectURL(f); var b = await (await fetch(u)).blob(); R = S([b instanceof File, b.name, b.type, b.size, b.lastModified === f.lastModified, Object.getPrototypeOf(b) === File.prototype])");
body("var u = URL.createObjectURL(new Blob(['q'])); var b = await (await fetch(u)).blob(); R = S([b instanceof File, b.name, b.size])");
body("var f = new File(['ab'], 'a.txt', { type: 'text/x' }); var b = await new Response(f).blob(); R = S([b instanceof File, b.name, b.type, b.size])");
body("var f = new File(['ab'], 'a.txt'); var u = URL.createObjectURL(f); var c = (await fetch(u)).clone(); var b = await c.blob(); R = S([b instanceof File, b.name])");
body("var u = URL.createObjectURL(new Blob(['x'], { type: 'text/plain' })); var x = new URL(u); R = S([u.replace(/[0-9a-f-]{36}$/, 'UUID'), x.origin, x.protocol, x.pathname.length, x.host, x.search, x.hash, x.href === u])");
body("var u = URL.createObjectURL(new File(['x'], 'n.txt')); R = S([u.length, /^blob:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(u), new URL(u + '?a#b').pathname.length])");
body("var o = []; [1, 's', null, undefined, {}, new Uint8Array(1), new Blob([]), new File([], 'e')].forEach(function (a) { try { o.push(typeof URL.createObjectURL(a)) } catch (e) { o.push([e.name, e.code, e.message]) } }); R = S(o)");
body("var o = []; try { URL.createObjectURL() } catch (e) { o.push([e.name, e.code, e.message]) } try { URL.revokeObjectURL() } catch (e) { o.push([e.name, e.code, e.message]) } [1, null, undefined, {}, ['blob:x']].forEach(function (a) { try { o.push(URL.revokeObjectURL(a)) } catch (e) { o.push([e.name, e.code, e.message]) } }); R = S(o)");
body("R = S([URL.revokeObjectURL('blob:nope'), URL.revokeObjectURL('http://x/'), URL.revokeObjectURL(''), URL.revokeObjectURL(new String('blob:y'))])");
body("var u = URL.createObjectURL(new Blob(['x'])); URL.revokeObjectURL(u); URL.revokeObjectURL(u); R = S(await F(u))");
body("var u = URL.createObjectURL(new Blob(['x'])); URL.revokeObjectURL(new String(u)); R = S(await F(u))");
expr("F('blob:http://localhost/00000000-0000-0000-0000-000000000000')");
// Fora de medição de propósito: `blob:` sem UUID registrado, em maiúsculas (`DATA:`, `BLOB:`), com sufixo (`#frag`) ou
// mal formado, e esquemas desconhecidos (`x:y`, `about:blank`, `javascript:1`, `mailto:`, `tel:`, `unix:`), além de URL com
// espaço antes do esquema: o bun trata tudo isso como `http:` com o esquema virando nome de host e chama getaddrinfo (DNS
// de verdade, tráfego fora da máquina). A guarda no fim do script falha se algum caso medir ENOTFOUND/getaddrinfo.

// file://.
expr(`F(${fileUrl("hello.txt")})`);
expr(`F(${fileUrl("hello.txt")}).then(function (r) { return typeof r === 'string' ? r : r.url === ${fileUrl("hello.txt")} })`);
expr(`F(${fileUrl("empty.txt")})`);
expr(`F(${fileUrl("data.json")})`);
expr(`F(${fileUrl("bin.dat")}).then(function (r) { return typeof r === 'string' ? r : [r.status, r.headers, r.body.length] })`);
expr(`fetch(${fileUrl("bin.dat")}).then(function (r) { return r.arrayBuffer().then(function (b) { var u = new Uint8Array(b); var ok = true; for (var i = 0; i < 256; i++) if (u[i] !== i) ok = false; return [b.byteLength, ok] }) })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { return [r.headers.get('content-type'), r.headers.get('content-length'), r.headers.get('last-modified'), r.headers.get('etag'), [...r.headers.keys()]] })`);
expr(`fetch(${fileUrl("data.json")}).then(function (r) { return [r.headers.get('content-type'), r.json ? 'has-json' : 0] }).then(function (a) { return a })`);
expr(`fetch(${fileUrl("data.json")}).then(function (r) { return r.json() })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { return r.blob().then(function (b) { return [b.size, b.type] }) })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { return [r.status, r.statusText, r.ok, r.type, r.redirected, r.bodyUsed, r.body instanceof ReadableStream] })`);
expr(`fetch(${fileUrl("empty.txt")}).then(function (r) { return [r.body === null, r.headers.get('content-length')] })`);
expr(`F(${fileUrl("nope.txt")})`);
expr(`F(${fileUrl("sub/nope.txt")})`);
expr(`F(${fileUrl("nope/nope.txt")})`);
expr(`F(${fileUrl("sub")})`);
expr(`F(${fileUrl("sub")} + '/')`);
expr(`F(${fileUrl("sub/inner.txt")})`);
expr(`F(${fileUrl("sub/../hello.txt")})`);
expr(`F(${fileUrl("./hello.txt")})`);
expr(`F(${fileUrl("link.txt")})`);
expr(`F(${fileUrl("sp ace.txt")})`);
expr(`F(${fileUrl("sp%20ace.txt")})`);
expr(`F(${fileUrl("%C3%BC.txt")})`);
expr(`F(${fileUrl("ü.txt")})`);
expr(`F(${fileUrl("hello.txt")} + '#frag')`);
expr(`F(${fileUrl("hello.txt")} + '?q=1')`);
expr(`F(${fileUrl("hello.txt")} + '%00')`);
expr(`F(${fileUrl("hello.txt")} + '%2F')`);
expr(`F(${fileUrl("no-read.txt")})`);
expr(`F('file://localhost' + DIR + '/hello.txt')`);
expr(`F('file://example.invalid' + DIR + '/hello.txt')`);
expr(`F('file:' + DIR + '/hello.txt')`);
expr(`F('file:///nonexistent-root-dir/x')`);
expr(`F('file:///')`);
expr(`F('file://')`);
expr(`F('file:')`);
expr(`F('FILE://' + DIR + '/hello.txt')`);
expr(`F('file:///proc/self/nope')`);
expr(`F(new URL(${fileUrl("hello.txt")}))`);
expr(`F(new Request(${fileUrl("hello.txt")}))`);
expr(`F(${fileUrl("hello.txt")}, { method: 'POST' })`);
expr(`F(${fileUrl("hello.txt")}, { method: 'HEAD' })`);
expr(`F(${fileUrl("hello.txt")}, { method: 'DELETE' })`);
expr(`F(${fileUrl("hello.txt")}, { method: 'PUT', body: 'x' })`);
expr(`F(${fileUrl("hello.txt")}, { method: 'GET', body: 'x' })`);
expr(`F(${fileUrl("hello.txt")}, { headers: { range: 'bytes=0-1' } })`);
expr(`F(${fileUrl("hello.txt")}, { headers: { 'x-a': '1' } })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { return r.text().then(function () { return r.text() }) })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { var c = r.clone(); return Promise.all([r.text(), c.text()]) })`);
expr(`fetch(${fileUrl("hello.txt")}).then(function (r) { return r.headers.set('x', 'y') })`);
expr(`Promise.all([fetch(${fileUrl("hello.txt")}), fetch(${fileUrl("hello.txt")})]).then(function (a) { return a[0] === a[1] })`);
body(`var p = fetch(${fileUrl("hello.txt")}); var fs = require('fs'); fs.writeFileSync('hello.txt', 'changed\\n'); var r = await p; R = S(await r.text()); fs.writeFileSync('hello.txt', 'hello\\n')`);
body(`var r = await fetch(${fileUrl("hello.txt")}); var fs = require('fs'); fs.writeFileSync('hello.txt', 'changed2\\n'); R = S(await r.text()); fs.writeFileSync('hello.txt', 'hello\\n')`);
body(`var fs = require('fs'); fs.writeFileSync('late.txt', 'late'); var r = await fetch(${fileUrl("late.txt")}); fs.unlinkSync('late.txt'); R = S([r.status, await r.text().catch(function (e) { return 'ERR ' + E(e) })])`);
body(`var fs = require('fs'); var p = fetch(${fileUrl("late2.txt")}); fs.writeFileSync('late2.txt', 'created after'); R = S(await p.then(function (r) { return r.text() }, function (e) { return 'ERR ' + E(e) })); fs.unlinkSync('late2.txt')`);

// URL relativa e inválida.
expr("F('/hello.txt')");
expr("F('hello.txt')");
expr("F('./hello.txt')");
expr("F('../hello.txt')");
expr("F('//hello.txt')");
expr("F('//example.invalid/x')");
expr("F('?q=1')");
expr("F('#frag')");
expr("F('http://')");
expr("F('http://[')");
expr("F('http://a b/')");
expr("F('http://:80/')");
expr("F('http://a:99999/')");
expr("F('http://%/')");
expr("F('https://')");
expr("F('ws://example.invalid/')");
expr("F('wss://example.invalid/')");
expr("F('ftp://example.invalid/')");
expr("F('chrome://x')");
expr("F('s3://bucket/key')");
expr("F('FILE:///x')");
expr("F('data:,x ')");
expr("F('http://user:pw@')");
expr("F('http://\\u0000/')");
expr("F('1')");
expr("F('null')");
expr("F('undefined')");
expr("F('[object Object]')");
expr("F(' ')");
expr("F('https://example.invalid:abc/')");

// Request como argumento.
expr("F(new Request('data:,fromRequest'))");
expr("F(new Request('data:,x', { method: 'POST', body: 'b' }))");
expr("F(new Request('data:,x', { method: 'HEAD' }))");
expr("F(new Request('data:,x', { headers: { 'x-a': '1' } }))");
expr("F(new Request('data:,x'), { method: 'POST' })");
expr("F(new Request('data:,x', { method: 'POST', body: 'b' }), { method: 'GET' })");
expr("F(new Request('data:,x', { method: 'POST', body: 'b' }), { body: 'c' })");
expr("F(new Request('data:,x'), { body: 'c' })");
expr("F(new Request('data:,x'), { method: 'PUT', body: 'c' })");
expr("F(new Request('data:,x'), { method: 'GET', body: 'c' })");
expr("F(new Request('data:,x'), { headers: { 'x-a': '1' } })");
expr("F(new Request('data:,x'), { signal: AbortSignal.abort() })");
expr("F(new Request('data:,x', { signal: AbortSignal.abort() }))");
expr("F(new Request('data:,x', { signal: AbortSignal.abort() }), { signal: new AbortController().signal })");
expr("F(new Request('data:,x'), undefined)");
expr("F(new Request('data:,x'), null)");
expr("F(new Request('data:,x'), 5)");
body("var rq = new Request('data:,x', { method: 'POST', body: 'b' }); await rq.text(); R = S(await F(rq))");
body("var rq = new Request('data:,x', { method: 'POST', body: 'b' }); await rq.text(); R = S(await F(rq, { body: 'again' }))");
body("var rq = new Request('data:,x', { method: 'POST', body: 'b' }); var p = F(rq); R = S([await p, rq.bodyUsed])");
body("var rq = new Request('data:,x', { method: 'POST', body: 'b' }); await F(rq); R = S(await F(rq))");
body("var rq = new Request('data:,x'); var r1 = await F(rq); var r2 = await F(rq); R = S([r1, r2])");
body("var rq = new Request('data:,x', { method: 'POST', body: 'b' }); R = S(await F(rq.clone()) && rq.bodyUsed)");
body("var rq = new Request('data:,x'); R = S(await fetch(rq).then(function (r) { return r.url === rq.url }))");
body("var rq = new Request(" + fileUrl("hello.txt") + "); R = S(await F(rq))");
body("var u = URL.createObjectURL(new Blob(['x'])); var rq = new Request(u); R = S(await F(rq))");
body("var o = Object.create(Request.prototype); R = S(await F(o))");
body("var rq = new Request('data:,x'); R = S(await F({ url: rq.url }))");
body("var rq = new Request('data:,x'); R = S(await F({ __proto__: rq }))");
body("var rq = new Request('data:,x'); Object.defineProperty(rq, 'url', { value: 'data:,patched' }); R = S(await F(rq))");
body("var rq = new Request('data:,x'); var r = await fetch(rq); R = S([r.url, rq.url, r.url === rq.url])");
expr("F(new Response('data:,notrequest'))");
expr("F(new Headers())");
expr("F(new Blob(['x']))");
expr("F(new URLSearchParams('a=b'))");

// Sinal.
expr("F('data:,x', { signal: AbortSignal.abort() })");
expr("F('data:,x', { signal: AbortSignal.abort('why') })");
expr("F('data:,x', { signal: AbortSignal.abort(new RangeError('custom')) })");
expr("F('data:,x', { signal: AbortSignal.timeout(100000) })");
expr("F('data:,x', { signal: new AbortController().signal })");
expr("F('data:,x', { signal: null })");
expr("F('data:,x', { signal: undefined })");
expr("F('data:,x', { signal: {} })");
expr("F('data:,x', { signal: 5 })");
expr("F('data:,x', { signal: new EventTarget() })");
expr(`F(${fileUrl("hello.txt")}, { signal: AbortSignal.abort() })`);
expr("F('blob:abc', { signal: AbortSignal.abort() })");
expr("F('blob:abc', { signal: AbortSignal.abort('why') })");
expr("F('blob:abc', { signal: AbortSignal.abort(new RangeError('custom')) })");
expr("F('ftp://a/b', { signal: AbortSignal.abort('why') })");
expr("F('/relative', { signal: AbortSignal.abort() })");
expr("F(1, { signal: AbortSignal.abort() })");
expr("F('data:,x', { signal: AbortSignal.abort(), method: 'FOO' })");
expr("F('data:,x', { signal: AbortSignal.abort(), method: 'GET', body: 'x' })");
body("var c = new AbortController(); var p = fetch('data:,x', { signal: c.signal }); c.abort(); R = S(await p.then(function (r) { return r.status }, function (e) { return 'ERR ' + E(e) }))");
body("var c = new AbortController(); var p = fetch('data:,x', { signal: c.signal }); await null; c.abort(); R = S(await p.then(function (r) { return r.status }, function (e) { return 'ERR ' + E(e) }))");
body("var c = new AbortController(); var r = await fetch('data:,x', { signal: c.signal }); c.abort(); R = S(await r.text().catch(function (e) { return 'ERR ' + E(e) }))");
body("var c = new AbortController(); var r = await fetch('data:,x', { signal: c.signal }); R = S(await r.text()); c.abort()");
body("var c = new AbortController(); var n = 0; c.signal.addEventListener('abort', function () { n++ }); await fetch('data:,x', { signal: c.signal }); c.abort(); R = S(n)");
body("var c = new AbortController(); var p = fetch(" + fileUrl("hello.txt") + ", { signal: c.signal }); c.abort(); R = S(await p.then(function (r) { return r.status }, function (e) { return 'ERR ' + E(e) }))");
body("var s = AbortSignal.abort(); var rq = new Request('data:,x', { signal: s }); R = S(await F(rq))");
body("var c = new AbortController(); c.abort('r'); R = S(await fetch('data:,x', { signal: c.signal }).catch(function (e) { return [typeof e, e] }))");
body("var c = new AbortController(); c.abort(); R = S(await fetch('data:,x', { signal: c.signal }).catch(function (e) { return [e instanceof DOMException, e.name, e.code, e.message] }))");

// Ordem das promessas.
body("var log = []; var p = fetch('data:,x').then(function () { log.push('fetch') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }); setTimeout(function () { log.push('timer') }, 0); setImmediate(function () { log.push('immediate') }); process.nextTick(function () { log.push('tick') }); log.push('sync'); await p; await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");
body("var log = []; fetch('data:,x').then(function () { log.push('data') }); fetch('data:,y').then(function () { log.push('data2') }); fetch(" + fileUrl("hello.txt") + ").then(function () { log.push('file') }); fetch('/relative').catch(function () { log.push('relative-err') }); fetch(1).catch(function () { log.push('bad-err') }); fetch('data:,z').then(function () { log.push('data3') }); await new Promise(function (r) { setTimeout(r, 50) }); R = S(log)");
body("var log = []; fetch(1).catch(function () { log.push('err') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }).then(function () { log.push('m4') }); await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");
body("var log = []; fetch('data:,x').then(function () { log.push('data') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }).then(function () { log.push('m4') }).then(function () { log.push('m5') }); await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");
body("var log = []; fetch('data:,x').then(function () { log.push('data') }); setTimeout(function () { log.push('timer0') }, 0); setImmediate(function () { log.push('immediate') }); await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");
body("var log = []; fetch(" + fileUrl("hello.txt") + ").then(function () { log.push('file') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }); setTimeout(function () { log.push('timer0') }, 0); setImmediate(function () { log.push('immediate') }); await new Promise(function (r) { setTimeout(r, 50) }); R = S(log)");
body("var log = []; var u = URL.createObjectURL(new Blob(['x'])); fetch(u).then(function () { log.push('blob') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }); setTimeout(function () { log.push('timer0') }, 0); await new Promise(function (r) { setTimeout(r, 30) }); R = S(log)");
body("var log = []; fetch('data:,x').then(function (r) { log.push('resp'); return r.text() }).then(function () { log.push('text') }); Promise.resolve().then(function () { log.push('m1') }).then(function () { log.push('m2') }).then(function () { log.push('m3') }).then(function () { log.push('m4') }).then(function () { log.push('m5') }).then(function () { log.push('m6') }); await new Promise(function (r) { setTimeout(r, 30) }); R = S(log)");
body("var log = []; var p1 = fetch('data:,1'); var p2 = fetch('data:,2'); var p3 = fetch('data:,3'); p3.then(function () { log.push(3) }); p1.then(function () { log.push(1) }); p2.then(function () { log.push(2) }); await Promise.all([p1, p2, p3]); R = S(log)");
body("var log = []; var p = fetch('data:,x'); log.push(S(Object.keys(p))); log.push(p instanceof Promise); log.push(typeof p.then); R = S(log); await p");
body("var log = []; var a = fetch(" + fileUrl("hello.txt") + ").then(function () { log.push('file') }); var b = fetch('data:,x').then(function () { log.push('data') }); await Promise.all([a, b]); R = S(log)");
body("var log = []; var a = fetch(" + fileUrl("nope.txt") + ").catch(function () { log.push('file-err') }); var b = fetch('data:,x').then(function () { log.push('data') }); await Promise.all([a, b]); R = S(log)");
body("var log = []; var u = URL.createObjectURL(new Blob(['x'])); var a = fetch(u).then(function () { log.push('blob') }); var b = fetch('data:,x').then(function () { log.push('data') }); await Promise.all([a, b]); R = S(log)");
body("var log = []; var a = Promise.race([fetch('data:,x').then(function () { return 'data' }), new Promise(function (r) { setTimeout(function () { r('timer') }, 0) })]); R = S(await a)");
body("var log = []; var a = Promise.race([fetch(" + fileUrl("hello.txt") + ").then(function () { return 'file' }), new Promise(function (r) { setTimeout(function () { r('timer') }, 0) })]); R = S(await a)");
body("var log = []; var a = Promise.race([fetch('data:,x').then(function () { return 'data' }), new Promise(function (r) { setImmediate(function () { r('immediate') }) })]); R = S(await a)");
body("var log = []; var a = Promise.race([fetch('data:,x').then(function () { return 'data' }), new Promise(function (r) { process.nextTick(function () { r('tick') }) })]); R = S(await a)");
body("var log = []; var a = Promise.race([fetch('data:,x').then(function () { return 'data' }), Promise.resolve().then(function () { return 'm1' }).then(function () { return 'm2' }).then(function () { return 'm3' })]); R = S(await a)");
body("var log = []; fetch('data:,x').then(function (r) { log.push('resp'); r.text().then(function () { log.push('text') }); Promise.resolve().then(function () { log.push('inner-m1') }) }); await new Promise(function (r) { setTimeout(r, 30) }); R = S(log)");
body("var log = []; fetch('data:,x').then(function (r) { r.text().then(function () { log.push('t1') }); r.clone; }); fetch('data:,y').then(function (r) { r.text().then(function () { log.push('t2') }) }); await new Promise(function (r) { setTimeout(r, 30) }); R = S(log)");
body("var log = []; process.on('exit', function () {}); fetch(" + fileUrl("hello.txt") + ").then(function () { log.push('file') }); await new Promise(function (r) { setImmediate(r) }); log.push('after-immediate'); await new Promise(function (r) { setTimeout(r, 30) }); R = S(log)");
body("var log = []; var p = fetch('data:,x'); await null; log.push('after-await-null'); await p; log.push('after-p'); R = S(log)");
body("var log = []; fetch('data:,x').finally(function () { log.push('finally') }); await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");
body("var log = []; fetch(1).finally(function () { log.push('finally') }).catch(function () { log.push('caught') }); await new Promise(function (r) { setTimeout(r, 20) }); R = S(log)");

// Rejeição não tratada de fetch inválido (o processo roda até o fim e o resultado é lido antes).
body("fetch(1).catch(function () {}); R = 'handled'");

function normalize(text, dir) {
  // O UUID de `URL.createObjectURL` é aleatório: nas mensagens de erro vira `<uuid>`.
  return text.split(dir + "/").join("/app/").split(dir).join("/app").replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, "<uuid>");
}

function setup(dir) {
  fs.writeFileSync(path.join(dir, "hello.txt"), "hello\n");
  fs.writeFileSync(path.join(dir, "empty.txt"), "");
  fs.writeFileSync(path.join(dir, "data.json"), '{"a":1}');
  fs.writeFileSync(path.join(dir, "bin.dat"), Buffer.from(Array.from({ length: 256 }, (_, i) => i)));
  fs.mkdirSync(path.join(dir, "sub"));
  fs.writeFileSync(path.join(dir, "sub", "inner.txt"), "inner");
  fs.writeFileSync(path.join(dir, "no-read.txt"), "secret");
  fs.chmodSync(path.join(dir, "no-read.txt"), 0);
  fs.symlinkSync("hello.txt", path.join(dir, "link.txt"));
  fs.writeFileSync(path.join(dir, "sp ace.txt"), "spaced");
  fs.writeFileSync(path.join(dir, "ü.txt"), "uml");
}

function runCase(source) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "fo-")));
  try {
    setup(dir);
    const file = path.join(dir, "main.js");
    fs.writeFileSync(
      file,
      `var R; globalThis.require = require;\nprocess.on("exit", function () { process.stdout.write(JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R))) });\n` +
        `Promise.resolve((0, eval)(${JSON.stringify(sourceAscii)})).then(function () {}, function (e) { globalThis.R = "TOPERR " + e });\n`,
    );
    const env = { PATH: process.env.PATH, HOME: dir, NO_COLOR: "1" };
    const run = spawnSync(process.execPath, [file], { cwd: dir, env, encoding: "utf8", timeout: 20000 });
    if (run.status !== 0 && run.status !== null) throw new Error(`bun falhou (${run.status}) em: ${sourceAscii}\n${run.stderr}`);
    return { sourceAscii, result: normalize(run.stdout, dir), stderr: normalize(run.stderr, dir) };
  } finally {
    try {
      fs.chmodSync(path.join(dir, "no-read.txt"), 0o644);
    } catch {}
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

const round = () => programs.map(runCase);
const first = round();
const second = round();
for (let i = 0; i < first.length; i++) {
  if (/getaddrinfo|ENOTFOUND|ECONN|EAI_AGAIN|ETIMEDOUT/.test(first[i].result)) throw new Error(`caso faz tráfego de rede: ${first[i].sourceAscii}\n${first[i].result}`);
  if (first[i].result !== second[i].result) throw new Error(`rodadas divergem em: ${first[i].sourceAscii}\n${first[i].result}\n${second[i].result}`);
}
for (const { sourceAscii, result } of first) emitRow(JSON.stringify(sourceAscii) + "\t" + result);
process.stderr.write(`${first.length} casos\n`);
