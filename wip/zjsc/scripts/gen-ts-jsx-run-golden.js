// Gera tests/golden/ts_jsx_run_bun.tsv: `bun run` de arquivos .ts, .tsx e .jsx importados pelo ponto de entrada (enum,
// namespace, decorators do TC39, satisfies, import type, `using`, parâmetros-propriedade, JSX clássico e automático),
// avaliados no bun. O resultado de cada programa é o log (valores, `String(função)` e posições de `Error.stack`), no
// mesmo formato e com o mesmo runner dos goldens de módulo.
// Uso: RUNS=3 bun scripts/gen-ts-jsx-run-golden.js > tests/golden/ts_jsx_run_bun.tsv
const cases = [];
const seen = new Set();
const add = (files) => {
  const key = JSON.stringify(files);
  if (seen.has(key)) return;
  seen.add(key);
  cases.push(files);
};

// O runtime de JSX automático: devolve o que recebe, com a fonte (`fileName`, linha e coluna) quando há.
const JSX_DEV = `export const Fragment = Symbol.for('frag');
export function jsxDEV(type, props, key, isStatic, source, self) {
  const t = typeof type === 'string' ? type : type === Fragment ? 'Fragment' : type.name;
  const { children, ...rest } = props;
  return { t, rest, children, key, isStatic, source, self: self === undefined ? 'undefined' : typeof self };
}
`;
const JSX_PROD = `export const Fragment = Symbol.for('frag');
const make = (type, props, key) => {
  const t = typeof type === 'string' ? type : type === Fragment ? 'Fragment' : type.name;
  const { children, ...rest } = props;
  return { t, rest, children, key };
};
export const jsx = (type, props, key) => ({ s: 'jsx', ...make(type, props, key) });
export const jsxs = (type, props, key) => ({ s: 'jsxs', ...make(type, props, key) });
`;
const RUNTIME = { "node_modules/react/jsx-dev-runtime.js": JSX_DEV, "node_modules/react/jsx-runtime.js": JSX_PROD };
const PRE = `globalThis.React = { createElement: (t, p, ...c) => ({ ce: typeof t === 'string' ? t : t === React.Fragment ? 'Fragment' : t.name, p, c }), Fragment: Symbol.for('rf') };
globalThis.h = (t, p, ...c) => ({ h: typeof t === 'string' ? t : t === globalThis.Frag ? 'Frag' : t.name, p, c });
globalThis.Frag = Symbol.for('hf');
`;
const SHOW = `const show = (v) => L(JSON.stringify(v, (k, x) => typeof x === 'function' ? 'fn:' + String(x) : typeof x === 'symbol' ? 'sym:' + String(x) : typeof x === 'bigint' ? x + 'n' : x === undefined ? '(undef)' : x));
`;

// `body` roda no main.mjs depois de importar `./x.<ext>` como `m`.
const prog = (ext, source, body, extra = {}) =>
  add({ "main.mjs": PRE + SHOW + `const m = await import('./x.${ext}');\n${body}`, [`x.${ext}`]: source, ...RUNTIME, ...extra });
const ts = (source, body = "show(m);") => prog("ts", source, body);
const tsx = (source, body = "show(m);") => prog("tsx", source, body);
const jsx = (source, body = "show(m);") => prog("jsx", source, body);

// 1. enum
ts("export enum E { A, B, C }\nexport const v = [E.A, E.B, E.C, E[0], E[1], E[2]];");
ts("export enum E { A = 1, B = A << 2, C = B | 3, D = 'x'.length, F = ~A }\nexport const v = Object.entries(E);");
ts("export enum S { A = 'a', B = 'b', C = `c` }\nexport const v = Object.entries(S);");
ts("export const enum C { A = 1, B = 2 }\nexport const v = C.A + C.B;");
ts("export const enum C { A = 'x', B = 'y' }\nexport const v = [C.A, C.B, C['A']];");
ts("export declare enum D { A }\nexport const v = typeof D;");
ts("enum E { A, B }\nenum E { C = 5 }\nexport const v = Object.entries(E);");
ts("export enum E { A = 1.5, B, C = -1, D, 'x y' = 7 }\nexport const v = Object.entries(E);");
ts("export enum E { A = 1 / 0, B = 0 / 0, C = 2 ** 3, D = 7 % 4, E = 1 + 2 * 3 }\nexport const v = Object.entries(E);");
ts("export enum E { A = 'a'.length, B = Math.max(1, 2) }\nexport const v = Object.entries(E);");
ts("export enum E { A }\nexport const f = () => E.A;\nexport const s = String(f);");
ts("export enum E { A }\nexport const s = (() => { E; return 1; })().toString();\nexport const t = Object.getOwnPropertyDescriptors(E);");
ts("export enum E { A, B }\nexport const keys = Object.keys(E);\nexport const frozen = Object.isFrozen(E);");
ts("export enum E { A = 1, B = A + 1, C = B + A }\nexport const v = [E.A, E.B, E.C];\nexport const f = function () { return E.B; };\nexport const s = String(f);");
ts("export const enum C { A = 1, B = A * 2 }\nexport function f() { return [C.A, C.B]; }\nexport const s = String(f);");
ts("export enum E { A }\nexport namespace E { export const z = 9; }\nexport const v = [E.A, E.z];");

// 2. namespace
ts("export namespace N { export const a = 1; export function f() { return a; } }\nexport const v = [N.a, N.f()];");
ts("export namespace N { export namespace M { export const x = 2; } }\nexport const v = N.M.x;");
ts("export namespace N.M.O { export const x = 3; }\nexport const v = N.M.O.x;");
ts("namespace N { export const a = 1; }\nnamespace N { export const b = a + 1; }\nexport const v = [N.a, N.b];");
ts("export namespace N { export class C { x = 1; } export enum E { A } export let l = 5; l++; }\nexport const v = [new N.C().x, N.E.A, N.l];");
ts("export declare namespace D { const x: number; }\nexport const v = typeof D;");
ts("export namespace N { const hidden = 1; export const shown = hidden + 1; }\nexport const v = Object.keys(N);");
ts("export namespace N { export function f() { return 1; } export function f(x: number) { return x; } }\nexport const v = N.f(4);");
ts("export namespace N { export const f = () => 1; }\nexport const s = String(N.f);\nexport const g = String(function () { return N; });");
ts("namespace N { export let x = 1; export function inc() { x++; } }\nN.inc();\nexport const v = N.x;");
ts("export namespace N { export var a = 1, b = 2; export const [c, d] = [3, 4]; export const { e } = { e: 5 }; }\nexport const v = [N.a, N.b, N.c, N.d, N.e];");

// 3. import type / export type / type-only
ts("import type { T } from './t.ts';\nimport { type U, w } from './t.ts';\nexport const v = w;", "show(m);");
add({ "main.mjs": SHOW + "const m = await import('./x.ts'); show(m);", "x.ts": "import type { T } from './t.ts';\nimport { type U, w } from './t.ts';\nexport type V = T | U;\nexport { type T };\nexport const v = w;", "t.ts": "export type T = 1;\nexport type U = 2;\nexport const w = 'w';" });
add({ "main.mjs": SHOW + "const m = await import('./x.ts'); show(m);", "x.ts": "import { T } from './t.ts';\nconst a: T = 1;\nexport const v = a;", "t.ts": "export type T = number;" });
add({ "main.mjs": SHOW + "const m = await import('./x.ts'); show(m);", "x.ts": "import T, { U } from './t.ts';\nexport const v = 1;", "t.ts": "export type U = 1;\nexport default 2;" });
add({ "main.mjs": SHOW + "const m = await import('./x.ts'); show(m);", "x.ts": "export type { T } from './t.ts';\nexport { type U, w } from './t.ts';", "t.ts": "export type T = 1;\nexport type U = 2;\nexport const w = 'w';" });
add({ "main.mjs": SHOW + "const m = await import('./x.ts'); show(m);", "x.ts": "import * as ns from './t.ts';\nexport const v = ns.w;", "t.ts": "export type T = 1;\nexport const w = 'w';" });
ts("import type X = require('x');\nexport const v = 1;");
ts("declare const g: number;\ndeclare function gf(): void;\ndeclare class GC {}\ndeclare module 'foo' { export const x: number; }\nexport const v = 1;");
ts("interface I { a: number }\ntype T = { a: I };\nexport const v = 1;");
ts("abstract class A { abstract m(): number; }\nclass B extends A { m() { return 2; } }\nexport const v = new B().m();");

// 4. satisfies / as / non-null / generics / overloads / parameter properties / definite assignment
ts("const o = { a: 1 } satisfies { a: number };\nexport const v = o;");
ts("export const v = (1 as number) + (<number>2) + ('3' as unknown as number);");
ts("function f(x?: number | null) { return x!.toFixed(1); }\nexport const v = f(2);");
ts("function id<T>(x: T): T { return x; }\nexport const v = id<number>(5);\nexport const s = String(id);");
ts("function f(a: number): number;\nfunction f(a: string): string;\nfunction f(a: any) { return a; }\nexport const v = f(1);");
ts("class P { constructor(public a: number, private b = 2, readonly c?: string, protected d: number = 4) {} sum() { return this.a + this.b + this.d; } }\nexport const v = new P(1);\nexport const s = String(P);");
ts("class Q { x!: number; y?: string; declare z: number; static s: number = 3; #p = 1; get p() { return this.#p; } }\nexport const v = [Object.keys(new Q()), Q.s, new Q().p];");
ts("class Base { constructor(public n: number) {} }\nclass D extends Base { constructor(public m: number, n: number) { super(n); } }\nexport const v = new D(1, 2);");
ts("export class C<T extends object = {}> implements Iterable<T> { *[Symbol.iterator]() {} }\nexport const v = typeof C;");
ts("export default class<T> {}\n");
ts("export const f = <T,>(x: T) => x;\nexport const s = String(f);");
ts("export const f = async <T,>(x: T): Promise<T> => x;\nexport const v = await f(3);");
ts("let x: number = 1;\nx += 2;\nexport const v = x;\nexport const s = `${typeof x}`;");
ts("export const v = (() => { type T = number; const a: T = 1; return a; })();");
ts("function f(this: { a: number }) { return this.a; }\nexport const v = f.call({ a: 7 });\nexport const s = String(f);");
ts("export const o = { m<T>(x: T): T { return x; }, get g(): number { return 1; }, set g(v: number) {} };\nexport const v = [o.m(1), o.g];");
ts("export const [a = 1 as number, b] = [undefined, 2] as const;\nexport const v = [a, b];");
ts("enum E { A }\nexport const v = E.A satisfies number;");
ts("export const v = <const T,>(x: T) => x;\nexport const s = String(v);");
ts("export abstract class A { abstract x: number; abstract m(): void; static create(): A { return null as any; } }\nexport const v = typeof A.create;");
ts("export class A { static #c = 1; static get c() { return A.#c; } static { A.#c++; } }\nexport const v = A.c;");
ts("export let x = 1;\nexport { x as y };\nexport const set = () => { x = 2; };", "m.set(); show([m.x, m.y]);");
ts("export = 1;");
ts("import A = Foo.Bar;\nnamespace Foo { export const Bar = 1; }\nexport const v = A;");
ts("export import B = Foo.Bar;\nnamespace Foo { export const Bar = 1; }\nexport const v = B;");

// 5. decorators (TC39, sem experimentalDecorators)
ts("function dec(v: any, ctx: any) { return class extends v { extra = 1; }; }\n@dec\nexport class C { a = 0; }\nexport const v = new C();");
ts("const log: any[] = [];\nfunction m(fn: any, ctx: any) { log.push([ctx.kind, ctx.name, ctx.static, ctx.private]); return function (this: any, ...a: any[]) { return fn.apply(this, a) + 1; }; }\nexport class C { @m f() { return 1; } @m static s() { return 10; } @m #p() { return 5; } callP() { return this.#p(); } }\nexport const v = [new C().f(), C.s(), new C().callP(), log];");
ts("function fld(_: any, ctx: any) { return (x: number) => x + 100; }\nexport class C { @fld a = 1; @fld static b = 2; }\nexport const v = [new C().a, C.b];");
ts("function acc(target: any, ctx: any) { return { get() { return target.get.call(this) * 2; }, set(v: number) { target.set.call(this, v + 1); }, init(v: number) { return v + 10; } }; }\nexport class C { @acc accessor a = 1; }\nconst c = new C(); c.a = 5;\nexport const v = [c.a];");
ts("const order: string[] = [];\nconst d = (n: string) => (v: any, ctx: any) => { order.push(n + ':' + ctx.kind + ':' + String(ctx.name)); };\n@d('c1') @d('c2')\nexport class C { @d('m') m() {} @d('f') f = 1; @d('s') static s() {} }\nexport const v = order;");
ts("function dec(_: any, ctx: any) { ctx.addInitializer(function (this: any) { this.init = true; }); }\nexport class C { @dec m() {} }\nexport const v = new C().init === undefined ? 'noinst' : 'inst';\nconst c = new C(); c.m();\nexport const w = (c as any).init;");
ts("export const dec = (v: any, ctx: any) => v;\n@dec class A {}\n@dec export class B {}\nexport const v = [A.name, B.name];");
ts("export default @((v: any) => v) class {}\n");

// 6. using / await using
ts("const log: string[] = [];\n{ using a = { [Symbol.dispose]() { log.push('a'); } }; using b = { [Symbol.dispose]() { log.push('b'); } }; log.push('body'); }\nexport const v = log;");
ts("const log: string[] = [];\nexport async function f() { await using a = { async [Symbol.asyncDispose]() { log.push('a'); } }; log.push('body'); }\nawait f();\nexport const v = log;");
ts("export function f() { using a = null; using b = undefined; return 1; }\nexport const v = f();\nexport const s = String(f);");
ts("const log: string[] = [];\nfor (using x of [{ [Symbol.dispose]() { log.push('d'); } }]) { log.push('it'); }\nexport const v = log;");
ts("export function f() { using a = { [Symbol.dispose]() { throw new Error('x'); } }; throw new Error('y'); }\nexport const v = (() => { try { f(); } catch (e: any) { return [e.constructor.name, e.error?.message, e.suppressed?.message]; } })();");

// 7. posições do runtime (stack) depois de apagar tipos
ts("function f(x: number): number {\n  type T = 1;\n  return new Error('e').stack!.split('\\n')[1];\n}\nexport const v = f(1);");
ts("enum E { A, B }\nexport function g() {\n  return new Error('e').stack!.split('\\n')[1];\n}\nexport const v = g();");
ts("interface I {}\nclass K {\n  m(): string {\n    return new Error('e').stack!.split('\\n')[1];\n  }\n}\nexport const v = new K().m();");
ts("namespace N {\n  export function f() {\n    return new Error('e').stack!.split('\\n')[1];\n  }\n}\nexport const v = N.f();");
ts("export function f<T>(x: T) {\n  const y = (x as any) as number;\n  return new Error('e').stack!.split('\\n')[1];\n}\nexport const v = f(1);");
ts("export function f() {\n  throw new TypeError('t');\n}\nexport const v = (() => { try { f(); } catch (e: any) { return e.stack.split('\\n').slice(0, 2).join('|'); } })();");

// 8. JSX clássico e automático
jsx("export const v = <div id=\"a\" className={'b'}>hi {1 + 1} <b>x</b></div>;");
jsx("/** @jsxRuntime classic */\nexport const v = <div id=\"a\">hi</div>;");
jsx("/** @jsxRuntime classic */\nexport const v = <><b/>x</>;");
jsx("/** @jsxRuntime classic */\n/** @jsx h */\n/** @jsxFrag Frag */\nexport const v = <><b/>x</>;");
jsx("/** @jsxRuntime classic */\n/** @jsx h */\nexport const v = <div {...{ a: 1 }} b=\"2\" {...{ c: 3 }}>text</div>;");
jsx("/** @jsxRuntime classic */\nfunction C(p) { return p; }\nexport const v = <C a=\"1\"><i/></C>;");
jsx("/** @jsxRuntime classic */\nexport const v = <div>{...[1, 2]}</div>;");
jsx("export const v = <div a=\"1\" key=\"k\">x</div>;");
jsx("export const v = <div>a<b/>c</div>;");
jsx("export const v = <><b/><i/></>;");
jsx("function C(p) { return p; }\nexport const v = <C a=\"1\" {...{ b: 2 }} key=\"z\"/>;");
jsx("export const v = <div {...{ key: 'k', a: 1 }}>x</div>;");
jsx("export const v = <a:b c:d=\"1\"/>;");
jsx("export const v = <div title=\"&amp; &lt; &#65; &nbsp;x\">&amp; &lt; &#65; &nbsp; y &copy;</div>;");
jsx("export const v = <div>\n  multi\n  line\n   text  \n  <b/>\n</div>;");
jsx("export const v = <div data-x={`t${1}`} {...null} aria-label=\"l\"/>;");
jsx("const o = { C: function () { return 1; } };\nexport const v = <o.C a=\"1\"/>;");
jsx("export const v = [1, 2].map((i) => <li key={i}>{i}</li>);");
jsx("export const f = () => <div>x</div>;\nexport const s = String(f);");
jsx("export function f() {\n  return new Error('e').stack.split('\\n')[1];\n}\nexport const v = <div>{f()}</div>;");
jsx("/** @jsxImportSource react */\nexport const v = <div>x</div>;");
tsx("export const v = <div a=\"1\">x</div>;");
tsx("const f = <T,>(x: T) => x;\nexport const v = <div>{f<number>(1)}</div>;");
tsx("function C<T>(p: { a: T }) { return p; }\nexport const v = <C<number> a={1}/>;");
tsx("export const v = <div>{(1 as number)}</div>;");
tsx("/** @jsxRuntime classic */\nexport const v = <div>{1 satisfies number}</div>;");
tsx("enum E { A }\nexport const v = <div>{E.A}</div>;");

require("./module-golden-run.js").runModuleGolden("ts-jsx-run-golden", `
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
`, cases);
