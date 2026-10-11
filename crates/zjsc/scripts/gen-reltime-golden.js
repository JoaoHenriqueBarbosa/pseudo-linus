// Gera, a partir do bun (JavaScriptCore real), os dados do Intl.RelativeTimeFormat de 37 línguas (en pt es fr de it ja ru ar hi ko zh fa he th tr pl nl sv da nb fi cs el id vi uk ro hu bg hr sr),
// it, ja, ru, ar e hi, e o golden dessas línguas mais tags com numberingSystem (`-u-nu-`):
//   tests/golden/reltime_bun.tsv             golden: lang, style, numeric, unit, valor, texto esperado
//   src/runtime/intl_relative_time_data.rs   tabela de padrões por categoria de plural e textos de auto
//
// Como regenerar (da raiz de wip/zjsc):  bun scripts/gen-reltime-golden.js
// Os padrões saem de formatToParts: as partes do número viram `{0}`, o resto é literal. A categoria
// de plural de cada amostra vem do Intl.PluralRules do próprio bun.
const fs = require("fs");
const path = require("path");

const LANGS = ["en", "pt", "es", "fr", "de", "it", "ja", "ru", "ar", "hi", "ko", "zh", "fa", "he", "th", "tr", "pl", "nl", "sv", "da", "nb", "fi", "cs", "el", "id", "vi", "uk", "ro", "hu", "bg", "hr", "sr", "en-GB", "pt-PT", "es-MX", "fr-CA", "zh-TW", "am", "my", "km", "lo", "mn", "ps", "sd", "so", "fil", "ha", "yo", "zu", "xh", "cy", "gd", "lb", "mt", "fo", "ky", "tg", "tk", "tt", "ku", "or", "as"];
// Tags só do golden: as com `-u-nu-` medem o numberingSystem.
const GOLDEN_ONLY = ["ar-u-nu-arab", "hi-u-nu-deva", "en-u-nu-arab", "es-u-nu-deva"];
const STYLES = ["long", "short", "narrow"];
const UNITS = ["second", "minute", "hour", "day", "week", "month", "quarter", "year"];
const CATEGORIES = ["zero", "one", "two", "few", "many", "other"];
// Valores do golden: o intervalo do auto e amostras de plural (inteiros abaixo de 1000, sem grupo).
const VALUES = [-2, -1, -0, 0, 1, 2, 3, 5, 11, 21, 22, 100, -3, -5, -11, -21, -100];
// Valores grandes e com fração: milhar, `many` de inteiros grandes e o separador decimal do locale.
const BIG_VALUES = [1234, 1000000, 1234567.891, 0.5, 1.5, -1234];

const root = path.resolve(__dirname, "..");

function candidates() {
  const list = [0];
  for (let n = 1; n <= 200; n++) list.push(n);
  list.push(1000000);
  return list;
}

function pattern(formatter, value, unit) {
  const parts = formatter.formatToParts(value, unit);
  let out = "";
  let inNumber = false;
  for (const part of parts) {
    if (part.unit !== undefined) {
      if (!inNumber) out += "{0}";
      inNumber = true;
    } else {
      inNumber = false;
      out += part.value;
    }
  }
  return out;
}

function rustString(text) {
  let out = '"';
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (ch === '"') out += '\\"';
    else if (ch === "\\") out += "\\\\";
    else if (code < 0x20 || code > 0x7e) out += "\\u{" + code.toString(16) + "}";
    else out += ch;
  }
  return out + '"';
}

const tsv = [];
const entries = [];
const autos = [];

for (const lang of LANGS) {
  const rules = new Intl.PluralRules(lang);
  for (const [styleIndex, style] of STYLES.entries()) {
    const always = new Intl.RelativeTimeFormat(lang, { style, numeric: "always" });
    const auto = new Intl.RelativeTimeFormat(lang, { style, numeric: "auto" });
    for (const [unitIndex, unit] of UNITS.entries()) {
      const past = CATEGORIES.map(() => "");
      const future = CATEGORIES.map(() => "");
      for (const n of candidates()) {
        const category = rules.select(n);
        const index = CATEGORIES.indexOf(category);
        if (future[index] === "") future[index] = pattern(always, n, unit);
        if (past[index] === "") past[index] = pattern(always, n === 0 ? -0 : -n, unit);
      }
      entries.push({ lang, styleIndex, unitIndex, past, future });
      for (let v = -2; v <= 2; v++) {
        const text = auto.format(v, unit);
        if (text !== always.format(v, unit)) autos.push({ lang, styleIndex, unitIndex, v, text });
      }
      for (const numericMode of ["always", "auto"]) {
        const formatter = numericMode === "always" ? always : auto;
        for (const value of [...VALUES, ...BIG_VALUES]) {
          const shown = Object.is(value, -0) ? "-0" : String(value);
          tsv.push([lang, style, numericMode, unit, shown, formatter.format(value, unit)].join("\t"));
        }
      }
    }
  }
}

// Só golden (sem tabela de padrões): as tags com sistema numérico explícito.
for (const lang of GOLDEN_ONLY) {
  for (const style of STYLES) {
    for (const numericMode of ["always", "auto"]) {
      const formatter = new Intl.RelativeTimeFormat(lang, { style, numeric: numericMode });
      for (const unit of UNITS) {
        for (const value of [...VALUES, ...BIG_VALUES]) {
          const shown = Object.is(value, -0) ? "-0" : String(value);
          tsv.push([lang, style, numericMode, unit, shown, formatter.format(value, unit)].join("\t"));
        }
      }
    }
  }
}

fs.writeFileSync(path.join(root, "tests/golden/reltime_bun.tsv"), require("./golden-prelude.js").assertPublicResult(tsv.join("\n") + "\n"));

const lines = [];
lines.push("//! Padrões do `Intl.RelativeTimeFormat` de 37 línguas (as 38 locales do escopo, sem de-AT que é igual a de), medidos no bun.");
lines.push("//!");
lines.push("//! ARQUIVO GERADO: não edite à mão. Para regenerar, na raiz de `wip/zjsc`:");
lines.push("//!");
lines.push("//! ```text");
lines.push("//! bun scripts/gen-reltime-golden.js");
lines.push("//! ```");
lines.push("//!");
lines.push("//! O mesmo script escreve `tests/golden/reltime_bun.tsv`, que `tests/reltime_bun_golden.rs` confere.");
lines.push("");
lines.push("/// Índices de estilo: `long` 0, `short` 1, `narrow` 2. Índices de unidade: `second` 0, `minute` 1, `hour` 2,");
lines.push("/// `day` 3, `week` 4, `month` 5, `quarter` 6, `year` 7. Índices de categoria de plural: `zero` 0, `one` 1,");
lines.push("/// `two` 2, `few` 3, `many` 4, `other` 5; o texto vazio quer dizer \"use `other`\".");
lines.push("pub struct Entry {");
lines.push("    pub lang: &'static str,");
lines.push("    pub style: u8,");
lines.push("    pub unit: u8,");
lines.push("    pub past: [&'static str; 6],");
lines.push("    pub future: [&'static str; 6],");
lines.push("}");
lines.push("");
lines.push("/// Os textos de `numeric: \"auto\"`: língua, estilo, unidade, valor (-2 a 2) e texto.");
lines.push("pub struct AutoText {");
lines.push("    pub lang: &'static str,");
lines.push("    pub style: u8,");
lines.push("    pub unit: u8,");
lines.push("    pub value: i8,");
lines.push("    pub text: &'static str,");
lines.push("}");
lines.push("");
lines.push("/// As línguas desta tabela.");
lines.push("pub const LANGUAGES: &[&str] = &[" + LANGS.map(rustString).join(", ") + "];");
lines.push("");
lines.push("pub static PATTERNS: &[Entry] = &[");
for (const e of entries) {
  const arr = (list) => "[" + list.map(rustString).join(", ") + "]";
  lines.push(
    `    Entry { lang: ${rustString(e.lang)}, style: ${e.styleIndex}, unit: ${e.unitIndex}, past: ${arr(e.past)}, future: ${arr(e.future)} },`,
  );
}
lines.push("];");
lines.push("");
lines.push("pub static AUTO: &[AutoText] = &[");
for (const a of autos) {
  lines.push(
    `    AutoText { lang: ${rustString(a.lang)}, style: ${a.styleIndex}, unit: ${a.unitIndex}, value: ${a.v}, text: ${rustString(a.text)} },`,
  );
}
lines.push("];");
lines.push("");

fs.writeFileSync(path.join(root, "src/runtime/intl_relative_time_data.rs"), lines.join("\n"));
console.log(`golden: ${tsv.length} linhas, padrões: ${entries.length}, auto: ${autos.length}`);
