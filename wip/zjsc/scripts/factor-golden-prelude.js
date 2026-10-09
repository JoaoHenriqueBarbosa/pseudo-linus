// Uso único: tira o prelúdio comum das linhas de goldens `tests/golden/<nome>_bun.tsv` (ver scripts/golden-prelude.js).
// Antes de sobrescrever, reconstrói cada programa a partir de prelúdio + sufixo e compara byte a byte com o tsv antigo;
// qualquer divergência aborta sem tocar em nada.
// Uso: node scripts/factor-golden-prelude.js nome1 nome2 ...   (sem o sufixo `_bun`)
const fs = require("fs");
const path = require("path");
const { factorRows, writePreludes, GOLDEN_DIR } = require("./golden-prelude.js");

for (const name of process.argv.slice(2)) {
  const tsvPath = path.join(GOLDEN_DIR, `${name}_bun.tsv`);
  const oldText = fs.readFileSync(tsvPath, "utf8");
  const oldLines = oldText.split("\n").filter(Boolean);
  const rows = oldLines.map((line) => {
    const [source, result, meta] = line.split("\t");
    const row = { source: JSON.parse(source), result: JSON.parse(result) };
    if (meta !== undefined) row.meta = JSON.parse(meta);
    return row;
  });
  const { preludes, lines } = factorRows(rows);

  lines.forEach((line, i) => {
    const [suffix, result, index] = line.split("\t");
    const source = preludes[index === undefined ? 0 : Number(index)] + JSON.parse(suffix);
    if (source !== rows[i].source || JSON.parse(result) !== rows[i].result) {
      throw new Error(`${name}: linha ${i + 1} não reconstrói o programa original`);
    }
  });

  writePreludes(name, preludes);
  const newText = lines.join("\n") + "\n";
  fs.writeFileSync(tsvPath, newText);
  const sizes = preludes.map((prelude) => prelude.length).join("+");
  console.log(`${name}: ${rows.length} linhas, prelúdios ${sizes} bytes, ${oldText.length} -> ${newText.length} bytes`);
}
