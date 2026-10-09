// Gera tests/golden/stack_column_bun.tsv: a coluna que o bun mostra em cada frame de `Error.prototype.stack` para
// chamadas escritas de muitas formas. O bun transpila todo fonte e remapeia a posição do JSC (o `(` da chamada)
// pelo source map do transpilador, então a coluna exibida é o início do último token mapeável com início <= divot
// na mesma linha (regra em wip/notes/stack-column-rule.md, porte em src/runtime/stack_frame.rs::callee_back_offset).
// Por isso cada programa roda como ARQUIVO real (`bun error_stack_case.js`), nunca por `vm.runInThisContext`, que
// pularia o source map. O programa normaliza a própria pilha (só `linha:coluna` dos frames do arquivo), então o
// resultado independe do caminho da máquina. Colunas do tsv: programa (JSON) e valor de `globalThis.R` (JSON).
// Uso: bun scripts/gen-stack-column-golden.js > tests/golden/stack_column_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const FILE = "error_stack_case.js";

// Prelúdio: `N` normaliza a pilha, `t` captura a pilha, `o` e amigos são os alvos das chamadas.
const PRELUDE = [
  'var G = globalThis;',
  'function N(s) { if (s && typeof s === "object") s = s.s; return String(s).split("\\n").filter(function (l) { return l.indexOf("' + FILE + '") >= 0 }).map(function (l) { var m = /error_stack_case\\.js:(\\d+)(?::(\\d+))?/.exec(l); return m ? m[1] + (m[2] ? ":" + m[2] : "") : l }).join(" ") }',
  'function t() { return new Error("x").stack }',
  'function F() { this.s = new Error("x").stack }',
  'var h = function () { return t };',
  'var k = "p";',
  'var o = { p: t, n: { q: t }, a: [t, t], F: F, "str": t, get g() { return t }, m: function () { return t }, ["c" + "d"]: t };',
  '',
].join("\n");

// Espaços/quebras entre o callee e o `(`.
const gaps = ["", " ", "\n", "\n    ", " /* c */ ", "\n// c\n", "\n/* c */ "];

// Cada forma recebe o intervalo `$` que fica antes do `(` (ou antes do argumento do template) e devolve a expressão.
const forms = [
  $ => `t${$}()`,
  $ => `o.p${$}()`,
  $ => `o.n.q${$}()`,
  $ => `(o.p)${$}()`,
  $ => `(0, o.p)${$}()`,
  $ => `(0,o.p)${$}()`,
  $ => `(t)${$}()`,
  $ => `((t))${$}()`,
  $ => `o.p?.${$}()`,
  $ => `o.p${$}?.()`,
  $ => `o?.p${$}()`,
  $ => `o?.n?.q${$}()`,
  $ => `o.n?.q${$}?.()`,
  $ => `o[k]${$}()`,
  $ => `o[ k ]${$}()`,
  $ => `o[ k ] ${$}()`,
  $ => `o["p"]${$}()`,
  $ => `o['p']${$}()`,
  $ => `o[\`p\`]${$}()`,
  $ => `o.a[0+0]${$}()`,
  $ => `o.a[0]${$}()`,
  $ => `o.a[ 1 ]${$}()`,
  $ => `o.a[o.a.length-1]${$}()`,
  $ => `o.a[o.a.length - 1]${$}()`,
  $ => `o.a[o.a.length-1 ]${$}()`,
  $ => `o.a[(1)]${$}()`,
  $ => `o.a[k === "p" ? 0 : 1]${$}()`,
  $ => `o.n["q"]${$}()`,
  $ => `o["n"].q${$}()`,
  $ => `o["n"]["q"]${$}()`,
  $ => `o.str${$}()`,
  $ => `o.cd${$}()`,
  $ => `o.g${$}()`,
  $ => `o.m()${$}()`,
  $ => `h()${$}()`,
  $ => `h()${$}(${$})`,
  $ => `(h)()${$}()`,
  $ => `(h)${$}()${$}()`,
  $ => `((h)())${$}()`,
  $ => `(h())${$}()`,
  $ => `h(${$})${$}()`,
  $ => `h( )( )`.replace("( )( )", `(${$})${$}(${$})`),
  $ => `new F${$}()`,
  $ => `new F${$}`,
  $ => `new (F)${$}()`,
  $ => `new (F${$})${$}()`,
  $ => `new o.F${$}()`,
  $ => `new o.F${$}`,
  $ => `new o["F"]${$}()`,
  $ => `new o[k === "p" ? "F" : "F"]${$}()`,
  $ => `new (o.F)${$}()`,
  $ => `new (h()${$}.constructor)${$}()`.replace("h()", "F"),
  $ => `new F(${$})`,
  $ => `new F ( ${$} ) `,
  $ => `o.p${$}\`x\``,
  $ => `t${$}\`x\``,
  $ => `o.n.q${$}\`x\``,
  $ => `o.p\`x\`${$}\`y\``,
  $ => `o["p"]${$}\`x\``,
  $ => `(0, o.p)${$}\`x\``,
  $ => `o.p${$}\`x\${1}y\``,
  $ => `\`a\${o.p${$}()}b\``,
  $ => `\`a\${ t${$}() }b\``,
  $ => `\`\${o.n.q${$}()}\``,
  $ => `\`\${ (0, o.p)${$}() }\``,
  $ => `\`x\${1}\${t${$}()}\``,
  $ => `(() => o.p${$}())()`,
  $ => `(() => t${$}())()`,
  $ => `(() => { return o.p${$}() })()`,
  $ => `(() => ${$} o.p${$}())()`,
  $ => `(async () => o.p${$}())()`,
  $ => `[1].map(() => o.p${$}())[0]`,
  $ => `[1].map(function () { return o.p${$}() })[0]`,
  $ => `({ get g() { return o.p${$}() } }).g`,
  $ => `({ get g() { return t${$}() } }).g`,
  $ => `({ set g(v) { G.R = N(o.p${$}()) } }).g = 1, G.R`,
  $ => `({ m() { return o.n.q${$}() } }).m()`,
  $ => `({ m() { return o[k]${$}() } }).m()`,
  $ => `o.p${$}(...[1, 2])`,
  $ => `o.p${$}(...[1, 2], ...[3])`,
  $ => `o.p(...[1, 2]${$})`,
  $ => `t${$}(...[1])`,
  $ => `t(1, ...[2]${$})`,
  $ => `o.p${$}(1, 2, 3)`,
  $ => `o.p(${$}1${$},${$}2${$})`,
  $ => `o.p(t${$}(), 2)`,
  $ => `o.p(1, t${$}())`,
  $ => `[o.p${$}()]`,
  $ => `[o.p${$}(), 1][0]`,
  $ => `({ a: o.p${$}() }).a`,
  $ => `[0, 1].length ? o.p${$}() : 0`,
  $ => `0 || o.p${$}()`,
  $ => `1 && o.n.q${$}()`,
  $ => `null ?? o.p${$}()`,
  $ => `!o.p${$}()`.replace("!", "") ,
  $ => `(1, o.p${$}())`,
  $ => `void 0, o.p${$}()`,
  $ => `typeof o.p${$}() === "string" ? o.p${$}() : 0`.replace(/^typeof [^?]+\?/, "1 ?").replace(/ : 0$/, " : 0"),
  $ => `o.p${$}().concat("")`,
  $ => `o.p${$}()${$}.concat("")`,
  $ => `o.p().concat(o.p${$}())`,
  $ => `o\n  .p${$}()`,
  $ => `o\n  .n\n  .q${$}()`,
  $ => `o\n.n\n.q${$}()\n.concat("")`,
  $ => `o.n\n  .q${$}()`,
  $ => `o\n  ?.n\n  ?.q${$}()`,
  $ => `o.p\n()`.replace("\n", $),
  $ => `h()\n()`.replace("\n", $),
  $ => `h${$}()${$}()`,
  $ => `h${$}()\n  ()`,
  $ => `o.p /* a */ ${$} /* b */ ()`,
  $ => `o /* a */ . /* b */ p ${$} ()`,
  $ => `o // a\n  . // b\n  p ${$}()`,
  $ => `o.p ${$}  (  )`,
  $ => `o . p ${$} ( )`,
  $ => `o . n . q ${$} ( )`,
  $ => `o [ "p" ] ${$} ( )`,
  $ => `o [ k ] ${$} ( )`,
  $ => `new   F  ${$}  (  )`,
  $ => `new /* a */ F ${$}()`,
  $ => `new\nF${$}()`,
  $ => `new\n  o.F${$}()`,
  $ => `await0(o.p${$}())`.replace("await0", ""),
  $ => `(function () { return o.p${$}() })()`,
  $ => `(function () { return o.p${$}() }).call(null)`,
  $ => `(function () { return t${$}() })${$}()`,
  $ => `(function () { return (0, o.p)${$}() })()`,
  $ => `(function () { return o.n.q${$}() }).apply(null, [])`,
  $ => `Reflect.apply(function () { return o.p${$}() }, null, [])`,
  $ => `eval("o.p${$.replace(/\n/g, "\\n").replace(/\//g, "/")}()")`,
  $ => `new Function("return o.p${$.replace(/\n/g, "\\n")}()")()`,
  $ => `o.p.call(null)`,
  $ => `o.p.call${$}(null)`,
  $ => `o.p.apply${$}(null, [])`,
  $ => `o.p.bind${$}(null)${$}()`,
  $ => `t.call${$}(null)`,
  $ => `Reflect.apply${$}(t, null, [])`,
  $ => `Reflect.construct${$}(F, [])`,
  $ => `[t].map(f => f${$}())[0]`,
  $ => `[o.p].map(f => f${$}())[0]`,
  $ => `[1].map(() => (0, o.p)${$}())[0]`,
  $ => `[1].map(() => (o.p)${$}())[0]`,
  $ => `[1].map(() => o?.p${$}())[0]`,
  $ => `[1].map(() => new F${$}())[0]`,
  $ => `[1].map(() => o.p${$}\`x\`)[0]`,
  $ => `Array.from([1], () => o.p${$}())[0]`,
  $ => `"a".replace("a", () => o.p${$}())`,
  $ => `"a".replace(/a/, function () { return t${$}() })`,
  $ => `/a/.test("a") ? o.p${$}() : 0`,
  $ => `"a/b".split("/").length > 1 ? o.p${$}() : 0`,
  $ => `o.p${$}() /* c */`,
  $ => `o.p${$}() // c`,
  $ => `"é" && o.p${$}()`,
  $ => `"日本" && o.n.q${$}()`,
  $ => `"\\u00e9" && o.p${$}()`,
  $ => `'it\\'s' && o.p${$}()`,
  $ => `1_0 && o.p${$}()`,
  $ => `0x1F && o.p${$}()`,
  $ => `10n && o.p${$}()`,
  $ => `.5 && o.p${$}()`,
  $ => `1e3 && o[k]${$}()`,
  $ => `[1, 2] && o.p${$}()`,
  $ => `({}) && o.p${$}()`,
  $ => `({ a: 1 }) && o.n.q${$}()`,
  $ => `(function () {}) && o.p${$}()`,
  $ => `(() => 0) && o.p${$}()`,
  $ => `null || o.p${$}()`,
  $ => `this === G || o.p${$}()`,
  $ => `typeof o && o.p${$}()`,
  $ => `-1 < 0 && o.p${$}()`,
  $ => `2 ** 2 && o.p${$}()`,
  $ => `new F().s && o.p${$}()`,
  $ => `new F().s.length && o.p${$}()`,
  $ => `o.p${$}().length && o.p${$}()`,
  $ => `[o.p${$}(), o.n.q${$}()][1]`,
  $ => `[...[1], o.p${$}()][1]`,
  $ => `({ ...{ a: 1 }, b: o.p${$}() }).b`,
  $ => `((a, b) => b)(1, o.p${$}())`,
  $ => `((a = o.p${$}()) => a)()`,
  $ => `(({ a = o.p${$}() }) => a)({})`,
  $ => `(([a = t${$}()]) => a)([])`,
  $ => `(function f(a = o.p${$}()) { return a })()`,
  $ => `o.p${$}()?.concat("")`,
  $ => `o.p${$}()?.[0]`,
  $ => `o?.[k]${$}()`,
  $ => `o?.["p"]${$}()`,
  $ => `o?.a?.[0]${$}()`,
  $ => `o.a?.[0]?.${$}()`,
  $ => `o.nn?.q${$}?.() ?? o.p${$}()`,
  $ => `(o?.p)${$}()`,
  $ => `(o?.n.q)${$}()`,
  $ => `(o.a[0])${$}()`,
  $ => `(o["p"])${$}()`,
  $ => `(0, o.a[0])${$}()`,
  $ => `(0, o[k])${$}()`,
  $ => `(0, o["p"])${$}()`,
  $ => `(0, t)${$}()`,
  $ => `(o.n, o.p)${$}()`,
  $ => `(1 ? o.p : 0)${$}()`,
  $ => `(1 ? o.p : 0)${$}\`x\``,
  $ => `(null ?? o.p)${$}()`,
  $ => `(o.p || t)${$}()`,
  $ => `(o.p && t)${$}()`,
  $ => `(0, h())${$}()`,
  $ => `h()${$}()${$}.concat("")`,
  $ => `h()()${$}.concat("")`,
  $ => `h()(1)(2)`.replace("(1)(2)", `${$}()`),
  $ => `h()${$}()`,
  $ => `h()\n\n  ()`.replace("\n\n  ", $),
  $ => `o.m()${$}()`,
  $ => `o.m(${$})${$}(${$})`,
  $ => `o.m().call${$}(null)`,
  $ => `new (o.m())${$}()`.replace("new (o.m())()", "new F()"),
];

// Casos que precisam de declarações próprias (classes e super).
const classCases = [
  $ => ({ pre: 'class A { m() { return new Error("x").stack } }\nclass C extends A { m() { return super.m' + $ + '() } }\n', expr: 'new C().m()' }),
  $ => ({ pre: 'class A { m() { return new Error("x").stack } }\nclass C extends A { m() { return super["m"]' + $ + '() } }\n', expr: 'new C().m()' }),
  $ => ({ pre: 'class A { m() { return new Error("x").stack } }\nclass C extends A { m() { return (() => super.m' + $ + '())() } }\n', expr: 'new C().m()' }),
  $ => ({ pre: 'class A { static m() { return new Error("x").stack } }\nclass C extends A { static m() { return super.m' + $ + '() } }\n', expr: 'C.m()' }),
  $ => ({ pre: 'class A { m() { return new Error("x").stack } }\nclass C extends A { m() { return super.m' + $ + '`x` } }\n', expr: 'new C().m()' }),
  $ => ({ pre: 'class A { m() { return new Error("x").stack } }\nclass C extends A { m() { return super.m.call' + $ + '(this) } }\n', expr: 'new C().m()' }),
  $ => ({ pre: 'class A { constructor() { this.s = new Error("x").stack } }\nclass C extends A { constructor() { super' + $ + '() } }\n', expr: 'new C()' }),
  $ => ({ pre: 'class A { constructor() { this.s = new Error("x").stack } }\nclass C extends A { constructor() { super(' + $ + ') } }\n', expr: 'new C()' }),
  $ => ({ pre: 'class A { constructor() { this.s = new Error("x").stack } }\nclass C extends A { constructor(...a) { super' + $ + '(...a) } }\n', expr: 'new C()' }),
  $ => ({ pre: 'class A { constructor() { this.s = new Error("x").stack } }\nclass C extends A { }\n', expr: 'new C' + $ + '()' }),
  $ => ({ pre: 'class K { m() { return o.p' + $ + '() } }\n', expr: 'new K().m()' }),
  $ => ({ pre: 'class K { static m() { return o.n.q' + $ + '() } }\n', expr: 'K.m()' }),
  $ => ({ pre: 'class K { get g() { return o.p' + $ + '() } }\n', expr: 'new K().g' }),
  $ => ({ pre: 'class K { static get g() { return t' + $ + '() } }\n', expr: 'K.g' }),
  $ => ({ pre: 'class K { constructor() { this.s = N(o.p' + $ + '()) } }\n', expr: 'new K().s' }),
  $ => ({ pre: 'class K { f = o.p' + $ + '(); }\n', expr: 'new K().f' }),
  $ => ({ pre: 'class K { static f = o.p' + $ + '(); }\n', expr: 'K.f' }),
  $ => ({ pre: 'class K { static { G.S = o.p' + $ + '() } }\n', expr: 'G.S' }),
  $ => ({ pre: 'class K { #p() { return t() } m() { return this.#p' + $ + '() } }\n', expr: 'new K().m()' }),
  $ => ({ pre: 'class K { static #p() { return t() } static m() { return K.#p' + $ + '() } }\n', expr: 'K.m()' }),
  $ => ({ pre: 'class K { #p = t; m() { return this.#p' + $ + '() } }\n', expr: 'new K().m()' }),
  $ => ({ pre: 'function* g() { yield o.p' + $ + '() }\n', expr: 'g().next().value' }),
  $ => ({ pre: 'function* g() { yield* [o.p' + $ + '()] }\n', expr: 'g().next().value' }),
  $ => ({ pre: 'var a = (b) => b;\n', expr: 'a(o.p' + $ + '())' }),
  $ => ({ pre: 'var a = (b) => b;\n', expr: 'a' + $ + '(o.p' + $ + '())' }),
  $ => ({ pre: 'label: {\n  G.S = o.p' + $ + '();\n}\n', expr: 'G.S' }),
  $ => ({ pre: 'var S;\nif (1) S = o.p' + $ + '();\n', expr: 'S' }),
  $ => ({ pre: 'var S;\nfor (var i = 0; i < 1; i++) S = o.p' + $ + '();\n', expr: 'S' }),
  $ => ({ pre: 'var S;\ntry { throw 0 } catch (e) { S = o.p' + $ + '() }\n', expr: 'S' }),
  $ => ({ pre: 'var S;\nswitch (1) { case 1: S = o.n.q' + $ + '() }\n', expr: 'S' }),
  $ => ({ pre: 'var S, T = o;\nwith (T) { S = p' + $ + '() }\n', expr: 'S' }),
];

const programs = [];
for (const form of forms) {
  for (const gap of gaps) {
    let expr;
    try {
      expr = form(gap);
    } catch (e) {
      continue;
    }
    // Chamada na mesma linha do `N(` e em linha própria com recuo.
    programs.push(PRELUDE + "G.R = N(" + expr + ")\n");
    programs.push(PRELUDE + "G.R = N(\n    " + expr + "\n)\n");
  }
}
for (const make of classCases) {
  for (const gap of gaps) {
    const { pre, expr } = make(gap);
    programs.push(PRELUDE + pre + "G.R = N(" + expr + ")\n");
    programs.push(PRELUDE + pre + "G.R = N(\n    " + expr + "\n)\n");
  }
}

// ---- Execução: `bun error_stack_case.js` num diretório novo, com TZ fixo, e `G.R` impresso por um preload.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stack-column-golden-"));
const file = path.join(dir, FILE);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const source of programs) {
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], {
    encoding: "utf8",
    cwd: dir,
    timeout: 15000,
    env: { ...process.env, TZ: "America/Sao_Paulo" },
  });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result === "<undefined>" || result === "" || /\/home\/|\/tmp\/|\/Users\/|\/var\/folders\//.test(result)) {
    dropped++;
    process.stderr.write("resultado vazio ou com caminho: " + JSON.stringify(source.slice(PRELUDE.length)) + " => " + JSON.stringify(result) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
