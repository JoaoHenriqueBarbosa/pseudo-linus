//! Módulo `string` do CPython 3.13: constantes de caracteres e `capwords`.
//!
//! Ficam de fora: `Template`, `Formatter` (precisam de classes) e `string.Formatter`.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

const ASCII_LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const ASCII_UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const HEXDIGITS: &str = "0123456789abcdefABCDEF";
const OCTDIGITS: &str = "01234567";
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
const WHITESPACE: &str = " \t\n\r\u{b}\u{c}";

fn capitalize(word: &str) -> String {
    let mut it = word.chars();
    match it.next() {
        None => String::new(),
        Some(first) => {
            let mut out: String = first.to_uppercase().collect();
            out.push_str(&it.as_str().to_lowercase());
            out
        }
    }
}

/// `string.capwords(s, sep=None)`.
pub fn capwords_str(s: &str, sep: Option<&str>) -> Result<String, &'static str> {
    match sep {
        None => Ok(s.split_whitespace().map(capitalize).collect::<Vec<_>>().join(" ")),
        Some("") => Err("empty separator"),
        Some(sep) => Ok(s.split(sep).map(capitalize).collect::<Vec<_>>().join(sep)),
    }
}

fn capwords(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("capwords", args, kw, &["s", "sep"], 1)?;
    let s = want_str("capwords", a[0].as_ref().unwrap())?;
    let sep = match &a[1] {
        None | Some(Value::None) => None,
        Some(Value::Str(p)) => Some(p.as_str()),
        Some(other) => {
            return Err(type_error(format!("must be str or None, not {}", other.type_name())));
        }
    };
    capwords_str(s, sep).map(Value::str).map_err(|m| exc("ValueError", m))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    let printable = format!("{DIGITS}{ASCII_LOWERCASE}{ASCII_UPPERCASE}{PUNCTUATION}{WHITESPACE}");
    let letters = format!("{ASCII_LOWERCASE}{ASCII_UPPERCASE}");
    ModuleBuilder::new("string")
        .value("ascii_letters", Value::str(letters))
        .value("ascii_lowercase", Value::str(ASCII_LOWERCASE))
        .value("ascii_uppercase", Value::str(ASCII_UPPERCASE))
        .value("digits", Value::str(DIGITS))
        .value("hexdigits", Value::str(HEXDIGITS))
        .value("octdigits", Value::str(OCTDIGITS))
        .value("punctuation", Value::str(PUNCTUATION))
        .value("printable", Value::str(printable))
        .value("whitespace", Value::str(WHITESPACE))
        .func("capwords", capwords)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{repr, to_str};

    #[test]
    fn constants() {
        let mut vm = Vm::new();
        let m = build(&mut vm);
        let a = m.attrs.borrow();
        assert_eq!(to_str(&a["ascii_letters"]), "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert_eq!(to_str(&a["digits"]), "0123456789");
        assert_eq!(to_str(&a["hexdigits"]), "0123456789abcdefABCDEF");
        assert_eq!(to_str(&a["octdigits"]), "01234567");
        assert_eq!(to_str(&a["punctuation"]), "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~");
        assert_eq!(repr(&a["whitespace"]), "' \\t\\n\\r\\x0b\\x0c'");
        assert_eq!(to_str(&a["printable"]).chars().count(), 100);
    }

    #[test]
    fn capwords_behaviour() {
        assert_eq!(capwords_str("hello   wORLD  foo", None).unwrap(), "Hello World Foo");
        assert_eq!(capwords_str("a-b  c-d", Some("-")).unwrap(), "A-B  c-D");
        assert_eq!(capwords_str("  x ", None).unwrap(), "X");
        assert_eq!(capwords_str("x", Some("")).unwrap_err(), "empty separator");
        let mut vm = Vm::new();
        let r = capwords(&mut vm, vec![Value::str("ola mundo")], Vec::new()).unwrap();
        assert_eq!(to_str(&r), "Ola Mundo");
    }
}
