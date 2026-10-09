// Mede no bun (JavaScriptCore + ICU) os locales disponíveis do Intl e gera os dados e o golden.
// Uso, a partir da raiz da crate:
//   bun scripts/gen-available-locales.js
// Escreve:
//   src/runtime/intl_available_locales_data.rs   a lista única de línguas disponíveis (as oito classes que
//                                                compartilham `intlAvailableLocales`) e a do Collator
//   tests/golden/available_locales_bun.tsv      120 tags x 10 classes: locale resolvido, supportedLocalesOf,
//                                                Intl.Locale e a mensagem do RangeError
// Sondagem: todo código de duas e três letras (aa..zzz) em `supportedLocalesOf`; o Collator tem lista
// própria (`ucol_getAvailable`), sondada pelo `resolvedOptions().locale`, que também revela os locales
// com escrita ou região que ele lista por si.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRustSource } from "./rust-escape.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const letters = "abcdefghijklmnopqrstuvwxyz";
const codes = [];
for (const a of letters) for (const b of letters) {
  codes.push(a + b);
  for (const c of letters) codes.push(a + b + c);
}

const classes = ["Collator", "DateTimeFormat", "DisplayNames", "DurationFormat", "ListFormat", "NumberFormat", "PluralRules", "RelativeTimeFormat", "Segmenter"];
const shared = classes.filter((name) => name !== "Collator");
const available = new Set();
for (const name of shared) {
  for (const code of codes) if (Intl[name].supportedLocalesOf([code]).length) available.add(code);
}
const collatorResolved = (tag) => new Intl.Collator(tag).resolvedOptions().locale;
const collatorLanguages = new Set();
for (const code of codes) {
  const resolved = collatorResolved(code);
  if (resolved !== "en-US" || code === "en") collatorLanguages.add(resolved.split("-")[0]);
}

// Locales do Collator com escrita ou região próprias: língua + escrita, língua + região, língua + escrita + região.
const regions = [];
for (const a of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") for (const b of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") regions.push(a + b);
const scripts = ["Latn", "Cyrl", "Arab", "Hans", "Hant", "Guru", "Deva", "Beng", "Grek", "Hebr", "Thai", "Jpan", "Kore", "Mong", "Tfng", "Vaii", "Olck"];
const collatorRegional = new Set();
for (const language of collatorLanguages) {
  const scriptHits = [];
  for (const script of scripts) {
    const resolved = collatorResolved(`${language}-${script}`);
    if (resolved === `${language}-${script}`) { collatorRegional.add(resolved); scriptHits.push(script); }
  }
  for (const region of regions) {
    const resolved = collatorResolved(`${language}-${region}`);
    if (resolved === `${language}-${region}`) collatorRegional.add(resolved);
  }
  for (const script of scriptHits) for (const region of regions) {
    const resolved = collatorResolved(`${language}-${script}-${region}`);
    if (resolved === `${language}-${script}-${region}`) collatorRegional.add(resolved);
  }
}

// Locales com escrita ou região próprias nas oito classes que compartilham `intlAvailableLocales`
// (`en-GB`, `zh-Hant`, `zh-Hant-TW`): o `bestAvailableLocale` do JSC corta subtags até achar um da lista
// exata, então `en-ZZ` cai em `en` e `zh-Hant-ZZ` em `zh-Hant`. Sondado pelo `resolvedOptions().locale`
// do NumberFormat: língua + escrita, língua + região (duas letras e três dígitos), língua + escrita + região.
const sharedResolved = (tag) => new Intl.NumberFormat(tag).resolvedOptions().locale;
const sharedRegions = [...regions];
for (let number = 1; number < 1000; number += 1) sharedRegions.push(String(number).padStart(3, "0"));
const sharedRegional = new Set();
for (const language of available) {
  const scriptHits = [];
  for (const script of scripts) {
    const resolved = sharedResolved(`${language}-${script}`);
    if (resolved === `${language}-${script}`) { sharedRegional.add(resolved); scriptHits.push(script); }
  }
  for (const region of sharedRegions) {
    const resolved = sharedResolved(`${language}-${region}`);
    if (resolved === `${language}-${region}`) sharedRegional.add(resolved);
  }
  for (const script of scriptHits) for (const region of sharedRegions) {
    const resolved = sharedResolved(`${language}-${script}-${region}`);
    if (resolved === `${language}-${script}-${region}`) sharedRegional.add(resolved);
  }
}

const rustList = (items) => {
  const lines = [];
  for (let index = 0; index < items.length; index += 10) {
    lines.push("    " + items.slice(index, index + 10).map((item) => JSON.stringify(item)).join(", ") + ",");
  }
  return lines.join("\n");
};
const sortedAvailable = [...available].sort();
const sortedShared = [...sharedRegional].sort();
const sortedCollator = [...collatorLanguages].sort();
const sortedRegional = [...collatorRegional].sort();
writeRustSource(
  join(root, "src/runtime/intl_available_locales_data.rs"),
  `//! Gerado por \`scripts/gen-available-locales.js\` (bun 1.4.2, ICU completo): os locales disponíveis do Intl.
//! Não editar à mão. O ponto único de resolução é \`intl_locale_data::best_available_locale\` (e \`best_available_by\`).

/// As línguas de \`intlAvailableLocales\` (DateTimeFormat, DisplayNames, DurationFormat, ListFormat,
/// NumberFormat, PluralRules, RelativeTimeFormat e Segmenter medem o mesmo conjunto), com os códigos de três letras e os apelidos (\`iw\`, \`in\`, \`ji\`, \`mo\`, \`jw\`). Região e escrita
/// próprias ficam em \`AVAILABLE_LOCALES\`.
pub const AVAILABLE_LANGUAGES: [&str; ${sortedAvailable.length}] = [
${rustList(sortedAvailable)}
];

/// Os locales com escrita ou região das mesmas classes (\`en-GB\`, \`zh-Hant\`, \`zh-Hant-TW\`). Uma região fora
/// da lista (\`en-ZZ\`) não é disponível: o \`bestAvailableLocale\` corta até a língua.
pub const AVAILABLE_LOCALES: [&str; ${sortedShared.length}] = [
${rustList(sortedShared)}
];

/// As línguas que o colador do bun conhece (\`ucol_getAvailable\`); o resto resolve para \`en-US\`.
pub const COLLATOR_LANGUAGES: [&str; ${sortedCollator.length}] = [
${rustList(sortedCollator)}
];

/// Os locales com região ou escrita que o colador lista por si; os outros caem na língua.
pub const COLLATOR_LOCALES: [&str; ${sortedRegional.length}] = [
${rustList(sortedRegional)}
];
`,
);

// Golden: 120 tags.
const tags = [
  "en", "en-US", "en-GB", "en-AU", "en-CA", "en-IN", "en-001", "en-US-u-hc-h23", "en-US-u-nu-arab", "en-u-ca-japanese",
  "pt", "pt-BR", "pt-PT", "pt-AO", "pt-MZ", "es", "es-ES", "es-MX", "es-419", "es-AR",
  "fr", "fr-FR", "fr-CA", "fr-CH", "fr-XX", "de", "de-DE", "de-AT", "de-CH", "de-CH-1996",
  "it", "it-IT", "nl", "nl-BE", "sv", "sv-FI", "nb", "no", "nn", "da",
  "fi", "is", "pl", "cs", "sk", "hu", "ro", "bg", "ru", "uk",
  "be", "sr", "sr-Latn", "sr-Cyrl", "sr-Latn-RS", "hr", "bs", "sl", "mk", "sq",
  "el", "tr", "az", "kk", "uz", "ka", "hy", "he", "iw", "ar",
  "ar-EG", "ar-SA", "fa", "ur", "hi", "bn", "ta", "te", "mr", "gu",
  "pa", "pa-Guru", "ne", "si", "th", "vi", "id", "in", "ms", "fil",
  "ja", "ja-JP", "ja-JP-u-ca-japanese", "ko", "ko-KR", "zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh-Hant",
  "zh-Hant-TW", "zh-HK", "yue", "sw", "am", "zu", "af", "eu", "ca", "gl",
  "cy", "ga", "gd", "lb", "mt", "xx", "tlh", "und", "root", "i-klingon",
];
const programs = [];
const guarded = (expression) => `(()=>{try{return ${expression}}catch(e){return e.name+": "+e.message}})()`;
for (const tag of tags) {
  const quoted = JSON.stringify(tag);
  for (const name of classes) {
    const options = name === "DisplayNames" ? ', {type: "region"}' : "";
    programs.push(guarded(`new Intl.${name}(${quoted}${options}).resolvedOptions().locale`));
  }
  programs.push(guarded(`Intl.NumberFormat.supportedLocalesOf([${quoted}])`));
  programs.push(guarded(`Intl.Collator.supportedLocalesOf([${quoted}])`));
  programs.push(guarded(`new Intl.Locale(${quoted}).toString()`));
}
const lines = programs.map((program) => `${program}\t${JSON.stringify(eval(program))}`);
writeFileSync(join(root, "tests/golden/available_locales_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.log(`disponíveis ${sortedAvailable.length}, colador ${sortedCollator.length} + ${sortedRegional.length}, golden ${lines.length} linhas`);
