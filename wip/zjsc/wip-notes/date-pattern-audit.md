# Auditoria dos padrões de data e hora por locale

Medido no bun 1.4.2 (JavaScriptCore + ICU) com `formatToParts`, em UTC.

## O que existe agora

- `scripts/gen-date-pattern-golden.js` gera `tests/golden/date_pattern_bun.tsv` (4408 linhas): 38 locales (en, pt-BR,
  es, fr, de, it, ja, zh, ar, ko, ru, nl; en-GB, en-AU, en-CA, en-IN, pt-PT, es-MX, es-AR, fr-CA, de-AT, de-CH, zh-TW, zh-HK;
  hi, th, tr, pl, sv, da, nb, fi, cs, el, he, id, vi, uk) x 58 conjuntos de opções (`dateStyle` x4, `timeStyle` x4, as 16 combinações,
  e 34 de componentes: ano, mês numérico, 2 dígitos, longo, curto, estreito, dia da semana, `hour12` true e false,
  fuso curto) x 2 instantes (manhã de 5 de março e tarde de 25 de novembro de 2024).
- `tests/date_pattern_bun_golden.rs` confere locale resolvido, `numberingSystem` e a lista inteira de partes na ordem,
  com os literais (diferente de `calendar_bun_golden.rs`, que só compara o conjunto de `tipo=valor`). Não foi executado
  (sem cargo, por regra desta tarefa): a primeira rodada vai dizer quantas linhas divergem.
- `scripts/gen-datetime-data.js` ganhou `nl` na lista `LOCALES`; regenerado, `src/runtime/intl_date_time_data.rs`
  só ganhou linhas (a estática `NL` e o braço `"nl" => &NL`), nenhuma linha antiga mudou. Os goldens
  `datetime_more_bun.tsv` e `datetime_gaps_bun.tsv` foram regenerados pelo mesmo script.

## Cobertura do porte

| Locale | Como o porte formata hoje |
|---|---|
| en | Código à mão em `intl_date_time_format.rs` (`Language::English`): nomes, ordem e juntor `" at "`/`", "`. |
| pt (pt-BR) | Código à mão (`Language::Portuguese`), juntor `" às "`. |
| es, fr, de, it, ja, ru, ar, zh, ko | Tabela gerada `intl_date_time_data.rs`: 349 padrões por locale, medidos por skeleton (`dateStyle`/`timeStyle` e componentes), com nomes de fuso e períodos do dia. |
| nl | Tabela gerada agora (349 padrões, 20 sem token), igual às demais. |

A tabela é chaveada pelo tag (`es`, `es-MX`, `zh-TW`...). A busca `locale_data_for` (gerada, em `intl_date_time_data.rs`) tenta
primeiro `língua-REGIÃO` (região = primeiro subtag de duas letras após um script opcional; `-u-` e `-x-` ignorados) e depois a
língua sozinha. Usada por `locale_data_parts`, pelo ciclo de horas em `intl_date_time_format.rs` e por `data_range`
(`range.rs`). `en-GB` etc. entram na tabela, `en`, `en-US` e `pt-BR` continuam no código à mão. `zh-Hant` sem região cai em `zh`.

Regeneração de 2026-10-08: o gerador `gen-datetime-data.js` foi rodado com Bash (o `bun`, como permitido) e reescreveu
`intl_date_time_data.rs` (31235 linhas, 36 locales), `datetime_more_bun.tsv` (13164 linhas) e `datetime_gaps_bun.tsv`;
`gen-date-pattern-golden.js` também. Nada foi compilado nem testado: a primeira rodada do cargo vai mostrar as divergências.
Medido: `fi` tem 56 skeletons sem token e `zh-TW` 30 (os demais, 20).

## O que ainda falta

1. **Variantes regionais (feito, ver acima; falta rodar o teste).** Uma só tabela por língua serve `es-MX`, `de-AT`, `fr-CA`, `pt-PT`, `en-GB`, `zh-TW` etc.,
   que no ICU têm padrões próprios (`en-GB` é dia-mês-ano e 24h; `fr-CA` usa `h` e `-`; `zh-TW`/`zh-Hant` usam outro
   padrão). O golden novo só mede `en` e `pt-BR`; falta medir as regionais e chavear a tabela por `lang-REGION`.
2. **`en` e `pt` não passam pela tabela.** A forma de verificar o código à mão é este golden; se divergir, a saída
   natural é gerar `EN` e `PT` pelo mesmo script (basta pôr os dois em `LOCALES`) e aposentar o código à mão.
3. **Gaps já conhecidos** nos locales da tabela: 20 skeletons sem token por locale (`datetime_gaps_bun.tsv`).
4. **Outras línguas** (hi, th, tr, pl, sv, da, nb, fi, cs, el, he, id, vi, uk já entraram; faltam fa, ms, ro, hu, bg...) caem hoje no formato inglês. Cada uma é uma
   palavra a mais em `LOCALES` do gerador (cresce ~870 linhas por locale na tabela).
5. **Calendários não gregorianos** continuam sem padrão por locale (ver `calendar_bun_golden.rs`): o golden de
   calendário só confere o conjunto de partes. O mesmo mecanismo de medição serve (opções `A` e `B` com `-u-ca-`).
6. Não verificado: se o locale `nl` chega a `locale_data_parts` (depende de a resolução de locale do `Intl.DateTimeFormat`
   aceitar `nl`; `Language::of_locale` devolve inglês para ele, o que não afeta o caminho da tabela).
