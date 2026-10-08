//! PEP 695: parâmetros de tipo (`def f[T]()`, `class C[T]:`) e a instrução `type X[T] = ...`, reescritos em
//! tempo de compilação como o CPython faz com o escopo de anotação: uma função sintética sem argumentos
//! cria os `TypeVar`, define o objeto de verdade dentro dela (os `T` ficam visíveis nas anotações, nas bases
//! e no corpo) e o devolve. O chamador a executa e guarda o resultado no nome original.
//!
//! Limite, restrição e padrão de cada parâmetro viram uma função sem argumentos (`typing._Lazy`), calculada na
//! primeira leitura de `__bound__`, `__constraints__` ou `__default__`, como no CPython. O escopo não liga nenhum
//! nome auxiliar (o módulo `typing` vem de `__import__`), para o `co_varnames` dele ser o do CPython.

use crate::ast::{
    Arguments, Constant, Expr, ExprContext, ExprKind as E, Keyword, Pos, Stmt, StmtKind as S, TypeParam, TypeParamKind,
};

/// `Code::type_params_role`: o código é o escopo `<generic parameters of f>` de uma função.
pub const SCOPE_FUNCTION: u8 = 1;
/// O escopo `<generic parameters of C>` de uma classe.
pub const SCOPE_CLASS: u8 = 2;
/// O escopo `<generic parameters of X>` de um `type X[T] = ...`.
pub const SCOPE_ALIAS: u8 = 4;
/// O código foi criado direto dentro de um escopo de parâmetros de tipo (a função, o corpo da classe, o valor
/// do alias, o limite de um `TypeVar`).
pub const CHILD: u8 = 8;
/// O `co_flags` leva `CO_NESTED` (a tabela de símbolos marca o bloco como aninhado).
pub const NESTED: u8 = 16;
/// Os bits que dizem de que escopo o código é.
pub const SCOPE_MASK: u8 = SCOPE_FUNCTION | SCOPE_CLASS | SCOPE_ALIAS;

/// O que o compilador precisa para gerar o escopo sintético.
pub struct Generic {
    /// Nome da função sintética (`<generic parameters of f>`).
    pub scope_name: String,
    /// Nome que recebe o resultado.
    pub target: String,
    pub body: Vec<Stmt>,
    /// `SCOPE_FUNCTION`, `SCOPE_CLASS` ou `SCOPE_ALIAS`.
    pub role: u8,
    /// Nomes das funções `lambda` que o escopo cria (limites, restrições, padrões e o valor do alias), na ordem de
    /// criação: o CPython dá a cada uma o nome do parâmetro de tipo ou do alias. O `bool` marca o valor do alias,
    /// cujo `RETURN_VALUE` leva a localização da instrução `type` inteira (nos limites e padrões é a da expressão).
    pub lambda_names: Vec<(String, bool)>,
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

/// `__import__("typing").attr`: o módulo sem ligar nome algum no escopo.
fn typing_attr(attr: &str, pos: Pos) -> Expr {
    let import = ex(
        E::Call { func: Box::new(name("__import__", ExprContext::Load, pos)), args: vec![text("typing", pos)], keywords: Vec::new() },
        pos,
    );
    ex(E::Attribute { value: Box::new(import), attr: attr.to_string(), ctx: ExprContext::Load }, pos)
}

fn kw(arg: &str, value: Expr, pos: Pos) -> Keyword {
    Keyword { arg: Some(arg.to_string()), value, pos }
}

fn assign(target: Expr, value: Expr, pos: Pos) -> Stmt {
    Stmt { kind: S::Assign { targets: vec![target], value: Box::new(value), type_comment: None }, pos }
}

/// `typing._Lazy(lambda: body)`; o `*x` de um padrão de `TypeVarTuple` vira o único item de `(*x,)`.
fn lazy(body: &Expr, pos: Pos) -> Expr {
    let body = match &body.kind {
        E::Starred { .. } => {
            let tuple = ex(E::Tuple { elts: vec![body.clone()], ctx: ExprContext::Load }, pos);
            let zero = ex(E::Constant { value: Constant::Int("0".to_string()), kind: None }, pos);
            ex(E::Subscript { value: Box::new(tuple), slice: Box::new(zero), ctx: ExprContext::Load }, pos)
        }
        _ => body.clone(),
    };
    let thunk = ex(E::Lambda { args: Box::new(no_args()), body: Box::new(body) }, pos);
    ex(E::Call { func: Box::new(typing_attr("_Lazy", pos)), args: vec![thunk], keywords: Vec::new() }, pos)
}

/// `T = typing.TypeVar("T", bound=_Lazy(...), infer_variance=True)` e as variantes `ParamSpec`/`TypeVarTuple`.
/// `lambdas` recebe o nome de cada função criada, na ordem em que o CPython as cria.
fn make_param(p: &TypeParam, pos: Pos, lambdas: &mut Vec<(String, bool)>) -> (String, Stmt) {
    let (id, ctor, mut args, mut keywords) = match &p.kind {
        TypeParamKind::TypeVar { name: id, bound, default_value } => {
            let mut args = vec![text(id, pos)];
            let mut keywords = Vec::new();
            if let Some(b) = bound.as_deref() {
                lambdas.push((id.clone(), false));
                // `[T: (int, str)]`: tupla literal são restrições; qualquer outra expressão é o limite.
                if matches!(b.kind, E::Tuple { .. }) {
                    args.push(lazy(b, pos));
                } else {
                    keywords.push(kw("bound", lazy(b, pos), pos));
                }
            }
            if let Some(d) = default_value {
                lambdas.push((id.clone(), false));
                keywords.push(kw("default", lazy(d, pos), pos));
            }
            (id.clone(), "TypeVar", args, keywords)
        }
        TypeParamKind::ParamSpec { name: id, default_value } => {
            let mut keywords = Vec::new();
            if let Some(d) = default_value {
                lambdas.push((id.clone(), false));
                keywords.push(kw("default", lazy(d, pos), pos));
            }
            (id.clone(), "ParamSpec", vec![text(id, pos)], keywords)
        }
        TypeParamKind::TypeVarTuple { name: id, default_value } => {
            let mut keywords = Vec::new();
            if let Some(d) = default_value {
                lambdas.push((id.clone(), false));
                keywords.push(kw("default", lazy(d, pos), pos));
            }
            (id.clone(), "TypeVarTuple", vec![text(id, pos)], keywords)
        }
    };
    keywords.push(kw("infer_variance", ex(E::Constant { value: Constant::Bool(true), kind: None }, pos), pos));
    args.shrink_to_fit();
    let call = ex(E::Call { func: Box::new(typing_attr(ctor, pos)), args, keywords }, pos);
    (id.clone(), assign(name(&id, ExprContext::Store, pos), call, pos))
}

fn params_tuple(ids: &[String], pos: Pos) -> Expr {
    ex(E::Tuple { elts: ids.iter().map(|i| name(i, ExprContext::Load, pos)).collect(), ctx: ExprContext::Load }, pos)
}

/// `typing.TypeAliasType("X", lambda: value[, type_params=(T,)])`, o valor ainda não calculado.
fn alias_call(alias: &str, value: &Expr, ids: Option<&[String]>, pos: Pos) -> Expr {
    let thunk = ex(E::Lambda { args: Box::new(no_args()), body: Box::new(value.clone()) }, pos);
    let keywords = ids.map(|ids| vec![kw("type_params", params_tuple(ids, pos), pos)]).unwrap_or_default();
    ex(E::Call { func: Box::new(typing_attr("TypeAliasType", pos)), args: vec![text(alias, pos), thunk], keywords }, pos)
}

/// `type X = valor` sem parâmetros de tipo: não há escopo, só a chamada que cria o alias. Devolve o nome, a
/// chamada e o nome da função do valor.
pub fn plain_alias(stmt: &Stmt) -> Option<(String, Expr, Vec<(String, bool)>)> {
    let S::TypeAlias { name: target, type_params, value } = &stmt.kind else { return None };
    let E::Name { id: alias, .. } = &target.kind else { return None };
    if !type_params.is_empty() {
        return None;
    }
    Some((alias.clone(), alias_call(alias, value, None, stmt.pos), vec![(alias.clone(), true)]))
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
            Some(definition(id, inner, decorator_list, type_params, SCOPE_FUNCTION, pos))
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
            Some(definition(id, inner, decorator_list, type_params, SCOPE_FUNCTION, pos))
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
            Some(definition(id, inner, decorator_list, type_params, SCOPE_CLASS, pos))
        }
        S::TypeAlias { name: target, type_params, value } if !type_params.is_empty() => {
            let E::Name { id: alias, .. } = &target.kind else { return None };
            let mut body = Vec::new();
            let mut ids = Vec::new();
            let mut lambdas = Vec::new();
            for p in type_params {
                let (id, stmt) = make_param(p, pos, &mut lambdas);
                ids.push(id);
                body.push(stmt);
            }
            lambdas.push((alias.clone(), true));
            let call = alias_call(alias, value, Some(&ids), pos);
            body.push(Stmt { kind: S::Return { value: Some(Box::new(call)) }, pos });
            Some(Generic {
                scope_name: format!("<generic parameters of {alias}>"),
                target: alias.clone(),
                body,
                role: SCOPE_ALIAS,
                lambda_names: lambdas,
            })
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
fn definition(id: &str, inner: S, decorators: &[Expr], type_params: &[TypeParam], role: u8, pos: Pos) -> Generic {
    let mut body = Vec::new();
    let mut ids = Vec::new();
    let mut lambdas = Vec::new();
    for p in type_params {
        let (pid, stmt) = make_param(p, pos, &mut lambdas);
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
    Generic { scope_name: format!("<generic parameters of {id}>"), target: id.to_string(), body, role, lambda_names: lambdas }
}
