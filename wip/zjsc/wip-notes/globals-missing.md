# Globais de JSGlobalObject.cpp: inventário e estado

Fonte: `upstream/JavaScriptCore/runtime/JSGlobalObject.cpp` (`globalObjectTable` na linha 725, `init(VM&)` a
partir da 1092, `initStaticGlobals` e os `putDirect` do `init`) contra `src/runtime/js_global_object_init.rs`
e os `install_*` que ele chama. Conferido por leitura/grep em 2026-10-08, com outros agentes mexendo nos
mesmos arquivos: onde diz "ausente" vale "não achei ligado no `init`".

## Propriedades globais do JSC

| Nome | Origem no C++ | Estado no porte |
|---|---|---|
| `isNaN`, `isFinite`, `escape`, `unescape`, `decodeURI`, `decodeURIComponent`, `encodeURI`, `encodeURIComponent`, `parseInt`, `parseFloat`, `eval` | tabela, `Function`/`CellProperty` | `add_global_functions` |
| `globalThis` | tabela, `CellProperty` de `m_globalThis` (um `JSGlobalProxy`) | **bloqueado**, ver abaixo |
| `NaN`, `Infinity`, `undefined` | `initStaticGlobals` | não achei ligado |
| `Object`, `Function`, `Array`, `RegExp` | `putDirectWithoutTransition` no `init` | `Object` e `RegExp` ligados; `Function` e `Array` têm construtor portado (`function_constructor.rs`, `array_constructor.rs`), não achei a propriedade global |
| `String`, `Number`, `Boolean`, `BigInt`, `Symbol`, `Date`, `Error` + 6 nativos, `Proxy`, `Reflect`, `JSON`, `Math`, `Map`, `Set`, `WeakMap`, `WeakSet` | tabela / `FOR_EACH_SIMPLE_BUILTIN_TYPE` | ligados |
| `Promise` | `FOR_EACH_SIMPLE_BUILTIN_TYPE_WITH_CONSTRUCTOR` | ausente (há `promise_global_functions.rs`) |
| `Iterator` | `putDirectWithoutTransition(Iterator)` | ausente |
| `ArrayBuffer`, `SharedArrayBuffer`, `DataView` | tabela / `useSharedArrayBuffer` | células e protótipos portados (`array_buffer*.rs`, `data_view*.rs`), ausentes no `init` |
| `Int8Array` ... `BigUint64Array`, `Float16Array` | tabela, `ClassStructure` | `js_generic_typed_array_view.rs` portado, construtores e protótipos ausentes |
| **`WeakRef`, `FinalizationRegistry`** | tabela, `ClassStructure` | **portados nesta fatia** (`weak_ref_globals::install_weak_refs`) |
| **`AggregateError`, `SuppressedError`** | tabela, `ClassStructure` | **portados nesta fatia** (`aggregate_error::install_aggregate_error`, `suppressed_error::install_suppressed_error`) |
| **`Atomics`** | tabela, `PropertyCallback` | **portado nesta fatia** (`atomics_object::install_atomics`), com `wait` sem prazo e `waitAsync` em `Unported` |
| `DisposableStack`, `AsyncDisposableStack` | `useExplicitResourceManagement` | ausentes |
| `ShadowRealm` | `m_shadowRealmPrototype` + `putDirect` | ausente |
| `Intl`, `Temporal` | `putDirectWithoutTransition(Intl/Temporal)` | ausentes |
| `WebAssembly` | tabela, `PropertyCallback` | ausente |
| `console` | tabela, `PropertyCallback` (`createConsoleProperty`) | ausente |

## Casos que não são o que a tarefa supunha

- **`queueMicrotask` não é global do JSC.** Nenhuma ocorrência em `JSGlobalObject.cpp` nem no `jsc.cpp`; a
  função vem do WebCore/Bun. O que o JSC tem é `JSGlobalObject::queueMicrotask(vm, task, ...)` (C++, não JS),
  que `js_promise_host.rs` já cobre. Nada a portar como propriedade.
- **`globalThis` é um `JSGlobalProxy`**, não o próprio global (`finishCreation` e `resetPrototype` chamam
  `setGlobalThis(vm, JSGlobalProxy::create(...))`). O `JSGlobalProxy.cpp` encaminha 14 métodos de
  `methodTable` ao alvo (`getOwnPropertySlot`, `put`, `defineOwnProperty`, `deleteProperty`, `isExtensible`,
  `preventExtensions`, `getOwnPropertyNames`, `setPrototype`, `getPrototype`, as variantes por índice) e
  `JSGlobalProxy::setTarget` faz `Structure::changeGlobalProxyTargetTransition`. O despacho de `js_object.rs`
  é por checagem de tipo fixa (como o `ProxyObject`), sem `methodTable`, então o proxy precisa de ganchos
  em `js_object.rs`, `js_cell.rs` (`GlobalProxyType` já é tratado como exótico) e a ligação em
  `JSGlobalObject::finish_creation`. `global_this()`/`set_global_this()` já existem em `js_global_object.rs`
  e ninguém chama o setter. Não porto o proxy sem esses ganchos (a alternativa de `globalThis` apontar para o
  próprio global é divergência observável: `Object.getPrototypeOf`, `Object.isFrozen`, identidade em
  `resetPrototype`).
- **`FinalizationRegistry.prototype.cleanupSome`** não existe neste JSC (só `register` e `unregister`).

## O que mudou nesta fatia

Arquivos novos em `src/runtime`: `js_weak_object_ref.rs`, `weak_object_ref_prototype.rs`,
`weak_object_ref_constructor.rs`, `js_finalization_registry.rs`, `finalization_registry_prototype.rs`,
`finalization_registry_constructor.rs`, `weak_ref_globals.rs`, `aggregate_error.rs`, `suppressed_error.rs`,
`array_prototype_unscopables.rs`, `atomics_object.rs`. `cell_registry.rs`: variantes `WeakObjectRef` e
`FinalizationRegistry`.

## Ligações pendentes (arquivos de outros agentes)

1. `js_global_object_init.rs`, depois de `init_error_classes(...)` (os dois dependem do `Error` no global):
   `crate::runtime::aggregate_error::install_aggregate_error(&global_object);` e
   `crate::runtime::suppressed_error::install_suppressed_error(&global_object);`
2. `js_global_object_init.rs`, no fim de `install_json_reflect_and_collections` (ou logo depois da chamada,
   linha 293): `crate::runtime::weak_ref_globals::install_weak_refs(self, object_prototype, function_prototype.as_value());`
   (dentro do método: `self`; fora: `&global_object`) e
   `crate::runtime::atomics_object::install_atomics(&global_object, &object_prototype);`.
3. `array_prototype.rs`, no fim de `finish_creation` (substitui a lacuna `Symbol.unscopables` do cabeçalho):
   `crate::runtime::array_prototype_unscopables::put_array_prototype_unscopables(vm, global_object, prototype);`
4. DRY: `aggregate_error.rs` repete `get_if_property_exists` (privada em `object_constructor.rs`) e, junto com
   `error_natives.rs`, a criação de protótipo de erro e de construtor. Quando `error_natives.rs` estabilizar,
   `get_if_property_exists`, `prototype_structure` e `create_error_prototype_object` sobem para `pub(crate)` ali
   e a cópia de `aggregate_error.rs` some; o `ErrorInstance` de `error_natives.rs` também pode usar
   `aggregate_error::get_if_property_exists` no lugar da sequência `HasProperty` + `get` inline do `cause`.
5. `error_natives.rs` e `native_error_constructor.rs` ainda dizem em comentário que `AggregateError` e
   `SuppressedError` não existem.

## Sem conferir

Nada foi compilado nem testado (os testes de unidade estão escritos). Pontos a confirmar na compilação:
`JSGlobalObject` fazer `Deref` até `get_direct_by_name` (usado em `install_error_subclass` para achar o `Error`
no global), `Options::useExplicitResourceManagement` ligar `SuppressedError` no Bun 1.4.2 (está na tabela, então
foi ligado sempre), a mensagem de `to_wtf_string_or_type_error` de objeto com `toString` próprio (o
`message` do `AggregateError` herda a limitação), e o `isAtomicsWaitAllowedOnCurrentThread` (assumido
verdadeiro). Conferir no `bun`: `Atomics.wait(new Int32Array(new SharedArrayBuffer(8)), 0, 1, 5)` (`"not-equal"`),
`Object.getOwnPropertyNames(Atomics)` (ordem), `new AggregateError([1], 'm', {cause: undefined}).hasOwnProperty('cause')`.
