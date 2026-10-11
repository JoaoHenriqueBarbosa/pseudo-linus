// Gera tests/golden/stack_bun.tsv: o `Error.stack` de cada programa medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON, porque tem várias linhas) e o valor da variável global `R` que o
// programa grava (JSON). A fonte já é o texto que roda: diretiva "use strict" e `globalThis.R`. O arquivo se chama `stack_case.js`; o bun mostra o caminho absoluto, e o diretório
// temporário sai do texto, então o que sobra é o que o zjsc imprime com a mesma URL.
// Programas que gravam `R` depois de `await`/`then` valem porque o bun lê `R` na saída do processo.
// Também grava tests/golden/stack.preludes.json (prelúdios fatorados) e a quinta coluna (modo e mapa de posições).
// Uso: bun scripts/gen-stack-golden.js > tests/golden/stack_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const lines = (...rows) => rows.join("\n");

const programs = [
  // Throw em função aninhada, e a coluna de cada chamador.
  lines('function a() { throw new Error("boom") }', "function b() { a() }", "try { b() } catch (e) { R = e.stack }"),
  lines("function f() { return new Error('x').stack }", "var r = f()", "R = r"),
  lines("function f() { return new Error('x').stack }", "  R = f()"),
  lines("function f() { return new Error('x').stack }", "var x = 1; R = f()"),
  lines("function f() { return new Error('x').stack }", "R = [", "  f()", "][0]"),
  lines("function f() { return new Error('x').stack }", "function g() {", "  var r = f()", "  return r", "}", "R = g()"),
  lines("function f() { return new Error('x').stack }", "var o = { m() { var r = f(); return r } }", "R = o.m()"),
  lines("function f() { return new Error('x').stack }", "var h = function () { var r = f(); return r }", "R = h()"),
  lines("function f() { return new Error('x').stack }", "var h = () => { var r = f(); return r }", "R = h()"),
  lines("function f() { return new Error('x').stack }", "var o = { fn: function () { var r = f(); return r }, ar: () => { var r = f(); return r } }", "R = o.fn() + o.ar()"),
  // Classe: método, estático, getter, construtor com new, derivada.
  lines("class K { m() { throw new Error('m') } }", "try { new K().m() } catch (e) { R = e.stack }"),
  lines("class K { static s() { throw new Error('s') } }", "try { K.s() } catch (e) { R = e.stack }"),
  lines("class K { get g() { throw new Error('g') } }", "try { new K().g } catch (e) { R = e.stack }"),
  lines("class K { constructor() { throw new Error('ctor') } }", "try { new K() } catch (e) { R = e.stack }"),
  lines("function F() { this.e = new Error('inF') }", "R = new F().e.stack"),
  lines("class B { constructor() { this.s = new Error('b').stack } }", "class C extends B { constructor() { super() } }", "R = new C().s"),
  lines("class A { static #p() { return new Error('p').stack } static q() { var r = A.#p(); return r } }", "R = A.q()"),
  lines("class E2 extends Error { constructor() { super('sub') } }", "R = new E2().stack"),
  lines("class E3 extends Error {}", "R = new E3('sub3').stack"),
  // Função anônima, arrow, função com nome inferido, símbolo e chave com espaço.
  lines("try { (() => { throw new Error('arrow') })() } catch (e) { R = e.stack }"),
  lines("function show(f) { try { f() } catch (e) { return e.stack } }", "R = show(() => { throw new Error('cb') })"),
  lines("function show(f) { try { f() } catch (e) { return e.stack } }", "R = show(function () { throw new Error('cb') })"),
  lines("var o = { get p() { return new Error('get').stack } }", "R = o.p"),
  lines("var s = Symbol('s'); var o = { [s]() { return new Error('x').stack } }", "R = o[s]()"),
  lines("var o = { 'a b'() { return new Error('x').stack } }", "R = o['a b']()"),
  lines("function* gen() { var e = new Error('x'); yield e.stack }", "R = gen().next().value"),
  // eval, eval indireto e new Function.
  lines("function g() { return new Error('x').stack }", "R = eval('g()')"),
  lines("R = eval(\"\\n\\n  new Error('e').stack\")"),
  lines("R = (0, eval)(\"new Error('e').stack\")"),
  lines("R = new Function('a', \"return new Error('e').stack\")()"),
  // Async e Promise.
  lines("async function as() { await 1; R = new Error('async').stack }", "as()"),
  lines("async function as() { R = new Error('sync part').stack }", "as()"),
  lines("Promise.resolve().then(function th() { R = new Error('then').stack })"),
  lines("Promise.resolve().then(() => { R = new Error('then').stack })"),
  lines("new Promise(function exec(resolve) { R = new Error('exec').stack; resolve() })"),
  // Nativas no meio da pilha.
  lines("R = [1].map(function cb() { return new Error('map').stack })[0]"),
  lines("R = [1].map(() => new Error('map').stack)[0]"),
  lines("[1].forEach(function () { R = new Error('fe').stack })"),
  lines("[3, 1, 2].sort(function (a, b) { R = new Error('sort').stack; return a - b })"),
  lines("JSON.parse('[1]', function (k, v) { R = new Error('parse').stack; return v })"),
  lines("'a'.replace('a', function () { R = new Error('rep').stack; return 'b' })"),
  lines("function F() { this.e = new Error('inF') }", "R = Reflect.construct(F, []).e.stack"),
  lines("function nm() { return new Error('c').stack }", "R = Function.prototype.call.call(nm)"),
  lines("function nm() { return new Error('c').stack }", "R = Reflect.apply(nm, null, [])"),
  lines("function nm() { return new Error('c').stack }", "R = nm.call(null)"),
  lines("function nm() { return new Error('c').stack }", "R = nm.bind(null)()"),
  // Top-level e posição.
  lines("R = new Error('top').stack"),
  lines("R = new TypeError('t').stack"),
  lines("R = (function () { return new Error('anon').stack })()"),
  lines("", "", "   R = new Error('later line').stack"),
  lines("R = Error('nonew').stack"),
  // Cabeçalho.
  lines("R = new Error().stack"),
  lines("R = new Error('a\\nb').stack"),
  lines("var e = new Error('m'); e.name = 'Custom'; R = e.stack"),
  lines("try { null.x } catch (e) { R = e.stack }"),
  lines("try { undefinedFn() } catch (e) { R = e.stack }"),
  lines("function f() { undefinedFn() }", "try { f() } catch (e) { R = e.stack }"),
  lines("try { (function () { 'use strict'; undefinedVar = 1 })() } catch (e) { R = e.stack }"),
  lines("function f() { return null.x }", "try { f() } catch (e) { R = e.stack }"),
  // captureStackTrace e o limite.
  lines("function cap() { var o = {}; Error.captureStackTrace(o); return o.stack }", "R = cap()"),
  lines("function cap() { var o = { name: 'N', message: 'M' }; Error.captureStackTrace(o); return o.stack }", "R = cap()"),
  lines("function inner(o) { Error.captureStackTrace(o, inner) }", "function outer() { var o = {}; inner(o); return o.stack }", "R = outer()"),
  lines("function f() { return new Error('x').stack }", "function g() { var r = f(); return r }", "Error.stackTraceLimit = 1; R = g()"),
  lines("function f() { return new Error('x').stack }", "function g() { var r = f(); return r }", "Error.stackTraceLimit = 0; R = g()"),
  lines("function f() { return new Error('x').stack }", "function g() { var r = f(); return r }", "Error.stackTraceLimit = 2; R = g()"),
  lines("R = Error.stackTraceLimit"),
  // Chamada em posição de cauda (modo estrito): o frame de quem chama some, e `call`/`apply` também.
  lines("function f() { return new Error('x').stack }", "function g() { return f() }", "function h() { var r = g(); return r }", "R = h()"),
  lines("function f() { return new Error('x').stack }", "function k() { return Reflect.apply(f, null, []) }", "R = k()"),
  lines("function k() { return [2, 1].sort(function c(a, b) { R = new Error('x').stack; return a - b }) }", "k()"),
  lines("function f() { return new Error('x').stack }", "function k() { var r = f.apply(null, []); return r }", "R = k()"),
  // prepareStackTrace e CallSite.
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFunctionName() + ':' + c.getLineNumber() + ':' + c.getColumnNumber()).join('|')", "function f() { return new Error('x').stack }", "R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => String(c)).join('|')", "function f() { return new Error('x').stack }", "R = f()"),
  lines("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFileName() + ':' + c.isNative() + ':' + c.isEval() + ':' + c.isConstructor() + ':' + c.isToplevel()).join('|')", "function F() { this.s = new Error('x').stack }", "R = new F().s"),
];

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stack-golden-"));
const file = path.join(dir, "stack_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
// O bun carrega arquivo como código estrito (`R = 1` sem declaração é ReferenceError), então o programa
// leva a diretiva na primeira linha e grava em `globalThis.R`. O bun transpila o arquivo antes do JSC: o golden grava
// o texto canônico (`prepareProgram`) e o bun executa `executable`, de modo que as posições do stack saem do fonte original.
const rows = [];
for (const body of programs) {
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir });
  const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) throw new Error("sem resultado para: " + original + "\n" + run.stderr);
  const stack = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(stack) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stdout.write(emitFactoredLines("stack", rows));
fs.rmSync(dir, { recursive: true, force: true });
