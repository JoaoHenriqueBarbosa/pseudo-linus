# Cobertura de skeletons nos calendários não gregorianos

## Problema

Em calendário não gregoriano, um skeleton de `Intl.DateTimeFormat` fora do conjunto medido em
`src/runtime/intl_calendar_patterns.rs` cai no padrão gregoriano e diverge do bun. O ICU resolve qualquer
skeleton pelo `DateTimePatternGenerator` (melhor `availableFormat` do calendário, `adjustFieldTypes`, junção
data+hora pelo `dateTimeFormat`).

## Medições (bun 1.4.2)

O espaço de campos de data é finito: weekday (4: nenhum, narrow, short, long) x era (4) x year (3: nenhum,
numeric, 2-digit) x month (6: nenhum, numeric, 2-digit, narrow, short, long) x day (3) menos o vazio =
**863 skeletons** por calendário e locale. Com 14 calendários e os 65 locales do porte: ~785 mil linhas
(medido: 753.588 depois da poda por locale pai, 36.280 padrões distintos).

| Configuração | Linhas | Fonte `.rs` |
|---|---|---|
| Conjunto medido atual, 65 locales | 324.326 | 29,7 MB |
| Exaustivo, 65 locales | 753.588 | 63,2 MB |
| Exaustivo, 6 locales (en,pt,de,ja,es,fr) | 90.636 | 7,5 MB |
| Conjunto medido, 6 locales (arquivo versionado) | ~38,5 mil | 3,5 MB |

O gerador leva ~105 s (a medição domina). As linhas com hora estão incluídas e não mudam entre as colunas.

## Opção (a): tabela exaustiva

Funciona e é fiel por construção (é a própria saída do ICU). O custo é o formato: a chave textual
`locale|calendário|skeleton` pesa ~84 bytes por linha no fonte. Com o formato atual, a opção (a) soma +33 MB
nos 65 locales, fora do "alguns MB". Com 6 locales são +4 MB, aceitável.

O problema é o formato, não a opção. Em vez de chave textual, uma tabela densa por (locale, calendário): 863
entradas `u16` (índice no pool de padrões, `0xFFFF` = sem padrão), o skeleton virando índice de base mista
(weekday, era, year, month, day). Estimativa: 785 mil x 2 B = 1,6 MB de binário, ~3,5 MB de fonte em literal
hex, mais o pool de 36 mil padrões (~2,5 MB). Total ~6 MB para a data exaustiva nos 65 locales. A poda por pai
fica desnecessária (o herdado vira dedupe de blocos idênticos).

Hora: as linhas com hora usam hoje um conjunto fixo (9 conjuntos de data x 21 de hora x 2 ciclos). Pela regra do
ICU o padrão de data+hora é o `dateTimeFormat` (estilo full/long/medium/short, escolhido pela largura do mês e
pelo weekday) aplicado ao padrão de data e ao de hora. Então a hora não precisa de tabela cruzada: medir os 4
`dateTimeFormat` por (locale, calendário) e compor data + cola + hora em runtime. Isso elimina as ~266 linhas
de hora por (locale, calendário) e cobre qualquer combinação de hora.

## Opção (b): portar o DateTimePatternGenerator

Esforço alto. O bun não expõe os `availableFormats`: só dá para inferá-los sondando skeletons canônicos
(`formatToParts` revela o padrão, mas não se o skeleton existe nos dados ou foi montado). Além disso, distância
de skeleton, `adjustFieldTypes` e os casos especiais (`y` vs `yy`, `MMMd` vs `MMMMd`, `E` dentro de data, `G`
posicional) precisam bater com o ICU campo a campo. Seria o caminho com mais código e mais testes, e ainda
seria validado contra a mesma tabela exaustiva que a opção (a) entrega pronta.

## Recomendação

1. **Opção (a) com formato denso** (tabela `u16` por locale e calendário, 863 entradas) mais composição de
   data+hora por `dateTimeFormat` medido. É a menor estratégia fiel: nenhuma heurística, todo skeleton válido
   tem linha, ~6 MB para tudo.
2. Não portar o gerador (b). Locales fora dos 65 já caem no padrão do `en`.
3. Se o formato denso for adiado: exaustivo só para os 6 locales de base (+4 MB), nunca para os 65 (+33 MB).

## O que foi feito

`scripts/gen-calendar-patterns.js` ganhou o modo exaustivo de data atrás de `GEN_CAL_EXHAUSTIVE=1`, desligado
por padrão porque no formato textual geraria 63 MB. `src/runtime/intl_calendar_patterns.rs` ficou como estava
(a regeneração de teste foi descartada). Próximo passo, no lado Rust (não feito, sem `cargo`): trocar `ROWS`
pelo formato denso em `intl_calendar_patterns.rs` e a `data_key` de `intl_date_time_format.rs` pelo índice de
base mista.
