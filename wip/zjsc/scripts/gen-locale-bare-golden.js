// Gera tests/golden/locale_bare_bun.tsv: métodos de locale chamados SEM `Intl` explícito, como o código comum faz,
// medido no bun 1.4.2 com TZ=UTC no processo filho. Cobre Number/BigInt/Date/Array/TypedArray.prototype.toLocaleString,
// toLocaleDateString, toLocaleTimeString, localeCompare e toLocaleUpperCase/toLocaleLowerCase em ~19 locales, com locale
// em string, array, undefined e inválido (RangeError com a mensagem exata), e opções (style, currency, fração, dateStyle,
// timeStyle, hour12, timeZone UTC/America/Sao_Paulo/Asia/Tokyo, timeZoneName, era, weekday e month longos).
// Programas cuja expressão já aparece em number_matrix_bun e locale_methods_bun (e vizinhos) são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-locale-bare-golden.js > tests/golden/locale_bare_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v)}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = (s) => JSON.stringify(s);

const locales = ["en-US", "en-GB", "pt-BR", "pt-PT", "de-DE", "fr-FR", "es-ES", "es-MX", "it-IT", "ja-JP", "ko-KR", "zh-CN", "ru-RU", "ar-EG", "hi-IN", "tr-TR", "sv-SE", "nl-NL", "pl-PL"];

// ---- 1. Number.prototype.toLocaleString.
const numbers = ["1234.5", "-9876543.21", "0.000123", "1e21", "123456789012", "0.5", "42"];
const numOpts = [
  "", "{style:'currency',currency:'USD'}", "{style:'currency',currency:'BRL'}", "{style:'currency',currency:'EUR'}", "{style:'currency',currency:'JPY'}",
  "{style:'percent'}", "{minimumFractionDigits:2}", "{maximumFractionDigits:0}", "{minimumFractionDigits:3,maximumFractionDigits:3}",
  "{useGrouping:false}", "{notation:'compact'}", "{style:'unit',unit:'kilometer-per-hour'}", "{signDisplay:'always'}", "{maximumSignificantDigits:3}",
  "{style:'currency',currency:'USD',currencyDisplay:'name'}", "{minimumIntegerDigits:5}",
];
for (const l of locales) for (const n of numbers) for (const o of numOpts) {
  if (o === "" && (n === "1234.5" || n === "0.000123")) continue;
  add(`(${n}).toLocaleString(${q(l)}${o ? "," + o : ""})`);
}
for (const n of ["NaN", "Infinity", "-Infinity", "-0", "0"]) for (const l of ["en-US", "de-DE", "ja-JP", "ar-EG", "hi-IN", "pt-BR"]) {
  add(`(${n}).toLocaleString(${q(l)},{style:'currency',currency:'USD'})`, `(${n}).toLocaleString(${q(l)},{signDisplay:'exceptZero'})`, `(${n}).toLocaleString(${q(l)},{style:'percent'})`);
}
// Formas de passar o locale.
const localeForms = ["undefined", "[]", "['de-DE']", "['xx','de-DE']", "['de-DE','fr-FR']", "'de'", "'DE-de'", "'de-DE-u-nu-arab'", "'ar-u-nu-latn'", "'en-u-nu-fullwide'", "'th-u-nu-thai'", "'zh-Hans-CN'", "'pt-u-hc-h12'", "'und'", "'zz-ZZ'", "'sr-Latn'", "'en-Latn-US'", "'ja-JP-u-ca-japanese'", "'hi-u-nu-deva'", "'fa-IR'", "'he-IL'", "'ca-ES'", "'nb-NO'", "'da-DK'", "'fi-FI'", "'cs-CZ'", "'el-GR'", "'id-ID'", "'vi-VN'", "'uk-UA'", "{toString(){return 'fr'}}", "new String('es')", "{length:1,0:'it'}", "'en-US-u-nu-hanidec'"];
for (const f of localeForms) {
  add(`(1234567.891).toLocaleString(${f})`, `(1234567.891).toLocaleString(${f},{style:'currency',currency:'EUR'})`, `(123n).toLocaleString(${f})`, `new Date(86400000*400+3723000).toLocaleDateString(${f})`, `new Date(86400000*400+3723000).toLocaleTimeString(${f})`, `new Date(86400000*400+3723000).toLocaleString(${f})`, `"a".localeCompare("b",${f})`, `"i".toLocaleUpperCase(${f})`, `[1234.5,2].toLocaleString(${f})`);
}
// Locale inválido, mensagens exatas.
const badLocales = ["'x_y'", "'en_US'", "''", "'a'", "'abcdefghi'", "'en-'", "'-en'", "'en--US'", "'en-US-'", "'e1'", "'en-u'", "'en-u-'", "'en-a-b-c-d-e-f-g-h-i-j-k-l-m'", "'en-US-u-nu-latn-nu-arab'", "'123'", "'en-x'", "'i-klingon'", "'en-US-US'", "'en-Latn-Latn'", "'en-GB-GB'", "'😀'", "'en US'", "'en-U'", "'x-private'", "'ENGLISH'", "[1]", "[null]", "[undefined]", "[{}]", "[Symbol()]", "[[]]", "[['en']]", "null", "1", "true", "Symbol()", "1n", "{get length(){throw new EvalError('l')}}", "{length:1,get 0(){throw new EvalError('g')}}", "['en','x_y']", "['x_y','en']"];
for (const f of badLocales) {
  add(`(1).toLocaleString(${f})`, `(1n).toLocaleString(${f})`, `new Date(0).toLocaleString(${f})`, `new Date(0).toLocaleDateString(${f})`, `new Date(0).toLocaleTimeString(${f})`, `"a".localeCompare("b",${f})`, `"a".toLocaleUpperCase(${f})`, `"a".toLocaleLowerCase(${f})`, `[1].toLocaleString(${f})`, `new Uint8Array(1).toLocaleString(${f})`);
}
// Opções inválidas e ordem de avaliação.
const badOpts = ["{style:'x'}", "{style:'currency'}", "{style:'currency',currency:'US'}", "{style:'currency',currency:'USDD'}", "{style:'currency',currency:1}", "{style:'unit'}", "{style:'unit',unit:'foo'}", "{style:'unit',unit:'meter-per-foo'}", "{minimumFractionDigits:-1}", "{minimumFractionDigits:101}", "{maximumFractionDigits:101}", "{minimumFractionDigits:5,maximumFractionDigits:2}", "{minimumIntegerDigits:0}", "{minimumIntegerDigits:22}", "{maximumSignificantDigits:0}", "{maximumSignificantDigits:22}", "{minimumSignificantDigits:5,maximumSignificantDigits:2}", "{notation:'x'}", "{signDisplay:'x'}", "{currencyDisplay:'x'}", "{compactDisplay:'x'}", "{useGrouping:'x'}", "{roundingMode:'x'}", "{localeMatcher:'x'}", "{numberingSystem:'x'}", "{numberingSystem:'abcdefghi'}", "null", "1", "'str'", "true", "Symbol()", "{get style(){throw new EvalError('s')}}", "{minimumFractionDigits:NaN}", "{minimumFractionDigits:'2'}", "{minimumFractionDigits:2.9}", "{roundingIncrement:3}", "{trailingZeroDisplay:'x'}"];
for (const o of badOpts) add(`(1234.5).toLocaleString("en-US",${o})`, `(1234n).toLocaleString("en-US",${o})`);

// ---- 2. BigInt.prototype.toLocaleString.
const bigs = ["0n", "-1n", "1234567890123456789012345678901234567890n", "999n", "1000n", "BigInt(Number.MAX_SAFE_INTEGER)*2n", "-(2n**64n)"];
const bigOpts = ["", "{style:'currency',currency:'BRL'}", "{useGrouping:false}", "{notation:'compact'}", "{minimumFractionDigits:2}", "{style:'percent'}", "{notation:'scientific'}", "{notation:'engineering'}"];
for (const l of locales) for (const b of bigs) for (const o of bigOpts) {
  if (o && !["en-US", "pt-BR", "de-DE", "ja-JP", "ar-EG", "hi-IN"].includes(l)) continue;
  add(`(${b}).toLocaleString(${q(l)}${o ? "," + o : ""})`);
}
add("BigInt.prototype.toLocaleString.call(1)", "BigInt.prototype.toLocaleString.call('1')", "BigInt.prototype.toLocaleString.call({})", "BigInt.prototype.toLocaleString.call(Object(1n))", "Number.prototype.toLocaleString.call('1')", "Number.prototype.toLocaleString.call(1n)", "Number.prototype.toLocaleString.call(Object(5))", "Number.prototype.toLocaleString.call({})", "Number.prototype.toLocaleString.call(undefined)", "Date.prototype.toLocaleString.call({})", "Date.prototype.toLocaleDateString.call(1)", "Date.prototype.toLocaleTimeString.call('x')", "Date.prototype.toLocaleString.call(Object.create(Date.prototype))", "Array.prototype.toLocaleString.call(null)", "Array.prototype.toLocaleString.call(undefined)", "Array.prototype.toLocaleString.call('ab')", "Array.prototype.toLocaleString.call({length:2,0:1,1:2})", "Array.prototype.toLocaleString.call({})", "String.prototype.localeCompare.call(null,'a')", "String.prototype.localeCompare.call(undefined,'a')", "String.prototype.toLocaleUpperCase.call(null)", "String.prototype.toLocaleLowerCase.call(undefined)", "String.prototype.toLocaleUpperCase.call(12)", "String.prototype.localeCompare.call(12,'3')", "BigInt.prototype.toLocaleString.length", "Number.prototype.toLocaleString.length", "Date.prototype.toLocaleString.length", "Array.prototype.toLocaleString.length", "String.prototype.localeCompare.length", "String.prototype.toLocaleUpperCase.length", "Date.prototype.toLocaleDateString.name", "Date.prototype.toLocaleTimeString.name", "Object.prototype.toLocaleString.length", "Object.prototype.toLocaleString.name", "Object.prototype.toLocaleString.call(1)", "Object.prototype.toLocaleString.call(null)", "Object.prototype.toLocaleString.call({toString(){return 'ts'}})", "Object.prototype.toLocaleString.call(Symbol('x'))", "Object.prototype.toLocaleString.call(Object(1n))");

// ---- 3. Date.prototype.toLocale{,Date,Time}String.
const dates = ["0", "1700000000123", "-62135596800000", "951782400000", "1735689599999", "NaN"];
const dateOpts = [
  "", "{timeZone:'UTC'}", "{timeZone:'America/Sao_Paulo'}", "{timeZone:'Asia/Tokyo'}", "{dateStyle:'full'}", "{dateStyle:'long'}", "{dateStyle:'medium'}", "{dateStyle:'short'}",
  "{timeStyle:'full'}", "{timeStyle:'long'}", "{timeStyle:'short'}", "{dateStyle:'full',timeStyle:'short'}", "{dateStyle:'medium',timeStyle:'medium',timeZone:'America/Sao_Paulo'}",
  "{hour12:true}", "{hour12:false}", "{hour:'numeric',minute:'2-digit',hour12:true,timeZone:'Asia/Tokyo'}", "{hourCycle:'h23',hour:'2-digit',minute:'2-digit'}",
  "{timeZoneName:'short',timeZone:'America/Sao_Paulo'}", "{timeZoneName:'long',timeZone:'Asia/Tokyo'}", "{timeZoneName:'shortOffset',timeZone:'UTC'}", "{timeZoneName:'longOffset',timeZone:'America/Sao_Paulo'}",
  "{era:'long',year:'numeric',timeZone:'UTC'}", "{era:'short',year:'numeric',month:'numeric',day:'numeric',timeZone:'UTC'}", "{era:'narrow',year:'numeric',timeZone:'UTC'}",
  "{weekday:'long',year:'numeric',month:'long',day:'numeric',timeZone:'UTC'}", "{weekday:'short',month:'short',day:'2-digit',timeZone:'UTC'}", "{month:'long'}", "{month:'narrow',day:'numeric',timeZone:'UTC'}",
  "{year:'2-digit',month:'2-digit',day:'2-digit',timeZone:'UTC'}", "{weekday:'long',timeZone:'Asia/Tokyo'}", "{fractionalSecondDigits:3,second:'numeric',timeZone:'UTC'}", "{dayPeriod:'long',hour:'numeric',timeZone:'UTC'}",
];
const methods = ["toLocaleString", "toLocaleDateString", "toLocaleTimeString"];
for (const l of locales) for (const d of dates) {
  if (d === "NaN") { for (const m of methods) add(`new Date(${d}).${m}(${q(l)})`); continue; }
  for (const o of dateOpts) {
    // Para cada (locale, data, opção) um método só, rodando entre os três; o par com `{}` completo fica nas formas de locale.
    const m = methods[(dateOpts.indexOf(o) + dates.indexOf(d) + locales.indexOf(l)) % 3];
    add(`new Date(${d}).${m}(${q(l)}${o ? "," + o : ""})`);
  }
}
// Os três métodos por opção em poucos locales (cobre o default de cada um com dateStyle/timeStyle).
for (const l of ["en-US", "pt-BR", "ja-JP", "ar-EG", "de-DE", "hi-IN"]) for (const o of dateOpts) for (const m of methods) {
  add(`new Date(1700000000123).${m}(${q(l)}${o ? "," + o : ""})`);
}
// Combinações inválidas.
const badDateOpts = ["{dateStyle:'full',year:'numeric'}", "{timeStyle:'short',hour:'numeric'}", "{dateStyle:'x'}", "{timeStyle:'x'}", "{timeZone:'Mars/Base'}", "{timeZone:'x'}", "{timeZone:''}", "{timeZone:'+25:00'}", "{timeZone:'+03:00'}", "{timeZone:'utc'}", "{timeZone:'america/sao_paulo'}", "{timeZone:'Etc/GMT+3'}", "{timeZone:'GMT'}", "{timeZone:'EST'}", "{timeZone:1}", "{timeZone:null}", "{timeZone:undefined}", "{hour12:'x'}", "{hourCycle:'x'}", "{weekday:'x'}", "{month:'x'}", "{era:'x'}", "{timeZoneName:'x'}", "{calendar:'x'}", "{calendar:'abcdefghi'}", "{calendar:'japanese',era:'long',year:'numeric',timeZone:'UTC'}", "{calendar:'buddhist',year:'numeric',timeZone:'UTC'}", "{calendar:'islamic',timeZone:'UTC'}", "{numberingSystem:'arab',timeZone:'UTC'}", "{fractionalSecondDigits:0}", "{fractionalSecondDigits:4}", "{formatMatcher:'x'}", "null", "1", "'x'", "{get timeZone(){throw new EvalError('tz')}}"];
for (const o of badDateOpts) for (const m of methods) add(`new Date(0).${m}("en-US",${o})`);
add(
  "new Date(NaN).toLocaleString('en-US',{timeZone:'Mars/Base'})", "new Date(8.64e15+1).toLocaleDateString('en-US')", "new Date(8.64e15).toLocaleString('en-US',{timeZone:'UTC'})", "new Date(-8.64e15).toLocaleString('en-US',{timeZone:'UTC'})",
  "new Date(8.64e15).toLocaleString('en-US',{timeZone:'Asia/Tokyo'})", "new Date(-8.64e15).toLocaleString('en-US',{timeZone:'America/Sao_Paulo'})", "new Date(0).toLocaleString('en-US',{timeZone:'UTC',timeZoneName:'short'})",
  "new Date(0).toLocaleDateString('en-US',{hour:'numeric',timeZone:'UTC'})", "new Date(0).toLocaleTimeString('en-US',{year:'numeric',timeZone:'UTC'})", "new Date(0).toLocaleDateString('en-US',{timeStyle:'short'})", "new Date(0).toLocaleTimeString('en-US',{dateStyle:'short'})",
  "new Date(0).toLocaleDateString('en-US',{weekday:'long'})", "new Date(0).toLocaleTimeString('en-US',{weekday:'long'})", "new Date(0).toLocaleDateString('en-US',{hour:'numeric',minute:'numeric',timeZone:'UTC'})",
  "new Date(0).toLocaleTimeString('en-US',{month:'long',timeZone:'UTC'})", "new Date(0).toLocaleString('en-US',{minute:'numeric',timeZone:'UTC'})", "new Date(0).toLocaleString('en-US',{second:'2-digit',timeZone:'UTC'})",
  "new Date(0).toLocaleString()", "new Date(0).toLocaleDateString()", "new Date(0).toLocaleTimeString()", "new Date(1700000000123).toLocaleString()", "new Date(1700000000123).toLocaleDateString()", "new Date(1700000000123).toLocaleTimeString()",
  "new Date(1700000000123).toLocaleString(undefined,{timeZone:'Asia/Tokyo'})", "new Date(1700000000123).toLocaleString(undefined,{dateStyle:'full',timeStyle:'full',timeZone:'America/Sao_Paulo'})",
);

// ---- 4. Array e TypedArray.prototype.toLocaleString: separador ',', null/undefined, toLocaleString dos elementos.
const arrays = [
  "[]", "[1234.5]", "[1234.5,-0.5,1e21]", "[1,null,2]", "[undefined,1]", "[null]", "[undefined,null,undefined]", "[1,,3]", "[[1234.5,2],[3]]", "[[],[]]", "[[null],[undefined]]", "[1234.5,'a',true,false]",
  "[new Date(0),1234.5]", "[new Date(NaN)]", "[12345678901234567890n,5n]", "[NaN,Infinity,-Infinity]", "[{},[]]", "[{toLocaleString(){return 'x'}},{toLocaleString(){return 'y'}}]", "[{toLocaleString:null}]", "[{toLocaleString:1}]",
  "[{toLocaleString(){return null}}]", "[{toLocaleString(){return undefined}}]", "[{toLocaleString(){return {toString(){return 'ts'}}}}]", "[{toLocaleString(){return Symbol()}}]", "[Symbol()]", "[Object.create(null)]", "[new Proxy({},{})]",
  "[{toLocaleString(){return [].slice.call(arguments).join('|')+arguments.length}}]", "[{toLocaleString(){return String(this===undefined)}}]", "[1.5,2.5]", "[1,[2,[3,[4]]]]", "[0,-0]", "['é','ü']", "[true]",
];
const arrLocales = [undefined, "en-US", "de-DE", "pt-BR", "fr-FR", "ja-JP", "ar-EG", "hi-IN", "ru-RU"];
for (const a of arrays) for (const l of arrLocales) {
  add(l === undefined ? `${a}.toLocaleString()` : `${a}.toLocaleString(${q(l)})`);
}
for (const a of arrays.slice(1, 6)) for (const o of ["{style:'currency',currency:'USD'}", "{minimumFractionDigits:2}", "{style:'percent'}", "{dateStyle:'short',timeZone:'UTC'}"]) {
  add(`${a}.toLocaleString("en-US",${o})`, `${a}.toLocaleString("pt-BR",${o})`, `${a}.toLocaleString(undefined,${o})`);
}
add(
  "(()=>{var a=[1];a[1]=a;return a.toLocaleString()})()", "(()=>{var a=[1,2];a.push(a);return a.toLocaleString('en-US')})()", "(()=>{var a=[[1]];a[0].push(a);return a.toLocaleString()})()",
  "(()=>{var a=[{toLocaleString(){return 'x'+arguments.length}}];return a.toLocaleString('en-US',{})})()", "(()=>{var a=[{toLocaleString(l,o){return String(l)+typeof o}}];return a.toLocaleString('en-US',{})})()",
  "(()=>{var a=[{toLocaleString(l,o){return String(l)+typeof o}}];return a.toLocaleString()})()", "(()=>{var a=[{toLocaleString(l,o){return String(l)+typeof o}}];return a.toLocaleString('de',1)})()",
  "(()=>{var log=[];var a=[{toLocaleString(){log.push('a');return 'a'}},{toLocaleString(){log.push('b');return 'b'}}];a.toLocaleString();return log.join()})()",
  "(()=>{var a=[1,2,3];a.length=5;return a.toLocaleString()})()", "(()=>{var a=[{toLocaleString(){a.length=0;return 'x'}},2,3];return a.toLocaleString()})()", "(()=>{var a=[{toLocaleString(){a.push(9);return 'x'}},2];return a.toLocaleString()})()",
  "(()=>{var o={length:3,0:1,1:null,2:1234.5};return Array.prototype.toLocaleString.call(o,'de-DE')})()", "(()=>{var o={length:'2',0:'a',1:'b'};return Array.prototype.toLocaleString.call(o)})()", "(()=>{var o={length:-1};return Array.prototype.toLocaleString.call(o)})()",
  "(()=>{var o={length:2,0:'a',1:'b',join(){return 'J'}};return Array.prototype.toLocaleString.call(o)})()", "(()=>{var a=[1,2];a.join=()=>'J';return a.toLocaleString()})()", "(()=>{var a=[1,2];Number.prototype.toLocaleString=function(){return 'N'};try{return a.toLocaleString()}finally{delete Number.prototype.toLocaleString}})()",
  "(()=>{var orig=Number.prototype.toLocaleString;Number.prototype.toLocaleString=function(l,o){return 'N'+l+o};try{return [1].toLocaleString('en',5)}finally{Number.prototype.toLocaleString=orig}})()",
  "[1234.5].toLocaleString.length", "[].toLocaleString.name", "Array.prototype.toLocaleString===Array.prototype.toString", "Object.prototype.toLocaleString===Object.prototype.toString", "Number.prototype.toLocaleString===Number.prototype.toString",
);
const typed = [["Uint8Array", "[1,2,255]"], ["Int8Array", "[-1,0,127]"], ["Int16Array", "[-1234,5678]"], ["Uint16Array", "[65535,1]"], ["Int32Array", "[-2147483648,2147483647]"], ["Uint32Array", "[4294967295,1000000]"], ["Float32Array", "[1234.5,0.1,-0]"], ["Float64Array", "[1234.5678,1e21,NaN,Infinity]"], ["Uint8ClampedArray", "[300,-5,1.5]"], ["BigInt64Array", "[-9223372036854775808n,12345678901234567890n]"], ["BigUint64Array", "[18446744073709551615n,1000n]"], ["Float16Array", "[1.5,1234.5]"]];
for (const [name, init] of typed) {
  for (const l of [undefined, "en-US", "de-DE", "pt-BR", "ar-EG", "hi-IN", "ja-JP"]) add(l === undefined ? `new ${name}(${init}).toLocaleString()` : `new ${name}(${init}).toLocaleString(${q(l)})`);
  add(`new ${name}(${init}).toLocaleString("en-US",{minimumFractionDigits:2})`, `new ${name}(${init}).toLocaleString("pt-BR",{style:"currency",currency:"BRL"})`, `new ${name}(${init}).toLocaleString("en-US",{useGrouping:false})`, `new ${name}(0).toLocaleString()`, `new ${name}(1).toLocaleString("fr-FR")`, `new ${name}(${init}).toLocaleString("x_y")`, `new ${name}(${init}).toLocaleString("en",{style:"x"})`, `new ${name}(${init}).subarray(1).toLocaleString("de-DE")`, `${name}.prototype.toLocaleString===Array.prototype.toLocaleString`, `${name}.prototype.toLocaleString.length`, `Object.getPrototypeOf(${name}).prototype.toLocaleString===Object.getPrototypeOf(Uint8Array).prototype.toLocaleString`);
}
add("Uint8Array.prototype.toLocaleString.call([1,2])", "Uint8Array.prototype.toLocaleString.call({})", "Uint8Array.prototype.toLocaleString.call(null)", "Uint8Array.prototype.toLocaleString.call(new ArrayBuffer(2))", "Uint8Array.prototype.toLocaleString.call(new DataView(new ArrayBuffer(2)))", "Object.getPrototypeOf(Uint8Array.prototype).toLocaleString.call(new Int16Array([1000,2]),'de')",
  "(()=>{var t=new Uint8Array(2);Object.defineProperty(Number.prototype,'toLocaleString',{value(){return 'N'},configurable:true,writable:true});try{return t.toLocaleString()}finally{delete Number.prototype.toLocaleString}})()",
  "(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);return t.toLocaleString()})()");

// ---- 5. String.prototype.localeCompare e toLocale{Upper,Lower}Case sem Intl.
const pairs = [["a", "b"], ["a", "A"], ["a", "á"], ["z", "ä"], ["ä", "z"], ["ä", "a"], ["résumé", "resume"], ["co-op", "coop"], ["10", "2"], ["a", "a"], ["", "a"], ["ß", "ss"], ["ç", "d"], ["ñ", "n"], ["ñ", "o"], ["I", "i"], ["ı", "i"], ["İ", "i"], ["Å", "Z"], ["å", "z"], ["ö", "z"], ["ch", "d"], ["ll", "m"], ["æ", "ae"], ["ø", "z"], ["aa", "b"], ["あ", "ア"], ["a", "ａ"], ["é", "é"], ["ẛ̣", "ẛ̣"], ["Z", "a"], ["_", "-"], ["a b", "ab"], ["日本", "中国"], ["я", "а"], ["ё", "е"], ["ا", "ب"], ["α", "β"], ["字", "字"], ["1", "١"]];
const collLocales = ["en-US", "de-DE", "sv-SE", "es-ES", "fr-FR", "tr-TR", "ja-JP", "pl-PL", "ru-RU", "ar-EG", "pt-BR", "da-DK", "cs-CZ", "ko-KR", "zh-CN", "hi-IN"];
for (const [a, b] of pairs) for (const l of collLocales) add(`${q(a)}.localeCompare(${q(b)},${q(l)})`);
const collOpts = ["{sensitivity:'base'}", "{sensitivity:'accent'}", "{sensitivity:'case'}", "{sensitivity:'variant'}", "{numeric:true}", "{caseFirst:'upper'}", "{caseFirst:'lower'}", "{ignorePunctuation:true}", "{usage:'search'}", "{numeric:true,sensitivity:'base'}"];
for (const [a, b] of pairs.slice(0, 14)) for (const o of collOpts) add(`${q(a)}.localeCompare(${q(b)},"en-US",${o})`, `${q(a)}.localeCompare(${q(b)},"sv",${o})`, `${q(a)}.localeCompare(${q(b)},undefined,${o})`);
for (const f of ["undefined", "[]", "['de']", "'de-u-co-phonebk'", "'sv-u-co-trad'", "'es-u-co-trad'", "'en-u-kn-true'", "'en-u-kf-upper'", "'zh-u-co-pinyin'", "'zh-u-co-stroke'", "'ja-u-co-unihan'", "'de-u-co-standard'", "'en-u-ks-level1'"]) {
  for (const [a, b] of [["ä", "af"], ["a10", "a2"], ["a", "A"], ["ä", "z"], ["字", "子"], ["a", "á"]]) add(`${q(a)}.localeCompare(${q(b)},${f})`);
}
add("'a'.localeCompare()", "'undefined'.localeCompare()", "'a'.localeCompare(undefined)", "'a'.localeCompare(null)", "'null'.localeCompare(null)", "'a'.localeCompare({toString(){return 'b'}})", "'a'.localeCompare(1)", "'1'.localeCompare(1)", "'a'.localeCompare(Symbol())", "'a'.localeCompare('b','en',{sensitivity:'x'})", "'a'.localeCompare('b','en',{caseFirst:'x'})", "'a'.localeCompare('b','en',{usage:'x'})", "'a'.localeCompare('b','en',null)", "['b','a','C','á','Z','z','ä'].sort((x,y)=>x.localeCompare(y)).join('')", "['b','a','C','á','Z','z','ä'].sort((x,y)=>x.localeCompare(y,'sv')).join('')", "['10','9','2','1'].sort((x,y)=>x.localeCompare(y,undefined,{numeric:true})).join()", "['é','e','f','E','É'].sort((x,y)=>x.localeCompare(y,'en')).join('')", "['é','e','f','E','É'].sort((x,y)=>x.localeCompare(y,'fr',{caseFirst:'upper'})).join('')", "['ä','a','z','o','ö'].sort((x,y)=>x.localeCompare(y,'de')).join('')", "['ä','a','z','o','ö'].sort((x,y)=>x.localeCompare(y,'sv')).join('')", "['ch','c','d','h'].sort((x,y)=>x.localeCompare(y,'cs')).join()", "['ñ','n','o'].sort((x,y)=>x.localeCompare(y,'es')).join('')", "['İ','I','i','ı'].sort((x,y)=>x.localeCompare(y,'tr')).join('')", "['İ','I','i','ı'].sort((x,y)=>x.localeCompare(y,'en')).join('')", "'a'.localeCompare.name", "''.localeCompare('')", "'\\u0000'.localeCompare('')", "'a\\u0000b'.localeCompare('ab')", "'\\ud800'.localeCompare('\\ud801')", "'\\ud83d\\ude00'.localeCompare('\\ud83d')");
const cases = ["i", "I", "ı", "İ", "ß", "ǆ", "ŉ", "ΐ", "ὈΔΥΣΣΕΎΣ", "σς", "Σ", "ǰ", "ﬃ", "ŀ", "Ĳ", "ij", "ǅ", "äöü", "ÄÖÜ", "ñ", "straße", "TITLE", "i̇", "İ", "ῖ", "ᾳ", "ᾼ", "ǈ", "ʼn", "Ա", "ա", "ᲀ", "𐐨", "𐐀", "ⅷ", "Ⅷ", "ⓐ", "ʻ", "\uD83D", "", "ab cd ef", "ǳ", "ﬁ", "ΑΣ", "ΑΣ.", "ΑΣ Α", "aΣ", "ΑΣ́", "Σ.Σ"];
const caseLocales = [undefined, "en-US", "tr", "tr-TR", "az", "az-Latn", "lt", "lt-LT", "de-DE", "el", "nl", "nl-NL", "pt-BR", "ja-JP", "ar", "ru", "und", "en-u-ca-gregory", "TR", "Tr-tr", "tr-u-nu-latn", "sv", "fr-CA", "ga", "hu", "ca-ES-valencia"];
for (const c of cases) for (const l of caseLocales) {
  const a = l === undefined ? "" : q(l);
  add(`${q(c)}.toLocaleUpperCase(${a})`, `${q(c)}.toLocaleLowerCase(${a})`);
}
add("'a'.toLocaleUpperCase(['tr','en'])", "'i'.toLocaleUpperCase(['tr','en'])", "'i'.toLocaleUpperCase(['xx','tr'])", "'i'.toLocaleUpperCase(['en','tr'])", "'I'.toLocaleLowerCase(['tr'])", "'a'.toLocaleUpperCase('')", "'a'.toLocaleUpperCase('x_y')", "'a'.toLocaleLowerCase(['x_y'])", "'a'.toLocaleUpperCase(null)", "'a'.toLocaleUpperCase(1)", "'a'.toLocaleUpperCase({})", "'a'.toLocaleUpperCase([])", "'a'.toLocaleUpperCase([undefined])", "'a'.toLocaleUpperCase('en','extra')", "'a'.toLocaleUpperCase.length", "'a'.toLocaleLowerCase.length", "'a'.toLocaleUpperCase.name", "''.toLocaleLowerCase('tr')", "'İ'.toLocaleLowerCase().length", "'İ'.toLocaleLowerCase('tr').length", "'I\\u0307'.toLocaleLowerCase('tr').length", "'ı'.toLocaleUpperCase('tr')", "'ß'.toLocaleUpperCase('de').length", "'ǆ'.toLocaleUpperCase('en')", "'ǅ'.toLocaleLowerCase('en')");

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("locale_bare_bun.tsv", (file) => !(!/^(number_matrix|locale_methods|locale_more|text_locale|numeric_limits|math|date_proto|datetime)/.test(file) || !file.endsWith(".tsv"))));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let kept = 0;
const PRELOAD = writeResultPreload();
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida (comparada nas duas formas de aspas).
  if (expr.length > 12 && (baseText.includes(expr) || baseText.includes(expr.replace(/"/g, "'")))) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>${expr})`;
  let result;
  try {
    // Processo fresco por programa, com TZ=UTC: as datas sem timeZone explícito saem em UTC.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, env: { ...process.env, TZ: "UTC" } });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("locale_bare", rows));
