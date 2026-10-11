// Gera tests/golden/url_bun.tsv: `URL` do global medido no bun 1.4.2 (descritor do global, `length`, `name`, chaves do
// construtor e do protótipo com getters e setters na ordem, estáticos canParse/parse/createObjectURL/revokeObjectURL,
// construtor com base, erros exatos, todos os getters, toString/toJSON e uma grade de parsing: esquemas especiais e não
// especiais, IPv4, IPv6, percent-encoding, IDNA básico, `..` e porta padrão).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-url-golden.js > tests/golden/url_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

const N = "URL";
const GETTERS = ["href", "origin", "protocol", "username", "password", "host", "hostname", "port", "pathname", "search", "searchParams", "hash"];

// Forma.
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
expr(`${N}.length`);
expr(`${N}.name`);
expr(`Object.getOwnPropertyNames(${N})`);
expr(`Object.getOwnPropertySymbols(${N}).map(String)`);
expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
expr(`${N}.prototype.constructor === ${N}`);
expr(`Object.getOwnPropertyNames(${N}.prototype)`);
expr(`Object.getOwnPropertySymbols(${N}.prototype).map(String)`);
expr(`Object.prototype.toString.call(new ${N}('http://a/'))`);
expr(`Object.keys(new ${N}('http://a/'))`);
expr(`Object.keys(${N}.prototype)`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.toStringTag))`);
for (const g of GETTERS) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.set && d.set.name, d.set && d.set.length] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${g}'))`);
}
for (const m of ["toString", "toJSON"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name, Object.getOwnPropertyNames(d.value)] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
}
for (const m of ["canParse", "parse", "createObjectURL", "revokeObjectURL"]) {
  expr(`(function(d){ return [typeof d, d && d.enumerable, d && d.writable, d && d.configurable, d && d.value.length, d && d.value.name] })(Object.getOwnPropertyDescriptor(${N}, '${m}'))`);
}
// Construtor e erros.
expr(`(function(){ try { ${N}('http://a/') } catch (e) { return [e.name, e.message, e.code] } })()`);
expr(`new ${N}()`);
for (const bad of ["''", "'a'", "'//a'", "'http://'", "'http://a b'", "'http://[::1'", "'http://a:99999'", "'http://a:b'", "'ht tp://a'", "undefined", "null", "5", "{}", "Symbol()", "'  '", "'http://%'", "'http://a%zz/'"]) {
  expr(`new ${N}(${bad})`);
  expr(`${N}.canParse(${bad})`);
  expr(`${N}.parse(${bad})`);
}
expr(`${N}.canParse()`);
expr(`${N}.parse()`);
expr(`(function(){ try { ${N}.createObjectURL() } catch (e) { return [e.name, e.message, e.code] } })()`);
expr(`${N}.revokeObjectURL('x')`);
expr(`new ${N}({ toString() { return 'http://obj/' } }).href`);
expr(`new (class X extends ${N} {})('http://a/b').pathname`);
expr(`Reflect.construct(${N}, ['http://a/'], Object).constructor === Object`);
for (const m of GETTERS) {
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${m}').get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${m}').get.call(null)`);
}
expr(`${N}.prototype.toString.call({})`);
expr(`${N}.prototype.toJSON.call({})`);
// Base.
for (const [u, b] of [["b", "http://x/a/c"], ["../b", "http://x/a/c/d"], ["/b", "http://x/a/c"], ["//y/b", "http://x/a"], ["?q", "http://x/a?z#h"], ["#f", "http://x/a?z#h"], ["", "http://x/a?z#h"],
  ["http://z/", "http://x/"], ["b", "mailto:a@b"], ["b", "data:text/plain,hi"], ["#f", "mailto:a@b"], ["b", "x"], ["b", "file:///a/c"], ["b", "foo://h/a/c"], ["b", "foo:/a/c"], ["c:/d", "file:///a/b"], ["//h/p", "file:///a/b"]]) {
  expr(`new ${N}(${JSON.stringify(u)}, ${JSON.stringify(b)}).href`);
  expr(`${N}.canParse(${JSON.stringify(u)}, ${JSON.stringify(b)})`);
}
expr(`new ${N}('b', new ${N}('http://x/a/c')).href`);
expr(`new ${N}('http://a/', undefined).href`);
expr(`new ${N}('b', null)`);
expr(`new ${N}('b', 'nope')`);
// Grade de parsing.
const GRID = [
  "http://example.com", "HTTP://EXAMPLE.COM/A", "http://user:pass@host:8080/p/a/t/h?query=string#hash", "http://user@host/", "http://:pass@host/", "http://@host/",
  "http://host:80/", "http://host:443/", "https://host:443/", "https://host:80/", "ws://h:80/", "wss://h:443/", "ftp://h:21/", "ftp://h:22/", "http://host:0/", "http://host:00080/", "http://host:65535/",
  "http://host:65536/", "http://host:/", "http://host:80x/", "file:///a/b", "file://host/a/b", "file:///C:/a", "file://localhost/a", "file:a", "file:/a", "file:",
  "http://a/b/../c", "http://a/b/./c", "http://a/b/%2e%2e/c", "http://a/b/%2E/c", "http://a/..", "http://a/../../b", "http://a//b//c", "http://a/b/", "http://a/b/..", "http://a/b/.",
  "http://a\\b\\c", "http:\\\\a\\b", "http:a", "http:/a", "http:///a", "http:////a", "https:example.org", "http://a/ b", "http://a/b c?d e#f g", "http://a/\"<>`{}", "http://a/?\"<>`{}'", "http://a/#\"<>`{}",
  "http://a/%41%zz%", "http://a/é", "http://a/?é#é", "http://a/\u{1F600}", "http://é.com/", "http://EXAMPLE.CÓM/", "http://bücher.de/", "http://xn--bcher-kva.de/", "http://日本語.jp/", "http://a.b.c./", "http://a..b/",
  "http://ａ.com/", "http://a\u00ad.com/", "http://ß.de/", "http://ǅ.com/",
  "http://1.2.3.4/", "http://1.2.3/", "http://1.2/", "http://1/", "http://0x7f.1/", "http://0300.0250.0.1/", "http://4294967295/", "http://4294967296/", "http://256.1.1.1/", "http://1.2.3.4.5/", "http://0x/", "http://1.2.3.4./",
  "http://127.1/", "http://010.1.1.1/", "http://a.1/", "http://1.a/",
  "http://[::1]/", "http://[::1]:8080/", "http://[2001:db8::1]/", "http://[2001:DB8:0:0:0:0:0:1]/", "http://[::ffff:1.2.3.4]/", "http://[1:2:3:4:5:6:7:8]/", "http://[1:2:3:4:5:6:7:8:9]/", "http://[::]/", "http://[1::]/", "http://[1:0:0:2:0:0:0:3]/", "http://[::1", "http://[:1]/", "http://[g::1]/", "http://[::1]x/",
  "http://a%20b/", "http://a%2fb/", "http://a b/", "http://a<b/", "http://a^b/", "http://a|b/", "http://a%/", "http://a_b/", "http://-a/", "http:///", "http://?", "http://#", "http://a?", "http://a#", "http://a?#",
  "javascript:alert(1)", "data:text/plain,hello world", "mailto:a@b.com?subject=x", "about:blank", "blob:http://a/uuid", "foo://Host/Path?Q#H", "foo:/a/../b", "foo://h:80/", "foo:bar", "foo:", "foo://", "foo:///", "foo://a b/",
  "tel:+1 2", "a+b-c.d://h/", "1a://h/", "a b://h/", "://h/", "urn:isbn:123", "http://a/\t\n\rb", "  http://a/  ", "\u0001http://a/\u0001", "http://a/b?c?d#e#f", "http://a/b#?c", "http://a/b?c#d?e",
  "http://u:p@h:80@h2/", "http://a@b@c/", "http://u:p:q@h/", "http://u%40:p%3a@h/", "http://üser:pässword@h/", "http://a:b@/", "ssh://git@host:22/repo", "ws://h/p?q", "wss://h", "HTTPS://H:443", "file://h/a", "file:///a/../b", "file:///a%2fb",
];
for (const g of GRID) {
  const lit = JSON.stringify(g);
  expr(`(function(){ var u = new ${N}(${lit}); return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash, String(u), u.toJSON(), JSON.stringify(u), u.searchParams.toString()] })()`);
}
// searchParams.
expr(`(function(){ var u = new ${N}('http://a/?x=1&y=2'); return [u.searchParams === u.searchParams, u.searchParams.get('x'), Object.prototype.toString.call(u.searchParams)] })()`);
expr(`(function(){ var u = new ${N}('http://a/?x=1'); u.searchParams.append('y', 'a b'); return [u.search, u.href] })()`);
expr(`(function(){ var u = new ${N}('http://a/?x=1'); u.searchParams.delete('x'); return [u.search, u.href] })()`);
expr(`(function(){ var u = new ${N}('http://a/?x=1'); u.search = 'z=2'; return [u.searchParams.get('z'), u.searchParams.get('x'), u.href] })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.searchParams = 'x'; return u.href })()`);
// Setters.
const SETTERS = {
  href: ["http://b/c?d#e", "b", "", "x:y", "http://", "  http://q/  "],
  protocol: ["https", "https:", "ftp", "file", "foo", "HTTPS:", "1x", "", "ws://x", "http:extra", "mailto"],
  username: ["u", "", "u v", "ü", "a:b", "a@b", "a/b"],
  password: ["p", "", "p q", "ü", "a:b", "a@b"],
  host: ["h2", "h2:81", "h2:80", "h2:", "[::1]", "[::1]:3", "a b", "", "h2/x", "h2?x", "H2.COM", "1.2.3", ":81"],
  hostname: ["h2", "h2:81", "[::1]", "a b", "", "H2.COM", "ü.com"],
  port: ["81", "80", "", "81x", "x", "65535", "65536", "-1", "0", "8e1", " 82"],
  pathname: ["/x", "x", "", "/a/../b", "/a b", "/a?b", "/a#b", "\\a\\b", "/é", "//x"],
  search: ["x=1", "?x=1", "", "?", "x y", "é", "#a", "??a"],
  hash: ["x", "#x", "", "#", "x y", "é", "##a"],
};
const BASES = ["http://u:p@a.com:8080/p/q?r#s", "foo://a/b", "mailto:x@y", "file:///a/b"];
for (const [k, vals] of Object.entries(SETTERS)) {
  for (const base of BASES) {
    for (const v of vals) {
      expr(`(function(){ var u = new ${N}(${JSON.stringify(base)}); u.${k} = ${JSON.stringify(v)}; return [u.href, u.${k}] })()`);
    }
  }
}
expr(`(function(){ var u = new ${N}('http://a/'); u.origin = 'x'; return u.href })()`);
expr(`(function(){ 'use strict'; var u = new ${N}('http://a/'); try { u.origin = 'x' } catch (e) { return [e.name, e.message] } })()`);
expr(`(function(){ var u = new ${N}('http://a/'); try { u.href = 'nope' } catch (e) { return [e.name, e.message, e.code] } })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.port = { toString() { return '99' } }; return u.href })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.port = 99; return u.href })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.hash = null; return u.href })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.hash = undefined; return u.href })()`);
expr(`(function(){ var u = new ${N}('http://a/'); u.pathname = Symbol(); return u.href })()`);
// Serialização e inspeção.
expr(`JSON.stringify({ u: new ${N}('http://a/b?c#d') })`);
expr(`'' + new ${N}('http://a/b')`);
expr(`new ${N}('http://a/b') + 1`);
expr(`new ${N}('http://a/b') == 'http://a/b'`);
expr(`Object.keys(JSON.parse(JSON.stringify(new ${N}('http://a/b'))))`);
// `this` alheio nos setters, origem de blob:/file:/data:/javascript:/ws/ftp e o inspect custom.
for (const m of ["href", "protocol", "username", "password", "host", "hostname", "port", "pathname", "search", "hash"]) {
  expr(`(function(){ try { Object.getOwnPropertyDescriptor(${N}.prototype, '${m}').set.call({}, 'x') } catch (e) { return [e.name, e.message, e.code, e instanceof TypeError] } })()`);
  expr(`(function(){ try { Object.getOwnPropertyDescriptor(${N}.prototype, '${m}').set.call(null) } catch (e) { return [e.name, e.message, e.code] } })()`);
}
for (const u of ["blob:http://a:81/uuid", "blob:https://a/uuid", "blob:null/x", "blob:foo/x", "file:///a", "file://h/a", "data:,x", "javascript:1", "ws://h:81/", "ws://h:80/", "wss://h:444", "ftp://h:2121/", "ftp://h:21/", "foo://h:5/", "about:blank", "http://a:80/", "https://a:8080/", "mailto:a@b", "blob:file:///a/b", "blob:ws://h/x", "blob:a", "blob:blob:http://a/b"]) {
  expr(`new ${N}(${JSON.stringify(u)}).origin`);
}
const INSPECT = `${N}.prototype[Symbol.for('nodejs.util.inspect.custom')]`;
expr(`(function(){ var u = new ${N}('http://u:p@a.com:8080/p/q?r=1&s=2#s'); return ${INSPECT}.call(u, 2, {}) })()`);
expr(`(function(){ var u = new ${N}("http://a/b?c=1&d='x\\"#h'"); return ${INSPECT}.call(u, 2, {}) })()`);
expr(`(function(){ return ${INSPECT}.call(new ${N}('file:///a'), 2, {}) })()`);
expr(`(function(){ return ${INSPECT}.call(new ${N}('http://a/?'), 2, undefined) })()`);
expr(`(function(){ return ${INSPECT}.call(new ${N}('http://a/?q=1'), undefined, undefined) })()`);
for (const [d, o] of [["0", "{depth:0}"], ["0", "{depth:1}"], ["1", "{depth:1}"], ["2", "{depth:null}"], ["2", "{depth:Infinity}"], ["'x'", "{}"], ["2", "{depth:'3'}"]]) {
  expr(`(function(){ return ${INSPECT}.call(new ${N}('http://a/?q=1'), ${d}, ${o}) })()`);
}
expr(`(function(){ var r = ${INSPECT}.call(new ${N}('http://a/?q=1'), -1, {}); return [typeof r, String(r)] })()`);
expr(`(function(){ var o = {}; return ${INSPECT}.call(o, 2, {}) === o })()`);
expr(`(function(){ class X extends ${N} {}; return ${INSPECT}.call(new X('http://a/'), 2, {}) })()`);
expr(`(function(){ var u = new ${N}('http://a/'); Object.setPrototypeOf(u, null); return ${INSPECT}.call(u, 2, {}) })()`);
expr(`(function(){ var u = new ${N}('http://a/?q=1'); return Bun.inspect(u) })()`);
expr(`(function(){ var u = new ${N}('http://a/?q=1'); return Bun.inspect({ u: u }, { depth: 0 }) })()`);
expr(`(function(){ var u = new ${N}('http://a/?q=1'); return Bun.inspect([u], { depth: 0 }) })()`);
// Host `xn--` inválido (hasValidParsedHost): construtor, canParse, parse, base, href e setters; esquema não especial
// e `file:` (especial) à parte; rótulo vindo de Unicode ou de `%`.
for (const h of ["xn--", "xn--a", "xn--bcher-kva", "xn--bcher-kvb", "xn--zz", "xn---", "xn--a.com", "a.xn--", "xn--ls8h", "xn--ls8h=", "XN--BCHER-KVA", "a.xn--bcher-kvb.com", "xn--ASCII-", "xn%2d%2da", "a.XN--zz", "bücher.xn--zz", "ex.com", "xn--a@h"]) {
  const lit = JSON.stringify(h);
  expr(`new ${N}('http://' + ${lit} + '/').href`);
  expr(`${N}.canParse('http://' + ${lit} + '/')`);
  expr(`${N}.parse('http://' + ${lit} + '/') === null`);
  expr(`new ${N}('x', 'http://' + ${lit} + '/').href`);
  expr(`new ${N}('//' + ${lit} + '/p', 'http://a/').href`);
  expr(`new ${N}('foo://' + ${lit} + '/').href`);
  expr(`new ${N}('file://' + ${lit} + '/x').href`);
  expr(`new ${N}('ws://' + ${lit} + ':81/').href`);
  expr(`new ${N}(' \\thttp://' + ${lit} + '/').href`);
  expr(`(function(){ var u = new ${N}('http://a/'); u.href = 'http://' + ${lit} + '/'; return u.href })()`);
  expr(`(function(){ var u = new ${N}('http://a/'); u.host = ${lit}; return u.href })()`);
  expr(`(function(){ var u = new ${N}('http://a/'); u.hostname = ${lit}; return u.href })()`);
  expr(`(function(){ var u = new ${N}('http://a/'); u.host = ${lit} + ':81'; return u.href })()`);
  expr(`(function(){ var u = new ${N}('foo://a/'); u.host = ${lit}; return u.href })()`);
}
// createObjectURL / revokeObjectURL: `blob:<uuid>` v4, tipos aceitos, erros, revogação.
const UUID = "/^blob:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/";
expr(`${UUID}.test(${N}.createObjectURL(new Blob(['x'])))`);
expr(`${UUID}.test(${N}.createObjectURL(new File(['a'], 'f')))`);
expr(`${UUID}.test(${N}.createObjectURL(new Blob(['x']), 2))`);
expr(`${N}.createObjectURL(new Blob([])).length`);
expr(`(function(){ var b = new Blob(['x']); return ${N}.createObjectURL(b) === ${N}.createObjectURL(b) })()`);
expr(`(function(){ var u = ${N}.createObjectURL(new Blob(['x'])); var p = new ${N}(u); return [p.protocol, p.origin, p.host, p.search, p.hash, p.pathname === u.slice(5), ${N}.canParse(u), p.href === u] })()`);
expr(`(function(){ var u = ${N}.createObjectURL(new Blob(['x'])); var r = ${N}.revokeObjectURL(u); return [r, ${N}.canParse(u), new ${N}(u).href === u] })()`);
expr(`${N}.createObjectURL.call(null, new Blob(['x'])).length`);
expr(`${N}.createObjectURL()`);
for (const bad of ["undefined", "null", "1", "'x'", "{}", "[]", "new URLSearchParams()", "Symbol()", "true", "new Uint8Array(1)", "new ArrayBuffer(1)", "Object.create(Blob.prototype)"]) {
  expr(`(function(){ try { ${N}.createObjectURL(${bad}) } catch (e) { return [e.name, e.message, e.code, e instanceof TypeError] } })()`);
}
expr(`${N}.revokeObjectURL()`);
for (const arg of ["undefined", "null", "5", "{}", "[]", "Symbol()", "true", "{ toString() { return 'x' } }", "''", "'blob:nope'", "new String('x')", "'x'"]) {
  expr(`(function(){ try { return String(${N}.revokeObjectURL(${arg})) } catch (e) { return [e.name, e.message, e.code] } })()`);
}
expr(`${N}.revokeObjectURL('x', 1)`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
