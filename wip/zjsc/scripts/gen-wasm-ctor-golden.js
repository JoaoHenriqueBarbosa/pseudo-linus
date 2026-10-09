// Gera tests/golden/wasm_ctor_bun.tsv: programas dos construtores do WebAssembly JS (Memory com `address`/`shared`/
// `grow`, Table, Global, comprimentos dos construtores) avaliados no bun. Complementa scripts/gen-wasm-api-golden.js,
// do qual herda o formato e o harness: cada programa registra eventos no array global `log` e usa os auxiliares
// (L, T, ...) de tests/golden/wasm_api_bun_harness.js. Colunas: fonte, depois o JSON do log depois de esvaziar as
// microtarefas, ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona. Cada programa roda num
// processo bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-ctor-golden.js > tests/golden/wasm_ctor_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_api_bun_harness.js"), "utf8");

const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

// Memory: opção `address` (i32 por padrão, i64 exige Memory64) e `shared`.
add(
  `L(T(() => new WebAssembly.Memory({initial: 1, address: 'x'})))`,
  `L(T(() => new WebAssembly.Memory({initial: 1, address: 'i64'})))`,
  `L(T(() => new WebAssembly.Memory({initial: 1n, maximum: 2n, address: 'i64'})))`,
  `L(T(() => new WebAssembly.Memory({initial: 1, address: 'i32'}).buffer.byteLength))`,
  `L(T(() => new WebAssembly.Memory({initial: 1, address: undefined}).buffer.byteLength))`,
  `L(T(() => new WebAssembly.Memory({initial: 1, address: 5})))`,
  `L(T(() => new WebAssembly.Memory({initial: 1, shared: true})))`,
  `var m = new WebAssembly.Memory({initial: 1, maximum: 3, shared: true}); L(Object.prototype.toString.call(m.buffer)); L(m.buffer.byteLength); L(m.grow(1)); L(m.buffer.byteLength)`
);
// Memory.grow: buffer antigo é destacado, limite declarado, receptor inválido.
add(
  `var m = new WebAssembly.Memory({initial: 1}); var b = m.buffer; L(m.grow(0)); L(b.byteLength); L(b === m.buffer); L(m.buffer.byteLength)`,
  `var m = new WebAssembly.Memory({initial: 1, maximum: 2}); L(T(() => m.grow(2))); L(m.grow(1)); L(m.buffer.byteLength)`,
  `L(T(() => WebAssembly.Memory.prototype.grow.call({}, 1)))`
);
// Table e Global.
add(
  `L(T(() => new WebAssembly.Table({element: 'externref', initial: 1, maximum: 1}).grow(1)))`,
  `L(T(() => WebAssembly.Table.prototype.length))`,
  `L(T(() => new WebAssembly.Table({element: 'externref', initial: 1}, 'x').get(0)))`,
  `L(T(() => new WebAssembly.Global({value: 'i64'}, 1)))`,
  `var g = new WebAssembly.Global({value: 'i32'}); L(g.value); L(g.valueOf()); L(T(() => { g.value = 1; }))`,
  `L(WebAssembly.Memory.length); L(WebAssembly.Table.length); L(WebAssembly.Global.length)`
);

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-ctor-golden-"));
const lines = [];
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
const kept = programs.filter((p) => !usesHostApi(p));
kept.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 50);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${kept.length} programas\n`);
