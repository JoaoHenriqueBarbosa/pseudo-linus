# e2e_values: auditoria dos seis sítios de pânico (2026-10-08)

Sem rodar nada (leitura e Edit). As causas abaixo vêm da leitura do código; confirmar rodando o teste.

1. `link_time_constant` vazia (`js_global_object.rs:535`). Faltantes segundo `link-time-constants.md`:
   o único que o corpus exercita pelos builtins JS é `arrayFromFastWithoutMapFn` (`Array.from(x)` sem
   mapFn, em `builtins_combined.js`). Ligado em `js_global_object_link_time_constants.rs` a
   `array_constructor_private_from_fast_without_map_fn_host` (novo em `array_constructor.rs`), que devolve
   `undefined` (atalho nunca se aplica, `Array.from` segue o caminho geral, que é o contrato do C++).
   Ainda vazios e sem uso nos builtins JS portados: `asyncFromSyncIteratorCreate` (`Array.fromAsync`),
   `regExpStringIteratorCreate`, `regExpCreate`, `isRegExp`, `stringIncludesInternal`,
   `stringIndexOfInternal`, `repeatCharacter`, `builtinLog`/`builtinDescribe`, e as de ShadowRealm.
2. `js_object.rs:1201` e 6. `function_constructor.rs:270` têm a mesma causa: `CellEntry::as_js_object`
   (`cell_registry.rs`) não tinha braço para `CellEntry::Function` nem `CellEntry::Callee`, então
   `JSObject::from_cell_id` e `from_value` devolviam `None` para qualquer função (daí `Function.prototype`
   como protótipo falhar o `is_null` e `new Function` falhar o `expect`). Adicionados os dois braços
   (o `Deref` encadeado JSFunction, JSCallee, JSNonFinalObject chega no `JSObject`).
3. `as_int32` em não-int32: `JSValue::as_uint32` não tratava `Double` (o `is_uint32` do porte só aceita
   `Int32`, mas campos internos e valores recebidos por bytecode podem vir como `Double` inteiro).
   `as_uint32` agora converte o `Double`; `internal_field_as_int32` (`asInt32AsAnyInt`) também aceita
   `Double`. Handlers de bitwise/shift/`add`/`below` e `to_property_key` já checam `is_int32` antes.
   Se o pânico persistir, o próximo suspeito é gerador (estado em `internal_field_as_int32`) ou
   `for-in` (`handlers_enumerator.rs`, `index`/`mode` por `as_uint32`).
4. `js_array.rs:931`: `handlers_iterator.rs` passava `array_structure()` (Undecided) como estrutura do
   par `[i, valor]` de `entries()`; o C++ usa `arrayStructureForIndexingTypeDuringAllocation(ArrayWithContiguous)`.
   Corrigido.
5. `property_slot.rs:294`: `get_with_this` (`handlers_accessor.rs`, `super.x` / `super[i]`) chamava
   `slot.get_value()` num slot de getter ou custom. Agora trata accessor com `call_getter` (exceção fica
   pendente, `check_exception` do chamador) e o resto com `get_value_for`/`get_value_for_index`.
6. `Can't find variable: x`: o bun mede `x is not defined` em todos os contextos (leitura, `x++`, chamada,
   atribuição estrita, `with`, `eval`, destructuring). `variable_not_found` (`llint/slow_paths.rs`) agora usa
   `create_undefined_variable_error`, a mesma mensagem de `slow_paths_object.rs`.
7. `new Symbol()`: `constructSymbol` lançava sem site; o C++ apenda `(evaluating '...')` no unwind. O
   `handle_host_call` (`llint/dispatch.rs`) agora detecta `construct_symbol` e lança com o site.
8. `get` em Proxy revogado escapava do try/catch: `get_property` (`slow_paths_object.rs`) não checava a
   exceção pendente após `get` de objeto, devolvia `Ok(valor)`. Agora retorna `Err(Thrown)` (como no ramo de função).
9. `re.lastIndex` undefined: `JSObject::get_own_property_slot` (`js_object.rs`) lia só a `Structure`, e o
   `lastIndex` de `RegExpObject` mora num campo (`reg_exp_object.rs`). Adicionado o desvio para
   `RegExpObject::get_own_property_slot` quando o tipo é `RegExpObjectType` e o nome é `lastIndex`.
10. `Function.length` 0 em arrow e com default (NÃO resolvido, só leitura): o parser
   (`parse_formal_parameters`, `parse_function_parameters`, `parse_function_body`), o `FunctionMetadataNode`, o
   `UnlinkedFunctionExecutable::create` e o `reify_length` conferem com o C++; `ParserFunctionInfo.function_length`
   existe mas ninguém o usa (candidato, se o JSC 2026 separa `functionLength` de `parameterCount`).
   `function f(a, b) {}` dá 2 nos goldens (corpo curto, fora do cache de funções); suspeitar do caminho com
   `SourceProviderCache` (função longa) ou de uma segunda análise. Precisa de rodada com depuração.
11. Gerador com `return()` dentro de `finally` (`Cannot access '6' before initialization`): investigado só por
   leitura, NÃO resolvido. Conferem com o C++: `FinallyContext::new` (`bytecode_generator_cpp1.rs`, completionType
   NORMAL e completionValue vazio via `move_empty_value`), `emit_finally_completion` e
   `emit_return_via_finally_if_needed` (`bytecode_generator_cpp6.rs`), a `BytecodeGeneratorification`
   (`bytecode_generatorification.rs`: save/resume por `put_to_scope`/`get_from_scope` `ResolvedClosureVar`,
   identificador `Identifier::from_u32(índice do local)`). Nenhum `emit_tdz_check` do gerador toca esses
   registradores (os únicos chamadores são `this`, brand e `emit_tdz_check_variable`). O nome '6' só pode vir do
   identificador do slot do quadro do gerador, e os únicos lançadores com identificador são `tdz_error` em
   `slow_paths.rs:160/195` e `throw_tdz` em `slow_paths_object.rs:870/900`, todos exigindo
   `scope.is_global_lexical_environment()`. Hipótese a medir: o registrador de escopo (argumento `Frame`) vira o
   ambiente léxico global no caminho de retomada com `resume mode Return`, ou o valor vazio do completionValue
   salvo é relido por outro caminho. Próximo passo: despejar o bytecode do corpo do gerador (op_resume/op_yield)
   e imprimir `scope` no `slow_path_get_from_scope` quando o identificador for numérico.

## `f.length` dava 0 em toda função de usuário (causa e correção)

Causa: `f.length` compila para `op_get_length`, e `get_length` em `src/llint/dispatch_ext.rs` testava
`JSObject::from_value` ANTES do ramo `as_js_function`. Como `JSFunction` também é `JSObject`, o ramo de função
nunca era alcançado e o `object.get` simples não passa por `JSFunction::getOwnPropertySlot`, então o `length`
preguiçoso (`reify_length`, que lê `parameter_count` corretamente) nunca era materializado. O `name` funcionava
porque vai por `get_by_id`. O parser, `UnlinkedFunctionExecutable::create` e `reify_length` estavam corretos.
Correção: o ramo `as_js_function` passou para antes do ramo `JSObject`. Não compilado nem medido (sem cargo);
reconferir com `(function(){ function f(a,b){} return f.length })()` (esperado 2).

## `lastIndex` de RegExp e `for-in` sobre `String` wrapper (causa e correção, não compilado)

O porte guarda `lastIndex` num campo de `RegExpObject`, não na `Structure`. `JSObject::get_property_slot` só chamava
`get_own_property_slot` para tipos com a flag `OverridesGetOwnPropertySlot` lida de `inline_type_flags`, e o desvio
anterior em `get_own_property_slot` nunca era alcançado. Correção em `src/runtime/js_object.rs`: o laço agora despacha
`RegExpObjectType`, `StringObjectType` e `DerivedStringObjectType` por tipo, além da flag. Em
`src/runtime/own_property_names.rs`, `lastIndex` entra em `get_own_non_index_property_names` (modo Include) antes das
propriedades da `Structure`.
`for-in` sobre `Object('ab')`: o enumerador já listava os índices, mas `has_enumerable_property` chamava
`get_property_slot` e o `JSObject` nunca consultava o `StringObject` (`length` e caracteres). Acrescentado
`string_object_own_slot` em `get_own_property_slot` e em `get_own_property_slot_by_index`.
Reconferir: `/a/g.lastIndex` (0), `Object.getOwnPropertyNames(/a/g)` (['lastIndex']), `for (k in Object('ab'))` ('0','1').
