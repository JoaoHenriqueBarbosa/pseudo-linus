//! Tradução de `JavaScriptCore/parser/ASTBuilder.h`, primeira fatia (linhas 1 a 852: a struct, os tipos
//! associados e os métodos de `createSourceElements` até `createImportDeclaration`). A segunda fatia fica
//! em `ast_builder_part2.rs`, incluída por `include!` dentro do `impl TreeBuilder for ASTBuilder`.
//!
//! Modelo de posse (CONVENTIONS, modelo de dados, item 3). O `Parser` guarda e copia os resultados do
//! construtor (`head`/`tail`, o mesmo nó visto como `Expression` e como `Comma`, o `base` de uma cadeia
//! opcional, o `PropertyList` cuja cauda é passada de volta ao `createPropertyList`), então os tipos
//! associados não podem ser os `Box` da árvore. Cada um é um `NodeRef<T>`: uma alça `Rc<RefCell<Option<T>>>`
//! com valor nulo (`Default`, o `nullptr` do C++) e igualdade por identidade (o `==` de ponteiros). Ao
//! montar o nó pai, o filho é movido da alça com `take()`; a alça fica "movida" e qualquer acesso depois
//! disso é `RELEASE_ASSERT` (panic). As listas encadeadas (`PropertyList`, `ElementList`, `ArgumentsList`,
//! `ClauseList`, as de template) são `NodeRef<NodeList<_>>`: cabeça e cauda são a MESMA alça, o anexo
//! empilha no `NodeList` compartilhado e o `into_head()` liga os nós quando a lista vira filha de outro nó.
//! Os tipos que o C++ trata como ponteiros do mesmo nó com tipos estáticos diferentes são o mesmo tipo
//! aqui: `Comma`, `ClassExpression` e `DefineField` são `Expression`/`Statement`; `ArrayPattern`,
//! `ObjectPattern`, `RestPattern` e `DestructuringPattern` são todos `PatternRef`.
//!
//! Desvios em relação ao C++ (todos mecânicos):
//!
//! - `ASTBuilder(VM&, ParserArena&, SourceCode*)` não guarda a arena nem o ponteiro: a arena só servia à
//!   alocação (que sumiu) e o `SourceCode` é copiado (um `Rc` por baixo). Assim o `Parser` não mantém
//!   empréstimo vivo de `self` enquanto o construtor existe.
//! - `FunctionBody` é `FunctionBodyRef`, que embrulha o `Rc<FunctionMetadataNode>` compartilhado (o
//!   `Rc<T>` sozinho não tem `Default` porque `FunctionMetadataNode` não tem).
//! - `BinaryExprContext` e `UnaryExprContext` não fazem nada no C++ (construtor vazio): são tipos unitários.
//! - `const Identifier*` que o C++ desreferencia sem teste (`*functionInfo.name`, `*classInfo.className`,
//!   `*propertyName`) é `RELEASE_ASSERT` de que a opção está preenchida.

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::lexer::LexerFlagSet;
use crate::parser::nodes::{
    ArgumentList, ArgumentListNode, ArgumentsNode, ArrayNode, ArrayPatternNode, ArrowFuncExprNode,
    AssignResolveNode, AssignmentContext, AssignmentElementNode, AwaitExprNode, BigIntNode, BindingNode,
    BlockNode, BooleanNode, BracketAccessorNode, BreakNode, BytecodeIntrinsicNode, BytecodeIntrinsicNodeType,
    CaseBlockNode, CaseClauseNode, ChainNode, ClassDeclNode, ClassElementTag, ClassExprNode, ClauseList,
    ClauseListNode, ConditionalNode, ContinueNode, DebuggerStatementNode, DeclarationStatement,
    DefineFieldNode, DefineFieldType, DestructuringAssignmentNode, DestructuringPatternNode, DoWhileNode,
    DotAccessorNode, DotType, DoubleNode, ElementList, ElementNode, EmptyLetExpression, EmptyStatementNode,
    EmptyVarExpression, ExportSpecifierListNode, ExportSpecifierNode, Expression, ExprStatementNode,
    ForInNode, ForNode, ForOfNode, FuncDeclNode, FuncExprNode, FunctionMetadataNode, FunctionParameters,
    FunctionStack, IfElseNode, ImportAttributesListNode, ImportDeclarationNode, ImportMetaNode, ImportNode,
    ImportSpecifierListNode, ImportSpecifierNode, ImportType, IntegerNode, LabelNode, LogicalNotNode,
    MethodDefinitionNode, ModuleNameNode, NewExprNode, NewTargetNode, NodeList, NullNode, ObjectLiteralNode,
    ObjectPatternNode, ObjectSpreadExpressionNode, Operator, OptionalChainNode, PrivateIdentifierNode,
    PropertyList, PropertyListNode, PropertyNode, PropertyNodeType, RegExpNode, ResolveNode,
    RestParameterNode, ReturnNode, SourceElements, SpreadExpressionNode, Statement, StringNode, SuperNode,
    SwitchNode, TaggedTemplateNode, TemplateExpressionListNode, TemplateLiteralNode, TemplateStringListNode,
    TemplateStringNode, ThisNode, ThrowNode, ThrowableExpressionData, TryNode, UnaryPlusNode, VoidNode,
    WhileNode, WithNode, YieldExprNode,
};
use crate::parser::parser::InferName;
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_function_info::{ParserClassInfo, ParserFunctionInfo};
use crate::parser::parser_modes::{
    LexicallyScopedFeatures, SourceParseMode, SuperBinding, ARGUMENTS_FEATURE, ARROW_FUNCTION_FEATURE,
    AWAIT_FEATURE, EVAL_FEATURE, NEW_TARGET_FEATURE, SUPER_CALL_FEATURE, SUPER_PROPERTY_FEATURE,
    THIS_FEATURE, WITH_FEATURE,
};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::source_code::SourceCode;
use crate::parser::tree_builder::{TreeBuilder, TreeNodeHandle};
use crate::parser::variable_environment::VariableEnvironment;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;
use crate::wtf::math_extras::is_zero_or_unordered;
use crate::yarr::yarr_error_code::has_error;
use crate::yarr::yarr_syntax_checker::check_syntax;

/// A alça de um nó do construtor: ver a nota do módulo.
pub struct NodeRef<T>(Option<Rc<RefCell<Option<T>>>>);

impl<T> NodeRef<T> {
    pub fn new(value: T) -> NodeRef<T> {
        NodeRef(Some(Rc::new(RefCell::new(Some(value)))))
    }

    /// `!node` (o `nullptr` do C++).
    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }

    /// Move o nó para o pai. `RELEASE_ASSERT`: a alça não é nula nem foi movida antes.
    pub fn take(&self) -> T {
        match &self.0 {
            Some(cell) => cell.borrow_mut().take().expect("RELEASE_ASSERT: nó já movido para o pai"),
            None => panic!("RELEASE_ASSERT: alça nula"),
        }
    }

    /// `take()` que aceita a alça nula (o ponteiro nulo do C++ vira `None`).
    pub fn take_opt(&self) -> Option<T> {
        if self.is_null() {
            None
        } else {
            Some(self.take())
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        match &self.0 {
            Some(cell) => f(cell.borrow().as_ref().expect("RELEASE_ASSERT: nó já movido para o pai")),
            None => panic!("RELEASE_ASSERT: alça nula"),
        }
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        match &self.0 {
            Some(cell) => f(cell.borrow_mut().as_mut().expect("RELEASE_ASSERT: nó já movido para o pai")),
            None => panic!("RELEASE_ASSERT: alça nula"),
        }
    }
}

impl<T> Clone for NodeRef<T> {
    fn clone(&self) -> NodeRef<T> {
        NodeRef(self.0.clone())
    }
}

impl<T> Default for NodeRef<T> {
    fn default() -> NodeRef<T> {
        NodeRef(None)
    }
}

/// Igualdade de ponteiros do C++.
impl<T> PartialEq for NodeRef<T> {
    fn eq(&self, other: &NodeRef<T>) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

/// `FunctionMetadataNode*`: o `Rc` compartilhado com o `FuncDeclNode`, o `BaseFuncExprNode` e a pilha de
/// funções do escopo.
#[derive(Clone, Default)]
pub struct FunctionBodyRef(Option<Rc<FunctionMetadataNode>>);

impl FunctionBodyRef {
    pub fn new(metadata: FunctionMetadataNode) -> FunctionBodyRef {
        FunctionBodyRef(Some(Rc::new(metadata)))
    }

    /// O `Rc` do metadado. `RELEASE_ASSERT`: a alça não é nula.
    pub fn rc(&self) -> &Rc<FunctionMetadataNode> {
        self.0.as_ref().expect("RELEASE_ASSERT: FunctionMetadataNode nulo")
    }
}

impl PartialEq for FunctionBodyRef {
    fn eq(&self, other: &FunctionBodyRef) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

pub type ExprRef = NodeRef<Expression>;
pub type StmtRef = NodeRef<Statement>;
pub type PatternRef = NodeRef<DestructuringPatternNode>;

/// `Node*` do `ASTBuilder` sobre uma expressão: o `Expression` faz `Deref` até o `Node`.
impl TreeNodeHandle for NodeRef<Expression> {
    fn set_end_offset(&self, offset: i32) {
        self.with_mut(|node| node.set_end_offset(offset));
    }
    fn end_offset(&self) -> i32 {
        self.with(|node| node.end_offset())
    }
    fn set_start_offset(&self, offset: i32) {
        self.with_mut(|node| node.set_start_offset(offset));
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        self.with_mut(|node| {
            node.set_needs_debug_hook();
            *node.position()
        })
    }
}

/// `Node*` do `ASTBuilder` sobre um statement.
impl TreeNodeHandle for NodeRef<Statement> {
    fn set_end_offset(&self, offset: i32) {
        self.with_mut(|node| node.set_end_offset(offset));
    }
    fn end_offset(&self) -> i32 {
        self.with(|node| node.end_offset())
    }
    fn set_start_offset(&self, offset: i32) {
        self.with_mut(|node| node.set_start_offset(offset));
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        self.with_mut(|node| {
            node.set_needs_debug_hook();
            *node.position()
        })
    }
}

/// `setStartOffset(CaseClauseNode*, int)`: o `CaseClauseNode` não é um `Node` e o `ASTBuilder` só o usa
/// com esta sobrecarga; as outras três operações não existem para ele no C++.
impl TreeNodeHandle for NodeRef<CaseClauseNode> {
    fn set_end_offset(&self, _offset: i32) {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
    fn end_offset(&self) -> i32 {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
    fn set_start_offset(&self, offset: i32) {
        self.with_mut(|node| node.start_offset = offset);
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
}

/// `Node*` do `ASTBuilder` sobre um `FunctionMetadataNode` (que é um `Node`).
impl TreeNodeHandle for FunctionBodyRef {
    fn set_end_offset(&self, offset: i32) {
        self.rc().base.borrow_mut().set_end_offset(offset);
    }
    fn end_offset(&self) -> i32 {
        self.rc().base.borrow().end_offset()
    }
    fn set_start_offset(&self, offset: i32) {
        self.rc().base.borrow_mut().set_start_offset(offset);
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        let mut node = self.rc().base.borrow_mut();
        node.set_needs_debug_hook();
        *node.position()
    }
}

/// As listas de template encadeiam por `next` como as de elementos, de propriedades e de argumentos.
impl ChainNode for TemplateStringListNode {
    fn next_mut(&mut self) -> &mut Option<Box<TemplateStringListNode>> {
        &mut self.next
    }
}

impl ChainNode for TemplateExpressionListNode {
    fn next_mut(&mut self) -> &mut Option<Box<TemplateExpressionListNode>> {
        &mut self.next
    }
}

/// `ASTBuilder::BinaryOpInfo`.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct BinaryOpInfo {
    pub start: JSTextPosition,
    pub divot: JSTextPosition,
    pub end: JSTextPosition,
    pub has_assignment: bool,
}

impl BinaryOpInfo {
    /// `BinaryOpInfo(otherStart, otherDivot, otherEnd, rhsHasAssignment)`.
    pub fn new(start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition, has_assignment: bool) -> BinaryOpInfo {
        BinaryOpInfo { start, divot, end, has_assignment }
    }

    /// `BinaryOpInfo(lhs, rhs)`.
    pub fn from_operands(lhs: &BinaryOpInfo, rhs: &BinaryOpInfo) -> BinaryOpInfo {
        BinaryOpInfo { start: lhs.start, divot: rhs.start, end: rhs.end, has_assignment: lhs.has_assignment || rhs.has_assignment }
    }
}

/// `ASTBuilder::AssignmentInfo`.
#[derive(Clone)]
pub struct AssignmentInfo {
    pub node: ExprRef,
    pub start: JSTextPosition,
    pub divot: JSTextPosition,
    pub init_assignments: i32,
    pub op: Operator,
}

impl AssignmentInfo {
    pub fn new(node: ExprRef, start: JSTextPosition, divot: JSTextPosition, init_assignments: i32, op: Operator) -> AssignmentInfo {
        AssignmentInfo { node, start, divot, init_assignments, op }
    }
}

/// `ASTBuilder::BinaryExprContext`: o construtor do C++ não faz nada.
pub struct BinaryExprContext;

/// `ASTBuilder::UnaryExprContext`: idem.
pub struct UnaryExprContext;

/// `ASTBuilder::Scope`.
#[derive(Default)]
struct Scope {
    features: i32,
    num_constants: i32,
}

pub struct ASTBuilder {
    vm: Rc<VM>,
    source_code: SourceCode,
    scope: Scope,
    binary_operand_stack: Vec<(ExprRef, BinaryOpInfo)>,
    assignment_info_stack: Vec<AssignmentInfo>,
    binary_operator_stack: Vec<(i32, i32)>,
    unary_token_stack: Vec<(i32, JSTextPosition)>,
    eval_count: i32,
}

/// Membros que não são do `TreeBuilder`: construtor, acessores do escopo e os auxiliares privados do
/// `ASTBuilder.h` (linhas 1125 a 1222) que as duas fatias usam.
impl ASTBuilder {
    pub fn new(vm: Rc<VM>, _parser_arena: &mut ParserArena, source_code: &SourceCode) -> ASTBuilder {
        ASTBuilder {
            vm,
            source_code: source_code.clone(),
            scope: Scope::default(),
            binary_operand_stack: Vec::new(),
            assignment_info_stack: Vec::new(),
            binary_operator_stack: Vec::new(),
            unary_token_stack: Vec::new(),
            eval_count: 0,
        }
    }

    pub fn features(&self) -> i32 {
        self.scope.features
    }

    pub fn num_constants(&self) -> i32 {
        self.scope.num_constants
    }

    /// `classElements->setHasPrivateAccessors(...)` (`Parser.cpp`): o `classElements` do C++ é a cabeça.
    pub fn set_has_private_accessors(&self, class_elements: &NodeRef<PropertyList>, has_private_accessors: bool) {
        class_elements.with_mut(|list| {
            if let Some(head) = list.nodes.first_mut() {
                head.has_private_accessors = has_private_accessors;
            }
        });
    }

    /// `ASTBuilder::checkArgumentsLengthModification`.
    fn check_arguments_length_modification(&mut self, node: &ExprRef) {
        // Since we exclude pattern `arguments.length` to enable ArgumentsFeature,
        // we need re-enable ArgumentsFeature for `arguments.length` modification.
        if !node.is_null() && node.with(|expression| expression.is_arguments_length_access(&self.vm)) {
            self.uses_arguments();
        }
    }

    /// `ASTBuilder::setExceptionLocation(ThrowableExpressionData*, divotStart, divot, divotEnd)`.
    fn set_exception_location(node: &mut ThrowableExpressionData, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition) {
        node.set_exception_source_code(divot, divot_start, divot_end);
    }

    fn inc_constants(&mut self) {
        self.scope.num_constants += 1;
    }

    fn uses_this(&mut self) {
        self.scope.features |= THIS_FEATURE as i32;
    }

    fn uses_arrow_function(&mut self) {
        self.scope.features |= ARROW_FUNCTION_FEATURE as i32;
    }

    fn uses_arguments(&mut self) {
        self.scope.features |= ARGUMENTS_FEATURE as i32;
    }

    fn uses_with(&mut self) {
        self.scope.features |= WITH_FEATURE as i32;
    }

    fn uses_super_call(&mut self) {
        self.scope.features |= SUPER_CALL_FEATURE as i32;
    }

    fn uses_super_property(&mut self) {
        self.scope.features |= SUPER_PROPERTY_FEATURE as i32;
    }

    fn uses_eval(&mut self) {
        self.eval_count += 1;
        self.scope.features |= EVAL_FEATURE as i32;
    }

    fn uses_new_target(&mut self) {
        self.scope.features |= NEW_TARGET_FEATURE as i32;
    }

    fn uses_await(&mut self) {
        self.scope.features |= AWAIT_FEATURE as i32;
    }

    /// `static_cast<BaseFuncExprNode*>(node)->metadata()->setEcmaName(ident)`.
    fn set_metadata_ecma_name(metadata: &FunctionMetadataNode, ident: &Identifier) {
        *metadata.ecma_name.borrow_mut() = ident.clone();
    }

    /// O par de ramos que o C++ repete em `createAssignResolve`, `createDefineField`, `createProperty`,
    /// `makeAssignNode` e `tryInferNameInPatternWithIdentifier`:
    /// `if (node->isBaseFuncExprNode()) metadata->setEcmaName(ident); else if (node->isClassExprNode()) classExpr->setEcmaName(ident);`.
    fn set_ecma_name_of_function_or_class(node: &ExprRef, ident: &Identifier) {
        node.with_mut(|expression| match expression {
            Expression::FuncExpr(n) => Self::set_metadata_ecma_name(&n.metadata, ident),
            Expression::ArrowFuncExpr(n) => Self::set_metadata_ecma_name(&n.metadata, ident),
            Expression::MethodDefinition(n) => Self::set_metadata_ecma_name(&n.metadata, ident),
            Expression::ClassExpr(n) => n.set_ecma_name(ident),
            _ => {}
        });
    }

    /// `ASTBuilder::tryInferNameInPattern` (com `tryInferNameInPatternWithIdentifier` no corpo).
    fn try_infer_name_in_pattern(pattern: &PatternRef, default_value: &ExprRef) {
        if default_value.is_null() {
            return;
        }

        let ident = pattern.with(|pattern| match pattern {
            DestructuringPatternNode::Binding(binding) => Some(binding.bound_property.clone()),
            DestructuringPatternNode::AssignmentElement(element) => match &element.assignment_target {
                Expression::Resolve(resolve) => Some(resolve.ident.clone()),
                _ => None,
            },
            _ => None,
        });
        if let Some(ident) = ident {
            Self::set_ecma_name_of_function_or_class(default_value, &ident);
        }
    }

    /// `*functionInfo.name` e `*classInfo.className`: o C++ desreferencia sem teste.
    fn non_null_name(name: &Option<Identifier>) -> &Identifier {
        name.as_ref().expect("RELEASE_ASSERT: nome nulo")
    }

    /// `m_sourceCode->subExpression(functionInfo.startOffset, endOffset, functionInfo.startLine, functionInfo.parametersStartColumn)`.
    fn function_source(&self, function_info: &ParserFunctionInfo<ASTBuilder>, end_offset: u32) -> SourceCode {
        self.source_code.sub_expression(function_info.start_offset, end_offset, function_info.start_line, function_info.parameters_start_column as i32)
    }

    /// O `endOffset` das funções seta: `isArrowFunctionBodyExpression() ? endOffset - 1 : endOffset`.
    fn arrow_function_end_offset(function_info: &ParserFunctionInfo<ASTBuilder>) -> u32 {
        if function_info.body.rc().is_arrow_function_body_expression {
            function_info.end_offset - 1
        } else {
            function_info.end_offset
        }
    }

    /// `functionInfo.body->setLoc(functionInfo.startLine, functionInfo.endLine, location.startOffset, location.lineStartOffset)`.
    fn set_function_body_loc(function_info: &ParserFunctionInfo<ASTBuilder>, location: &JSTokenLocation) {
        function_info.body.rc().set_loc(function_info.start_line as u32, function_info.end_line as u32, location.start_offset as i32, location.line_start_offset as i32);
    }

    /// `result->setLoc(first, last, location.startOffset, location.lineStartOffset)` dos statements.
    fn set_statement_loc(statement: &mut Statement, first_line: u32, last_line: u32, location: &JSTokenLocation) {
        statement.set_loc(first_line, last_line, location.start_offset as i32, location.line_start_offset as i32);
    }

    /// O corpo comum de `createFunctionExpr` e das duas rotas de `createGeneratorFunctionBody` e
    /// `createAsyncFunctionBody` que o chamam: o `FuncExprNode` e o `setLoc` do metadado.
    fn build_func_expr(&self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Expression {
        let source = self.function_source(function_info, function_info.end_offset);
        let result = FuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(function_info.body.rc()), &source);
        Self::set_function_body_loc(function_info, location);
        Expression::FuncExpr(Box::new(result))
    }

    /// `getter` e `setter` com nome fixo ou numérico: o `MethodDefinitionNode` com o corpo.
    fn build_accessor_method_definition(&self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, null_identifier: &Identifier) -> Expression {
        let source = self.function_source(function_info, function_info.end_offset);
        Expression::MethodDefinition(Box::new(MethodDefinitionNode::new(location, null_identifier, Rc::clone(function_info.body.rc()), &source)))
    }

    /// `comma_as_expression` (o `CommaNode*` é um `ExpressionNode*`): são a mesma alça.
    pub fn comma_as_expression(&self, comma: &ExprRef) -> ExprRef {
        comma.clone()
    }

    /// `ObjectPatternNode*` visto como `DestructuringPatternNode*`: a mesma alça.
    pub fn object_pattern_as_destructuring_pattern(&self, object_pattern: &PatternRef) -> PatternRef {
        object_pattern.clone()
    }
}

impl TreeBuilder for ASTBuilder {
    type Expression = ExprRef;
    type SourceElements = NodeRef<SourceElements>;
    type Arguments = NodeRef<ArgumentsNode>;
    type Comma = ExprRef;
    type Property = NodeRef<PropertyNode>;
    type PropertyList = NodeRef<PropertyList>;
    type ElementList = NodeRef<ElementList>;
    type ArgumentsList = NodeRef<ArgumentList>;
    type TemplateExpressionList = NodeRef<NodeList<TemplateExpressionListNode>>;
    type TemplateString = NodeRef<TemplateStringNode>;
    type TemplateStringList = NodeRef<NodeList<TemplateStringListNode>>;
    type TemplateLiteral = NodeRef<TemplateLiteralNode>;
    type FormalParameterList = NodeRef<FunctionParameters>;
    type FunctionBody = FunctionBodyRef;
    type ClassExpression = ExprRef;
    type ModuleName = NodeRef<ModuleNameNode>;
    type ImportSpecifier = NodeRef<ImportSpecifierNode>;
    type ImportSpecifierList = NodeRef<ImportSpecifierListNode>;
    type ImportAttributesList = NodeRef<ImportAttributesListNode>;
    type ExportSpecifier = NodeRef<ExportSpecifierNode>;
    type ExportSpecifierList = NodeRef<ExportSpecifierListNode>;
    type Statement = StmtRef;
    type ClauseList = NodeRef<ClauseList>;
    type Clause = NodeRef<CaseClauseNode>;
    type BinaryOperand = (ExprRef, BinaryOpInfo);
    type DestructuringPattern = PatternRef;
    type ArrayPattern = PatternRef;
    type ObjectPattern = PatternRef;
    type RestPattern = PatternRef;
    type DefineField = StmtRef;
    type BinaryExprContext = BinaryExprContext;
    type UnaryExprContext = UnaryExprContext;

    const CREATES_AST: bool = true;
    const NEEDS_FREE_VARIABLE_INFO: bool = true;
    const CAN_USE_FUNCTION_CACHE: bool = true;
    const DONT_BUILD_KEYWORDS: LexerFlagSet = LexerFlagSet::empty();
    const DONT_BUILD_STRINGS: LexerFlagSet = LexerFlagSet::empty();

    fn begin_binary_expr_context(&mut self) -> BinaryExprContext {
        BinaryExprContext
    }
    fn end_binary_expr_context(&mut self, _saved: BinaryExprContext) {}
    fn begin_unary_expr_context(&mut self) -> UnaryExprContext {
        UnaryExprContext
    }
    fn end_unary_expr_context(&mut self, _saved: UnaryExprContext) {}

    fn create_source_elements(&mut self) -> NodeRef<SourceElements> {
        NodeRef::new(SourceElements::new())
    }

    fn create_logical_not(&mut self, location: &JSTokenLocation, expr: ExprRef) -> ExprRef {
        let number = expr.with(|expression| match expression {
            Expression::Double(node) => Some(node.value),
            Expression::Integer(node) => Some(node.value),
            _ => None,
        });
        if let Some(value) = number {
            return self.create_boolean(location, is_zero_or_unordered(value));
        }

        NodeRef::new(Expression::LogicalNot(Box::new(LogicalNotNode::new(location, expr.take()))))
    }

    fn create_unary_plus(&mut self, location: &JSTokenLocation, expr: ExprRef) -> ExprRef {
        NodeRef::new(Expression::UnaryPlus(Box::new(UnaryPlusNode::new(location, expr.take()))))
    }

    fn create_void(&mut self, location: &JSTokenLocation, expr: ExprRef) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::Void(Box::new(VoidNode::new(location, expr.take()))))
    }

    fn create_this_expr(&mut self, location: &JSTokenLocation) -> ExprRef {
        self.uses_this();
        NodeRef::new(Expression::This(Box::new(ThisNode::new(location))))
    }

    fn create_super_expr(&mut self, location: &JSTokenLocation) -> ExprRef {
        NodeRef::new(Expression::Super(Box::new(SuperNode::new(location))))
    }

    fn create_import_expr(&mut self, location: &JSTokenLocation, expr: ExprRef, option: ExprRef, deferred: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = ImportNode::new(location, expr.take(), option.take_opt(), deferred);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::Import(Box::new(node)))
    }

    fn create_new_target_expr(&mut self, location: &JSTokenLocation) -> ExprRef {
        self.uses_new_target();
        NodeRef::new(Expression::NewTarget(Box::new(NewTargetNode::new(location))))
    }

    fn create_import_meta_expr(&mut self, location: &JSTokenLocation, expr: ExprRef) -> ExprRef {
        NodeRef::new(Expression::ImportMeta(Box::new(ImportMetaNode::new(location, expr.take()))))
    }

    fn is_meta_property(&mut self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_meta_property())
    }

    fn is_new_target(&mut self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_new_target())
    }

    fn is_import_meta(&mut self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_import_meta())
    }

    fn create_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition, need_to_check_uses_arguments: bool) -> ExprRef {
        if need_to_check_uses_arguments && self.vm.property_names.arguments == *ident {
            self.uses_arguments();
        }

        if ident.is_symbol() {
            if let Some(entry) = self.vm.bytecode_intrinsic_registry().lookup(ident) {
                return NodeRef::new(Expression::BytecodeIntrinsic(Box::new(BytecodeIntrinsicNode::new(BytecodeIntrinsicNodeType::Constant, location, entry, ident.clone(), None, start, start, end))));
            }
        }

        NodeRef::new(Expression::Resolve(Box::new(ResolveNode::new(location, ident.clone(), start))))
    }

    fn create_private_identifier_node(&mut self, location: &JSTokenLocation, ident: &Identifier) -> ExprRef {
        NodeRef::new(Expression::PrivateIdentifier(Box::new(PrivateIdentifierNode::new(location, ident.clone()))))
    }

    fn create_object_literal(&mut self, location: &JSTokenLocation) -> ExprRef {
        NodeRef::new(Expression::ObjectLiteral(Box::new(ObjectLiteralNode::new(location))))
    }

    fn create_object_literal_with_properties(&mut self, location: &JSTokenLocation, properties: NodeRef<PropertyList>) -> ExprRef {
        let list = properties.take_opt().and_then(NodeList::into_head);
        NodeRef::new(Expression::ObjectLiteral(Box::new(ObjectLiteralNode::with_list(location, list))))
    }

    fn create_array_elisions(&mut self, location: &JSTokenLocation, elisions: i32) -> ExprRef {
        if elisions != 0 {
            self.inc_constants();
        }
        NodeRef::new(Expression::Array(Box::new(ArrayNode::new(location, elisions))))
    }

    fn create_array_elements(&mut self, location: &JSTokenLocation, elems: NodeRef<ElementList>) -> ExprRef {
        let element = elems.take_opt().and_then(NodeList::into_head);
        NodeRef::new(Expression::Array(Box::new(ArrayNode::from_elements(location, element))))
    }

    fn create_array_elisions_elements(&mut self, location: &JSTokenLocation, elisions: i32, elems: NodeRef<ElementList>) -> ExprRef {
        if elisions != 0 {
            self.inc_constants();
        }
        let element = elems.take_opt().and_then(NodeList::into_head);
        NodeRef::new(Expression::Array(Box::new(ArrayNode::with_elision(location, elisions, element))))
    }

    fn create_double_expr(&mut self, location: &JSTokenLocation, d: f64) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::Double(Box::new(DoubleNode::new(location, d))))
    }

    fn create_integer_expr(&mut self, location: &JSTokenLocation, d: f64) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::Integer(Box::new(IntegerNode::new(location, d))))
    }

    fn create_big_int(&mut self, location: &JSTokenLocation, big_int: &Identifier, radix: u8) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::BigInt(Box::new(BigIntNode::new(location, big_int.clone(), radix))))
    }

    fn create_string(&mut self, location: &JSTokenLocation, string: &Identifier) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::String(Box::new(StringNode::new(location, string.clone()))))
    }

    fn create_boolean(&mut self, location: &JSTokenLocation, b: bool) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::Boolean(Box::new(BooleanNode::new(location, b))))
    }

    fn create_null(&mut self, location: &JSTokenLocation) -> ExprRef {
        self.inc_constants();
        NodeRef::new(Expression::Null(Box::new(NullNode::new(location))))
    }

    fn create_bracket_access(&mut self, location: &JSTokenLocation, base: ExprRef, property: ExprRef, property_has_assignments: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        if base.with(|node| node.is_super_node()) {
            self.uses_super_property();
        }

        let mut node = BracketAccessorNode::new(location, base.take(), property.take(), property_has_assignments);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::BracketAccessor(Box::new(node)))
    }

    fn create_dot_access(&mut self, location: &JSTokenLocation, base: ExprRef, property: &Identifier, type_: DotType, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        if base.with(|node| node.is_super_node()) {
            self.uses_super_property();
        }

        let mut node = DotAccessorNode::new(location, base.take(), property.clone(), type_);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::DotAccessor(Box::new(node)))
    }

    fn create_spread_expression(&mut self, location: &JSTokenLocation, expression: ExprRef, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = SpreadExpressionNode::new(location, expression.take());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::SpreadExpression(Box::new(node)))
    }

    fn create_object_spread_expression(&mut self, location: &JSTokenLocation, expression: ExprRef, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = ObjectSpreadExpressionNode::new(location, expression.take());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::ObjectSpreadExpression(Box::new(node)))
    }

    fn create_template_string(&mut self, location: &JSTokenLocation, cooked: Option<&Identifier>, raw: Option<&Identifier>) -> NodeRef<TemplateStringNode> {
        NodeRef::new(TemplateStringNode::new(location, cooked.cloned(), raw.cloned()))
    }

    fn create_template_string_list(&mut self, template_string: NodeRef<TemplateStringNode>) -> NodeRef<NodeList<TemplateStringListNode>> {
        let mut list = NodeList::new();
        list.push(Box::new(TemplateStringListNode::new(Box::new(template_string.take()))));
        NodeRef::new(list)
    }

    fn create_template_string_list_append(&mut self, template_string_list: NodeRef<NodeList<TemplateStringListNode>>, template_string: NodeRef<TemplateStringNode>) -> NodeRef<NodeList<TemplateStringListNode>> {
        template_string_list.with_mut(|list| list.push(Box::new(TemplateStringListNode::new(Box::new(template_string.take())))));
        template_string_list
    }

    fn create_template_expression_list(&mut self, expression: ExprRef) -> NodeRef<NodeList<TemplateExpressionListNode>> {
        let mut list = NodeList::new();
        list.push(Box::new(TemplateExpressionListNode::new(expression.take())));
        NodeRef::new(list)
    }

    fn create_template_expression_list_append(&mut self, template_expression_list: NodeRef<NodeList<TemplateExpressionListNode>>, expression: ExprRef) -> NodeRef<NodeList<TemplateExpressionListNode>> {
        template_expression_list.with_mut(|list| list.push(Box::new(TemplateExpressionListNode::new(expression.take()))));
        template_expression_list
    }

    fn create_template_literal(&mut self, location: &JSTokenLocation, template_string_list: NodeRef<NodeList<TemplateStringListNode>>) -> NodeRef<TemplateLiteralNode> {
        let strings = template_string_list.take_opt().and_then(NodeList::into_head);
        NodeRef::new(TemplateLiteralNode::new(location, strings))
    }

    fn create_template_literal_with_expressions(&mut self, location: &JSTokenLocation, template_string_list: NodeRef<NodeList<TemplateStringListNode>>, template_expression_list: NodeRef<NodeList<TemplateExpressionListNode>>) -> NodeRef<TemplateLiteralNode> {
        let strings = template_string_list.take_opt().and_then(NodeList::into_head);
        let expressions = template_expression_list.take_opt().and_then(NodeList::into_head);
        NodeRef::new(TemplateLiteralNode::with_expressions(location, strings, expressions))
    }

    fn create_tagged_template(&mut self, location: &JSTokenLocation, base: ExprRef, template_literal: NodeRef<TemplateLiteralNode>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = TaggedTemplateNode::new(location, base.take(), Box::new(template_literal.take()));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        node.set_end_offset(end.offset);
        NodeRef::new(Expression::TaggedTemplate(Box::new(node)))
    }

    fn create_reg_exp(&mut self, location: &JSTokenLocation, pattern: &Identifier, flags: &Identifier, start: JSTextPosition, skip_syntax_check: bool) -> ExprRef {
        if !skip_syntax_check && has_error(check_syntax(pattern.string(), flags.string())) {
            return NodeRef::default();
        }
        let mut node = RegExpNode::new(location, pattern.clone(), flags.clone());
        let size = pattern.length() as i32 + 2; // + 2 for the two /'s
        let end = start + size;
        Self::set_exception_location(&mut node.throwable, start, end, end);
        NodeRef::new(Expression::RegExp(Box::new(node)))
    }

    fn create_new_expr(&mut self, location: &JSTokenLocation, expr: ExprRef, arguments: NodeRef<ArgumentsNode>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = NewExprNode::with_args(location, expr.take(), arguments.take_opt().map(Box::new));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::NewExpr(Box::new(node)))
    }

    fn create_new_expr_no_arguments(&mut self, location: &JSTokenLocation, expr: ExprRef, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = NewExprNode::new(location, expr.take());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::NewExpr(Box::new(node)))
    }

    fn create_optional_chain(&mut self, location: &JSTokenLocation, base: ExprRef, expr: ExprRef, is_outermost: bool) -> ExprRef {
        if !base.is_null() {
            base.with_mut(|node| node.set_is_optional_chain_base());
        }
        NodeRef::new(Expression::OptionalChain(Box::new(OptionalChainNode::new(location, expr.take(), is_outermost))))
    }

    fn create_conditional_expr(&mut self, location: &JSTokenLocation, condition: ExprRef, lhs: ExprRef, rhs: ExprRef) -> ExprRef {
        NodeRef::new(Expression::Conditional(Box::new(ConditionalNode::new(location, condition.take(), lhs.take(), rhs.take()))))
    }

    fn create_assign_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, rhs: ExprRef, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition, assignment_context: AssignmentContext) -> ExprRef {
        Self::set_ecma_name_of_function_or_class(&rhs, ident);
        if assignment_context == AssignmentContext::AwaitUsingDeclarationStatement {
            self.uses_await();
        }
        let mut node = AssignResolveNode::new(location, ident.clone(), rhs.take(), assignment_context);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::AssignResolve(Box::new(node)))
    }

    fn create_empty_var_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> ExprRef {
        NodeRef::new(Expression::EmptyVarExpression(Box::new(EmptyVarExpression::new(location, ident.clone()))))
    }

    fn create_empty_let_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> ExprRef {
        NodeRef::new(Expression::EmptyLetExpression(Box::new(EmptyLetExpression::new(location, ident.clone()))))
    }

    fn create_yield(&mut self, location: &JSTokenLocation) -> ExprRef {
        NodeRef::new(Expression::YieldExpr(Box::new(YieldExprNode::new(location, None, false))))
    }

    fn create_yield_argument(&mut self, location: &JSTokenLocation, argument: ExprRef, delegate: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let mut node = YieldExprNode::new(location, argument.take_opt(), delegate);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::YieldExpr(Box::new(node)))
    }

    fn create_await(&mut self, location: &JSTokenLocation, argument: ExprRef, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        self.uses_await();
        let mut node = AwaitExprNode::new(location, argument.take());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::AwaitExpr(Box::new(node)))
    }

    fn create_define_field(&mut self, location: &JSTokenLocation, ident: &Identifier, initializer: ExprRef, type_: DefineFieldType) -> StmtRef {
        if !initializer.is_null() && type_ != DefineFieldType::ComputedName {
            Self::set_ecma_name_of_function_or_class(&initializer, ident);
        }
        NodeRef::new(Statement::DefineField(Box::new(DefineFieldNode::new(location, ident.clone(), initializer.take_opt(), type_))))
    }

    fn create_class_expr(&mut self, location: &JSTokenLocation, class_info: &ParserClassInfo<ASTBuilder>, class_head_environment: VariableEnvironment, class_environment: VariableEnvironment, constructor: ExprRef, parent_class: ExprRef, class_elements: NodeRef<PropertyList>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> ExprRef {
        let source = self.source_code.sub_expression(class_info.start_offset, class_info.end_offset, class_info.start_line, class_info.start_column as i32);
        let elements = class_elements.take_opt().and_then(NodeList::into_head);
        let mut node = ClassExprNode::new(location, Self::non_null_name(&class_info.class_name).clone(), source, class_head_environment, class_environment, constructor.take_opt(), parent_class.take_opt(), elements);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        NodeRef::new(Expression::ClassExpr(Box::new(node)))
    }

    fn create_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> ExprRef {
        NodeRef::new(self.build_func_expr(location, function_info))
    }

    fn create_generator_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, name: &Identifier) -> ExprRef {
        let result = self.build_func_expr(location, function_info);
        if !name.is_null() {
            Self::set_metadata_ecma_name(function_info.body.rc(), name);
        }
        NodeRef::new(result)
    }

    fn create_async_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, parse_mode: SourceParseMode, name: &Identifier) -> ExprRef {
        if parse_mode == SourceParseMode::AsyncArrowFunctionBodyMode {
            let source = self.function_source(function_info, Self::arrow_function_end_offset(function_info));
            let result = FuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(function_info.body.rc()), &source);
            if !name.is_null() {
                Self::set_metadata_ecma_name(function_info.body.rc(), name);
            }
            Self::set_function_body_loc(function_info, location);
            return NodeRef::new(Expression::FuncExpr(Box::new(result)));
        }
        let result = self.build_func_expr(location, function_info);
        if !name.is_null() {
            Self::set_metadata_ecma_name(function_info.body.rc(), name);
        }
        NodeRef::new(result)
    }

    fn create_method_definition(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> ExprRef {
        let source = self.function_source(function_info, function_info.end_offset);
        let result = MethodDefinitionNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(function_info.body.rc()), &source);
        Self::set_function_body_loc(function_info, location);
        NodeRef::new(Expression::MethodDefinition(Box::new(result)))
    }

    fn create_function_metadata(&mut self, start_location: &JSTokenLocation, end_location: &JSTokenLocation, start_column: u32, end_column: u32, function_start: u32, function_name_start: i32, parameters_start: i32, implementation_visibility: ImplementationVisibility, lexically_scoped_features: LexicallyScopedFeatures, constructor_kind: ConstructorKind, super_binding: SuperBinding, parameter_count: u32, mode: SourceParseMode, is_arrow_function_body_expression: bool) -> FunctionBodyRef {
        FunctionBodyRef::new(FunctionMetadataNode::new(start_location, end_location, start_column, end_column, function_start, function_name_start, parameters_start, implementation_visibility, lexically_scoped_features, constructor_kind, super_binding, parameter_count, mode, is_arrow_function_body_expression))
    }

    fn create_arrow_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> ExprRef {
        self.uses_arrow_function();
        let source = self.function_source(function_info, Self::arrow_function_end_offset(function_info));
        let result = ArrowFuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(function_info.body.rc()), &source);
        Self::set_function_body_loc(function_info, location);
        NodeRef::new(Expression::ArrowFuncExpr(Box::new(result)))
    }

    fn set_function_name_start(&mut self, _body: &FunctionBodyRef, _function_name_start: i32) {
        // O `ASTBuilder.h` não declara `setFunctionNameStart`: só o `SyntaxChecker.h` o traz (`{ }`) e o
        // `Parser.cpp` não o chama. O trait o herda do `SyntaxChecker`, então aqui também não faz nada.
    }

    fn create_arguments(&mut self) -> NodeRef<ArgumentsNode> {
        NodeRef::new(ArgumentsNode::new())
    }

    fn create_arguments_with_list(&mut self, args: NodeRef<ArgumentList>, has_assignments: bool) -> NodeRef<ArgumentsNode> {
        let list = args.take_opt().and_then(NodeList::into_head);
        NodeRef::new(ArgumentsNode::with_list(list, has_assignments))
    }

    fn create_arguments_list(&mut self, location: &JSTokenLocation, arg: ExprRef) -> NodeRef<ArgumentList> {
        let mut list = NodeList::new();
        list.push(Box::new(ArgumentListNode::new(location, arg.take())));
        NodeRef::new(list)
    }

    fn create_arguments_list_append(&mut self, location: &JSTokenLocation, args: NodeRef<ArgumentList>, arg: ExprRef) -> NodeRef<ArgumentList> {
        args.with_mut(|list| list.push(Box::new(ArgumentListNode::new(location, arg.take()))));
        args
    }

    fn create_getter_or_setter_property(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: &Identifier, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        Self::set_metadata_ecma_name(function_info.body.rc(), name);
        let null_identifier = self.vm.property_names.null_identifier.clone();
        let method_def = self.build_accessor_method_definition(location, function_info, &null_identifier);
        NodeRef::new(PropertyNode::from_name_and_assign(name.clone(), method_def, type_, SuperBinding::Needed, tag))
    }

    fn create_getter_or_setter_property_computed(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: ExprRef, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        let null_identifier = self.vm.property_names.null_identifier.clone();
        let method_def = self.build_accessor_method_definition(location, function_info, &null_identifier);
        NodeRef::new(PropertyNode::from_expression_and_assign(name.take(), method_def, type_, SuperBinding::Needed, tag))
    }

    fn create_getter_or_setter_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, location: &JSTokenLocation, type_: PropertyNodeType, name: f64, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        let ident = parser_arena.identifier_arena().borrow_mut().make_numeric_identifier(vm, name);
        Self::set_metadata_ecma_name(function_info.body.rc(), &ident);
        let method_def = self.build_accessor_method_definition(location, function_info, &vm.property_names.null_identifier);
        NodeRef::new(PropertyNode::from_name_and_assign(ident, method_def, type_, SuperBinding::Needed, tag))
    }

    fn create_property_identifier(&mut self, property_name: &Identifier, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        NodeRef::new(PropertyNode::from_name(property_name.clone(), type_, super_binding, tag))
    }

    fn create_property_named(&mut self, name: Option<&Identifier>, node: ExprRef, type_: PropertyNodeType, super_binding: SuperBinding, infer_name: InferName, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        let property_name = name.expect("RELEASE_ASSERT: nome de propriedade nulo");
        if infer_name == InferName::Allowed {
            Self::set_ecma_name_of_function_or_class(&node, property_name);
        }
        NodeRef::new(PropertyNode::from_name_and_assign(property_name.clone(), node.take(), type_, super_binding, tag))
    }

    fn create_property_expression(&mut self, node: ExprRef, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        NodeRef::new(PropertyNode::from_assign(node.take(), type_, super_binding, tag))
    }

    fn create_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, property_name: f64, node: ExprRef, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        let ident = parser_arena.identifier_arena().borrow_mut().make_numeric_identifier(vm, property_name);
        NodeRef::new(PropertyNode::from_name_and_assign(ident, node.take(), type_, super_binding, tag))
    }

    fn create_property_computed(&mut self, property_name: ExprRef, node: ExprRef, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        NodeRef::new(PropertyNode::from_expression_and_assign(property_name.take(), node.take(), type_, super_binding, tag))
    }

    fn create_property_identifier_computed(&mut self, identifier: &Identifier, property_name: ExprRef, node: ExprRef, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> NodeRef<PropertyNode> {
        NodeRef::new(PropertyNode::from_name_expression_and_assign(identifier.clone(), property_name.take(), node.take(), type_, super_binding, tag))
    }

    fn create_property_list(&mut self, location: &JSTokenLocation, property: NodeRef<PropertyNode>) -> NodeRef<PropertyList> {
        let mut list = NodeList::new();
        list.push(Box::new(PropertyListNode::new(location, Box::new(property.take()))));
        NodeRef::new(list)
    }

    fn create_property_list_append(&mut self, location: &JSTokenLocation, property: NodeRef<PropertyNode>, tail: NodeRef<PropertyList>) -> NodeRef<PropertyList> {
        tail.with_mut(|list| list.push(Box::new(PropertyListNode::new(location, Box::new(property.take())))));
        tail
    }

    fn create_element_list(&mut self, elisions: i32, expr: ExprRef) -> NodeRef<ElementList> {
        let mut list = NodeList::new();
        list.push(Box::new(ElementNode::new(elisions, expr.take())));
        NodeRef::new(list)
    }

    fn create_element_list_append(&mut self, elems: NodeRef<ElementList>, elisions: i32, expr: ExprRef) -> NodeRef<ElementList> {
        elems.with_mut(|list| list.push(Box::new(ElementNode::new(elisions, expr.take()))));
        elems
    }

    fn create_element_list_from_arguments(&mut self, elems: NodeRef<ArgumentList>) -> NodeRef<ElementList> {
        let mut list = NodeList::new();
        for node in elems.take().nodes {
            list.push(Box::new(ElementNode::new(0, node.expr)));
        }
        NodeRef::new(list)
    }

    fn create_formal_parameter_list(&mut self) -> NodeRef<FunctionParameters> {
        NodeRef::new(FunctionParameters::new())
    }

    fn append_parameter(&mut self, list: &NodeRef<FunctionParameters>, pattern: PatternRef, default_value: ExprRef) {
        Self::try_infer_name_in_pattern(&pattern, &default_value);
        list.with_mut(|parameters| parameters.append(Box::new(pattern.take()), default_value.take_opt()));
    }

    fn create_clause(&mut self, expr: ExprRef, statements: NodeRef<SourceElements>) -> NodeRef<CaseClauseNode> {
        NodeRef::new(CaseClauseNode::new(expr.take_opt(), statements.take_opt().map(Box::new)))
    }

    fn create_clause_list(&mut self, clause: NodeRef<CaseClauseNode>) -> NodeRef<ClauseList> {
        let mut list = NodeList::new();
        list.push(Box::new(ClauseListNode::new(Box::new(clause.take()))));
        NodeRef::new(list)
    }

    fn create_clause_list_append(&mut self, tail: NodeRef<ClauseList>, clause: NodeRef<CaseClauseNode>) -> NodeRef<ClauseList> {
        tail.with_mut(|list| list.push(Box::new(ClauseListNode::new(Box::new(clause.take())))));
        tail
    }

    fn create_func_decl_statement(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> StmtRef {
        let source = self.function_source(function_info, function_info.end_offset);
        let decl = FuncDeclNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(function_info.body.rc()), &source);
        if *Self::non_null_name(&function_info.name) == self.vm.property_names.arguments {
            self.uses_arguments();
        }
        Self::set_function_body_loc(function_info, location);
        NodeRef::new(Statement::FuncDecl(Box::new(decl)))
    }

    fn create_class_decl_statement(&mut self, location: &JSTokenLocation, class_expression: ExprRef, class_start: JSTextPosition, class_end: JSTextPosition, start_line: u32, end_line: u32) -> StmtRef {
        let name = class_expression.with(|expression| match expression {
            Expression::ClassExpr(class) => class.name.clone(),
            _ => panic!("RELEASE_ASSERT_NOT_REACHED"),
        });
        let assign = self.create_assign_resolve(location, &name, class_expression, class_start, class_start + 1i32, class_end, AssignmentContext::DeclarationStatement);
        let mut decl = Statement::ClassDecl(Box::new(ClassDeclNode::new(location, assign.take())));
        Self::set_statement_loc(&mut decl, start_line, end_line, location);
        NodeRef::new(decl)
    }

    fn create_block_statement(&mut self, location: &JSTokenLocation, elements: NodeRef<SourceElements>, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> StmtRef {
        let mut block = Statement::Block(Box::new(BlockNode::new(location, elements.take_opt().map(Box::new), lexical_variables, function_stack)));
        Self::set_statement_loc(&mut block, start_line as u32, end_line as u32, location);
        NodeRef::new(block)
    }

    fn create_expr_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, start: JSTextPosition, end: i32) -> StmtRef {
        let mut result = Statement::ExprStatement(Box::new(ExprStatementNode::new(location, expr.take())));
        result.set_loc(start.line as u32, end as u32, start.offset, start.line_start_offset);
        NodeRef::new(result)
    }

    fn create_if_statement(&mut self, location: &JSTokenLocation, condition: ExprRef, true_block: StmtRef, false_block: StmtRef, start: i32, end: i32) -> StmtRef {
        let mut result = Statement::IfElse(Box::new(IfElseNode::new(location, condition.take(), true_block.take(), false_block.take_opt())));
        Self::set_statement_loc(&mut result, start as u32, end as u32, location);
        NodeRef::new(result)
    }

    fn create_for_loop(&mut self, location: &JSTokenLocation, initializer: ExprRef, condition: ExprRef, iter: ExprRef, statements: StmtRef, start: i32, end: i32, lexical_variables: VariableEnvironment, initializer_contains_closure: bool) -> StmtRef {
        let mut result = Statement::For(Box::new(ForNode::new(location, initializer.take_opt(), condition.take_opt(), iter.take_opt(), statements.take(), lexical_variables, initializer_contains_closure)));
        Self::set_statement_loc(&mut result, start as u32, end as u32, location);
        NodeRef::new(result)
    }

    fn create_for_in_loop(&mut self, location: &JSTokenLocation, lhs: ExprRef, iter: ExprRef, statements: StmtRef, _decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> StmtRef {
        let mut node = ForInNode::new(location, lhs.take(), iter.take(), statements.take(), lexical_variables);
        Self::set_exception_location(&mut node.throwable, e_start, e_divot, e_end);
        let mut result = Statement::ForIn(Box::new(node));
        Self::set_statement_loc(&mut result, start as u32, end as u32, location);
        NodeRef::new(result)
    }

    fn create_for_in_loop_pattern(&mut self, location: &JSTokenLocation, pattern: PatternRef, iter: ExprRef, statements: StmtRef, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> StmtRef {
        let lexpr = NodeRef::new(Expression::DestructuringAssignment(Box::new(DestructuringAssignmentNode::new(decl_location, Box::new(pattern.take()), None))));
        self.create_for_in_loop(location, lexpr, iter, statements, decl_location, e_start, e_divot, e_end, start, end, lexical_variables)
    }

    fn create_for_of_loop(&mut self, is_for_await: bool, location: &JSTokenLocation, lhs: ExprRef, iter: ExprRef, statements: StmtRef, _decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> StmtRef {
        let mut node = ForOfNode::new(is_for_await, location, lhs.take(), iter.take(), statements.take(), lexical_variables);
        Self::set_exception_location(&mut node.throwable, e_start, e_divot, e_end);
        let mut result = Statement::ForOf(Box::new(node));
        Self::set_statement_loc(&mut result, start as u32, end as u32, location);
        if is_for_await {
            self.uses_await();
        }
        NodeRef::new(result)
    }

    fn create_for_of_loop_pattern(&mut self, is_for_await: bool, location: &JSTokenLocation, pattern: PatternRef, iter: ExprRef, statements: StmtRef, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> StmtRef {
        let lexpr = NodeRef::new(Expression::DestructuringAssignment(Box::new(DestructuringAssignmentNode::new(decl_location, Box::new(pattern.take()), None))));
        self.create_for_of_loop(is_for_await, location, lexpr, iter, statements, decl_location, e_start, e_divot, e_end, start, end, lexical_variables)
    }

    fn is_binding_node(&self, pattern: &PatternRef) -> bool {
        pattern.with(|node| node.is_binding_node())
    }

    fn is_location(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_location())
    }

    fn is_assignment_location(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_assignment_location())
    }

    fn is_private_location(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_private_location())
    }

    fn is_object_literal(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_object_literal())
    }

    fn is_array_literal(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_array_literal())
    }

    fn is_object_or_array_literal(&self, expr: &ExprRef) -> bool {
        self.is_object_literal(expr) || self.is_array_literal(expr)
    }

    fn is_function_call(&self, expr: &ExprRef) -> bool {
        expr.with(|node| node.is_function_call())
    }

    fn should_skip_pause_location(&self, statement: &StmtRef) -> bool {
        statement.is_null() || statement.with(|node| node.is_label())
    }

    fn create_empty_statement(&mut self, location: &JSTokenLocation) -> StmtRef {
        NodeRef::new(Statement::EmptyStatement(Box::new(EmptyStatementNode::new(location))))
    }

    fn create_declaration_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, start: i32, end: i32) -> StmtRef {
        let mut result = Statement::DeclarationStatement(Box::new(DeclarationStatement::new(location, expr.take())));
        Self::set_statement_loc(&mut result, start as u32, end as u32, location);
        NodeRef::new(result)
    }

    fn create_return_statement(&mut self, location: &JSTokenLocation, expression: ExprRef, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let mut node = ReturnNode::new(location, expression.take_opt());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        let mut result = Statement::Return(Box::new(node));
        result.set_loc(start.line as u32, end.line as u32, start.offset, start.line_start_offset);
        NodeRef::new(result)
    }

    fn create_break_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let null_identifier = self.vm.property_names.null_identifier.clone();
        self.create_break_statement_label(location, &null_identifier, start, end)
    }

    fn create_break_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let mut node = BreakNode::new(location, ident.clone());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        let mut result = Statement::Break(Box::new(node));
        result.set_loc(start.line as u32, end.line as u32, start.offset, start.line_start_offset);
        NodeRef::new(result)
    }

    fn create_continue_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let null_identifier = self.vm.property_names.null_identifier.clone();
        self.create_continue_statement_label(location, &null_identifier, start, end)
    }

    fn create_continue_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let mut node = ContinueNode::new(location, ident.clone());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        let mut result = Statement::Continue(Box::new(node));
        result.set_loc(start.line as u32, end.line as u32, start.offset, start.line_start_offset);
        NodeRef::new(result)
    }

    fn create_try_statement(&mut self, location: &JSTokenLocation, try_block: StmtRef, catch_pattern: PatternRef, catch_block: StmtRef, finally_block: StmtRef, start_line: i32, end_line: i32, catch_environment: VariableEnvironment) -> StmtRef {
        let node = TryNode::new(location, try_block.take(), catch_pattern.take_opt().map(Box::new), catch_block.take_opt(), catch_environment, finally_block.take_opt());
        let mut result = Statement::Try(Box::new(node));
        Self::set_statement_loc(&mut result, start_line as u32, end_line as u32, location);
        NodeRef::new(result)
    }

    fn create_switch_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, first_clauses: NodeRef<ClauseList>, default_clause: NodeRef<CaseClauseNode>, second_clauses: NodeRef<ClauseList>, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> StmtRef {
        let cases = CaseBlockNode::new(first_clauses.take_opt().and_then(NodeList::into_head), default_clause.take_opt().map(Box::new), second_clauses.take_opt().and_then(NodeList::into_head));
        let mut result = Statement::Switch(Box::new(SwitchNode::new(location, expr.take(), Box::new(cases), lexical_variables, function_stack)));
        Self::set_statement_loc(&mut result, start_line as u32, end_line as u32, location);
        NodeRef::new(result)
    }

    fn create_while_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, statement: StmtRef, start_line: i32, end_line: i32) -> StmtRef {
        let mut result = Statement::While(Box::new(WhileNode::new(location, expr.take(), statement.take())));
        Self::set_statement_loc(&mut result, start_line as u32, end_line as u32, location);
        NodeRef::new(result)
    }

    fn create_with_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, statement: StmtRef, start: u32, end: JSTextPosition, start_line: u32, end_line: u32) -> StmtRef {
        self.uses_with();
        let node = WithNode::new(location, expr.take(), statement.take(), end, (end - start).as_int() as u32);
        let mut result = Statement::With(Box::new(node));
        Self::set_statement_loc(&mut result, start_line, end_line, location);
        NodeRef::new(result)
    }

    fn create_do_while_statement(&mut self, location: &JSTokenLocation, statement: StmtRef, expr: ExprRef, start_line: i32, end_line: i32) -> StmtRef {
        let mut result = Statement::DoWhile(Box::new(DoWhileNode::new(location, statement.take(), expr.take())));
        Self::set_statement_loc(&mut result, start_line as u32, end_line as u32, location);
        NodeRef::new(result)
    }

    fn create_label_statement(&mut self, location: &JSTokenLocation, ident: &Identifier, statement: StmtRef, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let mut node = LabelNode::new(location, ident.clone(), statement.take());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        NodeRef::new(Statement::Label(Box::new(node)))
    }

    fn create_throw_statement(&mut self, location: &JSTokenLocation, expr: ExprRef, start: JSTextPosition, end: JSTextPosition) -> StmtRef {
        let mut node = ThrowNode::new(location, expr.take());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        let mut result = Statement::Throw(Box::new(node));
        result.set_loc(start.line as u32, end.line as u32, start.offset, start.line_start_offset);
        NodeRef::new(result)
    }

    fn create_debugger(&mut self, location: &JSTokenLocation, start_line: i32, end_line: i32) -> StmtRef {
        let mut result = Statement::DebuggerStatement(Box::new(DebuggerStatementNode::new(location)));
        Self::set_statement_loc(&mut result, start_line as u32, end_line as u32, location);
        NodeRef::new(result)
    }

    fn create_module_name(&mut self, location: &JSTokenLocation, module_name: &Identifier) -> NodeRef<ModuleNameNode> {
        NodeRef::new(ModuleNameNode::new(location, module_name.clone()))
    }

    fn create_import_specifier(&mut self, location: &JSTokenLocation, imported_name: &Identifier, local_name: &Identifier) -> NodeRef<ImportSpecifierNode> {
        NodeRef::new(ImportSpecifierNode::new(location, imported_name.clone(), local_name.clone()))
    }

    fn create_import_specifier_list(&mut self) -> NodeRef<ImportSpecifierListNode> {
        NodeRef::new(ImportSpecifierListNode::default())
    }

    fn append_import_specifier(&mut self, specifier_list: &NodeRef<ImportSpecifierListNode>, specifier: NodeRef<ImportSpecifierNode>) {
        specifier_list.with_mut(|list| list.append(Box::new(specifier.take())));
    }

    fn create_import_attributes_list(&mut self) -> NodeRef<ImportAttributesListNode> {
        NodeRef::new(ImportAttributesListNode::default())
    }

    fn append_import_assertion(&mut self, attributes_list: &NodeRef<ImportAttributesListNode>, key: &Identifier, value: &Identifier) {
        attributes_list.with_mut(|list| list.append(key.clone(), value.clone()));
    }

    fn create_import_declaration(&mut self, location: &JSTokenLocation, type_: ImportType, import_specifier_list: NodeRef<ImportSpecifierListNode>, module_name: NodeRef<ModuleNameNode>, import_attributes_list: NodeRef<ImportAttributesListNode>) -> StmtRef {
        let node = ImportDeclarationNode::new(location, type_, Box::new(import_specifier_list.take()), Box::new(module_name.take()), import_attributes_list.take_opt().map(Box::new));
        NodeRef::new(Statement::ImportDeclaration(Box::new(node)))
    }

    // Segunda fatia (`ASTBuilder.h`, do `createExportAllDeclaration` em diante, mais as definições de
    // `ASTBuilder.h` para `makeBinaryNode` e afins): os métodos restantes do trait.
    include!("ast_builder_part2.rs");
}
