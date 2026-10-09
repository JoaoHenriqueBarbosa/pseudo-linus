// Gera as tabelas do Intl.DisplayNames dos 38 locales de intl_available_locales (en pt es fr de ja ru zh ...) medindo o bun (JavaScriptCore + ICU):
//   src/runtime/intl_display_names_data_more.rs  (as tabelas, uma por língua)
//   tests/golden/display_names_bun.tsv           (o golden de tests/display_names_bun_golden.rs)
// Os códigos medidos são os que as tabelas de en de src/runtime/intl_display_names_data.rs já têm.
// Colunas do tsv: locale, type, style, languageDisplay, código, resultado (`undefined` se não há nome).
// Tudo é medido com `fallback: "none"`, então um código sem dado aparece como ausente.
// Uso (na raiz do crate): bun scripts/gen-display-names-data.js
const fs = require("fs");
const path = require("path");
const { escapeDashes, writeRustSource } = require("./rust-escape.js");

const root = path.join(__dirname, "..");
const dataSource = fs.readFileSync(path.join(root, "src/runtime/intl_display_names_data.rs"), "utf8");
const LOCALES = ["en", "en-GB", "pt", "pt-PT", "es", "es-MX", "fr", "fr-CA", "de", "de-AT", "it", "ja", "ko", "zh", "zh-TW", "ar", "fa", "he", "hi", "th", "tr", "pl", "nl", "sv", "da", "nb", "fi", "cs", "el", "id", "vi", "uk", "ru", "ro", "hu", "bg", "hr", "sr",
  // Segunda leva: línguas com Collator ou NumberFormat já suportados.
  "sk", "sl", "lt", "lv", "et", "sw", "ta", "te", "ur", "bn", "ca", "eu", "gl", "ms", "is", "ga", "af", "sq", "mk", "be", "ka", "hy", "az", "kk", "uz", "mr", "gu", "kn", "ml", "ne", "si",
  // Terceira leva: medidas no bun (resolvedOptions().locale igual ao pedido, não caem para en).
  "am", "my", "km", "lo", "mn", "ps", "sd", "so", "fil", "ha", "yo", "zu", "xh", "cy", "gd", "lb", "mt", "fo", "ky", "tg", "tk", "tt", "ku", "or", "as",
  // Quarta leva: pa (gurmukhi) e a escrita cirílica do uzbeque (uz-Cyrl; uz e uz-Latn compartilham a tabela uz).
  "pa", "uz-Cyrl"];
// Locales distinguidos pela escrita (subtag de quatro letras), que `table_for_locale` não acha pela região.
const SCRIPT_LOCALES = ["uz-Cyrl"];
// Locales regionais guardam só as linhas que diferem da tabela do pai.
const PARENTS = { "en-GB": "en", "pt-PT": "pt", "es-MX": "es", "fr-CA": "fr", "de-AT": "de", "zh-TW": "zh" };
const ident = (locale) => locale.toUpperCase().replace("-", "_");
const allValid = (type, length) => {
  const found = [];
  const letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
  for (let i = 0; i < letters.length ** 2; i++) {
    const code = letters[Math.floor(i / 26)] + letters[i % 26];
    const probe = type === "language" ? code.toLowerCase() : code;
    try { if (new Intl.DisplayNames(["en"], { type, fallback: "none" }).of(probe)) found.push(probe); } catch (error) { /* ignora */ }
  }
  return found;
};

/** Os códigos (primeira string de cada linha) da constante `name` do arquivo de dados em inglês. */
function codesOf(name) {
  const start = dataSource.indexOf(`const ${name}:`);
  if (start < 0) throw new Error(`constante ${name} não encontrada`);
  const end = dataSource.indexOf("\n];", start);
  const codes = [];
  for (const match of dataSource.slice(start, end).matchAll(/^\s*\("([^"]+)"/gm)) codes.push(match[1]);
  return codes;
}

const languageCodes = [...new Set([...codesOf("LANGUAGES"), ...allValid("language")])];
const dialectKeys = codesOf("DIALECTS");
const EXTRA_SCRIPTS = ["Arab", "Armn", "Beng", "Cans", "Cher", "Copt", "Cyrl", "Deva", "Ethi", "Geor", "Glag", "Goth", "Grek", "Gujr", "Guru", "Hang", "Hani", "Hans", "Hant", "Hebr", "Jpan", "Khmr", "Knda", "Kore", "Laoo", "Latn", "Mlym", "Mong", "Mymr", "Nkoo", "Ogam", "Orya", "Runr", "Sinh", "Syrc", "Taml", "Telu", "Tfng", "Thaa", "Thai", "Tibt", "Vaii", "Zyyy", "Zzzz", "Brai", "Java", "Sund", "Bali", "Zmth", "Zsym", "Zxxx"];
const scriptCodes = [...new Set([...codesOf("SCRIPTS"), ...EXTRA_SCRIPTS])].filter((code) => measure("en", "script", "long", code) !== undefined);
const regionCodes = [...new Set([...codesOf("REGIONS"), ...codesOf("SHORT_REGIONS"), ...allValid("region")])];
const currencyCodes = [...new Set([...codesOf("CURRENCIES"), ...codesOf("NARROW_SYMBOLS"), "XXX", "XTS"])];
const calendarKeys = codesOf("CALENDARS");
const FIELD_CODES = ["era", "year", "quarter", "month", "weekOfYear", "weekday", "day", "dayPeriod", "hour", "minute", "second", "timeZoneName"];
const BCP47_CALENDAR ={ gregorian: "gregory", "ethiopic-amete-alem": "ethioaa" };
const COMPOSED_SAMPLES = ["en-US", "en-GB", "pt-BR", "zh-Hans-CN", "zh-Hant", "fr-Latn-FR", "de-AT", "es-419", "ja-JP", "ru-Cyrl"];

function measure(locale, type, style, code, languageDisplay = "dialect") {
  try {
    return new Intl.DisplayNames([locale], { type, style, fallback: "none", languageDisplay }).of(code);
  } catch (error) {
    return undefined;
  }
}

/** Os travessões do CLDR (`Папуа \u2014 Новая Гвинея`) saem escapados, no .rs e no tsv. */
/** Literal de string Rust. */
function rs(text) {
  return escapeDashes(JSON.stringify(text).replace(/\\u([0-9a-f]{4})/gi, (_, hex) => `\\u{${hex}}`));
}

const tsv = [];
const record = (locale, type, style, languageDisplay, code, value) =>
  tsv.push([locale, type, style, languageDisplay, code, value === undefined ? "undefined" : escapeDashes(value)].join("\t"));

const lowerFirst = (text) => text.charAt(0).toLowerCase() + text.slice(1);

/** Espelha `language_display_name` de intl_display_names.rs sobre as tabelas medidas. */
function compose(tables, code, style, display) {
  const [language, ...subtags] = code.split("-");
  const script = subtags.find((tag) => tag.length === 4);
  const region = subtags.find((tag) => tag.length !== 4);
  const find = (list, key) => list.find((row) => row[0] === key);
  const dialect = (key) => {
    if (display !== "dialect") return undefined;
    const row = find(tables.dialects, key);
    return row && (style === "short" && row[2] ? row[2] : row[1]);
  };
  const key = [language, script, region].filter(Boolean).join("-");
  const base = key === language ? undefined : dialect(key);
  const languageRow = find(tables.languages, language);
  const name = base ?? (languageRow && (style === "short" && languageRow[2] ? languageRow[2] : languageRow[1]));
  if (name === undefined) return undefined;
  const details = [];
  if (base === undefined && script) {
    const scriptName = find(tables.scripts, script)?.[1];
    if (scriptName === undefined) return undefined;
    details.push(tables.lowercaseScriptDetail ? lowerFirst(scriptName) : scriptName);
  }
  if (base === undefined && region) {
    const row = find(tables.regions, region);
    if (!row) return undefined;
    details.push(style === "short" && row[2] ? row[2] : row[1]);
  }
  const [open, separator, close] = tables.details;
  return details.length ? name + open + nest(open, details.join(separator)) + close : name;
}

/** Espelha o fim de `language_display_name`: parênteses dentro dos detalhes viram colchetes se o padrão usa parênteses. */
const nest = (open, joined) => (open.includes("(") ? joined.replace(/\(/g, "[").replace(/\)/g, "]") : joined);

/** (open, separator, close) dos detalhes entre parênteses, derivados de uma amostra medida. */
function detailsPattern(locale) {
  const base = measure(locale, "language", "long", "en");
  const script = measure(locale, "script", "long", "Latn");
  const region = measure(locale, "region", "long", "US");
  const sample = measure(locale, "language", "long", "en-Latn-US", "standard");
  // Casa com a caixa exata (o `toLowerCase` muda o tamanho de `İ` em tr); a escrita pode perder a maiúscula (ru).
  const find = (rawNeedle, from) => {
    const needle = nest(" (", rawNeedle);
    const exact = sample.indexOf(needle, from);
    return exact >= 0 ? exact : sample.indexOf(lowerFirst(needle), from);
  };
  const baseEnd = find(base, 0) + base.length;
  const scriptStart = find(script, baseEnd);
  const regionStart = find(region, scriptStart + script.length);
  if (baseEnd < base.length || scriptStart < 0 || regionStart < 0) throw new Error(`${locale}: amostra ${sample}`);
  return [sample.slice(baseEnd, scriptStart), sample.slice(scriptStart + script.length, regionStart), sample.slice(regionStart + region.length)];
}

let narrowDiffers = 0;
const fullTables = {};
const out = [];
out.push("//! GERADO por scripts/gen-display-names-data.js (medido no bun, JavaScriptCore + ICU). Não edite à mão.");
out.push("//!");
out.push("//! Para regenerar, na raiz do crate: `bun scripts/gen-display-names-data.js`. O mesmo script grava");
out.push("//! `tests/golden/display_names_bun.tsv`, que `tests/display_names_bun_golden.rs` confere.");
out.push("//!");
out.push("//! Uma tabela por locale (38; en-GB, pt-PT, es-MX, fr-CA, de-AT e zh-TW guardam só o que difere do pai) com os nomes de língua, dialeto, escrita, região,");
out.push("//! moeda, calendário e campo de data e hora (`dateTimeField`), nos códigos que as tabelas de en já têm. Os textos são o resultado final do ICU");
out.push("//! (capitalização inclusa). Nome curto vazio quer dizer \"igual ao longo\". As linhas são índices u16 em `STRINGS` (0 é a string vazia),");
out.push("//! ordenadas pela chave (busca binária em `intl_table_lookup`).");
out.push("");
out.push("use crate::runtime::intl_display_names::MoreTable;");
out.push("");

for (const locale of LOCALES) {
  const languages = [];
  for (const code of languageCodes) {
    const value = measure(locale, "language", "long", code);
    record(locale, "language", "long", "dialect", code, value);
    const short = measure(locale, "language", "short", code);
    record(locale, "language", "short", "dialect", code, short);
    const narrow = measure(locale, "language", "narrow", code);
    record(locale, "language", "narrow", "dialect", code, narrow);
    if ((narrow ?? "") !== (short ?? "")) narrowDiffers++;
    if (value !== undefined) languages.push([code, value, short === value ? "" : short ?? ""]);
  }
  const dialects = [];
  for (const key of dialectKeys) {
    const long = measure(locale, "language", "long", key);
    const short = measure(locale, "language", "short", key);
    const standardLong = measure(locale, "language", "long", key, "standard");
    const standardShort = measure(locale, "language", "short", key, "standard");
    record(locale, "language", "long", "dialect", key, long);
    record(locale, "language", "short", "dialect", key, short);
    if (long !== undefined && (long !== standardLong || short !== standardShort)) {
      dialects.push([key, long, short === long ? "" : short ?? ""]);
    }
  }
  for (const code of COMPOSED_SAMPLES) {
    for (const display of ["dialect", "standard"]) {
      for (const style of ["long", "short"]) record(locale, "language", style, display, code, measure(locale, "language", style, code, display));
    }
  }
  const scripts = [];
  for (const code of scriptCodes) {
    const value = measure(locale, "script", "long", code);
    record(locale, "script", "long", "dialect", code, value);
    if (value !== undefined) scripts.push([code, value]);
  }
  const regions = [];
  for (const code of regionCodes) {
    const long = measure(locale, "region", "long", code);
    const short = measure(locale, "region", "short", code);
    record(locale, "region", "long", "dialect", code, long);
    record(locale, "region", "short", "dialect", code, short);
    const narrow = measure(locale, "region", "narrow", code);
    record(locale, "region", "narrow", "dialect", code, narrow);
    if ((narrow ?? "") !== (short ?? "")) narrowDiffers++;
    if (long !== undefined) regions.push([code, long, short === long ? "" : short ?? ""]);
  }
  const currencies = [];
  for (const code of currencyCodes) {
    const values = ["long", "short", "narrow"].map((style) => measure(locale, "currency", style, code));
    ["long", "short", "narrow"].forEach((style, index) => record(locale, "currency", style, "dialect", code, values[index]));
    if (values.some((value) => value !== undefined)) currencies.push([code, ...values.map((value) => value ?? "")]);
  }
  const calendars = [];
  for (const key of calendarKeys) {
    const value = measure(locale, "calendar", "long", BCP47_CALENDAR[key] ?? key);
    record(locale, "calendar", "long", "dialect", BCP47_CALENDAR[key] ?? key, value);
    if (value !== undefined) calendars.push([key, value]);
  }

  const fields = [];
  for (const code of FIELD_CODES) {
    const values = ["long", "short", "narrow"].map((style) => measure(locale, "dateTimeField", style, code));
    ["long", "short", "narrow"].forEach((style, index) => record(locale, "dateTimeField", style, "dialect", code, values[index]));
    fields.push([code, ...values.map((value) => value ?? "")]);
  }

  const rows = (list) => list.map((row) => `        (${row.map(rs).join(", ")}),`).join("\n");
  const [open, separator, close] = detailsPattern(locale);
  // O nome de escrita dentro dos parênteses perde a maiúscula inicial em ru (`Латиница` vs `латиница`).
  const scriptSample = measure(locale, "language", "long", "en-Latn", "standard");
  const lowercaseScriptDetail = scripts.find((row) => row[0] === "Latn")[1] !== scripts.find((row) => row[0] === "Latn")[1].toLowerCase() &&
    scriptSample.includes(lowerFirst(scripts.find((row) => row[0] === "Latn")[1]));
  const tables = { languages, dialects, scripts, regions, details: [open, separator, close], lowercaseScriptDetail };
  for (const code of [...COMPOSED_SAMPLES, ...languageCodes.map((c) => `${c}-Latn-US`), ...languageCodes.map((c) => `${c}-DE`)]) {
    for (const display of ["dialect", "standard"]) {
      for (const style of ["long", "short"]) {
        const expected = measure(locale, "language", style, code, display);
        const actual = compose(tables, code, style, display);
        if (expected !== actual) throw new Error(`${locale} ${code} ${style} ${display}: bun ${expected} contra tabelas ${actual}`);
      }
    }
  }
  fullTables[locale] = { open, separator, close, lowercaseScriptDetail, languages, dialects, scripts, regions, currencies, calendars, fields };
}

/** As linhas de `list` que diferem das do pai (por chave); uma chave que só o pai tem vira lápide (texto longo vazio). */
function diffRows(list, parentList, tombstone) {
  if (!parentList) return list;
  const parentByKey = new Map(parentList.map((row) => [row[0], row]));
  const own = new Set(list.map((row) => row[0]));
  const changed = list.filter((row) => {
    const other = parentByKey.get(row[0]);
    return !other || other.length !== row.length || other.some((cell, index) => cell !== row[index]);
  });
  if (tombstone) for (const row of parentList) if (!own.has(row[0])) changed.push(row.map((_, index) => (index === 0 ? row[0] : "")));
  return changed;
}

// Formato compacto: cada texto único (inclusive as chaves) vive uma vez em `STRINGS`, e as linhas das tabelas são
// índices u16 nele (0 é a string vazia: nome curto igual ao longo, moeda sem símbolo, lápide). As linhas saem
// ordenadas pela chave, para a busca binária do consumidor. Os mais frequentes ganham os índices menores.
const LIST_NAMES = ["languages", "dialects", "scripts", "regions", "currencies", "calendars", "fields"];
const TOMBSTONE_LISTS = ["languages", "dialects", "scripts", "regions", "calendars"];
const byKey = (a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
const emitted = {};
const counts = new Map();
for (const locale of LOCALES) {
  const parentName = PARENTS[locale];
  const parent = parentName && fullTables[parentName];
  emitted[locale] = {};
  for (const name of LIST_NAMES) {
    const list = [...diffRows(fullTables[locale][name], parent && parent[name], TOMBSTONE_LISTS.includes(name))].sort(byKey);
    list.forEach((row, index) => {
      if (!/^[\x20-\x7e]+$/.test(row[0])) throw new Error(`${locale} ${name}: chave não ASCII ${row[0]}`);
      if (index > 0 && !(list[index - 1][0] < row[0])) throw new Error(`${locale} ${name}: chave repetida ${row[0]}`);
      row.forEach((text) => counts.set(text, (counts.get(text) ?? 0) + 1));
    });
    emitted[locale][name] = list;
  }
}
const pool = ["", ...[...counts.keys()].filter((text) => text !== "").sort((a, b) => counts.get(b) - counts.get(a))];
if (pool.length > 65536) throw new Error(`${pool.length} textos não cabem em u16`);
const poolIndex = new Map(pool.map((text, index) => [text, index]));
const chunked = (items, size, indent) => {
  const lines = [];
  for (let i = 0; i < items.length; i += size) lines.push(indent + items.slice(i, i + size).join(","));
  return lines.join(",\n") + (items.length ? "," : "");
};
const rows = (list) => chunked(list.map((row) => `(${row.map((text) => poolIndex.get(text)).join(",")})`), 12, "        ");
out.push("/// Os textos únicos de todas as tabelas; as linhas guardam índices neste vetor (0 é a string vazia).");
out.push(`pub static STRINGS: &[&str] = &[\n${chunked(pool.map(rs), 10, "    ")}\n];`);
out.push("");
for (const locale of LOCALES) {
  const table = fullTables[locale];
  const parentName = PARENTS[locale];
  const parent = parentName && fullTables[parentName];
  out.push(`pub static ${ident(locale)}: MoreTable = MoreTable {`);
  out.push(`    locale: ${rs(locale)},`);
  out.push(`    parent: ${parent ? `Some(&${ident(parentName)})` : "None"},`);
  out.push(`    details: (${rs(table.open)}, ${rs(table.separator)}, ${rs(table.close)}),`);
  out.push(`    lowercase_script_detail: ${table.lowercaseScriptDetail},`);
  for (const name of LIST_NAMES) out.push(`    ${name}: &[\n${rows(emitted[locale][name])}\n    ],`);
  out.push("};");
  out.push("");
}

out.push("/// A tabela de `locale` (`en-GB`, `pt-PT`, `zh-TW`...) ou, sem tabela própria, a da língua (`es-ES` usa `es`, `zh-Hant`");
out.push("/// e `zh-HK` usam `zh-TW`); `None` para as línguas sem tabela.");
out.push("pub fn table_for_locale(locale: &str) -> Option<&'static MoreTable> {");
out.push("    let mut parts = locale.split('-');");
out.push("    let language = parts.next().unwrap_or(locale);");
out.push("    let rest: Vec<&str> = parts.collect();");
out.push('    if language == "zh" && rest.iter().any(|tag| matches!(*tag, "Hant" | "TW" | "HK" | "MO")) {');
out.push("        return Some(&ZH_TW);");
out.push("    }");
for (const locale of SCRIPT_LOCALES) {
  const [language, script] = locale.split("-");
  out.push(`    if language == ${rs(language)} && rest.contains(&${rs(script)}) {`);
  out.push(`        return Some(&${ident(locale)});`);
  out.push("    }");
}
out.push("    match (language, rest.iter().find(|tag| tag.len() == 2 || tag.len() == 3)) {");
// As entradas com região vêm antes das da língua sozinha: a ordem inversa deixaria o braço regional inalcançável.
for (const locale of [...LOCALES].filter((name) => !SCRIPT_LOCALES.includes(name)).sort((a, b) => Number(b.includes("-")) - Number(a.includes("-")))) {
  const [language, region] = locale.split("-");
  out.push(region ? `        (${rs(language)}, Some(&${rs(region)})) => Some(&${ident(locale)}),` : `        (${rs(language)}, _) => Some(&${ident(locale)}),`);
}
out.push("        _ => None,");
out.push("    }");
out.push("}");
out.push("");

writeRustSource(path.join(root, "src/runtime/intl_display_names_data_more.rs"), out.join("\n"));
fs.writeFileSync(path.join(root, "tests/golden/display_names_bun.tsv"), require("./golden-prelude.js").assertPublicResult(tsv.join("\n") + "\n"));
console.log(`${tsv.length} linhas de golden; narrow difere de short em ${narrowDiffers} casos`);
