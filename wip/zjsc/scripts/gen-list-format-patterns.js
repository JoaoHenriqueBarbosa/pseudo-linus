// Mede no bun (JavaScriptCore + ICU) os padrões de `Intl.ListFormat` de todos os locales e gera a tabela que o
// porte usa no lugar do `icu_list` (os dados do CLDR do icu4x divergem dos do ICU do bun em vários locales).
// Uso, a partir da raiz da crate:
//   bun scripts/gen-list-format-patterns.js
// Escreve:
//   src/runtime/intl_list_format_data.rs   PATTERNS (conjuntos únicos de 9 combinações type x style, cada uma
//                                          com pair/start/middle/end), TAGS (tag -> índice, só as tags cujo
//                                          resultado difere do da resolução por truncamento), CONDITIONALS
//                                          (regras de troca do literal: es "y"->"e"/"o"->"u", he "ו"->"ו-") e
//                                          HEBREW_RANGES (o script Hebrew medido, ponto de código a ponto).
//
// Método. Cada locale é medido com elementos neutros (uma letra hebraica seguida de um caractere de uso
// privado), então `format` de 2, 3 e 4 elementos entrega os literais pair, start/end e middle. Os candidatos são
// língua, língua-região (todas as 676), língua-escrita e língua-escrita-região. Uma tag só entra na tabela se o
// seu resultado difere do que a resolução por truncamento (l-s-r, l-s, l; ou l-r, l; língua ausente = "en")
// daria com a tabela já montada. As regras condicionais são medidas por sonda (`i`, `o`, letra latina) e os
// predicados são conferidos contra o bun por varredura (todos os pontos de código como primeiro caractere).
// O final do script formata com a tabela emulada em JS e compara com o bun em milhares de casos; qualquer
// divergência derruba o gerador.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRustSource } from "./rust-escape.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const TYPES = ["conjunction", "disjunction", "unit"];
const STYLES = ["long", "short", "narrow"];
const el = (i) => String.fromCharCode(0x5d0, 0xe000 + i);

// ---- língua canônicas disponíveis -----------------------------------------------------------------------------
const letters = "abcdefghijklmnopqrstuvwxyz";
const langs = [];
for (const a of letters) for (const b of letters) for (const c of ["", ...letters]) {
  const tag = a + b + c;
  if (Intl.ListFormat.supportedLocalesOf([tag]).length && Intl.getCanonicalLocales(tag)[0] === tag) langs.push(tag);
}
const regions = [];
for (const a of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") for (const b of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") regions.push(a + b);
const scriptSet = new Set(["Latn", "Cyrl", "Arab", "Hans", "Hant", "Guru", "Deva", "Beng", "Grek", "Hebr", "Thai", "Jpan", "Kore", "Mong", "Tfng", "Vaii", "Olck", "Armn", "Geor", "Ethi", "Khmr", "Laoo", "Mymr", "Sinh", "Taml", "Telu", "Knda", "Mlym", "Gujr", "Orya", "Tibt", "Syrc", "Thaa", "Nkoo", "Adlm", "Cher", "Shaw", "Dsrt"]);
for (const lang of langs) scriptSet.add(new Intl.Locale(lang).maximize().script);
const scripts = [...scriptSet].filter(Boolean).sort();

// ---- medição de um locale --------------------------------------------------------------------------------------
const strip = (text, first, second) => text.slice(text.indexOf(first) + first.length, text.indexOf(second));
function measure(tag) {
  const flat = [];
  for (const type of TYPES) for (const style of STYLES) {
    const formatter = new Intl.ListFormat(tag, { type, style });
    const e = [el(0), el(1), el(2), el(3)];
    const two = formatter.format([e[0], e[1]]);
    const three = formatter.format([e[0], e[1], e[2]]);
    const four = formatter.format(e);
    const headOf = (text, list) => text.slice(0, text.indexOf(list[0]));
    const tailOf = (text, list) => text.slice(text.indexOf(list[list.length - 1]) + 2);
    const pair = strip(two, e[0], e[1]);
    const start = strip(four, e[0], e[1]);
    const middle = strip(four, e[1], e[2]);
    const end = strip(four, e[2], e[3]);
    const head = headOf(four, e);
    const tail = tailOf(four, e);
    if (strip(three, e[0], e[1]) !== start || strip(three, e[1], e[2]) !== end || headOf(three, e) !== head || tailOf(three, e.slice(0, 3)) !== tail) {
      throw new Error(`${tag} ${type} ${style}: lista de 3 difere da de 4`);
    }
    const five = formatter.format([e[0], e[1], e[2], e[3], el(4)]);
    if (strip(five, e[2], e[3]) !== middle || strip(five, e[3], el(4)) !== end || headOf(five, e) !== head) throw new Error(`${tag} ${type} ${style}: lista de 5 difere da de 4`);
    flat.push(headOf(two, e), pair, tailOf(two, e.slice(0, 2)), head, start, middle, end, tail);
  }
  return flat;
}

// Regras condicionais por sonda: [predicado, palavra que o dispara]. O literal alternativo é o que sai com a palavra.
const PREDICATES = ["SpanishE", "SpanishU", "NotHebrew"];
const PROBES = [["NotHebrew", "xxx"], ["SpanishE", "ixx"], ["SpanishU", "oxx"]];
function measureConditionals(tag) {
  const rules = new Map();
  let combo = 0;
  for (const type of TYPES) for (const style of STYLES) {
    const formatter = new Intl.ListFormat(tag, { type, style });
    for (const [predicate, word] of PROBES) {
      for (const size of [2, 3]) {
        const e = [el(0), el(1)];
        if (size === 3) e.push(el(2));
        const neutral = formatter.format(e);
        const last = e.length - 1;
        const probed = formatter.format(e.map((x, i) => (i === last ? word : x)));
        const base = neutral.slice(neutral.indexOf(e[last - 1]) + 2, neutral.indexOf(e[last]));
        const alt = probed.slice(probed.indexOf(e[last - 1]) + 2, probed.indexOf(word));
        // Uma troca já explicada por um predicado anterior (a sonda `xxx` vale para `ixx` e `oxx`) não se repete.
        if (base !== alt && ![...rules.values()].some(([b, a]) => b === base && a === alt)) rules.set(`${base}\u0000${alt}\u0000${predicate}`, [base, alt, PREDICATES.indexOf(predicate)]);
      }
    }
    combo++;
  }
  return [...rules.values()].sort((a, b) => (a[0] + a[1]).localeCompare(b[0] + b[1]) || a[2] - b[2]);
}

// ---- a tabela, com resolução por truncamento -------------------------------------------------------------------
const patternPool = []; // array de 36 strings
const poolIndex = new Map();
const tagToPattern = new Map();
const tagToConditionals = new Map();
const keyOf = (value) => JSON.stringify(value);
function chain(tag) {
  const parts = tag.split("-");
  const lang = parts[0];
  const script = parts.find((p, i) => i > 0 && /^[A-Z][a-z]{3}$/.test(p));
  const region = parts.find((p, i) => i > 0 && /^([A-Z]{2}|\d{3})$/.test(p));
  const out = [];
  if (script && region) out.push(`${lang}-${script}-${region}`);
  if (script) out.push(`${lang}-${script}`);
  if (!script && region) out.push(`${lang}-${region}`);
  out.push(lang);
  return out;
}
function lookup(map, tag) {
  for (const candidate of chain(tag)) if (map.has(candidate)) return map.get(candidate);
  return undefined;
}
const lookupPattern = (tag) => lookup(tagToPattern, tag) ?? tagToPattern.get("en");
const lookupConditionals = (tag) => lookup(tagToConditionals, tag) ?? [];
const parentOf = (tag) => tag.split("-").slice(0, -1).join("-");

function consider(tag, isLanguage) {
  const flat = measure(tag);
  const rules = measureConditionals(tag);
  const key = keyOf(flat);
  if (!poolIndex.has(key)) { poolIndex.set(key, patternPool.length); patternPool.push(flat); }
  const index = poolIndex.get(key);
  let emitted = false;
  const parentPattern = isLanguage ? undefined : lookupPattern(tag);
  if (isLanguage || parentPattern !== index) { tagToPattern.set(tag, index); emitted = true; }
  const parentRules = isLanguage ? [] : lookupConditionals(tag);
  if (keyOf(rules) !== keyOf(parentRules)) tagToConditionals.set(tag, rules);
  return emitted;
}

// en primeiro, porque a língua ausente cai nele.
consider("en", true);
for (const lang of langs) if (lang !== "en") consider(lang, true);
for (const lang of langs) for (const region of regions) consider(`${lang}-${region}`, false);
const scriptHits = [];
for (const lang of langs) for (const script of scripts) {
  const tag = `${lang}-${script}`;
  if (consider(tag, false) || tagToConditionals.has(tag)) scriptHits.push([lang, script]);
}
for (const [lang, script] of scriptHits) for (const region of regions) consider(`${lang}-${script}-${region}`, false);

// ---- predicados: varredura por primeiro caractere --------------------------------------------------------------
const endLiteral = (formatter, word) => {
  const out = formatter.format([el(0), word]);
  return out.slice(out.indexOf(el(0)) + 2, out.length - word.length);
};
const hebrewRanges = [];
{
  const formatter = new Intl.ListFormat("he", { type: "conjunction" });
  const base = endLiteral(formatter, "א");
  let start = -1;
  for (let cp = 0; cp <= 0x10ffff; cp++) {
    if (cp >= 0xd800 && cp <= 0xdfff) { if (start >= 0) { hebrewRanges.push([start, cp - 1]); start = -1; } continue; }
    const hebrew = endLiteral(formatter, String.fromCodePoint(cp)) === base;
    if (hebrew && start < 0) start = cp;
    if (!hebrew && start >= 0) { hebrewRanges.push([start, cp - 1]); start = -1; }
  }
  if (start >= 0) hebrewRanges.push([start, 0x10ffff]);
}
const isHebrew = (cp) => hebrewRanges.some(([lo, hi]) => cp >= lo && cp <= hi);

const startsWithAt = (chars, index, set) => index < chars.length && set.includes(chars[index]);
const predicates = {
  SpanishE(word) {
    const c = [...word];
    if (startsWithAt(c, 0, "iI")) return true;
    return startsWithAt(c, 0, "hH") && startsWithAt(c, 1, "iI") && !startsWithAt(c, 2, "aAeE");
  },
  SpanishU(word) {
    const c = [...word];
    if (startsWithAt(c, 0, "oO8")) return true;
    if (startsWithAt(c, 0, "hH") && startsWithAt(c, 1, "oO")) return true;
    return c[0] === "1" && c[1] === "1" && (c.length === 2 || c[2] === " ");
  },
  NotHebrew(word) {
    const c = [...word];
    return c.length > 0 && !isHebrew(c[0].codePointAt(0));
  },
};

// ---- formatador emulado em JS, igual ao que o Rust faz ---------------------------------------------------------
function emulate(tag, type, style, items) {
  const set = lookupPattern(tag);
  const base = (TYPES.indexOf(type) * 3 + STYLES.indexOf(style)) * 8;
  const rules = lookupConditionals(tag);
  const patterns = patternPool[set];
  const literal = (slot, next) => {
    let text = patterns[base + slot];
    for (const [from, to, predicate] of rules) if (text === from && predicates[PREDICATES[predicate]](next)) { text = to; break; }
    return text;
  };
  const n = items.length;
  if (n === 0) return "";
  if (n === 1) return items[0];
  if (n === 2) return patterns[base] + items[0] + literal(1, items[1]) + items[1] + patterns[base + 2];
  let out = patterns[base + 3] + items[0] + patterns[base + 4] + items[1];
  for (let i = 2; i < n - 1; i++) out += patterns[base + 5] + items[i];
  return out + literal(6, items[n - 1]) + items[n - 1] + patterns[base + 7];
}

// Varredura dos predicados no bun: todo ponto de código como primeiro caractere, e as sequências curtas.
{
  const words = ["", "i", "I", "hi", "Hi", "hI", "HI", "hia", "hie", "hio", "hiu", "hii", "hiA", "hiE", "ia", "ie", "io", "o", "O", "ho", "Ho", "hO", "hoa", "8", "88", "8a", "11", "11 ", "11 mil", "110", "111", "1", "12", "x11", "h", "H", "hx", "u", "e", "y", "a", "ו", "אב", "ח", "日", "Ñ", "í", "í", "ó"];
  const cps = [];
  for (let cp = 0; cp <= 0x30000; cp++) if (cp < 0xd800 || cp > 0xdfff) cps.push(String.fromCodePoint(cp));
  const probes = [...words];
  for (const c of cps) { probes.push(c); }
  for (const pre of ["h", "H", "hi", "Hi", "ho", "i", "o", "8", "1", "11"]) for (const c of cps.slice(0, 0x2000)) probes.push(pre + c);
  for (const tag of ["es", "es-419", "es-MX", "he", "iw"]) {
    const canonical = Intl.getCanonicalLocales(tag)[0];
    for (const type of TYPES) for (const style of STYLES) {
      const formatter = new Intl.ListFormat(canonical, { type, style });
      for (const word of probes) {
        const items = ["z", word];
        const expected = formatter.format(items);
        if (word === "" && false) continue;
        const actual = emulate(canonical, type, style, items);
        if (actual !== expected) throw new Error(`predicado diverge: ${canonical} ${type} ${style} ${JSON.stringify(word)}: bun ${JSON.stringify(expected)}, emulado ${JSON.stringify(actual)}`);
      }
    }
  }
}

// ---- verificação geral: tags de amostra x combinações x listas -------------------------------------------------
let checked = 0;
{
  const sampleWords = ["a", "i", "hi", "hie", "o", "ho", "8", "11", "ו", "אב", "x", "Hi", "I", "O", "11 mil", "hia"];
  const tags = new Set([...tagToPattern.keys()].filter((_, i) => i % 7 === 0));
  for (const extra of ["xx", "xx-GB", "und", "und-GB", "en-US", "en-GB", "ca-ES-valencia", "zh-Hant-HK", "zh-Hans-CN", "sr-Latn-ME", "sr-ME", "sr-Cyrl-RS", "pt-AO", "pt-BR", "es-419", "es-ES", "he-IL", "es-US", "en-u-ca-gregory".split("-u-")[0], "fr-CA", "de-CH", "no-NO", "nb-NO", "zz", "en-ZZ", "az-Cyrl", "uz-Arab", "ff-Adlm-GN", "ha-Arab-NG", "yue-Hans", "yue-Hant-HK"]) tags.add(extra);
  for (const lang of langs) tags.add(lang);
  for (const tag of tags) {
    for (const type of TYPES) for (const style of STYLES) {
      const formatter = new Intl.ListFormat(tag, { type, style });
      for (let n = 0; n <= 5; n++) {
        for (let r = 0; r < (n >= 2 ? 3 : 1); r++) {
          const items = [];
          for (let i = 0; i < n; i++) items.push(i === n - 1 || i === 1 ? sampleWords[(checked + i * 5 + r * 3) % sampleWords.length] : "e" + i);
          const expected = formatter.format(items);
          const actual = emulate(tag, type, style, items);
          if (expected !== actual) throw new Error(`divergência: ${tag} ${type} ${style} ${JSON.stringify(items)}: bun ${JSON.stringify(expected)}, emulado ${JSON.stringify(actual)}`);
          checked++;
        }
      }
    }
  }
}

// ---- saída Rust ------------------------------------------------------------------------------------------------
// Escapa por ponto de código: um par de surrogates vira um único `\u{...}`, como o Rust exige.
const rustString = (text) => JSON.stringify(text).replace(/[\u{7f}-\u{10ffff}]/gu, (c) => `\\u{${c.codePointAt(0).toString(16)}}`);
const patternLines = patternPool.map((flat) => {
  const rows = [];
  for (let i = 0; i < 72; i += 4) rows.push("        " + flat.slice(i, i + 4).map(rustString).join(", ") + ",");
  return "    [\n" + rows.join("\n") + "\n    ],";
});
const sortedTags = [...tagToPattern.keys()].sort();
const tagLines = [];
for (let i = 0; i < sortedTags.length; i += 4) {
  tagLines.push("    " + sortedTags.slice(i, i + 4).map((tag) => `(${rustString(tag)}, ${tagToPattern.get(tag)})`).join(", ") + ",");
}
const sortedConditionalTags = [...tagToConditionals.keys()].sort();
const conditionalLines = sortedConditionalTags.map((tag) => {
  const rules = tagToConditionals.get(tag).map(([from, to, predicate]) => `(${rustString(from)}, ${rustString(to)}, Predicate::${PREDICATES[predicate]})`);
  return `    (${rustString(tag)}, &[${rules.join(", ")}]),`;
});
const rangeLines = [];
for (let i = 0; i < hebrewRanges.length; i += 6) rangeLines.push("    " + hebrewRanges.slice(i, i + 6).map(([lo, hi]) => `(0x${lo.toString(16)}, 0x${hi.toString(16)})`).join(", ") + ",");

writeRustSource(
  join(root, "src/runtime/intl_list_format_data.rs"),
  `//! Gerado por \`scripts/gen-list-format-patterns.js\` (bun 1.4.2, ICU completo): os padrões de \`Intl.ListFormat\`.
//! Não editar à mão. Cada conjunto de \`PATTERNS\` tem as nove combinações \`type\` x \`style\` (conjunction,
//! disjunction e unit, cada uma em long, short e narrow), e cada combinação quatro literais: pair, start, middle e
//! end. \`TAGS\` (ordenada) só lista as tags cujo conjunto difere do que a resolução por truncamento daria;
//! \`CONDITIONALS\` guarda as trocas de literal que dependem do próximo elemento (es, he); \`HEBREW_RANGES\` é o
//! script Hebrew do ICU, medido ponto de código a ponto.

/// O predicado, sobre o próximo elemento, que liga a troca de um literal por outro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Predicate {
    /// Começa com \`i\`, ou com \`hi\` que não seja \`hia\` nem \`hie\` (maiúsculas contam).
    SpanishE,
    /// Começa com \`o\`, \`ho\` ou \`8\`, ou é \`11\` seguido de espaço ou do fim.
    SpanishU,
    /// O primeiro caractere não é do script Hebrew.
    NotHebrew,
}

/// Oito textos por combinação, na ordem: antes e depois do par e o literal do par (\`pair\`); o que vem antes do
/// primeiro elemento, \`start\`, \`middle\`, \`end\` e o que vem depois do último. A combinação \`type\` x \`style\` fica no
/// índice \`(type * 3 + style) * 8\` (type: conjunction, disjunction, unit; style: long, short, narrow).
pub static PATTERNS: [[&str; 72];${patternPool.length}] = [
${patternLines.join("\n")}
];

pub static TAGS: [(&str, u16); ${sortedTags.length}] = [
${tagLines.join("\n")}
];

pub static CONDITIONALS: [(&str, &[(&str, &str, Predicate)]); ${sortedConditionalTags.length}] = [
${conditionalLines.join("\n")}
];

pub static HEBREW_RANGES: [(u32, u32); ${hebrewRanges.length}] = [
${rangeLines.join("\n")}
];
`,
);
console.log(`línguas ${langs.length}, conjuntos ${patternPool.length}, tags ${sortedTags.length}, condicionais ${sortedConditionalTags.length}, faixas hebraicas ${hebrewRanges.length}, casos conferidos ${checked}`);
