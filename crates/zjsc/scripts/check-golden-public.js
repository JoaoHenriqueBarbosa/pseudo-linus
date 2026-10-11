#!/usr/bin/env node
// Varre tests/golden/*.tsv e *.preludes.json atrás de caminho da máquina de quem gerou (o repositório é público).
// Uso: node scripts/check-golden-public.js   (sai com 1 se algum arquivo vazar)
const fs = require("fs");
const path = require("path");
const { findLeak, GOLDEN_DIR } = require("./golden-prelude.js");

let leaks = 0;
let scanned = 0;
for (const name of fs.readdirSync(GOLDEN_DIR).sort()) {
  if (!name.endsWith(".tsv") && !name.endsWith(".preludes.json")) continue;
  scanned++;
  const text = fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8");
  const leak = findLeak(text);
  if (leak) {
    leaks++;
    console.log(`${name}: ${leak.label}: ...${leak.sample.replace(/\n/g, "\\n")}...`);
  }
}
console.log(`${scanned} arquivos varridos, ${leaks} com vazamento`);
process.exit(leaks ? 1 : 0);
