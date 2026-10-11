// Gera tests/golden/reltime_more_bun.tsv: Intl.RelativeTimeFormat, Intl.ListFormat e Intl.PluralRules em 20 locales
// que reltime_bun.tsv não cobre, medido no bun 1.4.2. RelativeTimeFormat: numeric auto e always, style long, short e
// narrow, as oito unidades (incluindo quarter e plurais), valores negativos, zero, -0, fracionários e grandes, e
// formatToParts. Programas já presentes nos outros goldens são descartados.
// Colunas: a fonte do programa (expressão que devolve string) e o resultado medido (`throw` quando lançou).
// Uso: bun scripts/gen-reltime-more-golden.js > tests/golden/reltime_more_bun.tsv
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");

const locales = ["ca", "sk", "sl", "lt", "lv", "et", "sq", "af", "ga", "gl", "eu", "is", "br", "fy", "si", "ne", "bn", "ta", "ml", "ur"];

for (const locale of locales) {
  for (const ctor of ["RelativeTimeFormat", "ListFormat", "PluralRules"]) {
    if (Intl[ctor].supportedLocalesOf([locale]).length !== 1) throw new Error(`${ctor} não aceita ${locale}`);
  }
}

// [style, numeric, unit, value]
const relative = [
  ["long", "auto", "day", -1], ["long", "auto", "day", 0], ["long", "auto", "day", 1], ["long", "auto", "day", 2],
  ["long", "auto", "year", 0], ["long", "auto", "quarter", -1], ["long", "always", "quarter", 2.5],
  ["short", "auto", "month", 1], ["short", "always", "week", -3], ["narrow", "auto", "hour", 0],
  ["narrow", "always", "minute", -1], ["narrow", "always", "second", 5], ["short", "always", "year", -0],
  ["long", "always", "day", 0], ["long", "auto", "week", -0], ["narrow", "auto", "quarter", 1],
  ["short", "always", "quarter", -1], ["long", "always", "hour", 1.5], ["long", "always", "month", -1000000],
  ["short", "always", "second", 0.5], ["narrow", "always", "day", 21], ["long", "always", "year", 1234567.891],
  ["long", "auto", "second", 0], ["short", "auto", "minute", 0], ["long", "always", "quarters", 3],
  ["short", "auto", "quarters", -2],
];

const programs = [];
const q = JSON.stringify;
for (const locale of locales) {
  for (const [style, numeric, unit, value] of relative) {
    const literal = Object.is(value, -0) ? "-0" : String(value);
    programs.push(`new Intl.RelativeTimeFormat(${q(locale)}, {"style":${q(style)},"numeric":${q(numeric)}}).format(${literal}, ${q(unit)})`);
  }
  programs.push(`JSON.stringify(new Intl.RelativeTimeFormat(${q(locale)}, {"numeric":"always"}).formatToParts(-3, "day"))`);
  programs.push(`JSON.stringify(new Intl.RelativeTimeFormat(${q(locale)}, {"numeric":"auto","style":"short"}).formatToParts(1, "day"))`);
  programs.push(`JSON.stringify(new Intl.RelativeTimeFormat(${q(locale)}, {"style":"narrow"}).formatToParts(1234.5, "quarter"))`);
  for (const [type, style, items] of [
    ["conjunction", "long", ["a", "b", "c"]], ["disjunction", "short", ["a", "b"]], ["unit", "narrow", ["a", "b", "c", "d"]],
  ]) {
    programs.push(`new Intl.ListFormat(${q(locale)}, {"type":${q(type)},"style":${q(style)}}).format(${q(items)})`);
  }
  const values = "[0, 1, 2, 3, 5, 11, 21, 100, 1.5]";
  programs.push(`${values}.map(n => new Intl.PluralRules(${q(locale)}).select(n)).join(",")`);
  programs.push(`${values}.map(n => new Intl.PluralRules(${q(locale)}, {"type":"ordinal"}).select(n)).join(",")`);
  programs.push(`new Intl.PluralRules(${q(locale)}).resolvedOptions().pluralCategories.join(",")`);
}

// Descarta o que já está em outros goldens.
const known = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const program of knownPrograms("reltime_more_bun.tsv", (name) => name !== "reltime_more_bun.tsv")) known.add(JSON.stringify(program));

const seen = new Set();
const lines = [];
for (const source of programs) {
  if (known.has(source) || seen.has(source)) continue;
  seen.add(source);
  let result;
  try {
    result = String((0, eval)(source));
  } catch (error) {
    result = "throw";
  }
  lines.push(`${source}\t${result.replace(/[\t\n\r]/g, c => (c === "\t" ? "\\t" : c === "\n" ? "\\n" : "\\r"))}`);
}
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
