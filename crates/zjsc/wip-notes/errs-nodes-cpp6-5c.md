# Erros de nodes_codegen_cpp6 e nodes_codegen_cpp5c: o que ficou fora dos dois arquivos

- `TryNode::emit_bytecode` (cpp5c): `emit_push_catch_scope`/`emit_pop_catch_scope` pedem
  `&mut VariableEnvironment`, mas o nó chega por `&self` (no C++ o `lexicalVariables()` muta num `this`
  const). Por ora o uso é uma cópia (`lexical_variables.clone()`), o que perde o efeito de
  `mark_all_variables_as_captured` sobre o nó. Conserto fiel: `emit_push_catch_scope`,
  `emit_pop_catch_scope`, `push_lexical_scope_internal` e `pop_lexical_scope_internal` (cpp5/cpp3 do
  gerador) passarem a receber a `VariableEnvironment` por referência com mutação interior, ou o campo
  `VariableEnvironmentNode::lexical_variables` virar `RefCell`.
- `ScopeType` existe em dois lugares (`runtime::symbol_table` sem `LetConstScope`/`ClassScope`, e
  `bytecompiler::bytecode_generator` com eles). Os dois arquivos usam agora o do gerador, que é o que
  `push_lexical_scope`/`emit_push_catch_scope` recebem.
- `emit_profile_control_flow(scope_node.start_start_offset())` (cpp5c, ~467) mistura `u32` com o `i32` da
  assinatura; não apareceu na medição, conferir na próxima.
