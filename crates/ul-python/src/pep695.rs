//! PEP 695: parâmetros de tipo (`def f[T]()`, `class C[T]:`) e a instrução `type X[T] = ...`, reescritos em
//! tempo de compilação como o CPython faz com o escopo de anotação: uma função sintética sem argumentos
//! cria os `TypeVar`, define o objeto de verdade dentro dela (os `T` ficam visíveis nas anotações, nas bases
//! e no corpo) e o devolve. O chamador a executa e guarda o resultado no nome original.

use crate::ast::{
    Alias, Arguments, Constant, Expr, ExprContext, ExprKind as E, Keyword, Pos, Stmt, StmtKind as S, TypeParam,
    TypeParamKind,
};

/// O que o compilador precisa para gerar o escopo sintético.
pub struct Generic {
    /// Nome da função sintética (`<generic parameters of f>`), só para tracebacks.
    pub scope_name: String,
    /// Nome que recebe o resultado.
    pub target: String,
    pub body: Vec<Stmt>,
}

pub fn no_args() -> Arguments {
    Arguments {
        posonlyargs: Vec::new(),
        args: Vec::new(),
        vararg: None,
        kwonlyargs: Vec::new(),
        kw_defaults: Vec::new(),
        kwarg: None,
        defaults: Vec::new(),
    }
}

fn ex(kind: E, pos: Pos) -> Expr {
    Expr { kind, pos }
}

fn name(id: &str, ctx: ExprContext, pos: Pos) -> Expr {
    ex(E::Name { id: id.to_string(), ctx }, pos)
}

fn text(s: &str, pos: Pos) -> Expr {
    ex(E::Constant { value: Constant::Str(s.to_string()), kind: None }, pos)
}

fn typing_attr(attr: &str, pos: Pos) -> Expr {
    ex(E::Attribute { value: Box::new(name("__typing", ExprContext::Load, pos)), attr: attr.to_string(), ctx: ExprContext::Load }, pos)
}

fn kw(arg: &str, value: Expr, pos: Pos) -> Keyword {
    Keyword { arg: Some(arg.to_string()), value, pos }
}

fn assign(target: Expr, value: Expr, pos: Pos) -> Stmt {
    Stmt { kind: S::Assign { targets: vec![target], value: Box::new(value), type_comment: None }, pos }
}

/// `T = __typing.TypeVar("T", bound=..., infer_variance=True)` e as variantes `ParamSpec`/`TypeVarTuple`.
fn make_param(p: &TypeParam, pos: Pos) -> (String, Stmt) {
    let (id, ctor, mut args, mut keywords) = match &p.kind {
        TypeParamKind::TypeVar { name: id, bound, default_value } => {
            let mut args = vec![text(id, pos)];
            let mut keywords = Vec::new();
            match bound.as_deref() {
                // `[T: (int, str)]`: tupla literal são restrições; qualquer outra expressão é o limite.
                Some(Expr { kind: E::Tuple { elts, .. }, .. }) => args.extend(elts.iter().cloned()),
                Some(b) => keywords.push(kw("bound", b.clone(), pos)),
                None => {}
            }
            if let Some(d) = default_value {
                keywords.push(kw("default", (**d).clone(), pos));
            }
            (id.clone(), "TypeVar", args, keywords)
        }
        TypeParamKind::ParamSpec { name: id, default_value } => {
            let keywords = default_value.iter().map(|d| kw("default", (**d).clone(), pos)).collect();
            (id.clone(), "ParamSpec", vec![text(id, pos)], keywords)
        }
        TypeParamKind::TypeVarTuple { name: id, default_value } => {
            let keywords = default_value.iter().map(|d| kw("default", (**d).clone(), pos)).collect();
            (id.clone(), "TypeVarTuple", vec![text(id, pos)], keywords)
        }
    };
    keywords.push(kw("infer_variance", ex(E::Constant { value: Constant::Bool(true), kind: None }, pos), pos));
    args.shrink_to_fit();
    let call = ex(E::Call { func: Box::new(typing_attr(ctor, pos)), args, keywords }, pos);
    (id.clone(), assign(name(&id, ExprContext::Store, pos), call, pos))
}

fn import_typing(pos: Pos) -> Stmt {
    Stmt { kind: S::Import { names: vec![Alias { name: "typing".to_string(), asname: Some("__typing".to_string()), pos }] }, pos }
}

fn params_tuple(ids: &[String], pos: Pos) -> Expr {
    ex(E::Tuple { elts: ids.iter().map(|i| name(i, ExprContext::Load, pos)).collect(), ctx: ExprContext::Load }, pos)
}

/// `None` quando a instrução não usa a sintaxe da PEP 695.
pub fn desugar(stmt: &Stmt) -> Option<Generic> {
    let pos = stmt.pos;
    match &stmt.kind {
        S::FunctionDef { name: id, args, body, decorator_list, returns, type_comment, type_params }
            if !type_params.is_empty() =>
        {
            let inner = S::FunctionDef {
                name: id.clone(),
                args: args.clone(),
                body: body.clone(),
                decorator_list: Vec::new(),
                returns: returns.clone(),
                type_comment: type_comment.clone(),
                type_params: Vec::new(),
            };
            Some(definition(id, inner, decorator_list, type_params, pos))
        }
        S::AsyncFunctionDef { name: id, args, body, decorator_list, returns, type_comment, type_params }
            if !type_params.is_empty() =>
        {
            let inner = S::AsyncFunctionDef {
                name: id.clone(),
                args: args.clone(),
                body: body.clone(),
                decorator_list: Vec::new(),
                returns: returns.clone(),
                type_comment: type_comment.clone(),
                type_params: Vec::new(),
            };
            Some(definition(id, inner, decorator_list, type_params, pos))
        }
        S::ClassDef { name: id, bases, keywords, body, decorator_list, type_params } if !type_params.is_empty() => {
            // `class C[T]` herda de `Generic[T]`, para o `C[int]` funcionar.
            let ids: Vec<String> = type_params.iter().map(|p| param_name(p).to_string()).collect();
            let subscript = ex(
                E::Subscript {
                    value: Box::new(typing_attr("Generic", pos)),
                    slice: Box::new(params_tuple(&ids, pos)),
                    ctx: ExprContext::Load,
                },
                pos,
            );
            let mut bases = bases.clone();
            bases.push(subscript);
            let inner = S::ClassDef {
                name: id.clone(),
                bases,
                keywords: keywords.clone(),
                body: body.clone(),
                decorator_list: Vec::new(),
                type_params: Vec::new(),
            };
            Some(definition(id, inner, decorator_list, type_params, pos))
        }
        S::TypeAlias { name: target, type_params, value } => {
            let E::Name { id: alias, .. } = &target.kind else { return None };
            let mut body = vec![import_typing(pos)];
            let mut ids = Vec::new();
            for p in type_params {
                let (id, stmt) = make_param(p, pos);
                ids.push(id);
                body.push(stmt);
            }
            let thunk = ex(E::Lambda { args: Box::new(no_args()), body: value.clone() }, pos);
            let call = ex(
                E::Call {
                    func: Box::new(typing_attr("TypeAliasType", pos)),
                    args: vec![text(alias, pos), thunk],
                    keywords: vec![kw("type_params", params_tuple(&ids, pos), pos)],
                },
                pos,
            );
            body.push(Stmt { kind: S::Return { value: Some(Box::new(call)) }, pos });
            Some(Generic { scope_name: format!("<generic parameters of {alias}>"), target: alias.clone(), body })
        }
        _ => None,
    }
}

fn param_name(p: &TypeParam) -> &str {
    match &p.kind {
        TypeParamKind::TypeVar { name, .. } | TypeParamKind::ParamSpec { name, .. } | TypeParamKind::TypeVarTuple { name, .. } => name,
    }
}

/// O corpo do escopo sintético de `def`/`class`: cria os parâmetros, define o objeto, grava
/// `__type_params__`, aplica os decoradores e devolve o resultado.
fn definition(id: &str, inner: S, decorators: &[Expr], type_params: &[TypeParam], pos: Pos) -> Generic {
    let mut body = vec![import_typing(pos)];
    let mut ids = Vec::new();
    for p in type_params {
        let (pid, stmt) = make_param(p, pos);
        ids.push(pid);
        body.push(stmt);
    }
    body.push(Stmt { kind: inner, pos });
    let attr = ex(E::Attribute { value: Box::new(name(id, ExprContext::Load, pos)), attr: "__type_params__".to_string(), ctx: ExprContext::Store }, pos);
    body.push(assign(attr, params_tuple(&ids, pos), pos));
    for d in decorators.iter().rev() {
        let call = ex(E::Call { func: Box::new(d.clone()), args: vec![name(id, ExprContext::Load, pos)], keywords: Vec::new() }, pos);
        body.push(assign(name(id, ExprContext::Store, pos), call, pos));
    }
    body.push(Stmt { kind: S::Return { value: Some(Box::new(name(id, ExprContext::Load, pos))) }, pos });
    Generic { scope_name: format!("<generic parameters of {id}>"), target: id.to_string(), body }
}
