// Gera tests/golden/fetch_types_bun.tsv: `Request` e `Response` do global medidos no bun 1.4.2, sem rede (forma: descritor
// do global, `length`, `name`, protótipo e ordem das chaves, estáticos `Response.json/error/redirect`; construtor: URL
// relativa sem base, método normalizado, `headers` de init, corpo string/Blob/URLSearchParams/FormData/ArrayBuffer/typed
// array/ReadableStream e o Content-Type implícito; getters `status`, `statusText`, `ok`, `headers`, `bodyUsed`, `url`,
// `redirected`, `type`; `clone`; `text()`, `json()`, `arrayBuffer()`, `blob()`, `formData()`, `bytes()` com `bodyUsed` e o
// erro de corpo já lido).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), lido depois do esvaziamento das promessas.
// Uso: bun scripts/gen-fetch-types-golden.js > tests/golden/fetch_types_bun.tsv
const { emitRow } = require("./golden-prelude.js");

// O boundary do multipart é aleatório a cada FormData; `S` o troca por um valor fixo para o golden ser determinístico.
const HELPER =
  "var S = function (v) { return S0(v).replace(/WebKitFormBoundary[0-9a-f]{32}/g, 'WebKitFormBoundaryX') };\n" +
  "var S0 = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
// Resultado de uma promessa: `R` só é gravado quando ela se resolve ou rejeita.
const aexpr = (code) => programs.push(HELPER + `try { Promise.resolve(${code}).then(function (v) { R = S(v) }, function (e) { R = 'rejeitou ' + E(e) }) } catch (e) { R = E(e) }`);

const U = "'http://example.com/a?b=1'";
const PAIRS = "function (h) { return Array.from(h.entries()) }";
const BYTES = "function (b) { return Array.from(new Uint8Array(b)) }";

for (const N of ["Request", "Response"]) {
  const mk = N === "Request" ? (init, body) => `new Request(${U}, ${init ? `{ ${body ? `method: 'POST', body: ${body}, ` : ""}${init} }` : body ? `{ method: 'POST', body: ${body} }` : "{}"})` : (init, body) => `new Response(${body || "undefined"}${init ? `, { ${init} }` : ""})`;
  const mkb = (body) => (N === "Request" ? `new Request(${U}, { method: 'POST', body: ${body} })` : `new Response(${body})`);

  // Forma.
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
  expr(`${N}.length`);
  expr(`${N}.name`);
  expr(`Object.getOwnPropertyNames(${N})`);
  expr(`Reflect.ownKeys(${N}.prototype).map(String)`);
  expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
  expr(`Object.getPrototypeOf(${N}) === Function.prototype`);
  expr(`${N}.prototype.constructor === ${N}`);
  expr(`Object.prototype.toString.call(${mkb("'a'")})`);
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.toStringTag))`);
  for (const m of ["text", "json", "arrayBuffer", "blob", "formData", "bytes", "clone"]) {
    expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
    expr(`${N}.prototype.${m}.call({})`);
    expr(`${N}.prototype.${m}.call(null)`);
  }
  const props = N === "Request"
    ? ["method", "url", "headers", "body", "bodyUsed", "redirect", "signal", "cache", "credentials", "destination", "integrity", "keepalive", "mode", "referrer", "referrerPolicy", "duplex"]
    : ["status", "statusText", "ok", "headers", "body", "bodyUsed", "url", "redirected", "type"];
  for (const p of props) {
    expr(`(function(d){ return d ? [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get && d.get.name, d.get && d.get.length] : 'ausente' })(Object.getOwnPropertyDescriptor(${N}.prototype, '${p}'))`);
    expr(`(function(d){ return d && d.get ? d.get.call({}) : 'ausente' })(Object.getOwnPropertyDescriptor(${N}.prototype, '${p}'))`);
  }
  expr(`(function(){ try { ${N}() } catch (e) { return [e.name, e.message, e.code] } })()`);
  expr(`(function(){ class X extends ${N} {} var x = N_(); return [x instanceof ${N}, x instanceof X, Object.getPrototypeOf(x) === X.prototype]; function N_() { return ${N === "Request" ? `new X(${U})` : "new X('a')"} } })()`);
  expr(`Object.keys(${mkb("'a'")})`);
  expr(`JSON.stringify(${mkb("'a'")})`);
  expr(`(function(){ var x = ${mkb("'a'")}; x.foo = 1; return [x.foo, Object.keys(x)] })()`);

  // Corpo e Content-Type implícito.
  const bodies = [
    ["'hello'", "string"], ["''", "string vazia"], ["new Blob(['ab'])", "Blob sem type"], ["new Blob(['ab'], { type: 'Text/X' })", "Blob com type"],
    ["new URLSearchParams('a=1&b=2')", "URLSearchParams"], ["new URLSearchParams()", "URLSearchParams vazio"],
    ["new ArrayBuffer(3)", "ArrayBuffer"], ["new Uint8Array([1, 2, 3])", "Uint8Array"], ["new Uint16Array([1, 2])", "Uint16Array"],
    ["new DataView(new ArrayBuffer(4))", "DataView"], ["new FormData()", "FormData vazio"],
    ["(function(){ var f = new FormData(); f.append('k', 'v'); return f })()", "FormData"],
    ["5", "número"], ["true", "booleano"], ["({ a: 1 })", "objeto"], ["[1, 2]", "array"], ["null", "null"], ["undefined", "undefined"],
    ["new ReadableStream()", "ReadableStream"], ["Symbol()", "símbolo"], ["1n", "bigint"],
  ];
  for (const [b, label] of bodies) {
    expr(`(function(){ var x = ${mkb(b)}; return [${JSON.stringify(label)}, x.headers.get('content-type'), x.headers.get('content-length'), x.bodyUsed, x.body === null ? 'null' : x.body.constructor.name] })()`);
    aexpr(`${mkb(b)}.text()`);
  }
  expr(`(function(){ var x = ${N === "Request" ? `new Request(${U})` : "new Response()"}; return [x.body, x.bodyUsed, x.headers.get('content-type')] })()`);
  expr(`(function(){ var x = ${mk("headers: { 'Content-Type': 'x/y' }", "'a'")}; return x.headers.get('content-type') })()`);
  expr(`(function(){ var x = ${mk("headers: [['content-type', 'x/y']]", "new Blob(['a'], { type: 'q/r' })")}; return x.headers.get('content-type') })()`);
  expr(`(function(){ var x = ${mk("headers: new Headers({ 'content-type': 'x/y' })", "new URLSearchParams('a=1')")}; return x.headers.get('content-type') })()`);
  expr(`(function(){ var x = ${mk("", "(function(){ var f = new FormData(); f.append('k', 'v'); return f })()")}; return x.headers.get('content-type').replace(/[0-9a-zA-Z]{16,}/g, 'B') })()`);
  expr(`(function(){ var a = ${mkb("(function(){ var f = new FormData(); f.append('k', 'v'); return f })()")}.headers.get('content-type'); var b = ${mkb("(function(){ var f = new FormData(); f.append('k', 'v'); return f })()")}.headers.get('content-type'); return [a === b, /^multipart\\/form-data; boundary=/.test(a)] })()`);

  // Headers de init.
  expr(`(function(){ var x = ${mk("headers: { B: '1', a: '2', 'X-Y': ' z ' }", N === "Request" ? "" : "")}; return (${PAIRS})(x.headers) })()`);
  expr(`(function(){ var x = ${mk("headers: [['B', '1'], ['a', '2'], ['b', '3']]", "")}; return (${PAIRS})(x.headers) })()`);
  expr(`(function(){ var h = new Headers({ a: '1' }); var x = ${mk("headers: h", "")}; h.append('b', '2'); return [(${PAIRS})(x.headers), x.headers === h] })()`);
  expr(`${mk("headers: 5", "")}.headers`);
  expr(`${mk("headers: [['a']]", "")}.headers`);
  expr(`${mk("headers: { 'bad name': '1' }", "")}.headers`);
  expr(`${mk("headers: null", "")}.headers instanceof Headers`);
  expr(`${mk("headers: undefined", "")}.headers instanceof Headers`);
  expr(`(function(){ var x = ${mk("", "")}; return [x.headers === x.headers, x.headers instanceof Headers] })()`);
  expr(`(function(){ var x = ${mk("", "")}; x.headers.set('a', '1'); return x.headers.get('a') })()`);
  expr(`(function(){ var x = ${mk("", "")}; x.headers = 5; return typeof x.headers })()`);
}

// Request: construtor.
expr(`new Request()`);
expr(`new Request('/relative')`);
expr(`new Request('relative')`);
expr(`new Request('')`);
expr(`new Request('//example.com/x')`);
expr(`new Request('http://')`);
expr(`new Request('http://exa mple.com')`);
expr(`new Request('data:text/plain,hi').url`);
expr(`new Request('file:///x/y').url`);
expr(`new Request('about:blank').url`);
expr(`new Request('javascript:1').url`);
expr(`new Request('ftp://h/x').url`);
expr(`new Request('HTTP://EXAMPLE.com:80/A b?c d#e').url`);
expr(`new Request('http://example.com').url`);
expr(`new Request('http://user:pw@example.com/').url`);
expr(`new Request(new URL('http://example.com/u')).url`);
expr(`new Request({ toString: function () { return 'http://example.com/o' } }).url`);
expr(`new Request({ url: 'http://example.com/o' }).url`);
expr(`new Request(5)`);
expr(`new Request(null)`);
expr(`new Request(undefined)`);
expr(`new Request(Symbol())`);
expr(`new Request('http://example.com', null).method`);
expr(`new Request('http://example.com', 5).method`);
expr(`new Request('http://example.com', 'x').method`);
expr(`new Request.length`);
expr(`new Request('http://example.com#frag').url`);
for (const m of ["get", "GET", "Get", "post", "put", "delete", "head", "options", "patch", "PATCH", "pAtCh", "connect", "CONNECT", "trace", "TRACE", "track", "TRACK", "foo", "FOO", "", "m e", "a-b", "é", "null", "undefined", "5", "Purge", "link", "propfind"]) {
  expr(`new Request('http://example.com/', { method: ${JSON.stringify(m)} }).method`);
}
expr(`new Request('http://example.com/', { method: null }).method`);
expr(`new Request('http://example.com/', { method: undefined }).method`);
expr(`new Request('http://example.com/', { method: 5 }).method`);
expr(`new Request('http://example.com/', { method: { toString: function () { return 'put' } } }).method`);
expr(`new Request('http://example.com/').method`);
expr(`new Request('http://example.com/', { method: 'GET', body: 'x' })`);
expr(`new Request('http://example.com/', { method: 'HEAD', body: 'x' })`);
expr(`new Request('http://example.com/', { body: 'x' })`);
expr(`new Request('http://example.com/', { method: 'POST', body: null }).body`);
expr(`new Request('http://example.com/', { method: 'GET', body: null }).body`);
expr(`new Request('http://example.com/', { method: 'GET', body: undefined }).body`);
expr(`new Request('http://example.com/', { method: 'GET', body: '' })`);
for (const k of ["redirect", "cache", "credentials", "mode", "referrerPolicy", "duplex"]) {
  expr(`(function(){ var r = new Request('http://example.com/'); return r.${k} })()`);
}
for (const [k, vs] of [["redirect", ["follow", "error", "manual", "bad", "FOLLOW"]], ["cache", ["default", "no-store", "reload", "no-cache", "force-cache", "only-if-cached", "bad"]],
  ["credentials", ["omit", "same-origin", "include", "bad"]], ["mode", ["cors", "no-cors", "same-origin", "navigate", "bad"]],
  ["referrerPolicy", ["", "no-referrer", "origin", "bad"]], ["referrer", ["about:client", "", "http://example.com/r", "/r", "bad"]], ["integrity", ["sha256-x", ""]],
  ["keepalive", [true, false, 1, "x"]], ["duplex", ["half", "full", "bad"]]]) {
  for (const v of vs) expr(`new Request('http://example.com/', { ${k}: ${JSON.stringify(v)} }).${k}`);
}
expr(`(function(){ var ac = new AbortController(); var r = new Request('http://example.com/', { signal: ac.signal }); return [r.signal.aborted, r.signal === ac.signal, r.signal instanceof AbortSignal] })()`);
expr(`(function(){ var r = new Request('http://example.com/'); return [r.signal.aborted, r.signal instanceof AbortSignal, r.signal === r.signal] })()`);
expr(`new Request('http://example.com/', { signal: {} })`);
expr(`new Request('http://example.com/', { signal: null }).signal instanceof AbortSignal`);
// Request a partir de Request.
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'PUT', headers: { x: '1' }, body: 'hi' }); var b = new Request(a); return [b.url, b.method, Array.from(b.headers.entries()), a.bodyUsed, b.bodyUsed, b.body === null] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'PUT', body: 'hi' }); var b = new Request(a, { method: 'POST' }); return [b.url, b.method, a.bodyUsed] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'PUT', body: 'hi' }); var b = new Request(a, { body: 'other' }); return [b.method, a.bodyUsed] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'PUT', body: 'hi' }); new Request(a); return new Request(a) })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { headers: { x: '1' } }); var b = new Request(a, { headers: { y: '2' } }); return Array.from(b.headers.entries()) })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { redirect: 'manual', cache: 'no-store' }); var b = new Request(a); return [b.redirect, b.cache] })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var b = new Request(a, { method: 'POST' }); return [a.method, b.method, b === a] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST' }); var b = new Request(a); return [b.method, b.body] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); var c = a.clone(); return [c.url, c.method, a.bodyUsed, c.bodyUsed, c === a] })()`);
aexpr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); var c = a.clone(); return Promise.all([a.text(), c.text(), a.bodyUsed, c.bodyUsed]) })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); a.text(); return a.clone() })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var c = a.clone(); return [c.body, c.bodyUsed, c.url] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { headers: { x: '1' } }); var c = a.clone(); c.headers.set('x', '2'); return [a.headers.get('x'), c.headers.get('x'), a.headers === c.headers] })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var c = a.clone(); return [a.signal === c.signal, c.signal instanceof AbortSignal] })()`);
// Request: ordem de leitura do init, signal herdado, headers vazio, redirect/cache/mode com String, defaults.
expr(`(function(){ var o = []; try { new Request({ get url() { o.push('url'); return 'http://example.com/' } }, { get body() { o.push('b') }, get signal() { o.push('s') }, get headers() { o.push('h') }, get method() { o.push('m') }, get redirect() { o.push('r') }, get cache() { o.push('c') }, get mode() { o.push('mo') }, get credentials() { o.push('cr') }, get referrer() { o.push('rf') }, get keepalive() { o.push('k') }, get duplex() { o.push('d') } }) } catch (e) { o.push('E') } return o })()`);
expr(`(function(){ var o = []; try { new Request('bad', { get body() { o.push('b') }, get method() { o.push('m') }, get headers() { o.push('h') }, get signal() { o.push('s') } }) } catch (e) { o.push('E') } return o })()`);
expr(`(function(){ var ac = new AbortController(); var a = new Request('http://example.com/a', { signal: ac.signal }); var b = new Request(a); var c = a.clone(); ac.abort(); return [b.signal === a.signal, c.signal === a.signal, b.signal.aborted] })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var b = new Request(a); return [b.signal === a.signal] })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var s = a.signal; var b = new Request(a); var c = a.clone(); return [b.signal === s, c.signal === s] })()`);
expr(`(function(){ var a = new Request('http://example.com/a'); var c = a.clone(); var s = a.signal; return [c.signal === s] })()`);
expr(`(function(){ var ac = new AbortController(); ac.abort(); var a = new Request('http://example.com/a', { signal: ac.signal }); return [new Request(a).signal.aborted, a.clone().signal.aborted] })()`);
expr(`(function(){ var ac = new AbortController(); var a = new Request('http://example.com/a', { signal: ac.signal }); return [new Request(a, { signal: null }).signal === a.signal, new Request(a, { signal: undefined }).signal === a.signal] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { headers: { x: '1' } }); return [Array.from(new Request(a, { headers: {} }).headers.entries()), Array.from(new Request(a, { headers: [] }).headers.entries()), Array.from(new Request(a, { headers: new Headers() }).headers.entries()), Array.from(new Request(a, { headers: undefined }).headers.entries()), Array.from(new Request(a, { headers: { z: '1' } }).headers.entries())] })()`);
for (const h of ["null", "5", "'x'", "true", "[['a']]", "{ 'a b': '1' }", "[['a', '1'], ['B', '2']]", "new Headers({ x: '1' })"]) expr(`Array.from(new Request('http://example.com/', { headers: ${h} }).headers.entries())`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); var b = new Request(a, { body: null }); return [b.body, b.method] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); var b = new Request(a, { method: 'GET' }); return [b.method, b.body !== null] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: 'hi' }); a.text(); var b = new Request(a); return [b.body !== null, b.bodyUsed] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { method: 'POST', body: new URLSearchParams('a=1') }); return [new Request(a).headers.get('content-type'), new Request(a, { headers: { 'content-type': 'x/y' } }).headers.get('content-type')] })()`);
expr(`(function(){ var a = new Request('http://example.com/a', { mode: 'no-cors' }); return [new Request(a, { mode: undefined }).mode, new Request(a, { mode: null }).mode] })()`);
expr(`new Request('http://example.com/', { redirect: new String('manual') }).redirect`);
expr(`new Request('http://example.com/', { redirect: { toString: function () { return 'manual' } } })`);
expr(`new Request('http://example.com/', { redirect: '' })`);
for (const k of ["credentials", "referrer", "referrerPolicy", "integrity", "keepalive", "duplex"]) expr(`new Request('http://example.com/', { ${k}: ${k === "keepalive" ? "true" : "'omit'"} }).${k}`);
expr(`Reflect.ownKeys(new Request('http://example.com/'))`);
expr(`JSON.stringify(new Request('http://example.com/'))`);
for (const u of ["http://exa\\tmple.com/a\\nb", "http://[::1", "http://example.com:99999/", "http://0x7f.1/", "http:\\\\\\\\example.com\\\\a", "  http://example.com/  ", "http://example.com/a/../b/./c", "http:example.com", "http:", "a:b", "ws://example.com", "blob:http://example.com/x", "http://exämple.com/ü?ü#ü", "http://example.com/#", "mailto:a@b.c"]) expr(`new Request('${u}').url`);
expr(`new Request({ url: 5 }).url`);
expr(`new Request({ url: null })`);
expr(`new Request({ url: '' })`);
expr(`new Request(['http://example.com/x']).url`);
expr(`new Request('http://example.com/', { method: Symbol() })`);
expr(`new Request('http://example.com/', []).method`);
expr(`new Request('http://example.com/', function () {}).method`);
expr(`new Request(new Request('http://example.com/', { method: 'put' })).method`);

// Response: construtor.
expr(`new Response().status`);
expr(`new Response().statusText`);
expr(`new Response().ok`);
expr(`new Response().type`);
expr(`new Response().url`);
expr(`new Response().redirected`);
expr(`new Response().bodyUsed`);
expr(`new Response().body`);
expr(`Array.from(new Response().headers.entries())`);
expr(`Array.from(new Response('a').headers.entries())`);
expr(`new Response(undefined, undefined).status`);
expr(`new Response(null, null).status`);
expr(`new Response('a', 5).status`);
expr(`new Response('a', 'x').status`);
expr(`new Response.length`);
for (const s of [200, 201, 204, 205, 206, 299, 300, 301, 304, 399, 400, 404, 500, 599, 100, 101, 199, 600, 999, 99, 0, -1, 1000, 200.5, "'200'", "'abc'", "null", "undefined", "NaN", "Infinity", "true", "1e2", "{ valueOf: function () { return 201 } }", "70000", "65736", "4294967496"]) {
  expr(`(function(){ var r = new Response(null, { status: ${s} }); return [r.status, r.ok, r.statusText] })()`);
}
for (const s of [204, 205, 304, 101]) expr(`new Response('body', { status: ${s} })`);
for (const t of ["'OK'", "''", "'Not Found'", "5", "null", "undefined", "'é'", "'a\\nb'", "'a\\u0100'", "{ toString: function () { return 'T' } }", "'\\u0000'", "' x '"]) {
  expr(`new Response(null, { statusText: ${t} }).statusText`);
}
expr(`new Response(null, { status: 404 }).statusText`);
expr(`new Response('a', { headers: { 'x-a': '1', 'X-B': '2' } }).headers.get('x-b')`);
expr(`Array.from(new Response('a', { headers: { 'content-type': 'text/special' } }).headers.entries())`);
expr(`Array.from(new Response('a', { headers: { 'content-length': '99' } }).headers.entries())`);
expr(`new Response({ toString: function () { return 'zz' } }).headers.get('content-type')`);
expr(`new Response('a', { headers: {} }).headers.get('content-type')`);
// Response estáticos.
expr(`Object.getOwnPropertyNames(Response).filter(function (k) { return ['json', 'error', 'redirect'].indexOf(k) >= 0 }).sort()`);
for (const m of ["json", "error", "redirect"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(Response, '${m}'))`);
}
expr(`(function(){ var r = Response.error(); return [r.type, r.status, r.statusText, r.ok, r.url, r.redirected, r.body, r.bodyUsed, Array.from(r.headers.entries())] })()`);
// Corpo vazio em stream de `Response.error()` e `Response.redirect()`, `null` em `new Response()` e `new Response(null)`.
for (const make of ["Response.error()", "Response.redirect('http://example.com/a', 302)", "new Response()", "new Response(null)"]) {
  expr(`(function(){ var r = ${make}; return [r.body === null, typeof r.body, r.bodyUsed, r.body === null ? null : r.body.locked, r.body === r.body, r.body === null ? null : r.body.constructor.name] })()`);
  aexpr(`(function(){ var r = ${make}; return r.text().then(function (t) { return [t, r.bodyUsed, r.body === null ? null : r.body.locked] }) })()`);
  aexpr(`(function(){ var r = ${make}; return r.arrayBuffer().then(function (b) { return [b.byteLength, r.bodyUsed] }) })()`);
  aexpr(`(function(){ var r = ${make}; if (r.body === null) return 'sem corpo'; var rd = r.body.getReader(); return rd.read().then(function (x) { return [x.done, x.value === undefined, r.body.locked] }) })()`);
  expr(`(function(){ var r = ${make}; var c = r.clone(); return [c.body === null, r.body === null] })()`);
}
expr(`Response.error() instanceof Response`);
expr(`new Response.error()`);
expr(`(function(){ var r = Response.error(); r.headers.set('a', '1'); return r.headers.get('a') })()`);
expr(`(function(){ var r = Response.json({ a: 1 }); return [r.status, r.statusText, r.type, r.ok, Array.from(r.headers.entries())] })()`);
aexpr(`Response.json({ a: 1, b: [1, 2] }).text()`);
aexpr(`Response.json('str').text()`);
aexpr(`Response.json(5).text()`);
aexpr(`Response.json(null).text()`);
aexpr(`Response.json(true).text()`);
aexpr(`Response.json([1, 2]).text()`);
aexpr(`Response.json({ a: 1 }, { status: 201 }).then`);
expr(`Response.json()`);
expr(`Response.json(undefined)`);
expr(`Response.json(Symbol())`);
expr(`Response.json(function () {})`);
expr(`Response.json(1n)`);
expr(`(function(){ var a = {}; a.a = a; return Response.json(a) })()`);
expr(`(function(){ var r = Response.json({ a: 1 }, { status: 201, statusText: 'Made', headers: { 'x-a': '1' } }); return [r.status, r.statusText, Array.from(r.headers.entries())] })()`);
expr(`(function(){ var r = Response.json({ a: 1 }, { headers: { 'content-type': 'text/plain' } }); return Array.from(r.headers.entries()) })()`);
expr(`(function(){ var r = Response.json({ a: 1 }, { status: 204 }); return r.status })()`);
expr(`Response.json({ a: 1 }, { status: 99 })`);
aexpr(`Response.json({ toJSON: function () { return 'tj' } }).text()`);
aexpr(`Response.json({ a: undefined, b: 1 }).text()`);
aexpr(`Response.json('\\u2028é').text()`);
expr(`Response.json.length`);
expr(`(function(){ var r = Response.json({ a: 1 }); return [r.bodyUsed, r.body.constructor.name] })()`);
for (const [u, st] of [["'http://example.com/x'", ""], ["'/relative'", ""], ["'relative'", ""], ["''", ""], ["'http://example.com/x'", "301"], ["'http://example.com/x'", "302"], ["'http://example.com/x'", "303"],
  ["'http://example.com/x'", "307"], ["'http://example.com/x'", "308"], ["'http://example.com/x'", "300"], ["'http://example.com/x'", "200"], ["'http://example.com/x'", "304"], ["'http://example.com/x'", "'301'"],
  ["'http://example.com/x'", "undefined"], ["'http://example.com/x'", "null"], ["'http://example.com/x'", "306"], ["'http://example.com/x'", "500"], ["new URL('http://example.com/u')", ""],
  ["'http://exa mple.com'", ""], ["'HTTP://EXAMPLE.com/A b'", ""], ["'data:text/plain,x'", ""], ["5", ""], ["undefined", ""], ["null", ""]]) {
  expr(`(function(){ var r = Response.redirect(${u}${st ? ", " + st : ""}); return [r.status, r.statusText, r.type, r.ok, r.redirected, r.url, r.body, Array.from(r.headers.entries())] })()`);
}
expr(`Response.redirect()`);
expr(`Response.redirect.length`);
expr(`Response.redirect('http://example.com/x', { status: 301 })`);
expr(`Response.redirect('http://example.com/x', { status: 301 }).status`);
expr(`Response.redirect('http://example.com/x', { headers: { a: '1' } }).headers.get('a')`);
expr(`(function(){ var r = Response.redirect('http://example.com/x', 301); r.headers.set('a', '1'); return Array.from(r.headers.entries()) })()`);

// Getters e brand check.
expr(`(function(){ var r = new Response('a', { status: 201, statusText: 'Cr', headers: { x: '1' } }); return [r.status, r.statusText, r.ok, r.type, r.url, r.redirected, r.bodyUsed] })()`);
expr(`(function(){ var r = new Response('a'); try { r.status = 500 } catch (e) { return 'throw' } return r.status })()`);
expr(`(function(){ 'use strict'; var r = new Response('a'); try { r.status = 500 } catch (e) { return [e.name, e.message] } })()`);
expr(`(function(){ 'use strict'; var r = new Response('a'); try { r.ok = false } catch (e) { return [e.name, e.message] } })()`);
expr(`(function(){ 'use strict'; var r = new Request('http://example.com/'); try { r.method = 'POST' } catch (e) { return [e.name, e.message] } })()`);
expr(`Object.getOwnPropertyDescriptor(Response.prototype, 'status').get.call(new Request('http://example.com/'))`);
expr(`Object.getOwnPropertyDescriptor(Request.prototype, 'method').get.call(new Response())`);
expr(`Response.prototype.clone.call(new Request('http://example.com/'))`);
expr(`Request.prototype.text.call(new Response())`);
expr(`new Response('a') instanceof Request`);
expr(`new Request('http://example.com/') instanceof Response`);
expr(`(function(){ var r = new Response('a', { status: 201 }); var c = r.clone(); return [c.status, c.statusText, c.ok, c.type, c.url, r.bodyUsed, c.bodyUsed, c === r, c.headers === r.headers, c instanceof Response] })()`);
expr(`(function(){ var r = new Response(); var c = r.clone(); return [c.body, c.bodyUsed] })()`);
expr(`(function(){ var r = Response.error(); var c = r.clone(); return [c.type, c.status] })()`);
expr(`(function(){ var r = new Response('a'); r.text(); return r.clone() })()`);
expr(`(function(){ var r = new Response('a', { headers: { x: '1' } }); var c = r.clone(); c.headers.set('x', '2'); return [r.headers.get('x'), c.headers.get('x')] })()`);
aexpr(`(function(){ var r = new Response('hello'); var c = r.clone(); return Promise.all([r.text(), c.text(), r.bodyUsed, c.bodyUsed]) })()`);
aexpr(`(function(){ var r = new Response('hello'); var c = r.clone(); var d = c.clone(); return Promise.all([r.text(), c.text(), d.text()]) })()`);
aexpr(`(function(){ var r = new Response('hello'); var c = r.clone(); return c.text().then(function (t) { return [t, r.bodyUsed, c.bodyUsed, r.text()] }) })()`);
aexpr(`(function(){ var r = new Response('hello'); var c = r.clone(); return c.text().then(function () { return r.text() }) })()`);
expr(`(function(){ var r = new Response('a'); var c = r.clone(); return [r.body === c.body, r.body.locked, c.body.locked] })()`);

// Leitura do corpo.
for (const N of ["Response", "Request"]) {
  const mkb = (body) => (N === "Request" ? `new Request('http://example.com/', { method: 'POST', body: ${body} })` : `new Response(${body})`);
  const none = N === "Request" ? "new Request('http://example.com/')" : "new Response()";
  aexpr(`${mkb("'hello'")}.text()`);
  aexpr(`${mkb("'\\u4f60\\ud83d\\ude00 é'")}.text()`);
  aexpr(`${mkb("new Uint8Array([0xff, 0xfe, 0x41])")}.text()`);
  aexpr(`${mkb("new Uint8Array([0xef, 0xbb, 0xbf, 0x41])")}.text()`);
  aexpr(`${mkb("''")}.text()`);
  aexpr(`${none}.text()`);
  aexpr(`${mkb("'{\"a\":1}'")}.json()`);
  aexpr(`${mkb("'x'")}.json()`);
  aexpr(`${mkb("''")}.json()`);
  aexpr(`${none}.json()`);
  aexpr(`${mkb("'null'")}.json()`);
  aexpr(`${mkb("'\\ufeff[1]'")}.json()`);
  aexpr(`${mkb("'abc'")}.arrayBuffer().then(function (b) { return [b.constructor.name, b.byteLength, (${BYTES})(b)] })`);
  aexpr(`${none}.arrayBuffer().then(function (b) { return [b.constructor.name, b.byteLength] })`);
  aexpr(`${mkb("new Uint8Array([1, 2, 3]).subarray(1)")}.arrayBuffer().then(function (b) { return (${BYTES})(b) })`);
  aexpr(`${mkb("'abc'")}.bytes().then(function (b) { return [b.constructor.name, b.length, b.byteOffset, Array.from(b)] })`);
  aexpr(`${none}.bytes().then(function (b) { return [b.constructor.name, b.length] })`);
  aexpr(`${mkb("'abc'")}.blob().then(function (b) { return [b.constructor.name, b.size, b.type] })`);
  aexpr(`${mkb("new Blob(['abc'], { type: 'a/b' })")}.blob().then(function (b) { return [b.size, b.type] })`);
  aexpr(`${mkb("new URLSearchParams('a=1')")}.blob().then(function (b) { return [b.size, b.type] })`);
  aexpr(`new ${N}(${N === "Request" ? "'http://example.com/', { method: 'POST', headers: { 'content-type': 'q/r' }, body: 'abc' }" : "'abc', { headers: { 'content-type': 'q/r' } }"}).blob().then(function (b) { return [b.size, b.type] })`);
  aexpr(`${none}.blob().then(function (b) { return [b.size, b.type] })`);
  aexpr(`${mkb("new Blob(['abc'])")}.text()`);
  aexpr(`${mkb("new URLSearchParams('a=1&b=%C3%A9')")}.text()`);
  aexpr(`${mkb("new URLSearchParams('a=1&b=2')")}.formData().then(function (f) { return [f.constructor.name, Array.from(f.entries())] })`);
  aexpr(`${mkb("(function(){ var f = new FormData(); f.append('k', 'v'); f.append('k', 'w'); return f })()")}.formData().then(function (f) { return [f.constructor.name, Array.from(f.entries())] })`);
  aexpr(`${mkb("(function(){ var f = new FormData(); f.append('k', new Blob(['xx'], { type: 'a/b' }), 'n.txt'); return f })()")}.formData().then(function (f) { var v = f.get('k'); return [v.constructor.name, v.name, v.size, v.type] })`);
  aexpr(`${mkb("'a=1&b=2'")}.formData()`);
  aexpr(`${none}.formData()`);
  aexpr(`new ${N}(${N === "Request" ? "'http://example.com/', { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: 'a=1&b=%C3%A9&a=2' }" : "'a=1&b=%C3%A9&a=2', { headers: { 'content-type': 'application/x-www-form-urlencoded' } }"}).formData().then(function (f) { return Array.from(f.entries()) })`);
  aexpr(`new ${N}(${N === "Request" ? "'http://example.com/', { method: 'POST', headers: { 'content-type': 'multipart/form-data; boundary=zz' }, body: '--zz\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--zz--\\r\\n' }" : "'--zz\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--zz--\\r\\n', { headers: { 'content-type': 'multipart/form-data; boundary=zz' } }"}).formData().then(function (f) { return Array.from(f.entries()) })`);
  aexpr(`new ${N}(${N === "Request" ? "'http://example.com/', { method: 'POST', headers: { 'content-type': 'multipart/form-data' }, body: 'x' }" : "'x', { headers: { 'content-type': 'multipart/form-data' } }"}).formData()`);
  aexpr(`new ${N}(${N === "Request" ? "'http://example.com/', { method: 'POST', headers: { 'content-type': 'text/plain' }, body: 'x' }" : "'x', { headers: { 'content-type': 'text/plain' } }"}).formData()`);
  // bodyUsed e corpo já lido.
  for (const m of ["text", "json", "arrayBuffer", "blob", "formData", "bytes"]) {
    const body = m === "json" ? "'{\"a\":1}'" : m === "formData" ? "new URLSearchParams('a=1')" : "'abc'";
    expr(`(function(){ var x = ${mkb(body)}; var p = x.${m}(); p.catch(function () {}); return [x.bodyUsed, p instanceof Promise] })()`);
    aexpr(`(function(){ var x = ${mkb(body)}; var p = x.${m}(); return p.then(function () { return [x.bodyUsed, x.body === null ? 'null' : x.body.locked] }) })()`);
    aexpr(`(function(){ var x = ${mkb(body)}; return x.${m}().then(function () { return x.${m}() }) })()`);
    aexpr(`(function(){ var x = ${mkb(body)}; x.${m}(); return x.${m}() })()`);
    aexpr(`(function(){ var x = ${mkb(body)}; x.${m}(); return Promise.resolve().then(function () { return x.text() }) })()`);
    expr(`(function(){ var x = ${mkb(body)}; x.${m}(); try { x.clone() } catch (e) { return E(e) } return 'ok' })()`);
    aexpr(`(function(){ var x = ${none}; return x.${m}().then(function () { return x.bodyUsed }) })()`);
    aexpr(`(function(){ var x = ${none}; return x.${m}().then(function () { return x.${m}() }) })()`);
  }
  aexpr(`(function(){ var x = ${mkb("'abc'")}; var r = x.body.getReader(); return r.read().then(function (c) { return [x.bodyUsed, c.done, c.value && c.value.constructor.name, c.value && c.value.length] }) })()`);
  aexpr(`(function(){ var x = ${mkb("'abc'")}; x.body.getReader(); return x.text() })()`);
  aexpr(`(function(){ var x = ${mkb("'abc'")}; var r = x.body.getReader(); return r.read().then(function () { return x.text() }) })()`);
  aexpr(`(function(){ var x = ${mkb("'abc'")}; return x.text().then(function () { return x.body.locked }) })()`);
  expr(`(function(){ var x = ${mkb("'abc'")}; return [x.body === x.body, x.body.locked, x.body instanceof ReadableStream] })()`);
  aexpr(`(function(){ var x = ${mkb("new ReadableStream({ start: function (c) { c.enqueue(new TextEncoder().encode('st')); c.enqueue(new TextEncoder().encode('ream')); c.close() } })")}; return x.text() })()`);
  aexpr(`(function(){ var x = ${mkb("new ReadableStream({ start: function (c) { c.enqueue('str'); c.close() } })")}; return x.text() })()`);
  aexpr(`(function(){ var s = new ReadableStream({ start: function (c) { c.enqueue(new Uint8Array([65])); c.close() } }); var x = ${mkb("s")}; return [x.body === s, x.bodyUsed, s.locked] })()`);
  aexpr(`(function(){ var s = new ReadableStream({ start: function (c) { c.enqueue(new Uint8Array([65])); c.close() } }); s.getReader(); return ${mkb("s")} })()`);
  aexpr(`(function(){ var s = new ReadableStream({ start: function (c) { c.error(new RangeError('boom')) } }); return ${mkb("s")}.text() })()`);
  // Propriedades depois da leitura.
  aexpr(`(function(){ var x = ${mkb("'abc'")}; return x.text().then(function () { return [x.headers.get('content-type'), x.status, x.url] }) })()`);
  // Corpo consumido não muda o resto.
  expr(`(function(){ var x = ${mkb("'abc'")}; x.bodyUsed = true; return x.bodyUsed })()`);
  // Mudança de corpo por cabeçalho.
  aexpr(`(function(){ var x = ${mkb("'abc'")}; x.headers.set('content-type', 'application/json'); return x.text() })()`);
  aexpr(`(function(){ var x = ${mkb("'{\"a\":2}'")}; x.headers.set('content-type', 'text/plain'); return x.json() })()`);
}

// Request com URL: getters.
expr(`(function(){ var r = new Request('http://example.com/a?b=1#h', { method: 'put' }); return [r.method, r.url, r.bodyUsed, r.body, r.redirect, r.cache, r.credentials, r.destination, r.integrity, r.keepalive, r.mode, r.referrer, r.referrerPolicy, r.duplex] })()`);
expr(`new Request('http://example.com/').destination`);
expr(`new Request('http://example.com/', { destination: 'image' }).destination`);
expr(`new Request('http://example.com/').referrer`);
expr(`new Request('http://example.com/').headers.get('host')`);
expr(`Array.from(new Request('http://example.com/').headers.entries())`);
expr(`Array.from(new Request('http://example.com/', { method: 'POST', body: 'x' }).headers.entries())`);
expr(`Array.from(new Request('http://example.com/', { method: 'POST', body: new Blob(['x'], { type: 'a/b' }) }).headers.entries())`);
expr(`Array.from(new Request('http://example.com/', { method: 'POST', body: new URLSearchParams('a=1') }).headers.entries())`);
expr(`Array.from(new Response(new Uint8Array(2)).headers.entries())`);
expr(`Array.from(new Response(new ArrayBuffer(2)).headers.entries())`);
expr(`Array.from(new Response(new Blob(['x'], { type: 'a/b' })).headers.entries())`);
expr(`Array.from(new Response(new URLSearchParams('a=1')).headers.entries())`);
expr(`Array.from(new Response('x').headers.entries())`);
expr(`Array.from(new Response(5).headers.entries())`);
expr(`Array.from(new Response({}).headers.entries())`);
aexpr(`new Response({}).text()`);
aexpr(`new Response(5).text()`);
aexpr(`new Response([1, 2]).text()`);
aexpr(`new Response(null).text()`);
aexpr(`new Response(undefined).text()`);
aexpr(`new Response(true).text()`);
aexpr(`new Response(new Date(0)).text().then(function (t) { return t.slice(0, 3) })`);
expr(`new Response(Symbol())`);
expr(`new Response(1n)`);
aexpr(`new Response(new URL('http://example.com/u')).text()`);
aexpr(`new Response(new Headers({ a: '1' })).text()`);
aexpr(`new Response(new TextEncoder().encode('é')).text()`);
aexpr(`new Response(new Float32Array([1])).arrayBuffer().then(function (b) { return b.byteLength })`);
aexpr(`new Response(new DataView(new ArrayBuffer(4), 1, 2)).arrayBuffer().then(function (b) { return b.byteLength })`);
aexpr(`new Response(new SharedArrayBuffer(2)).arrayBuffer().then(function (b) { return [b.constructor.name, b.byteLength] })`);
aexpr(`new Response(new Uint8Array(new ArrayBuffer(8), 2, 3)).arrayBuffer().then(function (b) { return b.byteLength })`);
expr(`(function(){ var u = new Uint8Array([1, 2]); var r = new Response(u); u[0] = 9; return r.arrayBuffer().then ? 'p' : 'n' })()`);
aexpr(`(function(){ var u = new Uint8Array([1, 2]); var r = new Response(u); u[0] = 9; return r.bytes().then(function (b) { return Array.from(b) }) })()`);
aexpr(`(function(){ var s = 'ab'; var r = new Response({ toString: function () { return s } }); s = 'cd'; return r.text() })()`);

// formData() de multipart malformado: TypeError ERR_FORMDATA_PARSE_ERROR (corpo vazio inclusive).
for (const N of ["Response", "Request"]) {
  const mkm = (type, body) => (N === "Request" ? `new Request('http://example.com/', { method: 'POST', headers: { 'content-type': ${JSON.stringify(type)} }, body: ${JSON.stringify(body)} })` : `new Response(${JSON.stringify(body)}, { headers: { 'content-type': ${JSON.stringify(type)} } })`);
  const MP = "multipart/form-data; boundary=x";
  for (const body of ["garbage", "--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv", "", "--x\r\nContent-Disposition: form-data; name=\"a\"\r\n--x--\r\n", "--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--x--\r\n", "--x--\r\n", "--y\r\n\r\n--y--"]) {
    aexpr(`${mkm(MP, body)}.formData().then(function (f) { return Array.from(f.entries()) })`);
  }
  aexpr(`${mkm("multipart/form-data; boundary=" + "a".repeat(80), "x")}.formData()`);
  aexpr(`${mkm("application/x-www-form-urlencoded", "")}.formData().then(function (f) { return Array.from(f.entries()) })`);
}

// Auditoria de Response: status 101 vira type "error"; 204/205/304 com corpo não lançam; redirect com init.
for (const status of [101, 100, 199, 204, 205, 304, 600]) {
  expr(`(function(){ var r = new Response('a', { status: ${status} }); return r.status + ',' + r.type + ',' + r.ok })()`);
}
expr(`(function(){ var r = Response.redirect('http://a.com/x', { status: 307 }); return r.status + ',' + r.headers.get('location') })()`);
expr(`Response.redirect('http://a.com/x', 200)`);
expr(`Response.json(undefined)`);
expr(`(function(){ var r = new Response(new Blob(['a'])); return String(r.headers.get('content-type')) })()`);

// Request com corpo: Content-Type implícito por tipo de corpo, leituras, bodyUsed, new Request(r) com corpo usado.
{
  const RU = `'http://example.com/a'`;
  const post = (body) => `new Request(${RU}, { method: 'POST', body: ${body} })`;
  const types = [
    `'hi'`, `new Blob(['x'], { type: 'text/x' })`, `new Blob(['x'])`, `new URLSearchParams('a=1')`, `new ArrayBuffer(3)`,
    `new Uint8Array(3)`, `5`, `({ a: 1 })`, `''`,
  ];
  for (const body of types) expr(`(function(){ var r = ${post(body)}; return [r.headers.get('content-type'), r.bodyUsed, r.body === null] })()`);
  expr(`(function(){ var f = new FormData(); f.append('a', '1'); var r = ${post("f")}; return r.headers.get('content-type').replace(/[0-9a-zA-Z-]{20,}/, 'B') })()`);
  expr(`(function(){ var r = ${post("new ReadableStream({ start(c) { c.enqueue(new Uint8Array([104])); c.close() } })")}; return [r.headers.get('content-type'), r.bodyUsed] })()`);
  aexpr(`${post("new ReadableStream({ start(c) { c.enqueue(new Uint8Array([104])); c.close() } })")}.text()`);
  expr(`${RU} && new Request(${RU}, { method: 'POST', body: 'x', headers: { 'content-type': 'a/b' } }).headers.get('content-type')`);
  const mk = post(`'{"a":1}'`);
  aexpr(`${mk}.json()`);
  aexpr(`${mk}.bytes().then(function (b) { return [b.constructor.name, b.length] })`);
  aexpr(`${mk}.arrayBuffer().then(function (b) { return b.byteLength })`);
  aexpr(`${mk}.blob().then(function (b) { return [b.size, b.type] })`);
  aexpr(`${post("new URLSearchParams('a=1&b=2')")}.formData().then(function (f) { return [f.get('a'), f.get('b')] })`);
  aexpr(`${mk}.formData()`);
  aexpr(`new Request(${RU}, { method: 'POST', body: 'a=1', headers: { 'content-type': 'application/x-www-form-urlencoded' } }).formData().then(function (f) { return f.get('a') })`);
  aexpr(`(function(){ var f = new FormData(); f.append('a', '1'); return ${post("f")}.formData().then(function (g) { return g.get('a') }) })()`);
  aexpr(`(function(){ var f = new FormData(); f.append('a', '1'); return ${post("f")}.blob().then(function (b) { return b.type.replace(/[0-9a-zA-Z-]{20,}/, 'B') }) })()`);
  aexpr(`${post("new URLSearchParams('a=1')")}.blob().then(function (b) { return b.type })`);
  aexpr(`${post("'x'")}.blob().then(function (b) { return b.type })`);
  aexpr(`new Request(${RU}, { method: 'POST', body: 'x', headers: { 'content-type': 'a/b' } }).blob().then(function (b) { return b.type })`);
  aexpr(`${post("'x'")}.json()`);
  aexpr(`${post("''")}.json()`);
  aexpr(`(function(){ var r = ${mk}; var a = r.bodyUsed; return r.text().then(function (t) { return [a, r.bodyUsed, t] }) })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { return r.text() }) })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { return r.json() }) })()`);
  aexpr(`(function(){ var r = ${mk}; r.body.getReader(); return r.text() })()`);
  expr(`(function(){ var r = ${mk}; r.body.getReader(); return r.bodyUsed })()`);
  // clone com corpo não lido: os dois leem; clone de corpo lido lança.
  aexpr(`(function(){ var r = ${mk}; var c = r.clone(); return Promise.all([r.text(), c.text(), r.bodyUsed, c.bodyUsed]) })()`);
  aexpr(`(function(){ var r = ${mk}; var c = r.clone(); return r.text().then(function () { return c.text() }) })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { return r.clone() }) })()`);
  aexpr(`(function(){ var r = ${post("new ReadableStream({ start(c) { c.enqueue(new Uint8Array([104])); c.close() } })")}; var c = r.clone(); return Promise.all([r.text(), c.text()]) })()`);
  aexpr(`${post("new URLSearchParams('a=1')")}.clone().headers.get('content-type')`);
  // new Request(r): não consome o corpo de r; com r já lido, não lança e o novo corpo sai vazio.
  expr(`(function(){ var r = ${mk}; var n = new Request(r); return [r.bodyUsed, n.bodyUsed, r.body === null, n.body === null] })()`);
  aexpr(`(function(){ var r = ${mk}; var n = new Request(r); return n.text().then(function (t) { return [t, r.bodyUsed] }) })()`);
  aexpr(`(function(){ var r = ${mk}; var n = new Request(r); return r.text() })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { return new Request(r) }) })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { var n = new Request(r); return n.text().then(function (t) { return [t, n.bodyUsed, r.bodyUsed] }) }) })()`);
  expr(`(function(){ var r = ${mk}; return r.text().then(function () { return 0 }), r.bodyUsed })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { var n = new Request(r); return [n.body === null, n.bodyUsed] }) })()`);
  aexpr(`(function(){ var r = ${mk}; return r.text().then(function () { return new Request(r, { body: 'z' }).text() }) })()`);
  expr(`(function(){ var r = ${mk}; var n = new Request(r, { method: 'PUT' }); return [n.method, r.bodyUsed] })()`);
  expr(`(function(){ var n = new Request(new Request(${RU})); return [n.body === null] })()`);
  expr(`(function(){ var r = new Request(${RU}, { method: 'POST', body: new ReadableStream({ start(c) { c.close() } }) }); var n = new Request(r); return [r.bodyUsed, n.bodyUsed, r.body.locked] })()`);
  aexpr(`(function(){ var r = ${post("new ReadableStream({ start(c) { c.enqueue(new Uint8Array([104])); c.close() } })")}; var n = new Request(r); return n.text().then(function (t) { return [t, r.bodyUsed] }) })()`);
  // GET/HEAD com corpo e sem corpo.
  expr(`(function(){ try { new Request(${RU}, { body: 'x' }); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()`);
  expr(`(function(){ try { new Request(${RU}, { method: 'HEAD', body: 'x' }); return 'ok' } catch (e) { return e.name + ': ' + e.message } })()`);
  expr(`(function(){ var r = new Request(${RU}, { body: null }); return [r.body === null, r.bodyUsed] })()`);
  expr(`(function(){ var r = new Request(${RU}, { method: 'POST' }); return [r.body === null, r.bodyUsed] })()`);
  aexpr(`new Request(${RU}, { method: 'POST' }).text()`);
  aexpr(`new Request(${RU}).text()`);
  aexpr(`new Request(${RU}).arrayBuffer().then(function (b) { return b.byteLength })`);
  expr(`(function(){ var r = new Request(${RU}, { method: 'POST', body: '' }); return [r.body === null, r.headers.get('content-type')] })()`);
  expr(`(function(){ var r = ${mk}; return [r.body.constructor.name, r.body.locked, r.body === r.body] })()`);
}

// `input` objeto: lê `url` (qualquer valor presente, convertido a string); sem `url`, só um `toString` próprio vale.
for (const input of [
  `{}`, `{ url: 'http://a/' }`, `{ url: 5 }`, `{ toString() { return 'http://a/' } }`, `{ href: 'http://a/' }`, `new URL('http://a/')`,
  `{ get url() { throw new Error('boom') } }`, `new Proxy({}, {})`, `new Proxy({ url: 'http://a/' }, {})`,
  `{ url: null }`, `{ url: '' }`, `{ url: '', toString() { return 'http://b/' } }`, `{ url: 'http://a/', toString() { return 'http://b/' } }`,
  `Object.create({ toString() { return 'http://a/' } })`, `Object.create({ url: 'http://a/' })`, `[]`, `[1]`,
  `{ toString() { return '' } }`, `{ toString() { throw new Error('ts') } }`, `{ toString: 5 }`,
]) {
  expr(`(function(){ var r = new Request(${input}); return r.url })()`);
}

(async () => {
  for (const source of programs) {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 40; i++) await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
  }
})();
