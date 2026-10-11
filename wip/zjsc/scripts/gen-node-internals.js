// Gera `src/api/node_internals.js`: os módulos `internal/*` do bun 1.4.2 (MIT, `src/js/internal/`) que `node:net`, `node:tls` e os
// outros módulos gerados leem, num registro só (cada um avaliado uma vez por programa: o `node:net` e o `node:tls` dividem os
// mesmos símbolos de `internal/net/symbols` e o mesmo `internal/shared`). A fábrica `(host)` devolve o `require(nome)` do
// registro; `host.internalModule(nome)` o chama.
//
// Uso: `bun scripts/gen-node-internals.js <dir src/js do bun> > src/api/node_internals.js`
const { convert, indent, common, commonPrologue, exportDefault } = require("./bun-gen-common.js");

const root = process.argv[2];
if (!root) {
  console.error("uso: bun gen-node-internals.js <src/js do bun>");
  process.exit(2);
}

// O `shared.ts` do bun amarra quatro coisas ao C++ do bun: o nome privado de `kInternalSendOptions`, a função de relatar exceção
// não tratada, o `$isJSArray` e o nome privado que o EventTarget lê. Aqui: sem a marca privada (ninguém a lê), a exceção vira
// uma microtask que lança (o caminho de uma exceção não tratada) e o símbolo `kResistStopPropagation` do `host.internal`.
const sharedReplacements = [
  [/^\$putByIdDirectPrivate\(kInternalSendOptions, "internal", true\);\n/m, ""],
  [/\$newCppFunction\("BunProcess\.cpp", "jsFunctionReportUncaughtException", 1\)/, "error => queueMicrotask(() => { throw error; })"],
  [/\$isJSArray\(/g, "Array.isArray("],
  [/\(options as AddEventListenerOptions\)\.\$kResistStopPropagation = true/, "options[host.internal.kResistStopPropagation] = true"],
  [/\.\$kResistStopPropagation = true/, "[host.internal.kResistStopPropagation] = true"],
];

// [nome do registro, arquivo, substituições próprias]
const modules = [
  ["internal/shared", "internal/shared.ts", sharedReplacements],
  ["internal/timers", "internal/timers.ts", []],
  ["internal/net/isIP", "internal/net/isIP.ts", []],
  ["internal/net/symbols", "internal/net/symbols.ts", []],
  ["internal/async_hooks", "internal/async_hooks.ts", []],
  ["internal/cluster/isPrimary", "internal/cluster/isPrimary.ts", []],
  [
    "internal/tls",
    "internal/tls.ts",
    [
      [/\$inheritsBlob\(/g, "isBlob("],
      [/\$rust\("SecureContext\.rs", "js\.getConstructor"\)/, 'require("native/secure-context")'],
      [/^export \{([\s\S]*?)\};?\s*$/m, "return {$1};"],
    ],
  ],
];

// O construtor nativo do `SecureContext` (BoringSSL no bun) ainda não existe: o objeto existe (o `node:tls` o guarda por
// identidade) e cada uso lança "not implemented".
const nativeSecureContext = `    "native/secure-context": () => {
      function SecureContext() {
        return host.internal.throwNotImplemented("tls SecureContext");
      }
      for (const name of ["intern", "createPrivate", "parsePkcs12"]) SecureContext[name] = () => host.internal.throwNotImplemented("tls SecureContext");
      return SecureContext;
    },`;

const definitions = modules
  .map(([name, file, extra]) => `    ${JSON.stringify(name)}: () => {\n${indent(convert(root, file, [...common, ...extra, ...exportDefault]), 6)}\n    },`)
  .concat(nativeSecureContext)
  .join("\n");

process.stdout.write(
  "// GERADO por `scripts/gen-node-internals.js` a partir dos `src/js/internal/*` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    commonPrologue +
    "  const cache = { __proto__: null };\n" +
    "  const definitions = {\n" +
    definitions +
    "\n  };\n" +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/primordials") return host.internal;\n' +
    "    if (name in definitions) {\n" +
    "      if (!(name in cache)) cache[name] = definitions[name]();\n" +
    "      return cache[name];\n" +
    "    }\n" +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    "  return require;\n" +
    "})\n",
);
