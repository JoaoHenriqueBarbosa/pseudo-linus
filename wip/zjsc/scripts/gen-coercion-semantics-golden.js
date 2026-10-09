// Gera tests/golden/coercion_semantics_bun.tsv: conversões e coerção por semântica (ToPrimitive com hint, ordem de
// chamada de valueOf/toString/Symbol.toPrimitive, ordem de avaliação dos operandos, ToPropertyKey, ToObject, ==
// entre tipos, comparação BigInt/Number/String, Number()/parseFloat/parseInt exóticos, toString(radix), toFixed,
// toPrecision, toExponential, String(Symbol) e mensagens de TypeError) medidos no bun 1.4.2.
// Complementa o coercion_bun.tsv (matriz de valores x operadores) sem repeti-lo: aqui cada programa registra um
// rastro de chamadas (`log`) e o resultado. Colunas: a fonte (JSON) e o valor de `R` (JSON), igual ao scope golden.
// Sem APIs de host: o programa roda por `vm.runInThisContext` e grava `globalThis.R`.
// Uso: bun scripts/gen-coercion-semantics-golden.js > tests/golden/coercion_semantics_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Programa com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Expressão avaliada com rastro: R vira `resultado|rastro`, ou `Nome: mensagem|rastro` quando lança.
const E = (setup, expr) =>
  add(`var log = []; ${setup}; log = []; try { var v = (${expr}); R = (typeof v === 'bigint' ? v + 'n' : typeof v === 'symbol' ? 'sym' : Object.is(v, -0) ? '-0' : typeof v === 'string' ? JSON.stringify(v) : String(v)) + '|' + log.join() } catch (e) { R = e.name + ': ' + e.message + '|' + log.join() }`);

// Objeto que registra qual método de conversão foi chamado e devolve o que o fabricante decidir.
const obj = (name, valueOf, toString) =>
  `{ valueOf() { log.push('${name}.valueOf'); return ${valueOf} }, toString() { log.push('${name}.toString'); return ${toString} } }`;
const prim = (name, impl) => `{ [Symbol.toPrimitive](hint) { log.push('${name}:' + hint); return ${impl} } }`;

// ---- Hint por operador: Symbol.toPrimitive recebe default, number ou string.
const hintOps = [
  ["+", "x + 1"], ["+ str", "x + ''"], ["-", "x - 1"], ["*", "x * 2"], ["/", "x / 2"], ["%", "x % 2"], ["**", "x ** 2"],
  ["unary +", "+x"], ["unary -", "-x"], ["~", "~x"], ["<<", "x << 1"], [">>", "x >> 1"], [">>>", "x >>> 1"],
  ["&", "x & 1"], ["|", "x | 1"], ["^", "x ^ 1"], ["==", "x == 1"], ["!=", "x != 1"], ["===", "x === 1"],
  ["<", "x < 1"], [">", "x > 1"], ["<=", "x <= 1"], [">=", "x >= 1"], ["1 <", "1 < x"], ["template", "`${x}`"],
  ["template mix", "`a${x}b`"], ["computed key", "({ [x]: 1 })"], ["member read", "({})[x]"], ["member write", "({})[x] = 1"],
  ["in", "x in {}"], ["String()", "String(x)"], ["Number()", "Number(x)"], ["x++", "x++"], ["++x", "++x"],
  ["x--", "x--"], ["+=", "x += 1"], ["Object.is", "Object.is(x, 1)"], ["Math.abs", "Math.abs(x)"],
  ["parseInt", "parseInt(x)"], ["isNaN", "isNaN(x)"], ["JSON key", "JSON.stringify({ [x]: 1 })"],
  ["Array join", "[x].join()"], ["concat str", "'s'.concat(x)"], ["Date", "new Date(x).getTime()"],
  ["switch", "(function () { switch (1) { case x: return 'hit'; default: return 'miss' } })()"],
  ["Array index", "[1, 2, 3][x]"], ["Symbol.for", "Symbol.for(x).description"], ["String.raw", "String.raw`a${x}`"],
  ["toString.call", "Object.prototype.toString.call(x)"], ["delete", "delete ({})[x]"], ["hasOwn", "Object.hasOwn({}, x)"],
];
for (const [, expr] of hintOps) {
  E(`var x = ${prim("tp", "7")}`, expr);
  E(`var x = ${obj("o", "7", "'s'")}`, expr);
  E(`var x = ${obj("o", "{}", "'s'")}`, expr);
  E(`var x = ${obj("o", "{}", "{}")}`, expr);
}

// ---- Variações do Symbol.toPrimitive.
const tpVariants = [
  ["retorna objeto", "{}"], ["retorna null", "null"], ["retorna undefined", "undefined"], ["retorna símbolo", "Symbol('q')"],
  ["retorna bigint", "5n"], ["retorna string", "'str'"], ["lança", "(() => { throw new RangeError('boom') })()"],
];
for (const [, impl] of tpVariants) {
  for (const expr of ["x + 1", "x * 1", "`${x}`", "x == 1", "({ [x]: 1 })", "x < 1", "String(x)"]) E(`var x = ${prim("tp", impl)}`, expr);
}
T("var x = { [Symbol.toPrimitive]: 1 }; R = x + 1");
T("var x = { [Symbol.toPrimitive]: null, valueOf() { return 4 } }; R = x + 1");
T("var x = { [Symbol.toPrimitive]: undefined, valueOf() { return 4 } }; R = x + 1");
T("var x = { [Symbol.toPrimitive]: {} }; R = x + 1");
T("var x = { get [Symbol.toPrimitive]() { throw new Error('getter') } }; R = x + 1");
T("var x = { get [Symbol.toPrimitive]() { return function (h) { return 'g' + h } } }; R = x + 1");
T("var x = { [Symbol.toPrimitive]() { return this === x } }; R = x + 1");
T("var x = { [Symbol.toPrimitive]() { return arguments.length } }; R = x + 1");
T("var x = Object.create({ [Symbol.toPrimitive]: () => 'inh' }); R = x + 1");
T("R = Symbol.prototype[Symbol.toPrimitive].call(Symbol.iterator).toString()");
T("R = Date.prototype[Symbol.toPrimitive].call(new Date(0), 'number')");
T("R = Date.prototype[Symbol.toPrimitive].call(new Date(0), 'bad')");
T("R = Date.prototype[Symbol.toPrimitive].call({}, 'default')");
T("R = typeof Date.prototype[Symbol.toPrimitive].call({ toString() { return 'a' } }, 'string')");
T("R = Date.prototype[Symbol.toPrimitive].call(1, 'number')");
T("R = (new Date(0)) + 1 === (new Date(0)).toString() + '1'");
T("R = (new Date(5)) - 1");
T("R = (new Date(5)) == (new Date(5)).toString()");
T("R = Object(Symbol.iterator) + ''");
T("R = Object(1n) + 1n");
T("R = Object(1n) + 1");
T("R = Object('a') + 1");

// ---- valueOf/toString que lançam, retornam objeto, ordem e fallback.
const vtCases = [
  ["valueOf lança", "(() => { throw new Error('v') })()", "'s'"],
  ["toString lança", "7", "(() => { throw new Error('t') })()"],
  ["ambos lançam", "(() => { throw new Error('v') })()", "(() => { throw new Error('t') })()"],
  ["ambos objeto", "[]", "[]"],
  ["valueOf null", "null", "'s'"], ["valueOf símbolo", "Symbol()", "'s'"], ["valueOf bigint", "3n", "'s'"],
  ["toString símbolo", "{}", "Symbol('t')"], ["toString bigint", "{}", "9n"], ["toString undefined", "{}", "undefined"],
];
for (const [, v, s] of vtCases) {
  for (const expr of ["x + 1", "x + ''", "x * 1", "`${x}`", "x == 1", "x == 's'", "x < 2", "String(x)", "Number(x)", "x ** 1", "-x", "({ [x]: 1 })"]) {
    E(`var x = ${obj("o", v, s)}`, expr);
  }
}
T("var x = { valueOf: 1, toString() { return 'ts' } }; R = x + 1");
T("var x = { valueOf: {}, toString() { return 'ts' } }; R = x * 1");
T("var x = { toString: 1, valueOf() { return 5 } }; R = `${x}`");
T("var x = { toString: null, valueOf: null }; R = x + 1");
T("var x = { toString: undefined, valueOf: undefined }; R = String(x)");
T("var x = Object.create(null); R = x + 1");
T("var x = Object.create(null); R = `${x}`");
T("var x = Object.create(null); R = String(x)");
T("var x = Object.create(null); R = x == 1");
T("var x = Object.create(null); R = x < 1");
T("var x = Object.create(null); R = Number(x)");
T("var x = Object.create(null); R = ({ [x]: 1 })");
T("var x = Object.create(null); R = [x].join()");
T("var x = Object.create(null); R = x + ''");
T("var x = Object.create(null, { toString: { value: () => 'ct' } }); R = x + 1");

// ---- Ordem de avaliação dos operandos com efeitos colaterais.
const side = (n, v) => `(log.push('${n}'), ${v})`;
const binops = ["+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "&", "|", "^", "==", "!=", "===", "!==", "<", ">", "<=", ">=", "in", "instanceof", "&&", "||", "??", ","];
for (const op of binops) {
  const l = op === "in" ? "'a'" : "1";
  const r = op === "in" ? "{ a: 1 }" : op === "instanceof" ? "Object" : "2";
  E("", `${side("L", l)} ${op} ${side("R", r)}`);
}
for (const op of ["+", "-", "*", "<", "==", "<<"]) {
  E(`var a = ${obj("a", "1", "'1'")}, b = ${obj("b", "2", "'2'")}`, `a ${op} b`);
  E(`var a = ${prim("a", "1")}, b = ${prim("b", "2")}`, `a ${op} b`);
  E(`var a = ${obj("a", "1", "'1'")}, b = ${obj("b", "2", "'2'")}`, `(log.push('l'), a) ${op} (log.push('r'), b)`);
  E(`var a = ${obj("a", "1", "'1'")}, b = ${obj("b", "2", "'2'")}`, `b ${op} a`);
  E(`var a = ${obj("a", "(() => { throw new Error('a') })()", "'1'")}, b = ${obj("b", "2", "'2'")}`, `a ${op} b`);
  E(`var a = ${obj("a", "1", "'1'")}, b = ${obj("b", "(() => { throw new Error('b') })()", "'2'")}`, `a ${op} b`);
}
for (const op of ["<", ">", "<=", ">="]) {
  E(`var a = ${obj("a", "1", "'1'")}, b = ${obj("b", "2", "'2'")}`, `a ${op} b`);
  E(`var a = ${prim("a", "1")}, b = ${prim("b", "2")}`, `a ${op} b`);
  E(`var a = ${prim("a", "'1'")}, b = ${prim("b", "'2'")}`, `a ${op} b`);
}
// efeito colateral que muda um operando
T("var x = 1; R = x + (x = 5, x)");
T("var x = 1; R = (x = 5, x) + x");
T("var x = 1; R = x + x++");
T("var x = 1; R = x++ + x");
T("var x = 1; R = x + ++x");
T("var x = 1; R = ++x + x++");
T("var x = 1; R = x-- - --x");
T("var x = 1; R = x + (x += 2)");
T("var x = 1; R = (x += 2) + x");
T("var x = 'a'; R = x + (x = 'b') + x");
T("var x = 1; x += (x = 10, 5); R = x");
T("var x = 1; x *= (x = 10, 5); R = x");
T("var o = { a: 1 }; o.a += (o.a = 10, 5); R = o.a");
T("var o = { a: 1 }; o.a **= (o.a = 2, 3); R = o.a");
T("var log = []; var o = { get a() { log.push('get'); return 1 }, set a(v) { log.push('set' + v) } }; o.a += (log.push('rhs'), 2); R = log.join()");
T("var log = []; var o = { get a() { log.push('get'); return 1 }, set a(v) { log.push('set' + v) } }; o.a++; R = log.join()");
T("var log = []; var o = { get a() { log.push('get'); return { valueOf() { log.push('vo'); return 1 } } }, set a(v) { log.push('set' + typeof v) } }; o.a++; R = log.join()");
T("var log = []; var o = { get a() { log.push('get'); return { valueOf() { log.push('vo'); return 1 } } }, set a(v) { log.push('set' + typeof v) } }; var r = o.a++; R = log.join() + typeof r");
T("var log = []; var o = { get a() { log.push('get'); return 1n }, set a(v) { log.push('set' + typeof v) } }; o.a++; R = log.join()");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = { get a() { log.push('get'); return 1 }, set a(v) { log.push('set') } }; o[k] += 1; R = log.join()");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = {}; o[k] = (log.push('rhs'), 1); R = log.join()");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = {}; (log.push('obj'), o)[k] = (log.push('rhs'), 1); R = log.join()");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = {}; o[k]++; R = log.join() + o.a");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = { a: 1 }; delete o[k]; R = log.join() + o.a");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; var o = { a: 1 }; R = (k in o) + log.join()");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; null[k]; ");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; try { null[k] } catch (e) { R = e.message + log.join() }");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; try { undefined[k] = (log.push('rhs'), 1) } catch (e) { R = e.message + log.join() }");
T("var log = []; var k = { toString() { log.push('key'); return 'a' } }; try { null[(log.push('k'), 'a')] = (log.push('rhs'), 1) } catch (e) { R = e.message + log.join() }");
T("var log = []; function f() { log.push('f'); return 1 } function g() { log.push('g'); return 2 } function h() { log.push('h'); return 3 } R = f() + g() * h() + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = f(1) ** f(2) ** f(3); R += log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = (f(1) ? f(2) : f(3)) + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = (f(0) || f(2) && f(3)) + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = (f(null) ?? f(2)) + log.join()");
T("var log = []; function f(n) { log.push(n); return {} } var o = f(1); o[f(2)] = f(3); R = log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = [f(1), f(2), ...[f(3)], f(4)].join() + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = ({ [f('a')]: f(1), [f('b')]: f(2) }, log.join())");
T("var log = []; function f(n) { log.push(n); return n } R = `${f(1)}${f(2)}${f(3)}` + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = f(1) < f(2) == f(3) > f(4); R += log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = (f(1), f(2), f(3)) + log.join()");
T("var log = []; function f(n) { log.push(n); return () => n } R = f(1)(f(2)) + log.join()");
T("var log = []; function f(n) { log.push(n); return Math.max } R = f(1)(f(2), f(3)) + log.join()");
T("var log = []; function f(n) { log.push(n); return n } var a = [0, 0]; a[f(0)] = a[f(1)] = f(5); R = log.join() + a");
T("var log = []; function f(n) { log.push(n); return n } R = new (class { constructor(a, b) { this.s = a + b } })(f(1), f(2)).s + log.join()");
T("var log = []; function f(n) { log.push(n); return n } R = f(2) in { 2: 1 } && f(1) instanceof Object; R += log.join()");
T("var log = []; var o = { get a() { log.push('a'); return 1 }, get b() { log.push('b'); return 2 } }; R = o.b - o.a; R += log.join()");
T("var log = []; var o = { get a() { log.push('a'); return 1 } }; var { a, a: a2 } = o; R = log.join()");
T("var log = []; var o = { get a() { log.push('a'); return 1 }, get b() { log.push('b'); return 2 } }; var { b, a } = o; R = log.join()");
T("var log = []; var o = { get a() { log.push('a'); return undefined }, get b() { log.push('b'); return 2 } }; var { a = (log.push('def'), 1), b } = o; R = log.join()");
T("var i = 0; var a = [i++, i++, i++]; R = a.join() + i");
T("var i = 0; R = i++ + i++ + i++");
T("var i = 0; R = i++ + ++i");
T("var i = 5; R = i-- * i--");
T("var a = [1, 2, 3]; var i = 0; a[i++] = a[i++] + 10; R = a.join() + i");
T("var a = [1, 2, 3]; var i = 0; a[i] = i = 2; R = a.join()");
T("var a = 1; a = a++ + a++; R = a");
T("var a = 1; a += a++; R = a");
T("var a = 1; a = a-- - a--; R = a");

// ---- == entre tipos.
const eqVals = ["undefined", "null", "true", "false", "0", "-0", "1", "NaN", "''", "' '", "'0'", "'1'", "'a'", "'1n'", "0n", "1n", "-1n", "[]", "[0]", "[1]", "{}", "Symbol.iterator", "Object(1)", "Object('1')", "Object(1n)", "function () {}", "new Date(NaN)", "Object(Symbol.iterator)", "'0x1'", "'1e0'", "' 1 '", "'\\n'", "[[]]", "[null]", "[undefined]", "['1']"];
for (let i = 0; i < eqVals.length; i++) {
  for (let j = i; j < eqVals.length; j++) {
    if (eqVals[i] === "Symbol.iterator" && eqVals[j] === "Symbol.iterator") continue;
    T(`var a = ${eqVals[i]}, b = ${eqVals[j]}; R = [a == b, b == a, a != b].join()`);
  }
}
T("R = [null == undefined, null == 0, undefined == 0, null == false, undefined == '', null == '', null == null].join()");
T("R = [typeof null == 'object', null === null, undefined === void 0].join()");
T("var o = { valueOf() { return null } }; R = [o == null, o == undefined, o == 0, o == false].join()");
T("var o = { valueOf() { return undefined } }; R = [o == null, o == undefined].join()");
T("R = [0n == '', 0n == ' ', 1n == '1', 1n == '01', 1n == '1.0', 1n == '0x1', 1n == ' 1 ', 1n == '1n', 1n == 'a', 2n ** 64n == '18446744073709551616', 2n ** 64n == 18446744073709552000, 2n ** 53n + 1n == 2 ** 53].join()");
T("R = [1n == 1, 1n == 1.5, 1n == true, 0n == false, 0n == null, 0n == undefined, 0n == NaN, 1n == Infinity, 9007199254740993n == 9007199254740992, 9007199254740993n == 9007199254740993, -0 == 0n, 0n == -0].join()");
T("R = [1n == Object(1n), Object(1n) == Object(1n), 1n == { valueOf() { return 1n } }, 1n == { valueOf() { return 1 } }, 1n == { valueOf() { return '1' } }, 1n == [1], 1n == ['1'], 0n == [], 0n == ''].join()");
T("R = [1n === 1, 1n === 1n, Object(1n) === Object(1n), -0n === 0n, Object.is(0n, -0n)].join()");
T("R = [document => 1].length");
T("R = typeof document + '|' + typeof document_all");
T("R = [Symbol.iterator == Symbol.iterator, Symbol.iterator == Object(Symbol.iterator), Object(Symbol.iterator) == Symbol.iterator, Symbol.iterator == 'Symbol(Symbol.iterator)', Symbol('a') == Symbol('a')].join()");
T("var s = Symbol('a'); R = [s == s, s == 'Symbol(a)', s == 'a', s == 0, s == null, s == undefined, s == {}, Object(s) == s].join()");
T("var s = Symbol('a'); R = [s != 1, s !== 1, s === s].join()");
T("R = ['1' == 1, '1.0' == 1, '0x10' == 16, '1e2' == 100, '' == 0, ' ' == 0, '\\t\\n' == 0, '0b11' == 3, '0o7' == 7, '1_0' == 10, '.5' == 0.5, '5.' == 5, '+5' == 5, '-5' == -5, '--5' == -5, 'Infinity' == Infinity, '-Infinity' == -Infinity, 'infinity' == Infinity, '+Infinity' == Infinity].join()");
T("R = [NaN == NaN, NaN != NaN, NaN === NaN, [NaN].includes(NaN), [NaN].indexOf(NaN), Object.is(NaN, 0 / 0)].join()");
T("R = [true == 1, true == '1', true == 'true', false == '', false == '0', false == [], false == [0], true == [1], true == [2], true == { valueOf() { return 1 } }].join()");
T("R = [[] == '', [] == 0, [0] == 0, [[]] == 0, [[0]] == 0, [1, 2] == '1,2', [] == [], [] == ![], {} == '[object Object]', ({}) == ({})].join()");
T("var d = new Date(0); R = [d == d.toString(), d == d.getTime(), d == 0, d == d.valueOf(), d == String(d)].join()");
T("var f = function () {}; R = [f == f.toString(), f == 'function () {}', f == 0].join()");
T("R = [new String('a') == 'a', new String('a') == new String('a'), new Number(1) == 1, new Boolean(false) == false, new Boolean(false) == 0, new Boolean(false) == '', new Boolean(false) == true, !!new Boolean(false)].join()");
T("var log = []; var o = { valueOf() { log.push('v'); return 1 } }; R = [o == 1, 1 == o, o == '1', o == true, o == 2n].join() + log.join()");
T("var log = []; var o = { toString() { log.push('t'); return '1' }, valueOf() { log.push('v'); return 2 } }; R = [o == 2, o == '1', o == '2', o == 1n, o == 2n].join() + log.join()");
T("var log = []; var o = { [Symbol.toPrimitive](h) { log.push(h); return 1 } }; R = [o == 1, o == '1', o == 1n, o == true, o == null, o == undefined, o == o, o == {}].join() + log.join()");
T("var log = []; var o = { [Symbol.toPrimitive](h) { log.push(h); return 1 } }; R = [o != 1, o < 2, o + 1, o * 2, `${o}`, o === 1].join() + log.join()");
T("var log = []; var o = { valueOf() { log.push('v'); return {} }, toString() { log.push('t'); return 5 } }; R = (o == 5) + log.join()");
T("var log = []; var o = { valueOf() { log.push('v'); return {} }, toString() { log.push('t'); return {} } }; try { o == 5 } catch (e) { R = e.message + log.join() }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o == 5 } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o < 5 } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o + 5 } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { `${o}` } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { String(o) } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { Number(o) } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { ({ [o]: 1 }) } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { -o } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o++ } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o.x = o + '' } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { [o].join() } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { 'a'.concat(o) } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { Math.abs(o) } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o ** 2 } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { valueOf() { return {} }, toString() { return {} } }; try { o & 1 } catch (e) { R = e.name + ': ' + e.message }");

// ---- TypeError exatos de coerção.
const typeErrors = [
  "Symbol() + 1", "Symbol() + ''", "'' + Symbol()", "`${Symbol()}`", "+Symbol()", "-Symbol()", "~Symbol()", "Symbol() * 2", "Symbol() < 1",
  "1 < Symbol()", "Symbol() == 1", "Number(Symbol())", "parseInt(Symbol())", "parseFloat(Symbol())", "isNaN(Symbol())", "Math.abs(Symbol())",
  "Symbol('d') + Symbol('d')", "[Symbol()].join()", "'a'.concat(Symbol())", "'a' + Symbol.iterator", "Symbol() | 0", "Symbol() ** 2",
  "1n + 1", "1 + 1n", "1n - 1", "1n * 1.5", "1n / 1", "1n % 1", "1n ** 1", "+1n", "1n >>> 1n", "1n >>> 1", "1n & 1", "1n | 1", "1n ^ 1", "1n << 1", "1n >> 1", "~1n",
  "-1n", "1n++", "Math.abs(1n)", "Math.max(1n)", "Math.round(1n)", "Number(1n)", "isNaN(1n)", "parseInt(1n)", "1n + ''", "1n + 'a'", "1n + {}", "1n + []",
  "1n + null", "1n + undefined", "1n + true", "1n * true", "1n + Symbol()", "1n / 0n", "1n % 0n", "2n ** -1n", "BigInt(1.5)", "BigInt('1.5')", "BigInt('a')",
  "BigInt(Symbol())", "BigInt(undefined)", "BigInt(null)", "BigInt({})", "BigInt([])", "BigInt('')", "BigInt(NaN)", "BigInt(Infinity)", "BigInt(1e21)", "BigInt(true)",
  "BigInt(' 12 ')", "BigInt('0x1f')", "BigInt('1e3')", "BigInt('-0')", "BigInt('+5')", "BigInt('1_0')", "BigInt(-0)", "BigInt(2 ** 64)", "BigInt(Number.MAX_SAFE_INTEGER + 2)",
  "BigInt('-0x1')", "BigInt('0b101')", "BigInt('0o17')", "BigInt('0b')", "BigInt('٣')", "BigInt('9'.repeat(30))", "new BigInt(1)", "BigInt.asIntN(64, 2n ** 63n)",
  "BigInt.asUintN(8, -1n)", "BigInt.asIntN(-1, 1n)", "BigInt.asIntN(2 ** 53, 1n)", "BigInt.asUintN(1, 1)", "BigInt.asUintN(0, 5n)",
  "undefined.x", "null.x", "undefined[0]", "null['a b']", "null[Symbol.iterator]", "undefined.x.y", "({}).x.y", "({}).x()", "({ x: 1 }).x()", "({}).x.y.z",
  "undefined()", "null()", "(void 0)()", "({})()", "({ a: {} }).a()", "[][0]()", "[].x()", "new 1", "new (void 0)", "new ({})", "new (() => 1)", "new Math.max", "new Symbol()",
  "new Math", "1 in 1", "'a' in 'abc'", "'a' in null", "Symbol() in 1", "1 instanceof 1", "1 instanceof {}", "({}) instanceof (() => 1)", "({}) instanceof null",
  "({}) instanceof { [Symbol.hasInstance]: 1 }", "({}) instanceof { [Symbol.hasInstance]() { return 5 } }", "x = 1; let x", "Object.defineProperty(1, 'a', {})",
  "Object.setPrototypeOf(null, {})", "Object.setPrototypeOf({}, 1)", "Object.create(1)", "Object.keys(null)", "Object.keys(undefined)", "Object.assign(null)",
  "Object.entries(undefined)", "Object.fromEntries(1)", "Object.fromEntries([1])", "Object.fromEntries([[]])", "[...1]", "[...null]", "[...undefined]", "[...{}]", "[...Symbol()]",
  "new Set(1)", "new Map([1])", "new Map(1)", "new WeakSet([1])", "new WeakMap([[1, 1]])", "new WeakRef(1)", "Array.from(null)", "Array.from(undefined)", "new Array(-1)", "new Array(1.5)", "new Array(2 ** 32)",
  "[].length = -1", "[].length = 1.5", "'a'.repeat(-1)", "'a'.repeat(Infinity)", "'a'.padStart(2 ** 31 - 1 + 2, 'b'.repeat(1 << 29))", "'a'.normalize('x')", "String.fromCodePoint(-1)", "String.fromCodePoint(1.5)", "String.fromCodePoint(0x110000)",
  "'a'.localeCompare()", "'a'.at(Symbol())", "String.prototype.trim.call(null)", "String.prototype.trim.call(undefined)", "String.prototype.toString.call(1)", "Number.prototype.toString.call('1')",
  "Number.prototype.valueOf.call({})", "Boolean.prototype.valueOf.call(1)", "Symbol.prototype.toString.call(1)", "Symbol.prototype.valueOf.call({})", "BigInt.prototype.toString.call(1)", "BigInt.prototype.valueOf.call(1)",
  "Date.prototype.getTime.call({})", "Date.prototype.toISOString.call(new Date(NaN))", "new Date(NaN).toISOString()", "new Date(8.64e15 + 1).toISOString()", "Date.prototype.toString.call(1)",
  "Object.prototype.toString.call(null)", "Object.prototype.toString.call(undefined)", "Object.prototype.valueOf.call(null)", "Object.prototype.toLocaleString.call(null)", "Object.prototype.hasOwnProperty.call(null, 'a')",
  "Object.prototype.hasOwnProperty.call(undefined, Symbol())", "Object.prototype.isPrototypeOf.call(null, {})", "Object.prototype.propertyIsEnumerable.call(null, 'a')", "Object.getPrototypeOf(null)", "Object.getPrototypeOf(undefined)",
  "Object.getOwnPropertyNames(null)", "Object.getOwnPropertyDescriptor(null, 'a')", "Object.getOwnPropertyDescriptors(undefined)", "Object.freeze(1)", "Object.isFrozen(1)", "Reflect.get(1, 'a')", "Reflect.ownKeys(1)",
  "Reflect.apply(1)", "Reflect.construct(1, [])", "Reflect.construct(() => 1, [])", "Reflect.defineProperty(1, 'a', {})", "Reflect.getPrototypeOf(1)", "new Proxy(1, {})", "new Proxy({}, 1)", "Proxy({}, {})",
  "JSON.parse('{')", "JSON.parse('')", "JSON.parse(undefined)", "JSON.parse('{\"a\":}')", "JSON.parse('[1,]')", "JSON.parse(\"{'a':1}\")", "JSON.stringify(1n)", "JSON.stringify({ a: 1n })", "JSON.stringify((() => { const o = {}; o.o = o; return o })())",
  "JSON.stringify((() => { const o = { a: { b: [] } }; o.a.b.push(o); return o })())", "JSON.parse('1n')", "JSON.parse('NaN')", "JSON.parse('{\"a\":1,}')", "JSON.parse('\\u0000')",
  "new Intl.NumberFormat('en', { style: 'currency' })", "(1).toFixed(101)", "(1).toFixed(-1)", "(1).toPrecision(0)", "(1).toPrecision(101)", "(1).toExponential(101)", "(1).toString(1)", "(1).toString(37)", "(1n).toString(1)", "(1n).toString(37)",
  "(1).toString(Symbol())", "(1).toFixed(Symbol())", "Number.prototype.toFixed.call('1')", "Number.prototype.toPrecision.call({})", "Number.prototype.toExponential.call(null)",
  "Number.parseFloat === parseFloat", "Number.parseInt === parseInt", "Number(' 1 ')", "isFinite(Symbol())", "Number.isFinite(Symbol())", "Number.isInteger(1n)",
  "encodeURIComponent('\\ud800')", "decodeURIComponent('%')", "decodeURIComponent('%E0%A4%A')", "decodeURI('%zz')", "encodeURI('\\udc00')", "escape(Symbol())", "unescape(Symbol())",
  "new RegExp('[')", "new RegExp('(')", "new RegExp('a', 'gg')", "new RegExp('a', 'x')", "new RegExp(Symbol())", "/a/.test(Symbol())", "'a'.match(Symbol())", "'a'.replace(/a/, Symbol())", "'a'.split(Symbol())", "'a'.indexOf(Symbol())",
  "'a'.replaceAll(/a/, 'b')", "'a'.matchAll(/a/)", "'a'.startsWith(/a/)", "'a'.endsWith(/a/)", "'a'.includes(/a/)", "'a'.search(Symbol())", "RegExp.prototype.exec.call({}, 'a')", "RegExp.prototype.test.call(1, 'a')",
  "Array.prototype.map.call(null, x => x)", "[].map(1)", "[].reduce((a, b) => a)", "[].reduceRight((a, b) => a)", "[].forEach()", "[].sort(1)", "[].sort(null)", "[1].flatMap(1)", "Array.prototype.at.call(null)",
  "[].toSorted(1)", "[].toSpliced.call(null)", "new Array(2 ** 32 - 1).concat([1]).length", "Array.prototype.push.call(Object.freeze([]), 1)", "Object.freeze([1]).push(2)", "Object.freeze({ a: 1 }).a = 2", "'use strict'; Object.freeze({ a: 1 }).a = 2",
  "(function () { 'use strict'; Object.freeze({ a: 1 }).a = 2 })()", "(function () { 'use strict'; undefinedVar = 1 })()", "(function () { 'use strict'; 'abc'.length = 1 })()", "(function () { 'use strict'; 'abc'[0] = 'x' })()",
  "(function () { 'use strict'; (1).x = 1 })()", "(function () { 'use strict'; Symbol().x = 1 })()", "(function () { 'use strict'; delete Object.prototype })()", "(function () { 'use strict'; ({ get a() { return 1 } }).a = 2 })()",
  "(function () { 'use strict'; NaN = 1 })()", "(function () { 'use strict'; undefined = 1 })()", "(function () { 'use strict'; Infinity++ })()", "(function () { 'use strict'; delete [].length })()", "(function () { 'use strict'; Object.preventExtensions({}).a = 1 })()",
  "(function () { 'use strict'; true.x = 1 })()", "(function () { 'use strict'; 1n.x = 1 })()", "(function () { 'use strict'; (void 0).x = 1 })()", "(function () { 'use strict'; null.x = 1 })()", "(function () { 'use strict'; null[Symbol()] = 1 })()",
  "(function () { 'use strict'; Math.PI = 3 })()", "(function () { 'use strict'; arguments.callee })()", "(function () { 'use strict'; (function () {}).caller })()", "(function () { 'use strict'; (function () {}).arguments })()",
  "class A { constructor() { this.x } } class B extends A { constructor() { this.x; super() } } new B", "class A {} class B extends A { constructor() { } } new B", "class A {} A()", "class A { #p; static t(o) { return o.#p } } A.t({})", "class A { #p; static t(o) { o.#p = 1 } } A.t({})",
  "class A { #m() {} static t(o) { o.#m() } } A.t({})", "class A { static #p = 1; static t(o) { return #p in o } } A.t(1)", "class A extends null {} new A", "class A extends 1 {}", "class A extends (() => 1) {}", "class A extends Object { constructor() { super(); super() } } new A",
  "(function () { new.target.x })()", "function* g() {} new g", "async function f() {} new f", "(() => 1).call.call(1)", "Function.prototype.call.call(1)", "Function.prototype.toString.call({})", "Function.prototype.bind.call(1)", "(function () {}).bind().prototype.x",
  "Symbol.keyFor('a')", "Symbol.keyFor(1)", "Symbol.for(Symbol())", "Symbol().description.x", "Symbol.prototype.description", "Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.call(1)",
  "new Promise()", "new Promise(1)", "Promise()", "Promise.resolve.call(1)", "Promise.prototype.then.call(1)", "Promise.all.call(1)", "new (class extends Promise { constructor() { super(() => {}) } })().then.call(1)",
  "new WeakRef({}).deref.call(1)", "new FinalizationRegistry(1)", "new FinalizationRegistry(() => {}).register(1)", "new Uint8Array(-1)", "new Uint8Array(2 ** 53)", "new ArrayBuffer(-1)", "new DataView(1)", "new DataView(new ArrayBuffer(1)).getInt8(2)", "new Uint32Array(new ArrayBuffer(3))", "new Uint32Array(new ArrayBuffer(8), 1)",
  "Uint8Array.prototype.length", "Uint8Array.from.call(1)", "new Uint8Array(1).set([1, 2])", "new Uint8Array(1).set(1, -1)", "Atomics.add(new Float64Array(1), 0, 1)", "Atomics.add(new Int8Array(1), 5, 1)",
  "structuredClone", "new Error('a', 1).cause", "new AggregateError(1)", "Error.captureStackTrace", "new (class extends Error {})('a') instanceof Error",
  "with_undefined_ref", "typeof undefined_ref", "undefined_ref", "undefined_ref = 1", "(0, undefined_ref)", "delete undefined_ref", "undefined_ref++", "undefined_ref += 1", "[undefined_ref]", "({ a: undefined_ref })", "undefined_ref?.x",
  "globalThis.nope.x", "globalThis.nope()", "Math.nope()", "Math.PI()", "Math.PI.x()", "''.x()", "(1).x()", "true.x()", "(1n).x()", "Symbol().x()", "Symbol.iterator.x()",
  "[].x.y", "[1, 2].x.y", "'abc'.x.y", "(() => 1).x.y", "(class {}).x.y", "(function () {}).x.y", "new Date().x.y", "/a/.x.y", "new Map().x.y", "Promise.resolve().x.y", "new Error().x.y",
  "var a = {}; a.b.c.d", "var a = { b: null }; a.b.c", "var a = { b: undefined }; a.b.c", "var a = { b: undefined }; a.b()", "var a = { b: null }; a.b()", "var a = { b: 1 }; a.b()", "var a = { b: {} }; a.b()", "var a = { b: 'x' }; a.b()", "var a = { b: [] }; a.b()", "var a = []; a[0]()", "var a = []; a[0].x", "var a = {}; a['b-c']()", "var a = {}; a[1]()", "var a = {}; a[Symbol('s')]()", "var a = {}; a[{}]()", "var a = {}; a[[]]()", "var a = {}; a[null]()", "var a = {}; a[undefined]()", "var a = {}; a[1.5]()", "var a = {}; a[-0]()", "var a = {}; a[1n]()",
  "var f = 1; f()", "var f = 'a'; f()", "var f = {}; f()", "var f = null; f()", "var f = undefined; f()", "var f = Symbol(); f()", "var f = 1n; f()", "var f = []; f()", "var f = true; f()", "var f = new Date(); f()", "var f = /a/; f()", "var f = Math; f()", "var f = new Map(); f()", "var f = class {}; f()",
  "var f = 1; new f()", "var f = 'a'; new f()", "var f = {}; new f()", "var f = null; new f()", "var f = undefined; new f()", "var f = Symbol(); new f()", "var f = () => 1; new f()", "var f = async () => 1; new f()", "var f = { m() {} }; new f.m()", "var f = Math.max; new f()", "var f = parseInt; new f()", "var f = Symbol; new f()", "var f = BigInt; new f()",
  "var o = { m() { return 1 } }; new o.m", "var o = { get g() { return 1 } }; new o.g", "var o = { async m() {} }; new o.m", "var o = { *m() {} }; new o.m", "class C { m() {} } new (new C).m", "class C { static m() {} } new C.m",
];
for (const expr of typeErrors) T(`R = String(${expr.includes(";") || /\b(let|class|var|function\*)\b/.test(expr) || expr.startsWith("'use strict'") ? `(0, eval)(${JSON.stringify(expr)})` : expr})`);

// ---- ToPropertyKey.
const keyVals = ["1", "-0", "0", "1.5", "1e21", "1e-7", "NaN", "Infinity", "-Infinity", "null", "undefined", "true", "false", "''", "'1'", "'01'", "'-0'", "'1.0'", "2 ** 32", "2 ** 32 - 1", "2 ** 32 - 2", "-1", "2 ** 53", "1n", "-1n", "0n", "[]", "[1]", "[1, 2]", "{}", "[[]]", "Symbol.iterator", "Object(1)", "Object('a')", "new Date(0)", "function () {}", "0.1 + 0.2", "123456789012345680000", "0.000001", "0.0000001", "'4294967295'", "'4294967296'", "'4294967294'", "String(2 ** 31)"];
for (const k of keyVals) {
  T(`var k = ${k}; var o = {}; o[k] = 1; R = Reflect.ownKeys(o).map(x => typeof x === 'symbol' ? 'S' : x).join() + '|' + (k in o) + '|' + typeof Object.keys(o)[0]`);
  T(`var k = ${k}; var o = { [k]: 1 }; R = Reflect.ownKeys(o).map(x => typeof x === 'symbol' ? 'S' : x).join()`);
  T(`var k = ${k}; var a = []; a[k] = 1; R = a.length + '|' + Reflect.ownKeys(a).map(x => typeof x === 'symbol' ? 'S' : x).join()`);
  T(`var k = ${k}; var o = class { static [k]() {} }; R = Object.getOwnPropertyNames(o).join() + '|' + (typeof k === 'symbol' ? 'S' : o[k].name)`);
}
T("var o = { [Symbol('d')]() {} }; R = Object.getOwnPropertySymbols(o).map(s => o[s].name).join()");
T("var o = { [Symbol()]() {} }; R = Object.getOwnPropertySymbols(o).map(s => JSON.stringify(o[s].name)).join()");
T("var o = { get [Symbol('g')]() { return 1 } }; R = Object.getOwnPropertyDescriptor(o, Object.getOwnPropertySymbols(o)[0]).get.name");
T("var o = { get [1]() { return 1 }, set [1](v) {} }; var d = Object.getOwnPropertyDescriptor(o, 1); R = d.get.name + '|' + d.set.name");
T("var o = { [-0]() {} }; R = Object.keys(o).join() + o[0].name");
T("var o = { [1e21]() {} }; R = Object.keys(o).join() + o['1e+21'].name");
T("var o = { [1n]() {} }; R = Object.keys(o).join() + o[1].name");
T("var o = { [[1, 2]]() {} }; R = Object.keys(o).join() + o['1,2'].name");
T("var o = { [{}]() {} }; R = Object.keys(o).join()");
T("var o = { [null]: 1, [undefined]: 2, [true]: 3 }; R = Object.keys(o).join()");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' }, valueOf() { log.push('vo'); return 'v' } }; var o = { [k]: 1 }; R = Object.keys(o).join() + log.join()");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' }, valueOf() { log.push('vo'); return 'v' } }; var o = {}; o[k] = 1; var x = o[k]; R = Object.keys(o).join() + log.join()");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' } }; var o = {}; Object.defineProperty(o, k, { value: 1 }); R = log.join() + Object.getOwnPropertyNames(o)");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' } }; var o = { k: 1 }; R = [Object.hasOwn(o, k), o.hasOwnProperty(k), k in o, Reflect.has(o, k), Object.getOwnPropertyDescriptor(o, k).value, o.propertyIsEnumerable(k)].join() + log.join()");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' } }; var o = new Proxy({}, { get(t, p) { log.push(typeof p + ':' + String(p)); return 1 } }); o[k]; o[1]; o[Symbol.iterator]; R = log.join()");
T("var log = []; var k = { toString() { log.push('ts'); return 'k' } }; var o = new Proxy({}, { has(t, p) { log.push(typeof p); return false }, set(t, p) { log.push('set' + typeof p); return true }, deleteProperty(t, p) { log.push('del' + typeof p); return true } }); k in o; o[k] = 1; delete o[k]; R = log.join()");
T("var p = new Proxy({}, { get(t, k) { return typeof k } }); R = [p.a, p[1], p[Symbol.iterator], p[-0], p[1.5], p[1n], p[[]], p[{}], p[null]].join()");
T("var o = { 1: 'a', '01': 'b', 1.0: 'c' }; R = Object.keys(o).join() + JSON.stringify(o)");
T("var o = { 0.1: 1, .5: 2, 1e3: 3, 0x10: 4, 0b11: 5, 0o7: 6, 1_0: 7, 1n: 8 }; R = Object.keys(o).join()");
T("var o = {}; o[1] = 'n'; o['1'] = 's'; o[1.0] = 'f'; R = Object.keys(o).length + o[1]");
T("var o = { b: 1, 2: 1, a: 1, 1: 1, [Symbol()]: 1, '-1': 1, '01': 1, 4294967294: 1, 4294967295: 1, 4294967296: 1 }; R = Object.keys(o).join()");
T("var a = []; a['1'] = 1; a['01'] = 2; a[1.0] = 3; a['1.0'] = 4; a[-1] = 5; a[2 ** 32 - 1] = 6; R = a.length + '|' + Object.keys(a).join()");
T("var a = []; a[2 ** 32 - 2] = 1; R = a.length");
T("var a = []; a[2 ** 32 - 1] = 1; R = a.length");
T("var a = []; a['4294967294'] = 1; R = a.length");
T("var a = [1, 2, 3]; R = [a['1'], a[1.0], a['1.0'], a[true], a[[1]], a[{ toString() { return 2 } }], a[-0], a['-0'], a[1n], a[Object(1)]].join()");
T("var s = 'abc'; R = [s[1], s['1'], s[-1], s[3], s[1.5], s[1n], s[[2]], s[true], s.length, s['length']].join()");
T("var o = { toString() { return 'a' } }; var t = { a: 1, [o]: 2 }; R = JSON.stringify(t)");

// ---- ToObject.
const toObjVals = ["1", "'ab'", "true", "1n", "Symbol.iterator", "null", "undefined", "NaN", "-0", "''", "0"];
for (const v of toObjVals) {
  T(`R = typeof Object(${v}) + '|' + Object.prototype.toString.call(Object(${v})) + '|' + (Object(${v}) == ${v}) + '|' + (Object(${v}) === ${v})`);
  T(`R = Object.prototype.toString.call(Object.assign({}, ${v})) + Object.keys(Object(${v})).join()`);
  T(`R = JSON.stringify(Object.getOwnPropertyNames(Object(${v})))`);
  T(`R = Object.getPrototypeOf(Object(${v})) === Object.getPrototypeOf(${v})`);
  T(`R = String(Object.entries(${v}))`);
  T(`R = JSON.stringify(Object.getOwnPropertyDescriptors(${v}))`);
  T(`R = (function () { return typeof this }).call(${v}) + (function () { 'use strict'; return typeof this }).call(${v})`);
  T(`R = String(Reflect.ownKeys(Object(${v})).length)`);
  T(`var f = function () { return this }; var r = f.call(${v}); R = (r === globalThis) + typeof r`);
  T(`R = Object.prototype.hasOwnProperty.call(${v}, 'length')`);
  T(`R = ${v} instanceof Object`);
  T(`for (var k in ${v}) { R = k } R = R === undefined ? 'none' : R`);
  T(`R = [...Object.keys(${v})].length`);
  T(`R = Object.getOwnPropertyNames(${v}).length`);
}
T("Number.prototype.me = function () { 'use strict'; return typeof this }; R = (5).me(); delete Number.prototype.me");
T("Number.prototype.me = function () { return typeof this }; R = (5).me(); delete Number.prototype.me");
T("String.prototype.me = function () { return this === 'a' }; R = 'a'.me(); delete String.prototype.me");
T("String.prototype.me = function () { 'use strict'; return this === 'a' }; R = 'a'.me(); delete String.prototype.me");
T("var calls = 0; Object.defineProperty(Number.prototype, 'sp', { get() { 'use strict'; calls++; return typeof this }, configurable: true }); R = (5).sp + calls; delete Number.prototype.sp");
T("Object.defineProperty(Number.prototype, 'sp', { set(v) { 'use strict'; R = typeof this + v }, configurable: true }); (5).sp = 1; delete Number.prototype.sp");
T("Object.defineProperty(Number.prototype, 'sp', { set(v) { R = typeof this + v }, configurable: true }); (5).sp = 1; delete Number.prototype.sp");
T("(function () { 'use strict'; (5).x = 1 })()");
T("(function () { (5).x = 1; R = 'sloppy ok' })()");
T("R = Object.getOwnPropertyNames(Object('abc')).join()");
T("R = Object.keys('abc').join() + Object.keys(Object('abc')).join()");
T("R = JSON.stringify(Object.assign({}, 'ab', 1, true, null, undefined, [3]))");
T("R = JSON.stringify({ ...'ab', ...1, ...true, ...null, ...undefined, ...[3], ...Symbol() })");
T("R = Object.getOwnPropertyDescriptor('abc', 0).writable + '|' + Object.getOwnPropertyDescriptor('abc', 'length').value");
T("var { length } = 'abc'; var { x } = 1; var { toFixed } = 1; R = length + typeof x + typeof toFixed");
T("var { x } = null");
T("var { x } = undefined");
T("var {} = null");
T("var [] = null");
T("var [a] = 1");
T("var [a] = {}");
T("var [a] = Symbol()");
T("var { x } = Symbol(); R = typeof x");
T("function f({ a }) {} f()");
T("function f({ a }) {} f(null)");
T("function f([a]) {} f()");
T("(({ a }) => a)()");
T("for (var { a } of [null]);");
T("for (var [a] of [undefined]);");
T("var { [Symbol.iterator]: it } = 1; R = typeof it");
T("var { a: { b } } = { a: null }");
T("var { a: { b } } = { a: undefined }");
T("var { a: { b } } = {}");
T("var { a: [b] } = {}");
T("var { a: [b] } = { a: 1 }");
T("var [[a]] = [null]");
T("var [{ a }] = [undefined]");
T("with (null) {}");
T("with (undefined) {}");
T("with (1) { R = typeof toFixed }");
T("with ('abc') { R = length }");
T("with (Symbol.iterator) { R = typeof description + description }");
T("with (1n) { R = typeof toString }");
T("with (true) { R = typeof valueOf }");
T("for (var k in null) { R = 'x' } R = R === undefined ? 'no loop' : R");
T("for (var k in undefined) { R = 'x' } R = R === undefined ? 'no loop' : R");
T("var r = []; for (var k in 'ab') r.push(k); for (var k in 1) r.push(k); for (var k in true) r.push(k); R = r.join()");
T("for (var k of 1) {}");
T("for (var k of null) {}");
T("for (var k of {}) {}");
T("var r = []; for (var k of 'a😀b') r.push(k.length); R = r.join()");
T("var o = { a: 1 }; R = Object.keys(Object.create(o)).length + '|' + Object.entries(Object.create(o)).length + '|' + ('a' in Object.create(o))");
T("R = [Object(null) instanceof Object, Object(undefined) instanceof Object, Object({}) instanceof Object, new Object(1) instanceof Number, new Object('a') instanceof String, new Object(1n) instanceof BigInt, new Object(Symbol()) instanceof Symbol].join()");
T("var o = {}; R = [Object(o) === o, new Object(o) === o, Object.call(null, o) === o, Object.call(null) !== undefined].join()");
T("class A extends Object { constructor() { super(1) } } R = Object.prototype.toString.call(new A) + typeof new A");
T("class A extends Object { constructor() { super(); } } R = new A() instanceof A");
T("class A extends Number { } R = new A(5) + 1 + '|' + new A(5).valueOf() + '|' + (new A(5) == 5)");
T("class A extends String { } R = new A('x') + 1 + '|' + new A('x').length + '|' + new A('xy')[1]");
T("class A extends Boolean { } R = new A(false) ? 'truthy' : 'falsy'");
T("class A extends Array { } R = new A(3).length + '|' + (new A(3) + '') + '|' + Array.isArray(new A)");
T("class A extends Date { } R = new A(0) - 0 + '|' + typeof (new A(0) + 1)");
T("class A extends Symbol { }");
T("class A extends BigInt { } new A");
T("class A extends Error { } R = new A('m') + '|' + new A('m').name + '|' + Object.prototype.toString.call(new A)");
T("class A extends Function { } R = typeof new A('return 1') + new A('return 1')()");

// ---- Template literais.
const tplVals = ["undefined", "null", "true", "-0", "0n", "1e21", "1e-7", "[]", "[null]", "[undefined, 1]", "{}", "[{}]", "[[1, [2]]]", "function f() {}", "class A {}", "() => 1", "/a/g", "new Error('e')", "new Date(NaN)", "Symbol.iterator.description", "Object(1)", "Object('s')", "new Map()", "new Set([1])", "Promise.resolve()", "JSON", "Math", "globalThis", "new Uint8Array([1, 2])", "new ArrayBuffer(2)", "Object.create(null, { toString: { value: () => 'ct' } })", "Object(1n)", "[1n]", "[Symbol.iterator.description]", "(function () { return arguments })(1, 2)", "String(Symbol('z'))", "new Number(-0)", "[-0]", "[0.1 + 0.2]", "[1e21]", "'\\ud800'", "'\\u{1F600}'", "'\\0'"];
for (const v of tplVals) {
  T(`R = \`\${${v}}\``);
  T(`R = \`a\${${v}}b\${${v}}\``);
  T(`R = '' + (${v})`);
  T(`R = String(${v})`);
  T(`R = [${v}] + ''`);
}
T("R = `${{ toString() { return 1 } }}` + typeof `${{ toString() { return 1 } }}`");
T("R = `${{ valueOf() { return 'v' }, toString() { return 't' } }}`");
T("R = `${Symbol()}`");
T("R = `a${Symbol()}b`");
T("R = String.raw`a${Symbol()}`");
T("R = String.raw`\\n${1}\\u00zz`");
T("function tag(s, ...v) { return s.raw.join('|') + '#' + v.map(x => typeof x).join() } R = tag`a${1}b${{}}c${Symbol()}d`");
T("function tag(s, ...v) { return s.length + ':' + v.length } R = tag`${1}${2}`");
T("function tag(s) { return s } var a = tag`x`, b = tag`x`; R = a === b");
T("function tag(s) { return s } function g() { return tag`x` } R = g() === g()");
T("function tag(s) { return Object.isFrozen(s) + '' + Object.isFrozen(s.raw) } R = tag`x`");
T("function tag(s) { return s[0] === undefined } R = tag`\\unicode`");
T("function tag(s) { return String(s[0]) + s.raw[0] } R = tag`\\u{110000}`");
T("var log = []; var o = { toString() { log.push('ts'); return 'T' }, valueOf() { log.push('vo'); return 'V' } }; R = `${o}${o}` + log.join()");
T("var log = []; var mk = n => ({ toString() { log.push(n); return n } }); R = `${mk('a')}${mk('b')}${mk('c')}` + log.join()");
T("var log = []; var mk = n => ({ toString() { log.push('ts' + n); return n }, valueOf() { log.push('vo' + n); return n } }); R = (mk('a') + mk('b')) + `${mk('c')}` + log.join()");

// ---- Number() e parseFloat/parseInt exóticos.
const numStrs = ["''", "' '", "'\\n\\t\\r\\v\\f'", "'\\u00a0'", "'\\ufeff'", "'\\u2028'", "'\\u2029'", "'\\u1680'", "'\\u180e'", "'\\u200b'", "'\\u3000'", "'\\u2003 5 \\u2003'", "'\\u180e5'", "'\\u200b5'", "'0x'", "'0x0'", "'0xg'", "'0XaB'", "'0b'", "'0b2'", "'0B1'", "'0o'", "'0o8'", "'0O7'", "'-0x1'", "'+0x1'", "'0x1.8'", "'1_000'", "'1e'", "'1e+'", "'1e-'", "'1e1'", "'1E1'", "'1e1.5'", "'.e1'", "'.'", "'+.'", "'-.5'", "'5.'", "'.5e1'", "'5.e1'", "'0.0000001'", "'1e-7'", "'1e400'", "'-1e400'", "'1e-400'", "'-1e-400'", "'Infinity'", "'+Infinity'", "'-Infinity'", "'infinity'", "'INFINITY'", "'Infinityx'", "'NaN'", "'nan'", "'-NaN'", "'+NaN'", "'  Infinity  '", "'1 2'", "'1,5'", "'1.5.5'", "'++1'", "'+-1'", "'- 1'", "'1-'", "'0.1e-1'", "'00'", "'01'", "'-00'", "'007'", "'08'", "'0.'", "'00.5'", "'1e0001'", "'9007199254740993'", "'9007199254740992.5'", "'0.30000000000000004'", "'123456789012345678901234567890'", "'1.7976931348623157e308'", "'1.7976931348623159e308'", "'4.9e-324'", "'2.4e-324'", "'2.5e-324'", "'5e-324'", "'0x1fffffffffffff'", "'0x20000000000001'", "'0x' + 'f'.repeat(300)", "'1' + '0'.repeat(400)", "'0.' + '0'.repeat(400) + '1'", "'1e' + '9'.repeat(30)", "'\\u0661'", "'١٢٣'", "'1\\u0000'", "'\\u00001'", "'1n'", "'0n'", "'true'", "'null'", "'[1]'", "'0x1n'", "'1e1_0'", "'0_1'", "'0.1_1'", "'٠'", "'0e0'", "'-0e0'", "'0e-5'"];
for (const s of numStrs) {
  T(`var s = ${s}; R = [Number(s), parseFloat(s), parseInt(s), parseInt(s, 16), parseInt(s, 2), parseInt(s, 36), +s, s * 1, s - 0, s | 0, s >>> 0, Object.is(+s, -0), isNaN(s), Number.isNaN(s)].map(String).join()`);
  T(`var s = ${s}; R = [parseInt(s, 0), parseInt(s, 1), parseInt(s, 37), parseInt(s, 10), parseInt(s, 8), parseInt(s, undefined), parseInt(s, null), parseInt(s, NaN), parseInt(s, -0), parseInt(s, 2 ** 32 + 10), parseInt(s, 16.9), parseInt(s, '16')].map(String).join()`);
}
T("R = [parseInt('123abc'), parseInt('abc'), parseInt('  -42xyz'), parseInt('0x1fg'), parseInt('0x'), parseInt('1e3'), parseInt('1.9'), parseInt('-1.9'), parseInt('-0'), 1 / parseInt('-0'), parseInt('+'), parseInt('-'), parseInt(' ')].map(String).join()");
T("R = [parseInt(0.0000001), parseInt(1e21), parseInt(1e-7), parseInt(123456789012345680000), parseInt(-0), parseInt(0.5), parseInt(null), parseInt(undefined), parseInt(true), parseInt([]), parseInt([12, 3]), parseInt({}), parseInt(NaN), parseInt(Infinity), parseInt(-Infinity), parseInt(1n), parseInt(2 ** 53)].map(String).join()");
T("R = [parseInt('z', 36), parseInt('Z', 36), parseInt('10', 36), parseInt('zz', 36), parseInt('1', 2), parseInt('2', 2), parseInt('11111111111111111111111111111111111111111111111111111111111111111', 2), parseInt('9007199254740993'), parseInt('0x20000000000001'), parseInt('9'.repeat(400)), parseInt('1' + '0'.repeat(25))].map(String).join()");
T("R = [parseInt('10', 2.9), parseInt('10', 2.1), parseInt('10', '0x10'), parseInt('0x10', 16), parseInt('0x10', 10), parseInt('0x10', 0), parseInt('0x10', 8), parseInt('0b11', 2), parseInt('0o7', 8), parseInt('-0x10', 16), parseInt('+0x10', 16), parseInt('0X', 16), parseInt('0x', 36)].map(String).join()");
T("R = [parseFloat('3.14abc'), parseFloat('.5'), parseFloat('-.5'), parseFloat('5.'), parseFloat('1e3'), parseFloat('1e'), parseFloat('1e+'), parseFloat('1.5e+3x'), parseFloat('Infinityx'), parseFloat('-Infinity'), parseFloat('infinity'), parseFloat('0x10'), parseFloat('  \\n 7'), parseFloat('1_0'), parseFloat('.'), parseFloat('-'), parseFloat('+.e1'), parseFloat('-0'), 1 / parseFloat('-0'), parseFloat('1e1000'), parseFloat('-1e1000'), parseFloat('1e-1000'), parseFloat('00012'), parseFloat('0.0.1'), parseFloat('1.2.3'), parseFloat('1ee1'), parseFloat('NaN'), parseFloat('.1.1'), parseFloat('٣')].map(String).join()");
T("R = [parseFloat(null), parseFloat(undefined), parseFloat(true), parseFloat([]), parseFloat([1.5, 2]), parseFloat({}), parseFloat({ toString() { return '2.5x' } }), parseFloat(1n), parseFloat(0.0000001), parseFloat(1e21), parseFloat(-0), parseFloat(Infinity), parseFloat(NaN)].map(String).join()");
T("R = [Number(null), Number(undefined), Number(true), Number(false), Number([]), Number([5]), Number([1, 2]), Number({}), Number(new Date(5)), Number('12px'), Number(1n), Number(2n ** 64n), Number(-(2n ** 1024n)), Number(2n ** 1024n), Number(2n ** 53n + 1n), Number(2n ** 53n + 2n), Number(), Number(Object(1n)), Number(Object('7')), Number([[]]), Number([[1]]), Number(['0x10']), Number(function () {}), Number(/a/), Number(new Boolean(true))].map(String).join()");
T("R = [Number(2n ** 64n - 1n), Number(2n ** 63n), Number(-(2n ** 63n)), Number(0xfffffffffffff800n), Number(0xfffffffffffffc00n), Number(0x1fffffffffffff3n), Number(0x1fffffffffffff5n), Number(0x1fffffffffffffn), Number(0x20000000000001n), Number(0x20000000000003n), Number(0x20000000000005n)].map(String).join()");
T("R = [1 / Number('-0'), Object.is(Number('-0'), -0), Object.is(Number('0'), 0), Object.is(+'-0.0', -0), Object.is(-'0', -0), Object.is(Number(-0n), 0), Object.is(Number('-0e5'), -0), Object.is(parseInt('-0'), -0), Object.is(parseFloat('-0'), -0), Object.is(Math.round(-0.4), -0), Object.is(-0 + 0, 0), Object.is(-0 - 0, -0), Object.is(0 * -1, -0), Object.is(0 / -1, -0), Object.is(-0 % 1, -0), Object.is(5 % -5, 0), Object.is(-5 % 5, -0), Object.is(Math.sign(-0), -0), Object.is(Math.max(-0, 0), 0), Object.is(Math.min(-0, 0), -0), Object.is(Math.abs(-0), 0), Object.is(Math.ceil(-0.5), -0), Object.is(Math.trunc(-0.5), -0), Object.is(Math.fround(-0), -0), Object.is(Math.sqrt(-0), -0), Object.is(Math.atan2(-0, 1), -0), Object.is(-0 ** 2 === undefined, false)].join()");
T("R = [~~'12.7', ~~'-12.7', ~~NaN, ~~Infinity, ~~-Infinity, ~~1e21, ~~2 ** 31, ~~-(2 ** 31) - 1, 2 ** 32 | 0, (2 ** 32 + 5) | 0, (2 ** 31) | 0, (-(2 ** 31) - 1) | 0, 1e21 | 0, 1e300 | 0, -1e300 | 0, 4294967295 >>> 0, -1 >>> 0, -1 >>> 32, 1 << 32, 1 << 31, 1 << -1, 1 >> 33, -1 >> 31, -1 >>> 31, 2 ** 53 | 0, (2 ** 53 + 2) | 0, 0.9 | 0, -0.9 | 0, '0x10' | 0, '1e3' | 0, null | 0, undefined | 0, [] | 0, [7] | 0, {} | 0, true | 0, 1e-7 | 0, 4294967296.5 | 0, -4294967296.5 | 0, 2147483648.5 | 0, 2147483647.9 | 0].map(String).join()");
T("R = [5 / 0, -5 / 0, 0 / 0, 5 % 0, 0 % 5, -0 % 5, Infinity % 5, 5 % Infinity, -5 % Infinity, 5.5 % 2, -5.5 % 2, 5 % -2, 2 ** -1, 0 ** 0, NaN ** 0, 1 ** Infinity, (-1) ** Infinity, (-8) ** (1 / 3), 2 ** 0.5, (-2) ** 2, (-2) ** 3, 0 ** -1, (-0) ** -1, (-0) ** -2, Infinity ** 0, Infinity ** -1, (-Infinity) ** 3, (-Infinity) ** 2, (-Infinity) ** -3, 10 ** 308, 10 ** 309, 10 ** -324, 10 ** -325, 2 ** 1023 * 2, 2 ** 1024, 2 ** -1074, 2 ** -1075, 3 ** 40, 7 ** 22, 0.1 ** 2, 1.1 ** 100].map(String).join()");
T("R = [0.1 + 0.2, 0.1 * 3, 0.3 - 0.1, 1.1 * 1.1, 4.35 * 100, 1.005 * 1000, 9007199254740992 + 1, 9007199254740992 + 2, 9007199254740993, 2 ** 53 + 1, 1e16 + 1, 1e21 + 1, 123456789 * 987654321, 1 / 3, 2 / 3, 10 / 3, 1e300 * 1e10, -1e300 * 1e10, 1e-300 / 1e100, 5e-324 / 2, 5e-324 * 0.5, 5e-324 * 0.51, 1.7976931348623157e308 + 1e292, 1.7976931348623157e308 + 1e291].map(String).join()");

// ---- Number.prototype.toString(radix).
const radixNums = ["0", "-0", "1", "-1", "255", "0.5", "-0.5", "0.1", "0.2", "0.3", "1 / 3", "2 / 3", "Math.PI", "Math.E", "1e21", "1e-7", "1.5e-10", "123456789.123456789", "2 ** 53", "2 ** 53 + 2", "2 ** 31", "2 ** 32", "2 ** 64", "2 ** 100", "2 ** 1023", "Number.MAX_VALUE", "Number.MIN_VALUE", "Number.EPSILON", "Number.MAX_SAFE_INTEGER", "1e100", "1e-100", "NaN", "Infinity", "-Infinity", "0.000001", "0.0000001", "4.35", "1.005", "35", "36", "37", "1 / 7", "100.5", "-255.75", "1e15 + 0.5", "0.1 + 0.2", "5e-324", "1.7976931348623157e308 / 3"];
const radixes = ["2", "3", "7", "8", "10", "16", "32", "36"];
for (const n of radixNums) {
  T(`R = [${radixes.map(r => `(${n}).toString(${r})`).join(", ")}].join('|')`);
}
T("R = [(255).toString(undefined), (255).toString(10.9), (255).toString('16'), (255).toString(null === 0 ? 1 : 2.5), (255).toString(new Number(16)), (255).toString({ valueOf() { return 8 } })].join('|')");
T("R = (255).toString(1)");
T("R = (255).toString(0)");
T("R = (255).toString(37)");
T("R = (255).toString(NaN)");
T("R = (255).toString(null)");
T("R = (255).toString(Infinity)");
T("R = (255).toString(-2)");
T("R = (255).toString(1.9)");
T("R = (255).toString(36.99)");
T("R = (255).toString(37n)");
T("R = (255).toString({})");
T("R = (255).toString(true)");
T("R = (255).toString(false)");
T("R = (255).toString([2])");
T("R = (255).toString('')");
T("R = (255).toString(' 2 ')");
T("R = [(2 ** 53).toString(2), (2 ** 53 + 2).toString(36), (-(2 ** 53)).toString(7), (0.5).toString(2), (0.1).toString(2), (0.1).toString(3), (1 / 3).toString(3), (1e21).toString(36), (1e-7).toString(16), Number.MAX_VALUE.toString(2).length, Number.MIN_VALUE.toString(2).length, (2 ** -1074).toString(2).length, Number.MAX_VALUE.toString(36).length].join('|')");
T("R = [(1n).toString(2), (-255n).toString(16), (2n ** 64n).toString(36), (2n ** 64n).toString(2), (0n).toString(36), (-0n).toString(2), (123456789012345678901234567890n).toString(7), (2n ** 200n).toString(32), (-(2n ** 65n)).toString(3), BigInt(Number.MAX_VALUE).toString(16).length].join('|')");
T("R = [Number.prototype.toString.call(5, 2), Number.prototype.toString.call(new Number(5), 2), Number.prototype.toLocaleString.call(5).length > 0].join()");
T("R = [(25).toString(36), (35).toString(36), (36).toString(36), (-35).toString(36), (0.5).toString(36), (35.5).toString(36), (1e21).toString(2).length, (1e21).toString(10), (1e21).toString(16), (1e20).toString(10), (123e-20).toString(10), (123e-20).toString(2).length].join('|')");

// ---- toFixed, toPrecision, toExponential.
const fmtNums = ["0", "-0", "1", "-1", "0.5", "1.5", "2.5", "-2.5", "0.05", "0.15", "0.25", "0.35", "1.005", "1.45", "8.345", "10.235", "1.255", "123.456", "0.000001", "0.0000001", "1e-10", "1e20", "1e21", "1e22", "1.5e21", "123456789012345680000", "999999999999999900000", "2 ** 53", "Number.MAX_VALUE", "Number.MIN_VALUE", "NaN", "Infinity", "-Infinity", "0.1", "0.3", "0.7", "9.995", "99.995", "0.9999999", "1e-7", "5e-324", "1.7976931348623157e308", "4.35", "0.000123456", "1234.5678", "-1234.5678", "25", "35", "45", "0.5e-6", "9.5", "99.5", "999.5"];
const fixed = ["0", "1", "2", "3", "5", "10", "20", "50", "99", "100"];
for (const n of fmtNums) {
  T(`R = [${fixed.map(d => `(${n}).toFixed(${d})`).join(", ")}].join('|')`);
}
const precs = ["1", "2", "3", "5", "10", "21", "50", "99", "100"];
for (const n of fmtNums) {
  T(`R = [${precs.map(d => `(${n}).toPrecision(${d})`).join(", ")}, (${n}).toPrecision()].join('|')`);
  T(`R = [${["0", "1", "2", "5", "10", "20", "50", "99", "100"].map(d => `(${n}).toExponential(${d})`).join(", ")}, (${n}).toExponential()].join('|')`);
}
for (const bad of ["-1", "101", "NaN", "'a'", "undefined", "null", "1.9", "-0.9", "Infinity", "{}", "'2'", "true", "[3]", "101.5", "100.9", "-1e-9", "1n", "Symbol()"]) {
  T(`R = (1.5).toFixed(${bad})`);
  T(`R = (1.5).toPrecision(${bad})`);
  T(`R = (1.5).toExponential(${bad})`);
  T(`R = (NaN).toExponential(${bad}) + (Infinity).toPrecision(${bad}) + (NaN).toFixed(${bad})`);
}
T("R = [(0).toPrecision(1), (0).toPrecision(5), (-0).toPrecision(3), (0).toExponential(), (0).toExponential(3), (-0).toExponential(), (0).toFixed(2), (-0).toFixed(2), (-0.0001).toFixed(2), (-0.5).toFixed(0), (0.5).toFixed(0), (1.5).toFixed(0), (2.5).toFixed(0), (-1.5).toFixed(0), (-2.5).toFixed(0)].join('|')");
T("R = [(123456).toPrecision(2), (123456).toPrecision(6), (123456).toPrecision(7), (1e21).toPrecision(3), (1e-7).toPrecision(3), (0.000001).toPrecision(3), (0.0000001).toPrecision(1), (123.456).toPrecision(4), (123.456).toPrecision(2), (99.99).toPrecision(3), (99.99).toPrecision(2), (0.00001).toPrecision(1), (1e21).toPrecision(22), (1e21).toPrecision(21), (1e21).toPrecision(22), (12345).toPrecision(4)].join('|')");
T("R = [(1e21).toFixed(2), (1e21 - 1e5).toFixed(2), (-1e21).toFixed(2), (1e20).toFixed(2), (123456789012345680000).toFixed(2), (0.1).toFixed(20), (0.1).toFixed(25 > 100 ? 1 : 100).length, (1.1).toFixed(50), (5e-324).toFixed(100).length, (5e-324).toFixed(100).slice(-5), Number.MAX_VALUE.toFixed(0).length].join('|')");
T("R = [(0.000001234).toExponential(), (0.000001234).toExponential(2), (123456).toExponential(), (123456).toExponential(0), (123456).toExponential(10), (-123456).toExponential(1), (1.5).toExponential(0), (2.5).toExponential(0), (0.5).toExponential(0), (9.5).toExponential(0), (99.5).toExponential(1), (1e21).toExponential(), (1e-7).toExponential(), (5e-324).toExponential(), (5e-324).toExponential(3), Number.MAX_VALUE.toExponential(), Number.MAX_VALUE.toExponential(20), (1.255).toExponential(2), (1.245).toExponential(2)].join('|')");
T("R = [String(1e21), String(1e-7), String(123456789012345680000), String(1.5e300), String(1e300 * 10), String(0.000001), String(0.0000001), String(1.7976931348623157e308), String(5e-324), String(4.9406564584124654e-324), String(2 ** 70), String(0.1 + 0.7), String(-1e-7), String(1e21 - 1), String(999999999999999900000), String(100000000000000000000), String(12345678901234567890), String(1.2345678901234567e-8)].join('|')");
T("R = [1e21 + '', 1e-6 + '', 1e-7 + '', -1e-7 + '', 123e18 + '', 123e19 + '', 1.5e-9 + '', 0.00001 + '', 1e100 + '', 1e-100 + '', 2e0 + '', 0xff + '', 0o17 + '', 0b11 + '', .5e1 + '', 5e-1 + '', 1_0.0_1 + ''].join('|')");
T("R = [Number.MAX_SAFE_INTEGER + '', Number.MIN_SAFE_INTEGER + '', Number.EPSILON + '', Number.MAX_VALUE + '', Number.MIN_VALUE + '', (-Number.MAX_VALUE) + '', Number.MAX_SAFE_INTEGER + 2 + '', 2 ** 53 + 2 + '', 2 ** 63 + '', 2 ** 64 + '', 2 ** 70 + '', 2 ** 80 + '', 2 ** -20 + '', 2 ** -30 + '', 2 ** -40 + ''].join('|')");

// ---- String(Symbol), Symbol() + '', descrições.
T("R = [String(Symbol()), String(Symbol('')), String(Symbol('a')), String(Symbol(undefined)), String(Symbol(null)), String(Symbol(1)), String(Symbol({})), String(Symbol([1, 2])), String(Symbol('a b')), String(Symbol.iterator), String(Symbol.for('k')), String(Symbol('\\n'))].join('|')");
T("R = [Symbol().toString(), Symbol('x').toString(), Symbol().description, Symbol('').description, Symbol(undefined).description, Symbol(null).description, Symbol.iterator.description, Symbol.for('').description, Symbol.for().description, Symbol.for(undefined).description === 'undefined'].map(String).join('|')");
T("R = Symbol() + ''");
T("R = '' + Symbol('a')");
T("R = `${Symbol('a')}`");
T("R = Symbol('a') + 1");
T("R = Symbol('a') + 'b'");
T("R = ['a', Symbol('b')].join()");
T("R = [Symbol('b')] + ''");
T("R = String([Symbol('b')])");
T("R = [Symbol('b')].toString()");
T("R = JSON.stringify([Symbol('b')])");
T("R = JSON.stringify({ a: Symbol('b'), [Symbol('c')]: 1 })");
T("R = JSON.stringify(Symbol('b'))");
T("R = String(Object(Symbol('q')))");
T("R = Object(Symbol('q')).toString()");
T("R = Object(Symbol('q')) + ''");
T("R = Object(Symbol('q')) == Symbol('q')");
T("R = typeof Object(Symbol()) + typeof Object(Symbol()).valueOf() + typeof Object(Symbol())[Symbol.toPrimitive]()");
T("R = Symbol.prototype.toString.call(Object(Symbol('w')))");
T("R = Symbol.prototype.toString.call({})");
T("R = Symbol.prototype.toString.call('a')");
T("R = Symbol.prototype.description");
T("R = Symbol.prototype[Symbol.toPrimitive].call('a')");
T("R = Symbol.prototype[Symbol.toStringTag] + Object.prototype.toString.call(Symbol())");
T("R = Symbol.iterator.toString() + Symbol.asyncIterator.toString() + Symbol.hasInstance.toString() + Symbol.toPrimitive.toString() + Symbol.toStringTag.toString()");
T("R = Symbol('a') === Symbol('a')");
T("R = Symbol.for('a') === Symbol.for('a')");
T("R = Symbol.keyFor(Symbol.for('z')) + String(Symbol.keyFor(Symbol('z'))) + String(Symbol.keyFor(Symbol.iterator))");
T("R = Symbol().constructor === Symbol && typeof Symbol.prototype.valueOf.call(Symbol())");
T("R = [typeof Symbol(), typeof Object(Symbol()), Symbol() instanceof Symbol, Object(Symbol()) instanceof Symbol].join()");
T("R = !Symbol() + '|' + !!Symbol() + '|' + (Symbol() ? 't' : 'f') + '|' + (Symbol() && 'and') + '|' + (Symbol() || 'or')");
T("R = [Boolean(Symbol()), Boolean(Object(Symbol())), Boolean(0n), Boolean(1n), Boolean(-0), Boolean(NaN), Boolean(''), Boolean(' '), Boolean('0'), Boolean('false'), Boolean([]), Boolean({}), Boolean(null), Boolean(undefined), Boolean(function () {}), Boolean(new Boolean(false)), Boolean(Object(0n)), Boolean(Object(''))].join()");
T("var o = { [Symbol.toStringTag]: 'Custom' }; R = String(o) + Object.prototype.toString.call(o) + `${o}` + (o + '')");
T("var o = { [Symbol.toStringTag]: 1 }; R = String(o)");
T("var o = { [Symbol.toStringTag]: Symbol() }; R = String(o)");
T("var o = { get [Symbol.toStringTag]() { throw new Error('tag') } }; R = String(o)");
T("R = [Object.prototype.toString.call(null), Object.prototype.toString.call(undefined), Object.prototype.toString.call(1), Object.prototype.toString.call(''), Object.prototype.toString.call(true), Object.prototype.toString.call(1n), Object.prototype.toString.call(Symbol()), Object.prototype.toString.call([]), Object.prototype.toString.call(() => 1), Object.prototype.toString.call(new Date()), Object.prototype.toString.call(/a/), Object.prototype.toString.call(new Error), Object.prototype.toString.call(Math), Object.prototype.toString.call(JSON), Object.prototype.toString.call(new Map), Object.prototype.toString.call(new Set), Object.prototype.toString.call(Promise.resolve()), Object.prototype.toString.call((function () { return arguments })()), Object.prototype.toString.call(function* () {}), Object.prototype.toString.call((function* () {})()), Object.prototype.toString.call(async () => 1), Object.prototype.toString.call(new Uint8Array), Object.prototype.toString.call(new ArrayBuffer(1)), Object.prototype.toString.call(globalThis), Object.prototype.toString.call(new Proxy([], {})), Object.prototype.toString.call(new Proxy(function () {}, {})), Object.prototype.toString.call(Object.create(null)), Object.prototype.toString.call(Atomics), Object.prototype.toString.call(Reflect), Object.prototype.toString.call(Intl)].join('|')");

// ---- BigInt vs Number vs String: comparação relacional (fora do que o operator_edge e o bigint_edge já têm: aqui com rastro e coerção de objeto).
const cmpVals = ["1n", "-1n", "0n", "2n ** 53n", "2n ** 53n + 1n", "2n ** 64n", "2n ** 1024n", "-(2n ** 1024n)", "1", "-1", "0", "-0", "1.5", "-1.5", "2 ** 53", "2 ** 64", "1e308", "Infinity", "-Infinity", "NaN", "'1'", "'-1'", "'0'", "''", "' '", "'1.5'", "'abc'", "'1n'", "'0x10'", "'9007199254740993'", "'18446744073709551616'", "'1e3'", "'Infinity'", "'-Infinity'", "' 12 '", "'12a'", "'\\n'", "true", "false", "null", "undefined", "[]", "[1]", "{}", "Object(1n)", "Object(2)", "Object('3')", "'10'", "'9'", "'a'", "'B'", "'ab'", "'b'"];
const cmpOps = ["<", ">", "<=", ">="];
for (let i = 0; i < cmpVals.length; i++) {
  for (let j = 0; j < cmpVals.length; j++) {
    if (!/n\b/.test(cmpVals[i] + cmpVals[j]) && !/Object\(1n\)/.test(cmpVals[i] + cmpVals[j])) continue;
    if ((i * 7 + j * 3) % 5 !== 0) continue;
    T(`var a = ${cmpVals[i]}, b = ${cmpVals[j]}; R = ${JSON.stringify(cmpOps)}.map(op => { try { return { '<': a < b, '>': a > b, '<=': a <= b, '>=': a >= b }[op] } catch (e) { return e.name } }).join()`);
  }
}
T("R = ['a' < 'b', 'a' < 'B', 'B' < 'a', '10' < '9', '10' < 9, 10 < '9', 'a' < 1, '' < 'a', '' < 0, [] < 1, [2] > 1, [1, 2] < 3, null < 1, null >= 0, null > 0, null == 0, undefined < 1, undefined >= 0, NaN < NaN, NaN <= NaN, NaN >= NaN, 'abc' < 'abd', 'abc' < 'ab', '\\ud800' < '\\uffff', '\\u{1F600}' < '\\uffff', 'Z' < 'a', 'é' > 'z'].join()");
T("R = [1n < 2, 2 < 3n, 1n < 1.5, 2n > 1.5, 1n < 'a', 1n < '2', '2' > 1n, 1n < 'x1', 1n < '', 0n >= '', 0n <= ' ', 1n < Infinity, 1n > -Infinity, 1n < NaN, 1n >= NaN, 2n ** 64n > 1e19, 2n ** 64n < 1.8446744073709552e19, 2n ** 64n <= 1.8446744073709552e19, 2n ** 64n >= 1.8446744073709552e19, 2n ** 1024n < Infinity, 2n ** 1024n > 1.7976931348623157e308, -(2n ** 1024n) < -Infinity, 9007199254740993n > 9007199254740992, 9007199254740993n >= 9007199254740993, 9007199254740993n > 9007199254740993, 9007199254740992n < 9007199254740993, 1n < true, 0n < true, 1n <= true, 0n > false, 0n >= false, 1n > null, 0n >= null, 0n >= undefined, 1n < undefined].join()");
T("R = ['9007199254740993' > 9007199254740992n, 9007199254740993n > '9007199254740992', '9007199254740993' == 9007199254740993n, 9007199254740993n == '9007199254740993', '1e3' == 1000n, '1e3' < 1001n, 1n < '1e3', 5n > '4.9', 5n > '4a', 5n < '5.5', 5n == '5.0', '0b11' == 3n, '0x11' == 17n, '0o11' == 9n, '-0' == 0n, '-1' == -1n, '+1' == 1n, '1 ' == 1n, ' 1' == 1n, '١' == 1n].join()");
T("var log = []; var a = { valueOf() { log.push('a'); return 1n } }, b = { valueOf() { log.push('b'); return 2 } }; R = [a < b, a > b, a <= b, a >= b, a == b, b > a, b >= a].join() + log.join()");
T("var log = []; var a = { [Symbol.toPrimitive](h) { log.push('a' + h); return 1n } }, b = { [Symbol.toPrimitive](h) { log.push('b' + h); return '2' } }; R = [a < b, b > a, a == b].join() + log.join()");
T("var log = []; var a = { valueOf() { log.push('a'); return NaN } }, b = { valueOf() { log.push('b'); return 2 } }; R = [a < b, b < a, a >= b, b >= a].join() + log.join()");
T("var log = []; var a = { valueOf() { log.push('a'); return 'x' } }, b = { valueOf() { log.push('b'); return 'y' } }; R = [a < b, b < a, a >= b].join() + log.join()");
T("R = [1n + 2n, 2n * 3n, 7n / 2n, -7n / 2n, 7n % -2n, -7n % 2n, 2n ** 10n, (-2n) ** 3n, 0n ** 0n, 1n << 70n, -1n >> 70n, 5n >> 1n, -5n >> 1n, 5n << -1n, 5n & -2n, 5n | -8n, 5n ^ -1n, ~5n, ~-1n, -(-5n), -0n, 0n * -1n, 2n ** 64n - 1n, (2n ** 64n) * (2n ** 64n), -(2n ** 64n) / 3n, (2n ** 64n) % 1000n].map(String).join()");
T("R = [typeof (1n + 2n), typeof Object(1n), typeof BigInt(1), typeof (1n < 2), typeof -1n, typeof (1n == 1), typeof 1n.toString(), typeof BigInt.asIntN(8, 1n)].join()");
T("var x = 1n; x++; var y = x--; R = x + '|' + y + '|' + typeof x + '|' + (x += 5n) + '|' + (x **= 2n) + '|' + (x <<= 2n) + '|' + (x %= 7n)");
T("var x = 1n; x += 1");
T("var x = 1; x += 1n");
T("var x = 'a'; x += 1n; R = x");
T("var x = 1n; x += 'a'; R = x");
T("var x = 1n; x -= '1'");
T("var x = 1n; x *= null");
T("var x = 1n; x = +x");
T("var x = 1n; R = -x + '|' + (x > 0) + '|' + !x + '|' + !0n + '|' + (x ? 'y' : 'n') + '|' + (0n ? 'y' : 'n') + '|' + (x && 'and') + '|' + (0n || 'or') + '|' + (0n ?? 'nn')");
T("R = [BigInt('  12  '), BigInt('0x1F'), BigInt('0b101'), BigInt('0o17'), BigInt(''), BigInt('  '), BigInt('-12'), BigInt('+12'), BigInt(true), BigInt(false), BigInt(1e21), BigInt(-0), BigInt(Number.MAX_SAFE_INTEGER), BigInt(2 ** 70), BigInt(1.0), BigInt(Object(5n)), BigInt([]), BigInt([7]), BigInt(new Boolean(true)), BigInt({ valueOf() { return 3 } }), BigInt({ toString() { return '9' } }), BigInt({ [Symbol.toPrimitive]() { return 4n } })].map(String).join()");
T("R = [BigInt.asIntN(8, 255n), BigInt.asIntN(8, 128n), BigInt.asIntN(8, 127n), BigInt.asIntN(8, -129n), BigInt.asIntN(0, 5n), BigInt.asIntN(1, 1n), BigInt.asIntN(1, 2n), BigInt.asIntN(64, 2n ** 63n), BigInt.asIntN(64, -(2n ** 63n) - 1n), BigInt.asIntN(65, 2n ** 64n), BigInt.asUintN(8, 256n), BigInt.asUintN(8, -1n), BigInt.asUintN(64, -1n), BigInt.asUintN(0, -1n), BigInt.asUintN(1, -1n), BigInt.asUintN(100, -1n), BigInt.asIntN('8', '255'), BigInt.asIntN(8.9, 255n), BigInt.asUintN(true, 3n), BigInt.asUintN(2 ** 53 - 1, 5n)].map(String).join()");
T("R = [Number.isInteger(1n), Number.isSafeInteger(1n), Number.isFinite(1n), Number.isNaN(1n), isFinite(1n === 1n), typeof Number(1n), Math.max(...[1, 2]), Object.is(1n, 1n), Object.is(1n, 1), [1n].includes(1n), [1n].includes(1), [1n].indexOf(1), new Set([1n, 1n, 1]).size, new Map([[1n, 'a']]).get(1n), new Map([[1n, 'a']]).get(1), [1n, 1].map(x => x === 1n)].join()");
T("R = [JSON.stringify({ a: 1 }, (k, v) => typeof v === 'bigint' ? 'B' + v : v), String(Object(1n)), [1n, 2n].join('-'), [1n, 2n] + '', `${[1n]}`, (123n).toLocaleString('en'), 1n.toLocaleString(), BigInt.prototype.valueOf.call(Object(3n)) + 1n, Object(3n) + 1n].join()");
T("var a = [3n, 1, 2n, 10, '5']; a.sort(); R = a.join() + '|' + a.sort((x, y) => (x < y ? -1 : x > y ? 1 : 0)).join()");
T("R = [3n, 1n, 2n].sort().join() + [3n, 1n, 10n].sort().join() + [3n, 1n, 10n].sort((a, b) => Number(a - b)).join()");
T("R = [3n, 1n, 2n].sort((a, b) => a - b).join()");
T("R = [3n, 1n, 2n].sort((a, b) => (a < b ? -1 : 1)).join() + Math.max(...[1n].map(Number))");
T("R = Math.max(1n)");
T("R = [1n, 2n].reduce((a, b) => a + b) + [1n, 2n].reduce((a, b) => a + b, 0n)");
T("R = [1n, 2n].reduce((a, b) => a + b, 0)");

// ---- Execução.
// Diretório fixo: um nome aleatório de mkdtemp vazaria para o golden (o repositório é público).
const dir = "/tmp/zjsc-coercion-semantics";
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });
// O bun passa arquivos pelo transpilador próprio; `vm.runInThisContext` roda como ProgramExecutable do JSC puro. O
// SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "case_source.js");
const file = path.join(dir, "case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
