// Gera tests/golden/text_streams_bun.tsv: `TextEncoderStream` e `TextDecoderStream` medidos no bun 1.4.2 (forma das classes,
// getters, erros de brand check e de argumento, chunks, surrogates partidos entre pedaços no codificador, BOM e bytes
// partidos no decodificador, flush, fatal). Sem rede, sem arquivo e sem caminho da máquina. O resultado é o valor de `R`
// depois de esvaziar as microtasks e de um turno de timer; cada programa roda duas vezes e as saídas têm de ser idênticas.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-text-streams-golden.js > tests/golden/text_streams_bun.tsv
const { emitRow } = require("./golden-prelude.js");

process.on("unhandledRejection", () => {});

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'undefined') return 'undefined'; if (typeof v === 'function') return 'function'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e && e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var L = [];\n" +
  "var DESC = function (o, k) { var d = Object.getOwnPropertyDescriptor(o, k); if (!d) return 'none'; " +
  "return [typeof d.value, typeof d.get, typeof d.set, d.writable, d.enumerable, d.configurable, typeof d.value === 'function' ? d.value.length + ':' + d.value.name : (d.get ? d.get.name : '')] };\n" +
  // Escreve os pedaços, fecha, lê tudo e devolve o relato (escritas, leituras e erros).
  "var RUN = async function (ts, chunks, close) { var out = [], wr = [], w = ts.writable.getWriter(), r = ts.readable.getReader(); " +
  "var rd = (async function () { try { for (;;) { var x = await r.read(); if (x.done) { out.push('DONE'); break } " +
  "out.push(x.value instanceof Uint8Array ? 'u8[' + Array.prototype.map.call(x.value, function (b) { return b.toString(16) }) + ']' : JSON.stringify(x.value)) } } " +
  "catch (e) { out.push('ERR ' + E(e)) } })(); " +
  "for (var i = 0; i < chunks.length; i++) { try { await w.write(chunks[i]); wr.push('ok') } catch (e) { wr.push('W ' + E(e)) } } " +
  "if (close !== false) { try { await w.close(); wr.push('closed') } catch (e) { wr.push('C ' + E(e)) } } await rd; return wr.join(';') + ' => ' + out.join(' ') };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = 'throw ' + E(e) }`);
const abody = (body) =>
  programs.push(HELPER + `(async function () { ${body} })().then(function (v) { R = S(L.length ? L.concat([v]) : v) }, function (e) { R = 'rejeitou ' + E(e) + '|' + JSON.stringify(L) })`);
const run = (ctor, chunks) => abody(`return await RUN(new ${ctor}, ${chunks})`);
const u8 = (...bytes) => `new Uint8Array([${bytes.join(",")}])`;

// Forma das classes.
for (const N of ["TextEncoderStream", "TextDecoderStream"]) {
  expr(`DESC(globalThis, '${N}')`);
  expr(`[${N}.length, ${N}.name, Object.getOwnPropertyNames(${N}), Reflect.ownKeys(${N}.prototype).map(String)]`);
  expr(`[Object.getPrototypeOf(${N}.prototype) === Object.prototype, DESC(${N}.prototype, Symbol.toStringTag), DESC(${N}.prototype, 'constructor')]`);
  expr(`Object.getOwnPropertyNames(${N}.prototype).map(function (k) { return [k, DESC(${N}.prototype, k)] })`);
  expr(`DESC(${N}.prototype, Symbol.for('nodejs.util.inspect.custom'))`);
  expr(`${N}()`);
  expr(`Object.prototype.toString.call(new ${N}())`);
  expr(`(function () { var x = new ${N}(); return [x instanceof TransformStream, Object.keys(x), x.readable === x.readable, x.writable === x.writable, x.readable.constructor.name, x.writable.constructor.name] })()`);
  expr(`(function () { class X extends ${N} {} var x = new X(); return [x instanceof X, Object.getPrototypeOf(x) === X.prototype, x.encoding] })()`);
  for (const M of ["encoding", "readable", "writable", "fatal", "ignoreBOM"]) {
    expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${M}') ? Object.getOwnPropertyDescriptor(${N}.prototype, '${M}').get.call({}) : 'none'`);
    expr(`Object.getOwnPropertyDescriptor(${N}.prototype, '${M}') ? Object.getOwnPropertyDescriptor(${N}.prototype, '${M}').get.call(new ${N === "TextEncoderStream" ? "TextDecoderStream" : "TextEncoderStream"}()) : 'none'`);
  }
}
expr(`new TextEncoderStream(1, 2, 3).encoding`);
expr(`new TextEncoderStream().writable.getWriter().desiredSize`);

// Inspect custom chamado direto (o `util.inspect` monta `depth` e `options` assim): o texto de cada profundidade e largura.
const INSPECT = "[Symbol.for('nodejs.util.inspect.custom')]";
for (const ctor of ["new TextEncoderStream()", "new TextDecoderStream()", "new TextDecoderStream('utf-16le', { fatal: true, ignoreBOM: true })"]) {
  for (const depth of ["2", "1", "0", "-1", "null", "Infinity", "undefined"]) {
    const outer = depth === "2" || depth === "undefined" ? "2" : depth === "-1" ? "-1" : depth;
    expr(`(function () { var x = ${ctor}; return x${INSPECT}(${outer === "null" || outer === "Infinity" ? "Infinity" : outer}, { depth: ${depth} }) })()`);
  }
  expr(`(function () { var x = ${ctor}; return x${INSPECT}(2, { depth: 2, breakLength: Infinity }) })()`);
  expr(`(function () { var x = ${ctor}; return x${INSPECT}(2, { depth: 2, breakLength: 200 }) })()`);
  expr(`(function () { var x = ${ctor}; return x${INSPECT}(2, { depth: 2, breakLength: 20 }) })()`);
  expr(`(function () { var x = ${ctor}; return x${INSPECT}(2, {}) })()`);
  expr(`(function () { var x = ${ctor}; return x${INSPECT}(2) })()`);
  expr(`(function () { var x = ${ctor}; var w = x.writable.getWriter(); return x${INSPECT}(2, { depth: 2 }) })()`);
  expr(`(function () { var x = ${ctor}; return x${INSPECT}.call(x, 1, { depth: 2 }) === x${INSPECT}(1, { depth: 2 }) })()`);
}
expr(`(function () { class X extends TextEncoderStream {} return new X()${INSPECT}(2, { depth: 2 }) })()`);

// Construtor do decodificador.
expr(`(function () { var d = new TextDecoderStream(); return [d.encoding, d.fatal, d.ignoreBOM] })()`);
expr(`(function () { var d = new TextDecoderStream('utf-16le', { fatal: true, ignoreBOM: true }); return [d.encoding, d.fatal, d.ignoreBOM] })()`);
expr(`new TextDecoderStream(' UTF8 ').encoding`);
expr(`new TextDecoderStream(undefined, {}).fatal`);
expr(`new TextDecoderStream('nope')`);
expr(`new TextDecoderStream('utf-8', 1)`);
expr(`new TextDecoderStream('nope', 1)`);
expr(`new TextDecoderStream('latin1').encoding`);

// Codificador.
run("TextEncoderStream", "['a', 'é', '😀']");
run("TextEncoderStream", "['\\ud83d', '\\ude00']");
run("TextEncoderStream", "['a\\ud83d']");
run("TextEncoderStream", "['\\ude00', 'x']");
run("TextEncoderStream", "['\\ud83d', 'x']");
run("TextEncoderStream", "['\\ud83d', '\\ud83d', '\\ude00']");
run("TextEncoderStream", "['', 'a', '']");
run("TextEncoderStream", "[1, null, undefined, {}]");
run("TextEncoderStream", "[Symbol('x')]");
run("TextEncoderStream", "['\\ud83d']");
run("TextEncoderStream", "['\\ud83d', '']");
run("TextEncoderStream", "['\\ud83dab']");
run("TextEncoderStream", "[]");

// Decodificador.
run("TextDecoderStream", `[${u8(0x61, 0xc3)}, ${u8(0xa9)}]`);
run("TextDecoderStream", `[${u8(0xef, 0xbb)}, ${u8(0xbf, 0x61)}, ${u8(0xef, 0xbb, 0xbf)}]`);
abody(`return await RUN(new TextDecoderStream('utf-8', { ignoreBOM: true }), [${u8(0xef, 0xbb, 0xbf, 0x61)}])`);
run("TextDecoderStream", `[${u8(0x61, 0xe2, 0x82)}]`);
abody(`return await RUN(new TextDecoderStream('utf-8', { fatal: true }), [${u8(0x61, 0xe2, 0x82)}])`);
abody(`return await RUN(new TextDecoderStream('utf-8', { fatal: true }), [${u8(0x61, 0xff)}, ${u8(0x62)}])`);
abody(`return await RUN(new TextDecoderStream('utf-8', { fatal: true }), [${u8(0xe2)}, ${u8(0x82)}])`);
run("TextDecoderStream", `[${u8(0x61, 0xff, 0x62)}]`);
run("TextDecoderStream", `['abc']`);
run("TextDecoderStream", `[${u8(0x68, 0x69)}.buffer]`);
run("TextDecoderStream", `[new DataView(${u8(0x68, 0x69)}.buffer)]`);
run("TextDecoderStream", `[new Uint16Array([0x6968])]`);
for (const bad of ["1", "undefined", "null", "{}"]) run("TextDecoderStream", `[${bad}]`);
run("TextDecoderStream", `[${u8()}, ${u8(0x61)}]`);
run("TextDecoderStream", `[${u8(0xe2)}, ${u8()}]`);
run("TextDecoderStream", `[${u8(0xe2, 0x82)}, ${u8(0xac, 0x41)}]`);
abody(`return await RUN(new TextDecoderStream('utf-16le'), [${u8(0xff, 0xfe, 0x61)}, ${u8(0x00)}])`);
abody(`return await RUN(new TextDecoderStream('latin1'), [${u8(0xe9)}])`);
run("TextDecoderStream", "[]");

(async () => {
  const run1 = async (source) => {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    (0, eval)("var R");
    globalThis.R = undefined;
    (0, eval)(sourceAscii);
    for (let i = 0; i < 20; i++) await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 5));
    for (let i = 0; i < 20; i++) await Promise.resolve();
    return [sourceAscii, String(globalThis.R === undefined ? "<undefined>" : globalThis.R)];
  };
  let unstable = 0;
  for (const source of programs) {
    const [src, first] = await run1(source);
    const [, second] = await run1(source);
    if (first !== second) {
      unstable++;
      process.stderr.write(`INSTÁVEL: ${src.slice(HELPER.length, HELPER.length + 120)}\n  1: ${first}\n  2: ${second}\n`);
    }
    emitRow(JSON.stringify(src) + "\t" + JSON.stringify(first));
  }
  process.stderr.write(`${programs.length} programas, ${unstable} instáveis\n`);
  if (unstable) process.exitCode = 1;
})();
