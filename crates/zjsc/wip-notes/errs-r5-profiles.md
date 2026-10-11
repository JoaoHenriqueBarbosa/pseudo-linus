# errs r5: profiles de alocação e varredura de bytecode/*.rs

## object_allocation_profile.rs

`ObjectAllocationRealm` saiu. `initialize_profile(profile, vm, &Rc<JSGlobalObject>, owner, &JSObjectRef, inferred_inline_capacity)`.
Constantes `DEFAULT_INLINE_CAPACITY` (6), `MAX_INLINE_CAPACITY` (64) e `final_object_allocation_size` vivem no módulo
até o `JSFinalObject` ser portado (confirmar os valores contra o JSObject.h).

Chamados e ainda inexistentes (alguém precisa criar):

- `JSGlobalObject::structure_cache()` com `empty_object_structure_for_prototype(&Rc<JSGlobalObject>, &JSObjectRef, inline_capacity, is_poly_proto)` devolvendo algo com `cell_id()`.
- `JSGlobalObject::possible_default_property_count(&JSObjectRef) -> usize`.
- `VM::heap().final_object_allocator_for(allocation_size) -> Option<AllocatorInfo>`.
- `JSGlobalObject::object_prototype()` (já usado por `code_block.rs`).
- `code_block.rs:536` passa `&*global_object`; com a nova assinatura deve passar `&global_object`, e `&object_prototype` precisa ser `JSObjectRef`.

`array_allocation_profile.rs` não usava o realm (só o trait `LastArray`, sobre `HeapRef`), nada a trocar.

## Varredura de imports em bytecode/*.rs (exceto code_block.rs)

Só um módulo inexistente: `crate::runtime::function_overrides` (`FunctionOverrideInfo`, `FunctionOverrides::initialize_override_for`),
usado em `unlinked_function_executable.rs:54,564-567`. Precisa do porte de `runtime/FunctionOverrides` (opção de depuração
`Options::function_overrides`, que existe em `options_list.rs`). Métodos de tipos (não só módulos) não foram conferidos um a um.
