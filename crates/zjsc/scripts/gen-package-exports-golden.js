// Gera tests/golden/package_exports_bun.tsv: o campo `exports` do package.json resolvido pelo bun 1.4.2.
// Para cada caso monta numa pasta temporária um pacote `node_modules/pN` com o `exports` do caso e um conjunto
// fixo de arquivos, e roda `require.resolve` (CJS) e `import.meta.resolve` (ESM) de dentro de um importador.
// Todo caminho gravado é relativo à raiz da árvore (o prefixo da pasta temporária é cortado, tanto no resultado
// quanto nas mensagens de erro), nunca absoluto da máquina. Os mesmos casos estão fixados nos testes de
// `src/api/package_exports.rs` (`FILES` lá é o mesmo conjunto de arquivos daqui).
// Colunas: exports (JSON), subcaminho pedido ("." ou "./x"), resultado do require.resolve, resultado do
// import.meta.resolve. Resultado: caminho relativo, ou `ERR code: mensagem`.
// Uso: bun scripts/gen-package-exports-golden.js > tests/golden/package_exports_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";

const FILES = [
  "index.js", "main.js", "a.js", "b.js", "c.js", "d.js", "e.js", "i.mjs", "r.cjs", "bun.js", "node.js", "def.js",
  "types.js", "src/x.js", "src/y/z.js", "lib/x.js", "lib/y/z.js", "lib/x/g.js", "lib/y/z/g.js", "lib/x.js.js",
  "lib/q/index.js", "package.json.js",
];

const E = (o) => JSON.stringify(o);
// [exports, [subcaminhos]]
const cases = [
  ['"./e.js"', [".", "./x", "./e.js", "./package.json"]],
  ['"e.js"', ["."]],
  ['"../e.js"', ["."]],
  ['"./nope.js"', ["."]],
  ['"./node_modules/x.js"', ["."]],
  ['"./lib/../e.js"', ["."]],
  ['"./e.js/"', ["."]],
  [E({ ".": "./a.js", "./b": "./b.js", "./c.js": "./c.js" }), [".", "./b", "./c.js", "./c", "./nope", "./package.json", "./b/", "./"]],
  [E({ ".": "./a.js", "./b": null }), [".", "./b", "./package.json"]],
  [E({ ".": null }), ["."]],
  [E({ ".": "./a.js", "./lib/": "./lib/" }), ["./lib/x.js", "./lib/y/z.js", "./lib/", "./lib/nope.js"]],
  [E({ import: "./i.mjs", require: "./r.cjs" }), [".", "./x"]],
  [E({ ".": { import: "./i.mjs", require: "./r.cjs" } }), ["."]],
  [E({ ".": { require: "./r.cjs", import: "./i.mjs" } }), ["."]],
  [E({ ".": { default: "./def.js", import: "./i.mjs", require: "./r.cjs" } }), ["."]],
  [E({ ".": { import: "./i.mjs", default: "./def.js" } }), ["."]],
  [E({ ".": { require: "./r.cjs", default: "./def.js" } }), ["."]],
  [E({ ".": { bun: "./bun.js", node: "./node.js", default: "./def.js" } }), ["."]],
  [E({ ".": { node: "./node.js", bun: "./bun.js", default: "./def.js" } }), ["."]],
  [E({ ".": { bun: "./bun.js", import: "./i.mjs", require: "./r.cjs" } }), ["."]],
  [E({ ".": { import: "./i.mjs", bun: "./bun.js", require: "./r.cjs" } }), ["."]],
  [E({ ".": { node: "./node.js", import: "./i.mjs" } }), ["."]],
  [E({ ".": { types: "./types.js", import: "./i.mjs", require: "./r.cjs" } }), ["."]],
  [E({ ".": { browser: "./a.js", default: "./def.js" } }), ["."]],
  [E({ ".": { module: "./a.js", default: "./def.js" } }), ["."]],
  [E({ ".": { worker: "./a.js", deno: "./b.js", default: "./def.js" } }), ["."]],
  [E({ ".": { development: "./a.js", production: "./b.js", default: "./def.js" } }), ["."]],
  [E({ ".": { "react-native": "./a.js", default: "./def.js" } }), ["."]],
  [E({ ".": { "module-sync": "./a.js", default: "./def.js" } }), ["."]],
  [E({ ".": { import: null, default: "./def.js" } }), ["."]],
  [E({ ".": { import: { node: "./node.js", default: "./i.mjs" }, require: "./r.cjs" } }), ["."]],
  [E({ ".": { import: { nope: "./node.js" }, default: "./def.js" } }), ["."]],
  [E({ ".": { nope: "./node.js" } }), ["."]],
  [E({ ".": { import: "./nope.js", default: "./def.js" } }), ["."]],
  [E({ ".": { import: "i.mjs", default: "./def.js" } }), ["."]],
  [E({ ".": ["./nope.js", "./a.js"] }), ["."]],
  [E({ ".": ["i.mjs", "./a.js"] }), ["."]],
  [E({ ".": [{ nope: "./b.js" }, "./a.js"] }), ["."]],
  [E({ ".": [] }), ["."]],
  [E(["./a.js", "./b.js"]), [".", "./x"]],
  [E({ "./f/*": "./lib/*.js" }), ["./f/x", "./f/y/z", "./f/", "./f", "./f/x.js", "./f/nope", "./f/y/z/g"]],
  [E({ "./f/*.js": "./lib/*.js" }), ["./f/x.js", "./f/x", "./f/y/z.js"]],
  [E({ "./*": "./src/*.js" }), ["./x", "./y/z", "./x.js", "./package.json", "./", "."]],
  [E({ "./*": "./src/*" }), ["./x.js", "./y/z.js", "./package.json"]],
  [E({ "./f/*": "./lib/*", "./f/x/*": "./lib/y/*" }), ["./f/x.js", "./f/x/g.js", "./f/y/z.js"]],
  [E({ "./f/*": "./lib/*", "./f/y/*": null }), ["./f/x.js", "./f/y/z.js"]],
  [E({ "./f/*": "./lib/*", "./f/x.js": null }), ["./f/x.js", "./f/y/z.js"]],
  [E({ "./f/*": "./lib/*", "./f/x.js": "./a.js" }), ["./f/x.js", "./f/y/z.js"]],
  [E({ "./f/*": null }), ["./f/x.js"]],
  [E({ "./f/*/g.js": "./lib/*/g.js" }), ["./f/x/g.js", "./f/y/z/g.js", "./f/x/h.js"]],
  [E({ "./f/*": "./lib/x.js" }), ["./f/anything"]],
  [E({ "./f/*": "./lib/*/*.js" }), ["./f/x"]],
  [E({ "./f/**": "./lib/**" }), ["./f/x.js"]],
  [E({ "./f*": "./lib/*" }), ["./fx.js", "./f/x.js"]],
  [E({ "./f/*": { import: "./lib/*.js", default: "./a.js" } }), ["./f/x", "./f/y/z"]],
  [E({ "./f/*": ["./lib/nope", "./lib/*.js"] }), ["./f/x"]],
  [E({ "./f/*": "./lib/*.js", "./f/x": "./a.js" }), ["./f/x", "./f/y/z"]],
  [E({ ".": "./a.js", "import": "./i.mjs" }), [".", "./x"]],
  [E({ "./a": "./a.js" }), [".", "./a", "./a/", "./a.js"]],
  [E({ "./a/": "./lib/" }), ["./a/x.js", "./a/y/z.js", "./a/"]],
  [E({ "./a/*": "../x" }), ["./a/b"]],
  [E({ "./a": "./a.js", "./a": "./b.js" }), ["./a"]],
  [E({}), [".", "./a"]],
  [E(null), ["."]],
  [E(0), ["."]],
  [E(true), ["."]],
];

function rel(root, s) {
  return String(s).split("file://" + root + "/").join("").split(root + "/").join("").split(root).join("");
}

const root = fs.mkdtempSync(path.join(os.tmpdir(), "pkgexp-"));
const lines = [];
try {
  cases.forEach(([exportsJson], i) => {
    const dir = path.join(root, "node_modules", "p" + i);
    for (const f of FILES) {
      fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
      fs.writeFileSync(path.join(dir, f), "1");
    }
    fs.writeFileSync(path.join(dir, "package.json"), '{"main":"main.js","exports":' + exportsJson + "}");
  });
  fs.writeFileSync(path.join(root, "t.cjs"), "");
  fs.writeFileSync(path.join(root, "t.mjs"), "");
  const specs = [];
  cases.forEach(([exportsJson, subs], i) => {
    for (const sub of subs) specs.push({ exportsJson, sub, spec: sub === "." ? "p" + i : "p" + i + sub.slice(1) });
  });
  const probe = (file, body) => {
    fs.writeFileSync(path.join(root, file), body);
    const r = spawnSync(BUN, [path.join(root, file), JSON.stringify(specs.map((s) => s.spec))], { cwd: root, encoding: "utf8" });
    return JSON.parse(r.stdout);
  };
  const req = probe("t.cjs", `const out = [];
for (const s of JSON.parse(process.argv[2])) {
  try { out.push(require.resolve(s)); } catch (e) { out.push("ERR " + e.code + ": " + String(e.message).split("\\n")[0]); }
}
console.log(JSON.stringify(out));`);
  const imp = probe("t.mjs", `const out = [];
for (const s of JSON.parse(process.argv[2])) {
  try { out.push(import.meta.resolve(s)); } catch (e) { out.push("ERR " + e.code + ": " + String(e.message).split("\\n")[0]); }
}
console.log(JSON.stringify(out));`);
  specs.forEach((s, k) => {
    const norm = (v) => rel(root, v).replace(/\bp\d+\b/g, "pkg");
    lines.push([s.exportsJson, s.sub, norm(req[k]), norm(imp[k])].map((c) => JSON.stringify(c)).join("\t"));
  });
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
process.stdout.write(lines.join("\n") + "\n");
