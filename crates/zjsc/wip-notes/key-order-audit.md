# Auditoria da ordem das chaves próprias (Reflect.ownKeys) contra o bun 1.4.2

## O que foi gerado

- `scripts/gen-key-order-golden.js`: mede no bun a ordem de `Reflect.ownKeys` (símbolo pela descrição, `@@iterator`),
  as flags writable/enumerable/configurable, o tipo (get/set/valor) e `length`/`name` das funções, para os globais do
  ECMAScript e do JSC, os `.prototype` deles, todas as classes de `Intl` e `Temporal`, `%TypedArray%`, os protótipos de
  gerador e função assíncrona e `globalThis` (filtrado pela lista de nomes do ECMAScript, sem `fetch`, `Bun`,
  `process`, `console` e afins).
- `tests/golden/key_order_serializer.js`: o serializador, o mesmo texto no bun e no porte.
- `tests/golden/key_order_bun.tsv`: 139 objetos, uma linha cada.
- `tests/key_order_bun_golden.rs`: um programa por objeto em realm novo (`evaluate_script`), reporta a primeira chave
  que diverge por objeto.

## Leitura do porte (sem rodar nada, regra da tarefa)

- `Set.prototype`: `finish_creation` instala os sete métodos novos depois de `@@toStringTag`, mas `constructor` só entra
  depois, em `js_global_object_init.rs` (o `put_direct` do construtor), e `own_property_names.rs` coloca todos os
  símbolos no fim. A ordem de strings resultante é `add clear delete entries forEach has keys size values union ...
  isDisjointFrom constructor`, igual à do bun. Nenhuma edição necessária; o teste confirma ao rodar.
- `Iterator.prototype`: `constructor`, `toArray`, `forEach`, `some`, `every`, `find`, `reduce`, `map`, `filter`,
  `take`, `drop`, `flatMap`, `chunks`, `windows`, `includes`, `join` e depois os símbolos `@@iterator`,
  `@@toStringTag`, `@@dispose`: idêntica à do bun.

## Pendente

Rodar `cargo test --test key_order_bun_golden` (outro agente/integrador) e corrigir pelas tabelas HashTable e pela
ordem de `put_direct` as divergências que o teste listar. Nada foi compilado nem executado no porte nesta tarefa.
