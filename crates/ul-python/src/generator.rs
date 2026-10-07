//! Geradores, correntes (coroutines) e geradores assíncronos: uma função com `yield` ou `async def`
//! vira um objeto que guarda o quadro (pilha, blocos protegidos e `pc`) entre uma retomada e a seguinte.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::compile::Code;
use crate::object::{Env, ExcObj, ExtObject, Kw, Value};
use crate::vm::{exc, type_error, Block, Exit, PyException, PyResult, Slot, Vm};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Generator,
    Coroutine,
    AsyncGenerator,
}

struct GenState {
    stack: Vec<Slot>,
    blocks: Vec<Block>,
    pc: usize,
    started: bool,
    done: bool,
    running: bool,
    /// As exceções em tratamento dentro do gerador quando ele se suspendeu, e a altura da pilha
    /// de fora naquele momento (o CPython guarda o `exc_info` no próprio gerador).
    handled: Vec<Value>,
    handled_base: usize,
}

/// O que uma retomada produziu: um valor entregue (`yield`/suspensão) ou o fim com o valor de retorno.
pub enum Resumed {
    Yield(Value),
    Return(Value),
}

struct GenCore {
    vm: Vm,
    code: Rc<Code>,
    env: Rc<Env>,
    kind: Kind,
    state: RefCell<GenState>,
}

pub struct GenObj {
    core: Rc<GenCore>,
}

/// Cria o objeto de uma chamada de função com `yield` ou `async def`; o corpo só roda na primeira
/// retomada.
pub fn new_generator(vm: Vm, code: Rc<Code>, env: Rc<Env>) -> Value {
    let kind = match (code.is_async, code.is_generator) {
        (true, true) => Kind::AsyncGenerator,
        (true, false) => Kind::Coroutine,
        _ => Kind::Generator,
    };
    Value::Ext(Rc::new(GenObj {
        core: Rc::new(GenCore {
            vm,
            code,
            env,
            kind,
            state: RefCell::new(GenState {
                stack: Vec::new(),
                blocks: Vec::new(),
                pc: 0,
                started: false,
                done: false,
                running: false,
                handled: Vec::new(),
                handled_base: 0,
            }),
        }),
    }))
}

/// `StopIteration(valor)`: o `return valor` de um gerador ou corrente.
pub fn stop_iteration(value: Value) -> PyException {
    let args = if matches!(value, Value::None) { Vec::new() } else { vec![value] };
    PyException::from_value(&Value::Exception(Rc::new(ExcObj::new("StopIteration", args))))
}

/// O valor carregado por um `StopIteration` (`None` se não houver).
pub fn stop_value(e: &PyException) -> Value {
    match &e.value {
        Some(Value::Exception(x)) => x.args.first().cloned().unwrap_or(Value::None),
        Some(Value::Instance(i)) => match i.dict.borrow().get("args") {
            Some(Value::Tuple(t)) => t.first().cloned().unwrap_or(Value::None),
            _ => Value::None,
        },
        _ => Value::None,
    }
}

/// Valor entregue por um `yield` de gerador assíncrono, para o distinguir de um `await` suspenso.
struct AsyncGenWrapped(Value);

impl ExtObject for AsyncGenWrapped {
    fn type_name(&self) -> &'static str {
        "async_generator_wrapped_value"
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        (name == "value").then(|| Ok(self.0.clone()))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("object has no attribute '{name}'")))
    }
}

pub fn wrap_async_value(v: Value) -> Value {
    Value::Ext(Rc::new(AsyncGenWrapped(v)))
}

fn unwrap_async_value(vm: &mut Vm, v: &Value) -> Option<Value> {
    match v {
        Value::Ext(e) if e.type_name() == "async_generator_wrapped_value" => e.getattr(vm, "value").and_then(Result::ok),
        _ => None,
    }
}

impl GenCore {
    /// Retoma o quadro; com `inject`, a exceção é levantada no ponto em que ele parou.
    fn resume(&self, sent: Option<Value>, inject: Option<PyException>) -> PyResult<Resumed> {
        let what = match self.kind {
            Kind::Generator => "generator",
            Kind::Coroutine => "coroutine",
            Kind::AsyncGenerator => "async generator",
        };
        let (mut stack, mut blocks, mut pc) = {
            let mut st = self.state.borrow_mut();
            if st.running {
                return Err(exc("ValueError", format!("{what} already executing")));
            }
            if st.done {
                if self.kind == Kind::Coroutine && inject.is_none() {
                    return Err(exc("RuntimeError", "cannot reuse already awaited coroutine"));
                }
                return match inject {
                    Some(e) => Err(e),
                    None => Ok(Resumed::Return(Value::None)),
                };
            }
            if !st.started {
                if let Some(e) = inject {
                    st.done = true;
                    return Err(e);
                }
                if sent.is_some_and(|v| !matches!(v, Value::None)) {
                    return Err(type_error(format!("can't send non-None value to a just-started {what}")));
                }
            } else if inject.is_none() {
                st.stack.push(Slot::Val(sent.unwrap_or(Value::None)));
            }
            st.started = true;
            st.running = true;
            // Os blocos guardam a altura absoluta da pilha: retomado sob outra altura, rebaseia.
            let base = self.vm.handled_len();
            let old = st.handled_base;
            if base != old {
                for b in st.blocks.iter_mut() {
                    b.handled = (b.handled + base).saturating_sub(old);
                }
            }
            st.handled_base = base;
            self.vm.handled_extend(std::mem::take(&mut st.handled));
            (std::mem::take(&mut st.stack), std::mem::take(&mut st.blocks), st.pc)
        };
        let mut vm = self.vm.clone();
        let base = vm.handled_len().min(self.state.borrow().handled_base);
        let result = vm.run_loop(&self.code, &self.env, &mut stack, &mut blocks, &mut pc, inject);
        let inner = vm.handled_split(base);
        let mut st = self.state.borrow_mut();
        st.running = false;
        match result {
            Ok(Exit::Yield(v)) => {
                st.stack = stack;
                st.blocks = blocks;
                st.pc = pc;
                st.handled = inner;
                Ok(Resumed::Yield(v))
            }
            Ok(Exit::Return(v)) => {
                st.done = true;
                Ok(Resumed::Return(v))
            }
            Err(e) => {
                st.done = true;
                Err(e)
            }
        }
    }

    fn close(&self) {
        let mut st = self.state.borrow_mut();
        st.done = true;
        st.stack.clear();
        st.blocks.clear();
    }
}

impl GenObj {
    fn type_str(&self) -> &'static str {
        match self.core.kind {
            Kind::Generator => "generator",
            Kind::Coroutine => "coroutine",
            Kind::AsyncGenerator => "async_generator",
        }
    }

    /// `send`/`throw` de gerador e corrente: o valor entregue, ou `StopIteration(retorno)`.
    fn step(&self, sent: Option<Value>, inject: Option<PyException>) -> PyResult<Value> {
        match self.core.resume(sent, inject)? {
            Resumed::Yield(v) => Ok(v),
            Resumed::Return(v) => Err(stop_iteration(v)),
        }
    }

    fn awaitable(&self, mode: AGMode) -> Value {
        Value::Ext(Rc::new(AGAwait { core: self.core.clone(), mode: RefCell::new(Some(mode)), started: Cell::new(false), done: Cell::new(false), closing: Cell::new(false) }))
    }
}

impl ExtObject for GenObj {
    fn type_name(&self) -> &'static str {
        self.type_str()
    }
    fn repr(&self) -> String {
        format!(
            "<{} object {} at {:#x}>",
            self.type_str().replace('_', " "),
            self.core.code.name,
            crate::object::py_addr(self as *const GenObj as usize)
        )
    }
    fn methods(&self) -> &'static [&'static str] {
        match self.core.kind {
            Kind::Generator => &["send", "throw", "close", "__next__"],
            Kind::Coroutine => &["send", "throw", "close", "__await__"],
            Kind::AsyncGenerator => &["__aiter__", "__anext__", "asend", "athrow", "aclose"],
        }
    }
    fn is_iterable(&self) -> bool {
        self.core.kind == Kind::Generator
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        match self.core.resume(None, None)? {
            Resumed::Yield(v) => Ok(Some(v)),
            Resumed::Return(_) => Ok(None),
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "send" => {
                let [v] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("send() takes exactly one argument ({} given)", a.len())))?;
                self.step(Some(v), None)
            }
            "throw" => {
                let Some(first) = args.into_iter().next() else {
                    return Err(type_error("throw expected at least 1 argument, got 0"));
                };
                let e = raise_for_throw(vm, first)?;
                self.step(None, Some(e))
            }
            "__next__" => self.step(None, None),
            "__await__" => Ok(Value::Ext(Rc::new(CoroWrapper { core: self.core.clone() }))),
            "close" => {
                self.core.close();
                Ok(Value::None)
            }
            "__aiter__" => Err(type_error("__aiter__ returns self")),
            "__anext__" => Ok(self.awaitable(AGMode::Send(Value::None))),
            "asend" => {
                let [v] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("asend() takes exactly one argument ({} given)", a.len())))?;
                Ok(self.awaitable(AGMode::Send(v)))
            }
            "athrow" => {
                let Some(first) = args.into_iter().next() else {
                    return Err(type_error("athrow expected at least 1 argument, got 0"));
                };
                let e = raise_for_throw(vm, first)?;
                Ok(self.awaitable(AGMode::Throw(e)))
            }
            "aclose" => Ok(self.awaitable(AGMode::Close)),
            _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", self.type_str()))),
        }
    }
}

/// O iterador devolvido por `coroutine.__await__()`: repassa `send`/`throw` à corrente.
struct CoroWrapper {
    core: Rc<GenCore>,
}

impl ExtObject for CoroWrapper {
    fn type_name(&self) -> &'static str {
        "coroutine_wrapper"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["send", "throw", "close", "__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        match self.core.resume(None, None)? {
            Resumed::Yield(v) => Ok(Some(v)),
            Resumed::Return(_) => Ok(None),
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let step = |sent, inject| match self.core.resume(sent, inject)? {
            Resumed::Yield(v) => Ok(v),
            Resumed::Return(v) => Err(stop_iteration(v)),
        };
        match name {
            "send" => step(args.into_iter().next(), None),
            "__next__" => step(None, None),
            "throw" => {
                let first = args.into_iter().next().ok_or_else(|| type_error("throw expected at least 1 argument, got 0"))?;
                let e = raise_for_throw(vm, first)?;
                step(None, Some(e))
            }
            "close" => {
                self.core.close();
                Ok(Value::None)
            }
            _ => Err(exc("AttributeError", format!("'coroutine_wrapper' object has no attribute '{name}'"))),
        }
    }
}

/// `raise_any` para o `throw`: a exceção injetada carrega o traceback que já tinha (`__traceback__`),
/// para os quadros de quem a levantou continuarem aparecendo depois de atravessar o gerador.
fn raise_for_throw(vm: &mut Vm, v: Value) -> PyResult<PyException> {
    let mut e = vm.raise_any(v)?;
    e.seed_traceback();
    Ok(e)
}

enum AGMode {
    Send(Value),
    Throw(PyException),
    Close,
}

/// O aguardável de `agen.__anext__()`/`asend`/`athrow`/`aclose`: o `await` dele retoma o gerador
/// assíncrono; um `yield` do corpo termina o `await` com esse valor, um `await` do corpo suspende.
struct AGAwait {
    core: Rc<GenCore>,
    mode: RefCell<Option<AGMode>>,
    started: Cell<bool>,
    done: Cell<bool>,
    closing: Cell<bool>,
}

impl AGAwait {
    fn finish(&self, r: PyResult<Resumed>, vm: &mut Vm, closing: bool) -> PyResult<Value> {
        match r {
            Ok(Resumed::Yield(v)) => match unwrap_async_value(vm, &v) {
                Some(_) if closing => {
                    self.done.set(true);
                    Err(exc("RuntimeError", "async generator ignored GeneratorExit"))
                }
                Some(inner) => {
                    self.done.set(true);
                    Err(stop_iteration(inner))
                }
                None => Ok(v),
            },
            Ok(Resumed::Return(_)) => {
                self.done.set(true);
                Err(if closing { stop_iteration(Value::None) } else { exc("StopAsyncIteration", "") })
            }
            Err(e) if closing && matches!(e.kind, "GeneratorExit" | "StopAsyncIteration") => {
                self.done.set(true);
                Err(stop_iteration(Value::None))
            }
            Err(e) => {
                self.done.set(true);
                Err(e)
            }
        }
    }

    fn advance(&self, vm: &mut Vm, arg: Option<Value>, inject: Option<PyException>) -> PyResult<Value> {
        if self.done.get() {
            return Err(exc("StopIteration", ""));
        }
        let first = !self.started.replace(true);
        let mode = if first { self.mode.borrow_mut().take() } else { None };
        if first {
            self.closing.set(matches!(mode, Some(AGMode::Close)));
        }
        let closing = self.closing.get();
        let r = match (mode, inject) {
            (_, Some(e)) => self.core.resume(None, Some(e)),
            (Some(AGMode::Send(v)), None) => self.core.resume(Some(v), None),
            (Some(AGMode::Throw(e)), None) => self.core.resume(None, Some(e)),
            (Some(AGMode::Close), None) => {
                let st = self.core.state.borrow();
                if st.done || !st.started {
                    drop(st);
                    self.core.close();
                    self.done.set(true);
                    return Err(stop_iteration(Value::None));
                }
                drop(st);
                self.core.resume(None, Some(exc("GeneratorExit", "")))
            }
            (None, None) => self.core.resume(arg, None),
        };
        self.finish(r, vm, closing)
    }
}

impl ExtObject for AGAwait {
    fn type_name(&self) -> &'static str {
        "async_generator_asend"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["send", "throw", "close", "__next__", "__await__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let mut vm = crate::vm::current().ok_or_else(|| exc("RuntimeError", "no running interpreter"))?;
        match self.advance(&mut vm, None, None) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind == "StopIteration" => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "send" => self.advance(vm, args.into_iter().next(), None),
            "__next__" => self.advance(vm, None, None),
            "throw" => {
                let first = args.into_iter().next().ok_or_else(|| type_error("throw expected at least 1 argument, got 0"))?;
                let e = raise_for_throw(vm, first)?;
                self.advance(vm, None, Some(e))
            }
            "close" => {
                self.done.set(true);
                Ok(Value::None)
            }
            "__await__" => Err(type_error("__await__ returns self")),
            _ => Err(exc("AttributeError", format!("'async_generator_asend' object has no attribute '{name}'"))),
        }
    }
}
