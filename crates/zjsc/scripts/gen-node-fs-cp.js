// Gera `src/api/node_fs_cp.js` a partir de `src/js/internal/fs/cp-sync.ts` e `src/js/internal/fs/cp.ts` do bun 1.4.2 (MIT):
// o `fs.cpSync`, o `fs.cp` e o `fs.promises.cp`. Mesmo método do `gen-node-fs-streams.js`: tira os tipos com
// `Bun.Transpiler`, troca os intrínsecos do JSC pelos equivalentes públicos e embrulha cada arquivo numa função própria
// dentro de uma fábrica `(host)`. O epílogo (`node-fs-cp-epilogue.js`) traz os despachantes de `fs.ts` e
// `fs.promises.ts` (validação e escolha do caminho), sem o caminho nativo do bun. Se sobrar um `$nome` sem tradução o
// script falha.
//
// Uso: `bun scripts/gen-node-fs-cp.js <cp-sync.ts> <cp.ts> > src/api/node_fs_cp.js`
const fs = require("node:fs");
const path = require("node:path");

const [syncInput, asyncInput] = process.argv.slice(2);
if (!syncInput || !asyncInput) {
  console.error("uso: bun gen-node-fs-cp.js <cp-sync.ts> <cp.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
const replacements = [
  [/\$processBindingConstants\.os\.errno/g, 'host.requireBuiltin("node:os").constants.errno'],
  [/\$Promise\.prototype\.\$then/g, "Promise.prototype.then"],
  [/Promise\.\$reject/g, "Promise.reject"],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$isPromise\(/g, "isPromise("],
  [/^export default \{/m, "return {"],
];

function port(file) {
  let code = transpiler.transformSync(fs.readFileSync(file, "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`${file}: intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code.replace(/^/gm, "    ").replace(/[ ]+$/gm, "");
}

process.stdout.write(
  "// GERADO por `scripts/gen-node-fs-cp.js` a partir de `src/js/internal/fs/cp-sync.ts` e `cp.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { codes, validators } = host.internal;\n" +
    "  const isPromise = value => value instanceof Promise;\n" +
    "  let cpSyncModule;\n" +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/fs/cp-sync") return cpSyncModule;\n' +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    "  cpSyncModule = (function () {\n" +
    port(syncInput) +
    "\n  })();\n" +
    "  const cpModule = (function () {\n" +
    port(asyncInput) +
    "\n  })();\n" +
    fs.readFileSync(path.join(__dirname, "node-fs-cp-epilogue.js"), "utf8") +
    "})\n",
);
