# Produtores restantes das variantes de lacuna (2026-10-08)

Varredura de `LLIntFailure::Unported(`, `LLIntFailure::UnportedOpcode`, `PutError::Unported(`,
`Thrown::Unported(`, `HostThrown::Unported`, `ModuleThrown::Unported` em `wip/zjsc/src`.
Produtor é onde a variante nasce de um caso não portado. Os demais usos (conversão entre tipos de
erro, `panic!` no topo) são repasse e ficam fora da lista.

## Produtores nos arquivos de outros agentes (não tocados)

Em `src/llint/handlers_*.rs`:

| Local | Caso do C++ que falta | Classe |
|---|---|---|
| `handlers_enumerator.rs:52` | registrador do enumerador sem `JSPropertyNameEnumerator` (`jsCast<JSPropertyNameEnumerator*>`) | invariante (`jsCast` assert) |
| `handlers_object.rs:71` (`NO_INTERNAL_FIELDS`) | objeto sem campos internos | a conferir pelo dono do arquivo |
| `handlers_object.rs:245` | `has_structure_with_flags` sobre objeto fora do alcance do registro (escopo) | invariante provável (escopo não chega a JS) |
| `handlers_async.rs:152` | `create_promise` com callee que o registro de células não expõe como objeto | invariante (`jsCast<JSObject*>`) |
| `handlers_misc.rs:119` | `below`/`beloweq` com operando que não é uint32 | invariante (o gerador só emite sobre `urshift` ou constante inteira) |

Em `proxy_object.rs`, `js_object.rs`, `array_prototype.rs`: nenhum produtor encontrado pela varredura.

## Produtores fora da exclusão

| Local | Caso do C++ que falta | Classe | Ação |
|---|---|---|---|
| `src/llint/slow_paths_object.rs:164` (`object_for_access`) | `JSValue::synthesizePrototype` / `toObject` para base String, Symbol, BigInt, escopo, e Number/Boolean sem protótipo no `JSGlobalObject` | comportamento | pendente: portar os protótipos primitivos |
| `src/llint/slow_paths_object.rs:176` (`object_handle_for_access`) | `JSFunction` em caminho que só leva `JSObjectHandle` (falta o ramo de função em quem chama) | comportamento | pendente: dar a esses chamadores o ramo `ObjectRef::Function` |
| `src/llint/slow_paths_object.rs:~504` (`object_for_delete`) | `toObject` de primitivo devolvendo função | invariante | CORRIGIDO: `unreachable!` (o invólucro de primitivo nunca é `JSFunction`) |
| `src/llint/slow_paths_object.rs:~780` (`slow_path_create_this`) | `prototype` objeto que é escopo | invariante (`asObject(proto)`) | CORRIGIDO: `expect("ASSERT(proto.isObject())...")` |
| `src/llint/dispatch.rs:414` | opcode sem handler no `run_ext` | comportamento (lacuna de handler, vira `UnportedOpcode`) | ver "Opcodes sem handler" abaixo: a lista exata tem 4 itens e nenhum é lacuna real |

## Repasses e panics (não são produtores)

`iterator_operations.rs:60-61`, `js_getter_setter.rs:150-175`, `js_module_record.rs:170-171`,
`host_call.rs:67,208`, `slow_paths.rs:85,100` convertem entre os tipos de erro.
`js_promise_host.rs:223-255`, `js_module_loader.rs:485-541`, `js_microtask.rs:637`,
`js_module_namespace_object.rs:136`, `host_function_support.rs:312`, `property_slot.rs:305`,
`microtask_queue.rs:191`, `interpreter.rs:174-177` fazem panic ou checagem no topo.
Nenhum `HostThrown::Unported` nem `ModuleThrown::Unported` é construído em lugar algum; só aparecem
em padrões de `match`.

## Opcodes sem handler

Método: os 194 `op_*` do enum `OpcodeID` (`src/bytecode/opcode.rs`, gerado de `Bytecodes.h`) contra os
`OpcodeID::op_*` citados em `src/llint/dispatch.rs`, `dispatch_ext.rs` e `handlers_*.rs` (190 distintos).
Diferença exata: `op_unreachable`, `op_wide16`, `op_wide32`, `op_yield`.

| Opcode | O gerador do porte emite? | Situação | Teste/golden que exercitaria |
|---|---|---|---|
| `op_unreachable` | sim, `bytecode_generator_cpp1.rs:235` (fim do `generate` quando o corpo termina sem emitir) | PORTADO neste lote em `dispatch_ext.rs`: `slow_path_unreachable` é `UNREACHABLE_FOR_PLATFORM()` (`RELEASE_ASSERT_NOT_REACHED`), vira `unreachable!` | nenhum golden alcança (o código que o segue nunca roda); só um teste de unidade que force o pc nele, e isso derruba o processo como o C++ |
| `op_yield` | sim, `emit_yield_point` (`bytecode_generator_cpp6.rs:518`) | NÃO precisa de handler: `bytecode_generatorification.rs:115` troca cada `op_yield` por salvar/`switch` antes da execução, igual ao `BytecodeGeneratorification::run` do C++; o `.asm` nem tem `op_yield` | goldens de generator e async (`function*`, `async function`, `await`) |
| `op_wide16`, `op_wide32` | não são instruções: prefixos de largura | NÃO precisam de handler: o `Fits`/decode do `InstructionStream` consome o prefixo e entrega o opcode real (`llintOp` do `.asm` só os usa em `_llint_op_wide16/32` para pular o prefixo) | qualquer fonte com mais de 255 registradores locais ou salto longo (ex.: função gerada com centenas de `var`) |

Conclusão: nenhum opcode que o gerador emite e que seja executável fica sem handler. Os
`UnportedOpcode` restantes só podem vir de handler que devolve `None` por dentro de `run_ext`/`run_misc`
(braço `_ =>` que não reconhece), o que a lista acima exclui para os 190 citados; se um golden ainda
disparar `UnportedOpcode`, o opcode citado em algum handler pode estar só em comentário ou em braço
condicional, e a conferência seguinte é por execução (`cargo test` no golden), que esta passagem não rodou.
