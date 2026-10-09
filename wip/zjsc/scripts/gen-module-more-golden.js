// Gera tests/golden/module_more_bun.tsv: segundo golden de módulos ES (cerca de 1200 programas) avaliado no bun.
// Complementa gen-module-golden.js: export default anônimo, export * e export * as, ciclos com TDZ e hoisting,
// namespace objects, re-export ambíguo, import de binding inexistente, live bindings, import.meta, import()
// dinâmico, top-level await (ordem, ciclo, rejeição), strict, this, await reservado, import attributes e erros
// de parse. O bun só carrega módulos do disco: cada caso vira um diretório temporário com os arquivos, o ponto
// de entrada é main.mjs, e os caminhos da máquina são normalizados na saída (`file:///arquivo.mjs`).
// Formato das colunas e do runner idêntico ao do gerador original.
// Uso: bun scripts/gen-module-more-golden.js > tests/golden/module_more_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";
const { stepSampler } = require("./golden-prelude.js");
const pool = stepSampler();
const seen = new Set();
// Fração mantida dos casos gerados: os produtos cartesianos são afinados por seção (`section(fração)`), e a escolha dentro da
// seção é por hash do conjunto de arquivos (`sampleByHash` dentro do `stepSampler`), nunca pelo contador de casos.
let keep = 1;
let sectionId = 0;
const section = (fraction) => {
  keep = fraction;
  sectionId++;
};
const add = (files) => {
  const key = JSON.stringify(files);
  if (seen.has(key)) return;
  seen.add(key);
  pool.pushIn("seção " + sectionId, 1 / keep, files);
};
const main = (body, others = {}) => add({ "main.mjs": body, ...others });
// Tenta capturar o erro de uma expressão e registrar nome e mensagem.
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

// Gerador pseudoaleatório determinístico (mulberry32).
let seed = 20261008;
const rnd = () => {
  seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const pick = (arr) => arr[Math.floor(rnd() * arr.length)];

// 1. export default: formas anônimas e nomeadas x formas de uso.
section(0.5);
const defaultForms = {
  anonFn: "export default function () { return 1; }",
  anonClass: "export default class { static s = 1; }",
  anonAsync: "export default async function () {}",
  anonGen: "export default function* () {}",
  anonAsyncGen: "export default async function* () {}",
  namedFn: "export default function foo() {}",
  namedClass: "export default class Foo {}",
  arrow: "export default () => 1;",
  anonExprFn: "export default (function () {});",
  anonExprClass: "export default (class {});",
  namedExprFn: "export default (function bar() {});",
  num: "export default 42;",
  obj: "export default { a: 1 };",
  asDefault: "function q() {} export { q as default };",
  asDefaultConst: "const q = () => 1; export { q as default };",
  classStaticName: "export default class { static name = 'custom'; }",
  classStaticMethodName: "export default class { static name() {} }",
};
const defaultUses = [
  "L(typeof d); L(d && d.name);",
  "L(Object.getOwnPropertyNames(d).join());",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(d, 'name')));",
  "L(String(d).slice(0, 30));",
  "L(Object.prototype.toString.call(d));",
  "L(typeof d === 'function' ? d.length : 'nf');",
];
for (const [n, src] of Object.entries(defaultForms)) {
  for (const use of defaultUses) main(`import d from './a.mjs'; ${use}`, { "a.mjs": src });
  main(`import * as ns from './a.mjs'; const d = ns.default; ${defaultUses[0]}`, { "a.mjs": src });
  main(`const { default: d } = await import('./a.mjs'); ${defaultUses[0]}`, { "a.mjs": src });
  main(`import { default as d } from './r.mjs'; ${defaultUses[0]}`, { "r.mjs": "export { default } from './a.mjs';", "a.mjs": src });
  main(`import d from './r.mjs'; ${defaultUses[0]}`, { "r.mjs": "import x from './a.mjs'; export default x;", "a.mjs": src });
}

// 2. export * e export * as: combinações.
section(0.4);
const starProviders = {
  one: "export const a = 1, b = 2;",
  withDefault: "export const a = 1; export default 'd';",
  empty: "",
  fn: "export function f() {} export class C {}",
  conflictA: "export const x = 'A', a = 1;",
  conflictB: "export const x = 'B', b = 2;",
  same: "export { v as x } from './z.mjs';",
};
const starShapes = {
  star: "export * from './p.mjs';",
  starAs: "export * as n from './p.mjs';",
  starAsDefault: "export * as default from './p.mjs';",
  starAsString: "export * as 'a b' from './p.mjs';",
  starTwice: "export * from './p.mjs'; export * from './p.mjs';",
  starOwn: "export * from './p.mjs'; export const a = 'own';",
  starOwnDefault: "export * from './p.mjs'; export default 'od';",
  starAndAs: "export * from './p.mjs'; export * as ns from './p.mjs';",
  starOverImport: "import { a as q } from './p.mjs'; export * from './p.mjs'; export { q as a2 };",
};
const starConsumers = [
  "import * as ns from './r.mjs'; L(Object.keys(ns));",
  "import * as ns from './r.mjs'; L(Object.keys(ns)); L(JSON.stringify(Object.getOwnPropertyDescriptors(ns)));",
  "import * as ns from './r.mjs'; L(ns.a); L(ns.n && Object.keys(ns.n)); L(typeof ns.default);",
  "import * as ns from './r.mjs'; L(Reflect.ownKeys(ns).map(String).join());",
  "const ns = await import('./r.mjs'); L(Object.keys(ns)); L(ns[Symbol.toStringTag]);",
];
for (const [pn, psrc] of Object.entries(starProviders)) {
  for (const [sn, ssrc] of Object.entries(starShapes)) {
    for (const cons of starConsumers) {
      main(cons, { "r.mjs": ssrc, "p.mjs": psrc, "z.mjs": "export const v = 'zz';" });
    }
  }
}
// conflito de export * com e sem consumidor da binding ambígua
for (const name of ["x", "a", "b"]) {
  main(`import { ${name} } from './r.mjs'; L(${name});`, { "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": starProviders.conflictA, "pb.mjs": starProviders.conflictB });
  main(`import * as ns from './r.mjs'; L(ns.${name}); L(Object.keys(ns));`, { "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": starProviders.conflictA, "pb.mjs": starProviders.conflictB });
  main(`export { ${name} } from './r.mjs';`, { "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": starProviders.conflictA, "pb.mjs": starProviders.conflictB });
  main(`const ns = await import('./r.mjs'); L(${name} in ns);`.replace(`${name} in ns`, `'${name}' in ns`), { "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": starProviders.conflictA, "pb.mjs": starProviders.conflictB });
}

// 3. Namespace objects: operações x formas de provedor.
section(0.4);
const nsProviders = {
  consts: "export const z = 1, a = 2, m = 3;",
  withDefault: "export const b = 1; export default 'd';",
  digits: "const v = 1; export { v as '10', v as '2', v as 'a', v as '1' };",
  unicode: "export const é = 1, e = 2, 𝒳 = 3, $ = 4, _ = 5, Z = 6;",
  letVar: "export let l = 1; export var w = 2;",
  fnCls: "export function f() {} export class C {}",
  tdz: "export let late = 1; const x = 0;",
  str: "const v = 1; export { v as 'a b', v as '' };",
  empty: "export {};",
};
const nsOps = [
  "L(Object.keys(ns)); L(Object.getOwnPropertyNames(ns)); L(Reflect.ownKeys(ns).map(String).join());",
  "L(ns[Symbol.toStringTag]); L(Object.prototype.toString.call(ns)); L(typeof ns);",
  "L(Object.isExtensible(ns)); L(Object.isFrozen(ns)); L(Object.isSealed(ns));",
  "L(Object.getPrototypeOf(ns)); L(Reflect.getPrototypeOf(ns));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptors(ns)));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag)));",
  "'use strict'; " + T("ns.zz = 1"),
  T("(() => { 'use strict'; ns.a = 1; })()"),
  T("(() => { 'use strict'; ns.nope = 1; })()"),
  T("(() => { 'use strict'; delete ns.a; })()"),
  T("(() => { 'use strict'; delete ns.nope; })()"),
  T("(() => { 'use strict'; delete ns[Symbol.toStringTag]; })()"),
  T("(() => { 'use strict'; ns[Symbol.toStringTag] = 1; })()"),
  "L(Reflect.set(ns, 'a', 1)); L(Reflect.set(ns, 'nope', 1)); L(Reflect.deleteProperty(ns, 'a')); L(Reflect.deleteProperty(ns, 'nope'));",
  "L(Reflect.defineProperty(ns, 'a', { value: 1 })); L(Reflect.defineProperty(ns, 'nope', { value: 1 }));",
  T("Object.defineProperty(ns, 'a', { value: 99 })"),
  T("Object.defineProperty(ns, 'nope', { value: 99 })"),
  "L(Reflect.setPrototypeOf(ns, null)); L(Reflect.setPrototypeOf(ns, {})); L(Reflect.preventExtensions(ns));",
  T("Object.setPrototypeOf(ns, {})"),
  T("Object.preventExtensions(ns)"),
  T("Object.freeze(ns)"),
  T("Object.seal(ns)"),
  "L(Reflect.has(ns, 'a')); L('nope' in ns); L(Reflect.has(ns, Symbol.toStringTag)); L(Reflect.has(ns, Symbol.iterator));",
  "L(Object.hasOwn(ns, 'a')); L(Object.hasOwn(ns, Symbol.toStringTag)); L(ns.hasOwnProperty === undefined); L(ns.toString === undefined);",
  "L(JSON.stringify(ns)); L(JSON.stringify(Object.entries(ns)));",
  "const k = []; for (const x in ns) k.push(x); L(k); L(Object.values(ns).length);",
  "L(Object.assign({}, ns).constructor === Object); L(Object.keys({ ...ns }));",
  "L(ns.nope); L(ns[Symbol.iterator]); L(ns['__proto__']); L(ns.constructor);",
  "L(typeof ns.then); L(ns.then);",
  T("new ns()"),
  T("ns()"),
  T("Object.create(ns).x = 1"),
  "const o = Object.create(ns); L(Object.keys(o)); L(o.a === ns.a);",
  "const p = new Proxy(ns, {}); L(Object.keys(p)); L(p.a === ns.a);",
  "L(Object.getOwnPropertyNames(ns).length); L(Object.getOwnPropertySymbols(ns).length);",
  "L(Object.entries(Object.getOwnPropertyDescriptors(ns)).map(([k, d]) => k + ':' + d.writable + d.enumerable + d.configurable).join());",
  "L(ns === ns); L(Object.is(ns, ns));",
];
for (const [pn, psrc] of Object.entries(nsProviders)) {
  for (const op of nsOps) {
    const bodyOp = op.startsWith("'use strict'") ? op.slice(14) : op;
    main(`import * as ns from './a.mjs'; ${bodyOp}`, { "a.mjs": psrc });
  }
}
// [[Get]] e demais operações em binding em TDZ (ciclo): o namespace observado antes da avaliação do provedor.
const tdzOps = [
  "L(ns.x)", "L('x' in ns)", "L(Object.keys(ns))", "L(Object.getOwnPropertyDescriptor(ns, 'x'))",
  "L(Reflect.has(ns, 'x'))", "L(Reflect.get(ns, 'x'))", "L(JSON.stringify(ns))", "L(Object.values(ns))",
  "L(Object.entries(ns))", "L(Object.hasOwn(ns, 'x'))", "L(typeof ns.x)", "L(Reflect.set(ns, 'x', 1))",
  "L(Object.getOwnPropertyDescriptors(ns))", "L(Reflect.ownKeys(ns).length)", "L(delete ns.x)", "L(ns.y)",
  "(() => { 'use strict'; ns.x = 1; })()", "(() => { 'use strict'; delete ns.x; })()",
  "L(Reflect.defineProperty(ns, 'x', { value: 1, writable: true, enumerable: true, configurable: false }))",
  "L(Reflect.defineProperty(ns, 'x', { value: undefined }))",
];
const tdzDecls = {
  const: "export const x = 1;", let: "export let x = 1;", class: "export class x {}",
  var: "export var x = 1;", fn: "export function x() {}", defaultName: "export default 1; export { x }; let x = 1;",
  bound: "let q = 1; export { q as x };", constBound: "const q = 1; export { q as x };",
  classBound: "class q {} export { q as x };", dflt: "const q = 1; export { q as x }; export default q;",
};
for (const [dn, decl] of Object.entries(tdzDecls)) {
  for (const op of tdzOps) {
    main(`import './a.mjs';`, {
      "a.mjs": `import * as ns from './b.mjs'; ${decl.replace(/export \{ x \}; let x = 1;/, "let x2;")}`,
      "b.mjs": `import * as ns from './a.mjs'; ${T(op)} export const y = 1;`,
    });
  }
}

// 4. Ciclos: TDZ em binding exportado antes da avaliação, por forma x observador x via de acesso.
section(0.45);
const cycleDecls = {
  const: "export const v = 'c';", let: "export let v = 'l';", var: "export var v = 'va';",
  class: "export class v {}", fn: "export function v() { return 'f'; }",
  asyncFn: "export async function v() {}", genFn: "export function* v() {}",
  dflt: "export default 'dv'; export const v = 'dflt';", list: "const q = 'lq'; export { q as v };",
};
const cycleAccess = {
  direct: "L(v)", typeofv: "L(typeof v)", call: "L(v())", member: "L(v.name)", ns: "L(ns.v)", nsHas: "L('v' in ns)",
  nsKeys: "L(Object.keys(ns))", assign: "v = 1", incr: "v++", inFn: "(() => L(v))()", viaFn: "L(get())",
};
for (const [dn, decl] of Object.entries(cycleDecls)) {
  for (const [an, acc] of Object.entries(cycleAccess)) {
    main(`import './b.mjs'; L('main');`, {
      "a.mjs": `import './b.mjs'; L('a'); ${decl}`,
      "b.mjs": `import { v } from './a.mjs'; import * as ns from './a.mjs'; const get = () => v; ${T(acc)} L('b');`,
    });
    main(`import './a.mjs'; L('main');`, {
      "a.mjs": `import { v } from './b.mjs'; import * as ns from './b.mjs'; const get = () => v; ${T(acc)} L('a');`,
      "b.mjs": `import './a.mjs'; L('b'); ${decl}`,
    });
  }
}
// hoisting de função entre módulos do ciclo
const hoistVariants = [
  ["export function f() { return 'fa'; }", "import { f } from './a.mjs'; L(typeof f); L(f()); export function g() { return f(); }"],
  ["export async function f() { return 'fa'; }", "import { f } from './a.mjs'; L(typeof f); export function g() { return f(); }"],
  ["export function* f() { yield 1; }", "import { f } from './a.mjs'; L([...f()]);"],
  ["export var f = function () { return 'v'; };", "import { f } from './a.mjs'; L(typeof f);"],
  ["export let f = function () { return 'v'; };", "import { f } from './a.mjs'; " + T("L(typeof f)")],
  ["export default function f() { return 'df'; }", "import f from './a.mjs'; L(typeof f); L(f());"],
  ["export default function () { return 'anon'; }", "import f from './a.mjs'; L(typeof f); L(f.name); L(f());"],
  ["export default class {}", "import f from './a.mjs'; " + T("L(typeof f)")],
  ["export default class K {}", "import f from './a.mjs'; " + T("L(typeof f)")],
  ["export default (function () {});", "import f from './a.mjs'; " + T("L(typeof f)")],
  ["export { f }; function f() { return 'late'; }", "import { f } from './a.mjs'; L(f());"],
  ["export { f as g }; function f() { return 'late'; }", "import { g } from './a.mjs'; L(g());"],
  ["export * from './c.mjs';", "import { f } from './a.mjs'; L(f());"],
  ["export { f } from './c.mjs';", "import { f } from './a.mjs'; L(f());"],
  ["export * as n from './c.mjs';", "import { n } from './a.mjs'; " + T("L(n.f())")],
];
for (const [aSrc, bSrc] of hoistVariants) {
  for (const entry of ["a", "b"]) {
    main(`import './${entry}.mjs'; L('main');`, {
      "a.mjs": `import './b.mjs'; L('a'); ${aSrc}`,
      "b.mjs": `import './a.mjs'; L('b'); ${bSrc}`,
      "c.mjs": "export function f() { return 'cf'; }",
    });
  }
}
// ciclos de três, auto-importação, ciclo via re-export
const cyc3 = [
  ["a", "b", "c"], ["a", "c", "b"],
];
for (const [x, y, z] of cyc3) {
  for (const entry of ["a", "b", "c"]) {
    main(`import './${entry}.mjs'; L('main');`, {
      "a.mjs": `import './${x}.mjs'.replace; `.replace(".replace", "") + "L('a'); export const va = 'A';",
      "b.mjs": `import './${y}.mjs'; L('b'); export const vb = 'B';`,
      "c.mjs": `import './${z}.mjs'; L('c'); export const vc = 'C';`,
    });
  }
}
for (const decl of ["export const v = 1;", "export let v = 1;", "export function v() {}", "export class v {}", "export var v = 1;"]) {
  main(`import { v } from './a.mjs'; L(typeof v);`, { "a.mjs": `import { v as w } from './a.mjs'; ${T("L(typeof w)")} ${decl}` });
  main(`import './a.mjs';`, { "a.mjs": `export * from './a.mjs'; ${decl} L('a');` });
  main(`import './a.mjs';`, { "a.mjs": `import * as ns from './a.mjs'; ${T("L(Object.keys(ns))")} ${decl}` });
  main(`import { v } from './b.mjs'; L(typeof v);`, { "b.mjs": "export { v } from './a.mjs';", "a.mjs": `export { v } from './b.mjs'; ${decl.replace("export ", "export const w = 0; ")}` });
}
main(`import { v } from './a.mjs';`, { "a.mjs": "export { v } from './b.mjs';", "b.mjs": "export { v } from './a.mjs';" });
main(`import { v } from './a.mjs';`, { "a.mjs": "export * from './b.mjs';", "b.mjs": "export * from './a.mjs';" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "export * from './b.mjs'; export const a = 1;", "b.mjs": "export * from './a.mjs'; export const b = 1;" });
main(`import { a, b } from './a.mjs'; L(a + b);`, { "a.mjs": "export * from './b.mjs'; export const a = 1;", "b.mjs": "export * from './a.mjs'; export const b = 1;" });
main(`import { v } from './a.mjs';`, { "a.mjs": "import { v as q } from './b.mjs'; export { q as v };", "b.mjs": "import { v as q } from './a.mjs'; export { q as v };" });

// 5. SyntaxError de ligação: re-export ambíguo, binding inexistente (mensagens exatas).
section(0.5);
const importForms = [
  "import { x } from './a.mjs'; L(x);", "import { x as y } from './a.mjs'; L(y);", "import { 'a b' as y } from './a.mjs'; L(y);",
  "import { x } from './r.mjs'; L(x);", "export { x } from './a.mjs';", "export { x as y } from './a.mjs';",
  "import { default as d } from './a.mjs'; L(d);", "import d from './a.mjs'; L(d);", "import d, { x } from './a.mjs';",
  "export { default } from './a.mjs';", "export { x } from './r.mjs';", "import * as n from './a.mjs'; L(n.x);",
  "const n = await import('./a.mjs'); L(n.x);", "import { x } from './nope.mjs';", "import './nope.mjs';",
  "import { x } from './r.mjs'; import { y } from './a.mjs';", "import { x as default } from './a.mjs';",
];
const providers = [
  "export const y = 1;", "", "export default 1;", "export {};", "export { y as x };".replace("export { y as x };", "const y = 1; export { y as x };"),
  "export * from './b.mjs';", "export { z } from './b.mjs';", "export * as x from './b.mjs';",
];
for (const imp of importForms) {
  for (const prov of providers) {
    main(imp, { "a.mjs": prov, "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "b.mjs": "export const x = 'b', z = 1;" });
  }
}
// ambiguidade verdadeira, em profundidade e com nome string
for (const nm of ["x", "default", "'a b'"]) {
  const ex = nm === "default" ? "export { v as default };" : `export { v as ${nm} };`;
  const mk = (v) => `const v = '${v}'; ${ex}`;
  const imp = nm === "default" ? "import x from './r.mjs'; L(x);" : nm.startsWith("'") ? "import { 'a b' as x } from './r.mjs'; L(x);" : "import { x } from './r.mjs'; L(x);";
  main(imp, { "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": mk("a"), "pb.mjs": mk("b") });
  main(imp, { "r.mjs": "export * from './m.mjs'; export * from './pb.mjs';", "m.mjs": "export * from './pa.mjs';", "pa.mjs": mk("a"), "pb.mjs": mk("b") });
  main(imp.replace("./r.mjs", "./q.mjs"), { "q.mjs": "export * from './r.mjs';", "r.mjs": "export * from './pa.mjs'; export * from './pb.mjs';", "pa.mjs": mk("a"), "pb.mjs": mk("b") });
}

// 6. Live bindings por forma x mutação.
section(0.5);
const liveForms = {
  let: ["export let v = 0;", "v++"], letAssign: ["export let v = 0;", "v = v + 10"],
  var: ["export var v = 0;", "v += 3"], fnRebind: ["export function v() { return 0; }", "v = () => 1"],
  classRebind: ["export class v {}", "v = null"], listAs: ["let q = 0; export { q as v };", "q = 5"],
  letDestr: ["export let [v] = [0];", "v = 8"], letObj: ["export let { v } = { v: 0 };", "v = 9"],
  forLoop: ["export let v = 0; for (let i = 0; i < 3; i++) v += i;", "v = 100"],
  defaultList: ["let v = 0; export { v as default };", "v = 11"],
};
const liveUses = [
  "L(v); mut(); L(v); mut(); L(v);", "const get = () => v; L(get()); mut(); L(get());",
  "L(ns.v); mut(); L(ns.v); L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'v')));",
  "const d = Object.getOwnPropertyDescriptor(ns, 'v'); L(d.value); mut(); L(Object.getOwnPropertyDescriptor(ns, 'v').value);",
  T("v = 1") + " " + T("v++") + " L(typeof v);",
];
for (const [n, [decl, mutation]] of Object.entries(liveForms)) {
  const name = n === "defaultList" ? "default as v" : "v";
  for (const use of liveUses) {
    main(`import { ${name} } from './a.mjs'; import * as ns from './a.mjs'; import { mut } from './a.mjs'; ${use}`, { "a.mjs": `${decl} export function mut() { ${mutation}; }`.replace(/export function mut\(\) \{ (.*?); \}/, (m, mu) => `export function mut() { ${mu}; }`) });
    main(`import { v } from './r.mjs'; import { mut } from './a.mjs'; L(v); mut(); L(v);`.replace("{ v }", n === "defaultList" ? "{ default as v }" : "{ v }").replace("./r.mjs", "./r.mjs"), {
      "r.mjs": n === "defaultList" ? "export { default } from './a.mjs';" : "export { v } from './a.mjs';",
      "a.mjs": `${decl} export function mut() { ${mutation}; }`,
    });
    main(`import { v } from './r.mjs'; import { mut } from './a.mjs'; L(v); mut(); L(v);`.replace("{ v }", n === "defaultList" ? "{ default as v }" : "{ v }"), {
      "r.mjs": "export * from './a.mjs';" + (n === "defaultList" ? " export { default } from './a.mjs';" : ""),
      "a.mjs": `${decl} export function mut() { ${mutation}; }`,
    });
  }
}

// 7. import.meta.
section(0.6);
const metaUses = [
  "L(typeof import.meta);", "L(Object.getPrototypeOf(import.meta));", "L(Object.keys(import.meta));",
  "L(Object.getOwnPropertyNames(import.meta).sort().join());", "L(Object.isExtensible(import.meta));",
  "L(import.meta === import.meta);", "L(import.meta.url);", "L(Object.prototype.toString.call(import.meta));",
  "import.meta.custom = 1; L(import.meta.custom); L(Object.keys(import.meta).includes('custom'));",
  "L(delete import.meta.nope); L(import.meta.nope);", "L(Object.isFrozen(import.meta)); L(Object.isSealed(import.meta));",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(import.meta, 'url')));",
  "L(typeof import.meta.resolve);", "L(typeof import.meta.main);", "L('url' in import.meta);",
  "const m = import.meta; L(m === import.meta);", "L(((m) => m === import.meta)(import.meta));",
  "L(typeof import.meta.dir + typeof import.meta.dirname + typeof import.meta.file + typeof import.meta.path);",
  "L(Reflect.ownKeys(import.meta).length > 0);",
];
for (const use of metaUses) {
  main(use, {});
  main(`import './a.mjs';`, { "a.mjs": use });
  main(`await import('./a.mjs');`, { "a.mjs": use });
  main(`import { m } from './a.mjs'; L(m === import.meta);`, { "a.mjs": `export const m = import.meta; ${use}` });
}
main(`function f() { return import.meta; } L(typeof f());`, {});
main(`const g = () => import.meta.url; L(typeof g());`, {});
main(`L(eval('typeof import.meta'));`, {});
main(`L(new Function('return typeof import.meta')());`, {});
main(`const m = import.meta; m.x = 1; L(import.meta.x);`, {});
main(`L(typeof import.meta?.url); L(import.meta?.nope);`, {});
main(`const { url } = import.meta; L(typeof url);`, {});
main(`L(import . meta === import.meta);`, {});

// 8. import() dinâmico.
section(0.5);
const dynBodies = [
  "const p = import('./a.mjs'); L(p instanceof Promise); L(Object.prototype.toString.call(p)); await p;",
  "const ns = await import('./a.mjs'); L(Object.keys(ns));",
  "import('./a.mjs').then((ns) => L('then ' + Object.keys(ns)));  L('sync');",
  "L('before'); await import('./a.mjs'); L('after');",
  "const [x, y] = await Promise.all([import('./a.mjs'), import('./a.mjs')]); L(x === y);",
  "const x = await import('./a.mjs'); const y = await import('./a.mjs'); L(x === y);",
  "const x = await import('./a.mjs'); const y = await import('./b.mjs'); L(x === y); L(Object.keys(y));",
  "try { await import('./nope.mjs'); } catch (e) { L(e.name); L(e.message.slice(0, 20)); }",
  "const p = import('./nope.mjs'); L(p instanceof Promise); p.catch((e) => L('caught ' + e.name));",
  "try { await import('./bad.mjs'); } catch (e) { L(e.name + ': ' + e.message); }",
  "try { await import('./throws.mjs'); } catch (e) { L(e.name + ': ' + e.message); } try { await import('./throws.mjs'); } catch (e) { L('again ' + e.name + ': ' + e.message); }",
  "await import('./a.mjs'); await import('./a.mjs'); L('twice');",
  "const k = './a.mjs'; const ns = await import(k); L(Object.keys(ns));",
  "const ns = await import('./' + 'a' + '.mjs'); L(Object.keys(ns));",
  "try { await import(); } catch (e) { L(e.name); }",
  "try { await import(undefined); } catch (e) { L(e.name + ': ' + e.message.slice(0, 10)); }",
  "try { await import({ toString() { throw new Error('ts'); } }); } catch (e) { L(e.message); }",
  "const ns = await import({ toString() { return './a.mjs'; } }); L(Object.keys(ns));",
  "L(typeof import); ".replace("L(typeof import); ", "") + "L(import.length === undefined);".replace("L(import.length === undefined);", ""),
  "const f = async () => (await import('./a.mjs')).a; L(await f());",
  "const ns = await import('./cyc1.mjs'); L(Object.keys(ns)); L(ns.c1);",
  "const ns = await import('./tla.mjs'); L('got ' + ns.t);",
  "const p = import('./tla.mjs'); L('pending'); L(await p === await p);",
  "try { await import('./tlarej.mjs'); } catch (e) { L('rej ' + e.message); } try { await import('./tlarej.mjs'); } catch (e) { L('rej2 ' + e.message); }",
  "const order = []; await Promise.all([import('./o1.mjs'), import('./o2.mjs')]); L('done');",
  "const ns = await import('./a.mjs'); L(ns.a); ns.a; L(Object.getOwnPropertyDescriptor(ns, 'a').value);",
  "L(await import('./a.mjs').then(() => 'ok', () => 'ko'));",
  "L(await import('./nope.mjs').then(() => 'ok', () => 'ko'));",
  "import('./a.mjs'); import('./a.mjs'); L('queued');",
  "const x = import('./a.mjs'); const y = import('./a.mjs'); L(x === y);",
  "const ns = await import('./a.mjs', { with: {} }); L(Object.keys(ns));",
  "try { await import('./a.mjs', { with: { type: 'bogus' } }); } catch (e) { L(e.name + ': ' + e.message.slice(0, 40)); }",
  "try { await import('./a.mjs', 5); } catch (e) { L(e.name + ': ' + e.message.slice(0, 40)); }",
  "const ns = await import('./a.mjs', undefined); L(Object.keys(ns));",
];
const dynFiles = {
  "a.mjs": "L('eval a'); export const a = 1;", "b.mjs": "L('eval b'); export const b = 2;", "bad.mjs": "export const = 1;",
  "throws.mjs": "L('eval throws'); throw new TypeError('boom');", "cyc1.mjs": "import './cyc2.mjs'; export const c1 = 'c1';",
  "cyc2.mjs": "import { c1 } from './cyc1.mjs'; L('cyc2 ' + typeof c1);".replace("typeof c1", "'x'"),
  "tla.mjs": "L('tla start'); export const t = await Promise.resolve('T'); L('tla end');",
  "tlarej.mjs": "L('tlarej start'); await Promise.reject(new Error('R')); export const t = 1;",
  "o1.mjs": "L('o1 start'); await null; L('o1 end');", "o2.mjs": "L('o2 start'); await null; await null; L('o2 end');",
};
for (const body of dynBodies) {
  main(body, dynFiles);
  main(`import './pre.mjs'; ${body}`, { ...dynFiles, "pre.mjs": "L('pre');" });
  main(`import './sub/inner.mjs';`, { ...dynFiles, "sub/inner.mjs": body.replace(/\.\/(a|b|bad|throws|nope|cyc1|tla|tlarej|o1|o2)\.mjs/g, "../$1.mjs") });
}
main(`L('main'); import('./a.mjs'); L('end');`, { "a.mjs": "L('a');" });
main(`import('./a.mjs'); L('end');`, { "a.mjs": "L('a'); await null; L('a2');" });
main(`await import('./a.mjs');`, { "a.mjs": "const ns = await import('./b.mjs'); L(ns.b);", "b.mjs": "export const b = 'bb';" });
main(`await import('./a.mjs');`, { "a.mjs": "import('./a.mjs').then(() => L('self')); export const a = 1;" });
main(`const ns = await import('./a.mjs'); L(ns.default);`, { "a.mjs": "export default import.meta.url.endsWith('/a.mjs');" });
main(`const f = () => import('./a.mjs'); L((await f()).a === (await f()).a);`, { "a.mjs": "export const a = {};" });
main(`const ns = await import('./a.mjs'); L(ns.a === (await import('./a.mjs')).a);`, { "a.mjs": "export const a = {};" });
main(`import * as s from './a.mjs'; const d = await import('./a.mjs'); L(s === d);`, { "a.mjs": "export const a = 1;" });

// 9. Top-level await: ordem entre irmãos, ciclo, rejeição.
section(0.6);
const tlaAwaits = ["", "await null;", "await null; await null;", "await Promise.resolve().then(() => 1);", "await Promise.resolve();", "await 1; await 2; await 3;"];
for (const a of tlaAwaits) {
  for (const b of tlaAwaits) {
    main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": `L('a1'); ${a} L('a2');`, "b.mjs": `L('b1'); ${b} L('b2');` });
    main(`import './a.mjs'; import './b.mjs'; import './c.mjs'; L('main');`, { "a.mjs": `L('a1'); ${a} L('a2');`, "b.mjs": `L('b1'); ${b} L('b2');`, "c.mjs": "L('c');" });
  }
}
for (const a of tlaAwaits) {
  main(`import './a.mjs'; L('main');`, { "a.mjs": `import './b.mjs'; L('a1'); ${a} L('a2');`, "b.mjs": "import './c.mjs'; L('b');", "c.mjs": `L('c1'); ${a} L('c2');` });
  main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": `import './c.mjs'; L('a1'); ${a} L('a2');`, "b.mjs": "import './c.mjs'; L('b');", "c.mjs": `L('c1'); ${a} L('c2');` });
  main(`import './a.mjs'; L('main');`, { "a.mjs": `import './b.mjs'; L('a1'); ${a} L('a2');`, "b.mjs": `import './a.mjs'; L('b1'); ${a} L('b2');` });
  main(`import './b.mjs'; L('main');`, { "a.mjs": `import './b.mjs'; L('a1'); ${a} L('a2');`, "b.mjs": `import './a.mjs'; L('b1'); ${a} L('b2');` });
  main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": `import { y } from './b.mjs'; ${a} export const x = 'x' + y;`, "b.mjs": `${a} export const y = 'y';` });
  main(`import './a.mjs'; L('main');`, { "a.mjs": `import './b.mjs'; import './c.mjs'; L('a');`, "b.mjs": `${a} L('b');`, "c.mjs": "L('c');" });
  main(`import './a.mjs'; L('main');`, { "a.mjs": `import './b.mjs'; import './c.mjs'; L('a');`, "b.mjs": "L('b');", "c.mjs": `${a} L('c');` });
  main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": `import './d.mjs'; L('a');`, "b.mjs": `import './d.mjs'; L('b');`, "d.mjs": `L('d1'); ${a} L('d2');` });
  main(`import { v } from './a.mjs'; L(v);`, { "a.mjs": `export * from './b.mjs';`, "b.mjs": `${a} export const v = 'bv';` });
  main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(ns.v);`, { "a.mjs": `import * as n from './b.mjs'; export const v = n.v;`, "b.mjs": `${a} export const v = 'bv';` });
}
// ciclos mais elaborados com TLA
main(`import './a.mjs';`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A'; await null; L('a ' + b);", "b.mjs": "import { a } from './a.mjs'; export const b = 'B'; " + T("L('b ' + a)") });
main(`import './a.mjs';`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A'; L('a ' + b);", "b.mjs": "import { a } from './a.mjs'; export const b = 'B'; await null; " + T("L('b ' + a)") });
main(`import './a.mjs'; import './b.mjs';`, { "a.mjs": "import './c.mjs'; await null; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; L('c');" });
main(`import './b.mjs'; import './a.mjs';`, { "a.mjs": "import './c.mjs'; await null; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; L('c');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; import './c.mjs'; await null; L('a');", "b.mjs": "import './c.mjs'; await null; L('b');", "c.mjs": "import './b.mjs'; L('c');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './c.mjs'; await null; L('b');", "c.mjs": "import './d.mjs'; L('c');", "d.mjs": "import './b.mjs'; L('d');" });
// rejeição propaga
const rejKinds = ["throw new Error('E');", "await Promise.reject(new Error('E'));", "await null; throw new Error('E');", "throw 5;", "await Promise.reject(7);", "await null; await Promise.reject(new TypeError('T'));", "null.x;", "undefinedVar;"];
for (const k of rejKinds) {
  main(`import './a.mjs'; L('main');`, { "a.mjs": `L('a'); ${k}` });
  main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": `L('a'); ${k}`, "b.mjs": "L('b'); await null; L('b2');" });
  main(`import './b.mjs'; import './a.mjs'; L('main');`, { "a.mjs": `L('a'); ${k}`, "b.mjs": "L('b'); await null; L('b2');" });
  main(`import './b.mjs'; L('main');`, { "a.mjs": `L('a'); ${k}`, "b.mjs": "import './a.mjs'; L('b');" });
  main(`import './b.mjs'; L('main');`, { "a.mjs": `L('a'); ${k}`, "b.mjs": "import './a.mjs'; import './c.mjs'; L('b');", "c.mjs": "L('c'); await null;" });
  main(`try { await import('./a.mjs'); } catch (e) { L('c1 ' + (e && e.message)); } try { await import('./a.mjs'); } catch (e) { L('c2 ' + (e && e.message)); }`, { "a.mjs": `L('a'); ${k}` });
  main(`import './a.mjs';`, { "a.mjs": `import './b.mjs'; L('a');`, "b.mjs": `L('b'); ${k}` });
  main(`import './a.mjs';`, { "a.mjs": `import './b.mjs'; L('a');`, "b.mjs": `import './a.mjs'; L('b'); ${k}` });
  main(`const r = await Promise.allSettled([import('./a.mjs'), import('./b.mjs')]); L(r.map((x) => x.status)); L('main');`, { "a.mjs": `${k}`, "b.mjs": "await null; L('b');" });
}
// await em posições
main(`L(typeof await 1);`, {});
main(`for await (const x of [Promise.resolve(1), 2]) L(x);`, {});
main(`for await (const x of (async function* () { yield 'g1'; yield 'g2'; })()) L(x);`, {});
main(`const f = async () => { await null; return 'f'; }; L(await f());`, {});
main(`L(await { then(r) { r('thenable'); } });`, {});
main(`for (let i = 0; i < 3; i++) await Promise.resolve(); L('timer');`, {});
main(`label: { L('in'); break label; } await null; L('out');`, {});
main(`if (true) { await null; L('if'); }`, {});
main(`class K { static x = 1; } L(await K.x);`, {});
main(`const v = await import('./a.mjs'); L(v.a);`, { "a.mjs": "export const a = await 'A';" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "export let a; a = await 'A'; " });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "export let a = 'pre'; await null; a = 'post';" });
main(`import * as ns from './a.mjs'; L(ns.a); await null; await null; L(ns.a);`, { "a.mjs": "export let a = 'pre'; Promise.resolve().then(() => { a = 'timer'; });" });
main(`import { f } from './a.mjs'; L(await f());`, { "a.mjs": "export const f = async () => await 'in';" });
main(`const x = await 1, y = await 2; L(x + y);`, {});
main(`function f() { return await 1; }`, {});
main(`async function f() { return await 1; } L(await f());`, {});
main(`const await_ = 1; L(await_);`, {});

// 10. strict, this, await reservado.
section(0.5);
const strictProbes = [
  "L(typeof this); L(this === undefined);", "undeclared = 1;", "L((function () { return this; })());", "L((() => this)());",
  "L(typeof arguments);", "with ({}) {}", "delete Object.prototype;", "var public = 1;", "var eval = 1;", "var arguments = 1;",
  "function f(a, a) {}", "L(010);", "L('\\07');", "L(((a) => { a = 2; return arguments; }).length);".replace("arguments", "a"),
  "L(Object.getOwnPropertyDescriptor(globalThis, 'x'));", "var x = 1; L(globalThis.x);", "function g() {} L(typeof globalThis.g);",
  "let yield_ = 1; var yield = 2;", "var await = 1;", "var aw = 1; L(typeof await);", "var let_ = 1; var let = 2;", "var static = 1;",
  "var implements = 1;", "var interface = 1;", "var package = 1;", "var private = 1;", "var protected = 1;",
  "L(typeof module); L(typeof exports); L(typeof __filename); L(typeof __dirname);",
  "Object.freeze([1]).push(2);", "'abc'.length = 1;", "undefined = 1;", "NaN = 1;", "Object.defineProperty({}, 'a', { value: 1 }).a = 2;",
  "L(typeof globalThis); L(typeof window); L(typeof self);", "L(eval('var ev = 1; typeof ev')); L(typeof ev);",
  "L(new Function('return this')() === globalThis);", "function* g() { yield = 1; }", "async function af() { var await = 1; }",
  "const f = async function await() {};", "class await {}", "function await() {}", "var yield = 1;", "L(((await) => await)(1));",
  "label: await: 1;", "L({ await: 1 }.await);", "const o = { await }; ", "import.meta; L('im');", "L(this);", "L(typeof await);",
  "if (1) { function inner() {} } L(typeof inner);", "try { x = 1; } catch (e) { L(e.name); } var x;",
];
for (const probe of strictProbes) {
  main(probe, {});
  main(`import './a.mjs';`, { "a.mjs": probe });
  main(`import './a.mjs'; export {};`, { "a.mjs": `L('a start'); ${probe}` });
}
for (const word of ["await", "yield", "let", "static", "enum", "implements", "package", "arguments", "eval"]) {
  main(`const ${word} = 1;`, {});
  main(`function ${word}() {}`, {});
  main(`import ${word} from './a.mjs';`, { "a.mjs": "export default 1;" });
  main(`import { x as ${word} } from './a.mjs';`, { "a.mjs": "export const x = 1;" });
  main(`export { ${word} }; let ${word}_ = 1;`, {});
  main(`export const ${word} = 1;`, {});
  main(`export function ${word}() {}`, {});
  main(`import * as ${word} from './a.mjs';`, { "a.mjs": "" });
  main(`L(typeof ${word});`, {});
  main(`({ ${word}: 1 }); L('prop');`, {});
  main(`class C { ${word}() {} } L('method');`, {});
  main(`export { x as ${word} }; var x = 1;`, {});
  main(`import { ${word} as y } from './a.mjs'; L(y);`, { "a.mjs": `var x = 1; export { x as ${word} };` });
}

// 11. import attributes.
section(1);
const attrForms = [
  "import data from './d.json' with { type: 'json' }; L(JSON.stringify(data)); L(typeof data);",
  "import * as ns from './d.json' with { type: 'json' }; L(Object.keys(ns)); L(JSON.stringify(ns.default));",
  "import { a } from './d.json' with { type: 'json' }; L(a);",
  "import data from './d.json'; L(JSON.stringify(data));",
  "import data from './d.json' with { type: 'bogus' }; L(typeof data);",
  "import data from './d.json' with { }; L(typeof data);",
  "import data from './d.json' with { type: 'json', type: 'json' };",
  "import data from './d.json' with { foo: 'bar' };",
  "import data from './d.json' with { 'type': 'json' }; L(typeof data);",
  "import data from './d.json' assert { type: 'json' }; L(typeof data);",
  "import data from './a.mjs' with { type: 'json' }; L(typeof data);",
  "import data from './d.json' with { type: 1 };",
  "export { default as d } from './d.json' with { type: 'json' }; L('re');",
  "export * from './d.json' with { type: 'json' }; L('re');",
  "export * as j from './d.json' with { type: 'json' }; L('re');",
  "const ns = await import('./d.json', { with: { type: 'json' } }); L(JSON.stringify(ns.default));",
  "const ns = await import('./d.json'); L(JSON.stringify(ns.default));",
  "try { await import('./d.json', { with: { type: 'bogus' } }); } catch (e) { L(e.name); }",
  "import './d.json' with { type: 'json' }; L('side');",
  "import a from './d.json' with { type: 'json' }; import b from './d.json' with { type: 'json' }; L(a === b);",
  "import data from './arr.json' with { type: 'json' }; L(Array.isArray(data)); L(data.length);",
  "import data from './str.json' with { type: 'json' }; L(typeof data); L(data);",
  "import data from './num.json' with { type: 'json' }; L(typeof data); L(data);",
  "import data from './null.json' with { type: 'json' }; L(data);",
  "import data from './bad.json' with { type: 'json' }; L(data);",
  "import * as ns from './d.json' with { type: 'json' }; L(Object.prototype.toString.call(ns)); L(Object.isExtensible(ns));",
  "import data from './d.json' with { type: 'json' }; data.a = 2; L(data.a);",
  "import data from './d.json' with { type: 'json' }; L(Object.isFrozen(data));",
  "import with from './a.mjs';",
  "import x from './a.mjs' with { type: 'javascript' }; L(typeof x);",
  "import x from './a.mjs' with { type: 'js' }; L(typeof x);",
];
for (const body of attrForms) {
  main(body, { "d.json": '{"a":1,"b":[2]}', "a.mjs": "export default 1;", "arr.json": "[1,2,3]", "str.json": '"s"', "num.json": "7", "null.json": "null", "bad.json": "{" });
}

// 12. Erros de parse e de declaração de módulo.
const parseErrors = [
  "export const a = 1; export const a = 2;", "export { a }; export { a }; const a = 1;", "export { a, a }; const a = 1;",
  "export { a as b }; export { c as b }; const a = 1, c = 2;", "export default 1; export default 2;",
  "export default 1; export { x as default }; const x = 1;", "export function f() {} export function f() {}",
  "export class C {} export class C {}", "export var v; export var v;", "export let l; export var l;",
  "import { a } from './a.mjs'; import { a } from './a.mjs';", "import { a } from './a.mjs'; import { b as a } from './a.mjs';",
  "import a from './a.mjs'; import a from './a.mjs';", "import * as a from './a.mjs'; import * as a from './a.mjs';",
  "import { a } from './a.mjs'; const a = 1;", "import { a } from './a.mjs'; let a;", "import { a } from './a.mjs'; var a;",
  "import { a } from './a.mjs'; function a() {}", "import { a } from './a.mjs'; class a {}", "const a = 1; import { a } from './a.mjs';",
  "var a; import { a } from './a.mjs';", "import a, { a } from './a.mjs';", "import a, * as a from './a.mjs';",
  "export { x };", "export { x as y };", "export { x }; let y;", "export { x, y }; const x = 1;", "export { default };",
  "export { default as d };", "export { x as default };", "export { 'a b' };", "export { 'a b' as c }; ", "export { 'a b' } from './a.mjs';",
  "export { x }; function f() { var x; }", "export { x }; { let x; }", "export { x }; var x;", "export { x }; function x() {}",
  "export { x }; class x {}", "export { x }; import x from './a.mjs';", "export { x }; import { x } from './a.mjs';",
  "export { x }; import * as x from './a.mjs';", "export { x as y, z as y }; let x, z;",
  "export { 'a\\uD800' };", "export { 'a\\uD800' } from './a.mjs';", "export { x as 'a\\uD800' }; let x;", "import { 'a\\uD800' as y } from './a.mjs';",
  "import { 'a b' } from './a.mjs';", "import { default } from './a.mjs';", "import { if } from './a.mjs';", "import { if as x } from './a.mjs';",
  "import { x as if } from './a.mjs';", "import { x as default } from './a.mjs';", "export { if };", "export { if } from './a.mjs';",
  "export { x as if }; let x;", "export default;", "export default var x;", "export default let x;", "export default const x = 1;",
  "export default async () => 1; L('ok');", "export default async function () {}", "export default async\nfunction f() {}",
  "export default function f() {} export { f as f2 };", "export default class {} L('ok');", "export default (1, 2);", "export default 1, 2;",
  "export default function* () {}", "export default x => x;", "export default a = 1;", "export default { a } ; var a;",
  "export * from;", "export * as from './a.mjs';", "export * as default from './a.mjs';", "export * as 'a b' from './a.mjs';",
  "export * as x, y from './a.mjs';", "export *;", "export * from './a.mjs' L('x');", "export {};", "export {} from './a.mjs';",
  "import;", "import {};", "import {} from;", "import from './a.mjs';", "import x;", "import x from;", "import * from './a.mjs';",
  "import * as from './a.mjs';", "import x, from './a.mjs';", "import {x,} from './a.mjs';", "import {,} from './a.mjs';",
  "import { x as } from './a.mjs';", "import 1 from './a.mjs';", "import 'a.mjs' from './a.mjs';", "import './a.mjs' L('x');",
  "if (1) import './a.mjs';", "if (1) export const x = 1;", "{ import './a.mjs'; }", "{ export const x = 1; }",
  "function f() { import './a.mjs'; }", "function f() { export const x = 1; }", "label: export const x = 1;", "L(1); export const x = 2; L(3);",
  "() => { export default 1; };", "try { export const x = 1; } catch {}", "export const { a, a } = {};", "export const [a, a] = [];",
  "export let a, a;", "export { a }; export const a = 1; export { a };", "export { a as b, c as b }; const a = 1, c = 2;",
  "export { a }; export { a as b }; export const a = 1; L('ok');", "export default function () {} export default function () {}",
  "export function default() {}", "export class default {}", "export const default = 1;", "export let await = 1;", "export var yield = 1;",
  "export function await() {}", "export class let {}", "export const eval = 1;", "export function arguments() {}",
  "import { eval } from './a.mjs';", "import { arguments } from './a.mjs';", "import { eval as x } from './a.mjs';", "import { x as eval } from './a.mjs';",
  "import { x as arguments } from './a.mjs';", "import eval from './a.mjs';", "import * as arguments from './a.mjs';",
  "import { x as await } from './a.mjs';", "import await from './a.mjs';", "import * as await from './a.mjs';", "import { await } from './a.mjs';",
  "import { yield } from './a.mjs';", "import { let } from './a.mjs';", "import { static } from './a.mjs';",
  "import x from './a.mjs' with { type: 'json' };", "import x from './a.mjs' with { type: 'json' } L(1);", "import x from './a.mjs' with;",
  "import x from './a.mjs' with {;", "import x from './a.mjs' with { type };", "import x from './a.mjs' with { type: };",
  "import x from './a.mjs' with { type: 'json', };", "import x from './a.mjs' with { 1: 'json' };", "import x from './a.mjs' with { default: 'a' };",
  "import x from './a.mjs' with { type: `json` };", "import x from './a.mjs' with { 'type': 'a', type: 'b' };",
  "export { x } from './a.mjs' with { type: 'json' };", "export * from './a.mjs' with { type: 'json' };",
  "new.target;", "super.x;", "return;", "yield 1;", "await;", "await 1; L('ok');", "break;", "continue;", "L(1); return 2;",
  "import.meta = 1;", "import.meta++;", "import.meta.x = 1; L('ok');", "import.meta();", "new import.meta();", "import.metaa;", "import.meta.url = 1;",
  "import.foo;", "import(;", "import();", "import(1, 2, 3);", "import(...a);", "new import('./a.mjs');", "import('./a.mjs')();", "import?.('./a.mjs');",
  "L(typeof import('./a.mjs'));", "const i = import; ", "import = 1;", "import++;", "(import)('./a.mjs');", "x = import.meta.url; var x;",
  "<!-- html comment\nL(1);", "L(1);\n--> not allowed", "/* x */ --> y", "L('a');\n<!-- x",
  "var \\u0061wait = 1;", "L(\\u0061wait);", "aw\\u0061it: 1;", "var aw\\u0061it;", "({ aw\\u0061it: 1 });", "function* g() { yi\\u0065ld 1; }",
  "export { x as aw\\u0061it }; var x;", "export { x as \\u0061wait }; var x;", "import { \\u0061wait as x } from './a.mjs';",
  "import { x as \\u0061wait } from './a.mjs';", "var l\\u0065t = 1;", "var st\\u0061tic = 1;", "var impl\\u0065ments = 1;", "var \\u{61}wait;",
];
for (const src of parseErrors) {
  main(src, { "a.mjs": "export const a = 1, x = 2; export default 3;" });
}
main(`import './a.mjs'; L('main');`, { "a.mjs": "L('a'); export const a = 1; export const a = 2;" });
main(`import './b.mjs'; import './a.mjs'; L('main');`, { "a.mjs": "export const = 1;", "b.mjs": "L('b');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a');", "b.mjs": "export { nope };" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a');", "b.mjs": "import { nope } from './a.mjs';" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "export { nope };" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './c.mjs'; import { nope } from './a.mjs';", "c.mjs": "L('c');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "L('a'); throw new Error('x');", "b.mjs": "" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name + ': ' + e.message); } try { await import('./b.mjs'); } catch (e) { L(e.name + ': ' + e.message); }`, { "a.mjs": "export const a = 1; export const a = 2;", "b.mjs": "import { q } from './c.mjs';", "c.mjs": "export const r = 1;" });

// 13. Grafos aleatórios (determinísticos): importações, re-exports, TLA, ciclos, tudo registrando a ordem.
section(0.6);
const names = ["a", "b", "c", "d", "e"];
for (let g = 0; g < 330; g++) {
  const n = 3 + Math.floor(rnd() * 3);
  const mods = names.slice(0, n);
  const files = {};
  const cyclic = rnd() < 0.5;
  mods.forEach((m, idx) => {
    const lines = [];
    const deps = mods.filter((o, j) => o !== m && (cyclic || j > idx) && rnd() < 0.5);
    for (const d of deps) {
      const kind = pick(["side", "named", "star", "starAs", "reexport", "ns"]);
      switch (kind) {
        case "side": lines.push(`import './${d}.mjs';`); break;
        case "named": lines.push(`import { v_${d} as i_${d} } from './${d}.mjs';`, `${rnd() < 0.5 ? "try { " : "try { "}L('${m} sees ${d}=' + i_${d}); } catch (e) { L('${m} ${d} ' + e.name); }`); break;
        case "star": lines.push(`export * from './${d}.mjs';`); break;
        case "starAs": lines.push(`export * as n_${d} from './${d}.mjs';`); break;
        case "reexport": lines.push(`export { v_${d} as r_${d} } from './${d}.mjs';`); break;
        case "ns": lines.push(`import * as s_${d} from './${d}.mjs';`, `try { L('${m} keys ${d}=' + Object.keys(s_${d})); } catch (e) { L('${m} ${d} ' + e.name); }`); break;
      }
    }
    lines.push(`L('${m} start');`);
    const aw = pick(["", "", "await null;", "await null; await null;", "await Promise.resolve();"]);
    if (aw) lines.push(aw);
    lines.push(`export ${pick(["const", "let", "var"])} v_${m} = '${m}';`);
    if (rnd() < 0.15) lines.push(`throw new Error('${m} fails');`);
    lines.push(`L('${m} end');`);
    files[m + ".mjs"] = lines.join("\n");
  });
  const entry = pick(mods);
  const mainSrc = rnd() < 0.3 ? `const ns = await import('./${entry}.mjs'); L('keys ' + Object.keys(ns));` : `import './${entry}.mjs'; L('main');`;
  main(mainSrc, files);
}

// 14. Produto: forma de export x forma de import (cobre combinações simples restantes).
section(0.8);
const exportKinds = {
  const: ["export const v = 1;", "v"], let: ["export let v = 1;", "v"], var: ["export var v = 1;", "v"],
  fn: ["export function v() {}", "v"], cls: ["export class v {}", "v"], list: ["const q = 1; export { q as v };", "v"],
  str: ["const q = 1; export { q as 'v' };", "v"], dflt: ["const q = 1; export { q as default };", "default"],
  star: ["export * from './z.mjs';", "v"], starAs: ["export * as v from './z.mjs';", "v"], re: ["export { z as v } from './z.mjs';", "v"],
  reDefault: ["export { default as v } from './z.mjs';", "v"],
};
const importKindsFn = (name) => [
  `import { ${name} as w } from './a.mjs'; L(typeof w);`,
  `import * as ns from './a.mjs'; L(typeof ns.${name}); L('${name}' in ns);`,
  `const ns = await import('./a.mjs'); L(Object.keys(ns));`,
  `import { ${name} as w } from './r.mjs'; L(typeof w);`,
  `import * as ns from './r.mjs'; L(typeof ns.${name});`,
  `export { ${name} as out } from './a.mjs'; import { out } from './main.mjs';`.replace(/ import \{ out \}.*/, " L('re');"),
  `import { ${name} as w } from './a.mjs'; export { w as out }; L(typeof w);`,
  `import { ${name} as w } from './a.mjs'; const o = { w }; L(typeof o.w);`,
];
for (const [kn, [src, nm]] of Object.entries(exportKinds)) {
  const files = { "a.mjs": src, "z.mjs": "export const z = 'zz'; export default 'zd';", "r.mjs": "export * from './a.mjs'; export { default } from './a.mjs';" };
  if (nm === "default") {
    main(`import { default as w } from './a.mjs'; L(typeof w);`, files);
    main(`import w from './a.mjs'; L(typeof w);`, files);
    main(`import w from './r.mjs'; L(typeof w);`, files);
  }
  for (const imp of importKindsFn(nm === "default" ? "v" : nm)) main(imp, files);
}

// 15. import defer, import source, import.meta.url nos módulos aninhados, nomes string e JSON em mais formas.
section(1);
const deferBodies = [
  "import defer * as ns from './d.mjs'; L('main'); L(typeof ns); L(Object.prototype.toString.call(ns)); L('after typeof'); L(ns.x); L('end');",
  "import defer * as ns from './d.mjs'; L('main'); L(Object.keys(ns)); L('end');",
  "import defer * as ns from './d.mjs'; L('main'); L('x' in ns); L('end');",
  "import defer * as ns from './d.mjs'; L('main'); L(Reflect.ownKeys(ns).map(String).join()); L('end');",
  "import defer * as ns from './d.mjs'; L('main'); L(ns[Symbol.toStringTag]); L('end');",
  "import defer * as ns from './d.mjs'; L('main'); L(ns.nope); L('end');",
  "import defer * as ns from './d.mjs'; import * as ns2 from './d.mjs'; L(ns === ns2); L(Object.keys(ns2)); L('end');",
  "import defer * as ns from './e.mjs'; L('main'); " + T("ns.x") + " " + T("ns.x") + " L('end');",
  "import defer * as ns from './t.mjs'; L('main'); " + T("ns.x") + " L('end');",
  "import defer * as ns from './d2.mjs'; L('main'); L(ns.x); L('end');",
  "import defer * as ns from './d.mjs'; export { ns }; L('main');",
  "import defer { x } from './d.mjs';",
  "import defer x from './d.mjs';",
  "import defer from './d.mjs'; L(typeof defer);",
  "import defer, * as n from './d.mjs';",
  "import defer * as ns from './d.mjs' with { type: 'json' };",
  "export defer * from './d.mjs';",
  "const ns = await import.defer('./d.mjs'); L('got'); L(ns.x);",
  "import.defer('./d.mjs'); L('end');",
  "L(typeof import.defer);",
  "const defer = 1; L(defer);",
  "import defer * as defer from './d.mjs';",
  "import defer * as ns from './d.mjs'; L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'x'))); L('end');",
  "import defer * as ns from './d.mjs'; L(Object.isFrozen(ns)); L(Object.isExtensible(ns)); L('end');",
  "import defer * as ns from './d.mjs'; L(Reflect.has(ns, Symbol.toStringTag)); L('end');",
  "import defer * as ns from './d.mjs'; L(Reflect.defineProperty(ns, 'x', { value: 1 })); L('end');",
];
for (const body of deferBodies) {
  main(body, { "d.mjs": "L('d eval'); export const x = 'dx';", "e.mjs": "L('e eval'); throw new Error('boom'); export const x = 1;", "t.mjs": "L('t start'); await null; export const x = 1;", "d2.mjs": "import './d.mjs'; L('d2 eval'); export const x = 'd2x';" });
}
const sourceBodies = [
  "import source s from './a.mjs';", "import source s from './a.wasm';", "import source from './a.mjs'; L(typeof source);",
  "const s = await import.source('./a.mjs');", "L(typeof import.source);", "import source * as s from './a.mjs';",
  "import source { x } from './a.mjs';", "const source = 1; L(source);",
];
for (const body of sourceBodies) main(body, { "a.mjs": "export const x = 1;" });
// import.meta.url e relação entre módulos em subdiretórios
main(`import { u } from './sub/a.mjs'; L(import.meta.url); L(u);`, { "sub/a.mjs": "export const u = import.meta.url;" });
main(`import { u } from './sub/a.mjs'; L(u);`, { "sub/a.mjs": "import { w } from '../b.mjs'; export const u = import.meta.url + '|' + w;", "b.mjs": "export const w = import.meta.url;" });
main(`import { u } from './sub/../sub/./a.mjs'; L(u);`, { "sub/a.mjs": "export const u = import.meta.url;" });
main(`import { u } from './a.mjs?q=1'; L(u);`, { "a.mjs": "export const u = import.meta.url;" });
main(`import { u } from './a.mjs#h'; L(u);`, { "a.mjs": "export const u = import.meta.url;" });
main(`import a from './a.mjs?x'; import b from './a.mjs?y'; L(a === b); L(a); L(b);`, { "a.mjs": "L('eval'); export default {};" });
main(`import a from './a.mjs'; import b from './a.mjs?'; L(a === b);`, { "a.mjs": "L('eval'); export default {};" });
main(`import a from './sub/../a.mjs'; import b from './a.mjs'; L(a === b);`, { "a.mjs": "L('eval'); export default {};" });
// nomes string nos exports e imports
const strNames = ["a-b", "a b", "", "0", "default", "x.y", "é", "𝒳", "__proto__", "constructor", "then", "toString", "@@toStringTag", "1e3", "\\u{1F600}"];
for (const nm of strNames) {
  const lit = JSON.stringify(nm.replace("\\u{1F600}", "\u{1F600}"));
  main(`import { ${lit} as v } from './a.mjs'; L(v);`, { "a.mjs": `const q = 'q'; export { q as ${lit} };` });
  main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(ns[${lit}]); L(${lit} in ns); L(Object.hasOwn(ns, ${lit}));`, { "a.mjs": `const q = 'q'; export { q as ${lit} };` });
  main(`import { ${lit} as v } from './r.mjs'; L(v);`, { "r.mjs": `export { ${lit} } from './a.mjs';`, "a.mjs": `const q = 'q'; export { q as ${lit} };` });
  main(`import { ${lit} as v } from './r.mjs'; L(v);`, { "r.mjs": `export { ${lit} as ${lit} } from './a.mjs';`, "a.mjs": `const q = 'q'; export { q as ${lit} };` });
  main(`import { v } from './r.mjs'; L(v);`, { "r.mjs": `export { ${lit} as v } from './a.mjs';`, "a.mjs": `const q = 'q'; export { q as ${lit} };` });
  main(`import { ${lit} as v } from './r.mjs'; L(v);`, { "r.mjs": `export * as ${lit} from './a.mjs';`, "a.mjs": `export const q = 'q';` });
  main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": `const q = 'q'; export { q as ${lit} }; export { q as other };` });
  main(`import { ${lit} } from './a.mjs';`, { "a.mjs": `const q = 'q'; export { q as ${lit} };` });
}
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "const q = 1; export { q as '\\u0061b' }; export { q as ab };" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "const q = 1; export { q as 'a\\x62' }; export { q as ab };" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "const q = 1; export { q as 'a\\nb' };" });
main(`import { 'a\\nb' as v } from './a.mjs'; L(v);`, { "a.mjs": "const q = 1; export { q as 'a\\nb' };" });
// JSON modules em mais formas
const jsonFiles = { "d.json": '{"a":1,"b":[2],"__proto__":{"p":1},"default":9}', "e.json": "", "ws.json": "  [1]  ", "dup.json": '{"a":1,"a":2}', "big.json": "123456789012345678901234567890", "neg.json": "-0", "t.json": "true", "u.json": "[1,]", "c.json": "// c\n{}", "bom.json": "﻿{}" };
const jsonBodies = [
  "import d from './d.json' with { type: 'json' }; L(Object.keys(d)); L(Object.getPrototypeOf(d) === Object.prototype); L(d.default);",
  "import * as ns from './d.json' with { type: 'json' }; L(Object.keys(ns)); L(typeof ns.a);",
  "import d from './e.json' with { type: 'json' }; L(d);",
  "import d from './ws.json' with { type: 'json' }; L(JSON.stringify(d));",
  "import d from './dup.json' with { type: 'json' }; L(JSON.stringify(d));",
  "import d from './big.json' with { type: 'json' }; L(d);",
  "import d from './neg.json' with { type: 'json' }; L(Object.is(d, -0));",
  "import d from './t.json' with { type: 'json' }; L(d);",
  "import d from './u.json' with { type: 'json' }; L(d);",
  "import d from './c.json' with { type: 'json' }; L(JSON.stringify(d));",
  "import d from './bom.json' with { type: 'json' }; L(JSON.stringify(d));",
  "const ns = await import('./d.json', { with: { type: 'json' } }); L(Object.keys(ns)); L(ns.default === (await import('./d.json', { with: { type: 'json' } })).default);",
  "import d from './d.json' with { type: 'json' }; const ns = await import('./d.json', { with: { type: 'json' } }); L(d === ns.default);",
  "import d from './d.json' with { type: 'json' }; d.a = 5; const ns = await import('./d.json', { with: { type: 'json' } }); L(ns.default.a);",
  "import d from './d.json' with { type: 'json' }; import m from './m.mjs'; L(d === m);",
  "import * as ns from './d.json' with { type: 'json' }; L(Object.getOwnPropertyNames(ns).join()); L(ns[Symbol.toStringTag]);",
  "import { a } from './d.json' with { type: 'json' }; import { a as a2 } from './d.json' with { type: 'json' }; L(a + a2);",
  "import { default as d } from './d.json' with { type: 'json' }; L(typeof d);",
  "import d from './d.json' with { type: 'json' }; export { d }; L('ok');",
];
for (const body of jsonBodies) main(body, { ...jsonFiles, "m.mjs": "import d from './d.json' with { type: 'json' }; export default d;" });

// Execução.
const lines = [];
const failures = [];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "zjsc-module-more-golden-"));
const runner = path.join(tmp, "runner.mjs");
fs.writeFileSync(runner, RUNNER);
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
const cases = pool.resolve((files) => JSON.stringify(files)).filter((p) => !usesHostApi(p));
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
