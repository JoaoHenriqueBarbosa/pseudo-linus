// Mede no bun (JavaScriptCore + ICU) o `likelySubtags` do CLDR por sondagem de `Intl.Locale.maximize` e gera
// a tabela e o golden de `maximize` e `minimize`.
// Uso, a partir da raiz da crate:
//   bun scripts/gen-likely-subtags.js
// Escreve:
//   src/runtime/intl_likely_subtags_data.rs   as chaves `lang`, `lang-Script`, `lang-REGION` e `und-...` com a
//                                             tripla (língua, escrita, região) que o ICU devolve
//   tests/golden/likely_subtags_bun.tsv       maximize e minimize de uma grade de tags, no formato de
//                                             `locale_more_bun.tsv` (PROGRAMA, tabulação, resultado)
// O algoritmo (`maximizeModel`) é o mesmo de `intl_locale_data::maximize`, e o gerador só escreve os dados
// depois de conferir que o modelo reproduz o bun em toda a grade de sondagem e em uma grade aleatória.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRustSource } from "./rust-escape.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const letters = "abcdefghijklmnopqrstuvwxyz";

const bunMaximize = (tag) => new Intl.Locale(tag).maximize().toString();
const bunMinimize = (tag) => new Intl.Locale(tag).minimize().toString();
// "pt-Latn-BR" em [língua, escrita, região]; a escrita tem 4 letras, a região 2 letras ou 3 dígitos.
const parse = (tag) => {
  const parts = tag.split("-");
  const result = { language: parts.shift(), script: "", region: "" };
  if (parts[0] && parts[0].length === 4 && /^[A-Za-z]+$/.test(parts[0])) result.script = parts.shift();
  if (parts[0] && (parts[0].length === 2 || /^\d{3}$/.test(parts[0]))) result.region = parts.shift();
  return result;
};

// Língua: todo código de duas e três letras mais `und`.
const languages = ["und"];
for (const a of letters) for (const b of letters) {
  languages.push(a + b);
  for (const c of letters) languages.push(a + b + c);
}
// Só as formas canônicas entram na sondagem (`bel` vira `be`, `DD` vira `DE`): o `Intl.Locale` canonicaliza antes
// do `maximize`, e a tabela vale para a tag já canônica.
const canonicalLanguages = languages.filter((language) => new Intl.Locale(language).toString() === language);
languages.length = 0;
languages.push(...canonicalLanguages);
const regions = [];
for (const a of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") for (const b of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") regions.push(a + b);
for (let number = 1; number < 1000; number += 1) regions.push(String(number).padStart(3, "0"));
const canonicalRegions = regions.filter((region) => new Intl.Locale("en-" + region).toString() === "en-" + region);
regions.length = 0;
regions.push(...canonicalRegions);
const scripts = ("Adlm Arab Armn Avst Bali Bamu Bass Batk Beng Bopo Brah Brai Bugi Buhd Cakm Cans Cari Cham Cher Copt Cprt Cyrl " +
  "Deva Dsrt Egyp Ethi Geor Glag Goth Grek Gujr Guru Hang Hani Hano Hans Hant Hebr Hira Hmng Ital Java Jpan Kali Kana " +
  "Khmr Knda Kore Kthi Lana Laoo Latn Lepc Limb Linb Lisu Lyci Lydi Mand Mlym Mong Mtei Mymr Nkoo Olck Orkh Orya Osma " +
  "Phag Phnx Rjng Runr Samr Saur Shaw Sinh Sund Sylo Syrc Tagb Tale Talu Taml Tavt Telu Tfng Tglg Thaa Thai Tibt Ugar " +
  "Vaii Xpeo Xsux Yiii Zinh Zyyy Zzzz Zmth Zsym Qaaa").split(" ");

// 1. As chaves de língua: o que o ICU devolve para `lang` sozinha, quando muda.
const table = new Map(); // chave => [língua, escrita, região]
const tripleOf = (tag) => { const p = parse(tag); return [p.language, p.script, p.region]; };
for (const language of languages) {
  let out;
  try { out = bunMaximize(language); } catch { continue; }
  if (out !== language) table.set(language, tripleOf(out));
}
// Os apelidos que o `maximize` troca (a região `QU` vira `EU`, `AN` vira `CW`; a língua `dut` vira `nl`):
// valem mesmo com língua, escrita e região completas.
const regionAliases = new Map();
const rawRegions = [...regions];
for (const region of rawRegions) {
  if (region === "ZZ") continue;
  const out = parse(bunMaximize(`en-Latn-${region}`)).region;
  if (out !== region) regionAliases.set(region, out);
}
// Com língua, escrita e região completas o ICU também troca a língua apelido (`pmk-Latn-US` vira `crr-Latn-US`),
// o que `maximize` da língua sozinha não mostra.
const fullLanguageAliases = new Map();
for (const language of languages) {
  const out = parse(bunMaximize(`${language}-Latn-US`)).language;
  if (language !== "und" && out !== language) fullLanguageAliases.set(language, out);
}
const languageAlias = (language) => {
  const entry = table.get(language);
  return entry && language !== "und" ? entry[0] : language;
};
const aliasLanguages = new Set(languages.filter((language) => languageAlias(language) !== language));
for (const region of regionAliases.keys()) regions.splice(regions.indexOf(region), 1);

// O modelo do algoritmo (`uloc_addLikelySubtags`, ICU 64+): ZZ e Zzzz valem como ausentes; com língua, escrita e
// região o tag fica como veio; senão a primeira chave achada entre `L-S-R`, `L-S`, `L-R`, `L` dá a tripla, que só
// preenche o que falta; sem chave nenhuma a tag fica como veio.
// `preserve` é o `maximize` interno do `minimize`: sem apelidos, e a língua da tag não muda (só `und` toma a da entrada).
const maximizeModel = (language, script, region, entries, preserve = false) => {
  if (language === "") language = "und";
  if (!preserve) language = languageAlias(language);
  const originalRegion = region;
  if (!preserve) region = regionAliases.get(region) ?? region;
  // Sem chave nenhuma a tag fica como veio, sem o apelido da região (`fb-PZ`).
  const original = [language, script, originalRegion];
  if (script === "Zzzz") script = "";
  if (region === "ZZ") region = "";
  if (language === "") language = "und";
  if (language !== "und" && script && region) return [preserve ? language : fullLanguageAliases.get(language) ?? language, script, region];
  const keys = [];
  if (script && region) keys.push(`${language}-${script}-${region}`);
  if (script) keys.push(`${language}-${script}`);
  if (region) keys.push(`${language}-${region}`);
  keys.push(language);
  for (const key of keys) {
    const found = entries.get(key);
    if (found) return [preserve && language !== "und" ? language : found[0], script || found[1], region || found[2]];
  }
  return original;
};
const modelString = (triple) => triple.filter(Boolean).join("-");

// 2. As chaves com escrita ou região: só as que o ICU trata diferente do preenchimento pela chave da língua.
// QUICK=1 sonda só as línguas de duas letras (menos de um minuto); sem ela, as ~7600 línguas do CLDR (6 minutos).
const known = languages.filter(
  (language) => table.has(language) && !aliasLanguages.has(language) && (!process.env.QUICK || language.length <= 2 || language === "und"),
);
const probe = (language, script, region) => {
  const tag = [language, script, region].filter(Boolean).join("-");
  let out;
  try { out = bunMaximize(tag); } catch { return; }
  const model = modelString(maximizeModel(language, script, region, table));
  if (out !== model) {
    const triple = tripleOf(out);
    table.set(tag, triple);
  }
};
for (const language of known) {
  for (const script of scripts) probe(language, script, "");
  for (const region of regions) probe(language, "", region);
}
// `und-Script-REGION` existe no CLDR (`und-Latn-RU` é `krl`): a grade inteira para `und`.
for (const script of scripts) for (const region of regions) probe("und", script, region);

// 3. Confere o modelo contra o bun em uma grade aleatória (língua conhecida ou não, escrita, região).
let seed = 20261009;
const random = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; };
const pick = (list) => list[Math.floor(random() * list.length)];
const mismatches = [];
for (let round = 0; round < 60000; round += 1) {
  const language = random() < 0.8 ? pick(known) : pick(languages);
  const script = random() < 0.5 ? pick(scripts) : "";
  const region = random() < 0.5 ? pick(random() < 0.1 ? rawRegions : regions) : "";
  const tag = [language, script, region].filter(Boolean).join("-");
  let out;
  try { out = bunMaximize(tag); } catch { continue; }
  const model = modelString(maximizeModel(language, script, region, table));
  if (out !== model) mismatches.push(`${tag}: bun ${out}, modelo ${model}`);
}
if (mismatches.length) {
  console.error(`${mismatches.length} divergências do modelo:\n${mismatches.slice(0, 40).join("\n")}`);
  process.exit(1);
}

// 4. minimize: o ICU tira a escrita e a região que o `maximize` repõe, na ordem língua, língua + região,
// língua + escrita (`createTagStringWithAlternates`). Confere o modelo na mesma grade.
// A língua do candidato é a da tag (`zir` fica `zir`, não `scv`), salvo `und`, que toma a da forma máxima; a
// região e a escrita são as da tag, ou as da forma máxima quando a tag não as traz; sem redução a tag fica como veio.
const minimizeModel = (language, script, region) => {
  if (language === "") language = "und";
  const max = maximizeModel(language, script, region, table, true);
  const same = (candidate) => modelString(maximizeModel(candidate[0], candidate[1], candidate[2], table, true)) === modelString(max);
  const keptLanguage = language === "und" ? max[0] : language;
  const candidates = [
    [keptLanguage, "", ""],
    [keptLanguage, "", region || max[2]],
    [keptLanguage, script || max[1], ""],
  ];
  for (const candidate of candidates) if (same(candidate)) return candidate;
  return [keptLanguage, script, region];
};
const minimizeMismatches = [];
for (let round = 0; round < 30000; round += 1) {
  const language = random() < 0.8 ? pick(known) : pick(languages);
  const script = random() < 0.5 ? pick(scripts) : "";
  const region = random() < 0.5 ? pick(random() < 0.1 ? rawRegions : regions) : "";
  const tag = [language, script, region].filter(Boolean).join("-");
  let out;
  try { out = bunMinimize(tag); } catch { continue; }
  const model = modelString(minimizeModel(language, script, region));
  if (out !== model) minimizeMismatches.push(`${tag}: bun ${out}, modelo ${model}`);
}
if (minimizeMismatches.length) {
  console.error(`${minimizeMismatches.length} divergências do minimize:\n${minimizeMismatches.slice(0, 40).join("\n")}`);
  process.exit(1);
}

// 5. Dados em Rust, ordenados por chave (bytes) para busca binária.
const entries = [...table.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
const body = entries.map(([key, [language, script, region]]) => `    (${JSON.stringify(key)}, ${JSON.stringify(language)}, ${JSON.stringify(script)}, ${JSON.stringify(region)}),`).join("\n");
const languageAliasBody = [...fullLanguageAliases.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(([from, to]) => `    (${JSON.stringify(from)}, ${JSON.stringify(to)}),`).join("\n");
const aliasBody = [...regionAliases.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(([from, to]) => `    (${JSON.stringify(from)}, ${JSON.stringify(to)}),`).join("\n");
writeRustSource(
  join(root, "src/runtime/intl_likely_subtags_data.rs"),
  `//! Gerado por \`scripts/gen-likely-subtags.js\` (bun 1.4.2, ICU completo): o \`likelySubtags\` do CLDR como o
//! \`uloc_addLikelySubtags\` o aplica. Não editar à mão. O algoritmo está em \`intl_locale_data::maximize\`.

/// (chave, língua, escrita, região), ordenada pela chave. A chave é \`lang\`, \`lang-Script\`, \`lang-REGION\` ou
/// \`und-Script-REGION\` (as de escrita e região só constam quando diferem do preenchimento pela chave da língua).
pub const LIKELY_SUBTAGS: [(&str, &str, &str, &str); ${entries.length}] = [
${body}
];

/// As regiões que o \`maximize\` troca por outra (\`QU\` vira \`EU\`, \`AN\` vira \`CW\`), ordenadas. A língua apelido
/// (\`dut\`, \`tl\`) troca pela da própria entrada \`lang\` de \`LIKELY_SUBTAGS\`.
pub const REGION_ALIASES: [(&str, &str); ${regionAliases.size}] = [
${aliasBody}
];

/// As línguas apelido que o \`maximize\` troca quando a tag já traz escrita e região (\`pmk\` vira \`crr\`), ordenadas.
pub const LANGUAGE_ALIASES: [(&str, &str); ${fullLanguageAliases.size}] = [
${languageAliasBody}
];
`,
);

// 6. Golden: maximize e minimize, no formato de \`locale_more_bun.tsv\`.
const q = JSON.stringify;
const goldenTags = new Set(["und", "zh-TW", "sr-Cyrl", "en-Latn-US", "und-Hant", "en-ZZ", "en-Zzzz", "xx", "xx-Cyrl", "xx-RU",
  "und-AQ", "und-Latn-RU", "und-Zyyy", "uz-AF", "uz-Cyrl", "en-Shaw", "tl", "cmn", "sh", "no", "und-Zzzz-US", "ca-ES-valencia",
  "en-x-foo", "en-US-u-ca-gregory", "en-001", "pt-t-en"]);
for (let round = 0; round < 700; round += 1) {
  const language = random() < 0.85 ? pick(known) : pick(languages);
  const script = random() < 0.4 ? pick(scripts) : "";
  const region = random() < 0.5 ? pick(random() < 0.1 ? rawRegions : regions) : "";
  goldenTags.add([language, script, region].filter(Boolean).join("-"));
}
const lines = [];
for (const tag of goldenTags) {
  for (const method of ["maximize", "minimize"]) {
    const expression = `new Intl.Locale(${q(tag)}).${method}().toString()`;
    let result;
    try { result = eval(expression); } catch (error) { result = `ERR ${error.name}: ${error.message}`; }
    lines.push(`${expression}\t${result}`);
  }
}
writeFileSync(join(root, "tests/golden/likely_subtags_bun.tsv"), lines.join("\n") + "\n");
console.log(`${entries.length} chaves, golden ${lines.length} linhas`);
