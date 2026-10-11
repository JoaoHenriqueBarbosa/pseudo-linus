// Gera src/runtime/intl_calendar_patterns.rs a partir do bun: o padrão que o ICU (udatpg_getBestPattern sobre os
// availableFormats do calendário e do locale, com a herança do gregoriano) escolhe para cada skeleton de data
// em cada calendário não gregoriano e em cada locale. É a versão guiada por dados do que antes era um caso
// especial (`day_first_text_months`): o porte não decide a ordem nem a pontuação, só renderiza o padrão medido.
//
// Como regenerar (da raiz da crate zjsc):
//   bun scripts/gen-calendar-patterns.js
//   GEN_CAL_LOCALES=en,pt,de,ja,es,fr,it,ru bun scripts/gen-calendar-patterns.js   (outro conjunto de locales)
//
// A tabela é densa (ver `slots` abaixo): um bloco de índices u16 por (locale, calendário), e a chave do skeleton
// (`campo=valor` na ordem weekday, era, year, month, day, dayPeriod, hour, minute, second, timeZoneName,
// dateStyle, timeStyle, e `|12` ou `|24` no fim quando há hora, o mesmo texto de `data_key` em
// `intl_date_time_format.rs`) vira o índice dentro do bloco. O padrão é uma sequência de literais e tokens// `{tipo:argumento}`: weekday, month, day, year, relatedYear, yearName e era (do calendário), e hour, minute,
// second, dayPeriod, dayPeriodFlex e tz (o vocabulário de `intl_date_time_data`, que o renderizador delega a ele).
// Só entram skeletons com data: a hora sozinha não depende do calendário.
const fs = require("fs");
const path = require("path");
const { writeRustSource } = require("./rust-escape.js");

// Os locales do porte para o DateTimeFormat: a lista de `gen-datetime-data.js` (o `LOCALES` de lá) mais `en` e `pt`.
// `GEN_CAL_LOCALES` substitui a lista (para um teste rápido).
function portLocales() {
  const source = fs.readFileSync(path.join(__dirname, "gen-datetime-data.js"), "utf8");
  const body = /const LOCALES = \[([\s\S]*?)\];/.exec(source)[1];
  return [...new Set(["en", "pt", ...[...body.matchAll(/"([A-Za-z-]+)"/g)].map((match) => match[1])])];
}
// Todos os locales do porte por padrão (a tabela é densa, sem poda); `GEN_CAL_LOCALES` troca a lista.
const LOCALES = process.env.GEN_CAL_LOCALES ? process.env.GEN_CAL_LOCALES.split(",") : portLocales();
// O nome do `resolvedOptions().calendar`, que é o que o porte tem em mãos. O gregoriano entra na mesma tabela:
// é o caminho de `calendar_parts` para o calendário padrão, sem tratamento à parte.
const CALENDARS = [
  "buddhist", "chinese", "coptic", "dangi", "ethiopic", "ethioaa", "hebrew", "indian",
  "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese", "persian", "roc", "gregory",
];
const FIELDS = ["weekday", "era", "year", "month", "day", "dayPeriod", "hour", "minute", "second", "timeZoneName", "dateStyle", "timeStyle"];
const WIDTHS = ["long", "short", "narrow"];

const make = (locale, calendar, options) =>
  new Intl.DateTimeFormat(`${locale}-u-ca-${calendar}-nu-latn`, { timeZone: "UTC", ...options });

const keyOf = (options) => {
  const fields = FIELDS.filter((name) => options[name] !== undefined).map((name) => `${name}=${options[name]}`).join(";");
  return options.__cycle ? `${fields}|${options.__cycle === "h12" ? "12" : "24"}` : fields;
};

// Um dia com dia e mês de um dígito no calendário (senão `2-digit` e `numeric` não se distinguem).
function sampleDay(calendar) {
  for (let offset = 0; offset < 400; offset++) {
    const time = Date.UTC(2024, 2, 1) + offset * 86400000;
    const parts = make("pt", calendar, { year: "numeric", month: "numeric", day: "numeric" }).formatToParts(time);
    const get = (type) => parts.find((part) => part.type === type)?.value;
    const day = get("day");
    const month = get("month");
    if (/^\d$/.test(day) || /^0\d$/.test(day)) {
      if (/^0?\d$/.test(month) && Number(day) < 10 && Number(month) < 10) return time;
    }
  }
  throw new Error(`sem dia de amostra para ${calendar}`);
}

// A tabela é densa: cada (locale, calendário) tem um bloco de SLOTS entradas u16, e o skeleton escolhe a entrada
// pelo índice (`slots` abaixo, a mesma ordem que `slot` em Rust lê da chave). Ordem dos slots:
//   1. a grade exaustiva dos campos de data: weekday(4) x era(4) x year(3) x month(6) x day(3) menos o vazio =
//      863, em base mista (o último campo varia mais rápido), sem o zero (ver notes/calendar-skeleton-coverage.md);
//   2. dateStyle sozinho (4);
//   3. por ciclo de hora (12, 24): cada conjunto de data x cada conjunto de hora, depois dateStyle x timeStyle.
const GRID = [
  ["weekday", [undefined, "narrow", "short", "long"]],
  ["era", [undefined, "narrow", "short", "long"]],
  ["year", [undefined, "numeric", "2-digit"]],
  ["month", [undefined, "numeric", "2-digit", "narrow", "short", "long"]],
  ["day", [undefined, "numeric", "2-digit"]],
];
const STYLES = ["full", "long", "medium", "short"];
const slots = [];
(function addGrid(depth, options) {
  if (depth === GRID.length) {
    if (Object.keys(options).length) slots.push(options);
    return;
  }
  const [name, values] = GRID[depth];
  for (const value of values) addGrid(depth + 1, value === undefined ? options : { ...options, [name]: value });
})(0, {});
for (const dateStyle of STYLES) slots.push({ dateStyle });

// Data com hora. Um conjunto de datas e um de horas, nos dois ciclos (12 e 24 horas).
const DATE_SETS = [
  { year: "numeric", month: "numeric", day: "numeric" },
  { year: "numeric", month: "short", day: "numeric" },
  { year: "numeric", month: "long", day: "numeric" },
  { weekday: "long", year: "numeric", month: "long", day: "numeric" },
  { weekday: "short", year: "numeric", month: "short", day: "numeric" },
  { month: "numeric", day: "numeric" },
  { month: "long", day: "numeric" },
  { year: "2-digit", month: "2-digit", day: "2-digit" },
  { year: "numeric", month: "long" },
];
const TIME_SETS = [];
for (const hour of ["numeric", "2-digit"]) {
  TIME_SETS.push({ hour });
  TIME_SETS.push({ hour, minute: "2-digit" });
  TIME_SETS.push({ hour, minute: "2-digit", second: "2-digit" });
}
TIME_SETS.push({ hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "short" });
TIME_SETS.push({ hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "long" });
TIME_SETS.push({ hour: "numeric", minute: "2-digit", timeZoneName: "short" });
TIME_SETS.push({ dayPeriod: "short", hour: "numeric", minute: "2-digit" });
for (const __cycle of ["h12", "h23"]) {
  for (const date of DATE_SETS) for (const time of TIME_SETS) slots.push({ ...date, ...time, __cycle });
  for (const dateStyle of STYLES) {
    for (const timeStyle of STYLES) slots.push({ dateStyle, timeStyle, __cycle });
  }
}

const formDiffCache = new Map();
/** Um instante em que o nome do mês em `d MMMM y` difere do de `d MMMM` (com as duas formas), ou `null`. */
function formDiffTime(locale, calendar, width) {
  const key = `${locale}|${calendar}|${width}`;
  if (!formDiffCache.has(key)) {
    const monthOf = (options, time) => make(locale, calendar, options).formatToParts(time).find((part) => part.type === "month")?.value;
    let found = null;
    for (let offset = 0; offset < 400 && found === null; offset += 7) {
      const time = Date.UTC(2024, 0, 1) + offset * 86400000;
      const dayForm = monthOf({ month: width, day: "numeric" }, time);
      const yearForm = monthOf({ month: width, day: "numeric", year: "numeric" }, time);
      if (dayForm !== yearForm && !/^\d/.test(yearForm)) found = { time, dayForm, yearForm };
    }
    formDiffCache.set(key, found);
  }
  return formDiffCache.get(key);
}

/** O nome do mês (a parte `month`) de `options` no dia 5 de cada mês de 2024, ao meio-dia UTC. */
function monthNamesOf(locale, calendar, options) {
  const format = make(locale, calendar, options);
  return Array.from({ length: 12 }, (_, month) => format.formatToParts(Date.UTC(2024, month, 5, 12)).find((part) => part.type === "month")?.value);
}

const standaloneCache = new Map();
/** `true` quando o formatador de `options` escreve o mês na forma isolada nos doze meses, e não na de formato. */
function usesStandaloneMonth(locale, calendar, options, cycle, width) {
  const { __cycle, ...rest } = options;
  const key = `${locale}|${keyOf(options)}`;
  if (!standaloneCache.has(key)) {
    const actual = monthNamesOf(locale, calendar, cycle ? { ...rest, hourCycle: cycle } : rest);
    const standalone = monthNamesOf(locale, calendar, { month: width });
    const format = monthNamesOf(locale, calendar, { month: width, day: "numeric" });
    const same = (names) => names.every((name, index) => name === actual[index]);
    standaloneCache.set(key, same(standalone) && !same(format));
  }
  return standaloneCache.get(key);
}

/**
 * A largura do nome do mês `value` num padrão sem `month` pedido (`dateStyle`). Em março "mars" é igual em `long` e
 * `short` (fr, ko, zh...), então o nome de um só instante não decide: a largura certa é a que bate com o formatador em
 * doze instantes distintos, contra o nome isolado, o com dia e o com dia e ano. Sem largura que bata em todos, cai na
 * regra do instante único.
 */
function monthWidth(format, locale, calendar, time, value, nameOf) {
  const monthOf = (instance, at) => instance.formatToParts(at).find((part) => part.type === "month")?.value;
  const instants = Array.from({ length: 12 }, (_, step) => time + Math.round(step * 29.53 * 86400000));
  const actual = instants.map((at) => monthOf(format, at));
  const forms = [{}, { day: "numeric" }, { day: "numeric", year: "numeric" }];
  for (const width of ["long", "short", "narrow"]) {
    for (const form of forms) {
      const reference = make(locale, calendar, { month: width, ...form });
      if (instants.every((at, index) => monthOf(reference, at) === actual[index])) return width;
    }
  }
  return ["short", "long", "narrow"].find((w) => nameOf("month", w) === value);
}

const weekdayCache = new Map();
/**
 * `true` quando o formatador de `options` escreve o dia da semana na forma isolada nos sete dias, e não na de formato.
 * O skeleton só com o dia da semana (`E`) vira `ccc` no ICU (isolada): em en-AU o `narrow` sozinho é `T`, mas ao lado
 * do dia é `Tu.`. Mede nos sete dias para não confundir quando as duas formas coincidem no dia amostrado.
 */
function usesStandaloneWeekday(locale, calendar, options, cycle, width) {
  const { __cycle, ...rest } = options;
  const key = `${locale}|${calendar}|${keyOf(options)}`;
  if (!weekdayCache.has(key)) {
    const namesOf = (shape) => {
      const format = make(locale, calendar, shape);
      return Array.from({ length: 7 }, (_, day) => format.formatToParts(Date.UTC(2024, 2, 3 + day, 12)).find((part) => part.type === "weekday")?.value);
    };
    const actual = namesOf(cycle ? { ...rest, hourCycle: cycle } : rest);
    const same = (names) => names.every((name, index) => name === actual[index]);
    weekdayCache.set(key, same(namesOf({ weekday: width })) && !same(namesOf({ weekday: width, day: "numeric", month: "long" })));
  }
  return weekdayCache.get(key);
}

/** O padrão de `options` no instante `time`, ou `null` se alguma parte não tem token. */
function pattern(locale, calendar, options, time, ampm) {
  const { __cycle, ...rest } = options;
  const format = make(locale, calendar, __cycle ? { ...rest, hourCycle: __cycle } : rest);
  const zoneStyle = options.timeZoneName ?? { full: "long", long: "short" }[options.timeStyle];
  const parts = format.formatToParts(time);
  const nameOf = (kind, width) => {
    const probe = kind === "month" ? { month: width } : { era: width, year: "numeric" };
    const found = make(locale, calendar, probe).formatToParts(time).find((part) => part.type === kind);
    return found && found.value;
  };
  let out = "";
  for (const { type, value } of parts) {
    if (type === "literal") {
      if (/[{}]/.test(value)) return null;
      out += value;
    } else if (type === "weekday") {
      const width = options.weekday ?? "long";
      const tag = WIDTHS.includes(options.weekday) && options.dateStyle === undefined && usesStandaloneWeekday(locale, calendar, options, __cycle, width) ? ":s" : "";
      out += `{weekday:${width}${tag}}`;
    } else if (type === "month") {
      // Um nome que começa em dígito mas tem sufixo (ko `3월` com `dateStyle`/`month: "long"`) é nome, não número: o
      // sufixo `월` vem na parte `month`, então o token numérico o perderia.
      const numericPart = /^\d+$/.test(value) || (/^\d/.test(value) && (options.month === "numeric" || options.month === "2-digit"));
      if (numericPart) out += `{month:${value.length >= 2 && value.startsWith("0") ? "2-digit" : "numeric"}}`;
      else {
        const width = WIDTHS.includes(options.month) ? options.month : monthWidth(format, locale, calendar, time, value, nameOf);
        if (!width) return null;
        // O nome do mês junto do ano pode diferir do nome com dia (`fa`: `مهٔ` em `d MMMM y`, `مه` em `d MMMM`),
        // só em alguns meses: o terceiro campo `year` manda ler a forma `m?y` da tabela de nomes (ver
        // `gen-calendar-golden.js names`). Mede num mês em que as duas formas diferem, com as mesmas opções.
        const probe = formDiffTime(locale, calendar, width);
        let tag = "";
        if (probe !== null) {
          const { __cycle: cycle, ...restOptions } = options;
          const sameOptions = make(locale, calendar, cycle ? { ...restOptions, hourCycle: cycle } : restOptions);
          const at = sameOptions.formatToParts(probe.time).find((part) => part.type === "month")?.value;
          if (at === probe.yearForm && at !== probe.dayForm) tag = ":year";
        }
        // No gregoriano os nomes vêm de `intl_date_time_data`, que guarda a forma de formato e a isolada: o mês pedido
        // sozinho ou ao lado do dia da semana sai na isolada em de (`Nov`, não `Nov.`), em ru, pl... O token `:s` manda
        // ler a isolada. Mede nos doze meses, para não confundir quando as duas formas coincidem no mês amostrado.
        if (tag === "" && calendar === "gregory" && WIDTHS.includes(options.month) && options.dateStyle === undefined) {
          if (usesStandaloneMonth(locale, calendar, options, __cycle, width)) tag = ":s";
        }
        out += `{month:${width}${tag}}`;
      }
    } else if (type === "day") {
      out += `{day:${value.length >= 2 && value.startsWith("0") ? "2-digit" : "numeric"}}`;
    } else if (type === "year") {
      out += `{year:${value.length === 2 || (value.length >= 2 && value.startsWith("0")) ? "2-digit" : "numeric"}}`;
    } else if (type === "relatedYear") {
      out += `{relatedYear:numeric}`;
    } else if (type === "yearName") {
      out += "{yearName}";
    } else if (type === "era") {
      const width = WIDTHS.includes(options.era) ? options.era : ["short", "long", "narrow"].find((w) => nameOf("era", w) === value);
      if (!width) return null;
      out += `{era:${width}}`;
    } else if (type === "hour" || type === "minute" || type === "second") {
      out += `{${type}:${value.length >= 2 && value.startsWith("0") ? "2-digit" : "numeric"}}`;
    } else if (type === "dayPeriod") {
      if (options.dayPeriod !== undefined) out += `{dayPeriodFlex:${options.dayPeriod}}`;
      else if (ampm.includes(value)) out += "{dayPeriod}";
      else {
        // Período flexível (`B` do ICU: zh-TW `清晨`, `晚上`): o ICU o escolhe pela hora, não é AM/PM. A largura é a que
        // bate com o formatador de `dayPeriod` em duas horas distintas (manhã e noite); o renderizador lê a tabela
        // `dp|largura|hora|exato` do locale.
        const periodOf = (instance, at) => instance.formatToParts(at).find((part) => part.type === "dayPeriod")?.value;
        const instants = [time, time + 14 * 3600000];
        const flex = ["short", "long", "narrow"].find((width) => {
          const reference = make(locale, calendar, { hour: "numeric", dayPeriod: width, hourCycle: "h12" });
          return instants.every((at) => periodOf(reference, at) === periodOf(format, at));
        });
        if (!flex) return null;
        out += `{dayPeriodFlex:${flex}}`;
      }
    } else if (type === "timeZoneName") {
      if (zoneStyle === undefined) return null;
      out += `{tz:${zoneStyle}}`;
    } else return null;
  }
  return out;
}

const NONE = 0xffff;
const table = [];

const pool = [];
const poolIndex = new Map();
const indexOf = (text) => {
  if (!poolIndex.has(text)) {
    poolIndex.set(text, pool.length);
    pool.push(text);
  }
  return poolIndex.get(text);
};
// Um bloco de `slots.length` entradas por (locale, calendário), na ordem de `LOCALES` e `CALENDARS`; `NONE` é "sem padrão".
for (const locale of LOCALES) {
  for (const calendar of CALENDARS) {
    const day = sampleDay(calendar);
    const time = day + (5 * 3600 + 7 * 60 + 9) * 1000;
    const twelve = { hour: "numeric", hourCycle: "h12" };
    const ampm = [day, day + 12 * 3600000].map((at) => make(locale, calendar, twelve).formatToParts(at).find((part) => part.type === "dayPeriod")?.value);
    for (const options of slots) {
      const text = pattern(locale, calendar, options, time, ampm);
      table.push(text === null ? NONE : indexOf(text));
    }
  }
}
if (pool.length >= NONE) throw new Error(`${pool.length} padrões não cabem em u16`);
const quote = (text) => JSON.stringify(text);
const list = (values) => `[${values.map(quote).join(", ")}]`;
// Blocos iguais (locale filho igual ao pai, calendários sem diferença) viram um só: `blocks` guarda os únicos e
// `blockOf` tem, por (locale, calendário), o índice do bloco.
const blockIndex = new Map();
const blocks = [];
const blockOf = [];
for (let at = 0; at < table.length; at += slots.length) {
  const block = table.slice(at, at + slots.length);
  const text = block.join(",");
  if (!blockIndex.has(text)) {
    blockIndex.set(text, blocks.length);
    blocks.push(block);
  }
  blockOf.push(blockIndex.get(text));
}
if (blocks.length >= NONE) throw new Error(`${blocks.length} blocos não cabem em u16`);
const uniqueTable = blocks.flat();
const tableLines = [];
for (let at = 0; at < uniqueTable.length; at += 24) tableLines.push(`    ${uniqueTable.slice(at, at + 24).join(", ")},`);
const blockLines = [];
for (let at = 0; at < blockOf.length; at += 24) blockLines.push(`    ${blockOf.slice(at, at + 24).join(", ")},`);

// Os conjuntos de data e de hora do bloco com hora, no texto da chave (`keyOf`), para o `slot` de Rust casar a chave.
const dateKeys = DATE_SETS.map((options) => keyOf(options));
const timeKeys = TIME_SETS.map((options) => keyOf(options));
const perCycle = DATE_SETS.length * TIME_SETS.length + STYLES.length * STYLES.length;
const gridSize = slots.findIndex((options) => options.dateStyle !== undefined);

const source = `//! Padrões de data dos calendários não gregorianos do \`Intl.DateTimeFormat\`, por locale e skeleton.
//!
//! GERADO por \`scripts/gen-calendar-patterns.js\`: não edite à mão. Para regenerar, da raiz da crate:
//! \`bun scripts/gen-calendar-patterns.js\`. O padrão é o que o ICU escolheu no bun para o skeleton, com tokens
//! \`{tipo:argumento}\` que \`intl_calendar::render_pattern\` preenche.
//!
//! Formato denso: \`PATTERNS\` guarda cada padrão uma vez, \`TABLE\` os blocos distintos de \`SLOTS\` índices \`u16\`
//! (\`NONE\` = sem padrão) e \`BLOCK_OF\` o bloco de cada (locale, calendário). O slot sai da chave do skeleton (\`slot\`): a grade de campos
//! de data em base mista, \`dateStyle\`, e por ciclo de hora os conjuntos de data x hora e \`dateStyle\` x \`timeStyle\`.

/// Padrões distintos; \`TABLE\` guarda o índice.
static PATTERNS: [&str; ${pool.length}] = [
${pool.map((text) => `    ${quote(text)},`).join("\n")}
];

/// Entrada de \`TABLE\` sem padrão.
const NONE: u16 = ${NONE};

/// Os locales de que a tabela tem blocos.
pub const LOCALES: [&str; ${LOCALES.length}] = ${list(LOCALES)};

/// Os calendários da tabela (o \`resolvedOptions().calendar\`).
static CALENDARS: [&str; ${CALENDARS.length}] = ${list(CALENDARS)};

/// Os campos de data da grade, da variação lenta para a rápida, com os valores (o vazio é o índice 0).
static GRID: [(&str, &[&str]); ${GRID.length}] = [
${GRID.map(([name, values]) => `    (${quote(name)}, &${list(values.filter((value) => value !== undefined))}),`).join("\n")}
];

/// Os estilos de \`dateStyle\` e de \`timeStyle\`.
static STYLES: [&str; ${STYLES.length}] = ${list(STYLES)};

/// Os conjuntos de data e de hora do bloco com hora, no texto da chave.
static DATE_SETS: [&str; ${dateKeys.length}] = ${list(dateKeys)};
static TIME_SETS: [&str; ${timeKeys.length}] = ${list(timeKeys)};

/// Slots da grade de data (sem o zero), de um ciclo de hora e o total por bloco.
const GRID_SLOTS: usize = ${gridSize};
const PER_CYCLE: usize = ${perCycle};
const SLOTS: usize = ${slots.length};

/// Para cada (locale, calendário), na ordem de \`LOCALES\` x \`CALENDARS\`, o índice do bloco em \`TABLE\`.
static BLOCK_OF: [u16; ${blockOf.length}] = [
${blockLines.join("\n")}
];

/// Os ${blocks.length} blocos distintos de \`SLOTS\` índices cada, um depois do outro.
static TABLE: [u16; ${uniqueTable.length}] = [
${tableLines.join("\n")}
];

fn position(list: &[&str], value: &str) -> Option<usize> {
    list.iter().position(|item| *item == value)
}

/// O slot da chave \`key\` (o texto de \`data_key\`: \`campo=valor;...\` e \`|12\` ou \`|24\` quando há hora), ou \`None\`
/// quando o skeleton não é um dos tabelados.
fn slot(key: &str) -> Option<usize> {
    let (fields, cycle) = match key.rsplit_once('|') {
        Some((fields, "12")) => (fields, Some(0)),
        Some((fields, "24")) => (fields, Some(1)),
        Some(_) => return None,
        None => (key, None),
    };
    match cycle {
        None => match fields.strip_prefix("dateStyle=") {
            Some(style) => Some(GRID_SLOTS + position(&STYLES, style)?),
            None => grid_slot(fields),
        },
        Some(cycle) => {
            let base = GRID_SLOTS + STYLES.len() + cycle * PER_CYCLE;
            if let Some(styles) = fields.strip_prefix("dateStyle=") {
                let (date, time) = styles.split_once(";timeStyle=")?;
                return Some(base + DATE_SETS.len() * TIME_SETS.len() + position(&STYLES, date)? * STYLES.len() + position(&STYLES, time)?);
            }
            DATE_SETS.iter().enumerate().find_map(|(date, set)| {
                let time = fields.strip_prefix(set)?.strip_prefix(';')?;
                Some(base + date * TIME_SETS.len() + position(&TIME_SETS, time)?)
            })
        }
    }
}

/// O slot de uma chave só de campos de data: a base mista dos campos da \`GRID\`, menos o vazio.
fn grid_slot(fields: &str) -> Option<usize> {
    let mut digits = [0usize; GRID.len()];
    let mut next = 0;
    for part in fields.split(';') {
        let (name, value) = part.split_once('=')?;
        let field = (next..GRID.len()).find(|&field| GRID[field].0 == name)?;
        digits[field] = 1 + position(GRID[field].1, value)?;
        next = field + 1;
    }
    let index = digits.iter().zip(GRID.iter()).fold(0, |acc, (digit, (_, values))| acc * (values.len() + 1) + digit);
    index.checked_sub(1)
}

/// O padrão do skeleton \`key\` no calendário \`calendar\` do locale exato \`locale\` (\`en\`, \`pt\`, \`de\`...).
pub fn pattern(locale: &str, calendar: &str, key: &str) -> Option<&'static str> {
    let entry = position(&LOCALES, locale)? * CALENDARS.len() + position(&CALENDARS, calendar)?;
    match TABLE[usize::from(BLOCK_OF[entry]) * SLOTS + slot(key)?] {
        NONE => None,
        index => Some(PATTERNS[usize::from(index)]),
    }
}
`;
writeRustSource(path.join(__dirname, "../src/runtime/intl_calendar_patterns.rs"), source);
console.log(`${table.length} entradas, ${pool.length} padrões, ${slots.length} slots por bloco, ${blockOf.length} blocos de locale x calendário, ${blocks.length} únicos`);
