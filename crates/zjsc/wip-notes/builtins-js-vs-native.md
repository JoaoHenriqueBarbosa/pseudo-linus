# Builtins JS contra nativos: C++ e porte

Fonte: `JSC_BUILTIN_FUNCTION*` e entradas `JSBuiltin` das tabelas (`@begin ... @end`) em
`upstream/JavaScriptCore/runtime/*.cpp`, e o uso de `BuiltinCodeIndex::` em `src/runtime/*.rs`.
Um builtin JS aparece na pilha como `get (native:1:11)`; um nativo aparece como `(unknown)`.
Intl e Temporal ficam de fora (outros agentes).

| Construtor ou protótipo | JS no C++ | Estado no porte |
|---|---|---|
| `Reflect` | `apply`, `deleteProperty`, `get`, `has` (os outros nove são nativos no C++) | JS desde esta mudança (`reflect_object.rs`) |
| `Object` | `fromEntries`, `groupBy` | JS (`object_constructor.rs`); `Object.prototype` não tem nenhum JS no C++ |
| `Function.prototype` | `apply`, `call` | JS (`function_prototype.rs`) |
| `Array.prototype` | `every forEach some filter flatMap reduce reduceRight map find findLast findIndex findLastIndex at` (13) | JS (13) |
| `Array` | `from`, `fromAsync` | JS |
| `ArrayIteratorPrototype` | `next` | JS |
| `Map` | `Map.groupBy` | JS (`js_global_object_init.rs`) |
| `Map.prototype` | `forEach` | JS (`map_prototype.rs`, `MapPrototypeForEachCode`; também sob `@forEach`) |
| `Set.prototype` | `forEach` | JS (`set_prototype.rs`, `SetPrototypeForEachCode`; também sob `@forEach`) |
| `Promise` | `Promise.try` e o construtor (`promiseConstructorPromiseConstructorCodeGenerator`) | JS |
| `Iterator` (construtor) | `from`, `concat`, `zip`, `zipKeyed` | JS |
| `Iterator.prototype` | `some every find reduce map filter take drop flatMap chunks windows` e `@@dispose` | JS (12) |
| `%IteratorHelperPrototype%` | `next`, `return` | JS |
| `%WrapForValidIteratorPrototype%` | `next`, `return` | JS |
| `Generator.prototype` | `next`, `return`, `throw` | JS |
| `%AsyncIteratorPrototype%` | `@@asyncDispose` | JS |
| `DisposableStack.prototype` | `adopt defer dispose use move` | JS |
| `AsyncDisposableStack.prototype` | `adopt defer disposeAsync use move` | JS |
| `ShadowRealm.prototype` | `evaluate`, `importValue` | JS |
| `%TypedArray%` | `from` | JS |
| `%TypedArray%.prototype` | `at`, `toLocaleString` | JS |
| `String`, `String.prototype`, `Number`, `Boolean`, `Symbol`, `JSON`, `Math`, `Error`, `RegExp`, `Date`, `WeakMap`, `WeakSet`, `WeakRef` | nenhum (todos nativos no C++) | nativos, igual |

Divergência que sobra: nenhuma entre os builtins JS do C++ e o porte. `Map.prototype.forEach` e
`Set.prototype.forEach` passaram a ser o JS embutido de `MapPrototype.js`/`SetPrototype.js`, com os
intrínsecos de `ordered_hash_table_storage.rs`; as versões nativas foram removidas.

Testes de instalação (name, length, intrínseco): `reflect_object.rs`, módulo `tests`
(`installs_js_builtins_and_natives_with_name_length_and_intrinsic`), e `map_prototype.rs`/`set_prototype.rs`
(`for_each_is_js_builtin_under_public_and_private_name`). Não executados (sem cargo).
