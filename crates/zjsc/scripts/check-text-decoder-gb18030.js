// Confere o modelo do decodificador `gb18030` do WHATWG (o mesmo que `src/runtime/text_decoder.rs` porta, escrito aqui
// literalmente como o padrão, com a fila e o "prepend") contra o `TextDecoder` do bun 1.4.2 em sequências aleatórias de
// blocos (com `stream` e `fatal`, rótulos `gb18030` e `gbk`). As tabelas saem de `src/runtime/text_decoder_gb18030_data.rs`,
// então o script confere também o arquivo gerado. Sai com código 1 se algum caso diverge.
// Uso: bun scripts/check-text-decoder-gb18030.js [casos] [semente]
const fs = require("fs");
const path = require("path");
const cases = Number(process.argv[2] || 30000);
let seed = Number(process.argv[3] || 12345);
const random = () => {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 4294967296;
};

const source = fs.readFileSync(path.join(__dirname, "../src/runtime/text_decoder_gb18030_data.rs"), "utf8");
const ranges = [...source.slice(source.indexOf("GB18030_RANGES: ["), source.indexOf("GB18030_INDEX: [")).matchAll(/\((\d+), (\d+)\)/g)].map((m) => [Number(m[1]), Number(m[2])]);
const index = source
  .slice(source.indexOf("GB18030_INDEX: ["))
  .split("\n")
  .slice(1)
  .filter((line) => /^\s+\d/.test(line))
  .flatMap((line) => line.match(/\d+/g).map(Number));
if (index.length !== 126 * 190) throw new Error(`tabela de dois bytes com ${index.length} entradas`);

const fourByte = (pointer) => {
  let found = ranges[0];
  for (const range of ranges) if (range[0] <= pointer) found = range;
  return found[1] === 0 ? null : found[1] + pointer - found[0];
};

// Um decodificador do padrão: devolve a lista de pontos de código (`null` é erro).
const newState = () => ({ first: 0, second: 0, third: 0 });
function run(state, input, finish) {
  const queue = Array.from(input);
  const out = [];
  for (;;) {
    let byte = queue.length ? queue.shift() : null;
    if (byte === null) {
      if (!finish) return out;
      if (state.first || state.second || state.third) {
        state.first = state.second = state.third = 0;
        out.push(null);
        continue;
      }
      return out;
    }
    if (state.third !== 0) {
      if (byte < 0x30 || byte > 0x39) {
        queue.unshift(state.second, state.third, byte);
        state.first = state.second = state.third = 0;
        out.push(null);
        continue;
      }
      const pointer = ((state.first - 0x81) * 10 + state.second - 0x30) * 1260 + (state.third - 0x81) * 10 + byte - 0x30;
      state.first = state.second = state.third = 0;
      out.push(fourByte(pointer));
      continue;
    }
    if (state.second !== 0) {
      if (byte >= 0x81 && byte <= 0xfe) {
        state.third = byte;
        continue;
      }
      queue.unshift(state.second, byte);
      state.first = state.second = 0;
      out.push(null);
      continue;
    }
    if (state.first !== 0) {
      if (byte >= 0x30 && byte <= 0x39) {
        state.second = byte;
        continue;
      }
      const lead = state.first;
      state.first = 0;
      let point = 0;
      if ((byte >= 0x40 && byte <= 0x7e) || (byte >= 0x80 && byte <= 0xfe)) {
        point = index[(lead - 0x81) * 190 + byte - (byte < 0x7f ? 0x40 : 0x41)];
      }
      if (point) {
        out.push(point);
        continue;
      }
      if (byte < 0x80) queue.unshift(byte);
      out.push(null);
      continue;
    }
    if (byte < 0x80) out.push(byte);
    else if (byte === 0x80) out.push(0x20ac);
    else if (byte >= 0x81 && byte <= 0xfe) state.first = byte;
    else out.push(null);
  }
}

// A saída de `run` como o texto do `decode` (null vira U+FFFD) ou a exceção do `fatal`.
const toText = (points, fatal) => {
  if (fatal && points.includes(null)) return "ERR";
  return JSON.stringify(points.map((p) => (p === null ? 0xfffd : p)));
};
const cpList = (text) => JSON.stringify(Array.from(text, (c) => c.codePointAt(0)));

// Bytes que exercitam as bordas: leads, dígitos, trails, ASCII, 0x80 e 0xFF, e inícios de quatro bytes válidos.
const pool = [0x00, 0x30, 0x35, 0x39, 0x41, 0x7e, 0x7f, 0x80, 0x81, 0x84, 0x90, 0xa1, 0xa2, 0xa8, 0xe3, 0xfe, 0xff, 0x40, 0x31, 0x32, 0x9a, 0xa4, 0xa5];
const pick = () => (random() < 0.7 ? pool[Math.floor(random() * pool.length)] : Math.floor(random() * 256));

let failures = 0;
for (let n = 0; n < cases; n++) {
  const label = random() < 0.5 ? "gb18030" : "gbk";
  const fatal = random() < 0.3;
  const chunks = [];
  const count = 1 + Math.floor(random() * 4);
  for (let c = 0; c < count; c++) {
    const length = Math.floor(random() * 8);
    chunks.push({ bytes: Array.from({ length }, pick), stream: c < count - 1 ? random() < 0.8 : random() < 0.2 });
  }
  const decoder = new TextDecoder(label, { fatal });
  const state = newState();
  for (const { bytes, stream } of chunks) {
    let expected;
    const saved = { ...state };
    const points = run(state, bytes, !stream);
    expected = toText(points, fatal);
    let actual;
    try {
      actual = cpList(decoder.decode(new Uint8Array(bytes), { stream }));
    } catch (error) {
      actual = "ERR";
    }
    if (expected !== actual) {
      failures++;
      if (failures <= 10) console.log("diverge", label, fatal, JSON.stringify(chunks), "esperado", expected, "bun", actual);
      break;
    }
    if (actual === "ERR") break;
  }
}
console.log(`${cases} casos, ${failures} divergências`);
process.exit(failures ? 1 : 0);
