# Triagem de panic em src/runtime (PLAN.md item 10, primeira fatia)

Critério (o mesmo de `panic-triage-parser.md`): (a) invariante que o C++ garante com
RELEASE_ASSERT/ASSERT/CRASH (ou desreferência sem conferência), mantém; (b) caminho alcançável por
entrada JS do usuário em que o JSC lança erro (TypeError, RangeError, "Out of memory"), converte.

Método: `grep -nE '\.unwrap\(\)|\.expect\(|panic!|unreachable!' src/runtime/*.rs`, cortando cada arquivo
em `#[cfg(test)]` e descartando comentários `//!` e `///`. O total de ~1114 é inflado por código de
teste: os arquivos `*_prototype*.rs` e `*_constructor*.rs` com mais ocorrências brutas
(`array_prototype` 18, `promise_constructor` 20, `error_prototype` 7, `string_*` 3, `data_view_prototype` 3)
têm quase tudo dentro de `#[cfg(test)]`. Fora do teste, o maior é `js_global_object_init.rs` (32).

## Resultado desta fatia: nenhum caso (b) de panic por entrada JS

Nenhuma alteração de código e nenhum teste novo, porque nada achado é decidido por conteúdo JS onde o
JSC lança. Os caminhos de OOM e RangeError dos builtins já saem como erro: `String.prototype.repeat`
(`string_prototype.rs` 349-374, `StringOpError::OutOfMemory`), `ArrayBuffer`
(`array_buffer_constructor.rs` 97/139, `array_buffer_prototype.rs` 229/377, `Thrown::OutOfMemory`),
`new_array` com comprimento acima de 2^32 (`array_prototype.rs` 575, RangeError "Invalid array length").

## Lido e classificado

### (a) Invariantes, mantidos

| Arquivo:linha | Ponto | Upstream |
|---|---|---|
| `array_prototype.rs` 527 | `object_ref` sobre valor recém-criado | `asObject` sem conferência |
| `array_prototype.rs` 1766 | protótipo de Array | `jsCast<JSArray*>` |
| `function_constructor.rs` 237 | `fromGlobalCode` nulo sem exceção | `throwException(exception)` com ponteiro que o C++ não confere |
| `function_constructor.rs` 258 | `asObject(newTarget)` | `construct` só passa objeto como `newTarget` |
| `function_constructor.rs` 272 | função recém criada fora do registro | `jsCast` |
| `object_prototype.rs` 213, 378 | `slotBase` de CustomAccessor; `objectProtoToStringFunction` | desreferência/`ASSERT` do C++ |
| `promise_constructor.rs` 135-210, 334-350, 438-443, 755-840, 1229 | `uncheckedDowncast`, estruturas do global, `globalContext` do combinador | `uncheckedDowncast` / `ASSERT` |
| `promise_constructor.rs` 253 | `InternalMicrotask::None` | `RELEASE_ASSERT_NOT_REACHED()` |
| `js_array.rs` 273, 932 | `create`/`constructArray` sem memória | `RELEASE_ASSERT_RESOURCE_AVAILABLE(array, MemoryExhaustion, "Crash intentionally because memory is exhausted.")` (JSArray.cpp 2179); a variante com `try` já devolve `None` e quem tem entrada do usuário usa `new_array` (RangeError/OOM) |
| `js_array.rs` 345, 403, 442, 462-484, 601, 731 | mapa esparso, forma de indexação | `ASSERT(map)`, `RELEASE_ASSERT_NOT_REACHED()`/`CRASH()` nos `switch` de forma |
| `js_array.rs` 930 | `values.len()` acima de u32 | `constructArray` recebe `unsigned`; vetor já residente em memória |
| `js_object_array_storage.rs` 78-1146 | `arrayStorage()`, `switch` por forma, entrada recém-adicionada, protótipo objeto/null | `RELEASE_ASSERT`/`RELEASE_ASSERT_NOT_REACHED()`/`CRASH()` dos mesmos `switch` |
| `js_object_array_storage.rs` 784 | `RangeError` ao definir índice de Array | só nasce do nome `length` (comentário no código) |
| `js_object_array_storage.rs` 1249 | exceção em `getOwnPropertyNames` de materialização | `releaseAssertNoExceptionExceptTermination()` |
| `js_object.rs` 301, 1117, 1562, 1673-1701, 2143 | handle não-objeto, `structure().realm()` | `jsCast`, `Structure::globalObject()` nunca nulo |
| `js_object.rs` 1269-2940 | formas de indexação | `RELEASE_ASSERT_NOT_REACHED()` / `CRASH()` |
| `js_object.rs` 1645, 1662 | atributo Custom/Accessor com valor de outro tipo | `jsCast<CustomGetterSetter*>` / `jsCast<GetterSetter*>` |
| `proxy_object.rs` 240-1321 | `jsCallee` e `realm` do Proxy | `jsCast<ProxyObject*>` |
| `proxy_object.rs` 1043 | chave de `ownKeys` sem `StringImpl` | chave já validada como string/símbolo antes |
| `json_object.rs` 294-754 | pilhas do `Walker` (`mark_stack`, `index_stack`, ...) | `Vector::last()`/`takeLast()` com pilhas empilhadas em pares |
| `literal_parser.rs` 960-1397 | pilhas de estado, `setErrorMessageForToken` | `takeLast()` pareado; `RELEASE_ASSERT_NOT_REACHED()` (LiteralParser.cpp 204, 209) |
| `js_big_int_part7.rs` 57-210 | `digits.last()` de dígitos normalizados | `digits.last()` de BigInt sem zeros à esquerda (invariante de `JSBigInt`) |
| `js_big_int_ops.rs` 108-254 | operando sem BigInt | `ASSERT(isBigInt)` no despacho; o chamador só entra com BigInt |
| `js_microtask.rs` 79-706 | `uncheckedDowncast`, `payload` fora de `InternalMicrotask`, `realm` | `uncheckedDowncast`, payload gravado pelo próprio runtime |

### Lacunas do porte (não são (a) nem (b))

`panic!("... ainda não portado")` em `promise_constructor.rs` 321, 324, 326, 450 (carregador de módulos
assíncrono, `JSWebAssemblyStreamingContext`, `JSMicrotaskDispatcher`, `InternalFieldTuple`) e os
`PutError::Unported` / `Thrown::Unported` / `LLIntFailure::Unported(..)` que viram `panic!` em
`host_function_support.rs` 312 e `host_call.rs` 214. Esses são alcançáveis por JS (um comparador de
`Array.prototype.sort` ou um `valueOf` do usuário que cai num opcode do LLInt ainda sem handler, `Proxy`
em `isArray`), mas não têm "erro do upstream" para onde converter: o JSC executa o caminho. O remédio é
portar o caminho, não trocar o panic. Listados aqui para o roteiro de portes, não para esta triagem.

## O que falta

- `js_global_object_init.rs` (32) e `js_global_object.rs` (25): inicialização do global, classificadas
  só por amostra como `uncheckedDowncast`/estrutura ausente; falta ler uma a uma.
- `js_module_record.rs` (18), `js_microtask_async.rs` (13), `vm.rs` (12), `script_executable.rs` (11),
  `function_kind_intrinsics.rs` (10), `typed_array_realm.rs` (9), `js_web_assembly.rs` (9), `js_scope.rs` (8),
  `js_bound_function.rs` (8), `abstract_module_record_resolve.rs` (8), `call_data.rs` (7),
  `temporal_object.rs` (6), `structure.rs` (6): não lidos.
- Os demais `*_prototype*.rs` e `*_constructor*.rs` têm 0 a 5 ocorrências fora de teste cada; a contagem
  bruta aponta quase só `#[cfg(test)]`, mas o corte por `awk` não foi conferido arquivo a arquivo.
- Provar (b) por teste exige amostrar entradas JS hostis (comprimentos 2^32, `length` de objeto
  array-like gigante, `Array.prototype.concat` com espécie): o golpe mais provável de aparecer é fora
  deste conjunto, em `array_prototype` com `length` próximo de 2^53, que sai por `new_array` (RangeError).
  Próximo passo: golden contra o `bun` desses casos.

## Segunda fatia: os `panic!` de `promise_constructor.rs` e o `Unported`

### `promise_constructor.rs` 322, 325, 327, 451: hoje inalcançáveis por JS

Medido por `grep` em `src`: nenhum código enfileira `ModuleRegistryFetchSettled`, `ModuleLoad*`,
`ModuleGraphLoadingError`, `DynamicImportDeferLoadSettled`, `WebAssemblyCompileStreaming`,
`WebAssemblyInstantiateStreaming` nem `Opaque` (as variantes só aparecem em `microtask.rs`, no `match` de
`promise_constructor.rs` e na travessia de `js_module_record.rs`). `create_internal_field_tuple` só é
chamado quando `async_context()` não é `undefined` (sempre `undefined`, não há `AsyncLocalStorage`) ou
quando `is_internal_field_tuple` é verdadeiro (sempre `false`). Portanto nenhum desses panics dispara
com entrada JS, e não há comportamento observável no bun para medir: o upstream os alcança só com o
carregador de módulos assíncrono (`ModuleLoad*`, ainda a portar), o streaming de WebAssembly
(`WebAssembly.compileStreaming`, que o porte não expõe) e o `AsyncLocalStorage` do Bun. Portar o primeiro
(14 tarefas de carregador) não cabe em fatia e não tem teste possível antes de o produtor existir.
Nenhum código alterado. Regra para o roteiro: quando o produtor de cada grupo nascer, o handler nasce
no mesmo commit e o `panic!` some.

### Opcodes do LLInt sem handler: nenhum alcançável

Conferido contra `src/bytecode/opcode.rs` (194 opcodes) e as ocorrências `OpcodeID::op_*` em `src/llint`:
só `op_wide16`, `op_wide32` (prefixos, consumidos pela decodificação) e `op_yield` (o
`bytecode_generatorification.rs` troca todo `op_yield` antes da execução) não têm braço. Logo
`LLIntFailure::UnportedOpcode` não é alcançável a partir de comparador de `sort` nem de `valueOf`.

### Terceira fatia: módulos, microtasks assíncronas, VM, executáveis e vizinhos (nenhum caso (b))

Corte por `#[cfg(test)]` conferido arquivo a arquivo. Nenhum panic é decidido por conteúdo JS onde o JSC lança;
nenhuma alteração de código e nenhum teste novo. Medido no bun 1.4.2 o único caso suspeito (abaixo).

(a) Invariantes, mantidos:

| Arquivo:linha | Ponto | Upstream |
|---|---|---|
| `js_module_record.rs` 138, 187, 234-239, 320, 773 | `JSModuleData` definido uma vez, registro é `JSModuleRecord`, global existente | `ASSERT`/`jsCast` |
| `js_module_record.rs` 315, 401, 436, `abstract_module_record_resolve.rs` 95, 195, 214, 300, 340 | módulo requisitado já carregado | `getImportedModule` depois de `LoadRequestedModules` ("which has been ensured"); o link só roda depois de o carregamento ter dado certo |
| `js_module_record.rs` 537, 623, 634 | SymbolTable do ambiente, `Resolution` resolvida com registro, `UnlinkedCodeBlock` do `ModuleProgramExecutable` | `ASSERT`/desreferência do C++ |
| `js_module_record.rs` 733, 1131, 817, 1081, 1099, 1398 | pilhas de ligação e avaliação, capability do TLA, `CycleRoot`, `PendingAsyncDependencies` | passos 10.a e assertivas da ECMA-262 (InnerModuleLinking/Evaluation) |
| `abstract_module_record_resolve.rs` 110, 228, 258 | `frames` do ResolveExport | pilha empilhada em par |
| `js_microtask_async.rs` 46, 111, 164, 399-538 | realm da Structure, objeto recém-criado como resultado de iterador, alvo do AsyncFromSyncIterator, `uncheckedDowncast` | `uncheckedDowncast`, `ASSERT`, objeto criado pelo próprio runtime |
| `js_microtask_async.rs` 167, 441, 478, 492, 504, 563 | promessa pendente em continuação, razão de suspensão, tarefa fora do grupo | `RELEASE_ASSERT_NOT_REACHED()` (o `switch` do C++) |
| `vm.rs` 38-586 | getters de estruturas e sentinelas lidos antes da criação | `VM` cria tudo no construtor; o C++ lê o ponteiro sem conferir |
| `vm.rs` 928 | terminação pendente sem exceção | `hasPendingTerminationException()` implica exceção |
| `script_executable.rs` 138, 517-519, 620-641, 789, 819 | `SourceProvider`, tipo do executável em `installCode`, `CodeBlock` de substituição | `jsCast`/`ASSERT`/`RELEASE_ASSERT` |
| `script_executable.rs` 329 | `UNREACHABLE_FOR_PLATFORM()` | idem |
| `script_executable.rs` 759 | `to_error_object` do erro de parse de função | caminho só entra com `ParserError` diferente de `ErrorNone` (`error_info.rs` 91) |
| `js_bound_function.rs` 144-401 | alvo é `JSFunction`, callee é `JSBoundFunction` | `jsCast` |
| `call_data.rs` 75-242 | `NativeExecutable`, escopo de função não-host, realm do `CallData` | `ASSERT`/`jsCast` |
| `js_scope.rs` 119-851 | `JSWithScope::object()`, fim da cadeia, SymbolTable do léxico | `jsCast` e `ASSERT` |
| `structure.rs` 847-1311 | `PropertyName` nulo, dicionário sem tabela | `ASSERT` no C++ |
| `typed_array_realm.rs`, `function_kind_intrinsics.rs` | campos do global lidos antes da instalação | `LazyProperty`/ponteiros que o `JSGlobalObject` preenche em `init` |
| `temporal_object.rs` 372, 628-631, 685, 929 | `smallestUnit` `auto`, `lengthInNanoseconds` de calendário, membro da tabela | `RELEASE_ASSERT_NOT_REACHED()`. Caso 372 conferido no bun: `Duration.round({smallestUnit:"auto"})` dá `RangeError: smallestUnit cannot be "auto"`, e `validate_temporal_unit_value` já devolve esse `RangeError` (`temporal_object.rs` 187-189) antes do `unreachable!` |
| `js_web_assembly.rs` 281-1514, 407-624 | classe instalada, tipos do espaço de funções, `exnref`/`v128` | `RELEASE_ASSERT_NOT_REACHED()` |

Alcançabilidade dos módulos: `JSModuleRecord::create` só é chamado em teste (`js_module_record.rs` 133 é a única
definição, nenhum produtor em `src`). Não há carregador, `import()` nem `import` estático ligado ao
interpretador, então nenhum dos `expect` de módulo é alcançável por JS hoje; quando o carregador nascer, a
conferência dos 315/401/436 é a mesma (o link só roda depois de `LoadRequestedModules` ter dado certo).

Lacuna do porte (não é (a) nem (b)): `js_bound_function.rs` 369, o `panic!` em `encode_call_result` para
`LLIntFailure` que não é `Thrown`, é o mesmo caso de `host_call.rs` 214 acima (portar o caminho, não trocar o panic).

Restam sem leitura linha a linha só os `*_prototype*.rs` e `*_constructor*.rs` de 0 a 5 ocorrências fora de teste
e o corpo de `js_global_object_init.rs`/`js_global_object.rs`.

### `LLIntFailure::Unported` que ainda chegam ao `panic!` (host_call.rs 214, host_function_support.rs 312)

Origens em `src/llint`, todas condições que o C++ trata como invariante:
`slow_paths_object.rs` 167 (`asObject` sobre primitivo/célula não objeto), 180 (protótipo de wrapper
primitivo que não é objeto), 194 (`JSFunction` sem handle no registro); `handlers_misc.rs` 119
(`below`/`beloweq` com operando fora de uint32: o gerador só emite sobre resultado de `>>>` ou constante
inteira); `handlers_object.rs` 71 (`get_internal_field`/`put_internal_field` em célula sem campos
internos: `JSMapIterator`, `JSSetIterator`, `JSPromise`); `handlers_enumerator.rs` 52. Candidato real a
tropeço: `handlers_object.rs` 71, que depende do porte dessas três classes terem `InternalFields`; falta
um caso JS (iterador de `Map` consumido por `for-of` dentro de comparador) medido no bun e rodado no
porte para confirmar se algum chega lá. Próxima fatia.
