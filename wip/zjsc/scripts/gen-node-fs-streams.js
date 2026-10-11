// Gera `src/api/node_fs_streams.js` a partir de `src/js/internal/fs/streams.ts` do bun 1.4.2 (MIT): o `fs.ReadStream` e o
// `fs.WriteStream`. Mesmo método do `gen-node-stream.js`: tira os tipos com `Bun.Transpiler`, troca os intrínsecos do JSC
// pelos equivalentes públicos e embrulha o corpo numa fábrica `(host)` (o `require` interno resolve os validadores e o
// `destroy` do pacote de `stream`; `host.fsData()` é o `fs.promises.$data` do bun). Se sobrar um `$nome` sem tradução o
// script falha.
//
// Uso: `bun scripts/gen-node-fs-streams.js <streams.ts> > src/api/node_fs_streams.js`
const fs = require("node:fs");

const input = process.argv[2];
if (!input) {
  console.error("uso: bun gen-node-fs-streams.js <streams.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
let code = transpiler.transformSync(fs.readFileSync(input, "utf8"));

const replacements = [
  [/\(fs\.promises as any\)\.\$data/g, "host.fsData()"],
  [/\bfs\.promises\.\$data\b/g, "host.fsData()"],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$assert\(/g, "assert("],
  [/\$isPromise\(/g, "isPromise("],
  [/\$min\(/g, "Math.min("],
  [/\$toClass\(/g, "toClass("],
  [/^export default \{/m, "return {"],
];
for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
// `$fastPath` é o nome de uma opção interna (propriedade comum aqui); o resto não pode sobrar.
const left = code.replace(/\$fastPath/g, "").match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
if (left) {
  console.error(`intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
  process.exit(1);
}

process.stdout.write(
  "// GERADO por `scripts/gen-node-fs-streams.js` a partir de `src/js/internal/fs/streams.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { codes, validators, toClass } = host.internal;\n" +
    "  const assert = () => {};\n" +
    "  const isPromise = value => value instanceof Promise;\n" +
    "  const require = name => (name === \"internal/validators\" ? validators : name.startsWith(\"internal/\") ? host.requireInternal(name) : host.requireBuiltin(name));\n" +
    code.replace(/^/gm, "  ").replace(/[ ]+$/gm, "") +
    "\n})\n",
);
