// Gera `src/api/node_assert.js` a partir dos fontes do bun 1.4.2 (MIT): `src/js/node/assert.ts` e os internos que ele
// carrega (`internal/assert/myers_diff.ts`, `internal/assert/assertion_error.ts`, `internal/assert/utils.ts` e
// `internal/assert/calltracker.ts`).
//
// Uso: `bun scripts/gen-node-assert.js <diretório src/js do bun> > src/api/node_assert.js`.
//
// Como o `gen-node-util.js`: tira os tipos, troca os intrínsecos e as funções nativas pelo `host` e põe cada interno num
// escopo próprio. Os internos do `util` (`primordials`, `inspect`, `colors`, `deprecate`) são as mesmas instâncias do
// `node:util`, entregues por `host.utilInternal`, como no registro único de módulos do bun.
const fs = require("node:fs");
const path = require("node:path");

const [root] = process.argv.slice(2);
if (!root) {
  console.error("uso: bun gen-node-assert.js <src/js do bun>");
  process.exit(2);
}

const transpiler = new Bun.Transpiler({ loader: "ts", minifyWhitespace: false, inline: true });

const common = [
  [/require\("internal\/primordials"\)/g, 'host.utilInternal("primordials")'],
  [/require\("internal\/util\/inspect"\)/g, 'host.utilInternal("inspect")'],
  [/require\("internal\/util\/colors"\)/g, 'host.utilInternal("colors")'],
  [/require\("internal\/util\/deprecate"\)/g, 'host.utilInternal("deprecate")'],
  [/require\("internal\/validators"\)/g, "host.internal.validators"],
  [/require\("internal\/util\/comparisons"\)/g, "{ isDeepStrictEqual: host.jsFunctionIsDeepStrictEqual }"],
  [/require\("internal\/assert\/myers_diff"\)/g, "myersDiffModule"],
  [/require\("internal\/assert\/assertion_error"\)/g, "assertionErrorModule"],
  [/require\("internal\/assert\/utils"\)/g, "assertUtilsModule"],
  [/require\("internal\/assert\/calltracker"\)/g, "callTrackerModule"],
  [/require\("(node:)?([a-z_/]+)"\)/g, 'host.requireBuiltin("node:$2")'],
  [/\$ERR_([A-Z_0-9]+)\(/g, "codes.$1("],
  [/\.\$call\(/g, ".call("],
  [/\.\$apply\(/g, ".apply("],
  [/\$isUndefinedOrNull\(([^()]+)\)/g, "($1) == null"],
];

const perFile = {
  "internal/assert/myers_diff": [
    [
      /\$rust\("node_assert_binding\.rs", "generate"\)/g,
      "{ myersDiff: host.myersDiff, printSimpleMyersDiff: host.printSimpleMyersDiff, printMyersDiff: host.printMyersDiff }",
    ],
  ],
  "node/assert": [
    [/\$newCppFunction\("NodeUtilTypesModule\.cpp", "jsFunctionPartialDeepStrictEqual", 2\)/g, "host.jsFunctionPartialDeepStrictEqual"],
    // O `export default assert;` do meio do arquivo vira o `return` do fim (o resto do módulo ainda monta o `assert`).
    [/^export default assert;\n/m, ""],
    // `assert.ok.name` é `ok2` no bun 1.4.2 (e `assert.name` continua `ok`): o empacotador renomeia a expressão
    // `Assert.prototype.ok = function ok` porque o `function ok` do módulo já tem o nome. O nome é visível ao programa.
    [
      /^Assert\.prototype\.ok = function ok\(\.\.\.args\) \{\n  innerOk\(ok, /m,
      "Assert.prototype.ok = function ok2(...args) {\n  innerOk(ok2, ",
    ],
  ],
};

function convert(name) {
  const file = path.join(root, name + ".ts");
  let code = transpiler.transformSync(fs.readFileSync(file, "utf8"));
  for (const [pattern, replacement] of [...(perFile[name] ?? []), ...common]) code = code.replace(pattern, replacement);
  // `export function nome` e afins viram declarações, e os nomes, o objeto devolvido; `export default` é o `return`.
  const names = [];
  code = code.replace(/^export (function|class|const|let|var) ([A-Za-z_$][\w$]*)/gm, (_, kind, id) => {
    names.push(id);
    return `${kind} ${id}`;
  });
  code = code.replace(/^export default /m, "return ");
  if (names.length) code += `\nreturn { ${names.join(", ")} };\n`;
  const left = code.match(/\$[A-Za-z_][A-Za-z_0-9]*/g);
  if (left) {
    console.error(`${name}: intrínseco sem tradução: ${[...new Set(left)].join(", ")}`);
    process.exit(1);
  }
  return code;
}

// Cada interno num escopo próprio: `const nome = (() => { ...; return exports; })();`.
function scoped(variable, name) {
  return `const ${variable} = (() => {\n${convert(name)}\n})();\n`;
}

let out = "// GERADO por `scripts/gen-node-assert.js` a partir do `assert.ts` e dos internos que ele usa, do bun 1.4.2. Não editar à mão.\n";
out += "(function (host) {\n";
out += '  "use strict";\n\n';
out += "  const { codes } = host.internal;\n";
out += scoped("myersDiffModule", "internal/assert/myers_diff");
out += scoped("assertionErrorModule", "internal/assert/assertion_error");
out += scoped("assertUtilsModule", "internal/assert/utils");
out += scoped("callTrackerModule", "internal/assert/calltracker");
out += convert("node/assert");
out += "\nreturn assert;\n})\n";
process.stdout.write(out);
