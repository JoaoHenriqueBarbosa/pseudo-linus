// Gera tests/golden/build_message_bun.tsv: o que o `BuildMessage` do bun expõe (position, line, column, level, notes,
// toJSON) para o arquivo que o transpilador recusa, sozinho (`BuildMessage`) ou com vários erros (`AggregateError`).
// Mesmo formato e mesmo runner dos goldens de módulo.
// Uso: RUNS=3 bun scripts/gen-build-message-golden.js > tests/golden/build_message_bun.tsv
const cases = [];
const seen = new Set();
const add = (files) => {
  const key = JSON.stringify(files);
  if (seen.has(key)) return;
  seen.add(key);
  cases.push(files);
};

// O resumo de um BuildMessage: os acessores, a identidade do que o bun guarda em cache, toJSON e toString.
const DUMP = `
const one = (m, depth) => {
  const out = {
    ctor: m.constructor.name, name: m.name, message: m.message, level: m.level, line: m.line, column: m.column,
    position: m.position, same_position: m.position === m.position, same_notes: m.notes === m.notes,
    json: JSON.stringify(m), str: String(m), keys: Object.keys(m).join(),
  };
  if (depth === 0) out.notes = m.notes.map((n) => one(n, 1));
  else out.notes_len = m.notes.length;
  return out;
};
const dump = (e) => { L(e.constructor.name + ': ' + e.message); for (const m of (e.errors || [e])) L(JSON.stringify(one(m, 0))); };
`;
const bad = (name, source) =>
  add({ "main.mjs": DUMP + `try { await import('./${name}'); } catch (e) { dump(e); }`, [name]: source });

const sources = {
  "ident.mjs": "const = 1\n",
  "redecl.mjs": "let a = 1;\nlet a = 2;\n",
  "constassign.mjs": "const c = 1;\nc = 2;\n",
  "unterminated.mjs": 'var s = "abc\nvar t = 1;\n',
  "eof.mjs": "if (x) {\n  y();\n",
  "paren.mjs": "function f() {\n  return (1 + ;\n}\n",
  "tabs.mjs": "function f() {\n\tif (a) {\n\t\tvar x = ;\n\t}\n}\n",
  "unicode.mjs": 'var é = "ü" +;\n',
  "crlf.mjs": "let a = 1;\r\nlet a = 2;\r\n",
  "late.mjs": "var a = 1;\nvar b = 2;\nvar c = 3;\nvar d = (;\n",
  "exportdup.mjs": "export const x = 1;\nexport const x = 2;\n",
  "importdup.mjs": "import { a } from './x.mjs';\nimport { a } from './y.mjs';\n",
  "classdup.mjs": "class A { constructor() {} constructor() {} }\n",
  "regexp.mjs": "var r = /(/;\n",
  "template.mjs": "var t = `abc ${;\n",
  "bad.ts": "enum E { A = }\n",
  "badtype.ts": "let x: number = ;\n",
  "ts_redecl.ts": "let a: number = 1;\nlet a: string = '';\n",
  "bad.tsx": "const x = <div>;\n",
  "bad.jsx": "const x = <div></span>;\n",
  "badjsxattr.jsx": "const x = <div a=>x</div>;\n",
};
for (const [name, source] of Object.entries(sources)) bad(name, source);

const RUNNER = `
globalThis.log = [];
globalThis.L = (x) => { log.push(String(x)); };
const [dir] = process.argv.slice(2);
let error = null;
try { await import(dir + "/main.mjs"); } catch (e) {
  error = e instanceof Error ? e.name + ": " + e.message : "throw " + String(e);
}
for (let i = 0; i < 50; i++) await Promise.resolve();
const clean = (s) => s.split(dir + "/").join("");
console.log(clean(JSON.stringify({ log, error })));
`;

require("./module-golden-run.js").runModuleGolden("build-message-golden", RUNNER, cases);
