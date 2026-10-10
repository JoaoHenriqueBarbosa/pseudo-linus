// Gera `src/api/node_util.js` a partir dos fontes do bun 1.4.2 (MIT): `src/js/node/util.ts` e os internos que ele
// carrega (`internal/primordials.js`, `internal/util/inspect.js`, `internal/promisify.ts`,
// `internal/util/deprecate.ts`, `internal/util/colors.ts` e `internal/tty.ts`).
//
// Uso: `bun scripts/gen-node-util.js <diretório src/js do bun> > src/api/node_util.js`.
//
// Tira os tipos do TypeScript (`Bun.Transpiler`), troca os intrínsecos do JSC e as funções nativas do bun pelos
// equivalentes do `host` e põe cada interno num escopo próprio, como o módulo à parte que ele é no bun. Se sobrar um
// `$nome` sem tradução o script falha, em vez de deixar um intrínseco solto no módulo.
const fs = require("node:fs");
const path = require("node:path");

const [root] = process.argv.slice(2);
if (!root) {
  console.error("uso: bun gen-node-util.js <src/js do bun>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts", minifyWhitespace: false, inline: true });

// As trocas comuns a todos os arquivos.
const common = [
  [/require\("internal\/primordials"\)/g, "primordials"],
  [/require\("internal\/validators"\)/g, "host.internal.validators"],
  [/require\("internal\/shared"\)/g, "host.internal"],
  [/require\("internal\/promisify"\)/g, "promisifyModule"],
  [/require\("internal\/util\/deprecate"\)/g, "deprecateModule"],
  [/require\("internal\/util\/inspect"\)/g, "inspectModule"],
  [/require\("internal\/util\/colors"\)/g, "colorsModule"],
  [/require\("internal\/util\/comparisons"\)/g, "{ isDeepStrictEqual: host.isDeepStrictEqual }"],
  [/require\("internal\/util\/mime"\)/g, "{ MIMEType: host.MIMEType, MIMEParams: host.MIMEParams }"],
  [/require\("internal\/streams\/utils"\)/g, 'host.requireInternal("internal/streams/utils")'],
  [/require\("internal\/tty"\)/g, "ttyModule"],
  [/require\("(node:)?([a-z_/]+)"\)/g, 'host.requireBuiltin("node:$2")'],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$isJSArray\(/g, "host.isJSArray("],
  [/\$isArray\b/g, "ArrayIsArrayIntrinsic"],
  [/\$isObject\(/g, "isObject("],
  [/\$isPromise\(/g, "host.isPromise("],
  [/\$min\(/g, "MathMinIntrinsic("],
  [/^export default /m, "return "],
];

// As trocas de cada arquivo: as funções nativas que ele recebe do C++ ou do Rust do bun.
const perFile = {
  "internal/primordials.js": [
    [/\$getByIdDirect\(Function\.prototype, "call"\)/g, "Function.prototype.call"],
    [/\$Promise\.prototype\.\$then/g, "Promise.prototype.then"],
    [/Promise\.\$resolve\b/g, "Promise.resolve"],
  ],
  "internal/util/inspect.js": [
    [/\$newRustFunction\(\s*"node_util_binding\.rs",\s*"extractedSplitNewLinesFastPathStringsOnly",\s*1,?\s*\)/g, "host.splitNewLines"],
    [/\$newCppFunction\("stringWidth\.cpp", "jsFunctionBunStringWidth", 1\)/g, "host.stringWidth"],
    [/\$newCppFunction\("UtilInspect\.cpp", "jsFunctionGetOwnNonIndexProperties", 2\)/g, "host.getOwnNonIndexProperties"],
    [/\$requireMap\.\$get\("buffer"\)\?\.exports\.INSPECT_MAX_BYTES \?\? /g, ""],
    [/\$peekPromiseStatus\(/g, "host.promiseStatus("],
    [/\$peekPromiseSettledValue\(/g, "host.promiseResult("],
    [/\$isProxyObject\(/g, "host.isProxyObject("],
    [/\$getProxyInternalField\((\w+), \$proxyFieldHandler\)/g, "host.proxyHandler($1)"],
    [/\$getProxyInternalField\((\w+), \$proxyFieldTarget\)/g, "host.proxyTarget($1)"],
    [/\$getInternalField\(/g, "host.getInternalField("],
    [/\$isMap\(/g, "isMap("],
    [/\$isSet\(/g, "isSet("],
  ],
  "node/util.ts": [
    [/\$newRustFunction\("node_util_binding\.rs", "internalErrorName", 1\)/g, "host.internalErrorName"],
    [/\$newRustFunction\("node_util_binding\.rs", "internalErrorEntries", 0\)/g, "host.internalErrorEntries"],
    [/\$newRustFunction\("node_util_binding\.rs", "parseEnv", 1\)/g, "host.parseEnv"],
    [/\$newRustFunction\("parse_args\.rs", "parseArgs", 1\)/g, "host.parseArgs"],
    [/\$newCppFunction\("NodeUtilTypesModule\.cpp", "jsFunctionIsError", 1\)/g, "host.isError"],
    [/\$newPromise\(\)/g, "host.newPromise()"],
    [/Promise\.\$resolve\(\)/g, "Promise.resolve()"],
    [/\$resolvePromiseWithFirstResolvingFunctionCallCheck\(/g, "host.resolvePromise("],
  ],
};

function convert(name) {
  const file = path.join(root, name.endsWith(".js") || name.endsWith(".ts") ? name : name + ".ts");
  let code = transpiler.transformSync(fs.readFileSync(file, "utf8"));
  for (const [pattern, replacement] of [...(perFile[name] ?? []), ...common]) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`${name}: intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
}

const indent = text => text.replace(/^/gm, "  ").replace(/[ ]+$/gm, "");

// Cada interno num escopo próprio: `const nome = (() => { ...; return exports; })();`.
function scoped(variable, name) {
  return `const ${variable} = (() => {\n${indent(convert(name))}\n})();\n`;
}

let out = "// GERADO por `scripts/gen-node-util.js` a partir do `util.ts` e dos internos que ele usa, do bun 1.4.2. Não editar à mão.\n";
out += "(function (host) {\n";
out += '  "use strict";\n\n';
out += "  const { codes } = host.internal;\n";
out += "  // `$isArray` e `$min`: os intrínsecos guardados antes de qualquer código do usuário.\n";
out += "  const ArrayIsArrayIntrinsic = Array.isArray;\n";
out += "  const MathMinIntrinsic = Math.min;\n";
out += "  // `$isObject`: objeto ou função.\n";
out += '  const isObject = value => (typeof value === "object" && value !== null) || typeof value === "function";\n';
out += indent(scoped("primordials", "internal/primordials.js"));
out += indent(scoped("ttyModule", "internal/tty"));
out += indent(scoped("colorsModule", "internal/util/colors"));
out += indent(scoped("inspectModule", "internal/util/inspect.js"));
out += indent(scoped("promisifyModule", "internal/promisify"));
out += indent(scoped("deprecateModule", "internal/util/deprecate"));
out += indent(convert("node/util.ts"));
out += "\n})\n";
process.stdout.write(out);
