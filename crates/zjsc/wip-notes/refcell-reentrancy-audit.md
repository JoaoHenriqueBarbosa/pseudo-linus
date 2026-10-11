# Auditoria de reentrância de RefCell e de ciclo de protótipo (2026-10-08)

Leitura de código apenas (sem cargo). Nenhuma edição de código foi necessária.

## 1. Ciclo de protótipo

- `set_prototype_with_cycle_check` (`object_constructor.rs`) é o `JSObject::setPrototypeWithCycleCheck`
  do C++: percorre a cadeia do novo protótipo com `get_prototype_direct`, lança
  `TypeError: cyclic __proto__ value` se achar o próprio objeto, e PARA ao encontrar um `ProxyObjectType`
  (`break`). Isso é idêntico ao C++ e ao `OrdinarySetPrototypeOf` da especificação (passo 8.c.i): ciclo
  que passa por Proxy NÃO é detectado e é permitido. Então `Object.setPrototypeOf(a, b)` com `a` na cadeia
  de `b` só lança quando a cadeia até `a` é toda de objetos ordinários. Via Proxy, não lança (conforme).
- Laços de busca sem recursão de Proxy (`get_property_slot`, `put_inline_slow`, ~linhas 1100 a 1380 de
  `js_object.rs`) só usam `get_prototype_direct` e nunca atravessam Proxy: o Proxy sai antes pelo
  despacho `get_property_slot_from_proxy` / `overridesPut`. Cadeias ordinárias são acíclicas por
  construção, então não há laço infinito.
- Ciclo que passa por Proxy (`p = new Proxy({}, {})`, `Object.setPrototypeOf(target, p)`) vira recursão
  Proxy -> target -> Proxy, e cada operação do Proxy chama `ProxyObject::check_recursion`
  (`vm.is_safe_to_recurse()`), que devolve `Thrown::StackOverflow` (RangeError). Termina.
- `for-in` (`get_enumerable_property_names`) tem `MAXIMUM_PROTOTYPE_CHAIN_DEPTH` e devolve
  `Thrown::StackOverflow` ao estourar, usando `get_prototype` (com o trap), então um laço de
  `getPrototypeOf` de Proxy também termina.

Veredito: conforme com o C++; sem laço infinito.

## 2. Empréstimos RefCell cujo escopo contém JS

Varredura de todo `borrow()`/`borrow_mut()` em `src/runtime` (formas com guarda nomeado, `if let`, `match`,
`for`, e as de statement único). Resultado: nenhum guarda vivo atravessa chamada que possa executar JS
(getter, setter, toString/valueOf, trap de Proxy, callback de Array.prototype.*, comparador de sort).

Revisados e seguros:

- `js_object.rs` (`butterfly`, `inline_storage`): guardas só em blocos curtos que copiam o valor
  (`get_own_property_slot_by_index` copia `value` e solta antes de `slot.set_value`), `public_length`,
  `can_get_index_quickly`, `get_index_quickly`, conversões de forma. Sem chamada a JS dentro.
  `with_array_storage(_mut)` (61 usos: `js_array.rs`, `js_object_array_storage.rs`, `own_property_names.rs`)
  só recebe closures puras (lê/escreve o storage).
- `structure.rs` (`property_table`, `transition_table`): só manipulação de tabela, sem JS.
- `error_instance.rs::materialize_stack`: o `borrow_mut().pending_frames.take()` é um temporário de
  `let ... else`, solto antes de rodar `Error.prepareStackTrace`. Os frames saem antes do gancho, então a
  releitura de `stack` dentro do gancho não reentra. `set_stack_value` toma e solta o `borrow_mut`
  depois do `put_direct`.
- `js_map.rs`/`js_weak_map.rs`: `map_proto_get_or_insert` segura `borrow_mut` mas não chama JS;
  `map_proto_get_or_insert_computed` solta a tabela antes do `callback` e reconfere a chave (correto).
- `set_prototype.rs` (métodos de conjunto de ES2025): todos os `table().borrow()` são temporários de
  expressão; `has`/`keys`/`next` do `record` (JS) rodam fora de qualquer guarda. `drain_keys` recebe
  closures que só pegam `borrow()` momentâneo.
- `js_ordered_hash_table.rs::iterator_step`: `borrow()` em `let ... else` (solto antes do `else`),
  cursor vivo (forEach/iteradores toleram mutação).
- `array_prototype.rs::StringRecursionChecker`: `borrow_mut` só dentro de `enter` e de `Drop`, sem JS no meio.
- `generic_arguments.rs`, `js_scoped_arguments.rs`, `js_arguments_objects.rs`: guardas em métodos
  folha, sem JS (o `scope.variable_at` é leitura de variável).
- `js_scope.rs`, `iteration_protocol.rs`, `function_overrides.rs`, `executable.rs`, `microtask_queue.rs`,
  `vm.rs`: sem JS sob guarda (o `vm.rs:701` chama `Exception::create` sob `borrow_mut` do
  `termination_exception`; `Exception::create` não toca esse campo).

## 3. Enumeração com getters que mutam o objeto

- `own_names`/`own_property_keys`/`get_own_property_names` devolvem uma `Vec<Identifier>` própria
  (snapshot), construída antes de qualquer getter. Consumidores (`object_assign_generic`,
  `enumerable_own_properties` de `Object.values/entries`, `getOwnPropertyDescriptors`,
  `json_host.rs`, `js_global_object_private_functions.rs` do spread/copyDataProperties) iteram o
  snapshot e a cada nome refazem `own_descriptor` e `object_get`, como o spec manda (propriedade
  apagada por getter anterior é pulada; nunca se indexa a butterfly/structure capturada).
- Os índices de elementos em `get_own_indexed_property_names` são coletados de uma vez para o builder;
  `for index in 0..public_length()` usa `can_get_index_quickly` (limitado por `.get`), sem panic mesmo
  que o array encolha, e não chama JS no meio.
- `for-in`: `JSPropertyNameEnumerator` é imutável (`Vec<JSStringRef>` dentro de `Rc`);
  `compute_next` usa `property_name_at_index` (`.get`) e refaz `has_enumerable_property` por nome
  (o ramo de cache por `structureID` não existe, ver cabeçalho do arquivo), então mutação durante o
  laço só faz nomes apagados serem pulados.
- `Object.defineProperties` segue o mesmo padrão snapshot (`own_names` + descritores lidos um a um).

## 4. Não verificado a fundo (por limite de tempo)

- Callbacks de `Array.prototype.sort/forEach/map` etc. em `array_prototype.rs`: a leitura de elementos
  passa por `try_get_index_quickly` (limitado) e nenhum guarda de RefCell aparece no arquivo fora de
  `StringRecursionChecker` e dos testes; recomenda-se um teste dinâmico (comparador que faz
  `arr.length = 0`) quando o cargo puder rodar.
- Um teste de regressão sugerido para cada item acima: getter que apaga chaves em `Object.keys`,
  `Object.assign`, spread, `JSON.stringify`, `for-in`; `valueOf` que faz `arr.length = 0` em
  `Array.prototype.includes/indexOf`; Proxy com trap `getPrototypeOf` cíclico.

## 5. Teste dinâmico de mutação durante a iteração (2026-10-08)

- `tests/reentrancy_mutation.rs` roda 121 programas (um realm por programa) cujos callbacks,
  comparadores, `valueOf`/`toString`, getters, `toJSON`, iteradores e traps mutam a coleção percorrida
  (Array, Map/Set, JSON, Object.*, spread/for-of, TypedArray com `resize`/`transfer`, String/RegExp,
  Proxy). O esperado é medido no bun por `scripts/gen-reentrancy-golden.js`, que gera
  `tests/golden/reentrancy_bun.tsv`; o harness é `tests/golden/reentrancy_bun_harness.js`.
  Nenhum pânico é aceito; o resultado serializado precisa ser igual ao do bun.
- Este teste ainda NÃO foi rodado (sem cargo na tarefa): a primeira execução diz o que diverge.
- Leitura por inspeção, sem achado: `sort_impl` (`array_prototype.rs`) é snapshot como o C++
  (`sort_compact` -> `sort_stable_sort` -> `sort_commit` com o `length` capturado, e o commit relê o
  objeto por `put_index`/`delete_index`); `Array.prototype.map/filter/forEach/reduce/find/some/every/flat*`,
  `Map/Set.prototype.forEach` são builtins JS (`BuiltinCodeIndex`), então herdam a semântica relida do
  C++; os nativos de `TypedArray` (`sort`, `slice`, `fill`, `indexOf`, `join`, `copyWithin`) já relêem
  `integer_indexed_object_length()` depois do callback e limitam por `.min(...)`, como o C++. Nenhum
  `Vec`/índice capturado que o C++ relê foi encontrado, então nenhuma correção foi feita no código.
