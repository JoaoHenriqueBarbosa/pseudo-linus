//! Docstrings dos módulos em Python embutido iguais às do CPython.
//!
//! O fonte embutido é nosso e as docstrings dele descrevem a implementação, não a API. Antes de
//! compilar, cada docstring de módulo, classe ou função do fonte embutido sai do AST; no lugar entra a
//! docstring do nó com o mesmo nome qualificado no fonte do CPython que a imagem tem em disco
//! (`/usr/lib/python3.13`). Nó que o CPython não tem, ou que lá não tem docstring, fica sem
//! `__doc__`, como uma função sem docstring.

use std::collections::HashMap;

use crate::ast::{Constant, Expr, ExprKind as E, Mod, Stmt, StmtKind as S};

/// Marca, no `kind` da constante, a docstring que já vem limpa da tabela: o compilador não refaz a
/// limpeza de indentação (as docstrings em C do CPython não passam por ela).
pub const CLEANED_DOC: &str = "cpython-runtime-doc";

/// A docstring de cada escopo pelo nome qualificado (`""` é o módulo), com a indicação de já estar
/// limpa (tabela) ou crua como o parser a deixa (fonte do disco).
type Docs = HashMap<String, Option<(String, bool)>>;

fn doc_of(body: &[Stmt]) -> Option<String> {
    match body.first().map(|s| &s.kind) {
        Some(S::Expr { value }) => match &value.kind {
            E::Constant { value: Constant::Str(s), .. } => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn doc_stmt((text, cleaned): &(String, bool), pos: crate::ast::Pos) -> Stmt {
    let kind = cleaned.then(|| CLEANED_DOC.to_string());
    let value = Box::new(Expr { kind: E::Constant { value: Constant::Str(text.clone()), kind }, pos });
    Stmt { kind: S::Expr { value }, pos }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// Os corpos que pertencem ao mesmo escopo de `stmt` (ramos de `if`, `try`, `with`, laços).
fn same_scope_bodies(stmt: &Stmt) -> Vec<&Vec<Stmt>> {
    match &stmt.kind {
        S::If { body, orelse, .. }
        | S::For { body, orelse, .. }
        | S::AsyncFor { body, orelse, .. }
        | S::While { body, orelse, .. } => vec![body, orelse],
        S::With { body, .. } | S::AsyncWith { body, .. } => vec![body],
        S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
            let mut out = vec![body, orelse, finalbody];
            out.extend(handlers.iter().map(|h| &h.body));
            out
        }
        _ => Vec::new(),
    }
}

fn same_scope_bodies_mut(stmt: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match &mut stmt.kind {
        S::If { body, orelse, .. }
        | S::For { body, orelse, .. }
        | S::AsyncFor { body, orelse, .. }
        | S::While { body, orelse, .. } => vec![body, orelse],
        S::With { body, .. } | S::AsyncWith { body, .. } => vec![body],
        S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
            let mut out = vec![body, orelse, finalbody];
            out.extend(handlers.iter_mut().map(|h| &mut h.body));
            out
        }
        _ => Vec::new(),
    }
}

fn collect(body: &[Stmt], prefix: &str, out: &mut Docs) {
    for stmt in body {
        match &stmt.kind {
            S::FunctionDef { name, body, .. } | S::AsyncFunctionDef { name, body, .. } => {
                let qual = join(prefix, name);
                // A primeira definição vence: é a que o CPython mostra quando há alternativas por plataforma.
                out.entry(qual.clone()).or_insert_with(|| doc_of(body).map(|d| (d, false)));
                collect(body, &format!("{qual}.<locals>"), out);
            }
            S::ClassDef { name, body, .. } => {
                let qual = join(prefix, name);
                out.entry(qual.clone()).or_insert_with(|| doc_of(body).map(|d| (d, false)));
                collect(body, &qual, out);
            }
            _ => {
                for inner in same_scope_bodies(stmt) {
                    collect(inner, prefix, out);
                }
            }
        }
    }
}

/// Alinha as docstrings de `module` (fonte embutido) às do CPython: as do fonte `cpython`, se o
/// disco tiver um, com a tabela do que o CPython expõe diferente em tempo de execução por cima.
pub fn align(module: &mut Mod, name: &str, cpython: Option<&Mod>) {
    let mut docs = Docs::new();
    if let Some(Mod::Module { body, .. }) = cpython {
        docs.insert(String::new(), doc_of(body).map(|d| (d, false)));
        collect(body, "", &mut docs);
    }
    runtime_docs(name, &mut docs);
    let Mod::Module { body, .. } = module else { return };
    let pos = body.first().map(|s| s.pos);
    if doc_of(body).is_some() {
        body.remove(0);
    }
    if let (Some(Some(doc)), Some(pos)) = (docs.get(""), pos) {
        body.insert(0, doc_stmt(doc, pos));
    }
    rewrite_functions(body, "", &docs);
}

/// Docstrings do CPython em tempo de execução que o `.py` do disco não dá (`data/cpython-docs`).
const RUNTIME: &str = include_str!("../../data/cpython-docs/runtime.tsv");

/// As docstrings da tabela para o módulo `name`, pelo nome qualificado (o módulo é `""`).
fn runtime_docs(name: &str, docs: &mut Docs) {
    for line in RUNTIME.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(module), Some(qual), Some(doc)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if module == name {
            docs.insert(qual.to_string(), json_string(doc).map(|d| (d, true)));
        }
    }
}

/// Uma string JSON (`"..."`, com escapes e pares substitutos) ou `null`.
fn json_string(text: &str) -> Option<String> {
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    let hex4 = |chars: &mut std::str::Chars| -> Option<u32> {
        let digits: String = chars.by_ref().take(4).collect();
        u32::from_str_radix(&digits, 16).ok()
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                let mut code = hex4(&mut chars)?;
                if (0xd800..0xdc00).contains(&code) {
                    let rest = chars.as_str();
                    if let Some(low) = rest.strip_prefix("\\u") {
                        let low = u32::from_str_radix(low.get(..4)?, 16).ok()?;
                        code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                        chars.nth(5);
                    }
                }
                out.push(char::from_u32(code)?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// Funções: os escopos internos delas usam `<locals>` no nome qualificado.
fn rewrite_functions(body: &mut [Stmt], prefix: &str, docs: &Docs) {
    for stmt in body.iter_mut() {
        match &mut stmt.kind {
            S::FunctionDef { name, body, .. } | S::AsyncFunctionDef { name, body, .. } => {
                let qual = join(prefix, name);
                replace_doc(body, &qual, docs);
                rewrite_functions(body, &format!("{qual}.<locals>"), docs);
            }
            S::ClassDef { name, body, .. } => {
                let qual = join(prefix, name);
                replace_doc(body, &qual, docs);
                rewrite_functions(body, &qual, docs);
            }
            _ => {
                for inner in same_scope_bodies_mut(stmt) {
                    rewrite_functions(inner, prefix, docs);
                }
            }
        }
    }
}

fn replace_doc(body: &mut Vec<Stmt>, qual: &str, docs: &Docs) {
    let pos = body.first().map(|s| s.pos);
    if doc_of(body).is_some() {
        body.remove(0);
    }
    if let (Some(Some(doc)), Some(pos)) = (docs.get(qual), pos) {
        body.insert(0, doc_stmt(doc, pos));
    }
    if body.is_empty() {
        if let Some(pos) = pos {
            body.push(Stmt { kind: S::Pass, pos });
        }
    }
}
