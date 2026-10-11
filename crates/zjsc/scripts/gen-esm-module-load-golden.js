// Gera tests/golden/esm_module_load_bun.tsv: o carregamento de `import` estático e `import()` dinâmico a partir de
// arquivos (relativo com sonda, pacote em `node_modules`, mensagens de falha, identidade por `realpath`) no bun 1.4.2.
// Monta a árvore `tree` numa pasta temporária, roda cada caso como `main.mjs` na raiz dela e grava o `log` que o
// programa monta com `L(x)`. O prefixo da pasta temporária sai das strings por `ROOT` (derivado de `import.meta.url`),
// então o golden não tem caminho da máquina. A mesma árvore está em `tree()` de `tests/esm_module_load.rs`, onde a
// raiz é `/app`.
// Colunas: nome, JSON(fonte de main.mjs), JSON(log).
// Uso: bun scripts/gen-esm-module-load-golden.js > tests/golden/esm_module_load_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";

const tree = {
  "a.mjs": "export const n = 1; export default 'A';",
  "b.js": "export const b = 2;",
  "dir/index.mjs": "export const d = 3;",
  "bad_static.mjs": "import './nope';",
  "bad_static_bare.mjs": "import 'nopkg';",
  "sub/s.mjs": "import { t } from './t'; export const s = t + 1; export const url = import.meta.url;",
  "sub/t.ts": "export const t = 10;",
  "sub/bare.mjs": "import { x } from 'pk'; export { x };",
  "node_modules/pk/package.json": '{"main":"i.js"}',
  "node_modules/pk/i.js": "export const x = 'pk-main';",
  "node_modules/pk/lib/l.js": "export const l = 'pk-lib';",
  "node_modules/@s/p/index.js": "export const sp = 'scoped';",
  "real/x.mjs": "export const url = import.meta.url;",
  "sp ace/x y.mjs": "export const url = import.meta.url;",
  "c.cjs": "module.exports = { n: 1 };",
  // `__ROOT__` vira a pasta temporária aqui e `/app` em tests/esm_module_load.rs.
  "file_static.mjs":
    "import { n } from 'file://__ROOT__/a.mjs?x=1'; import { url } from 'file://localhost__ROOT__/sp%20ace/x%20y.mjs#h'; export const r = [n, url];",
  "bad_file_static.mjs": "import 'file://__ROOT__/nope%20x.mjs?q#h';",
  "req_file.cjs":
    "const D = __dirname; const t = (s) => { try { return JSON.stringify(require(s)); } catch (e) { return e.message.split(D).join(''); } };\n" +
    "module.exports = [t('file://' + D + '/c.cjs'), t('file://' + D + '/c.cjs?x=1#h'), t('file://localhost' + D + '/c.cjs'), t('file://' + D + '/nope%20c.cjs'), t('file:./c.cjs')];",
};
const links = { ln: "real" };

const prelude =
  "globalThis.log = []; globalThis.L = (x) => { log.push(String(x)); };\n" +
  'const ROOT = import.meta.url.slice("file://".length, -"/main.mjs".length);\n' +
  "const fix = (v) => String(v).split(ROOT).join('');\n" +
  "const tryImport = async (s) => { try { const m = await import(s); return Object.keys(m).sort().join(','); } catch (e) { return fix(e.message); } };\n";

const cases = [
  ["dyn_relative_ext", "L(await tryImport('./a.mjs'))"],
  ["dyn_relative_noext", "L(await tryImport('./a'))"],
  ["dyn_relative_js", "L(await tryImport('./b'))"],
  ["dyn_relative_dir_index", "L(await tryImport('./dir'))"],
  ["dyn_relative_ts_rewrite", "L(await tryImport('./sub/t.js'))"],
  ["dyn_relative_missing", "L(await tryImport('./nope'))"],
  ["dyn_relative_missing_ext", "L(await tryImport('./nope.mjs'))"],
  ["dyn_bare_main", "L(await tryImport('pk'))"],
  ["dyn_bare_subpath", "L(await tryImport('pk/lib/l'))"],
  ["dyn_bare_scoped", "L(await tryImport('@s/p'))"],
  ["dyn_bare_missing", "L(await tryImport('nopkg'))"],
  ["dyn_bare_missing_subpath", "L(await tryImport('nopkg/sub/x'))"],
  ["dyn_bare_scoped_missing", "L(await tryImport('@s/nope'))"],
  ["static_missing_relative", "L(await tryImport('./bad_static.mjs'))"],
  ["static_missing_bare", "L(await tryImport('./bad_static_bare.mjs'))"],
  ["static_chain_relative_ts", "const m = await import('./sub/s.mjs'); L(m.s)"],
  ["static_bare_from_subdir", "const m = await import('./sub/bare.mjs'); L(m.x)"],
  ["same_instance", "const a = await import('./a.mjs'); const b = await import('./a'); L(a === b)"],
  ["import_meta_url", "const m = await import('./sub/s.mjs'); L(fix(m.url))"],
  ["symlink_identity", "const a = await import('./ln/x.mjs'); const b = await import('./real/x.mjs'); L([a === b, fix(a.url)])"],
  // Esquema `file:` (medido no bun 1.4.2): host ignorado, query e hash caem, `%XX` se desfaz, e só `file://` conta.
  ["file_dyn_absolute", "L(await tryImport('file://' + ROOT + '/a.mjs'))"],
  ["file_dyn_localhost", "L(await tryImport('file://localhost' + ROOT + '/a.mjs'))"],
  ["file_dyn_other_host", "L(await tryImport('file://example.com' + ROOT + '/a.mjs'))"],
  ["file_dyn_noext", "L(await tryImport('file://' + ROOT + '/a'))"],
  ["file_dyn_dir_index", "L(await tryImport('file://' + ROOT + '/dir'))"],
  ["file_dyn_trailing_slash", "L(await tryImport('file://' + ROOT + '/a.mjs/'))"],
  ["file_dyn_dot_segments", "L(await tryImport('file://' + ROOT + '/sub/../a.mjs'))"],
  ["file_dyn_percent_space", "L(await tryImport('file://' + ROOT + '/sp%20ace/x%20y.mjs'))"],
  ["file_dyn_percent_letter", "L(await tryImport('file://' + ROOT + '/%61.mjs'))"],
  ["file_dyn_percent_slash", "L(await tryImport('file://' + ROOT + '/sp%2Face/x.mjs'))"],
  ["file_dyn_percent_invalid", "L(await tryImport('file://' + ROOT + '/a%zz'))"],
  ["file_dyn_query", "L(await tryImport('file://' + ROOT + '/a.mjs?q=1'))"],
  ["file_dyn_hash", "L(await tryImport('file://' + ROOT + '/a.mjs#h'))"],
  ["file_dyn_query_hash_missing", "L(await tryImport('file://' + ROOT + '/nope%20x.mjs?q#h'))"],
  ["file_dyn_missing", "L(await tryImport('file://' + ROOT + '/nope.mjs'))"],
  ["file_dyn_root_only", "L(await tryImport('file:///'))"],
  ["file_dyn_relative_one_slash", "L(await tryImport('file:./a.mjs'))"],
  ["file_dyn_no_slashes", "L(await tryImport('file:a.mjs'))"],
  ["file_dyn_uppercase_scheme", "L(await tryImport('FILE://' + ROOT + '/a.mjs'))"],
  ["file_identity_query_hash", "const a = await import('file://' + ROOT + '/sp%20ace/x%20y.mjs'); const b = await import('file://' + ROOT + '/sp%20ace/x%20y.mjs?q=1'); const c = await import('./sp ace/x y.mjs'); const d = await import('file://' + ROOT + '/sp%20ace/x%20y.mjs#h'); L([a === b, a === c, a === d, fix(b.url)])"],
  ["file_meta_url_encoded", "const m = await import('file://' + ROOT + '/sp%20ace/x%20y.mjs'); L(fix(m.url))"],
  ["file_static", "const m = await import('./file_static.mjs'); L(m.r.map(fix))"],
  ["file_static_missing", "L(await tryImport('./bad_file_static.mjs'))"],
  ["file_require", "L(JSON.stringify((await import('./req_file.cjs')).default.map(fix)))"],
];

const root = fs.mkdtempSync(path.join(os.tmpdir(), "esm-load-"));
for (const [rel, text] of Object.entries(tree)) {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), text.split("__ROOT__").join(fs.realpathSync(root)));
}
for (const [link, target] of Object.entries(links)) fs.symlinkSync(target, path.join(root, link));

const real = fs.realpathSync(root);
for (const [name, body] of cases) {
  const source = prelude + body;
  fs.writeFileSync(path.join(root, "main.mjs"), source + "\nprocess.stdout.write(JSON.stringify(log));\n");
  const run = spawnSync(BUN, [path.join(root, "main.mjs")], { cwd: root, encoding: "utf8" });
  if (run.status !== 0) throw new Error(`${name}: ${run.stderr}`);
  if (run.stdout.includes(real) || run.stdout.includes(root)) throw new Error(`${name}: caminho da máquina no resultado`);
  process.stdout.write(`${name}\t${JSON.stringify(source)}\t${run.stdout}\n`);
}
fs.rmSync(root, { recursive: true, force: true });
