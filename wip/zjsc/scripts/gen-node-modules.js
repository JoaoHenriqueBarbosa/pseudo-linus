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

// O `child_process.ts` fala com o `Bun.spawn` real; o `writableFromFileSink` vem do módulo gerado de `internal/fs/streams.ts` e o
// `constructNativeReadable` do registro de internos (`native/native-readable`).
const childProcess = [
  [/require\("internal\/fs\/streams"\)/g, "host.fsStreams()"],
  [/require\("internal\/streams\/native-readable"\)/g, 'require("native/native-readable")'],
  // `$debug` sem chamada é a flag `DEBUG_CHILD_PROCESS`, desligada.
  [/\$debug(?!\()/g, "false"],
  [/\$newRustFunction\("node_util_binding\.rs", "etimedoutErrorCode", 0\)/, "(() => -110)"],
  [/\$newRustFunction\("node_util_binding\.rs", "enobufsErrorCode", 0\)/, "(() => -105)"],
];

// `node:zlib`: as constantes saem do JSON medido no bun 1.4.2 (`Z_MAX_CHUNK` é `Infinity`); `crc32` vai sobre o `Bun.hash.crc32`
// com a validação medida no oráculo; as classes nativas (`NativeZlib`, `NativeBrotli`, `NativeZstd`) existem com o protótipo, e o
// `new` lança "not implemented" até o motor de fluxo entrar.
const zlibConstants = JSON.parse(fs.readFileSync(path.join(__dirname, "../src/api/node_zlib_constants.json"), "utf8"));
zlibConstants.Z_MAX_CHUNK = Infinity;
const zlibNative = name => `(native => (toClass(native, "${name}"), native))(function ${name}() { host.internal.throwNotImplemented("zlib.${name}"); })`;
const zlib = [
  [/process\.binding\("constants"\)/g, `({ zlib: Object.freeze(${JSON.stringify(zlibConstants).replace('"Z_MAX_CHUNK":null', '"Z_MAX_CHUNK":Infinity')}) })`],
  [
    /\$newRustFunction\("node_zlib_binding\.rs", "crc32", 1\)/,
    `((hashCrc32) => function crc32(data, ...rest) {
      let value = rest[0];
      if (typeof data === "string") data = new TextEncoder().encode(data);
      else if (data === undefined) throw host.internal.codes.INVALID_ARG_TYPE("data", ["string", "Buffer", "TypedArray", "DataView"], data);
      else if (!ArrayBuffer.isView(data) && !(data instanceof ArrayBuffer) && !(data instanceof SharedArrayBuffer)) {
        throw host.internal.makeError("TypeError", "ERR_INVALID_ARG_TYPE", \`The "data" property must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received \${data === null ? "object" : typeof data}\`);
      }
      if (rest.length === 0) value = 0;
      else if (typeof value !== "number") throw host.internal.codes.INVALID_ARG_TYPE("value", "number", value);
      else if (!Number.isInteger(value)) throw host.internal.codes.OUT_OF_RANGE("value", "an integer", value);
      else if (value < 0 || value > 4294967295) throw host.internal.codes.OUT_OF_RANGE("value", ">= 0 and <= 4294967295", value);
      return hashCrc32(data instanceof ArrayBuffer || data instanceof SharedArrayBuffer ? new Uint8Array(data) : data, value);
    })(Bun.hash.crc32)`,
  ],
  [/\$rust\("node_zlib_binding\.rs", "(Native\w+)"\)/g, (_, name) => zlibNative(name)],
  [/\$requireMap\.\$get\("buffer"\)\?\.exports\.kMaxLength \?\? /, ""],
];

// `node:dns`: o `Resolver` nativo do bun responde como o `Bun.dns` (mesma máquina sem rede), com `cancel`; a ordem padrão é `verbatim`.
const dns = [
  [/\$newRustFunction\(\s*"runtime\/dns_jsc\/dns\.rs",\s*"Resolver\.getRuntimeDefaultResultOrderOption",\s*0,?\s*\)/, '(() => "verbatim")'],
  [/\$newRustFunction\("runtime\/dns_jsc\/dns\.rs", "Resolver\.newResolver", 1\)/, "((options) => Object.assign(Object.create(Bun.dns), { cancel() {} }))"],
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
  ["child_process", "node/child_process.ts", childProcess],
  ["zlib", "node/zlib.ts", zlib],
  ["dns", "node/dns.ts", dns],
  ["dns_promises", "node/dns.promises.ts", [[/^export default (require\(.*\)\.\w+);?\s*$/m, "return $1;"]]],
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
