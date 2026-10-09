// Gera tests/golden/date_parse_v8_bun.tsv: o parser de data do V8 que o bun usa (`Date.parse` e `new Date(string)`)
// numa grade grande de cadeias (ISO completo e parcial, legado, fusos nomeados, parênteses, espaços, lixo, não ASCII),
// medido no bun 1.4.2 em dois fusos (UTC e America/Sao_Paulo) por subprocesso com `TZ` fixa.
// Colunas: fuso, JSON da cadeia (só ASCII, com \uXXXX), resultado `P=<Date.parse>|N=<new Date(s).getTime()>|I=<toISOString ou ->`.
// Uso: bun scripts/gen-date-parse-v8-golden.js > tests/golden/date_parse_v8_bun.tsv
const { spawnSync } = require("child_process");
const { sampleByHash } = require("./golden-prelude.js");

const ZONES = ["UTC", "America/Sao_Paulo"];

function ascii(s) {
  return JSON.stringify(s).replace(/[\u007f-￿]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

// PRNG determinístico (mulberry32) que só GERA a coleção de candidatos da parte F; quem escolhe entre os candidatos é
// `sampleByHash`, pelo texto da cadeia, nunca a posição nem o fluxo do PRNG.
function makeRandom(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function buildCases() {
  const cases = new Set();
  const add = (...list) => list.forEach((s) => cases.add(s));

  // ---- A. ISO: data x hora x zona.
  const isoDates = [
    "2024", "2024-01", "2024-02", "2024-12", "2024-13", "2024-00", "2024-01-15", "2024-02-29", "2023-02-29", "2024-02-30", "2024-04-31",
    "2024-01-00", "2024-01-32", "2024-1-5", "2024-01-5", "1970-01-01", "1969-12-31", "0000-01-01", "0001-01-01", "9999-12-31", "10000-01-01",
    "+002024", "+002024-01", "+002024-01-15", "-000001-01-01", "-000000-01-01", "+000000-01-01", "-002024-06-15", "+275760-09-13", "+275760-09-14",
    "-271821-04-20", "-271821-04-19", "+010000-01-01", "+99999-01-01", "+0020241-01-01", "24-01-15", "20240115", "2024-01-15x", "+2024-01-15",
  ];
  const isoTimes = [
    "", "T00", "T10", "T10:20", "T10:20:30", "T00:00:00", "T23:59:59", "T23:59:59.999", "T24:00", "T24:00:00", "T24:00:00.000", "T24:00:00.1", "T24:01",
    "T25:00", "T10:60", "T10:20:60", "T1:02", "T10:2", "T10:20:3", "T10:20:30.", "T10:20:30,5", "T10:20:30.5", "T10:20:30.123", "T10:20:30.1234567890",
    "T 10:20", "T10:20 ", "T10 :20", "T-10:20", "T",
  ];
  const isoZones = ["", "Z", "z", "+00:00", "-00:00", "+01:00", "-05:30", "+0100", "-0530", "+01", "+1", "+24:00", "+23:59", "+99:99", "+01:60", "+01:00:00", " Z", " +01:00", "GMT", "UTC"];
  for (const d of isoDates) {
    for (const t of isoTimes) {
      if (t === "") {
        add(d, d + "Z", d + "+01:00", d + " ", " " + d, d + "T");
        continue;
      }
      // Fora do dia 2024-01-15, duas zonas por (data, hora), escolhidas por hash entre seis candidatas.
      const zs = d === "2024-01-15" ? isoZones : sampleByHash(["", "Z", "+01:00", "-0530", "+0100", "-00:00"], 2, (z) => d + t + z);
      for (const z of zs) add(d + t + z);
    }
  }
  // Fração de 1 a 9 (e 10..12) dígitos, em várias zonas.
  for (let n = 1; n <= 12; n++) {
    for (const z of ["", "Z", "+01:00", "-03:00"]) {
      const frac = "123456789012".slice(0, n);
      add(`2024-03-10T10:20:30.${frac}${z}`, `2024-03-10T10:20:30.${"9".repeat(n)}${z}`, `2024-03-10T10:20:30.${"0".repeat(n)}${z}`, `2024-03-10T10:20:30,${frac}${z}`);
    }
  }
  // Separador espaço no lugar do T, e minúsculo.
  for (const d of ["2024-01-15", "2024-01", "2024", "+002024-01-15"]) {
    for (const t of ["10:20", "10:20:30", "10:20:30.5", "24:00", "10:20:30.123456"]) {
      for (const z of ["", "Z", "+01:00", "-0530", " GMT", " UTC", " EST", " +0100"]) add(`${d} ${t}${z}`, `${d}t${t}${z}`);
    }
  }
  // Amostras ISO: todas as combinações de ano, mês, dia, hora e zona, 300 delas por hash da cadeia.
  const isoCandidates = [];
  for (const y of ["2024", "1999", "0100", "+002000", "-000500", "1970", "2038", "0000"]) {
    for (const mo of ["", "-01", "-06", "-12", "-00", "-13"]) {
      for (const da of mo ? ["", "-01", "-15", "-28", "-29", "-30", "-31", "-00", "-32"] : [""]) {
        for (const t of da ? isoTimes : [""]) {
          for (const z of t ? isoZones : [""]) isoCandidates.push(y + mo + da + t + z);
        }
      }
    }
  }
  add(...sampleByHash(isoCandidates, 300));

  // ---- B. Legado.
  const months = ["Jan", "January", "jan", "JAN", "january", "JANUARY", "Feb", "February", "Mar", "March", "Apr", "April", "May", "Jun", "June", "Jul", "July", "Aug",
    "August", "Sep", "Sept", "September", "Oct", "October", "Nov", "November", "Dec", "December", "Ja", "Janu", "Janx", "Janu.", "Foo", "Mayo", "Marc", "Decem", "Dece"];
  const days = ["Mon", "Monday", "mon", "MON", "monday", "Tue", "Tuesday", "Tues", "Wed", "Wednesday", "Thu", "Thursday", "Thur", "Fri", "Friday", "Sat", "Saturday", "Sun", "Sunday", "Mo", "Xyz", "Mon,", "Monday,"];
  for (const m of months) {
    add(`${m} 15 2024`, `${m} 15, 2024`, `15 ${m} 2024`, `15-${m}-2024`, `${m} 2024`, `${m} 15`, `${m}-15-2024`, `2024 ${m} 15`, `${m} 15 2024 10:20:30`,
      `${m} 15 2024 10:20:30 GMT`, `15 ${m} 2024 10:20:30 +0100`, `15 ${m} 2024 10:20 PM`, `Mon, 15 ${m} 2024 10:20:30 GMT`, `${m}. 15, 2024`, `${m}/15/2024`,
      `15/${m}/2024`, `${m} 31 2024`, `${m} 32 2024`, `${m} 0 2024`, `${m} 15 24`, `${m} 15 99`, `${m}15 2024`);
  }
  for (const d of days) {
    add(`${d}, 01 Jan 2024 10:00:00 GMT`, `${d} Jan 01 2024`, `${d} 01 Jan 2024`, `${d}, 01 Jan 2024`, `${d} Jan 01 2024 10:00:00 GMT+0100 (CET)`, `${d}`, `${d} 2024`, `${d} 10:00`);
  }
  // Ordem mês-dia-ano e variantes numéricas.
  const dmy = ["1", "01", "12", "13", "31", "32", "0", "00", "99", "2024", "24", "100", "1000", "10000"];
  // Cerca de 12% das trincas (a, b, c) entram, cada uma com a barra e uma combinação de separadores; a escolha é por hash.
  const dmyCandidates = [];
  for (const a of dmy) {
    for (const b of ["1", "12", "13", "31", "0", "2024", "99"]) {
      for (const c of ["2024", "24", "99", "00", "49", "50", "1", "0", "100", "12345", "-1"]) {
        for (const s1 of ["/", ".", "-", " "]) {
          for (const s2 of ["/", ".", "-", " "]) dmyCandidates.push([`${a}${s1}${b}${s2}${c}`, `${a}/${b}/${c}`]);
        }
      }
    }
  }
  for (const [first, second] of sampleByHash(dmyCandidates, Math.round(dmyCandidates.length * 0.12 / 16), (pair) => pair[0] + "\n" + pair[1])) add(first, second);
  add("1/1", "12/31", "13/1", "1/32", "2024/1", "1.1", "1-1", "1 1", "2024.01.15", "15.01.2024", "15.1.2024", "1.15.2024", "01/15/2024", "2024/01/15", "2024/1/5",
    "2024/13/01", "2024/01/32", "01/15/24", "01/15/99", "01/15/00", "01/15/49", "01/15/50", "1/2/3", "12/31/1999", "12/31/2000", "2/29/2024", "2/29/2023", "2/30/2024",
    "02/29/2000", "02/29/1900", "1/1/1970", "12/31/1969", "1/1/0", "1/1/-1", "1/1/+1", "1/1/100000", "1/1/275760", "1/1/275761", "9/13/275760", "9/14/275760",
    "4/20/-271821", "4/19/-271821", "1/1/-271821", "1/1/-271822");
  // Ano de 2 dígitos 00..99.
  for (let y = 0; y < 100; y++) {
    const yy = String(y).padStart(2, "0");
    add(`1/1/${yy}`, `Jan 1 ${yy}`, `${yy}-01-01`);
  }
  // Horas, AM/PM.
  const hms = ["0:00", "00:00", "1:00", "12:00", "12:30", "13:00", "23:59", "24:00", "24:01", "25:00", "99:00", "0:60", "0:59", "10:61", "10:20:60", "10:20:59", "10:20:61",
    "10:20:30.5", "10:20:30.123456", "10:20:30,5", "10:", "10:20:", ":20", "10::30", "10:20:30:40", "1:2:3", "001:002", "10", "10 20"];
  const meridiems = ["", " AM", " PM", " am", "PM", " A.M.", " a", " AMX"];
  for (const t of hms) {
    for (const mer of meridiems) {
      add(`Jan 1 2024 ${t}${mer}`, `1/1/2024 ${t}${mer}`);
    }
    add(`${t} Jan 1 2024`, `${t} 1/1/2024`, `Jan 1 ${t} 2024`, `Jan 1 2024 ${t} GMT`);
  }
  for (const h of [0, 1, 11, 12, 13, 23]) {
    for (const mer of [" AM", " PM", "am", "pm", " AM ", " PM GMT"]) {
      add(`Jan 1 2024 ${h}:00${mer}`, `Jan 1 2024 ${h}:30:15${mer}`, `Jan 1 2024 ${h}${mer}`, `1/1/2024 ${h}:00${mer}`);
    }
  }
  // Fusos nomeados e offsets soltos.
  const zones = ["GMT", "UT", "UTC", "Z", "z", "gmt", "utc", "ut", "EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT", "est", "pdt", "Pst", "XYZ", "A", "J", "CET", "BRT",
    "GMT+1", "GMT-1", "GMT+0100", "GMT-0530", "GMT+01:00", "GMT+1:30", "UTC+1", "UTC-0800", "UT+0100", "Z+0100", "+0100", "-0100", "+0530", "-0530", "+05:30", "-05:30", "+5", "-5",
    "+1", "+01", "+24", "+2400", "+9999", "-9999", "+99", "+0000", "-0000", "+00:00", "+0", "0100", "+01:0", "+01:", "+:30", "GMT+", "GMT-", "GMT 0100", "(PST)", "(CET)"];
  for (const z of zones) {
    add(`Jan 1 2024 10:00 ${z}`, `Jan 1 2024 10:00${z}`, `Mon, 01 Jan 2024 10:00:00 ${z}`, `1/1/2024 10:00 ${z}`, `Jan 1 2024 ${z}`, `${z} Jan 1 2024 10:00`,
      `Jan 1 2024 10:00:00.5 ${z}`, `Jan 1 2024 10:00 PM ${z}`, `2024-01-01T10:00:00 ${z}`, `2024-01-01 10:00 ${z}`, `2024-01-01 10:00${z}`, `Jan 1 2024 ${z} 10:00`);
  }
  // Parênteses e comentários.
  const comments = ["(comment)", "(a (nested) b)", "(", ")", "(unclosed", "()", "(PST)", "(Brasilia)", "( )", "(a)(b)", "((", "))", "(\\)", "(é)"];
  for (const c of comments) {
    add(`Jan 1 2024 ${c}`, `${c} Jan 1 2024`, `Jan ${c} 1 2024`, `Jan 1 2024 10:00 ${c}`, `Jan 1 2024 10:00 GMT ${c}`, `Mon Jan 01 2024 10:00:00 GMT+0100 ${c}`,
      `Jan 1 2024 10${c}:00`, `2024-01-01T10:00:00Z ${c}`, `2024-01-01 ${c}`, `Jan 1 2024 10:00 ${c} PM`, `${c}`);
  }
  // Espaços, tabs, quebras, lixo.
  const ws = ["  ", "\t", "\n", "\r\n", "\v", "\f", " \t \n ", " ", " ", "﻿", " ", "　", "\u0000"];
  for (const w of ws) {
    add(`${w}Jan 1 2024`, `Jan 1 2024${w}`, `Jan${w}1${w}2024`, `${w}2024-01-15${w}`, `2024-01-15T10:20${w}`, `2024-01-15${w}10:20`, `Jan 1 2024${w}10:00${w}GMT`,
      `${w}2024-01-15T10:20:30Z${w}`, `1/${w}1/2024`, `${w}`, `Mon,${w}01${w}Jan${w}2024${w}10:00:00${w}GMT`);
  }
  const trash = ["x", "junk", "!", "?", ",", ";", "Z!", "GMT x", "1x", "0x10", "e5", "1e5", "Jan 1 2024 x", "2024-01-15 foo", "2024-01-15T10:20:30Zjunk", "Jan 1 2024 10:00 GMT junk",
    "Jan 1 2024 ,", "Jan 1 2024 ;", "Jan, 1, 2024", "Jan - 1 - 2024", "- Jan 1 2024", "+ Jan 1 2024", "Jan 1 2024 -", "Jan 1 2024 +", "Jan 1 2024 -1", "Jan 1 2024 +1",
    "Jan 1 2024 10:00 +", "Jan 1 2024 10:00 -", "the 1st of January 2024", "Jan 1st 2024", "1st Jan 2024", "Jan 1th 2024", "Today", "now", "Invalid Date", "NaN", "undefined", "null"];
  add(...trash);
  for (const t of trash) add(`2024-01-15 ${t}`, `Jan 1 2024 ${t}`, `${t} 2024-01-15`);
  add("", " ", "\t", "\n", "()", "(x)", ",", ".", "-", "+", ":", "/", "T", "Z");

  // ---- C. Números soltos 0..100 e variantes.
  for (let n = 0; n <= 100; n++) add(String(n), String(n).padStart(3, "0"), `-${n}`, `${n}:00`, `Jan ${n}`, `${n} Jan 2024`, `Jan ${n} 2024`);
  add("100", "101", "999", "1000", "1970", "1999", "2000", "2024", "9999", "10000", "99999", "100000", "275760", "275761", "12345678", "1e3", "1E3", "0x1", "1_000",
    "-1", "-0", "-00", "-0001", "-1970", "-2024", "-99999", "-271821", "-271822", "+2024", "+10000", "+275760", "+275761", "-0.5", "0.5", ".5", "5.", "1.2.3", "1,5", "1,2,3",
    "1 2 3", "1 2 3 4", "1:2", "1:2:3", "1:2:3:4", "2024 1", "2024 1 15", "2024 15 1", "15 1 2024", "1 15 2024", "99 99 99", "13 13 13", "0 0 0", "1 1 1",
    "9007199254740993", "99999999999999999999", "1" + "0".repeat(40), "-" + "9".repeat(30), "1/1/" + "9".repeat(20), "Jan 1 " + "9".repeat(20), "Jan 1 2024 10:" + "9".repeat(20),
    "2024-01-15T10:20:30." + "9".repeat(40), "+" + "0".repeat(30) + "2024-01-01", "0".repeat(30), "1".repeat(30));

  // ---- D. Textos de toString/toUTCString/toISOString típicos e variantes.
  add("Mon, 01 Jan 2024 10:00:00 GMT", "Mon Jan 01 2024 10:00:00 GMT+0000 (Coordinated Universal Time)", "Mon Jan 01 2024 10:00:00 GMT-0300 (Brasilia Standard Time)",
    "Mon Jan 01 2024", "Mon Jan 01 2024 10:00:00", "Mon Jan 01 2024 10:00:00 GMT", "Mon Jan 01 2024 10:00:00 GMT+0100", "Mon Jan 01 2024 10:00:00 GMT+01:00",
    "Mon Jan 01 2024 10:00:00 GMT+1", "Tue, 01 Jan 2024 10:00:00 GMT", "Mon, 32 Jan 2024 10:00:00 GMT", "Mon, 01 Foo 2024 10:00:00 GMT", "Mon, 01 Jan 2024 10:00:00 GMT+0100",
    "Mon, 1 Jan 2024 10:00:00 +0100", "Mon, 01 Jan 2024 10:00:00 -0000", "Mon, 01 Jan 2024 10:00:00 +0000", "Mon, 01 Jan 24 10:00:00 GMT", "Mon, 01 Jan 99 10:00:00 GMT",
    "Mon, 01 Jan -0001 10:00:00 GMT", "Sun, 31 Dec -271821 00:00:00 GMT", "Tue, 20 Apr -271821 00:00:00 GMT", "Sat, 13 Sep 275760 00:00:00 GMT", "Sat, 14 Sep 275760 00:00:00 GMT",
    "Thu, 01 Jan 1970 00:00:00 GMT", "Wed, 31 Dec 1969 23:59:59 GMT", "Thu Jan 01 1970 00:00:00 GMT+0000", "Fri, 29 Feb 2024 23:59:60 GMT", "Fri, 29 Feb 2024 24:00:00 GMT",
    "Fri, 29 Feb 2024 24:00:01 GMT", "1970-01-01T00:00:00.000Z", "1970-01-01T00:00:00.000+00:00", "-000001-12-31T23:59:59.999Z", "+275760-09-13T00:00:00.000Z",
    "+275760-09-13T00:00:00.001Z", "-271821-04-20T00:00:00.000Z", "-271821-04-19T23:59:59.999Z", "Sat Sep 13 275760 00:00:00 GMT+0000", "Sat Sep 13 275760 00:00:01 GMT+0000",
    "Jan 1, 2024, 10:00:00 AM", "1/1/2024, 10:00:00 AM", "1/1/2024, 10:00:00 PM UTC", "January 1, 2024 at 10:00 AM", "Monday, January 1, 2024 10:00:00 AM GMT-3",
    "2024-01-01 10:00:00.000 +0000", "2024-01-01 10:00:00 +00:00", "2024-01-01 10:00:00 UTC", "2024-01-01 10:00:00 GMT", "2024-01-01 10:00:00 EST", "2024-01-01 10:00:00 PDT",
    "2024-01-01T10:00:00 PDT", "2024-01-01T10:00:00Z PDT", "2024-01-01T10:00:00+01:00 PDT", "2024-01-01 10:00:00+01:00", "2024-01-01 10:00:00 +01:00", "20240101T100000Z", "2024-W01-1",
    "2024-001", "2024-366", "2024-01-01T10:00:00+01:00[Europe/Paris]", "2024-01-01T10:00:00Z[UTC]", "2024-01-01T10:00Z+01:00", "2024-01-01T10:00:00ZZ", "2024-01-01T10:00:00Z ",
    "2024-01-01T10:00:00 Z", "2024-01-01T10:00:00 +01:00", "2024-01-01T10:00:00 +0100", "2024-01-01T10:00:00 +01", "2024-01-01T10:00:00+1:00", "2024-01-01T10:00:00+01:0",
    "2024-01-01T10:00:00+0:00", "2024-01-01T10:00:00+00", "2024-01-01T10:00:00+0", "2024-01-01T10:00:00-", "2024-01-01T10:00:00+");

  // ---- E. Não ASCII: deve falhar (ou não) como no bun.
  const nonAscii = ["é", "ñ", "ü", "Ω", "日本", "١٢", "１２", " 1", "−", "😀", "\ud83d", "\ude00", "ı", "K", "ſ", "İ", "µ", "ÿ", "Ā", "​", " "];
  for (const c of nonAscii) {
    add(c, `${c}2024-01-15`, `2024-01-15${c}`, `2024${c}01${c}15`, `2024-01-15T10:20:30${c}`, `Jan${c} 1 2024`, `${c}Jan 1 2024`, `Jan 1 2024${c}`, `Jan 1 2024 10:00${c}`,
      `Jan 1 2024 10:00 ${c}`, `Jan 1 2024 (${c})`, `Mon, 01 Jan 2024 10:00:00 GMT${c}`, `2024-01-15T10:20:30.5${c}`, `2024-01-15T10:20:30+01${c}00`, `1${c}1${c}2024`, `1/1/2024${c}`, `1/1/${c}2024`);
  }
  // Mês e dia com letras que mudam de caixa por Unicode (K de Kelvin, s longo, i pontilhado).
  add("MaK", "ſep 1 2024", "ſeptember 1 2024", "Março 1 2024", "janeiro 1 2024", "İan 1 2024", "JanK 1 2024", "Monı Jan 1 2024", "MoK Jan 1 2024");

  // ---- F. Mistura aleatória de pedaços.
  const pieces = ["Jan", "15", "2024", "10:20", "10:20:30", "PM", "AM", "GMT", "+0100", "-0300", "Mon,", "(x)", "/", "-", ".", ",", "Z", "UTC", "EST", "1/2", "Feb", "29", "T", "31", "00", "99", ":"];
  const rand = makeRandom(20261008);
  const pick = (list) => list[Math.floor(rand() * list.length)];
  const mixes = [];
  for (let i = 0; i < 1800; i++) {
    const n = 2 + Math.floor(rand() * 5);
    const parts = [];
    for (let k = 0; k < n; k++) parts.push(pick(pieces));
    mixes.push(parts.join(pick([" ", " ", " ", "", "  ", "\t"])));
  }
  add(...sampleByHash(mixes, 300));

  return [...cases];
}

if (process.argv[2] === "--zone") {
  const zone = process.env.TZ;
  const actual = new Intl.DateTimeFormat().resolvedOptions().timeZone;
  if (actual !== zone) throw new Error(`TZ ${zone} ignorada: ${actual}`);
  const lines = [];
  for (const s of buildCases()) {
    const p = Date.parse(s);
    const d = new Date(s);
    const n = d.getTime();
    const iso = Number.isNaN(n) ? "-" : d.toISOString();
    lines.push(`${zone}\t${ascii(s)}\tP=${p}|N=${n}|I=${iso}`);
  }
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
} else {
  for (const zone of ZONES) {
    const child = spawnSync(process.execPath, [__filename, "--zone"], { env: { ...process.env, TZ: zone }, encoding: "utf8", maxBuffer: 1 << 28 });
    if (child.status !== 0) throw new Error(`filho ${zone} falhou: ${child.stderr}`);
    process.stdout.write(child.stdout);
  }
}
