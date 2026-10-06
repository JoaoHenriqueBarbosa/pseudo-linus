//! `sys.stdout.buffer`, `sys.stderr.buffer` e `sys.stdin.buffer`: a face binária dos fluxos padrão.
//!
//! A escrita entra no mesmo buffer que o texto (a ordem entre `print` e `buffer.write` se mantém) e
//! a leitura consome as mesmas linhas já carregadas do `sys.stdin`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{ExtObject, FileKind, Kw, Native, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

pub struct StdBuffer {
    pub file: Rc<RefCell<Native>>,
    pub kind: FileKind,
}

impl StdBuffer {
    pub fn value(file: &Rc<RefCell<Native>>, kind: FileKind) -> Value {
        Value::Ext(Rc::new(StdBuffer { file: file.clone(), kind }))
    }

    fn write_bytes(&self, vm: &mut Vm, data: &[u8]) -> PyResult<()> {
        match self.kind {
            FileKind::Stdout => vm.push_stdout(data),
            FileKind::Stderr => {
                let _ = sysabi::sys::write_all(sysabi::Fd::STDERR, data);
            }
            _ => return Err(exc("UnsupportedOperation", "write")),
        }
        Ok(())
    }

    /// Bytes exatos do stdin: `take` deles (bloqueia até juntar ou chegar ao fim do arquivo) ou o que falta.
    fn read_bytes(&self, take: Option<usize>, available_only: bool) -> PyResult<Vec<u8>> {
        if self.kind != FileKind::Stdin {
            return Err(exc("UnsupportedOperation", "read"));
        }
        let Native::File(f) = &mut *self.file.borrow_mut() else { return Ok(Vec::new()) };
        Ok(if available_only { crate::stdin::bytes_read1(f, take) } else { crate::stdin::bytes_read(f, take) })
    }

    fn read_line(&self) -> PyResult<Vec<u8>> {
        if self.kind != FileKind::Stdin {
            return Err(exc("UnsupportedOperation", "read"));
        }
        let Native::File(f) = &mut *self.file.borrow_mut() else { return Ok(Vec::new()) };
        Ok(crate::stdin::bytes_line(f))
    }
}

impl ExtObject for StdBuffer {
    fn type_name(&self) -> &'static str {
        if self.kind == FileKind::Stdin {
            "BufferedReader"
        } else {
            "BufferedWriter"
        }
    }

    fn repr(&self) -> String {
        let name = match self.kind {
            FileKind::Stdin => "<stdin>",
            FileKind::Stdout => "<stdout>",
            _ => "<stderr>",
        };
        format!("<_io.{} name='{name}'>", self.type_name())
    }

    fn methods(&self) -> &'static [&'static str] {
        &[
            "write", "writelines", "flush", "read", "read1", "readline", "readlines", "fileno", "isatty", "readable",
            "writable", "seekable", "close",
        ]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "closed" => Some(Ok(Value::Bool(false))),
            "name" => Some(Ok(Value::str(match self.kind {
                FileKind::Stdin => "<stdin>",
                FileKind::Stdout => "<stdout>",
                _ => "<stderr>",
            }))),
            "mode" => Some(Ok(Value::str(if self.kind == FileKind::Stdin { "rb" } else { "wb" }))),
            _ => None,
        }
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let bytes_arg = |v: &Value| {
            v.bytes_like()
                .ok_or_else(|| type_error(format!("a bytes-like object is required, not '{}'", v.type_name())))
        };
        match name {
            "write" => {
                let data = bytes_arg(args.first().ok_or_else(|| type_error("write() takes exactly one argument (0 given)"))?)?;
                self.write_bytes(vm, &data)?;
                Ok(Value::Int(data.len() as i64))
            }
            "writelines" => {
                for item in iterate(args.first().ok_or_else(|| type_error("writelines() takes exactly one argument (0 given)"))?)? {
                    let data = bytes_arg(&item)?;
                    self.write_bytes(vm, &data)?;
                }
                Ok(Value::None)
            }
            "flush" => {
                if matches!(self.kind, FileKind::Stdout) {
                    vm.flush_stdout();
                }
                Ok(Value::None)
            }
            "close" => Ok(Value::None),
            "read" | "read1" => {
                let take = match args.first() {
                    Some(Value::Int(n)) if *n >= 0 => Some(*n as usize),
                    _ => None,
                };
                Ok(Value::bytes(self.read_bytes(take, name == "read1")?))
            }
            "readline" => Ok(Value::bytes(self.read_line()?)),
            "readlines" => {
                let mut out = Vec::new();
                loop {
                    let l = self.read_line()?;
                    if l.is_empty() {
                        break;
                    }
                    out.push(Value::bytes(l));
                }
                Ok(Value::list(out))
            }
            "fileno" => Ok(Value::Int(match self.kind {
                FileKind::Stdin => 0,
                FileKind::Stdout => 1,
                _ => 2,
            })),
            "isatty" | "seekable" => Ok(Value::Bool(false)),
            "readable" => Ok(Value::Bool(self.kind == FileKind::Stdin)),
            "writable" => Ok(Value::Bool(self.kind != FileKind::Stdin)),
            _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", self.type_name()))),
        }
    }

    fn is_iterable(&self) -> bool {
        self.kind == FileKind::Stdin
    }

    fn iter_next(&self) -> PyResult<Option<Value>> {
        let l = self.read_line()?;
        Ok(if l.is_empty() { None } else { Some(Value::bytes(l)) })
    }
}
