# Auditoria de coerção e operadores

Golden denso medido no bun 1.4.2, para comparar com o `zjsc` quando o `cargo test` rodar.

## Artefatos

- `scripts/gen-coercion-golden.js`: gera `tests/golden/coercion_bun.tsv` (11610 linhas) e `tests/golden/coercion_prelude.js`.
  Uso: `timeout 120 bun scripts/gen-coercion-golden.js > tests/golden/coercion_bun.tsv`.
- `tests/coercion_bun_golden.rs`: teste `coercion_and_operators_match_bun`, no padrão de `function_error_bun_golden.rs`.
  Põe o prelúdio na frente de cada expressão e lê a global `R`.

## Cobertura

60 valores (`mk(i)` devolve um valor novo a cada chamada): primitivos, bordas numéricas (`-0`, `NaN`, `2**31`,
`2**53`, `1e21`, `1e-7`), strings de conversão (`''`, `' '`, `'0x10'`, `'0b11'`, `'  42  '`), arrays, objetos com
`valueOf`/`toString`/`Symbol.toPrimitive` (um por hint, um que devolve o hint, um que devolve objeto, um cujo
`valueOf` lança), `Symbol()`, `BigInt` (`1n`, `-1n`, `0n`, `2n**64n`), função, `Date`, wrappers, `Proxy`, objeto de
protótipo nulo.

Amostragem determinística (LCG, semente fixa) dos pares 60x60 por operador:

| Grupo | Operadores | Pares por operador |
|---|---|---|
| aritméticos | `+ - * / % **` | 700 |
| relacionais | `< > <= >=` | 500 |
| igualdade | `== != === !==` | 400 |
| bit a bit | `& \| ^ << >> >>>` | 350 |
| `in`, `instanceof` | | 300 |
| lógicos | `?? && \|\|` | 150 |

Unários, todos os 60 valores: `+ - ~ ! typeof void delete(.x)` e `x++ x-- ++x --x` (resultado e valor final de `x`).

Resultado serializado por `ser`: `number -0`, `number NaN`, `bigint 1`, `string "..."`, `object [object Date]`, ou
`error Nome: mensagem`. 2007 linhas são erro (mensagens do bun, por exemplo `No default value`,
`Cannot convert a symbol to a number`, `Invalid mix of BigInt and other type in addition.`).

## Leitura do código contra o upstream

Relido sem divergência óbvia: `to_number`, `to_numeric`, `to_primitive`, `to_boolean`, `to_int32`, `to_string`
(`js_value_conversions.rs`); `js_add`, `js_add_slow_case`, `js_add_non_number`, `arithmetic_binary_op`, `js_less`,
`js_less_eq`, `to_primitive_numeric`, `equal_slow_case` (`operations.rs`). As mensagens de `Symbol` e `BigInt` batem
com o golden. O comentário de `equal_slow_case` ("igualdade com `JSBigInt` ainda não está ligada") está
desatualizado: o corpo já chama `big_int_equals_string` e `big_int_equals_big_int_or_number`.

## Pendente

O teste não foi rodado (sem cargo nesta tarefa). Falhas esperadas, a investigar quando rodar: operadores `in` e
`instanceof` (mensagens de erro), `typeof` de `Proxy` de função, `delete` em primitivos, e a ordem de avaliação de
`Symbol.toPrimitive` nos relacionais com `LEFT_FIRST` falso (`>` e `>=`).
