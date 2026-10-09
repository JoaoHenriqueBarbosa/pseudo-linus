// Mede no bun as tabelas de alias de canonicalização de tags e gera src/runtime/intl_locale_aliases_data.rs.
// Uso: bun scripts/gen-locale-aliases.js > src/runtime/intl_locale_aliases_data.rs
const { escapeDashes } = require("./rust-escape.js");
const canon = (tag) => { try { return Intl.getCanonicalLocales(tag)[0]; } catch { return null; } };
const letters = "abcdefghijklmnopqrstuvwxyz";
const languages = [];
for (const a of letters) for (const b of letters) {
  languages.push(a + b);
  for (const c of letters) languages.push(a + b + c);
}
const languageAliases = [];
for (const language of languages) {
  const out = canon(language);
  if (out && out !== language) languageAliases.push([language, out]);
}
const regions = [];
for (const a of letters.toUpperCase()) for (const b of letters.toUpperCase()) regions.push(a + b);
for (let n = 0; n < 1000; n++) regions.push(String(n).padStart(3, "0"));
const regionAliases = [];
for (const region of regions) {
  const out = canon("en-" + region);
  if (out && out !== "en-" + region) regionAliases.push([region, out.slice(3)]);
}
const scriptAliases = [];
for (const script of ["Qaai", "Qaac", "Qaaq"]) {
  const out = canon("en-" + script);
  if (out && out !== "en-" + script) scriptAliases.push([script, out.slice(3)]);
}
// Valores de chave -u-: o que o bun troca.
const valueAliases = [];
const probe = (key, value) => {
  const out = canon("en-u-" + key + "-" + value);
  if (!out) return;
  const expected = "en-u-" + key + "-" + value;
  if (out === expected) return;
  const got = out.slice(("en-u-" + key).length);
  valueAliases.push([key, value, got === "" ? "" : got.slice(1)]);
};
const candidates = {
  ca: ["islamicc", "ethiopic-amete-alem", "gregorian", "gregory"],
  ms: ["imperial", "uksystem", "metric", "ussystem"],
  ks: ["primary", "secondary", "tertiary", "quaternary", "identic", "level1", "level2", "level3", "level4", "identic"],
  kb: ["yes", "true"], kc: ["yes", "true"], kh: ["yes", "true"], kk: ["yes", "true"], kn: ["yes", "true"],
  ka: ["yes", "true", "posix", "shifted", "noignore"],
  kf: ["yes", "true", "upper", "lower", "false", "no"],
  kr: ["space", "punct", "symbol", "currency", "digit"],
  lb: ["strict", "normal", "loose"], lw: ["normal", "breakall", "keepall", "phrase"],
  co: ["dictionary", "phonebook", "traditional", "gb2312han", "big5han", "pinyin", "stroke"],
  nu: ["traditional", "native", "finance", "latn"],
  hc: ["h11", "h12", "h23", "h24"],
  em: ["emoji", "text", "default"], fw: ["mon", "sun"], ss: ["none", "standard"], dx: ["thai"],
  mu: ["celsius", "kelvin", "fahrenhe"], cu: ["usd", "eur", "ADP", "adp", "xxx"],
  va: ["posix"], vt: ["0061"],
};
for (const [key, values] of Object.entries(candidates)) for (const value of values) probe(key, value);
// tz: todo o espaço 3 a 5 letras que o bun troca.
const tzAliases = [];
const tzProbe = (value) => {
  const out = canon("en-u-tz-" + value);
  if (out && out !== "en-u-tz-" + value) tzAliases.push([value, out.slice("en-u-tz-".length)]);
};
for (const a of letters) for (const b of letters) for (const c of letters) {
  tzProbe(a + b + c);
  for (const d of letters) tzProbe(a + b + c + d);
}
const countries = new Set(regions.filter((r) => r.length === 2).map((r) => r.toLowerCase()));
countries.add("utc");
for (const country of countries) for (const a of letters) for (const b of letters) for (const c of letters) tzProbe(country + a + b + c);
for (const value of ["utcw01", "utcw02", "utce01", "utcw12"]) tzProbe(value);
// Tags grandfathered e irregulares.
const grandfathered = ["art-lojban", "cel-gaulish", "en-GB-oed", "i-ami", "i-bnn", "i-hak", "i-klingon", "i-lux", "i-mingo", "i-navajo",
  "i-pwn", "i-tao", "i-tay", "i-tsu", "no-bok", "no-nyn", "sgn-BE-FR", "sgn-BE-NL", "sgn-CH-DE", "zh-guoyu", "zh-hakka", "zh-min",
  "zh-min-nan", "zh-xiang"].map((tag) => [tag, canon(tag)]);
const rs = (s) => JSON.stringify(s);
const pairs = (name, list, arity) => `pub const ${name}: [(${Array(arity).fill("&str").join(", ")}); ${list.length}] = [\n` +
  list.map((row) => "    (" + row.map(rs).join(", ") + "),").join("\n") + "\n];\n";
let out = "//! Gerado por `scripts/gen-locale-aliases.js` (medido no bun 1.4.2). Não editar à mão.\n\n";
out += "/// Língua (2 ou 3 letras) e o que o bun devolve para ela.\n" + pairs("LANGUAGE_ALIASES", languageAliases, 2);
out += "\n/// Região e o que o bun devolve para ela.\n" + pairs("REGION_ALIASES", regionAliases, 2);
out += "\n/// Chave `-u-`, valor e o novo valor (vazio quando o bun descarta o valor, como `kb-yes`).\n" + pairs("UNICODE_VALUE_ALIASES", valueAliases, 3);
out += "\n/// `-u-tz-` valor e o novo valor.\n" + pairs("TIMEZONE_ALIASES", tzAliases, 2);
out += "\n/// Tags grandfathered completas: `Some(novo)` ou `None` quando o bun responde RangeError.\n" +
  `pub const GRANDFATHERED: [(&str, Option<&str>); ${grandfathered.length}] = [\n` +
  grandfathered.map(([tag, value]) => `    (${rs(tag.toLowerCase())}, ${value === null ? "None" : "Some(" + rs(value) + ")"}),`).join("\n") + "\n];\n";
console.log(escapeDashes(out));
