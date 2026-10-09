const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/e2e_numeric.tsv: cada programa numérico simples avaliado no bun 1.4.2.
// Colunas: fonte do programa (uma linha), bits do double do resultado em hexadecimal (16 dígitos).
// Uso: bun scripts/gen-e2e-golden.js > tests/golden/e2e_numeric.tsv
const programs = [
  "1 + 1",
  "1 + 2 * 3",
  "10 - 4 - 3",
  "7 / 2",
  "7 % 3",
  "-7 % 3",
  "2 ** 10",
  "2 ** 0.5",
  "0.1 + 0.2",
  "1 / 0",
  "-1 / 0",
  "0 / 0",
  "1 << 31",
  "1 << 33",
  "-8 >> 1",
  "-1 >>> 0",
  "-1 >>> 28",
  "12 & 10",
  "12 | 10",
  "12 ^ 10",
  "~0",
  "~5",
  "4294967296 + 5 | 0",
  "1 < 2 ? 10 : 20",
  "var x = 1; x + 1",
  "var x = 5; var y = 7; x * y",
  "var x = 3; x = x + 4; x",
  "function f() { return 1 + 1 } f()",
  "function f(a, b) { return a * b } f(6, 7)",
  "let a = 2; const b = 3; a ** b",
];

const buf = new DataView(new ArrayBuffer(8));
function bits(x) {
  buf.setFloat64(0, x);
  return buf.getBigUint64(0).toString(16).padStart(16, "0");
}

for (const src of programs) {
  const result = (0, eval)(src);
  if (typeof result !== "number") throw new Error(`${src} não devolve número`);
  emitRow(`${src}\t${bits(result)}`);
}
