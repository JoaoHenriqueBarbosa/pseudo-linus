// Gera tests/golden/async_gen_bun.tsv: async functions, async generators, generators e for await (return, throw,
// yield*, ordem de microtarefas, await em finally, iteradores async de iteráveis sync, erros em next e return,
// reentrância "already running"), medidos no bun 1.4.2.
// Cada programa roda por `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do
// bun não tocar na fonte) num processo próprio, depois do prelúdio ASYNC_GEN_HARNESS, também via vm. Os programas
// não usam API de host (setTimeout, process, console, require, Bun, URL, Buffer): só L, tick, thenable, ok e bad.
// O golden é o JSON do log depois de esvaziar as microtarefas, ou `error<TAB>name<TAB>message JSON` se a fonte
// lançou de forma síncrona. O host do gerador drena as microtarefas com um setTimeout fora do programa.
// Programas repetidos de outros goldens de async são descartados. Caminho da máquina no resultado derruba a geração.
// Uso: bun scripts/gen-async-gen-golden.js > tests/golden/async_gen_bun.tsv
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Mesmo texto embutido em tests/async_gen_bun_golden.rs.
const HARNESS = `globalThis.log = [];
globalThis.L = function (x) { log.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.ok = function (v) { L("v:" + JSON.stringify(v)); };
globalThis.bad = function (e) { L("e:" + (e && e.name) + ":" + (e && e.message)); };
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(log); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\\t" + e.name + "\\t" + JSON.stringify(String(e.message)); }
};`;

const root = path.join(__dirname, "..");
const existing = new Set();
for (const program of knownPrograms("async_gen_bun.tsv", ["promise_bun.tsv", "async_bun.tsv", "microtask_bun.tsv"])) existing.add(program);
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
const SEE = "ok, bad";

// 1. Generator síncrono: estado (início, suspenso, completo) x método x corpo.
const syncBodies = {
  plain: "function* g() { L('a'); yield 1; L('b'); yield 2; L('c'); return 3; }",
  tryFinally: "function* g() { try { L('a'); yield 1; L('b'); } finally { L('fin'); } }",
  finallyYield: "function* g() { try { yield 1; } finally { yield 'f'; L('after'); } }",
  tryCatch: "function* g() { try { yield 1; } catch (e) { L('c:' + e); yield 'recovered'; } return 'end'; }",
  finallyReturn: "function* g() { try { yield 1; } finally { return 'over'; } }",
  finallyThrow: "function* g() { try { yield 1; } finally { throw new Error('ft'); } }",
  nested: "function* g() { try { try { yield 1; } finally { L('in'); } } finally { L('out'); } }",
};
const states = { start: "", mid: "g1.next();", done: "g1.next(); g1.next(); g1.next(); g1.next();" };
const resume = { next: "g1.next('n')", ret: "g1.return('r')", thr: "g1.throw(new Error('t'))" };
for (const [bn, body] of Object.entries(syncBodies)) {
  for (const [sn, prep] of Object.entries(states)) {
    for (const [rn, call] of Object.entries(resume)) {
      add(`${body} var g1 = g(); try { ${prep} L(JSON.stringify(${call})); L(JSON.stringify(g1.next())); } catch (e) { L('c:' + e.message); }`);
    }
  }
}

// 2. Reentrância: "already running".
add(
  `var g1; function* g() { try { g1.next(); } catch (e) { L(e.name + ':' + e.message); } yield 1; } g1 = g(); L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { try { g1.return(1); } catch (e) { L(e.name); } yield 1; } g1 = g(); L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { try { g1.throw(new Error('x')); } catch (e) { L(e.name + ':' + e.message); } yield 1; } g1 = g(); L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { yield g1.next(); } g1 = g(); try { g1.next(); } catch (e) { L(e.name); } L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { yield* g1; } g1 = g(); try { g1.next(); } catch (e) { L(e.name + ':' + e.message); } L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { for (var x of g1) L('x'); } g1 = g(); try { g1.next(); } catch (e) { L(e.name); } L(JSON.stringify(g1.next()));`,
  `var g1; function* g() { try { yield 1; } finally { try { g1.next(); } catch (e) { L('fin:' + e.name); } } } g1 = g(); g1.next(); L(JSON.stringify(g1.return(5)));`,
  `var g1; function* g() { try { yield 1; } finally { try { g1.return(); } catch (e) { L('fin:' + e.name); } } } g1 = g(); g1.next(); L(JSON.stringify(g1.return(5)));`,
  `var g1; function* g() { yield 1; } g1 = g(); var it = { [Symbol.iterator]() { return { next() { return g1.next(); } }; } }; for (var x of it) { try { g1.next(); } catch (e) { L(e.name); } L('x' + x); }`,
  `var o = { *g() { try { o.it.next(); } catch (e) { L(e.name); } } }; o.it = o.g(); o.it.next(); L('after');`,
  `var g1; function* g() { var r = yield 1; try { g1.next(); } catch (e) { L(e.constructor === TypeError); } return r; } g1 = g(); g1.next(); L(JSON.stringify(g1.next('q')));`,
  `var g1; function* g() { try { yield 1; } catch (e) { try { g1.throw(1); } catch (e2) { L(e2.name); } } } g1 = g(); g1.next(); L(JSON.stringify(g1.throw(new Error('s'))));`,
  `var g1; async function* g() { var p = g1.next(); L('queued'); yield 1; L('resumed'); } g1 = g(); g1.next().then(${SEE}); tick(5, 't5');`,
  `var g1; async function* g() { try { g1.next().then(${SEE}); } catch (e) { L('sync:' + e.name); } yield 'a'; yield 'b'; } g1 = g(); g1.next().then(${SEE}); tick(8, 't8');`,
  `var g1; async function* g() { g1.return('rr').then(${SEE}); yield 1; L('not'); } g1 = g(); g1.next().then(${SEE}); tick(8, 't8');`,
  `var g1; async function* g() { yield* g1; } g1 = g(); g1.next().then(${SEE}); tick(8, 't8');`,
  `var g1; async function* g() { for await (var x of g1) L('x'); } g1 = g(); g1.next().then(${SEE}, ${SEE.split(",")[1]}); tick(8, 't8');`
);
// Chamar gerador com receptor errado.
for (const kind of ["function*", "async function*"]) {
  const proto = kind === "function*" ? "Object.getPrototypeOf(function* () {}).prototype" : "Object.getPrototypeOf(async function* () {}).prototype";
  for (const m of ["next", "return", "throw"]) {
    const call = kind === "function*" ? `P.${m}.call(${"{}"})` : `P.${m}.call({}).then(${SEE})`;
    add(`var P = ${proto}; try { ${call}; } catch (e) { L(e.name); } tick(3, 't3');`);
    add(`var P = ${proto}; try { P.${m}.call(1); } catch (e) { L(e.name); }`);
    if (kind === "async function*") add(`var P = ${proto}; P.${m}.call(undefined).then(${SEE}); tick(3, 't3');`);
  }
}

// 3. Fila de requests do async generator: sequências de chamadas, em início e meio.
const aBodies = {
  yields: "async function* g() { try { var a = yield 1; L('a=' + a); var b = yield 2; L('b=' + b); } finally { L('fin'); } return 9; }",
  finAwait: "async function* g() { try { yield 1; yield 2; } finally { await null; L('fin'); } }",
  catcher: "async function* g() { try { yield 1; yield 2; } catch (e) { L('c:' + e.message); yield 'rec'; } }",
};
const reqs = { n: "g1.next('x')", r: "g1.return('R')", t: "g1.throw(new Error('T'))", rp: "g1.return(Promise.resolve('RP'))", rr: "g1.return(Promise.reject(new Error('RR')))" };
for (const [bn, body] of Object.entries(aBodies)) {
  for (const prime of ["", "g1.next();"]) {
    for (const a of Object.keys(reqs)) {
      for (const b of Object.keys(reqs)) {
        add(`${body} var g1 = g(); ${prime} ${reqs[a]}.then(${SEE}); ${reqs[b]}.then(${SEE}); g1.next().then(${SEE}); tick(12, 't12');`);
      }
    }
  }
}

// 4. async generator: yield, return e await com cada tipo de operando.
const operands = {
  value: "1", native: "Promise.resolve(1)", thenable: "thenable(1, 'th')", rejected: "Promise.reject(new Error('r'))",
  thenableRej: "{ then(_, rej) { rej(new Error('tr')); } }", asyncFn: "(async () => 'af')()", nested: "Promise.resolve(Promise.resolve(1))",
};
for (const [on, x] of Object.entries(operands)) {
  add(
    `var g1 = (async function* () { yield ${x}; })(); g1.next().then(${SEE}); tick(7, 't7');`,
    `var g1 = (async function* () { return ${x}; })(); g1.next().then(${SEE}); tick(7, 't7');`,
    `var g1 = (async function* () { try { yield 1; } finally { return ${x}; } })(); g1.next(); g1.return('r').then(${SEE}); tick(9, 't9');`,
    `var g1 = (async function* () { yield 1; })(); g1.next(); g1.return(${x}).then(${SEE}); tick(9, 't9');`,
    `var g1 = (async function* () { var r = yield 1; L('r=' + r); })(); g1.next(); g1.next(${x}).then(${SEE}); tick(9, 't9');`,
    `var g1 = (async function* () { yield 1; })(); g1.return(${x}).then(${SEE}); g1.next().then(${SEE}); tick(9, 't9');`,
    `var g1 = (async function* () { try { yield await ${x}; } catch (e) { L('c:' + e.message); yield 'rec'; } })(); g1.next().then(${SEE}); g1.next().then(${SEE}); tick(9, 't9');`,
    `(async () => { try { L('await ' + (await ${x})); } catch (e) { L('c:' + e.message); } finally { L('fin'); } })(); tick(6, 't6');`,
    `async function f() { return ${x}; } f().then(${SEE}); tick(7, 't7');`,
    `async function f() { return await ${x}; } f().then(${SEE}); tick(7, 't7');`,
    `async function f() { try { return ${x}; } catch (e) { L('caught'); } } f().then(${SEE}); tick(7, 't7');`,
    `async function f() { try { return await ${x}; } catch (e) { L('caught'); } } f().then(${SEE}); tick(7, 't7');`
  );
}

// 5. Finally com await, yield e return em async generator e async function.
add(
  `async function* g() { try { yield 1; } finally { await null; L('fin'); } } var it = g(); it.next().then(() => it.return('r')).then(ok); tick(9, 't9');`,
  `async function* g() { try { yield 1; } finally { await null; yield 'f'; L('after'); } } var it = g(); it.next().then(() => it.return('r')).then(v => { ok(v); return it.next(); }).then(ok); tick(12, 't12');`,
  `async function* g() { try { yield 1; } finally { await Promise.reject(new Error('fin')); } } var it = g(); it.next().then(() => it.return('r')).then(ok, bad); tick(9, 't9');`,
  `async function* g() { try { yield 1; } finally { await null; return 'over'; } } var it = g(); it.next().then(() => it.return('r')).then(ok); tick(9, 't9');`,
  `async function* g() { try { yield 1; } finally { await null; throw new Error('ft'); } } var it = g(); it.next().then(() => it.throw(new Error('t'))).then(ok, bad); tick(9, 't9');`,
  `async function* g() { try { try { yield 1; } finally { await null; L('in'); } } finally { await null; L('out'); } } var it = g(); it.next().then(() => it.return('r')).then(ok); tick(14, 't14');`,
  `async function* g() { try { yield 1; } finally { L('fin-start'); await thenable(0, 'x'); L('fin-end'); } } var it = g(); it.next(); it.return('r').then(ok); it.next().then(ok); tick(12, 't12');`,
  `async function f() { try { await 1; throw new Error('b'); } finally { await null; L('f1'); } } f().then(ok, bad); tick(6, 't6');`,
  `async function f() { try { return 'a'; } finally { await null; L('f1'); } } f().then(ok, bad); tick(6, 't6');`,
  `async function f() { try { return 'a'; } finally { return await 'b'; } } f().then(ok, bad); tick(6, 't6');`,
  `async function f() { try { throw new Error('a'); } finally { return 'swallow'; } } f().then(ok, bad); tick(5, 't5');`,
  `async function f() { try { throw new Error('a'); } finally { throw new Error('b'); } } f().then(ok, bad); tick(5, 't5');`,
  `async function f() { for (var i = 0; i < 3; i++) { try { if (i === 1) continue; await i; } finally { await null; L('f' + i); } } return 'end'; } f().then(ok); tick(14, 't14');`,
  `async function f() { l: { try { await 1; break l; } finally { await 2; L('fin'); } } L('after'); } f().then(ok); tick(8, 't8');`,
  `async function f() { try { await Promise.reject(new Error('a')); } catch (e) { await null; L('c'); } finally { await null; L('f'); } } f().then(ok); tick(8, 't8');`,
  `async function f() { try { try { await null; throw new Error('i'); } finally { await null; L('f1'); } } catch (e) { L('c:' + e.message); } finally { L('f2'); } } f().then(ok); tick(8, 't8');`,
  `async function f() { try { await null; return 1; } finally { try { await null; throw 2; } catch (e) { L('c' + e); } } } f().then(ok); tick(8, 't8');`,
  `async function f(n) { try { return n; } finally { L('fin' + n); } } f(1).then(ok); f(2).then(ok); tick(5, 't5');`,
  `async function f() { try { yield_ = 1; return await Promise.resolve(2); } finally { L('fin'); } } var yield_; f().then(ok); tick(5, 't5');`
);

// 6. Fontes de yield*: iterador async, sync e sem os métodos return/throw.
const mkAsync = (extra) => `{ [Symbol.asyncIterator]() { var i = 0; return { next(v) { L('an:' + v); return Promise.resolve(i++ < 2 ? { value: i, done: false } : { value: 'R', done: true }); }${extra} }; } }`;
const mkSync = (extra) => `{ [Symbol.iterator]() { var i = 0; return { next(v) { L('sn:' + v); return i++ < 2 ? { value: i, done: false } : { value: 'R', done: true }; }${extra} }; } }`;
const retM = (t) => `, return(v) { L('${t}.ret:' + v); return { value: 'rv', done: true }; }`;
const thrM = (t) => `, throw(e) { L('${t}.thr:' + e); return { value: 'tv', done: false }; }`;
const retMA = (t) => `, return(v) { L('${t}.ret:' + v); return Promise.resolve({ value: 'rv', done: true }); }`;
const thrMA = (t) => `, throw(e) { L('${t}.thr:' + e); return Promise.resolve({ value: 'tv', done: false }); }`;
const srcs = {
  asyncBoth: mkAsync(retMA("a") + thrMA("a")), asyncNoThrow: mkAsync(retMA("a")), asyncNoRet: mkAsync(""),
  syncBoth: mkSync(retM("s") + thrM("s")), syncNoThrow: mkSync(retM("s")), syncNoRet: mkSync(""),
};
const driveAsync = {
  next: "g1.next('A').then(ok, bad); g1.next('B').then(ok, bad); g1.next('C').then(ok, bad); g1.next('D').then(ok, bad);",
  ret: "g1.next().then(ok, bad); g1.return('RR').then(ok, bad); g1.next().then(ok, bad);",
  thr: "g1.next().then(ok, bad); g1.throw('TT').then(ok, bad); g1.next().then(ok, bad);",
};
for (const [sn, s] of Object.entries(srcs)) {
  for (const [dn, d] of Object.entries(driveAsync)) {
    add(`var g1 = (async function* () { var r = yield* ${s}; L('r=' + r); return 'done'; })(); ${d} tick(14, 't14');`);
    add(`var g1 = (async function* () { try { yield* ${s}; } catch (e) { L('c:' + (e && e.name)); } finally { L('fin'); } })(); ${d} tick(14, 't14');`);
  }
  const driveSync = {
    next: "L(JSON.stringify(g1.next('A'))); L(JSON.stringify(g1.next('B'))); L(JSON.stringify(g1.next('C'))); L(JSON.stringify(g1.next('D')));",
    ret: "g1.next(); L(JSON.stringify(g1.return('RR'))); L(JSON.stringify(g1.next()));",
    thr: "g1.next(); L(JSON.stringify(g1.throw('TT'))); L(JSON.stringify(g1.next()));",
  };
  if (sn.startsWith("sync")) {
    for (const [dn, d] of Object.entries(driveSync)) {
      add(`var g1 = (function* () { try { var r = yield* ${s}; L('r=' + r); return 'done'; } catch (e) { L('c:' + (e && e.name)); } finally { L('fin'); } })(); try { ${d} } catch (e) { L('out:' + e.name); }`);
    }
  }
}

// 7. for await sobre iteráveis sync: valores, rejeições, fechamento.
const fvals = {
  array: "[1, 2, 3]", promises: "[Promise.resolve(1), Promise.resolve(2)]", thenables: "[thenable(1, 'a'), thenable(2, 'b')]",
  rejMid: "[1, Promise.reject(new Error('rm')), 3]", string: "'ab'", set: "new Set([1, 2])", map: "new Map([[1, 2]])",
  gen: "(function* () { try { yield 1; yield 2; yield 3; } finally { L('genfin'); } })()", empty: "[]", holes: "[1, , 3]",
  mixed: "[1, Promise.resolve(2), thenable(3, 'c')]", nestedP: "[Promise.resolve(Promise.resolve(1))]",
};
for (const [fn, v] of Object.entries(fvals)) {
  add(
    `(async () => { try { for await (var x of ${v}) L('x=' + JSON.stringify(x)); } catch (e) { L('c:' + e.message); } L('end'); })(); tick(14, 't14');`,
    `(async () => { try { for await (var x of ${v}) { L('x=' + JSON.stringify(x)); break; } } catch (e) { L('c:' + e.message); } L('end'); })(); tick(10, 't10');`,
    `(async () => { try { for await (var x of ${v}) { L('x=' + JSON.stringify(x)); throw new Error('body'); } } catch (e) { L('c:' + e.message); } L('end'); })(); tick(10, 't10');`,
    `(async () => { for await (var x of ${v}) { L('x=' + JSON.stringify(x)); return 'early'; } })().then(ok, bad); tick(10, 't10');`,
    `(async () => { for await (var [a] of [${v}]) L('a=' + a); })().then(ok, bad); tick(10, 't10');`,
    `(async () => { for await (const x of ${v}) { await null; L('x=' + JSON.stringify(x)); } })().then(ok, bad); tick(16, 't16');`
  );
}
// Iterador sync com contadores de microtarefa entre chamadas.
add(
  `var i = 0; var it = { [Symbol.iterator]() { return { next() { L('next' + i); return { value: i, done: i++ >= 2 }; }, return() { L('return'); return {}; } }; } }; (async () => { for await (var x of it) L('x' + x); L('end'); })(); tick(1, 't1'); tick(2, 't2'); tick(3, 't3'); tick(4, 't4'); tick(8, 't8');`,
  `(async () => { for await (var x of [1, 2]) L('a' + x); })(); (async () => { for await (var x of [3, 4]) L('b' + x); })(); tick(9, 't9');`,
  `(async () => { for await (var x of [1, 2]) L('a' + x); L('a-end'); })(); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4')).then(() => L('p5')).then(() => L('p6'));`,
  `(async () => { for await (var x of (async function* () { yield 1; yield 2; })()) L('a' + x); L('a-end'); })(); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4')).then(() => L('p5')).then(() => L('p6')).then(() => L('p7'));`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]: undefined, [Symbol.iterator]() { L('sync'); return [1][Symbol.iterator](); } }) L('x' + x); })(); tick(6, 't6');`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]: null, [Symbol.iterator]() { L('sync'); return [1][Symbol.iterator](); } }) L('x' + x); })().then(ok, bad); tick(6, 't6');`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]: 1 }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of 5) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of null) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of undefined) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]() { return 1; } }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]() { return {}; } }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of { [Symbol.asyncIterator]() { throw new Error('gi'); } }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of { get [Symbol.asyncIterator]() { throw new Error('getter'); } }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (var x of { [Symbol.iterator]() { return { next: 1 }; } }) L('x'); })().then(ok, bad); tick(4, 't4');`,
  `(async () => { for await (x of [1, 2]) L('x' + x); var x; })().then(ok, bad); tick(8, 't8');`,
  `(async () => { var o = {}; for await (o.k of [1, 2]) L('k' + o.k); })().then(ok, bad); tick(8, 't8');`,
  `(async () => { for await (let x of [1, 2]) { setTimeoutless = () => x; L('x' + x); } })().then(ok, bad); var setTimeoutless; tick(8, 't8');`,
  `(async () => { var fs = []; for await (let x of [1, 2, 3]) fs.push(() => x); L(JSON.stringify(fs.map(f => f()))); })(); tick(10, 't10');`,
  `(async () => { outer: for (var i = 0; i < 2; i++) { for await (var x of [1, 2]) { if (x === 1) continue outer; L('no'); } } L('end'); })(); tick(12, 't12');`,
  `(async () => { outer: for await (var x of [1, 2]) { for (var j = 0; j < 2; j++) { if (j === 1) continue outer; L('x' + x + 'j' + j); } } L('end'); })(); tick(12, 't12');`,
  `(async () => { l: for await (var x of [1, 2]) { break l; } L('end'); })(); tick(8, 't8');`
);

// 8. Erros em next e return do iterador (assíncrono e síncrono) dentro de for await e yield*.
const nexts = {
  getterThrows: "get next() { throw new Error('gn'); }", notFn: "next: 1", retNonObj: "next() { return 1; }", retUndef: "next() {}",
  throws: "next() { throw new Error('nt'); }", rejects: "next() { return Promise.reject(new Error('nr')); }",
  retNonObjP: "next() { return Promise.resolve(1); }", doneGetter: "next() { return { get done() { throw new Error('dg'); } }; }",
  valueGetter: "next() { return { done: false, get value() { throw new Error('vg'); } }; }", thenableRes: "next() { return thenable({ done: true }, 'nt'); }",
};
for (const [nn, body] of Object.entries(nexts)) {
  for (const [kind, sym] of [["async", "Symbol.asyncIterator"], ["sync", "Symbol.iterator"]]) {
    // next síncrono devolvendo thenable cru nunca termina (done indefinido), então só vale para o iterador async.
    if (nn === "thenableRes" && kind === "sync") continue;
    const it = `{ [${sym}]() { return { ${body}, return() { L('ret'); return {}; } }; } }`;
    // Resultado de next() que não é objeto com done torna o laço infinito por especificação: o corpo conta as voltas e
    // dá `break` na quinta, registrando no log, para o programa terminar sem mudar o que testa.
    add(`(async () => { var n = 0; try { for await (var x of ${it}) { if (++n > 5) { L('cap'); break; } L('x'); } L('done'); } catch (e) { L('c:' + e.name + ':' + e.message); } })(); tick(8, 't8');`);
    add(`var g1 = (async function* () { yield* ${it}; })(); g1.next().then(ok, bad); g1.next().then(ok, bad); tick(10, 't10');`);
    add(`var g1 = (function* () { yield* ${it}; })(); try { g1.next(); } catch (e) { L('c:' + e.name); }`);
  }
}
const rets = {
  absent: "", nullRet: "return: null", undefRet: "return: undefined", notFn: "return: 1", retNonObj: "return() { return 1; }",
  throws: "return() { throw new Error('rt'); }", rejects: "return() { return Promise.reject(new Error('rr')); }",
  retNonObjP: "return() { return Promise.resolve(1); }", retObj: "return() { return {}; }", thenableRet: "return() { return thenable({}, 'rth'); }",
};
for (const [rn, body] of Object.entries(rets)) {
  for (const [kind, sym] of [["async", "Symbol.asyncIterator"], ["sync", "Symbol.iterator"]]) {
    const wrap = v => kind === "sync" ? v : `Promise.resolve(${v})`;
    const it = `{ [${sym}]() { return { next() { return ${wrap("{ value: 1, done: false }")}; }${body ? ", " + body : ""} }; } }`;
    add(`(async () => { try { for await (var x of ${it}) break; L('broke'); } catch (e) { L('c:' + e.name + ':' + e.message); } })(); tick(8, 't8');`);
    add(`(async () => { try { for await (var x of ${it}) throw new Error('body'); } catch (e) { L('c:' + e.name + ':' + e.message); } })(); tick(8, 't8');`);
    add(`(async () => { for await (var x of ${it}) return 'early'; })().then(ok, bad); tick(8, 't8');`);
    add(`var g1 = (async function* () { yield* ${it}; })(); g1.next().then(ok, bad); g1.return('R').then(ok, bad); tick(10, 't10');`);
    add(`var g1 = (async function* () { yield* ${it}; })(); g1.next().then(ok, bad); g1.throw(new Error('T')).then(ok, bad); tick(10, 't10');`);
    if (kind === "sync") {
      add(`var g1 = (function* () { yield* ${it}; })(); g1.next(); try { L(JSON.stringify(g1.return('R'))); } catch (e) { L('c:' + e.name); }`);
      add(`try { for (var x of ${it}) break; L('broke'); } catch (e) { L('c:' + e.name); }`);
      add(`try { var [a] = ${it}; L('a=' + a); } catch (e) { L('c:' + e.name); }`);
    }
  }
}

// 9. Generators síncronos: yield*, spread, destructuring, argumentos e protótipo.
add(
  `function* inner() { var x = yield 1; L('x=' + x); return 'ir'; } function* outer() { var r = yield* inner(); L('r=' + r); return 'or'; } var g1 = outer(); L(JSON.stringify([g1.next('a'), g1.next('b'), g1.next('c')]));`,
  `function* inner() { try { yield 1; } finally { L('ifin'); } } function* outer() { try { yield* inner(); } finally { L('ofin'); } } var g1 = outer(); g1.next(); L(JSON.stringify(g1.return('x')));`,
  `function* inner() { try { yield 1; } catch (e) { L('ic:' + e); yield 'recovered'; } } function* outer() { yield* inner(); } var g1 = outer(); g1.next(); L(JSON.stringify(g1.throw('boom'))); L(JSON.stringify(g1.next()));`,
  `function* inner() { try { yield 1; } finally { yield 'cleanup'; } } function* outer() { yield* inner(); } var g1 = outer(); g1.next(); L(JSON.stringify(g1.return('x'))); L(JSON.stringify(g1.next()));`,
  `function* g() { yield 1; yield 2; yield 3; } L(JSON.stringify([...g()])); L(JSON.stringify(Array.from(g(), x => x * 2)));`,
  `function* g() { try { yield 1; yield 2; yield 3; } finally { L('fin'); } } var [a, b] = g(); L(a + ',' + b);`,
  `function* g() { try { yield 1; yield 2; } finally { L('fin'); } } var [a, ...rest] = g(); L(a + ',' + JSON.stringify(rest));`,
  `function* g() { try { yield 1; } finally { L('fin'); } } for (var x of g()) { break; } L('after');`,
  `function* g() { try { yield 1; } finally { L('fin'); } } try { for (var x of g()) { throw new Error('b'); } } catch (e) { L(e.message); }`,
  `function* g() { try { yield 1; } finally { throw new Error('fin'); } } try { for (var x of g()) { throw new Error('body'); } } catch (e) { L(e.message); }`,
  `function* g() { try { yield 1; } finally { throw new Error('fin'); } } try { for (var x of g()) { break; } } catch (e) { L(e.message); }`,
  `function* g(a, b = a + 1) { yield arguments.length; yield b; } L(JSON.stringify([...g(1)]));`,
  `function* g() { yield this; } var o = { g }; L(String(o.g().next().value === o)); L(String(g().next().value === undefined || g().next().value === globalThis));`,
  `function* g() { yield new.target; } L(String(g().next().value));`,
  `function* g() {} try { new g(); } catch (e) { L(e.name); }`,
  `var G = Object.getPrototypeOf(function* () {}); L(G.constructor.name); L(String(G === Function.prototype)); L(Object.prototype.toString.call(function* () {}));`,
  `function* g() {} g.prototype = null; L(String(Object.getPrototypeOf(g()) === Object.getPrototypeOf(function* () {}).prototype));`,
  `function* g() {} var i = g(); L(String(i[Symbol.iterator]() === i)); L(Object.prototype.toString.call(i));`,
  `function* g() { var x = yield; L(String(x)); } var i = g(); i.next(1); i.next(2);`,
  `function* g() { yield* 'ab'; yield* [1, 2]; yield* new Set([3]); } L(JSON.stringify([...g()]));`,
  `function* g() { var r = yield* [1]; L(String(r)); } L(JSON.stringify([...g()]));`,
  `function* g() { yield 1; return 2; } L(JSON.stringify([...g()])); var i = g(); i.next(); L(JSON.stringify(i.next())); L(JSON.stringify(i.next()));`,
  `function* g() { return yield yield 1; } var i = g(); L(JSON.stringify([i.next('a'), i.next('b'), i.next('c')]));`,
  `function* g() { yield (yield 1) + (yield 2); } var i = g(); L(JSON.stringify([i.next(), i.next(10), i.next(20), i.next()]));`,
  `function* g() { try { yield 1; } catch (e) { L('c:' + e); } } var i = g(); try { i.throw('early'); } catch (e) { L('out:' + e); } L(JSON.stringify(i.next()));`,
  `function* g() { yield 1; } var i = g(); i.return(1); L(JSON.stringify(i.next())); L(JSON.stringify(i.return(2)));`,
  `function* g() { yield 1; } var i = g(); try { i.throw(new Error('x')); } catch (e) { L(e.message); } L(JSON.stringify(i.next()));`,
  `var o = { *[Symbol.iterator]() { yield 1; yield 2; } }; L(JSON.stringify([...o])); L(JSON.stringify(Array.from(o)));`,
  `class C { *g() { yield 1; } static *s() { yield 2; } } L(JSON.stringify([...new C().g(), ...C.s()]));`,
  `var g = function* named() { yield typeof named; }; L(g().next().value);`,
  `var g = function* () { yield typeof arguments; yield arguments.length; }; L(JSON.stringify([...g(1, 2)]));`,
  `function* g() { yield 1; L('b'); yield 2; } var i = g(); L('created'); i.next(); L('between'); i.next();`,
  `function* g(x) { L('body'); } var i = g((L('arg'), 1)); L('created'); i.next();`,
  `function* g(x = (L('default'), 1)) { L('body'); } var i = g(); L('created'); i.next();`,
  `function* g({ a } = (() => { throw new Error('param'); })()) {} try { g(); } catch (e) { L(e.message); }`,
  `function* g() { yield* g2(); } function* g2() { yield 1; return 2; } var i = g(); L(JSON.stringify([i.next(), i.next()]));`,
  `function* g() { try { yield 1; } finally { L('f1'); } } var i = g(); i.next(); i.return(); i.return(); L('end');`,
  `function* g() { let x = 1; yield () => x++; yield () => x; } var [inc, get] = g(); L(String(inc() + inc() + get()));`
);

// 10. Ordem de microtarefas de funções async e awaits (sem repetir o gerador de microtarefas).
add(
  `async function a() { L('a1'); await null; L('a2'); await null; L('a3'); } async function b() { L('b1'); await null; L('b2'); await null; L('b3'); } a(); b(); L('sync');`,
  `async function a() { await null; L('a'); } a(); Promise.resolve().then(() => L('p1')).then(() => L('p2')); L('sync');`,
  `async function a() { await Promise.resolve(); L('a'); } a(); Promise.resolve().then(() => L('p1')).then(() => L('p2')); L('sync');`,
  `async function a() { await thenable(1, 't'); L('a'); } a(); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')); L('sync');`,
  `var p = Promise.resolve(); async function a() { await p; L('a'); } a(); p.then(() => L('p1')); L('sync');`,
  `var p = Promise.resolve(1); p.then = function (a, b) { L('own then'); return Promise.prototype.then.call(this, a, b); }; (async () => { await p; L('after'); })(); tick(4, 't4');`,
  `var p = Promise.resolve(1); p.constructor = function () {}; (async () => { await p; L('after'); })(); tick(4, 't4');`,
  `class MyP extends Promise {} var p = MyP.resolve(1); (async () => { await p; L('after'); })(); tick(4, 't4');`,
  `(async () => { await { get then() { L('get'); return undefined; } }; L('after'); })(); tick(4, 't4');`,
  `(async () => { try { await { get then() { throw new Error('g'); } }; } catch (e) { L('c:' + e.message); } })(); tick(4, 't4');`,
  `(async () => { await { then(r) { r(1); r(2); L('then'); } }; L('after'); })(); tick(4, 't4');`,
  `(async () => { await { then(r) { throw new Error('t'); } }; })().then(ok, bad); tick(4, 't4');`,
  `(async () => { await { then(r, j) { j(new Error('rj')); } }; })().then(ok, bad); tick(4, 't4');`,
  `async function f() { return Promise.resolve(1); } f().then(() => L('f')); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4'));`,
  `async function f() { return 1; } f().then(() => L('f')); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3'));`,
  `async function f() { return thenable(1, 'r'); } f().then(() => L('f')); Promise.resolve().then(() => L('p1')).then(() => L('p2')).then(() => L('p3')).then(() => L('p4'));`,
  `async function f() { throw new Error('t'); } f().catch(() => L('caught')); Promise.resolve().then(() => L('p1')).then(() => L('p2'));`,
  `async function f(a = (() => { throw new Error('param'); })()) {} var p = f(); L(String(p instanceof Promise)); p.then(ok, bad); tick(3, 't3');`,
  `async function f({ a }) {} f(null).then(ok, bad); tick(3, 't3');`,
  `async function f() { await 1; } L(String(f() instanceof Promise)); L(String(Object.getPrototypeOf(f) === Function.prototype)); L(Object.prototype.toString.call(f));`,
  `async function f() { return arguments.length; } f(1, 2, 3).then(ok); tick(3, 't3');`,
  `var o = { async m() { return this === o; } }; o.m().then(ok); tick(3, 't3');`,
  `var f = async () => this === undefined || this === globalThis; f().then(ok); tick(3, 't3');`,
  `class C { async m() { await null; return this.v; } constructor() { this.v = 7; } } new C().m().then(ok); tick(3, 't3');`,
  `class C { static async s() { return 's'; } async *g() { yield 'g'; } } C.s().then(ok); new C().g().next().then(ok); tick(5, 't5');`,
  `try { new (async function () {})(); } catch (e) { L(e.name); }`,
  `try { new (async () => {})(); } catch (e) { L(e.name); }`,
  `try { new (async function* () {})(); } catch (e) { L(e.name); }`,
  `L(String('prototype' in async function () {})); L(String('prototype' in async function* () {})); L(String('prototype' in function* () {}));`,
  `var AF = Object.getPrototypeOf(async function () {}).constructor; AF('return await 5')().then(ok); tick(3, 't3');`,
  `var AG = Object.getPrototypeOf(async function* () {}).constructor; var g1 = AG('yield 1; yield 2')(); g1.next().then(ok); g1.next().then(ok); tick(6, 't6');`,
  `var GF = Object.getPrototypeOf(function* () {}).constructor; L(JSON.stringify([...GF('yield 1; yield 2')()]));`,
  `(async () => { var r = []; for (var i = 0; i < 3; i++) r.push(await i); L(JSON.stringify(r)); })(); tick(6, 't6');`,
  `(async () => { var r = await Promise.all([1, 2].map(async x => { await null; return x * 2; })); L(JSON.stringify(r)); })(); tick(8, 't8');`,
  `(async () => { L('a'); await (L('b'), null); L('c'); })(); L('d');`,
  `(async () => { var x = (await 1) + (await 2); L(String(x)); })(); tick(5, 't5');`,
  `(async () => { var o = { [await 'k']: await 'v' }; L(JSON.stringify(o)); })(); tick(5, 't5');`,
  `(async () => { L(JSON.stringify(await Promise.all([Promise.resolve(1), 2, thenable(3, 't')]))); })(); tick(7, 't7');`,
  `(async () => { try { await Promise.all([Promise.reject(new Error('a')), Promise.reject(new Error('b'))]); } catch (e) { L(e.message); } })(); tick(5, 't5');`,
  `(async () => { L(await (async () => { try { return await Promise.reject(1); } catch (e) { return 'c' + e; } })()); })(); tick(6, 't6');`,
  `async function f() { await f2(); L('f'); } async function f2() { await null; L('f2'); } f(); tick(4, 't4');`,
  `async function rec(n) { if (n === 0) return 0; return 1 + await rec(n - 1); } rec(5).then(ok); tick(14, 't14');`,
  `var order = []; async function f(id) { order.push(id + 'a'); await null; order.push(id + 'b'); } Promise.all([f(1), f(2), f(3)]).then(() => L(order.join())); tick(8, 't8');`
);

// Prototype preguiçoso de async generator: o objeto criado herda de `g.prototype` lido na chamada, com fallback ao
// realm. Entram depois da amostragem (FIXED), para não deslocar a seleção uniforme das demais famílias.
const FIXED = [
  `async function* g() {} var AGP = Object.getPrototypeOf(async function* () {}).prototype; L(String(Object.getPrototypeOf(g()) === g.prototype)); L(String(Object.getPrototypeOf(g.prototype) === AGP));`,
  `async function* g() {} var AGP = Object.getPrototypeOf(async function* () {}).prototype; g.prototype = null; L(String(Object.getPrototypeOf(g()) === AGP));`,
  `async function* g() {} var AGP = Object.getPrototypeOf(async function* () {}).prototype; g.prototype = 5; L(String(Object.getPrototypeOf(g()) === AGP));`,
  `async function* g() {} var p = { x: 1 }; g.prototype = p; var i = g(); L(String(Object.getPrototypeOf(i) === p)); L(String(i.x));`,
  `async function* g() {} var AGP = Object.getPrototypeOf(async function* () {}).prototype; delete g.prototype; L(String(Object.getPrototypeOf(g()) === AGP));`,
  `async function* g() {} var AGP = Object.getPrototypeOf(async function* () {}).prototype; g.prototype = Object.create(AGP); var i = g(); i.next().then(ok); tick(4, 't4');`,
  `async function* g() { yield 1; } var AGP = Object.getPrototypeOf(async function* () {}).prototype; var f = g; f.prototype = null; var i = f(); i.next().then(ok); L(String(Object.getPrototypeOf(i) === AGP)); tick(4, 't4');`,
  `async function* g() {} L(String(Object.hasOwn(g, 'prototype'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(g, 'prototype'), function (k, v) { return typeof v === 'object' && v !== null && k === 'value' ? 'obj' : v; })); L(String(g() instanceof g));`
];

// 11. Sequências do dia a dia de async generator: consumo manual e erros propagados.
const agBodies = {
  basic: "async function* g() { yield 1; yield 2; yield 3; }",
  awaits: "async function* g() { var a = await 1; yield a; var b = await Promise.resolve(2); yield b; }",
  throwing: "async function* g() { yield 1; throw new Error('boom'); }",
  early: "async function* g() { throw new Error('start'); }",
  ret: "async function* g() { yield 1; return 'fin'; }",
  yieldStar: "async function* g() { yield* [1, 2]; yield* (async function* () { yield 3; return 'ir'; })(); }",
  args: "async function* g(a, b = a * 2) { yield a; yield b; yield arguments.length; }",
  retYield: "async function* g() { var x = yield 1; return x; }",
};
for (const [bn, body] of Object.entries(agBodies)) {
  add(
    `${body} (async () => { var it = g(5); var r; try { do { r = await it.next('in'); L(JSON.stringify(r)); } while (!r.done); } catch (e) { L('c:' + e.message); } L(JSON.stringify(await it.next())); })(); tick(20, 't20');`,
    `${body} var it = g(5); var rs = [it.next(), it.next(), it.next(), it.next(), it.next()]; Promise.allSettled(rs).then(r => L(JSON.stringify(r.map(x => x.status + ':' + JSON.stringify(x.value))))); tick(14, 't14');`,
    `${body} (async () => { for await (var x of g(5)) L('x=' + JSON.stringify(x)); L('end'); })().catch(e => L('c:' + e.message)); tick(20, 't20');`,
    `${body} (async () => { var it = g(5); await it.next(); L(JSON.stringify(await it.return('R'))); L(JSON.stringify(await it.next())); })().catch(e => L('c:' + e.message)); tick(14, 't14');`,
    `${body} (async () => { var it = g(5); try { await it.throw(new Error('T')); } catch (e) { L('c:' + e.message); } L(JSON.stringify(await it.next())); })(); tick(14, 't14');`,
    `${body} (async () => { var it = g(5); await it.next(); try { await it.throw(new Error('T')); } catch (e) { L('c:' + e.message); } L(JSON.stringify(await it.next())); })(); tick(14, 't14');`,
    `${body} (async () => { var it = g(5); L(String(it[Symbol.asyncIterator]() === it)); L(Object.prototype.toString.call(it)); L(typeof it[Symbol.iterator]); })(); tick(2, 't2');`
  );
}

// Amostra determinística por hash (sampleByHash) do conjunto candidato inteiro; só depois saem os que os goldens vizinhos já têm.
const TARGET = 420;
let selected = sampleByHash(programs, TARGET).filter((source) => !existing.has(source));
selected = selected.concat(FIXED.filter((source) => !existing.has(source) && !seen.has(source)));

// Executa cada programa no bun, num processo próprio, pela API vm (a fonte nunca é um arquivo do projeto).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "async-gen-golden-"));
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
const lines = [];
selected.forEach((source, index) => {
  const srcFile = path.join(tmp, `p${index}.txt`);
  fs.writeFileSync(srcFile, source);
  const run = spawnSync(process.execPath, [driver, srcFile, harnessFile], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") process.stderr.write(`FALHA: ${source}\n${run.stderr}\n`);
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
const output = lines.join("\n") + "\n";
if (output.includes(tmp) || /\/home\/|\/tmp\//.test(output)) throw new Error("o golden vazou um caminho da máquina");
if (/error\tHarness/.test(output)) throw new Error("programa sem resultado do bun (timeout ou falha)");
process.stdout.write(output);
process.stderr.write(`${selected.length} de ${programs.length} programas\n`);
