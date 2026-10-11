// Gera tests/golden/plural_bun.tsv: programas de uma linha sobre Intl.PluralRules, avaliados no bun.
// Colunas: fonte do programa (uma expressão que devolve string) e o resultado medido (ou "throw").
// Cobre select(), selectRange() e resolvedOptions().pluralCategories em 19 locales, nos tipos cardinal
// e ordinal, com dígitos mínimos de fração 0/1/2 e notação compacta.
// Uso: bun scripts/gen-plural-golden.js > tests/golden/plural_bun.tsv
const locales = ["en", "pt", "es", "fr", "de", "ru", "pl", "ar", "cs", "he", "ja", "cy", "ga", "lt", "lv", "sl", "mt", "tzm", "ro"];
const types = ["cardinal", "ordinal"];
const numbers = [0, 1, 1.5, 2, 3, 5, 10, 11, 21, 100, 101, 1000000, 0.1, 2.5];
const compactNumbers = [1e6, 1.5e6];
const ranges = [[0, 1], [1, 2], [1, 5], [21, 22], [0, 100]];

const lines = [];
const add = (source) => {
  let result;
  try {
    result = String(eval(source));
  } catch (error) {
    result = "throw";
  }
  lines.push(source + "\t" + result);
};

const make = (locale, type, extra) => `new Intl.PluralRules("${locale}", { type: "${type}"${extra} })`;

for (const locale of locales) {
  for (const type of types) {
    add(`${make(locale, type, "")}.resolvedOptions().pluralCategories.join(",")`);
    for (const digits of [0, 1, 2]) {
      const rules = make(locale, type, `, minimumFractionDigits: ${digits}`);
      for (const value of numbers) add(`${rules}.select(${value})`);
      for (const [start, end] of ranges) add(`${rules}.selectRange(${start}, ${end})`);
    }
    for (const display of ["short", "long"]) {
      const rules = make(locale, type, `, notation: "compact", compactDisplay: "${display}"`);
      for (const value of compactNumbers) add(`${rules}.select(${value})`);
      for (const [start, end] of ranges) add(`${rules}.selectRange(${start}, ${end})`);
      add(`${rules}.selectRange(1e6, 1.5e6)`);
    }
  }
}

process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
