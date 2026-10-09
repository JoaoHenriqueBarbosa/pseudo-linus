// Gera tests/golden/tailcall_bun.tsv: recursão de 1e6 níveis em posição de cauda medida no bun 1.4.2.
// Colunas: nome do caso, fonte do programa (JSON), o valor de `R` (JSON) e a meta (modo e posições). Os nomes `esm_*`
// são os que o bun roda como ESM (sem diretiva e sem marcador de CJS o `.js` é ESM, logo estrito), medido em
// wip/notes/tailcall-sloppy.md. Os `sloppy_*` são autênticos: `.cjs` sem diretiva (`prepareProgram(original, true)`),
// onde o bun dá RangeError em todos (não há `op_tail_call` em sloppy). O programa termina com `globalThis.R = R` para
// o resultado sair do módulo CJS (ver o comentário no laço).
// Uso: bun scripts/gen-tailcall-golden.js > tests/golden/tailcall_bun.tsv
const { spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { prepareProgram } = require("./golden-prelude.js");

const bodies = {
  self: "function f(n){return n?f(n-1):0}",
  mutual: "function f(n){return n?h(n-1):0}function h(n){return n?f(n-1):0}",
  bound: "var g=function(n){return n?g(n-1):0}.bind(null);function f(n){return g(n)}",
  bound_args: "var g=function(a,n){return n?g(n-1):a}.bind(null,7);function f(n){return g(n)}",
  reflect: "function f(n){return n?Reflect.apply(f,this,[n-1]):0}",
  call: "function f(n){return n?f.call(this,n-1):0}",
  apply: "function f(n){return n?f.apply(this,[n-1]):0}",
  arrow: "var f=n=>n?f(n-1):0",
  try_finally: "function f(n){try{return n?f(n-1):0}finally{}}",
  construct: "function F(n){this.v=n?new F(n-1):0}function f(n){return new F(n).v===0?0:1}",
  non_tail: "function f(n){return n?1+f(n-1):0}",
};

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tailcall-"));
const quote = (text) => JSON.stringify(text);
for (const mode of ["strict", "esm", "sloppy"]) {
  for (const [name, body] of Object.entries(bodies)) {
    const directive = mode === "strict" ? '"use strict";\n' : "";
    // `globalThis.R = R` no fim: nos casos `strict_*` e `sloppy_*` o bun roda um módulo CJS, onde `var R` é local do
    // invólucro e o harness do porte (que lê `globalThis.R`) não o enxerga; os outros goldens CJS gravam `globalThis.R`.
    const original = directive + body + ";var R;try{R=String(f(1e6))}catch(e){R=e.name};globalThis.R=R";
    // O programa gravado é o fonte já transpilado pelo bun; o que o bun executa é `executableSource(original)` (a quarta
    // coluna leva o modo e o mapa de posições, ver golden-prelude.js). `canonicalSource` preserva a diretiva "use strict"
    // do topo; se o bun não parseia, fica o original.
    const { source, executable, meta, file_extension } = prepareProgram(original, mode === "sloppy");
    const file = path.join(dir, "case" + file_extension);
    fs.writeFileSync(file, executable + ";console.log(R)");
    const result = spawnSync(process.execPath, [file], { encoding: "utf8" });
    const value = result.stdout.trim();
    if (!value) continue;
    process.stdout.write(`${mode}_${name}\t${quote(source)}\t${quote(value)}${meta ? "\t" + JSON.stringify(meta) : ""}\n`);
  }
}
fs.rmSync(dir, { recursive: true });
