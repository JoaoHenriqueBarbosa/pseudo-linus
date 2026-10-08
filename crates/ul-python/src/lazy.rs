//! Iteradores preguiçosos embutidos: `map`, `filter`, `zip` e `enumerate`. Cada um guarda os
//! iteradores de origem e só puxa um item quando alguém pede o próximo, como no CPython (então
//! funcionam com geradores infinitos e preservam a ordem dos efeitos colaterais).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::object::{ExtImage, ExtObject, FuncObj, Kw, Value};
use crate::vm::{current, exc, get_iter, internal, type_error, PyException, PyIter, PyResult, Vm};

/// O iterador de um laço `for` (`PyIter`) como dado: os `Value`s que ele aponta inteiros, para a
/// imagem do heap (`heapimage`) e para o quadro de um gerador suspenso.
pub enum IterParts {
    List(Value, usize),
    Tuple(Value, usize),
    Str(Value, usize),
    Range { next: i64, step: i64, remaining: i64 },
    Items(Vec<Value>, usize),
    Native(Value),
    Ext(Value),
    Inst(Value),
}

pub(crate) fn iter_parts(it: &PyIter) -> IterParts {
    match it {
        PyIter::List(l, i) => IterParts::List(Value::List(l.clone()), *i),
        PyIter::Tuple(t, i) => IterParts::Tuple(Value::Tuple(t.clone()), *i),
        PyIter::Str(s, i) => IterParts::Str(Value::Str(s.clone()), *i),
        PyIter::Range { next, step, remaining } => IterParts::Range { next: *next, step: *step, remaining: *remaining },
        PyIter::Items(items, i) => IterParts::Items(items.clone(), *i),
        PyIter::Native(n) => IterParts::Native(Value::Native(n.clone())),
        PyIter::Ext(e) => IterParts::Ext(Value::Ext(e.clone())),
        PyIter::Inst(v) => IterParts::Inst(v.clone()),
    }
}

/// Refaz o iterador; `None` se um valor não é do tipo que a variante pede (imagem malformada).
pub(crate) fn iter_from_parts(parts: IterParts) -> Option<PyIter> {
    Some(match parts {
        IterParts::List(Value::List(l), i) => PyIter::List(l, i),
        IterParts::Tuple(Value::Tuple(t), i) => PyIter::Tuple(t, i),
        IterParts::Str(Value::Str(s), i) => PyIter::Str(s, i),
        IterParts::Range { next, step, remaining } => PyIter::Range { next, step, remaining },
        IterParts::Items(items, i) => PyIter::Items(items, i),
        IterParts::Native(Value::Native(n)) => PyIter::Native(n),
        IterParts::Ext(Value::Ext(e)) => PyIter::Ext(e),
        IterParts::Inst(v) => PyIter::Inst(v),
        IterParts::List(..) | IterParts::Tuple(..) | IterParts::Str(..) | IterParts::Native(_) | IterParts::Ext(_) => return None,
    })
}

fn iters_parts(iters: &RefCell<Vec<PyIter>>) -> Vec<IterParts> {
    iters.borrow().iter().map(iter_parts).collect()
}

/// O estado de um iterador preguiçoso embutido como dado (ver [`IterParts`]).
pub enum LazyParts {
    /// `iter(seq)`: a cópia dos itens e a posição.
    Seq { kind: &'static str, items: Vec<Value>, pos: usize },
    /// `reversed(seq)`: itens já invertidos e a posição.
    Reversed { kind: &'static str, items: Vec<Value>, next: usize },
    Map { func: Value, iters: Vec<IterParts> },
    Filter { func: Value, iter: IterParts },
    Zip { iters: Vec<IterParts>, strict: bool },
    Enumerate { iter: IterParts, next_index: i64 },
    CallIter { func: Value, sentinel: Value, done: bool },
    /// [`IterBox`]: o iterador vivo de um iterável qualquer.
    Boxed { iter: IterParts },
    /// [`OldSeqIter`]: o protocolo antigo de sequência (`__getitem__(0)`, `__getitem__(1)`...).
    OldSeq { obj: Value, index: i64, done: bool },
}

/// Refaz o iterador de `parts`; `None` se algum `Value` não é do tipo da variante.
pub(crate) fn lazy_from_parts(parts: LazyParts) -> Option<Value> {
    let many = |list: Vec<IterParts>| list.into_iter().map(iter_from_parts).collect::<Option<Vec<PyIter>>>();
    Some(match parts {
        LazyParts::Seq { kind, items, pos } => crate::builtins::seq_iter(kind, items, pos),
        LazyParts::Reversed { kind, items, next } => {
            Value::Ext(Rc::new(ReversedIter { kind, items, next: Cell::new(next) }))
        }
        LazyParts::Map { func, iters } => Value::Ext(Rc::new(MapIter { func, iters: RefCell::new(many(iters)?) })),
        LazyParts::Filter { func, iter } => Value::Ext(Rc::new(FilterIter { func, iter: RefCell::new(iter_from_parts(iter)?) })),
        LazyParts::Zip { iters, strict } => Value::Ext(Rc::new(ZipIter { iters: RefCell::new(many(iters)?), strict })),
        LazyParts::Enumerate { iter, next_index } => Value::Ext(Rc::new(EnumerateIter {
            iter: RefCell::new(iter_from_parts(iter)?),
            next_index: Cell::new(next_index),
        })),
        LazyParts::CallIter { func, sentinel, done } => Value::Ext(Rc::new(CallIter { func, sentinel, done: Cell::new(done) })),
        LazyParts::Boxed { iter } => IterBox::new(iter_from_parts(iter)?),
        LazyParts::OldSeq { obj, index, done } => {
            Value::Ext(Rc::new(OldSeqIter { obj, index: Cell::new(index), done: Cell::new(done) }))
        }
    })
}

/// `iterador.__next__()`: o próximo item, ou `StopIteration`; outro método é `AttributeError`.
fn next_method(it: &dyn ExtObject, name: &str) -> PyResult<Value> {
    match name {
        "__next__" => it.iter_next()?.ok_or_else(|| exc("StopIteration", "")),
        _ => Err(crate::object::no_attribute(it.type_name(), name)),
    }
}

/// O `repr` padrão de um objeto: `<tipo object at 0x...>`.
pub(crate) fn object_repr<T>(kind: &str, obj: &T) -> String {
    format!("<{kind} object at {:#x}>", crate::object::py_addr(obj as *const T as usize))
}

/// A parte de `ExtObject` que todo iterador preguiçoso de tipo fixo tem igual (nome, `repr`, só o
/// `__next__`), com o `iter_next` próprio de cada um.
macro_rules! lazy_iterator {
    ($ty:ident, $name:literal, image(&$i:ident) $image:block, fn iter_next(&$s:ident) $body:block) => {
        impl ExtObject for $ty {
            fn type_name(&self) -> &'static str {
                $name
            }
            fn as_any(&self) -> Option<&dyn std::any::Any> {
                Some(self)
            }
            fn image(&$i) -> Option<ExtImage> $image
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
    vm.call(func, args, Vec::new())
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

lazy_iterator!(MapIter, "map",
image(&self) { Some(ExtImage::Lazy(LazyParts::Map { func: self.func.clone(), iters: iters_parts(&self.iters) })) },
fn iter_next(&self) {
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

lazy_iterator!(CallIter, "callable_iterator",
image(&self) {
    Some(ExtImage::Lazy(LazyParts::CallIter { func: self.func.clone(), sentinel: self.sentinel.clone(), done: self.done.get() }))
},
fn iter_next(&self) {
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

    /// Se o item passa no filtro (`None` como função: a verdade do próprio item).
    fn keeps(&self, x: &Value) -> PyResult<bool> {
        if matches!(self.func, Value::None) {
            Ok(x.is_true())
        } else {
            Ok(call(&self.func, vec![x.clone()])?.is_true())
        }
    }
}

lazy_iterator!(FilterIter, "filter",
image(&self) { Some(ExtImage::Lazy(LazyParts::Filter { func: self.func.clone(), iter: iter_parts(&self.iter.borrow()) })) },
fn iter_next(&self) {
    loop {
        let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
        if self.keeps(&x)? {
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
        if ended > 0 {
            return Err(zip_mismatch(ended, "shorter"));
        }
        for j in 1..iters.len() {
            if iters[j].next()?.is_some() {
                return Err(zip_mismatch(j, "longer"));
            }
        }
        Ok(None)
    }
}

/// O `ValueError` de `zip(strict=True)`: o argumento `arg` (de zero) é mais curto ou mais longo que os
/// anteriores.
pub(crate) fn zip_mismatch(arg: usize, how: &str) -> PyException {
    let (plural, range) = if arg == 1 { ("", "1".to_string()) } else { ("s", format!("1-{arg}")) };
    crate::native_util::value_error(format!("zip() argument {} is {how} than argument{plural} {range}", arg + 1))
}

lazy_iterator!(ZipIter, "zip",
image(&self) { Some(ExtImage::Lazy(LazyParts::Zip { iters: iters_parts(&self.iters), strict: self.strict })) },
fn iter_next(&self) {
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

    /// O par `(índice, item)`, avançando o contador.
    fn number(&self, x: Value) -> Value {
        let i = self.next_index.get();
        self.next_index.set(i + 1);
        Value::tuple(vec![Value::Int(i), x])
    }
}

lazy_iterator!(EnumerateIter, "enumerate",
image(&self) {
    Some(ExtImage::Lazy(LazyParts::Enumerate { iter: iter_parts(&self.iter.borrow()), next_index: self.next_index.get() }))
},
fn iter_next(&self) {
    let Some(x) = self.iter.borrow_mut().next()? else { return Ok(None) };
    Ok(Some(self.number(x)))
});

/// O iterador vivo de um iterável qualquer, como objeto: a coleta do laço de quadros puxa dele quando a fonte
/// não é um gerador (`sorted(lista, key=f)`, `sum(IteradorDeUsuario())`), e a imagem do heap o leva inteiro.
pub struct IterBox {
    iter: RefCell<PyIter>,
}

impl IterBox {
    pub(crate) fn new(iter: PyIter) -> Value {
        Value::Ext(Rc::new(IterBox { iter: RefCell::new(iter) }))
    }
}

lazy_iterator!(IterBox, "iterator",
image(&self) { Some(ExtImage::Lazy(LazyParts::Boxed { iter: iter_parts(&self.iter.borrow()) })) },
fn iter_next(&self) {
    self.iter.borrow_mut().next()
});

/// O iterador do protocolo antigo de sequência (`iter(obj)` sem `__iter__`, com `__getitem__`): chama
/// `obj[0]`, `obj[1]`... a cada passo, até `IndexError` ou `StopIteration`, que o esgotam de vez
/// (`iter_iternext` do CPython).
pub struct OldSeqIter {
    obj: Value,
    index: Cell<i64>,
    done: Cell<bool>,
}

impl OldSeqIter {
    pub(crate) fn new(obj: Value) -> Value {
        Value::Ext(Rc::new(OldSeqIter { obj, index: Cell::new(0), done: Cell::new(false) }))
    }
}

lazy_iterator!(OldSeqIter, "iterator",
image(&self) {
    Some(ExtImage::Lazy(LazyParts::OldSeq { obj: self.obj.clone(), index: self.index.get(), done: self.done.get() }))
},
fn iter_next(&self) {
    if self.done.get() {
        return Ok(None);
    }
    let mut vm = current().ok_or_else(|| internal("no vm"))?;
    match vm.call_dunder(&self.obj, "__getitem__", vec![Value::Int(self.index.get())]) {
        Some(Ok(item)) => {
            self.index.set(self.index.get() + 1);
            Ok(Some(item))
        }
        Some(Err(e)) if ends_old_sequence(&e) => {
            self.done.set(true);
            Ok(None)
        }
        Some(Err(e)) => Err(e),
        None => Err(type_error(format!("'{}' object is not iterable", self.obj.type_name()))),
    }
});

/// `IndexError` e `StopIteration` encerram o protocolo antigo de sequência.
fn ends_old_sequence(e: &PyException) -> bool {
    matches!(e.kind, "IndexError" | "StopIteration")
}

// ---------------------------------------------------------------------------------------------
// Puxar item a item pelo laço de quadros
// ---------------------------------------------------------------------------------------------
//
// Quando uma cadeia de `enumerate`, `zip`, `map` e `filter` termina num gerador, ou roda Python no meio (a
// função do `map`, o predicado do `filter`, o `__next__` e o `__getitem__` de uma classe de usuário), o laço
// de quadros não chama o `iter_next` dela (que recursaria no Python): ele empilha o quadro e guarda as camadas
// já percorridas em `generator::Layer`. As funções abaixo são o que cada camada faz com a fonte e com o
// item; a conta em si fica em `Vm::drive`.

/// Os iteradores preguiçosos que o laço de quadros sabe puxar.
enum Parts<'a> {
    Map(&'a MapIter),
    Filter(&'a FilterIter),
    Zip(&'a ZipIter),
    Enumerate(&'a EnumerateIter),
}

fn parts(obj: &dyn ExtObject) -> Option<Parts<'_>> {
    let any = obj.as_any()?;
    any.downcast_ref::<MapIter>()
        .map(Parts::Map)
        .or_else(|| any.downcast_ref::<FilterIter>().map(Parts::Filter))
        .or_else(|| any.downcast_ref::<ZipIter>().map(Parts::Zip))
        .or_else(|| any.downcast_ref::<EnumerateIter>().map(Parts::Enumerate))
}

fn not_pullable() -> crate::vm::PyException {
    internal("not a lazy iterator the loop can pull")
}

/// Aplica `f` à fonte de índice `idx` de `obj`; `None` se `obj` não é um iterador preguiçoso destes ou não
/// tem essa fonte.
fn with_source<R>(obj: &dyn ExtObject, idx: usize, f: impl FnOnce(&mut PyIter) -> R) -> Option<R> {
    Some(match parts(obj)? {
        Parts::Map(m) => f(m.iters.borrow_mut().get_mut(idx)?),
        Parts::Zip(z) => f(z.iters.borrow_mut().get_mut(idx)?),
        Parts::Filter(x) if idx == 0 => f(&mut x.iter.borrow_mut()),
        Parts::Enumerate(e) if idx == 0 => f(&mut e.iter.borrow_mut()),
        Parts::Filter(_) | Parts::Enumerate(_) => return None,
    })
}

/// Quantas fontes `obj` tem; `None` se não é um iterador preguiçoso que o laço sabe puxar.
pub(crate) fn source_count(obj: &dyn ExtObject) -> Option<usize> {
    Some(match parts(obj)? {
        Parts::Map(m) => m.iters.borrow().len(),
        Parts::Zip(z) => z.iters.borrow().len(),
        Parts::Filter(_) | Parts::Enumerate(_) => 1,
    })
}

/// O próximo item da fonte `idx` de `obj`, direto (fonte que não roda Python em quadro).
pub(crate) fn source_next(obj: &dyn ExtObject, idx: usize) -> PyResult<Option<Value>> {
    with_source(obj, idx, PyIter::next).unwrap_or(Ok(None))
}

/// `func` roda código Python quando é chamada: função, método ligado a função, classe com `__init__` em Python
/// e instância com `__call__` em Python. É o que o laço de quadros executa em quadro (`Vm::enter_callable`),
/// em vez de a nativa chamar recursivamente.
pub(crate) fn runs_python(func: &Value) -> bool {
    match func {
        Value::Function(_) | Value::BoundFn(_) => true,
        Value::Class(c) => matches!(c.lookup("__init__"), Some(Value::Function(_))),
        Value::Instance(i) => matches!(i.class().lookup("__call__"), Some(Value::Function(_))),
        _ => false,
    }
}

/// `obj` é um `map` ou `filter` cuja função roda Python.
fn runs_callback(obj: &dyn ExtObject) -> bool {
    match parts(obj) {
        Some(Parts::Map(m)) => runs_python(&m.func),
        Some(Parts::Filter(f)) => runs_python(&f.func),
        _ => false,
    }
}

/// Puxar um item de `obj` precisa de um quadro: `obj` é um gerador, uma fonte folha que roda Python
/// ([`leaf`]) ou uma cadeia de `enumerate`/`zip`/`map`/`filter` com um callback Python ou uma fonte dessas.
pub(crate) fn needs_frames(obj: &Rc<dyn ExtObject>) -> bool {
    if let Some(core) = crate::generator::core_of(obj) {
        return core.kind() == crate::generator::Kind::Generator;
    }
    if leaf(&**obj).is_some() {
        return true;
    }
    let Some(count) = source_count(&**obj) else { return false };
    runs_callback(&**obj)
        || (0..count).any(|i| {
            with_source(&**obj, i, |it| match it {
                PyIter::Ext(e) => needs_frames(e),
                PyIter::Inst(v) => crate::vm::dunder_function(v, "__next__").is_some(),
                _ => false,
            })
            .unwrap_or(false)
        })
}

/// A fonte `idx` de `obj` que precisa de quadro, como objeto: o gerador, a cadeia ou a fonte folha; um iterador
/// de usuário (`PyIter::Inst` com `__next__` em Python) ganha um [`IterBox`] (a instância guarda o estado).
pub(crate) fn frame_source(obj: &dyn ExtObject, idx: usize) -> Option<Rc<dyn ExtObject>> {
    with_source(obj, idx, |it| match it {
        PyIter::Ext(e) => needs_frames(e).then(|| e.clone()),
        PyIter::Inst(v) => crate::vm::dunder_function(v, "__next__")
            .map(|_| Rc::new(IterBox { iter: RefCell::new(PyIter::Inst(v.clone())) }) as Rc<dyn ExtObject>),
        _ => None,
    })
    .flatten()
}

/// O que puxar um item de uma fonte folha pede ao laço de quadros.
pub(crate) enum Leaf {
    /// A fonte já esgotou (protocolo antigo de sequência).
    Ended,
    /// O método em Python a chamar e os argumentos dele.
    Call(Rc<FuncObj>, Vec<Value>),
}

/// A fonte folha que `obj` é: o `__next__` em Python de um iterador de usuário ([`IterBox`]) ou o `__getitem__`
/// em Python do protocolo antigo de sequência ([`OldSeqIter`]). `None` para qualquer outro objeto, e para
/// os que chamam código nativo.
pub(crate) fn leaf(obj: &dyn ExtObject) -> Option<Leaf> {
    let any = obj.as_any()?;
    if let Some(boxed) = any.downcast_ref::<IterBox>() {
        let it = boxed.iter.borrow();
        let PyIter::Inst(inst) = &*it else { return None };
        let next = crate::vm::dunder_function(inst, "__next__")?;
        return Some(Leaf::Call(next, vec![inst.clone()]));
    }
    if let Some(calls) = any.downcast_ref::<CallIter>() {
        if calls.done.get() {
            return Some(Leaf::Ended);
        }
        // `iter(função, sentinela)`: a função em Python roda em quadro (uma thread que bloqueia dentro dela, como o
        // `iter(taskqueue.get, None)` do `Pool`, precisa do laço mais externo para ceder a vez).
        return match &calls.func {
            Value::Function(f) => Some(Leaf::Call(f.clone(), Vec::new())),
            Value::BoundFn(b) => Some(Leaf::Call(b.1.clone(), vec![b.0.clone()])),
            _ => None,
        };
    }
    let seq = any.downcast_ref::<OldSeqIter>()?;
    if seq.done.get() {
        return Some(Leaf::Ended);
    }
    let item = crate::vm::dunder_function(&seq.obj, "__getitem__")?;
    Some(Leaf::Call(item, vec![seq.obj.clone(), Value::Int(seq.index.get())]))
}

/// O quadro da fonte folha `obj` terminou com `e`: o fim da fonte (`StopIteration`; no protocolo antigo
/// também `IndexError`) em vez de um erro.
pub(crate) fn leaf_stops(obj: &dyn ExtObject, e: &PyException) -> bool {
    match obj.as_any().and_then(|any| any.downcast_ref::<OldSeqIter>()) {
        Some(_) => ends_old_sequence(e),
        None => e.kind == "StopIteration",
    }
}

/// Aplica ao estado da fonte folha `obj` o desfecho do quadro: o item entregue (`Some`) ou o fim (`None`).
pub(crate) fn leaf_settle(obj: &dyn ExtObject, got: Option<Value>) -> Option<Value> {
    let any = obj.as_any();
    if let Some(seq) = any.and_then(|any| any.downcast_ref::<OldSeqIter>()) {
        match &got {
            Some(_) => seq.index.set(seq.index.get() + 1),
            None => seq.done.set(true),
        }
    }
    if let Some(calls) = any.and_then(|any| any.downcast_ref::<CallIter>()) {
        // O valor igual à sentinela (ou o `StopIteration` da função) esgota o iterador de vez.
        if got.as_ref().map_or(true, |v| crate::object::py_eq(v, &calls.sentinel)) {
            calls.done.set(true);
            return None;
        }
    }
    got
}

/// A camada de `obj` para o laço de quadros, na primeira fonte.
pub(crate) fn layer_for(obj: &Rc<dyn ExtObject>) -> Option<crate::generator::Layer> {
    use crate::generator::Layer;
    Some(match parts(&**obj)? {
        Parts::Enumerate(_) => Layer::Enumerate(obj.clone()),
        Parts::Filter(_) => Layer::Filter(obj.clone()),
        Parts::Map(_) | Parts::Zip(_) => Layer::Many { it: obj.clone(), at: 0, got: Vec::new() },
    })
}

/// O item de um `enumerate` que acabou de receber `item` da fonte: o par com o índice.
pub(crate) fn enumerate_item(obj: &dyn ExtObject, item: Value) -> PyResult<Value> {
    match parts(obj) {
        Some(Parts::Enumerate(e)) => Ok(e.number(item)),
        _ => Err(not_pullable()),
    }
}

/// O que o `filter` faz com um item que acabou de receber da fonte: já sabe se fica, ou precisa chamar o
/// predicado (`func`) em quadro.
pub(crate) enum Verdict {
    Keep(bool),
    Call(Value),
}

/// O veredito do `filter` `obj` sobre `item`: `None` como função é a verdade do próprio item; predicado em
/// Python vira um quadro; o resto é chamado na hora.
pub(crate) fn filter_verdict(obj: &dyn ExtObject, item: &Value) -> PyResult<Verdict> {
    match parts(obj) {
        Some(Parts::Filter(f)) => Ok(match &f.func {
            Value::None => Verdict::Keep(item.is_true()),
            func if runs_python(func) => Verdict::Call(func.clone()),
            func => Verdict::Keep(call(func, vec![item.clone()])?.is_true()),
        }),
        _ => Err(not_pullable()),
    }
}

/// O item de um `map` ou de um `zip` com o que cada fonte entregou: a tupla do `zip`, o valor da função do
/// `map` ou, quando ela roda Python, a função e os argumentos para o laço chamar em quadro.
pub(crate) enum Joined {
    Item(Value),
    Call(Value, Vec<Value>),
}

pub(crate) fn join_sources(obj: &dyn ExtObject, got: Vec<Value>) -> PyResult<Joined> {
    match parts(obj) {
        Some(Parts::Map(m)) if runs_python(&m.func) => Ok(Joined::Call(m.func.clone(), got)),
        Some(Parts::Map(m)) => call(&m.func, got).map(Joined::Item),
        Some(Parts::Zip(_)) => Ok(Joined::Item(Value::tuple(got))),
        _ => Err(not_pullable()),
    }
}

/// O que a camada de `zip`/`map` faz quando a fonte `at` acabou.
pub(crate) enum ZipEnd {
    /// O iterador acaba junto.
    Ends,
    /// `zip(strict=True)` com a primeira fonte esgotada: as demais precisam estar esgotadas também
    /// (`Layer::Check`).
    Check,
}

/// A fonte `at` de `obj` acabou: um `zip(strict=True)` levanta `ValueError` se `at` não é a primeira (ela é
/// mais curta que as anteriores) ou manda conferir as demais; `map` e `zip` comuns só terminam.
pub(crate) fn zip_ended(obj: &dyn ExtObject, at: usize) -> PyResult<ZipEnd> {
    match parts(obj) {
        Some(Parts::Zip(z)) if z.strict && at > 0 => Err(zip_mismatch(at, "shorter")),
        Some(Parts::Zip(z)) if z.strict && z.iters.borrow().len() > 1 => Ok(ZipEnd::Check),
        _ => Ok(ZipEnd::Ends),
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
        object_repr(self.kind, self)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__length_hint__", "__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::Lazy(LazyParts::Reversed { kind: self.kind, items: self.items.clone(), next: self.next.get() }))
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
