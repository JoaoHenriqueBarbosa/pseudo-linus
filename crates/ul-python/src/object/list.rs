//! `list` e `tuple` (`Objects/listobject.c`, `Objects/tupleobject.c`): `repr` com a proteção de
//! recursão do `Py_ReprEnter` e a comparação elemento a elemento.

use super::{is, py_eq, repr_into, ReprStack, Value};

/// `list_repr`: `[]` vazio, `[...]` quando a lista já está sendo impressa mais acima.
pub(super) fn list_repr(items: &[Value], id: usize, out: &mut String, stack: &mut ReprStack) {
    seq_repr(items, id, ("[", "]"), false, out, stack);
}

/// `tuple_repr`: `()` vazio, `(x,)` com um elemento e `(...)` na recursão.
pub(super) fn tuple_repr(items: &[Value], id: usize, out: &mut String, stack: &mut ReprStack) {
    seq_repr(items, id, ("(", ")"), true, out, stack);
}

fn seq_repr(
    items: &[Value],
    id: usize,
    (open, close): (&str, &str),
    is_tuple: bool,
    out: &mut String,
    stack: &mut ReprStack,
) {
    out.push_str(open);
    if items.is_empty() {
        out.push_str(close);
        return;
    }
    if !stack.enter(id) {
        out.push_str("...");
        out.push_str(close);
        return;
    }
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        repr_into(item, out, stack);
    }
    if is_tuple && items.len() == 1 {
        out.push(',');
    }
    stack.leave(id);
    out.push_str(close);
}

/// Igualdade de sequências: tamanhos iguais e cada par idêntico (`is`, o atalho do
/// `PyObject_RichCompare` usado por `list_richcompare`) ou igual.
pub(super) fn seq_eq(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| is(x, y) || py_eq(x, y))
}
