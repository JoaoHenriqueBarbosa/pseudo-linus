// Gera tests/golden/builtins_gap_bun.tsv: os dois maiores buracos de cobertura por método (ver wip-notes/coverage-map.md),
// medidos no bun 1.4.2. (1) Os métodos Annex B de String (anchor, big, blink, bold, fixed, fontcolor, fontsize, italics,
// link, small, strike, sub, sup, trimLeft, trimRight, substr) e (2) os getters de flag de RegExp, `flags`, `source`,
// `compile` e `toString`. Cada programa exercita resultados normais, argumentos limite (NaN, -0, Infinity, undefined, null,
// símbolo), objetos que logam a ordem de coerção (valueOf, toString, Symbol.toPrimitive), getters que lançam, this inválido
// e a mensagem exata de cada erro.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// O arquivo se chama `builtins_gap_case.js` dos dois lados.
// Uso: bun scripts/gen-builtins-gap-golden.js > tests/golden/builtins_gap_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = (...sources) => programs.push(...sources);

// `t` roda uma função e devolve o resultado (string em JSON) ou "Nome: mensagem" do erro lançado.
const PRELUDE =
  "function t(f) { try { var v = f(); return typeof v === 'string' ? JSON.stringify(v) : String(v) } catch (e) { return e.name + ': ' + e.message } }\n" +
  "var log = [];\n" +
  "var logged = (tag, prim) => ({ toString() { log.push(tag + '.toString'); return prim === undefined ? tag : prim }, valueOf() { log.push(tag + '.valueOf'); return 1 } });\n" +
  "var toPrim = tag => ({ [Symbol.toPrimitive](hint) { log.push(tag + '.' + hint); return tag }, toString() { log.push(tag + '.toString'); return 'x' }, valueOf() { log.push(tag + '.valueOf'); return 'y' } });\n" +
  "var thrower = { toString() { throw new RangeError('toString lançou') } };\n" +
  "var valueThrower = { toString: undefined, valueOf() { throw new SyntaxError('valueOf lançou') } };\n";
const program = body => PRELUDE + body;

// ---------------------------------------------------------------------------------------------
// 1. String Annex B: métodos de marcação HTML.
// ---------------------------------------------------------------------------------------------
const plain = [
  ["big", "big"], ["blink", "blink"], ["bold", "b"], ["fixed", "tt"], ["italics", "i"], ["small", "small"], ["strike", "strike"],
  ["sub", "sub"], ["sup", "sup"],
];
const attr = [["anchor", "a", "name"], ["fontcolor", "font", "color"], ["fontsize", "font", "size"], ["link", "a", "href"]];

const receivers = [
  ["'abc'", "abc"],
  ["''", "vazia"],
  ["'a\"b<c>&d'", "especiais"],
  ["12", "número"],
  ["-0", "menos zero"],
  ["true", "booleano"],
  ["{ toString() { log.push('this.toString'); return 'obj' }, valueOf() { log.push('this.valueOf'); return 1 } }", "objeto"],
  ["[1, 2]", "array"],
];
const badThis = ["undefined", "null", "Symbol('s')", "thrower", "valueThrower", "1n"];
const attrArgs = [
  "", "undefined", "null", "'x'", "''", "'a\"b'", "'a\"\"b\"'", "'<&>'", "7", "-0", "NaN", "1n", "Symbol('q')", "logged('arg')", "toPrim('arg')",
  "thrower", "valueThrower", "[1, 2]", "{}", "'x', 'y'",
];

for (const [name] of plain) {
  for (const [recv] of receivers) {
    add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(${recv})); R = r + '|' + log.join()`));
  }
  for (const bad of badThis) add(program(`R = t(() => String.prototype.${name}.call(${bad}))`));
  add(program(`R = t(() => String.prototype.${name}.call(undefined, 'ignored'))`));
  add(program(`R = JSON.stringify([String.prototype.${name}.length, String.prototype.${name}.name, typeof String.prototype.${name}])`));
  add(program(`R = JSON.stringify(Object.getOwnPropertyDescriptor(String.prototype, '${name}'), (k, v) => typeof v === 'function' ? 'fn' : v)`));
  add(program(`R = t(() => new String.prototype.${name})`));
  add(program(`R = t(() => ${name === "sub" ? "'abc'.sub()" : `'abc'.${name}()`}) + '|' + t(() => 'abc'.${name}('arg'))`));
}

for (const [name] of attr) {
  for (const [recv, label] of receivers.slice(0, 7)) {
    for (const arg of attrArgs.slice(0, 11)) {
      if (label !== "objeto" && label !== "vazia" && label !== "especiais" && label !== "abc" && arg !== "'x'" && arg !== "") continue;
      add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(${recv}${arg === "" ? "" : ", " + arg})); R = r + '|' + log.join()`));
    }
  }
  for (const arg of attrArgs.slice(11)) {
    add(program(`log.length = 0; var r = t(() => 'abc'.${name}(${arg})); R = r + '|' + log.join()`));
    add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(logged('this'), ${arg})); R = r + '|' + log.join()`));
  }
  for (const bad of badThis) {
    add(program(`R = t(() => String.prototype.${name}.call(${bad}, 'x'))`));
    add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(${bad}, logged('arg'))); R = r + '|' + log.join()`));
  }
  add(program(`R = JSON.stringify([String.prototype.${name}.length, String.prototype.${name}.name])`));
  add(program(`R = JSON.stringify(Object.getOwnPropertyDescriptor(String.prototype, '${name}'), (k, v) => typeof v === 'function' ? 'fn' : v)`));
  add(program(`R = t(() => new String.prototype.${name})`));
  add(program(`R = t(() => String.prototype.${name}.call('abc', 'x', 'y', 'z'))`));
}

// Homogeneidade de RegExp e nomes de tag: a ordem é this primeiro, argumento depois, e Symbol lança no this antes de olhar o arg.
add(program("log.length = 0; var r = t(() => String.prototype.anchor.call(logged('T'), logged('A'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var r = t(() => String.prototype.fontsize.call(Symbol('s'), logged('A'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var r = t(() => String.prototype.link.call(thrower, logged('A'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var r = t(() => String.prototype.link.call(logged('T'), thrower)); R = r + '|' + log.join()"));
add(program("R = JSON.stringify(Object.getOwnPropertyNames(String.prototype).filter(k => /^(anchor|big|blink|bold|fixed|fontcolor|fontsize|italics|link|small|strike|sub|sup|substr|trimLeft|trimRight)$/.test(k)))"));
add(program("R = 'x'.anchor('a').anchor('b')"));
add(program("R = '<b>'.bold().italics().link('u')"));
add(program("R = 'a'.repeat(3).sup().length + '|' + ''.sub().length"));
add(program("var s = Symbol('d'); R = t(() => 'a'.anchor(s)) + '|' + t(() => 'a'.anchor(Symbol.iterator))"));
add(program("String.prototype.toString = function () { return 'mudou' }; R = 'abc'.bold()"));
add(program("var o = { toString() { return 'ts' } }; R = String.prototype.link.call(o, o)"));
add(program("R = 'x'.link('\"\"') + 'x'.link('&quot;') + 'x'.link('\\\"')"));

// ---------------------------------------------------------------------------------------------
// 2. trimLeft, trimRight, substr e search (bordas que o golden de string não alcança).
// ---------------------------------------------------------------------------------------------
add(program("R = JSON.stringify([String.prototype.trimLeft === String.prototype.trimStart, String.prototype.trimRight === String.prototype.trimEnd])"));
add(program("R = JSON.stringify([String.prototype.trimLeft.name, String.prototype.trimRight.name, String.prototype.trimLeft.length, String.prototype.trimRight.length])"));
add(program("R = JSON.stringify(Object.getOwnPropertyDescriptor(String.prototype, 'trimLeft'), (k, v) => typeof v === 'function' ? 'fn' : v)"));
add(program("R = JSON.stringify(Object.getOwnPropertyDescriptor(String.prototype, 'trimRight'), (k, v) => typeof v === 'function' ? 'fn' : v)"));
const spaces = ["' \\t\\n\\v\\f\\r abc \\u00a0\\u1680\\u2000\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000\\ufeff'", "'\\u180eabc\\u180e'", "'\\u200babc\\u200b'", "'\\u0085abc\\u0085'", "''", "'   '", "'a b'", "'\\ufeff\\u2028'"];
for (const name of ["trimLeft", "trimRight", "trimStart", "trimEnd", "trim"]) {
  for (const s of spaces) add(program(`R = t(() => ${s}.${name}()) + '|' + ${s}.${name}().length`));
  for (const bad of badThis) add(program(`R = t(() => String.prototype.${name}.call(${bad}))`));
  add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(logged('this'))); R = r + '|' + log.join()`));
  add(program(`log.length = 0; var r = t(() => String.prototype.${name}.call(toPrim('this'))); R = r + '|' + log.join()`));
  add(program(`R = t(() => String.prototype.${name}.call(12)) + t(() => String.prototype.${name}.call(-0)) + t(() => String.prototype.${name}.call([' a ', 'b ']))`));
  add(program(`R = t(() => new "".${name})`));
}
const substrArgs = [
  "", "0", "1", "-1", "-100", "100", "NaN", "Infinity", "-Infinity", "-0", "1.9", "-1.9", "'1'", "'x'", "null", "undefined", "2 ** 32", "2 ** 53",
  "1, 2", "1, -1", "1, NaN", "1, Infinity", "1, undefined", "1, null", "-2, 1", "0, 0", "0, -0", "5, 1", "1, 2 ** 32", "-Infinity, Infinity",
  "logged('a'), logged('b')", "toPrim('a'), toPrim('b')", "thrower", "1, thrower", "1n", "Symbol('s')", "1, Symbol('s')",
];
for (const args of substrArgs) {
  add(program(`log.length = 0; var r = t(() => 'abcdef'.substr(${args})); R = r + '|' + log.join()`));
  add(program(`log.length = 0; var r = t(() => String.prototype.substr.call(logged('this'), ${args})); R = r + '|' + log.join()`));
}
for (const bad of badThis) add(program(`R = t(() => String.prototype.substr.call(${bad}, 1))`));
add(program("R = JSON.stringify([String.prototype.substr.length, String.prototype.substr.name, 'a\\ud83d\\ude00b'.substr(1, 1).length, 'a\\ud83d\\ude00b'.substr(2, 1).length])"));
const searchArgs = [
  "", "undefined", "null", "'b'", "/b/", "/b/g", "/(?:)/", "'.'", "'['", "7", "{}", "[]", "Symbol('s')", "logged('a')", "toPrim('a')", "thrower",
  "{ [Symbol.search](s) { log.push('search:' + s); return 42 } }", "{ [Symbol.search]: null, toString() { return 'a' } }", "{ [Symbol.search]: 1 }",
  "{ get [Symbol.search]() { throw new EvalError('getter lançou') } }", "/a/y", "/b/d",
];
for (const arg of searchArgs) {
  add(program(`log.length = 0; var r = t(() => 'abcabc'.search(${arg})); R = r + '|' + log.join()`));
}
for (const bad of badThis) add(program(`R = t(() => String.prototype.search.call(${bad}, 'a'))`));
add(program("var re = /b/g; re.lastIndex = 5; var r = 'abc'.search(re); R = r + '|' + re.lastIndex"));
add(program("var re = /b/y; re.lastIndex = 1; var r = 'abc'.search(re); R = r + '|' + re.lastIndex"));
add(program("var re = /x/; re.lastIndex = 3; R = 'abc'.search(re) + '|' + re.lastIndex"));
add(program("var o = { lastIndex: 5, exec() { return null } }; R = t(() => RegExp.prototype[Symbol.search].call(o, 'abc')) + '|' + o.lastIndex"));
add(program("R = t(() => RegExp.prototype[Symbol.search].call(1, 'abc')) + '|' + t(() => RegExp.prototype[Symbol.search].call(undefined, 'abc'))"));

// ---------------------------------------------------------------------------------------------
// 3. RegExp: getters de flag, flags, source, toString, compile.
// ---------------------------------------------------------------------------------------------
const flagGetters = [
  ["hasIndices", "d"], ["global", "g"], ["ignoreCase", "i"], ["multiline", "m"], ["dotAll", "s"], ["unicode", "u"], ["unicodeSets", "v"], ["sticky", "y"],
];
const getterOf = name => `Object.getOwnPropertyDescriptor(RegExp.prototype, '${name}').get`;
for (const [name, letter] of flagGetters) {
  add(program(`R = JSON.stringify([/a/${letter}.${name}, /a/.${name}, RegExp.prototype.${name}])`));
  add(program(`R = JSON.stringify([new RegExp('a', '${letter}').${name}, new RegExp('a', 'gimsuy'.replace('${letter}', '')).${name}])`).replace(/'gimsuy'\.replace\('v', ''\)/, "'gimsuy'"));
  for (const recv of ["{}", "[]", "1", "'a'", "null", "undefined", "Symbol('s')", "function () {}", "Object.create(RegExp.prototype)", "new Proxy(/a/, {})", "{ [Symbol.match]: true }", "new (class extends RegExp {})('a', '" + letter + "')", "Object.assign(/a/, { " + name + ": 'shadow' })"]) {
    add(program(`R = t(() => ${getterOf(name)}.call(${recv}))`));
  }
  add(program(`var d = Object.getOwnPropertyDescriptor(RegExp.prototype, '${name}'); R = JSON.stringify([typeof d.get, d.set, d.enumerable, d.configurable, 'value' in d, d.get.name, d.get.length])`));
  add(program(`R = t(() => ${getterOf(name)}.call())`));
  add(program(`R = JSON.stringify(Object.getOwnPropertyNames(/a/${letter}).concat(Object.keys(/a/${letter})))`));
  add(program(`var re = /a/${letter}; Object.defineProperty(re, '${name}', { value: 'own' }); R = JSON.stringify([re.${name}, re.flags])`));
  add(program(`R = t(() => { 'use strict'; /a/.${name} = true; return 'sem erro' })`));
  add(program(`R = t(() => { 'use strict'; delete RegExp.prototype.${name}; return String(RegExp.prototype.${name}) })`));
}
add(program("R = JSON.stringify([/a/dgimsuy.flags, /a/v.flags, /a/yvsmigd.flags, /a/.flags, RegExp('a', 'ydgsmi').flags, RegExp.prototype.flags])"));
add(program("R = t(() => new RegExp('a', 'gg')) + '|' + t(() => new RegExp('a', 'uv')) + '|' + t(() => new RegExp('a', 'x')) + '|' + t(() => new RegExp('a', 'G'))"));
add(program("R = t(() => RegExp('a', 'i i')) + '|' + t(() => RegExp('a', ' ')) + '|' + t(() => RegExp('a', '\\u0067')) + '|' + t(() => RegExp('a', 'ｇ'))"));
add(program("R = t(() => new RegExp('a', undefined).flags) + t(() => new RegExp('a', null)) + t(() => new RegExp('a', 1)) + t(() => new RegExp('a', { toString() { return 'gi' } }).flags)"));
add(program("R = t(() => new RegExp('a', Symbol('s'))) + '|' + t(() => new RegExp('a', 1n))"));
add(program("log.length = 0; var r = t(() => new RegExp(logged('p'), logged('gi'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var r = t(() => new RegExp(toPrim('p'), toPrim('g'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var r = t(() => new RegExp(thrower, logged('f'))); R = r + '|' + log.join()"));
const flagsGetter = getterOf("flags");
add(program(`var flagsOf = ${flagsGetter}; R = t(() => flagsOf.call({})) + '|' + t(() => flagsOf.call({ global: 1, sticky: 'x', dotAll: [] , hasIndices: 'a'})) + '|' + t(() => flagsOf.call(1)) + '|' + t(() => flagsOf.call(null))`));
add(program(`var flagsOf = ${flagsGetter}; log.length = 0; var o = {}; for (var k of ['hasIndices', 'global', 'ignoreCase', 'multiline', 'dotAll', 'unicode', 'unicodeSets', 'sticky']) { (function (k) { Object.defineProperty(o, k, { get() { log.push(k); return true } }) })(k) } var r = flagsOf.call(o); R = r + '|' + log.join()`));
add(program(`var flagsOf = ${flagsGetter}; var o = { get global() { throw new EvalError('global lançou') }, get sticky() { log.push('sticky'); return 1 } }; log.length = 0; R = t(() => flagsOf.call(o)) + '|' + log.join()`));
add(program(`var flagsOf = ${flagsGetter}; var o = { get hasIndices() { log.push('d'); return 1 }, get sticky() { throw new TypeError('sticky lançou') } }; log.length = 0; R = t(() => flagsOf.call(o)) + '|' + log.join()`));
add(program(`var flagsOf = ${flagsGetter}; R = t(() => flagsOf.call(new Proxy({}, { get(t, k) { log.push(String(k)); return k === 'global' } }))) + '|' + log.join()`));
add(program(`var flagsOf = ${flagsGetter}; R = JSON.stringify([flagsOf.call({ global: 0, ignoreCase: '', multiline: NaN, dotAll: null, unicode: undefined }), flagsOf.call({ global: 'false' }), flagsOf.call({ unicode: {}, unicodeSets: [], sticky: -0 })])`));
add(program("var re = /a/g; Object.defineProperty(re, 'flags', { value: 'zzz' }); R = re.flags + '|' + re.toString() + '|' + Object.prototype.toString.call(re)"));
add(program("var re = /a/gi; re.global = false; R = re.flags + '|' + re.global"));
add(program("class R2 extends RegExp { get global() { return false } }; R = JSON.stringify([new R2('a', 'g').flags, new R2('a', 'g').global, new R2('a', 'g').toString()])"));
add(program("class R2 extends RegExp { get flags() { return 'xyz' } }; R = JSON.stringify([new R2('a', 'g').flags, new R2('a', 'g').toString(), String(new R2('a', 'g'))])"));
add(program("var d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags'); R = JSON.stringify([typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length])"));

// source
const sourceGetter = getterOf("source");
add(program("R = JSON.stringify([RegExp.prototype.source, new RegExp('').source, new RegExp('/').source, new RegExp('\\\\/').source, new RegExp('\\n').source, new RegExp('\\r').source, new RegExp('\\u2028').source, new RegExp('\\u2029').source, new RegExp('[/]').source, new RegExp('\\\\\\n').source])"));
add(program("R = JSON.stringify([/a\\/b/.source, /[/]/.source, /\\//.source, RegExp('a/b').source, RegExp('a\\\\/b').source, RegExp('[\\\\/]').source, RegExp('\\\\\\\\/').source])"));
add(program("R = JSON.stringify([new RegExp('a\\nb').source, new RegExp('a\\\\nb').source, new RegExp('(?:)').source, new RegExp('\\\\').source === undefined])").replace("new RegExp('\\\\').source === undefined", "1"));
add(program(`R = t(() => ${sourceGetter}.call({})) + '|' + t(() => ${sourceGetter}.call(1)) + '|' + t(() => ${sourceGetter}.call(RegExp.prototype)) + '|' + t(() => ${sourceGetter}.call(null))`));
add(program(`R = t(() => ${sourceGetter}.call(Object.create(RegExp.prototype))) + '|' + t(() => ${sourceGetter}.call(new Proxy(/a/, {})))`));
add(program("R = RegExp(undefined).source + '|' + RegExp(null).source + '|' + RegExp(1e21).source + '|' + RegExp([1, 2]).source + '|' + RegExp({}).source + '|' + RegExp(-0).source"));
add(program("R = t(() => RegExp(Symbol('s'))) + '|' + t(() => RegExp(1n).source)"));
add(program("R = new RegExp(/a\\/b/g).source + '|' + new RegExp(/a/g, 'i').flags + '|' + new RegExp(/a/g, '').flags + '|' + new RegExp(/a/g, undefined).flags"));
add(program("var re = /a/g; var r2 = RegExp(re); R = (r2 === re) + '|' + (new RegExp(re) === re) + '|' + (RegExp(re, 'g') === re)"));
add(program("var re = /a/g; re.constructor = Object; R = (RegExp(re) === re) + '|' + (RegExp(re, 'g') === re)"));
add(program("var o = { [Symbol.match]: true, source: 'x', flags: 'g', constructor: RegExp }; R = (RegExp(o) === o) + '|' + RegExp(o, 'i').flags + '|' + RegExp(o).source"));
add(program("var o = { [Symbol.match]: true, source: 'x', flags: 'g' }; var r = RegExp(o); R = r.source + '|' + r.flags + '|' + (r === o)"));
add(program("var o = { [Symbol.match]: true, get source() { throw new EvalError('source lançou') } }; R = t(() => new RegExp(o))"));
add(program("var o = { [Symbol.match]: true, source: 'x', get flags() { throw new EvalError('flags lançou') } }; R = t(() => new RegExp(o))"));

// toString
add(program("R = JSON.stringify([RegExp.prototype.toString.call(/a\\/b/gi), RegExp.prototype.toString.call({ source: 'S', flags: 'F' }), RegExp.prototype.toString.call({}), String(RegExp.prototype), String(new RegExp(''))])"));
add(program("R = t(() => RegExp.prototype.toString.call(1)) + '|' + t(() => RegExp.prototype.toString.call(null)) + '|' + t(() => RegExp.prototype.toString.call(undefined)) + '|' + t(() => RegExp.prototype.toString.call('a')) + '|' + t(() => RegExp.prototype.toString.call(Symbol('s')))"));
add(program("log.length = 0; var o = { get source() { log.push('source'); return 'S' }, get flags() { log.push('flags'); return 'F' } }; R = RegExp.prototype.toString.call(o) + '|' + log.join()"));
add(program("var o = { source: logged('s'), flags: logged('f') }; log.length = 0; R = RegExp.prototype.toString.call(o) + '|' + log.join()"));
add(program("var o = { source: thrower, flags: 'f' }; R = t(() => RegExp.prototype.toString.call(o))"));
add(program("var o = { source: 's', flags: Symbol('f') }; R = t(() => RegExp.prototype.toString.call(o))"));
add(program("var o = { source: undefined, flags: undefined }; R = RegExp.prototype.toString.call(o)"));
add(program("R = JSON.stringify([RegExp.prototype.toString.length, RegExp.prototype.toString.name, RegExp.prototype.compile.length, RegExp.prototype.compile.name])"));

// compile
add(program("var re = /a/g; var r = re.compile('b', 'i'); R = JSON.stringify([r === re, re.source, re.flags, re.lastIndex])"));
add(program("var re = /a/g; re.lastIndex = 3; re.compile('b'); R = JSON.stringify([re.source, re.flags, re.lastIndex])"));
add(program("var re = /a/gy; re.lastIndex = 3; re.compile(); R = JSON.stringify([re.source, re.flags, re.lastIndex])"));
add(program("var re = /a/gy; re.compile(undefined, undefined); R = JSON.stringify([re.source, re.flags])"));
add(program("var re = /a/gy; re.compile(undefined, 'i'); R = JSON.stringify([re.source, re.flags])"));
add(program("var re = /a/gy; re.compile('z', undefined); R = JSON.stringify([re.source, re.flags])"));
add(program("var re = /a/g; var other = /b/i; re.compile(other); R = JSON.stringify([re.source, re.flags])"));
add(program("var re = /a/g; R = t(() => re.compile(/b/i, 'g')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile(/b/i, undefined)) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('(', 'g')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('b', 'zz')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('b', 'gg')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('b', 'uv')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('b', 'dgimsuy')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('b', 'v')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('\\\\p{L}', 'u')) + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile('[a&&b]', 'v')) + '|' + re.source + '|' + re.flags"));
for (const recv of ["{}", "[]", "1", "'a'", "null", "undefined", "Symbol('s')", "function () {}", "Object.create(RegExp.prototype)", "new Proxy(/a/, {})", "RegExp.prototype", "{ [Symbol.match]: true }"]) {
  add(program(`R = t(() => RegExp.prototype.compile.call(${recv}, 'a'))`));
}
add(program("class R2 extends RegExp {}; var re = new R2('a', 'g'); var r = re.compile('b', 'i'); R = JSON.stringify([r === re, re.source, re.flags, re instanceof R2])"));
add(program("var re = /a/g; Object.defineProperty(re, 'lastIndex', { writable: false }); R = t(() => re.compile('b')) + '|' + re.source"));
add(program("var re = /a/g; Object.freeze(re); R = t(() => re.compile('b')) + '|' + re.source + '|' + re.lastIndex"));
add(program("var re = /a/g; re.lastIndex = { valueOf() { log.push('li'); return 2 } }; log.length = 0; re.compile('b'); R = JSON.stringify([re.lastIndex, log.join()])"));
add(program("log.length = 0; var re = /a/g; var r = t(() => re.compile(logged('p'), logged('i'))); R = r + '|' + log.join() + '|' + re.source + '|' + re.flags"));
add(program("log.length = 0; var re = /a/g; var r = t(() => re.compile(toPrim('p'), toPrim('i'))); R = r + '|' + log.join()"));
add(program("log.length = 0; var re = /a/g; var r = t(() => re.compile(thrower, logged('i'))); R = r + '|' + log.join() + '|' + re.source"));
add(program("log.length = 0; var re = /a/g; var r = t(() => re.compile('p', thrower)); R = r + '|' + re.source + '|' + re.flags"));
add(program("var re = /a/g; R = t(() => re.compile(Symbol('s'))) + '|' + t(() => re.compile('a', Symbol('s'))) + '|' + t(() => re.compile(1n, 1n))"));
add(program("var re = /a/g; re.compile(null); R = re.source + '|' + re.flags"));
add(program("var re = /a/g; re.compile(NaN, -0); R = t(() => re.source + '|' + re.flags)"));
add(program("var re = /a/g; re.compile(''); R = re.source + '|' + re.flags + '|' + re.toString()"));
add(program("var re = /a/g; re.compile('/'); R = re.source + '|' + re.toString()"));
add(program("var re = /a/g; re.compile('\\n'); R = re.source + '|' + re.toString()"));
add(program("var re = /(?<n>a)/; re.compile('(?<m>b)'); var m = re.exec('b'); R = JSON.stringify([m && m.groups && Object.keys(m.groups), m && m[0]])"));
add(program("var re = /a/; re.compile('b'); R = JSON.stringify([re.test('b'), re.test('a'), RegExp.lastMatch, RegExp['$&']])"));
add(program("var re = /a/g; re.compile('a', 'y'); re.lastIndex = 1; R = JSON.stringify([re.exec('ba'), re.lastIndex])"));
add(program("var re = /a/; var r = Object.getOwnPropertyDescriptor(re, 'lastIndex'); re.compile('b'); R = JSON.stringify([r, Object.getOwnPropertyDescriptor(re, 'lastIndex')])"));
add(program("var d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'compile'); R = JSON.stringify([typeof d.value, d.writable, d.enumerable, d.configurable])"));
add(program("var d = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.matchAll); R = JSON.stringify([typeof d.value, d.writable, d.enumerable, d.configurable, d.value.name, d.value.length])"));
add(program("R = JSON.stringify(Object.getOwnPropertyNames(RegExp.prototype))"));
add(program("R = JSON.stringify(Object.getOwnPropertyNames(RegExp).filter(k => !k.startsWith('$')))"));
add(program("R = JSON.stringify(Reflect.ownKeys(RegExp.prototype).filter(k => typeof k === 'symbol').map(String))"));

// ---------------------------------------------------------------------------------------------
// Execução no bun.
// ---------------------------------------------------------------------------------------------
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "builtins-gap-golden-"));
const file = path.join(dir, "builtins_gap_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
const lines = [];
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir });
  const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("builtins_gap", lines));
fs.rmSync(dir, { recursive: true, force: true });
