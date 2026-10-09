# Elos que faltam para `evaluate_script("1 + 1") == 2`

Caminho: `src/api/eval.rs` (`evaluate_script`). Chega até o `UnlinkedProgramCodeBlock`; daí em diante faltam:

1. `src/runtime/js_global_object.rs`: `JSGlobalObject::init` completo e `FunctionPrototype`
   (hoje `create(vm, structure, function_prototype)` recebe `js_null()`). Assinatura: `fn create_function_prototype(vm: &VM, global: &JSGlobalObjectRef) -> JSValue`.
2. `src/runtime/program_executable.rs`: `initialize_global_properties` pede `&mut JSGlobalObject`,
   mas o global vive em `Rc<JSGlobalObject>`. Esperado: `global_object: &JSGlobalObject` (mutação interior) e
   `scope: &JSScopeRef`. Depois, `evaluate_script` passa a chamá-lo no lugar da consulta direta ao `CodeCache`.
3. `src/runtime/program_executable.rs`: `ProgramExecutable::prepare_for_execution(this: &Rc<RefCell<Self>>, vm: &VM, function: Option<&JSFunction>, scope: &JSScopeRef, kind: CodeSpecializationKind) -> Option<JSObjectRef>`
   (cria o `ProgramCodeBlock` via `ProgramCodeBlock::create`, que já existe, e o guarda em `m_programCodeBlock`).
4. `src/runtime/program_executable.rs`: `ProgramExecutable::program_code_block(&self) -> Option<CodeBlockRef>`.
5. `src/interpreter/interpreter.rs` (hoje só `DebugHookType`): `Interpreter::execute_program(&self, source: &SourceCode, this_obj: JSValue, global: &JSGlobalObjectRef) -> JSValue`
   (cria `VMEntryScope`, `ProtoCallFrame` e chama `execute_program_body`/`vm_entry`), mais `Interpreter::execute_call_impl`.
6. `src/llint` ou laço do interpretador (`CLoop`): despachar `op_enter`, `op_add`, `op_mov`/`op_load` de constante, `op_ret`/`op_end`
   sobre `InstructionStream`; `op_add` precisa de `JSValue::add` em `runtime/operations.rs`
   (`pub fn js_add(global: &JSGlobalObject, a: JSValue, b: JSValue) -> JSValue`).
7. `src/runtime/vm.rs`: `VM::new() -> VM` (o C++ devolve `Ref<VM>`); `Rc::new(VM::new())` basta, mas falta `VM::interpreter(&self) -> &Interpreter`.
8. `src/runtime/js_value.rs`: resultado de `1 + 1` sai como `js_number_i32(2)`; nada a criar, só confirmar `as_int32` após o `op_add`.

Estado atual: `evaluate_script` devolve `Err(js_undefined())` depois de gerar o bytecode sem erro (SyntaxError sai certo).
Nenhum build foi rodado nesta passagem.
