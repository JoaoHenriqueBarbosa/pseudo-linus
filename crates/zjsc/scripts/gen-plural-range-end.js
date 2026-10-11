// Mede no bun (JavaScriptCore + ICU) os pares de categorias do `selectRange` cujo resultado é a categoria
// final, e gera a tabela que `src/runtime/icu_plural.rs` consulta. São as linhas do CLDR que o gerador de dados
// do icu4x descarta (o `resolve_range` dele devolve a final quando a linha falta, e o ICU devolve `other`).
// Uso, a partir da raiz da crate:
//   bun scripts/gen-plural-range-end.js
// Escreve:
//   src/runtime/icu_plural_range_data.rs   CARDINAL e ORDINAL: (locale, pares "início-fim"), ordenados por locale
// Cobertura: toda língua que `Intl.PluralRules.supportedLocalesOf` aceita (códigos aa..zzz), e as variantes de
// região e de escrita (língua-REGIÃO, língua-Escrita, língua-Escrita-REGIÃO) que se comportam diferente da
// língua, nas regras de `select` ou na tabela de intervalos. Variante idêntica à língua não tem entrada: a
// consulta cai por truncamento (escrita+região, região, escrita, língua), como o ICU resolve o locale.
// Uma variante com regras próprias entra mesmo sem pares (lista vazia), para encobrir os pares da língua.
// Amostras: inteiros de 0 a 1500 (cobrem `many` ordinal do italiano, `zero` do árabe...), decimais com um dígito
// (`1.5`, `0.1`), com zero à direita (`1.0`) e potências compactas (`1e6`, `1.5e6`); por categoria, cinco
// inteiros e três não inteiros.
const { dirname, join } = require("path");
const { writeRustSource } = require("./rust-escape.js");

const root = join(dirname(__filename), "..");
const letters = "abcdefghijklmnopqrstuvwxyz";
const codes = [];
for (const a of letters) for (const b of letters) {
  codes.push(a + b);
  for (const c of letters) codes.push(a + b + c);
}
const languages = codes.filter((code) => Intl.PluralRules.supportedLocalesOf([code]).length);

const regions = ["001", "150", "419"];
for (const a of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") for (const b of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") regions.push(a + b);
const scripts = ["Latn", "Cyrl", "Arab", "Hans", "Hant", "Guru", "Deva", "Beng", "Grek", "Hebr", "Thai", "Jpan", "Kore", "Mong", "Tfng", "Vaii", "Olck"];

const fingerprintNumbers = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 20, 21, 22, 100, 101, 0.5, 1.5, 2.5, 1.0, 11.5, 1e6, 1.5e6];
const rangeNumbers = [0, 1, 2, 3, 5, 11, 21, 100.5];

function make(tag, type) {
  try {
    return new Intl.PluralRules(tag, { type });
  } catch {
    return null;
  }
}

// Impressão digital barata do comportamento (regras e intervalos) do locale, para achar as variantes que diferem.
function fingerprint(rules) {
  const parts = [];
  for (const n of fingerprintNumbers) parts.push(rules.select(n));
  for (const a of rangeNumbers) for (const b of rangeNumbers) parts.push(rules.selectRange(a, b));
  return parts.join(",");
}

function samples(rules) {
  const ints = {}, others = {};
  const note = (n) => {
    const category = rules.select(n);
    const bucket = Number.isInteger(n) ? ints : others;
    (bucket[category] ??= []).push(n);
  };
  for (let i = 0; i <= 1500; i++) note(i);
  for (let i = 0; i <= 120; i++) { note(i + 0.5); note(i + 0.1); }
  for (const n of [1.0, 2.0, 1e6, 2e6, 1.5e6]) note(n);
  const out = {};
  for (const category of new Set([...Object.keys(ints), ...Object.keys(others)])) {
    out[category] = [...(ints[category] ?? []).slice(0, 5), ...(others[category] ?? []).slice(0, 3)];
  }
  return out;
}

// Os pares "início-fim" (fim diferente de `other`) em que todas as amostras dão a categoria final.
function endPairs(tag, rules) {
  const sample = samples(rules);
  const keys = Object.keys(sample);
  const pairs = [];
  for (const start of keys) for (const end of keys) {
    if (end === "other") continue;
    const results = new Set();
    for (const a of sample[start]) for (const b of sample[end]) results.add(rules.selectRange(a, b));
    if (results.size === 1 && results.has(end)) pairs.push(`${start}-${end}`);
    else if (results.size > 1) console.error("MIXED", tag, start, end, [...results]);
  }
  return pairs;
}

function table(type) {
  const entries = new Map();
  const baseline = new Map();
  for (const language of languages) {
    const rules = make(language, type);
    if (!rules) continue;
    baseline.set(language, fingerprint(rules));
    const pairs = endPairs(language, rules);
    if (pairs.length) entries.set(language, pairs);
    const scriptHits = [];
    for (const script of scripts) {
      const tag = `${language}-${script}`;
      if (probe(tag, language, type, baseline)) { scriptHits.push(script); entries.set(tag, endPairs(tag, make(tag, type))); }
    }
    for (const region of regions) {
      const tag = `${language}-${region}`;
      if (probe(tag, language, type, baseline)) entries.set(tag, endPairs(tag, make(tag, type)));
    }
    for (const script of scriptHits) for (const region of regions) {
      const tag = `${language}-${script}-${region}`;
      if (probe(tag, `${language}-${script}`, type, baseline)) entries.set(tag, endPairs(tag, make(tag, type)));
    }
  }
  return [...entries.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
}

// Verdadeiro se `tag` se comporta diferente do `parent` (grava a impressão dela como nova linha de base).
function probe(tag, parent, type, baseline) {
  const rules = make(tag, type);
  if (!rules) return false;
  const print = fingerprint(rules);
  if (print === baseline.get(parent)) return false;
  baseline.set(tag, print);
  return true;
}

const render = (name, rows) =>
  `pub const ${name}: &[(&str, &[&str])] = &[\n${rows
    .map(([tag, pairs]) => `    (${JSON.stringify(tag)}, &[${pairs.map((p) => JSON.stringify(p)).join(", ")}]),`)
    .join("\n")}\n];\n`;

const cardinal = table("cardinal");
const ordinal = table("ordinal");
writeRustSource(
  join(root, "src/runtime/icu_plural_range_data.rs"),
  `//! Gerado por \`scripts/gen-plural-range-end.js\` (bun 1.4.2, ICU completo). Não editar à mão.
//!
//! Por locale, os pares de categorias (início e fim) do \`selectRange\` cujo resultado é a categoria final: linhas
//! do CLDR que o icu4x omite. Só há entrada para a língua com pares e para a variante de região ou escrita que
//! se comporta diferente da língua (lista possivelmente vazia, que encobre a da língua). Ordenado por locale,
//! para busca binária.

${render("CARDINAL", cardinal)}
${render("ORDINAL", ordinal)}`,
);
console.error(`cardinal ${cardinal.length} locales, ordinal ${ordinal.length} locales`);
