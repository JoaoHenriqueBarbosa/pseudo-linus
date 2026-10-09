// Uso único: converte os goldens mapeados (quinta coluna com as corridas do mapa de posições) do formato em que cada linha
// repetia as corridas do prelúdio para o fatorado (ver `factorMeta` em scripts/golden-prelude.js). Não roda o bun: lê o
// tsv e o `*.preludes.json` atuais, fatora, e antes de gravar monta tudo de volta com `readRows` e exige que cada linha
// (programa, resultado, alternativas e meta completo) saia idêntica à original. Qualquer divergência aborta sem gravar.
// Uso: node scripts/factor-golden-meta.js nome1 nome2 ...   (sem o sufixo `_bun`)
const fs = require("fs");
const path = require("path");
const { factorMeta, normalizePrelude, readRows, writePreludes, preludesPath, GOLDEN_DIR } = require("./golden-prelude.js");

for (const name of process.argv.slice(2)) {
  const tsvPath = path.join(GOLDEN_DIR, `${name}_bun.tsv`);
  const oldText = fs.readFileSync(tsvPath, "utf8");
  const oldPreludesText = fs.readFileSync(preludesPath(name), "utf8");
  const preludes = JSON.parse(oldPreludesText).map(normalizePrelude);
  if (preludes.some((prelude) => prelude.factored)) {
    console.log(`${name}: já fatorado`);
    continue;
  }
  const texts = preludes.map((prelude) => prelude.text);
  const oldRows = readRows(name, oldText);
  const columns = oldText.split("\n").filter(Boolean).map((line) => line.split("\t"));
  const entries = columns.map((cells) => ({ index: Number(cells[2] || 0), meta: cells[4] === undefined ? null : JSON.parse(cells[4]) }));
  const { preludeRuns, metas } = factorMeta(texts, entries);

  const newLines = columns.map((cells, i) => {
    if (metas[i]) cells[4] = JSON.stringify(metas[i]);
    return cells.join("\t");
  });
  const newText = newLines.join("\n") + "\n";

  // Verificação: grava os prelúdios novos, relê tudo e compara com as linhas originais; restaura se divergir.
  writePreludes(name, texts, preludeRuns);
  let verified = true;
  try {
    const newRows = readRows(name, newText);
    verified = newRows.length === oldRows.length && newRows.every((row, i) => JSON.stringify(row) === JSON.stringify(oldRows[i]));
  } finally {
    if (!verified) fs.writeFileSync(preludesPath(name), oldPreludesText);
  }
  if (!verified) throw new Error(`${name}: a montagem das linhas novas diverge das originais, nada foi gravado`);
  fs.writeFileSync(tsvPath, newText);
  console.log(`${name}: ${oldRows.length} linhas, ${oldText.length} -> ${newText.length} bytes (tsv), preludes.json ${oldPreludesText.length} -> ${fs.statSync(preludesPath(name)).size} bytes`);
}
