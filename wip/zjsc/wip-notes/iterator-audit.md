# Auditoria de iteradores contra o bun 1.4.2

## Entregue

- `scripts/gen-iterator-golden.js`: gera `tests/golden/iterator_bun.tsv` (3139 programas, 5 descartados porque o bun
  não devolveu `R`). Mais que os ~1500 pedidos, porque a matriz método x argumento x fonte x modo cresce rápido; se o
  teste ficar lento, reduzir `lazy`/`sources` no script.
- `tests/iterator_bun_golden.rs`: no padrão de `function_error_bun_golden.rs` (teste `iterators_match_bun`).
- Não foi rodado cargo nem o teste (regra da tarefa). Falhas esperadas: o motor ainda está em construção.

## Cobertura

Helpers (map, filter, take, drop, flatMap, reduce, toArray, forEach, some, every, find) contra fontes com e sem
`return`, vazia, de array, gerador, `next` que lança e `return` que lança; modos de consumo completo, `next`+`return`
e só criação (validação eager de argumento). `this` inválido, leitura de `next` uma vez, getters de `done`/`value`.
`Iterator`, `Iterator.from`, `Iterator.concat`. Encadeamento e reentrância. Geradores (12 corpos x 29 operações,
`yield*` contra 22 delegados). Destructuring, spread e `Array.from` com fechamento. Embutidos, protótipos de
iterador, patch de `Array.prototype[Symbol.iterator]`. Set methods com ~45 set-likes inválidos e ordem de chamadas.

## Fatos medidos no bun (para o motor)

- `Iterator.prototype` tem `includes`, `join`, `chunks`, `windows`, `[Symbol.dispose]`; `Iterator.zip`/`zipKeyed`
  existem, `Iterator.range` e `Iterator.prototype.flat` não. O Rust já liga `use_iterator_chunking/includes/join/
  sequencing/explicit_resource_management` por padrão em `options_list.rs`; `use_joint_iteration` (zip) não foi
  conferido no `iterator_constructor.rs`.

## Auditoria do código

Leitura parcial: `iterator_helper_prototype.rs` (next/return como builtins JS, `@@toStringTag`) e a ordem de
`finish_creation` em `iterator_prototype.rs` (toArray/forEach nativos, depois some/every/find/reduce/map/filter/take/
drop/flatMap builtins, chunks/windows, includes, join, dispose) conferem com o C++ de `JSIteratorPrototype.cpp`.
Os fontes JS em `upstream/JavaScriptCore/builtins/` (`IteratorHelpers.js`, `JSIteratorPrototype.js`,
`SetPrototype.js`) não foram comparados linha a linha com `iterator_operations.rs` por falta de tempo: nenhuma
divergência óbvia foi achada, nada foi editado em `src/`. Próximo passo: rodar `iterator_bun_golden` e triar as
falhas por família.
