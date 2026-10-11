# Caça ao `cell_id == 0` em `JSObject::as_value` (por leitura, sem cargo)

## Caminho canônico

- `JSObject::new` deixa `cell_id = 0`. Só `JSObject::allocate` (objetos folha, `CellEntry::Object`) e os
  construtores das subclasses fazem `cell_registry::reserve()`, constroem, `set_cell_id` na base e
  `cell_registry::set(.., CellEntry::X)`.
- `JSScope::new` e `JSCallee::new` reservam o id e gravam na base `JSObject` (`base.set_cell_id`); quem
  constrói a subclasse completa com `cell_registry::set` (`JSLexicalEnvironment`, `JSGlobalLexicalEnvironment`,
  `JSModuleEnvironment`, `JSWithScope`, `JSGlobalObject::create`, `JSFunction::create_impl` e as outras duas
  construções de `JSFunction`, `JSCallee::create`).

## Conferido (todos registram antes de qualquer `as_value`)

Todo ponto que chama `JSNonFinalObject::new`, `JSInternalFieldObjectImpl::new`, `JSScope::new`,
`JSCallee::new`, `InternalFunction::construct/new`, `JSArrayBufferView::new` ou `JSSymbolTableObject::*`:
`error_instance`, `date_instance`, `reg_exp_object`, `string_object`, `js_wrapper_object`, `js_promise`,
`js_array_iterator`, `js_string_iterator`, `js_reg_exp_string_iterator`, `js_arguments_objects`,
`js_scoped_arguments`, `js_array_buffer`, `js_data_view`, `js_generic_typed_array_view`, `js_global_proxy`,
`js_module_namespace_object`, `js_async_from_sync_iterator`, `js_weak_object_ref`,
`js_finalization_registry`, `shadow_realm_object`, `js_call_site`, `intl_support`, os oito `temporal_*`,
`proxy_object`, `proxy_revoke`, `internal_function`, as macros de `js_internal_field_object_impl` e
`js_ordered_hash_table` (Map, Set, WeakMap, WeakSet, iteradores, geradores, helpers, disposable stacks),
`JSArray` (via `JSObject::allocate`).

As únicas ocorrências sem registro próprio são classes-base abstratas (`js_array_buffer_view`,
`js_symbol_table_object`, `js_segmented_variable_object`), cujas subclasses registram.
`src/llint`, `src/wasm`, `src/api` e `src/interpreter` não constroem `JSObject` nem subtipo.

## Conclusão

Nenhum construtor sem registro encontrado por leitura estática. Nada foi editado no código. O `debug_assert`
de `JSObject::as_value` continua intacto. Suspeitas que sobram (precisam do backtrace ou de teste):

1. `as_value` chamado dentro de um construtor, entre `JSObject::new` e o `set_cell_id`, por exemplo em
   `finish_creation` ou em construtor de subclasse que consulta o próprio valor antes do registro.
2. Teste ou código de golden que monta `JSObject::new`/`JSNonFinalObject::new` direto (hoje só `js_object.rs`
   chama, e a única ocorrência fora de `allocate` é o construtor de `JSNonFinalObject`).
3. Um objeto vindo de `Clone` de dado interno em vez de `Rc` (não achei).
