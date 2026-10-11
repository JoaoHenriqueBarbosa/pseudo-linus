// Gera `src/api/node_net.js` a partir de `src/js/node/net.ts` do bun 1.4.2 (MIT). Mesmo método do `gen-node-crypto.js`: tira os
// tipos com `Bun.Transpiler`, troca os intrínsecos do JSC pelos equivalentes públicos e embrulha numa fábrica `(host)`. Os
// módulos `internal/*` que ele lê vêm do registro `node_internals.js` (`host.internalModule`).
//
// O `net.ts` do bun é inteiro sobre o socket nativo do bun (`Bun.listen`, `doConnect`, `$write`): aqui esse socket é o do
// `bun_socket.js` (`host.socketInternals()` devolve o `write`, o `doConnect`, o `newDetachedSocket` e o `getBufferedAmount`
// dele). O que depende de TLS (`upgradeDuplexToTLS` e afins) e de `BlockList`/`SocketAddress` não existe ainda e lança
// "not implemented" no ponto de uso. Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-net.js <dir src/js do bun> > src/api/node_net.js`
const { convert, indent, common, commonPrologue } = require("./bun-gen-common.js");

const root = process.argv[2];
if (!root) {
  console.error("uso: bun gen-node-net.js <src/js do bun>");
  process.exit(2);
}

const netCode = convert(root, "node/net.ts", [
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
  [/EventEmitter\.\$apply\(/g, "EventEmitter.apply("],
  [/^export default (netExports);?\s*$/m, "return $1;"],
]);

const prologue = `  "use strict";
${commonPrologue}  const socketInternals = host.socketInternals();
  const uvConstants = { UV_ECANCELED: -125, UV_ENOBUFS: -105, UV_ETIMEDOUT: -110 };
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
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/streams/duplex" || name === "internal/streams/state" || name === "internal/abort_listener") return host.requireInternal(name);
    // Nenhuma categoria de trace está ligada: o \`isCategoryGroupEnabled\` responde que não e nada é emitido.
    if (name === "internal/trace_events") return { isCategoryGroupEnabled: () => false, emitEvent() {}, emitEventWithArgs() {} };
    if (name.startsWith("internal/")) return host.internalModule(name);
    return host.requireBuiltin(name);
  };
`;

process.stdout.write(
  "// GERADO por `scripts/gen-node-net.js` a partir de `src/js/node/net.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    prologue +
    "  return (() => {\n" +
    indent(netCode) +
    "\n  })();\n" +
    "})\n",
);
