// Gera tests/golden/unicode_grid_bun.tsv: grade de Unicode, medida no bun 1.4.2.
// Cobre normalize (NFC/NFD/NFKC/NFKD) em grade de sequências (hangul, combinantes empilhados, singletons,
// compatibilidade, astrais, concatenações), toUpperCase/toLowerCase com casos especiais (ß, ligaduras, İ, ς final,
// Σ contextual, Deseret/Adlam), String.fromCodePoint/codePointAt com limites, surrogates soltos em cada método,
// isWellFormed/toWellFormed, comprimento e indexação de emojis com ZWJ, at() e iteração por code point, e
// encodeURI/decodeURI/encodeURIComponent/decodeURIComponent/escape/unescape com os URIError exatos.
// Todo texto do programa e do resultado sai em ASCII (escapes \uXXXX no fonte; o resultado é hexadecimal de unidades
// de código), então nenhum caractere fora de ASCII (nem travessão) entra no tsv. Resultado com byte fora de ASCII é
// descartado. Programas cujo texto já aparece num golden existente são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo (até 8 em paralelo, timeout de 5 s).
// Uso: bun scripts/gen-unicode-grid-golden.js > tests/golden/unicode_grid_bun.tsv
const fs = require("fs");
const { knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function H(s){var r=[];for(var i=0;i<s.length;i++)r.push(s.charCodeAt(i).toString(16));return r.join(" ")}\n' +
  'function S(v){var t=typeof v;if(t==="string")return "s:"+H(v);if(Array.isArray(v))return "["+v.map(S).join(",")+"]";' +
  'if(t==="number")return Object.is(v,-0)?"-0":""+v;if(t==="symbol")return "sym";if(t==="bigint")return v+"n";if(v&&t==="object")return "o";return ""+v}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

// Constrói um literal de string só com escapes \uXXXX a partir de pontos de código (surrogates soltos permitidos).
const units = (cp) => (cp > 0xffff ? [0xd800 + ((cp - 0x10000) >> 10), 0xdc00 + ((cp - 0x10000) & 0x3ff)] : [cp]);
const hex4 = (n) => n.toString(16).toUpperCase().padStart(4, "0");
const lit = (cps) => '"' + cps.flatMap(units).map((u) => "\\u" + hex4(u)).join("") + '"';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. normalize: grade de sequências x quatro formas.
const norm = [
  // hangul
  [0xac00], [0xd7a3], [0xac01], [0x1100, 0x1161], [0x1100, 0x1161, 0x11a8], [0x1112, 0x1175, 0x11c2], [0x1100, 0x1161, 0x11a8, 0x11a8],
  [0x1100, 0x1100, 0x1161], [0x1161, 0x11a8], [0x11a8], [0x1100], [0xac00, 0x11a8], [0xac01, 0x11a8], [0x1100, 0xac00], [0x1100, 0x1161, 0x1161],
  [0xd788, 0x11c2], [0x3131, 0x314f], [0xffa1, 0xffc2], [0xb098, 0x1100], [0xac00, 0x11a7], [0x1100, 0x1160],
  // combinantes empilhados e reordenação canônica
  [0x61, 0x301], [0x61, 0x301, 0x323], [0x61, 0x323, 0x301], [0x61, 0x300, 0x301, 0x302], [0x61, 0x302, 0x301, 0x300], [0x61, 0x328, 0x301, 0x323],
  [0x1e0b, 0x323], [0x1e0d, 0x307], [0x64, 0x307, 0x323], [0x64, 0x323, 0x307], [0xe9], [0x65, 0x301], [0xc5], [0x41, 0x30a], [0x1fa],
  [0x41, 0x30a, 0x301], [0x41, 0x301, 0x30a], [0x344], [0x308, 0x301], [0x61, 0x344], [0x1ebf], [0xea, 0x301], [0x65, 0x302, 0x301],
  [0x1ea4], [0x41, 0x302, 0x301], [0x1e69], [0x73, 0x323, 0x307], [0x73, 0x307, 0x323], [0x4f, 0x31b, 0x301], [0x4f, 0x301, 0x31b], [0x1edb],
  [0x5d1, 0x5b0, 0x5b4], [0x5d1, 0x5b4, 0x5b0], [0x5e9, 0x5c1, 0x5b8], [0xfb2a], [0x5e9, 0x5c1], [0x627, 0x653], [0x622], [0x649, 0x654], [0x626, 0x649],
  [0xf73], [0xf71, 0xf72], [0xf75], [0xf81], [0xf40, 0xfb5], [0xf69], [0x1b05, 0x1b35], [0x1b06],
  [0x915, 0x93c], [0x958], [0x9c7, 0x9be], [0x9cb], [0xb47, 0xb56], [0xb48], [0xbc6, 0xbbe], [0xbca], [0xd46, 0xd3e], [0xd4a],
  [0x300, 0x61], [0x301], [0x345], [0x3b1, 0x345], [0x1f80], [0x1f00, 0x345], [0x3b1, 0x313, 0x345], [0x1fb3], [0x1fbc],
  // singletons canônicos
  [0x212b], [0x2126], [0x212a], [0x340], [0x341], [0x343], [0x37e], [0x387], [0x1fef], [0x1fee], [0x1ffd], [0x2000], [0x2001], [0x2002], [0x2003],
  [0xf900], [0xfa0d], [0x2f800], [0x2fa1d], [0x2f899], [0x1d15e], [0x1d15f], [0x1d160], [0x1d161], [0x1d1bb], [0x1d1bc], [0x1d1c0], [0x2adc], [0x1d1bf],
  // compatibilidade
  [0xfb01], [0xfb00], [0xfb03], [0xfb06], [0xb2], [0xb3], [0xb9], [0x2460], [0x24b6], [0x3392], [0x33c2], [0xfdfa], [0xfdfb], [0x1d400], [0x1d7d8], [0x1d7ff],
  [0xff21], [0xff41], [0xff10], [0xff9e], [0xa0], [0x2122], [0x2474], [0x2488], [0x132], [0x133], [0x149], [0x17f], [0x1c4], [0x1c5], [0x1c6],
  [0x2126, 0x301], [0xfe10], [0xfe30], [0xfe35], [0x1f100], [0x1f12f], [0x1f200], [0x2160], [0x2179], [0x217f], [0x3250], [0x32cc], [0x3300], [0x33ff],
  [0x2025], [0x2026], [0x2033], [0x2034], [0x2057], [0x2036], [0x203c], [0x2047], [0x2049], [0x20a8], [0x2100], [0x2105], [0x2116], [0x2121], [0x2150],
  [0x2189], [0xff76, 0xff9e], [0x30ab, 0x3099], [0x30ac], [0x3094], [0x30fb], [0x309b], [0x309c], [0x309f], [0x30ff], [0xfa10], [0xfe4d], [0xffe3],
  [0x1e9b, 0x323], [0x1e9b], [0x17f, 0x307], [0x1d2c], [0x1d62], [0x2090], [0x2c7c], [0xa69c], [0x1d78],
  // astrais e sequências com astrais
  [0x10400], [0x10428], [0x1e900], [0x1e922], [0x1d400, 0x301], [0x1d7d8, 0x301], [0x1f600], [0x1f1e7, 0x1f1f7], [0x20000], [0x2f800, 0x301],
  [0x11099, 0x110ba], [0x1109a], [0x11131, 0x11127], [0x1112e], [0x1d157, 0x1d165], [0x1d15e, 0x1d16e], [0x10ffff], [0x1fbf0],
  // sequências mistas e surrogates soltos
  [0x41, 0x30a, 0xac00], [0x61, 0x301, 0x1100, 0x1161], [0xd800], [0xdc00], [0x61, 0xd800, 0x301], [0xd800, 0x61], [0xdc00, 0xd800], [0xfb01, 0xd800],
  [0x212b, 0xdc00], [0x20, 0x308], [0x20, 0x301], [0x20, 0x3099], [0xa8], [0xaf], [0xb4], [0xb8], [0x2dc], [0x385], [0x1fbf], [0x1fc0], [0x1fc1],
  [0x0], [0x7f], [0x80], [0x9f], [0xad], [0xfeff], [0x200b], [0x200c], [0x200d], [0x2060], [0xfffd], [0xfffe],
];
const forms = ["NFC", "NFD", "NFKC", "NFKD"];
for (const seq of norm) for (const f of forms) add(`T(()=>${lit(seq)}.normalize("${f}"))`);
// idempotência e coerência entre formas
for (const seq of norm.slice(0, 120)) {
  add(`T(()=>{var s=${lit(seq)};return [s.normalize()===s.normalize("NFC"),s.normalize("NFD").normalize("NFC")===s.normalize("NFC"),s.normalize("NFKC")===s.normalize("NFKD").normalize("NFC"),s.normalize("NFD").length,s.normalize("NFKD").length]})`);
}
// concatenações: reordenação canônica e composição entre vizinhos
const catA = [[0x61], [0x61, 0x301], [0x61, 0x323], [0x1100], [0x1100, 0x1161], [0xac00], [0x41, 0x30a], [0x20], [0xfb01], [0xe9], [0x5d1], [0x3b1], [0x1d400], [0xd800], [0x4f, 0x31b], [0x73]];
const catB = [[0x301], [0x323], [0x1161], [0x11a8], [0x308, 0x301], [0x345], [0x300, 0x323], [0x61], [0x5b0], [0xdc00], [0x31b], [0x307, 0x323], [0x3099], [0xf71, 0xf72], [0x302, 0x300]];
for (const a of catA) for (const b of catB) for (const f of forms) add(`T(()=>(${lit(a)}+${lit(b)}).normalize("${f}"))`);
// argumento de forma: valores válidos e inválidos
const formArgs = ['"NFC"', '"NFD"', '"NFKC"', '"NFKD"', '"nfc"', '"NFc"', '"NFKC "', '" NFC"', '""', '"NF"', '"NFE"', "undefined", "null", "0", "1", "true", "{}", "[]", "[\"NFD\"]", "{toString(){return 'NFD'}}", "{toString(){return 'bad'}}", "{toString(){throw new RangeError('k')}}", "Symbol()", "1n", "NaN"];
for (const fa of formArgs) {
  add(`T(()=>${lit([0xe9])}.normalize(${fa}))`, `T(()=>${lit([0xfb01])}.normalize(${fa}))`);
}
add(
  "T(()=>String.prototype.normalize.call(null))", "T(()=>String.prototype.normalize.call(undefined,'NFC'))", "T(()=>String.prototype.normalize.call(123))",
  "T(()=>String.prototype.normalize.call({toString(){return '\\u00e9'}},'NFD'))", "T(()=>String.prototype.normalize.call([1,2],'NFKD'))", "T(()=>String.prototype.normalize.call(Symbol()))",
  "T(()=>String.prototype.normalize.length+String.prototype.normalize.name)", "T(()=>''.normalize('NFD'))", "T(()=>''.normalize())", "T(()=>'abc'.normalize('NFKC'))",
  "T(()=>'\\u00e9'.normalize('NFD', 'NFC'))", "T(()=>new String('\\u0065\\u0301').normalize())",
);

// ---- 2. toUpperCase / toLowerCase e variantes locais.
const caseStrs = [
  [0xdf], [0x1e9e], [0x73, 0x73], [0xfb00], [0xfb01], [0xfb02], [0xfb03], [0xfb04], [0xfb05], [0xfb06], [0x149], [0x1f0], [0x390], [0x3b0], [0x587], [0x1e96], [0x1e97], [0x1e98], [0x1e99], [0x1e9a],
  [0x130], [0x69, 0x307], [0x131], [0x49], [0x69], [0x49, 0x307], [0x130, 0x130], [0x41, 0x130], [0x130, 0x41], [0x49, 0x323, 0x307], [0x4a, 0x307],
  [0x3c2], [0x3c3], [0x3a3], [0x41, 0x3a3], [0x41, 0x3a3, 0x42], [0x391, 0x3a3], [0x391, 0x3a3, 0x2e], [0x391, 0x3a3, 0x20, 0x391], [0x3a3, 0x391], [0x3a3, 0x3a3], [0x391, 0x3a3, 0x3a3],
  [0x391, 0x3a3, 0x301], [0x391, 0x3a3, 0x345], [0x41, 0x2e, 0x3a3], [0x41, 0x3a3, 0x2e, 0x42], [0x41, 0x20, 0x3a3], [0x3a3, 0x20, 0x41], [0x41, 0x301, 0x3a3], [0x41, 0x3a3, 0x301, 0x42],
  [0x41, 0x3a3, 0x2019], [0x41, 0x2019, 0x3a3], [0x41, 0x3a3, 0xad, 0x42], [0x41, 0xad, 0x3a3], [0x391, 0x3a3, 0xd83d, 0xde00], [0x391, 0x3a3, 0x1d400], [0x1d400, 0x3a3], [0x391, 0x3a3, 0x10400],
  [0x10400, 0x3a3], [0x391, 0x3a3, 0xdc00], [0x391, 0x3a3, 0xd800], [0x3a3, 0x3a3, 0x391], [0x391, 0x3a3, 0x3a3, 0x391], [0x391, 0x3a3, 0x3c2], [0x3c2, 0x3a3], [0x1f88], [0x1f80], [0x1fb3], [0x1fbc], [0x1fc3], [0x1fcc], [0x1ff3], [0x1ffc],
  [0x1f50], [0x1f52], [0x1fb6], [0x1fb7], [0x1fc6], [0x1fd6], [0x1fe4], [0x1fe6], [0x1ff7], [0x1fb2], [0x1f88, 0x1f89],
  [0x10400], [0x10428], [0x10400, 0x10428], [0x1e900], [0x1e922], [0x1e900, 0x1e922], [0x10c80], [0x10cc0], [0x118a0], [0x118c0], [0x16e40], [0x16e60], [0x1c90], [0x10d0], [0x10a0], [0x2d00], [0x13a0], [0xab70], [0x13f8], [0x13f0],
  [0x1c4], [0x1c5], [0x1c6], [0x1c7], [0x1c8], [0x1c9], [0x1ca], [0x1cb], [0x1cc], [0x1f1], [0x1f2], [0x1f3], [0x212a], [0x212b], [0x2126], [0x6b], [0x4b], [0xe5], [0xc5], [0x3a9],
  [0x2160], [0x2170], [0x24b6], [0x24d0], [0xff21], [0xff41], [0x1d400], [0x1d41a], [0xa7ae], [0x26a], [0xa78d], [0x265], [0x266], [0x1e9b], [0x17f], [0x3d0], [0x3d1], [0x3d5], [0x3d6], [0x3f0], [0x3f1], [0x3f4], [0x3f5], [0x1c80],
  [0x1fd3], [0x1fe3], [0x1fd2], [0x1fe2], [0xdf, 0xdf], [0x61, 0xdf, 0x62], [0xfb01, 0xdf], [0x7a, 0xdf], [0x131, 0x69], [0x1e9e, 0x73],
  [0xd800], [0xdc00], [0xd800, 0xdf, 0xdc00], [0x61, 0xd800, 0x62], [0xd83d], [0xde00], [0xd83d, 0xde00], [0xd801, 0xdc00], [0xd801], [0xdc28], [0xd801, 0x61], [0x61, 0xdc00, 0xd801], [0xdc00, 0xd801, 0xdc00],
  [0x41, 0x42, 0x43], [0x61, 0x62, 0x63], [0x20], [0x0], [0x7f], [0xa0], [0x180e], [0x200b], [0x2028], [0xfeff],
];
for (const s of caseStrs) {
  const L = lit(s);
  add(`T(()=>${L}.toUpperCase())`, `T(()=>${L}.toLowerCase())`, `T(()=>${L}.toLocaleUpperCase())`, `T(()=>${L}.toLocaleLowerCase())`);
  add(`T(()=>${L}.toUpperCase().length-${L}.length)`, `T(()=>${L}.toLowerCase().toUpperCase())`);
}
// idiomas com regras próprias
const localeStrs = [[0x69], [0x49], [0x130], [0x131], [0x69, 0x307], [0x49, 0x307], [0x49, 0x323, 0x307], [0xdf], [0x6a], [0x4a], [0x12e], [0x12f], [0xcc], [0xcd], [0x128], [0x49, 0x300], [0x49, 0x301], [0x49, 0x303], [0x69, 0x300], [0x41, 0x49, 0x42]];
for (const s of localeStrs) for (const loc of ['"tr"', '"az"', '"lt"', '"en"', '"und"', '"tr-TR"', '"de"', '"el"', '"nl"', '["tr","en"]']) {
  add(`T(()=>${lit(s)}.toLocaleUpperCase(${loc}))`, `T(()=>${lit(s)}.toLocaleLowerCase(${loc}))`);
}
for (const loc of ['"x"', '"en_US"', '"123"', '""', "null", "{}", "[]", "[1]", "1", "[\"tr\",1]", "\"en-\"", "\"i-klingon\"", "[\"en\",\"en\"]", "\"EN\"", "\"tR\"", "undefined"]) {
  add(`T(()=>${lit([0x49])}.toLocaleLowerCase(${loc}))`, `T(()=>${lit([0x69])}.toLocaleUpperCase(${loc}))`);
}
add(
  "T(()=>String.prototype.toUpperCase.call(null))", "T(()=>String.prototype.toLowerCase.call(undefined))", "T(()=>String.prototype.toUpperCase.call(1.5))", "T(()=>String.prototype.toLowerCase.call(true))",
  "T(()=>String.prototype.toUpperCase.call({toString(){return 'ab\\u00df'}}))", "T(()=>String.prototype.toLocaleUpperCase.call(null))", "T(()=>String.prototype.toUpperCase.call(Symbol()))",
  "T(()=>[String.prototype.toUpperCase.length,String.prototype.toLocaleLowerCase.length,String.prototype.toUpperCase.name,String.prototype.toLocaleUpperCase.name])",
  "T(()=>'a\\u00dfb'.toUpperCase('tr'))", "T(()=>'I'.toLowerCase('tr'))",
);

// ---- 3. fromCodePoint / codePointAt / charCodeAt / fromCharCode com limites.
const cpArgs = ["0", "1", "0x41", "0x7f", "0x80", "0xd7ff", "0xd800", "0xdbff", "0xdc00", "0xdfff", "0xe000", "0xfffe", "0xffff", "0x10000", "0x1f600", "0x10fffe", "0x10ffff", "0x110000", "0x7fffffff", "0xffffffff", "0x100000041", "2**32+65",
  "-1", "-0", "1.5", "65.0", "NaN", "Infinity", "-Infinity", "'65'", "'0x41'", "'0b101'", "' 65 '", "''", "'x'", "'1e1'", "null", "undefined", "true", "false", "{}", "[]", "[65]", "[65,66]", "{valueOf(){return 66}}", "{valueOf(){throw new RangeError('k')}}",
  "{toString(){return '67'}}", "1n", "Symbol()", "1e21", "5e-324", "0.5", "-0.5", "1114111.0", "1114111.5", "1114112"];
for (const a of cpArgs) add(`T(()=>String.fromCodePoint(${a}))`, `T(()=>String.fromCodePoint(${a}).length)`);
const cpPairs = ["0x41", "0xd83d", "0xde00", "0x1f600", "0x110000", "-1", "1.5", "NaN", "'x'", "0xdc00", "0x10ffff"];
for (const a of cpPairs) for (const b of cpPairs) add(`T(()=>String.fromCodePoint(${a},${b}))`);
add(
  "T(()=>String.fromCodePoint())", "T(()=>String.fromCodePoint().length)", "T(()=>String.fromCodePoint(0x41,0x42,0x43,0x44,0x1f600,0x45))", "T(()=>String.fromCodePoint(...[0x61,0x62,0x1f600]))",
  "T(()=>String.fromCodePoint.length+String.fromCodePoint.name)", "T(()=>String.fromCodePoint(0xd83d,0xde00)===String.fromCodePoint(0x1f600))", "T(()=>String.fromCodePoint(0xd83d,0xde00).length)",
  "T(()=>{var log=[];try{String.fromCodePoint({valueOf(){log.push('a');return 65}},{valueOf(){log.push('b');return -1}},{valueOf(){log.push('c');return 66}}})}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>String.fromCodePoint.call(null,65))", "T(()=>new String.fromCodePoint(65))", "T(()=>String.fromCharCode(0x1f600))", "T(()=>String.fromCharCode(0x10041))", "T(()=>String.fromCharCode(-1))", "T(()=>String.fromCharCode(65.9))",
  "T(()=>String.fromCharCode(0x1ffff,0xd83d,0xde00))", "T(()=>String.fromCharCode('65','x'))", "T(()=>String.fromCharCode(NaN,Infinity,null,undefined))", "T(()=>String.fromCharCode(2**32+65))", "T(()=>String.fromCharCode())",
  "T(()=>String.fromCharCode.length+String.fromCharCode.name)", "T(()=>String.fromCharCode(...[0xd83d,0xde00]).codePointAt(0))",
);
const cpStrs = [[], [0x61], [0xd800], [0xdc00], [0xd800, 0xdc00], [0xdc00, 0xd800], [0xd83d, 0xde00], [0x61, 0xd83d, 0xde00, 0x62], [0xd83d, 0x61], [0x61, 0xde00], [0x10ffff], [0x10000], [0xd7ff, 0xe000], [0xd800, 0xd800, 0xdc00]];
const idxs = ["0", "1", "2", "3", "4", "-1", "-0", "1.5", "1.9", "NaN", "undefined", "null", "'1'", "'x'", "Infinity", "-Infinity", "2**32", "2**32+1", "true", "{valueOf(){return 1}}", "[]", "[1]", "()=>1"];
for (const s of cpStrs) for (const i of idxs) add(`T(()=>${lit(s)}.codePointAt(${i}))`);
for (const s of cpStrs) for (const i of ["0", "1", "2", "-1", "1.5", "NaN", "undefined", "Infinity"]) add(`T(()=>${lit(s)}.charCodeAt(${i}))`, `T(()=>${lit(s)}.charAt(${i}))`);
add(
  "T(()=>'abc'.codePointAt())", "T(()=>String.prototype.codePointAt.call(null,0))", "T(()=>String.prototype.codePointAt.call(undefined))", "T(()=>String.prototype.codePointAt.call(123,1))",
  "T(()=>String.prototype.codePointAt.call({toString(){return '\\ud83d\\ude00'}},0))", "T(()=>String.prototype.codePointAt.length+String.prototype.codePointAt.name)", "T(()=>'abc'.codePointAt(Symbol()))",
  "T(()=>'abc'.codePointAt({valueOf(){throw new RangeError('k')}}))", "T(()=>{var n=0;'a'.codePointAt({valueOf(){n++;return 0}});return n})", "T(()=>String.prototype.codePointAt.call(null,{valueOf(){throw new RangeError('k')}}))",
);

// ---- 4. surrogates soltos em cada método.
const lone = [[0xd800], [0xdbff], [0xdc00], [0xdfff], [0x61, 0xd800], [0xd800, 0x61], [0xdc00, 0xd800], [0xd800, 0xd800, 0xdc00], [0xd83d, 0xde00], [0xd83d], [0xde00], [0x61, 0xd83d, 0xde00, 0x62], [0xd83d, 0xde00, 0xd83d], [0xdc00, 0xdc00], [0xd800, 0xd800], [0x61, 0xdc00, 0x62], [0xd83d, 0x61, 0xde00], [0x10000, 0xd800], [0xdbff, 0xdfff]];
const loneMethods = [
  (L) => `${L}.length`, (L) => `${L}.split("")`, (L) => `[...${L}]`, (L) => `Array.from(${L})`, (L) => `[...${L}].length`, (L) => `${L}.at(0)`, (L) => `${L}.at(-1)`, (L) => `${L}.codePointAt(0)`, (L) => `${L}.codePointAt(1)`,
  (L) => `${L}.slice(0,1)`, (L) => `${L}.slice(1)`, (L) => `${L}.substring(1,2)`, (L) => `${L}.substr(-1)`, (L) => `${L}.charAt(1)`, (L) => `${L}.indexOf("\\uDC00")`, (L) => `${L}.indexOf("\\uD800")`, (L) => `${L}.lastIndexOf("\\uD83D")`,
  (L) => `${L}.includes("\\uDE00")`, (L) => `${L}.startsWith("\\uD83D")`, (L) => `${L}.endsWith("\\uDE00")`, (L) => `${L}.repeat(2)`, (L) => `${L}.concat("\\uDC00")`, (L) => `"\\uD800".concat(${L})`, (L) => `${L}.padStart(4,"\\uD83D")`,
  (L) => `${L}.padEnd(4,"\\uDE00")`, (L) => `${L}.localeCompare("\\uD800")`, (L) => `${L}.normalize("NFD")`, (L) => `${L}.toUpperCase()`, (L) => `${L}.toLowerCase()`, (L) => `JSON.stringify(${L})`, (L) => `JSON.stringify([${L}])`,
  (L) => `JSON.stringify({[${L}]:1})`, (L) => `${L}.isWellFormed()`, (L) => `${L}.toWellFormed()`, (L) => `${L}.trim()`, (L) => `${L}.split(/(?:)/u)`, (L) => `${L}.split(/(?:)/)`, (L) => `${L}.match(/./gu)`, (L) => `${L}.match(/./g)`,
  (L) => `${L}.match(/[\\uD800-\\uDFFF]/gu)`, (L) => `${L}.match(/[\\uD800-\\uDFFF]/g)`, (L) => `${L}.replace(/./gu,"x")`, (L) => `${L}.replace(/./g,"x")`, (L) => `${L}.replace(/\\uD800/u,"x")`, (L) => `${L}.replace(/\\uD83D/,"x")`,
  (L) => `${L}.replaceAll("\\uD83D","x")`, (L) => `${L}.replaceAll("","-")`, (L) => `${L}.replace("","-")`, (L) => `/^.$/u.test(${L})`, (L) => `/^.$/.test(${L})`, (L) => `/^[^]$/u.test(${L})`, (L) => `/\\p{Cs}/u.test(${L})`,
  (L) => `/^\\p{Any}*$/u.test(${L})`, (L) => `${L}.search(/\\uDE00/)`, (L) => `${L}.search(/\\uDE00/u)`, (L) => `${L}.search(/(?<!\\uD83D)\\uDE00/)`, (L) => `[...${L}.matchAll(/./gu)].length`, (L) => `escape(${L})`,
  (L) => `[...${L}].map(c=>c.length)`, (L) => `[...${L}.entries?${L}.split(""):[]].length`, (L) => `Array.from(${L},c=>c.codePointAt(0))`, (L) => `Object.keys(${L})`, (L) => `Object.getOwnPropertyNames(${L}).length`,
  (L) => `${L}.split("",1)`, (L) => `${L}.split("\\uD83D")`, (L) => `${L}.split("\\uDE00")`, (L) => `${L}[0]`, (L) => `${L}[1]`, (L) => `${L}===${L}.toWellFormed()`, (L) => `new Set(${L}).size`, (L) => `${L}<"\\uD800"`, (L) => `${L}>"\\uFFFF"`,
  (L) => `${L}.localeCompare(${L}.toWellFormed())`, (L) => `String.raw({raw:[${L},${L}]},1)`, (L) => `[${L}].join("\\uD83D")`, (L) => `JSON.parse(JSON.stringify(${L}))`, (L) => `encodeURI(${L})`, (L) => `encodeURIComponent(${L})`,
  (L) => `decodeURI(encodeURI(${L}.toWellFormed()))`, (L) => `unescape(escape(${L}))===${L}`, (L) => `Symbol(${L}).description`, (L) => `({[${L}]:1})[${L}]`, (L) => `Object.keys({[${L}]:1})[0]`,
  (L) => `new RegExp(${L},"u").source.length`, (L) => `new RegExp(${L}).source.length`, (L) => `${L}.codePointAt(${L}.length-1)`, (L) => `${L}.lastIndexOf("")`, (L) => `${L}.indexOf("",5)`,
];
for (const s of lone) for (const m of loneMethods) add(`T(()=>${m(lit(s))})`);

// ---- 5. isWellFormed / toWellFormed.
const wf = [[], [0x61], [0x1f600], [0xd800], [0xdc00], [0xd800, 0xdc00], [0xdc00, 0xd800], [0xd800, 0x61], [0x61, 0xdc00], [0xd800, 0xd800], [0xdc00, 0xdc00], [0xd800, 0xd800, 0xdc00], [0xd800, 0xdc00, 0xdc00], [0xdbff, 0xdfff], [0xdbff, 0xdc00], [0xd7ff, 0xdc00], [0xd800, 0xe000],
  [0x61, 0xd83d, 0xde00, 0x62, 0xd83d], [0xdfff, 0xd800], [0xfffd], [0xd800, 0xdb00, 0xdc00], [0x10ffff, 0xd800]];
for (const s of wf) add(`T(()=>${lit(s)}.isWellFormed())`, `T(()=>${lit(s)}.toWellFormed())`, `T(()=>${lit(s)}.toWellFormed().isWellFormed())`, `T(()=>${lit(s)}.toWellFormed().length)`, `T(()=>${lit(s)}.toWellFormed()===${lit(s)})`);
for (const r of ["null", "undefined", "123", "true", "1n", "{}", "[]", "[0xd800]", "Symbol()", "{toString(){return '\\ud800'}}", "{toString(){return 'ok'}}", "{toString(){throw new RangeError('k')}}", "new String('\\udc00')", "Object('\\ud83d\\ude00')", "()=>1", "NaN", "-0"]) {
  add(`T(()=>String.prototype.isWellFormed.call(${r}))`, `T(()=>String.prototype.toWellFormed.call(${r}))`);
}
add(
  "T(()=>[String.prototype.isWellFormed.length,String.prototype.isWellFormed.name,String.prototype.toWellFormed.length,String.prototype.toWellFormed.name])", "T(()=>'\\ud800'.isWellFormed(1,2,3))", "T(()=>new String.prototype.isWellFormed())",
  "T(()=>Object.getOwnPropertyDescriptor(String.prototype,'isWellFormed').enumerable)", "T(()=>Object.getOwnPropertyDescriptor(String.prototype,'toWellFormed').writable)", "T(()=>'a\\ud800b\\udc00c'.toWellFormed().split('').map(c=>c.charCodeAt(0)))",
  "T(()=>'\\ud800'.toWellFormed().codePointAt(0))", "T(()=>[...'\\ud800\\ud800\\udc00'.toWellFormed()].length)", "T(()=>typeof 'x'.toWellFormed())",
);

// ---- 6. comprimento e indexação de emojis com ZWJ.
const emoji = {
  family: [0x1f468, 0x200d, 0x1f469, 0x200d, 0x1f467, 0x200d, 0x1f466], couple: [0x1f469, 0x200d, 0x2764, 0xfe0f, 0x200d, 0x1f48b, 0x200d, 0x1f468], rainbow: [0x1f3f3, 0xfe0f, 0x200d, 0x1f308],
  flagBR: [0x1f1e7, 0x1f1f7], flagTwo: [0x1f1e7, 0x1f1f7, 0x1f1fa, 0x1f1f8], flagOdd: [0x1f1e7, 0x1f1f7, 0x1f1fa], flagEng: [0x1f3f4, 0xe0067, 0xe0062, 0xe0065, 0xe006e, 0xe0067, 0xe007f], skin: [0x1f44d, 0x1f3fd],
  keycap: [0x31, 0xfe0f, 0x20e3], keycap2: [0x23, 0x20e3], heartVs: [0x2764, 0xfe0f], heartText: [0x2764, 0xfe0e], zwjOnly: [0x200d], zwjTwo: [0x200d, 0x200d], leadZwj: [0x200d, 0x1f600], trailZwj: [0x1f600, 0x200d],
  smile: [0x1f600], woman: [0x1f469, 0x1f3fb, 0x200d, 0x1f4bb], man: [0x1f468, 0x200d, 0x1f9b0], handshake: [0x1f91d, 0x1f3fc], ninja: [0x1f977], pirate: [0x1f3f4, 0x200d, 0x2620, 0xfe0f], bear: [0x1f43b, 0x200d, 0x2744, 0xfe0f],
  eye: [0x1f441, 0xfe0f, 0x200d, 0x1f5e8, 0xfe0f], run: [0x1f3c3, 0x1f3fe, 0x200d, 0x2640, 0xfe0f], hearts: [0x2764, 0x2764, 0xfe0f], mix: [0x61, 0x1f600, 0x62, 0x200d, 0x63], cafe: [0x63, 0x61, 0x66, 0x65, 0x301],
  hangul: [0x1100, 0x1161, 0x11a8], deva: [0x915, 0x94d, 0x937, 0x93f], thai: [0xe01, 0xe33], crlf: [0x0d, 0x0a], tag: [0xe0067, 0xe007f], vsOnly: [0xfe0f], astralPair: [0x10000, 0x10ffff], loneFlag: [0xd83c, 0xdde7],
};
for (const [name, cps] of Object.entries(emoji)) {
  const L = lit(cps);
  add(
    `T(()=>${L}.length)`, `T(()=>[...${L}].length)`, `T(()=>Array.from(${L}).length)`, `T(()=>${L}.split("").length)`, `T(()=>[...${L}].map(c=>c.length))`, `T(()=>[...${L}].map(c=>c.codePointAt(0)))`,
    `T(()=>${L}.at(0))`, `T(()=>${L}.at(1))`, `T(()=>${L}.at(-1))`, `T(()=>${L}.at(-2))`, `T(()=>${L}.at(${cps.flatMap(units).length}))`, `T(()=>${L}.charAt(1))`, `T(()=>${L}.codePointAt(0))`, `T(()=>${L}.codePointAt(1))`, `T(()=>${L}.codePointAt(2))`,
    `T(()=>${L}.slice(0,1))`, `T(()=>${L}.slice(0,2))`, `T(()=>${L}.slice(-1))`, `T(()=>${L}.slice(-2))`, `T(()=>${L}.substring(1))`, `T(()=>${L}.substr(1,2))`, `T(()=>${L}.split("\\u200D"))`, `T(()=>${L}.split("\\u200D").length)`,
    `T(()=>${L}.match(/./gu).length)`, `T(()=>${L}.match(/./g).length)`, `T(()=>${L}.match(/\\p{Emoji}/gu))`, `T(()=>${L}.match(/\\p{Extended_Pictographic}/gu)?.length)`, `T(()=>${L}.match(/\\p{Emoji_Presentation}/gu)?.length)`,
    `T(()=>${L}.match(/\\p{Regional_Indicator}/gu)?.length)`, `T(()=>/^\\p{RGI_Emoji}$/v.test(${L}))`, `T(()=>${L}.match(/\\p{M}/gu)?.length)`, `T(()=>[...${L}.matchAll(/[\\u200D\\uFE0F]/g)].length)`, `T(()=>${L}.replace(/\\u200D/g,"+"))`,
    `T(()=>${L}.normalize("NFD").length)`, `T(()=>${L}.normalize("NFKC").length)`, `T(()=>${L}.toUpperCase().length)`, `T(()=>${L}.toLowerCase()===${L})`, `T(()=>${L}.isWellFormed())`, `T(()=>${L}.indexOf("\\u200D"))`,
    `T(()=>${L}.lastIndexOf("\\u200D"))`, `T(()=>${L}.padEnd(${cps.flatMap(units).length + 2},"x"))`, `T(()=>${L}.repeat(2).length)`, `T(()=>[...${L}].reverse().join("").length)`, `T(()=>${L}.split("").reverse().join("").isWellFormed())`,
    `T(()=>encodeURIComponent(${L}))`, `T(()=>escape(${L}))`, `T(()=>JSON.stringify(${L}))`, `T(()=>[...new Intl.Segmenter("en",{granularity:"grapheme"}).segment(${L})].length)`,
    `T(()=>[...new Intl.Segmenter("en",{granularity:"grapheme"}).segment(${L})].map(x=>x.segment.length))`, `T(()=>${L}.localeCompare(${L}.normalize("NFD")))`, `T(()=>${L}.trim().length)`, `T(()=>${L}.search(/\\u200D/))`,
  );
}
add("T(()=>'\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67\\u200d\\ud83d\\udc66'.length)", "T(()=>[...'\\ud83c\\uddE7\\ud83c\\uddF7'].length)");

// ---- 7. at() e iteração por code point.
const atStrs = [[], [0x61], [0x61, 0x62, 0x63], [0xd83d, 0xde00], [0x61, 0xd83d, 0xde00, 0x62], [0xd800], [0xdc00, 0xd800], [0x1f468, 0x200d, 0x1f469], [0x10ffff, 0x0], [0xd83d, 0x61, 0xde00]];
const atIdx = ["0", "1", "2", "3", "4", "5", "-1", "-2", "-3", "-4", "-5", "-6", "1.5", "-1.5", "NaN", "undefined", "null", "'1'", "'-1'", "'x'", "Infinity", "-Infinity", "2**32", "-(2**32)", "true", "{valueOf(){return -1}}", "[]", "[2]", "-0", "0.9", "-0.9"];
for (const s of atStrs) for (const i of atIdx) add(`T(()=>${lit(s)}.at(${i}))`);
add(
  "T(()=>String.prototype.at.call(null,0))", "T(()=>String.prototype.at.call(undefined))", "T(()=>String.prototype.at.call(123,-1))", "T(()=>String.prototype.at.call({toString(){return 'xyz'}},-1))", "T(()=>String.prototype.at.length+String.prototype.at.name)",
  "T(()=>'abc'.at(Symbol()))", "T(()=>'abc'.at(1n))", "T(()=>'abc'.at({valueOf(){throw new RangeError('k')}}))", "T(()=>'abc'.at())", "T(()=>new String('abc').at(-1))",
);
for (const s of atStrs) {
  const L = lit(s);
  add(
    `T(()=>{var r=[];for(var c of ${L})r.push(c.codePointAt(0));return r})`, `T(()=>{var r=[];for(var c of ${L})r.push(c.length);return r})`, `T(()=>[...${L}].map(c=>c.codePointAt(0)))`, `T(()=>Array.from(${L},(c,i)=>c.length+":"+i))`,
    `T(()=>[...${L}[Symbol.iterator]()].length)`, `T(()=>[...${L}].join("")===${L})`, `T(()=>{var it=${L}[Symbol.iterator]();var a=it.next();var b=it.next();return [a.done,a.value,b.done,b.value]})`,
    `T(()=>{var it=${L}[Symbol.iterator]();var r=[];var x;while(!(x=it.next()).done)r.push(x.value);var y=it.next();return [r.length,y.done,y.value]})`, `T(()=>Object.prototype.toString.call(${L}[Symbol.iterator]()))`,
    `T(()=>{var it=${L}[Symbol.iterator]();return it[Symbol.iterator]()===it})`, `T(()=>${L}.split("").map(c=>c.codePointAt(0)))`, `T(()=>{var n=0;for(var i=0;i<${L}.length;i++)if(${L}.codePointAt(i)>0xffff)n++;return n})`,
    `T(()=>Object.entries(${L}).length)`, `T(()=>new Set(${L}).size)`, `T(()=>new Map(Object.entries(${L})).size)`, `T(()=>Math.max(...${L}.split("").map(c=>c.charCodeAt(0)),0))`, `T(()=>{var [a,b,...c]=${L};return [a,b,c.length]})`,
    `T(()=>{var [a,b,...c]=${L};return [a,b,c]})`, `T(()=>${L}.at(0)===${L}[0])`, `T(()=>${L}.at(-1)===${L}[${L}.length-1])`,
  );
}
add(
  "T(()=>Object.getPrototypeOf(''[Symbol.iterator]())[Symbol.toStringTag])", "T(()=>Object.getOwnPropertyNames(Object.getPrototypeOf(''[Symbol.iterator]())))", "T(()=>String.prototype[Symbol.iterator].name+String.prototype[Symbol.iterator].length)",
  "T(()=>String.prototype[Symbol.iterator].call(null))", "T(()=>String.prototype[Symbol.iterator].call(undefined))", "T(()=>[...String.prototype[Symbol.iterator].call(123)])", "T(()=>[...String.prototype[Symbol.iterator].call({toString(){return '\\ud83d\\ude00x'}})].length)",
  "T(()=>{var it=''[Symbol.iterator]();return typeof it.next+typeof it.return})", "T(()=>Object.getPrototypeOf(Object.getPrototypeOf(''[Symbol.iterator]()))===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())))",
  "T(()=>{var it=''[Symbol.iterator]();return it.next.call({})})", "T(()=>{var it='a'[Symbol.iterator]();return it.next.call([][Symbol.iterator]())})", "T(()=>{var s=new String('\\ud83d\\ude00');s[Symbol.iterator]=function*(){yield 'z'};return [...s]})",
  "T(()=>{var s='ab';var it=s[Symbol.iterator]();String.prototype[Symbol.iterator]=function(){throw 1};var r=[...it].length;return r})",
);

// ---- 8. encodeURI, decodeURI, encodeURIComponent, decodeURIComponent, escape, unescape.
const uriFns = ["encodeURI", "encodeURIComponent", "escape"];
for (let cp = 0; cp < 0x80; cp++) for (const fn of uriFns) add(`T(()=>${fn}(${lit([cp])}))`);
const uriCps = [0x80, 0xa0, 0xff, 0x100, 0x7ff, 0x800, 0xfff, 0xffff, 0xd7ff, 0xe000, 0xfffd, 0x10000, 0x1f600, 0x10ffff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0x20ac, 0x212b, 0x3042, 0x0301];
for (const cp of uriCps) for (const fn of ["encodeURI", "encodeURIComponent", "escape", "decodeURI", "decodeURIComponent", "unescape"]) add(`T(()=>${fn}(${lit([cp])}))`);
const uriPairs = [[0xd83d, 0xde00], [0xde00, 0xd83d], [0xd83d, 0x61], [0x61, 0xde00], [0xd83d, 0xd83d], [0xde00, 0xde00], [0xd83d, 0xd83d, 0xde00], [0xd83d, 0xde00, 0xde00], [0x61, 0xd800], [0xd800, 0x61, 0x62], [0xdbff, 0xdfff], [0xd800, 0xdc00, 0xd800]];
for (const s of uriPairs) for (const fn of uriFns) add(`T(()=>${fn}(${lit(s)}))`);
const reserved = ";/?:@&=+$,#-_.!~*'()[]%<>\"{}|\\^` ";
for (const fn of uriFns) add(`T(()=>${fn}(${JSON.stringify(reserved)}))`);
const decodes = ["%", "%4", "%41", "%zz", "%4z", "%z4", "%C3%A9", "%c3%a9", "%C3", "%C3%28", "%C3%C3", "%E2%82%AC", "%E2%82", "%E2%82%", "%E2%28%A1", "%F0%9F%98%80", "%F0%9F%98", "%F4%8F%BF%BF", "%F4%90%80%80", "%F7%BF%BF%BF", "%F8%88%80%80%80", "%FC%84%80%80%80%80",
  "%ED%9F%BF", "%ED%A0%80", "%ED%BF%BF", "%EE%80%80", "%C0%80", "%C0%AF", "%C1%BF", "%C2%80", "%E0%80%80", "%E0%9F%BF", "%E0%A0%80", "%F0%80%80%80", "%F0%8F%BF%BF", "%F0%90%80%80", "%80", "%BF", "%FF", "%FE", "%00", "%0", "%0a", "%0A%0d", "%7f", "%7F",
  "%25", "%2525", "%25%32%35", "%2f", "%2F%3f%26", "%3B%2C%2F%3F%3A%40%26%3D%2B%24%23", "%23", "%24", "%26", "%2B", "%2C", "%3A", "%3D", "%40", "%20", "%21", "%27", "%28", "%29", "%2A", "%2D", "%2E", "%5F", "%7E", "%3C", "%3E", "%22", "%7B", "%7D", "%5B", "%5D", "%5C", "%5E", "%60", "%7C",
  "%u0041", "%U0041", "%e9", "%E9", "a%20b%2Fc%3Fd", "a%", "a%4", "%%41", "%41%", "%C3%A9%C3%A9", "%C3%A9%", "%E3%81%82", "%E3%81", "%E3%81%82%E3", "%ef%bb%bf", "%EF%BF%BD", "%EF%BF%BE", "%EF%BF%BF", "%F0%9F%87%A7%F0%9F%87%B7", "%F0%9F%91%A8%E2%80%8D%F0%9F%91%A9",
  "%e2%82%ac", "%E2%82%aC", "%c3%A9", "%C3%a9", "plain", "", " ", "%20%20", "%2", "%G0", "%0G", "% 41", "%+41", "%-1", "%0x", "%４１", "%C3%A9%zz", "%zz%C3%A9", "%F0%9F%98%80%F0%9F", "%F0%9F%98%80%80"];
for (const d of decodes) for (const fn of ["decodeURI", "decodeURIComponent", "unescape"]) add(`T(()=>${fn}(${JSON.stringify(d)}))`);
for (const d of ["%C3%A9", "%F0%9F%98%80", "%2F%3F", "%41%42", "%E2%82%AC"]) add(`T(()=>decodeURI(${JSON.stringify(d)})===decodeURIComponent(${JSON.stringify(d)}))`, `T(()=>decodeURIComponent(encodeURIComponent(decodeURIComponent(${JSON.stringify(d)})))===decodeURIComponent(${JSON.stringify(d)}))`);
const unescapes = ["%u0041", "%u00e9", "%u00E9", "%u20ac", "%uD83D%uDE00", "%uD83D", "%uDE00", "%uDE00%uD83D", "%u004", "%u00", "%u0", "%u", "%u00zz", "%uzz00", "%U0041", "%4", "%zz", "%", "%%41", "%E9", "%e9", "%FF", "%100", "%u0041%u0042", "%u0041x", "x%u0041", "%41%u0042%43", "%u+041", "%u-041", "%u 041", "%0", "%00", "%u0000", "%uFFFF", "%uffff", "%u{41}", "%0g", "%g0", "%u000G"];
for (const u of unescapes) add(`T(()=>unescape(${JSON.stringify(u)}))`, `T(()=>unescape(${JSON.stringify(u)}).length)`);
const escSrc = [[0x41, 0x2a, 0x2b, 0x2d, 0x2e, 0x2f, 0x5f, 0x40], [0xe9, 0xff, 0x100, 0xffff], [0x20ac, 0x1f600], [0x0, 0x1f, 0x7f, 0x80], [0x61, 0x20, 0x62], [0xd83d], [0xde00, 0xd83d]];
for (const s of escSrc) add(`T(()=>escape(${lit(s)}))`, `T(()=>unescape(escape(${lit(s)}))===${lit(s)})`, `T(()=>escape(${lit(s)}).length)`);
const uriNonString = ["undefined", "null", "123", "-0", "1.5", "true", "NaN", "{}", "[]", "[1,2]", "{toString(){return 'a b'}}", "{toString(){throw new RangeError('k')}}", "{valueOf(){return 'v'},toString(){return 's'}}", "Symbol()", "1n", "new String('a b')", "()=>1", "'\\u00e9'"];
for (const a of uriNonString) for (const fn of ["encodeURI", "encodeURIComponent", "decodeURI", "decodeURIComponent", "escape", "unescape"]) add(`T(()=>${fn}(${a}))`);
for (const fn of ["encodeURI", "encodeURIComponent", "decodeURI", "decodeURIComponent", "escape", "unescape"]) {
  add(
    `T(()=>${fn}())`, `T(()=>[${fn}.length,${fn}.name,typeof ${fn}])`, `T(()=>new ${fn}("a"))`, `T(()=>${fn}.call(null,"a b"))`, `T(()=>Object.getOwnPropertyDescriptor(globalThis,"${fn}").enumerable)`, `T(()=>Object.getOwnPropertyDescriptor(globalThis,"${fn}").writable)`,
    `T(()=>${fn}("a","b","c"))`, `T(()=>${fn}.hasOwnProperty("prototype"))`,
  );
}
// URIError: tipo, mensagem e herança
add(
  "T(()=>{try{decodeURI('%')}catch(e){return [e instanceof URIError,e.name,e.message,Object.getPrototypeOf(e)===URIError.prototype,e.constructor===URIError]}})",
  "T(()=>{try{encodeURI('\\ud800')}catch(e){return [e instanceof URIError,e.name,e.message,String(e)]}})",
  "T(()=>{try{encodeURIComponent('\\udc00')}catch(e){return [e instanceof URIError,e.name,e.message,Object.prototype.toString.call(e)]}})",
  "T(()=>{try{decodeURIComponent('%E0%A4%A')}catch(e){return [e.name,e.message,e.stack===undefined]}})", "T(()=>{try{decodeURI('%C0%80')}catch(e){return Object.getOwnPropertyNames(e).sort()}})",
  "T(()=>Object.getPrototypeOf(URIError)===Error)", "T(()=>[URIError.length,URIError.name,URIError.prototype.name,URIError.prototype.message===''])", "T(()=>new URIError('x',{cause:1}).cause)", "T(()=>URIError('a').message)",
  "T(()=>{try{decodeURI('%C3%')}catch(e){return e.constructor.name}})", "T(()=>{var r=[];for(var s of ['%','%4','%zz','%C3','%E2%82','%ED%A0%80','%F4%90%80%80','%80','%C0%80','%F8%88%80%80%80']){try{decodeURIComponent(s);r.push('ok')}catch(e){r.push(e.name+':'+e.message)}}return r})",
  "T(()=>{var r=[];for(var s of ['\\ud800','\\udc00','a\\ud800','\\udc00\\ud800','\\ud800\\ud800\\udc00','\\ud83d\\ude00']){try{encodeURI(s);r.push('ok')}catch(e){r.push(e.name+':'+e.message)}}return r})",
  "T(()=>decodeURI('%E2%82%AC')===String.fromCodePoint(0x20ac))", "T(()=>decodeURIComponent('%F0%9F%98%80').length)", "T(()=>decodeURIComponent('%F0%9F%98%80').codePointAt(0).toString(16))",
  "T(()=>encodeURI('http://a.b/c d?e=f&g=h#i'))", "T(()=>encodeURIComponent('http://a.b/c d?e=f&g=h#i'))", "T(()=>decodeURI('http%3A%2F%2Fa.b%2Fc%20d%3Fe%3Df%26g%3Dh%23i'))", "T(()=>decodeURIComponent('http%3A%2F%2Fa.b%2Fc%20d%3Fe%3Df%26g%3Dh%23i'))",
  "T(()=>encodeURI('%41%zz'))", "T(()=>encodeURIComponent('%41'))", "T(()=>encodeURI(encodeURI(' ')))", "T(()=>decodeURI(decodeURI('%2520')))", "T(()=>escape('%u0041'))", "T(()=>unescape(escape('\\u0100\\u00ff')))",
);

// ---- Execução: dedup entre si e contra os goldens existentes, filhos novos em paralelo.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const baseSources = [];
baseSources.push(...knownPrograms("unicode_grid_bun.tsv", (file) => !(!file.endsWith(".tsv") || file === "unicode_grid_bun.tsv")));
// Igualdade do programa: o corpo "T(()=>...)" de cada linha dos goldens existentes entra num conjunto.
const baseExprs = new Set();
for (const source of baseSources) {
  if (typeof source !== "string") continue;
  for (const line of source.split("\n")) {
    const at = line.indexOf("T(()=>");
    if (at >= 0) baseExprs.add(line.slice(at).replace(/^globalThis\.R = /, ""));
  }
}
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let dup = 0;
const pending = [];
for (const expr of unique) {
  if (baseExprs.has(expr)) { dup++; continue; }
  pending.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { timeout: 5000, killSignal: "SIGKILL" });
    let out = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.resume();
    child.on("error", () => resolve(null));
    child.on("close", (code) => resolve(code === 0 ? decodeResult(out) : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(pending.length);
  let next = 0;
  const worker = async () => {
    while (next < pending.length) {
      const i = next++;
      results[i] = await runChild(pending[i].source);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < pending.length; i++) {
    const result = results[i];
    // Fonte e resultado têm que ser ASCII puro (nada de travessão nem marca do ambiente).
    if (result === null || /[^\x00-\x7f]/.test(result) || /[^\x00-\x7f]/.test(pending[i].source) || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(pending[i].expr).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push(JSON.stringify(pending[i].source) + "\t" + JSON.stringify(result));
  }
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
