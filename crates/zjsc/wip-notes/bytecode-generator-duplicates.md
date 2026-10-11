# BytecodeGenerator: duplicatas e nomes sem resolução (inventário de 2026-10-08)

Escopo: `src/bytecompiler/bytecode_generator.rs`, `_part2`, `_part3`, `_base`, `_cpp1`, `_cpp1c`, `_cpp2` a `_cpp6`, `label.rs`, `label_scope.rs`, `register_id.rs`.
Método: toda definição de nível de módulo e todo método de `impl` foi extraído por arquivo e agrupado por (tipo do `impl`, nome); todo caminho `crate::a::b::Item` foi conferido contra `src/` (arquivo do módulo e definição do item); os nomes soltos (sem `::`) foram conferidos contra os `use` do topo e os `use` locais de cada função. Referência do C++: `upstream/JavaScriptCore/bytecompiler/BytecodeGenerator.h` e `.cpp`. Nada foi alterado.

## 0. Estado da costura (pré-requisito de tudo abaixo)

- Nenhum `include!` está ativo no `bytecompiler`: os comentários dizem "Juntada por include!", mas `bytecode_generator.rs` não tem nenhuma linha `include!`, e `bytecompiler/mod.rs` só declara `label`, `label_scope`, `profile_type_bytecode_flag`, `register_id`, `static_property_analysis`, `static_property_analyzer`. Também não há `pub mod bytecompiler;` em `src/lib.rs`. Os `nodes_codegen_cpp*` seguem o mesmo padrão (comentário sem `include!`).
- Módulo efetivo assumido nesta análise (o que o desenho pede): `bytecode_generator.rs` com `include!` de `_part2`, `_part3`, `_cpp1`, `_cpp1c`, `_cpp2` a `_cpp6` (ordem sugerida: part3 com a struct, part2, cpp1, cpp1c, cpp2..cpp6). `_base`, `label`, `label_scope`, `register_id` são módulos próprios.
- `bytecode_generator.rs:20` faz `pub use crate::bytecompiler::bytecode_generator_part2::BytecodeGenerator;`, mas `part2` não é módulo (é fragmento) e a struct está em `_part3.rs:117`. Ver item 3 da tabela de duplicatas.

## 1. Definições duplicadas

Resultado da varredura: nenhum método, função livre, struct, enum, trait, const ou type com o mesmo nome em dois dos arquivos incluídos (518 definições distintas por (tipo do impl, nome); os `impl Variable`, `impl FinallyContext` e `impl ForInContext` espalhados em `_cpp1` e `_cpp6` não colidem com os de `bytecode_generator.rs`). As colisões reais são entre módulos e na costura:

| nome | arquivos | qual manter e por quê (segundo o C++) |
|---|---|---|
| `JSGeneratorTraits` | `label.rs:21` (`pub struct JSGeneratorTraits;`, base de `Label`, `LabelRef`, `BoundLabel`) e `bytecode_generator.rs:501` (`pub struct JSGeneratorTraits;` com `impl` da const `OPCODE_FOR_DISABLING_OPTIMIZATIONS`) | Manter o de `label.rs`. No C++ `Label.h` só declara `struct JSGeneratorTraits;` e a definição única está em `BytecodeGenerator.h:347` (com `opcodeForDisablingOptimizations = op_debug`). Em Rust, duas structs vazias são dois tipos distintos, então `GenericLabel<label::JSGeneratorTraits>` nunca casaria com o `JSGeneratorTraits` do gerador. Em `bytecode_generator.rs`: trocar a struct por `pub use crate::bytecompiler::label::JSGeneratorTraits;` e manter só o `impl JSGeneratorTraits` da const; o `impl BytecodeGeneratorTraits for JSGeneratorTraits` (hoje inexistente) entra junto, com `set_label_location` (ver item 5). |
| `RegisterRef` | `bytecode_generator.rs:23` (`pub type RegisterRef = Rc<RefCell<RegisterID>>;`) e `register_id.rs:95` (`pub struct RegisterRef` com contagem intrusiva, `Clone`/`Drop`) | Manter o struct de `register_id.rs`. `RefPtr<RegisterID>` do C++ incrementa e decrementa `m_refCount`, e o gerador observa `refCount()` (`BytecodeGeneratorBase::newTemporary`, `shrink_to_fit`, `LabelScope`); o alias `Rc<RefCell<..>>` perde essa semântica e deixa o `ref_count` do `RegisterID` sempre zero. Em `bytecode_generator.rs`: `pub use crate::bytecompiler::register_id::RegisterRef;` no lugar do alias (os 443 usos por `crate::bytecompiler::bytecode_generator::RegisterRef` continuam valendo). Além disso, `register_id.rs:14` define `RegisterIDRef = Rc<RefCell<RegisterID>>` e `_base` usa esse alias no lugar de `RegisterRef`: são três nomes para duas representações; `_base` deve passar a usar `RegisterRef` do `register_id.rs`, e `RegisterIDRef` sai se ninguém mais o usar. |
| `BytecodeGenerator` | `bytecode_generator.rs:20` (`pub use ...bytecode_generator_part2::BytecodeGenerator`) e `_part3.rs:117` (`pub struct BytecodeGenerator`) | Manter a struct de `_part3.rs:117` (no C++ a classe é uma só, `BytecodeGenerator.h:360`) e apagar o `pub use` da linha 20. Com os `include!` ativos o `pub use` colide com a struct (erro E0255); sem eles aponta para um módulo que não existe. |
| `set_label_location` (trait `BytecodeGeneratorTraits`, `_base.rs:98`) e `generic_label_set_location` (`_cpp1.rs:38`) | `_base` e `_cpp1` | Não é duplicata de nome, é a mesma função do C++ (`GenericLabel<JSGeneratorTraits>::setLocation`, `BytecodeGenerator.cpp`) em duas metades sem ligação: o `emit_label` da base chama o trait, o corpo está na função livre do `_cpp1` e nenhum `impl BytecodeGeneratorTraits for JSGeneratorTraits` existe (grep: zero). Manter o corpo de `_cpp1` e movê-lo para dentro desse `impl` (a função livre deixa de existir, sem repasse, pela regra DRY do projeto). |
| `PROPERTY_CONFIGURABLE`, `PROPERTY_WRITABLE`, `PROPERTY_ENUMERABLE` | `_part2.rs:704-706`, consts livres | Sem duplicata em `src/`, mas no C++ são enumeradores da classe (`BytecodeGenerator.h:887`, `PropertyConfigurable = 1` e seguintes). Manter, movendo para `impl BytecodeGenerator` como consts associadas, como `CURRENT_LEXICAL_SCOPE_INDEX` já está em `_part3.rs:279`. |

Total de duplicatas reais: 3 (`JSGeneratorTraits`, `RegisterRef`, `BytecodeGenerator`), mais 1 função partida em duas (`set_label_location`) e 1 grupo de consts fora do lugar. Casing divergente de enum, que também funciona como "duas definições para o mesmo nome do C++", está no item 2.4.

## 2. Nomes que não resolvem

### 2.1 Módulos `crate::...` inexistentes (60)

Contagem de ocorrências nos arquivos do escopo. `bytecode/mod.rs` declara só `opcode`, `bytecode_intrinsic_registry`, `bytecode_intrinsics_table`, `executable_info`, `code_block_hash`, `parse_hash`, `unlinked_function_executable`; `runtime/mod.rs` não tem `js_value`, `symbol_table` etc.; não existe `interpreter/`.

| módulo ausente | usos | observação |
|---|---|---|
| `bytecode::bytecode_list` | 144 | gerado pelo `.rb` do upstream (`Op*`, metadados) |
| `bytecode::bytecode_ops` | 91 | `OpAdd`, `OpJmp`, `ConstructOpcode`, `TestOp`, `CompareOp`, 193 nomes `Op*` distintos |
| `runtime::js_value` | 63 | `JSValue`, `js_number` e afins |
| `runtime::get_put_info` | 52 | `GetPutInfo`, `ResolveMode`, `ResolveType`, `InitializationMode` |
| `runtime::symbol_table` | 47 | `SymbolTable`, `SymbolTableEntry`, `NO_LOCKING_NECESSARY` |
| `bytecode::var_offset` | 30 | o código também cita `runtime::var_offset` (2); no C++ é `runtime/VarOffset.h`, então o certo é `runtime::var_offset` |
| `runtime::js_generator` | 18 | `ResumeMode` |
| `runtime::js_type` | 17 | `JSType` |
| `bytecode::link_time_constant` | 17 | o enum existe em `bytecode::bytecode_intrinsics_table::LinkTimeConstant` (`bytecode_intrinsics_table.rs:284`); corrigir o caminho |
| `interpreter::call_frame` | 12 | `CallFrameSlot` |
| `parser::source_code_representation` | 11 | `SourceCodeRepresentation` (C++ `runtime/JSCJSValue.h:91`, então o lugar é `runtime::js_value`) |
| `bytecode::code_generation_mode` | 11 | `CodeGenerationModeSet` |
| `runtime::js_async_generator` | 9 | `AsyncGeneratorSuspendReason` |
| `bytecode::handler_info` | 8 | `HandlerType` (também é `use` de `bytecode_generator.rs:11`) |
| `runtime::unlinked_function_executable` | 7 | existe só `bytecode::unlinked_function_executable` |
| `runtime::error_type` | 7 | |
| `bytecode::code_type` | 7 | `CodeType` (C++ `bytecode/CodeType.h:32`) |
| `runtime::property_attribute` | 6 | `READ_ONLY` (também é `use` de `bytecode_generator.rs:17`) |
| `runtime::js_string` | 5 | |
| `parser::unlinked_function_executable` | 5 | caminho errado, o módulo é `bytecode::unlinked_function_executable` |
| `bytecompiler::var_kind` | 5 | `VarKind` é de `runtime/VarOffset.h:35`, então `runtime::var_offset` |
| `bytecode::switch_info` | 5 | `SwitchType` existe em `parser::nodes::SwitchType` (`nodes.rs:185`) |
| `bytecode::instruction_stream` | 5 | |
| `runtime::abstract_module_record` | 4 | |
| `interpreter::debug_hook_type` | 4 | `DebugHookType` é de `interpreter/Interpreter.h:96`; também é citado como `bytecode::opcode::DebugHookType` (8 usos, não existe) |
| `wtf::vector` | 3 | |
| `runtime::template_object_descriptor` | 3 | |
| `runtime::resolve_type` | 3 | `ResolveType` está em `get_put_info` no C++ |
| `runtime::indexing_type` | 3 | |
| `bytecode::private_name_environment` | 3 | o tipo existe em `parser::variable_environment::PrivateNameEnvironment` |
| `runtime::private_name_entry` | 2 | existe `parser::variable_environment::PrivateNameEntry` |
| `runtime::js_template_object_descriptor` | 2 | |
| `runtime::inline_attribute` | 2 | |
| `parser::js_text_position` | 2 | o tipo existe em `parser::parser_tokens::JSTextPosition` |
| `bytecompiler::tdz_environment` | 2 | existe `parser::variable_environment::TDZEnvironmentLink` |
| `bytecode::tdz_environment` | 2 | idem |
| `bytecode::unlinked_code_block_generator` | 2 | |
| `bytecode::put_kind` | 2 | |
| `bytecode::put_by_id_flags` | 2 | |
| `wtf::bit_vector` | 1 | |
| `runtime::reg_exp` | 1 | |
| `runtime::js_cell_butterfly` | 1 | |
| `runtime::js_async_function_generator` | 1 | |
| `runtime::error_messages` | 1 | |
| `runtime::ecma_mode` | 1 | `ECMAMode` não existe em lugar nenhum de `src/` |
| `runtime::define_property_attributes` | 1 | |
| `runtime::defer_termination` | 1 | |
| `bytecompiler::bytecode_generatorification` | 1 | `BytecodeGeneratorification.cpp` ainda não portado (`_cpp1.rs:408`) |
| `bytecode::unlinked_string_jump_table` | 1 | |
| `bytecode::unlinked_program_code_block` | 1 | |
| `bytecode::unlinked_module_program_code_block` | 1 | |
| `bytecode::unlinked_handler_info` | 1 | |
| `bytecode::unlinked_function_code_block` | 1 | |
| `bytecode::unlinked_eval_code_block` | 1 | |
| `bytecode::unlinked_code_block` | 1 | |
| `bytecode::speculated_type` | 1 | |
| `bytecode::line_column` | 1 | |
| `bytecode::code_block` | 1 | |
| `bytecode::bytecode_use_def` | 1 | |

Efeito direto nos `use` do topo de `bytecode_generator.rs`: dos 11 `use`, três não resolvem (`HandlerType`, `READ_ONLY`, `VarOffset`); os demais (`Label`, `LabelScope`, `LabelScopeType`, `RegisterID`, `ArgumentsNode`, `NodeRef`, `Statement`, `Identifier`) resolvem. Em `_base.rs`, todos os `use` resolvem (`virtual_register_for_local`, `round_up_to_multiple_of`, `label`, `register_id`); `label.rs`, `label_scope.rs` e `register_id.rs` também.

### 2.2 `OpcodeID` com nomes de variante que não existem (42 variantes distintas, 93 usos)

`bytecode/opcode.rs` define as variantes em snake_case (`op_add`, `op_debug`, `op_call_varargs`...; `bytecode_generator.rs:504` usa `OpcodeID::op_debug` e resolve). O código usa CamelCase nas outras: `_cpp2.rs` (54 usos), `_cpp4.rs` (28), `_part2.rs` (9), `_part3.rs` (2). Todas as 42 têm a variante snake_case correspondente em `opcode.rs`, então é só renomear o uso: `OpAdd` para `op_add`, `OpBelow`, `OpBeloweq`, `OpBitand`, `OpBitnot`, `OpBitor`, `OpBitxor`, `OpCall`, `OpCallDirectEval` (`op_call_direct_eval`), `OpCallIgnoreResult`, `OpCallVarargs`, `OpConstruct`, `OpConstructVarargs`, `OpDebug`, `OpDiv`, `OpEq`, `OpEqNull`, `OpGreater`, `OpGreatereq`, `OpIsUndefinedOrNull`, `OpLess`, `OpLesseq`, `OpLshift`, `OpMod`, `OpMul`, `OpNegate`, `OpNeq`, `OpNeqNull`, `OpNot`, `OpNstricteq`, `OpPow`, `OpRshift`, `OpStricteq`, `OpSub`, `OpSuperConstruct`, `OpSuperConstructVarargs`, `OpTailCall`, `OpTailCallVarargs`, `OpToNumber`, `OpToNumeric`, `OpUnsigned`, `OpUrshift`. O `use crate::bytecode::opcode::OpcodeID;` local em si resolve.

### 2.3 Caminhos errados para itens que existem em outro lugar (27)

| usado como | usos | existe em |
|---|---|---|
| `parser::parser_modes::DerivedContextType` | 4 | `bytecode::executable_info::DerivedContextType` (`executable_info.rs:12`) |
| `parser::parser_modes::EvalContextType` | 1 | `bytecode::executable_info::EvalContextType` (`executable_info.rs:20`) |
| `bytecode::executable_info::PrivateBrandRequirement` | 3 | `parser::parser_modes::PrivateBrandRequirement` (`parser_modes.rs:29`) |
| `bytecode::executable_info::SuperBinding` | 1 | `parser::parser_modes::SuperBinding` (`parser_modes.rs:22`) |
| `parser::parser_modes::ConstructorKind`, `parser::unlinked_function_executable::ConstructorKind` | 1 e 1 local | `runtime::constructor_kind::ConstructorKind` |
| `parser::parser_modes::ImplementationVisibility` | 1 | `runtime::implementation_visibility::ImplementationVisibility` |
| `parser::parser::SourceParseMode` (local em `_cpp1.rs:513`, `parser.rs` só importa) | 2 | `parser::parser_modes::SourceParseMode` (`parser_modes.rs:63`) |
| `parser::parser::JSTextPosition` | 1 | `parser::parser_tokens::JSTextPosition` (`parser_tokens.rs:200`) |
| `parser::parser_tokens::PrivateNameEnvironment` | 4 | `parser::variable_environment::PrivateNameEnvironment` (`variable_environment.rs:333`) |
| `runtime::identifier::UniquedStringImpl`, `wtf::text::uniqued_string_impl::UniquedStringImpl` (6) e `UniquedStringImplRef` (1) | 5 + 6 + 1 | `wtf::text::atom_string_impl::UniquedStringImpl` (`atom_string_impl.rs:35`); `UniquedStringImplRef` não existe |
| `runtime::identifier::IdentifierSet` | 2 | `parser::parser::IdentifierSet` (`parser.rs:97`) |
| `bytecompiler::label::FallThroughMode` | 1 | `parser::nodes::FallThroughMode` (`nodes.rs:160`) |
| `bytecode::opcode::OpcodeSize` | 1 | `bytecompiler::bytecode_generator_base::OpcodeSize` |
| `bytecompiler::bytecode_generator::FIRST_CONSTANT_REGISTER_INDEX` | 1 | `bytecode::virtual_register::FIRST_CONSTANT_REGISTER_INDEX` (`virtual_register.rs:13`) |
| `bytecompiler::bytecode_generator::TdzEnvironmentLink` | 4 (+ `TDZEnvironmentLink` solto 2) | `parser::variable_environment::TDZEnvironmentLink` (`variable_environment.rs:970`) |
| `bytecompiler::bytecode_generator::{CURRENT,OUTERMOST}_LEXICAL_SCOPE_INDEX` | 2 + 1 | consts associadas `BytecodeGenerator::CURRENT_LEXICAL_SCOPE_INDEX` (`_part3.rs:279`), não caminho de módulo |
| `bytecompiler::bytecode_generator::NO_EXPECTED_FUNCTION` | 1 | variante `ExpectedFunction::NoExpectedFunction` (`bytecode_generator.rs:29`); no C++ é `NoExpectedFunction`, enumerador de `ExpectedFunction` |
| `runtime::construct_ability::construct_ability_for_parse_mode` | 1 | `parser::parser_modes::construct_ability_for_parse_mode` (`parser_modes.rs:314`) |
| `runtime::options::{optimize_recursive_tail_calls, eval_mode}` | 1 + 1 | métodos de `Options` em `runtime/options_list.rs:1686` e `:1356` (não funções do módulo `options`) |
| nomes soltos em `_cpp1.rs` sem `use`: `VirtualRegister`, `SourceParseModeSet`, `LinkTimeConstant`, `ErrorType` (só local em `generate`) | 3 + 3 + 2 + 3 | `bytecode::virtual_register`, `parser::parser_modes`, `bytecode::bytecode_intrinsics_table`, `parser::parser_error` |

### 2.4 Nomes que não existem em lugar nenhum de `src/` (30) e casing

Nomes de tipos e consts:

| nome | usos | arquivo (exemplo) | onde está no C++ / observação |
|---|---|---|---|
| `DebugHookType` | 8 | `_cpp5.rs:274` | `interpreter/Interpreter.h:96` |
| `CodeType` | 9 | `_cpp1.rs:194`, `_cpp3.rs:591` | `bytecode/CodeType.h:32` |
| `ECMAMode` | 5 | `_cpp1.rs:466` | `runtime/ECMAMode.h` |
| `SourceCodeRepresentation` | 5 | `_cpp2.rs:953` | `runtime/JSCJSValue.h:91` |
| `ExistingVariableMode` | 8 | `_cpp1.rs:983`, `_cpp2.rs:112` | `BytecodeGenerator.h:420` (`enum ExistingVariableMode { VerifyExisting, IgnoreExisting };`): falta definir em `_part3.rs` |
| `Scope` (`Scope::Program/Function/Eval`) | 3 | `_cpp1.rs:451,519`, `_cpp1c.rs:19` | variante usada no lugar do `m_scopeNode` (`ScopeNode*`); não há tipo `Scope` no C++ do gerador |
| `IdentifierMap` | 1 | `_part3.rs:180` | `BytecodeGenerator.h:1423`, tipo de origem a localizar |
| `TdzEnvironment` | 1 | `_cpp4.rs:721` | `parser/VariableEnvironment.h:345` (`TDZEnvironment`, alias de `HashSet`); no Rust só existe `TDZEnvironmentLink` |
| `CallFrameSlot` | 3 | `_cpp1.rs:665`, `_cpp2.rs:100`, `_part3.rs:535` | `interpreter/CallFrame.h` |
| `try_make_string` | 2 | `_cpp1.rs:686,708` | `wtf::text` só tem `make_string_by_replacing*` |
| `FunctionMetadataNodeRef`, `ScopeNodeRef`, `RestParameterNodeRef` | 3 + 2 + 1 | `_cpp1.rs`, `_cpp4.rs` | não são tipos; o modelo é `NodeRef<FunctionMetadataNode>` etc. (`NodeRef<T> = Rc<RefCell<T>>`, `nodes.rs:88`) |
| `FunctionNode`, `ProgramNode`, `ModuleProgramNode`, `EvalNode` | 2 + 1 + 1 + 1 | `_cpp1.rs`, `_cpp1c.rs`, `_cpp2.rs` | `parser::parser_cpp*`/`parsed_node_impls.rs` também os importam de `parser::nodes`, onde não há `struct` com esses nomes (lacuna do parser, não do gerador) |
| `SymbolTable`, `SymbolTableEntry`, `SymbolTableOrScopeDepth`, `DirectArgumentsOffset`, `VarOffset`, `VarKind` | 1, 8, 2, 1, 12+2, 8 | `_cpp1.rs`, `_cpp2.rs` | `runtime/SymbolTable.h`, `runtime/VarOffset.h` (módulos 2.1) |
| `ResolveMode`, `InitializationMode`, `GetPutInfo`, `ResolveType`, `ResumeMode`, `AsyncGeneratorSuspendReason`, `JSType` | 4, 4, 2, 2, 2, 1, 2 | `_cpp1.rs`, `_cpp2.rs`, `_cpp6.rs` | módulos 2.1 (a maioria em `runtime/GetPutInfo.h`, `JSGenerator.h`, `JSAsyncGenerator.h`, `JSType.h`) |

Casing: `_part3.rs` define `TdzCheckOptimization`, `TdzRequirement`, `TdzNecessityLevel`, `TdzMap`, `TdzStackEntry`, `PreservedTdzStack`; o resto do código (e o C++, `BytecodeGenerator.h:1128,1131,1278,1294`) usa `TDZCheckOptimization`, `TDZRequirement`, `TDZNecessityLevel`, `TDZStackEntry`, e o parser já tem `TDZEnvironmentLink`. Os usos qualificados `bytecode_generator::TDZCheckOptimization` (12 em cpp1, cpp1c, cpp2) e `TDZRequirement` (11) não resolvem contra as definições `Tdz*`. Manter a grafia do C++ (`TDZ*`) e renomear as definições em `_part3.rs` (incluindo os usos internos de `_part3.rs:498,625,699` e afins), porque são ~25 usos externos contra ~10 internos e é a grafia que o resto do crate já adotou.

### 2.5 Métodos chamados em `self.` sem definição (15)

- Oito sem `fn` em lugar nenhum: `emit_throw_type_error_str` (10 chamadas), `is_ignored_result` (4), `add_constant_value_symbol_table` (2), `parser_arena` (1; já listado em `nodes-codegen-missing-names.md`), `new_parameter_register` (1), `emit_node_in_tail_position` (1; as sobrecargas existem só com sufixo: `_statement`, `_expression`, `_from_return_node`...), `emit_node_dst_statement` (1), `emit_debug_hook_property_list` (1).
- Sete existem só na `BytecodeGeneratorBase` genérica (`_base.rs`) e o `BytecodeGenerator` não os herda nem os repassa: `new_label` (121 chamadas), `new_emitted_label` (19), `emit_label` (131), `new_temporary` (259), `new_register` (4), `add_var` (5), `new_temporaries` (1). O `_part3.rs` diz que os membros da base ficam "achatados na mesma struct", então a base genérica não é usada pelo gerador. Decisão que o C++ impõe (`class BytecodeGenerator : public BytecodeGeneratorBase<JSGeneratorTraits>`): o gerador contém uma `BytecodeGeneratorBase<JSGeneratorTraits>` e dá `Deref`/`DerefMut` para ela (a regra DRY do projeto sugere `Deref` no newtype em vez de métodos repassando), ou os campos achatados passam a ser exatamente os da base. Não se escrevem os sete métodos de novo no gerador.

## 3. Resumo

- Duplicatas reais: 3 nomes (`JSGeneratorTraits`, `RegisterRef`, `BytecodeGenerator`), mais `set_label_location` partida em duas e consts `PROPERTY_*` fora da classe.
- Nomes sem resolução: 60 módulos inexistentes, 42 variantes de `OpcodeID` com grafia errada, 27 caminhos errados (item existe em outro módulo), 30 nomes inexistentes em todo o crate, 15 métodos sem definição, 3 `use` do topo de `bytecode_generator.rs` quebrados, e o casing `Tdz*` contra `TDZ*`.
