// Gera tests/golden/cjs_import_bun.tsv: grafos ES que importam arquivos CommonJS, medidos no bun (interop de `__esModule`,
// chaves dinâmicas de `module.exports`, ordem de carga, cache dividido com `require`, vários CommonJS, erro na carga).
// Cada caso vira um diretório com os arquivos; o ponto de entrada é main.mjs, que usa o global `L` (empurra String(x) em
// `log`). A ordem entre CommonJS irmãos não é fixa no bun (a leitura corre num pool de threads), então os casos só
// registram o que é determinístico: o CommonJS roda antes dos corpos ES e o `require` aninhado roda dentro do pai.
// Colunas: nome, mapa de arquivos em JSON, saída `{"log":[...],"error":...}`.
// Uso (no oráculo): bun scripts/gen-cjs-import-golden.js > tests/golden/cjs_import_bun.tsv
const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const cases = [];
const add = (name, files) => cases.push([name, files]);

const interop = {
  esm_flag_with_default: "exports.__esModule = true; exports.default = 'D'; exports.x = 1;",
  esm_flag_without_default: "exports.__esModule = true; exports.x = 1;",
  esm_flag_object: "module.exports = { __esModule: true, default: 'D3', y: 2 };",
  plain_object: "module.exports = { a: 1, b: 2 };",
  hidden_flag: "Object.defineProperty(exports, '__esModule', { value: true }); exports.default = 'D5'; exports.z = 3;",
  function_with_props: "module.exports = function f() {}; module.exports.k = 1;",
  primitive: "module.exports = 5;",
  default_key_without_flag: "exports.default = 'D8'; exports.q = 1;",
};
for (const [name, text] of Object.entries(interop)) {
  add("interop_" + name, {
    "c.cjs": text,
    "main.mjs": "const ns = await import('./c.cjs'); L(JSON.stringify(Object.keys(ns))); L(typeof ns.default === 'object' ? JSON.stringify(ns.default) : String(ns.default));",
  });
}

add("load_order", {
  "a.mjs": "L('a eval');",
  "c.cjs": "module.exports = { a: 1, b: 2 }; exports.zzz = 9;",
  "d.cjs": "L('d eval'); const c = require('./e.cjs'); module.exports = { d: c.e + 1, me: __filename.split('/').pop(), req: typeof require };",
  "e.cjs": "L('e eval'); exports.e = 10;",
  "n.mjs": "L('n eval'); export const n = 1;",
  "main.mjs": "import './a.mjs';\nimport { a, b } from './c.cjs';\nimport dd, { d, me, req } from './d.cjs';\nimport './n.mjs';\nL(['main', a, b, d, me, req, JSON.stringify(dd)].join(' '));\nconst x = await import('./e.cjs');\nL(JSON.stringify(x) + ' ' + Object.keys(x));",
});

add("two_commonjs_share_a_third", {
  "shared.cjs": "exports.v = 10; L('shared');",
  "p.cjs": "exports.x = require('./shared.cjs').v + 1;",
  "q.cjs": "exports.y = require('./shared.cjs').v + 2;",
  "main.mjs": "import { x } from './p.cjs'; import { y } from './q.cjs'; import r from './p.cjs'; L(x + y); L(r.x);",
});

add("import_and_require_share_the_cache", {
  "e.cjs": "L('e eval'); exports.e = { n: 1 };",
  "r.cjs": "module.exports = require('./e.cjs');",
  "main.mjs": "import d from './e.cjs'; import r from './r.cjs'; const again = await import('./e.cjs'); L(d === again.default); L(r === d);",
});

add("static_import_of_three_commonjs", {
  "a.cjs": "exports.v = 'a';",
  "b.cjs": "exports.v = 'b';",
  "c.cjs": "exports.v = 'c';",
  "main.mjs": "import { v as a } from './a.cjs'; import { v as b } from './b.cjs'; import { v as c } from './c.cjs'; L(a + b + c);",
});

add("throw_on_load", {
  "t.cjs": "L('t eval'); throw new Error('boom');",
  "main.mjs": "try { await import('./t.cjs'); } catch (e) { L(e.name + ': ' + e.message); } try { await import('./t.cjs'); } catch (e) { L(e.name + ': ' + e.message); } L('fim');",
});

const RUNNER = `
globalThis.log = [];
globalThis.L = (x) => { log.push(String(x)); };
const [dir] = process.argv.slice(2);
let error = null;
try { await import(dir + "/main.mjs"); } catch (e) {
  error = e instanceof Error ? e.name + ": " + e.message : "throw " + String(e);
}
await new Promise((r) => setTimeout(r, 0));
const clean = (s) => s.split("file://" + dir + "/").join("file:///").split(dir + "/").join("");
console.log(clean(JSON.stringify({ log, error })));
`;

const base = fs.mkdtempSync(path.join(require("os").tmpdir(), "cjs-import-"));
fs.writeFileSync(path.join(base, "runner.mjs"), RUNNER);
for (const [name, files] of cases) {
  const dir = path.join(base, name);
  fs.mkdirSync(dir);
  for (const [file, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, file), text);
  const run = spawnSync(process.execPath, [path.join(base, "runner.mjs"), dir], { encoding: "utf8", timeout: 20000 });
  const out = (run.stdout || "").trim().split("\n").pop();
  process.stdout.write(name + "\t" + JSON.stringify(files) + "\t" + out + "\n");
}
fs.rmSync(base, { recursive: true, force: true });
