# Tipo de cada construtor global: bun contra o porte

Medido com `Reflect.ownKeys(C)` no bun 1.4.2 (script `/tmp/ck.js`). Regra de leitura:

- `JSF` = `JSFunction` sobre `NativeExecutable`: `length,name` preguiçosos, saem ANTES dos nomes da tabela estática.
- `IF` = `InternalFunction`: os nomes da tabela vêm antes de `length,name`.

| Construtor | ownKeys no bun | Tipo no bun | Tipo no porte | Estado |
|---|---|---|---|---|
| Array | `from,length,name,prototype,of,isArray,fromAsync,@@species` | IF (`from` vem da tabela estática antes de `length,name`, molde InternalFunction) | IF (`array_constructor.rs`; `tests/array_static_props.rs` já fixa `from` na frente) | ok |
| Object | tabela, `length,name,prototype,hasOwn,groupBy` | IF | IF | ok |
| Function | `length,name,prototype` | JSF | IF (`function_constructor.rs`) | DIVERGE, refactor grande (campo `function_constructor` é `InternalFunctionRef`, estrutura dos irmãos deriva dele) |
| AsyncFunction, GeneratorFunction, AsyncGeneratorFunction | `length,name,prototype` | JSF | IF (mesmo `create_function_construction_constructor`) | DIVERGE, junto com Function |
| String | `length,name,fromCharCode,...,prototype` | JSF | JSF | ok (corrigido antes) |
| Number | `length,name,isFinite,...,prototype,EPSILON,...` | JSF | JSF | ok |
| Boolean | `length,name,prototype` | JSF | JSF | ok |
| Symbol | `for,keyFor,length,name,prototype,...` | IF | IF | ok |
| BigInt | `asUintN,asIntN,length,name,prototype` | IF | IF | ok |
| Date | `parse,UTC,now,length,name,prototype` | IF | IF | ok |
| RegExp | `input,$_,...,$9,length,name,prototype,escape,@@species` | IF | IF (`RegExpConstructor` em `reg_exp_prototype_natives.rs`: `InternalFunction::new`, tabela legada antes de `length,name`, `escape` e `@@species` depois do `prototype`) | ok (lido no código, teste `reg_exp_own_keys`) |
| Error | `length,name,prototype,stackTraceLimit,captureStackTrace,isError,appendStackTrace,prepareStackTrace` (os dois últimos são do Bun, fora do escopo do porte) | JSF | IF (`error_natives.rs`, `create_constructor`; `ErrorConstructor` tem `OVERRIDES_PUT` para o `stackTraceLimit`) | CORRIGIDO (não compilado): `JSFunction` com `JSFunctionType` e `OVERRIDES_PUT` na `Structure` (`error_constructor_structure`); o put/delete do `stackTraceLimit` seguem pelos ganchos de `JSObject::put`/`delete_property` |
| EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, AggregateError | `length,name,prototype` | JSF | JSF (`create_error_constructor_function` em `error_natives.rs`; `AggregateError`, `SuppressedError` e erros Wasm pelo mesmo caminho) | CORRIGIDO (não compilado) |
| Promise | `length,name,resolve,reject,race,all,allSettled,any,withResolvers,prototype,try,@@species` | JSF | JSF (`promise_constructor.rs`, `JSFunction::create_with_structure` sobre o builtin) | ok (teste `promise_and_error_own_keys`) |
| Map | `length,name,prototype,groupBy,@@species` | JSF | JSF (`define_collection_constructor!`, migrado) | CORRIGIDO (não compilado) |
| Set | `length,name,prototype,@@species` | JSF | JSF (idem) | CORRIGIDO (não compilado) |
| WeakMap, WeakSet | `length,name,prototype` | JSF | JSF (idem) | CORRIGIDO (não compilado) |
| WeakRef, FinalizationRegistry | `length,name,prototype` | JSF | JSF (`create_native_collection_constructor`) | CORRIGIDO (não compilado) |
| Proxy | `length,name,revocable` (sem `prototype`) | JSF | era IF, agora JSF | CORRIGIDO nesta fatia |
| ArrayBuffer | `length,name,prototype,isView,@@species` | JSF | JSF | CORRIGIDO (não compilado) |
| SharedArrayBuffer | `length,name,prototype,@@species` | JSF | JSF (mesmo `ArrayBufferConstructor::create`) | CORRIGIDO junto, sem teste (global só com `useSharedArrayBuffer`) |
| DataView | `length,name,prototype,BYTES_PER_ELEMENT` | JSF | JSF | CORRIGIDO (não compilado) |
| Int8Array...BigUint64Array, Float16Array | `length,name,prototype,BYTES_PER_ELEMENT` (Uint8Array +`fromBase64,fromHex`) | JSF | JSF (`typed_array_constructors.rs`) | CORRIGIDO (não compilado) |
| %TypedArray% | `length,name,prototype,of,from,@@species` | JSF | JSF (`typed_array_constructor.rs`; `TypedArrayRealm` com `JSFunctionRef`) | CORRIGIDO (não compilado) |
| Iterator | `length,name,prototype,from,concat,zip,zipKeyed` | JSF | JSF (`iterator_constructor.rs`; `IteratorGlobalData` com `JSFunctionRef`) | CORRIGIDO (não compilado). Atenção: `concat`/`zip`/`zipKeyed` dependem das opções `useIteratorSequencing`/`useJointIteration`, o teste `iterator_own_keys` as assume ligadas |
| DisposableStack, AsyncDisposableStack, ShadowRealm, Temporal.*, Intl.* | não medidos | ? | IF (`create_collection_constructor`) | pendente de medição no bun |

Nota: `Reflect.ownKeys` põe os símbolos sempre por último, então a ordem relativa de `@@species` e `groupBy`/`of`/`from` não é observável.

Observação sobre ordem de varredura: o `Array` é `InternalFunction` no bun (o `from` sai da tabela estática antes
de `length,name`, molde IF), então não diverge. O primeiro divergente com molde claro e raio pequeno foi o `Proxy`,
depois `ArrayBuffer`/`SharedArrayBuffer`/`DataView`.

## Proxy: o que mudou

`src/runtime/proxy_constructor.rs`: `ProxyConstructor::create` agora usa `JSFunction::create_native_with_structure`
(length 2, name `Proxy`, `Intrinsic::NoIntrinsic`), a `Structure` é `JSFunctionType` com `JS_FUNCTION_S_INFO` como pai,
e `revocable` continua por `put_direct_native_function_without_transition`. O chamador
(`js_global_object_init.rs`) não muda: só consome `.as_value()`. Não compilado nem testado (sem cargo nesta fatia).

## ArrayBuffer, SharedArrayBuffer e DataView: o que mudou

`collection_support.rs` ganhou `native_constructor_structure` (`JSFunctionType`, `ClassInfo` com pai
`JS_FUNCTION_S_INFO`) e `create_native_collection_constructor` (`JSFunction::create_native_with_structure` com
`length`/`name` preguiçosos, `prototype` e, com `species`, o acessor `@@species`). `array_buffer_constructor.rs` e
`data_view_constructor.rs` usam os dois; `ClassStructure.constructor` e os retornos de `init_array_buffer`/
`init_data_view` em `js_array_buffer.rs` passaram de `InternalFunctionRef` a `JSFunctionRef`. O `ownKeys` esperado
(medido no bun) é igual ao que o molde IF já dava (`length,name` primeiro, pois o `finish_creation` os põe antes de
`prototype`); a mudança é de tipo da célula (`JSFunctionType`). Teste: `tests/constructor_kinds.rs`. Não compilado nem
rodado (sem cargo nesta fatia).
