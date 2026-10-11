// Gera tests/golden/timezone_bun.tsv: fusos IANA em Intl.DateTimeFormat, Date e Temporal.ZonedDateTime, medidos no
// bun 1.4.2. Colunas: fuso do ambiente (TZ), fonte do programa (literal JSON, ASCII), texto de `R` (literal JSON, ASCII).
// Programas sem dependência do fuso local rodam só em UTC; os de Date local rodam em UTC e America/Sao_Paulo.
// Não repete date_tz_bun.tsv, temporal_zoned_bun.tsv e timezone_names_bun.tsv: aqui entram os dez fusos de borda,
// ambiguidade e lacuna com as quatro disambiguation, fusos históricos, offset ao segundo, aliases e timeZoneName
// (seis estilos) em dez locales.
// Uso: bun scripts/gen-timezone-golden.js > tests/golden/timezone_bun.tsv
const { spawnSync } = require("child_process");

const programs = []; // { src, local }
const P = (expr, local = false) =>
  programs.push({ src: `var R; try { R = String(${expr}); } catch (e) { R = 'E:' + e.name; }`, local });
const q = (s) => JSON.stringify(s);

const ZONES = [
  "America/Sao_Paulo", "America/New_York", "Europe/London", "Asia/Kolkata", "Asia/Kathmandu",
  "Australia/Lord_Howe", "Pacific/Chatham", "Africa/Casablanca", "Europe/Dublin", "Europe/Moscow",
];
const LOCALES = ["en-US", "pt-BR", "de", "fr", "ja", "ar", "hi", "ru", "zh-Hans", "es"];
const STYLES = ["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"];

// Instantes: 1900, 1970, 2018 (fim do DST no Brasil), 2019, 2024 verão e inverno, em torno das transições de 2024.
const INSTANTS = [
  -2208988800000, 0, 1541289600000, 1552204800000, 1704067200000, 1710054000000, 1710054000001 + 3600000, 1711846800000,
  1720000000000, 1730000000000, 1730592000000,
];

// A) Formatação em dez fusos x instantes (hora completa com offset).
for (const zone of ZONES) {
  for (const t of INSTANTS) {
    P(`new Intl.DateTimeFormat('en-US', { timeZone: ${q(zone)}, dateStyle: 'full', timeStyle: 'long', hourCycle: 'h23' }).format(${t})`);
    P(`new Intl.DateTimeFormat('en-GB', { timeZone: ${q(zone)}, year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23', timeZoneName: 'longOffset' }).format(${t})`);
  }
}

// B) timeZoneName: seis estilos x dez locales x três fusos x instantes de inverno e verão.
for (const style of STYLES) {
  for (const locale of LOCALES) {
    for (const [zone, instant] of [["America/New_York", 1705320000000], ["America/New_York", 1721044800000], ["Asia/Kolkata", 1721044800000]]) {
      P(`new Intl.DateTimeFormat(${q(locale)}, { timeZone: ${q(zone)}, timeZoneName: ${q(style)}, hour: 'numeric' }).format(${instant})`);
    }
  }
}

// C) resolvedOptions().timeZone para aliases.
for (const alias of ["Asia/Calcutta", "US/Pacific", "UTC", "Etc/GMT+3", "GMT", "Z", "Etc/UTC", "utc", "america/sao_paulo", "Asia/Kathmandu", "Asia/Katmandu", "Etc/GMT-14", "Europe/Kiev", "Asia/Saigon", "+03:00", "-0330", "Foo/Bar", "Etc/GMT+15"]) {
  P(`new Intl.DateTimeFormat('en', { timeZone: ${q(alias)} }).resolvedOptions().timeZone`);
}
P(`new Intl.DateTimeFormat('en').resolvedOptions().timeZone`, true);

// D) Temporal.ZonedDateTime: lacunas, repetições e as quatro disambiguation.
const DISAMBIG = ["compatible", "earlier", "later", "reject"];
const SPOTS = [
  ["America/New_York", "2024-03-10T02:30:00", "2024-11-03T01:30:00"],
  ["America/Sao_Paulo", "2018-11-04T00:30:00", "2018-02-17T23:30:00"],
  ["Europe/London", "2024-03-31T01:30:00", "2024-10-27T01:30:00"],
  ["Europe/Dublin", "2024-03-31T01:30:00", "2024-10-27T01:30:00"],
  ["Australia/Lord_Howe", "2024-10-06T02:15:00", "2024-04-07T01:45:00"],
  ["Pacific/Chatham", "2024-09-29T02:45:00", "2024-04-07T03:45:00"],
  ["Africa/Casablanca", "2024-03-10T02:30:00", "2024-04-14T02:30:00"],
  ["Europe/Moscow", "2011-03-27T02:30:00", "2014-10-26T01:30:00"],
];
for (const [zone, gap, overlap] of SPOTS) {
  for (const d of DISAMBIG) {
    for (const wall of [gap, overlap]) {
      P(`Temporal.PlainDateTime.from(${q(wall)}).toZonedDateTime(${q(zone)}, { disambiguation: ${q(d)} }).toString()`);
    }
  }
  P(`Temporal.ZonedDateTime.from(${q(gap + "[" + zone + "]")}).toString()`);
  P(`Temporal.ZonedDateTime.from(${q(overlap + "[" + zone + "]")}).epochMilliseconds`);
}

// E) Fusos históricos e offset ao segundo.
for (const zone of ZONES.concat(["Europe/Amsterdam", "Africa/Monrovia", "Asia/Kolkata"])) {
  for (const t of [-2208988800000, -2000000000000, 0, 86400000 * 365]) {
    P(`Temporal.Instant.fromEpochMilliseconds(${t}).toZonedDateTimeISO(${q(zone)}).offset`);
    P(`Temporal.Instant.fromEpochMilliseconds(${t}).toZonedDateTimeISO(${q(zone)}).offsetNanoseconds`);
  }
  P(`new Intl.DateTimeFormat('en-US', { timeZone: ${q(zone)}, timeZoneName: 'longOffset', year: 'numeric' }).format(-2208988800000)`);
}

// F) timeZoneId, getTimeZoneTransition e from com colchetes.
for (const zone of ZONES) {
  P(`Temporal.ZonedDateTime.from('2024-06-15T12:00:00[' + ${q(zone)} + ']').timeZoneId`);
  for (const dir of ["next", "previous"]) {
    for (const t of [0, 1704067200000, 1720000000000]) {
      P(`Temporal.Instant.fromEpochMilliseconds(${t}).toZonedDateTimeISO(${q(zone)}).getTimeZoneTransition(${q(dir)})?.toString()`);
    }
  }
  P(`Temporal.ZonedDateTime.from('2024-06-15T12:00:00[' + ${q(zone)} + ']').hoursInDay`);
  P(`Temporal.ZonedDateTime.from('2024-06-15T12:00:00[' + ${q(zone)} + ']').startOfDay().toString()`);
}
for (const text of ["2024-01-01T00:00:00[America/Sao_Paulo]", "2024-01-01T00:00:00-03:00[America/Sao_Paulo]", "2024-01-01T00:00:00+09:00[America/Sao_Paulo]", "2024-01-01T00:00:00[Asia/Calcutta]", "2024-01-01T00:00:00[UTC]", "2024-01-01T00:00:00[+05:30]", "2024-01-01T00:00:00[Foo/Bar]", "2024-01-01T00:00:00[us/pacific]", "2024-01-01T00:00:00[Etc/GMT+3]"]) {
  P(`Temporal.ZonedDateTime.from(${q(text)}).toString()`);
  P(`Temporal.ZonedDateTime.from(${q(text)}).timeZoneId`);
}

// G) Date com fuso local do ambiente (UTC e America/Sao_Paulo).
const LOCAL_INSTANTS = [-2208988800000, -1, 0, 1541289600000, 1541296800000, 1550000000000, 1550000000001, 1710054000000, 1720000000000, 1730000000000, 1760000000000, 4102444800000, 8.64e15, -8.64e15, NaN];
for (const t of LOCAL_INSTANTS) {
  for (const m of ["getTimezoneOffset", "toString", "toTimeString", "toDateString", "toLocaleString", "toLocaleTimeString", "toLocaleDateString", "toISOString", "toUTCString"]) {
    P(`new Date(${t}).${m}()`, true);
  }
  P(`new Date(${t}).toLocaleString('en-US', { timeZoneName: 'long' })`, true);
  P(`new Date(${t}).toLocaleString('pt-BR', { timeZone: 'Asia/Kathmandu', timeZoneName: 'longOffset' })`, true);
}
for (const [y, mo, d, h] of [[2018, 10, 4, 0], [2018, 10, 4, 1], [2018, 1, 17, 23], [2018, 1, 18, 0], [1900, 0, 1, 0], [1970, 0, 1, 0], [2019, 1, 16, 23], [2024, 2, 10, 2]]) {
  P(`new Date(${y}, ${mo}, ${d}, ${h}, 30).getTime() + '|' + new Date(${y}, ${mo}, ${d}, ${h}, 30).getTimezoneOffset()`, true);
  P(`new Date(${y}, ${mo}, ${d}, ${h}, 30).toString()`, true);
}
for (const zone of ZONES) {
  P(`new Date(1710054000000).toLocaleString('en-US', { timeZone: ${q(zone)}, timeZoneName: 'short' }) + '|' + new Date(1710054000000).toString()`, true);
}

// ---- Execução ----
const child = `
const fs = require('fs');
const list = JSON.parse(fs.readFileSync(0, 'utf8'));
const out = [];
for (const src of list) {
  globalThis.R = undefined;
  try { (0, eval)(src); out.push(String(globalThis.R)); } catch (e) { out.push('E:' + e.name); }
}
process.stdout.write(JSON.stringify(out));
`;
const ascii = (value) => JSON.stringify(value).replace(/[\u007f-￿]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
const results = {};
for (const tz of ["UTC", "America/Sao_Paulo"]) {
  const wanted = programs.filter((p) => tz === "UTC" || p.local);
  const run = spawnSync(process.execPath, ["-e", child], { env: { ...process.env, TZ: tz }, input: JSON.stringify(wanted.map((p) => p.src)), encoding: "utf8", maxBuffer: 1 << 28 });
  if (run.status !== 0) throw new Error(run.stderr);
  const values = JSON.parse(run.stdout);
  results[tz] = wanted.map((p, i) => [p.src, values[i]]);
}
const lines = [];
for (const tz of ["UTC", "America/Sao_Paulo"]) for (const [src, value] of results[tz]) lines.push(`${tz}\t${ascii(src)}\t${ascii(value)}`);
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
