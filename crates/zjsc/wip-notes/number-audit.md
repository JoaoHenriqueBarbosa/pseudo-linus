# Auditoria de Number (formatação e leitura)

Estado em 2026-10-08.

## O que entrou

- `scripts/gen-number-format-golden.js`: mede no bun 1.4.2 programas de Number e Math fora do libm. Roda todos num
  único processo por eval indireto; exceção vira `Nome: mensagem` e `-0` vira `-0`.
- `tests/golden/number_format_bun.tsv`: 5473 programas (o pedido era cerca de 2500; as varreduras de dígitos 0..100,
  fronteiras do shortest repr e `toLocaleString` ficaram densas). Sem caminho da máquina nas linhas.
- `tests/number_format_bun_golden.rs`: mesmo padrão de `tests/function_error_bun_golden.rs`, arquivo
  `number_format_case.js`.

## Cobertura

- `toString(radix)` 2..36 (fracionários, expoentes grandes, radix inválido).
- `toFixed`/`toExponential`/`toPrecision` com todos os dígitos 0..100 em oito valores de arredondamento
  (1.005, 1/3, 5e-324, MAX_VALUE...) e argumentos inválidos.
- `Number()`, unário `+`, `parseFloat`, `parseInt` (radix 0..37): hex, octal, binário, separadores `_`, espaços
  unicode, expoentes enormes, strings de 400 dígitos, denormais, meio-termo de arredondamento (2**53 + 1 etc.).
- `String(number)` nas fronteiras de potências de 2 e de 10, com mais e menos 1 ulp.
- Predicados e constantes de Number, `BigInt(number)`, `BigInt.asIntN`/`asUintN`.
- Math: `fround`, `clz32`, `imul`, `trunc`, `sign`, `cbrt`, `expm1`, `log1p`, `hypot`, `round`, `max`/`min` com
  `-0` e NaN.
- `toLocaleString('en-US')` básico e com opções comuns.

## Sobreposição com goldens existentes

`math_bun.tsv` e `number_to_string.tsv` medem bits e shortest repr por valor; este golden mede por programa JS
(inclui erros e coerção), então os dois lados se complementam. `proxy_class_bun.tsv` e `function_error_bun.tsv`
não tocam Number.

## Leitura contra o upstream

`src/runtime/number_prototype.rs` (toExponential, toFixed, toPrecision, toString) foi lido contra
`upstream/JavaScriptCore/runtime/NumberPrototype.cpp`: mensagens de RangeError, limites 0..100 e 1..100, ordem de
`ToIntegerOrInfinity` antes do teste de finito e o corte `!(abs(x) < 1e21)` do toFixed batem. Nenhuma divergência
óbvia encontrada, então nada foi editado. `src/wtf/dtoa/*` e `number_constructor.rs` não foram lidos linha a linha
por falta de tempo.

## Pendente

Nenhum cargo foi rodado (regra da tarefa). O primeiro `cargo test --test number_format_bun_golden` deve mostrar as
divergências reais; elas são o próximo trabalho.
