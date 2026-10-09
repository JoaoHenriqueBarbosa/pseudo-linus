# Intl.NumberFormat: unidades e moedas (estado em 2026-10-09)

## Achado

O PLAN.md está defasado. "31 de 45 unidades" já não vale: `UNITS` em `src/runtime/default_number_format.rs` tem 45 com
`mile-scandinavian` (acrescentada nesta fatia, medida no bun; teste `tests/number_unit_scandinavian_bun_golden.rs`).

O crate NÃO tem icu4x de unidades nem de moedas (só `icu_decimal`, `icu_plurals`, `icu_locale`...). `icu_experimental`
(unidades e `CurrencyFormatter`) não está no Cargo.toml nem no registry local, e exigiria mexer no Cargo.lock sem cargo.
Então o caminho é completar as tabelas medidas no bun, como o resto.

## O que ainda falta (medido)

1. Moedas em `en`: `KNOWN_CURRENCIES` (20), `currency_symbol` e `currency_name` em `default_number_format.rs` cobrem 20 de 307
   (`Intl.supportedValuesOf("currency")`). Os locales não en já têm moedas extras geradas
   (`ExtraLocale` / `extra_currency` em `icu_number_data.rs`, 26 línguas), `en` não. Em bun, `en` com `SEK` dá `SEK 5.00`, com
   `currencyDisplay: "name"` dá `Swedish kronor`.
2. Unidades fora do `en`: `scripts/gen-number-format-data.js` gera só 21 das 45 unidades por locale (lista `UNITS`).
   Falta ampliar a lista para as 45 (inclui `acre`, `hectare`, `fluid-ounce`, `gallon`, `stone`, `yard`, `kilobit`...,
   `mile-scandinavian`); `de` long `5 Kilometer pro Stunde` já sai, `de` `mile-scandinavian` long `2,5 skandinavische Meilen` não.
3. Moedas base (`CURRENCIES`, 12) têm as três formas de exibição; as outras 295 só têm símbolo, estreito e nome.

## Plano de fatias

- A: gerador ganha `EN` em `EXTRA_CODES` (símbolo, estreito, nome one/other por moeda, em `en`), saída como `ExtraLocale "en"`,
  e `currency_symbol`/`currency_name` passam a consultar `patterns::extra_currency("en", code)` antes de `None`.
- B: `UNITS` do gerador vira `Intl.supportedValuesOf("unit")` completo, regenera `icu_number_data.rs`
  (`bun scripts/gen-number-format-data.js`, que também refaz `tests/golden/number_format_more_bun.tsv`).
- C: golden novo `number_currency_en_bun.tsv` com as 307 moedas (symbol, narrowSymbol, code, name, 1 e 2.5).
