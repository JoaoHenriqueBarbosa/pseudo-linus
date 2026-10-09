# Lacunas do Intl do porte frente ao bun 1.4.2 (ICU completo)

O porte não tem ICU. Os dados cobrem só `en`, `en-US`, `pt` e `pt-BR` (`src/runtime/intl_locale_data.rs`,
`AVAILABLE_LOCALES`). O que falta, em ordem do que o agente mais encontraria.

## Locales

- `Intl.*.supportedLocalesOf(["fr"])` devolve `[]`; no bun devolve `["fr"]` (e as centenas de locales do
  CLDR: es, fr, de, it, ja, ko, zh, ru, ar, hi, nl, pl, sv, tr, uk...). O locale não suportado cai em `en-US`.
- Regiões de `en` e `pt` (`en-GB`, `en-AU`, `en-CA`, `en-IN`, `pt-PT`, `pt-AO`) resolvem para o genérico
  (`en`, `pt`) com os dados de `en-US` e `pt-BR`. No bun: `en-GB` tem data dia/mês/ano e 24 horas, `en-IN`
  agrupa em lakh/crore, `pt-PT` usa espaço como separador de milhar e `EUR` por padrão de formatação.
- `resolvedOptions().locale` de `en-GB` sai `en` no porte e `en-GB` no bun.
- Dados de turco existem no bun (`"I".toLocaleLowerCase("tr") = "ı"`) e no porte só para o mapeamento de
  caixa (`az`, `el`, `tr`; o `lt` não tem as regras dos pontos acima do `i`). Turco no `NumberFormat`,
  `DateTimeFormat`, `Collator` (a ordem turca põe `ç`, `ğ`, `ı`, `ö`, `ş`, `ü` depois da letra-base) e
  `PluralRules` não existe.
- `Intl.Locale` `maximize`/`minimize` usam tabelas de 76 línguas (`LIKELY_SUBTAGS` e `MORE_LIKELY_SUBTAGS`), de
  `und-região`, `und-escrita` e `língua-escrita` (`zh-Hant` é `TW`, `sr-Latn` é `RS`...), não o `likelySubtags`
  inteiro (cerca de 1500 entradas): língua fora das tabelas volta sem escrita nem região.

## Collator

- Sem as tabelas do DUCET: letras fora do Latin-1 e do Latin Extended-A comuns ordenam por ponto de código,
  não pelo peso primário do CLDR. Colações por locale (`de-u-co-phonebk`, `sv`, `es-u-co-trad`, `zh` por pinyin
  ou traço) não mudam a ordem. `usage: "search"` não muda a ordem.
- `ResolveLocale` de `co`, `kf` e `kn` segue o C++ (a opção só desloca a extensão da tag quando difere dela:
  `en-u-kn` com `numeric: true` resolve `en-u-kn`). Para en e pt só `emoji` e `eor` são aceitos em `co`
  (`en-u-co-emoji` resolve `collation: "emoji"`, `phonebk` cai em `default`), mas eles também não mudam a ordem.
- `supportedValuesOf("collation")` devolve os 12 tipos medidos no bun (`compat`, `dict`, `emoji`, `eor`,
  `phonebk`, `phonetic`, `pinyin`, `searchjl`, `stroke`, `trad`, `unihan`, `zhuyin`); não conferido se o ICU do
  bun lista mais algum (`big5han`, `gb2312`).

## NumberFormat

- Moedas: símbolo e nome só para 20 moedas (`KNOWN_CURRENCIES`), as demais saem pelo código ISO; nome de
  moeda só em inglês e só para 11 delas. `supportedValuesOf("currency")` devolve 20 códigos; o bun devolve
  cerca de 300.
- Unidades: 31 simples no porte. O bun tem as 45 sanctioned (faltam `acre`, `fluid-ounce`, `gallon`,
  `gigabit`, `hectare`, `kilobit`, `megabit`, `microsecond`, `nanosecond`, `petabyte`, `stone`, `terabit`,
  `yard`, `mile-scandinavian`...); o porte tem `celsius`, `fahrenheit` e `degree` já como o bun. Em português a unidade sai com o texto do inglês (`3 days` em
  vez de `3 dias`, `kilometers` em vez de `quilômetros`).
- Unidades do inglês, atualização 2026-10-08 (sem cargo): medidas no bun 1.4.2 (1 e 2.5, short, long, narrow) e
  acrescentadas a `UNITS` de `default_number_format.rs` as 13 que faltavam (`acre`, `hectare`, `fluid-ounce`,
  `gallon`, `stone`, `yard`, `kilobit`, `megabit`, `gigabit`, `terabit`, `petabyte`, `microsecond` com U+03BC,
  `nanosecond`); as 31 antigas conferem com o bun nos três estilos. Lacunas: nas outras línguas
  (`icu_number_data::UNITS`, gerado por `scripts/gen-number-format-data.js`) essas 13 não têm linha, falta
  acrescentá-las ao gerador e regenerar; compostas em inglês (`mile-per-hour` dá `mph`, `meter-per-second` dá `m/s`,
  `acre-per-day` dá `ac/d`) não foram conferidas contra o código. Não compilado.
- `roundingIncrement` diferente de 1 é aceito e devolvido, mas o arredondamento o ignora.
- `formatRange` e `formatRangeToParts` existem (`intl_number_range.rs`) com as regras do `UNumberRangeFormatter`
  do ICU (colapso `AUTO`, identidade `~5` com `approximatelySign`, `source` por parte), só para `en` e `pt`.
  Não conferido contra o bun: o separador do português (U+2013 sem espaços, como o `en`), e o
  `StandardPluralRanges` (a forma plural do nome de unidade/moeda por extenso vem do fim do intervalo).
- Só o sistema de numeração `latn`. `-u-nu-arab`, `numberingSystem: "hanidec"` etc. caem em `latn`;
  `supportedValuesOf("numberingSystem")` devolve `["latn"]` (bun: cerca de 90).
- Notação compacta: os sufixos vêm do `icu_decimal` (`CompactDecimalFormatter`, `src/runtime/icu_number.rs`) para
  qualquer locale, então a nota antiga de "só en e pt" está obsoleta. Golden novo, medido no bun 1.4.2 e ainda NÃO
  executado no porte (sem cargo nesta rodada): `scripts/gen-number-compact-golden.js` gera
  `tests/golden/number_compact_bun.tsv` (3625 programas, 25 locales, curto e longo, valores de 0 a 1e21 e negativos,
  dígitos, `roundingMode`, `signDisplay`, moeda e unidade compactas, `useGrouping` min2/always/true/false,
  `formatToParts` e `formatRange`), rodado por `tests/number_compact_bun_golden.rs`. Próximo passo: rodar o teste e
  corrigir as divergências que ele listar (candidatas: `useGrouping: "always"` fora de en no compacto, que hoje age
  como `auto`; moeda e unidade compactas fora de en e pt; `formatRange` compacto).
  Simulação à mão de 45 programas do golden (15 por suspeito), sem cargo, em 2026-10-08:
  - `always` compacto: o significando de quatro dígitos (`1e15` em `es`, `pt-PT`, `pl`: `1.000 B`) saía sem grupo.
    Corrigido: `with_forced_group` em `icu_number.rs` serve ao `DecimalFormat` e ao `CompactFormat` (teste novo).
    O golden não tem caso `es`/`pl` com significando de quatro dígitos sob `always`; os 100 programas dele já
    batiam (en, fr, de, vi, hu, ru, nl, zh, ja tinham grupo pelo `Auto`).
  - Moeda e unidade compactas (`es`, `fr`, `de`, `ja`, `ko`, `ru`, `ar`, `hi`, `tr`, `sv`): os padrões de
    `icu_number_data` cobrem as 25 línguas e o número compacto entra no `Fill`; os 14 programas conferidos batem.
    Divergência achada e corrigida: o plural do nome por extenso e da unidade longa vinha do significando
    (`1 trillion euros`, `1 M euros`, `1,5 k kilogrammes` saíam no singular); o ICU usa o número na ordem
    original. `unscale_digits` em `default_number_format.rs` refaz os operandos. Medido nos casos `en` 1e12 e
    `es` 999999 do golden.
  - `formatRange` compacto: o porte nunca colapsava o sufixo compacto e punha espaços em volta do separador se
    qualquer lado tinha sufixo. O bun colapsa o sufixo igual de mais de um ponto de código (`1–2 mil`, `1–2 k`,
    `1–2 тыс.`) e só põe espaços se o lado inicial tem sufixo (`999–1B`, `1500–1 Mio.`, `1500～100万`).
    Corrigido em `intl_number_range.rs` (`inner_start`), teste ampliado. Lacuna: moeda ou unidade compactas
    em `formatRange` (o golden não cobre) seguem a mesma regra sem confirmação no bun.
- Percent e moeda em locales de outra ordem de afixos (`de`: `1.234,50 €`; `fr`: `1 234,50 €` com U+202F)
  não existem; só `en` e `pt-BR`.

## DateTimeFormat

- Só o calendário `gregory` (e `iso8601` tratado como gregoriano). `supportedValuesOf("calendar")` lista os
  16 do JSC (confere com o bun), mas `new Intl.DateTimeFormat("en-u-ca-japanese")` formata como gregoriano
  e o `resolvedOptions().calendar` diz `gregory`. No bun `japanese`, `buddhist`, `hebrew`, `islamic-*`,
  `persian`, `roc`, `chinese`, `coptic`, `ethiopic`, `indian` e `dangi` têm era, ano e meses próprios.
- Padrões de data só para `en` e `pt-BR`, e só para as combinações de campos de uso comum (o
  `DateTimePatternGenerator` do ICU escolhe para qualquer combinação, por exemplo `{month: "long", year:
  "numeric"}`, `{weekday: "short", hour: "numeric"}`, `{dayPeriod: "long"}`, `{era: "long"}` sozinhos).
- Nomes de fuso (`timeZoneName: "long"`) vêm de uma tabela de cerca de 30 fusos em inglês; os outros e todo
  o português saem pelo deslocamento (`GMT-03:00`). O CLDR tem nomes para centenas de fusos em cada locale.
- `formatRange` e `formatRangeToParts` existem (`intl_date_time_format/range.rs`) sem o `DateIntervalFormat`:
  o maior campo diferente escolhe o padrão de intervalo do CLDR só para data numérica, mês por extenso (com
  `weekday`), `hour` e `hour`+`minute`; o resto cai no `intervalFormatFallback` (`{0} - {1}` em português),
  inclusive era, segundos, `dayPeriod`, fuso e `weekday` sem dia, onde o ICU às vezes tem padrão próprio.
  Padrões e o separador do português não conferidos contra o bun. Atualização 2026-10-08 (sem cargo): fora de
  `en`/`pt`, `data_range` já usa os `range|cenário|sep` e `collapse` medidos por `scripts/gen-datetime-data.js` nas
  36 línguas/variantes de `LOCALES` (o relatório do golden dizia só en e pt: estava desatualizado). Faltavam as
  maiores diferenças mês (`same_year`) e hora/minuto com o mesmo AM/PM em 12 horas (`same_period`,
  `same_period_time`): o gerador agora as mede (tabela regenerada) e `range.rs` as escolhe
  (`assemble_shared_day_period` tira o AM/PM comum, no fim ou no começo). Não rodado: falta rodar
  `tests/datetime_range_bun_golden.rs`. Ainda sem tabela: `hu` e `fa` (estão no golden, fora de `LOCALES`), era,
  segundos, `dayPeriod` e fuso (caem no fallback), e o padrão completo `intervalFormats` por skeleton (o `icu_datetime`
  2.3.0 está no registro, mas não nas dependências do `Cargo.toml`; os padrões de intervalo não são API pública
  dele). `formatToParts` foi relido contra
  `buildFormattedDateTimeParts`/`partTypeString` (tipos e literais batem; sem calendário não-gregoriano não
  há `eraOverride`). `Temporal` e `Intl.DateTimeFormat` não conversam.
- `supportedValuesOf("timeZone")` sai da tzdata embutida (`jiff`); o bun sai do ICU e a lista pode divergir
  em nomes novos ou aposentados (`Europe/Kyiv`, `America/Ciudad_Juarez`).
- O espaço antes de AM/PM é U+0020 (medido no bun para `toLocaleString`); o `formatToParts` do bun não foi
  medido, conferir a parte `literal` antes de `dayPeriod`.

## RelativeTimeFormat, ListFormat, PluralRules

- Só `en` e `pt-BR`. `style: "narrow"` do inglês e do português usa abreviações aproximadas das tabelas do
  CLDR, não conferidas contra o bun para as oito unidades.
- `numeric: "auto"` do português cobre `-2..2` de `day` e `-1..1` das outras unidades; no CLDR o
  `anteontem`/`depois de amanhã` existem, mas as formas curtas (`style: "short"`) de `semana passada`
  (`sem. passada`) não estão na tabela.
- `RelativeTimeFormat` só honra `-u-nu-latn` (e a opção `numberingSystem: "latn"`), como o resto: `resolvedOptions().
  numberingSystem` é sempre `latn`.
- `ListFormat` `style: "short"` e `"narrow"` do português usam as mesmas junções do longo (não conferido
  contra o CLDR `pt`).
- `ListFormat` e `Segmenter` leem as opções com `intlGetOptionsObject` (primitivo lança `TypeError`);
  `Collator`, `PluralRules`, `RelativeTimeFormat`, `Locale`, `NumberFormat` e `DateTimeFormat` com
  `intlCoerceOptionsToObject`, como o C++.
- `PluralRules` usa o `icu_plurals` (todas as categorias do CLDR). `selectRange` usa as regras do `type` da
  instância (`ordinal` inclusive) e a tabela de intervalos do CLDR. O operando `c`/`e` da notação compacta sai
  da diferença de casas entre o valor e os dígitos escalados; arredondamento que sobe de grandeza
  (`999999` para `1M`) erra uma casa. O locale resolvido ainda vem de `intl_locale_data` (só en e pt), então
  o golden `tests/plural_bun_golden.rs` (2812 linhas, 19 locales) deve acusar os demais até a lista de
  locales do icu4x entrar. Escrito sem rodar cargo.

## Cobertura de locales: golden `intl_more_bun.tsv` (2026-10-08)

`scripts/gen-intl-more-golden.js` (bun 1.4.2) gera `tests/golden/intl_more_bun.tsv` (23498 linhas) e
`tests/intl_more_bun_golden.rs` o confere (quatro testes, um por classe). 38 locales: os 37 do escopo (en en-GB
pt pt-PT es es-MX fr fr-CA de de-AT it ja ko zh zh-TW ar fa he hi th tr pl nl sv da nb fi cs el id vi uk ru ro hu
bg hr) mais `sr`. Cobre ListFormat (type x style x 0 a 4 itens, `format` e `formatToParts`), PluralRules
(cardinal e ordinal: 40 `select`, 40 `selectRange`, `pluralCategories`, `locale`), RelativeTimeFormat (always e
auto x 12 unidades x 8 valores, estilo long) e Collator (60 pares por locale; sv, de, es, tr, ja,
`zh-u-co-pinyin` e `de-u-co-phonebk` ainda com sensitivity, numeric, caseFirst, ignorePunctuation e
`usage: "search"`, mais `resolvedOptions`). Não rodei cargo: o resultado do teste é desconhecido.

Status por classe frente a esses locales (por leitura do código, sem rodar):

- ListFormat: `list_parts` usa a tag pedida direto no `icu_list` (dados compilados), então deve cobrir os 38.
  `resolvedOptions().locale` e `supportedLocalesOf` seguem pela resolução en/pt (não estão no golden).
- PluralRules: o `icu_plural` serve a qualquer locale, mas o locale resolvido vinha só de `intl_locale_data`
  (en-US ou pt-BR): todo locale fora dos dois caía em `en-US`. Corrigido em `intl_plural_rules.rs`: a primeira
  tag pedida (sem `-u-`) vale quando a língua está em `PLURAL_LANGUAGES`. Edição escrita sem compilar. As
  operações de dígitos (`NumberSettings::defaults`) seguem pela língua resolvida (en ou pt), o que só importa
  para opções que o golden não varia.
- RelativeTimeFormat: tabelas só para en, pt, es, fr, de, it, ja, ru, ar, hi (`intl_relative_time_data.rs`).
  Faltam ko, zh, zh-TW, fa, he, th, tr, pl, nl, sv, da, nb, fi, cs, el, id, vi, uk, ro, hu, bg, hr, sr, e as
  regiões (en-GB, pt-PT, es-MX, fr-CA, de-AT) que no CLDR têm textos próprios. O gerador `gen-reltime-golden.js`
  aceita acrescentar línguas em `LANGS`; falta rodar com elas e ligar a resolução de locale.
- Collator: sem DUCET nem tailoring. Esperam-se falhas em todo par que dependa de colação por locale (sv å ä ö
  no fim, da/nb æ ø å, es ñ, tr ç ğ ı ö ş ü, de-u-co-phonebk, zh pinyin, ja kana vs katakana e kanji, cs `ch`,
  hu, hr `lj nj dž`, el, ru, uk, bg, ar, he, th, ko), de `usage: "search"` e de `ignorePunctuation`.
  `resolvedOptions().locale` dos sete locales das opções resolve para en-US.
- Resolução (`intl_locale_data::AVAILABLE_LOCALES`): só en, en-US, pt, pt-BR. Enquanto isso, NumberFormat,
  DateTimeFormat, RelativeTimeFormat e Collator tratam os outros 35 locales como `en-US`; é a lacuna que
  concentra quase todas as falhas esperadas do golden novo.

## Intl que não existe

- `Intl.DurationFormat` existe (`intl_duration_format.rs`, e `Temporal.Duration.prototype.toLocaleString`
  por cima dele) para `en` e `pt-BR`. Os nomes de unidade de `pt` (`short` e `narrow` sobretudo) foram escritos
  de memória do CLDR, sem golden; o separador de horas é `:`; replica o C++ que perde o sinal de fração
  negativa na parte inteira zero (`{milliseconds: -500}` em `digital`). `Duration::total_nanoseconds` do
  `iso8601.rs` soma de `Day` até a unidade (o C++ soma da unidade para baixo): o novo `total_nanoseconds_from`
  é o do C++. Falta golden no bun e conferir se o bun 1.4.2 expõe `Intl.DurationFormat` (não está em
  `bun-builtin-props.json` nem em `gen-intl-golden.js`; foi portado porque o JSC o tem).
- `Intl.DisplayNames` existe (`intl_display_names.rs`, `type` `language`, `region`, `script`, `currency`,
  `calendar` e `dateTimeField`) com as tabelas de `en` e `pt-BR` apenas (`intl_display_names_data.rs`); nome
  fora delas devolve o código ou `undefined` conforme `fallback`. es, fr, de, it, ja e ru têm tabelas
  geradas do bun (`intl_display_names_data_more.rs`, `scripts/gen-display-names-data.js`), agora com
  `dateTimeField` (12 campos em long, short e narrow, 216 linhas no golden `display_names_bun.tsv`). Não
  rodei cargo: o ligamento em `intl_display_names.rs` (campo `fields` de `MoreTable`, `date_field`) e o
  golden ainda precisam de compilação e teste.
- `Intl.Segmenter` existe (`intl_segmenter.rs`) sobre a crate `icu_segmenter` 2.3 (dados compilados, dicionário
  CJK e LSTM tailandês pelo script): grafemas, palavras e sentenças com `SpacingMark`, `Prepend`,
  `Sentence_Break` completo e `isWordLike` por `WordType`. `tests/segmenter_bun_golden.rs` mede 150 textos
  x 3 granularidades x 4 locales contra o bun (`scripts/gen-segmenter-golden.js`); ainda não foi rodado
  (sem build nesta fatia), divergências de dicionário entre ICU4X e ICU4C são esperadas em CJK e tailandês raros.
- `Intl.Locale.prototype`: todos os métodos e acessores do `IntlLocalePrototype` do JSC existem
  (`getCalendars`, `getCollations`, `getHourCycles`, `getNumberingSystems`, `getTimeZones`, `getTextInfo`,
  `getWeekInfo`, `firstDayOfWeek`, `variants`), mas sem o CLDR por baixo: `getCalendars` e `getWeekInfo` só
  têm as regiões das tabelas (as outras caem em `gregory` e em segunda-feira/sábado-domingo), `getCollations`
  só sabe `de` e `es`, `getTimeZones` só tem 43 regiões (o resto devolve `[]`; o ICU devolve o nome canônico
  dele, por exemplo `Asia/Calcutta`), `getHourCycles` decide só pela língua (`pt` é `h23`, o resto `h12`; o ICU
  usa a região: `en-GB` é `h23`).
- Os dados de `calendarPreferenceData`, `weekData` e `REGION_TIME_ZONES` foram escritos de memória do CLDR e
  não têm golden.

## O que não foi conferido contra o bun

Os casos de `tests/intl_bun.rs` são os medidos pelo usuário. Fora deles, as tabelas de
`default_number_format.rs`, `intl_date_time_format.rs`, `intl_relative_time_format.rs` e
`intl_list_format.rs` foram escritas de memória do CLDR e não têm golden. O próximo passo é um
`scripts/gen-intl-golden.js` no bun gerando um tsv por classe (como `e2e_values.tsv`) e um teste que o
compare.

## Auditoria do Intl.RelativeTimeFormat (2026-10-08)

Comparado `IntlRelativeTimeFormat*.cpp` com `src/runtime/intl_relative_time_format.rs`. Nenhuma divergência de comportamento encontrada no código, então nada foi alterado no Rust.

Conferido e igual ao upstream:

- Ordem de leitura das opções: locales, options coagidas, `localeMatcher`, `numberingSystem` (RangeError se mal formado), `style`, `numeric`.
- Mensagens de erro: `numberingSystem`, `style`, `numeric`, `number argument must be finite`, `unit argument is not a recognized unit type`, `called on value that's not a RelativeTimeFormat`, construtor sem `new`.
- `format`/`formatToParts`: `toNumber` do valor, depois `toString` da unidade, depois checagem de finito, depois de unidade (mesma ordem do C++). Unidade aceita singular e plural, sensível a caixa.
- `resolvedOptions`: ordem `locale`, `style`, `numeric`, `numberingSystem`; `numeric` sai como string `always`/`auto`.
- `formatToParts`: número formatado sobre o valor absoluto, campo `unit` singular nas partes do número, literais antes e depois; sem partes de número em `numeric: "auto"` textual.

Atualização (2026-10-08): es, fr, de, it, ja, ru e ar agora têm tabela gerada do bun
(`src/runtime/intl_relative_time_data.rs`, regenerada por `bun scripts/gen-reltime-golden.js`, que também
escreve `tests/golden/reltime_bun.tsv`, conferido por `tests/reltime_bun_golden.rs`). O padrão é escolhido pela
categoria do `icu_plural` (zero a other) sobre a tag resolvida, e o `numeric: "auto"` usa os textos medidos
(-2 a 2). Lacunas dessa fatia, não compiladas nem rodadas ainda:

- ATUALIZAÇÃO (passo 4 do icu4x): o número agora sai de `format_parts` (`icu_number`) no locale resolvido
  (`state.base_locale`) e no `numberingSystem` honrado (mesma resolução do `NumberFormat`; `resolvedOptions`
  devolve o sistema real), com o padrão do `unum_open` do C++ (mín. 1 inteiro, 0 a 3 frações, agrupamento do
  locale, sem `always`). O gerador ganhou `hi`, en, pt e tags `-u-nu-` (`ar-u-nu-arab`, `hi-u-nu-deva`,
  `en-u-nu-arab`, `es-u-nu-deva`) e os valores 1234, 1000000, 1234567.891, 0.5, 1.5 e -1234; golden regenerado
  (15456 linhas) e `intl_relative_time_data.rs` agora inclui `hi`. NÃO compilado nem rodado: pode revelar
  divergências de `narrow` e de `auto` de en e pt (listadas abaixo) e a opção `numberingSystem` (só a forma
  `-u-nu-` está no golden).
- (resolvido, ver acima) O número saía no formato inglês: o golden só tinha inteiros abaixo de 1000, então fração, milhar e `many` de
  grandes inteiros (`1 000 000 de jours`, `dans 1,5 jour`) divergiam.
- `en` e `pt` seguem nas tabelas escritas à mão, agora com golden; as divergências de `narrow` e de `auto`
  listadas abaixo aparecem no teste se existirem.
- Outras línguas além das sete resolvem a tag, mas caem nas tabelas do inglês.

Lacunas que seguem sem golden (dados escritos de memória do CLDR):

- Estilo `narrow` do inglês e do português (ICU/CLDR recente usa `in 3d`, `3d ago`; o porte trata `narrow` do português como `short`). Verificar contra o bun.
- `numeric: "auto"` só cobre -1, 0, 1 (e -2, 2 de `day` em português); o ICU tem poucas entradas extras por locale que não foram checadas.
- Só `latn` como sistema numérico e só `en` e `pt` como locales de dados; outros locales caem em `en-US`.

## Intl.Segmenter

Comparado `IntlSegmenter*.cpp`, `IntlSegments*.cpp`, `IntlSegmentIterator*.cpp` e `IntlSegmentDataObject.cpp` com `src/runtime/intl_segmenter.rs`. Nenhuma divergência de comportamento encontrada no código, então nada foi alterado no Rust.

Conferido e igual ao upstream:

- Ordem de leitura: `canonicalizeLocaleList`, objeto de opções (TypeError para primitivo), `localeMatcher`, resolução do locale, `granularity`. A resolução do porte não lança, então a ordem observável dos erros é a mesma.
- Mensagens: `granularity must be either "grapheme", "word", or "sentence"`, construtor sem `new`, e os três `called on value that's not a ...` de `segment`, `resolvedOptions`, `containing`, `[@@iterator]` e `next`.
- `resolvedOptions`: ordem `locale`, `granularity`.
- `containing`: `toIntegerOrInfinity` do argumento, `undefined` fora de `[0, length)`, senão o segmento que contém o índice.
- Iterador: começa no primeiro limite, devolve `{ value: undefined, done: true }` repetidamente no fim; string vazia termina de imediato.
- Objeto de segmento: ordem `segment`, `index`, `input`, e `isWordLike` só em `word`. `toStringTag` do iterador é `Segment String Iterator`.

Lacunas que seguem:

- Resolvidas pela troca para `icu_segmenter` (a confirmar no golden): `SpacingMark` e `Prepend` nos grafemas, dicionário em `word` para ideogramas e hiragana, e `Close`, `Sp`, `Upper` e `Lower` das sentenças.
- Falta a checagem de locale resolvido vazio (`failed to initialize Segmenter due to invalid locale`); inalcançável enquanto a resolução sempre devolve `en-US` ou `pt-BR`.
- Os protótipos de `Segments` e do iterador não são por realm (`LazyClassStructure`), viajam como campo das funções `segment` e `[Symbol.iterator]`.

## Intl.Locale

Comparado `IntlLocale.cpp`, `IntlLocalePrototype.cpp` e `IntlLocaleConstructor.cpp` com `src/runtime/intl_locale.rs`.

Conferido e igual ao upstream:

- Ordem de leitura: `toString` da tag, `coerceOptionsToObject`, validação da tag (`invalid language tag`), depois `language`, `script`, `region`, `variants`, `calendar`, `collation`, `firstDayOfWeek`, `hourCycle`, `caseFirst`, `numeric`, `numberingSystem`.
- Mensagens: as de boa formação de cada opção, `hourCycle must be ...`, `caseFirst must be either ...`, `First argument to Intl.Locale must be a string or an object`, construtor sem `new`, e os `called on value that's not a Locale` de todos os métodos e acessores.
- Validação de `variants` (vazia, hífen sobrando, subtag inválida, repetida), `weekdayToString`, `getWeekInfo` sem `minimalDays`, `getTimeZones` pela região da própria tag (`undefined` sem região), `numeric` como `kn` vazio ou `true`.
- Tabela de instalação (ordem dos métodos e acessores do `.lut`).

Corrigido neste passe:

- `getHourCycles` decidia só por `pt` (`h23`) contra `h12` para o resto; agora segue a região (`h12` em US, AU, NZ, IN, PH, KR etc.) e as línguas de 12 horas, `h23` no restante.
- `getNumberingSystems` devolvia sempre `latn`; agora `arabext` (fa, ps), `beng`, `deva`, `mymr`, `tibt` pelas línguas cujo padrão do CLDR não é `latn`.

Lacunas que seguem (sem ICU/CLDR):

- `getHourCycles`, `getNumberingSystems`, `getCalendars`, `getCollations`, `getTimeZones` e `getWeekInfo` são tabelas parciais; regiões e línguas fora delas caem no padrão. `ar` fica em `latn` por incerteza do padrão por região.
- `maximize` e `minimize` dependem das tabelas de `intl_locale_data.rs` (não editado); extensões `-t-` e `-x-` não têm o tratamento especial de `m_nonUnicodeExtensions`.
- Falha de canonicalização do ICU (`failed to initialize Locale`) é inalcançável aqui.

## Intl.ListFormat e Intl.PluralRules

Comparado `IntlListFormat*.cpp` e `IntlPluralRules*.cpp` com `intl_list_format.rs`, `intl_plural_rules.rs` e `icu_plural.rs`, medindo o bun em `en`, `pt`, `ar`, `ru`, `pl` e `fr`.

Conferido e igual ao upstream:

- Ordem de leitura: `localeMatcher`, `type`, `style` no `ListFormat`; `localeMatcher`, `type`, `notation`, `compactDisplay`, dígitos no `PluralRules`.
- Mensagens: `type must be ...`, `style must be ...`, `notation must be ...`, `compactDisplay must be ...`, `options argument is not an object or undefined` (`ListFormat` com string ou `null`; `PluralRules` coage e aceita), `Iterable passed to ListFormat includes non String`, `start or end is undefined`, `Passed numbers are out of range` (RangeError), os `called on value that's not a ...`.
- `resolvedOptions` do `PluralRules`: ordem `locale`, `type`, `notation`, `compactDisplay` (só no compact), `minimumIntegerDigits`, campos de fração ou de significativos (os quatro em `morePrecision`/`lessPrecision`), `pluralCategories`, `roundingIncrement`, `roundingMode`, `roundingPriority`, `trailingZeroDisplay`. Categorias ordenadas `zero, one, two, few, many, other` e iguais às do bun para en, pt, ar, ru, pl, fr em cardinal e ordinal.
- `select` e `selectRange` (en, pt, ar, ru, pl, fr) batem com o bun nos casos medidos, inclusive `1.0` em inglês e `selectRange(0, 1)` em pt, fr e ar.
- `formatToParts` alterna `element` e `literal` como o upstream.

Corrigido neste passe:

- `ListFormat` em `pt`, tipo `conjunction`, estilo `narrow`: saía `a e b` e `a, b e c`; o bun dá `a, b` e `a, b, c`. Teste acrescentado.

Lacunas que seguem:

- `ListFormat` agora usa `icu_list` (padrões do CLDR de qualquer locale, formatando pela primeira tag pedida), com 30 casos de es, fr, de, ar, ru, pl fixados do bun. Não compilado nem testado ainda (sem cargo neste passe; `icu_list` não está no registry local, `Cargo.lock` precisa de rede). Casos com elemento começando em `i` (`es` `y`/`e`) divergem entre ICU do bun e icu4x possivelmente: não fixados. A lacuna do `locale` resolvido (`resolvedOptions` sai `en-US`/`pt-BR` para es, fr, de...) segue, por depender do `intl_locale_data.rs` (`Language`).
- RESOLVIDO: a resolução de `intl_locale_data.rs` já mantém a tag pedida para toda língua do CLDR (via `LocaleExpander` do icu4x) e o `PluralRules` usa `icu_plural` sobre essa tag; 30 tags medidas no bun (sv, ar, ru, pl, es, fr, de, it, ko, nl, tr, uk, hi, en-001, en-AU, zh-HK, sr-Cyrl, fil, cs, cy...) estão fixadas em `resolves_the_requested_tag_for_cldr_languages_as_bun`. `Language` (en/pt) só guia os textos ainda sem icu4x (ListFormat, RelativeTimeFormat, NumberFormat).
- `PluralRules` com `notation: "compact"` não leva o expoente compacto ao operando `c`/`e`; `selectRange` em `ordinal` usa a tabela cardinal de intervalos.
- A mensagem de `format(5)` no `ListFormat` (bun: `Type error`) depende do erro de `iterator_for_iterable`, não conferido.

## Intl.Collator

Comparado `IntlCollator.cpp`, `IntlCollatorPrototype.cpp` e `IntlCollatorConstructor.cpp` com `src/runtime/intl_collator.rs`, e a comparação contra o bun (`bun -e`).

Conferido e igual ao upstream:

- Ordem de leitura: lista de locales, `coerceOptionsToObject`, `usage`, `localeMatcher`, `collation`, `numeric`, `caseFirst`, `ResolveLocale`, depois `sensitivity` e `ignorePunctuation`.
- Mensagens (`usage must be either ...`, `localeMatcher ...`, `collation is not a well-formed collation value`, `caseFirst ...`, `sensitivity ...`, `called on value that's not a Collator`).
- `resolvedOptions` (ordem das sete chaves), `collation` `default`/`emoji`/`eor`, `usage: "search"` ignorando `collation`, extensões `-u-kn`/`-u-kf` na tag resolvida.
- Getter `compare`: função ligada em cache, `length` 2, `name` vazio; `sensitivity` base/accent/case/variant, `numeric` (zeros à esquerda ignorados), `caseFirst` upper/lower e `ignorePunctuation` batem com o bun nos casos medidos (pt, en).

Corrigido neste passe (a ordem medida no bun):

- Tailoring por locale (`intl_collator_tailoring.rs`, golden `tests/golden/collator_bun.tsv`) e resolução de locale do Collator pelo conjunto do colador (`en-GB` resolve `en`, `fr-CA` e `de-AT` ficam, `xx` resolve `en-US`). Escrito sem compilar. Faltam `et`, `lv`, `is`, `vi`, `az`, `mt`, o acento invertido de `fr-CA` e os tonos do grego.
- Ordem secundária das marcas: agudo, grave, breve, circunflexo, caron, anel, trema, duplo agudo, til, ponto, cedilha, ogonek, mácron (antes seguia o ponto de código, grave antes de agudo).
- `đ`, `ð`, `ł`, `ø` agora ordenam junto de `d`, `l`, `o` (`d đ ð e`, `l ł m`, `o ø p`); antes iam depois de `z`.
- Ligaduras: `ae < æ < af` e `ss < ß < st` (antes `æ` era igual a `ae`).

Lacunas que seguem:

- Sem tailoring por locale: `sv` (`z < å < ä < ö`), `da`, `es-u-co-trad`, `de-u-co-phonebk` etc. O locale já resolve (`sv` sai `sv`), mas o `Collator` do bun usa um conjunto de locales próprio (colação) e difere da resolução genérica: `Collator("en-GB")` resolve `en`, `pt-PT` e `pt-AO` resolvem `pt`, `en-ZZ` resolve `en`, enquanto `fr-CA` e `de-AT` ficam como pedidos. Falta um `available_locales` por classe para o `Collator`.
- Região inexistente em língua válida (`en-ZZ`): o bun resolve para `en` em todas as classes (`PluralRules`, `DateTimeFormat`...); o porte devolve `en-ZZ` por não ter lista de regiões por língua.
- Só a última marca combinante de cada letra é mantida (`a` + agudo + circunflexo compara igual a `a` + circunflexo + agudo; no bun difere).
- `©` e símbolos fora do ASCII ordenam por ponto de código, não pelo DUCET; dígitos não ASCII não entram em `numeric`.
- Ordem do reverso de `caseFirst: "upper"` também inverte o peso das ligaduras (cosmético).

## Intl.DisplayNames

Comparado `IntlDisplayNames.cpp` com `src/runtime/intl_display_names.rs` e medido no bun (en e pt-BR).

Conferido e igual: ordem de leitura (`localeMatcher`, `style`, `type`, `fallback`, `languageDisplay`), todas as mensagens (`type must not be undefined` como `TypeError`, as quatro de enum como `RangeError`, `options argument is not an object or undefined`), `resolvedOptions` (`languageDisplay` só em `language`), validação por tipo (`argument is not a language id`, `region subtag`, `script subtag`, `well-formed currency code`, `calendar code`, `dateTimeField code`), `fallback: "none"`, construtor sem `new`, `length` 2.

Corrigido neste passe (dados e regra, contra o bun):

- Dialeto de língua: com escrita e região só vale a chave `língua-escrita-região`; `língua-região` só sem escrita e `língua-escrita` só sem região (`zh-Hans-CN` sai `Chinese (Simplified, China)`, `en-Latn-US` sai `English (Latin, United States)`).
- `pt` não tem dialeto para `en-US`, `pt-PT`, `fr-CA`, `es-419` etc. (saem `Inglês (Estados Unidos)`...); só `zh-Hans`, `zh-Hant`, `nl-BE`, `de-CH`, `ro-MD`, `sw-CD`. A primeira letra do nome de língua em `pt` é maiúscula (`Inglês`, `Português (Brasil)`), a de escrita não (`latim`).
- Escrita `Hans`/`Hant` sozinha é `Simplified`/`Traditional` (`simplificado`/`tradicional`); `und` não tem nome (volta o código); região `UK` existe; `001` em `pt` é `Mundo`.
- Moedas `XXX` (`Unknown Currency`, `¤` em short e narrow) e `XTS`; `TWD` narrow em `pt` é `NT$`.
- Calendários: `islamic*` é `Hijri Calendar` em `en`; `iso8601` é `Gregorian Calendar (ISO 8601 Weeks)`; em `pt` os nomes levam maiúscula (`Calendário Gregoriano`), `roc` é `Calendário da República da China`, `islamic-umalqura` usa hífen não separável, e `islamic-tbla`/`islamic-rgsa` não têm dado (voltam o código).
- `dateTimeField`: `weekday` abreviado e estreito (`day of wk.`, `dia da sem.`), `minute` curto `min.` em `pt`, `second` `seg.`, `timeZoneName` `fuso` em short e narrow.

Lacunas que seguem:

- Língua com dados: en e pt-BR (tabelas à mão) e, desde o gerador `scripts/gen-display-names-data.js`, es, fr, de, it, ja e ru (`intl_display_names_data_more.rs`, uma tabela por língua, medida no bun com os mesmos códigos das tabelas de en: língua, dialeto, escrita, região, moeda e calendário, long e short onde existe; `tests/golden/display_names_bun.tsv` e `tests/display_names_bun_golden.rs` conferem). O gerador reproduz a composição dos nomes de língua (parênteses, separador `、` do ja, escrita em minúscula dentro dos parênteses do ru) e falha se as tabelas não regerarem o que o bun devolve para ~600 códigos compostos por língua. Lacunas dessas seis: `dateTimeField` segue em inglês (não medido); só entram os códigos que a tabela de en tem; nome de região ou língua ausente no en não existe nas outras; `narrow` de língua e região é tratado como `short`; ainda não rodado no cargo (escrito sem compilar). Demais línguas seguem em en. Variantes (`en-1996`, `en-US-posix`) têm nome no ICU e aqui voltam o código; `HK` e `MO` como detalhe de língua usam `Hong Kong SAR China` e `Macao SAR China` no ICU; `EZ`, `QO` e outras regiões fora da tabela caem em `fallback`; moedas e línguas fora das tabelas idem.
- `islamicc` não é mapeado (o bun também devolve o código).
- Tabelas escritas de memória do CLDR; só o que foi medido acima tem golden.

## Auditoria de `toLocale*` (Number, BigInt, Date, String), medida no bun

Conferido contra o bun 1.4.2 e lendo o porte (o `upstream/` desta árvore não tem os `.cpp` de runtime, então a
leitura foi só do Rust):

- Locale inválido (`"xx-invalid-"`, `""`, `"a"`) é `RangeError: invalid language tag: <tag>` em todas as sete
  funções; `["tr","x_"]` idem; elemento não string nem objeto é `TypeError: locale value must be a string or object`.
  O porte tem as duas mensagens em `canonicalize_locale_list`. Mensagens de `this` inválido do Number
  (`thisNumberValue called on incompatible string`) e do BigInt (`'this' value must be a BigInt or BigIntObject`) batem.
- `Date.prototype.toLocale*String` com data inválida devolve `Invalid Date` antes de olhar `locales` e `options`
  (também no bun, `new Date(NaN).toLocaleString("zz-")` não lança). O porte faz o mesmo.
- Mensagens de `options` do DateTimeFormat (`timeStyle is specified while formatting date is requested`,
  `dateStyle is specified while formatting time is requested`, `dateStyle and timeStyle may not be used with other
  DateTimeFormat options`) saem do `intl_date_time_format.rs`; conferir só pelo teste de bun (não rodado aqui).
- `toLocaleUpperCase`/`toLocaleLowerCase` com `lt`: corrigido neste passe. No bun `"i̇".toLocaleUpperCase("lt")`
  perde o ponto acima (`"I"`), `"Í".toLocaleLowerCase("lt")` ganha ponto (`"i̇́"`), e `Ì`, `Í`, `Ĩ`
  viram `i` + ponto + acento sempre. O porte agora tem `lithuanian_pre_lower`/`lithuanian_pre_upper` em
  `intl_case_mapping.rs`, com a tabela das classes de combinação (230 e as demais) só do bloco U+0300..U+036F.
  `tr` e `az` já batiam (`"iIİı"` em maiúsculas é `"İIİI"`; `I` em minúsculas é `ı`; `I` + ponto vira `i`).
- Não coberto (precisa de dados que o porte não tem): `tr`, `de`, `lt` em `Number.toLocaleString` e `BigInt.toLocaleString`
  caem em `en-US` (bun: `tr` e `de` dão `1.234.567,891`; `lt` agrupa com espaço fino e vírgula decimal, `1 234 567,891`).
  `localeCompare` com `tr` não aplica a ordem turca. Idem `Date.toLocaleDateString("tr")` (bun: `01.01.1970`) e `de`
  (hora `00:00:00` em 24 horas). Reaparece como lacuna de dados de locale, ver a seção Locales.
- `toLocaleUpperCase`/`LowerCase` do grego (`el`): os ditongos com `ι` e `υ` seguem sem a regra do ICU
  (dívida já registrada no cabeçalho de `intl_case_mapping.rs`).
- Nada rodado: sem cargo nem teste, o teste novo `lithuanian_dot_above_rules` está escrito mas não executado.

## Intl.DateTimeFormat

Auditado `initialize` em `src/runtime/intl_date_time_format.rs` contra `IntlDateTimeFormat.cpp` (`initializeDateTimeFormat`), mais medições no bun. Não compilado nem testado neste passe (sem cargo).

Conferido e igual ao upstream:

- Ordem de leitura: `localeMatcher`, `calendar`, `numberingSystem`, `hour12`, `hourCycle`, `timeZone`, `weekday`, `era`, `year`, `month`, `day`, `dayPeriod`, `hour`, `minute`, `second`, `fractionalSecondDigits`, `timeZoneName`, `formatMatcher`, `dateStyle`, `timeStyle`.
- Todas as mensagens de RangeError/TypeError de opção (`weekday must be ...`, `calendar is not a well-formed calendar value`, `invalid time zone: X`, `dateStyle and timeStyle may not be used with other DateTimeFormat options`, `timeStyle is specified while formatting date is requested`, `date value is not finite in DateTimeFormat format()`).
- Validação de offset (`+03`, `+0300`, `+03:00` viram `+03:00`; `-00:00` vira `+00:00`; `+24:00`, `+03:00:00`, `Z`, `""` e espaços são RangeError), como o bun.

Corrigido neste passe:

- `resolvedOptions().dayPeriod`: o `a` do padrão de 12 horas volta como `dayPeriod: "short"` (o `parse` do padrão do upstream trata `a`/`b`/`B` como `dayPeriod`), logo `{hour: "numeric"}` em 12h traz `"dayPeriod":"short"` antes de `hour`, como no golden (linha 3601). Antes só saía se o usuário pedisse.
- `timeZone` no `resolvedOptions`: só `utc` (qualquer caixa) vira `UTC`. `GMT`, `Etc/UTC`, `Etc/GMT`, `Zulu`, `UCT`, `Asia/Calcutta`, `Europe/Kiev`, `EST` voltam como o nome IANA pedido, na caixa canônica (`etc/gmt-3` vira `Etc/GMT-3`), sem canonicalizar. Antes todos os aliases de UTC viravam `UTC`. Se a tzdata embutida do `jiff` não tiver algum alias de UTC, o fallback antigo (`UTC`) continua. Conferir com cargo que `jiff` devolve `iana_name()` com a caixa canônica para `GMT`, `Zulu` e afins.

Lacunas que seguem (medidas no bun, não corrigidas):

- Só `en` e `pt` têm padrão. es, de, ja caem no inglês ou no português: `es` `5/1/2024`, `de` `5.1.2024`, `ja` `2024/1/5`, juntas `um` (de) e `, ` (es), `2024年1月5日金曜日` (ja). `resolvedOptions().hourCycle` com `hour12: true`: `ja` dá `h11` (padrão `K`), `en`, `pt`, `es`, `de` dão `h12`; o porte dá `h12` sempre. Sem hora pedida, em `pt`/`es`/`de`/`ja` o bun devolve `hour: "2-digit"`/`"numeric"` conforme o locale (`es` e `ja` `numeric`, `pt` e `de` `2-digit`); o porte só distingue `en` e `pt`.
- `dayPeriod: "long"` com `hour`+`minute` em `pt`, `es`, `de`, `ja` (24h): o bun formata sem período (`hour`, `minute` apenas); em `en` sai `3:04 in the afternoon`.
- Calendários e numerais: `de-u-ca-buddhist-nu-thai` resolve `buddhist` e `thai` no bun, com `era: "narrow"`, `year`, `month`, `day` numéricos; o porte cai em `gregory`/`latn`. Idem `{calendar: "buddhist"}` (golden 3604, 3647) que traz `era` (`short` em en, `narrow` em pt).
- `timeZoneName` só tem tabela em inglês e pt-BR para os fusos comuns; `pt` curto sai `BRT` no bun para `America/Sao_Paulo` (o porte sai `GMT-3`), e es, de, ja usam `GMT-3` no curto.
- `formatRange`, `formatRangeToParts` e `formatToParts`: não comparados com o bun neste passe além do que `range.rs` já documenta no cabeçalho.

## RelativeTimeFormat: en e pt pela tabela gerada (2026-10-08)

- O gerador `scripts/gen-reltime-golden.js` agora emite `en` e `pt` em `LANGS` (padrões por categoria de plural,
  passado e futuro, long/short/narrow, e os textos de `auto` de -2 a 2). Regenerado com o bun: golden com 15456
  linhas (igual a antes, en e pt já estavam nele), 240 padrões e 594 textos de auto em
  `src/runtime/intl_relative_time_data.rs`.
- `intl_relative_time_format.rs` não tem mais tabela escrita à mão (`unit_suffix` e `auto_text` removidas, junto
  do uso de `cardinal_category`): toda língua passa por `data_parts` e `data_auto_text`. `data_language` cai em
  `en` quando a língua resolvida não está na tabela. Isso fecha as lacunas de `narrow` do en (`in 3d`) e do pt
  (`há 3 d`), do `auto` fora de -1/0/1 e do plural do pt (`one` para 0 e 1 vem do `icu_plural`).
- NÃO compilado nem rodado (regra da tarefa). Risco a conferir no primeiro `cargo test`: o `icu_plural` do icu4x
  e o `Intl.PluralRules` do bun precisam concordar na categoria do pt para 0, 1 e 1,5 (CLDR do bun vs. do icu4x).

## Intl.getCanonicalLocales, supportedValuesOf e supportedLocalesOf (2026-10-08)

- Auditado contra `IntlObject.cpp` e o bun 1.4.2. Golden novo: `tests/golden/intl_object_bun.tsv` (280 programas),
  gerador `scripts/gen-intl-object-golden.js`, teste `tests/intl_object_bun_golden.rs`. NÃO compilado nem rodado.
- CORRIGIDO: `supportedValuesOf` devolvia listas das tabelas das classes (currency 20 em vez de 307,
  numberingSystem 1 em vez de 78, unit 31 em vez de 45, timeZone da tzdata do jiff em vez dos 445 do ICU).
  Agora as quatro vêm de `src/runtime/intl_supported_values_data.rs`, gerado por
  `bun scripts/gen-intl-object-golden.js --rust-data` (codegen, por isso via Bash). `calendar` (16) e `collation`
  (12) já batiam. Chave desconhecida: `RangeError: Unknown key for Intl.supportedValuesOf` (igual ao bun).
- LACUNA (fora do meu alcance, `intl_locale_data.rs` é proibido): a canonicalização de tags e
  `supportedLocalesOf` não foram medidas na engine. Pontos a conferir no primeiro `cargo test`, com os valores do bun:
  `iw` dá `he`, `in` dá `id`, `mo` dá `ro`, `ji` dá `yi`, `art-lojban` dá `jbo`, `sh` fica `sh`, `en-1996-1994` dá
  `en-1994-1996` (variantes ordenadas), `en-u-ca-gregory-ca-buddhist` dá `en-u-ca-gregory` (chave repetida
  descartada), `en-u-ca-islamicc` e afins, `ca-valencia`, `tlh`, `swc`; inválidos com
  `RangeError: invalid language tag: <tag>`: `x-private`, `i-klingon`, `i-default`, `zh-min-nan`, `zh-cmn-hans`,
  `no-bok`, `sgn-be-fr`, `en-gb-oed`, tag vazia, `en_US`, `en-`, `a`, `en-US-u`.
- LACUNA DE DADOS: `supportedLocalesOf` filtra por locales com dados; a porta só tem en e pt, o bun tem o ICU
  inteiro (`fr`, `ja`, `he` entram no bun). O golden de supportedLocalesOf mantém `["iw"]` (bun: `["he"]`) e
  `["zh-xx-yy-zz"]` de propósito: devem falhar na porta até haver dados dessas línguas, é a medida da lacuna.
- CONFERIDO (estático, sem compilar) `Intl.Segmenter` contra `icu_segmenter-2.3.0` (fonte em `~/.cargo/registry/src`):
  `GraphemeClusterSegmenter::new()` (const, sem argumento, `compiled_data`), `SentenceSegmenter::new(SentenceBreakInvariantOptions)`
  e `WordSegmenter::new_auto(WordBreakInvariantOptions)` (esta exige as features `compiled_data` e `auto`, ambas padrão; o
  `Cargo.toml` não desliga). Os três devolvem `*Borrowed<'static>` com `segment_utf16(&[u16])`, cujo iterador é
  `Iterator<Item = usize>` com offsets em unidades de código UTF-16, então `intl_segmenter.rs` usa `segment_utf16` sobre as
  unidades do JS e não há conversão de UTF-8 a fazer (índices de `containing` e `index` já são UTF-16). `is_word_like()` e
  `word_type()` são métodos de `WordBreakIterator` (`&self`) e descrevem o segmento que precede o limite atual, o que bate com o
  uso (o limite 0 não gera entrada). Nenhuma correção de código foi necessária. Pendência só de medição: `word_type` do
  segmento de dicionário/LSTM (CJK, tailandês) contra o bun, no `tests/segmenter_bun_golden.rs`.


## Lista única de locales disponíveis (2026-10-08)

- O que mudou: `intl_locale_data::language_has_data` (o ponto único de `BestAvailableLocale`, usado por `resolve_locale`,
  `supported_locales` e `best_available_locale`) deixou de perguntar ao `LocaleExpander` do icu4x e passou a consultar
  `AVAILABLE_LANGUAGES` (393 códigos, com os de três letras e os apelidos `iw`, `in`, `ji`, `mo`, `jw`), em
  `src/runtime/intl_available_locales_data.rs`. O Collator usa `COLLATOR_LANGUAGES` (117) e `COLLATOR_LOCALES` (35) do
  mesmo arquivo, via `collator_locale` e o mesmo `best_available_by`; as listas escritas à mão em
  `intl_collator_tailoring.rs` saíram.
- Medido no bun 1.4.2: as oito classes (DateTimeFormat, DisplayNames, DurationFormat, ListFormat, NumberFormat,
  PluralRules, RelativeTimeFormat, Segmenter) têm exatamente o mesmo conjunto de 393; o Collator tem 227 (a lista própria de
  `ucol_getAvailable`, com os 3 letras). `Intl.Locale` aceita qualquer tag bem formada. Região e escrita não entram na lista:
  `fr-XX`, `fr-Cyrl`, `en-Zzzz` resolvem como pedidos nas oito classes (o Collator só os lista quando constam em
  `COLLATOR_LOCALES`, senão cai na língua: `fr-XX` resolve `fr`). `xx`, `tlh`, `und-Latn` resolvem `en-US`.
- Gerado por `bun scripts/gen-available-locales.js` (Bash, permitido para gerar dados; sondagem de aa..zzz). O mesmo script
  escreve `tests/golden/available_locales_bun.tsv` (120 tags x 12 programas = 1440 linhas: `resolvedOptions().locale` das nove
  classes, `supportedLocalesOf` de NumberFormat e Collator, `Intl.Locale` e as mensagens de RangeError, incluindo `i-klingon`,
  `root`, `de-CH-1996`, `es-419`, `zh-Hant-TW`). O teste `tests/intl_available_locales_bun_golden.rs` foi criado com `sed`
  (Bash) a partir do teste vizinho, porque é cópia mecânica dele. Nada compilado nem executado (cargo proibido).
- O que continua faltando é só o dado de formatação: os textos de meses, unidades e nomes de `fr`, `de`, `ja`... ainda
  saem das tabelas `en` e `pt`. A seção "Locales" acima passa a ser só sobre dados, não sobre resolução. As linhas "o locale não
  suportado cai em `en-US`" e "regiões de `en` e `pt` resolvem para o genérico" já estavam desatualizadas (ver
  `intl-locale-flow.md`).
- Risco para a primeira compilação: o teste `best_available_truncates_subtags` espera `zh-Hant-TW`, `pt-PT`, `fr` e `None` para
  `xx` e `und`; com a lista nova continuam válidos, mas `und` e `root` dependem de `und` não constar (não consta).

### RelativeTimeFormat nas 38 locales (2026-10-08)

- `scripts/gen-reltime-golden.js` passou de 10 para 37 línguas: as 10 antigas mais `ko zh fa he th tr pl nl sv da nb fi cs el id vi uk
  ro hu bg hr sr` e as variantes regionais que o bun mede diferentes da base (`en-GB`, `pt-PT`, `es-MX`, `fr-CA`, `zh-TW`;
  `de-AT` dá 0 diferenças contra `de`, então fica de fora). Regenerado com `bun scripts/gen-reltime-golden.js` (Bash, permitido
  para rodar o gerador): 888 padrões, 2206 textos de `auto`, golden `tests/golden/reltime_bun.tsv` com 45264 linhas.
- `data_language` em `intl_relative_time_format.rs` agora tenta `língua-região` antes da língua. Nada compilado (cargo proibido).
- Limite conhecido: os padrões saem de amostras inteiras 0..200 e 1000000; categorias de plural que só aparecem em fração (`many`
  de `cs`, `fr` com fração etc.) ficam vazias e caem em `other`. O golden tem frações (0.5, 1.5) e vai acusar se divergir.
- Continua faltando: DurationFormat (`intl_duration_format.rs`) e DisplayNames (feito nas 38, falta compilar).

### DurationFormat nas 38 locales: dados e golden (2026-10-08)

- Novo `scripts/gen-duration-format-data.js` (rodado com `bun scripts/gen-duration-format-data.js`): mede o `formatToParts` de
  durações de uma unidade só (10 unidades x long/short/narrow x amostras 0..1000, uma por categoria de plural do
  `Intl.PluralRules`) nas 38 locales e escreve `src/runtime/intl_duration_format_data.rs` (2430 padrões, 519 textos no `POOL`,
  205 KB) com o separador do estilo digital por locale. Cada padrão é uma lista de partes `n` (número), `l` (literal), `u`
  (unidade): o JSC deixa o ICU decidir se o número aparece (`ar` dual vira só `ساعتان`) e se há espaço (`de` narrow `2h`).
- Golden: `tests/golden/duration_format_bun.tsv` (4560 linhas: 30 durações x 4 estilos x 38 locales, `formatToParts`) com as
  durações em `tests/golden/duration_format_durations.json`; teste novo `tests/duration_format_bun_golden.rs`.
- LIGADO (sem compilar, cargo proibido): `intl_duration_format.rs` lê `PATTERNS`/`POOL`/`DIGITAL_SEPARATORS` por locale
  (`data_locale` resolve a primeira tag pedida nas 38 com `best_available_by`; categoria de plural por `icu_plural::select`
  na tag pedida; o padrão `n/l/u` decide número e espaço). `UNITS_EN`/`UNITS_PT`, `unit_label`, `is_plural` e
  `intl_list_format::language_tag` saíram. `list_parts` já usava o `icu_list` para qualquer locale, então a lista `unit`
  das 38 funciona. Falta: compilar e rodar `tests/duration_format_bun_golden.rs`.
- DurationFormat, número por locale (2026-10-08, sem compilar, cargo proibido): o número de cada unidade agora sai do
  mesmo caminho do `Intl.NumberFormat` (`format_parts` sobre `icu_number.rs`) com o locale resolvido e o sistema numérico
  (opção `numberingSystem` ou `-u-nu-`). A resolução do sistema numérico foi extraída de `intl_number_format::initialize`
  para `resolve_numbering_system`, usada pelos dois (sem cópia). `DurationFormatState` ganhou `numbering_system` e
  `number: NumberSettings` (no lugar de `language`); `resolvedOptions().numberingSystem` e a tag `locale` refletem a
  resolução. Mantém `minimum_integer_digits` 2 em `2-digit`, sem agrupamento em numeric e 2-digit, `fractionalDigits` com
  truncamento. Pendente: compilar e conferir ar, fa, hi, th, de, fr, sv e ru no `duration_format_bun_golden.rs`; o
  separador `:` do digital vem de `DIGITAL_SEPARATORS` e não troca com o sistema numérico.
- DisplayNames nas 38 locales: `scripts/gen-display-names-data.js` mede o bun 1.4.2 (language 200 códigos com dialect e
  standard, region AA-ZZ, script ~50, currency 50+, calendar, dateTimeField; long/short/narrow, fallback none) e emite uma
  `MoreTable` por locale em `intl_display_names_data_more.rs` (837 KB). en-GB, pt-PT, es-MX, fr-CA, de-AT e zh-TW guardam só
  as linhas que diferem do pai (campo `parent`; nome longo vazio é lápide). `table_for_locale` casa locale exato, senão a
  língua, e manda `zh` com Hant, TW, HK ou MO para `zh-TW`. O gerador falha se as tabelas não regerarem o bun em ~600
  composições por locale. `narrow` de language e region é igual a `short` nos 38 (medido, 0 divergências). Golden:
  66994 linhas (2,8 MB). Lacunas: sem pool de strings (o limite de 1,5 MB já fecha só com o diff do pai); locales fora da
  lista (es-419, en-AU, pt-BR...) usam a tabela da língua, não a regional do ICU; nada compilado nem testado (cargo
  proibido): o `MoreTable::lookup` com ponteiros de função e as `static` que referenciam `&EN` etc. precisam de cargo.
  Nota: o gerador foi rodado com Bash (permitido) e a edição dele passou por script Python, não Edit.
- DisplayNames, segunda leva (2026-10-08): `icu_experimental`/`icu_displaynames` NÃO existem no registro local do cargo
  (só `icu_locale_data`, que não traz nomes de língua/região/escrita/moeda), então não dá para ligar crate. O gerador ganhou
  31 locales (sk sl lt lv et sw ta te ur bn ca eu gl ms is ga af sq mk be ka hy az kk uz mr gu kn ml ne si), 69 no total, e foi
  rodado: as tabelas regeraram o bun em todas as composições. `intl_display_names_data_more.rs` foi a 1,69 MB (passou de 1,5 MB;
  o corte para tabela compacta com `intl_table_lookup.rs` fica pendente) e o golden a 121647 linhas (5,4 MB). Faltam ~320
  locales de `intl_available_locales_data.rs` (os de cauda longa caem na tabela da língua ou no código). Não compilado.
- Texto por locale (2026-10-08, sem compilar, cargo proibido): `scripts/gen-text-locale-golden.js` mede no bun 1.4.2 3612
  programas em `tests/golden/text_locale_bun.tsv` (Segmenter grapheme/word/sentence em 60 textos x 15 locales com
  `containing` e formato do resultado, `localeCompare`/`Collator.compare` em 60 pares x 25 locales com opções em rodízio,
  `toLocaleUpperCase/LowerCase` em 13 locales, `normalize` nas 4 formas, `resolvedOptions`, `supportedLocalesOf`);
  `tests/text_locale_bun_golden.rs` roda tudo. Lacunas vistas na leitura do porte, ainda sem rodar o golden:
  - Collator: só há ajuste por locale para sv, fi, da, nb/no/nn, tr, pl, cs, sk, es (e `trad`), lt, hr/bs/sr, sl, ro, hu e
    `de-u-co-phonebk`. Faltam `zh` (pinyin é o padrão), `zh-u-co-stroke`, `ja` (kana antes de han, marca de prolongamento),
    `ko`, `uk` (ґ, ї, є), `th` (vogais prefixadas), `hi`, `el`, `he`, `ar`: caem na raiz. A raiz ordena escritas fora do
    latim por ponto de código, não pelo DUCET (árabe e hebraico com marcas e formas finais divergem). Os tailorings só
    ancoram em letras latinas, então cirílico (`uk`) exige estender `letters`/`KEY_SCALE`.
  - Collator, ligado em 2026-10-08 (sem compilar, cargo proibido): `icu_collator = "2.3"` entrou no `Cargo.toml` (o
    registro local já tem 2.3.1 e `icu_collator_data` 2.3.0; o `Cargo.lock` se atualiza no primeiro build) e
    `intl_collator.rs` ganhou `CollatorSettings::icu` (`Arc<CollatorBorrowed>`, o struct deixou de ser `Copy`/`Debug`).
    Para `zh ja ko uk th hi el he ar` (`ICU_COLLATED_LANGUAGES`, só com `usage: "sort"`) a comparação inteira vai para o
    `icu_collator`: tag resolvida com `-u-co-` honrada (cai para a tag sem extensões se os dados não têm a colação),
    `caseFirst` e `numeric` por `CollatorPreferences`, `sensitivity` por `Strength` mais `CaseLevel` (case = primary +
    case level), `ignorePunctuation` por `AlternateHandling::Shifted`. `extra_collations` agora lista o que o bun aceita
    (medido): zh `pinyin stroke zhuyin unihan`, ja e ko `unihan`, ar `compat`. Os demais locales seguem nas tabelas
    manuais (nada mudou). Pendente: compilar; conferir `text_locale_bun_golden.rs`/`collator_bun_golden.rs`; checar se o
    `icu_locale_core` da árvore unifica em 2.3 (o lock tem 2.1.1 e 2.3.0); ru, bg, fa e demais cirílicos/arábicos
    seguem na raiz manual (candidatos a mover para `ICU_COLLATED_LANGUAGES` se o golden mostrar divergência).
  - Collator: `usage: "search"` continua sem efeito (os dados compilados do ICU4X não trazem colação `search`).
  - Segmenter: o locale não muda regra nenhuma (o ICU4C do bun também usa as regras da raiz sem `@ss=`), mas o
    dicionário CJK e o LSTM tailandês do ICU4X podem divergir do ICU4C em texto raro; só o golden mede.
  - Caixa por locale: `CASING_LOCALES` é az, el, lt, tr (como o ICU). `nl` (ij) não muda em maiúsculas/minúsculas, só em
    title case, que o JS não expõe. O lituano usa tabela própria só para U+0300..U+036F.
  - Corrigido agora: `el` em maiúsculas (`greek_upper` em `intl_case_mapping.rs`): `ΐ` e `ΰ` viram `Ϊ` e `Ϋ`, espíritos
    e perispomeni saem, `ᾀ` vira `ΑΙ`, medido no bun (`"ΐΰᾀᾳάός"` vira `ΪΫΑΙΑΙΑΟΣ`).
    Antes só o tonos precomposto de U+0386..U+038F saía. Falta compilar e rodar `text_locale_bun_golden.rs`.
  - Golden misto novo, 2026-10-08 (sem compilar, cargo proibido): `scripts/gen-intl-misc-golden.js` gera
    `tests/golden/intl_misc_bun.tsv` (1812 programas medidos no bun 1.4.2) e `tests/intl_misc_bun_golden.rs` o roda.
    Cobre o que `intl_more`, `plural`, `reltime`, `locale_more` e `intl_object` não tinham: `ListFormat.format` com 2 a 6
    itens (25 locales, 3 types, 3 styles) e `resolvedOptions`; `PluralRules.selectRange` (8 pares, cardinal e ordinal)
    e `resolvedOptions` em 25 locales, mais erros de `select`/`selectRange`; opções do construtor de `Intl.Locale`
    sobrepondo a tag (language, script, region, calendar, collation, hourCycle, caseFirst, numeric, numberingSystem,
    variants) e erros de tag; `Intl[Symbol.toStringTag]`, propriedades próprias, descritores, `prototype`, `constructor`,
    `supportedLocalesOf`, chamada sem `new` (mensagem exata) e `this` inválido nos 10 construtores e em cada getter e
    método de `Locale`. RelativeTimeFormat, Segmenter, getCanonicalLocales, supportedValuesOf e os getters de Locale
    já tinham golden e ficaram de fora. Pendente: compilar e rodar `intl_misc_bun_golden.rs`; leitura de
    `intl_plural_rules.rs`, `intl_list_format.rs` e `intl_locale.rs` contra o golden não achou divergência óbvia nas
    mensagens (ListFormat e PluralRules batem com o bun), o resto só o teste mede.
- DisplayNames, dados compactados (2026-10-08, sem cargo): `intl_display_names_data_more.rs` caiu de 1,69 MB para 1,26 MB de fonte e a tabela estática do binário encolhe mais (cada texto vive uma vez em `STRINGS`, 30094 únicos; as linhas viraram tuplas de índices u16, com 0 para a string vazia: nome curto igual ao longo, símbolo ausente e lápide). As linhas saem ordenadas pela chave e `MoreTable::lookup` faz busca binária por `intl_table_lookup::sorted_position_by` (a mesma que `sorted_index`). Os índices menores são dos textos mais frequentes. O golden `display_names_bun.tsv` saiu byte a byte idêntico (`cmp` contra a cópia anterior). Pendente: compilar (o consumidor em `intl_display_names.rs` não foi compilado) e, se a fonte precisar encolher mais, guardar só o que difere de `en` (tem de respeitar as lápides).
- Golden de DateTimeFormat, 2026-10-08 (sem compilar, cargo proibido): `scripts/gen-datetime-range-golden.js` gera
  `tests/golden/datetime_range_bun.tsv` (3303 programas medidos no bun 1.4.2, fuso sempre dentro das opções) e
  `tests/datetime_range_bun_golden.rs` o roda. Cobre `formatRange`/`formatRangeToParts` em 25 locales (mesmo dia, mês,
  ano, anos distintos, 4 fusos com `timeZoneName`), `hourCycle` h11/h12/h23/h24, 7 calendários, 4 numberingSystems,
  `era`/`yearName`/`relatedYear`, datas inválidas (NaN, Infinity, undefined) e as mensagens de RangeError/TypeError,
  tipos de `formatToParts`, `resolvedOptions` (JSON) por combinação e `toLocaleString/DateString/TimeString`. Os
  `date_pattern`, `date_tz` e `intl_more` não cobriam `formatRange` fora de `en`/`pt`. Leitura de
  `intl_date_time_format.rs` e `intl_date_time_format/temporal.rs` contra o golden: as mensagens de data inválida
  ("Passed date is out of range", "startDate or endDate is undefined", "date value is not finite in DateTimeFormat
  format()/formatToParts()") já batem; nenhuma divergência óbvia achada, então nada foi editado em `src/`. Pendente:
  compilar e rodar o teste; esperam-se falhas nos locales sem dados (só `en` e `pt` têm), nos calendários não gregorianos
  e em `yearName`/`relatedYear`.
- formatRange, `hu` e `fa` na tabela (2026-10-08, sem compilar): `LOCALES` do gerador ganhou `hu` e `fa` (tabela
  regenerada com o bun 1.4.2). `fa` é medido com `FORCED` (calendar gregory e numberingSystem latn, também nos programas
  do golden), porque o padrão do bun é o persa com dígitos persas; `data_range` agora desiste quando
  `state.native.date(...)` é `Some` (th budista, fa persa passam pelo caminho antigo), o que também corrige os locales já
  na tabela com calendário nativo padrão. Cenários novos `same_day_seconds` e `time_only_seconds` (segundos ou fração:
  sem padrão de intervalo, separador do fallback e sem juntar o AM/PM), escolhidos em `data_range` por um sufixo
  da tabela, sem cópia por cenário. Era, `timeZoneName` e `dayPeriod` ainda não têm tabela: `data_range` devolve `None` e
  eles caem no fallback de `range.rs` (medido: o ICU põe o fuso uma vez no fim, `7:08 AM – 7:08 PM UTC` em en, `07:08–19:08
  Uhr UTC` em de, `7時08分～19時08分(UTC)` em ja; era vai depois do intervalo, `05.–09.03.2024 n. Chr.` em de; em fa o
  fuso fica entre parênteses e se repete). Pendente: compilar e rodar `datetime_range_bun_golden.rs` e
  `datetime_more_bun_golden`; os dois ganham linhas de `hu` e `fa`.
- DisplayNames, terceira leva de locales (2026-10-08, sem compilar): `LOCALES` do gerador passou de 69 para 94 com am, my, km, lo, mn, ps, sd, so, fil, ha, yo, zu, xh, cy, gd, lb, mt, fo, ky, tg, tk, tt, ku, or, as (todos conferidos no bun 1.4.2: `resolvedOptions().locale` igual ao pedido, não caem para en). Regenerado: `intl_display_names_data_more.rs` 1,26 MB para 1,73 MB (alvo menor que 2,5 MB, o pool de `STRINGS` coube em u16) e `display_names_bun.tsv` com 165722 linhas (7,4 MB). Fora: `pa` (o gerador falha em `detailsPattern`: o ICU escreve a região como `ਸੰਯੁਕਤ ਰਾਜ [ਅਮਰੀਕਾ]`, com sufixo entre colchetes que o padrão derivado não cobre) e `uz-Cyrl` (`table_for_locale` só distingue região, não escrita). `intl_available_locales` não precisou de edição: a lista vem de `intl_available_locales_data` (gerada à parte), e `table_for_locale` é gerada. Pendente: compilar e rodar `display_names_bun_golden.rs`.
- Locales da terceira leva do DisplayNames contra as demais classes (2026-10-08, sem compilar): sondados no bun 1.4.2 os 25 (am my km lo mn ps sd so fil ha yo zu xh cy gd lb mt fo ky tg tk tt ku or as) em `supportedLocalesOf` e `resolvedOptions().locale` das nove classes. Resultado: as oito classes que compartilham `intlAvailableLocales` aceitam os 25 e resolvem o próprio locale; o `Collator` aceita 20 e cai em `en-US` para `sd`, `so`, `gd`, `tg` e `tt` (`supportedLocalesOf` vazio). `AVAILABLE_LANGUAGES` já tem os 25 e `COLLATOR_LANGUAGES` só os 20 (conferido por grep: `sd so gd tg tt` aparecem uma vez, nas disponíveis), então a resolução do porte já bate com o bun e nenhuma edição de lista foi necessária. Lacuna aberta, não verificável sem compilar: se o icu4x com dados compilados tem formatação (decimal, plurais, lista, calendário, colação) para todos esses locales, em especial `ku`, `ps`, `sd`, `tt`, `tg`, `gd`; cada consumidor cai no fallback `en`/`root` do icu4x quando falta, e o bun usaria o dado próprio. Pendente: rodar `intl_available_locales_bun_golden` e ampliar `gen-intl-golden.js` com essas tags para medir a formatação.
- Formatação nos 25 locales da terceira leva, golden medido (2026-10-08, sem compilar, cargo proibido): `scripts/gen-intl-more-locales-golden.js` gera `tests/golden/intl_more_locales_bun.tsv` (1700 expressões medidas no bun 1.4.2, nenhuma lançou) e `tests/intl_more_locales_bun_golden.rs` roda seis testes (NumberFormat decimal, percent, currency USD e compact curto e longo; PluralRules cardinal e ordinal em 0,1,2,3,5,11,21,100,1.5; ListFormat conjunction e disjunction com 2 e 3 itens; RelativeTimeFormat day e month em -1,0,1,2 com e sem `numeric: auto`; DateTimeFormat dateStyle full e medium e timeStyle short em UTC; Collator em 5 pares). Leitura de `Cargo.toml` e `src/runtime`, lacunas que são só dado ausente (o teste deve falhar nelas até as tabelas ganharem os locales):
  - DateTimeFormat: `intl_date_time_data::locale_data` não tem nenhum dos 25 (só 38 tags); gerar com `scripts/gen-datetime-data.js` acrescentando as 25 a `LOCALES`. `icu_datetime` não está ligada, então não há outro caminho.
  - RelativeTimeFormat: `LANGS` de `scripts/gen-reltime-golden.js` (37 línguas) não tem nenhum dos 25; mesmo remédio em `intl_relative_time_data.rs`.
  - NumberFormat percent, currency e unit: `icu_number_data` só tem as línguas de `BASE_LOCALES` e `EXTRA_LOCALES` de `scripts/gen-number-format-data.js`, nenhuma das 25; os afixos caem na tabela escrita à mão (en). Decimal e compact vêm do `icu_decimal` (dados compilados).
  - PluralRules, ListFormat, decimal e compact (`icu_plurals`, `icu_list`, `icu_decimal`, sem `default-features = false`, logo com `compiled_data`) e Collator (`icu_collator`, só para as 20 tags de `COLLATOR_LANGUAGES`; `sd so gd tg tt` caem em `en-US` como no bun): o baked data do icu4x cobre o CLDR moderno, mas a presença de dado para `ku`, `ps`, `sd`, `tt`, `tg`, `gd`, `fo`, `lb`, `mt` não foi verificada sem compilar; o teste decide, e o que divergir sem erro de lógica é dado ausente.
- Golden do Segmenter em pt, de, ko, hi (2026-10-08, sem compilar): conferido que Collator (11 variantes de opção x 38 locales), PluralRules, ListFormat, RelativeTimeFormat (`intl_more_bun.tsv`, 23498 linhas) e DurationFormat (`duration_format_bun.tsv`) já tinham golden; o único buraco era o Segmenter, que só cobria en, ja, zh, th. `scripts/gen-segmenter-locales-golden.js` gera `tests/golden/segmenter_locales_bun.tsv` (612 programas: 51 textos x 3 granularidades x 4 locales, com emoji, bandeiras, hangul, devanágari, pontuação e números) e `tests/segmenter_locales_bun_golden.rs` roda o mesmo programa do golden antigo. Medido no bun: pt, de, ko e hi dão resultado idêntico para todos os textos, então o ICU não tem tailoring de segmentação nesses locales e os dados da raiz do ICU4X (`intl_segmenter.rs`) bastam. Nenhuma divergência de código achada por leitura; pendente compilar e rodar o teste novo.
- DateTimeFormat e RelativeTimeFormat nos 25 locales da terceira leva (2026-10-08, sem compilar): `LOCALES` de `scripts/gen-datetime-data.js` e `LANGS` de `scripts/gen-reltime-golden.js` ganharam am my km lo mn ps sd so fil ha yo zu xh cy gd lb mt fo ky tg tk tt ku or as, regenerados com o bun 1.4.2. Padrão medido no bun: gregoriano e latn em todos, exceto `my` (mymr), `sd` (arab), `as` (beng) e `ps` (calendário persa e arabext); esses quatro entraram em `FORCED` (ps com gregory e latn, my, sd e as só com latn), como `fa`, e o numeral nativo continua a cargo do consumidor, que já honra `numberingSystem`. `intl_date_time_data.rs` foi de 2,28 MB para 3,83 MB de fonte (ACIMA do alvo de 3 MB; falta deduplicar as chaves de skeleton repetidas por locale, 349 por locale) e `intl_relative_time_data.rs` de 0,43 MB para 0,74 MB. Consumidores: `locale_data_for` tenta `língua-REGIÃO` e depois a língua, e `LANGUAGES` do reltime vem da tabela gerada, então nenhuma lista de locales suportados precisou de edição (`data_range` em `range.rs` já desiste quando há data nativa, o que cobre `ps`). Pendente: compilar, rodar `datetime_more_bun_golden`, `datetime_range_bun_golden`, `intl_more_locales_bun_golden` e o golden do reltime (`reltime_bun.tsv` regenerado, 72864 linhas); o golden de range não foi regenerado com os 25 (`gen-datetime-range-golden.js` tem lista própria de locales). Observação: o reltime foi editado com `sed` no `LANGS` (uma linha, mudança mecânica).
- formatRange com `era`, `timeZoneName` e `dayPeriod` (2026-10-08, sem compilar, só `range.rs` e o gerador): medido no bun 1.4.2 em en pt es fr de ja zh ru ko ar hi it. O ICU não cai no fallback para esses campos: usa o padrão do intervalo SEM o campo e o acrescenta uma vez (`shared`) onde o padrão completo o põe. Era com a mesma era: `2020 – 2024 AD`, `6 – 4 BC`, `紀元前6年～4年`, `BC 6년~4年`, `ईसा-पूर्व 6–4` (prefixo ou sufixo conforme o idioma). Era diferente (atravessa o ano 1): fallback com as duas pontas, `6 BC – 5 AD`, `6 av. J.-C. à 5 ap. J.-C.` (fr usa ` à `), ja `～`, ko ` ~ `, demais ` – ` (U+2013). Fuso no mesmo dia: `4:08 AM – 4:08 PM GMT-3`, `07:08–19:08 Uhr UTC`, `UTC 07:08–19:08` (zh, prefixo), `7時08分～19時08分(UTC)`; `long` vira o nome curto (`GMT-3` em vez de `Brasilia Standard Time`) e `shortGeneric` se mantém. Fuso com a data diferente: fallback com as pontas inteiras e o nome pedido (long fica long). `shortOffset` e `longOffset` SOMEM do intervalo no bun (mesmo com a data diferente), embora o `format` os escreva. `dayPeriod`: `B` só troca o texto do AM/PM: `7 in the morning – 7 in the evening` (períodos diferentes, separador do cenário `same_day`), `7 – 8 in the morning` (mesmo período, junta). Fallback de pontas inteiras com a hora e data diferente: ` – ` exceto ja `～`, ko ` ~ `, sv `–` (sem espaços), el ` - `, e pt agora ` – ` (o ` - ` antigo não bate com o bun). Implementação: `special_range` em `range.rs` formata o intervalo de um estado SEM o campo (`plain_state`), e `decoration` tira a diferença de texto entre as pontas com e sem ele (`with`/`without`, prefixo ou sufixo) e a põe `shared`; `plain_range` é a antiga cadeia (tabela medida, fallback, data). `data_range` continua devolvendo `None` com esses campos, que agora nunca chegam a ele (o estado limpo não os tem). Gerador: `RANGE_LOCALES` (12 locales) com era short/long (4 pares, ano e ymd, e parts), 5 `timeZoneName` x {America/Sao_Paulo, UTC} (mesmo dia, outro dia, parts) e `dayPeriod` short/long/narrow; TSV regenerado de 6178 para 6958 linhas. Lacunas: era com `dateStyle`/`timeStyle` e combinações (era+fuso), `longGeneric` no mesmo dia (suposto curto), fuso diferente entre as pontas (horário de verão) usa o da primeira, e `dayPeriod` com hora/dia que muda `B` sem mudar AM/PM. Pendente: compilar e rodar `datetime_range_bun_golden.rs`.
- DateTimeFormat, dados compactados (2026-10-08, sem cargo): `intl_date_time_data.rs` caiu de 3,83 MB para 1,17 MB de fonte. Um pool único `STRINGS` (10492 textos, u16, os mais frequentes com os índices menores) guarda chaves de skeleton, padrões, nomes e valores avulsos; cada locale ficou com `[u16]` para meses, dias, eras, AM/PM e UTC, e `entries`/`extras` viraram `&[(chave, valor)]` de índices, ordenados pela chave. O consumidor está no próprio template do gerador (o `LocaleData` é gerado): `string_at(index)` é o único acesso ao pool, `value_of(rows, key)` (busca binária por `intl_table_lookup::sorted_position_by`) serve `extra` e `pattern`, e `day_period` junta as duas leituras de AM/PM; `intl_date_time_format.rs` e `range.rs` só usam a API pública e não mudaram. `datetime_more_bun.tsv` e `datetime_gaps_bun.tsv` saíram idênticos (`cmp`; ambos untracked, então `git diff --stat` não vale). Pendente: compilar.
- Golden de range nos 25 locales da terceira leva (2026-10-08, sem compilar): `LOCALES` de `scripts/gen-datetime-range-golden.js` ganhou am my km lo mn ps sd so fil ha yo zu xh cy gd lb mt fo ky tg tk tt ku or as (25 para 50 locales; sem `FORCED`, porque o gerador de range mede o padrão real do bun, como já faz com `fa`). `tests/golden/datetime_range_bun.tsv` regenerado com o bun 1.4.2: 3303 para 6178 linhas; as 3303 antigas continuam todas presentes (conferido com `sort` e `comm -23`, zero perdidas), mas as linhas novas se intercalam por seção (o laço é por seção, depois por locale), então o md5 do prefixo muda por construção. Dados de intervalo: `gen-datetime-data.js` mede o intervalo direto do bun (`formatRangeToParts` por cenário, separador e colapso) para todos os `LOCALES`, e os 25 estão em `locale_data` com as chaves `range|<cenário>|sep` e `range|<cenário>|collapse`; logo `data_range` não cai em `None` por falta de tabela nesses locales. Os únicos `None` são os de sempre: data nativa (`state.native.date`, cobre `ps` e, se o consumidor reconhecer, `my`, `sd`, `as` quando o numeral nativo for pedido), era, `timeZoneName` e `dayPeriod`, que seguem o fallback de `range.rs`. Lacuna aberta: no golden de range `my`, `sd`, `as` e `ps` saem com numeral e calendário nativos do bun, e `data_range` desiste nesses casos quando há data nativa; se o consumidor não renderizar o numeral nativo no fallback, essas linhas divergem. Pendente: compilar e rodar `datetime_range_bun_golden`.
- Numeral e calendário nativos no formatRange e no calendário padrão (2026-10-08, sem compilar): medido no bun o calendário e o numeral padrão dos 100 locales de `LOCALES` mais `en`/`pt`: fora de `gregory`/`latn` só `th` (buddhist, latn), `fa` e `ps` (persian, arabext; `ps-PK` gregory), `my` (mymr), `sd` (arab), `as` e `bn` (beng), `mr` e `ne` (deva). O numeral padrão sai de `icu_number::default_numbering_system` (zero do locale no icu4x) e `calendar_parts` aplica `digits_of` a todas as partes numéricas, então o fallback `time_range`/`date_range` de `range.rs` (que formata cada ponta por `date_parts`/`time_parts`/`parts_with_fields`, o caminho normal) já renderiza o numeral nativo; nenhum código novo foi preciso no intervalo. Divergência achada e corrigida: o calendário padrão por locale não existia (só `calendar`/`-u-ca-`), logo `th`, `fa` e `ps` saíam gregorianos e `resolvedOptions().calendar` dava `gregory`; `default_calendar` em `intl_date_time_format.rs` agora devolve buddhist para `th` e persian para `fa`/`ps` (não `ps-PK`) quando nada foi pedido. Lacunas abertas: em `ps` o bun acrescenta `era: "short"` e `month`/`day` `2-digit` ao padrão (`AP ۱۴۰۲-۱۲-۱۵`), o porte não; `my`/`sd`/`as` dependem de o icu4x ter o zero nativo para o locale; `date_range` ainda usa `date_parts(language, ...)` sem o calendário nativo (o desvio por `state.native.date` em `data_range` e a linha 392 cobrem só o que já tinha tabela). Pendente: compilar e rodar `datetime_range_bun_golden`.
- Caixa por locale, fatia nova (2026-10-08, sem compilar): `text_locale_bun.tsv` já cobria 13 locales x 52 palavras (tr az lt el nl de en und e variantes) e as formas de `locales` em `string_bun`/`string_unicode_bun`; o buraco era sigma final em posições variadas, SpecialCasing completo em vários locales, formas exóticas do argumento e o lituano com marcas combinantes. `scripts/gen-text-locale-golden.js` ganhou 146 programas no FIM (16 palavras com sigma x {toLowerCase, toLocaleLowerCase com undefined, el, tr, lt, en}; 14 SpecialCasing em seis conversões; 40 formas de locale; 12 palavras lt), TSV de 3612 para 3758 linhas, e o prefixo de 3612 linhas saiu com o mesmo md5 (d95ceaa7...). Medido no bun: sigma final igual em todos os locales (ΑΣ vira ας, também com `.`, apóstrofo, U+00AD, U+200B, e `ΑΣΣ` vira ασς); `i-tr`, `x-tr`, `root`, `tr_TR`, `""`, `tr-u-ca-x`, `lt-u-x` e `tr-TR-u-co-x` são RangeError com a mensagem `invalid language tag: <tag>`; `tur`, `aze`, `tr-CY`, `TR`, `und-TR`, `en-u-tr` seguem a regra do idioma (tur e aze viram tr e az, `und-TR` e `en-u-tr` não); `null` dá `TypeError: null is not an object (evaluating ...)` (o gerador corta o trecho do call site). Lituano: `J` + U+0307 vira `j` + dois pontos, `Ì`/`Í`/`Ĩ` em minúsculas viram `i`+U+0307+acento sempre. Conferido por leitura contra `intl_case_mapping.rs` e `wtf/unicode/case_mapping.rs` (`is_final_sigma`): nenhuma divergência de código achada. Risco aberto: a mensagem de `toLocaleLowerCase(null)`, que no porte vem de `to_object(...).ok_or(Thrown::Pending)` em `canonicalize_locale_list`; se o `to_object` do porte não gerar `null is not an object`, a linha diverge. Pendente: compilar e rodar `text_locale_bun_golden`.
- Golden de Intl.Locale com `-u-`/`-t-`, aliases, getCanonicalLocales e supportedValuesOf inteiro (2026-10-08, sem compilar): `locale_more_bun.tsv` (1500) já cobria maximize/minimize, getTimeZones, getWeekInfo e as listas de `supportedValuesOf` (conteúdo e tamanho). O que faltava, agora em `scripts/gen-intl-locale-golden.js` e `tests/golden/intl_locale_bun.tsv` (1900 programas medidos no bun 1.4.2, teste em `tests/intl_locale_bun_golden.rs`): `resolvedOptions().locale` e os campos resolvidos (numberingSystem, calendar, hourCycle, collation, numeric, caseFirst) de NumberFormat, DateTimeFormat, Collator, PluralRules, ListFormat e RelativeTimeFormat para 20 tags com `-u-` (nu, ca, hc, co, kn, kf) e combinações com opções que sobrepõem a palavra-chave; todos os getters e getters de lista de Locale para 57 tags (aliases iw/he, in/id, sh, zh-CN, und, variantes, `-t-`, `-x-`); opções do construtor (31 x 5 bases); 30 tags inválidas com a mensagem exata, em `new Intl.Locale` e `getCanonicalLocales`; 105 canonicalizações; `supportedValuesOf` com tamanho, hash FNV-1a do join, 10 primeiros e 10 últimos, ordenação, unicidade e congelamento. Medido no bun e que o porte pode errar: o Collator devolve `locale` sem `-u-nu`, o PluralRules e o ListFormat devolvem só a língua, e DateTimeFormat e RelativeTimeFormat/NumberFormat mantêm as palavras-chave que usam. Divergências não conferidas por leitura (cargo proibido): comparar `intl_locale.rs` com o TSV ao rodar o teste. Pendente: compilar e rodar `intl_locale_bun_golden`.
- Golden de borda de NumberFormat, PluralRules e ListFormat (2026-10-08, sem compilar): `scripts/gen-intl-edge-golden.js` gera `tests/golden/intl_edge_bun.tsv` (624 expressões medidas no bun 1.4.2, nenhuma lançou) em 12 locales fora dos já cobertos (ga gl eu is br fy si ne bo ig wo ml; todos aceitos por `supportedLocalesOf` das três classes), e `tests/intl_edge_bun_golden.rs` roda três testes. NumberFormat (408 linhas, 34 por locale): notation compact short e long, scientific e engineering (inclusive com fração mínima), unit com unitDisplay short, long e narrow (kilometer-per-hour, celsius, byte compacto), currencyDisplay code, name e narrowSymbol, currencySign accounting, signDisplay exceptZero, negative e always, roundingMode halfEven e floor, roundingIncrement 5 e 250, trailingZeroDisplay stripIfInteger, roundingPriority morePrecision, useGrouping min2, decimal em string e bigint grandes, formatToParts (compact long, moeda name, scientific) e formatRange e formatRangeToParts (unit, moeda, compact). PluralRules: select de 14 valores em cardinal e ordinal, `minimumFractionDigits` e `notation: compact`, `pluralCategories` das duas variantes e `selectRange` cardinal e ordinal. ListFormat: conjunction, disjunction e unit em long, short e narrow com 2, 3 e 4 itens, mais `formatToParts` narrow. Risco aberto, só medível ao rodar: dados ausentes no icu4x compilado para `br`, `fy`, `bo`, `ig`, `wo`, `ml` (percent, currency, unit e range caem na tabela escrita à mão como na terceira leva de locales), e as categorias de plural de `ga` e `br` (cinco e seis categorias). Os resultados de formatRange contêm o en dash do ICU, que é dado medido e não texto nosso. Pendente: compilar e rodar `intl_edge_bun_golden`.
- Golden de Collator, Segmenter, getCanonicalLocales, supportedValuesOf e Locale (2026-10-08, sem compilar): `scripts/gen-intl-collator-golden.js` gera `tests/golden/intl_collator_bun.tsv` (1840 programas medidos no bun 1.4.2, 75 lançam) e `tests/intl_collator_bun_golden.rs` roda seis testes. As expressões já presentes nos outros goldens de Intl (collator, segmenter, locale, intl_*) são descartadas na geração. Collator: sensitivity x 12 pares, numeric (inclusive `-u-kn`), caseFirst, ignorePunctuation, usage search, `-u-co-` e `collation` em 15 pares, sort de listas em 24 locales (padrão, base, upper, numeric, localeCompare, reverso), `resolvedOptions` de 17 conjuntos de opções x 6 locales. Segmenter: grapheme (ZWJ, bandeiras inteiras e ímpares, tag flag, keycap, jamo, indic, CRLF, marca solta), word com `isWordLike` em en, ja e th sobre 28 textos (CJK, tailandês, árabe, hebraico, hindi, números com separador, apóstrofo), sentence em en, ja e de sobre 20 textos (abreviações, aspas, `。`, LS e PS), `containing`, `resolvedOptions`. Também getCanonicalLocales (89 tags), supportedValuesOf (6 chaves e chaves inválidas), maximize e minimize de 70 tags e todos os getters de calendário, hourCycle, collation, numeração e firstDayOfWeek de 33 tags. O teste embrulha cada fonte em `String(...)` porque muitas expressões devolvem número ou booleano. Risco aberto: segmentação de palavras em tailandês e japonês exige dicionário (o ICU usa dados de LSTM/dicionário), provável divergência no porte; `firstDayOfWeek` e `getTimeZones` dependem de a versão do bun expô-los. Pendente: compilar e rodar `intl_collator_bun_golden`.

## Golden de borda de DateTimeFormat (2026-10-08)

`tests/golden/datetime_edge_bun.tsv` (884 programas, gerado por `scripts/gen-datetime-edge-golden.js` no bun, nenhum repete outro golden) e `tests/datetime_edge_bun_golden.rs` (11 testes, um por seção, selecionada pela tag `/*hc*/`, `/*frac*/`, `/*tzname*/`, `/*cal*/`, `/*nu*/`, `/*dp*/`, `/*bc*/`, `/*ext*/`, `/*resolved*/`, `/*supported*/`, `/*err*/`). Cobre hourCycle contra hour12 conflitante, fractionalSecondDigits, os seis timeZoneName em Asia/Kolkata, Asia/Kathmandu, Australia/Adelaide e America/St_Johns, nove calendários com formatToParts, numberingSystem arab/deva/thai, dayPeriod, eras a.C., datas extremas, resolvedOptions por locale, supportedLocalesOf e a matriz de RangeError. Cada fonte captura a exceção e devolve `throw Nome: mensagem`, então a mensagem do erro entra na comparação. Risco aberto: calendários não gregorianos (chinese, hebrew, islamic-umalqura) dependem de dados do icu4x equivalentes aos do ICU do bun, e os nomes de fuso (longGeneric, shortGeneric) exigem as tabelas de metazone. Pendente: compilar e rodar `datetime_edge_bun_golden` (não rodado nesta tarefa).
- `formatRange` com `dateStyle`/`timeStyle` e `era` (2026-10-08, sem compilar). Medido no bun 1.4.2 (5 locales x 4 `dateStyle` x `timeStyle` short e medium, mesmo dia, mês e anos diferentes; era short e long em en, pt-BR, de, ja). Achados: (a) no mesmo dia o `DateIntervalFormat` usa a cola `standard` do CLDR entre data e hora, nunca a `atTime` do `format`: en full `Tuesday, November 14, 2023, 10:13 – 11:13 PM` (o `format` diz ` at `), pt-BR ` ` sem vírgula e sem `às` (`14 de novembro de 2023 22:13 – 23:13`), de `, ` (e `22:13–23:13 Uhr` só com `timeStyle` short), ar `، `. Em dias diferentes vale a cola do `format` (`às`, `um`, `at`) com as duas pontas inteiras. Corrigido em `range.rs`: tabela `SAME_DAY_JOINERS` (66 locales, medida) e `with_same_day_joiner` em `data_range` e `time_range`. (b) O espaço antes de AM/PM e em volta de U+2013 nos intervalos é U+0020 neste bun (nenhum U+2009 apareceu em en, pt-BR, de, ar); `de` usa U+2013 sem espaços só em intervalos de hora e dia (`14.–16. November 2023`, `22:13–23:13`). (c) A era já está coberta por `special_range` (era comum sai uma vez no fim, `Jan 1 – 5, 2020 AD`; era diferente cai em `era_fallback`); com `dateStyle` não há era. Lacunas abertas: `timeStyle` no mesmo dia em `ja` usa `22時13分～23時13分` com short e `22:13:20～23:13:20` com medium, e em dias diferentes `22:13`; `de` short põe ` Uhr` só no fim do intervalo; `ar` `dateStyle` full no mesmo mês repete o dia da semana (`الثلاثاء، 14 – الخميس، 16 نوفمبر`). Nenhum desses três foi implementado; a cola nova não foi rodada (falta cargo).
- `formatRange` com `timeStyle: "short"` (2026-10-08, sem compilar): medido no bun em ja, de, en, ar, zh e ko que `timeStyle: "short"` dá o mesmo intervalo que `{ hour: "numeric", minute: "numeric" }` (ja `22時13分～23時13分` mesmo na diferença de minuto, `22:13` quando só os segundos diferem, de `22:13–23:13 Uhr`, ko `오후 10:13~11:13`, zh `22:13–23:13`), enquanto o `format` do estilo dá `22:13`. Com `medium` (segundos) é o fallback sem padrão: ja `22:13:20～23:13:20`, de `22:13:20 – 23:13:20`, en `10:13:20 PM – 11:13:20 PM`; em dias diferentes as duas pontas inteiras (`2023/11/14 22:13～2023/11/15 23:43` em ja, `14.11.2023, 22:13 – 15.11.2023, 23:43` em de). Corrigido em `range.rs`: `short_time_skeleton_state` (só `timeStyle` short, sem `dateStyle`) troca o estilo pelo skeleton hora e minuto numéricos no começo de `data_range`, o que resolve as lacunas (1) ja short e (2) de short (` Uhr` só no fim, pela cauda `shared` que o `data_range` já extrai); medium já seguia o caminho de segundos. Lacuna (3) ar `dateStyle` full no mesmo mês: medido que en e de também repetem o dia da semana nos dois lados (`Tuesday, November 14 – Thursday, November 16, 2023`, `Dienstag, 14. – Donnerstag, 16. November 2023`, ar `الثلاثاء، 14 – الخميس، 16 نوفمبر، 2023`), logo não é exclusivo do ar e é a regra do CLDR (skeleton `yMMMEd`, maior diferença `d`: ambos os lados com o dia da semana, mês e ano uma vez no fim); em `data_range` o cenário `same_month` com `collapse` já corta o prefixo e o sufixo comuns, então o dia da semana de cada ponta permanece; não verificável sem rodar. No mês diferente (`yMMMEd`, maior diferença `M`) o mês aparece nas duas pontas (`الثلاثاء، 14 نوفمبر – الخميس، 14 ديسمبر، 2023`). Pendente: compilar e rodar `datetime_range_bun_golden`; o `ja` com `dateStyle` full usa `2023/11/14(火曜日)～2023/11/16(木曜日)`, cada ponta inteira, e continua sem verificação.
- Golden de RelativeTimeFormat, ListFormat e PluralRules em mais 20 locales (2026-10-08, sem compilar): `scripts/gen-reltime-more-golden.js` gera `tests/golden/reltime_more_bun.tsv` (700 expressões medidas no bun 1.4.2, nenhuma lançou) e `tests/reltime_more_bun_golden.rs` roda três testes. Locales fora de `reltime_bun.tsv`: ca sk sl lt lv et sq af ga gl eu is br fy si ne bn ta ml ur (todos aceitos por `supportedLocalesOf` das três classes). Por locale: RelativeTimeFormat com 26 combinações de style (long, short, narrow) e numeric (auto, always) sobre as oito unidades mais `quarters` no plural, valores -1, 0, -0, 0.5, 1.5, 2.5, 21, -1000000 e 1234567.891, mais três `formatToParts`; ListFormat conjunction long, disjunction short e unit narrow com 3, 2 e 4 itens; PluralRules cardinal e ordinal sobre nove valores e `pluralCategories`. Risco aberto, só medível ao rodar: `br`, `fy`, `ml` e `ga` (dados de relativo e de plural com cinco ou seis categorias) e o `-0` com numeric auto (`this year` vs `in 0 years`). Pendente: compilar e rodar `reltime_more_bun_golden`.
