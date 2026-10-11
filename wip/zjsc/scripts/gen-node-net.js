// Gera `src/api/node_net.js` a partir de `src/js/node/net.ts` do bun 1.4.2 (MIT) e dos módulos internos que ele lê
// (`internal/shared`, `internal/timers`, `internal/net/isIP`, `internal/net/symbols`, `internal/async_hooks`,
// `internal/cluster/isPrimary`). Mesmo método do `gen-node-crypto.js`: tira os tipos com `Bun.Transpiler`, troca os
// intrínsecos do JSC pelos equivalentes públicos e embrulha numa fábrica `(host)`.
//
// O `net.ts` do bun é inteiro sobre o socket nativo do bun (`Bun.listen`, `doConnect`, `$write`): aqui esse socket é o do
// `bun_socket.js` (`host.socketInternals()` devolve o `write`, o `doConnect`, o `newDetachedSocket` e o `getBufferedAmount`
// dele). O que depende de TLS (`upgradeDuplexToTLS` e afins) e de `BlockList`/`SocketAddress` não existe ainda e lança
// "not implemented" no ponto de uso. Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-net.js <dir src/js do bun> > src/api/node_net.js`
const fs = require("node:fs");
const path = require("node:path");

const root = process.argv[2];
if (!root) {
  console.error("uso: bun gen-node-net.js <src/js do bun>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts" });

const convert = (file, replacements) => {
  let code = transpiler.transformSync(fs.readFileSync(path.join(root, file), "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`intrínseco sem tradução em ${file}: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
};

const indent = code => code.replace(/^/gm, "    ").replace(/[ ]+$/gm, "");

const common = [
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$debug\(/g, "debug("],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\$toClass\(/g, "toClass("],
];

const exportDefault = [[/^export default (\w+|\{[\s\S]*?\n\});?\s*$/m, "return $1;"]];

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

const internal = (file, extra = []) => indent(convert(file, [...common, ...extra, ...exportDefault]));

const netCode = convert("node/net.ts", [
  ...common,
  [/\$rust\("node_net_binding\.rs", "getDefaultAutoSelectFamily"\)/, "binding.getDefaultAutoSelectFamily"],
  [/\$rust\("node_net_binding\.rs", "setDefaultAutoSelectFamily"\)/, "binding.setDefaultAutoSelectFamily"],
  [/\$rust\("node_net_binding\.rs", "getDefaultAutoSelectFamilyAttemptTimeout"\)/, "binding.getDefaultAutoSelectFamilyAttemptTimeout"],
  [/\$rust\("node_net_binding\.rs", "setDefaultAutoSelectFamilyAttemptTimeout"\)/, "binding.setDefaultAutoSelectFamilyAttemptTimeout"],
  [/\$rust\("node_net_binding\.rs", "BlockList"\)/, 'notImplemented("net.BlockList")'],
  [/\$rust\("node_net_binding\.rs", "SocketAddress"\)/, 'notImplemented("net.SocketAddress")'],
  [/\$newRustFunction\("node_net_binding\.rs", "newDetachedSocket", 1\)/, "socketInternals.newDetachedSocket"],
  [/\$newRustFunction\("node_net_binding\.rs", "doConnect", 2\)/, "socketInternals.doConnect"],
  [/\$newRustFunction\("Listener\.rs", "jsAddServerName", 3\)/, 'notImplemented("tls server name")'],
  [/\$newRustFunction\("runtime\/socket\/socket\.rs", "jsUpgradeDuplexToTLS", 2\)/, 'notImplemented("tls upgrade")'],
  [/\$newRustFunction\("runtime\/socket\/socket\.rs", "jsUpgradeTLSDeferred", 2\)/, 'notImplemented("tls upgrade")'],
  [/\$newRustFunction\("runtime\/socket\/socket\.rs", "jsIsNamedPipeSocket", 1\)/, "() => false"],
  [/\$newRustFunction\("runtime\/socket\/socket\.rs", "jsGetBufferedAmount", 1\)/, "socketInternals.getBufferedAmount"],
  [/process\.binding\("uv"\)/, "uvConstants"],
  [/(\w+)\.\$write\(/g, "socketInternals.write($1, "],
  [/\$makeAbortError\(/g, "makeAbortError("],
  [/\$isArray\(/g, "Array.isArray("],
  [/\$isObject\(/g, "isObject("],
  [/Promise\.\$resolve\(/g, "Promise.resolve("],
  [/\$assert\(/g, "assert("],
  [/EventEmitter\.\$apply\(/g, "EventEmitter.apply("],
  [/^export default (netExports);?\s*$/m, "return $1;"],
]);

const prologue = `  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const socketInternals = host.socketInternals();
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  const uvConstants = { UV_ECANCELED: -125, UV_ENOBUFS: -105, UV_ETIMEDOUT: -110 };
  // O que o TLS e o \`BlockList\` ainda não têm: lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  // O estado de \`autoSelectFamily\` do \`node_net_binding.rs\` do bun.
  let autoSelectFamilyDefault = true;
  let autoSelectFamilyAttemptTimeoutDefault = 500;
  const binding = {
    getDefaultAutoSelectFamily: () => autoSelectFamilyDefault,
    setDefaultAutoSelectFamily: value => {
      if (typeof value !== "boolean") throw codes.INVALID_ARG_TYPE("autoSelectFamilyDefault", "boolean", value);
      autoSelectFamilyDefault = value;
      return value;
    },
    getDefaultAutoSelectFamilyAttemptTimeout: () => autoSelectFamilyAttemptTimeoutDefault,
    setDefaultAutoSelectFamilyAttemptTimeout: value => {
      validators.validateInt32(value, "value", 1);
      autoSelectFamilyAttemptTimeoutDefault = value < 10 ? 10 : value;
      return autoSelectFamilyAttemptTimeoutDefault;
    },
  };
  const kInternal = { __proto__: null };
  const moduleCache = { __proto__: null };
  const definitions = {
    "internal/shared": () => {
${internal("internal/shared.ts", sharedReplacements)}
    },
    "internal/timers": () => {
${internal("internal/timers.ts")}
    },
    "internal/net/isIP": () => {
${internal("internal/net/isIP.ts")}
    },
    "internal/net/symbols": () => {
${internal("internal/net/symbols.ts")}
    },
    "internal/async_hooks": () => {
${internal("internal/async_hooks.ts")}
    },
    "internal/cluster/isPrimary": () => {
${internal("internal/cluster/isPrimary.ts")}
    },
  };
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/streams/duplex" || name === "internal/streams/state" || name === "internal/abort_listener") return host.requireInternal(name);
    // Nenhuma categoria de trace está ligada: o \`isCategoryGroupEnabled\` responde que não e nada é emitido.
    if (name === "internal/trace_events") return { isCategoryGroupEnabled: () => false, emitEvent() {}, emitEventWithArgs() {} };
    if (name === "internal/primordials") return host.internal;
    if (name in definitions) {
      if (!(name in moduleCache)) moduleCache[name] = definitions[name]();
      return moduleCache[name];
    }
    return host.requireBuiltin(name);
  };
`;

process.stdout.write(
  "// GERADO por `scripts/gen-node-net.js` a partir de `src/js/node/net.ts` e dos `internal/*` que ele lê, do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    prologue +
    "  return (() => {\n" +
    indent(netCode) +
    "\n  })();\n" +
    "})\n",
);
