# Fluxo do locale no Intl (medido em 2026-10-08)

## Veredito sobre o relato conflitante

Os dois agentes estavam parcialmente certos, em camadas diferentes.

- Resolução (nomes, `available`, `supportedLocalesOf`): `resolve_locale` e `supported_locales` em
  `src/runtime/intl_locale_data.rs` usam o `LocaleExpander` do icu4x (`language_has_data`). Toda língua do CLDR
  é aceita e o `BestAvailableLocale` devolve a tag pedida (base name, sem `-u-`): `sv`, `ar-EG`, `pt-PT`,
  `zh-Hans-CN`, `es-419`. Língua desconhecida (`xx`, `tlh`) e `und` caem em `en-US`. `iw` vira `he` na
  canonicalização. O teste `resolves_the_requested_tag_for_cldr_languages_as_bun` é verdadeiro.
- Dados de formatação: `Language::{English,Portuguese}` é escolhido só em `Language::of_locale` (`pt` ou
  `pt-*` é Portuguese, todo o resto é English), chamado por `resolve_locale` e no `Collator`. Então o locale
  devolvido é a tag pedida, mas os textos (meses, unidades, nomes) ainda saem de tabelas `en` e `pt`, exceto onde a
  classe já consome icu4x pela tag (PluralRules, ListFormat, DisplayNames com `table_for_locale`, RelativeTimeFormat
  via `data_language`).

## Por classe: locale dos dados e locale em `resolvedOptions().locale`

- NumberFormat: `resolve_locale_from(["nu"])`. `resolvedOptions().locale` é `resolved.tag_with(honored)` (tag pedida mais
  `-u-nu-` honrado). Dados: `NumberSettings::defaults(resolved.language)` com `settings.locale = base_locale` (a tag).
- DateTimeFormat: `resolve_locale_from(["ca","hc","nu"])`. Devolve `tag_with(honored)`; só `ca` gregoriano/iso8601,
  `nu-latn` e `hc` entram. Dados de nomes de mês e dia: `Language` (enum).
- PluralRules: `resolve_locale_from([])`. Locale devolvido e dados vêm da tag (`icu_plural::select(&state.locale...)`);
  `Language` só alimenta `NumberSettings`.
- ListFormat: `resolve_locale_from([])`. Devolve `resolved.locale`; os dados usam `format_locale` (a primeira tag
  pedida, não a resolvida).
- RelativeTimeFormat: `resolve_locale_from(["nu"])`. Devolve `tag_with(honored)`; plural pela tag, textos por
  `data_language(locale)` (`en`/`pt`) e `NumberSettings` com `state.language`.
- DisplayNames: `resolve_locale_from([])`. Devolve `resolved.locale`; nomes por `table_for_locale(&resolved.locale)` quando há
  tabela gerada, senão `Language`.
- Collator: `resolve_locale` e depois sobrescreve `resolved.locale` com `collator_locale` (conjunto próprio do colador do
  bun: `en-GB` vira `en`, `ar-EG` vira `ar`, `de-AT` e `zh-Hans-CN` ficam; sem cobertura, `en-US`) e `language =
  Language::of_locale`. Tailoring escolhido pelo idioma do locale resolvido.
- Segmenter: `resolve_locale_from([])`, devolve `resolved.locale`. Regras do UAX 29 não dependem de locale.
- DurationFormat: `resolve_locale_from(["nu"])`. Devolve `resolved.locale` (ou `tag_with(nu-latn)`); textos pela enum
  `Language` (`UNITS_PT`/`UNITS_EN`).

## Divergência encontrada (única)

Medido no bun com 20 tags x 9 classes (`tests/golden/resolved_locale_bun.tsv`, 180 linhas): só uma célula difere do
que a leitura do código prevê. `new Intl.DateTimeFormat("ja-JP-u-ca-japanese").resolvedOptions().locale` no bun é
`ja-JP-u-ca-japanese` (calendário japonês suportado); aqui sai `ja-JP`. Não foi corrigida: depende de suportar o
calendário japonês no DateTimeFormat (resolvedOptions().calendar e formatação), e só mexer na string mentiria.
Todas as outras 8 classes devolvem `ja-JP` nessa tag, como no bun. As demais 19 tags batem por leitura de código,
inclusive as peculiaridades do bun no Collator (`ar-EG`, `es-419`, `pt-PT`, `en-GB`, `ja-JP` viram a língua).

O teste `tests/intl_resolved_locale_bun_golden.rs` ainda não foi executado (cargo proibido nesta tarefa).

## Golden ampliado: locale_more_bun.tsv (2026-10-08)

`scripts/gen-locale-more-golden.js` mede no bun 1.4.2 1500 programas (maximize/minimize de 200 tags, 12 propriedades
e 7 getters de método mais as versões antigas `calendars`/`collations`/... em 120 locales, opções do construtor
sobrepondo extensões, erros de RangeError, `Intl.getCanonicalLocales` com 150 tags, `Intl.supportedValuesOf` com
todas as chaves). Saída: `tests/golden/locale_more_bun.tsv` (expressão, tabulação, resultado ou `ERR Nome: mensagem`);
teste em `tests/locale_more_bun_golden.rs`, ainda não executado (cargo proibido nesta tarefa).

Divergências corrigidas por leitura (`src/runtime/intl_locale_data.rs`): o bun 1.4.2 NÃO troca `tl`, `cmn`, `arb`, `swc`
(ficam como vieram) nem a região `UK` (`en-UK` fica `en-UK`); a tabela de aliases trocava as quatro línguas e `UK` por `GB`.

Divergências do golden fechadas em 2026-10-08 (sem cargo; nada foi compilado nem executado):
- Tabelas geradas por `scripts/gen-locale-aliases.js` (roda no bun 1.4.2, 26 s) em
  `src/runtime/intl_locale_aliases_data.rs`: 263 aliases de língua (todas as de 2 e 3 letras medidas, ex. `aam` vira `aas`;
  `tl`, `cmn`, `arb`, `swc`, `sh`, `no` não trocam), 6 de região, 20 de valor `-u-` (`ca-islamicc`, `ca-ethiopic-amete-alem`,
  `ms-imperial`, `ks-primary`/`tertiary` para `level1`/`level3`, `kb|kc|kh|kk|kn` com `yes`/`true` descartam o valor), 36 de
  `tz` (`aqams` vira `aqmcm`, `cnckg` vira `cnsha`, `est` vira `papty`...; espaço de 3 a 5 letras e país+3 varrido inteiro).
  `canonical_unicode_extension` aplica o alias depois de deduplicar chaves (a primeira vale, como no bun).
  Fora da tabela por não haver como varrer: `sd`/`rg` (subdivisões), `-t-` (o bun só põe em minúsculas, medido).
- Variante `posix` sozinha migra para `-u-va-posix` (com outra variante fica, `en-posix-1996` vira `en-1996-posix`; com `-u-`
  existente a chave `va` entra no fim). Medido: `en-US-posix-u-ca-gregory` dá `en-US-u-ca-gregory-va-posix`.
- Grandfathered: `art-lojban` dá `jbo`, `zh-guoyu` dá `cmn`, `zh-hakka` dá `hak`, `zh-xiang` dá `hsn` (com extensões:
  `zh-guoyu-u-ca-roc` dá `cmn-u-ca-roc`); `cel-gaulish` fica. `i-*`, `no-bok`, `no-nyn`, `sgn-BE-FR`, `en-GB-oed`,
  `zh-min(-nan)` e `zh-guoyu-CN` já eram rejeitados pela análise, igual ao bun. Teste novo em `intl_locale_data.rs`
  (`canonicalizes_values_variants_and_grandfathered_tags_as_bun`), não executado.
- maximize/minimize: o `maximize` só usava tabelas locais (76 línguas); 22 línguas do golden (ug, ks, ky, tg, sd, ff, ha, yo,
  ig, zu, xh, ti, so, om, dz, lo, bo, yue, nn...) ficavam sem escrita e região. Agora o que não está na tabela passa pelo
  `icu_locale::LocaleExpander::new_extended()` (`expander_maximize`), com `sh`/`no` intactos e `tl` e `cmn` trocados para
  `fil` e `zh-Hans`, como o bun mede. A conferência dos 150 tags contra o golden foi só por leitura da cobertura (script
  Python contou as línguas fora da tabela); a igualdade tag a tag fica para a primeira execução do teste golden.
  `minimize` herda a correção (usa `maximize`).
- Mensagem do RangeError (`invalid language tag: <tag>` no `getCanonicalLocales`, sem a tag no `new Intl.Locale`) segue a conferir
  em `intl_locale.rs` na primeira execução.

Ainda abertas: aliases de região do CLDR além dos seis ficam como vieram no bun (`YU`, `CS`, `SU`, `NT`, `AN`), já correto.
