//! Objetos de traceback, quadro e código que `e.__traceback__` e o módulo `traceback` enxergam.
//! São só leitura: cada entrada guarda a linha e o nome do código registrados quando a exceção
//! atravessou o quadro, com o nome do arquivo de quem a capturou.

use std::rc::Rc;

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{exc, PyException, PyResult, Vm};

/// Uma entrada: linha e nome do código, do quadro mais externo para o mais interno.
pub type Entries = Rc<Vec<(usize, String)>>;

pub struct TracebackObj {
    entries: Entries,
    idx: usize,
    filename: Rc<str>,
}

impl TracebackObj {
    /// O traceback que começa no quadro mais externo de `entries`; `None` se estiver vazio.
    pub fn make(entries: Vec<(usize, String)>, filename: &str) -> Value {
        if entries.is_empty() {
            return Value::None;
        }
        Value::Ext(Rc::new(TracebackObj { entries: Rc::new(entries), idx: 0, filename: Rc::from(filename) }))
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
        let (line, code) = &self.entries[self.idx];
        Some(Ok(match name {
            "tb_lineno" => Value::Int(*line as i64),
            "tb_lasti" => Value::Int(0),
            "tb_frame" => Value::Ext(Rc::new(FrameObj {
                line: *line,
                name: code.clone(),
                filename: self.filename.clone(),
            })),
            "tb_next" => {
                if self.idx + 1 < self.entries.len() {
                    Value::Ext(Rc::new(TracebackObj {
                        entries: self.entries.clone(),
                        idx: self.idx + 1,
                        filename: self.filename.clone(),
                    }))
                } else {
                    Value::None
                }
            }
            _ => return None,
        }))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        Err(exc("AttributeError", format!("'traceback' object has no attribute '{name}'")))
    }
}

struct FrameObj {
    line: usize,
    name: String,
    filename: Rc<str>,
}

impl ExtObject for FrameObj {
    fn type_name(&self) -> &'static str {
        "frame"
    }

    fn repr(&self) -> String {
        format!("<frame at {:p}, file '{}', line {}, code {}>", self, self.filename, self.line, self.name)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        Some(Ok(match name {
            "f_lineno" => Value::Int(self.line as i64),
            "f_code" => Value::Ext(Rc::new(CodeObject { name: self.name.clone(), filename: self.filename.clone() })),
            "f_back" => Value::None,
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
