# Divergências cpp5 / cpp6 contra BytecodeGenerator.cpp

Conferência de `bytecode_generator_cpp5.rs` (só leitura) e `bytecode_generator_cpp6.rs` (corrigido no local).
Arquivo:linha vale para o estado de 2026-10-08.

## Já corrigido em cpp6

- `emit_throw_type_error_str` (inexistente) virou `emit_throw_type_error`; a variante com `Identifier` usa `emit_throw_type_error_identifier`.
- `emit_debug_hook_position` (só existia em comentário no part3) virou `emit_debug_hook` (a sobrecarga de posição está em cpp5:272).
- `emit_throw`/`emit_return`/`emit_call` (func)/`emit_get_generic_async_iterator`/`emit_async_iterator_next`/`emit_put_internal_field` recebem `Option<RegisterRef>`, como definidos em cpp5/cpp4.
- Rótulos: `LabelRef` em `TryRange`, `TryContext`, `register_jump`, `optional_chain_target_stack`; `bind_generator(&label)` no lugar de `bind_generator(self)`; `OpNop::emit` no lugar de `emit_narrow`.

## A corrigir fora de cpp6

1. `r#move` não existe em lugar nenhum: cpp5.rs:59,79,106,240,548,557,1094,1108,1219,1275,1277,1291,1354,1403,1591,1632 e part2.rs:479,481. O que existe é `move_register(Option<&RegisterRef>, &RegisterRef)` (bytecode_generator.rs).
2. `emit_call_varargs::<ConstructOp::VarArgs>` em cpp5.rs:61 e 82: cpp4.rs:1830 define `emit_call_varargs` sem genérico (e `emit_call_varargs_op` com `OpcodeID`). `ConstructOpcode`/`VarArgs` não existem em bytecode_ops.rs.
3. `new_label_scope` devolve `Rc<LabelScope>` (bytecode_generator.rs:847), mas cpp5.rs:1501 e 1611 usam `scope.borrow()` como `Rc<RefCell<..>>` (part3.rs também declara `label_scopes: Vec<Rc<RefCell<LabelScope>>>`).
4. cpp2.rs:637, 704, 769: `emit_jump`/`emit_jump_if_true`/`emit_jump_if_false` recebem `&Label`; todos os chamadores (cpp1, cpp5, cpp6) passam `&LabelRef`. Devem receber `&LabelRef`. As 15 chamadas `target.bind_generator(self)` em cpp2.rs (638, 674, 696, 764, 833..881) e cpp4.rs (1553, 1579, 1610) não compilam: `LabelRef` não tem `bind_generator`, e `GenericLabel::bind_generator` pede o `&LabelRef` como argumento.
5. label.rs:154: `bind_generator(&mut self, label)` chama `label.clone()`, que faz `borrow_mut()` no mesmo `RefCell` já emprestado por `target.borrow_mut()`: pânico `BorrowMutError` em rótulo forward. Precisa de função associada em `GenericLabelRef` que não segure o empréstimo ao clonar.
6. Tipos antigos `Rc<Label>` em bytecode_generator.rs:252 (`FinallyJump.target_label`), :286 (`FinallyContext::finally_label`), :318 (`register_jump`), :470-478 (`TryContext`, `TryRange`) e part3.rs:178 (`optional_chain_target_stack`): cpp5 e cpp6 usam `LabelRef`. Trocar tudo para `LabelRef`.
7. part3.rs:138 `callee_register: RegisterID` (valor), mas cpp1.rs:612 e cpp6 usam como `RegisterRef` (`.borrow_mut()`, `.clone()`). Idem `this_register`/`ignored_result_register` (part3.rs:136-137) contra `Rc::ptr_eq` em bytecode_generator.rs.
8. Convenção dividida de parâmetro não anulável (`&RegisterRef` x `Option<RegisterRef>`) nas funções de cpp6 `emit_await` (src), `emit_is_object` (src), `emit_iterator_open`/`emit_iterator_next`/`emit_iterator_generic_close` (todos os registradores): cpp5.rs:1301, 1369, 1524, 1545, 1549, 1571, 1592, 1634, 1656, 1669 e nodes_codegen_cpp2.rs:417, 1259 passam `Option`; nodes_codegen_cpp3b/4/6/7 passam `&RegisterRef`. cpp6 ficou com `&RegisterRef` (maioria dos chamadores); cpp5 e os dois de nodes_codegen_cpp2 precisam de `.as_ref().unwrap()`. Falta decisão única.
9. cpp6 depende de itens inexistentes: `crate::bytecode::bytecode_use_def::compute_defs_for_bytecode_index` (cpp6.rs, `ForInContext::finalize`) e `OpJmp::SIZE`/`OpJneqPtr::SIZE` (o `static_assert(sizeof(OpJmp) <= sizeof(OpJneqPtr))`). Faltam portar o `BytecodeUseDef` e expor o tamanho nos ops.
10. `pop_for_in_scope` (cpp6) faz `self.code_block.clone()` para satisfazer o empréstimo de `finalize(&mut generator, &code_block)`; o C++ passa `m_codeBlock.get()` sem cópia. `code_block` é `Box<UnlinkedCodeBlockGenerator>` em part3.rs:120: precisa de `Rc<RefCell<..>>` ou de um `take`/restaura em torno da chamada.
