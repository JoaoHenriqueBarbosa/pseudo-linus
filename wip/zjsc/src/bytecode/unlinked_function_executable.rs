//! Porte de `JavaScriptCore/bytecode/UnlinkedFunctionExecutable.h` e `.cpp` (e do `Inlines.h`): o
//! `UnlinkedFunctionExecutable`, o enum `UnlinkedFunctionKind` e os tipos aninhados
//! `ClassElementDefinition` e `RareData`.
//!
//! Divergências (documentadas, nenhuma muda o que o programa JS observa):
//!
//! - Sem `JSCell`: é um struct comum, compartilhado como `UnlinkedFunctionExecutableRef`
//!   (`Rc<RefCell<..>>`, o mesmo modelo dos nós do parser). `create` devolve a referência; os
//!   `set*` do C++ (`setEcmaName`, `setClassSource`, `setClassElementDefinitions`,
//!   `recordParse`...) são `&mut self` e passam por `borrow_mut()`. `subspaceFor`, `destroy`,
//!   `visitChildren`, `reconcileWeakReferencesAtGCEnd`, `createStructure`, `DECLARE_EXPORT_INFO` e o
//!   `static_assert` de tamanho não existem (o `WriteBarrier<UnlinkedFunctionCodeBlock>` é a
//!   referência compartilhada, e o ciclo executável/bloco é desfeito por `clear_code`).
//! - Os bitfields guardam o enum ou o inteiro com a largura natural.
//! - `name()` devolve `Identifier` por valor: o C++ devolve `vm().propertyNames->nullIdentifier`
//!   quando não há nome, e aqui o nulo é `Identifier::null_identifier()`, sem precisar da VM.
//! - O cache de bytecode (`m_isGeneratedFromCache`, `m_isCached`, `m_decoder`, os offsets do
//!   `CachedFunctionExecutable`, `decodeCachedCodeBlocks`) não existe: o cache de bytecode do
//!   `CodeCache` é uma camada do runtime que ainda não foi portada.
//! - Ainda sem porte, por dependerem de camadas posteriores (`FunctionExecutable`, `CodeCache`,
//!   `BuiltinExecutables`, `Parser::parse<FunctionNode>` + `BytecodeGenerator::generate`):
//!   `unlinkedCodeBlockFor` (e o `generateUnlinkedFunctionCodeBlock` estático), `fromGlobalCode`,
//!   `linkedSourceCode` e `link`. `clearCode` não mexe no conjunto do heap
//!   (`unlinkedFunctionExecutableSpaceAndSet`), que não existe.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::executable_info::{
    DerivedContextType, EvalContextType, NeedsClassFieldInitializer,
};
use crate::bytecode::unlinked_code_block::UnlinkedFunctionCodeBlock;
use crate::parser::nodes::FunctionMetadataNode;
use crate::parser::parser_modes::{
    is_arrow_function_parse_mode, CodeFeatures, FunctionMode, JSParserScriptMode, LexicallyScopedFeatures,
    PrivateBrandRequirement, SourceParseMode, SuperBinding, NO_FEATURES, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::parser_tokens::JSTextPosition;
use crate::parser::source_code::SourceCode;
use crate::parser::variable_environment::{PrivateNameEnvironment, TDZEnvironmentLink};
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `UnlinkedFunctionExecutable::ClassElementDefinition::Kind` (`ClassElementDefinitionKind` no porte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClassElementDefinitionKind {
    #[default]
    FieldWithLiteralPropertyKey = 0,
    FieldWithComputedPropertyKey = 1,
    FieldWithPrivatePropertyKey = 2,
    StaticInitializationBlock = 3,
}

/// `UnlinkedFunctionExecutable::ClassElementDefinition`.
#[derive(Clone, Debug, Default)]
pub struct ClassElementDefinition {
    pub ident: Identifier,
    pub position: JSTextPosition,
    pub initializer_position: Option<JSTextPosition>,
    pub kind: ClassElementDefinitionKind,
}

/// `enum UnlinkedFunctionKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlinkedFunctionKind {
    UnlinkedNormalFunction,
    UnlinkedBuiltinFunction,
}

/// `UnlinkedFunctionExecutable::RareData`.
#[derive(Clone, Default)]
pub struct RareData {
    pub class_source: SourceCode,
    pub source_url_directive: WtfString,
    pub source_mapping_url_directive: WtfString,
    pub generator_or_async_wrapper_function_parameter_names: Vec<Identifier>,
    pub class_element_definitions: Vec<ClassElementDefinition>,
    pub parent_private_name_environment: PrivateNameEnvironment,
}

/// Referência compartilhada ao `UnlinkedFunctionExecutable*` (o `WriteBarrier<UnlinkedFunctionExecutable>`
/// do `UnlinkedCodeBlock`).
pub type UnlinkedFunctionExecutableRef = Rc<RefCell<UnlinkedFunctionExecutable>>;

/// `class UnlinkedFunctionExecutable`.
pub struct UnlinkedFunctionExecutable {
    first_line_offset: u32,
    line_count: u32,
    has_captured_variables: bool,
    unlinked_function_start: u32,
    is_builtin_function: bool,
    unlinked_body_start_column: u32,
    is_builtin_default_class_constructor: bool,
    unlinked_body_end_column: u32,
    construct_ability: ConstructAbility,
    start_offset: u32,
    script_mode: JSParserScriptMode,
    source_length: u32,
    super_binding: SuperBinding,
    parameters_start_offset: u32,
    unlinked_function_end: u32,
    needs_class_field_initializer: NeedsClassFieldInitializer,
    parameter_count: u32,
    singleton_has_been_invalidated: bool,
    private_brand_requirement: PrivateBrandRequirement,
    features: CodeFeatures,
    constructor_kind: ConstructorKind,
    source_parse_mode: SourceParseMode,
    implementation_visibility: ImplementationVisibility,
    lexically_scoped_features: LexicallyScopedFeatures,
    function_mode: FunctionMode,
    derived_context_type: DerivedContextType,
    inline_attribute: InlineAttribute,
    eval_context_type: EvalContextType,
    has_name: bool,

    unlinked_code_block_for_call: Option<Rc<RefCell<UnlinkedFunctionCodeBlock>>>,
    unlinked_code_block_for_construct: Option<Rc<RefCell<UnlinkedFunctionCodeBlock>>>,

    ecma_name: Identifier,
    parent_scope_tdz_variables: Option<Rc<TDZEnvironmentLink>>,

    rare_data: Option<Box<RareData>>,
}

impl UnlinkedFunctionExecutable {
    /// `static create(VM&, const SourceCode& source, FunctionMetadataNode*, UnlinkedFunctionKind, ConstructAbility,
    /// InlineAttribute, JSParserScriptMode, RefPtr<TDZEnvironmentLink>, Vector<Identifier>&&, std::optional<PrivateNameEnvironment>,
    /// DerivedContextType, EvalContextType, NeedsClassFieldInitializer, PrivateBrandRequirement, bool isBuiltinDefaultClassConstructor = false)`.
    /// O último argumento tem `false` como padrão no C++: `create_with_builtin_default_class_constructor`.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        vm: &VM,
        source: &SourceCode,
        node: &FunctionMetadataNode,
        unlinked_function_kind: UnlinkedFunctionKind,
        construct_ability: ConstructAbility,
        inline_attribute: InlineAttribute,
        script_mode: JSParserScriptMode,
        parent_scope_tdz_variables: Option<Rc<TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Vec<Identifier>,
        parent_private_name_environment: Option<PrivateNameEnvironment>,
        derived_context_type: DerivedContextType,
        eval_context_type: EvalContextType,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
    ) -> UnlinkedFunctionExecutableRef {
        Self::create_with_builtin_default_class_constructor(
            vm,
            source,
            node,
            unlinked_function_kind,
            construct_ability,
            inline_attribute,
            script_mode,
            parent_scope_tdz_variables,
            generator_or_async_wrapper_function_parameter_names,
            parent_private_name_environment,
            derived_context_type,
            eval_context_type,
            needs_class_field_initializer,
            private_brand_requirement,
            false,
        )
    }

    /// `create(...)` com o `isBuiltinDefaultClassConstructor` explícito, e o construtor privado do C++.
    #[allow(clippy::too_many_arguments)]
    pub fn create_with_builtin_default_class_constructor(
        _vm: &VM,
        parent_source: &SourceCode,
        node: &FunctionMetadataNode,
        kind: UnlinkedFunctionKind,
        construct_ability: ConstructAbility,
        inline_attribute: InlineAttribute,
        script_mode: JSParserScriptMode,
        parent_scope_tdz_variables: Option<Rc<TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Vec<Identifier>,
        parent_private_name_environment: Option<PrivateNameEnvironment>,
        derived_context_type: DerivedContextType,
        eval_context_type: EvalContextType,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
        is_builtin_default_class_constructor: bool,
    ) -> UnlinkedFunctionExecutableRef {
        let node_first_line = node.base.borrow().position.line;
        let node_source = node.source.borrow();
        let line_count = (node.last_line() as i32).wrapping_sub(node_first_line) as u32;
        let end_column = node.end_column.get();
        let start_column = node.start_column;
        let ident_is_null = node.ident.borrow().is_null();

        let mut executable = UnlinkedFunctionExecutable {
            first_line_offset: node_first_line.wrapping_sub(parent_source.first_line().one_based_int()) as u32,
            line_count,
            has_captured_variables: false,
            unlinked_function_start: node.function_start,
            is_builtin_function: kind == UnlinkedFunctionKind::UnlinkedBuiltinFunction,
            unlinked_body_start_column: start_column,
            is_builtin_default_class_constructor,
            unlinked_body_end_column: if line_count != 0 { end_column } else { end_column.wrapping_sub(start_column) },
            construct_ability,
            start_offset: node_source.start_offset().wrapping_sub(parent_source.start_offset()) as u32,
            script_mode,
            source_length: node_source.length() as u32,
            super_binding: node.super_binding,
            parameters_start_offset: node.parameters_start as u32,
            unlinked_function_end: (node.start_start_offset as u32)
                .wrapping_add(node_source.length() as u32)
                .wrapping_sub(1),
            needs_class_field_initializer,
            parameter_count: node.parameter_count,
            singleton_has_been_invalidated: false,
            private_brand_requirement,
            features: NO_FEATURES,
            constructor_kind: node.constructor_kind,
            source_parse_mode: node.parse_mode,
            implementation_visibility: node.implementation_visibility,
            lexically_scoped_features: node.lexically_scoped_features,
            function_mode: node.function_mode.get(),
            derived_context_type,
            inline_attribute,
            eval_context_type,
            has_name: !ident_is_null,
            unlinked_code_block_for_call: None,
            unlinked_code_block_for_construct: None,
            ecma_name: node.ecma_name(),
            parent_scope_tdz_variables,
            rare_data: None,
        };
        drop(node_source);

        debug_assert!(ident_is_null || *node.ident.borrow() == node.ecma_name());
        debug_assert!(!(executable.is_builtin_default_class_constructor && executable.constructor_kind() == ConstructorKind::None));
        debug_assert!(
            executable.needs_class_field_initializer == NeedsClassFieldInitializer::No
                || executable.is_class_constructor_function()
                || derived_context_type == DerivedContextType::DerivedConstructorContext
        );
        if !node.class_source.borrow().is_null() {
            executable.set_class_source(node.class_source.borrow().clone());
        }
        if !generator_or_async_wrapper_function_parameter_names.is_empty() {
            executable.ensure_rare_data().generator_or_async_wrapper_function_parameter_names =
                generator_or_async_wrapper_function_parameter_names;
        }
        if let Some(environment) = parent_private_name_environment {
            executable.ensure_rare_data().parent_private_name_environment = environment;
        }
        Rc::new(RefCell::new(executable))
    }

    /// `ensureRareData()` (e `ensureRareDataSlow`).
    fn ensure_rare_data(&mut self) -> &mut RareData {
        self.rare_data.get_or_insert_with(Box::default)
    }

    /// `name()`: o `ecmaName` quando a função tem nome, senão o identificador nulo.
    pub fn name(&self) -> Identifier {
        if self.has_name {
            return self.ecma_name.clone();
        }
        Identifier::null_identifier()
    }

    pub fn ecma_name(&self) -> &Identifier {
        &self.ecma_name
    }

    pub fn set_ecma_name(&mut self, name: Identifier) {
        debug_assert!(!self.has_name || name == self.ecma_name);
        self.ecma_name = name;
    }

    /// Sem o `this`.
    pub fn parameter_count(&self) -> u32 {
        self.parameter_count
    }

    pub fn parse_mode(&self) -> SourceParseMode {
        self.source_parse_mode
    }

    pub fn class_source(&self) -> SourceCode {
        match &self.rare_data {
            Some(rare_data) => rare_data.class_source.clone(),
            None => SourceCode::default(),
        }
    }

    pub fn set_class_source(&mut self, source: SourceCode) {
        self.ensure_rare_data().class_source = source;
    }

    pub fn is_in_strict_context(&self) -> bool {
        self.lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE != 0
    }

    pub fn function_mode(&self) -> FunctionMode {
        self.function_mode
    }

    pub fn constructor_kind(&self) -> ConstructorKind {
        self.constructor_kind
    }

    pub fn super_binding(&self) -> SuperBinding {
        self.super_binding
    }

    pub fn line_count(&self) -> u32 {
        self.line_count
    }

    pub fn linked_start_column(&self, parent_start_column: u32) -> u32 {
        self.unlinked_body_start_column + if self.first_line_offset == 0 { parent_start_column } else { 1 }
    }

    pub fn linked_end_column(&self, start_column: u32) -> u32 {
        self.unlinked_body_end_column + if self.line_count == 0 { start_column } else { 1 }
    }

    pub fn unlinked_function_start(&self) -> u32 {
        self.unlinked_function_start
    }

    pub fn unlinked_function_end(&self) -> u32 {
        self.unlinked_function_end
    }

    pub fn unlinked_body_start_column(&self) -> u32 {
        self.unlinked_body_start_column
    }

    pub fn unlinked_body_end_column(&self) -> u32 {
        self.unlinked_body_end_column
    }

    pub fn start_offset(&self) -> u32 {
        self.start_offset
    }

    pub fn source_length(&self) -> u32 {
        self.source_length
    }

    pub fn parameters_start_offset(&self) -> u32 {
        self.parameters_start_offset
    }

    pub fn first_line_offset(&self) -> u32 {
        self.first_line_offset
    }

    /// `clearCode(VM&)`: sem o conjunto do heap (ver o topo do módulo).
    pub fn clear_code(&mut self) {
        self.unlinked_code_block_for_call = None;
        self.unlinked_code_block_for_construct = None;
    }

    /// `m_unlinkedCodeBlockForCall` / `m_unlinkedCodeBlockForConstruct`.
    pub fn unlinked_code_block_for_call(&self) -> Option<&Rc<RefCell<UnlinkedFunctionCodeBlock>>> {
        self.unlinked_code_block_for_call.as_ref()
    }

    pub fn unlinked_code_block_for_construct(&self) -> Option<&Rc<RefCell<UnlinkedFunctionCodeBlock>>> {
        self.unlinked_code_block_for_construct.as_ref()
    }

    pub fn record_parse(
        &mut self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
    ) {
        self.features = features;
        self.lexically_scoped_features = lexically_scoped_features;
        self.has_captured_variables = has_captured_variables;
    }

    pub fn features(&self) -> CodeFeatures {
        self.features
    }

    pub fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        self.lexically_scoped_features
    }

    pub fn has_captured_variables(&self) -> bool {
        self.has_captured_variables
    }

    pub fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        self.private_brand_requirement
    }

    pub fn implementation_visibility(&self) -> ImplementationVisibility {
        self.implementation_visibility
    }

    pub fn is_builtin_function(&self) -> bool {
        self.is_builtin_function
    }

    pub fn construct_ability(&self) -> ConstructAbility {
        self.construct_ability
    }

    pub fn script_mode(&self) -> JSParserScriptMode {
        self.script_mode
    }

    pub fn is_class_constructor_function(&self) -> bool {
        match self.constructor_kind() {
            ConstructorKind::None | ConstructorKind::Naked => false,
            ConstructorKind::Base | ConstructorKind::Extends => true,
        }
    }

    pub fn is_class(&self) -> bool {
        match &self.rare_data {
            None => false,
            Some(rare_data) => !rare_data.class_source.is_null(),
        }
    }

    pub fn is_builtin_default_class_constructor(&self) -> bool {
        self.is_builtin_default_class_constructor
    }

    pub fn parent_scope_tdz_variables(&self) -> Option<Rc<TDZEnvironmentLink>> {
        self.parent_scope_tdz_variables.clone()
    }

    /// `generatorOrAsyncWrapperFunctionParameterNames()`: nulo quando não há dados raros.
    pub fn generator_or_async_wrapper_function_parameter_names(&self) -> Option<&Vec<Identifier>> {
        self.rare_data.as_ref().map(|rare_data| &rare_data.generator_or_async_wrapper_function_parameter_names)
    }

    /// `parentPrivateNameEnvironment()`: nulo quando não há dados raros.
    pub fn parent_private_name_environment(&self) -> Option<&PrivateNameEnvironment> {
        self.rare_data.as_ref().map(|rare_data| &rare_data.parent_private_name_environment)
    }

    pub fn is_arrow_function(&self) -> bool {
        is_arrow_function_parse_mode(self.parse_mode())
    }

    pub fn singleton_has_been_invalidated(&self) -> bool {
        self.singleton_has_been_invalidated
    }

    pub fn set_singleton_has_been_invalidated(&mut self) {
        self.singleton_has_been_invalidated = true;
    }

    pub fn derived_context_type(&self) -> DerivedContextType {
        self.derived_context_type
    }

    pub fn eval_context_type(&self) -> EvalContextType {
        self.eval_context_type
    }

    pub fn inline_attribute(&self) -> InlineAttribute {
        self.inline_attribute
    }

    pub fn source_url_directive(&self) -> WtfString {
        match &self.rare_data {
            Some(rare_data) => rare_data.source_url_directive.clone(),
            None => WtfString::default(),
        }
    }

    pub fn source_mapping_url_directive(&self) -> WtfString {
        match &self.rare_data {
            Some(rare_data) => rare_data.source_mapping_url_directive.clone(),
            None => WtfString::default(),
        }
    }

    pub fn set_source_url_directive(&mut self, source_url: WtfString) {
        self.ensure_rare_data().source_url_directive = source_url;
    }

    pub fn set_source_mapping_url_directive(&mut self, source_mapping_url: WtfString) {
        self.ensure_rare_data().source_mapping_url_directive = source_mapping_url;
    }

    pub fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        self.needs_class_field_initializer
    }

    pub fn class_element_definitions(&self) -> Option<&Vec<ClassElementDefinition>> {
        self.rare_data.as_ref().map(|rare_data| &rare_data.class_element_definitions)
    }

    pub fn set_class_element_definitions(&mut self, class_element_definitions: Vec<ClassElementDefinition>) {
        if class_element_definitions.is_empty() {
            return;
        }
        self.ensure_rare_data().class_element_definitions = class_element_definitions;
    }
}
