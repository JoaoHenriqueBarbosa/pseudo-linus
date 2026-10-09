# Auditoria de Array contra o bun 1.4.2

## O que foi feito

- `scripts/gen-array-golden.js` mede no bun 1.4.2 (JavaScriptCore) programas de Array e iteração e grava
  `tests/golden/array_bun.tsv` (2518 programas mantidos, 14 descartados por estourar 4 s, todos com `length`
  gigante em laço denso, como `reduce` sobre `length: 2**53 - 1`).
- `tests/array_bun_golden.rs` roda cada linha com `evaluate_named_script_result(source, "array_case.js", "R")`, no
  mesmo molde de `tests/function_error_bun_golden.rs`. Não foi executado (a tarefa proibia cargo).
- Cobertura: `at`, `concat` com `Symbol.isConcatSpreadable`, `copyWithin`, `fill`, `flat`, `flatMap`, `includes`,
  `indexOf`, `lastIndexOf`, `join` com ciclos, `reverse`, `slice`, `sort` estável com comparadores inconsistentes,
  `splice`, `toSorted`, `toSpliced`, `toReversed`, `with`, `findLast`, `findLastIndex`, `entries`/`keys`/`values`,
  `Array.from`, `Array.of`, `Array.fromAsync`; arrays esparsos e com buracos; `length` 2**32-1 e 2**53-1 em
  array-like; subclasses com species; proxies com log de traps; getters que mutam durante a iteração;
  TypedArray versus Array (inclusive buffer redimensionável e transferido); mensagens exatas de erro.
- A parte combinatorial (arrays x índices) é amostrada a um em seis para o golden ficar perto de 2500 linhas. Cada
  linha carrega o prelúdio `S`/`T`/`D` (serializa buraco, `-0`, bigint; captura exceção), por isso o arquivo tem 3 MB.
- Programas não determinísticos (rodados duas vezes no bun) são descartados; nenhum caiu aqui.

## Auditoria de `src/runtime/array_prototype.rs`

Lidos contra `upstream/JavaScriptCore/runtime/ArrayPrototype.cpp` e `builtins/ArrayPrototype.js`: `push`,
`unshift`, `splice`, `with`, `toReversed`, `toSpliced`, `toSorted`, `copyWithin`. Comparados com medições do bun
para os casos de borda de comprimento:

- `with` com índice fora do intervalo e `length` 2**32: o bun dá `Array index out of range` antes do erro de
  comprimento; o Rust confere o índice primeiro. Igual.
- `with`/`toReversed`/`toSorted` com `length` 2**32-1: `RangeError: Out of memory`; o Rust usa
  `MAX_STORAGE_VECTOR_LENGTH` e `PutError::OutOfMemory`. Igual.
- `with`/`toSpliced`/`toSorted`/`toReversed` com `length` 2**32: `Array length must be a positive integer of safe
  magnitude.`. Igual.
- `toSpliced` com `length` 2**53-1: `TypeError: Array length exceeds 2**53 - 1` (o bun também lança quando o novo
  tamanho é exatamente 2**53-1, e o Rust usa `>=`). Igual.
- Mensagens de `push`, `unshift` e `splice` com 2**53-1: iguais às do bun.

Nenhuma divergência óbvia encontrada, então nenhuma edição foi feita em `src/runtime`. O que o golden vai
revelar só aparece quando alguém rodar `cargo test --test array_bun_golden`; as falhas listam o corpo do programa,
o esperado e o obtido.

## Pendências

- Rodar o teste e triar as divergências (fora do escopo desta tarefa por causa da regra de não rodar cargo).
- Os 14 programas descartados (length 2**53-1 em laço denso) merecem versões com `length` menor se algum bug
  de desempenho aparecer.

## tests/call_spread_varargs.rs: três falhas (leitura, sem rodar)

1. `spread_in_new_and_super`: causa achada. `Structure::is_valid_prototype` usava só `JSObject::from_value`, que não
   cobre `JSFunction`; `class B extends A` com `A` função dá `B.__proto__ = A` e o `debug_assert` de
   `change_prototype_transition` disparava (o C++ usa `prototype.isObject()`, que inclui função). Corrigido em
   `src/runtime/structure.rs`: `is_valid_prototype` aceita função com `may_be_prototype`, e a chave da transição
   `ChangePrototype` usa o `cell_id` da função em vez de `PointerKey::Null` (que misturava todas as funções).
2. `too_many_arguments_is_a_range_error`: sem causa achada pela leitura. Medido no bun: `length` 0x10001 passa,
   0x100001 lança `RangeError: Maximum call stack size exceeded.`; logo `Interpreter::MAX_ARGUMENTS = 0x100000`
   (execute_call.rs:42) está certo, e `size_of_varargs` já lança acima dele (Infinity e 2**32 viram `u32::MAX` por
   `to_length_clamped_to_unsigned`). Falta rodar para ver a falha real (mensagem, ou exceção que não chega ao catch).
3. `deep_recursion_through_varargs_is_a_range_error`: sem causa achada. `enter_frame` confere
   `MAX_NATIVE_DEPTH`, `is_safe_to_recurse` e `ensure_capacity_for`, e `size_frame_for_varargs` confere
   `ensure_capacity_for` antes de copiar. Hipóteses a medir: estouro da pilha nativa de 2 MiB no
   `call_varargs` (frame nativo maior que o das chamadas comuns, com margem de 64 KiB insuficiente) ou o custo
   quadrático do `r(...x, 1)` (cópia do array a cada nível).

## Golden de borda: array_edge_bun (600 programas)

`scripts/gen-array-edge-golden.js` gera `tests/golden/array_edge_bun.tsv` (rodado no bun 1.4.2) e
`tests/array_edge_bun_golden.rs` o confere (`array_edge_matches_bun`, ainda não executado: sem cargo nesta rodada).
Complementa `array_bun.tsv`: 37 programas idênticos aos do base foram descartados na geração, e dos 2740 candidatos
restantes entram 600 em passo fixo (4), para manter a proporção entre as seções.

Seções: `splice`/`toSpliced` e `copyWithin` em array-likes de `length` gigante (2**53-1, 2**53+10, 2**32, Infinity,
negativo, string); `flat`/`flatMap` (profundidades estranhas, proxies, revogado, ciclo, species); `sort` estável com
vinte comparadores inconsistentes (NaN, booleano, alternado, BigInt, lançando, mutando o array); `toSorted`/`toSpliced`/
`toReversed`/`with` (limite 2**32-1, RangeError, proxies com log de armadilhas); `find`/`findIndex`/`findLast`/
`findLastIndex`; `at`; `includes`/`indexOf`/`lastIndexOf` com NaN, -0 e `fromIndex` esquisito; species (28 variações de
`constructor`, em oito métodos); buracos com índices herdados de `Array.prototype`/`Object.prototype`;
`Symbol.isConcatSpreadable` (proxy, getter que lança, length gigante, protótipo); `Array.from`/`Array.of` com iterables
patológicos, `mapFn` e fechamento do iterador (`return` que lança ou devolve primitivo); iteradores e callbacks que
mutam o array. `Array.fromAsync` fica de fora (exige microtarefas).

Cuidado de geração: cada programa roda num processo do bun com limite de 4 s e é executado duas vezes (descarta o não
determinístico); length gigante só entra em forma que sai antes do laço (TypeError/RangeError) ou que toca poucos
índices, porque o spec varre `length` inteiro nos demais.
