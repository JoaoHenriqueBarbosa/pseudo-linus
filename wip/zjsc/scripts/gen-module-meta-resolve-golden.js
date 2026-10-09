// Gera tests/golden/module_meta_resolve_bun.tsv: golden de `import.meta.resolve` e `import.meta.resolveSync`
// avaliado no bun 1.4.2. Mesmo formato e mesmo runner dos goldens de módulo (mapa de arquivos em JSON, `main.mjs`
// de entrada, saída `{"log":[...],"error":...}`), mas sem o filtro de `host-api.js`: aqui o `import.meta` do host
// é justamente o que se mede. Só entram resoluções que não dependem da máquina (caminho relativo ou absoluto,
// `node:`, `file:`); falha de resolução registra só que lançou, porque a mensagem é da camada do Bun.
// Medido: `resolve` devolve a URL sem conferir se o arquivo existe e sem sondar extensão; `resolveSync` devolve o
// caminho e lança se não achar; ambas têm `length` 0.
// Uso: bun scripts/gen-module-meta-resolve-golden.js > tests/golden/module_meta_resolve_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";
const cases = [];
const seen = new Set();
const add = (files) => {
  const key = JSON.stringify(files);
  if (seen.has(key)) return;
  seen.add(key);
  cases.push(files);
};

const T = (expr) => `try { L(JSON.stringify(${expr})); } catch (e) { L('threw'); }`;
const specs = [
  "./a.mjs", "./nope.mjs", "./a", "../a.mjs", "./s/b.mjs", "./s/../a.mjs", "./s/./b.mjs", "/abs/a.mjs",
  "/a.mjs", "file:///abs/a.mjs", "node:fs", "node:path", ".", "./", "", "./a.mjs?x=1", "./a.mjs#h",
];
const files = { "a.mjs": "export {};", "s/b.mjs": "export {};" };
// `resolveSync` do Bun sonda extensão (`./a` acha `a.mjs`) e aceita query; o host em memória não sonda, então
// esses especificadores só entram para `resolve` (que não sonda nem confere).
const probing = new Set(["./a", "./a.mjs?x=1", "./a.mjs#h"]);
for (const spec of specs) {
  for (const fn of ["resolve", "resolveSync"]) {
    if (fn === "resolveSync" && probing.has(spec)) continue;
    // "../" a partir da raiz do caso sairia do diretório temporário e vazaria o caminho da máquina
    if (!spec.startsWith("../")) add({ "main.mjs": T(`import.meta.${fn}(${JSON.stringify(spec)})`), ...files });
    // de dentro de um módulo em subdiretório: o referrer é o módulo dono do `import.meta`
    add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": T(`import.meta.${fn}(${JSON.stringify(spec)})`), ...files });
  }
}
for (const fn of ["resolve", "resolveSync"]) {
  add({ "main.mjs": `L(typeof import.meta.${fn}); L(import.meta.${fn}.name); L(import.meta.${fn}.length); L('${fn}' in import.meta);` });
  add({ "main.mjs": T(`import.meta.${fn}()`) });
  add({ "main.mjs": `L(Object.keys(import.meta).join()); L(JSON.stringify(import.meta));` });
  add({ "main.mjs": `import { r } from './s/c.mjs'; L(r); L(import.meta.${fn}('./a.mjs'));`, "s/c.mjs": `export const r = import.meta.${fn}('./a.mjs');`, ...files });
}

// Chamada solta e `this` trocado: `resolve` exige o `import.meta` (TypeError com mensagem fixa); `resolveSync`
// recusa só `this` que não é célula. Registra nome e mensagem do erro, que não dependem da máquina.
const TE = (expr) => `try { L(JSON.stringify(${expr})); } catch (e) { L(e.name + ': ' + e.message); }`;
add({ "main.mjs": `const r = import.meta.resolve; ${TE(`r("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; ${TE(`r("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const o = { resolve: import.meta.resolve }; ${TE(`o.resolve("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const o = { resolveSync: import.meta.resolveSync }; ${TE(`o.resolveSync("./a.mjs")`)}`, ...files });
// `.call(undefined)` lança em `resolveSync`. A chamada solta `r()` funciona quando `r` é uma variável de escopo
// (capturada por closure, ou de módulo): o bytecode entrega o objeto de escopo como `this`, que é célula. Com `r`
// em registrador local (não capturada) o `this` é `undefined` e lança, como `(0, f)()`, `apply`, `bind` e
// `Reflect.apply` com `undefined`.
add({ "main.mjs": `const r = import.meta.resolveSync; const keep = () => r; ${TE(`r("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolve; const keep = () => r; ${TE(`r("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; const keep = () => r; ${TE(`(0, r)("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; const keep = () => r; ${TE(`r.apply(undefined, ["./a.mjs"])`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; ${TE(`r.bind(undefined)("./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; ${TE(`Reflect.apply(r, undefined, ["./a.mjs"])`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; ${TE(`Reflect.apply(r, globalThis, ["./a.mjs"])`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; ${TE(`r.call(globalThis, "./a.mjs")`)}`, ...files });
add({ "main.mjs": `const r = import.meta.resolveSync; const f = () => r("./a.mjs"); ${TE(`f()`)}`, ...files });
add({ "main.mjs": `const o = { r: import.meta.resolveSync }; ${TE(`o.r("./a.mjs")`)}`, ...files });
for (const self of ["undefined", "{}", "null", "1", "'x'", "import.meta", "Object.create(import.meta)"]) {
  for (const fn of ["resolve", "resolveSync"]) {
    add({ "main.mjs": `${TE(`import.meta.${fn}.call(${self}, "./a.mjs")`)}`, ...files });
  }
}
add({ "main.mjs": `const r = import.meta.resolve.bind(import.meta); ${TE(`r("./a.mjs")`)}`, ...files });
add({ "main.mjs": `${TE(`new import.meta.resolve("./a.mjs")`)}`, ...files });

// Segundo argumento (parent) de `resolve`: string vira a origem (sem `file://`, enraizada em `/`, só o
// diretório conta); outro tipo é ignorado. `resolveSync` resolve sempre contra o chamador.
const parents = [
  "./s/b.mjs", "./s/", "./s", "s/x.mjs", "/s/x.mjs", "/x/y/z.mjs", "file:///s/x.mjs", "file:///s/", "", "./s/../x.mjs",
];
const nonStrings = ["undefined", "null", "5", "{}", "true"];
const parentSpecs = ["./a.mjs", "./b.mjs", "../a.mjs", "/abs/a.mjs", "node:fs", "./s/b.mjs"];
for (const spec of parentSpecs) {
  for (const parent of parents) {
    add({ "main.mjs": T(`import.meta.resolve(${JSON.stringify(spec)}, ${JSON.stringify(parent)})`), ...files });
  }
  for (const parent of nonStrings) {
    // "../" a partir da raiz do caso sairia do diretório temporário (o parent ignorado cai no chamador)
    if (!spec.startsWith("../")) add({ "main.mjs": T(`import.meta.resolve(${JSON.stringify(spec)}, ${parent})`), ...files });
    add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": T(`import.meta.resolve(${JSON.stringify(spec)}, ${parent})`), ...files });
  }
}
for (const parent of ["./s/b.mjs", "./s/", "file:///s/x.mjs", "/x/y/z.mjs"]) {
  add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": T(`import.meta.resolve("./b.mjs", ${JSON.stringify(parent)})`), ...files });
  add({ "main.mjs": T(`import.meta.resolveSync("./a.mjs", ${JSON.stringify(parent)})`), ...files });
  add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": T(`import.meta.resolveSync("./b.mjs", ${JSON.stringify(parent)})`), ...files });
}

// Barras duplas no parent e no especificador: o bun não colapsa (`http://h/p/q.mjs` dá `file:///http://h/p/b.mjs`),
// e um `..` consome também um segmento vazio. `.` ou `..` no fim dão a barra final.
const slashParents = ["http://h/p/q.mjs", "http://h/p/", "a//b/c.mjs", "//x/c.mjs", "/a///b/c.mjs", "a//b/"];
const slashSpecs = ["./b.mjs", "../b.mjs", "../../b.mjs", "../../../b.mjs", "../../../../b.mjs", "./c//d.mjs", "./c/../d.mjs", "/f//g.mjs", "./", "../"];
for (const parent of slashParents) {
  for (const spec of slashSpecs) add({ "main.mjs": T(`import.meta.resolve(${JSON.stringify(spec)}, ${JSON.stringify(parent)})`), ...files });
}
for (const spec of ["./s//a.mjs", "./s///a.mjs", ".//a.mjs", "./s/..//a.mjs", "/x//y.mjs", "./s//", "./s/.", "./s/..", "./s/../.", "./s/./.", "/x/.", "/x/..", "./a.mjs/.", "./a.mjs/..", "./."]) {
  add({ "main.mjs": T(`import.meta.resolve(${JSON.stringify(spec)})`), ...files });
  add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": T(`import.meta.resolve(${JSON.stringify(spec)})`), ...files });
}

// Falha de resolução: `ResolveMessage` com a mensagem, o código, o specifier e o referrer do bun. `TM` registra
// tudo que o bun define (o caminho do diretório temporário sai pela limpeza do runner).
const TM = (expr) => `try { L(JSON.stringify(${expr})); } catch (e) { L([e.name, e.constructor.name, e.message, e.code, e.specifier, e.referrer, e instanceof Error, Object.keys(e).length].join('|')); }`;
const failSpecs = ["./nope.mjs", "nopkg", "nopkg/x", "@s/nopkg/x", "node:nope", "/abs/nope.mjs", "", ".", "..", "./s/nope.mjs", "./nope.mjs?q=1", "../x/../nope.mjs"];
for (const spec of failSpecs) {
  const sync = `import.meta.resolveSync(${JSON.stringify(spec)})`;
  if (!spec.startsWith("../")) add({ "main.mjs": TM(sync), ...files });
  add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": TM(sync), ...files });
  if (spec !== "./nope.mjs" && spec !== "./s/nope.mjs" && !spec.startsWith("/abs") && !spec.startsWith("./nope")) {
    if (!spec.startsWith("../")) add({ "main.mjs": TM(`import.meta.resolve(${JSON.stringify(spec)})`), ...files });
  }
}
const failParents = ["/q/z.mjs", "q/z.mjs", "file:///q/z.mjs", "./q", "", "/", "http://h/p/q.mjs"];
for (const parent of failParents) {
  for (const spec of ["./nope.mjs", "nopkg/x", "node:nope"]) {
    add({ "main.mjs": TM(`import.meta.resolveSync(${JSON.stringify(spec)}, ${JSON.stringify(parent)})`), ...files });
    if (spec !== "node:nope") add({ "main.mjs": TM(`import.meta.resolve(${JSON.stringify(spec)}, ${JSON.stringify(parent)})`), ...files });
  }
}
// `resolveSync` com argumento não string cita `undefined` na mensagem; `resolve` cita o chamador.
for (const parent of ["undefined", "null", "5", "{}", "true"]) {
  add({ "main.mjs": TM(`import.meta.resolveSync("./nope.mjs", ${parent})`), ...files });
  add({ "main.mjs": TM(`import.meta.resolve("nopkg", ${parent})`), ...files });
}

// Especificador `//x`: o bun o lê como host (`resolve` dá `file://x/`; `resolveSync` devolve o texto como veio, sem
// conferir). `\x` e `\\x` são caminho que não existe (`Cannot find module`), `a\b` é pacote.
const hostSpecs = [
  "//x", "//x/y", "///x", "//", "///", "////x", "//x/../y", "//x/", "//x/./y", "//x//y", "//x/..", "//x/y/../..",
  "//x/y/../../..", "//nope.mjs", "//x/y.mjs", "\\x", "\\\\x", "\\\\", "a\\b", "x\\", "C:\\x", "\\\\x\\y",
];
for (const spec of hostSpecs) {
  for (const fn of ["resolve", "resolveSync"]) {
    add({ "main.mjs": TM(`import.meta.${fn}(${JSON.stringify(spec)})`), ...files });
    add({ "main.mjs": `import './s/c.mjs';`, "s/c.mjs": TM(`import.meta.${fn}(${JSON.stringify(spec)})`), ...files });
  }
  add({ "main.mjs": TM(`import.meta.resolve(${JSON.stringify(spec)}, "./s/b.mjs")`), ...files });
}

const RUNNER = `
globalThis.log = [];
globalThis.L = (x) => { log.push(String(x)); };
const [dir] = process.argv.slice(2);
let error = null;
try { await import(dir + "/main.mjs"); } catch (e) {
  error = e instanceof Error ? e.name + ": " + e.message : "throw " + String(e);
}
for (let i = 0; i < 50; i++) await Promise.resolve();
const clean = (s) => s.split("file://" + dir + "/").join("file:///").split(dir + "/").join("/");
console.log(clean(JSON.stringify({ log, error })));
`;

// `resolveSync("")` lança `TypeError` com `code` (protótipo por código, `code` herdado): ver src/runtime/node_error.rs.
add({
  "main.mjs":
    "const f = (n) => n !== 'sourceURL'; let e; try { import.meta.resolveSync('') } catch (x) { e = x } const p = Object.getPrototypeOf(e);\n" +
    "L(JSON.stringify([e.constructor.name, String(e), e.code, Object.getOwnPropertyNames(e).filter(f), Object.keys(e), Object.hasOwn(e, 'code'), 'code' in e, p === TypeError.prototype, Object.getPrototypeOf(p) === TypeError.prototype, Object.getOwnPropertyNames(p), String(e.stack).split('\\n')[0]]));",
});

const lines = [];
const failures = [];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "zjsc-module-meta-resolve-golden-"));
const runner = path.join(tmp, "runner.mjs");
fs.writeFileSync(runner, RUNNER);
cases.forEach((caseFiles, i) => {
  const dir = path.join(tmp, "case" + i);
  for (const [name, source] of Object.entries(caseFiles)) {
    const file = path.join(dir, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, source);
  }
  const r = spawnSync(BUN, [runner, dir], { encoding: "utf8", timeout: 10000, cwd: tmp });
  let out = (r.stdout || "").trim().split("\n").pop() || "";
  if (r.error || r.status === null) out = JSON.stringify({ log: [], error: "timeout" });
  else if (!out.startsWith("{")) out = JSON.stringify({ log: [], error: "exit " + r.status });
  const clean = JSON.stringify(caseFiles);
  if (/[\t\n\r]/.test(clean) || /[\t\n\r]/.test(out)) throw new Error("tab ou quebra de linha no caso " + i);
  if (out.includes(tmp)) failures.push(i);
  lines.push(clean + "\t" + out);
});
fs.rmSync(tmp, { recursive: true, force: true });
if (failures.length) throw new Error("caminho da máquina na saída dos casos " + failures.join(","));
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`casos: ${lines.length}\n`);
