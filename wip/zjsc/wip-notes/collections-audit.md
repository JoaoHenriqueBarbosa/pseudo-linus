# Auditoria de coleções (Map, Set, Weak*)

Estado em 2026-10-08.

- O golden já existia: `wip/zjsc/scripts/gen-collections-golden.js`, `tests/golden/collections_bun.tsv` e
  `tests/collections_bun_golden.rs`. Cobrem Map/Set (ordem com mutação durante iteração, SameValueZero, iteradores,
  adder customizado, species, `Symbol.iterator` reatribuído), os sete métodos de conjunto novos contra set-likes,
  `Map.groupBy`/`Object.groupBy`, WeakMap, WeakSet, WeakRef e FinalizationRegistry (só API).
- Faltava `getOrInsert` e `getOrInsertComputed`, que o bun 1.4.2 tem em Map e WeakMap (não em Set nem WeakSet).
  Entraram 40 programas no gerador; o golden passou de 2232 para 2272 linhas (9 programas continuam descartados por
  falta de resultado no bun, os mesmos de antes).
- Comparação com a implementação: `src/runtime/map_prototype.rs` e `weak_map_prototype.rs` já registram os dois
  métodos, com a ordem de conferência (receptor, chave, `callback`) do C++. Nenhuma divergência óbvia encontrada
  por leitura; a conferência real depende de rodar `tests/collections_bun_golden.rs` (não rodado nesta passada).
