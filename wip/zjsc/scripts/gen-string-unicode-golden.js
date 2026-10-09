// Gera tests/golden/string_unicode_bun.tsv: String.prototype com Unicode de borda (surrogates soltos, casos especiais de
// caixa, normalização), limites de string, template com Symbol, `new String` e as funções de URI, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Cada expressão é embrulhada num auxiliar `F` que serializa o valor (ou "Nome: mensagem" da exceção).
// Uso: bun scripts/gen-string-unicode-golden.js > tests/golden/string_unicode_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const lines = [];

const HELPER =
  "function F(f){try{return D(f())}catch(e){return e instanceof Error?e.name+': '+e.message:'throw '+String(e)}}" +
  "function D(v){if(typeof v==='string')return 's'+JSON.stringify(v);if(typeof v==='symbol')return 'sym';" +
  "if(typeof v==='number')return Object.is(v,-0)?'-0':'n'+v;if(Array.isArray(v))return '['+v.map(D).join(',')+']';" +
  "if(v&&typeof v==='object')return 'o'+JSON.stringify(Object.keys(v))+Object.prototype.toString.call(v);" +
  "return typeof v+':'+String(v)}\n";

const exprs = [];
const add = e => exprs.push(e);

// Cadeias de teste (literais JS como texto de fonte).
const strs = [
  "''", "'abc'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'\\ude00\\ud83d'", "'a\\ud83db'", "'\\ud83d\\ud83d\\ude00'",
  "'a\\ud83d\\ude00b\\ud83d'", "'\\ud801\\udc28'", "'\\u00df'", "'\\u0130'", "'\\ufb03'", "'\\u0149'", "'\\u0390'",
  "'\\u03a3'", "'a\\u03a3'", "'a\\u03a3b'", "'A\\u03a3 '", "'\\u03a3\\u03a3'", "'\\u10d0'", "'\\u1c90'", "'\\u0041\\u030a'",
  "'\\u00c5'", "'\\u212b'", "'\\u1e9b\\u0323'", "'\\ufb00\\ufb01\\ufb02'", "'\\u1f80'", "'\\u0587'", "'\\u01c5'",
];
const intArgs = ["0", "1", "2", "-1", "-2", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'1'", "undefined", "null", "100", "-0"];

// ---- at / charAt / charCodeAt / codePointAt
for (const s of strs.slice(0, 12))
  for (const a of ["0", "1", "2", "-1", "-2", "NaN", "Infinity", "1.5", "undefined"])
    for (const m of ["at", "charAt", "charCodeAt", "codePointAt"]) add(`${s}.${m}(${a})`);

// ---- isWellFormed / toWellFormed / normalize
for (const s of strs) {
  add(`${s}.isWellFormed()`);
  add(`${s}.toWellFormed()`);
  for (const f of ["NFC", "NFD", "NFKC", "NFKD", "undefined"]) add(`${s}.normalize(${f === "undefined" ? "" : `'${f}'`})`);
}
for (const f of ["'nfc'", "''", "'NFX'", "null", "1", "Symbol()", "{toString(){return 'NFD'}}"]) add(`'a'.normalize(${f})`);

// ---- localeCompare sem locale
for (const a of ["''", "'a'", "'b'", "'A'", "'\\u00e9'", "'e\\u0301'", "'\\ud83d'", "'\\ud83d\\ude00'"])
  for (const b of ["''", "'a'", "'A'", "'\\u00e9'", "'e\\u0301'", "'\\ud83d\\ude00'", "undefined", "null"]) add(`${a}.localeCompare(${b})`);

// ---- padStart / padEnd / repeat
for (const m of ["padStart", "padEnd"])
  for (const [n, p] of [["5", "'ab'"], ["5", "''"], ["5", "undefined"], ["0", "'x'"], ["-1", "'x'"], ["NaN", "'x'"], ["6", "'\\ud83d\\ude00'"],
    ["4", "'\\ud83d\\ude00'"], ["3", "'\\ud83d'"], ["Infinity", "''"], ["2.9", "'xyz'"], ["'4'", "'-'"], ["5", "null"], ["5", "0"]])
    add(`'\\ud83d\\ude00'.${m}(${n}, ${p})`);
for (const n of ["0", "1", "3", "-1", "NaN", "Infinity", "-Infinity", "2.5", "'2'", "undefined", "null", "-0", "-0.5", "2**31", "2**30", "2**32"])
  add(`'ab'.repeat(${n})`), add(`''.repeat(${n})`);
add("'\\ud83d'.repeat(3)");

// ---- trim*
const ws = ["'\\u0009'", "'\\u000a'", "'\\u000b'", "'\\u000c'", "'\\u000d'", "'\\u0020'", "'\\u00a0'", "'\\u1680'", "'\\u2000'", "'\\u200a'",
  "'\\u2028'", "'\\u2029'", "'\\u202f'", "'\\u205f'", "'\\u3000'", "'\\ufeff'", "'\\u180e'", "'\\u200b'", "'\\u0085'", "'\\u2060'"];
for (const w of ws) for (const m of ["trim", "trimStart", "trimEnd"]) add(`(${w} + 'a' + ${w}).${m}().length`);
add("'\\ud83d\\ude00 '.trimEnd()"), add("' \\ud83d'.trimStart()");

// ---- split
for (const s of ["'a\\ud83d\\ude00b'", "'\\ud83d\\ude00'", "'\\ud83d'", "''", "'abc'", "'a,b,,c'"])
  for (const [sep, lim] of [["''", "undefined"], ["''", "2"], ["''", "0"], ["''", "-1"], ["''", "2**32"], ["''", "2**32+1"], ["''", "NaN"], ["''", "Infinity"],
    ["','", "undefined"], ["','", "2"], ["','", "0"], ["undefined", "undefined"], ["undefined", "0"], ["null", "undefined"], ["'\\ud83d'", "undefined"], ["'\\ude00'", "undefined"],
    ["/(?:)/u", "undefined"], ["/(?:)/", "undefined"], ["/(,)/", "undefined"], ["/,/", "1"]])
    add(`${s}.split(${sep}, ${lim})`);

// ---- replace com padrões especiais
const rs = ["'abc'", "'a\\ud83db'", "'\\ud83d\\ude00x\\ud83d'", "'xyz'", "''"];
const reps = ["'$&'", "'$`'", "\"$'\"", "'$$'", "'$1'", "'$01'", "'$10'", "'$<n>'", "'[$&$&]'", "'$'", "'$0'", "'$00'", "'\\ud83d$&'", "'$&\\ude00'", "'$<'"];
for (const s of rs) for (const r of reps) add(`${s}.replace('b', ${r})`), add(`${s}.replace('', ${r})`);
const rgx = [["/(b)/", reps.slice(4, 8)], ["/(?<n>b)/", ["'$<n>'", "'$<m>'", "'$<n'", "'$1$<n>'", "'$2'", "'$01'", "'$10'", "'$<>'"]],
  ["/(a)(b)(c)/", ["'$1$2$3'", "'$03'", "'$12'", "'$10'", "'$30'", "'$4'", "'$&|$`|$\\''"]],
  ["/(?:)/gu", ["'-'", "'$&'"]], ["/(?:)/g", ["'-'"]], ["/\\ud83d/", ["'X'"]], ["/\\ud83d/u", ["'X'"]]];
for (const s of ["'abc'", "'a\\ud83d\\ude00b'", "'\\ud83d\\ude00'"]) for (const [re, rr] of rgx) for (const r of rr) add(`${s}.replace(${re}, ${r})`);
add("'abc'.replace('b', (m, i, s) => m + i + s)"), add("'abc'.replace(/(?<n>b)/, (...a) => JSON.stringify(a))");
add("'aaa'.replaceAll('a', '$&$&')"), add("'aaa'.replaceAll('', '-')"), add("'a\\ud83d\\ude00'.replaceAll('', '-')"), add("'a'.replaceAll(/a/, 'b')");

// ---- indexOf / lastIndexOf
for (const s of ["'abcabc'", "'a\\ud83d\\ude00a\\ud83d\\ude00'"])
  for (const t of ["'a'", "''", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'z'", "undefined"])
    for (const p of ["NaN", "Infinity", "-Infinity", "-1", "0", "2", "100", "undefined", "-0"]) add(`${s}.indexOf(${t}, ${p})`), add(`${s}.lastIndexOf(${t}, ${p})`);
add("'abc'.indexOf()"), add("'abc'.lastIndexOf()"), add("'undefined'.indexOf()"), add("'abc'.includes('b', NaN)"), add("'abc'.startsWith('b', 1)"), add("'abc'.endsWith('b', 2)");
add("'abc'.includes(/b/)"), add("'abc'.startsWith(/b/)"), add("'abc'.endsWith(/b/)");

// ---- substring / substr / slice
for (const s of ["'abcdef'", "'a\\ud83d\\ude00b'"])
  for (const a of ["0", "2", "-2", "NaN", "Infinity", "-Infinity", "undefined", "100", "-100", "1.9"])
    for (const b of ["undefined", "0", "3", "-1", "NaN", "Infinity", "-Infinity", "1.9"])
      for (const m of ["substring", "substr", "slice"]) add(`${s}.${m}(${a}, ${b})`);

// ---- caixa
for (const s of strs) for (const m of ["toUpperCase", "toLowerCase", "toLocaleUpperCase", "toLocaleLowerCase"]) add(`${s}.${m}()`);
for (const s of ["'\\ud801\\udc28'", "'\\ud801\\udc00'", "'\\u1c90\\u1c91'", "'\\u10d0\\u10d1'", "'\\u0130i'", "'i\\u0307'", "'\\u03a3.'", "'a.\\u03a3'", "'\\u03a3\\u0301'", "'a\\u0301\\u03a3'",
  "'\\ufb06'", "'\\u1e9e'", "'\\u0587'", "'\\u1f88'", "'\\u1fb3'", "'\\u01f0'", "'\\u02bc\\u0073'", "'\\ud83d\\ude00'", "'\\ud801'", "'\\ud801a'", "'\\u0345'", "'\\u03c2'", "'\\u03a3\\u03a3\\u03a3'",
  "'\\u2160'", "'\\u24b6'", "'\\ua64a'", "'\\u1c88'", "'\\u0132'", "'\\u0149'"])
  for (const m of ["toUpperCase", "toLowerCase"]) add(`${s}.${m}()`);
for (const l of ["'tr'", "'az'", "'lt'", "'en'", "undefined", "'xx-invalid-'", "['tr']"]) for (const s of ["'Ii\\u0130\\u0131'", "'I\\u0307'"]) add(`${s}.toLocaleLowerCase(${l})`), add(`${s}.toLocaleUpperCase(${l})`);

// ---- String.raw
add("String.raw`a\\n${1}b`"), add("String.raw`\\u00zz`"), add("String.raw({raw: ['a','b','c']}, 1, 2, 3)"), add("String.raw({raw: 'abc'}, 1, 2)"), add("String.raw({raw: []}, 1)");
add("String.raw({raw: {length: 2, 0: 'x', 1: 'y'}}, 'M')"), add("String.raw({raw: {length: -1}})"), add("String.raw({raw: {length: NaN}})"), add("String.raw()"), add("String.raw({})"), add("String.raw(null)");
add("String.raw({raw: ['\\ud83d','\\ude00']}, '')"), add("String.raw({raw: {length: 3}}, 1, 2)"), add("String.raw({raw: [1,2]}, Symbol())"), add("String.raw({raw: {length: Infinity}})");

// ---- fromCodePoint / fromCharCode
for (const v of ["0", "65", "0x10ffff", "0x110000", "-1", "1.5", "NaN", "Infinity", "-Infinity", "'65'", "'x'", "undefined", "null", "{}", "0xd800", "0xdfff", "2**32", "2**32+65", "-0", "1e21", "Symbol()", "1n"])
  add(`String.fromCodePoint(${v})`), add(`String.fromCharCode(${v})`);
add("String.fromCodePoint()"), add("String.fromCharCode()"), add("String.fromCodePoint(0xd83d, 0xde00)"), add("String.fromCodePoint(0x1f600, 0x1f601)"), add("String.fromCharCode(0xd83d, 0xde00)");
add("String.fromCharCode(65.9, 66.1)"), add("String.fromCharCode(0x10041)"), add("String.fromCharCode(-1)"), add("String.fromCodePoint(65, -1)"), add("String.fromCodePoint(65, 0x110000)");
add("String.fromCodePoint.length"), add("String.fromCharCode.length"), add("String.raw.length");

// ---- String(Symbol()) e conversões
add("String(Symbol())"), add("String(Symbol('d'))"), add("String(Symbol(''))"), add("String(Symbol.iterator)"), add("new String(Symbol())"), add("Symbol() + ''"), add("'' + Symbol()"),
  add("`${Symbol()}`"), add("`a${Symbol('x')}b`"), add("`${{toString(){return Symbol()}}}`"), add("[Symbol()].join()"), add("'a'.concat(Symbol())"), add("'a'.padStart(5, Symbol())"),
  add("'a'.repeat(Symbol())"), add("Symbol('d').toString()"), add("Symbol('d').description"), add("String({[Symbol.toPrimitive](){return Symbol()}})"), add("`${{[Symbol.toPrimitive](h){return h}}}`"),
  add("String(null)"), add("String(undefined)"), add("String(-0)"), add("String(1n)"), add("String()"), add("String(void 0, 1)"), add("`${-0}`"), add("`${[1,[2,3]]}`"), add("`${{}}`"), add("`${null}${undefined}`");

// ---- new String: indexação e propriedades próprias
for (const s of ["'abc'", "''", "'a\\ud83d\\ude00'", "'\\ud83d'"]) {
  add(`Object.getOwnPropertyNames(new String(${s}))`);
  add(`Reflect.ownKeys(new String(${s}))`);
  add(`Object.keys(new String(${s}))`);
  add(`JSON.stringify(Object.getOwnPropertyDescriptors(new String(${s})))`);
  add(`(function(){var o = new String(${s}); var r = []; for (var k in o) r.push(k); return r})()`);
  add(`(function(){var o = new String(${s}); o.x = 1; o[5] = 2; o[3] = 3; o[1] = 9; return Reflect.ownKeys(o)})()`);
  add(`(function(){'use strict'; var o = new String(${s}); try { o[0] = 'z' } catch (e) { return e.name + ': ' + e.message } return o[0]})()`);
  add(`(function(){'use strict'; var o = new String(${s}); try { o.length = 5 } catch (e) { return e.name + ': ' + e.message } return o.length})()`);
  add(`(function(){'use strict'; var o = new String(${s}); try { delete o[0] } catch (e) { return e.name + ': ' + e.message } return 'ok'})()`);
  add(`(function(){var o = new String(${s}); return [delete o[0], delete o.length, 0 in o, 5 in o, '0' in o, o.hasOwnProperty(0), o.hasOwnProperty(-0), o.hasOwnProperty('00')]})()`);
  add(`Object.getOwnPropertyDescriptor(new String(${s}), '0')`);
  add(`Object.isFrozen(Object.freeze(new String(${s})))`);
  add(`Object.getOwnPropertyDescriptor(new String(${s}), 'length').value`);
}
add("new String('ab')[0] + new String('ab')[1] + new String('ab')[2]"), add("new String('ab')['1']"), add("new String('ab')[1.0]"), add("new String('ab')['01']"), add("new String('ab')[-1]");
add("Object.defineProperty(new String('ab'), '0', {value: 'a'})[0]"), add("Object.defineProperty(new String('ab'), '0', {value: 'z'})"), add("Object.defineProperty(new String('ab'), '2', {value: 'z'})[2]");
add("Object.defineProperty(new String('ab'), 'length', {value: 5})"), add("Object.defineProperty(new String('ab'), '1', {writable: true})");
add("Object.getOwnPropertyNames(Object('a'))"), add("typeof new String('a')"), add("typeof String('a')"), add("new String('a') == 'a'"), add("new String('a') === 'a'"), add("Object.prototype.toString.call(new String('a'))");
add("new String(Symbol())"), add("new String(1, 2).length"), add("new String().length"), add("new String(undefined).length"), add("Object.keys(Object.assign({}, 'ab'))"), add("Object.entries('ab')"), add("Object.values('a\\ud83d\\ude00')");
add("Object.getPrototypeOf(new String('a')) === String.prototype"), add("String.prototype.length"), add("Object.getOwnPropertyNames(String.prototype).includes('0')"), add("String.prototype.valueOf.call(1)"), add("String.prototype.toString.call({})");
add("String.prototype.valueOf.call(new String('q'))"), add("String.prototype.at.call(null)"), add("String.prototype.at.call(undefined, 0)"), add("String.prototype.trim.call(null)"), add("String.prototype.isWellFormed.call(undefined)");
add("String.prototype.normalize.call(null)"), add("String.prototype.padStart.call(undefined, 2)"), add("String.prototype.repeat.call(null, 2)"), add("String.prototype.split.call(null, 'a')"), add("String.prototype.replace.call(null, 'a', 'b')");
add("String.prototype.localeCompare.call(null, 'a')"), add("String.prototype.toUpperCase.call(null)"), add("String.prototype.indexOf.call(undefined)"), add("String.prototype.substr.call(null)"), add("String.prototype.slice.call(null)"), add("String.prototype.charAt.call(null)");
add("String.prototype.at.call(12345, -2)"), add("String.prototype.charAt.call({toString(){return 'xyz'}}, 1)"), add("String.prototype.toUpperCase.call(true)"), add("String.prototype.concat.call(1, 2, 3)");

// ---- iterador
for (const s of ["'a\\ud83d\\ude00b'", "'\\ud83d'", "'\\ude00'", "'\\ude00\\ud83d'", "'\\ud83d\\ud83d\\ude00'", "''", "'\\ud83d\\ude00\\ud83d\\ude00'"]) {
  add(`[...${s}]`);
  add(`Array.from(${s}).map(c => c.length)`);
  add(`Array.from(${s}, c => c.codePointAt(0))`);
  add(`[...${s}].length`);
  add(`${s}[Symbol.iterator]().next()`);
  add(`(function(){var it = ${s}[Symbol.iterator](); var r = []; var n; while (!(n = it.next()).done) r.push(n.value); return [r, it.next().done]})()`);
}
add("String.prototype[Symbol.iterator].call(null)"), add("String.prototype[Symbol.iterator].call(12)[Symbol.toStringTag]"), add("Object.prototype.toString.call(''[Symbol.iterator]())");
add("String.prototype[Symbol.iterator].name"), add("''[Symbol.iterator]().next.call({})"), add("Object.getPrototypeOf(''[Symbol.iterator]()).hasOwnProperty('next')");

// ---- strings enormes (cada uma só mede o comprimento ou o erro)
for (const e of ["28", "29", "30", "31"]) add(`'x'.repeat(2**${e}).length`);
add("'x'.repeat(2**31-1).length"), add("'x'.repeat(2**31).length"), add("'xx'.repeat(2**30).length"), add("'xx'.repeat(2**29).length"), add("'x'.repeat(2**32)"), add("'x'.repeat(2**53)"), add("''.repeat(2**31)");
add("'abc'.repeat(2**30)"), add("'abc'.repeat(2**29).length"), add("'\\u1234'.repeat(2**30).length"), add("'\\u1234'.repeat(2**29).length");
add("var s = 'x'.repeat(2**29); (s + s).length"), add("var s = 'x'.repeat(2**30 - 1); s.length"), add("var s = 'x'.repeat(2**30 - 1); (s + 'y').length");
add("var s = 'x'.repeat(2**30 - 1); (s + s).length"), add("var s = 'x'.repeat(2**30); (s + s).length"), add("var s = 'x'.repeat(2**30); s.concat(s).length");
add("var s = 'x'.repeat(2**30); `${s}${s}`.length"), add("var s = 'x'.repeat(2**30); [s, s].join('').length"), add("var s = 'x'.repeat(2**30); s.padEnd(2**31).length");
add("'x'.padEnd(2**31)"), add("'x'.padStart(2**31 - 1, 'y').length"), add("'x'.padEnd(2**30, 'y').length"), add("'x'.padEnd(2**32, 'y')"), add("'x'.padEnd(2**53, 'y')");
add("[1,2,3].join('x'.repeat(2**30))"), add("Array(2**30).join('xx').length"), add("Array(2**31).join('x')"), add("new Array(2**28).join('ab').length");
add("var s = 'x'.repeat(2**28); s.split('').length"), add("var s = 'a'.repeat(2**25); s.replaceAll('a', 'bb').length"), add("var s = 'a'.repeat(2**25); s.replaceAll('a', 'b'.repeat(2**7)).length");
add("'ab'.repeat(2**30).length"), add("String.fromCharCode.apply(null, new Array(2**17).fill(65)).length"), add("'x'.repeat(2**28).toUpperCase().length"), add("'\\u00df'.repeat(2**28).toUpperCase().length");
add("'\\u00df'.repeat(2**30 - 1).toUpperCase().length"), add("'\\ufb03'.repeat(2**29).toUpperCase().length"), add("'\\u00e9'.repeat(2**28).normalize('NFD').length");

// ---- encodeURI / decodeURI / escape / unescape
const uriFns = ["encodeURI", "decodeURI", "encodeURIComponent", "decodeURIComponent"];
const encInputs = ["''", "'abc'", "'a b'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'\\ude00\\ud83d'", "'a\\ud83d'", "'\\ud83da'", "'\\ud83d\\ud83d'", "'\\u0000'", "'\\u007f'", "'\\u0080'", "'\\u07ff'", "'\\u0800'",
  "'\\uffff'", "'\\ud7ff'", "'\\ue000'", "'\\udbff\\udfff'", "'\\udbff\\udc00'", "'\\ud800\\udc00'", "'-_.!~*\\'()'", "';/?:@&=+$,#'", "'[]{}|\\\\^`<>\"%'", "'%'", "'\\ufeff'", "'\\u2028'", "'\\ud83d\\ude00\\ud83d'"];
for (const i of encInputs) for (const f of ["encodeURI", "encodeURIComponent", "escape"]) add(`${f}(${i})`);
const decInputs = ["''", "'%41'", "'%4'", "'%'", "'%zz'", "'%4z'", "'%z4'", "'%80'", "'%BF'", "'%C0%80'", "'%C1%BF'", "'%C2%80'", "'%C2'", "'%C2%'", "'%C2%7F'", "'%C2%C0'", "'%DF%BF'", "'%E0%80%80'", "'%E0%9F%BF'", "'%E0%A0%80'",
  "'%ED%9F%BF'", "'%ED%A0%80'", "'%ED%BF%BF'", "'%EE%80%80'", "'%EF%BF%BF'", "'%E2%82'", "'%E2%82%AC'", "'%F0%80%80%80'", "'%F0%8F%BF%BF'", "'%F0%90%80%80'", "'%F4%8F%BF%BF'", "'%F4%90%80%80'", "'%F5%80%80%80'", "'%F8%80%80%80%80'",
  "'%FC%80%80%80%80%80'", "'%FE'", "'%FF'", "'%F0%9F%98'", "'%F0%9F%98%80'", "'%f0%9f%98%80'", "'%25'", "'%2525'", "'%23'", "'%3B%2F%3F%3A%40%26%3D%2B%24%2C'", "'%20%21%27'", "'%0A'", "'%00'", "'%7F'", "'a%2'", "'%E0%A0'", "'%E0%A0%'",
  "'%C2%80%C2'", "'%ud83d'", "'\\ud83d%41'", "'%41\\ude00'", "'%C3%A9'", "'%c3%a9'", "'%C3%a9'", "'%C3%A'", "'%F0%9F%98%80%F0%9F%98%80'", "'%EF%BB%BF'", "'%E1%80%80'", "'%E1%80'", "'%F1%80%80%80'", "'%F3%BF%BF%BF'"];
for (const i of decInputs) for (const f of ["decodeURI", "decodeURIComponent", "unescape"]) add(`${f}(${i})`);
for (const i of ["'%u0041'", "'%u00e9'", "'%uD83D%uDE00'", "'%u12'", "'%u'", "'%uZZZZ'", "'%41%u0042'", "'%'", "'%4'", "'%zz'", "'%E9'", "'%e9'", "'%u+041'", "'%U0041'", "'%00'", "'%uFFFF'", "'%u004'", "'%u0041%'", "'%1'", "'abc%'"]) add(`unescape(${i})`);
for (const i of ["'\\u00e9'", "'\\u0100'", "'\\uffff'", "'\\ud83d\\ude00'", "'@*_+-./'", "'!#$%&'", "'\\u0000\\u001f\\u007f\\u00ff'", "'az09AZ'", "' '", "'\\u00a0'"]) add(`escape(${i})`);
for (const f of uriFns) { add(`${f}()`); add(`${f}(undefined)`); add(`${f}(null)`); add(`${f}(1)`); add(`${f}({toString(){return 'a b'}})`); add(`${f}(Symbol())`); add(`${f}.length`); add(`${f}.name`); }
add("escape()"), add("unescape()"), add("escape(Symbol())"), add("unescape(Symbol())"), add("escape.length"), add("unescape.name");
for (const [i, j] of [["'%C3%A9'", "'%C3'"], ["'%41'", "'%zz'"]]) add(`decodeURI(${i} + ${j})`);
add("decodeURI('%23%24%26%2B%2C%2F%3A%3B%3D%3F%40')"), add("decodeURI('%2d%2D')"), add("decodeURI('%3b')"), add("decodeURI('%3B')"), add("decodeURI('%E3%80%80%23')"), add("decodeURI('%F0%9F%98%80%23')");
add("decodeURIComponent('%23%24%26')"), add("encodeURI('\\ud83d\\ude00#?')"), add("encodeURIComponent('#?&=')");
add("decodeURIComponent('%'.repeat(1000))"), add("decodeURIComponent('%41'.repeat(100000)).length"), add("encodeURIComponent('\\u00e9'.repeat(100000)).length"), add("encodeURIComponent('x'.repeat(2**28)).length");
add("encodeURIComponent('\\ud83d\\ude00'.repeat(2**27)).length"), add("encodeURIComponent('\\u0800'.repeat(2**28)).length"), add("encodeURIComponent('\\u0800'.repeat(2**29)).length"), add("escape('\\u0800'.repeat(2**28)).length");
for (let b = 0; b < 256; b += 17) add(`decodeURIComponent('%${b.toString(16).toUpperCase().padStart(2, "0")}')`);
for (let c = 0x80; c <= 0x10ffff; c = c < 0x800 ? c * 2 : c < 0x10000 ? c * 3 : c + 0x40000) {
  if (c >= 0xd800 && c < 0xe000) continue;
  add(`encodeURIComponent(String.fromCodePoint(${c}))`);
  add(`encodeURIComponent(String.fromCodePoint(${c} - 1))`);
}
for (const c of [0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff]) add(`decodeURIComponent(encodeURIComponent(String.fromCodePoint(${c})))`);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "string-unicode-golden-"));
const file = path.join(dir, "string_unicode_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const e of exprs) {
  if (seen.has(e)) continue;
  seen.add(e);
  // Programas com `var` viram corpo de função cujo último trecho é o valor devolvido.
  let body = "(" + e + ")";
  if (e.startsWith("var ")) {
    const cut = e.lastIndexOf("; ");
    body = "{ " + e.slice(0, cut) + "; return (" + e.slice(cut + 2) + "); }";
  }
  const original = '"use strict";\n' + HELPER + "globalThis.R = F(() => " + body + ");";
  // O programa gravado é o fonte já transpilado pelo bun; o que o bun executa é `executableSource(original)`, para as
  // posições do stack saírem no fonte original. `meta` leva o modo e o mapa de posições (quinta coluna do tsv).
  // `canonicalSource` preserva a diretiva "use strict" do topo; se o bun não parseia, fica o original.
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 20000, maxBuffer: 1 << 26 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(e) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  // Sucesso com string de 100 milhões de unidades ou mais pesa demais para o teste e depende da memória da máquina.
  if (result.includes(dir) || /^n\d{9,}$/.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("string_unicode", lines));
fs.rmSync(dir, { recursive: true, force: true });
