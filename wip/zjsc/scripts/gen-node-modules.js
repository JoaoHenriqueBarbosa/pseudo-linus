// Gera os módulos do bun 1.4.2 (MIT, `src/js/node/`) que não têm nativo próprio além do que o registro `node_internals.js` já entrega:
// `node:diagnostics_channel`, `node:http`, `node:https` e os `node:_http_*`. Cada um sai como um arquivo `src/api/node_<nome>.js`
// com a fábrica `(host)`. Os `internal/*` e `native/*` vêm de `host.internalModule`; se sobrar um `$nome` sem tradução o script
// falha. O servidor (`Bun.serve` com `onNodeHTTPRequest`) e o analisador HTTP nativo ainda não existem e lançam "not implemented"
// no ponto de uso.
//
// Uso: `bun scripts/gen-node-modules.js <dir src/js do bun> <dir de saída>`
const fs = require("node:fs");
const path = require("node:path");
const { convert, indent, common, commonPrologue, exportDefault } = require("./bun-gen-common.js");

const [root, outDir] = process.argv.slice(2);
if (!root || !outDir) {
  console.error("uso: bun gen-node-modules.js <src/js do bun> <dir de saída>");
  process.exit(2);
}

const asyncContext = [
  [/\$getInternalField\(\$asyncContext, 0\)/g, "host.getAsyncContext()"],
  [/\$putInternalField\(\$asyncContext, 0, (\w+)\)/g, "host.setAsyncContext($1)"],
];

// Cada `$nome` do bun que sobra depois das traduções comuns tem a sua aqui, com o módulo que o usa.
const httpCommon = [
  [/process\.binding\("http_parser"\)/g, 'require("native/http_parser")'],
  [/\$newRustFunction\("node_http_binding\.rs", "getInsecureHTTPParser", 0\)/, "(() => false)"],
  [/\$newRustFunction\("node_http_binding\.rs", "getBunServerAllClosedPromise", 1\)/, 'notImplemented("http.Server close")'],
  [/\$newRustFunction\("node_http_binding\.rs", "newDetachedSocket", 1\)/, 'notImplemented("http.Server")'],
];

// [nome do módulo embutido, arquivo, substituições próprias, o que o `export` final vira]
const modules = [
  ["diagnostics_channel", "node/diagnostics_channel.ts", []],
  ["_http_common", "node/_http_common.ts", httpCommon],
  ["_http_incoming", "node/_http_incoming.ts", [[/^export \{([\s\S]*?)\};?\s*$/m, "return {$1};"]]],
  ["_http_outgoing", "node/_http_outgoing.ts", httpCommon],
  ["_http_agent", "node/_http_agent.ts", httpCommon],
  ["_http_client", "node/_http_client.ts", [...asyncContext, ...httpCommon]],
  ["_http_server", "node/_http_server.ts", httpCommon],
  ["http", "node/http.ts", httpCommon],
  ["https", "node/https.ts", httpCommon],
];

const prologue = `  "use strict";
${commonPrologue}  const require = name => (name.startsWith("internal/") || name.startsWith("native/") ? host.internalModule(name) : host.requireBuiltin(name));
`;

for (const [name, file, extra] of modules) {
  const code = convert(root, file, [...common, ...extra, ...exportDefault]);
  const source =
    `// GERADO por \`scripts/gen-node-modules.js\` a partir de \`src/js/${file}\` do bun 1.4.2. Não editar à mão.\n` +
    "(function (host) {\n" +
    prologue +
    "  return (() => {\n" +
    indent(code) +
    "\n  })();\n" +
    "})\n";
  fs.writeFileSync(path.join(outDir, `node_${name}.js`), source);
}
