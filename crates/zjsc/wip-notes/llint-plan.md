# Plano do interpretador (LLInt/CLoop) até `1 + 1` e `function f(){return 1+1}; f()`

Levantamento de 2026-10-09. Tamanhos em linhas (`grep -c ''`).

## 1. O que o CLoop é e de onde vem

- O CLoop não é C++ escrito à mão: `offlineasm/cloop.rb` (1215 linhas de Ruby) traduz o
  `LowLevelInterpreter*.asm` em C++ (um `switch`/computed goto dentro de `CLoop::execute`, em
  `llint/LowLevelInterpreter.cpp`, 737 linhas, que só define `CLoopRegister`, `PUSH/POP`, `decodeResult`
  e inclui o `LLIntAssembly.h` gerado). Registradores emulados: t0..t7, sp, cfr, lr, pc, pcBase,
  metadataTable, numberTag, notCellMask, d0/d1.
- `derived/JavaScriptCore/` NÃO tem o CLoop pronto. `LLIntAssembly.h` (71070 linhas) é a variante
  RISCV64+JIT (`OFFLINE_ASM_RISCV64`, `!OFFLINE_ASM_C_LOOP`; zero blocos com `C_LOOP` positivo). Serve só
  para conferir offsets: `LLIntDesiredOffsets.h` (4289 linhas) e `LLIntDesiredSettings.h` (5123) têm os
  offsets/settings resolvidos, mas o de CLoop também precisa de `OFFLINE_ASM_C_LOOP=1`.
- Fonte do asm: `LowLevelInterpreter.asm` 3027, `LowLevelInterpreter64.asm` 3735 (so ~900 a 1500 linhas
  delas importam para o alvo). `InPlaceInterpreter*.asm` é Wasm: fora.
- Recomendação: NÃO emular registradores. Interpretador Rust por `match` sobre `OpcodeID`, lendo os
  operandos com `bytecode_ops_decode`, com a semântica dos handlers do `.asm` e dos slow paths
  (CONVENTIONS, item 4). Gerar o CLoop em C só serviria de oráculo de leitura, e exige Ruby no host
  (não verificado).

## 2. Handlers e slow paths de que o alvo depende

Fato importante: `ASTBuilder::makeAddNode` dobra `1 + 1` em `NumberNode(2)`. O bytecode de `1+1` NÃO tem
`op_add`: Program = `op_enter`, (`op_get_scope` se houver), `op_mov`/load_const do 2, `op_end`. `f` =
`op_enter`, `op_ret`. Para exercitar `op_add` de verdade usar `var a = 1; a + 1` (ou `x + 1` com
parâmetro) como caso extra.

Handlers (LowLevelInterpreter64.asm / .asm): `op_enter` (821), `op_get_scope` (880), `op_mov` (915),
`op_end` e `op_ret` (2603), `op_check_traps` (2474), `op_loop_hint` (2453), `op_get_argument` (860),
`op_argument_count` (872), `op_jmp` (2344), `binaryOp(add)` (1323, com `binaryOpCustomStore` 1218; int32
com overflow, doubles, senão slow path `slow_path_add`), `op_call` via `commonCallOp` (2497,
`prepareForRegularCall`/`invokeForRegularCall`/`doCallVarargs` 2551), `op_new_func` e `op_resolve_scope`
(2798)/`op_get_from_scope` (2889)/`op_put_to_scope` (2968) para `f` global, mais `op_get_by_id` se vier.
Prólogo/entrada: `vmEntryToJavaScript`/`doVMEntry`, `functionPrologue`, `codeBlockPrologue`,
`prepareForCall`, `checkStackOverflow` (LowLevelInterpreter.asm ~1000 a 1400).

Slow paths (nomes confirmados): `LLIntSlowPaths.cpp` 2918 linhas, `.h` 173 (106 definições; usar só ~12:
`slow_path_enter`, `slow_path_call`, `slow_path_new_func`, `slow_path_resolve_scope`,
`slow_path_get_from_scope`, `slow_path_put_to_scope`, `slow_path_check_traps`, `slow_path_throw_*`
stack overflow, `entry` para a primeira chamada). `runtime/CommonSlowPaths.cpp` 1726 + `.h` 344 +
`Inlines.h` 268 (`slow_path_add`, `slow_path_to_this`). Metadados/offsets: `LLIntData.cpp` 459 + `.h`
248 (tabelas de opcode e `LLIntOffsets`), `LLIntEntrypoint.cpp` 275 (escolhe o entrypoint do
CodeBlock), `LLIntOffsetsExtractor.cpp` 131, `LLIntExceptions.cpp` 96, `LLIntThunks.cpp` 933 (na maior
parte JIT: pular), `LLIntCLoop.cpp` 44, `LLIntOpcode.h` 83. Wasm/`InPlaceInterpreter`: fora.

## 3. Runtime mínimo (C++ em linhas) e o que já existe em `src/`

| Peça | C++ | Estado em src/ |
|---|---|---|
| JSValue (JSCJSValue.h 1036, Inlines 564) | encode/decode, number tag | `runtime/js_value.rs` 451, `js_string.rs`, `js_type.rs`: feito (puro) |
| CallFrame/Register/ProtoCallFrame/CLoopStack | 423 / ~150 / 100 / 170 | `interpreter/call_frame.rs` 56: só constantes de slot. Falta frame real, Register, ProtoCallFrame, pilha |
| VM (VM.h 1498, VM.cpp 2276) | heap, exceção, stack | `runtime/vm.rs` 279: esqueleto. Falta exceção, topCallFrame, entryScope, small strings |
| Structure (1174+1830), JSCell 334 | forma dos objetos | `runtime/structure.rs` 61: quase nada |
| JSObject (1415+4778), butterfly | propriedades | só `js_cell_butterfly.rs`; falta tudo |
| JSFunction (281+699), JSCallee, Executables (Program 319, Function 221, Script 609) | chamada | `unlinked_function_executable.rs`, `unlinked_code_block.rs` 1042; faltam as células Executable/JSFunction |
| JSGlobalObject (1438+4032), JSScope 429, JSLexicalEnvironment 119 | escopo global | nada |
| CodeBlock (1100+3967) | linkagem do Unlinked, metadata, constantes | `bytecode/code_block.rs` 34 (stub) |
| Heap (1384+3775) | GC | NÃO portar: arena simples (`Vec`/índices, sem coleta) basta para o marco |
| Interpreter.cpp 1866 | `executeProgram`, `executeCall` | `interpreter/interpreter.rs` 16 |

Decisão de projeto a validar (CONVENTIONS proíbe unsafe): células como índices em arena (`CellId`),
frame como janela `&mut [Register]` sobre uma pilha `Vec<EncodedJSValue>`; sem ponteiros crus.

## 4. Fatias (cada uma até 5 min, ~800 linhas de C++; ordem; saída medida contra o bun)

1. `Register` + `ProtoCallFrame` + pilha (CLoopStack) + `CallFrame` real. In: interpreter/Register.h,
   ProtoCallFrame.h, CLoopStack.cpp, CallFrame.h. Out: `interpreter/{register,proto_call_frame,
   cloop_stack}.rs`, `call_frame.rs`. Critério: teste de ida e volta dos slots (header 5, argumentos).
2. JSCell + Structure mínima + arena de células + `JSObject` com butterfly sem propriedades. In:
   JSCell.h, Structure.h/.cpp (só criação e `m_classInfo`), JSObject.h. Out: `runtime/{js_cell,
   structure,js_object}.rs`. Critério: criar objeto vazio e ler o tipo.
3. `JSGlobalObject` mínimo + `JSScope`/`JSLexicalEnvironment` + `JSCallee`/`JSFunction`
   (`createWithInvalidatedReallocationWatchpoint`). Out: `runtime/{js_global_object,js_scope,
   js_function}.rs`. Critério: globalThis com `var`.
4. Executables: `ScriptExecutable`, `ProgramExecutable`, `FunctionExecutable`, `Completion.cpp` (só
   `evaluate`/`checkSyntax` até gerar o UnlinkedCodeBlock). Out: `runtime/{executable,completion}.rs`.
   Critério: `evaluate("1+1")` chega a ter um `UnlinkedProgramCodeBlock` com os mesmos opcodes que
   `bun --print` mostra com `BUN_JSC_dumpBytecode`/`$vm` (conferir se o bun expõe; senão `describe` do
   gerador contra o dump esperado de `bytecode/BytecodeDumper`).
5. `CodeBlock` real (link do Unlinked: constantes, metadata table, `JSCallee` scope register). In:
   CodeBlock.h/.cpp trechos de `finishCreation`/`setConstantRegisters`. Out: `bytecode/code_block.rs`.
6. Interpretador, parte A: laço de despacho + `op_enter`, `op_mov`, `op_end`, `op_get_scope`,
   `op_ret`, `op_check_traps`, `op_loop_hint`, `op_jmp`. In: os handlers acima. Out:
   `llint/{mod,low_level_interpreter}.rs`. Critério: `1 + 1` retorna o JSValue int32 2.
7. `LLIntData`/`LLIntEntrypoint`/`LLIntOffsets` (só o necessário: opcode table, entrypoint do
   CodeBlock) + `Interpreter::executeProgram` + `VM::entryScope`. Out: `llint/llint_data.rs`,
   `interpreter/interpreter.rs`. Critério: `jsc_eval("1+1")` == bun `1+1` impresso como `2`.
8. Aritmética: `binaryOp(add/sub/mul)`, `op_to_this`, `op_get_argument`, `op_argument_count` +
   `slow_path_add` e `jsValueAdd` de `runtime/Operations.h`. Critério: golden `arith.tsv` com ~50
   expressões (`a+1`, overflow int32, doubles, `1+"2"`) contra o bun.
9. Chamada: `op_new_func`, `op_resolve_scope`, `op_get_from_scope`, `op_put_to_scope`, `op_call`,
   `prepareForRegularCall`/`invokeForRegularCall`, `slow_path_call`/`entry`. Critério:
   `function f(){return 1+1}; f()` == `2`.
10. Exceção mínima: `throw`/`op_throw`, `LLIntExceptions`, `ThrowScope` e `stack overflow`. Critério:
    `throw 1` e recursão infinita dão o mesmo `RangeError` do bun (mensagem idêntica).
11. Golden final `tests/golden/eval-basic.tsv`: entrada JS, saída `bun -e 'console.log(JSON.stringify(eval(...)))'`;
    teste `eval_golden`. Meta do marco: 100% das linhas.

Dependência dura antes da fatia 4: o gerador precisa estar compilando (módulos `bytecompiler/*` ainda
fora da compilação). Fatias 1 a 3 independem dele e podem rodar já.
