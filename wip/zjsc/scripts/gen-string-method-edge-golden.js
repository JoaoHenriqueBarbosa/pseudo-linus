// Gera tests/golden/string_method_edge_bun.tsv: bordas de String.prototype e dos wrappers Number/Boolean/String que
// string_bun.tsv, string_unicode_bun.tsv e number_*.tsv não cobrem, medidas no bun 1.4.2. Índices NaN/negativos/
// Infinity/fracionários/-0 em substring/substr/slice/at/charAt/charCodeAt/codePointAt, indexOf/lastIndexOf com posição
// estranha, split com limite 0/2**32, repeat com contagem de borda e RangeError, padStart/padEnd com fill vazio, concat
// com objetos, trim com todos os whitespaces Unicode, localeCompare e normalize, fromCharCode/fromCodePoint com valores
// de borda e erros, template literal e String.raw, startsWith/endsWith/includes com RegExp (TypeError), métodos HTML
// (anchor, big, blink, fixed, fontcolor, link) com aspas, comprimento máximo (RangeError sem alocar), toString/valueOf
// dos wrappers, comparação com surrogates e número para string em radix e exponenciais.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança). Caminho da máquina descarta o programa.
// Uso: bun scripts/gen-string-method-edge-golden.js > tests/golden/string_method_edge_bun.tsv
const fs = require("fs");
const { emitRow, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n';

const programs = [];
const q = s => JSON.stringify(s);
// Expressão avaliada; o resultado vira texto por S, a exceção vira `Nome: mensagem`.
const E = expr =>
  programs.push(PRELUDE + `try { globalThis.R = S(${expr}) } catch (e) { globalThis.R = e.name + ': ' + e.message }`);

// ---- 1. Índices de borda em substring/substr/slice/at/charAt/charCodeAt/codePointAt.
const indexes = ["NaN", "-1", "-0", "0", "1", "2", "3", "5", "Infinity", "-Infinity", "1.9", "-1.9", "0.5", "2**32", "2**53", "-(2**32)", "'2'", "'x'", "null", "undefined", "true", "({})", "[]", "[1]"];
const base = q("abcde");
for (const a of indexes) {
  for (const m of ["substring", "slice", "substr"]) E(`${base}.${m}(${a})`);
  for (const m of ["at", "charAt", "charCodeAt", "codePointAt"]) E(`${base}.${m}(${a})`);
  for (const m of ["indexOf", "lastIndexOf", "includes", "startsWith", "endsWith"]) E(`${base}.${m}("c", ${a})`);
}
for (const a of ["NaN", "-1", "0", "1", "2", "Infinity", "-Infinity", "1.9", "undefined", "-0"]) {
  for (const b of ["NaN", "-1", "0", "1", "3", "Infinity", "-Infinity", "1.9", "undefined", "-0"]) {
    for (const m of ["substring", "slice", "substr"]) E(`${base}.${m}(${a}, ${b})`);
  }
}
E(`"".at(0)`, `""`);
E(`"".charAt(0)`);
E(`"".charCodeAt(0)`);
E(`"".codePointAt(0)`);
E(`"\\ud83d\\ude00".codePointAt(0)`);
E(`"\\ud83d\\ude00".codePointAt(1)`);
E(`"\\ud83d\\ude00".at(-1)`);
E(`"\\ud83d".codePointAt(0)`);
E(`"abc".at()`);
E(`"abc".charAt()`);
E(`"abc".charCodeAt()`);

// ---- 2. indexOf/lastIndexOf com vazio, repetido e posição estranha.
for (const needle of ['""', '"a"', '"aa"', '"ab"', '"abab"', '"b"', '"z"']) {
  for (const pos of ["undefined", "NaN", "-1", "0", "1", "2", "3", "4", "5", "Infinity", "-Infinity", "1.5"]) {
    E(`"abab".indexOf(${needle}, ${pos})`);
    E(`"abab".lastIndexOf(${needle}, ${pos})`);
  }
}
E(`"abc".indexOf()`);
E(`"undefined".indexOf()`);
E(`"abc".lastIndexOf()`);
E(`"null".indexOf(null)`);
E(`"abc".indexOf("")`);
E(`"abc".lastIndexOf("")`);
E(`"abc".lastIndexOf("", NaN)`);
E(`"abc".lastIndexOf("c", -Infinity)`);

// ---- 3. split.
for (const lim of ["undefined", "0", "1", "2", "3", "-1", "2**32", "2**32+1", "2**32-1", "NaN", "Infinity", "1.9", "'2'", "null", "-0"]) {
  E(`"a,b,c".split(",", ${lim})`);
  E(`"abc".split("", ${lim})`);
  E(`"abc".split(undefined, ${lim})`);
}
for (const sep of ['""', '","', '"abc"', '"abcd"', "undefined", "null", "/,/", "/(,)/", "/(?:)/", "/x*/", "{[Symbol.split](s,l){return [s,l]}}", "1", "{toString(){return ','}}"]) {
  E(`"a,b,c".split(${sep})`);
  E(`"".split(${sep})`);
  E(`"a,b,c".split(${sep}, 2)`);
}
E(`"\\ud83d\\ude00x".split("")`);
E(`"a1b2c".split(/\\d/)`);
E(`"a1b2c".split(/(\\d)/, 3)`);
E(`"test".split(/(?:)/u)`);

// ---- 4. repeat.
for (const c of ["0", "1", "2", "-1", "-0", "0.9", "1.9", "-0.9", "NaN", "Infinity", "-Infinity", "'3'", "'x'", "null", "undefined", "true", "2**31", "2**32", "2**53", "1e10", "({valueOf(){return 2}})"]) {
  E(`"ab".repeat(${c})`);
  E(`"".repeat(${c})`);
}
E(`"ab".repeat()`);
E(`"ab".repeat(2**30).length`);
E(`"ab".repeat(2**30)`.replace(/^.*$/, `"ab".repeat(2**30).repeat(2).length`));
E(`"a".repeat(2**31)`);
E(`"ab".repeat(2**31)`);
E(`"abc".repeat(2**30)`);
E(`"".repeat(2**31).length`);
E(`"".repeat(Infinity)`);
E(`"".repeat(-1)`);
E(`String.prototype.repeat.call(null, 1)`);
E(`String.prototype.repeat.call(undefined, 1)`);
E(`String.prototype.repeat.call(12, 2)`);
E(`String.prototype.repeat.call({toString(){return "q"}}, 3)`);

// ---- 5. padStart/padEnd.
for (const m of ["padStart", "padEnd"]) {
  for (const len of ["-1", "0", "2", "5", "7", "NaN", "Infinity", "1.9", "'6'", "undefined", "2**32", "2**31"]) {
    for (const fill of ["undefined", '""', '" "', '"ab"', '"abcdefgh"', "null", "1", "{toString(){return 'z'}}", "{}", "Symbol()"]) {
      if (/2\*\*3|Infinity/.test(len)) {
        if (fill !== '"ab"' && fill !== '""' && fill !== "undefined") continue;
      }
      E(`${q("abc")}.${m}(${len}, ${fill})`);
    }
  }
  E(`"abc".${m}()`);
  E(`"".${m}(3, "xy")`);
  E(`"\\ud83d\\ude00".${m}(4, "\\ud83d\\ude00")`);
  E(`"abc".${m}(5, "\\ud83d")`);
  E(`String.prototype.${m}.call(null, 3)`);
}

// ---- 6. concat.
for (const args of ["", "1", "null, undefined", "[1,2], [3]", "{}", "{toString(){return 'T'}}", "{valueOf(){return 'V'}}", "{[Symbol.toPrimitive](){return 'P'}}", "Symbol()", "1n", "-0", "NaN", "true, false", "new String('w')", "new Number(7)", "[[]]", "{toString(){throw new Error('boom')}}", "function(){}", "'a','b','c'"]) {
  E(`"x".concat(${args})`);
  E(`"".concat(${args})`);
}
E(`String.prototype.concat.call(1, 2)`);
E(`String.prototype.concat.call(null)`);
E(`String.prototype.concat.length`);
E(`"a".concat.name`);

// ---- 7. trim com whitespaces Unicode.
const wsCodes = [0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0x85, 0xa0, 0x1680, 0x180e, 0x2000, 0x2001, 0x2003, 0x2006, 0x2007, 0x200a, 0x200b, 0x200c, 0x200d, 0x2028, 0x2029, 0x202f, 0x205f, 0x2060, 0x3000, 0xfeff, 0xfffe, 0x0, 0x1c, 0x1f];
for (const c of wsCodes) {
  const s = `String.fromCharCode(${c})`;
  E(`(${s}+"a"+${s}).trim().length`);
  E(`(${s}+"a"+${s}).trimStart().length`);
  E(`(${s}+"a"+${s}).trimEnd().length`);
  E(`(${s}).trim().length`);
  E(`(${s}+"a b"+${s}).trim()`);
}
E(`"\\u2028\\u2029 x \\ufeff\\u00a0".trim()`);
E(`"\\u180e x".trim().length`);
E(`"\\ud83d\\ude00 ".trim()`);
E(`String.prototype.trim.call(null)`);
E(`String.prototype.trimLeft === String.prototype.trimStart`);
E(`String.prototype.trimRight === String.prototype.trimEnd`);
E(`String.prototype.trimLeft.name`);
E(`String.prototype.trimRight.name`);
E(`/\\s/.test("\\u180e")`);
E(`/\\s/.test("\\ufeff")`);
E(`Number("\\u180e1")`);
E(`Number("\\ufeff1\\u2028")`);
E(`Number("\\u00a05\\u00a0")`);
E(`parseInt("\\u180e9")`);
E(`parseFloat("\\u20289.5x")`);

// ---- 8. localeCompare e normalize (fora do foco, só o que é determinístico).
for (const [a, b] of [["a", "b"], ["b", "a"], ["a", "a"], ["", "a"], ["a", ""], ["a", "A"], ["A", "a"], ["é", "é"], ["z", "ä"], ["a", "😀"], ["10", "9"], ["abc", "abd"], ["abc", "ab"]]) {
  E(`${q(a)}.localeCompare(${q(b)})`);
}
E(`"a".localeCompare()`);
E(`"undefined".localeCompare()`);
E(`"a".localeCompare("b", "en", {sensitivity:"base"})`);
E(`"a".localeCompare("A", "en", {sensitivity:"base"})`);
E(`"a".localeCompare("b", "xx-invalid-locale-zzzzzzzzzz")`);
E(`String.prototype.localeCompare.call(null, "a")`);
for (const f of ["NFC", "NFD", "NFKC", "NFKD", "nfc", "NFX", "", "undefined", "null", "1"]) {
  E(`"\\u1e9b\\u0323".normalize(${q(f)}).length`);
  E(`"\\u00c5".normalize(${q(f)}).length`);
}
E(`"\\u1e9b\\u0323".normalize().length`);
E(`"\\u1e9b\\u0323".normalize(undefined).length`);
E(`"\\ufb01".normalize("NFKC")`);
E(`"\\ud83d".normalize("NFC").length`);
E(`"".normalize("NFD")`);
E(`String.prototype.normalize.call(undefined)`);

// ---- 9. fromCharCode / fromCodePoint.
for (const c of ["", "65", "-1", "65536", "65601", "0x10ffff", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'66'", "'x'", "null", "undefined", "true", "{}", "[67]", "2**32+65", "-(2**32)+65", "2**53", "0xd83d, 0xde00", "0xd83d", "0xde00, 0xd83d", "65, 66, 67", "1e21", "Symbol()", "1n"]) {
  E(`String.fromCharCode(${c}).length`);
  E(`String.fromCharCode(${c}).split("").map(x=>x.charCodeAt(0))`);
  E(`String.fromCodePoint(${c})`.replace(/String.fromCodePoint\((.*)\)$/, "String.fromCodePoint($1)"));
}
for (const c of ["0x10ffff", "0x110000", "-1", "-0", "1.5", "NaN", "Infinity", "'65'", "'0x41'", "'x'", "undefined", "null", "{}", "0xd800", "0xdfff", "0xd800, 0xdc00", "0x1f600, 0x41", "2**32", "2**32+65", "1e21", "[]", "[65]", "true", "0, 0x10ffff", "Symbol()", "1n"]) {
  E(`String.fromCodePoint(${c}).length`);
  E(`Array.from(String.fromCodePoint(${c}), x=>x.codePointAt(0))`);
}
E(`String.fromCodePoint()`);
E(`String.fromCharCode()`);
E(`String.fromCharCode.length`);
E(`String.fromCodePoint.length`);
E(`String.fromCharCode.call(null, 65)`);
E(`new String.fromCharCode(65)`);

// ---- 10. Template literal e String.raw.
E("`a${1}b${null}c${undefined}d${[1,2]}e${{}}f`");
E("`${Symbol()}`");
E("`${{toString(){return 'T'},valueOf(){return 'V'}}}`");
E("`${{[Symbol.toPrimitive](h){return h}}}`");
E("`${1n}${-0}${0.1+0.2}${1e21}${1e-7}`");
E("`line1\\nline2\\t\\u0041\\x41\\u{1F600}`.length");
E("`\\\n`.length");
E("`a\r\nb`.length");
E("`a\rb`.length");
E("`a\\\r\nb`.length");
E("(x=>x)`a${1}b`.raw");
E("(x=>x.raw)`\\u{`");
E("(x=>x[0])`\\u{`");
E("(x=>x.raw[0])`\\xg\\unicode`");
E("(x=>x.length+':'+Object.isFrozen(x)+Object.isFrozen(x.raw))`a${1}b${2}c`");
E("(x=>x)`a` === (x=>x)`a`");
E("(()=>{const f=()=>(x=>x)`a`;return f()===f()})()");
E("String.raw`a\\n${1}b\\t${2}c`");
E("String.raw`\\u{`");
E("String.raw({raw:['a','b','c']}, 1, 2, 3)");
E("String.raw({raw:['a','b','c']})");
E("String.raw({raw:'abc'}, 1, 2)");
E("String.raw({raw:{length:3, 0:'x', 2:'z'}}, '-', '+')");
E("String.raw({raw:{length:-1}}, 1)");
E("String.raw({raw:{length:2**32}}, 1)".replace("2**32", "0"));
E("String.raw({raw:[]}, 1)");
E("String.raw({})");
E("String.raw()");
E("String.raw(null)");
E("String.raw({raw:null})");
E("String.raw({raw:['a','b']}, {toString(){return 'T'}})");
E("String.raw({raw:['a','b']}, Symbol())");
E("String.raw.length");
E("String.raw`${1}${2}`");
E("String.raw`\\``");
E("String.raw`$\\{`");
E("String.raw`\\$\\{1}`");

// ---- 11. startsWith/endsWith/includes com RegExp (TypeError) e Symbol.match.
for (const m of ["startsWith", "endsWith", "includes"]) {
  for (const arg of ["/a/", "/a/g", "new RegExp('a')", "{[Symbol.match]: true}", "{[Symbol.match]: 1}", "{[Symbol.match]: 'x'}", "{[Symbol.match]: false}", "{[Symbol.match]: 0}", "{[Symbol.match]: undefined}", "{[Symbol.match]: null}", "{[Symbol.match]: ''}", "(()=>{const r=/a/;r[Symbol.match]=false;return r})()", "(()=>{const r=/a/;r[Symbol.match]=undefined;return r})()", "Object.assign(()=>{}, {[Symbol.match]: true})", "Symbol()", "{toString(){return 'a'}}", "'a'", "undefined", "null", "1", "[]", "['a']", "new String('a')"]) {
    E(`"banana".${m}(${arg})`);
  }
  E(`"banana".${m}()`);
  E(`${m === "includes" ? '"undefined"' : '"undefined"'}.${m}()`);
  E(`String.prototype.${m}.call(null, "a")`);
  E(`String.prototype.${m}.call(/a/, "/")`);
  E(`String.prototype.${m}.length`);
}
E(`"abc".endsWith("c", 3)`);
E(`"abc".endsWith("c", 2)`);
E(`"abc".endsWith("c", undefined)`);
E(`"abc".endsWith("c", NaN)`);
E(`"abc".endsWith("a", 1)`);
E(`"abc".endsWith("", -5)`);
E(`"abc".endsWith("abc", Infinity)`);
E(`"abc".startsWith("", 5)`);
E(`"abc".startsWith("c", 2)`);
E(`"abc".startsWith("bc", 1.9)`);
E(`"abc".startsWith("a", -Infinity)`);
E(`"abc".includes("", 99)`);
E(`"abc".includes("c", -99)`);

// ---- 12. Métodos HTML.
for (const m of ["anchor", "big", "blink", "bold", "fixed", "fontcolor", "fontsize", "italics", "link", "small", "strike", "sub", "sup"]) {
  for (const arg of ['"x"', '"a\\"b"', '"\\"\\""', '""', "undefined", "null", "1", "{toString(){return '\"q\"'}}", '"<>&\'"', '"a\\u0022b"', "Symbol()"]) {
    E(`"txt".${m}(${arg})`);
  }
  E(`"".${m}("v")`);
  E(`"<b>&\\"".${m}("v")`);
  E(`String.prototype.${m}.call(null, "v")`);
  E(`String.prototype.${m}.call(12, "v")`);
  E(`String.prototype.${m}.length`);
  E(`String.prototype.${m}.name`);
  E(`"x".${m}.call === Function.prototype.call`);
}
E(`"x".anchor("a", "b")`);
E(`"x".big("arg")`);
E(`"x".fontcolor()`);
E(`"x".link()`);

// ---- 13. Wrappers: toString/valueOf e exceções.
for (const w of ["new String('s')", "new Number(5)", "new Number(-0)", "new Boolean(false)", "new Boolean(true)", "Object('x')", "Object(1)", "Object(true)", "Object(1n)", "Object(Symbol.iterator)"]) {
  E(`typeof ${w}`);
  E(`${w}.valueOf()`);
  E(`${w}.toString()`);
  E(`${w} + ""`);
  E(`${w} + 1`);
  E(`Object.prototype.toString.call(${w})`);
  E(`${w} == ${w}.valueOf()`);
  E(`${w} === ${w}.valueOf()`);
  E(`!!${w}`);
  E(`JSON.stringify(${w})`);
  E(`Object.getOwnPropertyNames(${w})`);
}
for (const [recv, name] of [["String", "valueOf"], ["String", "toString"], ["Number", "valueOf"], ["Number", "toString"], ["Boolean", "valueOf"], ["Boolean", "toString"], ["Symbol", "valueOf"], ["BigInt", "valueOf"]]) {
  for (const t of ["undefined", "null", "1", "'s'", "true", "{}", "[]", "Symbol()", "1n", "new String('s')", "new Number(1)", "new Boolean(true)", "Object.create(String.prototype)", "Object.create(Number.prototype)", "String.prototype", "Number.prototype", "Boolean.prototype"]) {
    E(`${recv}.prototype.${name}.call(${t})`);
  }
}
E(`String.prototype.valueOf()`);
E(`Number.prototype.valueOf()`);
E(`Boolean.prototype.valueOf()`);
E(`String.prototype.length`);
E(`Object.prototype.toString.call(String.prototype)`);
E(`Object.prototype.toString.call(Number.prototype)`);
E(`Object.prototype.toString.call(Boolean.prototype)`);
E(`new String("abc").length`);
E(`Object.getOwnPropertyDescriptor(new String("abc"), "length")`.replace(/^.*$/, `JSON.stringify(Object.getOwnPropertyDescriptor(new String("abc"), "length"))`));
E(`JSON.stringify(Object.getOwnPropertyDescriptor(new String("abc"), "1"))`);
E(`Object.keys(new String("ab"))`);
E(`(()=>{"use strict";const s=new String("ab");s[0]="z";return s[0]})()`);
E(`(()=>{"use strict";const s=new String("ab");s[5]="z";return s[5]})()`);
E(`(()=>{const s=new String("ab");s.length=9;return s.length})()`);
E(`(()=>{"use strict";const s=new String("ab");s.length=9;return s.length})()`);
E(`(()=>{const s=new String("ab");s[3]="z";return Object.keys(s)})()`);
E(`(()=>{const s=new String("ab");delete s[0];return s[0]})()`);
E(`(()=>{"use strict";const s=new String("ab");delete s[0]})()`);
E(`(()=>{"use strict";return delete "abc"[0]})()`);
E(`(()=>{"use strict";return delete "abc".length})()`);
E(`(()=>{"use strict";"abc"[0]="z"})()`);
E(`(()=>{"use strict";(5).x=1})()`);
E(`(()=>{"use strict";true.x=1})()`);
E(`String(Symbol("d"))`);
E(`new String(Symbol("d"))`);
E(`"" + Symbol("d")`);
E(`String({toString(){return {}}, valueOf(){return 7}})`);
E(`String({toString(){return {}}, valueOf(){return {}}})`);
E(`String(null) + String(undefined) + String(-0) + String(1n)`);
E(`String()`);
E(`new String().length`);
E(`new String(undefined).length`);
E(`Number()`);
E(`Number(undefined)`);
E(`Number(null)`);
E(`Number("")`);
E(`Number(" 12 ")`);
E(`Number("0b101")`);
E(`Number("0o17")`);
E(`Number("0x1F")`);
E(`Number("-0x1F")`);
E(`Number("1_0")`);
E(`Number("1e1000")`);
E(`Number(1n)`);
E(`Number(Symbol())`);
E(`Number([5])`);
E(`Number([1,2])`);
E(`Number(new Date(5))`);
E(`Boolean("")`);
E(`Boolean("0")`);
E(`Boolean(0n)`);
E(`Boolean(document)`.replace("document", "{}"));
E(`Boolean(NaN)`);
E(`Boolean(new Boolean(false))`);
E(`new Boolean(new Boolean(false)).valueOf()`);
E(`Boolean()`);
E(`Boolean.length`);
E(`Number.length + ":" + String.length`);

// ---- 14. Comparação de strings com surrogates.
const cmpPairs = [
  ["\\ud800", "\\udfff"], ["\\ud83d\\ude00", "\\uffff"], ["\\ud83d\\ude00", "\\ue000"], ["\\ud83d\\ude00", "\\ud83d"], ["\\ud83d", "\\ud83d\\ude00"],
  ["\\ud83d\\ude00", "\\ud83d\\ude01"], ["\\uffff", "\\ud800"], ["\\ue000", "\\ud800"], ["a", "\\ud800"], ["\\ud800", "\\udc00"],
  ["\\udc00", "\\ud800"], ["\\ud83d\\ude00", "\\ud83d\\ude00"], ["", "\\ud800"], ["\\ud800a", "\\ud800b"], ["\\ud83d\\ude00a", "\\ud83d\\ude00"],
];
for (const [a, b] of cmpPairs) {
  const A = `"${a}"`, B = `"${b}"`;
  E(`${A} < ${B}`);
  E(`${A} > ${B}`);
  E(`${A} <= ${B}`);
  E(`${A} >= ${B}`);
  E(`${A} == ${B}`);
  E(`[${A}, ${B}].sort()`);
  E(`${A}.localeCompare(${B})`);
  E(`${A}.localeCompare(${B}) === -${B}.localeCompare(${A})`);
}
E(`"a" < "b" && "B" < "a" && "10" < "9"`);
E(`"" < "a"`);
E(`"a" < "a\\0"`);
E(`"\\0" < "\\u0001"`);
E(`"abc" < "abd" ? "lt" : "ge"`);
E(`"a" == new String("a")`);
E(`new String("a") == new String("a")`);
E(`"1" == 1 && "" == 0 && "0" == false && " \\n" == 0`);
E(`"\\u180e" == 0`);
E(`"\\ufeff" == 0`);
E(`"a" < 1`);
E(`"2" > 1`);
E(`"2" > "12"`);
E(`"b" > null`);
E(`"" < null`);
E(`"a" < undefined`);
E(`"a" >= undefined`);
E(`"9007199254740993" == 9007199254740992`);
E(`"9007199254740993" < "9007199254740992"`);
E(`"a" < 1n`);
E(`"1" == 1n`);
E(`"1.5" == 1n`);
E(`"0x10" == 16n`);
E(`" 1 " == 1n`);
E(`"abc" < 1n`);

// ---- 15. Número para string em radix e exponenciais.
const radixNums = ["0", "-0", "1", "-1", "255", "0.5", "-0.5", "0.1", "0.3", "1/3", "2**53", "2**53+2", "2**31", "2**64", "1e21", "1e-7", "123.456", "-123.456", "NaN", "Infinity", "-Infinity", "1e300", "5e-324", "1.7976931348623157e308", "0.000001", "1e-10", "35", "36", "0.9999999999999999", "4294967295.5", "2**-20", "3.141592653589793", "9007199254740993", "-2**31", "1e22", "255.255", "0.1+0.2"];
for (const n of radixNums) {
  for (const r of ["2", "3", "7", "8", "16", "32", "36", "undefined", "10"]) E(`(${n}).toString(${r})`);
}
for (const r of ["1", "0", "37", "-1", "NaN", "Infinity", "2.9", "'16'", "'x'", "null", "true", "[]", "{}", "36.9", "1.9", "-0", "2**32+2", "Symbol()", "1n"]) {
  E(`(255).toString(${r})`);
  E(`(NaN).toString(${r})`);
}
for (const n of ["0", "-0", "1", "123.456", "-1.5", "1e21", "1e-7", "NaN", "Infinity", "0.000123", "123456789", "5e-324", "1.7976931348623157e308", "0.1", "9.995", "1.005", "99.99", "1e100", "-1e-100", "2**53"]) {
  for (const d of ["undefined", "0", "1", "2", "5", "10", "20", "100", "-1", "101", "NaN", "1.9", "'3'", "null"]) {
    E(`(${n}).toExponential(${d})`);
    E(`(${n}).toFixed(${d})`);
    E(`(${n}).toPrecision(${d})`);
  }
}
E(`(1e21).toFixed(2)`);
E(`(1e21).toLocaleString === undefined`);
E(`(-1.5).toFixed(0)`);
E(`(2.5).toFixed(0)`);
E(`(0.5).toFixed(0)`);
E(`(1.45).toFixed(1)`);
E(`(8.345).toFixed(2)`);
E(`(1000000000000000128).toString()`);
E(`(1000000000000000128).toFixed(0)`);
E(`(0.000001).toString()`);
E(`(0.0000001).toString()`);
E(`(123456789012345680000).toString()`);
E(`(1e21).toString()`);
E(`(-1e-7).toString()`);
E(`String(-0) + (-0).toString() + (-0).toFixed(1) + (-0).toExponential() + (-0).toPrecision(2)`);
E(`(25).toString(36) + (0.5).toString(2) + (-255).toString(16)`);
E(`(0.1).toString(2)`);
E(`(1e21).toString(7)`);
E(`(1e-7).toString(36)`);
E(`Number.prototype.toString.call("1")`);
E(`Number.prototype.toFixed.call("1", 1)`);
E(`Number.prototype.toString.call(new Number(255), 16)`);
E(`Number.prototype.toString.length + ":" + Number.prototype.toFixed.length + ":" + Number.prototype.toExponential.length + ":" + Number.prototype.toPrecision.length`);
E(`(1).toString.call(1n)`);
E(`parseInt("ff", 16) + parseInt("z", 36) + parseInt("10", 2)`);
E(`parseInt("10", 37)`);
E(`parseInt("10", 1)`);
E(`parseInt("10", 0)`);
E(`parseInt("0x10", 16)`);
E(`parseInt("0x10", 10)`);
E(`parseInt("  -0")`);
E(`1/parseInt("-0")`);
E(`parseInt("123abc")`);
E(`parseInt("1e3")`);
E(`parseInt(1e21)`);
E(`parseInt(0.0000005)`);
E(`parseInt(null, 36)`);
E(`parseInt("9007199254740993")`);
E(`parseFloat("1e1000")`);
E(`parseFloat(".5.5")`);
E(`parseFloat("-.5e-2x")`);
E(`parseFloat("Infinityx")`);
E(`parseFloat("0x10")`);
E(`Number.parseFloat === parseFloat && Number.parseInt === parseInt`);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "string-edge-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro, sem o transpilador do bun.
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
const seen = new Set();
let kept = 0;
let dropped = 0;
// A matriz tem milhares de programas; o gerador amostra por hash (sampleByHash) 1 em cada STRIDE (padrão 7, cerca de 500). `STRIDE=1` gera tudo.
const STRIDE = Number(process.env.STRIDE || 7);
const allPrograms = [...new Set(programs)];
for (const source of sampleByHash(allPrograms, Math.ceil(allPrograms.length / STRIDE))) {
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
