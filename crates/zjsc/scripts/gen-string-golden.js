// Gera tests/golden/string_bun.tsv: String.prototype, Array.prototype.join, String.raw, template literals,
// normalize, localeCompare (en, pt, sv, de), toLocaleUpperCase('tr'), at, padStart, split com regex e limite,
// replace com padrões $ e funções, matchAll, isWellFormed/toWellFormed, codePointAt e surrogates, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). O programa traz o próprio serializador
// `S`, que mantém o tipo (`s:`, `n:`, `a[...]`), e grava `R = S(expressão)` ou `throw Nome: mensagem`.
// Resultado com caminho da máquina é descartado. Cada programa roda num bun próprio, com timeout.
// A coluna de fonte do TSV guarda o programa canônico (prelúdio fatorado em tests/golden/string.preludes.json) e a quinta
// coluna o modo e o mapa de posições (ver golden-prelude.js).
// Uso: timeout 1800 bun scripts/gen-string-golden.js > tests/golden/string_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = expr => programs.push(expr);
const q = text => JSON.stringify(text);

// ---- Bases com surrogates, acentos e casos especiais de caixa.
const bases = [
  "''", "'a'", "'abc'", "'  ab  '", "'aXbXc'", "'ça va'", "'ß'", "'\\ud83d\\ude00'", "'a\\ud83d'", "'\\ude00a'",
  "'\\ud83d\\ude00\\ud83d'", "'i̇'", "'İI'", "'ıi'", "'\\u00e9'", "'e\\u0301'", "'\\u212b'", "'\\ufb01'", "'Straße'",
];

// ---- at, charAt, charCodeAt, codePointAt: índices de borda.
const indexes = ["0", "1", "2", "-1", "-2", "-3", "9", "NaN", "Infinity", "-Infinity", "1.9", "-1.9", "'1'", "null", "undefined", "true", "{}"];
for (const b of bases) {
  for (const i of indexes) {
    add(`${b}.at(${i})`);
    add(`${b}.codePointAt(${i})`);
  }
  add(`${b}.charAt(1)`);
  add(`${b}.charCodeAt(1)`);
  add(`${b}.length`);
}

// ---- isWellFormed / toWellFormed.
const wellFormedSources = [
  "''", "'abc'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'\\ude00\\ud83d'", "'a\\ud83db'", "'\\ud83d\\ud83d\\ude00'",
  "'\\ud83d\\ude00\\ude00'", "'\\ud800\\udc00'", "'\\udbff\\udfff'", "'\\ud7ff\\ue000'", "'\\ud83d\\u0041'",
];
for (const w of wellFormedSources) {
  add(`${w}.isWellFormed()`);
  add(`${w}.toWellFormed()`);
  add(`${w}.toWellFormed().length`);
  add(`[...${w}.toWellFormed()].map(c => c.codePointAt(0))`);
  add(`${w}.toWellFormed().isWellFormed()`);
  add(`${w}.split('').map(c => c.charCodeAt(0))`);
  add(`[...${w}].length`);
  add(`Array.from(${w}).map(c => c.length)`);
  add(`${w}.normalize().isWellFormed()`);
  add(`encodeURIComponent(${w}.toWellFormed())`);
}
add("String.prototype.isWellFormed.call(null)");
add("String.prototype.toWellFormed.call(undefined)");
add("String.prototype.isWellFormed.call(12)");
add("String.prototype.toWellFormed.call({ toString() { return '\\ud83d' } }).charCodeAt(0)");
add("String.prototype.toWellFormed.call(Symbol())");

// ---- padStart, padEnd.
const padArgs = [
  "0", "1", "3", "5", "8", "NaN", "-1", "'4'", "undefined", "3.9", "Infinity", "2**31",
];
const fills = ["", ",' '", ",''", ",'xy'", ",'\\ud83d\\ude00'", ",'\\ud83d'", ",undefined", ",null", ",12", ",'0'"];
for (const b of ["'ab'", "''", "'\\ud83d\\ude00'", "'abcdef'", "'é'"]) {
  for (const n of padArgs.slice(0, 11)) {
    for (const f of fills.slice(0, 6)) {
      add(`${b}.padStart(${n}${f})`);
      add(`${b}.padEnd(${n}${f})`);
    }
  }
}
for (const f of fills) {
  add(`'7'.padStart(4${f})`);
  add(`'7'.padEnd(4${f})`);
}
add("'ab'.padStart(2**30,'x').length");
add("'ab'.padEnd(2**30+1,'xy').length");
add("'a'.padStart(2**31)");

// ---- repeat.
for (const n of ["0", "1", "3", "NaN", "-1", "Infinity", "2.5", "'2'", "undefined", "null", "2**31", "2**30"]) add(`'ab'.repeat(${n})`);
add("''.repeat(2**31)");
add("''.repeat(2**30).length");

// ---- split com string, regex e limite.
const splitSubjects = ["''", "'abc'", "'a,b,,c'", "'aXbXc'", "'a1b22c333'", "'\\ud83d\\ude00x\\ud83d\\ude00'", "'  a  b  '", "'a\\nb\\r\\nc'", "'abc'.repeat(3)"];
const splitSeps = [
  "undefined", "''", "','", "'X'", "/,/", "/X/g", "/(,)/", "/(X)(?:)/", "/\\d+/", "/(\\d)(\\d)?/", "//", "/(?:)/u", "/b*/", "/^/", "/$/", "/^|$/m",
  "/\\s+/", "/(?<k>X)/", "/\\ud83d/", "/\\ud83d\\ude00/u", "/./u", "/./s", "/a|/", "/(a)|(b)/", "/,/y", "/,/gi",
];
const splitLimits = ["", ",0", ",1", ",2", ",3", ",-1", ",2**32", ",2**32+1", ",NaN", ",'2'", ",undefined", ",4294967295", ",4294967296", ",1.9", ",Infinity"];
for (const subject of splitSubjects) {
  for (const sep of splitSeps) {
    add(`${subject}.split(${sep})`);
    for (const limit of splitLimits.slice(1, 6)) add(`${subject}.split(${sep}${limit})`);
  }
}
for (const sep of splitSeps.slice(0, 8)) for (const limit of splitLimits) add(`'a,b,,c'.split(${sep.includes("/") ? sep : "','"}${limit})`);
add("'abc'.split({ [Symbol.split](s, l) { return [s, l] } }, 3)");
add("'abc'.split(null)");
add("'anullb'.split(null)");
add("'a1b'.split(1)");
add("'abc'.split('', 0)");
add("'abc'.split(undefined, 0)");

// ---- replace e replaceAll com padrões $ e funções.
const replaceSubjects = ["'abcabc'", "'aaa'", "'xyz'", "''", "'a-b-c'", "'John Smith'", "'\\ud83d\\ude00a\\ud83d\\ude00'"];
const patterns = ["'b'", "'a'", "''", "/b/", "/b/g", "/(a)(b)?/", "/(a)(b)?/g", "/(?<x>a)/", "/(?<x>a)/g", "/z/", "/(?:)/g", "/a*/g", "/./gu", "/$/", "/^/g", "/(\\w+) (\\w+)/"];
const dollarReplacements = [
  "'$&'", "'$$'", "'$`'", "'$\\''", "'$1'", "'$2'", "'$01'", "'$10'", "'$00'", "'$0'", "'$<x>'", "'$<y>'", "'$<'", "'$'", "'$$$'", "'[$&$&]'", "'$1$2'",
  "'$99'", "'$1x'", "'x$'", "'$_'", "'$<x'", "'$-'", "'$&$`$\\''", "'$2 $1'", "''", "'\\ud83d'", "'$<x>$<x>'",
];
for (const subject of replaceSubjects) {
  for (const pattern of patterns) {
    for (const rep of dollarReplacements.slice(0, 12)) {
      add(`${subject}.replace(${pattern}, ${rep})`);
    }
  }
}
for (const rep of dollarReplacements) {
  add(`'abcabc'.replace('b', ${rep})`);
  add(`'abcabc'.replaceAll('b', ${rep})`);
  add(`'John Smith'.replace(/(\\w+) (\\w+)/, ${rep})`);
  add(`'abab'.replace(/(?<x>a)(b)/g, ${rep})`);
  add(`'abab'.replaceAll(/(a)(b)/g, ${rep})`);
}
const replacers = [
  "(m) => m + m", "(m, i) => i", "(m, p1) => p1", "(m, p1, p2) => String(p2)", "(...a) => a.length", "(...a) => JSON.stringify(a)",
  "() => undefined", "() => null", "() => {}", "() => 1", "() => '$&'", "() => '$1'", "(m, i, s) => s", "function () { return this === undefined ? 'u' : typeof this }",
  "(m) => m.toUpperCase()", "() => { throw new RangeError('boom') }", "(...a) => typeof a[a.length - 1]",
];
for (const rep of replacers) {
  for (const pattern of ["'b'", "/b/", "/b/g", "/(a)(b)?/g", "/(?<x>a)/g", "''", "/(?:)/g", "/z/"]) {
    add(`'abcabc'.replace(${pattern}, ${rep})`);
    if (pattern !== "/b/") add(`'abcabc'.replaceAll(${pattern.startsWith("/") && !pattern.endsWith("g") ? "/b/g" : pattern}, ${rep})`);
  }
}
add("'aaa'.replaceAll('a', (m, i) => i)");
add("'aaa'.replaceAll('', '-')");
add("'abc'.replaceAll('', (m, i) => '[' + i + ']')");
add("'abc'.replaceAll(/b/, 'x')");
add("'abc'.replaceAll(/b/g, 'x')");
add("'abc'.replace(/b/g, () => 'x$&')");
add("'abc'.replace('b', 7)");
add("'abc'.replace('b', {})");
add("'abc'.replace('b', null)");
add("'abc'.replace('b')");
add("'abc'.replace()");
add("'a\\ud83db'.replace(/\\ud83d/g, '!')");
add("'\\ud83d\\ude00'.replace(/\\ud83d/u, '!')");
add("'\\ud83d\\ude00'.replace(/(?:)/gu, '-')");
add("'\\ud83d\\ude00'.replace(/(?:)/g, '-').length");

// ---- matchAll e match.
const matchSubjects = ["'a1b2c3'", "'aaa'", "''", "'abc'", "'\\ud83d\\ude00a'", "'xAyAz'"];
const matchPatterns = ["/\\d/g", "/(\\d)/g", "/(?<n>\\d)/g", "/a/g", "/(?:)/g", "/./gu", "/z/g", "/[A-Z]/gi", "/(a)|(b)/g", "/a*?/g", "/\\b/g", "'a'", "'1'", "''", "/./g", "/./gs", "/^/gm"];
for (const subject of matchSubjects) {
  for (const pattern of matchPatterns) {
    add(`[...${subject}.matchAll(${pattern})].map(m => [m[0], m.index, m.length, m.groups && JSON.stringify(m.groups)])`);
    add(`[...${subject}.matchAll(${pattern})].length`);
    add(`${subject}.match(${pattern})`);
  }
}
add("'a1'.matchAll(/\\d/)");
add("'a1'.matchAll(/\\d/).toString()");
add("Object.prototype.toString.call('a'.matchAll(/a/g))");
add("typeof 'a'.matchAll(/a/g).next");
add("'a1'.matchAll(/\\d/g).next().done");
add("'ab'.matchAll(/./g).next().value[0]");
add("(() => { const it = 'aa'.matchAll(/a/g); it.next(); it.next(); return it.next().done })()");
add("(() => { const re = /a/g; re.lastIndex = 1; return [...'aaa'.matchAll(re)].length })()");
add("(() => { const re = /a/g; re.lastIndex = 1; 'aaa'.matchAll(re); return re.lastIndex })()");
add("'abc'.matchAll(undefined).next().value[0]");
add("'abc'.matchAll(null).next().done");
add("'a'.matchAll({ [Symbol.matchAll]: () => 'custom' })");

// ---- normalize.
const normStrings = [
  "'\\u00c5'", "'A\\u030a'", "'\\u212b'", "'\\u1e9b\\u0323'", "'\\ufb01'", "'\\u2460'", "'\\uff21'", "'\\u00e9'", "'e\\u0301'", "'\\u1100\\u1161'", "'\\uac00'",
  "'\\u0344'", "'\\u0958'", "'\\ud834\\udd5e'", "'\\u2126'", "'\\u00b5'", "'\\u2163'", "'\\u3392'", "'\\u0041\\u0323\\u030a'", "'\\u0041\\u030a\\u0323'",
  "'\\ud83d'", "'a\\ud83d'", "''", "'abc'", "'\\u1e0b\\u0323'", "'\\u1e0d\\u0307'", "'\\ufdfa'", "'\\u00bd'", "'\\u2126\\u212a'", "'\\u0f73'", "'\\u0f71\\u0f72'",
];
for (const n of normStrings) {
  for (const form of ["", "'NFC'", "'NFD'", "'NFKC'", "'NFKD'"]) {
    add(`${n}.normalize(${form})`);
    add(`${n}.normalize(${form}).length`);
  }
}
for (const bad of ["'nfc'", "'NFX'", "''", "null", "1", "{}", "undefined", "'NFC '", "[]", "'NFKD'"]) add(`'a'.normalize(${bad})`);
add("[...'\\u1e9b\\u0323'.normalize('NFKD')].map(c => c.codePointAt(0).toString(16))");
add("[...'\\u00c5'.normalize('NFD')].map(c => c.codePointAt(0).toString(16))");
add("'\\u1e9b\\u0323'.normalize('NFC') === '\\u1e9b\\u0323'");
add("'\\u0041\\u030a' === '\\u00c5'");
add("'\\u0041\\u030a'.normalize() === '\\u00c5'");

// ---- localeCompare (en, pt, sv, de) e opções.
const words = ["a", "A", "b", "z", "ä", "å", "ö", "ç", "c", "é", "e", "resume", "résumé", "Résumé", "ñ", "n", "o", "ß", "ss", "ae", "æ", "a\\u0301", "o\\u0308", "I", "i", "İ", "ı", "10", "2", "-", "_", "", " ", "ab", "a b", "á", "à", "ã", "õ", "Z", "zz", "Å", "Ä", "Ö"];
const locales = ["undefined", "'en'", "'pt'", "'pt-BR'", "'sv'", "'de'", "'de-u-co-phonebk'", "'sv-SE'", "'en-US'"];
const localePairs = [
  ["a", "b"], ["b", "a"], ["a", "A"], ["A", "a"], ["a", "ä"], ["ä", "z"], ["z", "ä"], ["z", "å"], ["å", "ö"], ["ö", "z"], ["a", "å"], ["ä", "ö"], ["o", "ö"], ["ö", "o"],
  ["c", "ç"], ["ç", "d"], ["resume", "résumé"], ["résumé", "Résumé"], ["e", "é"], ["n", "ñ"], ["ñ", "o"], ["ß", "ss"], ["ae", "æ"], ["ä", "ae"], ["a", "a\\u0301"], ["á", "a\\u0301"],
  ["ã", "a"], ["õ", "o"], ["I", "i"], ["İ", "i"], ["ı", "i"], ["10", "2"], ["-", "_"], ["", "a"], ["a", ""], ["", ""], [" ", "a"], ["ab", "a b"], ["zz", "z"], ["Z", "a"], ["Å", "Ä"], ["Ä", "Ö"], ["Ö", "Z"],
];
for (const loc of locales) {
  for (const [x, y] of localePairs) add(`'${x}'.localeCompare('${y}', ${loc})`);
}
for (const [x, y] of localePairs.slice(0, 22)) {
  for (const opt of ["{ sensitivity: 'base' }", "{ sensitivity: 'accent' }", "{ sensitivity: 'case' }", "{ numeric: true }", "{ caseFirst: 'upper' }", "{ ignorePunctuation: true }"]) {
    add(`'${x}'.localeCompare('${y}', 'en', ${opt})`);
  }
}
for (const loc of ["'sv'", "'de'", "'pt'", "'en'"]) {
  add(`['z','å','ä','ö','a','o','Z','Å','Ä','Ö'].sort((x, y) => x.localeCompare(y, ${loc})).join('')`);
  add(`['résumé','resume','Resume','résume','resumé'].sort((x, y) => x.localeCompare(y, ${loc})).join()`);
  add(`['a10','a2','a1','A1'].sort((x, y) => x.localeCompare(y, ${loc}, { numeric: true })).join()`);
  add(`['ñ','n','o','nz','ña'].sort((x, y) => x.localeCompare(y, ${loc})).join()`);
  add(`['ae','æ','ad','af','äe'].sort((x, y) => x.localeCompare(y, ${loc})).join()`);
  add(`['ç','c','d','cz','ça'].sort((x, y) => x.localeCompare(y, ${loc})).join()`);
}
add("'a'.localeCompare()");
add("'undefined'.localeCompare()");
add("'a'.localeCompare('b', 'xx-invalid-')");
add("'a'.localeCompare('b', ['sv', 'en'])");
add("'a'.localeCompare('b', null)");
add("'a'.localeCompare('b', 'en', { sensitivity: 'x' })");
add("'a'.localeCompare('b', 'en', null)");
add("String.prototype.localeCompare.call(null, 'a')");
add("'\\u00e9'.localeCompare('e\\u0301')");
add("'a\\ud83d'.localeCompare('a')");

// ---- toLocaleUpperCase / toLocaleLowerCase com tr e outros.
const caseSubjects = ["'i'", "'I'", "'ı'", "'İ'", "'iI'", "'ıİ'", "'i\\u0307'", "'I\\u0307'", "'ß'", "'ǆ'", "'ǅ'", "'ﬁ'", "'ΑΣ'", "'ΑΣ Α'", "'ὈΔΥΣΣΕΎΣ'", "'abc'", "'ÀÉÎ'", "'àéî'", "'\\ud801\\udc28'", "'\\ud83d'", "'Σ'", "'ς'", "'ŉ'", "'ǰ'"];
const caseLocales = ["", "'tr'", "'tr-TR'", "'az'", "'lt'", "'en'", "'de'", "'sv'", "'pt'", "'el'", "'nl'", "['tr']", "'xx'", "undefined"];
for (const sub of caseSubjects) {
  for (const loc of caseLocales) {
    add(`${sub}.toLocaleUpperCase(${loc})`);
    add(`${sub}.toLocaleLowerCase(${loc})`);
  }
  add(`${sub}.toUpperCase()`);
  add(`${sub}.toLowerCase()`);
  add(`${sub}.toUpperCase().length`);
}
add("'i'.toLocaleUpperCase('i-invalid-')");
add("'i'.toLocaleUpperCase(null)");
add("'i'.toLocaleUpperCase(1)");
add("'i'.toLocaleUpperCase([])");
add("'i'.toLocaleUpperCase(['tr', 'en'])");
add("'I'.toLocaleLowerCase('tr').length");
add("'\\u0130'.toLocaleLowerCase('tr').length");
add("'\\u0130'.toLowerCase().length");
add("'\\u0130'.toLocaleLowerCase('en').length");

// ---- Array.prototype.join.
const joinArrays = [
  "[]", "[1]", "['', 'a']", "['a', '']", "['', '']", "[,]", "[, 1]", "[1, ,]", "[1, , 2]", "[null]", "[undefined]", "[null, undefined]", "[1, null, 2, undefined, 3]",
  "[[]]", "[[], []]", "[[1, 2], [3]]", "[[1, [2, [3]]]]", "[{}]", "[{ toString() { return 'x' } }]", "[true, false]", "[1.5, -0, NaN, 1e21, 1e-7]", "[1n, 2n]",
  "['\\ud83d', '\\ude00']", "['a\\ud83d', '\\ude00b']", "new Array(3)", "new Array(5).fill('')", "Array(2**20).join('x').length", "Array(1000).join('ab').length",
  "[...'abc']", "'abc'.split('')", "[0, '', null, undefined, false]", "[Symbol.iterator.description]", "[new Date(0).getTime()]",
];
const joinSeps = ["", "','", "''", "' - '", "undefined", "null", "0", "1", "{}", "[]", "['-']", "{ toString() { return '|' } }", "'\\ud83d'", "'\\u20ac'", "true", "-0", "1.5", "2n", "'x'.repeat(3)"];
for (const arr of joinArrays) {
  for (const sep of joinSeps) add(`${arr}.join(${sep})`);
}
add("[Symbol()].join()");
add("[1, Symbol()].join()");
add("[1].join(Symbol())");
add("[].join(Symbol())");
add("[1, 2].join({ toString() { throw new RangeError('sep') } })");
add("[{ toString() { throw new TypeError('el') } }].join()");
add("[{ toString() { return 1 } }].join()");
add("[{ toString: null, valueOf() { return 'v' } }].join()");
add("Array.prototype.join.call({ length: 3, 0: 'a', 2: 'c' }, '-')");
add("Array.prototype.join.call({ length: 2, 0: 'a', 1: 'b' })");
add("Array.prototype.join.call('abc', '+')");
add("Array.prototype.join.call({ length: -1 })");
add("Array.prototype.join.call({ length: '2', 0: 1, 1: 2 })");
add("Array.prototype.join.call({})");
add("Array.prototype.join.call(null)");
add("Array.prototype.join.call(undefined)");
add("Array.prototype.join.call(5, ',')");
add("(() => { const a = [1, 2]; a.push(a); return a.join() })()");
add("(() => { const a = ['x']; a.push([a]); return a.join('-') })()");
add("(() => { const a = [1, 2]; a.push(a); return a.toString() })()");
add("(() => { const a = []; a[0] = a; return a.join() })()");
add("(() => { const a = [1, 2, 3]; return a.join({ toString() { a.length = 1; return '-' } }) })()");
add("(() => { const a = [1, 2, 3]; return a.join({ toString() { a.push(4); return '-' } }) })()");
add("(() => { const a = [{ toString() { a.length = 0; return 'x' } }, 2, 3]; return a.join() })()");
add("[1, [2, 3]].toString()");
add("[1, 2].toString === Array.prototype.join");
add("Array.prototype.toString.call({ join() { return 'J' } })");
add("Array.prototype.toString.call({})");
add("Array.prototype.toString.call({ join: 1 })");
add("Array.prototype.toLocaleString.call([1234.5, new Date(0)]).length > 0");
add("[1234.5, 'a', null, undefined].toLocaleString('de')");
add("[1234.5, 0.5].toLocaleString('pt-BR')");
add("[1234.5].toLocaleString('sv')");

// ---- String.raw e template literals.
const rawCases = [
  "String.raw`a\\n${1}b`", "String.raw`\\u{41}`", "String.raw`\\xzz`", "String.raw`\\u00`", "String.raw``", "String.raw`${1}`", "String.raw`${1}${2}`", "String.raw`a${1}b${2}c`",
  "String.raw`\\``", "String.raw`\\${x}`", "String.raw`$\\{`", "String.raw`\\\\`", "String.raw`\\\r\n`", "String.raw`line1\nline2`", "String.raw`\r\n`", "String.raw`\r`",
  "String.raw({ raw: ['a', 'b', 'c'] }, 1, 2)", "String.raw({ raw: ['a', 'b', 'c'] }, 1)", "String.raw({ raw: ['a', 'b', 'c'] })", "String.raw({ raw: ['a'] }, 1, 2)", "String.raw({ raw: [] }, 1)",
  "String.raw({ raw: 'abc' }, 1, 2)", "String.raw({ raw: { length: 2, 0: 'x', 1: 'y' } }, '-')", "String.raw({ raw: { length: 3 } }, 1, 2)", "String.raw({ raw: { length: -1 } })", "String.raw({ raw: { length: '2', 0: 'a', 1: 'b' } }, 'M')",
  "String.raw({ raw: [1, 2, 3] }, 'x', 'y')", "String.raw({ raw: [null, undefined] }, 0)", "String.raw({ raw: ['\\ud83d', '\\ude00'] }, '')", "String.raw({ raw: ['a', 'b'] }, { toString() { return 'obj' } })",
  "String.raw({ raw: ['a', 'b'] }, Symbol())", "String.raw({ raw: ['a', 'b'] }, 1n)", "String.raw({})", "String.raw()", "String.raw(null)", "String.raw(1)", "String.raw('abc')", "String.raw({ raw: null })", "String.raw({ raw: undefined })", "String.raw({ raw: 1 })",
  "String.raw.length", "String.raw.name", "String.raw`a${1}`.length",
  "(t => t.raw[0])`a\\nb`", "(t => t[0])`a\\nb`", "(t => t[0])`\\unicode`", "(t => t.raw[0])`\\unicode`", "(t => t.length)`a${1}b${2}c`", "(t => Object.isFrozen(t) && Object.isFrozen(t.raw))`x`",
  "(t => t.raw.length)`${0}${1}`", "(t => JSON.stringify(t))`\\x`", "(t => JSON.stringify(t.raw))`\\x`", "(t => Array.isArray(t.raw))`x`", "(t => Object.getOwnPropertyNames(t).join())`x`",
  "(t => Object.getOwnPropertyDescriptor(t, 'raw').enumerable)`x`", "(t => t)`a` === (t => t)`a`", "(() => { const f = t => t; const g = () => f`a`; return g() === g() })()",
  "(() => { const f = t => t; return f`a` === f`a` })()", "(() => { const f = t => t; const r = []; for (let i = 0; i < 2; i++) r.push(f`z`); return r[0] === r[1] })()",
  "(t, ...v) => v".length, "((t, ...v) => v.join('+'))`a${1}b${2}`", "((t, ...v) => t.join('|'))`a${1}b${2}c`", "((t, ...v) => t.raw.join('|'))`a\\n${1}b\\t`",
];
for (const r of rawCases) add(r);
const tplValues = ["1", "1.5", "-0", "NaN", "null", "undefined", "true", "'s'", "[1, 2]", "[]", "{}", "1n", "{ toString() { return 'T' } }", "{ valueOf() { return 7 }, toString() { return 'S' } }", "{ [Symbol.toPrimitive](h) { return h } }", "new Date(NaN)", "() => 1", "'\\ud83d'", "[null]", "[undefined, 1]"];
for (const v of tplValues) {
  add(`\`[\${${v}}]\``);
  add(`\`\${${v}}\${${v}}\``);
  add(`'' + ${v}`);
  add(`String(${v})`);
  add(`\`\${${v}}\`.length`);
}
const tplMisc = [
  "`a${1}b${2}c`", "`\\u{1F600}`.length", "`\\ud83d\\ude00`.codePointAt(0)", "`\n`.length", "`\r\n`.length", "`\r`.length", "`\\\r\n`.length", "`a\\\nb`", "`\\x41\\u0042\\103`".length,
  "`\\0`.length", "`\\0`.charCodeAt(0)", "`${`${`x`}`}`", "`${1 + 1}${'a'.repeat(2)}`", "`${Symbol.iterator.toString()}`", "`${[1, [2, [3]]]}`", "`\\${}`", "`$`", "`$$`", "`${'$'}{`", "`\\``",
  "`a${1}`.concat`b`", "typeof `x`", "`${{ toString: () => 'ts', valueOf: () => 'vo' }}`", "`${{ valueOf: () => 'vo' }}`", "`${{ [Symbol.toPrimitive]: () => 'tp' }}`",
  "(() => { try { return `${Symbol()}` } catch (e) { return e.name + ': ' + e.message } })()", "(() => { try { return `${{ toString: null, valueOf: null }}` } catch (e) { return e.name + ': ' + e.message } })()",
  "(() => { const o = []; const t = { toString() { o.push('t'); return '' } }; const u = { toString() { o.push('u'); return '' } }; `${t}${u}`; return o.join() })()",
  "(() => { let i = 0; return `${i++}${i++}${i++}` })()", "(() => { const a = 'x'; return `${a}${a}` === 'xx' })()",
  "String(`a`) === 'a'", "`${1}${2}` === '12'", "`a` + `b`", "`line1\nline2`.split('\\n').length",
  "((s) => s.raw[0].length)`\\u{1F600}`", "((s) => s[0].length)`\\u{1F600}`", "((s) => s[0])`\\u{110000}`", "((s) => s[0] === undefined)`\\u{110000}`", "((s) => s.raw[0])`\\u{110000}`",
  "((s) => s[0])`\\07`", "((s) => s.raw[0])`\\07`", "((s) => s[0])`\\08`", "((s) => s[0] === undefined)`\\1`", "((s) => s[0] === undefined)`\\xg`",
];
for (const t of tplMisc) add(t);

// ---- Métodos diversos de String.prototype (bordas de argumento).
const sub = "'abcdef'";
for (const a of ["0", "1", "-1", "3", "10", "NaN", "Infinity", "-Infinity", "undefined", "'2'", "null", "1.5"]) {
  for (const b of ["", ",0", ",2", ",-1", ",10", ",NaN", ",undefined", ",Infinity", ",1"]) {
    add(`${sub}.slice(${a}${b})`);
    add(`${sub}.substring(${a}${b})`);
    add(`${sub}.substr(${a}${b})`);
  }
  add(`${sub}.indexOf('c', ${a})`);
  add(`${sub}.lastIndexOf('c', ${a})`);
  add(`${sub}.includes('c', ${a})`);
  add(`${sub}.startsWith('c', ${a})`);
  add(`${sub}.endsWith('c', ${a})`);
  add(`${sub}.search(/c/)`);
}
for (const t of ["''", "'a'", "'abc'", "'  a b  '", "'\\u00a0a\\u2003'", "'\\ufeffa\\ufeff'", "'\\u180ea'", "'\\u200ba'", "'\\u2028a\\u2029'", "'\\ud83d\\ude00 '", "'\\t\\n\\v\\f\\r a'", "'\\u0085a'"]) {
  add(`${t}.trim()`);
  add(`${t}.trimStart()`);
  add(`${t}.trimEnd()`);
  add(`${t}.trim().length`);
}
for (const a of ["'a'", "''", "'ab'", "undefined", "null", "1", "/a/", "{}"]) {
  add(`'abab'.indexOf(${a})`);
  add(`'abab'.lastIndexOf(${a})`);
  add(`'abab'.concat(${a})`);
  add(`'abab'.includes(${a})`);
  add(`'abab'.startsWith(${a})`);
  add(`'abab'.endsWith(${a})`);
  add(`'abab'.search(${a})`);
  add(`'abab'.localeCompare(${a})`);
}
for (const m of ["trim", "toString", "valueOf", "toUpperCase", "at", "isWellFormed", "padStart", "split", "replace", "normalize", "localeCompare", "codePointAt", "repeat", "slice", "concat", "includes"]) {
  add(`String.prototype.${m}.call(null)`);
  add(`String.prototype.${m}.call(undefined)`);
  add(`String.prototype.${m}.call(1)`);
  add(`String.prototype.${m}.call(true)`);
  add(`String.prototype.${m}.call([1, 2])`);
  add(`String.prototype.${m}.call(Symbol())`);
  add(`String.prototype.${m}.length`);
  add(`String.prototype.${m}.name`);
}
add("String.fromCodePoint(0x1F600).length");
add("String.fromCodePoint(0xD83D, 0xDE00) === '\\ud83d\\ude00'");
add("String.fromCodePoint(0x110000)");
add("String.fromCodePoint(-1)");
add("String.fromCodePoint(1.5)");
add("String.fromCodePoint('x')");
add("String.fromCodePoint()");
add("String.fromCharCode(0x10000 + 65).charCodeAt(0)");
add("String.fromCharCode(65, 66.9, '67', NaN)");
add("String.fromCharCode(-1).charCodeAt(0)");
add("String.fromCharCode(0xD83D, 0xDE00).codePointAt(0)");
add("'abc'.concat(1, null, undefined, [2, 3], {})");
add("'a'.concat('b').concat`c`");
add("new String('ab').at(-1)");
add("Object('ab').padStart(4, '-')");
add("Object.keys(new String('ab')).join()");
add("Object.getOwnPropertyNames('ab').join()");
add("('ab')[1] + 'ab'[5]");
add("'ab'[-1]");
add("'ab'['1']");
add("'ab'.hasOwnProperty('1')");
add("1 in Object('ab')");
add("typeof String.prototype[Symbol.iterator]");
add("[...'a\\ud83d\\ude00b'].length");
add("Array.from('\\ud83d\\ude00').length");
add("'a\\ud83d\\ude00b'[Symbol.iterator]().next().value.length");
add("Object.prototype.toString.call('a'[Symbol.iterator]())");
add("'abc'.localeCompare('abd') < 0");
add("'a' < 'b' && 'a\\ud83d' < 'a\\ude00'");
add("'\\ud83d\\ude00' < '\\uffff'");
add("['\\uffff', '\\ud83d\\ude00'].sort().map(s => s.length).join()");
add("['b', 'a', 'B', 'A', 'é', 'e', 'z'].sort().join('')");
add("['b', 'a', 'B', 'A', 'é', 'e', 'z'].sort((x, y) => x.localeCompare(y)).join('')");
add("['b', 'a', 'B', 'A', 'é', 'e', 'z'].sort((x, y) => x.localeCompare(y, 'sv')).join('')");
add("'abc'.anchor('x\"y')");
add("'abc'.link('u')");
add("'abc'.big()");
add("'abc'.substr(-2, 1)");
add("escape('\\u00e9\\ud83d\\ude00 ')");
add("unescape('%E9%u0041%zz')");
add("'a-b_c d'.split(/[-_ ]/, 2)");
add("'test'.search(/s/)");
add("'x'.toString === String.prototype.toString");
add("String(Symbol('d'))");
add("String(null) + String(undefined) + String(1n)");
add("new String('x') == 'x' && new String('x') !== 'x'");
add("typeof new String('x')");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "string-golden-"));
const file = path.join(dir, "string_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";

// O serializador fica em tests/golden/string_bun_prelude.js, que o teste Rust também inclui: a mesma fonte dos dois lados.
// A coluna de fonte do TSV guarda só a expressão; o programa é prelúdio + `try { R = S(expr) } catch ...`.
const serializer = fs.readFileSync(path.join(__dirname, "..", "tests", "golden", "string_bun_prelude.js"), "utf8");

const seen = new Set();
let kept = 0;
let dropped = 0;
const rows = [];
for (const expr of programs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  const original =
    serializer +
    "try { globalThis.R = S(" + expr + ") } catch (e) { globalThis.R = 'throw ' + (e && e.name) + ': ' + (e && e.message) }\n";
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executable`.
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, maxBuffer: 64 * 1024 * 1024 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(expr) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(expr) + "\n");
    continue;
  }
  kept++;
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("string", rows));
fs.rmSync(dir, { recursive: true, force: true });
