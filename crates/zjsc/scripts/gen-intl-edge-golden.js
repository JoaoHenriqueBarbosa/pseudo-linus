// Gera tests/golden/intl_edge_bun.tsv: casos de borda de Intl.NumberFormat, Intl.PluralRules e Intl.ListFormat em 12
// locales fora dos já cobertos (ga gl eu is br fy si ne bo ig wo ml), medidos no bun 1.4.2.
// NumberFormat: notation compact/scientific/engineering, unit e unitDisplay, currencyDisplay, signDisplay,
// roundingMode, roundingIncrement, trailingZeroDisplay, formatRange e formatToParts. PluralRules: select, selectRange
// e ordinal. ListFormat: conjunction, disjunction e unit em long, short e narrow.
// Colunas: a expressão (devolve string) e o resultado medido (`throw` quando lançou), igual a gen-intl-more-locales-golden.js.
// Uso: bun scripts/gen-intl-edge-golden.js > tests/golden/intl_edge_bun.tsv
const LOCALES = ["ga", "gl", "eu", "is", "br", "fy", "si", "ne", "bo", "ig", "wo", "ml"];

const exprs = [];
const add = expr => exprs.push(expr);
const q = JSON.stringify;

for (const locale of LOCALES) {
  const nf = options => `new Intl.NumberFormat(${q(locale)}, ${JSON.stringify(options)})`;
  const fmt = (options, value) => add(`${nf(options)}.format(${value})`);
  const parts = (options, value) => add(`JSON.stringify(${nf(options)}.formatToParts(${value}))`);

  fmt({ notation: "compact", compactDisplay: "short" }, 1234567);
  fmt({ notation: "compact", compactDisplay: "long" }, 1234567);
  fmt({ notation: "compact", compactDisplay: "short", maximumFractionDigits: 2 }, 98765.4321);
  fmt({ notation: "scientific" }, 123456.789);
  fmt({ notation: "scientific", maximumFractionDigits: 0 }, 0.000345);
  fmt({ notation: "engineering" }, 123456.789);
  fmt({ notation: "engineering", minimumFractionDigits: 2 }, 0.0000345);
  fmt({ style: "unit", unit: "kilometer-per-hour", unitDisplay: "short" }, 1234.5);
  fmt({ style: "unit", unit: "kilometer-per-hour", unitDisplay: "long" }, 1);
  fmt({ style: "unit", unit: "celsius", unitDisplay: "narrow" }, -3.5);
  fmt({ style: "unit", unit: "byte", unitDisplay: "long", notation: "compact" }, 2500000);
  fmt({ style: "currency", currency: "EUR", currencyDisplay: "code" }, 1234.5);
  fmt({ style: "currency", currency: "EUR", currencyDisplay: "name" }, 1);
  fmt({ style: "currency", currency: "USD", currencyDisplay: "narrowSymbol" }, -1234.567);
  fmt({ style: "currency", currency: "JPY", currencySign: "accounting" }, -5000);
  fmt({ signDisplay: "exceptZero" }, 0);
  fmt({ signDisplay: "negative" }, -0);
  fmt({ signDisplay: "always", style: "percent" }, 0.256);
  fmt({ roundingMode: "halfEven", maximumFractionDigits: 0 }, 2.5);
  fmt({ roundingMode: "floor", maximumFractionDigits: 1 }, -1.25);
  fmt({ roundingIncrement: 5, minimumFractionDigits: 2, maximumFractionDigits: 2 }, 1.23);
  fmt({ roundingIncrement: 250, maximumFractionDigits: 0 }, 1234);
  fmt({ trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2 }, 12);
  fmt({ roundingPriority: "morePrecision", maximumSignificantDigits: 3, maximumFractionDigits: 1 }, 1.2345);
  fmt({ useGrouping: "min2" }, 1234);
  fmt({}, "12345678901234567890.123456789");
  fmt({ notation: "compact" }, "123456789012345678901234567890");
  parts({ notation: "compact", compactDisplay: "long" }, 1234567);
  parts({ style: "currency", currency: "EUR", currencyDisplay: "name" }, -1234.5);
  parts({ notation: "scientific", signDisplay: "always" }, 0.00123);
  add(`${nf({ style: "unit", unit: "meter", unitDisplay: "long" })}.formatRange(1, 5)`);
  add(`${nf({ style: "currency", currency: "EUR", maximumFractionDigits: 0 })}.formatRange(3, 3.2)`);
  add(`${nf({ notation: "compact" })}.formatRange(1500, 2500000)`);
  add(`JSON.stringify(${nf({ maximumFractionDigits: 0 })}.formatRangeToParts(10, 20))`);
}

for (const locale of LOCALES) {
  const values = [0, 1, 2, 3, 4, 5, 6, 7, 11, 21, 100, 101, 1.5, 1000000];
  for (const type of ["cardinal", "ordinal"]) {
    add(`[${values.join(",")}].map(n => new Intl.PluralRules(${q(locale)}, { type: ${q(type)} }).select(n)).join()`);
  }
  add(`new Intl.PluralRules(${q(locale)}, { minimumFractionDigits: 1 }).select(1)`);
  add(`new Intl.PluralRules(${q(locale)}, { notation: "compact" }).select(1000000)`);
  add(`JSON.stringify(new Intl.PluralRules(${q(locale)}, { type: "ordinal" }).resolvedOptions().pluralCategories)`);
  add(`JSON.stringify(new Intl.PluralRules(${q(locale)}).resolvedOptions().pluralCategories)`);
  add(`[[0,1],[1,2],[2,5],[1,1],[3,11],[1,100]].map(([a, b]) => new Intl.PluralRules(${q(locale)}).selectRange(a, b)).join()`);
  add(`[[1,2],[2,5],[21,22],[100,101]].map(([a, b]) => new Intl.PluralRules(${q(locale)}, { type: "ordinal" }).selectRange(a, b)).join()`);
}

for (const locale of LOCALES) {
  for (const type of ["conjunction", "disjunction", "unit"]) {
    for (const style of ["long", "short", "narrow"]) {
      const lf = `new Intl.ListFormat(${q(locale)}, { type: ${q(type)}, style: ${q(style)} })`;
      add(`[["A","B"],["A","B","C"],["A","B","C","D"]].map(l => ${lf}.format(l)).join(" | ")`);
    }
  }
  add(`JSON.stringify(new Intl.ListFormat(${q(locale)}, { type: "unit", style: "narrow" }).formatToParts(["x", "y", "z"]))`);
}

const rows = [];
const seen = new Set();
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  let result;
  try {
    result = (0, eval)(expr);
    if (typeof result !== "string") result = "throw";
  } catch (error) {
    result = "throw";
  }
  rows.push(`${expr.replace(/[\t\n]/g, " ")}\t${result.replace(/[\t\n]/g, " ")}`);
}
console.log(rows.join("\n"));
