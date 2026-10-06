//! Mutilação de nomes privados (`__x` dentro de `class A` vira `_A__x`), feita no AST do corpo de cada
//! classe antes de compilar, como o `_Py_Mangle` do CPython.
//!
//! Entram nomes lidos e escritos, atributos, parâmetros, nomes de `def` e `class`, nomes ligados por
//! `import`, `global`/`nonlocal`, o alvo do `except ... as` e os nomes capturados por `match`. Ficam de fora
//! os argumentos nomeados de chamada (`f(__k=1)` passa `__k`), strings e nomes dunder (`__x__`). O corpo
//! de uma classe aninhada não é tocado aqui: ele faz o próprio passe, com o nome dela, quando é compilado.

use crate::ast::{Arguments, Comprehension, Expr, ExprKind, Pattern, PatternKind, Stmt, StmtKind};

/// O nome da classe sem os `_` iniciais, que vira o prefixo; `None` quando não sobra nada (`class ___`).
pub fn prefix(class_name: &str) -> Option<&str> {
    let p = class_name.trim_start_matches('_');
    (!p.is_empty()).then_some(p)
}

/// `__x` vira `_Prefixo__x`; o resto passa igual.
pub fn mangle(prefix: &str, name: &str) -> Option<String> {
    (name.starts_with("__") && !name.ends_with("__") && !name.contains('.')).then(|| format!("_{prefix}{name}"))
}

/// Desfaz a mutilação de um nome de `def`/`class` (o `__name__` e o `__qualname__` mostram o original).
pub fn unmangle<'a>(prefix: Option<&str>, name: &'a str) -> &'a str {
    let Some(p) = prefix else { return name };
    match name.strip_prefix('_').and_then(|r| r.strip_prefix(p)) {
        Some(rest) if rest.starts_with("__") && !rest.ends_with("__") => rest,
        _ => name,
    }
}

/// O corpo da classe `class_name` com os nomes privados mutilados.
pub fn class_body(class_name: &str, body: &[Stmt]) -> Option<Vec<Stmt>> {
    let p = prefix(class_name)?;
    let mut body = body.to_vec();
    let m = Mangler { prefix: p };
    m.stmts(&mut body);
    Some(body)
}

struct Mangler<'a> {
    prefix: &'a str,
}

impl Mangler<'_> {
    fn name(&self, n: &mut String) {
        if let Some(m) = mangle(self.prefix, n) {
            *n = m;
        }
    }

    fn stmts(&self, body: &mut [Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn opt(&self, e: &mut Option<Box<Expr>>) {
        if let Some(e) = e {
            self.expr(e);
        }
    }

    fn exprs(&self, es: &mut [Expr]) {
        for e in es {
            self.expr(e);
        }
    }

    fn arguments(&self, a: &mut Arguments) {
        for arg in a.posonlyargs.iter_mut().chain(a.args.iter_mut()).chain(a.kwonlyargs.iter_mut()) {
            self.name(&mut arg.arg);
            self.opt(&mut arg.annotation);
        }
        for arg in a.vararg.iter_mut().chain(a.kwarg.iter_mut()) {
            self.name(&mut arg.arg);
            self.opt(&mut arg.annotation);
        }
        self.exprs(&mut a.defaults);
        for d in a.kw_defaults.iter_mut().flatten() {
            self.expr(d);
        }
    }

    fn stmt(&self, s: &mut Stmt) {
        match &mut s.kind {
            StmtKind::FunctionDef { name, args, body, decorator_list, returns, .. }
            | StmtKind::AsyncFunctionDef { name, args, body, decorator_list, returns, .. } => {
                self.name(name);
                self.arguments(args);
                self.stmts(body);
                self.exprs(decorator_list);
                self.opt(returns);
            }
            StmtKind::ClassDef { name, bases, keywords, decorator_list, .. } => {
                // O corpo fica para o passe da própria classe.
                self.name(name);
                self.exprs(bases);
                for k in keywords {
                    self.expr(&mut k.value);
                }
                self.exprs(decorator_list);
            }
            StmtKind::Return { value } => self.opt(value),
            StmtKind::Delete { targets } => self.exprs(targets),
            StmtKind::Assign { targets, value, .. } => {
                self.exprs(targets);
                self.expr(value);
            }
            StmtKind::TypeAlias { name, value, .. } => {
                self.expr(name);
                self.expr(value);
            }
            StmtKind::AugAssign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::AnnAssign { target, annotation, value, .. } => {
                self.expr(target);
                self.expr(annotation);
                self.opt(value);
            }
            StmtKind::For { target, iter, body, orelse, .. } | StmtKind::AsyncFor { target, iter, body, orelse, .. } => {
                self.expr(target);
                self.expr(iter);
                self.stmts(body);
                self.stmts(orelse);
            }
            StmtKind::While { test, body, orelse } | StmtKind::If { test, body, orelse } => {
                self.expr(test);
                self.stmts(body);
                self.stmts(orelse);
            }
            StmtKind::With { items, body, .. } | StmtKind::AsyncWith { items, body, .. } => {
                for it in items {
                    self.expr(&mut it.context_expr);
                    self.opt(&mut it.optional_vars);
                }
                self.stmts(body);
            }
            StmtKind::Match { subject, cases } => {
                self.expr(subject);
                for c in cases {
                    self.pattern(&mut c.pattern);
                    self.opt(&mut c.guard);
                    self.stmts(&mut c.body);
                }
            }
            StmtKind::Raise { exc, cause } => {
                self.opt(exc);
                self.opt(cause);
            }
            StmtKind::Try { body, handlers, orelse, finalbody } | StmtKind::TryStar { body, handlers, orelse, finalbody } => {
                self.stmts(body);
                for h in handlers {
                    self.opt(&mut h.r#type);
                    if let Some(n) = &mut h.name {
                        self.name(n);
                    }
                    self.stmts(&mut h.body);
                }
                self.stmts(orelse);
                self.stmts(finalbody);
            }
            StmtKind::Assert { test, msg } => {
                self.expr(test);
                self.opt(msg);
            }
            StmtKind::Import { names } | StmtKind::ImportFrom { names, .. } => {
                // O nome ligado é o `as` ou, sem ele, o próprio nome (o primeiro componente no `import a.b`).
                for a in names {
                    match &mut a.asname {
                        Some(asname) => self.name(asname),
                        None if !a.name.contains('.') => {
                            if let Some(m) = mangle(self.prefix, &a.name) {
                                a.asname = Some(m);
                            }
                        }
                        None => {}
                    }
                }
            }
            StmtKind::Global { names } | StmtKind::Nonlocal { names } => {
                for n in names {
                    self.name(n);
                }
            }
            StmtKind::Expr { value } => self.expr(value),
            StmtKind::Pass | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn comprehensions(&self, gens: &mut [Comprehension]) {
        for g in gens {
            self.expr(&mut g.target);
            self.expr(&mut g.iter);
            self.exprs(&mut g.ifs);
        }
    }

    fn pattern(&self, p: &mut Pattern) {
        match &mut p.kind {
            PatternKind::MatchValue { value } => self.expr(value),
            PatternKind::MatchSingleton { .. } => {}
            PatternKind::MatchSequence { patterns } | PatternKind::MatchOr { patterns } => {
                for q in patterns {
                    self.pattern(q);
                }
            }
            PatternKind::MatchMapping { keys, patterns, rest } => {
                self.exprs(keys);
                for q in patterns {
                    self.pattern(q);
                }
                if let Some(r) = rest {
                    self.name(r);
                }
            }
            PatternKind::MatchClass { cls, patterns, kwd_patterns, .. } => {
                self.expr(cls);
                for q in patterns.iter_mut().chain(kwd_patterns.iter_mut()) {
                    self.pattern(q);
                }
            }
            PatternKind::MatchStar { name } => {
                if let Some(n) = name {
                    self.name(n);
                }
            }
            PatternKind::MatchAs { pattern, name } => {
                if let Some(q) = pattern {
                    self.pattern(q);
                }
                if let Some(n) = name {
                    self.name(n);
                }
            }
        }
    }

    fn expr(&self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Name { id, .. } => self.name(id),
            ExprKind::Attribute { value, attr, .. } => {
                self.expr(value);
                self.name(attr);
            }
            ExprKind::BoolOp { values, .. } => self.exprs(values),
            ExprKind::NamedExpr { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            ExprKind::BinOp { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::UnaryOp { operand, .. } => self.expr(operand),
            ExprKind::Lambda { args, body } => {
                self.arguments(args);
                self.expr(body);
            }
            ExprKind::IfExp { test, body, orelse } => {
                self.expr(test);
                self.expr(body);
                self.expr(orelse);
            }
            ExprKind::Dict { keys, values } => {
                for k in keys.iter_mut().flatten() {
                    self.expr(k);
                }
                self.exprs(values);
            }
            ExprKind::Set { elts } | ExprKind::List { elts, .. } | ExprKind::Tuple { elts, .. } => self.exprs(elts),
            ExprKind::ListComp { elt, generators } | ExprKind::SetComp { elt, generators } | ExprKind::GeneratorExp { elt, generators } => {
                self.expr(elt);
                self.comprehensions(generators);
            }
            ExprKind::DictComp { key, value, generators } => {
                self.expr(key);
                self.expr(value);
                self.comprehensions(generators);
            }
            ExprKind::Await { value } | ExprKind::YieldFrom { value } | ExprKind::Starred { value, .. } => self.expr(value),
            ExprKind::Yield { value } => self.opt(value),
            ExprKind::Compare { left, comparators, .. } => {
                self.expr(left);
                self.exprs(comparators);
            }
            ExprKind::Call { func, args, keywords } => {
                self.expr(func);
                self.exprs(args);
                // O nome do argumento nomeado não é mutilado; só o valor.
                for k in keywords {
                    self.expr(&mut k.value);
                }
            }
            ExprKind::FormattedValue { value, format_spec, .. } => {
                self.expr(value);
                self.opt(format_spec);
            }
            ExprKind::JoinedStr { values } => self.exprs(values),
            ExprKind::Constant { .. } => {}
            ExprKind::Subscript { value, slice, .. } => {
                self.expr(value);
                self.expr(slice);
            }
            ExprKind::Slice { lower, upper, step } => {
                self.opt(lower);
                self.opt(upper);
                self.opt(step);
            }
        }
    }
}
