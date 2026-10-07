//! Árvore sintática abstrata do CPython 3.13, espelhando `Parser/Python.asdl`.
//!
//! Cada tipo soma do ASDL vira um par `struct` (posição + `kind`) e `enum` (construtores), com os
//! nomes de construtores e campos iguais aos do ASDL; `type` vira `r#type`. Campos `?` são `Option`,
//! campos `*` são `Vec`. As duas exceções conhecidas do ASDL ficam explícitas no tipo: `Dict.keys`
//! guarda `None` para `**d` e `arguments.kw_defaults` guarda `None` para parâmetro só-nomeado sem
//! padrão, exatamente como as listas do CPython.
//!
//! `dump` reproduz `ast.dump(tree)` do 3.13 com os argumentos padrão (campos nomeados, sem
//! atributos de posição, sem indentação): campo opcional ausente é omitido, lista vazia aparece como
//! `[]` e as constantes saem com o `repr` do Python.

/// Atributos de posição de `stmt`, `expr`, `excepthandler`, `arg`, `keyword` e `alias`
/// (`int lineno, int col_offset, int? end_lineno, int? end_col_offset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub lineno: usize,
    pub col_offset: usize,
    pub end_lineno: Option<usize>,
    pub end_col_offset: Option<usize>,
}

/// Atributos de posição de `pattern` e `type_param`, onde o fim é obrigatório.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FullPos {
    pub lineno: usize,
    pub col_offset: usize,
    pub end_lineno: usize,
    pub end_col_offset: usize,
}

/// `mod`.
#[derive(Debug, Clone, PartialEq)]
pub enum Mod {
    Module { body: Vec<Stmt>, type_ignores: Vec<TypeIgnore> },
    Interactive { body: Vec<Stmt> },
    Expression { body: Box<Expr> },
    FunctionType { argtypes: Vec<Expr>, returns: Box<Expr> },
}

/// `stmt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    FunctionDef {
        name: String,
        args: Box<Arguments>,
        body: Vec<Stmt>,
        decorator_list: Vec<Expr>,
        returns: Option<Box<Expr>>,
        type_comment: Option<String>,
        type_params: Vec<TypeParam>,
    },
    AsyncFunctionDef {
        name: String,
        args: Box<Arguments>,
        body: Vec<Stmt>,
        decorator_list: Vec<Expr>,
        returns: Option<Box<Expr>>,
        type_comment: Option<String>,
        type_params: Vec<TypeParam>,
    },
    ClassDef {
        name: String,
        bases: Vec<Expr>,
        keywords: Vec<Keyword>,
        body: Vec<Stmt>,
        decorator_list: Vec<Expr>,
        type_params: Vec<TypeParam>,
    },
    Return { value: Option<Box<Expr>> },
    Delete { targets: Vec<Expr> },
    Assign { targets: Vec<Expr>, value: Box<Expr>, type_comment: Option<String> },
    TypeAlias { name: Box<Expr>, type_params: Vec<TypeParam>, value: Box<Expr> },
    AugAssign { target: Box<Expr>, op: Operator, value: Box<Expr> },
    AnnAssign { target: Box<Expr>, annotation: Box<Expr>, value: Option<Box<Expr>>, simple: i64 },
    For {
        target: Box<Expr>,
        iter: Box<Expr>,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
        type_comment: Option<String>,
    },
    AsyncFor {
        target: Box<Expr>,
        iter: Box<Expr>,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
        type_comment: Option<String>,
    },
    While { test: Box<Expr>, body: Vec<Stmt>, orelse: Vec<Stmt> },
    If { test: Box<Expr>, body: Vec<Stmt>, orelse: Vec<Stmt> },
    With { items: Vec<WithItem>, body: Vec<Stmt>, type_comment: Option<String> },
    AsyncWith { items: Vec<WithItem>, body: Vec<Stmt>, type_comment: Option<String> },
    Match { subject: Box<Expr>, cases: Vec<MatchCase> },
    Raise { exc: Option<Box<Expr>>, cause: Option<Box<Expr>> },
    Try { body: Vec<Stmt>, handlers: Vec<ExceptHandler>, orelse: Vec<Stmt>, finalbody: Vec<Stmt> },
    TryStar { body: Vec<Stmt>, handlers: Vec<ExceptHandler>, orelse: Vec<Stmt>, finalbody: Vec<Stmt> },
    Assert { test: Box<Expr>, msg: Option<Box<Expr>> },
    Import { names: Vec<Alias> },
    ImportFrom { module: Option<String>, names: Vec<Alias>, level: Option<i64> },
    Global { names: Vec<String> },
    Nonlocal { names: Vec<String> },
    Expr { value: Box<Expr> },
    Pass,
    Break,
    Continue,
}

/// `expr`.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    BoolOp { op: BoolOp, values: Vec<Expr> },
    NamedExpr { target: Box<Expr>, value: Box<Expr> },
    BinOp { left: Box<Expr>, op: Operator, right: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    Lambda { args: Box<Arguments>, body: Box<Expr> },
    IfExp { test: Box<Expr>, body: Box<Expr>, orelse: Box<Expr> },
    /// `None` em `keys` marca um `**d`.
    Dict { keys: Vec<Option<Expr>>, values: Vec<Expr> },
    Set { elts: Vec<Expr> },
    ListComp { elt: Box<Expr>, generators: Vec<Comprehension> },
    SetComp { elt: Box<Expr>, generators: Vec<Comprehension> },
    DictComp { key: Box<Expr>, value: Box<Expr>, generators: Vec<Comprehension> },
    GeneratorExp { elt: Box<Expr>, generators: Vec<Comprehension> },
    Await { value: Box<Expr> },
    Yield { value: Option<Box<Expr>> },
    YieldFrom { value: Box<Expr> },
    Compare { left: Box<Expr>, ops: Vec<CmpOp>, comparators: Vec<Expr> },
    Call { func: Box<Expr>, args: Vec<Expr>, keywords: Vec<Keyword> },
    FormattedValue { value: Box<Expr>, conversion: i64, format_spec: Option<Box<Expr>> },
    JoinedStr { values: Vec<Expr> },
    Constant { value: Constant, kind: Option<String> },
    Attribute { value: Box<Expr>, attr: String, ctx: ExprContext },
    Subscript { value: Box<Expr>, slice: Box<Expr>, ctx: ExprContext },
    Starred { value: Box<Expr>, ctx: ExprContext },
    Name { id: String, ctx: ExprContext },
    List { elts: Vec<Expr>, ctx: ExprContext },
    Tuple { elts: Vec<Expr>, ctx: ExprContext },
    Slice { lower: Option<Box<Expr>>, upper: Option<Box<Expr>>, step: Option<Box<Expr>> },
}

/// O tipo embutido `constant` do ASDL.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    None,
    Bool(bool),
    /// Inteiro em decimal, sem sinal nem zeros à esquerda, até o `int` arbitrário da fatia 19.
    Int(String),
    Float(f64),
    /// Parte real e parte imaginária.
    Complex(f64, f64),
    Str(String),
    Bytes(Vec<u8>),
    Ellipsis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprContext {
    Load,
    Store,
    Del,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Add,
    Sub,
    Mult,
    MatMult,
    Div,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
    FloorDiv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Invert,
    Not,
    UAdd,
    USub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
}

/// `comprehension` (sem atributos de posição).
#[derive(Debug, Clone, PartialEq)]
pub struct Comprehension {
    pub target: Expr,
    pub iter: Expr,
    pub ifs: Vec<Expr>,
    pub is_async: i64,
}

/// `excepthandler`, cujo único construtor é `ExceptHandler`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExceptHandler {
    pub r#type: Option<Box<Expr>>,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

/// `arguments` (sem atributos de posição).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arguments {
    pub posonlyargs: Vec<Arg>,
    pub args: Vec<Arg>,
    pub vararg: Option<Box<Arg>>,
    pub kwonlyargs: Vec<Arg>,
    /// `None` para parâmetro só-nomeado sem valor padrão.
    pub kw_defaults: Vec<Option<Expr>>,
    pub kwarg: Option<Box<Arg>>,
    pub defaults: Vec<Expr>,
}

/// `arg`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub arg: String,
    pub annotation: Option<Box<Expr>>,
    pub type_comment: Option<String>,
    pub pos: Pos,
}

/// `keyword`; `arg` é `None` em `**kwargs`.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyword {
    pub arg: Option<String>,
    pub value: Expr,
    pub pos: Pos,
}

/// `alias`.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub name: String,
    pub asname: Option<String>,
    pub pos: Pos,
}

/// `withitem` (sem atributos de posição).
#[derive(Debug, Clone, PartialEq)]
pub struct WithItem {
    pub context_expr: Expr,
    pub optional_vars: Option<Box<Expr>>,
}

/// `match_case` (sem atributos de posição).
#[derive(Debug, Clone, PartialEq)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Vec<Stmt>,
}

/// `pattern`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub pos: FullPos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    MatchValue { value: Box<Expr> },
    MatchSingleton { value: Constant },
    MatchSequence { patterns: Vec<Pattern> },
    MatchMapping { keys: Vec<Expr>, patterns: Vec<Pattern>, rest: Option<String> },
    MatchClass { cls: Box<Expr>, patterns: Vec<Pattern>, kwd_attrs: Vec<String>, kwd_patterns: Vec<Pattern> },
    MatchStar { name: Option<String> },
    MatchAs { pattern: Option<Box<Pattern>>, name: Option<String> },
    MatchOr { patterns: Vec<Pattern> },
}

/// `type_ignore`, cujo único construtor é `TypeIgnore`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeIgnore {
    pub lineno: i64,
    pub tag: String,
}

/// `type_param`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub kind: TypeParamKind,
    pub pos: FullPos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeParamKind {
    TypeVar { name: String, bound: Option<Box<Expr>>, default_value: Option<Box<Expr>> },
    ParamSpec { name: String, default_value: Option<Box<Expr>> },
    TypeVarTuple { name: String, default_value: Option<Box<Expr>> },
}

// ---------------------------------------------------------------------------------------------
// dump

/// `ast.dump(tree)` do 3.13 com os argumentos padrão.
pub fn dump(module: &Mod) -> String {
    match module {
        Mod::Module { body, type_ignores } => Node::new("Module")
            .field("body", list(body, dump_stmt))
            .field("type_ignores", list(type_ignores, dump_type_ignore))
            .end(),
        Mod::Interactive { body } => Node::new("Interactive").field("body", list(body, dump_stmt)).end(),
        Mod::Expression { body } => Node::new("Expression").field("body", dump_expr(body)).end(),
        Mod::FunctionType { argtypes, returns } => Node::new("FunctionType")
            .field("argtypes", list(argtypes, dump_expr))
            .field("returns", dump_expr(returns))
            .end(),
    }
}

/// Monta `Nome(campo=valor, ...)` na ordem dos campos do ASDL.
struct Node {
    text: String,
    any: bool,
}

impl Node {
    fn new(name: &str) -> Node {
        Node { text: format!("{name}("), any: false }
    }

    fn field(mut self, name: &str, value: String) -> Node {
        if self.any {
            self.text.push_str(", ");
        }
        self.text.push_str(name);
        self.text.push('=');
        self.text.push_str(&value);
        self.any = true;
        self
    }

    /// Campo `?`: o `ast.dump` omite o que vale `None`.
    fn opt(self, name: &str, value: Option<String>) -> Node {
        match value {
            Some(value) => self.field(name, value),
            None => self,
        }
    }

    fn end(mut self) -> String {
        self.text.push(')');
        self.text
    }
}

fn list<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
    let parts: Vec<String> = items.iter().map(f).collect();
    format!("[{}]", parts.join(", "))
}

fn opt_expr(value: &Option<Box<Expr>>) -> Option<String> {
    value.as_deref().map(dump_expr)
}

fn opt_str(value: &Option<String>) -> Option<String> {
    value.as_deref().map(str_repr)
}

/// Elemento de lista que pode ser `None` (`Dict.keys`, `kw_defaults`): aparece como `None`.
fn maybe_expr(value: &Option<Expr>) -> String {
    match value {
        Some(e) => dump_expr(e),
        None => "None".to_string(),
    }
}

fn unit(name: &str) -> String {
    format!("{name}()")
}

fn dump_stmt(stmt: &Stmt) -> String {
    use StmtKind as S;
    match &stmt.kind {
        S::FunctionDef { name, args, body, decorator_list, returns, type_comment, type_params }
        | S::AsyncFunctionDef { name, args, body, decorator_list, returns, type_comment, type_params } => {
            let ctor = if matches!(stmt.kind, S::FunctionDef { .. }) { "FunctionDef" } else { "AsyncFunctionDef" };
            Node::new(ctor)
                .field("name", str_repr(name))
                .field("args", dump_arguments(args))
                .field("body", list(body, dump_stmt))
                .field("decorator_list", list(decorator_list, dump_expr))
                .opt("returns", opt_expr(returns))
                .opt("type_comment", opt_str(type_comment))
                .field("type_params", list(type_params, dump_type_param))
                .end()
        }
        S::ClassDef { name, bases, keywords, body, decorator_list, type_params } => Node::new("ClassDef")
            .field("name", str_repr(name))
            .field("bases", list(bases, dump_expr))
            .field("keywords", list(keywords, dump_keyword))
            .field("body", list(body, dump_stmt))
            .field("decorator_list", list(decorator_list, dump_expr))
            .field("type_params", list(type_params, dump_type_param))
            .end(),
        S::Return { value } => Node::new("Return").opt("value", opt_expr(value)).end(),
        S::Delete { targets } => Node::new("Delete").field("targets", list(targets, dump_expr)).end(),
        S::Assign { targets, value, type_comment } => Node::new("Assign")
            .field("targets", list(targets, dump_expr))
            .field("value", dump_expr(value))
            .opt("type_comment", opt_str(type_comment))
            .end(),
        S::TypeAlias { name, type_params, value } => Node::new("TypeAlias")
            .field("name", dump_expr(name))
            .field("type_params", list(type_params, dump_type_param))
            .field("value", dump_expr(value))
            .end(),
        S::AugAssign { target, op, value } => Node::new("AugAssign")
            .field("target", dump_expr(target))
            .field("op", unit(operator_name(*op)))
            .field("value", dump_expr(value))
            .end(),
        S::AnnAssign { target, annotation, value, simple } => Node::new("AnnAssign")
            .field("target", dump_expr(target))
            .field("annotation", dump_expr(annotation))
            .opt("value", opt_expr(value))
            .field("simple", simple.to_string())
            .end(),
        S::For { target, iter, body, orelse, type_comment }
        | S::AsyncFor { target, iter, body, orelse, type_comment } => {
            let ctor = if matches!(stmt.kind, S::For { .. }) { "For" } else { "AsyncFor" };
            Node::new(ctor)
                .field("target", dump_expr(target))
                .field("iter", dump_expr(iter))
                .field("body", list(body, dump_stmt))
                .field("orelse", list(orelse, dump_stmt))
                .opt("type_comment", opt_str(type_comment))
                .end()
        }
        S::While { test, body, orelse } | S::If { test, body, orelse } => {
            let ctor = if matches!(stmt.kind, S::While { .. }) { "While" } else { "If" };
            Node::new(ctor)
                .field("test", dump_expr(test))
                .field("body", list(body, dump_stmt))
                .field("orelse", list(orelse, dump_stmt))
                .end()
        }
        S::With { items, body, type_comment } | S::AsyncWith { items, body, type_comment } => {
            let ctor = if matches!(stmt.kind, S::With { .. }) { "With" } else { "AsyncWith" };
            Node::new(ctor)
                .field("items", list(items, dump_withitem))
                .field("body", list(body, dump_stmt))
                .opt("type_comment", opt_str(type_comment))
                .end()
        }
        S::Match { subject, cases } => Node::new("Match")
            .field("subject", dump_expr(subject))
            .field("cases", list(cases, dump_match_case))
            .end(),
        S::Raise { exc, cause } => {
            Node::new("Raise").opt("exc", opt_expr(exc)).opt("cause", opt_expr(cause)).end()
        }
        S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
            let ctor = if matches!(stmt.kind, S::Try { .. }) { "Try" } else { "TryStar" };
            Node::new(ctor)
                .field("body", list(body, dump_stmt))
                .field("handlers", list(handlers, dump_excepthandler))
                .field("orelse", list(orelse, dump_stmt))
                .field("finalbody", list(finalbody, dump_stmt))
                .end()
        }
        S::Assert { test, msg } => {
            Node::new("Assert").field("test", dump_expr(test)).opt("msg", opt_expr(msg)).end()
        }
        S::Import { names } => Node::new("Import").field("names", list(names, dump_alias)).end(),
        S::ImportFrom { module, names, level } => Node::new("ImportFrom")
            .opt("module", opt_str(module))
            .field("names", list(names, dump_alias))
            .opt("level", level.map(|l| l.to_string()))
            .end(),
        S::Global { names } => Node::new("Global").field("names", list(names, |n| str_repr(n))).end(),
        S::Nonlocal { names } => Node::new("Nonlocal").field("names", list(names, |n| str_repr(n))).end(),
        S::Expr { value } => Node::new("Expr").field("value", dump_expr(value)).end(),
        S::Pass => unit("Pass"),
        S::Break => unit("Break"),
        S::Continue => unit("Continue"),
    }
}

/// Representação de uma expressão no formato do `ast.dump`.
pub fn dump_expr(expr: &Expr) -> String {
    use ExprKind as E;
    match &expr.kind {
        E::BoolOp { op, values } => Node::new("BoolOp")
            .field("op", unit(boolop_name(*op)))
            .field("values", list(values, dump_expr))
            .end(),
        E::NamedExpr { target, value } => Node::new("NamedExpr")
            .field("target", dump_expr(target))
            .field("value", dump_expr(value))
            .end(),
        E::BinOp { left, op, right } => Node::new("BinOp")
            .field("left", dump_expr(left))
            .field("op", unit(operator_name(*op)))
            .field("right", dump_expr(right))
            .end(),
        E::UnaryOp { op, operand } => Node::new("UnaryOp")
            .field("op", unit(unaryop_name(*op)))
            .field("operand", dump_expr(operand))
            .end(),
        E::Lambda { args, body } => Node::new("Lambda")
            .field("args", dump_arguments(args))
            .field("body", dump_expr(body))
            .end(),
        E::IfExp { test, body, orelse } => Node::new("IfExp")
            .field("test", dump_expr(test))
            .field("body", dump_expr(body))
            .field("orelse", dump_expr(orelse))
            .end(),
        E::Dict { keys, values } => Node::new("Dict")
            .field("keys", list(keys, maybe_expr))
            .field("values", list(values, dump_expr))
            .end(),
        E::Set { elts } => Node::new("Set").field("elts", list(elts, dump_expr)).end(),
        E::ListComp { elt, generators }
        | E::SetComp { elt, generators }
        | E::GeneratorExp { elt, generators } => {
            let ctor = match expr.kind {
                E::ListComp { .. } => "ListComp",
                E::SetComp { .. } => "SetComp",
                _ => "GeneratorExp",
            };
            Node::new(ctor)
                .field("elt", dump_expr(elt))
                .field("generators", list(generators, dump_comprehension))
                .end()
        }
        E::DictComp { key, value, generators } => Node::new("DictComp")
            .field("key", dump_expr(key))
            .field("value", dump_expr(value))
            .field("generators", list(generators, dump_comprehension))
            .end(),
        E::Await { value } => Node::new("Await").field("value", dump_expr(value)).end(),
        E::Yield { value } => Node::new("Yield").opt("value", opt_expr(value)).end(),
        E::YieldFrom { value } => Node::new("YieldFrom").field("value", dump_expr(value)).end(),
        E::Compare { left, ops, comparators } => Node::new("Compare")
            .field("left", dump_expr(left))
            .field("ops", list(ops, |o| unit(cmpop_name(*o))))
            .field("comparators", list(comparators, dump_expr))
            .end(),
        E::Call { func, args, keywords } => Node::new("Call")
            .field("func", dump_expr(func))
            .field("args", list(args, dump_expr))
            .field("keywords", list(keywords, dump_keyword))
            .end(),
        E::FormattedValue { value, conversion, format_spec } => Node::new("FormattedValue")
            .field("value", dump_expr(value))
            .field("conversion", conversion.to_string())
            .opt("format_spec", opt_expr(format_spec))
            .end(),
        E::JoinedStr { values } => Node::new("JoinedStr").field("values", list(values, dump_expr)).end(),
        E::Constant { value, kind } => Node::new("Constant")
            .field("value", constant_repr(value))
            .opt("kind", opt_str(kind))
            .end(),
        E::Attribute { value, attr, ctx } => Node::new("Attribute")
            .field("value", dump_expr(value))
            .field("attr", str_repr(attr))
            .field("ctx", unit(ctx_name(*ctx)))
            .end(),
        E::Subscript { value, slice, ctx } => Node::new("Subscript")
            .field("value", dump_expr(value))
            .field("slice", dump_expr(slice))
            .field("ctx", unit(ctx_name(*ctx)))
            .end(),
        E::Starred { value, ctx } => Node::new("Starred")
            .field("value", dump_expr(value))
            .field("ctx", unit(ctx_name(*ctx)))
            .end(),
        E::Name { id, ctx } => {
            Node::new("Name").field("id", str_repr(id)).field("ctx", unit(ctx_name(*ctx))).end()
        }
        E::List { elts, ctx } | E::Tuple { elts, ctx } => {
            let ctor = if matches!(expr.kind, E::List { .. }) { "List" } else { "Tuple" };
            Node::new(ctor)
                .field("elts", list(elts, dump_expr))
                .field("ctx", unit(ctx_name(*ctx)))
                .end()
        }
        E::Slice { lower, upper, step } => Node::new("Slice")
            .opt("lower", opt_expr(lower))
            .opt("upper", opt_expr(upper))
            .opt("step", opt_expr(step))
            .end(),
    }
}

fn dump_comprehension(c: &Comprehension) -> String {
    Node::new("comprehension")
        .field("target", dump_expr(&c.target))
        .field("iter", dump_expr(&c.iter))
        .field("ifs", list(&c.ifs, dump_expr))
        .field("is_async", c.is_async.to_string())
        .end()
}

fn dump_excepthandler(h: &ExceptHandler) -> String {
    Node::new("ExceptHandler")
        .opt("type", opt_expr(&h.r#type))
        .opt("name", opt_str(&h.name))
        .field("body", list(&h.body, dump_stmt))
        .end()
}

fn dump_arguments(a: &Arguments) -> String {
    Node::new("arguments")
        .field("posonlyargs", list(&a.posonlyargs, dump_arg))
        .field("args", list(&a.args, dump_arg))
        .opt("vararg", a.vararg.as_deref().map(dump_arg))
        .field("kwonlyargs", list(&a.kwonlyargs, dump_arg))
        .field("kw_defaults", list(&a.kw_defaults, maybe_expr))
        .opt("kwarg", a.kwarg.as_deref().map(dump_arg))
        .field("defaults", list(&a.defaults, dump_expr))
        .end()
}

fn dump_arg(a: &Arg) -> String {
    Node::new("arg")
        .field("arg", str_repr(&a.arg))
        .opt("annotation", opt_expr(&a.annotation))
        .opt("type_comment", opt_str(&a.type_comment))
        .end()
}

fn dump_keyword(k: &Keyword) -> String {
    Node::new("keyword").opt("arg", opt_str(&k.arg)).field("value", dump_expr(&k.value)).end()
}

fn dump_alias(a: &Alias) -> String {
    Node::new("alias").field("name", str_repr(&a.name)).opt("asname", opt_str(&a.asname)).end()
}

fn dump_withitem(w: &WithItem) -> String {
    Node::new("withitem")
        .field("context_expr", dump_expr(&w.context_expr))
        .opt("optional_vars", opt_expr(&w.optional_vars))
        .end()
}

fn dump_match_case(m: &MatchCase) -> String {
    Node::new("match_case")
        .field("pattern", dump_pattern(&m.pattern))
        .opt("guard", opt_expr(&m.guard))
        .field("body", list(&m.body, dump_stmt))
        .end()
}

fn dump_pattern(p: &Pattern) -> String {
    use PatternKind as P;
    match &p.kind {
        P::MatchValue { value } => Node::new("MatchValue").field("value", dump_expr(value)).end(),
        P::MatchSingleton { value } => {
            Node::new("MatchSingleton").field("value", constant_repr(value)).end()
        }
        P::MatchSequence { patterns } => {
            Node::new("MatchSequence").field("patterns", list(patterns, dump_pattern)).end()
        }
        P::MatchMapping { keys, patterns, rest } => Node::new("MatchMapping")
            .field("keys", list(keys, dump_expr))
            .field("patterns", list(patterns, dump_pattern))
            .opt("rest", opt_str(rest))
            .end(),
        P::MatchClass { cls, patterns, kwd_attrs, kwd_patterns } => Node::new("MatchClass")
            .field("cls", dump_expr(cls))
            .field("patterns", list(patterns, dump_pattern))
            .field("kwd_attrs", list(kwd_attrs, |n| str_repr(n)))
            .field("kwd_patterns", list(kwd_patterns, dump_pattern))
            .end(),
        P::MatchStar { name } => Node::new("MatchStar").opt("name", opt_str(name)).end(),
        P::MatchAs { pattern, name } => Node::new("MatchAs")
            .opt("pattern", pattern.as_deref().map(dump_pattern))
            .opt("name", opt_str(name))
            .end(),
        P::MatchOr { patterns } => Node::new("MatchOr").field("patterns", list(patterns, dump_pattern)).end(),
    }
}

fn dump_type_ignore(t: &TypeIgnore) -> String {
    Node::new("TypeIgnore").field("lineno", t.lineno.to_string()).field("tag", str_repr(&t.tag)).end()
}

fn dump_type_param(t: &TypeParam) -> String {
    use TypeParamKind as T;
    match &t.kind {
        T::TypeVar { name, bound, default_value } => Node::new("TypeVar")
            .field("name", str_repr(name))
            .opt("bound", opt_expr(bound))
            .opt("default_value", opt_expr(default_value))
            .end(),
        T::ParamSpec { name, default_value } => Node::new("ParamSpec")
            .field("name", str_repr(name))
            .opt("default_value", opt_expr(default_value))
            .end(),
        T::TypeVarTuple { name, default_value } => Node::new("TypeVarTuple")
            .field("name", str_repr(name))
            .opt("default_value", opt_expr(default_value))
            .end(),
    }
}

fn ctx_name(c: ExprContext) -> &'static str {
    match c {
        ExprContext::Load => "Load",
        ExprContext::Store => "Store",
        ExprContext::Del => "Del",
    }
}

fn boolop_name(o: BoolOp) -> &'static str {
    match o {
        BoolOp::And => "And",
        BoolOp::Or => "Or",
    }
}

fn operator_name(o: Operator) -> &'static str {
    use Operator as O;
    match o {
        O::Add => "Add",
        O::Sub => "Sub",
        O::Mult => "Mult",
        O::MatMult => "MatMult",
        O::Div => "Div",
        O::Mod => "Mod",
        O::Pow => "Pow",
        O::LShift => "LShift",
        O::RShift => "RShift",
        O::BitOr => "BitOr",
        O::BitXor => "BitXor",
        O::BitAnd => "BitAnd",
        O::FloorDiv => "FloorDiv",
    }
}

fn unaryop_name(o: UnaryOp) -> &'static str {
    match o {
        UnaryOp::Invert => "Invert",
        UnaryOp::Not => "Not",
        UnaryOp::UAdd => "UAdd",
        UnaryOp::USub => "USub",
    }
}

fn cmpop_name(o: CmpOp) -> &'static str {
    use CmpOp as C;
    match o {
        C::Eq => "Eq",
        C::NotEq => "NotEq",
        C::Lt => "Lt",
        C::LtE => "LtE",
        C::Gt => "Gt",
        C::GtE => "GtE",
        C::Is => "Is",
        C::IsNot => "IsNot",
        C::In => "In",
        C::NotIn => "NotIn",
    }
}

// ---------------------------------------------------------------------------------------------
// repr das constantes

/// `repr()` de uma constante, como o `ast.dump` imprime.
pub fn constant_repr(c: &Constant) -> String {
    match c {
        Constant::None => "None".to_string(),
        Constant::Bool(true) => "True".to_string(),
        Constant::Bool(false) => "False".to_string(),
        Constant::Int(digits) => digits.clone(),
        Constant::Float(x) => float_repr(*x, true),
        Constant::Complex(re, im) => complex_repr(*re, *im),
        Constant::Str(s) => str_repr(s),
        Constant::Bytes(b) => bytes_repr(b),
        Constant::Ellipsis => "Ellipsis".to_string(),
    }
}

/// `float_repr_style == 'short'` do `Python/pystrtod.c`: menor sequência de dígitos que volta ao
/// mesmo `double`, em notação fixa quando o expoente decimal fica em `-4 <= exp < 16` e científica
/// (`1e-05`, `1e+16`) fora disso. `add_dot_0` acrescenta o `.0` dos inteiros, que o `repr` de
/// `float` usa e o de `complex` não.
fn float_repr(x: f64, add_dot_0: bool) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    // O `{:e}` do Rust já entrega os dígitos mínimos de ida e volta, como o modo 0 do `dtoa.c`.
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decpt > -4 && decpt <= 16 {
        let len = digits.len() as i64;
        if decpt <= 0 {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-decpt) as usize));
            out.push_str(&digits);
        } else if decpt >= len {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', (decpt - len) as usize));
            if add_dot_0 {
                out.push_str(".0");
            }
        } else {
            out.push_str(&digits[..decpt as usize]);
            out.push('.');
            out.push_str(&digits[decpt as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        out.push_str(&format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs()));
    }
    out
}

/// `complex_repr` do `Objects/complexobject.c`: só a parte imaginária quando a real é `+0.0`, senão
/// `(real+imagj)`.
fn complex_repr(re: f64, im: f64) -> String {
    if re == 0.0 && re.is_sign_positive() {
        return format!("{}j", float_repr(im, false));
    }
    let imag = float_repr(im, false);
    let sign = if imag.starts_with('-') { "" } else { "+" };
    format!("({}{sign}{imag}j)", float_repr(re, false))
}

/// `repr()` de `str` (`unicode_repr` do `Objects/unicodeobject.c`).
pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            c if is_printable(c) => out.push(c),
            c => {
                let v = c as u32;
                if v <= 0xff {
                    out.push_str(&format!("\\x{v:02x}"));
                } else if v <= 0xffff {
                    out.push_str(&format!("\\u{v:04x}"));
                } else {
                    out.push_str(&format!("\\U{v:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

use crate::object::is_printable;

/// `repr()` de `bytes` (`bytes_repr` do `Objects/bytesobject.c`).
pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') { b'"' } else { b'\'' };
    let mut out = String::with_capacity(b.len() + 3);
    out.push('b');
    out.push(char::from(quote));
    for &c in b {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(char::from(c));
            }
            0x20..=0x7e => out.push(char::from(c)),
            c => out.push_str(&format!("\\x{c:02x}")),
        }
    }
    out.push(char::from(quote));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(kind: ExprKind) -> Expr {
        Expr { kind, pos: Pos::default() }
    }

    fn s(kind: StmtKind) -> Stmt {
        Stmt { kind, pos: Pos::default() }
    }

    fn name(id: &str) -> Expr {
        e(ExprKind::Name { id: id.to_string(), ctx: ExprContext::Load })
    }

    fn constant(value: Constant) -> Expr {
        e(ExprKind::Constant { value, kind: None })
    }

    fn module(body: Vec<Stmt>) -> Mod {
        Mod::Module { body, type_ignores: vec![] }
    }

    #[test]
    fn empty_module() {
        assert_eq!(dump(&module(vec![])), "Module(body=[], type_ignores=[])");
    }

    #[test]
    fn print_call() {
        let call = e(ExprKind::Call {
            func: Box::new(name("print")),
            args: vec![constant(Constant::Str("hi".to_string()))],
            keywords: vec![],
        });
        let tree = module(vec![s(StmtKind::Expr { value: Box::new(call) })]);
        assert_eq!(
            dump(&tree),
            "Module(body=[Expr(value=Call(func=Name(id='print', ctx=Load()), \
             args=[Constant(value='hi')], keywords=[]))], type_ignores=[])"
        );
    }

    #[test]
    fn relative_import_omits_missing_module() {
        let stmt = s(StmtKind::ImportFrom {
            module: None,
            names: vec![Alias { name: "x".to_string(), asname: Some("y".to_string()), pos: Pos::default() }],
            level: Some(1),
        });
        assert_eq!(
            dump(&module(vec![stmt])),
            "Module(body=[ImportFrom(names=[alias(name='x', asname='y')], level=1)], type_ignores=[])"
        );
    }

    #[test]
    fn dict_with_unpacking_and_constants() {
        let dict = e(ExprKind::Dict {
            keys: vec![None, Some(constant(Constant::Str("it's".to_string())))],
            values: vec![name("a"), constant(Constant::Float(1e16))],
        });
        let tree = Mod::Expression { body: Box::new(dict) };
        assert_eq!(
            dump(&tree),
            "Expression(body=Dict(keys=[None, Constant(value=\"it's\")], \
             values=[Name(id='a', ctx=Load()), Constant(value=1e+16)]))"
        );
    }

    #[test]
    fn function_def_with_empty_arguments() {
        let def = s(StmtKind::FunctionDef {
            name: "f".to_string(),
            args: Box::default(),
            body: vec![s(StmtKind::Pass)],
            decorator_list: vec![],
            returns: None,
            type_comment: None,
            type_params: vec![],
        });
        assert_eq!(
            dump(&module(vec![def])),
            "Module(body=[FunctionDef(name='f', args=arguments(posonlyargs=[], args=[], \
             kwonlyargs=[], kw_defaults=[], defaults=[]), body=[Pass()], decorator_list=[], \
             type_params=[])], type_ignores=[])"
        );
    }

    #[test]
    fn constant_reprs() {
        assert_eq!(constant_repr(&Constant::Complex(0.0, 1.0)), "1j");
        assert_eq!(constant_repr(&Constant::Complex(0.0, 2.5)), "2.5j");
        assert_eq!(constant_repr(&Constant::Float(0.1)), "0.1");
        assert_eq!(constant_repr(&Constant::Float(1e-5)), "1e-05");
        assert_eq!(constant_repr(&Constant::Float(0.0001)), "0.0001");
        assert_eq!(constant_repr(&Constant::Float(1e15)), "1000000000000000.0");
        assert_eq!(constant_repr(&Constant::Float(f64::INFINITY)), "inf");
        assert_eq!(constant_repr(&Constant::Bytes(b"a'\x00\xff".to_vec())), "b\"a'\\x00\\xff\"");
        assert_eq!(constant_repr(&Constant::Str("\u{a0}é\n".to_string())), "'\\xa0é\\n'");
        assert_eq!(constant_repr(&Constant::Ellipsis), "Ellipsis");
    }
}
