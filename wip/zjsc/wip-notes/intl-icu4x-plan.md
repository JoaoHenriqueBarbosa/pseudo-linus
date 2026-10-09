# Plano: dados de locale do Intl por icu4x (dados compilados, Rust seguro)

Objetivo: trocar as tabelas escritas à mão (`default_number_format.rs`, `intl_date_time_format*`, `intl_display_names_data.rs`,
`intl_list_format.rs`, `intl_relative_time_format.rs`, regras de `intl_plural_rules.rs`, `intl_locale_data.rs`) pelo CLDR do icu4x,
para cobrir todos os locales como o ICU do bun (fr, de, ja, ar, hi, en-GB, pt-PT...). Sem libicu, sem `unsafe`.

## O que há no registry local (`~/.cargo/registry`, fontes e `.crate` presentes)

| Crate | Versão | Módulo do Intl | Situação |
|---|---|---|---|
| `icu_plurals` (+`_data`) | 2.3.0 | PluralRules, plural de unidade/moeda | disponível |
| `icu_decimal` (+`_data`) | 2.3.0 | NumberFormat (dígitos, separadores, agrupamento, compacto curto) | disponível |
| `fixed_decimal` | 0.7.2 | `Decimal` (arredondamento, `multiply_pow10`) | disponível |
| `icu_datetime` (+`_data`) | 2.3.0 | DateTimeFormat | disponível |
| `icu_calendar` (+`_data`) | 2.3.0 | calendários não gregorianos (japanese, buddhist, hebrew, hijri, persian, roc, coptic, ethiopian, indian, dangi/chinese) | disponível |
| `icu_time` (+`_data`) | 2.3.0 / 2.3.1 | fusos (nomes) | disponível |
| `icu_collator` (+`_data`) | 2.3.1 / 2.3.0 | Collator | disponível |
| `icu_locale` (+`_data`) | 2.3.1 | `LocaleExpander` (maximize/minimize), `names` (nomes de língua, região, escrita, variante) | disponível |
| `icu_locale_core` | 2.3.0 (2.1.1 também) | `Locale`, `LanguageIdentifier`, parse e canonicalização | disponível |
| `icu_locale_fallback` (+`_data`), `icu_provider`, `icu_pattern`, `zerovec`, `zerotrie`, `tinystr`, `writeable` | ok | infraestrutura | disponível |
| `icu_list` | AUSENTE | ListFormat | falta baixar |
| `icu_experimental` (relativetime, displaynames de moeda/calendário/dateTimeField, `units`, compact longo) | AUSENTE | RelativeTimeFormat, DisplayNames (moeda etc.), unidades | falta baixar |
| `icu_casemap`, `icu_segmenter` | AUSENTES | caixa por locale, Segmenter | fora do escopo desta camada |

`Cargo.toml` hoje: `icu_normalizer = "2"`. O lock já traz `icu_provider`/`icu_locale_core` 2.1.x; as novas crates pedem
`icu_provider >= 2.3`, então o primeiro `cargo` atualiza o lock (todas as `.crate` necessárias estão no cache).

## Cargo.toml (somente o que já está local; feito no passo 1)

```toml
icu_locale_core = "2"
icu_plurals = "2"      # default-features = compiled_data (dados de todos os locales no binário)
fixed_decimal = "0.7"
```

Para os passos seguintes (todas locais): `icu_decimal = "2"`, `icu_datetime = "2"`, `icu_calendar = "2"`,
`icu_time = "2"`, `icu_collator = "2"`, `icu_locale = "2"`. O `compiled_data` é default em todas; o custo é tamanho de
binário (alguns MB por crate), aceitável. A feature `unstable` não é necessária para os construtores `try_new`.
Lista que falta baixar (deixar no plano, não adicionar até estarem no registry): `icu_list = "2"`, `icu_experimental = "0.4"`
(feature `compiled_data`; versão a conferir contra o `icu` 2.3 do workspace icu4x).

## APIs reais (lidas das fontes)

- **PluralRules** (`icu_plurals`): `PluralRules::try_new(prefs: PluralRulesPreferences, options: PluralRulesOptions)`
  (com `PluralRulesOptions::from(PluralRuleType::{Cardinal,Ordinal})`); `prefs` sai de `(&Locale).into()`;
  `rules.category_for(&Decimal) -> PluralCategory`; `rules.categories()`; `PluralRulesWithRanges::try_new_cardinal(prefs)`
  e `category_for_range(&start, &end)` (tabela `StandardPluralRanges`, resolve o `selectRange`). Operandos: `From<&Decimal>`
  (`i` é `u64`, então número gigante vira `Decimal` por `Decimal::try_from_str`); o `c` (notação compacta) vem de
  `From<&CompactDecimal>`.
- **NumberFormat** (`icu_decimal`): `DecimalFormatter::try_new(prefs: DecimalFormatterPreferences, DecimalFormatterOptions
  { grouping_strategy })` com `GroupingStrategy::{Auto,Never,Always,Min2}` (mapeia `useGrouping` `auto`/`false`/`always`/
  `min2`); `.format(&Decimal)` devolve `FormattedDecimal` (`Writeable`, com partes em `parts.rs`: `integer`, `group`,
  `decimal`, `fraction`, `minus`, `plus`). Numeração (`-u-nu-`) vem da preferência do locale. Compacto curto:
  `CompactDecimalFormatter::try_new_short(prefs, options)` e `.format(&Decimal)`; o compacto longo e moeda/unidade/percent
  (padrão com afixos) NÃO estão em `icu_decimal`: precisam de `icu_experimental` (`dimension`: currency, percent, units).
  Arredondamento e dígitos significativos ficam em `fixed_decimal` (`round_with_mode`, `round_with_mode_and_increment`,
  `pad_end`), o que cobre `roundingIncrement`.
- **DateTimeFormat** (`icu_datetime`): `DateTimeFormatter::try_new(prefs: DateTimeFormatterPreferences, field_set)` com
  field sets em `fieldsets` (`YMD`, `YMDT`, `T`, `M`...), `.with_length(Length::Long|Medium|Short)` e `Calendar` pelo
  preferência do locale; `.format(&input)` com `input` por `icu_calendar::Date`/`icu_time::Time`/`ZonedDateTime`. Existe
  `fieldbag` (`builder.rs`, `dynamic.rs`): `DateTimeFormatter<CompositeFieldSet>` por `FieldSetBuilder` monta o conjunto a
  partir de opções (`year`, `month`, `weekday`, `hour`...), que é o análogo do `DateTimePatternGenerator`. A API cobre os
  campos do `Intl` mas não todas as combinações livres do ECMA-402: os campos sem equivalente caem no padrão mais próximo
  e precisam ser conferidos contra `tests/golden/intl_bun.tsv`. `formatToParts` usa `parts.rs` (`PartsWriteable`).
  `formatRange`: módulo `range` (`DateTimeInterval`). Os calendários vêm de `icu_calendar::cal::{Japanese, Buddhist, Hebrew,
  Hijri*, Persian, Roc, Coptic, Ethiopian, Indian, EastAsianTraditional, ...}`.
- **Collator** (`icu_collator`): `Collator::try_new(prefs: CollatorPreferences, options: CollatorOptions { strength,
  alternate_handling, max_variable, case_level })` devolve `CollatorBorrowed<'static>`; `.compare(&str, &str) -> Ordering`.
  O `-u-co-` e o `numeric`/`caseFirst` entram pelas preferências (`CollatorPreferences`). Usa `icu_normalizer` (já na árvore).
- **Locale** (`icu_locale`): `LocaleExpander::new_common()` (`maximize`, `minimize`, `get_likely_script`) no lugar da
  tabela de 46 línguas de `intl_locale_data.rs`. `names`: `LanguageDisplayNames`, `RegionDisplayNames`, `ScriptDisplayNames`,
  `VariantDisplayNames` (`try_new_*` por nível de dado) cobrem DisplayNames de `language`, `region`, `script`.
- **ListFormat**: `icu_list::ListFormatter::try_new_and(prefs, ListFormatterOptions::default().with_length(ListLength::
  Wide|Short|Narrow))` (`new_or`, `new_unit`), `.format(iter)` (API conhecida da documentação; conferir nas fontes quando a
  crate chegar).
- **RelativeTimeFormat**: `icu_experimental::relativetime::RelativeTimeFormatter::try_new_long_year/…` por unidade
  (conferir nas fontes quando chegar); o `numeric: "auto"` usa o campo de frases fixas do CLDR.
- **DisplayNames de moeda/calendário/dateTimeField**: `icu_experimental::displaynames` (a conferir).

## Ordem de migração (passos de ~5 minutos cada; um módulo do porte por passo, golden `intl_bun.tsv` a cada passo)

1. [FEITO] Camada nova isolada `src/runtime/icu_plural.rs` (`categories`, `select`, `select_range` sobre `icu_plurals`) +
   Cargo.toml (`icu_locale_core`, `icu_plurals`, `fixed_decimal`) + `pub mod icu_plural;` em `runtime/mod.rs`. Ainda não ligada
   a `intl_plural_rules.rs` (arquivo com outro agente).
2. [FEITO, sem compilar] Ligar `intl_plural_rules.rs`: `select`/`pluralCategories` chamam `icu_plural` (o operando vem dos dígitos já arredondados
   do `NumberFormat`); `selectRange` deixa de devolver `other`. Remover `cardinal_category`/`ordinal_category` e `Language`
   do plural. Conferir fr, ar, pl, ru, cy, pt-PT contra o bun.
3. [FEITO, sem compilar] `AVAILABLE_LOCALES` saiu de `intl_locale_data.rs`: a língua é suportada se consta nos subtags
   prováveis do `icu_locale::LocaleExpander` (nova dependência `icu_locale = "2"`), o `BestAvailableLocale` mantém a região
   pedida (`en-GB`, `pt-PT`, `fr`, `ja`...), e `data_fallback_chain` expõe o `LocaleFallbacker` (`en-GB`, `en-001`, `en`).
   Lacuna: sem lista de regiões por língua (`en-ZZ` não cai em `en`). `Language` (English/Portuguese) segue até o passo 4.
4. [FEITO, sem compilar] NumberFormat decimal e compacto: `src/runtime/icu_number.rs` (`DecimalFormat`, `CompactFormat`, resolução de
   `numberingSystem`) sobre `icu_decimal = { version = "2.3.0", features = ["unstable"] }` + `writeable = "0.6"`, ligado em
   `default_number_format.rs` (`NumberSettings` ganhou `locale` e `numbering_system`; `defaults(Language)` segue igual para os
   outros módulos) e em `intl_number_format.rs` (`numberingSystem`/`-u-nu-` honrados, `resolvedOptions().numberingSystem`).
   Vêm do CLDR: separadores, tamanho dos grupos (lakh/crore em `en-IN`, `hi`), `minimumGroupingDigits` (`es`, `pt-PT`), sinais com as
   marcas bidirecionais do `ar`, dígitos por sistema numérico, e o compacto curto e longo para qualquer locale (o `icu_decimal` 2.3
   TEM compacto, não só o curto: `try_new_short`/`try_new_long` com a feature `unstable`; o expoente vem de
   `compact_exponent_for_magnitude`, então `ja` compacta em 10^4/10^8 e `hi` em 10^5/10^7). O arredondamento continua o do JSC
   (`Digits`), e o significando arredondado entra por `format_with_exponent`. As tabelas escritas à mão de compacto (`compact_suffix`),
   `separators` e `groups` foram apagadas. Ficam à mão: o padrão de percentual por língua (`percent_pattern`: espaço em `fr`/`de`/`es`...,
   `%` na frente em `tr`, LRM em `ar`), moeda e unidade (só `en`/`pt`; dependem do `icu_experimental`).
   Lacunas fechadas (medidas no bun, fixadas em testes de `icu_number.rs` e `default_number_format.rs`): `NaN` por locale
   (`icu_number::nan_symbol`: ar, fa, ru, my, ka, hy, kk, ky, uz, am, yue, zh-Hant, fi, lv, lo; os demais `NaN`), `%` por sistema numérico
   (`percent_sign`: `٪` + ALM em `arab`, `٪` em `arabext`, e o `ar` com dígitos árabes perde as marcas LRM), e `useGrouping: "always"`
   (`DecimalFormat::number` força o grupo dos quatro dígitos em `es`/`pt-PT`/`fr`; `true` do JS já era `always`). Ainda abertas: `always`
   no compacto (age como `auto`), padrão de percentual com espaço do `ckb` e outros fora de `percent_pattern`, `NaN` com `style: percent`,
   moeda e unidade de `fr`/`de`/`ja`/`ar`/`hi` no padrão do inglês até o passo 5. Nada compilado nem rodado nesta fatia.
   Conferir ao compilar: `tests/golden/intl_bun.tsv` (NumberFormat em fr, de, ja, ar, hi, en-GB, pt-PT, `en`, `pt-BR`) e os testes de
   `icu_number.rs` e `default_number_format.rs`.
5. [FEITO por dados medidos no bun, sem compilar nem rodar] NumberFormat percentual, moeda e unidade sem esperar o `icu_experimental`:
   `scripts/gen-number-format-data.js` mede o bun e escreve `src/runtime/icu_number_data.rs` (padrões de moeda em symbol,
   narrowSymbol e code com positivo, negativo e contábil; nome por categoria de plural; percentual; unidades simples long, short e
   narrow por categoria) e `tests/golden/number_format_more_bun.tsv` (5880 casos), para es, fr, de, it, ja, ru, ar, hi, zh, ko
   (mais fr-CA, de-CH, es-MX, zh-TW) e USD, EUR, BRL, JPY, GBP, CNY, INR, RUB, KRW, MXN, CHF, CAD. `icu_number_patterns.rs` lê e
   renderiza os padrões (locale exato, depois a língua); `default_number_format.rs` ganhou `localized_parts` (moeda e unidade,
   retorno antecipado em `format_parts`) e `percent_parts` consulta a tabela antes de `percent_pattern`. `en` e `pt` seguem nas
   tabelas à mão. Teste: `tests/number_format_more_bun_golden.rs`. Lacunas: moeda fora das 12 usa o padrão em código do USD com o
   código trocado; unidade fora das 15 e unidade composta fora de `kilometer-per-hour` seguem no inglês; `currencySign: accounting`
   com `signDisplay: always` positivo e dígitos de outro sistema numérico usam o padrão do sistema latino; variantes regionais fora
   das quatro medidas caem na língua. Ao compilar, conferir `icu_number_patterns` (testes) e o golden novo.
6. DateTimeFormat gregoriano: `icu_datetime` com `FieldSetBuilder` a partir de `dateStyle`/`timeStyle` primeiro, depois dos
   campos soltos; padrões escritos à mão ficam como fallback até todos os casos do golden baterem.
7. DateTimeFormat calendários e fusos: `icu_calendar` + nomes de fuso de `icu_time`/`icu_datetime`; `formatRange` por `range`.
8. Collator: `icu_collator` em `intl_collator.rs` (ordem, `numeric`, `caseFirst`, `sensitivity`, `-u-co-`).
9. Locale: `LocaleExpander` em `maximize`/`minimize`; `DisplayNames` de língua/região/escrita por `icu_locale::names`.
10. ListFormat e RelativeTimeFormat (após baixar `icu_list`, `icu_experimental`); DisplayNames de moeda/calendário.
11. Apagar as tabelas escritas à mão que o golden dispensa e atualizar `wip-notes/intl-gaps.md`.

## Riscos a conferir no oráculo

- Versão de CLDR: icu4x 2.3 traz CLDR 48; o bun 1.4.2 usa o ICU do WebKit (CLDR ligeiramente anterior). Diferenças pontuais
  (nomes de fuso, `supportedValuesOf`) aparecem no golden e se resolvem por exceção documentada, não por tabela nova.
- Tamanho do binário release estático musl: medir depois do passo 4 (dados de decimal e datetime são os maiores).
- `icu_normalizer` 2.1.1 e 2.3.0 coexistem no registry: o lock precisa unificar em `icu_provider` 2.3.x.
