//! Módulo `fnmatch` do CPython 3.13 (semântica POSIX, sensível a maiúsculas): `fnmatch`,
//! `fnmatchcase`, `filter` e `translate`.
//!
//! A comparação não usa regex: o padrão vira uma lista de itens (`*`, `?`, literal, conjunto) e é
//! casado com retrocesso. `translate` devolve o mesmo texto de regex que o CPython 3.13 gera
//! (`(?s:...)\Z`). Detalhe conhecido: em conjuntos com vários hífens (`[a-c-e]`) a leitura dos
//! intervalos é a ingênua da esquerda para a direita, não a do `translate`.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{exactly, no_kwargs, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{iterate, PyResult, Vm};

#[derive(Debug)]
enum SetEl {
    One(char),
    Range(char, char),
}

#[derive(Debug)]
enum Item {
    Star,
    Any,
    Lit(char),
    Set { neg: bool, els: Vec<SetEl> },
}

/// Posição do `]` que fecha o conjunto aberto antes de `i` (a regra do `translate`).
fn find_close(p: &[char], i: usize) -> Option<usize> {
    let n = p.len();
    let mut j = i;
    if j < n && p[j] == '!' {
        j += 1;
    }
    if j < n && p[j] == ']' {
        j += 1;
    }
    while j < n && p[j] != ']' {
        j += 1;
    }
    if j >= n {
        None
    } else {
        Some(j)
    }
}

fn parse_set(stuff: &[char]) -> Item {
    let (neg, rest) = match stuff.first() {
        Some('!') => (true, &stuff[1..]),
        _ => (false, stuff),
    };
    let mut els = Vec::new();
    let mut idx = 0;
    while idx < rest.len() {
        if idx + 2 < rest.len() && rest[idx + 1] == '-' {
            els.push(SetEl::Range(rest[idx], rest[idx + 2]));
            idx += 3;
        } else {
            els.push(SetEl::One(rest[idx]));
            idx += 1;
        }
    }
    if neg && els.is_empty() {
        return Item::Any;
    }
    Item::Set { neg, els }
}

fn parse(pat: &str) -> Vec<Item> {
    let p: Vec<char> = pat.chars().collect();
    let mut items: Vec<Item> = Vec::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        i += 1;
        match c {
            '*' => {
                if !matches!(items.last(), Some(Item::Star)) {
                    items.push(Item::Star);
                }
            }
            '?' => items.push(Item::Any),
            '[' => match find_close(&p, i) {
                None => items.push(Item::Lit('[')),
                Some(j) => {
                    items.push(parse_set(&p[i..j]));
                    i = j + 1;
                }
            },
            c => items.push(Item::Lit(c)),
        }
    }
    items
}

fn item_matches(item: &Item, c: char) -> bool {
    match item {
        Item::Any => true,
        Item::Lit(l) => *l == c,
        Item::Set { neg, els } => {
            let hit = els.iter().any(|e| match e {
                SetEl::One(x) => *x == c,
                SetEl::Range(lo, hi) => *lo <= c && c <= *hi,
            });
            hit != *neg
        }
        Item::Star => false,
    }
}

fn glob_match(items: &[Item], s: &[char]) -> bool {
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while j < s.len() {
        if i < items.len() {
            if matches!(items[i], Item::Star) {
                star = Some((i, j));
                i += 1;
                continue;
            }
            if item_matches(&items[i], s[j]) {
                i += 1;
                j += 1;
                continue;
            }
        }
        match star {
            Some((si, sj)) => {
                i = si + 1;
                j = sj + 1;
                star = Some((si, sj + 1));
            }
            None => return false,
        }
    }
    while i < items.len() && matches!(items[i], Item::Star) {
        i += 1;
    }
    i == items.len()
}

/// `fnmatch.fnmatchcase(name, pat)`.
pub fn fnmatchcase_str(name: &str, pat: &str) -> bool {
    let items = parse(pat);
    let chars: Vec<char> = name.chars().collect();
    glob_match(&items, &chars)
}

fn re_escape(c: char) -> String {
    if "()[]{}?*+-|^$\\.&~# \t\n\r\u{b}\u{c}".contains(c) {
        format!("\\{c}")
    } else {
        c.to_string()
    }
}

enum Piece {
    Star,
    Text(String),
}

/// `fnmatch.translate(pat)`.
pub fn translate_str(pat: &str) -> String {
    let p: Vec<char> = pat.chars().collect();
    let n = p.len();
    let mut res: Vec<Piece> = Vec::new();
    let mut i = 0;
    while i < n {
        let c = p[i];
        i += 1;
        match c {
            '*' => {
                if !matches!(res.last(), Some(Piece::Star)) {
                    res.push(Piece::Star);
                }
            }
            '?' => res.push(Piece::Text(".".to_string())),
            '[' => match find_close(&p, i) {
                None => res.push(Piece::Text("\\[".to_string())),
                Some(j) => {
                    let body: Vec<char> = p[i..j].to_vec();
                    let mut stuff: String;
                    if !body.contains(&'-') {
                        stuff = body.iter().collect::<String>().replace('\\', "\\\\");
                    } else {
                        let mut chunks: Vec<String> = Vec::new();
                        let mut ii = i;
                        let mut k = if p[i] == '!' { i + 2 } else { i + 1 };
                        loop {
                            let found = (k..j).find(|&x| p[x] == '-');
                            let Some(kk) = found else { break };
                            chunks.push(p[ii..kk].iter().collect());
                            ii = kk + 1;
                            k = kk + 3;
                        }
                        let chunk: String = p[ii..j].iter().collect();
                        if !chunk.is_empty() {
                            chunks.push(chunk);
                        } else if let Some(last) = chunks.last_mut() {
                            last.push('-');
                        }
                        for k in (1..chunks.len()).rev() {
                            let a = chunks[k - 1].chars().last();
                            let b = chunks[k].chars().next();
                            if let (Some(a), Some(b)) = (a, b) {
                                if a > b {
                                    let mut s: Vec<char> = chunks[k - 1].chars().collect();
                                    s.pop();
                                    s.extend(chunks[k].chars().skip(1));
                                    chunks[k - 1] = s.into_iter().collect();
                                    chunks.remove(k);
                                }
                            }
                        }
                        stuff = chunks
                            .iter()
                            .map(|s| s.replace('\\', "\\\\").replace('-', "\\-"))
                            .collect::<Vec<_>>()
                            .join("-");
                    }
                    let mut escaped = String::new();
                    for ch in stuff.chars() {
                        if matches!(ch, '&' | '~' | '|') {
                            escaped.push('\\');
                        }
                        escaped.push(ch);
                    }
                    stuff = escaped;
                    i = j + 1;
                    if stuff.is_empty() {
                        res.push(Piece::Text("(?!)".to_string()));
                    } else if stuff == "!" {
                        res.push(Piece::Text(".".to_string()));
                    } else {
                        if let Some(rest) = stuff.strip_prefix('!') {
                            stuff = format!("^{rest}");
                        } else if stuff.starts_with('^') || stuff.starts_with('[') {
                            stuff = format!("\\{stuff}");
                        }
                        res.push(Piece::Text(format!("[{stuff}]")));
                    }
                }
            },
            c => res.push(Piece::Text(re_escape(c))),
        }
    }
    let total = res.len();
    let mut out = String::new();
    let mut idx = 0;
    while idx < total {
        match &res[idx] {
            Piece::Text(t) => {
                out.push_str(t);
                idx += 1;
            }
            Piece::Star => break,
        }
    }
    let mut group = 0;
    while idx < total {
        idx += 1; // consome o Star
        if idx == total {
            out.push_str(".*");
            break;
        }
        let mut fixed = String::new();
        while idx < total {
            match &res[idx] {
                Piece::Text(t) => {
                    fixed.push_str(t);
                    idx += 1;
                }
                Piece::Star => break,
            }
        }
        if idx == total {
            out.push_str(".*");
            out.push_str(&fixed);
        } else {
            out.push_str(&format!("(?=(?P<g{group}>.*?{fixed}))(?P=g{group})"));
            group += 1;
        }
    }
    format!("(?s:{out})\\Z")
}

fn fnmatch(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fnmatch", &kw)?;
    exactly("fnmatch", &args, 2)?;
    let name = want_str("fnmatch", &args[0])?;
    let pat = want_str("fnmatch", &args[1])?;
    Ok(Value::Bool(fnmatchcase_str(name, pat)))
}

fn fnmatchcase(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fnmatchcase", &kw)?;
    exactly("fnmatchcase", &args, 2)?;
    let name = want_str("fnmatchcase", &args[0])?;
    let pat = want_str("fnmatchcase", &args[1])?;
    Ok(Value::Bool(fnmatchcase_str(name, pat)))
}

fn filter(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("filter", &kw)?;
    exactly("filter", &args, 2)?;
    let pat = want_str("filter", &args[1])?;
    let items = parse(pat);
    let mut out = Vec::new();
    for name in iterate(&args[0])? {
        let s = want_str("filter", &name)?;
        let chars: Vec<char> = s.chars().collect();
        if glob_match(&items, &chars) {
            out.push(name.clone());
        }
    }
    Ok(Value::list(out))
}

fn translate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("translate", &kw)?;
    exactly("translate", &args, 1)?;
    Ok(Value::str(translate_str(want_str("translate", &args[0])?)))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("fnmatch")
        .func("fnmatch", fnmatch)
        .func("fnmatchcase", fnmatchcase)
        .func("filter", filter)
        .func("translate", translate)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{repr, to_str};

    #[test]
    fn matching() {
        assert!(fnmatchcase_str("foo.txt", "*.txt"));
        assert!(!fnmatchcase_str("foo.txt", "*.py"));
        assert!(fnmatchcase_str("abc", "a?c"));
        assert!(!fnmatchcase_str("ac", "a?c"));
        assert!(fnmatchcase_str("", "*"));
        assert!(fnmatchcase_str("abc", "a*b*c"));
        assert!(fnmatchcase_str("axxbyyc", "a*b*c"));
        assert!(!fnmatchcase_str("abcd", "a*b*c"));
        assert!(fnmatchcase_str("b", "[abc]"));
        assert!(!fnmatchcase_str("d", "[abc]"));
        assert!(fnmatchcase_str("d", "[!abc]"));
        assert!(!fnmatchcase_str("a", "[!abc]"));
        assert!(fnmatchcase_str("m", "[a-z]"));
        assert!(!fnmatchcase_str("M", "[a-z]"));
        assert!(fnmatchcase_str("[", "["));
        assert!(fnmatchcase_str("a[b", "a[b"));
        assert!(fnmatchcase_str("Foo.TXT", "*.TXT"));
        assert!(!fnmatchcase_str("foo.txt", "*.TXT"));
        assert!(fnmatchcase_str("a\nb", "a?b"));
    }

    #[test]
    fn translation() {
        assert_eq!(translate_str("*.txt"), "(?s:.*\\.txt)\\Z");
        assert_eq!(translate_str("a?b"), "(?s:a.b)\\Z");
        assert_eq!(translate_str("[a-c]x"), "(?s:[a-c]x)\\Z");
        assert_eq!(translate_str("[!abc]"), "(?s:[^abc])\\Z");
        assert_eq!(translate_str("*"), "(?s:.*)\\Z");
        assert_eq!(translate_str("["), "(?s:\\[)\\Z");
    }

    #[test]
    fn module_functions() {
        let mut vm = Vm::new();
        let names = Value::list(vec![Value::str("a.py"), Value::str("b.txt"), Value::str("c.py")]);
        let r = filter(&mut vm, vec![names, Value::str("*.py")], Vec::new()).unwrap();
        assert_eq!(repr(&r), "['a.py', 'c.py']");
        let r = fnmatch(&mut vm, vec![Value::str("x.c"), Value::str("*.c")], Vec::new()).unwrap();
        assert_eq!(repr(&r), "True");
        let r = translate(&mut vm, vec![Value::str("a*")], Vec::new()).unwrap();
        assert_eq!(to_str(&r), "(?s:a.*)\\Z");
    }
}
