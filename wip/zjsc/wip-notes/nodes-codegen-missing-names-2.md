# NodesCodegen: nomes usados e não definidos, segunda passagem (2026-10-08)

Escopo: as 13 fatias `nodes_codegen_cpp1..7` (inclui `cpp5d`, `cpp6`, `cpp7`, que o inventário 1 não cobria). Complementa `nodes-codegen-missing-names.md`.
Método: todo `generator.NOME` e todo `.metodo(` conferido por `fn NOME` em `src/`; todo `crate::a::b::Item` conferido no arquivo do módulo. As chamadas a `Op*` passam só por `generator.emit_unary_op::<Op..>` / `emit_binary_op::<Op..>` (assinatura de 2 e 4 argumentos, bate com `bytecode_generator_part2.rs:325/343`); `Op*::emit` direto não é chamado em nenhuma fatia. As structs `OpAdd`, `OpEq`, `OpStricteq`, `OpEqNull`, `OpNeqNull`, `OpNot`, `OpUnsigned` existem (geradas por macro em `bytecode_ops.rs`, com `BinaryOpcode`/`UnaryOpcode`).

## Corrigido nas fatias

- `cpp2:1517`, `cpp4:148`, `cpp4b:575`: `crate::parser::nodes::is_non_index_string_element` não existe; passou a usar o auxiliar local `nodes_codegen_is_non_index_string_element` (cpp1b:773).
- `cpp7:115`: `crate::runtime::identifier::IdentifierSet` passou a `crate::parser::parser::IdentifierSet`.
- `cpp2:87`: `self.base.throwable_position()` (não existe) passou a `self.base.base.position().clone()` (o `m_position` do C++).
- `cpp6:282-292`: `FunctionMetadataNode` tem `Cell` para `needs_class_field_initializer` e `private_brand_requirement` e campo simples `super_binding`; o código usava `borrow_mut().set_*` e `borrow().super_binding()`. Agora usa `.set(..)`, `.super_binding` e `metadata` como `Rc` clonado (`.metadata.clone()`).

## Falta em outros arquivos

| arquivo:linha | o que falta | onde criar |
|---|---|---|
| cpp2:744, 922; cpp1:188 | `BytecodeGenerator::parser_arena()` (e `ScopeNode::parser_arena()`) | `bytecode_generator_part3.rs`; `parser/nodes_part2.rs` |
| cpp5c:311, 373 | `generator.emit_node_in_ignore_result_position_statement(&stmt)` | `bytecode_generator_part2.rs`, junto da versão `_expression` (linha 83) |
| cpp5b:26,33,103,109,115,133,157,320,368,383,406 | `Expression::as_resolve_node`, `as_assign_resolve_node`, `as_dot_accessor_node`, `as_bracket_accessor_node`, `as_destructuring_node` (retornam o `Rc<RefCell<..>>` da variante; hoje só há `is_*` em `nodes.rs:1985-2009`) | `parser/nodes.rs`, `impl Expression` |
| cpp5b:633, 655; cpp6:281 | `Expression::as_number_node` (Double e Integer), `as_string_node`, `as_func_expr_node` (FuncExpr e MethodDefinition) | idem |
| cpp6:283-284 | `FunctionMetadataNode::set_ecma_name(&self, &Identifier)` (`m_ecmaName = m_name.isNull() ? name : m_name`) e `set_class_source(&self, &SourceCode)`; só existem os campos `RefCell` | `parser/nodes_part3.rs` (~linha 451) |
| cpp2:704 | `BytecodeIntrinsicRegistry::constant_value(constant, generator)` | `bytecode/bytecode_intrinsic_registry.rs` |
| cpp1:78 | `JSValue::pure_to_boolean` | `runtime/js_value.rs` |
| cpp1:181 | `crate::runtime::reg_exp::RegExp::create` (módulo não existe) | `runtime/reg_exp.rs` |
| cpp1:633 | `runtime::indexing_type::least_upper_bound_of_indexing_type_and_value` (o módulo só tem os predicados; o cabeçalho cita que fica de fora por depender de `JSValue`) | `runtime/indexing_type.rs` |
| cpp1:640, 696 | `JSString::try_get_value_impl`, `JSString::get_value_impl` | `runtime/js_string.rs` |
| cpp1:671-700 | `Heap::is_deferred`, `VM::cell_butterfly_structure(IndexingType)`, `VM::cell_butterfly_only_atom_strings_structure()`, `VM::atom_string_to_js_string_map()` com `ensure_value(impl, closure)`; `runtime::js_cell_butterfly::JSCellButterfly::try_create` (módulo não existe) | `runtime/vm.rs`, `runtime/js_cell_butterfly.rs` |
| todas as fatias (`crate::bytecompiler::bytecode_generator::X`) | `bytecompiler/mod.rs` ainda não declara `bytecode_generator*` nem `nodes_codegen`; `bytecode_generator.rs` não reexporta de `bytecode_generator_part2/part3`: `NestedScopeType`, `TDZCheckOptimization`, `PreservedTDZStack`, `PROPERTY_CONFIGURABLE`, `PROPERTY_WRITABLE` (cpp1b:691, cpp4b:801, cpp5c:182, cpp6:365, cpp7:108). Obs.: o inventário 1 acusa grafia `TdzCheckOptimization`, mas o arquivo atual tem `TDZCheckOptimization` | `bytecompiler/mod.rs`, `bytecode_generator.rs` (`pub use`) |

## Conferido e sem problema

`generator.*`: dos 271 nomes, só os três acima não têm `fn` (`move` é só comentário em cpp5d:49). Intrínsecos `emit_intrinsic_*` do cpp2 são gerados por macro (cpp2:1302-1482), inclusive os `get/put_*_internal_field`. Módulos `crate::...` citados existem como arquivo, exceto `reg_exp` e `js_cell_butterfly`. Nós do AST (`*Node`) existem em `parser/nodes*.rs`, inclusive `ProgramNode`, `EvalNode`, `ModuleProgramNode` e `FunctionNode` (via macro/enum).
