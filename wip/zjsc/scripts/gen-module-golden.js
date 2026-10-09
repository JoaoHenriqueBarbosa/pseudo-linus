// Gera tests/golden/module_bun.tsv: grafos de módulos ES (2 a 5 arquivos .mjs) avaliados no bun.
// Cada caso vira um diretório temporário com os arquivos; o ponto de entrada é sempre main.mjs. Os módulos
// usam os globais `log` (array) e `L(x)` (empurra String(x) em `log`), definidos por um prelúdio. O runner
// importa main.mjs com import() e imprime JSON.stringify({log, error}), onde error é `Nome: mensagem` do que
// rejeitou o import, ou null. O caminho do diretório temporário sai de mensagens e de import.meta.url
// (que vira `file:///arquivo.mjs`). Colunas: o mapa de arquivos em JSON, depois a saída esperada.
// Cada caso roda num processo bun próprio, com timeout. Uso: bun scripts/gen-module-golden.js > tests/golden/module_bun.tsv
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

const RUNNER = `
globalThis.log = [];
globalThis.L = (x) => { log.push(String(x)); };
const [dir] = process.argv.slice(2);
let error = null;
try { await import(dir + "/main.mjs"); } catch (e) {
  error = e instanceof Error ? e.name + ": " + e.message : "throw " + String(e);
}
await new Promise((r) => setTimeout(r, 0));
const clean = (s) => s.split("file://" + dir + "/").join("file:///").split(dir + "/").join("");
console.log(clean(JSON.stringify({ log, error })));
`;

// 1. Importações nomeadas: forma do export x forma do import.
const exportForms = {
  const: ["export const v = 1;", "v"],
  let: ["export let v = 2;", "v"],
  var: ["export var v = 3;", "v"],
  function: ["export function v() { return 4; }", "v()"],
  class: ["export class v { static n = 5; }", "v.n"],
  list: ["const a = 6; export { a as v };", "v"],
  asyncFn: ["export async function v() { return 7; }", "typeof v"],
  gen: ["export function* v() { yield 8; }", "[...v()][0]"],
  destructure: ["export const { v } = { v: 9 };", "v"],
  arrayDestructure: ["export const [v] = [10];", "v"],
};
for (const [n, [src, use]] of Object.entries(exportForms)) {
  main(`import { v } from './a.mjs'; L(${use});`, { "a.mjs": src });
  main(`import { v as w } from './a.mjs'; L(typeof w); L(String(w).length > 0);`, { "a.mjs": src });
  main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(typeof ns.v);`, { "a.mjs": src });
  main(`const ns = await import('./a.mjs'); L(Object.keys(ns)); L(Object.prototype.toString.call(ns));`, { "a.mjs": src });
}

// 2. Default: formas.
const defaults = {
  expr: "export default 42;",
  fn: "export default function () { return 1; }",
  namedFn: "export default function foo() { return 1; }",
  cls: "export default class { static x = 1; }",
  namedCls: "export default class Foo {}",
  arrow: "export default () => 1;",
  obj: "export default { a: 1, b: [2] };",
  asyncFn: "export default async function () {}",
  genFn: "export default function* () {}",
  listAs: "const q = 7; export { q as default };",
  str: "export default 'texto';",
};
for (const [n, src] of Object.entries(defaults)) {
  main(`import d from './a.mjs'; L(typeof d); L(d && d.name); L(d);`, { "a.mjs": src });
  main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(typeof ns.default);`, { "a.mjs": src });
  main(`import { default as d } from './a.mjs'; L(typeof d);`, { "a.mjs": src });
}

// 3. Namespace: propriedades, ordem, Symbol.toStringTag, imutabilidade.
main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(Object.getOwnPropertyNames(ns)); L(ns[Symbol.toStringTag]); L(Object.isFrozen(ns)); L(Object.isSealed(ns)); L(Object.isExtensible(ns)); L(Reflect.ownKeys(ns).length);`, { "a.mjs": "export const z = 1, a = 2, m = 3; export default 0;" });
main(`import * as ns from './a.mjs'; try { ns.a = 1; } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "export const a = 0;" });
main(`import * as ns from './a.mjs'; try { ns.b = 1; } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "export const a = 0;" });
main(`import * as ns from './a.mjs'; try { delete ns.a; } catch (e) { L(e.name); L(e.message); } L(delete ns.nope);`, { "a.mjs": "export const a = 0;" });
main(`import * as ns from './a.mjs'; L(Object.getPrototypeOf(ns)); L(JSON.stringify(Object.getOwnPropertyDescriptor(ns, 'a'))); L(Reflect.has(ns, 'a')); L('x' in ns); L(Reflect.defineProperty(ns, 'a', { value: 1 })); L(Reflect.defineProperty(ns, 'a', { value: 0 }));`, { "a.mjs": "export const a = 0;" });
main(`import * as ns from './a.mjs'; L(Reflect.setPrototypeOf(ns, null)); L(Reflect.setPrototypeOf(ns, {})); L(Reflect.preventExtensions(ns));`, { "a.mjs": "export const a = 0;" });
main(`import * as ns from './a.mjs'; L(JSON.stringify(ns)); L(JSON.stringify(Object.entries(ns))); L(String(Object.getOwnPropertySymbols(ns).length));`, { "a.mjs": "export const a = 1; export const b = 'x';" });
main(`import * as ns from './a.mjs'; const d = Object.getOwnPropertyDescriptor(ns, 'a'); L(d.writable); L(d.enumerable); L(d.configurable);`, { "a.mjs": "export let a = 1;" });
main(`import * as ns from './a.mjs'; L(ns.a); L(ns.b); L('b' in ns);`, { "a.mjs": "export let a = 1;" });
main(`import * as ns from './a.mjs'; const k = []; for (const x in ns) k.push(x); L(k); L(Object.values(ns));`, { "a.mjs": "export const é = 1, b = 2, a = 3, 'x y' = 0;".replace(", 'x y' = 0", "") });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "const a = 1, b = 2, c = 3; export { c, b as z, a as '0'; export { a as 'a-b' };".replace("; export { a as 'a-b' };", ", a as 'a-b' };") });

// 4. Ligação viva (live bindings).
main(`import { c, inc } from './a.mjs'; L(c); inc(); L(c); inc(); L(c);`, { "a.mjs": "export let c = 0; export function inc() { c++; }" });
main(`import { c, set } from './a.mjs'; L(c); set(5); L(c);`, { "a.mjs": "export let c = 0; export function set(v) { c = v; }" });
main(`import * as ns from './a.mjs'; L(ns.c); ns.inc(); L(ns.c);`, { "a.mjs": "export let c = 0; export function inc() { c += 2; }" });
main(`import { c } from './a.mjs'; try { c = 1; } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "export let c = 0;" });
main(`import { c } from './a.mjs'; try { c++; } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "export let c = 0;" });
main(`import d from './a.mjs'; L(d); L(typeof d);`, { "a.mjs": "let x = 1; export default x; x = 2;" });
main(`import { d } from './a.mjs'; L(d); `, { "a.mjs": "let x = 1; export { x as d }; x = 2;" });
main(`import d, { n } from './a.mjs'; L(d); L(n); n_inc();`.replace(" n_inc();", ""), { "a.mjs": "export default function f() {} export let n = 1;" });
main(`import { f } from './a.mjs'; L(typeof f); L(f()); `, { "a.mjs": "export { f }; function f() { return 'hoisted'; }" });
main(`import { C } from './a.mjs'; L(typeof C);`, { "a.mjs": "export class C {}" });
main(`import { f } from './a.mjs'; L(f.name);`, { "a.mjs": "export const f = () => 1;" });
main(`import d from './a.mjs'; L(d.name === 'default' ? 'default' : d.name);`, { "a.mjs": "export default (() => 1);" });
main(`import d from './a.mjs'; L(d.name);`, { "a.mjs": "export default class {}" });
main(`import d from './a.mjs'; L(d.name);`, { "a.mjs": "export default function () {}" });

// 5. Re-exports.
main(`import { a, b } from './r.mjs'; L(a); L(b);`, { "r.mjs": "export { a, b } from './a.mjs';", "a.mjs": "export const a = 1, b = 2;" });
main(`import { x, y } from './r.mjs'; L(x); L(y);`, { "r.mjs": "export { a as x, b as y } from './a.mjs';", "a.mjs": "export const a = 1, b = 2;" });
main(`import { d } from './r.mjs'; L(d);`, { "r.mjs": "export { default as d } from './a.mjs';", "a.mjs": "export default 'dflt';" });
main(`import d from './r.mjs'; L(d);`, { "r.mjs": "export { a as default } from './a.mjs';", "a.mjs": "export const a = 'viaA';" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(ns.a); L(ns.b);`, { "r.mjs": "export * from './a.mjs';", "a.mjs": "export const a = 1, b = 2; export default 3;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(ns.default);`, { "r.mjs": "export * from './a.mjs'; export default 'own';", "a.mjs": "export const a = 1; export default 3;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(ns.a);`, { "r.mjs": "export * from './a.mjs'; export const a = 'local';", "a.mjs": "export const a = 'remote', z = 1;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(typeof ns.inner); L(Object.keys(ns.inner));`, { "r.mjs": "export * as inner from './a.mjs';", "a.mjs": "export const a = 1, b = 2;" });
main(`import { inner } from './r.mjs'; L(inner.a); L(Object.prototype.toString.call(inner)); L(inner === inner);`, { "r.mjs": "export * as inner from './a.mjs';", "a.mjs": "export const a = 1;" });
main(`import * as m from './r.mjs'; import * as a from './a.mjs'; L(m.inner === a);`, { "r.mjs": "export * as inner from './a.mjs';", "a.mjs": "export const a = 1;" });
main(`import { default as d } from './r.mjs'; L(Object.keys(d));`, { "r.mjs": "export * as default from './a.mjs';", "a.mjs": "export const a = 1;" });
main(`import { s } from './r.mjs'; L(s);`, { "r.mjs": "export { 'a b' as s } from './a.mjs';", "a.mjs": "const q = 'strname'; export { q as 'a b' };" });
main(`import { 'a b' as s } from './a.mjs'; L(s);`, { "a.mjs": "const q = 'strname2'; export { q as 'a b' };" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns));`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export const a = 1;", "b.mjs": "export const b = 2;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns));`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export const x = 1, a = 1;", "b.mjs": "export const x = 1, b = 2;" });
main(`import { x } from './r.mjs'; L(x);`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export const x = 1;", "b.mjs": "export const x = 2;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(ns.x);`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export const x = 1;", "b.mjs": "export const x = 2;" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns)); L(ns.x);`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export { v as x } from './c.mjs';", "b.mjs": "export { v as x } from './c.mjs';", "c.mjs": "export const v = 'same';" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(ns.v);`, { "a.mjs": "export * from './a.mjs'; export const v = 1;" });
main(`import { v } from './a.mjs'; L(v);`, { "a.mjs": "export { v } from './b.mjs';", "b.mjs": "export { v } from './c.mjs';", "c.mjs": "export const v = 'deep';" });
main(`import { v } from './a.mjs'; L(v);`, { "a.mjs": "export * from './b.mjs';", "b.mjs": "export * from './c.mjs';", "c.mjs": "export let v = 'deepstar';" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "import { x } from './b.mjs'; export { x };", "b.mjs": "export const x = 1;" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(ns.n.x);`, { "a.mjs": "import * as n from './b.mjs'; export { n };", "b.mjs": "export const x = 1;" });
main(`import { c } from './r.mjs'; L(c); import { inc } from './a.mjs'; inc(); L(c);`, { "r.mjs": "export { c } from './a.mjs';", "a.mjs": "export let c = 0; export function inc() { c++; }" });

// 6. Ordem de avaliação.
main(`L('main'); `.replace("L('main'); ", "") + `import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a');", "b.mjs": "L('b');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "L('c');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a'); import './c.mjs';", "b.mjs": "L('b');", "c.mjs": "L('c');" });
main(`import './a.mjs'; import './a.mjs'; import './a.mjs'; L('main');`, { "a.mjs": "L('a');" });
main(`import './a.mjs'; await import('./a.mjs'); L('main');`, { "a.mjs": "L('a');" });
main(`import { x } from './a.mjs'; import { y } from './b.mjs'; L(x + y);`, { "a.mjs": "L('a'); export const x = 1;", "b.mjs": "L('b'); export const y = 2;" });
main(`L('main1'); import './a.mjs'; L('main2');`, { "a.mjs": "L('a');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "import './d.mjs'; L('b');", "c.mjs": "import './d.mjs'; L('c');", "d.mjs": "L('d');" });
main(`import './a.mjs';`, { "a.mjs": "export * from './b.mjs'; L('a');", "b.mjs": "L('b'); export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export { x } from './b.mjs'; L('a');", "b.mjs": "L('b'); export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as n from './b.mjs'; L('a');", "b.mjs": "L('b');" });
main(`import './a.mjs';`, { "a.mjs": "import {} from './b.mjs'; L('a');", "b.mjs": "L('b');" });
main(`import './a.mjs';`, { "a.mjs": "export {} from './b.mjs'; L('a');", "b.mjs": "L('b');" });
main(`L(typeof this); L(typeof globalThis.x); L(this === undefined);`, { "a.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "L(typeof this); L(typeof arguments === 'undefined'); L(typeof require); L(typeof module); L(typeof exports);" });
main(`import './a.mjs';`, { "a.mjs": "var v = 1; L(typeof globalThis.v); L(typeof v); function f() {} L(typeof globalThis.f);" });
main(`L(import.meta.url); L(typeof import.meta); L(Object.keys(import.meta));`, { "a.mjs": "" });
main(`import { u } from './a.mjs'; L(import.meta.url); L(u); L(import.meta.url === u);`, { "a.mjs": "export const u = import.meta.url;" });
main(`import { u } from './sub/a.mjs'; L(u);`, { "sub/a.mjs": "export const u = import.meta.url;" });
main(`L(import.meta === import.meta); L(Object.getPrototypeOf(import.meta));`, { "a.mjs": "" });
main(`import { m } from './a.mjs'; L(m === import.meta);`, { "a.mjs": "export const m = import.meta;" });
main(`L(typeof import.meta.dir); L(typeof import.meta.filename); L(typeof import.meta.dirname); L(typeof import.meta.path);`.replace(/L\(typeof import\.meta\.(dir|filename|dirname|path)\);/g, "L(import.meta.$1 === undefined ? 'u' : 'd');"), { "a.mjs": "" });
main(`import { f } from './sub/a.mjs'; L(f());`, { "sub/a.mjs": "import { g } from '../b.mjs'; export const f = () => g();", "b.mjs": "export const g = () => 'g';" });
main(`import { f } from './sub/a.mjs'; L(f());`, { "sub/a.mjs": "import { g } from './deeper/c.mjs'; export const f = () => g();", "sub/deeper/c.mjs": "export const g = () => 'c';" });

// 7. Ciclos e TDZ em ciclo.
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './a.mjs'; L('b');" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A' + b;", "b.mjs": "import { a } from './a.mjs'; export const b = 'B';" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A'; L('a' + b);", "b.mjs": "import { a } from './a.mjs'; export const b = 'B'; L('b' + a);" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A'; L('a' + b);", "b.mjs": "import { a } from './a.mjs'; export const b = 'B'; try { L('b' + a); } catch (e) { L(e.name + ': ' + e.message); }" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export let a = 'A'; L(b);", "b.mjs": "import { a } from './a.mjs'; export var b = typeof a;" }.constructor === Object ? { "a.mjs": "import { b } from './b.mjs'; export let a = 'A'; L(b);", "b.mjs": "import { a } from './a.mjs'; let r; try { r = typeof a; } catch (e) { r = e.name; } export var b = r;" } : {});
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { f } from './b.mjs'; export function g() { return 'g'; } L(f());", "b.mjs": "import { g } from './a.mjs'; export function f() { return g(); }" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { f } from './b.mjs'; export const g = () => 'g'; export const a = f();", "b.mjs": "import { g } from './a.mjs'; export function f() { return g(); }" });
main(`import { f } from './b.mjs'; L(f());`, { "a.mjs": "import { f } from './b.mjs'; export class C { static v = 'C'; }  export const g = () => f;", "b.mjs": "import { C } from './a.mjs'; export function f() { return C.v; }" });
main(`import './a.mjs';`, { "a.mjs": "import { C } from './b.mjs'; export class Base {} L('a');", "b.mjs": "import { Base } from './a.mjs'; export class C extends Base {}" });
main(`import './b.mjs';`, { "a.mjs": "import { C } from './b.mjs'; export class Base {} L('a');", "b.mjs": "import { Base } from './a.mjs'; export class C extends Base {} L('b');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; L('c');" });
main(`import './c.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; L('c');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './a.mjs'; L('a');" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "import { x as y } from './a.mjs'; export const x = 1; L(y);" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "L(typeof x); export let x = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "try { x; } catch (e) { L(e.name + ': ' + e.message); } export let x = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "try { x; } catch (e) { L(e.name + ': ' + e.message); } export const x = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "try { x; } catch (e) { L(e.name + ': ' + e.message); } export class x {}" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "L(typeof x); export var x = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "L(typeof x); export function x() {}" });
main(`import { b } from './b.mjs';`, { "b.mjs": "import { a } from './a.mjs'; export const b = 1; L(a);", "a.mjs": "import { b } from './b.mjs'; export const a = typeof b === 'number' ? 'n' : 'x'; " });
main(`import './a.mjs';`, { "a.mjs": "import { b } from './b.mjs'; L('a'); export const a = 1;", "b.mjs": "import { a } from './a.mjs'; try { L(a); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { b } from './b.mjs'; L('a'); export let a = 1;", "b.mjs": "import { a } from './a.mjs'; try { L(a); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as ns from './b.mjs'; L('a'); export const a = 1;", "b.mjs": "import * as ns from './a.mjs'; try { L(ns.a); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as ns from './b.mjs'; L('a'); export const a = 1;", "b.mjs": "import * as ns from './a.mjs'; try { L(Object.keys(ns)); } catch (e) { L(e.name + ': ' + e.message); } try { L('a' in ns); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as ns from './b.mjs'; L('a'); export default 1;", "b.mjs": "import d from './a.mjs'; try { L(d); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as ns from './b.mjs'; L('a'); export default function f() { return 1; }", "b.mjs": "import d from './a.mjs'; try { L(d()); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a'); export default class K {}", "b.mjs": "import d from './a.mjs'; try { L(typeof d); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a'); export default 5 + 5;", "b.mjs": "import d from './a.mjs'; try { L(d); } catch (e) { L(e.name + ': ' + e.message); } export const b = 1;" });
main(`import { r } from './a.mjs'; L(r);`, { "a.mjs": "import { b } from './b.mjs'; export * from './c.mjs'; export const r = b;", "b.mjs": "import { c } from './c.mjs'; export const b = 'b' + c;", "c.mjs": "export const c = 'c';" });

// 8. Top-level await e ordem.
main(`import './a.mjs'; L('main');`, { "a.mjs": "L('a1'); await null; L('a2');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a1'); await null; L('a2');", "b.mjs": "L('b');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a1'); await null; await null; L('a2');", "b.mjs": "L('b1'); await null; L('b2');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "L('a1'); await new Promise(r => setTimeout(r, 5)); L('a2');", "b.mjs": "L('b1'); await null; L('b2');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "await 1; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "await 1; await 1; L('c');" });
main(`import { v } from './a.mjs'; L(v);`, { "a.mjs": "export let v = 0; v = await Promise.resolve(7);" });
main(`import { v } from './a.mjs'; L(v);`, { "a.mjs": "export const v = await Promise.resolve('tla');" });
main(`const v = await Promise.resolve(3); L(v); L(await 4);`, {});
main(`L(await Promise.all([1, Promise.resolve(2), (async () => 3)()]));`, {});
main(`try { await Promise.reject(new Error('rej')); } catch (e) { L(e.message); } L('after');`, {});
main(`for await (const x of [Promise.resolve(1), 2]) L(x); L('done');`, {});
main(`import './a.mjs';`, { "a.mjs": "for await (const x of (async function* () { yield 1; yield 2; })()) L(x);" });
main(`await 1; L(typeof await);`.replace("L(typeof await);", "L('x');"), {});
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "export const x = await (async () => { L('in'); return 1; })(); L('out');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b'); await null; L('b2');", "c.mjs": "L('c');" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "L('c1'); await null; L('c2');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "throw new Error('b falhou');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "await null; throw new Error('b tla falhou');" });
main(`import './a.mjs';`, { "a.mjs": "await Promise.reject(new TypeError('tla rej'));" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.message); } try { await import('./a.mjs'); } catch (e) { L(e.message); }`, { "a.mjs": "L('run'); throw new Error('uma vez');" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.message); } try { await import('./b.mjs'); } catch (e) { L(e.message); }`, { "a.mjs": "import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "L('c'); throw new Error('c quebrou');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a'); await null; L('a2');", "b.mjs": "import './a.mjs'; L('b'); await null; L('b2');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a'); await null; L('a2');", "b.mjs": "import './a.mjs'; L('b');" });
main(`import './a.mjs';`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './a.mjs'; L('b'); await null; L('b2');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "import './a.mjs'; await null; L('b');", "c.mjs": "L('c'); await null; L('c2');" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export const a = 'A'; L(b);", "b.mjs": "import { a } from './a.mjs'; export let b = 'pre'; try { a; } catch (e) { b = e.name; } await null;" });
main(`let r; try { await import('./a.mjs'); } catch (e) { r = e; } L(r.name); L(r.message);`, { "a.mjs": "import { b } from './b.mjs'; await null;", "b.mjs": "import { a } from './a.mjs'; export const b = a;" });
main(`L(await Promise.race([import('./a.mjs').then(() => 'a'), import('./b.mjs').then(() => 'b')]));`, { "a.mjs": "await new Promise(r => setTimeout(r, 10));", "b.mjs": "await null;" });
main(`import './a.mjs'; await 0; L('main');`, { "a.mjs": "await null; L('a');" });
main(`await null; L('main1'); import('./a.mjs').then(() => L('then')); L('main2');`, { "a.mjs": "L('a');" });
main(`import './a.mjs'; L('main'); await null; L('main2');`, { "a.mjs": "queueMicrotask(() => L('micro')); L('a'); await null; L('a2');" });
main(`queueMicrotask(() => L('m1')); await null; L('after'); queueMicrotask(() => L('m2'));`, {});
main(`setTimeout(() => L('timer'), 0); await null; L('main');`, {});
main(`await new Promise(r => setTimeout(r, 1)); L('slept');`, {});

// 9. Import dinâmico.
main(`const ns = await import('./a.mjs'); L(ns.x); L(ns.default);`, { "a.mjs": "export const x = 1; export default 2;" });
main(`const p = import('./a.mjs'); L(p instanceof Promise); L('sync'); await p; L('done');`, { "a.mjs": "L('a'); export const x = 1;" });
main(`const [a, b] = await Promise.all([import('./a.mjs'), import('./a.mjs')]); L(a === b);`, { "a.mjs": "L('a');" });
main(`import * as s from './a.mjs'; const d = await import('./a.mjs'); L(s === d);`, { "a.mjs": "export const x = 1;" });
main(`const n = './a.mjs'; const ns = await import(n); L(ns.x);`, { "a.mjs": "export const x = 'computed';" });
main(`const ns = await import('./' + 'a' + '.mjs'); L(ns.x);`, { "a.mjs": "export const x = 'concat';" });
main(`L(typeof import('./a.mjs').then);`, { "a.mjs": "" });
main(`await import('./a.mjs'); L('main');`, { "a.mjs": "L('a'); await null; L('a2');" });
main(`import('./a.mjs'); L('main');`, { "a.mjs": "L('a');" });
main(`function f() { return import('./a.mjs'); } const ns = await f(); L(ns.x);`, { "a.mjs": "export const x = 'inFn';" });
main(`import { l } from './a.mjs'; L(await l());`, { "a.mjs": "export const l = async () => (await import('./sub/b.mjs')).x;", "sub/b.mjs": "export const x = 'rel-to-a';" });
main(`import { l } from './sub/a.mjs'; L(await l());`, { "sub/a.mjs": "export const l = async () => (await import('./b.mjs')).x;", "sub/b.mjs": "export const x = 'sub-b';" });
main(`const ns = await import('./a.mjs'); L(ns.y); await import('./a.mjs'); L(ns.y);`, { "a.mjs": "export let y = 0; y++;" });
main(`const ns = await import('./a.mjs'); L(ns.f()); L(ns.f());`, { "a.mjs": "let c = 0; export const f = () => ++c;" });
main(`try { await import('./nope.mjs'); } catch (e) { L(e.name); L(e.message); }`, {});
main(`try { await import('./sub/nope.mjs'); } catch (e) { L(e.name); L(e.message); }`, {});
main(`try { await import('nopkg'); } catch (e) { L(e.name); L(e.message); }`, {});
main(`try { await import('./a.mjs'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "import './missing.mjs';" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "import { q } from './b.mjs';", "b.mjs": "export const z = 1;" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "export const = 1;" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name); L(e.message); } try { await import('./a.mjs'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "let let = 1;" });
main(`await import('./a.mjs').catch(e => L(e.name + ': ' + e.message)); L('end');`, { "a.mjs": "throw new RangeError('r');" });
main(`const r = await Promise.allSettled([import('./a.mjs'), import('./b.mjs')]); L(r.map(x => x.status));`, { "a.mjs": "export const a = 1;", "b.mjs": "throw 1;" });
main(`L(typeof import.meta.resolve); `, {});
main(`import('./a.mjs', { with: { type: 'json' } }).then(ns => L(ns.default.k));`, { "a.json": "{\"k\":1}", "a.mjs": "" }.constructor ? { "a.mjs": "export const k = 1;" } : {});
main(`const ns = await import('./a.json', { with: { type: 'json' } }); L(JSON.stringify(ns.default)); L(Object.keys(ns));`, { "a.json": "{\"k\":[1,2,{\"z\":null}]}" });
main(`const ns = await import('./a.json'); L(JSON.stringify(ns.default)); L(Object.keys(ns));`, { "a.json": "{\"k\":1}" });
main(`try { const ns = await import('./a.json', { with: { type: 'json' } }); L(ns.default); } catch (e) { L(e.name + ': ' + e.message); }`, { "a.json": "[1,2,3]" });
main(`try { const ns = await import('./a.json', { with: { type: 'json' } }); L(ns.default); } catch (e) { L(e.name); }`, { "a.json": "{bad" });
main(`import d from './a.json' with { type: 'json' }; L(d.k); L(typeof d);`, { "a.json": "{\"k\":\"v\"}" });
main(`import d from './a.json' with { type: 'json' }; L(d); L(typeof d);`, { "a.json": "123" });
main(`import d from './a.json' with { type: 'json' }; L(d); L(typeof d);`, { "a.json": "\"str\"" });
main(`import d from './a.json' with { type: 'json' }; L(d); L(typeof d);`, { "a.json": "null" });
main(`import d from './a.json' with { type: 'json' }; L(Array.isArray(d)); L(d.length);`, { "a.json": "[1,2,3]" });
main(`import * as ns from './a.json' with { type: 'json' }; L(Object.keys(ns)); L(ns.default.a);`, { "a.json": "{\"a\":1,\"b\":2}" });
main(`import d from './a.json' with { type: 'json' }; import e from './a.json' with { type: 'json' }; L(d === e);`, { "a.json": "{\"a\":1}" });
main(`import d from './a.json' with { type: 'json' }; d.a = 2; import e from './b.mjs'; L(e);`, { "a.json": "{\"a\":1}", "b.mjs": "import d from './a.json' with { type: 'json' }; export default d.a;" });
main(`import { a } from './a.json' with { type: 'json' }; L(a);`, { "a.json": "{\"a\":1}" });
main(`import d from './a.json' with { type: 'json' }; L(d.a);`, { "a.json": "{\"a\":1} " });
main(`import d from './a.json' with { type: 'json' }; L(d.a);`, { "a.json": "\n{\"a\":1}\n" });
main(`import d from './a.json' with { type: 'json' }; L(d.a);`, { "a.json": "{\"a\":1,\"a\":2}" });
main(`import d from './a.json' with { type: 'json' }; L(d.__proto__ === Object.prototype); L(Object.keys(d));`, { "a.json": "{\"__proto__\":1,\"b\":2}" });
main(`import d from './a.json' with { type: 'json' }; L(d[1]);`, { "a.json": "{\"1\":\"um\",\"0\":\"zero\"}" });
main(`import './a.mjs' with { type: 'json' };`, { "a.mjs": "export const x = 1;" });
main(`import d from './a.json'; L(d.a);`, { "a.json": "{\"a\":1}" });
main(`import { x } from './a.mjs' with { }; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x } from './a.mjs' with { type: 'javascript' }; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x } from './a.mjs' with { foo: 'bar' }; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x } from './a.mjs' with { type: 'json', type: 'json' }; L(x);`, { "a.mjs": "export const x = 1;" });
main(`export { default } from './a.json' with { type: 'json' }; L('x');`, { "a.json": "{}" });
main(`import { d } from './r.mjs'; L(d.a);`, { "r.mjs": "export { default as d } from './a.json' with { type: 'json' };", "a.json": "{\"a\":1}" });
main(`import { d } from './r.mjs'; L(d.a);`, { "r.mjs": "import x from './a.json' with { type: 'json' }; export { x as d };", "a.json": "{\"a\":1}" });
main(`const ns = await import('./a.json', { with: { type: 'json' } }); const ns2 = await import('./a.json', { with: { type: 'json' } }); L(ns === ns2);`, { "a.json": "{\"a\":1}" });
main(`try { await import('./a.mjs', { with: { type: 'nope' } }); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', { with: { type: '' } }); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', { with: 5 }); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', 5); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', { with: { type: 5 } }); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', {}); L('ok'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import('./a.mjs', undefined); L('ok'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import({ toString() { return './a.mjs'; } }); L('ok'); } catch (e) { L(e.name); L(e.message); }`, { "a.mjs": "" });
main(`try { await import(Symbol()); } catch (e) { L(e.name); L(e.message); }`, {});
main(`try { await import(); } catch (e) { L(e.name); }`.replace("await import();", "eval('1')"), {});

// 10. Erros de ligação e de sintaxe.
main(`import { x } from './a.mjs';`, { "a.mjs": "export const y = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "export default 1;" });
main(`import x from './a.mjs'; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x, y } from './a.mjs'; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x as z } from './a.mjs';`, { "a.mjs": "export const y = 1;" });
main(`import { x } from './r.mjs';`, { "r.mjs": "export { x } from './a.mjs';", "a.mjs": "export const y = 1;" });
main(`import { x } from './r.mjs';`, { "r.mjs": "export * from './a.mjs';", "a.mjs": "export const y = 1;" });
main(`import { x } from './r.mjs';`, { "r.mjs": "export * from './a.mjs'; export * from './b.mjs';", "a.mjs": "export const x = 1;", "b.mjs": "export const x = 2;" });
main(`import { x } from './r.mjs';`, { "r.mjs": "export { x } from './a.mjs';", "a.mjs": "export { x } from './b.mjs';", "b.mjs": "export { x } from './r.mjs';" });
main(`import { x } from './a.mjs';`, { "a.mjs": "export { x } from './a.mjs';" });
main(`import { x } from './a.mjs';`, { "a.mjs": "export { x };" });
main(`import { x } from './a.mjs'; `, { "a.mjs": "export { x } from './b.mjs';", "b.mjs": "import { x } from './a.mjs'; export { x };" });
main(`import * as ns from './r.mjs'; L(Object.keys(ns));`, { "r.mjs": "export { x } from './a.mjs';", "a.mjs": "export const y = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export const x = 1; export const x = 2;" });
main(`import './a.mjs';`, { "a.mjs": "export const x = 1; export { x };" });
main(`import './a.mjs';`, { "a.mjs": "const x = 1, y = 2; export { x, y as x };" });
main(`import './a.mjs';`, { "a.mjs": "export default 1; export default 2;" });
main(`import './a.mjs';`, { "a.mjs": "export default 1; export { x as default }; const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export function f() {} export function f() {}" });
main(`import './a.mjs';`, { "a.mjs": "export function f() {} export var f;" });
main(`import './a.mjs';`, { "a.mjs": "export class C {} export class C {}" });
main(`import './a.mjs';`, { "a.mjs": "export { x };" });
main(`import './a.mjs';`, { "a.mjs": "export { x as y };" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; export { x };", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; import { x } from './b.mjs';", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; const x = 1;", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "const x = 1; import { x } from './b.mjs';", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; var x;", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; function x() {}", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x } from './b.mjs'; class x {}", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "import x, * as x from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "import * as ns, { x } from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { default } from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { if } from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "import { x as if } from './b.mjs';", "b.mjs": "export const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export { if };" });
main(`import './a.mjs';`, { "a.mjs": "export { if } from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "export { default } from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "export * as default from './b.mjs';", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "export * as x from './b.mjs'; export const x = 1;", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "function f() { import { x } from './b.mjs'; }", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "if (1) { export const x = 1; }" });
main(`import './a.mjs';`, { "a.mjs": "function f() { export default 1; }" });
main(`import './a.mjs';`, { "a.mjs": "{ import x from './b.mjs'; }", "b.mjs": "export default 1;" });
main(`import './a.mjs';`, { "a.mjs": "export var;" });
main(`import './a.mjs';`, { "a.mjs": "export const;" });
main(`import './a.mjs';`, { "a.mjs": "export { x, };const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export {,};" });
main(`import './a.mjs';`, { "a.mjs": "import {,} from './b.mjs';", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "import from './b.mjs';", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "import x from;", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "import x from 5;", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "import 'x' as y;", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "export * from;" });
main(`import './a.mjs';`, { "a.mjs": "export * as from './b.mjs';", "b.mjs": "" });
main(`import './a.mjs';`, { "a.mjs": "with (x) {}" });
main(`import './a.mjs';`, { "a.mjs": "var yield = 1;" });
main(`import './a.mjs';`, { "a.mjs": "var await = 1;" });
main(`import './a.mjs';`, { "a.mjs": "function await() {}" });
main(`import './a.mjs';`, { "a.mjs": "var let = 1;" });
main(`import './a.mjs';`, { "a.mjs": "var eval = 1;" });
main(`import './a.mjs';`, { "a.mjs": "delete x;" });
main(`import './a.mjs';`, { "a.mjs": "012;" });
main(`import './a.mjs';`, { "a.mjs": "'\\07';" });
main(`import './a.mjs';`, { "a.mjs": "x = 1; L(x);" });
main(`import './a.mjs';`, { "a.mjs": "undeclared_variable_zz = 1;" });
main(`import './a.mjs';`, { "a.mjs": "L(undeclared_variable_zz);" });
main(`import './a.mjs';`, { "a.mjs": "<!-- comentario html\nL('ok');" });
main(`import './a.mjs';`, { "a.mjs": "L(new.target);" });
main(`import './a.mjs';`, { "a.mjs": "return 1;" });
main(`import './a.mjs';`, { "a.mjs": "await;" });
main(`import './a.mjs';`, { "a.mjs": "super.x;" });
main(`import './a.mjs';`, { "a.mjs": "break;" });
main(`import './a.mjs';`, { "a.mjs": "import.meta = 1;" });
main(`import './a.mjs';`, { "a.mjs": "L(import.foo);" });
main(`import './a.mjs';`, { "a.mjs": "L(new import('x'));" });
main(`import './a.mjs';`, { "a.mjs": "export const x = 1\nexport const y = 2\nL('asi')" });
main(`import './a.mjs';`, { "a.mjs": "export default\nfunction f() {}\nL(typeof f)" });
main(`import './a.mjs';`, { "a.mjs": "export default async function () {}\nL('ok')" });
main(`import './a.mjs';`, { "a.mjs": "export default async () => 1;\nL('ok')" });
main(`import './a.mjs';`, { "a.mjs": "export default (class {}, 1);\nL('ok')" });
main(`import './a.mjs';`, { "a.mjs": "export default function* g() {}\nL(typeof g)" });
main(`import './a.mjs';`, { "a.mjs": "export default let x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export default const x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export default var x = 1;" });
main(`import './a.mjs';`, { "a.mjs": "export async function f() {} export async function* g() {} L(typeof f + typeof g)" });
main(`import './a.mjs';`, { "a.mjs": "export let a, b = 2, c;\nL(a); L(b); L(c);" });
main(`import './a.mjs';`, { "a.mjs": "export const { a, ...rest } = { a: 1, b: 2, c: 3 };\nL(a); L(Object.keys(rest));" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "export var a = 1, a2 = a + 1;" });

// 11. Arquivos ausentes e resolução de caminhos.
main(`import './nope.mjs';`, {});
main(`import { x } from './nope.mjs';`, {});
main(`import x from './sub/nope.mjs';`, {});
main(`import './a.mjs';`, { "a.mjs": "import '../nope.mjs';" });
main(`import './a.mjs';`, { "a.mjs": "import './nope.mjs';" });
main(`import 'nopkg';`, {});
main(`import 'nopkg/sub';`, {});
main(`import './sub';`, { "sub/index.mjs": "L('idx');" });
main(`import './a';`, { "a.mjs": "L('a');" });
main(`import './a.js';`, { "a.mjs": "L('a');" });
main(`import './A.mjs';`, { "a.mjs": "L('a');" });
main(`import { x } from './sub/../a.mjs'; L(x);`, { "a.mjs": "export const x = 'norm';", "sub/z.mjs": "" });
main(`import { x } from './sub/../a.mjs'; import { x as y } from './a.mjs'; L(x === y);`, { "a.mjs": "L('once'); export const x = 'norm';", "sub/z.mjs": "" });
main(`import { x } from './a.mjs?q=1'; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import { x } from './a.mjs#frag'; L(x);`, { "a.mjs": "export const x = 1;" });
main(`import './a.mjs'; import './a.mjs?x'; L('main');`, { "a.mjs": "L('a');" });
main(`import { x } from './sub/a.mjs'; L(x);`, { "sub/a.mjs": "import { y } from '../b.mjs'; export const x = y + 1;", "b.mjs": "export const y = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "import { y } from './sub/b.mjs'; export const x = y + 1;", "sub/b.mjs": "import { z } from '../c.mjs'; export const y = z + 1;", "c.mjs": "export const z = 1;" });
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "import { y } from './sub/b.mjs'; export const x = y;", "sub/b.mjs": "import { z } from '../a.mjs'; export const y = typeof z;" }.constructor === Object ? { "a.mjs": "import { y } from './sub/b.mjs'; export const x = y;", "sub/b.mjs": "import { x } from '../a.mjs'; export const y = typeof x;".replace("typeof x", "1") } : {});
main(`import './a.mjs';`, { "a.mjs": "import 'file:///nope.mjs';" });
main(`import './a.mjs';`, { "a.mjs": "import 'node:nope';" });
main(`import './a.mjs';`, { "a.mjs": "import 'data:text/javascript,L(1)';" });
main(`import '';`, {});
main(`import ' ';`, {});
main(`import 'a b';`, {});
main(`import './a b.mjs'; L('sp');`, { "a b.mjs": "L('spaced');" });
main(`import './é.mjs'; L('uni');`, { "é.mjs": "L('accent');" });
main(`import { 'é' as e } from './a.mjs'; L(e);`, { "a.mjs": "const q = 1; export { q as 'é' };" });
main(`import { 'x y' as e } from './a.mjs'; L(e);`, { "a.mjs": "export const q = 1;" });

// 12. Corpos, escopo e semântica de módulo.
main(`import './a.mjs';`, { "a.mjs": "'use strict'; L(typeof this);" });
main(`import './a.mjs';`, { "a.mjs": "function f() { return this; } L(f() === undefined);" });
main(`import './a.mjs';`, { "a.mjs": "L((function () { return typeof this; })());" });
main(`import './a.mjs';`, { "a.mjs": "L((() => typeof this)());" });
main(`import './a.mjs';`, { "a.mjs": "try { undeclared = 1; } catch (e) { L(e.name + ': ' + e.message); }" });
main(`import './a.mjs';`, { "a.mjs": "try { Object.freeze([1]).push(2); } catch (e) { L(e.name); }" });
main(`import './a.mjs';`, { "a.mjs": "try { (function () { arguments.callee; })(); } catch (e) { L(e.name); }" });
main(`import './a.mjs';`, { "a.mjs": "const o = { get x() { return this === o; } }; L(o.x); L(typeof eval('1+1'));" });
main(`import './a.mjs';`, { "a.mjs": "var x = 'm'; L(eval('typeof x')); L(globalThis.x);" });
main(`import './a.mjs';`, { "a.mjs": "L(typeof globalThis); L(typeof window); L(typeof self);" });
main(`import './a.mjs'; L(typeof shared); `, { "a.mjs": "globalThis.shared = 1;" });
main(`import './a.mjs'; L(shared);`, { "a.mjs": "globalThis.shared = 'g';" });
main(`import './a.mjs'; L(v);`, { "a.mjs": "var v = 1;" }.constructor ? { "a.mjs": "globalThis.v = 'viaGlobal';" } : {});
main(`import { x } from './a.mjs'; L(x);`, { "a.mjs": "export const x = (() => { try { return typeof y; } catch (e) { return e.name; } })(); let y;" });
main(`import { f } from './a.mjs'; L(f());`, { "a.mjs": "export function f() { return typeof g; } function g() {}" });
main(`import { f } from './a.mjs'; L(f());`, { "a.mjs": "export const f = () => typeof g; var g = 1;" });
main(`import './a.mjs';`, { "a.mjs": "L(typeof f); function f() {} L(typeof C); try { new C(); } catch (e) { L(e.name); } class C {}" });
main(`import './a.mjs';`, { "a.mjs": "label: { L('in'); break label; L('never'); } L('out');" });
main(`import './a.mjs';`, { "a.mjs": "L([1,2,3].map(x => x * 2));" });
main(`import './a.mjs';`, { "a.mjs": "L(new Error('e', { cause: 1 }).cause);" });
main(`import './a.mjs';`, { "a.mjs": "class A { static #p = 1; static g() { return A.#p; } } L(A.g());" });
main(`import { A } from './a.mjs'; L(new A().v); L(A.s);`, { "a.mjs": "export class A { v = 1; static s = 2; }" });
main(`import { A } from './a.mjs'; class B extends A {} L(new B().hi());`, { "a.mjs": "export class A { hi() { return 'hi'; } }" });
main(`import { s } from './a.mjs'; L(s.description); L(s === Symbol.for('k'));`, { "a.mjs": "export const s = Symbol.for('k');" });
main(`import { o } from './a.mjs'; o.n = 2; import { r } from './a.mjs'; L(r());`, { "a.mjs": "export const o = { n: 1 }; export const r = () => o.n;" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "export const a = 1; export { a as b }; export { a as c };" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns)); L(ns.b === ns.a);`, { "a.mjs": "export const a = 1; export { a as b };" });
main(`import { b, a } from './a.mjs'; L(a + b);`, { "a.mjs": "export const a = 1; export { a as b };" });
main(`import { g } from './a.mjs'; L(g()); L(g.name); L(g.length);`, { "a.mjs": "export function g(a, b) { return a ?? 'u'; }" });
main(`import { e } from './a.mjs'; try { e(); } catch (x) { L(x.stack.split('\\n').length > 0); L(x.message); }`, { "a.mjs": "export function e() { throw new Error('de a'); }" });
main(`import './a.mjs';`, { "a.mjs": "L(new Error('x').stack.includes('a.mjs'));" });
main(`import './a.mjs';`, { "a.mjs": "L(new Error('x').stack.split('\\n')[1].replace(/\\(.*[\\\\/]/, '(').replace(/^.*[\\\\/]/, ''));" });

// 13. Ciclo de TLA e variações com ordem.
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; await null; L('a');", "b.mjs": "import './a.mjs'; await null; L('b');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; await null; L('c');" });
main(`import { a } from './a.mjs'; L(a);`, { "a.mjs": "import { b } from './b.mjs'; export const a = await Promise.resolve('A' + b);", "b.mjs": "import { a } from './a.mjs'; export const b = 'B';" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a'); await null;", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './b.mjs'; L('c'); await null;" });
main(`import './a.mjs'; import './b.mjs'; L('main');`, { "a.mjs": "import './c.mjs'; L('a');", "b.mjs": "import './c.mjs'; L('b');", "c.mjs": "import './a.mjs'; L('c1'); await null; L('c2');" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name + ': ' + e.message); }`, { "a.mjs": "import './b.mjs'; await null; L('a');", "b.mjs": "import './a.mjs'; throw new Error('b cai');" });
main(`try { await import('./a.mjs'); } catch (e) { L(e.name + ': ' + e.message); } L('fim');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './a.mjs'; await null; throw new Error('b cai tla');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "await new Promise(r => setTimeout(r, 3)); L('b');", "c.mjs": "await null; L('c');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; import './d.mjs'; L('a');", "b.mjs": "L('b1'); await null; L('b2');", "c.mjs": "L('c1'); await null; await null; L('c2');", "d.mjs": "L('d');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; L('a');", "b.mjs": "import './c.mjs'; import './d.mjs'; L('b');", "c.mjs": "await null; L('c');", "d.mjs": "L('d');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "import './b.mjs'; import './c.mjs'; L('a');", "b.mjs": "import './d.mjs'; L('b');", "c.mjs": "import './d.mjs'; L('c');", "d.mjs": "await null; L('d');" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "L('a1'); const x = await Promise.resolve(1); L('a2' + x); export {};" });
main(`import './a.mjs'; L('main');`, { "a.mjs": "export const p = new Promise(r => setTimeout(() => r('late'), 2)); L(await p);" });
main(`import { p } from './a.mjs'; L(await p);`, { "a.mjs": "export const p = Promise.resolve('p');" });
main(`import { p } from './a.mjs'; L('main'); L(await p);`, { "a.mjs": "export const p = new Promise(r => setTimeout(() => r('p2'), 1)); L('a');" });
main(`await 1; import('./a.mjs').then(n => L(n.x));`, { "a.mjs": "export const x = await 'in';" });

// 14. Tipos de valores exportados e import.meta.
main(`import { x } from './a.mjs'; L(typeof x); L(String(x));`, { "a.mjs": "export const x = Symbol('s');" });
main(`import { x } from './a.mjs'; L(typeof x); L(String(x));`, { "a.mjs": "export const x = 10n ** 20n;" });
main(`import { x } from './a.mjs'; L(x); L(Object.is(x, -0));`, { "a.mjs": "export const x = -0;" });
main(`import { x } from './a.mjs'; L(x); L(Number.isNaN(x));`, { "a.mjs": "export const x = NaN;" });
main(`import { x } from './a.mjs'; L(x === undefined); L('x' in {});`, { "a.mjs": "export const x = undefined;" });
main(`import { x } from './a.mjs'; L(x === null);`, { "a.mjs": "export const x = null;" });
main(`import { x } from './a.mjs'; L(x instanceof RegExp); L(x.source);`, { "a.mjs": "export const x = /a+b/g;" });
main(`import { x } from './a.mjs'; L(x instanceof Map); L(x.get(1));`, { "a.mjs": "export const x = new Map([[1, 'um']]);" });
main(`import { x } from './a.mjs'; L(x.next().value); L(x.next().done);`, { "a.mjs": "export const x = (function* () { yield 'g'; })();" });
main(`import { f } from './a.mjs'; L(await f());`, { "a.mjs": "export async function f() { return await 'af'; }" });
main(`import { a, b, c, d, e } from './a.mjs'; L([a, b, c, d, e]);`, { "a.mjs": "export const a = 1, b = 2, c = 3, d = 4, e = 5;" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "export const b = 1, a = 2, 1 = 3;".replace(", 1 = 3", "") + " export { a as '10', a as '2', a as 'a1' };" });
main(`import * as ns from './a.mjs'; L(Object.keys(ns));`, { "a.mjs": "export const $ = 1, _ = 2, Z = 3, a = 4, 𝒳 = 5;" });
main(`import { Z, a } from './a.mjs'; L(Z + a);`, { "a.mjs": "export const Z = 1, a = 2;" });
main(`import { default as x, default as y } from './a.mjs'; L(x === y);`, { "a.mjs": "export default {};" });
main(`import { x as a, x as b } from './a.mjs'; L(a === b);`, { "a.mjs": "export const x = {};" });
main(`import * as a from './a.mjs'; import * as b from './a.mjs'; L(a === b);`, { "a.mjs": "export const x = {};" });
main(`import d, * as ns from './a.mjs'; L(d); L(ns.default === d); L(Object.keys(ns));`, { "a.mjs": "export default 'dd'; export const q = 1;" });
main(`import d, { q } from './a.mjs'; L(d + q);`, { "a.mjs": "export default 'dd'; export const q = 1;" });
main(`import { q }, d from './a.mjs';`, { "a.mjs": "" });

// 15. import defer / source phase (bun 1.4 pode recusar a sintaxe; o resultado medido vale).
main(`import defer * as ns from './a.mjs'; L('main'); L(ns.x);`, { "a.mjs": "L('a'); export const x = 1;" });
main(`import source x from './a.mjs'; L('main');`, { "a.mjs": "L('a');" });

// Execução.
const dirs = [];
const lines = [];
const failures = [];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "zjsc-module-golden-"));
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
  // Mensagens de erro de saída do próprio bun (stderr) não entram: só o que o runner captura.
  const clean = JSON.stringify(files);
  if (/[\t\n\r]/.test(clean) || /[\t\n\r]/.test(out)) throw new Error("tab ou quebra de linha no caso " + i);
  if (out.includes(tmp)) failures.push(i);
  lines.push(clean + "\t" + out);
});
fs.rmSync(tmp, { recursive: true, force: true });
if (failures.length) throw new Error("caminho da máquina na saída dos casos " + failures.join(","));
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`casos: ${lines.length}\n`);
