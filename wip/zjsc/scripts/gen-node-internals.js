// Gera `src/api/node_internals.js`: os módulos `internal/*` do bun 1.4.2 (MIT, `src/js/internal/`) que `node:net`, `node:tls`, os
// `node:_http_*` e os outros módulos gerados leem, num registro só (cada um avaliado uma vez por programa: os módulos dividem os
// mesmos símbolos de `internal/net/symbols` e o mesmo `internal/shared`). A fábrica `(host)` devolve o `require(nome)` do
// registro; `host.internalModule(nome)` o chama. Os nomes `native/*` são o que no bun é C++ ou Rust e que o registro monta
// aqui em JavaScript.
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
  [/\(options as AddEventListenerOptions\)\.\$kResistStopPropagation = true/, "options[host.internal.kResistStopPropagation] = true"],
  [/\.\$kResistStopPropagation = true/, "[host.internal.kResistStopPropagation] = true"],
];

// O `internal/http` do bun: o que é C++ (`NodeHTTP.cpp`, o servidor do `Bun.serve`) lança "not implemented" no ponto de uso; o
// tamanho máximo de cabeçalho é o estado de `httpBinding` (abaixo).
const httpReplacements = [
  [
    /\$cpp\(\s*"NodeHTTP\.cpp",\s*"createNodeHTTPInternalBinding",?\s*\)/,
    '{ setServerCustomOptions: notImplemented("http.Server"), setServerAppFlags: notImplemented("http.Server"), drainMicrotasks: notImplemented("http.Server") }',
  ],
  // Os `const enum` do bun saem exportados (o `_http_server.ts` e o `_http_incoming.ts` os desestruturam): sem o `export`, e
  // entregues junto do bloco final de exportações.
  [/^export (var|const|let|function|class) /gm, "$1 "],
  [
    /^export \{([\s\S]*?)\};?\s*$/m,
    "return {\n  NodeHTTPResponseAbortEvent,\n  NodeHTTPIncomingRequestType,\n  NodeHTTPBodyReadState,\n  NodeHTTPResponseFlags,\n  NodeHTTPHeaderState,$1};",
  ],
  [/\$newRustFunction\("node_http_binding\.rs", "setMaxHTTPHeaderSize", 1\)/, "httpBinding.setMaxHTTPHeaderSize"],
  [/\$newRustFunction\("node_http_binding\.rs", "getMaxHTTPHeaderSize", 0\)/, "httpBinding.getMaxHTTPHeaderSize"],
];

// O contexto assíncrono do `AsyncContextFrame` é o `$asyncContext` do JSC (ver `node_async_hooks.js`).
const asyncContextReplacements = [
  [/\$getInternalField\(\$asyncContext, 0\)/g, "host.getAsyncContext()"],
  [/\$putInternalField\(\$asyncContext, 0, (\w+)\)/g, "host.setAsyncContext($1)"],
];

// [nome do registro, arquivo, substituições próprias]
const modules = [
  ["internal/shared", "internal/shared.ts", sharedReplacements],
  ["internal/timers", "internal/timers.ts", []],
  ["internal/net/isIP", "internal/net/isIP.ts", []],
  ["internal/net/symbols", "internal/net/symbols.ts", []],
  ["internal/async_hooks", "internal/async_hooks.ts", []],
  ["internal/async_context_frame", "internal/async_context_frame.ts", asyncContextReplacements],
  ["internal/cluster/isPrimary", "internal/cluster/isPrimary.ts", []],
  ["internal/freelist", "internal/freelist.ts", []],
  ["internal/url", "internal/url.ts", []],
  ["internal/util/deprecate", "internal/util/deprecate.ts", []],
  ["internal/http", "internal/http.ts", httpReplacements],
  ["internal/http/FakeSocket", "internal/http/FakeSocket.ts", []],
  ["internal/http1_server_fallback", "internal/http1_server_fallback.ts", [[/process\.binding\("http_parser"\)/, 'require("native/http_parser")']]],
  [
    "internal/tls",
    "internal/tls.ts",
    [
      [/\$rust\("SecureContext\.rs", "js\.getConstructor"\)/, 'require("native/secure-context")'],
      [/^export \{([\s\S]*?)\};?\s*$/m, "return {$1};"],
    ],
  ],
];

// O que no bun é nativo e ainda não existe aqui: o objeto existe com a forma que o bun mostra e cada uso lança "not implemented".
const synthetic = {
  // O construtor nativo do `SecureContext` (BoringSSL no bun): o `node:tls` o guarda por identidade.
  "native/secure-context": `
      function SecureContext() {
        return host.internal.throwNotImplemented("tls SecureContext");
      }
      toClass(SecureContext, "SecureContext");
      for (const name of ["intern", "createPrivate", "parsePkcs12"]) SecureContext[name] = () => host.internal.throwNotImplemented("tls SecureContext");
      return SecureContext;`,
  // `process.binding("http_parser")`: as tabelas de métodos e as constantes do llhttp (medidas no bun 1.4.2); o analisador em si
  // (`HTTPParser.prototype.execute` e companhia) ainda não existe.
  "native/http_parser": `
      const methods = ["DELETE", "GET", "HEAD", "POST", "PUT", "CONNECT", "OPTIONS", "TRACE", "COPY", "LOCK", "MKCOL", "MOVE", "PROPFIND", "PROPPATCH", "SEARCH", "UNLOCK", "BIND", "REBIND", "UNBIND", "ACL", "REPORT", "MKACTIVITY", "CHECKOUT", "MERGE", "M-SEARCH", "NOTIFY", "SUBSCRIBE", "UNSUBSCRIBE", "PATCH", "PURGE", "MKCALENDAR", "LINK", "UNLINK", "SOURCE", "QUERY"];
      const allMethods = [...methods.slice(0, 34), "PRI", "DESCRIBE", "ANNOUNCE", "SETUP", "PLAY", "PAUSE", "TEARDOWN", "GET_PARAMETER", "SET_PARAMETER", "REDIRECT", "RECORD", "FLUSH", "QUERY"];
      function HTTPParser() {
        return host.internal.throwNotImplemented("http parser");
      }
      toClass(HTTPParser, "HTTPParser");
      for (const [name, value] of Object.entries({
        REQUEST: 1, RESPONSE: 2, kOnMessageBegin: 0, kOnHeaders: 1, kOnHeadersComplete: 2, kOnBody: 3, kOnMessageComplete: 4, kOnExecute: 5,
        kOnTimeout: 6, kLenientNone: 0, kLenientHeaders: 1, kLenientChunkedLength: 2, kLenientKeepAlive: 4, kLenientTransferEncoding: 8,
        kLenientVersion: 16, kLenientDataAfterClose: 32, kLenientOptionalLFAfterCR: 64, kLenientOptionalCRLFAfterChunk: 128,
        kLenientOptionalCRBeforeLF: 256, kLenientSpacesAfterChunkSize: 512, kLenientHeaderValueRelaxed: 1, kLenientAll: 1023,
      })) HTTPParser[name] = value;
      for (const name of ["close", "free", "remove", "execute", "finish", "initialize", "pause", "resume", "consume", "unconsume", "getCurrentBuffer", "duration", "headersCompleted"]) {
        HTTPParser.prototype[name] = function () {
          return host.internal.throwNotImplemented("http parser");
        };
      }
      function ConnectionsList() {
        return host.internal.throwNotImplemented("http connections list");
      }
      toClass(ConnectionsList, "ConnectionsList");
      return { methods, allMethods, HTTPParser, ConnectionsList };`,
  // `constructNativeReadable` do bun (`internal/streams/native-readable.ts`) liga um `Readable` ao ponteiro nativo do fluxo; aqui o
  // `Readable` lê do `ReadableStream` pelo leitor, que é o caminho que o `Readable.fromWeb` do bun usa sem o ponteiro nativo.
  "native/native-readable": `
      const Readable = require("internal/streams/readable");
      function constructNativeReadable(readableStream, options) {
        const reader = readableStream.getReader();
        let reading = false;
        return new Readable({
          ...options,
          read() {
            if (reading) return;
            reading = true;
            reader.read().then(
              ({ done, value }) => {
                reading = false;
                if (done) this.push(null);
                else this.push(value);
              },
              error => {
                reading = false;
                this.destroy(error);
              },
            );
          },
          destroy(error, callback) {
            reader.cancel(error).then(() => callback(error), () => callback(error));
          },
        });
      }
      return { constructNativeReadable };`,
  // Nenhuma categoria de trace está ligada: o `isCategoryGroupEnabled` responde que não e nada é emitido.
  "internal/trace_events": `
      return { isCategoryGroupEnabled: () => false, emitEvent() {}, emitEventWithArgs() {} };`,
};

// Cada módulo sintético é uma função de nível de fábrica (não uma seta dentro do objeto de definições).
const syntheticName = name => `define_${name.replace(/[^A-Za-z0-9]/g, "_")}`;
const syntheticFunctions = Object.entries(synthetic)
  .map(([name, body]) => `  function ${syntheticName(name)}() {${body}\n  }`)
  .join("\n");

const definitions = modules
  .map(([name, file, extra]) => `    ${JSON.stringify(name)}: () => {\n${indent(convert(root, file, [...common, ...extra, ...exportDefault]), 6)}\n    },`)
  .concat(Object.keys(synthetic).map(name => `    ${JSON.stringify(name)}: ${syntheticName(name)},`))
  .join("\n");


process.stdout.write(
  "// GERADO por `scripts/gen-node-internals.js` a partir dos `src/js/internal/*` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    '  "use strict";\n' +
    commonPrologue +
    "  // O estado de `node_http_binding.rs` do bun.\n" +
    "  let maxHeaderSize = 16384;\n" +
    "  const httpBinding = {\n" +
    "    getMaxHTTPHeaderSize: () => maxHeaderSize,\n" +
    "    setMaxHTTPHeaderSize(value) {\n" +
    '      if (!Number.isInteger(value) || value < 0) throw host.internal.makeError("TypeError", "ERR_INVALID_ARG_TYPE", `The "maxHeaderSize" argument must be of type non-negative integer. Received ${host.internal.determineSpecificType(value)}`);\n' +
    "      maxHeaderSize = value;\n" +
    "    },\n" +
    "  };\n" +
    syntheticFunctions +
    "\n" +
    "  const cache = { __proto__: null };\n" +
    "  const definitions = {\n" +
    definitions +
    "\n  };\n" +
    "  const require = name => {\n" +
    '    if (name === "internal/validators") return validators;\n' +
    '    if (name === "internal/primordials") return host.internal;\n' +
    '    if (name === "internal/util/inspect") return host.requireBuiltin("node:util");\n' +
    '    if (name.startsWith("internal/stream") || name === "internal/abort_listener") return host.requireInternal(name);\n' +
    "    if (name in definitions) {\n" +
    "      if (!(name in cache)) cache[name] = definitions[name]();\n" +
    "      return cache[name];\n" +
    "    }\n" +
    "    return host.requireBuiltin(name);\n" +
    "  };\n" +
    "  return require;\n" +
    "})\n",
);
