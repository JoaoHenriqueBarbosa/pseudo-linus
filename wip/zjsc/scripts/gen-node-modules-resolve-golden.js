// Gera tests/golden/node_modules_resolve_bun.tsv: resolução de especificador nu por `node_modules` no bun 1.4.2.
// Monta uma árvore real numa pasta temporária, roda `require.resolve` e `import.meta.resolve` de dentro de cada
// importador e grava, por caso, o resultado ou a mensagem de erro. Todo caminho é relativo à raiz da árvore (o
// prefixo da pasta temporária é cortado, `/app/node_modules/a/index.js`, `file:///app/...`), nunca absoluto da máquina.
// A mesma árvore e os mesmos casos estão fixados nos testes de `src/api/module_probe.rs`.
// Colunas: importador, especificador (JSON), resultado do require.resolve, resultado do import.meta.resolve.
// Uso: bun scripts/gen-node-modules-resolve-golden.js > tests/golden/node_modules_resolve_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";

const tree = {
  "app/node_modules/a/package.json": '{"main":"lib/i.js"}',
  "app/node_modules/a/index.js": "1",
  "app/node_modules/a/sub.js": "1",
  "app/node_modules/@s/p/index.js": "1",
  "app/node_modules/@s/p/lib/x.js": "1",
  "app/node_modules/b/package.json": '{"main":"lib/i.js"}',
  "app/node_modules/b/lib/i.js": "1",
  "app/node_modules/b/lib/y.js": "1",
  "node_modules/up/index.js": "1",
  "node_modules/dirmain/package.json": '{"main":"lib/m"}',
  "node_modules/dirmain/lib/m.js": "1",
  "app/node_modules/withexp/package.json": '{"exports":"./e.js","main":"m.js"}',
  "app/node_modules/withexp/e.js": "1",
  "app/node_modules/withexp/m.js": "1",
  "app/node_modules/sd/lib/package.json": '{"main":"x.js"}',
  "app/node_modules/sd/lib/x.js": "1",
  "app/node_modules/sd/lib/index.js": "1",
  "app/node_modules/sd/dir/index.js": "1",
  "app/node_modules/sd/dir.js": "1",
  "node_modules/sd/lib/only_up.js": "1",
  "app/node_modules/empty/readme.txt": "1",
  "node_modules/empty/index.js": "1",
  "app/node_modules/inner/node_modules/deep/index.js": "1",
  "app/node_modules/inner/src/i.js": "1",
  "node_modules/deep.js": "1",
  "node_modules/f.js": "1",
  "app/node_modules/@t/q.js": "1",
  "app/node_modules/@t/q/index.js": "1",
  "app/node_modules/node_modules/nn/index.js": "1",
  "app/node_modules/pk/index.js": "1",
  "app/node_modules/pk/sub/index.js": "1",
  "app/node_modules/pk/node_modules/in/index.js": "1",
  "app/node_modules/file.json": "1",
  "app/node_modules/sp ace.js": "1",
  "real/index.js": "1",
  "real/lib/z.js": "1",
};
const links = { "app/node_modules/lnk": "../../real" };
const importers = ["app/src/t.js", "app/node_modules/inner/src/t.js", "app/node_modules/pk/sub/t.js"];

const fromApp = [
  "a", "a/sub", "a/sub.js", "a/nope", "a/", "a//sub", "a/../b", "@s/p", "@s/p/lib/x", "@s/p/lib/x.js", "@s", "@s/",
  "@s/nope", "@nope/x", "b", "b/lib/y", "up", "up/", "dirmain", "withexp", "nope", "empty", "sd/lib", "sd/lib/x",
  "sd/lib/", "sd/dir", "sd/dir/", "sd/lib/only_up", "f", "f.js", "f/x", "deep", "@t/q", "@t/q.js", "nn", "PK", "pk/.",
  "@s/p/zzz", "@s/p/lib/zzz", "a/nope/deeper", "nope/sub", "pk/sub/..", "pk\\sub", "pk/sub\\", "pk/sub/index", "pk/index.js", "file", "file.json", "sp ace", "lnk", "lnk/lib/z",
];
const cases = [];
for (const spec of fromApp) cases.push([importers[0], spec]);
for (const spec of ["deep", "inner", "sd", "empty", "f"]) cases.push([importers[1], spec]);
for (const spec of ["nn", "in", "pk"]) cases.push([importers[2], spec]);

const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "nmr-")));
for (const [rel, text] of Object.entries(tree)) {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), text);
}
for (const [rel, target] of Object.entries(links)) fs.symlinkSync(target, path.join(root, rel));
const probe = `
const s = process.argv[2];
let r, m;
try { r = require.resolve(s); } catch (e) { r = "ERR " + e.message; }
try { m = import.meta.resolve(s); } catch (e) { m = "ERR " + e.message; }
console.log(JSON.stringify([r, m]));
`;
for (const rel of importers) {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), probe);
}

const clean = (text) => text.split(root).join("").replace(/\n/g, "\\n");
const lines = [];
for (const [importer, spec] of cases) {
  const run = spawnSync(BUN, [path.join(root, importer), spec], { cwd: root, encoding: "utf8" });
  const [r, m] = JSON.parse(run.stdout.trim().split("\n").pop());
  lines.push([importer, JSON.stringify(spec), clean(r), clean(m)].join("\t"));
}
fs.rmSync(root, { recursive: true, force: true });
// A árvore usada, para o teste Rust montar o `MemoryFs`: uma entrada por linha, `["caminho","conteúdo"]` em `files` e
// `["link","alvo relativo"]` em `links`. Os importadores não entram (o conteúdo deles é só a sonda do bun).
const treePath = path.join(__dirname, "..", "tests", "golden", "node_modules_resolve_tree.json");
const rows = (entries) => entries.map((entry) => "  " + JSON.stringify(entry)).join(",\n");
fs.writeFileSync(treePath, `{\n"files": [\n${rows(Object.entries(tree))}\n],\n"links": [\n${rows(Object.entries(links))}\n]\n}\n`);
process.stdout.write(lines.join("\n") + "\n");
