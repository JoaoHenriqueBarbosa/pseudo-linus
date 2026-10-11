# NodesCodegen: nomes usados e não definidos (inventário de 2026-10-08)

Escopo: `src/bytecompiler/nodes_codegen_cpp1.rs`, `cpp1b`, `cpp2`, `cpp3`, `cpp3b`, `cpp4`, `cpp4b`, `cpp5`, `cpp5b`, `cpp5c`.
Método: todo `generator.NOME` foi extraído e conferido por `fn NOME` em `src/bytecompiler/bytecode_generator*.rs`, `label*.rs`, `register_id.rs`; caminhos `crate::...` foram conferidos contra os módulos existentes em `src/`. Nada foi alterado.

## 1. Métodos de `generator.` que não existem

Dos cerca de 300 nomes distintos chamados em `generator.`, só dois não têm `fn`. (Ocorrências de `generator.ignoredResult`, `ensureThis` e `ecmaMode` são só comentário com o nome C++, não são chamadas.)

| arquivo | nome usado | usos | onde deveria existir | assinatura que o C++ tem |
|---|---|---|---|---|
| cpp2 | `generator.parser_arena()` | 2 (linhas 744 e 922); outra chamada em `bytecode_generator_cpp4.rs:1137` como `self.parser_arena()`, total 3 | `bytecompiler/bytecode_generator_part3.rs` (ou `bytecode_generator.rs`), `impl BytecodeGenerator` | `ParserArena& parserArena() const { return m_scopeNode->parserArena(); }` (BytecodeGenerator.h:378). Depende de `ScopeNode::parserArena()`, que também não existe em `src/parser/nodes*.rs`; o chamador ainda encadeia `.identifier_arena().make_identifier(vm, símbolo)`. |
| cpp5c | `generator.emit_node_in_ignore_result_position_statement(&stmt)` | 2 (linhas 311 e 373) | `bytecompiler/bytecode_generator_part2.rs`, ao lado de `emit_node_in_ignore_result_position_expression` (linha 86) | `void emitNodeInIgnoreResultPosition(StatementNode* n)` (BytecodeGenerator.h:495): `SetForScope(m_allowTailCallOptimization,false)`, `SetForScope(m_allowCallIgnoreResultOptimization, m_defaultAllowCallIgnoreResultOptimization)`, depois `emitNodeInTailPosition(ignoredResult(), n)`. A sobrecarga de expressão (h:554) já existe com o sufixo `_expression`; a de statement precisa do sufixo `_statement`. |

## 2. Módulos `crate::...` citados que não existem no crate

Contagem = ocorrências do prefixo do módulo nos dez arquivos. Nenhum destes arquivos está em `runtime/mod.rs`, `bytecode/mod.rs` ou existe um `interpreter/`. São a maior fonte de erro de compilação previsível.

| módulo ausente | usos | arquivos (usos) | o que se usa de lá |
|---|---|---|---|
| `runtime::js_value` | 67 | cpp1 19, cpp3b 13, cpp2 7, cpp5b 7, cpp5c 6, cpp1b 5, cpp4b 4, cpp3 3, cpp5 3 | `JSValue`, `js_undefined`, `js_null`, `js_number`, `js_number_i32`, `js_number_u32`, `js_boolean`, `JSValue::Undefined`, `JSValue::from_js_string` |
| `runtime::get_put_info` | 39 | cpp1b 11, cpp4b 13, cpp5b 7, cpp3b 5, cpp1 3 | `ResolveMode::{ThrowIfNotFound,DoNotThrowIfNotFound}`, `InitializationMode::{Initialization,ConstInitialization,NotInitialization}` |
| `bytecode::bytecode_ops` | 15 | cpp3b 7, cpp4 3, cpp1 2, cpp1b 1, cpp4b 1, cpp5c 1 | `OpAdd`, `OpEq`, `OpUnsigned`, `OpStricteq`, `OpNot`, `OpEqNull`, `OpNeqNull` (e demais `Op*`) |
| `bytecode::link_time_constant` | 4 | cpp1b 3, cpp1 1 | `LinkTimeConstant::{CloneObject,ImportModule,CreatePrivateSymbol}`; o enum existe em `bytecode/bytecode_intrinsics_table.rs:284` |
| `runtime::indexing_type` | 4 | cpp1 | `IndexingType`, `COPY_ON_WRITE`, `ARRAY_WITH_UNDECIDED`, `least_upper_bound_of_indexing_type_and_value` |
| `runtime::property_attribute` | 3 | cpp1b | `ACCESSOR`, `DONT_ENUM` |
| `bytecode::operand_types` | 3 | cpp1 2, cpp1b 1 | `OperandTypes::new`; o struct existe em `parser/result_type.rs:217` |
| `interpreter::call_frame` | 3 | cpp1 1, cpp2 2 | `CallFrameSlot::CALLEE`, `CallFrame::HEADER_SIZE_IN_REGISTERS` |
| `runtime::structure` | 3 | cpp2 | `Structure::{HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_BITS,HAS_NON_CONFIGURABLE_PROPERTIES_BITS,DID_PREVENT_EXTENSIONS_BITS}` |
| `bytecode::switch_info` | 3 | cpp5c | `SwitchType`; o enum existe em `parser/nodes.rs:185` |
| `runtime::js_generator` | 3 | cpp2 | `NUMBER_OF_INTERNAL_FIELDS`, `Field` |
| `runtime::js_disposable_stack` | 3 | cpp2 | idem |
| `runtime::js_async_disposable_stack` | 3 | cpp2 | idem |
| `runtime::js_array_iterator` | 3 | cpp2 | idem |
| `runtime::js_iterator_helper` | 2 | cpp2 | idem |
| `runtime::js_wrap_for_valid_iterator` | 2 | cpp2 | idem |
| `runtime::proxy_object` | 2 | cpp2 | `NUMBER_OF_INTERNAL_FIELDS`, `Field` |
| `bytecode::speculated_type` | 2 | cpp2 | `SpecNone`, `speculation_from_string` |
| `bytecode::handler_info` | 1 | cpp5c | `HandlerType::{Catch,Finally}` |
| `runtime::error_type` | 1 | cpp1 | `ErrorTypeWithExtension::SyntaxError` |
| `runtime::js_cell_butterfly` | 1 | cpp1 | `JSCellButterfly::try_create` |
| `runtime::reg_exp` | 1 | cpp1 | `RegExp::create` |

## 3. Tipos citados que o crate não define, ou define em outro caminho

| tipo | caminho usado nos nodes_codegen | situação |
|---|---|---|
| `ECMAMode` (`::strict`, `::sloppy`) | `parser::parser_modes::ECMAMode` (3 usos) | não definido em lugar nenhum (`parser_modes.rs` não o tem); `bytecode_generator_cpp2.rs` usa ainda um terceiro caminho `runtime::ecma_mode::ECMAMode`, também inexistente |
| `SourceCodeRepresentation` (`Integer`, `Double`) | `parser::parser_tokens::SourceCodeRepresentation` (2) | não definido |
| `DerivedContextType` | `parser::parser_modes::DerivedContextType` (3) | definido em `bytecode/executable_info.rs:12`, não em `parser_modes` |
| `DebugHookType` | `bytecode::opcode::DebugHookType` (3 mais `use` locais no cpp5c) | não definido em `bytecode/opcode.rs`; o gerador usa também `interpreter::debug_hook_type::DebugHookType` (4) |
| `CodeType::FunctionCode` | `bytecompiler::bytecode_generator::CodeType` (1) | não definido; o gerador usa `bytecode::code_type::CodeType` (7) e `bytecode::executable_info::CodeType` (6), nenhum existe |
| `ErrorTypeWithExtension` | `bytecompiler::bytecode_generator::` (2) e `runtime::error_type::` (1) | não definido; o gerador usa `runtime::error_type::` (7) |
| `ResolveMode` | `runtime::get_put_info::` (23) e `bytecompiler::bytecode_generator::` (4) | não definido; o gerador usa `get_put_info` (22) e `runtime::resolve_type::` (2). Unificar em um só caminho |
| `InitializationMode` | `runtime::get_put_info::` (14) | não definido; o gerador também usa `runtime::resolve_type::` (1) |
| `TDZCheckOptimization` | `bytecode_generator::` (2) e `bytecode_generator_part3::` (4) | o enum existe como `TdzCheckOptimization` (grafia diferente) em `bytecode_generator_part3.rs:22`; `bytecode_generator` não o reexporta |
| `ScopeType`, `NestedScopeType` | `bytecode_generator::` (4 e 2) e `bytecode_generator_part3::` (4 e 4) | definidos em `bytecode_generator_part3.rs`; só o caminho `bytecode_generator::` precisa de `pub use` |
| `CallFrame`, `CallFrameSlot` | `interpreter::call_frame` | ver seção 2 |
| `StrictModeScope`, `PreservedTdzStack`, `ForInContext`, `FinallyContext`, `Variable`, `CompletionType`, `CallArguments`, `DebuggableCall`, `EmitAwait`, `ExpectedFunction`, `ThisResolutionType`, `InvalidPrototypeMode` | `bytecode_generator[_part3]::` | existem |
| `ScopeNode`, `CaseBlockNode` (e `::TABLE_SWITCH_MINIMUM`), `ClauseListNode`, `CaseClauseNode`, `ForOfNode`, `ReadModifyResolveNode`, `ShortCircuitReadModifyResolveNode`, `OptionalChainNode`, `DestructuringPatternNode`, `ProgramNode`, `ModuleProgramNode`, `EvalNode` | `parser::nodes::` | existem (`nodes_part2.rs`, `nodes_part3.rs`, os três últimos via macro) |
| `UnaryPlusNode`, `LogicalNotNode`, `EqualNode`, `StrictEqualNode` e demais nós por nome | `parser::nodes::` | não conferidos um a um fora da lista acima; os do grupo `Op*Node` ficam para o compilador acusar |

## 4. Os dez nomes mais repetidos que faltam

Por número de ocorrências de uso nos dez arquivos:

1. `crate::runtime::js_value::*` (67)
2. `crate::runtime::get_put_info::*` (39, `ResolveMode` e `InitializationMode`)
3. `crate::bytecode::bytecode_ops::Op*` (15)
4. `ResolveMode` fora de `get_put_info` (4 usos em `bytecode_generator::`, mais 2 em `resolve_type` no gerador)
5. `crate::runtime::indexing_type::*` (4)
6. `crate::bytecode::link_time_constant::LinkTimeConstant` (4, o enum existe em outro módulo)
7. `TDZCheckOptimization` (6 usos; o enum se chama `TdzCheckOptimization`)
8. `ECMAMode` (3, mais 6 no gerador)
9. `DerivedContextType` em `parser_modes` (3, definido em `executable_info`)
10. `parser_arena` (3), `emit_node_in_ignore_result_position_statement` (2), `crate::runtime::property_attribute::*` (3), `interpreter::call_frame` (3), `bytecode::switch_info::SwitchType` (3)
