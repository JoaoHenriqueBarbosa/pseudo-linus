# Auditoria de Date em fusos (2026-10-08)

## Cobertura do golden antigo

`tests/golden/date_bun.tsv` (1302 linhas) cobre só dois fusos: America/Sao_Paulo e UTC (651 programas
em cada). Faltavam fusos com deslocamento fracionário (Asia/Kolkata +05:30, Pacific/Chatham +12:45/+13:45,
Asia/Tehran +03:30), horário de verão de 30 minutos (Australia/Lord_Howe), hemisfério norte com DST
(America/New_York, Europe/London) e transições de DST em hora ambígua ou inexistente.

## Golden novo

- Gerador: `scripts/gen-date-tz-golden.js` (bun 1.4.2, um subprocesso por fuso com `TZ` definida e
  timeout de 120 s; mesmo serializador `tests/golden/date_bun_harness.js`).
- Saída: `tests/golden/date_tz_bun.tsv`, 11336 linhas (1417 programas x 8 fusos: UTC, America/Sao_Paulo,
  America/New_York, Europe/London, Asia/Kolkata, Australia/Lord_Howe, Pacific/Chatham, Asia/Tehran).
  Sem caminhos da máquina.
- Cobertura: `Date.parse` de ~400 textos (ISO com e sem Z e offset, RFC 2822, `Mon Jan 01 2024`, am/pm,
  anos de 2 dígitos, mês por nome, EST/PST/GMT+0300, comentários entre parênteses, inválidos);
  construtor com componentes (inclusive 1883 a 2100 e transições de DST); `Date.UTC`; getters locais e
  UTC por instante; `getTimezoneOffset` em varredura de anos e de horas de transição; setters com
  overflow (local, UTC, sobre transição, sobre Date inválida); `toString`/`toDateString`/`toTimeString`/
  `toLocale*` (en-US, pt-BR)/`toISOString`/`toUTCString`/`toJSON`; ida e volta de parse.
- Teste: `tests/date_tz_bun_golden.rs` agrupa por fuso e usa `set_time_zone_spec_override` (por thread),
  um `VM` novo por programa. NÃO foi executado (regra desta tarefa: sem cargo).

## Pendências esperadas ao rodar

- Nomes longos de fuso em `toString` (`Lord Howe Standard Time`, `Chatham Standard Time`, `India Standard
  Time`, `Iran Standard Time`) dependem de `time_zone_names.rs`; o doc de `process_time_zone.rs` já avisa
  que o fuso fora da tabela sai como `GMT+05:30`. Falhas aqui são de nomes, não de aritmética.
- `toLocaleString` com `timeZoneName` depende do CLDR.

## Leitura de `parse_date` contra upstream/WTF/wtf/DateMath.cpp

Lidos lado a lado `parse_date`, `skip_spaces_and_comments`, `find_month`, `safe_string_to_integer`,
`parse_int`, `parse_long` e `KNOWN_ZONES`: sem divergência por leitura (mesma ordem de ramos, mesmos
limites, mesma regra de ano de 2 dígitos, mesmo padrão 2000 sem ano). Nenhuma edição feita.
`parse_es5_date` e `parse_es5_time_portion` não foram relidos nesta passada.

## Nomes longos de fuso (medição de 2026-10-08)

- Medição: `scripts/gen-timezone-names.js` (bun 1.4.2, um subprocesso por zona com `TZ` definida) amostra o
  `toString()` a cada trimestre de 1900 a 2100 em 544 zonas (`Intl.supportedValuesOf('timeZone')`, UTC e
  os apelidos da tabela antiga). O nome é o mesmo em todos os anos (o JSC usa o metafuso de hoje, não o
  da data); varia só padrão/verão. O nome de verão é o do maior deslocamento no mesmo ano.
- Saída: `src/runtime/time_zone_names_data.rs` (`ZONE_NAMES`, ordenada, busca binária; padrão vazio = o bun
  imprime `GMT+hh:mm`, ou seja `long_name` devolve `None`; verão vazio = nenhuma amostra o mostrou) e
  `tests/golden/timezone_names_bun.tsv` (3560 linhas, zonas canônicas, janeiro e julho de 1900, 1970,
  2024 e 2100). Zonas sem nome no ICU do bun: Africa/Casablanca, Africa/El_Aaiun, Asia/Amman, Asia/Damascus,
  Asia/Urumqi, Antarctica/Palmer, America/Punta_Arenas, America/Coyhaique, Etc/GMT+N e afins (211 entradas
  com algum campo vazio).
- Ligação: `long_name` consulta `ZONE_NAMES` primeiro; a tabela de metafusos antiga só serve a zona não
  medida e ao nome de verão de zona sem horário de verão.
- Teste: `tests/timezone_names_bun_golden.rs`. NÃO foi executado, nem cargo/rustc (regra da tarefa).
- Bash foi usado para rodar o bun do gerador (permitido); os arquivos de código saíram por Write/Edit,
  exceto os dois gerados pelo script (`time_zone_names_data.rs` e o `.tsv`) e este apêndice.
- Risco: o `.tsv` assume que o jiff devolve a mesma flag de horário de verão que o ICU nas zonas de
  horário de verão negativo (Europe/Dublin, Africa/Windhoek); o teste vai mostrar.

## Golden de parse de strings (apêndice)

- Gerador: `scripts/gen-date-parse-golden.js` (bun 1.4.2). Cada programa roda em UTC e em Asia/Kolkata:
  igual nos dois vira fuso `any` (3500 linhas, o teste roda em ambos), diferente vira uma linha por
  fuso (1342 programas dependentes). Saída: `tests/golden/date_parse_bun.tsv` (6184 linhas, 4842
  programas). Os programas já presentes em `date_tz_bun.tsv` podem reaparecer, mas a maioria é nova.
- Cobertura: ISO com e sem offset/zona (inclusive `+01:60`, `+24:00`, `T24:00:01`), anos expandidos e
  `-000000`, frações de 1 a 10 dígitos, legados (`Jan 1 2020`, `1 Jan 2020 10:00 GMT+0100`, RFC 2822,
  `2020/01/05`, `01/05/2020`, AM/PM, fusos nomeados, comentários entre parênteses), espaços Unicode e
  lixo, ano de 2 dígitos (corte 50), dias inválidos, `Date.UTC`, construtor com NaN e coerções,
  setters UTC com NaN e extremos, `toString`/`toUTCString`/`toISOString`/`toJSON`/`toLocale*` (en-US),
  `Symbol.toPrimitive`.
- Teste: `tests/date_parse_bun_golden.rs`. NÃO foi executado (sem cargo).
- Auditoria por leitura de `src/wtf/date_math.rs` (`parse_es5_date`, `parse_date`, `find_month`,
  `safe_string_to_integer`, `skip_spaces_and_comments`, `ymdhmsto_milliseconds`, tabela de fusos),
  `src/runtime/js_date_math.rs::parse_date` (normalização de espaços, cache, offset local) e
  `src/runtime/date_constructor.rs` (`milliseconds_from_components`, `construct_date`, `make_day`)
  contra `upstream/WTF/wtf/DateMath.cpp` e `upstream/JavaScriptCore/runtime/{JSDateMath,DateConstructor}.cpp`:
  nenhuma divergência encontrada, nenhum código alterado. `useV8DateParser` é false por padrão no upstream.
  Divergências, se houver, aparecerão só ao rodar o golden.

## Auditoria de Date.prototype (2026-10-08)

- `scripts/gen-date-proto-golden.js`: 2155 programas únicos, medidos no bun 1.4.2 em UTC e
  America/Sao_Paulo, 4310 linhas em `tests/golden/date_proto_bun.tsv`. Cobre os 14 setters com
  argumentos opcionais, NaN, Infinity, strings, objetos com `valueOf` que mudam a data, data inválida,
  limites de ±8.64e15, anos 0 a 99, `Date.UTC` com 0 e 1 argumento, `getTimezoneOffset` nas transições
  de Sao_Paulo 2018 e New_York 2024, formatadores em anos extremos, `Symbol.toPrimitive`, `Date()` sem
  `new`, subclasses e `Reflect.construct`.
- `tests/date_proto_bun_golden.rs`: padrão de `tests/date_tz_bun_golden.rs`, dois fusos. Não executado.
- Leitura: os setters de `src/runtime/date_prototype.rs` batem com `DatePrototype.cpp`; nenhuma
  divergência óbvia, nenhuma edição em `src/`.
- Pendente: `make_day`/`make_time` em `js_date_math.rs` contra `DateMath.cpp`.

## Auditoria do parser legado de Date (2026-10-08)

- `scripts/gen-date-legacy-parse-golden.js`: 2255 programas únicos medidos no bun 1.4.2 em UTC e
  America/Sao_Paulo (3055 linhas em `tests/golden/date_legacy_parse_bun.tsv`; `any` quando os dois fusos
  concordam). Cobre só formatos não ISO: RFC 1123, `Dec 25, 1995`, `12/25/1995`, `1995/12/25`, meses
  abreviados e completos (e truncados/com lixo), dia da semana errado, AM/PM, 24:00:00, fração de segundo,
  comentários entre parênteses, fusos (`UT`, `Z`, `EST`, `PDT`, `GMT+5`, `+0530`, `UTC-3`), ano de 2
  dígitos (pivô 49/50), anos negativos e estendidos, espaços e vírgulas extras, minúsculas, `T` fora do
  ISO, datas inválidas, ida e volta de toString/toUTCString e strings que o V8 aceita e o JSC não.
- `tests/date_legacy_parse_bun_golden.rs`: padrão de `tests/date_parse_bun_golden.rs`, com os fusos UTC e
  America/Sao_Paulo (transições de horário de verão de 1995/96 e 2018/19 entram nos casos). Não executado.
- Leitura: `parse_date` em `src/wtf/date_math.rs` confere com o upstream no pivô de ano (`< 50` soma 2000,
  senão 1900; o bun mede `Dec 25 49` como 2049 e `Dec 25 50` como 1950) e no AM/PM; nenhuma edição em `src/`.
  Divergências reais só aparecem ao rodar o golden.

## Golden de bordas de Date (`date_edge_bun`)

- `scripts/gen-date-edge-golden.js` gera `tests/golden/date_edge_bun.tsv` (784 programas, 1969 linhas: 547 `any` e
  237 dependentes de fuso, uma linha por fuso) no bun; `tests/date_edge_bun_golden.rs` roda nos seis fusos
  (UTC, America/Sao_Paulo, Europe/London, Asia/Kolkata, Australia/Lord_Howe, Pacific/Apia) pelo
  `set_time_zone_spec_override`. Não executado (sem cargo nesta tarefa).
- Cobre setters encadeados com overflow, `getTimezoneOffset` e hora local em transições, `toLocale*String` com
  opções em en-US, pt-BR, de-DE, ja-JP e ar-EG, `Symbol.toPrimitive`, `Date.UTC` com anos 0..99, extremos
  (±8.64e15), `toISOString`/`toJSON` de inválida, `valueOf` de objetos e comparação.
- Risco conhecido: os `toLocale*` dependem do ICU do bun; divergências ali podem ser de dados de locale (ICU4X),
  não de lógica de Date. Triar essas linhas separadas das demais ao rodar.
