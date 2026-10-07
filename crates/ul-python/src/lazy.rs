//! Iteradores preguiçosos embutidos: `map`, `filter`, `zip` e `enumerate`. Cada um guarda os
//! iteradores de origem e só puxa um item quando alguém pede o próximo, como no CPython (então
//! funcionam com geradores infinitos e preservam a ordem dos efeitos colaterais).

use std::cell::{Cell, RefCell};

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{current, exc, get_iter, internal, PyIter, PyResult, Vm};

/// `iterador.__next__()`: o próximo item, ou `StopIteration`; outro método é `AttributeError`.
fn next_method(it: &dyn ExtObject, name: &str) -> PyResult<Value> {
    match name {
        "__next__" => it.iter_next()?.ok_or_else(|| exc("StopIteration", "")),
        _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", it.type_name()))),
    }
}

/// O `repr` padrão de um objeto: `<tipo object at 0x...>`.
fn object_repr<T>(kind: &str, obj: &T) -> String {
    format!("<{kind} object at {:#x}>", crate::object::py_addr(obj as *const T as usize))
}

/// A parte de `ExtObject` que todo iterador preguiçoso de tipo fixo tem igual (nome, `repr`, só o
/// `__next__`), com o `iter_next` próprio de cada um.
macro_rules! lazy_iterator {
    ($ty:ident, $name:literal, fn iter_next(&$s:ident) $body:block) => {
        impl ExtObject for $ty {
            fn type_name(&self) -> &'static str {
                $name
            }
            fn repr(&self) -> String {
                object_repr($name, self)
            }
            fn methods(&self) -> &'static [&'static str] {
                &["__next__"]
            }
            fn is_iterable(&self) -> bool {
                true
            }
            fn iter_next(&$s) -> PyResult<Option<Value>> $body
            fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
                next_method(self, name)
            }
        }
    };
}

/// Chama `func` com `args` na VM corrente.
fn call(func: &Value, args: Vec<Value>) -> PyResult<Value> {
    let mut vm = current().ok_or_else(|| internal("no vm"))?;
    vm.call_value(func, args, Vec::new())
}

/// O próximo item de cada iterador, ou `None` (e a posição do que acabou) se um deles acabou.
fn next_of_each(iters: &mut [PyIter]) -> PyResult<Result<Vec<Value>, usize>> {
    let mut items = Vec::with_capacity(iters.len());
    for (i, it) in iters.iter_mut().enumerate() {
        match it.next()? {
            Some(v) => items.push(v),
            None => return Ok(Err(i)),
        }
    }
    Ok(Ok(items))
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

lazy_iterator!(MapIter, "map", fn iter_next(&self) {
    // O empréstimo dos iteradores acaba antes da chamada, que pode voltar a este mesmo `map`.
    let args = next_of_each(&mut self.iters.borrow_mut())?;
    match args {
        Ok(args) => Ok(Some(call(&self.func, args)?)),
        Err(_) => Ok(None),
    }
});

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

lazy_iterator!(CallIter, "callable_iterator", fn iter_next(&self) {
    if self.done.get() {
        return Ok(None);
    }
    let x = call(&self.func, Vec::new())?;
    if crate::object::py_eq(&x, &self.sentinel) {
        self.done.set(true);
        return Ok(None);
    }
    Ok(Some(x))
});

pub struct FilterIter {
    func: Value,
    iter: RefCell<PyIter>,
}

impl FilterIter {
    pub fn new(func: Value, source: &Value) -> PyResult<Value> {
        Ok(Value::Ext(std::rc::Rc::new(FilterIter { func, iter: RefCell::new(get_iter(source)?) })))
    }
}

lazy_iterator!(FilterIter, "filter", fn iter_next(&self) {
    loop {
        let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
        let keep = if matches!(self.func, Value::None) {
            x.is_true()
        } else {
            call(&self.func, vec![x.clone()])?.is_true()
        };
        if keep {
            return Ok(Some(x));
        }
    }
});

pub struct ZipIter {
    iters: RefCell<Vec<PyIter>>,
    strict: bool,
}

impl ZipIter {
    pub fn new(sources: &[Value], strict: bool) -> PyResult<Value> {
        let iters = sources.iter().map(get_iter).collect::<PyResult<Vec<_>>>()?;
        Ok(Value::Ext(std::rc::Rc::new(ZipIter { iters: RefCell::new(iters), strict })))
    }

    /// `zip(strict=True)`: o iterador `ended` acabou; os demais precisam ter acabado também.
    fn strict_end(&self, iters: &mut [PyIter], ended: usize) -> PyResult<Option<Value>> {
        let mismatch = |arg: usize, how: &str| {
            let (plural, range) = if arg == 1 { ("", "1".to_string()) } else { ("s", format!("1-{arg}")) };
            Err(crate::native_util::value_error(format!(
                "zip() argument {} is {how} than argument{plural} {range}",
                arg + 1
            )))
        };
        if ended > 0 {
            return mismatch(ended, "shorter");
        }
        for j in 1..iters.len() {
            if iters[j].next()?.is_some() {
                return mismatch(j, "longer");
            }
        }
        Ok(None)
    }
}

lazy_iterator!(ZipIter, "zip", fn iter_next(&self) {
    let mut iters = self.iters.borrow_mut();
    if iters.is_empty() {
        return Ok(None);
    }
    match next_of_each(&mut iters)? {
        Ok(items) => Ok(Some(Value::tuple(items))),
        Err(ended) if self.strict => self.strict_end(&mut iters, ended),
        Err(_) => Ok(None),
    }
});

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

lazy_iterator!(EnumerateIter, "enumerate", fn iter_next(&self) {
    let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
    let i = self.next_index.get();
    self.next_index.set(i + 1);
    Ok(Some(Value::tuple(vec![Value::Int(i), x])))
});

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
        object_repr(self.kind, self)
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
            "__length_hint__" => Ok(Value::Int(self.items.len().saturating_sub(self.next.get()) as i64)),
            _ => next_method(self, name),
        }
    }
}
