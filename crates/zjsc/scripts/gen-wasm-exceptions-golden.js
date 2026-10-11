// Gera tests/golden/wasm_exceptions_bun.tsv: programas de exceções do WebAssembly (seção Tag id 13,
// throw, try/catch/catch_all/rethrow/delegate, try_table, tag importada de JS, exceção de JS atravessando
// o wasm), avaliados no bun. Cada programa registra eventos no array global `log` e usa os auxiliares de
// tests/golden/wasm_js_bun_harness.js e de tests/golden/wasm_exceptions_bun_extra.js (EXM, RUN, X, C e os
// módulos EHA, EHI, EHN, EHT), o mesmo texto que tests/wasm_exceptions_bun_golden.rs embute. Colunas: fonte,
// depois o JSON do log, ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona.
// Cada programa roda num processo bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-exceptions-golden.js > tests/golden/wasm_exceptions_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness =
  fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_js_bun_harness.js"), "utf8") +
  "\n" +
  fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_exceptions_bun_extra.js"), "utf8");
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

const A = "var x = RUN(EHA); ";
const I = "var tag = new WebAssembly.Tag({ parameters: ['i32'] }); ";

// throw simples chegando ao JS.
add(
  A + "try { x.thr0(5) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }",
  A + "try { x.thr0(5) } catch (e) { L(e instanceof Error); L(typeof e); L(Object.prototype.toString.call(e)); L(e.constructor === WebAssembly.Exception) }",
  A + "try { x.thr0(-7) } catch (e) { L(e.getArg(x.t0, 0)) }",
  A + "try { x.thr0(2147483647) } catch (e) { L(e.getArg(x.t0, 0)) }",
  A + "try { x.thr0(5) } catch (e) { L(e.is(x.t1)); L(e.is(x.t2)); L(e.is(new WebAssembly.Tag({ parameters: ['i32'] }))) }",
  A + "try { x.thr0(5) } catch (e) { L(T(() => e.getArg(x.t1, 0))); L(T(() => e.getArg(x.t0, 1))); L(T(() => e.getArg(x.t0, -1))) }",
  A + "try { x.thr0(5) } catch (e) { L(e.stack); L(String(e)); L(Object.keys(e).length) }",
  A + "try { x.thr1() } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t1)); L(T(() => e.getArg(x.t1, 0))) }",
  A + "try { x.thr2(3, 1.5) } catch (e) { L(e.is(x.t2)); L(e.getArg(x.t2, 0)); L(e.getArg(x.t2, 1)); L(T(() => e.getArg(x.t2, 2))) }",
  A + "try { x.thr2(3, 0.1) } catch (e) { L(e.getArg(x.t2, 1)) }",
  A + "L(x.t0 instanceof WebAssembly.Tag); L(x.t0 === x.t0); L(x.t0 === x.t1); L(Object.prototype.toString.call(x.t0)); L(Object.keys(x).join())",
  A + "var a = null, b = null; try { x.thr0(1) } catch (e) { a = e } try { x.thr0(1) } catch (e) { b = e } L(a === b); L(a.is(x.t0) && b.is(x.t0))",
  A + "var e1 = C(() => x.thr0(1)); L(e1); L(C(() => x.thr1())); L(C(() => x.thr2(1, 2)))",
  "var y = RUN(EHN); try { y.thr64(5n) } catch (e) { L(e.is(y.t64)); L(typeof e.getArg(y.t64, 0)); L(String(e.getArg(y.t64, 0))) }",
  "var y = RUN(EHN); try { y.thr64(-5n) } catch (e) { L(String(e.getArg(y.t64, 0))) }",
  "var y = RUN(EHN); try { y.thr64(2n ** 63n - 1n) } catch (e) { L(String(e.getArg(y.t64, 0))) }",
  "var y = RUN(EHN); try { y.thr32(1.5) } catch (e) { L(e.is(y.t32)); L(e.getArg(y.t32, 0)) }",
  "var y = RUN(EHN); try { y.thr32(0.1) } catch (e) { L(e.getArg(y.t32, 0)) }",
  "var y = RUN(EHN); L(C(() => y.thr64(1))); L(C(() => y.thr32('x')))",
  "var y = RUN(EHN); L(y.t64 instanceof WebAssembly.Tag); L(y.t64 === y.t32)"
);

// catch por tag, catch_all, rethrow, delegate, aninhamento.
add(
  A + "L(x.catch0(5)); L(x.catch0(-1)); L(x.catch0(2147483647))",
  A + "L(x.catchAll(5)); L(x.catchAll(0))",
  A + "L(C(() => x.onlyTag1(5)))",
  A + "try { x.onlyTag1(5) } catch (e) { L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }",
  A + "L(x.call1())",
  A + "L(C(() => x.rethrow(8)))",
  A + "try { x.rethrow(8) } catch (e) { L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }",
  A + "L(x.delegate(5)); L(x.delegate(0)); L(x.delegate(-50))",
  A + "L(x.nested(5)); L(x.nested(0))",
  A + "L(C(() => x.catchThrow(5)))",
  A + "try { x.catchThrow(5) } catch (e) { L(e.is(x.t1)); L(e.is(x.t0)) }",
  A + "L(x.catch2(1, 2.5))",
  A + "L(C(() => x.through(4)))",
  A + "try { x.through(4) } catch (e) { L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }",
  A + "L(C(() => x.trapCatch()))",
  A + "try { x.trapCatch() } catch (e) { L(e instanceof WebAssembly.RuntimeError); L(e instanceof WebAssembly.Exception); L(E(e)) }",
  A + "L(x.catch0.length); L(x.catchAll.name); L(x.thr2.length)",
  A + "var total = 0; for (var i = 0; i < 20; i++) total += x.catch0(i); L(total)",
  A + "var n = 0; for (var i = 0; i < 20; i++) { try { x.thr0(i) } catch (e) { n += e.getArg(x.t0, 0) } } L(n)",
  A + "var y = RUN(EHA); L(x.catch0(3)); L(y.catch0(4)); try { x.thr0(1) } catch (e) { L(e.is(y.t0)) }"
);

// Tag importada de JS (WebAssembly.Tag) e exceção de JS atravessando o wasm.
add(
  I + "var f = v => { throw new WebAssembly.Exception(tag, [v * 2]) }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(5)); L(x.catchAllImp(5))",
  I + "var f = v => {}; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(5)); L(x.catchAllImp(5)); L(x.rethrowAll(5))",
  I + "var err = new RangeError('boom'); var f = v => { throw err }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.catchImp(5) } catch (e) { L(e === err) }",
  I + "var err = new RangeError('boom'); var f = v => { throw err }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchAllImp(5))",
  I + "var err = new RangeError('boom'); var f = v => { throw err }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(5) } catch (e) { L(e === err); L(E(e)) }",
  I + "var f = v => { throw 42 }; var x = RUN(EHI, { m: { t: tag, f } }); L(C(() => x.catchImp(1))); L(x.catchAllImp(1)); L(C(() => x.rethrowAll(1)))",
  I + "var obj = { a: 1 }; var f = v => { throw obj }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(1) } catch (e) { L(e === obj) }",
  I + "var f = v => { throw undefined }; var x = RUN(EHI, { m: { t: tag, f } }); L(C(() => x.catchImp(1))); L(x.catchAllImp(1))",
  I + "var other = new WebAssembly.Tag({ parameters: ['i32'] }); var f = v => { throw new WebAssembly.Exception(other, [v]) }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchAllImp(1)); try { x.catchImp(1) } catch (e) { L(e.is(other)); L(e.is(tag)); L(e.getArg(other, 0)) }",
  I + "var f = v => {}; var x = RUN(EHI, { m: { t: tag, f } }); L(x.t === tag); L(x.t instanceof WebAssembly.Tag)",
  I + "var f = v => {}; var x = RUN(EHI, { m: { t: tag, f } }); try { x.thr(9) } catch (e) { L(e.is(tag)); L(e.getArg(tag, 0)); L(e instanceof WebAssembly.Exception) }",
  I + "var f = v => {}; var x = RUN(EHI, { m: { t: tag, f } }); var thrown = null; try { x.thr(9) } catch (e) { thrown = e } L(thrown.is(x.t)); L(thrown.getArg(x.t, 0))",
  I + "var f = v => {}; L(T(() => RUN(EHI, { m: { t: new WebAssembly.Tag({ parameters: ['f64'] }), f } })))",
  I + "var f = v => {}; L(T(() => RUN(EHI, { m: { t: new WebAssembly.Tag({ parameters: [] }), f } })))",
  I + "var f = v => {}; L(T(() => RUN(EHI, { m: { t: {}, f } })))",
  I + "var f = v => {}; L(T(() => RUN(EHI, { m: { t: 1, f } })))",
  I + "var f = v => {}; L(T(() => RUN(EHI, { m: { f } })))",
  I + "var a = RUN(EHA); var f = v => a.thr0(v); var x = RUN(EHI, { m: { t: a.t0, f } }); L(x.catchImp(7)); L(x.catchAllImp(7))",
  I + "var a = RUN(EHA); var x = RUN(EHI, { m: { t: a.t0, f: a.thr0 } }); L(x.catchImp(7)); L(x.catchAllImp(7)); L(x.t === a.t0)",
  I + "var a = RUN(EHA); var x = RUN(EHI, { m: { t: tag, f: a.thr0 } }); L(C(() => x.catchImp(7))); L(x.catchAllImp(7))",
  I + "var a = RUN(EHA); var x = RUN(EHI, { m: { t: a.t0, f: a.thr0 } }); try { x.rethrowAll(7) } catch (e) { L(e.is(a.t0)); L(e.getArg(a.t0, 0)) }",
  I + "var f = v => { throw new WebAssembly.Exception(tag, [v]) }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(3) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(tag)); L(e.getArg(tag, 0)) }",
  I + "var f = v => { throw new TypeError('t') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.catchImp(1) } catch (e) { L(e instanceof TypeError); L(E(e)) }",
  I + "var f = v => { throw new WebAssembly.Exception(tag, [v]) }; var x = RUN(EHI, { m: { t: tag, f } }); var s = 0; for (var i = 0; i < 10; i++) s += x.catchImp(i); L(s)",
  I + "var f = v => { if (v % 2) throw new Error('odd'); }; var x = RUN(EHI, { m: { t: tag, f } }); var odd = 0; for (var i = 0; i < 10; i++) { try { x.catchImp(i) } catch (e) { odd++ } } L(odd)"
);

// Exceções de JS criadas sem wasm e o protocolo de WebAssembly.Exception.
add(
  I + "var e = new WebAssembly.Exception(tag, [7]); L(e.is(tag)); L(e.getArg(tag, 0)); L(e instanceof Error)",
  I + "var e = new WebAssembly.Exception(tag, [7]); var f = v => { throw e }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(0)); L(x.catchAllImp(0))",
  I + "var e = new WebAssembly.Exception(tag, [7]); var f = v => { throw e }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(0) } catch (r) { L(r.is(tag)); L(r.getArg(tag, 0)) }",
  I + "var e = new WebAssembly.Exception(tag, [7.9]); var f = v => { throw e }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(0))",
  I + "var e = new WebAssembly.Exception(tag, ['12']); var f = v => { throw e }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(0))",
  I + "var f = v => { throw new WebAssembly.Exception(tag, [v]) }; var x = RUN(EHI, { m: { t: tag, f } }); L(x.catchImp(2 ** 31)); L(x.catchImp(-1))"
);

// try_table (EH novo): o bun pode ou não aceitar; o resultado de cada programa é o do bun.
add(
  "var x = RUN(EHT); L(x.catchTT(5)); L(x.catchTT(-3))",
  "var x = RUN(EHT); L(x.catchAllTT(5))",
  "var x = RUN(EHT); L(C(() => x.throwRef(5)))",
  "var x = RUN(EHT); try { x.throwRef(5) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }",
  "var x = RUN(EHT); try { x.thr0(5) } catch (e) { L(e.is(x.t0)); L(e.getArg(x.t0, 0)) }"
);

// Exceção de JS atravessando chamada wasm (JS -> wasm -> JS import que lança -> wasm -> JS): identidade do valor,
// frames do `stack` (normalizados: sem caminho, linha e coluna), JSTag e instanceof.
const S = "var S = e => e.stack.split('\\n').slice(0, e.stack.split('\\n').findIndex(l => /^\\s*at eval\\b/.test(l))).map(l => l.replace(/\\s*\\(.*$/, '').replace(/ \\/\\S+$/, ' <file>')).join('|'); ";
const J =
  "var EHJ = EXM({ types: [[[0x6f], []], [[0x7f], [0x7f]], [[0x7f], []], [[0x7f], [0x6f]]], imports: [['m', 't', [4, 0, 0]], ['m', 'f', [0, 2]]], funcs: [1, 3, 1], " +
  "exports: [['catchJ', 0, 1], ['valJ', 0, 2], ['plain', 0, 3]], codes: [" +
  "[0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 0, 0x1a, 0x41, 7, 0x0b], " +
  "[0x06, 0x6f, 0x20, 0, 0x10, 0, 0xd0, 0x6f, 0x07, 0, 0x0b], " +
  "[0x20, 0, 0x10, 0, 0x20, 0]] }); ";
add(
  I + S + "var f = v => { throw new Error('boom') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(1) } catch (e) { L(S(e)) }",
  I + S + "var f = v => { throw new Error('boom') }; var x = RUN(EHI, { m: { t: tag, f } }); function g() { return x.rethrowAll(1) } try { g() } catch (e) { L(S(e)) }",
  I + S + "var f = v => { throw new Error('boom') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.thr0 } catch (e) {} try { x.catchImp(1) } catch (e) { L(S(e)); L(e instanceof WebAssembly.Exception) }",
  I + "var f = v => { throw new Error('boom') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.catchImp(1) } catch (e) { L(/wasm-function/.test(e.stack)); L(e.stack.split('\\n').length) }",
  I + "var f = v => { throw new Error('boom') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.catchImp(1) } catch (e) { L(Object.getOwnPropertyNames(e).join()); L(e.message); L(e.name) }",
  I + "var err = new Error('boom'); var f = v => { throw err }; var x = RUN(EHI, { m: { t: tag, f } }); var seen = []; for (var i = 0; i < 3; i++) { try { x.catchImp(i) } catch (e) { seen.push(e === err) } } L(seen.join()); L(err.stack === err.stack)",
  I + "var err = new Error('boom'); var before = err.stack; var f = v => { throw err }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.catchImp(1) } catch (e) { L(e.stack === before) }",
  I + "var f = v => { throw Symbol.for('s') }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(1) } catch (e) { L(e === Symbol.for('s')) }",
  I + "var f = v => { throw 10n }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(1) } catch (e) { L(typeof e); L(String(e)) }",
  I + "var f = v => { throw null }; var x = RUN(EHI, { m: { t: tag, f } }); try { x.rethrowAll(1) } catch (e) { L(e === null) } L(x.catchAllImp(1))",
  I + "var o = {}; var f = v => { throw o }; var x = RUN(EHI, { m: { t: tag, f } }); var g = v => x.rethrowAll(v); var y = RUN(EHI, { m: { t: tag, f: g } }); try { y.rethrowAll(1) } catch (e) { L(e === o) }",
  I + "var o = {}; var f = v => { throw o }; var x = RUN(EHI, { m: { t: tag, f } }); var g = v => { try { x.rethrowAll(v) } catch (e) { throw [e] } }; var y = RUN(EHI, { m: { t: tag, f: g } }); try { y.rethrowAll(1) } catch (e) { L(Array.isArray(e)); L(e[0] === o) }",
  I + S + "var f = v => { throw new Error('inner') }; var x = RUN(EHI, { m: { t: tag, f } }); var g = v => x.rethrowAll(v); var y = RUN(EHI, { m: { t: tag, f: g } }); try { y.rethrowAll(1) } catch (e) { L(S(e)) }",
  "L(typeof WebAssembly.JSTag); L(WebAssembly.JSTag instanceof WebAssembly.Tag); L(Object.prototype.toString.call(WebAssembly.JSTag))",
  I + "var a = RUN(EHA); try { a.thr0(1) } catch (e) { L(e.is(WebAssembly.JSTag)) } L(Object.getOwnPropertyDescriptor(WebAssembly, 'JSTag') && Object.keys(Object.getOwnPropertyDescriptor(WebAssembly, 'JSTag')).join())",
  J + "var f = v => { throw new RangeError('r') }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.catchJ(1))",
  J + "var f = v => { throw 42 }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.catchJ(1)); L(x.valJ(1))",
  J + "var o = {}; var f = v => { throw o }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.valJ(1) === o)",
  J + "var err = new TypeError('t'); var f = v => { throw err }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.valJ(1) === err)",
  J + "var f = v => { throw undefined }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.valJ(1))",
  J + "var f = v => {}; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(x.catchJ(1)); L(x.plain(5))",
  I + J + "var f = v => { throw new WebAssembly.Exception(tag, [1]) }; var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f } }); L(C(() => x.catchJ(1))); L(C(() => x.valJ(1)))",
  I + J + "L(T(() => RUN(EHJ, { m: { t: tag, f: v => {} } })))",
  J + "L(T(() => RUN(EHI, { m: { t: WebAssembly.JSTag, f: v => {} } })))",
  J + "var a = RUN(EHA); var x = RUN(EHJ, { m: { t: WebAssembly.JSTag, f: a.thr0 } }); L(C(() => x.catchJ(1))); L(C(() => x.valJ(1)))"
);

// Validação de módulos com exceções.
add(
  "L(WebAssembly.validate(EHA)); L(WebAssembly.validate(EHI)); L(WebAssembly.validate(EHN)); L(WebAssembly.validate(EHT))",
  "L(JSON.stringify(WebAssembly.Module.exports(new WebAssembly.Module(EHN))))",
  "L(JSON.stringify(WebAssembly.Module.imports(new WebAssembly.Module(EHI))))"
);

if (programs.length < 60) throw new Error("só " + programs.length + " programas");

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-exceptions-golden-"));
const lines = [];
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 50);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
