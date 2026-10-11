// Gera `src/api/node_fs_utf8.js` a partir de `src/js/internal/streams/fast-utf8-stream.ts` do bun 1.4.2 (MIT, portado do
// Node e derivado do SonicBoom, MIT, Copyright (c) 2017 Matteo Collina): o `fs.Utf8Stream`. Mesmo método do
// `gen-node-fs-cp.js`: tira os tipos com `Bun.Transpiler`, troca os intrínsecos e embrulha numa fábrica `(host)` que
// devolve `{ Utf8Stream }`. Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-fs-utf8.js <fast-utf8-stream.ts> > src/api/node_fs_utf8.js`
const fs = require("node:fs");

const input = process.argv[2];
if (!input) {
  console.error("uso: bun gen-node-fs-utf8.js <fast-utf8-stream.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
let code = transpiler.transformSync(fs.readFileSync(input, "utf8"));
const replacements = [
  [/\$ERR_([A-Z_0-9a-z]+)\(/g, "codes.$1("],
  [/^export default Utf8Stream;/m, "return { Utf8Stream };"],
];
for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
if (left) {
  console.error(`intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
  process.exit(1);
}

process.stdout.write(
  "// GERADO por `scripts/gen-node-fs-utf8.js` a partir de `src/js/internal/streams/fast-utf8-stream.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { codes, validators } = host.internal;\n" +
    "  const require = name => (name === \"internal/validators\" ? validators : host.requireBuiltin(name));\n" +
    code.replace(/^/gm, "  ").replace(/[ ]+$/gm, "") +
    "\n})\n",
);
