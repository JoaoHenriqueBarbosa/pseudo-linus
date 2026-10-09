// Gera tests/golden/global_var_decl_bun.tsv: declarações (`var`, `function`, `let`, `const`, `class`) de nomes que já
// são propriedades do objeto global (`globalThis`, `undefined`, `NaN`, `Infinity`, `Math`, `Array`, `eval`, `Object` e
// nomes do bun como `navigator`, `self`, `global`, `atob`), por script, por eval direto no topo e por eval indireto,
// medidas no bun 1.4.2. É a GlobalDeclarationInstantiation (CanDeclareGlobalVar/Function, CreateGlobalVarBinding,
// conflito de lexical com propriedade não configurável) vista de fora.
// Cada linha do golden tem quatro colunas: modo, nome, fonte, resultado.
//   modo `script`: a fonte seguida das sondas roda como um script (`vm.runInThisContext`, nunca arquivo, para o
//     transpilador do bun não tocar). Se a instanciação do script lançar, o resultado é `SCRIPTERR nome: mensagem`
//     (o script inteiro não roda, então não há sonda).
//   modo `eval`: `eval(fonte)` direto, escrito no topo de um script (o ambiente de variáveis é o global).
//   modo `indirect`: `(0, eval)(fonte)`.
//   Nos dois modos de eval um erro vira `ERR nome: mensagem` e as sondas rodam depois, fora do eval.
// Sondas: `typeof NOME` e o descritor de `NOME` no global (`tipo-do-valor|accessor,writable,enumerable,configurable`
// ou `none`). O resultado é `out.join('|')`.
// Os programas não usam API de host: só `out`. Um processo por programa. Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-global-var-decl-golden.js > tests/golden/global_var_decl_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Mesmos textos embutidos em tests/global_var_decl_bun_golden.rs.
const PRELUDE = `globalThis.out = [];
globalThis.__g = globalThis;
globalThis.__gopd = Object.getOwnPropertyDescriptor;
globalThis.__final = function () { return out.join('|'); };
`;
const PROBES = `out.push(typeof NAME);
out.push((function () { var d = __gopd(__g, "NAME"); return d ? ('value' in d ? typeof d.value : 'accessor') + ',' + d.writable + ',' + d.enumerable + ',' + d.configurable : 'none'; })());`;
const CATCH = `catch (e) { out.push('ERR ' + e.name + ': ' + e.message); }`;

function build(mode, name, source) {
  const probes = PROBES.replace(/NAME/g, name);
  if (mode === "script") return `${PRELUDE}${source}\n${probes}`;
  const call = mode === "eval" ? "eval" : "(0, eval)";
  return `${PRELUDE}try { ${call}(${JSON.stringify(source)}); } ${CATCH}\n${probes}`;
}

const declarations = {
  var: (n) => `var ${n} = 1;`,
  bare: (n) => `var ${n};`,
  func: (n) => `function ${n}() {}`,
  let: (n) => `let ${n} = 1;`,
  const: (n) => `const ${n} = 1;`,
  class: (n) => `class ${n} {}`,
};

const programs = [];
const add = (mode, name, kind) => programs.push({ mode, name, source: declarations[kind](name) });

for (const name of ["undefined", "NaN", "Infinity", "globalThis"]) {
  for (const kind of ["var", "func", "let", "class"]) add("script", name, kind);
  for (const kind of ["var", "func", "let"]) add("eval", name, kind);
  for (const kind of ["var", "func"]) add("indirect", name, kind);
}
for (const name of ["Math", "Array", "eval", "Object", "navigator", "self", "global", "atob"]) {
  add("script", name, "var");
  add("script", name, "let");
  add("indirect", name, "func");
}
// O valor depois da declaração ou da atribuição: `var` só declara (a propriedade global não gravável mantém o valor),
// a atribuição sloppy é ignorada em silêncio e a strict lança TypeError.
for (const name of ["NaN", "Infinity", "undefined"]) {
  const show = `out.push(String(${name}));`;
  add2("script", name, `var ${name} = 1; ${show}`);
  add2("script", name, `${name} = 1; ${show}`);
  add2("script", name, `"use strict"; ${name} = 1;`);
  add2("script", name, `(function () { "use strict"; try { ${name} = 1; } catch (e) { out.push(e.name + ': ' + e.message); } })();`);
  add2("script", name, `(function () { try { ${name} = 1; } catch (e) { out.push(e.name + ': ' + e.message); } ${show} })();`);
  add2("eval", name, `var ${name} = 1; ${show}`);
  add2("indirect", name, `var ${name} = 2; ${show}`);
}
function add2(mode, name, source) {
  programs.push({ mode, name, source });
}
for (const { source } of programs) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "global-var-decl-golden-"));
const driver = path.join(tmp, "driver.js");
fs.writeFileSync(
  driver,
  `const vm = require("node:vm");
const fs = require("node:fs");
const [mode, name, program] = [process.argv[2], process.argv[3], fs.readFileSync(process.argv[4], "utf8")];
process.on("unhandledRejection", () => {});
if (mode === "script") {
  const prelude = ${JSON.stringify(PRELUDE)};
  vm.runInThisContext(prelude, { filename: "prelude" });
  try {
    vm.runInThisContext(program, { filename: "program" });
    fs.writeSync(1, vm.runInThisContext("__final()"));
  } catch (e) {
    fs.writeSync(1, "SCRIPTERR " + e.name + ": " + e.message);
  }
} else {
  vm.runInThisContext(program, { filename: "program" });
  fs.writeSync(1, vm.runInThisContext("__final()"));
}
`
);
const lines = [];
programs.forEach(({ mode, name, source }, index) => {
  const file = path.join(tmp, `p${index}.txt`);
  // No modo script o prelúdio roda à parte; o arquivo leva só a fonte e as sondas.
  const full = build(mode, name, source);
  fs.writeFileSync(file, mode === "script" ? full.slice(PRELUDE.length) : full);
  const run = spawnSync(process.execPath, [driver, mode, name, file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  if (run.error || run.status !== 0 || run.stdout === "") {
    process.stderr.write(`FALHA: ${mode} ${name} ${source}\n${run.stderr}\n`);
    throw new Error("programa sem resultado do bun (timeout ou falha)");
  }
  lines.push(`${mode}\t${name}\t${source}\t${run.stdout.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
const output = lines.join("\n") + "\n";
if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
process.stdout.write(output);
process.stderr.write(`${programs.length} programas\n`);
