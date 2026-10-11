// Gera tests/golden/intl_calendar_format_bun.tsv (e intl_calendar_format.preludes.json): a saída formatada de
// `Intl.DateTimeFormat` (format e formatToParts) em 14 calendários × 6 locales (en, pt, de, ja, es, fr) para datas
// fixas (viradas de ano em cada calendário, ano bissexto hebraico, mês bissexto chinês, trocas de era japonesa) e um
// conjunto de opções (skeletons de data, hora 12/24, timeZoneName, dateStyle e timeStyle). É o golden da saída dos
// padrões medidos por `scripts/gen-calendar-patterns.js` e consumidos por `native_pattern_parts`.
// Rodar no bun (o fuso fixo America/Sao_Paulo vai no filho): `bun scripts/gen-intl-calendar-format-golden.js`.
// Cada linha segue o formato fatorado de `scripts/golden-prelude.js`: JSON(sufixo) TAB JSON(texto de R).
const { spawnSync } = require("child_process");
const { emitFactored, GOLDEN_DIR } = require("./golden-prelude.js");

const NAME = "intl_calendar_format";
const ZONE = "America/Sao_Paulo";
const LOCALES = ["en", "pt", "de", "ja", "es", "fr"];
const CALENDARS = [
  "buddhist", "chinese", "coptic", "dangi", "ethiopic", "ethioaa", "hebrew", "indian",
  "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese", "persian", "roc",
];

// Datas em horário de Brasília (-03:00); as anteriores a 2019 passam pelo horário de verão do fuso, que o bun aplica.
const DATES = [
  "2024-03-15T13:45:30-03:00", "2024-12-31T23:59:59-03:00", "2025-01-01T00:00:00-03:00", "2023-12-31T23:59:59-03:00",
  "2024-01-01T00:00:00-03:00", "2024-02-09T23:59:59-03:00", "2024-02-10T00:00:00-03:00", "2023-01-22T12:00:00-03:00",
  // Mês bissexto chinês (segundo mês de 2023) e o ano bissexto hebraico 5784 (Adar I e Adar II).
  "2023-04-01T09:05:07-03:00", "2024-02-20T18:30:00-03:00", "2024-03-15T00:00:00-03:00", "2023-09-15T23:00:00-03:00",
  "2023-09-16T00:00:00-03:00", "2024-10-01T23:59:59-03:00", "2024-10-02T00:00:00-03:00",
  // Viradas de ano islâmica, persa, copta e etíope, indiana.
  "2024-07-06T23:59:59-03:00", "2024-07-07T10:00:00-03:00", "2024-03-19T23:59:59-03:00", "2024-03-20T00:00:00-03:00",
  "2023-09-11T23:59:59-03:00", "2023-09-12T00:00:00-03:00", "2024-03-20T23:59:59-03:00", "2024-03-21T00:00:00-03:00",
  // Trocas de era japonesa (Reiwa, Heisei, Showa, Taisho) e extremos do calendário.
  "2019-04-30T23:59:59-03:00", "2019-05-01T00:00:00-03:00", "1989-01-07T12:00:00-02:00", "1989-01-08T12:00:00-02:00",
  "1926-12-24T12:00:00-03:00", "1926-12-25T12:00:00-03:00", "2000-02-29T12:00:00-03:00", "1900-01-01T00:00:00-03:00",
  "1970-01-01T00:00:00-03:00", "2100-12-31T23:59:59-03:00", "1912-01-01T12:00:00-03:00", "1912-02-01T12:00:00-03:00",
];

const OPTIONS = [
  { year: "numeric", month: "numeric", day: "numeric" },
  { year: "numeric", month: "long", day: "numeric" },
  { weekday: "long", year: "numeric", month: "short", day: "numeric" },
  { year: "numeric", month: "long" },
  { month: "short", day: "numeric" },
  { year: "2-digit", month: "2-digit", day: "2-digit" },
  { era: "short", year: "numeric", month: "numeric", day: "numeric" },
  { year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "2-digit", hour12: true },
  { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" },
  { year: "numeric", month: "long", day: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "short" },
  { dateStyle: "full" },
  { dateStyle: "long" },
  { dateStyle: "medium" },
  { dateStyle: "short" },
  { timeStyle: "short" },
  { dateStyle: "full", timeStyle: "long" },
  { dateStyle: "medium", timeStyle: "short" },
  { dateStyle: "short", timeStyle: "medium" },
  { weekday: "short", year: "numeric", month: "narrow", day: "numeric", timeZoneName: "long", timeZone: "UTC" },
  { year: "numeric", month: "long", day: "numeric", hour: "numeric", timeZone: "Asia/Tokyo", timeZoneName: "shortOffset" },
  { year: "numeric" },
  { month: "long" },
  { weekday: "long" },
  { year: "numeric", month: "short", day: "numeric", hour: "numeric", hour12: false },
];

// As opções que medem as viradas de ano: uma só de data, uma com dia da semana e era, e duas com hora.
const BOUNDARY_OPTIONS = [1, 6, 9, 10];

const PRELUDE = `var R;
function G(l, c, o, t) { try { var f = new Intl.DateTimeFormat(l + "-u-ca-" + c, o); var d = new Date(t); return f.format(d) + " # " + f.formatToParts(d).map(function (p) { return p.type + "=" + p.value; }).join("|"); } catch (e) { return e.name + ": " + e.message; } }
`;

function child() {
  const rows = [];
  const times = DATES.map((iso) => Date.parse(iso));
  if (times.some(Number.isNaN)) throw new Error("data inválida");
  const add = (locale, calendar, options, time) => {
    const source = PRELUDE + `R = G(${JSON.stringify(locale)}, ${JSON.stringify(calendar)}, ${JSON.stringify(options)}, ${time});\n`;
    let result;
    (0, eval)(source);
    result = globalThis.R;
    rows.push({ source, result });
  };
  let combo = 0;
  for (const calendar of CALENDARS) {
    for (const locale of LOCALES) {
      // Cada opção em duas datas rotativas (o rodízio cobre todas as datas ao longo do conjunto).
      OPTIONS.forEach((options, optionIndex) => {
        for (let k = 0; k < 2; k++) add(locale, calendar, options, times[(combo * 7 + optionIndex * 3 + k * 11) % times.length]);
      });
      // Todas as datas, com uma das opções de virada de ano.
      times.forEach((time, dateIndex) => add(locale, calendar, OPTIONS[BOUNDARY_OPTIONS[(dateIndex + combo) % BOUNDARY_OPTIONS.length]], time));
      combo++;
    }
  }
  process.stdout.write(JSON.stringify(rows));
}

if (process.argv[2] === "--child") {
  child();
} else {
  const run = spawnSync(process.execPath, [__filename, "--child"], { env: { ...process.env, TZ: ZONE }, encoding: "utf8", maxBuffer: 1 << 30 });
  if (run.status !== 0) throw new Error(run.stderr);
  const rows = JSON.parse(run.stdout);
  require("fs").writeFileSync(`${GOLDEN_DIR}/${NAME}_bun.tsv`, emitFactored(NAME, rows));
  console.error(`${rows.length} casos`);
}
