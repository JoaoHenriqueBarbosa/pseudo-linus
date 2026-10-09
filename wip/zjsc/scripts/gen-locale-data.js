// Mede no bun (JavaScriptCore + ICU) os sete getters de Intl.Locale e gera os dados e o golden.
// Uso, a partir da raiz da crate:
//   bun scripts/gen-locale-data.js
// Escreve:
//   src/runtime/intl_locale_getters_data.rs   tabelas por par língua-região, por região e por língua
//   tests/golden/locale_getters_bun.tsv       TAG e os sete getters serializados, um por coluna
// Os getters: getCalendars, getCollations, getHourCycles, getNumberingSystems, getTimeZones, getTextInfo e
// getWeekInfo. `undefined` (getTimeZones sem região) sai como `undefined`.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRustSource } from "./rust-escape.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const REGION_TAGS = `en-US en-GB en-AU en-CA en-IN en-NZ en-ZA en-IE en-SG pt-BR pt-PT pt-AO es-ES es-MX es-AR es-CO
es-CL es-US fr-FR fr-CA fr-BE fr-CH de-DE de-AT de-CH it-IT it-CH ja-JP ko-KR zh-CN zh-TW zh-HK zh-SG ru-RU ar-SA ar-EG
ar-AE ar-MA he-IL fa-IR fa-AF hi-IN th-TH tr-TR pl-PL nl-NL nl-BE sv-SE da-DK nb-NO fi-FI cs-CZ el-GR hu-HU ro-RO uk-UA
vi-VN id-ID ms-MY bn-BD bn-IN ta-IN ta-LK sw-KE sw-TZ fil-PH ur-PK ur-IN ca-ES sk-SK bg-BG hr-HR sr-RS sl-SI lt-LT lv-LV
et-EE is-IS mr-IN te-IN ne-NP my-MM km-KH am-ET af-ZA ps-AF`.split(/\s+/);
const LANGUAGE_TAGS = `en pt es fr de it ja ko zh ru ar he fa hi th tr pl nl sv da nb fi cs el hu ro uk vi id ms bn ta
sw fil ur ca`.split(/\s+/);
const EXTENSION_TAGS = [
  "en-u-ca-buddhist", "en-US-u-ca-japanese", "ja-JP-u-ca-japanese", "th-TH-u-ca-gregory", "ar-SA-u-ca-gregory",
  "fa-IR-u-ca-persian", "he-IL-u-ca-hebrew", "zh-CN-u-ca-chinese", "en-US-u-hc-h23", "pt-BR-u-hc-h12", "ja-u-hc-h11",
  "de-u-hc-h24", "en-u-nu-thai", "ar-u-nu-latn", "ar-EG-u-nu-arab", "hi-IN-u-nu-deva", "de-u-co-phonebk",
  "es-u-co-trad", "zh-u-co-pinyin", "en-US-u-fw-mon",
];
const TAGS = [...REGION_TAGS, ...LANGUAGE_TAGS, ...EXTENSION_TAGS];

const GETTERS = ["getCalendars", "getCollations", "getHourCycles", "getNumberingSystems", "getTimeZones", "getTextInfo", "getWeekInfo"];

const measure = (tag) => {
  const locale = new Intl.Locale(tag);
  return GETTERS.map((getter) => {
    const value = locale[getter]();
    return value === undefined ? "undefined" : JSON.stringify(value);
  });
};

const rows = TAGS.map((tag) => ({ tag, raw: measure(tag) }));

writeFileSync(
  join(ROOT, "tests/golden/locale_getters_bun.tsv"),
  rows.map((row) => [row.tag, ...row.raw].join("\t") + "\n").join(""),
);

// Tabelas. Cada chave guarda o valor serializado; chave com valores diferentes entre as tags é conflito e sai
// da tabela (quem resolve é o nível seguinte da cadeia: par, região, língua, padrão).
const split = (tag) => {
  const parts = tag.split("-");
  const region = parts.find((part, index) => index > 0 && /^[A-Z]{2}$/.test(part));
  return { language: parts[0], region };
};
const plainRows = rows.filter((row) => !row.tag.includes("-u-"));

const build = (index, keyOf, onlyWith) => {
  const table = new Map();
  const conflicts = new Set();
  for (const row of plainRows) {
    const { language, region } = split(row.tag);
    const key = keyOf(language, region);
    if (key === undefined || (onlyWith && !onlyWith(language, region))) continue;
    const text = row.raw[index];
    if (table.has(key) && table.get(key) !== text) conflicts.add(key);
    else table.set(key, text);
  }
  for (const key of conflicts) table.delete(key);
  return [...table.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
};
const byPair = (language, region) => (region ? `${language}-${region}` : undefined);
const byRegion = (language, region) => region;
const byLanguage = (language, region) => (region ? undefined : language);

const rustStr = (text) => JSON.stringify(text);
const strList = (json) => `&[${JSON.parse(json).map(rustStr).join(", ")}]`;
const numList = (list) => `&[${list.join(", ")}]`;
const emit = (name, type, entries, render) => {
  if (entries.length === 0) return `const ${name}: &[(&str, ${type})] = &[];\n`;
  return `const ${name}: &[(&str, ${type})] = &[\n${entries.map(([key, text]) => `    (${rustStr(key)}, ${render(text)}),`).join("\n")}\n];\n`;
};
const week = (json) => {
  const info = JSON.parse(json);
  return `(${info.firstDay}, ${numList(info.weekend)})`;
};
const direction = (json) => rustStr(JSON.parse(json).direction);

let out = `//! Dados dos getters de \`Intl.Locale\` (\`getCalendars\`, \`getCollations\`, \`getHourCycles\`,
//! \`getNumberingSystems\`, \`getTimeZones\`, \`getTextInfo\`, \`getWeekInfo\`), medidos no bun.
//!
//! Arquivo gerado, não edite à mão. Para regenerar, a partir da raiz da crate:
//!   bun scripts/gen-locale-data.js
//! O mesmo comando regrava \`tests/golden/locale_getters_bun.tsv\`.
//!
//! Calendários, ciclos de hora, fusos e semana saem por região, medidos para todas as regiões que o bun
//! aceita (a região é a da tag ou a do \`maximize\`). Collations, sistemas de numeração e direção seguem o
//! par língua-região (só onde a medição é única), depois a língua, depois o padrão.

`;
// Dados por região: o ICU resolve calendários, ciclos de hora, fusos e semana pela região (a da tag ou a do
// maximize), então a medição cobre TODAS as regiões que o bun aceita (2 letras e 3 dígitos), com `und-REGIÃO`.
// A entrada igual ao valor mais comum (o padrão) não é emitida.
const ALL_REGIONS = [];
for (let a = 65; a < 91; a++) for (let b = 65; b < 91; b++) ALL_REGIONS.push(String.fromCharCode(a, b));
for (let i = 1; i < 1000; i++) ALL_REGIONS.push(String(i).padStart(3, "0"));
const VALID_REGIONS = ALL_REGIONS.filter((region) => {
  try {
    new Intl.Locale(`und-${region}`);
    return true;
  } catch {
    return false;
  }
});
const regionRaw = (index, tag) => measure(tag)[index];
const mode = (values) => {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts.entries()].sort((x, y) => y[1] - x[1] || (x[0] < y[0] ? -1 : 1))[0][0];
};
// [valor padrão, entradas que diferem dele]
const regionTable = (index, valueOf) => {
  const values = VALID_REGIONS.map((region) => [region, valueOf(region)]);
  const fallback = mode(values.map(([, value]) => value));
  return [fallback, values.filter(([, value]) => value !== fallback)];
};
const emitRegion = (name, type, index, render, defaultRender) => {
  const [fallback, entries] = regionTable(index, (region) => regionRaw(index, `und-${region}`));
  return `const ${name}_DEFAULT: ${type} = ${defaultRender(fallback)};\n` + emit(name, type, entries, render);
};
// Ciclo de hora: depende também da língua em poucas regiões (en-CA é h12 e fr-CA é h23; `und-ZZ` é h12 e
// quase toda língua com ZZ é h23). A tabela por região usa o valor da língua sem dados, e as línguas com dados
// que divergem dele entram numa tabela de pares.
// Varredura exaustiva, sem amostra. Todas as línguas de 2 e 3 letras (a-z) são candidatas (18252). Só as que o
// ICU tem dados (`Intl.DateTimeFormat.supportedLocalesOf`, 393 no bun) podem mudar o resultado; as demais caem
// no mesmo caminho de "língua desconhecida" (o valor da região para uma língua sem dados, que NÃO é o de
// `und-REGIÃO`: `und-ZZ` é h12 e `xx-ZZ` é h23). A tabela por região guarda o valor da língua desconhecida
// (conferido contra cinco sondas desconhecidas, o gerador aborta se divergirem) e a de pares guarda toda
// língua com dados que diverge dele, medida em TODAS as regiões aceitas. Custo: ~393 x 1675 medições, ~100 s.
const languagesOfLength = (length) => {
  const out = [];
  const walk = (prefix) => {
    if (prefix.length === length) return void out.push(prefix);
    for (let c = 97; c < 123; c++) walk(prefix + String.fromCharCode(c));
  };
  walk("");
  return out;
};
const CANDIDATE_LANGUAGES = [...languagesOfLength(2), ...languagesOfLength(3)];
const KNOWN_LANGUAGES = [
  "und",
  ...CANDIDATE_LANGUAGES.filter((language) => {
    try {
      return Intl.DateTimeFormat.supportedLocalesOf(language).length > 0;
    } catch {
      return false;
    }
  }),
];
const UNKNOWN_PROBES = ["xx", "qq", "zzz", "abc", "kxx"];
const hourCyclesOf = (tag) => JSON.stringify(new Intl.Locale(tag).getHourCycles());
const hourByRegion = new Map(
  VALID_REGIONS.map((region) => {
    const values = UNKNOWN_PROBES.map((language) => hourCyclesOf(`${language}-${region}`));
    if (new Set(values).size !== 1) throw new Error(`línguas desconhecidas divergem em ${region}: ${values}`);
    return [region, values[0]];
  }),
);
const hourFallback = mode([...hourByRegion.values()]);
const hourRegionEntries = [...hourByRegion.entries()].filter(([, value]) => value !== hourFallback);
const hourPairEntries = [];
for (const region of VALID_REGIONS) {
  for (const language of KNOWN_LANGUAGES) {
    let value;
    try {
      value = hourCyclesOf(`${language}-${region}`);
    } catch {
      continue;
    }
    if (value !== hourByRegion.get(region)) hourPairEntries.push([`${language}-${region}`, value]);
  }
}
console.log(`ciclos de hora: ${KNOWN_LANGUAGES.length} línguas, ${hourPairEntries.length} pares divergentes`);

out += emitRegion("CALENDARS_BY_REGION", "&[&str]", 0, strList, (json) => strList(json));
out += emit("COLLATIONS_BY_PAIR", "&[&str]", build(1, byPair), strList);
out += emit("COLLATIONS_BY_LANGUAGE", "&[&str]", build(1, byLanguage), strList);
out += `const HOUR_CYCLES_BY_REGION_DEFAULT: &[&str] = ${strList(hourFallback)};\n`;
out += emit("HOUR_CYCLES_BY_REGION", "&[&str]", hourRegionEntries, strList);
out += emit("HOUR_CYCLES_BY_PAIR", "&[&str]", hourPairEntries, strList);
out += emit("NUMBERING_SYSTEMS_BY_PAIR", "&[&str]", build(3, byPair), strList);
out += emit("NUMBERING_SYSTEMS_BY_LANGUAGE", "&[&str]", build(3, byLanguage), strList);
out += emitRegion("TIME_ZONES_BY_REGION", "&[&str]", 4, strList, (json) => strList(json));
out += emit("TEXT_DIRECTION_BY_LANGUAGE", "&str", build(5, byLanguage), direction);
out += emitRegion("WEEK_INFO_BY_REGION", "(u8, &[u8])", 6, week, (json) => week(json));

out += `
fn find<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.iter().find(|(candidate, _)| *candidate == key).map(|(_, value)| *value)
}

/// A chave do par \`língua-REGIÃO\`.
fn pair(language: &str, region: Option<&str>) -> Option<String> {
    region.map(|region| format!("{language}-{region}"))
}

/// Os calendários da região (a da tag ou a do \`maximize\`, resolvida pelo chamador).
pub fn calendars(region: &str) -> &'static [&'static str] {
    find(CALENDARS_BY_REGION, region).unwrap_or(CALENDARS_BY_REGION_DEFAULT)
}

pub fn collations(language: &str, region: Option<&str>) -> &'static [&'static str] {
    pair(language, region)
        .and_then(|key| find(COLLATIONS_BY_PAIR, &key))
        .or_else(|| find(COLLATIONS_BY_LANGUAGE, language))
        .unwrap_or(&["emoji", "eor"])
}

/// O ciclo de hora: o par língua-região onde a língua diverge da região, senão a região.
pub fn hour_cycles(language: &str, region: &str) -> &'static [&'static str] {
    find(HOUR_CYCLES_BY_PAIR, &format!("{language}-{region}"))
        .or_else(|| find(HOUR_CYCLES_BY_REGION, region))
        .unwrap_or(HOUR_CYCLES_BY_REGION_DEFAULT)
}

pub fn numbering_systems(language: &str, region: Option<&str>) -> &'static [&'static str] {
    pair(language, region)
        .and_then(|key| find(NUMBERING_SYSTEMS_BY_PAIR, &key))
        .or_else(|| find(NUMBERING_SYSTEMS_BY_LANGUAGE, language))
        .unwrap_or(&["latn"])
}

/// \`[]\` para a região fora da tabela; a tag sem região (\`undefined\` no getter) é decidida pelo chamador.
pub fn time_zones(region: &str) -> &'static [&'static str] {
    find(TIME_ZONES_BY_REGION, region).unwrap_or(TIME_ZONES_BY_REGION_DEFAULT)
}

pub fn text_direction(language: &str) -> &'static str {
    find(TEXT_DIRECTION_BY_LANGUAGE, language).unwrap_or("ltr")
}

/// O primeiro dia (1 segunda a 7 domingo) e os dias do fim de semana.
pub fn week_info(region: &str) -> (u8, &'static [u8]) {
    find(WEEK_INFO_BY_REGION, region).unwrap_or(WEEK_INFO_BY_REGION_DEFAULT)
}
`;
writeRustSource(join(ROOT, "src/runtime/intl_locale_getters_data.rs"), out);
console.log(`${TAGS.length} tags, ${plainRows.length} sem extensão`);
