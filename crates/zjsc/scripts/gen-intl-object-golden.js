const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/intl_object_bun.tsv: Intl.getCanonicalLocales, Intl.supportedValuesOf e
// supportedLocalesOf dos construtores, medidos no bun. Colunas: programa de uma linha (ASCII) e o
// resultado em JSON ASCII. tests/intl_object_bun_golden.rs roda cada programa na engine e compara.
// Uso: bun scripts/gen-intl-object-golden.js > tests/golden/intl_object_bun.tsv
//      bun scripts/gen-intl-object-golden.js --rust-data > src/runtime/intl_supported_values_data.rs
//      (a segunda forma é codegen: as listas completas de supportedValuesOf, como o ICU do bun as dá)
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

if (process.argv.includes("--rust-data")) {
  const out = [];
  out.push("//! Gerado por `scripts/gen-intl-object-golden.js --rust-data` (bun 1.4.2, ICU completo): as listas de");
  out.push("//! `Intl.supportedValuesOf` que o ICU reporta, em ordem de ponto de código. Não editar à mão.");
  out.push("");
  for (const [name, key] of [["CURRENCIES", "currency"], ["NUMBERING_SYSTEMS", "numberingSystem"], ["TIME_ZONES", "timeZone"], ["UNITS", "unit"]]) {
    const values = Intl.supportedValuesOf(key);
    out.push(`pub const ${name}: [&str; ${values.length}] = [`);
    for (let i = 0; i < values.length; i += 8) {
      out.push("    " + values.slice(i, i + 8).map((v) => JSON.stringify(v)).join(", ") + ",");
    }
    out.push("];");
    out.push("");
  }
  console.log(out.join("\n").trimEnd());
  process.exit(0);
}

const programs = [];
const add = (source) => programs.push(source);

// supportedValuesOf: a lista inteira por chave, e as bordas.
for (const key of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"]) {
  add(`Intl.supportedValuesOf(${q(key)})`);
}
for (const key of ["x", "", "Calendar", "timezone", "numberingsystem", "units", "region", "script", "language"]) {
  add(`(() => { try { return Intl.supportedValuesOf(${q(key)}); } catch (e) { return e.name + ": " + e.message; } })()`);
}
add(`(() => { try { return Intl.supportedValuesOf(); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.supportedValuesOf(undefined); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`Intl.supportedValuesOf({ toString() { return "unit"; } }).length`);
add(`Intl.supportedValuesOf.length`);
add(`Intl.supportedValuesOf.name`);
add(`Intl.getCanonicalLocales.length`);
add(`Intl.getCanonicalLocales.name`);

// getCanonicalLocales: canonicalização de tags.
const tags = [
  "en-us", "EN-latn-us", "zh-hant-tw", "iw", "in", "sh", "art-lojban", "en-u-ca-gregory-nu-latn", "x-private", "i-klingon",
  "", "en_US", "en-", "a", "toolongsubtag1", "en-US-u", "en-u-ca-gregory-ca-buddhist", "en-a-bbb-x-a-ccc", "ja-jp-u-ca-japanese",
  "zh-cmn-hans", "no-bok", "he-IL", "tl", "mo", "ji", "yi", "pt-br", "PT-BR", "en-gb-oed", "sgn-be-fr", "zh-min-nan", "ar-u-nu-arab",
  "de-u-co-phonebk", "en-t-hi", "en-x-foo", "und", "und-Latn", "en-latn", "ES-419", "sr-cyrl-rs", "fr-ca-u-hc-h23", "en-u-hc-h12-ca-gregory",
  "en-US-POSIX", "ca-valencia", "en-1996-1994", "en-a", "en-a-b", "de-1901", "tlh", "cmn", "zh-yue", "no-nyn", "en-u-kn", "en-u-kn-true",
  "en-u-nu-latn-ca-gregory", "iw-u-ca-gregory", "hy-arevela", "sv-fi", "az-latn-az", "uz-uz", "pa-pk", "ru-ru", "ro-md", "tgl", "jw", "swc",
  "en-u-ca-islamicc", "en-u-ca-ethiopic-amete-alem", "en-u-ms-imperial", "en-u-tz-usnyc", "en-u-va-posix", "en-u-rg-gbzzzz",
  " en", "en ", "en-US-", "-en", "en--US", "en-u-", "en-u-ca", "e", "eng", "engl", "englis", "english", "en-12", "en-123", "en-1234", "en-12345",
  "en-Latn-Latn", "en-US-US", "en-US-GB", "en-u-ca-gregory-u-nu-latn", "en-a-b-a-c", "i-default", "i-enochian", "x-whatever-y",
];
for (const tag of tags) {
  add(`(() => { try { return Intl.getCanonicalLocales(${q(tag)}); } catch (e) { return e.name + ": " + e.message; } })()`);
}
// Listas: duplicatas, ordem, objetos, não-strings.
add(`Intl.getCanonicalLocales(["en-US", "EN-us", "pt-br"])`);
add(`Intl.getCanonicalLocales(["pt-br", "en", "PT-BR", "en"])`);
add(`Intl.getCanonicalLocales()`);
add(`Intl.getCanonicalLocales(undefined)`);
add(`Intl.getCanonicalLocales([])`);
add(`Intl.getCanonicalLocales(new Intl.Locale("en-us"))`);
add(`Intl.getCanonicalLocales([new Intl.Locale("pt-br"), "en"])`);
add(`Intl.getCanonicalLocales({ length: 2, 0: "en", 1: "fr-ca" })`);
add(`Intl.getCanonicalLocales({})`);
add(`(() => { try { return Intl.getCanonicalLocales(null); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales(1); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales([1]); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales(["en", null]); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales(["en", "x"]); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales(Symbol()); } catch (e) { return e.name + ": " + e.message; } })()`);
add(`(() => { try { return Intl.getCanonicalLocales(true); } catch (e) { return e.name + ": " + e.message; } })()`);

// supportedLocalesOf dos construtores: o conjunto disponível depende do ICU, então só entram tags
// de en e pt (os locales com dados na porta) e o comportamento de erro e canonicalização.
const ctors = ["Collator", "DateTimeFormat", "DisplayNames", "ListFormat", "NumberFormat", "PluralRules", "RelativeTimeFormat", "Segmenter", "DurationFormat"];
for (const name of ctors) {
  for (const arg of [`"en-us"`, `["EN-us", "pt-br"]`, `["zh-xx-yy-zz"]`, `["iw"]`, `["en-u-ca-gregory"]`, `["x-private"]`, `[]`, `undefined`, `["en", "en-US", "en"]`, `[1]`, `null`]) {
    add(`(() => { try { return Intl.${name}.supportedLocalesOf(${arg}); } catch (e) { return e.name + ": " + e.message; } })()`);
  }
  add(`(() => { try { return Intl.${name}.supportedLocalesOf("en", null); } catch (e) { return e.name + ": " + e.message; } })()`);
  add(`(() => { try { return Intl.${name}.supportedLocalesOf("en", { localeMatcher: "x" }); } catch (e) { return e.name + ": " + e.message; } })()`);
  add(`(() => { try { return Intl.${name}.supportedLocalesOf("en", { localeMatcher: "lookup" }); } catch (e) { return e.name + ": " + e.message; } })()`);
  add(`Intl.${name}.supportedLocalesOf.length`);
  add(`Intl.${name}.supportedLocalesOf.name`);
}
add(`typeof Intl.Locale.supportedLocalesOf`);
add(`Object.prototype.toString.call(Intl)`);

for (const source of programs) {
  let result;
  try {
    const value = (0, eval)(source);
    result = JSON.stringify(value);
  } catch (error) {
    result = "throw";
  }
  const ascii = String(result).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  emitRow(source + "\t" + ascii);
}
