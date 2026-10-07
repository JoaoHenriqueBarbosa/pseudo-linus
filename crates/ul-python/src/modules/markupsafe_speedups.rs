//! `markupsafe._speedups` do MarkupSafe 3.0.4: só `_escape_inner(s)`, que troca `&`, `>`, `<`, `'` e `"`
//! por `&amp;`, `&gt;`, `&lt;`, `&#39;` e `&#34;`.
//!
//! Quando não há nada a trocar devolve o próprio objeto recebido (identidade preservada), como o C.
//! O módulo não é importável pelo nome curto: só existe como `markupsafe._speedups`, carregado a
//! partir do `.so` da wheel.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{type_error, PyResult, Vm};

/// Escapa `text`; `None` quando não há caractere a trocar.
fn escape_inner(text: &str) -> Option<String> {
    let first = text.bytes().position(|b| matches!(b, b'&' | b'>' | b'<' | b'\'' | b'"'))?;
    let mut out = String::with_capacity(text.len() + text.len() / 4 + 8);
    out.push_str(&text[..first]);
    for c in text[first..].chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '>' => out.push_str("&gt;"),
            '<' => out.push_str("&lt;"),
            '\'' => out.push_str("&#39;"),
            '"' => out.push_str("&#34;"),
            c => out.push(c),
        }
    }
    Some(out)
}

fn escape_inner_fn(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if !kw.is_empty() {
        return Err(type_error("markupsafe._speedups._escape_inner() takes no keyword arguments"));
    }
    let [arg] = <[Value; 1]>::try_from(args).map_err(|a| {
        type_error(format!("markupsafe._speedups._escape_inner() takes exactly one argument ({} given)", a.len()))
    })?;
    // O C devolve NULL sem exceção para o que não é str: o interpretador acusa o próprio módulo.
    let Value::Str(s) = &arg else { return Err(crate::vm::exc("SystemError", "error return without exception set")) };
    match escape_inner(s.as_str()) {
        Some(out) => Ok(Value::str(out)),
        None => Ok(arg),
    }
}

pub fn build_markupsafe_speedups(_vm: &mut Vm) -> PyResult<Rc<ModuleObj>> {
    Ok(ModuleBuilder::new("markupsafe._speedups")
        .func("_escape_inner", escape_inner_fn)
        .value("__doc__", Value::None)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::to_str;

    fn call(args: Vec<Value>) -> PyResult<Value> {
        let mut vm = Vm::new();
        escape_inner_fn(&mut vm, args, Vec::new())
    }

    #[test]
    fn escapes_the_five_characters() {
        let v = call(vec![Value::str("<a href=\"x\">'&'</a>")]).unwrap();
        assert_eq!(to_str(&v), "&lt;a href=&#34;x&#34;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }

    #[test]
    fn keeps_non_ascii_text() {
        let v = call(vec![Value::str("ação & <é>")]).unwrap();
        assert_eq!(to_str(&v), "ação &amp; &lt;é&gt;");
    }

    #[test]
    fn returns_same_object_when_nothing_to_replace() {
        let s = Value::str("plain text");
        let v = call(vec![s.clone()]).unwrap();
        match (&s, &v) {
            (Value::Str(a), Value::Str(b)) => assert!(Rc::ptr_eq(a, b)),
            _ => panic!("esperava str"),
        }
    }

    #[test]
    fn rejects_non_str() {
        let e = call(vec![Value::Bool(true)]).unwrap_err();
        assert_eq!(e.kind, "SystemError");
        assert_eq!(e.msg, "error return without exception set");
        assert!(call(vec![]).is_err());
    }

    #[test]
    fn module_shape() {
        let mut vm = Vm::new();
        let m = build_markupsafe_speedups(&mut vm).unwrap();
        assert_eq!(m.name, "markupsafe._speedups");
        let attrs = m.attrs.borrow();
        assert!(attrs.contains_key("_escape_inner"));
        assert!(matches!(attrs.get("__doc__"), Some(Value::None)));
    }
}
