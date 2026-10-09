# Erros de cpp1 que dependem de definição em outro arquivo

1. `bytecode_generator_part3.rs:98` (`AsyncFuncParametersTryCatchInfo`): falta `#[derive(Clone)]`
   (`Option<..>::clone()` em cpp1: `async_func_parameters_try_catch_wrap` e `generate`). Todos os campos já são `Clone`.
2. `bytecode_generator.rs:515` (`TryRange`): falta `#[derive(Clone)]` (`self.try_ranges.clone()` em `generate`).
3. `bytecode_generator_part3.rs:603` (`emit_to_this_this_register`): embrulha `self.this_register.clone()` num `Rc<RefCell<..>>` novo,
   o que copia o `RegisterID` em vez de apontar para ele. Deve ser `let r = self.this_register(); self.emit_to_this(&r);`
   (`this_register()` devolve `RegisterRef`). cpp1 chama `emit_to_this_this_register()` (o `emitToThis()` sem argumento do .h).
4. `runtime/var_offset.rs`: sem `dump`. cpp1 usa `format!("{:?}", offset)` em `Variable::dump` (só depuração); se quiser fiel,
   portar `VarOffset::dump` (`invalid` / ScopeOffset / VirtualRegister / DirectArgumentsOffset).
5. `bytecompiler/bytecode_generator_part3.rs`: `is_private_builtin_function` é método do ramo `#else` de `USE(BUN_JSC_ADDITIONS)`;
   cpp1 deixou de atribuir o campo (bloco BUN removido do construtor de função).
6. `FinallyContext`: sem construtor `with_outer`; cpp1 monta a struct direto (campos públicos).
7. `ScopeNode`: `function_stack`/`lexical_variables` vivem em `base.variable_environment`, `var_declarations` em `base`;
   `FunctionNode.parameters` é `Option<NodeRef<FunctionParameters>>` e `FunctionParameters` não tem `size()`/`at()`/`is_simple_parameter_list()`
   (cpp1 usa `patterns.len()`, `patterns[i]`, campo). Se outros arquivos usarem os acessores do C++, aplicar o mesmo.
