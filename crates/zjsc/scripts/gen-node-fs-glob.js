// Gera `src/api/node_fs_glob.js` a partir de `src/js/internal/fs/glob.ts` do bun 1.4.2 (MIT, com o minimatch do Node
// embutido sem alteração, ISC): o `fs.glob`, o `fs.globSync` e o `fs.promises.glob`. Mesmo método do
// `gen-node-fs-cp.js`: tira os tipos com `Bun.Transpiler`, troca os intrínsecos e embrulha numa fábrica `(host)`; o
// epílogo (`node-fs-glob-epilogue.js`) traz os despachantes de `fs.ts` e `fs.promises.ts`. Se sobrar um `$nome` sem
// tradução o script falha.
//
// Uso: `bun scripts/gen-node-fs-glob.js <glob.ts> > src/api/node_fs_glob.js`
const fs = require("node:fs");
const path = require("node:path");

const input = process.argv[2];
if (!input) {
  console.error("uso: bun gen-node-fs-glob.js <glob.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
let code = transpiler.transformSync(fs.readFileSync(input, "utf8"));
const replacements = [
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$isPromise\(/g, "isPromise("],
  [/\$isArray\(/g, "Array.isArray("],
  [/^export default \{/m, "const globModule = {"],
];
for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
if (left) {
  console.error(`intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
  process.exit(1);
}

process.stdout.write(
  "// GERADO por `scripts/gen-node-fs-glob.js` a partir de `src/js/internal/fs/glob.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { codes, validators } = host.internal;\n" +
    "  const isPromise = value => value instanceof Promise;\n" +
    "  const emptyObject = Object.freeze({ __proto__: null });\n" +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/shared") return { kEmptyObject: emptyObject };\n' +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    code.replace(/^/gm, "  ").replace(/[ ]+$/gm, "") +
    "\n" +
    fs.readFileSync(path.join(__dirname, "node-fs-glob-epilogue.js"), "utf8") +
    "})\n",
);
