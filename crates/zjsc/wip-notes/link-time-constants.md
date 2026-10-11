# LinkTimeConstant: o que está ligado no `JSGlobalObject` do porte

Fonte: `bytecode/LinkTimeConstant.h` (158 constantes, `LINK_TIME_CONSTANT_TABLE` em
`src/bytecode/bytecode_intrinsics_table.rs`), `JSC_FOREACH_BUILTIN_LINK_TIME_CONSTANT` (`derived/JavaScriptCore/JSCBuiltins.h`, 34
entradas) e `runtime/JSGlobalObject.cpp` (`init`, linhas 1196 a 2254). Contagem: 158 = 140 ligadas (as já existentes
mais as desta fatia e da seguinte) + 18 faltando (duas delas sem inicializador no próprio C++).

"Ligada" quer dizer que existe um `set_link_time_constant` no porte. A nova fatia é
`src/runtime/js_global_object_link_time_constants.rs` (`init_link_time_constants`).

## Ligadas antes desta fatia

| Constante | Onde |
|---|---|
| arrayIteratorNextHelper, addDisposableResource, createDisposableResource, getDisposeMethod, getAsyncDisposeMethod | `js_global_object_init.rs` |
| sentinelString, emptyPropertyNameEnumerator, Map, Set | `js_global_object_init.rs` |
| Iterator, wrapForValidIteratorCreate, iteratorHelperCreate | `iterator_constructor.rs` |
| DisposableStack, AsyncDisposableStack | `disposable_stack_globals.rs` |
| typedArrayLength, isTypedArrayView, isSharedTypedArrayView, isResizableOrGrowableSharedTypedArrayView, typedArrayFromFast, isDetached, isTypedArrayOutOfBounds | `typed_array_prototype.rs` |
| Int8Array ... BigUint64Array (12 construtores, com Float16Array) | `typed_array_realm.rs` |
| enqueueJob, resolvePromise, rejectPromise, fulfillPromise, markPromiseAsHandled, isPromiseStatePending, os três `...WithFirstResolvingFunctionCallCheck`, newResolvedPromise, newRejectedPromise, resolveWithInternalMicrotaskForAsyncAwait, asyncFunctionDrive, newHandledRejectedPromise, promiseReturnUndefinedOnFulfilled, promiseResolve, promiseReject, promiseResolveWithThen, performPromiseThen | `promise_global_functions.rs` |
| defaultPromiseThen, Promise | `promise_constructor.rs` |
| asyncGeneratorPrototypeNext, asyncIteratorPrototypeSymbolAsyncIterator | `function_kind_intrinsics.rs` |
| importModule | `js_module_loader.rs` |
| evalFunction | `js_global_object_functions_natives.rs` |

## Ligadas por `init_link_time_constants`

| Constante | Origem |
|---|---|
| 29 `INIT_PRIVATE_GLOBAL` restantes: builtinMapIterable, builtinSetIterable, closeAllIterators, createArrayWithoutPrototype, createInspectorInjectedScript, createObjectWithoutPrototype, crossRealmThrow, defaultAsyncFromAsyncArrayLike, defaultAsyncFromAsyncIterator, flatIntoArray, flatIntoArrayWithCallback, generatorResume, getIteratorFlattenable, getIteratorSync, getOptionsObject, iteratorCloseAllNormal, iteratorZip, performIteration, performProxyObject{Get,GetByVal,Has,HasByVal,SetByValSloppy,SetByValStrict,SetSloppy,SetStrict}, removeFirstFromList, wrapRemoteValue, wrappedIterator | `create_builtin_function` sobre `BuiltinCodeIndex` |
| stringSubstring, isArray, ownKeys, isFinite, min, sameValue (`is`), jsonParse, jsonStringify | `JSFunction::create_native` sobre corpos que já existiam (ver "Visibilidade") |
| Object, String, AggregateError, ReferenceError, SuppressedError | propriedade do global recém-criada por `init` |
| RegExp | `reg_exp_constructor()` |
| regExpProto{Flags,HasIndices,Global,IgnoreCase,Multiline,Source,Sticky,Unicode,DotAll,UnicodeSets}Getter | `GetterSetter` próprio de `RegExp.prototype` |
| regExpBuiltinExec, regExpPrototypeSymbolMatch, regExpPrototypeSymbolMatchAll, regExpPrototypeSymbolReplace | `getDirect` de `RegExp.prototype` |
| hasOwnPropertyFunction | `Object.prototype.hasOwnProperty` |
| callFunction, applyFunction | builtins `call`/`apply` do `Function.prototype` |
| throwTypeErrorFunction, setPrototypeDirect, setPrototypeDirectOrThrow, copyDataProperties, cloneObject, toIntegerOrInfinity, toLength, instanceOf, handleNegativeProxyHasTrapResult, handleProxyGetTrapResult, handlePositiveProxySetTrapResult, createPrivateSymbol, makeTypeError | `js_global_object_private_functions.rs` (corpos nativos) ligados por `JSFunction::create_native`; `copyDataProperties` com conjunto excluído (argumento 1) é `Thrown::Unported`, ver o cabeçalho do arquivo |
| mapStorage, mapIterationNext, mapIterationEntry, mapIterationEntryKey, mapIterationEntryValue, setStorage, setIterationNext, setIterationEntry, setIterationEntryKey | `mapPrivateFunc*`/`setPrivateFunc*` de `ordered_hash_table_storage.rs`: o "storage" é uma `JSCellButterfly` vazia no `cell_registry` com o estado (coleção, `Cursor`, entrada, chave, valor) numa tabela por thread; o cursor faz o papel do rastro de transição do C++ (ver o cabeçalho do arquivo) |
| Array | só se `init` já criou a propriedade global `Array` (hoje não cria `ArrayConstructor`; fica vazia) |

## Faltando

| Constante | Depende de |
|---|---|
| Array | `init` criar o `ArrayConstructor` (existe em `array_constructor.rs`, ninguém o chama) e gravar o global `Array`; a fatia nova já o liga quando a propriedade existir |
| importInRealm, evalInRealm, moveFunctionToRealm | corpos privados em `shadow_realm_prototype.rs` (`import_in_realm_body`, `eval_in_realm_body`, `move_function_to_realm_body`); falta o dono do arquivo expô-los como `host_function!(pub ...)` |
| createRemoteFunction, isRemoteFunction | `createRemoteFunction`/`isRemoteFunction` de `JSRemoteFunction.cpp`, ausentes (`shadow_realm_globals.rs` documenta o aborto) |
| BuiltinLog, BuiltinDescribe | `globalFuncBuiltinLog`, `globalFuncBuiltinDescribe` |
| repeatCharacter | `stringProtoFuncRepeatCharacter` |
| regExpCreate, isRegExp | `esSpecRegExpCreate`, `esSpecIsRegExp` |
| stringIncludesInternal, stringIndexOfInternal | `builtinStringIncludesInternal`, `builtinStringIndexOfInternal` |
| arrayFromFastWithoutMapFn | `arrayConstructorPrivateFromFastWithoutMapFn` |
| regExpStringIteratorCreate | `regExpStringIteratorPrivateFuncCreate` (`js_reg_exp_string_iterator.rs` o deixa de fora) |
| asyncFromSyncIteratorCreate | `asyncFromSyncIteratorCreatePrivate` (`IteratorOperations.cpp`) |
| isConstructor, regExpSearchFast | nenhum inicializador em `JSGlobalObject.cpp` (grep em `LinkTimeConstant::isConstructor`/`regExpSearchFast` só acha o enum); no C++ ficam vazias, nada a portar |

## Visibilidade alterada em arquivos de terceiros

Para a fatia nova alcançar corpos já portados, só a visibilidade mudou (sem tocar em lógica):
`global_func_is_finite` e `array_constructor_is_array_host` viraram `pub(crate)`;
`object_constructor_is` e `reflect_object_own_keys` viraram `host_function!(pub ...)`; os macros
`json_host_function!` e `string_host_function!` geram `pub(crate) fn`; `math_function!` usa
`host_function!(pub ...)`.
