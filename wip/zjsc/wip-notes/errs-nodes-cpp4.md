# Erros de nodes_codegen_cpp4.rs com causa em outros arquivos

- Rótulos: `new_label()` devolve `LabelRef` e `emit_jump`/`push_optional_chain_target_existing` recebem
  `&LabelRef`, mas `BytecodeGenerator::emit_node_in_condition_context` (`bytecode_generator_part2.rs:179`) ainda
  recebe `&mut Label`, e `expression_node_emit_bytecode_in_condition_context` (`nodes_codegen_cpp1.rs:19`) e
  os demais `emit_bytecode_in_condition_context` (cpp3b, cpp4b) usam `&Label`. O C++ passa `Label&` que é o
  rótulo compartilhado; o tipo certo em todo lugar é `&LabelRef`. O cpp4 já foi migrado para `&LabelRef`
  (os quatro `emit_bytecode_in_condition_context`); falta o resto e o despacho.
- `Expression::is_pure(generator)` (e `needs_debug_hook`) são do despacho `impl Expression`, a cargo de outro agente.
- Os erros de `Option<&RegisterRef>` vs `Option<&Rc<..>>` eram da medição anterior à unificação em `RegisterRef`;
  o arquivo já usa `RegisterRef` (via `Cpp4Reg`), nada a mudar.
