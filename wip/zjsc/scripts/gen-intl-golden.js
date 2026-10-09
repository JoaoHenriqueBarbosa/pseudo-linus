// Gera tests/golden/intl_bun.tsv: programas de Intl avaliados no bun 1.4.2 (ICU completo).
// Colunas: fonte do programa (ASCII puro, o que não é ASCII entra por \u), KIND e REPR, no mesmo
// formato de tests/golden/e2e_values.tsv. O serializador é tests/golden/e2e_values_harness.js, o mesmo
// texto que tests/intl_bun_golden.rs embute; a saída do bun é passada para ASCII-safe só na fonte.
// Todos os instantes são fixos (Date.UTC) e todo fuso é explícito, então a saída não depende de TZ.
// Uso: bun scripts/gen-intl-golden.js > tests/golden/intl_bun.tsv
const fs = require("fs");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const programs = [];
const add = (source) => programs.push(source);

const LOCALES = ["en-US", "pt-BR"];
const q = (value) => JSON.stringify(value);

// Instantes fixos usados pelas datas.
const D1 = "new Date(Date.UTC(2024,0,5,15,4,5))";
const D2 = "new Date(Date.UTC(2024,0,9,3,30,0))";
const D3 = "new Date(Date.UTC(2024,5,15,12,0,0))";
const D4 = "new Date(Date.UTC(2025,11,31,23,59,59,999))";

// NumberFormat.
const numbers = ["0", "1", "-1", "1234.5", "1234567.891", "0.256", "-0.0005", "1e21", "123456789012", "0.000001234", "NaN", "Infinity", "-Infinity"];
for (const locale of LOCALES) {
  for (const n of numbers) {
    add(`new Intl.NumberFormat(${q(locale)}).format(${n})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "percent"}).format(${n})`);
  }
  for (const currency of ["BRL", "USD", "EUR"]) {
    for (const n of ["0", "1234.5", "-1234.567", "1e6", "0.005"]) {
      add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}}).format(${n})`);
    }
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencyDisplay: "code"}).format(1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencyDisplay: "name"}).format(1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencyDisplay: "name"}).format(1)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencySign: "accounting"}).format(-1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, notation: "compact"}).format(1234567)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}}).formatToParts(-1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}}).resolvedOptions()`);
  }
  for (const n of ["0", "999", "1000", "1234", "12345", "123456", "1234567", "12345678", "1234567890", "1.5e12", "1e15", "-4200"]) {
    add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).format(${n})`);
    add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact", compactDisplay: "long"}).format(${n})`);
  }
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).formatToParts(1234567)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "scientific"}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "engineering"}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "scientific"}).formatToParts(0.00012)`);
  for (const [unit, display] of [["kilometer", "short"], ["kilometer", "long"], ["kilometer", "narrow"], ["celsius", "short"], ["celsius", "long"], ["byte", "short"], ["kilobyte", "short"], ["megabyte", "long"], ["liter", "long"], ["kilogram", "short"], ["second", "long"], ["minute", "short"], ["hour", "long"], ["day", "long"], ["day", "short"], ["week", "narrow"], ["month", "long"], ["year", "short"], ["percent", "short"], ["mile", "long"], ["meter", "narrow"], ["fahrenheit", "short"], ["gram", "long"], ["kilometer-per-hour", "short"], ["kilometer-per-hour", "long"], ["meter-per-second", "short"], ["mile-per-hour", "long"], ["liter-per-kilometer", "short"]]) {
    for (const n of ["1", "3", "1.5", "1234.5"]) {
      add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: ${q(unit)}, unitDisplay: ${q(display)}}).format(${n})`);
    }
  }
  add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "kilometer", unitDisplay: "long"}).formatToParts(12.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {minimumFractionDigits: 2, maximumFractionDigits: 2}).format(3.14159)`);
  add(`new Intl.NumberFormat(${q(locale)}, {maximumFractionDigits: 0}).format(2.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {maximumFractionDigits: 0}).format(3.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {minimumIntegerDigits: 4}).format(7)`);
  add(`new Intl.NumberFormat(${q(locale)}, {maximumSignificantDigits: 3}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {minimumSignificantDigits: 5}).format(1.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {useGrouping: false}).format(1234567.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {useGrouping: "min2"}).format(1234)`);
  add(`new Intl.NumberFormat(${q(locale)}, {useGrouping: "min2"}).format(12345)`);
  add(`new Intl.NumberFormat(${q(locale)}, {signDisplay: "always"}).format(5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {signDisplay: "exceptZero"}).format(0)`);
  add(`new Intl.NumberFormat(${q(locale)}, {signDisplay: "negative"}).format(-0)`);
  add(`new Intl.NumberFormat(${q(locale)}, {signDisplay: "never"}).format(-5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {roundingMode: "floor"}).format(1.239)`);
  add(`new Intl.NumberFormat(${q(locale)}, {roundingMode: "ceil", maximumFractionDigits: 1}).format(1.21)`);
  add(`new Intl.NumberFormat(${q(locale)}, {roundingMode: "halfEven", maximumFractionDigits: 0}).format(2.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {roundingMode: "expand", maximumFractionDigits: 0}).format(-2.1)`);
  add(`new Intl.NumberFormat(${q(locale)}, {trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2}).format(5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {roundingPriority: "lessPrecision", maximumFractionDigits: 3, maximumSignificantDigits: 2}).format(1.23456)`);
  add(`new Intl.NumberFormat(${q(locale)}).format(12345678901234567890n)`);
  add(`new Intl.NumberFormat(${q(locale)}).format("1234.5678")`);
  add(`new Intl.NumberFormat(${q(locale)}).format("12345678901234567890.123456789")`);
  add(`new Intl.NumberFormat(${q(locale)}).formatToParts(-1234567.891)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatToParts(NaN)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "percent", maximumFractionDigits: 1}).formatToParts(0.12345)`);
  add(`new Intl.NumberFormat(${q(locale)}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "percent"}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "meter"}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {maximumSignificantDigits: 4}).resolvedOptions()`);
  // formatRange e formatRangeToParts.
  const ranges = [["3", "5"], ["3", "3"], ["3", "3.0001"], ["0", "1000"], ["-5", "5"], ["1e6", "2e6"], ["1234.5", "99999.123"], ["5", "Infinity"]];
  for (const [a, b] of ranges) {
    add(`new Intl.NumberFormat(${q(locale)}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "BRL"}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "USD", maximumFractionDigits: 0}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "percent"}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "kilometer", unitDisplay: "long"}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "kilometer", unitDisplay: "short"}).formatRange(${a}, ${b})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "celsius"}).formatRange(${a}, ${b})`);
  }
  add(`new Intl.NumberFormat(${q(locale)}).formatRangeToParts(3, 5)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatRangeToParts(3, 3)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "EUR"}).formatRangeToParts(1234.5, 6789)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "EUR", currencyDisplay: "name"}).formatRange(1, 5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "USD", currencyDisplay: "name"}).formatRange(1, 1)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "day", unitDisplay: "long"}).formatRange(1, 2)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatRange(NaN, 5)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatRange(5)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatRange(5, undefined)`);
  add(`(1234567.891).toLocaleString(${q(locale)})`);
  add(`(12345n).toLocaleString(${q(locale)})`);
}

// DateTimeFormat.
const zones = ["UTC", "America/Sao_Paulo"];
const dates = [D1, D2, D3, D4];
for (const locale of LOCALES) {
  for (const zone of zones) {
    for (const date of dates) {
      for (const style of ["full", "long", "medium", "short"]) {
        add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: ${q(style)}, timeZone: ${q(zone)}}).format(${date})`);
        add(`new Intl.DateTimeFormat(${q(locale)}, {timeStyle: ${q(style)}, timeZone: ${q(zone)}}).format(${date})`);
      }
      add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "full", timeStyle: "long", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "short", timeStyle: "short", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "medium", timeStyle: "medium", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "long", day: "numeric", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "short", day: "numeric", weekday: "short", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {year: "2-digit", month: "2-digit", day: "2-digit", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", minute: "2-digit", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", minute: "2-digit", second: "2-digit", hour12: false, timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {month: "long", year: "numeric", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {month: "long", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {weekday: "long", timeZone: ${q(zone)}}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}, timeZoneName: "short", hour: "numeric"}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}, timeZoneName: "long", hour: "numeric"}).format(${date})`);
    }
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "full", timeStyle: "full", timeZone: ${q(zone)}}).formatToParts(${D1})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "short", timeStyle: "medium", timeZone: ${q(zone)}}).formatToParts(${D2})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "long", day: "numeric", weekday: "long", hour: "numeric", minute: "numeric", second: "numeric", timeZone: ${q(zone)}}).formatToParts(${D3})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}}).formatToParts(${D1})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}}).resolvedOptions()`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "long", timeStyle: "short", timeZone: ${q(zone)}}).resolvedOptions()`);
    // formatRange e formatRangeToParts.
    const pairs = [[D1, D1], [D1, D2], [D1, D3], [D1, D4], [D1, "new Date(Date.UTC(2024,0,5,18,30,0))"], [D1, "new Date(Date.UTC(2024,0,5,15,4,9))"]];
    for (const [a, b] of pairs) {
      add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "medium", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "long", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "full", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "short", timeStyle: "short", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {timeStyle: "short", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {month: "long", day: "numeric", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {month: "short", day: "numeric", year: "numeric", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", minute: "numeric", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "long", timeZone: ${q(zone)}}).formatRange(${a}, ${b})`);
    }
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "medium", timeZone: ${q(zone)}}).formatRangeToParts(${D1}, ${D2})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "medium", timeZone: ${q(zone)}}).formatRangeToParts(${D1}, ${D1})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {timeStyle: "short", timeZone: ${q(zone)}}).formatRangeToParts(${D1}, new Date(Date.UTC(2024,0,5,18,30,0)))`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "short", timeStyle: "short", timeZone: ${q(zone)}}).formatRangeToParts(${D1}, ${D3})`);
  }
  add(`${D1}.toLocaleString(${q(locale)}, {timeZone: "UTC"})`);
  add(`${D1}.toLocaleDateString(${q(locale)}, {timeZone: "America/Sao_Paulo"})`);
  add(`${D1}.toLocaleTimeString(${q(locale)}, {timeZone: "America/Sao_Paulo"})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "UTC"}).format(new Date(NaN))`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "Nowhere/Land"})`);
}
add(`Intl.DateTimeFormat.supportedLocalesOf(["en-US", "pt-BR", "xx"])`);

// RelativeTimeFormat.
const units = ["second", "minute", "hour", "day", "week", "month", "quarter", "year"];
for (const locale of LOCALES) {
  for (const numeric of ["always", "auto"]) {
    for (const style of ["long", "short", "narrow"]) {
      for (const unit of units) {
        for (const n of ["-2", "-1", "0", "1", "2", "5", "-5", "1.5", "-0", "1000"]) {
          add(`new Intl.RelativeTimeFormat(${q(locale)}, {numeric: ${q(numeric)}, style: ${q(style)}}).format(${n}, ${q(unit)})`);
        }
      }
    }
  }
  add(`new Intl.RelativeTimeFormat(${q(locale)}).formatToParts(-3, "day")`);
  add(`new Intl.RelativeTimeFormat(${q(locale)}, {numeric: "auto"}).formatToParts(1, "day")`);
  add(`new Intl.RelativeTimeFormat(${q(locale)}).formatToParts(1234.5, "hour")`);
  add(`new Intl.RelativeTimeFormat(${q(locale)}).resolvedOptions()`);
  add(`new Intl.RelativeTimeFormat(${q(locale)}).format(1, "days")`);
  add(`new Intl.RelativeTimeFormat(${q(locale)}).format(1, "fortnight")`);
}

// ListFormat.
const lists = ["[]", '["a"]', '["a","b"]', '["a","b","c"]', '["a","b","c","d"]'];
for (const locale of LOCALES) {
  for (const type of ["conjunction", "disjunction", "unit"]) {
    for (const style of ["long", "short", "narrow"]) {
      for (const list of lists) {
        add(`new Intl.ListFormat(${q(locale)}, {type: ${q(type)}, style: ${q(style)}}).format(${list})`);
      }
    }
  }
  add(`new Intl.ListFormat(${q(locale)}).formatToParts(["a","b","c"])`);
  add(`new Intl.ListFormat(${q(locale)}, {type: "disjunction"}).formatToParts(["x","y"])`);
  add(`new Intl.ListFormat(${q(locale)}).resolvedOptions()`);
}

// PluralRules.
for (const locale of LOCALES) {
  for (const type of ["cardinal", "ordinal"]) {
    for (const n of ["0", "1", "2", "3", "4", "5", "11", "12", "13", "21", "22", "23", "101", "111", "1.5", "0.5", "1000000", "-1", "NaN", "Infinity"]) {
      add(`new Intl.PluralRules(${q(locale)}, {type: ${q(type)}}).select(${n})`);
    }
    add(`new Intl.PluralRules(${q(locale)}, {type: ${q(type)}}).resolvedOptions()`);
  }
  add(`new Intl.PluralRules(${q(locale)}, {minimumFractionDigits: 1}).select(1)`);
  add(`new Intl.PluralRules(${q(locale)}, {notation: "compact"}).select(1000000)`);
  add(`new Intl.PluralRules(${q(locale)}).selectRange(1, 5)`);
  add(`new Intl.PluralRules(${q(locale)}).selectRange(0, 1)`);
}
add(`new Intl.PluralRules("pt-BR").select(1000000)`);
add(`new Intl.PluralRules("pt-BR").select(2000000)`);
add(`new Intl.PluralRules("pt-BR", {notation: "compact"}).select(2000000)`);
add(`new Intl.PluralRules("pt-BR").select(0)`);
add(`new Intl.PluralRules("pt-BR").select(1.5)`);

// Collator.
const wordLists = [
  '["z","a","Z","A","\\u00e1","\\u00e0","b","B"]',
  '["resume","r\\u00e9sum\\u00e9","Resume","resum\\u00e9"]',
  '["a10","a2","a1","a20"]',
  '["\\u00e7a","ca","cb","da","\\u00e7b"]',
  '["\\u00e4","a","z","\\u00f6","o"]',
  '["a b","ab","a-b","a_b","a.b"]',
  '["","a"," ","A"]',
  '["\\u00c9","e","E","\\u00e9","f"]',
];
for (const locale of LOCALES) {
  for (const list of wordLists) {
    add(`${list}.sort(new Intl.Collator(${q(locale)}).compare)`);
    add(`${list}.sort(new Intl.Collator(${q(locale)}, {numeric: true}).compare)`);
    add(`${list}.sort(new Intl.Collator(${q(locale)}, {sensitivity: "base"}).compare)`);
    add(`${list}.sort(new Intl.Collator(${q(locale)}, {sensitivity: "accent"}).compare)`);
    add(`${list}.sort(new Intl.Collator(${q(locale)}, {caseFirst: "upper"}).compare)`);
    add(`${list}.sort(new Intl.Collator(${q(locale)}, {ignorePunctuation: true}).compare)`);
  }
  add(`new Intl.Collator(${q(locale)}, {sensitivity: "base"}).compare("a", "A")`);
  add(`new Intl.Collator(${q(locale)}, {sensitivity: "base"}).compare("a", "\\u00e1")`);
  add(`new Intl.Collator(${q(locale)}, {sensitivity: "accent"}).compare("a", "A")`);
  add(`new Intl.Collator(${q(locale)}, {sensitivity: "case"}).compare("a", "\\u00e1")`);
  add(`new Intl.Collator(${q(locale)}).compare("a", "b")`);
  add(`new Intl.Collator(${q(locale)}).compare("b", "a")`);
  add(`new Intl.Collator(${q(locale)}).compare("a", "a")`);
  add(`new Intl.Collator(${q(locale)}, {numeric: true}).compare("2", "10")`);
  add(`new Intl.Collator(${q(locale)}).resolvedOptions()`);
  add(`new Intl.Collator(${q(locale)}, {usage: "search", sensitivity: "base"}).resolvedOptions()`);
  add(`["b", "a", "c"].sort(new Intl.Collator(${q(locale)}).compare)`);
  add(`"a".localeCompare("b", ${q(locale)})`);
  add(`"\\u00e1".localeCompare("a", ${q(locale)}, {sensitivity: "base"})`);
}

// DisplayNames.
const displayCases = [
  ["language", ["pt", "pt-BR", "en", "en-US", "fr", "es", "de", "ja", "zh", "zh-Hans", "ru", "xx"]],
  ["region", ["BR", "US", "PT", "GB", "FR", "DE", "JP", "CN", "AR", "ZZ"]],
  ["script", ["Latn", "Cyrl", "Arab", "Hans", "Hant", "Jpan"]],
  ["currency", ["BRL", "USD", "EUR", "JPY", "GBP", "XXX"]],
  ["calendar", ["gregory", "buddhist", "japanese", "iso8601"]],
  ["dateTimeField", ["year", "month", "weekOfYear", "weekday", "day", "dayPeriod", "hour", "minute", "second", "timeZoneName", "era", "quarter"]],
];
for (const locale of LOCALES) {
  for (const [type, codes] of displayCases) {
    for (const code of codes) {
      add(`new Intl.DisplayNames(${q(locale)}, {type: ${q(type)}}).of(${q(code)})`);
    }
  }
  add(`new Intl.DisplayNames(${q(locale)}, {type: "language", languageDisplay: "standard"}).of("en-US")`);
  add(`new Intl.DisplayNames(${q(locale)}, {type: "language", style: "short"}).of("en-US")`);
  add(`new Intl.DisplayNames(${q(locale)}, {type: "region", style: "short"}).of("US")`);
  add(`new Intl.DisplayNames(${q(locale)}, {type: "region", fallback: "none"}).of("ZZ")`);
  add(`new Intl.DisplayNames(${q(locale)}, {type: "language"}).resolvedOptions()`);
  add(`new Intl.DisplayNames(${q(locale)}, {type: "region"}).of("zz!")`);
  add(`new Intl.DisplayNames(${q(locale)})`);
}

// Segmenter.
const texts = ['"Hello world. How are you?"', '"a b  c"', '"caf\\u00e9"', '"Ol\\u00e1, mundo! Tudo bem?"', '"e\\u0301a"', '"\\ud83d\\ude00\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67x"', '"one,two;three"'];
for (const locale of LOCALES) {
  for (const granularity of ["grapheme", "word", "sentence"]) {
    for (const text of texts) {
      add(`Array.from(new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).segment(${text}), function (s) { return [s.segment, s.index, s.isWordLike]; })`);
    }
  }
  add(`new Intl.Segmenter(${q(locale)}, {granularity: "word"}).segment("Hello world").containing(7)`);
  add(`new Intl.Segmenter(${q(locale)}).resolvedOptions()`);
}

// Locales e supportedLocalesOf.
for (const ctor of ["NumberFormat", "DateTimeFormat", "Collator", "PluralRules", "RelativeTimeFormat", "ListFormat", "DisplayNames", "Segmenter"]) {
  add(`Intl.${ctor}.supportedLocalesOf(["en-US", "pt-BR", "en", "pt", "fr", "ja", "xx"])`);
  add(`Intl.${ctor}.supportedLocalesOf("pt-BR")`);
  add(`Intl.${ctor}.supportedLocalesOf([])`);
  add(`Intl.${ctor}.supportedLocalesOf(["en-GB", "pt-PT", "en-IN", "pt-AO"])`);
  add(`Intl.${ctor}.supportedLocalesOf(["pt-BR-u-nu-latn"])`);
  add(`Intl.${ctor}.supportedLocalesOf(["not a locale"])`);
}
add(`Intl.getCanonicalLocales(["EN-us", "pt-br", "zh-hans-cn"])`);
add(`new Intl.NumberFormat("en-GB").resolvedOptions().locale`);
add(`new Intl.NumberFormat("pt-PT").format(1234567.891)`);
add(`new Intl.NumberFormat("en-GB", {style: "currency", currency: "GBP"}).format(1234.5)`);
add(`new Intl.NumberFormat("en-IN").format(12345678)`);
add(`new Intl.DateTimeFormat("en-GB", {timeZone: "UTC"}).format(${D1})`);
add(`new Intl.DateTimeFormat("pt-PT", {timeZone: "UTC", dateStyle: "long"}).format(${D1})`);
add(`new Intl.NumberFormat("fr").format(1234567.891)`);
add(`new Intl.NumberFormat("de").format(1234567.891)`);
add(`new Intl.NumberFormat("de", {style: "currency", currency: "EUR"}).format(1234.5)`);
add(`new Intl.NumberFormat("ja", {notation: "compact"}).format(123456789)`);
add(`new Intl.DateTimeFormat("ja", {timeZone: "UTC", dateStyle: "full"}).format(${D1})`);
add(`new Intl.DateTimeFormat("de", {timeZone: "UTC", dateStyle: "full"}).format(${D1})`);
add(`new Intl.PluralRules("ru").select(3)`);
add(`new Intl.PluralRules("ar").select(2)`);
add(`new Intl.Locale("pt-BR").maximize().toString()`);
add(`new Intl.Locale("en").maximize().toString()`);
add(`Intl.supportedValuesOf("calendar")`);
add(`Intl.supportedValuesOf("numberingSystem").length > 1`);
add(`Intl.supportedValuesOf("currency").includes("BRL")`);
add(`Intl.supportedValuesOf("unit")`);

// Intl.Locale: getters, maximize/minimize, getters de informação e extensões u-.
const localeTags = [
  "en", "en-US", "pt-BR", "pt", "fr-CA", "de-DE", "ja-JP", "ar-EG", "ar", "hi-IN", "zh-Hans-CN", "zh-TW", "sr-Latn-RS", "und", "und-Latn",
  "en-u-ca-buddhist", "en-u-nu-arab-hc-h23", "pt-BR-u-ca-gregory-co-phonebk-kf-upper-kn-true", "de-u-co-phonebk", "ja-u-ca-japanese",
  "ar-u-nu-latn-ca-islamic", "hi-u-nu-deva", "en-US-u-fw-mon-hc-h12", "th-u-ca-buddhist-nu-thai", "en-t-hi", "en-x-private", "fa-IR", "he-IL", "ru-RU",
];
const localeGetters = ["language", "script", "region", "baseName", "calendar", "collation", "hourCycle", "caseFirst", "numeric", "numberingSystem", "firstDayOfWeek"];
for (const tag of localeTags) {
  for (const getter of localeGetters) add(`new Intl.Locale(${q(tag)}).${getter}`);
  add(`new Intl.Locale(${q(tag)}).toString()`);
  add(`new Intl.Locale(${q(tag)}).maximize().toString()`);
  add(`new Intl.Locale(${q(tag)}).minimize().toString()`);
  add(`new Intl.Locale(${q(tag)}).getCalendars()`);
  add(`new Intl.Locale(${q(tag)}).getCollations()`);
  add(`new Intl.Locale(${q(tag)}).getHourCycles()`);
  add(`new Intl.Locale(${q(tag)}).getNumberingSystems()`);
  add(`new Intl.Locale(${q(tag)}).getTimeZones()`);
  add(`new Intl.Locale(${q(tag)}).getTextInfo()`);
  add(`new Intl.Locale(${q(tag)}).getWeekInfo()`);
  add(`new Intl.Locale(${q(tag)}).calendars`);
  add(`new Intl.Locale(${q(tag)}).weekInfo`);
  add(`new Intl.Locale(${q(tag)}).textInfo`);
}
const localeOptions = [
  `{calendar: "buddhist"}`, `{collation: "phonebk"}`, `{hourCycle: "h11"}`, `{caseFirst: "lower"}`, `{numeric: true}`, `{numberingSystem: "arab"}`,
  `{language: "fr"}`, `{script: "Latn"}`, `{region: "CA"}`, `{firstDayOfWeek: "mon"}`, `{firstDayOfWeek: 3}`, `{calendar: "islamic-civil", hourCycle: "h23"}`,
];
for (const options of localeOptions) {
  add(`new Intl.Locale("pt-BR", ${options}).toString()`);
  add(`new Intl.Locale("en-u-ca-gregory", ${options}).toString()`);
}
add(`new Intl.Locale(new Intl.Locale("pt-BR-u-ca-gregory")).toString()`);
add(`new Intl.Locale("en-US").maximize().minimize().toString()`);
add(`new Intl.Locale("zh-Hant").minimize().toString()`);
add(`new Intl.Locale("sr-Cyrl").maximize().toString()`);
add(`new Intl.Locale("")`);
add(`new Intl.Locale("en_US")`);
add(`new Intl.Locale()`);
add(`new Intl.Locale("en", {calendar: "x"})`);
add(`Intl.Locale.length`);
add(`Object.prototype.toString.call(new Intl.Locale("en"))`);
add(`Intl.getCanonicalLocales("EN-us")`);
add(`Intl.getCanonicalLocales([])`);
add(`Intl.getCanonicalLocales(undefined)`);
add(`Intl.getCanonicalLocales(["en-US", "en-us", "EN-US"])`);
add(`Intl.getCanonicalLocales("pt-BR-u-ca-gregory-nu-latn")`);
add(`Intl.getCanonicalLocales("en-u-nu-latn-ca-gregory")`);
add(`Intl.getCanonicalLocales("en-u-ca-islamicc")`);
add(`Intl.getCanonicalLocales("en-u-ks-primary-kn")`);
add(`Intl.getCanonicalLocales("iw")`);
add(`Intl.getCanonicalLocales("in-ID")`);
add(`Intl.getCanonicalLocales("sh")`);
add(`Intl.getCanonicalLocales("zh-cmn-Hans-CN")`);
add(`Intl.getCanonicalLocales("de-DD")`);
add(`Intl.getCanonicalLocales("en-x-Foo")`);
add(`Intl.getCanonicalLocales("en-t-HI-u-ca-Gregory")`);
add(`Intl.getCanonicalLocales("und-latn-us")`);
add(`Intl.getCanonicalLocales("i-klingon")`);
add(`Intl.getCanonicalLocales("en-")`);
add(`Intl.getCanonicalLocales("en--US")`);
add(`Intl.getCanonicalLocales("e")`);
add(`Intl.getCanonicalLocales(["en", 5])`);
add(`Intl.getCanonicalLocales(null)`);

// Intl.supportedValuesOf para cada chave.
for (const key of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"]) {
  add(`Intl.supportedValuesOf(${q(key)})`);
  add(`Intl.supportedValuesOf(${q(key)}).length`);
  add(`Intl.supportedValuesOf(${q(key)}).slice().sort().join() === Intl.supportedValuesOf(${q(key)}).join()`);
}
add(`Intl.supportedValuesOf("timezone")`);
add(`Intl.supportedValuesOf()`);
add(`Intl.supportedValuesOf("region")`);
add(`Intl.supportedValuesOf("currency").includes("EUR")`);
add(`Intl.supportedValuesOf("timeZone").includes("America/Sao_Paulo")`);
add(`Intl.supportedValuesOf("timeZone").includes("UTC")`);
add(`Intl.supportedValuesOf("timeZone").includes("Asia/Calcutta")`);
add(`Intl.supportedValuesOf("timeZone").includes("Asia/Kolkata")`);
add(`Intl.supportedValuesOf("numberingSystem").includes("arab")`);
add(`Intl.supportedValuesOf("numberingSystem").includes("deva")`);
add(`Intl.supportedValuesOf("calendar").includes("islamic-umalqura")`);
add(`Intl.supportedValuesOf("collation").includes("phonebk")`);
add(`Intl.supportedValuesOf("unit").includes("kilometer-per-hour")`);

// DateTimeFormat e NumberFormat em mais locales.
const wideLocales = ["en-US", "pt-BR", "fr", "de", "ja", "ar", "hi"];
for (const locale of wideLocales) {
  for (const date of [D1, D3]) {
    for (const style of ["full", "long", "medium", "short"]) {
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: ${q(style)}, timeStyle: ${q(style)}, timeZone: "UTC"}).format(${date})`);
      add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: ${q(style)}, timeZone: "Asia/Tokyo"}).format(${date})`);
    }
    add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "long", day: "numeric", weekday: "long", hour: "numeric", minute: "numeric", second: "numeric", timeZone: "UTC"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "full", timeStyle: "full", timeZone: "America/Sao_Paulo"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "short", timeStyle: "short", timeZone: "UTC"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {month: "long", year: "numeric", timeZone: "UTC"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", minute: "numeric", dayPeriod: "long", timeZone: "UTC"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", fractionalSecondDigits: 3, minute: "numeric", second: "numeric", timeZone: "UTC"}).formatToParts(${D4})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {era: "short", year: "numeric", timeZone: "UTC"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {timeZoneName: "short", hour: "numeric", timeZone: "Asia/Kolkata"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {timeZoneName: "longGeneric", hour: "numeric", timeZone: "America/Sao_Paulo"}).formatToParts(${date})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "long", timeZone: "UTC"}).formatRange(${D1}, ${D2})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "medium", timeZone: "UTC"}).formatRangeToParts(${D1}, ${D3})`);
  }
  add(`new Intl.DateTimeFormat(${q(locale)}).resolvedOptions().locale`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "UTC"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {dateStyle: "full", timeStyle: "long", timeZone: "Asia/Kolkata"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", minute: "numeric", timeZone: "UTC"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {hour: "numeric", hour12: false, timeZone: "UTC"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {year: "numeric", month: "short", day: "numeric", weekday: "short", timeZone: "UTC"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {calendar: "buddhist", timeZone: "UTC"}).resolvedOptions()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {calendar: "japanese", dateStyle: "long", timeZone: "UTC"}).format(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {calendar: "islamic-umalqura", dateStyle: "long", timeZone: "UTC"}).format(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {numberingSystem: "arab", dateStyle: "short", timeZone: "UTC"}).format(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {numberingSystem: "deva", dateStyle: "short", timeZone: "UTC"}).format(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {hourCycle: "h11", hour: "numeric", timeZone: "UTC"}).format(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "UTC"}).formatToParts(${D1})`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "UTC"}).formatToParts(${D1}).map(function (p) { return p.type; }).join()`);
  add(`new Intl.DateTimeFormat(${q(locale)}, {timeZone: "UTC", month: "narrow", weekday: "narrow"}).format(${D1})`);
  add(`Intl.DateTimeFormat.supportedLocalesOf(${q(locale)})`);
  add(`new Intl.DateTimeFormat("${locale}-u-ca-buddhist-nu-latn", {timeZone: "UTC"}).resolvedOptions()`);
}
for (const locale of ["fr", "de", "ja", "ar", "hi"]) {
  for (const n of ["0", "1", "-1", "1234.5", "1234567.891", "0.256", "-0.0005", "1e21", "NaN", "Infinity", "12345678901234567890n"]) {
    add(`new Intl.NumberFormat(${q(locale)}).format(${n})`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "percent"}).format(${n})`);
  }
  for (const currency of ["BRL", "USD", "EUR", "JPY", "INR"]) {
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}}).format(-1234.567)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencyDisplay: "name"}).format(1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencyDisplay: "code"}).format(1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}, currencySign: "accounting"}).format(-1234.5)`);
    add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: ${q(currency)}}).formatToParts(-1234.5)`);
  }
  for (const n of ["0", "999", "1000", "12345", "1234567", "1234567890", "1.5e12", "-4200"]) {
    add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).format(${n})`);
    add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact", compactDisplay: "long"}).format(${n})`);
  }
  for (const [unit, display] of [["kilometer", "long"], ["kilometer", "short"], ["celsius", "short"], ["byte", "long"], ["liter", "long"], ["hour", "long"], ["day", "short"], ["kilometer-per-hour", "short"], ["percent", "long"]]) {
    for (const n of ["1", "3", "1234.5"]) {
      add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: ${q(unit)}, unitDisplay: ${q(display)}}).format(${n})`);
    }
  }
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "scientific"}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "engineering"}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).formatToParts(1234567)`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "scientific"}).formatToParts(0.00012)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatToParts(-1234567.891)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatToParts(NaN)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "percent", maximumFractionDigits: 1}).formatToParts(0.12345)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "unit", unit: "kilometer", unitDisplay: "long"}).formatToParts(12.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {signDisplay: "always"}).format(5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {useGrouping: "min2"}).format(1234)`);
  add(`new Intl.NumberFormat(${q(locale)}, {useGrouping: false}).format(1234567.5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {minimumFractionDigits: 2, maximumFractionDigits: 2}).format(3.14159)`);
  add(`new Intl.NumberFormat(${q(locale)}, {maximumSignificantDigits: 3}).format(123456)`);
  add(`new Intl.NumberFormat(${q(locale)}, {numberingSystem: "latn"}).format(1234567.891)`);
  add(`new Intl.NumberFormat(${q(locale)}, {numberingSystem: "arab"}).format(1234567.891)`);
  add(`new Intl.NumberFormat(${q(locale)}, {numberingSystem: "deva"}).format(1234567.891)`);
  add(`new Intl.NumberFormat(${q(locale)}).formatRange(3, 5)`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "EUR"}).formatRange(1234.5, 6789)`);
  add(`new Intl.NumberFormat(${q(locale)}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {style: "currency", currency: "EUR"}).resolvedOptions()`);
  add(`new Intl.NumberFormat(${q(locale)}, {notation: "compact"}).resolvedOptions()`);
  add(`new Intl.NumberFormat("${locale}-u-nu-latn").resolvedOptions()`);
  add(`(1234567.891).toLocaleString(${q(locale)})`);
  add(`Intl.NumberFormat.supportedLocalesOf(${q(locale)})`);
}

// Segmenter com emoji e acentos, em mais locales.
const segTexts = [
  '"Ol\\u00e1, mundo! Tudo bem? Sim."', '"Cora\\u00e7\\u00e3o d\\u00e1 a\\u00e7\\u00e3o."', '"e\\u0301a\\u0302 n\\u0303"',
  '"\\ud83d\\ude00\\ud83d\\udc4d\\ud83c\\udffd\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67\\u200d\\ud83d\\udc66x"',
  '"\\ud83c\\udde7\\ud83c\\uddf7\\ud83c\\uddfa\\ud83c\\uddf8\\ud83c\\udde9\\ud83c\\uddea"', '"\\u2764\\ufe0f\\ud83c\\udff3\\ufe0f\\u200d\\ud83c\\udf08 ok"',
  '"Dr. Silva chegou. Ele disse: \\"Vamos!\\" Ent\\u00e3o sa\\u00edmos."', '"3.14 e 1,5 s\\u00e3o n\\u00fameros; foo@bar.com"',
  '"\\u3053\\u3093\\u306b\\u3061\\u306f\\u4e16\\u754c\\u3002\\u3055\\u3088\\u3046\\u306a\\u3089\\uff01"', '"\\u0645\\u0631\\u062d\\u0628\\u0627 \\u0628\\u0627\\u0644\\u0639\\u0627\\u0644\\u0645. \\u0643\\u064a\\u0641 \\u062d\\u0627\\u0644\\u0643\\u061f"',
  '"\\u0928\\u092e\\u0938\\u094d\\u0924\\u0947 \\u0926\\u0941\\u0928\\u093f\\u092f\\u093e\\u0964 \\u0915\\u094d\\u092f\\u093e \\u0939\\u093e\\u0932 \\u0939\\u0948?"',
  '"\\ud55c\\uad6d\\uc5b4 \\ud14d\\uc2a4\\ud2b8"', '"line1\\r\\nline2\\nline3"', '""',
];
for (const locale of ["en-US", "pt-BR", "fr", "de", "ja", "ar", "hi"]) {
  for (const granularity of ["grapheme", "word", "sentence"]) {
    for (const text of segTexts) {
      add(`Array.from(new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).segment(${text}), function (s) { return [s.segment, s.index, s.isWordLike]; })`);
    }
    add(`new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).segment("Ol\\u00e1 \\ud83d\\ude00 mundo. Fim!").containing(5)`);
    add(`new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).segment("Ol\\u00e1 \\ud83d\\ude00 mundo. Fim!").containing(100)`);
    add(`new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).segment("abc").input`);
    add(`new Intl.Segmenter(${q(locale)}, {granularity: ${q(granularity)}}).resolvedOptions()`);
  }
}

const seen = new Set();
for (const source of programs) {
  if (seen.has(source)) continue;
  seen.add(source);
  if (/[^\x20-\x7e]/.test(source)) throw new Error("fonte não ASCII: " + source);
  let kind, repr;
  const out = (0, eval)(harness + "(" + JSON.stringify(source) + ")");
  const tab = out.indexOf("\t");
  kind = out.slice(0, tab);
  repr = out.slice(tab + 1);
  process.stdout.write(source + "\t" + kind + "\t" + repr.replace(/[\r\n]/g, " ") + "\n");
}
