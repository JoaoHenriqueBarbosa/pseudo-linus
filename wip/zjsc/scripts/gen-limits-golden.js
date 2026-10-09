// Gera tests/golden/limits_bun.tsv: limites e robustez (recursão profunda, literais enormes, tamanhos máximos,
// muitos argumentos, eval aninhado) medidos no bun 1.4.2. Cada programa grava o texto de `R`.
// Programas que passam de 3 s, ou cujo resultado muda entre duas execuções, ou que mostram caminho da máquina,
// são descartados. Fontes grandes são montadas dentro do programa (`"(".repeat(n)`), então a linha do golden é curta.
// Uso: bun scripts/gen-limits-golden.js > tests/golden/limits_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Roda `body` num try/catch e grava tipo e mensagem do erro (ou "ok"); depois confere que o motor segue utilizável.
const probe = body =>
  `var r; try { ${body}; r = "ok" } catch (e) { r = (e instanceof RangeError) + "|" + e.constructor.name + "|" + e.message } ` +
  `var alive = (function () { return [1, 2, 3].map(function (x) { return x * 2 }).join() })(); R = r + "|" + alive`;

// ---- Recursão profunda em JS puro: o que importa é RangeError + mensagem + motor vivo depois.
const deepDepths = [1e5, 1e6];
for (const n of deepDepths) {
  add(probe(`function f(n) { return n === 0 ? 0 : 1 + f(n - 1) } f(${n})`));
  add(probe(`function a(n) { return n === 0 ? 0 : 1 + b(n - 1) } function b(n) { return a(n) } a(${n})`));
  add(probe(`var o = { get x() { return this.x + 1 } }; o.x`));
  add(probe(`var o = { toString() { return "" + this } }; "" + o`));
  add(probe(`var o = { toString() { return String(this) } }; String(o)`));
  add(probe(`var p = new Proxy({}, { get(t, k, r) { return r[k] } }); p.x`));
  add(probe(`var p = new Proxy({}, { has(t, k) { return k in p } }); "x" in p`));
  add(probe(`var p = new Proxy({}, { set(t, k, v, r) { r[k] = v; return true } }); p.x = 1`));
  add(probe(`var p = new Proxy(function () {}, { apply(t, th, args) { return p() } }); p()`));
  add(probe(`var p = new Proxy({}, { getPrototypeOf() { return Object.getPrototypeOf(p) } }); Object.getPrototypeOf(p)`));
  add(probe(`var p = new Proxy({}, { ownKeys() { return Reflect.ownKeys(p) } }); Reflect.ownKeys(p)`));
  add(probe(`var p = new Proxy({}, { defineProperty(t, k, d) { return Reflect.defineProperty(p, k, d) } }); Object.defineProperty(p, "x", { value: 1 })`));
  add(probe(`var p = new Proxy({}, { deleteProperty(t, k) { return delete p[k] } }); delete p.x`));
}
// Variações de recursão com tamanhos da pilha.
for (const k of ["0", "1", "2", "3", "4", "5", "6", "7"]) {
  add(probe(`var d = 0; function f() { d++; f() } try { f() } catch (e) { if (!(e instanceof RangeError)) throw e } if (d < 1000) throw new Error("curto" + ${k}); throw new RangeError("Maximum call stack size exceeded.")`));
  add(probe(`function f() { return f.call(null) } f()`));
  add(probe(`function f() { return f.apply(null, []) } f()`));
  add(probe(`function f() { return f.bind(null)() } f()`));
  add(probe(`function f() { return Reflect.apply(f, null, []) } f()`));
  add(probe(`function f() { return new f() } new f()`));
  add(probe(`var f = () => f(); f()`));
  add(probe(`class A { constructor() { new A() } } new A()`));
  add(probe(`class A { static m() { return A.m() } } A.m()`));
  add(probe(`function* g() { yield* g() } g().next()`));
  add(probe(`function f() { return [1].map(f) } f()`));
  add(probe(`function f() { return [1].forEach(f) } f()`));
  add(probe(`function f() { return [1].reduce(f) , f() } f()`));
  add(probe(`function f() { return [3, 2, 1].sort(f) , f() } f()`));
  add(probe(`function f() { return "a".replace(/a/, f) } f()`));
  add(probe(`function f() { return JSON.stringify({ toJSON: f }) } f()`));
  add(probe(`function f() { return JSON.parse("1", f) , f() } f()`));
  add(probe(`function f() { return new Promise(f) } f()`));
  add(probe(`function f() { return eval("f()") } f()`));
  add(probe(`function f() { return new Function("f", "return f()")(f) } f()`));
  add(probe(`function f() { return Symbol.toPrimitive in {} , ({ [Symbol.toPrimitive]: f }) + "" } f()`));
  add(probe(`var o = { valueOf() { return +this } }; +o`));
  add(probe(`var o = { [Symbol.toPrimitive]() { return +o } }; +o`));
  add(probe(`var o = {}; o.toString = function () { return o + "" }; o + ""`));
  add(probe(`function f() { try { return f() } finally { } } f()`));
  add(probe(`function f() { try { f() } catch (e) { f() } } f()`));
  add(probe(`var o = { get [Symbol.iterator]() { return o[Symbol.iterator] } }; [...o]`));
  add(probe(`function f() { return Array.from({ length: 1, 0: 1 }, f) , f() } f()`));
  add(probe(`var o = { get length() { return o.length } }; Array.prototype.slice.call(o)`));
  add(probe(`Object.defineProperty(globalThis, "gg", { get() { return gg }, configurable: true }); gg`));
  break;
}

// ---- Recursão nativa: JSON.stringify, JSON.parse, RegExp, flat, toString de Array aninhado.
const nests = [1e3, 1e4, 3e4, 1e5];
for (const n of nests) {
  add(probe(`var o = {}; var c = o; for (var i = 0; i < ${n}; i++) { c.a = {}; c = c.a } JSON.stringify(o).length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } JSON.stringify(o).length`));
  add(probe(`JSON.parse("[".repeat(${n}) + "]".repeat(${n}))`));
  add(probe(`JSON.parse('{"a":'.repeat(${n}) + "1" + "}".repeat(${n}))`));
  add(probe(`JSON.parse("[".repeat(${n}))`));
  add(probe(`JSON.parse("[".repeat(${n}) + "]".repeat(${n}), function (k, v) { return v })`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.flat(Infinity).length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } String(o).length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.join().length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.toString().length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.toLocaleString().length`));
  add(probe(`var o = {}; var c = o; for (var i = 0; i < ${n}; i++) { c.a = {}; c = c.a } Object.freeze(o); 1`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } "" + o`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.concat([]).length`));
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.flat(${n}).length`));
  add(probe(`new RegExp("(".repeat(${n}) + "a" + ")".repeat(${n})).test("a")`));
  add(probe(`new RegExp("(?:".repeat(${n}) + "a" + ")".repeat(${n})).test("a")`));
  add(probe(`new RegExp("(?=".repeat(${n}) + "a" + ")".repeat(${n})).test("a")`));
  add(probe(`new RegExp("(".repeat(${n}) + "a").test("a")`));
  add(probe(`new RegExp("a" + "|a".repeat(${n})).test("a")`));
  add(probe(`new RegExp("[".repeat(${n}))`));
  add(probe(`new RegExp("a{1}".repeat(${n})).test("a")`));
  add(probe(`new RegExp("(a)".repeat(${n})).exec("a".repeat(${n}))[0].length`));
  add(probe(`/a*/.test("a".repeat(${n}))`));
  add(probe(`/(a|b)*c/.test("ab".repeat(${n}))`));
  add(probe(`/(?:a|b)*c/.test("ab".repeat(${n}) + "c")`));
  add(probe(`/(a*)*b/.test("a".repeat(${Math.min(n, 30)}))`));
  add(probe(`"a".repeat(${n}).replace(/a/g, "b").length`));
  add(probe(`"a".repeat(${n}).split("").length`));
  add(probe(`var s = new Set(); var c = s; for (var i = 0; i < ${n}; i++) { var x = new Set(); c.add(x); c = x } String(s)`));
  add(probe(`var m = new Map(); var c = m; for (var i = 0; i < ${n}; i++) { var x = new Map(); c.set(1, x); c = x } m.size`));
  add(probe(`var o = {}; var c = o; for (var i = 0; i < ${n}; i++) { c.a = {}; c = c.a } structuredClone ? 1 : 1`));
  add(probe(`var p = Object.create(null); var c = p; for (var i = 0; i < ${n}; i++) { var x = Object.create(c); c = x } c.nothing`));
  add(probe(`var p = {}; var c = p; for (var i = 0; i < ${n}; i++) { c = Object.create(c) } "x" in c`));
  add(probe(`var p = {}; var c = p; for (var i = 0; i < ${n}; i++) { c = Object.create(c) } c.x = 1; c.x`));
  add(probe(`var p = {}; var c = p; for (var i = 0; i < ${n}; i++) { c = Object.create(c) } p.isPrototypeOf(c)`));
  add(probe(`var p = {}; var c = p; for (var i = 0; i < ${n}; i++) { c = Object.create(c) } c instanceof Object`));
  add(probe(`var f = function () {}; for (var i = 0; i < ${n}; i++) { f = f.bind(null) } f()`));
  add(probe(`var f = function () { return 1 }; for (var i = 0; i < ${n}; i++) { f = f.bind(null) } f.name.length`));
  add(probe(`var f = function () { return 1 }; for (var i = 0; i < ${n}; i++) { f = f.bind(null) } new f()`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } p.x`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } "x" in p`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } Object.keys(p).length`));
  add(probe(`var p = []; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } Array.isArray(p)`));
  add(probe(`var p = function () {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } typeof p`));
  add(probe(`var p = function () { return 1 }; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } p()`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } Object.getPrototypeOf(p) === Object.prototype`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } JSON.stringify(p)`));
  add(probe(`var p = {}; for (var i = 0; i < ${n}; i++) { p = new Proxy(p, {}) } Object.prototype.toString.call(p)`));
  add(probe(`var e = new Error("x"); for (var i = 0; i < ${n}; i++) { e = new Error("x", { cause: e }) } String(e)`));
  add(probe(`var e = new Error("x"); for (var i = 0; i < ${n}; i++) { e = new Error("x", { cause: e }) } 1`));
  add(probe(`var p = Promise.resolve(1); for (var i = 0; i < ${n}; i++) { p = p.then(function (x) { return x }) } 1`));
  add(probe(`var a = []; a[0] = a; String(a)`));
  add(probe(`var a = []; a[0] = a; a.flat(Infinity).length`));
  add(probe(`var a = []; a[0] = a; JSON.stringify(a)`));
  add(probe(`var a = []; a[0] = a; a.join().length`));
  add(probe(`var o = {}; o.o = o; JSON.stringify(o)`));
  add(probe(`var a = []; a[0] = a; a.toLocaleString().length`));
}
// Tamanhos pequenos garantem saída determinística que não estoura.
for (const n of [10, 100, 500]) {
  add(probe(`var o = []; var c = o; for (var i = 0; i < ${n}; i++) { var x = []; c.push(x); c = x } o.flat(Infinity).length`));
  add(probe(`var o = {}; var c = o; for (var i = 0; i < ${n}; i++) { c.a = {}; c = c.a } JSON.stringify(o).length`));
  add(probe(`JSON.parse("[".repeat(${n}) + "]".repeat(${n})).length`));
  add(probe(`new RegExp("(".repeat(${n}) + "a" + ")".repeat(${n})).exec("a").length`));
}

// ---- Literais enormes e profundidade de parse (via eval e Function: a linha do golden fica curta).
const parseN = [500, 1000, 3000, 5000, 20000, 100000];
const parseKinds = {
  parens: n => `"(".repeat(${n}) + "1" + ")".repeat(${n})`,
  arrays: n => `"[".repeat(${n}) + "]".repeat(${n})`,
  objects: n => `"({a:".repeat(${n}) + "1" + "})".repeat(${n})`,
  blocks: n => `"{".repeat(${n}) + "}".repeat(${n})`,
  unary: n => `"!".repeat(${n}) + "1"`,
  minus: n => `"- ".repeat(${n}) + "1"`,
  typeofs: n => `"typeof ".repeat(${n}) + "1"`,
  voids: n => `"void ".repeat(${n}) + "1"`,
  plus: n => `"1" + "+1".repeat(${n})`,
  minusbin: n => `"1" + "-1".repeat(${n})`,
  times: n => `"1" + "*1".repeat(${n})`,
  comma: n => `"1" + ",1".repeat(${n})`,
  ternary: n => `"1?" .repeat(${n}) + "1" + ":1".repeat(${n})`,
  assign: n => `"a=".repeat(${n}) + "1"`,
  members: n => `"a" + ".b".repeat(${n})`,
  calls: n => `"f" + "()".repeat(${n})`,
  arrows: n => `"a=>".repeat(${n}) + "1"`,
  templates: n => `"\`${"$"}{".repeat(${n}) + "1" + "}\`".repeat(${n})`,
  ifs: n => `"if(1)".repeat(${n}) + ";"`,
  whiles: n => `"while(0)".repeat(${n}) + ";"`,
  labels: n => `"a:".repeat(${n}) + ";"`,
  news: n => `"new ".repeat(${n}) + "Object"`,
  awaits: n => `"async function f(){" + "await ".repeat(${n}) + "1}"`,
  spreads: n => `"[" + "...[".repeat(${n}) + "]".repeat(${n}) + "]"`,
  classes: n => `"class A{static{".repeat(${n}) + "}}".repeat(${n})`,
  destructure: n => `"var ".concat("[".repeat(${n}), "a", "]".repeat(${n}), "=0")`,
  functions: n => `"(function(){".repeat(${n}) + "})".repeat(${n}) + "()".repeat(${n})`,
};
for (const [name, build] of Object.entries(parseKinds)) {
  for (const n of parseN) {
    if ((name === "plus" || name === "minusbin" || name === "times" || name === "comma") && n < 20000) continue;
    add(probe(`eval(${build(n)}); 0`));
    if (n === 5000 || n === 20000) add(probe(`new Function(${build(n)}); 0`));
  }
}
// Strings, arrays, objetos e operandos grandes.
add(probe(`eval('"' + "a".repeat(2 ** 20) + '"').length`));
add(probe(`eval("'" + "a".repeat(2 ** 20) + "'").length`));
add(probe(`eval("\`" + "a".repeat(2 ** 20) + "\`").length`));
add(probe(`eval("/" + "a".repeat(2 ** 16) + "/").source.length`));
add(probe(`eval("[" + "0,".repeat(50000) + "]").length`));
add(probe(`eval("[" + "1,".repeat(50000) + "]").length`));
add(probe(`eval("[" + ",".repeat(50000) + "]").length`));
add(probe(`eval("({" + Array.from({ length: 20000 }, function (_, i) { return "k" + i + ":" + i }).join(",") + "})") && Object.keys(eval("({" + Array.from({ length: 20000 }, function (_, i) { return "k" + i + ":" + i }).join(",") + "})")).length`));
add(probe(`Object.keys(eval("({" + Array.from({ length: 20000 }, function (_, i) { return "'" + i + "':1" }).join(",") + "})")).length`));
add(probe(`eval("1" + "+1".repeat(20000))`));
add(probe(`eval("1" + "+1".repeat(5000))`));
add(probe(`eval("'a'" + "+'a'".repeat(20000)).length`));
add(probe(`eval("0" + "||0".repeat(20000))`));
add(probe(`eval("1" + "&&1".repeat(20000))`));
add(probe(`eval("1" + "|1".repeat(20000))`));
add(probe(`eval("1" + "<1".repeat(5000))`));
add(probe(`eval("1" + "==1".repeat(5000))`));
add(probe(`eval("a" + ".b".repeat(5000)); 0`));
add(probe(`eval("var a = {}; a" + "?.b".repeat(5000))`));
add(probe(`eval("var a = 1;" + "a++;".repeat(50000) + "a")`));
add(probe(`eval("var a = 1;" + "a = a + 1;".repeat(50000) + "a")`));
add(probe(`eval("var " + Array.from({ length: 20000 }, function (_, i) { return "v" + i }).join(",") + "; 1")`));
add(probe(`eval("switch(0){" + "case 1:".repeat(20000) + "}")`));
add(probe(`eval("function f(" + Array.from({ length: 20000 }, function (_, i) { return "p" + i }).join(",") + "){}; f.length")`));
add(probe(`eval("f(" + "1,".repeat(20000) + "1)")`));
add(probe(`eval("function f(){return arguments.length} f(" + "1,".repeat(20000) + "1)")`));
add(probe(`eval("function f(){return arguments.length} f(" + "1,".repeat(70000) + "1)")`));
add(probe(`eval("function f(){return arguments.length} f(" + "1,".repeat(130000) + "1)")`));
add(probe(`eval("new Array(" + "1,".repeat(20000) + "1).length")`));
add(probe(`new Function(Array.from({ length: 20000 }, function (_, i) { return "p" + i }).join(","), "return 1")()`));
add(probe(`new Function("a".repeat(2 ** 16), "return 1").length`));
add(probe(`new Function("return " + "1+".repeat(20000) + "1")()`));
add(probe(`"x".repeat(2 ** 20).length`));
add(probe(`("x".repeat(2 ** 20) + "y").length`));
add(probe(`Array.from({ length: 50000 }, function (_, i) { return i }).join().length`));

// ---- Limites de tamanho.
const sizeBodies = [
  `new Array(2 ** 32)`, `new Array(2 ** 32 - 1).length`, `new Array(2 ** 32 - 2).length`, `new Array(-1)`, `new Array(1.5)`,
  `new Array(NaN)`, `new Array(Infinity)`, `new Array("4294967296").length`, `Array(2 ** 32)`, `Array(4294967295).length`,
  `Array(4294967296)`, `[].length = 2 ** 32`, `var a = []; a.length = 2 ** 32 - 1; a.length`, `var a = []; a.length = 2 ** 32`,
  `var a = []; a.length = -1`, `var a = []; a.length = 1.5`, `var a = []; a.length = "x"`, `var a = []; a.length = NaN`,
  `var a = []; a.length = Infinity`, `var a = []; a.length = 4294967295; a.push(1)`, `var a = []; a.length = 4294967295; a.push()`,
  `var a = []; a.length = 4294967295; a[4294967295] = 1; a.length`, `var a = []; a.length = 4294967295; a.unshift(1)`,
  `var a = []; a.length = 4294967295; a.concat([1])`, `var a = [1]; a.length = 4294967295; a.splice(0, 0, 1)`,
  `Array.from({ length: 2 ** 32 })`, `Array.from({ length: 2 ** 32 - 1 }).length`, `Array.prototype.slice.call({ length: 2 ** 32 }, 0, 1).length`,
  `Array.prototype.push.call({ length: 2 ** 53 - 1 }, 1)`, `Array.prototype.push.call({ length: 2 ** 53 - 2 }, 1)`,
  `var o = { length: 2 ** 53 - 1 }; Array.prototype.push.call(o); o.length`, `Array.prototype.unshift.call({ length: 2 ** 53 - 1 }, 1)`,
  `Array.prototype.concat.call([], { length: 1, [Symbol.isConcatSpreadable]: true }).length`,
  `Array.prototype.splice.call({ length: 2 ** 53 - 1 }, 0, 0, 1)`, `Array.prototype.fill.call({ length: 5 }, 1).length`,
  `Array.of.call(function (n) { return { length: n } }, 1, 2).length`, `new Array(1e5).fill(0).length`, `new Array(1e6).fill(0).length`,
  `new Array(1e6).join().length`, `Array(1e6).join("ab").length`, `new Array(1e7).length`, `new Array(1e9).length`,
  `new Array(2 ** 31).length`, `var a = []; a[2 ** 31] = 1; a.length`, `var a = []; a[4294967294] = 1; a.length`, `var a = []; a[4294967295] = 1; a.length`,
  `var a = []; a[1e9] = 1; a.length`, `var a = []; a[1e9] = 1; a.indexOf(1)`, `var a = []; a[1e9] = 1; Object.keys(a).length`,
  `var a = []; a[1e9] = 1; a.reverse().length`, `var a = [1, 2]; a.length = 1e9; a.indexOf(2)`, `var a = [1, 2]; a.length = 1e9; a.lastIndexOf(2)`,
  `var a = [1, 2]; a.length = 1e9; a.includes(0)`, `var a = [1, 2]; a.length = 1e9; Object.keys(a).length`, `var a = [1, 2]; a.length = 1e9; a.pop()`,
  `var a = [1, 2]; a.length = 1e9; a.slice(0, 2).length`, `var a = [1, 2]; a.length = 1e9; a.slice(-2).length`, `var a = [1, 2]; a.length = 1e9; a.splice(0, 1).length`,
  `var a = [1, 2]; a.length = 1e9; a.shift()`, `var a = [1, 2]; a.length = 1e9; a.at(-1)`, `var a = [1, 2]; a.length = 1e9; a.findLast(function (x) { return x === 2 })`,
  `var a = [1, 2]; a.length = 1e9; a.some(function (x) { return x === 2 })`, `var a = [1, 2]; a.length = 1e9; JSON.stringify(a).length`,
  `var a = [1, 2]; a.length = 1e9; a.toString().length`, `var a = [1, 2]; a.length = 1e9; a.join().length`,
  `"x".repeat(2 ** 30)`, `"x".repeat(2 ** 31)`, `"x".repeat(2 ** 32)`, `"x".repeat(2 ** 29)`, `"x".repeat(2 ** 28).length`, `"xx".repeat(2 ** 29)`,
  `"x".repeat(-1)`, `"x".repeat(Infinity)`, `"".repeat(2 ** 40).length`, `"x".repeat(2 ** 30 - 25).length`, `"x".repeat(2 ** 30 - 1)`,
  `var s = "x".repeat(2 ** 29); s + s`, `var s = "x".repeat(2 ** 28); (s + s + s + s + s).length`, `"x".padEnd(2 ** 30)`, `"x".padEnd(2 ** 31)`, `"x".padStart(2 ** 32)`,
  `"x".padEnd(2 ** 28, "ab").length`, `"x".padEnd(2 ** 30 - 1).length`, `"x".padEnd(2 ** 30 - 25).length`,
  `new ArrayBuffer(2 ** 53)`, `new ArrayBuffer(2 ** 53 - 1)`, `new ArrayBuffer(2 ** 32)`, `new ArrayBuffer(-1)`, `new ArrayBuffer(1.5).byteLength`, `new ArrayBuffer(NaN).byteLength`,
  `new ArrayBuffer(Infinity)`, `new ArrayBuffer("x").byteLength`, `new ArrayBuffer(1e9).byteLength`, `new ArrayBuffer(1e12)`, `new ArrayBuffer(2 ** 31).byteLength`,
  `new ArrayBuffer(2 ** 31 - 1).byteLength`, `new ArrayBuffer(0, { maxByteLength: 2 ** 53 })`, `new ArrayBuffer(1, { maxByteLength: 0 })`, `new ArrayBuffer(1, { maxByteLength: 2 ** 53 })`,
  `new Uint8Array(2 ** 53)`, `new Uint8Array(2 ** 32)`, `new Uint8Array(-1)`, `new Uint8Array(1.5).length`, `new Uint8Array(NaN).length`, `new Uint8Array(Infinity)`,
  `new Uint8Array(1e9).length`, `new Uint8Array(2 ** 31).length`, `new Uint8Array(1e8).length`, `new Uint32Array(2 ** 30)`, `new Uint32Array(2 ** 32)`, `new Float64Array(2 ** 29)`,
  `new Float64Array(2 ** 28).length`, `new Float64Array(2 ** 53)`, `new Uint8Array(new ArrayBuffer(8), 9)`, `new Uint8Array(new ArrayBuffer(8), 0, 9)`, `new Uint8Array(new ArrayBuffer(8), 1.5)`,
  `new Uint16Array(new ArrayBuffer(8), 1)`, `new Uint16Array(new ArrayBuffer(7))`, `new Uint8Array({ length: 2 ** 32 })`, `new Uint8Array({ length: 2 ** 53 })`,
  `Uint8Array.from({ length: 2 ** 32 })`, `Uint8Array.of().length`, `new DataView(new ArrayBuffer(8), 9)`, `new DataView(new ArrayBuffer(8), 0, 9)`, `new DataView(new ArrayBuffer(8)).getInt8(8)`,
  `new DataView(new ArrayBuffer(8)).getFloat64(1)`, `new DataView(new ArrayBuffer(8)).getFloat64(-1)`, `new DataView(new ArrayBuffer(8)).getFloat64(2 ** 53)`,
  `new SharedArrayBuffer(2 ** 53)`, `new SharedArrayBuffer(-1)`, `new SharedArrayBuffer(2 ** 32)`,
  `new Set().add(1).size`, `new Map([[1, 2]]).size`, `Object.keys(Array.from({ length: 2 ** 20 })).length`, `Array.from({ length: 2 ** 20 }).length`, `Array.from({ length: 2 ** 22 }).length`,
  `Array(2 ** 20).fill(0).length`, `Array(2 ** 24).fill(0).length`, `Array(2 ** 24).length`, `Array(2 ** 25).fill().length`, `Array.apply(null, Array(2 ** 16)).length`,
  `Number.prototype.toFixed.call(1, 101)`, `(1).toFixed(100).length`, `(1).toFixed(-1)`, `(1).toPrecision(0)`, `(1).toPrecision(101)`, `(1).toPrecision(100).length`,
  `(1).toExponential(101)`, `(1).toExponential(100).length`, `(255).toString(1)`, `(255).toString(37)`, `(255).toString(36)`, `(2 ** 1000).toString(2).length`, `(2 ** 1023).toString(2).length`,
  `1n << (2n ** 30n)`, `1n << (2n ** 40n)`, `1n << 1000000n >> 1000000n`, `2n ** 1000000n > 0n`, `2n ** (2n ** 40n)`, `BigInt.asUintN(2 ** 53 - 1, 1n)`, `BigInt.asUintN(2 ** 53, 1n)`,
  `BigInt.asIntN(2 ** 53 - 1, 1n)`, `BigInt.asUintN(-1, 1n)`, `(2n ** 100000n).toString().length`, `BigInt("1".repeat(100000)) > 0n`, `BigInt("9".repeat(10000)).toString(36).length`,
  `new Date(8.64e15).getTime()`, `new Date(8.64e15 + 1).getTime()`, `new Date(2 ** 53).getTime()`, `new Date(8.64e15).toISOString()`, `new Date(8.64e15 + 1).toISOString()`,
  `"x".normalize("NFX")`, `"x".localeCompare("y", "xx-invalid-locale-tag-too-long-xxxxxxxx")`, `new Intl.NumberFormat("en", { maximumFractionDigits: 101 })`, `new Intl.NumberFormat("en", { maximumFractionDigits: 100 }).format(1).length`,
  `new Intl.NumberFormat("en", { minimumFractionDigits: 101 })`, `new Intl.NumberFormat("en", { maximumSignificantDigits: 22 })`, `new Intl.NumberFormat("en", { maximumSignificantDigits: 21 }).format(1).length`,
  `String.fromCodePoint(0x110000)`, `String.fromCodePoint(-1)`, `String.fromCodePoint(1.5)`, `String.fromCodePoint(0x10ffff).length`, `String.fromCodePoint(NaN)`,
  `"a".codePointAt(2 ** 53)`, `"a".at(2 ** 53)`, `"abc".substring(2 ** 53, 0)`, `"abc".slice(-(2 ** 53))`, `"abc".substr(-(2 ** 53), 2 ** 53)`,
  `Object.defineProperty([], "length", { value: 2 ** 32 })`, `Object.defineProperty([], "length", { value: 2 ** 32 - 1 }).length`, `Object.defineProperty([], "length", { value: -1 })`,
  `Object.defineProperty([], "length", { get() { return 1 } })`, `Object.defineProperty([], "length", { value: 1.5 })`, `Reflect.defineProperty([], "length", { value: 2 ** 32 })`,
  `Reflect.defineProperty([], "length", { value: 2 ** 32 - 1 })`, `Reflect.set([], "length", 2 ** 32)`, `Reflect.set([], "length", 2 ** 32 - 1)`,
  `Object.keys(Object.fromEntries(Array.from({ length: 2 ** 16 }, function (_, i) { return [i, i] }))).length`,
  `Object.keys(Object.fromEntries(Array.from({ length: 2 ** 16 }, function (_, i) { return ["k" + i, i] }))).length`,
  `new Set(Array.from({ length: 2 ** 16 }, function (_, i) { return i })).size`, `new Map(Array.from({ length: 2 ** 16 }, function (_, i) { return [i, i] })).size`,
  `new WeakMap().set({}, 1) instanceof WeakMap`, `structuredClone === undefined`, `Atomics.wait === undefined`, `Symbol.keyFor(Symbol.for("x".repeat(2 ** 16))).length`,
  `Symbol("x".repeat(2 ** 20)).description.length`, `({ ["x".repeat(2 ** 20)]: 1 })["x".repeat(2 ** 20)]`, `Object.keys({ ["x".repeat(2 ** 20)]: 1 })[0].length`,
  `JSON.stringify("x".repeat(2 ** 20)).length`, `JSON.parse(JSON.stringify("x".repeat(2 ** 20))).length`, `JSON.stringify(Array(1e5).fill(1)).length`,
  `JSON.parse("[" + "1,".repeat(1e5) + "1]").length`, `JSON.parse("{" + Array.from({ length: 1e4 }, function (_, i) { return '"k' + i + '":1' }).join() + "}")["k9999"]`,
  `encodeURIComponent("x".repeat(2 ** 20)).length`, `decodeURIComponent("%41".repeat(2 ** 16)).length`, `escape("x".repeat(2 ** 20)).length`, `btoa === undefined`,
  `parseInt("1".repeat(2 ** 16)) === Infinity`, `parseFloat("1".repeat(2 ** 16)) === Infinity`, `Number("1".repeat(400))`, `Number("0." + "0".repeat(400) + "1")`,
  `Number("1e" + "9".repeat(20))`, `Number("1e-" + "9".repeat(20))`, `+("9".repeat(2 ** 16))`, `Number("0x" + "f".repeat(2 ** 10))`, `Number("0b" + "1".repeat(2 ** 10))`,
];
for (const b of sizeBodies) add(probe(b));
// Resultado dos que não lançam: mostra o valor para ficar comparável (apenas os que têm valor curto).
for (const b of sizeBodies.filter(s => !/[;{]/.test(s) || /^new /.test(s))) {
  add(`var r; try { var v = (${b}); r = typeof v === "object" || typeof v === "function" ? Object.prototype.toString.call(v) : typeof v === "string" ? "str" + v.length : String(v) } catch (e) { r = e.constructor.name + ": " + e.message } R = r`);
}

// ---- Muitos argumentos: apply, spread, fromCharCode, Math.max, arguments.
for (const n of [1000, 10000, 65535, 65536, 70000, 100000, 120000, 125000, 130000, 200000, 500000, 1000000, 2000000]) {
  add(probe(`function f() { return arguments.length } f.apply(null, new Array(${n}))`));
  add(probe(`function f() { return arguments.length } f.apply(null, { length: ${n} })`));
  add(probe(`function f() { return arguments.length } f(...new Array(${n}))`));
  add(probe(`function f() { return arguments.length } f(...Array(${n}).fill(1), 1)`));
  add(probe(`function f(...a) { return a.length } f(...new Array(${n}))`));
  add(probe(`function f(...a) { return a.length } f.apply(null, new Array(${n}))`));
  add(probe(`String.fromCharCode.apply(null, new Array(${n}).fill(65)).length`));
  add(probe(`String.fromCharCode(...new Array(${n}).fill(65)).length`));
  add(probe(`String.fromCodePoint.apply(null, new Array(${n}).fill(65)).length`));
  add(probe(`Math.max(...Array(${n})) `));
  add(probe(`Math.max.apply(null, Array(${n}).fill(1))`));
  add(probe(`Math.min(...Array(${n}).fill(1))`));
  add(probe(`Math.hypot(...Array(${n}).fill(1))`));
  add(probe(`[].push.apply([], new Array(${n})).toString()`));
  add(probe(`var a = []; a.push(...new Array(${n})); a.length`));
  add(probe(`var a = []; a.unshift(...new Array(${n})); a.length`));
  add(probe(`var a = [0]; a.splice(0, 0, ...new Array(${n})); a.length`));
  add(probe(`[].concat(...new Array(${n})).length`));
  add(probe(`Array.of(...new Array(${n})).length`));
  add(probe(`new Array(...new Array(${n})).length`));
  add(probe(`Array(...new Array(${n})).length`));
  add(probe(`Reflect.apply(function () { return arguments.length }, null, new Array(${n}))`));
  add(probe(`Reflect.construct(function () { this.n = arguments.length }, new Array(${n})).n`));
  add(probe(`new (function () { this.n = arguments.length })(...new Array(${n})).n`));
  add(probe(`function f() { return arguments.length } f.call(null, ...new Array(${n}))`));
  add(probe(`function f() { return arguments.length } f.bind(null, ...new Array(${n}))()`));
  add(probe(`function f() { return arguments.length } f.bind.apply(f, [null].concat(new Array(${n})))()`));
  add(probe(`function f() { return arguments.length } (function () { return f.apply(this, arguments) }).apply(null, new Array(${n}))`));
  add(probe(`function f() { return Array.prototype.slice.call(arguments).length } f.apply(null, new Array(${n}))`));
  add(probe(`function f() { return [...arguments].length } f.apply(null, new Array(${n}))`));
  add(probe(`function f() { return Array.from(arguments).length } f.apply(null, new Array(${n}))`));
  add(probe(`function f(a) { "use strict"; return arguments.length } f.apply(null, new Array(${n}))`));
  add(probe(`function f(a) { return arguments.length } f.apply(null, new Array(${n}))`));
  add(probe(`function f(a, b) { arguments[0] = 5; return a } f.apply(null, new Array(${n}))`));
  add(probe(`function f() { return arguments[${n - 1}] } f.apply(null, Array(${n}).fill(7))`));
  add(probe(`Function.prototype.call.apply(function () { return arguments.length }, new Array(${n}))`));
  add(probe(`new Function("return arguments.length").apply(null, new Array(${n}))`));
  add(probe(`(() => arguments.length).apply === undefined`));
  add(probe(`((...a) => a.length)(...new Array(${n}))`));
  add(probe(`((a, ...r) => r.length)(...new Array(${n}))`));
  add(probe(`class A { constructor(...a) { this.n = a.length } } new A(...new Array(${n})).n`));
  add(probe(`class A { constructor() { this.n = arguments.length } } class B extends A { constructor() { super(...new Array(${n})) } } new B().n`));
  add(probe(`class A { constructor() { this.n = arguments.length } } class B extends A { } new B(...new Array(${n})).n`));
  add(probe(`async function f() { return arguments.length } f.apply(null, new Array(${n})) instanceof Promise`));
  add(probe(`function* g() { yield arguments.length } g.apply(null, new Array(${n})).next().value`));
  add(probe(`Object.assign({}, ...new Array(${n})).constructor.name`));
  add(probe(`Math.max.apply(null, { length: ${n}, 0: 1 })`));
  add(probe(`String.raw({ raw: new Array(${n}).fill("a") }).length`));
  add(probe(`String.raw({ raw: ["a", "b"] }, ...new Array(${n}))`));
  add(probe(`"".concat(...new Array(${n}).fill("a")).length`));
  add(probe(`"".concat.apply("", new Array(${n}).fill("a")).length`));
  add(probe(`Object.defineProperties({}, {}) && Object.assign.apply(null, [{}].concat(new Array(${n}))).constructor.name`));
  add(probe(`Promise.all.call(null, ...new Array(${n})) === 0`));
  add(probe(`new Set([].concat(new Array(${n}))).size`));
  add(probe(`Array.prototype.flat.apply(new Array(${n})).length`));
  add(probe(`var s = Symbol.for; s.apply(null, new Array(${n})).description`));
  add(probe(`eval.apply(null, new Array(${n}))`));
  add(probe(`parseInt.apply(null, new Array(${n}))`));
  add(probe(`isNaN.apply(null, new Array(${n}))`));
  add(probe(`Date.UTC.apply(null, new Array(${n}).fill(1))`));
  add(probe(`new Date(...new Array(${n}).fill(1)).getTime()`));
  add(probe(`Number.apply(null, new Array(${n}).fill(1))`));
  add(probe(`String.apply(null, new Array(${n}).fill(1)).length`));
  add(probe(`Array.prototype.indexOf.apply(new Array(${n}), [undefined, 0])`));
  add(probe(`Object.keys.apply(null, [new Array(${n})]).length`));
}
// arguments grande dentro de função recursiva leve.
for (const n of [100, 1000, 10000, 70000]) {
  add(probe(`function f(k, ...a) { return k === 0 ? a.length : f(k - 1, ...a, 1) } f(${Math.min(n, 100)})`));
  add(probe(`function f() { return arguments.length } f.apply(null, new Array(${n}).fill(0)) + f.apply(null, new Array(${n}).fill(0))`));
  add(probe(`var a = new Array(${n}).fill(2); (function () { var s = 0; for (var i = 0; i < arguments.length; i++) s += arguments[i]; return s }).apply(null, a)`));
}

// ---- eval aninhado.
for (const n of [10, 100, 250, 500, 1000, 2000, 5000]) {
  add(probe(`var s = "1"; for (var i = 0; i < ${n}; i++) s = "eval(" + JSON.stringify(s) + ")"; eval(s)`));
  add(probe(`var s = "1"; for (var i = 0; i < ${n}; i++) s = "(0,eval)(" + JSON.stringify(s) + ")"; eval(s)`));
  add(probe(`var s = "1"; for (var i = 0; i < ${n}; i++) s = "new Function(" + JSON.stringify("return " + s) + ")()"; eval(s)`));
  add(probe(`var s = "var x = 1; x"; for (var i = 0; i < ${n}; i++) s = "eval(" + JSON.stringify(s) + ")"; eval(s)`));
  add(probe(`var s = "(function(){ return 1 })()"; for (var i = 0; i < ${n}; i++) s = "(function(){ return eval(" + JSON.stringify(s) + ") })()"; eval(s)`));
  add(probe(`"use strict"; var s = "1"; for (var i = 0; i < ${n}; i++) s = "eval(" + JSON.stringify(s) + ")"; eval(s)`));
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "limits-golden-"));
const file = path.join(dir, "limits_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const runOnce = source => {
  fs.writeFileSync(file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 3000, maxBuffer: 1 << 24 });
  if (run.error || run.status === null) return null;
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  return marked ? JSON.parse(marked.slice(1)) : null;
};
const seen = new Set();
let kept = 0;
let dropped = 0;
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  const first = runOnce(source);
  if (first === null) {
    dropped++;
    process.stderr.write("sem resultado ou lento: " + JSON.stringify(body).slice(0, 120) + "\n");
    continue;
  }
  const second = runOnce(source);
  if (second !== first) {
    dropped++;
    process.stderr.write("instável: " + JSON.stringify(body).slice(0, 120) + "\n");
    continue;
  }
  if (first.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(first)) {
    dropped++;
    process.stderr.write("caminho da máquina: " + JSON.stringify(body).slice(0, 120) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(first));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
