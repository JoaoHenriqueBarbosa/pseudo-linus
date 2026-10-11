// Gera src/runtime/icu_number_data.rs e tests/golden/number_format_more_bun.tsv a partir do bun (o oráculo):
// os padrões de moeda, de percentual e de unidade do Intl.NumberFormat por língua, que o icu_decimal não traz.
// Uso (da raiz do crate): bun scripts/gen-number-format-data.js
//
// Cada padrão sai como uma sequência de símbolos separados por U+001F: o primeiro caractere é o tipo e o resto
// é o texto. Tipos: `n` número (os dígitos, grupos e decimal ficam por conta do DecimalFormat), `l` literal,
// `c` moeda, `u` unidade, `-` sinal (o do locale, com as marcas bidirecionais), `%` sinal de percentual.
const fs = require("fs");
const path = require("path");
const { writeRustSource } = require("./rust-escape.js");

const root = path.join(__dirname, "..");
const harness = fs.readFileSync(path.join(root, "tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const BASE_LOCALES = ["pt", "es", "fr", "de", "it", "ja", "ru", "ar", "hi", "zh", "ko", "fa", "th"];
const EXTRA_LOCALES = ["fr-CA", "de-CH", "es-MX", "zh-TW", "pt-PT", "es-AR", "fr-CH", "de-AT", "ar-EG", "en-GB", "en-IN", "en-AU", "ar-SA", "tr", "pl", "nl", "sv", "he", "da", "nb", "fi", "cs", "el", "id", "vi", "uk", "zh-HK", "en-CA"];
const LOCALES = [...BASE_LOCALES, ...EXTRA_LOCALES];
const CURRENCIES = ["USD", "EUR", "BRL", "JPY", "GBP", "CNY", "INR", "RUB", "KRW", "MXN", "CHF", "CAD"];
const CORE_UNITS = [
  "kilometer", "meter", "kilogram", "liter", "second", "minute", "hour", "day", "percent", "celsius", "byte",
  "kilobyte", "megabyte", "gigabyte", "kilometer-per-hour", "meter-per-second", "mile", "year", "month", "week", "fahrenheit",
];
// Todas as unidades simples sancionadas (Intl.supportedValuesOf("unit") no bun), depois as 21 de sempre.
const UNITS = [...CORE_UNITS, ...Intl.supportedValuesOf("unit").filter((unit) => !CORE_UNITS.includes(unit))];
const UNIT_DISPLAYS = ["long", "short", "narrow"];
const CURRENCY_DISPLAYS = ["symbol", "narrowSymbol", "code"];
const SAMPLES = [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 20, 21, 22, 100, 101, 102, 1000, 1000000, 0.5, 1.5, 2.5];
const SEPARATOR = "\u001f";
const MARKS = /[‎‏؜]+$/;

function digitsOf(currency) {
  return new Intl.NumberFormat("en", { style: "currency", currency }).resolvedOptions().maximumFractionDigits;
}

/** As partes do formatToParts como um padrão: o número vira `n`, o sinal `-`, e assim por diante. */
function template(parts) {
  const tokens = [];
  const push = (kind, text) => {
    const last = tokens[tokens.length - 1];
    if (last && last.kind === kind && (kind === "l" || kind === "n")) last.text += text;
    else tokens.push({ kind, text });
  };
  for (const { type, value } of parts) {
    switch (type) {
      case "integer": case "group": case "decimal": case "fraction": push("n", ""); break;
      case "literal": push("l", value); break;
      case "currency": push("c", value); break;
      case "unit": push("u", value); break;
      case "percentSign": push("%", value); break;
      case "minusSign": case "plusSign": {
        const last = tokens[tokens.length - 1];
        if (last && last.kind === "l") {
          last.text = last.text.replace(MARKS, "");
          if (last.text === "") tokens.pop();
        }
        push("-", "");
        break;
      }
      default: throw new Error(`parte inesperada ${type}`);
    }
  }
  return tokens.map((token) => token.kind + token.text).join(SEPARATOR);
}

/** Literal de string Rust: ASCII imprimível como está, o resto como `\u{...}`. */
function rs(text) {
  let out = '"';
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (ch === '"' || ch === "\\") out += "\\" + ch;
    else if (code >= 0x20 && code < 0x7f) out += ch;
    else out += code < 0x80 ? `\\x${code.toString(16).padStart(2, "0")}` : `\\u{${code.toString(16)}}`;
  }
  return out + '"';
}

/** Literal de string Rust compacto: o texto legível sai como está, o invisível (marcas, espaços especiais) escapado. */
function rsRaw(text) {
  let out = '"';
  for (const ch of text) {
    if (ch === '"' || ch === "\\") out += "\\" + ch;
    else if (ch === " " || (!/[\p{Cc}\p{Cf}\p{Zs}\p{Zl}\p{Zp}]/u.test(ch))) out += ch;
    else out += ch.codePointAt(0) < 0x80 ? `\\x${ch.codePointAt(0).toString(16).padStart(2, "0")}` : `\\u{${ch.codePointAt(0).toString(16)}}`;
  }
  return out + '"';
}

const currencyRows = [];
const accountingPositiveRows = [];
const currencyNameRows = [];
const unitRows = [];
const percentRows = [];
const percentCompactRows = [];

for (const locale of LOCALES) {
  // Percentual.
  percentRows.push(
    `    PercentEntry { locale: ${rs(locale)}, template: ${rs(template(new Intl.NumberFormat(locale, { style: "percent" }).formatToParts(0.5)))} },`,
  );
  // Percentual compacto: o ICU usa o padrão da unidade `percent` (espaço comum em fr/de, `pct.` em da...).
  percentCompactRows.push(
    `    PercentEntry { locale: ${rs(locale)}, template: ${rs(template(new Intl.NumberFormat(locale, { style: "percent", notation: "compact" }).formatToParts(0.5)))} },`,
  );

  for (const currency of CURRENCIES) {
    // Símbolo, símbolo estreito e código.
    let symbolRow = null;
    for (const [index, display] of CURRENCY_DISPLAYS.entries()) {
      const plain = new Intl.NumberFormat(locale, { style: "currency", currency, currencyDisplay: display });
      const accounting = new Intl.NumberFormat(locale, { style: "currency", currency, currencyDisplay: display, currencySign: "accounting" });
      const row = [template(plain.formatToParts(1234.5)), template(plain.formatToParts(-1234.5)), template(accounting.formatToParts(-1234.5))];
      // O positivo contábil quase sempre é o comum, mas não em ar (marca ALM no lugar da RLM), fa e nb.
      const accountingPositive = template(accounting.formatToParts(1234.5));
      if (accountingPositive !== row[0]) {
        accountingPositiveRows.push(`    AccountingPositiveEntry { locale: ${rs(locale)}, currency: ${rs(currency)}, display: ${index}, template: ${rs(accountingPositive)} },`);
      }
      if (index === 1 && symbolRow && row.every((value, i) => value === symbolRow[i])) continue;
      if (index === 0) symbolRow = row;
      currencyRows.push(
        `    CurrencyEntry { locale: ${rs(locale)}, currency: ${rs(currency)}, display: ${index}, positive: ${rs(row[0])}, negative: ${rs(row[1])}, accounting: ${rs(row[2])} },`,
      );
    }

    // Nome, por categoria de plural (o número visível leva as casas da moeda).
    const digits = digitsOf(currency);
    const rules = new Intl.PluralRules(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits });
    const format = new Intl.NumberFormat(locale, { style: "currency", currency, currencyDisplay: "name" });
    const seen = new Map();
    for (const sample of SAMPLES) {
      const category = rules.select(sample);
      if (seen.has(category)) continue;
      seen.set(category, [template(format.formatToParts(sample)), template(format.formatToParts(-sample))]);
    }
    const other = seen.get("other");
    for (const [category, row] of seen) {
      if (category !== "other" && other && row[0] === other[0] && row[1] === other[1]) continue;
      currencyNameRows.push(
        `    CurrencyNameEntry { locale: ${rs(locale)}, currency: ${rs(currency)}, category: ${rs(category)}, positive: ${rs(row[0])}, negative: ${rs(row[1])} },`,
      );
    }
  }

  collectUnits(locale);
}
// O `en` só entra na tabela de unidades (moeda e percentual dele vêm das tabelas à mão de `default_number_format`).
collectUnits("en");

// Unidades simples sanctioned e kilometer-per-hour de um locale.
function collectUnits(locale) {
  for (const unit of UNITS) {
    for (const [index, display] of UNIT_DISPLAYS.entries()) {
      const rules = new Intl.PluralRules(locale);
      const format = new Intl.NumberFormat(locale, { style: "unit", unit, unitDisplay: display });
      const seen = new Map();
      for (const sample of SAMPLES) {
        const category = rules.select(sample);
        if (!seen.has(category)) seen.set(category, template(format.formatToParts(sample)));
      }
      const other = seen.get("other");
      for (const [category, row] of seen) {
        if (category !== "other" && row === other) continue;
        unitRows.push(
          `    UnitEntry { locale: ${rs(locale)}, unit: ${rs(unit)}, display: ${index}, category: ${rs(category)}, template: ${rs(row)} },`,
        );
      }
    }
  }
}

// Padrão "por unidade" (perUnitPattern do CLDR): para cada denominador, o sufixo que o ICU põe depois do
// texto do numerador (`kilometer` -> `km`, `kilometer-per-hour` -> `km/h`, sufixo `/h`). Medido com o numerador
// `kilometer`; locale em que o composto não começa pelo texto do numerador não tem entrada.
const perUnitRows = [];
const DENOMINATORS = Intl.supportedValuesOf("unit");
for (const locale of ["en", ...BASE_LOCALES]) {
  for (const denominator of DENOMINATORS) {
    for (const [index, display] of UNIT_DISPLAYS.entries()) {
      const unitText = (unit) => new Intl.NumberFormat(locale, { style: "unit", unit, unitDisplay: display }).formatToParts(1).find((part) => part.type === "unit").value;
      const numerator = unitText("kilometer");
      const composite = unitText(`kilometer-per-${denominator}`);
      if (!composite.startsWith(numerator)) continue;
      perUnitRows.push(`    PerUnitEntry { locale: ${rs(locale)}, denominator: ${rs(denominator)}, display: ${index}, suffix: ${rs(composite.slice(numerator.length))} },`);
    }
  }
}

// Unidades compostas com padrão próprio no CLDR (`mile-per-hour` short é `mph`, não `mi/h`): para cada par
// numerador-per-denominador que o JS aceita (produto cartesiano das unidades simples), o texto inteiro da parte
// `unit` em `en`, por unitDisplay, para o valor 1 (singular) e 2 (plural). Só entra o par cujo texto difere de
// numerador + sufixo (o que `default_number_format` monta com `PER_UNITS`).
const compoundRows = [];
{
  const unitPart = (unit, display, value) =>
    new Intl.NumberFormat("en", { style: "unit", unit, unitDisplay: display }).formatToParts(value).find((part) => part.type === "unit").value;
  const suffixOf = new Map();
  for (const denominator of DENOMINATORS) {
    for (const [index, display] of UNIT_DISPLAYS.entries()) {
      const numerator = unitPart("kilometer", display, 1);
      const composite = unitPart(`kilometer-per-${denominator}`, display, 1);
      suffixOf.set(`${denominator}|${index}`, composite.startsWith(numerator) ? composite.slice(numerator.length) : null);
    }
  }
  for (const numerator of DENOMINATORS) {
    for (const denominator of DENOMINATORS) {
      if (numerator === denominator) continue;
      for (const [index, display] of UNIT_DISPLAYS.entries()) {
        let measured;
        try {
          measured = [1, 2].map((value) => unitPart(`${numerator}-per-${denominator}`, display, value));
        } catch {
          continue;
        }
        const suffix = suffixOf.get(`${denominator}|${index}`);
        const built = [1, 2].map((value) => (suffix === null ? null : unitPart(numerator, display, value) + suffix));
        if (measured[0] === built[0] && measured[1] === built[1]) continue;
        compoundRows.push(
          `    CompoundUnitEntry { numerator: ${rs(numerator)}, denominator: ${rs(denominator)}, display: ${index}, one: ${rs(measured[0])}, other: ${rs(measured[1])} },`,
        );
      }
    }
  }
}

// Moedas fora de CURRENCIES (as de Intl.supportedValuesOf): só o que difere do código, em formato compacto.
// O padrão (posição do símbolo, espaço) vem de CurrencyBase, um por forma distinta no locale (sem o texto da
// moeda, que quem renderiza troca). Os textos ficam numa tabela de strings únicas (STRINGS, índices u16),
// compartilhada entre locales; as moedas de cada locale saem agrupadas e ordenadas por código.
const EXTRA_CODES = Intl.supportedValuesOf("currency").filter((code) => !CURRENCIES.includes(code)).sort();
const baseRows = [];
const extraGroups = [];
let extraCount = 0;
const stringIndex = new Map([["", 0]]);
const strings = [""];
const intern = (text) => {
  if (!stringIndex.has(text)) {
    stringIndex.set(text, strings.length);
    strings.push(text);
  }
  return stringIndex.get(text);
};
const stripCurrency = (row) => row.map((value) => value.split(SEPARATOR).map((token) => (token[0] === "c" ? "c" : token)).join(SEPARATOR));
// `en` entra só com símbolo, estreito e nomes (consultados por `default_number_format`); sem padrões base,
// porque os padrões de `en` ficam nas tabelas à mão e não em `CURRENCY_BASES`.
for (const locale of ["en", ...LOCALES]) {
  const bases = new Map();
  const baseId = (row) => {
    if (locale === "en") return 0;
    const stripped = stripCurrency(row);
    const key = stripped.join("\u0000");
    if (!bases.has(key)) {
      if (bases.size > 255) throw new Error("bases demais");
      bases.set(key, bases.size);
      baseRows.push(`    CurrencyBase { locale: ${rs(locale)}, id: ${bases.size - 1}, positive: ${rsRaw(stripped[0])}, negative: ${rsRaw(stripped[1])}, accounting: ${rsRaw(stripped[2])} },`);
    }
    return bases.get(key);
  };
  const probe = (code, display) => {
    const plain = new Intl.NumberFormat(locale, { style: "currency", currency: code, currencyDisplay: display });
    const accounting = new Intl.NumberFormat(locale, { style: "currency", currency: code, currencyDisplay: display, currencySign: "accounting" });
    const parts = plain.formatToParts(1234.5);
    return {
      text: parts.find((part) => part.type === "currency").value,
      row: [template(parts), template(plain.formatToParts(-1234.5)), template(accounting.formatToParts(-1234.5))],
    };
  };
  const group = [];
  for (const code of EXTRA_CODES) {
    const sym = probe(code, "symbol");
    const nar = probe(code, "narrowSymbol");
    const symbol = sym.text === code ? "" : sym.text;
    const narrow = nar.text === sym.text ? "" : nar.text;
    const symBase = symbol ? baseId(sym.row) : 0;
    const narBase = narrow ? baseId(nar.row) : 0;
    const digits = digitsOf(code);
    const rules = new Intl.PluralRules(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits });
    const format = new Intl.NumberFormat(locale, { style: "currency", currency: code, currencyDisplay: "name" });
    const names = new Map();
    for (const sample of SAMPLES) {
      const category = rules.select(sample);
      if (!names.has(category)) names.set(category, format.formatToParts(sample).find((part) => part.type === "currency").value);
    }
    const other = names.get("other") ?? "";
    const name = other === code ? "" : other;
    // Plural igual ao `other` não precisa de entrada (já é `!== other` abaixo).
    const plurals = [...names].filter(([category, text]) => category !== "other" && text !== other).map(([category, text]) => `${category}=${text}`).join(SEPARATOR);
    if (!symbol && !narrow && !name && !plurals) continue;
    group.push(`        extra(${rs(code)}, ${intern(symbol)}, ${intern(narrow)}, ${symBase}, ${narBase}, ${intern(name)}, ${intern(plurals)}),`);
  }
  extraCount += group.length;
  extraGroups.push(`    ExtraLocale { locale: ${rs(locale)}, entries: &[\n${group.join("\n")}\n    ] },`);
}
if (strings.length > 65535) throw new Error("strings demais para u16");
const stringRows = strings.map((text) => `    ${rsRaw(text)},`);
const extraRows = { length: extraCount };

// Contábil: se o negativo de `currencySign: "accounting"` usa parênteses, por locale x currencyDisplay
// (0 symbol, 1 narrowSymbol, 2 code, 3 name) x notação (padrão ou compacta). Medido com várias moedas, que
// têm de concordar. `en` entra aqui mesmo sem padrões de moeda, porque a consulta cai nele.
const accountingRows = [];
for (const locale of ["en", ...LOCALES]) {
  for (const [index, display] of [...CURRENCY_DISPLAYS, "name"].entries()) {
    for (const compact of [false, true]) {
      const answers = new Set(
        ["USD", "EUR", "BRL", "JPY"].map((currency) =>
          /[()]/.test(new Intl.NumberFormat(locale, { style: "currency", currency, currencySign: "accounting", currencyDisplay: display, ...(compact ? { notation: "compact" } : {}) }).format(-1234.5))),
      );
      if (answers.size !== 1) throw new Error(`contábil ${locale} ${display} sem consenso entre moedas`);
      accountingRows.push(`    AccountingEntry { locale: ${rs(locale)}, display: ${index}, compact: ${compact}, parentheses: ${[...answers][0]} },`);
    }
  }
}

// Espaço entre moeda e número que o ICU insere (currencySpacing) e que some com Infinity e NaN, porque o
// `∞` e o `NaN` não são dígitos: por locale x currencySign, medido em todas as moedas e nos três
// currencyDisplay de símbolo. O espaço que já está no padrão (pt, de, fr...) fica. Os dois comportamentos no
// mesmo locale e sinal seriam um erro de modelo, então não há consenso = falha.
const spacingRows = [];
{
  const isSpace = (part) => part && part.type === "literal" && /^[   ]$/.test(part.value);
  const around = (parts) => {
    const at = parts.findIndex((part) => part.type === "currency");
    return [parts[at - 1], parts[at + 1]].filter(isSpace).length;
  };
  for (const locale of ["en", ...LOCALES]) {
    for (const sign of ["standard", "accounting"]) {
      const answers = new Set();
      for (const currency of [...CURRENCIES, ...EXTRA_CODES]) {
        for (const display of CURRENCY_DISPLAYS) {
          const format = new Intl.NumberFormat(locale, { style: "currency", currency, currencyDisplay: display, currencySign: sign });
          for (const [finite, special] of [[1234.5, Infinity], [-1234.5, -Infinity], [1234.5, NaN]]) {
            const spaces = around(format.formatToParts(finite));
            if (spaces === 0) continue;
            answers.add(around(format.formatToParts(special)) < spaces);
          }
        }
      }
      if (answers.size > 1) throw new Error(`espaçamento de moeda ${locale} ${sign} sem consenso`);
      spacingRows.push(`    CurrencySpacingEntry { locale: ${rs(locale)}, accounting: ${sign === "accounting"}, inserted: ${[...answers][0] ?? false} },`);
    }
  }
}

// Intervalos (formatRange): o separador do padrão de intervalo e o sinal de aproximado, por locale.
const rangeRows = [];
for (const locale of ["en", ...LOCALES]) {
  const decimal = new Intl.NumberFormat(locale);
  const separator = decimal.formatRangeToParts(3, 5).find((part) => part.type === "literal" && part.source === "shared").value;
  const approx = decimal.formatRangeToParts(5, 5).find((part) => part.type === "approximatelySign").value;
  rangeRows.push(`    RangeEntry { locale: ${rs(locale)}, separator: ${rsRaw(separator)}, approximately: ${rsRaw(approx)} },`);
}

const header = `//! Padrões de moeda, de percentual e de unidade do \`Intl.NumberFormat\` por língua, medidos no bun (JavaScriptCore/ICU).
//!
//! ARQUIVO GERADO, não edite à mão. Para regenerar (da raiz do crate, com o bun no PATH):
//!
//! \`\`\`sh
//! bun scripts/gen-number-format-data.js
//! \`\`\`
//!
//! O mesmo comando refaz \`tests/golden/number_format_more_bun.tsv\`. Os padrões são lidos por
//! \`icu_number_patterns\`: cada um é uma sequência de símbolos separados por U+001F, o primeiro caractere
//! de cada símbolo é o tipo (\`n\` número, \`l\` literal, \`c\` moeda, \`u\` unidade, \`-\` sinal, \`%\` percentual)
//! e o resto é o texto.

use crate::runtime::icu_number_patterns::{
    AccountingEntry, AccountingPositiveEntry, CompoundUnitEntry, CurrencyBase, CurrencyEntry, CurrencyNameEntry, CurrencySpacingEntry, ExtraLocale, PerUnitEntry, PercentEntry, RangeEntry, UnitEntry, extra,
};

`;

const out =
  header +
  `pub static PERCENTS: &[PercentEntry] = &[\n${percentRows.join("\n")}\n];\n\n` +
  `/// Percentual na notação compacta (o padrão da unidade \`percent\`), por locale.\npub static PERCENTS_COMPACT: &[PercentEntry] = &[\n${percentCompactRows.join("\n")}\n];\n\n` +
  `pub static CURRENCIES: &[CurrencyEntry] = &[\n${currencyRows.join("\n")}\n];\n\n` +
  `pub static CURRENCY_NAMES: &[CurrencyNameEntry] = &[\n${currencyNameRows.join("\n")}\n];\n\n` +
  `pub static UNITS: &[UnitEntry] = &[\n${unitRows.join("\n")}\n];\n\n` +
  `/// Sufixos "por unidade" por (locale, denominador, unitDisplay).\npub static PER_UNITS: &[PerUnitEntry] = &[\n${perUnitRows.join("\n")}\n];\n\n` +
  `/// Unidades compostas (\`en\`) cujo texto difere de numerador + sufixo, por (numerador, denominador, unitDisplay).\npub static COMPOUND_UNITS: &[CompoundUnitEntry] = &[\n${compoundRows.join("\n")}\n];\n\n` +
  `/// Moedas de \`Intl.supportedValuesOf("currency")\` fora de \`CURRENCIES\`, ordenadas por (locale, moeda).\n` +
  `pub static EXTRA_CURRENCIES: &[ExtraLocale] = &[\n${extraGroups.join("\n")}\n];\n\n` +
  `/// Textos únicos das moedas extras, por índice (o 0 é o texto vazio).\n` +
  `pub static STRINGS: &[&str] = &[\n${stringRows.join("\n")}\n];\n\n` +
  `pub static CURRENCY_BASES: &[CurrencyBase] = &[\n${baseRows.join("\n")}\n];\n\n` +
  `pub static RANGES: &[RangeEntry] = &[\n${rangeRows.join("\n")}\n];\n\n` +
  `/// Se o negativo contábil usa parênteses, por (locale, currencyDisplay, notação).\npub static ACCOUNTING: &[AccountingEntry] = &[\n${accountingRows.join("\n")}\n];\n\n` +
  `/// Padrão do positivo contábil nas moedas em que ele difere do positivo comum, por (locale, moeda, currencyDisplay).\npub static ACCOUNTING_POSITIVES: &[AccountingPositiveEntry] = &[\n${accountingPositiveRows.join("\n")}\n];\n\n` +
  `/// Se o espaço entre moeda e número é o inserido pelo ICU (some com Infinity e NaN), por (locale, currencySign).\npub static CURRENCY_SPACING: &[CurrencySpacingEntry] = &[\n${spacingRows.join("\n")}\n];\n`;
writeRustSource(path.join(root, "src/runtime/icu_number_data.rs"), out);

// ---------------------------------------------------------------------------------------------
// Golden: programas de uma linha, avaliados pelo mesmo harness de tests/regexp_bun_golden.rs.
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
// Amostra das moedas de Intl.supportedValuesOf("currency") fora de CURRENCIES: uma a cada doze, em ordem.
const GOLDEN_EXTRA_CURRENCIES = Intl.supportedValuesOf("currency").filter((code) => !CURRENCIES.includes(code)).filter((_, i) => i % 12 === 0);
const programs = [];
const nf = (locale, options, value) => `new Intl.NumberFormat(${q(locale)}, ${JSON.stringify(options)}).format(${value})`;
for (const locale of LOCALES) {
  programs.push(nf(locale, { style: "percent" }, 0.125));
  programs.push(nf(locale, { style: "percent" }, -0.5));
  for (const currency of CURRENCIES) {
    for (const display of [...CURRENCY_DISPLAYS, "name"]) {
      for (const value of display === "name" ? [1, 2, 5, 1234.5, -1] : [1234.5, -1234.5, 0]) {
        programs.push(nf(locale, { style: "currency", currency, currencyDisplay: display }, value));
      }
    }
    programs.push(nf(locale, { style: "currency", currency, currencySign: "accounting" }, -1234.5));
    programs.push(nf(locale, { style: "currency", currency, signDisplay: "always" }, 3));
  }
  programs.push(
    `JSON.stringify(new Intl.NumberFormat(${q(locale)}, {style:"currency",currency:"EUR"}).formatToParts(-1234.5))`,
  );
  for (const code of GOLDEN_EXTRA_CURRENCIES) {
    for (const display of [...CURRENCY_DISPLAYS, "name"]) {
      for (const value of display === "name" ? [1, 2, 1234.5] : [1234.5, -1]) {
        programs.push(nf(locale, { style: "currency", currency: code, currencyDisplay: display }, value));
      }
    }
  }
  goldenUnits(locale);
}
goldenUnits("en");
function goldenUnits(locale) {
  for (const unit of UNITS) {
    for (const display of UNIT_DISPLAYS) {
      for (const value of CORE_UNITS.includes(unit) ? [1, 2, 5, 1234.5, -3] : [1, 2, 1234.5]) {
        programs.push(nf(locale, { style: "unit", unit, unitDisplay: display }, value));
      }
    }
  }
}
const seen = new Set();
const lines = [];
for (const src of programs) {
  if (seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  const result = (0, eval)(`${harness}(${JSON.stringify(src)})`);
  if (typeof result !== "string") throw new Error(`${src}: o harness não devolveu string`);
  lines.push(`${src}\t${result}`);
}
fs.writeFileSync(path.join(root, "tests/golden/number_format_more_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.error(`${extraRows.length} moedas extras, ${rangeRows.length} intervalos, ${currencyRows.length} moedas, ${currencyNameRows.length} nomes, ${unitRows.length} unidades, ${lines.length} casos`);
