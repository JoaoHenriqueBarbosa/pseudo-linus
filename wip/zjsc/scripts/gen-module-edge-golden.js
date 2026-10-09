// Gera tests/golden/module_edge_bun.tsv: golden de módulos ES de borda (cerca de 400 programas) avaliado no bun.
// Complementa gen-module-golden.js e gen-module-more-golden.js: ciclos com TDZ em export, export * conflitante e
// ambíguo, export default de classe e função anônima (name), import.meta, import() dinâmico com rejeição,
// top-level await com ordem entre irmãos, namespace objects (Symbol.toStringTag, extensibilidade, descritores),
// re-export de namespace e import attributes. Mesmo formato e mesmo runner do gerador anterior.
// Uso: bun scripts/gen-module-edge-golden.js > tests/golden/module_edge_bun.tsv
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
const main = (body, others = {}) => add({ "main.mjs": body, ...others });
const T = (expr) => `try { ${expr}; } catch (e) { L(e.name + ': ' + e.message); }`;

const RUNNER = `
globalThis.log = [];
globalThis.L = (x) => { log.push(String(x)); };
const [dir] = process.argv.slice(2);
let error = null;
try { await import(dir + "/main.mjs"); } catch (e) {
  error = e instanceof Error ? e.name + ": " + e.message : "throw " + String(e);
}
for (let i = 0; i < 50; i++) await Promise.resolve();
const clean = (s) => s.split("file://" + dir + "/").join("file:///").split(dir + "/").join("");
console.log(clean(JSON.stringify({ log, error })));
`;

// 1. Ciclos com TDZ em export: o que cada forma de declaração mostra quando lida antes da avaliação.
const decls = {
  let: "export let v = 'lv';",
  const: "export const v = 'cv';",
  var: "export var v = 'vv';",
  fn: "export function v() { return 'fv'; }",
  cls: "export class v {}",
  asyncFn: "export async function v() {}",
  gen: "export function* v() {}",
  deflt: "const q = 'dq'; export { q as v };",
  late: "L('a body'); export let v = 'late';",
};
const cycleReads = [
  "L(typeof v);",
  "try { L(v); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { L(typeof v); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { v = 1; } catch (e) { L(e.name + ': ' + e.message); }",
  "try { L(v === undefined); } catch (e) { L(e.name + ': ' + e.message); }",
];
for (const [dn, dsrc] of Object.entries(decls)) {
  for (const read of cycleReads) {
    // o ciclo: main importa b, b importa a (que importa b de volta); b lê v de a antes de a avaliar
    main(`import './b.mjs'; L('main');`, {
      "a.mjs": `import './b.mjs'; L('a start'); ${dsrc} L('a end');`,
      "b.mjs": `import { v } from './a.mjs'; L('b start'); ${read} L('b end');`,
    });
  }
  main(`import * as ns from './a.mjs'; L('main'); L(typeof ns.v);`, {
    "a.mjs": `import * as self from './a.mjs'; try { L(typeof self.v); } catch (e) { L(e.name + ': ' + e.message); } try { L(Object.keys(self).join()); } catch (e) { L(e.name + ': ' + e.message); } try { L(JSON.stringify(Object.getOwnPropertyDescriptor(self, 'v'))); } catch (e) { L(e.name + ': ' + e.message); } ${dsrc}`,
  });
  main(`import * as ns from './b.mjs'; L('main');`, {
    "a.mjs": `import * as nb from './b.mjs'; ${dsrc} export const first = 1;`,
    "b.mjs": `import * as na from './a.mjs'; try { L(Reflect.has(na, 'v')); L(Object.keys(na).join()); L('first' in na); } catch (e) { L(e.name + ': ' + e.message); } export const bb = 1; import './a.mjs';`,
  });
}
// reexport em ciclo e TDZ visto por quem reexporta
for (const read of cycleReads) {
  main(`import './b.mjs'; L('main');`, {
    "a.mjs": `import './b.mjs'; export let v = 'av'; L('a end');`,
    "b.mjs": `import { w } from './r.mjs'; L('b start'); ${read.replace(/\bv\b/g, "w")} L('b end');`,
    "r.mjs": `export { v as w } from './a.mjs';`,
  });
  main(`import './b.mjs'; L('main');`, {
    "a.mjs": `import './b.mjs'; export let v = 'av'; L('a end');`,
    "b.mjs": `import { w } from './r.mjs'; L('b start'); ${read.replace(/\bv\b/g, "w")} L('b end');`,
    "r.mjs": `import { v } from './a.mjs'; export { v as w };`,
  });
}

// 2. export * conflitante e ambíguo.
const stars = {
  conflict: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs';", "p.mjs": "export const x = 'P', a = 1;", "q.mjs": "export const x = 'Q', b = 2;" },
  sameBinding: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs';", "p.mjs": "export { x } from './z.mjs';", "q.mjs": "export { x } from './z.mjs';", "z.mjs": "export const x = 'Z';" },
  sameViaAlias: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs';", "p.mjs": "export { y as x } from './z.mjs';", "q.mjs": "export { y as x } from './z.mjs';", "z.mjs": "export const y = 'Z';" },
  diffBindingSameValue: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs';", "p.mjs": "export const x = 1;", "q.mjs": "export const x = 1;" },
  ownWins: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs'; export const x = 'own';", "p.mjs": "export const x = 'P';", "q.mjs": "export const x = 'Q';" },
  starAsWins: { "r.mjs": "export * from './p.mjs'; export * as x from './q.mjs';", "p.mjs": "export const x = 'P';", "q.mjs": "export const k = 'Q';" },
  defaultNotStarred: { "r.mjs": "export * from './p.mjs'; export * from './q.mjs';", "p.mjs": "export default 'pd'; export const a = 1;", "q.mjs": "export default 'qd'; export const b = 2;" },
  deepConflict: { "r.mjs": "export * from './m.mjs'; export * from './q.mjs';", "m.mjs": "export * from './p.mjs';", "p.mjs": "export const x = 'P';", "q.mjs": "export const x = 'Q';" },
  diamond: { "r.mjs": "export * from './m1.mjs'; export * from './m2.mjs';", "m1.mjs": "export * from './p.mjs';", "m2.mjs": "export * from './p.mjs';", "p.mjs": "export const x = 'P';" },
  cycleStar: { "r.mjs": "export * from './p.mjs'; export const own = 1;", "p.mjs": "export * from './r.mjs'; export const pp = 2;" },
  cycleConflict: { "r.mjs": "export * from './p.mjs'; export const x = 'r';", "p.mjs": "export * from './r.mjs'; export const x = 'p';" },
};
const starUses = [
  "import * as ns from './r.mjs'; L(Object.keys(ns).join());",
  "import { x } from './r.mjs'; L(x);",
  "import { a } from './r.mjs'; L(a);",
  "export { x } from './r.mjs'; L('after');",
  "import * as ns from './r.mjs'; L(ns.x); L('x' in ns); L(Reflect.ownKeys(ns).map(String).join());",
  "const ns = await import('./r.mjs'); L(Object.keys(ns).join()); L(ns.x);",
  "import def from './r.mjs'; L(def);",
  "export * from './r.mjs'; import * as me from './main.mjs'; L(Object.keys(me).join());",
];
for (const [sn, files] of Object.entries(stars)) for (const use of starUses) main(use, { "z.mjs": "export const z = 1;", ...files });

// 3. export default de classe e função anônima, e name.
const defaults = {
  anonClass: "export default class {}",
  anonClassStaticName: "export default class { static name = 'n'; }",
  anonClassNameMethod: "export default class { static name() { return 1; } }",
  anonClassStaticBlock: "export default class { static { this.s = this.name; } }",
  anonFn: "export default function () {}",
  anonAsync: "export default async function () {}",
  anonGen: "export default function* () {}",
  anonAsyncGen: "export default async function* () {}",
  namedClass: "export default class K {}",
  namedFn: "export default function f() {}",
  exprClass: "export default (class {});",
  exprFn: "export default (function () {});",
  exprArrow: "export default () => {};",
  exprNamedClass: "export default (class Inner {});",
  exprClassStatic: "export default (class { static x = 1; });",
  comma: "export default (0, class {});",
  asDefaultClass: "class C {} export { C as default };",
  asDefaultAnon: "const f = function () {}; export { f as default };",
  asDefaultObjFn: "export default { m() {}, f: function () {}, a: () => {} };",
  starAsDefault: "export * as default from './z.mjs';",
  reexportDefault: "export { default } from './z.mjs';",
  reexportDefaultAs: "export { default as default } from './z.mjs';",
  importExportDefault: "import d from './z.mjs'; export default d;",
};
const defaultUses = [
  "import d from './a.mjs'; L(typeof d); L(d && d.name); L(Object.getOwnPropertyNames(d || {}).join());",
  "import d from './a.mjs'; L(JSON.stringify(Object.getOwnPropertyDescriptor(d, 'name')));",
  "import * as ns from './a.mjs'; L(Object.keys(ns).join()); L(typeof ns.default); L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'default')));",
  "const m = await import('./a.mjs'); L(m.default && m.default.name);",
  "import d from './a.mjs'; L(Object.prototype.toString.call(d)); L(d.constructor && d.constructor.name);",
];
for (const [dn, src] of Object.entries(defaults)) {
  for (const use of defaultUses) main(use, { "a.mjs": src, "z.mjs": "export default function zf() {} export const q = 1;" });
}

// 4. import.meta.
const metaBodies = [
  "L(typeof import.meta); L(Object.getPrototypeOf(import.meta));",
  "L(Object.keys(import.meta).sort().join());",
  "L(Object.isExtensible(import.meta)); L(Object.isFrozen(import.meta)); L(Object.isSealed(import.meta));",
  "L(import.meta === import.meta);",
  "import.meta.custom = 1; L(import.meta.custom); L(delete import.meta.custom); L(import.meta.custom);",
  "L(import.meta.url);",
  "L(typeof import.meta.url); L(typeof import.meta.dir); L(typeof import.meta.dirname); L(typeof import.meta.filename); L(typeof import.meta.resolve);",
  "L(Object.prototype.toString.call(import.meta));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(import.meta, 'url') && Object.keys(Object.getOwnPropertyDescriptor(import.meta, 'url'))));",
  "const { url } = import.meta; L(url);",
  "const f = () => import.meta.url; L(f());",
  "function g() { return import.meta.url; } L(g());",
  "L(eval('typeof import.meta'));",
  "L(new Function('return typeof import.meta')());",
  "import { m } from './a.mjs'; L(m === import.meta);",
  "import { u } from './a.mjs'; L(u);",
  "import * as a from './a.mjs'; L(a.m.url === import.meta.url);",
  "L(import.meta?.url); L(import.meta['url']);",
  "import.meta = 1;",
  "L(typeof import.meta.nope); L(import.meta.nope);",
  "export const m = import.meta; L(m === import.meta);",
  "L(Object.getOwnPropertySymbols(import.meta).length);",
  "label: { L(import.meta.url); }",
  "class K { static s = import.meta.url; m() { return import.meta.url; } } L(K.s); L(new K().m());",
];
for (const body of metaBodies) main(body, { "a.mjs": "export const m = import.meta; export const u = import.meta.url;" });
main("L(import.meta.url);", { "a.js": "" });
main("import './sub/a.mjs';", { "sub/a.mjs": "L(import.meta.url); L(import.meta.url.endsWith('/sub/a.mjs'));" });
main("const s = 'import.meta'; L(s); L(typeof import.meta);", {});
main("var import.meta;", {});
main("let { import.meta: x } = {};", {});
main("L(new import.meta.url);", {});
main("L(typeof new.target);", {});
main("L(import.meta.url.length > 0);", {});

// 5. import() dinâmico com rejeição.
const dynBodies = [
  "try { await import('./missing.mjs'); L('ok'); } catch (e) { L(e.name); }",
  "try { await import('./thrower.mjs'); L('ok'); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./thrower.mjs'); } catch (e) { L(e.message); } try { await import('./thrower.mjs'); } catch (e) { L(e.message); }",
  "const p1 = import('./thrower.mjs').catch((e) => e); const p2 = import('./thrower.mjs').catch((e) => e); const [a, b] = await Promise.all([p1, p2]); L(a === b); L(a.message);",
  "try { await import('./syntax.mjs'); } catch (e) { L(e.name); }",
  "try { await import('./badexport.mjs'); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./ambig.mjs'); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./tdzcycle.mjs'); } catch (e) { L(e.name + ': ' + e.message); }",
  "const p = import('./thrower.mjs'); L(p instanceof Promise); L(Object.prototype.toString.call(p)); try { await p; } catch (e) { L('caught'); }",
  "try { await import(); } catch (e) { L(e.name); }",
  "try { await import(undefined); } catch (e) { L(e.name); }",
  "try { await import({ toString() { return './ok.mjs'; } }).then((m) => L(m.v)); } catch (e) { L(e.name); }",
  "try { await import(Symbol('s')); } catch (e) { L(e.name + ': ' + e.message); }",
  "const p = import({ toString() { throw new Error('ts'); } }); L(p instanceof Promise); try { await p; } catch (e) { L(e.message); }",
  "try { await import('./ok.mjs', 5); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./ok.mjs', { with: 5 }); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./ok.mjs', { with: { type: 5 } }); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { const m = await import('./ok.mjs', { with: {} }); L(m.v); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { const m = await import('./ok.mjs', undefined); L(m.v); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./ok.mjs', { with: { type: 'json' } }); } catch (e) { L(e.name); }",
  "try { await import('./ok.mjs', { with: { foo: 'bar' } }); } catch (e) { L(e.name); }",
  "await Promise.allSettled([import('./ok.mjs'), import('./thrower.mjs'), import('./missing.mjs')]).then((r) => L(r.map((x) => x.status).join()));",
  "const r = await Promise.race([import('./thrower.mjs').catch(() => 'rej'), import('./ok.mjs').then(() => 'ok')]); L(r);",
  "import('./thrower.mjs').then(() => L('then'), (e) => L('rej ' + e.message)); L('sync');",
  "import('./ok.mjs').then((m) => L('A ' + m.v)); import('./ok.mjs').then((m) => L('B ' + m.v)); L('sync');",
  "const a = await import('./ok.mjs'); const b = await import('./ok.mjs'); L(a === b);",
  "const a = await import('./ok.mjs'); const b = await import('./ok.mjs?q'); L(a === b);",
  "import * as s from './ok.mjs'; const b = await import('./ok.mjs'); L(s === b);",
  "const f = () => import('./ok.mjs'); L((await f()).v);",
  "L(typeof import); ",
  "const x = import; ",
  "new import('./ok.mjs');",
  "L(import('./ok.mjs').constructor === Promise);",
  "try { await import('./self-throw.mjs'); } catch (e) { L(e.message); }",
  "try { await import('./tla-reject.mjs'); } catch (e) { L(e); }",
  "try { await import('./tla-reject.mjs'); } catch (e) { L(e); } try { await import('./tla-reject.mjs'); } catch (e) { L(e); }",
  "try { await import('./imports-thrower.mjs'); } catch (e) { L(e.message); } L(log.join('|'));",
];
for (const body of dynBodies) {
  main(body, {
    "ok.mjs": "export const v = 'ok';",
    "thrower.mjs": "L('thrower eval'); throw new Error('boom');",
    "syntax.mjs": "export const = ;",
    "badexport.mjs": "import { nope } from './ok.mjs';",
    "ambig.mjs": "import { x } from './amb.mjs'; L(x);",
    "amb.mjs": "export * from './p.mjs'; export * from './q.mjs';",
    "p.mjs": "export const x = 1;",
    "q.mjs": "export const x = 2;",
    "tdzcycle.mjs": "import { w } from './tdz2.mjs'; export let t = 1;",
    "tdz2.mjs": "import { t } from './tdzcycle.mjs'; L(t); export const w = 1;",
    "self-throw.mjs": "import * as me from './self-throw.mjs'; L(Object.keys(me).join()); throw new RangeError('self');",
    "tla-reject.mjs": "await Promise.resolve(); throw new TypeError('tla');",
    "imports-thrower.mjs": "import './ok.mjs'; import './thrower.mjs'; L('never');",
  });
}

// 6. Top-level await em grafos: ordem entre irmãos.
const tlaSibling = {
  none: "L('S start'); L('S end');",
  nullAwait: "L('S start'); await null; L('S end');",
  twoAwaits: "L('S start'); await null; L('S mid'); await null; L('S end');",
  timer: "L('S start'); for (let i = 0; i < 5; i++) await Promise.resolve(); L('S end');",
  thenable: "L('S start'); await { then(r) { L('S then'); r(); } }; L('S end');",
  reject: "L('S start'); await null; throw new Error('S fail');",
  forAwait: "L('S start'); for await (const x of [1, 2]) L('S it ' + x); L('S end');",
};
const tlaGraphs = {
  twoSiblings: (s1, s2) => ({ "main.mjs": "import './a.mjs'; import './b.mjs'; L('main');", "a.mjs": s1.replace(/S /g, "a "), "b.mjs": s2.replace(/S /g, "b ") }),
  siblingThenSync: (s1) => ({ "main.mjs": "import './a.mjs'; import './b.mjs'; import './c.mjs'; L('main');", "a.mjs": s1.replace(/S /g, "a "), "b.mjs": "L('b sync');", "c.mjs": "L('c sync');" }),
  parentChild: (s1, s2) => ({ "main.mjs": "import './a.mjs'; import './b.mjs'; L('main');", "a.mjs": "import './c.mjs'; " + s1.replace(/S /g, "a "), "b.mjs": s2.replace(/S /g, "b "), "c.mjs": "L('c start'); await null; L('c end');" }),
  sharedDep: (s1, s2) => ({ "main.mjs": "import './a.mjs'; import './b.mjs'; L('main');", "a.mjs": "import './c.mjs'; " + s1.replace(/S /g, "a "), "b.mjs": "import './c.mjs'; " + s2.replace(/S /g, "b "), "c.mjs": "L('c start'); await null; L('c end');" }),
};
const tlaKeys = Object.keys(tlaSibling);
for (const [gn, mk] of Object.entries(tlaGraphs)) {
  if (gn === "siblingThenSync") {
    for (const k1 of tlaKeys) add(mk(tlaSibling[k1]));
    continue;
  }
  for (const k1 of tlaKeys) for (const k2 of tlaKeys) {
    if ((tlaKeys.indexOf(k1) + tlaKeys.indexOf(k2)) % 2 === 1 && gn !== "twoSiblings") continue;
    add(mk(tlaSibling[k1], tlaSibling[k2]));
  }
}
// await dentro de ciclo e consumidores que observam a ordem
main("import './a.mjs'; L('main');", { "a.mjs": "import './b.mjs'; L('a start'); await null; L('a end');", "b.mjs": "import './a.mjs'; L('b start'); await null; L('b end');" });
main("import { a } from './a.mjs'; L('main ' + a);", { "a.mjs": "import './b.mjs'; export let a = 'A'; await null; a = 'A2';", "b.mjs": "import './a.mjs'; L('b');" });
main("import './a.mjs'; import './b.mjs'; L('main');", { "a.mjs": "L('a'); await 1; L('a2');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "L('c'); await 1; L('c2');" });
main("import './a.mjs'; Promise.resolve().then(() => L('micro')); L('main');", { "a.mjs": "await null; L('a');" });
main("Promise.resolve().then(() => L('timer')); import './a.mjs'; L('main');", { "a.mjs": "for (let i = 0; i < 5; i++) await Promise.resolve(); L('a');" });
main("await null; L('main1'); await null; L('main2'); import './a.mjs';", { "a.mjs": "await null; L('a');" });
main("import { v } from './a.mjs'; L(v);", { "a.mjs": "export const v = await Promise.resolve('tla value');" });
main("import { v } from './a.mjs'; L(v);", { "a.mjs": "export let v; v = await Promise.resolve('assigned');" });
main("import * as ns from './a.mjs'; L(ns.v); L(Object.keys(ns).join());", { "a.mjs": "export const v = await 1; export default await 2;" });
main("L(typeof await);", {});
main("function f() { await 1; } L('x');", {});
main("const f = () => await 1;", {});
main("L(await await 1);", {});
main("L(await (async () => 5)());", {});
main("for await (const x of [Promise.resolve(1), 2]) L(x);", {});
main("await Promise.reject(new Error('top'));", {});
main("await Promise.reject(7);", {});
main("try { await Promise.reject(new Error('c')); } catch (e) { L(e.message); } L('after');", {});
main("import './a.mjs'; L('main');", { "a.mjs": "await Promise.reject(new Error('a rej'));" });
main("import './a.mjs'; import './b.mjs'; L('main');", { "a.mjs": "await null; throw new Error('a');", "b.mjs": "await null; throw new Error('b');" });
main("L('main start'); for (let i = 0; i < 3; i++) await Promise.resolve(); L('main end');", {});
main("import('./a.mjs').then(() => L('a done')); import('./b.mjs').then(() => L('b done')); L('sync');", { "a.mjs": "await null; await null; L('a');", "b.mjs": "await null; L('b');" });
main("await Promise.all([import('./a.mjs'), import('./b.mjs')]); L('both');", { "a.mjs": "await null; await null; L('a');", "b.mjs": "await null; L('b');" });
main("const [a, b] = await Promise.all([import('./a.mjs'), import('./b.mjs')]); L(a.v + b.v);", { "a.mjs": "export const v = await 'A';", "b.mjs": "export const v = await 'B';" });

// 7. Namespace objects: toStringTag, extensibilidade, descritores, operações.
const nsProvider = { "a.mjs": "export let x = 1; export const y = 'y'; export function inc() { x++; } export default 'd';" };
const nsOps = [
  "L(ns[Symbol.toStringTag]); L(Object.prototype.toString.call(ns)); L(String(Object.getPrototypeOf(ns)));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag)));",
  "L(Object.isExtensible(ns)); L(Object.isFrozen(ns)); L(Object.isSealed(ns));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'x'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'nope')));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptors(ns)));",
  "L(Reflect.ownKeys(ns).map(String).join());",
  "L(Object.getOwnPropertySymbols(ns).length); L(Object.getOwnPropertyNames(ns).join());",
  "L(Reflect.set(ns, 'x', 2)); L(Reflect.set(ns, 'nope', 2)); L(Reflect.set(ns, Symbol.toStringTag, 'z'));",
  "L(Reflect.defineProperty(ns, 'x', { value: 1 })); L(Reflect.defineProperty(ns, 'x', { value: 2 })); L(Reflect.defineProperty(ns, 'nope', { value: 2 }));",
  "L(Reflect.defineProperty(ns, 'x', { value: 1, writable: true, enumerable: true, configurable: false })); L(Reflect.defineProperty(ns, 'x', { configurable: true }));",
  "L(Reflect.deleteProperty(ns, 'x')); L(Reflect.deleteProperty(ns, 'nope')); L(Reflect.deleteProperty(ns, Symbol.toStringTag)); L(Reflect.deleteProperty(ns, Symbol.iterator));",
  "L(Reflect.setPrototypeOf(ns, null)); L(Reflect.setPrototypeOf(ns, {})); L(Reflect.getPrototypeOf(ns));",
  "L(Reflect.preventExtensions(ns)); L(Reflect.isExtensible(ns));",
  "" + T("ns.x = 2") + T("ns.nope = 1") + T("delete ns.x") + T("ns[Symbol.toStringTag] = 'q'"),
  "" + T("Object.defineProperty(ns, 'x', { value: 5 })") + T("Object.defineProperty(ns, 'nope', { value: 5 })") + T("Object.setPrototypeOf(ns, {})") + T("Object.freeze(ns)") + T("Object.seal(ns)") + T("Object.preventExtensions(ns)"),
  "L(Object.keys(ns).join()); L(Object.values(ns).join()); L(JSON.stringify(Object.entries(ns)));",
  "L(JSON.stringify(ns)); L(Object.assign({}, ns).y); L(JSON.stringify({ ...ns }));",
  "L('x' in ns); L('nope' in ns); L(Symbol.toStringTag in ns); L(Object.hasOwn(ns, 'x')); L(ns.hasOwnProperty); L(ns.toString);",
  "L(typeof ns.then); L(ns.constructor); L(ns.__proto__);",
  "L(Object.getPrototypeOf(ns) === null); L(ns instanceof Object); L(typeof ns);",
  "ns.inc(); L(ns.x); inc2(); L(ns.x);",
  "L(Object.is(ns, ns)); L(ns === (await import('./a.mjs')));",
  "for (const k in ns) L(k); for (const k of Object.keys(ns)) L(k);",
  "const { x, y, ...rest } = ns; L(x); L(y); L(Object.keys(rest).join());",
  "try { for (const v of ns) L(v); } catch (e) { L(e.name + ': ' + e.message); }",
  "L(Array.isArray(ns)); L(Object.prototype.propertyIsEnumerable.call(ns, 'x')); L(Object.prototype.propertyIsEnumerable.call(ns, Symbol.toStringTag));",
  "const p = new Proxy(ns, {}); L(Object.keys(p).join()); L(p.x); L(Reflect.set(p, 'x', 3));",
  "const w = new WeakSet(); w.add(ns); L(w.has(ns)); const m = new Map([[ns, 1]]); L(m.get(ns));",
  "L(Object.getOwnPropertyNames(ns).length); L(Object.entries(Object.getOwnPropertyDescriptors(ns)).map(([k, d]) => k + ':' + d.writable + d.enumerable + d.configurable).join());",
  "Object.defineProperty(ns, Symbol.toStringTag, { value: 'Module', writable: false, enumerable: false, configurable: false }); L('same ok');",
  "L(ns.default); L(ns['default']); L(Reflect.get(ns, 'default'));",
  "L(Reflect.get(ns, 'x', { x: 'receiver' })); L(Reflect.has(ns, 'y'));",
  "L(Object.getOwnPropertyDescriptor(ns, 'x').get); L(Object.getOwnPropertyDescriptor(ns, 'x').set);",
];
for (const op of nsOps) {
  main(`import * as ns from './a.mjs'; import { inc as inc2 } from './a.mjs'; ${op}`, nsProvider);
}
main("import * as ns from './a.mjs'; export { ns }; import * as me from './main.mjs'; L(me.ns === ns); L(Object.keys(me).join()); L(me[Symbol.toStringTag]);", nsProvider);
main("import * as me from './main.mjs'; export const k = 1; L(me.k); L(Object.keys(me).join());", {});
main("import * as me from './main.mjs'; L(Object.keys(me).join()); L(typeof me.k); export let k = 1;", {});
main("import * as me from './main.mjs'; try { L(me.k); } catch (e) { L(e.name + ': ' + e.message); } try { L(Object.getOwnPropertyDescriptor(me, 'k')); } catch (e) { L(e.name + ': ' + e.message); } try { L(JSON.stringify(Object.values(me))); } catch (e) { L(e.name + ': ' + e.message); } export let k = 1;", {});
main("import * as me from './main.mjs'; try { L(Reflect.ownKeys(me).map(String).join()); L('k' in me); L(Reflect.has(me, 'k')); L(Object.hasOwn(me, 'k')); } catch (e) { L(e.name + ': ' + e.message); } export let k = 1;", {});
main("import * as me from './main.mjs'; try { me.k = 1; } catch (e) { L(e.name + ': ' + e.message); } try { delete me.k; } catch (e) { L(e.name + ': ' + e.message); } export let k = 1;", {});

// 8. Re-export de namespace.
const reNs = {
  starAs: "export * as n from './a.mjs';",
  starAsDefault: "export * as default from './a.mjs';",
  starAsString: "export * as 'a-b' from './a.mjs';",
  importThenExport: "import * as n from './a.mjs'; export { n };",
  importThenExportDefault: "import * as n from './a.mjs'; export default n;",
  twoNames: "export * as n from './a.mjs'; export * as m from './a.mjs';",
  nested: "export * as n from './r2.mjs';",
  withStar: "export * from './a.mjs'; export * as n from './a.mjs';",
  cycleSelf: "export * as me from './r.mjs'; export const k = 1;",
};
const reNsUses = [
  "import { n } from './r.mjs'; L(Object.keys(n).join()); L(n[Symbol.toStringTag]);",
  "import * as r from './r.mjs'; L(Object.keys(r).join()); L(JSON.stringify(Object.getOwnPropertyDescriptor(r, 'n')));",
  "import * as r from './r.mjs'; import * as a from './a.mjs'; L(r.n === a); L(r.default === a); L(r.m === r.n);",
  "import d from './r.mjs'; L(Object.keys(d).join()); L(typeof d);",
  "import { 'a-b' as n } from './r.mjs'; L(Object.keys(n).join());",
  "import { me } from './r.mjs'; L(Object.keys(me).join()); L(me.me === me);",
  "const r = await import('./r.mjs'); L(Object.keys(r).join()); L(Reflect.ownKeys(r).map(String).join());",
  "import { n } from './r.mjs'; try { n.x = 1; } catch (e) { L(e.name + ': ' + e.message); } L(n.x);",
];
for (const [rn, rsrc] of Object.entries(reNs)) {
  for (const use of reNsUses) main(use, { "r.mjs": rsrc, "r2.mjs": "export * as inner from './a.mjs'; export const two = 2;", "a.mjs": "export let x = 1; export const y = 2; export default 'd';" });
}

// 9. Import attributes.
const attrFiles = { "d.json": '{"a":1,"b":[2]}', "ok.mjs": "export const v = 1;", "t.txt": "hi" };
const attrBodies = [
  "import d from './d.json' with { type: 'json' }; L(JSON.stringify(d));",
  "import d from './d.json' with { type: \"json\" }; L(typeof d);",
  "import d from './d.json' with { 'type': 'json' }; L(typeof d);",
  "import d from './d.json' with { type: 'json', }; L(typeof d);",
  "import d from './d.json' with { type: 'json', type: 'json' }; L(typeof d);",
  "import d from './d.json' with { type: 'json', foo: 'bar' }; L(typeof d);",
  "import d from './d.json' with { foo: 'bar' }; L(typeof d);",
  "import d from './d.json' with { type: 'JSON' }; L(typeof d);",
  "import d from './d.json' with { type: json }; L(typeof d);",
  "import d from './d.json' with { type: 1 }; L(typeof d);",
  "import d from './d.json' with { type: `json` }; L(typeof d);",
  "import d from './d.json' assert { type: 'json' }; L(typeof d);",
  "import d from './d.json' with type: 'json'; L(typeof d);",
  "import d from './d.json' with { type: 'json' } with { type: 'json' }; L(typeof d);",
  "import d from './d.json'; L(typeof d);",
  "import d from './ok.mjs' with { type: 'json' }; L(typeof d);",
  "import { v } from './ok.mjs' with { type: 'javascript' }; L(v);",
  "import { v } from './ok.mjs' with { }; L(v);",
  "import { v } from './ok.mjs' with { type: 'js' }; L(v);",
  "import * as ns from './d.json' with { type: 'json' }; L(Object.keys(ns).join()); L(ns.default.a);",
  "import './d.json' with { type: 'json' }; L('side effect');",
  "import { default as d } from './d.json' with { type: 'json' }; L(d.b[0]);",
  "export { default as d } from './d.json' with { type: 'json' }; L('reexport');",
  "export * from './d.json' with { type: 'json' }; L('star');",
  "export * as j from './d.json' with { type: 'json' }; import * as me from './main.mjs'; L(Object.keys(me.j).join());",
  "import a from './d.json' with { type: 'json' }; import b from './d.json' with { type: 'json' }; L(a === b);",
  "import a from './d.json' with { type: 'json' }; const b = await import('./d.json', { with: { type: 'json' } }); L(a === b.default);",
  "const b = await import('./d.json', { with: { type: 'json' } }); L(JSON.stringify(b.default)); L(Object.keys(b).join());",
  "try { const b = await import('./d.json'); L(Object.keys(b).join()); } catch (e) { L(e.name); }",
  "try { await import('./d.json', { with: { type: 'text' } }); } catch (e) { L(e.name); }",
  "try { await import('./d.json', { assert: { type: 'json' } }); L('assert ok'); } catch (e) { L(e.name); }",
  "try { await import('./d.json', { with: { type: 'json', x: 'y' } }); L('extra ok'); } catch (e) { L(e.name); }",
  "const opts = { with: { type: 'json' } }; await import('./d.json', opts); L(Object.keys(opts.with).join());",
  "const opts = { get with() { L('getter'); return { type: 'json' }; } }; await import('./d.json', opts); L('done');",
  "const o = { with: { get type() { L('type getter'); return 'json'; } } }; await import('./d.json', o); L('done');",
  "try { await import('./d.json', { with: null }); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./d.json', null); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./d.json', { with: [] }); L('arr ok'); } catch (e) { L(e.name + ': ' + e.message); }",
  "import t from './t.txt' with { type: 'text' }; L(typeof t);",
  "import.meta.x = 1; import d from './d.json' with { type: 'json' }; L(typeof d);",
  "import d, { a } from './d.json' with { type: 'json' }; L(typeof d); L(a);",
  "import d, * as ns from './d.json' with { type: 'json' }; L(d === ns.default);",
];
for (const body of attrBodies) main(body, attrFiles);

// Execução.
const lines = [];
const failures = [];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "zjsc-module-edge-golden-"));
const runner = path.join(tmp, "runner.mjs");
fs.writeFileSync(runner, RUNNER);
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
cases.splice(0, cases.length, ...cases.filter((p) => !usesHostApi(p)));
cases.forEach((files, i) => {
  const dir = path.join(tmp, "case" + i);
  for (const [name, source] of Object.entries(files)) {
    const file = path.join(dir, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, source);
  }
  const r = spawnSync(BUN, [runner, dir], { encoding: "utf8", timeout: 10000, cwd: tmp });
  let out = (r.stdout || "").trim().split("\n").pop() || "";
  if (r.error || r.status === null) out = JSON.stringify({ log: [], error: "timeout" });
  else if (!out.startsWith("{")) out = JSON.stringify({ log: [], error: "exit " + r.status });
  const clean = JSON.stringify(files);
  if (/[\t\n\r]/.test(clean) || /[\t\n\r]/.test(out)) throw new Error("tab ou quebra de linha no caso " + i);
  if (out.includes(tmp)) failures.push(i);
  lines.push(clean + "\t" + out);
});
fs.rmSync(tmp, { recursive: true, force: true });
if (failures.length) throw new Error("caminho da máquina na saída dos casos " + failures.join(","));
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`casos: ${lines.length}\n`);
