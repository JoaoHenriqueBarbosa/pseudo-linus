# Causas externas dos erros de bytecode_generator_cpp5.rs

Lista medida em 2026-10-08. As causas abaixo estão em definições fora do cpp5; o cpp5 segue a convenção
do cabeçalho do arquivo (`RegisterID*` anulável é `Option<RegisterRef>`).

1. `src/bytecompiler/bytecode_generator_base.rs:220` `new_temporary() -> RegisterIDRef` (`Rc<RefCell<RegisterID>>`).
   Todo o resto do gerador (bytecode_generator.rs:801, 829, 846, 852 e as assinaturas de cpp2 a cpp6) espera
   `RegisterRef`. Correção: um `BytecodeGenerator::new_temporary(&mut self) -> RegisterRef` que embrulhe com
   `RegisterRef::new(&base.new_temporary())` (o `RefPtr<RegisterID>`). Responde por cerca de 110 dos erros
   `expected RegisterRef, found Rc<RefCell<RegisterID>>` (e os inversos em `Option`) no cpp5.
2. `src/bytecompiler/bytecode_generator_part2.rs:41` (e `emit_node_in_tail_position_*`, linhas 46 a 80):
   `emit_node_expression` recebe e devolve `Option<Rc<RefCell<RegisterID>>>`; deve ser `Option<RegisterRef>`.
   Atinge cpp5.rs linhas 47, 48, 103, 723 e as chamadas de `emit_node_expression` nos laços.
3. `src/bytecode/bytecode_ops.rs:82-104`: `IntoOperand<VirtualRegister>` só existe para `RegisterIDRef`
   (`Rc<RefCell<RegisterID>>`). Faltam `&RegisterRef`, `RegisterRef`, `Option<RegisterRef>` e
   `&Option<RegisterRef>` (corpo: `VirtualRegister::from_register_id(&self.borrow())`). Responde por cerca de
   35 erros `&RegisterRef: IntoOperand<VirtualRegister>` no cpp5 (linhas 56, 127-137, 205-273, 311, 576-610,
   784-794 etc.).
4. `GeneratorCodeBlock::num_vars` (bytecode_generator_base.rs:37) é método de trait: o cpp5 agora importa o
   trait dentro de `emit_push_function_name_scope`; se o trait ficar em escopo no `bytecode_generator.rs`,
   o `use` local pode sair.
5. Os erros "arguments to this method are incorrect" em `emit_iterator_open`/`emit_iterator_next` (cpp5.rs
   ~1610 e ~1655) são consequência de 1 (os registradores vêm de `new_temporary`).
6. Duplicação (DRY): `cpp2_number_value` (nodes_codegen_cpp2.rs:18) e a closure `number_value` que entrou em
   `end_switch` (cpp5.rs) fazem a mesma coisa; deveria virar um método único (`Expression::number_value`)
   em `parser/nodes.rs`.
