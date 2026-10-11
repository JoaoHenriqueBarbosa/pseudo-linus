// Gera tests/golden/compression_streams_bun.tsv: `CompressionStream` e `DecompressionStream` medidos no bun 1.4.2 (forma das
// classes, formatos aceitos e recusados, bytes exatos da saída comprimida de casos pequenos, ida e volta de casos
// grandes, pedaços que não são BufferSource, dado corrompido, truncado, vazio, com sobra e com membros gzip
// concatenados, e o inspect). Sem rede, sem arquivo e sem caminho da máquina. O resultado é o valor de `R` depois de
// esvaziar as microtasks e de um turno de timer; cada programa roda duas vezes e as saídas têm de ser idênticas.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-compression-streams-golden.js > tests/golden/compression_streams_bun.tsv
const { emitRow } = require("./golden-prelude.js");

process.on("unhandledRejection", () => {});

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'undefined') return 'undefined'; if (typeof v === 'function') return 'function'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e && e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var L = [];\n" +
  "var HEX = function (b) { return Array.prototype.map.call(b, function (x) { return (x < 16 ? '0' : '') + x.toString(16) }).join('') };\n" +
  "var DESC = function (o, k) { var d = Object.getOwnPropertyDescriptor(o, k); if (!d) return 'none'; " +
  "return [typeof d.value, typeof d.get, typeof d.set, d.writable, d.enumerable, d.configurable, typeof d.value === 'function' ? d.value.length + ':' + d.value.name : (d.get ? d.get.name : '')] };\n" +
  // Escreve os pedaços, fecha, lê tudo e devolve o relato (escritas, leituras e erros); `sizes` troca os bytes por o comprimento.
  "var RUN = async function (ts, chunks, sizes) { var out = [], wr = [], w = ts.writable.getWriter(), r = ts.readable.getReader(); " +
  "var rd = (async function () { try { for (;;) { var x = await r.read(); if (x.done) { out.push('DONE'); break } " +
  "out.push(x.value instanceof Uint8Array ? (sizes ? 'u8:' + x.value.length : HEX(x.value)) : JSON.stringify(x.value)) } } " +
  "catch (e) { out.push('ERR ' + E(e)) } })(); " +
  "for (var i = 0; i < chunks.length; i++) { try { await w.write(chunks[i]); wr.push('ok') } catch (e) { wr.push('W ' + E(e)) } } " +
  "try { await w.close(); wr.push('closed') } catch (e) { wr.push('C ' + E(e)) } await rd; return wr.join(';') + ' => ' + out.join(' ') };\n" +
  // Comprime com a classe, descomprime com a outra e confere com o original.
  "var ROUND = async function (fmt, data) { var c = new CompressionStream(fmt), d = new DecompressionStream(fmt); " +
  "var packed = [], rc = c.readable.getReader(), wc = c.writable.getWriter(); " +
  "var rd = (async function () { for (;;) { var x = await rc.read(); if (x.done) break; packed.push(x.value) } })(); " +
  "await wc.write(data); await wc.close(); await rd; var n = 0; packed.forEach(function (p) { n += p.length }); " +
  "var all = new Uint8Array(n), o = 0; packed.forEach(function (p) { all.set(p, o); o += p.length }); " +
  "var back = [], rr = d.readable.getReader(), wd = d.writable.getWriter(); " +
  "var rd2 = (async function () { for (;;) { var x = await rr.read(); if (x.done) break; back.push(x.value) } })(); " +
  "await wd.write(all); await wd.close(); await rd2; var m = 0; back.forEach(function (p) { m += p.length }); " +
  "var ok = m === data.length, pos = 0; back.forEach(function (p) { for (var i = 0; i < p.length; i++, pos++) if (p[i] !== data[pos]) ok = false }); " +
  "return [fmt, data.length, ok].join(',') };\n" +
  "var ENC = new TextEncoder();\n" +
  "var CAT = function (list) { var n = 0, o = 0; list.forEach(function (p) { n += p.length }); var all = new Uint8Array(n); list.forEach(function (p) { all.set(p, o); o += p.length }); return all };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = 'throw ' + E(e) }`);
const abody = (body) =>
  programs.push(HELPER + `(async function () { ${body} })().then(function (v) { R = S(L.length ? L.concat([v]) : v) }, function (e) { R = 'rejeitou ' + E(e) + '|' + JSON.stringify(L) })`);
const run = (ctor, chunks, sizes) => abody(`return await RUN(new ${ctor}, ${chunks.replace(/Buffer\.concat/g, "CAT").replace(/Buffer\.from/g, "new Uint8Array")}${sizes ? ", true" : ""})`);
const u8 = (...bytes) => `new Uint8Array([${bytes.join(",")}])`;
const text = (s) => `ENC.encode(${JSON.stringify(s)})`;
// Os programas rodam no porte, sem `zlib` nem `Buffer`: os bytes de entrada da descompressão vão como literais.
const zlib = require("zlib");
const bytes = (buffer) => `new Uint8Array([${[...buffer].join(",")}])`;
const chunks = (...buffers) => `[${buffers.map(bytes).join(", ")}]`;

// Forma das classes.
for (const N of ["CompressionStream", "DecompressionStream"]) {
  expr(`DESC(globalThis, '${N}')`);
  expr(`[${N}.length, ${N}.name, Object.getOwnPropertyNames(${N}), Reflect.ownKeys(${N}.prototype).map(String)]`);
  expr(`[Object.getPrototypeOf(${N}.prototype) === Object.prototype, DESC(${N}.prototype, Symbol.toStringTag), DESC(${N}.prototype, 'constructor')]`);
  expr(`Object.getOwnPropertyNames(${N}.prototype).map(function (k) { return [k, DESC(${N}.prototype, k)] })`);
  expr(`DESC(${N}.prototype, Symbol.for('nodejs.util.inspect.custom'))`);
  expr(`Object.prototype.toString.call(new ${N}('gzip'))`);
  expr(`[new ${N}('gzip') instanceof TransformStream, Object.keys(new ${N}('gzip')), new ${N}('gzip').readable instanceof ReadableStream, new ${N}('gzip').writable instanceof WritableStream]`);
  expr(`(function () { var s = new ${N}('gzip'); return [s.readable === s.readable, s.writable === s.writable] })()`);
  expr(`${N}('gzip')`);
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, 'readable').get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(${N}.prototype, 'writable').get.call(new ${N === "CompressionStream" ? "DecompressionStream" : "CompressionStream"}('gzip'))`);
  // Inspect custom chamado direto (o `util.inspect` monta `depth` e `options` assim).
  for (const [outer, opts] of [["2", "{ depth: 2 }"], ["1", "{ depth: 1 }"], ["0", "{ depth: 0 }"], ["-1", "{ depth: -1 }"], ["2", "{ depth: 2, breakLength: Infinity }"], ["2", "{ depth: 2, breakLength: 20 }"], ["2", "{}"]]) {
    expr(`new ${N}('deflate')[Symbol.for('nodejs.util.inspect.custom')](${outer}, ${opts})`);
  }
  // Formatos.
  for (const f of ["gzip", "deflate", "deflate-raw", "brotli", "zstd", "GZIP", "", "x", "gzip ", "Deflate"]) expr(`(new ${N}(${JSON.stringify(f)}), 'ok')`);
  expr(`new ${N}()`);
  expr(`new ${N}(undefined)`);
  expr(`new ${N}(null)`);
  expr(`new ${N}(5)`);
  expr(`new ${N}({ toString: function () { return 'gzip' } }) && 'ok'`);
  expr(`new ${N}(Symbol('s'))`);
}

// Bytes exatos da compressão de casos pequenos (nível padrão do zlib).
for (const f of ["gzip", "deflate", "deflate-raw", "brotli", "zstd"]) {
  run(`CompressionStream('${f}')`, "[]");
  run(`CompressionStream('${f}')`, `[${text("hello world")}]`);
  run(`CompressionStream('${f}')`, `[${text("hello ")}, ${text("world")}]`);
  run(`CompressionStream('${f}')`, `[${text("a")}, ${u8()}, ${text("abc".repeat(40))}]`);
}
run("CompressionStream('gzip')", `[${text("hello ")}, ${text("world")}]`);
run("CompressionStream('deflate')", `[${text("hello ")}, ${text("world")}]`);
run("CompressionStream('deflate-raw')", `[${text("hello ")}, ${text("world")}]`);
// Pedaços que não são BufferSource.
for (const f of ["gzip", "deflate"]) {
  run(`CompressionStream('${f}')`, "['abc']");
  run(`CompressionStream('${f}')`, "[5]");
  run(`CompressionStream('${f}')`, "[null]");
  run(`CompressionStream('${f}')`, "[undefined]");
  run(`CompressionStream('${f}')`, "[{}]");
  run(`CompressionStream('${f}')`, "[[1, 2, 3]]");
  run(`CompressionStream('${f}')`, "[new Uint8Array([1, 2, 3]).buffer]");
  run(`CompressionStream('${f}')`, "[new DataView(new ArrayBuffer(3))]");
  run(`CompressionStream('${f}')`, "[new Uint16Array([1, 2])]");
  run(`CompressionStream('${f}')`, "[new Uint8Array(new SharedArrayBuffer(3))]");
  run(`DecompressionStream('${f}')`, "['abc']");
  run(`DecompressionStream('${f}')`, "[5]");
  run(`DecompressionStream('${f}')`, "[null]");
  run(`DecompressionStream('${f}')`, "[{}]");
}
// Saída grande: ida e volta, e o número de pedaços e o tamanho total.
for (const f of ["gzip", "deflate", "deflate-raw"]) {
  abody(`return await ROUND('${f}', new Uint8Array(100000).fill(97))`);
  abody(`var d = new Uint8Array(70000); for (var i = 0; i < d.length; i++) d[i] = (i * 7 + (i >> 8)) & 255; return await ROUND('${f}', d)`);
  abody(`var d = new Uint8Array(300000); var s = 12345; for (var i = 0; i < d.length; i++) { s = (s * 1103515245 + 12345) & 0x7fffffff; d[i] = s >> 16 } return await ROUND('${f}', d)`);
  run(`CompressionStream('${f}')`, "[new Uint8Array(100000).fill(97)]", true);
  run(`CompressionStream('${f}')`, "[new Uint8Array(100000).fill(97), new Uint8Array(100000).fill(98)]", true);
}
// Descompressão: dado válido, em pedaços, corrompido, truncado, vazio, com sobra, concatenado.
const ZL = (fn) => bytes(zlib[fn](Buffer.from("hello world")));
const GZ = ZL("gzipSync");
for (const [f, fn] of [["gzip", "gzipSync"], ["deflate", "deflateSync"], ["deflate-raw", "deflateRawSync"]]) {
  const D = `DecompressionStream('${f}')`;
  run(D, `[${ZL(fn)}]`);
  run(D, `[${ZL(fn)}.subarray(0, 5), ${ZL(fn)}.subarray(5)]`);
  run(D, `[${ZL(fn)}.subarray(0, 3), ${ZL(fn)}.subarray(3, 9), ${ZL(fn)}.subarray(9)]`);
  run(D, `[${ZL(fn)}.subarray(0, ${ZL(fn)}.length - 3)]`);
  run(D, `[${ZL(fn)}.subarray(0, ${ZL(fn)}.length - 6)]`);
  run(D, `[${ZL(fn)}.subarray(0, 2)]`);
  run(D, `[${ZL(fn)}.subarray(0, 1)]`);
  run(D, "[]");
  run(D, `[${u8()}]`);
  run(D, `[${u8(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11)}]`);
  run(D, `[${u8(1, 2, 3, 4, 5)}]`);
  run(D, `[${u8(7, 7, 7, 7, 7)}]`);
  run(D, `[${u8(1, 2, 3)}]`);
  run(D, `[Buffer.concat([${ZL(fn)}, Buffer.from([1, 2, 3])])]`);
  run(D, `[Buffer.concat([${ZL(fn)}, Buffer.from([0])])]`);
  run(D, `[${ZL(fn)}, ${u8(1, 2, 3)}]`);
  run(D, `[${ZL(fn)}, ${u8()}]`);
  run(D, `[Buffer.concat([${ZL(fn)}, ${ZL(fn)}])]`);
  run(D, `[${ZL(fn)}, ${ZL(fn)}]`);
  run(D, `[Buffer.from(${ZL(fn)}.map(function (b, i) { return i === 8 ? b ^ 255 : b }))]`);
  run(D, `[Buffer.from(${ZL(fn)}.map(function (b, i, a) { return i === a.length - 1 ? b ^ 255 : b }))]`);
  run(D, `[Buffer.from(${ZL(fn)}.map(function (b, i, a) { return i === a.length - 5 ? b ^ 255 : b }))]`);
}
run("DecompressionStream('gzip')", `[Buffer.concat([${GZ}, ${GZ}])]`);
run("DecompressionStream('gzip')", `[Buffer.concat([${GZ}, ${GZ}]).subarray(0, 40)]`);
run("DecompressionStream('gzip')", `[Buffer.concat([${GZ}, ${GZ}]).subarray(0, 32)]`);
run("DecompressionStream('gzip')", `[Buffer.concat([${GZ}, ${GZ}]).subarray(0, 28)]`);
run("DecompressionStream('gzip')", `[${GZ}, ${u8(0x1f, 0x8b)}]`);
run("DecompressionStream('gzip')", `[${GZ}, ${u8(0)}]`);
run("DecompressionStream('gzip')", `[${u8(0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0)}]`);
run("DecompressionStream('gzip')", `[${u8(0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 3, 97, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0)}]`);
run("DecompressionStream('gzip')", `[${u8(0x1f, 0x8b, 9, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0)}]`);
run("DecompressionStream('deflate')", `[${u8(0x78, 0x9c, 3, 0, 0, 0, 0, 1)}]`);
run("DecompressionStream('deflate')", `[${u8(0x78, 0x9c, 3, 0, 0, 0, 0, 2)}]`);
run("DecompressionStream('deflate')", `[${u8(0x78, 0x01, 3, 0, 0, 0, 0, 1)}]`);
run("DecompressionStream('deflate')", `[${u8(0x78, 0xda, 3, 0, 0, 0, 0, 1)}]`);

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
