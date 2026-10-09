// Gera tests/golden/delete_bun.tsv: `delete` de identificador medido no bun 1.4.2 (binding de var, parâmetro, let,
// const, função, arguments, var criado por eval sloppy, strict mode dentro de eval/Function, with, catch).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Cada programa é sloppy e captura a exceção em `R` como "Nome: mensagem" (os SyntaxError de strict mode saem do parser).
// Uso: bun scripts/gen-delete-golden.js > tests/golden/delete_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const bodies = [
  // var criado por eval sloppy é deletável e some do escopo
  "R = eval('var a=1; delete a')",
  "eval('var b=1; delete b'); R = typeof b",
  "R = eval('var a=1; delete a; typeof a')",
  "R = (function(){ eval('var q=1'); return delete q })()",
  "R = (function(){ eval('var q=1'); delete q; return typeof q })()",
  "R = (function(){ eval('var q=1'); return [delete q, delete q] })().join()",
  "R = (function(){ eval('function q(){}'); return delete q })()",
  "R = (function(p){ eval('var p=2'); return delete p })(1)",
  "R = eval('var ev=1; delete ev')",
  "R = eval('var ev2=1; delete ev2; typeof ev2')",
  "R = (0, eval)('var ge=1; delete ge')",
  "R = (0, eval)('var ge2=1; delete ge2; typeof ge2')",
  // strict mode: SyntaxError do parser
  "R = eval('\"use strict\"; var a; delete a')",
  "R = (function(){ 'use strict'; return eval('var a; delete a') })()",
  "R = (function(){ return eval('\"use strict\"; delete a') })()",
  "R = new Function('\"use strict\"; delete x')",
  "R = new Function('\"use strict\"; delete (x)')",
  "R = new Function('\"use strict\"; delete ((x))')",
  "R = eval('class C { m(){ return eval(\"delete zz\") } }; new C().m()')",
  "R = eval('class C { m(){ delete zz } }')",
  // bindings declarados: não deletáveis
  "R = (function(){ var v=1; return delete v })()",
  "R = (function(p){ return delete p })(1)",
  "R = (function(){ let l=1; return delete l })()",
  "R = (function(){ const c=1; return delete c })()",
  "R = (function(){ function k(){} return delete k })()",
  "R = (function(){ return delete arguments })()",
  "R = (function(){ var z=1; eval('delete z'); return typeof z })()",
  "R = (function(){ let l=1; return eval('delete l') })()",
  "R = (function(p){ return eval('delete p') })(1)",
  "R = (()=>{ var a = 1; return eval('delete a') })()",
  "R = (function(){ try { throw 1 } catch (e) { return delete e } })()",
  "R = (function f(){ return delete f })()",
  "R = (function(){ function f(){ return delete f } return f() })()",
  // global
  "globalThis.gx = 1; R = delete gx",
  "R = delete undefinedName",
  "let tl = 1; R = delete tl",
  "R = (function(){ with({w:1}){ return delete w } })()",
];

const programs = bodies.map(body => `try { ${body} } catch (e) { R = e.name + ": " + e.message }`);

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "delete-golden-"));
const file = path.join(dir, "delete_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  // O arquivo do bun é tratado como estrito: o programa roda por eval indireto, que é código global sloppy.
  fs.writeFileSync(file, `(0, eval)(${JSON.stringify(source)})`);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir });
  const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
