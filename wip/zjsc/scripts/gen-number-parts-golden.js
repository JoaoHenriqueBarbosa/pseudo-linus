// Gera tests/golden/number_parts_bun.tsv: programas de uma linha sobre Intl.NumberFormat (format,
// formatToParts, formatRange, formatRangeToParts), avaliados no bun. Colunas: fonte do programa, KIND
// (o typeof do valor de conclusão, ou "throw") e REPR, serializados pelo mesmo
// tests/golden/e2e_values_harness.js que tests/number_parts_bun_golden.rs embute.
// Uso: bun scripts/gen-number-parts-golden.js > tests/golden/number_parts_bun.tsv
const fs = require("fs");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const programs = [];
const nf = (locale, options) => `new Intl.NumberFormat(${JSON.stringify(locale)}, ${JSON.stringify(options)})`;
const parts = (locale, options, value) => programs.push(`${nf(locale, options)}.formatToParts(${value})`);
const fmt = (locale, options, value) => programs.push(`${nf(locale, options)}.format(${value})`);
const range = (locale, options, a, b) => programs.push(`${nf(locale, options)}.formatRange(${a}, ${b})`);
const rangeParts = (locale, options, a, b) => programs.push(`${nf(locale, options)}.formatRangeToParts(${a}, ${b})`);

// Tipos de parte básicos.
parts("en-US", {}, "-1234567.891");
parts("en-US", {}, "1234.5");
parts("en-US", {}, "NaN");
parts("en-US", {}, "Infinity");
parts("en-US", {}, "-Infinity");
parts("en-US", { style: "percent" }, "0.256");
parts("en-US", { style: "percent", signDisplay: "always" }, "0.5");
parts("en-US", { style: "currency", currency: "USD" }, "-1234.5");
parts("de-DE", { style: "currency", currency: "EUR" }, "1234.5");
parts("en-US", { style: "currency", currency: "EUR", currencyDisplay: "name" }, "2");
parts("en-US", { style: "currency", currency: "USD", currencyDisplay: "code" }, "2");
parts("en-US", { style: "currency", currency: "USD", currencySign: "accounting" }, "-5");
parts("ja-JP", { style: "currency", currency: "JPY" }, "1234");
parts("pt-BR", { style: "currency", currency: "BRL" }, "1234.5");
parts("en-US", { style: "unit", unit: "kilometer-per-hour" }, "50");
parts("en-US", { style: "unit", unit: "liter", unitDisplay: "long" }, "16");
parts("en-US", { style: "unit", unit: "byte", unitDisplay: "narrow" }, "3");
parts("de-DE", {}, "1234567.891");
parts("fr-FR", {}, "1234567.891");
parts("en-IN", {}, "1234567.891");
parts("en-US", { useGrouping: false }, "1234567.891");
parts("en-US", { minimumFractionDigits: 2 }, "1");
parts("en-US", { useGrouping: "min2" }, "1234");
parts("en-US", { useGrouping: "min2" }, "12345");

// Notação.
parts("en-US", { notation: "scientific" }, "123456");
parts("en-US", { notation: "scientific" }, "0.00012");
parts("en-US", { notation: "scientific" }, "-0.00012");
parts("en-US", { notation: "engineering" }, "123456");
parts("en-US", { notation: "engineering" }, "0.00012");
parts("en-US", { notation: "engineering", signDisplay: "always" }, "5e-10");
parts("en-US", { notation: "compact" }, "1234");
parts("en-US", { notation: "compact" }, "1234567");
parts("en-US", { notation: "compact", compactDisplay: "long" }, "1234567");
parts("en-US", { notation: "compact", compactDisplay: "long" }, "1234567890");
parts("en-US", { notation: "compact" }, "999999");
parts("en-US", { notation: "compact" }, "-15000");
parts("de-DE", { notation: "compact", compactDisplay: "long" }, "2500000");
parts("ja-JP", { notation: "compact" }, "123456789");
parts("en-US", { notation: "compact", style: "currency", currency: "USD" }, "1234567");
fmt("en-US", { notation: "scientific" }, "0");
fmt("en-US", { notation: "engineering" }, "1e21");
fmt("en-US", { notation: "compact" }, "1e15");

// signDisplay.
for (const signDisplay of ["auto", "never", "always", "exceptZero", "negative"]) {
  for (const value of ["-5", "5", "0", "-0", "NaN", "-0.0001"]) {
    if (programs.length < 400) fmt("en-US", { signDisplay, maximumFractionDigits: 2 }, value);
  }
}
parts("en-US", { signDisplay: "exceptZero" }, "0");
parts("en-US", { signDisplay: "negative" }, "-0");
parts("en-US", { signDisplay: "always" }, "NaN");
parts("en-US", { signDisplay: "exceptZero", style: "currency", currency: "USD", currencySign: "accounting" }, "-3");

// roundingMode.
for (const roundingMode of ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"]) {
  for (const value of ["2.5", "-2.5", "1.25"]) {
    fmt("en-US", { roundingMode, maximumFractionDigits: 0 }, value);
  }
}
fmt("en-US", { roundingMode: "halfEven", maximumFractionDigits: 1 }, "0.25");
fmt("en-US", { roundingMode: "ceil", maximumFractionDigits: 1 }, "-0.05");

// roundingIncrement.
fmt("en-US", { roundingIncrement: 5, maximumFractionDigits: 2, minimumFractionDigits: 2 }, "1.23");
fmt("en-US", { roundingIncrement: 50, maximumFractionDigits: 2, minimumFractionDigits: 2 }, "1.77");
fmt("en-US", { roundingIncrement: 250, maximumFractionDigits: 3, minimumFractionDigits: 3 }, "0.6");
programs.push(`new Intl.NumberFormat("en-US", { roundingIncrement: 5 })`);
programs.push(`new Intl.NumberFormat("en-US", { roundingIncrement: 3, maximumFractionDigits: 2, minimumFractionDigits: 2 })`);
programs.push(`new Intl.NumberFormat("en-US", { roundingIncrement: 5, maximumFractionDigits: 1, minimumFractionDigits: 2 })`);

// roundingPriority e trailingZeroDisplay.
fmt("en-US", { roundingPriority: "morePrecision", maximumSignificantDigits: 3, maximumFractionDigits: 1 }, "1.2345");
fmt("en-US", { roundingPriority: "lessPrecision", maximumSignificantDigits: 3, maximumFractionDigits: 1 }, "1.2345");
fmt("en-US", { roundingPriority: "lessPrecision", maximumSignificantDigits: 2, maximumFractionDigits: 3 }, "123.4567");
fmt("en-US", { roundingPriority: "morePrecision", minimumSignificantDigits: 2, maximumSignificantDigits: 3, minimumFractionDigits: 2, maximumFractionDigits: 2 }, "5");
fmt("en-US", { trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2 }, "1");
fmt("en-US", { trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2 }, "1.5");
parts("en-US", { trailingZeroDisplay: "stripIfInteger", style: "currency", currency: "USD" }, "5");
programs.push(`new Intl.NumberFormat("en-US", { roundingPriority: "morePrecision", maximumSignificantDigits: 3, maximumFractionDigits: 1 }).resolvedOptions()`);
programs.push(`new Intl.NumberFormat("en-US", { roundingPriority: "lessPrecision", maximumFractionDigits: 1 }).resolvedOptions()`);
programs.push(`new Intl.NumberFormat("en-US", { roundingMode: "trunc", roundingIncrement: 5, minimumFractionDigits: 1, maximumFractionDigits: 1, trailingZeroDisplay: "stripIfInteger" }).resolvedOptions()`);

// BigInt e string decimal.
fmt("en-US", {}, "1234567890123456789012n");
fmt("en-US", {}, "-1234567890123456789012n");
fmt("en-US", {}, '"1234567890123456789012"');
fmt("en-US", {}, '"-0.000000000000000000123"');
fmt("en-US", { maximumFractionDigits: 25 }, '"0.1234567890123456789012345"');
fmt("en-US", { notation: "compact" }, "123456789012345678901234567890n");
fmt("en-US", { notation: "scientific" }, '"123456789012345678901234567890"');
fmt("en-US", { style: "percent" }, '"12345678901234567890.5"');
fmt("en-US", { maximumFractionDigits: 0 }, '"9007199254740993.5"');
fmt("en-US", {}, '"  12  "');
fmt("en-US", {}, '"1e3"');
fmt("en-US", {}, '"Infinity"');
fmt("en-US", {}, '"abc"');
fmt("en-US", {}, '"0x10"');
fmt("en-US", {}, '""');
parts("en-US", {}, "12345678901234567890123n");
parts("en-US", { signDisplay: "always" }, "0n");

// formatRange.
range("en-US", {}, "3", "5");
range("en-US", {}, "3", "3");
range("en-US", { maximumFractionDigits: 0 }, "2.9", "3.1");
range("en-US", { style: "currency", currency: "USD" }, "3", "5");
range("en-US", { style: "currency", currency: "EUR" }, "3", "5");
range("en-US", { style: "percent" }, "0.3", "0.5");
range("en-US", { notation: "compact" }, "1000", "5000000");
range("en-US", { notation: "compact" }, "1500000", "2500000");
range("en-US", { style: "unit", unit: "kilometer" }, "3", "5");
range("en-US", { style: "unit", unit: "kilometer", unitDisplay: "long" }, "3", "5");
range("en-US", { signDisplay: "always" }, "-3", "5");
range("en-US", { signDisplay: "exceptZero" }, "0", "5");
range("en-US", {}, "-Infinity", "Infinity");
range("en-US", {}, "1n", "100000000000000000000000n");
range("en-US", {}, '"1.5"', '"2.5"');
range("de-DE", { style: "currency", currency: "EUR" }, "3", "5");
range("ja-JP", {}, "3", "5");
range("en-US", {}, "5", "3");
range("en-US", {}, "undefined", "3");
range("en-US", {}, "3", "undefined");
range("en-US", {}, "NaN", "3");
range("en-US", {}, "3", "NaN");
range("en-US", { notation: "scientific" }, "1000", "50000");

// formatRangeToParts (source startRange, endRange, shared).
rangeParts("en-US", {}, "3", "5");
rangeParts("en-US", {}, "3", "3");
rangeParts("en-US", { maximumFractionDigits: 0 }, "2.9", "3.1");
rangeParts("en-US", { style: "currency", currency: "USD" }, "3", "5");
rangeParts("en-US", { style: "currency", currency: "EUR" }, "-3", "5");
rangeParts("en-US", { style: "percent" }, "0.3", "0.5");
rangeParts("en-US", { notation: "compact" }, "1000", "5000000");
rangeParts("en-US", { notation: "compact" }, "1500000", "2500000");
rangeParts("en-US", { style: "unit", unit: "kilometer" }, "3", "5");
rangeParts("en-US", { signDisplay: "always" }, "-3", "5");
rangeParts("en-US", { notation: "scientific" }, "1000", "50000");
rangeParts("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }, "2.9", "3.1");
rangeParts("pt-BR", { style: "currency", currency: "BRL" }, "1000", "2000");

const rows = programs.map((source) => {
  const program = `${harness}(${JSON.stringify(source)})`;
  let result;
  try {
    result = (0, eval)(program);
  } catch (error) {
    result = "throw\t" + error;
  }
  // Todo caractere fora do ASCII sai como \uXXXX (o espaço fino, o NBSP e o separador de intervalo
  // aparecem no REPR); o teste aplica a mesma troca no que o porte devolve.
  const ascii = result.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  return `${source}\t${ascii}`;
});
process.stdout.write(require("./golden-prelude.js").assertPublicResult(rows.join("\n") + "\n"));
