// Gera tests/golden/control_flow_more_bun.tsv: complemento combinatório de gen-control-flow-golden.js, medido no bun 1.4.2.
// Geradores (return/throw/next em cada ponto de suspensão, try/finally com yield, return, throw, break), delegação
// yield* (iterador interno com e sem return/throw, resultado não objeto), destructuring (ordem de avaliação, defaults,
// rest, aninhado, iterador que fecha), spread em chamada/array/objeto, labels, switch com fallthrough, for-of/for-in
// com break/continue em finally e getters em for-in. Programas que já estão em control_flow_bun.tsv são descartados.
// Mesmo formato e mesma execução (um processo bun por programa, harness de tests/golden/async_bun_harness.js).
// Uso: bun scripts/gen-control-flow-more-golden.js > tests/golden/control_flow_more_bun.tsv
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/async_bun_harness.js"), "utf8");
const existing = new Set(knownPrograms("control_flow_more_bun.tsv", ["control_flow_bun.tsv"]).map(program => JSON.stringify(program)));
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source) && !existing.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

// Fábrica de iterável instrumentado: `it(nome, opções)` registra iter/next/return no log.
const IT =
  "function it(n,o){o=o||{};var i=0;return {[Symbol.iterator](){L('iter:'+n);return this},next(){L('next:'+n+i);" +
  "if(o.nextThrow===i)throw new Error('nt');return i<(o.len===undefined?3:o.len)?{value:i++,done:false}:{value:'r',done:true}}," +
  "return:o.noRet?undefined:function(v){L('ret:'+n);if(o.retThrow)throw new Error('rt');return o.retBad?1:{done:true,value:v}}}}";
const wrap = body => `${IT} try { ${body} } catch (e) { L('c:' + (e && e.message)); }`;

// ---- 1. Geradores: método x ponto de suspensão x comportamento do finally.
const finallies = {
  plain: "L('f');",
  yield: "L('f'); yield 'fy'; L('f2');",
  ret: "L('f'); return 'fr';",
  throw: "L('f'); throw new Error('ft');",
  brk: "L('f'); break;",
  cont: "L('f'); continue;",
  yieldRet: "L('f'); yield 'fy'; return 'fr';",
};
const methods = {
  next: "g.next('a')",
  ret: "g.return('R')",
  thr: "g.throw(new Error('T'))",
};
for (const [fname, fbody] of Object.entries(finallies)) {
  for (const [mname, call] of Object.entries(methods)) {
    // suspenso no yield dentro do try
    const loopWrap = fname === "brk" || fname === "cont";
    const inner = `try { L('t'); var x = yield 1; L('x=' + x); yield 2; } finally { ${fbody} }`;
    const body = loopWrap ? `for (var k = 0; k < 2; k++) { ${inner} } L('after'); yield 'end';` : `${inner} L('after'); yield 'end';`;
    const drive = `var g = (function* () { ${body} })(); var r = []; r.push(g.next()); r.push(${call}); r.push(g.next()); r.push(g.next()); r.push(g.next()); L(JSON.stringify(r));`;
    add(`try { ${drive} } catch (e) { L('c:' + e.message); }`);
    // suspenso dentro do finally
    const drive2 = `var g = (function* () { try { L('t'); } finally { yield 'in-f'; L('f-after'); } yield 'tail'; })(); var r = []; r.push(g.next()); try { r.push(${call}); r.push(g.next()); r.push(g.next()); } catch (e) { r.push('c:' + e.message); } L(JSON.stringify(r));`;
    add(drive2);
    // try aninhado: catch captura throw, finally externo
    const drive3 = `var g = (function* () { try { try { yield 1; } catch (e) { L('ic:' + e.message); yield 'recov'; } finally { ${fbody} } } finally { L('outer'); } return 'done'; })(); var r = []; try { r.push(g.next()); r.push(${call}); r.push(g.next()); r.push(g.next()); } catch (e) { r.push('c:' + e.message); } L(JSON.stringify(r));`;
    if (!loopWrap) add(drive3);
  }
}
// return/throw/next em gerador não iniciado, em execução e terminado.
for (const m of ["return(5)", "throw(new Error('T'))", "next(5)", "return()", "return(undefined)"]) {
  add(`var g = (function* () { L('body'); try { yield 1; } finally { L('f'); } })(); try { L(JSON.stringify(g.${m})); } catch (e) { L('c:' + e.message); } L(JSON.stringify(g.next())); L(JSON.stringify(g.next()));`);
  add(`var g = (function* () { yield 1; })(); g.next(); g.next(); try { L(JSON.stringify(g.${m})); } catch (e) { L('c:' + e.message); } L(JSON.stringify(g.next()));`);
  add(`var g; g = (function* () { try { g.${m}; } catch (e) { L('c:' + e.name + ':' + e.message); } yield 1; })(); L(JSON.stringify(g.next()));`);
  add(`function* h() { try { yield 1; yield 2; } finally { L('f'); } } for (var v of h()) { L(v); var g2 = h(); g2.next(); try { g2.${m}; } catch (e) { L('c:' + e.message); } break; }`);
}
// return em laços dentro do gerador, finally que sobrescreve.
for (const [name, loop] of Object.entries({
  for: "for (var i = 0; i < 3; i++)",
  while: "while (true)",
  doWhile: "do",
  forOf: "for (var i of [1, 2, 3])",
  forIn: "for (var i in {a: 1, b: 2, c: 3})",
})) {
  const tail = name === "doWhile" ? " while (true);" : "";
  for (const mname of ["ret", "thr"]) {
    add(`var g = (function* () { var i = 0; try { ${loop} { try { yield i; } finally { L('fi' + i); } }${tail} } finally { L('fo'); } })(); L(JSON.stringify(g.next())); try { L(JSON.stringify(g.${mname === "ret" ? "return(9)" : "throw(new Error('T'))"})); } catch (e) { L('c:' + e.message); } L(JSON.stringify(g.next()));`);
    add(`var g = (function* () { var i = 0; ${loop} { try { yield i; } finally { L('fi' + i); ${mname === "ret" ? "return 'over'" : "break"}; } }${tail} yield 'out'; })(); L(JSON.stringify(g.next())); L(JSON.stringify(g.return('R'))); L(JSON.stringify(g.next()));`);
  }
}
// yield como expressão: precedência, argumento default, atribuição, em template, em chave computada.
for (const [i, e] of [
  "var a = yield; L(a)", "var a = (yield 1) + (yield 2); L(a)", "L(`t${yield 1}u${yield 2}`)", "var o = {[yield 1]: yield 2}; L(JSON.stringify(o))",
  "var [a, b = yield 'd'] = [1]; L(a + ',' + b)", "var {a = yield 'd'} = {}; L(a)", "L(yield yield 1)", "L((yield 1) ? 'y' : 'n')",
  "var a = [yield 1, yield 2]; L(JSON.stringify(a))", "L(f(yield 1, yield 2))", "if (yield 1) L('then'); else L('else')", "switch (yield 1) { case 'v': L('v'); break; default: L('d'); }",
  "for (var q = yield 1; q < 2; q++) L('q' + q)", "L(typeof (yield))", "L((yield 1, yield 2))", "x = yield* [1, 2]; L(x)", "L([...(yield* [7, 8])])", "L(JSON.stringify({...(yield 1)}))",
].entries()) {
  add(`function f(a, b) { return a + ':' + b; } var x; var g = (function* () { ${e}; })(); var r = []; for (var k = 0; k < 4; k++) { try { r.push(JSON.stringify(g.next('v' + k))); } catch (er) { r.push('c:' + er.message); } } L(r.join('|'));`);
  add(`function f(a, b) { return a + ':' + b; } var x; var g = (function* () { ${e}; })(); var r = []; r.push(JSON.stringify(g.next())); try { r.push(JSON.stringify(g.throw(new Error('T')))); } catch (er) { r.push('c:' + er.message); } r.push(JSON.stringify(g.next())); L(r.join('|'));`);
}

// ---- 2. yield*: iterador interno x operação x presença de return/throw.
const inners = {
  gen: "(function* () { try { L('in-start'); var a = yield 'i1'; L('in-a=' + a); yield 'i2'; return 'inret'; } finally { L('in-fin'); } })()",
  genCatch: "(function* () { try { yield 'i1'; } catch (e) { L('in-c:' + e.message); yield 'rec'; } return 'inret'; })()",
  genFinYield: "(function* () { try { yield 'i1'; } finally { yield 'fy'; L('in-f2'); } })()",
  arr: "[10, 20][Symbol.iterator]()",
  custom: "it('cu')",
  noRet: "it('nr', {noRet: true})",
  retBad: "it('rb', {retBad: true})",
  retThrow: "it('rt', {retThrow: true})",
  noThrow: "{[Symbol.iterator]() { return this; }, next(v) { L('n:' + v); return {value: 1, done: false}; }, return(v) { L('ret:' + v); return {done: true, value: 'rv'}; }}",
  withThrow: "{[Symbol.iterator]() { return this; }, next(v) { L('n:' + v); return {value: 1, done: false}; }, throw(e) { L('thr:' + e.message); return {done: true, value: 'tv'}; }, return(v) { L('ret:' + v); return {done: true, value: 'rv'}; }}",
  throwBad: "{[Symbol.iterator]() { return this; }, next(v) { return {value: 1, done: false}; }, throw(e) { return 1; }}",
  nextBad: "{[Symbol.iterator]() { return this; }, next(v) { return 1; }}",
  nextGetter: "{[Symbol.iterator]() { return this; }, next() { return {get done() { L('get done'); return false; }, get value() { L('get value'); return 5; }}; }}",
};
const ops = {
  next: "g.next('A')", ret: "g.return('R')", thr: "g.throw(new Error('T'))", none: "0",
};
for (const [iname, inner] of Object.entries(inners)) {
  for (const [oname, op] of Object.entries(ops)) {
    add(wrap(`var g = (function* () { var r = yield* ${inner}; L('r=' + r); return 'outer'; })(); var out = []; out.push(JSON.stringify(g.next('x'))); out.push(JSON.stringify(${op})); out.push(JSON.stringify(g.next('B'))); out.push(JSON.stringify(g.next('C'))); L(out.join('|'));`));
    add(wrap(`var g = (function* () { try { var r = yield* ${inner}; L('r=' + r); } finally { L('outer-fin'); } return 'outer'; })(); var out = []; out.push(JSON.stringify(g.next('x'))); out.push(JSON.stringify(${op})); out.push(JSON.stringify(g.next('B'))); L(out.join('|'));`));
  }
}
// yield* sobre não iteráveis, strings, Map, argumentos, objeto que lança em Symbol.iterator.
for (const [i, v] of [
  "1", "undefined", "null", "{}", "'ab'", "new Map([[1, 2]])", "new Set([3])", "(function () { return arguments; })(1, 2)", "{[Symbol.iterator]: 1}",
  "{[Symbol.iterator]() { throw new Error('si'); }}", "{[Symbol.iterator]() { return 1; }}", "{[Symbol.iterator]() { return {}; }}", "{[Symbol.iterator]() { return {next: 1}; }}",
  "[1, , 3]", "new Array(2)", "[][Symbol.iterator]()", "function* () {}", "(function* () { yield* [1]; yield* 'xy'; })()", "Object.create(null)", "new String('hi')",
].entries()) {
  add(`try { var g = (function* () { var r = yield* ${v}; return r; })(); var o = []; for (var k = 0; k < 4; k++) o.push(JSON.stringify(g.next())); L(o.join('|')); } catch (e) { L(e.name + ':' + e.message); }`);
}
// yield* encadeado e recursivo, ordem de finalização.
add(
  "function* a() { try { yield 1; yield 2; } finally { L('fa'); } } function* b() { try { yield* a(); } finally { L('fb'); } } function* c() { try { yield* b(); } finally { L('fc'); } } var g = c(); L(g.next().value); L(JSON.stringify(g.return('R'))); L(JSON.stringify(g.next()));",
  "function* a() { try { yield 1; } finally { L('fa'); throw new Error('A'); } } function* b() { try { yield* a(); } finally { L('fb'); } } var g = b(); g.next(); try { g.return('R'); } catch (e) { L('c:' + e.message); } L(JSON.stringify(g.next()));",
  "function* a() { try { yield 1; } catch (e) { L('ca:' + e.message); return 'ra'; } } function* b() { var r = yield* a(); L('r=' + r); yield 'b2'; } var g = b(); g.next(); L(JSON.stringify(g.throw(new Error('T')))); L(JSON.stringify(g.next()));",
  "function* tree(n) { if (n > 0) { yield* tree(n - 1); yield n; yield* tree(n - 1); } } L(JSON.stringify([...tree(3)]));",
  "function* fl(a) { for (var x of a) { if (Array.isArray(x)) yield* fl(x); else yield x; } } L(JSON.stringify([...fl([1, [2, [3, [4]], 5], [], [[6]]])]));",
  "function* g1() { var x = yield* g2(); L('x=' + x); return x + 1; } function* g2() { var y = yield 'a'; return y * 2; } var g = g1(); g.next(); L(JSON.stringify(g.next(21)));",
  "var it1 = {[Symbol.iterator]() { return this; }, next() { return {value: 1, done: false}; }, return() { L('ret'); return {}; }}; var g = (function* () { yield* it1; })(); g.next(); L(JSON.stringify(g.return(5))); L(JSON.stringify(g.return(6)));",
  "var it1 = {[Symbol.iterator]() { return this; }, next() { return {value: 1, done: false}; }, return() { L('ret'); return {done: false, value: 'keep'}; }}; var g = (function* () { yield* it1; })(); g.next(); L(JSON.stringify(g.return(5))); L(JSON.stringify(g.return(6)));",
  "var it1 = {[Symbol.iterator]() { return this; }, next() { return {value: 1, done: false}; }, return() { L('ret'); return undefined; }}; var g = (function* () { yield* it1; })(); g.next(); try { L(JSON.stringify(g.return(5))); } catch (e) { L(e.name + ':' + e.message); }",
  "var it1 = {[Symbol.iterator]() { return this; }, next() { return {value: 1, done: false}; }, get return() { L('get return'); return function () { return {done: true}; }; }}; var g = (function* () { yield* it1; })(); g.next(); g.return(1);",
  "var it1 = {[Symbol.iterator]() { return this; }, get next() { L('get next'); return function () { return {value: 1, done: false}; }; }}; var g = (function* () { yield* it1; })(); g.next(); g.next(); g.next();",
  "var g = (function* () { yield* [1, 2]; })(); Array.prototype[Symbol.iterator] = function* () { L('patched'); }; L(JSON.stringify(g.next()));"
);

// ---- 3. Destructuring: ordem, defaults, rest, aninhado, fechamento do iterador.
const patterns = {
  "[a]": 1, "[a, b]": 2, "[a, , b]": 3, "[, a]": 1, "[a, ...r]": 2, "[...r]": 0, "[]": 0, "[,]": 0, "[a = 'D']": 1, "[a, b = 'D']": 2, "[[a]]": 1,
  "[a, [b, c]]": 3, "[a, [b, ...c]]": 3, "[{x: a}]": 1, "[...[a, b]]": 2, "[...{length: n}]": 0,
};
const sources = {
  len0: "it('s', {len: 0})", len1: "it('s', {len: 1})", len3: "it('s')", len6: "it('s', {len: 6})", noRet: "it('s', {noRet: true})", retThrow: "it('s', {retThrow: true})",
  nextThrow: "it('s', {nextThrow: 1})",
};
for (const [pat, n] of Object.entries(patterns)) {
  for (const [sname, src] of Object.entries(sources)) {
    add(wrap(`var a, b, c, r, n; (${pat} = ${src}); L(JSON.stringify([a, b, c, r, n]));`));
  }
}
// forma declarativa (let/const/var) e em parâmetros.
for (const pat of ["[a, b]", "[a, ...r]", "[a = 1, b = 2]", "[, a]", "[[a], b]"]) {
  for (const decl of ["let", "const", "var"]) {
    for (const [sname, src] of [["len1", "it('s', {len: 1})"], ["len3", "it('s')"], ["retThrow", "it('s', {retThrow: true})"]]) {
      const names = pat.replace(/[^\w]/g, " ").split(/\s+/).filter(Boolean).filter(x => x !== "1" && x !== "2");
      add(wrap(`${decl} ${pat} = ${src}; L(JSON.stringify([${names.join(",")}]));`));
    }
  }
  add(wrap(`function f(${pat}) { L('body'); return 1; } f(it('p'));`));
  add(wrap(`function f(${pat}) { L('body'); } f(it('p', {len: 1}));`));
  add(wrap(`var f = (${pat}) => L('arrow'); f(it('p', {retThrow: true}));`));
  add(wrap(`for (var ${pat} of [it('o')]) L('iter'); `));
  add(wrap(`for (const ${pat} of [it('o', {len: 1})]) { L('body'); break; }`));
  add(wrap(`try { throw it('t'); } catch (${pat}) { L('caught'); }`));
}
// Ordem de avaliação: alvo, valor, default, chave computada.
add(
  "var o = {}; function k(n) { L('k' + n); return n; } function v(n) { L('v' + n); return n; } var src = {a: 1, b: undefined}; ({[k('a')]: o[k('x')] = v('d1'), [k('b')]: o[k('y')] = v('d2')} = src); L(JSON.stringify(o));",
  "function t(n) { L('t' + n); return {set p(v) { L('set' + n + '=' + v); }}; } [t(1).p, t(2).p] = [10, 20];",
  "function t(n) { L('t' + n); return {set p(v) { L('set' + n + '=' + v); }}; } var src = {get a() { L('get a'); return 1; }, get b() { L('get b'); return 2; }}; ({a: t(1).p, b: t(2).p} = src);",
  "var log2 = []; var src = {get a() { L('get a'); return undefined; }}; var {a = (L('def'), 5), b = (L('def b'), 6)} = src; L(a + ',' + b);",
  "var src = {get a() { L('get a'); return {get b() { L('get b'); return 7; }}; }}; var {a: {b}} = src; L(b);",
  "var {a, ...rest} = {get a() { L('get a'); return 1; }, get b() { L('get b'); return 2; }, c: 3}; L(JSON.stringify(rest));",
  "var key = {toString() { L('key'); return 'a'; }}; var {[key]: v1, ...rest} = {a: 1, b: 2}; L(JSON.stringify([v1, rest]));",
  "var {a = L('d1'), b = L('d2')} = {a: 1}; L('done');",
  "var [a = L('d1'), b = L('d2')] = [1]; L('done');",
  "var [a = L('d1'), b = L('d2')] = [undefined, undefined]; L(JSON.stringify([a, b]));",
  "var [a = L('d1'), b = a] = []; L(JSON.stringify([a, b]));",
  "try { var [a = b, b = 1] = []; } catch (e) { L(e.name + ':' + e.message); }",
  "try { let [a = b, b = 1] = []; } catch (e) { L(e.name + ':' + e.message); }",
  "try { let {a = a} = {}; } catch (e) { L(e.name + ':' + e.message); }",
  "try { let {a = 1, b = a + 1} = {}; L(a + ',' + b); } catch (e) { L(e.name + ':' + e.message); }",
  "var {} = 1; var [] = []; L('ok');",
  "try { var {} = null; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var {} = undefined; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var [] = {}; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var [a] = null; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var {a} = null; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var {a: {b}} = {}; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var {a: [b]} = {a: 1}; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var [[a]] = [null]; } catch (e) { L(e.name + ':' + e.message); }",
  "try { (function ({a}) {})(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { (function ([a]) {})(); } catch (e) { L(e.name + ':' + e.message); }",
  "try { (({a}) => a)(null); } catch (e) { L(e.name + ':' + e.message); }",
  "var a = 1, b = 2; [a, b] = [b, a]; L(a + ',' + b);",
  "var a = [1, 2, 3]; var i = 0; [a[i++], a[i++]] = [a[1], a[0]]; L(JSON.stringify([a, i]));",
  "var o = {}; [o.a, o['b'], ...o.c] = [1, 2, 3, 4]; L(JSON.stringify(o));",
  "var o = {}; ({a: o.x, ...o.rest} = {a: 1, b: 2, c: 3}); L(JSON.stringify(o));",
  "var r; ({a: r} = {a: 1}); L(r); var x; [x] = [2]; L(x);",
  "var a; L(([a] = [1, 2]).length); L(({a} = {a: 5}).a);",
  "var a, b; L(JSON.stringify([a, b] = 'xy')); L(a + b);",
  "var a, b; [a, b] = new Set([1, 2]); L(a + b); [a, b] = new Map([[1, 2], [3, 4]]); L(JSON.stringify([a, b]));",
  "var [a, b] = 'é😀'; L(a + '|' + b);",
  "var [a, ...b] = '😀x'; L(JSON.stringify([a, b]));",
  "var {length, 0: first, [1]: second} = 'xyz'; L(JSON.stringify([length, first, second]));",
  "var {a, b: {c = 3} = {}} = {a: 1}; L(JSON.stringify([a, c]));",
  "var {a: {b: {c}}} = {a: {b: {c: 1}}}; L(c);",
  "var [[[x]]] = [[[9]]]; L(x);",
  "var {0: a, 1: b, length} = [7, 8]; L(JSON.stringify([a, b, length]));",
  "var {a, a: b} = {a: 1}; L(a + ',' + b);",
  "var {__proto__: p} = {}; L(p === Object.prototype); var {['__proto__']: q} = {}; L(q === Object.prototype);",
  "var {x, ...rest} = Object.create({inherited: 1}, {own: {value: 2, enumerable: true}, hid: {value: 3}}); L(JSON.stringify([x, rest]));",
  "var s = Symbol('s'); var {[s]: v, ...rest} = {[s]: 1, a: 2, [Symbol('t')]: 3}; L(v + ',' + Object.getOwnPropertySymbols(rest).length + ',' + rest.a);",
  "var {...r} = 'ab'; L(JSON.stringify(r));",
  "var {...r} = [1, 2]; L(JSON.stringify(r));",
  "var {...r} = 5; L(JSON.stringify(r));",
  "var {...r} = new Proxy({a: 1, b: 2}, {ownKeys(t) { L('ownKeys'); return Reflect.ownKeys(t); }, getOwnPropertyDescriptor(t, k) { L('gopd:' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }, get(t, k) { L('get:' + String(k)); return t[k]; }}); L(JSON.stringify(r));",
  "var {a, ...r} = new Proxy({a: 1, b: 2}, {get(t, k) { L('get:' + String(k)); return t[k]; }, ownKeys(t) { L('ownKeys'); return Reflect.ownKeys(t); }}); L(JSON.stringify(r));",
  "var [a, b] = new Proxy([1, 2], {get(t, k) { L('get:' + String(k)); return t[k]; }}); L(a + b);",
  "var g = (function* () { try { yield 1; yield 2; yield 3; } finally { L('closed'); } })(); var [a, b] = g; L(a + b); L(JSON.stringify(g.next()));",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var [a, b, c] = g; L(JSON.stringify([a, b, c]));",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var [...all] = g; L(JSON.stringify(all));",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var [a, , ] = g; L(a);",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var [, , ] = g; L('x');",
  "var g = (function* () { try { yield 1; } finally { L('closed'); throw new Error('F'); } })(); try { var [a] = g; } catch (e) { L('c:' + e.message); }",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); try { var [a = (() => { throw new Error('D'); })(), b] = [undefined, 0]; } catch (e) { L('c:' + e.message); }",
  "var g = (function* () { try { yield undefined; yield 2; } finally { L('closed'); } })(); try { var [a = (() => { throw new Error('D'); })(), b] = g; } catch (e) { L('c:' + e.message); }",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); try { var [{x}] = g; } catch (e) { L('c:' + e.name); }",
  "var g = (function* () { try { yield null; yield 2; } finally { L('closed'); } })(); try { var [{x}] = g; } catch (e) { L('c:' + e.name); }",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var o = {set p(v) { L('set'); throw new Error('S'); }}; try { [o.p] = g; } catch (e) { L('c:' + e.message); }",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); var o = {}; try { [o.a.b] = g; } catch (e) { L('c:' + e.name); }",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('closed'); } })(); try { [(L('target'), {}).p] = g; } catch (e) { L('c:' + e.name); } L('end');",
  IT + "var a, b; var it1 = it('s', {len: 1}); [a, b] = it1; L(JSON.stringify([a, b]));",
  IT + "var a, b; var it1 = it('s', {len: 5}); [a, ...b] = it1; L(JSON.stringify([a, b]));",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: true}; }, return() { L('ret'); return {}; }}; }}; var [a, b] = ex; L('ok');",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, value: 1}; }, return() { L('ret'); return {}; }}; }}; var [a, b] = ex; L('ok');",
  "var ex = {[Symbol.iterator]() { return {next() { throw new Error('N'); }, return() { L('ret'); return {}; }}; }}; try { var [a] = ex; } catch (e) { L('c:' + e.message); }",
  "var ex = {[Symbol.iterator]() { return {next() { return {get done() { throw new Error('D'); }}; }, return() { L('ret'); return {}; }}; }}; try { var [a] = ex; } catch (e) { L('c:' + e.message); }",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, get value() { throw new Error('V'); }}; }, return() { L('ret'); return {}; }}; }}; try { var [a] = ex; } catch (e) { L('c:' + e.message); }",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, value: 1}; }, return() { L('ret'); return 5; }}; }}; try { var [a] = ex; } catch (e) { L(e.name + ':' + e.message); }",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, value: 1}; }, return: 5}; }}; try { var [a] = ex; } catch (e) { L(e.name + ':' + e.message); }",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, value: 1}; }, return: null}; }}; var [a] = ex; L('ok');",
  "var ex = {[Symbol.iterator]() { return {next() { return {done: false, value: 1}; }, get return() { L('get return'); return undefined; }}; }}; var [a] = ex; L('ok');"
);

// ---- 4. Spread.
const spreadSrc = {
  arr: "[1, 2, 3]", holes: "[1, , 3]", str: "'ab'", surr: "'😀a'", set: "new Set([1, 1, 2])", map: "new Map([[1, 'a']])", gen: "(function* () { try { yield 1; yield 2; } finally { L('gfin'); } })()",
  custom: "it('sp')", args: "(function () { return arguments; })(1, 2)", ta: "new Uint8Array([5, 6])", empty: "[]", nested: "[[1], [2]]",
  getterIt: "{get [Symbol.iterator]() { L('get iter'); return function* () { yield 1; }; }}", nextThrow: "it('sp', {nextThrow: 1})",
};
for (const [name, src] of Object.entries(spreadSrc)) {
  add(wrap(`L(JSON.stringify([...${src}]));`));
  add(wrap(`L(JSON.stringify([0, ...${src}, 9]));`));
  add(wrap(`function f() { return arguments.length + ':' + JSON.stringify([].slice.call(arguments)); } L(f(...${src}));`));
  add(wrap(`function f() { return arguments.length; } L(f(1, ...${src}, ...${src}));`));
  add(wrap(`function F() { this.n = arguments.length; } L(new F(...${src}).n);`));
  add(wrap(`L(Math.max(...${src}.constructor === Object ? [] : ${src}));`));
  add(wrap(`var o = {...${src}}; L(JSON.stringify(o));`));
  add(wrap(`L(JSON.stringify(Array.of(...${src})));`));
  add(wrap(`var [first, ...rest] = [...${src}]; L(JSON.stringify([first, rest]));`));
}
add(
  "var a = [1, 2]; var r = [...a, a.push(3), ...a]; L(JSON.stringify(r));",
  "var a = [1, 2]; function f() { return arguments.length; } L(f(...a, a.push(9)));",
  "var a = [1, 2]; var it1 = {[Symbol.iterator]() { var i = 0; return {next() { if (i === 0) a.push('late'); return i < a.length ? {value: a[i++], done: false} : {done: true}; }}; }}; L(JSON.stringify([...it1]));",
  "function f(a, b, c) { return JSON.stringify([a, b, c]); } L(f(...[1], ...[2, 3]));",
  "function f(a, b, c) { return JSON.stringify([a, b, c]); } L(f(...[], 1, ...[2], ...[], 3));",
  "function f(...r) { return r.length; } L(f(...new Array(5)));",
  "function f(...r) { return JSON.stringify(r); } L(f(...[1, , 3]));",
  "var o = {f(...a) { return [this === o, a.length]; }}; L(JSON.stringify(o.f(...[1, 2])));",
  "var o = {f(...a) { return [this === o, a.length]; }}; L(JSON.stringify(o['f'](...[1, 2], ...[3])));",
  "var o = {f(...a) { return this === o; }}; L(o?.f(...[1]));",
  "function f() { return new.target === undefined; } L(f(...[]));",
  "class A { constructor(...a) { this.a = a; } } class B extends A { constructor(...a) { super(...a, 'x'); } } L(JSON.stringify(new B(1, 2).a));",
  "class A { constructor(...a) { this.a = a; } } L(JSON.stringify(Reflect.construct(A, [...'ab']).a));",
  "function f() { return arguments.length; } L(f.apply(null, [...'abc']));",
  "function f(a, b = 2, ...c) { return f.length + ':' + JSON.stringify([a, b, c]); } L(f(...[1, undefined, 3, 4]));",
  "try { (function () {})(...undefined); } catch (e) { L(e.name + ':' + e.message); }",
  "try { (function () {})(...null); } catch (e) { L(e.name + ':' + e.message); }",
  "try { (function () {})(...1); } catch (e) { L(e.name + ':' + e.message); }",
  "try { (function () {})(...{}); } catch (e) { L(e.name + ':' + e.message); }",
  "try { [...undefined]; } catch (e) { L(e.name + ':' + e.message); }",
  "try { [...{}]; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var x = {}; [...x.y]; } catch (e) { L(e.name + ':' + e.message); }",
  "try { var o = {a: 1}; o.f(...[1]); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new (function () {})(...5); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new 5(...[]); } catch (e) { L(e.name + ':' + e.message); }",
  "L(JSON.stringify({...null, ...undefined, ...1, ...true, ...'hi', ...[7]}));",
  "L(JSON.stringify({...{a: 1}, ...{a: 2, b: 3}, a: 4, ...{c: 5}}));",
  "L(JSON.stringify({a: 0, ...{a: 1}, a: 2}));",
  "var o = {...{get a() { L('get a'); return 1; }}}; L(Object.getOwnPropertyDescriptor(o, 'a').get === undefined);",
  "var s = Symbol('s'); var o = {...{[s]: 1, a: 2}}; L(JSON.stringify([o[s], o.a, Object.getOwnPropertySymbols(o).length]));",
  "var o = {...Object.create({inh: 1}, {own: {value: 2, enumerable: true}, hid: {value: 3, enumerable: false}})}; L(JSON.stringify(o));",
  "var o = {...{__proto__: {p: 1}, a: 1}}; L(JSON.stringify([o.p, Object.getPrototypeOf(o) === Object.prototype]));",
  "var o = {...JSON.parse('{\"__proto__\": 5, \"b\": 1}')}; L(JSON.stringify([Object.keys(o), Object.getPrototypeOf(o) === Object.prototype]));",
  "var o = {set a(v) { L('setter'); }, ...{a: 1}}; L(JSON.stringify(Object.getOwnPropertyDescriptor(o, 'a')));",
  "Object.defineProperty(Object.prototype, 'zz', {set(v) { L('proto setter'); }, configurable: true}); var o = {...{zz: 1}}; L(JSON.stringify(Object.keys(o))); delete Object.prototype.zz;",
  "var src = new Proxy({a: 1, b: 2}, {ownKeys(t) { L('ownKeys'); return ['b', 'a']; }, getOwnPropertyDescriptor(t, k) { L('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k); }, get(t, k) { L('get:' + k); return t[k]; }}); L(JSON.stringify({...src}));",
  "var src = {get a() { L('get a'); delete this.b; return 1; }, b: 2, c: 3}; L(JSON.stringify({...src}));",
  "var src = {get a() { L('get a'); Object.defineProperty(this, 'b', {enumerable: false}); return 1; }, b: 2}; L(JSON.stringify({...src}));",
  "var src = {a: 1, get b() { throw new Error('G'); }, c: 3}; try { var o = {...src}; } catch (e) { L('c:' + e.message); }",
  "var n = 0; var o = {[n++]: n++, ...{[n++]: n++}, [n++]: n++}; L(JSON.stringify([o, n]));",
  "var o = {a: 1}; var p = {...o, a: 2}; L(JSON.stringify([o, p]));",
  "var o = {x: {y: 1}}; var p = {...o}; p.x.y = 2; L(o.x.y);",
  "L(JSON.stringify({...[1, 2], length: 5}));",
  "L(JSON.stringify([...'ab', ...[1], ...new Set([2])].map(x => typeof x)));",
  "L(JSON.stringify([...[...[...[1, 2]]]]));",
  "L(JSON.stringify([..., ].length));".replace("..., ", "...[], ")
);
// Chamada com spread: this, ordem de avaliação do callee e dos argumentos.
add(
  "function c() { L('callee'); return function () { return arguments.length; }; } function a(n) { L('a' + n); return [n]; } L(c()(...a(1), ...a(2)));",
  "var o = {get f() { L('get f'); return function () { return 1; }; }}; function a() { L('args'); return []; } o.f(...a());",
  "var o = null; function a() { L('args'); return []; } try { o.f(...a()); } catch (e) { L('c:' + e.name); }",
  "var o = {}; function a() { L('args'); return []; } try { o.f(...a()); } catch (e) { L('c:' + e.message); }",
  "var f; function a() { L('args'); return []; } try { f(...a()); } catch (e) { L('c:' + e.name + ':' + e.message); }",
  "function a() { L('args'); throw new Error('A'); } var o = {}; try { o.f(...a()); } catch (e) { L('c:' + e.message); }"
);

// ---- 5. Labels e switch com fallthrough.
const swBodies = [
  "case 1: L('1'); case 2: L('2'); break; case 3: L('3'); default: L('d');",
  "case 1: L('1'); default: L('d'); case 2: L('2'); break; case 3: L('3');",
  "default: L('d'); case 1: L('1'); break; case 2: L('2');",
  "case 1: case 2: L('12'); break; case 3: L('3'); case 4: L('4');",
  "case 1: { L('1'); break; } case 2: { L('2'); } default: { L('d'); }",
  "case 1: L('1'); continue; case 2: L('2'); break; default: L('d'); continue;",
  "case 1: try { L('t1'); break; } finally { L('f1'); } case 2: L('2'); break; default: L('d');",
  "case 1: try { L('t1'); } finally { L('f1'); } case 2: L('2'); break; default: L('d');",
  "case 1: try { throw 1; } catch (e) { L('c1'); } case 2: L('2'); break; default: L('d');",
  "case 1: L('1'); case 2: L('2'); return; default: L('d');",
];
for (const [i, body] of swBodies.entries()) {
  for (const val of [0, 1, 2, 3, 4]) {
    const useContinue = /continue/.test(body);
    add(`function f(x) { ${useContinue ? "for (var k = 0; k < 1; k++) " : ""}switch (x) { ${body} } L('after'); } f(${val}); L('end');`);
  }
}
add(
  "function f(x) { switch (x) { case L('c1'), 1: L('b1'); case L('c2'), 2: L('b2'); break; default: L('d'); case L('c3'), 3: L('b3'); } } f(2); f(5);",
  "function f(x) { switch (L('disc'), x) { case 1: case 2: case 3: L('n'); } } f(3); f(9);",
  "function f(x) { switch (x) { case (L('a'), 1): break; case (L('b'), 2): break; default: L('d'); case (L('c'), 3): L('3'); } } f(3); f(7);",
  "var calls = 0; function f(x) { switch (x) { case calls++: L('first'); break; case calls++: L('second'); break; default: L('d'); } } f(1); L(calls);",
  "switch (NaN) { case NaN: L('nan'); break; default: L('d'); }",
  "switch (0) { case -0: L('negzero'); break; default: L('d'); }",
  "switch ('1') { case 1: L('num'); break; case '1': L('str'); break; }",
  "switch (null) { case undefined: L('u'); break; case null: L('n'); break; }",
  "var o = {}; switch (o) { case {}: L('new'); break; case o: L('same'); }",
  "switch (1) { }; L('empty');",
  "switch (1) { default: }; L('only default');",
  "switch (3) { case 1: let a = 1; case 3: try { L(a); } catch (e) { L(e.name); } }",
  "switch (1) { case 1: let a = 1; L(a); break; case 2: let b = 2; } L(typeof b);",
  "try { switch (1) { case 1: let a; case 2: let a; } } catch (e) { L(e.name); }".replace("let a; case 2: let a;", "L(1);"),
  "switch (1) { case 1: function f() { return 'hoisted'; } L(f()); case 2: L(f()); }",
  "switch (2) { case 1: function g() { return 1; } } L(typeof g);",
  "var r = []; for (var i = 0; i < 4; i++) { switch (i) { case 0: continue; case 1: r.push('one'); break; case 2: r.push('two'); default: r.push('dflt'); } r.push('e' + i); } L(JSON.stringify(r));",
  "var r = []; out: for (var i = 0; i < 3; i++) { switch (i) { case 1: continue out; case 2: break out; } r.push(i); } L(JSON.stringify(r));",
  "var r = []; for (var i = 0; i < 3; i++) { switch (i) { case 1: break; } r.push(i); } L(JSON.stringify(r));",
  "var r = eval('switch (1) { case 1: 5; break; }'); L(r);",
  "L(eval('switch (1) { case 1: 5; case 2: 6; }'));",
  "L(eval('switch (1) { case 1: 5; case 2: }'));",
  "L(eval('1; switch (3) { case 1: 5; }'));",
  "L(eval('1; switch (1) { case 1: }'));",
  "L(eval('1; switch (1) { case 1: break; }'));",
  "L(eval('2; do { 3; break; } while (false)'));",
  "L(eval('a: { 1; break a; }'));",
  "L(eval('1; a: { break a; }'));",
  "L(eval('1; for (var i = 0; i < 2; i++) { if (i) break; 5; }'));",
  "L(eval('1; for (var i = 0; i < 2; i++) { 5; continue; }'));",
  "L(eval('1; try { 2; } finally { 3; }'));",
  "L(eval('1; try { 2; throw 0; } catch (e) { 3; } finally { 4; }'));",
  "L(eval('1; if (false) 2;'));",
  "L(eval('1; if (true) ;'));",
  "L(eval('1; while (false);'));",
  "L(eval('1; var x = 2;'));",
  "L(eval('1; function f() {}'));",
  "L(eval('1; {}'));",
  "L(eval('1; l: ;'));"
);
// Labels.
const labelBodies = [
  "a: for (var i = 0; i < 3; i++) { b: for (var j = 0; j < 3; j++) { if (j === 1) continue a; if (i === 2) break a; L(i + '' + j); } }",
  "a: for (var i = 0; i < 3; i++) { b: for (var j = 0; j < 3; j++) { if (j === 1) continue b; if (i === 1) break b; L(i + '' + j); } L('o' + i); }",
  "a: { L('1'); b: { L('2'); break a; L('x'); } L('y'); } L('3');",
  "a: { L('1'); b: { L('2'); break b; L('x'); } L('y'); } L('3');",
  "a: b: c: for (var i = 0; i < 2; i++) { if (i) break b; L(i); continue c; }",
  "a: if (true) { L('in'); break a; L('x'); } L('after');",
  "a: try { L('t'); break a; } finally { L('f'); } L('after');",
  "a: try { L('t'); throw 1; } catch (e) { L('c'); break a; } finally { L('f'); } L('after');",
  "a: for (var i of [1, 2, 3]) { try { if (i === 2) continue a; L(i); } finally { L('f' + i); } }",
  "a: for (var i in {x: 1, y: 2, z: 3}) { try { if (i === 'y') break a; L(i); } finally { L('f' + i); } } L('after');",
  "a: while (true) { do { L('d'); break a; } while (true); }",
  "a: do { L('d'); continue a; } while (false); L('after');",
  "a: for (;;) { try { try { break a; } finally { L('f1'); } } finally { L('f2'); } }",
  "a: for (var i = 0; i < 2; i++) { try { continue a; } finally { L('f' + i); } }",
  "a: switch (1) { case 1: L('1'); break a; case 2: L('2'); } L('after');",
  "a: switch (1) { case 1: for (;;) { break a; } } L('after');",
  "a: { b: { c: { break a; } L('x'); } L('y'); } L('z');",
  "a: for (var i = 0; i < 2; i++) b: for (var j = 0; j < 2; j++) { if (j) continue a; L(i + '' + j); }",
  "a: for (var i = 0; i < 2; i++) { (function () { b: for (;;) { break b; } })(); L(i); }",
  "a: { (() => { a: { L('inner'); break a; } L('inner2'); })(); L('outer'); }",
  "a: a1: for (var i = 0; i < 2; i++) { continue a; } L('done');",
  "a: { var f = function () { return 1; }; L(f()); } L('x');",
  "a: function f() { return 'lf'; } L(f());",
  "a: let1: for (var i = 0; i < 1; i++) L('let1');",
  "yield: for (var i = 0; i < 1; i++) { L('yield label'); break yield; }",
  "await: for (var i = 0; i < 1; i++) { L('await label'); continue await; }",
  "async: for (var i = 0; i < 1; i++) { L('async label'); break async; }",
  "of: for (var i = 0; i < 1; i++) { L('of'); break of; }",
  "var x = 0; a: while (x < 3) { x++; try { if (x === 2) continue a; L('x' + x); } finally { L('f' + x); } }",
  "var i = 0; a: do { i++; if (i < 3) continue a; L('i' + i); } while (i < 3);",
  "a: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { try { if (j === 1) continue a; } finally { L('f' + i + j); } } }",
  "a: for (let i = 0; i < 2; i++) { setTimeout; var fn = () => i; L(fn()); continue a; }",
  "a: for (const x of [1, 2]) { b: for (const y of [3, 4]) { if (y === 4) continue a; L(x * y); } }",
];
for (const [i, b] of labelBodies.entries()) add(b);
for (const bad of [
  "a: a: ;", "a: { a: ; }", "break;", "continue;", "break a;", "a: { continue a; }", "a: for (;;) { (function () { break a; })(); }", "a: for (;;) { (() => { continue a; })(); }",
  "do { break b; } while (0);", "a: { b: ; break b; }", "'use strict'; yield: ;", "'use strict'; let: ;", "if (1) function f() {} else ;", "a: let\nx = 1;", "a: class C {}",
  "a: const c = 1;", "a: let z = 1;", "while (1) function f() {}", "a: async function f() {}", "a: function* g() {}", "'use strict'; a: function f() {}", "for (;;) { break\na; }",
]) {
  add(`try { eval(${JSON.stringify(bad)}); L('ok'); } catch (e) { L(e.name + ':' + e.message); }`);
}

// ---- 6. for-of / for-in com break/continue em finally e fechamento de iterador.
const loopKinds = {
  of: (body, src) => `for (var v of ${src || "it('lo', {len: 3})"}) { ${body} }`,
  ofLet: (body, src) => `for (let v of ${src || "it('lo', {len: 3})"}) { ${body} }`,
  ofGen: body => `for (var v of (function* () { try { yield 0; yield 1; yield 2; } finally { L('gfin'); } })()) { ${body} }`,
  in: body => `for (var v in {p: 1, q: 2, r: 3}) { ${body} }`,
  inLet: body => `for (let v in {p: 1, q: 2, r: 3}) { ${body} }`,
};
const finBodies = {
  brkInFin: "try { L('t' + v); } finally { L('f' + v); break; }",
  contInFin: "try { L('t' + v); } finally { L('f' + v); continue; } L('unreached');",
  brkInTry: "try { L('t' + v); break; } finally { L('f' + v); }",
  contInTry: "try { L('t' + v); continue; } finally { L('f' + v); } L('unreached');",
  contFinBrkTry: "try { L('t' + v); if (v == 1 || v == 'q') break; } finally { L('f' + v); continue; }",
  brkFinOverThrow: "try { throw new Error('E' + v); } finally { L('f' + v); break; }",
  contFinOverThrow: "try { throw new Error('E' + v); } finally { L('f' + v); continue; }",
  contFinOverThrowCatch: "try { try { throw new Error('E' + v); } finally { L('f' + v); continue; } } catch (e) { L('never'); }",
  thrInFinOverBrk: "try { L('t' + v); break; } finally { L('f' + v); throw new Error('F' + v); }",
  thrInFinOverCont: "try { L('t' + v); continue; } finally { L('f' + v); throw new Error('F' + v); }",
  retInFin: "try { L('t' + v); } finally { L('f' + v); (function () { })(); }",
  nestedFin: "try { try { L('t' + v); break; } finally { L('f1' + v); } } finally { L('f2' + v); }",
  nestedFinCont: "try { try { L('t' + v); continue; } finally { L('f1' + v); } } finally { L('f2' + v); }",
  nestedFinBrk2: "try { try { L('t' + v); break; } finally { L('f1' + v); continue; } } finally { L('f2' + v); }",
  catchBrk: "try { throw 1; } catch (e) { L('c' + v); break; } finally { L('f' + v); }",
  catchCont: "try { throw 1; } catch (e) { L('c' + v); continue; } finally { L('f' + v); }",
  catchThrowFinCont: "try { throw 1; } catch (e) { L('c' + v); throw new Error('C'); } finally { L('f' + v); continue; }",
  labelBrk: "o: { try { L('t' + v); break o; } finally { L('f' + v); } L('unreached'); } L('lab' + v);",
};
for (const [lname, mk] of Object.entries(loopKinds)) {
  for (const [bname, body] of Object.entries(finBodies)) {
    add(wrap(`${mk(body)} L('end');`));
  }
}
// for-of com labels, retorno de função, throw no corpo: o iterador fecha?
for (const [what, stmt] of Object.entries({
  ret: "return 'R'", thr: "throw new Error('B')", brk: "break", cont: "continue", labBrk: "break lab", labCont: "continue lab",
})) {
  for (const [sname, src] of Object.entries({ plain: "it('x')", noRet: "it('x', {noRet: true})", retThrow: "it('x', {retThrow: true})", retBad: "it('x', {retBad: true})" })) {
    add(wrap(`function f() { lab: for (var a of [0]) { for (var v of ${src}) { if (v === 1) { ${stmt}; } L('v' + v); } } return 'end'; } L(f());`));
  }
}
// for-of: mutação da coleção durante a iteração, iteradores reutilizados, valor retornado do next, destructuring no cabeçalho.
add(
  "var a = [1, 2, 3]; for (var x of a) { L(x); if (x === 1) a.push(4); }",
  "var a = [1, 2, 3]; for (var x of a) { L(x); a.length = 1; }",
  "var a = [1, 2, 3]; for (var x of a) { L(x); if (x === 1) a.shift(); }",
  "var s = new Set([1, 2]); for (var x of s) { L(x); if (x < 4) s.add(x + 2); }",
  "var s = new Set([1, 2, 3]); for (var x of s) { L(x); s.delete(2); }",
  "var m = new Map([[1, 'a']]); for (var [k, v] of m) { L(k + v); if (k < 3) m.set(k + 1, 'z'); }",
  "var m = new Map([[1, 'a'], [2, 'b']]); for (var [k] of m) { L(k); m.clear(); m.set(9, 'n'); }",
  "var i1 = [1, 2, 3][Symbol.iterator](); for (var x of i1) { L(x); break; } for (var x of i1) L('again' + x);",
  "var g = (function* () { yield 1; yield 2; yield 3; })(); for (var x of g) { L(x); break; } for (var x of g) L('again' + x); L(JSON.stringify(g.next()));",
  "var g = (function* () { try { yield 1; yield 2; } finally { L('gfin'); } })(); for (var x of g) { L(x); continue; }",
  "for (var [a, b] of [[1, 2], [3, 4]]) L(a + b);",
  "for (var {a, b = 'D'} of [{a: 1}, {a: 2, b: 3}]) L(a + b);",
  "for (var [a, ...r] of ['xy', 'z']) L(JSON.stringify([a, r]));",
  IT + "for (let [a, b] of [it('p', {len: 1}), it('q', {len: 5})]) { L('body'); }",
  "try { for (var [a] of [null]) ; } catch (e) { L(e.name); }",
  "try { for (var {a} of [null]) ; } catch (e) { L(e.name); }",
  "var o = {}; for (o.p of [1, 2]) L(o.p); for ([o.q] of [[3]]) L(o.q); for ({z: o.r} of [{z: 4}]) L(o.r);",
  "var o = {set p(v) { L('set' + v); }}; for (o.p of [1, 2]) ;",
  "var fns = []; for (let x of [1, 2, 3]) fns.push(() => x); L(JSON.stringify(fns.map(f => f())));",
  "var fns = []; for (var x of [1, 2, 3]) fns.push(() => x); L(JSON.stringify(fns.map(f => f())));",
  "var fns = []; for (const k in {a: 1, b: 2}) fns.push(() => k); L(JSON.stringify(fns.map(f => f())));",
  "var fns = []; for (let i = 0; i < 3; i++) { fns.push(() => i); i += 0; } L(JSON.stringify(fns.map(f => f())));",
  "var fns = []; for (let i = 0; i < 3; fns.push(() => i), i++); L(JSON.stringify(fns.map(f => f())));",
  "for (let x of [1, (L('eval'), 2)]) { L(x); }",
  "try { for (let x of [x]) ; } catch (e) { L(e.name + ':' + e.message); }",
  "try { for (let x in {[x]: 1}) ; } catch (e) { L(e.name + ':' + e.message); }",
  "try { for (const x of [1]) { x = 2; } } catch (e) { L(e.name + ':' + e.message); }",
  "for (var x of []) L('never'); L(typeof x);",
  "for (var x of 'ab') L(x);",
  "for (var x of (function () { return arguments; })(1, 2)) L(x);",
  "for (var x of new Uint8Array([1, 2])) L(x);",
  "try { for (var x of 5) ; } catch (e) { L(e.name + ':' + e.message); }",
  "try { for (var x of {}) ; } catch (e) { L(e.name + ':' + e.message); }",
  "try { for (var x of undefined) ; } catch (e) { L(e.name + ':' + e.message); }",
  "var r = eval('for (var x of [1, 2]) { x; }'); L(r);",
  "var r = eval('1; for (var x of []) { x; }'); L(r);",
  "var r = eval('for (var x of [1, 2]) { if (x == 2) break; x; }'); L(r);",
  "var r = eval('for (var x of [1, 2]) { 5; break; }'); L(r);"
);
// for-in: ordem, getters, mutação durante a enumeração, protótipo, shadowing, tipos.
add(
  "var o = {b: 1, a: 2, 2: 'x', 1: 'y', [Symbol('s')]: 3}; for (var k in o) L(k);",
  "var o = {get a() { L('get a'); return 1; }, get b() { L('get b'); return 2; }}; for (var k in o) L('k:' + k);",
  "var o = {get a() { L('get a'); return 1; }, b: 2}; for (var k in o) { L('k:' + k + ' v:' + o[k]); }",
  "var o = {a: 1, b: 2, c: 3}; for (var k in o) { L(k); delete o.b; }",
  "var o = {a: 1, b: 2, c: 3}; for (var k in o) { L(k); delete o.c; o.d = 4; }",
  "var o = {a: 1, b: 2}; for (var k in o) { L(k); o[k + 'x'] = 1; }",
  "var o = {a: 1, b: 2}; for (var k in o) { L(k); delete o.b; o.b = 5; }",
  "var o = {a: 1, get b() { L('get b'); delete this.c; return 2; }, c: 3}; for (var k in o) { L(k); var tmp = o[k]; }",
  "var o = {a: 1, b: 2, c: 3}; for (var k in o) { L(k); Object.defineProperty(o, 'c', {enumerable: false}); }",
  "var p = {pa: 1, shared: 1}; var o = Object.create(p); o.own = 1; o.shared = 2; for (var k in o) L(k);",
  "var p = {x: 1}; var o = Object.create(p, {x: {value: 2, enumerable: false}}); for (var k in o) L(k); L('end');",
  "var p = {x: 1}; var o = Object.create(p); p.y = 2; for (var k in o) { L(k); delete p.y; }",
  "var o = Object.create({a: 1}); Object.defineProperty(o, 'b', {value: 1, enumerable: false}); o.c = 1; for (var k in o) L(k);",
  "for (var k in 'ab') L(k);",
  "for (var k in [7, 8, , 9]) L(k);",
  "var a = [1, 2]; a.extra = 1; for (var k in a) L(k + typeof k);",
  "for (var k in null) L('never'); for (var k in undefined) L('never'); L('ok');",
  "for (var k in 5) L('never'); for (var k in true) L('never'); L('ok');",
  "for (var k in new String('xy')) L(k);",
  "for (var k in function f(a) {}) L(k); L('end');",
  "for (var k in new Proxy({a: 1, b: 2}, {ownKeys(t) { L('ownKeys'); return ['b', 'a']; }, getOwnPropertyDescriptor(t, k) { L('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k); }, getPrototypeOf(t) { L('gpo'); return null; }})) L('k:' + k);",
  "var o = {a: 1}; for (var k in o) { L(k); for (var k2 in {x: 1}) { L(k2); } }",
  "var o = {}; for (o.k in {a: 1, b: 2}) L(o.k); for (o['m'] in {c: 1}) L(o.m);",
  "var r = []; for (var [a, b] in {xy: 1, pq: 2}) r.push(a + b); L(JSON.stringify(r));",
  "var r = []; for (var {length} in {abc: 1, de: 2}) r.push(length); L(JSON.stringify(r));",
  "var r = []; for (var x = (L('init'), 0) in {a: 1}) r.push(x); L(JSON.stringify(r));",
  "var r = []; var i = 0; for (var k in {a: 1, b: 2}) { r.push(k); if (++i > 5) break; } L(JSON.stringify(r));",
  "var o = {a: 1, b: 2, c: 3}; var seen = []; for (var k in o) { seen.push(k); if (k === 'a') { o.a2 = 1; delete o.c; } } L(JSON.stringify(seen));",
  "var o = {x: 1}; var proto = {y: 2}; for (var k in o) { Object.setPrototypeOf(o, proto); L(k); } for (var k in o) L('again:' + k);",
  "class C { constructor() { this.a = 1; } m() {} get g() { return 1; } static s() {} } for (var k in new C()) L(k); for (var k in C) L('static:' + k);",
  "var o = {toString: 1, valueOf: 2, hasOwnProperty: 3, constructor: 4}; for (var k in o) L(k);",
  "var s = []; for (var k in {a: 1}) s.push(function () { return k; }); L(s[0]());",
  "var o = {10: 1, 9: 1, b: 1, a: 1, '-1': 1, '01': 1, 4294967295: 1, 4294967294: 1}; for (var k in o) L(k);",
  "var o = {__proto__: {inherited: 1}, own: 1}; var r = []; for (var k in o) { if (Object.hasOwn(o, k)) r.push(k); } L(JSON.stringify(r));",
  "var o = {get a() { L('get a'); return 1; }}; for (var k in o) { L('body'); break; } L('end');",
  "var o = {a: 1, b: 2}; var fn = () => { for (var k in o) { try { return k; } finally { L('fin ' + k); } } }; L(fn());",
  "var r = eval('for (var k in {a: 1, b: 2}) { k; }'); L(r);",
  "var r = eval('3; for (var k in {}) { k; }'); L(r);"
);
// getters em for-in combinados com generators: o getter executa dentro do gerador.
add(
  "var o = {get a() { L('get a'); return 1; }, get b() { L('get b'); return 2; }}; function* g() { for (var k in o) { yield k + o[k]; } } var it1 = g(); L(it1.next().value); L(it1.return('R').value); L(JSON.stringify(it1.next()));",
  "var o = {get a() { L('get a'); throw new Error('G'); }, b: 2}; function* g() { try { for (var k in o) { yield o[k]; } } finally { L('fin'); } } var it1 = g(); try { it1.next(); } catch (e) { L('c:' + e.message); } L(JSON.stringify(it1.next()));",
  "function* g() { for (var k in {a: 1, b: 2, c: 3}) { try { yield k; } finally { L('f' + k); if (k === 'b') return 'early'; } } } L(JSON.stringify([...g()])); var it1 = g(); it1.next(); it1.next(); L(JSON.stringify(it1.return('R')));",
  "function* g() { for (var v of [1, 2, 3]) { try { yield v; } finally { L('f' + v); continue; } } } var it1 = g(); it1.next(); L(JSON.stringify(it1.return('R'))); L(JSON.stringify(it1.next()));",
  "function* g() { for (var v of [1, 2, 3]) { try { yield v; } finally { L('f' + v); break; } } yield 'after'; } var it1 = g(); it1.next(); L(JSON.stringify(it1.return('R'))); L(JSON.stringify(it1.next()));",
  "function* inner() { try { yield 1; yield 2; } finally { L('inner fin'); } } function* g() { for (var v of inner()) { try { yield v; } finally { L('f' + v); break; } } } var it1 = g(); it1.next(); L(JSON.stringify(it1.return('R'))); L(JSON.stringify(it1.next()));",
  "function* inner() { try { yield 1; yield 2; } finally { L('inner fin'); } } function* g() { for (var v of inner()) { yield v; } } var it1 = g(); it1.next(); L(JSON.stringify(it1.throw(new Error('T')))); ".replace("L(JSON.stringify(it1.throw(new Error('T'))));", "try { it1.throw(new Error('T')); } catch (e) { L('c:' + e.message); }"),
  "function* inner() { try { yield 1; yield 2; } finally { L('inner fin'); } } function* g() { for (var v of inner()) { try { yield v; } catch (e) { L('caught ' + e.message); } } } var it1 = g(); it1.next(); L(JSON.stringify(it1.throw(new Error('T')))); L(JSON.stringify(it1.next()));"
);

// ---- Execução no bun, um processo por programa.
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "control-flow-more-golden-"));
const lines = [];
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (let i = programs.length - 1; i >= 0; i--) if (HOST.test(programs[i])) programs.splice(i, 1);
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\nprocess.on("uncaughtException", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 0);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  result = result.replace(/[\t\n\r]+$/, "");
  if (result.includes(tmp) || result.includes("/home/") || result.includes("/tmp/")) return;
  if (result.startsWith("error\tHarness")) return;
  lines.push(`${source}\t${result}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${lines.length} programas de ${programs.length}\n`);
