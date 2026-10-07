//! Iteradores preguiçosos embutidos: `map`, `filter`, `zip` e `enumerate`. Cada um guarda os
//! iteradores de origem e só puxa um item quando alguém pede o próximo, como no CPython (então
//! funcionam com geradores infinitos e preservam a ordem dos efeitos colaterais).

use std::cell::{Cell, RefCell};

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{current, exc, get_iter, internal, PyIter, PyResult, Vm};

fn stop(name: &str, method: &str) -> PyResult<Value> {
    let _ = (name, method);
    Err(exc("StopIteration", ""))
}

fn attr_error(kind: &str, name: &str) -> crate::vm::PyException {
    exc("AttributeError", format!("'{kind}' object has no attribute '{name}'"))
}

pub struct MapIter {
    func: Value,
    iters: RefCell<Vec<PyIter>>,
}

impl MapIter {
    pub fn new(func: Value, sources: &[Value]) -> PyResult<Value> {
        let iters = sources.iter().map(get_iter).collect::<PyResult<Vec<_>>>()?;
        Ok(Value::Ext(std::rc::Rc::new(MapIter { func, iters: RefCell::new(iters) })))
    }
}

impl ExtObject for MapIter {
    fn type_name(&self) -> &'static str {
        "map"
    }
    fn repr(&self) -> String {
        format!("<map object at {:#x}>", crate::object::py_addr(self as *const MapIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let mut args = Vec::new();
        for it in self.iters.borrow_mut().iter_mut() {
            match it.next()? {
                Some(v) => args.push(v),
                None => return Ok(None),
            }
        }
        let mut vm = current().ok_or_else(|| internal("no vm"))?;
        Ok(Some(vm.call_value(&self.func, args, Vec::new())?))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop("map", name),
            },
            _ => Err(attr_error("map", name)),
        }
    }
}

/// `iter(chamável, sentinela)`: chama a função a cada passo e termina ao receber o valor sentinela.
pub struct CallIter {
    func: Value,
    sentinel: Value,
    done: Cell<bool>,
}

impl CallIter {
    pub fn new(func: Value, sentinel: Value) -> Value {
        Value::Ext(std::rc::Rc::new(CallIter { func, sentinel, done: Cell::new(false) }))
    }
}

impl ExtObject for CallIter {
    fn type_name(&self) -> &'static str {
        "callable_iterator"
    }
    fn repr(&self) -> String {
        format!("<callable_iterator object at {:#x}>", crate::object::py_addr(self as *const CallIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        if self.done.get() {
            return Ok(None);
        }
        let mut vm = current().ok_or_else(|| internal("no vm"))?;
        let x = vm.call_value(&self.func, Vec::new(), Vec::new())?;
        if crate::object::py_eq(&x, &self.sentinel) {
            self.done.set(true);
            return Ok(None);
        }
        Ok(Some(x))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop("callable_iterator", name),
            },
            _ => Err(attr_error("callable_iterator", name)),
        }
    }
}

pub struct FilterIter {
    func: Value,
    iter: RefCell<PyIter>,
}

impl FilterIter {
    pub fn new(func: Value, source: &Value) -> PyResult<Value> {
        Ok(Value::Ext(std::rc::Rc::new(FilterIter { func, iter: RefCell::new(get_iter(source)?) })))
    }
}

impl ExtObject for FilterIter {
    fn type_name(&self) -> &'static str {
        "filter"
    }
    fn repr(&self) -> String {
        format!("<filter object at {:#x}>", crate::object::py_addr(self as *const FilterIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        loop {
            let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
            let keep = if matches!(self.func, Value::None) {
                x.is_true()
            } else {
                let mut vm = current().ok_or_else(|| internal("no vm"))?;
                vm.call_value(&self.func, vec![x.clone()], Vec::new())?.is_true()
            };
            if keep {
                return Ok(Some(x));
            }
        }
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop("filter", name),
            },
            _ => Err(attr_error("filter", name)),
        }
    }
}

pub struct ZipIter {
    iters: RefCell<Vec<PyIter>>,
    strict: bool,
}

impl ZipIter {
    pub fn new(sources: &[Value], strict: bool) -> PyResult<Value> {
        let iters = sources.iter().map(get_iter).collect::<PyResult<Vec<_>>>()?;
        Ok(Value::Ext(std::rc::Rc::new(ZipIter { iters: RefCell::new(iters), strict })))
    }
}

impl ExtObject for ZipIter {
    fn type_name(&self) -> &'static str {
        "zip"
    }
    fn repr(&self) -> String {
        format!("<zip object at {:#x}>", crate::object::py_addr(self as *const ZipIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let mut iters = self.iters.borrow_mut();
        if iters.is_empty() {
            return Ok(None);
        }
        let mut items = Vec::with_capacity(iters.len());
        for i in 0..iters.len() {
            match iters[i].next()? {
                Some(v) => items.push(v),
                None => {
                    if self.strict {
                        return self.strict_end(&mut iters, i);
                    }
                    return Ok(None);
                }
            }
        }
        Ok(Some(Value::tuple(items)))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop("zip", name),
            },
            _ => Err(attr_error("zip", name)),
        }
    }
}

impl ZipIter {
    /// `zip(strict=True)`: o iterador `ended` acabou; os demais precisam ter acabado também.
    fn strict_end(&self, iters: &mut [PyIter], ended: usize) -> PyResult<Option<Value>> {
        let plural_range = |upto: usize| {
            if upto == 1 {
                ("", "1".to_string())
            } else {
                ("s", format!("1-{upto}"))
            }
        };
        if ended > 0 {
            let (plural, range) = plural_range(ended);
            return Err(crate::native_util::value_error(format!(
                "zip() argument {} is shorter than argument{plural} {range}",
                ended + 1
            )));
        }
        for j in 1..iters.len() {
            if iters[j].next()?.is_some() {
                let (plural, range) = plural_range(j);
                return Err(crate::native_util::value_error(format!(
                    "zip() argument {} is longer than argument{plural} {range}",
                    j + 1
                )));
            }
        }
        Ok(None)
    }
}

pub struct EnumerateIter {
    iter: RefCell<PyIter>,
    next_index: Cell<i64>,
}

impl EnumerateIter {
    pub fn new(source: &Value, start: i64) -> PyResult<Value> {
        Ok(Value::Ext(std::rc::Rc::new(EnumerateIter {
            iter: RefCell::new(get_iter(source)?),
            next_index: Cell::new(start),
        })))
    }
}

impl ExtObject for EnumerateIter {
    fn type_name(&self) -> &'static str {
        "enumerate"
    }
    fn repr(&self) -> String {
        format!("<enumerate object at {:#x}>", crate::object::py_addr(self as *const EnumerateIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
        let i = self.next_index.get();
        self.next_index.set(i + 1);
        Ok(Some(Value::tuple(vec![Value::Int(i), x])))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop("enumerate", name),
            },
            _ => Err(attr_error("enumerate", name)),
        }
    }
}

/// `reversed(seq)`: os itens de trás para a frente. O tipo é o que o CPython dá para cada origem
/// (`list_reverseiterator`, `range_iterator`, `dict_reversekeyiterator` ou o `reversed` genérico).
pub struct ReversedIter {
    kind: &'static str,
    items: Vec<Value>,
    next: Cell<usize>,
}

impl ReversedIter {
    pub fn new(kind: &'static str, mut items: Vec<Value>) -> Value {
        items.reverse();
        Value::Ext(std::rc::Rc::new(ReversedIter { kind, items, next: Cell::new(0) }))
    }
}

impl ExtObject for ReversedIter {
    fn type_name(&self) -> &'static str {
        self.kind
    }
    fn repr(&self) -> String {
        format!("<{} object at {:#x}>", self.kind, crate::object::py_addr(self as *const ReversedIter as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__length_hint__", "__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let i = self.next.get();
        let Some(v) = self.items.get(i) else { return Ok(None) };
        self.next.set(i + 1);
        Ok(Some(v.clone()))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => match self.iter_next()? {
                Some(v) => Ok(v),
                None => stop(self.kind, name),
            },
            "__length_hint__" => Ok(Value::Int((self.items.len() - self.next.get().min(self.items.len())) as i64)),
            _ => Err(attr_error(self.kind, name)),
        }
    }
}
