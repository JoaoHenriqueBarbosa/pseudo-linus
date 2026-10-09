// Gera tests/golden/string_unicode_more_bun.tsv: complemento de gen-string-unicode-golden.js, medido no bun 1.4.2.
// Matriz de String Unicode de borda: normalize (NFC/NFD/NFKC/NFKD, formas inválidas, Hangul, ordem canônica),
// toUpperCase/toLowerCase/toLocale* (ß, İ, sigma final, ligaduras, locales), localeCompare, isWellFormed/toWellFormed,
// at/codePointAt/charAt com surrogates, padStart/padEnd com preenchimentos longos, replaceAll com padrões `$`,
// split com regex Unicode, String.raw, matchAll e as flags v/u/d/y. Programas que já estão em string_unicode_bun.tsv ou
// em string_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-string-unicode-golden.js.
// Uso: bun scripts/gen-string-unicode-more-golden.js > tests/golden/string_unicode_more_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const HELPER =
  "function F(f){try{return D(f())}catch(e){return e instanceof Error?e.name+': '+e.message:'throw '+String(e)}}" +
  "function D(v){if(typeof v==='string')return 's'+JSON.stringify(v);if(typeof v==='symbol')return 'sym';" +
  "if(typeof v==='number')return Object.is(v,-0)?'-0':'n'+v;if(Array.isArray(v))return '['+v.map(D).join(',')+']';" +
  "if(v&&typeof v==='object')return 'o'+JSON.stringify(Object.keys(v))+Object.prototype.toString.call(v);" +
  "return typeof v+':'+String(v)}\n";

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. normalize: formas válidas e inválidas sobre cadeias de borda.
const normSamples = [
  "'\\u00c5'", "'\\u212b'", "'A\\u030a'", "'\\u1e9b\\u0323'", "'\\u1e9b'", "'\\u0323\\u0307'", "'\\u0307\\u0323'", "'q\\u0307\\u0323'", "'q\\u0323\\u0307'",
  "'\\ufb01'", "'\\ufb03'", "'\\u2126'", "'\\u03a9'", "'\\u00bd'", "'\\u2460'", "'\\u2163'", "'\\uff21'", "'\\u3392'", "'\\u00a0'", "'\\u2002'",
  "'\\uac00'", "'\\u1100\\u1161'", "'\\u1100\\u1161\\u11a8'", "'\\uac01'", "'\\ud55c\\uae00'", "'\\u1112\\u1161\\u11ab'", "'\\u1100'", "'\\u11a8'",
  "'\\u0958'", "'\\u0915\\u093c'", "'\\u2126\\u0301'", "'\\u0344'", "'\\u0308\\u0301'", "'\\u0301\\u0308'", "'\\u1e0b\\u0323'", "'\\u1e0d\\u0307'",
  "'\\u0d4a'", "'\\u0b4b'", "'\\u0f73'", "'\\u0f75'", "'\\u2000'", "'\\ud835\\udd0a'", "'\\ud835\\udc00'", "'\\ud83d\\ude00'", "'\\ud83d'", "'\\ude00'",
  "'a\\ud83d'", "'\\ude00b'", "'\\ud834\\udd5e'", "'\\ud834\\udd57\\ud834\\udd65'", "'\\ufdfa'", "'\\u0132'", "'\\u017f'", "'\\u00b5'", "'\\u2103'",
  "'\\u1f71'", "'\\u1fbb'", "'\\u0385'", "'\\u1fee'", "'\\u00e9'", "'e\\u0301'", "'\\u0065\\u0301\\u0323'", "'\\u0065\\u0323\\u0301'", "''",
];
const forms = ["'NFC'", "'NFD'", "'NFKC'", "'NFKD'", "undefined", "'nfc'", "'NFX'", "''", "null", "'NFC '", "1"];
for (const s of normSamples) for (const f of forms) add(`${s}.normalize(${f})`);
for (const s of normSamples) add(`${s}.normalize()`, `${s}.normalize().length`, `${s}.normalize('NFD').length`, `${s}.normalize('NFKD').length`);
for (const s of normSamples.slice(0, 40)) {
  add(`${s}.normalize('NFD').normalize('NFC') === ${s}.normalize('NFC')`);
  add(`${s}.normalize('NFKD').normalize('NFKC') === ${s}.normalize('NFKC')`);
  add(`Array.from(${s}.normalize('NFKD'), c => c.codePointAt(0).toString(16))`);
}
add("'\\u0061\\u0301'.normalize('NFC') === '\\u00e1'", "String.prototype.normalize.call(123, 'NFD')", "String.prototype.normalize.call({toString(){return '\\u00c5'}}, 'NFD').length",
  "String.prototype.normalize.call(null)", "String.prototype.normalize.call(undefined, 'NFC')", "'a'.normalize({toString(){return 'NFD'}})", "'a'.normalize(Symbol())",
  "'a'.normalize.length", "'a'.normalize.name", "'\\u1e9b\\u0323'.normalize('NFKC')", "'\\u1e9b\\u0323'.normalize('NFKD').length",
  "'x'.repeat(10) + '\\u0301'.repeat(5) + 'y'", "('e' + '\\u0301'.repeat(40)).normalize('NFC').length", "('a\\u0323\\u0307'.repeat(30)).normalize('NFC').length",
  "'\\u0307\\u0323'.repeat(20).normalize('NFD') === '\\u0323\\u0307'.repeat(20)", "('\\u1100\\u1161\\u11a8'.repeat(50)).normalize('NFC').length",
  "'\\uac01'.repeat(20).normalize('NFD').length");

// ---- 2. Caixa: especiais, ligaduras, sigma final, locales.
const caseSamples = [
  "'\\u00df'", "'\\u1e9e'", "'\\u0130'", "'\\u0131'", "'i'", "'I'", "'\\u0049\\u0307'", "'i\\u0307'", "'\\u03a3'", "'\\u03c3'", "'\\u03c2'", "'\\u0391\\u03a3'", "'\\u0391\\u03a3\\u0392'",
  "'A\\u03a3'", "'A\\u03a3.'", "'A.\\u03a3'", "'A\\u03a3 B'", "'\\u03a3A'", "'a\\u03a3\\u03a3'", "'\\u03a3\\u03a3\\u03a3'", "'A\\u00ad\\u03a3'", "'A\\u0301\\u03a3'",
  "'\\u03a3\\u0301'", "'\\u0391\\u03a3\\u0301'", "'\\ud835\\udc00\\u03a3'", "'\\u03a3\\ud835\\udc00'", "'\\ufb00'", "'\\ufb01'", "'\\ufb02'", "'\\ufb03'", "'\\ufb04'", "'\\ufb05'", "'\\ufb06'",
  "'\\u0587'", "'\\u0149'", "'\\u01f0'", "'\\u0390'", "'\\u03b0'", "'\\u1e96'", "'\\u1e97'", "'\\u1e98'", "'\\u1e99'", "'\\u1e9a'", "'\\u1f50'", "'\\u1f80'", "'\\u1f88'", "'\\u1fb3'",
  "'\\u1fbc'", "'\\u1ff3'", "'\\u1ffc'", "'\\u1f6f'", "'\\u01c4'", "'\\u01c5'", "'\\u01c6'", "'\\u01c7'", "'\\u01c8'", "'\\u01c9'", "'\\u01ca'", "'\\u01cb'", "'\\u01cc'", "'\\u01f1'", "'\\u01f2'",
  "'\\u2160'", "'\\u2170'", "'\\u24b6'", "'\\u24d0'", "'\\u10d0'", "'\\u1c90'", "'\\u00b5'", "'\\u017f'", "'\\u212a'", "'\\u2126'", "'\\u212b'", "'\\ud801\\udc00'", "'\\ud801\\udc28'",
  "'\\ud83d'", "'\\ude00'", "'a\\ud801'", "'\\ud801\\udc28b\\ud801'", "'\\ud806\\udcc0'", "'\\ud806\\udce0'", "'\\ud803\\udcc0'", "'\\ud81b\\ude40'", "'\\ud81b\\ude60'", "'\\ud83a\\udd00'",
  "'\\u0345'", "'\\u0399'", "'\\u03b9'", "'\\u1fbe'", "'\\u0307'", "'\\u0130\\u0307'", "'\\u00c7'", "'\\u0069\\u0307\\u0301'",
];
for (const s of caseSamples) {
  add(`${s}.toUpperCase()`, `${s}.toLowerCase()`, `${s}.toLocaleUpperCase()`, `${s}.toLocaleLowerCase()`, `${s}.toUpperCase().toLowerCase().length`,
    `${s}.toLowerCase().toUpperCase().length`, `${s}.toLowerCase().toLowerCase() === ${s}.toLowerCase()`);
}
const locales = ["'tr'", "'az'", "'lt'", "'el'", "'de'", "'en-US'", "'tr-TR'", "'und'", "['tr','en']", "undefined"];
for (const s of ["'\\u0130'", "'\\u0131'", "'i'", "'I'", "'\\u0049\\u0307'", "'\\u00df'", "'\\u03a3'", "'A\\u03a3'", "'ii\\u0130\\u0131'", "'\\ufb01'", "'\\u00cc'", "'J\\u0307'", "'I\\u0307'"]) {
  for (const l of locales) add(`${s}.toLocaleUpperCase(${l})`, `${s}.toLocaleLowerCase(${l})`);
}
add("'a'.toLocaleUpperCase('x')", "'a'.toLocaleUpperCase('tr_TR')", "'a'.toLocaleUpperCase(1)", "'a'.toLocaleLowerCase([1])", "'a'.toLocaleUpperCase({})",
  "'a'.toLocaleUpperCase([undefined])", "'a'.toLocaleLowerCase('tr-')", "'a'.toLocaleLowerCase('')", "'a'.toLocaleLowerCase(['tr','tr'])",
  "'\\u00df'.repeat(5).toUpperCase()", "'\\ufb03'.repeat(4).toUpperCase().length", "'\\u0130'.repeat(3).toLowerCase().length", "'\\u0130'.toLowerCase().length",
  "'\\u0130'.toLowerCase().codePointAt(1)", "'\\u1f80'.toUpperCase().length", "'\\u0149'.toUpperCase().length", "'\\u0390'.toUpperCase().length",
  "'ǅ'.toUpperCase()", "'ǅ'.toLowerCase()", "'ΑΣ'.toLowerCase()", "'ΑΣΑ'.toLowerCase()", "'ΑΣ.'.toLowerCase()", "'ΑΣ.Α'.toLowerCase()",
  "String.prototype.toUpperCase.call(null)", "String.prototype.toLowerCase.call(undefined)", "String.prototype.toLocaleUpperCase.call(null)",
  "String.prototype.toUpperCase.call(123)", "String.prototype.toUpperCase.call({toString(){return 'x\\u00df'}})", "'a'.toUpperCase.length", "'a'.toLocaleLowerCase.length");

// ---- 3. localeCompare.
const cmp = [
  ["'a'", "'b'"], ["'a'", "'A'"], ["'A'", "'a'"], ["'a'", "'a'"], ["'\\u00e9'", "'e\\u0301'"], ["'e\\u0301'", "'e'"], ["'\\u00e9'", "'f'"], ["'\\u00e4'", "'z'"],
  ["'\\u00e4'", "'a'"], ["'z'", "'\\u00e4'"], ["'\\u00df'", "'ss'"], ["'ss'", "'\\u00df'"], ["'\\ufb01'", "'fi'"], ["'\\u212b'", "'\\u00c5'"], ["'\\u00c5'", "'A\\u030a'"],
  ["'\\u0130'", "'I'"], ["'\\u0131'", "'i'"], ["'\\u03c3'", "'\\u03c2'"], ["'\\u03a3'", "'\\u03c3'"], ["''", "'a'"], ["'a'", "''"], ["''", "''"], ["'a'", "'ab'"],
  ["'\\ud83d'", "'\\ud83d\\ude00'"], ["'\\ud83d\\ude00'", "'\\ud83d\\ude01'"], ["'\\ud83d'", "'\\ude00'"], ["'\\ude00'", "'\\ud83d'"], ["'\\ud83d'", "'a'"], ["'a'", "'\\ud83d'"],
  ["'\\ud83d\\ude00'", "'a'"], ["'\\uff21'", "'A'"], ["'\\uff41'", "'a'"], ["'\\u2460'", "'1'"], ["'1'", "'2'"], ["'10'", "'2'"], ["'a b'", "'ab'"], ["'a-b'", "'ab'"],
  ["'\\u00bd'", "'1/2'"], ["'\\u0663'", "'3'"], ["'\\u4e2d'", "'\\u6587'"], ["'\\u3042'", "'\\u30a2'"], ["'\\uac00'", "'\\u1100\\u1161'"], ["'a\\u0000'", "'a'"],
  ["'a\\u200b'", "'a'"], ["'a\\u00ad'", "'a'"], ["'\\u0041\\u030a'", "'\\u00c5'"], ["'\\u00fc'", "'u'"], ["'\\u00fc'", "'v'"], ["'\\u00e5'", "'z'"], ["'\\u00f1'", "'n'"], ["'\\u00f1'", "'o'"],
];
const cmpArgs = ["", ", 'en'", ", 'de'", ", 'sv'", ", 'tr'", ", undefined, {sensitivity:'base'}", ", undefined, {sensitivity:'accent'}", ", undefined, {sensitivity:'case'}",
  ", undefined, {numeric:true}", ", undefined, {ignorePunctuation:true}", ", 'en', {caseFirst:'upper'}", ", 'en-u-kn-true'"];
for (const [a, b] of cmp) {
  add(`${a}.localeCompare(${b})`, `${b}.localeCompare(${a})`);
  for (const extra of cmpArgs.slice(1)) add(`${a}.localeCompare(${b}${extra})`);
}
add("'a'.localeCompare()", "'undefined'.localeCompare()", "'a'.localeCompare(null)", "'null'.localeCompare(null)", "'1'.localeCompare(1)", "'a'.localeCompare('b', 'x')",
  "'a'.localeCompare('b', undefined, {sensitivity:'x'})", "'a'.localeCompare('b', undefined, null)", "'a'.localeCompare.length",
  "['\\u00e4','a','z','\\u00e5','A','Z'].sort((x,y)=>x.localeCompare(y))", "['\\u00e4','a','z','\\u00e5'].sort((x,y)=>x.localeCompare(y,'sv'))",
  "['\\u00e4','a','z','\\u00e5'].sort((x,y)=>x.localeCompare(y,'de'))", "['b','a','B','A','\\u00e1','\\u00c1'].sort((x,y)=>x.localeCompare(y))",
  "['10','9','2','1'].sort((x,y)=>x.localeCompare(y,undefined,{numeric:true}))", "['\\ud83d\\ude00','\\ud83d','a','\\ude00'].sort((x,y)=>x.localeCompare(y))",
  "String.prototype.localeCompare.call(null,'a')", "String.prototype.localeCompare.call(1,'1')");

// ---- 4. isWellFormed / toWellFormed.
const wf = [
  "''", "'abc'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'\\ude00\\ud83d'", "'a\\ud83d'", "'\\ud83da'", "'\\ude00a'", "'a\\ude00'", "'\\ud83d\\ud83d'", "'\\ude00\\ude00'",
  "'\\ud83d\\ud83d\\ude00'", "'\\ud83d\\ude00\\ude00'", "'\\ud83d\\ude00\\ud83d'", "'\\ud800'", "'\\udbff'", "'\\udc00'", "'\\udfff'", "'\\ud800\\udc00'", "'\\udbff\\udfff'",
  "'\\ud7ff'", "'\\ue000'", "'\\ud83d\\u0041'", "'\\u0041\\ude00'", "'\\ud83d\\ud83d\\ude00\\ude00'", "'x'.repeat(100)+'\\ud83d'", "'\\ud83d'+'x'.repeat(100)",
  "'\\ud83d\\ude00'.repeat(50)", "'\\ud83d\\ude00'.repeat(50)+'\\ud83d'", "'\\ud83d\\ude00'.slice(1)", "'\\ud83d\\ude00'.slice(0,1)", "'\\ud83d\\ude00'.substring(1,2)",
  "'a\\ud83d\\ude00b'.slice(2)", "'a\\ud83d\\ude00b'.slice(0,2)", "String.fromCharCode(0xd83d)", "String.fromCharCode(0xd83d, 0xde00)", "String.fromCodePoint(0x1f600)",
  "String.fromCharCode(0xde00, 0xd83d)", "'\\ud83d'.concat('\\ude00')", "'\\ud83d' + '\\ude00'", "['\\ud83d','\\ude00'].join('')", "'\\ud83d'.padEnd(2,'\\ude00')",
];
for (const s of wf) add(`${s}.isWellFormed()`, `${s}.toWellFormed()`, `${s}.toWellFormed().length`, `${s}.toWellFormed().isWellFormed()`, `${s}.toWellFormed() === ${s}`,
  `Array.from(${s}.toWellFormed(), c => c.codePointAt(0).toString(16))`, `encodeURIComponent(${s}.toWellFormed())`, `T2(()=>encodeURIComponent(${s}))`);
add("String.prototype.isWellFormed.call(null)", "String.prototype.toWellFormed.call(undefined)", "String.prototype.isWellFormed.call(1)", "String.prototype.toWellFormed.call({toString(){return '\\ud83d'}})",
  "String.prototype.isWellFormed.call({toString(){throw new RangeError('x')}})", "'a'.isWellFormed.length", "'a'.toWellFormed.name", "'a'.isWellFormed(1,2,3)",
  "new String('\\ud83d').isWellFormed()", "new String('\\ud83d').toWellFormed()", "Object('\\ud83d\\ude00').isWellFormed()", "'\\ud83d'.isWellFormed.call('\\ud83d\\ude00')",
  "Object.getOwnPropertyDescriptor(String.prototype,'isWellFormed').enumerable", "Object.getOwnPropertyDescriptor(String.prototype,'toWellFormed').writable");

// ---- 5. at / charAt / charCodeAt / codePointAt com surrogates.
const sg = ["'\\ud83d\\ude00'", "'a\\ud83d\\ude00b'", "'\\ud83d'", "'\\ude00'", "'\\ude00\\ud83d'", "'\\ud83d\\ud83d\\ude00'", "'\\ud83d\\ude00\\ude00'", "'\\ud800\\udc00\\udbff\\udfff'", "'\\ud83d\\ude00\\ud83d\\ude01'", "'\\ud83d\\u0041'"];
const idx = ["0", "1", "2", "3", "4", "-1", "-2", "-5", "NaN", "Infinity", "-Infinity", "0.9", "-0.9", "1.9", "undefined", "null", "'1'", "'x'", "true", "{valueOf(){return 1}}", "2**32", "-(2**32)", "2**53"];
for (const s of sg) for (const i of idx) add(`${s}.at(${i})`, `${s}.codePointAt(${i})`, `${s}.charAt(${i})`, `${s}.charCodeAt(${i})`);
for (const s of sg) add(`${s}.at()`, `${s}.codePointAt()`, `Array.from(${s}).length`, `[...${s}].length`, `${s}.length`, `${s}.split('').length`, `Array.from(${s}, c => c.length)`,
  `[...${s}].map(c => c.codePointAt(0).toString(16))`, `${s}.codePointAt(0) > 0xffff`, `String.fromCodePoint(...[...${s}].map(c => c.codePointAt(0))) === ${s}`);
add("''.at(0)", "''.at(-1)", "''.codePointAt(0)", "'a'.at(Symbol())", "String.prototype.at.call(null,0)", "String.prototype.codePointAt.call(undefined,0)", "String.prototype.at.call(12345, -1)",
  "'a'.at.length", "'a'.codePointAt.length", "'abc'.at({valueOf(){throw new Error('v')}})", "'abc'.at(1n)", "'abc'.codePointAt(1n)", "String.fromCodePoint(0x110000)", "String.fromCodePoint(-1)",
  "String.fromCodePoint(1.5)", "String.fromCodePoint('x')", "String.fromCodePoint(0x10ffff).length", "String.fromCodePoint(0xd800, 0xdc00).length", "String.fromCodePoint(0xd800, 0xdc00) === '\\ud800\\udc00'",
  "String.fromCodePoint()", "String.fromCodePoint(NaN)", "String.fromCodePoint(Infinity)", "String.fromCodePoint(null)", "String.fromCodePoint(undefined)", "String.fromCodePoint(0x41, 0x1f600, 0x42).length",
  "String.fromCharCode(0x10041)", "String.fromCharCode(-1).charCodeAt(0)", "String.fromCharCode(65.9)", "String.fromCharCode(2**32 + 65)");

// ---- 6. padStart / padEnd com strings longas.
const padFill = ["' '", "'ab'", "'\\ud83d\\ude00'", "'\\ud83d'", "'\\ude00'", "'abc'.repeat(50)", "''", "undefined", "null", "'\\u0301'", "'xy'.repeat(1000)", "'\\u00df'"];
const padLens = ["0", "1", "2", "3", "5", "6", "7", "10", "100", "-1", "NaN", "Infinity", "2.9", "undefined", "'4'", "{valueOf(){return 3}}"];
for (const fill of padFill) for (const n of padLens.slice(0, 11).concat(["2.9", "undefined"])) {
  if (n === "Infinity") continue;
  add(`'x'.padStart(${n}, ${fill})`, `'\\ud83d\\ude00'.padEnd(${n}, ${fill})`);
}
for (const n of padLens) add(`'abc'.padStart(${n})`, `'abc'.padEnd(${n})`, `'abc'.padStart(${n}, 'xy')`);
for (const s of ["'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'\\u00e9'", "'e\\u0301'", "''"]) {
  add(`${s}.padStart(5, 'ab').length`, `${s}.padEnd(5, '\\ud83d\\ude00')`, `${s}.padEnd(6, '\\ud83d\\ude00').isWellFormed()`, `${s}.padStart(4, '\\ud83d\\ude00').toWellFormed()`,
    `${s}.padStart(1e5, 'xyz').length`, `${s}.padEnd(1e5, 'xyz').slice(-4)`);
}
add("'abc'.padStart(2**30, '')", "'abc'.padStart(2**30, 'x').length", "'abc'.padStart(2**31, 'x')", "'abc'.padEnd(2**53, 'x')", "'abc'.padStart(2**53, '')", "'abc'.padStart(Infinity)",
  "'abc'.padStart(Infinity, '')", "'abc'.padEnd(Symbol())", "'abc'.padStart(5, Symbol())", "'abc'.padStart(5, {toString(){return '12'}})", "'abc'.padStart(5, 1)", "'abc'.padStart(5, false)",
  "'abc'.padStart(5, [1,2])", "String.prototype.padStart.call(null, 3)", "String.prototype.padEnd.call(12, 5, '0')", "'a'.padStart.length", "'a'.padEnd.length",
  "(function(){ try { return 'x'.padStart(2**31 - 1, 'ab').length } catch (e) { return e.name } })()", "'x'.padEnd(10, 'ab'.repeat(3)).length");

// ---- 7. replaceAll / replace com padrões `$`.
const dollar = ["'$$'", "'$&'", "'$`'", "\"$'\"", "'$1'", "'$01'", "'$10'", "'$<n>'", "'$0'", "'$'", "'$$$'", "'$&$&'", "'[$`|$&|$\\']'", "'$00'", "'$99'", "'$<'", "'$<>'", "'x$'", "'$ '", "'\\ud83d$&\\ude00'"];
const hay = ["'abcabc'", "'a.b.a'", "'\\ud83d\\ude00x\\ud83d\\ude00'", "'aaa'", "''", "'abc'"];
for (const h of hay) for (const d of dollar) add(`${h}.replaceAll('a', ${d})`, `${h}.replace('a', ${d})`, `${h}.replaceAll('', ${d})`, `${h}.replaceAll('b', ${d})`);
for (const d of dollar) add(`'abcabc'.replaceAll(/(?<n>a)(b)?/g, ${d})`, `'abcabc'.replace(/(a)(b)(c)/, ${d})`, `'abcabc'.replaceAll(/a/g, ${d})`, `'abcabc'.replaceAll(/(?:)/g, ${d})`,
  `'\\ud83d\\ude00\\ud83d\\ude00'.replaceAll(/(?:)/gu, ${d})`, `'\\ud83d\\ude00'.replaceAll(/(?:)/g, ${d})`);
add("'abc'.replaceAll(/a/, 'x')", "'abc'.replaceAll(/a/y, 'x')", "'abc'.replaceAll({[Symbol.replace]: (s, r) => s + r}, 'z')", "'abc'.replaceAll('b', (m, i, s) => m + i + s)",
  "'abcabc'.replaceAll('b', function(){ return arguments.length })", "'abcabc'.replaceAll('', (m, i) => i)", "'aXbX'.replaceAll('X', () => '$&')", "'aXbX'.replaceAll('X', () => '$$')",
  "'abc'.replaceAll('b', undefined)", "'abc'.replaceAll('b', null)", "'abc'.replaceAll(undefined, 'z')", "'undefined'.replaceAll(undefined, 'z')", "'abc'.replaceAll('b')",
  "'abc'.replaceAll()", "'a.c'.replaceAll('.', 'x')", "'abc'.replaceAll(/b/g)", "'a+b'.replaceAll('+', '-')", "'\\u00e9e\\u0301'.replaceAll('\\u00e9', 'X')",
  "'\\u00e9e\\u0301'.replaceAll('e', 'X')", "'abc'.replaceAll.length", "String.prototype.replaceAll.call(null, 'a', 'b')", "'abc'.replaceAll({toString(){return 'b'}}, 'X')",
  "'abc'.replaceAll(/b/g, {toString(){return 'Y'}})", "'xaax'.replaceAll('aa', '$`')", "'xaax'.replaceAll('aa', \"$'\")", "'aaa'.replaceAll('aa', 'b')", "'aaaa'.replaceAll('aa', 'b')",
  "'abc'.replaceAll('', '_')", "''.replaceAll('', '_')", "'\\ud83d\\ude00'.replaceAll('', '_')", "'\\ud83d\\ude00'.replaceAll('\\ud83d', 'X')", "'\\ud83d\\ude00'.replaceAll('\\ude00', 'X')",
  "'\\ud83d\\ude00'.replace('\\ud83d', 'X').isWellFormed()", "'a$b'.replaceAll('$', '$$')", "'a$b'.replaceAll('$', '$&$&')");

// ---- 8. split com regex Unicode.
const splitSubj = ["'a\\ud83d\\ude00b'", "'\\ud83d\\ude00\\ud83d\\ude00'", "'\\ud83d'", "'\\ude00'", "'a,b;c'", "'\\u00e9e\\u0301'", "''", "'\\u0130i'", "'abc'", "'a\\u2028b\\nc\\u2029d'", "'\\ud83d\\ude00x'", "'x\\ud83d\\ude00'"];
const splitSeps = ["/(?:)/u", "/(?:)/", "/(?:)/v", "/./u", "/./", "/./s", "/./su", "/\\u{1f600}/u", "/\\ud83d/", "/\\ud83d/u", "/\\ude00/u", "/[\\ud83d\\ude00]/u", "/[\\ud83d\\ude00]/", "/\\p{L}/u", "/\\P{L}/u",
  "/(\\p{L})/u", "/\\p{Emoji}/u", "/\\p{Emoji}/v", "/[\\p{L}--[a-z]]/v", "/\\s/u", "/\\W/u", "/\\W/iu", "/\\b/u", "/(?<=a)/u", "/(?=\\ud83d)/", "/(?=\\ud83d)/u", "/,|;/u", "/(,)|(;)/u", "/\\u{61}/u", "/$/u", "/^/u", "/^/mu", "/a*/u"];
for (const s of splitSubj) for (const sep of splitSeps) add(`${s}.split(${sep})`);
for (const s of splitSubj.slice(0, 6)) for (const sep of ["/(?:)/u", "/./u", "/\\p{L}/u"]) for (const lim of ["0", "1", "2", "-1", "undefined", "2**32", "2**32+1"]) add(`${s}.split(${sep}, ${lim})`);
for (const s of splitSubj) add(`${s}.split('')`, `${s}.split('', 2)`, `${s}.split(undefined)`, `${s}.split(undefined, 0)`, `${s}.split()`, `${s}.split(null)`, `${s}.split('\\ud83d')`, `${s}.split('\\ude00')`);
add("'abc'.split({[Symbol.split](s, l){ return [s, l] }}, 5)", "'abc'.split.length", "String.prototype.split.call(null, '')", "'a1b2'.split(/\\d/y)", "'a1b2'.split(/(\\d)/g, 3)",
  "'\\ud83d\\ude00'.split(/(?:)/u).every(c => c.length === 2)", "'\\ud83d\\ude00'.split(/(?:)/).every(c => c.length === 1)", "Array.from('\\ud83d\\ude00a').length");

// ---- 9. String.raw e templates.
add("String.raw`a\\nb`", "String.raw`\\u{1f600}`", "String.raw`\\ud83d\\ude00`", "String.raw`\\x41${1}\\u0042`", "String.raw`${1}${2}`", "String.raw`\\``", "String.raw`${'\\n'}`",
  "String.raw({raw: ['a','b','c']}, 1, 2, 3)", "String.raw({raw: ['a','b','c']}, 1)", "String.raw({raw: 'abc'}, 1, 2)", "String.raw({raw: 'abc'})", "String.raw({raw: {length: 3, 0:'x', 1:'y', 2:'z'}}, '-', '+')",
  "String.raw({raw: {length: 0}}, 1)", "String.raw({raw: {length: -1}})", "String.raw({raw: []})", "String.raw({raw: ['\\ud83d','\\ude00']}, 'x')", "String.raw({raw: ['\\ud83d','\\ude00']}).isWellFormed()",
  "String.raw()", "String.raw({})", "String.raw(null)", "String.raw(undefined)", "String.raw({raw: null})", "String.raw({raw: undefined})", "String.raw({raw: 1})", "String.raw({raw: {length: 2**32, 0: 'a'}}).length > 0",
  "String.raw({raw: ['a','b']}, Symbol())", "String.raw({raw: ['a','b']}, {toString(){return 'T'}})", "String.raw({raw: ['a','b']}, 1n)", "String.raw({raw: [1, 2]}, 3)",
  "String.raw({raw: {length: 1.9, 0: 'a', 1: 'b'}})", "String.raw({raw: {length: '2', 0: 'a', 1: 'b'}}, '-')", "String.raw.length", "String.raw.name", "String.raw`\\\\`", "String.raw`\\u00e9`.length",
  "String.raw`\\u{10ffff}`.length", "String.raw`a${1}${2}b${3}`", "(s => s.raw[0])`\\u{zzz}`", "(s => s[0])`\\u{zzz}`", "(s => s.raw.length)`${1}${2}`", "(s => [s[0], s.raw[0]])`\\unicode and \\u{55}`",
  "(s => Object.isFrozen(s) && Object.isFrozen(s.raw))`a`", "(s => Array.isArray(s.raw))`a`", "((s, ...v) => v.length)`${1}${2}${3}`", "`${'\\ud83d'}${'\\ude00'}`.length", "`\\ud83d\\ude00`.length",
  "`${'\\ud83d'}${'\\ude00'}`.isWellFormed()", "`a${Symbol.iterator.description}b`", "`${[1,[2,3]]}`", "`${{toString(){return 'T'}}}`", "`${{valueOf(){return 1}, toString: undefined}}`", "`${null}${undefined}`",
  "(function(){ try { return `${Symbol()}` } catch (e) { return e.name + ': ' + e.message } })()", "(function(){ try { return `${1n}` } catch (e) { return e.name } })()");

// ---- 10. matchAll e as flags v / u / d / y / g.
const maSubj = ["'a\\ud83d\\ude00b\\ud83d\\ude00'", "'\\ud83d\\ude00'", "'abcabc'", "'\\u00e9e\\u0301'", "''", "'\\ud83d'", "'aAbB'", "'\\u0130i\\u0131I'", "'\\u212a\\u017f'", "'x1y22z333'"];
const maPats = ["/./gu", "/./g", "/./gs", "/./gv", "/(?:)/gu", "/(?:)/g", "/\\p{L}+/gu", "/\\p{Emoji}/gu", "/\\p{Emoji}/gv", "/\\p{RGI_Emoji}/gv", "/[\\p{L}&&\\p{ASCII}]/gv", "/[\\p{L}--\\p{ASCII}]/gv", "/[^\\p{L}]/gv",
  "/[\\q{abc|d}]/gv", "/(a)(b)?/g", "/(?<l>[a-z])(?<d>\\d)?/g", "/\\d+/g", "/\\d+/gd", "/(\\d)(\\d)?/gd", "/(?<x>\\d)/gdu", "/k/giu", "/k/gi", "/s/giu", "/s/gi", "/i/giu", "/I/gi", "/\\u{1f600}/gu", "/\\ud83d/g", "/\\ud83d/gu", "/./gy", "/a/gy", "/\\w/gi", "/\\w/giu", "/\\W/giu", "/\\b/gu", "/[a-z]/gi", "/[^a-z]/giu"];
for (const s of maSubj) for (const p of maPats) {
  add(`Array.from(${s}.matchAll(${p}), m => [m[0], m.index])`);
  if (!p.includes("(?<")) add(`[...${s}.matchAll(${p})].length`);
}
for (const p of ["/(\\d)(\\d)?/gd", "/(?<x>\\d)/gd", "/\\d+/gd", "/(?<x>\\p{L})/gdu", "/(a)|(b)/gd", "/(?:)/gd"]) {
  for (const s of ["'a1b22'", "'\\ud83d\\ude00a1'", "'ab'", "''"]) add(`Array.from(${s}.matchAll(${p}), m => JSON.stringify([m.indices, m.indices && m.indices.groups]))`);
}
for (const s of ["'a\\ud83d\\ude00'", "'\\ud83d\\ude00'", "'\\ud83d'", "'abc'"]) for (const p of ["/./", "/./u", "/./v", "/./s", "/./d", "/./y", "/(?:)/u", "/(?:)/", "/\\p{L}/u", "/a/y", "/\\ud83d/u", "/\\ud83d/", "/\\u{1f600}/u", "/[^a]/u", "/[^a]/"]) {
  add(`${s}.match(${p})`, `${s}.search(${p})`, `${p}.test(${s})`, `${p}.exec(${s})`);
}
for (const p of ["/a/g", "/a/y", "/a/gy", "/./gu", "/(?:)/gu", "/(?:)/g", "/a/", "/a/d", "/a/v", "/a/gv"]) {
  add(`(function(){ var r = ${p}; r.lastIndex = 1; var m = r.exec('aaa'); return [m && m.index, r.lastIndex] })()`,
    `(function(){ var r = ${p}; r.lastIndex = 5; var m = r.exec('aaa'); return [m, r.lastIndex] })()`,
    `(function(){ var r = ${p}; r.lastIndex = 1; return ['aaa'.replace(r, 'X'), r.lastIndex] })()`,
    `(function(){ var r = ${p}; return [r.flags, r.global, r.sticky, r.unicode, r.unicodeSets, r.hasIndices, r.dotAll, r.ignoreCase, r.multiline].join() })()`,
    `(function(){ var r = ${p}; r.lastIndex = 1; return ['aaa'.split(r), r.lastIndex] })()`);
}
for (const p of ["/\\ud83d/u", "/./u", "/(?:)/u", "/\\udc00/u"]) for (const li of ["0", "1", "2", "3"]) {
  add(`(function(){ var r = new RegExp(${p}.source, 'gu'); r.lastIndex = ${li}; var m = r.exec('\\ud83d\\ude00\\ud83d\\ude00'); return [m && m.index, r.lastIndex] })()`,
    `(function(){ var r = new RegExp(${p}.source, 'yu'); r.lastIndex = ${li}; var m = r.exec('\\ud83d\\ude00\\ud83d\\ude00'); return [m && m.index, r.lastIndex] })()`);
}
add("'a'.matchAll(/a/)", "'a'.matchAll('a').next().value[0]", "'a.a'.matchAll('.').next().value[0]", "[...'a.a'.matchAll('.')].length", "'a'.matchAll(null)", "'null'.matchAll(null).next().value[0]",
  "'a'.matchAll(undefined).next().value[0]", "'a'.matchAll()", "'a'.matchAll.length", "'abc'.matchAll(/b/g)[Symbol.toStringTag]", "Object.getPrototypeOf('a'.matchAll(/a/g))[Symbol.toStringTag]",
  "(function(){ var r = /a/g; r.lastIndex = 1; return [...'aaa'.matchAll(r)].map(m => m.index) })()", "(function(){ var r = /a/g; r.lastIndex = 1; var it = 'aaa'.matchAll(r); r.lastIndex = 0; return [...it].length + ':' + r.lastIndex })()",
  "(function(){ var r = /a/y; return [...'aaa'.matchAll(new RegExp(r, 'g'))].length })()", "[...'aaa'.matchAll(/a/gy)].length", "'a'.matchAll({[Symbol.matchAll]: s => 'M' + s})", "'a'.matchAll({[Symbol.matchAll]: null, toString(){return 'a'}}).next().value[0]",
  "'abc'.matchAll(/b/gi).next()", "'abc'.matchAll(/b/g).next.name", "(function(){ var it = 'ab'.matchAll(/./g); it.next(); it.next(); return JSON.stringify(it.next()) })()",
  "new RegExp('[\\\\p{L}--[a-z]]', 'v').test('A')", "new RegExp('[\\\\p{L}--[a-z]]', 'v').test('a')", "new RegExp('\\\\p{RGI_Emoji}', 'v').test('\\ud83d\\ude00')", "new RegExp('\\\\p{RGI_Emoji}', 'v').exec('\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67')[0].length",
  "new RegExp('\\\\p{RGI_Emoji}', 'u')", "new RegExp('a', 'uv')", "new RegExp('a', 'vu')", "new RegExp('[a-z&&[aeiou]]', 'v').test('e')", "new RegExp('[a-z&&[aeiou]]', 'v').test('b')", "new RegExp('[a&&&b]', 'v')",
  "new RegExp('[(]', 'v')", "new RegExp('[(]', 'u').test('(')", "new RegExp('[\\\\q{ab|c}]', 'v').exec('xabx')[0]", "new RegExp('[\\\\q{ab|c}]', 'u')", "new RegExp('\\\\P{Emoji_Keycap_Sequence}', 'v')",
  "new RegExp('[^\\\\p{RGI_Emoji}]', 'v')", "new RegExp('\\\\p{Lowercase}', 'iv').test('A')", "new RegExp('\\\\P{Lowercase}', 'iv').test('A')", "new RegExp('[^\\\\P{Lowercase}]', 'iv').test('A')", "new RegExp('\\\\P{Lowercase}', 'iu').test('A')",
  "/\\u{1f600}/.test('\\ud83d\\ude00')", "/\\u{1f600}/u.test('\\ud83d\\ude00')", "/^.$/u.test('\\ud83d\\ude00')", "/^.$/.test('\\ud83d\\ude00')", "/^..$/.test('\\ud83d\\ude00')", "/\\ud83d\\ude00/.test('\\ud83d\\ude00')",
  "/^[\\ud83d\\ude00]$/u.test('\\ud83d\\ude00')", "/^[\\ud83d\\ude00]$/.test('\\ud83d\\ude00')", "/\\udc00/u.test('\\ud800\\udc00')", "/\\udc00/.test('\\ud800\\udc00')", "/\\u{110000}/u", "new RegExp('\\\\u{110000}', 'u')",
  "new RegExp('\\\\u{10ffff}', 'u').test('\\udbff\\udfff')", "/[\\u{1f600}-\\u{1f64f}]/u.test('\\ud83d\\ude42')", "/[\\ud83d\\ude00-\\ud83d\\ude4f]/u.test('\\ud83d\\ude42')", "new RegExp('[\\ud83d\\ude00-\\ud83d\\ude4f]', 'u').test('\\ud83d\\ude42')",
  "/\\p{Script=Greek}/u.test('\\u03b1')", "/\\p{sc=Grek}/u.test('\\u03b1')", "/\\p{scx=Grek}/u.test('\\u0342')", "/\\p{Script_Extensions=Latin}/u.test('a')", "/\\p{Lu}/u.test('\\u0130')", "/\\p{Ll}/u.test('\\u00df')",
  "/\\p{Lt}/u.test('\\u01c5')", "/\\p{Any}/u.test('\\ud800')", "/\\p{ASCII}/u.test('\\u0080')", "/\\p{Assigned}/u.test('\\u0378')", "/\\p{L}/u.test('\\ud83d')", "/\\p{Cs}/u.test('\\ud800')", "/\\p{Cs}/u.test('\\ud83d\\ude00')",
  "/\\p{Nd}/u.test('\\u0663')", "/\\d/u.test('\\u0663')", "/\\p{White_Space}/u.test('\\u00a0')", "/\\s/u.test('\\ufeff')", "/\\p{Foo}/u", "/\\p{L/u", "/\\p{}/u", "/\\p/u", "/\\p{IsL}/u", "/\\p{Letter}/u.test('a')", "/\\p{General_Category=L}/u.test('a')",
  "/\\p{gc=Lu}/u.test('A')", "/\\p{Script=Latn}/iu.test('A')", "/[\\p{Lu}]/iu.test('a')", "/\\p{Lu}/iu.test('a')", "/\\P{Lu}/iu.test('a')", "/\\w/iu.test('\\u017f')", "/\\w/i.test('\\u017f')", "/\\W/iu.test('\\u017f')", "/[\\w]/iu.test('\\u212a')",
  "/\\u212a/i.test('k')", "/\\u212a/iu.test('k')", "/\\u017f/i.test('s')", "/\\u017f/iu.test('s')", "/\\u00df/i.test('\\u1e9e')", "/\\u00df/iu.test('\\u1e9e')", "/\\u0130/i.test('i')", "/\\u0130/iu.test('i')", "/\\u0131/iu.test('i')",
  "/\\u03c3/i.test('\\u03c2')", "/\\u03c2/iu.test('\\u03a3')", "/\\u1e9e/iu.test('ss')", "/\\ufb01/iu.test('fi')", "/\\u01c5/iu.test('\\u01c4')", "/\\u01c5/i.test('\\u01c6')", "/\\ud801\\udc00/iu.test('\\ud801\\udc28')", "/\\ud801\\udc00/i.test('\\ud801\\udc28')",
  "/[\\ud801\\udc00]/iu.test('\\ud801\\udc28')", "/\\b/u.test('\\u017f')", "/\\bs\\b/iu.test('\\u017f')", "/\\Bs\\B/iu.test('a\\u017fa')");

// ---- 11. Misc de string com Unicode: iteradores, slice/substring em surrogates, indexOf, trim, repeat, concat.
const mx = ["'\\ud83d\\ude00'", "'a\\ud83d\\ude00b'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00\\ud83d\\ude00'", "'\\u00e9e\\u0301'", "'\\u2028\\u2029\\u00a0\\ufeff x \\u3000\\u180e'"];
for (const s of mx) add(`${s}.trim().length`, `${s}.trimStart().length`, `${s}.trimEnd().length`, `${s}.slice(1)`, `${s}.slice(-1)`, `${s}.substring(1)`, `${s}.substring(1, 0)`, `${s}.substr(-1)`,
  `${s}.indexOf('\\ud83d')`, `${s}.indexOf('\\ude00')`, `${s}.lastIndexOf('\\ud83d')`, `${s}.includes('\\ude00')`, `${s}.startsWith('\\ud83d')`, `${s}.endsWith('\\ude00')`, `${s}.endsWith('\\ud83d', 1)`,
  `${s}.repeat(2).length`, `${s}.concat('\\ud83d', '\\ude00').isWellFormed()`, `${s}[Symbol.iterator]().next()`, `[...${s}].reverse().join('')`, `${s}.split('').reverse().join('').isWellFormed()`,
  `Array.from(${s}.matchAll(/./gu)).length`, `${s}.search(/\\ud83d/u)`, `${s}.search(/\\ud83d/)`, `${s}.lastIndexOf('')`, `${s}.indexOf('', 99)`, `${s}.localeCompare(${s}.normalize('NFD'))`,
  `escape(${s})`, `unescape(escape(${s})) === ${s}`, `encodeURI(${s}.toWellFormed())`, `JSON.stringify(${s})`, `JSON.stringify(${s}.toWellFormed())`, `${s}.anchor('x')`, `${s}.toString() === ${s}`, `${s}.valueOf().length`,
  `new String(${s}).length`, `Object.keys(new String(${s})).length`, `Object.getOwnPropertyNames(new String(${s})).length`, `${s}.big().length`, `typeof ${s}[0]`, `${s}[0].codePointAt(0)`);
add("'\\u180e'.trim().length", "'\\u200b'.trim().length", "'\\u0085'.trim().length", "'\\u2007'.trim().length", "'\\u202f'.trim().length", "'\\u205f'.trim().length", "'\\ufeff'.trim().length", "'\\u00a0'.trim().length",
  "'\\u1680'.trim().length", "'\\u2000\\u200a'.trim().length", "'\\u2028\\u2029'.trim().length", "'\\u000b\\u000c'.trim().length", "'\\u0009\\u000a\\u000d'.trim().length", "'\\u0001'.trim().length");

// ---- Execução, deduplicando contra os goldens existentes.
const baseSources = new Set();
for (const program of knownPrograms("string_unicode_more_bun.tsv", ["string_unicode_bun.tsv", "string_bun.tsv"])) baseSources.add(JSON.stringify(program));
const seen = new Set();
const allUnique = exprs.filter(e => !seen.has(e) && seen.add(e));
// A matriz completa passa de 8 mil programas; a amostra por hash (sampleByHash) mantém ~600.
const TARGET = Number(process.env.TARGET || 600);
const unique = sampleByHash(allUnique, TARGET);
process.stderr.write(`gerados ${allUnique.length}, amostrados ${unique.length}\n`);
let kept = 0;
let dropped = 0;
let dup = 0;
// `T2` só existe nas expressões de encodeURIComponent: devolve o nome do erro, sem a mensagem do motor.
const HELPER_ALL = HELPER + "function T2(f){try{return f()}catch(e){return e.name+': '+e.message}}\n";
for (const expr of unique) {
  const source = '"use strict";\n' + HELPER_ALL + "globalThis.R = F(() => (" + expr + "));";
  if (baseSources.has(JSON.stringify(source))) {
    dup++;
    continue;
  }
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result) || /^n\d{9,}$/.test(result)) {
    dropped++;
    process.stderr.write("resultado dependente da máquina: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos do base ${dup}\n`);
