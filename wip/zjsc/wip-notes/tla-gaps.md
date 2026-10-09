# Top-level await em módulo: o que falta no LLInt

Conferido contra `BytecodeGenerator.cpp` (construtor do `ModuleProgramNode`, `emitAwait`, `emitYieldPoint`,
`emitGeneratorStateChange`), `BytecodeGeneratorification.cpp` e o porte em `src/bytecompiler`, `src/llint`,
`src/interpreter/execute_module_program.rs` e `src/runtime/js_microtask.rs`.

No JSC não existe `op_await`. O TLA vira gerador: `await` é `op_yield` com razão `Await`, o passo de
generatorification o troca por salvar locais (`op_put_to_scope`) mais `op_ret`, e a retomada é
`op_switch_imm` sobre o estado mais `op_get_from_scope`. O porte do gerador de bytecode segue o C++
(inclusive a ausência do marcador `op_create_generator_frame_environment` nos módulos: os locais salvos vão
para o próprio `JSModuleEnvironment`, cuja tabela é clonada depois da geração, como no C++).

## Falta (bloqueia o TLA de ponta a ponta)

Nada conhecido por leitura (2026-10-08). O item que existia, `op_get_internal_field` e `op_put_internal_field`
sobre o registro de módulo, está resolvido: `impl InternalFields for AbstractModuleRecord`
(`abstract_module_record.rs`) e o braço `CellEntry::ModuleRecord` em `CellEntry::internal_fields`
(`cell_registry.rs`); `register_module_record` insere o registro como célula.

## Conferido por leitura contra o C++ (js_module_record.rs, js_microtask.rs)

- `execute` (HasTLA com capability, `asyncCapability`), `execute_async` (promessa mais
  `AsyncModuleExecutionDone`), `async_execution_fulfilled`/`rejected` (worklist, ordenação por
  `AsyncEvaluationOrder`), `gather_available_ancestors` (pendentes por contagem), `inner_module_evaluation`
  (passos 10 a 17 da spec com TLA, ciclo e `CycleRoot`), `async_module_resolve_evaluation` e
  `AsyncModuleExecutionResume`. Sem divergência encontrada. Os fontes `CyclicModuleRecord.cpp` não estão em
  `upstream/` com esse nome; a conferência foi contra a spec citada nos comentários e o porte existente.
- Os casos de TLA do `tests/golden/module_bun.tsv` (ordem, ciclo, erro) não foram rodados (regra: sem cargo).

## Conferido e presente

- `op_enter`, `op_get_scope`, `op_mov`, `op_ret`, `op_jtrue`, `op_throw`, `op_switch_imm` (`dispatch.rs`,
  `dispatch_ext.rs`).
- `op_stricteq`, `op_debug` (`WillAwait`/`DidAwait`, sem efeito), `op_check_tdz`, `op_get_argument`
  (`handlers_misc.rs`).
- `op_create_lexical_environment` (`handlers_scope.rs`); `op_put_to_scope` com `ResolvedClosureVar` aceita
  `JSModuleEnvironment` (`slow_paths_object.rs`).
- `op_yield` sem handler é correto: o `.asm` o trata como `notSupported()` e a generatorification o reescreve
  antes da execução (`op_create_generator_frame_environment` idem, `crash()`).
- Driver: `execute_module_program` passa `record, state, sentValue, resumeMode, scope`;
  `async_module_resolve_evaluation` e `AsyncModuleExecutionResume` existem em `js_microtask.rs`.

## Sem conferir

- `op_get_from_scope` com `ResolvedClosureVar` lê por nome (`scope_get_property`) e não pelo offset do
  operando, como o C++; funciona se o nome gerado pela generatorification estiver na tabela clonada do
  ambiente, o que não foi exercitado.
- Se o `JSModuleEnvironment` é criado com `scopeSize()` da tabela clonada já incluindo os slots de locais
  salvos (a ordem geração, clone, criação do ambiente parece igual ao C++, mas não foi executada).
- A liveness (`bytecode_liveness_analysis.rs`) sobre código de módulo com `await` dentro de `try/finally`,
  laços e `for await`.

## Conferido em 2026-10-08 (segunda passada)

- `uses_await` do `ModuleProgramNode` (Parser.cpp:337): `ASTBuilder::usesAwait` aparece nos quatro pontos do
  C++ (`createAssignResolve` e `createBindingLocation` com `AwaitUsingDeclarationStatement`, `createAwait`,
  `createForOfLoop` com for-await, em `ast_builder.rs` e `ast_builder_part2.rs`), e `setUsesAwait` nos três do
  `Parser.cpp` (`parseAwaitExpression`, for-await, `await using`, em `parser_cpp7.rs`, `parser_cpp2.rs`,
  `parser_cpp3.rs`). `module_analyzer.rs` copia `node.uses_await` para `set_has_tla`. Sem divergência.
