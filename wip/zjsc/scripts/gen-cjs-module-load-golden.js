// Gera tests/golden/cjs_module_load_bun.tsv: o `require` do CommonJS carregando arquivos (relativo, `node_modules`,
// `require.cache`, `module.id/filename/loaded/children/parent`) no bun 1.4.2. Monta a árvore `tree` numa pasta temporária,
// roda cada caso como `main.js` na raiz dela e grava o `R` que o programa calcula. O prefixo da pasta temporária sai das
// strings por `ROOT` (derivado de `module.filename`), então o golden não tem caminho da máquina. A mesma árvore está em
// `tree()` de `tests/cjs_module_load.rs`, onde a raiz é `/` e o arquivo principal é `/main.js`.
// Colunas: nome, JSON(fonte de main.js), JSON(resultado).
// Uso: bun scripts/gen-cjs-module-load-golden.js > tests/golden/cjs_module_load_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";

const tree = {
  "a.js":
    "exports.n=1;exports.id=module.id;exports.filename=module.filename;exports.loaded=module.loaded;exports.path=module.path;" +
    "exports.parentId=module.parent&&module.parent.id;exports.keys=Object.keys(module);exports.children=module.children.length;" +
    "exports.paths0=module.paths[0];",
  "node_modules/pk/package.json": '{"main":"i.js"}',
  "node_modules/pk/i.js": "exports.x=2;exports.id=module.id;exports.parentId=module.parent.id;",
  "c1.js": "exports.a=1;const o=require('./c2');exports.b=o.seen;",
  "c2.js": "const p=require('./c1');exports.seen=JSON.stringify(p);",
  "d.json": '{"k":[1,2]}',
  "thrower.js": "throw new Error('boom');",
  "sub/s.js": "exports.t=require('./t').v;exports.parentFile=module.parent.id;exports.res=require.resolve('./t');",
  "sub/t.js": "exports.v=7;",
  "real/x.js": "exports.id=module.id;",
};
const links = { ln: "real" };

const prelude =
  'const ROOT=module.filename.slice(0,-"/main.js".length);' +
  "const fix=v=>JSON.stringify(v,(k,x)=>typeof x==='string'?x.split(ROOT).join(''):x);\n";

const cases = [
  ["relative_basic", "R=fix([require('./a').n,require('./a')===require('./a.js')])"],
  ["relative_module_fields", "R=fix(require('./a'))"],
  ["bare_package_main", "R=fix(require('pk'))"],
  ["cache_keys", "require('./a');require('pk');R=fix(Object.keys(require.cache))"],
  ["main_children", "require('./a');require('pk');R=fix(module.children.map(c=>c.id))"],
  ["main_fields", "R=fix([module.id,module.filename,module.loaded,module.parent,require.main===module])"],
  ["not_found_relative", "try{require('./nope')}catch(e){R=fix([e.code,e.message])}"],
  ["not_found_bare", "try{require('nope')}catch(e){R=fix([e.code,e.message])}"],
  ["resolve", "R=fix([require.resolve('./a'),require.resolve('pk'),require.resolve('./sub/s')])"],
  ["symlink_cache_key", "require('./ln/x');R=fix(Object.keys(require.cache))"],
  ["symlink_module_id", "R=fix(require('./ln/x').id)"],
  ["circular", "R=fix(require('./c1'))"],
  ["json", "R=fix(require('./d.json'))"],
  ["json_cache", "R=fix([require('./d.json')===require('./d.json'),Object.keys(require.cache)])"],
  ["thrower_leaves_cache", "try{require('./thrower')}catch(e){};R=fix(Object.keys(require.cache).includes(ROOT+'/thrower.js'))"],
  ["thrower_message", "try{require('./thrower')}catch(e){R=fix(e.message)}"],
  ["child_relative", "R=fix(require('./sub/s'))"],
  ["child_children", "require('./c1');R=fix(require.cache[ROOT+'/c1.js'].children.map(c=>c.id))"],
  ["child_parent_is_main", "R=fix(require('./a').parentId===module.id&&require.cache[ROOT+'/a.js'].parent===module)"],
  ["cache_delete_reloads", "const a=require('./a');delete require.cache[ROOT+'/a.js'];R=fix(require('./a')===a)"],
  ["child_require_resolve", "R=fix(require('./sub/s').res)"],
];

const root = fs.mkdtempSync(path.join(os.tmpdir(), "cjs-load-"));
for (const [rel, text] of Object.entries(tree)) {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), text);
}
for (const [link, target] of Object.entries(links)) fs.symlinkSync(target, path.join(root, link));

const real = fs.realpathSync(root);
for (const [name, body] of cases) {
  const source = prelude + body;
  fs.writeFileSync(path.join(root, "main.js"), source + "\nprocess.stdout.write(JSON.stringify(R));\n");
  const run = spawnSync(BUN, [path.join(root, "main.js")], { cwd: root, encoding: "utf8" });
  if (run.status !== 0) throw new Error(`${name}: ${run.stderr}`);
  // `R` já saiu sem `ROOT`; a saída é o JSON de R (uma string).
  if (run.stdout.includes(real) || run.stdout.includes(root)) throw new Error(`${name}: caminho da máquina no resultado`);
  process.stdout.write(`${name}\t${JSON.stringify(source)}\t${run.stdout}\n`);
}
fs.rmSync(root, { recursive: true, force: true });
