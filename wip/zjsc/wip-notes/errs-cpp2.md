# Causas externas dos erros de bytecode_generator_cpp2.rs

1. **Duas representações de registrador** (cerca de 90 dos 144 erros). `bytecode_generator_base.rs:220/238`
   (`new_temporary`, `add_var`, `new_register`, `new_temporaries`) devolvem `RegisterIDRef`
   (`Rc<RefCell<RegisterID>>`), enquanto o gerador inteiro usa `RegisterRef` (struct com contagem intrusiva).
   Correção sugerida: `_base` passar a devolver `RegisterRef` (como já decide `bytecode-generator-duplicates.md`),
   e `BytecodeGenerator::parameters`, `callee_register`, `this_register`, `register_for` idem.
   Afeta cpp2 em: 93, 180, 317, 329-337, 365, 509, 565, 603, 939, 973, 993, 996, 1055, 1109-1172, 1201, 1206.
2. **`IntoOperand<VirtualRegister>` falta para `RegisterRef`** (`bytecode/bytecode_ops.rs:82-104` só cobre
   `RegisterIDRef`). Correção: impls para `RegisterRef`, `&RegisterRef`, `Option<RegisterRef>`,
   `&Option<RegisterRef>` e `Option<&RegisterRef>` (usando `RegisterRef::borrow()`). Cobre os ~60 erros
   `Option<RegisterRef>: IntoOperand<VirtualRegister>` (cpp2:559-1319).
3. `RegisterID::set_index_virtual` e `BytecodeGenerator::new_parameter_register` não existem (cpp2:503, 506);
   o C++ (`BytecodeGenerator.cpp:1382`) faz `m_parameters.grow` e `registerFor(reg)`. Falta o
   `register_for` devolver o elemento de `parameters` e um setter de índice virtual em `RegisterID`.
4. `ScopeNode::captures(uid)` não existe (o C++ é `m_varDeclarations.captures(uid)`); usei
   `var_declarations.captures` direto, sem precisar de ajuste externo.
5. `JSType: IntoOperand<u8>` (cpp2:1262, 1267): o impl existe em `bytecode_ops.rs:113`; se o erro persistir, o
   campo do `OpIsCellWithType` foi gerado com outro tipo (conferir o `type_` no `bytecode_ops`).
6. `InstructionStreamWriter::len` não existe: usei `position()` (cpp2 `emit_type_profiler_expression_info`).
