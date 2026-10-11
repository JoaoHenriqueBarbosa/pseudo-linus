# e2e: var, let/const e função lançam exceção (análise estática, 2026-10-08)

## Causa provável (única para os dois sintomas)

O gerador de bytecode (`allocate_scope`, espelho de `BytecodeGenerator::allocateScope`) só reserva o
`scopeRegister` e o grava em `CodeBlock::scope_register`. Quem põe o escopo do callee nesse registrador
é o `op_enter` (o `.asm` do LLInt e o JIT: `emitGetScope(m_profiledCodeBlock->scopeRegister())`). O
`op_enter` de `src/llint/dispatch.rs` só zerava os locais com `undefined`, então o registrador ficava
`undefined`:

- `scope_operand` (`slow_paths_object.rs`, usado por `resolve_scope`, `put_to_scope`, `get_from_scope`)
  devolve `Unported("registrador de escopo sem escopo no registro de células")`, que
  `value_or_pending_exception` vira um `Error` lançado: é a "exceção sem mensagem" de `var x = 1`,
  `let/const` e `f()` (todos começam por `resolve_scope` ou `new_func` com o escopo do topo).
- `scope_in_register` (`slow_paths_control.rs:96`, usado por `new_func`/`new_func_exp`/
  `create_lexical_environment`) faz `expect` e dá o panic de `try { function f(){} f() }`.

O global object, o ambiente lexical global e o `global_callee` estão certos: `global_scope()` é o
`JSGlobalObject.global_lexical_environment` (registrado como `CellEntry::Scope` em
`js_global_lexical_environment.rs:53`) e é o escopo do `global_callee`/`eval_callee`/`zombie_frame_callee`.

## Correção feita

- `src/llint/dispatch.rs`, `op_enter`: depois de zerar os locais, se `block.scope_register()` é válido,
  grava nele `callee.scope()` (o mesmo cálculo de `op_get_scope`).

## Diagnóstico para a próxima rodada

- `src/api/eval.rs`: nova `describe_exception(&JSValue) -> String` (`Nome: mensagem` de um `ErrorInstance`).
- `tests/e2e_numeric_golden.rs`: a falha agora imprime `lançou exceção (Nome: mensagem)`.

## Se ainda falhar depois disso (não verificado, sem build)

1. `resolve_scope` do programa: conferir `JSScope::resolve` contra `JSScope::resolve` do C++ para o
   `GlobalLexicalEnvironment` e se `ProgramExecutable::initialize_global_properties` declara as `var` no
   global object (`JSGlobalObject::addVar`/`addFunction`, `GlobalPropertyInfo`), senão `put_to_scope` de
   `var x` em `GlobalVar` não acha o slot.
2. `let/const` globais: `put_to_scope` com `GlobalLexicalVar` precisa do `SymbolTable` do
   `JSGlobalLexicalEnvironment` populado em `initialize_global_properties` (TDZ via `op_mov` de empty).
3. `f()` no programa: `op_new_func` + `op_call`; dentro de `f`, o `op_enter` agora grava o escopo do
   `JSFunction` (callee), confira que `JSFunction::create` guarda o escopo recebido.
