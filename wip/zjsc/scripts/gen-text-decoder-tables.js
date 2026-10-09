// Gera src/runtime/text_decoder_single_byte_data.rs: as codificações de byte único do WHATWG Encoding medidas no
// `TextDecoder` do bun 1.4.2. Para cada nome canônico (o `encoding` que o bun devolve para algum rótulo candidato):
// os rótulos que o bun aceita e a tabela dos bytes 0x80..0xFF (decodificados um a um, com `fatal`; o byte que o bun
// recusa é "indefinido" e vira U+FFFD sem `fatal`). Os rótulos candidatos são a tabela de rótulos do WHATWG; o que o
// bun não aceita cai fora, e o nome canônico vem sempre do bun. Os bytes 0x00..0x7F têm de ser ASCII em todas
// (o gerador aborta se alguma divergir).
// Uso: bun scripts/gen-text-decoder-tables.js > src/runtime/text_decoder_single_byte_data.rs
const CANDIDATES = [
  "866 cp866 csibm866 ibm866",
  "csisolatin2 iso-8859-2 iso-ir-101 iso8859-2 iso88592 iso_8859-2 iso_8859-2:1987 l2 latin2",
  "csisolatin3 iso-8859-3 iso-ir-109 iso8859-3 iso88593 iso_8859-3 iso_8859-3:1988 l3 latin3",
  "csisolatin4 iso-8859-4 iso-ir-110 iso8859-4 iso88594 iso_8859-4 iso_8859-4:1988 l4 latin4",
  "csisolatincyrillic cyrillic iso-8859-5 iso-ir-144 iso8859-5 iso88595 iso_8859-5 iso_8859-5:1988",
  "arabic asmo-708 csiso88596e csiso88596i csisolatinarabic ecma-114 iso-8859-6 iso-8859-6-e iso-8859-6-i iso-ir-127 iso8859-6 iso88596 iso_8859-6 iso_8859-6:1987",
  "csisolatingreek ecma-118 elot_928 greek greek8 iso-8859-7 iso-ir-126 iso8859-7 iso88597 iso_8859-7 iso_8859-7:1987 sun_eu_greek",
  "csiso88598e csisolatinhebrew hebrew iso-8859-8 iso-8859-8-e iso-ir-138 iso8859-8 iso88598 iso_8859-8 iso_8859-8:1988 visual",
  "csiso88598i iso-8859-8-i logical",
  "csisolatin6 iso-8859-10 iso-ir-157 iso8859-10 iso885910 l6 latin6",
  "iso-8859-13 iso8859-13 iso885913",
  "iso-8859-14 iso8859-14 iso885914",
  "csisolatin9 iso-8859-15 iso8859-15 iso885915 iso_8859-15 l9",
  "iso-8859-16",
  "cskoi8r koi koi8 koi8-r koi8_r",
  "koi8-ru koi8-u",
  "csmacintosh mac macintosh x-mac-roman",
  "dos-874 iso-8859-11 iso8859-11 iso885911 tis-620 windows-874",
  "cp1250 windows-1250 x-cp1250",
  "cp1251 windows-1251 x-cp1251",
  "ansi_x3.4-1968 ascii cp1252 cp819 csisolatin1 ibm819 iso-8859-1 iso-ir-100 iso8859-1 iso88591 iso_8859-1 iso_8859-1:1987 l1 latin1 us-ascii windows-1252 x-cp1252",
  "cp1253 windows-1253 x-cp1253",
  "cp1254 csisolatin5 iso-8859-9 iso-ir-148 iso8859-9 iso88599 iso_8859-9 iso_8859-9:1989 l5 latin5 windows-1254 x-cp1254",
  "cp1255 windows-1255 x-cp1255",
  "cp1256 windows-1256 x-cp1256",
  "cp1257 windows-1257 x-cp1257",
  "cp1258 windows-1258 x-cp1258",
  "x-mac-cyrillic x-mac-ukrainian",
  "x-user-defined",
].flatMap((line) => line.split(" "));

const byName = new Map();
for (const label of CANDIDATES) {
  let decoder;
  try {
    decoder = new TextDecoder(label);
  } catch (error) {
    continue;
  }
  if (!byName.has(decoder.encoding)) byName.set(decoder.encoding, []);
  byName.get(decoder.encoding).push(label);
}

function table(name) {
  const decoder = new TextDecoder(name, { fatal: true });
  for (let byte = 0; byte < 0x80; byte++) {
    if (decoder.decode(new Uint8Array([byte])) !== String.fromCharCode(byte)) throw new Error(`${name}: byte ${byte} não é ASCII`);
  }
  const high = [];
  for (let byte = 0x80; byte < 0x100; byte++) {
    let text;
    try {
      text = decoder.decode(new Uint8Array([byte]));
    } catch (error) {
      high.push(0xffff);
      continue;
    }
    const points = Array.from(text);
    if (points.length !== 1 || points[0].codePointAt(0) > 0xfffe) throw new Error(`${name}: byte ${byte} deu ${JSON.stringify(text)}`);
    high.push(points[0].codePointAt(0));
  }
  return high;
}

const names = [...byName.keys()].sort();
const out = [];
out.push("//! As codificações de byte único do WHATWG Encoding, medidas no `TextDecoder` do bun 1.4.2: o nome canônico, os rótulos");
out.push("//! aceitos e o ponto de código de cada byte de 0x80 a 0xFF (`UNDEFINED` é o byte que o bun recusa: U+FFFD, ou erro com");
out.push("//! `fatal`). Os bytes de 0x00 a 0x7F são ASCII em todas.");
out.push("//!");
out.push("//! ARQUIVO GERADO, não edite à mão. Para regenerar (da raiz do crate, com o bun no PATH):");
out.push("//!");
out.push("//! ```sh");
out.push("//! bun scripts/gen-text-decoder-tables.js > src/runtime/text_decoder_single_byte_data.rs");
out.push("//! ```");
out.push("");
out.push("/// O byte sem mapeamento na tabela.");
out.push("pub(crate) const UNDEFINED: u16 = 0xFFFF;");
out.push("");
out.push("/// Uma codificação de byte único.");
out.push("pub(crate) struct SingleByteEncoding {");
out.push("    pub(crate) name: &'static str,");
out.push("    pub(crate) labels: &'static [&'static str],");
out.push("    /// O ponto de código dos bytes 0x80..=0xFF.");
out.push("    pub(crate) high: [u16; 128],");
out.push("}");
out.push("");
out.push(`pub(crate) static SINGLE_BYTE_ENCODINGS: [SingleByteEncoding; ${names.length}] = [`);
for (const name of names) {
  const labels = byName.get(name).sort().map((label) => JSON.stringify(label)).join(", ");
  const high = table(name);
  out.push("    SingleByteEncoding {");
  out.push(`        name: ${JSON.stringify(name)},`);
  out.push(`        labels: &[${labels}],`);
  out.push("        high: [");
  for (let row = 0; row < 8; row++) {
    const cells = high.slice(row * 16, row * 16 + 16).map((value) => (value === 0xffff ? "UNDEFINED" : "0x" + value.toString(16).toUpperCase().padStart(4, "0")));
    out.push(`            ${cells.join(", ")},`);
  }
  out.push("        ],");
  out.push("    },");
}
out.push("];");
process.stdout.write(out.join("\n") + "\n");
