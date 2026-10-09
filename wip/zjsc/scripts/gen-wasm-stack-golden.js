// Gera tests/golden/wasm_stack_bun.expected: o `e.stack` que atravessa WebAssembly, medido no bun 1.4.2 rodando
// tests/golden/wasm_stack_bun.js (o fonte é escrito à mão e versionado; só o esperado é gerado).
// O bun mostra o caminho absoluto do arquivo nos frames; o golden guarda só o nome do arquivo, então o diretório
// do fonte é removido do texto. Caminho da máquina que sobrar na saída derruba a geração.
// Uso: bun scripts/gen-wasm-stack-golden.js   (GOLDEN_OUT_DIR desvia a escrita, para conferência em /tmp)
const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const goldenDir = path.join(__dirname, "..", "tests", "golden");
const source = path.join(goldenDir, "wasm_stack_bun.js");
const outDir = process.env.GOLDEN_OUT_DIR || goldenDir;

const run = spawnSync(process.execPath, [source], { encoding: "utf8" });
if (run.status !== 0) throw new Error("bun saiu com " + run.status + ": " + run.stderr);

const text = run.stdout.split(goldenDir + path.sep).join("");
if (/\/home\/|\/tmp\/|\/Users\//.test(text)) throw new Error("caminho da máquina no resultado");
fs.writeFileSync(path.join(outDir, "wasm_stack_bun.expected"), text);
