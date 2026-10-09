# Erros de outros donos vistos nos executáveis (rodada 6)

Corrigidos em `script_executable.rs`, `program_executable.rs`, `function_executable.rs` e
`module_program_executable.rs`. Sobra o que depende de código de outro dono:

## `runtime/js_global_object*.rs` (JSGlobalObject)
- `bump_global_lexical_binding_epoch(&self, vm: &VM)`: usado em `program_executable.rs` (initialize_global_properties).
  C++: incrementa `m_globalLexicalBindingEpoch`; ao chegar em `Options::thresholdForGlobalLexicalBindingEpoch()` volta a 1.
- `try_get_cached_function_executable_for_function_constructor(name, &program, source_origin, tainted_origin,
  source_url, position, lexically_scoped_features, function_construction_mode) -> Option<Rc<RefCell<FunctionExecutable>>>`
  e `cached_function_executable_for_function_constructor(&executable)`: usados em `FunctionExecutable::from_global_code`.

## `runtime/js_object.rs` (JSObject)
- `get_own_property_descriptor(&self, global_object, &PropertyName, &mut PropertyDescriptor) -> bool`
  (`JSObject::getOwnPropertyDescriptor`): usado em `has_restricted_global_property` (program_executable.rs:74).

## `runtime/js_template_object_descriptor.rs`
- `create_template_object(&self, global_object: &JSGlobalObject) -> Option<JSArrayRef>`: usado em
  `ScriptExecutableRef::create_template_object` (script_executable.rs).

## `bytecode/code_block.rs` (CodeBlock)
- `dump(&self, out: &mut dyn fmt::Write) -> fmt::Result` (`CodeBlock::dump(PrintStream&)`): usado em
  `ScriptExecutableRef::dump` (5 chamadas).

## `debugger/debugger.rs`
- `Debugger::register_code_block(&self, &CodeBlockRef)` (`Debugger::registerCodeBlock`): usado em
  `install_code_for_kind`.

## Decisões locais (divergências marcadas no código)
- Sem `m_alternative`/JIT: `baselineVersion()` e `baselineAlternative()` viram o próprio bloco; `setAlternative`,
  `setIsJettisoned`, `jitType` (assert) e `unlinkOrUpgradeIncomingCalls` foram omitidos.
- `Options::validateBytecode()` chamava `CodeBlock::validate()`: o validador não foi portado, chamada omitida.
- `CodeBlock::vm()` não existe: o `VM` vem de `global_object().vm_rc()`.
