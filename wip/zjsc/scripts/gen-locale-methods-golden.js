// Gera tests/golden/locale_methods_bun.tsv: métodos sensíveis a locale (String.prototype.localeCompare, normalize,
// toLocaleUpperCase e toLocaleLowerCase; Number, BigInt e Date toLocale*String com locales e opções variados;
// Array.prototype.toLocaleString; Intl.getCanonicalLocales e Intl.supportedValuesOf) medidos no bun 1.4.2.
// Locales: en-US, pt-BR, de-DE, ja-JP, ar-EG, hi-IN, tr-TR. Datas sempre com timeZone UTC (o resultado não pode
// depender do fuso da máquina). Sem APIs de host: só ECMAScript e Intl.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-locale-methods-golden.js > tests/golden/locale_methods_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Programa com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
const J = "JSON.stringify";
const locales = ["en-US", "pt-BR", "de-DE", "ja-JP", "ar-EG", "hi-IN", "tr-TR"];
const q = JSON.stringify;

// ---- localeCompare.
const words = [
  ["a", "b"], ["b", "a"], ["a", "A"], ["A", "a"], ["a", "á"], ["z", "ä"], ["ä", "z"], ["ç", "d"], ["I", "ı"],
  ["i", "İ"], ["ö", "o"], ["ö", "p"], ["résumé", "resume"], ["Å", "Z"], ["ß", "ss"], ["10", "9"], ["a", "a"],
  ["あ", "ア"], ["か", "が"], ["漢", "字"], ["ا", "ب"], ["क", "ख"], ["", "a"], ["ñ", "n"],
];
for (const loc of locales) for (const [a, b] of words) T(`R = ${q(a)}.localeCompare(${q(b)}, ${q(loc)})`);
for (const [a, b] of [["a", "A"], ["a", "á"], ["résumé", "resume"], ["10", "9"], ["ä", "a"]]) {
  for (const opt of [
    "{ sensitivity: 'base' }", "{ sensitivity: 'accent' }", "{ sensitivity: 'case' }", "{ sensitivity: 'variant' }",
    "{ numeric: true }", "{ caseFirst: 'upper' }", "{ ignorePunctuation: true }", "{ usage: 'search' }",
  ]) T(`R = ${q(a)}.localeCompare(${q(b)}, 'de-DE', ${opt})`);
}
T("R = 'a'.localeCompare()");
T("R = 'undefined'.localeCompare()");
T("R = 'a'.localeCompare('b', 'xx-invalid-locale-')");
T("R = 'a'.localeCompare('b', ['sv', 'en'])");
T("R = ['ä', 'a', 'z'].sort((x, y) => x.localeCompare(y, 'sv')).join()");
T("R = ['ä', 'a', 'z'].sort((x, y) => x.localeCompare(y, 'de')).join()");
T("R = ['č', 'c', 'd', 'ch', 'h'].sort((x, y) => x.localeCompare(y, 'cs')).join()");
T("R = ['a2', 'a10', 'a1'].sort((x, y) => x.localeCompare(y, 'en', { numeric: true })).join()");
T("R = ['I', 'i', 'ı', 'İ'].sort((x, y) => x.localeCompare(y, 'tr')).join()");
T("R = ['I', 'i', 'ı', 'İ'].sort((x, y) => x.localeCompare(y, 'en')).join()");
T("R = String.prototype.localeCompare.call(1, 2)");
T("R = String.prototype.localeCompare.call(null, 'a')");
T("R = 'a'.localeCompare('b', 'en', { sensitivity: 'bogus' })");
T("R = 'a\\u0301'.localeCompare('\\u00e1', 'en')");
T("R = '\\u212b'.localeCompare('\\u00c5', 'en')");

// ---- normalize.
const norms = ["é", "é", "Å", "Å", "ẛ̣", "ﬁ", "½", "Ａ", "가", "가", "Ạ̊", "क़", "Ω", "😀", "ぱ", "ぱ"];
for (const s of norms) for (const f of ["NFC", "NFD", "NFKC", "NFKD"]) T(`R = ${J}(Array.from(${q(s)}.normalize(${q(f)}), c => c.codePointAt(0).toString(16)))`);
T("R = 'abc'.normalize()");
T("R = '\\u00e9'.normalize(undefined).length");
T("R = 'a'.normalize('nfc')");
T("R = 'a'.normalize('')");
T("R = 'a'.normalize(null)");
T("R = '\\ud800'.normalize('NFD').length");
T("R = '\\ud800x'.normalize('NFKC').charCodeAt(0)");
T("R = String.prototype.normalize.call(123, 'NFD')");
T("R = String.prototype.normalize.call(undefined)");
T("R = 'e\\u0301\\u0323'.normalize('NFC').length");
T("R = '\\u0323\\u0301e'.normalize('NFC').length");

// ---- toLocaleUpperCase / toLocaleLowerCase.
const cases = ["i", "I", "ı", "İ", "ß", "straße", "ǆ", "ŉ", "ΐ", "ΣΑΣ", "ὈΔΥΣΣΕΎΣ", "ﬁ", "istanbul", "ISTANBUL", "ǅ", "iJ", "ǰ", "ж", "ა", "ᾳ"];
const caseLocales = ["en-US", "tr-TR", "az", "lt", "de-DE", "el", "pt-BR", "ja-JP", "nl", "en"];
for (const s of cases) for (const loc of caseLocales) {
  T(`R = ${q(s)}.toLocaleUpperCase(${q(loc)})`);
  T(`R = ${q(s)}.toLocaleLowerCase(${q(loc)})`);
}
T("R = 'i'.toLocaleUpperCase()");
T("R = 'I'.toLocaleLowerCase(['tr', 'en'])");
T("R = 'i'.toLocaleUpperCase('xx-invalid-')");
T("R = 'i'.toLocaleUpperCase([])");
T("R = 'i'.toLocaleUpperCase(undefined)");
T("R = 'i'.toLocaleUpperCase(null)");
T("R = 'i'.toLocaleUpperCase(5)");
T("R = 'i\\u0307'.toLocaleUpperCase('lt')");
T("R = 'I\\u0307'.toLocaleLowerCase('tr')");
T("R = 'I\\u0300'.toLocaleLowerCase('lt')");
T("R = 'i\\u0307'.toLocaleUpperCase('tr')");
T("R = String.prototype.toLocaleUpperCase.call(null)");
T("R = String.prototype.toLocaleLowerCase.call(true, 'tr')");

// ---- Number.prototype.toLocaleString.
const nums = [0, -0, 1234.5, -1234567.891, 0.000123, 1e21, 123456789012345680000, NaN, Infinity, -Infinity, 0.5, 1e-7, 42];
for (const loc of locales) for (const n of nums) T(`R = (${Object.is(n, -0) ? "-0" : n}).toLocaleString(${q(loc)})`);
const numOpts = [
  "{ style: 'currency', currency: 'USD' }", "{ style: 'currency', currency: 'BRL' }", "{ style: 'currency', currency: 'EUR' }",
  "{ style: 'currency', currency: 'JPY' }", "{ style: 'currency', currency: 'EUR', currencyDisplay: 'name' }",
  "{ style: 'currency', currency: 'EUR', currencyDisplay: 'code' }", "{ style: 'currency', currency: 'USD', currencySign: 'accounting' }",
  "{ style: 'percent' }", "{ style: 'percent', minimumFractionDigits: 1 }", "{ minimumFractionDigits: 3 }", "{ maximumFractionDigits: 0 }",
  "{ minimumIntegerDigits: 5 }", "{ maximumSignificantDigits: 3 }", "{ useGrouping: false }", "{ notation: 'compact' }",
  "{ notation: 'compact', compactDisplay: 'long' }", "{ notation: 'scientific' }", "{ notation: 'engineering' }",
  "{ style: 'unit', unit: 'kilometer-per-hour' }", "{ style: 'unit', unit: 'liter', unitDisplay: 'long' }", "{ signDisplay: 'always' }",
  "{ signDisplay: 'exceptZero' }", "{ numberingSystem: 'arab' }", "{ numberingSystem: 'deva' }", "{ roundingMode: 'floor', maximumFractionDigits: 0 }",
];
for (const loc of locales) for (const opt of numOpts) T(`R = (-1234567.895).toLocaleString(${q(loc)}, ${opt})`);
T("R = (1234.5).toLocaleString('ar-EG-u-nu-latn')");
T("R = (1234.5).toLocaleString('hi-IN-u-nu-deva')");
T("R = (1234.5).toLocaleString('en-US-u-nu-thai')");
T("R = (1234.5).toLocaleString('de-DE', { style: 'currency' })");
T("R = (1234.5).toLocaleString('de-DE', { style: 'currency', currency: 'xx' })");
T("R = (1234.5).toLocaleString('de-DE', { maximumFractionDigits: 101 })");
T("R = (1234.5).toLocaleString('de-DE', { minimumFractionDigits: 5, maximumFractionDigits: 2 })");
T("R = (1234.5).toLocaleString(['xx', 'pt-BR'])");
T("R = Number.prototype.toLocaleString.call('1')");
T("R = new Number(1234.5).toLocaleString('hi-IN')");
T("R = (1e5).toLocaleString('hi-IN')");
T("R = (12345678).toLocaleString('hi-IN')");

// ---- BigInt.prototype.toLocaleString.
const bigs = ["0n", "-1234567890123456789012345678901234567890n", "123456789n", "10000000000000000000000n"];
for (const loc of locales) for (const b of bigs) T(`R = (${b}).toLocaleString(${q(loc)})`);
for (const opt of ["{ style: 'currency', currency: 'EUR' }", "{ notation: 'compact' }", "{ useGrouping: false }", "{ minimumFractionDigits: 2 }", "{ style: 'percent' }", "{ numberingSystem: 'arab' }"])
  for (const loc of ["en-US", "de-DE", "ar-EG", "hi-IN"]) T(`R = (123456789n).toLocaleString(${q(loc)}, ${opt})`);
T("R = BigInt.prototype.toLocaleString.call(1)");
T("R = BigInt.prototype.toLocaleString.call(5n)");

// ---- Date toLocale*String (sempre UTC).
const dates = ["Date.UTC(2024, 0, 5, 3, 4, 5, 6)", "Date.UTC(2023, 11, 31, 23, 59, 59)", "Date.UTC(1999, 6, 4, 12, 0, 0)", "Date.UTC(2024, 1, 29, 0, 0, 0)", "Date.UTC(1970, 0, 1)", "Date.UTC(2100, 8, 9, 15, 30)"];
for (const loc of locales) for (const d of dates) {
  T(`R = new Date(${d}).toLocaleString(${q(loc)}, { timeZone: 'UTC' })`);
  T(`R = new Date(${d}).toLocaleDateString(${q(loc)}, { timeZone: 'UTC' })`);
  T(`R = new Date(${d}).toLocaleTimeString(${q(loc)}, { timeZone: 'UTC' })`);
}
const dateOpts = [
  "{ dateStyle: 'full' }", "{ dateStyle: 'long' }", "{ dateStyle: 'medium' }", "{ dateStyle: 'short' }", "{ timeStyle: 'full' }", "{ timeStyle: 'short' }",
  "{ dateStyle: 'medium', timeStyle: 'medium' }", "{ weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' }",
  "{ month: 'short', day: '2-digit' }", "{ hour: '2-digit', minute: '2-digit', hour12: false }", "{ hour: 'numeric', hourCycle: 'h23' }",
  "{ era: 'short', year: 'numeric' }", "{ timeZoneName: 'short', hour: 'numeric' }", "{ month: 'narrow' }", "{ year: '2-digit', month: '2-digit' }",
  "{ calendar: 'japanese', dateStyle: 'long' }", "{ calendar: 'islamic', dateStyle: 'long' }", "{ numberingSystem: 'arab', dateStyle: 'short' }",
  "{ dayPeriod: 'long', hour: 'numeric' }", "{ fractionalSecondDigits: 3, second: 'numeric' }",
];
for (const loc of locales) for (const opt of dateOpts) T(`R = new Date(Date.UTC(2024, 0, 5, 15, 4, 5, 678)).toLocaleString(${q(loc)}, { timeZone: 'UTC', ...${opt} })`);
T("R = new Date(NaN).toLocaleString('en-US')");
T("R = new Date(NaN).toLocaleDateString('de-DE')");
T("R = new Date(NaN).toLocaleTimeString('ja-JP')");
T("R = new Date(0).toLocaleString('en-US', { timeZone: 'Asia/Tokyo' })");
T("R = new Date(0).toLocaleString('en-US', { timeZone: 'America/Sao_Paulo' })");
T("R = new Date(0).toLocaleString('en-US', { timeZone: 'Nowhere/City' })");
T("R = new Date(0).toLocaleDateString('en-US', { timeZone: 'UTC', timeStyle: 'short' })");
T("R = new Date(0).toLocaleTimeString('en-US', { timeZone: 'UTC', dateStyle: 'short' })");
T("R = new Date(0).toLocaleString('en-US', { timeZone: 'UTC', dateStyle: 'short', hour: 'numeric' })");
T("R = new Date(8.64e15).toLocaleString('en-US', { timeZone: 'UTC' })");
T("R = new Date(-8.64e15).toLocaleString('en-US', { timeZone: 'UTC' })");
T("R = new Date(0).toLocaleDateString('en-US', { timeZone: 'UTC', hour: 'numeric' })");
T("R = new Date(0).toLocaleTimeString('en-US', { timeZone: 'UTC', year: 'numeric' })");
T("R = Date.prototype.toLocaleString.call({})");
T("R = new Date(0).toLocaleString('en-US-u-hc-h23', { timeZone: 'UTC' })");
T("R = new Date(0).toLocaleString('ja-JP-u-ca-japanese', { timeZone: 'UTC' })");
T("R = new Date(0).toLocaleString('th-TH', { timeZone: 'UTC' })");

// ---- Array.prototype.toLocaleString.
for (const loc of locales) {
  T(`R = [1234.5, 1e6, -0.5].toLocaleString(${q(loc)})`);
  T(`R = [1234.5, new Date(Date.UTC(2024, 0, 5)), 'x', 12345678901234567890n].toLocaleString(${q(loc)}, { timeZone: 'UTC' })`);
  T(`R = [1234.5, 0.5].toLocaleString(${q(loc)}, { style: 'percent' })`);
  T(`R = [null, undefined, 1000].toLocaleString(${q(loc)})`);
  T(`R = [[1000, 2000], [3000]].toLocaleString(${q(loc)})`);
}
T("R = [].toLocaleString()");
T("R = [1, 2, 3].toLocaleString('de-DE', { style: 'currency', currency: 'EUR' })");
T("R = [{ toLocaleString() { return typeof arguments[0] + ':' + typeof arguments[1] } }].toLocaleString('en')");
T("R = [{ toLocaleString() { return arguments.length } }].toLocaleString()");
T("R = [{ toLocaleString() { return arguments.length } }].toLocaleString('en', {})");
T("R = [{ toLocaleString: 1 }].toLocaleString()");
T("R = [{ toLocaleString() { return 5 } }].toLocaleString()");
T("R = Array.prototype.toLocaleString.call({ length: 2, 0: 1000, 1: 2000 }, 'de-DE')");
T("R = Array.prototype.toLocaleString.call('ab')");
T("R = Array.prototype.toLocaleString.call(null)");
T("R = (() => { const a = [1000]; a.push(a); return a.toLocaleString('en') })()");
T("R = new Uint8Array([1, 2, 255]).toLocaleString('hi-IN')");
T("R = new Float64Array([1234.5, 0.5]).toLocaleString('de-DE')");
T("R = new BigInt64Array([1234567n]).toLocaleString('de-DE')");
T("R = new Uint8Array(0).toLocaleString()");

// ---- Intl.getCanonicalLocales.
const tags = [
  "EN-us", "pt-br", "de-de", "JA-jp", "ar-eg", "hi-in", "tr-tr", "en-latn-us", "zh-hans-cn", "sr-cyrl-rs", "es-419", "en-u-ca-gregory",
  "en-u-nu-latn-ca-gregory", "en-u-ca-gregory-nu-latn", "de-u-co-phonebk", "de-u-kn", "de-u-kn-true", "de-u-kf-false", "tr-u-ks-level1",
  "ja-u-ca-japanese-hc-h23", "en-x-private", "x-private", "en-a-bbb-x-a-ccc", "ar-u-nu-arab", "hi-u-nu-deva", "iw", "in", "ji", "no-bok",
  "zh-min-nan", "sgn-be-fr", "en-gb-oed", "i-klingon", "art-lojban", "de-1901", "de-1996-1901", "sl-rozaj-biske", "en-t-ja", "en-us-t-es-latn-m0-ungegn",
  "und", "und-u-ca-gregory", "en-GB-U-CA-Gregory", "en-US-posix", "cmn", "ZH-yue", "ar-arb", "tl", "mo", "sh", "en-aaa", "en-u-ca-islamicc",
  "en-u-ca-ethiopic-amete-alem", "pt-BR-u-ca-gregory", "en-US-u-tz-usnyc", "en-u-rg-gbzzzz", "und-Qaai", "he", "ro-MD", "fil", "pa-pk", "az-latn-az",
];
for (const t of tags) T(`R = ${J}(Intl.getCanonicalLocales(${q(t)}))`);
for (const t of ["", "en_US", "en-", "-en", "en--us", "e", "abcdefghi", "en-u", "en-u-ca", "en-us-us", "en-a", "en-x", "en-1", "en-latn-latn", "en-u-ca-a", "123", "en-u-nu-latn-nu-arab", "en-a-bbb-a-ccc", "en-t-en-t-fr", "en\u0000"])
  T(`R = ${J}(Intl.getCanonicalLocales(${q(t)}))`);
T("R = JSON.stringify(Intl.getCanonicalLocales(['EN', 'en', 'en-US', 'EN-us', 'pt']))");
T("R = JSON.stringify(Intl.getCanonicalLocales())");
T("R = JSON.stringify(Intl.getCanonicalLocales(undefined))");
T("R = JSON.stringify(Intl.getCanonicalLocales([]))");
T("R = JSON.stringify(Intl.getCanonicalLocales(null))");
T("R = JSON.stringify(Intl.getCanonicalLocales(5))");
T("R = JSON.stringify(Intl.getCanonicalLocales([5]))");
T("R = JSON.stringify(Intl.getCanonicalLocales({ length: 2, 0: 'en', 1: 'pt-br' }))");
T("R = JSON.stringify(Intl.getCanonicalLocales(new Intl.Locale('pt-br')))");
T("R = JSON.stringify(Intl.getCanonicalLocales([new Intl.Locale('de-u-co-phonebk')]))");
T("R = JSON.stringify(Intl.getCanonicalLocales(Object('en-us')))");
T("R = JSON.stringify(Intl.getCanonicalLocales([Object('en-us')]))");
T("R = Intl.getCanonicalLocales.length + ':' + Intl.getCanonicalLocales.name");
T("R = Object.prototype.toString.call(Intl.getCanonicalLocales('en'))");
T("R = new Intl.Locale('PT-br').toString()");

// ---- Intl.supportedValuesOf.
for (const k of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"]) {
  T(`R = Intl.supportedValuesOf(${q(k)}).length > 0 ? ${J}(Intl.supportedValuesOf(${q(k)}).slice(0, 12)) : 'vazio'`);
  T(`R = ${J}(Intl.supportedValuesOf(${q(k)}).slice(-6))`);
  T(`R = (() => { const v = Intl.supportedValuesOf(${q(k)}); return v.length + ':' + (v.join() === v.slice().sort().join()) })()`);
}
T("R = Intl.supportedValuesOf('calendar').includes('gregory') + ',' + Intl.supportedValuesOf('calendar').includes('japanese')");
T("R = Intl.supportedValuesOf('currency').includes('BRL') + ',' + Intl.supportedValuesOf('currency').includes('XXX')");
T("R = Intl.supportedValuesOf('timeZone').includes('America/Sao_Paulo') + ',' + Intl.supportedValuesOf('timeZone').includes('UTC')");
T("R = Intl.supportedValuesOf('unit').includes('kilometer-per-hour') + ',' + Intl.supportedValuesOf('unit').includes('percent')");
T("R = Intl.supportedValuesOf('numberingSystem').includes('arab') + ',' + Intl.supportedValuesOf('numberingSystem').includes('deva')");
T("R = Intl.supportedValuesOf('collation').includes('standard') + ',' + Intl.supportedValuesOf('collation').includes('search')");
for (const k of ["", "language", "region", "script", "Calendar", "calendars", "timezone", "numbering", "text", "undefined"]) T(`R = ${J}(Intl.supportedValuesOf(${q(k)}))`);
T("R = Intl.supportedValuesOf()");
T("R = Intl.supportedValuesOf(undefined)");
T("R = Intl.supportedValuesOf(null)");
T("R = Intl.supportedValuesOf({ toString() { return 'calendar' } }).length > 0");
T("R = Intl.supportedValuesOf.length + ':' + Intl.supportedValuesOf.name");
T("R = Array.isArray(Intl.supportedValuesOf('calendar')) + ',' + Object.isFrozen(Intl.supportedValuesOf('calendar'))");
T("R = Intl.supportedValuesOf('calendar') === Intl.supportedValuesOf('calendar')");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "locale-methods-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro, sem o transpilador do bun.
const source_file = path.join(dir, "locale_source.js");
const file = path.join(dir, "locale_case.js");
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
