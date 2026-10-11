// Gera tests/golden/global_declaration_bun.tsv: declarações globais em eval indireto depois de
// `Object.preventExtensions/seal/freeze(globalThis)`, medidas no bun 1.4.2. Cobre `canDeclareGlobalVar`,
// `canDeclareGlobalFunction` (TypeError com a mensagem exata), let/const/class (que moram no ambiente lexical e passam),
// atribuição implícita sloppy (cria a global) e strict (ReferenceError), `globalThis.x = 1` e `defineProperty`.
// Colunas: a fonte do programa (JSON) e o valor da global `R` (JSON). `R` nasce antes do congelamento.
// Uso: bun scripts/gen-global-declaration-golden.js > tests/golden/global_declaration_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// `freeze` fica fora: congela também o `R` (e o `process` do hospedeiro), então o resultado não sai; o efeito sobre as
// declarações é o do `seal` e os atributos de `Array` estão nos testes unitários de `can_declare_global_function`.
const modes = ["preventExtensions", "seal"];
const sources = [
  "var x1 = 1",
  "var x1",
  "function f1() {}",
  "let l1 = 1",
  "const c1 = 1",
  "class C1 {}",
  "zz1 = 1",
  '"use strict"; zz2 = 1',
  "globalThis.gx1 = 1",
  '"use strict"; globalThis.gx1 = 1',
  'Object.defineProperty(globalThis, "dp1", { value: 1 })',
  "var undefined",
  "var NaN",
  "function NaN() {}",
  "var Array",
  "function Array() {}",
  "var x1; function f1() {}",
  "if (true) { function f2() {} }",
  "{ function f3() {} }",
  "var x1 = 1; let l2 = 2",
  "let undefined",
];

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "gdecl-"));
const file = path.join(dir, "p.js");
let kept = 0;
for (const mode of modes) {
  for (const src of sources) {
    const program =
      `globalThis.R = ''; Object.${mode}(globalThis);\n` +
      `try { (0, eval)(${JSON.stringify(src)}); globalThis.R = 'ok' } catch (e) { globalThis.R = e.name + ': ' + e.message }`;
    // O bun imprime `R` no fim: a global não aceita propriedade nova depois do congelamento, mas `R` já existe.
    fs.writeFileSync(file, program + "\nprocess.stdout.write('\\u0001' + JSON.stringify(globalThis.R))\n");
    const run = spawnSync(process.execPath, [file], { encoding: "utf8", cwd: dir, timeout: 10000 });
    const marked = (run.stdout || "").split("\u0001")[1];
    if (marked === undefined) {
      process.stderr.write("sem resultado para: " + JSON.stringify(program) + "\n");
      continue;
    }
    kept++;
    process.stdout.write(JSON.stringify(program) + "\t" + marked + "\n");
  }
}
process.stderr.write(`mantidos ${kept}\n`);
fs.rmSync(dir, { recursive: true, force: true });
