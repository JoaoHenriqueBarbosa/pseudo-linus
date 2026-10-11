// Gera src/runtime/intl_iso_field_labels_data.rs: o rótulo dos campos `month` e `day` que o ICU acrescenta ao
// padrão do calendário `iso8601` quando a era pede um campo que o padrão não traz (`" (month: 1)"`, `" (dia: 5)"`,
// `" (月: 1月)"`). É o `dateTimeField` largo do locale (`Intl.DisplayNames`), medido nos mesmos 65 locales de
// `scripts/gen-datetime-data.js`; o formato do texto e a escolha de quando o rótulo aparece estão em
// `iso_era_field_parts` (src/runtime/intl_date_time_format.rs).
// Uso: bun scripts/gen-iso-field-labels.js > src/runtime/intl_iso_field_labels_data.rs
const fs = require("fs");
const path = require("path");

const source = fs.readFileSync(path.join(__dirname, "gen-datetime-data.js"), "utf8");
const locales = source.match(/const LOCALES = \[([\s\S]*?)\];/)[1].match(/"[^"]+"/g).map((text) => JSON.parse(text));

const escape = (text) => JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => `\\u{${c.codePointAt(0).toString(16)}}`);

// Os locales cujo mês abreviado do `iso8601` não é o nome isolado do gregoriano de `intl_date_time_data` (em `el` o
// ICU usa os nomes com acento): o resto vem dos dados do locale.
const shortMonths = (locale, calendar) => {
  const format = new Intl.DateTimeFormat(locale, { calendar, numberingSystem: "latn", month: "short", timeZone: "UTC", ...(calendar === "iso8601" ? { era: "short" } : {}) });
  return Array.from({ length: 12 }, (_, month) => {
    const date = new Date(Date.UTC(2024, month, 5));
    return calendar === "iso8601" ? format.formatToParts(date).find((part) => part.type === "month").value : format.format(date);
  });
};
const overrides = locales
  .map((locale) => [locale, shortMonths(locale, "iso8601"), shortMonths(locale, "gregory")])
  .filter(([, iso, gregory]) => iso.join() !== gregory.join())
  .map(([locale, iso]) => `    (${escape(locale)}, [${iso.map(escape).join(", ")}]),`);

const rows = locales.map((locale) => {
  const names = new Intl.DisplayNames(locale, { type: "dateTimeField" });
  return `    (${escape(locale)}, ${escape(names.of("month"))}, ${escape(names.of("day"))}),`;
});

process.stdout.write(`//! Rótulos de \`month\` e \`day\` do calendário \`iso8601\` com era, por locale, e os meses abreviados que diferem do gregoriano (medidos no bun).
//!
//! GERADO por \`scripts/gen-iso-field-labels.js\`: não edite à mão. Para regenerar, da raiz da crate:
//! \`bun scripts/gen-iso-field-labels.js > src/runtime/intl_iso_field_labels_data.rs\`.

/// (locale, rótulo do mês, rótulo do dia), na ordem de \`LOCALES\` do gerador de \`intl_date_time_data\`.
static LABELS: [(&str, &str, &str); ${rows.length}] = [
${rows.join("\n")}
];

fn find(key: &str) -> Option<(&'static str, &'static str)> {
    LABELS.iter().find(|(name, _, _)| *name == key).map(|(_, month, day)| (*month, *day))
}

/// Os meses abreviados do \`iso8601\` dos locales em que diferem do gregoriano isolado.
static SHORT_MONTHS: [(&str, [&str; 12]); ${overrides.length}] = [
${overrides.join("\n")}
];

/// O mês abreviado (\`month0\` de 0 a 11) do \`iso8601\` quando o locale difere do gregoriano isolado.
pub fn short_month_override(tag: &str, month0: usize) -> Option<&'static str> {
    let language = tag.split('-').next().unwrap_or("");
    SHORT_MONTHS.iter().find(|(name, _)| *name == language).map(|(_, months)| months[month0])
}

/// O rótulo do mês e o do dia do tag BCP 47: primeiro \`língua-REGIÃO\`, depois a língua sozinha, por fim o inglês.
pub fn labels(tag: &str) -> (&'static str, &'static str) {
    let mut subtags = tag.split('-');
    let language = subtags.next().unwrap_or("");
    let region = subtags.take_while(|subtag| subtag.len() > 1).find(|subtag| subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()));
    region
        .and_then(|region| find(&format!("{language}-{}", region.to_ascii_uppercase())))
        .or_else(|| find(language))
        .unwrap_or(("month", "day"))
}
`);
