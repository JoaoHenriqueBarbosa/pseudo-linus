//! Views de dicionário: `d.keys()`, `d.values()` e `d.items()`. Refletem o dicionário (a view
//! enxerga mudanças posteriores), podem ser percorridas várias vezes e as de chaves e itens se
//! comportam como conjuntos (`&`, `|`, `-`, `^`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{Dict, ExtObject, Kw, Set, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Keys,
    Values,
    Items,
}

pub struct DictView {
    dict: Rc<RefCell<Dict>>,
    kind: Kind,
}

impl DictView {
    pub fn make(dict: Rc<RefCell<Dict>>, kind: Kind) -> Value {
        Value::Ext(Rc::new(DictView { dict, kind }))
    }

    fn items(&self) -> Vec<Value> {
        let d = self.dict.borrow();
        match self.kind {
            Kind::Keys => d.keys().cloned().collect(),
            Kind::Values => d.values().cloned().collect(),
            Kind::Items => d.iter().map(|(k, v)| Value::tuple(vec![k.clone(), v.clone()])).collect(),
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Keys => "dict_keys",
            Kind::Values => "dict_values",
            Kind::Items => "dict_items",
        }
    }

    /// As view de chaves e de itens são conjuntos; a de valores não.
    fn set_like(&self) -> bool {
        self.kind != Kind::Values
    }
}

fn to_set(items: Vec<Value>) -> Result<Set, PyException> {
    let mut s = Set::new();
    for v in items {
        s.add(v)?;
    }
    Ok(s)
}

impl ExtObject for DictView {
    fn type_name(&self) -> &'static str {
        self.name()
    }

    fn repr(&self) -> String {
        let inner: Vec<String> = self.items().iter().map(crate::object::repr).collect();
        format!("{}([{}])", self.name(), inner.join(", "))
    }

    fn methods(&self) -> &'static [&'static str] {
        &["isdisjoint"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match (name, args.as_slice()) {
            ("isdisjoint", [other]) if self.set_like() => {
                let mine = to_set(self.items())?;
                for x in crate::vm::iterate(other)? {
                    if mine.contains(&x)? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", self.name()))),
        }
    }

    fn len(&self) -> Option<usize> {
        Some(self.dict.borrow().len())
    }

    fn is_true(&self) -> bool {
        !self.dict.borrow().is_empty()
    }

    fn to_items(&self) -> Option<Vec<Value>> {
        Some(self.items())
    }

    fn contains_item(&self, item: &Value) -> Option<Result<bool, PyException>> {
        match self.kind {
            Kind::Keys => Some(self.dict.borrow().contains(item).map_err(Into::into)),
            Kind::Items => match item {
                Value::Tuple(t) if t.len() == 2 => {
                    let found = self.dict.borrow().get(&t[0]);
                    Some(match found {
                        Ok(Some(v)) => Ok(crate::object::py_eq(&v, &t[1])),
                        Ok(None) => Ok(false),
                        Err(e) => Err(e.into()),
                    })
                }
                _ => Some(Ok(false)),
            },
            Kind::Values => None,
        }
    }

    fn binop(&self, op: &str, other: &Value, reflected: bool) -> Option<PyResult<Value>> {
        if !self.set_like() || !matches!(op, "&" | "|" | "-" | "^") {
            return None;
        }
        let other_items = match crate::vm::iterate(other) {
            Ok(items) => items,
            Err(_) => return None,
        };
        let (left, right) = if reflected { (other_items, self.items()) } else { (self.items(), other_items) };
        let result = (|| -> PyResult<Value> {
            let (ls, rs) = (to_set(left.clone())?, to_set(right.clone())?);
            let mut out = Set::new();
            match op {
                "&" => {
                    for x in &left {
                        if rs.contains(x)? {
                            out.add(x.clone())?;
                        }
                    }
                }
                "|" => {
                    for x in left.iter().chain(right.iter()) {
                        out.add(x.clone())?;
                    }
                }
                "-" => {
                    for x in &left {
                        if !rs.contains(x)? {
                            out.add(x.clone())?;
                        }
                    }
                }
                _ => {
                    for x in &left {
                        if !rs.contains(x)? {
                            out.add(x.clone())?;
                        }
                    }
                    for x in &right {
                        if !ls.contains(x)? {
                            out.add(x.clone())?;
                        }
                    }
                }
            }
            Ok(Value::set(out))
        })();
        Some(result)
    }

    fn richcmp(&self, op: &str, other: &Value) -> Option<PyResult<bool>> {
        if !self.set_like() || !matches!(op, "==" | "!=") {
            return None;
        }
        let comparable = match other {
            Value::Set(_) => true,
            Value::Ext(e) => matches!(e.type_name(), "dict_keys" | "dict_items"),
            _ => false,
        };
        if !comparable {
            return Some(Ok(op == "!="));
        }
        let result = (|| -> PyResult<bool> {
            let mine = to_set(self.items())?;
            let theirs = to_set(crate::vm::iterate(other)?)?;
            let mut same = mine.len() == theirs.len();
            if same {
                for x in mine.iter() {
                    if !theirs.contains(x)? {
                        same = false;
                        break;
                    }
                }
            }
            Ok(if op == "==" { same } else { !same })
        })();
        Some(result)
    }
}

/// Atalho para os erros de tipo das funções nativas.
#[allow(dead_code)]
fn unsupported(what: &str) -> PyException {
    type_error(what.to_string())
}
