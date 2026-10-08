//! O caminho inverso de `astnative`: uma árvore de objetos `ast.*` do Python vira a AST nativa do
//! compilador (`crate::ast`), como o `PyAST_obj2mod` do CPython (`Python/Python-ast.c`) seguido do
//! `_PyAST_Validate` (`Python/ast.c`). É o que `compile(tree, ...)` usa: a árvore é compilada direto,
//! sem passar por `ast.unparse`, então ela guarda as posições que o usuário pôs e aceita o que o
//! texto não consegue dizer (um `JoinedStr` com qualquer expressão como valor, por exemplo).
//!
//! As mensagens são as do CPython: campo obrigatório ausente (`required field "lineno" missing from
//! stmt`), tipo errado de nó (`expected some sort of expr, but got ...`), lista que não é lista, e as
//! validações estruturais de `ast.c` (`empty body on FunctionDef`, `Compare with no comparators`...).
//! Campo `*` ausente vale lista vazia, campo `?` ausente vale `None` e `ctx` ausente vale `Load`, como
//! no 3.13.

use num_bigint::Sign;

use crate::ast::*;
use crate::object::{repr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

const STMTS: &[&str] = &[
    "FunctionDef", "AsyncFunctionDef", "ClassDef", "Return", "Delete", "Assign", "TypeAlias", "AugAssign", "AnnAssign", "For",
    "AsyncFor", "While", "If", "With", "AsyncWith", "Match", "Raise", "Try", "TryStar", "Assert", "Import", "ImportFrom",
    "Global", "Nonlocal", "Expr", "Pass", "Break", "Continue",
];
const EXPRS: &[&str] = &[
    "BoolOp", "NamedExpr", "BinOp", "UnaryOp", "Lambda", "IfExp", "Dict", "Set", "ListComp", "SetComp", "DictComp",
    "GeneratorExp", "Await", "Yield", "YieldFrom", "Compare", "Call", "FormattedValue", "JoinedStr", "Constant", "Attribute",
    "Subscript", "Starred", "Name", "List", "Tuple", "Slice",
];
const PATTERNS: &[&str] =
    &["MatchValue", "MatchSingleton", "MatchSequence", "MatchMapping", "MatchClass", "MatchStar", "MatchAs", "MatchOr"];
const TYPE_PARAMS: &[&str] = &["TypeVar", "ParamSpec", "TypeVarTuple"];

const CONTEXTS: &[(&str, ExprContext)] =
    &[("Load", ExprContext::Load), ("Store", ExprContext::Store), ("Del", ExprContext::Del)];
const BOOL_OPS: &[(&str, BoolOp)] = &[("And", BoolOp::And), ("Or", BoolOp::Or)];
const OPERATORS: &[(&str, Operator)] = &[
    ("Add", Operator::Add),
    ("Sub", Operator::Sub),
    ("Mult", Operator::Mult),
    ("MatMult", Operator::MatMult),
    ("Div", Operator::Div),
    ("Mod", Operator::Mod),
    ("Pow", Operator::Pow),
    ("LShift", Operator::LShift),
    ("RShift", Operator::RShift),
    ("BitOr", Operator::BitOr),
    ("BitXor", Operator::BitXor),
    ("BitAnd", Operator::BitAnd),
    ("FloorDiv", Operator::FloorDiv),
];
const UNARY_OPS: &[(&str, UnaryOp)] =
    &[("Invert", UnaryOp::Invert), ("Not", UnaryOp::Not), ("UAdd", UnaryOp::UAdd), ("USub", UnaryOp::USub)];
const CMP_OPS: &[(&str, CmpOp)] = &[
    ("Eq", CmpOp::Eq),
    ("NotEq", CmpOp::NotEq),
    ("Lt", CmpOp::Lt),
    ("LtE", CmpOp::LtE),
    ("Gt", CmpOp::Gt),
    ("GtE", CmpOp::GtE),
    ("Is", CmpOp::Is),
    ("IsNot", CmpOp::IsNot),
    ("In", CmpOp::In),
    ("NotIn", CmpOp::NotIn),
];

/// Os nomes das classes da cadeia de herança de `v`, quando ele é uma instância.
fn mro_names(v: &Value) -> Option<Vec<String>> {
    let Value::Instance(i) = v else { return None };
    Some(i.class().mro().iter().map(|c| c.name.clone()).collect())
}

/// `v` é um nó `ast.AST` (a classe ou uma subclasse dela).
pub fn is_ast_node(v: &Value) -> bool {
    mro_names(v).is_some_and(|names| names.iter().any(|n| n == "AST"))
}

/// O nome do tipo de `v` como o `tp_name` do CPython o mostra nas mensagens.
fn type_label(v: &Value) -> String {
    match v {
        Value::Instance(i) => i.class().name.clone(),
        other => other.type_name().to_string(),
    }
}

/// O construtor de `names` mais próximo na herança de `v`: a subclasse de `ast.Name` ainda é um `Name`.
fn constructor(v: &Value, names: &[&'static str]) -> Option<&'static str> {
    let chain = mro_names(v)?;
    if !chain.iter().any(|n| n == "AST") {
        return None;
    }
    chain.iter().find_map(|c| names.iter().find(|n| **n == *c).copied())
}

fn to_int(v: &Value) -> PyResult<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Big(_) => Err(exc("OverflowError", "Python int too large to convert to C int")),
        other => Err(exc("ValueError", format!("invalid integer value: {}", repr(other)))),
    }
}

fn identifier(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        _ => Err(type_error("AST identifier must be of type str")),
    }
}

fn string(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        _ => Err(type_error("AST string must be of type str")),
    }
}

fn float_part(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(n) => *n as f64,
        _ => 0.0,
    }
}

/// Um nó e o nome do construtor dele, que as mensagens de campo ausente citam.
#[derive(Clone, Copy)]
struct Node<'v> {
    v: &'v Value,
    owner: &'static str,
}

struct Reader<'a> {
    vm: &'a mut Vm,
}

impl Reader<'_> {
    /// O atributo do nó; ausente (`AttributeError`) vira `None`, que é diferente de valer `None` no Python.
    fn field(&mut self, n: Node, name: &str) -> PyResult<Option<Value>> {
        match self.vm.load_attr(n.v, name) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind == "AttributeError" => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn required(&mut self, n: Node, name: &str) -> PyResult<Value> {
        self.field(n, name)?
            .ok_or_else(|| type_error(format!("required field \"{name}\" missing from {}", n.owner)))
    }

    /// Campo `?`: ausente ou `None` dá `None`.
    fn optional(&mut self, n: Node, name: &str) -> PyResult<Option<Value>> {
        Ok(self.field(n, name)?.filter(|v| !matches!(v, Value::None)))
    }

    /// Campo `*`: ausente é lista vazia; o que não é `list` é erro de tipo.
    fn list(&mut self, n: Node, name: &str) -> PyResult<Vec<Value>> {
        match self.field(n, name)? {
            None => Ok(Vec::new()),
            Some(Value::List(items)) => Ok(items.borrow().clone()),
            Some(other) => {
                Err(type_error(format!("{} field \"{name}\" must be a list, not a {}", n.owner, type_label(&other))))
            }
        }
    }

    /// Converte cada item de um campo `*`.
    fn each<T>(&mut self, n: Node, name: &str, mut item: impl FnMut(&mut Self, &Value) -> PyResult<T>) -> PyResult<Vec<T>> {
        let values = self.list(n, name)?;
        values.iter().map(|v| item(self, v)).collect()
    }

    /// `each` com a regra do `validate_nonempty_seq` de `ast.c`.
    fn nonempty<T>(&mut self, n: Node, name: &str, item: impl FnMut(&mut Self, &Value) -> PyResult<T>) -> PyResult<Vec<T>> {
        let out = self.each(n, name, item)?;
        if out.is_empty() {
            return Err(exc("ValueError", format!("empty {name} on {}", n.owner)));
        }
        Ok(out)
    }

    fn int(&mut self, n: Node, name: &str) -> PyResult<i64> {
        let v = self.required(n, name)?;
        to_int(&v)
    }

    fn opt_int(&mut self, n: Node, name: &str) -> PyResult<Option<i64>> {
        self.optional(n, name)?.as_ref().map(to_int).transpose()
    }

    fn index(&mut self, n: Node, name: &str) -> PyResult<usize> {
        Ok(self.int(n, name)?.max(0) as usize)
    }

    fn opt_index(&mut self, n: Node, name: &str) -> PyResult<Option<usize>> {
        Ok(self.opt_int(n, name)?.map(|x| x.max(0) as usize))
    }

    fn name(&mut self, n: Node, name: &str) -> PyResult<String> {
        identifier(&self.required(n, name)?)
    }

    fn opt_name(&mut self, n: Node, name: &str) -> PyResult<Option<String>> {
        self.optional(n, name)?.as_ref().map(identifier).transpose()
    }

    fn opt_string(&mut self, n: Node, name: &str) -> PyResult<Option<String>> {
        self.optional(n, name)?.as_ref().map(string).transpose()
    }

    /// O construtor de um tipo soma; o que não é nó dele é o `expected some sort of X` do CPython.
    fn constructor(&mut self, v: &Value, sum: &str, names: &[&'static str]) -> PyResult<&'static str> {
        constructor(v, names).ok_or_else(|| type_error(format!("expected some sort of {sum}, but got {}", repr(v))))
    }

    /// Um nó de enumeração do ASDL (`expr_context`, `operator`...).
    fn pick<T: Copy>(&mut self, v: &Value, sum: &str, table: &[(&'static str, T)]) -> PyResult<T> {
        let names: Vec<&'static str> = table.iter().map(|(name, _)| *name).collect();
        let chosen = self.constructor(v, sum, &names)?;
        Ok(table.iter().find(|(name, _)| *name == chosen).map(|(_, t)| *t).expect("o nome veio da tabela"))
    }

    fn pick_field<T: Copy>(&mut self, n: Node, name: &str, sum: &str, table: &[(&'static str, T)]) -> PyResult<T> {
        let v = self.required(n, name)?;
        self.pick(&v, sum, table)
    }

    fn context(&mut self, n: Node) -> PyResult<ExprContext> {
        match self.field(n, "ctx")? {
            None => Ok(ExprContext::Load),
            Some(v) => self.pick(&v, "expr_context", CONTEXTS),
        }
    }

    /// `lineno`, `col_offset`, `end_lineno` e `end_col_offset`; `kind` é o tipo soma que a mensagem cita.
    fn pos(&mut self, v: &Value, kind: &'static str) -> PyResult<Pos> {
        let n = Node { v, owner: kind };
        Ok(Pos {
            lineno: self.index(n, "lineno")?,
            col_offset: self.index(n, "col_offset")?,
            end_lineno: self.opt_index(n, "end_lineno")?,
            end_col_offset: self.opt_index(n, "end_col_offset")?,
        })
    }

    /// A posição de `pattern` e `type_param`, onde o fim é obrigatório.
    fn full_pos(&mut self, v: &Value, kind: &'static str) -> PyResult<FullPos> {
        let n = Node { v, owner: kind };
        Ok(FullPos {
            lineno: self.index(n, "lineno")?,
            col_offset: self.index(n, "col_offset")?,
            end_lineno: self.index(n, "end_lineno")?,
            end_col_offset: self.index(n, "end_col_offset")?,
        })
    }

    // ----- expressões

    fn expr_field(&mut self, n: Node, name: &str) -> PyResult<Expr> {
        let v = self.required(n, name)?;
        self.expr(&v)
    }

    fn boxed(&mut self, n: Node, name: &str) -> PyResult<Box<Expr>> {
        Ok(Box::new(self.expr_field(n, name)?))
    }

    fn opt_boxed(&mut self, n: Node, name: &str) -> PyResult<Option<Box<Expr>>> {
        self.optional(n, name)?.map(|v| self.expr(&v).map(Box::new)).transpose()
    }

    fn exprs(&mut self, n: Node, name: &str) -> PyResult<Vec<Expr>> {
        self.each(n, name, Self::expr)
    }

    /// Item de lista que admite `None` (`Dict.keys`, `arguments.kw_defaults`).
    fn maybe_expr(&mut self, v: &Value) -> PyResult<Option<Expr>> {
        if matches!(v, Value::None) {
            return Ok(None);
        }
        self.expr(v).map(Some)
    }

    fn generators(&mut self, n: Node) -> PyResult<Vec<Comprehension>> {
        let out = self.each(n, "generators", Self::comprehension)?;
        if out.is_empty() {
            return Err(exc("ValueError", "comprehension with no generators"));
        }
        Ok(out)
    }

    fn comprehension(&mut self, v: &Value) -> PyResult<Comprehension> {
        let n = Node { v, owner: "comprehension" };
        Ok(Comprehension {
            target: self.expr_field(n, "target")?,
            iter: self.expr_field(n, "iter")?,
            ifs: self.exprs(n, "ifs")?,
            is_async: self.int(n, "is_async")?,
        })
    }

    fn keyword(&mut self, v: &Value) -> PyResult<Keyword> {
        let pos = self.pos(v, "keyword")?;
        let n = Node { v, owner: "keyword" };
        Ok(Keyword { arg: self.opt_name(n, "arg")?, value: self.expr_field(n, "value")?, pos })
    }

    fn arg(&mut self, v: &Value) -> PyResult<Arg> {
        let pos = self.pos(v, "arg")?;
        let n = Node { v, owner: "arg" };
        Ok(Arg {
            arg: self.name(n, "arg")?,
            annotation: self.opt_boxed(n, "annotation")?,
            type_comment: self.opt_string(n, "type_comment")?,
            pos,
        })
    }

    fn opt_arg(&mut self, n: Node, name: &str) -> PyResult<Option<Box<Arg>>> {
        self.optional(n, name)?.map(|v| self.arg(&v).map(Box::new)).transpose()
    }

    fn arguments(&mut self, n: Node, name: &str) -> PyResult<Box<Arguments>> {
        let v = self.required(n, name)?;
        let n = Node { v: &v, owner: "arguments" };
        let args = Arguments {
            posonlyargs: self.each(n, "posonlyargs", Self::arg)?,
            args: self.each(n, "args", Self::arg)?,
            vararg: self.opt_arg(n, "vararg")?,
            kwonlyargs: self.each(n, "kwonlyargs", Self::arg)?,
            kw_defaults: self.each(n, "kw_defaults", Self::maybe_expr)?,
            kwarg: self.opt_arg(n, "kwarg")?,
            defaults: self.exprs(n, "defaults")?,
        };
        if args.kwonlyargs.len() != args.kw_defaults.len() {
            return Err(exc("ValueError", "length of kwonlyargs is not the same as kw_defaults on arguments"));
        }
        if args.defaults.len() > args.posonlyargs.len() + args.args.len() {
            return Err(exc("ValueError", "more positional defaults than args on arguments"));
        }
        Ok(Box::new(args))
    }

    /// Um valor Python de `Constant` que cabe na AST nativa: o sinal vai à parte, porque `Constant::Int`
    /// e `Constant::Float` guardam o módulo.
    fn scalar(&mut self, v: &Value) -> PyResult<(bool, Constant)> {
        Ok(match v {
            Value::None => (false, Constant::None),
            Value::Bool(b) => (false, Constant::Bool(*b)),
            Value::Int(i) => (*i < 0, Constant::Int(i.unsigned_abs().to_string())),
            Value::Big(b) => (b.sign() == Sign::Minus, Constant::Int(b.magnitude().to_string())),
            Value::Float(f) => (f.is_sign_negative() && !f.is_nan(), Constant::Float(f.abs())),
            Value::Str(s) => (false, Constant::Str(s.as_str().to_string())),
            Value::Bytes(b) => (false, Constant::Bytes(b.to_vec())),
            Value::Builtin("Ellipsis") => (false, Constant::Ellipsis),
            _ if v.type_name() == "complex" => {
                let re = self.vm.load_attr(v, "real")?;
                let im = self.vm.load_attr(v, "imag")?;
                (false, Constant::Complex(float_part(&re), float_part(&im)))
            }
            _ => return Err(type_error(format!("got an invalid type in Constant: {}", type_label(v)))),
        })
    }

    /// `Constant(value)` como expressão. O número negativo vira `-literal` e a tupla de constantes vira
    /// `Tuple`, que o compilador dobra de volta na mesma constante.
    fn constant(&mut self, v: &Value, kind: Option<String>, pos: Pos) -> PyResult<Expr> {
        if let Value::Tuple(items) = v {
            let elts = items.iter().map(|x| self.constant(x, None, pos)).collect::<PyResult<Vec<_>>>()?;
            return Ok(Expr { kind: ExprKind::Tuple { elts, ctx: ExprContext::Load }, pos });
        }
        let (negative, value) = self.scalar(v)?;
        let literal = Expr { kind: ExprKind::Constant { value, kind }, pos };
        if !negative {
            return Ok(literal);
        }
        Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::USub, operand: Box::new(literal) }, pos })
    }

    fn expr(&mut self, v: &Value) -> PyResult<Expr> {
        let name = self.constructor(v, "expr", EXPRS)?;
        let pos = self.pos(v, "expr")?;
        let n = Node { v, owner: name };
        use ExprKind as K;
        let kind = match name {
            "BoolOp" => {
                let op = self.pick_field(n, "op", "boolop", BOOL_OPS)?;
                let values = self.exprs(n, "values")?;
                if values.len() < 2 {
                    return Err(exc("ValueError", "BoolOp with less than 2 values"));
                }
                K::BoolOp { op, values }
            }
            "NamedExpr" => {
                let target = self.boxed(n, "target")?;
                if !matches!(target.kind, K::Name { .. }) {
                    return Err(type_error("NamedExpr target must be a Name"));
                }
                K::NamedExpr { target, value: self.boxed(n, "value")? }
            }
            "BinOp" => K::BinOp {
                left: self.boxed(n, "left")?,
                op: self.pick_field(n, "op", "operator", OPERATORS)?,
                right: self.boxed(n, "right")?,
            },
            "UnaryOp" => K::UnaryOp {
                op: self.pick_field(n, "op", "unaryop", UNARY_OPS)?,
                operand: self.boxed(n, "operand")?,
            },
            "Lambda" => K::Lambda { args: self.arguments(n, "args")?, body: self.boxed(n, "body")? },
            "IfExp" => K::IfExp {
                test: self.boxed(n, "test")?,
                body: self.boxed(n, "body")?,
                orelse: self.boxed(n, "orelse")?,
            },
            "Dict" => {
                let keys = self.each(n, "keys", Self::maybe_expr)?;
                let values = self.exprs(n, "values")?;
                if keys.len() != values.len() {
                    return Err(exc("ValueError", "Dict doesn't have the same number of keys as values"));
                }
                K::Dict { keys, values }
            }
            "Set" => K::Set { elts: self.exprs(n, "elts")? },
            "ListComp" => K::ListComp { elt: self.boxed(n, "elt")?, generators: self.generators(n)? },
            "SetComp" => K::SetComp { elt: self.boxed(n, "elt")?, generators: self.generators(n)? },
            "DictComp" => K::DictComp {
                key: self.boxed(n, "key")?,
                value: self.boxed(n, "value")?,
                generators: self.generators(n)?,
            },
            "GeneratorExp" => K::GeneratorExp { elt: self.boxed(n, "elt")?, generators: self.generators(n)? },
            "Await" => K::Await { value: self.boxed(n, "value")? },
            "Yield" => K::Yield { value: self.opt_boxed(n, "value")? },
            "YieldFrom" => K::YieldFrom { value: self.boxed(n, "value")? },
            "Compare" => {
                let left = self.boxed(n, "left")?;
                let mut ops = Vec::new();
                for op in self.list(n, "ops")? {
                    ops.push(self.pick(&op, "cmpop", CMP_OPS)?);
                }
                let comparators = self.exprs(n, "comparators")?;
                if comparators.is_empty() {
                    return Err(exc("ValueError", "Compare with no comparators"));
                }
                if comparators.len() != ops.len() {
                    return Err(exc("ValueError", "Compare has a different number of comparators and operands"));
                }
                K::Compare { left, ops, comparators }
            }
            "Call" => K::Call {
                func: self.boxed(n, "func")?,
                args: self.exprs(n, "args")?,
                keywords: self.each(n, "keywords", Self::keyword)?,
            },
            "FormattedValue" => K::FormattedValue {
                value: self.boxed(n, "value")?,
                conversion: self.int(n, "conversion")?,
                format_spec: self.opt_boxed(n, "format_spec")?,
            },
            // O CPython aceita qualquer expressão como valor: o `BUILD_STRING` exige `str` em tempo de execução.
            "JoinedStr" => K::JoinedStr { values: self.exprs(n, "values")? },
            "Constant" => {
                let value = self.required(n, "value")?;
                let kind = self.opt_string(n, "kind")?;
                return self.constant(&value, kind, pos);
            }
            "Attribute" => K::Attribute {
                value: self.boxed(n, "value")?,
                attr: self.name(n, "attr")?,
                ctx: self.context(n)?,
            },
            "Subscript" => K::Subscript {
                value: self.boxed(n, "value")?,
                slice: self.boxed(n, "slice")?,
                ctx: self.context(n)?,
            },
            "Starred" => K::Starred { value: self.boxed(n, "value")?, ctx: self.context(n)? },
            "Name" => {
                let id = self.name(n, "id")?;
                if matches!(id.as_str(), "None" | "True" | "False") {
                    return Err(exc("ValueError", format!("identifier field can't represent '{id}' constant")));
                }
                K::Name { id, ctx: self.context(n)? }
            }
            "List" => K::List { elts: self.exprs(n, "elts")?, ctx: self.context(n)? },
            "Tuple" => K::Tuple { elts: self.exprs(n, "elts")?, ctx: self.context(n)? },
            "Slice" => K::Slice {
                lower: self.opt_boxed(n, "lower")?,
                upper: self.opt_boxed(n, "upper")?,
                step: self.opt_boxed(n, "step")?,
            },
            _ => unreachable!("o construtor veio de EXPRS"),
        };
        Ok(Expr { kind, pos })
    }

    // ----- padrões e parâmetros de tipo

    fn patterns(&mut self, n: Node, name: &str) -> PyResult<Vec<Pattern>> {
        self.each(n, name, Self::pattern)
    }

    fn pattern(&mut self, v: &Value) -> PyResult<Pattern> {
        let name = self.constructor(v, "pattern", PATTERNS)?;
        let pos = self.full_pos(v, "pattern")?;
        let n = Node { v, owner: name };
        use PatternKind as P;
        let kind = match name {
            "MatchValue" => P::MatchValue { value: self.boxed(n, "value")? },
            "MatchSingleton" => {
                let value = self.required(n, "value")?;
                let (_, value) = self.scalar(&value)?;
                if !matches!(value, Constant::None | Constant::Bool(_)) {
                    return Err(exc("ValueError", "MatchSingleton can only contain True, False and None"));
                }
                P::MatchSingleton { value }
            }
            "MatchSequence" => P::MatchSequence { patterns: self.patterns(n, "patterns")? },
            "MatchMapping" => P::MatchMapping {
                keys: self.exprs(n, "keys")?,
                patterns: self.patterns(n, "patterns")?,
                rest: self.opt_name(n, "rest")?,
            },
            "MatchClass" => P::MatchClass {
                cls: self.boxed(n, "cls")?,
                patterns: self.patterns(n, "patterns")?,
                kwd_attrs: self.each(n, "kwd_attrs", |_, x| identifier(x))?,
                kwd_patterns: self.patterns(n, "kwd_patterns")?,
            },
            "MatchStar" => P::MatchStar { name: self.opt_name(n, "name")? },
            "MatchAs" => P::MatchAs {
                pattern: self.optional(n, "pattern")?.map(|p| self.pattern(&p).map(Box::new)).transpose()?,
                name: self.opt_name(n, "name")?,
            },
            "MatchOr" => P::MatchOr { patterns: self.patterns(n, "patterns")? },
            _ => unreachable!("o construtor veio de PATTERNS"),
        };
        Ok(Pattern { kind, pos })
    }

    fn match_case(&mut self, v: &Value) -> PyResult<MatchCase> {
        let n = Node { v, owner: "match_case" };
        let pattern = self.required(n, "pattern")?;
        Ok(MatchCase {
            pattern: self.pattern(&pattern)?,
            guard: self.opt_boxed(n, "guard")?,
            body: self.nonempty(n, "body", Self::stmt)?,
        })
    }

    fn type_param(&mut self, v: &Value) -> PyResult<TypeParam> {
        let name = self.constructor(v, "type_param", TYPE_PARAMS)?;
        let pos = self.full_pos(v, "type_param")?;
        let n = Node { v, owner: name };
        use TypeParamKind as T;
        let kind = match name {
            "TypeVar" => T::TypeVar {
                name: self.name(n, "name")?,
                bound: self.opt_boxed(n, "bound")?,
                default_value: self.opt_boxed(n, "default_value")?,
            },
            "ParamSpec" => {
                T::ParamSpec { name: self.name(n, "name")?, default_value: self.opt_boxed(n, "default_value")? }
            }
            "TypeVarTuple" => {
                T::TypeVarTuple { name: self.name(n, "name")?, default_value: self.opt_boxed(n, "default_value")? }
            }
            _ => unreachable!("o construtor veio de TYPE_PARAMS"),
        };
        Ok(TypeParam { kind, pos })
    }

    fn type_params(&mut self, n: Node) -> PyResult<Vec<TypeParam>> {
        self.each(n, "type_params", Self::type_param)
    }

    // ----- instruções

    fn stmts(&mut self, n: Node, name: &str) -> PyResult<Vec<Stmt>> {
        self.each(n, name, Self::stmt)
    }

    fn with_items(&mut self, n: Node) -> PyResult<Vec<WithItem>> {
        self.nonempty(n, "items", |r, v| {
            let item = Node { v, owner: "withitem" };
            Ok(WithItem { context_expr: r.expr_field(item, "context_expr")?, optional_vars: r.opt_boxed(item, "optional_vars")? })
        })
    }

    fn alias(&mut self, v: &Value) -> PyResult<Alias> {
        let pos = self.pos(v, "alias")?;
        let n = Node { v, owner: "alias" };
        Ok(Alias { name: self.name(n, "name")?, asname: self.opt_name(n, "asname")?, pos })
    }

    fn handler(&mut self, v: &Value) -> PyResult<ExceptHandler> {
        let name = self.constructor(v, "excepthandler", &["ExceptHandler"])?;
        let pos = self.pos(v, "excepthandler")?;
        let n = Node { v, owner: name };
        Ok(ExceptHandler {
            r#type: self.opt_boxed(n, "type")?,
            name: self.opt_name(n, "name")?,
            body: self.nonempty(n, "body", Self::stmt)?,
            pos,
        })
    }

    /// `Try` e `TryStar`: o corpo, os `except`, o `else` e o `finally` com as regras de `validate_stmt`.
    fn try_parts(&mut self, n: Node) -> PyResult<(Vec<Stmt>, Vec<ExceptHandler>, Vec<Stmt>, Vec<Stmt>)> {
        let body = self.nonempty(n, "body", Self::stmt)?;
        let handlers = self.each(n, "handlers", Self::handler)?;
        let orelse = self.stmts(n, "orelse")?;
        let finalbody = self.stmts(n, "finalbody")?;
        if handlers.is_empty() && finalbody.is_empty() {
            return Err(exc("ValueError", format!("{} has neither except handlers nor finalbody", n.owner)));
        }
        if handlers.is_empty() && !orelse.is_empty() {
            return Err(exc("ValueError", format!("{} has orelse but no except handlers", n.owner)));
        }
        Ok((body, handlers, orelse, finalbody))
    }

    fn stmt(&mut self, v: &Value) -> PyResult<Stmt> {
        let name = self.constructor(v, "stmt", STMTS)?;
        let pos = self.pos(v, "stmt")?;
        let n = Node { v, owner: name };
        use StmtKind as S;
        let kind = match name {
            "FunctionDef" | "AsyncFunctionDef" => {
                let def_name = self.name(n, "name")?;
                let args = self.arguments(n, "args")?;
                let body = self.nonempty(n, "body", Self::stmt)?;
                let decorator_list = self.exprs(n, "decorator_list")?;
                let returns = self.opt_boxed(n, "returns")?;
                let type_comment = self.opt_string(n, "type_comment")?;
                let type_params = self.type_params(n)?;
                if name == "FunctionDef" {
                    S::FunctionDef { name: def_name, args, body, decorator_list, returns, type_comment, type_params }
                } else {
                    S::AsyncFunctionDef { name: def_name, args, body, decorator_list, returns, type_comment, type_params }
                }
            }
            "ClassDef" => S::ClassDef {
                name: self.name(n, "name")?,
                bases: self.exprs(n, "bases")?,
                keywords: self.each(n, "keywords", Self::keyword)?,
                body: self.nonempty(n, "body", Self::stmt)?,
                decorator_list: self.exprs(n, "decorator_list")?,
                type_params: self.type_params(n)?,
            },
            "Return" => S::Return { value: self.opt_boxed(n, "value")? },
            "Delete" => S::Delete { targets: self.nonempty(n, "targets", Self::expr)? },
            "Assign" => S::Assign {
                targets: self.nonempty(n, "targets", Self::expr)?,
                value: self.boxed(n, "value")?,
                type_comment: self.opt_string(n, "type_comment")?,
            },
            "TypeAlias" => {
                let alias_name = self.boxed(n, "name")?;
                if !matches!(alias_name.kind, ExprKind::Name { .. }) {
                    return Err(type_error("TypeAlias with non-Name name"));
                }
                S::TypeAlias { name: alias_name, type_params: self.type_params(n)?, value: self.boxed(n, "value")? }
            }
            "AugAssign" => S::AugAssign {
                target: self.boxed(n, "target")?,
                op: self.pick_field(n, "op", "operator", OPERATORS)?,
                value: self.boxed(n, "value")?,
            },
            "AnnAssign" => S::AnnAssign {
                target: self.boxed(n, "target")?,
                annotation: self.boxed(n, "annotation")?,
                value: self.opt_boxed(n, "value")?,
                simple: self.int(n, "simple")?,
            },
            "For" | "AsyncFor" => {
                let target = self.boxed(n, "target")?;
                let iter = self.boxed(n, "iter")?;
                let body = self.nonempty(n, "body", Self::stmt)?;
                let orelse = self.stmts(n, "orelse")?;
                let type_comment = self.opt_string(n, "type_comment")?;
                if name == "For" {
                    S::For { target, iter, body, orelse, type_comment }
                } else {
                    S::AsyncFor { target, iter, body, orelse, type_comment }
                }
            }
            "While" => S::While {
                test: self.boxed(n, "test")?,
                body: self.nonempty(n, "body", Self::stmt)?,
                orelse: self.stmts(n, "orelse")?,
            },
            "If" => S::If {
                test: self.boxed(n, "test")?,
                body: self.nonempty(n, "body", Self::stmt)?,
                orelse: self.stmts(n, "orelse")?,
            },
            "With" | "AsyncWith" => {
                let items = self.with_items(n)?;
                let body = self.nonempty(n, "body", Self::stmt)?;
                let type_comment = self.opt_string(n, "type_comment")?;
                if name == "With" {
                    S::With { items, body, type_comment }
                } else {
                    S::AsyncWith { items, body, type_comment }
                }
            }
            "Match" => {
                S::Match { subject: self.boxed(n, "subject")?, cases: self.nonempty(n, "cases", Self::match_case)? }
            }
            "Raise" => {
                let exc_value = self.opt_boxed(n, "exc")?;
                let cause = self.opt_boxed(n, "cause")?;
                if exc_value.is_none() && cause.is_some() {
                    return Err(exc("ValueError", "Raise with cause but no exception"));
                }
                S::Raise { exc: exc_value, cause }
            }
            "Try" => {
                let (body, handlers, orelse, finalbody) = self.try_parts(n)?;
                S::Try { body, handlers, orelse, finalbody }
            }
            "TryStar" => {
                let (body, handlers, orelse, finalbody) = self.try_parts(n)?;
                S::TryStar { body, handlers, orelse, finalbody }
            }
            "Assert" => S::Assert { test: self.boxed(n, "test")?, msg: self.opt_boxed(n, "msg")? },
            "Import" => S::Import { names: self.nonempty(n, "names", Self::alias)? },
            "ImportFrom" => {
                let module = self.opt_name(n, "module")?;
                let names = self.nonempty(n, "names", Self::alias)?;
                let level = self.opt_int(n, "level")?;
                if level.is_some_and(|l| l < 0) {
                    return Err(exc("ValueError", "Negative ImportFrom level"));
                }
                S::ImportFrom { module, names, level }
            }
            "Global" => S::Global { names: self.nonempty(n, "names", |_, x| identifier(x))? },
            "Nonlocal" => S::Nonlocal { names: self.nonempty(n, "names", |_, x| identifier(x))? },
            "Expr" => S::Expr { value: self.boxed(n, "value")? },
            "Pass" => S::Pass,
            "Break" => S::Break,
            "Continue" => S::Continue,
            _ => unreachable!("o construtor veio de STMTS"),
        };
        Ok(Stmt { kind, pos })
    }

    fn type_ignore(&mut self, v: &Value) -> PyResult<TypeIgnore> {
        let name = self.constructor(v, "type_ignore", &["TypeIgnore"])?;
        let n = Node { v, owner: name };
        let tag = self.required(n, "tag")?;
        Ok(TypeIgnore { lineno: self.int(n, "lineno")?, tag: string(&tag)? })
    }
}

/// A raiz de uma árvore de `compile(tree, ..., mode)`: `Module` em `exec`, `Expression` em `eval` e
/// `Interactive` em `single`, cada uma já validada.
pub fn module_from_tree(vm: &mut Vm, tree: &Value, mode: &str) -> PyResult<Mod> {
    let want = match mode {
        "exec" => "Module",
        "eval" => "Expression",
        "single" => "Interactive",
        _ => return Err(exc("ValueError", "compile() mode must be 'exec', 'eval' or 'single'")),
    };
    if constructor(tree, &[want]).is_none() {
        return Err(type_error(format!("expected {want} node, got {}", type_label(tree))));
    }
    let mut r = Reader { vm };
    let n = Node { v: tree, owner: want };
    Ok(match want {
        "Module" => Mod::Module { body: r.stmts(n, "body")?, type_ignores: r.each(n, "type_ignores", Reader::type_ignore)? },
        "Interactive" => Mod::Interactive { body: r.stmts(n, "body")? },
        _ => Mod::Expression { body: r.boxed(n, "body")? },
    })
}
