// Gera `src/api/node_stream_modules.js` a partir dos fontes do `stream` do bun 1.4.2 (`src/js/internal/streams/*.ts`,
// `src/js/internal/stream.ts`, `src/js/internal/stream.promises.ts` e `src/js/node/stream.ts`, MIT).
//
// Uso: `bun scripts/gen-node-stream.js <diretório com os fontes> > src/api/node_stream_modules.js`, onde o diretório tem
// `stream.ts` (node/stream.ts), `internal/stream.ts`, `internal/stream.promises.ts` e `internal/streams/*.ts`, baixados
// de `https://raw.githubusercontent.com/oven-sh/bun/bun-v1.4.2/src/js/...`.
//
// O que o script faz com cada arquivo: tira os tipos do TypeScript (`Bun.Transpiler`), troca os intrínsecos do JSC
// pelos equivalentes públicos (o `prelude` abaixo define os auxiliares) e embrulha o corpo numa função
// `(module, exports, require)`, no lugar do `export default`. Se sobrar um `$nome` sem tradução o script falha, em vez de
// deixar um intrínseco solto no módulo.
const fs = require("node:fs");
const path = require("node:path");

const root = process.argv[2];
if (!root) {
  console.error("uso: bun gen-node-stream.js <diretório dos fontes>");
  process.exit(2);
}

const streamsDir = path.join(root, "internal", "streams");
const modules = [
  ["node:stream", path.join(root, "stream.ts")],
  ["internal/stream", path.join(root, "internal", "stream.ts")],
  ["internal/stream.promises", path.join(root, "internal", "stream.promises.ts")],
  ...fs
    .readdirSync(streamsDir)
    .filter(name => name.endsWith(".ts"))
    .sort()
    .map(name => ["internal/streams/" + name.slice(0, -3), path.join(streamsDir, name)]),
];

const transpiler = new Bun.Transpiler({ loader: "ts" });

const replacements = [
  [/\$cpp\("NodeModuleModule\.cpp", "createStreamIterEnabledFlag"\)/g, "false"],
  [/\$getInternalField\(\$asyncContext, 0\)/g, "undefined"],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\.\$then\b/g, ".then"],
  [/\.\$(resolve|reject)\b/g, ".$1"],
  [/\$Promise\b/g, "Promise"],
  [/new \$Buffer\(/g, "new (getBuffer())("],
  [/\$newArrayWithSize\(/g, "new Array("],
  [/\$(debug|makeAbortError|toClass|isCallable|assert|webStreamControllerError|webStreamClosedPromise)\(/g, "$1("],
  [/\$inherits(ReadableStream|WritableStream|TransformStream|Blob)\(/g, "inherits$1("],
  [/^export default /m, "module.exports = "],
];

function convert(name, file) {
  let code = transpiler.transformSync(fs.readFileSync(file, "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`${name}: intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
}

const prelude = fs.readFileSync(path.join(__dirname, "node-stream-prelude.js"), "utf8");

let out = "// GERADO por `scripts/gen-node-stream.js` a partir dos fontes do stream do bun 1.4.2. Não editar à mão.\n";
out += prelude;
out += "  const definitions = {\n";
for (const [name, file] of modules) {
  out += `    ${JSON.stringify(name)}: function (module, _unusedExports, require) {\n      "use strict";\n`;
  out += convert(name, file).replace(/^/gm, "      ").replace(/[ ]+$/gm, "");
  out += "\n    },\n";
}
out += "  };\n";
out += fs.readFileSync(path.join(__dirname, "node-stream-epilogue.js"), "utf8");
process.stdout.write(out);
