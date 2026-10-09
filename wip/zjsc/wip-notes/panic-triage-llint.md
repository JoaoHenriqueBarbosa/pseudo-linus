# Triagem de panic em src/llint e src/interpreter (PLAN.md item 10)

Critério: (a) invariante que o C++ garante com RELEASE_ASSERT/ASSERT/CRASH (ou desreferência sem
conferência), mantém; (b) caminho alcançável por JS do usuário em que o JSC lança erro, converte.

Método: `grep -rnE 'unwrap\(|expect\(|panic!|unreachable!' src/llint src/interpreter` (72 ocorrências
fora de linha de comentário). Cada ponto foi lido no contexto e conferido contra
`upstream/JavaScriptCore/runtime/CommonSlowPaths.cpp`, `interpreter/Interpreter.cpp` e
`llint/LLIntSlowPaths.cpp`.

Resultado: **nenhum caso (b)**. Nenhuma alteração de código e nenhum teste novo foram necessários. Os
pontos em que JS do usuário poderia chegar já têm tratamento de erro antes do `expect`:

- `varargs.rs` 191/253: `size_of_varargs` rejeita não-célula, String, Symbol e BigInt com
  `create_invalid_function_apply_parameter_error` (TypeError do upstream) e devolve 0 para
  `undefined`/`null`; só então cai no `RELEASE_ASSERT(arguments.isObject())`.
- `handlers_iterator.rs` 212-220, 290, 318: `iterator_next_try_fast` só roda com a sentinela em `next`,
  que `iterator_open` só grava para iterável com modo rápido (`getIterationMode`); o C++ faz
  `downcast<JSArray>`/`ASSERT(isJSArray(array))` no mesmo ponto (CommonSlowPaths.cpp 1046-1048).
- `slow_paths_control.rs` 110 (`get_property`): os três chamadores já checaram `is_object`.
- `slow_paths_object.rs` 592, 856, 903: invólucro de primitivo nunca é `JSFunction`; `delete` só deixa
  `object` vazio quando há função; `prototype` objeto sempre tem `ObjectRef` (escopo não é alcançável como valor).
- `handlers_accessor.rs` 150, 177: o gerador emite `to_property_key` antes de `set_function_name`.

## (a) Invariantes, mantidos

| Arquivo:linha | Ponto | Asserção do upstream |
|---|---|---|
| `handlers_arguments.rs` 36, 57, 79 | callee que não é JSFunction, escopo que não é de função, registrador que não é DirectArguments | `uncheckedDowncast<...>` no C++ (sem conferência) |
| `handlers_accessor.rs` 150, 177, 192 | `set_function_name` / `new_reg_exp` com operando de tipo errado | `jsCast<JSFunction*>`, `ASSERT(value.isString())` de `JSFunction::setFunctionName`, constante `RegExp` |
| `handlers_object.rs` 259 | `has_structure_with_flags` sobre não-objeto | `asObject(GET_C(m_operand).jsValue())` |
| `handlers_array.rs` 96, 119 | constante de `new_array_buffer`, array de `new_array_with_species` | `ASSERT` / `asObject(...)` |
| `handlers_async.rs` 63, 151 | generator assíncrono e callee como objeto | `asObject(...)` |
| `handlers_private_brand.rs` 35 | base objeto | `ASSERT(baseValue.isObject())` |
| `handlers_iterator.rs` 189, 212, 216, 220, 236, 290, 318, 411, 430, 462 | modos rápidos e sentinelas | `RELEASE_ASSERT_NOT_REACHED()` / `downcast<JSArray>` / `uncheckedDowncast<JSCellButterfly>` |
| `slow_paths_generator.rs` 40, 89 | callee objeto; `create_generator_frame_environment` | `asObject(...)`; `notSupported()` (reescrito pela generatorification) |
| `slow_paths_object.rs` 592, 856, 903 | ver acima | `ASSERT_NOT_REACHED` / `ASSERT(proto.isObject())` |
| `slow_paths_control.rs` 99, 110, 146, 242, 419 | escopo sem JSScope, get_property, catch sem exceção, symbolTable, escopo de eval | `jsCast`, `RELEASE_ASSERT(exception)` em `retrieveAndClearExceptionIfCatchable` |
| `slow_paths.rs` 122, 135 | registrador de escopo, `localScopeDepth` | `jsCast<JSScope*>`; `getScope` do `.asm` |
| `varargs.rs` 191, 253, 326, 354, 440 | argumentos objeto; frame do callee dentro da pilha; escopo de eval; SourceProvider | `RELEASE_ASSERT` em `sizeOfVarargs`/`loadVarargs`/`Interpreter::callFrameForEval` |
| `dispatch.rs` 78, 178, 181, 536, 550, 554 | callee JSCallee, CodeBlock do frame, JSFunction de script com escopo, JITCode do LLInt | `ASSERT`/`RELEASE_ASSERT` do `.asm` e de `Interpreter::executeCallImpl` |
| `dispatch_ext.rs` 350, 388 | `op_unreachable`; `globalThis` fixado | `RELEASE_ASSERT_NOT_REACHED()` / `crash()` no `.asm`; `JSGlobalObject::create` |
| `interpreter.rs` 193 | opcode sem handler (`op_unreachable`, `op_yield`, `op_create_generator_frame_environment`, `wide16/32`) | `crash()` / `notSupported()`; nenhum alcançável por JS (ver `interpreter-panics.md`) |
| `interpreter.rs` 246, 254, 539 | `prepareForExecution` sem CodeBlock, JITCode do LLInt, SourceProvider | `ASSERT(codeBlock)` / `RELEASE_ASSERT` em `Interpreter::executeProgram` |
| `interpreter.rs` 382, 429, 495, 499, 529 | caminho JSONP | `RELEASE_ASSERT_NOT_REACHED()` em `Interpreter::executeJSONPProgram`; o caminho vem da API do embedder, não do fonte JS |
| `execute_call.rs` 98, 113, 118, 145 | `CallData::JS` com callee JSFunction, CodeBlock, JITCode; `CallData::None` | `ASSERT(callData.type == JS)`, `RELEASE_ASSERT`; chamadores checam `isCallable` antes |
| `execute_eval.rs` 89, 244, 331 | nó da cadeia, JITCode, CodeBlock do eval | `RELEASE_ASSERT(node)`, `ASSERT(codeBlock)` |
| `execute_module_program.rs` 57, 65 | CodeBlock e JITCode do módulo | `ASSERT(codeBlock)`, `RELEASE_ASSERT` |
| `call_frame.rs` 354 | `stackPointerOffset` com CodeBlock | `ASSERT` de `CallFrame::...`; valor do próprio CodeBlock |
| `stack_visitor.rs` 258, 259, 268 | `createArguments` em frame sem CodeBlock/callee | `jsCast<JSFunction*>(callee)` e `ASSERT` em `StackVisitor::Frame::createArguments` |
| `unwind.rs` 289, 290 | frame JS do percurso tem CallFrame e CodeBlock | `ASSERT(callFrame)` / `codeBlock()` desreferenciado em `UnwindFunctor` |

## Código de teste (fora do escopo)

- `caller_source_origin.rs` 49, dentro de `#[cfg(test)]`.

## Limites que NÃO são panic (conferidos)

- Lacunas de porte (`LLIntFailure::Unported`) viram `Error` lançado, não abortam (ver
  `interpreter-panics.md`).
- Entrada inválida de `apply`/spread com varargs, `delete`/`in`/leitura/escrita em primitivo e `catch`
  de terminação saem por exceção JS com a mensagem do upstream.
