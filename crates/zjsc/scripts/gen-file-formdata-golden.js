// Gera tests/golden/file_formdata_bun.tsv: os globais `File` e `FormData` medidos no bun 1.4.2 (descritor, `length`,
// `name`, chaves, protótipo, construtor de `File` com nome, `type` e `lastModified`, relação com `Blob`, erros;
// `FormData` com append/set/get/getAll/has/delete/entries/keys/values/forEach/toJSON, valor string e Blob/File, nome
// de arquivo padrão, iteração viva, `length`, subclasse).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por
// indirect eval no mesmo processo. Valores que dependem do relógio (`lastModified` padrão) entram só como comparação
// (`> 1e12`), e o `boundary` do corpo multipart (aleatório) fica de fora.
// Medições surpreendentes (bun 1.4.2): `File.prototype === Blob.prototype` e `File` não herda de `Blob`
// (`Object.getPrototypeOf(File) !== Blob`); o protótipo compartilhado tem os métodos de arquivo do bun (`stat`,
// `write`, `writer`, `unlink`, `exists`); `FormData.prototype.append` com Blob sem nome devolve um Blob de nome
// `null` e `append(nome, blob, nome2)` copia o Blob (o `File` original nunca é o objeto devolvido, e o nome do
// `File` de origem vence o terceiro argumento); string com terceiro argumento é `TypeError`.
// Uso: bun scripts/gen-file-formdata-golden.js > tests/golden/file_formdata_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
// Corpo de função anônima que devolve o valor.
const fn = (body) => expr(`(function(){ ${body} })()`);

// Descritores e forma dos globais.
for (const n of ["File", "FormData"]) {
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${n}'))`);
  expr(`[${n}.length, ${n}.name]`);
  expr(`Object.getOwnPropertyNames(${n})`);
  expr(`Object.getOwnPropertyNames(${n}.prototype)`);
  expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
  expr(`${n}.prototype.constructor === ${n}`);
  expr(`${n}.prototype[Symbol.toStringTag]`);
  expr(`Object.prototype.hasOwnProperty.call(globalThis, '${n}')`);
  expr(`${n}()`);
}
// Sem `new`: `File` não leva código (mensagem de classe do JSC), `Blob` e `FormData` levam ERR_ILLEGAL_CONSTRUCTOR.
expr("File(['a'], 'n')");
expr("Blob([])");
expr("FormData()");
expr("new File(['a'])");
expr("File.prototype === Blob.prototype");
expr("Object.getPrototypeOf(File) === Blob");
expr("Object.getPrototypeOf(File.prototype) === Object.prototype");
expr("Object.getOwnPropertyNames(Blob.prototype)");
expr("new File([], 'a') instanceof Blob");
expr("new File([], 'a') instanceof File");
expr("new Blob([]) instanceof File");
expr("Object.getOwnPropertyNames(globalThis).indexOf('FormData') > -1");

// File: construtor.
fn("var f = new File(['ab', 'c'], 'a.txt'); return [f.name, f.size, f.type, f.lastModified > 1e12, String(f), Object.prototype.toString.call(f), Object.keys(f)]");
fn("var f = new File([], 'a', { type: 'Text/Plain', lastModified: 5 }); return [f.type, f.lastModified]");
expr("new File()");
expr("new File([])");
expr("new File('x', 'a')");
expr("new File([], 'a', { lastModified: '7' }).lastModified");
expr("new File([], 'a', { lastModified: 1.9 }).lastModified");
expr("new File([], 'a', { lastModified: -1 }).lastModified");
expr("new File([], 'a', { lastModified: NaN }).lastModified");
expr("new File([], 'a', { lastModified: 2 ** 60 }).lastModified");
expr("new File([], 'a', { lastModified: undefined }).lastModified > 1e12");
expr("new File([], 'a', { lastModified: null }).lastModified");
expr("new File([], 'a/b\\\\c').name");
expr("new File([], undefined).name");
expr("new File([], null).name");
expr("new File([], '').name");
expr("new File([], { toString() { return 'obj' } }).name");
expr("new File([], Symbol())");
expr("new File([], 'a', null).type");
expr("new File([], 'a', 5).type");
expr("new File([], 'a', { type: '\\u00e9' }).type");
expr("new File(['a'], 'a', { type: 'A/B;Charset=X' }).type");
expr("new File([], 'a', { endings: 'native' }).size");
expr("new File([new Blob(['x']), new File(['y'], 'n')], 'a').size");
expr("new File([new Uint8Array([1, 2, 3]), new ArrayBuffer(2), 'zz', 5], 'a').size");
expr("new File([{}], 'a').size");
expr("new File({ length: 1, 0: 'ab' }, 'a')");
expr("new File(new Set(['ab']), 'a').size");
fn("var f = new File(['x'], 'a'); f.name = 'z'; return f.name");
fn("var F = class extends File {}; var f = new F([], 'q'); return [f.name, f instanceof F, f instanceof File]");

// File: membros herdados do protótipo compartilhado.
fn("var f = new File(['abc'], 'a', { type: 'x/y' }); var s = f.slice(1, 2); return [s instanceof File, s.constructor.name, s.size, s.type, s.name]");
fn("var f = new File(['abc'], 'a'); var s = f.slice(); return [s.constructor.name, 'name' in s, s.size]");
expr("(function(){ var d = Object.getOwnPropertyDescriptor(File.prototype, 'name'); return [typeof d.get, d.set, d.enumerable, d.configurable] })()");
expr("(function(){ var d = Object.getOwnPropertyDescriptor(File.prototype, 'lastModified'); return [typeof d.get, d.set, d.enumerable, d.configurable] })()");
expr("Object.getOwnPropertyDescriptor(File.prototype, 'webkitRelativePath')");
expr("new File([], 'a').webkitRelativePath");
expr("Object.getOwnPropertyDescriptor(File.prototype, 'name').get.call({})");
expr("Object.getOwnPropertyDescriptor(File.prototype, 'lastModified').get.call(new Blob())");
expr("new Blob(['x']).name");
expr("new Blob(['x']).lastModified");
expr("[typeof File.prototype.text, typeof File.prototype.stream, typeof File.prototype.bytes, typeof File.prototype.arrayBuffer]");
fn("var f = new File(['\\u00e9'], 'a'); return [f.size, f.type]");
fn("var f = new File(['x'], 'a'); var d = Object.getOwnPropertyDescriptor(File.prototype, 'size'); return [typeof d.get, d.set, f.size]");

// FormData: forma.
expr("(function(){ var f = new FormData(); return [Object.prototype.toString.call(f), Object.keys(f), f.length, String(f)] })()");
expr("['append', 'delete', 'get', 'getAll', 'has', 'set', 'entries', 'keys', 'values', 'forEach', 'toJSON'].map(function (n) { var d = Object.getOwnPropertyDescriptor(FormData.prototype, n); return [n, typeof d.value, d.writable, d.enumerable, d.configurable, FormData.prototype[n].length] })");
expr("(function(){ var d = Object.getOwnPropertyDescriptor(FormData.prototype, 'length'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable] })()");
expr("[Object.getOwnPropertySymbols(FormData.prototype).map(String), FormData.prototype[Symbol.iterator] === FormData.prototype.entries]");
expr("[typeof FormData.from, FormData.from.length]");
expr("Object.getOwnPropertyDescriptor(FormData, 'from')");
expr("typeof FormData.from(new Uint8Array([97]))");
expr("FormData.prototype.append.call({}, 'a', 'b')");
expr("Object.getOwnPropertyDescriptor(FormData.prototype, 'length').get.call({})");
expr("FormData.prototype.entries.call({})");
expr("FormData.prototype.get.call(new Blob(), 'a')");

// FormData: construtor.
for (const a of ["1", "{}", "null", "undefined", "'x'", "new FormData()", "[]", "new Blob()"]) expr(`new FormData(${a}).length`);
fn("class F extends FormData {}; var f = new F(); f.append('a', '1'); return [f instanceof F, f.get('a')]");
fn("var f = new FormData(); f.append('a', '1'); var g = new FormData(f); return [g.length, g.has('a')]");

// FormData: append, get, getAll, has, length com strings.
fn("var f = new FormData(); f.append('a', '1'); f.append('a', '2'); f.append('b', 3); return [f.get('a'), f.getAll('a'), f.get('b'), f.get('c'), f.getAll('c'), f.has('a'), f.has('c'), f.length]");
fn("var f = new FormData(); f.append('a', undefined); return f.get('a')");
fn("var f = new FormData(); f.append('a', null); return f.get('a')");
fn("var f = new FormData(); f.append('a', { toString() { return 'o' } }); return f.get('a')");
fn("var f = new FormData(); f.append('a', 1n); return f.get('a')");
fn("var f = new FormData(); f.append('a', 1.5); f.append('b', true); f.append('c', [1, 2]); f.append('d', -0); return [...f]");
fn("var f = new FormData(); f.append(1, 2); return [f.get('1'), f.get(1)]");
fn("var f = new FormData(); f.append('undefined', 'u'); return [f.get(undefined), f.has(undefined)]");
fn("var f = new FormData(); f.append('null', 'u'); return f.get(null)");
fn("var f = new FormData(); f.append('a\\r\\nb', 'c\\nd'); return [f.get('a\\r\\nb'), f.get('a\\nb')]");
fn("var f = new FormData(); f.append('\\ud800', '\\udc00'); return [f.get('\\ud800'), f.get('\\ufffd'), f.has('\\ud800')]");
fn("var f = new FormData(); f.append('A', 'x'); return [f.get('a'), f.get('A')]");
fn("var f = new FormData(); f.append('', ''); return [f.get(''), f.has(''), f.length]");
fn("var f = new FormData(); f.append('a', '1'); f.append('a', '2'); return f.get('a')");
fn("var f = new FormData(); f.append('__proto__', '1'); return [f.get('__proto__'), f.length]");
expr("new FormData().append('a')");
expr("new FormData().append()");
expr("new FormData().append('a', Symbol())");
expr("new FormData().append(Symbol(), 'a')");
expr("new FormData().append('a', { toString() { throw new RangeError('x') } })");
expr("new FormData().append({ toString() { throw new RangeError('x') } }, 'a')");
fn("var f = new FormData(); return [f.append('a', 'b'), f.set('a', 'b'), f.delete('a')]");
expr("new FormData().get()");
expr("new FormData().getAll()");
expr("new FormData().has()");
expr("new FormData().delete()");
expr("new FormData().set('a')");
expr("new FormData().set()");

// FormData: valor Blob e File, nome de arquivo padrão.
fn("var f = new FormData(); f.append('a', new Blob(['x'])); var v = f.get('a'); return [v instanceof File, v instanceof Blob, v.constructor.name, v.name, v.size, v.type, typeof v.lastModified]");
fn("var f = new FormData(); f.append('a', new Blob(['x']), 'n.txt'); var v = f.get('a'); return [v.constructor.name, v.name]");
fn("var f = new FormData(); var F = new File(['x'], 'orig.txt', { type: 'a/b', lastModified: 9 }); f.append('a', F); var v = f.get('a'); return [v === F, v.name, v.lastModified, v.type, v.size]");
fn("var f = new FormData(); var F = new File(['x'], 'orig.txt', { lastModified: 9 }); f.append('a', F, 'new.txt'); var v = f.get('a'); return [v === F, v.name, v.lastModified, v.constructor.name, F.name]");
fn("var f = new FormData(); f.append('a', new Blob(['x']), undefined); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x']), ''); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x']), null); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x']), 5); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x']), { toString() { return 'obj' } }); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x']), Symbol()); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x'], { type: 'T/X' })); return f.get('a').type");
fn("var f = new FormData(); f.append('a', new Blob(['x'])); return f.get('a') === f.get('a')");
fn("var f = new FormData(); f.append('a', new Blob(['x'])); return f.get('a').lastModified > 1e12");
fn("var f = new FormData(); f.append('a', new Blob(['xyz'])); var g = f.getAll('a'); return [g.length, g[0].size]");
fn("var f = new FormData(); var b = new Blob(['x']); f.append('a', b); return [f.get('a') === b, f.get('a').size]");
expr("new FormData().append('a', 's', 'n')");
expr("new FormData().append('a', 's', 'n', 1)");
expr("new FormData().append('a', {}, 'x')");
expr("new FormData().append('a', new Blob(['x']), Symbol.iterator)");
expr("new FormData().append('a', new Uint8Array([1]))");
expr("new FormData().append('a', new ArrayBuffer(1))");
fn("var f = new FormData(); f.append('a', new Blob(['x']), 'one'); f.append('a', new File(['yy'], 'two')); f.append('a', 'str'); return f.getAll('a').map(function (v) { return typeof v === 'string' ? v : [v.constructor.name, v.name, v.size] })");

// FormData: set e delete.
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); f.append('a', '3'); f.set('a', '9'); return [...f]");
fn("var f = new FormData(); f.set('a', '1'); f.set('b', '2'); f.set('a', '3'); return [...f]");
fn("var f = new FormData(); f.append('x', '1'); f.append('a', '1'); f.append('y', '1'); f.append('a', '2'); f.set('a', 'z'); return [...f.keys()]");
fn("var f = new FormData(); f.set('a', new Blob(['x']), 'n'); var v = f.get('a'); return [v.constructor.name, v.name]");
fn("var f = new FormData(); f.set('a', new Blob(['x'])); var v = f.get('a'); return [v.constructor.name, v.name]");
fn("var f = new FormData(); f.set('a', new File(['x'], 'orig'), 'new'); return f.get('a').name");
fn("var f = new FormData(); f.append('a', new Blob(['x'])); f.set('a', 's'); return [f.get('a'), f.length]");
fn("var f = new FormData(); f.set('a', 's'); f.set('a', new Blob(['x'])); return [typeof f.get('a'), f.length]");
fn("var f = new FormData(); f.set('a', 's', 'n')");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); f.append('a', '3'); f.delete('a'); return [[...f], f.length, f.delete('zz'), f.has('a')]");
fn("var f = new FormData(); f.append('a', '1'); f.delete('a'); f.append('a', '2'); return [...f]");
fn("var f = new FormData(); f.append('a', '1'); f.delete('a', '1'); return f.length");
fn("var f = new FormData(); f.append('a', '1'); f.delete({ toString() { return 'a' } }); return f.length");

// FormData: iteração.
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); var it = f.entries(); return [Object.prototype.toString.call(it), it[Symbol.iterator]() === it, it.next(), it.next(), it.next(), it.next(), Object.getOwnPropertyNames(Object.getPrototypeOf(it))]");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); return [[...f.keys()], [...f.values()], Object.prototype.toString.call(f.keys()), Object.prototype.toString.call(f.values())]");
fn("var f = new FormData(); return Object.getPrototypeOf(f.entries()) === Object.getPrototypeOf(f.keys())");
fn("var f = new FormData(); var p = Object.getPrototypeOf(f.entries()); return [Object.getPrototypeOf(p) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())), Object.getOwnPropertySymbols(p).map(String), p[Symbol.toStringTag]]");
fn("var f = new FormData(); f.append('a', '1'); var r = []; f.forEach(function (v, k, t) { r.push([v, k, t === f, this === undefined, typeof this]) }); return r");
fn("var f = new FormData(); f.append('a', '1'); var r = []; f.forEach(function () { r.push(this) }, 5); return typeof r[0]");
fn("var f = new FormData(); f.append('a', '1'); f.forEach()");
fn("var f = new FormData(); f.append('a', '1'); f.forEach(5)");
fn("var f = new FormData(); var r = []; f.forEach(function () { r.push(1) }); return r");
fn("var f = new FormData(); f.append('a', '1'); return f.forEach(function () {})");
fn("var f = new FormData(); f.append('a', '1'); var r = []; for (var e of f) { r.push(e[0], e[1]); if (r.length < 6) f.append('b', '2') } return r");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); var r = []; for (var e of f) { r.push(e[0]); f.delete('b') } return r");
fn("var f = new FormData(); f.append('a', '1'); var it = f.entries(); it.next(); it.next(); f.append('b', '2'); return it.next()");
fn("var f = new FormData(); f.append('a', '1'); var e = f.entries().next().value; return [Array.isArray(e), Object.isFrozen(e), e.length]");
fn("var f = new FormData(); f.append('a', new Blob(['x']), 'q'); return [...f][0][1].name");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', new Blob(['x']), 'q'); var r = []; f.forEach(function (v, k) { r.push([k, typeof v, v instanceof File]) }); return r");
fn("var f = new FormData(); f.append('a', '1'); var it = f.keys(); return [it.next(), it.next()]");
fn("var f = new FormData(); f.append('a', '1'); var it = f.values(); return [it.next(), it.next()]");
fn("var f = new FormData(); f.append('a', '1'); f.append('a', '2'); return Array.from(f).length");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); return Object.fromEntries(f)");
fn("var f = new FormData(); f.append('a', '1'); f.append('a', '2'); return new URLSearchParams(f).toString()");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); var it = f.entries(); it.next(); f.delete('a'); return it.next()");
fn("var f = new FormData(); f.append('a', '1'); f.append('b', '2'); var it = f.keys(); it.next(); f.set('a', 'z'); return [it.next(), it.next()]");
expr("Object.getPrototypeOf(new FormData().entries()).next.call({})");

// FormData: JSON.
fn("var f = new FormData(); f.append('a', '1'); return JSON.stringify(f)");
fn("var f = new FormData(); f.append('a', '1'); f.append('a', '2'); f.append('b', '3'); return f.toJSON()");
fn("var f = new FormData(); f.append('a', '1'); f.append('a', new Blob(['x']), 'q'); var j = f.toJSON(); return [Array.isArray(j.a), typeof j.a[1], String(j.a[1])]");
fn("var f = new FormData(); f.append('__proto__', '1'); var j = f.toJSON(); return [Object.getPrototypeOf(j) === Object.prototype, Object.keys(j)]");
fn("var f = new FormData(); return [f.toJSON(), Object.keys(f.toJSON())]");
fn("var f = new FormData(); f.append('a', new Blob(['x'])); var j = f.toJSON(); return [typeof j.a, j.a instanceof Blob]");
fn("var f = new FormData(); f.append('a', '1'); var d = Object.getOwnPropertyDescriptor(FormData.prototype, 'toJSON'); return [d.writable, d.enumerable, d.configurable]");

// FormData.from: erros de argumento e formas de entrada.
expr("FormData.from()");
expr("FormData.from(undefined)");
expr("FormData.from(null)");
expr("FormData.from(1)");
expr("FormData.from({})");
expr("FormData.from([])");
expr("FormData.from(true)");
expr("FormData.from(Symbol('s'))");
fn("var f = FormData.from(''); return [f.length, f.toJSON()]");
fn("var f = FormData.from(new Blob(['a=1&b=2'])); return [f.length, f.toJSON()]");
fn("var f = FormData.from(new ArrayBuffer(0)); return [f.length, f.toJSON()]");
fn("var f = FormData.from(new TextEncoder().encode('a=1').buffer); return [f.length, f.toJSON()]");
fn("var f = FormData.from(new Uint8Array([97, 61, 49])); return [f.length, f.toJSON()]");
fn("var f = FormData.from(new DataView(new TextEncoder().encode('a=1').buffer)); return [f.length, f.toJSON()]");
expr("FormData.from('a=1', 1)");
expr("FormData.from('a=1', {})");
expr("FormData.from('a=1', true)");
expr("FormData.from('a=1', new Blob(['x']))");
expr("FormData.from(1, 1)");
expr("FormData.from(undefined, 1)");
fn("var f = FormData.from('a=1&b=2', undefined); return f.toJSON()");
fn("var f = FormData.from('a=1&b=2', null); return f.toJSON()");
fn("var f = FormData.from('a=1&b=2', ''); return f.toJSON()");
fn("var f = FormData.from('a=1&b=2', new Uint8Array(0)); return f.toJSON()");
fn("var f = FormData.from('--B\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--B--\\r\\n', 'B'); return f.toJSON()");
fn("var f = FormData.from('--B\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--B--\\r\\n', new TextEncoder().encode('B')); return f.toJSON()");
fn("var f = FormData.from('--B\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--B--\\r\\n', new TextEncoder().encode('B').buffer); return f.toJSON()");
fn("var f = FormData.from('garbage', 'B'); return [f.length, f.toJSON()]");
fn("var f = FormData.from('--B\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1', 'B'); return [f.length, f.toJSON()]");
fn("var f = FormData.from('--B\\r\\nbroken\\r\\n--B--\\r\\n', 'B'); return [f.length, f.toJSON()]");
fn("var f = FormData.from('--B\\r\\nContent-Disposition: form-data; name=\"a\"\\r\\n\\r\\n1\\r\\n--B--\\r\\n', 'X'); return [f.length, f.toJSON()]");
fn("var f = FormData.from('a=1&b=%zz&c'); return f.toJSON()");

// Corpo multipart (sem o boundary aleatório).
// File: nome e opções com efeitos (ToString do nome, getters que lançam, partes mistas, slice herda nome e data).
expr("new File(['a'], Symbol())");
expr("new File(['a'], { toString: function () { throw new RangeError('nm') } })");
expr("new File(['a'], 'f', { get type() { throw new RangeError('tp') } })");
expr("new File(['a'], 'f', { lastModified: { valueOf: function () { throw new RangeError('lm') } } })");
expr("[new File(['a'], undefined).name, new File(['a'], null).name, new File(['a'], 5).name, new File(['a'], '').name]");
expr("new File(['a'], 'f', { lastModified: 2n })");
expr("[new File(['a'], 'f', { lastModified: 1.5 }).lastModified, new File(['a'], 'f', { lastModified: Infinity }).lastModified, new File(['a'], 'f', { lastModified: true }).lastModified]");
expr("new File(['a'], 'f', 5).type");
expr("new File(new Blob(['ab']), 'f')");
expr("new File([, 'a'], 'f').size");
expr("new File([new Blob(['a']), new File(['bb'], 'g'), new Uint8Array(2), 5], 'f').size");
fn("var f = new File(['abc'], 'f', { lastModified: 7, type: 'A/B' }); var s = f.slice(1); return [s instanceof File, s.name, s.lastModified, s.type, s.size]");
expr("Object.keys(new File(['a'], 'f'))");
expr("JSON.stringify(new File(['a'], 'f'))");

expr("/^multipart\\/form-data; boundary=----WebKitFormBoundary[0-9a-f]{32}$/.test(new Response(new FormData()).headers.get('content-type'))");
expr("new Request('http://x/', { method: 'POST', body: new FormData() }).headers.get('content-type').split(';')[0]");
fn("var f = new FormData(); f.append('a', '1'); return /^multipart\\/form-data; boundary=----WebKitFormBoundary[0-9a-f]{32}$/.test(new Response(f).headers.get('content-type'))");
fn("var f = new FormData(); f.append('a', '1'); return [...new Response(f).headers].map(function (h) { return h[0] })");
// Os casos assíncronos (corpo multipart de `new Response(fd).text()`, com `filename=\"\"` para Blob sem nome e para nome
// vazio, `Content-Type: application/octet-stream` sem tipo, nome e filename com `\"`, CR e LF em %22, %0D, %0A; e
// `new Response(multipart, { headers }).formData()` com `ERR_FORMDATA_PARSE_ERROR` sem boundary) foram medidos à mão
// no bun 1.4.2 e ficam fora deste gerador síncrono.

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
