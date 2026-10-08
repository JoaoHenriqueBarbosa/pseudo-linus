// Gera tests/golden/case_mapping.tsv no bun 1.4.2: cada ponto de código cujo toLowerCase ou
// toUpperCase muda, e strings de teste do sigma final. Colunas: entrada, minúsculas, maiúsculas,
// todas em hexadecimal UTF-16 separado por espaço.
const hex = (s) => [...Array(s.length).keys()].map((i) => s.charCodeAt(i).toString(16)).join(" ");
const out = [];
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c < 0xe000) continue;
  const s = String.fromCodePoint(c);
  const lo = s.toLowerCase(), up = s.toUpperCase();
  if (lo !== s || up !== s) out.push([hex(s), hex(lo), hex(up)].join("\t"));
}
for (const s of ["ΣΑΣ", "ΑΣ", "Σ", "ΑΣ Α", "Α.Σ", "ΑΣ.", "ΆΣ", "aΣb", "AΣ́", "İstanbul", "ß", "ﬃ", "ŉ"])
  out.push([hex(s), hex(s.toLowerCase()), hex(s.toUpperCase())].join("\t"));
console.log(out.join("\n"));
