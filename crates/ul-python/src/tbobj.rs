//! Objetos de traceback, quadro e código que `e.__traceback__` e o módulo `traceback` enxergam.
//! São só leitura: cada entrada guarda a linha e o nome do código registrados quando a exceção
//! atravessou o quadro, com o nome do arquivo de quem a capturou.

use std::rc::Rc;

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{exc, PyException, PyResult, Vm};

/// Uma entrada: linha e nome do código, do quadro mais externo para o mais interno.
pub type Entries = Rc<Vec<crate::vm::TbEntry>>;

pub struct TracebackObj {
    entries: Entries,
    idx: usize,
    filename: Rc<str>,
    /// `tb.tb_next = ...` (o `unittest` corta tracebacks): vale no lugar do próximo quadro.
    next: std::cell::RefCell<Option<Value>>,
}

impl TracebackObj {
    /// O traceback que começa no quadro mais externo de `entries`; `None` se estiver vazio.
    pub fn make(entries: Vec<crate::vm::TbEntry>, filename: &str) -> Value {
        if entries.is_empty() {
            return Value::None;
        }
        Value::Ext(Rc::new(TracebackObj { entries: Rc::new(entries), idx: 0, filename: Rc::from(filename), next: Default::default() }))
    }
}

impl TracebackObj {
    /// Os quadros que este traceback cobre (do mais externo para o mais interno) e o arquivo padrão.
    pub fn frames(&self) -> (Vec<crate::vm::TbEntry>, Rc<str>) {
        let mut out = vec![self.entries[self.idx].clone()];
        match &*self.next.borrow() {
            // Cortado ou religado por `tb_next = ...`: segue o que foi atribuído.
            Some(Value::None) => {}
            Some(Value::Ext(n)) => {
                if let Some(t) = n.as_any().and_then(|a| a.downcast_ref::<TracebackObj>()) {
                    out.extend(t.frames().0);
                }
            }
            _ => out.extend(self.entries[self.idx + 1..].iter().cloned()),
        }
        (out, self.filename.clone())
    }
}

impl ExtObject for TracebackObj {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn type_name(&self) -> &'static str {
        "traceback"
    }

    fn repr(&self) -> String {
        format!("<traceback object at {:p}>", self)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let (line, code, own, span) = &self.entries[self.idx];
        let file = if own.is_empty() { self.filename.clone() } else { own.clone() };
        Some(Ok(match name {
            "tb_lineno" => Value::Int(*line as i64),
            "tb_lasti" => Value::Int(0),
            "_position" if span.lineno > 0 => Value::tuple(vec![
                Value::Int(span.lineno as i64),
                Value::Int(span.end_lineno as i64),
                Value::Int(span.col as i64),
                Value::Int(span.end_col as i64),
            ]),
            "_position" => Value::None,
            "tb_frame" => Value::Ext(Rc::new(FrameObj {
                chain: Rc::new(vec![(*line, code.clone(), file)]),
                idx: 0,
            })),
            "tb_next" if self.next.borrow().is_some() => self.next.borrow().clone().unwrap_or(Value::None),
            "tb_next" => {
                // O nó seguinte é criado uma vez e guardado: `tb.tb_next.tb_next = None` precisa valer depois.
                let next = if self.idx + 1 < self.entries.len() {
                    Value::Ext(Rc::new(TracebackObj {
                        entries: self.entries.clone(),
                        idx: self.idx + 1,
                        filename: self.filename.clone(),
                        next: Default::default(),
                    }))
                } else {
                    Value::None
                };
                *self.next.borrow_mut() = Some(next.clone());
                next
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
    Value::Ext(Rc::new(CodeObject { name: name.to_string(), filename: Rc::from(filename), code: None }))
}

/// `função.__code__` com os parâmetros e as marcas que `inspect` lê.
pub fn function_code(code: &Rc<crate::compile::Code>, filename: &str) -> Value {
    Value::Ext(Rc::new(CodeObject { name: code.name.clone(), filename: Rc::from(filename), code: Some(code.clone()) }))
}

impl ExtObject for FrameObj {
    fn type_name(&self) -> &'static str {
        "frame"
    }

    fn repr(&self) -> String {
        let (line, name, filename) = &self.chain[self.idx];
        format!("<frame at {:p}, file '{}', line {}, code {}>", self, filename, line, name)
    }

    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let (line, code, filename) = &self.chain[self.idx];
        Some(Ok(match name {
            "f_lineno" => Value::Int(*line as i64),
            "f_code" => Value::Ext(Rc::new(CodeObject { name: code.clone(), filename: filename.clone(), code: None })),
            "f_back" => {
                if self.idx + 1 < self.chain.len() {
                    Value::Ext(Rc::new(FrameObj { chain: self.chain.clone(), idx: self.idx + 1 }))
                } else {
                    Value::None
                }
            }
            "f_globals" => {
                // As globais do módulo cujo `__file__` é o do quadro (o script principal: as da VM).
                let found = vm.module_globals.borrow().values().find_map(|g| {
                    let matches = match g.borrow().get("__file__") {
                        Some(Value::Str(f)) => f.as_str() == &**filename,
                        _ => false,
                    };
                    matches.then(|| g.clone())
                });
                crate::globalsview::view_for(&found.unwrap_or_else(|| vm.globals.clone()), None)
            }
            "f_locals" | "f_builtins" => Value::dict(crate::object::Dict::new()),
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
    /// O código compilado, quando o objeto vem de uma função (`f.__code__`).
    code: Option<Rc<crate::compile::Code>>,
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
            "co_firstlineno" => Value::Int(self.code.as_ref().map_or(1, |c| c.first_line.max(1)) as i64),
            _ => {
                let c = self.code.as_ref()?;
                let strs = |v: Vec<Rc<str>>| Value::tuple(v.into_iter().map(|s| Value::str(&*s)).collect());
                match name {
                    "co_argcount" => Value::Int(c.params.len() as i64),
                    "co_posonlyargcount" => Value::Int(c.posonly as i64),
                    "co_kwonlyargcount" => Value::Int(c.kwonly.len() as i64),
                    "co_varnames" => {
                        let mut v: Vec<Rc<str>> = c.params.clone();
                        v.extend(c.kwonly.iter().cloned());
                        v.extend(c.vararg.iter().cloned());
                        v.extend(c.kwarg.iter().cloned());
                        strs(v)
                    }
                    "co_flags_varargs" => Value::Bool(c.vararg.is_some()),
                    // Globais e atributos usados pelo código, sem os locais (que estão em `co_varnames`).
                    "co_names" => {
                        let locals: Vec<&Rc<str>> = c.params.iter().chain(&c.kwonly).chain(&c.vararg).chain(&c.kwarg).collect();
                        let mut v: Vec<Rc<str>> = Vec::new();
                        for n in &c.names {
                            if !locals.contains(&n) && !v.contains(n) {
                                v.push(n.clone());
                            }
                        }
                        strs(v)
                    }
                    "co_flags_varkw" => Value::Bool(c.kwarg.is_some()),
                    // As constantes do código, com os corpos de funções e lambdas aninhados como `code`.
                    "co_consts" => {
                        let mut v: Vec<Value> = c.consts.clone();
                        for f in &c.functions {
                            v.push(Value::Ext(Rc::new(CodeObject {
                                name: f.name.clone(),
                                filename: self.filename.clone(),
                                code: Some(f.clone()),
                            })));
                        }
                        Value::tuple(v)
                    }
                    "co_kinds" => {
                        let mut k = Vec::new();
                        match (c.is_async, c.is_generator) {
                            (true, true) => k.push(Rc::from("asyncgen")),
                            (true, false) => k.push(Rc::from("coroutine")),
                            (false, true) => k.push(Rc::from("generator")),
                            _ => {}
                        }
                        strs(k)
                    }
                    _ => return None,
                }
            }
        }))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        Err(exc("AttributeError", format!("'code' object has no attribute '{name}'")))
    }
}
