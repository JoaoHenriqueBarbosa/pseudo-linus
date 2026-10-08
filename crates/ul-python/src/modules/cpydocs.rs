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

/// As assinaturas de texto (`__text_signature__`) das funções de módulo no CPython 3.13, geradas no
/// oráculo por `gen_module_function_sigs.py`: `módulo<TAB>função[<TAB>assinatura]`.
const MODULE_SIGNATURES: &str = include_str!("../../data/cpython-docs/module-function-sigs.tsv");

/// Tabela `(dono, nome) -> assinatura` de um `.tsv` gerado no oráculo (`dono<TAB>nome[<TAB>assinatura]`).
/// A linha sem a terceira coluna é um chamável sem `__text_signature__`; a linha sem TAB continua a
/// assinatura da anterior (o texto do CPython tem quebras de linha). `#` abre comentário.
pub(crate) fn parse_signature_table(src: &'static str) -> std::collections::HashMap<(&'static str, &'static str), Option<&'static str>> {
    let mut table = std::collections::HashMap::new();
    // A linha corrente: `(dono, nome, início e fim da assinatura no texto)`.
    let mut open: Option<(&'static str, &'static str, Option<(usize, usize)>)> = None;
    let mut flush = |open: &mut Option<(&'static str, &'static str, Option<(usize, usize)>)>| {
        if let Some((owner, name, span)) = open.take() {
            table.insert((owner, name), span.map(|(a, b)| &src[a..b]));
        }
    };
    let mut pos = 0;
    for line in src.split('\n') {
        let end = pos + line.len();
        if line.starts_with('#') || line.is_empty() {
            flush(&mut open);
        } else if let Some((owner, rest)) = line.split_once('\t') {
            flush(&mut open);
            let (name, sig) = rest.split_once('\t').map_or((rest, None), |(n, _)| (n, Some(pos + owner.len() + 1 + n.len() + 1)));
            open = Some((owner, name, sig.map(|a| (a, end))));
        } else if let Some((_, _, Some(span))) = open.as_mut() {
            span.1 = end;
        }
        pos = end + 1;
    }
    flush(&mut open);
    table
}

/// O `__text_signature__` da função de módulo `name` de `module` na tabela do CPython.
pub(crate) fn module_function_signature(module: &str, name: &str) -> Option<&'static str> {
    module_signatures().get(&(module, name)).copied().flatten()
}

fn module_signatures() -> &'static std::collections::HashMap<(&'static str, &'static str), Option<&'static str>> {
    static TABLE: std::sync::OnceLock<std::collections::HashMap<(&'static str, &'static str), Option<&'static str>>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| parse_signature_table(MODULE_SIGNATURES))
}

/// As assinaturas de texto dos tipos de módulos em C do CPython 3.13 (`itertools.chain`, `_io.BytesIO`...),
/// geradas no oráculo por `gen_module_class_sigs.py`: `módulo<TAB>tipo<TAB>assinatura`.
const CLASS_SIGNATURES: &str = include_str!("../../data/cpython-docs/module-class-sigs.tsv");

/// O `__text_signature__` do tipo `name` do módulo `module` na tabela do CPython.
pub(crate) fn class_signature(module: &str, name: &str) -> Option<&'static str> {
    static TABLE: std::sync::OnceLock<std::collections::HashMap<(&'static str, &'static str), Option<&'static str>>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| parse_signature_table(CLASS_SIGNATURES)).get(&(module, name)).copied().flatten()
}

thread_local! {
    /// Assinaturas de texto das funções nativas em Rust, pelo endereço da função (`register_native`).
    static NATIVE_SIGNATURES: std::cell::RefCell<std::collections::HashMap<usize, &'static str>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// Docstrings das funções nativas em Rust, pelo endereço da função (`register_native`).
    static NATIVE_DOCS: std::cell::RefCell<std::collections::HashMap<usize, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// O módulo dono de cada função nativa, pelo endereço da função: o `__module__` e o `__self__`.
    static NATIVE_OWNERS: std::cell::RefCell<std::collections::HashMap<usize, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Liga as funções nativas do módulo `module` às docstrings que o CPython dá a elas na tabela, e ao
/// módulo que as define.
pub fn register_native(module: &str, attrs: &std::collections::BTreeMap<String, crate::object::Value>) {
    let mut docs = Docs::new();
    runtime_docs(module, &mut docs);
    NATIVE_DOCS.with(|m| {
        NATIVE_OWNERS.with(|owners| {
            let (mut m, mut owners) = (m.borrow_mut(), owners.borrow_mut());
            for (name, value) in attrs {
                let crate::object::Value::NativeFn(f) = value else { continue };
                let key = f.f as *const () as usize;
                // Módulo de apoio não existe para o programa: o dono é o módulo público que expõe as funções dele
                // (o `_sys` é o `sys`), e os demais não aparecem como dono.
                let public = if module == "_sys" { Some("sys") } else { Some(module).filter(|m| !super::INTERNAL.contains(m)) };
                if let Some(owner) = public {
                    owners.entry(key).or_insert_with(|| owner.to_string());
                }
                if let Some(Some((doc, _))) = docs.get(name.as_str()) {
                    m.entry(key).or_insert_with(|| doc.clone());
                }
                if let Some(sig) = module_function_signature(module, name) {
                    NATIVE_SIGNATURES.with(|s| {
                        s.borrow_mut().entry(key).or_insert(sig);
                    });
                }
            }
        });
    });
}

/// Gera as consultas do que `register_native` guardou de uma função nativa, pelo endereço dela.
macro_rules! native_lookups {
    ($($(#[$meta:meta])* $name:ident($table:ident) -> $ty:ty;)*) => {
        $($(#[$meta])*
        pub fn $name(f: &crate::object::NativeFn) -> Option<$ty> {
            $table.with(|m| m.borrow().get(&(f.f as *const () as usize)).cloned())
        })*
    };
}

native_lookups! {
    /// O módulo que define a função nativa `f` (`math` para `math.sqrt`), se foi registrada por
    /// `register_native`.
    native_owner(NATIVE_OWNERS) -> String;
    /// O `__text_signature__` de uma função nativa registrada por `register_native` (`None` sem linha na
    /// tabela ou linha sem assinatura).
    native_signature(NATIVE_SIGNATURES) -> &'static str;
    /// A docstring de uma função nativa registrada por `register_native`.
    native_doc(NATIVE_DOCS) -> String;
}

/// A linha da tabela do módulo `builtins` para `name` (`len`, `str`, `str.upper`, `int.__add__`...):
/// `None` sem linha, `Some(None)` quando a docstring é `None` no CPython.
pub(crate) fn builtin_doc_entry(name: &str) -> Option<Option<String>> {
    thread_local! {
        static BUILTINS: Docs = {
            let mut docs = Docs::new();
            runtime_docs("builtins", &mut docs);
            docs
        };
    }
    BUILTINS.with(|d| d.get(name).map(|entry| entry.as_ref().map(|(doc, _)| doc.clone())))
}

/// A docstring da função ou do tipo embutido `name` (`len`, `abs`, `str`...), da tabela do módulo `builtins`.
pub fn builtin_doc(name: &str) -> Option<String> {
    builtin_doc_entry(name).flatten()
}

/// A docstring de módulo que a tabela dá para `name` (`None` se ela não tem ou se é `None` no CPython).
pub fn runtime_module_doc(name: &str) -> Option<String> {
    let mut docs = Docs::new();
    runtime_docs(name, &mut docs);
    docs.get("").cloned().flatten().map(|(d, _)| d)
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
            // `_build` de módulo embutido: monta a API pública num escopo fechado, e os nomes de
            // dentro dele são os do módulo (o `__qualname__` é reatribuído no fim).
            S::FunctionDef { name, body, .. } if prefix.is_empty() && name == "_build" => {
                rewrite_functions(body, "", docs);
            }
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
