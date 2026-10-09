// Gera src/runtime/intl_date_time_data.rs e tests/golden/datetime_more_bun.tsv a partir do bun:
// nomes de mês, dia da semana, AM/PM e era, o ciclo de horas padrão e os padrões resolvidos de cada
// skeleton (dateStyle/timeStyle e os componentes mais comuns) de es, fr, de, it, ja, ru, ar, zh e ko.
// Tudo é medido em UTC e com datas fixas, então a saída é reproduzível.
//
// Como regenerar (da raiz da crate zjsc):
//   bun scripts/gen-datetime-data.js
//
// Um padrão é a ordem das partes de `formatToParts` com os literais; cada parte vira um token
// `{tipo:argumento}` (`{month:long:s}` é a forma isolada, "standalone"). A chave de um skeleton é a lista
// `campo=valor` na ordem weekday, era, year, month, day, dayPeriod, hour, minute, second,
// fractionalSecondDigits, timeZoneName, dateStyle,
// timeStyle, mais `|12` ou `|24` quando há hora. O mesmo texto é montado por `data_key` em
// `src/runtime/intl_date_time_format.rs`.
process.env.TZ = "UTC";
const fs = require("fs");
const path = require("path");
const { writeRustSource } = require("./rust-escape.js");

// As línguas e, depois delas, as variantes regionais: a tabela é chaveada pelo tag inteiro (`en-GB`) e a busca
// em `locale_data_for` tenta primeiro `língua-REGIÃO` e depois a língua sozinha.
const LOCALES = [
  "en", "pt", "es", "fr", "de", "it", "ja", "ru", "ar", "zh", "ko", "nl",
  "hi", "th", "tr", "pl", "sv", "da", "nb", "fi", "cs", "el", "he", "id", "vi", "uk",
  "en-GB", "en-AU", "en-CA", "en-IN", "pt-PT", "es-MX", "es-AR", "fr-CA", "de-AT", "de-CH", "zh-TW", "zh-HK",
  "hu", "fa",
  "am", "my", "km", "lo", "mn", "ps", "sd", "so", "fil", "ha", "yo", "zu", "xh", "cy", "gd", "lb", "mt", "fo", "ky", "tg", "tk", "tt", "ku", "or", "as",
];
// Línguas cujo padrão no bun não é o gregoriano com dígitos latinos (fa usa o calendário persa e dígitos
// persas): os dados são medidos no gregoriano com dígitos ASCII, que é o que o consumidor renderiza; o
// calendário nativo padrão é tratado fora da tabela (`data_range` desiste quando há data nativa).
// my (mymr), sd (arab), as (beng) usam numeral nativo e ps (persa mais arabext) também o calendário, medidos igual.
const FORCED = {
  fa: { calendar: "gregory", numberingSystem: "latn" },
  ps: { calendar: "gregory", numberingSystem: "latn" },
  my: { numberingSystem: "latn" },
  sd: { numberingSystem: "latn" },
  as: { numberingSystem: "latn" },
};
const staticName = (locale) => locale.replace(/-/g, "_").toUpperCase();
const AM = Date.UTC(2024, 2, 5, 7, 8, 9);
const PM = Date.UTC(2024, 2, 5, 19, 8, 9);
const BC = new Date(AM);
BC.setUTCFullYear(-99);
const FIELDS = ["weekday", "era", "year", "month", "day", "dayPeriod", "hour", "minute", "second", "fractionalSecondDigits", "timeZoneName", "dateStyle", "timeStyle"];
// Todos os fusos que o bun conhece (`Intl.supportedValuesOf('timeZone')`, mais UTC e os apelidos IANA que o
// `Date.prototype.toString()` já mede em `time_zone_names_data.rs`): o nome curto (`AEDT`, `GMT+11`, `UTC+11`...)
// varia por locale, então cada fuso é medido em todos os locales. Só entra na tabela o nome que difere do formato
// GMT que o Rust calcula sozinho (ver `extras`), o que mantém o dado pequeno.
function allZones() {
  const text = fs.readFileSync(path.join(__dirname, "../src/runtime/time_zone_names_data.rs"), "utf8");
  const found = new Set([...Intl.supportedValuesOf("timeZone"), "UTC"]);
  for (const [, zone] of text.matchAll(/^    \("([A-Za-z_+\-0-9]+(?:\/[A-Za-z_+\-0-9]+)*)",/gm)) {
    try { new Intl.DateTimeFormat("en", { timeZone: zone }); found.add(zone); } catch { /* apelido que o bun não aceita */ }
  }
  return [...found].sort();
}
const ZONES = allZones();
// O golden usa todos os fusos em en e pt e uma amostra fixa (um a cada 16) nos demais locales, para o arquivo não
// crescer com 60 locales vezes todos os fusos.
const goldenZones = (locale) => (locale === "en" || locale === "pt" ? ZONES : ZONES.filter((_, index) => index % 16 === 0));
const ZONE_STYLES = ["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"];
const WINTER = Date.UTC(2024, 0, 15, 12, 8, 9);
const SUMMER = Date.UTC(2024, 6, 15, 12, 8, 9);
const RAMADAN = Date.UTC(2024, 2, 25, 12, 8, 9);
const NEGATIVE_DST_ZONES = ["Europe/Dublin", "Africa/Casablanca", "Africa/El_Aaiun"];
const DAY_PERIOD_WIDTHS = ["narrow", "short", "long"];
const ALL_LOCALES = LOCALES;
const WIDTHS = ["long", "short", "narrow"];

const make = (locale, options) => new Intl.DateTimeFormat(locale, { timeZone: "UTC", ...FORCED[locale], ...options });
const partsOf = (locale, options, time) => make(locale, options).formatToParts(time);
const partValue = (locale, options, time, type) => partsOf(locale, options, time).find((part) => part.type === type).value;

function keyOf(options) {
  const fields = FIELDS.filter((name) => options[name] !== undefined).map((name) => `${name}=${options[name]}`);
  const hasHour = options.hour !== undefined || options.timeStyle !== undefined;
  return fields.join(";") + (hasHour ? "|" + (options.__cycle === "h12" ? "12" : "24") : "");
}

function tables(locale) {
  const months = { format: [], standalone: [] };
  const weekdays = { format: [], standalone: [] };
  for (const width of WIDTHS) {
    const monthFormat = [];
    const monthStandalone = [];
    for (let month = 0; month < 12; month++) {
      const time = Date.UTC(2024, month, 5, 12);
      monthFormat.push(partValue(locale, { day: "numeric", month: width }, time, "month"));
      monthStandalone.push(partValue(locale, { month: width }, time, "month"));
    }
    months.format.push(monthFormat);
    months.standalone.push(monthStandalone);
    const weekdayFormat = [];
    const weekdayStandalone = [];
    for (let weekday = 0; weekday < 7; weekday++) {
      const time = Date.UTC(2024, 2, 3 + weekday, 12);
      weekdayFormat.push(partValue(locale, { weekday: width, day: "numeric", month: "long" }, time, "weekday"));
      weekdayStandalone.push(partValue(locale, { weekday: width }, time, "weekday"));
    }
    weekdays.format.push(weekdayFormat);
    weekdays.standalone.push(weekdayStandalone);
  }
  const eras = WIDTHS.map((width) => [
    partValue(locale, { era: width, year: "numeric" }, AM, "era"),
    partValue(locale, { era: width, year: "numeric" }, BC.getTime(), "era"),
  ]);
  const twelve = { hour: "numeric", hourCycle: "h12" };
  const dayPeriods = [partValue(locale, twelve, AM, "dayPeriod"), partValue(locale, twelve, PM, "dayPeriod")];
  const zone = [
    partValue(locale, { timeZoneName: "long", hour: "numeric" }, AM, "timeZoneName"),
    partValue(locale, { timeZoneName: "short", hour: "numeric" }, AM, "timeZoneName"),
  ];
  const cycleOf = (options) => make(locale, { hour: "numeric", ...options }).resolvedOptions().hourCycle;
  return {
    months, weekdays, eras, dayPeriods, zone,
    defaultCycle: cycleOf({}), hour12True: cycleOf({ hour12: true }), hour12False: cycleOf({ hour12: false }),
  };
}

/** O token de um nome de mês ou de dia da semana: forma de formato, senão isolada; `null` se não é nome. */
function nameToken(type, value, table, index, resample) {
  let candidates = [];
  for (const form of ["format", "standalone"]) {
    for (let width = 0; width < 3; width++) {
      if (table[form][width][index] === value) candidates.push({ form, width });
    }
  }
  // Nomes iguais em larguras diferentes na data amostrada ("mars" longo e curto em março) não dizem qual é a
  // largura do padrão: reamostra em outros meses (ou dias da semana) até sobrar uma só.
  const count = type === "month" ? 12 : 7;
  for (let other = 0; other < count && candidates.length > 1; other++) {
    if (other === index) continue;
    const sample = resample(other);
    candidates = candidates.filter(({ form, width }) => table[form][width][other] === sample);
  }
  if (candidates.length === 0) return null;
  const { form, width } = candidates[0];
  return `{${type}:${WIDTHS[width]}${form === "standalone" ? ":s" : ""}}`;
}

/** O padrão de `options` no instante `time`, ou `null` se alguma parte não tem token. */
function pattern(locale, data, options, time) {
  const { __cycle, ...rest } = options;
  const hasHour = rest.hour !== undefined || rest.timeStyle !== undefined;
  const parts = partsOf(locale, hasHour ? { ...rest, hourCycle: __cycle } : rest, time);
  let out = "";
  for (const { type, value } of parts) {
    const width = value.length >= 2 && value.startsWith("0") ? "2-digit" : "numeric";
    let token = null;
    if (type === "literal") {
      if (/[{}]/.test(value)) return null;
      out += value;
      continue;
    }
    const sampled = (name, shifted) => (other) =>
      partsOf(locale, hasHour ? { ...rest, hourCycle: __cycle } : rest, shifted(other)).find((part) => part.type === name)?.value;
    if (type === "weekday") token = nameToken("weekday", value, data.weekdays, 2, sampled("weekday", (day) => Date.UTC(2024, 2, 3 + day, 12)));
    else if (type === "month") token = nameToken("month", value, data.months, 2, sampled("month", (month) => Date.UTC(2024, month, 5, 12))) ?? `{month:${width}}`;
    else if (type === "year") token = `{year:${value.length === 2 ? "2-digit" : "numeric"}}`;
    else if (type === "day" || type === "hour" || type === "minute" || type === "second") token = `{${type}:${width}}`;
    else if (type === "era") {
      const index = data.eras.findIndex((pair) => pair[0] === value);
      token = index < 0 ? null : `{era:${WIDTHS[index]}}`;
    } else if (type === "dayPeriod") {
      if (options.dayPeriod !== undefined) token = `{dayPeriodFlex:${options.dayPeriod}}`;
      else if (data.dayPeriods.includes(value)) token = "{dayPeriod}";
      else {
        // Período flexível (`B` do ICU: zh-TW `清晨`, `晚上`): o ICU o escolhe pela hora, não é AM/PM. A largura é a que
        // bate com o formatador de `dayPeriod` em duas horas distintas; o renderizador lê o extra `dp|largura|hora|exato`.
        const periodOf = (instance, at) => instance.formatToParts(at).find((part) => part.type === "dayPeriod")?.value;
        const actual = make(locale, hasHour ? { ...rest, hourCycle: __cycle } : rest);
        const flex = ["short", "long", "narrow"].find((width) => {
          const reference = make(locale, { hour: "numeric", dayPeriod: width, hourCycle: "h12" });
          return [time, time + 14 * 3600000].every((at) => periodOf(reference, at) === periodOf(actual, at));
        });
        token = flex ? `{dayPeriodFlex:${flex}}` : null;
      }
    } else if (type === "fractionalSecond") token = `{fraction:${value.length}}`;
    else if (type === "timeZoneName") {
      // O `timeStyle` full usa o nome longo do fuso e o long o curto (o ICU pede `zzzz` e `z`).
      const style = options.timeZoneName ?? { full: "long", long: "short" }[options.timeStyle];
      token = style === undefined ? null : `{tz:${style}}`;
    }
    if (token === null) return null;
    out += token;
  }
  return out;
}

// Os skeletons.
const dateSkeletons = [];
for (const weekday of [undefined, "short", "long"]) {
  for (const year of [undefined, "numeric", "2-digit"]) {
    for (const month of [undefined, "numeric", "2-digit", "short", "long"]) {
      for (const day of [undefined, "numeric", "2-digit"]) {
        const options = {};
        if (weekday) options.weekday = weekday;
        if (year) options.year = year;
        if (month) options.month = month;
        if (day) options.day = day;
        if (Object.keys(options).length) dateSkeletons.push(options);
      }
    }
  }
}
dateSkeletons.push({ weekday: "narrow" }, { month: "narrow" }, { year: "numeric", month: "narrow", day: "numeric" });
for (const era of ["short", "long"]) {
  dateSkeletons.push({ era, year: "numeric", month: "numeric", day: "numeric" });
  dateSkeletons.push({ era, year: "numeric", month: "long", day: "numeric" });
}
const timeSkeletons = [];
for (const hour of ["numeric", "2-digit"]) {
  timeSkeletons.push({ hour });
  for (const minute of ["numeric", "2-digit"]) {
    timeSkeletons.push({ hour, minute });
    for (const second of ["numeric", "2-digit"]) {
      timeSkeletons.push({ hour, minute, second });
      timeSkeletons.push({ hour, minute, second, timeZoneName: "short" });
      timeSkeletons.push({ hour, minute, second, timeZoneName: "long" });
    }
  }
}
for (const timeZoneName of ZONE_STYLES) {
  timeSkeletons.push({ hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName });
  timeSkeletons.push({ hour: "numeric", minute: "2-digit", timeZoneName });
  // O que `toLocaleString`/`toLocaleTimeString` pedem com só `timeZoneName`: todos os campos `numeric`.
  timeSkeletons.push({ hour: "numeric", minute: "numeric", second: "numeric", timeZoneName });
}
for (const dayPeriod of DAY_PERIOD_WIDTHS) {
  timeSkeletons.push({ dayPeriod, hour: "numeric" });
  timeSkeletons.push({ dayPeriod, hour: "numeric", minute: "2-digit" });
}
for (const fractionalSecondDigits of [1, 2, 3]) {
  timeSkeletons.push({ second: "numeric", fractionalSecondDigits });
  timeSkeletons.push({ minute: "2-digit", second: "2-digit", fractionalSecondDigits });
  timeSkeletons.push({ hour: "numeric", minute: "2-digit", second: "2-digit", fractionalSecondDigits });
}
const joinedDates = [
  { year: "numeric", month: "numeric", day: "numeric" },
  { year: "numeric", month: "2-digit", day: "2-digit" },
  { year: "numeric", month: "short", day: "numeric" },
  { year: "numeric", month: "long", day: "numeric" },
  { weekday: "long", year: "numeric", month: "long", day: "numeric" },
  { weekday: "short", year: "numeric", month: "short", day: "numeric" },
  { month: "short", day: "numeric" },
  { month: "long", day: "numeric" },
];
const joinedTimes = [
  { hour: "numeric", minute: "2-digit" },
  { hour: "numeric", minute: "2-digit", second: "2-digit" },
  { hour: "numeric", minute: "numeric", second: "numeric" },
  { hour: "2-digit", minute: "2-digit", second: "2-digit" },
  { hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "short" },
  { hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "long" },
  { hour: "numeric", minute: "numeric", second: "numeric", timeZoneName: "short" },
  { hour: "numeric", minute: "numeric", second: "numeric", timeZoneName: "long" },
];
const styles = ["full", "long", "medium", "short"];
const skeletons = [];
for (const options of dateSkeletons) skeletons.push(options);
for (const options of timeSkeletons) for (const cycle of ["h12", "h23"]) skeletons.push({ ...options, __cycle: cycle });
for (const date of joinedDates) {
  for (const time of joinedTimes) for (const cycle of ["h12", "h23"]) skeletons.push({ ...date, ...time, __cycle: cycle });
}
// O dia da semana sozinho com a hora (o ICU junta com espaço ou vírgula conforme o locale) e minuto com segundo.
for (const weekday of ["short", "long"]) {
  for (const time of [{ hour: "numeric" }, ...joinedTimes]) {
    for (const cycle of ["h12", "h23"]) skeletons.push({ weekday, ...time, __cycle: cycle });
  }
}
for (const cycle of ["h12", "h23"]) skeletons.push({ minute: "2-digit", second: "2-digit", __cycle: cycle });
for (const dateStyle of styles) skeletons.push({ dateStyle });
for (const timeStyle of styles) for (const cycle of ["h12", "h23"]) skeletons.push({ timeStyle, __cycle: cycle });
for (const dateStyle of styles) {
  for (const timeStyle of styles) for (const cycle of ["h12", "h23"]) skeletons.push({ dateStyle, timeStyle, __cycle: cycle });
}

// O formato GMT localizado (`gmtFormat` com `hourFormat`) que o ICU usa para o fuso sem nome no locale, em dígitos
// ASCII (a conversão para o `numberingSystem` é de quem formata). Sai como modelos: `gmt|short|pos`, `neg`, `posmin` e
// `negmin` (estilo `shortOffset`, sem e com minutos, sinal positivo e negativo) e `gmt|long|pos`, `gmt|long|neg`
// (`longOffset`, sempre com minutos), com os marcadores `{h}` (hora sem zero à esquerda), `{hh}` (com) e `{mm}`.
// Medido: o deslocamento zero usa o modelo positivo (`GMT+0`, `GMT+00:00`), sem `gmtZeroFormat`. `gmt|zero` é o nome
// curto de `Etc/GMT` (`GMT` em en, es, nl..., `GMT+0` nos demais). O gerador confere os modelos contra o bun em 40
// deslocamentos e lança se algum não reproduz.
const GMT_PROBES = [
  ["Etc/GMT-13", 13 * 60], ["Etc/GMT+3", -3 * 60], ["Asia/Kolkata", 330], ["Pacific/Marquesas", -570],
  ["Pacific/Chatham", 13 * 60 + 45], ["America/St_Johns", -210], ["Asia/Kathmandu", 345], ["Australia/Adelaide", 630],
  ["Etc/GMT-14", 14 * 60], ["Etc/GMT+12", -12 * 60], ["Etc/GMT-1", 60], ["Etc/GMT+1", -60], ["Etc/GMT", 0],
];
function gmtOffsetAt(locale, zone, style) {
  return partValue(locale, { timeZone: zone, hour: "numeric", timeZoneName: style, numberingSystem: "latn" }, WINTER, "timeZoneName");
}
function renderGmt(templates, style, minutes) {
  const sign = minutes < 0 ? "neg" : "pos";
  const hours = Math.floor(Math.abs(minutes) / 60);
  const rest = Math.abs(minutes) % 60;
  const template = style === "long" ? templates[`long|${sign}`] : templates[`short|${sign}${rest === 0 ? "" : "min"}`];
  return template.replace("{hh}", String(hours).padStart(2, "0")).replace("{h}", String(hours)).replace("{mm}", String(rest).padStart(2, "0"));
}
function gmtTemplates(locale) {
  const long = (zone) => gmtOffsetAt(locale, zone, "longOffset");
  const short = (zone) => gmtOffsetAt(locale, zone, "shortOffset");
  const withMinutes = (text, hour) => text.replace("30", "{mm}").replace(hour, "{h}");
  const templates = {
    "short|pos": short("Etc/GMT-13").replace("13", "{h}"),
    "short|neg": short("Etc/GMT+3").replace("3", "{h}"),
    "short|posmin": withMinutes(short("Asia/Kolkata"), "5"),
    "short|negmin": withMinutes(short("Pacific/Marquesas"), "9"),
    "long|pos": long("Etc/GMT-13").replace("00", "{mm}").replace("13", "{hh}"),
    "long|neg": long("Etc/GMT+3").replace("00", "{mm}").replace("03", "{hh}"),
  };
  for (const [zone, minutes] of GMT_PROBES) {
    for (const style of ["short", "long"]) {
      const expected = gmtOffsetAt(locale, zone, `${style}Offset`);
      const actual = renderGmt(templates, style, minutes);
      if (expected !== actual) throw new Error(`${locale}: gmt ${style} ${zone}: bun ${JSON.stringify(expected)}, modelo ${JSON.stringify(actual)}`);
    }
  }
  return { templates, zero: gmtOffsetAt(locale, "Etc/GMT", "short") };
}
function gmtRows(locale) {
  const { templates, zero } = gmtTemplates(locale);
  return [...Object.entries(templates).map(([key, value]) => [`gmt|${key}`, value]), ["gmt|zero", zero]];
}
// O nome que o Rust calcula quando a tabela não tem a linha do fuso (`time_zone_name_text`): o GMT localizado do
// deslocamento, e o curto do deslocamento zero é o de `Etc/GMT`.
function gmtFallback(model, style, minutes) {
  if ((style === "short" || style === "shortGeneric") && minutes === 0) return model.zero;
  return renderGmt(model.templates, style.startsWith("short") ? "short" : "long", minutes);
}
const offsetCache = new Map();
function offsetMinutes(zone, time) {
  const key = `${zone}|${time}`;
  if (!offsetCache.has(key)) {
    const text = partValue("en", { timeZone: zone, hour: "numeric", timeZoneName: "longOffset" }, time, "timeZoneName");
    const found = /^GMT(?:([+-])(\d\d):(\d\d))?$/.exec(text);
    if (!found) throw new Error(`${zone}: longOffset inesperado ${JSON.stringify(text)}`);
    offsetCache.set(key, found[1] ? (found[1] === "-" ? -1 : 1) * (Number(found[2]) * 60 + Number(found[3])) : 0);
  }
  return offsetCache.get(key);
}
// `w` é o horário padrão (o de menor deslocamento) e `s` o de verão, o que o Rust pergunta pelo `is_dst`; no
// hemisfério sul janeiro é o verão. Fuso sem horário de verão usa o mesmo instante nos dois.
function zoneSeasons(zone) {
  return offsetMinutes(zone, WINTER) <= offsetMinutes(zone, SUMMER) ? { w: WINTER, s: SUMMER } : { w: SUMMER, s: WINTER };
}

// Dados avulsos medidos por locale: `tz|fuso|estilo|w` (inverno) ou `s` (verão), `dp|largura|hora|exato`
// (nome flexível do período do dia, `exato` é 1 em hh:00:00), `frac|sep` (literal entre segundo e fração) e
// `range|cenário|sep` (literal compartilhado entre os dois extremos de formatRange) e `range|cenário|collapse`
// (`1` se os campos comuns saem uma vez, `0` se as duas datas se repetem inteiras).
function extras(locale) {
  const out = [];
  // `tz|fuso|estilo|w` (padrão) e `s` (verão) só existem quando o nome difere do GMT localizado que o Rust calcula
  // sozinho; a busca de `s` cai em `w` quando não há linha de verão (nome igual nas duas estações).
  const model = gmtTemplates(locale);
  for (const zone of ZONES) {
    const seasons = zoneSeasons(zone);
    for (const style of ZONE_STYLES) {
      const value = {};
      const fallback = {};
      for (const season of ["w", "s"]) {
        value[season] = partValue(locale, { timeZone: zone, hour: "numeric", timeZoneName: style }, seasons[season], "timeZoneName");
        fallback[season] = gmtFallback(model, style, offsetMinutes(zone, seasons[season]));
      }
      const hasStandard = value.w !== fallback.w;
      if (hasStandard) out.push([`tz|${zone}|${style}|w`, value.w]);
      if (value.s !== (hasStandard ? value.w : fallback.s)) out.push([`tz|${zone}|${style}|s`, value.s]);
    }
  }
  out.push(...gmtRows(locale));
  for (const width of DAY_PERIOD_WIDTHS) {
    for (let hour = 0; hour < 24; hour++) {
      for (const [exact, minute] of [[1, 0], [0, 30]]) {
        const parts = partsOf(locale, { hour: "numeric", dayPeriod: width, hourCycle: "h12" }, Date.UTC(2024, 2, 5, hour, minute, 0));
        const found = parts.find((part) => part.type === "dayPeriod");
        if (found) out.push([`dp|${width}|${hour}|${exact}`, found.value]);
      }
    }
  }
  const fractionParts = partsOf(locale, { second: "numeric", fractionalSecondDigits: 2 }, AM + 123);
  const fractionAt = fractionParts.findIndex((part) => part.type === "fractionalSecond");
  if (fractionAt > 0 && fractionParts[fractionAt - 1].type === "literal") out.push(["frac|sep", fractionParts[fractionAt - 1].value]);
  // `sdj|0..3` (full, long, medium, short): a cola `standard` entre a data e a hora no intervalo do mesmo dia com
  // `dateStyle` (o literal compartilhado antes da primeira parte de hora ou de período do dia do primeiro extremo).
  ["full", "long", "medium", "short"].forEach((dateStyle, index) => {
    const parts = make(locale, { dateStyle, timeStyle: "short" }).formatRangeToParts(Date.UTC(2023, 10, 14, 22, 13), Date.UTC(2023, 10, 14, 23, 13));
    const timeAt = parts.findIndex((part) => part.type === "hour" || part.type === "dayPeriod");
    let from = timeAt;
    while (from > 0 && parts[from - 1].type === "literal" && parts[from - 1].source === "shared") from--;
    // O literal que fecha a data (`г.`, `일`, `.`) vem junto: o consumidor troca todo o literal final da data pela cola.
    const joiner = parts.slice(from, timeAt).map((part) => part.value).join("");
    if (timeAt >= 0) out.push([`sdj|${index}`, joiner]);
  });
  for (const family of ZONE_DATE) out.push(...zoneDateRows(locale, family));
  for (const wrap of ZONE_WRAPS) out.push(...zoneWrapRows(locale, wrap));
  for (const scenario of [...RANGES, ...ZONE_RANGES]) {
    const parts = make(locale, scenario.options).formatRangeToParts(scenario.from, scenario.to);
    const first = parts.findIndex((part) => part.source === "endRange");
    const last = parts.map((part) => part.source).lastIndexOf("startRange");
    const between = parts.slice(last + 1, first).filter((part) => part.source === "shared");
    out.push([`range|${scenario.name}|sep`, between.map((part) => part.value).join("")]);
    // Só hora em dias diferentes: o intervalo é o `format` com `yMd` de cada ponta e o separador entre elas. O `sep` acima
    // leva o literal da hora (` h`, ` Uhr`) quando o ciclo é de 24 horas; `pairsep` é o que sobra entre os dois `format`
    // (medido sem os zeros à esquerda, porque a largura da hora é a de `hourpad`).
    if (scenario.name.startsWith("days_")) {
      const unpadded = (text) => text.replace(/(^|\D)0(\d)/g, "$1$2");
      const dated = make(locale, { ...scenario.options, year: "numeric", month: "numeric", day: "numeric" });
      const whole = unpadded(make(locale, scenario.options).formatRange(scenario.from, scenario.to));
      const left = unpadded(dated.format(scenario.from));
      const right = unpadded(dated.format(scenario.to));
      if (!whole.startsWith(left) || !whole.endsWith(right)) throw new Error(`${locale} ${scenario.name}: o intervalo não é format + separador + format`);
      out.push([`range|${scenario.name}|pairsep`, whole.slice(left.length, whole.length - right.length)]);
    }
    // `1` quando algum trecho `shared` fica antes do primeiro extremo ou depois do último (o ICU colapsa os
    // campos comuns); `0` quando ele repete as duas datas inteiras (ja em `same_month`).
    const sources = parts.map((part) => part.source);
    const collapsed = sources.some((kind, index) => kind === "shared" && (index < sources.indexOf("startRange") || index > sources.lastIndexOf("endRange")));
    out.push([`range|${scenario.name}|collapse`, collapsed ? "1" : "0"]);
    // Quantas partes `shared` abrem o intervalo antes do primeiro extremo e quantas o fecham depois do último: o
    // ICU não junta o maior sufixo igual (pt `5 de mar. – 9 de mar. de 2024` só junta o ano).
    const firstStart = sources.indexOf("startRange");
    const lastEnd = sources.lastIndexOf("endRange");
    out.push([`range|${scenario.name}|head`, String(firstStart)]);
    out.push([`range|${scenario.name}|tail`, String(sources.length - 1 - lastEnd)]);
    // Quantas partes saem como `startRange` e como `endRange` (`Mar 5 – 9, 2024` em en: três e uma).
    out.push([`range|${scenario.name}|startlen`, String(sources.filter((kind) => kind === "startRange").length)]);
    out.push([`range|${scenario.name}|endlen`, String(sources.filter((kind) => kind === "endRange").length)]);
    // A largura da hora das pontas: o padrão de intervalo tem o seu (`HH:mm` em pt, fr, de) e não segue o `format`
    // (`H:mm`), e muda com o ciclo (`7:08 AM` em 12 horas, `07:08` em 24). A hora do início é 7: `1` se sai com dois dígitos.
    if (scenario.options.hour) {
      for (const [cycle, label] of [["h12", "12"], ["h23", "24"]]) {
        const cycled = make(locale, { ...scenario.options, hourCycle: cycle }).formatRangeToParts(scenario.from, scenario.to);
        const startHour = cycled.find((part) => part.type === "hour" && part.source === "startRange");
        if (startHour) out.push([`range|${scenario.name}|hourpad|${label}`, [...startHour.value].length >= 2 ? "1" : "0"]);
      }
    }
  }
  return out.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
}

const RANGES = [
  { name: "same_day", options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }, from: AM, to: PM },
  { name: "same_month", options: { year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2024, 2, 9, 12) },
  { name: "other_years", options: { year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2025, 3, 9, 12) },
  { name: "time_only", options: { hour: "numeric", minute: "2-digit" }, from: AM, to: PM },
  // Com `weekday` o ICU usa os padrões `yMMMEd`, que em geral não têm `d\u2013d` e repetem a data dos dois lados.
  { name: "same_month_weekday", options: { weekday: "short", year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2024, 2, 9, 12) },
  { name: "same_year_weekday", options: { weekday: "short", year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2024, 10, 9, 12) },
  { name: "other_years_weekday", options: { weekday: "short", year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2025, 3, 9, 12) },
  // Maior diferença no mês (mesmo ano) e na hora com o mesmo AM/PM (ciclo de 12 horas): o ICU escolhe outro
  // padrão de intervalo (`intervalFormats` y/M/d e a/h/m), e o `collapse` diz se o resto sai uma vez.
  { name: "same_year", options: { year: "numeric", month: "short", day: "numeric" }, from: AM, to: Date.UTC(2024, 10, 9, 12) },
  // Mês numérico (`yMd`, o `dateStyle: "short"`): o CLDR quase nunca tem `d–d` e as datas se repetem inteiras.
  { name: "same_month_numeric", options: { year: "numeric", month: "numeric", day: "numeric" }, from: AM, to: Date.UTC(2024, 2, 9, 12) },
  { name: "same_year_numeric", options: { year: "numeric", month: "numeric", day: "numeric" }, from: AM, to: Date.UTC(2024, 10, 9, 12) },
  { name: "other_years_numeric", options: { year: "numeric", month: "numeric", day: "numeric" }, from: AM, to: Date.UTC(2025, 3, 9, 12) },
  { name: "same_period", options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle: "h12" }, from: AM, to: Date.UTC(2024, 2, 5, 9, 38, 9) },
  { name: "same_period_time", options: { hour: "numeric", minute: "2-digit", hourCycle: "h12" }, from: AM, to: Date.UTC(2024, 2, 5, 9, 38, 9) },
  // Com segundos (ou fração) o ICU não tem padrão de intervalo e usa o fallback, sem juntar o AM/PM: o
  // separador é outro (`07:08\u201319:08` em de, `07:08:09 \u2013 19:08:09` com segundos).
  { name: "same_day_seconds", options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", second: "2-digit" }, from: AM, to: PM },
  { name: "time_only_seconds", options: { hour: "numeric", minute: "2-digit", second: "2-digit" }, from: AM, to: PM },
  // Só hora em dias diferentes: o ICU prefixa a data e a largura da hora é a do padrão de intervalo (`7:08` em es).
  { name: "days_time", options: { hour: "numeric", minute: "2-digit" }, from: AM, to: Date.UTC(2024, 2, 25, 19, 8, 9) },
  { name: "days_hour", options: { hour: "numeric" }, from: AM, to: Date.UTC(2024, 2, 25, 19, 8, 9) },
];

// `timeZoneName` no intervalo (medido em America/Sao_Paulo, onde o nome curto é `GMT-3`). Com segundos o ICU não tem
// padrão de intervalo e o fallback repete o fuso nos dois extremos, então estes cenários entram no mesmo laço de
// `sep`, `head`, `tail`, `startlen`, `endlen` e `hourpad` (o `sep` leva o literal que fecha o fuso, `] – ` em zh-TW).
const SAO_PAULO = "America/Sao_Paulo";
const ZONE_RANGES = [
  { name: "zone_same_day_seconds", options: { timeZone: SAO_PAULO, timeZoneName: "short", year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", second: "2-digit" }, from: AM, to: PM },
  { name: "zone_time_only_seconds", options: { timeZone: SAO_PAULO, timeZoneName: "short", hour: "numeric", minute: "2-digit", second: "2-digit" }, from: AM, to: PM },
];
const SAME_PERIOD_END = Date.UTC(2024, 2, 5, 9, 38, 9);
const NEXT_MONTH = Date.UTC(2024, 2, 9, 12);

// Data diferente com fuso: o ICU cai no fallback com as duas pontas inteiras. `sep` é o literal entre elas (varia por
// locale: `-` em da, ` a el ` em es-AR) e `ends` diz como cada ponta é escrita: `fmt` (o `format` com os mesmos campos),
// `num` (o `format` com mês numérico, em ja, zh, fi, cs) ou `x` (nenhum dos dois, ou os estilos de fuso discordam). Os
// estilos `shortOffset` e `longOffset` somem do intervalo só de data, então só entram nas famílias com hora.
const ZONE_DATE = [
  { name: "short", dateOnly: true, options: { year: "numeric", month: "short", day: "numeric" } },
  { name: "long", dateOnly: true, options: { year: "numeric", month: "long", day: "numeric" } },
  { name: "weekday", dateOnly: true, options: { weekday: "short", year: "numeric", month: "short", day: "numeric" } },
  { name: "time", dateOnly: false, options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle: "h12" } },
  { name: "time24", dateOnly: false, options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle: "h23" } },
];
function zoneDateShape(locale, family, style) {
  const options = { timeZone: SAO_PAULO, ...family.options, timeZoneName: style };
  const parts = make(locale, options).formatRangeToParts(AM, NEXT_MONTH);
  const sources = parts.map((part) => part.source);
  const first = sources.indexOf("endRange");
  const last = sources.lastIndexOf("startRange");
  const text = (list) => list.map((part) => part.value).join("");
  const sep = last < first ? text(parts.slice(last + 1, first)) : "";
  const firstStart = sources.indexOf("startRange");
  const lastEnd = sources.lastIndexOf("endRange");
  const edgeShared = sources.some((kind, index) => kind === "shared" && (index < firstStart || index > lastEnd));
  const middleOnlyShared = last < first && sources.slice(last + 1, first).every((kind) => kind === "shared");
  // Cada ponta é inteira do seu lado: nenhuma parte `shared` dentro delas.
  const wholeEnds = sources.slice(0, last + 1).every((kind) => kind === "startRange") && sources.slice(first).every((kind) => kind === "endRange");
  if (edgeShared || !middleOnlyShared || !wholeEnds) return { sep, ends: "x" };
  const start = text(parts.slice(0, last + 1));
  const end = text(parts.slice(first));
  const formatted = (extra) => [make(locale, { ...options, ...extra }).format(AM), make(locale, { ...options, ...extra }).format(NEXT_MONTH)];
  const [fmtStart, fmtEnd] = formatted({});
  const [numStart, numEnd] = formatted({ month: "numeric" });
  const ends = start === fmtStart && end === fmtEnd ? "fmt" : start === numStart && end === numEnd ? "num" : "x";
  return { sep, ends };
}
function zoneDateRows(locale, family) {
  const styles = ZONE_STYLES.filter((style) => !(family.dateOnly && style.endsWith("Offset")));
  const shapes = styles.map((style) => zoneDateShape(locale, family, style));
  const agree = shapes.every((shape) => shape.sep === shapes[0].sep && shape.ends === shapes[0].ends);
  const ends = agree ? shapes[0].ends : "x";
  return [[`range|zone_date_${family.name}|sep`, shapes[0].sep], [`range|zone_date_${family.name}|ends`, ends]];
}

// Fuso no intervalo SEM segundos: o ICU escolhe o padrão do intervalo sem o fuso e o acrescenta uma vez, mas o lugar e os
// literais dependem do locale (`04:08–16:08 Uhr GMT-3` em de, `4時08分～16時08分(GMT-3)` em ja, `GMT-3 04:08–16:08` em
// zh). `before` e `after` são o que o fuso acrescenta ao intervalo sem ele, com `{z}` no lugar do nome do fuso; só entram
// quando o mesmo molde vale nos estilos de fuso e com o mesmo AM/PM e AM/PM diferente.
const ZONE_WRAPS = [
  { name: "zone_time", options: { hour: "numeric", minute: "2-digit" } },
  { name: "zone_time_hour", options: { hour: "numeric" } },
  { name: "zone_day", options: { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" } },
  { name: "zone_day_hour", options: { year: "numeric", month: "short", day: "numeric", hour: "numeric" } },
];
function zoneWrapRows(locale, wrap) {
  const templates = new Set();
  for (const style of ["short", "long", "shortGeneric", "longGeneric"]) {
    for (const [to, cycle] of [[PM, {}], [SAME_PERIOD_END, { hourCycle: "h12" }]]) {
      const options = { timeZone: SAO_PAULO, ...wrap.options, ...cycle };
      const withZone = make(locale, { ...options, timeZoneName: style }).formatRangeToParts(AM, to);
      const base = make(locale, options).formatRange(AM, to);
      const zone = withZone.find((part) => part.type === "timeZoneName");
      const full = withZone.map((part) => part.value).join("");
      if (!zone) { templates.add("x"); continue; }
      if (full.startsWith(base)) templates.add(JSON.stringify(["", full.slice(base.length).replace(zone.value, "{z}")]));
      else if (full.endsWith(base)) templates.add(JSON.stringify([full.slice(0, full.length - base.length).replace(zone.value, "{z}"), ""]));
      else templates.add("x");
    }
  }
  if (templates.size !== 1 || templates.has("x")) return [];
  const [before, after] = JSON.parse([...templates][0]);
  return [[`range|${wrap.name}|before`, before], [`range|${wrap.name}|after`, after]];
}

// Saída em Rust.
const rs =(text) =>
  '"' + [...text].map((c) => (c === '"' || c === "\\" ? "\\" + c : c.codePointAt(0) < 0x7f && c.codePointAt(0) >= 0x20 ? c : `\\u{${c.codePointAt(0).toString(16)}}`)).join("") + '"';

let source = `//! Dados de \`Intl.DateTimeFormat\` das línguas e variantes regionais de \`LOCALES\` do gerador, medidos no bun.
//!
//! GERADO por \`scripts/gen-datetime-data.js\`: não edite à mão. Para regenerar, da raiz da crate:
//! \`bun scripts/gen-datetime-data.js\` (reescreve este arquivo e \`tests/golden/datetime_more_bun.tsv\`).
//!
//! Cada locale traz os nomes (mês e dia da semana por largura, em formato e isolados, AM/PM, era, nome
//! do UTC), o ciclo de horas padrão e uma tabela de padrões por skeleton. O padrão é uma sequência de
//! literais e tokens \`{tipo:argumento}\`; a chave do skeleton está descrita no gerador.

use ul_common::time::Civil;

type Part = (String, String);

/// Os dados de uma língua.
pub struct LocaleData {
    pub default_cycle: &'static str,
    pub hour12_true_cycle: &'static str,
    pub hour12_false_cycle: &'static str,
    /// \`[largura][mês]\`, largura na ordem long, short, narrow.
    months_format: [[u16; 12]; 3],
    months_standalone: [[u16; 12]; 3],
    /// \`[largura][dia]\`, domingo primeiro.
    weekdays_format: [[u16; 7]; 3],
    weekdays_standalone: [[u16; 7]; 3],
    /// \`[largura][d.C., a.C.]\`.
    eras: [[u16; 2]; 3],
    day_periods: [u16; 2],
    /// O nome do UTC: longo, curto.
    utc_zone: [u16; 2],
    /// Chave do skeleton e padrão, ordenados pela chave.
    entries: &'static [Row],
    /// Dados avulsos ordenados pela chave: nomes de fuso, períodos do dia, separadores (ver o gerador).
    extras: &'static [Row],
}

/// Uma linha das tabelas: índices em \`STRINGS\` da chave e do valor.
type Row = (u16, u16);

/// O texto de índice \`index\` no pool único \`STRINGS\` (todo texto das tabelas vive uma vez só).
fn string_at(index: u16) -> &'static str {
    STRINGS[usize::from(index)]
}

/// O valor da chave \`key\` na tabela \`rows\`, ordenada pela chave.
fn value_of(rows: &[Row], key: &str) -> Option<&'static str> {
    let position = crate::runtime::intl_table_lookup::sorted_position_by(rows, |row| string_at(row.0), key)?;
    Some(string_at(rows[position].1))
}

impl LocaleData {
    /// O dado avulso de chave \`key\`.
    pub fn extra(&self, key: &str) -> Option<&'static str> {
        value_of(self.extras, key)
    }

    /// O nome do fuso \`iana\` no estilo \`style\` (\`short\`, \`long\`, \`shortOffset\`, \`longOffset\`, \`shortGeneric\`,
    /// \`longGeneric\`), de verão ou não. A tabela só guarda o nome que difere do GMT localizado: \`None\` quer dizer
    /// que o nome é esse formato, que o chamador calcula do deslocamento. A linha de verão (\`s\`) ausente repete a
    /// do horário padrão (\`w\`).
    pub fn zone_name(&self, iana: &str, style: &str, summer: bool) -> Option<&'static str> {
        let row = |season: &str| self.extra(&format!("tz|{iana}|{style}|{season}"));
        if summer { row("s").or_else(|| row("w")) } else { row("w") }
    }

    /// O formato GMT localizado do deslocamento (\`GMT-3\`, \`غرينتش+5:30\`, \`UTC−03:00\`), em dígitos ASCII: \`long\` é o
    /// \`longOffset\` (\`HH:mm\`), senão o \`shortOffset\` (hora sem zero à esquerda, minutos só se não nulos). O deslocamento
    /// zero usa o modelo positivo (\`GMT+0\`), como o ICU do bun. \`None\` quando o locale não tem os modelos.
    pub fn gmt_offset_name(&self, long: bool, offset_seconds: i32) -> Option<String> {
        let magnitude = offset_seconds.unsigned_abs();
        let (hours, minutes) = (magnitude / 3600, magnitude % 3600 / 60);
        let sign = if offset_seconds < 0 { "neg" } else { "pos" };
        let key = match (long, minutes) {
            (true, _) => format!("gmt|long|{sign}"),
            (false, 0) => format!("gmt|short|{sign}"),
            (false, _) => format!("gmt|short|{sign}min"),
        };
        let template = self.extra(&key)?;
        Some(
            template
                .replace("{hh}", &format!("{hours:02}"))
                .replace("{h}", &hours.to_string())
                .replace("{mm}", &format!("{minutes:02}")),
        )
    }

    /// O nome curto do fuso de deslocamento zero sem nome próprio no locale (\`Etc/GMT\`): \`GMT\` em en, es, nl, \`GMT+0\` em de.
    pub fn gmt_zero_short_name(&self) -> Option<&'static str> {
        self.extra("gmt|zero")
    }

    /// O nome flexível do período do dia (\`narrow\`, \`short\`, \`long\`) para a hora, minuto e segundo.
    pub fn flexible_day_period(&self, width: &str, civil: &Civil) -> Option<&'static str> {
        let exact = civil.min == 0 && civil.sec == 0;
        self.extra(&format!("dp|{width}|{}|{}", civil.hour, u8::from(exact)))
    }

    /// O literal entre o segundo e a fração (\`.\` ou \`,\`).
    pub fn fraction_separator(&self) -> &'static str {
        self.extra("frac|sep").unwrap_or(".")
    }

    /// O padrão do skeleton \`key\`.
    pub fn pattern(&self, key: &str) -> Option<&'static str> {
        value_of(self.entries, key)
    }

    /// O nome do UTC (\`long\` ou curto).
    pub fn utc_zone_name(&self, long: bool) -> &'static str {
        string_at(self.utc_zone[if long { 0 } else { 1 }])
    }

    /// O nome do dia da semana (\`0\` é domingo) na largura \`long\`, \`short\` ou \`narrow\`, na forma de formato ou, com
    /// \`standalone\`, na isolada (a do skeleton só com o dia da semana, \`ccc\` no ICU).
    pub fn weekday_name(&self, width: &str, weekday: usize, standalone: bool) -> &'static str {
        let index = match width {
            "short" => 1,
            "narrow" => 2,
            _ => 0,
        };
        let table = if standalone { &self.weekdays_standalone } else { &self.weekdays_format };
        string_at(table[index][weekday])
    }

    /// O nome do mês isolado (\`month0\` de 0 a 11) na largura \`short\` ou \`narrow\`, o que o calendário \`iso8601\` usa com
    /// \`month: "short"\` e \`month: "narrow"\`.
    pub fn standalone_month_name(&self, width: &str, month0: usize) -> &'static str {
        string_at(self.months_standalone[if width == "narrow" { 2 } else { 1 }][month0])
    }

    /// O AM ou PM da hora de \`civil\`.
    fn day_period(&self, civil: &Civil) -> &'static str {
        string_at(self.day_periods[usize::from(civil.hour >= 12)])
    }

    /// As partes de \`civil\` formatado com \`pattern\`; \`cycle\` é \`h11\`, \`h12\`, \`h23\` ou \`h24\`, \`milliseconds\` é
    /// a fração de segundo (0 a 999) e \`zone\` devolve o nome do fuso no estilo pedido (\`short\`, \`long\`, ...).
    pub fn render(&self, pattern: &str, civil: &Civil, milliseconds: i64, cycle: &str, zone: &dyn Fn(&str) -> String) -> Vec<Part> {
        let mut parts: Vec<Part> = Vec::new();
        let mut literal = String::new();
        let mut rest = pattern;
        while let Some(character) = rest.chars().next() {
            if character != '{' {
                literal.push(character);
                rest = &rest[character.len_utf8()..];
                continue;
            }
            let end = rest.find('}').unwrap_or(rest.len());
            if !literal.is_empty() {
                parts.push(("literal".to_string(), std::mem::take(&mut literal)));
            }
            let mut fields = rest[1..end].split(':');
            let kind = fields.next().unwrap_or("");
            let argument = fields.next().unwrap_or("");
            let standalone = fields.next() == Some("s");
            parts.push(self.token(kind, argument, standalone, civil, milliseconds, cycle, zone));
            rest = &rest[(end + 1).min(rest.len())..];
        }
        if !literal.is_empty() {
            parts.push(("literal".to_string(), literal));
        }
        parts
    }

    pub fn token(&self, kind: &str, argument: &str, standalone: bool, civil: &Civil, milliseconds: i64, cycle: &str, zone: &dyn Fn(&str) -> String) -> Part {
        let width = match argument {
            "short" => 1,
            "narrow" => 2,
            _ => 0,
        };
        let digits = |value: i64| if argument == "2-digit" { format!("{value:02}") } else { value.to_string() };
        let before_common_era = civil.year < 1;
        let (name, value) = match kind {
            "weekday" => ("weekday", self.weekday_name(argument, civil.wday as usize, standalone).to_string()),
            "month" if matches!(argument, "long" | "short" | "narrow") => {
                let table = if standalone { &self.months_standalone } else { &self.months_format };
                ("month", string_at(table[width][civil.mon as usize - 1]).to_string())
            }
            "month" => ("month", digits(civil.mon)),
            "year" => {
                let year = if before_common_era { 1 - civil.year } else { civil.year };
                ("year", if argument == "2-digit" { format!("{:02}", year.rem_euclid(100)) } else { year.to_string() })
            }
            "day" => ("day", digits(civil.mday)),
            "hour" => {
                let hour = match cycle {
                    "h12" => if civil.hour % 12 == 0 { 12 } else { civil.hour % 12 },
                    "h11" => civil.hour % 12,
                    "h24" => if civil.hour == 0 { 24 } else { civil.hour },
                    _ => civil.hour,
                };
                ("hour", digits(hour))
            }
            "minute" => ("minute", digits(civil.min)),
            "second" => ("second", digits(civil.sec)),
            "era" => ("era", string_at(self.eras[width][usize::from(before_common_era)]).to_string()),
            "dayPeriod" => ("dayPeriod", self.day_period(civil).to_string()),
            "dayPeriodFlex" => (
                "dayPeriod",
                self.flexible_day_period(argument, civil).unwrap_or(self.day_period(civil)).to_string(),
            ),
            "fraction" => {
                let digits: usize = argument.parse().unwrap_or(3);
                ("fractionalSecond", format!("{milliseconds:03}")[..digits.clamp(1, 3)].to_string())
            }
            _ => ("timeZoneName", zone(argument)),
        };
        (name.to_string(), value)
    }
}

/// Os dados da chave exata \`key\`: uma língua (\`es\`) ou \`língua-REGIÃO\` (\`es-MX\`).
pub fn locale_data(key: &str) -> Option<&'static LocaleData> {
    Some(match key {
${LOCALES.map((l) => `        "${l}" => &${staticName(l)},`).join("\n")}
        _ => return None,
    })
}

/// Os dados de um tag BCP 47: primeiro \`língua-REGIÃO\` (a região é o primeiro subtag de duas letras, depois de um
/// script opcional), depois a língua sozinha. Extensões (\`-u-\`, \`-x-\`) são ignoradas.
pub fn locale_data_for(tag: &str) -> Option<&'static LocaleData> {
    let mut subtags = tag.split('-');
    let language = subtags.next()?;
    let region = subtags
        .take_while(|subtag| subtag.len() > 1)
        .find(|subtag| subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()));
    if let Some(region) = region {
        let key = format!("{language}-{}", region.to_ascii_uppercase());
        if let Some(data) = locale_data(&key) {
            return Some(data);
        }
    }
    locale_data(language)
}
`;

// Os programas do golden (ASCII de uma linha); o REPR sai do harness comum, avaliado no bun.
const programs = [];
const quoteJs = (value) => JSON.stringify(value);
const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

// Formato compacto: cada texto único (chaves, padrões, nomes, valores avulsos) vive uma vez em `STRINGS`, e as
// tabelas de cada locale são índices u16 nele; os mais frequentes ganham os índices menores. As linhas saem
// ordenadas pela chave, para a busca binária de `value_of`.
const counts = new Map();
const emitters = [];
for (const locale of LOCALES) {
  const data = tables(locale);
  const patternsOfLocale = [];
  const index = new Map();
  const entries = [];
  const keys = new Set();
  let skipped = 0;
  for (const options of skeletons) {
    const key = keyOf(options);
    if (keys.has(key)) continue;
    const text = pattern(locale, data, options, AM);
    const evening = pattern(locale, data, options, PM);
    if (text === null || evening === null) {
      skipped++;
      continue;
    }
    keys.add(key);
    if (!index.has(text)) {
      index.set(text, patternsOfLocale.length);
      patternsOfLocale.push(text);
    }
    entries.push([key, index.get(text)]);
  }
  entries.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  const name = staticName(locale);
  const extraRows = extras(locale);
  for (const [key, text] of [...entries.map(([k, position]) => [k, patternsOfLocale[position]]), ...extraRows]) {
    if (!/^[\x20-\x7e]+$/.test(key)) throw new Error(`${locale}: chave não ASCII ${key}`);
    for (const used of [key, text]) counts.set(used, (counts.get(used) ?? 0) + 1);
  }
  for (const text of [...data.months.format.flat(), ...data.months.standalone.flat(), ...data.weekdays.format.flat(), ...data.weekdays.standalone.flat(), ...data.eras.flat(), ...data.dayPeriods, ...data.zone]) {
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }
  emitters.push(() => `
static ${name}: LocaleData = LocaleData {
    default_cycle: ${rs(data.defaultCycle)},
    hour12_true_cycle: ${rs(data.hour12True)},
    hour12_false_cycle: ${rs(data.hour12False)},
    months_format: ${idxNested(data.months.format)},
    months_standalone: ${idxNested(data.months.standalone)},
    weekdays_format: ${idxNested(data.weekdays.format)},
    weekdays_standalone: ${idxNested(data.weekdays.standalone)},
    eras: ${idxNested(data.eras)},
    day_periods: ${idxArray(data.dayPeriods)},
    utc_zone: ${idxArray(data.zone)},
    entries: &[
${chunked(entries.map(([key, position]) => `(${poolIndex.get(key)},${poolIndex.get(patternsOfLocale[position])})`), 12, "        ")}
    ],
    extras: &[
${chunked(extraRows.map(([key, value]) => `(${poolIndex.get(key)},${poolIndex.get(value)})`), 12, "        ")}
    ],
};
`);
  console.error(`${locale}: ${entries.length} padrões, ${patternsOfLocale.length} distintos, ${skipped} sem token`);

  // Golden: o que o bun formata, por format e formatToParts, para um subconjunto dos skeletons.
  const chosen = skeletons.filter((options, position) => keyOf(options) && (position % 7 === 0 || options.dateStyle || options.timeStyle));
  const seen = new Set();
  for (const options of chosen) {
    const key = keyOf(options);
    if (seen.has(key) || !keys.has(key)) continue;
    seen.add(key);
    const { __cycle, ...rest } = options;
    const config = { timeZone: "UTC", ...FORCED[locale], ...rest };
    if (__cycle && rest.hour !== undefined) config.hourCycle = __cycle;
    if (__cycle && rest.timeStyle !== undefined) config.hourCycle = __cycle;
    const base = `new Intl.DateTimeFormat(${quoteJs(locale)}, ${JSON.stringify(config)})`;
    for (const time of [AM, PM]) programs.push(`${base}.format(${time})`);
    programs.push(`${base}.formatToParts(${AM}).map(function (p) { return p.type + ":" + p.value }).join("|")`);
  }
  // Ciclo de horas e toLocaleString padrão.
  for (const options of [{}, { hour12: true }, { hour12: false }]) {
    const config = JSON.stringify({ timeZone: "UTC", ...FORCED[locale], hour: "numeric", ...options });
    programs.push(`new Intl.DateTimeFormat(${quoteJs(locale)}, ${config}).resolvedOptions().hourCycle`);
  }
  programs.push(`new Date(${PM}).toLocaleString(${quoteJs(locale)}, {timeZone: "UTC"})`);
}

// Golden dos nomes de fuso, dayPeriod, fractionalSecondDigits e formatRange. As nove línguas da tabela entram
// no golden principal; en e pt (que usam os dados próprios) e formatRange (ainda sem a tabela) vão para
// `datetime_gaps_bun.tsv`, que nenhum teste lê até o porte cobrir o caso.
const gapPrograms = [];
const formatOf = (locale, config, time) => `new Intl.DateTimeFormat(${quoteJs(locale)}, ${JSON.stringify({ timeZone: "UTC", ...FORCED[locale], ...config })}).format(${time})`;
for (const locale of ALL_LOCALES) {
  const sink = LOCALES.includes(locale) ? programs : gapPrograms;
  for (const timeZone of goldenZones(locale)) {
    for (const timeZoneName of ZONE_STYLES) {
      for (const time of [WINTER, SUMMER]) sink.push(formatOf(locale, { timeZone, hour: "numeric", minute: "2-digit", timeZoneName }, time));
    }
  }
  // Fusos em que o tzdb tem DST negativo (o horário padrão do tzdb é o de maior deslocamento): o ICU chama de verão
  // o deslocamento maior (Dublin: `Irish Standard Time` em julho, `Greenwich Mean Time` em janeiro), então o porte
  // não pode escolher `w`/`s` pelo `isdst` do tzdb. Casablanca e El Aaiun só têm o GMT localizado, e o Ramadã
  // (março de 2024) muda o deslocamento deles.
  for (const timeZone of NEGATIVE_DST_ZONES) {
    for (const timeZoneName of ZONE_STYLES) {
      for (const time of [WINTER, SUMMER, RAMADAN]) sink.push(formatOf(locale, { timeZone, hour: "numeric", minute: "2-digit", timeZoneName }, time));
    }
  }
  for (const dayPeriod of DAY_PERIOD_WIDTHS) {
    for (const hour of [0, 3, 8, 12, 15, 19, 22]) {
      const time = Date.UTC(2024, 2, 5, hour, hour === 12 ? 0 : 30, 0);
      sink.push(formatOf(locale, { hour: "numeric", dayPeriod }, time));
      sink.push(formatOf(locale, { hour: "numeric", minute: "2-digit", dayPeriod }, time));
    }
  }
  for (const fractionalSecondDigits of [1, 2, 3]) {
    for (const config of [{ second: "numeric" }, { minute: "2-digit", second: "2-digit" }, { hour: "numeric", minute: "2-digit", second: "2-digit" }]) {
      sink.push(formatOf(locale, { ...config, fractionalSecondDigits }, AM + 123));
    }
  }
  // Só hora (e minuto) em dias diferentes, com e sem `hour12` e com a hora de dois dígitos: o intervalo é `format` com
  // `yMd` de cada ponta, e o separador (`range|days_*|pairsep`) vem do locale.
  const daysEnd = Date.UTC(2024, 2, 25, 19, 40, 0);
  const daysOptions = [{ hour: "numeric" }, { hour: "2-digit" }, { hour: "numeric", minute: "2-digit" }];
  const daysRanges = [];
  for (const base of daysOptions) for (const extra of [{}, { hour12: true }, { hour12: false }]) daysRanges.push({ options: { ...base, ...extra }, from: AM, to: daysEnd });
  for (const { options, from, to } of [...RANGES, ...daysRanges]) {
    const config = JSON.stringify({ timeZone: "UTC", ...FORCED[locale], ...options });
    gapPrograms.push(`new Intl.DateTimeFormat(${quoteJs(locale)}, ${config}).formatRange(${from}, ${to})`);
    gapPrograms.push(`new Intl.DateTimeFormat(${quoteJs(locale)}, ${config}).formatRangeToParts(${from}, ${to}).map(function (p) { return p.type + ":" + p.source + ":" + p.value }).join("|")`);
  }
}

const linesOf = (list) => {
  const lines = [];
  const seenPrograms = new Set();
  for (const program of list) {
    if (seenPrograms.has(program)) continue;
    seenPrograms.add(program);
    const out = (0, eval)(`${harness}(${JSON.stringify(program)})`);
    if (typeof out !== "string") throw new Error(`${program}: o harness não devolveu string`);
    lines.push(`${program}\t${out}`);
  }
  return lines;
};
const lines = linesOf(programs);
const gapLines = linesOf(gapPrograms);

const pool = [...counts.keys()].sort((a, b) => counts.get(b) - counts.get(a) || (a < b ? -1 : a > b ? 1 : 0));
if (pool.length > 65536) throw new Error(`${pool.length} textos não cabem em u16`);
const poolIndex = new Map(pool.map((text, index) => [text, index]));
const chunked = (items, size, indent) => {
  const out = [];
  for (let i = 0; i < items.length; i += size) out.push(indent + items.slice(i, i + size).join(","));
  return out.join(",\n") + (items.length ? "," : "");
};
const idxArray = (list) => "[" + list.map((text) => poolIndex.get(text)).join(",") + "]";
const idxNested = (lists) => "[" + lists.map(idxArray).join(",") + "]";
for (const emit of emitters) source += emit();
source += `
/// Os textos únicos de todas as tabelas; as linhas guardam índices neste vetor.
static STRINGS: &[&str] = &[
${chunked(pool.map(rs), 8, "    ")}
];
`;
console.error(`pool: ${pool.length} textos únicos`);

writeRustSource(path.join(__dirname, "../src/runtime/intl_date_time_data.rs"), source);
fs.writeFileSync(path.join(__dirname, "../tests/golden/datetime_more_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
fs.writeFileSync(path.join(__dirname, "../tests/golden/datetime_gaps_bun.tsv"), require("./golden-prelude.js").assertPublicResult(gapLines.join("\n") + "\n"));
console.error(`golden: ${lines.length} linhas, lacunas: ${gapLines.length} linhas`);
