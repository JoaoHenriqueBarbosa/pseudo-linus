# Opcodes do laço do LLInt (inventário de 2026-10-09)

Fonte da lista: `bytecode/bytecode_dumper.rs` (todos os `OpcodeID` do `BytecodeList.rb`, 179 ops). Braços
conferidos em `llint/dispatch.rs`, `llint/dispatch_ext.rs` e `llint/varargs.rs`.

## Com braço antes desta fatia

op_add, op_bitand, op_bitnot, op_bitor, op_bitxor, op_call, op_call_direct_eval, op_call_ignore_result,
op_call_varargs, op_catch, op_check_traps, op_construct, op_construct_varargs, op_create_this, op_dec,
op_del_by_id, op_del_by_val, op_div, op_enter, op_eq_null, op_get_by_id, op_get_by_val, op_get_from_scope,
op_get_length, op_get_scope, op_in_by_id, op_inc, op_instanceof, op_is_boolean, op_is_callable, op_is_empty,
op_is_number, op_is_object, op_is_undefined_or_null, op_jeq_null, op_jfalse, op_jmp, op_jneq_null,
op_jnundefined_or_null, op_jtrue, op_jundefined_or_null, op_loop_hint, op_lshift, op_mod, op_mov, op_mul,
op_negate, op_neq_null, op_new_array, op_new_async_func, op_new_async_func_exp, op_new_async_generator_func,
op_new_async_generator_func_exp, op_new_func, op_new_func_exp, op_new_generator_func,
op_new_generator_func_exp, op_new_object, op_nop, op_not, op_pow, op_put_by_id, op_put_by_val, op_put_to_scope,
op_resolve_scope, op_ret, op_rshift, op_sub, op_super_construct, op_super_construct_varargs, op_switch_char,
op_switch_imm, op_switch_string, op_tail_call, op_tail_call_varargs, op_throw, op_to_number, op_to_numeric,
op_to_string, op_to_this, op_typeof, op_typeof_is_function, op_typeof_is_object, op_typeof_is_undefined,
op_unsigned, op_urshift.

## Portados nesta fatia (ainda sem a linha de encaminhamento no laço)

`llint/handlers_misc.rs` (`run_misc`): op_eq, op_neq, op_stricteq, op_nstricteq, op_less, op_lesseq, op_greater,
op_greatereq, op_below, op_beloweq, op_jless, op_jnless, op_jgreater, op_jngreater, op_jlesseq, op_jnlesseq,
op_jgreatereq, op_jngreatereq, op_jeq, op_jneq, op_jstricteq, op_jnstricteq, op_jbelow, op_jbeloweq,
op_jeq_ptr, op_jneq_ptr, op_is_big_int, op_is_constructor, op_is_cell_with_type, op_to_primitive,
op_to_property_key, op_to_property_key_or_number, op_to_object (parcial), op_strcat, op_argument_count,
op_get_argument, op_check_tdz, op_super_sampler_begin, op_super_sampler_end, op_identity_with_profile,
op_profile_type, op_profile_control_flow, op_log_shadow_chicken_prologue, op_log_shadow_chicken_tail, op_debug.

`llint/handlers_object.rs` (`run_object`): op_get_internal_field, op_put_internal_field (geradores),
op_get_prototype_of (só `JSObject`), op_get_by_id_direct, op_in_by_val, op_define_data_property,
op_define_accessor_property, op_new_array_with_size, op_throw_static_error.

`llint/handlers_scope.rs` (`run_scope`): op_get_parent_scope, op_push_with_scope, op_create_lexical_environment,
op_resolve_scope_for_hoisting_func_decl_in_eval, op_create_rest, op_create_generator, op_create_async_generator,
op_new_generator, op_new_async_function_generator.

## Portados na segunda fatia (já encadeados no `_ =>` de `run_ext`)

`llint/handlers_accessor.rs` (`run_accessor`): op_put_getter_by_id, op_put_setter_by_id, op_put_getter_setter_by_id,
op_put_getter_by_val, op_put_setter_by_val (getter ou setter `JSFunction` entra: o `GetterSetter` guarda `ObjectRef`),
op_set_function_name, op_new_reg_exp, op_put_by_val_direct, op_get_by_id_with_this,
op_get_by_val_with_this, op_put_by_id_with_this, op_put_by_val_with_this.

`llint/handlers_arguments.rs` (`run_arguments`): op_create_direct_arguments, op_create_cloned_arguments,
op_create_scoped_arguments, op_get_from_arguments, op_put_to_arguments (cells em `runtime/js_arguments_objects.rs` e
`runtime/js_scoped_arguments.rs`; índices mapeados em `runtime/generic_arguments.rs`, despachados por `exotic_of`
no começo de `get_own_property_slot_by_index`, `put_by_index`, `delete_property_by_index`, `define_own_property` e
`own_property_names`).

`llint/handlers_enumerator.rs` (`run_enumerator`): op_get_property_enumerator, op_enumerator_next,
op_enumerator_get_by_val, op_enumerator_in_by_val, op_enumerator_has_own_property, op_enumerator_put_by_val
(cell em `runtime/js_property_name_enumerator.rs`, só `GenericMode`; base primitiva por `JSValue::to_object`).

`llint/handlers_iterator.rs` (`run_iterator`): op_iterator_open, op_iterator_next (todos os modos de
`getIterationMode`: `FastArray`, `FastMap`, `FastSet`, `FastString` e os de iterador já aberto
`FastArray*`/`FastMap*`/`FastSet*`; as nove sentinelas em `VM`, `canUseFastIterationMode` sobre o `seenModes` do
metadata; sem watchpoint, a validade é a conferência direta do `next` do protótipo do iterador, ver
`runtime/iteration_protocol.rs`; o `CHECK_EXCEPTION` do `JSArrayIterator::next` é a exceção pendente no `VM`; não
confere a ausência de `return` na cadeia nem faz `PROFILE_VALUE_IN`), op_spread, op_new_array_with_spread.

`llint/handlers_async.rs` (`run_async`): op_async_iterator_open (`AsyncFromSync` sem `@@asyncIterator`,
`FastAsyncGenerator` para gerador assíncrono primordial, e o genérico sem `validateIterable`),
op_async_iterator_next (sentinela roda `asyncIteratorNextWithDriver`; senão `next.call`), op_new_promise,
op_create_promise (callee `JSFunction` subclasse é `Unported`). `createAsyncFromSyncIteratorForIterable` está em
`runtime/iterator_operations.rs` com `getIterationMode` e `fastSyncIteratorForIterable` (modos rápidos do invólucro).

`llint/handlers_array.rs` (`run_array`): op_new_array_buffer (copia, sem copy-on-write), op_new_array_with_species.

`llint/handlers_object.rs`: também op_has_private_name, op_get_private_name, op_put_private_name e
op_has_structure_with_flags.

## Sem handler, com o que falta

| Opcode | Falta |
|---|---|
| op_has_private_brand, op_check_private_brand, op_set_private_brand | `BrandedStructure` e `Structure::setBrandTransition` |
| op_yield, op_create_generator_frame_environment | `notSupported()` no `.asm`: a `BytecodeGeneratorification` os reescreve antes de executar |
| op_unreachable | `crash()` no C++ |
| op_wide16, op_wide32 | prefixos de largura: o decoder do fluxo os absorve, não são instruções do laço |

Desvios e lacunas dos que entraram estão no cabeçalho de cada `handlers_*.rs`.
