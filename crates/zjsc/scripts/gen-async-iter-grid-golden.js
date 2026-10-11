// Gera tests/golden/async_iter_grid_bun.tsv: grade de async generators e for-await medida no bun 1.4.2.
// Cobre yield de promessa rejeitada e de thenable, next/return/throw enfileirados (concorrentes e em sequência) em
// async generators com try/catch/finally, yield* de async e de sync (AsyncFromSyncIterator) com promessas rejeitadas,
// thenables e iteradores com return ausente ou estranho, for await sobre iteráveis sync e async com break, throw,
// continue, return e break rotulado (fechamento do iterador), await e yield em parâmetros default (SyntaxError),
// async arrow com arguments e a palavra await no topo do script (sem módulo, o eval global não permite await).
// Mesmo modelo de gen-async-order-golden.js: cada programa roda por `vm.runInThisContext` num processo bun filho
// novo, depois do prelúdio ORDER_HARNESS, sem API de host. O golden é o JSON de R depois de esvaziar as microtarefas
// (setTimeout fora do programa), ou `error<TAB>name<TAB>message JSON` se a fonte lançou de forma síncrona.
// Os filhos rodam em paralelo (8 por vez) com timeout de 5 s cada. Programas já presentes em outros goldens de
// async/promise/await são descartados. Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-async-iter-grid-golden.js > tests/golden/async_iter_grid_bun.tsv
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { execFile } = require("child_process");

// Mesmo texto embutido em tests/async_iter_grid_bun_golden.rs.
const HARNESS = `globalThis.R = [];
globalThis.L = function (x) { R.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.ok = function (v) { L("v:" + JSON.stringify(v)); };
globalThis.bad = function (e) { L("e:" + (e && e.name) + (e && e.name === "Error" ? ":" + e.message : "")); };
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(R); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
};`;

const root = path.join(__dirname, "..");
const existing = new Set();
for (const program of knownPrograms("async_iter_grid_bun.tsv", (name) => /(async|promise|microtask|await)/.test(name) && name !== "async_iter_grid_bun.tsv")) existing.add(program);
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

// Auxiliares dentro do programa: S registra o valor, E registra o erro.
const PRE = "var S = r => L('r:' + JSON.stringify(r)), E = e => L('x:' + (e && e.name) + ':' + (e && e.message)); ";
const TICK = " tick(30, 't30');";

// Operações sobre o iterador `it`.
const OPS = {
  n: "it.next('n')",
  r: "it.return('R')",
  t: "it.throw(new Error('T'))",
  rp: "it.return(Promise.resolve('RP'))",
  rr: "it.return(Promise.reject(new Error('RR')))",
  tp: "it.throw(Promise.resolve('TP'))",
};
const opNames = Object.keys(OPS);
function sequences(maxLen) {
  let out = [];
  let level = [[]];
  for (let len = 1; len <= maxLen; len++) {
    const next = [];
    for (const prefix of level) for (const name of opNames) next.push([...prefix, name]);
    out = out.concat(next);
    level = next;
  }
  return out;
}
// Concorrente: todas as chamadas no mesmo turno síncrono (fila de pedidos). Sequencial: uma await por vez.
function concurrent(seq) {
  return seq.map(name => OPS[name] + ".then(S, E); ").join("");
}
function sequential(seq) {
  return "(async () => { " + seq.map(name => "try { S(await " + OPS[name] + "); } catch (e) { E(e); } ").join("") + "})(); ";
}
// Grade de uma fonte de generator `gen` (define g) contra todas as sequências até maxLen.
function gridOf(gen, maxLen) {
  for (const seq of sequences(maxLen)) {
    add(PRE + gen + " var it = g(); " + concurrent(seq) + TICK);
    add(PRE + gen + " var it = g(); " + sequential(seq) + TICK);
  }
}

// 1. Async generator com try/catch/finally e yield de cada tipo de valor.
const yielded = {
  value: "1",
  resolved: "Promise.resolve(2)",
  rejected: "Promise.reject(new Error('r'))",
  thenable: "thenable(3, 't')",
};
for (const [vn, v] of Object.entries(yielded)) {
  gridOf(
    "async function* g() { L('a'); try { var x = yield " + v + "; L('x:' + x); yield 'two'; } catch (e) { L('c:' + (e && e.message)); yield 'rec'; } finally { L('fin'); } L('end'); return 'ret'; }",
    3
  );
}
// 2. Sem try: o yield rejeitado ou o throw encerram o generator.
for (const v of Object.values(yielded)) {
  gridOf("async function* g() { L('a'); yield " + v + "; L('b'); yield 2; return 3; }", 2);
  gridOf("async function* g() { L('a'); var x = yield " + v + "; L('b:' + x); await null; L('c'); return x; }", 2);
  gridOf("async function* g() { try { yield " + v + "; } finally { await null; L('f'); yield 'fy'; L('f2'); } }", 2);
  gridOf("async function* g() { try { yield " + v + "; } finally { L('f'); return 'override'; } }", 2);
  gridOf("async function* g() { try { yield " + v + "; } finally { L('f'); throw new Error('fe'); } }", 2);
}
// return() / throw() em generator ainda não iniciado e já terminado, e dentro de try com await.
gridOf("async function* g() { L('never'); yield 1; }", 3);
gridOf("async function* g() { return Promise.resolve('p'); }", 3);
gridOf("async function* g() { return Promise.reject(new Error('pr')); }", 3);
gridOf("async function* g() { return thenable('tr', 'rt'); }", 3);
gridOf("async function* g() { throw new Error('start'); }", 2);
gridOf("async function* g() { var x = await Promise.reject(new Error('aw')); yield x; }", 2);

// 3. yield* delegando a iteráveis sync e async.
function mkSync(vals, ret) {
  return (
    "{ [Symbol.iterator]() { var i = 0, vals = " + vals + "; return { next() { L('n' + i); return i < vals.length ? { value: vals[i++], done: false } : { value: 'dv', done: true }; }" +
    (ret === null ? "" : ", return(v) { L('ret:' + v); return " + ret + "; }") +
    ", throw(e) { L('thr:' + (e && e.message)); return { value: 'tv', done: true }; } }; } }"
  );
}
function mkSyncNoThrow(vals, ret) {
  return (
    "{ [Symbol.iterator]() { var i = 0, vals = " + vals + "; return { next() { L('n' + i); return i < vals.length ? { value: vals[i++], done: false } : { value: 'dv', done: true }; }" +
    (ret === null ? "" : ", return(v) { L('ret:' + v); return " + ret + "; }") +
    " }; } }"
  );
}
function mkAsync(vals, ret) {
  return (
    "{ [Symbol.asyncIterator]() { var i = 0, vals = " + vals + "; return { next() { L('an' + i); return Promise.resolve(i < vals.length ? { value: vals[i++], done: false } : { value: 'dv', done: true }); }" +
    (ret === null ? "" : ", return(v) { L('aret:' + v); return " + ret + "; }") +
    ", throw(e) { L('athr:' + (e && e.message)); return Promise.resolve({ value: 'atv', done: true }); } }; } }"
  );
}
const VALS = {
  nums: "[0, 1]",
  rejMid: "[0, Promise.reject(new Error('r')), 2]",
  rejFirst: "[Promise.reject(new Error('r'))]",
  thens: "[thenable(0, 'a'), thenable(1, 'b')]",
  thenThrows: "[0, { then() { throw new Error('tt'); } }, 2]",
  mixed: "[Promise.resolve(0), 1, Promise.resolve(Promise.resolve(2))]",
};
const RETS = { none: null, obj: "{}", num: "1", resolved: "Promise.resolve({})", rejected: "Promise.reject(new Error('rr'))", doneVal: "{ value: 'rv', done: true }" };
const delegates = {};
for (const [vn, vals] of Object.entries(VALS)) {
  delegates["sync_" + vn + "_none"] = mkSync(vals, null);
  delegates["sync_" + vn + "_obj"] = mkSync(vals, "{}");
  delegates["sync_" + vn + "_rej"] = mkSync(vals, "Promise.reject(new Error('rr'))");
}
for (const [rn, ret] of Object.entries(RETS)) {
  delegates["syncR_" + rn] = mkSyncNoThrow(VALS.mixed, ret);
  delegates["syncRrej_" + rn] = mkSyncNoThrow(VALS.rejMid, ret);
}
delegates.async_nums = mkAsync(VALS.nums, "Promise.resolve({})");
delegates.async_noret = mkAsync(VALS.nums, null);
delegates.async_rejret = mkAsync(VALS.nums, "Promise.reject(new Error('arr'))");
delegates.async_numret = mkAsync(VALS.nums, "1");
delegates.array = "[1, Promise.resolve(2), Promise.reject(new Error('ar'))]";
delegates.string = "'ab'";
delegates.syncGen = "(function* () { try { var a = yield 1; L('a:' + a); yield Promise.reject(new Error('gr')); } finally { L('gfin'); } })()";
delegates.asyncGen = "(async function* () { try { var a = yield 1; L('a:' + a); yield Promise.reject(new Error('gr')); } finally { L('gfin'); } })()";
delegates.nonIterable = "5";
delegates.nullish = "null";
delegates.asyncIterNull = "{ [Symbol.asyncIterator]: null, [Symbol.iterator]() { return [7, 8][Symbol.iterator](); } }";
delegates.asyncIterUndef = "{ [Symbol.asyncIterator]: undefined, [Symbol.iterator]() { return [7, 8][Symbol.iterator](); } }";
delegates.asyncIterBad = "{ [Symbol.asyncIterator]: 5 }";
delegates.nextNotFn = "{ [Symbol.iterator]() { return { next: 5 }; } }";
delegates.nextNotObj = "{ [Symbol.iterator]() { return { next() { return 1; } }; } }";
delegates.doneGetter = "{ [Symbol.iterator]() { return { next() { return { get done() { L('dget'); return false; }, get value() { L('vget'); return 1; } }; } }; } }";
for (const [dn, d] of Object.entries(delegates)) {
  gridOf("async function* g() { L('s'); var r = yield* " + d + "; L('r:' + JSON.stringify(r)); yield 'after'; return 'end'; }", 2);
}
// yield* dentro de try/finally: o fechamento passa pelo bloco.
for (const dn of ["sync_nums_none", "sync_nums_obj", "sync_rejMid_rej", "syncR_rejected", "async_nums", "syncGen", "asyncGen"]) {
  gridOf("async function* g() { try { yield* " + delegates[dn] + "; } catch (e) { L('c:' + (e && e.message)); yield 'rec'; } finally { L('fin'); } }", 2);
}

// 4. for await sobre iteráveis sync e async, com cada corpo e cada forma de cabeçalho.
const iterables = { ...delegates };
delete iterables.nullish;
delete iterables.doneGetter; // iterador infinito: o for await nunca termina
const bodies = {
  plain: "L('b:' + x);",
  brk: "L('b:' + x); if (++k === 2) break;",
  brkFirst: "L('b:' + x); break;",
  thr: "L('b:' + x); if (++k === 2) throw new Error('bt');",
  cont: "if (++k === 1) continue; L('b:' + x);",
  ret: "L('b:' + x); if (++k === 2) return 'early';",
  awaiting: "await null; L('b:' + x); if (++k === 2) break;",
};
const heads = {
  const: "const x",
  let: "let x",
  var: "var x",
  bare: "x",
  arr: "const [x]",
  obj: "const { length: x }",
};
for (const [inn, it] of Object.entries(iterables)) {
  for (const [bn, body] of Object.entries(bodies)) {
    for (const [hn, head] of Object.entries(heads)) {
      if (hn !== "const" && hn !== "var" && !["plain", "brk"].includes(bn)) continue;
      const pre = hn === "bare" ? "var x; " : "";
      add(PRE + "var k = 0; " + pre + "(async () => { try { for await (" + head + " of " + it + ") { " + body + " } L('done'); return 'fn'; } catch (e) { E(e); } finally { L('fin'); } })().then(S, E);" + TICK);
    }
  }
  // break rotulado e continue rotulado atravessando for await aninhado.
  add(PRE + "(async () => { try { o: for (var z of [1, 2]) { for await (var x of " + it + ") { L('b:' + x + ':' + z); break o; } } L('done'); } catch (e) { E(e); } })();" + TICK);
  add(PRE + "(async () => { try { o: for (var z of [1, 2]) { for await (var x of " + it + ") { L('b:' + x + ':' + z); continue o; } } L('done'); } catch (e) { E(e); } })();" + TICK);
  // for await dentro de async generator, repassando cada valor com yield.
  add(PRE + "async function* g() { try { for await (var x of " + it + ") { yield x; } } finally { L('gfin'); } } var it = g(); (async () => { for (var i = 0; i < 3; i++) { try { S(await it.next()); } catch (e) { E(e); } } })();" + TICK);
  add(PRE + "async function* g() { for await (var x of " + it + ") { yield x; } } var it = g(); (async () => { try { S(await it.next()); S(await it.return('R')); S(await it.next()); } catch (e) { E(e); } })();" + TICK);
  // for await fora de async function.
  add(PRE + "function f() { for await (var x of " + it + ") ; }");
}
// Variantes soltas de cabeçalho e de for await.
add(
  PRE + "(async () => { for await (var x of []) L('never'); L('empty'); })();" + TICK,
  PRE + "(async () => { for await (var x of [1, 2], [3]) L('b:' + x); })();" + TICK,
  PRE + "(async () => { for await (let x of [Promise.resolve(1)]) { setX = () => x; L('b:' + x); } })();" + TICK,
  PRE + "(async () => { var fs = []; for await (let x of [1, 2, 3]) fs.push(() => x); L(fs.map(f => f()).join()); })();" + TICK,
  PRE + "(async () => { for await (var x in {a: 1}) ; })();",
  PRE + "(async () => { for await (var x = 1 of []) ; })();",
  PRE + "(async () => { for await (x of [1]) ; })().then(S, E);" + TICK,
  PRE + "(async () => { for await (const x of [1]) { x = 2; } })().then(S, E);" + TICK,
  PRE + "(async () => { for await (let let of [1]) ; })();",
  PRE + "(async () => { for await (async of [1]) ; })();",
  PRE + "(async () => { for await (var o = {} of []) ; })();",
  PRE + "var o = {}; (async () => { for await (o.p of [1, 2]) L('b:' + o.p); })();" + TICK,
  PRE + "var a = []; (async () => { for await (a[a.length] of [1, 2]) L('b:' + a.join()); })();" + TICK,
  PRE + "(async () => { label: for await (var x of [1]) { L('b'); continue label; } L('after'); })();" + TICK,
  PRE + "(async () => { for await (var x of [1]) function f() {} })();"
);

// 5. await e yield em parâmetros default e em outras posições onde são proibidos ou permitidos.
const defaults = ["await 1", "await", "(await 1)", "yield", "yield 1", "x => await 1", "async x => await 1", "async () => await 1", "function () { await 1 }", "async function () { await 1 }", "(await) => 1", "[await 1]", "{ a: await 1 }", "await x", "typeof await", "new.target"];
const wrappers = {
  asyncDecl: d => "async function f(a = " + d + ") {}",
  asyncExpr: d => "var f = async function (a = " + d + ") {};",
  asyncArrow: d => "var f = async (a = " + d + ") => 0;",
  asyncArrowDestr: d => "var f = async ({ a = " + d + " }) => 0;",
  asyncGenDecl: d => "async function* f(a = " + d + ") {}",
  asyncGenExpr: d => "var f = async function* (a = " + d + ") {};",
  genDecl: d => "function* f(a = " + d + ") {}",
  plainDecl: d => "function f(a = " + d + ") {}",
  arrowPlain: d => "var f = (a = " + d + ") => 0;",
  asyncMethod: d => "var o = { async m(a = " + d + ") {} };",
  asyncGenMethod: d => "var o = { async *m(a = " + d + ") {} };",
  classAsync: d => "class C { async m(a = " + d + ") {} }",
  classStaticAsyncGen: d => "class C { static async *m(a = " + d + ") {} }",
  insideAsyncBody: d => "async function f() { var a = " + d + "; }",
  insideAsyncGenBody: d => "async function* f() { var a = " + d + "; }",
  insideGenBody: d => "function* f() { var a = " + d + "; }",
  insidePlainBody: d => "function f() { var a = " + d + "; }",
  nestedPlain: d => "async function f() { function h() { var a = " + d + "; } }",
  nestedArrowInAsync: d => "async function f() { return () => " + d + "; }",
};
for (const [wn, w] of Object.entries(wrappers)) {
  for (const d of defaults) {
    // Sem try: o SyntaxError sai do eval. Com try/catch em volta de new Function e de eval indireto.
    add(w(d));
    add("try { (0, eval)(" + JSON.stringify(w(d)) + "); L('parsed'); } catch (e) { L(e.name); }");
    add("try { new Function(" + JSON.stringify(w(d)) + "); L('parsed'); } catch (e) { L(e.name); }");
  }
}
// await e yield como identificador em contextos sloppy e como nome de parâmetro.
const names = ["await", "yield"];
const nameForms = [
  n => "var " + n + " = 1; L(" + n + ");",
  n => "function f(" + n + ") { return " + n + "; } L(f(2));",
  n => "async function f(" + n + ") {}",
  n => "async function " + n + "() {} L(typeof " + n + ");",
  n => "async function* f() { var " + n + "; }",
  n => "function* f() { var " + n + "; }",
  n => "var f = async " + n + " => 0;",
  n => "var f = async (" + n + ") => 0;",
  n => "var f = " + n + " => " + n + "; L(f(3));",
  n => "var o = { " + n + ": 1, " + n + "() { return 2; } }; L(o." + n + "());",
  n => "label: { var q = 1; } " + n + ": for (;;) break " + n + "; L('ok');",
  n => "async function f() { return { " + n + ": 1 }." + n + "; } f().then(S, E);",
  n => "async function f() { class " + n + " {} }",
  n => "async function* f() { function " + n + "() {} }",
  n => "var f = async function " + n + "() {};",
  n => "var f = async function* " + n + "() {};",
  n => "var f = function " + n + "() {}; L(typeof f);",
  n => "var f = function* " + n + "() {};",
  n => "class C { " + n + "() { return 1; } static " + n + " = 2; } L(C." + n + ");",
  n => "var {" + n + "} = {" + n + ": 5}; L(" + n + ");",
  n => "async function f() { var { " + n + " } = {}; }",
  n => "async function f() { try {} catch (" + n + ") {} }",
  n => "async function f() { for (var " + n + " of []) ; }",
  n => "async function* f() { try {} catch (" + n + ") {} }",
  n => "async function f(a = () => " + n + ") {}",
  n => "async function* f(a = " + n + ") {}",
];
for (const n of names) for (const nf of nameForms) add(PRE + nf(n) + TICK, "try { (0, eval)(" + JSON.stringify(nf(n)) + "); L('parsed'); } catch (e) { L(e.name); }");
// Top-level await e for await no topo: o eval global é script, não módulo.
add(
  "await 1;",
  "await Promise.resolve(1);",
  "var r = await 1; L(r);",
  "L(typeof await);",
  "var await = 1; L(await);",
  "var await; L(await);",
  "await: for (;;) break await; L('ok');",
  "for await (var x of []) ;",
  "for await (var x of [1]) L(x);",
  "(async () => { await 1; })(); await 2;",
  "function await() { return 1; } L(await());",
  "var o = { await: 1 }; L(o.await);",
  "class C { await() { return 1; } } L(new C().await());",
  "const f = async () => await 1; f().then(S, E); await f();",
  "try { eval('await 1'); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Function('await 1'); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new Function('return await 1'); } catch (e) { L(e.name); }",
  "var AsyncFunction = (async () => {}).constructor; try { new AsyncFunction('return await 1')().then(ok, bad); } catch (e) { L(e.name); }",
  "var AsyncFunction = (async () => {}).constructor; try { new AsyncFunction('a = await 1', 'return a'); L('parsed'); } catch (e) { L(e.name); }",
  "var AsyncFunction = (async () => {}).constructor; try { new AsyncFunction('await', 'return 1'); L('parsed'); } catch (e) { L(e.name); }",
  "var AsyncGenerator = (async function* () {}).constructor; try { var g = new AsyncGenerator('yield await 1'); g().next().then(ok, bad); } catch (e) { L(e.name); }",
  "var AsyncGenerator = (async function* () {}).constructor; try { new AsyncGenerator('a = yield', ''); L('parsed'); } catch (e) { L(e.name); }"
);

// 6. async arrow com arguments.
const argUses = {
  length: "arguments.length",
  first: "arguments[0]",
  join: "Array.prototype.join.call(arguments)",
  typeof: "typeof arguments",
  tag: "Object.prototype.toString.call(arguments)",
  mutate: "(arguments[0] = 'm', arguments[0])",
  callee: "typeof arguments.callee",
  spread: "[...arguments].length",
};
for (const [un, u] of Object.entries(argUses)) {
  const wrapOuter = (inner, kind) => {
    if (kind === "fn") return "function outer(a) { return " + inner + "; } ";
    if (kind === "strict") return "function outer(a) { 'use strict'; return " + inner + "; } ";
    if (kind === "gen") return "function* outer(a) { yield " + inner + "; } ";
    if (kind === "asyncGen") return "async function* outer(a) { yield " + inner + "; } ";
    if (kind === "asyncFn") return "async function outer(a) { return " + inner + "; } ";
    if (kind === "method") return "var o = { m(a) { return " + inner + "; } }; var outer = o.m; ";
    if (kind === "classMethod") return "class C { m(a) { return " + inner + "; } } var outer = new C().m; ";
    if (kind === "arrowOuter") return "var outer = (a) => " + inner + "; ";
    if (kind === "dflt") return "function outer(a, b = " + inner + ") { return b; } ";
    return "";
  };
  const forms = {
    direct: "(async () => " + u + ")()",
    afterAwait: "(async () => { await null; return " + u + "; })()",
    nestedArrow: "(async () => (() => " + u + ")())()",
    defaultParam: "(async (p = " + u + ") => p)()",
    inBlockFn: "(async () => { var f = () => " + u + "; await null; return f(); })()",
    plainFnInside: "(async () => (function () { return " + u + "; })())()",
    asyncFnInside: "(async () => (async function () { return " + u + "; })())()",
  };
  for (const kind of ["fn", "strict", "gen", "asyncGen", "asyncFn", "method", "classMethod", "arrowOuter", "dflt"]) {
    for (const [fnm, form] of Object.entries(forms)) {
      const call = kind === "gen" || kind === "asyncGen" ? "outer(1, 2)" : "outer(1, 2)";
      const run =
        kind === "gen"
          ? "var r = outer(1, 2).next().value; Promise.resolve(r).then(S, E);"
          : kind === "asyncGen"
            ? "outer(1, 2).next().then(r => r.value).then(S, E);"
            : "Promise.resolve(" + call + ").then(S, E);";
      add(PRE + wrapOuter(form, kind) + run + TICK);
    }
  }
  add(
    PRE + "(async () => " + u + ")().then(S, E);" + TICK,
    PRE + "var f = async () => " + u + "; f(1, 2).then(S, E);" + TICK,
    PRE + "var f = async x => " + u + "; f(1, 2).then(S, E);" + TICK,
    PRE + "var f = async ({ a }, ...rest) => " + u + "; f({ a: 1 }, 2, 3).then(S, E);" + TICK,
    PRE + "var arguments = 'g'; (async () => " + u + ")().then(S, E);" + TICK,
    PRE + "function outer() { var arguments = 'v'; return (async () => " + u + ")(); } outer(1, 2).then(S, E);" + TICK,
    PRE + "function outer() { return (async () => " + u + ")(); } outer(1, 2).then(S, E);" + TICK,
    PRE + "function outer() { arguments = [9]; return (async () => " + u + ")(); } outer(1, 2).then(S, E);" + TICK,
    PRE + "function outer(a) { a = 5; return (async () => " + u + ")(); } outer(1, 2).then(S, E);" + TICK,
    PRE + "function outer(a) { var p = (async () => { await null; return " + u + "; })(); a = 7; return p; } outer(1, 2).then(S, E);" + TICK,
    PRE + "class C { f = async () => " + u + "; } new C().f().then(S, E);" + TICK,
    PRE + "class C { static f = async () => " + u + "; } C.f().then(S, E);" + TICK,
    PRE + "var o = { m() { return (async () => " + u + ")(); } }; o.m(1, 2).then(S, E);" + TICK,
    PRE + "var o = { async m() { return (async () => " + u + ")(); } }; o.m(1, 2).then(S, E);" + TICK,
    PRE + "var o = { async *m() { yield (async () => " + u + ")(); } }; o.m(1, 2).next().then(r => S(r.value), E);" + TICK
  );
}

// 7. Ordem de next() concorrentes entre dois iteradores e com awaits internos.
const slow = "async function* g(tag, n) { for (var i = 0; i < n; i++) { await null; L(tag + i); yield tag + i; } return tag + 'end'; } ";
for (const n of [1, 2, 3]) {
  add(PRE + slow + "var a = g('a', " + n + "), b = g('b', " + n + "); for (var i = 0; i <= " + n + "; i++) { a.next().then(S, E); b.next().then(S, E); }" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "); for (var i = 0; i <= " + n + " + 1; i++) a.next().then(S, E); a.return('R').then(S, E); a.next().then(S, E);" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "); a.next().then(S, E); a.throw(new Error('T')).then(S, E); a.next().then(S, E);" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "); a.return(thenable('rt', 'x')).then(S, E); a.next().then(S, E);" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "); a.next().then(S, E); a.return(Promise.reject(new Error('rr'))).then(S, E); a.next().then(S, E);" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "), b = g('b', " + n + "); Promise.all([a.next(), b.next(), a.next(), b.next()]).then(S, E);" + TICK);
  add(PRE + slow + "var a = g('a', " + n + "), b = g('b', " + n + "); Promise.race([a.next(), b.next()]).then(S, E); a.next().then(S, E);" + TICK);
  add(PRE + slow + "(async () => { var a = g('a', " + n + "); for await (var v of a) { L('v:' + v); a.next().then(S, E); } L('done'); })();" + TICK);
  add(PRE + slow + "(async () => { var a = g('a', " + n + "); for await (var v of a) { L('v:' + v); break; } S(await a.next()); })();" + TICK);
  add(PRE + slow + "(async () => { var a = g('a', " + n + "); for await (var v of a) { L('v:' + v); throw new Error('bt'); } })().catch(E);" + TICK);
}
// Protótipos e identidade dos objetos assíncronos.
const AG = "Object.getPrototypeOf(async function* () {})";
add(
  PRE + "var g = async function* () {}; var it = g(); L(String(Object.prototype.toString.call(it))); L(String(typeof it[Symbol.asyncIterator])); L(String(it[Symbol.asyncIterator]() === it)); L(String(typeof it[Symbol.iterator]));",
  PRE + "var p = " + AG + ".prototype; L(Object.getOwnPropertyNames(p).join()); L(String(p[Symbol.toStringTag]));",
  PRE + "var p = Object.getPrototypeOf(" + AG + ".prototype); L(Object.getOwnPropertyNames(p).join()); L(String(typeof p[Symbol.asyncIterator]));",
  PRE + "var next = " + AG + ".prototype.next; next.call({}).then(S, E); next.call(1).then(S, E); next.call(undefined).then(S, E); L('sync');" + TICK,
  PRE + "var ret = " + AG + ".prototype.return; ret.call({}, 1).then(S, E); ret.call(null).then(S, E); L('sync');" + TICK,
  PRE + "var thr = " + AG + ".prototype.throw; thr.call({}, 1).then(S, E); thr.call(null).then(S, E); L('sync');" + TICK,
  PRE + "var g = async function* () { yield 1; }; var it = g(); var next = " + AG + ".prototype.next; next.call(it).then(S, E); next.call(g()).then(S, E);" + TICK,
  PRE + "async function* g() { yield 1; } L(String(g.hasOwnProperty('prototype'))); L(String(Object.getPrototypeOf(g.prototype) === " + AG + ".prototype)); L(String(g.name)); L(String(g.length));",
  PRE + "async function* g() {} try { new g(); } catch (e) { L(e.name); }",
  PRE + "var o = { async *m() {} }; try { new o.m(); } catch (e) { L(e.name); } L(String(o.m.hasOwnProperty('prototype')));",
  PRE + "async function* g() { yield 1; } g.prototype = null; var it = g(); L(String(Object.getPrototypeOf(it) === " + AG + ".prototype)); it.next().then(S, E);" + TICK,
  PRE + "async function* g() { yield 1; } g.prototype = 5; var it = g(); L(String(Object.getPrototypeOf(it) === " + AG + ".prototype));",
  PRE + "async function* g() { yield this; } g.call(5).next().then(r => L(typeof r.value), E);" + TICK,
  PRE + "async function* g() { yield new.target; } g().next().then(S, E);" + TICK,
  PRE + "async function* g() { yield arguments.length; } g(1, 2, 3).next().then(S, E);" + TICK,
  PRE + "async function* g() { var r = yield; L('r:' + r); } var it = g(); it.next('ignored').then(S, E); it.next('seen').then(S, E);" + TICK,
  PRE + "async function* g() { yield* []; return 'r'; } g().next().then(S, E);" + TICK,
  PRE + "async function* g() { yield await Promise.resolve(1); yield yield 2; } var it = g(); it.next().then(S, E); it.next('x').then(S, E); it.next('y').then(S, E); it.next('z').then(S, E);" + TICK,
  PRE + "async function* g() { yield* g2(); } async function* g2() { yield 1; return 2; } var it = g(); it.next().then(S, E); it.next().then(S, E);" + TICK,
  PRE + "var it = (async function* () { yield 1; })(); it.next().then(S, E); L(String(it.next() instanceof Promise));" + TICK,
  PRE + "var it = (async function* () { yield 1; })(); var p = it.next(); L(String(p.constructor === Promise));" + TICK,
  PRE + "var it = (async function* () { var r = yield 1; return r; })(); it.next(); it.next(Promise.resolve('pr')).then(S, E);" + TICK,
  PRE + "var it = (async function* () { try { yield 1; } finally { L('fin'); } })(); it.return().then(S, E); L('s');" + TICK,
  PRE + "var it = (async function* () { try { yield 1; } finally { L('fin'); } })(); it.next(); it.return().then(S, E); L('s');" + TICK,
  PRE + "var it = (async function* () { try { yield 1; } finally { L('fin'); } })(); it.next(); it.return(Promise.reject(new Error('x'))).then(S, E); L('s');" + TICK,
  PRE + "var it = (async function* () { try { yield 1; } catch (e) { L('c'); } })(); it.next(); it.return(Promise.reject(new Error('x'))).then(S, E); L('s');" + TICK
);

// Alvo mínimo.
if (programs.length < 2500) throw new Error("só " + programs.length + " programas");

// Amostra determinística por hash (sampleByHash) do conjunto candidato inteiro, para o golden rodar em tempo razoável;
// só depois saem os que os goldens vizinhos já têm.
const TARGET = 3000;
programs.splice(0, programs.length, ...sampleByHash(programs.splice(0), TARGET).filter((source) => !existing.has(source)));

// Executa cada programa no bun, num processo próprio, em paralelo, pela API vm.
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "async-iter-grid-golden-"));
const driver = path.join(tmp, "driver.js");
fs.writeFileSync(
  driver,
  `const vm = require("node:vm");
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.on("unhandledRejection", () => {});
vm.runInThisContext(fs.readFileSync(process.argv[3], "utf8"), { filename: "harness" });
try { vm.runInThisContext(source, { filename: "program" }); } catch (e) { globalThis.__err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
setTimeout(() => { const out = globalThis.__final(); process.stdout.write(out); }, 0);
`
);
const harnessFile = path.join(tmp, "harness.txt");
fs.writeFileSync(harnessFile, HARNESS);

function runOne(source, index) {
  const srcFile = path.join(tmp, `p${index}.txt`);
  fs.writeFileSync(srcFile, source);
  return new Promise(resolve => {
    execFile(process.execPath, [driver, srcFile, harnessFile], { timeout: 5000, encoding: "utf8", cwd: tmp }, (error, stdout, stderr) => {
      let result = stdout;
      if (error || result === "") {
        process.stderr.write(`FALHA: ${source}\n${stderr}\n`);
        result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
      }
      resolve(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
    });
  });
}

async function main() {
  const lines = new Array(programs.length);
  let next = 0;
  const worker = async () => {
    while (next < programs.length) {
      const index = next++;
      lines[index] = await runOne(programs[index], index);
    }
  };
  await Promise.all(Array.from({ length: 16 }, worker));
  fs.rmSync(tmp, { recursive: true, force: true });
  const output = lines.join("\n") + "\n";
  if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
  if (/error\tHarness/.test(output)) throw new Error("programa sem resultado do bun (timeout ou falha)");
  if (output.includes(String.fromCharCode(0x2014)) || output.includes(String.fromCharCode(0x2013))) throw new Error("travessão no golden");
  process.stdout.write(output);
  process.stderr.write(`${programs.length} programas\n`);
}
main();
