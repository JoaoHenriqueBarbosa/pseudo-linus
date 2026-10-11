# Auditoria de JSON e conversões numéricas (2026-10-08)

Golden: `tests/golden/json_number_bun.tsv` (3940 programas, medidos no bun 1.4.2), gerado por
`scripts/gen-json-number-golden.js`, com o harness `tests/golden/json_number_bun_harness.js` e o teste
`tests/json_number_bun_golden.rs`. O teste não foi rodado (regra da tarefa: sem cargo).

Cobertura: JSON.parse (erros por posição e por token, prefixos e trocas de caractere em 3 documentos, reviver com
`context.source`, `__proto__`, números enormes, surrogates, escapes, profundidade 100 a 1000000), JSON.rawJSON e
isRawJSON, JSON.stringify (replacer função e array, space com clamp, toJSON, BigInt, ciclos, exóticos, boxed, Map/Set,
Date, surrogates soltos, profundidade), `toString(radix)` com 300 valores, `toFixed`/`toPrecision`/`toExponential`
(330 combinações com RangeError e conversão do argumento), parseFloat/Number/unário (300 strings, 900 programas),
BigInt(string), asIntN/asUintN, `String(num)` (200 doubles, positivo e negativo).

## Fatos medidos no bun 1.4.2 que o zjsc precisa reproduzir

- Ciclo: a mensagem é só `JSON.stringify cannot serialize cyclic structures.` (sem o texto do ciclo). O fonte já bate.
- JSON.parse é iterativo: 1000000 de aninhamento de `[` / `{"a":` tem sucesso. `JSON.stringify` aninhado até
  20000 funciona; 50000 ou mais dá `RangeError: Maximum call stack size exceeded.` (o fonte usa
  `MAXIMUM_SIDE_STACK_RECURSION = 40000`, coerente com o intervalo medido).
- `BigInt("1_000")`, `"1n"`, `"1.5"`: `SyntaxError: Failed to parse String to BigInt` (o fonte tem a mesma string).
- `toFixed/toExponential`: `... argument must be between 0 and 100`; `toPrecision`: `between 1 and 100`;
  `thisNumberValue called on incompatible <tipo>` (o fonte tem as quatro).
- `rawJSON("1n")` e `rawJSON("01")`: `JSON Parse error: Unexpected content at end of JSON literal`.

## Leitura do fonte

Li `literal_parser.rs` (mensagens e `get_error_message`), `json_object.rs` (ciclo e limites de pilha) e as mensagens de
`number_prototype.rs` contra a tabela de mensagens do golden: as 30 mensagens distintas de `JSON Parse error`
observadas no bun têm correspondente no `literal_parser.rs`, e nenhuma divergência óbvia apareceu por leitura. Nenhum
Edit foi feito em `src/`. Não li `src/wtf/dtoa` a fundo no tempo disponível: a divergência real, se existir, aparece
na primeira rodada do teste novo, e a lista de falhas aponta o programa exato.

## Segunda leitura: radix, toFixed/toPrecision/toExponential e string para número (2026-10-08)

Lidos contra o C++: `src/runtime/number_prototype.rs` (`to_string_with_radix_internal`, `to_string_with_radix`,
`extract_to_string_radix_argument`, `this_number_value`, os três formatadores), `src/wtf/dtoa/mod.rs`
(`number_to_fixed_width_string`, `number_to_fixed_precision_string`), `js_value_conversions.rs` (`to_double`,
`js_to_number`) e `js_global_object_functions.rs` (`parse_float`, `js_to_number`, `to_double`), `parse_double`.

Medido no bun 1.4.2 (valores de referência para o golden): `(0.1).toString(2)` =
`0.0001100110011001100110011001100110011001100110011001101`; `(2**53).toString(36)` = `2gosa7pa2gw`;
`(0.5).toString(36)` = `0.i`; `(-255.5).toString(16)` = `-ff.8`; `(1e21).toFixed(20)` = `1e+21`;
`(0.5).toFixed(20)` = `0.50000000000000000000`; `(123.456).toPrecision(100)` mostra os 100 dígitos exatos
(`123.4560000000000030695446184836328029632568359375000000...`); `(1.255).toFixed(2)` = `1.25`;
`(-0.0000001).toFixed(2)` = `-0.00`; `(1.45).toFixed(1)` = `1.4`; `(0).toExponential(5)` = `0.00000e+0`;
`(255).toPrecision(2)` = `2.6e+2`; `parseFloat("1e-400")` = 0; `Number("0b11")` = 3; `Number("-0x10")`,
`Number("0x")`, `Number("1_0")`, `Number("1e")`, `Number(".")`, `Number("infinity")` = NaN; `Number("5.")` = 5;
`parseFloat("0x10")` = 0; `parseFloat("1e1000")` = Infinity; `Number(" 12﻿")` = 12.

Resultado por leitura: nenhuma divergência. `to_string_with_radix_internal` segue o C++ passo a passo (sinal,
caminho inteiro abaixo de 2^51, `Uint16WithFraction`, deltas para o vizinho, arredondamento par, subida do `9` para `a`,
BigInteger para a parte inteira); `-0` cai no ramo int32 e dá `0`; radix 10 e não finito usam o formatador decimal.
`toFixed` devolve `ToString` para `|x| >= 1e21` e NaN; `toPrecision` sem argumento é `ToString`; a ordem
(ToInteger antes do teste de finito, RangeError depois) bate com o bun. `to_double`/`js_to_number`/`parse_float`
reproduzem os atalhos de 1 e 2 caracteres e os prefixos `0x/0o/0b` exigindo um dígito válido na posição 2.

Dívida sem efeito observável: `js_to_number`/`to_double`/`parse_float` existem duas vezes, em `js_value_conversions.rs`
(genérico em `CharType`) e em `js_global_object_functions.rs` (`&[u16]`). Viola o DRY do projeto; a cópia de
`js_global_object_functions.rs` deveria virar chamada à de `js_value_conversions.rs`. Não mexi (outros agentes editam
`src/runtime`, e a unificação exige compilar).

Não lido a fundo: `fast_dtoa`, `bignum_dtoa`, `fixed_dtoa` e `strtod` (os números acima são o golden para eles).

## Próximo passo

Rodar `cargo test --test json_number_bun_golden` e triar as falhas por família (prefixo do programa).
