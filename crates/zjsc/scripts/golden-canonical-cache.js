// Monta o cache em disco da forma canônica dos goldens (ver `knownProgramSet` em golden-prelude.js).
// Uso: bun scripts/golden-canonical-cache.js ARQ.tsv...   (pai: reparte os arquivos entre subprocessos)
//      bun scripts/golden-canonical-cache.js --one ARQ.tsv (filho: canoniza um arquivo)
// Cada arquivo é independente, então o pai roda um filho por núcleo, com o maior arquivo primeiro. Progresso em stderr.
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");
const { buildCanonicalCache, GOLDEN_DIR } = require("./golden-prelude.js");

const args = process.argv.slice(2);
if (args[0] === "--one") {
  buildCanonicalCache(args[1]);
  process.exit(0);
}

const queue = args.slice().sort((a, b) => fs.statSync(path.join(GOLDEN_DIR, b)).size - fs.statSync(path.join(GOLDEN_DIR, a)).size);
const total = queue.length;
let done = 0;
let failed = false;
const started = Date.now();
const worker = async () => {
  while (queue.length > 0 && !failed) {
    const name = queue.shift();
    const code = await new Promise((resolve) => {
      const child = spawn(process.execPath, [__filename, "--one", name], { stdio: ["ignore", "inherit", "inherit"] });
      child.on("close", resolve);
    });
    if (code !== 0) {
      failed = true;
      process.stderr.write(`falhou: ${name} (código ${code})\n`);
      return;
    }
    done++;
    process.stderr.write(`canonizado ${done}/${total} ${name} (${Math.round((Date.now() - started) / 1000)}s)\n`);
  }
};
Promise.all(Array.from({ length: Math.max(1, os.cpus().length) }, worker)).then(() => process.exit(failed ? 1 : 0));
