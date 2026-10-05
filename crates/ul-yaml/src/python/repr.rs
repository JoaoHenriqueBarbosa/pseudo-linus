//! `str()` e `repr()` dos valores que o `json.loads` produz, como o CPython os escreve; o `csv`
//! converte com `str()` o campo que não é texto.
//!
//! O percurso é iterativo (pilha explícita): um JSON aninhado em milhares de níveis é válido pro
//! Python e não pode estourar a pilha da thread.

use crate::py::{Py, float_repr};

use super::text::as_surrogate;

enum Task<'a> {
    Value(&'a Py),
    Text(&'static str),
}

/// `repr(valor)`.
pub fn repr(v: &Py) -> String {
    let mut out = String::new();
    let mut stack: Vec<Task> = vec![Task::Value(v)];
    while let Some(task) = stack.pop() {
        match task {
            Task::Text(t) => out.push_str(t),
            Task::Value(Py::None) => out.push_str("None"),
            Task::Value(Py::Bool(b)) => out.push_str(if *b { "True" } else { "False" }),
            Task::Value(Py::Int(i)) => out.push_str(&i.to_string()),
            Task::Value(Py::Float(f)) => out.push_str(&float_repr(*f)),
            Task::Value(Py::Str(s) | Py::Date(s, _)) => str_repr(s, &mut out),
            Task::Value(Py::List(items)) => {
                out.push('[');
                stack.push(Task::Text("]"));
                for (i, item) in items.iter().enumerate().rev() {
                    stack.push(Task::Value(item));
                    if i > 0 {
                        stack.push(Task::Text(", "));
                    }
                }
            }
            Task::Value(Py::Dict(pairs)) => {
                out.push('{');
                stack.push(Task::Text("}"));
                for (i, (key, val)) in pairs.iter().enumerate().rev() {
                    stack.push(Task::Value(val));
                    stack.push(Task::Text(": "));
                    stack.push(Task::Value(key));
                    if i > 0 {
                        stack.push(Task::Text(", "));
                    }
                }
            }
        }
    }
    out
}

/// `str(valor)`: o próprio texto pra `str`, o `repr` pro resto.
pub fn to_str(v: &Py) -> String {
    match v {
        Py::Str(s) => s.clone(),
        other => repr(other),
    }
}

/// `repr(str)`: aspas simples, salvo quando o texto tem `'` e não tem `"`.
fn str_repr(s: &str, out: &mut String) {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => {
                if let Some(surrogate) = as_surrogate(c) {
                    out.push_str(&format!("\\u{surrogate:04x}"));
                } else if is_printable(c) {
                    out.push(c);
                } else {
                    let v = c as u32;
                    if v <= 0xff {
                        out.push_str(&format!("\\x{v:02x}"));
                    } else if v <= 0xffff {
                        out.push_str(&format!("\\u{v:04x}"));
                    } else {
                        out.push_str(&format!("\\U{v:08x}"));
                    }
                }
            }
        }
    }
    out.push(quote);
}

/// `str.isprintable()` por caractere. Sem as tabelas do Unicode: ASCII e Latin-1 exatos, e do resto
/// só as categorias de controle, formato, separador, uso privado e substituto que se conhecem por
/// faixa. Um ponto ainda não atribuído conta como imprimível (o Python o escaparia).
fn is_printable(c: char) -> bool {
    let v = c as u32;
    if v < 0x20 || v == 0x7f {
        return false;
    }
    if v < 0x7f {
        return true;
    }
    if v <= 0xa0 || v == 0xad {
        return false;
    }
    !matches!(
        v,
        0x600..=0x605
            | 0x61c
            | 0x6dd
            | 0x70f
            | 0x890..=0x891
            | 0x8e2
            | 0x1680
            | 0x180e
            | 0x2000..=0x200f
            | 0x2028..=0x202f
            | 0x205f..=0x2064
            | 0x2066..=0x206f
            | 0x3000
            | 0xd800..=0xf8ff
            | 0xfeff
            | 0xfff9..=0xfffb
            | 0xe0001
            | 0xe0020..=0xe007f
            | 0xf0000..=0x10ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_of_containers() {
        let v = Py::List(vec![
            Py::Int(1i32.into()),
            Py::Str("a".to_string()),
            Py::None,
            Py::Bool(true),
            Py::Dict(vec![(Py::Str("k".to_string()), Py::Float(2.5))]),
        ]);
        assert_eq!(repr(&v), "[1, 'a', None, True, {'k': 2.5}]");
    }

    #[test]
    fn repr_of_strings() {
        assert_eq!(repr(&Py::Str("it's".to_string())), "\"it's\"");
        assert_eq!(repr(&Py::Str("a\"b'c".to_string())), "'a\"b\\'c'");
        assert_eq!(repr(&Py::Str("tab\there\u{1}".to_string())), "'tab\\there\\x01'");
    }
}
