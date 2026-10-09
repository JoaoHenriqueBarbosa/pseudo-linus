// Gera tests/golden/string_unicode_extra_bun.tsv: terceiro complemento de String Unicode, medido no bun 1.4.2.
// Cobre o que string_bun, string_unicode_bun, string_unicode_more_bun, string_method_edge_bun e string_replace_bun
// não trazem: caixa com locale (lt, tr, az, el, nl, inválidos e listas), normalize de Hangul/combinantes/compatíveis
// por pontos de código, split/indexOf/lastIndexOf/includes/startsWith/endsWith com surrogates isolados e posições,
// codePointAt/fromCodePoint nos limites, isWellFormed/toWellFormed com `call`, localeCompare com acentos e locales,
// encodeURI/decodeURI/encodeURIComponent/decodeURIComponent/escape/unescape com sequências inválidas (URIError com a
// mensagem exata), trim de todos os espaços Unicode, Number/parseFloat/parseInt com espaços, identificadores Unicode
// (ID_Start/ID_Continue, escapes \u{...}) em var, propriedade e label, e escapes `\u` inválidos em strings e templates.
// Programas cuja expressão já aparece nos goldens de string existentes são descartados.
// Sem APIs de host. Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a
// gen-object-edge-golden.js. Cada programa roda num bun filho novo.
// Uso: bun scripts/gen-string-unicode-extra-golden.js > tests/golden/string_unicode_extra_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  "function S(v){var t=typeof v;if(t==='string')return 's'+JSON.stringify(v);if(t==='symbol')return 'sym';" +
  "if(t==='number')return Object.is(v,-0)?'-0':'n'+v;if(Array.isArray(v))return '['+v.map(S).join(',')+']';" +
  "if(v&&t==='object')return 'o'+JSON.stringify(Object.keys(v));return t+':'+String(v)}\n" +
  "function T(f){try{return S(f())}catch(e){return 'throw '+(e&&e.name)+': '+(e&&e.message)}}\n" +
  "function E(c){try{return S((0,eval)(c))}catch(e){return 'throw '+(e&&e.name)+': '+(e&&e.message)}}\n" +
  "function H(s){return Array.from(s,function(c){return c.codePointAt(0).toString(16)}).join(' ')}\n";

const exprs = [];
const add = (...list) => exprs.push(...list);
const lit = (s) => "'" + [...s].map((c) => {
  const n = c.codePointAt(0);
  return n < 0x80 && n >= 0x20 && c !== "'" && c !== "\\" ? c : n > 0xffff ? "\\u{" + n.toString(16) + "}" : "\\u" + n.toString(16).padStart(4, "0");
}).join("") + "'";
const units = (arr) => "'" + arr.map((n) => "\\u" + n.toString(16).padStart(4, "0")).join("") + "'";
const cp = (...n) => lit(String.fromCodePoint(...n));

// ---- 1. Caixa com locale.
const caseSamples = [
  "I", "i", "İ", "ı", "i̇", "İ", "İ", "i̇", "Ì", "ì", "Í", "í", "Ĩ", "į", "Į",
  "J̇", "j̇", "Į", "į", "Ĩ", "Ì", "Í", "ì", "í", "ß", "ẞ", "ŉ", "ǰ", "ΐ", "ΰ", "ﬁ", "ﬃ", "ﬆ", "ǅ", "ǈ", "ǋ", "ǲ",
  "Σ", "ΑΣ", "ΑΣΒ", "ΑΣ.", "Α.Σ", "ΑΣ́", "ΑΣ Β", "ΑΣΣ", "Ά", "ΐ", "ᾳ", "ᾼ", "ᾈ", "ῳ", "ῼ", "ᾯ", "ὒ", "ΊΣΙ", "ΣΙ", "ΙΣ",
  "𐐀", "𐐨", "😀", "Iİıiİ", "istanbul", "ISTANBUL", "TITLE", "title", "ÅÖÜ", "åöü", "Ǆ", "ǆ", "ς", "ʼn", "ǰ", "ẖ", "ṷ",
];
const locales = ["'tr'", "'az'", "'lt'", "'el'", "'nl'", "'en'", "'de'", "'tr-TR'", "'az-Latn'", "'lt-LT'", "'TR'", "'tr-u-co-phonebk'", "['lt','tr']", "['xx']",
  "undefined", "[]", "'tr_TR'", "'x-i'", "''", "'en-'", "1", "null", "'und'", "'tur'", "'lt-u-kn'", "'az-Cyrl'"];
for (const s of caseSamples) {
  const L = lit(s);
  for (const loc of locales) add(`${L}.toLocaleUpperCase(${loc})`, `${L}.toLocaleLowerCase(${loc})`);
  add(`H(${L}.toUpperCase())`, `H(${L}.toLowerCase())`, `H(${L}.toLocaleUpperCase('tr'))`, `H(${L}.toLocaleLowerCase('lt'))`, `H(${L}.toLocaleLowerCase('tr'))`);
}
add("String.prototype.toLocaleUpperCase.call(null,'tr')", "String.prototype.toLocaleLowerCase.call(undefined)", "'a'.toLocaleUpperCase.length", "'a'.toLocaleLowerCase.name",
  "'a'.toLocaleUpperCase('tr', 'x')", "'i'.toLocaleUpperCase({toString(){return 'tr'}})", "'i'.toLocaleUpperCase(Symbol())", "'i'.toLocaleUpperCase(['tr','tr'])",
  "'i'.toLocaleUpperCase(['tr', {}])", "'i'.toLocaleUpperCase([undefined])", "'i'.toLocaleUpperCase(new String('tr'))", "'i'.toLocaleUpperCase('TR-latn')");

// ---- 2. normalize por pontos de código.
const hangul = [0xac00, 0xac01, 0xac1c, 0xb098, 0xb2e4, 0xd55c, 0xd788, 0xd7a3, 0xc815, 0xc880, 0xcc44, 0xbd88];
const normSamples = [];
for (const h of hangul) normSamples.push(cp(h), cp(h, 0x11a8), cp(h, 0x0301));
for (const l of [0x1100, 0x1101, 0x1112]) for (const v of [0x1161, 0x1175]) {
  normSamples.push(cp(l, v), cp(l, v, 0x11a8), cp(l, v, 0x11c2), cp(l, v, 0x11a8, 0x11a8), cp(l, 0x1161, 0x1161));
}
normSamples.push(cp(0x1100), cp(0x1161), cp(0x11a8), cp(0x1161, 0x1100), cp(0x11a8, 0x1100), cp(0xac00, 0x1161), cp(0xac01, 0x11a8), cp(0x1100, 0x11a8), cp(0x1176), cp(0x11c3),
  cp(0x3131), cp(0x314f), cp(0xffa1), cp(0xffc2), cp(0x3200), cp(0x3260), cp(0x327e), cp(0xd7b0), cp(0xd7cb));
for (const base of [0x61, 0x65, 0x6f, 0x75, 0x4e, 0x43]) {
  for (const marks of [[0x300], [0x301, 0x323], [0x323, 0x301], [0x308, 0x301, 0x323], [0x327, 0x301], [0x30a, 0x301], [0x345], [0x31b, 0x301, 0x323], [0x0334, 0x0301]]) {
    normSamples.push(cp(base, ...marks));
  }
}
for (const c of [0x1e69, 0x1e0b, 0x1e0d, 0x1ed9, 0x1ec7, 0x01d5, 0x01d7, 0x1fa8, 0x1f87, 0x0344, 0x0958, 0x0929, 0x0d4a, 0x1b06, 0x0f43, 0x0f77, 0x2adc, 0x1d15e, 0x1d160]) normSamples.push(cp(c));
for (const c of [0xfb00, 0xfb01, 0xfb05, 0xfb06, 0x2150, 0x2189, 0x2474, 0x249c, 0x3300, 0x33ff, 0x32c9, 0x3250, 0xfdfa, 0xfdfb, 0xfe30, 0xff01, 0xff5e, 0xffe0,
  0x1d400, 0x1d7ce, 0x1f100, 0x1f19a, 0x2f800, 0x2f801, 0xf900, 0xfa6d, 0x2000, 0x2003, 0x2011, 0x2024, 0x2025, 0x2026, 0x2033, 0x2057, 0x2121, 0x2122,
  0x02b0, 0x02e0, 0x1d2c, 0x1d62, 0x2070, 0x2071, 0x2079, 0x208e, 0x00aa, 0x00ba, 0x00b2, 0x00b3, 0x00b9, 0x00bc, 0x0140, 0x0149, 0x017f, 0x01c4, 0x01f1]) normSamples.push(cp(c));
for (const s of normSamples) {
  add(`H(${s}.normalize('NFC'))`, `H(${s}.normalize('NFD'))`, `H(${s}.normalize('NFKC'))`, `H(${s}.normalize('NFKD'))`,
    `${s}.normalize('NFC').length - ${s}.length`, `${s}.normalize('NFKD').normalize('NFC') === ${s}.normalize('NFKC')`,
    `${s}.normalize('NFD') === ${s}.normalize('NFD').normalize('NFD')`);
}
add("'\\u1100\\u1161'.repeat(100).normalize('NFC').length", "'\\uac00\\u11a8'.repeat(50).normalize('NFC') === '\\uac01'.repeat(50)", "'\\uac01'.repeat(33).normalize('NFD').length",
  "'\\u1100'.repeat(30).normalize('NFC') === '\\u1100'.repeat(30)", "'\\u0301'.repeat(100).normalize('NFC').length", "('a' + '\\u0301\\u0323'.repeat(40)).normalize('NFD').length",
  "('a' + '\\u0323\\u0301'.repeat(40)).normalize('NFC').length", "'\\u00e9'.repeat(200).normalize('NFD').length", "'\\ufb03'.repeat(100).normalize('NFKC')",
  "'\\ud800'.normalize('NFC') === '\\ud800'", "'\\udc00\\ud800'.normalize('NFD').length", "'\\ud800\\u0301'.normalize('NFC').length", "'a\\ud800\\u0301'.normalize('NFD').length",
  "'\\ud835\\udc00\\u0301'.normalize('NFKC').length", "String.prototype.normalize.call(true)", "String.prototype.normalize.call([])", "'x'.normalize(undefined)",
  "'x'.normalize('NFC', 'NFD')", "'x'.normalize({})", "'x'.normalize([])", "'x'.normalize(['NFC'])", "'x'.normalize(new String('NFD'))", "'x'.normalize('\\u004eFC')");

// ---- 3. split/indexOf/lastIndexOf/includes/startsWith/endsWith com surrogates isolados.
const strs = [
  units([0xd83d, 0xde00]), units([0xd83d]), units([0xde00]), units([0xde00, 0xd83d]), units([0x61, 0xd83d, 0xde00, 0x62]), units([0xd83d, 0xd83d, 0xde00]),
  units([0xd83d, 0xde00, 0xde00]), units([0xd83d, 0xde00, 0xd83d, 0xde00]), units([0xde00, 0xd83d, 0xde00]), units([0xd800, 0xdc00, 0xdbff, 0xdfff]), units([0x61, 0xd800, 0x62, 0xdc00, 0x63]),
];
const needles = [units([0xd83d]), units([0xde00]), units([0xd83d, 0xde00]), units([0xde00, 0xd83d]), units([0x61]), "''", units([0xd800]), units([0xdc00]), units([0xdbff, 0xdfff])];
const poss = ["undefined", "0", "1", "2", "-1", "99", "NaN", "1.9", "Infinity", "-Infinity", "'1'", "null"];
for (const s of strs) {
  for (const n of needles) {
    add(`${s}.indexOf(${n})`, `${s}.lastIndexOf(${n})`, `${s}.includes(${n})`, `${s}.startsWith(${n})`, `${s}.endsWith(${n})`,
      `${s}.split(${n}).length`, `${s}.split(${n}).map(x => H(x)).join('|')`);
    for (const p of ["0", "1", "2", "-1", "99", "NaN", "Infinity"]) {
      add(`${s}.indexOf(${n}, ${p})`, `${s}.lastIndexOf(${n}, ${p})`, `${s}.includes(${n}, ${p})`, `${s}.startsWith(${n}, ${p})`, `${s}.endsWith(${n}, ${p})`);
    }
  }
  add(`${s}.split('').length`, `${s}.split('', 2).length`, `${s}.split(undefined)`, `${s}.split(undefined, 0)`, `H(${s}.split('').join(''))`, `${s}.split(/(?:)/u).length`, `${s}.split(/(?:)/).length`,
    `${s}.split(/(?:)/u, 1).length`, `[...${s}].length`, `Array.from(${s}).map(c => c.length).join()`, `${s}.at(-1).length`, `${s}.charAt(1).length`, `H(${s}.substring(1))`, `H(${s}.slice(0, 1))`,
    `${s}.indexOf('', 1.5)`, `${s}.lastIndexOf('', -1)`, `${s}.lastIndexOf('', Infinity)`, `${s}.search(/(?:)/u)`);
}
for (const p of poss) add(`'abc'.indexOf('c', ${p})`, `'abc'.lastIndexOf('a', ${p})`, `'abc'.includes('', ${p})`, `'abc'.startsWith('', ${p})`, `'abc'.endsWith('', ${p})`, `'abc'.endsWith('c', ${p})`);
add("'abc'.includes(/b/)", "'abc'.startsWith(/a/)", "'abc'.endsWith(/c/)", "'abc'.includes({[Symbol.match]: false, toString(){return 'b'}})", "'abc'.startsWith({[Symbol.match]: true})",
  "'/a/'.includes(Object.assign(/a/, {[Symbol.match]: false}))", "'abc'.includes(Symbol())", "String.prototype.includes.call(null,'a')", "String.prototype.indexOf.call(undefined,'a')");

// ---- 4. codePointAt / fromCodePoint nos limites.
for (const s of [units([0xd83d, 0xde00]), units([0xd83d]), units([0xde00]), units([0xde00, 0xd83d]), units([0xdbff, 0xdfff]), units([0xd800, 0xdc00]), units([0xd800, 0xdbff]), "''", units([0xdbff, 0xe000]), units([0xd7ff, 0xdc00])]) {
  for (const i of ["0", "1", "2", "-1", "NaN", "0.5", "1.9", "Infinity", "'1'", "undefined", "null", "-0"]) add(`${s}.codePointAt(${i})`, `${s}.charCodeAt(${i})`);
}
for (const v of ["0", "-0", "1", "0xd7ff", "0xd800", "0xdbff", "0xdc00", "0xdfff", "0xe000", "0xffff", "0x10000", "0x10ffff", "0x110000", "-1", "1.5", "NaN", "Infinity", "-Infinity", "'65'", "'0x41'",
  "'abc'", "null", "undefined", "true", "[]", "[65]", "[65, 66]", "{}", "1e21", "2**32", "2**32+65", "0x7fffffff", "0xfffffffff", "'1e1'", "''", "' '", "Symbol()", "1n", "{valueOf(){return 66}}"]) {
  add(`String.fromCodePoint(${v})`, `String.fromCodePoint(65, ${v})`, `String.fromCharCode(${v})`, `String.fromCharCode(${v}).length`);
}
add("String.fromCodePoint()", "String.fromCodePoint.length", "String.fromCharCode()", "String.fromCharCode.length", "String.fromCodePoint(0xd83d, 0xde00).length",
  "String.fromCodePoint(0x1f600) === String.fromCharCode(0xd83d, 0xde00)", "String.fromCodePoint(0xd83d, 0xde00) === '\\ud83d\\ude00'", "String.fromCodePoint(...[0x61, 0x62, 0x1f600]).length",
  "String.fromCharCode(0x10041, 0x1ffff, -1, 65536.9)", "String.fromCodePoint.call(null, 65)", "String.fromCharCode.apply(null, new Array(1000).fill(97)).length");

// ---- 5. isWellFormed / toWellFormed.
const wf = [units([0xd83d, 0xde00]), units([0xd83d]), units([0xde00]), units([0xde00, 0xd83d]), units([0x61, 0xd83d]), units([0xd83d, 0x62]), units([0xd83d, 0xd83d, 0xde00]), units([0xd83d, 0xde00, 0xde00]),
  "''", units([0xd800, 0xdbff, 0xdc00]), units([0xdbff, 0xdfff]), units([0xdfff, 0xdbff]), units([0xd7ff, 0xe000]), "'a'.repeat(1000)", "'a'.repeat(1000) + '\\ud800'", "'\\ud800' + 'a'.repeat(1000)"];
for (const s of wf) add(`${s}.isWellFormed()`, `H(${s}.toWellFormed())`, `${s}.toWellFormed().isWellFormed()`, `${s}.toWellFormed().length`, `${s}.toWellFormed() === ${s}`, `encodeURIComponent(${s}.toWellFormed())`);
add("String.prototype.isWellFormed.call(null)", "String.prototype.toWellFormed.call(undefined)", "String.prototype.isWellFormed.call(123)", "String.prototype.toWellFormed.call({toString(){return '\\ud800'}}).charCodeAt(0)",
  "String.prototype.isWellFormed.call([ '\\ud800' ])", "String.prototype.isWellFormed.call(Symbol())", "'a'.isWellFormed.length", "'a'.toWellFormed.name", "'a'.isWellFormed(1,2,3)",
  "new String('\\ud800').isWellFormed()", "Object('\\ud800').toWellFormed().charCodeAt(0)", "'\\ud800'.isWellFormed.call('\\udc00\\ud800')", "Reflect.ownKeys(String.prototype).includes('isWellFormed')");

// ---- 6. localeCompare com acentos.
const words = ["a", "A", "á", "Á", "à", "ä", "â", "ã", "å", "b", "c", "ç", "e", "é", "è", "ê", "ë", "i", "í", "ï", "ı", "İ", "n", "ñ", "o", "ö", "ó", "ô", "õ", "ø", "u", "ü", "ú", "z", "ß", "ss", "SS", "ae", "æ", "œ", "oe", "ij", "ĳ", "á", "é", "é", "ö", "ch", "ll", "ñ", "ñ", "a-b", "ab", "a b", "a_b", "10", "9", "a1", "A1"];
const lcLocales = ["undefined", "'en'", "'sv'", "'de'", "'es'", "'tr'", "'fr-CA'", "'da'", "'pt'", "'de-u-co-phonebk'", "'es-u-co-trad'", "'sv-u-co-reformed'", "'en-u-kn-true'", "'en-u-kf-upper'"];
for (let i = 0; i < words.length; i++) {
  const a = lit(words[i]), b = lit(words[(i * 7 + 3) % words.length]), c = lit(words[(i * 11 + 5) % words.length]);
  for (const loc of lcLocales) add(`${a}.localeCompare(${b}, ${loc})`, `${a}.localeCompare(${c}, ${loc})`);
  add(`${a}.localeCompare(${b}, 'en', {sensitivity:'base'})`, `${a}.localeCompare(${b}, 'en', {sensitivity:'accent'})`, `${a}.localeCompare(${b}, 'en', {sensitivity:'case'})`,
    `${a}.localeCompare(${b}, 'en', {numeric:true})`, `${a}.localeCompare(${c}, 'sv', {sensitivity:'base'})`, `${a}.localeCompare(${a}.normalize('NFD'))`, `${a}.localeCompare(${a}.toUpperCase(), undefined, {caseFirst:'upper'})`);
}
add("'a'.localeCompare()", "'undefined'.localeCompare()", "'a'.localeCompare('b', 'xx-invalid-locale-')", "'a'.localeCompare('b', '')", "'a'.localeCompare('b', 'en', {sensitivity:'x'})",
  "'a'.localeCompare('b', 'en', null)", "'a'.localeCompare('b', 'en', 1)", "['z','ä','a','å','ö'].sort((x,y)=>x.localeCompare(y,'sv')).join()", "['z','ä','a','å','ö'].sort((x,y)=>x.localeCompare(y,'de')).join()",
  "['z','ä','a','å','ö'].sort((x,y)=>x.localeCompare(y,'en')).join()", "['ñ','n','o','nz'].sort((x,y)=>x.localeCompare(y,'es')).join()", "['ñ','n','o','nz'].sort((x,y)=>x.localeCompare(y,'en')).join()",
  "String.prototype.localeCompare.call(null,'a')", "'\\ud800'.localeCompare('\\ud800')", "'\\ud800'.localeCompare('\\udc00')", "'a\\u0000'.localeCompare('a')", "'\\u200b'.localeCompare('')", "'\\u00ad'.localeCompare('')");

// ---- 7. URI: encode/decode, escape/unescape e URIError.
const uriIn = ["a b", "%", "%%", "%2", "%zz", "%e", "%E0", "%E0%A4", "%E0%A4%A", "%E0%A4%A8", "%C0%80", "%C1%BF", "%C2", "%C2%80", "%C2%7f", "%ED%A0%80", "%ED%9F%BF", "%ED%BF%BF", "%EE%80%80",
  "%F0%80%80%80", "%F0%90%80%80", "%F4%8F%BF%BF", "%F4%90%80%80", "%F5%80%80%80", "%F8%88%80%80%80", "%FF", "%80", "%BF", "%7f", "%7F", "%00", "%25", "%2525", "%23", "%3b", "%2F", "%3F", "%3a%2f", "%e4%bd%a0",
  "%E4%BD%A0%E5%A5%BD", "%ud83d", "%u0041", "%U0041", "%f0%9f%98%80", "%F0%9F%98", "%F0%9F%98%0", "%F0%9F%98%G0", "%e0%a4%a8%e0%a4", "a%", "a%2", "%a", "% 41", "%4 1", "%+41", "%-1", "%2e", "%7E", "+", "a+b",
  ";/?:@&=+$,#", "-_.!~*'()", "é", "\u0000", "\u007f", "\u0080", "߿", "ࠀ", "￿", "😀", "퟿", "", "\u{10ffff}", "𐀀", "\u{fffd}", "​", "﻿", "[]{}|\\^`\"<> "];
for (const raw of uriIn) {
  const s = lit(raw);
  for (const fn of ["encodeURI", "encodeURIComponent", "decodeURI", "decodeURIComponent", "escape", "unescape"]) add(`T(() => ${fn}(${s}))`, `T(() => ${fn}(${s}).length)`);
  add(`T(() => decodeURI(${s}) === decodeURIComponent(${s}))`, `T(() => encodeURI(decodeURI(${s})) === ${s})`, `T(() => unescape(escape(${s})) === ${s})`,
    `T(() => decodeURIComponent(encodeURIComponent(${s})) === ${s})`, `T(() => H(decodeURIComponent(${s})))`, `T(() => H(unescape(${s})))`);
}
for (const s of [units([0xd800]), units([0xdc00]), units([0xdbff, 0x41]), units([0x41, 0xdc00]), units([0xdc00, 0xd800]), units([0xd800, 0xd800, 0xdc00]), units([0xd800, 0xdc00, 0xdc00])]) {
  for (const fn of ["encodeURI", "encodeURIComponent", "escape"]) add(`T(() => ${fn}(${s}))`);
  add(`T(() => unescape(escape(${s})) === ${s})`, `T(() => escape(${s}.toWellFormed()))`);
}
add("T(() => encodeURI())", "T(() => decodeURI())", "T(() => encodeURI(undefined))", "T(() => encodeURI(null))", "T(() => encodeURIComponent({}))", "T(() => decodeURIComponent([]))", "T(() => encodeURI(Symbol()))",
  "T(() => decodeURI({toString(){throw new RangeError('x')}}))", "T(() => encodeURI.length)", "T(() => decodeURIComponent.length)", "T(() => escape.length)", "T(() => unescape.name)",
  "T(() => encodeURI.call(null, 'a b'))", "T(() => escape(1.5))", "T(() => escape(-0))", "T(() => unescape('%u004'))", "T(() => unescape('%u00411'))", "T(() => unescape('%u'))", "T(() => unescape('%'))", "T(() => unescape('%4'))",
  "T(() => unescape('%G1'))", "T(() => unescape('%u0g41'))", "T(() => unescape('%u+041'))", "T(() => unescape('%+1'))", "T(() => escape('@*_+-./'))", "T(() => escape('!#$&\\'()=,;:?'))", "T(() => escape('\\u00ff\\u0100'))",
  "T(() => escape('\\ud83d\\ude00'))", "T(() => unescape('%uD83D%uDE00') === '\\ud83d\\ude00')", "T(() => unescape('%E9') === '\\u00e9')", "T(() => decodeURIComponent('%E9'))", "T(() => decodeURI('%23%24%26%2B%2C%2F%3A%3B%3D%3F%40'))",
  "T(() => decodeURIComponent('%23%24%26%2B%2C%2F%3A%3B%3D%3F%40'))", "T(() => decodeURI('%41%42%2b'))", "T(() => encodeURI('http://a.b/c d?e=f&g=h#i j'))", "T(() => encodeURIComponent('http://a.b/c d?e=f&g=h#i j'))",
  "T(() => decodeURI('%25') === '%')", "T(() => decodeURI('%2525'))", "T(() => decodeURI('a'.repeat(5000) + '%'))", "T(() => encodeURIComponent('\\u00e9'.repeat(3000)).length)", "T(() => decodeURIComponent('%C3%A9'.repeat(3000)).length)");

// ---- 8. trim e conversões numéricas com espaços Unicode.
const spaceCps = [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x180e, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x200b, 0x200c, 0x200d,
  0x2028, 0x2029, 0x202f, 0x205f, 0x2060, 0x3000, 0xfeff, 0xfffe, 0x0001, 0x001c, 0x001f, 0x061c, 0x200e, 0x200f, 0x2065, 0x2800, 0x3164, 0xe0020, 0x180b, 0x00ad, 0x034f, 0x115f];
for (const c of spaceCps) {
  const w = cp(c);
  add(`${w}.trim().length`, `${w}.trimStart().length`, `${w}.trimEnd().length`, `('x' + ${w}).trim().length`, `(${w} + 'x').trim().length`, `(${w} + 'x' + ${w}).trimStart().length`, `(${w} + 'x' + ${w}).trimEnd().length`,
    `(${w} + 'x' + ${w}).trim() === 'x'`, `(${w} + 'x' + ${w}).trimLeft === String.prototype.trimStart`, `Number(${w})`, `Number(${w} + '12' + ${w})`, `Number(${w} + '1' + ${w} + '2')`, `+(${w} + '0x1f' + ${w})`,
    `parseFloat(${w} + '1.5' + ${w})`, `parseFloat(${w} + '-.5e1x')`, `parseInt(${w} + '42' + ${w})`, `parseInt(${w} + '0x1f')`, `parseInt(${w} + '-' + ${w} + '1')`, `isNaN(${w} + '7')`, `Number('1' + ${w} + 'e3')`,
    `Number(${w} + 'Infinity')`, `Number(${w} + '-Infinity' + ${w})`, `Number(${w} + '1_0')`, `BigInt(${w} + '12' + ${w})`, `Number(${w} + '0b101')`, `Number(${w} + '0o17')`, `JSON.parse(${w} + '1')`,
    `JSON.parse('[' + ${w} + '1]')`, `'a' + ${w} + 'b'`, `('a' + ${w} + 'b').split(/\\s/).length`, `/^\\s$/.test(${w})`, `/\\s/u.test(${w})`, `/[\\s]/.test(${w})`, `/\\S/.test(${w})`, `/^[\\s\\S]$/.test(${w})`);
}
add("'\\u180e1'.trim().length", "Number('\\u180e1')", "parseFloat('\\u180e1')", "'\\u2028\\u2029\\n\\r x'.trimStart()", "'\\ufeff\\u00a0\\u2003x\\u3000\\u200a'.trim()", "'\\u200bx\\u200b'.trim().length",
  "String.prototype.trim.call(null)", "String.prototype.trimStart.call(undefined)", "String.prototype.trimEnd.call(12)", "'a'.trim.length", "String.prototype.trimLeft.name", "String.prototype.trimRight.name",
  "String.prototype.trimLeft === String.prototype.trimStart", "String.prototype.trimRight === String.prototype.trimEnd", "BigInt('\\u180e1')", "Number('\\u2028 1\\u2029')", "Number('\\u0085 1')", "Number('1\\u0085')");

// ---- 9. Identificadores Unicode.
const idChars = [0x61, 0x5f, 0x24, 0xaa, 0xb5, 0xba, 0xc0, 0xd7, 0xf7, 0x1c5, 0x2b0, 0x370, 0x37e, 0x386, 0x3a3, 0x3c2, 0x5d0, 0x620, 0x660, 0x904, 0x966, 0xe01, 0xe50, 0x1100, 0x1e00, 0x2071, 0x2118, 0x212e, 0x2160,
  0x2c00, 0x3005, 0x3041, 0x30a1, 0x3400, 0x4e00, 0xac00, 0xd7a3, 0xf900, 0xfb1d, 0xff21, 0xff3f, 0xff10, 0x10400, 0x1d400, 0x1d7ce, 0x20000, 0x2fa1d, 0x30000, 0x31350, 0x32000, 0xe0100, 0x1f600, 0x203f, 0x2040,
  0x200c, 0x200d, 0x30, 0x39, 0x300, 0x903, 0x5bf, 0x1dc0, 0x20d0, 0x2e2f, 0x2e, 0x2d, 0xb7, 0x387, 0x19da, 0x1369, 0x180e, 0xfeff, 0x2118, 0x309b, 0x309c, 0x1885, 0x1886, 0x2cef, 0x1e944, 0x16f50, 0x11000];
for (const c of idChars) {
  const ch = String.fromCodePoint(c), L = JSON.stringify(ch), esc = "\\u{" + c.toString(16) + "}", esc4 = c <= 0xffff ? "\\u" + c.toString(16).padStart(4, "0") : null;
  const h = c.toString(16);
  add(`E(${JSON.stringify("var " + ch + " = 1; " + ch)})`, `E(${JSON.stringify("var a" + ch + " = 2; a" + ch)})`, `E(${JSON.stringify("var " + esc + " = 3; " + esc)})`,
    `E(${JSON.stringify("var a" + esc + " = 4; a" + esc)})`, `E(${JSON.stringify("({" + ch + ": 5})." + ch)})`, `E(${JSON.stringify("({a" + ch + ": 6}).a" + ch)})`, `E(${JSON.stringify("({" + esc + ": 7})." + esc)})`,
    `E(${JSON.stringify("x: for (;;) { break x" + ch + "; }")})`, `E(${JSON.stringify(ch + ": { 1; break " + ch + "; }")})`, `E(${JSON.stringify("a" + ch + ": { 1; break a" + ch + "; }")})`,
    `E(${JSON.stringify(esc + ": { 8; break " + esc + "; }")})`, `E(${JSON.stringify("var o = {}; o." + ch + " = 9; Object.keys(o)[0].codePointAt(0)")})`,
    `E(${JSON.stringify("var o = {}; o.a" + ch + " = 9; Object.keys(o)[0].length")})`, `E(${JSON.stringify("class " + ch + " {} " + ch + ".name.length")})`,
    `E(${JSON.stringify("(function " + ch + "(){ return " + ch + ".name })()")})`, `E(${JSON.stringify("({ get " + ch + "() { return 1 } })." + ch)})`, `E(${JSON.stringify("var {" + ch + "} = {" + ch + ": 10}; " + ch)})`,
    `E(${JSON.stringify("(" + ch + " => " + ch + ")(11)")})`, `E(${JSON.stringify("var " + ch + "x; typeof " + ch + "x")})`, `E(${JSON.stringify("let " + ch + " = 1; " + ch + " + 1")})`);
  if (esc4) add(`E(${JSON.stringify("var " + esc4 + " = 12; " + esc4)})`, `E(${JSON.stringify("var a" + esc4 + " = 13; a" + esc4)})`, `E(${JSON.stringify("var a" + esc4 + "b = 14; a" + ch + "b")})`,
    `E(${JSON.stringify("({" + esc4 + ": 15})." + ch)})`);
  add(`E(${JSON.stringify("var \\u{0000" + h + "} = 16; \\u{" + h + "}")})`, `E(${JSON.stringify("var a\\u{" + h + "}; typeof a" + ch)})`, `E(${JSON.stringify("'use strict'; var " + esc + "; 1")})`,
    `E(${JSON.stringify("var o = { " + esc + "() { return 17 } }; o." + ch + "()")})`, `E(${JSON.stringify("var o = { a" + esc + " : 18 }; Object.keys(o)[0].length")})`);
}
add("E('var \\\\u{61} = 1; a')", "E('var \\\\u{110000} = 1')", "E('var \\\\u{} = 1')", "E('var \\\\u{ 61} = 1')", "E('var \\\\u{61 } = 1')", "E('var a\\\\u{0000000000061} = 1; aa')", "E('var \\\\u0061b = 1; ab')",
  "E('var \\\\u00 = 1')", "E('var \\\\u = 1')", "E('var \\\\x61 = 1')", "E('var \\\\u{d800} = 1')", "E('var \\\\ud800 = 1')", "E('var \\\\ud835\\\\udc00 = 1')", "E('var \\\\u0030 = 1')", "E('var a\\\\u0030 = 1; a0')",
  "E('var a\\\\u200c = 1')", "E('var a\\\\u200d = 1')", "E('var a\\\\u00b7 = 1')", "E('var \\\\u0024 = 1; $')", "E('var \\\\u005f = 1; _')", "E('var v\\\\u0061r = 1')", "E('v\\\\u0061r a = 1')", "E('var \\\\u0069f = 1')",
  "E('({ \\\\u0069f: 1 }).if')", "E('({ i\\\\u0066 }).if')", "E('var o = {}; o.\\\\u0069f = 1; o.if')", "E('\\\\u0069f (1) 2')", "E('var l\\\\u0065t = 1; let')", "E('\\'use strict\\'; var l\\\\u0065t = 1')",
  "E('var yi\\\\u0065ld = 1; yield')", "E('\\'use strict\\'; var yi\\\\u0065ld = 1')", "E('async function f(){ var aw\\\\u0061it }')", "E('var \\\\u{1d400} = 1; \\ud835\\udc00')", "E('var \\ud835 = 1')", "E('var \\udc00 = 1')",
  "E('var \\ud835\\udc00 = 1; \\ud835\\udc00')", "E('var a\\ud835\\udc00 = 1')", "E('var a\\ud83d\\ude00 = 1')", "E('var \\u{1f600} = 1')", "E('var a\\u2028 = 1')", "E('var a\\u00a0b = 1')", "E('var a\\u180eb = 1')",
  "E('var \\ufeffa = 1; a')", "E('a\\u200bb')", "E('var \\u00d7 = 1')", "E('var \\u00f7 = 1')", "E('var \\u2118 = 1; \\u2118')", "E('var \\u212e = 1; \\u212e')", "E('var \\u309b = 1')", "E('var a\\u309b = 1; a\\u309b')");

// ---- 10. Escapes `\u` inválidos em strings e templates.
const badEsc = ["\\u", "\\u0", "\\u00", "\\u000", "\\u000g", "\\ug000", "\\u{", "\\u{}", "\\u{0", "\\u{g}", "\\u{110000}", "\\u{10ffff}", "\\u{10FFFF}", "\\u{0000000041}", "\\u{ 41}", "\\u{41 }", "\\u{-1}", "\\u{+1}", "\\u{1_0}",
  "\\u{d800}", "\\u{dfff}", "\\ud800", "\\ud800\\udc00", "\\u{d83d}\\u{de00}", "\\ud83d\\u{de00}", "\\x", "\\x4", "\\xg1", "\\x41", "\\1", "\\01", "\\08", "\\8", "\\9", "\\0", "\\00", "\\377", "\\400", "\\u0041\\u", "a\\u", "\\u\\u0041",
  "\\U0041", "\\U", "\\c", "\\a", "\\ ", "\\ ", "\\\r\n", "\\\n", "\\\r", "\\\r\\\n", "\\ ", "\\v", "\\b", "\\f", "\\z"];
for (const e of badEsc) {
  const body = e;
  add(`E(${JSON.stringify("'" + body + "'")})`, `E(${JSON.stringify('"' + body + '"')})`, `E(${JSON.stringify("'use strict'; '" + body + "'")})`, `E(${JSON.stringify("`" + body + "`")})`,
    `E(${JSON.stringify("(x => x.raw[0])`" + body + "`")})`, `E(${JSON.stringify("(x => x[0])`" + body + "`")})`, `E(${JSON.stringify("(x => x[0] === undefined)`a" + body + "b`")})`, `E(${JSON.stringify("String.raw`" + body + "`")})`,
    `E(${JSON.stringify("(x => x.raw[0].length)`" + body + "`")})`, `E(${JSON.stringify("({ '" + body + "': 1 })")})`, `E(${JSON.stringify("(function(){ 'use strict'; return '" + body + "' })()")})`, `E(${JSON.stringify("`${1}" + body + "`")})`,
    `E(${JSON.stringify("(x => x.length)`" + body + "${1}" + body + "`")})`, `E(${JSON.stringify("/" + body + "/")})`, `E(${JSON.stringify("new RegExp('" + body + "')")})`);
}
add("E('(x => x)`\\\\u{`')", "E('`\\\\u{`')", "E('(x => [x[0], x.raw[0]])`\\\\u{110000}`')", "E('(x => x.raw[0])`\\\\u0g${1}\\\\u{`')", "E('(x => x.length)`\\\\u${1}\\\\xz${2}\\\\01`')",
  "E('`\\\\u{20}`')", "E('`\\\\u{00000020}`')", "E('`\\\\0`')", "E('`\\\\00`')", "E('`\\\\1`')", "E('`\\\\08`')", "E('(x=>x[0])`\\\\0`')", "E('(x=>x[0])`\\\\01`')", "E('(x=>x.raw[0])`\\\\01`')", "E('`${`\\\\u{`}`')",
  "E('(x=>x)`${`\\\\u{`}`')", "E('tag`\\\\u{`')", "E('(function(){ return (x=>x[0])`\\\\u` })()')", "E('(x=>x[0])`\\\\u{d800}`.length')", "E('(x=>x[0])`\\\\ud800`.length')", "E('(x=>x.raw[0])`\\\\ud800`.length')",
  "E('\"\\\\u{0}\" === \"\\\\0\"')", "E('\"\\\\u{000000000000000000041}\"')", "E('\"\\\\u0041\\\\u{42}\\\\x43\"')", "E('\"\\\\u{1F600}\".length')", "E('\"\\\\ud83d\\\\ude00\" === \"\\\\u{1f600}\"')", "E('\"a\\\\\\nb\"')", "E('\"a\\\\\\u2028b\"')");

// ---- Execução com processo fresco, deduplicando contra os goldens de string existentes.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const decode = (s) => { try { return JSON.parse(s); } catch (e) { return s; } };
let baseText = "";
for (const program of knownPrograms("string_unicode_extra_bun.tsv", (name) => !(!/^(string|template|case|text_locale|coercion|global|builtins|number_edge|json)/.test(name) || !name.endsWith(".tsv") || name.startsWith("string_unicode_extra")))) baseText += program + "\n";
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const call = /^(T|E)\(/.test(expr) ? expr : `T(() => ${expr})`;
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + "globalThis.R = " + call + ";" });
}
const runChild = (source) => new Promise((resolve) => {
  const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
  let out = "", err = "";
  child.stdout.setEncoding("utf8"); child.stderr.setEncoding("utf8");
  child.stdout.on("data", (d) => (out += d)); child.stderr.on("data", (d) => (err += d));
  child.on("close", (status) => resolve({ status, out, err }));
  child.stdin.end(source);
});
(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => { for (;;) { const i = next++; if (i >= jobs.length) return; results[i] = await runChild(jobs[i].source); } };
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0, dropped = 0;
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const { expr, source } = jobs[i], r = results[i];
    const result = r.status === 0 ? decodeResult(r.out) : null;
    if (result === null) { dropped++; process.stderr.write("filho falhou: " + JSON.stringify(expr).slice(0, 140) + " " + r.err.slice(0, 120) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[\u2013\u2014]/.test(result + source)) { dropped++; process.stderr.write("resultado inadequado: " + JSON.stringify(expr).slice(0, 140) + "\n"); continue; }
    kept++;
    rows.push({ source, result });
  }
  process.stdout.write(emitFactored("string_unicode_extra", rows));
  process.stderr.write(`gerados ${unique.length}, mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
