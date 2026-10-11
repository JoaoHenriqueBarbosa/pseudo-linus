# Erros de nodes_codegen_cpp4b.rs com causa em outro arquivo

1. `bytecode_generator_part2.rs:115-120`, `:249-262`, `:268-282` (`emit_node_expression_no_dst`,
   `emit_node_for_left_hand_side`, `emit_node_for_left_hand_side_for_property`): ainda devolvem
   `Option<Rc<RefCell<RegisterID>>>`; devem devolver `Option<RegisterRef>` (e `new_temporary` já é `RegisterRef`,
   então `Some(dst.clone())` basta). Causa da maioria dos ~60 erros de `Option<Rc>` vs `Option<RegisterRef>`
   (cpp4b linhas 78, 88, 346, 403 a 430, 450, 473, 489, 565 a 579, 725, 777, 780, 849, 989, 999 e afins).
2. `bytecode_generator_part2.rs:325-337` `emit_unary_op`: dst/src/retorno em `Option<Rc<..>>`, trocar por
   `Option<RegisterRef>` (cpp4b:90). O mesmo vale para os `emit_binary_op`/`emit_equality_op` da part2 (cpp4b:585-700).
3. `Expression::is_pure(&mut BytecodeGenerator) -> bool` não existe (só `ResolveNode::is_pure` em
   `nodes_codegen_cpp1.rs:341`). Falta o despacho do `ExpressionNode::isPure` por variante (default false, Resolve usa
   o de `ResolveNode`, literais e afins true). Usado também em cpp4, cpp3b. Erros em cpp4b:123, 242, 442, 467, 503,
   556, 557, 605, 606, 664, 665.
4. `bytecode_generator_part2.rs:182-183` `emit_node_in_condition_context` recebe `&mut Label`; o C++ recebe
   `Label&` mas os chamadores (cpp4b:732 e `IfElseNode`) passam `LabelRef`. Trocar para `&LabelRef` (labels são
   `GenericLabelRef` vindos de `new_label()`), e `emit_bytecode_in_condition_context` idem.
5. `bytecode_generator.rs` `temp_destination`, `emit_put_by_id`, `emit_put_by_val*`, `emit_get_by_id`, `emit_require_object_coercible`,
   `emit_to_property_key_or_number`, `ensure_this`: conferir que usam `RegisterRef` (erros 748, 984 "arguments to this
   method are incorrect").

Corrigido neste arquivo: `short_circuit_assignment` recebe `&LabelRef`; `ScopeType::LetConstScope` virou
`LexicalScope` (o enum de `symbol_table.rs` não tem `LetConstScope`).
