// Gera tests/golden/global_navigator_bun.tsv: `global`, `self` e `navigator` do global medidos no bun 1.4.2
// (descritor, valor, atribuição, `delete`, o par `get`/`set` do `self`, ordem de chaves, o objeto `navigator`,
// seus acessores e o `toString`).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Cada programa roda num processo `bun` próprio: vários mudam o global (`self = 7`, `delete globalThis.global`) e
// um estado herdado de outra linha falsificaria a medição.
// Valores que dependem da máquina entram só como tipo: `hardwareConcurrency` vira `typeof` e inteiro positivo; o
// `userAgent` traz a versão do bun, então é comparado pelo prefixo `Bun/`; `platform` é o do Linux x86_64.
// Uso: bun scripts/gen-global-navigator-golden.js > tests/golden/global_navigator_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v === undefined) return 'undefined'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message };\n" +
  "var D = function (o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x && [typeof x.value, x.writable, x.enumerable, x.configurable, typeof x.get, typeof x.set] };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const stmts = (code) => programs.push(HELPER + `try { ${code} } catch (e) { R = E(e) }`);
const strict = (code) => programs.push(HELPER + `try { (function () { 'use strict'; ${code} })() } catch (e) { R = E(e) }`);
const G = "Object.getOwnPropertyDescriptor(globalThis, 'self')";

// global: descritor, identidade, escrita, delete.
expr("D(globalThis, 'global')");
expr("global === globalThis");
expr("global.global === globalThis");
expr("typeof global");
expr("Object.prototype.hasOwnProperty.call(globalThis, 'global')");
expr("Object.keys(globalThis).indexOf('global') >= 0");
expr("Object.getOwnPropertyNames(globalThis).indexOf('global') - Object.getOwnPropertyNames(globalThis).indexOf('structuredClone')");
expr("Object.getOwnPropertyNames(globalThis).indexOf('global') > Object.getOwnPropertyNames(globalThis).indexOf('structuredClone')");
stmts("global = 5; R = S([typeof global, D(globalThis, 'global'), globalThis.global])");
strict("global = 5; R = S([typeof global, D(globalThis, 'global')])");
stmts("var i = Object.getOwnPropertyNames(globalThis).indexOf('global'); global = 5; R = S(Object.getOwnPropertyNames(globalThis).indexOf('global') - i)");
stmts("R = S([delete globalThis.global, typeof global, 'global' in globalThis, D(globalThis, 'global')])");
stmts("delete globalThis.global; try { global } catch (e) { R = E(e) }");
strict("delete globalThis.global; global");
stmts("delete globalThis.global; global = 3; R = S([typeof global, D(globalThis, 'global')])");
stmts("var f = function () { return global }; R = S(f() === globalThis)");
expr("Object.getOwnPropertyDescriptor(globalThis, 'global').value === globalThis");
expr("(function () { return this === global })()");

// self: descritor, identidade, par get/set.
expr("D(globalThis, 'self')");
expr("self === globalThis");
expr("self.self === globalThis");
expr("typeof self");
expr("window");
expr("Object.keys(globalThis).indexOf('self') >= 0");
expr(`[${G}.get.name, ${G}.get.length, ${G}.set.name, ${G}.set.length]`);
expr(`[${G}.get.toString(), ${G}.set.toString()]`);
expr(`[Object.getOwnPropertyNames(${G}.get), Object.getOwnPropertyNames(${G}.set)]`);
expr(`['prototype' in ${G}.get, 'prototype' in ${G}.set]`);
expr(`[Object.getPrototypeOf(${G}.get) === Function.prototype, Object.getPrototypeOf(${G}.set) === Function.prototype]`);
expr(`[${G}.get.call(null) === globalThis, ${G}.get.call(undefined) === globalThis, ${G}.get.call(5) === globalThis, ${G}.get.call({}) === globalThis, ${G}.get.call('x') === globalThis]`);
expr(`[${G}.enumerable, ${G}.configurable, 'value' in ${G}, 'writable' in ${G}]`);
expr(`Object.getOwnPropertyNames(globalThis).indexOf('self') - Object.getOwnPropertyNames(globalThis).indexOf('ShadowRealm')`);
expr(`Object.getOwnPropertyNames(globalThis).indexOf('self') > Object.getOwnPropertyNames(globalThis).indexOf('Symbol')`);
expr(`Object.getOwnPropertyNames(globalThis).indexOf('self') > Object.getOwnPropertyNames(globalThis).indexOf('global')`);

// self: o setter.
stmts("self = 7; R = S([self, typeof self, D(globalThis, 'self')])");
strict("self = 7; R = S([self, D(globalThis, 'self')])");
stmts("var i = Object.getOwnPropertyNames(globalThis).indexOf('self'); self = 7; R = S(Object.getOwnPropertyNames(globalThis).indexOf('self') - i)");
stmts(`var g = ${G}; R = S([String(g.set.call({}, 1)), String(g.set.call({}, 'a')), String(g.set.call(null, 7)), String(g.set.call(undefined, 8)), String(g.set.call(5, 9)), String(g.set.call({})), self])`);
stmts(`var g = ${G}; var o = {}; g.set.call(o, 3); R = S([D(o, 'self'), o.self, D(globalThis, 'self'), self])`);
stmts(`var g = ${G}; g.set.call(globalThis, 5); R = S([D(globalThis, 'self'), self])`);
stmts(`var g = ${G}; g.set.call({}); R = S([D(globalThis, 'self'), self])`);
stmts("var o = Object.create(globalThis); o.self = 4; R = S([D(o, 'self'), o.self, D(globalThis, 'self'), self])");
strict("var o = Object.create(globalThis); o.self = 4; R = S([D(o, 'self'), o.self, D(globalThis, 'self')])");
stmts("R = S(Reflect.set(globalThis, 'self', 6)); R = S([R, D(globalThis, 'self'), self])");
stmts("R = S([delete globalThis.self, typeof self, 'self' in globalThis, D(globalThis, 'self')])");
stmts("delete globalThis.self; try { self } catch (e) { R = E(e) }");
stmts("delete globalThis.self; self = 3; R = S([typeof self, D(globalThis, 'self')])");
stmts("Object.defineProperty(globalThis, 'self', { value: 1 }); R = S([D(globalThis, 'self'), self])");
stmts("Object.defineProperty(globalThis, 'self', { value: 1, writable: false }); R = S([D(globalThis, 'self'), self])");
stmts("Object.defineProperty(globalThis, 'self', { get: function () { return 42 } }); R = S([D(globalThis, 'self'), self])");
stmts("Object.defineProperty(globalThis, 'self', { enumerable: false }); R = S([D(globalThis, 'self'), self === globalThis])");
stmts(`var g = ${G}; var s = g.set; self = 1; R = S([self, String(s(2)), self, D(globalThis, 'self')])`);
stmts(`var g = ${G}; self = 1; var h = Object.getOwnPropertyDescriptor(globalThis, 'self'); R = S([h.value, typeof h.get])`);
stmts("var d = " + G + "; self = 1; Object.defineProperty(globalThis, 'self', d); R = S([self === globalThis, D(globalThis, 'self')])");
stmts("self.self.self = 3; R = S([self, D(globalThis, 'self')])");
stmts("with (globalThis) { self = 9 } R = S([self, D(globalThis, 'self')])");
stmts("this.self = 8; R = S([self, D(globalThis, 'self')])");
stmts("var v = (function () { return self })(); R = S(v === globalThis)");
stmts("R = S(JSON.stringify(Object.keys(globalThis).filter(function (k) { return k === 'self' || k === 'global' || k === 'navigator' })))");
stmts("var c = 0; for (var k in globalThis) if (k === 'self' || k === 'global' || k === 'navigator') c++; R = S(c)");

// navigator: descritor, forma.
expr("D(globalThis, 'navigator')");
expr("typeof navigator");
expr("typeof Navigator");
expr("Object.prototype.hasOwnProperty.call(globalThis, 'navigator')");
expr("Object.getOwnPropertyNames(globalThis).indexOf('navigator') > Object.getOwnPropertyNames(globalThis).indexOf('global')");
expr("Object.getOwnPropertyNames(globalThis).indexOf('navigator') < Object.getOwnPropertyNames(globalThis).indexOf('isNaN')");
expr("Object.getOwnPropertyNames(navigator)");
expr("Reflect.ownKeys(navigator).map(String)");
expr("Object.keys(navigator)");
expr("(function () { var k = []; for (var n in navigator) k.push(n); return k })()");
expr("Object.getPrototypeOf(navigator) === Object.prototype");
expr("navigator.constructor === Object");
expr("Object.prototype.toString.call(navigator)");
expr("String(navigator)");
expr("navigator + ''");
expr("navigator.toString === Object.prototype.toString");
expr("Object.getOwnPropertyDescriptor(navigator, Symbol.toStringTag)");
expr("navigator[Symbol.toStringTag]");
expr("[Object.isExtensible(navigator), Object.isFrozen(navigator), Object.isSealed(navigator)]");
expr("Object.getOwnPropertySymbols(navigator).map(String)");
expr("navigator === navigator");
expr("globalThis.navigator === navigator");
expr("Object.prototype.toString.call(Object.create(navigator))");
expr("String(Object.create(navigator))");
expr("Object.create(navigator).userAgent.startsWith('Bun/')");

// navigator: acessores.
for (const n of ["userAgent", "platform", "hardwareConcurrency"]) {
  const d = `Object.getOwnPropertyDescriptor(navigator, '${n}')`;
  expr(`[typeof ${d}.get, typeof ${d}.set, ${d}.enumerable, ${d}.configurable, 'value' in ${d}]`);
  expr(`[${d}.get.name, ${d}.get.length, ${d}.get.toString()]`);
  expr(`Object.getOwnPropertyNames(${d}.get)`);
  expr(`'prototype' in ${d}.get`);
  expr(`Object.getPrototypeOf(${d}.get) === Function.prototype`);
  expr(`[typeof ${d}.get.call(null), typeof ${d}.get.call(undefined), typeof ${d}.get.call(5), typeof ${d}.get.call({})]`);
  expr(`[${d}.get.call(null) === navigator.${n}, ${d}.get.call({}) === navigator.${n}]`);
  expr(`typeof navigator.${n}`);
  strict(`navigator.${n} = 'x'`);
  stmts(`navigator.${n} = 'x'; R = S([typeof navigator.${n}, Object.keys(navigator)])`);
  expr(`Reflect.set(navigator, '${n}', 'x')`);
  expr(`delete navigator.${n}`);
  stmts(`delete navigator.${n}; R = S([Object.keys(navigator), '${n}' in navigator, typeof navigator.${n}])`);
  stmts(`Object.defineProperty(navigator, '${n}', { value: 'q' }); R = S([navigator.${n}, D(navigator, '${n}')])`);
  expr(`typeof (0, eval)('typeof ${n}')`);
  expr(`(0, eval)('typeof ${n}')`);
}

// navigator: valores (por tipo ou prefixo).
expr("navigator.userAgent.startsWith('Bun/')");
expr("/^Bun\\/\\d+\\.\\d+\\.\\d+$/.test(navigator.userAgent)");
expr("navigator.platform");
expr("typeof navigator.hardwareConcurrency");
expr("Number.isInteger(navigator.hardwareConcurrency) && navigator.hardwareConcurrency > 0");
expr("[typeof navigator.language, typeof navigator.languages, typeof navigator.onLine, typeof navigator.vendor, typeof navigator.appName, typeof navigator.cookieEnabled, typeof navigator.product]");
expr("['language', 'languages', 'onLine', 'vendor', 'appName', 'appVersion', 'product', 'cookieEnabled', 'deviceMemory', 'maxTouchPoints', 'userAgentData', 'webdriver', 'connection', 'mediaDevices', 'serviceWorker', 'storage', 'clipboard', 'locks', 'gpu', 'sendBeacon', 'permissions'].filter(function (k) { return k in navigator })");
expr("JSON.stringify(navigator).replace(/\"hardwareConcurrency\":\\d+/, '\"hardwareConcurrency\":N').replace(/Bun\\/[\\d.]+/, 'Bun/V')");
expr("JSON.stringify(Object.keys(JSON.parse(JSON.stringify(navigator))))");
expr("Object.entries(navigator).map(function (e) { return [e[0], typeof e[1]] })");
expr("Object.assign({}, navigator).platform");
expr("Object.keys(Object.assign({}, navigator))");
expr("({ ...navigator }).platform");
expr("Object.getOwnPropertyNames(Object.getOwnPropertyDescriptors(navigator))");
expr("structuredClone(navigator).platform");
expr("navigator.hasOwnProperty('userAgent')");
expr("Object.hasOwn(navigator, 'userAgent')");
expr("navigator.propertyIsEnumerable('userAgent')");
expr("navigator.propertyIsEnumerable('hardwareConcurrency')");
stmts("navigator.extra = 1; R = S([navigator.extra, Object.keys(navigator)])");
stmts("var old = navigator; globalThis.navigator = 5; R = S([typeof navigator, D(globalThis, 'navigator')])");
stmts("R = S([delete globalThis.navigator, typeof navigator, D(globalThis, 'navigator')])");
stmts("delete globalThis.navigator; try { navigator } catch (e) { R = E(e) }");
stmts("var f = function () { return navigator.platform }; R = S(f())");
stmts("var n1 = navigator, n2 = globalThis.navigator, n3 = self.navigator, n4 = global.navigator; R = S([n1 === n2, n2 === n3, n3 === n4])");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "gn-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\nprocess.stdout.write(JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  const run = spawnSync(process.execPath, [file], { encoding: "utf8" });
  fs.rmSync(dir, { recursive: true, force: true });
  if (run.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + run.stderr);
  emitRow(JSON.stringify(sourceAscii) + "\t" + run.stdout);
}
