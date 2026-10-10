// A execução comum dos goldens de grafos de módulos ES (gen-module-golden, gen-module-more-golden,
// gen-module-edge-golden, gen-module-meta-resolve-golden): grava o mapa de arquivos de cada caso, roda o `runner` no
// bun e escreve o tsv `JSON(arquivos) <TAB> JSON({log, error}) [<TAB> JSON([outras saídas aceitas])]`.
//
// A leitura de arquivos do bun corre num pool de threads: dois `import()` paralelos assentam em ordem variável
// (medido no bun 1.4.2, `Promise.all([import(a), import(b)])` dá `a,b` em ~70% das execuções e `b,a` no resto).
// Por isso cada caso roda várias vezes (`measureStable`), e as saídas além da menor viram a terceira coluna, lida por
// tests/common/module_golden.rs.
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");
const { measureStable } = require("./golden-alternatives.js");

const BUN = process.env.BUN || "bun";
// Com a variante rara em ~30% das execuções, sete delas ainda a perdem em 8% das vezes; vinte, em menos de 0,1%.
const RUNS = Number(process.env.RUNS || 20);
const PARALLEL = Number(process.env.PARALLEL || os.cpus().length);

// Uma execução do runner sobre `dir`: a última linha do stdout, ou a saída sintética de timeout/saída anormal.
function runOnce(runner, dir, tmp) {
  return new Promise((resolve) => {
    const child = spawn(BUN, [runner, dir], { cwd: tmp });
    let stdout = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.resume();
    const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
    child.on("close", (status) => {
      clearTimeout(timer);
      let out = stdout.trim().split("\n").pop() || "";
      if (status === null) out = JSON.stringify({ log: [], error: "timeout" });
      else if (!out.startsWith("{")) out = JSON.stringify({ log: [], error: "exit " + status });
      resolve({ ok: true, out, err: "" });
    });
  });
}

// Roda `cases` (mapas nome -> fonte) com o `runnerSource` e escreve o tsv no stdout. `label` nomeia o diretório
// temporário.
async function runModuleGolden(label, runnerSource, cases) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `zjsc-${label}-`));
  const runner = path.join(tmp, "runner.mjs");
  fs.writeFileSync(runner, runnerSource);
  const lines = new Array(cases.length);
  const failures = [];
  let next = 0;
  const worker = async () => {
    while (next < cases.length) {
      const i = next++;
      const files = cases[i];
      const dir = path.join(tmp, "case" + i);
      for (const [name, source] of Object.entries(files)) {
        const file = path.join(dir, name);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, source);
      }
      const { out, alternatives } = await measureStable(() => runOnce(runner, dir, tmp), RUNS);
      const clean = JSON.stringify(files);
      const columns = [clean, out];
      if (alternatives.length) columns.push(JSON.stringify(alternatives));
      if (columns.some((column) => /[\t\n\r]/.test(column))) throw new Error("tab ou quebra de linha no caso " + i);
      if (columns.some((column) => column.includes(tmp))) failures.push(i);
      lines[i] = columns.join("\t");
    }
  };
  await Promise.all(Array.from({ length: PARALLEL }, worker));
  fs.rmSync(tmp, { recursive: true, force: true });
  if (failures.length) throw new Error("caminho da máquina na saída dos casos " + failures.join(","));
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
  process.stderr.write(`casos: ${lines.length}, com alternativas: ${lines.filter((line) => line.split("\t").length > 2).length}\n`);
}

module.exports = { runModuleGolden };
