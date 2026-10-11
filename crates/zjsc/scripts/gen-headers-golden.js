// Gera tests/golden/headers_bun.tsv: `Headers` do global medido no bun 1.4.2 (descritor do global, `length`, `name`,
// chaves do construtor e do protótipo, descritores, construtor com registro, sequência de pares e outro Headers, erros,
// normalização de nome e valor, combinação com ", ", `set-cookie` separado, ordem de iteração, iteradores, forEach, toJSON,
// count, getSetCookie e getAll).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-headers-golden.js > tests/golden/headers_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

const N = "Headers";
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
expr(`${N}.length`);
expr(`${N}.name`);
expr(`Object.getOwnPropertyNames(${N})`);
expr(`Object.getPrototypeOf(${N}.prototype) === Object.prototype`);
expr(`${N}.prototype.constructor === ${N}`);
expr(`Object.getOwnPropertyNames(${N}.prototype)`);
expr(`Object.getOwnPropertySymbols(${N}.prototype).map(String)`);
expr(`Object.prototype.toString.call(new ${N}())`);
expr(`Object.keys(new ${N}({ a: 1 }))`);
expr(`Object.keys(${N}.prototype)`);
expr(`${N}.prototype[Symbol.iterator] === ${N}.prototype.entries`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value] })(Object.getOwnPropertyDescriptor(${N}.prototype, Symbol.toStringTag))`);
for (const m of ["append", "delete", "get", "getAll", "has", "set", "entries", "keys", "values", "forEach", "toJSON", "getSetCookie"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name, Object.getOwnPropertyNames(d.value)] })(Object.getOwnPropertyDescriptor(${N}.prototype, '${m}'))`);
}
expr(`(function(d){ return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length] })(Object.getOwnPropertyDescriptor(${N}.prototype, 'count'))`);
expr(`(function(){ try { ${N}() } catch (e) { return [e.name, e.message, e.code] } })()`);
for (const m of ["append", "delete", "get", "getAll", "has", "set", "entries", "keys", "values", "forEach", "toJSON"]) {
  expr(`${N}.prototype.${m}.call({}, 'a', 'b')`);
  expr(`${N}.prototype.${m}.call(null, 'a', 'b')`);
}
expr(`${N}.prototype.getSetCookie.call({})`);
expr(`Object.getOwnPropertyDescriptor(${N}.prototype, 'count').get.call({})`);
for (const m of ["append", "delete", "get", "getAll", "has", "set"]) expr(`new ${N}().${m}()`);
expr(`new ${N}().append('a')`);
expr(`new ${N}().set('a')`);
// Construtor.
for (const init of ["undefined", "null", "5", "true", "'x'", "''", "'abc'", "Symbol()", "[]", "{}", "() => 1", "[1]", "['ab']", "[{}]", "[[]]", "[['a']]", "[['a','b','c']]",
  "[[1,2]]", "[[{},'b']]", "[new Set(['a','b'])]", "new Map([['a','b']])", "new Set([['a','b']])", "{ 'a b': '1' }", "{ a: '\\n' }", "{ a: ' x \\t' }",
  "{ A: 1, a: 2 }", "{ a: [1, 2] }", "{ a: null }", "{ a: undefined }", "{ [Symbol()]: 1 }", "{ [Symbol.iterator]: 5 }", "{ [Symbol.iterator]: null, a: 1 }",
  "{ a: 1, b: 2 }", "[['B','1'],['a','2'],['b','3'],['Set-Cookie','x=1'],['set-cookie','y=2']]", "new Headers({ x: 1, 'Set-Cookie': 'q' })"]) {
  expr(`(function(){ var h = new ${N}(${init}); return [h.count, [...h]] })()`);
}
expr(`(function(){ var a = new ${N}({ x: '1' }); var b = new ${N}(a); b.append('y', '2'); return [[...a], [...b]] })()`);
expr(`(function(){ var closed = false; var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 5 } }, return() { closed = true; return {} } } } }; try { new ${N}(it) } catch (e) { return [e.name, e.message, e.code, closed] } })()`);
expr(`new (class X extends ${N} {})({ a: 1 }).get('a')`);
expr(`Reflect.construct(${N}, [], Object).constructor === Object`);
// Nome e valor.
for (const name of ["", "a b", "a:b", "é", "a\\0", "a\\n", "(", "ok-Name_1.2~!#$%&\\'*+^`|", "A", "SET-COOKIE"]) {
  expr(`new ${N}().has('${name}')`);
  expr(`(function(){ var h = new ${N}(); h.append('${name}', '1'); return [...h] })()`);
}
for (const value of ["", " x ", "\\t x\\n", "b\\rc", "b\\nc", "\\0", "a\\0b", "\\u00a0x\\u00a0", "\\u000bx", "\\u007f", "é", "\\u00ff", "\\u0100", "\\u00ff\\u0100", "a\\u0100", "x y", " "]) {
  expr(`(function(){ var h = new ${N}(); h.append('X-Y', '${value}'); return h.get('x-y') })()`);
  expr(`(function(){ var h = new ${N}(); h.set('X-Y', '${value}'); return h.get('X-Y') })()`);
}
expr(`new ${N}({ a: 1 }).get(Symbol())`);
expr(`new ${N}({ a: 1 }).set('a', Symbol())`);
expr(`new ${N}({ a: 1 }).get({ toString() { return 'A' } })`);
expr(`new ${N}().append(1, 2)`);
expr(`(function(){ var r = []; try { new ${N}().append({ toString() { r.push('n'); return 'a b' } }, { toString() { r.push('v'); return '1' } }) } catch (e) { r.push(e.message) } return r })()`);
// Leitura e escrita.
expr(`(function(){ var h = new ${N}(); h.append('A', '1'); h.append('a', '2'); h.append('a', '3'); return [h.get('a'), h.get('A'), h.has('a'), h.has('b'), h.get('b'), h.count, [...h]] })()`);
expr(`(function(){ var h = new ${N}([['a','1'],['b','2']]); h.set('a', '9'); h.set('c', '3'); return [[...h], h.toJSON()] })()`);
expr(`(function(){ var h = new ${N}([['a','1'],['b','2'],['a','3']]); h.delete('A'); return [[...h], h.delete('zz'), h.has('a')] })()`);
expr(`(function(){ var h = new ${N}([['a','1'],['b','2']]); h.delete('a'); h.append('a', '4'); return h.toJSON() })()`);
expr(`(function(){ var h = new ${N}(); return [h.append('a','1'), h.set('a','2'), h.delete('a')] })()`);
// set-cookie.
expr(`(function(){ var h = new ${N}(); h.append('Set-Cookie', 'a=1'); h.append('x', '1'); h.append('SET-COOKIE', 'b=2'); return [h.getSetCookie(), h.getAll('set-cookie'), h.get('set-cookie'), [...h], h.toJSON(), h.count] })()`);
expr(`(function(){ var h = new ${N}(); h.set('Set-Cookie', 'a'); h.set('set-cookie', 'b'); return h.getSetCookie() })()`);
expr(`(function(){ var h = new ${N}({ 'set-cookie': 'a' }); h.delete('SET-COOKIE'); return [h.has('set-cookie'), h.getSetCookie(), h.count, h.get('set-cookie')] })()`);
expr(`new ${N}().getSetCookie()`);
expr(`new ${N}().getAll('set-cookie')`);
expr(`new ${N}({ a: 1 }).getAll('a')`);
expr(`new ${N}({ a: 1 }).getAll()`);
expr(`new ${N}({ a: 1 }).getAll('a b')`);
expr(`new ${N}({ 'set-cookie': 'z', c: 1, a: 2, b: 3, d: 4 }).toJSON()`);
expr(`Object.keys(new ${N}({ c: 1, a: 2, b: 3 }).toJSON())`);
expr(`(function(){ var h = new ${N}([['a','1'],['a','2']]); return [h.get('a'), h.toJSON(), JSON.stringify(h)] })()`);
expr(`Object.prototype.toString.call(new ${N}({ a: 1 }).toJSON())`);
expr(`Object.getOwnPropertyDescriptor(new ${N}({ a: 1 }).toJSON(), 'a')`);
// Iteração.
expr(`(function(){ var h = new ${N}([['B','1'],['a','2'],['b','3'],['Set-Cookie','x=1'],['set-cookie','y=2']]); return [[...h.keys()], [...h.values()], [...h.entries()], [...h], h[Symbol.iterator] === h.entries] })()`);
expr(`(function(){ var h = new ${N}([['x','1'],['set-cookie','a'],['a','2'],['set-cookie','b']]); return [...h] })()`);
expr(`Object.prototype.toString.call(new ${N}().entries())`);
expr(`Object.prototype.toString.call(new ${N}().keys())`);
expr(`Object.prototype.toString.call(new ${N}().values())`);
expr(`Object.getOwnPropertyNames(Object.getPrototypeOf(new ${N}().entries()))`);
expr(`Object.getPrototypeOf(new ${N}().entries()) === Object.getPrototypeOf(new ${N}().keys())`);
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(new ${N}().entries()), 'next'))`);
expr(`Object.getPrototypeOf(Object.getPrototypeOf(new ${N}().entries())) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))`);
expr(`Object.getPrototypeOf(new ${N}().entries()).next.call({})`);
expr(`(function(){ var h = new ${N}({ b: 1, a: 2 }); var it = h.entries(); h.append('0', 'z'); return [it.next(), it.next(), it.next(), it.next(), it.next()] })()`);
expr(`(function(){ var h = new ${N}({ b: 1 }); var it = h.keys(); var first = it.next(); h.append('a', '1'); return [first, it.next(), it.next()] })()`);
expr(`(function(){ var h = new ${N}({ b: 1, a: 2 }); var r = []; h.forEach(function (v, k, t) { r.push([v, k, t === h, this && this.x]); if (k === 'a') h.append('c', '3') }, { x: 7 }); return r })()`);
expr(`new ${N}({ a: 1 }).forEach(1)`);
expr(`new ${N}({ a: 1 }).forEach()`);
expr(`new ${N}({ a: 1 }).forEach(function () { throw new RangeError('boom') })`);
expr(`(function(){ var h = new ${N}({ a: 1 }); var r = []; h.forEach(function (v) { r.push(typeof this) }); return r })()`);
expr(`(function(){ var h = new ${N}({ a: 1 }); var it = h.entries(); return [it[Symbol.iterator]() === it, it.next().value, it.next().done, it.next().done] })()`);

// Iterador vivo (não congela no primeiro next): esgotado fica esgotado, mexida depois do primeiro next, índice além do fim.
expr(`(function(){ var h = new ${N}({ a: 1, b: 2 }); var i = h.entries(); var r = []; r.push(i.next().done, i.next().done, i.next().done); h.append('c', '3'); r.push(i.next(), [...i]); return r })()`);
expr(`(function(){ var h = new ${N}({ b: 1, c: 2 }); var i = h.keys(); i.next(); h.append('a', '3'); return [i.next(), i.next(), i.next()] })()`);
expr(`(function(){ var h = new ${N}({ b: 1, c: 2 }); var i = h.keys(); i.next(); h.append('d', '3'); return [...i] })()`);
expr(`(function(){ var h = new ${N}({ b: 1, c: 2 }); var i = h.keys(); i.next(); h.delete('c'); return [...i] })()`);
expr(`(function(){ var h = new ${N}({ b: 1, c: 2 }); var i = h.values(); i.next(); h.set('c', '9'); return [...i] })()`);
expr(`(function(){ var h = new ${N}({ a: 1, b: 2 }); var i = h.keys(); i.next(); i.next(); h.delete('a'); h.delete('b'); h.append('x', '1'); h.append('y', '1'); h.append('z', '1'); return [i.next(), i.next()] })()`);

// Auditoria de 2026-10-09: gerador como iterável, Map, unidade acima de 0xFF no nome, valor vazio, vírgula em cookie,
// getAll em maiúsculas, String/JSON.stringify, duplicatas de nome conhecido e escrita depois de set.
expr(`[...new ${N}(function* () { yield ['a', '1'] }())]`);
expr(`[...new ${N}(new Map([['a', '1'], ['B', '2']]))]`);
expr(`new ${N}({ a: '1' }).get('a\\u0100')`);
expr(`new ${N}().append('a', 'b\\u0100')`);
expr(`new ${N}().delete('')`);
expr(`new ${N}().set('a b', 'x')`);
expr(`new ${N}(['a'])`);
expr(`new ${N}([['a', '1'], ['b']])`);
expr(`new ${N}([['a', '1', '2']])`);
expr(`new ${N}({ a: '1' }).forEach.call(1)`);
expr(`String(new ${N}())`);
expr(`JSON.stringify(new ${N}({ a: '1', 'set-cookie': 'q' }))`);
expr(`new ${N}([['content-length', '5'], ['Content-Length', '6']]).get('content-length')`);
expr(`(function(){ var h = new ${N}(); h.append('a', '1'); h.append('A', '2'); h.set('a', '3'); return [...h] })()`);
expr(`(function(){ var h = new ${N}([['a', '1'], ['b', '2'], ['a', '3']]); h.set('a', '9'); return [...h] })()`);
expr(`new ${N}({ 'set-cookie': 'a=1, b=2' }).getSetCookie()`);
expr(`new ${N}([['set-cookie', 'a'], ['set-cookie', 'b']]).get('Set-Cookie')`);
expr(`(function(){ var h = new ${N}(); h.append('x', ''); return [h.get('x'), h.has('x')] })()`);
expr(`new ${N}([['set-cookie', 'a']]).getAll('SET-COOKIE')`);
expr(`(function(){ var h = new ${N}([['B', '1'], ['a', '2'], ['Set-Cookie', 'x=1'], ['set-cookie', 'y=2']]); var c = new ${N}(h); c.append('set-cookie', 'z=3'); return [h.getSetCookie(), c.getSetCookie(), [...c]] })()`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
