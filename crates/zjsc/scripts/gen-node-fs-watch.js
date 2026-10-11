// Gera `src/api/node_fs_watch.js` a partir de `src/js/internal/fs/watch.ts` e `watchfile.ts` do bun 1.4.2 (MIT): o
// `fs.watch`, o `fs.watchFile`, o `fs.unwatchFile` e (no epílogo, de `fs.promises.ts`) o `fs.promises.watch`. O que é
// nativo no bun (`internal/fs/binding`: `watch` e `watchFile`) vem do prelúdio (`node-fs-watch-prelude.js`), que
// observa por comparação de `stat` e `readdir` porque o sysabi não tem inotify. Mesmo método do `gen-node-fs-cp.js`:
// tira os tipos com `Bun.Transpiler`, troca os intrínsecos e embrulha cada arquivo numa função da fábrica `(host)`.
// Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-fs-watch.js <watch.ts> <watchfile.ts> > src/api/node_fs_watch.js`
const fs = require("node:fs");
const path = require("node:path");

const [watchInput, watchFileInput] = process.argv.slice(2);
if (!watchInput || !watchFileInput) {
  console.error("uso: bun gen-node-fs-watch.js <watch.ts> <watchfile.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
const replacements = [
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\$isArray\(/g, "Array.isArray("],
  [/\.\$get\(/g, ".get("],
  [/process\.binding\("uv"\)/g, "processBindingUv"],
  [/Bun\.fileURLToPath\(/g, 'host.requireBuiltin("node:url").fileURLToPath('],
  [/new Bun\.Glob\(/g, "globMatcher("],
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

const read = name => fs.readFileSync(path.join(__dirname, name), "utf8");
process.stdout.write(
  "// GERADO por `scripts/gen-node-fs-watch.js` a partir de `src/js/internal/fs/watch.ts` e `watchfile.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { codes, validators } = host.internal;\n" +
    "  const kInternalAssertionSuffix =\n" +
    '    "\\nThis is caused by either a bug in Node.js or incorrect usage of Node.js internals.\\n" +\n' +
    '    "Please open an issue with this stack trace at https://github.com/nodejs/node/issues\\n";\n' +
    read("node-fs-watch-prelude.js") +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/fs/binding") return binding;\n' +
    '    if (name === "internal/shared") return { kInternalAssertionSuffix };\n' +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    "  const watchModule = (function () {\n" +
    port(watchInput) +
    "\n  })();\n" +
    "  const watchFileModule = (function () {\n" +
    port(watchFileInput) +
    "\n  })();\n" +
    read("node-fs-watch-epilogue.js") +
    "})\n",
);
