const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/temporal_calendar_format_bun.tsv: formatação de objetos Temporal contra formatadores de
// calendário igual ou diferente (validateCalendar do handleDateTimeValue). Rodar no bun com
// TZ=America/Sao_Paulo: `TZ=America/Sao_Paulo bun scripts/gen-temporal-calendar-format-golden.js > tests/golden/temporal_calendar_format_bun.tsv`.
// Cada linha: JSON(programa) TAB JSON(texto de R). O programa grava em `R` o resultado ou `Nome: mensagem`.
const programs = [];

function add(body) {
  programs.push(`var R; try { R = String(${body}); } catch (e) { R = e.name + ": " + e.message; }`);
}

const iso = "2024-03-15";
// Casos pedidos.
add(`new Intl.DateTimeFormat("en").format(Temporal.PlainDate.from("${iso}").withCalendar("japanese"))`);
add(`new Intl.DateTimeFormat("en-u-ca-gregory").format(Temporal.PlainDate.from("${iso}").withCalendar("japanese"))`);
add(`new Intl.DateTimeFormat("en-u-ca-japanese").format(Temporal.PlainDate.from("${iso}"))`);
add(`new Intl.DateTimeFormat("en-u-ca-japanese").format(Temporal.PlainDate.from("${iso}").withCalendar("gregory"))`);
add(`new Intl.DateTimeFormat("en-u-ca-iso8601").format(Temporal.PlainDate.from("${iso}").withCalendar("japanese"))`);
add(`new Intl.DateTimeFormat("en-u-ca-japanese").format(Temporal.PlainDate.from("${iso}").withCalendar("japanese"))`);
add(`new Intl.DateTimeFormat("en-u-ca-japanese", {timeZone: "UTC"}).format(Temporal.PlainDate.from("2024-01-01").withCalendar("japanese"))`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toLocaleString("en")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toLocaleString("en-u-ca-japanese")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toLocaleString("en-u-ca-gregory")`);
add(`Temporal.PlainDate.from("${iso}").toLocaleString("en-u-ca-japanese")`);
add(`Temporal.PlainDate.from("${iso}").toLocaleString("en-u-ca-iso8601")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("gregory").toLocaleString("en")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("gregory").toLocaleString("en-u-ca-iso8601")`);

// PlainYearMonth e PlainMonthDay.
for (const cal of ["japanese", "iso8601", "gregory", "buddhist"]) {
  for (const fmt of ["en", "en-u-ca-japanese", "en-u-ca-iso8601", "en-u-ca-gregory", "en-u-ca-buddhist"]) {
    add(`new Intl.DateTimeFormat("${fmt}").format(Temporal.PlainDate.from("${iso}").withCalendar("${cal}").toPlainYearMonth())`);
    add(`new Intl.DateTimeFormat("${fmt}").format(Temporal.PlainDate.from("${iso}").withCalendar("${cal}").toPlainMonthDay())`);
    add(`new Intl.DateTimeFormat("${fmt}").format(Temporal.PlainDateTime.from("${iso}T10:20:30").withCalendar("${cal}"))`);
    add(`new Intl.DateTimeFormat("${fmt}").format(Temporal.PlainDate.from("${iso}").withCalendar("${cal}"))`);
  }
}
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toPlainYearMonth().toLocaleString("en-u-ca-japanese")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toPlainMonthDay().toLocaleString("en-u-ca-japanese")`);
add(`Temporal.PlainDate.from("${iso}").withCalendar("japanese").toPlainMonthDay().toLocaleString("en")`);
add(`Temporal.PlainDateTime.from("${iso}T10:20:30").toLocaleString("en-u-ca-iso8601")`);
add(`Temporal.PlainDateTime.from("${iso}T10:20:30").withCalendar("iso8601").toLocaleString("en")`);
add(`Temporal.PlainDateTime.from("${iso}T10:20:30").withCalendar("japanese").toLocaleString("en")`);
add(`Temporal.PlainDateTime.from("${iso}T10:20:30").withCalendar("japanese").toLocaleString("en-u-ca-japanese")`);

// Outros calendários, formatador do mesmo calendário e de outro.
for (const cal of ["buddhist", "hebrew", "islamic-civil", "chinese", "roc", "persian", "islamic-tbla", "islamic-umalqura", "coptic", "ethiopic", "ethioaa", "indian", "dangi"]) {
  const d = `Temporal.PlainDate.from("${iso}").withCalendar("${cal}")`;
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").format(${d})`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").formatToParts(${d}).map(function (p) { return p.type + "=" + p.value; }).join("|")`);
  add(`${d}.toLocaleString("en-u-ca-${cal}")`);
  add(`${d}.toLocaleString("en")`);
  add(`new Intl.DateTimeFormat("en-u-ca-japanese").format(${d})`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").format(Temporal.PlainDate.from("${iso}"))`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").format(${d}.toPlainYearMonth())`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").format(${d}.toPlainMonthDay())`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").format(Temporal.PlainDateTime.from("${iso}T10:20:30").withCalendar("${cal}"))`);
  add(`new Intl.DateTimeFormat("en-u-ca-${cal}").resolvedOptions().calendar`);
  add(`${d}.calendarId`);
}
// Alias: "islamic" vira islamic-tbla no formatador; "gregory" nomeado.
add(`new Intl.DateTimeFormat("en-u-ca-islamic").format(Temporal.PlainDate.from("${iso}").withCalendar("islamic-tbla"))`);
add(`new Intl.DateTimeFormat("en-u-ca-islamic").format(Temporal.PlainDate.from("${iso}").withCalendar("islamic-civil"))`);
add(`new Intl.DateTimeFormat("en-u-ca-islamic-civil").format(Temporal.PlainDate.from("${iso}").withCalendar("islamic-tbla"))`);
add(`new Intl.DateTimeFormat("en", {calendar: "japanese"}).format(Temporal.PlainDate.from("${iso}").withCalendar("japanese"))`);

for (const program of programs) {
  const R = (() => { var R; (0, eval)(program + "; globalThis.__R = R;"); return globalThis.__R; })();
  emitRow(JSON.stringify(program) + "\t" + JSON.stringify(R));
}
