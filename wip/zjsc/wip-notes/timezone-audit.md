# Auditoria de fusos (2026-10-08)

Golden novo: `scripts/gen-timezone-golden.js` gera `tests/golden/timezone_bun.tsv` (1107 linhas: 10 fusos de borda
em Intl.DateTimeFormat, seis estilos de timeZoneName em dez locales, aliases em resolvedOptions, Temporal
ZonedDateTime com lacuna e repetição nas quatro disambiguation, fusos de 1900 e 1970, offset ao segundo,
getTimeZoneTransition, Date local em UTC e America/Sao_Paulo). Teste: `tests/timezone_bun_golden.rs` (NÃO rodado).

Já existiam e não foram repetidos: `date_tz_bun`, `temporal_zoned_bun`, `timezone_names_bun`.

Revisão do código (os arquivos `intl_time_zone*.rs` não existem; a lógica está em `temporal_time_zone.rs` e
`intl_date_time_format.rs::resolve_time_zone`):

- Corrigido: o fallback de aliases de UTC devolvia `UTC` no resolvedOptions; o bun devolve o alias na caixa
  canônica (`GMT`, `Etc/UTC`, `Zulu`...). Só vale se o jiff não resolver o alias antes.
- Medido no bun, a conferir quando rodar o teste: `Z` e `Etc/GMT+15` lançam RangeError; `-0330` vira `-03:30`;
  `utc` vira `UTC`; `america/sao_paulo` vira `America/Sao_Paulo`; `Asia/Katmandu`, `Europe/Kiev`, `Asia/Saigon`
  voltam como pedidos.
- Risco: `temporal_time_zone.rs::BACKWARD_LINKS` tem só 44 ligações; `timeZoneEquals` entre aliases fora da lista
  falha. Nenhuma divergência medida ainda, só rodar o teste dirá.
