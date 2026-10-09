const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/resolved_locale_bun.tsv: `resolvedOptions().locale` das nove classes do Intl para
// 20 tags, medido no bun. Colunas: programa de uma linha (ASCII) e o resultado em JSON.
// tests/intl_resolved_locale_bun_golden.rs roda cada programa na engine e compara.
// Uso: bun scripts/gen-resolved-locale-golden.js > tests/golden/resolved_locale_bun.tsv
const tags = [
  "sv", "ar-EG", "ru", "pl", "zh-Hans-CN", "es-419", "pt-PT", "de-AT", "en-GB", "ja-JP-u-ca-japanese",
  "xx", "tlh", "und", "he", "iw", "fil", "sr-Latn", "nb", "no", "zh-TW",
];
const classes = [
  "NumberFormat", "DateTimeFormat", "PluralRules", "ListFormat", "RelativeTimeFormat", "DisplayNames", "Collator",
  "Segmenter", "DurationFormat",
];
for (const tag of tags) {
  for (const name of classes) {
    const options = name === "DisplayNames" ? ', {type: "region"}' : "";
    const program = `new Intl.${name}(${JSON.stringify(tag)}${options}).resolvedOptions().locale`;
    const result = new Intl[name](tag, name === "DisplayNames" ? { type: "region" } : undefined).resolvedOptions().locale;
    emitRow(program + "\t" + JSON.stringify(result));
  }
}
