// Confere `parse_label` de src/runtime/text_decoder.rs contra o bun 1.4.2 para todos os rótulos do WHATWG Encoding
// (scripts/text-decoder-labels.js) e variantes de caixa e de espaço. Mede no bun `new TextDecoder(l).encoding` ou o erro
// (nome, mensagem, code) e simula `parse_label` lendo as listas de rótulos dos arquivos de dados do porte.
// Uso: bun scripts/check-text-decoder-labels.js   (sai com 1 se algo divergir)
const fs = require("fs");
const path = require("path");
const { LABELS } = require("./text-decoder-labels.js");

const dir = path.join(__dirname, "..", "src", "runtime");
const read = (name) => fs.readFileSync(path.join(dir, name), "utf8");
const strings = (text) => [...text.matchAll(/"([^"]*)"/g)].map((m) => m[1]);
const constList = (source, name) => {
  const m = source.match(new RegExp(`const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\]`));
  if (!m) throw new Error("const não achada: " + name);
  return strings(m[1]);
};

const main = read("text_decoder.rs");
const arms = (variant) => {
  const lines = main.split("\n").filter((l) => l.includes(`=> Some(Encoding::${variant})`) && l.includes('"'));
  return lines.flatMap((l) => strings(l.split("=>")[0]));
};
// Na ordem de `parse_label`: nome canônico e rótulos.
const table = [
  ["utf-8", arms("Utf8")],
  ["utf-16le", arms("Utf16Le")],
  ["utf-16be", arms("Utf16Be")],
  ["gb18030", constList(read("text_decoder_gb18030_data.rs"), "GB18030_LABELS")],
  ["gbk", constList(read("text_decoder_gb18030_data.rs"), "GBK_LABELS")],
  ["euc-kr", constList(read("text_decoder_euc_kr_data.rs"), "LABELS")],
  ["shift_jis", constList(read("text_decoder_jis_data.rs"), "SHIFT_JIS_LABELS")],
  ["euc-jp", constList(read("text_decoder_jis_data.rs"), "EUC_JP_LABELS")],
  ["iso-2022-jp", arms("Iso2022Jp")],
  ["big5", constList(read("text_decoder_big5_data.rs"), "LABELS")],
];
const single = read("text_decoder_single_byte_data.rs");
for (const m of single.matchAll(/name: "([^"]+)",[\s\S]*?labels: &\[([^\]]*)\]/g)) table.push([m[1], strings(m[2])]);

const asciiLower = (s) => s.replace(/[A-Z]/g, (c) => c.toLowerCase());
const trimAscii = (s) => s.replace(/^[\t\n\f\r ]+/, "").replace(/[\t\n\f\r ]+$/, "");
const simulate = (label) => {
  const key = asciiLower(trimAscii(label));
  for (const [name, labels] of table) if (labels.includes(key)) return name;
  return "ERR|RangeError|" + `Unsupported encoding label "${label}"` + "|ERR_ENCODING_NOT_SUPPORTED";
};

const measure = (label) => {
  try {
    return new TextDecoder(label).encoding;
  } catch (e) {
    return "ERR|" + e.name + "|" + e.message + "|" + e.code;
  }
};

const inputs = [];
for (const [label] of LABELS) {
  inputs.push(label, label.toUpperCase(), " " + label + " ", "\t" + label + "\n", "\f\r" + label + "\x20\t", label[0].toUpperCase() + label.slice(1));
}
inputs.push("", " ", "x", "utf-8\0", "\x0butf-8", "\xa0utf-8", "K" + "oi8-r", "UTF-8K", "iso-2022-jp-2", "utf-32", "utf-7", "euc_kr", "big5-hkscs ");

let bad = 0;
for (const label of inputs) {
  const real = measure(label);
  const port = simulate(label);
  if (real !== port) {
    bad++;
    console.log("DIVERGE", JSON.stringify(label), "bun=", real, "porte=", port);
  }
}
// A tabela do WHATWG contra o bun: o rótulo tem de dar o nome canônico (ou o erro, para `replacement`).
for (const [label, name] of LABELS) {
  const real = measure(label);
  const expected = name;
  if (real !== expected) console.log("NOTA", JSON.stringify(label), "WHATWG=", expected, "bun=", real);
}
console.log(`${inputs.length} entradas, ${LABELS.length} rótulos, ${bad} divergências`);
process.exit(bad ? 1 : 0);
