//! Porte de `bytecompiler/BytecodeGenerator.h` e `BytecodeGenerator.cpp`.
//!
//! Esta primeira parte traz as classes auxiliares (`CallArguments`, `Variable`, `FinallyContext`,
//! `ForInContext`, `TryData` e afins) e os métodos inline do `BytecodeGenerator` até `emitNode`.
//! O `struct BytecodeGenerator` com os campos fica em `bytecode_generator_part2.rs`; aqui os métodos
//! usam os campos pelos nomes em snake_case do `m_` correspondente.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::bytecode::handler_info::HandlerType;
use crate::bytecompiler::label::Label;
use crate::bytecompiler::label_scope::{LabelScope, LabelScopeType};
use crate::bytecompiler::register_id::RegisterID;
use crate::parser::nodes::{ArgumentsNode, NodeRef, Statement};
use crate::runtime::identifier::Identifier;
use crate::runtime::property_attribute::READ_ONLY;
use crate::runtime::var_offset::VarOffset;

pub use crate::bytecompiler::bytecode_generator_part2::BytecodeGenerator;

/// `RefPtr<RegisterID>` do C++: posse compartilhada, identidade por ponteiro.
pub type RegisterRef = Rc<RefCell<RegisterID>>;

/// `enum ExpectedFunction`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpectedFunction {
    NoExpectedFunction,
    ExpectObjectConstructor,
    ExpectArrayConstructor,
}

/// `enum class EmitAwait : bool`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmitAwait {
    No,
    Yes,
}

/// `enum class DebuggableCall : bool`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebuggableCall {
    No,
    Yes,
}

/// `enum class ThisResolutionType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThisResolutionType {
    Local,
    Scoped,
}

/// `enum class InvalidPrototypeMode : uint8_t`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum InvalidPrototypeMode {
    Throw,
    Ignore,
}

/// `class CallArguments`. `m_argv` é um `std::span` sobre `m_allocatedRegisters`; aqui os registros
/// alocados são o próprio vetor e `argv` guarda o mesmo conteúdo na ordem (this primeiro).
pub struct CallArguments {
    pub arguments_node: Option<NodeRef<ArgumentsNode>>,
    pub argv: Vec<Option<RegisterRef>>,
    pub allocated_registers: Vec<Option<RegisterRef>>,
}

impl CallArguments {
    pub fn this_register(&self) -> Option<RegisterRef> {
        self.argv[0].clone()
    }

    pub fn argument_register(&self, i: usize) -> Option<RegisterRef> {
        self.argv[i + 1].clone()
    }

    pub fn stack_offset(&self) -> u32 {
        let index = match &self.argv[0] {
            Some(register) => register.borrow().index(),
            None => 0,
        };
        (-index + crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS) as u32
    }

    pub fn argument_count_including_this(&self) -> u32 {
        self.argv.len() as u32
    }

    pub fn arguments_node(&self) -> Option<NodeRef<ArgumentsNode>> {
        self.arguments_node.clone()
    }
}

/// `Variable::VariableKind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VariableKind {
    NormalVariable,
    SpecialVariable,
}

/// `class Variable`. `m_local` é `RegisterID*` (identidade por ponteiro), logo `Option<RegisterRef>`.
#[derive(Clone)]
pub struct Variable {
    pub ident: Identifier,
    pub offset: VarOffset,
    pub local: Option<RegisterRef>,
    pub attributes: u32,
    pub kind: VariableKind,
    pub symbol_table_constant_index: i32,
    pub is_lexically_scoped: bool,
}

impl Default for Variable {
    fn default() -> Self {
        Variable {
            ident: Identifier::empty_identifier(),
            offset: VarOffset::default(),
            local: None,
            attributes: 0,
            kind: VariableKind::NormalVariable,
            symbol_table_constant_index: 0,
            is_lexically_scoped: false,
        }
    }
}

impl Variable {
    pub fn from_ident(ident: &Identifier) -> Variable {
        Variable {
            ident: ident.clone(),
            offset: VarOffset::default(),
            local: None,
            attributes: 0,
            kind: VariableKind::NormalVariable, // Sem significado para este tipo de Variable.
            symbol_table_constant_index: 0, // Sem significado para este tipo de Variable.
            is_lexically_scoped: false,
        }
    }

    pub fn new(
        ident: &Identifier,
        offset: VarOffset,
        local: Option<RegisterRef>,
        attributes: u32,
        kind: VariableKind,
        symbol_table_constant_index: i32,
        is_lexically_scoped: bool,
    ) -> Variable {
        Variable {
            ident: ident.clone(),
            offset,
            local,
            attributes,
            kind,
            symbol_table_constant_index,
            is_lexically_scoped,
        }
    }

    // Se não definido, é uma variável sem escopo local. Se definido, pode ser uma variável de pilha,
    // uma variável com escopo num escopo local, ou uma variável capturada no objeto de arguments direto.
    pub fn is_resolved(&self) -> bool {
        self.offset.is_set()
    }

    pub fn symbol_table_constant_index(&self) -> i32 {
        debug_assert!(self.is_resolved() && !self.is_special());
        self.symbol_table_constant_index
    }

    pub fn ident(&self) -> &Identifier {
        &self.ident
    }

    pub fn offset(&self) -> VarOffset {
        self.offset
    }

    pub fn is_local(&self) -> bool {
        self.offset.is_stack()
    }

    pub fn local(&self) -> Option<RegisterRef> {
        self.local.clone()
    }

    pub fn is_read_only(&self) -> bool {
        self.attributes & READ_ONLY != 0
    }

    pub fn is_special(&self) -> bool {
        self.kind != VariableKind::NormalVariable
    }

    pub fn is_const(&self) -> bool {
        self.is_read_only() && self.is_lexically_scoped
    }

    pub fn set_is_read_only(&mut self) {
        self.attributes |= READ_ONLY;
    }
}

/// `operator==(const Variable&, const Variable&) = default`: compara membro a membro, com o
/// `RegisterID*` por ponteiro.
impl PartialEq for Variable {
    fn eq(&self, other: &Variable) -> bool {
        let same_local = match (&self.local, &other.local) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        self.ident == other.ident
            && self.offset == other.offset
            && same_local
            && self.attributes == other.attributes
            && self.kind == other.kind
            && self.symbol_table_constant_index == other.symbol_table_constant_index
            && self.is_lexically_scoped == other.is_lexically_scoped
    }
}

// https://tc39.github.io/ecma262/#sec-completion-record-specification-type
//
// Nos casos Break e Continue, em vez dos valores Break e Continue do enum abaixo, o jumpID único do
// break/continue serve de codificação do CompletionType. O emitFinallyCompletion() usa esse jumpID depois
// para achar o alvo do salto após executar os blocos finally. O jumpID é:
//     jumpID = bytecodeOffset (do nó break/continue) + CompletionType::NumberOfTypes.
// Logo não há colisão entre jumpIDs e os valores do enum.
/// `enum class CompletionType : int`. O valor é um `i32` porque carrega jumpIDs além das três variantes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompletionType(pub i32);

impl CompletionType {
    pub const NORMAL: CompletionType = CompletionType(0);
    pub const THROW: CompletionType = CompletionType(1);
    pub const RETURN: CompletionType = CompletionType(2);
    pub const NUMBER_OF_TYPES: CompletionType = CompletionType(3);
}

pub fn bytecode_offset_to_jump_id(offset: u32) -> CompletionType {
    let jump_id_as_int = offset as i32 + CompletionType::NUMBER_OF_TYPES.0;
    debug_assert!(jump_id_as_int >= CompletionType::NUMBER_OF_TYPES.0);
    CompletionType(jump_id_as_int)
}

/// `struct FinallyJump`. `Ref<Label>` vira `Rc<Label>`.
pub struct FinallyJump {
    pub jump_id: CompletionType,
    pub target_lexical_scope_index: i32,
    pub target_label: Rc<Label>,
}

impl FinallyJump {
    pub fn new(jump_id: CompletionType, target_lexical_scope_index: i32, target_label: Rc<Label>) -> FinallyJump {
        FinallyJump { jump_id, target_lexical_scope_index, target_label }
    }
}

/// O `struct { typeRegister; valueRegister; } m_completionRecord` de `FinallyContext`.
#[derive(Default)]
pub struct CompletionRecord {
    pub type_register: Option<RegisterRef>,
    pub value_register: Option<RegisterRef>,
}

/// `class FinallyContext`. O construtor `FinallyContext(BytecodeGenerator&, Label&)` está no `.cpp`.
/// `m_outerContext` é um ponteiro para o contexto externo da pilha de escopos de controle; vira
/// `Option<Rc<RefCell<FinallyContext>>>`.
#[derive(Default)]
pub struct FinallyContext {
    pub outer_context: Option<Rc<RefCell<FinallyContext>>>,
    pub finally_label: Option<Rc<Label>>,
    pub number_of_breaks_or_continues: u32,
    pub handles_returns: bool,
    pub jumps: Vec<FinallyJump>,
    pub completion_record: CompletionRecord,
}

impl FinallyContext {
    pub fn outer_context(&self) -> Option<Rc<RefCell<FinallyContext>>> {
        self.outer_context.clone()
    }

    pub fn finally_label(&self) -> Option<Rc<Label>> {
        self.finally_label.clone()
    }

    pub fn completion_type_register(&self) -> Option<RegisterRef> {
        self.completion_record.type_register.clone()
    }

    pub fn completion_value_register(&self) -> Option<RegisterRef> {
        self.completion_record.value_register.clone()
    }

    pub fn number_of_breaks_or_continues(&self) -> u32 {
        self.number_of_breaks_or_continues
    }

    /// `Checked<uint32_t, CrashOnOverflow>::operator++`: estouro derruba o processo.
    pub fn inc_number_of_breaks_or_continues(&mut self) {
        self.number_of_breaks_or_continues = self
            .number_of_breaks_or_continues
            .checked_add(1)
            .expect("CheckedArithmetic: estouro em numberOfBreaksOrContinues");
    }

    pub fn handles_returns(&self) -> bool {
        self.handles_returns
    }

    pub fn set_handles_returns(&mut self) {
        self.handles_returns = true;
    }

    pub fn register_jump(&mut self, jump_id: CompletionType, lexical_scope_index: i32, target_label: Rc<Label>) {
        self.jumps.push(FinallyJump::new(jump_id, lexical_scope_index, target_label));
    }

    pub fn number_of_jumps(&self) -> usize {
        self.jumps.len()
    }

    pub fn jumps(&mut self, i: usize) -> &mut FinallyJump {
        &mut self.jumps[i]
    }
}

/// `ControlFlowScope::Type` (`uint8_t`) e as constantes anônimas `Label`, `Finally`.
pub type ControlFlowScopeType = u8;
pub const CONTROL_FLOW_SCOPE_LABEL: ControlFlowScopeType = 0;
pub const CONTROL_FLOW_SCOPE_FINALLY: ControlFlowScopeType = 1;

/// `struct ControlFlowScope`.
#[derive(Clone)]
pub struct ControlFlowScope {
    pub type_: ControlFlowScopeType,
    pub lexical_scope_index: i32,
    pub finally_context: Option<Rc<RefCell<FinallyContext>>>,
}

impl ControlFlowScope {
    pub fn new(type_: ControlFlowScopeType, lexical_scope_index: i32, finally_context: Option<Rc<RefCell<FinallyContext>>>) -> ControlFlowScope {
        ControlFlowScope { type_, lexical_scope_index, finally_context }
    }

    pub fn is_label_scope(&self) -> bool {
        self.type_ == CONTROL_FLOW_SCOPE_LABEL
    }

    pub fn is_finally_scope(&self) -> bool {
        self.type_ == CONTROL_FLOW_SCOPE_FINALLY
    }
}

/// `ForInContext::GetInst` (e `PutInst`, `InInst`): índice da instrução e índice do registro da propriedade.
pub type ForInGetInst = (u32, i32);
pub type ForInPutInst = ForInGetInst;
pub type ForInInInst = ForInGetInst;
/// `ForInContext::HasOwnPropertyJumpInst`: índice do desvio e alvo do caminho genérico.
pub type ForInHasOwnPropertyJumpInst = (u32, u32);

/// `class ForInContext : public RefCounted<ForInContext>`. Quem compartilha usa `Rc<RefCell<ForInContext>>`.
/// O método `finalize(BytecodeGenerator&, UnlinkedCodeBlockGenerator*, unsigned)` está no `.cpp`.
pub struct ForInContext {
    pub local_register: Option<RegisterRef>,
    pub property_name: Option<RegisterRef>,
    pub property_offset: Option<RegisterRef>,
    pub enumerator: Option<RegisterRef>,
    pub mode: Option<RegisterRef>,
    pub base_variable: Option<Variable>,
    pub is_valid: bool,
    pub body_bytecode_start_offset: u32,
    pub in_insts: Vec<ForInInInst>,
    pub get_insts: Vec<ForInGetInst>,
    pub put_insts: Vec<ForInPutInst>,
    pub has_own_property_jump_insts: Vec<ForInHasOwnPropertyJumpInst>,
}

impl ForInContext {
    pub fn new(
        local_register: Option<RegisterRef>,
        property_name: Option<RegisterRef>,
        property_offset: Option<RegisterRef>,
        enumerator: Option<RegisterRef>,
        mode: Option<RegisterRef>,
        base_variable: Option<Variable>,
        body_bytecode_start_offset: u32,
    ) -> ForInContext {
        ForInContext {
            local_register,
            property_name,
            property_offset,
            enumerator,
            mode,
            base_variable,
            is_valid: true,
            body_bytecode_start_offset,
            in_insts: Vec::new(),
            get_insts: Vec::new(),
            put_insts: Vec::new(),
            has_own_property_jump_insts: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    pub fn invalidate(&mut self) {
        self.is_valid = false;
    }

    pub fn local(&self) -> Option<RegisterRef> {
        self.local_register.clone()
    }

    pub fn property_name(&self) -> Option<RegisterRef> {
        self.property_name.clone()
    }

    pub fn property_offset(&self) -> Option<RegisterRef> {
        self.property_offset.clone()
    }

    pub fn enumerator(&self) -> Option<RegisterRef> {
        self.enumerator.clone()
    }

    pub fn mode(&self) -> Option<RegisterRef> {
        self.mode.clone()
    }

    pub fn base_variable(&self) -> &Option<Variable> {
        &self.base_variable
    }

    pub fn add_get_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.get_insts.push((inst_index, property_reg_index));
    }

    pub fn add_put_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.put_insts.push((inst_index, property_reg_index));
    }

    pub fn add_in_inst(&mut self, inst_index: u32, property_reg_index: i32) {
        self.in_insts.push((inst_index, property_reg_index));
    }

    pub fn add_has_own_property_jump(&mut self, branch_inst_index: u32, generic_path_target: u32) {
        self.has_own_property_jump_insts.push((branch_inst_index, generic_path_target));
    }

    pub fn body_bytecode_start_offset(&self) -> u32 {
        self.body_bytecode_start_offset
    }
}

/// `struct TryData`. O `TryData*` do C++ aponta para um objeto que vive num `SegmentedVector` do
/// gerador e é alterado depois; por isso o compartilhamento é `Rc<RefCell<TryData>>`.
pub struct TryData {
    pub target: Rc<Label>,
    pub handler_type: HandlerType,
}

/// `struct TryContext`.
pub struct TryContext {
    pub start: Rc<Label>,
    pub try_data: Rc<RefCell<TryData>>,
}

/// `struct TryRange`.
pub struct TryRange {
    pub start: Rc<Label>,
    pub end: Rc<Label>,
    pub try_data: Rc<RefCell<TryData>>,
}

/// `struct UsingSlot`.
#[derive(Default)]
pub struct UsingSlot {
    pub value: Option<RegisterRef>,
    pub method: Option<RegisterRef>,
    pub reached: Option<RegisterRef>,
    pub is_async: bool,
}

/// `struct UsingScope`.
#[derive(Default)]
pub struct UsingScope {
    pub slots: Vec<UsingSlot>,
    pub next_slot: u32,
    pub has_await_using: bool,
}

/// `struct JSGeneratorTraits`: os tipos associados vivem no trait `BytecodeGeneratorTraits`
/// (ver `bytecode_generator_base`); aqui a marca do tipo e a constante `opcodeForDisablingOptimizations`.
pub struct JSGeneratorTraits;

impl JSGeneratorTraits {
    pub const OPCODE_FOR_DISABLING_OPTIMIZATIONS: crate::bytecode::opcode::OpcodeID = crate::bytecode::opcode::OpcodeID::op_debug;
}

/// Contrato dos quatro construtores do `BytecodeGenerator` (Program, Function, Eval, ModuleProgram),
/// usado pelo `generate` genérico. A implementação por nó vive junto do `.cpp`.
pub trait BytecodeGeneratorNode<UnlinkedCodeBlock> {
    #[allow(clippy::too_many_arguments)]
    fn new_generator(
        vm: &crate::runtime::vm::VM,
        node: &NodeRef<Self>,
        unlinked_code_block: &Rc<RefCell<UnlinkedCodeBlock>>,
        code_generation_mode: crate::bytecode::code_generation_mode::CodeGenerationMode,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::bytecode::private_name_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator
    where
        Self: Sized;
}

// Dados que o `.cpp` inclui no gerador e que o cabeçalho só declara.
thread_local! {
    /// Conta os `TryData` criados, para o `SegmentedVector<TryData, 8>` do `.cpp` manter índices estáveis.
    pub static TRY_DATA_COUNTER: Cell<usize> = const { Cell::new(0) };
}

impl BytecodeGenerator {
    pub fn vm(&self) -> &crate::runtime::vm::VM {
        &self.vm
    }

    pub fn property_names(&self) -> &crate::runtime::common_identifiers::CommonIdentifiers {
        self.vm.property_names()
    }

    pub fn is_constructor(&self) -> bool {
        self.code_block.borrow().is_constructor()
    }

    pub fn derived_context_type(&self) -> crate::parser::parser_modes::DerivedContextType {
        self.derived_context_type
    }

    pub fn uses_arrow_function(&self) -> bool {
        self.scope_node.uses_arrow_function()
    }

    pub fn needs_to_update_arrow_function_context(&self) -> bool {
        self.needs_to_update_arrow_function_context
    }

    pub fn uses_eval(&self) -> bool {
        self.scope_node.uses_eval()
    }

    pub fn uses_this(&self) -> bool {
        self.scope_node.uses_this()
    }

    pub fn is_function_node(&self) -> bool {
        self.scope_node.is_function_node()
    }

    pub fn has_shadows_arguments_code_feature(&self) -> bool {
        self.scope_node.has_shadows_arguments_feature()
    }

    pub fn is_async_function_without_await(&self) -> bool {
        self.scope_node.is_async_function_without_await()
    }

    pub fn lexically_scoped_features(&self) -> crate::parser::parser_modes::LexicallyScopedFeatures {
        self.scope_node.lexically_scoped_features()
    }

    pub fn private_brand_requirement(&self) -> crate::bytecode::executable_info::PrivateBrandRequirement {
        self.code_block.borrow().private_brand_requirement()
    }

    pub fn constructor_kind(&self) -> crate::runtime::constructor_kind::ConstructorKind {
        self.code_block.borrow().constructor_kind()
    }

    pub fn super_binding(&self) -> crate::bytecode::executable_info::SuperBinding {
        self.code_block.borrow().super_binding()
    }

    pub fn script_mode(&self) -> crate::parser::parser_modes::JSParserScriptMode {
        self.code_block.borrow().script_mode()
    }

    pub fn needs_class_field_initializer(&self) -> crate::bytecode::executable_info::NeedsClassFieldInitializer {
        self.code_block.borrow().needs_class_field_initializer()
    }

    /// `static ParserError generate(...)`. O ramo `Options::reportBytecodeCompileTimes()` só imprime
    /// diagnóstico com `dataLogLn` (saída de depuração do processo, sem efeito observável pelo programa
    /// JS), logo some; `DeferGC` é a guarda de coleta e vive no `VM`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_for<Node: BytecodeGeneratorNode<UnlinkedCodeBlock>, UnlinkedCodeBlock>(
        vm: &crate::runtime::vm::VM,
        node: &NodeRef<Node>,
        _source_code: &crate::parser::source_code::SourceCode,
        unlinked_code_block: &Rc<RefCell<UnlinkedCodeBlock>>,
        code_generation_mode: crate::bytecode::code_generation_mode::CodeGenerationMode,
        parent_scope_tdz_variables: &Option<Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&[Identifier]>,
        private_name_environment: Option<&crate::bytecode::private_name_environment::PrivateNameEnvironment>,
    ) -> crate::parser::parser_error::ParserError {
        let _defer_gc = vm.defer_gc();
        let mut bytecode_generator = Node::new_generator(
            vm,
            node,
            unlinked_code_block,
            code_generation_mode,
            parent_scope_tdz_variables,
            generator_or_async_wrapper_function_parameter_names,
            private_name_environment,
        );
        let mut size = 0u32;
        bytecode_generator.generate(&mut size)
    }

    // Retorna o registro que guarda "this".
    pub fn this_register(&self) -> RegisterRef {
        self.this_register.clone()
    }

    pub fn arguments_register(&self) -> Option<RegisterRef> {
        self.arguments_register.clone()
    }

    pub fn new_target(&self) -> RegisterRef {
        debug_assert!(self.new_target_register.is_some());
        self.new_target_register.clone().expect("newTargetRegister ausente")
    }

    pub fn scope_register(&self) -> Option<RegisterRef> {
        self.scope_register.clone()
    }

    pub fn generator_register(&self) -> Option<RegisterRef> {
        self.generator_register.clone()
    }

    pub fn promise_register(&self) -> Option<RegisterRef> {
        self.promise_register.clone()
    }

    // Igual a newTemporary(), mas devolve "suggestion" se ela for temporária. Serve quando você pôs
    // "suggestion" num RefPtr, mas quer permitir que a próxima instrução a sobrescreva mesmo assim.
    pub fn new_temporary_or(&mut self, suggestion: &RegisterRef) -> RegisterRef {
        if suggestion.borrow().is_temporary() {
            suggestion.clone()
        } else {
            self.new_temporary()
        }
    }

    // Funções para tratar o registro dst

    pub fn ignored_result(&self) -> RegisterRef {
        self.ignored_result_register.clone()
    }

    // Retorna um lugar para escrever valores intermediários de uma operação, reaproveitando dst se for
    // seguro.
    pub fn temp_destination(&mut self, dst: Option<&RegisterRef>) -> RegisterRef {
        match dst {
            Some(d) if !Rc::ptr_eq(d, &self.ignored_result_register) && d.borrow().is_temporary() => d.clone(),
            _ => self.new_temporary(),
        }
    }

    // Retorna o lugar onde escrever a saída final de uma operação.
    pub fn final_destination(&mut self, original_dst: Option<&RegisterRef>, temp_dst: Option<&RegisterRef>) -> RegisterRef {
        if let Some(d) = original_dst {
            if !Rc::ptr_eq(d, &self.ignored_result_register) {
                return d.clone();
            }
        }
        if let Some(t) = temp_dst {
            debug_assert!(!Rc::ptr_eq(t, &self.ignored_result_register));
            if t.borrow().is_temporary() {
                return t.clone();
            }
        }
        self.new_temporary()
    }

    pub fn destination_for_assign_result(&mut self, dst: Option<&RegisterRef>) -> Option<RegisterRef> {
        if let Some(d) = dst {
            if !Rc::ptr_eq(d, &self.ignored_result_register) {
                return Some(if d.borrow().is_temporary() { d.clone() } else { self.new_temporary() });
            }
        }
        None
    }

    // Move src para dst se dst não for nulo e for diferente de src, senão só devolve src.
    pub fn move_register(&mut self, dst: Option<&RegisterRef>, src: &RegisterRef) -> Option<RegisterRef> {
        match dst {
            Some(d) if Rc::ptr_eq(d, &self.ignored_result_register) => None,
            Some(d) if !Rc::ptr_eq(d, src) => self.emit_move(d, src),
            _ => Some(src.clone()),
        }
    }

    pub fn new_label_scope(&mut self, type_: LabelScopeType, name: Option<&Identifier>) -> Rc<LabelScope> {
        self.new_label_scope_impl(type_, name)
    }

    pub fn emit_node(&mut self, dst: Option<&RegisterRef>, n: &Statement) {
        let tail_position_poisoner = self.allow_tail_call_optimization;
        self.allow_tail_call_optimization = false;
        let call_ignore_result_position_poisoner = self.allow_call_ignore_result_optimization;
        self.allow_call_ignore_result_optimization = false;
        self.emit_node_in_tail_position(dst, n);
        // `SetForScope`: restaura os valores ao sair do escopo.
        self.allow_call_ignore_result_optimization = call_ignore_result_position_poisoner;
        self.allow_tail_call_optimization = tail_position_poisoner;
    }
}

// Continua em bytecode_generator_part2.rs (linha 495 do .h em diante).
