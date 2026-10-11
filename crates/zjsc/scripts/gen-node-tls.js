// Gera `src/api/node_tls.js` a partir de `src/js/node/tls.ts` do bun 1.4.2 (MIT). Os `internal/*` que ele lê vêm do registro
// `node_internals.js` (`host.internalModule`). O que no bun é C++ (`NodeTLS.cpp`) ou BoringSSL fica no objeto `tlsBinding`:
// `canonicalizeIP` e a lista de cifras são JavaScript, os certificados raiz embutidos vêm do JSON medido no `tls.rootCertificates`
// do bun 1.4.2 (121 certificados), e o `SecureContext` nativo (e com ele o handshake) ainda não existe: lança "not implemented"
// no ponto de uso. Se sobrar um `$nome` sem tradução o script falha.
//
// Uso: `bun scripts/gen-node-tls.js <dir src/js do bun> <certificados.json> > src/api/node_tls.js`
const fs = require("node:fs");
const { convert, indent, common, commonPrologue, exportDefault } = require("./bun-gen-common.js");

const [root, certificates] = process.argv.slice(2);
if (!root || !certificates) {
  console.error("uso: bun gen-node-tls.js <src/js do bun> <certificados.json>");
  process.exit(2);
}

const tlsCode = convert(root, "node/tls.ts", [
  ...common,
  [/\$newRustFunction\("Listener\.rs", "jsAddServerName", 3\)/, 'notImplemented("tls server name")'],
  [/\$newCppFunction\("NodeTLS\.cpp", "getBundledRootCertificates", 1\)/, "tlsBinding.getBundledRootCertificates"],
  [/\$newCppFunction\("NodeTLS\.cpp", "getExtraCACertificates", 1\)/, "tlsBinding.getExtraCACertificates"],
  [/\$newCppFunction\("NodeTLS\.cpp", "getSystemCACertificates", 1\)/, "tlsBinding.getSystemCACertificates"],
  [/\$newCppFunction\("NodeTLS\.cpp", "Bun__canonicalizeIP", 1\)/, "tlsBinding.canonicalizeIP"],
  [/\$newCppFunction\("NodeTLS\.cpp", "parseCACertificates", 1\)/, "tlsBinding.parseCACertificates"],
  [/\$newCppFunction\("NodeTLS\.cpp", "getDefaultCiphers", 0\)/, "tlsBinding.getDefaultCiphers"],
  [/\$newCppFunction\("NodeTLS\.cpp", "setDefaultCiphers", 1\)/, "tlsBinding.setDefaultCiphers"],
  [/\$newRustFunction\("bun\.rs", "getUseSystemCA", 0\)/, "() => false"],
  [/\$rust\("SecureContext\.rs", "js\.getConstructor"\)/, 'require("native/secure-context")'],
  ...exportDefault,
]);

const roots = JSON.stringify(JSON.parse(fs.readFileSync(certificates, "utf8")));

const prologue = `  "use strict";
${commonPrologue}  // O \`NodeTLS.cpp\` do bun. A lista de cifras é a de \`default_ciphers.h\` depois dos três \`TLS_\` que o \`getDefaultCiphers\` do
  // JavaScript antepõe (medida em \`tls.DEFAULT_CIPHERS\` do bun 1.4.2).
  let defaultCiphers = "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA";
  // \`inet_ntop(inet_pton(texto))\`: o texto canônico de um endereço IPv4 ou IPv6, ou \`undefined\` se não for um.
  function canonicalizeIP(text) {
    if (typeof text !== "string") return undefined;
    const v4 = value => {
      const parts = value.split(".");
      if (parts.length !== 4) return undefined;
      const bytes = [];
      for (const part of parts) {
        if (!/^(0|[1-9][0-9]{0,2})$/.test(part) || Number(part) > 255) return undefined;
        bytes.push(Number(part));
      }
      return bytes;
    };
    const four = v4(text);
    if (four !== undefined) return four.join(".");
    if (!text.includes(":")) return undefined;
    let groups;
    const halves = text.split("::");
    if (halves.length > 2) return undefined;
    const parse = side => {
      if (side === "") return [];
      const out = [];
      const pieces = side.split(":");
      for (let index = 0; index < pieces.length; index++) {
        const piece = pieces[index];
        if (index === pieces.length - 1 && piece.includes(".")) {
          const bytes = v4(piece);
          if (bytes === undefined) return undefined;
          out.push((bytes[0] << 8) | bytes[1], (bytes[2] << 8) | bytes[3]);
        } else if (/^[0-9a-fA-F]{1,4}$/.test(piece)) {
          out.push(parseInt(piece, 16));
        } else {
          return undefined;
        }
      }
      return out;
    };
    const head = parse(halves[0]);
    if (head === undefined) return undefined;
    if (halves.length === 2) {
      const tail = parse(halves[1]);
      if (tail === undefined || head.length + tail.length > 7) return undefined;
      groups = [...head, ...new Array(8 - head.length - tail.length).fill(0), ...tail];
    } else {
      if (head.length !== 8) return undefined;
      groups = head;
    }
    // O IPv4 mapeado (\`::ffff:a.b.c.d\`) o \`inet_ntop\` escreve com o IPv4 em decimal.
    if (groups.slice(0, 5).every(group => group === 0) && groups[5] === 0xffff) {
      return \`::ffff:\${groups[6] >> 8}.\${groups[6] & 255}.\${groups[7] >> 8}.\${groups[7] & 255}\`;
    }
    let bestStart = -1;
    let bestLength = 0;
    for (let start = 0; start < 8; ) {
      if (groups[start] !== 0) {
        start++;
        continue;
      }
      let end = start;
      while (end < 8 && groups[end] === 0) end++;
      if (end - start > bestLength) {
        bestStart = start;
        bestLength = end - start;
      }
      start = end;
    }
    const hex = group => group.toString(16);
    if (bestLength < 2) return groups.map(hex).join(":");
    return \`\${groups.slice(0, bestStart).map(hex).join(":")}::\${groups.slice(bestStart + bestLength).map(hex).join(":")}\`;
  }
  const tlsBinding = {
    getBundledRootCertificates: () => JSON.parse(${JSON.stringify(roots)}),
    // \`NODE_EXTRA_CA_CERTS\` sem o arquivo lido (o hospedeiro não lê arquivos daqui): vazio quando a variável não existe.
    getExtraCACertificates: () => {
      if (process.env.NODE_EXTRA_CA_CERTS) host.internal.throwNotImplemented("tls NODE_EXTRA_CA_CERTS");
      return [];
    },
    getSystemCACertificates: notImplemented("tls system CA certificates"),
    parseCACertificates: notImplemented("tls.setDefaultCACertificates"),
    canonicalizeIP,
    getDefaultCiphers: () => defaultCiphers,
    setDefaultCiphers: value => {
      defaultCiphers = value;
    },
  };
  const require = name => (name.startsWith("internal/") || name.startsWith("native/") ? host.internalModule(name) : host.requireBuiltin(name));
`;

process.stdout.write(
  "// GERADO por `scripts/gen-node-tls.js` a partir de `src/js/node/tls.ts` do bun 1.4.2. Não editar à mão.\n" +
    "(function (host) {\n" +
    prologue +
    "  return (() => {\n" +
    indent(tlsCode) +
    "\n  })();\n" +
    "})\n",
);
