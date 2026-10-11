// O que os geradores `gen-node-*.js` que partem de TypeScript do bun 1.4.2 (MIT) dividem: tirar os tipos com `Bun.Transpiler`,
// trocar os intrínsecos do JSC pelos equivalentes públicos e falhar se sobrar um `$nome` sem tradução.
const fs = require("node:fs");
const path = require("node:path");

const transpiler = new Bun.Transpiler({ loader: "ts" });

// `root` é o `src/js` do bun. `replacements` são pares `[padrão, texto]` aplicados em ordem.
const convert = (root, file, replacements) => {
  let code = transpiler.transformSync(fs.readFileSync(path.join(root, file), "utf8"));
  for (const [pattern, replacement] of replacements) code = code.replace(pattern, replacement);
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`intrínseco sem tradução em ${file}: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
};

const indent = (code, spaces = 4) => code.replace(/^/gm, " ".repeat(spaces)).replace(/[ ]+$/gm, "");

// Os intrínsecos que viram a mesma coisa em todo módulo: `$call`, `$apply`, `$debug`, os `$ERR_*` (`host.internal.codes`),
// `$toClass`, `$makeAbortError`, `$isArray`, `$isObject`, `Promise.$resolve`, `$assert`, `$isTypedArrayView`, `$isUndefinedOrNull`.
const common = [
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$debug\(/g, "debug("],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\$toClass\(/g, "toClass("],
  [/\$makeAbortError\(/g, "makeAbortError("],
  [/\$isArray\(/g, "Array.isArray("],
  [/\$isObject\(/g, "isObject("],
  [/\$(HPE_[A-Z_0-9]+)\(/g, "codes.$1("],
  [/\$newPromise\(\)/g, "newPromise()"],
  [/\$resolvePromise(?:WithFirstResolvingFunctionCallCheck)?\(/g, "resolvePromise("],
  [/\.\$then\(/g, ".then("],
  [/\.\$has\(/g, ".has("],
  [/Promise\.\$resolve\b/g, "Promise.resolve"],
  [/Promise\.\$reject\b/g, "Promise.reject"],
  [/\$arrayPush\(/g, "Array.prototype.push.call("],
  [/\$assert\(/g, "assert("],
  [/\$inheritsBlob\(/g, "isBlob("],
  [/\$isCallable\(([^()]*)\)/g, '(typeof $1 === "function")'],
  [/\$isJSArray\(/g, "Array.isArray("],
  [/\$isTypedArrayView\(([^()]*)\)/g, "(ArrayBuffer.isView($1) && !($1 instanceof DataView))"],
  [/\$isUndefinedOrNull\(([^()]*)\)/g, "($1 == null)"],
];

// O prólogo que os intrínsecos acima supõem: `debug`, `assert` e `isObject` locais e o que vem de `host.internal`.
const commonPrologue = `  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  // \`$newPromise()\` e \`$resolvePromise()\` do JSC: uma promessa sem executor, resolvida de fora.
  const promiseResolvers = new WeakMap();
  const newPromise = () => {
    let resolve;
    const promise = new Promise(settle => {
      resolve = settle;
    });
    promiseResolvers.set(promise, resolve);
    return promise;
  };
  const resolvePromise = (promise, value) => promiseResolvers.get(promise)(value);
  const isBlob = value => typeof Blob === "function" && value instanceof Blob;
  // O que ainda não existe (TLS, \`BlockList\`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
`;

const exportDefault = [[/^export default (\w+|\{[\s\S]*?\n\});?\s*$/m, "return $1;"]];

module.exports = { convert, indent, common, commonPrologue, exportDefault };
