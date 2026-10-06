//! Objetos de traceback, quadro e código que `e.__traceback__` e o módulo `traceback` enxergam.
//! São só leitura: cada entrada guarda a linha e o nome do código registrados quando a exceção
//! atravessou o quadro, com o nome do arquivo de quem a capturou.

use std::rc::Rc;

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{exc, PyException, PyResult, Vm};

/// Uma entrada: linha e nome do código, do quadro mais externo para o mais interno.
pub type Entries = Rc<Vec<(usize, String, Rc<str>)>>;

pub struct TracebackObj {
    entries: Entries,
    idx: usize,
    filename: Rc<str>,
    /// `tb.tb_next = ...` (o `unittest` corta tracebacks): vale no lugar do próximo quadro.
    next: std::cell::RefCell<Option<Value>>,
}

impl TracebackObj {
    /// O traceback que começa no quadro mais externo de `entries`; `None` se estiver vazio.
    pub fn make(entries: Vec<(usize, String, Rc<str>)>, filename: &str) -> Value {
        if entries.is_empty() {
            return Value::None;
        }
        Value::Ext(Rc::new(TracebackObj { entries: Rc::new(entries), idx: 0, filename: Rc::from(filename), next: Default::default() }))
    }
}

impl ExtObject for TracebackObj {
    fn type_name(&self) -> &'static str {
        "traceback"
    }

    fn repr(&self) -> String {
        format!("<traceback object at {:p}>", self)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let (line, code, own) = &self.entries[self.idx];
        let file = if own.is_empty() { self.filename.clone() } else { own.clone() };
        Some(Ok(match name {
            "tb_lineno" => Value::Int(*line as i64),
            "tb_lasti" => Value::Int(0),
            "tb_frame" => Value::Ext(Rc::new(FrameObj {
                chain: Rc::new(vec![(*line, code.clone(), file)]),
                idx: 0,
            })),
            "tb_next" if self.next.borrow().is_some() => self.next.borrow().clone().unwrap_or(Value::None),
            "tb_next" => {
                if self.idx + 1 < self.entries.len() {
                    Value::Ext(Rc::new(TracebackObj {
                        entries: self.entries.clone(),
                        idx: self.idx + 1,
                        filename: self.filename.clone(),
                        next: Default::default(),
                    }))
                } else {
                    Value::None
                }
            }
            _ => return None,
        }))
    }

    fn setattr(&self, name: &str, value: Value) -> Option<Result<(), PyException>> {
        if name == "tb_next" {
            *self.next.borrow_mut() = Some(value);
            return Some(Ok(()));
        }
        None
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        Err(exc("AttributeError", format!("'traceback' object has no attribute '{name}'")))
    }
}

/// Quadros do mais interno para o mais externo: linha, nome do código e arquivo de cada um.
type FrameChain = Rc<Vec<(usize, String, Rc<str>)>>;

struct FrameObj {
    chain: FrameChain,
    idx: usize,
}

/// O quadro `depth` níveis acima do mais interno de `chain` (0 é o mais interno).
pub fn frame_at(chain: Vec<(usize, String, Rc<str>)>, depth: usize) -> Option<Value> {
    if depth >= chain.len() {
        return None;
    }
    Some(Value::Ext(Rc::new(FrameObj { chain: Rc::new(chain), idx: depth })))
}

/// Um objeto `code` solto (`função.__code__`).
pub fn code_object(name: &str, filename: &str) -> Value {
    Value::Ext(Rc::new(CodeObject { name: name.to_string(), filename: Rc::from(filename) }))
}

impl ExtObject for FrameObj {
    fn type_name(&self) -> &'static str {
        "frame"
    }

    fn repr(&self) -> String {
        let (line, name, filename) = &self.chain[self.idx];
        format!("<frame at {:p}, file '{}', line {}, code {}>", self, filename, line, name)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let (line, code, filename) = &self.chain[self.idx];
        Some(Ok(match name {
            "f_lineno" => Value::Int(*line as i64),
            "f_code" => Value::Ext(Rc::new(CodeObject { name: code.clone(), filename: filename.clone() })),
            "f_back" => {
                if self.idx + 1 < self.chain.len() {
                    Value::Ext(Rc::new(FrameObj { chain: self.chain.clone(), idx: self.idx + 1 }))
                } else {
                    Value::None
                }
            }
            "f_globals" | "f_locals" | "f_builtins" => Value::dict(crate::object::Dict::new()),
            _ => return None,
        }))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        Err(exc("AttributeError", format!("'frame' object has no attribute '{name}'")))
    }
}

struct CodeObject {
    name: String,
    filename: Rc<str>,
}

impl ExtObject for CodeObject {
    fn type_name(&self) -> &'static str {
        "code"
    }

    fn repr(&self) -> String {
        format!("<code object {} at {:p}, file \"{}\">", self.name, self, self.filename)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        Some(Ok(match name {
            "co_name" | "co_qualname" => Value::str(self.name.clone()),
            "co_filename" => Value::str(self.filename.to_string()),
            "co_firstlineno" => Value::Int(1),
            _ => return None,
        }))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        Err(exc("AttributeError", format!("'code' object has no attribute '{name}'")))
    }
}
