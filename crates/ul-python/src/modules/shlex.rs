//! Módulo `shlex` do CPython 3.13: `split` (modo POSIX e não POSIX, com `comments`), `quote` e `join`.
//!
//! Ficam de fora: a classe `shlex` e `punctuation_chars`.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn is_quote(c: char) -> bool {
    c == '\'' || c == '"'
}

/// Pula o resto da linha (o `readline` do comentário), incluindo a quebra.
fn skip_line(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() {
        let c = chars[*pos];
        *pos += 1;
        if c == '\n' {
            break;
        }
    }
}

/// Um token da máquina de estados do `shlex.read_token` com `whitespace_split=True`.
/// `Ok(None)` é o fim da entrada.
fn read_token(chars: &[char], pos: &mut usize, comments: bool, posix: bool) -> Result<Option<String>, String> {
    // ' ' entre palavras, 'a' dentro de palavra, aspa ou barra invertida.
    let mut state = ' ';
    let mut token = String::new();
    let mut quoted = false;
    let mut escaped_state = ' ';
    loop {
        let next = chars.get(*pos).copied();
        if next.is_some() {
            *pos += 1;
        }
        match state {
            ' ' => match next {
                None => return Ok(None),
                Some(c) if is_space(c) => {
                    if !token.is_empty() || (posix && quoted) {
                        return Ok(Some(token));
                    }
                }
                Some('#') if comments => skip_line(chars, pos),
                Some('\\') if posix => {
                    escaped_state = 'a';
                    state = '\\';
                }
                Some(c) if is_quote(c) => {
                    if !posix {
                        token.push(c);
                    }
                    state = c;
                }
                Some(c) => {
                    token.push(c);
                    state = 'a';
                }
            },
            '\'' | '"' => {
                quoted = true;
                match next {
                    None => return Err("No closing quotation".to_string()),
                    Some(c) if c == state => {
                        if posix {
                            state = 'a';
                        } else {
                            token.push(c);
                            return Ok(Some(token));
                        }
                    }
                    Some('\\') if posix && state == '"' => {
                        escaped_state = state;
                        state = '\\';
                    }
                    Some(c) => token.push(c),
                }
            }
            '\\' => match next {
                None => return Err("No escaped character".to_string()),
                Some(c) => {
                    if is_quote(escaped_state) && c != '\\' && c != escaped_state {
                        token.push('\\');
                    }
                    token.push(c);
                    state = escaped_state;
                }
            },
            _ => match next {
                None => {
                    if posix && !quoted && token.is_empty() {
                        return Ok(None);
                    }
                    return Ok(Some(token));
                }
                Some(c) if is_space(c) => {
                    state = ' ';
                    if !token.is_empty() || (posix && quoted) {
                        return Ok(Some(token));
                    }
                }
                Some('#') if comments => {
                    skip_line(chars, pos);
                    if posix {
                        state = ' ';
                        if !token.is_empty() || quoted {
                            return Ok(Some(token));
                        }
                    }
                }
                Some(c) if posix && is_quote(c) => state = c,
                Some('\\') if posix => {
                    escaped_state = 'a';
                    state = '\\';
                }
                Some(c) => token.push(c),
            },
        }
    }
}

/// `shlex.split(s, comments, posix)`. O erro é a mensagem do `ValueError`.
pub fn split_str(s: &str, comments: bool, posix: bool) -> Result<Vec<String>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut pos = 0usize;
    let mut out = Vec::new();
    while let Some(t) = read_token(&chars, &mut pos, comments, posix)? {
        out.push(t);
    }
    Ok(out)
}

/// `shlex.quote(s)`.
pub fn quote_str(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-');
    if s.chars().all(safe) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn split(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("split", args, kw, &["s", "comments", "posix"], 1)?;
    let s = want_str("split", a[0].as_ref().unwrap())?;
    let comments = a[1].as_ref().map(|v| v.is_true()).unwrap_or(false);
    let posix = a[2].as_ref().map(|v| v.is_true()).unwrap_or(true);
    let toks = split_str(s, comments, posix).map_err(|m| exc("ValueError", m))?;
    Ok(Value::list(toks.into_iter().map(Value::str).collect()))
}

fn want_quotable(v: &Value) -> PyResult<&str> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(type_error(format!("expected string or bytes-like object, got '{}'", other.type_name()))),
    }
}

fn quote(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("quote", &kw)?;
    exactly("quote", &args, 1)?;
    Ok(Value::str(quote_str(want_quotable(&args[0])?)))
}

fn join(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("join", &kw)?;
    exactly("join", &args, 1)?;
    let items = iterate(&args[0])?;
    let mut parts = Vec::with_capacity(items.len());
    for it in &items {
        parts.push(quote_str(want_quotable(it)?));
    }
    Ok(Value::str(parts.join(" ")))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("shlex").func("split", split).func("quote", quote).func("join", join).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{repr, to_str};

    fn sp(s: &str) -> Vec<String> {
        split_str(s, false, true).unwrap()
    }

    #[test]
    fn split_posix() {
        assert_eq!(sp("a b 'c d' \"e f\""), vec!["a", "b", "c d", "e f"]);
        assert_eq!(sp("a\\ b"), vec!["a b"]);
        assert_eq!(sp("x \"a\\\"b\""), vec!["x", "a\"b"]);
        assert_eq!(sp("a''b"), vec!["ab"]);
        assert_eq!(sp("''"), vec![""]);
        assert_eq!(sp("  "), Vec::<String>::new());
        assert_eq!(sp("\"a\\nb\""), vec!["a\\nb"]);
        assert_eq!(sp("a#b c"), vec!["a#b", "c"]);
    }

    #[test]
    fn split_comments_and_errors() {
        assert_eq!(split_str("# c\nfoo bar # tail", true, true).unwrap(), vec!["foo", "bar"]);
        assert_eq!(split_str("\"oops", false, true).unwrap_err(), "No closing quotation");
        assert_eq!(split_str("a\\", false, true).unwrap_err(), "No escaped character");
    }

    #[test]
    fn split_non_posix() {
        assert_eq!(split_str("a 'b c'", false, false).unwrap(), vec!["a", "'b c'"]);
    }

    #[test]
    fn quote_and_join() {
        assert_eq!(quote_str(""), "''");
        assert_eq!(quote_str("abc-1.2/x"), "abc-1.2/x");
        assert_eq!(quote_str("it's"), "'it'\"'\"'s'");
        assert_eq!(quote_str("a b"), "'a b'");
        let mut vm = Vm::new();
        let l = Value::list(vec![Value::str("a b"), Value::str("c")]);
        let r = join(&mut vm, vec![l], Vec::new()).unwrap();
        assert_eq!(to_str(&r), "'a b' c");
        let r = split(&mut vm, vec![Value::str("x 'y z'")], Vec::new()).unwrap();
        assert_eq!(repr(&r), "['x', 'y z']");
        let e = quote(&mut vm, vec![Value::Int(1)], Vec::new()).unwrap_err();
        assert_eq!(e.msg, "expected string or bytes-like object, got 'int'");
    }
}
