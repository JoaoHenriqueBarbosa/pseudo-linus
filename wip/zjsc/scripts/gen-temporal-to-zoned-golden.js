// Gera tests/golden/temporal_to_zoned_bun.tsv: programas de uma linha sobre
// Temporal.PlainDate.prototype.toZonedDateTime e Temporal.PlainDateTime.prototype.toZonedDateTime avaliados no bun.
// Colunas: fonte do programa e o resultado (`ok:<JSON>` ou `throw:<Nome>: <mensagem>`), com o que passa de ASCII
// escapado como \uXXXX. O avaliador é tests/golden/temporal_locale_harness.js, o MESMO texto que
// tests/temporal_to_zoned_bun_golden.rs embute. Nenhum programa depende do fuso da máquina.
// Uso: TZ=UTC bun scripts/gen-temporal-to-zoned-golden.js > tests/golden/temporal_to_zoned_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const path = require("path");

process.env.TZ = "UTC";
const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/temporal_locale_harness.js"), "utf8").trim();
const evaluate = (0, eval)(harness);

const programs = [];
const add = (source) => programs.push(source);

const DATE = "Temporal.PlainDate.from('2024-03-10')";
const PDT = (text) => `Temporal.PlainDateTime.from('${text}')`;
const show = (expr) => `(${expr}).toString()`;

// Fusos simples: início do dia (PlainDate) e a hora dada (PlainDateTime).
for (const zone of ["UTC", "America/New_York", "Asia/Kolkata", "Europe/London", "+05:30", "-08:00", "+00:00", "America/Sao_Paulo"]) {
  add(show(`Temporal.PlainDate.from('2024-01-05').toZonedDateTime('${zone}')`));
  add(show(`${PDT("2024-01-05T15:04:05.123456789")}.toZonedDateTime('${zone}')`));
}

// PlainDate com objeto { timeZone, plainTime }.
add(show(`${DATE}.toZonedDateTime({timeZone: 'America/New_York'})`));
add(show(`${DATE}.toZonedDateTime({timeZone: 'America/New_York', plainTime: '12:30'})`));
add(show(`${DATE}.toZonedDateTime({timeZone: 'Asia/Kolkata', plainTime: Temporal.PlainTime.from('23:59:59.999')})`));
add(show(`${DATE}.toZonedDateTime({timeZone: 'UTC', plainTime: undefined})`));
add(show(`${DATE}.toZonedDateTime(Temporal.ZonedDateTime.from('2020-01-01T00:00:00+01:00[Europe/Paris]'))`));
// Lacuna do horário de verão em New_York: 2024-03-10 02:30 não existe. O dia começa à meia-noite.
add(show(`${DATE}.toZonedDateTime({timeZone: 'America/New_York', plainTime: '02:30'})`));
// Início do dia onde a meia-noite não existe (São Paulo em 2018-11-04 pulou 00:00 para 01:00).
add(show("Temporal.PlainDate.from('2018-11-04').toZonedDateTime('America/Sao_Paulo')"));
add(show("Temporal.PlainDate.from('2018-11-04').toZonedDateTime({timeZone: 'America/Sao_Paulo', plainTime: '00:30'})"));

// Disambiguation em lacuna (2024-03-10T02:30 em New_York) e sobreposição (2024-11-03T01:30).
for (const mode of ["compatible", "earlier", "later", "reject"]) {
  add(show(`${PDT("2024-03-10T02:30")}.toZonedDateTime('America/New_York', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-11-03T01:30")}.toZonedDateTime('America/New_York', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-03-31T01:30")}.toZonedDateTime('Europe/London', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-10-27T01:30")}.toZonedDateTime('Europe/London', {disambiguation: '${mode}'})`));
}
add(show(`${PDT("2024-03-10T02:30")}.toZonedDateTime('America/New_York')`));
add(show(`${PDT("2024-11-03T01:30")}.toZonedDateTime('America/New_York', {})`));
add(show(`${PDT("2024-03-10T02:30")}.toZonedDateTime('America/New_York', {disambiguation: undefined})`));
add(show(`${PDT("2024-03-10T02:30")}.toZonedDateTime('+01:00', {disambiguation: 'reject'})`));
add(show(`${PDT("2024-03-10T02:30:00.5")}.toZonedDateTime('America/New_York', {disambiguation: 'later'})`));

// Propriedades do resultado.
add(`${PDT("2024-11-03T01:30")}.toZonedDateTime('America/New_York', {disambiguation: 'later'}).offset`);
add(`${PDT("2024-11-03T01:30")}.toZonedDateTime('America/New_York', {disambiguation: 'earlier'}).epochMilliseconds`);
add(`${PDT("2024-01-05T15:04")}.toZonedDateTime('Asia/Kolkata').timeZoneId`);
add(`${DATE}.toZonedDateTime('Europe/London').hoursInDay`);
add(`Temporal.PlainDate.from('2024-03-10').toZonedDateTime('America/New_York').hoursInDay`);
add(`${PDT("2024-01-05T15:04")}.withCalendar('gregory').toZonedDateTime('UTC').calendarId`);
add(`Temporal.PlainDate.from('2024-01-05').withCalendar('japanese').toZonedDateTime('UTC').calendarId`);

// Argumentos inválidos: fuso.
for (const call of ["toZonedDateTime()", "toZonedDateTime(undefined)", "toZonedDateTime(null)", "toZonedDateTime(1)", "toZonedDateTime(true)",
  "toZonedDateTime('')", "toZonedDateTime('Nope/Zone')", "toZonedDateTime('+25:00')", "toZonedDateTime('2024-01-05')",
  "toZonedDateTime('2024-01-05T00:00:00Z')", "toZonedDateTime(Symbol('x'))", "toZonedDateTime({})", "toZonedDateTime({timeZone: 'Nope/Zone'})",
  "toZonedDateTime({timeZone: 1})", "toZonedDateTime({timeZone: null})", "toZonedDateTime({timeZone: 'UTC', plainTime: 'bogus'})",
  "toZonedDateTime({timeZone: 'UTC', plainTime: null})", "toZonedDateTime({timeZone: 'UTC', plainTime: 5})"]) {
  add(`Temporal.PlainDate.from('2024-01-05').${call}`);
  add(`${PDT("2024-01-05T15:04")}.${call}`);
}

// Argumentos inválidos: options e disambiguation.
for (const options of ["null", "5", "'reject'", "true", "{disambiguation: 'bogus'}", "{disambiguation: ''}", "{disambiguation: 'EARLIER'}",
  "{disambiguation: null}", "{disambiguation: 5}", "{disambiguation: {}}"]) {
  add(`${PDT("2024-01-05T15:04")}.toZonedDateTime('UTC', ${options})`);
}
add(`${PDT("2024-01-05T15:04")}.toZonedDateTime('UTC', function () {})`);
add(`${PDT("2024-01-05T15:04")}.toZonedDateTime('Nope/Zone', {disambiguation: 'bogus'})`);
add(`${PDT("2024-01-05T15:04")}.toZonedDateTime('UTC', {disambiguation: 'bogus'})`);

// Limites do intervalo: 275760-09-13T00:00 UTC é o último instante.
add(show("Temporal.PlainDateTime.from('+275760-09-13T00:00:00').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDateTime.from('+275760-09-13T00:00:00.000000001').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDateTime.from('-271821-04-20T00:00:00').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDateTime.from('-271821-04-19T23:59:59.999999999').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDateTime.from('+275760-09-13T00:00:00').toZonedDateTime('-01:00')"));
add(show("Temporal.PlainDate.from('+275760-09-13').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDate.from('+275760-09-13').toZonedDateTime('+01:00')"));
add(show("Temporal.PlainDate.from('-271821-04-19').toZonedDateTime('UTC')"));
add(show("Temporal.PlainDate.from('-271821-04-20').toZonedDateTime('UTC')"));

// Início do dia fora da faixa (meia-noite local passa do máximo), plainTime como objeto e identificadores de fuso.
add(show("Temporal.PlainDate.from('+275760-09-13').toZonedDateTime('America/Vancouver')"));
add(show("Temporal.PlainDate.from('+275760-09-12').toZonedDateTime('America/Vancouver')"));
add(show("Temporal.PlainDate.from('2024-01-05').toZonedDateTime({timeZone: 'UTC', plainTime: {hour: 25}})"));
add(show("Temporal.PlainDate.from('2024-01-05').toZonedDateTime({timeZone: 'UTC', plainTime: {}})"));
add(show("Temporal.PlainDate.from('2024-01-05').toZonedDateTime({timeZone: 'UTC', plainTime: {hour: 3}})"));
add(show("Temporal.PlainDate.from('2024-01-05').toZonedDateTime({get timeZone() { throw new Error('x'); }})"));
add(show("Temporal.PlainDate.from('2024-01-05').toZonedDateTime(Object.assign(Object.create(null), {timeZone: 'UTC'}))"));
add("Temporal.PlainDate.from('2024-01-05').toZonedDateTime('Asia/Calcutta').timeZoneId");
add("Temporal.PlainDate.from('2024-01-05').toZonedDateTime('europe/london').timeZoneId");
add("Temporal.PlainDate.from('2024-01-05').toZonedDateTime('+0530').timeZoneId");
add("Temporal.PlainDate.from('2024-01-05').toZonedDateTime('+05:30:15').timeZoneId");
add("Temporal.PlainDate.from('2024-01-05').toZonedDateTime('2024-01-05T10:00+05:30[UTC]').timeZoneId");

// Cadeias de fuso: ParseTemporalTimeZoneString (identificador, deslocamento, cadeia com Z, deslocamento ou anotação).
for (const zone of ["2024-01-05T00:00:00Z", "2024-01-05T00:00+05:30[UTC]", "-00:00", "-0000", "+00", "+05", "UTC", "utc", "etc/utc", "uct",
  "gmt", "Etc/GMT+5", "etc/gmt+5", "ASIA/CALCUTTA", "us/pacific", "japan", "Z", "z", "+24:00", "+0", "+0530:15", "+05:30:00", "+053015",
  "2024-01-05T00:00-03:00", "2024-01-05T00:00[Europe/Paris]", "2024-01-05T00:00[+01:00]", "2024-01-05T00:00[-00:00]", "2024-01-05T00:00[!UTC]",
  "2024-01-05T00:00+01:00[UTC]", "2024-01-05T00:00Z[Europe/Paris]", "2024-01-05T00:00:00+05:30:15", "2024-01-05T00:00:00+05:30:15[UTC]",
  "2024-01-05T00:00[u-ca=iso8601]", "2024-01-05T00:00Z[u-ca=iso8601]", "[Europe/Paris]", "10:00[UTC]", "10:00", "10:00Z", "T10:00Z",
  "--01-05", "2024-01", "12-31", "UTC+1", "UTC/", "a//b", "Europe/Paris/", "Z[UTC]", "−05:00"]) {
  add(`Temporal.PlainDate.from('2024-01-05').toZonedDateTime(${JSON.stringify(zone).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"))}).timeZoneId`);
}

// Lacuna de 30 minutos (Australia/Lord_Howe: 2024-10-06 02:00 vira 02:30) e sobreposição de 30 minutos
// (2024-04-07 02:00 volta para 01:30).
for (const mode of ["compatible", "earlier", "later", "reject"]) {
  add(show(`${PDT("2024-10-06T02:15")}.toZonedDateTime('Australia/Lord_Howe', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-10-06T02:00")}.toZonedDateTime('Australia/Lord_Howe', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-10-06T02:29:59.999999999")}.toZonedDateTime('Australia/Lord_Howe', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-04-07T01:45")}.toZonedDateTime('Australia/Lord_Howe', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2024-04-07T01:30")}.toZonedDateTime('Australia/Lord_Howe', {disambiguation: '${mode}'})`));
  // Dia inteiro que não existe: Pacific/Apia pulou 2011-12-30 (UTC-10 para UTC+14).
  add(show(`${PDT("2011-12-30T00:00")}.toZonedDateTime('Pacific/Apia', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2011-12-30T12:00")}.toZonedDateTime('Pacific/Apia', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2011-12-30T23:59:59.999999999")}.toZonedDateTime('Pacific/Apia', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2011-12-29T23:59:59.999999999")}.toZonedDateTime('Pacific/Apia', {disambiguation: '${mode}'})`));
  add(show(`${PDT("2011-12-31T00:00")}.toZonedDateTime('Pacific/Apia', {disambiguation: '${mode}'})`));
}
add(show("Temporal.PlainDate.from('2024-10-06').toZonedDateTime('Australia/Lord_Howe')"));
add(show("Temporal.PlainDate.from('2024-10-06').toZonedDateTime({timeZone: 'Australia/Lord_Howe', plainTime: '02:15'})"));
add(show("Temporal.PlainDate.from('2024-04-07').toZonedDateTime({timeZone: 'Australia/Lord_Howe', plainTime: '01:45'})"));
add(show("Temporal.PlainDate.from('2011-12-30').toZonedDateTime('Pacific/Apia')"));
add(show("Temporal.PlainDate.from('2011-12-30').toZonedDateTime({timeZone: 'Pacific/Apia', plainTime: '12:00'})"));
add(show("Temporal.PlainDate.from('2011-12-29').toZonedDateTime('Pacific/Apia')"));
add(show("Temporal.PlainDate.from('2011-12-31').toZonedDateTime('Pacific/Apia')"));
add("Temporal.PlainDate.from('2011-12-30').toZonedDateTime('Pacific/Apia').hoursInDay");
add("Temporal.PlainDate.from('2011-12-29').toZonedDateTime('Pacific/Apia').hoursInDay");
add("Temporal.PlainDate.from('2024-10-06').toZonedDateTime('Australia/Lord_Howe').hoursInDay");
add("Temporal.PlainDate.from('2024-04-07').toZonedDateTime('Australia/Lord_Howe').hoursInDay");

// Receptor errado e propriedades das funções.
add("Temporal.PlainDate.prototype.toZonedDateTime.call({}, 'UTC')");
add("Temporal.PlainDate.prototype.toZonedDateTime.call(Temporal.PlainDateTime.from('2024-01-05T00:00'), 'UTC')");
add("Temporal.PlainDateTime.prototype.toZonedDateTime.call(Temporal.PlainDate.from('2024-01-05'), 'UTC')");
add("Temporal.PlainDateTime.prototype.toZonedDateTime.call(1, 'UTC')");
add("Temporal.PlainDate.prototype.toZonedDateTime.name + '/' + Temporal.PlainDate.prototype.toZonedDateTime.length");
add("Temporal.PlainDateTime.prototype.toZonedDateTime.name + '/' + Temporal.PlainDateTime.prototype.toZonedDateTime.length");

// ---------------------------------------------------------------------------------------------
const seen = new Set();
for (const src of programs) {
  if (!src || seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  emitRow(`${src}\t${evaluate(src)}`);
}
