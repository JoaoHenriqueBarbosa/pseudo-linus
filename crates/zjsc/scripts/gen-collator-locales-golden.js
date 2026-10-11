// Gera tests/golden/collator_locales_bun.tsv: a ordenação característica de cada locale que o PLAN.md
// listava como incompleto no Collator (et, lv, is, vi, az, mt, fr-CA, el), medida no bun.
// Uso: bun scripts/gen-collator-locales-golden.js > tests/golden/collator_locales_bun.tsv
const words = {
  et: ["z", "s", "š", "ž", "t", "a", "u", "õ", "ä", "ö", "ü", "w", "x", "y", "o", "v", "tee", "tõde", "sau", "šašlõkk", "zebra"],
  lv: ["c", "č", "d", "g", "ģ", "i", "y", "j", "k", "ķ", "z", "ž", "s", "š", "l", "ļ", "n", "ņ", "r", "ŗ", "ā", "a"],
  is: ["z", "þ", "ö", "æ", "á", "a", "ð", "d", "é", "e", "í", "i", "ó", "o", "ú", "u", "ý", "y", "þak", "zebra"],
  vi: ["a", "ă", "â", "b", "c", "d", "đ", "e", "ê", "g", "h", "i", "k", "l", "m", "n", "o", "ô", "ơ", "p", "u", "ư", "y", "z", "à", "ả", "ã", "á", "ạ"],
  az: ["c", "ç", "d", "e", "ə", "g", "ğ", "h", "x", "ı", "i", "j", "o", "ö", "s", "ş", "u", "ü", "z"],
  mt: ["c", "ċ", "g", "ġ", "għ", "gz", "h", "ħ", "z", "ż", "a", "b", "ie", "ġie"],
  "fr-CA": ["cote", "côte", "coté", "côté", "cote", "pêche", "péché", "pèche", "pêché"],
  fr: ["cote", "côte", "coté", "côté", "cote", "pêche", "péché", "pèche", "pêché"],
  el: ["α", "ά", "β", "ω", "ώ", "Α", "Ά", "ς", "σ", "γράμμα", "ελληνικά", "Ελλάδα"],
};
// Alfabetos amplos e pares sensíveis de et e lv (a ordem vem do CLDR compilado do icu_collator).
const wide = {
  et: [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "š", "z", "ž",
    "t", "u", "v", "w", "õ", "ä", "ö", "ü", "x", "y", "Õ", "Ä", "Ö", "Ü", "Š", "Ž", "sa", "ša", "za", "ža", "ta",
    "vana", "või", "wa", "õun", "äge", "öö", "üks", "xa", "ya", "Sauna", "saun", "šaun", "Zoo", "žongl",
  ],
  lv: [
    "a", "ā", "b", "c", "č", "d", "e", "ē", "f", "g", "ģ", "h", "i", "ī", "j", "k", "ķ", "l", "ļ", "m", "n", "ņ",
    "o", "p", "q", "r", "ŗ", "s", "š", "t", "u", "ū", "v", "w", "x", "y", "z", "ž", "cik", "čaula", "gads", "ģimene",
    "kaķis", "ķekars", "lauks", "ļaudis", "nakts", "ņemt", "sals", "šalle", "zils", "žogs", "Āda", "ada", "yoga", "ī",
  ],
};
for (const [locale, list] of Object.entries(wide)) {
  words[`${locale}#wide`] = list;
}
const q = (value) => JSON.stringify(value);
const programs = [];
for (const [key, list] of Object.entries(words)) {
  const locale = key.split("#")[0];
  programs.push(`JSON.stringify(${q(list)}.sort(new Intl.Collator(${q(locale)}).compare))`);
  programs.push(`JSON.stringify(${q([...list].reverse())}.sort(new Intl.Collator(${q(locale)}).compare))`);
  programs.push(`new Intl.Collator(${q(locale)}).resolvedOptions().locale`);
}
for (const tag of ["et-EE", "lv-LV", "is-IS", "vi-VN", "az-AZ", "az-Latn", "mt-MT", "el-GR", "fr-CA-u-kn", "fr-FR"]) {
  programs.push(`new Intl.Collator(${q(tag)}).resolvedOptions().locale`);
}
for (const source of programs) {
  const result = JSON.stringify((0, eval)(source));
  const ascii = result.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const sourceAscii = source.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  console.log(`${sourceAscii}\t${ascii}`);
}
