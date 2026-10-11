# Erros de nodes_codegen_cpp2.rs que dependem de outros arquivos

- `bytecompiler/bytecode_generator_part2.rs:41` (`emit_node_expression`, e provavelmente
  `emit_node_in_tail_position_expression`, `emit_node_expression_no_dst`): assinatura ainda em
  `Option<Rc<RefCell<RegisterID>>>`; deve ser `Option<RegisterRef>` (causa de ~40 erros em cpp2:
  141, 164, 447, 717, 738, 756, 794, 806, 822, 870, 871, 890, 915, 933, 952, 1042 a 1358, 1422, 1464,
  1516, 1518, 1547, 1549 e dos `&Rc` em 1105, 1232, 1278).
- `parser/nodes*.rs`: falta `Expression::is_pure(&self, generator) -> bool` (usado em
  nodes_codegen_cpp2.rs:1521 e nodes_codegen_cpp4.rs). Só existe `ResolveNode::is_pure`
  (nodes_codegen_cpp1.rs:341). C++: `ExpressionNode::isPure(BytecodeGenerator&)`, com despacho por variante.
- `bytecompiler/bytecode_generator_cpp6.rs` `emit_is_*` recebem `src: &RegisterRef` (o macro de cpp2 já
  passa `src.as_ref().unwrap()`).
