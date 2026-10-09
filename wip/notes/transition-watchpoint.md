# transitionWatchpointSet da Structure: conferência contra o Structure.cpp

Contexto: o `ObjectAdaptiveStructureWatchpoint` (porte de `ObjectAdaptiveStructureWatchpoint.h`) vigia o
`transition_watchpoint_set` da `Structure` do objeto. A conferência abaixo é por leitura de
`upstream/JavaScriptCore/runtime/Structure.cpp`, `StructureInlines.h` e `src/runtime/structure.rs`.

## Onde o C++ dispara o set

Só dois lugares:

1. `Structure::finishCreation(vm, previous, deferred)` (StructureInlines.h:631) termina com
   `previous->fireStructureTransitionWatchpoint(deferred)`. Toda `Structure::create(vm, previous, ...)`
   passa por ele.
2. `Structure::nonPropertyTransition` (StructureInlines.h:541), no atalho das estruturas de array originais:
   não cria estrutura, então chama `structure->didTransitionFromThisStructure(deferred)` à mão.

O construtor copia-de-anterior só chama `didTransitionFromThisStructureWithoutFiringWatchpoint()` (marca
`transitionWatchpointIsLikelyToBeFired`), sem disparar.

## No porte

`Structure::new_from_previous` (structure.rs) faz as duas coisas do construtor e do `finishCreation`:
`did_transition_from_this_structure_without_firing_watchpoint()`, copia o bit, e termina com
`previous.fire_structure_transition_watchpoint(vm)`. O atalho de array em `non_property_transition` chama
`did_transition_from_this_structure(vm)`.

## Tabela

| Caminho do C++ | Dispara no C++ | Porte | Situação |
|---|---|---|---|
| `addNewPropertyTransition` (estrutura nova) | sim, via `create` | `new_from_previous` | bate |
| `addPropertyTransition` com transição existente | não | `add_property_transition_to_existing_structure` | bate |
| `addNewPropertyTransition` que vira dicionário | sim, via `toDictionaryTransition` | `to_cacheable_dictionary_transition` | bate |
| `removeNewPropertyTransition` | sim | `new_from_previous` / dicionário | bate |
| `attributeChangeTransition` (estrutura nova) | sim | `new_from_previous` | bate |
| `attributeChangeTransition` em dicionário não cacheável | não (`attributeChangeWithoutTransition`) | idem | bate |
| `toDictionaryTransition` | sim | `new_from_previous` | bate |
| `changePrototypeTransition` | sim | `new_from_previous` | bate |
| `changeGlobalProxyTargetTransition` | sim | `new_from_previous` | bate |
| `nonPropertyTransitionSlow` | sim | `new_from_previous` | bate |
| `nonPropertyTransition`, atalho de array original | sim, `didTransitionFromThisStructure` | idem | bate |
| `setBrandTransition` (`BrandedStructure::create`) | sim | `new_from_previous` | bate |
| `addPropertyWithoutTransition` / `removePropertyWithoutTransition` / `addOrReplace...` / `attributeChangeWithoutTransition` | não | não dispara | bate |
| `flattenDictionaryStructure` | não | não dispara | bate |
| `setPrototypeWithoutTransition` | não | (só em JSGlobalObject.cpp:1212, na inicialização) | bate |
| `put_direct` em dicionário sem transição | não | não dispara | bate |

Mutação de dicionário não dispara no C++ porque `PropertyCondition::isWatchableWhenValid` rejeita estrutura
dicionário (property_condition.rs:315 e :332 também); o objeto em dicionário nunca tem esse watchpoint
instalado.

## O que falta

Nada. Nenhum caminho precisou de correção, por isso nenhum código de produção mudou.

## Testes (structure.rs, módulo `tests`)

Um teste por caminho que dispara, mais o que não dispara:

- `add_property_transition_fires_previous_transition_watchpoint` (e a transição existente não dispara de novo)
- `remove_property_transition_fires_previous_transition_watchpoint`
- `attribute_change_transition_fires_previous_transition_watchpoint`
- `change_prototype_transition_fires_previous_transition_watchpoint`
- `non_property_transition_fires_previous_transition_watchpoint`
- `original_array_structure_shortcut_fires_transition_watchpoint`
- `to_dictionary_transition_fires_and_dictionary_mutation_does_not`

Os testes não foram rodados (proibido cargo nesta fatia). Falta o teste de ponta a ponta com
`Object.defineProperty`, `delete` e `Object.setPrototypeOf` no construtor de typed array via JS, que depende
de `typed_array_realm.rs` e do pipeline do bytecode.
