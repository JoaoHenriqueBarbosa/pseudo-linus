//! PEP 695: o bytecode de `def f[T]`, `class C[T]` e `type X[T] = ...`.
//!
//! Espelha `compiler_function`, `compiler_class`, `compiler_type_alias`, `compiler_type_params` e
//! `compiler_type_param_bound_or_default` do `compile.c` do 3.13. O CPython cria uma função sintética
//! `<generic parameters of f>` que monta os `TypeVar`, `ParamSpec` e `TypeVarTuple` por intrínsecas, define o
//! objeto (anotações, bases e corpo enxergam os `T`) e o devolve; quem define só a chama.
//!
//! O interpretador executa a mesma estrutura (`pep695.rs`), mas com variáveis locais próprias dele. Por isso o
//! código do escopo e dos filhos diretos mostra, em `co_varnames`, `co_cellvars` e `co_freevars`, o desenho do
//! CPython ([`layout`]), e não o que o interpretador numera.

use super::*;
use crate::ast::{FullPos, TypeParam, TypeParamKind};
use crate::pep695::{CHILD, SCOPE_CLASS, SCOPE_FUNCTION, SCOPE_MASK};

pub(super) const LOAD_LOCALS: u16 = 25;
pub(super) const LOAD_FROM_DICT_OR_DEREF: u16 = 89;

const INTRINSIC_TYPEVAR: i64 = 7;
const INTRINSIC_PARAMSPEC: i64 = 8;
const INTRINSIC_TYPEVARTUPLE: i64 = 9;
const INTRINSIC_SUBSCRIPT_GENERIC: i64 = 10;
const INTRINSIC_TYPEALIAS: i64 = 11;
const INTRINSIC_TYPEVAR_WITH_BOUND: i64 = 2;
const INTRINSIC_TYPEVAR_WITH_CONSTRAINTS: i64 = 3;
const INTRINSIC_SET_FUNCTION_TYPE_PARAMS: i64 = 4;
const INTRINSIC_SET_TYPEPARAM_DEFAULT: i64 = 5;

/// Os nomes de variáveis de um código como o CPython os numera.
pub struct Layout {
    pub varnames: Vec<Rc<str>>,
    pub cellvars: Vec<Rc<str>>,
    pub freevars: Vec<Rc<str>>,
}

/// O nome que o escopo `<generic parameters of NOME>` define (`f`, `C` ou o alias).
fn scope_target(code: &Code) -> Option<&str> {
    code.name.strip_prefix("<generic parameters of ")?.strip_suffix('>')
}

/// `co_varnames`, `co_cellvars` e `co_freevars` de `code`. Fora da PEP 695 são os do interpretador. No escopo
/// `<generic parameters of ...>` saem o nome que o interpretador liga para o objeto definido e entram os que só o
/// CPython tem: `.defaults` (função), `.generic_base` e a célula `.type_params` (classe). O corpo de uma classe
/// genérica fecha `.type_params`.
pub fn layout(code: &Code) -> Layout {
    let mut l = Layout { varnames: code.varnames.clone(), cellvars: code.cellvars.clone(), freevars: code.freevars.clone() };
    if code.type_params_role & CHILD != 0 && code.is_class {
        l.freevars.insert(0, Rc::from(".type_params"));
        return l;
    }
    let kind = code.type_params_role & SCOPE_MASK;
    let Some(target) = scope_target(code).filter(|_| kind != 0 && code.is_function) else { return l };
    l.varnames.retain(|n| &**n != target);
    l.cellvars.retain(|n| &**n != target);
    match kind {
        SCOPE_FUNCTION => l.varnames.insert(0, Rc::from(".defaults")),
        SCOPE_CLASS => {
            l.varnames.push(Rc::from(".generic_base"));
            l.cellvars.insert(0, Rc::from(".type_params"));
        }
        _ => {}
    }
    l
}

/// Um `Code` só com o que o `Gen` lê, mas com os nomes do [`layout`] (o `Gen` classifica os nomes por eles).
fn view(code: &Code, is_class: bool) -> Code {
    let l = layout(code);
    Code {
        name: code.name.clone(),
        qualname: code.qualname.clone(),
        is_function: !is_class,
        is_class,
        varnames: l.varnames,
        cellvars: l.cellvars,
        freevars: l.freevars,
        functions: code.functions.clone(),
        uses_class_cell: code.uses_class_cell,
        first_line: code.first_line,
        ..Code::default()
    }
}

/// O corpo de uma classe genérica visto com a variável livre `.type_params`.
pub(super) fn class_view(code: &Code) -> Code {
    view(code, true)
}

fn pos_of(p: &FullPos) -> Pos {
    Pos { lineno: p.lineno, col_offset: p.col_offset, end_lineno: Some(p.end_lineno), end_col_offset: Some(p.end_col_offset) }
}

/// Valores padrão não cabem aqui: o CPython os passa como argumentos do escopo (`.defaults`, `.kwdefaults`).
fn no_defaults(args: &Arguments) -> Res<()> {
    if args.defaults.is_empty() && args.kw_defaults.iter().all(Option::is_none) {
        Ok(())
    } else {
        Err(Unsupported)
    }
}

/// O bytecode do escopo `<generic parameters of ...>` criado para `stmt`. `code` é o escopo do interpretador.
pub fn generic_scope(code: &Code, stmt: &Stmt, imports: &HashSet<String>, future: bool) -> Option<Emitted> {
    let scope = view(code, false);
    let first = code.first_line.max(1) as i32;
    let resume = Loc { line: first, end_line: first, col: 0, end_col: 0 };
    let no_globals = HashSet::new();
    let mut g = Gen::new(true, &scope, &no_globals, imports, resume);
    g.future = future;
    g.scope_body(stmt).ok()?;
    g.finish(first, 0)
}

impl Gen<'_> {
    /// A função do `k`-ésimo `Code::functions`, criada com a closure que ela pedir.
    fn next_function(&mut self, loc: Loc, flags: i64) -> Res<()> {
        let k = self.next_fn;
        let inner = self.code.functions.get(k).ok_or(Unsupported)?.clone();
        self.next_fn += 1;
        self.closure_code(k, &inner, loc, flags)
    }

    /// `compiler_type_param_bound_or_default` e o padrão: a função que calcula o valor, e a intrínseca que o aplica.
    fn param_default(&mut self, default: Option<&Expr>, loc: Loc) -> Res<()> {
        if default.is_some() {
            self.next_function(loc, 0)?;
            self.add(CALL_INTRINSIC_2, INTRINSIC_SET_TYPEPARAM_DEFAULT, loc);
        }
        Ok(())
    }

    /// `compiler_type_params`: cada parâmetro vira a chamada da intrínseca, é guardado no nome e o escopo termina
    /// com a tupla de todos.
    fn type_params(&mut self, params: &[TypeParam]) -> Res<()> {
        let mut first = None;
        for p in params {
            let loc = self.loc(&pos_of(&p.pos));
            first.get_or_insert(loc);
            let id = match &p.kind {
                TypeParamKind::TypeVar { name, bound, default_value } => {
                    self.check_plain(name)?;
                    self.load_cv(Cv::str(name.clone()), loc);
                    match bound.as_deref() {
                        Some(b) => {
                            self.next_function(loc, 0)?;
                            let which = if matches!(b.kind, E::Tuple { .. }) {
                                INTRINSIC_TYPEVAR_WITH_CONSTRAINTS
                            } else {
                                INTRINSIC_TYPEVAR_WITH_BOUND
                            };
                            self.add(CALL_INTRINSIC_2, which, loc);
                        }
                        None => self.add(CALL_INTRINSIC_1, INTRINSIC_TYPEVAR, loc),
                    }
                    self.param_default(default_value.as_deref(), loc)?;
                    name
                }
                TypeParamKind::ParamSpec { name, default_value } => {
                    self.check_plain(name)?;
                    self.load_cv(Cv::str(name.clone()), loc);
                    self.add(CALL_INTRINSIC_1, INTRINSIC_PARAMSPEC, loc);
                    self.param_default(default_value.as_deref(), loc)?;
                    name
                }
                TypeParamKind::TypeVarTuple { name, default_value } => {
                    self.check_plain(name)?;
                    self.load_cv(Cv::str(name.clone()), loc);
                    self.add(CALL_INTRINSIC_1, INTRINSIC_TYPEVARTUPLE, loc);
                    self.param_default(default_value.as_deref(), loc)?;
                    name
                }
            };
            self.add(COPY, 1, loc);
            self.store_name(id, loc)?;
        }
        self.add(BUILD_TUPLE, params.len() as i64, first.ok_or(Unsupported)?);
        Ok(())
    }

    /// O corpo do escopo, conforme a instrução que o pediu.
    fn scope_body(&mut self, s: &Stmt) -> Res<()> {
        let loc = self.loc(&s.pos);
        match &s.kind {
            S::FunctionDef { args, returns, type_params, .. } | S::AsyncFunctionDef { args, returns, type_params, .. } => {
                no_defaults(args)?;
                self.type_params(type_params)?;
                self.make_function(args, returns.as_deref(), loc)?;
                self.add(SWAP, 2, loc);
                self.add(CALL_INTRINSIC_2, INTRINSIC_SET_FUNCTION_TYPE_PARAMS, loc);
                self.add(RETURN_VALUE, 0, loc);
            }
            S::ClassDef { name, bases, keywords, type_params, .. } => {
                self.check_plain(name)?;
                if bases.iter().any(is_starred) || keywords.iter().any(|k| k.arg.is_none()) {
                    return Err(Unsupported);
                }
                if bases.len() + keywords.len() * 2 + 1 > STACK_USE_GUIDELINE {
                    return Err(Unsupported);
                }
                self.type_params(type_params)?;
                self.store_name(".type_params", loc)?;
                let k = self.next_fn;
                self.add(LOAD_BUILD_CLASS, 0, loc);
                self.add(PUSH_NULL, 0, loc);
                self.next_function(loc, 0)?;
                self.load_cv(Cv::str(name.clone()), loc);
                self.load_name(".type_params", loc, false)?;
                self.add(CALL_INTRINSIC_1, INTRINSIC_SUBSCRIPT_GENERIC, loc);
                self.store_name(".generic_base", loc)?;
                for b in bases {
                    self.expr(b)?;
                }
                self.load_name(".generic_base", loc, false)?;
                let positional = 2 + bases.len() as i64 + 1;
                if keywords.is_empty() {
                    self.add(CALL, positional, loc);
                } else {
                    let mut names = Vec::new();
                    for kw in keywords {
                        let n = kw.arg.clone().ok_or(Unsupported)?;
                        self.check_name(&n)?;
                        self.expr(&kw.value)?;
                        names.push(Cv::str(n));
                    }
                    self.load_cv(Cv::tuple(names), loc);
                    self.add(CALL_KW, positional + keywords.len() as i64, loc);
                }
                // As bases com `lambda` criam a função antes do corpo no interpretador, e depois no CPython.
                if self.next_fn != k + 1 {
                    return Err(Unsupported);
                }
                self.add(RETURN_VALUE, 0, loc);
            }
            S::TypeAlias { name, type_params, .. } => {
                let E::Name { id, .. } = &name.kind else { return Err(Unsupported) };
                self.load_cv(Cv::str(id.clone()), loc);
                self.type_params(type_params)?;
                self.next_function(loc, 0)?;
                self.add(BUILD_TUPLE, 3, loc);
                self.add(CALL_INTRINSIC_1, INTRINSIC_TYPEALIAS, loc);
                self.add(RETURN_VALUE, 0, loc);
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    /// `def`, `class` e `type` da PEP 695 vistos de fora do escopo; `None` se `s` não é um deles.
    pub(super) fn generic_stmt(&mut self, s: &Stmt, loc: Loc) -> Option<Res<()>> {
        match &s.kind {
            S::FunctionDef { name, args, decorator_list, type_params, .. }
            | S::AsyncFunctionDef { name, args, decorator_list, type_params, .. }
                if !type_params.is_empty() =>
            {
                Some(no_defaults(args).and_then(|()| self.generic_definition(name, decorator_list, false, loc)))
            }
            S::ClassDef { name, decorator_list, type_params, .. } if !type_params.is_empty() => {
                Some(self.generic_definition(name, decorator_list, true, loc))
            }
            S::TypeAlias { name, type_params, .. } => {
                let E::Name { id, .. } = &name.kind else { return Some(Err(Unsupported)) };
                if type_params.is_empty() {
                    Some(self.plain_alias(id, loc))
                } else {
                    Some(self.generic_definition(id, &[], false, loc))
                }
            }
            _ => None,
        }
    }

    /// Decoradores, a criação e a chamada do escopo, os decoradores aplicados e o nome ligado (`compiler_function`,
    /// `compiler_class` e `compiler_type_alias` com parâmetros de tipo). Corpo de classe e classe dentro de função com
    /// variáveis fechadas ficam de fora: pedem `__classdict__` e células que o interpretador não lista.
    fn generic_definition(&mut self, name: &str, decorators: &[Expr], class: bool, loc: Loc) -> Res<()> {
        let closes_outer = self.function && !(self.code.cellvars.is_empty() && self.code.freevars.is_empty());
        if self.code.is_class || (class && closes_outer) {
            return Err(Unsupported);
        }
        self.check_plain(name)?;
        for d in decorators {
            self.expr(d)?;
        }
        self.next_function(loc, 0)?;
        self.add(PUSH_NULL, 0, loc);
        self.add(CALL, 0, loc);
        for d in decorators.iter().rev() {
            let l = self.loc(&d.pos);
            self.add(CALL, 0, l);
        }
        self.store_name(name, loc)
    }

    /// `type X = valor` sem parâmetros de tipo: o nome, `None` no lugar dos parâmetros, a função do valor e a
    /// intrínseca que monta o `TypeAliasType`.
    fn plain_alias(&mut self, name: &str, loc: Loc) -> Res<()> {
        if self.code.is_class {
            return Err(Unsupported);
        }
        self.check_plain(name)?;
        self.load_cv(Cv::str(name.to_string()), loc);
        self.load_cv(Cv::none(), loc);
        self.next_function(loc, 0)?;
        self.add(BUILD_TUPLE, 3, loc);
        self.add(CALL_INTRINSIC_1, INTRINSIC_TYPEALIAS, loc);
        self.store_name(name, loc)
    }

    /// No corpo de uma classe genérica: `__type_params__` vem da célula `.type_params`.
    pub(super) fn type_params_store(&mut self, loc: Loc) -> Option<()> {
        self.add(LOAD_LOCALS, 0, loc);
        let i = self.localsplus.iter().position(|v| &**v == ".type_params")?;
        self.add(LOAD_FROM_DICT_OR_DEREF, i as i64, loc);
        self.store_name("__type_params__", loc).ok()
    }
}
