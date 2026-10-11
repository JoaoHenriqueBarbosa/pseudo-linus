// Gera tests/golden/generator_close_bun.tsv: controle não local em generators e async, medido no bun 1.4.2.
// Cobre break/continue rotulados atravessando try/finally com yield/await no finally, return dentro de finally
// sobrescrevendo throw, for-of sobre generator com break chamando return() que por sua vez faz yield (e iteradores
// com return() que devolve não objeto, lança ou devolve done:false), destructuring de generator com elementos a mais
// e a menos (fechamento), spread, Array.from com mapfn que lança, Promise.all/allSettled/race/any, new Map/Set/WeakMap
// com generator que lança depois de duas entradas, Object.fromEntries, e o log da ordem das chamadas.
// Colunas: fonte do programa (JSON) e o texto de `R` (JSON), com prelúdio fatorado (`tests/golden/generator_close.preludes.json`).
// `R` é um acessor global do prelúdio que junta o log `L(...)`; o bun lê `R` depois de esvaziar as microtarefas.
// Cada programa roda num processo bun novo (no máximo 6 em paralelo, timeout de 8 s), sem APIs de host no programa.
// Uso: bun scripts/gen-generator-close-golden.js > tests/golden/generator_close_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");
const { knownPrograms, emitFactored, prepareProgram, sampleByHash } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

const PRELUDE = [
  "var LOG = [];",
  "function L(x) { LOG.push(typeof x === 'string' ? x : S(x)); }",
  "Object.defineProperty(globalThis, 'R', { get: function () { return LOG.join('|'); }, configurable: true });",
  "function S(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'function') return 'fn';",
  "  if (v === null || typeof v !== 'object') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (d > 3) return '...';",
  "  if (v instanceof Error) return v.name + ':' + v.message;",
  "  if (Array.isArray(v)) return '[' + v.map(function (x) { return S(x, d + 1); }).join(',') + ']';",
  "  if (v instanceof Map) return 'Map(' + Array.from(v.entries()).map(function (x) { return S(x[0], d + 1) + '=>' + S(x[1], d + 1); }).join(',') + ')';",
  "  if (v instanceof Set) return 'Set(' + Array.from(v.values()).map(function (x) { return S(x, d + 1); }).join(',') + ')';",
  "  return '{' + Object.keys(v).map(function (k) { return k + ':' + S(v[k], d + 1); }).join(',') + '}';",
  "}",
  "function E(e) { return e instanceof Error ? e.name + ':' + e.message : 'thrown ' + S(e); }",
  "function T(f) { try { return f(); } catch (e) { L('catch:' + E(e)); } }",
  "function N(g, n) { for (var k = 0; k < n; k++) L('n' + k + '=' + S(T(function () { return g.next(); }))); }",
  "function AN(g, n) {",
  "  var p = Promise.resolve();",
  "  for (let k = 0; k < n; k++) p = p.then(function () { return g.next(); }).then(function (r) { L('n' + k + '=' + S(r)); }, function (e) { L('n' + k + ' rej ' + E(e)); });",
  "  return p;",
  "}",
  "function TK(n) { var p = Promise.resolve(); for (let i = 1; i <= n; i++) p = p.then(function () { L('tick' + i); }); return p; }",
  // Generator com log: n elementos; o: throwAt, finYield, finThrow, finRet, pair.
  "function G(tag, n, o) {",
  "  o = o || {};",
  "  return (function* () {",
  "    L(tag + ':start');",
  "    try {",
  "      for (var i = 0; i < n; i++) {",
  "        L(tag + ':y' + i);",
  "        yield o.pair ? [i, 'v' + i] : i;",
  "        if (o.throwAt === i) throw new Error(tag + 'T' + i);",
  "      }",
  "      L(tag + ':end');",
  "      return 'done';",
  "    } finally {",
  "      L(tag + ':fin');",
  "      if (o.finYield) { yield 'F'; L(tag + ':afterFinYield'); }",
  "      if (o.finThrow) throw new Error(tag + 'FT');",
  "      if (o.finRet) return 'FR';",
  "    }",
  "  })();",
  "}",
  // Iterador manual: o: throwAt (next lança), badAt (next devolve não objeto), ret ('obj','undef','prim','throw','notdone'), pair.
  "function CI(tag, n, o) {",
  "  o = o || {};",
  "  var i = 0;",
  "  return {",
  "    [Symbol.iterator]() { L(tag + ':iter'); return this; },",
  "    next() {",
  "      L(tag + ':next' + i);",
  "      if (o.throwAt === i) throw new Error(tag + 'NT' + i);",
  "      if (o.badAt === i) return 5;",
  "      if (i >= n) return { done: true, value: 'end' };",
  "      var k = i++;",
  "      return { done: false, value: o.pair ? [k, 'v' + k] : k };",
  "    },",
  "    return(v) {",
  "      L(tag + ':return');",
  "      if (o.ret === 'throw') throw new Error(tag + 'RT');",
  "      if (o.ret === 'undef') return undefined;",
  "      if (o.ret === 'prim') return 5;",
  "      if (o.ret === 'notdone') return { done: false, value: 'x' };",
  "      return {};",
  "    }",
  "  };",
  "}",
  "",
].join("\n");

// Caracteres que a ferramenta de escrita converteria; montados em tempo de execução.
const EN_DASH = String.fromCharCode(0x2013);
const EM_DASH = String.fromCharCode(0x2014);

const programs = [];
const seen = new Set();
const add = (body) => {
  const source = PRELUDE + body;
  if (/[\t\r]/.test(body)) throw new Error("fonte com tab: " + body);
  if (!seen.has(source)) { seen.add(source); programs.push(source); }
};
const product = (...lists) => lists.reduce((acc, list) => acc.flatMap((a) => list.map((b) => [...a, b])), [[]]);

// ---- 1. break/continue rotulados atravessando try/finally com yield no finally (generator síncrono).
const FIN = {
  none: "L('fin');",
  yield: "L('fin1'); yield 'Y'; L('fin2');",
  throw: "L('fin1'); throw new Error('FT');",
  ret: "L('fin1'); return 'FR';",
  brkOut: "L('fin1'); break OUT;",
  contIn: "L('fin1'); continue IN;",
  yieldTwice: "yield 'Y1'; yield 'Y2'; L('fin');",
  yieldThrow: "yield 'Y'; throw new Error('FT2');",
};
const JUMPS = ["break OUT;", "break IN;", "continue OUT;", "continue IN;", "if (j == 1) break OUT;", "if (j == 1) continue OUT;", "if (i == 1) break IN;", "if (j == 0) continue IN; else break OUT;"];
const STRUCT = {
  loops: (jump, fin) => `OUT: for (var i = 0; i < 3; i++) { IN: for (var j = 0; j < 3; j++) { try { L('b' + i + j); yield i * 10 + j; ${jump} } finally { ${fin} } } L('afterIn' + i); } L('end');`,
  nested: (jump, fin) => `OUT: for (var i = 0; i < 2; i++) { IN: for (var j = 0; j < 2; j++) { try { try { yield i * 10 + j; ${jump} } finally { ${fin} } } finally { L('outerFin'); yield 'O'; } } } L('end');`,
  catchy: (jump, fin) => `OUT: for (var i = 0; i < 2; i++) { IN: for (var j = 0; j < 2; j++) { try { try { yield i * 10 + j; throw new Error('in'); } catch (e) { L('c:' + e.message); ${jump} } finally { ${fin} } } finally { L('outerFin'); } } } L('end');`,
  whileLoop: (jump, fin) => `var i = 0, j = 0; OUT: while (i < 2) { i++; j = 0; IN: do { j++; try { yield i * 10 + j; ${jump} } finally { ${fin} } } while (j < 2); } L('end');`,
};
const DRIVERS = {
  manual: "N(g, 9);",
  forof: "T(function () { for (var x of g) L('x=' + S(x)); });",
  breakFirst: "T(function () { for (var x of g) { L('x=' + S(x)); break; } }); N(g, 2);",
  breakSecond: "T(function () { var c = 0; for (var x of g) { L('x=' + S(x)); if (++c == 2) break; } }); N(g, 2);",
  ret: "N(g, 1); L(S(T(function () { return g.return('R'); }))); N(g, 3);",
  throw1: "N(g, 1); L(S(T(function () { return g.throw(new Error('X')); }))); N(g, 3);",
  ret2: "N(g, 3); L(S(T(function () { return g.return(7); }))); N(g, 3);",
  spread: "L(T(function () { return [...g]; }));",
};
for (const sname of Object.keys(STRUCT)) for (const jump of JUMPS) for (const fname of Object.keys(FIN)) for (const dname of Object.keys(DRIVERS)) {
  // Amostra determinística: nem toda combinação, para equilibrar com as outras seções.
  const h = (sname.length * 7 + jump.length * 3 + fname.length * 5 + dname.length * 11) % 3;
  if (h !== 0 && !(sname === "loops" || dname === "manual")) continue;
  add(`var g = (function* () { ${STRUCT[sname](jump, FIN[fname])} })(); ${DRIVERS[dname]}`);
}

// Rótulo em bloco e em switch.
for (const fname of Object.keys(FIN)) for (const dname of Object.keys(DRIVERS)) {
  const f = FIN[fname].replace(/break OUT;/, "break BLK;").replace(/continue IN;/, "L('ci');");
  add(`var g = (function* () { BLK: { try { yield 1; break BLK; } finally { ${f} } L('unreach'); } L('afterBlk'); yield 2; })(); ${DRIVERS[dname]}`);
  add(`var g = (function* () { SW: switch (1) { case 1: try { yield 1; break SW; } finally { ${f} } L('unreach'); case 2: yield 3; } L('afterSw'); yield 2; })(); ${DRIVERS[dname]}`);
  add(`var g = (function* () { try { yield 1; } finally { ${f} } yield 2; })(); ${DRIVERS[dname]}`);
}

// ---- 2. return em finally sobrescrevendo throw e return.
const BODIES = {
  throwNoCatch: "try { yield 1; throw new Error('B'); } finally { L('fin'); RET }",
  throwCatchRethrow: "try { try { yield 1; throw new Error('B'); } catch (e) { L('c'); throw e; } } finally { L('fin'); RET }",
  retTry: "try { yield 1; return 'TR'; } finally { L('fin'); RET }",
  retTryYield: "try { yield 1; return (yield 2); } finally { L('fin'); RET }",
  loopThrow: "for (var i = 0; i < 3; i++) { try { yield i; if (i == 1) throw new Error('B' + i); } finally { L('fin' + i); RET } }",
  nestedFin: "try { try { yield 1; throw new Error('B'); } finally { L('f1'); RET } } finally { L('f2'); }",
  nestedFin2: "try { try { yield 1; throw new Error('B'); } finally { L('f1'); } } finally { L('f2'); RET }",
};
const RETS = ["return 'FR';", "return;", "yield 'FY'; return 'FR';", "throw new Error('FT');", "if (true) return 'FR';", "try { return 'FR'; } finally { L('inner'); }", "try { throw new Error('IT'); } finally { return 'FR2'; }", "return (yield 'FY');"];
const DRV2 = {
  manual: "N(g, 6);",
  forof: "T(function () { for (var x of g) L('x=' + S(x)); });",
  ret: "N(g, 1); L(S(T(function () { return g.return('R'); }))); N(g, 3);",
  throw1: "N(g, 1); L(S(T(function () { return g.throw(new Error('X')); }))); N(g, 3);",
  nextVal: "L(S(g.next('a'))); L(S(T(function () { return g.next('b'); }))); L(S(T(function () { return g.next('c'); }))); N(g, 2);",
  spread: "L(T(function () { return [...g]; }));",
  destr: "T(function () { var [a, b] = g; L([a, b]); });",
};
for (const bname of Object.keys(BODIES)) for (const ret of RETS) for (const dname of Object.keys(DRV2)) {
  add(`var g = (function* () { ${BODIES[bname].replace("RET", ret)} })(); ${DRV2[dname]}`);
}

// ---- 3. for-of sobre generator/iterador com ação no corpo e fechamento (return() que faz yield, lança etc).
const GSRC = [];
for (const n of [0, 1, 3]) for (const o of ["", "finYield:1", "finThrow:1", "finRet:1", "finYield:1,finThrow:1", "throwAt:0", "throwAt:1,finYield:1", "throwAt:1,finThrow:1"]) {
  GSRC.push(`G('a', ${n}, {${o}})`);
}
const CSRC = [];
for (const n of [0, 2, 3]) for (const o of ["", "ret:'throw'", "ret:'undef'", "ret:'prim'", "ret:'notdone'", "throwAt:1", "badAt:1", "throwAt:1,ret:'throw'", "badAt:1,ret:'throw'"]) {
  CSRC.push(`CI('c', ${n}, {${o}})`);
}
const ACTIONS = {
  none: "L('x=' + S(x));",
  break: "L('x=' + S(x)); break;",
  breakOut: "L('x=' + S(x)); break OUT;",
  continueOut: "L('x=' + S(x)); continue OUT;",
  continue: "L('x=' + S(x)); continue;",
  throw: "L('x=' + S(x)); throw new Error('body');",
  return: "L('x=' + S(x)); return 'ret';",
  breakIf: "L('x=' + S(x)); if (x == 1) break;",
  tryBreak: "try { L('x=' + S(x)); break; } finally { L('bodyFin'); }",
  tryBreakFinThrow: "try { L('x=' + S(x)); break; } finally { L('bodyFin'); throw new Error('bf'); }",
};
for (const src of [...GSRC, ...CSRC]) for (const aname of Object.keys(ACTIONS)) {
  add(`function f() { OUT: for (var o of [0, 1]) { L('o' + o); for (var x of ${src}) { ${ACTIONS[aname]} } } L('end'); return 'normal'; } L(S(T(f))); N(G('post', 1), 0);`);
}
// Dois iteradores aninhados: a ordem dos fechamentos.
for (const [a, b] of product(["G('a', 3, {})", "G('a', 3, {finYield:1})", "CI('a', 3, {ret:'throw'})", "G('a', 3, {finThrow:1})"], ["G('b', 3, {})", "G('b', 3, {finYield:1})", "CI('b', 3, {ret:'throw'})", "G('b', 3, {finThrow:1})"])) {
  for (const act of ["break OUT;", "continue OUT;", "return 'r';", "throw new Error('body');", "break;"]) {
    add(`function f() { OUT: for (var x of ${a}) { L('x=' + S(x)); for (var y of ${b}) { L('y=' + S(y)); ${act} } } return 'end'; } L(S(T(f)));`);
  }
}
// For-of dentro de generator/async com yield no corpo e return() do consumidor.
for (const src of GSRC.slice(0, 12)) for (const dname of ["manual", "ret", "throw1", "forof"]) {
  add(`var g = (function* () { for (var x of ${src}) { L('x=' + S(x)); yield 'o' + x; } L('endOuter'); })(); ${DRIVERS[dname]}`);
  add(`var g = (function* () { try { for (var x of ${src}) { yield 'o' + x; } } finally { L('outerFin'); yield 'OF'; } })(); ${DRIVERS[dname]}`);
  add(`var g = (function* () { yield* ${src}; L('afterYieldStar'); return 'ys'; })(); ${DRIVERS[dname]}`);
}

// ---- 4. destructuring, spread e demais consumidores sobre as fontes.
const DESTR = {
  two: "var [a, b] = SRC; L([a, b]);",
  three: "var [a, b, c] = SRC; L([a, b, c]);",
  hole: "var [a, , b] = SRC; L([a, b]);",
  empty: "var [] = SRC; L('empty');",
  comma: "var [,] = SRC; L('comma');",
  rest: "var [a, ...r] = SRC; L([a, r]);",
  restOnly: "var [...r] = SRC; L(r);",
  defaults: "var [a = 'd1', b = 'd2', c = 'd3', d = 'd4'] = SRC; L([a, b, c, d]);",
  assign: "var a, b; [a, b] = SRC; L([a, b]);",
  nested: "var { x: [a, b] } = { x: SRC }; L([a, b]);",
  param: "(function ([a, b]) { L([a, b]); })(SRC);",
  forofPat: "for (var [a, b] of [SRC]) L([a, b]);",
  defaultThrows: "var [a = (function () { throw new Error('dflt'); })(), b] = SRC; L([a, b]);",
  assignTarget: "var o = {}; [o.a, o.b] = SRC; L(o);",
  targetThrows: "var o = { set a(v) { throw new Error('setter'); } }; [o.a, o.b] = SRC; L(o);",
  catchParam: "try { throw 1; } catch ([a] ) { } L('x');",
};
delete DESTR.catchParam;
const CONSUME = {
  spread: "L([...SRC]);",
  spreadMid: "L([0, ...SRC, 9]);",
  from: "L(Array.from(SRC));",
  fromMap: "L(Array.from(SRC, function (v, i) { return v + ':' + i; }));",
  fromMapThrow: "L(Array.from(SRC, function (v, i) { if (i == 1) throw new Error('mapfn'); return v; }));",
  fromMapOnce: "L(Array.from(SRC, function (v, i) { L('map' + i); return v; }));",
  map: "L(new Map(SRC));",
  set: "L(new Set(SRC));",
  weakMap: "new WeakMap(SRC); L('wm');",
  weakSet: "new WeakSet(SRC); L('ws');",
  fromEntries: "L(Object.fromEntries(SRC));",
  max: "L(Math.max(...SRC));",
  u8: "L(new Uint8Array(SRC));",
  call: "L(String.fromCharCode(...SRC));",
  apply: "L(Math.min.apply(null, [...SRC]));",
  newSpread: "L(new Array(...SRC));",
  yieldStar: "var g = (function* () { var r = yield* SRC; L('r=' + S(r)); })(); N(g, 6);",
  mapSetOverride: "var s = new Set(); s.add = function (v) { L('add' + v); if (v == 1) throw new Error('add'); return Set.prototype.add.call(this, v); }; Set.call; L('x');",
  groupBy: "L(Object.groupBy(SRC, function (v) { return v % 2 ? 'odd' : 'even'; }));",
  mapGroupBy: "L(Map.groupBy(SRC, function (v) { if (v == 1) throw new Error('gb'); return v; }));",
  mapProtoAdd: "var orig = Map.prototype.set; Map.prototype.set = function (k, v) { L('set' + k); if (k == 1) throw new Error('set'); return orig.call(this, k, v); }; T(function () { L(new Map(SRC)); }); Map.prototype.set = orig;",
};
delete CONSUME.mapSetOverride;
const PAIR_SRC = [];
for (const n of [0, 1, 2, 3, 4]) for (const o of ["", "throwAt:1", "throwAt:2", "throwAt:1,finYield:1", "throwAt:2,finThrow:1", "finYield:1", "finThrow:1", "finRet:1"]) {
  PAIR_SRC.push(`G('p', ${n}, {pair:1${o ? "," + o : ""}})`);
}
for (const n of [1, 3, 5]) for (const o of ["", "ret:'throw'", "ret:'prim'", "throwAt:2", "badAt:2", "throwAt:2,ret:'throw'"]) {
  PAIR_SRC.push(`CI('p', ${n}, {pair:1${o ? "," + o : ""}})`);
}
const VAL_SRC = [];
for (const n of [0, 1, 2, 3, 4]) for (const o of ["", "throwAt:1", "throwAt:2", "throwAt:1,finYield:1", "throwAt:2,finThrow:1", "finYield:1", "finThrow:1", "finRet:1"]) {
  VAL_SRC.push(`G('v', ${n}, {${o}})`);
}
for (const n of [1, 3, 5]) for (const o of ["", "ret:'throw'", "ret:'prim'", "ret:'notdone'", "throwAt:2", "badAt:2", "throwAt:2,ret:'throw'"]) {
  VAL_SRC.push(`CI('v', ${n}, {${o}})`);
}
const needsPair = new Set(["map", "weakMap", "fromEntries", "mapProtoAdd"]);
for (const [name, tpl] of Object.entries(DESTR)) for (const src of VAL_SRC) add(`T(function () { ${tpl.replace(/SRC/g, src)} });`);
for (const [name, tpl] of Object.entries(CONSUME)) {
  for (const src of needsPair.has(name) ? PAIR_SRC : [...VAL_SRC, ...(name === "set" || name === "from" || name === "spread" ? [] : [])]) {
    add(`T(function () { ${tpl.replace(/SRC/g, src)} });`);
  }
}
// Mesmos consumidores de valor com fontes de par onde o item não é objeto (TypeError que fecha o iterador).
for (const name of ["map", "weakMap", "fromEntries"]) for (const src of VAL_SRC.slice(0, 20)) add(`T(function () { ${CONSUME[name].replace(/SRC/g, src)} });`);
// new Map(generator) que lança depois de duas entradas, com o set observado.
for (const throwAt of [0, 1, 2, 3]) for (const fin of ["", ", finYield:1", ", finThrow:1", ", finRet:1"]) for (const how of ["Map", "WeakMap", "Set", "Object.fromEntries", "Promise.all", "Array.from"]) {
  const src = `G('m', 5, {pair:1, throwAt:${throwAt}${fin}})`;
  const call = how === "Object.fromEntries" || how === "Array.from" ? `${how}(${src})` : how === "Promise.all" ? `Promise.all(${src}).then(L, function (e) { L('rej:' + E(e)); })` : `new ${how}(${src})`;
  add(`T(function () { L(${call}); }); TK(4);`);
}

// ---- 5. Promise combinators com generator.
const PSRC = [];
for (const n of [0, 1, 3]) for (const o of ["", "throwAt:1", "throwAt:0", "finYield:1", "finThrow:1"]) {
  PSRC.push(`G('p', ${n}, {${o}})`);
  PSRC.push(`(function* () { try { for (var i = 0; i < ${n}; i++) yield i % 2 ? Promise.reject(new Error('r' + i)) : Promise.resolve('p' + i); } finally { L('pfin'); } })()`);
  PSRC.push(`(function* () { try { for (var i = 0; i < ${n}; i++) yield { then(res, rej) { L('then' + i); res('t'); } }; ${o.includes("throwAt") ? "throw new Error('pt');" : ""} } finally { L('pfin'); } })()`);
}
for (const m of ["all", "allSettled", "race", "any"]) for (const src of PSRC) {
  add(`Promise.${m}(${src}).then(function (v) { L('ok:' + S(v)); }, function (e) { L('rej:' + (e instanceof AggregateError ? 'Agg' : E(e))); }); TK(6);`);
}
// Promise.all com Promise.resolve sobrescrito: erro em resolve fecha o iterador.
for (const m of ["all", "allSettled", "race", "any"]) for (const src of ["G('q', 3, {})", "G('q', 3, {finYield:1})", "CI('q', 3, {})", "CI('q', 3, {ret:'throw'})", "CI('q', 3, {ret:'prim'})"]) {
  add(`var orig = Promise.resolve; Promise.resolve = function (v) { L('resolve' + v); if (v == 1) throw new Error('res'); return orig.call(this, v); }; Promise.${m}(${src}).then(function (v) { L('ok:' + S(v)); }, function (e) { L('rej:' + E(e)); }); Promise.resolve = orig; TK(6);`);
  add(`var P = function (ex) { return new Promise(ex); }; P.resolve = function () { throw new Error('noresolve'); }; Promise.${m}.call(P, ${src}).then(function (v) { L('ok:' + S(v)); }, function (e) { L('rej:' + E(e)); }); TK(6);`);
  add(`Promise.${m}.call(function (ex) { ex(function () { L('res'); }, function () { L('rej'); }); throw new Error('ctor'); }, ${src}); L('sync');`);
}

// ---- 6. async: await no finally, break/continue rotulado, async generators e for await.
const AFIN = {
  await: "L('f1'); await null; L('f2');",
  awaitThrow: "L('f1'); await null; throw new Error('AF');",
  awaitRet: "L('f1'); await null; return 'AFR';",
  awaitRej: "L('f1'); await Promise.reject(new Error('AR')); L('f2');",
  awaitBreak: "L('f1'); await null; break OUT;",
  awaitCont: "L('f1'); await null; continue IN;",
  awaitTwo: "await null; await null; L('f');",
  none: "L('f');",
};
const AJUMPS = ["break OUT;", "break IN;", "continue OUT;", "continue IN;", "if (j == 1) break OUT;", "if (j == 1) continue OUT;"];
const ASTRUCT = {
  fn: (j, f) => `OUT: for (var i = 0; i < 2; i++) { IN: for (var j = 0; j < 3; j++) { try { L('b' + i + j); await null; ${j} } finally { ${f} } } L('afterIn' + i); } L('end'); return 'AEND';`,
  nested: (j, f) => `OUT: for (var i = 0; i < 2; i++) { IN: for (var j = 0; j < 2; j++) { try { try { await null; ${j} } finally { ${f} } } finally { L('outerFin'); await null; L('outerFin2'); } } } L('end'); return 'AEND';`,
};
for (const sname of Object.keys(ASTRUCT)) for (const jump of AJUMPS) for (const fname of Object.keys(AFIN)) {
  const body = ASTRUCT[sname](jump, AFIN[fname]);
  add(`(async function () { ${body} })().then(function (v) { L('v:' + S(v)); }, function (e) { L('e:' + E(e)); }); TK(10);`);
}
const AGDRV = {
  manual: "AN(g, 8);",
  forawait: "(async function () { try { for await (var x of g) L('x=' + S(x)); } catch (e) { L('catch:' + E(e)); } L('done'); })();",
  breakFirst: "(async function () { try { for await (var x of g) { L('x=' + S(x)); break; } } catch (e) { L('catch:' + E(e)); } L('done'); return AN(g, 2); })();",
  ret: "AN(g, 1).then(function () { return g.return('R'); }).then(function (r) { L('r=' + S(r)); }, function (e) { L('rret:' + E(e)); }).then(function () { return AN(g, 3); });",
  throw1: "AN(g, 1).then(function () { return g.throw(new Error('X')); }).then(function (r) { L('r=' + S(r)); }, function (e) { L('rthrow:' + E(e)); }).then(function () { return AN(g, 3); });",
  concurrent: "var a = g.next(), b = g.next(), c = g.return('R'), d = g.next(); [a, b, c, d].forEach(function (p, k) { p.then(function (r) { L('p' + k + '=' + S(r)); }, function (e) { L('p' + k + ' rej ' + E(e)); }); });",
};
const AGBODY = {
  tryFin: (f) => `try { L('s'); yield 1; yield 2; yield 3; } finally { ${f} }`,
  labeled: (f) => `OUT: for (var i = 0; i < 3; i++) { try { yield i; if (i == 1) break OUT; } finally { ${f.replace("break OUT;", "L('b');")} } } L('end');`,
  throwBody: (f) => `try { yield 1; throw new Error('B'); } finally { ${f.replace("break OUT;", "L('b');").replace("continue IN;", "L('c');")} }`,
  retTry: (f) => `try { yield 1; return 'TR'; } finally { ${f.replace("break OUT;", "L('b');").replace("continue IN;", "L('c');")} }`,
  awaitYield: (f) => `try { var v = yield await 1; L('v=' + v); yield Promise.resolve(2); } finally { ${f.replace("break OUT;", "L('b');").replace("continue IN;", "L('c');")} }`,
};
for (const bname of Object.keys(AGBODY)) for (const fname of Object.keys(AFIN)) for (const dname of Object.keys(AGDRV)) {
  add(`var g = (async function* () { ${AGBODY[bname](AFIN[fname])} })(); ${AGDRV[dname]} TK(14);`);
}
// Async gen com yield no finally (yield* / yield / await misturados).
for (const fy of ["yield 'F1';", "yield 'F1'; yield 'F2';", "yield await 'F1';", "await null; yield 'F1';", "yield* [1, 2];", "yield Promise.reject(new Error('FY'));", "return await 'FR';", "return Promise.resolve('FRP');"]) for (const dname of Object.keys(AGDRV)) {
  add(`var g = (async function* () { try { yield 1; yield 2; } finally { L('fin'); ${fy} } })(); ${AGDRV[dname]} TK(14);`);
  add(`var g = (async function* () { try { yield 1; throw new Error('B'); } finally { L('fin'); ${fy} } })(); ${AGDRV[dname]} TK(14);`);
}
// for await sobre sync iterables/generators e fechamento (break chama return do iterador sync, resolvendo o valor).
const FASRC = ["G('s', 3, {})", "G('s', 3, {finYield:1})", "G('s', 3, {finThrow:1})", "G('s', 3, {throwAt:1})", "CI('s', 3, {ret:'throw'})", "CI('s', 3, {ret:'prim'})", "CI('s', 3, {ret:'undef'})", "CI('s', 3, {ret:'notdone'})", "[Promise.resolve(1), Promise.reject(new Error('rj')), 3]", "[1, 2, 3]", "[{ then(r) { L('thn'); r('T'); } }, 2]"];
const FAACT = { none: "L('x=' + S(x));", break: "L('x=' + S(x)); break;", cont: "L('x=' + S(x)); continue;", throw: "L('x=' + S(x)); throw new Error('body');", ret: "L('x=' + S(x)); return 'ret';", breakOut: "L('x=' + S(x)); break OUT;", awaitBreak: "await null; L('x=' + S(x)); break;" };
for (const src of FASRC) for (const aname of Object.keys(FAACT)) {
  add(`(async function () { OUT: for (var o of [0, 1]) { for await (var x of ${src}) { ${FAACT[aname]} } L('o' + o); } return 'end'; })().then(function (v) { L('v:' + S(v)); }, function (e) { L('e:' + E(e)); }); TK(12);`);
}
// Async iterators manuais: return() assíncrono, que rejeita, que devolve não objeto.
for (const ret of ["return Promise.resolve({ done: true })", "return Promise.reject(new Error('RR'))", "return 5", "return Promise.resolve(5)", "throw new Error('RT')", "return { then(r) { L('rthen'); r({ done: true }); } }", "await null; return {}"]) for (const aname of Object.keys(FAACT)) {
  add(`var it = { [Symbol.asyncIterator]() { return this; }, i: 0, next() { L('next' + this.i); return Promise.resolve({ done: this.i > 3, value: this.i++ }); }, async return(v) { L('return'); ${ret.startsWith("return") || ret.startsWith("throw") || ret.startsWith("await") ? ret : "return " + ret}; } }; (async function () { OUT: for (var o of [0]) { for await (var x of it) { ${FAACT[aname]} } } return 'end'; })().then(function (v) { L('v:' + S(v)); }, function (e) { L('e:' + E(e)); }); TK(12);`);
}
// Ticker concorrente: ordem de log entre async function com finally e o ticker.
for (const fname of Object.keys(AFIN)) for (const n of [3, 6]) {
  add(`(async function () { OUT: for (var i = 0; i < 2; i++) { IN: for (var j = 0; j < 2; j++) { try { L('b' + i + j); if (j == 0) continue IN; break OUT; } finally { ${AFIN[fname]} } } } L('end'); })().then(function () { L('settled'); }, function (e) { L('e:' + E(e)); }); TK(${n});`);
}

// ---- 7. return/throw/next reentrantes e geradores em execução.
for (const act of ["g.next()", "g.return(1)", "g.throw(new Error('re'))", "[...g]", "g[Symbol.iterator]().next()"]) for (const where of ["body", "fin", "catch"]) {
  const inner = `L(S(T(function () { return ${act}; })));`;
  const body = where === "body" ? `${inner} yield 1;` : where === "fin" ? `try { yield 1; } finally { ${inner} }` : `try { throw 1; } catch (e) { ${inner} } yield 1;`;
  for (const dname of ["manual", "ret", "throw1", "forof"]) add(`var g = (function* () { ${body} })(); ${DRIVERS[dname]}`);
}
// Yield em finally com return() repetido e throw() dentro de finally.
for (const fin of ["yield 'F';", "yield 'F1'; yield 'F2';", "try { yield 'F'; } finally { L('inner'); yield 'I'; }", "try { yield 'F'; } catch (e) { L('c:' + e.message); }", "try { yield 'F'; } catch (e) { L('c:' + e.message); return 'CR'; }"]) for (const calls of [
  "N(g, 1); L(S(g.return('R1'))); L(S(g.return('R2'))); N(g, 2);",
  "N(g, 1); L(S(g.return('R1'))); L(S(T(function () { return g.throw(new Error('T1')); }))); N(g, 2);",
  "N(g, 1); L(S(g.return('R1'))); L(S(g.next('N1'))); L(S(g.return('R2'))); N(g, 2);",
  "N(g, 1); L(S(T(function () { return g.throw(new Error('T0')); }))); L(S(T(function () { return g.throw(new Error('T1')); }))); N(g, 2);",
  "N(g, 1); L(S(g.return('R1'))); L(S(g.return('R2'))); L(S(g.return('R3'))); L(S(g.return('R4')));",
]) {
  add(`var g = (function* () { try { yield 1; } finally { ${fin} } })(); ${calls}`);
  add(`var g = (function* () { try { yield 1; throw new Error('B'); } finally { ${fin} } })(); ${calls}`);
  add(`var g = (function* () { try { yield 1; } catch (e) { L('outerC:' + e.message); yield 'C'; } finally { ${fin} } yield 'after'; })(); ${calls}`);
}

// ---- Dedup contra goldens vizinhos, execução no bun e gravação.
const existing = new Set(knownPrograms("generator_close_bun.tsv", (name) => /(gen|async|iter|close|destruct|spread|collection|promise)/.test(name) && name !== "generator_close_bun.tsv"));
// Amostra TARGET programas por hash (sampleByHash), do conjunto inteiro, antes de descontar os goldens vizinhos.
const TARGET = 4200;
const candidates = sampleByHash(programs.filter((source) => !usesHostApi(source.slice(PRELUDE.length))), TARGET).filter((source) => !existing.has(source));
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "gen-close-"));
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('unhandledRejection', () => {});\nprocess.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const runOnce = (file, source) =>
  new Promise((resolve) => {
    fs.writeFileSync(file, source);
    const child = spawn(process.execPath, ["--preload", preload, file], { cwd: dir, stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.on("data", (chunk) => { if (out.length < 1e6) out += chunk; });
    child.on("close", () => { clearTimeout(timer); resolve(out.split("\n").find((line) => line.startsWith("\u0001"))); });
  });
const results = new Array(candidates.length);
let dropped = 0;
const handle = async (index, file) => {
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  const { source, executable, meta } = prepareProgram(candidates[index]);
  const marked = await runOnce(file, executable);
  const brief = () => JSON.stringify(source.slice(PRELUDE.length)).slice(0, 160);
  if (!marked) { dropped++; process.stderr.write("sem resultado: " + brief() + "\n"); return; }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result) || result.includes(EN_DASH) || result.includes(EM_DASH)) {
    dropped++; process.stderr.write("caminho ou travessão no resultado: " + brief() + "\n"); return;
  }
  if ((await runOnce(file, executable)) !== marked) { dropped++; process.stderr.write("não determinístico: " + brief() + "\n"); return; }
  results[index] = { source, result, meta };
};
(async () => {
  let next = 0;
  const workers = Array.from({ length: 6 }, async (_, w) => {
    const file = path.join(dir, `case_${w}.js`);
    while (next < candidates.length) await handle(next++, file);
  });
  await Promise.all(workers);
  const rows = results.filter(Boolean);
  process.stdout.write(emitFactored("generator_close", rows));
  process.stderr.write(`candidatos ${candidates.length}, mantidos ${rows.length}, descartados ${dropped}\n`);
  fs.rmSync(dir, { recursive: true, force: true });
})();
