// Gera tests/golden/template_edge_bun.tsv: template literals e tagged templates (cache do template object por site,
// cooked vs raw com escapes inválidos, congelamento, formas de tag, ordem de avaliação das substituições,
// toString/Symbol.toPrimitive, quebras de linha CRLF/LS/PS, aninhamento, String.raw, sequências de escape) medidos no
// bun 1.4.2. Sem APIs de host. Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a
// gen-scope-golden.js. Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança); SyntaxError sai por
// eval indireto. Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Nos fontes, `§` vira crase e `¤` vira `${`, para o gerador não precisar escapar a si mesmo.
// Uso: bun scripts/gen-template-edge-golden.js > tests/golden/template_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const bt = s => s.replace(/§/g, "`").replace(/¤/g, "${");
const add = body => programs.push(bt(body));
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
const S = src => add(`try { (0, eval)(${JSON.stringify(bt(src))}); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }`);
const V = src => add(`try { R = String((0, eval)(${JSON.stringify(bt(src))})) } catch (e) { R = e.name + ': ' + e.message }`);
const J = "JSON.stringify";
const P = "var tg = (s, ...v) => s; var J = JSON.stringify; function cr(s, ...v) { return J([s.length, s.raw.length, s.slice(), s.raw.slice(), v]) } ";
// Corpo que começa com 'use strict' vira função estrita (diretiva só vale no topo de função ou script).
const TP = body =>
  body.startsWith("'use strict'; ")
    ? add(`(function () { 'use strict'; try { ${P}${body.slice(14)} } catch (e) { R = e.name + ': ' + e.message } })()`)
    : T(P + body);

// ---- 1. Cache do template object por site.
TP("function f() { return tg§a§ } R = f() === f()");
TP("function f() { return tg§a§ } R = f() === tg§a§");
TP("var a = tg§a§, b = tg§a§; R = a === b");
TP("var a = tg§a${1}b§, b = tg§a${1}b§; R = a === b");
TP("function f(x) { return tg§a${x}b§ } R = f(1) === f(2)");
TP("var l = []; for (var i = 0; i < 3; i++) l.push(tg§x§); R = [l[0] === l[1], l[1] === l[2]].join()");
TP("var l = []; for (let i = 0; i < 3; i++) l.push(tg§x${i}y§); R = (l[0] === l[2]) + ',' + l[0].raw.join()");
TP("var l = []; var i = 0; while (i++ < 4) l.push(tg§w§); R = new Set(l).size");
TP("var l = []; for (var k of [1, 2, 3]) l.push(tg§o§); R = new Set(l).size");
TP("var l = []; for (var k in {a:1,b:2}) l.push(tg§i§); R = new Set(l).size");
TP("var l = []; [1,2,3].forEach(() => l.push(tg§c§)); R = new Set(l).size");
TP("var l = []; var g = () => tg§c§; l.push(g(), g()); R = l[0] === l[1]");
TP("var a = () => tg§c§, b = () => tg§c§; R = a() === b()");
TP("var l = []; function* g() { while (true) yield tg§g§ } var it = g(); l.push(it.next().value, it.next().value); R = l[0] === l[1]");
TP("class C { static m() { return tg§m§ } } R = C.m() === C.m()");
TP("class C { m() { return tg§m§ } } R = new C().m() === new C().m()");
TP("var o = { m() { return tg§m§ } }; R = o.m() === o.m()");
TP("function mk() { return function () { return tg§x§ } } var a = mk(), b = mk(); R = a() === b()");
TP("function mk() { return () => tg§x§ } R = mk()() === mk()()");
TP("var f = new Function('tg', 'return tg§a§'); R = f(tg) === f(tg)");
TP("var f = Function('tg', 'return tg§a§'); var g = Function('tg', 'return tg§a§'); R = f(tg) === g(tg)");
TP("var l = []; for (var i = 0; i < 3; i++) l.push(eval('tg§e§')); R = new Set(l).size");
TP("var l = []; for (var i = 0; i < 3; i++) l.push((0, eval)('tg§e§')); R = new Set(l).size");
TP("var l = []; var src = 'tg§e§'; l.push(eval(src), eval(src)); R = l[0] === l[1]");
TP("var l = []; var src = 'tg§e§'; l.push((0, eval)(src), (0, eval)(src)); R = l[0] === l[1]");
TP("function f() { return eval('tg§e§') } R = f() === f()");
TP("var f = eval('(function () { return tg§z§ })'); R = f() === f()");
TP("var f = (0, eval)('(function (tg) { return tg§z§ })'); R = f(tg) === f(tg)");
TP("var s = tg§a§; var t = tg§a§; R = s === t || s.raw === t.raw");
TP("var s = tg§a§; R = tg§a§.raw === s.raw");
TP("function f() { return tg§r§.raw } R = f() === f()");
TP("var a = tg§1§, b = tg§1§; R = [a === b, a[0] === b[0]].join()");
TP("var a = []; function f() { a.push(tg§m§); if (a.length < 3) f() } f(); R = a[0] === a[1] && a[1] === a[2]");
TP("var a = [tg§x§, tg§x§]; R = a[0] === a[1]");
TP("var a = [1, 2].map(() => tg§x§); R = a[0] === a[1]");
TP("var a = [1, 2].map(() => [tg§x§, tg§x§]); R = [a[0][0] === a[1][0], a[0][0] === a[0][1]].join()");
TP("var a = []; var f = x => a.push(x); f(tg§p§); f(tg§p§); R = a[0] === a[1]");
TP("var a = []; for (var i = 0; i < 2; i++) { (function () { a.push(tg§q§) })() } R = a[0] === a[1]");
TP("var a = []; for (var i = 0; i < 2; i++) { a.push(new Function('tg', 'return tg§q§')(tg)) } R = a[0] === a[1]");
TP("var o = { a: tg§k§, b: tg§k§ }; R = o.a === o.b");
TP("var o = { get g() { return tg§k§ } }; R = o.g === o.g");
TP("class C { x = tg§k§; static y = tg§k§ } var a = new C(), b = new C(); R = [a.x === b.x, a.x === C.y].join()");
TP("function f(a = tg§d§) { return a } R = f() === f()");
TP("function f(x = tg§d§) { return x } R = f() === f(tg§d§)");
TP("var a = []; label: for (var i = 0; i < 3; i++) { a.push(tg§l§); continue label } R = new Set(a).size");
TP("var a = []; switch (1) { case 1: a.push(tg§s§); default: a.push(tg§s§) } R = a[0] === a[1]");
TP("var a = []; try { a.push(tg§t§) } finally { a.push(tg§t§) } R = a[0] === a[1]");
TP("var f = tg§a§.constructor; R = f === Array");
TP("var a = tg§a§; a.x = 1; R = 'x' in a");
TP("var a = tg§a§; R = tg§a§.x");
TP("var a = tg§${1}§, b = tg§${2}§; R = a === b");
TP("var a = tg§ ${1} §, b = tg§ ${2} §; R = a === b");

// ---- 2. Cooked vs raw com escapes (válidos e inválidos).
const escapes = [
  "\\u", "\\u1", "\\u12", "\\u123", "\\u1234", "\\u{", "\\u{}", "\\u{1", "\\u{g}", "\\u{110000}", "\\u{10FFFF}", "\\u{41}", "\\u{0000041}", "\\uD83D", "\\uD83D\\uDE00",
  "\\x", "\\x1", "\\xg1", "\\x41", "\\xZZ", "\\0", "\\00", "\\01", "\\07", "\\08", "\\1", "\\7", "\\8", "\\9", "\\10", "\\377", "\\400", "\\a", "\\b", "\\v", "\\f", "\\n", "\\r",
  "\\t", "\\\\", "\\§", "\\${", "\\$", "\\'", "\\\"", "\\z", "\\\n", "\\\r\n", "\\\r", "\\ ", "\\ ", "\\ ", "\\/", "\\e", "\\u00e9", "\\xe9", "\\0\\1",
];
for (const e of escapes) {
  TP(`R = cr§a${e}b§`);
  if (/[\n\r\u2028\u2029]/.test(e)) continue;
  S(`§a${e}b§`);
}
TP("function t(s) { return String(s[0]) + '|' + s.raw[0] } R = t§\\u§ + t§\\xz§ + t§\\01§");
TP("function t(s) { return s[0] === undefined } R = [t§\\u§, t§\\u{§, t§\\xg§, t§\\9§, t§ok§].join()");
TP("function t(s, ...v) { return J([s, s.raw, v]) } R = t§\\u${1}\\x${2}\\8§");
TP("function t(s) { return s.hasOwnProperty(0) + ',' + (0 in s) + ',' + s.length } R = t§\\u§");
TP("function t(s) { return J(Object.getOwnPropertyDescriptor(s, 0)) } R = t§\\u§");
TP("function t(s) { return J(Object.keys(s)) + J(Object.getOwnPropertyNames(s)) } R = t§a${1}\\u§");
S("tg§\\u§");
S("§\\u§");
S("(§\\u§)");
S("tg§\\u${1}§");
S("tg§${1}\\u§");
S("tg§${§\\u§}§");
S("tg§${tg§\\u§}§");
S("§${tg§\\u§}§");
S("tg§\\u§§\\x§");
S("(function () { 'use strict'; return tg§\\01§ })()");
S("(function () { 'use strict'; return §\\01§ })()");
S("(function () { return §\\01§ })()");
S("§\\8§");
S("§\\9§");
S("tg§\\8§");
S("§\\0§");
S("§\\00§");
S("§\\u{110000}§");
S("§\\u{10FFFF}§");
V("tg§\\u§[0]");
V("tg§\\u§.raw[0]");
V("tg§\\u{110000}§.raw[0]");
V("§\\0§.length");
V("§\\u{1F600}§.length");
V("§\\u{1F600}§.codePointAt(0)");
V("§\\uD83D\\uDE00§ === §\\u{1F600}§");
V("tg§\\uD83D\\uDE00§.raw[0]");
V("tg§\\u{1F600}§.raw[0]");
V("tg§\\u0041§[0] + tg§\\u0041§.raw[0]");

// ---- 3. Congelamento.
TP("var s = tg§a${1}b§; R = [Object.isFrozen(s), Object.isFrozen(s.raw), Object.isSealed(s), Object.isExtensible(s), Object.isExtensible(s.raw)].join()");
TP("var s = tg§§; R = [Object.isFrozen(s), Object.isFrozen(s.raw), s.length].join()");
TP("var s = tg§a§; s[0] = 'z'; R = s[0]");
TP("'use strict'; var s = tg§a§; try { s[0] = 'z' } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { s.push(1) } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { s.length = 0 } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { s.raw[0] = 'z' } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { s.raw = 1 } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { s.x = 1 } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { delete s[0] } catch (e) { R = e.name + ': ' + e.message }");
TP("'use strict'; var s = tg§a§; try { delete s.raw } catch (e) { R = e.name + ': ' + e.message }");
TP("var s = tg§a§; R = J(Object.getOwnPropertyDescriptor(s, 'raw'))");
TP("var s = tg§a§; R = J(Object.getOwnPropertyDescriptor(s, 'length'))");
TP("var s = tg§a§; R = J(Object.getOwnPropertyDescriptor(s.raw, 0)) + J(Object.getOwnPropertyDescriptor(s.raw, 'length'))");
TP("var s = tg§a§; R = [Array.isArray(s), Array.isArray(s.raw), Object.getPrototypeOf(s) === Array.prototype, Object.getPrototypeOf(s.raw) === Array.prototype, s === s.raw].join()");
TP("var s = tg§a§; R = Object.getOwnPropertyNames(s).join() + '|' + Object.getOwnPropertyNames(s.raw).join()");
TP("var s = tg§a§; R = J(Reflect.ownKeys(s))");
TP("var s = tg§a§; R = Reflect.defineProperty(s, 'x', { value: 1 })");
TP("var s = tg§a§; R = Reflect.set(s, 0, 'z')");
TP("var s = tg§a§; R = Reflect.deleteProperty(s, 0)");
TP("var s = tg§a§; R = Reflect.preventExtensions(s)");
TP("var s = tg§a§; R = Object.getOwnPropertyDescriptor(s, 'raw').enumerable");
TP("var s = tg§a§; R = Object.keys(s).join() + '|' + J(s) + '|' + J(s.raw)");
TP("var s = tg§a${1}b${2}c§; R = s.length + ',' + s.raw.length + ',' + s.map(x => x.toUpperCase()).join() + ',' + [...s.raw].join('-')");
TP("var s = tg§a§; R = Object.isFrozen(s.concat([1])) + ',' + Object.isFrozen([...s]) + ',' + Object.isFrozen(s.slice())");
TP("var s = tg§a§; R = s instanceof Array");
TP("var s = tg§a§; R = typeof s.raw + Object.prototype.toString.call(s.raw)");
TP("var s = tg§a§; R = s.raw.toString() + s.toString()");

// ---- 4. Formas de tag: membro, chamada, new, optional chain.
TP("var o = { t(s) { return this === o } }; R = o.t§x§");
TP("var o = { t(s) { return this === o } }; R = o['t']§x§");
TP("var o = { t(s) { return this === o } }; R = (o.t)§x§");
TP("var o = { t(s) { return this === o } }; R = (0, o.t)§x§ + ''");
TP("var o = { t(s) { return typeof this } }; R = (0, o.t)§x§");
TP("var o = { t(s) { 'use strict'; return typeof this } }; R = (0, o.t)§x§");
TP("var o = { t(s) { 'use strict'; return typeof this } }; R = (o.t = o.t)§x§");
TP("var o = { a: { t(s) { return this === o.a } } }; R = o.a.t§x§");
TP("var o = { t(s) { return this === o } }; R = (o?.t)§x§");
TP("function t(s) { return this === globalThis } R = t§x§");
TP("function t(s) { 'use strict'; return this } R = t§x§");
TP("var t = s => this; R = typeof t§x§");
TP("function mk() { return function (s) { return s.raw[0] } } R = mk()§a§");
TP("function mk() { return function (s) { return s.raw[0] + 'b' } } R = mk()§a§");
TP("function t(s) { return function (u) { return s.raw[0] + u.raw[0] } } R = t§a§§b§");
TP("function t(s) { return t2 } function t2(s) { return s.raw[0] } R = t§a§§b§");
TP("function t(s, ...v) { return v.length ? t : s.raw.join('') } R = t§a§§b§§c§");
TP("function t(s) { return this } var o = { t }; R = o.t§a§ === o");
TP("function t(s) { return new.target } R = t§a§");
TP("function T(s) { this.s = s.raw[0]; } R = new T§a§ instanceof T");
TP("function T(s) { this.s = s.raw[0]; } R = new T§a§.s");
TP("function T(s) { this.s = s.raw[0]; } R = (new T)§a§ + ''");
TP("var o = { T: function (s) { this.s = s.raw[0] } }; R = new o.T§a§ instanceof o.T");
TP("var o = { T: function (s) { this.s = s.raw[0] } }; R = (new o.T§a§).s");
TP("var o = { T: function (s) { this.s = s.raw[0] } }; R = new o.T§a§§b§ + ''");
TP("function T(s) { return function (u) { this.u = u.raw[0] } } R = new (T§a§)§b§ instanceof Object");
TP("var t = (s, ...v) => J(v); R = t§a${1}${2}§");
TP("function t() { return arguments.length + ':' + J(arguments[0].raw) } R = t§a${1}b${2}c§");
TP("function t(s) { return s } var o = { t }; R = o.t§a§ === t§a§");
TP("class C { static t(s) { return this === C } } R = C.t§x§");
TP("class C { t(s) { return this instanceof C } } R = new C().t§x§");
TP("class B { t(s) { return 'B' + s.raw[0] } } class C extends B { t(s) { return super.t§y§ } } R = new C().t§x§");
TP("class B { t(s) { return this instanceof C } } class C extends B { m() { return super.t§y§ } } R = new C().m()");
TP("var o = { t(s) { return s.raw[0] }, get g() { return this.t } }; R = o.g§z§");
TP("async function t(s) { return s.raw[0] } R = t§a§ instanceof Promise");
TP("function* t(s) { yield s.raw[0] } R = Object.prototype.toString.call(t§a§)");
TP("var b = function (s) { return this.v + s.raw[0] }.bind({ v: 'B' }); R = b§x§");
TP("R = String.raw§a\\n${1}§");
TP("R = String.raw§§ === ''");
TP("R = (0, String.raw)§a${1}b§");
TP("var f = String.raw; R = f§\\u§");
TP("R = [1, 2].map§x§ + ''");
TP("R = Array§a§.length");
TP("R = Object§a§[0]");
TP("R = typeof Symbol§a§");
TP("R = Function.prototype.call§a§");
TP("R = Math.max§a§");
S("a?.b§x§");
S("a?.§x§");
S("a?.b.c§x§");
S("a?.[0]§x§");
S("a?.()§x§");
S("(a?.b)§x§");
S("var a = { b(s) { return 1 } }; (a?.b)§x§");
S("var a = { b(s) { return 1 } }; a.b?.§x§");
S("var a = { b(s) { return 1 } }; a?.b§x§");
S("var a = { b(s) { return 1 } }; a?.b.c§x§");
S("var a = { b: { c(s) { return 1 } } }; a?.b.c§x§");
S("new a?.b§x§");
S("new a?.b()");
S("new a§x§?.b");
S("a§x§?.b");
S("a§x§§y§?.[0]");
S("a?.§x§§y§");
S("new.target§x§");
S("function f() { return new.target§x§ }");
S("import§x§");
S("import.meta§x§");
S("super§x§");
S("class C extends Object { m() { return super§x§ } }");
S("class C extends Object { constructor() { super§x§ } }");
S("a§x§ = 1");
S("a§x§++");
S("++a§x§");
S("delete a§x§");
S("(a§x§) => 1");
S("a§x§ => 1");
S("async a§x§ => 1");
S("function f() { a§x§ }");
S("for (a§x§ of []);");
S("for (a§x§ in {});");
S("[a§x§] = [1]");
S("({ b: a§x§ } = { b: 1 })");
S("a§x§§y§ = 1");
S("typeof a§x§");
S("void a§x§");
S("0§x§");
S("1.5§x§");
S("'s'§x§");
S("null§x§");
S("true§x§");
S("[]§x§");
S("{}§x§");
S("({})§x§");
S("(function () {})§x§");
S("(() => 1)§x§");
S("() => 1§x§");
S("class {}§x§");
S("(class {})§x§");
S("/re/§x§");
S("this§x§");
S("(a, b)§x§");
S("a ? b§x§ : c");
S("yield§x§");
S("function* g() { yield§x§ }");
S("async function f() { await§x§ }");
S("async function f() { await tg§x§ }");
S("tg§x§.y");
S("tg§x§[0]");
S("tg§x§()");
S("tg§x§§y§.z");
S("tg§x§ ?? 1");
S("tg§x§ || 1");
S("tg§x§ + tg§y§");
S("a.b.c§x§");
S("a[0]§x§");
S("a[0]§x§[1]§y§");
S("a.b()§x§");
S("a()()§x§");
S("a§x§()§y§");
S("a.#p§x§");
S("class C { #p(s) { return 1 } m() { return this.#p§x§ } }");
S("class C { #p(s) { return 1 } m() { return this?.#p§x§ } }");
S("class C { #p = 1; m() { return this.#p§x§ } }");

// ---- 5. Ordem de avaliação e conversão das substituições.
TP("var l = []; var f = x => (l.push(x), x); R = §${f(1)}${f(2)}${f(3)}§ + l.join()");
TP("var l = []; var f = x => (l.push(x), x); var t = (s, ...v) => l.join(); R = t§${f(1)}${f(2)}§");
TP("var l = []; var t = (...a) => (l.push('call'), a); var f = x => (l.push(x), x); (l.push('tag'), t)§${f(1)}${f(2)}§; R = l.join()");
TP("var l = []; var o = { get t() { l.push('get'); return function () { l.push('call') } } }; var f = x => (l.push(x), x); o.t§${f(1)}§; R = l.join()");
TP("var l = []; var o = { toString() { l.push('s'); return 'S' }, valueOf() { l.push('v'); return 'V' } }; R = §${o}§ + l.join()");
TP("var l = []; var o = { toString() { l.push('s'); return 'S' }, valueOf() { l.push('v'); return 'V' } }; R = tg§${o}§.length + l.join()");
TP("var o = { toString() { return 'S' }, valueOf() { return 'V' } }; R = §a${o}b§");
TP("var o = { toString() { return {} }, valueOf() { return 'V' } }; R = §${o}§");
TP("var o = { toString() { return {} }, valueOf() { return {} } }; R = §${o}§");
TP("var o = { valueOf() { return 'V' } }; R = §${o}§");
TP("var o = { toString: null, valueOf() { return 'V' } }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive](h) { return h } }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive](h) { return h } }; R = §${o}§ + (o + '') + `${o}`");
TP("var o = { [Symbol.toPrimitive](h) { return 1 } }; R = §${o}§ + typeof §${o}§");
TP("var o = { [Symbol.toPrimitive]: null, toString() { return 'T' } }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive]: 1 }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive]() { return {} } }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive]() { return Symbol() } }; R = §${o}§");
TP("var o = { [Symbol.toPrimitive]() { throw new Error('boom') } }; R = §a${o}b§");
TP("var l = []; var o = { toString() { l.push('o'); throw new Error('o') } }; var p = { toString() { l.push('p'); return 'p' } }; try { §${p}${o}${p}§ } catch (e) { R = l.join() }");
TP("var l = []; var o = { toString() { l.push('o'); throw new Error('o') } }; var t = (s, ...v) => 'ok'; R = t§${o}§ + l.join()");
TP("R = §${Symbol()}§");
TP("R = tg§${Symbol()}§.length");
TP("R = §${Symbol.iterator.description}§");
TP("R = §${Object.create(null)}§");
TP("R = §${null}${undefined}${true}${false}§");
TP("R = §${-0}${0}${+0}${1e21}${1e-7}${0.1 + 0.2}${NaN}${Infinity}${-Infinity}§");
TP("R = §${1n}${-1n}${2n ** 64n}§");
TP("R = §${[1, [2, 3]]}${{}}${[]}${[null]}${() => 1}§");
TP("R = §${function f() { return 1 }}${class A {}}§");
TP("R = §${new Date(0).getTime()}${new Error('e')}${/re/g}§");
TP("R = §${new String('s')}${new Number(3)}${new Boolean(false)}${Object(1n)}§");
TP("R = §${[1,2,3].map(x => x * 2)}§");
TP("R = §${§${§${1}§}§}§");
TP("R = §a${§b${§c§}d§}e§");
TP("R = §${'}'}${\"{\"}${'`'}${'$'}${'${'}§");
TP("R = §${{ a: 1 }.a}${[1, 2][1]}${(1, 2)}§");
TP("R = §${ {}.x }§");
TP("var x = 1; R = §${x++}${x++}${x}§");
TP("var x = 1; R = §${x}${x = 5}${x}§");
TP("var i = 0; R = §${i++}${i++}§ + §${++i}§");
TP("var a = [1, 2]; R = §${a.pop()}${a.pop()}${a.pop()}§");
TP("var o = { get a() { return 'A' } }; R = §${o.a}${o.a}§");
TP("function f() { return §${arguments.length}${this === undefined}§ } R = f.call(undefined, 1, 2)");
TP("var o = { v: 'V', m() { return §${this.v}§ }, n() { return () => §${this.v}§ } }; R = o.m() + o.n()()");
TP("function* g() { var x = §a${yield 1}b${yield 2}c§; return x } var it = g(); it.next(); it.next('X'); R = it.next('Y').value");
TP("function* g() { return tg§${yield}§.length } var it = g(); it.next(); R = it.next(1).value");
TP("async function f() { return §${await 1}${await Promise.resolve(2)}§ } f().then(v => { R = v })");
TP("async function f() { var l = []; var t = (s, ...v) => l.push(...v); t§${await 1}${await 2}§; return l.join() } f().then(v => { R = v })");
TP("var x = 'o'; R = §${x}§.length + §${x}§.toUpperCase()");
TP("R = §a§.length + §${1}${2}§.length");
TP("R = typeof §§ + typeof §${1}§ + §§.length");
TP("R = §§ === '' && §${''}§ === ''");
TP("R = §a§ === 'a' && §${'a'}§ === 'a'");
TP("var s = 'a'; R = §${s}${s}§ === s + s");
TP("R = Object.prototype.toString.call(§a§)");
TP("R = §${1}§ + 1");
TP("R = §${1}§ * 1 + §${'2'}§ * 1");
TP("R = [§a§, §b§].join('')");
TP("R = (§a§).concat(§b§)");
TP("R = §a§?.length");
TP("R = §a§ in { a: 1 } ? 'in' : 'out'");
TP("var o = { a: 1 }; R = o[§a§] + o[§${'a'}§]");
TP("var o = { [§k${1}§]: 1 }; R = Object.keys(o).join()");
TP("var o = { §a§: 1 }");

// ---- 6. Quebras de linha CRLF, CR, LS, PS.
const nl = { LF: "\\n", CRLF: "\\r\\n", CR: "\\r", LS: "\\u2028", PS: "\\u2029" };
for (const [name, seq] of Object.entries(nl)) {
  const raw = seq === "\\n" ? "\n" : seq === "\\r\\n" ? "\r\n" : seq === "\\r" ? "\r" : seq === "\\u2028" ? " " : " ";
  TP(`var s = tg§a${raw}b§; R = J([s[0], s.raw[0], s[0].length, s.raw[0].length])`);
  TP(`var s = §a${raw}b§; R = J([s, s.length])`);
  TP(`R = cr§a${raw}${raw}b${raw}§`);
  TP(`R = cr§${raw}${"$"}{1}${raw}§`);
  TP(`R = cr§a\\${raw}b§`);
  TP(`R = cr§a\\${raw}${"$"}{1}\\${raw}b§`);
  TP(`R = String.raw§a${raw}b${"$"}{1}${raw}c§.length`);
  TP(`var s = tg§a${raw}b§; R = [...s.raw[0]].map(c => c.charCodeAt(0)).join()`);
  TP(`var s = tg§a${raw}b§; R = [...s[0]].map(c => c.charCodeAt(0)).join()`);
}
TP("var s = tg§a\r\n\r\nb§; R = J(s.raw[0])");
TP("var s = tg§a\r\r\nb\n\rc§; R = J(s.raw[0]) + J(s[0])");
TP("var s = tg§\\\r\n\\\r\\\n§; R = J(s.raw[0]) + J(s[0])");
TP("var s = tg§\\ x§; R = J(s.raw[0]) + J(s[0])");
TP("var s = tg§\\ x§; R = J(s.raw[0]) + J(s[0])");
TP("var s = tg§a b c§; R = J(s.raw[0]) + J(s[0]) + (s[0] === s.raw[0])");
TP("R = §\r\n§.length + §\r§.length + §\n§.length + § §.length");
TP("R = String.raw§\r\n§ === '\\n' ? 'lf' : 'crlf'");
TP("R = J(String.raw§a\r\nb\rc\nd§)");
TP("R = J(§line1\r\nline2\r\n§)");
TP("R = (function () { return §\r\n§ }).toString().length");
TP("R = eval('§a\\r\\nb§').length + eval('tg§a\\r\\nb§').raw[0].length");
TP("R = (0, eval)('(s => s.raw[0].length)§a\\r\\nb§')");
TP("R = (0, eval)('(s => s.raw[0].length)§a\\\\r\\\\nb§')");

// ---- 7. Aninhamento.
TP("R = cr§a${cr§b${1}c§}d§");
TP("R = tg§a${tg§b§}c§.length + '' + tg§a${tg§b§}c§[0]");
TP("var t = (s, ...v) => s.raw.join('|') + ':' + v.join('|'); R = t§a${t§b${t§c§}d§}e§");
TP("var t = (s, ...v) => s.raw.join('|') + ':' + v.join('|'); R = t§a${1}§ + t§${§x${2}y§}§");
TP("R = §${§${§${§${'deep'}§}§}§}§");
TP("var t = (s, ...v) => v[0]; R = t§${t§${t§${3}§}§}§");
TP("var t = (s, ...v) => J(s.raw) + v; R = t§${[1, 2].map(x => t§<${x}>§)}§");
TP("var t = (s, ...v) => J(s); R = §${[1, 2].map(x => §<${x}>§).join('')}§");
TP("var l = []; var t = (s, ...v) => (l.push(s), s); var f = () => t§x§; f(); f(); t§${t§x§}§; R = new Set(l).size");
TP("var t = (s) => s; var a = t§${t§x§ === t§x§}§; R = a.raw.join()");
TP("var l = []; var f = n => n ? tg§${f(n - 1)}§ : tg§z§; R = f(3).length");
TP("R = §${(() => §${(() => §in§)()}§)()}§");
TP("R = §${function () { return §a${1}§ }()}§");
TP("R = §${{ toString() { return §x${1}§ } }}§");
TP("R = §${eval('§${1 + 1}§')}§");
TP("R = §${§${1}§ + §${2}§}§");
TP("R = §${§§}${§§}§.length");
TP("var t = (s, ...v) => v.length; R = t§${t§${1}${2}§}${3}§");

// ---- 8. String.raw.
TP("R = String.raw§a${1}b${2}c§");
TP("R = String.raw({ raw: ['a', 'b', 'c'] }, 1, 2)");
TP("R = String.raw({ raw: ['a', 'b', 'c'] }, 1)");
TP("R = String.raw({ raw: ['a', 'b', 'c'] })");
TP("R = String.raw({ raw: ['a', 'b', 'c'] }, 1, 2, 3, 4)");
TP("R = String.raw({ raw: ['a'] }, 1, 2)");
TP("R = String.raw({ raw: [] }, 1, 2)");
TP("R = J(String.raw({ raw: [] }))");
TP("R = String.raw({ raw: 'abc' }, 1, 2)");
TP("R = String.raw({ raw: 'abc' }, 1, 2, 3)");
TP("R = String.raw({ raw: { length: 3, 0: 'a', 1: 'b', 2: 'c' } }, '-', '+')");
TP("R = String.raw({ raw: { length: 2, 0: 'a' } }, '-')");
TP("R = String.raw({ raw: { length: '2', 0: 'a', 1: 'b' } }, '-')");
TP("R = String.raw({ raw: { length: 2.9, 0: 'a', 1: 'b', 2: 'c' } }, '-', '+')");
TP("R = String.raw({ raw: { length: -1, 0: 'a' } }, '-')");
TP("R = String.raw({ raw: { length: NaN, 0: 'a' } }, '-')");
TP("R = String.raw({ raw: { length: undefined, 0: 'a' } }, '-')");
TP("R = String.raw({ raw: { length: null, 0: 'a' } }, '-')");
TP("R = String.raw({ raw: { length: { valueOf() { return 2 } }, 0: 'a', 1: 'b' } }, '-')");
TP("R = String.raw({ raw: { 0: 'a', 1: 'b' } }, '-')");
TP("R = String.raw({ raw: [1, 2, 3] }, 'x', 'y')");
TP("R = String.raw({ raw: [null, undefined, {}] }, 'x', 'y')");
TP("R = String.raw({ raw: [{ toString() { return 'T' } }, 'b'] }, 'x')");
TP("R = String.raw({ raw: [Symbol(), 'b'] }, 'x')");
TP("R = String.raw({ raw: ['a', 'b'] }, Symbol())");
TP("R = String.raw({ raw: ['a', 'b'] }, { toString() { return 'T' } })");
TP("R = String.raw({ raw: ['a', 'b'] }, undefined)");
TP("R = String.raw({ raw: ['a', 'b'] }, null)");
TP("var l = []; var raw = { get length() { l.push('len'); return 3 }, get 0() { l.push(0); return 'a' }, get 1() { l.push(1); return 'b' }, get 2() { l.push(2); return 'c' } }; var s = { toString() { l.push('s'); return 'S' } }; String.raw({ raw }, s, s); R = l.join()");
TP("var l = []; var p = new Proxy({ length: 2, 0: 'a', 1: 'b' }, { get(t, k) { l.push(String(k)); return t[k] } }); R = String.raw({ raw: p }, 'x') + l.join()");
TP("var l = []; var o = new Proxy({ raw: ['a', 'b'] }, { get(t, k) { l.push(String(k)); return t[k] } }); R = String.raw(o, 'x') + l.join()");
TP("R = String.raw({ get raw() { return ['p', 'q'] } }, 1)");
TP("R = String.raw(Object.create({ raw: ['i', 'j'] }), 1)");
TP("R = String.raw({})");
TP("R = String.raw({ raw: undefined })");
TP("R = String.raw({ raw: null })");
TP("R = String.raw({ raw: 1 })");
TP("R = String.raw({ raw: true })");
TP("R = String.raw({ raw: Symbol() })");
TP("R = String.raw({ raw: function (a, b) {} }, 'x', 'y')");
TP("R = String.raw({ raw: new String('ab') }, 'x')");
TP("R = String.raw({ raw: new Uint8Array([65, 66, 67]) }, '-', '+')");
TP("R = String.raw({ raw: new Set([1]) }, 1)");
TP("R = String.raw('abc')");
TP("R = String.raw(1)");
TP("R = String.raw(1, 2)");
TP("R = String.raw(null)");
TP("R = String.raw(undefined)");
TP("R = String.raw()");
TP("R = String.raw(Object('abc'))");
TP("R = String.raw(function () {})");
TP("R = String.raw([])");
TP("R = String.raw({ raw: Object('xyz') }, 1, 2)");
TP("R = String.raw.length + ',' + String.raw.name + ',' + typeof String.raw + ',' + J(Object.getOwnPropertyDescriptor(String, 'raw'))");
TP("R = String.raw.call(null, { raw: ['a', 'b'] }, 1)");
TP("R = String.raw.apply(null, [{ raw: ['a', 'b'] }, 1])");
TP("R = Reflect.apply(String.raw, 0, [{ raw: ['a', 'b'] }, 1])");
TP("R = [1, 2].map(n => String.raw§n${n}§).join()");
TP("R = String.raw§\\u§ + String.raw§\\x§ + String.raw§\\1§");
TP("R = String.raw§\\n\\t\\§§");
TP("R = String.raw§\\${1}§");
TP("R = String.raw§\\\\${1}§");
TP("R = String.raw§$${1}$§");
TP("R = String.raw§${1}${2}${3}§");
TP("R = String.raw§§ + String.raw§${''}§");
TP("R = String.raw§a${1}§.length + String.raw§${1}a§.length");
TP("R = new String.raw§a§");
TP("R = new String.raw({ raw: ['a'] })");
TP("R = Object.getOwnPropertyNames(String.raw).join()");
TP("R = String.raw.hasOwnProperty('prototype')");
TP("var raw = String.raw; R = raw({ raw: ['<', '>'] }, 'x')");
TP("R = String.raw({ raw: ['a', 'b'] }, ...[1, 2])");
TP("R = String.raw({ raw: Array(3) }, 1, 2)");
TP("R = String.raw({ raw: [, 'b', ,] }, 1, 2)");
TP("R = String.raw({ raw: { length: 2 ** 53 + 2, get 0() { return 'a' } } }, 1).length > 0");

// ---- 9. Sequências \§ e \${ e cifrão.
TP("R = §\\§§");
TP("R = §a\\§b§");
TP("R = §\\${1}§");
TP("R = §\\${§");
TP("R = §$§ + §$$§ + §$$$§");
TP("R = §$${1}§");
TP("R = §$${1}$§");
TP("R = §${1}$§");
TP("R = §$\\{1}§");
TP("R = §{${1}}§");
TP("R = §}${1}{§");
TP("R = §${'$'}{1}§");
TP("R = §\\\\${1}§");
TP("R = §\\\\\\${1}§");
TP("R = §\\\\\\§§");
TP("R = cr§\\§§");
TP("R = cr§\\${§");
TP("R = cr§$${1}\\${2}§");
TP("R = cr§\\\\${1}§");
TP("R = cr§\\§${1}\\§§");
TP("R = cr§$\\{§");
TP("R = cr§ ${ 1 } §");
TP("R = cr§${1}${2}§");
TP("R = cr§${1}§");
TP("R = cr§§");
TP("R = cr§a§");
TP("R = cr§${}§");
S("§${}§");
S("§${§");
S("§${1§");
S("§${1}§§");
S("§");
S("§\\§");
S("§${§§}§");
S("§${1,}§");
S("§${...a}§");
S("§${a b}§");
S("§${a;}§");
S("§${yield}§");
S("§${await 1}§");
S("async function f() { return §${await 1}§ }");
S("function f() { return §${await 1}§ }");
S("function* g() { return §${yield 1}§ }");
S("function g() { return §${yield 1}§ }");
S("'use strict'; function g() { return §${yield}§ }");
S("§${new.target}§");
S("function f() { return §${new.target}§ }");
S("§${this}§");
S("§${super.x}§");
S("§${arguments}§");
S("function f() { return §${arguments[0]}§ }");
S("§${import('x')}§");
S("§$ {1}§");
S("§${ }§");
S("§${/}/.source}§");
S("§${'§'}§");
S("§${'${'}§");
S("§${§}§§");
S("tg§${1}§§");
S("tg§§§§");
S("tg§a§ tg§b§");
S("tg§a§tg§b§");
S("tg§a§\n§b§");
S("var a = §a§ §b§");
S("§a§ §b§");
S("§a§§b§");
S("§a§(1)");
S("§a§[0]");
S("§a§.length");
S("§a§++");
S("§a§ = 1");
S("(§a§) = 1");
S("for (§a§ of []);");
S("[§a§] = []");
S("({ §a§: 1 })");
S("({ [§a§]: 1 })");
S("class C { §a§() {} }");
S("class C { [§a§]() {} }");
S("function f(§a§) {}");
S("function §a§() {}");
S("var §a§");
S("let §a§ = 1");
S("import §a§ from 'x'");
S("'use strict'; §a§");
S("§use strict§; 1");
S("(§a§)() ");
S("new §a§");
S("new §a§()");
S("async §a§ => 1");
S("§a§ => 1");
S("§a§?.b");
S("§a§ ?? 1");
S("a ?? §b§");
S("delete §a§");
S("typeof §a§");
S("§a§ instanceof Object");
S("x = §a§, y = §b${1}§");
S("with (§a§) {}");
S("switch (§a§) { case §b§: }");
S("label: §a§");
S("if (§a§) {}");
S("throw §a§");
S("try { throw §a§ } catch (e) {}");
S("({ get [§a${1}§]() { return 1 } })");
S("({ __proto__: §a§ })");
S("class C extends §a§ {}");
S("class C { static x = §a${1}§ }");
S("function f(a = §a${1}§) { return a } f()");
S("§\\u{1F600}§");
S("§\\u{}§");
S("tg§\\u{}§");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "template-golden-"));
// O bun passa arquivos pelo transpilador próprio; `vm.runInThisContext` roda como ProgramExecutable do JSC puro, então o
// programa vai por ele; o SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "template_source.js");
const file = path.join(dir, "template_case.js");
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
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
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
