# Causas externas dos erros de bytecode_generator_cpp3.rs

1. bytecode/bytecode_ops.rs:82-102: faltam `impl IntoOperand<VirtualRegister>` para `&RegisterRef`,
   `Option<&RegisterRef>` e `&Option<RegisterRef>` (hoje só existem para `RegisterIDRef`). Causa de cerca de
   90 dos erros do arquivo (todo `Op*::emit(self, &killed, base.as_ref().unwrap(), ...)`). Correção:
   `impl IntoOperand<VirtualRegister> for &RegisterRef { fn into_operand(self) -> VirtualRegister { self.borrow().virtual_register() } }`
   e as variantes de Option (None vira `VirtualRegister::default()`).
2. bytecompiler/bytecode_generator_base.rs:206/220/238: `new_register`, `new_temporary`, `add_var` devolvem
   `RegisterIDRef`, mas todo o resto (cpp2, cpp3, bytecode_generator.rs:797-830) espera `RegisterRef`
   (`RefPtr<RegisterID>`). Correção sugerida: a base devolver `RegisterRef` (`RegisterRef::new(&rc)`).
   Isso resolve os "expected RegisterRef, found Rc<RefCell<RegisterID>>" em cpp3 (temp em
   initialize_block_scoped_functions, hoist_sloppy_mode_function_if_necessary, emit_resolve_scope_for_hoisting...).
3. bytecompiler/bytecode_generator_part3.rs:623 `kill` e part2.rs:648 `emit_is_undefined` ainda usam
   `Rc<RefCell<RegisterID>>`; trocar por `RegisterRef` (cpp3:676, 642-643). Idem
   `bytecode_generator_part3.rs:650 register_for` e `constant_pool_registers` (Vec<RegisterIDRef>), cuja
   leitura em cpp2:938/975/989 devolve `Rc` onde se espera `RegisterRef`.
4. parser/nodes_accessors.rs:31: falta `as_comma_node => Comma(CommaNode)` (cpp3 usa match direto, então não bloqueia).
5. parser/nodes.rs:346 `VariableEnvironmentNode` guarda `lexical_variables` e `function_stack` sem
   RefCell, e os callers passam `&node`. O `mark_all_variables_as_captured` do C++ muta o ambiente do nó;
   em cpp3 push/pop/prepare trabalham numa CÓPIA (idempotente, refeita no pop). Se algum leitor depois
   precisar ver a marcação persistente, tornar o campo `RefCell<VariableEnvironment>`.
6. bytecompiler/bytecode_generator.rs:88 `ThisResolutionType` só tem `Local`/`Scoped` (igual ao C++);
   o cpp3 usava `Global` inexistente, corrigido para o default `Local` do C++.
