# Erros de nodes_codegen_cpp3b.rs (causas fora do arquivo)

Conferido contra o modelo novo (`register-model.md`): o corpo de `nodes_codegen_cpp3b.rs` já usa
`Option<RegisterRef>`/`&RegisterRef` de forma coerente com as definições de cpp2..cpp6 (`temp_destination`,
`final_destination`, `move_register`, `emit_to_numeric`, `emit_load_js_value`, `emit_call_in_tail_position`,
`emit_binary_op_dynamic`, `emit_to_property_key_or_number` etc.). Nenhum uso a corrigir no arquivo. Os erros
restantes vêm de duas causas em outros arquivos.

## 1. `bytecode_generator_part2.rs` ainda declara `Rc<RefCell<RegisterID>>` (maioria dos ~100 erros)

Causa: as assinaturas reais (não comentário) de `part2` usam `std::rc::Rc<std::cell::RefCell<...RegisterID>>`
no lugar de `RegisterRef`. Correção: trocar por `crate::bytecompiler::register_id::RegisterRef` em todas as
assinaturas e corpos. Afetam este arquivo:

- `bytecode_generator_part2.rs:41-45` `emit_node_expression` (a mais usada: linhas 121, 223, 273, 276, 286, 1113-1150 e 1297-1330 daqui)
- `:57-61`, `:70-74`, `:99-103`, `:123-126` (`emit_node_in_tail_position_*`)
- `:174` `emit_node_in_condition_context`, `:254` e `:270` (`emit_node_for_left_hand_side`, `emit_node_for_property`)
- `:327-329`, `:345-349` (`emit_unary_op`/`emit_binary_op` genéricos), `:377-380` `emit_equality_op<E>` (usado em cpp3b 183/191/543/855 etc.), `:605-614`
- `:462-467`, `:134-138` e demais `Option<Rc<..>>`/`&Rc<..>` (varrer com
  `grep -n 'std::rc::Rc<std::cell::RefCell<crate::bytecompiler::register_id::RegisterID>>' src/bytecompiler/bytecode_generator_part2.rs`).

Também `bytecode_generator_part3.rs:159/161` (`parameters`, `constant_pool_registers`): são armazenamento,
devem continuar `Rc` (ver register-model.md).

## 2. `Expression::is_pure` não existe

`nodes_codegen_cpp3b.rs:401, 782, 1272, 1319` (e cpp4.rs:30/50/64/86/147, cpp4b.rs:123/242/442/467/503/556/557,
cpp1b.rs:827) chamam `expr.is_pure(generator)`. Só existe `ResolveNode::is_pure` em `nodes_codegen_cpp1.rs:341`.
No C++ `ExpressionNode::isPure(BytecodeGenerator&)` é virtual (padrão `false`; sobrescrito em ResolveNode,
NumberNode, StringNode, BooleanNode, NullNode, ThisNode e demais conforme `Nodes.h`/`NodesCodegen.cpp`).
Correção: criar `impl Expression { pub fn is_pure(&self, generator: &mut BytecodeGenerator) -> bool }` em
`nodes_codegen_cpp1.rs` (ao lado de `ResolveNode::is_pure`), despachando por variante como o C++.
