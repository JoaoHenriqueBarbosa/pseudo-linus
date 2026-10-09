# Auditoria do golden de builtins

Golden novo, medido no bun 1.4.2.

- `scripts/gen-builtins-golden.js`: gera `tests/golden/builtins_bun.tsv` (4828 programas, 587 que terminam em erro).
  Rodar com `timeout 120 bun scripts/gen-builtins-golden.js > tests/golden/builtins_bun.tsv`. `TRACE=1` imprime cada
  programa antes de rodar, para achar o que trava.
- `tests/golden/builtins_bun_harness.js`: o harness próprio. Serializa o resultado com os tipos preservados
  (`num:`, `str:` em JSON, `big:`, `sym:`, `fn:nome/length`, `arr(n)[...]` com `<k holes>`, `map{}`, `set{}`,
  `date:`, `re:`, `err:`, objetos com chaves próprias), ou `error<TAB>name<TAB>message JSON`, ou `thrown`.
- `tests/builtins_bun_golden.rs`: no padrão de `errors_bun_golden.rs`, cada programa num `evaluate_script` próprio.
  Não foi executado (regra da tarefa: sem cargo).

Cobertura: Array.prototype inteiro (sobre arrays comuns, esparsos, array-likes, species, length enorme),
Object.groupBy e Map.groupBy, String.prototype inteiro (surrogates, normalize, split com regex, matchAll),
Map/Set (métodos de conjunto, getOrInsert), WeakMap/WeakSet/WeakRef/FinalizationRegistry, Number, Math (incl.
sumPrecise, f16round), BigInt, JSON (incl. rawJSON e source do reviver), URI, escape/unescape, atob/btoa e
structuredClone.

Fatos medidos no bun 1.4.2 que o porte precisa reproduzir: `Map.prototype.getOrInsert` e `getOrInsertComputed`
existem; `Math.sumPrecise`, `Math.f16round`, `JSON.rawJSON`, `JSON.isRawJSON` e `structuredClone` existem;
`Set.groupBy` não existe; `Math.sumPrecise(5)` lança TypeError "Type error".

## Comparação por leitura (parcial, limite de tempo)

Conferidos sem divergência: mensagens de `push`/`unshift`/`splice` com length acima de 2**53-1, RangeError
"Array length must be a positive integer of safe magnitude." (`array_prototype.rs`), mensagens do
`Math.sumPrecise` (`math_object.rs`, o "Type error" vem de `iterator_operations.rs`), presença de
`getOrInsert` em `map_prototype.rs`.

Nenhuma correção feita em `src/runtime` nesta passada. Próximo passo: rodar o teste e triar as divergências
pelo agrupamento por prefixo de fonte (Array., String., Map., Set., Number., Math., BigInt., JSON., globais).
