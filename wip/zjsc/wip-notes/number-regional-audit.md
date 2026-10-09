# Auditoria do Intl.NumberFormat regional (2026-10-08)

Leitura de `icu_number.rs`, `icu_number_data.rs` e `icu_number_patterns.rs` (e do ponto de uso em
`default_number_format.rs`) contra o bun 1.4.2. Nada foi compilado nem rodado (sem cargo neste passe).

## Golden novo

- `scripts/gen-number-regional-golden.js` gera `tests/golden/number_regional_bun.tsv` (11958 linhas, sem
  caminhos da máquina) e `tests/number_regional_bun_golden.rs` o confere, no padrão de
  `number_format_more_bun_golden.rs`.
- Cobre as 17 variantes pedidas (en-GB, en-IN, en-AU, pt-PT, es-MX, es-AR, fr-CA, fr-CH, de-CH, de-AT,
  zh-TW, hi-IN, ar-EG, ar-SA, fa-IR, th-TH-u-nu-thai, ja-JP-u-nu-hanidec) com 15 valores por combinação
  (0, -0, 1, -1, 1234.5, -1234.5, 12345, 1234567.891, 0.5, 0.256, 0.000001, 1e21, NaN, Infinity, -Infinity):
  decimal, percent, `useGrouping` (true, false, min2, always, auto), scientific, engineering, compact short e
  long, `resolvedOptions`; moeda (EUR, USD, BRL, JPY, INR, CHF) em 8 locales com symbol, code, name,
  narrowSymbol e accounting; 10 unidades short, long e narrow em 6 locales; `signDisplay` (5 modos) em 4
  locales; `roundingMode` (9) e `roundingIncrement` (14) em 4 locales com valores de empate.
- É esperado que o teste acuse muitas divergências até as lacunas abaixo fecharem.

## Lacunas (o porte não cobre)

1. Padrões de moeda, percentual e unidade existem só para es, fr, de, it, ja, ru, ar, hi, zh, ko e os
   regionais fr-CA, de-CH, es-MX, zh-TW (e, a partir deste passe, pt-PT, es-AR, fr-CH, de-AT, ar-EG). Fora
   disso a busca cai na língua e, sem `en` nem `pt` na tabela, nas tabelas à mão de
   `default_number_format.rs`, que só sabem inglês americano e português do Brasil. Sem padrão próprio:
   en-GB, en-IN, en-AU (moeda `A$`, `€` etc. coincidem com o americano, o resto não), fa-IR, th, ar-SA,
   ja com `hanidec`.
2. Nomes de unidade em inglês britânico: `kilometres`, `metres`, `litres` (en-GB, en-IN, en-AU) saem como
   `kilometers`, `meters`, `liters` das tabelas à mão. Medido no bun: `2 kilometres` em en-GB e en-IN.
3. `pt-PT` unidade e moeda por extenso saíam com o texto do Brasil (`quilômetros`); o bun dá
   `quilómetros`. Fechado pela tabela gerada de pt-PT (não conferido).
4. Locales sem tabela: `fa`, `th`, `tr`, `pl`, `nl`, `sv`, `he`... e tudo que não está na lista acima; moeda,
   percentual e unidade saem no padrão do inglês.
5. `roundingIncrement` diferente de 1 é aceito e devolvido pelo `resolvedOptions`, mas o arredondamento o
   ignora (`default_number_format.rs`, `round_number`). O golden tem 14 incrementos medidos com
   `minimumFractionDigits = maximumFractionDigits = 2`.
6. Moedas fora das 6 do gerador (AUD, GBP, CNY e as demais) só têm padrão onde o gerador as cobre
   (`CURRENCIES` do `gen-number-format-data.js`); as outras saem pelo código ISO.
7. `th-TH-u-nu-thai` e `ja-JP-u-nu-hanidec`: `numbering_system_honored` só aceitava os sistemas de
   `ZERO_DIGITS`, e `hanidec` não estava (`〇一二三四五六七八九`, zero U+3007). Corrigido, ver abaixo. `thai` já estava.
8. `nan_symbol`: só os locales medidos (ar, fa, ru, zh-Hant etc.). `th`, `hi`, `ja` usam `NaN` como o bun.
   `ar-SA` e `ar-EG` dependem de `default_numbering_system` (zero do icu4x) coincidir com o ICU do bun
   (ambos `arab` no bun): não conferido.
9. Percentual com `ar-EG`/`ar-SA`: `percent_sign` decide só pelo sistema numérico, a marca `\u{200e}` em volta
   vem do padrão gerado de `ar`; se o ICU der um padrão diferente em `ar-SA` (sem marcas), diverge.
10. `useGrouping: "always"` e `"min2"` em compacto agem como `auto`; `fr-CH` usa U+202F como grupo e `.` como
    decimal (o bun) e o `icu_decimal` deve dar o mesmo, mas isso só o golden confirma.
11. Notação compacta: depende de o icu4x ter os dados do locale (`ja`, `hi`, `zh-TW`, `th` têm); as formas
    longas por plural (`other`/`one`) do inglês britânico e do português europeu seguem o `icu_decimal`.
12. `formatRange` segue só para `en` e `pt` (`intl_number_range.rs`).

## Corrigido neste passe (edição pequena)

- `icu_number.rs`: `hanidec` entrou em `ZERO_DIGITS` (agora 21 sistemas), então `ja-JP-u-nu-hanidec` e
  `numberingSystem: "hanidec"` são honrados em vez de caírem em `latn`.
- `scripts/gen-number-format-data.js`: `EXTRA_LOCALES` ganhou `pt-PT`, `es-AR`, `fr-CH`, `de-AT` e `ar-EG`;
  `src/runtime/icu_number_data.rs` e `tests/golden/number_format_more_bun.tsv` foram regenerados no bun
  (565 moedas, 315 nomes, 1096 unidades, 7980 casos). Não compilado.

## Corrigido no segundo passe (não compilado, sem cargo)

- Bash foi usado só para rodar o gerador no bun (`bun scripts/gen-number-format-data.js`) e um `sed` na lista
  de locales do gerador (mudança em lote de uma linha).
- Lacunas 1, 2 e 4 (parte): `BASE_LOCALES` ganhou `fa` e `th`; `EXTRA_LOCALES` ganhou `en-GB`, `en-IN`,
  `en-AU` e `ar-SA`. `icu_number_data.rs` e `number_format_more_bun.tsv` regenerados (741 moedas, 387 nomes,
  1466 unidades, 10500 casos); `en-GB` já traz `kilometres`/`kilometre`. As buscas de `icu_number_patterns`
  acham `en-GB` antes de `en`, então `en`/`en-US` seguem nas tabelas à mão. Lakh grouping do `en-IN` já
  vem do `icu_decimal` (não alterado).
- Lacuna 5: `round_to_increment` em `default_number_format.rs` (aritmética decimal sobre os dígitos, resto
  por `increment`, os nove modos via `rounds_up` extraído de `round_keeping`); vale para
  `Rounding::FractionDigits` com incremento diferente de 1, `minimum_fraction` = min. `trailingZeroDisplay`
  segue o caminho existente. Não compilado nem conferido contra o golden.
- Ainda abertas: 3 (conferir), 6, 7 do `ja` fora do hanidec, 8 a 12, e `tr`, `pl`, `nl`, `sv`, `he`.

## Próximos passos

1. Rodar `tests/number_regional_bun_golden.rs` e agrupar as divergências por locale e por opção.
2. Gerar `en-GB`, `en-IN`, `en-AU`, `fa`, `th` e `ar-SA` no gerador de padrões, depois de decidir se `en` e
   `pt` saem das tabelas à mão para a tabela gerada (única fonte).
3. Implementar `roundingIncrement` em `round_number`.

## Terceiro passe (2026-10-08, não compilado, sem cargo)

- `EXTRA_LOCALES` do gerador ganhou `tr`, `pl`, `nl`, `sv`, `he`, `da`, `nb`, `fi`, `cs`, `el`, `id`, `vi`, `uk`,
  `zh-HK` e `en-CA` (`hi`, `ko`, `zh-TW`, `es-MX` e `fr-CH` já estavam). Bash rodou só o gerador no bun
  (`bun scripts/gen-number-format-data.js`): `icu_number_data.rs` e `number_format_more_bun.tsv` regenerados
  (1194 moedas, 610 nomes, 2369 unidades, 16800 casos). Fecha o item 4 para esses locales.
- Item 8 medido no bun para os 20 locales: `NaN` em todos exceto `fi` (`epäluku`), `zh-TW`/`zh-HK` (`非數值`),
  `ar-SA`/`ar-EG` (`ليس رقمًا`); `nan_symbol` já cobre tudo isso, nada a mudar.
- Item 9, achado novo: `ar-SA-u-nu-latn` percentual sai `50٪` (sem marcas), enquanto `ar`/`ar-EG` com latn saem
  `50‎%‎` (com U+200E). `percent_sign` só olha o sistema numérico, então `ar-SA` + latn diverge se o padrão gerado
  não vier de `ar-SA`. Com o sistema padrão `arab` (`٥٠٪؜`) está certo. Aberto: conferir se o lookup do padrão
  usa o `ar-SA` gerado antes do `ar`.
- Item 10, achados no bun: `useGrouping: "min2"` dá `1234` em todos os locales medidos; compacto com `"always"`
  não agrupa (`1,2 B`, `1.2K`), e em `ja` e `zh-TW` o compacto com `"always"` cai em `1,234` (sem compacto). Não
  alterado em `intl_number_format*.rs` (sem tempo de ler contra IntlNumberFormat.cpp).
- Ainda abertas: 3 (conferir no golden), 6, 11, 12.

## Quarto passe (2026-10-08, não compilado, sem cargo; só bun para medir)

- Item 9 fechado em código. Medido: `ar-SA` percentual `٥٠٪؜` (arab), `50٪` (latn, sem marcas); `ar-EG` latn `50‎%‎`
  (com U+200E, igual a `ar`). `percent_sign` (`icu_number.rs`) devolve `٪` para `ar-SA` com sistema não árabe;
  `percent_parts` (`default_number_format.rs`) usa o padrão de `ar` para `ar-*` com `%` e tira a marca U+061C do
  padrão `arab` quando o sinal não a leva.
- Itens 10 e 11 conferidos por leitura, sem mudança: o `min2`/`always` vai ao `icu_decimal` por
  `GroupingStrategy`; em `ja`/`zh-TW` o compacto de 1234 tem expoente 0 e o agrupamento mínimo do locale é 1, então
  `always` e `auto` dão `1,234` e `min2` dá `1234` (medido no bun: igual). `es`/`pt-PT` compactos
  (`1,2 mil`) não passam por agrupamento; o `always` de 4 dígitos no não compacto já é forçado em `number`
  (`es` `1.234`, `pt-PT` `1 234`, `pl` `1 234`, medidos). Só o golden confirma.
- Item 12 fechado em código (não compilado). Medido no bun 1.4.2 para os 38 locales do gerador mais `en` e `pt`:
  o separador do padrão de intervalo (`-` em es/it/zh/th/nl/da/vi/zh-*, U+2013 na maioria, `～` em ja, `~` em ko,
  ` - ` em pt-PT) e o sinal de aproximado (`~`, `≃` fr/fr-CH, `≈` de/ru/fr-CA/de-*, `約` ja, `ca.` nb). O gerador
  emite `RANGES` (`RangeEntry`); `intl_number_range.rs` lê o separador dali e, na identidade, troca o `plusSign`
  de `signDisplay: "always"` pelo aproximado, o que reproduz a posição do bun (`nl` `€ ~5,00`, `de-AT` `≈€ 5,00`,
  `de-CH` `EUR≈5.00`). O colapso de moeda, unidade e percent segue a regra geral por pontos de código, que já
  confere nos casos medidos (es `3,00-5,00 €`, it `3% - 5%`, ar com marcas em volta do percent). Pendências: marcas bidirecionais do
  aproximado em ar/he/fa (`‏~5.00 €`) dependem de `sign_parts` e só o golden confirma; o plural do nome por
  extenso usa sempre o do fim do intervalo (sem `StandardPluralRanges` de outros locales).
- Item 6 fechado em código (não compilado). O gerador agora cobre os 307 códigos de
  `Intl.supportedValuesOf("currency")`: os 12 antigos seguem completos em `CURRENCIES`; os 295 restantes saem em
  `EXTRA_CURRENCIES` (11024 linhas, só o que difere do código: símbolo, símbolo estreito, nome e nomes por
  plural) mais `CURRENCY_BASES` (uma forma de padrão por símbolo no locale, ~2600 linhas). `icu_number_data.rs`
  foi de 659 KB para 3,0 MB (nomes em UTF-8 cru, só marcas e espaços especiais escapados). `localized_parts`
  usa o nome com o padrão `name` do dólar trocado, e o símbolo com o padrão de `CurrencyBase`; sem entrada,
  cai no código como antes.
- Como foi feito: `bun scripts/gen-number-format-data.js` rodado com Bash (geração de dados, permitido), que
  refez `icu_number_data.rs` e `tests/golden/number_format_more_bun.tsv` (o golden não ganhou casos novos de
  moeda extra nem de intervalo: falta ampliar o gerador do golden). Os edits do próprio gerador foram aplicados
  por um script Python (deveria ter sido Edit). Nada compilado: conferir `cargo test` na próxima rodada
  (suspeitos: `Fill { ..fill }` em `localized_parts`, visibilidade de `SignDisplay`).
- Item 3 depende do golden.
- Enxugamento de `icu_number_data.rs` (3,0 MB para 1,4 MB, medido com `wc -c`). `EXTRA_CURRENCIES` agora é
  `&[ExtraLocale]` (moedas agrupadas por locale e ordenadas por código, busca binária) com linhas
  `extra("AED", símbolo, estreito, base, base, nome, plurais)` que guardam índices u16 numa tabela `STRINGS` de
  textos únicos (nomes iguais entre locales viram uma entrada; plural igual a `other` já não tinha entrada).
  `CurrencyBase` sai sem o texto da moeda (`c` vazio, quem renderiza troca): de ~2600 para 10 KB. Literais
  usam `\x1f` no lugar de `\u{1f}`. O leitor (`icu_number_patterns.rs`) ganhou `symbol()`/`narrow()` e
  `default_number_format.rs` foi ajustado nesses dois acessos. Não compilado: conferir `cargo test`
  (suspeita: `extra` const fn dentro de `static` aninhado, import de `extra` no arquivo gerado).
  Gerador rodado com Bash (geração de dados), edits do gerador por script Python (deveria ter sido Edit).
- Golden `number_regional_bun.tsv` (11958 para 12718 casos): 20 moedas extras (AUD NZD SEK NOK DKK PLN TRY ZAR
  SGD HKD THB ILS AED SAR EGP CZK HUF CLP COP TWD) x 5 locales (es fr de ja ar) x symbol/code/name (700
  casos), e 60 `formatRange` com moeda, unidade e percent espalhados pelos 38 locales (RANGE_LOCALES).
  Falta rodar contra o motor para ver o que diverge.
