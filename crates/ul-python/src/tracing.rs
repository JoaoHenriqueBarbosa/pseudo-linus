//! `sys.settrace`: os eventos `call`, `line`, `return` e `exception` do CPython para funções escritas
//! em Python. A função global recebe o `call`; o que ela devolve vira o rastreador local daquele
//! quadro, que recebe os demais eventos. Enquanto um rastreador roda, nada é rastreado (como no
//! CPython). Com o rastreio desligado, o custo na VM é a leitura de um `Cell<bool>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::compile::Code;
use crate::object::Value;
use crate::vm::{PyResult, Vm};

struct TraceFrame {
    code: Rc<Code>,
    local: Option<Value>,
    last_line: usize,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static GLOBAL: RefCell<Option<Value>> = const { RefCell::new(None) };
    static FRAMES: RefCell<Vec<TraceFrame>> = const { RefCell::new(Vec::new()) };
}

/// `sys.settrace(f)`.
pub fn set(f: Option<Value>) {
    ACTIVE.with(|a| a.set(f.is_some()));
    GLOBAL.with(|g| *g.borrow_mut() = f);
}

/// `sys.gettrace()`.
pub fn get() -> Value {
    GLOBAL.with(|g| g.borrow().clone()).unwrap_or(Value::None)
}

#[inline]
pub fn active() -> bool {
    ACTIVE.with(Cell::get) && !INSIDE.with(Cell::get)
}

/// Chama o rastreador `f` com `(quadro, evento, arg)`; devolve o que ele devolveu.
fn invoke(vm: &mut Vm, f: &Value, event: &str, arg: Value) -> PyResult<Value> {
    let frame = crate::modules::pysys::current_frame(vm)?;
    INSIDE.with(|i| i.set(true));
    let r = vm.call_value(f, vec![frame, Value::str(event), arg], Vec::new());
    INSIDE.with(|i| i.set(false));
    if r.is_err() {
        // Como o CPython: rastreador que levanta é desligado.
        set(None);
    }
    r
}

/// Entrada numa função Python: dispara `call`. Devolve `true` se um quadro foi empilhado (o
/// chamador deve chamar [`leave`]).
pub fn enter(vm: &mut Vm, code: &Rc<Code>) -> PyResult<bool> {
    if !active() {
        return Ok(false);
    }
    let Some(global) = GLOBAL.with(|g| g.borrow().clone()) else { return Ok(false) };
    let first = if code.first_line > 0 { code.first_line } else { code.lines.first().copied().unwrap_or(0) };
    vm.cur_line.set(first);
    let local = invoke(vm, &global, "call", Value::None)?;
    let local = (!matches!(local, Value::None)).then_some(local);
    FRAMES.with(|f| f.borrow_mut().push(TraceFrame { code: code.clone(), local, last_line: 0 }));
    Ok(true)
}

/// Uma instrução da função `code` na linha `line`: dispara `line` quando a linha muda.
pub fn line(vm: &mut Vm, code: &Rc<Code>, line: usize) -> PyResult<()> {
    if INSIDE.with(Cell::get) {
        return Ok(());
    }
    let tracer = FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let top = f.last_mut()?;
        if !Rc::ptr_eq(&top.code, code) || top.last_line == line {
            return None;
        }
        top.last_line = line;
        top.local.clone()
    });
    if let Some(t) = tracer {
        let next = invoke(vm, &t, "line", Value::None)?;
        FRAMES.with(|f| {
            if let Some(top) = f.borrow_mut().last_mut() {
                top.local = (!matches!(next, Value::None)).then_some(next);
            }
        });
    }
    Ok(())
}

/// Erro numa instrução da função `code` (capturado ou não): dispara `exception` no quadro dela.
pub fn exception(vm: &mut Vm, code: &Rc<Code>, e: &crate::vm::PyException) -> PyResult<()> {
    if INSIDE.with(Cell::get) {
        return Ok(());
    }
    let tracer = FRAMES.with(|f| {
        let f = f.borrow();
        let top = f.last()?;
        if !Rc::ptr_eq(&top.code, code) {
            return None;
        }
        top.local.clone()
    });
    let Some(t) = tracer else { return Ok(()) };
    let exc = e.to_value();
    let kind = vm.getattr(&exc, "__class__").unwrap_or(Value::None);
    let tb = vm.getattr(&exc, "__traceback__").unwrap_or(Value::None);
    let next = invoke(vm, &t, "exception", Value::tuple(vec![kind, exc, tb]))?;
    FRAMES.with(|f| {
        if let Some(top) = f.borrow_mut().last_mut() {
            top.local = (!matches!(next, Value::None)).then_some(next);
        }
    });
    Ok(())
}

/// Saída da função: `return` (com `None` se saiu por erro), depois desempilha.
pub fn leave(vm: &mut Vm, result: &PyResult<Value>) -> PyResult<()> {
    let tracer = FRAMES.with(|f| f.borrow().last().and_then(|t| t.local.clone()));
    let out = match tracer {
        Some(t) => {
            let v = result.as_ref().map_or(Value::None, Clone::clone);
            invoke(vm, &t, "return", v).map(|_| ())
        }
        None => Ok(()),
    };
    FRAMES.with(|f| f.borrow_mut().pop());
    out
}
