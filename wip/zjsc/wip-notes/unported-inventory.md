# Inventário de Unported, panic!, unimplemented! e todo!

Varredura de `grep -rnE 'Unported|panic!|unimplemented!|todo!' src` em 2026-10-08. Linhas são de antes das
edições desta passada. `todo!` não ocorre em lugar nenhum. As linhas só de comentário (`//!`, `///`) que citam
`Unported` não estão listadas, a não ser que o cabeçalho do arquivo seja a única documentação da lacuna.

Legenda de estado: RESOLVIDO (nesta passada), ABERTO, INVARIANTE (`panic!` que espelha `RELEASE_ASSERT` ou
`ASSERT_NOT_REACHED` do C++, não é lacuna).

## 1. Resolvidos nesta passada

| Arquivo:linha | O que faltava | Como fechou |
|---|---|---|
| runtime/number_constructor.rs:49 e :73 | `JSBigInt::toNumber` em `Number(bigint)` e `new Number(bigint)` | `JSBigInt::to_number(JSValue)` em js_big_int_part9.rs (sobre `to_number_heap`) |
| runtime/iterator_operations.rs:47 | `JSValue::get` de primitivo em `get_value_property` | `toObject` e busca no protótipo do wrapper com o primitivo como receptor |
| runtime/iterator_prototype.rs:186 | `join` com objeto, `Symbol` e `BigInt` | `JSValue::to_wtf_string` (lança `TypeError` de Symbol, chama `toPrimitive`) |
| runtime/reg_exp_prototype.rs:152 | `flagsString` genérico (objeto que não é `RegExpObject`) | `generic_flags_string` lê as oito flags na ordem de `JSC_REGEXP_FLAGS` |
| runtime/reg_exp_prototype.rs (toString) | `toString` lia `source`/`flags` direto, sem `get` | passa por `get` de `source` e `flags` como o C++; getter e `toString` agora devolvem `Thrown` |
| runtime/error_natives.rs, error_instance.rs, interpreter/unwind.rs | `Error.captureStackTrace`, propriedade `stack` | ver relatório; `Error.prepareStackTrace` continua ABERTO (item 4) |

## 2. ABERTO, sem dependência de outro agente

| Arquivo:linha | O que falta | Dependência |
|---|---|---|
| runtime/array_prototype.rs:127 | `ToObject` de primitivo no `Array.prototype.*` (`PutError::Unported`) | `JSValue::to_object` existe (host_function_support.rs:172): trocar o retorno por `to_object`; é um refator de assinatura (`PutError` não carrega exceção pendente) |
| runtime/js_array.rs:857 | `put` de `length` com valor não primitivo ou string (`ToNumber` chama `valueOf`) | `PutError` sem variante de exceção pendente |
| runtime/date_prototype_natives.rs:249 | `toObject` de primitivo em `Date.prototype.toJSON` | `JSValue::to_object` existe |
| runtime/date_prototype_natives.rs:309 e :314 | `Intl.DateTimeFormat` com locales/options e ano anterior a 1 | Intl inexistente (ICU) |
| runtime/number_prototype.rs:434, bigint_prototype.rs:103 | `toLocaleString` com locales/options | Intl inexistente |
| runtime/string_prototype_natives_part2.rs:415 | `String.prototype.normalize` fora de ASCII | tabelas unorm2 |
| runtime/function_constructor.rs:211, :219 a :221 | `new Function` com `newTarget` de subclasse; `GeneratorFunction`, `AsyncFunction`, `AsyncGeneratorFunction` | `getFunctionRealm`/`createSubclassStructure`; construtores de generator e async (async_*.rs de outro agente) |
| runtime/js_bound_function.rs:308, :334 | `createInvalidInstanceofParameterError*` | mensagens em error_messages/exception_helpers, trivial se existirem |
| runtime/object_prototype.rs:266 | `call(toString)` de `Object.prototype.toString` via `Interpreter::executeCall` | `call_function` (object_to_primitive.rs) já cobre; trocar o panic |
| runtime/property_slot.rs:305, host_call.rs:160, host_function_support.rs:254, js_promise_host.rs:223 a :256, object_to_primitive.rs:55, js_getter_setter.rs:139 a :168, iterator_operations.rs:33 | Conversões `LLIntFailure::Unported`/`PutError::Unported` em `panic!` ou `Thrown::Unported` (a ponte entre as lacunas do interpretador e o hospedeiro) | Somem à medida que os itens do interpretador (seção 3) fecham; são o canal, não a lacuna |

## 3. ABERTO, interpretador e LLInt (llint/, interpreter/)

| Arquivo:linha | O que falta |
|---|---|
| llint/slow_paths_object.rs:172 | `asObject` de célula que o registro não expõe como `JSObject` (função, escopo) |
| llint/slow_paths_object.rs:227 | `toPropertyKey` de objeto (`toPrimitive`) ou de Symbol |
| llint/slow_paths_object.rs:266 | `JSObject::defineOwnProperty` em put direto (não extensível, `DontDelete`) |
| llint/slow_paths_object.rs:372 | `JSObject::deleteProperty` e `deletePropertyByIndex` no slow path de `del_by_*` |
| llint/slow_paths_object.rs:423, :431, :503 | registrador de escopo sem escopo no registro; `op_get_scope` de callee sem escopo; `ResolvedClosureVar` fora de `JSLexicalEnvironment` |
| llint/slow_paths_object.rs:569 | `prototype` que é função ou escopo (`JSObject::from_value` não os cobre) |
| llint/slow_paths_object.rs:646 | `Function.prototype[Symbol.hasInstance]` e `JSObject::hasInstance` |
| llint/slow_paths.rs:94 a :114 | `resolve_scope` de `ModuleVar` e `Dynamic` (with, módulos) |
| llint/dispatch.rs:118 a :445, dispatch_ext.rs:365 a :389 | frames sem CodeBlock, callee que não é JSCallee, `toThis` de primitivo e sem `globalThis`, `JITCode` que não é entrada do LLInt |
| llint/handlers_object.rs:71, :92 | campo interno de célula que não é gerador (`JSGenerator`, `JSAsyncGenerator`); `getPrototype` de primitivo ou função |
| llint/handlers_scope.rs:54, :63, :83 | `get_parent_scope` sem pai; escopo sem `toObject`; `resolveScopeForHoistingFuncDeclInEval` |
| llint/handlers_misc.rs:110, :152 | `below`/`beloweq` com operando não int32; segundo `Unported` em handlers_misc.rs:152 (ver o texto) |
| llint/varargs.rs:148 a :406 | varargs com argumentos que são célula e não objeto; frame abaixo do fim da pilha; eval direto sem JSScope; CodeBlock chamador sem SourceProvider |
| interpreter/execute_call.rs:98 a :118, execute_eval.rs:123 a :319 | `CallData::JS` de célula que não é JSFunction; `StrictEvalActivation`; put de var de eval; `JITCode` do eval |
| runtime/js_object.rs:484, :1510 | `getPrototype` e `isExtensible` sobrescritos (`JSGlobalProxy`) |
| runtime/js_object.rs:1074, :1124, :1361 | `ordinarySetSlow` com receptor diferente; `put` sobrescrito na cadeia de protótipos; `putDirectIndex` com `GetterSetter` (SparseArrayValueMap) |
| runtime/js_object_array_storage.rs:928, :941 | `notifyPresenceOfIndexedAccessors` e `haveABadTime` (varrem o heap) |
| runtime/js_scope.rs:386 | `JSWithScope` sem objeto |
| runtime/js_property_name_enumerator.rs:259 | protótipo que é função ou escopo no for-in |
| runtime/collection_support.rs:91 | callee de construtor de coleção que não é objeto |

## 4. ABERTO, depende do trabalho de outros agentes

| Arquivo:linha | O que falta | Dono provável |
|---|---|---|
| runtime/proxy_object.rs:203, :1041, :1257, :1362, :1380 | `CustomGetterSetter` em `getValue`; chave de `ownKeys` sem `StringImpl`; trap lançando em método de JSObject sem variante em `PutError`; células que não são objeto do registro | proxy_*.rs |
| runtime/js_promise.rs:1151 a :1223 | 13 `unimplemented!()` (variantes de `PromiseReaction`/`InternalMicrotask` sem corpo) | async_*.rs, js_promise |
| runtime/promise_constructor.rs:260, :282, :285, :287, :408 | corpo de `async function`/generator, carregador de módulos, WebAssembly streaming, `InternalMicrotask::Opaque`, `InternalFieldTuple` | async_*.rs, module*.rs |
| runtime/js_global_object_functions_natives.rs:173, api/eval.rs:86 | `eval`/interpretador lançando `Unported` vira `panic!` | llint/handlers_*.rs |
| runtime/error_natives.rs:178 | `AggregateError` e `SuppressedError` sem construtor | aggregate_error_*.rs |
| `Error.prepareStackTrace` | exige objetos `CallSite` (`getFileName`, `getLineNumber`, `getFunctionName`...), inexistentes; a captura nem consulta a propriedade | novo `js_call_site.rs` (nenhum agente) |
| Estado de `stack` em `ErrorInstance` | lazy `materializeErrorInfoIfNeeded` e os `getOwnPropertySlot`/`put`/`deleteProperty` sobrescritos | `PropertySlot`, put_property_slot.rs |

## 5. INVARIANTE (espelham `RELEASE_ASSERT` do C++, não são lacuna)

parser/parser_part2.rs:453, parser_part3.rs:449 e :459, parser_cpp4.rs:92 e :115 (`RELEASE_ASSERT_NOT_REACHED`);
bytecode/fits.rs:220, code_block_hash.rs:92, parser_part3.rs:779/:867/:938 (`SourceCode` sem provider);
runtime/get_put_info.rs:31/:85/:107, symbol_table.rs:250, typed_array_type.rs:98/:115/:188/:197,
script_executable.rs:518/:621 a :642 (`installCode` com tipo errado), builtin_executables.rs:173,
cell_registry.rs:377, js_microtask.rs:353, js_promise_capability.rs:350, microtask_queue.rs:104,
data_view_prototype.rs:475 a :482, js_big_int_ops.rs:350, host_function_support.rs:205 (`toObject` de
valor que não é primitivo nem objeto), property_slot.rs:339, promise_constructor.rs:128,
js_object_array_storage.rs:1161, wtf/text/* (`StringImpl`, `StringBuilder`, `StringView`, `AtomString`:
estouro de comprimento e variante de largura errada), wtf/dragonbox/detail/div.rs:11, yarr/yarr_pattern.rs:622 a :672
(`PatternTerm` de tipo errado).
