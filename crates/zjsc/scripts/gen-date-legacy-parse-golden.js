// Gera tests/golden/date_legacy_parse_bun.tsv: formatos NÃO ISO de Date.parse e de new Date(string), os que o
// JSC aceita pela rotina legada (parseDateFromNullTerminatedCharacters), medidos no bun 1.4.2.
// Complementa gen-date-parse-golden.js com combinações: forma x fuso x hora, meses, dia da semana errado,
// AM/PM, 24:00:00, fração, comentários, ano de 2 dígitos (pivô 49/50), anos negativos e estendidos,
// espaços e vírgulas extras, minúsculas, "T" fora do ISO, datas inválidas e o que o V8 aceita e o JSC não.
// Colunas: fuso, fonte do programa (uma linha), KIND e REPR (serializador de tests/golden/date_bun_harness.js).
// Cada programa roda em UTC e em America/Sao_Paulo. Se o resultado é o mesmo nos dois, sai com fuso "any"
// (o teste roda nos dois fusos); se difere, sai uma linha por fuso.
// Uso: bun scripts/gen-date-legacy-parse-golden.js > tests/golden/date_legacy_parse_bun.tsv
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const ZONES = ["UTC", "America/Sao_Paulo"];
const programs = [];
const q = (text) => JSON.stringify(text);
const both = (text) => programs.push(`Date.parse(${q(text)})`, `new Date(${q(text)}).getTime()`);
const parse = (text) => programs.push(`Date.parse(${q(text)})`);

const tzs = ["", " GMT", " UT", " Z", " EST", " PDT", " GMT+5", " UTC-3", " +0530", " GMT+0100 (CET)"];
const times = ["", " 13:30:00", " 24:00:00", " 12:00:00.5", " 10:00 PM", " 12:00 AM"];

// Formas base com todas as combinações de hora e fuso (fuso só quando há hora, mais um subconjunto sem hora).
const forms = [
  "Mon, 25 Dec 1995", "Dec 25, 1995", "12/25/1995", "1995/12/25", "25 December 1995", "Tue Mar 01 2022", "Sat, 25 Dec 1995", "25-dec-1995",
];
for (const form of forms) {
  for (const time of times) {
    for (const tz of tzs) {
      if (time === "" && tz !== "" && ["", " GMT", " PST", " +0530"].indexOf(tz) < 0) continue;
      parse(form + time + tz);
    }
    both(form + time);
  }
}

// Os exemplos do enunciado, integrais.
for (const text of [
  "Mon, 25 Dec 1995 13:30:00 GMT", "Dec 25, 1995", "12/25/1995", "1995/12/25", "25 December 1995 10:00 PST",
  "Tue Mar 01 2022 10:00:00 GMT+0100 (CET)", "Mon, 25 Dec 1995 13:30:00 +0430", "Mon, 25 Dec 1995 13:30:00 UT", "Mon, 25 Dec 1995 13:30:00 Z",
  "Mon, 25 Dec 1995 13:30:00 EST", "Mon, 25 Dec 1995 13:30:00 PDT", "Mon, 25 Dec 1995 13:30:00 GMT+5", "Mon, 25 Dec 1995 13:30:00 UTC-3",
]) both(text);

// Meses abreviados e completos, todos.
const longMonths = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
longMonths.forEach((name, index) => {
  const short = name.slice(0, 3);
  for (const spelling of [name, short, name.toUpperCase(), name.slice(0, 4), name + "x"]) {
    both(`${spelling} 15, 1999 10:00:00 GMT`);
    parse(`15 ${spelling} 1999`);
    parse(`${spelling} 15 1999 UTC`);
  }
  const mm = String(index + 1).padStart(2, "0");
  parse(`${mm}/15/1999`); parse(`1999/${mm}/15`); parse(`${index + 1}/1/1999 10:00 PM`); parse(`1999/${index + 1}/31`);
});
parse("Sept 5 1999"); parse("Sept. 5 1999"); parse("Dec. 25, 1995");

// Dia da semana errado, certo, abreviado, completo, ausente.
for (const day of ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun", "Monday", "Tuesday", "sunday", "SAT", "Mo", "Xyz", "Funday", "Mon.", "Mon,"]) {
  parse(`${day} Dec 25 1995`); parse(`${day}, 25 Dec 1995 00:00:00 GMT`); parse(`${day} 25 Dec 1995 10:00 PST`);
}

// Fusos isolados em hora fixa, em duas formas.
for (const tz of ["UT", "Z", "z", "EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT", "GMT", "UTC", "GMT+5", "GMT+05", "GMT+0530", "GMT+05:30", "GMT-3", "GMT-03:00", "UTC-3", "UTC+5", "UTC+05:30", "UT+2", "+0530", "-0300", "+5", "-3", "+05:30", "-03:00", "+0000", "-0000", "+2359", "+2400", "+9999", "+0560", "BRT", "CET", "gmt+5", "Gmt-3", "GMT +5", "GMT+ 5", "EST5", "ESTx"]) {
  parse(`Dec 25 1995 13:30:00 ${tz}`); parse(`25 Dec 1995 13:30:00${tz}`); parse(`1995/12/25 13:30:00 ${tz}`);
}

// AM/PM e horas limite.
for (const hour of ["0", "1", "11", "12", "13", "24"]) {
  for (const meridiem of ["", " AM", " PM", " pm", "AM", " P.M.", " aM"]) {
    parse(`Dec 25 1995 ${hour}:30${meridiem}`); parse(`Dec 25 1995 ${hour}:30:15${meridiem} GMT`);
  }
}
for (const text of ["Dec 25 1995 24:00:00", "Dec 25 1995 24:00:00 GMT", "Dec 25 1995 24:00:01 GMT", "Dec 25 1995 24:01:00 GMT", "Dec 25 1995 23:59:60 GMT", "Dec 25 1995 24:00:00.000 GMT", "Dec 25 1995 24:00:00.001 GMT", "12/31/1995 24:00:00 GMT", "Dec 31 1999 24:00 UTC", "Feb 28 2020 24:00:00 GMT", "Feb 29 2020 24:00:00 GMT"]) both(text);

// Segundos fracionários.
for (const fraction of ["", ".", ".0", ".5", ".05", ".005", ".0005", ".123", ".1234", ".999", ".9999", ".99999999999", ",5", ".5.5", ".5a"]) {
  parse(`Dec 25 1995 13:30:00${fraction} GMT`); parse(`Dec 25 1995 13:30:00${fraction}`); parse(`Dec 25 1995 1:30:00${fraction} PM GMT`);
}

// Comentários entre parênteses.
for (const text of [
  "Dec 25 1995 (comment)", "Dec 25 1995 13:30 (comment)", "Dec 25 1995 13:30 GMT (comment)", "Dec 25 1995 13:30 (comment) GMT", "(comment) Dec 25 1995", "Dec (comment) 25 1995",
  "Dec 25 (comment) 1995", "Dec 25 1995 13:30:00 GMT+0100 (Central European Standard Time)", "Dec 25 1995 (a (b) c) 13:30", "Dec 25 1995 (a (b c) 13:30", "Dec 25 1995 a) 13:30",
  "Dec 25 1995 () 13:30", "Dec 25 1995 13:30 ()", "Dec 25 1995 13:30 (", "Dec 25 1995 (13:30)", "(Dec 25 1995)", "Dec 25 1995 13:30 GMT+0100 (CET) extra", "Dec 25 1995 13:30 GMT+0100(CET)",
  "Dec 25 1995 (GMT) 13:30", "Dec(x)25(y)1995", "Dec 25 1995 13:30 (x) (y)", "Dec 25 1995 13:30 PM (x)", "Dec 25 1995 13:30 (x) PM",
]) both(text);

// Ano de 2 dígitos, pivô 49/50, e outros comprimentos.
for (const year of ["00", "01", "09", "10", "48", "49", "50", "51", "68", "69", "70", "99", "0", "1", "7", "100", "101", "999", "0999", "1000", "00000", "000100", "5", "05"]) {
  for (const form of [`Dec 25 ${year}`, `12/25/${year}`, `${year}/12/25`, `25-Dec-${year}`, `Dec 25, ${year} 10:00:00 GMT`]) parse(form);
}

// Anos negativos e estendidos.
for (const year of ["-1", "-0", "-49", "-50", "-99", "-100", "-1000", "-271821", "-271822", "+1", "+2020", "+275760", "+275761", "-000001", "+000001", "-002020", "+002020", "+010000", "-010000", "+0000000", "-0000000", "+99999", "-99999", "+123456", "-123456", "+1234567", "-1234567", "1234567"]) {
  parse(`Dec 25 ${year}`); parse(`Dec 25 ${year} 10:00:00 GMT`); parse(`12/25/${year}`); parse(`${year}-12-25T00:00:00Z`);
}
for (const text of ["Apr 20 -271821 00:00:00 GMT", "Apr 19 -271821 23:59:59 GMT", "Sep 13 275760 00:00:00 GMT", "Sep 13 275760 00:00:01 GMT", "Sep 14 275760 GMT", "Jan 1 -0000 GMT", "Jan 1 -1 GMT", "Jan 1 0 GMT", "Jan 1 0000 GMT"]) both(text);

// Espaços, vírgulas, pontuação e maiúsculas/minúsculas.
for (const text of [
  "  Dec 25 1995", "Dec 25 1995  ", "  Dec   25   1995  ", "Dec,25,1995", "Dec, 25, 1995", "Dec ,25 ,1995", "Dec  ,  25  ,  1995", "Dec 25,,1995", ",Dec 25 1995", "Dec 25 1995,", "Dec 25 1995 , 10:00", "Dec 25 1995,10:00", "Dec 25 1995, 10:00:00 , GMT",
  "Mon,Dec 25 1995", "Mon , Dec 25 1995", "Mon,,Dec 25 1995", "Dec\t25\t1995", "Dec\n25\n1995", "Dec\r25\r1995", "Dec\u000b25\u000c1995", "Dec 25 1995", "Dec 1995", "Dec 25 1995\t", "\tDec 25 1995", " Dec 25 1995",
  "dec 25 1995", "DEC 25 1995", "dEc 25 1995", "december 25 1995", "DECEMBER 25 1995", "mon dec 25 1995 10:00:00 gmt", "MON DEC 25 1995 10:00:00 GMT", "dec 25 1995 10:00 pm pst", "DEC 25 1995 10:00 PM PST",
  "Dec 25 1995 10:00:00 gmt+0100", "Dec 25 1995 10:00:00 Gmt+0100", "Dec 25 1995 10:00:00 utc", "Dec 25 1995 10:00:00 ut", "Dec 25 1995 10:00:00 z", "Dec 25 1995 10:00:00 est", "Dec 25 1995 10:00:00 Est",
  "Dec 25 1995 10 :00", "Dec 25 1995 10: 00", "Dec 25 1995 10 : 00", "Dec 25 1995 10:00 : 00", "Dec 25 1995 10:00:", "Dec 25 1995 10::00", "Dec 25 1995 :10:00",
  "Dec 25 - 1995", "Dec - 25 - 1995", "Dec-25 1995", "Dec 25-1995", "Dec/25/1995", "Dec.25.1995", "Dec 25.1995", "12 25 1995", "12-25-1995", "12.25.1995", "12/25 1995", "12 /25/ 1995", "12 / 25 / 1995", "1995 12 25", "1995-12 25", "1995 Dec 25", "1995 December 25 10:00 GMT", "25 1995 Dec",
]) both(text);

// "T" fora do ISO, e misturas.
for (const text of [
  "Dec 25 1995T10:00", "Dec 25 1995 T10:00", "Dec 25 1995 T 10:00", "12/25/1995T10:00", "12/25/1995T10:00:00Z", "1995/12/25T10:00", "1995/12/25T10:00:00Z", "1995/12/25T10:00:00+01:00", "1995/12/25 10:00:00Z", "1995/12/25 10:00:00+01:00",
  "25 Dec 1995T10:00", "1995-12-25T10:00:00 GMT", "1995-12-25T10:00:00 PST", "1995-12-25T10:00:00 PM", "1995-12-25 10:00:00 PM", "1995-12-25 10:00 AM", "1995-12-25 10:00:00 GMT+0100", "1995-12-25 10:00:00 (CET)", "1995-12-25T10:00:00Z (CET)",
  "1995-12-25 GMT", "1995-12-25 PST", "1995-12-25 10:00 PST", "1995-12-25 22:00 -0800", "1995-12-25 22:00 +0530", "1995-12-25T22:00 -0800", "1995-12-25t22:00", "1995-12-25 22:00 z", "1995-12-25 22:00:00.5 PST",
  "1995-12", "1995-12 PST", "1995-12 10:00", "1995-1-1", "1995-1-1 10:00", "1995-01-01 10:00:00", "95-12-25", "95/12/25", "12/25/95 10:00", "1995-12-25T10", "1995-12-25 10", "1995-12-25T10:00Z+01",
  "T10:00 Dec 25 1995", "Dec 25 T 1995", "TDec 25 1995", "Dec 25 1995T", "Dec 25 1995 T", "Dec 25 1995 Tue", "Dec 25 1995 Z", "Z Dec 25 1995",
]) both(text);

// Datas inválidas e limites de calendário.
for (const text of [
  "Feb 31 2020", "Feb 30 2020", "Feb 29 2021", "Feb 29 2020", "Feb 29 1900", "Feb 29 2000", "Feb 30 2020 10:00", "Apr 31 2020", "Jun 31 2020", "Sep 31 2020", "Nov 31 2020", "Dec 31 2020", "Dec 32 2020", "Jan 0 2020", "Jan 00 2020", "Jan 1 2020",
  "13/01/2020", "13/13/2020", "00/10/2020", "12/32/2020", "12/00/2020", "02/30/2020", "02/31/2020", "02/29/2021", "02/29/2020", "04/31/2020", "2020/02/31", "2020/02/30", "2020/02/29", "2021/02/29", "2020/13/01", "2020/00/01", "2020/01/00", "2020/01/32", "2020/04/31",
  "31 Feb 2020", "30 Feb 2020", "29 Feb 2021", "31 Apr 2020", "32 Jan 2020", "0 Jan 2020", "31 Dec 2020", "1 Foo 2020", "1 Janx 2020", "13 13 2020", "Foo 1 2020", "Month 1 2020",
  "Dec 25 1995 25:00", "Dec 25 1995 23:60", "Dec 25 1995 23:59:61", "Dec 25 1995 24:60", "Dec 25 1995 99:99", "Dec 25 1995 -1:00", "Dec 25 1995 1:5", "Dec 25 1995 1:5:5", "Dec 25 1995 01:05:5", "Dec 25 1995 001:00",
  "Dec 32 1995", "Dec 25 1995 10:00 GMT+2500", "Dec 25 1995 10:00 GMT+9999", "Dec 25 1995 10:00 +2500", "Dec 25 1995 10:00 +0099",
]) both(text);

// O que o V8 aceita e o JSC não (ou o contrário): medido no bun, qualquer que seja o resultado.
for (const text of [
  "Dec 25 1995 13:30 GMT+0100 (CET) Mon", "1995 Dec 25 13:30 GMT", "25.12.1995", "25/12/1995", "25-12-1995", "25 12 1995", "1995.12.25", "December 25th, 1995", "Dec 25th 1995", "Dec 25st 1995", "25th Dec 1995",
  "Monday, December 25, 1995", "Monday, December 25, 1995 1:30 PM", "Monday, December 25, 1995 1:30:00 PM GMT", "12/25/1995 1:30 PM", "12/25/1995, 1:30:00 PM", "12/25/1995, 13:30:00", "25/12/1995, 13:30:00", "1995-12-25 1:30 PM",
  "Dec 25, 1995 1:30 PM EST", "Dec 25, 1995 at 1:30 PM", "Dec 25, 1995 1:30 PM Eastern", "Dec 25, 1995 1:30 PM Eastern Standard Time", "Dec 25, 1995 13:30 America/New_York", "Dec 25, 1995 13:30 +05:30", "Dec 25, 1995 13:30 GMT+05:30", "Dec 25, 1995 13:30 GMT+5:30",
  "1995-12-25 13:30:00 UTC", "1995-12-25 13:30:00 UTC+1", "1995-12-25 13:30:00 +01", "1995-12-25 13:30:00 +1", "1995-12-25 13:30:00+1", "1995-12-25 13:30:00 +1:00", "1995-12-25 13:30:00 -01:00", "1995-12-25 13:30:00 Z", "1995-12-25T13:30:00 Z", "1995-12-25T13:30:00z",
  "Dec 25 1995 13:30:00 GMT+01:00", "Dec 25 1995 13:30:00 GMT+1:00", "Dec 25 1995 13:30:00 GMT+100", "Dec 25 1995 13:30:00 GMT+01000", "Dec 25 1995 13:30:00 UTC+0100", "Dec 25 1995 13:30:00 UTC+01:00",
  "Mon Dec 25 1995 13:30:00 GMT+0100 (Central European Standard Time)", "Mon Dec 25 1995 13:30:00 GMT-0300 (Brasilia Standard Time)", "Mon Dec 25 1995 13:30:00 GMT+0000 (Coordinated Universal Time)",
  "Mon, 25 Dec 1995 13:30:00 GMT+0100", "Mon, 25 Dec 1995 13:30:00 gmt", "Mon, 25 Dec 1995 13:30:00 UTC", "Mon, 25 Dec 1995 13:30:00 -0800 (PST)", "Mon, 25 Dec 95 13:30:00 GMT", "Mon, 25 Dec 1995 13:30 GMT", "Mon, 25 Dec 1995 13 GMT",
  "1 2", "1 2 3", "1/2", "1/2/3", "2020", "1999", "99", "Dec", "Dec 1995", "1995 Dec", "Dec 25", "25 Dec", "12/25", "Dec-25", "12-25", "1995-12-25-", "1995-12-25T", "Dec 1 1 1", "Dec 25 1995 1995", "Dec 25 25 1995", "Dec Dec 25 1995", "Dec 25 1995 Dec",
  "Tuesday", "Today", "Now", "Yesterday", "Tomorrow", "Next Monday", "Last Friday", "Noon", "Midnight", "10:00 AM", "10 AM", "10AM", "22:00", "1e3", "1e3 Dec 1995", "0x10", "Infinity", "-Infinity", "+0", "-0", "-1", "+1", ".5", "5.", "1.5", "Dec 25.5 1995", "Dec 25 1995.5",
]) both(text);

// Idas e voltas: a saída de toString/toUTCString/toDateString/toLocale reentra.
for (const instant of ["0", "819895800000", "-1", "-62135596800000", "-62198755200000", "253402300799000", "1646125200000", "951782400000", "-2208988800000", "788918400000", "1e12", "8.64e15", "-8.64e15", "-61504521600000", "1577836800000"]) {
  const d = `new Date(${instant})`;
  for (const method of ["toString()", "toUTCString()", "toDateString()", "toISOString()", "toGMTString()", "toLocaleString('en-US', {timeZone: 'UTC'})", "toLocaleDateString('en-US', {timeZone: 'UTC'})"]) {
    programs.push(`Date.parse(${d}.${method}) - ${d}.getTime()`, `new Date(${d}.${method}).getTime() - ${d}.getTime()`);
  }
  programs.push(`${d}.toString()`, `${d}.toUTCString()`);
}

// Fusos e horário de verão locais: horas na transição (Sao_Paulo mudava em out/fev nos anos 90 e 2018).
for (const text of [
  "Dec 25 1995 13:30", "Oct 15 1995 00:00", "Oct 15 1995 01:00", "Feb 18 1996 00:00", "Feb 17 1996 23:30", "Nov 4 2018 00:00", "Nov 4 2018 01:00", "Feb 17 2019 23:00", "Feb 16 2019 23:59:59", "Mar 10 2024 02:30", "Nov 3 2024 01:30", "Mar 31 2024 02:30", "Oct 27 2024 01:30",
  "10/15/1995 00:30", "1995/10/15 00:30", "Jan 1 1900", "Jan 1 1900 00:00:00", "Dec 31 1969 21:00", "Jan 1 1970 00:00", "Jan 1 1883", "Nov 18 1883 12:00", "Jan 1 0001", "Jan 1 0099", "Jan 1 0100", "Dec 31 1 23:59:59",
]) both(text);

// Fuso local do Date.parse e a mesma instância em outras formas.
programs.push(
  "Date.parse('Dec 25 1995 10:00') === Date.parse('1995/12/25 10:00')", "Date.parse('12/25/1995') === Date.parse('Dec 25 1995')", "Date.parse('Dec 25 1995') === Date.parse('1995-12-25T00:00:00')", "Date.parse('Dec 25 1995 GMT') === Date.parse('1995-12-25')",
  "new Date('Dec 25 1995 10:00 PM').getHours()", "new Date('Dec 25 1995 12:00 AM').getHours()", "new Date('Dec 25 1995 12:00 PM').getHours()", "new Date('Dec 25 1995 10:00 GMT+0530').toISOString()", "new Date('Dec 25 1995 10:00 PST').toISOString()", "new Date('25 December 1995 10:00 PST').toISOString()",
  "new Date('Mon, 25 Dec 1995 13:30:00 GMT').toISOString()", "new Date('Tue Mar 01 2022 10:00:00 GMT+0100 (CET)').toISOString()", "new Date('Dec 25, 1995').toString()", "new Date('12/25/1995').toString()", "new Date('1995/12/25').toString()", "new Date('1995/12/25').getTimezoneOffset()",
  "new Date('Dec 25 1995 10:00 GMT+0530').toUTCString()", "new Date('Dec 25 1995 24:00:00 GMT').toISOString()", "new Date('Dec 25 49').getFullYear()", "new Date('Dec 25 50').getFullYear()", "new Date('12/25/49').getFullYear()", "new Date('12/25/50').getFullYear()", "new Date('Dec 25 -5').getFullYear()", "new Date('Dec 25 +5').getFullYear()",
  "var d = new Date('Dec 25 1995 10:00 PST'); d.getUTCHours() + ':' + d.getUTCMinutes()", "var d = new Date('Dec 25 1995 13:30:00.250 GMT'); d.getUTCMilliseconds()", "String(new Date('Dec 31 2020 24:00:00 GMT'))", "String(new Date('Feb 31 2020'))", "String(new Date('Dec 25 1995 25:00'))",
  "typeof Date.parse('Dec 25 1995')", "Number.isNaN(Date.parse('Foo 25 1995'))", "Date.parse('Dec 25 1995') === new Date('Dec 25 1995').valueOf()", "Date.parse(new String('Dec 25 1995 GMT'))", "Date.parse({ toString() { return 'Dec 25 1995 GMT' } })", "Date.parse(['Dec 25 1995 GMT'])",
);

if (process.argv[2] === "child") {
  const fs = require("fs");
  const path = require("path");
  const harness = eval(fs.readFileSync(path.join(__dirname, "../tests/golden/date_bun_harness.js"), "utf8").trim());
  for (const source of new Set(programs)) {
    if (/[\t\n\r]/.test(source)) throw new Error("programa com tabulação ou quebra de linha: " + source);
    emitRow(`${source}\t${harness(source)}`);
  }
} else {
  const byZone = ZONES.map((zone) => {
    const child = spawnSync(process.execPath, [__filename, "child"], {
      env: { ...process.env, TZ: zone },
      encoding: "utf8",
      maxBuffer: 1 << 28,
      timeout: 120000,
    });
    if (child.status !== 0) throw new Error(`fuso ${zone}: ${child.stderr || child.error}`);
    return child.stdout.split("\n").filter((line) => line);
  });
  let independent = 0;
  let dependent = 0;
  for (let index = 0; index < byZone[0].length; index++) {
    const lines = byZone.map((zoneLines) => zoneLines[index]);
    if (lines.every((line) => line === lines[0])) {
      emitRow(`any\t${lines[0]}`);
      independent++;
    } else {
      ZONES.forEach((zone, zoneIndex) => emitRow(`${zone}\t${lines[zoneIndex]}`));
      dependent++;
    }
  }
  console.error(`${new Set(programs).size} programas, ${independent} independentes de fuso, ${dependent} dependentes`);
}
