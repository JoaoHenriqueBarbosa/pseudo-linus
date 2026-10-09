# Auditoria de TypedArray (2026-10-08)

Golden novo, medido no bun 1.4.2 (Float16Array existe):

- `scripts/gen-typedarray-golden.js` gera `tests/golden/typedarray_bun.tsv` (906 programas, 12 classes) e
  `tests/typedarray_bun_golden.rs` o confere. Complementa `typedarray_more_bun` (redimensionável, detach, DataView).
- Cobre: construtores (comprimento, array, iterável, buffer com offset/length, alinhamento), from/of, métodos
  (set, subarray, slice, fill, copyWithin, sort, indexOf/includes, join, toReversed/toSorted/with/at, findLast,
  map/filter com species, iteradores), conversões (clamped, overflow, BigInt vs Number), índices canônicos
  ('-0', '1.5', 'Infinity'), defineProperty/freeze/seal, Reflect.ownKeys/set/get, getters de protótipo.
- Não rodei cargo nem o teste, por instrução. Nenhum resultado do porte foi medido ainda.

## Comparação de mensagens de erro (leitura de `src/runtime`)

Cada mensagem de TypeError/RangeError do golden foi buscada no código: todas existem
(js_generic_typed_array_view, typed_array_constructors, typed_array_prototype_support, typed_array_prototype_natives,
js_value_conversions, js_big_int_ops). "length cannot be negative", "byteOffset cannot be negative" e "larger than
(2 ** 53) - 1" vêm de `js_value_conversions.rs` (formatadas com o nome). Nenhuma divergência óbvia de texto, então
nenhum Edit em `src`. Pendências a confirmar só rodando o teste: ordem de coerção, `Reflect.set` com receptor
diferente, `Object.freeze` em TypedArray não vazio.
