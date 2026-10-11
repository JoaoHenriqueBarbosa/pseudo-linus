# Erros externos de bytecode_generator_cpp6.rs

Causas cuja correção fica em definições de outros arquivos.

1. `src/bytecompiler/bytecode_generator_base.rs:206` e `:220`: `new_register()` e `new_temporary()` devolvem
   `RegisterIDRef` (`Rc<RefCell<RegisterID>>`), mas o `BytecodeGenerator` inteiro (bytecode_generator.rs:797-829,
   `temp_destination`, `new_temporary_or`) trata o resultado como `RegisterRef` (o `RefPtr<RegisterID>`).
   Correção sugerida: `new_temporary` e `new_register` devolverem `RegisterRef` (`RegisterRef::new(&rc)`), ou um
   `new_temporary` inerente no `BytecodeGenerator` que embrulhe o da base. Resolve a maior parte dos E0308
   "expected RegisterRef, found Rc<RefCell<RegisterID>>" (usos de `new_temporary()` neste arquivo).
2. `src/bytecompiler/bytecode_generator_part3.rs:495` e `:500`: `generator_value_register()` e
   `generator_resume_mode_register()` devolvem `Rc<RefCell<RegisterID>>`; devem devolver `RegisterRef`
   (como `callee_register()` na linha 643 e `this_register()`). Causa dos erros em emit_yield, emit_await, etc.
3. `src/bytecode/bytecode_ops.rs:82-104`: só há `IntoOperand<VirtualRegister>` para `&RegisterIDRef`,
   `RegisterIDRef`, `Option<..>` e `&Option<..>`. Faltam `&RegisterRef`, `RegisterRef`, `Option<RegisterRef>` e
   `&Option<RegisterRef>` (via `register.borrow()`/`get()`). Causa de ~45 erros E0277 `&RegisterRef: IntoOperand`.
4. `src/bytecode/bytecode_ops.rs`: `OpIsCellWithType::emit` espera `u8` no campo `type`; falta
   `impl IntoOperand<u8> for JSType` (cpp2.rs:1264 também passa `JSType` direto) ou o campo deve ser `JSType`.
5. Campos `new_target_register` e afins (`self.new_target_register.clone()`, cpp6 linha ~215) e
   `finally_label`/`completion_type` dos contextos de finally (`FinallyContext`, linhas 1112-1364 do arquivo):
   os tipos de campo e de função em bytecode_generator.rs misturam `Option<Rc<RefCell<RegisterID>>>` e
   `Option<RegisterRef>`. Padronizar tudo em `RegisterRef`.
6. `emit_resolve_scope`/`emit_get_from_scope` (bytecode_generator_cpp3.rs:1050, 1114) já usam `RegisterRef`;
   nada a mudar lá, só dependem do item 1.
