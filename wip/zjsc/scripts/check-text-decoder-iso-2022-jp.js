// Confere o modelo do decodificador `iso-2022-jp` do WHATWG (o mesmo que `src/runtime/text_decoder.rs` porta) contra o
// `TextDecoder` do bun 1.4.2 em sequências aleatórias de blocos (com `stream` e `fatal`). Sai com código 1 se algum
// caso diverge. A tabela jis0208 sai do próprio bun (decodificando o `euc-jp`, que usa o mesmo ponteiro de 94 * 94).
// Uso: bun scripts/check-text-decoder-iso-2022-jp.js [casos] [semente]
const cases = Number(process.argv[2] || 20000);
let seed = Number(process.argv[3] || 12345);
const random = () => {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 4294967296;
};

const eucJp = new TextDecoder("euc-jp");
const jis0208 = new Array(94 * 94).fill(0);
for (let lead = 0; lead < 94; lead++) {
  for (let trail = 0; trail < 94; trail++) {
    const text = eucJp.decode(new Uint8Array([0xa1 + lead, 0xa1 + trail]));
    const unit = text.codePointAt(0);
    jis0208[lead * 94 + trail] = text.length === 1 && unit !== 0xfffd ? unit : 0;
  }
}

const initial = () => ({ mode: "ascii", output: "ascii", flag: false, lead: 0 });

// Devolve a lista de unidades (`null` é erro). `finish` é o fim do fluxo (`decode` sem `stream`).
function run(state, input, finish) {
  const queue = Array.from(input);
  const out = [];
  for (;;) {
    const byte = queue.length ? queue.shift() : null;
    if (byte === null && !finish) return out;
    const ascii = (b) => b !== null && b <= 0x7f && b !== 0x0e && b !== 0x0f && b !== 0x1b;
    switch (state.mode) {
      case "ascii":
      case "roman":
      case "katakana":
      case "lead": {
        if (byte === 0x1b) {
          state.mode = "escapeStart";
          break;
        }
        if (byte === null) return out;
        if (state.mode === "ascii" && ascii(byte)) {
          state.flag = false;
          out.push(byte);
        } else if (state.mode === "roman" && ascii(byte)) {
          state.flag = false;
          out.push(byte === 0x5c ? 0xa5 : byte === 0x7e ? 0x203e : byte);
        } else if (state.mode === "katakana" && byte >= 0x21 && byte <= 0x5f) {
          state.flag = false;
          out.push(0xff61 - 0x21 + byte);
        } else if (state.mode === "lead" && byte >= 0x21 && byte <= 0x7e) {
          state.flag = false;
          state.lead = byte;
          state.mode = "trail";
        } else {
          state.flag = false;
          out.push(null);
        }
        break;
      }
      case "trail": {
        if (byte === 0x1b) {
          state.mode = "escapeStart";
          out.push(null);
        } else if (byte !== null && byte >= 0x21 && byte <= 0x7e) {
          state.mode = "lead";
          out.push(jis0208[(state.lead - 0x21) * 94 + byte - 0x21] || null);
        } else {
          state.mode = "lead";
          out.push(null);
        }
        break;
      }
      case "escapeStart": {
        if (byte === 0x24 || byte === 0x28) {
          state.lead = byte;
          state.mode = "escape";
        } else {
          if (byte !== null) queue.unshift(byte);
          state.flag = false;
          state.mode = state.output;
          out.push(null);
        }
        break;
      }
      case "escape": {
        const lead = state.lead;
        state.lead = 0;
        let next = null;
        if (lead === 0x28 && byte === 0x42) next = "ascii";
        else if (lead === 0x28 && byte === 0x4a) next = "roman";
        else if (lead === 0x28 && byte === 0x49) next = "katakana";
        else if (lead === 0x24 && (byte === 0x40 || byte === 0x42)) next = "lead";
        if (next !== null) {
          state.mode = state.output = next;
          const was = state.flag;
          state.flag = true;
          if (was) out.push(null);
        } else {
          if (byte !== null) queue.unshift(byte);
          queue.unshift(lead);
          state.flag = false;
          state.mode = state.output;
          out.push(null);
        }
        break;
      }
    }
  }
}

// Um `decode`: devolve o texto ou `"ERR"`. O erro com `fatal` volta o estado ao inicial.
function modelDecode(box, bytes, stream, fatal) {
  const units = run(box.state, bytes, !stream);
  if (!stream) box.state = initial();
  if (fatal && units.includes(null)) {
    box.state = initial();
    return "ERR";
  }
  return units.map((unit) => (unit === null ? "�" : String.fromCodePoint(unit))).join("");
}

const alphabet = [0x1b, 0x1b, 0x1b, 0x24, 0x24, 0x28, 0x28, 0x40, 0x42, 0x42, 0x4a, 0x49, 0x0a, 0x0e, 0x0f, 0x5c, 0x7e, 0x80, 0xff, 0x30, 0x21, 0x21, 0x41, 0x5f, 0x60, 0x7f, 0x00];
const pick = () => (random() < 0.8 ? alphabet[Math.floor(random() * alphabet.length)] : Math.floor(random() * 256));
let bad = 0;
for (let n = 0; n < cases && bad < 10; n++) {
  const fatal = random() < 0.3;
  const real = new TextDecoder("iso-2022-jp", fatal ? { fatal: true } : {});
  const box = { state: initial() };
  const calls = 1 + Math.floor(random() * 5);
  const trace = [];
  for (let c = 0; c < calls; c++) {
    const length = Math.floor(random() * 9);
    const bytes = Array.from({ length }, pick);
    const stream = random() < 0.6;
    let expected;
    let got;
    try {
      got = real.decode(new Uint8Array(bytes), stream ? { stream: true } : {});
    } catch (e) {
      got = "ERR";
    }
    expected = modelDecode(box, bytes, stream, fatal);
    trace.push(`${JSON.stringify(bytes.map((b) => b.toString(16)))}${stream ? "s" : ""} -> ${JSON.stringify(got)} (modelo ${JSON.stringify(expected)})`);
    if (got !== expected) {
      bad++;
      console.log(`DIVERGE fatal=${fatal}\n  ${trace.join("\n  ")}`);
      break;
    }
  }
}
console.log(bad === 0 ? `ok: ${cases} casos` : `${bad} divergências`);
process.exit(bad === 0 ? 0 : 1);
