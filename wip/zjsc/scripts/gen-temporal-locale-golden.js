// Gera tests/golden/temporal_locale_bun.tsv: programas de uma linha sobre toLocaleString,
// Intl.DateTimeFormat (format, formatToParts, formatRange, resolvedOptions) e Date.prototype.toTemporalInstant
// com objetos Temporal, avaliados no bun. Colunas: fonte do programa e o resultado
// (`ok:<JSON>` ou `throw:<Nome>: <mensagem>`), com o que passa de ASCII escapado como \uXXXX. O avaliador
// é tests/golden/temporal_locale_harness.js, o MESMO texto que tests/temporal_locale_bun_golden.rs embute.
// Nenhum programa depende do fuso da máquina: todo uso de Instant ou Date leva timeZone explícito.
// Uso: TZ=UTC bun scripts/gen-temporal-locale-golden.js > tests/golden/temporal_locale_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const path = require("path");

process.env.TZ = "UTC";
const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/temporal_locale_harness.js"), "utf8").trim();
const evaluate = (0, eval)(harness);

const programs = [];
const add = (source) => programs.push(source);
const q = (text) => JSON.stringify(text);

const fixtures = {
  PlainDate: "Temporal.PlainDate.from('2024-01-05')",
  PlainTime: "Temporal.PlainTime.from('15:04:05.123')",
  PlainDateTime: "Temporal.PlainDateTime.from('2024-01-05T15:04:05.123')",
  PlainYearMonth: "Temporal.PlainYearMonth.from('2024-01')",
  PlainMonthDay: "Temporal.PlainMonthDay.from('01-05')",
  Instant: "Temporal.Instant.from('2024-01-05T15:04:05.123Z')",
  ZonedSaoPaulo: "Temporal.ZonedDateTime.from('2024-01-05T15:04:05.123-03:00[America/Sao_Paulo]')",
  ZonedUTC: "Temporal.ZonedDateTime.from('2024-01-05T15:04:05.123+00:00[UTC]')",
  ZonedOffset: "Temporal.ZonedDateTime.from('2024-01-05T15:04:05.123+05:30[+05:30]')",
  Duration: "Temporal.Duration.from('P1Y2M3DT4H5M6S')",
};
const locales = ["en-US", "pt-BR", "de", "ja"];

// Opções por tipo: o que cada um aceita e o que ele recusa.
const dateOptions = [
  "{}", "{dateStyle: 'short'}", "{dateStyle: 'medium'}", "{dateStyle: 'long'}", "{dateStyle: 'full'}",
  "{year: 'numeric'}", "{month: 'long', day: 'numeric'}", "{year: 'numeric', month: '2-digit', day: '2-digit'}",
  "{weekday: 'long'}", "{weekday: 'short', month: 'short', day: 'numeric'}", "{month: 'long', year: 'numeric'}",
  "{era: 'short', year: 'numeric'}", "{timeStyle: 'short'}", "{hour: 'numeric'}", "{timeZoneName: 'short'}",
];
const timeOptions = [
  "{}", "{timeStyle: 'short'}", "{timeStyle: 'medium'}", "{timeStyle: 'long'}", "{timeStyle: 'full'}",
  "{hour: 'numeric'}", "{hour: '2-digit', minute: '2-digit'}", "{hour: 'numeric', minute: 'numeric', hour12: false}",
  "{hour: 'numeric', minute: 'numeric', hour12: true}", "{hour: 'numeric', minute: 'numeric', second: 'numeric', fractionalSecondDigits: 3}",
  "{dateStyle: 'short'}", "{year: 'numeric'}", "{timeZoneName: 'short'}", "{hour: 'numeric', hourCycle: 'h23'}",
];
const dateTimeOptions = [
  "{}", "{dateStyle: 'short', timeStyle: 'short'}", "{dateStyle: 'medium', timeStyle: 'medium'}",
  "{dateStyle: 'long', timeStyle: 'long'}", "{dateStyle: 'full', timeStyle: 'full'}", "{dateStyle: 'long'}", "{timeStyle: 'short'}",
  "{year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: 'numeric'}", "{hour: 'numeric', minute: 'numeric', hour12: false}",
  "{month: 'short', day: 'numeric', hour: 'numeric'}", "{timeZoneName: 'short'}", "{hour: 'numeric'}", "{year: 'numeric'}",
];
const yearMonthOptions = [
  "{calendar: 'iso8601'}", "{calendar: 'iso8601', dateStyle: 'long'}", "{calendar: 'iso8601', year: 'numeric', month: 'long'}",
  "{calendar: 'iso8601', month: 'short'}", "{calendar: 'iso8601', day: 'numeric'}", "{calendar: 'iso8601', hour: 'numeric'}",
  "{calendar: 'iso8601', dateStyle: 'short'}", "{}", "{calendar: 'gregory'}",
];
const monthDayOptions = [
  "{calendar: 'iso8601'}", "{calendar: 'iso8601', dateStyle: 'long'}", "{calendar: 'iso8601', month: 'long', day: 'numeric'}",
  "{calendar: 'iso8601', year: 'numeric'}", "{calendar: 'iso8601', dateStyle: 'short'}", "{calendar: 'iso8601', timeStyle: 'short'}", "{}",
];
const instantOptions = [
  "{timeZone: 'UTC'}", "{timeZone: 'America/Sao_Paulo'}", "{timeZone: 'UTC', dateStyle: 'short'}",
  "{timeZone: 'America/Sao_Paulo', dateStyle: 'full', timeStyle: 'long'}", "{timeZone: 'UTC', timeZoneName: 'short'}",
  "{timeZone: 'America/Sao_Paulo', timeZoneName: 'short'}", "{timeZone: 'America/Sao_Paulo', timeZoneName: 'long'}",
  "{timeZone: 'UTC', hour: 'numeric', minute: 'numeric', hour12: false}", "{timeZone: 'UTC', hour: 'numeric'}",
  "{timeZone: 'UTC', month: 'long', day: 'numeric'}", "{timeZone: 'UTC', timeStyle: 'short'}",
  "{timeZone: 'UTC', dateStyle: 'medium', timeStyle: 'medium'}", "{timeZone: 'UTC', fractionalSecondDigits: 3, second: 'numeric'}",
];
const zonedOptions = [
  "{}", "{dateStyle: 'short'}", "{timeStyle: 'short'}", "{dateStyle: 'long', timeStyle: 'long'}", "{dateStyle: 'full', timeStyle: 'full'}",
  "{hour: 'numeric'}", "{hour: 'numeric', minute: 'numeric', hour12: false}", "{timeZoneName: 'short'}", "{timeZoneName: 'long'}",
  "{month: 'long', day: 'numeric'}", "{year: 'numeric'}", "{timeZone: 'UTC'}", "{calendar: 'iso8601'}",
];

function toLocale(fixture, optionList, localeList = locales) {
  for (const locale of localeList) {
    for (const options of optionList) {
      add(`${fixtures[fixture]}.toLocaleString(${q(locale)}, ${options})`);
    }
  }
}
toLocale("PlainDate", dateOptions);
toLocale("PlainTime", timeOptions);
toLocale("PlainDateTime", dateTimeOptions);
toLocale("PlainYearMonth", yearMonthOptions);
toLocale("PlainMonthDay", monthDayOptions);
toLocale("Instant", instantOptions);
toLocale("ZonedSaoPaulo", zonedOptions);
toLocale("ZonedUTC", zonedOptions.slice(0, 9), ["en-US", "pt-BR"]);
toLocale("ZonedOffset", zonedOptions.slice(0, 9), ["en-US", "pt-BR"]);
for (const locale of locales) {
  add(`${fixtures.Duration}.toLocaleString(${q(locale)})`);
  add(`${fixtures.Duration}.toLocaleString(${q(locale)}, {style: 'short'})`);
  add(`${fixtures.Duration}.toLocaleString(${q(locale)}, {style: 'narrow'})`);
}
add(`${fixtures.Duration}.toLocaleString('en-US', {style: 'digital'})`);

// Intl.DateTimeFormat.format e formatToParts.
const formatCases = [
  ["PlainDate", "{}"], ["PlainDate", "{dateStyle: 'long'}"], ["PlainDate", "{year: 'numeric', month: 'long'}"],
  ["PlainTime", "{}"], ["PlainTime", "{timeStyle: 'short'}"], ["PlainTime", "{hour: 'numeric', minute: 'numeric'}"],
  ["PlainDateTime", "{}"], ["PlainDateTime", "{dateStyle: 'medium', timeStyle: 'short'}"],
  ["PlainDateTime", "{timeZone: 'America/Sao_Paulo'}"], ["PlainDateTime", "{timeZone: 'UTC', timeZoneName: 'short'}"],
  ["PlainYearMonth", "{calendar: 'iso8601'}"], ["PlainYearMonth", "{}"], ["PlainYearMonth", "{calendar: 'iso8601', dateStyle: 'long'}"],
  ["PlainMonthDay", "{calendar: 'iso8601'}"], ["PlainMonthDay", "{}"],
  ["Instant", "{timeZone: 'UTC'}"], ["Instant", "{timeZone: 'America/Sao_Paulo'}"], ["Instant", "{timeZone: 'America/Sao_Paulo', dateStyle: 'full', timeStyle: 'full'}"],
  ["Instant", "{timeZone: 'UTC', timeZoneName: 'short'}"],
  ["ZonedSaoPaulo", "{}"], ["ZonedUTC", "{}"],
  ["PlainDate", "{hour: 'numeric'}"], ["PlainTime", "{year: 'numeric'}"], ["PlainDate", "{timeStyle: 'short'}"], ["PlainTime", "{dateStyle: 'short'}"],
  ["PlainYearMonth", "{calendar: 'iso8601', day: 'numeric'}"], ["PlainMonthDay", "{calendar: 'iso8601', year: 'numeric'}"],
];
for (const locale of locales) {
  for (const [fixture, options] of formatCases) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, ${options})`;
    add(`${dtf}.format(${fixtures[fixture]})`);
    add(`${dtf}.formatToParts(${fixtures[fixture]}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
  }
}

// formatRange e formatRangeToParts.
const rangeCases = [
  ["PlainDate", "2024-01-05", "2024-01-07", "Temporal.PlainDate"], ["PlainDate", "2024-01-05", "2024-01-05", "Temporal.PlainDate"],
  ["PlainDate", "2024-01-05", "2024-03-07", "Temporal.PlainDate"], ["PlainDate", "2024-01-05", "2025-01-05", "Temporal.PlainDate"],
  ["PlainTime", "15:04:05", "17:30:00", "Temporal.PlainTime"], ["PlainTime", "15:04:05", "15:04:05", "Temporal.PlainTime"],
  ["PlainDateTime", "2024-01-05T15:04:05", "2024-01-05T17:30:00", "Temporal.PlainDateTime"],
  ["PlainDateTime", "2024-01-05T15:04:05", "2024-01-07T17:30:00", "Temporal.PlainDateTime"],
  ["Instant", "2024-01-05T15:04:05Z", "2024-01-05T17:30:00Z", "Temporal.Instant"],
  ["Instant", "2024-01-05T15:04:05Z", "2024-02-05T17:30:00Z", "Temporal.Instant"],
];
for (const locale of locales) {
  for (const [, a, b, ctor] of rangeCases) {
    const options = ctor === "Temporal.Instant" ? "{timeZone: 'America/Sao_Paulo'}" : "{}";
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, ${options})`;
    add(`${dtf}.formatRange(${ctor}.from(${q(a)}), ${ctor}.from(${q(b)}))`);
    add(`${dtf}.formatRangeToParts(${ctor}.from(${q(a)}), ${ctor}.from(${q(b)})).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
  }
}
// PlainYearMonth e PlainMonthDay (calendar iso8601) em mais locales: format, formatToParts, formatRange e
// formatRangeToParts, com intervalos no mesmo ano, em anos diferentes, no mesmo mês e iguais.
const extraLocales = ["fr", "es", "it", "ko", "zh", "ar"];
const yearMonthRanges = [["2024-01", "2024-03"], ["2024-01", "2025-03"], ["2024-01", "2024-01"]];
const monthDayRanges = [["01-05", "03-07"], ["01-05", "01-07"], ["01-05", "01-05"]];
for (const locale of extraLocales) {
  const dtf = `new Intl.DateTimeFormat(${q(locale)}, {calendar: 'iso8601'})`;
  const dtfLong = `new Intl.DateTimeFormat(${q(locale)}, {calendar: 'iso8601', year: 'numeric', month: 'long'})`;
  const dtfDay = `new Intl.DateTimeFormat(${q(locale)}, {calendar: 'iso8601', month: 'long', day: 'numeric'})`;
  for (const fixture of ["PlainYearMonth", "PlainMonthDay"]) {
    add(`${dtf}.format(${fixtures[fixture]})`);
    add(`${dtf}.formatToParts(${fixtures[fixture]}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
  }
  add(`${dtfLong}.formatToParts(${fixtures.PlainYearMonth}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
  add(`${dtfDay}.formatToParts(${fixtures.PlainMonthDay}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
  for (const [ctor, ranges] of [["Temporal.PlainYearMonth", yearMonthRanges], ["Temporal.PlainMonthDay", monthDayRanges]]) {
    for (const [a, b] of ranges) {
      add(`${dtf}.formatRange(${ctor}.from(${q(a)}), ${ctor}.from(${q(b)}))`);
      add(`${dtf}.formatRangeToParts(${ctor}.from(${q(a)}), ${ctor}.from(${q(b)})).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
  for (const month of ["short", "narrow"]) {
    const dtfText = `new Intl.DateTimeFormat(${q(locale)}, {calendar: 'iso8601', month: '${month}', day: 'numeric'})`;
    add(`${dtfText}.formatToParts(${fixtures.PlainMonthDay}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
    add(`${dtfText}.formatRange(Temporal.PlainMonthDay.from('01-05'), Temporal.PlainMonthDay.from('03-07'))`);
  }
  add(`${dtfLong}.formatRange(Temporal.PlainYearMonth.from('2024-01'), Temporal.PlainYearMonth.from('2024-03'))`);
  add(`${dtfDay}.formatRange(Temporal.PlainMonthDay.from('01-05'), Temporal.PlainMonthDay.from('03-07'))`);
}
add("new Intl.DateTimeFormat('en-US', {calendar: 'iso8601'}).formatRange(Temporal.PlainYearMonth.from('2024-01'), Temporal.PlainYearMonth.from('2024-03'))");
add("new Intl.DateTimeFormat('en-US', {calendar: 'iso8601'}).formatRange(Temporal.PlainMonthDay.from('01-05'), Temporal.PlainMonthDay.from('03-07'))");
add("new Intl.DateTimeFormat('en-US', {dateStyle: 'long'}).formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDate.from('2024-01-07'))");
add("new Intl.DateTimeFormat('en-US', {dateStyle: 'long'}).formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDate.from('2024-03-07'))");

// calendar 'iso8601' com opções de hora e era, em PlainDateTime, PlainDate e Date, e formatRange disso.
const isoLocales = ["en-US", "pt-BR", "de", "ja", "fr", "ko"];
const isoOptions = [
  "{hour: 'numeric'}", "{hour: 'numeric', minute: 'numeric'}", "{hour: 'numeric', minute: 'numeric', hour12: false}",
  "{hour: 'numeric', minute: 'numeric', hour12: true}", "{timeStyle: 'short'}", "{timeStyle: 'medium'}", "{dateStyle: 'short', timeStyle: 'short'}",
  "{dateStyle: 'long', timeStyle: 'short'}", "{year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: 'numeric'}",
  "{era: 'short', year: 'numeric'}", "{era: 'long', year: 'numeric', month: 'long', day: 'numeric'}", "{era: 'narrow'}", "{dateStyle: 'full'}",
];
for (const locale of isoLocales) {
  for (const options of isoOptions) {
    const full = options.replace("{", "{calendar: 'iso8601', ");
    add(`${fixtures.PlainDateTime}.toLocaleString(${q(locale)}, ${full})`);
    add(`new Date(Date.UTC(2024, 0, 5, 15, 4, 5)).toLocaleString(${q(locale)}, ${full.replace("{", "{timeZone: 'UTC', ")})`);
    add(`new Intl.DateTimeFormat(${q(locale)}, ${full}).formatToParts(${fixtures.PlainDateTime}).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
    const eraTime = `{timeZone: 'UTC', calendar: 'iso8601', ${options.slice(1)}`;
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, ${eraTime})`;
    add(`${dtf}.formatRange(new Date(Date.UTC(2024, 0, 5, 15, 4, 5)), new Date(Date.UTC(2024, 0, 5, 17, 30, 0)))`);
    add(`${dtf}.formatRange(new Date(Date.UTC(2024, 0, 5, 15, 4, 5)), new Date(Date.UTC(2024, 0, 7, 17, 30, 0)))`);
    add(`${dtf}.formatRangeToParts(new Date(Date.UTC(2024, 0, 5, 15, 4, 5)), new Date(Date.UTC(2024, 0, 7, 17, 30, 0))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    const dtfPlain = `new Intl.DateTimeFormat(${q(locale)}, ${full})`;
    add(`${dtfPlain}.formatRange(Temporal.PlainDateTime.from('2024-01-05T15:04:05'), Temporal.PlainDateTime.from('2024-01-05T17:30:00'))`);
    add(`${dtfPlain}.formatRange(Temporal.PlainDateTime.from('2024-01-05T15:04:05'), Temporal.PlainDateTime.from('2024-03-07T17:30:00'))`);
  }
  for (const options of ["{}", "{dateStyle: 'short'}", "{dateStyle: 'long'}", "{year: 'numeric', month: 'long', day: 'numeric'}", "{era: 'short'}", "{month: 'short', day: 'numeric'}"]) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, ${options.replace("{", "{timeZone: 'UTC', calendar: 'iso8601', ").replace(", }", "}")})`;
    for (const [a, b] of [["2024, 0, 5", "2024, 0, 7"], ["2024, 0, 5", "2024, 2, 7"], ["2024, 0, 5", "2025, 0, 5"], ["2024, 0, 5", "2024, 0, 5"]]) {
      add(`${dtf}.formatRange(new Date(Date.UTC(${a})), new Date(Date.UTC(${b})))`);
      add(`${dtf}.formatRangeToParts(new Date(Date.UTC(${a})), new Date(Date.UTC(${b}))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
    add(`${dtf}.formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDate.from('2024-03-07'))`);
  }
}

// calendar 'iso8601' com hora atravessando o meio-dia (AM/PM diferente no ciclo de 12 horas, 24 horas com 15:30 e 12:30).
for (const locale of ["en-US", "ko", "ja", "de"]) {
  for (const options of [
    "hour: 'numeric', minute: 'numeric'", "hour: 'numeric', minute: 'numeric', hour12: true", "hour: 'numeric', minute: 'numeric', hour12: false",
    "hour: 'numeric'", "timeStyle: 'short'", "dateStyle: 'short', timeStyle: 'short'",
    "year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: 'numeric', hour12: true", "hour: 'numeric', minute: 'numeric', hourCycle: 'h11'",
  ]) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, {timeZone: 'UTC', calendar: 'iso8601', ${options}})`;
    for (const [a, b] of [["9, 4, 0", "15, 30, 0"], ["11, 4, 0", "12, 30, 0"]]) {
      const args = `new Date(Date.UTC(2024, 0, 5, ${a})), new Date(Date.UTC(2024, 0, 5, ${b}))`;
      add(`${dtf}.formatRange(${args})`);
      add(`${dtf}.formatRangeToParts(${args}).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
}

// calendar 'iso8601' com mês por extenso (o ICU usa o padrão de intervalo da raiz, com o mês vazio): dateStyle, weekday, era.
for (const locale of ["en-US", "pt-BR", "ja"]) {
  for (const options of [
    "dateStyle: 'medium'", "dateStyle: 'long'", "dateStyle: 'full'", "month: 'short', day: 'numeric'", "month: 'long', year: 'numeric'",
    "weekday: 'short', month: 'short', day: 'numeric'", "era: 'long', year: 'numeric', month: 'long', day: 'numeric'",
    "era: 'short', month: 'long', year: 'numeric'", "era: 'short', month: 'short', day: 'numeric'", "year: 'numeric', month: 'narrow', day: 'numeric'",
  ]) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, {timeZone: 'UTC', calendar: 'iso8601', ${options}})`;
    for (const [a, b] of [["2024, 0, 5", "2024, 0, 7"], ["2024, 0, 5", "2024, 2, 7"], ["2024, 0, 5", "2025, 0, 5"]]) {
      add(`${dtf}.formatRange(new Date(Date.UTC(${a})), new Date(Date.UTC(${b})))`);
      add(`${dtf}.formatRangeToParts(new Date(Date.UTC(${a})), new Date(Date.UTC(${b}))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
}

// calendar 'iso8601' sem dia: só o ano (a era some do intervalo, `2024–2025`) e dia da semana com era, ano e mês estreito ou por extenso.
for (const locale of ["en-US", "pt-BR", "de", "ja", "fr", "ko"]) {
  for (const options of [
    "year: 'numeric'", "era: 'short', year: 'numeric'", "era: 'long', year: 'numeric'", "era: 'narrow', year: 'numeric'",
    "weekday: 'short', month: 'narrow'", "weekday: 'short', month: 'short'", "weekday: 'short', month: 'long'",
    "weekday: 'short', era: 'short', month: 'narrow'", "weekday: 'short', era: 'long', month: 'long'", "weekday: 'short', era: 'narrow', month: 'short'",
    "weekday: 'long', year: 'numeric'", "weekday: 'short', era: 'short', year: 'numeric'",
    "weekday: 'short', month: 'long', year: 'numeric'", "weekday: 'short', month: 'narrow', year: 'numeric'",
    "weekday: 'short', era: 'short', month: 'long', year: 'numeric'", "weekday: 'short', era: 'short', month: 'narrow', year: 'numeric'",
    "weekday: 'short', era: 'short', month: 'narrow', day: 'numeric'", "weekday: 'short', era: 'long', month: 'long', day: 'numeric'",
  ]) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, {timeZone: 'UTC', calendar: 'iso8601', ${options}})`;
    for (const [a, b] of [["2024, 0, 5", "2024, 0, 7"], ["2024, 0, 5", "2024, 2, 7"], ["2024, 0, 5", "2025, 0, 5"]]) {
      add(`${dtf}.formatRange(new Date(Date.UTC(${a})), new Date(Date.UTC(${b})))`);
      add(`${dtf}.formatRangeToParts(new Date(Date.UTC(${a})), new Date(Date.UTC(${b}))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
}

// calendar 'iso8601' com era e um único campo de data (`month` ou `day`): o ICU acrescenta o campo com o rótulo do locale
// (`" (month: 1)"`, `" (dia: 5)"`) conforme a largura da era e a do campo; e era com hora, em que o dia que difere leva o
// rótulo (`" (day: 5), 1 AM"`). Todas as larguras de era e de mês, em format, formatToParts, formatRange e formatRangeToParts
// (mesmo mês, meses diferentes, anos diferentes).
for (const locale of ["en-US", "pt-BR", "de", "ja", "fr", "ko", "el", "ru", "ar"]) {
  const fieldOptions = [];
  for (const era of ["narrow", "short", "long"]) {
    for (const month of ["numeric", "2-digit", "narrow", "short", "long"]) fieldOptions.push(`era: '${era}', month: '${month}'`);
    for (const day of ["numeric", "2-digit"]) fieldOptions.push(`era: '${era}', day: '${day}'`);
  }
  for (const options of fieldOptions) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, {timeZone: 'UTC', calendar: 'iso8601', ${options}})`;
    add(`${dtf}.format(new Date(Date.UTC(2024, 0, 5)))`);
    add(`${dtf}.formatToParts(new Date(Date.UTC(2024, 5, 25))).map(function (p) { return p.type + '=' + p.value; }).join('|')`);
    for (const [a, b] of [["2024, 0, 5", "2024, 0, 25"], ["2024, 0, 5", "2024, 5, 25"], ["2024, 0, 5", "2026, 5, 25"]]) {
      add(`${dtf}.formatRange(new Date(Date.UTC(${a})), new Date(Date.UTC(${b})))`);
      add(`${dtf}.formatRangeToParts(new Date(Date.UTC(${a})), new Date(Date.UTC(${b}))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
  for (const options of ["era: 'narrow', hour: 'numeric'", "era: 'short', hour: 'numeric'", "era: 'long', hour: 'numeric'", "era: 'short', hour: 'numeric', hour12: false"]) {
    const dtf = `new Intl.DateTimeFormat(${q(locale)}, {timeZone: 'UTC', calendar: 'iso8601', ${options}})`;
    for (const [a, b] of [["2024, 0, 5, 1", "2024, 0, 5, 5"], ["2024, 0, 5, 1", "2024, 0, 7, 5"], ["2024, 0, 5, 1", "2024, 5, 7, 5"], ["2024, 0, 5, 1", "2026, 5, 7, 5"]]) {
      add(`${dtf}.formatRange(new Date(Date.UTC(${a})), new Date(Date.UTC(${b})))`);
      add(`${dtf}.formatRangeToParts(new Date(Date.UTC(${a})), new Date(Date.UTC(${b}))).map(function (p) { return p.type + '=' + p.value + '@' + p.source; }).join('|')`);
    }
  }
}

// resolvedOptions: por formatador, não depende do objeto Temporal formatado.
for (const locale of locales) {
  for (const options of ["{}", "{dateStyle: 'short'}", "{timeStyle: 'short'}", "{hour: 'numeric', hour12: false}", "{timeZone: 'America/Sao_Paulo', timeZoneName: 'short'}", "{calendar: 'iso8601', year: 'numeric'}"]) {
    add(`(function (f) { f.format(Temporal.PlainDate.from('2024-01-05').withCalendar('iso8601')); return f.resolvedOptions(); })(new Intl.DateTimeFormat(${q(locale)}, ${options}))`);
  }
}

// Erros.
const errorPrograms = [
  "new Intl.DateTimeFormat('en-US', {hour: 'numeric'}).format(Temporal.PlainDate.from('2024-01-05'))",
  "new Intl.DateTimeFormat('en-US', {year: 'numeric'}).format(Temporal.PlainTime.from('15:04:05'))",
  "new Intl.DateTimeFormat('en-US').format(Temporal.PlainYearMonth.from('2024-01'))",
  "new Intl.DateTimeFormat('en-US').format(Temporal.PlainMonthDay.from('01-05'))",
  "new Intl.DateTimeFormat('en-US', {calendar: 'gregory'}).format(Temporal.PlainYearMonth.from('2024-01'))",
  "new Intl.DateTimeFormat('en-US').format(Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]'))",
  "new Intl.DateTimeFormat('en-US').formatToParts(Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]'))",
  "new Intl.DateTimeFormat('en-US').formatRange(Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]'), Temporal.ZonedDateTime.from('2024-01-06T15:04:05+00:00[UTC]'))",
  "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainTime.from('15:04:05'))",
  "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), 0)",
  "new Intl.DateTimeFormat('en-US').formatRange(0, Temporal.PlainDate.from('2024-01-05'))",
  "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDateTime.from('2024-01-05T00:00'))",
  "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'))",
  "new Intl.DateTimeFormat('en-US').formatRange()",
  "new Intl.DateTimeFormat('en-US').formatRange(NaN, 0)",
  "new Intl.DateTimeFormat('en-US').format(Temporal.Duration.from('PT1S'))",
  "Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]').toLocaleString('en-US', {timeZone: 'UTC'})",
  "Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]').toLocaleString('en-US', {timeZone: undefined})",
  "Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]').toLocaleString('xx-invalid-locale-')",
  "Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {timeStyle: 'short'})",
  "Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {hour: 'numeric'})",
  "Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {dateStyle: 'short', year: 'numeric'})",
  "Temporal.PlainTime.from('15:04:05').toLocaleString('en-US', {dateStyle: 'short'})",
  "Temporal.PlainTime.from('15:04:05').toLocaleString('en-US', {timeStyle: 'short', hour: 'numeric'})",
  "Temporal.PlainTime.from('15:04:05').toLocaleString('en-US', {timeZone: 'UTC'})",
  "Temporal.PlainTime.from('15:04:05').toLocaleString('en-US', {timeZoneName: 'short'})",
  "Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {timeZone: 'UTC'})",
  "Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {timeZone: 'Nope/Zone'})",
  "Temporal.Instant.from('2024-01-05T15:04:05Z').toLocaleString('en-US', {timeZone: 'Nope/Zone'})",
  "Temporal.Instant.from('2024-01-05T15:04:05Z').toLocaleString('en-US', {dateStyle: 'bogus'})",
  "Temporal.PlainYearMonth.from('2024-01').toLocaleString('en-US')",
  "Temporal.PlainYearMonth.from('2024-01').toLocaleString('en-US', {calendar: 'gregory'})",
  "Temporal.PlainMonthDay.from('01-05').toLocaleString('en-US')",
  "Temporal.PlainYearMonth.from('2024-01').toLocaleString('en-US', {calendar: 'iso8601', hour: 'numeric'})",
  "Temporal.PlainTime.prototype.toLocaleString.call({})",
  "Temporal.PlainDate.prototype.toLocaleString.call(Temporal.PlainTime.from('15:04'))",
  "Temporal.PlainDateTime.prototype.toLocaleString.call(1)",
  "Temporal.PlainYearMonth.prototype.toLocaleString.call({})",
  "Temporal.PlainMonthDay.prototype.toLocaleString.call({})",
  "Temporal.Instant.prototype.toLocaleString.call({})",
  "Temporal.ZonedDateTime.prototype.toLocaleString.call({})",
  "Temporal.Duration.prototype.toLocaleString.call({})",
  "new Date(NaN).toTemporalInstant()",
  "Date.prototype.toTemporalInstant.call({})",
  "Date.prototype.toTemporalInstant.call(1)",
];
for (const source of errorPrograms) add(source);

// Date.prototype.toTemporalInstant.
for (const source of [
  "new Date(Date.UTC(2024, 0, 5, 15, 4, 5)).toTemporalInstant().toString()",
  "new Date(Date.UTC(2024, 0, 5, 15, 4, 5, 123)).toTemporalInstant().toString()",
  "new Date(0).toTemporalInstant().toString()",
  "new Date(-1).toTemporalInstant().toString()",
  "new Date(8.64e15).toTemporalInstant().toString()",
  "new Date(-8.64e15).toTemporalInstant().toString()",
  "new Date(Date.UTC(2024, 0, 5, 15, 4, 5)).toTemporalInstant().epochMilliseconds",
  "Date.prototype.toTemporalInstant.name + '/' + Date.prototype.toTemporalInstant.length",
  "Object.getOwnPropertyDescriptor(Date.prototype, 'toTemporalInstant').enumerable",
  "Object.getOwnPropertyDescriptor(Date.prototype, 'toTemporalInstant').writable",
  "Object.getOwnPropertyDescriptor(Date.prototype, 'toTemporalInstant').configurable",
  "new Date(Date.UTC(2024, 0, 5, 15, 4, 5)).toTemporalInstant().toLocaleString('en-US', {timeZone: 'UTC'})",
]) {
  add(source);
}
// Propriedades dos toLocaleString.
for (const name of ["PlainDate", "PlainTime", "PlainDateTime", "PlainYearMonth", "PlainMonthDay", "Instant", "ZonedDateTime", "Duration"]) {
  add(`Temporal.${name}.prototype.toLocaleString.name + '/' + Temporal.${name}.prototype.toLocaleString.length`);
}

// ---------------------------------------------------------------------------------------------
const seen = new Set();
for (const src of programs) {
  if (!src || seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  emitRow(`${src}\t${evaluate(src)}`);
}
