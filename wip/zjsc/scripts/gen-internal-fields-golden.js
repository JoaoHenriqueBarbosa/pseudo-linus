// Gera tests/golden/internal_fields_bun.tsv: casos que atravessam os campos internos (op_get_internal_field e
// op_put_internal_field) e os iteradores nativos: for-of sobre Map e Set dentro de comparador de sort, matchAll,
// Iterator.from, helpers de iterador, generators, iteradores de array e de string, async generators e async-from-sync,
// medidos no bun 1.4.2.
// Cada programa roda por `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do
// bun não tocar na fonte) num processo próprio, depois do prelúdio INTERNAL_FIELDS_HARNESS, também via vm. A fonte
// roda dentro de uma função e empilha em `out`; o golden é `out.join('|')` depois de esvaziar as microtarefas
// (o host do gerador drena com um setTimeout fora do programa), com `ERR nome: mensagem` empilhado se a fonte lançou.
// Os programas não usam API de host (setTimeout, process, console, require, Bun, URL, Buffer): só `out`.
// São 12 programas fixos, sem amostragem e sem descarte: nenhum é filtrado por repetir golden vizinho.
// Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-internal-fields-golden.js > tests/golden/internal_fields_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Mesmo texto embutido em tests/internal_fields_bun_golden.rs.
const HARNESS = `globalThis.out = [];
globalThis.__final = function () { return out.join('|'); };
globalThis.__run = function (source) {
  try { (0, eval)('(function () { ' + source + ' })()'); } catch (e) { out.push('ERR ' + e.name + ': ' + e.message); }
};`;

const programs = [];
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    programs.push(source);
  }
};

add(
  `var m = new Map([[3,'a'],[1,'b'],[2,'c']]); var s = new Set([30,10,20]); var r = [5,4,3,2,1].sort(function (a, b) { var t = ''; for (var [k, v] of m) t += k + v; for (var x of s) t += x; out.push(t.length); return a - b; }); out.push(r.join());`,
  `var m = new Map([[1,2],[3,4]]); var it = m.entries(); it.next(); out.push(JSON.stringify([...it])); var s = new Set('abc'); var si = s.values(); si.next(); out.push([...si].join()); out.push(String(m.keys()) + String(s.entries()));`,
  `var r = []; for (var x of 'a1b22c'.matchAll(/\\d+/g)) r.push(x[0] + '@' + x.index); out.push(r.join()); out.push(String('ab'.matchAll(/a/g))); var mi = 'aaa'.matchAll(/a/g); mi.next(); out.push([...mi].length);`,
  `var it = Iterator.from({ next() { return { done: false, value: 7 }; } }); out.push(String(it)); out.push(it.take(3).toArray().join()); out.push(Iterator.from([1,2,3]).map(function (x) { return x * 2; }).drop(1).toArray().join()); var w = Iterator.from({ next() { return { done: true }; } }); out.push(JSON.stringify(w.next()));`,
  `var h = [1,2,3,4].values().map(function (x) { return x * 2; }).filter(function (x) { return x > 2; }); out.push(h.next().value); out.push(h.toArray().join()); out.push(String(h)); var f = [1,2].values().flatMap(function (x) { return [x, x]; }); out.push(f.toArray().join()); var c = [1,2,3].values().map(function (x) { return x; }); c.next(); out.push(JSON.stringify(c.return())); out.push(JSON.stringify(c.next()));`,
  `function* g() { var x = yield 1; try { yield x * 2; } finally { out.push('fin'); } return 9; } var gi = g(); out.push(JSON.stringify([gi.next(), gi.next(5), gi.return(4), gi.next()])); var g2 = g(); g2.next(); try { g2.throw(new Error('boom')); } catch (e) { out.push(e.message); } out.push(String(g()));`,
  `var ai = [1,2,3][Symbol.iterator](); ai.next(); out.push(JSON.stringify([ai.next(), [...ai], ai.next()])); var ki = [7,8].entries(); out.push(JSON.stringify([...ki])); out.push(String(ai)); var ti = new Uint8Array([4,5]).keys(); out.push([...ti].join());`,
  `var si = 'aé😀'[Symbol.iterator](); out.push(si.next().value); out.push([...si].join('|')); out.push(String(si));`,
  `function* inner() { yield 1; yield 2; return 'r'; } function* outer() { var v = yield* inner(); yield v; } out.push([...outer()].join()); var p = new Proxy({a: 1}, { get(t, k) { return String(k) + '!'; } }); out.push(p.zz);`,
  `async function* ag() { try { yield 1; yield 2; } finally { out.push('afin'); } } (async function () { var acc = []; for await (var x of ag()) { acc.push(x); } for await (var y of [Promise.resolve(10), 11]) acc.push(y); for await (var z of new Set([Promise.resolve(20)])) acc.push(z); out.push('async:' + acc.join()); var it = ag(); await it.next(); out.push(JSON.stringify(await it.return(5))); out.push(String(it)); })();`,
  `var a = { [Symbol.iterator]() { var i = 0; return { next() { return { value: i++, done: i > 3 }; }, return() { out.push('closed'); return {}; } }; } }; (async function () { for await (var x of a) { out.push(x); if (x == 1) break; } try { for await (var q of { [Symbol.iterator]() { return { next() { return { value: Promise.reject(new Error('rej')), done: false }; }, return() { out.push('ret'); return {}; } }; } }) {} } catch (e) { out.push(e.message); } })();`,
  `async function f() { return await new Promise(function (r) { r(3); }); } f().then(function (v) { out.push('p' + v); }); Promise.all([1, Promise.resolve(2)]).then(function (v) { out.push(v.join()); }); Promise.allSettled([Promise.reject(1)]).then(function (v) { out.push(v[0].status); });`
);

// Executa cada programa no bun, num processo próprio, pela API vm (a fonte nunca é um arquivo do projeto).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "internal-fields-golden-"));
const driver = path.join(tmp, "driver.js");
fs.writeFileSync(
  driver,
  `const vm = require("node:vm");
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.on("unhandledRejection", () => {});
vm.runInThisContext(fs.readFileSync(process.argv[3], "utf8"), { filename: "harness" });
globalThis.__run(source);
setTimeout(() => { const out = globalThis.__final(); process.stdout.write(out); }, 0);
`
);
const harnessFile = path.join(tmp, "harness.txt");
fs.writeFileSync(harnessFile, HARNESS);
const lines = [];
programs.forEach((source, index) => {
  const srcFile = path.join(tmp, `p${index}.txt`);
  fs.writeFileSync(srcFile, source);
  const run = spawnSync(process.execPath, [driver, srcFile, harnessFile], { timeout: 10000, encoding: "utf8", cwd: tmp });
  const result = run.stdout;
  if (run.error || run.status !== 0 || result === "") {
    process.stderr.write(`FALHA: ${source}\n${run.stderr}\n`);
    throw new Error("programa sem resultado do bun (timeout ou falha)");
  }
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
const output = lines.join("\n") + "\n";
if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
process.stdout.write(output);
process.stderr.write(`${programs.length} programas\n`);
