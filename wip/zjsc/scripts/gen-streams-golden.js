// Gera tests/golden/streams_bun.tsv: Web Streams medidas no bun 1.4.2 (`ReadableStream`, os leitores, `WritableStream`,
// `TransformStream`, as strategies e `Blob.prototype.stream`): descritores do global, `length`, `name`, chaves do
// protótipo, construtor com fonte subjacente (start/pull/cancel), `getReader`, `read`, `releaseLock`, `locked`, `cancel`,
// `tee`, `pipeTo`, `pipeThrough`, `Symbol.asyncIterator`, `ReadableStream.from`, ordem das microtasks (logs), erros.
// Sem rede, sem arquivo e sem caminho da máquina. O resultado é o valor de `R` depois de esvaziar as microtasks e de um
// turno de timer; cada programa roda duas vezes e as saídas têm de ser idênticas (senão o gerador falha).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-streams-golden.js > tests/golden/streams_bun.tsv
const { emitRow } = require("./golden-prelude.js");

// Promessas rejeitadas sem tratador são parte normal dos casos de erro (`try { rs.cancel() }` rejeita pela spec).
process.on("unhandledRejection", () => {});

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'undefined') return 'undefined'; if (typeof v === 'function') return 'function'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e && e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var L = [];\n" +
  "var DESC = function (o, k) { var d = Object.getOwnPropertyDescriptor(o, k); if (!d) return 'none'; " +
  "return [typeof d.value, typeof d.get, typeof d.set, d.writable, d.enumerable, d.configurable, typeof d.value === 'function' ? d.value.length + ':' + d.value.name : (d.get ? d.get.name : '')] };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = 'throw ' + E(e) }`);
// Corpo assíncrono: `body` usa `await` e `L.push`; o resultado é o log (ou o erro).
const abody = (body) =>
  programs.push(HELPER + `(async function () { ${body} })().then(function (v) { R = S(L.length ? L.concat([v]) : v) }, function (e) { R = 'rejeitou ' + E(e) + '|' + JSON.stringify(L) })`);

// Forma das classes.
const CLASSES = [
  "ReadableStream", "ReadableStreamDefaultReader", "ReadableStreamBYOBReader", "ReadableStreamDefaultController", "ReadableByteStreamController",
  "ReadableStreamBYOBRequest", "WritableStream", "WritableStreamDefaultWriter", "WritableStreamDefaultController", "TransformStream",
  "TransformStreamDefaultController", "ByteLengthQueuingStrategy", "CountQueuingStrategy", "TextEncoderStream", "TextDecoderStream",
  "CompressionStream", "DecompressionStream",
];
for (const N of CLASSES) {
  expr(`DESC(globalThis, '${N}')`);
  expr(`typeof ${N} === 'function' ? [${N}.length, ${N}.name, Object.getOwnPropertyNames(${N}), Reflect.ownKeys(${N}.prototype).map(String)] : 'ausente'`);
  expr(`typeof ${N} === 'function' ? [Object.getPrototypeOf(${N}.prototype) === Object.prototype, DESC(${N}.prototype, Symbol.toStringTag), DESC(${N}.prototype, 'constructor')] : 'ausente'`);
}
expr(`Object.getOwnPropertyNames(ReadableStream.prototype).map(function (k) { return [k, DESC(ReadableStream.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(ReadableStreamDefaultReader.prototype).map(function (k) { return [k, DESC(ReadableStreamDefaultReader.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(ReadableStreamBYOBReader.prototype).map(function (k) { return [k, DESC(ReadableStreamBYOBReader.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(ReadableStreamDefaultController.prototype).map(function (k) { return [k, DESC(ReadableStreamDefaultController.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(ReadableByteStreamController.prototype).map(function (k) { return [k, DESC(ReadableByteStreamController.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(WritableStream.prototype).map(function (k) { return [k, DESC(WritableStream.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(WritableStreamDefaultWriter.prototype).map(function (k) { return [k, DESC(WritableStreamDefaultWriter.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(WritableStreamDefaultController.prototype).map(function (k) { return [k, DESC(WritableStreamDefaultController.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(TransformStream.prototype).map(function (k) { return [k, DESC(TransformStream.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(TransformStreamDefaultController.prototype).map(function (k) { return [k, DESC(TransformStreamDefaultController.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(CountQueuingStrategy.prototype).map(function (k) { return [k, DESC(CountQueuingStrategy.prototype, k)] })`);
expr(`Object.getOwnPropertyNames(ByteLengthQueuingStrategy.prototype).map(function (k) { return [k, DESC(ByteLengthQueuingStrategy.prototype, k)] })`);
expr(`Object.getOwnPropertySymbols(ReadableStream.prototype).map(String)`);
expr(`ReadableStream.prototype[Symbol.asyncIterator] === ReadableStream.prototype.values`);
expr(`[typeof ReadableStream.from, ReadableStream.from && ReadableStream.from.length, Object.getOwnPropertyNames(ReadableStream)]`);
expr(`Object.prototype.toString.call(new ReadableStream())`);
expr(`Object.prototype.toString.call(new WritableStream())`);
expr(`Object.prototype.toString.call(new TransformStream())`);
expr(`Object.prototype.toString.call(new ReadableStream().getReader())`);
expr(`Object.prototype.toString.call(new WritableStream().getWriter())`);
expr(`Object.prototype.toString.call(new CountQueuingStrategy({ highWaterMark: 1 }))`);
expr(`Object.prototype.toString.call(new ReadableStream().values())`);
expr(`Object.getOwnPropertyNames(Object.getPrototypeOf(new ReadableStream().values()))`);
expr(`Object.getPrototypeOf(Object.getPrototypeOf(new ReadableStream().values())) === Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}.prototype))`);

// Chamada sem `new` e construtores.
for (const N of ["ReadableStream", "WritableStream", "TransformStream", "CountQueuingStrategy", "ByteLengthQueuingStrategy"]) {
  expr(`${N}()`);
}
for (const N of ["ReadableStreamDefaultReader", "ReadableStreamBYOBReader", "WritableStreamDefaultWriter"]) {
  expr(`new ${N}()`);
  expr(`new ${N}({})`);
}
for (const N of ["ReadableStreamDefaultController", "ReadableByteStreamController", "WritableStreamDefaultController", "TransformStreamDefaultController", "ReadableStreamBYOBRequest"]) {
  expr(`new ${N}()`);
}
expr(`[new ReadableStream().locked, new WritableStream().locked]`);
expr(`new ReadableStream(null).locked`);
expr(`new ReadableStream(5)`);
expr(`new ReadableStream({}, 5)`);
expr(`new ReadableStream({ type: 'x' })`);
expr(`new ReadableStream({ type: 'bytes' }).locked`);
expr(`new ReadableStream({ type: 'bytes' }, { highWaterMark: 1, size: function () { return 1 } })`);
expr(`new ReadableStream({ start: 5 })`);
expr(`new ReadableStream({ pull: 5 })`);
expr(`new ReadableStream({ cancel: 5 })`);
expr(`new ReadableStream({}, { highWaterMark: -1 })`);
expr(`new ReadableStream({}, { highWaterMark: NaN })`);
expr(`new ReadableStream({}, { size: 5 })`);
expr(`new ReadableStream({}, { highWaterMark: Infinity }).locked`);
expr(`new ReadableStream({ get start() { throw new RangeError('boom') } })`);
expr(`new ReadableStream({ start: function () { throw new RangeError('boom') } })`);
expr(`new WritableStream(5)`);
expr(`new WritableStream({ type: 'x' })`);
expr(`new WritableStream({ write: 5 })`);
expr(`new WritableStream({}, { highWaterMark: -1 })`);
expr(`new WritableStream({ start: function () { throw new RangeError('boom') } })`);
expr(`new TransformStream({ readableType: 'x' })`);
expr(`new TransformStream({ writableType: 'x' })`);
expr(`new TransformStream({ transform: 5 })`);
expr(`new TransformStream({}, { highWaterMark: -1 })`);
expr(`new TransformStream({}, {}, { highWaterMark: -1 })`);
expr(`(function(t){ return [typeof t.readable, typeof t.writable, t.readable.locked, t.writable.locked, Object.keys(t)] })(new TransformStream())`);

// Strategies.
expr(`new CountQueuingStrategy()`);
expr(`new CountQueuingStrategy({})`);
expr(`new CountQueuingStrategy({ highWaterMark: 'a' })`);
expr(`(function(s){ return [s.highWaterMark, s.size.length, s.size.name, s.size(), s.size({}), s.size === new CountQueuingStrategy({ highWaterMark: 1 }).size, Object.keys(s), JSON.stringify(s)] })(new CountQueuingStrategy({ highWaterMark: 3 }))`);
expr(`new CountQueuingStrategy({ highWaterMark: '7' }).highWaterMark`);
expr(`new CountQueuingStrategy({ highWaterMark: -1 }).highWaterMark`);
expr(`new CountQueuingStrategy({ highWaterMark: NaN }).highWaterMark`);
expr(`new CountQueuingStrategy({ highWaterMark: undefined }).highWaterMark`);
expr(`new ByteLengthQueuingStrategy()`);
expr(`(function(s){ return [s.highWaterMark, s.size.length, s.size.name, s.size({ byteLength: 5 }), s.size({}), Object.keys(s)] })(new ByteLengthQueuingStrategy({ highWaterMark: 4 }))`);
expr(`new ByteLengthQueuingStrategy({ highWaterMark: 4 }).size(null)`);
expr(`new ByteLengthQueuingStrategy({ highWaterMark: 4 }).size()`);
expr(`(function(s){ s.highWaterMark = 9; return s.highWaterMark })(new CountQueuingStrategy({ highWaterMark: 1 }))`);
expr(`Object.getOwnPropertyDescriptor(CountQueuingStrategy.prototype, 'highWaterMark').get.call({})`);
expr(`Object.getOwnPropertyDescriptor(CountQueuingStrategy.prototype, 'size').get.call({})`);

// Chamadas com `this` alheio.
for (const [cls, ms] of [
  ["ReadableStream", ["cancel", "getReader", "pipeThrough", "pipeTo", "tee", "values"]],
  ["ReadableStreamDefaultReader", ["cancel", "read", "releaseLock"]],
  ["WritableStream", ["abort", "close", "getWriter"]],
  ["WritableStreamDefaultWriter", ["abort", "close", "releaseLock", "write"]],
  ["TransformStream", []],
]) {
  for (const m of ms) {
    expr(`(function(){ try { var r = ${cls}.prototype.${m}.call({}); if (r && typeof r.then === 'function') return r.then(function(){}, function(){}) && 'promise'; return 'ok' } catch (e) { return 'throw ' + E(e) } })()`);
  }
}
expr(`Object.getOwnPropertyDescriptor(ReadableStream.prototype, 'locked').get.call({})`);
expr(`Object.getOwnPropertyDescriptor(WritableStream.prototype, 'locked').get.call({})`);
expr(`Object.getOwnPropertyDescriptor(TransformStream.prototype, 'readable').get.call({})`);
abody(`var p = Object.getOwnPropertyDescriptor(ReadableStreamDefaultReader.prototype, 'closed').get.call({}); return await p.then(function () { return 'ok' }, function (e) { return 'rej ' + E(e) })`);
abody(`var g = Object.getOwnPropertyDescriptor(WritableStreamDefaultWriter.prototype, 'closed').get; var p = g.call({}); return await p.then(function () { return 'ok' }, function (e) { return 'rej ' + E(e) })`);
abody(`var g = Object.getOwnPropertyDescriptor(WritableStreamDefaultWriter.prototype, 'ready').get; var p = g.call({}); return await p.then(function () { return 'ok' }, function (e) { return 'rej ' + E(e) })`);

// Leitura básica, ordem de start/pull.
abody(`var rs = new ReadableStream({ start: function (c) { L.push('start'); c.enqueue(1); c.enqueue(2); c.close() }, pull: function () { L.push('pull') } });
  var r = rs.getReader(); L.push(rs.locked); var a = await r.read(); var b = await r.read(); var c = await r.read(); return [a, b, c, Object.keys(a), Object.keys(c)]`);
abody(`var n = 0; var rs = new ReadableStream({ start: function () { L.push('start') }, pull: function (c) { L.push('pull' + n); c.enqueue(n++); if (n == 3) c.close() }, cancel: function () { L.push('cancel') } });
  await Promise.resolve(); await Promise.resolve(); L.push('apos-start'); var r = rs.getReader(); var out = []; for (;;) { var x = await r.read(); if (x.done) break; out.push(x.value) } return out`);
abody(`var rs = new ReadableStream({ pull: function (c) { L.push('pull') } }, { highWaterMark: 0 }); await Promise.resolve(); await Promise.resolve(); L.push('hwm0'); var r = rs.getReader(); var p = r.read(); await Promise.resolve(); await Promise.resolve(); L.push('apos-read'); return 'fim'`);
abody(`var rs = new ReadableStream({ pull: function (c) { L.push('pull'); c.enqueue('a') } }, { highWaterMark: 3 }); await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); L.push('desiredSize'); return 'fim'`);
abody(`var ctrl; var rs = new ReadableStream({ start: function (c) { ctrl = c } }, { highWaterMark: 2 }); var d = [ctrl.desiredSize]; ctrl.enqueue('a'); d.push(ctrl.desiredSize); ctrl.enqueue('b'); d.push(ctrl.desiredSize); ctrl.enqueue('c'); d.push(ctrl.desiredSize); ctrl.close(); d.push(ctrl.desiredSize); return d`);
abody(`var ctrl; var rs = new ReadableStream({ start: function (c) { ctrl = c } }); ctrl.close(); try { ctrl.enqueue(1) } catch (e) { L.push(E(e)) } try { ctrl.close() } catch (e) { L.push(E(e)) } return ctrl.desiredSize`);
abody(`var ctrl; var rs = new ReadableStream({ start: function (c) { ctrl = c } }); ctrl.error(new RangeError('e1')); try { ctrl.enqueue(1) } catch (e) { L.push(E(e)) } try { ctrl.close() } catch (e) { L.push(E(e)) } L.push(ctrl.desiredSize); ctrl.error('x'); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ctrl; var rs = new ReadableStream({ start: function (c) { ctrl = c } }); L.push(Object.prototype.toString.call(ctrl), Object.keys(ctrl), typeof ctrl.enqueue, ctrl.constructor.name); return ctrl instanceof ReadableStreamDefaultController`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(undefined); c.enqueue(null); c.enqueue({ a: 1 }); c.close() } }); var r = rs.getReader(); return [await r.read(), await r.read(), await r.read(), await r.read()]`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a', 'b') } }, { size: function (x) { L.push('size ' + x); return 2 }, highWaterMark: 5 }); return rs.getReader().read()`);
abody(`var rs = new ReadableStream({ start: function (c) { try { c.enqueue('a') } catch (e) { L.push(E(e)) } } }, { size: function () { throw new RangeError('sz') } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { try { c.enqueue('a') } catch (e) { L.push(E(e)) } } }, { size: function () { return -1 } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { try { c.enqueue('a') } catch (e) { L.push(E(e)) } } }, { size: function () { return Infinity } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function () { return new Promise(function (r) { L.push('start-p'); r() }) }, pull: function () { L.push('pull') } }); await new Promise(function (r) { setTimeout(r, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function () { return Promise.reject(new RangeError('st')) } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1) }, pull: function () { throw new RangeError('pl') } }, { highWaterMark: 2 }); var r = rs.getReader(); L.push(await r.read()); return await r.read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ pull: function () { return Promise.reject(new RangeError('pl2')) } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ pull: function (c) { L.push('pull'); return new Promise(function (r) { setTimeout(function () { c.enqueue('late'); r() }, 1) }) } }); var r = rs.getReader(); var x = await r.read(); return x`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a') }, pull: function (c) { L.push('pull') } }); var r = rs.getReader(); var p1 = r.read(); var p2 = r.read(); var p3 = r.read(); L.push('lidos'); await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); return await Promise.race([p1, Promise.resolve('p1'), ]).then(function (v) { return v })`);

// Leitor: bloqueio, releaseLock, closed, cancel.
abody(`var rs = new ReadableStream(); var r = rs.getReader(); var e1; try { rs.getReader() } catch (e) { e1 = E(e) } var e2; try { rs.cancel() } catch (e) { e2 = E(e) } return [rs.locked, e1, e2, await rs.cancel().catch(function (e) { return 'rej ' + E(e) })]`);
abody(`var rs = new ReadableStream(); var r = rs.getReader(); r.releaseLock(); L.push(rs.locked); var p = r.read(); return await p.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); var r = rs.getReader(); var p = r.read(); r.releaseLock(); L.push(rs.locked); return await p.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); var r = rs.getReader(); var c = r.closed; r.releaseLock(); L.push(await c.catch(function (e) { return 'rej ' + E(e) })); return await r.closed.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.close() } }); var r = rs.getReader(); L.push(await r.closed); r.releaseLock(); return await r.closed.then(function (v) { return 'ok ' + v }, function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('er')) } }); var r = rs.getReader(); return await r.closed.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ cancel: function (reason) { L.push('cancel ' + S(reason)); return 'ignorado' } }); var r = rs.getReader(); var v = await r.cancel('why'); L.push(S(v)); return await r.read()`);
abody(`var rs = new ReadableStream({ cancel: function (reason) { throw new RangeError('cx') } }); return await rs.cancel('r').catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ cancel: function () { L.push('cancel') } }); await rs.cancel(); await rs.cancel(); return rs.locked`);
abody(`var rs = new ReadableStream({ start: function (c) { c.close() }, cancel: function () { L.push('cancel') } }); await rs.cancel(); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('qq')) } }); return await rs.cancel().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); var r = rs.getReader(); return [typeof r.read, r.constructor.name, Object.keys(r), r instanceof ReadableStreamDefaultReader, r.read.length, r.cancel.length]`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1) } }); var r = rs.getReader({ mode: undefined }); return r.constructor.name`);
abody(`var rs = new ReadableStream(); try { rs.getReader({ mode: 'x' }) } catch (e) { return E(e) }`);
abody(`var rs = new ReadableStream(); try { rs.getReader({ mode: 'byob' }) } catch (e) { return E(e) }`);
abody(`var rs = new ReadableStream(); try { rs.getReader(5) } catch (e) { return E(e) }`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var r = rs.getReader(); await r.read(); r.releaseLock(); var r2 = rs.getReader(); return await r2.read()`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2) } }); var r = rs.getReader(); await r.read(); r.releaseLock(); var r2 = rs.getReader(); return await r2.read()`);

// Leitor BYOB e fluxo de bytes.
abody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { L.push(Object.prototype.toString.call(c), c.byobRequest, c.desiredSize); c.enqueue(new Uint8Array([1, 2, 3])); c.close() } }); var r = rs.getReader(); var x = await r.read(); return [x.done, x.value.constructor.name, Array.from(x.value), x.value.buffer.byteLength]`);
abody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(new Uint8Array([1, 2, 3, 4])); c.close() } }); var r = rs.getReader({ mode: 'byob' }); L.push(r.constructor.name); var x = await r.read(new Uint8Array(3)); var y = await r.read(new Uint8Array(3)); var z = await r.read(new Uint8Array(3)); return [x.done, Array.from(x.value), y.done, Array.from(y.value), z.done, z.value && z.value.byteLength]`);
abody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(new Uint8Array(0)) } catch (e) { L.push(E(e)) } try { await r.read(5) } catch (e) { L.push(E(e)) } try { await r.read({}) } catch (e) { L.push(E(e)) } return await r.read().catch(function (e) { return E(e) })`);
abody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 4, pull: function (c) { var b = c.byobRequest; L.push(b && b.view.byteLength); b.view[0] = 9; b.respond(1) } }); var r = rs.getReader(); var x = await r.read(); return [x.done, Array.from(x.value)]`);
abody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var b = c.byobRequest; L.push(Object.prototype.toString.call(b), b.view.constructor.name, b.view.byteLength); b.view.set([7, 8]); b.respond(2) } }); var r = rs.getReader({ mode: 'byob' }); var x = await r.read(new Uint16Array(2)); return [x.done, x.value.constructor.name, x.value.byteLength, Array.from(x.value)]`);
abody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { try { c.enqueue(5) } catch (e) { L.push(E(e)) } try { c.enqueue(new Uint8Array(0)) } catch (e) { L.push(E(e)) } try { c.enqueue(new ArrayBuffer(2)) } catch (e) { L.push(E(e)) } } }); return 'fim'`);
abody(`var rs = new ReadableStream({ type: 'bytes' }, { highWaterMark: 5 }); var r = rs.getReader(); return rs.locked`);
abody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(new Uint8Array([1, 2])) } }); var [a, b] = rs.tee(); var x = await a.getReader().read(); var y = await b.getReader().read(); return [Array.from(x.value), Array.from(y.value), x.value === y.value, x.value.buffer === y.value.buffer]`);

// tee.
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b'); c.close() } }); var t = rs.tee(); L.push(Array.isArray(t), t.length, rs.locked, t[0] instanceof ReadableStream, t[0] === t[1]); var r0 = t[0].getReader(); var r1 = t[1].getReader(); var o0 = []; var o1 = []; for (;;) { var x = await r0.read(); if (x.done) break; o0.push(x.value) } for (;;) { var y = await r1.read(); if (y.done) break; o1.push(y.value) } return [o0, o1]`);
abody(`var obj = { k: 1 }; var rs = new ReadableStream({ start: function (c) { c.enqueue(obj); c.close() } }); var [a, b] = rs.tee(); var x = await a.getReader().read(); var y = await b.getReader().read(); return [x.value === obj, y.value === obj, x.value === y.value]`);
abody(`var rs = new ReadableStream({ cancel: function (r) { L.push('cancel ' + S(r)) } }); var [a, b] = rs.tee(); await a.cancel('A'); L.push('a-cancelado'); await b.cancel('B'); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('te')) } }); var [a, b] = rs.tee(); L.push(await a.getReader().read().catch(function (e) { return 'rej ' + E(e) })); return await b.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); rs.getReader(); try { rs.tee() } catch (e) { return E(e) }`);
abody(`var rs = new ReadableStream({ pull: function (c) { L.push('pull'); c.enqueue(1) } }, { highWaterMark: 0 }); var [a, b] = rs.tee(); await Promise.resolve(); await Promise.resolve(); L.push('tee'); await a.getReader().read(); await Promise.resolve(); await Promise.resolve(); return 'fim'`);

// Iterador assíncrono, values.
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2); c.close() } }); var out = []; for await (var v of rs) out.push(v); L.push(rs.locked); return out`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2); c.close() }, cancel: function (r) { L.push('cancel ' + S(r)) } }); for await (var v of rs) { break } L.push(rs.locked); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2); c.close() }, cancel: function (r) { L.push('cancel ' + S(r)) } }); try { for await (var v of rs.values({ preventCancel: true })) { break } } catch (e) {} L.push(rs.locked); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var it = rs.values(); L.push(rs.locked, typeof it.next, typeof it.return, it[Symbol.asyncIterator]() === it, Object.keys(it)); var a = await it.next(); var b = await it.next(); var c = await it.next(); return [a, b, c, rs.locked]`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2) }, cancel: function (r) { L.push('cancel ' + S(r)) } }); var it = rs.values(); await it.next(); var r = await it.return('fim'); return [r, rs.locked]`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('it')) } }); try { for await (var v of rs) {} } catch (e) { L.push(E(e)) } return rs.locked`);
abody(`var rs = new ReadableStream(); rs.getReader(); try { rs.values() } catch (e) { return E(e) }`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var it = rs.values(); var p1 = it.next(); var p2 = it.next(); return [await p1, await p2]`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var it = rs.values(); var p1 = it.next(); var p2 = it.return('r'); var p3 = it.next(); return [await p1, await p2, await p3]`);

// ReadableStream.from.
abody(`var rs = ReadableStream.from([1, 2, 3]); var out = []; for await (var v of rs) out.push(v); return [out, rs instanceof ReadableStream]`);
abody(`var rs = ReadableStream.from((function* () { yield 'a'; yield 'b' })()); var r = rs.getReader(); return [await r.read(), await r.read(), await r.read()]`);
abody(`var rs = ReadableStream.from((async function* () { L.push('inicio'); yield 1; L.push('meio'); yield 2; L.push('fim') })()); var out = []; for await (var v of rs) out.push(v); return out`);
abody(`var rs = ReadableStream.from('ab'); var out = []; for await (var v of rs) out.push(v); return out`);
abody(`var rs = ReadableStream.from([Promise.resolve(1), 2]); var out = []; for await (var v of rs) out.push(v); return out`);
abody(`try { ReadableStream.from(5) } catch (e) { L.push(E(e)) } try { ReadableStream.from(null) } catch (e) { L.push(E(e)) } try { ReadableStream.from({}) } catch (e) { L.push(E(e)) } try { ReadableStream.from() } catch (e) { L.push(E(e)) } return 'fim'`);
abody(`var it = { [Symbol.asyncIterator]: function () { return { next: function () { L.push('next'); return Promise.resolve({ done: false, value: 1 }) }, return: function (r) { L.push('return ' + S(r)); return Promise.resolve({}) } } } }; var rs = ReadableStream.from(it); await rs.cancel('c'); var r = rs.getReader(); return 'fim'`);
abody(`var rs = ReadableStream.from({ [Symbol.iterator]: function () { var i = 0; return { next: function () { return { done: i >= 2, value: i++ } } } } }); var out = []; for await (var v of rs) out.push(v); return out`);
abody(`var rs = ReadableStream.from([1]); var r = rs.getReader(); var res = []; res.push(await r.read()); res.push(await r.read()); return [res, (await r.closed)]`);
abody(`var rs = ReadableStream.from({ [Symbol.asyncIterator]: function () { return { next: function () { return 5 } } } }); return await rs.getReader().read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = ReadableStream.from([1, 2, 3]); L.push(rs.locked); var [a, b] = rs.tee(); return [await a.getReader().read(), await b.getReader().read()]`);

// WritableStream.
abody(`var ws = new WritableStream({ start: function () { L.push('start') }, write: function (chunk, c) { L.push('write ' + S(chunk) + ' ' + Object.prototype.toString.call(c)) }, close: function () { L.push('close') }, abort: function (r) { L.push('abort') } }); var w = ws.getWriter(); L.push(ws.locked, w.desiredSize); await w.ready; L.push('ready'); var p = w.write('a'); L.push(w.desiredSize); await p; await w.write('b'); await w.close(); L.push(w.desiredSize); return await w.closed`);
abody(`var ws = new WritableStream(); var w = ws.getWriter(); return [w.constructor.name, Object.keys(w), w.desiredSize, await w.ready, typeof w.closed.then, w.write.length, w.close.length, w.abort.length, w.releaseLock.length]`);
abody(`var ws = new WritableStream({ write: function (c) { L.push('write ' + c); return new Promise(function (r) { setTimeout(r, 1) }) } }, { highWaterMark: 2 }); var w = ws.getWriter(); var d = [w.desiredSize]; var p1 = w.write(1); d.push(w.desiredSize); var p2 = w.write(2); d.push(w.desiredSize); var p3 = w.write(3); d.push(w.desiredSize); await Promise.all([p1, p2, p3]); d.push(w.desiredSize); return d`);
abody(`var ws = new WritableStream({ write: function (c) { throw new RangeError('w') } }); var w = ws.getWriter(); var p = w.write(1); L.push(await p.catch(function (e) { return 'rej ' + E(e) })); L.push(await w.closed.catch(function (e) { return 'rej ' + E(e) })); L.push(await w.ready.catch(function (e) { return 'rej ' + E(e) })); return await w.write(2).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream({ abort: function (r) { L.push('abort ' + S(r)); return 'x' } }); var w = ws.getWriter(); var v = await w.abort('why'); L.push(S(v)); L.push(await w.closed.catch(function (e) { return 'rej ' + E(e) })); return await w.write(1).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream(); var w = ws.getWriter(); await w.close(); L.push(await w.close().catch(function (e) { return 'rej ' + E(e) })); L.push(await w.abort().catch(function (e) { return 'rej ' + E(e) })); return await w.write(1).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream(); var w = ws.getWriter(); w.releaseLock(); L.push(ws.locked); L.push(await w.closed.catch(function (e) { return 'rej ' + E(e) })); L.push(await w.ready.catch(function (e) { return 'rej ' + E(e) })); L.push(w.desiredSize === null); return await w.write(1).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream(); ws.getWriter(); L.push(await ws.close().catch(function (e) { return 'rej ' + E(e) })); L.push(await ws.abort().catch(function (e) { return 'rej ' + E(e) })); try { ws.getWriter() } catch (e) { L.push(E(e)) } return ws.locked`);
abody(`var c; var ws = new WritableStream({ start: function (ctrl) { c = ctrl } }); var w = ws.getWriter(); L.push(Object.prototype.toString.call(c), Object.keys(c), typeof c.error, c.signal && c.signal.constructor.name, c.signal && c.signal.aborted); c.error(new RangeError('ce')); L.push(await w.closed.catch(function (e) { return 'rej ' + E(e) })); return await w.write(1).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var c; var ws = new WritableStream({ start: function (ctrl) { c = ctrl; c.signal.addEventListener('abort', function () { L.push('sinal') }) }, abort: function () { L.push('abort') } }); await ws.abort('r'); L.push(c.signal.aborted, S(c.signal.reason)); return 'fim'`);
abody(`var ws = new WritableStream({ write: function () {} }, { size: function (x) { L.push('size ' + x); return 1 }, highWaterMark: 4 }); var w = ws.getWriter(); await w.write('a'); L.push(w.desiredSize); return 'fim'`);
abody(`var ws = new WritableStream({ close: function () { return Promise.reject(new RangeError('cl')) } }); var w = ws.getWriter(); L.push(await w.close().catch(function (e) { return 'rej ' + E(e) })); return await w.closed.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream({ start: function () { return Promise.reject(new RangeError('ws')) } }); var w = ws.getWriter(); L.push(await w.ready.catch(function (e) { return 'rej ' + E(e) })); return await w.closed.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ws = new WritableStream({ write: function () { L.push('w') } }); var w = ws.getWriter(); w.write('a'); w.write('b'); L.push('sync'); w.releaseLock(); L.push('liberado'); await new Promise(function (r) { setTimeout(r, 0) }); return 'fim'`);
abody(`var ws = new WritableStream({ write: function (c) { L.push('w' + c); return new Promise(function (r) { setTimeout(r, 1) }) } }); var w = ws.getWriter(); w.write(1); w.write(2); var p = w.close(); L.push('close-chamado'); await p; return 'fim'`);

// TransformStream.
abody(`var ts = new TransformStream({ start: function (c) { L.push('start ' + Object.prototype.toString.call(c)) }, transform: function (chunk, c) { L.push('transform ' + chunk); c.enqueue(chunk.toUpperCase()); c.enqueue(chunk + '!') }, flush: function (c) { L.push('flush'); c.enqueue('fim') } }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write('a'); w.write('b'); w.close(); var out = []; for (;;) { var x = await r.read(); if (x.done) break; out.push(x.value) } return out`);
abody(`var ts = new TransformStream(); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); var p = w.write('x'); L.push(w.desiredSize); var x = await r.read(); await p; return [x, w.desiredSize]`);
abody(`var ts = new TransformStream(); var w = ts.writable.getWriter(); L.push(w.desiredSize); var p = w.write('x'); L.push(w.desiredSize); var p2 = w.write('y'); L.push(w.desiredSize); return 'fim'`);
abody(`var c; var ts = new TransformStream({ start: function (ctrl) { c = ctrl } }); L.push(c.desiredSize, Object.keys(c), c.constructor.name); c.enqueue('a'); L.push(c.desiredSize); c.terminate(); L.push(c.desiredSize); var r = ts.readable.getReader(); L.push(await r.read()); L.push(await r.read()); return await ts.writable.getWriter().write('z').catch(function (e) { return 'rej ' + E(e) })`);
abody(`var c; var ts = new TransformStream({ start: function (ctrl) { c = ctrl } }); c.error(new RangeError('te')); return [await ts.readable.getReader().read().catch(function (e) { return 'rej ' + E(e) }), await ts.writable.getWriter().write(1).catch(function (e) { return 'rej ' + E(e) })]`);
abody(`var ts = new TransformStream({ transform: function () { throw new RangeError('tr') } }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); var p = w.write(1); L.push(await p.catch(function (e) { return 'rej ' + E(e) })); return await r.read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ts = new TransformStream({ flush: function () { throw new RangeError('fl') } }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); var p = w.close(); L.push(await p.catch(function (e) { return 'rej ' + E(e) })); return await r.read().catch(function (e) { return 'rej ' + E(e) })`);
abody(`var ts = new TransformStream({ transform: function (ch, c) { return new Promise(function (r) { setTimeout(function () { c.enqueue(ch * 2); r() }, 1) }) } }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write(1); w.write(2); w.close(); var out = []; for (;;) { var x = await r.read(); if (x.done) break; out.push(x.value) } return out`);
abody(`var ts = new TransformStream({ start: function () { return new Promise(function (r) { setTimeout(r, 1) }) } }); var w = ts.writable.getWriter(); L.push(w.desiredSize); await w.ready; return w.desiredSize`);
abody(`var ts = new TransformStream({}, { highWaterMark: 3 }, { highWaterMark: 0 }); var w = ts.writable.getWriter(); return [w.desiredSize, (function(c){ return c })(await w.ready)]`);
abody(`var ts = new TransformStream({}, { highWaterMark: 3 }, { highWaterMark: 2 }); var w = ts.writable.getWriter(); return w.desiredSize`);
abody(`var ts = new TransformStream({ transform: function (ch, c) { c.enqueue(ch) }, cancel: function (r) { L.push('cancel ' + S(r)) } }); await ts.readable.cancel('rc'); L.push(await ts.writable.getWriter().write(1).catch(function (e) { return 'rej ' + E(e) })); return 'fim'`);
abody(`var ts = new TransformStream({ flush: function () { L.push('flush') }, cancel: function () { L.push('cancel') } }); await ts.writable.abort('wa'); L.push('abortado'); return await ts.readable.getReader().read().catch(function (e) { return 'rej ' + S(e) })`);
abody(`var ts = new TransformStream({ transform: function (c, ctrl) { ctrl.enqueue(c) }, readableType: undefined, writableType: undefined }); return [ts.readable instanceof ReadableStream, ts.writable instanceof WritableStream]`);

// pipeTo / pipeThrough.
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b'); c.close() } }); var ws = new WritableStream({ write: function (c) { L.push('write ' + c) }, close: function () { L.push('close') } }); var v = await rs.pipeTo(ws); L.push(rs.locked, ws.locked); return S(v)`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.close() } }); var ws = new WritableStream({ write: function (c) { L.push('write ' + c) }, close: function () { L.push('close') } }); await rs.pipeTo(ws, { preventClose: true }); L.push(ws.locked); var w = ws.getWriter(); await w.write('z'); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('src')) } }); var ws = new WritableStream({ abort: function (r) { L.push('abort ' + E(r)) } }); return await rs.pipeTo(ws).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.error(new RangeError('src')) } }); var ws = new WritableStream({ abort: function (r) { L.push('abort') } }); return await rs.pipeTo(ws, { preventAbort: true }).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b') }, cancel: function (r) { L.push('cancel ' + E(r)) } }); var ws = new WritableStream({ write: function () { throw new RangeError('dst') } }); return await rs.pipeTo(ws).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b') }, cancel: function (r) { L.push('cancel') } }); var ws = new WritableStream({ write: function () { throw new RangeError('dst') } }); return await rs.pipeTo(ws, { preventCancel: true }).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); var ws = new WritableStream(); var ac = new AbortController(); var p = rs.pipeTo(ws, { signal: ac.signal }); ac.abort(); return await p.catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ cancel: function (r) { L.push('cancel ' + S(r)) } }); var ws = new WritableStream({ abort: function (r) { L.push('abort ' + S(r)) } }); var ac = new AbortController(); var p = rs.pipeTo(ws, { signal: ac.signal }); ac.abort('mot'); return await p.catch(function (e) { return 'rej ' + S(e) })`);
abody(`var rs = new ReadableStream(); var ws = new WritableStream(); var ac = new AbortController(); ac.abort(); return await rs.pipeTo(ws, { signal: ac.signal }).catch(function (e) { return 'rej ' + E(e) + ' ' + rs.locked + ' ' + ws.locked })`);
abody(`var rs = new ReadableStream(); rs.getReader(); var ws = new WritableStream(); return await rs.pipeTo(ws).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream(); var ws = new WritableStream(); ws.getWriter(); return await rs.pipeTo(ws).catch(function (e) { return 'rej ' + E(e) })`);
abody(`return await new ReadableStream().pipeTo({}).catch(function (e) { return 'rej ' + E(e) })`);
abody(`return await new ReadableStream().pipeTo().catch(function (e) { return 'rej ' + E(e) })`);
abody(`return await new ReadableStream().pipeTo(new WritableStream(), { signal: {} }).catch(function (e) { return 'rej ' + E(e) })`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b'); c.close() } }); var out = rs.pipeThrough(new TransformStream({ transform: function (c, t) { t.enqueue(c + c) } })); L.push(out instanceof ReadableStream, rs.locked, out.locked); var r = out.getReader(); var o = []; for (;;) { var x = await r.read(); if (x.done) break; o.push(x.value) } return o`);
abody(`try { new ReadableStream().pipeThrough({}) } catch (e) { L.push(E(e)) } try { new ReadableStream().pipeThrough({ readable: 1, writable: new WritableStream() }) } catch (e) { L.push(E(e)) } try { new ReadableStream().pipeThrough() } catch (e) { L.push(E(e)) } var rs = new ReadableStream(); rs.getReader(); try { rs.pipeThrough(new TransformStream()) } catch (e) { L.push(E(e)) } return 'fim'`);
abody(`var rs = new ReadableStream(); var ts = new TransformStream(); ts.writable.getWriter(); try { rs.pipeThrough(ts) } catch (e) { L.push(E(e)) } return rs.locked`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var o = { readable: new ReadableStream(), writable: new WritableStream() }; var ret = rs.pipeThrough(o); return ret === o.readable`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('x'); c.close() } }); var ws = new WritableStream({ write: function (c) { L.push('write') } }); L.push('antes'); var p = rs.pipeTo(ws); L.push('depois'); await p; return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('x'); c.close() } }); var ts = new TransformStream(); var ws = new WritableStream({ write: function (c) { L.push('ws ' + c) } }); await rs.pipeThrough(ts).pipeTo(ws); return 'fim'`);

// Blob.prototype.stream e consumo de Response/Request (sem rede).
expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(Blob.prototype, 'stream'))`);
expr(`new Blob(['abc']).stream() instanceof ReadableStream`);
expr(`(function(s){ return [s.locked, Object.prototype.toString.call(s), Object.keys(s), s.constructor === ReadableStream, Object.getPrototypeOf(s) === ReadableStream.prototype] })(new Blob(['abc']).stream())`);
expr(`new Blob(['abc']).stream() === new Blob(['abc']).stream()`);
expr(`(function(b){ return b.stream() === b.stream() })(new Blob(['abc']))`);
expr(`Blob.prototype.stream.call({})`);
expr(`Blob.prototype.stream.call(null)`);
abody(`var s = new Blob(['hello world']).stream(); var r = s.getReader(); var x = await r.read(); var y = await r.read(); return [x.done, x.value.constructor.name, x.value.byteLength, Array.from(x.value).slice(0, 5), y.done, y.value]`);
abody(`var s = new Blob([]).stream(); var r = s.getReader(); var x = await r.read(); return [x.done, x.value]`);
abody(`var s = new Blob(['a', new Uint8Array([98, 99]), new Blob(['d'])]).stream(); var r = s.getReader(); var o = []; var n = 0; for (;;) { var x = await r.read(); n++; if (x.done) break; o.push(Array.from(x.value)) } return [o, n]`);
abody(`var s = new Blob(['abc']).stream(); var r = s.getReader({ mode: 'byob' }); L.push(r.constructor.name); var x = await r.read(new Uint8Array(8)); var y = await r.read(new Uint8Array(8)); return [x.done, Array.from(x.value), x.value.byteLength, y.done, y.value.byteLength]`);
abody(`var s = new Blob(['abc']).stream(); var r = s.getReader({ mode: 'byob' }); var x = await r.read(new Uint8Array(2)); var y = await r.read(new Uint8Array(2)); return [Array.from(x.value), Array.from(y.value)]`);
abody(`var s = new Blob(['hello']).stream(); var out = []; for await (var c of s) out.push(Array.from(c)); return out`);
abody(`var s = new Blob(['hello']).stream(); await s.cancel('x'); var r = s.getReader(); return await r.read()`);
abody(`var s = new Blob(['hello']).stream(); var [a, b] = s.tee(); var x = await a.getReader().read(); var y = await b.getReader().read(); return [Array.from(x.value), Array.from(y.value)]`);
abody(`var s = new Blob(['hello']).stream(); var o = []; await s.pipeTo(new WritableStream({ write: function (c) { o.push(c.constructor.name + ':' + c.byteLength) }, close: function () { o.push('close') } })); return o`);
abody(`var s = new Blob(['hello']).pipe && 1; var out = new Blob(['hello']).stream().pipeThrough(new TextDecoderStream()); var r = out.getReader(); var o = []; for (;;) { var x = await r.read(); if (x.done) break; o.push(x.value) } return o`);
abody(`var big = new Uint8Array(200000); for (var i = 0; i < big.length; i++) big[i] = i & 255; var s = new Blob([big]).stream(); var r = s.getReader(); var n = 0, tot = 0; for (;;) { var x = await r.read(); if (x.done) break; n++; tot += x.value.byteLength } return [n > 0, tot]`);
abody(`var s = new Blob(['abc']).slice(1).stream(); var r = s.getReader(); return Array.from((await r.read()).value)`);
abody(`var b = new Blob(['abc']); var s1 = b.stream(); var s2 = b.stream(); return [Array.from((await s1.getReader().read()).value), Array.from((await s2.getReader().read()).value)]`);
abody(`var res = new Response('hello'); L.push(res.body instanceof ReadableStream, res.bodyUsed, res.body.locked); var r = res.body.getReader(); var x = await r.read(); L.push(x.value.constructor.name, Array.from(x.value)); return [res.bodyUsed, (await r.read()).done]`);
abody(`var res = new Response(new ReadableStream({ start: function (c) { c.enqueue(new TextEncoder().encode('xy')); c.close() } })); return await res.text()`);
abody(`var res = new Response(null); return [res.body, res.bodyUsed]`);
abody(`var res = new Response('abc'); var t = await res.text(); try { res.body.getReader() } catch (e) { L.push(E(e)) } return [t, res.bodyUsed, res.body && res.body.locked]`);
abody(`var res = new Response('abc'); var r = res.body.getReader(); var p = res.text().catch(function (e) { return 'rej ' + E(e) }); return await p`);
abody(`var req = new Request('http://x.invalid/', { method: 'POST', body: 'abc' }); L.push(req.body instanceof ReadableStream); return await req.text()`);

// Codificadores e compressão em fluxo (sem rede).
abody(`var ts = new TextEncoderStream(); L.push(ts.encoding, ts.readable instanceof ReadableStream, ts.writable instanceof WritableStream, Object.prototype.toString.call(ts)); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write('h\\u00e9'); w.close(); var x = await r.read(); return [Array.from(x.value), (await r.read()).done]`);
abody(`var ts = new TextDecoderStream(); L.push(ts.encoding, ts.fatal, ts.ignoreBOM, Object.prototype.toString.call(ts)); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write(new Uint8Array([0xc3])); w.write(new Uint8Array([0xa9])); w.close(); var o = []; for (;;) { var x = await r.read(); if (x.done) break; o.push(x.value) } return o`);
abody(`var cs = new CompressionStream('gzip'); L.push(Object.prototype.toString.call(cs), cs.readable instanceof ReadableStream); var ds = new DecompressionStream('gzip'); var w = cs.writable.getWriter(); w.write(new TextEncoder().encode('hello hello hello')); w.close(); var out = cs.readable.pipeThrough(ds); var r = out.getReader(); var o = []; for (;;) { var x = await r.read(); if (x.done) break; o.push(new TextDecoder().decode(x.value)) } return o.join('')`);
abody(`try { new CompressionStream('x') } catch (e) { L.push(E(e)) } try { new CompressionStream() } catch (e) { L.push(E(e)) } try { new DecompressionStream('') } catch (e) { L.push(E(e)) } return ['gzip', 'deflate', 'deflate-raw'].map(function (f) { try { new CompressionStream(f); return 'ok' } catch (e) { return E(e) } })`);

// Ordem de microtasks observável entre promessas e streams.
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1) } }); var r = rs.getReader(); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }); r.read().then(function () { L.push('read') }); queueMicrotask(function () { L.push('qm') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var r = rs.getReader(); r.closed.then(function () { L.push('closed') }); r.read().then(function () { L.push('read1') }); r.read().then(function () { L.push('read2') }); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var ws = new WritableStream({ write: function (c) { L.push('write ' + c) }, close: function () { L.push('close') } }); var w = ws.getWriter(); w.write(1).then(function () { L.push('w1') }); w.close().then(function () { L.push('c') }); w.closed.then(function () { L.push('closed') }); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }).then(function () { L.push('p5') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var ts = new TransformStream({ transform: function (c, t) { L.push('t ' + c); t.enqueue(c) }, flush: function () { L.push('flush') } }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); r.read().then(function (x) { L.push('r1 ' + S(x.value)) }); w.write('a').then(function () { L.push('w') }); w.close().then(function () { L.push('c') }); r.read().then(function (x) { L.push('r2 ' + x.done) }); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b'); c.close() } }); var ws = new WritableStream({ write: function (c) { L.push('write ' + c) }, close: function () { L.push('close') } }); rs.pipeTo(ws).then(function () { L.push('piped') }); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }).then(function () { L.push('p5') }).then(function () { L.push('p6') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { L.push('start'); c.enqueue(1) }, pull: function () { L.push('pull') } }); L.push('ctor'); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.enqueue(2); c.enqueue(3) } }); var r = rs.getReader(); var a = r.read(), b = r.read(), c = r.read(); a.then(function () { L.push('a') }); b.then(function () { L.push('b') }); c.then(function () { L.push('c') }); queueMicrotask(function () { L.push('qm') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ cancel: function () { L.push('cancel') } }); var r = rs.getReader(); r.read().then(function (x) { L.push('read done=' + x.done) }); r.cancel().then(function () { L.push('cancelled') }); r.closed.then(function () { L.push('closed') }); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }); var o = []; (async function () { for await (var v of rs) L.push('v' + v); L.push('fim-for') })(); Promise.resolve().then(function () { L.push('p1') }).then(function () { L.push('p2') }).then(function () { L.push('p3') }).then(function () { L.push('p4') }).then(function () { L.push('p5') }); await new Promise(function (res) { setTimeout(res, 0) }); return 'fim'`);

// Fatia 3: length de respond/respondWithNewView, inspect.custom de cada classe, chamada sem `new` dos controllers.
expr(`[ReadableStreamBYOBRequest.prototype.respond.length, ReadableStreamBYOBRequest.prototype.respondWithNewView.length]`);
for (const N of ["ReadableStream", "ReadableStreamDefaultReader", "ReadableStreamBYOBReader", "ReadableStreamDefaultController", "ReadableByteStreamController",
  "ReadableStreamBYOBRequest", "WritableStream", "WritableStreamDefaultWriter", "WritableStreamDefaultController", "TransformStream", "TransformStreamDefaultController"]) {
  expr(`DESC(${N}.prototype, Symbol.for('nodejs.util.inspect.custom'))`);
}
for (const N of ["ReadableStreamDefaultController", "ReadableByteStreamController", "WritableStreamDefaultController", "TransformStreamDefaultController", "ReadableStreamBYOBRequest"]) {
  expr(`${N}()`);
}
const INSPECT = "Symbol.for('nodejs.util.inspect.custom')";
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new ReadableStream())`);
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new ReadableStream().getReader())`);
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new WritableStream())`);
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new TransformStream())`);
expr(`(function(o){ return o[${INSPECT}](-1, {}) === o })(new ReadableStream())`);
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new ReadableStream({ start: function (c) { c.enqueue(1); c.close() } }))`);
expr(`(function(o){ return o[${INSPECT}](2, {}) })(new ReadableStream({ start: function (c) { c.error(1) } }))`);
expr(`(function(c){ return c[${INSPECT}](2, {}) })((function(){ var k; new ReadableStream({ start: function (c) { k = c } }); return k })())`);

// Reentrada no `TransformStream` por dentro do `size()` da estratégia legível e do `transform` do usuário (medido no bun
// 1.4.2): `enqueue`/`error`/`terminate`/`desiredSize` chamados de dentro do `size()` agem de verdade, na ordem do bun
// (o `enqueue` reentrante entra na fila antes do chunk que o disparou). Linhas 339 em diante do TSV.
const REENTRY_OPS = ["c.enqueue('x')", "c.error(new Error('boom'))", "c.terminate()", "c.desiredSize", "(c.enqueue('x'), c.terminate())", "(c.terminate(), c.enqueue('x'))"];
for (const op of REENTRY_OPS) {
  abody(
    `var ctl, calls = 0; var ts = new TransformStream({ start: function (c) { ctl = c }, transform: function (chunk, c) { try { c.enqueue(chunk); L.push('t ok') } catch (e) { L.push('t threw ' + E(e)) } }, flush: function () { L.push('flush') } }, ` +
      `{ highWaterMark: 1 }, { highWaterMark: 5, size: function () { calls++; if (calls === 1) { var c = ctl; try { L.push('size ' + S(${op})) } catch (e) { L.push('size threw ' + E(e)) } } return 1 } }); ` +
      `var w = ts.writable.getWriter(), r = ts.readable.getReader(); ` +
      `var ev = function (label) { return function (v) { L.push(label + ' ok ' + S(v)) } }, er = function (label) { return function (e) { L.push(label + ' rej ' + S(e && e.message)) } }; ` +
      `w.write('a').then(ev('w1'), er('w1')); await null; await null; await null; L.push('desired ' + S(ctl.desiredSize)); ` +
      `r.read().then(ev('r1'), er('r1')); await null; await null; await null; ` +
      `w.write('b').then(ev('w2'), er('w2')); await null; await null; await null; ` +
      `r.read().then(ev('r2'), er('r2')); await null; await null; await null; ` +
      `w.close().then(ev('close'), er('close')); await null; await null; await null; L.push('calls ' + calls)`,
  );
}
// `enqueue` reentrante de dentro do `transform` e `terminate`/`error` seguidos de `enqueue` no mesmo turno.
abody(
  `var ts = new TransformStream({ transform: function (chunk, c) { c.enqueue(chunk + '1'); c.terminate(); try { c.enqueue(chunk + '2') } catch (e) { L.push('threw ' + E(e)) } } }); ` +
    `var w = ts.writable.getWriter(), r = ts.readable.getReader(); w.write('a').then(function () { L.push('w ok') }, function (e) { L.push('w rej ' + S(e && e.message)) }); ` +
    `r.read().then(function (v) { L.push('r ' + S(v)) }); await null; await null; await null; await null; r.read().then(function (v) { L.push('r2 ' + S(v)) }); await null; await null; await null`,
);
abody(
  `var ts = new TransformStream({ transform: function (chunk, c) { c.error(new Error('e1')); try { c.enqueue(chunk) } catch (e) { L.push('threw ' + E(e)) } } }); ` +
    `var w = ts.writable.getWriter(), r = ts.readable.getReader(); w.write('a').then(function () { L.push('w ok') }, function (e) { L.push('w rej ' + S(e && e.message)) }); ` +
    `r.read().then(function (v) { L.push('r ' + S(v)) }, function (e) { L.push('r rej ' + S(e && e.message)) }); await null; await null; await null; await null`,
);

// Grade de pipeTo: cada combinação de preventClose/preventAbort/preventCancel contra quatro cenários (término normal,
// origem com erro, destino com erro, sinal abortado depois de uma leitura), com o log de write/close/abort/cancel e o
// estado dos travamentos ao final. Linhas novas ficam no fim do TSV (a partir da 347).
for (const opts of ["", "preventClose: true", "preventAbort: true", "preventCancel: true", "preventClose: true, preventAbort: true, preventCancel: true"]) {
  const O = `{ ${opts} }`;
  const sink = `{ write: function (c) { L.push('write ' + c) }, close: function () { L.push('close') }, abort: function (r) { L.push('abort ' + S(r)) } }`;
  abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b'); c.close() } }); var ws = new WritableStream(${sink}); await rs.pipeTo(ws, ${O}); L.push(rs.locked, ws.locked); return 'fim'`);
  abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.error('src') } }); var ws = new WritableStream(${sink}); try { await rs.pipeTo(ws, ${O}) } catch (e) { L.push('rej ' + S(e)) } L.push(rs.locked, ws.locked); return 'fim'`);
  abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.enqueue('b') }, cancel: function (r) { L.push('cancel ' + S(r)) } }); var ws = new WritableStream({ write: function (c) { if (c === 'b') throw 'dst'; L.push('write ' + c) } }); try { await rs.pipeTo(ws, ${O}) } catch (e) { L.push('rej ' + S(e)) } L.push(rs.locked, ws.locked); return 'fim'`);
  abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a') }, cancel: function (r) { L.push('cancel ' + S(r)) } }); var ws = new WritableStream(${sink}); var ac = new AbortController(); var p = rs.pipeTo(ws, Object.assign({ signal: ac.signal }, ${O})); await null; await null; ac.abort('sig'); try { await p } catch (e) { L.push('rej ' + S(e)) } L.push(rs.locked, ws.locked); return 'fim'`);
}
// Destino já fechado ou com erro antes do pipe, e origem já fechada.
abody(`var rs = new ReadableStream({ cancel: function (r) { L.push('cancel ' + E(r)) } }); var ws = new WritableStream(); await ws.close(); try { await rs.pipeTo(ws) } catch (e) { L.push('rej ' + E(e)) } return rs.locked + ' ' + ws.locked`);
abody(`var rs = new ReadableStream({ start: function (c) { c.close() } }); var ws = new WritableStream({ close: function () { L.push('close') } }); await rs.pipeTo(ws); try { ws.getWriter().write('x').catch(function (e) { L.push('w ' + E(e)) }) } catch (e) { L.push('t ' + E(e)) } await null; return 'fim'`);
abody(`var rs = new ReadableStream({ start: function (c) { c.enqueue('a'); c.close() } }); var o = rs.pipeThrough(new TransformStream({ transform: function (c, t) { t.enqueue(c + '!'); t.enqueue(c + '?') } })); var r = o.getReader(); var x = []; for (;;) { var y = await r.read(); if (y.done) break; x.push(y.value) } return x`);

// Fluxo de bytes (fatia 11): `type: 'bytes'`, ReadableByteStreamController, ReadableStreamBYOBRequest e
// ReadableStreamBYOBReader. Os casos acrescentam linhas ao fim do TSV; as anteriores não mudam.
const BYTES = "var u8 = function (a) { return new Uint8Array(a) }; var A = function (v) { return v === undefined ? 'undefined' : v === null ? 'null' : Object.prototype.toString.call(v) + ':' + v.byteLength + ':' + v.byteOffset + ':' + Array.from(new Uint8Array(v.buffer || v)).join(',') };\n";
const bbody = (body) => abody(BYTES + body);
const bexpr = (code) => programs.push(HELPER + BYTES + `try { R = S(${code}) } catch (e) { R = 'throw ' + E(e) }`);
// getReader: modos e options.
bexpr(`(function () { var r = new ReadableStream({ type: 'bytes' }).getReader({ mode: 'byob' }); return [Object.prototype.toString.call(r), r.constructor.name, r instanceof ReadableStreamBYOBReader] })()`);
bexpr(`(function () { var r = new ReadableStream({ type: 'bytes' }).getReader({ mode: undefined }); return r.constructor.name })()`);
bexpr(`new ReadableStream({ type: 'bytes' }).getReader({ mode: 'x' })`);
bexpr(`new ReadableStream({ type: 'bytes' }).getReader({ mode: 1 })`);
bexpr(`new ReadableStream({ type: 'bytes' }).getReader(1)`);
bexpr(`new ReadableStream({ type: 'bytes' }).getReader(null).constructor.name`);
bexpr(`new ReadableStream({}).getReader({ mode: 'byob' })`);
bexpr(`(function () { var rs = new ReadableStream({ type: 'bytes' }); rs.getReader({ mode: 'byob' }); return rs.locked })()`);
bexpr(`(function () { var rs = new ReadableStream({ type: 'bytes' }); rs.getReader({ mode: 'byob' }); return rs.getReader() })()`);
bexpr(`new ReadableStreamBYOBReader(new ReadableStream({ type: 'bytes' })).constructor.name`);
bexpr(`new ReadableStreamBYOBReader(new ReadableStream({}))`);
bexpr(`new ReadableStreamBYOBReader({})`);
bexpr(`new ReadableStreamBYOBReader()`);
bexpr(`ReadableStreamBYOBReader(new ReadableStream({ type: 'bytes' }))`);
// Construtor e controlador.
bexpr(`new ReadableStream({ type: 'bytes' }, { size: function () { return 1 } })`);
bexpr(`new ReadableStream({ type: 'bytes' }, { highWaterMark: -1 })`);
bexpr(`new ReadableStream({ type: 'bytes' }, { highWaterMark: NaN })`);
bexpr(`new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 0 })`);
bexpr(`new ReadableStream({ type: 'bytes', autoAllocateChunkSize: -1 })`);
bexpr(`new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 'x' })`);
bexpr(`new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 1.5 }).locked`);
bexpr(`new ReadableByteStreamController()`);
bexpr(`new ReadableStreamBYOBRequest()`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); return [Object.prototype.toString.call(c), c.constructor.name, c.desiredSize, c.byobRequest]`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }, { highWaterMark: 5 }); return c.desiredSize`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.enqueue(u8([1, 2, 3])) } }, { highWaterMark: 5 }); return c.desiredSize`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.close() } }); return c.desiredSize`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.error('e') } }); return String(c.desiredSize)`);
// enqueue.
for (const arg of ["1", "'a'", "null", "undefined", "{}", "[1]", "new ArrayBuffer(4)", "new Uint8Array(0)", "new DataView(new ArrayBuffer(2))", "new Uint16Array(2)"]) {
  bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); try { c.enqueue(${arg}) } catch (e) { return E(e) } return 'ok'`);
}
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); try { c.enqueue() } catch (e) { return E(e) }`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.close() } }); try { c.enqueue(u8([1])) } catch (e) { return E(e) }`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.error('e') } }); try { c.enqueue(u8([1])) } catch (e) { return E(e) }`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var b = u8([1, 2]); c.enqueue(b); return [b.byteLength, b.buffer.byteLength, b.byteOffset]`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var b = u8([1, 2]); c.enqueue(b); try { c.enqueue(b) } catch (e) { return E(e) }`);
// close.
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); c.close(); try { c.close() } catch (e) { return E(e) }`);
bbody(`var c; new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.error('e') } }); try { c.close() } catch (e) { return E(e) }`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); c.enqueue(u8([1])); c.close(); var r = rs.getReader(); var a = await r.read(); var b = await r.read(); return [A(a.value), a.done, b.value, b.done]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); c.enqueue(u8([9, 8])); c.close(); var a = await p; var b = await r.read(u8(4)); return [A(a.value), a.done, A(b.value), b.done]`);
// error.
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); c.error('boom'); c.error('again'); try { await rs.getReader().read() } catch (e) { return e } return 'x'`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); c.close(); c.error('x'); return (await rs.getReader().read()).done`);
// Leitor padrão sobre fluxo de bytes: autoAllocateChunkSize e fila.
bbody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 4, pull: function (c) { L.push('pull ' + (c.byobRequest ? c.byobRequest.view.byteLength : 'sem')) ; if (c.byobRequest) { c.byobRequest.view[0] = 7; c.byobRequest.respond(1) } } }); var r = rs.getReader(); var a = await r.read(); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { L.push('pull ' + String(c.byobRequest)); c.enqueue(u8([5, 6])) } }); var r = rs.getReader(); var a = await r.read(); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { L.push('start'); }, pull: function (c) { L.push('pull') } }, { highWaterMark: 0 }); await null; await null; return 'x'`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { L.push('start') }, pull: function (c) { L.push('pull') } }, { highWaterMark: 2 }); await null; await null; return 'x'`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { L.push('pull'); c.enqueue(u8([L.length])) } }, { highWaterMark: 3 }); await null; await null; await null; await null; return 'x'`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader(); var p = r.read(); c.enqueue(u8([1, 2, 3])); var a = await p; return [A(a.value), a.done, c.desiredSize]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); c.enqueue(u8([1, 2, 3])); c.enqueue(u8([4])); var r = rs.getReader(); var a = await r.read(); var b = await r.read(); return [A(a.value), A(b.value)]`);
// BYOB: read com view.
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read() } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(1) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read({}) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(new ArrayBuffer(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(0)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(new DataView(new ArrayBuffer(0))) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var b = u8(4); r.read(b); return [b.byteLength, b.buffer.byteLength]`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var b = u8(4); b.buffer.transfer ? 0 : 0; var a = b.buffer; r.read(b); return [a.byteLength, a.detached]`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var ab = new ArrayBuffer(4); ab.transfer(); try { await r.read(new Uint8Array(ab)) } catch (e) { return E(e) }`);
// min.
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4), {}) } catch (e) { return E(e) } return 'ok'`);
for (const min of ["0", "-1", "5", "2", "1.5", "'2'", "NaN", "Infinity", "undefined"]) {
  bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); try { var p = r.read(u8(4), { min: ${min} }); c.enqueue(u8([1, 2])); c.enqueue(u8([3, 4])); var a = await p; return [A(a.value), a.done] } catch (e) { return E(e) }`);
}
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4), 1) } catch (e) { return E(e) } return 'ok'`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(new Uint16Array(2), { min: 3 }) } catch (e) { return E(e) } return 'ok'`);
// Entrega de dados enfileirados ao BYOB.
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.enqueue(u8([1, 2, 3])) } }); var r = rs.getReader({ mode: 'byob' }); var b = u8(2); var a = await r.read(b); return [A(a.value), a.done, b.byteLength, A(a.value) === A(b)]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.enqueue(u8([1, 2, 3])) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(8)); var b = await r.read(u8(8), { min: 1 }); return [A(a.value)]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.enqueue(u8([1, 2, 3])) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(new Uint16Array(4)); return [A(a.value), a.value.constructor.name]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x; c.enqueue(u8([1, 2, 3])) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(new DataView(new ArrayBuffer(8), 2)); return [A(a.value), a.value.constructor.name]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(u8([1, 2, 3])); c.close() } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(8)); var b = await r.read(u8(8)); return [A(a.value), a.done, A(b.value), b.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.close() } }); var r = rs.getReader({ mode: 'byob' }); var b = u8(8); var a = await r.read(b); return [A(a.value), a.done, b.byteLength]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.error('boom') } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(8)) } catch (e) { return e }`);
// byobRequest e respond.
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; L.push([Object.prototype.toString.call(q), q.constructor.name, q.view.byteLength, q === c.byobRequest, q.view === q.view]); q.respond(2) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var v = c.byobRequest.view; v[0] = 10; v[1] = 20; c.byobRequest.respond(2) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; var v = q.view; q.respond(2); L.push([v.byteLength, v.buffer.byteLength, String(c.byobRequest)]) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value)]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(0) } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(5) } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(-1) } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond('x') } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(NaN) } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond() } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(1); try { q.respond(1) } catch (e) { L.push(E(e)) } try { q.respondWithNewView(u8(1)) } catch (e) { L.push(E(e)) } L.push(String(q.view)) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return A(a.value)`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respondWithNewView(u8([4, 5, 6])) } }); var r = rs.getReader({ mode: 'byob' }); try { var a = await r.read(u8(4)); return [A(a.value), a.done] } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; var v = q.view; q.respondWithNewView(new Uint8Array(v.buffer, v.byteOffset, 2)) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
for (const arg of ["1", "{}", "new ArrayBuffer(4)", "undefined", "u8(5)", "new Uint8Array(new ArrayBuffer(8), 1, 2)"]) {
  bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { c.byobRequest.respondWithNewView(${arg}) } }); var r = rs.getReader({ mode: 'byob' }); try { var a = await r.read(u8(4)); return [A(a.value), a.done] } catch (e) { return E(e) }`);
}
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; q.respond(1) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(new Uint16Array(2)); return 'x'`);
// respond no estado fechado.
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', pull: function (x) { c = x; var q = x.byobRequest; x.close(); q.respond(0); L.push('fechou'); } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; try { c.close() } catch (e) { L.push(E(e)) } q.respond(1) } }); var r = rs.getReader({ mode: 'byob' }); try { var a = await r.read(u8(4)); return [A(a.value), a.done] } catch (e) { return E(e) }`);
// enqueue com byobRequest pendente.
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { var q = c.byobRequest; c.enqueue(u8([7, 7])); L.push(String(c.byobRequest)) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
// byobRequest fora de pull.
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); r.read(u8(4)); await null; return [c.byobRequest && c.byobRequest.view.byteLength, c.byobRequest && c.byobRequest.view.constructor.name]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var b = new Uint16Array(4); r.read(b); await null; var q = c.byobRequest; return [q.view.constructor.name, q.view.byteLength, q.view.length, q.view.buffer === b.buffer]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); c.byobRequest.view[0] = 3; c.byobRequest.respond(1); var a = await p; return [A(a.value), a.done, String(c.byobRequest)]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); var q = c.byobRequest; c.error('e'); try { q.respond(1) } catch (e) { return E(e) }`);
// Leitor padrão com pull sem enqueue; autoAllocate com byobRequest.
bbody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 3, pull: function (c) { var q = c.byobRequest; L.push([q.view.constructor.name, q.view.byteLength, q.view.byteOffset]); q.respond(3) } }); var r = rs.getReader(); var a = await r.read(); var b = await r.read(); return [A(a.value), A(b.value)]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 3, pull: function (c) { c.byobRequest.respond(2) } }); var r = rs.getReader(); var a = await r.read(); return [A(a.value), a.value.constructor.name]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 3, pull: function (c) { c.enqueue(u8([1, 2, 3, 4, 5])) } }); var r = rs.getReader(); var a = await r.read(); return [A(a.value)]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', autoAllocateChunkSize: 3, pull: function (c) { c.byobRequest.respond(3); c.close() } }); var r = rs.getReader(); var a = await r.read(); var b = await r.read(); return [A(a.value), b.done]`);
// releaseLock, cancel e closed.
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); r.releaseLock(); return [rs.locked, r.releaseLock()]`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); r.releaseLock(); try { await p } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); r.releaseLock(); try { await r.read(u8(4)) } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); r.releaseLock(); try { await r.cancel() } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); r.releaseLock(); try { await r.closed } catch (e) { return E(e) }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', cancel: function (x) { L.push('cancel ' + S(x)) } }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); var q = r.cancel('why'); var a = await p; return [A(a.value), a.done, await q]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(u8([1])) }, cancel: function (x) { L.push('cancel ' + S(x)) } }); var r = rs.getReader({ mode: 'byob' }); await r.cancel('r'); var a = await r.read(u8(4)); return [A(a.value), a.done]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.close() } }); var r = rs.getReader({ mode: 'byob' }); await r.closed; return 'fechado'`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { c.error('e') } }); var r = rs.getReader({ mode: 'byob' }); try { await r.closed } catch (e) { return e }`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var p = r.closed; return [p instanceof Promise, p === r.closed]`);
bbody(`var rs = new ReadableStream({ type: 'bytes' }); var r = rs.getReader({ mode: 'byob' }); var p = r.read(u8(4)); r.releaseLock(); try { await p } catch (e) { L.push(E(e)) } var r2 = rs.getReader({ mode: 'byob' }); return rs.locked`);
// Ordem de pull e microtasks.
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function (c) { L.push('start') }, pull: function (c) { L.push('pull ' + c.byobRequest.view.byteLength); c.byobRequest.respond(1) } }); var r = rs.getReader({ mode: 'byob' }); L.push('antes'); var p = r.read(u8(4)); L.push('depois'); var a = await p; L.push('lido'); return A(a.value)`);
bbody(`var n = 0; var rs = new ReadableStream({ type: 'bytes', pull: function (c) { n++; L.push('pull ' + n); if (n < 3) c.enqueue(u8([n])); else c.close() } }); var r = rs.getReader(); var o = []; for (;;) { var a = await r.read(); if (a.done) break; o.push(A(a.value)) } return o`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { L.push('pull'); return new Promise(function (res) { setTimeout(function () { c.byobRequest.respond(1); res() }, 1) }) } }); var r = rs.getReader({ mode: 'byob' }); var a = await r.read(u8(4)); var b = await r.read(u8(4)); return [A(a.value), A(b.value)]`);
bbody(`var rs = new ReadableStream({ type: 'bytes', pull: function (c) { throw 'pullboom' } }); var r = rs.getReader({ mode: 'byob' }); try { await r.read(u8(4)) } catch (e) { return e }`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function () { throw 'startboom' } }); return 'x'`);
bbody(`var rs = new ReadableStream({ type: 'bytes', start: function () { return Promise.reject('rej') } }); var r = rs.getReader(); try { await r.read() } catch (e) { return e }`);
// Duas leituras BYOB pendentes e um leitor padrão com fila pendente.
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p1 = r.read(u8(2)); var p2 = r.read(u8(2)); c.enqueue(u8([1, 2, 3])); var a = await p1; var b = await p2; return [A(a.value), A(b.value)]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p1 = r.read(u8(4)); c.enqueue(u8([1, 2])); c.close(); var a = await p1; var b = await r.read(u8(4)); return [A(a.value), a.done, A(b.value), b.done]`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p1 = r.read(u8(4), { min: 3 }); c.enqueue(u8([1, 2])); try { c.close() } catch (e) { L.push(E(e)) } try { await p1 } catch (e) { return E(e) }`);
bbody(`var c; var rs = new ReadableStream({ type: 'bytes', start: function (x) { c = x } }); var r = rs.getReader({ mode: 'byob' }); var p1 = r.read(u8(4), { min: 3 }); c.enqueue(u8([1, 2])); c.enqueue(u8([3])); var a = await p1; return [A(a.value), a.done]`);
// inspect e tee.
bexpr(`require('util').inspect(new ReadableStream({ type: 'bytes' }))`);
bexpr(`(function () { var rs = new ReadableStream({ type: 'bytes' }); return [typeof rs.getReader({ mode: 'byob' }).read, ReadableStreamBYOBReader.prototype.read.length, ReadableStreamBYOBRequest.prototype.respond.length, ReadableStreamBYOBRequest.prototype.respondWithNewView.length] })()`);
bexpr(`(function () { var f = Object.getOwnPropertyDescriptor(ReadableStreamBYOBRequest.prototype, 'view').get; return f.call({}) })()`);
bexpr(`(function () { var f = Object.getOwnPropertyDescriptor(ReadableByteStreamController.prototype, 'byobRequest').get; return f.call({}) })()`);
bexpr(`ReadableByteStreamController.prototype.close.call({})`);
bexpr(`ReadableStreamBYOBRequest.prototype.respond.call({}, 1)`);

// Corpo de Blob, Response e Request como ReadableStream (fatia final: não desloca os índices anteriores). O stream do
// bun não é de bytes (BYOB rejeita), entrega Uint8Array e parte em 16384 + resto.
abody(`var s = new Blob(['hello']).stream(); L.push(Object.prototype.toString.call(s), s.locked, s.type); try { s.getReader({ mode: 'byob' }) } catch (e) { L.push(E(e)) } var r = s.getReader(); var c = await r.read(); L.push(Object.prototype.toString.call(c.value), c.value.length, c.done, JSON.stringify(await r.read())); return s.locked`);
abody(`var r = new Blob([new Uint8Array(70000)]).stream().getReader(); var n = []; for (;;) { var q = await r.read(); if (q.done) break; n.push(q.value.length) } return n`);
abody(`var r = new Blob(['']).stream().getReader(); return JSON.stringify(await r.read())`);
abody(`var b = new Blob(['x']); return [b.stream() === b.stream(), b.stream() instanceof ReadableStream]`);
abody(`var res = new Response('abc'); L.push(res.body === res.body, res.bodyUsed, res.body.locked); var rr = res.body.getReader(); L.push(res.bodyUsed, res.body.locked); var x = await rr.read(); L.push(res.bodyUsed, x.value.length); try { await res.text() } catch (e) { L.push(E(e)) } return res.bodyUsed`);
abody(`var res = new Response('xyz'); res.body.getReader(); try { await res.text() } catch (e) { return E(e) }`);
abody(`return [new Response('').body instanceof ReadableStream, new Response().body, new Response(null).body, new Request('http://a/').body, new Request('http://a/', { method: 'POST', body: 'x' }).body instanceof ReadableStream]`);
abody(`var res = new Response('q'); await res.text(); return [res.body instanceof ReadableStream, res.bodyUsed]`);
abody(`var rd = new Response(new Blob(['ab'])).body.getReader(); return JSON.stringify([...(await rd.read()).value])`);
abody(`var rq = new Request('http://a/', { method: 'POST', body: 'req' }); var rd = rq.body.getReader(); var x = await rd.read(); L.push(rq.bodyUsed, new TextDecoder().decode(x.value)); try { await rq.text() } catch (e) { L.push(E(e)) } return rq.body === rq.body`);
abody(`var res = new Response('c'); res.body.getReader(); try { res.clone() } catch (e) { return E(e) }`);

// Corpo que é um ReadableStream do usuário (new Response(stream), new Request(url, {body: stream})).
const sbs = `function rs() { var a = arguments; return new ReadableStream({ start: function (k) { for (var i = 0; i < a.length; i++) k.enqueue(a[i]); k.close() } }) } function u(){ return new Uint8Array([].slice.call(arguments)) } `;
abody(sbs + `var s = rs(u(1)); var r = new Response(s); var q = new Request('http://a/', { method: 'POST', body: s }); return [r.body === s, r.bodyUsed, s.locked, q.body === s, r.headers.get('content-type')]`);
abody(sbs + `var r = new Response(rs(u(104, 105), u(33))); return [await r.text(), r.bodyUsed]`);
abody(sbs + `var r = new Request('http://a/', { method: 'POST', body: rs(u(104, 105)) }); return await r.text()`);
abody(sbs + `var r = new Response(rs(u(1, 2), u(3))); return [...new Uint8Array(await r.arrayBuffer())]`);
abody(sbs + `var r = new Response(rs(u(1, 2), u(3))); return [...await r.bytes()]`);
abody(sbs + `var r = new Response(rs(u(123, 34, 97, 34, 58), u(49, 125))); return await r.json()`);
abody(sbs + `var r = new Response(rs(u(123))); try { await r.json() } catch (e) { return E(e) }`);
abody(sbs + `var r = new Response(rs(u(1, 2)), { headers: { 'content-type': 'a/b' } }); var b = await r.blob(); return [b.size, b.type]`);
abody(sbs + `var r = new Response(rs(u(97, 61, 49)), { headers: { 'content-type': 'application/x-www-form-urlencoded' } }); return [...(await r.formData())]`);
abody(sbs + `var r = new Response(rs(u(97, 61, 49))); try { await r.formData() } catch (e) { return E(e) }`);
for (const chunk of ["'abc'", 'new ArrayBuffer(2)', 'new Uint16Array([65])', 'new DataView(new ArrayBuffer(2))', '5', '{}', 'undefined']) {
  abody(sbs + `var r = new Response(rs(${chunk})); try { return JSON.stringify(await r.text()) } catch (e) { return [E(e), r.bodyUsed] }`);
}
abody(sbs + `var r = new Response(new ReadableStream({ start: function (c) { c.error(new Error('boom')) } })); try { await r.text() } catch (e) { return E(e) }`);
abody(sbs + `var r = new Response(new ReadableStream({ start: function (c) { c.error('boom') } })); try { await r.text() } catch (e) { return [typeof e, e] }`);
abody(sbs + `var n = 0; var r = new Response(new ReadableStream({ pull: function (c) { if (n++ == 0) c.enqueue(u(65)); else c.error(new Error('late')) } })); try { await r.text() } catch (e) { return E(e) }`);
abody(sbs + `var s = rs(u(1)); s.getReader(); try { new Response(s) } catch (e) { return [E(e), Object.keys(e)] }`);
abody(sbs + `var s = rs(u(1)); s.getReader(); try { new Request('http://a/', { method: 'POST', body: s }) } catch (e) { return E(e) }`);
abody(sbs + `var s = rs(u(1)); await s.getReader().read(); try { new Response(s) } catch (e) { return E(e) }`);
abody(sbs + `var r = new Response(rs(u(65))); var p = r.text(); var d = [r.bodyUsed, r.body.locked]; await p; return d.concat([r.bodyUsed, r.body.locked])`);
abody(sbs + `var r = new Response(rs(u(65))); await r.text(); try { await r.text() } catch (e) { return E(e) }`);
abody(sbs + `var r = new Response(rs(u(65))); var p = r.text(); try { await r.text() } catch (e) { return E(e) }`);
abody(sbs + `var c; var r = new Response(new ReadableStream({ start: function (k) { c = k } })); var p = r.text(); c.enqueue(u(66)); c.close(); return await p`);
abody(sbs + `return [await new Response(rs()).text(), (await new Response(rs(u(0xef, 0xbb, 0xbf, 65))).text()).length, await new Response(rs(u(0xe2, 0x82), u(0xac))).text()]`);

// textStream de Response e Request.
const tsd = `async function drain(s) { var rd = s.getReader(); var o = []; for (;;) { var x = await rd.read(); if (x.done) { o.push('DONE'); break } o.push(x.value.length + ':' + typeof x.value + ':' + x.value) } return o } `;
abody(tsd + `return [await drain(new Response('').textStream()), await drain(new Response('h\\u00e9llo').textStream())]`);
abody(tsd + `var r = new Response(); var t = r.textStream(); return [Object.prototype.toString.call(t), r.bodyUsed, await drain(t)]`);
abody(tsd + `var r = new Response('a'); r.textStream(); try { r.textStream() } catch (e) { return [E(e), e.code] }`);
abody(tsd + `var r = new Response('a'); await r.text(); try { r.textStream() } catch (e) { return [E(e), e.code] }`);
abody(tsd + `var r = new Response('a'); r.body.getReader(); try { r.textStream() } catch (e) { return [E(e), e.code] }`);
abody(tsd + `return await drain(new Response(new Uint8Array(40000).fill(65)).textStream()).then(function (o) { return [o.length, o[0].slice(0, 12)] })`);
abody(tsd + `return await drain(new Response(new Uint8Array([0xef, 0xbb, 0xbf, 65])).textStream())`);
abody(tsd + `return await drain(new Response(new Uint8Array([0xff, 65])).textStream())`);
abody(tsd + `var q = new Request('http://a/', { method: 'POST', body: '\\u00e9' }); return [await drain(q.textStream()), q.bodyUsed]`);
abody(sbs + tsd + `var r = new Response(rs(u(0xe2, 0x82), u(0xac, 65))); var t = r.textStream(); return [r.bodyUsed, t.locked, await drain(t)]`);
abody(sbs + tsd + `var r = new Response(rs(u(65), 'x')); var rd = r.textStream().getReader(); var a = await rd.read(); try { await rd.read() } catch (e) { return [JSON.stringify(a), E(e)] }`);
abody(sbs + tsd + `try { await drain(new Response(rs(5)).textStream()) } catch (e) { return E(e) }`);
abody(sbs + tsd + `try { await drain(new Response(new ReadableStream({ start: function (c) { c.error('bad') } })).textStream()) } catch (e) { return e }`);
abody(`try { return Response.prototype.textStream.call({}) } catch (e) { return [E(e), e.code] }`);
abody(sbs + `var r = new Response('abc'); var t = r.textStream(); return [r.body.locked, await r.body.getReader().read().then(function () { return 'leu' }, function (e) { return E(e) })]`);
abody(sbs + `var r = new Response(rs(u(65))); r.textStream(); return r.body.locked`);

// tee: ordem de leituras, cancel de um ramo, dos dois (razões combinadas em array), erro propagado, corpo clonado.
const teeSrc = (extra) => `var L = []; var src = new ReadableStream({ pull: function (c) { L.push('pull'); c.enqueue(L.length) }, cancel: function (r) { L.push('cancel ' + S(r)); return 7 } }${extra || ''}); `;
abody(teeSrc() + `var t = src.tee(); var a = t[0].getReader(); var b = t[1].getReader(); var x = await a.read(); var y = await b.read(); var z = await a.read(); var w = await b.read(); return [x.value, y.value, z.value, w.value, L]`);
abody(teeSrc() + `var t = src.tee(); await t[0].cancel('A'); var r = t[1].getReader(); var x = await r.read(); return [x.value, L]`);
abody(teeSrc() + `var t = src.tee(); var p = t[0].cancel('A'); var q = t[1].cancel('B'); return [await p, await q, L]`);
abody(teeSrc() + `var t = src.tee(); var p = t[1].cancel('B'); var q = t[0].cancel('A'); await p; return [await q, L]`);
abody(`var src = new ReadableStream({ pull: function (c) { c.error(new TypeError('boom')) } }); var t = src.tee(); var o = []; for (var i = 0; i < 2; i++) o.push(await t[i].getReader().read().catch(function (e) { return 'rej ' + E(e) })); return o`);
abody(`var src = new ReadableStream({ start: function (c) { c.enqueue('a'); c.close() } }); var t = src.tee(); var d = await t[0].getReader().closed.then(function () { return 'closed0' }, E); var r = t[1].getReader(); return [await r.read().then(function (x) { return x.value }), await r.read().then(function (x) { return x.done }), d]`);
abody(`var src = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(new Uint8Array([1, 2])) } }); var t = src.tee(); var r = []; for (var i = 0; i < 2; i++) { try { t[i].getReader({ mode: 'byob' }); r.push('byob') } catch (e) { r.push(E(e)) } } return r`);
abody(sbs + `var r = new Response(rs(u(104), u(105))); var c = r.clone(); return [await r.text(), await c.text()]`);
abody(sbs + `var r = new Response(rs(u(104), u(105))); var c = r.clone(); return [r.body.locked, c.body.locked, r.bodyUsed, c.bodyUsed]`);
abody(sbs + `var r = new Request('http://a/', { method: 'POST', body: rs(u(104), u(105)) }); var c = r.clone(); return [await c.text(), await r.text()]`);
abody(sbs + `var r = new Response(rs(u(104))); r.body.getReader(); try { r.clone() } catch (e) { return [E(e), e.code] }`);

// tee de stream de bytes (ReadableByteStreamTee): ramos de bytes com leitor BYOB, cópia do pedaço, cancel e erro.
const bt = `var u = function () { return new Uint8Array(Array.prototype.slice.call(arguments)) }; var A = function (v) { return v instanceof Uint8Array ? Array.from(v) : v }; `;
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(u(1, 2, 3)); c.close() } }); var t = s.tee(); var ra = t[0].getReader({ mode: 'byob' }); var x = await ra.read(new Uint8Array(8)); var y = await t[1].getReader().read(); return [A(x.value), x.done, A(y.value), x.value.buffer === y.value.buffer, x.value.byteLength, t[0] instanceof ReadableStream, s.locked, t[0].locked]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(u(1, 2, 3)); c.close() } }); var t = s.tee(); var ra = t[0].getReader({ mode: 'byob' }); var rb = t[1].getReader({ mode: 'byob' }); var x = await ra.read(new Uint8Array(8)); var y = await rb.read(new Uint8Array(2)); var z = await rb.read(new Uint8Array(8)); var w = await ra.read(new Uint8Array(8)); return [A(x.value), A(y.value), A(z.value), z.done, A(w.value), w.done]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { var r = c.byobRequest; L.push(r ? 'byob' : 'none'); if (r) { r.view[0] = 9; r.respond(1) } else c.enqueue(u(5)) } }); var t = s.tee(); var x = await t[0].getReader({ mode: 'byob' }).read(new Uint8Array(4)); var y = await t[1].getReader().read(); return [A(x.value), A(y.value), x.value.buffer === y.value.buffer]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { L.push(c.byobRequest ? 'byob' : 'none'); c.enqueue(u(5, 6)) } }); var t = s.tee(); var ra = t[0].getReader(); var rb = t[1].getReader(); var x = await ra.read(); var y = await rb.read(); return [A(x.value), A(y.value), x.value.buffer === y.value.buffer]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { L.push('pull'); c.enqueue(u(L.length)) } }); var t = s.tee(); var rb = t[1].getReader({ mode: 'byob' }); var x = await rb.read(new Uint8Array(1)); var y = await t[0].getReader().read(); var z = await rb.read(new Uint8Array(1)); return [A(x.value), A(y.value), A(z.value)]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { c.enqueue(u(5)) }, cancel: function (r) { L.push('cancel ' + S(r)) } }); var t = s.tee(); var p = t[0].cancel('A'); var y = await t[1].getReader().read(); return [A(y.value), L, t[0].locked]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { c.enqueue(u(5)) }, cancel: function (r) { L.push('cancel ' + S(r)); return 7 } }); var t = s.tee(); var p = t[0].cancel('A'); var q = t[1].cancel('B'); return [await p, await q, L]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { c.enqueue(u(5)) }, cancel: function (r) { L.push('cancel ' + S(r)) } }); var t = s.tee(); var p = t[1].cancel('B'); var q = t[0].cancel('A'); return [await p, await q, L]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', cancel: function (r) { L.push('cancel ' + S(r)) } }); var t = s.tee(); var ra = t[0].getReader({ mode: 'byob' }); var pend = ra.read(new Uint8Array(4)); var p = ra.cancel('A'); var q = t[1].cancel('B'); var rd = await pend; return [await p, await q, rd.done, A(rd.value), L]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.error(new RangeError('te')) } }); var t = s.tee(); var o = []; for (var i = 0; i < 2; i++) o.push(await t[i].getReader().read().catch(function (e) { return 'rej ' + E(e) })); return o`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.error(new RangeError('te')) } }); var t = s.tee(); var o = []; for (var i = 0; i < 2; i++) o.push(await t[i].getReader({ mode: 'byob' }).read(new Uint8Array(2)).catch(function (e) { return 'rej ' + E(e) })); return o`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', pull: function (c) { c.error(new TypeError('boom')) } }); var t = s.tee(); var o = []; for (var i = 0; i < 2; i++) o.push(await t[i].getReader({ mode: 'byob' }).read(new Uint8Array(2)).catch(function (e) { return 'rej ' + E(e) })); return o`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.close() } }); var t = s.tee(); var x = await t[0].getReader({ mode: 'byob' }).read(new Uint8Array(3)); var y = await t[1].getReader().read(); return [A(x.value), x.done, x.value.byteLength, x.value.buffer.byteLength, y.done]`);
abody(bt + `var s = new ReadableStream({ type: 'bytes' }); var t = s.tee(); var r; try { s.getReader() } catch (e) { r = E(e) } return [s.locked, t[0].locked, r]`);
abody(bt + `var n = 0; var s = new ReadableStream({ type: 'bytes', pull: function (c) { L.push('pull ' + n + ' ' + !!c.byobRequest); if (n < 2) c.enqueue(u(++n, 10 + n)); else { c.close(); if (c.byobRequest) c.byobRequest.respond(0) } } }); var t = s.tee(); var ra = t[0].getReader({ mode: 'byob' }); var rb = t[1].getReader({ mode: 'byob' }); var o = []; for (var i = 0; i < 3; i++) { var x = await ra.read(new Uint8Array(4)); o.push([A(x.value), x.done]) } for (var i = 0; i < 4; i++) { var y = await rb.read(new Uint8Array(1)); o.push([A(y.value), y.done]) } return o`);
abody(bt + `var s = new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(u(1, 2)); c.enqueue(u(3)); c.close() } }); var t = s.tee(); var ra = t[0].getReader(); var rb = t[1].getReader({ mode: 'byob' }); var a1 = await ra.read(); var b1 = await rb.read(new Uint8Array(1)); var b2 = await rb.read(new Uint8Array(4)); var a2 = await ra.read(); var a3 = await ra.read(); var b3 = await rb.read(new Uint8Array(4)); return [A(a1.value), A(b1.value), A(b2.value), A(a2.value), a3.done, b3.done]`);

// brotli em streaming: em que escrita sai cada pedaço de saída (64 KiB), fluxo cortado e códigos de erro de entrada inválida.
const brz = `var cmp = async function (f, data) { var cs = new CompressionStream(f); var w = cs.writable.getWriter(); w.write(data); w.close(); var parts = []; var rd = cs.readable.getReader(); for (;;) { var x = await rd.read(); if (x.done) break; parts.push(x.value) } var n = 0; parts.forEach(function (p) { n += p.length }); var all = new Uint8Array(n); var at = 0; parts.forEach(function (p) { all.set(p, at); at += p.length }); return all }; var feed = async function (f, pieces) { var ds = new DecompressionStream(f); var w = ds.writable.getWriter(); var rd = ds.readable.getReader(); var log = []; var cur = -1; var reading = (async function () { for (;;) { try { var x = await rd.read(); if (x.done) { log.push('end'); return } log.push('w' + cur + ':' + x.value.length) } catch (e) { log.push('err ' + E(e) + ' ' + e.code); return } } })(); for (var i = 0; i < pieces.length; i++) { cur = i; try { await w.write(pieces[i]) } catch (e) { log.push('write ' + i + ' rejected') ; break } for (var k = 0; k < 20; k++) await Promise.resolve() } cur = 'close'; try { await w.close() } catch (e) { log.push('close rejected') } await reading; return log.join(' ') }; var split = function (b, n) { var o = []; var s = Math.ceil(b.length / n); for (var i = 0; i < b.length; i += s) o.push(b.subarray(i, i + s)); return o }; `;
abody(brz + `var data = new TextEncoder().encode('The quick brown fox jumps over the lazy dog. '.repeat(40000)); var c = await cmp('brotli', data); return [c.length, await feed('brotli', [c]), await feed('brotli', split(c, 10))]`);
abody(brz + `var data = new TextEncoder().encode('The quick brown fox jumps over the lazy dog. '.repeat(40000)); var c = await cmp('brotli', data); return [await feed('brotli', [c.subarray(0, 1), c.subarray(1, 2), c.subarray(2)]), await feed('brotli', [c.subarray(0, c.length >> 1)])]`);
abody(brz + `var o = []; var bad = [[0xff], [0xff, 0xff, 0xff, 0xff], [0, 0, 0, 0, 0, 0, 0, 0], [0xaa, 0xbb, 0xcc, 0xdd, 0xee], [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3], [0x1b, 0xff, 0xff], [0x00], [0x05]]; for (var i = 0; i < bad.length; i++) o.push(await feed('brotli', [new Uint8Array(bad[i])])); o.push(await feed('brotli', [new TextEncoder().encode('hello world, not brotli at all')])); o.push(await feed('brotli', [new TextEncoder().encode('garbage12345')])); return o`);
abody(brz + `var c = await cmp('brotli', new TextEncoder().encode('hello world hello world hello world')); var j = new Uint8Array(c.length + 3); j.set(c); j.set([1, 2, 3], c.length); var j2 = await feed('brotli', [c, new Uint8Array([1])]); return [await feed('brotli', [j]), j2]`);

// zstd em streaming: saída por bloco, fluxo cortado, quadros concatenados e lixo no fim (comprimido pelo próprio motor).
abody(brz + `var data = new TextEncoder().encode('The quick brown fox jumps over the lazy dog. '.repeat(40000)); var c = await cmp('zstd', data); return [await feed('zstd', split(c, 10)), await feed('zstd', [c.subarray(0, c.length >> 1)])]`);
abody(brz + `var c = await cmp('zstd', new TextEncoder().encode('The quick brown fox jumps over the lazy dog. '.repeat(40000))); var two = new Uint8Array(c.length * 2); two.set(c); two.set(c, c.length); return [await feed('zstd', [two]), await feed('zstd', [c, c])]`);
abody(brz + `var c = await cmp('zstd', new TextEncoder().encode('The quick brown fox jumps over the lazy dog. '.repeat(40000))); var j = new Uint8Array(c.length + 3); j.set(c); j.set([1, 2, 3], c.length); return [await feed('zstd', [c, new Uint8Array([1])]), await feed('zstd', [c, new Uint8Array([0x28, 0xb5])]), await feed('zstd', [new TextEncoder().encode('garbage12345')]), await feed('zstd', [new Uint8Array(0)])]`);

// brotli comprimindo: nada sai antes do close (nem com escrita de 1 MB), e no close os pedaços seguem o highWaterMark
// do segundo argumento do construtor (65536 por padrão); em que fase (escrita i ou close) sai cada pedaço.
const brc = `var comp = async function (data, step, hwm) { var cs = hwm ? new CompressionStream('brotli', { highWaterMark: hwm }) : new CompressionStream('brotli'); var rd = cs.readable.getReader(); var w = cs.writable.getWriter(); var log = []; var phase = 'w0'; var reading = (async function () { for (;;) { var x = await rd.read(); if (x.done) return; log.push(phase + ':' + x.value.length) } })(); for (var off = 0, i = 0; off < data.length; off += step, i++) { phase = 'w' + i; await w.write(data.subarray(off, off + step)); for (var k = 0; k < 20; k++) await Promise.resolve() } phase = 'close'; await w.close(); await reading; return log.join(' ') }; var bin = function (n) { var s = 3, b = new Uint8Array(n); for (var i = 0; i < n; i++) { s = (Math.imul(s, 1103515245) + 12345) >>> 0; b[i] = s >>> 24 } return b }; `;
abody(brc + `return [await comp(bin(1024), 1024), await comp(bin(65536), 65536), await comp(bin(200000), 200000), await comp(bin(300000), 100000), await comp(bin(262144), 1000)]`);
abody(brc + `return [await comp(bin(20000), 20000, 1000), await comp(bin(20000), 5000, 1000000), await comp(bin(20000), 20000, 1)]`);

// stream lido internamente (text, pipeTo, tee, for await, Response): locked e o erro de getReader/tee/pipeTo/iterador.
const slow = `function slow() { var n = 0; return new ReadableStream({ pull: function (k) { return new Promise(function (r) { setTimeout(r, 5) }).then(function () { if (n++ < 2) k.enqueue(new TextEncoder().encode('hi')); else k.close() }) } }) } function tr(f) { try { return f() } catch (e) { return [E(e), e.code] } } `;
abody(slow + `var s = slow(); var r = new Response(s); var p = r.text(); return [s.locked, r.body.locked, r.bodyUsed, tr(function () { s.getReader() }), await p, s.locked]`);
abody(slow + `var s = slow(); var r = new Response(s); var p = r.text(); var o = [tr(function () { s.tee() }), tr(function () { s[Symbol.asyncIterator]() })]; try { await s.pipeTo(new WritableStream()) } catch (e) { o.push([E(e), e.code]) } return [o, await p]`);
abody(slow + `var s = slow(); var r = new Response(s); var p = r.blob(); var o = tr(function () { new Response(s) }); return [o, s.locked, (await p).size]`);
abody(slow + `var s = slow(); var r = new Response(s); var p = r.text(); try { await r.text() } catch (e) { return [E(e), e.code, await p] }`);
abody(slow + `var s = slow(); var p = s.pipeTo(new WritableStream()); var a = s.locked; var e1 = tr(function () { s.getReader() }); await p; return [a, e1, s.locked]`);
abody(slow + `var s = slow(); var t = s.tee(); return [s.locked, t[0].locked, t[1].locked, tr(function () { s.getReader() }), tr(function () { s.tee() })]`);
abody(slow + `var s = slow(); var it = s[Symbol.asyncIterator](); var a = s.locked; var e1 = tr(function () { s.getReader() }); await it.return(); return [a, e1, s.locked]`);
abody(slow + `var s = slow(); s.getReader(); var o = [tr(function () { new Response(s) })]; try { await s.pipeTo(new WritableStream()) } catch (e) { o.push([E(e), e.code]) } o.push(tr(function () { s.tee() })); try { await new Response(s).text() } catch (e) { o.push([E(e), e.code]) } return o`);

// stream com todos os pedaços na fila e o fechamento pedido: text()/blob() esvaziam de forma síncrona, o leitor interno
// é solto e getReader/tee seguem aceitos; sem o fechamento o stream continua travado.
const full = `function full() { return new ReadableStream({ start: function (c) { c.enqueue(new TextEncoder().encode('a')); c.enqueue(new TextEncoder().encode('b')); c.close() } }) } function tr(f) { try { return f() } catch (e) { return [E(e), e.code] } } `;
abody(full + `var s = full(); var p = new Response(s).text(); var l = s.locked; var rd = tr(function () { return s.getReader() }); return [l, typeof rd, (await rd.read()).done, await p]`);
abody(full + `var s = full(); var p = new Response(s).blob(); var rd = tr(function () { return s.getReader() }); return [s.locked, await rd.read(), (await p).size]`);
abody(full + `var s = full(); var r = new Response(s); var p = r.text(); return [tr(function () { return s.tee().length }), tr(function () { new Response(s) }), r.bodyUsed, await p]`);
abody(`var s = new ReadableStream({ start: function (c) { c.enqueue(new TextEncoder().encode('a')) } }); var p = new Response(s).text(); return [s.locked, (function () { try { s.getReader() } catch (e) { return [E(e), e.code] } })()]`);

const fullbytes = `function fullbytes() { return new ReadableStream({ type: 'bytes', start: function (c) { c.enqueue(new Uint8Array([1, 2])); c.close() } }) } function tr(f) { try { return f() } catch (e) { return [E(e), e.code] } } `;
abody(fullbytes + `var s = fullbytes(); var p = new Response(s).text(); var l = s.locked; var rd = tr(function () { return s.getReader() }); return [l, typeof rd, (await rd.read()).done, await p]`);
abody(fullbytes + `var s = fullbytes(); var p = new Response(s).blob(); var rd = tr(function () { return s.getReader() }); return [s.locked, await rd.read(), (await p).size]`);
abody(fullbytes + `var s = fullbytes(); var p = new Response(s).arrayBuffer(); var l = s.locked; var rd = tr(function () { return s.getReader() }); return [l, s.locked, (await rd.read()).done, (await p).byteLength]`);
abody(fullbytes + `var s = fullbytes(); await new Response(s).text(); var rd = s.getReader(); return [s.locked, (await rd.read()).done]`);

// Segundo argumento do construtor (parseCodecHighWaterMark): padrão 64 KiB, +Infinity, truncamento, mínimo 1 e os erros de
// convertQueuingStrategyDict (medidos no bun 1.4.2), e o tamanho dos pedaços de saída de cada formato com ele.
const hw = `var mk = function (K, s) { try { new K('gzip', s); return 'ok' } catch (e) { return E(e) + ' ' + e.code } }; var bin = function (n) { var s = 3, b = new Uint8Array(n); for (var i = 0; i < n; i++) { s = (Math.imul(s, 1103515245) + 12345) >>> 0; b[i] = s >>> 24 } return b }; var cat = function (parts) { var n = 0; parts.forEach(function (p) { n += p.length }); var all = new Uint8Array(n); var at = 0; parts.forEach(function (p) { all.set(p, at); at += p.length }); return all }; var sizes = async function (K, f, data, hwm) { var cs = new K(f, hwm === undefined ? undefined : { highWaterMark: hwm }); var w = cs.writable.getWriter(); var rd = cs.readable.getReader(); var lens = []; var reading = (async function () { for (;;) { var x = await rd.read(); if (x.done) return; lens.push(x.value.length) } })(); w.write(data); w.close(); await reading; return lens.length > 12 ? lens.slice(0, 6).join(',') + '...(' + lens.length + ')' : lens.join(',') }; `;
abody(hw + `return [undefined, null, {}, { highWaterMark: undefined }, { highWaterMark: 0 }, { highWaterMark: 2.9 }, { highWaterMark: -1 }, { highWaterMark: NaN }, { highWaterMark: Infinity }, { highWaterMark: -Infinity }, { highWaterMark: '5' }, { highWaterMark: 'x' }, { highWaterMark: 1e30 }, { highWaterMark: null }, { highWaterMark: true }, { highWaterMark: {} }, { highWaterMark: { valueOf: function () { return 7 } } }, { get highWaterMark() { throw new RangeError('boom') } }, 1, 'a', true, function () {}, Symbol('s'), 10n].map(function (s) { return [mk(CompressionStream, s), mk(DecompressionStream, s)] })`);
abody(hw + `try { new CompressionStream('gzip', { highWaterMark: 10n }) } catch (e) { L.push(E(e)) } try { new DecompressionStream('gzip', { highWaterMark: Symbol() }) } catch (e) { L.push(E(e)) } try { new CompressionStream('x', { highWaterMark: -1 }) } catch (e) { L.push(E(e), e.code) } return L`);
abody(hw + `var o = []; for (var h of [1, 10, 40000, Infinity, 0.5, 2.9, undefined]) o.push(await sizes(CompressionStream, 'gzip', bin(50000), h)); for (var f of ['deflate', 'deflate-raw', 'brotli', 'zstd']) for (var h of [1000, 200000, Infinity]) o.push(await sizes(CompressionStream, f, bin(50000), h)); return o`);
abody(hw + `var o = []; for (var f of ['gzip', 'deflate', 'brotli', 'zstd']) { var z = await new Response(new Blob([bin(30000)]).stream().pipeThrough(new CompressionStream(f))).arrayBuffer(); for (var h of [1000, 7, Infinity, undefined]) o.push(await sizes(DecompressionStream, f, new Uint8Array(z), h)) } return o`);

// Erro no meio de uma escrita grande do decodificador: os pedaços dos passos que já encheram o teto saem antes do erro, o
// que o passo que falhou tinha produzido se perde (medido: gzip com 53 pedaços de 5697 antes do erro, hwm 1000).
const dec = `var bin = function (n) { var s = 3, b = new Uint8Array(n); for (var i = 0; i < n; i++) { s = (Math.imul(s, 1103515245) + 12345) >>> 0; b[i] = (i % 16 === 0) ? (s >>> 24) : 0 } return b }; var pack = async function (f, data) { return new Uint8Array(await new Response(new Blob([data]).stream().pipeThrough(new CompressionStream(f))).arrayBuffer()) }; var run = async function (f, data, hwm) { var ds = new DecompressionStream(f, hwm === undefined ? undefined : { highWaterMark: hwm }); var w = ds.writable.getWriter(); var rd = ds.readable.getReader(); var lens = []; var err = null; var reading = (async function () { for (;;) { try { var x = await rd.read(); if (x.done) return; lens.push(x.value.length) } catch (e) { err = E(e) + ' ' + e.code; return } } })(); try { await w.write(data) } catch (e) { err = (err || '') + ' | write ' + E(e) } if (!err) { try { await w.close() } catch (e) { err = 'close ' + E(e) } } await reading; var sum = 0; lens.forEach(function (n) { sum += n }); return [lens.length, lens.slice(0, 2), lens.slice(-1), sum, data.length, err] }; `;
abody(dec + `var gz = await pack('gzip', bin(100000)); var bad = new Uint8Array(gz); bad.fill(255, bad.length - 60, bad.length - 10); var crc = new Uint8Array(gz); crc.fill(255, crc.length - 8, crc.length - 4); var early = new Uint8Array(gz); early.fill(255, 20, 60); return [await run('gzip', bad, 1000), await run('gzip', bad, 1 << 20), await run('gzip', crc, 1000), await run('gzip', crc, undefined), await run('gzip', early, 1000)]`);
abody(dec + `var o = []; for (var f of ['deflate', 'deflate-raw', 'brotli', 'zstd']) { var z = await pack(f, bin(100000)); var bad = new Uint8Array(z); bad.fill(255, bad.length - 60, bad.length - 10); o.push(await run(f, bad, 1000), await run(f, bad, undefined)) } return o`);

// zstd e brotli com um quadro de 300000 bytes (1 de cada 4 aleatório, o resto zero: vários blocos de 128 KiB) corrompido no
// MEIO, com highWaterMark 1000 e 1 MiB: a regra do piso floor(parcial / teto) com teto = max(hwm, entrada). Medido no bun 1.4.2:
// zstd comprimido tem 101646 bytes (blocos comprimidos até 44682, 88892 e o resto); corrupção em 0.4 (bloco 1): 0 pedaços;
// em 0.6 e 0.8 (bloco 2): 1 pedaço de 101646 com hwm 1000 e nenhum com 1 MiB; brotli (um meta-bloco): 0 pedaços, erro de formato.
const mid = `var bin = function (n) { var s = 3, b = new Uint8Array(n); for (var i = 0; i < n; i++) { s = (Math.imul(s, 1103515245) + 12345) >>> 0; b[i] = (i % 4 === 0) ? (s >>> 24) : 0 } return b }; var pack = async function (f, data) { return new Uint8Array(await new Response(new Blob([data]).stream().pipeThrough(new CompressionStream(f))).arrayBuffer()) }; var run = async function (f, data, hwm) { var ds = new DecompressionStream(f, { highWaterMark: hwm }); var w = ds.writable.getWriter(); var rd = ds.readable.getReader(); var lens = []; var err = null; var reading = (async function () { for (;;) { try { var x = await rd.read(); if (x.done) return; lens.push(x.value.length) } catch (e) { err = E(e) + ' ' + e.code; return } } })(); try { await w.write(data) } catch (e) { err = (err || '') + ' | write ' + E(e) } if (!err) { try { await w.close() } catch (e) { err = 'close ' + E(e) } } await reading; return [lens.length, lens.slice(0, 3), err] }; `;
abody(mid + `var o = []; for (var f of ['zstd', 'brotli']) { var z = await pack(f, bin(300000)); for (var pos of [0.4, 0.6, 0.8]) for (var h of [1000, 1 << 20]) { var bad = new Uint8Array(z); var at = Math.floor(z.length * pos); bad.fill(255, at, at + 200); o.push([f, pos, h, await run(f, bad, h)]) } } return o`);

// TextEncoderStream e TextDecoderStream em pipeThrough: sequências multibyte e substitutas partidas entre pedaços, conversão do
// pedaço no codificador, BufferSource no decodificador, estado do fluxo depois de um erro (medido no bun 1.4.2).
const u8 = (...bytes) => `new Uint8Array([${bytes.join(",")}])`;
const tsColl = `var COLL = async function (rs) { var o = []; var r = rs.getReader(); try { for (;;) { var x = await r.read(); if (x.done) { o.push('DONE'); break } o.push(x.value instanceof Uint8Array ? 'u8[' + Array.from(x.value).map(function (b) { return b.toString(16) }) + ']' : JSON.stringify(x.value)) } } catch (e) { o.push('ERR ' + E(e)) } return o.join(' ') }; var SRC = function (a) { return new ReadableStream({ start: function (c) { for (var i = 0; i < a.length; i++) c.enqueue(a[i]); c.close() } }) }; `;
abody(tsColl + `return await COLL(SRC(['\\ud83d', '\\ude00', 'a\\ud83d', '\\ude00b']).pipeThrough(new TextEncoderStream()))`);
abody(tsColl + `return await COLL(SRC(['\\ud83d', '\\ud83d']).pipeThrough(new TextEncoderStream()))`);
abody(tsColl + `return await COLL(SRC([{ toString: function () { return 'ob' } }, new String('s'), 12n]).pipeThrough(new TextEncoderStream()))`);
abody(tsColl + `return await COLL(SRC([{ toString: function () { throw new RangeError('boom') } }]).pipeThrough(new TextEncoderStream()))`);
abody(tsColl + `return await COLL(SRC(['\\u00e9\\ud83d\\ude00x']).pipeThrough(new TextEncoderStream()).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `return await COLL(SRC([new Int8Array([104, 105])]).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `return await COLL(SRC([new Uint8Array(new SharedArrayBuffer(2))]).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `var b = new Uint8Array([1, 2]); structuredClone(b.buffer, { transfer: [b.buffer] }); return await COLL(SRC([b]).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `return await COLL(SRC([${u8(0xf0)}, ${u8(0x9f, 0x98)}, ${u8(0x80, 0x41)}]).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `return await COLL(SRC([${u8(0xf0, 0x9f, 0x98)}]).pipeThrough(new TextDecoderStream()))`);
abody(tsColl + `return await COLL(SRC([${u8(0, 0x61, 0xd8)}, ${u8(0x3d, 0xde)}, ${u8(0)}]).pipeThrough(new TextDecoderStream('utf-16be')))`);
abody(tsColl + `return await COLL(SRC([${u8(0x61, 0)}, ${u8(0x62)}]).pipeThrough(new TextDecoderStream('utf-16le')))`);
abody(tsColl + `return await COLL(SRC([${u8(0xe2, 0x82)}]).pipeThrough(new TextDecoderStream('utf-8', { fatal: true })))`);
abody(tsColl + `return await COLL(SRC([${u8(0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf, 0x61)}]).pipeThrough(new TextDecoderStream()))`);
abody(`var ts = new TextDecoderStream('utf-8', { fatal: true }); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); var p = r.read().then(function (x) { L.push('r ' + JSON.stringify(x)) }, function (e) { L.push('rE ' + E(e)) }); try { await w.write(${u8(0xff)}) } catch (e) { L.push('w1 ' + E(e)) } await p; try { await w.write(${u8(0x61)}) } catch (e) { L.push('w2 ' + E(e)) } try { await w.close() } catch (e) { L.push('c ' + E(e)) } try { await r.read() } catch (e) { L.push('r2 ' + E(e)) } return w.desiredSize`);
abody(`var w = new TextDecoderStream().writable.getWriter(); try { await w.write(5) } catch (e) { L.push('w1 ' + E(e)) } try { await w.write(${u8(0x61)}) } catch (e) { L.push('w2 ' + E(e)) } return 1`);
abody(`var w = new TextEncoderStream().writable.getWriter(); try { await w.write(Symbol('x')) } catch (e) { L.push('w1 ' + E(e)) } try { await w.write('a') } catch (e) { L.push('w2 ' + E(e)) } return 1`);
abody(`var ts = new TextEncoderStream(); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write('\\ud83d'); w.write('a'); w.close(); var o = []; for (;;) { var x = await r.read(); if (x.done) break; o.push(Array.from(x.value)) } return o`);
abody(`var ts = new TextEncoderStream(); var w = ts.writable.getWriter(); var r = ts.readable.getReader(); w.write('a'); var x = await r.read(); w.write('b'); var y = await r.read(); return [x.value.constructor.name, x.value.byteOffset, x.value.buffer.byteLength, y.value.byteOffset, x.value.buffer === y.value.buffer]`);
abody(`var ts = new TextEncoderStream(); await ts.readable.getReader().cancel('why'); var w = ts.writable.getWriter(); try { await w.write('a') } catch (e) { return String(e) } return 'ok'`);
abody(`var ts = new TextDecoderStream(); await ts.writable.getWriter().abort('zz'); try { await ts.readable.getReader().read() } catch (e) { return String(e) } return 'ok'`);
abody(`return new TextEncoderStream().writable.getWriter().desiredSize`);
for (const args of ["'utf-8', { fatal: 1 }", "'utf-8', { ignoreBOM: 'x' }", "'utf-8', null", "'utf-8', []", "null", "{ toString: function () { return 'utf-8' } }", "'utf-7'", "'utf-16'", "'gbk'", "'replacement'"]) {
  abody(`try { var d = new TextDecoderStream(${args}); return [d.encoding, d.fatal, d.ignoreBOM] } catch (e) { return E(e) }`);
}

// Execução: duas rodadas idênticas em cada programa.
(async () => {
  const run = async (source) => {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 20; i++) await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 5));
    for (let i = 0; i < 20; i++) await Promise.resolve();
    // Compressão e descompressão rodam em outra thread do bun: o programa termina quando o motor termina, não em 5 ms.
    // Medir antes disso deixava `R` indefinido e o programa lento vazava para o seguinte (a escrita tardia caía em `R`
    // do próximo). O mesmo vale para os que dormem em `setTimeout` de 1 a 5 ms por escrita: três escritas passam de 5 ms.
    // Esses programas esperam o `R` próprio, com teto; os demais mantêm o turno fixo de timer.
    if (/CompressionStream|DecompressionStream|setTimeout\(/.test(source)) {
      for (let waited = 0; globalThis.R === undefined && waited < 20000; waited += 5) {
        await new Promise((resolve) => setTimeout(resolve, 5));
      }
      for (let i = 0; i < 20; i++) await Promise.resolve();
    }
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
