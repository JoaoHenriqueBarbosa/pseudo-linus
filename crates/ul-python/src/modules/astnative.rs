//! Módulo nativo `_ast_native`: roda o parser (`ast.rs`, espelho do `Python.asdl`) e devolve a árvore
//! como tuplas `(nome, ((campo, valor), ...), posição)`, que `modules/py/_ast.py` transforma nas
//! instâncias dos nós. Lista vira `list`, ausente vira `None`, e a constante complexa sai como
//! `("__complex__", (re, im), None)`.

use std::rc::Rc;

use num_bigint::BigInt;

use crate::ast::*;
use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, PyResult, Vm};

type Fields = Vec<(&'static str, Value)>;

fn s(x: &str) -> Value {
    Value::str(x.to_string())
}

fn opt_s(x: &Option<String>) -> Value {
    x.as_deref().map_or(Value::None, s)
}

fn pos_tuple(p: &Pos) -> Value {
    let opt = |x: Option<usize>| x.map_or(Value::None, |n| Value::Int(n as i64));
    Value::tuple(vec![Value::Int(p.lineno as i64), Value::Int(p.col_offset as i64), opt(p.end_lineno), opt(p.end_col_offset)])
}

fn full_pos(p: &FullPos) -> Value {
    Value::tuple(vec![
        Value::Int(p.lineno as i64),
        Value::Int(p.col_offset as i64),
        Value::Int(p.end_lineno as i64),
        Value::Int(p.end_col_offset as i64),
    ])
}

fn node(name: &str, fields: Fields, pos: Option<Value>) -> Value {
    let pairs: Vec<Value> = fields.into_iter().map(|(k, v)| Value::tuple(vec![s(k), v])).collect();
    Value::tuple(vec![s(name), Value::tuple(pairs), pos.unwrap_or(Value::None)])
}

fn unit(name: &str) -> Value {
    node(name, Vec::new(), None)
}

fn exprs(v: &[Expr]) -> Value {
    Value::list(v.iter().map(expr).collect())
}

fn stmts(v: &[Stmt]) -> Value {
    Value::list(v.iter().map(stmt).collect())
}

fn opt_expr(e: &Option<Box<Expr>>) -> Value {
    e.as_ref().map_or(Value::None, |x| expr(x))
}

fn ctx(c: ExprContext) -> Value {
    unit(match c {
        ExprContext::Load => "Load",
        ExprContext::Store => "Store",
        ExprContext::Del => "Del",
    })
}

fn operator(o: Operator) -> Value {
    unit(match o {
        Operator::Add => "Add",
        Operator::Sub => "Sub",
        Operator::Mult => "Mult",
        Operator::MatMult => "MatMult",
        Operator::Div => "Div",
        Operator::Mod => "Mod",
        Operator::Pow => "Pow",
        Operator::LShift => "LShift",
        Operator::RShift => "RShift",
        Operator::BitOr => "BitOr",
        Operator::BitXor => "BitXor",
        Operator::BitAnd => "BitAnd",
        Operator::FloorDiv => "FloorDiv",
    })
}

fn unaryop(o: UnaryOp) -> Value {
    unit(match o {
        UnaryOp::Invert => "Invert",
        UnaryOp::Not => "Not",
        UnaryOp::UAdd => "UAdd",
        UnaryOp::USub => "USub",
    })
}

fn cmpop(o: CmpOp) -> Value {
    unit(match o {
        CmpOp::Eq => "Eq",
        CmpOp::NotEq => "NotEq",
        CmpOp::Lt => "Lt",
        CmpOp::LtE => "LtE",
        CmpOp::Gt => "Gt",
        CmpOp::GtE => "GtE",
        CmpOp::Is => "Is",
        CmpOp::IsNot => "IsNot",
        CmpOp::In => "In",
        CmpOp::NotIn => "NotIn",
    })
}

fn constant(c: &Constant) -> Value {
    match c {
        Constant::None => Value::None,
        Constant::Bool(b) => Value::Bool(*b),
        Constant::Int(digits) => match BigInt::parse_bytes(digits.as_bytes(), 10) {
            Some(b) => crate::bigint::norm(b),
            None => Value::Int(0),
        },
        Constant::Float(f) => Value::Float(*f),
        Constant::Complex(re, im) => node("__complex__", vec![("re", Value::Float(*re)), ("im", Value::Float(*im))], None),
        Constant::Str(t) => s(t),
        Constant::Bytes(b) => Value::bytes(b.clone()),
        Constant::Ellipsis => Value::Builtin("Ellipsis"),
    }
}

fn comprehension(c: &Comprehension) -> Value {
    node(
        "comprehension",
        vec![("target", expr(&c.target)), ("iter", expr(&c.iter)), ("ifs", exprs(&c.ifs)), ("is_async", Value::Int(c.is_async))],
        None,
    )
}

fn arg(a: &Arg) -> Value {
    node(
        "arg",
        vec![("arg", s(&a.arg)), ("annotation", opt_expr(&a.annotation)), ("type_comment", opt_s(&a.type_comment))],
        Some(pos_tuple(&a.pos)),
    )
}

fn arguments(a: &Arguments) -> Value {
    node(
        "arguments",
        vec![
            ("posonlyargs", Value::list(a.posonlyargs.iter().map(arg).collect())),
            ("args", Value::list(a.args.iter().map(arg).collect())),
            ("vararg", a.vararg.as_ref().map_or(Value::None, |x| arg(x))),
            ("kwonlyargs", Value::list(a.kwonlyargs.iter().map(arg).collect())),
            ("kw_defaults", Value::list(a.kw_defaults.iter().map(|d| d.as_ref().map_or(Value::None, expr)).collect())),
            ("kwarg", a.kwarg.as_ref().map_or(Value::None, |x| arg(x))),
            ("defaults", exprs(&a.defaults)),
        ],
        None,
    )
}

fn keyword(k: &Keyword) -> Value {
    node("keyword", vec![("arg", opt_s(&k.arg)), ("value", expr(&k.value))], Some(pos_tuple(&k.pos)))
}

fn alias(a: &Alias) -> Value {
    node("alias", vec![("name", s(&a.name)), ("asname", opt_s(&a.asname))], Some(pos_tuple(&a.pos)))
}

fn withitem(w: &WithItem) -> Value {
    node("withitem", vec![("context_expr", expr(&w.context_expr)), ("optional_vars", opt_expr(&w.optional_vars))], None)
}

fn handler(h: &ExceptHandler) -> Value {
    node(
        "ExceptHandler",
        vec![("type", opt_expr(&h.r#type)), ("name", opt_s(&h.name)), ("body", stmts(&h.body))],
        Some(pos_tuple(&h.pos)),
    )
}

fn type_param(t: &TypeParam) -> Value {
    let p = Some(full_pos(&t.pos));
    match &t.kind {
        TypeParamKind::TypeVar { name, bound, default_value } => node(
            "TypeVar",
            vec![("name", s(name)), ("bound", opt_expr(bound)), ("default_value", opt_expr(default_value))],
            p,
        ),
        TypeParamKind::ParamSpec { name, default_value } => {
            node("ParamSpec", vec![("name", s(name)), ("default_value", opt_expr(default_value))], p)
        }
        TypeParamKind::TypeVarTuple { name, default_value } => {
            node("TypeVarTuple", vec![("name", s(name)), ("default_value", opt_expr(default_value))], p)
        }
    }
}

fn type_params(v: &[TypeParam]) -> Value {
    Value::list(v.iter().map(type_param).collect())
}

fn patterns(v: &[Pattern]) -> Value {
    Value::list(v.iter().map(pattern).collect())
}

fn pattern(p: &Pattern) -> Value {
    let pos = Some(full_pos(&p.pos));
    match &p.kind {
        PatternKind::MatchValue { value } => node("MatchValue", vec![("value", expr(value))], pos),
        PatternKind::MatchSingleton { value } => node("MatchSingleton", vec![("value", constant(value))], pos),
        PatternKind::MatchSequence { patterns: ps } => node("MatchSequence", vec![("patterns", patterns(ps))], pos),
        PatternKind::MatchMapping { keys, patterns: ps, rest } => node(
            "MatchMapping",
            vec![("keys", exprs(keys)), ("patterns", patterns(ps)), ("rest", opt_s(rest))],
            pos,
        ),
        PatternKind::MatchClass { cls, patterns: ps, kwd_attrs, kwd_patterns } => node(
            "MatchClass",
            vec![
                ("cls", expr(cls)),
                ("patterns", patterns(ps)),
                ("kwd_attrs", Value::list(kwd_attrs.iter().map(|a| s(a)).collect())),
                ("kwd_patterns", patterns(kwd_patterns)),
            ],
            pos,
        ),
        PatternKind::MatchStar { name } => node("MatchStar", vec![("name", opt_s(name))], pos),
        PatternKind::MatchAs { pattern: inner, name } => node(
            "MatchAs",
            vec![("pattern", inner.as_ref().map_or(Value::None, |x| pattern(x))), ("name", opt_s(name))],
            pos,
        ),
        PatternKind::MatchOr { patterns: ps } => node("MatchOr", vec![("patterns", patterns(ps))], pos),
    }
}

fn match_case(m: &MatchCase) -> Value {
    node(
        "match_case",
        vec![("pattern", pattern(&m.pattern)), ("guard", opt_expr(&m.guard)), ("body", stmts(&m.body))],
        None,
    )
}

fn expr(e: &Expr) -> Value {
    let pos = Some(pos_tuple(&e.pos));
    match &e.kind {
        ExprKind::BoolOp { op, values } => node(
            "BoolOp",
            vec![("op", unit(if matches!(op, BoolOp::And) { "And" } else { "Or" })), ("values", exprs(values))],
            pos,
        ),
        ExprKind::NamedExpr { target, value } => node("NamedExpr", vec![("target", expr(target)), ("value", expr(value))], pos),
        ExprKind::BinOp { left, op, right } => {
            node("BinOp", vec![("left", expr(left)), ("op", operator(*op)), ("right", expr(right))], pos)
        }
        ExprKind::UnaryOp { op, operand } => node("UnaryOp", vec![("op", unaryop(*op)), ("operand", expr(operand))], pos),
        ExprKind::Lambda { args, body } => node("Lambda", vec![("args", arguments(args)), ("body", expr(body))], pos),
        ExprKind::IfExp { test, body, orelse } => {
            node("IfExp", vec![("test", expr(test)), ("body", expr(body)), ("orelse", expr(orelse))], pos)
        }
        ExprKind::Dict { keys, values } => node(
            "Dict",
            vec![
                ("keys", Value::list(keys.iter().map(|k| k.as_ref().map_or(Value::None, expr)).collect())),
                ("values", exprs(values)),
            ],
            pos,
        ),
        ExprKind::Set { elts } => node("Set", vec![("elts", exprs(elts))], pos),
        ExprKind::ListComp { elt, generators } => node(
            "ListComp",
            vec![("elt", expr(elt)), ("generators", Value::list(generators.iter().map(comprehension).collect()))],
            pos,
        ),
        ExprKind::SetComp { elt, generators } => node(
            "SetComp",
            vec![("elt", expr(elt)), ("generators", Value::list(generators.iter().map(comprehension).collect()))],
            pos,
        ),
        ExprKind::DictComp { key, value, generators } => node(
            "DictComp",
            vec![("key", expr(key)), ("value", expr(value)), ("generators", Value::list(generators.iter().map(comprehension).collect()))],
            pos,
        ),
        ExprKind::GeneratorExp { elt, generators } => node(
            "GeneratorExp",
            vec![("elt", expr(elt)), ("generators", Value::list(generators.iter().map(comprehension).collect()))],
            pos,
        ),
        ExprKind::Await { value } => node("Await", vec![("value", expr(value))], pos),
        ExprKind::Yield { value } => node("Yield", vec![("value", opt_expr(value))], pos),
        ExprKind::YieldFrom { value } => node("YieldFrom", vec![("value", expr(value))], pos),
        ExprKind::Compare { left, ops, comparators } => node(
            "Compare",
            vec![
                ("left", expr(left)),
                ("ops", Value::list(ops.iter().map(|o| cmpop(*o)).collect())),
                ("comparators", exprs(comparators)),
            ],
            pos,
        ),
        ExprKind::Call { func, args, keywords } => node(
            "Call",
            vec![("func", expr(func)), ("args", exprs(args)), ("keywords", Value::list(keywords.iter().map(keyword).collect()))],
            pos,
        ),
        ExprKind::FormattedValue { value, conversion, format_spec } => node(
            "FormattedValue",
            vec![("value", expr(value)), ("conversion", Value::Int(*conversion)), ("format_spec", opt_expr(format_spec))],
            pos,
        ),
        ExprKind::JoinedStr { values } => node("JoinedStr", vec![("values", exprs(values))], pos),
        ExprKind::Constant { value, kind } => node("Constant", vec![("value", constant(value)), ("kind", opt_s(kind))], pos),
        ExprKind::Attribute { value, attr, ctx: c } => {
            node("Attribute", vec![("value", expr(value)), ("attr", s(attr)), ("ctx", ctx(*c))], pos)
        }
        ExprKind::Subscript { value, slice, ctx: c } => {
            node("Subscript", vec![("value", expr(value)), ("slice", expr(slice)), ("ctx", ctx(*c))], pos)
        }
        ExprKind::Starred { value, ctx: c } => node("Starred", vec![("value", expr(value)), ("ctx", ctx(*c))], pos),
        ExprKind::Name { id, ctx: c } => node("Name", vec![("id", s(id)), ("ctx", ctx(*c))], pos),
        ExprKind::List { elts, ctx: c } => node("List", vec![("elts", exprs(elts)), ("ctx", ctx(*c))], pos),
        ExprKind::Tuple { elts, ctx: c } => node("Tuple", vec![("elts", exprs(elts)), ("ctx", ctx(*c))], pos),
        ExprKind::Slice { lower, upper, step } => {
            node("Slice", vec![("lower", opt_expr(lower)), ("upper", opt_expr(upper)), ("step", opt_expr(step))], pos)
        }
    }
}

fn stmt(st: &Stmt) -> Value {
    let pos = Some(pos_tuple(&st.pos));
    let def_fields = |name: &String,
                      args: &Arguments,
                      body: &[Stmt],
                      decorators: &[Expr],
                      returns: &Option<Box<Expr>>,
                      type_comment: &Option<String>,
                      tparams: &[TypeParam]|
     -> Fields {
        vec![
            ("name", s(name)),
            ("args", arguments(args)),
            ("body", stmts(body)),
            ("decorator_list", exprs(decorators)),
            ("returns", opt_expr(returns)),
            ("type_comment", opt_s(type_comment)),
            ("type_params", type_params(tparams)),
        ]
    };
    match &st.kind {
        StmtKind::FunctionDef { name, args, body, decorator_list, returns, type_comment, type_params } => {
            node("FunctionDef", def_fields(name, args, body, decorator_list, returns, type_comment, type_params), pos)
        }
        StmtKind::AsyncFunctionDef { name, args, body, decorator_list, returns, type_comment, type_params } => {
            node("AsyncFunctionDef", def_fields(name, args, body, decorator_list, returns, type_comment, type_params), pos)
        }
        StmtKind::ClassDef { name, bases, keywords, body, decorator_list, type_params: tp } => node(
            "ClassDef",
            vec![
                ("name", s(name)),
                ("bases", exprs(bases)),
                ("keywords", Value::list(keywords.iter().map(keyword).collect())),
                ("body", stmts(body)),
                ("decorator_list", exprs(decorator_list)),
                ("type_params", type_params(tp)),
            ],
            pos,
        ),
        StmtKind::Return { value } => node("Return", vec![("value", opt_expr(value))], pos),
        StmtKind::Delete { targets } => node("Delete", vec![("targets", exprs(targets))], pos),
        StmtKind::Assign { targets, value, type_comment } => node(
            "Assign",
            vec![("targets", exprs(targets)), ("value", expr(value)), ("type_comment", opt_s(type_comment))],
            pos,
        ),
        StmtKind::TypeAlias { name, type_params: tp, value } => {
            node("TypeAlias", vec![("name", expr(name)), ("type_params", type_params(tp)), ("value", expr(value))], pos)
        }
        StmtKind::AugAssign { target, op, value } => {
            node("AugAssign", vec![("target", expr(target)), ("op", operator(*op)), ("value", expr(value))], pos)
        }
        StmtKind::AnnAssign { target, annotation, value, simple } => node(
            "AnnAssign",
            vec![
                ("target", expr(target)),
                ("annotation", expr(annotation)),
                ("value", opt_expr(value)),
                ("simple", Value::Int(*simple)),
            ],
            pos,
        ),
        StmtKind::For { target, iter, body, orelse, type_comment } => node(
            "For",
            vec![
                ("target", expr(target)),
                ("iter", expr(iter)),
                ("body", stmts(body)),
                ("orelse", stmts(orelse)),
                ("type_comment", opt_s(type_comment)),
            ],
            pos,
        ),
        StmtKind::AsyncFor { target, iter, body, orelse, type_comment } => node(
            "AsyncFor",
            vec![
                ("target", expr(target)),
                ("iter", expr(iter)),
                ("body", stmts(body)),
                ("orelse", stmts(orelse)),
                ("type_comment", opt_s(type_comment)),
            ],
            pos,
        ),
        StmtKind::While { test, body, orelse } => {
            node("While", vec![("test", expr(test)), ("body", stmts(body)), ("orelse", stmts(orelse))], pos)
        }
        StmtKind::If { test, body, orelse } => {
            node("If", vec![("test", expr(test)), ("body", stmts(body)), ("orelse", stmts(orelse))], pos)
        }
        StmtKind::With { items, body, type_comment } => node(
            "With",
            vec![("items", Value::list(items.iter().map(withitem).collect())), ("body", stmts(body)), ("type_comment", opt_s(type_comment))],
            pos,
        ),
        StmtKind::AsyncWith { items, body, type_comment } => node(
            "AsyncWith",
            vec![("items", Value::list(items.iter().map(withitem).collect())), ("body", stmts(body)), ("type_comment", opt_s(type_comment))],
            pos,
        ),
        StmtKind::Match { subject, cases } => {
            node("Match", vec![("subject", expr(subject)), ("cases", Value::list(cases.iter().map(match_case).collect()))], pos)
        }
        StmtKind::Raise { exc, cause } => node("Raise", vec![("exc", opt_expr(exc)), ("cause", opt_expr(cause))], pos),
        StmtKind::Try { body, handlers, orelse, finalbody } => node(
            "Try",
            vec![
                ("body", stmts(body)),
                ("handlers", Value::list(handlers.iter().map(handler).collect())),
                ("orelse", stmts(orelse)),
                ("finalbody", stmts(finalbody)),
            ],
            pos,
        ),
        StmtKind::TryStar { body, handlers, orelse, finalbody } => node(
            "TryStar",
            vec![
                ("body", stmts(body)),
                ("handlers", Value::list(handlers.iter().map(handler).collect())),
                ("orelse", stmts(orelse)),
                ("finalbody", stmts(finalbody)),
            ],
            pos,
        ),
        StmtKind::Assert { test, msg } => node("Assert", vec![("test", expr(test)), ("msg", opt_expr(msg))], pos),
        StmtKind::Import { names } => node("Import", vec![("names", Value::list(names.iter().map(alias).collect()))], pos),
        StmtKind::ImportFrom { module, names, level } => node(
            "ImportFrom",
            vec![
                ("module", opt_s(module)),
                ("names", Value::list(names.iter().map(alias).collect())),
                ("level", level.map_or(Value::None, Value::Int)),
            ],
            pos,
        ),
        StmtKind::Global { names } => node("Global", vec![("names", Value::list(names.iter().map(|n| s(n)).collect()))], pos),
        StmtKind::Nonlocal { names } => node("Nonlocal", vec![("names", Value::list(names.iter().map(|n| s(n)).collect()))], pos),
        StmtKind::Expr { value } => node("Expr", vec![("value", expr(value))], pos),
        StmtKind::Pass => node("Pass", Vec::new(), pos),
        StmtKind::Break => node("Break", Vec::new(), pos),
        StmtKind::Continue => node("Continue", Vec::new(), pos),
    }
}

fn module(m: &Mod) -> Value {
    match m {
        Mod::Module { body, type_ignores } => node(
            "Module",
            vec![
                ("body", stmts(body)),
                (
                    "type_ignores",
                    Value::list(type_ignores
                        .iter()
                        .map(|t| node("TypeIgnore", vec![("lineno", Value::Int(t.lineno)), ("tag", s(&t.tag))], None))
                        .collect()),
                ),
            ],
            None,
        ),
        Mod::Interactive { body } => node("Interactive", vec![("body", stmts(body))], None),
        Mod::Expression { body } => node("Expression", vec![("body", expr(body))], None),
        Mod::FunctionType { argtypes, returns } => {
            node("FunctionType", vec![("argtypes", exprs(argtypes)), ("returns", expr(returns))], None)
        }
    }
}

/// `parse(source, filename, mode)`: `mode` é `exec`, `eval` ou `single`.
fn parse(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("parse", args, kw, &["source", "filename", "mode"], 1)?;
    let Some(Value::Str(src)) = a[0].as_ref() else {
        return Err(crate::vm::type_error("parse() source must be str"));
    };
    let fname = match a[1].as_ref() {
        Some(Value::Str(f)) => f.as_str().to_string(),
        _ => "<unknown>".to_string(),
    };
    let mode = match a[2].as_ref() {
        Some(Value::Str(m)) => m.as_str().to_string(),
        _ => "exec".to_string(),
    };
    let mut text = src.as_str().to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    match mode.as_str() {
        "eval" => {
            let e = crate::parser::parse_expression(src.as_str().trim()).map_err(|e| crate::vm::syntax_exc(e, &fname, src.as_str()))?;
            Ok(module(&Mod::Expression { body: Box::new(e) }))
        }
        "exec" => Ok(module(&crate::parser::parse_module(&text).map_err(|e| crate::vm::syntax_exc(e, &fname, src.as_str()))?)),
        "single" => match crate::parser::parse_module(&text).map_err(|e| crate::vm::syntax_exc(e, &fname, src.as_str()))? {
            Mod::Module { body, .. } => Ok(module(&Mod::Interactive { body })),
            other => Ok(module(&other)),
        },
        _ => Err(exc("ValueError", "compile() mode must be 'exec', 'eval' or 'single'")),
    }
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_ast_native").func("parse", parse).build()
}
