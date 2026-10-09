// Gera tests/golden/url_search_params_bun.tsv: `URLSearchParams` do global medido no bun 1.4.2 (descritor do global,
// `length`, `name`, chaves do construtor e do protótipo, descritores, serialização e análise de
// application/x-www-form-urlencoded, métodos de leitura e escrita, `size`, `length`, erros, iteradores, forEach, toJSON,
// inspect, construtor com registro ou sequência de pares, `delete` e `has` com o segundo argumento).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-url-search-params-golden.js > tests/golden/url_search_params_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

const N = "URLSearchParams";
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
expr(`${N}.length`);
expr(`${N}.name`);
expr(`Object.getOwnPropertyNames(${N})`);
expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
expr(`${N}.prototype.constructor === ${N}`);
expr(`Object.getOwnPropertyNames(${N}.prototype)`);
expr(`Object.prototype.toString.call(new ${N}())`);
for (const m of ["append", "delete", "get", "getAll", "has", "set", "sort", "toString"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name, Object.getOwnPropertyNames(d.value)] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
}
for (const p of ["size", "length"]) {
  expr(`(function(d){ return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${p}'))`);
  expr(`new ${N}('a=1&b=2&a=3').${p}`);
  expr(`new ${N}().${p}`);
}
expr(`(function(){ try { ${N}() } catch (e) { return [e.name, e.message, e.code] } })()`);
for (const m of ["append", "delete", "get", "getAll", "has", "set"]) {
  expr(`new ${N}().${m}()`);
  expr(`${N}.prototype.${m}.call({}, 'a', 'b')`);
  expr(`${N}.prototype.${m}.call(null, 'a', 'b')`);
}
expr(`${N}.prototype.sort.call({})`);
expr(`${N}.prototype.toString.call({})`);
expr(`Object.getOwnPropertyDescriptor(${N}.prototype, 'size').get.call({})`);
// Análise.
for (const q of ["", "?", "a=1", "?a=1&b=2", "a=1&&b=2", "&a", "a=", "=b", "a=b=c", "a+b=c+d", "a%20b=%41", "%", "%4", "%zz", "%C3%A9", "%C3", "%E4%BD%A0", "a=1;b=2", "?a=1?b", "??a", " a = b ", "a=%2B+%2b", "é=ü", "\ud800=\udc00", "a=1&a=2&a=3"]) {
  expr(`String(new ${N}(${JSON.stringify(q)}))`);
  expr(`[new ${N}(${JSON.stringify(q)}).size, new ${N}(${JSON.stringify(q)}).get('a'), new ${N}(${JSON.stringify(q)}).getAll('a')]`);
}
expr(`String(new ${N}())`);
expr(`String(new ${N}(undefined))`);
expr(`String(new ${N}(null))`);
expr(`String(new ${N}(5))`);
expr(`String(new ${N}(true))`);
expr(`String(new ${N}(new ${N}('x=1&y=2')))`);
expr(`(function(){ var a = new ${N}('x=1'); var b = new ${N}(a); b.append('y', '2'); return [String(a), String(b)] })()`);
expr(`new ${N}({ toString: function () { return 'q=7' } }).get('q')`);
expr(`new ${N}({ toString: function () { throw new RangeError('boom') } })`);
// Escrita e serialização.
expr(`(function(){ var p = new ${N}(); p.append('a b', 'c&d=e'); p.append('é', '*-._~!\\'()'); p.append('\\ud800', '\\u4f60'); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3'); p.set('a', '9'); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3'); p.set('c', '9'); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3&a=4'); p.set('a', 'z'); return [String(p), p.size] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3'); p.delete('a'); return [String(p), p.size] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); p.delete('zz'); return String(p) })()`);
expr(`(function(){ var p = new ${N}('b=2&a=1&c=3&a=0'); p.sort(); return String(p) })()`);
expr(`(function(){ var p = new ${N}('\\u00e9=1&z=2&\\ud83d\\ude00=3&\\uffff=4&a=5'); p.sort(); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1'); return [p.has('a'), p.has('b'), p.has(), 0] })()`);
expr(`new ${N}('a=1').has()`);
expr(`new ${N}('1=x').get(1)`);
expr(`new ${N}('undefined=x').get(undefined)`);
expr(`new ${N}('null=x').has(null)`);
expr(`(function(){ var p = new ${N}(); p.append(1, 2); p.append(null, undefined); return String(p) })()`);
expr(`(function(){ var p = new ${N}(); p.append(Symbol(), 'x') })()`);
expr(`(function(){ var p = new ${N}(); p.set('a', { toString: function () { return 'obj' } }); return String(p) })()`);
expr(`new ${N}('a=1').toString === ${N}.prototype.toString`);
expr(`'' + new ${N}('a=1&b=2')`);
expr(`new ${N}('a=1').append('b', '2')`);
expr(`(function(){ class P extends ${N} {}; return String(new P('a=1')) + '|' + (new P() instanceof ${N}) })()`);
expr(`Object.getOwnPropertyNames(globalThis).indexOf('URLSearchParams') - Object.getOwnPropertyNames(globalThis).indexOf('URL')`);

// Iteração, forEach, toJSON, inspect.
const IT = `Object.getPrototypeOf(new ${N}().keys())`;
const INSPECT = `${N}.prototype[Symbol.for('nodejs.util.inspect.custom')]`;
expr(`Reflect.ownKeys(${N}.prototype).map(String)`);
expr(`Object.keys(${N}.prototype)`);
expr(`(function(){ var r = []; for (var k in new ${N}('a=1')) r.push(k); return r })()`);
for (const m of ["entries", "keys", "values", "forEach", "toJSON"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
}
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.iterator))`);
expr(`(function(d){ return [typeof d.value, d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.for('nodejs.util.inspect.custom')))`);
expr(`${N}.prototype[Symbol.iterator] === ${N}.prototype.entries`);
expr(`new ${N}('a=1&b=2').length`);
expr(`[...new ${N}('a=1&b=2&a=3').entries()]`);
expr(`[...new ${N}('a=1&b=2&a=3').keys()]`);
expr(`[...new ${N}('a=1&b=2&a=3').values()]`);
expr(`[...new ${N}('a=1&b=2')]`);
expr(`Array.from(new ${N}('a=1&b=2'))`);
expr(`Object.fromEntries(new ${N}('a=1&b=2'))`);
expr(`(function(){ var r = []; for (var e of new ${N}('a=1&b=2')) r.push(e[0] + e[1]); return r })()`);
expr(`Object.prototype.toString.call(new ${N}().entries())`);
expr(`String(new ${N}().keys())`);
expr(`Reflect.ownKeys(${IT}).map(String)`);
expr(`${IT} === Object.getPrototypeOf(new ${N}().values())`);
expr(`Object.getPrototypeOf(${IT}) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${IT}, 'next'))`);
expr(`(function(d){ return [d.value, d.enumerable, d.writable, d.configurable] })(Object.getOwnPropertyDescriptor(${IT}, Symbol.toStringTag))`);
expr(`(function(){ var i = new ${N}('a=1&b=2').entries(); return [i.next(), i.next(), i.next(), i.next()] })()`);
expr(`(function(){ var i = new ${N}().keys(); return [Reflect.ownKeys(i.next()), i.next().done] })()`);
expr(`(function(){ var i = new ${N}('a=1').keys(); return [i[Symbol.iterator]() === i, Object.getPrototypeOf(i) === ${IT}] })()`);
expr(`(function(){ var r = new ${N}('a=1').entries().next(); return [Reflect.ownKeys(r), Object.getPrototypeOf(r) === Object.prototype, Array.isArray(r.value)] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); return p.entries().next().value !== p.entries().next().value })()`);
expr(`${IT}.next.call({})`);
expr(`${IT}.next.call([][Symbol.iterator]())`);
expr(`${IT}.next.call(undefined)`);
expr(`${N}.prototype.entries.call({})`);
expr(`${N}.prototype.keys.call(null)`);
expr(`${N}.prototype.values.call([])`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.keys(); var r = [i.next().value]; p.delete('a'); r.push(i.next(), i.next()); return r })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.keys(); var r = [i.next().value]; p.append('c', '3'); r.push(i.next().value, i.next().value, i.next()); return r })()`);
expr(`(function(){ var r = []; new ${N}('a=1&b=2').forEach(function (v, k, o) { r.push([v, k, o instanceof ${N}, this === undefined ? 'u' : typeof this]) }); return r })()`);
expr(`(function(){ var t = {}, r = []; new ${N}('a=1').forEach(function () { r.push(this === t) }, t); return r })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'), r = []; p.forEach(function (v, k) { r.push(k); if (k == 'a') p.append('c', '3') }); return r })()`);
expr(`new ${N}('a=1').forEach(function () {})`);
expr(`new ${N}().forEach()`);
expr(`new ${N}('a=1').forEach(1)`);
expr(`${N}.prototype.forEach.call({}, function () {})`);
expr(`new ${N}('a=1').forEach(function () { throw new RangeError('x') })`);
expr(`new ${N}('a=1&b=2').toJSON()`);
expr(`new ${N}('b=1&a=2&b=3&a=4&b=5').toJSON()`);
expr(`new ${N}().toJSON()`);
expr(`JSON.stringify(new ${N}('a=1&a=2&b=3'))`);
expr(`Reflect.ownKeys(new ${N}('b=1&a=2').toJSON()).map(String)`);
expr(`(function(d){ return [d.value, d.writable, d.enumerable, d.configurable] })(Object.getOwnPropertyDescriptor(new ${N}('a=1').toJSON(), Symbol.toStringTag))`);
expr(`Object.getPrototypeOf(new ${N}('a=1').toJSON()) === Object.prototype`);
expr(`new ${N}('1=x&b=y&0=z').toJSON()`);
expr(`Object.getOwnPropertyDescriptor(new ${N}('__proto__=x').toJSON(), '__proto__')`);
expr(`${N}.prototype.toJSON.call({})`);
expr(`${INSPECT}.call(new ${N}('a=1&b=2'), 2, {})`);
expr(`${INSPECT}.call(new ${N}(), 2, {})`);
expr(`${INSPECT}.call(new ${N}('a=1'))`);
expr(`${INSPECT}.call(new ${N}('a=1'), -1, {})`);
expr(`${INSPECT}.call(new ${N}('a=1'), null, {})`);
expr(`${INSPECT}.call(new ${N}("a'b=c d&e=%0A"), 2, {})`);
expr(`${INSPECT}.call(new ${N}('a\\'b"c=1'), 2, {})`);
expr(`${INSPECT}.call(new ${N}('a=1&b=2&c=3'), 2, {})`);
expr(`(function(o){ return o === ${INSPECT}.call(o, 2, {}) })({})`);
// Construtor com objeto (registro).
expr(`String(new ${N}({ a: 1, b: 'x y' }))`);
expr(`String(new ${N}({ b: 1, a: 2, 1: 'n' }))`);
expr(`String(new ${N}({ [Symbol('s')]: 1, a: 2 }))`);
expr(`String(new ${N}(Object.defineProperty({ v: 2 }, 'h', { value: 1, enumerable: false })))`);
expr(`String(new ${N}(Object.create({ inh: 1 }, { own: { value: 2, enumerable: true } })))`);
expr(`String(new ${N}({}))`);
expr(`String(new ${N}(function () {}))`);
expr(`String(new ${N}(new Number(3)))`);
expr(`String(new ${N}({ a: undefined, b: null }))`);
expr(`String(new ${N}({ length: 1, 0: ['a', 'b'] }))`);
expr(`String(new ${N}(new Proxy({ a: 1 }, {})))`);
expr(`String(new ${N}({ '\\ud800': '\\udc00' }))`);
expr(`String(new ${N}({ get a() { throw new RangeError('g') } }))`);
expr(`String(new ${N}({ a: { toString: function () { throw new RangeError('t') } } }))`);
expr(`String(new ${N}({ a: Symbol() }))`);
expr(`String(new ${N}({ [Symbol.iterator]: null, a: 2 }))`);
expr(`String(new ${N}({ [Symbol.iterator]: undefined, a: 2 }))`);
expr(`String(new ${N}({ [Symbol.iterator]: 1, a: 2 }))`);
expr(`(function(){ class Q extends ${N} {}; return String(new Q({ z: 1 })) })()`);
// Construtor com sequência de pares.
expr(`String(new ${N}([['a', '1'], ['b', '2'], ['a', '3']]))`);
expr(`String(new ${N}([]))`);
expr(`String(new ${N}([['a']]))`);
expr(`String(new ${N}([['a', 'b', 'c']]))`);
expr(`String(new ${N}([[]]))`);
expr(`String(new ${N}(['ab']))`);
expr(`String(new ${N}([1]))`);
expr(`String(new ${N}([null]))`);
expr(`String(new ${N}([undefined]))`);
expr(`String(new ${N}([{ 0: 'a', 1: 'b', length: 2 }]))`);
expr(`String(new ${N}([new Set(['a', 'b'])]))`);
expr(`String(new ${N}([['a', Symbol()]]))`);
expr(`String(new ${N}((function* () { yield ['a', '1']; yield ['b', '2'] })()))`);
expr(`String(new ${N}(new Map([['a', '1'], ['b', '2']])))`);
expr(`String(new ${N}(new Set([['a', '1']])))`);
expr(`String(new ${N}({ [Symbol.iterator]: function* () { yield ['x', 'y'] } }))`);
expr(`String(new ${N}(new String('a=1')))`);
expr(`(function(){ var closed = false; var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 5 } }, return() { closed = true; return {} } } } }; try { new ${N}(it) } catch (e) { return [e.name, e.message, e.code, closed] } })()`);
expr(`String(new ${N}(new ${N}('a=1')))`);
// delete e has com o segundo argumento.
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3&a=1'); p.delete('a', '1'); return [String(p), p.size] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3'); p.delete('a', undefined); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=null&a=1'); p.delete('a', null); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1&a=2'); p.delete('a', 1); return String(p) })()`);
expr(`(function(){ var p = new ${N}('a=1&a=2'); p.delete('a', '9'); return String(p) })()`);
expr(`new ${N}('a=1').delete('a', Symbol())`);
expr(`(function(){ var p = new ${N}('a=obj&a=2'); p.delete('a', { toString: function () { return 'obj' } }); return String(p) })()`);
expr(`[${N}.prototype.delete.length, ${N}.prototype.has.length]`);
expr(`(function(){ var p = new ${N}('a=1&b=2&a=3'); return [p.has('a', '1'), p.has('a', '2'), p.has('a', undefined), p.has('a', null), p.has('b', 2), p.has('c', '1')] })()`);
expr(`(function(){ var p = new ${N}('a=null'); return [p.has('a', null), p.has('a')] })()`);
expr(`new ${N}('a=1').has('a', Symbol())`);
expr(`(function(){ var r = []; new ${N}('a=1').has({ toString: function () { r.push('n'); return 'a' } }, { toString: function () { r.push('v'); return '1' } }); return r })()`);
expr(`(function(){ var r = []; new ${N}('a=1').delete({ toString: function () { r.push('n'); return 'a' } }, { toString: function () { r.push('v'); return '1' } }); return r })()`);
expr(`${N}.prototype.has.call({}, 'a', 'b')`);

// Iterador vivo: esgotado fica esgotado, índice além do fim, mexida no meio.
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.entries(); var r = []; r.push(i.next().done, i.next().done, i.next().done); p.append('c', '3'); r.push(i.next(), [...i]); return r })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.keys(); i.next(); p.delete('a'); p.append('c', '3'); return [i.next(), i.next(), i.next()] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.keys(); i.next(); i.next(); p.delete('a'); p.delete('b'); p.append('x', '1'); p.append('y', '1'); p.append('z', '1'); return [i.next(), i.next()] })()`);
expr(`(function(){ var p = new ${N}('a=1&b=2'); var i = p.keys(); p.append('c', '3'); return [...i] })()`);

// Auditoria de 2026-10-09: substitutas soltas, UTF-8 malformado, ligação com URL e erros de símbolo.
expr(`String(new ${N}('\\ud800=\\udc00&a=\\ud83d'))`);
expr(`new ${N}('a=%ED%A0%80').get('a').length`);
expr(`Array.from(new ${N}('a=%F0%9F%98').get('a'), function (c) { return c.charCodeAt(0) })`);
expr(`String(new ${N}('a=%C0%80&b=%e4%bd%a0%&c=%zz%'))`);
expr(`String(new ${N}('?a=1&?b=2'))`);
expr(`(function(){ var p = new ${N}('b=1&a=2&b=0&\\ud83d\\ude00=x&\\uffff=y&\\ue000=z'); p.sort(); return String(p) })()`);
expr(`String(new ${N}({ a: { toString: function () { return 'z' } }, b: [1, 2] }))`);
expr(`new ${N}('x').append(Symbol(), 1)`);
expr(`new ${N}('a=1').get(Symbol())`);
expr(`new ${N}(Symbol())`);
expr(`new ${N}([[Symbol(), 1]])`);
expr(`(function(){ var u = new URL('http://x/?a=1'); u.searchParams.append('b', '2 3'); return [u.search, u.href] })()`);
expr(`(function(){ var u = new URL('http://x/?'); var n = u.searchParams.size; u.searchParams.append('a', ''); return [n, u.search, u.href] })()`);
expr(`(function(){ var u = new URL('http://x/?a=1#h'); u.searchParams.delete('a'); return [u.search, u.href, u.hash] })()`);
expr(`(function(){ var u = new URL('http://x/?a=1#h'); u.search = ''; return [u.href, String(u.searchParams)] })()`);
expr(`(function(){ var u = new URL('http://x/?a=1'); var p = u.searchParams; u.search = '?z=9&y'; return [String(p), p.size, p === u.searchParams] })()`);
expr(`(function(){ var u = new URL('http://x/?a=1'); var p = u.searchParams; u.href = 'http://y/?q=2'; return [String(p), u.search] })()`);
expr(`(function(){ var u = new URL('http://x/?a=b+c%20d~'); u.searchParams.sort(); return [u.search, u.href] })()`);
expr(`(function(){ var u = new URL('http://x/?a=1'); u.searchParams.set('a', '\\ud800'); return [u.search, u.href] })()`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
