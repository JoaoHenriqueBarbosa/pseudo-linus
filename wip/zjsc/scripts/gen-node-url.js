// Gera `src/api/node_url.js` a partir do `src/js/node/url.ts` e do `src/js/internal/url.ts` do bun 1.4.2 (MIT).
//
// Uso: `bun scripts/gen-node-url.js <url.ts> <internal/url.ts> > src/api/node_url.js`.
//
// Tira os tipos do TypeScript (`Bun.Transpiler`, que também expande o `const enum Char`), troca os intrínsecos do JSC
// pelos equivalentes do `host` e embrulha tudo na fábrica `(function (host) { ... })`. Se sobrar um `$nome` sem
// tradução o script falha, em vez de deixar um intrínseco solto no módulo.
const fs = require("node:fs");

const [urlFile, internalUrlFile] = process.argv.slice(2);
if (!urlFile || !internalUrlFile) {
  console.error("uso: bun gen-node-url.js <url.ts> <internal/url.ts>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts", minifyWhitespace: false, inline: true });

const replacements = [
  [/\$cpp\("NodeURL\.cpp", "Bun::createNodeURLBinding"\)/g, "[host.domainToAscii, host.domainToUnicode, host.idnaToASCII]"],
  [/require\("internal\/url"\)/g, "internalUrl"],
  [/require\("internal\/validators"\)/g, "host.internal.validators"],
  [/require\("internal\/shared"\)/g, "host.internal"],
  [/require\("node:path"\)/g, 'host.requireBuiltin("node:path")'],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$charCodeAt\(/g, ".charCodeAt("],
  [/\$isObject\(/g, "isObject("],
  [/\$putByIdDirect\(/g, "putByIdDirect("],
  [/\$toLength\(/g, "toLength("],
  [/^export default /m, "return "],
];

function convert(file) {
  let code = transpiler.transformSync(fs.readFileSync(file, "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`${file}: intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
}

// O `internal/url` num escopo próprio, como módulo à parte que é no bun: o `url.ts` declara de novo `urlToHttpOptions`.
const internalUrl = "const internalUrl = (() => {\n" + convert(internalUrlFile).replace(/^/gm, "  ").replace(/[ ]+$/gm, "") + "\n})();";

let out = "// GERADO por `scripts/gen-node-url.js` a partir do `url.ts` e do `internal/url.ts` do bun 1.4.2. Não editar à mão.\n";
out += "(function (host) {\n";
out += '  "use strict";\n\n';
out += "  const { codes } = host.internal;\n";
out += "  // `$isObject`: objeto ou função.\n";
out += '  const isObject = value => (typeof value === "object" && value !== null) || typeof value === "function";\n';
out += "  // `$putByIdDirect`: propriedade própria comum, sem passar por setter.\n";
out += "  const putByIdDirect = (object, key, value) =>\n";
out += "    Object.defineProperty(object, key, { value, writable: true, enumerable: true, configurable: true });\n";
out += "  // `$toLength` do JSC (ToLength da especificação).\n";
out += "  const toLength = value => {\n";
out += "    const length = Math.trunc(Number(value)) || 0;\n";
out += "    return length <= 0 ? 0 : Math.min(length, Number.MAX_SAFE_INTEGER);\n";
out += "  };\n";
out += internalUrl.replace(/^/gm, "  ").replace(/[ ]+$/gm, "");
out += "\n";
out += convert(urlFile).replace(/^/gm, "  ").replace(/[ ]+$/gm, "");
out += "\n})\n";
process.stdout.write(out);
