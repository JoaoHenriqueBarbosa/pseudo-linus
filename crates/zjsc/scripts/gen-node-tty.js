// Gera `src/api/node_tty.js` a partir de `src/js/node/tty.ts` e `src/js/internal/tty.ts` do bun 1.4.2 (MIT): o `node:tty`
// (`isatty`, `ReadStream`, `WriteStream` e o `getColorDepth` do `internal/tty`). Mesmo método do `gen-node-fs-streams.js`:
// tira os tipos com `Bun.Transpiler`, troca os intrínsecos do JSC pelos equivalentes públicos e embrulha numa fábrica
// `(host)`. O `$cpp("ProcessBindingTTYWrap.cpp", "createBunTTYFunctions")` vira `host.tty` (`isatty`, `getWindowSize`,
// `setRawMode`, `rawModeStateSize`); os caminhos de Windows (`$bunNativePtr`, `$start`) ficam no texto e não rodam no
// Linux. Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-tty.js <tty.ts> <internal-tty.ts> > src/api/node_tty.js`
const fs = require("node:fs");

const [input, internalInput] = process.argv.slice(2);
if (!input || !internalInput) {
  console.error("uso: bun gen-node-tty.js <tty.ts> <internal-tty.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });
const convert = (path, replacements) => {
  let code = transpiler.transformSync(fs.readFileSync(path, "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  // `$fastPath` é o nome de uma opção interna (propriedade comum aqui); o resto não pode sobrar.
  const left = code.replace(/\$fastPath/g, "").match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`intrínseco sem tradução em ${path}: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  if (/^export /m.test(code)) {
    console.error(`export sem tradução em ${path}`);
    process.exit(1);
  }
  return code.replace(/^/gm, "    ").replace(/[ ]+$/gm, "");
};

const internalCode = convert(internalInput, [[/^export default \{ getColorDepth \};/m, "return { getColorDepth };"]]);
const ttyCode = convert(input, [
  [/\$cpp\("ProcessBindingTTYWrap\.cpp", "createBunTTYFunctions"\)/, "host.tty"],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$toClass\(/g, "toClass("],
  [/this\.\$bunNativePtr/g, "undefined"],
  [/this\.\$start\(\)/g, "undefined"],
  [/^export default \{ ReadStream, WriteStream, isatty \};/m, "return { ReadStream, WriteStream, isatty };"],
]);

process.stdout.write(
  "// GERADO por `scripts/gen-node-tty.js` a partir de `src/js/node/tty.ts` e `src/js/internal/tty.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    "  const { validators, toClass } = host.internal;\n" +
    "  const internalTty = (() => {\n" +
    internalCode +
    "\n  })();\n" +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/tty") return internalTty;\n' +
    '    if (name === "internal/fs/streams") return { get ReadStream() { return host.requireBuiltin("node:fs").ReadStream; }, get WriteStream() { return host.requireBuiltin("node:fs").WriteStream; } };\n' +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    "  return (() => {\n" +
    ttyCode +
    "\n  })();\n" +
    "})\n",
);
