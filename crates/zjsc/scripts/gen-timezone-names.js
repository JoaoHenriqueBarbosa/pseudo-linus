// Gera src/runtime/time_zone_names_data.rs e tests/golden/timezone_names_bun.tsv a partir do bun 1.4.2:
// o nome longo do fuso no `Date.prototype.toString()` (`(India Standard Time)`), com `TZ` definida por
// subprocesso. Zonas medidas: `Intl.supportedValuesOf('timeZone')`, UTC e os apelidos IANA que a tabela
// antiga `time_zone_names.rs` lista. Cada zona é amostrada a cada trimestre de 1900 a 2100 (a cada ano de
// 1970 a 2037, a cada cinco fora disso); o nome do deslocamento maior no mesmo ano é o de verão (`UCAL_DST`),
// o outro é o padrão. Nome de verão que nenhuma amostra mostra (fuso sem horário de verão) sai vazio e o
// Rust cai na tabela antiga. Nome no formato `GMT+12:45` vira vazio: o Rust devolve `None` e usa o GMT
// localizado.
// Uso: bun scripts/gen-timezone-names.js
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { writeRustSource } = require("./rust-escape.js");

const root = path.join(__dirname, "..");
const PROBE = `
const out = [];
for (let y = 1900; y <= 2100; y += (y < 1970 || y > 2037 ? 5 : 1)) for (const m of [0, 3, 6, 9]) {
  const d = new Date(Date.UTC(y, m, 15, 12));
  const mt = d.toString().match(/\\((.*)\\)$/);
  out.push([y, mt ? mt[1] : "", -d.getTimezoneOffset()]);
}
console.log(JSON.stringify({ resolved: Intl.DateTimeFormat().resolvedOptions().timeZone, out }));
`;

const legacy = fs.readFileSync(path.join(root, "src/runtime/time_zone_names.rs"), "utf8");
const legacyZones = [];
for (const block of legacy.matchAll(/zones:\s*&\[([^\]]*)\]/g)) {
  for (const m of block[1].matchAll(/"([^"]+)"/g)) legacyZones.push(m[1]);
}
const zones = [...new Set([...Intl.supportedValuesOf("timeZone"), "UTC", ...legacyZones])].sort();

const GMT_FORMAT = /^GMT[+-]\d/;
const data = [];
const skipped = [];
for (const zone of zones) {
  const run = spawnSync(process.execPath, ["-e", PROBE], { env: { ...process.env, TZ: zone }, encoding: "utf8", timeout: 60000 });
  if (run.status !== 0) {
    skipped.push(zone);
    continue;
  }
  const { out } = JSON.parse(run.stdout);
  const byYear = new Map();
  for (const [year, name, offset] of out) {
    if (!byYear.has(year)) byYear.set(year, []);
    byYear.get(year).push([name, offset]);
  }
  const standardVotes = new Map();
  const daylightVotes = new Map();
  const vote = (map, name) => map.set(name, (map.get(name) || 0) + 1);
  const names = new Set(out.map((row) => row[1]));
  for (const rows of byYear.values()) {
    const offsets = [...new Set(rows.map((row) => row[1]))];
    if (offsets.length < 2) continue;
    const low = Math.min(...offsets);
    const high = Math.max(...offsets);
    for (const [name, offset] of rows) vote(offset === high ? daylightVotes : offset === low ? standardVotes : new Map(), name);
  }
  const top = (map) => [...map.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  let standard;
  let daylight = "";
  if (daylightVotes.size === 0) {
    standard = names.size === 1 ? [...names][0] : top(new Map(out.map((row) => [row[1], 1])));
  } else {
    standard = top(standardVotes);
    daylight = top(daylightVotes);
    if (daylight === standard) daylight = "";
  }
  if (names.size > 2) console.error(`aviso: ${zone} tem ${names.size} nomes: ${[...names].join(" | ")}`);
  if (GMT_FORMAT.test(standard)) standard = "";
  if (GMT_FORMAT.test(daylight)) daylight = "";
  data.push({ zone, standard, daylight });
}

const esc = (text) => text.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
let rust = "//! Gerado por `scripts/gen-timezone-names.js` a partir do bun 1.4.2 (nome longo do fuso no\n";
rust += "//! `Date.prototype.toString()`); NÃO editar à mão. `(zona IANA, nome padrão, nome de verão)`, ordenado por zona\n";
rust += "//! para busca binária. Nome padrão vazio: o fuso não tem nome no ICU (o bun imprime `GMT+12:45`). Nome de verão\n";
rust += "//! vazio: nenhuma amostra de 1900 a 2100 o mostrou (fuso sem horário de verão).\n\n";
rust += "pub const ZONE_NAMES: &[(&str, &str, &str)] = &[\n";
for (const { zone, standard, daylight } of data) rust += `    ("${esc(zone)}", "${esc(standard)}", "${esc(daylight)}"),\n`;
rust += "];\n";
writeRustSource(path.join(root, "src/runtime/time_zone_names_data.rs"), rust);

// Golden: o `toString` de seis instantes por fuso (janeiro e julho de 1900, 1970, 2024 e 2100), só nas
// zonas canônicas (as de `Intl.supportedValuesOf` e UTC). Colunas: zona, ms desde a época, nome entre parênteses.
const INSTANTS = [1900, 1970, 2024, 2100].flatMap((year) => [0, 6].map((month) => Date.UTC(year, month, 15, 12)));
const canonical = new Set([...Intl.supportedValuesOf("timeZone"), "UTC"]);
let tsv = "";
for (const zone of zones) {
  if (!canonical.has(zone) || skipped.includes(zone)) continue;
  const script = `console.log(JSON.stringify(${JSON.stringify(INSTANTS)}.map((t) => new Date(t).toString().match(/\\((.*)\\)$/)[1])))`;
  const run = spawnSync(process.execPath, ["-e", script], { env: { ...process.env, TZ: zone }, encoding: "utf8", timeout: 60000 });
  const names = JSON.parse(run.stdout);
  INSTANTS.forEach((instant, index) => (tsv += `${zone}\t${instant}\t${names[index]}\n`));
}
fs.writeFileSync(path.join(root, "tests/golden/timezone_names_bun.tsv"), require("./golden-prelude.js").assertPublicResult(tsv));
console.error(`zonas: ${data.length}, puladas: ${skipped.join(",") || "nenhuma"}, linhas do golden: ${tsv.split("\n").length - 1}`);
